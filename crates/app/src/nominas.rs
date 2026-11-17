//! Nóminas calculadas en TaxPain para trabajadores por cuenta ajena del Régimen General: la
//! ficha de cotización y salario del trabajador, el cálculo del mes (`taxpain_core::nomina`), su
//! registro como recibo de salarios (con el asiento y el 111 y el 190 de [`crate::payroll`]), el
//! recibo en PDF con el modelo oficial, el resumen de los seguros sociales del mes con su pago y
//! el finiquito básico.
//!
//! Sin conexión con nada: TaxPain no envía las bases al Sistema RED ni a SILTRA (eso lo hace la
//! empresa o su autorizado RED); la liquidación que calcula la TGSS (el RLC) manda, y aquí sólo
//! se compara. La retención del IRPF es el tipo que teclea el usuario (ver
//! `taxpain_core::nomina::PROGRAMA_RETENCIONES`).
//!
//! Recibo de salarios: modelo del anexo de la Orden de 27 de diciembre de 1994 (BOE-A-1995-358),
//! en la redacción de la Orden ESS/2098/2014 (que añadió la determinación de las bases y la
//! aportación de la empresa). `TODO(VERIFY)`: que no haya una versión posterior del anexo.
//!
//! Seguros sociales: las cuotas de un mes se ingresan dentro del mes natural siguiente (art. 56
//! del Reglamento general de recaudación, RD 1415/2004); fuera de plazo llevan los recargos del
//! art. 30 TRLGSS. El asiento del devengo lo hace cada recibo (642 al debe y 476 al haber, con
//! la parte del trabajador descontada del 640); el del pago lleva la 476 al debe contra
//! tesorería.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{AccountCode, EntryLine, JournalEntry, Origin};
use taxpain_core::money::{Money, Rate};
use taxpain_core::nomina::finiquito::{self, DatosFiniquito, Finiquito};
use taxpain_core::nomina::{
    self as calculo, ClaseConcepto, ClaveCuota, Concepto, Contrato, DatosNomina, Nomina,
    PagasExtra, Retencion,
};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::format_description::well_known::Rfc3339;
use time::{Date, Month, OffsetDateTime};

use crate::accounting;
use crate::error::{AppError, Result};
use crate::payroll::{self, PayslipInput};

fn other(e: impl ToString) -> AppError {
    AppError::Other(e.to_string())
}

fn now_str(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

fn periodo(year: i32, month: u8) -> String {
    format!("{year:04}-{month:02}")
}

fn fmt_fecha(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

const MESES: [&str; 12] = [
    "enero",
    "febrero",
    "marzo",
    "abril",
    "mayo",
    "junio",
    "julio",
    "agosto",
    "septiembre",
    "octubre",
    "noviembre",
    "diciembre",
];

fn nombre_mes(month: u8) -> &'static str {
    MESES
        .get(usize::from(month.saturating_sub(1)))
        .copied()
        .unwrap_or("")
}

/// Datos de cotización y salario de un trabajador para calcular su nómina cada mes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FichaNomina {
    pub employee_id: i64,
    /// Número de afiliación a la Seguridad Social.
    #[serde(default)]
    pub naf: String,
    /// Categoría o grupo profesional del convenio.
    #[serde(default)]
    pub categoria: String,
    /// Código de cuenta de cotización de la empresa en el que está de alta.
    #[serde(default)]
    pub ccc: String,
    #[serde(default)]
    pub antiguedad: Option<Date>,
    /// CNAE de la actividad (para la tarifa de AT/EP).
    #[serde(default)]
    pub cnae: String,
    pub grupo: u8,
    #[serde(default)]
    pub contrato: Contrato,
    pub jornada: Rate,
    pub salario_base: Money,
    #[serde(default)]
    pub complementos: Vec<Concepto>,
    #[serde(default)]
    pub pagas: PagasExtra,
    pub at_it: Rate,
    pub at_ims: Rate,
    /// Tipo de retención del IRPF (del programa de la AEAT).
    pub tipo_irpf: Rate,
}

impl FichaNomina {
    /// Datos del mes con lo de la ficha (mes entero, sin horas extra ni anticipos).
    pub fn datos_del_mes(&self, year: i32, month: u8) -> DatosNomina {
        DatosNomina {
            year,
            month,
            grupo: self.grupo,
            contrato: self.contrato,
            dias: if self.grupo >= 8 {
                Month::try_from(month)
                    .map(|m| time::util::days_in_month(m, year))
                    .unwrap_or(30)
            } else {
                30
            },
            jornada: self.jornada,
            salario_base: self.salario_base,
            complementos: self.complementos.clone(),
            horas_extra: Money::ZERO,
            horas_extra_fuerza_mayor: false,
            pagas: PagasExtra {
                en_este_mes: 0,
                ..self.pagas
            },
            at_it: self.at_it,
            at_ims: self.at_ims,
            retencion: Retencion::Manual {
                tipo: self.tipo_irpf,
            },
            anticipos: Money::ZERO,
            otras_deducciones: Money::ZERO,
        }
    }
}

/// Un trabajador por cuenta ajena (no un administrador: su retribución va por la clave E con la
/// retención fija y, si es autónomo, sin cotizaciones en el recibo).
fn trabajador(v: &Vault, employee_id: i64) -> Result<payroll::Employee> {
    let e = payroll::employee(v.connection(), employee_id)?;
    if e.administrator {
        return Err(other(
            "la nómina calculada es para trabajadores por cuenta ajena: la retribución del \
             administrador se registra con los importes de su recibo",
        ));
    }
    Ok(e)
}

pub fn ficha(v: &Vault, employee_id: i64) -> Result<Option<FichaNomina>> {
    let raw: Option<String> = v
        .connection()
        .query_row(
            "SELECT datos FROM nomina_ficha WHERE employee_id = ?1",
            [employee_id],
            |r| r.get(0),
        )
        .optional()?;
    raw.map(|s| serde_json::from_str(&s).map_err(other))
        .transpose()
}

/// Guarda la ficha (comprueba que con ella se puede calcular un mes).
pub fn save_ficha(v: &Vault, f: &FichaNomina, now: OffsetDateTime) -> Result<()> {
    trabajador(v, f.employee_id)?;
    let year = now.date().year();
    let t = calculo::tablas::tabla(year)
        .or_else(|| {
            calculo::tablas::ANOS
                .last()
                .and_then(|y| calculo::tablas::tabla(*y))
        })
        .ok_or_else(|| other("sin tablas de cotización"))?;
    calculo::calcular_con(t, &f.datos_del_mes(t.year, 1)).map_err(other)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO nomina_ficha(employee_id, datos, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(employee_id) DO UPDATE SET datos = excluded.datos,
           updated_at = excluded.updated_at",
        params![
            f.employee_id,
            serde_json::to_string(f).map_err(other)?,
            now_str(now)
        ],
    )?;
    // Ni salario ni datos personales en la auditoría: sólo el cambio.
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "nomina_ficha.save",
        &f.employee_id.to_string(),
        &serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

/// Calcula la nómina del mes (sin guardar nada).
pub fn calcular(d: &DatosNomina) -> Result<Nomina> {
    calculo::calcular(d).map_err(other)
}

/// Nómina calculada que se registra como recibo de salarios.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistroNomina {
    pub employee_id: i64,
    /// Fecha del recibo (la del pago o la del último día del mes).
    pub fecha: Date,
    pub datos: DatosNomina,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Guardado {
    datos: DatosNomina,
    nomina: Nomina,
}

/// Registra la nómina calculada: el recibo con su asiento (640 y 642 contra 4751, 476 y 465),
/// que va al 111 y a la clave A del 190, y el cálculo para el recibo en PDF y los seguros
/// sociales del mes. Devuelve el id del recibo.
pub fn registrar(v: &Vault, r: &RegistroNomina, now: OffsetDateTime) -> Result<i64> {
    trabajador(v, r.employee_id)?;
    let nomina = calcular(&r.datos)?;
    let devengo = Date::from_calendar_date(
        r.datos.year,
        Month::try_from(r.datos.month).map_err(other)?,
        1,
    )
    .map_err(other)?;
    if r.fecha < devengo {
        return Err(other(
            "la fecha del recibo no puede ser anterior al mes que liquida",
        ));
    }
    let per = periodo(r.datos.year, r.datos.month);
    let conn = v.connection();
    let repetida: Option<i64> = conn
        .query_row(
            "SELECT c.payslip_id FROM nomina_calculo c JOIN payslip p ON p.id = c.payslip_id
             WHERE c.periodo = ?1 AND p.employee_id = ?2",
            params![per, r.employee_id],
            |x| x.get(0),
        )
        .optional()?;
    if repetida.is_some() {
        return Err(other(format!(
            "ya hay una nómina calculada de ese trabajador de {} de {}: bórrala antes si quieres \
             rehacerla",
            nombre_mes(r.datos.month),
            r.datos.year
        )));
    }
    let tx = conn.unchecked_transaction()?;
    let id = payroll::register_in(
        v,
        &tx,
        &PayslipInput {
            employee_id: r.employee_id,
            date: r.fecha,
            gross: nomina.total_devengado,
            irpf: nomina.retencion_irpf,
            ss_employee: nomina.total_trabajador,
            ss_company: nomina.total_empresa,
            small_entity: None,
        },
        now,
    )?;
    let guardado = Guardado {
        datos: r.datos.clone(),
        nomina,
    };
    tx.execute(
        "INSERT INTO nomina_calculo(payslip_id, periodo, datos, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            id,
            per,
            serde_json::to_string(&guardado).map_err(other)?,
            now_str(now)
        ],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "nomina.calculada",
        &id.to_string(),
        &serde_json::json!({ "periodo": per }),
    )?;
    tx.commit()?;
    Ok(id)
}

/// Recibos del año natural `year` registrados con una nómina calculada.
pub fn calculadas(v: &Vault, year: i32) -> Result<Vec<i64>> {
    let mut st = v.connection().prepare(
        "SELECT payslip_id FROM nomina_calculo WHERE periodo LIKE ?1 || '-%' ORDER BY periodo",
    )?;
    let rows = st.query_map([format!("{year:04}")], |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

struct Recibo {
    employee: payroll::Employee,
    fecha: Date,
    guardado: Guardado,
}

fn recibo(v: &Vault, payslip_id: i64) -> Result<Recibo> {
    let row: Option<(i64, String, String)> = v
        .connection()
        .query_row(
            "SELECT p.employee_id, p.date, c.datos FROM nomina_calculo c
             JOIN payslip p ON p.id = c.payslip_id WHERE c.payslip_id = ?1",
            [payslip_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let (employee_id, fecha, datos) = row.ok_or_else(|| {
        other("ese recibo no se calculó en TaxPain: el recibo en PDF es el de la gestoría")
    })?;
    Ok(Recibo {
        employee: payroll::employee(v.connection(), employee_id)?,
        fecha: Date::parse(
            &fecha,
            time::macros::format_description!("[year]-[month]-[day]"),
        )
        .map_err(other)?,
        guardado: serde_json::from_str(&datos).map_err(other)?,
    })
}

/// Recibo individual de salarios en PDF con el modelo oficial. Devuelve el nombre del fichero
/// y el PDF.
pub fn recibo_pdf(v: &Vault, payslip_id: i64) -> Result<(String, Vec<u8>)> {
    use taxpain_invoicing::pdf::{Para, text_document};
    let r = recibo(v, payslip_id)?;
    let f = ficha(v, r.employee.id)?;
    let s = crate::invoicing::require_settings(v)?;
    let empresa_nif = match &s.issuer.id {
        taxpain_core::invoice::PartyId::Nif(n) => n.clone(),
        _ => String::new(),
    };
    let domicilio = s
        .issuer
        .address
        .as_ref()
        .map(|a| format!("{}, {} {}", a.street, a.post_code, a.town))
        .unwrap_or_default();
    let d = &r.guardado.datos;
    let n = &r.guardado.nomina;
    let month = Month::try_from(d.month).map_err(other)?;
    let desde = Date::from_calendar_date(d.year, month, 1).map_err(other)?;
    let hasta = Date::from_calendar_date(d.year, month, time::util::days_in_month(month, d.year))
        .map_err(other)?;
    let dato = |x: Option<&str>| {
        x.filter(|s| !s.trim().is_empty())
            .unwrap_or("—")
            .to_string()
    };
    let mut doc = vec![
        Para::Title("Recibo individual justificativo del pago de salarios".into()),
        Para::Text(format!(
            "Empresa: {} · CIF/NIF {} · Domicilio: {} · Código de cuenta de cotización a la \
             Seguridad Social: {}",
            s.issuer.name,
            empresa_nif,
            dato(Some(&domicilio)),
            dato(f.as_ref().map(|f| f.ccc.as_str()))
        )),
        Para::Text(format!(
            "Trabajador: {} · NIF {} · Número de afiliación a la Seguridad Social: {} · \
             Categoría o grupo profesional: {} · Grupo de cotización: {} · Fecha de antigüedad \
             en la empresa: {}",
            r.employee.name,
            r.employee.nif,
            dato(f.as_ref().map(|f| f.naf.as_str())),
            dato(f.as_ref().map(|f| f.categoria.as_str())),
            n.grupo,
            f.as_ref()
                .and_then(|f| f.antiguedad)
                .map(fmt_fecha)
                .unwrap_or_else(|| "—".into())
        )),
        Para::Text(format!(
            "Periodo de liquidación: del {} al {} · Total días: {}",
            fmt_fecha(desde),
            fmt_fecha(hasta),
            d.dias
        )),
        Para::Gap,
        Para::Bold("I. DEVENGOS".into()),
        Para::Text("1. Percepciones salariales".into()),
    ];
    for x in n
        .devengos
        .iter()
        .filter(|x| x.clase == ClaseConcepto::Salarial)
    {
        let concepto = if x.extraordinaria {
            format!("Gratificaciones extraordinarias: {}", x.concepto)
        } else {
            x.concepto.clone()
        };
        doc.push(Para::Row(concepto, x.importe.to_string()));
    }
    let no_salariales: Vec<_> = n
        .devengos
        .iter()
        .filter(|x| x.clase != ClaseConcepto::Salarial)
        .collect();
    doc.push(Para::Text("2. Percepciones no salariales".into()));
    if no_salariales.is_empty() {
        doc.push(Para::Row("—".into(), Money::ZERO.to_string()));
    }
    for x in no_salariales {
        doc.push(Para::Row(
            format!("Indemnizaciones o suplidos: {}", x.concepto),
            x.importe.to_string(),
        ));
    }
    doc.push(Para::Row(
        "A. TOTAL DEVENGADO".into(),
        n.total_devengado.to_string(),
    ));
    doc.push(Para::Gap);
    doc.push(Para::Bold("II. DEDUCCIONES".into()));
    doc.push(Para::Text(
        "1. Aportación del trabajador a las cotizaciones a la Seguridad Social y conceptos de \
         recaudación conjunta"
            .into(),
    ));
    for c in n.cuotas.iter().filter(|c| !c.trabajador.is_zero()) {
        let tipo = if c.tipo_trabajador.bp() > 0 {
            format!(" ({})", c.tipo_trabajador)
        } else {
            String::new()
        };
        doc.push(Para::Row(
            format!("{}{tipo}", c.concepto),
            c.trabajador.to_string(),
        ));
    }
    doc.push(Para::Row(
        "Total aportaciones".into(),
        n.total_trabajador.to_string(),
    ));
    doc.push(Para::Row(
        format!(
            "2. Impuesto sobre la Renta de las Personas Físicas ({})",
            n.tipo_irpf
        ),
        n.retencion_irpf.to_string(),
    ));
    doc.push(Para::Row("3. Anticipos".into(), n.anticipos.to_string()));
    doc.push(Para::Row(
        "4. Valor de los productos recibidos en especie".into(),
        Money::ZERO.to_string(),
    ));
    doc.push(Para::Row(
        "5. Otras deducciones".into(),
        n.otras_deducciones.to_string(),
    ));
    doc.push(Para::Row(
        "B. TOTAL A DEDUCIR".into(),
        n.total_deducciones.to_string(),
    ));
    doc.push(Para::Row(
        "LÍQUIDO TOTAL A PERCIBIR (A − B)".into(),
        n.liquido.to_string(),
    ));
    doc.push(Para::Gap);
    doc.push(Para::Text(format!(
        "Fecha: {} · Firma y sello de la empresa · Recibí:",
        fmt_fecha(r.fecha)
    )));
    doc.push(Para::Gap);
    doc.push(Para::Bold(
        "DETERMINACIÓN DE LAS BASES DE COTIZACIÓN A LA SEGURIDAD SOCIAL Y CONCEPTOS DE \
         RECAUDACIÓN CONJUNTA Y DE LA BASE SUJETA A RETENCIÓN DEL IRPF Y APORTACIÓN DE LA \
         EMPRESA"
            .into(),
    ));
    doc.push(Para::Text("1. Contingencias comunes".into()));
    doc.push(Para::Row(
        "Importe remuneración mensual".into(),
        n.remuneracion_mensual.to_string(),
    ));
    doc.push(Para::Row(
        "Importe prorrata pagas extraordinarias".into(),
        n.prorrata_pagas.to_string(),
    ));
    doc.push(Para::Row(
        "Total base de contingencias comunes".into(),
        n.base_comunes.to_string(),
    ));
    let empresa = |claves: &[ClaveCuota], doc: &mut Vec<Para>| {
        for c in n.cuotas.iter().filter(|c| claves.contains(&c.clave)) {
            doc.push(Para::Row(
                format!(
                    "Aportación de la empresa: {} ({} × {})",
                    c.concepto, c.base, c.tipo_empresa
                ),
                c.empresa.to_string(),
            ));
        }
    };
    empresa(&[ClaveCuota::Comunes, ClaveCuota::Mei], &mut doc);
    doc.push(Para::Text(
        "2. Contingencias profesionales y conceptos de recaudación conjunta".into(),
    ));
    doc.push(Para::Row(
        "Base (AT y EP, desempleo, formación profesional y FOGASA)".into(),
        n.base_profesionales.to_string(),
    ));
    empresa(
        &[
            ClaveCuota::AtIt,
            ClaveCuota::AtIms,
            ClaveCuota::Desempleo,
            ClaveCuota::Formacion,
            ClaveCuota::Fogasa,
        ],
        &mut doc,
    );
    doc.push(Para::Text(
        "3. Cotización adicional horas extraordinarias".into(),
    ));
    doc.push(Para::Row("Base".into(), n.base_horas_extra.to_string()));
    empresa(&[ClaveCuota::HorasExtra, ClaveCuota::Solidaridad], &mut doc);
    doc.push(Para::Row(
        "4. Base sujeta a retención del IRPF".into(),
        n.base_irpf.to_string(),
    ));
    doc.push(Para::Row(
        "Total aportación de la empresa".into(),
        n.total_empresa.to_string(),
    ));
    doc.push(Para::Gap);
    doc.push(Para::Text(format!(
        "Modelo de recibo de la Orden de 27 de diciembre de 1994 (redacción de la Orden \
         ESS/2098/2014). Calculado por TaxPain con {}; la retención es el tipo indicado por la \
         empresa. Las cuotas que valen son las de la liquidación de la TGSS.",
        n.norma
    )));
    let pdf = text_document("Recibo de salarios", &[doc]).map_err(other)?;
    Ok((
        format!(
            "Recibo de salarios {} {}.pdf",
            r.employee.nif,
            periodo(d.year, d.month)
        ),
        pdf,
    ))
}

/// Una línea del resumen de cotizaciones del mes, por concepto.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ConceptoSeguros {
    pub clave: ClaveCuota,
    pub concepto: String,
    pub empresa: Money,
    pub trabajador: Money,
    pub total: Money,
}

/// Un trabajador en el resumen del mes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TrabajadorSeguros {
    pub payslip_id: i64,
    pub employee: String,
    pub nif: String,
    pub base_comunes: Money,
    pub base_profesionales: Money,
    pub trabajador: Money,
    pub empresa: Money,
    /// Registrado con los importes del recibo (sin desglose por concepto).
    pub sin_desglose: bool,
}

/// Línea de un asiento del resumen.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LineaAsiento {
    pub cuenta: String,
    pub nombre: String,
    pub debe: Money,
    pub haber: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PagoSeguros {
    pub fecha: Date,
    pub importe: Money,
    pub cuenta: String,
    pub entry_id: Option<i64>,
}

/// Seguros sociales de un mes: lo que la empresa ingresa a la TGSS por la liquidación de
/// cuotas (RLC) de ese mes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ResumenSeguros {
    pub year: i32,
    pub month: u8,
    pub conceptos: Vec<ConceptoSeguros>,
    pub trabajadores: Vec<TrabajadorSeguros>,
    pub total_trabajador: Money,
    pub total_empresa: Money,
    /// Total a ingresar (trabajador + empresa).
    pub total: Money,
    /// Último día del mes siguiente…
    pub vence_nominal: Date,
    /// …o el hábil anterior si es inhábil (como el calendario).
    pub vence: Date,
    /// Asiento del devengo (suma de los de los recibos del mes, ya hechos).
    pub devengo: Vec<LineaAsiento>,
    pub pago: Option<PagoSeguros>,
    pub avisos: Vec<String>,
}

fn rango_mes(year: i32, month: u8) -> Result<(Date, Date)> {
    let m = Month::try_from(month).map_err(other)?;
    Ok((
        Date::from_calendar_date(year, m, 1).map_err(other)?,
        Date::from_calendar_date(year, m, time::util::days_in_month(m, year)).map_err(other)?,
    ))
}

/// Último día del mes siguiente al de devengo, y el hábil anterior si es inhábil.
pub fn vencimiento(v: &Vault, year: i32, month: u8) -> Result<(Date, Date)> {
    let (_, fin) = rango_mes(year, month)?;
    let siguiente = fin
        .next_day()
        .ok_or_else(|| other("fecha fuera de rango"))?;
    let (_, nominal) = rango_mes(siguiente.year(), u8::from(siguiente.month()))?;
    let profile = v
        .setting::<taxpain_core::calendar::TaxpayerProfile>("taxpayer_profile")
        .ok()
        .flatten();
    let (h, _) = taxpain_core::calendar::calendar_for(
        nominal.year(),
        profile.as_ref().and_then(|p| p.ccaa),
        profile
            .as_ref()
            .map(|p| p.festivos_locales.as_slice())
            .unwrap_or(&[]),
    );
    Ok((nominal, h.roll_back(nominal)))
}

pub fn seguros_sociales(v: &Vault, year: i32, month: u8) -> Result<ResumenSeguros> {
    let per = periodo(year, month);
    let (inicio, fin) = rango_mes(year, month)?;
    let conn = v.connection();
    let mut conceptos: BTreeMap<ClaveCuota, (Money, Money)> = BTreeMap::new();
    let mut trabajadores = Vec::new();
    let mut gross = Money::ZERO;
    let mut irpf = Money::ZERO;
    {
        let mut st = conn.prepare(
            "SELECT p.id, e.name, e.nif, c.datos, p.gross, p.irpf FROM nomina_calculo c
             JOIN payslip p ON p.id = c.payslip_id JOIN employee e ON e.id = p.employee_id
             WHERE c.periodo = ?1 ORDER BY e.name, p.id",
        )?;
        let rows = st.query_map([&per], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })?;
        for row in rows {
            let (id, name, nif, datos, g, i) = row?;
            let n = serde_json::from_str::<Guardado>(&datos)
                .map_err(other)?
                .nomina;
            for c in &n.cuotas {
                let e = conceptos.entry(c.clave).or_default();
                e.0 += c.empresa;
                e.1 += c.trabajador;
            }
            gross += Money::from_cents(g);
            irpf += Money::from_cents(i);
            trabajadores.push(TrabajadorSeguros {
                payslip_id: id,
                employee: name,
                nif,
                base_comunes: n.base_comunes,
                base_profesionales: n.base_profesionales,
                trabajador: n.total_trabajador,
                empresa: n.total_empresa,
                sin_desglose: false,
            });
        }
    }
    // Recibos tecleados del mes (fecha del recibo) con cotizaciones: suman al total sin desglose.
    {
        let mut st = conn.prepare(
            "SELECT p.id, e.name, e.nif, p.ss_employee, p.ss_company, p.gross, p.irpf
             FROM payslip p JOIN employee e ON e.id = p.employee_id
             WHERE p.date BETWEEN ?1 AND ?2
               AND p.id NOT IN (SELECT payslip_id FROM nomina_calculo)
               AND (p.ss_employee > 0 OR p.ss_company > 0)
             ORDER BY e.name, p.id",
        )?;
        let rows = st.query_map(params![inicio.to_string(), fin.to_string()], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })?;
        for row in rows {
            let (id, name, nif, se, sc, g, i) = row?;
            gross += Money::from_cents(g);
            irpf += Money::from_cents(i);
            trabajadores.push(TrabajadorSeguros {
                payslip_id: id,
                employee: name,
                nif,
                base_comunes: Money::ZERO,
                base_profesionales: Money::ZERO,
                trabajador: Money::from_cents(se),
                empresa: Money::from_cents(sc),
                sin_desglose: true,
            });
        }
    }
    let total_trabajador: Money = trabajadores.iter().map(|t| t.trabajador).sum();
    let total_empresa: Money = trabajadores.iter().map(|t| t.empresa).sum();
    let total = total_trabajador + total_empresa;
    let conceptos = conceptos
        .into_iter()
        .filter(|(_, (e, t))| !(e.is_zero() && t.is_zero()))
        .map(|(clave, (empresa, trabajador))| ConceptoSeguros {
            clave,
            concepto: clave.nombre().into(),
            empresa,
            trabajador,
            total: empresa + trabajador,
        })
        .collect();
    let (vence_nominal, vence) = vencimiento(v, year, month)?;
    let linea = |cuenta: &str, nombre: &str, debe: Money, haber: Money| LineaAsiento {
        cuenta: cuenta.into(),
        nombre: nombre.into(),
        debe,
        haber,
    };
    let devengo = if trabajadores.is_empty() {
        Vec::new()
    } else {
        vec![
            linea("640", "Sueldos y salarios", gross, Money::ZERO),
            linea(
                "642",
                "Seguridad Social a cargo de la empresa",
                total_empresa,
                Money::ZERO,
            ),
            linea(
                "4751",
                "H. P., acreedora por retenciones practicadas",
                Money::ZERO,
                irpf,
            ),
            linea(
                "476",
                "Organismos de la Seguridad Social, acreedores",
                Money::ZERO,
                total,
            ),
            linea(
                "465",
                "Remuneraciones pendientes de pago",
                Money::ZERO,
                gross - irpf - total_trabajador,
            ),
        ]
    };
    let pago = conn
        .query_row(
            "SELECT fecha, importe, cuenta, entry_id FROM seguros_sociales_pago
             WHERE periodo = ?1",
            [&per],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<i64>>(3)?,
                ))
            },
        )
        .optional()?
        .map(|(f, importe, cuenta, entry_id)| -> Result<PagoSeguros> {
            Ok(PagoSeguros {
                fecha: Date::parse(
                    &f,
                    time::macros::format_description!("[year]-[month]-[day]"),
                )
                .map_err(other)?,
                importe: Money::from_cents(importe),
                cuenta,
                entry_id,
            })
        })
        .transpose()?;
    let mut avisos = vec![format!(
        "Plazo de ingreso: hasta el {} (último día del mes siguiente, art. 56 RD 1415/2004{}). \
         Fuera de plazo hay recargo (art. 30 TRLGSS).",
        fmt_fecha(vence),
        if vence == vence_nominal {
            String::new()
        } else {
            format!(
                "; el {} es inhábil y, como en el calendario, se avisa el hábil anterior",
                fmt_fecha(vence_nominal)
            )
        }
    )];
    if trabajadores.iter().any(|t| t.sin_desglose) {
        avisos.push(
            "Hay recibos registrados con los importes de la gestoría: suman al total, sin \
             desglose por concepto."
                .into(),
        );
    }
    avisos.push(
        "TaxPain no envía nada al Sistema RED ni a SILTRA: compara este total con el de la \
         liquidación de cuotas (RLC) de la TGSS, que es el que se carga en el banco."
            .into(),
    );
    if let Some(p) = &pago
        && p.importe != total
    {
        avisos.push(format!(
            "El pago registrado ({}) no coincide con el total del mes ({total}).",
            p.importe
        ));
    }
    Ok(ResumenSeguros {
        year,
        month,
        conceptos,
        trabajadores,
        total_trabajador,
        total_empresa,
        total,
        vence_nominal,
        vence,
        devengo,
        pago,
        avisos,
    })
}

/// Pago de los seguros sociales de un mes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PagoSegurosInput {
    pub year: i32,
    pub month: u8,
    pub fecha: Date,
    /// Importe cargado por la TGSS (el del RLC).
    pub importe: Money,
    /// Cuenta de tesorería (572…).
    pub cuenta: String,
}

/// Asiento del pago: 476 al debe y tesorería al haber.
pub fn asiento_pago(p: &PagoSegurosInput) -> Result<JournalEntry> {
    let cuenta = AccountCode::new(p.cuenta.trim())
        .map_err(|e| other(format!("cuenta de tesorería: {e}")))?;
    if !p.cuenta.trim().starts_with("57") {
        return Err(other("la cuenta de tesorería es una 57 (572 bancos…)"));
    }
    Ok(JournalEntry {
        date: p.fecha,
        concept: format!("Seguros sociales de {} de {}", nombre_mes(p.month), p.year),
        origin: Origin::Nomina,
        lines: [
            EntryLine::signed(acc("476"), p.importe, true),
            EntryLine::signed(cuenta, p.importe, false),
        ]
        .into_iter()
        .flatten()
        .collect(),
    })
}

/// Registra el pago de los seguros sociales del mes con su asiento. Devuelve el id del asiento.
pub fn pagar_seguros(
    v: &Vault,
    p: &PagoSegurosInput,
    confirmed: bool,
    now: OffsetDateTime,
) -> Result<i64> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    if p.importe.is_negative() || p.importe.is_zero() {
        return Err(other("el importe tiene que ser mayor que cero"));
    }
    let (inicio, _) = rango_mes(p.year, p.month)?;
    if p.fecha < inicio {
        return Err(other(
            "la fecha del pago no puede ser anterior al mes que liquida",
        ));
    }
    let per = periodo(p.year, p.month);
    let conn = v.connection();
    let ya: Option<Option<i64>> = conn
        .query_row(
            "SELECT entry_id FROM seguros_sociales_pago WHERE periodo = ?1",
            [&per],
            |r| r.get(0),
        )
        .optional()?;
    if matches!(ya, Some(Some(_))) {
        return Err(other(
            "el pago de ese mes ya está registrado: borra su asiento en el diario si quieres \
             rehacerlo",
        ));
    }
    let entry = asiento_pago(p)?;
    let tx = conn.unchecked_transaction()?;
    let entry_id = accounting::record_entry(&tx, &entry, now).map_err(|e| {
        if e.to_string().contains("cerrado") {
            other("el ejercicio de ese pago está cerrado: reábrelo en Cierre")
        } else {
            e
        }
    })?;
    tx.execute(
        "INSERT INTO seguros_sociales_pago(periodo, fecha, importe, cuenta, entry_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(periodo) DO UPDATE SET fecha = excluded.fecha, importe = excluded.importe,
           cuenta = excluded.cuenta, entry_id = excluded.entry_id,
           created_at = excluded.created_at",
        params![
            per,
            p.fecha.to_string(),
            p.importe.cents(),
            p.cuenta.trim(),
            entry_id,
            now_str(now)
        ],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "seguros_sociales.pago",
        &per,
        &serde_json::json!({ "entry": entry_id, "importe": p.importe.cents() }),
    )?;
    tx.commit()?;
    Ok(entry_id)
}

/// Finiquito básico (sin guardar nada).
pub fn calcular_finiquito(d: &DatosFiniquito) -> Result<Finiquito> {
    finiquito::calcular(d).map_err(other)
}
