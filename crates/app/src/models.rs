//! Modelos 303, 130, 111 y 115 calculados con los datos del almacén: borrador casilla a casilla
//! y fichero DR para importar en la Sede (o presentar en la Fase 6, siempre con confirmación).
//!
//! Origen de los datos:
//! - 303: facturas emitidas (no anuladas) por fecha de devengo (la de la operación si es
//!   anterior a la de expedición, art. 75 LIVA; ver [`vat_accrual_date`]) y recibidas revisadas
//!   por fecha de registro (como en los libros registro). Las rectificativas por sustitución se
//!   declaran por la diferencia con la factura rectificada.
//! - 130: ingresos (grupo 7) y gastos (grupo 6) del diario, acumulados hasta el fin del
//!   trimestre, sin asientos de regularización ni cierre; [06] = retenciones de las emitidas.
//! - 111: profesionales = recibidas con retención distinta del 19 %; trabajo = asientos de nómina
//!   (640 / 4751), perceptores por empleado.
//! - 115: recibidas con retención del 19 % (arrendamientos).
//! - 123 y 193: pagos de dividendos a socios residentes (`socios`); los de no residentes van
//!   al 216/296 y no se incluyen.
//! - 190: además de profesionales (G.01) y empleados (A), la retribución de administradores
//!   (clave E con la subclave de cada recibo) y las dietas exceptuadas (L.01, `viajes`).
//!
//! `TODO(VERIFY)`:
//! - Alquileres: se marcan al revisar la recibida; en las revisadas antes de existir la marca se
//!   detectan por la retención del 19 % (heurística: 19 % también es el tipo de capital
//!   mobiliario).

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::Origin;
use taxpain_core::invoice::{
    Invoice, InvoiceKind, PartyId, RectificationMethod, TaxBreakdown, Totals, VatTreatment,
};
use taxpain_core::{Money, Nif, Rate};
use taxpain_models::m100::{IrpfRegime, Reduction32};
use taxpain_models::m130::{self, Input130, Previous130, Result130};
use taxpain_models::m303::{self, IssuedVat, Options303, ReceivedKind, ReceivedVat, Result303};
use taxpain_models::withholding::{self, Input111, Payment, ResultW};
use taxpain_models::{Declarant, dr, periodo};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::OptionalExtension;
use time::{Date, Month, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::presentados::FiledBoxes;
use crate::{accounting, invoicing};

mod ia;
pub mod pago202;
pub(crate) use ia::{casilla, periodo, sin_terceros, tema_borrador};

const SETTINGS_KEY: &str = "models";

/// Datos de los modelos que no salen de las facturas ni de la contabilidad.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSettings {
    /// IBAN para domiciliar los ingresos y recibir devoluciones.
    pub iban: Option<String>,
    /// Domiciliar el pago cuando el resultado sea a ingresar.
    pub domiciliar: bool,
    /// 303: cuotas a compensar pendientes de ejercicios anteriores al empezar el año (el de
    /// `vat_compensation_year` o, sin él, el primero del que TaxPain no tiene el anterior). Los
    /// años siguientes encadenan con el 4T del anterior: no se vuelve a aplicar.
    pub vat_compensation_carried: Money,
    /// Año al empezar el cual quedaban `vat_compensation_carried` por compensar.
    #[serde(default)]
    pub vat_compensation_year: Option<i32>,
    /// 303 4T: pedir la devolución en vez de compensar.
    pub request_refund: bool,
    /// Exonerado del modelo 390.
    pub exempt_from_390: bool,
    /// 130: rendimiento neto de actividades del año anterior (minoración de [13]).
    pub previous_year_net: Option<Money>,
    /// 202 (art. 40.2 LIS): cuota del último IS presentado menos deducciones, bonificaciones,
    /// retenciones e ingresos a cuenta.
    #[serde(default)]
    pub corporate_tax_base: Option<Money>,
    /// 202 1P (abril): la base es el último IS **vencido el 1 de abril**, que es el del
    /// ejercicio Y−2 (el de Y−1 se presenta en julio). Si falta, se usa `corporate_tax_base`.
    #[serde(default)]
    pub corporate_tax_base_1p: Option<Money>,
    /// 202: CNAE de la actividad principal y tipo de gravamen del IS del ejercicio.
    #[serde(default)]
    pub cnae: String,
    #[serde(default)]
    pub corporate_tax_rate: String,
    /// 202: cifra de negocios de los 12 meses anteriores superior a 6 millones.
    #[serde(default)]
    pub turnover_over_6m: bool,
    /// 202: cifra de negocios del período anterior inferior a 1 millón.
    #[serde(default)]
    pub turnover_under_1m: bool,
    /// 200: régimen de tipos, correcciones extracontables, reserva de capitalización y bases
    /// negativas pendientes. `TODO(VERIFY)`: son datos de cada ejercicio; hoy se guardan uno.
    #[serde(default)]
    pub corporate_regime: taxpain_models::m200::TaxRegime,
    /// Multas y sanciones del ejercicio (no deducibles) [01815].
    #[serde(default)]
    pub corporate_fines: Money,
    #[serde(default)]
    pub corporate_increases: Money,
    #[serde(default)]
    pub corporate_decreases: Money,
    #[serde(default)]
    pub capitalization_reserve: Money,
    #[serde(default)]
    pub pending_losses: Money,
    /// 200: modelo de cuentas anuales para las páginas de balance y PyG.
    #[serde(default)]
    pub accounts_model: taxpain_core::accounting::statements::ModelKind,
    /// Acogido al régimen especial del criterio de caja del IVA (arts. 163 decies a
    /// sexiesdecies LIVA): el IVA de las ventas se devenga al cobrar y el de las compras se
    /// deduce al pagar (como tarde, el 31 de diciembre del año siguiente).
    #[serde(default)]
    pub cash_basis: bool,
    /// Prorrata del IVA: porcentaje provisional (el definitivo del año anterior).
    /// `None` = sin prorrata (todo el IVA soportado es deducible).
    #[serde(default)]
    pub prorata: Option<u8>,
    /// Prorrata especial (art. 106 LIVA): cada compra se marca por su destino y el porcentaje
    /// sólo se aplica a las de uso común.
    #[serde(default)]
    pub prorata_special: bool,
    /// Prorratas definitivas de años anteriores a los datos de TaxPain, para regularizar los
    /// bienes de inversión [43] (año → porcentaje).
    #[serde(default)]
    pub prorata_history: BTreeMap<i32, u8>,
    /// Descripción breve de la actividad (apartado de prorratas del 390).
    #[serde(default)]
    pub activity_description: String,
    /// Datos de cada ejercicio para el 130, el 202 y el 200 (cambian cada año). Los campos
    /// sueltos de arriba son de la configuración antigua, sin año: se usan (con aviso) en los
    /// ejercicios que aún no tienen los suyos, hasta que el usuario los borre.
    #[serde(default)]
    pub years: BTreeMap<i32, YearSettings>,
    /// Inscrita en el registro de devolución mensual (REDEME, art. 30 RIVA): el 303 es mensual
    /// (art. 71.3 RIVA), se puede pedir la devolución cada mes (art. 116 LIVA) y los libros
    /// registro van por el SII (art. 62.6 RIVA).
    #[serde(default)]
    pub redeme: bool,
    /// Acogida voluntariamente al SII (art. 62.6 RIVA, párrafo segundo).
    #[serde(default)]
    pub sii_voluntario: bool,
}

/// Datos de un ejercicio para el 130, el 202 y el 200.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct YearSettings {
    /// 130 de ese año: rendimiento neto de actividades del año anterior. Sin él, se calcula con
    /// la contabilidad del año anterior si TaxPain la tiene.
    #[serde(default)]
    pub previous_year_net: Option<Money>,
    /// 202 2P y 3P de ese año: cuota del IS del ejercicio anterior (art. 40.2 LIS). Sin ella, la
    /// del 200 de ese ejercicio presentado, si consta.
    #[serde(default)]
    pub corporate_tax_base: Option<Money>,
    /// 202 1P (abril) de ese año: la del IS de hace dos ejercicios (el último vencido el 1 de
    /// abril). Sin ella, la del 200 de hace dos ejercicios presentado, si consta.
    #[serde(default)]
    pub corporate_tax_base_1p: Option<Money>,
    /// 200 de ese ejercicio.
    #[serde(default)]
    pub corporate_regime: Option<taxpain_models::m200::TaxRegime>,
    #[serde(default)]
    pub corporate_fines: Money,
    #[serde(default)]
    pub corporate_increases: Money,
    #[serde(default)]
    pub corporate_decreases: Money,
    #[serde(default)]
    pub capitalization_reserve: Money,
    /// Bases imponibles negativas pendientes al empezar el ejercicio.
    #[serde(default)]
    pub pending_losses: Money,
    /// 130 y Renta: modalidad de estimación directa del IRPF de ese año. Sin ella, simplificada
    /// (o normal, con aviso, si ya había 130 presentados de ese año; ver [`irpf_regime`]).
    #[serde(default)]
    pub irpf_regime: Option<IrpfRegime>,
    /// Renta: reducción del art. 32.2 LIRPF que se cumple.
    #[serde(default)]
    pub activity_reduction: Reduction32,
    /// Renta: otras rentas (sin las exentas) de más de 6.500 € (sin la reducción adicional).
    #[serde(default)]
    pub other_income_over_6500: bool,
    /// Renta: reducción del 20 % por inicio de actividad (art. 32.3 LIRPF).
    #[serde(default)]
    pub activity_start: bool,
    /// 202 de ese período impositivo: modalidad elegida (art. 40.2 o 40.3 LIS). Sin ella, la de
    /// la cuota, salvo que la cifra de negocios obligue a la base (ver [`pago202::modalidad`]).
    #[serde(default)]
    pub m202_modalidad: Option<taxpain_models::m202::Modalidad>,
}

impl ModelSettings {
    /// Datos del ejercicio `year` y si salen de la configuración antigua (sin año).
    pub fn year_settings(&self, year: i32) -> (YearSettings, bool) {
        if let Some(y) = self.years.get(&year) {
            return (y.clone(), false);
        }
        let legacy = YearSettings {
            previous_year_net: self.previous_year_net,
            corporate_tax_base: self.corporate_tax_base,
            corporate_tax_base_1p: self.corporate_tax_base_1p,
            corporate_regime: None,
            corporate_fines: self.corporate_fines,
            corporate_increases: self.corporate_increases,
            corporate_decreases: self.corporate_decreases,
            capitalization_reserve: self.capitalization_reserve,
            pending_losses: self.pending_losses,
            ..YearSettings::default()
        };
        if legacy != YearSettings::default() {
            return (legacy, true);
        }
        (YearSettings::default(), false)
    }
}

/// Aviso cuando los datos del ejercicio vienen de la configuración antigua, sin año.
const LEGACY_YEAR_WARNING: &str = "Los datos de «Datos de los modelos» (rendimiento del año \
    anterior, cuota del IS, correcciones del 200…) no indican el ejercicio: revísalos y \
    guárdalos para este año";

/// Cuota del IS que sirve de base al 202 `period` de `year` (art. 40.2 LIS): la indicada para
/// ese año o, si no, la del 200 presentado del ejercicio que toca ([00599]: cuota líquida menos
/// retenciones), con la explicación de dónde sale.
pub fn corporate_tax_base(
    v: &Vault,
    year: i32,
    period: u8,
) -> Result<(Option<Money>, Option<String>)> {
    let s = settings(v)?;
    let (ys, legacy) = s.year_settings(year);
    let typed = if period == 1 {
        ys.corporate_tax_base_1p
            .or(if legacy { ys.corporate_tax_base } else { None })
    } else {
        ys.corporate_tax_base
    };
    if let Some(b) = typed {
        let note = if legacy && period == 1 && ys.corporate_tax_base_1p.is_none() {
            Some(
                "El 1P se calcula con el IS de hace dos ejercicios (el del año pasado se \
                 presenta en julio): revisa la base del 1P en «Datos de los modelos»"
                    .to_owned(),
            )
        } else {
            legacy.then(|| LEGACY_YEAR_WARNING.to_owned())
        };
        return Ok((Some(b), note));
    }
    // El último ejercicio cuyo 200 había vencido el primer día del plazo del pago (art. 40.2
    // LIS): en un año natural, el de hace dos ejercicios para el 1P (abril) y el anterior para
    // el 2P y el 3P; con otro ejercicio, según cuándo acaba.
    let from = base_year(v, year, period)?;
    if let Some((_, boxes)) = filed_boxes(v, "200", from, "0A")?
        && let Some(b) = boxes.get(&599)
    {
        let mut base = (*b).max(Money::ZERO);
        let mut note = format!(
            "Base del {period}P: cuota del Impuesto sobre Sociedades de {from} presentado ([00599])"
        );
        // Si ese periodo duró menos de doce meses (transición al cambiar la fecha de cierre), se
        // añade la parte proporcional de la cuota de los anteriores hasta completar doce meses
        // (art. 40.2 LIS, párrafo segundo).
        if let Ok(fy) = accounting::fiscal_year(v, from)
            && fy.days() < fy.full_year_days()
        {
            let mut missing = fy.full_year_days() - fy.days();
            let mut y = from - 1;
            while missing > 0 && y >= from - 3 {
                let Ok(prev) = accounting::fiscal_year(v, y) else {
                    break;
                };
                let taken = missing.min(prev.days());
                match filed_boxes(v, "200", y, "0A")? {
                    Some((_, pb)) => {
                        let cuota = pb
                            .get(&599)
                            .copied()
                            .unwrap_or(Money::ZERO)
                            .max(Money::ZERO);
                        let part = Money::from_cents(cuota.cents() * taken / prev.days());
                        base += part;
                        note.push_str(&format!(
                            " más {part} del 200 de {y} ({taken} de sus {} días) para completar \
                             doce meses, porque el ejercicio {from} duró {} (art. 40.2 LIS)",
                            prev.days(),
                            fy.days()
                        ));
                    }
                    None => note.push_str(&format!(
                        ". Falta el 200 de {y}: el ejercicio {from} duró {} días y la base debe \
                         completarse con la parte proporcional de la cuota anterior hasta doce \
                         meses (art. 40.2 LIS)",
                        fy.days()
                    )),
                }
                missing -= taken;
                y -= 1;
            }
        }
        note.push_str(". Si no es la buena, indícala en «Datos de los modelos»");
        return Ok((Some(base), Some(note)));
    }
    Ok((None, None))
}

/// Ejercicio del IS que sirve de base al 202 `period` de `year` (art. 40.2 LIS): el último cuyo
/// plazo del 200 había vencido el día 1 de abril, octubre o diciembre.
fn base_year(v: &Vault, year: i32, period: u8) -> Result<i32> {
    let month = match period {
        1 => Month::April,
        2 => Month::October,
        _ => Month::December,
    };
    let day1 =
        Date::from_calendar_date(year, month, 1).map_err(|e| AppError::Other(e.to_string()))?;
    for label in (year - 3..=year).rev() {
        let Ok(fy) = accounting::fiscal_year(v, label) else {
            continue;
        };
        if crate::cierre::corporate_tax_deadline(fy.end) < day1 {
            return Ok(label);
        }
    }
    Ok(if period == 1 { year - 2 } else { year - 1 })
}

pub fn settings(v: &Vault) -> Result<ModelSettings> {
    Ok(v.setting(SETTINGS_KEY)?.unwrap_or_default())
}

pub fn save_settings(v: &Vault, s: &ModelSettings, now: OffsetDateTime) -> Result<()> {
    v.set_setting(SETTINGS_KEY, s)?;
    audit::append(
        v.connection(),
        &invoicing::now_str(now),
        Actor::User,
        "models.settings",
        "models",
        &serde_json::json!({}),
    )?;
    Ok(())
}

/// Borrador listo para mostrar: casillas, resultado, avisos y fichero.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Draft {
    pub model: String,
    pub year: i32,
    pub quarter: u8,
    /// (casilla, importe en céntimos).
    pub boxes: Vec<(u16, Money)>,
    /// (casilla, nº de perceptores) en 111/115.
    pub payees: Vec<(u16, u64)>,
    /// Detalle por operador (349): (descripción, importe).
    pub details: Vec<(String, Money)>,
    pub declaration: String,
    pub result: Money,
    pub warnings: Vec<String>,
    pub file_name: String,
    /// Fichero DR (texto; se escribe en ISO-8859-1).
    pub file: String,
    /// Nombres e identificadores de los terceros que salen en el detalle o en los avisos. No se
    /// enseñan aparte: sirven para que no lleguen a la IA (ver `models::ia`).
    #[serde(skip)]
    pub terceros: Vec<String>,
}

/// Añade a `out` el nombre y el identificador de un tercero (sin vacíos ni repetidos).
fn tercero(out: &mut Vec<String>, name: &str, id: &str) {
    for s in [name.trim(), id.trim()] {
        if !s.is_empty() && !out.iter().any(|x| x == s) {
            out.push(s.to_owned());
        }
    }
}

/// Primer y último día de un periodo: trimestre (1–4) o mes (101–112, ver
/// [`taxpain_models::periodo`]).
fn quarter_range(year: i32, quarter: u8) -> Result<(Date, Date)> {
    let (first, last) = match periodo::mes(quarter) {
        Some(m) => (m, m),
        None if (1..=4).contains(&quarter) => (quarter * 3 - 2, quarter * 3),
        None => return Err(AppError::Other(format!("trimestre {quarter} no válido"))),
    };
    let first = Month::try_from(first).map_err(|e| AppError::Other(e.to_string()))?;
    let last = Month::try_from(last).map_err(|e| AppError::Other(e.to_string()))?;
    let start =
        Date::from_calendar_date(year, first, 1).map_err(|e| AppError::Other(e.to_string()))?;
    let end = Date::from_calendar_date(year, last, last.length(year))
        .map_err(|e| AppError::Other(e.to_string()))?;
    Ok((start, end))
}

fn parse_date(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]"))
        .map_err(|e| AppError::Other(e.to_string()))
}

/// Fecha en la España peninsular de un instante guardado en RFC 3339 (`reviewed_at`). Los
/// guardados en UTC (todas las revisiones anteriores a esta corrección) se pasan a la hora
/// peninsular: una revisión a las 00:30 del 1 de abril en Madrid es el 31 de marzo a las
/// 22:30 UTC y cuenta en el 2T, no en el 1T. Con otro desfase ya es la hora local (así se
/// guardan ahora). Los importados como fecha («…T00:00:00Z») conservan su día.
pub(crate) fn local_date(ts: &str) -> Result<Date> {
    use time::format_description::well_known::Rfc3339;
    match OffsetDateTime::parse(ts, &Rfc3339) {
        Ok(t) if t.offset().is_utc() => Ok(taxpain_core::spain_time::peninsular_date(t)),
        Ok(t) => Ok(t.date()),
        Err(_) => parse_date(ts.get(..10).unwrap_or(ts)),
    }
}

pub(crate) fn declarant(v: &Vault) -> Result<Declarant> {
    let s = invoicing::settings(v)?.ok_or(AppError::NotConfigured)?;
    let PartyId::Nif(nif) = &s.issuer.id else {
        return Err(AppError::Other("el titular debe tener NIF".into()));
    };
    Ok(match &s.issuer.person {
        Some(p) => Declarant {
            nif: nif.clone(),
            name: match &p.second_surname {
                Some(s2) => format!("{} {s2}", p.first_surname),
                None => p.first_surname.clone(),
            },
            given_name: Some(p.name.clone()),
        },
        None => Declarant {
            nif: nif.clone(),
            name: s.issuer.name.clone(),
            given_name: None,
        },
    })
}

/// Fecha de devengo del IVA de una factura emitida (art. 75 LIVA), la que decide el periodo del
/// 303, del 390 y del 349:
/// - la de la **operación** si consta y es anterior a la de expedición: una factura expedida
///   después de la entrega o del servicio devenga cuando se hizo la operación (art. 75.Uno.1º
///   y 2º);
/// - la de expedición si no consta o si la operación es posterior (p. ej. un cobro anticipado
///   facturado antes, que devenga al cobrarse: art. 75.Dos);
/// - en una entrega intracomunitaria exenta (todas las líneas E5), el día 15 del mes siguiente
///   al del inicio del transporte, o la de la factura si es anterior (art. 75.Uno.8º);
/// - las rectificativas, en la de su expedición (art. 89.Cinco LIVA y art. 15 RD 1619/2012:
///   la modificación se declara en el periodo en que se rectifica).
///
/// El 347 sigue con la fecha de expedición (lo que imputan los terceros con sus libros).
pub(crate) fn vat_accrual_date(inv: &Invoice, breakdown: &[TaxBreakdown]) -> Date {
    use taxpain_core::invoice::Exemption;
    if matches!(inv.kind, InvoiceKind::Rectificativa(_)) {
        return inv.issue_date;
    }
    let Some(op) = inv.operation_date.filter(|op| *op < inv.issue_date) else {
        return inv.issue_date;
    };
    let intra_goods = !breakdown.is_empty()
        && breakdown
            .iter()
            .all(|b| b.vat == VatTreatment::Exempt(Exemption::E5));
    if intra_goods {
        let next = if op.month() == Month::December {
            Date::from_calendar_date(op.year() + 1, Month::January, 15)
        } else {
            Date::from_calendar_date(op.year(), op.month().next(), 15)
        };
        return next.map_or(inv.issue_date, |d| d.min(inv.issue_date));
    }
    op
}

#[derive(Deserialize)]
struct StoredIssued {
    #[serde(skip, default)]
    id: i64,
    invoice: Invoice,
    totals: Totals,
}

impl StoredIssued {
    /// Fecha de devengo del IVA (ver [`vat_accrual_date`]).
    fn accrual(&self) -> Date {
        vat_accrual_date(&self.invoice, &self.totals.breakdown)
    }
}

/// Emitidas no anuladas de `year` (y del año anterior), por número.
fn issued_of_year(v: &Vault, year: i32) -> Result<BTreeMap<String, StoredIssued>> {
    Ok(issued_of_years(v, year - 1, year)?
        .into_iter()
        .map(|st| (st.invoice.number.clone(), st))
        .collect())
}

/// Emitidas no anuladas expedidas entre los años `first` y `last`, por orden de alta. En una
/// lista y no por número: los libros importados pueden reiniciar la numeración cada año.
fn issued_of_years(v: &Vault, first: i32, last: i32) -> Result<Vec<StoredIssued>> {
    issued_where(v, first, last, "")
}

/// Emitidas no anuladas expedidas entre `start` y `end` que van al 349 (entregas exentas E5 o
/// servicios no sujetos N2 a un operador de otro Estado miembro con NIF-IVA): el cotejo del
/// censo las usa para ver si hace falta el alta en el ROI.
pub(crate) fn intracomunitarias_emitidas(v: &Vault, start: Date, end: Date) -> Result<usize> {
    use taxpain_core::invoice::{Exemption, NotSubject, VatTreatment};
    Ok(issued_of_years(v, start.year(), end.year())?
        .iter()
        .filter(|st| st.invoice.issue_date >= start && st.invoice.issue_date <= end)
        .filter(|st| {
            st.totals.breakdown.iter().any(|t| match t.vat {
                VatTreatment::Exempt(Exemption::E5) => eu_operator(&st.invoice, true).is_some(),
                VatTreatment::NotSubject(NotSubject::N2) => {
                    eu_operator(&st.invoice, false).is_some()
                }
                _ => false,
            })
        })
        .count())
}

/// Rectificativas emitidas no anuladas expedidas entre los años `first` y `last`.
fn rectifying_of_years(v: &Vault, first: i32, last: i32) -> Result<Vec<StoredIssued>> {
    Ok(issued_where(v, first, last, "AND kind LIKE 'R%'")?
        .into_iter()
        .filter(|st| matches!(st.invoice.kind, InvoiceKind::Rectificativa(_)))
        .collect())
}

/// Lo facturado en un periodo según los libros registro de TaxPain (emitidas y recibidas), para
/// estimar el resultado cuando el año no tiene contabilidad.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Facturado {
    /// Base de las emitidas no anuladas (las rectificativas restan; las de sustitución, sólo la
    /// diferencia con la que sustituyen). Las autorrepercusiones de compras no se importan como
    /// emitidas ([`crate::libros::mark_self_assessed`]).
    pub ingresos: Money,
    /// Base de las recibidas que son gasto, más su IVA no deducible; sin los bienes de inversión
    /// ni los DUA de importación (la mercancía ya está en la factura del proveedor), de los que
    /// sólo cuenta el IVA no deducible.
    pub gastos: Money,
    /// Base de las recibidas marcadas como bien de inversión (se amortizan, no son gasto).
    pub inversion: Money,
    pub emitidas: usize,
    pub recibidas: usize,
    /// Fecha de la última factura del periodo (emitida o recibida).
    pub hasta: Option<Date>,
}

/// Facturas emitidas (por fecha de expedición) y recibidas (por fecha de la factura o, si no
/// consta, la de registro) entre `start` y `end`.
pub(crate) fn invoiced_between(v: &Vault, start: Date, end: Date) -> Result<Facturado> {
    let mut f = Facturado::default();
    let all = issued_of_years(v, start.year() - 1, end.year())?;
    let base = |b: &[TaxBreakdown]| b.iter().map(|t| t.base).sum::<Money>();
    for st in &all {
        let d = st.invoice.issue_date;
        if d < start || d > end {
            continue;
        }
        let mut b = base(&st.totals.breakdown);
        if let InvoiceKind::Rectificativa(r) = st.invoice.kind
            && r.method == RectificationMethod::Sustitucion
        {
            for (num, date) in &st.invoice.rectified {
                let same = |o: &&StoredIssued| o.invoice.number == *num;
                if let Some(o) = all
                    .iter()
                    .filter(same)
                    .find(|o| o.invoice.issue_date == *date)
                    .or_else(|| all.iter().find(same))
                {
                    b -= base(&o.totals.breakdown);
                }
            }
        }
        f.ingresos += b;
        f.emitidas += 1;
        f.hasta = f.hasta.max(Some(d));
    }
    let mut stmt = v.connection().prepare(
        "SELECT COALESCE(base, 0), COALESCE(vat, 0), deductible_pct, capital_good, vat_regime,
                import_dua, COALESCE(issue_date, substr(reviewed_at, 1, 10)) AS d
         FROM invoice_received WHERE status = 'revisada' AND d BETWEEN ?1 AND ?2",
    )?;
    let rows = stmt.query_map([start.to_string(), end.to_string()], |r| {
        Ok((
            Money::from_cents(r.get(0)?),
            Money::from_cents(r.get(1)?),
            r.get::<_, i64>(2)?,
            r.get::<_, bool>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, bool>(5)?,
            r.get::<_, String>(6)?,
        ))
    })?;
    for row in rows {
        let (base, vat, pct, capital_good, regime, dua, d) = row?;
        f.recibidas += 1;
        f.hasta = f.hasta.max(parse_date(&d).ok());
        if capital_good {
            // Su IVA no deducible es más coste del bien: también se amortiza.
            f.inversion += base;
            continue;
        }
        if !dua {
            f.gastos += base;
        }
        // El IVA no deducible es gasto; el autorrepercutido no se guarda en `vat`.
        if regime == "interior" {
            f.gastos += Money::from_cents(vat.cents() * (100 - pct.clamp(0, 100)) / 100);
        }
    }
    Ok(f)
}

fn issued_where(v: &Vault, first: i32, last: i32, filter: &str) -> Result<Vec<StoredIssued>> {
    let mut stmt = v.connection().prepare(&format!(
        "SELECT id, data FROM invoice_issued WHERE status = 'emitida'
           AND CAST(substr(issue_date, 1, 4) AS INTEGER) BETWEEN ?1 AND ?2 {filter}"
    ))?;
    let rows = stmt.query_map([first, last], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, data) = row?;
        out.push(stored_issued(id, &data)?);
    }
    out.sort_by_key(|st| st.id);
    Ok(out)
}

fn stored_issued(id: i64, data: &str) -> Result<StoredIssued> {
    let mut st: StoredIssued =
        serde_json::from_str(data).map_err(|e| AppError::Other(e.to_string()))?;
    st.id = id;
    Ok(st)
}

/// Emitida no anulada con el número `number` (es único: los importados que lo repiten se
/// guardan con otro).
fn issued_by_number(v: &Vault, number: &str) -> Result<Option<StoredIssued>> {
    let row = v
        .connection()
        .query_row(
            "SELECT id, data FROM invoice_issued WHERE status = 'emitida' AND number = ?1",
            [number],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?;
    row.map(|(id, data)| stored_issued(id, &data)).transpose()
}

struct Received {
    id: i64,
    /// Fecha de registro (revisión en TaxPain), peninsular.
    reviewed: Date,
    /// Proveedor acogido al criterio de caja.
    cash_basis: bool,
    /// Fecha de expedición (la de la operación), si consta.
    issue_date: Option<Date>,
    /// DUA de importación.
    import_dua: bool,
    /// Destino con prorrata especial.
    prorata_use: m303::ProrataUse,
    capital_good: bool,
    nature: String,
    /// `interior`, `intracomunitaria` o `isp`.
    vat_regime: String,
    /// Tipo de la autorrepercusión (puntos básicos) si no es interior.
    self_assessed_rate: Option<i64>,
    country: Option<String>,
    intra_goods: bool,
    nif: String,
    number: String,
    date: Date,
    base: Money,
    vat: Money,
    withholding: Money,
    pct: u8,
}

/// Autorrepercusión de una recibida con inversión del sujeto pasivo.
fn self_assessed(r: &Received) -> Option<m303::SelfAssessed> {
    let kind = match r.vat_regime.as_str() {
        "intracomunitaria" => m303::SelfAssessedKind::Intracomunitaria,
        "isp" => m303::SelfAssessedKind::Otra,
        _ => return None,
    };
    let rate = Rate::from_bp(i32::try_from(r.self_assessed_rate.unwrap_or(2100)).unwrap_or(2100));
    Some(m303::SelfAssessed {
        kind,
        quota: r.base.apply_rate(rate),
        goods: r.intra_goods,
        part: m303::SelfAssessedPart::Ambas,
    })
}

/// Fecha de devengo de una recibida con autorrepercusión (adquisición intracomunitaria o
/// inversión del sujeto pasivo): la de la factura, que es cuando se entiende hecha la operación
/// (art. 75.Uno.8º y 76 LIVA: el 15 del mes siguiente al inicio del transporte o la factura si
/// es anterior; art. 75.Uno.1º y 2º para los servicios), y nunca después del registro: sin
/// fecha (o con una posterior) cuenta la del registro. Es la que usan [10]–[13] y el 349 del
/// proveedor; la deducción sigue a la fecha de registro.
fn self_assessed_accrual(r: &Received) -> Date {
    r.issue_date.map_or(r.reviewed, |d| d.min(r.reviewed))
}

/// Fecha con la que se imputa una recibida.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Basis {
    /// IVA soportado y 347: cuando se recibe y registra (revisión en TaxPain), en fecha
    /// peninsular.
    Received,
    /// Retenciones (111, 115, 190, 180): cuando se paga (art. 78.1 RIRPF). Fecha del pago
    /// conciliado en el banco; si no lo hay, la de la factura.
    Payment,
    /// Sólo las autorrepercutidas (intracomunitarias e ISP), por su fecha de devengo
    /// ([`self_assessed_accrual`]): [10]–[13] del 303 y claves A/I del 349.
    SelfAssessedAccrual,
}

fn received_between(v: &Vault, start: Date, end: Date) -> Result<Vec<Received>> {
    received_by(v, start, end, Basis::Received)
}

fn received_by(v: &Vault, start: Date, end: Date, basis: Basis) -> Result<Vec<Received>> {
    // La fecha de registro es la peninsular de `reviewed_at` (ver `local_date`): SQL sólo hace
    // una criba con un día de margen a cada lado y el filtro exacto se hace abajo.
    let day = time::Duration::days(1);
    let reviewed = "substr(reviewed_at, 1, 10)";
    let (date_expr, filter, lo, hi) = match basis {
        Basis::Received => (reviewed, "d BETWEEN ?1 AND ?2", start - day, end + day),
        // Devengo = la menor de la fecha de la factura y la de registro: una de las dos cae en
        // el periodo (con el margen de la hora peninsular).
        Basis::SelfAssessedAccrual => (
            reviewed,
            "vat_regime IN ('intracomunitaria', 'isp')
               AND (issue_date BETWEEN ?1 AND ?2 OR d BETWEEN ?1 AND ?2)",
            start - day,
            end + day,
        ),
        Basis::Payment => (
            "COALESCE((SELECT min(b.op_date) FROM bank_tx b
                        WHERE b.matched_kind = 'recibida' AND b.matched_id = invoice_received.id),
                      issue_date)",
            "d BETWEEN ?1 AND ?2",
            start,
            end,
        ),
    };
    let mut stmt = v.connection().prepare(&format!(
        "SELECT supplier_nif, number, {date_expr} AS d, base, vat, withholding, deductible_pct,
                capital_good, nature, vat_regime, self_assessed_rate, supplier_country, intra_goods,
                id, cash_basis, issue_date, import_dua, prorata_use, property, reviewed_at
         FROM invoice_received WHERE status = 'revisada'
           AND {filter} ORDER BY d, id"
    ))?;
    let rows = stmt.query_map([lo.to_string(), hi.to_string()], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<i64>>(3)?,
            r.get::<_, Option<i64>>(4)?,
            r.get::<_, Option<i64>>(5)?,
            r.get::<_, i64>(6)?,
            r.get::<_, bool>(7)?,
            r.get::<_, String>(8)?,
            r.get::<_, String>(9)?,
            r.get::<_, Option<i64>>(10)?,
            r.get::<_, Option<String>>(11)?,
            r.get::<_, bool>(12)?,
            (
                r.get::<_, i64>(13)?,
                r.get::<_, bool>(14)?,
                r.get::<_, Option<String>>(15)?,
                r.get::<_, bool>(16)?,
                r.get::<_, String>(17)?,
                r.get::<_, bool>(18)?,
                r.get::<_, Option<String>>(19)?,
            ),
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (
            nif,
            number,
            date,
            base,
            vat,
            wh,
            pct,
            capital_good,
            nature,
            regime,
            rate,
            country,
            goods,
            (id, cash_basis, issue_date, import_dua, prorata_use, _property, reviewed_at),
        ) = row?;
        let reviewed = match &reviewed_at {
            Some(ts) => local_date(ts)?,
            None => parse_date(&date)?,
        };
        let issue_date = issue_date.as_deref().and_then(|d| parse_date(d).ok());
        let date = match basis {
            Basis::Received => reviewed,
            Basis::Payment => parse_date(&date)?,
            Basis::SelfAssessedAccrual => issue_date.map_or(reviewed, |d| d.min(reviewed)),
        };
        if date < start || date > end {
            continue;
        }
        out.push(Received {
            id,
            reviewed,
            cash_basis,
            import_dua,
            prorata_use: m303::ProrataUse::parse(&prorata_use),
            issue_date,
            capital_good,
            nature,
            vat_regime: regime,
            self_assessed_rate: rate,
            country,
            intra_goods: goods,
            nif: nif.unwrap_or_default(),
            number: number.unwrap_or_default(),
            date,
            base: Money::from_cents(base.unwrap_or(0)),
            vat: Money::from_cents(vat.unwrap_or(0)),
            withholding: Money::from_cents(wh.unwrap_or(0)),
            pct: u8::try_from(pct.clamp(0, 100)).unwrap_or(100),
        });
    }
    out.sort_by_key(|r| (r.date, r.id));
    Ok(out)
}

/// Alquiler con retención: marcado al revisar la factura o, en las revisadas antes de existir la
/// marca («general»), detectado por la retención del 19 % (`TODO(VERIFY)`: heurística).
fn is_rent(r: &Received) -> bool {
    if r.withholding.is_zero() {
        return false;
    }
    match r.nature.as_str() {
        "alquiler" => true,
        "profesional" => false,
        _ => {
            r.base
                .apply_rate(Rate::from_bp(1900))
                .cents()
                .abs_diff(r.withholding.cents())
                <= 1
        }
    }
}

/// Criterio de caja: importes «por la regla general» del periodo (información adicional del
/// 303: [62]/[63] ventas, [74]/[75] compras) y si la empresa es destinataria de operaciones
/// del régimen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct CashInfo {
    issued_base: Money,
    issued_vat: Money,
    received_base: Money,
    received_vat: Money,
    recipient: bool,
}

/// Vencimientos de cada factura de la cartera: (fecha de cobro o pago si lo está, importe) y
/// el total a cobrar o pagar de la factura.
type Share = (Vec<(Option<Date>, Money)>, Money);
type Shares = BTreeMap<(String, i64), Share>;

fn cartera_shares(v: &Vault, today: Date) -> Result<Shares> {
    let mut out: Shares = BTreeMap::new();
    for d in crate::cartera::list(v, today)? {
        let e = out
            .entry((d.kind.clone(), d.invoice_id))
            .or_insert_with(|| (Vec::new(), d.invoice_amount));
        e.0.push((d.settled_on, d.amount));
    }
    Ok(out)
}

/// Devengo (ventas) o deducción (compras) con criterio de caja de lo cobrado o pagado el día
/// `settled`: ese día y, si no se cobra o paga, el 31 de diciembre del año siguiente al de la
/// operación (arts. 163 terdecies y decies LIVA).
fn cash_date(operation: Date, settled: Option<Date>) -> Date {
    let limit =
        Date::from_calendar_date(operation.year() + 1, Month::December, 31).unwrap_or(operation);
    settled.map_or(limit, |d| d.min(limit))
}

/// Parte (en tanto por `den`) de la factura devengada hasta `t` inclusive.
fn cash_accrued(operation: Date, share: Option<&Share>, t: Date) -> (i128, i128) {
    match share {
        Some((items, total)) if !total.is_zero() => {
            let done: Money = items
                .iter()
                .filter(|(s, _)| cash_date(operation, *s) <= t)
                .map(|(_, m)| *m)
                .sum();
            (i128::from(done.cents()), i128::from(total.cents()))
        }
        _ => (i128::from(cash_date(operation, None) <= t), 1),
    }
}

/// `m` × `num` / `den`, redondeado al céntimo.
pub(crate) fn scale(m: Money, num: i128, den: i128) -> Money {
    if den == 0 {
        return Money::ZERO;
    }
    let x = i128::from(m.cents()) * num;
    let q = x / den;
    let r = x % den;
    let q = if 2 * r.abs() >= den.abs() {
        q + x.signum() * den.signum()
    } else {
        q
    };
    Money::from_cents(i64::try_from(q).unwrap_or(0))
}

/// Lo de `m` devengado con criterio de caja entre `start` y `end`.
fn cash_part(m: Money, operation: Date, share: Option<&Share>, start: Date, end: Date) -> Money {
    let (n1, d1) = cash_accrued(operation, share, end);
    let (n0, d0) = cash_accrued(operation, share, start - time::Duration::days(1));
    scale(m, n1, d1) - scale(m, n0, d0)
}

/// Factura emitida a la que se aplica el criterio de caja: sólo operaciones interiores con IVA
/// repercutido (no exentas, exportaciones, intracomunitarias ni inversión del sujeto pasivo;
/// art. 163 nonies.Cuatro LIVA).
fn cash_eligible(st: &StoredIssued) -> bool {
    !st.totals.breakdown.is_empty()
        && st
            .totals
            .breakdown
            .iter()
            .all(|b| matches!(b.vat, VatTreatment::Taxed { .. }))
}

/// País e identificador del destinatario si es un operador intracomunitario: empresario de otro
/// Estado miembro identificado con NIF-IVA, para entregas de bienes (`goods`) o servicios. Es
/// lo que lleva una operación al 349 y, en el 303, los servicios no sujetos a [59].
fn eu_operator(inv: &Invoice, goods: bool) -> Option<(&str, &str)> {
    use taxpain_core::invoice::IdType;
    match inv.recipient.as_ref().map(|r| &r.id) {
        Some(PartyId::Foreign {
            country,
            id_type: IdType::NifIva,
            id,
        }) if taxpain_core::eu::is_other_member_state(country, goods) => {
            Some((country.as_str(), id.as_str()))
        }
        _ => None,
    }
}

/// Emitidas, recibidas, avisos e información del criterio de caja de un periodo.
type VatInputs = (Vec<IssuedVat>, Vec<ReceivedVat>, Vec<String>, CashInfo);

/// Emitidas y recibidas cuyo IVA se devenga o deduce entre `start` y `end`, tal como las usan
/// el 303 y el 390 (con criterio de caja si la empresa o el proveedor lo aplican).
fn vat_inputs(v: &Vault, year: i32, start: Date, end: Date) -> Result<VatInputs> {
    let cash = settings(v)?.cash_basis;
    let mut info = CashInfo::default();
    // También las del año siguiente: una factura de enero puede devengar en diciembre.
    let all = issued_of_years(v, year - 1, year + 1)?;
    let shares = cartera_shares(v, end)?;
    let mut issued = Vec::new();
    let mut warnings = Vec::new();
    for st in &all {
        // Devengo por la fecha de la operación si es anterior a la de expedición (art. 75).
        let d = st.accrual();
        let in_period = d >= start && d <= end;
        let cash_share = if cash && cash_eligible(st) {
            if in_period {
                info.issued_base += st.totals.breakdown.iter().map(|b| b.base).sum::<Money>();
                info.issued_vat += st
                    .totals
                    .breakdown
                    .iter()
                    .map(|b| b.vat_amount)
                    .sum::<Money>();
            }
            Some(shares.get(&("emitida".to_owned(), st.id)))
        } else {
            if !in_period {
                continue;
            }
            None
        };
        let (rectification, breakdown) = match st.invoice.kind {
            InvoiceKind::Rectificativa(r) if r.method == RectificationMethod::Sustitucion => {
                let mut old = Vec::new();
                for (num, date) in &st.invoice.rectified {
                    let same = |o: &&StoredIssued| o.invoice.number == *num;
                    let found = all
                        .iter()
                        .filter(same)
                        .find(|o| o.invoice.issue_date == *date)
                        .or_else(|| all.iter().find(same));
                    match found {
                        Some(o) => old.extend(o.totals.breakdown.iter().cloned()),
                        None => warnings.push(format!(
                            "{}: no encuentro la factura rectificada {num}; se declara completa",
                            st.invoice.number
                        )),
                    }
                }
                (
                    true,
                    crate::rectificativas_emitidas::breakdown_diff(&st.totals.breakdown, &old),
                )
            }
            InvoiceKind::Rectificativa(_) => (true, st.totals.breakdown.clone()),
            _ => (false, st.totals.breakdown.clone()),
        };
        // Criterio de caja: sólo la parte cobrada en el periodo (o la que vence el 31/12 del
        // año siguiente sin cobrar).
        let breakdown = match cash_share {
            Some(share) => {
                let part: Vec<TaxBreakdown> = breakdown
                    .into_iter()
                    .map(|b| TaxBreakdown {
                        base: cash_part(b.base, d, share, start, end),
                        vat_amount: cash_part(b.vat_amount, d, share, start, end),
                        surcharge_amount: cash_part(b.surcharge_amount, d, share, start, end),
                        ..b
                    })
                    .collect();
                if part
                    .iter()
                    .all(|b| b.base.is_zero() && b.vat_amount.is_zero())
                {
                    continue;
                }
                part
            }
            None => breakdown,
        };
        issued.push(IssuedVat {
            number: st.invoice.number.clone(),
            date: d,
            rectification,
            breakdown,
            cash_basis: cash_share.is_some(),
            eu_business: eu_operator(&st.invoice, false).is_some(),
            oss: st.totals.oss.is_some(),
        });
    }

    // Recibidas: con criterio de caja (de la empresa o del proveedor), la cuota se deduce al
    // pagar; se miran las de los dos años anteriores que aún pueden deducirse ahora.
    let window_start = if cash || received_cash_exists(v)? {
        Date::from_calendar_date(start.year() - 2, Month::January, 1).unwrap_or(start)
    } else {
        start
    };
    let mut received = Vec::new();
    // Reglas de afectación y sectores diferenciados de Contabilidad → IVA y libros.
    let afectacion = crate::iva_holding::Afectacion::load(v)?;
    for r in received_by(v, window_start, end, Basis::Received)? {
        let in_period = r.date >= start && r.date <= end;
        let applies = (cash || r.cash_basis) && r.vat_regime == "interior" && !r.vat.is_zero();
        let (base, vat) = if applies {
            if in_period {
                info.received_base += r.base;
                info.received_vat += r.vat;
                info.recipient |= r.cash_basis;
            }
            let op = r.issue_date.unwrap_or(r.date);
            let share = shares.get(&("recibida".to_owned(), r.id));
            let vat = cash_part(r.vat, op, share, start, end);
            if vat.is_zero() {
                continue;
            }
            (cash_part(r.base, op, share, start, end), vat)
        } else {
            if !in_period {
                continue;
            }
            (r.base, r.vat)
        };
        // Autorrepercutida registrada en el periodo: aquí se deduce; si su fecha de devengo es
        // de otro periodo, allí se autorrepercute (ver `SelfAssessedPart`).
        let mut sa = self_assessed(&r);
        if let Some(sa) = sa.as_mut() {
            let accrual = self_assessed_accrual(&r);
            if accrual < start {
                sa.part = m303::SelfAssessedPart::Deduccion;
                warnings.push(format!(
                    "{}: factura del {} registrada el {}: su IVA se autorrepercute ([10]–[13]) en \
                     el periodo de la factura y aquí sólo se deduce",
                    r.number,
                    taxpain_core::calendar::reminders::format_es(accrual),
                    taxpain_core::calendar::reminders::format_es(r.reviewed)
                ));
            }
        }
        let ap = afectacion.aplicar(v, r.id, &r.nif, r.reviewed, r.pct)?;
        received.push(ReceivedVat {
            self_assessed: sa,
            rectification: r.base.cents() < 0,
            number: r.number,
            date: r.date,
            kind: if r.capital_good {
                ReceivedKind::BienInversion
            } else {
                ReceivedKind::Corriente
            },
            base,
            vat,
            deductible_pct: ap.pct,
            import: r.import_dua,
            prorata_use: r.prorata_use,
            outside_prorata: ap.fuera_prorrata,
        });
    }
    // Autorrepercutidas con fecha de devengo en el periodo pero registradas después: el devengo
    // ([10]–[13]) va aquí y la deducción en el periodo del registro. Es legal que caigan en
    // trimestres distintos (arts. 75, 76 y 99 LIVA), aunque lo habitual es que coincidan.
    for r in received_by(v, start, end, Basis::SelfAssessedAccrual)? {
        if (start..=end).contains(&r.reviewed) {
            continue; // Registrada en el periodo: ya está arriba con devengo y deducción.
        }
        let Some(mut sa) = self_assessed(&r) else {
            continue;
        };
        sa.part = m303::SelfAssessedPart::Devengo;
        // Con esta base, `r.date` ya es la fecha de devengo.
        warnings.push(format!(
            "{}: factura del {} registrada el {}: aquí se autorrepercute su IVA ([10]–[13]) y se \
             deduce en el periodo del registro",
            r.number,
            taxpain_core::calendar::reminders::format_es(r.date),
            taxpain_core::calendar::reminders::format_es(r.reviewed)
        ));
        let ap = afectacion.aplicar(v, r.id, &r.nif, r.reviewed, r.pct)?;
        received.push(ReceivedVat {
            self_assessed: Some(sa),
            rectification: r.base.cents() < 0,
            number: r.number,
            date: r.date,
            kind: if r.capital_good {
                ReceivedKind::BienInversion
            } else {
                ReceivedKind::Corriente
            },
            base: r.base,
            vat: r.vat,
            deductible_pct: ap.pct,
            import: r.import_dua,
            prorata_use: r.prorata_use,
            outside_prorata: ap.fuera_prorrata,
        });
    }
    Ok((issued, received, warnings, info))
}

fn received_cash_exists(v: &Vault) -> Result<bool> {
    Ok(v.connection().query_row(
        "SELECT EXISTS(SELECT 1 FROM invoice_received WHERE cash_basis = 1 AND status = 'revisada')",
        [],
        |r| r.get(0),
    )?)
}

/// Último día para presentar el 303 del periodo del día `accrual`: el del calendario fiscal con
/// el perfil de la empresa (trimestral o mensual, trasladado a hábil con sus festivos) o, sin
/// perfil, el trimestral (día 20 del mes siguiente; el 4T, 30 de enero) con los festivos
/// nacionales (art. 71 RIVA).
fn vat_deadline(v: &Vault, accrual: Date) -> Result<Date> {
    use taxpain_core::calendar::{self, Model, Period, TaxpayerProfile};
    let profile = v
        .setting::<TaxpayerProfile>("taxpayer_profile")?
        .filter(|p| p.iva.is_some())
        .unwrap_or_else(TaxpayerProfile::autonomo);
    let year = accrual.year();
    let month = u8::from(accrual.month());
    let quarter = (month - 1) / 3 + 1;
    for y in [year, year + 1] {
        let (h, _) = calendar::calendar_for(y, profile.ccaa, &profile.festivos_locales);
        if let Some(d) = calendar::deadlines_in_year(&profile, y, &h)
            .into_iter()
            .find(|d| {
                d.model == Model::M303
                    && d.fiscal_year == year
                    && (d.period == Period::Quarter(quarter) || d.period == Period::Month(month))
            })
        {
            return Ok(d.due_on);
        }
    }
    // No debería pasar (el perfil tiene IVA): el plazo nominal trimestral.
    let (_, end) = quarter_range(year, quarter)?;
    let next = end + time::Duration::days(1);
    Date::from_calendar_date(
        next.year(),
        next.month(),
        if quarter == 4 { 30 } else { 20 },
    )
    .map_err(|e| AppError::Other(e.to_string()))
}

/// Por qué ya no se puede cambiar el IVA devengado el día `accrual` (`None` si aún se puede): el
/// 303 de su periodo está presentado (en producción desde TaxPain o importado de la gestoría)
/// o su plazo terminó antes de `today`. Lo usa la anulación de facturas: el IVA de un periodo
/// cerrado se corrige con una rectificativa en el periodo actual (art. 89 LIVA).
pub(crate) fn vat_period_closed(v: &Vault, accrual: Date, today: Date) -> Result<Option<String>> {
    let year = accrual.year();
    let month = u8::from(accrual.month());
    let quarter = (month - 1) / 3 + 1;
    let (quarterly, monthly) = (format!("{quarter}T"), format!("{month:02}"));
    let filed: bool = v.connection().query_row(
        "SELECT EXISTS(SELECT 1 FROM tax_return WHERE model = '303' AND year = ?1
                         AND period IN (?2, ?3) AND environment = 'produccion')
             OR EXISTS(SELECT 1 FROM filed_model WHERE model = '303' AND year = ?1
                         AND period IN (?2, ?3))",
        taxpain_vault::rusqlite::params![year, quarterly, monthly],
        |r| r.get(0),
    )?;
    if filed {
        return Ok(Some(format!(
            "el 303 del {quarterly} de {year}, donde se declaró su IVA, ya está presentado"
        )));
    }
    let due = vat_deadline(v, accrual)?;
    if today > due {
        return Ok(Some(format!(
            "el plazo del 303 del {quarterly} de {year}, donde se declara su IVA, terminó el {}",
            taxpain_core::calendar::reminders::format_es(due)
        )));
    }
    Ok(None)
}

/// Casillas fijadas a mano de un modelo y periodo.
pub fn adjustments(v: &Vault, model: &str, year: i32, quarter: u8) -> Result<BTreeMap<u16, Money>> {
    let mut st = v.connection().prepare(
        "SELECT box, amount FROM model_adjustment WHERE model = ?1 AND year = ?2 AND period = ?3",
    )?;
    let rows = st.query_map(
        taxpain_vault::rusqlite::params![model, year, period_code(model, quarter)],
        |r| Ok((r.get::<_, u16>(0)?, Money::from_cents(r.get(1)?))),
    )?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// ¿Se puede fijar a mano la casilla `b` del modelo?
fn adjustable(model: &str, b: u16) -> bool {
    match model {
        "303" => m303::adjustable(b),
        "130" => m130::ADJUSTABLE.contains(&b),
        "111" | "115" => withholding::adjustable(model, b),
        "202" => pago202::ADJUSTABLE.contains(&b),
        _ => false,
    }
}

/// Fija a mano una casilla (o, con `None`, vuelve al cálculo automático): 303, 130, 111, 115 y
/// 202 por la base.
pub fn set_adjustment(
    v: &Vault,
    model: &str,
    year: i32,
    quarter: u8,
    box_: u16,
    amount: Option<Money>,
    now: OffsetDateTime,
) -> Result<()> {
    if !matches!(model, "303" | "130" | "111" | "115" | "202") {
        return Err(AppError::Other(format!(
            "en el {model} no se pueden fijar casillas a mano"
        )));
    }
    if amount.is_some() && !adjustable(model, box_) {
        return Err(AppError::Other(format!(
            "la casilla [{box_:02}] no se puede fijar a mano: es un total o no existe en el {model}"
        )));
    }
    let period = period_code(model, quarter);
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    match amount {
        Some(m) => {
            tx.execute(
                "INSERT INTO model_adjustment(model, year, period, box, amount, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(model, year, period, box) DO UPDATE SET amount = excluded.amount,
                   updated_at = excluded.updated_at",
                taxpain_vault::rusqlite::params![
                    model,
                    year,
                    period,
                    box_,
                    m.cents(),
                    now.format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_default()
                ],
            )?;
        }
        None => {
            tx.execute(
                "DELETE FROM model_adjustment WHERE model = ?1 AND year = ?2 AND period = ?3 AND box = ?4",
                taxpain_vault::rusqlite::params![model, year, period, box_],
            )?;
        }
    }
    audit::append(
        &tx,
        &now.format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
        Actor::User,
        "model.adjust",
        &format!("{model} {period} {year} [{box_:02}]"),
        &serde_json::json!({ "amount": amount.map(Money::cents) }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Si una operación cuenta en la prorrata con derecho a deducir (`Some(true)`, en los dos
/// términos), sin él (`Some(false)`, sólo en el denominador) o no cuenta (`None`), art. 104.Dos
/// LIVA. Las no sujetas no se computan. TODO(VERIFY): las no sujetas por localización que darían
/// derecho a deducir (N2, art. 94.Uno.2.º) deberían computarse, los servicios por su
/// contraprestación × la parte de coste soportada en el territorio (art. 104.Cinco), que TaxPain
/// no tiene; por ahora no cuentan.
fn prorata_right(vat: VatTreatment) -> Option<bool> {
    use taxpain_core::invoice::Exemption;
    match vat {
        VatTreatment::Taxed { .. }
        | VatTreatment::ReverseCharge
        | VatTreatment::Exempt(Exemption::E2 | Exemption::E3 | Exemption::E4 | Exemption::E5) => {
            Some(true)
        }
        VatTreatment::Exempt(_) => Some(false),
        VatTreatment::NotSubject(_) => None,
    }
}

/// Cómo tributa la factura emitida `invoice_id` (la venta de un bien de inversión, para el art.
/// 110 LIVA): bases con derecho a deducir (gravadas o exentas con derecho, art. 110.Uno.1.º),
/// IVA repercutido, bases exentas sin derecho y bases no sujetas. `None` si no está emitida.
pub(crate) fn sale_invoice_vat(
    v: &Vault,
    invoice_id: i64,
) -> Result<Option<(Money, Money, Money, Money)>> {
    let row = v
        .connection()
        .query_row(
            "SELECT data FROM invoice_issued WHERE id = ?1 AND status = 'emitida'",
            [invoice_id],
            |r| r.get::<_, String>(0),
        )
        .optional()?;
    let Some(data) = row else {
        return Ok(None);
    };
    let st = stored_issued(invoice_id, &data)?;
    let (mut right, mut vat, mut exempt, mut not_subject) =
        (Money::ZERO, Money::ZERO, Money::ZERO, Money::ZERO);
    for b in &st.totals.breakdown {
        match prorata_right(b.vat) {
            Some(true) => {
                right += b.base;
                vat += b.vat_amount;
            }
            Some(false) => exempt += b.base,
            None => not_subject += b.base,
        }
    }
    Ok(Some((right, vat, exempt, not_subject)))
}

/// Bases de las emitidas no anuladas con devengo en `year`, por NIF del destinatario (letras y
/// cifras en mayúsculas): los servicios facturados a cada participada.
pub(crate) fn issued_bases_by_client(v: &Vault, year: i32) -> Result<BTreeMap<String, Money>> {
    let mut out = BTreeMap::new();
    for st in issued_of_years(v, year - 1, year)? {
        if st.accrual().year() != year {
            continue;
        }
        let Some(p) = &st.invoice.recipient else {
            continue;
        };
        let base = st.totals.breakdown.iter().map(|b| b.base).sum::<Money>();
        // Con un identificador extranjero vale con o sin el prefijo del país.
        let keys = match &p.id {
            taxpain_core::invoice::PartyId::Nif(nif) => vec![crate::vinculadas::clave_nif(nif)],
            taxpain_core::invoice::PartyId::Foreign { country, id, .. } => vec![
                crate::vinculadas::clave_nif(id),
                crate::vinculadas::clave_nif(&format!("{country}{id}")),
            ],
        };
        for k in keys {
            *out.entry(k).or_insert(Money::ZERO) += base;
        }
    }
    Ok(out)
}

/// Prorrata del año: (total de operaciones, operaciones con derecho a deducir, cuotas
/// deducibles del año antes de aplicar la prorrata; con la especial, sólo las de uso común).
pub(crate) fn prorata_year(v: &Vault, year: i32, special: bool) -> Result<(Money, Money, Money)> {
    let (start, _) = quarter_range(year, 1)?;
    let (_, end) = quarter_range(year, 4)?;
    let (issued, received, _, _) = vat_inputs(v, year, start, end)?;
    let mut total = Money::ZERO;
    let mut with_right = Money::ZERO;
    for inv in &issued {
        for b in &inv.breakdown {
            match prorata_right(b.vat) {
                Some(true) => {
                    total += b.base;
                    with_right += b.base;
                }
                Some(false) => total += b.base,
                None => {}
            }
        }
    }
    // Las entregas de bienes de inversión utilizados en la actividad no se computan en ninguno
    // de los dos términos (art. 104.Tres.3.º LIVA): las ventas de inmovilizado del año, las
    // mismas que se restan del volumen del 390 ([106] y [107]).
    let sales = asset_sales_390(v, year, VolumeIn::M390)?;
    total = (total - sales.real_estate - sales.capital_goods).max(Money::ZERO);
    with_right = (with_right - sales.with_right).max(Money::ZERO).min(total);
    let received: Vec<ReceivedVat> = received
        .into_iter()
        .filter(|r| !r.outside_prorata && (!special || r.prorata_use == m303::ProrataUse::Comun))
        .collect();
    let full = m303::compute(year, 4, &[], &received, &Options303::default());
    let cuotas = [29u16, 31, 33, 35, 37, 39, 41]
        .iter()
        .map(|k| full.get(*k))
        .sum::<Money>();
    Ok((total, with_right, cuotas))
}

/// Prorrata definitiva de un año: la anotada a mano para los años anteriores a TaxPain o, si
/// hay facturas emitidas ese año, la calculada con ellas.
pub(crate) fn definitive_prorata(v: &Vault, s: &ModelSettings, year: i32) -> Result<Option<u8>> {
    if let Some(p) = s.prorata_history.get(&year) {
        return Ok(Some((*p).min(100)));
    }
    if issued_of_year(v, year)?.is_empty() {
        return Ok(None);
    }
    let (ops, with_right, _) = prorata_year(v, year, s.prorata_special)?;
    Ok(Some(m303::prorata_percent(with_right, ops)))
}

/// Regularización de las deducciones por bienes de inversión de años anteriores (arts. 107 a
/// 109 LIVA): durante los cuatro años siguientes al de compra (nueve si es un inmueble), si la
/// prorrata definitiva del año difiere en más de 10 puntos de la del año de compra, se
/// regulariza la quinta (o décima) parte de la diferencia. Devuelve el importe y los avisos.
fn investment_regularization(
    v: &Vault,
    s: &ModelSettings,
    year: i32,
    definitive: u8,
) -> Result<(Money, Vec<String>)> {
    crate::iva_holding::regularizacion_anual(v, s, year, definitive)
}

/// Prorrata para el 303 del trimestre (y, en el 4T, para el 390): provisional y, en el último
/// periodo, definitiva con sus regularizaciones. Devuelve también los avisos.
fn prorata_for(
    v: &Vault,
    s: &ModelSettings,
    year: i32,
    quarter: u8,
) -> Result<(Option<m303::Prorata>, Vec<String>)> {
    let Some(prov) = s.prorata else {
        return Ok((None, vec![]));
    };
    let mut p = m303::Prorata {
        provisional: prov.min(100),
        special: s.prorata_special,
        cnae: s.cnae.clone(),
        ..Default::default()
    };
    let mut warnings = Vec::new();
    if periodo::ultimo(quarter) {
        let (ops, with_right, cuotas) = prorata_year(v, year, s.prorata_special)?;
        let def = m303::prorata_percent(with_right, ops);
        p.definitive = Some(def);
        p.operations = ops;
        p.deductible_operations = with_right;
        p.regularization =
            cuotas.apply_rate(Rate::percent(i32::from(def) - i32::from(p.provisional)));
        let (reg, w) = investment_regularization(v, s, year, def)?;
        p.investment_regularization = reg;
        warnings = w;
    }
    Ok((Some(p), warnings))
}

/// 303 de un trimestre con la compensación pendiente indicada.
fn compute_303(
    v: &Vault,
    year: i32,
    quarter: u8,
    pending: Money,
    s: &ModelSettings,
) -> Result<Result303> {
    let (start, end) = quarter_range(year, quarter)?;
    let (issued, received, warnings, cash) = vat_inputs(v, year, start, end)?;
    let fixed = adjustments(v, "303", year, quarter)?;
    let (prorata, prorata_warnings) = prorata_for(v, s, year, quarter)?;
    let (sale_regularization, sale_warnings) =
        crate::iva_holding::regularizacion_ventas(v, s, start, end)?;
    let opts = Options303 {
        pending_compensation: pending,
        adjustments: fixed.clone(),
        prorata,
        capital_goods_sale_regularization: sale_regularization,
        ..options_303(s, quarter)
    };
    let mut r = m303::compute(year, quarter, &issued, &received, &opts);
    r.warnings.extend(warnings);
    r.warnings.extend(prorata_warnings);
    r.warnings.extend(sale_warnings);
    r.warnings.extend(crate::iva_holding::aviso_sectores(v)?);
    // Información adicional del criterio de caja: importes por la regla general de devengo.
    for (b, m) in [
        (62, cash.issued_base),
        (63, cash.issued_vat),
        (74, cash.received_base),
        (75, cash.received_vat),
    ] {
        if !m.is_zero() && !fixed.contains_key(&b) {
            r.boxes.insert(b, m);
            r.no_activity = false;
        }
    }
    r.cash_basis_recipient = cash.recipient;
    if s.cash_basis {
        let footer = invoicing::settings(v)?
            .and_then(|i| i.footer)
            .unwrap_or_default()
            .to_lowercase();
        if !footer.contains("criterio de caja") {
            r.warnings.push(
                "Con criterio de caja tus facturas deben decir «Régimen especial del criterio de \
                 caja» (art. 6.1.o del Reglamento de facturación): añádelo al pie de factura en \
                 Ajustes."
                    .into(),
            );
        }
    }
    Ok(r)
}

/// Casillas de la última presentación **en producción** de un modelo y periodo, si la hay.
fn filed_in_production(
    v: &Vault,
    model: &str,
    year: i32,
    period: &str,
) -> Result<Option<(String, BTreeMap<u16, Money>)>> {
    let row: Option<(String, String)> = v
        .connection()
        .query_row(
            "SELECT declaration, boxes FROM tax_return
             WHERE model = ?1 AND year = ?2 AND period = ?3 AND environment = 'produccion'
             ORDER BY presented_at DESC, id DESC LIMIT 1",
            taxpain_vault::rusqlite::params![model, year, period],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map(Some)
        .or_else(|e| match e {
            taxpain_vault::rusqlite::Error::QueryReturnedNoRows => Ok(None),
            e => Err(e),
        })?;
    Ok(match row {
        Some((decl, boxes)) => {
            let list: Vec<(u16, Money)> =
                serde_json::from_str(&boxes).map_err(|e| AppError::Other(e.to_string()))?;
            Some((decl, list.into_iter().collect()))
        }
        None => None,
    })
}

/// Casillas presentadas de un modelo y periodo: las de la presentación en producción desde
/// TaxPain o, si no la hay, las del gestor (importadas de la carpeta o tecleadas), con el tipo
/// de declaración si se sabe.
pub(crate) fn filed_boxes(
    v: &Vault,
    model: &str,
    year: i32,
    period: &str,
) -> Result<Option<FiledBoxes>> {
    if let Some((decl, boxes)) = filed_in_production(v, model, year, period)? {
        return Ok(Some((Some(decl), boxes)));
    }
    crate::presentados::filed_by_gestor(v, model, year, period)
}

/// Casillas del 303 presentado de un trimestre (desde TaxPain o por el gestor), si lo hay: la
/// liquidación contable usa su resultado, que es lo que se paga.
pub(crate) fn filed_303_boxes(
    v: &Vault,
    year: i32,
    quarter: u8,
) -> Result<Option<BTreeMap<u16, Money>>> {
    Ok(filed_boxes(v, "303", year, &periodo::codigo(quarter))?.map(|(_, b)| b))
}

/// Tipo de declaración, casillas y aviso de un 303 presentado.
type Filed303 = (String, BTreeMap<u16, Money>, Option<String>);

/// Un 303 presentado (desde TaxPain o por el gestor): tipo de declaración, casillas y un aviso
/// si el tipo no consta y no se puede saber por las casillas (un 4T negativo sin nada pendiente
/// puede ser a devolver o a compensar: se supone a compensar).
fn filed_303(v: &Vault, year: i32, quarter: u8) -> Result<Option<Filed303>> {
    let Some((decl, boxes)) = filed_boxes(v, "303", year, &periodo::codigo(quarter))? else {
        return Ok(None);
    };
    let get = |b: u16| boxes.get(&b).copied().unwrap_or(Money::ZERO);
    let (decl, warning) = match decl {
        Some(d) => (d, None),
        None => {
            let r = get(71);
            if r.cents() > 0 {
                ("I".to_owned(), None)
            } else if r.is_zero() {
                ("N".to_owned(), None)
            } else if !periodo::ultimo(quarter) || get(87).cents() > 0 {
                // A compensar: lo pendiente sigue pendiente ([87] = [110]).
                ("C".to_owned(), None)
            } else if get(78).cents() > 0 {
                // A devolver: se aplica todo lo pendiente ([78] = [110], [87] = 0).
                ("D".to_owned(), None)
            } else {
                (
                    "C".to_owned(),
                    Some(format!(
                        "No consta si el 303 del {} ({r}) se pidió a devolver o a \
                         compensar: se arrastra como «a compensar» a la [110]; revísala",
                        periodo::nombre(quarter, year)
                    )),
                )
            }
        }
    };
    Ok(Some((decl, boxes, warning)))
}

/// Lo que queda por compensar tras un 303: lo pendiente que no se aplicó ([87]) más, si salió
/// a compensar, su resultado negativo.
fn carry(declaration: &str, b87: Money, result: Money) -> Money {
    b87 + if declaration == "C" {
        -result
    } else {
        Money::ZERO
    }
}

/// ¿Hay datos del IVA de `year` (facturas o 303 presentados)?
fn has_vat_data(v: &Vault, year: i32) -> Result<bool> {
    Ok(v.connection().query_row(
        "SELECT EXISTS(SELECT 1 FROM invoice_issued WHERE status = 'emitida'
                  AND substr(issue_date, 1, 4) = ?1)
             OR EXISTS(SELECT 1 FROM invoice_received WHERE status = 'revisada'
                  AND substr(reviewed_at, 1, 4) = ?1)
             OR EXISTS(SELECT 1 FROM filed_model WHERE model = '303' AND year = ?2)
             OR EXISTS(SELECT 1 FROM tax_return WHERE model = '303' AND year = ?2
                  AND environment = 'produccion')",
        taxpain_vault::rusqlite::params![year.to_string(), year],
        |r| r.get(0),
    )?)
}

/// Cuotas a compensar al empezar `year` (la [110] del 1T): lo que quedó del 4T del año anterior
/// (presentado desde TaxPain o por el gestor; si no, calculado con los datos de ese año) o, al
/// empezar la cadena, lo anotado en los ajustes. Con los avisos que haya.
fn carried_into(v: &Vault, year: i32, s: &ModelSettings) -> Result<(Money, Vec<String>)> {
    if s.vat_compensation_year == Some(year) {
        return Ok((s.vat_compensation_carried, vec![]));
    }
    // El último periodo del año anterior: diciembre si era mensual, el 4T si era trimestral.
    let filed_last = match filed_303(v, year - 1, periodo::de_mes(12))? {
        Some(f) => Some(f),
        None => filed_303(v, year - 1, 4)?,
    };
    if let Some((decl, boxes, warning)) = filed_last {
        let get = |b: u16| boxes.get(&b).copied().unwrap_or(Money::ZERO);
        return Ok((
            carry(&decl, get(87), get(71)),
            warning.into_iter().collect(),
        ));
    }
    let chain = match s.vat_compensation_year {
        // La cadena empieza en el año anotado: hasta él se encadena aunque falten datos.
        Some(y) if y < year => year - y <= 30,
        _ => has_vat_data(v, year - 1)?,
    };
    // Sin lo presentado, el último periodo calculado con la periodicidad que diga el perfil.
    let last_period = if vat_monthly(v)? {
        periodo::de_mes(12)
    } else {
        4
    };
    if chain && let Some(r) = quarters_303(v, year - 1, last_period, s)?.pop() {
        return Ok((carry(r.declaration.code(), r.get(87), r.result()), vec![]));
    }
    Ok((
        if s.vat_compensation_year.is_none() {
            s.vat_compensation_carried
        } else {
            Money::ZERO
        },
        vec![],
    ))
}

/// Los 303 del primer periodo del año al `quarter` (1T…4T o enero…diciembre), encadenando la
/// compensación pendiente: lo arrastrado del año anterior más los resultados negativos
/// compensados de cada periodo. Si un periodo anterior ya se presentó (en producción desde
/// TaxPain o por el gestor), se usan sus casillas reales.
fn quarters_303(v: &Vault, year: i32, quarter: u8, s: &ModelSettings) -> Result<Vec<Result303>> {
    if !periodo::valido(quarter) {
        return Err(AppError::Other(format!("periodo {quarter} no válido")));
    }
    let mut out: Vec<Result303> = Vec::new();
    let (mut pending, mut notes) = carried_into(v, year, s)?;
    for q in periodo::primero(quarter)..=quarter {
        if let Some(r) = out.last() {
            pending = match filed_303(v, year, q - 1)? {
                Some((decl, boxes, warning)) => {
                    notes.extend(warning);
                    let get = |b: u16| boxes.get(&b).copied().unwrap_or(Money::ZERO);
                    carry(&decl, get(87), get(71))
                }
                None => carry(r.declaration.code(), r.get(87), r.result()),
            };
        }
        let mut r = compute_303(v, year, q, pending, s)?;
        r.warnings.append(&mut notes);
        out.push(r);
    }
    Ok(out)
}

/// Tipo de declaración del 303 por su código.
fn declaration_type(code: &str) -> Option<m303::DeclarationType> {
    use m303::DeclarationType as D;
    Some(match code {
        "I" => D::Ingreso,
        "U" => D::Domiciliacion,
        "C" => D::Compensacion,
        "D" => D::Devolucion,
        "N" => D::SinActividad,
        _ => return None,
    })
}

fn options_303(s: &ModelSettings, quarter: u8) -> Options303 {
    Options303 {
        pending_compensation: Money::ZERO,
        // El IBAN sirve para domiciliar (si se marcó) y para la devolución del último periodo
        // (de cualquiera si está en el REDEME).
        iban: s
            .iban
            .clone()
            .filter(|_| s.domiciliar || periodo::ultimo(quarter) || s.redeme),
        domiciliar: s.domiciliar,
        request_refund: s.request_refund,
        exempt_from_390: s.exempt_from_390,
        cash_basis: s.cash_basis,
        adjustments: BTreeMap::new(),
        prorata: None,
        exonerado_390: None,
        capital_goods_sale_regularization: Money::ZERO,
        redeme: s.redeme,
        sii_voluntario: s.sii_voluntario,
    }
}

/// Borrador del 303 (con la compensación encadenada desde el 1T).
pub fn draft_303(v: &Vault, year: i32, quarter: u8) -> Result<Draft> {
    let s = settings(v)?;
    let r = quarters_303(v, year, quarter, &s)?
        .pop()
        .ok_or_else(|| AppError::Other("trimestre no válido".into()))?;
    refuse_unmapped_rates("303", &r.unmapped)?;
    let who = declarant(v)?;
    let mut opts = options_303(&s, quarter);
    let mut boxes: Vec<(u16, Money)> = r.boxes.iter().map(|(k, v)| (*k, *v)).collect();
    let mut warnings = r.warnings.clone();
    if periodo::ultimo(quarter) && s.exempt_from_390 {
        let e = exonerado_390(v, year, &s, &mut warnings)?;
        // Página 04 en el borrador: casillas propias (no coinciden con las de las páginas 01-03).
        let vol = &e.volume;
        let page4 = [
            (80, vol.general),
            (81, vol.cash_basis),
            (93, vol.intra),
            (94, vol.exports),
            (83, vol.exempt),
            (84, vol.not_subject),
            (125, vol.reverse_charge),
            (79, vol.real_estate),
            (99, vol.capital_goods),
        ];
        boxes.extend(page4.into_iter().filter(|(_, m)| !m.is_zero()));
        boxes.push((88, vol.total()));
        opts.exonerado_390 = Some(e);
    }
    let file = m303::file(&r, &who, &opts)?;
    Ok(Draft {
        model: "303".into(),
        year,
        quarter,
        boxes,
        payees: vec![],
        details: vec![],
        declaration: r.declaration.code().into(),
        result: r.result(),
        warnings,
        file_name: format!("{}_{year}_{}_303.txt", who.nif, periodo::codigo(quarter)),
        file,
        terceros: vec![],
    })
}

/// No se prepara un 303 o un 390 que dejaría fuera la cuota de facturas con un tipo sin casilla
/// (ver [`m303::unmapped_rates`]): mejor un error que diga qué facturas corregir que un modelo
/// que declara menos IVA del repercutido.
fn refuse_unmapped_rates(model: &str, unmapped: &[String]) -> Result<()> {
    if unmapped.is_empty() {
        return Ok(());
    }
    Err(AppError::Other(format!(
        "No se puede preparar el {model}: estas facturas tienen un tipo que no existe en el \
         modelo y su cuota quedaría sin declarar. Corrige el tipo de IVA (o de recargo) de cada \
         una: {}",
        unmapped.join("; ")
    )))
}

/// Cuenta del gasto (o ingreso) por impuesto sobre beneficios del PGC (PYMES y normal), que en
/// la cuenta de resultados va en «Impuestos sobre beneficios» y no en los gastos de la
/// actividad: 630 (impuesto corriente 6300 y diferido 6301), 633 «ajustes negativos en la
/// imposición sobre beneficios» (saldo deudor: más gasto, p. ej. más IS de ejercicios
/// anteriores) y 638 «ajustes positivos» (saldo acreedor: menos gasto). Su saldo deudor neto es
/// el gasto por IS que el 200 suma al resultado contable (art. 15.b LIS).
pub(crate) fn is_profit_tax(account: &str) -> bool {
    ["630", "633", "638"].iter().any(|p| account.starts_with(p))
}

/// Ingresos (grupo 7) y gastos (grupo 6) del diario hasta `end`, sin regularización ni cierre ni
/// el impuesto sobre beneficios (630, 633 y 638).
pub(crate) fn income_expenses(v: &Vault, year: i32, end: Date) -> Result<(Money, Money)> {
    let mut income = Money::ZERO;
    let mut expenses = Money::ZERO;
    for e in accounting::journal(v, year)? {
        if e.entry.date > end || matches!(e.entry.origin, Origin::Regularizacion | Origin::Cierre) {
            continue;
        }
        for l in &e.entry.lines {
            match l.account.group() {
                7 => income -= l.net(),
                6 if !is_profit_tax(l.account.as_str()) => expenses += l.net(),
                _ => {}
            }
        }
    }
    Ok((income, expenses))
}

/// Ingresos y gastos de la actividad económica de una persona física (130 y Renta) hasta `end`:
/// como [`income_expenses`], pero sin los asientos de criptomonedas (van a la base del ahorro),
/// ni los ingresos financieros (76) ni los resultados del inmovilizado (77 y 670–673), que no son
/// rendimientos de la actividad sino del capital o ganancias y pérdidas patrimoniales (art. 28.2
/// LIRPF).
pub(crate) fn activity_income_expenses(v: &Vault, year: i32, end: Date) -> Result<(Money, Money)> {
    let mut income = Money::ZERO;
    let mut expenses = Money::ZERO;
    for e in accounting::journal(v, year)? {
        if e.entry.date > end
            || matches!(
                e.entry.origin,
                Origin::Regularizacion | Origin::Cierre | Origin::Cripto(_) | Origin::Valores(_)
            )
        {
            continue;
        }
        for l in &e.entry.lines {
            let a = l.account.as_str();
            let fuera = |p: &[&str]| p.iter().any(|x| a.starts_with(x));
            match l.account.group() {
                7 if !fuera(&["76", "77"]) => income -= l.net(),
                6 if !is_profit_tax(a) && !fuera(&["670", "671", "672", "673"]) => {
                    expenses += l.net()
                }
                _ => {}
            }
        }
    }
    Ok((income, expenses))
}

/// Saldo acreedor de las cuentas 760 (ingresos de participaciones en instrumentos de patrimonio)
/// del ejercicio, sin regularización ni cierre: los dividendos cobrados de las participadas.
pub(crate) fn dividends_760(v: &Vault, year: i32) -> Result<Money> {
    let mut total = Money::ZERO;
    for e in accounting::journal(v, year)? {
        if matches!(e.entry.origin, Origin::Regularizacion | Origin::Cierre) {
            continue;
        }
        for l in e
            .entry
            .lines
            .iter()
            .filter(|l| l.account.as_str().starts_with("760"))
        {
            total -= l.net();
        }
    }
    Ok(total)
}

/// Retenciones e ingresos a cuenta soportados en el ejercicio para el 200 [01766], desde la
/// cuenta 473 «Hacienda Pública, retenciones y pagos a cuenta» del diario: así entran también
/// las que no vienen de una factura emitida (intereses del banco, alquileres cobrados…) y las
/// de las facturas (que se contabilizan en la 473 al emitirlas, `post_issued`) sólo una vez.
/// La 473 recoge también los pagos fraccionados (202), que el 200 resta aparte: se excluyen
/// los asientos de pago de modelos (registrados en TaxPain o 473 contra tesorería), el de
/// apertura (saldos de otros ejercicios), la regularización y el cierre, y los de liquidación
/// del propio IS, que saldan la 473 contra la 630/633/638 o contra la 4752 (cuota a pagar) o la
/// 4709 (a devolver).
pub(crate) fn withholdings_473(v: &Vault, year: i32) -> Result<Money> {
    withholdings_473_until(v, year, None)
}

/// Como [`withholdings_473`], pero sólo los asientos hasta `end` (pagos fraccionados del 202 por
/// la base del periodo, art. 40.3 LIS).
pub(crate) fn withholdings_473_until(v: &Vault, year: i32, end: Option<Date>) -> Result<Money> {
    let paid: BTreeSet<i64> = {
        let mut stmt = v
            .connection()
            .prepare("SELECT entry_id FROM tax_payment WHERE entry_id IS NOT NULL")?;
        let rows = stmt.query_map([], |r| r.get::<_, i64>(0))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let treasury = |a: &str| ["57", "52", "55"].iter().any(|p| a.starts_with(p));
    let mut total = Money::ZERO;
    for e in accounting::journal(v, year)? {
        if paid.contains(&e.id)
            || end.is_some_and(|d| e.entry.date > d)
            || matches!(
                e.entry.origin,
                Origin::Apertura | Origin::Regularizacion | Origin::Cierre | Origin::PagoModelo(_)
            )
        {
            continue;
        }
        let lines = &e.entry.lines;
        let on_473 = |a: &str| a.starts_with("473");
        let settles_tax =
            |a: &str| is_profit_tax(a) || a.starts_with("4752") || a.starts_with("4709");
        if !lines.iter().any(|l| on_473(l.account.as_str()))
            || lines.iter().any(|l| settles_tax(l.account.as_str()))
            || lines
                .iter()
                .filter(|l| !on_473(l.account.as_str()))
                .all(|l| treasury(l.account.as_str()))
        {
            continue;
        }
        total += lines
            .iter()
            .filter(|l| on_473(l.account.as_str()))
            .map(|l| l.net())
            .sum::<Money>();
    }
    Ok(total)
}

/// Retenciones soportadas en las emitidas entre `start` y `end` (ejercicio no natural).
pub(crate) fn issued_withholdings_between(v: &Vault, start: Date, end: Date) -> Result<Money> {
    Ok(issued_of_year(v, end.year())?
        .values()
        .filter(|st| (start..=end).contains(&st.invoice.issue_date))
        .map(|st| st.totals.withholding)
        .sum())
}

/// Retenciones soportadas en las emitidas del año hasta `end`.
pub(crate) fn issued_withholdings(v: &Vault, year: i32, end: Date) -> Result<Money> {
    Ok(issued_of_year(v, year)?
        .values()
        .filter(|st| st.invoice.issue_date.year() == year && st.invoice.issue_date <= end)
        .map(|st| st.totals.withholding)
        .sum())
}

/// Ejercicios con algún 130 presentado antes de existir la modalidad (migración 0046).
const IRPF_REGIME_PENDING_KEY: &str = "models.irpf_regime_pending";

/// Modalidad de estimación directa del ejercicio (RETENCIONES-02): la indicada en «Datos de los
/// modelos»; si no hay, simplificada (la de casi todos los autónomos), salvo en los ejercicios
/// que ya tenían algún 130 presentado antes de esta versión: ahí se sigue como hasta ahora
/// (normal) y se avisa, para no cambiar nada en silencio. Devuelve también el aviso, si lo hay.
pub fn irpf_regime(
    v: &Vault,
    s: &ModelSettings,
    year: i32,
) -> Result<(IrpfRegime, Option<String>)> {
    let (ys, _) = s.year_settings(year);
    if let Some(r) = ys.irpf_regime {
        return Ok((r, None));
    }
    let pending: Vec<i32> = v.setting(IRPF_REGIME_PENDING_KEY)?.unwrap_or_default();
    if pending.contains(&year) {
        return Ok((
            IrpfRegime::Normal,
            Some(format!(
                "Falta la modalidad de estimación directa de {year}. Como ya tenías modelos 130 \
                 de {year} presentados, se sigue calculando como hasta ahora (normal, sin el 5 % \
                 de gastos de difícil justificación). Si estás en simplificada (lo habitual si \
                 facturas menos de 600.000 € al año), indícalo en «Datos de los modelos»: el \
                 siguiente 130 recogerá la deducción de todo el año y pagarás menos."
            )),
        ));
    }
    Ok((
        IrpfRegime::Simplificada,
        Some(format!(
            "Calculado en estimación directa simplificada (la habitual). Si en {year} estás en \
             la normal, cámbialo en «Datos de los modelos»."
        )),
    ))
}

fn compute_130(v: &Vault, year: i32, quarter: u8, s: &ModelSettings) -> Result<Result130> {
    let (ys, legacy) = s.year_settings(year);
    // Rendimiento neto del año anterior: el indicado o, si TaxPain tiene la contabilidad de ese
    // año, el que sale de ella.
    let mut notes = Vec::new();
    if legacy {
        notes.push(LEGACY_YEAR_WARNING.to_owned());
    }
    let (regime, regime_note) = irpf_regime(v, s, year)?;
    notes.extend(regime_note);
    let previous_net = match ys.previous_year_net {
        Some(n) => Some(n),
        None => {
            let prev = year - 1;
            let journal = accounting::journal(v, prev)?;
            let first = journal.iter().map(|e| e.entry.date).min();
            if let Some(first) = first {
                let end = Date::from_calendar_date(prev, Month::December, 31)
                    .map_err(|e| AppError::Other(e.to_string()))?;
                let (income, expenses) = activity_income_expenses(v, prev, end)?;
                let mut net = income - expenses;
                // En simplificada, el rendimiento neto ya descuenta el 5 % de gastos de difícil
                // justificación (art. 30.2.4.ª LIRPF).
                if irpf_regime(v, s, prev)?.0 == IrpfRegime::Simplificada {
                    net -= taxpain_models::m100::hard_to_justify(prev, net);
                }
                notes.push(format!(
                    "Rendimiento neto de {prev} calculado con tu contabilidad: {net} (si no es \
                     el de tu renta, indícalo en «Datos de los modelos»)"
                ));
                // Si TaxPain sólo tiene parte del año, el rendimiento sale menor y la deducción
                // de la casilla [13] podría no corresponder.
                if first.month() != Month::January {
                    notes.push(format!(
                        "La contabilidad de {prev} en TaxPain empieza el {}: si la actividad ya \
                         existía antes, ese rendimiento es parcial; indica el de tu renta en \
                         «Datos de los modelos» (cuenta para la deducción de la casilla [13])",
                        first
                            .format(&time::macros::format_description!("[day]/[month]/[year]"))
                            .unwrap_or_default()
                    ));
                }
                Some(net)
            } else {
                // Sin contabilidad del año anterior ni dato tecleado no se aplica la minoración
                // de la casilla [13] (art. 110.3.c RIRPF). Si no hubo actividad, el rendimiento
                // anterior se considera cero y corresponden 100 € (instrucciones del 130,
                // casilla 13): hay que decírselo al usuario.
                notes.push(format!(
                    "Sin el rendimiento neto de {prev} no se aplica la minoración de la casilla \
                     [13]; si no tuviste actividad ese año, indica 0 en «Datos de los modelos» \
                     (te corresponden 100 €)"
                ));
                None
            }
        }
    };
    let mut previous = Vec::new();
    let mut last = None;
    for q in 1..=quarter {
        let (_, end) = quarter_range(year, q)?;
        let (income, expenses) = activity_income_expenses(v, year, end)?;
        // La amortización del ejercicio se contabiliza al cierre: hasta entonces se estima la
        // devengada hasta el fin del trimestre (art. 28 LIRPF; rendimiento acumulado del año).
        let expenses = expenses + accounting::accrued_depreciation(v, year, end)?;
        let input = Input130 {
            income,
            expenses,
            withholdings: issued_withholdings(v, year, end)?,
            previous: previous.clone(),
            previous_year_net: previous_net,
            iban: s.iban.clone().filter(|_| s.domiciliar),
            adjustments: adjustments(v, "130", year, q)?,
            simplified: regime == IrpfRegime::Simplificada,
        };
        let r = m130::compute(year, q, &input);
        // Para los trimestres siguientes cuentan las casillas presentadas (desde TaxPain o por
        // el gestor), si las hay.
        let filed = filed_boxes(v, "130", year, &format!("{q}T"))?;
        let get = |b: u16| match &filed {
            Some((_, boxes)) => boxes.get(&b).copied().unwrap_or(Money::ZERO),
            None => r.get(b),
        };
        previous.push(Previous130 {
            box07: get(7),
            box15: get(15),
            box16: get(16),
            box19: get(19),
        });
        last = Some(r);
    }
    let mut r = last.ok_or_else(|| AppError::Other("trimestre no válido".into()))?;
    r.warnings.extend(notes);
    Ok(r)
}

/// Borrador del 130 (sólo personas físicas en estimación directa).
pub fn draft_130(v: &Vault, year: i32, quarter: u8) -> Result<Draft> {
    let who = declarant(v)?;
    if !Nif::parse(&who.nif).is_ok_and(|n| n.is_persona_fisica()) {
        return Err(AppError::Other(
            "el modelo 130 es para personas físicas (autónomos)".into(),
        ));
    }
    let s = settings(v)?;
    let r = compute_130(v, year, quarter, &s)?;
    let iban = s.iban.as_deref().filter(|_| s.domiciliar);
    let file = m130::file(&r, &who, iban)?;
    Ok(Draft {
        model: "130".into(),
        year,
        quarter,
        boxes: r.boxes.iter().map(|(k, v)| (*k, *v)).collect(),
        payees: vec![],
        details: vec![],
        declaration: r.declaration.code().into(),
        result: r.get(19),
        warnings: r.warnings,
        file_name: format!("{}_{year}_{quarter}T_130.txt", who.nif),
        file,
        terceros: vec![],
    })
}

/// Nóminas de un diario importado (del gestor) del periodo: los asientos con sueldos (640) en
/// el debe; el IRPF es lo que abonan a la 4751 esos mismos asientos (las retenciones de
/// profesionales van en asientos sin 640). Los perceptores se cuentan por las subcuentas de
/// remuneraciones pendientes (465) o, si no las hay, como uno: hay que revisarlos.
fn imported_payroll(v: &Vault, year: i32, start: Date, end: Date) -> Result<Vec<Payment>> {
    let mut out = Vec::new();
    for e in accounting::journal(v, year)? {
        if !matches!(e.entry.origin, Origin::Importado(_) | Origin::Gestor(_))
            || e.entry.date < start
            || e.entry.date > end
        {
            continue;
        }
        let sum = |code: &str, debit: bool| -> Money {
            e.entry
                .lines
                .iter()
                .filter(|l| l.account.starts_with(code))
                .map(|l| if debit { l.net() } else { -l.net() })
                .sum()
        };
        let gross = sum("640", true);
        if gross.cents() <= 0 {
            continue;
        }
        let payees: BTreeSet<&str> = e
            .entry
            .lines
            .iter()
            .filter(|l| l.account.starts_with("465") && l.net().is_negative())
            .map(|l| l.account.as_str())
            .collect();
        let mut payees = payees.into_iter();
        out.push(Payment {
            payee: payees
                .next()
                .unwrap_or("Nóminas del diario importado")
                .to_owned(),
            base: gross,
            withheld: sum("4751", false),
        });
        // El resto de perceptores del asiento, para contarlos (los importes van en el primero).
        out.extend(payees.map(|p| Payment {
            payee: p.to_owned(),
            base: Money::ZERO,
            withheld: Money::ZERO,
        }));
    }
    Ok(out)
}

/// Nóminas del trimestre: (empleado, bruto 640, IRPF 4751). Las registradas en Nóminas van
/// con el NIF del empleado; los asientos de nómina hechos a mano, con el nombre del concepto.
/// Si en el periodo no hay ninguna de TaxPain, las del diario importado del gestor (y un aviso
/// en `warnings`); si hay de las dos, sólo cuentan las de TaxPain (las mismas no se suman dos
/// veces).
fn payroll(
    v: &Vault,
    year: i32,
    start: Date,
    end: Date,
    warnings: &mut Vec<String>,
) -> Result<Vec<Payment>> {
    let mut out = own_payroll(v, year, start, end)?;
    let imported = imported_payroll(v, year, start, end)?;
    if !imported.is_empty() {
        if out.is_empty() {
            let payees: BTreeSet<&str> = imported.iter().map(|p| p.payee.as_str()).collect();
            warnings.push(format!(
                "Nóminas sacadas del diario importado del gestor (sueldos 640 e IRPF 4751): \
                 {} perceptor(es) contados por sus subcuentas 465; revisa la casilla [01]",
                payees.len()
            ));
            out = imported;
        } else {
            warnings.push(
                "Hay nóminas en el diario importado y en TaxPain en el mismo periodo: sólo \
                 cuentan las de TaxPain"
                    .into(),
            );
        }
    }
    Ok(out)
}

/// Nóminas de TaxPain del periodo (registradas y asientos de nómina hechos a mano).
fn own_payroll(v: &Vault, year: i32, start: Date, end: Date) -> Result<Vec<Payment>> {
    let slips = crate::payroll::payslips(v.connection(), start, end)?;
    let mut out: Vec<Payment> = slips
        .iter()
        .map(|s| Payment {
            payee: s.nif.clone(),
            base: s.gross,
            withheld: s.irpf,
        })
        .collect();
    let registered: BTreeSet<i64> = slips.iter().filter_map(|s| s.entry_id).collect();
    for e in accounting::journal(v, year)? {
        if e.entry.origin != Origin::Nomina
            || e.entry.date < start
            || e.entry.date > end
            || registered.contains(&e.id)
        {
            continue;
        }
        let employee = e
            .entry
            .concept
            .strip_prefix("Nómina ")
            .unwrap_or(&e.entry.concept)
            .to_owned();
        let sum = |code: &str, debit: bool| -> Money {
            e.entry
                .lines
                .iter()
                .filter(|l| l.account.starts_with(code))
                .map(|l| if debit { l.net() } else { -l.net() })
                .sum()
        };
        out.push(Payment {
            payee: employee,
            base: sum("640", true),
            withheld: sum("4751", false),
        });
    }
    Ok(out)
}

fn withholding_draft(v: &Vault, r: &ResultW, warnings: Vec<String>) -> Result<Draft> {
    let s = settings(v)?;
    let who = declarant(v)?;
    let iban = s.iban.as_deref().filter(|_| s.domiciliar);
    let file = withholding::file(r, &who, iban)?;
    Ok(Draft {
        model: r.model.into(),
        year: r.year,
        quarter: r.quarter,
        boxes: r.boxes.iter().map(|(k, v)| (*k, *v)).collect(),
        payees: r.payees.clone(),
        details: vec![],
        declaration: r.declaration.code().into(),
        result: r.result(),
        warnings,
        file_name: format!(
            "{}_{}_{}_{}.txt",
            who.nif,
            r.year,
            periodo::codigo(r.quarter),
            r.model
        ),
        file,
        terceros: vec![],
    })
}

pub fn draft_111(v: &Vault, year: i32, quarter: u8) -> Result<Draft> {
    let (start, end) = quarter_range(year, quarter)?;
    let s = settings(v)?;
    let professionals: Vec<Payment> = received_by(v, start, end, Basis::Payment)?
        .into_iter()
        .filter(|r| !r.withholding.is_zero() && !is_rent(r))
        .map(|r| Payment {
            payee: r.nif,
            base: r.base,
            withheld: r.withholding,
        })
        .collect();
    let mut warnings = Vec::new();
    let work = payroll(v, year, start, end, &mut warnings)?;
    let (travel_note, travellers) = travel_taxable_warning(v, start, end)?;
    if !work.is_empty() && warnings.is_empty() {
        let employees: BTreeSet<&str> = work.iter().map(|p| p.payee.as_str()).collect();
        warnings.push(format!(
            "Perceptores del trabajo contados por el concepto de las nóminas: {}",
            employees.len()
        ));
    }
    warnings.extend(travel_note);
    let iban = s.domiciliar && s.iban.is_some();
    let mut r = withholding::compute_111(
        year,
        quarter,
        &Input111 {
            work,
            professionals,
        },
        iban,
    );
    warnings.extend(withholding::adjust(
        &mut r,
        &adjustments(v, "111", year, quarter)?,
        iban,
    ));
    let mut d = withholding_draft(v, &r, warnings)?;
    d.terceros = travellers;
    Ok(d)
}

pub fn draft_115(v: &Vault, year: i32, quarter: u8) -> Result<Draft> {
    let (start, end) = quarter_range(year, quarter)?;
    let s = settings(v)?;
    let received: Vec<Received> = received_by(v, start, end, Basis::Payment)?
        .into_iter()
        .filter(is_rent)
        .collect();
    // Sólo se avisa de las deducidas por la retención: las marcadas como alquiler ya se revisaron.
    let guessed = received.iter().any(|r| r.nature != "alquiler");
    let rents: Vec<Payment> = received
        .into_iter()
        .map(|r| Payment {
            payee: r.nif,
            base: r.base,
            withheld: r.withholding,
        })
        .collect();
    let mut warnings = if guessed {
        vec!["Alquileres detectados por la retención del 19 %: compruébalos".to_owned()]
    } else {
        vec![]
    };
    warnings.extend(rent_without_withholding(v, year, start, end)?);
    let iban = s.domiciliar && s.iban.is_some();
    let mut r = withholding::compute_115(year, quarter, &rents, iban);
    warnings.extend(withholding::adjust(
        &mut r,
        &adjustments(v, "115", year, quarter)?,
        iban,
    ));
    withholding_draft(v, &r, warnings)
}

/// Avisos del 115 por los alquileres marcados como tales y pagados sin retención en el
/// trimestre, cuyo arrendador no tiene acreditación vigente para no retener y cobra más de 900 €
/// en el año (art. 75.3.g RIRPF; `inmuebles::aviso_sin_retencion`).
fn rent_without_withholding(v: &Vault, year: i32, start: Date, end: Date) -> Result<Vec<String>> {
    let (y0, _) = quarter_range(year, 1)?;
    let (_, y1) = quarter_range(year, 4)?;
    let unwithheld: Vec<Received> = received_by(v, y0, y1, Basis::Payment)?
        .into_iter()
        .filter(|r| r.nature == "alquiler" && r.withholding.is_zero())
        .collect();
    let names = supplier_names(v)?;
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for r in unwithheld
        .iter()
        .filter(|r| r.date >= start && r.date <= end)
    {
        if !seen.insert(r.nif.clone()) {
            continue;
        }
        let annual: Money = unwithheld
            .iter()
            .filter(|x| x.nif == r.nif)
            .map(|x| x.base)
            .sum();
        let name = names.get(&r.nif).map_or("Arrendador", String::as_str);
        out.extend(crate::inmuebles::aviso_sin_retencion(
            v.connection(),
            &r.nif,
            name,
            r.date,
            annual,
        )?);
    }
    Ok(out)
}

/// Entregas (E5) y servicios (N2) a operadores de la UE devengados en un trimestre: la cifra del
/// art. 81.2 RIVA que decide si el 349 es trimestral o mensual.
pub(crate) fn intra_quarter_total(v: &Vault, year: i32, quarter: u8) -> Result<Money> {
    use taxpain_core::invoice::{Exemption, NotSubject, VatTreatment};
    let (start, end) = quarter_range(year, quarter)?;
    let mut total = Money::ZERO;
    for st in &issued_of_years(v, year - 1, year + 1)? {
        let d = st.accrual();
        if d < start || d > end {
            continue;
        }
        for t in &st.totals.breakdown {
            let goods = match t.vat {
                VatTreatment::Exempt(Exemption::E5) => true,
                VatTreatment::NotSubject(NotSubject::N2) => false,
                _ => continue,
            };
            if eu_operator(&st.invoice, goods).is_some() {
                total += t.base;
            }
        }
    }
    Ok(total)
}

/// Aviso del art. 81.2 RIVA (redacción del RD 192/2010): el 349 es mensual si en el trimestre
/// en curso o en alguno de los cuatro anteriores las entregas y servicios intracomunitarios
/// superan 50.000 €. Sólo si el perfil fiscal no lo tiene ya como mensual.
fn warn_349_monthly(v: &Vault, year: i32, quarter: u8) -> Result<Option<String>> {
    use taxpain_core::calendar::{Periodicity, TaxpayerProfile};
    let profile: Option<TaxpayerProfile> = v.setting("taxpayer_profile")?;
    if profile.is_some_and(|p| p.intracomunitarias == Some(Periodicity::Mensual)) {
        return Ok(None);
    }
    let limit = Money::from_euros(50_000, 0);
    let (mut y, mut q) = (year, quarter);
    let mut over = Vec::new();
    for _ in 0..5 {
        if intra_quarter_total(v, y, q)? > limit {
            over.push(format!("{q}T {y}"));
        }
        (y, q) = if q == 1 { (y - 1, 4) } else { (y, q - 1) };
    }
    if over.is_empty() {
        return Ok(None);
    }
    Ok(Some(format!(
        "Las entregas y servicios a operadores de la UE superan 50.000 € en {}: el 349 debe \
         presentarse mensualmente desde el mes en que se supera, y mientras no pasen cuatro \
         trimestres por debajo (RIVA art. 81.2). Cambia la periodicidad del 349 a mensual en el \
         perfil fiscal.",
        over.join(", ")
    )))
}

/// 349 del trimestre: entregas intracomunitarias exentas (E5 → clave E) y servicios a
/// operadores de la UE no sujetos por localización (N2 con NIF-IVA → clave S).
/// Base de entregas (E) o servicios (S) a un operador en un trimestre, tal como la calcula el
/// 349 de ese periodo (sin las rectificaciones de periodos anteriores).
fn declared_349(
    v: &Vault,
    year: i32,
    quarter: u8,
    country: &str,
    vat_id: &str,
    key: taxpain_models::m349::Key,
) -> Result<Money> {
    use taxpain_core::invoice::{Exemption, NotSubject, VatTreatment};
    use taxpain_models::m349::{Key, split_vat_id};
    let (start, end) = quarter_range(year, quarter)?;
    let target = split_vat_id(country, vat_id);
    let mut total = Money::ZERO;
    for st in &issued_of_years(v, year - 1, year + 1)? {
        let d = st.accrual();
        if d < start || d > end || st.invoice.rectified.iter().any(|(_, o)| *o < start) {
            continue;
        }
        let Some((c, id)) = eu_operator(&st.invoice, key == Key::E) else {
            continue;
        };
        if split_vat_id(c, id) != target {
            continue;
        }
        for t in &st.totals.breakdown {
            let k = match t.vat {
                VatTreatment::Exempt(Exemption::E5) => Key::E,
                VatTreatment::NotSubject(NotSubject::N2) => Key::S,
                _ => continue,
            };
            if k == key {
                total += t.base;
            }
        }
    }
    Ok(total)
}

/// Aviso de una emitida E5 o N2 que no va en el 349 porque el destinatario no es un empresario
/// de otro Estado miembro con NIF-IVA (`None` si no hace falta avisar: servicios no sujetos a un
/// cliente de fuera de la UE identificado como tal, que se declaran sólo en [120] del 303).
fn not_eu_349(inv: &Invoice, key: taxpain_models::m349::Key) -> Option<String> {
    use taxpain_core::invoice::IdType;
    use taxpain_models::m349::Key;
    let id = &inv.recipient.as_ref()?.id;
    match (id, key) {
        (
            PartyId::Foreign {
                country,
                id_type: IdType::NifIva,
                ..
            },
            _,
        ) => Some(format!(
            "{}: el cliente tiene un NIF-IVA de «{country}», que no es otro Estado miembro de la \
             UE{}: no va en el 349; revisa su identificación",
            inv.number,
            if key == Key::E {
                " (una entrega exenta fuera de la UE es una exportación, E2)"
            } else {
                ""
            }
        )),
        (_, Key::E) => Some(format!(
            "{}: entrega intracomunitaria sin NIF-IVA de otro Estado miembro del destinatario: no \
             va en el 349",
            inv.number
        )),
        _ => None,
    }
}

pub fn draft_349(v: &Vault, year: i32, quarter: u8) -> Result<Draft> {
    use taxpain_core::invoice::{Exemption, NotSubject, VatTreatment};
    use taxpain_models::m349::{self, Key, Operation};
    let (start, end) = quarter_range(year, quarter)?;
    let mut ops = Vec::new();
    let mut rects = Vec::new();
    let mut warnings = Vec::new();
    // Por la fecha de devengo, como el 303: las operaciones van en el 349 del periodo en que se
    // devenga el IVA (art. 80.1 RIVA) y su total se cruza con [59].
    for st in &issued_of_years(v, year - 1, year + 1)? {
        let d = st.accrual();
        if d < start || d > end {
            continue;
        }
        let Some(recipient) = &st.invoice.recipient else {
            continue;
        };
        // Rectificativa por sustitución: lleva los importes correctos completos; en el 349 va lo
        // que cambia, su base menos la de las facturas que sustituye.
        let (breakdown, w) =
            crate::rectificativas_emitidas::net_breakdown(v.connection(), &st.invoice, &st.totals)?;
        warnings.extend(w);
        // Rectificación de una factura de un periodo anterior: en el 349 va como registro de
        // rectificación de ese periodo (art. 81 RIVA), no sumada al actual.
        if let Some((_, original)) = st.invoice.rectified.iter().find(|(_, d)| *d < start) {
            // El periodo rectificado, con la periodicidad del 349 que se prepara.
            let m0 = u8::from(original.month());
            let (y0, q0) = (
                original.year(),
                if periodo::mes(quarter).is_some() {
                    periodo::de_mes(m0)
                } else {
                    (m0 - 1) / 3 + 1
                },
            );
            for t in &breakdown {
                let key = match t.vat {
                    VatTreatment::Exempt(Exemption::E5) => Key::E,
                    VatTreatment::NotSubject(NotSubject::N2) => Key::S,
                    _ => continue,
                };
                let Some((country, id)) = eu_operator(&st.invoice, key == Key::E) else {
                    if let Some(w) = not_eu_349(&st.invoice, key) {
                        warnings.push(w);
                    }
                    continue;
                };
                rects.push(m349::Rectification {
                    country: country.to_owned(),
                    vat_id: id.to_owned(),
                    name: recipient.name.clone(),
                    key,
                    year: y0,
                    period: periodo::codigo(q0),
                    delta: t.base,
                    previous: declared_349(v, y0, q0, country, id, key)?,
                });
            }
            continue;
        }
        for t in &breakdown {
            let key = match t.vat {
                VatTreatment::Exempt(Exemption::E5) => Key::E,
                VatTreatment::NotSubject(NotSubject::N2) => Key::S,
                _ => continue,
            };
            match eu_operator(&st.invoice, key == Key::E) {
                Some((country, id)) => ops.push(Operation {
                    country: country.to_owned(),
                    vat_id: id.to_owned(),
                    name: recipient.name.clone(),
                    key,
                    base: t.base,
                }),
                None => warnings.extend(not_eu_349(&st.invoice, key)),
            }
        }
    }
    // Adquisiciones intracomunitarias (claves A, bienes, e I, servicios), por la fecha de
    // devengo (la de la factura), como [10]/[11] del 303 y el 349 del proveedor (art. 80 RIVA).
    let names = supplier_names(v)?;
    for r in received_by(v, start, end, Basis::SelfAssessedAccrual)? {
        if r.vat_regime != "intracomunitaria" {
            continue;
        }
        let country = r.country.clone().unwrap_or_default();
        if !taxpain_core::eu::is_other_member_state(&country, r.intra_goods) {
            warnings.push(format!(
                "{}: el proveedor es de «{country}», que no es otro Estado miembro de la UE: no \
                 va en el 349 (si es de fuera de la UE, márcala como inversión del sujeto pasivo)",
                r.number
            ));
            continue;
        }
        let vat_id = r
            .nif
            .strip_prefix(country.as_str())
            .unwrap_or(&r.nif)
            .to_owned();
        ops.push(Operation {
            country,
            vat_id,
            name: names.get(&r.nif).cloned().unwrap_or_default(),
            key: if r.intra_goods { Key::A } else { Key::I },
            base: r.base,
        });
    }
    let period = periodo::codigo(quarter);
    let mut r = m349::compute(year, &period, &ops);
    m349::add_rectifications(&mut r, &rects);
    warnings.extend(warn_349_monthly(v, year, periodo::trimestre(quarter))?);
    r.warnings.extend(warnings);
    // Los avisos nombran a los operadores (también a los que no entran en el 349).
    let mut terceros = Vec::new();
    for (name, country, vat_id) in ops
        .iter()
        .map(|o| (&o.name, &o.country, &o.vat_id))
        .chain(rects.iter().map(|x| (&x.name, &x.country, &x.vat_id)))
    {
        tercero(&mut terceros, name, &format!("{country}{vat_id}"));
        tercero(&mut terceros, "", vat_id);
    }
    for l in &r.lines {
        tercero(
            &mut terceros,
            &l.name,
            &format!("{}{}", l.country, l.vat_id),
        );
        tercero(&mut terceros, "", &l.vat_id);
    }
    let who = declarant(v)?;
    let file = m349::file(&r, &who, 1)?;
    Ok(Draft {
        model: "349".into(),
        year,
        quarter,
        boxes: vec![],
        payees: vec![],
        details: r
            .lines
            .iter()
            .map(|l| {
                (
                    format!(
                        "{} · {}{} · clave {}",
                        l.name,
                        l.country,
                        l.vat_id,
                        l.key.code()
                    ),
                    l.base,
                )
            })
            .chain(r.rectifications.iter().map(|l| {
                (
                    format!(
                        "{} · {}{} · clave {} · rectifica {} {} (antes {})",
                        l.name,
                        l.country,
                        l.vat_id,
                        l.key.code(),
                        l.period,
                        l.year,
                        l.previous
                    ),
                    l.rectified,
                )
            }))
            .collect(),
        // Informativa: no hay resultado a ingresar ni tipo de declaración.
        declaration: String::new(),
        result: r.total,
        warnings: r.warnings,
        file_name: format!("{}_{year}_{}_349.txt", who.nif, periodo::codigo(quarter)),
        file,
        terceros,
    })
}

/// 390 del ejercicio a partir de los cuatro 303 (`quarters`) y de las ventas de inmovilizado
/// del año (`sales`, de [`asset_sales_390`]), con sus avisos.
fn compute_390(
    v: &Vault,
    year: i32,
    s: &ModelSettings,
    quarters: &[Result303],
    sales: &AssetSales390,
) -> Result<(taxpain_models::m390::Result390, Vec<String>)> {
    use taxpain_models::m390::{self, Annual390};
    let (carried, _) = carried_into(v, year, s)?;
    let (start, _) = quarter_range(year, 1)?;
    let (_, end) = quarter_range(year, 4)?;
    let (issued, received, mut warnings, cash) = vat_inputs(v, year, start, end)?;
    refuse_unmapped_rates("390", &m303::unmapped_rates(&issued))?;
    let (prorata, _) = prorata_for(v, s, year, 4)?;
    let (sale_regularization, _) = crate::iva_holding::regularizacion_ventas(v, s, start, end)?;
    let extra = Annual390 {
        prorata,
        capital_goods_sale_regularization: sale_regularization,
        cash_basis: s.cash_basis,
        cash_recipient: cash.recipient,
        cash_issued_base: cash.issued_base,
        cash_issued_vat: cash.issued_vat,
        cash_received_base: cash.received_base,
        cash_received_vat: cash.received_vat,
        real_estate: sales.real_estate,
        capital_goods: sales.capital_goods,
    };
    let mut r = m390::compute_with(year, &issued, &received, quarters, carried, &extra);
    warnings.append(&mut r.warnings);
    Ok((r, warnings))
}

/// Ventas del inmovilizado que no cuentan en el volumen de operaciones del año (art. 121.Tres
/// LIVA): ya suman en su casilla de operaciones por su factura y se restan en [106] y [107] del
/// 390 (en [79] y [99] de la página 04 del 303 del 4T de un exonerado).
#[derive(Debug, Default)]
struct AssetSales390 {
    /// [106]: terrenos y construcciones («entregas de bienes inmuebles… no habituales»).
    real_estate: Money,
    /// [107]: los demás bienes de inversión (art. 108 LIVA).
    capital_goods: Money,
    /// De [106] y [107], lo de líneas con derecho a deducir (para la prorrata, que excluye las
    /// entregas de bienes de inversión de sus dos términos, art. 104.Tres.3.º LIVA).
    with_right: Money,
    /// Una línea por venta que se resta, con su factura (o la rectificativa que la cambia), para
    /// el borrador.
    details: Vec<(String, Money)>,
    /// Ventas que no se restan, o que se restan con dudas o en otro año, y por qué.
    warnings: Vec<String>,
}

/// Dónde se restan las ventas de inmovilizado, para nombrar sus casillas en los avisos: en el
/// 390 o en la página 04 del 303 del 4T de un exonerado del 390.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VolumeIn {
    M390,
    Exonerado303,
}

impl VolumeIn {
    /// Casilla de los inmuebles ([106] o [79]) o de los demás bienes de inversión ([107] o
    /// [99]).
    fn box_of(self, real_estate: bool) -> &'static str {
        match (self, real_estate) {
            (VolumeIn::M390, true) => "[106]",
            (VolumeIn::M390, false) => "[107]",
            (VolumeIn::Exonerado303, true) => "[79]",
            (VolumeIn::Exonerado303, false) => "[99]",
        }
    }

    fn volume(self) -> &'static str {
        match self {
            VolumeIn::M390 => "el volumen de operaciones del 390",
            VolumeIn::Exonerado303 => "el volumen de operaciones de la página 04",
        }
    }
}

/// Baja del inmovilizado que puede ir en [106] o [107]: la de un bien de inversión con precio de
/// venta o la vinculada a su factura de venta en Contabilidad › Inmovilizado.
struct AssetSale {
    /// `fixed_asset.id`.
    id: i64,
    name: String,
    account: String,
    /// Bien de inversión ([`capital_good`]). La venta vinculada que no lo es ocupa su línea de la
    /// factura, para que no la tome otra venta, pero no se resta.
    capital: bool,
    /// Terrenos y construcciones: [106]; los demás, [107].
    real_estate: bool,
    /// El precio de la baja; si está vinculada, lo que su factura de venta pasa a la venta (lo
    /// vinculado), que puede llevar líneas no sujetas.
    price: Money,
    date: Date,
    /// Fecha de alta: una factura anterior no puede ser su venta (como en `link_in`).
    acquired: Date,
    /// Su factura de venta vinculada (`invoice_issued.id`), sin las rectificativas.
    invoice: Option<i64>,
}

impl AssetSale {
    /// Orden de las ventas que no depende del de alta: inmuebles, bienes de inversión y las demás,
    /// y después cuenta, nombre, precio y fecha (el id sólo desempata dos ventas iguales).
    fn key(&self) -> impl Ord + '_ {
        (
            !self.real_estate,
            !self.capital,
            self.account.as_str(),
            self.name.as_str(),
            Reverse(self.price),
            self.date,
            self.id,
        )
    }
}

/// Bajas que pueden restarse del volumen, por fecha: las de bienes de inversión con precio de
/// venta y las vinculadas a su factura de venta (también las dadas de baja sin precio y
/// vinculadas después, y las que no son bienes de inversión). Bienes de inversión, sólo con lo
/// que guarda `fixed_asset` (cuenta, coste, fecha y precio de la baja):
/// - terrenos y construcciones (210, 211, 220 y 221) van a [106], como el almacén del supuesto
///   práctico del 390 en el Manual del IVA;
/// - los demás, a [107] si costaron 3.005,06 € o más (art. 108.Dos.5.º LIVA: no son bienes de
///   inversión los de menos de 500.000 pesetas), y si no, se quedan en el volumen;
/// - el intangible (20x) no es un bien corporal (art. 108.Uno LIVA) y sigue en el volumen.
fn asset_disposals(v: &Vault) -> Result<Vec<AssetSale>> {
    let mut stmt = v.connection().prepare(
        "SELECT a.id, a.name, a.account, a.cost, COALESCE(a.sale_price, 0), a.disposed_on,
                a.acquired_on, s.invoice_id, s.amount
           FROM fixed_asset a
           LEFT JOIN (SELECT l.asset_id, l.invoice_id, l.amount FROM asset_sale_invoice l
                        JOIN invoice_issued i ON i.id = l.invoice_id
                       WHERE i.status = 'emitida' AND i.kind NOT LIKE 'R%') s
             ON s.asset_id = a.id
          WHERE a.disposed_on IS NOT NULL
            AND (COALESCE(a.sale_price, 0) > 0 OR s.invoice_id IS NOT NULL)
          ORDER BY a.disposed_on, a.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            (r.get::<_, String>(1)?, r.get::<_, String>(2)?),
            r.get::<_, i64>(3)?,
            r.get::<_, i64>(4)?,
            (r.get::<_, String>(5)?, r.get::<_, String>(6)?),
            r.get::<_, Option<i64>>(7)?,
            r.get::<_, Option<i64>>(8)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, (name, account), cost, price, (date, acquired), invoice, linked) = row?;
        let class = capital_good(&account, Money::from_cents(cost));
        // Lo que pasa la factura de venta vinculada es positivo (ver `link_in`).
        let price = Money::from_cents(linked.unwrap_or(price));
        if (invoice.is_none() && class.is_none()) || price.cents() <= 0 {
            continue;
        }
        out.push(AssetSale {
            id,
            name,
            account,
            capital: class.is_some(),
            real_estate: class == Some(true),
            price,
            date: parse_date(&date)?,
            acquired: parse_date(&acquired)?,
            invoice,
        });
    }
    Ok(out)
}

/// Casilla de la venta de un elemento del inmovilizado: `Some(true)` si son terrenos o
/// construcciones ([106]), `Some(false)` si es otro bien de inversión ([107]) y `None` si sigue en
/// el volumen (el intangible y lo que costó menos de 3.005,06 €). Ver [`asset_disposals`].
fn capital_good(account: &str, cost: Money) -> Option<bool> {
    /// Coste mínimo de un bien de inversión: 500.000 pesetas (art. 108.Dos.5.º LIVA).
    const MIN_COST: Money = Money::from_cents(300_506);
    let real_estate = ["210", "211", "220", "221"]
        .iter()
        .any(|p| account.starts_with(p));
    let corporeal = account.starts_with("21") || account.starts_with("22");
    (real_estate || (corporeal && cost >= MIN_COST)).then_some(real_estate)
}

/// Último día en que se busca la factura de una baja del día `d`: el 31 de diciembre del año
/// siguiente.
fn last_issue_day(d: Date) -> Date {
    Date::from_calendar_date(d.year() + 1, Month::December, 31).unwrap_or(d)
}

/// Emitidas no anuladas expedidas del día `from` al `to`.
fn issued_between(v: &Vault, from: Date, to: Date) -> Result<Vec<StoredIssued>> {
    let mut stmt = v.connection().prepare(
        "SELECT id, data FROM invoice_issued
          WHERE status = 'emitida' AND issue_date BETWEEN ?1 AND ?2",
    )?;
    let rows = stmt.query_map([from.to_string(), to.to_string()], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, data) = row?;
        out.push(stored_issued(id, &data)?);
    }
    Ok(out)
}

/// Emitidas en las que buscar la factura de las ventas `sales` (ver [`match_asset_sales`]): las
/// expedidas desde 45 días antes de cada baja hasta el final del año siguiente y, sean de cuando
/// sean, las de venta vinculadas. Por orden de alta.
fn sale_invoices(v: &Vault, sales: &[AssetSale]) -> Result<Vec<StoredIssued>> {
    let margin = time::Duration::days(SALE_INVOICE_DAYS);
    let mut ranges: Vec<(Date, Date)> = sales
        .iter()
        .map(|s| (s.date - margin, last_issue_day(s.date)))
        .collect();
    ranges.sort();
    let mut merged: Vec<(Date, Date)> = Vec::new();
    for (from, to) in ranges {
        match merged.last_mut() {
            Some(last) if from <= last.1 => last.1 = last.1.max(to),
            _ => merged.push((from, to)),
        }
    }
    let mut out = Vec::new();
    for (from, to) in merged {
        out.extend(issued_between(v, from, to)?);
    }
    for id in sales.iter().filter_map(|s| s.invoice) {
        if out.iter().any(|st| st.id == id) {
            continue;
        }
        let data: Option<String> = v
            .connection()
            .query_row(
                "SELECT data FROM invoice_issued WHERE id = ?1 AND status = 'emitida'",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(data) = data {
            out.push(stored_issued(id, &data)?);
        }
    }
    out.sort_by_key(|st| st.id);
    out.dedup_by_key(|st| st.id);
    Ok(out)
}

/// Lo que cada rectificativa pasa a cada venta en Contabilidad › Inmovilizado, con su signo:
/// (elemento, rectificativa) → importe.
fn rectifying_links(v: &Vault) -> Result<BTreeMap<(i64, i64), Money>> {
    let mut stmt = v.connection().prepare(
        "SELECT l.asset_id, l.invoice_id, l.amount FROM asset_sale_invoice l
           JOIN invoice_issued i ON i.id = l.invoice_id
          WHERE i.status = 'emitida' AND i.kind LIKE 'R%'",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            (r.get::<_, i64>(0)?, r.get::<_, i64>(1)?),
            Money::from_cents(r.get::<_, i64>(2)?),
        ))
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Días como mucho entre la baja y el devengo de su factura: el plazo más largo para expedirla
/// es hasta el día 15 del mes siguiente (art. 11 RD 1619/2012).
const SALE_INVOICE_DAYS: i64 = 45;

/// La línea `i` de `st` es sujeta (también la exenta): puede ir en [106] y [107].
fn subject_line(st: &StoredIssued, i: usize) -> bool {
    !matches!(st.invoice.lines[i].vat, VatTreatment::NotSubject(_))
}

fn rectifying(st: &StoredIssued) -> bool {
    matches!(st.invoice.kind, InvoiceKind::Rectificativa(_))
}

/// Base de cada línea de `st` con los descuentos de la factura: sus líneas negativas (que no son
/// de anticipo) rebajan las positivas del mismo tipo, sujetas o no sujetas. La que nombra unas
/// líneas («Descuento de la consultoría») rebaja ésas, hasta su base, porque es el descuento de esa
/// operación (art. 78.Tres.2.º LIVA); las demás, y lo que no quepa, todas en proporción. Cero en
/// las de anticipo y en las negativas.
fn net_bases(st: &StoredIssued) -> Vec<Money> {
    use taxpain_core::invoice::totals::line_base;
    let lines = &st.invoice.lines;
    let mut out = vec![Money::ZERO; lines.len()];
    for subject in [true, false] {
        let of_kind: Vec<usize> = (0..lines.len())
            .filter(|i| !lines[*i].advance && subject_line(st, *i) == subject)
            .collect();
        let positive: Vec<usize> = of_kind
            .iter()
            .copied()
            .filter(|i| line_base(&lines[*i]).cents() > 0)
            .collect();
        for i in &positive {
            out[*i] = line_base(&lines[*i]);
        }
        let mut cut = Money::ZERO;
        for n in of_kind
            .iter()
            .copied()
            .filter(|i| line_base(&lines[*i]).is_negative())
        {
            let mut m = line_base(&lines[n]);
            let named: Vec<usize> = positive
                .iter()
                .copied()
                .filter(|i| names_line(&lines[n].description, &lines[*i].description))
                .collect();
            let weights: Vec<Money> = named.iter().map(|i| out[*i]).collect();
            let here = m.max(-weights.iter().copied().sum::<Money>());
            for (i, d) in named.iter().zip(prorate(here, &weights)) {
                out[*i] += d;
            }
            m -= here;
            cut += m;
        }
        let weights: Vec<Money> = positive.iter().map(|i| out[*i]).collect();
        let cut = cut.max(-weights.iter().copied().sum::<Money>());
        for (i, d) in positive.iter().zip(prorate(cut, &weights)) {
            out[*i] += d;
        }
    }
    out
}

/// Por qué no se sabe seguro que una línea es la de la venta que se resta.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Doubt {
    /// Sin vincular: la línea tiene su base pero no nombra el elemento.
    Unnamed,
    /// Vinculada: ninguna línea tiene lo vinculado ni nombra el elemento, y se toma la sujeta
    /// mayor.
    Largest,
    /// Vinculada: varias líneas tienen lo vinculado y ninguna nombra el elemento.
    Several,
    /// Vinculada por más de lo que tienen las líneas que nombran el elemento: también se restan
    /// estas otras.
    Extra(Vec<usize>),
}

/// La factura de una venta del inmovilizado.
enum SaleInvoice {
    /// La línea `line` de la emitida `id` (si la venta está vinculada, la mayor de las suyas);
    /// `price`, lo de la venta que está en el volumen en esa línea (o en las suyas), lo que se
    /// resta; `doubt`, si no se sabe seguro que es su línea.
    Line {
        id: i64,
        line: usize,
        /// Todas sus líneas: las de la vinculada que lleva varias, o la suya.
        lines: Vec<usize>,
        price: Money,
        doubt: Option<Doubt>,
    },
    /// Su línea es no sujeta: no hay nada que restar, porque [106] y [107] son de entregas sujetas
    /// (instrucciones del 390).
    NotSubject,
    /// La línea `line` de la emitida `id` está vinculada a la venta `with` y le queda `open`, el
    /// precio de ésta (o más, si la nombra): para restarla hay que vincularla también.
    Shared {
        id: i64,
        line: usize,
        open: Money,
        with: usize,
    },
    /// La línea `line` de la emitida `id` nombra el elemento y tiene más que su precio: quizá es
    /// su venta con otras cosas.
    Partial { id: i64, line: usize },
    /// Hay líneas con esa base, pero no valen: por qué no vale la más cercana.
    Unusable(String),
    /// Ninguna línea con esa base.
    Missing,
}

/// Lo que una venta vinculada toma de su factura (ver [`place_linked`]).
enum Placed {
    /// Sus líneas sujetas, la mayor primero, lo que se resta de ellas (`subject`) y lo vinculado que
    /// queda sin línea (`rest`).
    Lines {
        lines: Vec<usize>,
        subject: Money,
        rest: Money,
        doubt: Option<Doubt>,
    },
    NotSubject,
}

/// Lo que llevan de cada línea las ventas vinculadas y quién la lleva primero.
#[derive(Default)]
struct LineUse {
    used: BTreeMap<(i64, usize), Money>,
    owner: BTreeMap<(i64, usize), usize>,
}

impl LineUse {
    fn take(&mut self, at: (i64, usize), m: Money, k: usize) {
        *self.used.entry(at).or_default() += m;
        self.owner.entry(at).or_insert(k);
    }

    fn used(&self, at: (i64, usize)) -> Money {
        self.used.get(&at).copied().unwrap_or_default()
    }
}

/// Las líneas de cada venta vinculada a su factura en Contabilidad › Inmovilizado, en tres pasadas
/// (en cada una, factura a factura y en el orden de [`AssetSale::key`], que no depende del de alta):
/// 1. las que alguna línea nombra: la que tiene lo vinculado y si no las que lo nombran, de mayor a
///    menor, hasta lo vinculado (una venta puede ir en dos líneas, como una furgoneta y sus
///    estanterías); si la mayor es no sujeta, la venta también. Si lo vinculado pasa de esas
///    líneas, el resto está en las que no nombran otra venta: todas si suman ese resto, la que lo
///    tiene o, si no, las mayores, con duda;
/// 2. las demás, la línea con lo vinculado que no lleva otra venta (antes la sujeta y la que no
///    nombra otra venta; con duda si hay varias);
/// 3. las que quedan, la sujeta mayor con sitio (con duda si la factura tiene más líneas) y, si no
///    queda ninguna, la venta es no sujeta.
///
/// Ninguna línea lleva más que su base (con los descuentos, [`net_bases`]). Aquí sólo nombra la
/// descripción de la línea: la de la factura es de todas.
fn place_linked(
    sales: &[AssetSale],
    invoices: &[StoredIssued],
    nets: &BTreeMap<i64, Vec<Money>>,
    found: &mut [Option<SaleInvoice>],
    lines_use: &mut LineUse,
) {
    use taxpain_core::invoice::totals::line_base;
    let net = |st: &StoredIssued, i: usize| nets.get(&st.id).map_or(Money::ZERO, |n| n[i]);
    let mut linked: Vec<(usize, &StoredIssued)> = sales
        .iter()
        .enumerate()
        .filter_map(|(k, s)| {
            let id = s.invoice?;
            invoices.iter().find(|st| st.id == id).map(|st| (k, st))
        })
        .collect();
    linked.sort_by(|(a, x), (b, y)| {
        x.id.cmp(&y.id)
            .then_with(|| sales[*a].key().cmp(&sales[*b].key()))
    });
    let lines_of = |st: &StoredIssued| -> Vec<usize> {
        (0..st.invoice.lines.len())
            .filter(|i| !st.invoice.lines[*i].advance && net(st, *i).cents() > 0)
            .collect()
    };
    let room = |u: &LineUse, st: &StoredIssued, i: usize| net(st, i) - u.used((st.id, i));
    let describes =
        |st: &StoredIssued, i: usize, name: &str| names(&st.invoice.lines[i].description, name);
    // La línea nombra otra venta que puede ser de esta factura.
    let other_sale = |st: &StoredIssued, i: usize, k: usize| {
        let accrual = st.accrual();
        sales.iter().enumerate().any(|(j, o)| {
            j != k
                && (o.invoice == Some(st.id)
                    || (o.date - accrual).whole_days().abs() <= SALE_INVOICE_DAYS)
                && describes(st, i, &o.name)
        })
    };
    let mut placed: BTreeMap<usize, Placed> = BTreeMap::new();
    // 1. Las que alguna línea nombra.
    for &(k, st) in &linked {
        let s = &sales[k];
        let exact = |i: usize| line_base(&st.invoice.lines[i]) == s.price || net(st, i) == s.price;
        let mut named: Vec<usize> = lines_of(st)
            .into_iter()
            .filter(|i| describes(st, *i, &s.name))
            .collect();
        named.sort_by_key(|i| (!exact(*i), Reverse(net(st, *i)), *i));
        let Some(&first) = named.first() else {
            continue;
        };
        if !subject_line(st, first) {
            let m = s.price.min(room(lines_use, st, first));
            if m.cents() > 0 {
                lines_use.take((st.id, first), m, k);
            }
            placed.insert(k, Placed::NotSubject);
            continue;
        }
        let (mut mine, mut subject, mut rest) = (Vec::new(), Money::ZERO, s.price);
        for i in named.into_iter().filter(|i| subject_line(st, *i)) {
            if rest.cents() <= 0 {
                break;
            }
            let here = rest.min(room(lines_use, st, i));
            if here.cents() <= 0 {
                continue;
            }
            lines_use.take((st.id, i), here, k);
            mine.push(i);
            subject += here;
            rest -= here;
        }
        if !mine.is_empty() {
            placed.insert(
                k,
                Placed::Lines {
                    lines: mine,
                    subject,
                    rest,
                    doubt: None,
                },
            );
        }
    }
    // Lo vinculado de más, en las líneas que no nombran otra venta.
    for &(k, st) in &linked {
        let Some(Placed::Lines {
            lines,
            subject,
            rest,
            doubt,
        }) = placed.get_mut(&k)
        else {
            continue;
        };
        if rest.cents() <= 0 {
            continue;
        }
        let name = &sales[k].name;
        let others: Vec<(usize, Money)> = lines_of(st)
            .into_iter()
            .filter(|i| !lines.contains(i) && !describes(st, *i, name) && !other_sale(st, *i, k))
            .map(|i| (i, room(lines_use, st, i)))
            .filter(|(_, r)| r.cents() > 0)
            .collect();
        let all: Money = others.iter().map(|(_, r)| *r).sum();
        let (pick, sure) = if all == *rest {
            (others, true)
        } else if let Some(one) = others.iter().find(|(_, r)| *r == *rest) {
            (vec![*one], true)
        } else {
            let mut by_size = others;
            by_size.sort_by_key(|(i, r)| (Reverse(*r), *i));
            let mut left = *rest;
            let mut pick = Vec::new();
            for (i, r) in by_size {
                if left.cents() <= 0 {
                    break;
                }
                let here = left.min(r);
                pick.push((i, here));
                left -= here;
            }
            (pick, false)
        };
        let mut extra = Vec::new();
        for (i, m) in pick {
            lines_use.take((st.id, i), m, k);
            *rest -= m;
            if subject_line(st, i) {
                *subject += m;
                lines.push(i);
                extra.push(i);
            }
        }
        if !sure && !extra.is_empty() {
            *doubt = Some(Doubt::Extra(extra));
        }
    }
    // 2. Las demás, la línea con lo vinculado.
    for &(k, st) in &linked {
        if placed.contains_key(&k) {
            continue;
        }
        let s = &sales[k];
        let mut exact: Vec<usize> = lines_of(st)
            .into_iter()
            .filter(|i| {
                (line_base(&st.invoice.lines[*i]) == s.price || net(st, *i) == s.price)
                    && lines_use.used((st.id, *i)).is_zero()
            })
            .collect();
        exact.sort_by_key(|i| (other_sale(st, *i, k), !subject_line(st, *i), *i));
        let Some(&i) = exact.first() else {
            continue;
        };
        let m = net(st, i).min(s.price);
        lines_use.take((st.id, i), m, k);
        placed.insert(
            k,
            if subject_line(st, i) {
                Placed::Lines {
                    lines: vec![i],
                    subject: m,
                    rest: s.price - m,
                    doubt: (exact.len() > 1).then_some(Doubt::Several),
                }
            } else {
                Placed::NotSubject
            },
        );
    }
    // 3. Las que quedan, la sujeta mayor con sitio.
    for &(k, st) in &linked {
        if placed.contains_key(&k) {
            continue;
        }
        let s = &sales[k];
        let all = lines_of(st);
        let best = all
            .iter()
            .copied()
            .filter(|i| subject_line(st, *i) && room(lines_use, st, *i).cents() > 0)
            .min_by_key(|i| (Reverse(room(lines_use, st, *i)), *i));
        placed.insert(
            k,
            match best {
                Some(i) => {
                    let here = s.price.min(room(lines_use, st, i));
                    lines_use.take((st.id, i), here, k);
                    Placed::Lines {
                        lines: vec![i],
                        subject: here,
                        rest: s.price - here,
                        doubt: (all.len() > 1).then_some(Doubt::Largest),
                    }
                }
                None => Placed::NotSubject,
            },
        );
    }
    for (k, p) in placed {
        let Some(id) = sales[k].invoice else {
            continue;
        };
        found[k] = Some(match p {
            Placed::Lines {
                lines,
                subject,
                doubt,
                ..
            } => SaleInvoice::Line {
                id,
                line: lines.first().copied().unwrap_or_default(),
                lines,
                price: subject,
                doubt,
            },
            Placed::NotSubject => SaleInvoice::NotSubject,
        });
    }
}

/// Emparejamiento de las ventas sin vincular con sus líneas (algoritmo de Kuhn): cada venta toma
/// la línea libre más cercana y, si no queda ninguna, la de otra venta de la misma pasada que
/// pueda tomar otra. Así ninguna se queda sin línea si hay una para cada una.
#[derive(Default)]
struct Matching {
    /// Línea → venta (posición) y pasada en que la tomó.
    holder: BTreeMap<(i64, usize), (usize, bool)>,
    /// Venta → su línea.
    chosen: BTreeMap<usize, (i64, usize)>,
}

impl Matching {
    fn augment(
        &mut self,
        p: usize,
        pass: bool,
        lines: &[Vec<(i64, usize)>],
        seen: &mut BTreeSet<(i64, usize)>,
    ) -> bool {
        if let Some(&line) = lines[p].iter().find(|l| !self.holder.contains_key(*l)) {
            self.assign(p, line, pass);
            return true;
        }
        for &line in &lines[p] {
            if !seen.insert(line) {
                continue;
            }
            let Some(&(h, h_pass)) = self.holder.get(&line) else {
                continue;
            };
            if h_pass == pass && h != p && self.augment(h, pass, lines, seen) {
                self.assign(p, line, pass);
                return true;
            }
        }
        false
    }

    fn assign(&mut self, p: usize, line: (i64, usize), pass: bool) {
        if let Some(old) = self.chosen.insert(p, line)
            && self.holder.get(&old).is_some_and(|(h, _)| *h == p)
        {
            self.holder.remove(&old);
        }
        self.holder.insert(line, (p, pass));
    }
}

/// Días de la baja de `s` al devengo de `st`, si `st` puede ser su factura: expedida desde el alta
/// del elemento hasta el final del año siguiente a la baja, y devenga a 45 días o menos de ella.
fn days_to(s: &AssetSale, st: &StoredIssued) -> Option<i64> {
    let issued = st.invoice.issue_date;
    let days = (st.accrual() - s.date).whole_days().abs();
    (issued >= s.acquired && issued <= last_issue_day(s.date) && days <= SALE_INVOICE_DAYS)
        .then_some(days)
}

/// Líneas de las emitidas que pueden ser la factura de `s` (ver [`days_to`]), con los días.
fn lines_near<'a>(
    s: &AssetSale,
    invoices: &'a [StoredIssued],
) -> Vec<(&'a StoredIssued, usize, i64)> {
    invoices
        .iter()
        .filter_map(|st| days_to(s, st).map(|d| (st, d)))
        .flat_map(|(st, d)| (0..st.invoice.lines.len()).map(move |i| (st, i, d)))
        .collect()
}

/// Líneas que pueden ser la factura de `s` con su base (o con la de su factura con descuentos,
/// [`net_bases`]), de la más cercana a la más lejana.
fn near_lines<'a>(
    s: &AssetSale,
    invoices: &'a [StoredIssued],
    nets: &BTreeMap<i64, Vec<Money>>,
) -> Vec<(&'a StoredIssued, usize, i64)> {
    use taxpain_core::invoice::totals::line_base;
    let net = |st: &StoredIssued, i: usize| nets.get(&st.id).map_or(Money::ZERO, |n| n[i]);
    let mut c: Vec<(&StoredIssued, usize, i64)> = lines_near(s, invoices)
        .into_iter()
        .filter(|(st, i, _)| {
            line_base(&st.invoice.lines[*i]) == s.price
                || (!rectifying(st) && net(st, *i) == s.price)
        })
        .collect();
    c.sort_by_key(|(st, i, days)| (*days, st.id, *i));
    c
}

/// Factura de cada venta de `sales`, en su orden (`None` si está vinculada a una que no está en
/// `invoices`). Se busca una vez para todas las ventas, sea cual sea el año del 390: así el 390 de
/// cada año ve las mismas.
///
/// Las vinculadas en Contabilidad › Inmovilizado van primero y sólo en su factura (ver
/// [`place_linked`]): sus líneas no las toma otra venta.
///
/// Para las demás vale la línea de una emitida expedida desde el alta del elemento hasta el final
/// del año siguiente a la baja que no sea de una rectificativa ni un anticipo, con la base igual al
/// precio de la baja (o con los descuentos de su factura, [`net_bases`]), que devengue a 45 días o
/// menos de ella y, si es no sujeta, que nombre el elemento. Primero las líneas que lo nombran;
/// después, la que nombra dos ventas o más aún sin línea y tiene la suma de sus precios (el terreno
/// y la construcción de un local), que es de todas; y luego las demás. En cada pasada, por orden de
/// baja, la más cercana (ver [`Matching`]). Cada línea es de una sola venta, salvo esa. Si no hay
/// ninguna, se dice por qué.
fn match_asset_sales(sales: &[AssetSale], invoices: &[StoredIssued]) -> Vec<Option<SaleInvoice>> {
    use taxpain_core::invoice::totals::line_base;
    let nets: BTreeMap<i64, Vec<Money>> =
        invoices.iter().map(|st| (st.id, net_bases(st))).collect();
    let net = |st: &StoredIssued, i: usize| nets.get(&st.id).map_or(Money::ZERO, |n| n[i]);
    let by_id: BTreeMap<i64, &StoredIssued> = invoices.iter().map(|st| (st.id, st)).collect();
    let mut found: Vec<Option<SaleInvoice>> = sales.iter().map(|_| None).collect();
    let mut lines_use = LineUse::default();
    place_linked(sales, invoices, &nets, &mut found, &mut lines_use);
    let lines_use = lines_use;
    let near = |s: &AssetSale| near_lines(s, invoices, &nets);
    let usable = |st: &StoredIssued, i: usize| {
        !rectifying(st)
            && !st.invoice.lines[i].advance
            && net(st, i).cents() > 0
            && !lines_use.used.contains_key(&(st.id, i))
    };
    let mut unlinked: Vec<usize> = (0..sales.len())
        .filter(|k| sales[*k].invoice.is_none())
        .collect();
    unlinked.sort_by(|a, b| {
        let (a, b) = (&sales[*a], &sales[*b]);
        a.date.cmp(&b.date).then_with(|| a.key().cmp(&b.key()))
    });
    // Candidatas de cada una (posición en `unlinked`) y si la nombran.
    let cands: Vec<Vec<(i64, usize, bool)>> = unlinked
        .iter()
        .map(|k| {
            let s = &sales[*k];
            near(s)
                .into_iter()
                .filter(|(st, i, _)| usable(st, *i))
                .map(|(st, i, _)| (st, i, names_asset(st, i, &s.name)))
                .filter(|(st, i, named)| *named || subject_line(st, *i))
                .map(|(st, i, named)| (st.id, i, named))
                .collect()
        })
        .collect();
    let lines_of = |named_only: bool, skip: &BTreeSet<(i64, usize)>| -> Vec<Vec<(i64, usize)>> {
        cands
            .iter()
            .map(|c| {
                c.iter()
                    .filter(|(id, i, named)| (*named || !named_only) && !skip.contains(&(*id, *i)))
                    .map(|(id, i, _)| (*id, *i))
                    .collect()
            })
            .collect()
    };
    // 1. Las líneas que nombran el elemento.
    let mut m = Matching::default();
    let named = lines_of(true, &BTreeSet::new());
    for p in 0..unlinked.len() {
        m.augment(p, true, &named, &mut BTreeSet::new());
    }
    // 2. La línea que nombra dos ventas o más sin línea y tiene la suma de sus precios es de todas:
    //    vale más que la de otra factura que sólo tiene la base de una de ellas.
    let mut groups: BTreeMap<usize, (i64, usize, Money)> = BTreeMap::new();
    let unmatched: Vec<usize> = (0..unlinked.len())
        .filter(|p| !m.chosen.contains_key(p))
        .collect();
    for st in invoices {
        let close: Vec<usize> = unmatched
            .iter()
            .copied()
            .filter(|p| days_to(&sales[unlinked[*p]], st).is_some())
            .collect();
        if close.len() < 2 {
            continue;
        }
        for i in 0..st.invoice.lines.len() {
            if !usable(st, i) || m.holder.contains_key(&(st.id, i)) {
                continue;
            }
            let members: Vec<usize> = close
                .iter()
                .copied()
                .filter(|p| {
                    !groups.contains_key(p) && names_asset(st, i, &sales[unlinked[*p]].name)
                })
                .collect();
            if members.len() < 2 {
                continue;
            }
            let prices: Vec<Money> = members.iter().map(|p| sales[unlinked[*p]].price).collect();
            let sum: Money = prices.iter().copied().sum();
            if sum != line_base(&st.invoice.lines[i]) && sum != net(st, i) {
                continue;
            }
            for (p, part) in members.iter().zip(prorate(net(st, i), &prices)) {
                groups.insert(*p, (st.id, i, part));
            }
        }
    }
    // 3. Las demás, también con las líneas con su base que no las nombran.
    let grouped: BTreeSet<(i64, usize)> = groups.values().map(|(id, i, _)| (*id, *i)).collect();
    let all = lines_of(false, &grouped);
    for p in 0..unlinked.len() {
        if !m.chosen.contains_key(&p) && !groups.contains_key(&p) {
            m.augment(p, false, &all, &mut BTreeSet::new());
        }
    }
    // Quién lleva cada línea: las vinculadas, las emparejadas y las de una línea con otras.
    let mut taken_by: BTreeMap<(i64, usize), usize> = lines_use.owner.clone();
    for (line, (p, _)) in &m.holder {
        taken_by.entry(*line).or_insert(unlinked[*p]);
    }
    for (p, (id, i, _)) in &groups {
        taken_by.entry((*id, *i)).or_insert(unlinked[*p]);
    }
    for (p, &k) in unlinked.iter().enumerate() {
        let s = &sales[k];
        let line = match (m.chosen.get(&p), groups.get(&p)) {
            (Some((id, i)), _) => by_id.get(id).map(|st| (*st, *i, None)),
            (None, Some((id, i, part))) => by_id.get(id).map(|st| (*st, *i, Some(*part))),
            (None, None) => None,
        };
        found[k] = Some(match line {
            Some((st, i, _)) if !subject_line(st, i) => SaleInvoice::NotSubject,
            Some((st, i, part)) => SaleInvoice::Line {
                id: st.id,
                line: i,
                lines: vec![i],
                price: part.unwrap_or_else(|| net(st, i)),
                doubt: (!names_asset(st, i, &s.name)).then_some(Doubt::Unnamed),
            },
            None => match near(s).first() {
                Some((st, i, _)) => {
                    let n = &st.invoice.number;
                    let l = &st.invoice.lines[*i];
                    SaleInvoice::Unusable(if rectifying(st) {
                        format!(
                            "la línea con esa base es de la rectificativa {n}, que corrige otra \
                             factura (si la venta no está facturada, emítela)"
                        )
                    } else if l.advance {
                        format!(
                            "la línea con esa base es el anticipo de la factura {n} (falta la \
                             factura de la entrega, con el anticipo descontado)"
                        )
                    } else if let Some(j) = taken_by.get(&(st.id, *i)) {
                        format!(
                            "la línea con esa base de la factura {n} ya es la venta de {} (emite \
                             la de esta, o pon cada elemento en su línea)",
                            sales[*j].name
                        )
                    } else if !subject_line(st, *i) {
                        format!(
                            "la línea con esa base, «{}» de la factura {n}, es no sujeta y no \
                             nombra el elemento (si es su venta, vincúlala; si no, emite la suya)",
                            l.description
                        )
                    } else {
                        format!(
                            "la línea con esa base de la factura {n} ya es la venta de otro \
                             elemento (emite la de esta, o pon cada elemento en su línea)"
                        )
                    })
                }
                None => {
                    // La línea de una vinculada a la que le queda sin vincular su precio (o más,
                    // si la nombra), la más cercana.
                    let shared = lines_near(s, invoices)
                        .into_iter()
                        .filter(|(st, i, _)| !rectifying(st) && !st.invoice.lines[*i].advance)
                        .filter_map(|(st, i, days)| {
                            let used = *lines_use.used.get(&(st.id, i))?;
                            let open = net(st, i) - used;
                            let with = *lines_use.owner.get(&(st.id, i))?;
                            (open == s.price || (open > s.price && names_asset(st, i, &s.name)))
                                .then_some((st, i, open, with, days))
                        })
                        .min_by_key(|(st, i, _, _, days)| (*days, st.id, *i));
                    // Si no, la que nombra el elemento y tiene más que su precio.
                    let partial = lines_near(s, invoices)
                        .into_iter()
                        .filter(|(st, i, _)| {
                            usable(st, *i)
                                && !taken_by.contains_key(&(st.id, *i))
                                && names_asset(st, *i, &s.name)
                                && net(st, *i) > s.price
                        })
                        .min_by_key(|(st, i, days)| (*days, st.id, *i));
                    match (shared, partial) {
                        (Some((st, line, open, with, _)), _) => SaleInvoice::Shared {
                            id: st.id,
                            line,
                            open,
                            with,
                        },
                        (None, Some((st, line, _))) => SaleInvoice::Partial { id: st.id, line },
                        (None, None) => SaleInvoice::Missing,
                    }
                }
            },
        });
    }
    found
}

/// La línea `i` de `st` o la propia factura nombran el elemento `name` (ver [`names`]).
fn names_asset(st: &StoredIssued, i: usize, name: &str) -> bool {
    names(
        &format!(
            "{} {}",
            st.invoice.lines[i].description, st.invoice.description
        ),
        name,
    )
}

/// `text` nombra el elemento `name`: lleva alguna palabra suya de cuatro letras o más (o el
/// nombre entero, si no tiene ninguna), sin mirar mayúsculas ni tildes.
fn names(text: &str, name: &str) -> bool {
    use crate::documentos::fold;
    let text = fold(text);
    let name = fold(name);
    let words: Vec<&str> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 4)
        .collect();
    if words.is_empty() {
        let name = name.trim();
        !name.is_empty() && text.contains(name)
    } else {
        words.iter().any(|w| text.contains(w))
    }
}

/// `text` nombra la línea de factura `line`: lleva alguna palabra suya de cuatro letras o más que no
/// es sólo un número (un año o un importe no nombran nada), sin mirar mayúsculas ni tildes.
fn names_line(text: &str, line: &str) -> bool {
    use crate::documentos::fold;
    let text = fold(text);
    fold(line)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 4 && w.chars().any(char::is_alphabetic))
        .any(|w| text.contains(w))
}

/// `a` sin pasar de `limit`: con su signo y como mucho su tamaño (cero si van en sentidos
/// contrarios).
fn capped(a: Money, limit: Money) -> Money {
    match (a.is_negative(), limit.is_negative()) {
        (true, true) => a.max(limit),
        (false, false) => a.min(limit),
        _ => Money::ZERO,
    }
}

/// Base sujeta (también la exenta) de un desglose: la que puede ir en [106] y [107].
fn subject_base(b: &[TaxBreakdown]) -> Money {
    b.iter()
        .filter(|t| !matches!(t.vat, VatTreatment::NotSubject(_)))
        .map(|t| t.base)
        .sum()
}

/// Factura en la que está la venta `s` y su precio en el volumen (`None` si su línea es no
/// sujeta): la de su línea o, si es no sujeta, la vinculada.
fn on_invoice(found: &Option<SaleInvoice>, s: &AssetSale) -> Option<(i64, Option<Money>)> {
    match found {
        Some(SaleInvoice::Line { id, price, .. }) => Some((*id, Some(*price))),
        Some(SaleInvoice::NotSubject) => s.invoice.map(|id| (id, None)),
        _ => None,
    }
}

/// Lo que una rectificativa cambia en la base sujeta de cada factura que rectifica y lo que de ello
/// se ha pasado a cada venta en Contabilidad › Inmovilizado (ver [`rect_part`]).
struct RectPart {
    /// Facturas que rectifica (ids), en el orden de su lista.
    invoices: Vec<i64>,
    /// El cambio de la base sujeta de cada una.
    change: BTreeMap<i64, Money>,
    /// Lo pasado a cada venta (índice en las ventas), sin lo que es de líneas no sujetas.
    passed: BTreeMap<usize, Money>,
    /// De lo que no se ha pasado, lo de cada factura que es de sus otras líneas: el de las líneas
    /// de la rectificativa que sólo nombran una línea suya que no es de una venta («Ampliación de
    /// la consultoría»).
    others: BTreeMap<i64, Money>,
    /// Si rectifica varias facturas y no dice qué parte es de cada una, los números de las que se
    /// lo reparten.
    several: Option<Vec<String>>,
}

/// Lo que la rectificativa `r` de las facturas `xs` cambia en cada una: su base sujeta si es por
/// diferencias, y la diferencia con ellas si es por sustitución, como en el 303. Cuenta una vez:
/// - a cada venta de esas facturas, lo que se le ha pasado (`links`) sin lo que es de líneas no
///   sujetas (ver [`passed_not_subject`]), y entre todas sin pasar del cambio sujeto;
/// - lo de las líneas de `r` que sólo nombran otra línea de una de las facturas, que no es de una
///   venta, a esa factura y no a sus ventas;
/// - el resto, a las facturas que aún pueden tenerlo (con ventas sin su parte o con otras líneas
///   sujetas: no a la que sólo tiene ventas con su parte pasada) y, si son varias, en proporción a
///   su base sujeta (y se avisa).
fn rect_part(
    r: &StoredIssued,
    xs: &[&StoredIssued],
    sales: &[AssetSale],
    found: &[Option<SaleInvoice>],
    links: &BTreeMap<(i64, i64), Money>,
) -> RectPart {
    use taxpain_core::invoice::totals::line_base;
    let total = |b: &[TaxBreakdown]| b.iter().map(|t| t.base).sum::<Money>();
    let by_difference = !matches!(r.invoice.kind,
        InvoiceKind::Rectificativa(k) if k.method == RectificationMethod::Sustitucion);
    let mut change = subject_base(&r.totals.breakdown);
    let mut whole = total(&r.totals.breakdown);
    if !by_difference {
        for x in xs {
            change -= subject_base(&x.totals.breakdown);
            whole -= total(&x.totals.breakdown);
        }
    }
    let ids: Vec<i64> = xs.iter().map(|x| x.id).collect();
    // Ventas de estas facturas, con su factura y su precio en el volumen.
    let on: Vec<(usize, i64, Option<Money>)> = sales
        .iter()
        .enumerate()
        .filter_map(|(k, s)| {
            let (x, price) = on_invoice(&found[k], s)?;
            ids.contains(&x).then_some((k, x, price))
        })
        .collect();
    // Las que tienen algo pasado: lo pasado, su factura y lo vinculado que no se resta (líneas no
    // sujetas).
    let linked: Vec<(usize, Money, i64, Money)> = on
        .iter()
        .filter_map(|(k, x, price)| {
            let s = &sales[*k];
            let m = *links.get(&(s.id, r.id))?;
            let other = (s.price - price.unwrap_or_default()).max(Money::ZERO);
            Some((*k, m, *x, other))
        })
        .collect();
    let not_subject = passed_not_subject(r, by_difference, whole - change, &linked);
    let subject: Vec<Money> = linked
        .iter()
        .zip(not_subject)
        .map(|((_, m, ..), n)| *m - n)
        .collect();
    let sum: Money = subject.iter().copied().sum();
    let fits = capped(sum, change);
    let subject = if fits == sum {
        subject
    } else {
        let weights: Vec<Money> = subject.iter().map(|m| m.abs()).collect();
        prorate(fits, &weights)
    };
    let rest = change - fits;
    // Lo de las líneas de `r` que sólo nombran otra línea de una factura, sin pasar del resto.
    let sale_lines = |x: i64| -> BTreeSet<usize> {
        found
            .iter()
            .filter_map(|f| match f {
                Some(SaleInvoice::Line { id, lines, .. }) if *id == x => Some(lines.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    };
    let mine: Vec<BTreeSet<usize>> = xs.iter().map(|x| sale_lines(x.id)).collect();
    let subject_lines =
        r.invoice.lines.iter().filter(|l| {
            by_difference && !l.advance && !matches!(l.vat, VatTreatment::NotSubject(_))
        });
    let mut others: BTreeMap<i64, Money> = BTreeMap::new();
    for l in subject_lines {
        let text = &l.description;
        let of_sale = on.iter().any(|(k, ..)| names(text, &sales[*k].name))
            || xs.iter().zip(&mine).any(|(x, lines)| {
                lines
                    .iter()
                    .any(|j| names_line(text, &x.invoice.lines[*j].description))
            });
        if of_sale {
            continue;
        }
        let to: Vec<i64> = xs
            .iter()
            .zip(&mine)
            .filter(|(x, lines)| {
                (0..x.invoice.lines.len()).any(|j| {
                    let line = &x.invoice.lines[j];
                    !lines.contains(&j)
                        && !line.advance
                        && subject_line(x, j)
                        && line_base(line).cents() > 0
                        && names_line(text, &line.description)
                })
            })
            .map(|(x, _)| x.id)
            .collect();
        if let [x] = to.as_slice() {
            *others.entry(*x).or_default() += line_base(l);
        }
    }
    let named: Money = others.values().copied().sum();
    let fit = capped(named, rest);
    if fit != named {
        let weights: Vec<Money> = others.values().map(|m| m.abs()).collect();
        for (m, f) in others.values_mut().zip(prorate(fit, &weights)) {
            *m = f;
        }
    }
    let rest = rest - fit;
    // El resto, de las facturas que aún pueden tenerlo, en proporción a su base sujeta.
    let takes: Vec<&StoredIssued> = xs
        .iter()
        .copied()
        .filter(|x| {
            let of_x = || on.iter().filter(|(_, y, _)| *y == x.id);
            let open = of_x().any(|(k, _, p)| {
                p.is_some_and(|p| p.cents() > 0) && !links.contains_key(&(sales[*k].id, r.id))
            });
            let priced: Money = of_x().map(|(.., p)| p.unwrap_or_default()).sum();
            open || subject_base(&x.totals.breakdown) > priced
        })
        .collect();
    let takes = if takes.is_empty() { xs.to_vec() } else { takes };
    let shares = if takes.len() == 1 {
        vec![rest]
    } else {
        let weights: Vec<Money> = takes
            .iter()
            .map(|x| subject_base(&x.totals.breakdown).max(Money::ZERO))
            .collect();
        prorate(rest, &weights)
    };
    let mut by: BTreeMap<i64, Money> = ids.iter().map(|x| (*x, Money::ZERO)).collect();
    for (x, m) in takes.iter().zip(shares) {
        *by.entry(x.id).or_default() += m;
    }
    for (x, m) in &others {
        *by.entry(*x).or_default() += *m;
    }
    let mut passed = BTreeMap::new();
    for ((k, _, x, _), m) in linked.iter().zip(subject) {
        *by.entry(*x).or_default() += m;
        passed.insert(*k, m);
    }
    RectPart {
        invoices: ids,
        change: by,
        passed,
        others,
        several: (takes.len() > 1 && !rest.is_zero())
            .then(|| takes.iter().map(|x| x.invoice.number.clone()).collect()),
    }
}

/// Lo no sujeto de lo pasado a cada venta de `linked` (venta, lo pasado, su factura y lo no sujeto
/// de su vínculo) desde la rectificativa `r`, que cambia lo no sujeto en `change`:
/// - si lo pasado suma unas líneas de `r` (por diferencias) y todas las formas de sumarlo dan lo
///   mismo, lo no sujeto de ésas, porque lo pasado suele ser una línea o varias: la anulación no
///   sujeta de la grúa es de la grúa y la rebaja de la furgoneta, toda sujeta;
/// - si no, lo que falta del cambio, en proporción a lo no sujeto de cada vínculo (un suplido) y
///   sin pasar de ello si es una rebaja, ni de lo pasado.
fn passed_not_subject(
    r: &StoredIssued,
    by_difference: bool,
    change: Money,
    linked: &[(usize, Money, i64, Money)],
) -> Vec<Money> {
    use taxpain_core::invoice::totals::line_base;
    /// Líneas que se combinan como mucho (4.095 sumas por venta).
    const MAX_LINES: usize = 12;
    // Base y parte no sujeta de cada línea.
    let lines: Vec<(Money, Money)> = r
        .invoice
        .lines
        .iter()
        .filter(|l| !l.advance)
        .map(|l| {
            let b = line_base(l);
            let not_subject = matches!(l.vat, VatTreatment::NotSubject(_));
            (b, if not_subject { b } else { Money::ZERO })
        })
        .filter(|(b, _)| !b.is_zero())
        .collect();
    let exact = |m: Money| -> Option<Money> {
        if !by_difference || lines.len() > MAX_LINES {
            return None;
        }
        let mut out = None;
        for set in 1u32..(1 << lines.len()) {
            let (sum, not_subject) = lines
                .iter()
                .enumerate()
                .filter(|(i, _)| set & (1 << i) != 0)
                .fold((Money::ZERO, Money::ZERO), |(s, n), (_, (b, x))| {
                    (s + *b, n + *x)
                });
            if sum == m {
                match out {
                    Some(o) if o != not_subject => return None,
                    _ => out = Some(not_subject),
                }
            }
        }
        out
    };
    let mut known: Vec<Option<Money>> = linked.iter().map(|(_, m, ..)| exact(*m)).collect();
    let sum: Money = known.iter().flatten().copied().sum();
    if capped(sum, change) != sum {
        known = vec![None; linked.len()];
    }
    let rest = change - known.iter().flatten().copied().sum::<Money>();
    let open: Vec<usize> = (0..linked.len()).filter(|i| known[*i].is_none()).collect();
    let weights: Vec<Money> = open.iter().map(|i| linked[*i].3).collect();
    let rest = if rest.is_negative() {
        rest.max(-weights.iter().copied().sum::<Money>())
    } else {
        rest
    };
    let mut out: Vec<Money> = known.iter().map(|k| k.unwrap_or_default()).collect();
    for (i, n) in open.iter().zip(prorate(rest, &weights)) {
        let m = linked[*i].1;
        out[*i] = if n.is_negative() == m.is_negative() && n.abs() > m.abs() {
            m
        } else {
            n
        };
    }
    out
}

/// Lo que queda en el volumen de las ventas de una factura (ver [`left_of_sales`]).
struct Lefts<'a> {
    /// De cada venta, en el orden en que se dan (cero en las no sujetas).
    left: Vec<Money>,
    /// Rectificativas repartidas entre dos ventas o más sin saber qué parte es de cada una, y
    /// entre cuáles (posiciones).
    spread: Vec<(&'a StoredIssued, Vec<usize>)>,
    /// Rectificativas restadas de las ventas aunque no se les ha pasado todo su cambio (la
    /// factura no tiene otras líneas sujetas): con lo pasado y el cambio.
    short: Vec<(&'a StoredIssued, Money, Money)>,
}

/// Lo que queda en el volumen de cada venta de `on` (la venta y su precio en el volumen, `None` si
/// su línea es no sujeta), facturadas en `x`, con las rectificativas `by` de `x`, por fecha;
/// `advances`, lo que `x` descuenta de anticipos que están en el volumen de su mismo año (ver
/// [`advances_of_year`]). El cambio de cada rectificativa en `x` (ver [`rect_part`]) cuenta una
/// vez:
/// - a cada venta, lo que se le ha pasado en Contabilidad › Inmovilizado (sin lo no sujeto);
/// - lo de las líneas que sólo nombran otra línea de la factura, a sus otras líneas sujetas;
/// - el resto, a las ventas sujetas a las que no se les ha pasado nada, en proporción a lo que
///   queda de cada una (también si la sube) y sin bajar ninguna de cero; lo que no les cabe (o
///   todo, si no hay ninguna), a las otras líneas sujetas de la factura y, si no las tiene (o no
///   cubren la rebaja), a sus ventas, que es todo lo que tiene.
///
/// Al final ninguna venta queda por debajo de cero (lo que falta se quita de las demás) y entre
/// todas no pasan de lo sujeto de `x` en el volumen, con esos anticipos.
fn left_of_sales<'a>(
    on: &[(usize, Option<Money>)],
    x: &StoredIssued,
    advances: Money,
    by: &[&'a StoredIssued],
    parts: &BTreeMap<i64, RectPart>,
) -> Lefts<'a> {
    let price: Vec<Money> = on.iter().map(|(_, p)| p.unwrap_or_default()).collect();
    let mut left = price.clone();
    let base = subject_base(&x.totals.breakdown) + advances;
    let first_others = (base - price.iter().copied().sum::<Money>()).max(Money::ZERO);
    let mut others = first_others;
    // Lo que se llevan de `m` las otras líneas sujetas: de una rebaja, hasta lo que les queda; una
    // subida, si las hay.
    let mut absorb = |m: Money| -> Money {
        let here = if m.is_negative() {
            m.max(-others)
        } else if first_others.cents() > 0 {
            m
        } else {
            Money::ZERO
        };
        others += here;
        here
    };
    let mut volume = base;
    let mut spread = Vec::new();
    let mut short = Vec::new();
    for r in by {
        let Some(part) = parts.get(&r.id) else {
            continue;
        };
        let change = part.change.get(&x.id).copied().unwrap_or_default();
        volume += change;
        let (mut rest, mut passed, mut open) = (change, Money::ZERO, Vec::new());
        for (j, (k, p)) in on.iter().enumerate() {
            match part.passed.get(k) {
                Some(m) => {
                    if p.is_some() {
                        left[j] += *m;
                    }
                    rest -= *m;
                    passed += *m;
                }
                None if p.is_some() => open.push(j),
                None => {}
            }
        }
        // Lo de las líneas que sólo nombran otra línea de la factura, de ésta.
        rest -= absorb(capped(
            part.others.get(&x.id).copied().unwrap_or_default(),
            rest,
        ));
        if !open.is_empty() && !rest.is_zero() {
            // De las ventas sin su parte; una rebaja, sin bajar ninguna de cero.
            let mut weights: Vec<Money> = open.iter().map(|j| left[*j].max(Money::ZERO)).collect();
            let room: Money = weights.iter().copied().sum();
            if room.is_zero() {
                weights = open.iter().map(|j| price[*j]).collect();
            }
            let fit = if rest.is_negative() {
                rest.max(-room)
            } else {
                rest
            };
            if open.len() > 1 && !fit.is_zero() {
                spread.push((*r, open.clone()));
            }
            for (j, m) in open.iter().zip(prorate(fit, &weights)) {
                left[*j] += m;
            }
            rest -= fit;
        }
        // Lo que no les cabe (o todo, si todas tienen lo suyo), de las otras líneas sujetas.
        rest -= absorb(rest);
        if rest.is_zero() {
            continue;
        }
        // Ni las tiene: de todas sus ventas, que es lo que tiene la factura.
        if open.is_empty() {
            short.push((*r, passed, change));
        }
        let to: Vec<usize> = (0..on.len()).filter(|j| on[*j].1.is_some()).collect();
        let mut weights: Vec<Money> = to.iter().map(|j| left[*j].max(Money::ZERO)).collect();
        if weights.iter().all(|w| w.is_zero()) {
            weights = to.iter().map(|j| price[*j]).collect();
        }
        for (j, m) in to.iter().zip(prorate(rest, &weights)) {
            left[*j] += m;
        }
    }
    // Ninguna por debajo de cero: lo que falta, de las demás.
    let missing: Money = left.iter().map(|m| (*m).min(Money::ZERO)).sum();
    if missing.is_negative() {
        for m in &mut left {
            *m = (*m).max(Money::ZERO);
        }
        let weights = left.clone();
        let cut = missing.max(-weights.iter().copied().sum::<Money>());
        for (m, d) in left.iter_mut().zip(prorate(cut, &weights)) {
            *m += d;
        }
    }
    // Ni todas juntas más de lo sujeto de la factura en el volumen.
    let cap = volume.max(Money::ZERO);
    if left.iter().copied().sum::<Money>() > cap {
        let weights = left.clone();
        left = prorate(cap, &weights);
    }
    Lefts {
        left,
        spread,
        short,
    }
}

/// `amount` repartido en proporción a `weights` (no negativos) sin perder céntimos: cada parte se
/// queda a menos de un céntimo de la exacta, y los céntimos que sobran van a las de resto mayor
/// (si empatan, a la primera: quien llama las ordena sin depender del orden de alta). Si `amount`
/// no pasa de la suma de los pesos, ninguna pasa del suyo.
fn prorate(amount: Money, weights: &[Money]) -> Vec<Money> {
    let total: i128 = weights.iter().map(|w| i128::from(w.cents().max(0))).sum();
    if total <= 0 {
        return vec![Money::ZERO; weights.len()];
    }
    let sign = i128::from(amount.cents().signum());
    let a = i128::from(amount.cents()).abs();
    let mut parts: Vec<(i128, i128)> = weights
        .iter()
        .map(|w| {
            let n = a * i128::from(w.cents().max(0));
            (n / total, n % total)
        })
        .collect();
    let mut extra = a - parts.iter().map(|(q, _)| *q).sum::<i128>();
    let mut order: Vec<usize> = (0..parts.len()).collect();
    order.sort_by_key(|i| (Reverse(parts[*i].1), *i));
    for i in order {
        if extra <= 0 {
            break;
        }
        parts[i].0 += 1;
        extra -= 1;
    }
    parts
        .iter()
        .map(|(q, _)| Money::from_cents(i64::try_from(sign * q).unwrap_or_default()))
        .collect()
}

/// «A», «A y B», «A, B y C».
fn y_list(items: &[&str]) -> String {
    match items.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} y {last}", rest.join(", ")),
        _ => items.concat(),
    }
}

/// Lo que `x` descuenta de anticipos que están en el volumen de su mismo año: los facturados a su
/// cliente ese año antes que ella, tomando primero los más antiguos aún sin descontar. El anticipo
/// devenga al cobrarse (art. 75.Dos LIVA): con él en el mismo año, la venta de `x` está entera en
/// ese volumen aunque `x` sólo lleve la parte que falta.
fn advances_of_year(v: &Vault, x: &StoredIssued) -> Result<Money> {
    use std::collections::VecDeque;
    use taxpain_core::invoice::totals::line_base;
    // Las líneas de anticipo sujetas: positivas en la factura del anticipo y negativas donde se
    // descuenta.
    let advances = |st: &StoredIssued| -> Vec<Money> {
        (0..st.invoice.lines.len())
            .filter(|i| st.invoice.lines[*i].advance && subject_line(st, *i))
            .map(|i| line_base(&st.invoice.lines[i]))
            .collect()
    };
    let recipient = x.invoice.recipient.as_ref().map(|p| match &p.id {
        PartyId::Nif(n) => n.clone(),
        PartyId::Foreign { id, .. } => id.clone(),
    });
    let (Some(recipient), true) = (recipient, advances(x).iter().any(|m| m.is_negative())) else {
        return Ok(Money::ZERO);
    };
    let mut stmt = v.connection().prepare(
        "SELECT id, data FROM invoice_issued
          WHERE status = 'emitida' AND recipient_tax_id = ?1 AND instr(data, '\"advance\":true') > 0",
    )?;
    let rows = stmt.query_map([recipient], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut all = Vec::new();
    for row in rows {
        let (id, data) = row?;
        all.push(stored_issued(id, &data)?);
    }
    all.sort_by_key(|st| (st.accrual(), st.id));
    // Lo anticipado aún sin descontar, con su año, del más antiguo al más nuevo.
    let mut pending: VecDeque<(i32, Money)> = VecDeque::new();
    for st in &all {
        let year = st.accrual().year();
        let mut deducted: Money = -advances(st)
            .into_iter()
            .filter(|m| m.is_negative())
            .sum::<Money>();
        let mut same = Money::ZERO;
        while deducted.cents() > 0 {
            let Some((y, m)) = pending.front_mut() else {
                break;
            };
            let here = deducted.min(*m);
            if *y == year {
                same += here;
            }
            *m -= here;
            deducted -= here;
            if m.is_zero() {
                pending.pop_front();
            }
        }
        if st.id == x.id {
            return Ok(same);
        }
        let new: Money = advances(st).into_iter().filter(|m| m.cents() > 0).sum();
        if new.cents() > 0 {
            pending.push_back((year, new));
        }
    }
    Ok(Money::ZERO)
}

/// Ventas de inmovilizado que se restan del volumen de operaciones de `year` (las ventas, en
/// [`asset_disposals`]; su factura, en [`match_asset_sales`]: la vinculada en Contabilidad ›
/// Inmovilizado o, si no, la buscada por su base y su fecha), con los avisos con las casillas de
/// `volume`:
/// - se restan en el año en que devenga su factura, que es el del volumen donde está su base
///   (art. 121.Dos LIVA), aunque la baja sea de otro año; en el de la baja se dice dónde; con su
///   anticipo del mismo año, enteras (ver [`advances_of_year`]);
/// - si la factura tiene rectificativas, se resta sólo lo que queda de la venta en el volumen
///   (ver [`left_of_sales`]: cada rectificativa cuenta una vez, con lo que se ha pasado a cada
///   venta o repartida entre ellas, también si rectifica varias facturas), y el cambio de una
///   rectificativa de un año posterior se compensa en ese año;
/// - sin factura la venta no está en el volumen y no se resta: se avisa, salvo si su factura la
///   declara no sujeta, que no va en [106] ni en [107];
/// - las que se restan con la factura buscada se avisan juntas, porque sin el vínculo cuentan dos
///   veces en la contabilidad; sólo las que se pueden vincular (no las de una factura importada
///   del libro de la gestoría, que no tiene asiento, ni las de un ejercicio cerrado). Los avisos
///   que piden vincular algo de un ejercicio cerrado dicen que hay que reabrirlo antes.
fn asset_sales_390(v: &Vault, year: i32, volume: VolumeIn) -> Result<AssetSales390> {
    use crate::venta_inmovilizado::{closed_year_at, imported, linkable};
    use taxpain_core::invoice::Exemption;
    let conn = v.connection();
    let mut out = AssetSales390::default();
    let sales = asset_disposals(v)?;
    if sales.is_empty() {
        return Ok(out);
    }
    let invoices = sale_invoices(v, &sales)?;
    let by_id: BTreeMap<i64, &StoredIssued> = invoices.iter().map(|st| (st.id, st)).collect();
    let found = match_asset_sales(&sales, &invoices);
    // Ventas de cada factura, en un orden que no depende del de alta.
    let mut on_invoices: BTreeMap<i64, Vec<(usize, Option<Money>)>> = BTreeMap::new();
    for (k, f) in found.iter().enumerate() {
        if let Some((x, price)) = on_invoice(f, &sales[k]) {
            on_invoices.entry(x).or_default().push((k, price));
        }
    }
    for on in on_invoices.values_mut() {
        on.sort_by(|(a, _), (b, _)| sales[*a].key().cmp(&sales[*b].key()));
    }
    // Lo que descuenta cada una de anticipos de su mismo año, que también están en su volumen.
    let mut advances: BTreeMap<i64, Money> = BTreeMap::new();
    for id in on_invoices.keys() {
        if let Some(x) = by_id.get(id) {
            advances.insert(*id, advances_of_year(v, x)?);
        }
    }
    let advance = |x: &StoredIssued| advances.get(&x.id).copied().unwrap_or_default();
    let capital_on = |x: i64| {
        on_invoices
            .get(&x)
            .is_some_and(|on| on.iter().any(|(k, p)| p.is_some() && sales[*k].capital))
    };
    // Rectificativas que pueden cambiar una venta: hay cuatro años para rectificar (art.
    // 89.Uno LIVA). Lo que cada una cambia en cada factura con ventas.
    let sold_by: BTreeSet<&str> = invoices
        .iter()
        .filter(|x| on_invoices.contains_key(&x.id))
        .map(|x| x.invoice.number.as_str())
        .collect();
    let rects: Vec<StoredIssued> = rectifying_of_years(v, year - 5, year)?
        .into_iter()
        .filter(|r| {
            r.invoice
                .rectified
                .iter()
                .any(|(n, _)| sold_by.contains(n.as_str()))
        })
        .collect();
    let links = rectifying_links(v)?;
    // Las facturas rectificadas sin ventas, para repartir entre ellas (ver [`rect_part`]).
    let mut more: Vec<StoredIssued> = Vec::new();
    for r in &rects {
        for (number, _) in &r.invoice.rectified {
            if !invoices
                .iter()
                .chain(&more)
                .any(|x| x.invoice.number == *number)
                && let Some(x) = issued_by_number(v, number)?
            {
                more.push(x);
            }
        }
    }
    let mut parts: BTreeMap<i64, RectPart> = BTreeMap::new();
    for r in &rects {
        let mut xs: Vec<&StoredIssued> = Vec::new();
        for (number, _) in &r.invoice.rectified {
            if let Some(x) = invoices
                .iter()
                .chain(&more)
                .find(|x| x.invoice.number == *number)
                && x.id != r.id
                && !xs.iter().any(|y| y.id == x.id)
            {
                xs.push(x);
            }
        }
        parts.insert(r.id, rect_part(r, &xs, &sales, &found, &links));
    }
    let dmy = |d: Date| {
        d.format(&time::macros::format_description!("[day]/[month]/[year]"))
            .unwrap_or_default()
    };
    // Las rectificativas de `x` que devengan hasta `until`, por fecha.
    let rectified = |x: &StoredIssued, until: i32| -> Vec<&StoredIssued> {
        let mut by: Vec<&StoredIssued> = rects
            .iter()
            .filter(|r| {
                r.accrual().year() <= until
                    && parts.get(&r.id).is_some_and(|p| p.invoices.contains(&x.id))
            })
            .collect();
        by.sort_by_key(|r| (r.accrual(), r.id));
        by
    };
    // `x`: la factura de la venta `k`, para saber si su línea da derecho a deducir.
    let add = |out: &mut AssetSales390, k: usize, x: &StoredIssued, m: Money| {
        let s = &sales[k];
        if s.real_estate {
            out.real_estate += m;
        } else {
            out.capital_goods += m;
        }
        if let Some(SaleInvoice::Line { line, .. }) = &found[k]
            && x.invoice
                .lines
                .get(*line)
                .is_some_and(|l| prorata_right(l.vat) == Some(true))
        {
            out.with_right += m;
        }
    };
    // Qué pedir para pasar a las ventas la rectificativa `r`: a las de las facturas `to_link`, sin
    // vincular, sólo se les puede pasar con su factura vinculada, y en un ejercicio cerrado (el de
    // `r` o el de una de ellas) no se puede vincular nada sin reabrirlo.
    let pass_it = |r: &StoredIssued, to_link: &[&StoredIssued]| -> Result<String> {
        let mut closed: Vec<String> = Vec::new();
        let days = std::iter::once(r.invoice.issue_date)
            .chain(to_link.iter().map(|x| x.invoice.issue_date));
        for d in days {
            if let Some(y) = closed_year_at(conn, d)?
                && !closed.contains(&y)
            {
                closed.push(y);
            }
        }
        closed.sort();
        let closed: Vec<&str> = closed.iter().map(String::as_str).collect();
        Ok(match (closed.as_slice(), to_link.is_empty()) {
            ([], true) => "Pasa a cada venta su parte en Contabilidad › Inmovilizado.".into(),
            ([], false) => "Vincula a cada venta su factura y pásale su parte en Contabilidad › \
                            Inmovilizado."
                .into(),
            ([y], true) => format!(
                "Su ejercicio, {y}, está cerrado: para pasar a cada venta su parte, reábrelo antes \
                 desde Cierre."
            ),
            ([y], false) => format!(
                "Para pasar a cada venta su parte hay que vincularle su factura, y el ejercicio \
                 {y} está cerrado: reábrelo antes desde Cierre."
            ),
            (years, _) => format!(
                "Para pasar a cada venta su parte hay que vincularle su factura, y los ejercicios \
                 {} están cerrados: reábrelos antes desde Cierre.",
                y_list(years)
            ),
        })
    };
    // La rectificativa `r` de `x`, repartida entre las ventas `names` (vinculadas todas, o no)
    // sin saber qué parte es de cada una.
    let unsplit = |r: &StoredIssued,
                   x: &StoredIssued,
                   (names, linked): (Vec<&str>, bool)|
     -> Result<String> {
        let to_link: &[&StoredIssued] = if linked { &[] } else { &[x] };
        Ok(format!(
            "La rectificativa {} de {}, que vende {}, no dice qué parte es de cada venta: aquí \
                 se reparte entre ellas en proporción a lo que queda de cada una. {}",
            r.invoice.number,
            x.invoice.number,
            y_list(&names),
            pass_it(r, to_link)?
        ))
    };
    // La rectificativa `r` de `x`, restada de sus ventas aunque se les ha pasado menos.
    let short = |r: &StoredIssued, x: &StoredIssued, passed: Money, change: Money| {
        Ok::<_, AppError>(format!(
            "La rectificativa {} de {} cambia su base sujeta en {change}, pero a sus ventas sólo se \
             ha pasado {passed} en Contabilidad › Inmovilizado y la factura no tiene otras líneas \
             sujetas: aquí se resta de sus ventas el cambio entero. {}",
            r.invoice.number,
            x.invoice.number,
            pass_it(r, &[])?
        ))
    };
    // Los nombres de las ventas `at` de `on`, por orden de baja, y si todas están vinculadas.
    let names_at = |on: &[(usize, Option<Money>)], at: &[usize]| -> (Vec<&str>, bool) {
        let mut ks: Vec<usize> = at.iter().map(|j| on[*j].0).collect();
        ks.sort_unstable();
        (
            ks.iter().map(|k| sales[*k].name.as_str()).collect(),
            ks.iter().all(|k| sales[*k].invoice.is_some()),
        )
    };
    // Lo que queda en el volumen de cada venta, por factura: cada rectificativa cuenta una vez.
    let mut left: BTreeMap<usize, Money> = BTreeMap::new();
    for (id, on) in &on_invoices {
        let Some(x) = by_id.get(id) else {
            continue;
        };
        if x.accrual().year() != year || !capital_on(*id) {
            continue;
        }
        let l = left_of_sales(on, x, advance(x), &rectified(x, year), &parts);
        left.extend(on.iter().map(|(k, _)| *k).zip(l.left));
        for (r, at) in &l.spread {
            out.warnings.push(unsplit(r, x, names_at(on, at))?);
        }
        for (r, passed, change) in &l.short {
            out.warnings.push(short(r, x, *passed, *change)?);
        }
    }
    // Ventas que se restan con la factura buscada, sin vincular: un aviso para todas.
    let mut not_linked: Vec<String> = Vec::new();
    for (k, f) in found.iter().enumerate() {
        let s = &sales[k];
        // La vinculada que no es bien de inversión sólo ocupa su línea.
        let (true, Some(f)) = (s.capital, f) else {
            continue;
        };
        let box_ = volume.box_of(s.real_estate);
        let head = |price: Money| format!("Venta de {} del {} por {price}", s.name, dmy(s.date));
        let in_year = s.date.year() == year;
        match f {
            SaleInvoice::Line {
                id,
                line,
                price,
                doubt,
                ..
            } => {
                let Some(x) = by_id.get(id) else {
                    continue;
                };
                let head = head(*price);
                let number = &x.invoice.number;
                let description = &x.invoice.lines[*line].description;
                let accrual = x.accrual().year();
                if accrual != year {
                    if in_year {
                        let mut w = format!(
                            "{head}: su factura {number} devenga en {accrual} y la venta se resta \
                             del volumen de operaciones de {accrual}."
                        );
                        let intra = x
                            .totals
                            .breakdown
                            .iter()
                            .all(|b| b.vat == VatTreatment::Exempt(Exemption::E5));
                        if doubt.is_some() {
                            w.push_str(&format!(
                                " Comprueba que la línea «{description}» es la de la venta."
                            ));
                        } else if accrual > year && x.invoice.operation_date.is_none() && !intra {
                            w.push_str(&format!(
                                " Si la entrega fue el {}, la factura debía llevar esa fecha de la \
                                 operación (art. 6.1.g RD 1619/2012) y devengar en {year} (art. 75 \
                                 LIVA).",
                                dmy(s.date)
                            ));
                        }
                        out.warnings.push(w);
                    }
                    continue;
                }
                let by = rectified(x, year);
                let left = left.get(&k).copied().unwrap_or(*price);
                add(&mut out, k, x, left);
                if !left.is_zero() {
                    out.details.push((
                        format!(
                            "{box_} Venta de {} ({}) del {} · factura {number}",
                            s.name,
                            s.account,
                            dmy(s.date)
                        ),
                        left,
                    ));
                }
                if !by.is_empty() {
                    let numbers: Vec<&str> = by.iter().map(|r| r.invoice.number.as_str()).collect();
                    let which = if by.len() == 1 {
                        "la rectificativa"
                    } else {
                        "las rectificativas"
                    };
                    out.warnings.push(format!(
                        "{head}: su factura {number} tiene {which} {}; de la venta quedan {left} \
                         en el volumen, y eso es lo que se resta en {box_}.",
                        y_list(&numbers)
                    ));
                }
                let what = format!("Venta de {} del {}", s.name, dmy(s.date));
                let amount = if left == *price {
                    format!("{left}")
                } else {
                    format!("{left} (lo que queda en el volumen)")
                };
                match doubt {
                    _ if left.is_zero() => {}
                    None => {}
                    Some(Doubt::Unnamed) => out.warnings.push(format!(
                        "{head}: se resta en {box_} con la línea «{description}» de la factura \
                         {number}, que tiene esa base pero no nombra el elemento: comprueba que es \
                         la venta."
                    )),
                    Some(Doubt::Largest) => out.warnings.push(format!(
                        "{what}: su factura {number} no tiene una línea por lo vinculado ({}) ni \
                         que nombre el elemento, y se resta en {box_} su línea sujeta mayor, \
                         «{description}», por {amount}: comprueba que es la venta (si no, \
                         desvincula la factura y vincúlala por la base de su línea).",
                        s.price
                    )),
                    Some(Doubt::Several) => out.warnings.push(format!(
                        "{what}: su factura {number} tiene varias líneas con lo vinculado ({}) y \
                         ninguna nombra el elemento; se resta en {box_} «{description}» por \
                         {amount}: comprueba que es la venta (si no, nombra el elemento en su \
                         línea).",
                        s.price
                    )),
                    Some(Doubt::Extra(extra)) => {
                        let lines: Vec<String> = extra
                            .iter()
                            .map(|i| format!("«{}»", x.invoice.lines[*i].description))
                            .collect();
                        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
                        let (verb, they) = if lines.len() == 1 {
                            ("resta", "es")
                        } else {
                            ("restan", "son")
                        };
                        out.warnings.push(format!(
                            "{what}: lo vinculado ({}) pasa de las líneas de su factura {number} \
                             que nombran el elemento, y se {verb} en {box_} también {} (en total \
                             {amount}): comprueba que {they} de la venta.",
                            s.price,
                            y_list(&lines)
                        ));
                    }
                }
                // Sin vincular, si se puede vincular: no si viene del libro de la gestoría, que
                // sin asiento en TaxPain no cuenta dos veces, ni si su ejercicio está cerrado.
                if !left.is_zero() && s.invoice.is_none() && linkable(conn, s.id, *id)? {
                    not_linked.push(format!("{} ({number})", s.name));
                }
            }
            SaleInvoice::NotSubject => {}
            SaleInvoice::Shared {
                id,
                line,
                open,
                with,
            } if in_year => {
                let Some(x) = by_id.get(id) else {
                    continue;
                };
                let n = &x.invoice.number;
                let reopen = match closed_year_at(conn, x.invoice.issue_date)? {
                    None => String::new(),
                    Some(y) => {
                        format!(" (su ejercicio, {y}, está cerrado: reábrelo antes desde Cierre)")
                    }
                };
                out.warnings.push(format!(
                    "{}: no se resta en {box_} porque la línea «{}» de la factura {n} está \
                     vinculada a la venta de {} y le quedan {open} sin vincular: si son de esta \
                     venta, vincula también {n} a {} en Contabilidad › Inmovilizado{reopen}; si \
                     no, emite la suya.",
                    head(s.price),
                    x.invoice.lines[*line].description,
                    sales[*with].name,
                    s.name
                ));
            }
            SaleInvoice::Partial { id, line } if in_year => {
                let Some(x) = by_id.get(id) else {
                    continue;
                };
                let n = &x.invoice.number;
                let base = net_bases(x).get(*line).copied().unwrap_or_default();
                let description = &x.invoice.lines[*line].description;
                let why = if imported(conn, *id)? {
                    format!(
                        "la línea «{description}» de la factura {n} nombra el elemento, pero su \
                         base ({base}) no es su precio y la factura viene del libro de la \
                         gestoría, que TaxPain no puede vincular: si es su venta, corrige \
                         {box_} al presentarlo"
                    )
                } else {
                    let reopen = match closed_year_at(conn, x.invoice.issue_date)? {
                        None => String::new(),
                        Some(y) => format!(
                            " (su ejercicio, {y}, está cerrado: reábrelo antes desde Cierre)"
                        ),
                    };
                    format!(
                        "la línea «{description}» de la factura {n} nombra el elemento, pero su \
                         base ({base}) no es su precio: si la venta va en ella con otras cosas, \
                         vincúlala por su parte en Contabilidad › Inmovilizado{reopen}; si no, \
                         emite la suya"
                    )
                };
                out.warnings.push(format!(
                    "{}: no se resta en {box_} porque {why}.",
                    head(s.price)
                ));
            }
            SaleInvoice::Unusable(why) if in_year => {
                out.warnings.push(format!(
                    "{}: no se resta en {box_} porque {why}.",
                    head(s.price)
                ));
            }
            SaleInvoice::Missing if in_year => {
                let margin = time::Duration::days(SALE_INVOICE_DAYS);
                let (from, to) = (s.date - margin, s.date + margin);
                // Con facturas del libro de la gestoría cerca, puede ser una de ellas con otra
                // base, que TaxPain no puede vincular.
                let book: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM invoice_issued WHERE origin <> 'app'
                       AND status = 'emitida' AND issue_date BETWEEN ?1 AND ?2)",
                    [from.to_string(), to.to_string()],
                    |r| r.get(0),
                )?;
                let book = if book {
                    format!(
                        " Si su factura viene del libro de la gestoría con otra base, TaxPain no \
                         puede vincularla: corrige {box_} al presentarlo."
                    )
                } else {
                    String::new()
                };
                out.warnings.push(format!(
                    "{}: ninguna factura emitida del {} al {} tiene una línea con esa base. \
                     Sin factura la venta no está en el 303 ni en {}, así que no se resta en \
                     {box_}: emítela (o corrige la fecha de la baja) y vincúlala en \
                     Contabilidad › Inmovilizado.{book}",
                    head(s.price),
                    dmy(from),
                    dmy(to),
                    volume.volume()
                ));
            }
            SaleInvoice::Shared { .. }
            | SaleInvoice::Partial { .. }
            | SaleInvoice::Unusable(_)
            | SaleInvoice::Missing => {}
        }
    }
    if !not_linked.is_empty() {
        out.warnings.push(format!(
            "Ventas del inmovilizado sin vincular a su factura: {}. En {} se restan con la factura \
             buscada por su base y su fecha; si no se corrigió a mano, en la contabilidad cada una \
             cuenta dos veces (en ventas y en el resultado de la baja): vincula sus facturas en \
             Contabilidad › Inmovilizado.",
            not_linked.join(", "),
            volume.volume()
        ));
    }
    // Rectificativas del año de varias facturas, repartidas entre ellas.
    for r in rects.iter().filter(|r| r.accrual().year() == year) {
        let Some(part) = parts.get(&r.id) else {
            continue;
        };
        let Some(numbers) = &part.several else {
            continue;
        };
        if !part.invoices.iter().any(|x| capital_on(*x)) {
            continue;
        }
        // Las facturas del reparto con ventas sin vincular: hay que vincularlas antes.
        let to_link: Vec<&StoredIssued> = part
            .invoices
            .iter()
            .filter_map(|id| by_id.get(id).copied())
            .filter(|x| numbers.contains(&x.invoice.number))
            .filter(|x| {
                on_invoices
                    .get(&x.id)
                    .is_some_and(|on| on.iter().any(|(k, _)| sales[*k].invoice.is_none()))
            })
            .collect();
        let numbers: Vec<&str> = numbers.iter().map(String::as_str).collect();
        out.warnings.push(format!(
            "La rectificativa {} rectifica varias facturas ({}) y no dice qué parte es de cada \
             una: aquí se reparte entre ellas en proporción a su base sujeta. {}",
            r.invoice.number,
            y_list(&numbers),
            pass_it(r, &to_link)?
        ));
    }
    // Rectificativas del año de facturas de ventas restadas en un año anterior: cambian lo que
    // queda de la venta en el volumen de este año, y aquí se compensa ese cambio.
    let mut seen = BTreeSet::new();
    for r in rects.iter().filter(|r| r.accrual().year() == year) {
        let Some(part) = parts.get(&r.id) else {
            continue;
        };
        for id in &part.invoices {
            let (Some(x), Some(on)) = (by_id.get(id), on_invoices.get(id)) else {
                continue;
            };
            let accrual = x.accrual().year();
            if accrual >= year || !capital_on(*id) || !seen.insert(*id) {
                continue;
            }
            // Todas las ventas de la factura: la rectificativa cuenta una vez entre ellas.
            let by = rectified(x, year);
            let before: Vec<&StoredIssued> = by
                .iter()
                .copied()
                .filter(|r| r.accrual().year() < year)
                .collect();
            let now = left_of_sales(on, x, advance(x), &by, &parts);
            let then = left_of_sales(on, x, advance(x), &before, &parts);
            let numbers: Vec<&str> = by
                .iter()
                .filter(|r| r.accrual().year() == year)
                .map(|r| r.invoice.number.as_str())
                .collect();
            let (label, which, changes) = if numbers.len() == 1 {
                ("rectificativa", "la rectificativa", "cambia")
            } else {
                ("rectificativas", "las rectificativas", "cambian")
            };
            let numbers = y_list(&numbers);
            // Por orden de baja.
            let mut changes_of: Vec<(&(usize, Option<Money>), Money)> = on
                .iter()
                .zip(now.left.iter().zip(&then.left).map(|(a, b)| *a - *b))
                .collect();
            changes_of.sort_by_key(|((k, _), _)| *k);
            for ((k, price), change) in changes_of {
                let s = &sales[*k];
                if !s.capital || price.is_none() || change.is_zero() {
                    continue;
                }
                add(&mut out, *k, x, change);
                let box_ = volume.box_of(s.real_estate);
                out.details.push((
                    format!(
                        "{box_} Venta de {} ({}) del {} · {label} {numbers}",
                        s.name,
                        s.account,
                        dmy(s.date)
                    ),
                    change,
                ));
                out.warnings.push(format!(
                    "Venta de {} del {} por {} (factura {}, que se restó del volumen de \
                     {accrual}): {which} {numbers} {changes} en {year} lo que queda de la venta, \
                     y {box_} lleva {change} para que ese cambio no cuente en el volumen.",
                    s.name,
                    dmy(s.date),
                    price.unwrap_or_default(),
                    x.invoice.number
                ));
            }
            for (r, at) in now
                .spread
                .iter()
                .filter(|(r, _)| r.accrual().year() == year)
            {
                out.warnings.push(unsplit(r, x, names_at(on, at))?);
            }
            for (r, passed, change) in now
                .short
                .iter()
                .filter(|(r, ..)| r.accrual().year() == year)
            {
                out.warnings.push(short(r, x, *passed, *change)?);
            }
        }
    }
    Ok(out)
}

/// Actividad principal y marca del 347 del 390 (las mismas que la página 04 del 303 de un
/// exonerado).
fn options_390(
    v: &Vault,
    year: i32,
    s: &ModelSettings,
) -> Result<taxpain_models::m390::Options390> {
    let inv = invoicing::settings(v)?.ok_or(AppError::NotConfigured)?;
    Ok(taxpain_models::m390::Options390 {
        activity_code: inv
            .activity
            .as_ref()
            .map(|a| format!("{}{}", a.code, a.kind).chars().take(3).collect())
            .unwrap_or_default(),
        iae: inv
            .activity
            .as_ref()
            .map(|a| a.iae.chars().take(4).collect())
            .unwrap_or_default(),
        // Dato estadístico D (campo 43; 44 desde 2026): obligado al 347 si hay algún tercero
        // por encima del umbral (art. 31 RGAT; instrucciones del 390, apartado 3 D).
        files_347: !result_347(v, year)?.lines.is_empty(),
        activity: s.activity_description.clone(),
    })
}

/// Página 04 del 303 del 4T de un exonerado del 390 (Orden EHA/3111/2009 art. 1.2;
/// instrucciones del 390/2025, pág. 2): la actividad, la marca del 347 y el volumen de
/// operaciones del año, calculados igual que el 390 que sustituye.
fn exonerado_390(
    v: &Vault,
    year: i32,
    s: &ModelSettings,
    warnings: &mut Vec<String>,
) -> Result<m303::Exonerado390> {
    let quarters = quarters_303(v, year, 4, s)?;
    // Las ventas de inmovilizado sin factura también se avisan en el 303 del 4T, con las
    // casillas de su página 04.
    let sales = asset_sales_390(v, year, VolumeIn::Exonerado303)?;
    warnings.extend(sales.warnings.iter().cloned());
    let (r, _) = compute_390(v, year, s, &quarters, &sales)?;
    let o = options_390(v, year, s)?;
    if o.activity_code.is_empty() || o.iae.is_empty() {
        warnings.push(
            "Estás exonerado del 390: el 303 del 4T debe llevar tu actividad principal (clave y \
             epígrafe del IAE). Indícala en Ajustes de facturación antes de presentarlo."
                .into(),
        );
    }
    Ok(m303::Exonerado390 {
        activity_code: o.activity_code,
        iae: o.iae,
        files_347: o.files_347,
        volume: m303::Volumen390::of(&r),
    })
}

/// Borrador del 390 del ejercicio (`quarter` se ignora).
pub fn draft_390(v: &Vault, year: i32) -> Result<Draft> {
    use taxpain_models::m390;
    let s = settings(v)?;
    let mut quarters = quarters_303(v, year, 4, &s)?;
    // Los trimestres ya presentados (desde TaxPain o por el gestor) cuentan con sus casillas
    // reales para los resultados del año ([95] ingresado, [97]/[98] del 4T) y el cuadre.
    for r in &mut quarters {
        if let Some((decl, boxes, _)) = filed_303(v, year, r.quarter)? {
            for b in [46u16, 71, 78, 87, 110] {
                match boxes.get(&b) {
                    Some(m) => r.boxes.insert(b, *m),
                    None => r.boxes.remove(&b),
                };
            }
            r.declaration = declaration_type(&decl).unwrap_or(r.declaration);
        }
    }
    let sales = asset_sales_390(v, year, VolumeIn::M390)?;
    let (r, mut warnings) = compute_390(v, year, &s, &quarters, &sales)?;
    warnings.extend(sales.warnings);
    let opts = options_390(v, year, &s)?;
    let who = declarant(v)?;
    let file = m390::file(&r, &who, &opts)?;
    if s.exempt_from_390 {
        warnings.push("Estás marcado como exonerado del 390: no tienes que presentarlo".into());
    }
    Ok(Draft {
        model: "390".into(),
        year,
        quarter: 0,
        boxes: r.boxes.iter().map(|(k, v)| (*k, *v)).collect(),
        payees: vec![],
        // Las ventas de inmovilizado que se restan del volumen ([106] y [107]).
        details: sales.details,
        declaration: String::new(),
        result: r.get(86),
        warnings,
        file_name: format!("{}_{year}_390.txt", who.nif),
        file,
        terceros: vec![],
    })
}

/// 202 del periodo `period` (1P, 2P, 3P), sólo sociedades.
/// `TODO(VERIFY)`: ejercicios que no coinciden con el año natural (se supone 1 de enero).
pub fn draft_202(v: &Vault, year: i32, period: u8) -> Result<Draft> {
    use taxpain_models::m202::{self, Input202};
    if !(1..=3).contains(&period) {
        return Err(AppError::Other(
            "el 202 tiene tres periodos: 1P, 2P y 3P".into(),
        ));
    }
    let who = declarant(v)?;
    if Nif::parse(&who.nif).is_ok_and(|n| n.is_persona_fisica()) {
        return Err(AppError::Other("el modelo 202 es para sociedades".into()));
    }
    // Modalidad del período impositivo: la de la base (art. 40.3 LIS) si se ha elegido o si la
    // cifra de negocios la hace obligatoria.
    let modal = pago202::modalidad_del_pago(v, year, period)?;
    if modal.efectiva == m202::Modalidad::Base {
        return pago202::draft_base(v, year, period, &modal);
    }
    let s = settings(v)?;
    let (base, base_note) = corporate_tax_base(v, year, period)?;
    let input = Input202 {
        base: base.unwrap_or(Money::ZERO),
        // Período impositivo en curso el primer día del plazo (abril, octubre o diciembre).
        period_start: {
            let month = [Month::April, Month::October, Month::December][usize::from(period - 1)];
            let first = Date::from_calendar_date(year, month, 1)
                .map_err(|e| AppError::Other(e.to_string()))?;
            accounting::fiscal_year_at(v, first)?.start
        },
        cnae: s.cnae.clone(),
        tax_rate: s.corporate_tax_rate.clone(),
        turnover_over_6m: s.turnover_over_6m,
        turnover_under_1m: s.turnover_under_1m,
        iban: s.iban.clone().filter(|_| s.domiciliar),
    };
    let mut r = m202::compute(year, period, &input);
    match (base, base_note) {
        (None, _) => r.warnings.push(format!(
            "Indica en «Datos de los modelos» ({year}) la cuota del Impuesto sobre Sociedades de \
             {} (o importa su 200 presentado)",
            if period == 1 { year - 2 } else { year - 1 }
        )),
        (Some(_), Some(note)) => r.warnings.push(note),
        (Some(_), None) => {}
    }
    if !adjustments(v, "202", year, period)?.is_empty() {
        r.warnings.push(
            "Las casillas fijadas a mano del 202 son de la modalidad de la base: con la de la \
             cuota no se usan"
                .into(),
        );
    }
    // Con más de 6 millones la modalidad ya es la de la base (arriba): aquí no se llega con
    // `turnover_over_6m`, pero si se llegara no se escribe un fichero de la modalidad A.
    let (file, file_name) = if input.turnover_over_6m {
        (String::new(), String::new())
    } else {
        (
            m202::file(&r, &who, &input)?,
            format!("{}_{year}_{period}P_202.txt", who.nif),
        )
    };
    Ok(Draft {
        model: "202".into(),
        year,
        quarter: period,
        boxes: r.boxes.iter().map(|(k, v)| (*k, *v)).collect(),
        payees: vec![],
        details: vec![],
        declaration: r.declaration.clone(),
        result: r.result(),
        warnings: r.warnings,
        file_name,
        file,
        terceros: vec![],
    })
}

/// Código de provincia (2 cifras) a partir del código postal.
fn province_of(p: Option<&taxpain_core::invoice::Address>) -> String {
    p.map(|a| a.post_code.chars().take(2).collect::<String>())
        .filter(|c| c.len() == 2 && c.chars().all(|ch| ch.is_ascii_digit()))
        .unwrap_or_default()
}

fn informative_draft(
    v: &Vault,
    model: &str,
    year: i32,
    total: Money,
    details: Vec<(String, Money)>,
    warnings: Vec<String>,
    file: String,
) -> Result<Draft> {
    let who = declarant(v)?;
    Ok(Draft {
        model: model.into(),
        year,
        quarter: 0,
        boxes: vec![],
        payees: vec![],
        details,
        declaration: String::new(),
        result: total,
        warnings,
        file_name: format!("{}_{year}_{model}.txt", who.nif),
        file,
        terceros: vec![],
    })
}

/// 347: ventas (B) desde las emitidas a clientes con NIF español y compras (A) desde las
/// recibidas sin retención, IVA incluido, por encima de 3.005,06 € por tercero y clave.
/// La provincia del proveedor (que no tiene domicilio en TaxPain) se toma del titular:
/// `TODO(VERIFY)`.
/// Cálculo del 347 (sin fichero): terceros, claves e importes por trimestre.
pub(crate) fn result_347(v: &Vault, year: i32) -> Result<taxpain_models::informative::Result347> {
    use taxpain_core::invoice::{Exemption, NotSubject, VatTreatment};
    use taxpain_models::informative::{self, Key347, Op347};
    let inv = invoicing::settings(v)?.ok_or(AppError::NotConfigured)?;
    let own_province = province_of(inv.issuer.address.as_ref());
    let quarter_of = |d: Date| (u8::from(d.month()) - 1) / 3 + 1;
    let (cash_by_invoice, cash_other_years) = cash_collected_347(v, year)?;
    let mut ops = Vec::new();
    for st in issued_of_year(v, year)?.values() {
        // El 347 sigue con la fecha de expedición, no con la de devengo del 303: las operaciones
        // se imputan por su anotación en los libros registro (art. 35 RGAT), que es la fecha de
        // la factura, y así coinciden con lo que declaran los clientes y con las cartas del 347.
        let d = st.invoice.issue_date;
        if d.year() != year {
            continue;
        }
        let Some(r) = &st.invoice.recipient else {
            continue;
        };
        let PartyId::Nif(nif) = &r.id else { continue };
        // Las operaciones con retención de IRPF ya las declara el pagador en su 190: no van en
        // el 347 (art. 33.2.i RD 1065/2007).
        if !st.totals.withholding.is_zero() {
            continue;
        }
        // Por sustitución, lo que cambia: su importe menos el de las que sustituye.
        // `TODO(VERIFY)`: si las rectificativas por impago o concurso (R2/R3, art. 80.Tres y
        // Cuatro LIVA) restan en el 347; aquí restan como el resto de rectificativas.
        let (breakdown, _) =
            crate::rectificativas_emitidas::net_breakdown(v.connection(), &st.invoice, &st.totals)?;
        let amount: Money = breakdown
            .iter()
            .filter(|t| {
                !matches!(
                    t.vat,
                    VatTreatment::Exempt(
                        Exemption::E2 | Exemption::E3 | Exemption::E4 | Exemption::E5
                    ) | VatTreatment::NotSubject(NotSubject::N2)
                )
            })
            .map(|t| t.base + t.vat_amount + t.surcharge_amount)
            .sum();
        ops.push(Op347 {
            nif: nif.clone(),
            name: r.name.clone(),
            province: province_of(r.address.as_ref()),
            key: Key347::B,
            quarter: quarter_of(d),
            amount,
            business_rental: false,
            cash: cash_by_invoice.get(&st.id).copied().unwrap_or(Money::ZERO),
        });
    }
    let (start, _) = quarter_range(year, 1)?;
    let (_, end) = quarter_range(year, 4)?;
    let names = supplier_names(v)?;
    let mut foreign = 0usize;
    for r in received_between(v, start, end)? {
        // Las importaciones (DUA) no van en el 347 (art. 33.2 RGAT).
        if !r.withholding.is_zero() || r.nif.is_empty() || r.import_dua {
            continue;
        }
        // Proveedores sin NIF español (p. ej. de otro país de la UE): no van en el 347; las
        // adquisiciones intracomunitarias se declaran en el 349.
        if taxpain_core::Nif::parse(&r.nif).is_err() {
            foreign += 1;
            continue;
        }
        ops.push(Op347 {
            name: names.get(&r.nif).cloned().unwrap_or_default(),
            nif: r.nif,
            province: own_province.clone(),
            key: Key347::A,
            quarter: quarter_of(r.date),
            amount: r.base + r.vat,
            business_rental: false,
            cash: Money::ZERO,
        });
    }
    let mut r = informative::compute_347(year, &ops);
    for (nif, (y, m)) in cash_other_years {
        r.warnings.push(format!(
            "{nif}: {m} cobrados en metálico en {year} de facturas de {y}. Si con ellos pasa de \
             6.000 € en metálico, van en el 347 de {year} en un registro aparte con ejercicio \
             {y} (art. 34.1.h RGAT): TaxPain no lo genera, añádelo en la Sede"
        ));
    }
    if foreign > 0 {
        r.warnings.push(format!(
            "{foreign} factura(s) de proveedores sin NIF español no se incluyen en el 347"
        ));
    }
    Ok(r)
}

/// Cobros en metálico de emitidas registrados en Cobros y pagos en el año (`settlement.method`
/// = «efectivo»): por factura, para las del propio año, y aparte por cliente y año de las
/// facturas de otros años (NIF → (año de la factura, importe)).
#[allow(clippy::type_complexity)]
fn cash_collected_347(
    v: &Vault,
    year: i32,
) -> Result<(HashMap<i64, Money>, BTreeMap<String, (i32, Money)>)> {
    let mut stmt = v.connection().prepare(
        "SELECT s.invoice_id, COALESCE(i.amount, 0), CAST(substr(ii.issue_date, 1, 4) AS INTEGER),
                COALESCE(ii.recipient_tax_id, '')
           FROM settlement s
           JOIN invoice_issued ii ON ii.id = s.invoice_id
           LEFT JOIN invoice_installment i
             ON i.kind = s.kind AND i.invoice_id = s.invoice_id AND i.seq = s.seq
          WHERE s.kind = 'emitida' AND s.method = 'efectivo'
            AND CAST(substr(s.date, 1, 4) AS INTEGER) = ?1",
    )?;
    let rows = stmt.query_map([year], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i32>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut by_invoice: HashMap<i64, Money> = HashMap::new();
    let mut other: BTreeMap<String, (i32, Money)> = BTreeMap::new();
    for row in rows {
        let (id, cents, y, nif) = row?;
        let m = Money::from_cents(cents);
        if y == year {
            *by_invoice.entry(id).or_default() += m;
        } else if !nif.is_empty() {
            let e = other.entry(nif).or_insert((y, Money::ZERO));
            e.1 += m;
        }
    }
    Ok((by_invoice, other))
}

/// Cartas del 347: una por cliente o proveedor con el importe anual y por trimestres que se va a
/// declarar, para que lo comprueben antes de presentar (práctica habitual; evita discrepancias
/// en las imputaciones de la AEAT). Devuelve (nombre del fichero, PDF).
pub fn letters_347(v: &Vault, year: i32, place_date: &str) -> Result<(String, Vec<u8>)> {
    use taxpain_invoicing::pdf::{Para, text_document};
    use taxpain_models::informative::Key347;
    let r = result_347(v, year)?;
    if r.lines.is_empty() {
        return Err(AppError::Other(format!(
            "en {year} no hay terceros que superen 3.005,06 €: no hay cartas que enviar"
        )));
    }
    let who = declarant(v)?;
    let fmt = |m: Money| m.to_string();
    let docs: Vec<Vec<Para>> = r
        .lines
        .iter()
        .map(|l| {
            let (what, us) = if l.key == Key347::A {
                ("compras que os hemos hecho", "adquisiciones")
            } else {
                ("ventas que os hemos hecho", "entregas")
            };
            vec![
                Para::Bold(format!("{} · NIF {}", who.full_name(), who.nif)),
                Para::Text(place_date.to_owned()),
                Para::Gap,
                Para::Bold(format!("{} · NIF {}", l.name, l.nif)),
                Para::Gap,
                Para::Title(format!("Declaración anual de operaciones con terceros (modelo 347) {year}")),
                Para::Text(format!(
                    "Os comunicamos el importe de las {what} durante {year}, IVA incluido, que vamos a \
                     declarar en el modelo 347 como {us} (clave {}):",
                    if l.key == Key347::A { "A" } else { "B" }
                )),
                Para::Row("Primer trimestre".into(), fmt(l.quarters[0])),
                Para::Row("Segundo trimestre".into(), fmt(l.quarters[1])),
                Para::Row("Tercer trimestre".into(), fmt(l.quarters[2])),
                Para::Row("Cuarto trimestre".into(), fmt(l.quarters[3])),
                Para::Row("Total del año".into(), fmt(l.total)),
                Para::Gap,
                Para::Text(
                    "Si vuestros datos no coinciden, os rogamos que nos lo comuniquéis cuanto \
                     antes para revisarlo antes de presentar la declaración. Si coinciden, no hace \
                     falta contestar."
                        .into(),
                ),
                Para::Gap,
                Para::Text("Un saludo.".into()),
            ]
        })
        .collect();
    let pdf =
        text_document(&format!("Cartas del 347 de {year}"), &docs).map_err(AppError::Other)?;
    Ok((format!("Cartas 347 {year} {}.pdf", who.nif), pdf))
}

pub fn draft_347(v: &Vault, year: i32) -> Result<Draft> {
    use taxpain_models::informative::{self, Key347};
    let r = result_347(v, year)?;
    let who = declarant(v)?;
    let file = informative::file_347(&r, &who)?;
    let details = r
        .lines
        .iter()
        .map(|l| {
            let k = if l.key == Key347::A {
                "compras"
            } else {
                "ventas"
            };
            (format!("{} · {} · {k}", l.name, l.nif), l.total)
        })
        .collect();
    let mut terceros = Vec::new();
    for l in &r.lines {
        tercero(&mut terceros, &l.name, &l.nif);
    }
    let mut d = informative_draft(v, "347", year, r.total, details, r.warnings, file)?;
    d.terceros = terceros;
    Ok(d)
}

fn supplier_names(v: &Vault) -> Result<BTreeMap<String, String>> {
    let mut stmt = v.connection().prepare(
        "SELECT supplier_nif, supplier_name FROM invoice_received WHERE status = 'revisada'",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?,
            r.get::<_, Option<String>>(1)?,
        ))
    })?;
    let mut out = BTreeMap::new();
    for row in rows {
        if let (Some(n), Some(name)) = row? {
            out.insert(n, name);
        }
    }
    Ok(out)
}

/// 190 (profesionales, clave G) y 180 (alquileres) del año, desde las recibidas con retención.
/// Aviso si el resumen anual no cuadra con la suma de los trimestrales (190 con los 111, 180 con
/// los 115): se comparan las retenciones; los presentados (en producción o por el gestor) mandan.
fn check_quarters(v: &Vault, quarterly: &str, year: i32, annual: Money) -> Result<Option<String>> {
    let mut sum = Money::ZERO;
    for q in 1..=4u8 {
        sum += match filed_boxes(v, quarterly, year, &format!("{q}T"))? {
            // Resultado a ingresar: [30] en el 111, [14] en el 123 y [05] en el 115.
            Some((_, boxes)) => boxes
                .get(&match quarterly {
                    "111" => 30,
                    "123" => 14,
                    _ => 5,
                })
                .copied()
                .unwrap_or(Money::ZERO)
                .max(Money::ZERO),
            None => draft(v, quarterly, year, q)
                .map(|d| d.result.max(Money::ZERO))
                .unwrap_or(Money::ZERO),
        };
    }
    Ok(((annual - sum).abs() >= Money::from_euros(1, 0)).then(|| {
        format!(
            "Las retenciones del resumen anual ({annual}) no cuadran con la suma de los {quarterly} \
             del año ({sum}): revisa los trimestres antes de presentar"
        )
    }))
}

/// Índice del detalle de gastos del 184 para una cuenta de gasto (6).
fn expense_slot_184(account: &str, rent: bool) -> usize {
    let p = |x: &str| account.starts_with(x);
    if rent {
        // Intereses, conservación, pendientes, tributos, dudoso cobro, terceros, seguros,
        // amortización del inmueble, de muebles y otros.
        match () {
            _ if p("66") => 0,
            _ if p("622") => 1,
            _ if p("63") => 3,
            _ if p("650") || p("694") => 4,
            _ if p("623") || p("629") => 5,
            _ if p("625") => 6,
            _ if p("68") => 7,
            _ => 9,
        }
    } else {
        // Personal, consumos, tributos, arrendamientos, reparaciones, servicios profesionales,
        // suministros, financieros, amortizaciones, provisiones y otros.
        match () {
            _ if p("64") => 0,
            _ if p("60") || p("61") => 1,
            _ if p("63") => 2,
            _ if p("621") => 3,
            _ if p("622") => 4,
            _ if p("623") => 5,
            _ if p("628") => 6,
            _ if p("66") => 7,
            _ if p("68") => 8,
            _ if p("69") => 9,
            _ => 10,
        }
    }
}

/// 184 de una entidad en régimen de atribución de rentas: rendimiento de la actividad (o del
/// alquiler) desde el diario, retenciones de las emitidas y atribución a los socios.
pub fn draft_184(v: &Vault, year: i32) -> Result<Draft> {
    use taxpain_models::m184::{self, Activity184, Input184, Rent184};
    let who = declarant(v)?;
    let s = crate::atribucion::settings(v)?;
    let fy = accounting::fiscal_year(v, year)?;
    let rent = s.main_activity == 3;
    let mut income = Money::ZERO;
    let mut detail = vec![Money::ZERO; if rent { 10 } else { 11 }];
    for e in accounting::journal(v, year)? {
        if matches!(e.entry.origin, Origin::Regularizacion | Origin::Cierre) {
            continue;
        }
        for l in &e.entry.lines {
            match l.account.group() {
                7 => income -= l.net(),
                6 if !l.account.starts_with("630") => {
                    if let Some(slot) = detail.get_mut(expense_slot_184(l.account.as_str(), rent)) {
                        *slot += l.net();
                    }
                }
                _ => {}
            }
        }
    }
    let withheld = issued_withholdings_between(v, fy.start, fy.end)?;
    let input = Input184 {
        entity_type: s.entity_type,
        main_activity: s.main_activity,
        turnover: income,
        representative_nif: s.representative_nif.clone(),
        representative_name: s.representative_name.clone(),
        activity: (!rent).then(|| Activity184 {
            regime: s.regime,
            kind: s.activity_kind,
            iae: s.iae.clone(),
            income,
            expenses: std::array::from_fn(|k| detail.get(k).copied().unwrap_or(Money::ZERO)),
            cash: s.cash,
        }),
        rent: rent.then(|| Rent184 {
            income,
            expenses: std::array::from_fn(|k| detail.get(k).copied().unwrap_or(Money::ZERO)),
            situation: s.rent_situation,
            cadastral_ref: s.cadastral_ref.clone(),
            urban: s.urban,
            days: s.rent_days,
        }),
        withholding_activity: if rent { Money::ZERO } else { withheld },
        withholding_rent: if rent { withheld } else { Money::ZERO },
        partners: crate::atribucion::partners_184(v, year)?,
    };
    let r = m184::compute(year, &input);
    let mut warnings = r.warnings.clone();
    if !who.nif.starts_with(['E', 'J', 'H']) {
        warnings.push(
            "El 184 lo presentan las comunidades de bienes, sociedades civiles y herencias \
             yacentes (NIF que empieza por E, J o H)"
                .into(),
        );
    }
    if s.representative_nif.trim().is_empty() {
        warnings.push("Falta el NIF del representante de la entidad".into());
    }
    let file = m184::file(&r, &input, &who)?;
    let net = input
        .activity
        .as_ref()
        .map(Activity184::net)
        .or(input.rent.as_ref().map(Rent184::net))
        .unwrap_or(Money::ZERO);
    let details = r
        .attributions
        .iter()
        .filter_map(|a| {
            input.partners.get(a.partner).map(|p| {
                (
                    format!("{} · {}{:02} · {}", p.name, a.key, a.subkey, p.nif),
                    a.amount,
                )
            })
        })
        .collect();
    let mut terceros = Vec::new();
    for p in &input.partners {
        tercero(&mut terceros, &p.name, &p.nif);
    }
    tercero(&mut terceros, &s.representative_name, &s.representative_nif);
    let mut d = informative_draft(v, "184", year, net, details, warnings, file)?;
    d.terceros = terceros;
    Ok(d)
}

/// Recibidas con retención del año, por la fecha de pago, que van en el 190 o en el 180 (las de
/// alquiler), con los nombres de los proveedores y la provincia del domicilio propio.
fn retained_received(
    v: &Vault,
    model: &str,
    year: i32,
) -> Result<(String, BTreeMap<String, String>, Vec<Received>)> {
    let inv = invoicing::settings(v)?.ok_or(AppError::NotConfigured)?;
    let own_province = province_of(inv.issuer.address.as_ref());
    let (start, _) = quarter_range(year, 1)?;
    let (_, end) = quarter_range(year, 4)?;
    let names = supplier_names(v)?;
    let rows = received_by(v, start, end, Basis::Payment)?
        .into_iter()
        .filter(|r| !r.withholding.is_zero() && (model == "180") == is_rent(r))
        .collect();
    Ok((own_province, names, rows))
}

/// Alquileres del 180 desde las recibidas con retención, con el inmueble guardado de cada
/// arrendador (`inmuebles`), si lo tiene.
fn rents_180(
    v: &Vault,
    rows: Vec<Received>,
    names: &BTreeMap<String, String>,
    own_province: &str,
) -> Result<Vec<taxpain_models::informative::Rent180>> {
    let mut props: BTreeMap<String, Option<taxpain_models::informative::Property>> =
        BTreeMap::new();
    let mut out = Vec::new();
    for r in rows {
        if !props.contains_key(&r.nif) {
            let p = crate::inmuebles::of_nif(v.connection(), &r.nif)?.map(|i| i.property());
            props.insert(r.nif.clone(), p);
        }
        out.push(taxpain_models::informative::Rent180 {
            name: names.get(&r.nif).cloned().unwrap_or_default(),
            property: props.get(&r.nif).cloned().flatten(),
            nif: r.nif,
            province: own_province.to_owned(),
            base: r.base,
            withheld: r.withholding,
        });
    }
    Ok(out)
}

/// El 180 del año tal como lo declara el borrador: un registro por arrendador e inmueble.
pub fn result_180(v: &Vault, year: i32) -> Result<taxpain_models::informative::Result180> {
    let (own_province, names, rows) = retained_received(v, "180", year)?;
    Ok(taxpain_models::informative::compute_180(
        year,
        &rents_180(v, rows, &names, &own_province)?,
    ))
}

/// El 190 del año tal como lo declara el borrador: un registro por perceptor, clave y subclave.
pub fn result_190(v: &Vault, year: i32) -> Result<taxpain_models::informative::Result190> {
    let (own_province, names, rows) = retained_received(v, "190", year)?;
    let (pays, _, _) = payments_190(
        v,
        year,
        rows,
        &names,
        &own_province,
        &mut Vec::new(),
        &mut Vec::new(),
    )?;
    Ok(taxpain_models::informative::compute_190(year, &pays))
}

/// Percepciones del 190 del año: profesionales (recibidas con retención), nóminas (clave A),
/// administradores (clave E) y dietas exceptuadas (L.01). Devuelve también los NIF con la
/// clave fijada en sus condiciones de pago y los asientos de las nóminas registradas.
#[allow(clippy::type_complexity)]
fn payments_190(
    v: &Vault,
    year: i32,
    rows: Vec<Received>,
    names: &BTreeMap<String, String>,
    own_province: &str,
    warnings: &mut Vec<String>,
    terceros: &mut Vec<String>,
) -> Result<(
    Vec<taxpain_models::informative::Payment190>,
    BTreeSet<String>,
    BTreeSet<i64>,
)> {
    use taxpain_models::informative::{self, Payment190};
    let own_province = own_province.to_owned();
    let (start, _) = quarter_range(year, 1)?;
    let (_, end) = quarter_range(year, 4)?;
    // Clave G (profesionales) con la subclave fijada en las condiciones del proveedor (G.02 a
    // G.04 y H.01 a H.04 comparten tipos de retención, así que no se deduce del tipo); sin
    // ella, G.01.
    let mut fixed_keys: BTreeSet<String> = BTreeSet::new();
    let mut pays = Vec::new();
    for r in rows {
        let key = crate::cartera::key_190_of_supplier(v.connection(), &r.nif)?
            .and_then(|k| informative::Key190::from_code(&k))
            .unwrap_or_default();
        if key != informative::Key190::default() {
            fixed_keys.insert(r.nif.clone());
        }
        pays.push(Payment190 {
            name: names.get(&r.nif).cloned().unwrap_or_default(),
            nif: r.nif,
            province: own_province.clone(),
            gross: r.base,
            withheld: r.withholding,
            employee: None,
            key,
            deductible: Money::ZERO,
        });
    }
    // Clave A: nóminas registradas, con los datos de la ficha de cada empleado. Clave E: la
    // retribución de los administradores, con la subclave de cada recibo.
    let slips = crate::payroll::payslips_of_year(v, year)?;
    let registered: BTreeSet<i64> = slips.iter().filter_map(|s| s.entry_id).collect();
    for s in &slips {
        let e = crate::payroll::employee(v.connection(), s.employee_id)?;
        let province = if e.province.is_empty() {
            own_province.clone()
        } else {
            e.province.clone()
        };
        if let Some(sub) = s.director_subkey {
            pays.push(Payment190 {
                nif: e.nif.clone(),
                name: e.name.clone(),
                province,
                gross: s.gross,
                withheld: s.irpf,
                employee: None,
                key: informative::Key190::E(sub),
                // Gastos deducibles sólo en E.01 y E.02 (cotizaciones que descuenta la entidad).
                deductible: if matches!(sub, 1 | 2) {
                    s.ss_employee
                } else {
                    Money::ZERO
                },
            });
            continue;
        }
        pays.push(Payment190 {
            nif: e.nif.clone(),
            name: e.name.clone(),
            province: if e.province.is_empty() {
                own_province.clone()
            } else {
                e.province.clone()
            },
            gross: s.gross,
            withheld: s.irpf,
            employee: Some(informative::Employee190 {
                birth_year: e.birth_year,
                family: e.family,
                spouse_nif: e.spouse_nif.clone(),
                disability: e.disability,
                contract: e.contract,
                mobility: e.mobility,
                deductible_expenses: s.ss_employee,
                children_under3: e.children_under3,
                children_rest: e.children_rest,
                children_whole: e.children_whole,
            }),
            key: informative::Key190::A,
            deductible: Money::ZERO,
        });
    }
    // Clave L.01: dietas y gastos de viaje exceptuados (art. 9 RIRPF), por perceptor.
    pays.extend(travel_190(
        v,
        start,
        end,
        &own_province,
        warnings,
        terceros,
    )?);
    Ok((pays, fixed_keys, registered))
}

pub fn draft_190_180(v: &Vault, model: &str, year: i32) -> Result<Draft> {
    use taxpain_models::informative;
    let (own_province, names, rows) = retained_received(v, model, year)?;
    let who = declarant(v)?;
    let mut warnings = vec![format!(
        "La provincia de los perceptores se toma de tu domicilio ({}): revísala",
        if own_province.is_empty() {
            "sin indicar"
        } else {
            own_province.as_str()
        }
    )];
    if model == "180" {
        let res = informative::compute_180(year, &rents_180(v, rows, &names, &own_province)?);
        warnings.extend(res.warnings.clone());
        // El diseño del 180 pide la situación y la referencia catastral de cada inmueble: sin
        // ellas el fichero no pasa la validación de la AEAT. Con el inmueble de todos los
        // arrendadores guardado (Modelos → 180 → Inmuebles arrendados) se genera el fichero; si
        // falta alguno, sólo el borrador.
        let file = if res.lines.iter().all(|l| l.property.is_some()) {
            informative::file_180(&res, &who)?
        } else {
            warnings.push(
                "El fichero del 180 necesita la situación, la referencia catastral y la \
                 dirección del inmueble de cada arrendador: guárdalas en «Inmuebles arrendados» \
                 y vuelve a preparar el borrador."
                    .into(),
            );
            String::new()
        };
        let details = res
            .lines
            .iter()
            .map(|l| (format!("{} · {}", l.name, l.nif), l.withheld))
            .collect();
        let mut terceros = Vec::new();
        for l in &res.lines {
            tercero(&mut terceros, &l.name, &l.nif);
        }
        warnings.extend(check_quarters(v, "115", year, res.total_withheld)?);
        let mut d = informative_draft(v, "180", year, res.total_withheld, details, warnings, file)?;
        d.terceros = terceros;
        return Ok(d);
    }
    let mut terceros = Vec::new();
    let (pays, fixed_keys, registered) = payments_190(
        v,
        year,
        rows,
        &names,
        &own_province,
        &mut warnings,
        &mut terceros,
    )?;
    let res = informative::compute_190(year, &pays);
    for l in &res.lines {
        tercero(&mut terceros, &l.name, &l.nif);
    }
    warnings.extend(res.warnings.iter().cloned());
    // Subclave de la clave G según el tipo retenido (arts. 95.1 y 95.6 RIRPF): sin clave en
    // las condiciones del proveedor va la G.01 (15 %). Con el 7 % (G.02, G.03 o G.04) o el 1 %
    // (H.04, estimación objetiva) se avisa, porque la AEAT cruza la subclave con la Renta del
    // perceptor; la clave se fija en Cobros y pagos → Condiciones de pago del proveedor.
    for l in res
        .lines
        .iter()
        .filter(|l| l.key == informative::Key190::default() && l.gross.cents() > 0)
        .filter(|l| !fixed_keys.contains(&l.nif))
    {
        let bp = l.withheld.cents() * 10_000 / l.gross.cents();
        let subclave = match bp {
            650..=750 => {
                "G.03 (7 %: profesional en sus tres primeros años de actividad; G.04 si tuvo \
                 menos de 15.000 € de rendimientos el año anterior, G.02 recaudadores y mediadores)"
            }
            50..=150 => "H.04 (1 %: actividad en estimación objetiva)",
            _ => continue,
        };
        warnings.push(format!(
            "{} ({}) retiene al {} %: en el 190 va con la subclave G.01 (15 %), pero le \
             corresponde la {subclave}. Ponla en sus condiciones de pago (Cobros y pagos → \
             Condiciones de pago → clave del 190) y vuelve a generar el 190",
            l.name,
            l.nif,
            bp / 100
        ));
    }
    if accounting::journal(v, year)?
        .iter()
        .any(|e| e.entry.origin == Origin::Nomina && !registered.contains(&e.id))
    {
        warnings.push(
            "Hay asientos de nómina hechos a mano: no van en la clave A del 190. Regístralas en \
             Contabilidad → Nóminas o añádelas en el formulario de la Sede."
                .into(),
        );
    }
    // Nóminas del diario importado del gestor (en los trimestres sin nóminas de TaxPain): sin
    // los datos de cada empleado no se pueden declarar en la clave A.
    let mut imported = Money::ZERO;
    for q in 1..=4u8 {
        let (qs, qe) = quarter_range(year, q)?;
        if own_payroll(v, year, qs, qe)?.is_empty() {
            imported += imported_payroll(v, year, qs, qe)?
                .iter()
                .map(|p| p.withheld)
                .sum::<Money>();
        }
    }
    if !imported.is_zero() {
        warnings.push(format!(
            "El diario importado del gestor tiene nóminas con {imported} de retenciones que no \
             van en la clave A del 190 (faltan los datos de cada empleado): regístralas en \
             Contabilidad → Nóminas o añádelas en el formulario de la Sede"
        ));
    }
    let file = informative::file_190(&res, &who)?;
    let details = res
        .lines
        .iter()
        .map(|l| {
            let key = l.effective_key();
            let amount = if key == informative::Key190::L01 {
                l.gross
            } else {
                l.withheld
            };
            (format!("{} · {} · {}", l.name, l.nif, key.label()), amount)
        })
        .collect();
    warnings.extend(check_quarters(v, "111", year, res.total_withheld)?);
    let mut d = informative_draft(v, "190", year, res.total_withheld, details, warnings, file)?;
    d.terceros = terceros;
    Ok(d)
}

/// Error si el declarante es una persona física (el 200 es de las sociedades).
pub(crate) fn exige_sociedad(v: &Vault) -> Result<Declarant> {
    let who = declarant(v)?;
    if Nif::parse(&who.nif).is_ok_and(|n| n.is_persona_fisica()) {
        return Err(AppError::Other("el modelo 200 es para sociedades".into()));
    }
    Ok(who)
}

/// 200: liquidación completa ([`crate::modelo200`]: resultado contable del diario, IS
/// contabilizado, correcciones propuestas y tecleadas, reservas, bases negativas por año,
/// deducciones, retenciones de la 473 y los tres 202) y fichero para Sociedades WEB.
pub fn draft_200(v: &Vault, year: i32) -> Result<Draft> {
    use taxpain_models::m200;
    let who = exige_sociedad(v)?;
    let s = settings(v)?;
    let (ys, _) = s.year_settings(year);
    let p = crate::modelo200::preparar(v, year)?;
    let fy = &p.fy;
    let end = fy.end;
    let input = &p.input;
    // Con la duración del periodo: en un ejercicio de transición (menos de un año) se prorratean
    // el tramo de 50.000 € y el millón de bases negativas (arts. 29.1 y 26.1 LIS).
    let mut r = crate::modelo200::liquidar(&p);
    let mut warnings = p.avisos.clone();
    warnings.append(&mut r.warnings);
    r.warnings = warnings;
    if p.legacy_year {
        r.warnings.push(LEGACY_YEAR_WARNING.to_owned());
    }
    // Deterioros y provisiones no deducibles de los ajustes de cierre: entran como correcciones
    // propuestas de `modelo200` en su casilla ([00321]/[00322], [00337]/[00338]). Se avisa para
    // que no se tecleen otra vez a mano.
    let de_cierre: Vec<String> = p
        .ajustes
        .iter()
        .filter(|a| a.clave.starts_with("cierre:") && a.importe.cents() > 0)
        .map(|a| format!("[{:05}] {} {}", a.casilla, a.concepto, a.importe))
        .collect();
    if !de_cierre.is_empty() {
        r.warnings.push(format!(
            "Correcciones de los ajustes de cierre (Contabilidad → Cierre), ya sumadas: {}. No \
             las repitas como corrección tecleada.",
            de_cierre.join("; ")
        ));
    }
    if fy.days() < fy.full_year_days() {
        r.warnings.push(format!(
            "Ejercicio de {} días: el tramo de 50.000 € de las microempresas y el millón de bases \
             negativas compensables se prorratean por días/365 (arts. 29.1 y 26.1 LIS)",
            fy.days()
        ));
    }
    // Bases negativas del año anterior que no se han indicado para éste (no se arrastran solas:
    // el importe y sus límites los revisa el usuario).
    if ys.pending_losses.is_zero() && input.bins.is_empty() {
        let (prev_ys, _) = s.year_settings(year - 1);
        let carried = match filed_boxes(v, "200", year - 1, "0A")? {
            Some((_, b)) => {
                let get = |k: u16| b.get(&k).copied().unwrap_or(Money::ZERO);
                prev_ys.pending_losses - get(547) + (-get(552)).max(Money::ZERO)
            }
            None => prev_ys.pending_losses,
        };
        if carried.cents() > 0 {
            r.warnings.push(format!(
                "De {} quedan unos {carried} de bases imponibles negativas por compensar y en \
                 {year} no hay ninguna indicada: añádelas por año en Modelos → 200 (bases \
                 negativas) o usa «Pasar al ejercicio siguiente» en el 200 de {}.",
                year - 1,
                year - 1
            ));
        } else if filed_boxes(v, "200", year - 1, "0A")?.is_none() {
            let prev_end = accounting::fiscal_year(v, year - 1).map(|f| f.end);
            if let Ok(end) = prev_end
                && let Ok((income, expenses)) = income_expenses(v, year - 1, end)
                && income < expenses
            {
                r.warnings.push(format!(
                    "En {} hubo pérdidas contables: si dieron base imponible negativa, indícalas \
                     por año en Modelos → 200 (bases negativas) para compensarlas.",
                    year - 1
                ));
            }
        }
    }
    // Las retenciones de las emitidas deberían estar todas en la 473: si no, falta contabilizar
    // alguna (p. ej. facturas importadas del libro sin el diario de la gestoría).
    let from_invoices = issued_withholdings_between(v, fy.start, end)?;
    if from_invoices > input.withholdings + Money::from_euros(1, 0) {
        r.warnings.push(format!(
            "Las retenciones de las facturas emitidas del ejercicio ({from_invoices}) superan las \
             contabilizadas en la 473 ({}): revisa que el diario esté completo antes de \
             presentar",
            input.withholdings
        ));
    }
    if !p.estimated.is_empty() {
        r.warnings.push(format!(
            "No consta presentado el 202 de {}: se usa su borrador. Si lo presentó tu gestoría, \
             impórtalo o tecléalo en Importar → Comparar, y comprueba los importes realmente \
             ingresados.",
            p.estimated.join(", ")
        ));
    }
    // Fichero para importar en Sociedades WEB, con el balance y la PyG del modelo elegido.
    use taxpain_core::accounting::statements::ModelKind;
    let kind = s.accounts_model;
    let ca = accounting::annual_accounts_model(v, year, kind)?;
    let mut lines: Vec<(String, Money)> = Vec::new();
    for l in &ca.balance.lines {
        lines.push((format!("balance:{}", l.id), l.amount));
    }
    for l in &ca.income.lines {
        lines.push((format!("pyg:{}", l.id), l.amount));
    }
    let f = m200::File200 {
        period_start: fy.start,
        period_end: end,
        cnae: s.cnae.clone(),
        model: match kind {
            ModelKind::Pymes => "pymes",
            ModelKind::Abreviado => "abreviado",
            ModelKind::Normal => "normal",
        }
        .into(),
        lines,
        iban: s.iban.clone(),
        domiciliar: s.domiciliar,
        turnover: input.turnover,
        participadas: crate::modelo200::participadas_200(&p.participadas),
    };
    let (file, file_warnings) = match m200::file(&r, &who, &f, input.regime) {
        Ok((x, w)) => (x, w),
        Err(e) => (
            String::new(),
            vec![format!("No se pudo generar el fichero: {e}")],
        ),
    };
    r.warnings.extend(file_warnings);
    if s.cnae.trim().is_empty() {
        r.warnings
            .push("Falta el CNAE en los ajustes de modelos (página 01 del 200)".into());
    }
    r.warnings.push(
        "El 200 se presenta en Sociedades WEB: importa allí este fichero, complétalo y revísalo. TaxPain no lo presenta.".into(),
    );
    Ok(Draft {
        model: "200".into(),
        year,
        quarter: 0,
        boxes: r.boxes.iter().map(|(k, v)| (*k, *v)).collect(),
        payees: vec![],
        details: vec![],
        declaration: String::new(),
        result: r.result(),
        warnings: r.warnings,
        file_name: if file.is_empty() {
            String::new()
        } else {
            format!("{}_200_{year}.200", who.nif)
        },
        file,
        terceros: vec![],
    })
}

// ---------- Dividendos (123 y 193) y gastos de viaje (190) ----------

/// Aviso para el 111 si en el periodo hay gastos de viaje que son retribución (exceso sobre los
/// límites del art. 9 RIRPF o dietas de un administrador sin relación laboral), con los
/// terceros que nombra.
fn travel_taxable_warning(
    v: &Vault,
    start: Date,
    end: Date,
) -> Result<(Option<String>, Vec<String>)> {
    let mut terceros = Vec::new();
    let people: Vec<String> = crate::viajes::totals_by_person(v, start, end)?
        .into_iter()
        .filter(|p| !p.taxable.is_zero())
        .map(|p| {
            tercero(&mut terceros, &p.name, &p.nif);
            format!("{} ({})", p.name, p.taxable)
        })
        .collect();
    let note = (!people.is_empty()).then(|| {
        format!(
            "Gastos de viaje que son retribución sujeta a retención: {}. Han de estar en sus \
             nóminas del periodo para que entren en este 111.",
            people.join(", ")
        )
    });
    Ok((note, terceros))
}

/// Registros L.01 del 190 (lo exceptuado de los viajes de cada perceptor) y avisos de lo que
/// es retribución (con los terceros que nombran, en `terceros`).
fn travel_190(
    v: &Vault,
    start: Date,
    end: Date,
    own_province: &str,
    warnings: &mut Vec<String>,
    terceros: &mut Vec<String>,
) -> Result<Vec<taxpain_models::informative::Payment190>> {
    use taxpain_models::informative::{Key190, Payment190};
    let staff = crate::payroll::employees(v)?;
    let mut out = Vec::new();
    for p in crate::viajes::totals_by_person(v, start, end)? {
        if !p.taxable.is_zero() {
            tercero(terceros, &p.name, &p.nif);
            warnings.push(format!(
                "{}: {} de gastos de viaje son retribución (exceso sobre los límites del art. 9 \
                 RIRPF o dietas sin relación laboral): han de estar en sus nóminas, con su \
                 retención, y no van en la clave L.",
                p.name, p.taxable
            ));
        }
        if p.exempt.is_zero() {
            continue;
        }
        let card = staff
            .iter()
            .find(|e| Some(e.id) == p.employee_id || e.nif == p.nif);
        let province = card
            .map(|e| e.province.clone())
            .filter(|pr| !pr.is_empty())
            .unwrap_or_else(|| own_province.to_owned());
        out.push(Payment190 {
            nif: p.nif,
            name: p.name,
            province,
            gross: p.exempt,
            withheld: Money::ZERO,
            employee: None,
            key: Key190::L01,
            deductible: Money::ZERO,
        });
    }
    Ok(out)
}

/// Líneas de dividendos entre `start` y `end`: (fecha, línea) de los socios residentes y avisos
/// por los no residentes (van al 216/296), con los socios que nombran en `terceros`.
fn dividend_lines(
    v: &Vault,
    start: Date,
    end: Date,
    warnings: &mut Vec<String>,
    terceros: &mut Vec<String>,
) -> Result<Vec<(Date, crate::socios::PayoutLine)>> {
    let mut out = Vec::new();
    for pay in crate::socios::payments_between(v, start, end)? {
        for l in pay.lines {
            if l.resident {
                out.push((pay.date, l));
            } else {
                tercero(terceros, &l.name, &l.nif);
                warnings.push(format!(
                    "{}: dividendo de {} a un socio no residente (retención {}): no va aquí, sino \
                     en el 216 y el 296 (IRNR), que TaxPain aún no prepara",
                    l.name, l.gross, l.withheld
                ));
            }
        }
    }
    Ok(out)
}

/// Intereses pagados de los préstamos recibidos con vencimiento en el periodo (ver
/// [`crate::prestamos::intereses_pagados`]), con un aviso por los que no llevan retención y no
/// son de un banco.
fn loan_interest(
    v: &Vault,
    start: Date,
    end: Date,
    warnings: &mut Vec<String>,
    terceros: &mut Vec<String>,
) -> Result<Vec<crate::prestamos::InteresPagado>> {
    let out = crate::prestamos::intereses_pagados(v.connection(), start, end)?;
    let mut avisados = BTreeSet::new();
    for i in &out {
        if i.retencion.is_zero() {
            if !i.entidad_credito && avisados.insert(i.prestamo.clone()) {
                warnings.push(format!(
                    "Préstamo «{}» de {}: intereses de {} sin retención. Si el prestamista es \
                     residente y no es un banco, la sociedad tenía que retener el 19 % (art. \
                     101.4 LIRPF; art. 128.6 LIS).",
                    i.prestamo, i.nombre, i.intereses
                ));
            }
        } else {
            tercero(terceros, &i.nombre, &i.nif);
        }
    }
    Ok(out)
}

/// 123 del trimestre: retenciones de los dividendos pagados a socios residentes y de los
/// intereses de los préstamos recibidos.
pub fn draft_123(v: &Vault, year: i32, quarter: u8) -> Result<Draft> {
    use taxpain_models::m123::{self, Renta123};
    let (start, end) = quarter_range(year, quarter)?;
    let s = settings(v)?;
    let who = declarant(v)?;
    let mut warnings = Vec::new();
    let mut terceros = Vec::new();
    let lines = dividend_lines(v, start, end, &mut warnings, &mut terceros)?;
    for (_, l) in &lines {
        tercero(&mut terceros, &l.name, &l.nif);
    }
    let mut rentas: Vec<Renta123> = lines
        .iter()
        .map(|(_, l)| Renta123 {
            payee: l.nif.clone(),
            dividend: true,
            base: l.gross,
            withheld: l.withheld,
        })
        .collect();
    // «Resto de rentas»: intereses de los préstamos recibidos (Contabilidad → Préstamos).
    let intereses = loan_interest(v, start, end, &mut warnings, &mut terceros)?;
    rentas.extend(intereses.iter().map(|i| Renta123 {
        payee: i.nif.clone(),
        dividend: false,
        base: i.intereses,
        withheld: i.retencion,
    }));
    let iban = s.domiciliar && s.iban.is_some();
    let period = periodo::codigo(quarter);
    let r = m123::compute_123(year, &period, &rentas, iban);
    if rentas.iter().all(|x| x.withheld.is_zero()) {
        warnings.push(
            "En el periodo no se pagaron rentas sometidas a retención: no hay que presentar el \
             123 (art. 108.1 RIRPF y art. 68.1 RIS)."
                .into(),
        );
    }
    if rentas.iter().any(|x| x.dividend && x.withheld.is_zero()) {
        warnings.push(
            "Los dividendos sin retención (sociedades con derecho a la exención del art. 21 LIS) \
             no van en el 123, pero sí en el 193."
                .into(),
        );
    }
    warnings.push(
        "Dividendos e intereses de los préstamos recibidos (Contabilidad → Préstamos, en «Resto \
         de rentas»). Otras rentas del capital mobiliario (arrendamiento de negocio, p. ej.) hay \
         que añadirlas en el formulario de la Sede."
            .into(),
    );
    let file = m123::file_123(&r, &who, s.iban.as_deref().filter(|_| s.domiciliar))?;
    let details = lines
        .iter()
        .filter(|(_, l)| !l.withheld.is_zero())
        .map(|(d, l)| (format!("{} · {} · {}", l.name, l.nif, d), l.withheld))
        .chain(
            intereses
                .iter()
                .filter(|i| !i.retencion.is_zero())
                .map(|i| {
                    (
                        format!(
                            "Intereses · {} · {} · {} · {}",
                            i.prestamo, i.nombre, i.nif, i.fecha
                        ),
                        i.retencion,
                    )
                }),
        )
        .collect();
    Ok(Draft {
        model: "123".into(),
        year,
        quarter,
        boxes: r.boxes.iter().map(|(k, m)| (*k, *m)).collect(),
        payees: r.rentas.clone(),
        details,
        declaration: r.declaration.code().into(),
        result: r.result(),
        warnings,
        file_name: format!("{}_{year}_{}_123.txt", who.nif, periodo::codigo(quarter)),
        file,
        terceros,
    })
}

fn dividends_193(
    lines: &[(Date, crate::socios::PayoutLine)],
) -> Vec<taxpain_models::informative::Dividend193> {
    lines
        .iter()
        .map(|(d, l)| taxpain_models::informative::Dividend193 {
            nif: l.nif.clone(),
            name: l.name.clone(),
            province: l.province.clone(),
            accrual: *d,
            gross: l.gross,
            withheld: l.withheld,
            rate: l.rate,
        })
        .collect()
}

/// Intereses de los préstamos recibidos con retención, en el formato del 193 (clave B).
fn interest_193(
    loans: &[crate::prestamos::InteresPagado],
) -> Vec<taxpain_models::informative::Interest193> {
    loans
        .iter()
        .map(|i| taxpain_models::informative::Interest193 {
            nif: i.nif.clone(),
            name: i.nombre.clone(),
            province: i.provincia.clone(),
            gross: i.intereses,
            withheld: i.retencion,
            rate: i.tipo,
        })
        .collect()
}

/// El 193 del año tal como lo declara el borrador: un registro por socio residente, naturaleza
/// y fecha de devengo, y uno por prestamista (clave B).
pub fn result_193(v: &Vault, year: i32) -> Result<taxpain_models::informative::Result193> {
    let (start, _) = quarter_range(year, 1)?;
    let (_, end) = quarter_range(year, 4)?;
    let lines = dividend_lines(v, start, end, &mut Vec::new(), &mut Vec::new())?;
    let intereses = interest_193(&loan_interest(
        v,
        start,
        end,
        &mut Vec::new(),
        &mut Vec::new(),
    )?);
    Ok(taxpain_models::informative::compute_193_with_interest(
        year,
        &dividends_193(&lines),
        &intereses,
    ))
}

/// 193 del año: un registro por socio residente, naturaleza (02 con retención, 08 sin ella) y
/// fecha de devengo; y uno por prestamista de los préstamos recibidos con retención (clave B,
/// naturaleza 03).
pub fn draft_193(v: &Vault, year: i32) -> Result<Draft> {
    use taxpain_models::informative;
    let (start, _) = quarter_range(year, 1)?;
    let (_, end) = quarter_range(year, 4)?;
    let who = declarant(v)?;
    let mut warnings = Vec::new();
    let mut terceros = Vec::new();
    let lines = dividend_lines(v, start, end, &mut warnings, &mut terceros)?;
    for (_, l) in &lines {
        tercero(&mut terceros, &l.name, &l.nif);
    }
    let intereses = interest_193(&loan_interest(v, start, end, &mut warnings, &mut terceros)?);
    let res = informative::compute_193_with_interest(year, &dividends_193(&lines), &intereses);
    for l in &res.lines {
        tercero(&mut terceros, &l.name, &l.nif);
    }
    warnings.extend(res.warnings.iter().cloned());
    if res.lines.is_empty() {
        warnings.push(format!(
            "No hay dividendos pagados a socios residentes ni intereses con retención en {year}: \
             no hay 193 que presentar."
        ));
    }
    let file = informative::file_193(&res, &who)?;
    let details = res
        .lines
        .iter()
        .map(|l| {
            (
                if l.key == 'A' {
                    format!(
                        "{} · {} · naturaleza {:02} · {} · bruto {}",
                        l.name, l.nif, l.nature, l.accrual, l.gross
                    )
                } else {
                    format!(
                        "{} · {} · clave {} naturaleza {:02} (intereses de préstamos) · bruto {}",
                        l.name, l.nif, l.key, l.nature, l.gross
                    )
                },
                l.withheld,
            )
        })
        .collect();
    warnings.extend(check_quarters(v, "123", year, res.total_withheld)?);
    let mut d = informative_draft(v, "193", year, res.total_withheld, details, warnings, file)?;
    d.terceros = terceros;
    Ok(d)
}

/// Borrador de cualquiera de los modelos soportados.
pub fn draft(v: &Vault, model: &str, year: i32, quarter: u8) -> Result<Draft> {
    let mut d = draft_inner(v, model, year, quarter)?;
    if let Some(w) = design_warning(model, year, &period_code(model, quarter)) {
        d.warnings.push(w);
    }
    if let Some(w) = monthly_warning(v, model, quarter)? {
        d.warnings.insert(0, w);
    }
    Ok(d)
}

/// Modelos que se pueden preparar por meses (303 con IVA mensual, 111, 115 y 123 de las grandes
/// empresas, 349 mensual).
pub const MENSUALES: [&str; 5] = ["303", "111", "115", "123", "349"];

/// ¿Se presenta el 303 cada mes, según el perfil fiscal (IVA mensual: gran empresa, REDEME o
/// grupo de entidades, art. 71.3 RIVA) o los ajustes (REDEME)?
fn vat_monthly(v: &Vault) -> Result<bool> {
    use taxpain_core::calendar::{Periodicity, TaxpayerProfile};
    let p: Option<TaxpayerProfile> = v.setting("taxpayer_profile")?;
    Ok(p.and_then(|p| p.iva)
        .is_some_and(|i| i.periodicity == Periodicity::Mensual)
        || settings(v)?.redeme)
}

/// ¿Se presenta `model` cada mes según el perfil fiscal? (`None`: sin perfil.)
pub fn monthly_model(v: &Vault, model: &str) -> Result<Option<bool>> {
    use taxpain_core::calendar::{Periodicity, TaxpayerProfile};
    let Some(p) = v.setting::<TaxpayerProfile>("taxpayer_profile")? else {
        return Ok(settings(v)?.redeme.then_some(model == "303"));
    };
    Ok(Some(match model {
        "303" => vat_monthly(v)?,
        "111" | "115" | "123" | "216" => p.gran_empresa,
        "349" => p.intracomunitarias == Some(Periodicity::Mensual),
        _ => false,
    }))
}

/// Si el perfil fiscal dice que el modelo se presenta cada mes (IVA mensual con SII, gran
/// empresa, 349 mensual) y se pide un trimestre, o al revés, se avisa delante: el borrador sirve
/// de comprobación, pero no es el periodo que se presenta.
fn monthly_warning(v: &Vault, model: &str, quarter: u8) -> Result<Option<String>> {
    if model == "216" {
        // El 216 todavía sale sólo por trimestres.
        return Ok((monthly_model(v, model)? == Some(true)).then(|| {
            "Tu perfil fiscal dice que el 216 lo presentas cada mes: este borrador es trimestral \
             y TaxPain todavía no hace el 216 mensual. Úsalo sólo como comprobación y presenta \
             los meses con el formulario de la Sede."
                .to_string()
        }));
    }
    if !MENSUALES.contains(&model) {
        return Ok(None);
    }
    let monthly = monthly_model(v, model)?;
    let is_month = periodo::mes(quarter).is_some();
    Ok(match (monthly, is_month) {
        (Some(true), false) => Some(format!(
            "Tu perfil fiscal dice que el {model} lo presentas cada mes: este borrador es del \
             trimestre entero y sirve sólo como comprobación. Elige el mes en «Periodo» para \
             preparar el que se presenta."
        )),
        (Some(false) | None, true) => Some(format!(
            "Tu perfil fiscal no dice que el {model} sea mensual: lo normal es presentarlo por \
             trimestres (sólo es mensual con más de 6.010.121,04 € de volumen de operaciones, en \
             el REDEME o en un grupo de entidades; el 349, con más de 50.000 € de entregas a la \
             UE en el trimestre). Revisa el perfil antes de presentar este mes."
        )),
        _ => None,
    })
}

/// El fichero se genera con el diseño de registro del generador del modelo
/// (`taxpain_models::dr_versions::generator_design`: el vigente, salvo el 390 hasta 2025, que
/// sigue con el de 2025); si el de ese ejercicio y periodo es otro, el fichero no sirve para
/// presentar. Las casillas del borrador sí valen.
///
/// Un ejercicio posterior al último diseño comprobado (2027 hoy, o el 200 y las informativas de
/// 2026) no falla: el fichero sale con el último vigente y se dice cuál
/// (`dr_versions::aviso_diseno_no_publicado`).
pub fn design_warning(model: &str, year: i32, period: &str) -> Option<String> {
    use taxpain_models::dr_versions;
    if let Some(w) = dr_versions::aviso_diseno_no_publicado(model, year) {
        return Some(w);
    }
    if dr_versions::generates_with_right_design(model, year, period) {
        return None;
    }
    // El diseño con que se escribe de verdad el fichero (no siempre el último de la lista).
    let used = dr_versions::generator_design(model, year, period)?;
    let last = dr_versions::DESIGNS.iter().rfind(|d| d.model == model)?;
    Some(match dr_versions::design(model, year, period) {
        Some(d) => format!(
            "El fichero sigue el diseño de registro {}; el de {year} {period} es otro ({}): no \
             lo presentes con este fichero (las casillas del borrador sí valen).",
            used.name, d.name
        ),
        None if year > last.from.0 => format!(
            "TaxPain aún no tiene el diseño de registro de {year} (la AEAT lo publica cerca del \
             plazo): el fichero sigue el diseño {}. Antes de presentarlo, actualiza TaxPain o \
             comprueba que la Sede lo admite; las casillas del borrador sí valen.",
            used.name
        ),
        None => format!(
            "El fichero sigue el diseño de registro {}; TaxPain no tiene el de {year} {period}: \
             no lo presentes con este fichero (las casillas del borrador sí valen).",
            used.name
        ),
    })
}

fn draft_inner(v: &Vault, model: &str, year: i32, quarter: u8) -> Result<Draft> {
    if periodo::mes(quarter).is_some() && !MENSUALES.contains(&model) {
        return Err(AppError::Other(format!(
            "el modelo {model} no tiene periodos mensuales"
        )));
    }
    match model {
        "303" => draft_303(v, year, quarter),
        "130" => draft_130(v, year, quarter),
        "111" => draft_111(v, year, quarter),
        "115" => draft_115(v, year, quarter),
        "349" => draft_349(v, year, quarter),
        "390" => draft_390(v, year),
        "202" => draft_202(v, year, quarter),
        "347" => draft_347(v, year),
        "200" => draft_200(v, year),
        "190" | "180" => draft_190_180(v, model, year),
        "184" => draft_184(v, year),
        "123" => draft_123(v, year, quarter),
        "193" => draft_193(v, year),
        "216" => crate::no_residentes::draft_216(v, year, quarter),
        "296" => crate::no_residentes::draft_296(v, year),
        m => Err(AppError::Other(format!("modelo {m} aún no disponible"))),
    }
}

/// Código de periodo del modelo para la AEAT: «3T», «2P» (202) o «0A» (anuales).
pub fn period_code(model: &str, quarter: u8) -> String {
    match model {
        "202" => format!("{quarter}P"),
        "390" | "347" | "190" | "180" | "184" | "193" | "200" | "296" => "0A".into(),
        _ => periodo::codigo(quarter),
    }
}

/// Deja constancia en la auditoría de una prevalidación enviada a la AEAT (sin efectos
/// jurídicos, pero los datos salen del Mac).
pub fn record_prevalidation(
    v: &Vault,
    d: &Draft,
    valid: bool,
    messages: usize,
    now: OffsetDateTime,
) -> Result<()> {
    use sha2::{Digest, Sha256};
    audit::append(
        v.connection(),
        &invoicing::now_str(now),
        Actor::User,
        "models.prevalidate",
        &format!(
            "{}/{}/{}",
            d.model,
            d.year,
            period_code(&d.model, d.quarter)
        ),
        &serde_json::json!({
            "sha256": hex::encode(Sha256::digest(d.file.as_bytes())),
            "valid": valid,
            "messages": messages,
        }),
    )?;
    Ok(())
}

/// Bytes del fichero (ISO-8859-1). Deja constancia en la auditoría de que se generó, con su
/// huella, sin que eso tenga efecto legal alguno: nada se presenta.
pub fn export(v: &Vault, d: &Draft, now: OffsetDateTime) -> Result<Vec<u8>> {
    if d.file.is_empty() {
        return Err(AppError::Other(format!(
            "el modelo {} aún no tiene fichero: copia las casillas del borrador",
            d.model
        )));
    }
    let bytes = dr::to_latin1(&d.file)?;
    use sha2::{Digest, Sha256};
    let hash = hex::encode(Sha256::digest(&bytes));
    audit::append(
        v.connection(),
        &invoicing::now_str(now),
        Actor::User,
        "models.export",
        &format!(
            "{}/{}/{}",
            d.model,
            d.year,
            period_code(&d.model, d.quarter)
        ),
        &serde_json::json!({ "sha256": hash, "result": d.result.cents() }),
    )?;
    Ok(bytes)
}

// ---------- Declaraciones presentadas (Fase 6) ----------

/// Justificante de una presentación aceptada por la AEAT.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FiledReceipt {
    pub csv: String,
    pub receipt_number: String,
    pub date: String,
    pub time: String,
    pub pdf_url: String,
    pub notices: Vec<String>,
    pub warnings: Vec<String>,
}

/// Una declaración presentada, tal como se guardó.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FiledReturn {
    pub id: i64,
    pub model: String,
    pub year: i32,
    pub period: String,
    pub environment: String,
    pub declaration: String,
    pub result: Money,
    pub boxes: Vec<(u16, Money)>,
    pub csv: String,
    pub receipt_number: String,
    pub pdf_url: String,
    pub presented_at: String,
    /// El justificante en PDF ya está guardado en el almacén.
    pub has_receipt_pdf: bool,
}

/// Comprobaciones antes de presentar: el NIF del fichero es el del titular de los datos y el
/// presentador es ese mismo titular (no hay apoderamientos todavía).
/// `TODO(VERIFY)`: presentación por representante o colaborador social (Fase 7+).
pub fn check_presenter(v: &Vault, presenter_nif: &str) -> Result<()> {
    let who = declarant(v)?;
    if !who.nif.eq_ignore_ascii_case(presenter_nif) {
        return Err(AppError::Other(format!(
            "el certificado es de {presenter_nif} y la declaración de {}: por ahora sólo se puede presentar en nombre propio",
            who.nif
        )));
    }
    Ok(())
}

/// Comprueba que el borrador que se va a presentar es exactamente el que el usuario revisó.
pub fn ensure_reviewed(d: &Draft, reviewed_file: &str) -> Result<()> {
    if d.file != reviewed_file {
        return Err(AppError::Other(
            "los datos han cambiado desde que revisaste el borrador: revísalo de nuevo".into(),
        ));
    }
    Ok(())
}

/// Fases de una presentación que se anotan en la auditoría además de la aceptada
/// (`models.file`): la petición (antes de enviar), el rechazo y el resultado desconocido.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilingEvent {
    Request,
    Rejected,
    /// La conexión con la AEAT no llegó a abrirse (sin red, DNS…): nada salió del Mac y no hay
    /// duda que consultar.
    NotSent,
    /// El envío pudo llegar a la AEAT pero no hubo respuesta válida (corte, tiempo agotado…).
    Unknown,
    /// El usuario consultó en la AEAT que no consta y decide reintentar.
    RetryAfterUnknown,
}

impl FilingEvent {
    fn action(self) -> &'static str {
        match self {
            FilingEvent::Request => "models.file.request",
            FilingEvent::Rejected => "models.file.rejected",
            FilingEvent::NotSent => "models.file.not_sent",
            FilingEvent::Unknown => "models.file.unknown",
            FilingEvent::RetryAfterUnknown => "models.file.retry",
        }
    }
}

fn env_name(production: bool) -> &'static str {
    if production { "produccion" } else { "pruebas" }
}

pub fn record_filing_event(
    v: &Vault,
    d: &Draft,
    production: bool,
    event: FilingEvent,
    detail: &str,
    now: OffsetDateTime,
) -> Result<()> {
    use sha2::{Digest, Sha256};
    audit::append(
        v.connection(),
        &invoicing::now_str(now),
        Actor::User,
        event.action(),
        &format!(
            "{}/{}/{}",
            d.model,
            d.year,
            period_code(&d.model, d.quarter)
        ),
        &serde_json::json!({
            "environment": env_name(production),
            "sha256": hex::encode(Sha256::digest(d.file.as_bytes())),
            "detail": detail,
        }),
    )?;
    Ok(())
}

/// ¿Hay una presentación de este modelo y periodo con resultado desconocido (o interrumpida)?
/// Mientras la haya no se reintenta sin que el usuario lo confirme tras consultar en la AEAT:
/// una autoliquidación presentada no se puede retirar.
pub fn filing_in_doubt(v: &Vault, d: &Draft, production: bool) -> Result<bool> {
    let last: Option<String> = v
        .connection()
        .query_row(
            "SELECT action FROM audit_log
             WHERE subject = ?1 AND action IN ('models.file', 'models.file.request',
               'models.file.rejected', 'models.file.not_sent', 'models.file.unknown',
               'models.file.retry')
               AND json_extract(details, '$.environment') = ?2
             ORDER BY seq DESC LIMIT 1",
            taxpain_vault::rusqlite::params![
                format!(
                    "{}/{}/{}",
                    d.model,
                    d.year,
                    period_code(&d.model, d.quarter)
                ),
                env_name(production)
            ],
            |r| r.get(0),
        )
        .optional()?;
    Ok(matches!(
        last.as_deref(),
        Some("models.file.unknown" | "models.file.request")
    ))
}

/// Guarda una presentación aceptada (inmutable) y la anota en la auditoría.
pub fn record_filing(
    v: &Vault,
    d: &Draft,
    production: bool,
    r: &FiledReceipt,
    now: OffsetDateTime,
) -> Result<i64> {
    use sha2::{Digest, Sha256};
    let conn = v.connection();
    let period = period_code(&d.model, d.quarter);
    let env = if production { "produccion" } else { "pruebas" };
    let sha = hex::encode(Sha256::digest(d.file.as_bytes()));
    let boxes = serde_json::to_string(&d.boxes).map_err(|e| AppError::Other(e.to_string()))?;
    let receipt = serde_json::to_string(r).map_err(|e| AppError::Other(e.to_string()))?;
    conn.execute_batch("SAVEPOINT record_filing")?;
    let res = (|| -> Result<i64> {
        conn.execute(
            "INSERT INTO tax_return(model, year, period, environment, declaration, result, boxes,
               file_sha256, file, csv, receipt_number, receipt, presented_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            taxpain_vault::rusqlite::params![
                d.model,
                d.year,
                period,
                env,
                d.declaration,
                d.result.cents(),
                boxes,
                sha,
                d.file,
                r.csv,
                r.receipt_number,
                receipt,
                invoicing::now_str(now),
            ],
        )?;
        let id = conn.last_insert_rowid();
        audit::append(
            conn,
            &invoicing::now_str(now),
            Actor::User,
            "models.file",
            &format!("{}/{}/{period}", d.model, d.year),
            &serde_json::json!({
                "environment": env,
                "csv": r.csv,
                "receipt": r.receipt_number,
                "sha256": sha,
                "result": d.result.cents(),
            }),
        )?;
        Ok(id)
    })();
    match &res {
        Ok(_) => conn.execute_batch("RELEASE record_filing")?,
        Err(_) => conn.execute_batch("ROLLBACK TO record_filing; RELEASE record_filing")?,
    }
    res
}

/// Declaraciones presentadas, de la más reciente a la más antigua.
pub fn filed_returns(v: &Vault, year: Option<i32>) -> Result<Vec<FiledReturn>> {
    let mut stmt = v.connection().prepare(
        "SELECT t.id, t.model, t.year, t.period, t.environment, t.declaration, t.result, t.boxes,
                t.csv, t.receipt_number, t.receipt, t.presented_at, r.document_id IS NOT NULL
         FROM tax_return t LEFT JOIN tax_return_receipt r ON r.tax_return_id = t.id
         WHERE ?1 IS NULL OR t.year = ?1 ORDER BY t.presented_at DESC, t.id DESC",
    )?;
    let rows = stmt.query_map([year], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i32>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, i64>(6)?,
            r.get::<_, String>(7)?,
            r.get::<_, String>(8)?,
            r.get::<_, String>(9)?,
            r.get::<_, String>(10)?,
            r.get::<_, String>(11)?,
            r.get::<_, bool>(12)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (
            id,
            model,
            year,
            period,
            environment,
            declaration,
            result,
            boxes,
            csv,
            num,
            receipt,
            at,
            pdf,
        ) = row?;
        let receipt: FiledReceipt = serde_json::from_str(&receipt).unwrap_or_default();
        out.push(FiledReturn {
            id,
            model,
            year,
            period,
            environment,
            declaration,
            result: Money::from_cents(result),
            boxes: serde_json::from_str(&boxes).unwrap_or_default(),
            csv,
            receipt_number: num,
            pdf_url: receipt.pdf_url,
            presented_at: at,
            has_receipt_pdf: pdf,
        });
    }
    Ok(out)
}

/// Guarda el PDF justificante de una presentación (cifrado en el almacén).
pub fn store_receipt_pdf(
    v: &Vault,
    tax_return_id: i64,
    pdf: &[u8],
    now: OffsetDateTime,
) -> Result<i64> {
    if !pdf.starts_with(b"%PDF") {
        return Err(AppError::Other(
            "el justificante descargado no es un PDF".into(),
        ));
    }
    let conn = v.connection();
    let (model, year, period, csv): (String, i32, String, String) = conn.query_row(
        "SELECT model, year, period, csv FROM tax_return WHERE id = ?1",
        [tax_return_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    conn.execute_batch("SAVEPOINT receipt_pdf")?;
    let res = (|| -> Result<i64> {
        let doc = invoicing::store_document(
            conn,
            "justificante_aeat",
            &format!("{model}_{year}_{period}_{csv}.pdf"),
            "application/pdf",
            pdf,
            now,
        )?;
        conn.execute(
            "INSERT INTO tax_return_receipt(tax_return_id, document_id, downloaded_at) VALUES (?1, ?2, ?3)",
            taxpain_vault::rusqlite::params![tax_return_id, doc, invoicing::now_str(now)],
        )?;
        Ok(doc)
    })();
    match &res {
        Ok(_) => conn.execute_batch("RELEASE receipt_pdf")?,
        Err(_) => conn.execute_batch("ROLLBACK TO receipt_pdf; RELEASE receipt_pdf")?,
    }
    res
}

/// PDF justificante guardado de una presentación, si lo hay.
pub fn receipt_pdf(v: &Vault, tax_return_id: i64) -> Result<Option<Vec<u8>>> {
    let r = v.connection().query_row(
        "SELECT d.content FROM tax_return_receipt t JOIN document d ON d.id = t.document_id
         WHERE t.tax_return_id = ?1",
        [tax_return_id],
        |r| r.get::<_, Vec<u8>>(0),
    );
    match r {
        Ok(b) => Ok(Some(b)),
        Err(taxpain_vault::rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

// ---------- Cotejo con los datos fiscales de Renta ----------

/// Una línea del cotejo: lo que dice la AEAT frente a lo que tiene TaxPain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CheckLine {
    /// `ingresos`, `retenciones`, `pago_130` o `cotizaciones`.
    pub topic: String,
    pub label: String,
    pub aeat: Money,
    pub taxpain: Money,
    /// `true` si coinciden (diferencia menor de 1 €).
    pub matches: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CrossCheck {
    pub year: i32,
    pub process_date: Option<String>,
    pub lines: Vec<CheckLine>,
    pub notes: Vec<String>,
}

const DATOS_FISCALES_KEY: &str = "datos_fiscales";

/// Guarda los datos fiscales descargados: el fichero tal cual (cifrado, como documento) y lo
/// interpretado para el cotejo.
pub fn store_tax_data(
    v: &Vault,
    bytes: &[u8],
    data: &taxpain_models::datos_fiscales::TaxData,
    now: OffsetDateTime,
) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    invoicing::store_document(
        &tx,
        "datos_fiscales",
        &format!("datos_fiscales_{}.txt", data.year),
        "text/plain",
        bytes,
        now,
    )?;
    tx.commit()?;
    v.set_setting(&format!("{DATOS_FISCALES_KEY}.{}", data.year), data)?;
    audit::append(
        conn,
        &invoicing::now_str(now),
        Actor::User,
        "renta.datos_fiscales",
        &data.year.to_string(),
        &serde_json::json!({
            "actividades": data.activities.len(),
            "pagos": data.installments.len(),
        }),
    )?;
    Ok(())
}

pub fn tax_data(v: &Vault, year: i32) -> Result<Option<taxpain_models::datos_fiscales::TaxData>> {
    Ok(v.setting(&format!("{DATOS_FISCALES_KEY}.{year}"))?)
}

/// Coteja los datos fiscales de la AEAT con lo que hay en TaxPain:
/// - ingresos y retenciones que cada cliente declaró en su 190 frente a las facturas emitidas;
/// - pagos fraccionados del 130 que constan en la AEAT frente a los presentados desde TaxPain;
/// - cotizaciones de autónomo frente a la cuenta 642 de la contabilidad.
pub fn cross_check(v: &Vault, year: i32) -> Result<Option<CrossCheck>> {
    let Some(data) = tax_data(v, year)? else {
        return Ok(None);
    };
    let close = |a: Money, b: Money| (a - b).abs() < Money::from_euros(1, 0);
    let mut lines = Vec::new();
    let mut notes = vec![
        "Los pagadores declaran en el 190 por fecha de pago y TaxPain por fecha de factura: \
         una factura de diciembre cobrada en enero puede explicar una diferencia."
            .to_owned(),
    ];

    // Facturas emitidas del año por cliente (base y retención).
    let mut ours: BTreeMap<String, (Money, Money)> = BTreeMap::new();
    for st in issued_of_year(v, year)?.values() {
        if st.invoice.issue_date.year() != year {
            continue;
        }
        if let Some(PartyId::Nif(nif)) = st.invoice.recipient.as_ref().map(|r| &r.id) {
            let e = ours.entry(nif.to_uppercase()).or_default();
            e.0 += st.totals.base;
            e.1 += st.totals.withholding;
        }
    }
    let mut theirs: BTreeMap<String, (String, Money, Money)> = BTreeMap::new();
    for a in &data.activities {
        let e = theirs.entry(a.payer_nif.to_uppercase()).or_insert((
            a.payer_name.clone(),
            Money::ZERO,
            Money::ZERO,
        ));
        e.1 += a.income;
        e.2 += a.withholding;
    }
    for (nif, (name, income, wh)) in &theirs {
        let (base, our_wh) = ours.get(nif).copied().unwrap_or_default();
        lines.push(CheckLine {
            topic: "ingresos".into(),
            label: format!("{name} ({nif}): ingresos"),
            aeat: *income,
            taxpain: base,
            matches: close(*income, base),
        });
        lines.push(CheckLine {
            topic: "retenciones".into(),
            label: format!("{name} ({nif}): retenciones"),
            aeat: *wh,
            taxpain: our_wh,
            matches: close(*wh, our_wh),
        });
    }
    for (nif, (_, wh)) in &ours {
        if !theirs.contains_key(nif) && !wh.is_zero() {
            lines.push(CheckLine {
                topic: "retenciones".into(),
                label: format!("{nif}: con retención en tus facturas pero sin datos en la AEAT"),
                aeat: Money::ZERO,
                taxpain: *wh,
                matches: false,
            });
        }
    }

    // Pagos fraccionados del 130 por trimestre.
    for q in 1..=4u8 {
        let period = format!("{q}T");
        let aeat: Money = data
            .installments
            .iter()
            .filter(|p| p.model == "130" && p.period == period)
            .map(|p| p.amount)
            .sum();
        let filed = filed_in_production(v, "130", year, &period)?.map(|(_, boxes)| {
            boxes
                .get(&19)
                .copied()
                .unwrap_or(Money::ZERO)
                .max(Money::ZERO)
        });
        if aeat.is_zero() && filed.is_none() {
            continue;
        }
        lines.push(CheckLine {
            topic: "pago_130".into(),
            label: format!("Modelo 130 {period}"),
            aeat,
            taxpain: filed.unwrap_or(Money::ZERO),
            matches: filed.is_some_and(|f| close(aeat, f)),
        });
    }

    // Cotizaciones de autónomos (RETA) frente a la cuenta 642.
    let reta: Money = data
        .contributions
        .iter()
        .filter(|c| matches!(c.regime.as_str(), "0521" | "0540" | "0825" | "0840"))
        .map(|c| c.amount)
        .sum();
    if !reta.is_zero() {
        let booked: Money = accounting::journal(v, year)?
            .iter()
            .filter(|e| !matches!(e.entry.origin, Origin::Regularizacion | Origin::Cierre))
            .flat_map(|e| e.entry.lines.iter())
            .filter(|l| l.account.starts_with("642"))
            .map(|l| l.net())
            .sum();
        lines.push(CheckLine {
            topic: "cotizaciones".into(),
            label: "Cotizaciones de autónomo (RETA)".into(),
            aeat: reta,
            taxpain: booked,
            matches: close(reta, booked),
        });
        if !close(reta, booked) {
            notes.push(
                "Las cuotas de autónomo son gasto deducible de la actividad: regístralas en la \
                 cuenta 642 (o concilia los cargos del banco con ella) para que cuenten en el 130."
                    .into(),
            );
        }
    }
    Ok(Some(CrossCheck {
        year,
        process_date: data.process_date.clone(),
        lines,
        notes,
    }))
}

// ---------- SII: facturas de un periodo para los plazos ----------

/// Una factura para los plazos del SII ([`crate::sii`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FacturaSii {
    /// `true`: emitida (libro de facturas expedidas); `false`: recibida.
    pub emitida: bool,
    pub numero: String,
    pub nif: String,
    pub nombre: String,
    /// Emitidas: la de expedición. Recibidas: la de registro (revisión en TaxPain).
    pub fecha: Date,
    /// Fecha de devengo del IVA (emitidas) o de la factura (recibidas).
    pub devengo: Date,
}

/// Emitidas expedidas y recibidas registradas entre `start` y `end`.
pub(crate) fn facturas_sii(v: &Vault, start: Date, end: Date) -> Result<Vec<FacturaSii>> {
    use taxpain_core::invoice::PartyId;
    let mut out = Vec::new();
    for st in issued_of_years(v, start.year(), end.year())? {
        let d = st.invoice.issue_date;
        if d < start || d > end {
            continue;
        }
        let (nif, nombre) = match &st.invoice.recipient {
            Some(p) => (
                match &p.id {
                    PartyId::Nif(n) => n.clone(),
                    PartyId::Foreign { country, id, .. } => format!("{country}{id}"),
                },
                p.name.clone(),
            ),
            None => (String::new(), String::new()),
        };
        out.push(FacturaSii {
            emitida: true,
            devengo: st.accrual(),
            numero: st.invoice.number.clone(),
            nif,
            nombre,
            fecha: d,
        });
    }
    let names = supplier_names(v)?;
    for r in received_between(v, start, end)? {
        out.push(FacturaSii {
            emitida: false,
            numero: r.number.clone(),
            nombre: names.get(&r.nif).cloned().unwrap_or_default(),
            nif: r.nif.clone(),
            fecha: r.reviewed,
            devengo: r.issue_date.unwrap_or(r.date),
        });
    }
    Ok(out)
}
