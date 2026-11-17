//! Declaraciones informativas anuales de registros de 500 posiciones: 347 (operaciones con
//! terceras personas), 190 (retenciones del trabajo y de actividades económicas), 180
//! (retenciones de arrendamientos de inmuebles urbanos) y 193 (retenciones del capital
//! mobiliario: dividendos).
//!
//! Diseños transcritos de los PDF oficiales a `fixtures/aeat/dr/{347,190,180,193}.json`, con sus
//! reglas y dudas en los `*-notas.md` del mismo directorio. Formato común: un registro de
//! declarante (tipo 1) y uno de tipo 2 por tercero; importes con un campo de signo («N» si es
//! negativo), la parte entera y la decimal; ISO-8859-1; registros separados por CRLF
//! (`TODO(VERIFY)`: separador, no lo fija ningún PDF).
//!
//! Base legal:
//! - 347: arts. 31 a 35 RD 1065/2007 (RGAT); Orden HAC/1431/2025 y anteriores. Umbral de
//!   3.005,06 € por tercero y clave en el año (art. 33.1 RGAT).
//! - 190: art. 108 RIRPF; Orden EHA/3127/2009.
//! - 180: art. 108 RIRPF; Orden HAP/1732/2014.
//! - 193: art. 108.3 RIRPF y art. 68 RIS (RD 634/2015); Orden EHA/3377/2011 (act. Orden
//!   HAC/1430/2025), del 1 al 31 de enero (art. 5). Incluye los dividendos a sociedades no
//!   sometidos a retención (art. 3.2 de la orden: letra p del art. 61 RIS actual).
//!
//! Alcance y `TODO(VERIFY)`:
//! - 347: claves A (compras) y B (ventas) desde las facturas, IVA incluido; quedan fuera las
//!   operaciones que van en el 349, las exportaciones e importaciones, las compras con
//!   retención (que van en el 190/180) y las ventas en las que el cliente nos retiene (las
//!   declara él en su 190/180): art. 33.2.i RGAT.
//! - 190: clave G (actividades profesionales, subclave 01), clave A (empleados por cuenta
//!   ajena) con los datos de la ficha del empleado: año de nacimiento, situación familiar,
//!   discapacidad, contrato, movilidad, cotizaciones como gastos deducibles e hijos; clave E
//!   (consejeros y administradores, subclaves 01-04, con las cotizaciones como gastos
//!   deducibles en E.01/E.02 y la retención en «Hacienda Estatal», pos. 323-335) y clave L,
//!   subclave 01 (dietas y gastos de viaje exceptuados, art. 9 RIRPF). Sin incapacidad laboral,
//!   especie, reducciones ni ascendientes.
//! - 193: dividendos (clave A) de una sociedad que los paga directamente a sus socios
//!   residentes: naturaleza 02 con retención u 08 sin ella (art. 128.4.d LIS y art. 61.p RIS);
//!   naturaleza del declarante «S» (no es entidad depositaria, mediadora ni emisora con
//!   valores depositados).
//!   Sin intereses (claves B/D), otras rentas (C), préstamos de valores ni relación de gastos.
//! - 180: necesita la referencia catastral y la dirección del inmueble; sin ellas se avisa.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::Money;

use crate::Declarant;
use crate::dr::{DrError, PageReader, PageWriter, Spec, alnum};

pub const DR347_JSON: &str = include_str!("../../../fixtures/aeat/dr/347.json");
pub const DR190_JSON: &str = include_str!("../../../fixtures/aeat/dr/190.json");
pub const DR180_JSON: &str = include_str!("../../../fixtures/aeat/dr/180.json");
pub const DR193_JSON: &str = include_str!("../../../fixtures/aeat/dr/193.json");

pub fn spec(model: &str) -> &'static Spec {
    static S347: std::sync::OnceLock<Spec> = std::sync::OnceLock::new();
    static S190: std::sync::OnceLock<Spec> = std::sync::OnceLock::new();
    static S180: std::sync::OnceLock<Spec> = std::sync::OnceLock::new();
    static S193: std::sync::OnceLock<Spec> = std::sync::OnceLock::new();
    match model {
        "347" => S347.get_or_init(|| Spec::parse(DR347_JSON)),
        "190" => S190.get_or_init(|| Spec::parse(DR190_JSON)),
        "193" => S193.get_or_init(|| Spec::parse(DR193_JSON)),
        _ => S180.get_or_init(|| Spec::parse(DR180_JSON)),
    }
}

/// Umbral del 347 (art. 33.1 RGAT): 3.005,06 € por tercero y clave en el año.
pub const THRESHOLD_347: Money = Money::from_cents(300_506);

/// Importes percibidos en metálico que se declaran en el 347 (art. 34.1.h RGAT): los
/// superiores a 6.000 € de cada tercero en el año.
pub const CASH_THRESHOLD_347: Money = Money::from_cents(600_000);

fn fit(s: &str, n: usize) -> String {
    alnum(s)
        .chars()
        .take(n)
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// Importe con signo en tres campos: signo (`sign`), parte entera (`int`) y decimal (`dec`).
fn signed(
    w: &mut PageWriter<'_>,
    sign: Option<u16>,
    int: u16,
    dec: u16,
    m: Money,
) -> Result<(), DrError> {
    if let Some(s) = sign
        && m.cents() < 0
    {
        w.text(s, "N")?;
    }
    let abs = m.cents().unsigned_abs();
    w.text(int, &(abs / 100).to_string())?
        .text(dec, &format!("{:02}", abs % 100))?;
    Ok(())
}

/// Número identificativo de 13 cifras: modelo + ejercicio + secuencia.
/// `TODO(VERIFY)`: en la presentación por formulario lo asigna la AEAT.
fn id_number(model: &str, year: i32, seq: u32) -> String {
    format!("{model}{year:04}{:06}", seq % 1_000_000)
}

/// Campos 1–13 del registro de declarante, comunes a los cuatro modelos.
fn declarant_record<'a>(
    spec: &'a Spec,
    model: &str,
    year: i32,
    who: &Declarant,
    count: usize,
) -> Result<PageWriter<'a>, DrError> {
    let mut w = PageWriter::new(spec.page(&format!("{model}-1"))?);
    let name = fit(&who.full_name(), 40);
    w.text(3, &year.to_string())?
        .text(4, &who.nif.to_uppercase())?
        .text(5, &name)?
        .text(6, "T")?
        .text(8, &name)?
        .text(9, &id_number(model, year, 1))?
        .text(13, &count.to_string())?;
    Ok(w)
}

fn common_third<'a>(
    spec: &'a Spec,
    sheet: &str,
    year: i32,
    who: &Declarant,
) -> Result<PageWriter<'a>, DrError> {
    let mut w = PageWriter::new(spec.page(sheet)?);
    w.text(3, &year.to_string())?
        .text(4, &who.nif.to_uppercase())?;
    Ok(w)
}

// ---------- 347 ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Key347 {
    /// A: adquisiciones de bienes y servicios.
    A,
    /// B: entregas de bienes y prestaciones de servicios.
    B,
}

impl Key347 {
    pub fn code(self) -> &'static str {
        match self {
            Key347::A => "A",
            Key347::B => "B",
        }
    }
}

/// Una operación (factura) con un tercero español, IVA incluido.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Op347 {
    pub nif: String,
    pub name: String,
    /// Código de provincia (2 cifras) del domicilio del tercero.
    pub province: String,
    pub key: Key347,
    /// Trimestre natural (1–4) de la operación.
    pub quarter: u8,
    pub amount: Money,
    /// Arrendamiento de local de negocio.
    pub business_rental: bool,
    /// Lo percibido en metálico de esta operación en el año (art. 34.1.h RGAT); sólo en las
    /// ventas (clave B).
    #[serde(default)]
    pub cash: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line347 {
    pub nif: String,
    pub name: String,
    pub province: String,
    pub key: Key347,
    pub total: Money,
    pub quarters: [Money; 4],
    pub business_rental: bool,
    /// Importe percibido en metálico del tercero en el año, si pasa de 6.000 €
    /// ([`CASH_THRESHOLD_347`]); si no, cero (no se declara).
    #[serde(default)]
    pub cash: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Result347 {
    pub year: i32,
    pub lines: Vec<Line347>,
    pub total: Money,
    pub warnings: Vec<String>,
}

pub fn compute_347(year: i32, ops: &[Op347]) -> Result347 {
    let mut acc: BTreeMap<(String, Key347), Line347> = BTreeMap::new();
    for o in ops {
        let q = usize::from(o.quarter.clamp(1, 4) - 1);
        let l = acc
            .entry((o.nif.to_uppercase(), o.key))
            .or_insert_with(|| Line347 {
                nif: o.nif.to_uppercase(),
                name: o.name.clone(),
                province: o.province.clone(),
                key: o.key,
                total: Money::ZERO,
                quarters: [Money::ZERO; 4],
                business_rental: false,
                cash: Money::ZERO,
            });
        l.total += o.amount;
        l.quarters[q] += o.amount;
        l.business_rental |= o.business_rental;
        l.cash += o.cash;
    }
    let lines: Vec<Line347> = acc
        .into_values()
        .filter(|l| l.total.abs() > THRESHOLD_347)
        .map(|mut l| {
            // El metálico sólo se declara si pasa de 6.000 € (art. 34.1.h RGAT).
            if l.cash <= CASH_THRESHOLD_347 {
                l.cash = Money::ZERO;
            }
            l
        })
        .collect();
    let mut warnings = Vec::new();
    for l in &lines {
        if l.province.len() != 2 {
            warnings.push(format!(
                "{} ({}): falta la provincia del domicilio",
                l.name, l.nif
            ));
        }
    }
    Result347 {
        year,
        total: lines.iter().map(|l| l.total).sum(),
        lines,
        warnings,
    }
}

pub fn file_347(r: &Result347, who: &Declarant) -> Result<String, DrError> {
    let s = spec("347");
    let rentals: Vec<&Line347> = r.lines.iter().filter(|l| l.business_rental).collect();
    let mut w = declarant_record(s, "347", r.year, who, r.lines.len())?;
    signed(&mut w, Some(14), 15, 16, r.total)?;
    // Los registros de inmueble (arrendamientos) aún no se generan: 0 inmuebles.
    let rentals_total: Money = rentals.iter().map(|l| l.total).sum();
    signed(&mut w, Some(18), 19, 20, rentals_total)?;
    let mut out = vec![w.finish()?];
    for l in &r.lines {
        let mut w = common_third(s, "347-2D", r.year, who)?;
        w.text(5, &l.nif)?
            .text(7, &fit(&l.name, 40))?
            .text(9, &l.province)?
            .text(12, l.key.code())?;
        signed(&mut w, Some(13), 14, 15, l.total)?;
        if l.business_rental {
            w.text(17, "X")?;
        }
        // Importe percibido en metálico (pos. 101-115) y ejercicio de las operaciones que lo
        // originan (pos. 132-135): el del propio 347 (el metálico de facturas de otros años se
        // avisa aparte).
        if l.cash.cents() > 0 {
            signed(&mut w, None, 18, 19, l.cash)?;
            w.text(23, &r.year.to_string())?;
        }
        for (i, q) in l.quarters.iter().enumerate() {
            let base = 24 + 6 * u16::try_from(i).unwrap_or(0);
            signed(&mut w, Some(base), base + 1, base + 2, *q)?;
        }
        out.push(w.finish()?);
    }
    Ok(out.join("\r\n"))
}

// ---------- Lectura de un 347 o un 349 ya presentado ----------

/// Un tercero (347), un operador (349) o un perceptor (190, 180, 193) de un fichero ya
/// presentado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadLine {
    /// NIF del tercero; en el 349, con el código de país delante («DE123456789»).
    pub nif: String,
    pub name: String,
    /// Clave de operación: A-G en el 347; E, A, S, I, T… en el 349; clave y subclave en el 190
    /// («A», «G01», «E01»); clave en el 193 («A», «B»); «1» (dineraria) o «2» (especie) en el
    /// 180.
    pub key: String,
    /// Importe: de las operaciones (347, 349), percepción íntegra dineraria (190), base de
    /// retenciones (180) o importe de las percepciones (193).
    pub amount: Money,
    /// Retenciones e ingresos a cuenta del perceptor (190, 180, 193).
    #[serde(default)]
    pub withheld: Option<Money>,
}

/// Una informativa leída de su fichero (registros de 500 posiciones): 347, 349, 190, 180 o 193.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadFile {
    /// «347», «349», «190», «180» o «193».
    pub model: String,
    pub year: i32,
    /// «0A», salvo en el 349: «1T»…«4T», «01»…«12» o «0A».
    pub period: String,
    pub declarant_nif: String,
    /// Importe total del registro de declarante.
    pub total: Money,
    /// Retenciones del registro de declarante (190, 180, 193).
    #[serde(default)]
    pub withheld: Option<Money>,
    /// Terceros, operadores o perceptores (sin las rectificaciones de periodos anteriores del
    /// 349 ni los registros de inmueble del 347).
    pub lines: Vec<ReadLine>,
}

/// Registros de 500 caracteres: separados por fin de línea o seguidos.
fn records_500(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.split('\n') {
        let chars: Vec<char> = line.trim_end_matches('\r').chars().collect();
        if chars.is_empty() {
            continue;
        }
        for c in chars.chunks(500) {
            out.push(c.iter().collect());
        }
    }
    out
}

/// Lee el fichero de un 347 o un 349 (el inverso de [`file_347`] y de
/// [`crate::m349::file`]): registro de declarante (tipo 1) y de declarados u operadores
/// (tipo 2). También los perceptores del 190, el 180 y el 193 ([`read_payees`]). `None` si no
/// lo es.
pub fn read_file(text: &str) -> Option<ReadFile> {
    let recs = records_500(text);
    let first = recs.first()?;
    let model: String = first.chars().skip(1).take(3).collect();
    if !first.starts_with('1') {
        return None;
    }
    if matches!(model.as_str(), "190" | "180" | "193") {
        return read_payees(&model, &recs);
    }
    if !matches!(model.as_str(), "347" | "349") {
        return None;
    }
    let spec = if model == "347" {
        spec("347")
    } else {
        crate::m349::spec()
    };
    let head = PageReader::new(spec.page(&format!("{model}-1")).ok()?, first).ok()?;
    let year: i32 = head.raw(3).ok()?.parse().ok()?;
    let declarant_nif = head.raw(4).ok()?.trim().to_owned();
    let (period, total) = if model == "347" {
        ("0A".to_owned(), signed_amount(&head, 14, 15, 16)?)
    } else {
        (head.raw(13).ok()?.trim().to_owned(), head.amount(15).ok()??)
    };
    let mut lines = Vec::new();
    for r in recs.iter().skip(1) {
        if !r.starts_with(&format!("2{model}")) {
            continue;
        }
        let line = if model == "347" {
            let d = PageReader::new(spec.page("347-2D").ok()?, r).ok()?;
            // Registro de inmueble (hoja «I»): no es un tercero.
            if d.raw(8).ok()?.trim() != "D" {
                continue;
            }
            ReadLine {
                nif: d.raw(5).ok()?.trim().to_owned(),
                name: d.raw(7).ok()?.trim().to_owned(),
                key: d.raw(12).ok()?.trim().to_owned(),
                amount: signed_amount(&d, 13, 14, 15)?,
                withheld: None,
            }
        } else {
            let o = PageReader::new(spec.page("349-2op").ok()?, r).ok()?;
            // Rectificación de un periodo anterior: sin importe del periodo (va en blanco).
            if o.raw(10).ok()?.trim().is_empty() {
                continue;
            }
            let amount = o.amount(10).ok()??;
            ReadLine {
                nif: format!("{}{}", o.raw(6).ok()?.trim(), o.raw(7).ok()?.trim()),
                name: o.raw(8).ok()?.trim().to_owned(),
                key: o.raw(9).ok()?.trim().to_owned(),
                amount,
                withheld: None,
            }
        };
        lines.push(line);
    }
    Some(ReadFile {
        model,
        year,
        period,
        declarant_nif,
        total,
        withheld: None,
        lines,
    })
}

/// Perceptores (registros de tipo 2) de un 190, un 180 o un 193, con los campos de
/// `fixtures/aeat/dr/{190,180,193}.json` (hoja «-2»), los mismos que escriben [`file_190`],
/// [`file_180`] y [`file_193`]:
/// - 190: NIF (5), nombre (7), clave (9) y subclave (10), percepción íntegra dineraria no
///   derivada de incapacidad laboral con signo (11-13) y sus retenciones (14-15). La parte en
///   especie y la de incapacidad laboral no se leen.
/// - 180: NIF (5), nombre (7), modalidad (9), base con signo (10-12) y retenciones (15).
/// - 193: NIF (5), nombre (7), clave (12), importe de las percepciones (20-21) y retenciones
///   (29-30).
fn read_payees(model: &str, recs: &[String]) -> Option<ReadFile> {
    let s = spec(model);
    let summary = read_summary(&recs.join("\n"))?;
    let page = s.page(&format!("{model}-2")).ok()?;
    let mut lines = Vec::new();
    for r in recs.iter().skip(1) {
        if !r.starts_with(&format!("2{model}")) {
            continue;
        }
        let p = PageReader::new(page, r).ok()?;
        let nif = p.raw(5).ok()?.trim().to_owned();
        let name = p.raw(7).ok()?.trim().to_owned();
        let line = match model {
            "190" => {
                let key = p.raw(9).ok()?.trim().to_owned();
                let sub = p.raw(10).ok()?.trim().to_owned();
                ReadLine {
                    nif,
                    name,
                    key: if sub.is_empty() || sub == "00" {
                        key
                    } else {
                        format!("{key}{sub}")
                    },
                    amount: signed_amount(&p, 11, 12, 13)?,
                    withheld: Some(unsigned_amount(&p, 14, 15)?),
                }
            }
            "180" => ReadLine {
                nif,
                name,
                key: p.raw(9).ok()?.trim().to_owned(),
                amount: signed_amount(&p, 10, 11, 12)?,
                withheld: Some(p.amount(15).ok()?.unwrap_or(Money::ZERO)),
            },
            _ => ReadLine {
                nif,
                name,
                key: p.raw(12).ok()?.trim().to_owned(),
                amount: unsigned_amount(&p, 20, 21)?,
                withheld: Some(unsigned_amount(&p, 29, 30)?),
            },
        };
        lines.push(line);
    }
    Some(ReadFile {
        model: summary.model,
        year: summary.year,
        period: summary.period,
        declarant_nif: summary.declarant_nif,
        total: summary.total,
        withheld: summary.withheld,
        lines,
    })
}

/// Importe con signo en tres campos (el inverso de `signed`).
/// Totales del registro de declarante de una declaración informativa (190, 193, 180, 347 o
/// 349), para la evolución de la empresa y los cruces entre modelos.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub model: String,
    pub year: i32,
    /// «0A», salvo en el 349 (su periodo).
    pub period: String,
    pub declarant_nif: String,
    /// Registros de tipo 2: perceptores (190, 193, 180), declarados (347) u operadores (349).
    pub records: u32,
    /// Importe total: percepciones (190), base de retenciones (193, 180), operaciones (347,
    /// 349).
    pub total: Money,
    /// Retenciones e ingresos a cuenta (190, 193, 180).
    pub withheld: Option<Money>,
}

/// Importe sin signo de una parte entera y una decimal.
fn unsigned_amount(r: &PageReader<'_>, int: u16, dec: u16) -> Option<Money> {
    let euros: i64 = r.raw(int).ok()?.trim().parse().ok()?;
    let cents: i64 = r.raw(dec).ok()?.trim().parse().ok()?;
    Some(Money::from_cents(euros * 100 + cents))
}

/// Totales del registro de declarante (tipo 1) del fichero de una informativa. Campos de
/// `fixtures/aeat/dr/{190,193,180}.json` (hoja «-1»): 190, n.º 13 perceptores, 14-16
/// percepciones con signo y 17-18 retenciones; 193, n.º 13, 14-15 base y 16-17 retenciones; 180,
/// n.º 13, 14-16 base con signo y 17-18 retenciones. El 347 y el 349, con [`read_file`].
pub fn read_summary(text: &str) -> Option<Summary> {
    let recs = records_500(text);
    let first = recs.first()?;
    if !first.starts_with('1') {
        return None;
    }
    let model: String = first.chars().skip(1).take(3).collect();
    if matches!(model.as_str(), "347" | "349") {
        let f = read_file(text)?;
        let records = u32::try_from(f.lines.len()).unwrap_or(u32::MAX);
        return Some(Summary {
            model: f.model,
            year: f.year,
            period: f.period,
            declarant_nif: f.declarant_nif,
            records,
            total: f.total,
            withheld: None,
        });
    }
    if !matches!(model.as_str(), "190" | "193" | "180") {
        return None;
    }
    let head = PageReader::new(spec(&model).page(&format!("{model}-1")).ok()?, first).ok()?;
    let year: i32 = head.raw(3).ok()?.parse().ok()?;
    let declarant_nif = head.raw(4).ok()?.trim().to_owned();
    let records: u32 = head.raw(13).ok()?.trim().parse().ok()?;
    let (total, withheld) = match model.as_str() {
        "193" => (
            unsigned_amount(&head, 14, 15)?,
            unsigned_amount(&head, 16, 17)?,
        ),
        // 190 y 180: total con signo y retenciones.
        _ => (
            signed_amount(&head, 14, 15, 16)?,
            unsigned_amount(&head, 17, 18)?,
        ),
    };
    Some(Summary {
        model,
        year,
        period: "0A".into(),
        declarant_nif,
        records,
        total,
        withheld: Some(withheld),
    })
}

fn signed_amount(r: &PageReader<'_>, sign: u16, int: u16, dec: u16) -> Option<Money> {
    let euros: i64 = r.raw(int).ok()?.trim().parse().ok()?;
    let cents: i64 = r.raw(dec).ok()?.trim().parse().ok()?;
    let m = Money::from_cents(euros * 100 + cents);
    Some(if r.raw(sign).ok()?.trim() == "N" {
        -m
    } else {
        m
    })
}

// ---------- 190 ----------

/// Clave y subclave de percepción del 190 que genera TaxPain (DR 190, págs. 10-27).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Key190 {
    /// G: actividades profesionales, con su subclave: 01 tipo general del 15 % (art. 95.1
    /// RIRPF); 02 el 7 % de recaudadores municipales, mediadores de seguros y delegados de
    /// Loterías (art. 95.1, 2.º párrafo); 03 el 7 % de los tres primeros años de actividad
    /// (art. 95.1, 3.º párrafo); 04 el 7 % de quien tuvo menos de 15.000 € de rendimientos el
    /// año anterior (art. 101.5.a LIRPF).
    G(u8),
    /// H: actividades económicas, con su subclave: 01 agrícolas y ganaderas (2 %), 02 engorde
    /// de porcino y avicultura (1 %), 03 forestales (2 %), 04 en estimación objetiva (1 %)
    /// (art. 95.4 a 95.6 RIRPF).
    H(u8),
    /// A: empleados por cuenta ajena (con los datos de `Payment190::employee`).
    A,
    /// E: consejeros y administradores, con su subclave (ver [`director_subkey`]).
    E(u8),
    /// L.01: dietas y asignaciones para gastos de viaje exceptuadas de gravamen (art. 9
    /// RIRPF). Sin retención.
    L01,
}

impl Default for Key190 {
    /// G.01: el tipo general de los profesionales.
    fn default() -> Self {
        Key190::G(1)
    }
}

impl Key190 {
    /// (clave, subclave) como van en las posiciones 78 y 79-80.
    pub fn codes(self) -> (&'static str, String) {
        match self {
            Key190::G(s) => ("G", format!("{s:02}")),
            Key190::H(s) => ("H", format!("{s:02}")),
            Key190::A => ("A", "00".into()),
            Key190::E(s) => ("E", format!("{s:02}")),
            Key190::L01 => ("L", "01".into()),
        }
    }

    /// Clave de un proveedor tal como se guarda en sus condiciones («G01»…«G04», «H01»…«H04»).
    pub fn from_code(code: &str) -> Option<Key190> {
        let c = code.trim().to_ascii_uppercase();
        let (k, sub) = c.split_at(c.len().min(1));
        let sub: u8 = sub.parse().ok().filter(|s| (1..=4).contains(s))?;
        match k {
            "G" => Some(Key190::G(sub)),
            "H" => Some(Key190::H(sub)),
            _ => None,
        }
    }

    /// Los códigos que admite [`Key190::from_code`], en orden.
    pub const SUPPLIER_CODES: [&'static str; 8] =
        ["G01", "G02", "G03", "G04", "H01", "H02", "H03", "H04"];

    /// «clave A», «clave E.03», «clave L.01»…
    pub fn label(self) -> String {
        match self.codes() {
            (k, s) if s == "00" => format!("clave {k}"),
            (k, s) => format!("clave {k}.{s}"),
        }
    }
}

/// Régimen de Seguridad Social de un consejero o administrador, que decide su subclave E.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DirectorSs {
    /// Régimen general como «asimilado» a trabajador por cuenta ajena.
    Asimilado,
    /// Régimen especial de trabajadores autónomos (RETA).
    Autonomo,
    /// Sin alta por este cargo (o en otro régimen).
    #[default]
    Ninguno,
}

/// Subclave de la clave E (DR 190, págs. 17-18):
/// - 01: asimilado en el régimen general, salvo que proceda la 02;
/// - 02: asimilado y la entidad tuvo un importe neto de la cifra de negocios inferior a
///   100.000 € en el último periodo impositivo finalizado antes del pago;
/// - 03: autónomo (RETA) y esa misma cifra de negocios inferior a 100.000 €;
/// - 04: el resto (p. ej. autónomo en una entidad de 100.000 € o más, o sin alta).
pub fn director_subkey(ss: DirectorSs, small_entity: bool) -> u8 {
    match (ss, small_entity) {
        (DirectorSs::Asimilado, false) => 1,
        (DirectorSs::Asimilado, true) => 2,
        (DirectorSs::Autonomo, true) => 3,
        _ => 4,
    }
}

/// Tipo de retención de un consejero o administrador (art. 101.2 LIRPF y art. 80.1.3.º RIRPF):
/// 35 %, o 19 % si la entidad tuvo una cifra de negocios inferior a 100.000 € en el último
/// periodo impositivo finalizado antes del pago. Las subclaves 02 y 03 implican el 19 %.
pub fn director_rate(small_entity: bool) -> taxpain_core::Rate {
    taxpain_core::Rate::percent(if small_entity { 19 } else { 35 })
}

/// Una percepción del 190: a un profesional (clave G, subclave 01), a un empleado (clave A, con
/// sus datos adicionales en `employee`), a un administrador (clave E) o dietas exceptuadas
/// (clave L.01).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payment190 {
    pub nif: String,
    pub name: String,
    pub province: String,
    pub gross: Money,
    pub withheld: Money,
    /// Empleado por cuenta ajena (clave A).
    #[serde(default)]
    pub employee: Option<Employee190>,
    /// Clave cuando no es un empleado (con `employee` es siempre la A).
    #[serde(default)]
    pub key: Key190,
    /// Clave E.01/E.02: cotizaciones a la Seguridad Social deducidas por el pagador (gastos
    /// deducibles, pos. 184-196).
    #[serde(default)]
    pub deductible: Money,
}

impl Payment190 {
    /// Clave efectiva: A si hay datos de empleado; si no, la de `key`.
    pub fn effective_key(&self) -> Key190 {
        if self.employee.is_some() {
            Key190::A
        } else {
            self.key
        }
    }
}

/// Datos adicionales de la clave A (registro de perceptor, posiciones 153-254).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Employee190 {
    /// Año de nacimiento (obligatorio).
    pub birth_year: u16,
    /// Situación familiar 1, 2 o 3 (art. 88 RIRPF; obligatoria).
    pub family: u8,
    /// NIF del cónyuge (sólo situación 2).
    pub spouse_nif: Option<String>,
    /// Discapacidad 0-3.
    pub disability: u8,
    /// Contrato o relación 1-4 (1 general, 2 inferior al año, 3 relación especial, 4 jornaleros).
    pub contract: u8,
    /// Movilidad geográfica.
    pub mobility: bool,
    /// Gastos deducibles: cotizaciones del trabajador a la Seguridad Social (art. 19.2.a LIRPF).
    pub deductible_expenses: Money,
    /// Hijos y otros descendientes menores de 3 años y el resto (número total).
    pub children_under3: u8,
    pub children_rest: u8,
    /// Los descendientes se computan por entero (si no, por mitad).
    pub children_whole: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Result190 {
    pub year: i32,
    pub lines: Vec<Payment190>,
    pub total_gross: Money,
    pub total_withheld: Money,
    pub warnings: Vec<String>,
}

pub fn compute_190(year: i32, payments: &[Payment190]) -> Result190 {
    // Un registro por perceptor, clave y subclave.
    let mut acc: BTreeMap<(String, Key190), Payment190> = BTreeMap::new();
    for p in payments {
        let key = p.effective_key();
        acc.entry((p.nif.to_uppercase(), key))
            .and_modify(|l| {
                l.gross += p.gross;
                l.withheld += p.withheld;
                l.deductible += p.deductible;
                if let (Some(a), Some(b)) = (l.employee.as_mut(), p.employee.as_ref()) {
                    a.deductible_expenses += b.deductible_expenses;
                }
            })
            .or_insert_with(|| Payment190 {
                nif: p.nif.to_uppercase(),
                key,
                ..p.clone()
            });
    }
    let lines: Vec<Payment190> = acc.into_values().collect();
    let mut warnings = Vec::new();
    for l in &lines {
        match l.key {
            Key190::E(s) if !(1..=4).contains(&s) => {
                warnings.push(format!("{}: subclave E.{s:02} no válida (01 a 04)", l.name));
            }
            Key190::G(s) | Key190::H(s) if !(1..=4).contains(&s) => {
                let (k, _) = l.key.codes();
                warnings.push(format!(
                    "{}: subclave {k}.{s:02} no válida (01 a 04)",
                    l.name
                ));
            }
            Key190::E(3 | 4) if !l.deductible.is_zero() => warnings.push(format!(
                "{}: en la clave E.03/E.04 no van cotizaciones como gastos deducibles (sólo en \
                 E.01 y E.02, cuando las descuenta la entidad)",
                l.name
            )),
            Key190::L01 if !l.withheld.is_zero() => warnings.push(format!(
                "{}: las dietas exceptuadas (L.01) no llevan retención",
                l.name
            )),
            _ => {}
        }
        if let Some(e) = &l.employee {
            let born = i32::from(e.birth_year);
            if !(1900..=year).contains(&born) {
                warnings.push(format!("{}: falta el año de nacimiento", l.name));
            }
            if !(1..=3).contains(&e.family) {
                warnings.push(format!(
                    "{}: situación familiar no válida (1, 2 o 3)",
                    l.name
                ));
            }
            if !(1..=4).contains(&e.contract) {
                warnings.push(format!("{}: tipo de contrato no válido (1 a 4)", l.name));
            }
            if e.disability > 3 {
                warnings.push(format!(
                    "{}: grado de discapacidad no válido (0 a 3)",
                    l.name
                ));
            }
        }
    }
    Result190 {
        year,
        total_gross: lines.iter().map(|l| l.gross).sum(),
        total_withheld: lines.iter().map(|l| l.withheld).sum(),
        lines,
        warnings,
    }
}

pub fn file_190(r: &Result190, who: &Declarant) -> Result<String, DrError> {
    let s = spec("190");
    let mut w = declarant_record(s, "190", r.year, who, r.lines.len())?;
    signed(&mut w, Some(14), 15, 16, r.total_gross)?;
    signed(&mut w, None, 17, 18, r.total_withheld)?;
    let mut out = vec![w.finish()?];
    for l in &r.lines {
        let mut w = common_third(s, "190-2", r.year, who)?;
        w.text(5, &l.nif)?
            .text(7, &fit(&l.name, 40))?
            .text(8, &l.province)?;
        match &l.employee {
            None => {
                let key = l.effective_key();
                let (k, sub) = key.codes();
                w.text(9, k)?.text(10, &sub)?;
                if let Key190::E(s) = key {
                    // Gastos deducibles sólo en E.01 y E.02 (cotizaciones que descuenta la
                    // entidad); la retención, ingresada en la Hacienda Estatal (pos. 323-335).
                    if matches!(s, 1 | 2) {
                        signed(&mut w, None, 34, 35, l.deductible)?;
                    }
                    signed(&mut w, None, 77, 78, l.withheld)?;
                }
            }
            Some(e) => {
                // Clave A: sin subclave; datos adicionales del perceptor.
                w.text(9, "A")?
                    .text(10, "00")?
                    .text(25, &format!("{:04}", e.birth_year))?
                    .text(26, &e.family.to_string())?
                    .text(28, &e.disability.min(3).to_string())?
                    .text(29, &e.contract.to_string())?
                    .text(31, if e.mobility { "1" } else { "0" })?;
                if e.family == 2
                    && let Some(n) = e.spouse_nif.as_deref().filter(|n| !n.trim().is_empty())
                {
                    w.text(27, &n.trim().to_uppercase())?;
                }
                signed(&mut w, None, 34, 35, e.deductible_expenses)?;
                let whole = |n: u8| if e.children_whole { n } else { 0 };
                w.text(40, &e.children_under3.min(9).to_string())?
                    .text(41, &whole(e.children_under3.min(9)).to_string())?
                    .text(42, &format!("{:02}", e.children_rest.min(99)))?
                    .text(43, &format!("{:02}", whole(e.children_rest.min(99))))?;
                // Cómputo de los tres primeros hijos (pos. 251-253): 1 por entero, 2 por mitad,
                // y ceros para los que no existen (DR 190, Orden EHA/3127/2009).
                let n = u16::from(e.children_under3.min(9) + e.children_rest.min(99)).min(3);
                for i in 0..n {
                    w.text(60 + i, if e.children_whole { "1" } else { "2" })?;
                }
            }
        }
        signed(&mut w, Some(11), 12, 13, l.gross)?;
        signed(&mut w, None, 14, 15, l.withheld)?;
        out.push(w.finish()?);
    }
    Ok(out.join("\r\n"))
}

// ---------- 180 ----------

/// Inmueble arrendado (datos del registro de perceptor).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Property {
    /// 1: con referencia catastral en territorio común; 2: País Vasco; 3: Navarra; 4: sin
    /// referencia catastral (180-notas.md).
    pub situation: u8,
    pub cadastral_ref: String,
    pub street: String,
    pub number: String,
    pub municipality: String,
    pub province_code: String,
    pub postal_code: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rent180 {
    pub nif: String,
    pub name: String,
    pub province: String,
    pub base: Money,
    pub withheld: Money,
    pub property: Option<Property>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Result180 {
    pub year: i32,
    pub lines: Vec<Rent180>,
    pub total_base: Money,
    pub total_withheld: Money,
    pub warnings: Vec<String>,
}

pub fn compute_180(year: i32, rents: &[Rent180]) -> Result180 {
    let mut acc: BTreeMap<(String, String), Rent180> = BTreeMap::new();
    for r in rents {
        let key = (
            r.nif.to_uppercase(),
            r.property
                .as_ref()
                .map(|p| p.cadastral_ref.clone())
                .unwrap_or_default(),
        );
        acc.entry(key)
            .and_modify(|l| {
                l.base += r.base;
                l.withheld += r.withheld;
            })
            .or_insert_with(|| Rent180 {
                nif: r.nif.to_uppercase(),
                ..r.clone()
            });
    }
    let lines: Vec<Rent180> = acc.into_values().collect();
    let warnings = lines
        .iter()
        .filter(|l| l.property.is_none())
        .map(|l| {
            format!(
                "{} ({}): faltan la referencia catastral y la dirección del inmueble",
                l.name, l.nif
            )
        })
        .collect();
    Result180 {
        year,
        total_base: lines.iter().map(|l| l.base).sum(),
        total_withheld: lines.iter().map(|l| l.withheld).sum(),
        lines,
        warnings,
    }
}

pub fn file_180(r: &Result180, who: &Declarant) -> Result<String, DrError> {
    let s = spec("180");
    let mut w = declarant_record(s, "180", r.year, who, r.lines.len())?;
    signed(&mut w, Some(14), 15, 16, r.total_base)?;
    signed(&mut w, None, 17, 18, r.total_withheld)?;
    let mut out = vec![w.finish()?];
    for l in &r.lines {
        let mut w = common_third(s, "180-2", r.year, who)?;
        w.text(5, &l.nif)?
            .text(7, &fit(&l.name, 40))?
            .text(8, &l.province)?
            .text(9, "1")?;
        signed(&mut w, Some(10), 11, 12, l.base)?;
        // % de retención (19 % salvo casos especiales): parte entera y decimal.
        let pct = if l.base.is_zero() {
            0
        } else {
            (i128::from(l.withheld.cents()) * 10_000 / i128::from(l.base.cents())).unsigned_abs()
        };
        // El campo 16 («Ejercicio devengo», pos. 110-113) queda a ceros: sólo se rellena con
        // atrasos de ejercicios anteriores o reintegros (DR 180, Orden HAP/1732/2014), y TaxPain
        // declara las rentas del propio ejercicio.
        w.text(13, &(pct / 100).to_string())?
            .text(14, &format!("{:02}", pct % 100))?
            .amount(15, l.withheld.abs())?;
        if let Some(p) = &l.property {
            w.text(17, &p.situation.to_string())?
                .text(18, &p.cadastral_ref)?
                .text(20, &fit(&p.street, 50))?
                .text(22, &fit(&p.number, 5))?
                .text(31, &fit(&p.municipality, 30))?
                .text(33, &p.province_code)?
                .text(34, &p.postal_code)?;
        }
        out.push(w.finish()?);
    }
    Ok(out.join("\r\n"))
}

// ---------- 193 ----------

/// Un dividendo pagado a un socio residente (clave de percepción A).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dividend193 {
    pub nif: String,
    pub name: String,
    /// Código de provincia (2 cifras) del domicilio del perceptor.
    pub province: String,
    /// Fecha de devengo: la de pago fijada en el acuerdo de reparto (pos. 331-338).
    pub accrual: time::Date,
    pub gross: Money,
    /// Retención practicada; cero si el dividendo está excluido de retención (art. 128.4.d LIS:
    /// sociedad con derecho a la exención del art. 21 LIS).
    pub withheld: Money,
    /// Tipo aplicado (19 % o 0).
    pub rate: taxpain_core::Rate,
}

/// Intereses de un préstamo recibido pagados a un prestamista residente que no es entidad de
/// crédito, con su retención (clave de percepción B, naturaleza 03 «intereses de préstamos no
/// bancarios»: DR 193, pág. 19).
///
/// `TODO(VERIFY)`: la clave D (cesión de capitales a una entidad vinculada por una persona
/// física, por lo que exceda de tres veces su participación en los fondos propios: art. 46.a
/// LIRPF) no se calcula: todo va con la B y la app lo avisa.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interest193 {
    pub nif: String,
    pub name: String,
    pub province: String,
    pub gross: Money,
    pub withheld: Money,
    pub rate: taxpain_core::Rate,
}

fn clave_a() -> char {
    'A'
}

/// Un registro de perceptor del 193: un perceptor, una clave y naturaleza y (en la clave A) una
/// fecha de devengo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line193 {
    pub nif: String,
    pub name: String,
    pub province: String,
    /// Clave de percepción: A (dividendos) o B (intereses de préstamos).
    #[serde(default = "clave_a")]
    pub key: char,
    /// Sólo cuenta en la clave A (en la B es el 1 de enero del ejercicio y no se escribe).
    pub accrual: time::Date,
    /// Clave A: 02 dividendos con retención, 08 dividendos no sometidos a retención. Clave B:
    /// 03 intereses de préstamos no bancarios.
    pub nature: u8,
    pub gross: Money,
    pub base: Money,
    pub rate: taxpain_core::Rate,
    pub withheld: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Result193 {
    pub year: i32,
    pub lines: Vec<Line193>,
    pub total_base: Money,
    pub total_withheld: Money,
    pub warnings: Vec<String>,
}

/// Naturaleza de la clave A: 02 «dividendos y participaciones en beneficios» o 08
/// «dividendos no sometidos a retención ni ingreso a cuenta» (DR 193, pág. 15).
fn nature_193(d: &Dividend193) -> u8 {
    if d.withheld.is_zero() { 8 } else { 2 }
}

pub fn compute_193(year: i32, dividends: &[Dividend193]) -> Result193 {
    compute_193_with_interest(year, dividends, &[])
}

/// [`compute_193`] con los intereses de préstamos recibidos (clave B, naturaleza 03): un
/// registro por prestamista; sólo los que llevan retención.
pub fn compute_193_with_interest(
    year: i32,
    dividends: &[Dividend193],
    interest: &[Interest193],
) -> Result193 {
    // Un registro por socio, naturaleza y fecha de devengo (la fecha va en el registro).
    let mut acc: BTreeMap<(String, char, u8, time::Date), Line193> = BTreeMap::new();
    let jan1 =
        time::Date::from_calendar_date(year, time::Month::January, 1).unwrap_or(time::Date::MIN);
    for i in interest.iter().filter(|i| !i.withheld.is_zero()) {
        acc.entry((i.nif.to_uppercase(), 'B', 3, jan1))
            .and_modify(|l| {
                l.gross += i.gross;
                l.base += i.gross;
                l.withheld += i.withheld;
            })
            .or_insert_with(|| Line193 {
                nif: i.nif.to_uppercase(),
                name: i.name.clone(),
                province: i.province.clone(),
                key: 'B',
                accrual: jan1,
                nature: 3,
                gross: i.gross,
                base: i.gross,
                rate: i.rate,
                withheld: i.withheld,
            });
    }
    for d in dividends {
        let nature = nature_193(d);
        let base = if nature == 2 { d.gross } else { Money::ZERO };
        acc.entry((d.nif.to_uppercase(), 'A', nature, d.accrual))
            .and_modify(|l| {
                l.gross += d.gross;
                l.base += base;
                l.withheld += d.withheld;
            })
            .or_insert_with(|| Line193 {
                nif: d.nif.to_uppercase(),
                name: d.name.clone(),
                province: d.province.clone(),
                key: 'A',
                accrual: d.accrual,
                nature,
                gross: d.gross,
                base,
                rate: if nature == 2 {
                    d.rate
                } else {
                    taxpain_core::Rate::ZERO
                },
                withheld: d.withheld,
            });
    }
    let lines: Vec<Line193> = acc.into_values().collect();
    let mut warnings = Vec::new();
    for l in &lines {
        let prov_ok = l.province.len() == 2
            && l.province
                .parse::<u8>()
                .is_ok_and(|n| (1..=53).contains(&n));
        if !prov_ok {
            warnings.push(format!(
                "{} ({}): falta la provincia del domicilio (01 a 53)",
                l.name, l.nif
            ));
        }
        if l.key == 'A' && l.accrual.year() != year {
            warnings.push(format!(
                "{} ({}): dividendo con devengo fuera de {year}",
                l.name, l.nif
            ));
        }
    }
    Result193 {
        year,
        total_base: lines.iter().map(|l| l.base).sum(),
        total_withheld: lines.iter().map(|l| l.withheld).sum(),
        lines,
        warnings,
    }
}

pub fn file_193(r: &Result193, who: &Declarant) -> Result<String, DrError> {
    let s = spec("193");
    let mut w = declarant_record(s, "193", r.year, who, r.lines.len())?;
    signed(&mut w, None, 14, 15, r.total_base)?;
    signed(&mut w, None, 16, 17, r.total_withheld)?;
    // Sociedad que paga directamente a sus socios: no es depositaria ni mediadora, ni emisora
    // con los valores depositados (categorías a-d, pág. 8). Con «S», las retenciones
    // «ingresadas» (175-189) y los gastos (220-234) van a ceros.
    w.text(23, "S")?;
    let mut out = vec![w.finish()?];
    for (i, l) in r.lines.iter().enumerate() {
        let mut w = common_third(s, "193-2", r.year, who)?;
        w.text(5, &l.nif)?
            .text(7, &fit(&l.name, 40))?
            .text(9, &l.province)?
            .text(12, &l.key.to_string())?
            .text(13, &format!("{:02}", l.nature))?
            .text(19, "1")?;
        signed(&mut w, None, 20, 21, l.gross)?;
        signed(&mut w, None, 25, 26, l.base)?;
        let bp = l.rate.bp().unsigned_abs();
        w.text(27, &format!("{:02}", (bp / 100).min(99)))?
            .text(28, &format!("{:02}", bp % 100))?;
        signed(&mut w, None, 29, 30, l.withheld)?;
        w.text(35, "S")?
            .text(52, "0")?
            .text(53, &format!("{:07}", i + 1))?
            // Participaciones de una SL o un préstamo: no cotizan en ningún mercado.
            // TODO(VERIFY): clave de mercado «D» también en la clave B (préstamo no negociado).
            .text(58, "D")?;
        // La fecha de devengo sólo se rellena en la clave A (pág. 46).
        if l.key == 'A' {
            w.text(55, &format!("{:02}", l.accrual.day()))?
                .text(56, &format!("{:02}", u8::from(l.accrual.month())))?
                .text(57, &format!("{:04}", l.accrual.year()))?;
        }
        out.push(w.finish()?);
    }
    Ok(out.join("\r\n"))
}
