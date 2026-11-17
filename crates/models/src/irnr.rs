//! Retenciones del Impuesto sobre la Renta de no Residentes (IRNR) sobre rentas pagadas a no
//! residentes sin establecimiento permanente: tipo que corresponde, modelo 216 (declaración
//! trimestral o mensual de las retenciones) y modelo 296 (resumen anual).
//!
//! Base legal:
//! - Art. 13 TRLIRNR (RDLeg. 5/2004): rentas obtenidas en España (dividendos, intereses y
//!   cánones pagados por residentes; prestaciones de servicios utilizadas en territorio
//!   español).
//! - Art. 14 TRLIRNR: exenciones; las que aquí se guían son la 14.1.c (intereses de residentes
//!   en otro Estado de la UE), la 14.1.h (dividendos a la matriz de la UE, Directiva
//!   2011/96/UE) y la 14.1.m (cánones a una sociedad asociada de la UE, Directiva 2003/49/CE).
//! - Art. 25.1 TRLIRNR: tipo general del 24 % (19 % para residentes en la UE o en el EEE con
//!   intercambio efectivo de información tributaria) y 19 % para dividendos e intereses (letra
//!   f).
//! - Art. 31 TRLIRNR y arts. 7-11 RIRNR (RD 1776/2004): obligación de retener del pagador; el
//!   tipo reducido del convenio o la exención se acreditan con el certificado de residencia
//!   fiscal del perceptor, que vale un año desde su expedición.
//! - Orden EHA/3290/2008: modelos 216 y 296.
//!
//! `TODO(VERIFY)`: el número y la descripción de las casillas del 216 ([01]-[07] abajo) se han
//! tomado del formulario sin el diseño de registro, que no está en `fixtures/aeat/dr`: no se
//! genera fichero, sólo el borrador para teclearlo en el formulario de la Sede.
//!
//! Los tipos de convenio de [`CONVENIOS`] son los límites generales (sin las condiciones
//! especiales para sociedades con participación significativa, fondos de pensiones, etc.) de
//! los convenios que se han podido identificar con su BOE; todos van con `TODO(VERIFY)` y el
//! usuario puede teclear otro. Los demás países, sin tabla: el usuario teclea el tipo del
//! convenio.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use taxpain_core::eu::EU_COUNTRIES;
use taxpain_core::{Money, Rate};
use time::Date;

/// Naturaleza de la renta pagada al no residente.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Naturaleza {
    /// Dividendos y participaciones en beneficios.
    Dividendo,
    /// Intereses (préstamos de no residentes).
    Interes,
    /// Cánones: propiedad intelectual o industrial, software con cesión de derechos, know-how.
    Canon,
    /// Prestaciones de servicios utilizadas en España (asistencia técnica, profesionales).
    Servicio,
    Otros,
}

impl Naturaleza {
    pub fn code(self) -> &'static str {
        match self {
            Naturaleza::Dividendo => "dividendo",
            Naturaleza::Interes => "interes",
            Naturaleza::Canon => "canon",
            Naturaleza::Servicio => "servicio",
            Naturaleza::Otros => "otros",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "dividendo" => Naturaleza::Dividendo,
            "interes" => Naturaleza::Interes,
            "canon" => Naturaleza::Canon,
            "servicio" => Naturaleza::Servicio,
            "otros" => Naturaleza::Otros,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Naturaleza::Dividendo => "Dividendos",
            Naturaleza::Interes => "Intereses",
            Naturaleza::Canon => "Cánones",
            Naturaleza::Servicio => "Prestaciones de servicios",
            Naturaleza::Otros => "Otras rentas",
        }
    }
}

/// Cómo tributa la renta.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Regimen {
    /// Tipo del art. 25.1 TRLIRNR.
    General,
    /// Tipo reducido (o cero) del convenio, acreditado con el certificado de residencia.
    Convenio,
    /// Exenta por el art. 14 TRLIRNR.
    Exenta,
}

impl Regimen {
    pub fn code(self) -> &'static str {
        match self {
            Regimen::General => "general",
            Regimen::Convenio => "convenio",
            Regimen::Exenta => "exenta",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "general" => Regimen::General,
            "convenio" => Regimen::Convenio,
            "exenta" => Regimen::Exenta,
            _ => return None,
        })
    }
}

/// Exención del art. 14 TRLIRNR que se pide aplicar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Exencion {
    /// Art. 14.1.c: intereses obtenidos por residentes en otro Estado miembro de la UE.
    Art14_1C,
    /// Art. 14.1.h: dividendos a una sociedad matriz residente en otro Estado de la UE
    /// (participación de al menos el 5 % o de valor de adquisición de más de 20 millones,
    /// mantenida un año).
    Art14_1H,
    /// Art. 14.1.m: cánones a una sociedad asociada residente en otro Estado de la UE
    /// (participación directa de al menos el 25 %, mantenida un año).
    Art14_1M,
    /// Otra exención del art. 14 (la indica el usuario).
    Otra,
}

impl Exencion {
    pub fn code(self) -> &'static str {
        match self {
            Exencion::Art14_1C => "art14_1_c",
            Exencion::Art14_1H => "art14_1_h",
            Exencion::Art14_1M => "art14_1_m",
            Exencion::Otra => "otra",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "art14_1_c" => Exencion::Art14_1C,
            "art14_1_h" => Exencion::Art14_1H,
            "art14_1_m" => Exencion::Art14_1M,
            "otra" => Exencion::Otra,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Exencion::Art14_1C => "Intereses de residentes en la UE (art. 14.1.c TRLIRNR)",
            Exencion::Art14_1H => "Dividendos a la matriz de la UE (art. 14.1.h TRLIRNR)",
            Exencion::Art14_1M => "Cánones a sociedad asociada de la UE (art. 14.1.m TRLIRNR)",
            Exencion::Otra => "Otra exención del art. 14 TRLIRNR",
        }
    }

    /// Naturaleza a la que se aplica (ninguna en `Otra`).
    fn naturaleza(self) -> Option<Naturaleza> {
        match self {
            Exencion::Art14_1C => Some(Naturaleza::Interes),
            Exencion::Art14_1H => Some(Naturaleza::Dividendo),
            Exencion::Art14_1M => Some(Naturaleza::Canon),
            Exencion::Otra => None,
        }
    }
}

/// Estados del EEE que no son de la UE. `TODO(VERIFY)`: que con los tres haya «efectivo
/// intercambio de información tributaria» (art. 25.1.a TRLIRNR, que remite a la disp. adic.
/// 1.ª de la Ley 36/2006).
const EEE_NO_UE: [&str; 3] = ["IS", "LI", "NO"];

/// Código de país normalizado (mayúsculas; Grecia «EL» → «GR», como en ISO 3166).
pub fn pais(c: &str) -> String {
    match c.trim().to_ascii_uppercase().as_str() {
        "EL" => "GR".into(),
        s => s.into(),
    }
}

/// Otro Estado miembro de la UE (no España).
pub fn es_ue(country: &str) -> bool {
    let c = pais(country);
    let vies = if c == "GR" { "EL" } else { c.as_str() };
    c != "ES" && vies != "XI" && EU_COUNTRIES.contains(&vies)
}

/// Otro Estado de la UE o del EEE.
pub fn es_ue_eee(country: &str) -> bool {
    es_ue(country) || EEE_NO_UE.contains(&pais(country).as_str())
}

/// Tipo general del art. 25.1 TRLIRNR, sin convenio ni exención.
pub fn tipo_general(n: Naturaleza, country: &str) -> Rate {
    match n {
        // Art. 25.1.f: dividendos e intereses, 19 %.
        Naturaleza::Dividendo | Naturaleza::Interes => Rate::percent(19),
        // Art. 25.1.a: 24 %; 19 % para residentes en la UE o en el EEE.
        _ if es_ue_eee(country) => Rate::percent(19),
        _ => Rate::percent(24),
    }
}

/// Límites generales de un convenio para dividendos, intereses y cánones (en centésimas de
/// punto). Las prestaciones de servicios sin establecimiento permanente son beneficios
/// empresariales (art. 7 de los convenios): no tributan en España.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Convenio {
    pub pais: &'static str,
    pub nombre: &'static str,
    pub dividendos_bp: i32,
    pub intereses_bp: i32,
    pub canones_bp: i32,
    pub fuente: &'static str,
}

/// Convenios con su tipo general. `TODO(VERIFY)`: todos (comprobar con el texto del BOE y sus
/// protocolos antes de aplicarlos).
pub const CONVENIOS: &[Convenio] = &[
    Convenio {
        pais: "DE",
        nombre: "Alemania",
        dividendos_bp: 1500,
        intereses_bp: 0,
        canones_bp: 0,
        fuente: "Convenio de 3-2-2011 (BOE 30-7-2012), arts. 10-12",
    },
    Convenio {
        pais: "FR",
        nombre: "Francia",
        dividendos_bp: 1500,
        intereses_bp: 1000,
        canones_bp: 500,
        fuente: "Convenio de 10-10-1995 (BOE 12-6-1997), arts. 10-12",
    },
    Convenio {
        pais: "GB",
        nombre: "Reino Unido",
        dividendos_bp: 1000,
        intereses_bp: 0,
        canones_bp: 0,
        fuente: "Convenio de 14-3-2013 (BOE 15-5-2014), arts. 10-12",
    },
    Convenio {
        pais: "IT",
        nombre: "Italia",
        dividendos_bp: 1500,
        intereses_bp: 1200,
        canones_bp: 800,
        fuente: "Convenio de 8-9-1977 (BOE 22-12-1980), arts. 10-12",
    },
    Convenio {
        pais: "PT",
        nombre: "Portugal",
        dividendos_bp: 1500,
        intereses_bp: 1500,
        canones_bp: 500,
        fuente: "Convenio de 26-10-1993 (BOE 7-11-1995), arts. 10-12",
    },
    Convenio {
        pais: "US",
        nombre: "Estados Unidos",
        dividendos_bp: 1500,
        intereses_bp: 0,
        canones_bp: 0,
        fuente: "Convenio de 22-2-1990 con el Protocolo de 14-1-2013 (en vigor desde \
                 27-11-2019), arts. 10-12",
    },
];

pub fn convenio(country: &str) -> Option<&'static Convenio> {
    let c = pais(country);
    CONVENIOS.iter().find(|x| x.pais == c)
}

/// Tipo de la tabla de convenios para una naturaleza, si el país está en ella.
pub fn tipo_convenio_tabla(n: Naturaleza, country: &str) -> Option<Rate> {
    let c = convenio(country)?;
    Some(Rate::from_bp(match n {
        Naturaleza::Dividendo => c.dividendos_bp,
        Naturaleza::Interes => c.intereses_bp,
        Naturaleza::Canon => c.canones_bp,
        Naturaleza::Servicio => 0,
        Naturaleza::Otros => return None,
    }))
}

/// Certificado de residencia fiscal del perceptor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CertificadoResidencia {
    pub pais: String,
    pub expedido: Date,
    pub valido_hasta: Date,
    /// Expedido «a efectos del convenio».
    pub convenio: bool,
}

impl CertificadoResidencia {
    pub fn vigente(&self, fecha: Date) -> bool {
        self.expedido <= fecha && fecha <= self.valido_hasta
    }
}

/// Fin de la validez de un certificado expedido en `expedido`: un año (el día anterior al
/// aniversario).
pub fn valido_hasta(expedido: Date) -> Date {
    let next = expedido
        .replace_year(expedido.year() + 1)
        .unwrap_or_else(|_| {
            // 29 de febrero: el 28 del año siguiente.
            expedido
                .replace_day(28)
                .and_then(|d| d.replace_year(expedido.year() + 1))
                .unwrap_or(expedido)
        });
    next.previous_day().unwrap_or(next)
}

/// Lo que se sabe de la renta para proponer el tipo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entrada {
    pub naturaleza: Naturaleza,
    pub pais: String,
    pub fecha: Date,
    /// Certificado del perceptor que TaxPain tiene (el más reciente).
    pub certificado: Option<CertificadoResidencia>,
    /// Tipo del convenio tecleado por el usuario (prevalece sobre la tabla).
    pub tipo_convenio: Option<Rate>,
    /// Exención que se pide aplicar.
    pub exencion: Option<Exencion>,
}

/// Tipo propuesto con el régimen y por qué.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Propuesta {
    pub regimen: Regimen,
    pub tipo: Rate,
    pub tipo_general: Rate,
    pub exencion: Option<Exencion>,
    pub motivo: String,
    pub avisos: Vec<String>,
}

/// Propone el tipo de retención: exención (si se pide y se cumplen país y certificado), tipo
/// del convenio (con certificado vigente) o el general.
pub fn proponer(e: &Entrada) -> Propuesta {
    let general = tipo_general(e.naturaleza, &e.pais);
    let mut avisos = Vec::new();
    let c = pais(&e.pais);
    if c == "ES" || c.is_empty() {
        avisos.push(
            "El perceptor tiene país ES: un residente en España no tributa por el IRNR (su \
             retención va en el 111, el 115 o el 123)."
                .into(),
        );
    }
    let cert = e.certificado.as_ref().filter(|x| pais(&x.pais) == c);
    let vigente = cert.is_some_and(|x| x.vigente(e.fecha));
    match (&e.certificado, cert) {
        (None, _) => avisos.push(
            "Sin certificado de residencia fiscal del perceptor: se aplica el tipo general \
             (art. 25.1 TRLIRNR). Con el certificado se puede aplicar el convenio o la exención."
                .into(),
        ),
        (Some(x), None) => avisos.push(format!(
            "El certificado de residencia es de {} y la renta es de un residente en {c}: \
             revísalo.",
            pais(&x.pais)
        )),
        (Some(x), Some(_)) if !vigente => avisos.push(format!(
            "El certificado de residencia vale del {} al {}: no cubre la fecha de la renta \
             ({}). Pide uno nuevo al perceptor.",
            x.expedido, x.valido_hasta, e.fecha
        )),
        _ => {}
    }
    if let Some(ex) = e.exencion {
        let mut ok = vigente;
        if let Some(n) = ex.naturaleza()
            && n != e.naturaleza
        {
            avisos.push(format!(
                "La exención «{}» es para {}, no para {}.",
                ex.label(),
                n.label().to_lowercase(),
                e.naturaleza.label().to_lowercase()
            ));
            ok = false;
        }
        if ex != Exencion::Otra && !es_ue(&c) {
            avisos.push(format!(
                "La exención «{}» es sólo para residentes en otro Estado de la UE.",
                ex.label()
            ));
            ok = false;
        }
        if ok {
            match ex {
                Exencion::Art14_1H => avisos.push(
                    "Comprueba los requisitos de la matriz: participación de al menos el 5 % \
                     (o valor de adquisición de más de 20 millones de euros) mantenida un año, \
                     forma societaria de la Directiva 2011/96/UE y que no sea una estructura \
                     artificial (art. 14.1.h TRLIRNR)."
                        .into(),
                ),
                Exencion::Art14_1M => avisos.push(
                    "Comprueba que la perceptora sea sociedad asociada: participación directa \
                     de al menos el 25 % mantenida un año (art. 14.1.m TRLIRNR y Directiva \
                     2003/49/CE)."
                        .into(),
                ),
                Exencion::Art14_1C => avisos.push(
                    "La exención de intereses no se aplica si se obtienen a través de un \
                     paraíso fiscal o de un establecimiento permanente fuera de la UE (art. \
                     14.1.c TRLIRNR)."
                        .into(),
                ),
                Exencion::Otra => avisos.push(
                    "Exención del art. 14 TRLIRNR indicada por ti: anota en el concepto cuál \
                     es. TODO(VERIFY)."
                        .into(),
                ),
            }
            return Propuesta {
                regimen: Regimen::Exenta,
                tipo: Rate::ZERO,
                tipo_general: general,
                exencion: Some(ex),
                motivo: ex.label().into(),
                avisos,
            };
        }
        if !vigente {
            avisos.push(
                "La exención exige el certificado de residencia vigente: se aplica el tipo \
                 general."
                    .into(),
            );
        }
    }
    if vigente {
        let tabla = tipo_convenio_tabla(e.naturaleza, &c);
        let (tipo, fuente) = match (e.tipo_convenio, tabla) {
            (Some(t), _) => (Some(t), "tipo del convenio tecleado".to_string()),
            (None, Some(t)) => (
                Some(t),
                format!(
                    "tipo general del convenio con {} ({}; TODO(VERIFY))",
                    convenio(&c).map_or("", |x| x.nombre),
                    convenio(&c).map_or("", |x| x.fuente)
                ),
            ),
            (None, None) => (None, String::new()),
        };
        if let Some(mut t) = tipo {
            if t.bp() > general.bp() {
                avisos.push(format!(
                    "El tipo del convenio ({t}) es mayor que el general ({general}): se aplica \
                     el general."
                ));
                t = general;
            }
            if e.naturaleza == Naturaleza::Servicio {
                avisos.push(
                    "Con convenio, un servicio prestado sin establecimiento permanente suele ser \
                     beneficio empresarial (art. 7) y no tributa en España; pero algunos \
                     convenios tratan la asistencia técnica como canon. Revísalo. TODO(VERIFY)."
                        .into(),
                );
            }
            if cert.is_some_and(|x| !x.convenio) {
                avisos.push(
                    "El certificado no dice que sea «a efectos del convenio»: la AEAT puede no \
                     aceptarlo para el tipo reducido."
                        .into(),
                );
            }
            return Propuesta {
                regimen: Regimen::Convenio,
                tipo: t,
                tipo_general: general,
                exencion: None,
                motivo: fuente,
                avisos,
            };
        }
        avisos.push(format!(
            "TaxPain no tiene el tipo del convenio con {c}: teclea el que diga el convenio, o se \
             aplica el general."
        ));
    }
    Propuesta {
        regimen: Regimen::General,
        tipo: general,
        tipo_general: general,
        exencion: None,
        motivo: "tipo general (art. 25.1 TRLIRNR)".into(),
        avisos,
    }
}

/// Una renta pagada a un no residente, ya con su tipo y retención.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Renta {
    pub fecha: Date,
    pub nif: String,
    pub nombre: String,
    pub pais: String,
    pub naturaleza: Naturaleza,
    pub bruto: Money,
    pub tipo: Rate,
    pub retencion: Money,
    pub regimen: Regimen,
    pub exencion: Option<Exencion>,
}

/// Casillas del 216. `TODO(VERIFY)` (ver el módulo).
pub mod casillas_216 {
    /// Rentas sometidas a retención: número de rentas, base y retenciones.
    pub const N_RENTAS: u16 = 1;
    pub const BASE: u16 = 2;
    pub const RETENCIONES: u16 = 3;
    /// Rentas no sometidas a retención (exentas o con tipo cero del convenio).
    pub const N_RENTAS_SIN: u16 = 4;
    pub const BASE_SIN: u16 = 5;
    /// A deducir (complementaria): TaxPain lo deja a cero.
    pub const A_DEDUCIR: u16 = 6;
    pub const RESULTADO: u16 = 7;
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Result216 {
    pub year: i32,
    /// «1T»…«4T» o «01»…«12».
    pub period: String,
    pub con_retencion: u64,
    pub base: Money,
    pub retenciones: Money,
    pub sin_retencion: u64,
    pub base_sin: Money,
    pub resultado: Money,
    /// «I» a ingresar; «N» negativa (sólo rentas sin retención); vacío: no hay que presentar.
    pub declaracion: String,
    pub warnings: Vec<String>,
}

impl Result216 {
    /// (casilla, importe) para el borrador.
    pub fn boxes(&self) -> Vec<(u16, Money)> {
        use casillas_216::*;
        vec![
            (BASE, self.base),
            (RETENCIONES, self.retenciones),
            (BASE_SIN, self.base_sin),
            (A_DEDUCIR, Money::ZERO),
            (RESULTADO, self.resultado),
        ]
    }

    pub fn payees(&self) -> Vec<(u16, u64)> {
        use casillas_216::*;
        vec![
            (N_RENTAS, self.con_retencion),
            (N_RENTAS_SIN, self.sin_retencion),
        ]
    }
}

/// 216 del periodo con las rentas cuyo devengo cae en él (el llamante las filtra).
pub fn compute_216(year: i32, period: &str, rentas: &[Renta]) -> Result216 {
    let (con, sin): (Vec<&Renta>, Vec<&Renta>) =
        rentas.iter().partition(|r| !r.retencion.is_zero());
    let base: Money = con.iter().map(|r| r.bruto).sum();
    let retenciones: Money = con.iter().map(|r| r.retencion).sum();
    let base_sin: Money = sin.iter().map(|r| r.bruto).sum();
    let mut warnings = Vec::new();
    for r in &con {
        let esperado = r.bruto.apply_rate(r.tipo);
        if esperado != r.retencion {
            warnings.push(format!(
                "{} ({}): la retención apuntada ({}) no es el {} de {} ({esperado}).",
                r.nombre, r.nif, r.retencion, r.tipo, r.bruto
            ));
        }
    }
    let declaracion = if !retenciones.is_zero() {
        "I"
    } else if !sin.is_empty() {
        warnings.push(
            "Sólo hay rentas sin retención (exentas o con tipo cero del convenio): el borrador \
             sale como declaración negativa. TODO(VERIFY): si en tu caso hay que presentarla \
             (Orden EHA/3290/2008)."
                .into(),
        );
        "N"
    } else {
        ""
    };
    Result216 {
        year,
        period: period.into(),
        con_retencion: con.len() as u64,
        base,
        retenciones,
        sin_retencion: sin.len() as u64,
        base_sin,
        resultado: retenciones,
        declaracion: declaracion.into(),
        warnings,
    }
}

/// Un registro del 296: perceptor, naturaleza, régimen y tipo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Linea296 {
    pub nif: String,
    pub nombre: String,
    pub pais: String,
    pub naturaleza: Naturaleza,
    pub regimen: Regimen,
    pub exencion: Option<Exencion>,
    pub tipo: Rate,
    /// Primera y última fecha de devengo de las rentas sumadas.
    pub desde: Date,
    pub hasta: Date,
    pub rentas: u64,
    pub bruto: Money,
    pub retencion: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Result296 {
    pub year: i32,
    pub lines: Vec<Linea296>,
    pub total_bruto: Money,
    pub total_retencion: Money,
    pub warnings: Vec<String>,
}

/// 296 del año: un registro por perceptor, naturaleza, régimen y tipo.
pub fn compute_296(year: i32, rentas: &[Renta]) -> Result296 {
    let mut acc: BTreeMap<(String, Naturaleza, Regimen, i32), Linea296> = BTreeMap::new();
    for r in rentas.iter().filter(|r| r.fecha.year() == year) {
        let key = (
            r.nif.trim().to_uppercase(),
            r.naturaleza,
            r.regimen,
            r.tipo.bp(),
        );
        acc.entry(key)
            .and_modify(|l| {
                l.bruto += r.bruto;
                l.retencion += r.retencion;
                l.rentas += 1;
                l.desde = l.desde.min(r.fecha);
                l.hasta = l.hasta.max(r.fecha);
            })
            .or_insert_with(|| Linea296 {
                nif: r.nif.trim().to_uppercase(),
                nombre: r.nombre.clone(),
                pais: pais(&r.pais),
                naturaleza: r.naturaleza,
                regimen: r.regimen,
                exencion: r.exencion,
                tipo: r.tipo,
                desde: r.fecha,
                hasta: r.fecha,
                rentas: 1,
                bruto: r.bruto,
                retencion: r.retencion,
            });
    }
    let lines: Vec<Linea296> = acc.into_values().collect();
    let mut warnings = vec![
        "TaxPain no tiene el diseño de registro del 296: no genera el fichero. Presenta el 296 \
         en el formulario de la Sede con estos registros (clave de la renta, código de país, \
         fecha de devengo y domicilio del perceptor en su país). TODO(VERIFY): claves del \
         Anexo de la Orden EHA/3290/2008."
            .to_string(),
    ];
    for l in lines.iter().filter(|l| l.regimen != Regimen::General) {
        warnings.push(format!(
            "{} ({}): {} con tipo reducido o exención; guarda el certificado de residencia que \
             lo justifica.",
            l.nombre,
            l.nif,
            l.naturaleza.label().to_lowercase()
        ));
    }
    Result296 {
        year,
        total_bruto: lines.iter().map(|l| l.bruto).sum(),
        total_retencion: lines.iter().map(|l| l.retencion).sum(),
        lines,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn entrada(n: Naturaleza, pais: &str) -> Entrada {
        Entrada {
            naturaleza: n,
            pais: pais.into(),
            fecha: date!(2026 - 05 - 10),
            certificado: None,
            tipo_convenio: None,
            exencion: None,
        }
    }

    fn cert(pais: &str) -> CertificadoResidencia {
        CertificadoResidencia {
            pais: pais.into(),
            expedido: date!(2026 - 01 - 15),
            valido_hasta: valido_hasta(date!(2026 - 01 - 15)),
            convenio: true,
        }
    }

    #[test]
    fn tipos_generales_del_art_25() {
        assert_eq!(tipo_general(Naturaleza::Dividendo, "US"), Rate::percent(19));
        assert_eq!(tipo_general(Naturaleza::Interes, "CN"), Rate::percent(19));
        assert_eq!(tipo_general(Naturaleza::Canon, "US"), Rate::percent(24));
        assert_eq!(tipo_general(Naturaleza::Canon, "FR"), Rate::percent(19));
        assert_eq!(tipo_general(Naturaleza::Servicio, "NO"), Rate::percent(19));
        assert_eq!(tipo_general(Naturaleza::Servicio, "GB"), Rate::percent(24));
        assert!(es_ue("EL") && es_ue("gr") && !es_ue("ES") && !es_ue("XI"));
    }

    #[test]
    fn el_certificado_vale_un_ano() {
        assert_eq!(valido_hasta(date!(2026 - 01 - 15)), date!(2027 - 01 - 14));
        assert_eq!(valido_hasta(date!(2028 - 02 - 29)), date!(2029 - 02 - 27));
        let c = cert("FR");
        assert!(c.vigente(date!(2027 - 01 - 14)));
        assert!(!c.vigente(date!(2027 - 01 - 15)));
    }

    #[test]
    fn sin_certificado_tipo_general() {
        let p = proponer(&entrada(Naturaleza::Canon, "US"));
        assert_eq!(p.regimen, Regimen::General);
        assert_eq!(p.tipo, Rate::percent(24));
        assert!(p.avisos.iter().any(|a| a.contains("Sin certificado")));
    }

    #[test]
    fn con_certificado_convenio_de_la_tabla_o_tecleado() {
        let mut e = entrada(Naturaleza::Canon, "FR");
        e.certificado = Some(cert("FR"));
        let p = proponer(&e);
        assert_eq!(p.regimen, Regimen::Convenio);
        assert_eq!(p.tipo, Rate::percent(5));
        e.tipo_convenio = Some(Rate::percent(3));
        assert_eq!(proponer(&e).tipo, Rate::percent(3));
        // Un tipo tecleado mayor que el general no se aplica.
        e.tipo_convenio = Some(Rate::percent(30));
        assert_eq!(proponer(&e).tipo, Rate::percent(19));
        // País sin tabla y sin tipo tecleado: general con aviso.
        let mut e = entrada(Naturaleza::Canon, "JP");
        e.certificado = Some(cert("JP"));
        let p = proponer(&e);
        assert_eq!(p.regimen, Regimen::General);
        assert!(p.avisos.iter().any(|a| a.contains("teclea")));
    }

    #[test]
    fn certificado_caducado_o_de_otro_pais() {
        let mut e = entrada(Naturaleza::Canon, "FR");
        e.fecha = date!(2027 - 03 - 01);
        e.certificado = Some(cert("FR"));
        let p = proponer(&e);
        assert_eq!(p.regimen, Regimen::General);
        assert!(p.avisos.iter().any(|a| a.contains("no cubre")));
        let mut e = entrada(Naturaleza::Canon, "FR");
        e.certificado = Some(cert("DE"));
        assert_eq!(proponer(&e).regimen, Regimen::General);
    }

    #[test]
    fn exenciones_del_art_14() {
        let mut e = entrada(Naturaleza::Interes, "DE");
        e.certificado = Some(cert("DE"));
        e.exencion = Some(Exencion::Art14_1C);
        let p = proponer(&e);
        assert_eq!(p.regimen, Regimen::Exenta);
        assert_eq!(p.tipo, Rate::ZERO);
        // Fuera de la UE no hay exención de intereses.
        let mut e = entrada(Naturaleza::Interes, "US");
        e.certificado = Some(cert("US"));
        e.exencion = Some(Exencion::Art14_1C);
        let p = proponer(&e);
        assert_eq!(p.regimen, Regimen::Convenio);
        assert_eq!(p.tipo, Rate::ZERO);
        // Sin certificado, ni exención ni convenio.
        let mut e = entrada(Naturaleza::Dividendo, "PT");
        e.exencion = Some(Exencion::Art14_1H);
        let p = proponer(&e);
        assert_eq!(p.regimen, Regimen::General);
        assert_eq!(p.tipo, Rate::percent(19));
        // Naturaleza equivocada.
        let mut e = entrada(Naturaleza::Canon, "PT");
        e.certificado = Some(cert("PT"));
        e.exencion = Some(Exencion::Art14_1H);
        assert_ne!(proponer(&e).regimen, Regimen::Exenta);
    }

    fn renta(nif: &str, n: Naturaleza, bruto: i64, tipo: i32, fecha: Date) -> Renta {
        let tipo = Rate::percent(tipo);
        Renta {
            fecha,
            nif: nif.into(),
            nombre: format!("Perceptor {nif}"),
            pais: "US".into(),
            naturaleza: n,
            bruto: Money::from_cents(bruto),
            tipo,
            retencion: Money::from_cents(bruto).apply_rate(tipo),
            regimen: if tipo.bp() == 2400 {
                Regimen::General
            } else {
                Regimen::Convenio
            },
            exencion: None,
        }
    }

    #[test]
    fn el_216_separa_rentas_con_y_sin_retencion() {
        let rs = [
            renta("X1", Naturaleza::Canon, 100_000, 24, date!(2026 - 04 - 10)),
            renta("X2", Naturaleza::Canon, 50_000, 0, date!(2026 - 05 - 10)),
            renta("X1", Naturaleza::Canon, 200_000, 24, date!(2026 - 06 - 10)),
        ];
        let r = compute_216(2026, "2T", &rs);
        assert_eq!(r.con_retencion, 2);
        assert_eq!(r.base, Money::from_cents(300_000));
        assert_eq!(r.retenciones, Money::from_cents(72_000));
        assert_eq!(r.sin_retencion, 1);
        assert_eq!(r.base_sin, Money::from_cents(50_000));
        assert_eq!(r.declaracion, "I");
        assert!(r.boxes().contains(&(7, Money::from_cents(72_000))));
        let r = compute_216(2026, "2T", &rs[1..2]);
        assert_eq!(r.declaracion, "N");
        assert_eq!(compute_216(2026, "3T", &[]).declaracion, "");
    }

    #[test]
    fn el_296_agrupa_por_perceptor_naturaleza_y_tipo() {
        let rs = [
            renta("X1", Naturaleza::Canon, 100_000, 24, date!(2026 - 04 - 10)),
            renta("x1", Naturaleza::Canon, 200_000, 24, date!(2026 - 11 - 10)),
            renta("X1", Naturaleza::Interes, 10_000, 0, date!(2026 - 06 - 10)),
            renta("X2", Naturaleza::Canon, 50_000, 24, date!(2025 - 05 - 10)),
        ];
        let r = compute_296(2026, &rs);
        assert_eq!(r.lines.len(), 2);
        let canon = r
            .lines
            .iter()
            .find(|l| l.naturaleza == Naturaleza::Canon)
            .unwrap();
        assert_eq!(canon.rentas, 2);
        assert_eq!(canon.desde, date!(2026 - 04 - 10));
        assert_eq!(canon.hasta, date!(2026 - 11 - 10));
        assert_eq!(r.total_bruto, Money::from_cents(310_000));
        assert_eq!(r.total_retencion, Money::from_cents(72_000));
    }
}
