//! Borrador de la memoria de las cuentas anuales en el modelo elegido: PYMES (PGC de PYMES, RD
//! 1515/2007, tercera parte), abreviado o normal (PGC, RD 1514/2007, tercera parte). Cada modelo
//! tiene sus notas (ver [`orden`]) y se rellenan con la contabilidad: identificación, aplicación
//! del resultado, movimientos del inmovilizado, activos y pasivos financieros por categoría con
//! los vencimientos de los préstamos, fondos propios con sus movimientos (los del ECPN), situación
//! fiscal (del borrador del 200), ingresos y gastos, partes vinculadas, plantilla media,
//! honorarios de auditoría, hechos posteriores y periodo medio de pago a proveedores.
//!
//! Lo que no se puede saber va entre «[…]». Lo que se teclea (hechos posteriores, honorarios del
//! auditor, fecha de formulación, firmantes y una nota fija de regímenes especiales que se repite
//! cada año, como la del canje de valores del art. 86 LIS) se guarda en `settings` (clave
//! `memoria`). Se guarda en RTF (se edita en Pages o Word) o en PDF. Los administradores la
//! formulan y la junta la aprueba; se adjunta al depósito de cuentas.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::entry::Origin;
use taxpain_core::accounting::fiscal_year::FiscalYear;
use taxpain_core::accounting::prestamo::Sentido;
use taxpain_core::accounting::statements::ModelKind;
use taxpain_core::money::Money;
use taxpain_core::nif::Nif;
use taxpain_invoicing::pdf::{self, Para};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use time::{Date, OffsetDateTime};

use crate::accounting::{self, StoredEntry};
use crate::error::{AppError, Result};

/// Saldo inicial (apertura) y movimientos del ejercicio de unas cuentas, sin el cierre.
#[derive(Clone, Copy, Default)]
struct Mov {
    opening: Money,
    debit: Money,
    credit: Money,
}

impl Mov {
    /// Saldo deudor (positivo) al cierre, antes del asiento de cierre.
    fn balance(self) -> Money {
        self.opening + self.debit - self.credit
    }
}

fn movements(entries: &[StoredEntry], prefixes: &[&str]) -> Mov {
    let mut m = Mov::default();
    for e in entries {
        if matches!(e.entry.origin, Origin::Cierre) {
            continue;
        }
        let opening = matches!(e.entry.origin, Origin::Apertura);
        for l in &e.entry.lines {
            if !prefixes.iter().any(|p| l.account.starts_with(p)) {
                continue;
            }
            if opening {
                m.opening += l.debit - l.credit;
            } else {
                m.debit += l.debit;
                m.credit += l.credit;
            }
        }
    }
    m
}

/// Saldo de cuentas de gastos e ingresos del ejercicio (sin regularización ni cierre).
fn pnl(entries: &[StoredEntry], prefixes: &[&str]) -> Money {
    let mut total = Money::ZERO;
    for e in entries {
        if matches!(e.entry.origin, Origin::Regularizacion | Origin::Cierre) {
            continue;
        }
        for l in &e.entry.lines {
            if prefixes.iter().any(|p| l.account.starts_with(p)) {
                total += l.debit - l.credit;
            }
        }
    }
    total
}

fn row(label: &str, m: Money) -> Para {
    Para::Row(label.to_owned(), m.to_string())
}

fn legal_form(nif: Option<&str>) -> &'static str {
    match nif.and_then(|n| n.chars().next()) {
        Some('B') => "sociedad de responsabilidad limitada",
        Some('A') => "sociedad anónima",
        Some('F') => "sociedad cooperativa",
        _ => "[forma jurídica]",
    }
}

fn dmy(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

// ---------------- Lo que se teclea ----------------

/// Clave de `settings` con los textos de la memoria que no salen de la contabilidad.
pub const TEXTOS_KEY: &str = "memoria";

/// Textos de un ejercicio.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextosEjercicio {
    /// Hechos posteriores al cierre (vacío: se deja el corchete).
    #[serde(default)]
    pub hechos_posteriores: String,
    /// Honorarios del auditor del ejercicio (`Some(0)`: no se audita; `None`: corchete).
    #[serde(default)]
    pub honorarios_auditoria: Option<Money>,
    /// Fecha de formulación de las cuentas.
    #[serde(default)]
    pub formulacion: Option<Date>,
    /// Administradores que firman (hoja de firmas).
    #[serde(default)]
    pub firmantes: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Textos {
    /// Nota fija de regímenes especiales que se repite cada año (p. ej. la del art. 86 LIS
    /// sobre el canje de valores).
    #[serde(default)]
    nota_fija: String,
    #[serde(default)]
    ejercicios: BTreeMap<String, TextosEjercicio>,
}

/// Textos de la memoria de `year`: la nota fija (de todos los años) y los del ejercicio.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextosMemoria {
    #[serde(default)]
    pub nota_fija: String,
    #[serde(flatten)]
    pub ejercicio: TextosEjercicio,
}

pub fn textos(v: &Vault, year: i32) -> Result<TextosMemoria> {
    let t: Textos = v.setting(TEXTOS_KEY)?.unwrap_or_default();
    Ok(TextosMemoria {
        ejercicio: t
            .ejercicios
            .get(&year.to_string())
            .cloned()
            .unwrap_or_default(),
        nota_fija: t.nota_fija,
    })
}

/// Guarda los textos de la memoria de `year` (queda en la auditoría).
pub fn guardar_textos(
    v: &Vault,
    year: i32,
    nuevos: &TextosMemoria,
    now: OffsetDateTime,
) -> Result<()> {
    if nuevos
        .ejercicio
        .honorarios_auditoria
        .is_some_and(|m| m.cents() < 0)
    {
        return Err(AppError::Other(
            "los honorarios de auditoría no pueden ser negativos".into(),
        ));
    }
    let mut t: Textos = v.setting(TEXTOS_KEY)?.unwrap_or_default();
    t.nota_fija = nuevos.nota_fija.trim().to_owned();
    let mut e = nuevos.ejercicio.clone();
    e.hechos_posteriores = e.hechos_posteriores.trim().to_owned();
    e.firmantes = e
        .firmantes
        .iter()
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty())
        .collect();
    t.ejercicios.insert(year.to_string(), e);
    v.set_setting(TEXTOS_KEY, &t)?;
    audit::append(
        v.connection(),
        &crate::invoicing::now_str(now),
        Actor::User,
        "memoria.textos",
        &year.to_string(),
        &serde_json::json!({}),
    )?;
    Ok(())
}

// ---------------- Notas ----------------

/// Notas de la memoria. Cada modelo las ordena y numera a su manera.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Nota {
    Actividad,
    Bases,
    Aplicacion,
    Normas,
    /// Material, intangible e inversiones inmobiliarias juntos (PYMES y abreviado).
    Inmovilizado,
    InmMaterial,
    InvInmobiliarias,
    InmIntangible,
    Arrendamientos,
    ActivosFin,
    PasivosFin,
    /// Instrumentos financieros (normal): activos, pasivos y fondos propios.
    InstrumentosFin,
    FondosPropios,
    Existencias,
    MonedaExtranjera,
    Fiscal,
    IngresosGastos,
    Provisiones,
    MedioAmbiente,
    RetribucionesLp,
    PagosInstrumentos,
    Subvenciones,
    Combinaciones,
    NegociosConjuntos,
    ActivosVenta,
    HechosPosteriores,
    Vinculadas,
    OtraInfo,
    Segmentada,
    Pmp,
}

/// Orden de las notas en cada modelo. PYMES: PGC de PYMES (RD 1515/2007, 3.ª parte); abreviado
/// y normal: PGC (RD 1514/2007, 3.ª parte). TODO(VERIFY): la numeración exacta de la memoria
/// abreviada tras el RD 602/2016 y el lugar de la nota del periodo medio de pago.
fn orden(kind: ModelKind) -> &'static [Nota] {
    use Nota::*;
    match kind {
        ModelKind::Pymes => &[
            Actividad,
            Bases,
            Aplicacion,
            Normas,
            Inmovilizado,
            ActivosFin,
            PasivosFin,
            FondosPropios,
            Fiscal,
            IngresosGastos,
            Subvenciones,
            Vinculadas,
            OtraInfo,
            Pmp,
        ],
        ModelKind::Abreviado => &[
            Actividad,
            Bases,
            Aplicacion,
            Normas,
            Inmovilizado,
            ActivosFin,
            PasivosFin,
            FondosPropios,
            Fiscal,
            IngresosGastos,
            Subvenciones,
            HechosPosteriores,
            Vinculadas,
            OtraInfo,
            Pmp,
        ],
        ModelKind::Normal => &[
            Actividad,
            Bases,
            Aplicacion,
            Normas,
            InmMaterial,
            InvInmobiliarias,
            InmIntangible,
            Arrendamientos,
            InstrumentosFin,
            Existencias,
            MonedaExtranjera,
            Fiscal,
            IngresosGastos,
            Provisiones,
            MedioAmbiente,
            RetribucionesLp,
            PagosInstrumentos,
            Subvenciones,
            Combinaciones,
            NegociosConjuntos,
            ActivosVenta,
            HechosPosteriores,
            Vinculadas,
            OtraInfo,
            Segmentada,
            Pmp,
        ],
    }
}

impl Nota {
    fn titulo(self) -> &'static str {
        match self {
            Nota::Actividad => "Actividad de la empresa",
            Nota::Bases => "Bases de presentación de las cuentas anuales",
            Nota::Aplicacion => "Aplicación de resultados",
            Nota::Normas => "Normas de registro y valoración",
            Nota::Inmovilizado => "Inmovilizado material, intangible e inversiones inmobiliarias",
            Nota::InmMaterial => "Inmovilizado material",
            Nota::InvInmobiliarias => "Inversiones inmobiliarias",
            Nota::InmIntangible => "Inmovilizado intangible",
            Nota::Arrendamientos => "Arrendamientos y otras operaciones de naturaleza similar",
            Nota::ActivosFin => "Activos financieros",
            Nota::PasivosFin => "Pasivos financieros",
            Nota::InstrumentosFin => "Instrumentos financieros",
            Nota::FondosPropios => "Fondos propios",
            Nota::Existencias => "Existencias",
            Nota::MonedaExtranjera => "Moneda extranjera",
            Nota::Fiscal => "Situación fiscal",
            Nota::IngresosGastos => "Ingresos y gastos",
            Nota::Provisiones => "Provisiones y contingencias",
            Nota::MedioAmbiente => "Información sobre medio ambiente",
            Nota::RetribucionesLp => "Retribuciones a largo plazo al personal",
            Nota::PagosInstrumentos => {
                "Transacciones con pagos basados en instrumentos de patrimonio"
            }
            Nota::Subvenciones => "Subvenciones, donaciones y legados",
            Nota::Combinaciones => "Combinaciones de negocios",
            Nota::NegociosConjuntos => "Negocios conjuntos",
            Nota::ActivosVenta => {
                "Activos no corrientes mantenidos para la venta y operaciones interrumpidas"
            }
            Nota::HechosPosteriores => "Hechos posteriores al cierre",
            Nota::Vinculadas => "Operaciones con partes vinculadas",
            Nota::OtraInfo => "Otra información",
            Nota::Segmentada => "Información segmentada",
            Nota::Pmp => "Información sobre el periodo medio de pago a proveedores",
        }
    }
}

/// Lo que comparten las notas.
struct Ctx<'a> {
    v: &'a Vault,
    year: i32,
    kind: ModelKind,
    fy: FiscalYear,
    entries: Vec<StoredEntry>,
    name: String,
    result: Money,
    textos: TextosMemoria,
}

impl Ctx<'_> {
    fn mov(&self, prefixes: &[&str]) -> Mov {
        movements(&self.entries, prefixes)
    }

    /// Saldo acreedor (positivo) al cierre.
    fn credit_balance(&self, prefixes: &[&str]) -> Money {
        -self.mov(prefixes).balance()
    }

    fn pnl(&self, prefixes: &[&str]) -> Money {
        pnl(&self.entries, prefixes)
    }

    /// Número de una nota en este modelo («nota 12»).
    fn numero(&self, n: Nota) -> Option<usize> {
        orden(self.kind).iter().position(|x| *x == n).map(|i| i + 1)
    }

    fn nombre_pgc(&self) -> &'static str {
        match self.kind {
            ModelKind::Pymes => {
                "Plan General de Contabilidad de Pequeñas y Medianas Empresas (Real Decreto \
                 1515/2007)"
            }
            _ => "Plan General de Contabilidad (Real Decreto 1514/2007)",
        }
    }
}

/// Título del documento según el modelo.
fn titulo(kind: ModelKind, year: i32) -> String {
    match kind {
        ModelKind::Pymes => format!("Memoria del ejercicio {year} (modelo de PYMES)"),
        ModelKind::Abreviado => format!("Memoria abreviada del ejercicio {year}"),
        ModelKind::Normal => format!("Memoria del ejercicio {year} (modelo normal)"),
    }
}

/// Párrafos de la memoria de `year` con el modelo de cuentas elegido en los ajustes.
pub fn paragraphs(v: &Vault, year: i32) -> Result<Vec<Para>> {
    let kind = crate::models::settings(v)?.accounts_model;
    paragraphs_modelo(v, year, kind)
}

/// Párrafos de la memoria de `year` con el modelo `kind` (PYMES, abreviado o normal).
pub fn paragraphs_modelo(v: &Vault, year: i32, kind: ModelKind) -> Result<Vec<Para>> {
    let info = crate::companies::info(v)?;
    if info
        .nif
        .as_deref()
        .and_then(|n| Nif::parse(n).ok())
        .is_some_and(|n| n.is_persona_fisica())
    {
        return Err(AppError::Other(
            "la memoria de las cuentas anuales es de las sociedades".into(),
        ));
    }
    let fy = accounting::fiscal_year(v, year)?;
    let entries = accounting::journal(v, year)?;
    let result = -pnl(&entries, &["6", "7"]);
    let c = Ctx {
        v,
        year,
        kind,
        fy,
        entries,
        name: info.name.clone(),
        result,
        textos: textos(v, year)?,
    };
    let nif = info.nif.clone().unwrap_or_else(|| "[NIF]".into());
    let mut p = vec![
        Para::Title(titulo(kind, year)),
        Para::Text(format!(
            "{} · NIF {nif} · Ejercicio del {} al {}",
            c.name,
            dmy(fy.start),
            dmy(fy.end)
        )),
        Para::Text(
            "BORRADOR preparado con TaxPain a partir de la contabilidad: revísalo y completa lo \
             que va entre corchetes antes de formular las cuentas."
                .into(),
        ),
        Para::Gap,
    ];
    for (i, n) in orden(kind).iter().enumerate() {
        p.push(Para::Bold(format!("{}. {}", i + 1, n.titulo())));
        nota(&c, *n, &mut p, info.nif.as_deref())?;
        p.push(Para::Gap);
    }
    firmas(&c, &mut p);
    Ok(p)
}

fn nota(c: &Ctx, n: Nota, p: &mut Vec<Para>, nif: Option<&str>) -> Result<()> {
    match n {
        Nota::Actividad => actividad(c, p, nif)?,
        Nota::Bases => bases(c, p),
        Nota::Aplicacion => aplicacion(c, p)?,
        Nota::Normas => normas(c, p),
        Nota::Inmovilizado => inmovilizado(c, p, &INMOVILIZADO),
        Nota::InmMaterial => inmovilizado(c, p, &INMOVILIZADO[1..2]),
        Nota::InvInmobiliarias => inmovilizado(c, p, &INMOVILIZADO[2..3]),
        Nota::InmIntangible => inmovilizado(c, p, &INMOVILIZADO[..1]),
        Nota::Arrendamientos => arrendamientos(c, p),
        Nota::ActivosFin => activos_financieros(c, p)?,
        Nota::PasivosFin => pasivos_financieros(c, p)?,
        Nota::InstrumentosFin => {
            p.push(Para::Text("a) Activos financieros".into()));
            activos_financieros(c, p)?;
            p.push(Para::Text("b) Pasivos financieros".into()));
            pasivos_financieros(c, p)?;
            p.push(Para::Text("c) Fondos propios".into()));
            fondos_propios(c, p)?;
        }
        Nota::FondosPropios => fondos_propios(c, p)?,
        Nota::Existencias => existencias(c, p),
        Nota::MonedaExtranjera => p.push(Para::Text(
            "Las operaciones en moneda extranjera se convierten al tipo de cambio de la fecha de \
             la operación y las partidas monetarias, al del cierre; las diferencias de cambio van \
             a resultados (cuentas 668 y 768)."
                .into(),
        )),
        Nota::Fiscal => fiscal(c, p),
        Nota::IngresosGastos => ingresos_gastos(c, p),
        Nota::Provisiones => provisiones(c, p),
        Nota::MedioAmbiente => p.push(Para::Text(
            "La sociedad no tiene activos ni ha incurrido en gastos de naturaleza \
             medioambiental, ni tiene provisiones ni contingencias por este concepto \
             [compruébalo]."
                .into(),
        )),
        Nota::RetribucionesLp => p.push(Para::Text(
            "La sociedad no tiene compromisos de retribuciones a largo plazo con el personal \
             (planes de pensiones, premios de jubilación) [compruébalo]."
                .into(),
        )),
        Nota::PagosInstrumentos => p.push(Para::Text(
            "La sociedad no tiene transacciones con pagos basados en instrumentos de patrimonio \
             [compruébalo]."
                .into(),
        )),
        Nota::Subvenciones => subvenciones(c, p),
        Nota::Combinaciones => p.push(Para::Text(
            "No ha habido combinaciones de negocios en el ejercicio [compruébalo].".into(),
        )),
        Nota::NegociosConjuntos => p.push(Para::Text(
            "La sociedad no participa en negocios conjuntos [compruébalo].".into(),
        )),
        Nota::ActivosVenta => {
            let m = c.mov(&["58"]).balance();
            p.push(if m.is_zero() {
                Para::Text(
                    "La sociedad no tiene activos no corrientes mantenidos para la venta ni \
                     operaciones interrumpidas."
                        .into(),
                )
            } else {
                row(
                    "Activos no corrientes mantenidos para la venta (58) [detalla]",
                    m,
                )
            });
        }
        Nota::HechosPosteriores => hechos_posteriores(c, p),
        Nota::Vinculadas => vinculadas(c, p)?,
        Nota::OtraInfo => otra_info(c, p)?,
        Nota::Segmentada => p.push(Para::Text(
            "Distribución del importe neto de la cifra de negocios por categorías de \
             actividades y por mercados geográficos: [detalla, salvo que pueda formular la \
             cuenta de pérdidas y ganancias abreviada]."
                .into(),
        )),
        Nota::Pmp => pmp(c, p),
    }
    Ok(())
}

fn actividad(c: &Ctx, p: &mut Vec<Para>, nif: Option<&str>) -> Result<()> {
    let inv = crate::invoicing::settings(c.v)?;
    let ms = crate::models::settings(c.v)?;
    let address = inv
        .as_ref()
        .and_then(|s| s.issuer.address.as_ref())
        .map(|a| a.lines().join(", "))
        .unwrap_or_else(|| "[domicilio social]".into());
    let activity = if ms.activity_description.trim().is_empty() {
        "[actividad]".to_owned()
    } else {
        ms.activity_description.trim().to_owned()
    };
    p.push(Para::Text(format!(
        "{} es una {} con domicilio social en {address}. Su actividad es {activity}{}. La \
         moneda funcional es el euro.",
        c.name,
        legal_form(nif),
        if ms.cnae.trim().is_empty() {
            String::new()
        } else {
            format!(" (CNAE {})", ms.cnae.trim())
        }
    )));
    Ok(())
}

fn bases(c: &Ctx, p: &mut Vec<Para>) {
    let documentos = match c.kind {
        ModelKind::Pymes => {
            "el balance, la cuenta de pérdidas y ganancias y esta memoria (el estado de cambios \
             en el patrimonio neto es voluntario)"
        }
        ModelKind::Abreviado => {
            "el balance, la cuenta de pérdidas y ganancias, el estado de cambios en el \
             patrimonio neto y esta memoria, todos abreviados (arts. 257 a 261 de la Ley de \
             Sociedades de Capital); no se formula el estado de flujos de efectivo"
        }
        ModelKind::Normal => {
            "el balance, la cuenta de pérdidas y ganancias, el estado de cambios en el \
             patrimonio neto, el estado de flujos de efectivo y esta memoria"
        }
    };
    p.push(Para::Text(format!(
        "Las cuentas anuales comprenden {documentos}. Se han preparado a partir de los registros \
         contables de la sociedad y se presentan de acuerdo con la legislación mercantil vigente \
         y con el {}, de forma que muestran la imagen fiel del patrimonio, de la situación \
         financiera y de los resultados. No ha habido razones excepcionales para no aplicar \
         disposiciones legales en materia contable, ni cambios de criterio ni corrección de \
         errores [compruébalo].",
        c.nombre_pgc()
    )));
    let anterior = accounting::journal(c.v, c.year - 1).is_ok_and(|e| !e.is_empty());
    p.push(Para::Text(if anterior {
        format!(
            "Comparación de la información: las cifras de {} se presentan en el balance y en la \
             cuenta de pérdidas y ganancias a efectos comparativos.",
            c.year - 1
        )
    } else {
        "Comparación de la información: no hay cifras del ejercicio anterior en TaxPain \
         [añádelas si la sociedad existía, o indica que es el primer ejercicio]."
            .into()
    }));
    if c.kind == ModelKind::Pymes
        && crate::modelo_cuentas::aviso(c.v, c.year).is_ok_and(|a| a.puede_micro == Some(true))
    {
        p.push(Para::Text(
            "[Si aplicas los criterios específicos de microempresas (art. 4 del RD 1515/2007), \
             dilo aquí.]"
                .into(),
        ));
    }
}

fn aplicacion(c: &Ctx, p: &mut Vec<Para>) -> Result<()> {
    p.push(row(
        "Base de reparto: pérdidas y ganancias del ejercicio",
        c.result,
    ));
    match applied_result(c.v, c.year)? {
        Some(lines) => {
            p.push(Para::Text("Aplicación aprobada por la junta:".into()));
            for (label, m) in lines {
                p.push(row(&label, m));
            }
        }
        None if c.result.cents() > 0 => {
            if let Ok(a) = crate::cierre::allocation(c.v, c.year) {
                p.push(Para::Text("Propuesta de aplicación:".into()));
                for (label, m) in [
                    ("A reserva legal", a.suggested.legal),
                    (
                        "A compensar pérdidas de ejercicios anteriores",
                        a.suggested.losses,
                    ),
                    (
                        "A reserva de capitalización (art. 25 LIS)",
                        a.suggested.capitalization,
                    ),
                    ("A reservas voluntarias", a.suggested.voluntary),
                    ("A dividendos", a.suggested.dividends),
                    ("A remanente", a.suggested.remanente),
                ] {
                    if m.cents() > 0 {
                        p.push(row(label, m));
                    }
                }
            } else {
                p.push(Para::Text(
                    "[Propuesta de aplicación: cierra el ejercicio en TaxPain para calcularla con \
                     la reserva legal mínima.]"
                        .into(),
                ));
            }
        }
        None => {
            p.push(Para::Text(
                "Propuesta de aplicación: a resultados negativos de ejercicios anteriores.".into(),
            ));
        }
    }
    let a_cuenta = c.mov(&["557"]).balance();
    if !a_cuenta.is_zero() {
        p.push(row(
            "Dividendos a cuenta repartidos en el ejercicio (con el estado contable de liquidez \
             del art. 277 LSC)",
            a_cuenta,
        ));
    }
    Ok(())
}

fn normas(c: &Ctx, p: &mut Vec<Para>) {
    let mut t = vec![
        "Inmovilizado intangible y material: se valoran por su precio de adquisición o coste de \
         producción, menos la amortización acumulada y, en su caso, las pérdidas por deterioro. \
         Se amortizan linealmente durante su vida útil [revisa los coeficientes]."
            .to_owned(),
        "Instrumentos financieros: los créditos y débitos por operaciones comerciales con \
         vencimiento inferior a un año se valoran por su valor nominal; los préstamos, por su \
         coste amortizado con el tipo de interés efectivo."
            .to_owned(),
        "Impuesto sobre beneficios: el gasto se calcula sobre el resultado del ejercicio \
         corregido por las diferencias con la base imponible, menos las deducciones y \
         bonificaciones."
            .to_owned(),
        "Ingresos y gastos: se imputan según el devengo, con independencia del momento del cobro \
         o del pago. Los ingresos por prestación de servicios se reconocen cuando el servicio se \
         ha prestado."
            .to_owned(),
    ];
    if !c.mov(&["3"]).balance().is_zero() {
        t.push(
            "Existencias: se valoran por su precio de adquisición o coste de producción, con el \
             método del precio medio ponderado [compruébalo]."
                .into(),
        );
    }
    if c.kind == ModelKind::Normal {
        t.push(
            "Provisiones y contingencias: se reconocen las obligaciones presentes con salida \
             probable de recursos, por el valor actual de la mejor estimación."
                .into(),
        );
    }
    for x in t {
        p.push(Para::Text(x));
    }
}

/// Partidas del inmovilizado: nombre, cuentas de coste y de amortización acumulada.
const INMOVILIZADO: [(&str, &[&str], &[&str]); 3] = [
    ("Inmovilizado intangible", &["20"], &["280"]),
    ("Inmovilizado material", &["21", "23"], &["281"]),
    ("Inversiones inmobiliarias", &["22"], &["282"]),
];

fn inmovilizado(c: &Ctx, p: &mut Vec<Para>, partidas: &[(&str, &[&str], &[&str])]) {
    let mut any = false;
    for (label, cost, dep) in partidas {
        let m = c.mov(cost);
        let d = c.mov(dep);
        if m.opening.is_zero() && m.debit.is_zero() && m.credit.is_zero() {
            continue;
        }
        any = true;
        if partidas.len() > 1 {
            p.push(Para::Text((*label).into()));
        }
        p.push(row("  Coste: saldo inicial", m.opening));
        p.push(row("  Entradas", m.debit));
        p.push(row("  Salidas", m.credit));
        p.push(row("  Saldo final", m.balance()));
        p.push(row("  Amortización acumulada inicial", -d.opening));
        // Dotación: lo abonado a la 28x; las bajas (cargos) van aparte.
        p.push(row("  Dotación del ejercicio", d.credit));
        if !d.debit.is_zero() {
            p.push(row("  Bajas de amortización acumulada", d.debit));
        }
        p.push(row("  Amortización acumulada final", -d.balance()));
        p.push(row(
            "  Valor neto contable al cierre",
            m.balance() + d.balance(),
        ));
    }
    if !any {
        let que = if partidas.len() > 1 {
            "inmovilizado material, intangible ni inversiones inmobiliarias".to_owned()
        } else {
            partidas[0].0.to_lowercase()
        };
        p.push(Para::Text(format!(
            "La sociedad no tiene {que} [compruébalo]."
        )));
    } else {
        let fully: Money = c.pnl(&["68"]);
        if !fully.is_zero() {
            p.push(row(
                "Gasto por amortización del ejercicio (cuenta de pérdidas y ganancias)",
                fully,
            ));
        }
    }
}

fn arrendamientos(c: &Ctx, p: &mut Vec<Para>) {
    let operativos = c.pnl(&["621"]);
    p.push(row(
        "Arrendamientos operativos: gasto del ejercicio (621)",
        operativos,
    ));
    let financieros = c.credit_balance(&["174", "524"]);
    if !financieros.is_zero() {
        p.push(row(
            "Arrendamientos financieros: deuda pendiente al cierre (174 y 524)",
            financieros,
        ));
    }
}

/// Vencimientos de las cuotas de capital de los préstamos de Contabilidad → Préstamos posteriores
/// al cierre, por años: 1, 2, 3, 4, 5 y más de cinco.
fn vencimientos(c: &Ctx, sentido: Sentido) -> Result<[Money; 6]> {
    let mut out = [Money::ZERO; 6];
    let end = c.fy.end;
    let limite = |k: i32| {
        end.replace_year(end.year() + k).unwrap_or_else(|_| {
            Date::from_calendar_date(end.year() + k, end.month(), 28).unwrap_or(end)
        })
    };
    for pv in crate::prestamos::list(c.v, end)? {
        if pv.prestamo.sentido != sentido {
            continue;
        }
        for q in &pv.cuotas {
            let f = q.q.fecha;
            if f <= end {
                continue;
            }
            let k = (1..=5)
                .find(|&k| f <= limite(k))
                .map_or(5, |k| k as usize - 1);
            out[k] += q.q.capital - q.q.capitalizados;
        }
    }
    Ok(out)
}

fn tabla_vencimientos(c: &Ctx, p: &mut Vec<Para>, v: [Money; 6]) {
    for (i, m) in v.iter().enumerate() {
        if m.is_zero() {
            continue;
        }
        let label = if i < 5 {
            format!("  Vence en {}", c.year + 1 + i as i32)
        } else {
            "  Vence en años posteriores".to_owned()
        };
        p.push(row(&label, *m));
    }
}

fn activos_financieros(c: &Ctx, p: &mut Vec<Para>) -> Result<()> {
    p.push(Para::Text(
        "Activos financieros a coste amortizado (créditos, clientes y otras cuentas a cobrar):"
            .into(),
    ));
    let lp = c.mov(&["25", "26"]).balance() - c.mov(&["250"]).balance();
    if !lp.is_zero() {
        p.push(row("  A largo plazo: créditos y otros (25, 26)", lp));
    }
    p.push(row(
        "  A corto plazo: clientes por ventas y prestaciones de servicios",
        c.mov(&["430", "431", "432", "433", "434", "435", "436", "490"])
            .balance(),
    ));
    // Los saldos con las Administraciones Públicas no son instrumentos financieros: van en la
    // situación fiscal.
    p.push(row(
        "  A corto plazo: otros deudores",
        c.mov(&["44", "460"]).balance(),
    ));
    let cp = c.mov(&["53", "54", "55", "56"]).balance()
        - c.mov(&["540", "530", "551", "552", "553", "555", "556", "557"])
            .balance();
    if !cp.is_zero() {
        p.push(row("  A corto plazo: créditos y otros", cp));
    }
    let socios = c.mov(&["551", "552", "553"]).balance();
    if socios.cents() > 0 {
        p.push(row(
            "  A corto plazo: cuentas corrientes con socios y administradores",
            socios,
        ));
    }
    let coste = c.mov(&["240", "241", "242", "293"]).balance();
    if !coste.is_zero() {
        p.push(row(
            "Activos financieros a coste: instrumentos de patrimonio de empresas del grupo, \
             multigrupo y asociadas (240 a 242)",
            coste,
        ));
    }
    let vr = c.mov(&["250", "540", "530"]).balance();
    if !vr.is_zero() {
        p.push(row(
            "Activos financieros a valor razonable con cambios en pérdidas y ganancias \
             [compruébalo] (250, 530, 540)",
            vr,
        ));
    }
    p.push(row(
        "Efectivo y otros activos líquidos equivalentes",
        c.mov(&["57"]).balance(),
    ));
    let v = vencimientos(c, Sentido::Concedido)?;
    if v.iter().any(|m| !m.is_zero()) {
        p.push(Para::Text(
            "Vencimientos de los préstamos concedidos (Contabilidad → Préstamos):".into(),
        ));
        tabla_vencimientos(c, p, v);
    }
    // Empresas del grupo y participadas: el detalle va en la nota de vinculadas.
    if c.kind != ModelKind::Pymes && !coste.is_zero() {
        p.push(Para::Text(
            "Empresas del grupo, multigrupo y asociadas: [denominación, domicilio, forma \
             jurídica, actividad, porcentaje de participación, capital, reservas y resultado de \
             cada una]."
                .into(),
        ));
    }
    Ok(())
}

fn pasivos_financieros(c: &Ctx, p: &mut Vec<Para>) -> Result<()> {
    p.push(Para::Text(
        "Pasivos financieros a coste amortizado, por clases:".into(),
    ));
    p.push(row(
        "Deudas a largo plazo con entidades de crédito",
        c.credit_balance(&["170"]),
    ));
    p.push(row(
        "Deudas a corto plazo con entidades de crédito",
        c.credit_balance(&["520"]),
    ));
    p.push(row(
        "Acreedores por arrendamiento financiero",
        c.credit_balance(&["174", "524"]),
    ));
    p.push(row(
        "Otras deudas",
        c.credit_balance(&["171", "173", "180", "521", "523", "526", "555", "465"]),
    ));
    let socios = c.credit_balance(&["551", "552", "553"]);
    if socios.cents() > 0 {
        p.push(row(
            "Cuentas corrientes con socios y administradores (acreedoras)",
            socios,
        ));
    }
    // Deudas con empresas del grupo, asociadas y otras partes vinculadas (163, 513, 514): sólo
    // si las hay; el detalle de los préstamos va en la nota de partes vinculadas.
    let vinculadas = c.credit_balance(&["163", "513", "514"]);
    if !vinculadas.is_zero() {
        let n = c.numero(Nota::Vinculadas).unwrap_or(0);
        p.push(row(
            &format!(
                "Deudas con empresas del grupo, asociadas y otras partes vinculadas (detalle en \
                 la nota {n})"
            ),
            vinculadas,
        ));
    }
    p.push(row(
        "Proveedores y acreedores comerciales",
        c.credit_balance(&["40", "41"]),
    ));
    let largo = c.credit_balance(&["16", "17", "18"]);
    let corto = c.credit_balance(&["40", "41", "465", "50", "51", "52", "555"]);
    p.push(row("Total a largo plazo", largo));
    p.push(row("Total a corto plazo", corto));
    let v = vencimientos(c, Sentido::Recibido)?;
    if v.iter().any(|m| !m.is_zero()) {
        p.push(Para::Text(
            "Vencimientos de los préstamos recibidos en cada uno de los cinco años siguientes y \
             después (cuadros de Contabilidad → Préstamos):"
                .into(),
        ));
        tabla_vencimientos(c, p, v);
    } else if !largo.is_zero() {
        p.push(Para::Text(
            "Vencimientos de las deudas a largo plazo en cada uno de los cinco años siguientes: \
             [detalla, o da de alta los préstamos en Contabilidad → Préstamos para calcularlos]."
                .into(),
        ));
    }
    p.push(Para::Text(
        "Deudas con garantía real: [indica si hay hipotecas u otras garantías, o «no hay»].".into(),
    ));
    Ok(())
}

fn fondos_propios(c: &Ctx, p: &mut Vec<Para>) -> Result<()> {
    p.push(row(
        "Capital",
        c.credit_balance(&["100", "101", "102", "103", "104"]),
    ));
    let prima = c.credit_balance(&["110"]);
    if !prima.is_zero() {
        p.push(row("Prima de emisión", prima));
    }
    p.push(row("Reserva legal", c.credit_balance(&["112"])));
    let capitalizacion = c.credit_balance(&["114"]);
    if !capitalizacion.is_zero() {
        p.push(row(
            "Reserva de capitalización (art. 25 LIS, indisponible cinco años)",
            capitalizacion,
        ));
    }
    p.push(row(
        "Otras reservas",
        c.credit_balance(&["113", "115", "119"]),
    ));
    let aportaciones = c.credit_balance(&["118"]);
    if !aportaciones.is_zero() {
        p.push(row("Otras aportaciones de socios", aportaciones));
    }
    p.push(row(
        "Remanente y resultados de ejercicios anteriores",
        c.credit_balance(&["120", "121"]),
    ));
    p.push(row("Resultado del ejercicio", c.result));
    let a_cuenta = c.credit_balance(&["557"]);
    if !a_cuenta.is_zero() {
        p.push(row("Dividendo a cuenta", a_cuenta));
    }
    // Movimientos del ejercicio (los del estado total de cambios en el patrimonio neto).
    let e = crate::ecpn::ecpn(c.v, c.year, c.kind)?;
    let cols = e.columnas_con_datos();
    if !cols.is_empty() {
        p.push(Para::Text("Movimientos del ejercicio:".into()));
        for col in cols {
            let ini = e
                .fila(crate::ecpn::Fila::SaldoInicial)
                .map_or(Money::ZERO, |f| f.importes[col]);
            let fin = e
                .fila(crate::ecpn::Fila::SaldoFinal)
                .map_or(Money::ZERO, |f| f.importes[col]);
            if ini == fin {
                continue;
            }
            p.push(row(&format!("  {} al inicio", e.columnas[col]), ini));
            p.push(row(&format!("  {} al cierre", e.columnas[col]), fin));
        }
    }
    // Participaciones del libro registro de socios.
    if let Ok(m) = crate::libros_societarios::registro::movements(c.v)
        && !m.is_empty()
        && let Ok(s) = crate::libros_societarios::registro::replay(&m, Some(c.fy.end))
        && s.count > 0
    {
        p.push(Para::Text(format!(
            "El capital social, de {}, está formado por {} participaciones o acciones (según el \
             libro registro de socios), totalmente suscritas y desembolsadas [compruébalo].",
            s.capital, s.count
        )));
        if s.holders.len() == 1 {
            p.push(Para::Text(format!(
                "La sociedad es unipersonal: su socio único es {} (art. 13 LSC).",
                s.holders[0].name
            )));
        } else {
            for h in s.holders.iter().filter(|h| h.pct >= 100_000) {
                p.push(Para::Text(format!(
                    "{} tiene el {},{:02} % del capital.",
                    h.name,
                    h.pct / 10_000,
                    h.pct % 10_000 / 100
                )));
            }
        }
    } else {
        p.push(Para::Text(
            "Número y valor nominal de las participaciones o acciones: [completa, o llévalo al \
             libro registro de socios de TaxPain]."
                .into(),
        ));
    }
    Ok(())
}

fn existencias(c: &Ctx, p: &mut Vec<Para>) {
    let s = c.mov(&["30", "31", "32", "33", "34", "35", "36", "39"]);
    if s.balance().is_zero() && s.opening.is_zero() {
        p.push(Para::Text("La sociedad no tiene existencias.".into()));
        return;
    }
    p.push(row("Existencias al inicio", s.opening));
    p.push(row("Existencias al cierre", s.balance()));
    p.push(row(
        "Variación de existencias en resultados (61 y 71)",
        -c.pnl(&["61", "71"]),
    ));
}

fn provisiones(c: &Ctx, p: &mut Vec<Para>) {
    let lp = c.credit_balance(&["14"]);
    let cp = c.credit_balance(&["499", "529"]);
    if lp.is_zero() && cp.is_zero() {
        p.push(Para::Text(
            "La sociedad no tiene provisiones. Contingencias: [no hay / describe las que haya]."
                .into(),
        ));
        return;
    }
    p.push(row("Provisiones a largo plazo (14)", lp));
    p.push(row("Provisiones a corto plazo (499, 529)", cp));
    p.push(Para::Text(
        "[Describe cada provisión y su movimiento.]".into(),
    ));
}

fn fiscal(c: &Ctx, p: &mut Vec<Para>) {
    match crate::models::draft(c.v, "200", c.year, 0) {
        Ok(d) => {
            let get = |b: u16| {
                d.boxes
                    .iter()
                    .find(|(k, _)| *k == b)
                    .map_or(Money::ZERO, |(_, m)| *m)
            };
            p.push(Para::Text(
                "Conciliación del resultado contable con la base imponible del Impuesto sobre \
                 Sociedades (del borrador del modelo 200):"
                    .into(),
            ));
            // [00501] ya suma el gasto por el impuesto: los aumentos y disminuciones son el resto.
            p.push(row("Resultado contable antes de impuestos", get(501)));
            p.push(row("Aumentos", get(417)));
            p.push(row("Disminuciones", get(418)));
            p.push(row("Reserva de capitalización (art. 25 LIS)", get(1032)));
            p.push(row("Compensación de bases imponibles negativas", get(547)));
            p.push(row("Base imponible", get(552)));
            p.push(row("Cuota íntegra", get(562)));
            p.push(row(
                "Retenciones y pagos a cuenta",
                get(1766) + get(601) + get(603) + get(605),
            ));
            p.push(row("Cuota diferencial (a ingresar o devolver)", get(611)));
        }
        Err(e) => p.push(Para::Text(format!(
            "[No se ha podido preparar el 200 de {}: {e}. Completa la conciliación.]",
            c.year
        ))),
    }
    let gasto = c.pnl(&["630", "633", "638"]);
    if !gasto.is_zero() {
        p.push(row(
            "Gasto por impuesto sobre beneficios en la cuenta de pérdidas y ganancias (63)",
            gasto,
        ));
    }
    p.push(Para::Text(
        "Saldos con las Administraciones Públicas al cierre:".into(),
    ));
    p.push(row(
        "Deudores (IVA, retenciones, pagos a cuenta, subvenciones)",
        c.mov(&["470", "471", "472", "473"]).balance(),
    ));
    p.push(row(
        "Acreedores (IVA, IRPF, IS, Seguridad Social)",
        c.credit_balance(&["475", "476", "477"]),
    ));
    let diferidos = c.mov(&["474"]).balance() - c.credit_balance(&["479"]);
    if !diferidos.is_zero() {
        p.push(row(
            "Activos menos pasivos por impuesto diferido (474 y 479)",
            diferidos,
        ));
    }
    p.push(Para::Text(
        "La sociedad tiene abiertos a inspección los ejercicios no prescritos de los impuestos \
         que le son aplicables [revisa si hay actas o contingencias]."
            .into(),
    ));
    let fija = c.textos.nota_fija.trim();
    if !fija.is_empty() {
        p.push(Para::Text(format!("Regímenes fiscales especiales: {fija}")));
    }
}

fn ingresos_gastos(c: &Ctx, p: &mut Vec<Para>) {
    p.push(row("Importe neto de la cifra de negocios", -c.pnl(&["70"])));
    if c.kind != ModelKind::Pymes {
        let ventas = -c.pnl(&["700", "701", "702", "703", "704"]);
        let servicios = -c.pnl(&["705"]);
        if !ventas.is_zero() && !servicios.is_zero() {
            p.push(row("  Ventas de bienes", ventas));
            p.push(row("  Prestaciones de servicios", servicios));
        }
    }
    p.push(row("Aprovisionamientos", c.pnl(&["60", "61"])));
    p.push(row("Sueldos y salarios", c.pnl(&["640", "641"])));
    p.push(row("Cargas sociales", c.pnl(&["642", "643", "649"])));
    if c.kind != ModelKind::Pymes {
        let ss = c.pnl(&["642"]);
        if !ss.is_zero() {
            p.push(row(
                "  de las que, Seguridad Social a cargo de la empresa",
                ss,
            ));
        }
    }
    p.push(row(
        "Otros gastos de explotación",
        c.pnl(&["62", "631", "634", "639", "65", "69"]),
    ));
    p.push(row("Amortización del inmovilizado", c.pnl(&["68"])));
    let fin_i = -c.pnl(&["76"]);
    let fin_g = c.pnl(&["66"]);
    if !fin_i.is_zero() {
        p.push(row("Ingresos financieros", fin_i));
    }
    if !fin_g.is_zero() {
        p.push(row("Gastos financieros", fin_g));
    }
    let otros = -c.pnl(&["678", "778"]);
    if !otros.is_zero() {
        p.push(row("Otros resultados (678 y 778)", otros));
    }
}

fn subvenciones(c: &Ctx, p: &mut Vec<Para>) {
    let patrimonio = c.credit_balance(&["130", "131", "132"]);
    let resultados = -c.pnl(&["74"]);
    // El registro de Contabilidad → Subvenciones (concedente, finalidad, condiciones).
    let registro = crate::subvenciones::memoria(c.v, c.year).unwrap_or_default();
    if patrimonio.is_zero() && resultados.is_zero() && registro.is_empty() {
        p.push(Para::Text(
            "La sociedad no ha recibido subvenciones, donaciones ni legados [compruébalo].".into(),
        ));
        return;
    }
    if !patrimonio.is_zero() {
        p.push(row(
            "Saldo en el patrimonio neto al cierre (13)",
            patrimonio,
        ));
    }
    if !resultados.is_zero() {
        p.push(row("Imputadas a resultados del ejercicio (74)", resultados));
    }
    let diferido = c.credit_balance(&["479"]);
    if !registro.is_empty() && !diferido.is_zero() {
        p.push(row(
            "Pasivos por diferencias temporarias imponibles (479)",
            diferido,
        ));
    }
    if registro.is_empty() {
        p.push(Para::Text(
            "[Indica el ente concedente, la finalidad y si se han cumplido las condiciones.]"
                .into(),
        ));
    } else {
        for t in registro {
            p.push(Para::Text(t));
        }
    }
}

fn hechos_posteriores(c: &Ctx, p: &mut Vec<Para>) {
    let t = c.textos.ejercicio.hechos_posteriores.trim();
    p.push(Para::Text(if t.is_empty() {
        "Hechos posteriores al cierre: [no se han producido hechos relevantes / describe los que \
         haya]."
            .into()
    } else if c.kind == ModelKind::Pymes {
        format!("Hechos posteriores al cierre: {t}")
    } else {
        t.to_owned()
    }));
}

fn vinculadas(c: &Ctx, p: &mut Vec<Para>) -> Result<()> {
    let dividends = c.mov(&["526"]).debit;
    p.push(row(
        "Dividendos pagados a los socios en el ejercicio",
        dividends,
    ));
    // Con el registro de operaciones vinculadas, la retribución y las operaciones por categoría;
    // sin él, los corchetes.
    match crate::vinculadas::nota_memoria(c.v, c.year) {
        Some(paras) => p.extend(paras),
        None => p.push(Para::Text(
            "Retribuciones del órgano de administración: [importe, o «el cargo de administrador \
             es gratuito» según los estatutos]. Otras operaciones con socios o administradores: \
             [describe préstamos, cuentas corrientes (551) o servicios, si los hay]."
                .into(),
        )),
    }
    // Los préstamos con vinculadas de Contabilidad → Préstamos, uno a uno.
    p.extend(crate::vinculadas::nota_prestamos(c.v, c.year));
    // Art. 16.2 LSC: referencia expresa e individualizada a los contratos con el socio único.
    let sole: Vec<_> = crate::libros_societarios::contratos::contracts(c.v)?
        .into_iter()
        .filter(|x| x.date >= c.fy.start && x.date <= c.fy.end)
        .collect();
    if !sole.is_empty() {
        p.push(Para::Text(
            "Contratos celebrados con el socio único en el ejercicio (art. 16.2 de la Ley de \
             Sociedades de Capital), transcritos en el libro registro de contratos:"
                .into(),
        ));
        for x in &sole {
            let date = dmy(x.date);
            match x.amount {
                Some(a) => p.push(row(&format!("{date} · {}", x.object), a)),
                None => p.push(Para::Text(format!("{date} · {}", x.object))),
            }
        }
    }
    let partners = c.mov(&["551", "552", "553"]).balance();
    if !partners.is_zero() {
        p.push(row(
            "Saldo de cuentas corrientes con socios y administradores",
            partners,
        ));
    }
    if c.kind != ModelKind::Pymes {
        p.push(Para::Text(
            "Los administradores no tienen situaciones de conflicto, directo o indirecto, con el \
             interés de la sociedad (art. 229 LSC) [compruébalo]."
                .into(),
        ));
    }
    Ok(())
}

fn otra_info(c: &Ctx, p: &mut Vec<Para>) -> Result<()> {
    match crate::modelo_cuentas::plantilla_media(c.v, c.fy.start, c.fy.end)? {
        Some(pl) => {
            p.push(Para::Text(format!(
                "Número medio de personas empleadas en el ejercicio: {} (según las nóminas \
                 registradas en TaxPain), de las que {} con contrato fijo y {} no fijas.",
                crate::modelo_cuentas::decimas(pl.decimas),
                crate::modelo_cuentas::decimas(pl.fijos_decimas()),
                crate::modelo_cuentas::decimas(pl.no_fijos_decimas)
            )));
            if c.kind == ModelKind::Normal {
                p.push(Para::Text(
                    "Distribución por categorías y por sexos al cierre, y personas con \
                     discapacidad igual o superior al 33 %: [completa]."
                        .into(),
                ));
            }
        }
        None => p.push(Para::Text(
            "Número medio de personas empleadas en el ejercicio: ninguna con nómina en TaxPain \
             [compruébalo]."
                .into(),
        )),
    }
    match c.textos.ejercicio.honorarios_auditoria {
        Some(m) if m.is_zero() => p.push(Para::Text(
            "Las cuentas anuales del ejercicio no se someten a auditoría.".into(),
        )),
        Some(m) => p.push(row(
            "Honorarios del auditor de cuentas por la auditoría del ejercicio",
            m,
        )),
        None if c.kind != ModelKind::Pymes => p.push(Para::Text(
            "Honorarios de auditoría: [si las cuentas se auditan, los honorarios del auditor por \
             la auditoría y por otros servicios; si no, quita esta frase]."
                .into(),
        )),
        None => {}
    }
    if c.kind == ModelKind::Normal {
        p.push(Para::Text(
            "Acuerdos que no figuran en el balance: [no hay / describe su naturaleza y efecto \
             financiero]."
                .into(),
        ));
    }
    if matches!(c.kind, ModelKind::Pymes) {
        hechos_posteriores(c, p);
    }
    Ok(())
}

fn pmp(c: &Ctx, p: &mut Vec<Para>) {
    let dias = |x: i64| format!("{},{:02} días", x / 100, (x % 100).abs());
    match crate::cartera::payment_period(c.v, c.fy.start, c.fy.end) {
        Ok(pp) => {
            p.push(Para::Text(format!(
                "Periodo medio de pago a proveedores en el ejercicio: {} (Ley 15/2010, \
                 disposición adicional tercera, y Resolución del ICAC de 29 de enero de 2016), \
                 calculado con las facturas recibidas y sus pagos en TaxPain.",
                dias(pp.average)
            )));
            if c.kind == ModelKind::Normal {
                p.push(Para::Row(
                    "Ratio de operaciones pagadas".into(),
                    dias(pp.paid_ratio),
                ));
                p.push(Para::Row(
                    "Ratio de operaciones pendientes de pago".into(),
                    dias(pp.pending_ratio),
                ));
                p.push(row("Total pagos realizados", pp.paid_total));
                p.push(row("Total pagos pendientes", pp.pending_total));
            }
        }
        Err(_) => p.push(Para::Text(
            "Periodo medio de pago a proveedores: [días].".into(),
        )),
    }
}

/// Fecha de formulación y hoja de firmas de los administradores (art. 253 LSC).
fn firmas(c: &Ctx, p: &mut Vec<Para>) {
    let t = &c.textos.ejercicio;
    let fecha = t.formulacion.map_or_else(|| "[fecha]".to_owned(), dmy);
    p.push(Para::Text(format!(
        "Las cuentas anuales del ejercicio {} han sido formuladas por el órgano de administración \
         de {} el {fecha} (art. 253 de la Ley de Sociedades de Capital). Firmado:",
        c.year, c.name
    )));
    if t.firmantes.is_empty() {
        p.push(Para::Text(
            "[administrador o administradores: nombre y firma de cada uno; si falta alguna, la \
             causa]"
                .into(),
        ));
    } else {
        for f in &t.firmantes {
            p.push(Para::Gap);
            p.push(Para::Gap);
            p.push(Para::Text(format!("Fdo.: {f}")));
        }
    }
}

/// Aplicación del resultado ya contabilizada (asiento `AplicacionResultado` del ejercicio).
pub(crate) fn applied_result(v: &Vault, year: i32) -> Result<Option<Vec<(String, Money)>>> {
    // La junta se celebra en el ejercicio siguiente, pero puede retrasarse: el asiento de
    // aplicación de este resultado se busca en los ejercicios posteriores.
    let Some(e) = (year + 1..=year + 3)
        .filter_map(|y| accounting::journal(v, y).ok())
        .flatten()
        .find(|e| matches!(e.entry.origin, Origin::AplicacionResultado(y) if y == i64::from(year)))
    else {
        return Ok(None);
    };
    let label = |acc: &str| match acc.get(..3) {
        Some("112") => "A reserva legal",
        Some("113") => "A reservas voluntarias",
        Some("114") => "A reserva de capitalización (art. 25 LIS)",
        Some("121") => "A resultados negativos de ejercicios anteriores",
        Some("526") => "A dividendos",
        Some("557") => "A dividendos (repartidos a cuenta)",
        Some("120") => "A remanente",
        _ => "Otras aplicaciones",
    };
    Ok(Some(
        e.entry
            .lines
            .iter()
            .filter(|l| !l.account.starts_with("129"))
            .map(|l| {
                (
                    label(l.account.as_str()).to_owned(),
                    if l.credit.is_zero() {
                        l.debit
                    } else {
                        l.credit
                    },
                )
            })
            .collect(),
    ))
}

/// PDF del borrador con el modelo elegido en los ajustes.
pub fn pdf(v: &Vault, year: i32) -> Result<Vec<u8>> {
    let kind = crate::models::settings(v)?.accounts_model;
    pdf_modelo(v, year, kind)
}

/// PDF del borrador con el modelo `kind`.
pub fn pdf_modelo(v: &Vault, year: i32, kind: ModelKind) -> Result<Vec<u8>> {
    let p = paragraphs_modelo(v, year, kind)?;
    pdf::text_document(&format!("Memoria {year}"), &[p]).map_err(AppError::Other)
}

/// Texto RTF (Pages, Word, TextEdit): con negritas, títulos e importes alineados a la derecha.
pub fn rtf(v: &Vault, year: i32) -> Result<String> {
    Ok(to_rtf(&paragraphs(v, year)?))
}

/// RTF con el modelo `kind`.
pub fn rtf_modelo(v: &Vault, year: i32, kind: ModelKind) -> Result<String> {
    Ok(to_rtf(&paragraphs_modelo(v, year, kind)?))
}

fn rtf_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            c if c.is_ascii() => out.push(c),
            // Unicode en RTF: \uN? con N en decimal con signo (16 bits).
            c => {
                let mut buf = [0u16; 2];
                for unit in c.encode_utf16(&mut buf) {
                    let signed = i16::from_ne_bytes(unit.to_ne_bytes());
                    out.push_str(&format!("\\u{}?", i32::from(signed)));
                }
            }
        }
    }
    out
}

/// Párrafos en RTF (también para otros documentos: el contrato de préstamo).
pub fn to_rtf(paras: &[Para]) -> String {
    let mut out = String::from(
        "{\\rtf1\\ansi\\ansicpg1252\\deff0{\\fonttbl{\\f0\\fswiss Helvetica;}}\\f0\\fs22\n",
    );
    for p in paras {
        match p {
            Para::Title(t) => out.push_str(&format!(
                "{{\\pard\\sa200\\b\\fs32 {}\\par}}\n",
                rtf_escape(t)
            )),
            Para::Bold(t) => out.push_str(&format!(
                "{{\\pard\\sb200\\sa100\\b\\fs24 {}\\par}}\n",
                rtf_escape(t)
            )),
            Para::Text(t) => out.push_str(&format!("{{\\pard\\sa100 {}\\par}}\n", rtf_escape(t))),
            Para::Row(l, r) => out.push_str(&format!(
                "{{\\pard\\tqr\\tx9000 {}\\tab {}\\par}}\n",
                rtf_escape(l),
                rtf_escape(r)
            )),
            Para::Gap => out.push_str("{\\pard\\par}\n"),
        }
    }
    out.push('}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rtf_con_acentos_y_llaves() {
        let r = to_rtf(&[
            Para::Title("Memoria año".into()),
            Para::Row("Capital {social}".into(), "3.000,00 €".into()),
        ]);
        assert!(r.starts_with("{\\rtf1"));
        assert!(r.contains("a\\u241?o"), "{r}");
        assert!(r.contains("\\{social\\}"));
        assert!(r.contains("\\u8364?"), "euro");
        assert!(r.ends_with('}'));
    }
}
