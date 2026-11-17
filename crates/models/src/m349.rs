//! Modelo 349: declaración recapitulativa de operaciones intracomunitarias.
//!
//! Base legal: art. 164.Uno.5.º LIVA; arts. 78 a 81 RIVA (RD 1624/1992); Orden HAC/174/2020
//! (modelo y diseño). Diseño: `DR_Anexo_349.pdf`, transcrito a `fixtures/aeat/dr/349.json`.
//!
//! Registros de 500 posiciones en ISO-8859-1: uno de declarante (tipo 1) y uno de tipo 2 por
//! operador y clave de operación, acumulando importes del periodo (art. 80 RIVA).
//!
//! Alcance: claves E y S (entregas y servicios, desde las emitidas), A e I (adquisiciones de
//! bienes y servicios, desde las recibidas con autorrepercusión) y registros de rectificación de
//! periodos anteriores. Triangulares (T), importaciones exentas (M, H) y ventas en consigna
//! (R, D, C) aún no se registran en TaxPain.
//!
//! `TODO(VERIFY)`:
//! - Número identificativo de la declaración (108-120): lo asigna el formulario de la AEAT; aquí
//!   se genera «349» + ejercicio + periodo + secuencia.
//! - Importe de las rectificaciones del registro de declarante (171-185): se pone la suma de
//!   las bases rectificadas (las nuevas), no la de las diferencias.
//! - La base «declarada anteriormente» la calcula la app con los datos del periodo original;
//!   si aquel 349 se presentó con otros importes, hay que corregirla a mano.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::Money;

use crate::Declarant;
use crate::dr::{DrError, PageWriter, Spec, alnum};

pub const DR_JSON: &str = include_str!("../../../fixtures/aeat/dr/349.json");

pub fn spec() -> &'static Spec {
    static SPEC: std::sync::OnceLock<Spec> = std::sync::OnceLock::new();
    SPEC.get_or_init(|| Spec::parse(DR_JSON))
}

/// Clave de operación (posición 133).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Key {
    /// E: entregas intracomunitarias exentas.
    E,
    /// S: prestaciones intracomunitarias de servicios.
    S,
    /// A: adquisiciones intracomunitarias sujetas.
    A,
    /// I: adquisiciones intracomunitarias de servicios.
    I,
}

impl Key {
    pub fn code(self) -> &'static str {
        match self {
            Key::E => "E",
            Key::S => "S",
            Key::A => "A",
            Key::I => "I",
        }
    }
}

/// Una operación con un operador intracomunitario.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    /// Código del Estado miembro tal como lo usa el VIES (Grecia = EL).
    pub country: String,
    /// NIF-IVA sin el prefijo del país.
    pub vat_id: String,
    pub name: String,
    pub key: Key,
    /// Base imponible; negativa si es una rectificación del mismo periodo.
    pub base: Money,
}

/// Registro acumulado por operador y clave.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorLine {
    pub country: String,
    pub vat_id: String,
    pub name: String,
    pub key: Key,
    pub base: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Result349 {
    pub year: i32,
    /// "1T".."4T" o "01".."12".
    pub period: String,
    pub lines: Vec<OperatorLine>,
    pub total: Money,
    /// Rectificaciones de periodos anteriores (registro de rectificación, art. 81 RIVA).
    #[serde(default)]
    pub rectifications: Vec<RectificationLine>,
    pub warnings: Vec<String>,
}

/// Rectificación de lo declarado para un operador y clave en un periodo anterior.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rectification {
    pub country: String,
    pub vat_id: String,
    pub name: String,
    pub key: Key,
    /// Ejercicio y periodo rectificados.
    pub year: i32,
    pub period: String,
    /// Importe de la corrección (negativo si reduce).
    pub delta: Money,
    /// Base declarada en aquel periodo para ese operador y clave.
    pub previous: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RectificationLine {
    pub country: String,
    pub vat_id: String,
    pub name: String,
    pub key: Key,
    pub year: i32,
    pub period: String,
    /// Base rectificada (la correcta tras la corrección).
    pub rectified: Money,
    pub previous: Money,
}

/// Añade las rectificaciones de periodos anteriores, acumuladas por operador, clave y periodo.
pub fn add_rectifications(r: &mut Result349, rects: &[Rectification]) {
    let mut acc: BTreeMap<(String, String, Key, i32, String), RectificationLine> = BTreeMap::new();
    for x in rects {
        let (country, vat_id) = split_vat_id(&x.country, &x.vat_id);
        if let Some(w) = not_member_state(&x.name, &country, x.key) {
            r.warnings.push(w);
            continue;
        }
        if vat_id.is_empty() || vat_id.len() > 15 {
            r.warnings
                .push(format!("{}: NIF-IVA {country}{vat_id} no válido", x.name));
            continue;
        }
        acc.entry((
            country.clone(),
            vat_id.clone(),
            x.key,
            x.year,
            x.period.clone(),
        ))
        .and_modify(|l| l.rectified += x.delta)
        .or_insert(RectificationLine {
            country,
            vat_id,
            name: x.name.clone(),
            key: x.key,
            year: x.year,
            period: x.period.clone(),
            rectified: x.previous + x.delta,
            previous: x.previous,
        });
    }
    for l in acc.into_values() {
        if l.rectified.cents() < 0 {
            r.warnings.push(format!(
                "{} ({}{}): la rectificación del {} {} deja la base en negativo; revísala",
                l.name, l.country, l.vat_id, l.period, l.year
            ));
            continue;
        }
        r.rectifications.push(l);
    }
}

/// Normaliza el NIF-IVA: mayúsculas, sin espacios ni puntos y sin el prefijo del país.
pub fn split_vat_id(country: &str, id: &str) -> (String, String) {
    let c = match country.to_uppercase().as_str() {
        "GR" => "EL".to_owned(),
        c => c.to_owned(),
    };
    let clean: String = id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect::<String>()
        .to_uppercase();
    let number = clean.strip_prefix(&c).map_or(clean.clone(), str::to_owned);
    (c, number)
}

/// Aviso si `country` no es otro Estado miembro de la UE para la clave: el 349 sólo recoge
/// operaciones con operadores de otros Estados miembros (art. 79 RIVA). Irlanda del Norte (XI)
/// sólo vale para bienes (claves E y A).
fn not_member_state(name: &str, country: &str, key: Key) -> Option<String> {
    let goods = matches!(key, Key::E | Key::A);
    if country == "ES" {
        Some(format!("{name}: un NIF español no va en el 349"))
    } else if !taxpain_core::eu::is_other_member_state(country, goods) {
        Some(format!(
            "{name}: «{country}» no es otro Estado miembro de la UE{}: no va en el 349",
            if country == "XI" {
                " para servicios"
            } else {
                ""
            }
        ))
    } else {
        None
    }
}

pub fn compute(year: i32, period: &str, ops: &[Operation]) -> Result349 {
    let mut acc: BTreeMap<(String, String, Key), OperatorLine> = BTreeMap::new();
    let mut warnings = Vec::new();
    for o in ops {
        let (country, vat_id) = split_vat_id(&o.country, &o.vat_id);
        if let Some(w) = not_member_state(&o.name, &country, o.key) {
            warnings.push(w);
            continue;
        }
        if vat_id.is_empty() || vat_id.len() > 15 {
            warnings.push(format!("{}: NIF-IVA {vat_id} no válido", o.name));
            continue;
        }
        acc.entry((country.clone(), vat_id.clone(), o.key))
            .and_modify(|l| l.base += o.base)
            .or_insert(OperatorLine {
                country,
                vat_id,
                name: o.name.clone(),
                key: o.key,
                base: o.base,
            });
    }
    let mut lines: Vec<OperatorLine> = acc.into_values().collect();
    lines.retain(|l| {
        if l.base.cents() < 0 {
            warnings.push(format!(
                "{} ({}{}): base negativa en el periodo; las rectificaciones de periodos anteriores se declaran en un registro de rectificación (aún no soportado)",
                l.name, l.country, l.vat_id
            ));
            false
        } else {
            !l.base.is_zero()
        }
    });
    let total = lines.iter().map(|l| l.base).sum();
    Result349 {
        year,
        period: period.to_owned(),
        lines,
        total,
        rectifications: Vec::new(),
        warnings,
    }
}

/// Fichero del 349: registro de declarante + registros de operador, sin separadores.
/// `TODO(VERIFY)`: si los registros van separados por fin de línea (CRLF) en la presentación
/// mediante fichero; aquí se separan con CRLF, como en el resto de informativas.
pub fn file(r: &Result349, who: &Declarant, sequence: u32) -> Result<String, DrError> {
    let s = spec();
    let year = format!("{:04}", r.year);
    let nif = format!("{:0>9}", who.nif.to_uppercase());
    // Persona física: apellidos y nombre, en ese orden (ya es el orden de `full_name`).
    let name = fit(&alnum(&who.full_name()), 40);
    let mut out = String::new();

    let mut w = PageWriter::new(s.page("349-1")?);
    w.text(3, &year)?
        .text(4, &nif)?
        .text(5, &name)?
        .text(8, &name)?
        .text(
            9,
            &format!(
                "349{year}{:0>2}{:04}",
                period_digits(&r.period),
                sequence % 10_000
            ),
        )?
        .text(13, &r.period)?
        .text(14, &r.lines.len().to_string())?
        .amount(15, r.total)?;
    if !r.rectifications.is_empty() {
        // Número de registros de rectificación (posiciones 162-170).
        w.text(16, &r.rectifications.len().to_string())?
            .amount(17, r.rectifications.iter().map(|l| l.rectified).sum())?;
    }
    out.push_str(&w.finish()?);

    for l in &r.lines {
        out.push_str("\r\n");
        let mut w = PageWriter::new(s.page("349-2op")?);
        w.text(3, &year)?
            .text(4, &nif)?
            .text(6, &l.country)?
            .text(7, &l.vat_id)?
            .text(8, &fit(&alnum(&l.name), 40))?
            .text(9, l.key.code())?
            .amount(10, l.base)?;
        out.push_str(&w.finish()?);
    }
    for l in &r.rectifications {
        out.push_str("\r\n");
        let mut w = PageWriter::new(s.page("349-2rect")?);
        w.text(3, &year)?
            .text(4, &nif)?
            .text(6, &l.country)?
            .text(7, &l.vat_id)?
            .text(8, &fit(&alnum(&l.name), 40))?
            .text(9, l.key.code())?
            .text(11, &format!("{:04}", l.year))?
            .text(12, &l.period)?
            .amount(13, l.rectified)?
            .amount(14, l.previous)?;
        out.push_str(&w.finish()?);
    }
    Ok(out)
}

/// Recorta a `n` caracteres (los nombres largos no caben en el registro).
fn fit(s: &str, n: usize) -> String {
    s.chars().take(n).collect::<String>().trim_end().to_owned()
}

/// "1T" → "01" … "4T" → "04"; "07" → "07" (sólo para el número identificativo).
fn period_digits(p: &str) -> String {
    p.strip_suffix('T')
        .map_or_else(|| p.to_owned(), |q| format!("{q:0>2}"))
}
