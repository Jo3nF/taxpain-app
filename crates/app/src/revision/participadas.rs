//! Dividendos cobrados de las participadas, confirmados con sus documentos: el acta de reparto de
//! la participada (con lo que le corresponde a la empresa), su 123 o su 193, el certificado de
//! retenciones o la carta de la transferencia. Con lo cobrado en el banco (el neto), un documento
//! de la misma participada que dice el bruto (lo cobrado entre 0,81, con la retención del 19 %) o
//! un acta o certificado que da como bruto lo mismo que se cobró (sin retención) confirma el
//! bruto y la retención.
//!
//! La participada se reconoce por el concepto del banco («TRANSFERENCIA DE PARTICIPADA UNO SL ·
//! DIVIDENDO») y por el nombre de sus documentos o de su carpeta (`…/PARTICIPADAS/PARTICIPADA UNO
//! SL/…`). El porcentaje de la empresa en ella sale de lo que dicen esos documentos junto al
//! nombre o el NIF de la empresa («EMPRESA DEMO SL… que representa un 50 % del capital», «titular del 50
//! %», «participada … al 50 %»).

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;
use taxpain_core::money::Money;

use super::cruces::datos::MovBanco;
use super::expediente::{Doc, Expediente};
use crate::documentos::fold;

/// Lo que se ha leído de un documento que puede ser de una participada (está en una carpeta de
/// participadas o habla de dividendos): su texto (la ruta, la descripción de la lectura y el
/// texto del PDF), en minúsculas y sin tildes.
pub(crate) fn completar(d: &mut Doc, bytes: &[u8]) {
    let base = fold(&format!("{} {}", d.ruta, d.descripcion));
    if !base.contains("dividend") && !base.contains("participad") {
        return;
    }
    let pdf = if bytes.starts_with(b"%PDF") {
        crate::pdf_text::text(bytes)
    } else if d.ruta.to_lowercase().ends_with(".txt") {
        std::str::from_utf8(bytes).ok().map(str::to_owned)
    } else {
        None
    };
    let texto = fold(&format!("{} {}", base, pdf.as_deref().unwrap_or_default()));
    d.participada = Some(texto);
}

/// Un dividendo cobrado en el banco con el bruto y la retención que dicen los documentos de la
/// participada que lo paga.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Confirmado {
    /// La participada (el nombre de su carpeta o, si no, el concepto del banco).
    pub pagadora: String,
    pub neto: Money,
    pub bruto: Money,
    pub retencion: Money,
    /// Los documentos que lo justifican (los que dicen el bruto o la retención).
    pub ficheros: Vec<String>,
    /// El porcentaje de la empresa en la participada, en centésimas, si lo dicen los documentos.
    pub porcentaje: Option<i64>,
    /// El primer año de los documentos que dicen ese porcentaje (para el año de tenencia).
    pub desde: Option<i32>,
}

impl Confirmado {
    /// ¿Dicen los documentos que la empresa tiene al menos el 5 %, y desde antes del año del
    /// cobro? (El año exacto de tenencia del art. 21.1 LIS no se puede comprobar con el día.)
    pub fn grupo(&self, y: i32) -> bool {
        self.porcentaje.is_some_and(|p| p >= 500) && self.desde.is_some_and(|d| d < y)
    }

    /// «50 %» / «12,5 %».
    pub fn porcentaje_texto(&self) -> Option<String> {
        self.porcentaje.map(|p| {
            if p % 100 == 0 {
                format!("{} %", p / 100)
            } else {
                format!(
                    "{},{} %",
                    p / 100,
                    format!("{:02}", p % 100).trim_end_matches('0')
                )
            }
        })
    }
}

/// Palabras del concepto del banco que no dicen quién paga.
const GENERICAS: &[&str] = &[
    "transferencia",
    "transferencias",
    "transf",
    "dividendo",
    "dividendos",
    "abono",
    "cobro",
    "recibida",
    "ingreso",
    "participada",
    "participadas",
    "sociedad",
    "limitada",
    "pago",
    "favor",
    "orden",
    "nomina",
    "concepto",
    "referencia",
];

fn palabras(s: &str) -> BTreeSet<String> {
    fold(s)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 4 && !w.chars().all(|c| c.is_ascii_digit()))
        .filter(|w| !GENERICAS.contains(w))
        .map(str::to_owned)
        .collect()
}

/// La participada de un documento: la carpeta que va tras la de las participadas
/// (`…/PARTICIPADAS/PARTICIPADA UNO SL/…`) o, si no está en una, el propio documento.
fn grupo_de(ruta: &str) -> String {
    let partes: Vec<&str> = ruta.split('/').collect();
    partes
        .iter()
        .position(|p| fold(p).contains("participad"))
        .and_then(|i| partes.get(i + 1).filter(|_| i + 2 < partes.len()))
        .map_or_else(|| ruta.to_owned(), |p| (*p).to_owned())
}

/// Importes del texto, en céntimos («12.345,68», «***10.000,00»).
fn importes(texto: &str) -> BTreeSet<i64> {
    static IMPORTE: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"(?:\d{1,3}(?:\.\d{3})+|\d+),\d{2}\b").ok());
    let Some(re) = IMPORTE.as_ref() else {
        return BTreeSet::new();
    };
    re.find_iter(texto)
        .filter_map(|m| m.as_str().replace(['.', ','], "").parse::<i64>().ok())
        .collect()
}

/// El porcentaje de la empresa en la participada: el primero que sale tras su nombre o su NIF
/// (en las 200 letras siguientes), sin los de la retención («retención del 19 %», «menos el 19
/// %»). En centésimas; el mayor si hay varios.
fn porcentaje(texto: &str, claves: &[String]) -> Option<i64> {
    static PCT: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"(\d{1,3})(?:[.,](\d{1,2}))?\s?%").ok());
    let re = PCT.as_ref()?;
    let mut mejor: Option<i64> = None;
    for clave in claves.iter().filter(|c| !c.is_empty()) {
        for (i, _) in texto.match_indices(clave.as_str()) {
            let fin = (i + clave.len() + 200).min(texto.len());
            let Some(ventana) = texto.get(i..fin).or_else(|| texto.get(i..)) else {
                continue;
            };
            let Some(c) = re.captures_iter(ventana).find(|c| {
                let m = c.get(0).map_or(0..0, |m| m.range());
                let antes = ventana
                    .get(m.start.saturating_sub(25)..m.start)
                    .unwrap_or("");
                let despues = ventana
                    .get(m.end..(m.end + 25).min(ventana.len()))
                    .unwrap_or("");
                !antes.contains("retenc") && !antes.contains("menos") && !despues.contains("retenc")
            }) else {
                continue;
            };
            let entero: i64 = c.get(1).and_then(|m| m.as_str().parse().ok()).unwrap_or(0);
            let dec: i64 = c
                .get(2)
                .map(|m| {
                    let s = format!("{:0<2}", m.as_str());
                    s.parse().unwrap_or(0)
                })
                .unwrap_or(0);
            let p = entero * 100 + dec;
            if p <= 10_000 {
                mejor = Some(mejor.map_or(p, |x| x.max(p)));
            }
        }
    }
    mejor
}

/// El año de un documento: el primero que dice su nombre o, si no lo dice, el primero de su
/// texto con día («30/06/2022»).
fn año_de(ruta: &str, texto: &str) -> Option<i32> {
    static AÑO: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"(?:^|[^0-9])(20\d{2})(?:[^0-9]|$)").ok());
    static FECHA: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"\b\d{1,2}/\d{1,2}/(20\d{2})\b").ok());
    let nombre = ruta.rsplit('/').next().unwrap_or(ruta);
    let de = |re: &LazyLock<Option<Regex>>, s: &str| {
        re.as_ref()?
            .captures(s)
            .and_then(|c| c.get(1)?.as_str().parse().ok())
    };
    de(&AÑO, nombre).or_else(|| de(&FECHA, texto))
}

/// Lo cobrado entre 0,81 (el bruto con la retención del 19 %, art. 128.6.a LIS), al céntimo.
pub(crate) fn bruto_con_retencion(neto: Money) -> Money {
    let n = i128::from(neto.cents()) * 100;
    let (q, r) = (n / 81, n % 81);
    let q = if 2 * r >= 81 { q + 1 } else { q };
    Money::from_cents(i64::try_from(q).unwrap_or(neto.cents()))
}

/// ¿Es un acta, un certificado o un modelo informativo, que dicen el bruto? (No la carta de la
/// transferencia, que sólo dice lo que se paga.)
fn dice_el_bruto(texto: &str) -> bool {
    [
        "bruto",
        "acta",
        "junta",
        "certifica",
        "modelo 193",
        "modelo193",
        "decision",
    ]
    .iter()
    .any(|p| texto.contains(p))
}

/// Confirma cada cobro con los documentos de las participadas, si los hay. Cada documento sirve
/// para un cobro: dos cobros iguales de dos participadas van cada uno con los suyos.
pub(crate) fn confirmar(exp: &Expediente, cobros: &[&MovBanco]) -> Vec<Option<Confirmado>> {
    let mut grupos: BTreeMap<String, Vec<&Doc>> = BTreeMap::new();
    for d in exp.docs.iter().filter(|d| d.participada.is_some()) {
        grupos.entry(grupo_de(&d.ruta)).or_default().push(d);
    }
    let mut claves: Vec<String> = Vec::new();
    if let Some(n) = exp.own_nif.as_deref() {
        claves.push(fold(n));
    }
    // La primera palabra del nombre de la empresa que no es la forma jurídica.
    if let Some(w) = fold(&exp.empresa)
        .split(|c: char| !c.is_alphanumeric())
        .find(|w| w.chars().count() >= 4 && !GENERICAS.contains(w) && *w != "anonima")
    {
        claves.push(w.to_owned());
    }
    let mut usados: BTreeSet<(String, i64)> = BTreeSet::new();
    cobros
        .iter()
        .map(|m| {
            let c = confirmar_uno(m, &grupos, &claves, &usados)?;
            usados.insert((c.pagadora.clone(), m.importe.cents()));
            Some(c)
        })
        .collect()
}

/// El texto de un documento de una participada.
fn texto(d: &Doc) -> &str {
    d.participada.as_deref().unwrap_or_default()
}

fn confirmar_uno(
    m: &MovBanco,
    grupos: &BTreeMap<String, Vec<&Doc>>,
    claves: &[String],
    usados: &BTreeSet<(String, i64)>,
) -> Option<Confirmado> {
    let neto = m.importe;
    let bruto19 = bruto_con_retencion(neto);
    let del_concepto = palabras(&m.concepto);
    // Cada participada que tiene un documento con el bruto (o, sin retención, un acta o un
    // certificado con lo cobrado), con lo que se parece al concepto del banco.
    let mut candidatas: Vec<(usize, &String, Money)> = Vec::new();
    for (g, docs) in grupos {
        if usados.contains(&(g.clone(), neto.cents())) {
            continue;
        }
        let en_nombre = palabras(g);
        let parecido = del_concepto
            .iter()
            .map(|w| {
                if en_nombre.contains(w) {
                    10
                } else if docs.iter().any(|d| texto(d).contains(w.as_str())) {
                    1
                } else {
                    0
                }
            })
            .sum::<usize>();
        if !del_concepto.is_empty() && parecido == 0 {
            continue;
        }
        let con = |cents: i64, solo_bruto: bool| {
            docs.iter().any(|d| {
                let t = texto(d);
                t.contains("dividend")
                    && (!solo_bruto || dice_el_bruto(t))
                    && importes(t).contains(&cents)
            })
        };
        if con(bruto19.cents(), false) {
            candidatas.push((parecido, g, bruto19));
        } else if con(neto.cents(), true) {
            candidatas.push((parecido, g, neto));
        }
    }
    let max = candidatas.iter().map(|c| c.0).max()?;
    let mejores: Vec<_> = candidatas.into_iter().filter(|c| c.0 == max).collect();
    // Sin nada en el concepto que diga quién paga, sólo si es una participada.
    let [(_, g, bruto)] = mejores.as_slice() else {
        return None;
    };
    let docs = grupos.get(*g)?;
    let retencion = *bruto - neto;
    let mut ficheros: Vec<String> = docs
        .iter()
        .filter(|d| {
            let i = importes(texto(d));
            texto(d).contains("dividend")
                && (i.contains(&bruto.cents())
                    || (retencion.cents() > 0 && i.contains(&retencion.cents())))
        })
        .map(|d| d.ruta.clone())
        .collect();
    ficheros.sort();
    let mut porcentaje_max: Option<i64> = None;
    let mut desde: Option<i32> = None;
    for d in docs {
        if let Some(p) = porcentaje(texto(d), claves)
            && p >= 500
        {
            porcentaje_max = Some(porcentaje_max.map_or(p, |x| x.max(p)));
            if let Some(a) = año_de(&d.ruta, texto(d)) {
                desde = Some(desde.map_or(a, |x| x.min(a)));
            }
        } else if porcentaje_max.is_none()
            && let Some(p) = porcentaje(texto(d), claves)
        {
            porcentaje_max = Some(p);
        }
    }
    Some(Confirmado {
        pagadora: (*g).clone(),
        neto,
        bruto: *bruto,
        retencion,
        ficheros,
        porcentaje: porcentaje_max,
        desde,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn importes_y_porcentajes() {
        let t = fold(
            "D. EMPRESA DEMO SL, titular del 40 % del capital social, le corresponde un \
             dividendo bruto de 12.345,68 € y ***********10.000,00",
        );
        assert_eq!(
            importes(&t),
            [1_234_568, 1_000_000].into_iter().collect::<BTreeSet<_>>()
        );
        assert_eq!(porcentaje(&t, &["empresa".into()]), Some(4000));
        let t = fold("de EMPRESA DEMO SL de 10.000,00 €: el bruto menos el 19 % de retención");
        assert_eq!(porcentaje(&t, &["empresa".into()]), None);
        let t = fold("participada de Empresa al 12,5 %");
        assert_eq!(porcentaje(&t, &["empresa".into()]), Some(1250));
    }

    #[test]
    fn la_participada_por_su_carpeta() {
        assert_eq!(
            grupo_de("04_PARTICIPADAS/PARTICIPADA UNO SL/Actas/acta.pdf"),
            "PARTICIPADA UNO SL"
        );
        assert_eq!(grupo_de("SOCIEDAD/acta.pdf"), "SOCIEDAD/acta.pdf");
        assert_eq!(
            bruto_con_retencion(Money::from_euros(10_000, 0)),
            Money::from_cents(1_234_568)
        );
    }
}
