//! Las piezas de una línea: fechas, importes y texto, cada una con su posición.
//!
//! Una fecha o un importe pueden ocupar varias palabras («4 abr 2024», «- €2 345.67»,
//! «12,00 EUR», «1.234,56 D»): se juntan dentro de un mismo trozo de la línea, nunca de un
//! trozo a otro (el hueco entre trozos es de columna).

use std::sync::LazyLock;

use regex::Regex;
use taxpain_core::money::Money;

use super::maqueta::{Linea, Palabra, Trozo};
use crate::extractos::numeros;

/// Un importe leído, con lo que se sabe de su signo y su divisa.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Cifra {
    pub importe: Money,
    /// ¿Lleva signo, paréntesis o marca de debe/haber?
    pub con_signo: bool,
    /// Divisa que lleva al lado (código o símbolo), si lleva.
    pub divisa: Option<&'static str>,
}

/// Una pieza de una línea.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pieza {
    pub x0: i32,
    pub x1: i32,
    /// El texto tal cual (las palabras separadas por un espacio).
    pub texto: String,
    /// ¿Puede ser una fecha? (Se lee más tarde, con el año y el orden de día y mes.)
    pub fecha: bool,
    /// El importe, si puede serlo.
    pub cifra: Option<Cifra>,
    /// Índice del trozo de la línea del que sale.
    pub trozo: usize,
    /// Índice del grupo de texto seguido dentro del trozo (las palabras de texto contiguas
    /// comparten grupo; cada fecha o importe va en el suyo).
    pub grupo: usize,
}

impl Pieza {
    pub(crate) fn es_texto(&self) -> bool {
        !self.fecha && self.cifra.is_none()
    }
}

/// Códigos de divisa que se reconocen junto a un importe.
const DIVISAS: [&str; 30] = [
    "EUR", "USD", "GBP", "CHF", "JPY", "CAD", "AUD", "SEK", "NOK", "DKK", "PLN", "CZK", "MXN",
    "HUF", "RON", "BGN", "TRY", "CNY", "HKD", "SGD", "NZD", "ZAR", "BRL", "INR", "ILS", "AED",
    "SAR", "RUB", "ISK", "THB",
];

/// Divisa ISO de un código o símbolo («€», «eur», «US$»…).
pub(crate) fn divisa(s: &str) -> Option<&'static str> {
    let t = s
        .trim()
        .trim_matches(|c: char| c == '(' || c == ')' || c == ':');
    match t {
        "€" => return Some("EUR"),
        "$" | "US$" => return Some("USD"),
        "£" => return Some("GBP"),
        "¥" => return Some("JPY"),
        _ => {}
    }
    let up = t.to_ascii_uppercase();
    if matches!(up.as_str(), "EURO" | "EUROS") {
        return Some("EUR");
    }
    DIVISAS.iter().find(|d| **d == up).copied()
}

/// Un importe escrito en una o varias palabras («-1.234,56», «€2 345.67», «12,00 EUR»,
/// «1.234,56 D», «(12.00)», «12.00 CR»). Con una o dos cifras decimales: un número sin
/// decimales no se toma por importe (son oficinas, códigos, referencias…).
pub(crate) fn cifra(s: &str) -> Option<Cifra> {
    static RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(
            r"(?x)^
            (?P<s1>[-−–+(])?\s?
            (?P<c1>[€$£¥]|US\$|[A-Za-z]{3})?\s?
            (?P<s2>[-−–+(])?\s?
            (?P<num>\d{1,3}(?:[.,\u{a0}\u{202f}'\ ]\d{3})+(?:[.,]\d{1,2})?|\d+[.,]\d{1,2})
            (?P<post>[-−+)])?\s?
            (?P<c2>[€$£¥]|[A-Za-z]{3,5})?\s?
            (?P<dh>[DH]|DR|CR|Dr|Cr|DB|dr|cr)?
            $",
        )
        .ok()
    });
    let re = RE.as_ref()?;
    let s = s.trim();
    let c = re.captures(s)?;
    let num = c.name("num")?.as_str();
    // Sin decimales sólo valen los que se ven agrupados por miles con punto o coma y no con
    // espacio («1.234» puede ser un importe; «12 345» son dos números).
    let tiene_decimales = num
        .rfind(['.', ','])
        .is_some_and(|p| (1..=2).contains(&(num.len() - p - 1)));
    if !tiene_decimales {
        return None;
    }
    let mut moneda = None;
    for nombre in ["c1", "c2"] {
        if let Some(m) = c.name(nombre) {
            let d = divisa(m.as_str())?;
            moneda = Some(d);
        }
    }
    let importe = numeros::importe(s)?;
    let con_signo = c.name("s1").is_some()
        || c.name("s2").is_some()
        || c.name("post").is_some()
        || c.name("dh").is_some();
    Some(Cifra {
        importe,
        con_signo,
        divisa: moneda,
    })
}

/// ¿Tiene forma de fecha (con o sin año)? Se comprueba con [`numeros::fecha`] con un año
/// cualquiera, así que «31/02» no lo es.
pub(crate) fn es_fecha(s: &str) -> bool {
    static NUM: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(
            r"^(?:\d{1,2}[/.\-]\d{1,2}(?:[/.\-](?:\d{4}|\d{2}))?\.?|\d{4}[/.\-]\d{1,2}[/.\-]\d{1,2})$",
        )
        .ok()
    });
    static LETRA: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(
            r"(?ix)^(?:
                \d{1,2}(?:st|nd|rd|th|º)?\.?[\s\-/]*(?:de\s+)?\p{L}{3,10}\.?(?:[\s\-/,]*(?:de\s+|del\s+)?\d{2,4})?
                |\p{L}{3,10}\.?\s+\d{1,2}(?:st|nd|rd|th)?,?(?:\s+\d{4})?
            )$",
        )
        .ok()
    });
    let s = s.trim();
    if s.len() < 3 || s.len() > 30 {
        return false;
    }
    let numerica = NUM.as_ref().is_some_and(|r| r.is_match(s));
    let con_letra = !numerica && LETRA.as_ref().is_some_and(|r| r.is_match(s));
    if !numerica && !con_letra {
        return false;
    }
    let s = s.trim_end_matches('.');
    [numeros::Orden::DiaMes, numeros::Orden::MesDia]
        .iter()
        .any(|o| numeros::fecha(s, Some(2024), *o).is_some())
}

/// ¿Es una hora («10:26», «18:37:05»)?
pub(crate) fn es_hora(s: &str) -> bool {
    let partes: Vec<&str> = s.split(':').collect();
    (2..=3).contains(&partes.len())
        && partes
            .iter()
            .all(|p| (1..=2).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Las piezas de un texto suelto (sin posiciones), de izquierda a derecha.
pub(crate) fn de_linea_texto(t: &str) -> Vec<Pieza> {
    let palabras: Vec<Palabra> = t
        .split_whitespace()
        .enumerate()
        .map(|(i, w)| {
            let x = i32::try_from(i).unwrap_or(i32::MAX).saturating_mul(100);
            Palabra {
                x0: x,
                x1: x.saturating_add(90),
                texto: w.to_owned(),
            }
        })
        .collect();
    let trozo = Trozo {
        x0: 0,
        x1: palabras.last().map_or(0, |p| p.x1),
        texto: t.to_owned(),
        palabras,
    };
    let mut out = Vec::new();
    let mut grupo = 0;
    piezas_de_trozo(&trozo, 0, &mut grupo, &mut out);
    out
}

/// Las piezas de una línea, de izquierda a derecha.
pub(crate) fn de_linea(l: &Linea) -> Vec<Pieza> {
    let mut out = Vec::new();
    let mut grupo = 0;
    for (it, t) in l.trozos.iter().enumerate() {
        piezas_de_trozo(t, it, &mut grupo, &mut out);
        grupo += 1;
    }
    out
}

/// Palabras que en realidad son dos, pegadas al sacar el texto del PDF: dos importes
/// («45,30954,70»), un importe y una palabra («42,10Crédito») o una fecha y una palabra
/// («03/01RECIBO»). La posición de cada parte se reparte por sus letras.
fn despegar(w: &Palabra) -> Vec<Palabra> {
    static RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(
            r"(?x)^(?:
                (?P<a1>[-−]?\d{1,3}(?:\.\d{3})*,\d{2})(?P<b1>[-−]?\d{1,3}(?:\.\d{3})*,\d{2}-?)
                |(?P<a2>[-−]?\d{1,3}(?:,\d{3})*\.\d{2})(?P<b2>[-−]?\d{1,3}(?:,\d{3})*\.\d{2}-?)
                |(?P<a3>[-−]?\d[\d.,]*[.,]\d{2})(?P<b3>\p{L}{3,}.*)
                |(?P<a4>\d{1,2}[/.\-]\d{1,2}(?:[/.\-]\d{2,4})?)(?P<b4>\p{L}{2,}.*)
            )$",
        )
        .ok()
    });
    let entera = || vec![w.clone()];
    if cifra(&w.texto).is_some() || es_fecha(&w.texto) {
        return entera();
    }
    let Some(c) = RE.as_ref().and_then(|r| r.captures(&w.texto)) else {
        return entera();
    };
    let partes = [("a1", "b1"), ("a2", "b2"), ("a3", "b3"), ("a4", "b4")]
        .iter()
        .find_map(|(a, b)| Some((c.name(a)?.as_str(), c.name(b)?.as_str())));
    let Some((a, b)) = partes else {
        return entera();
    };
    // Que cada parte sea lo que parece (una divisa pegada ya la acepta `cifra`).
    if !(cifra(a).is_some() || es_fecha(a)) {
        return entera();
    }
    let total = i32::try_from(w.texto.chars().count()).unwrap_or(1).max(1);
    let na = i32::try_from(a.chars().count()).unwrap_or(0);
    let corte = w.x0 + (w.x1 - w.x0) * na / total;
    vec![
        Palabra {
            x0: w.x0,
            x1: corte,
            texto: a.to_owned(),
        },
        Palabra {
            x0: corte,
            x1: w.x1,
            texto: b.to_owned(),
        },
    ]
}

fn piezas_de_trozo(t: &Trozo, it: usize, grupo: &mut usize, out: &mut Vec<Pieza>) {
    let p: Vec<Palabra> = t.palabras.iter().flat_map(despegar).collect();
    let p = &p;
    let mut i = 0;
    let mut en_texto = false;
    while i < p.len() {
        let junta = |n: usize| -> String {
            p[i..i + n]
                .iter()
                .map(|w| w.texto.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        };
        let max = (p.len() - i).min(6);
        // La fecha más larga y el importe más largo que empiezan aquí: gana el más largo («12.05
        // EUR» es un importe; «12.05», las dos cosas, y lo decide la columna).
        let nf = (1..=max.min(5)).rev().find(|n| es_fecha(&junta(*n)));
        let ni = (1..=max)
            .rev()
            .find_map(|n| cifra(&junta(n)).map(|c| (n, c)));
        let hecho = match (nf, ni) {
            (Some(f), Some((n, c))) if n > f => Some((n, junta(n), false, Some(c))),
            (Some(f), _) => {
                let s = junta(f);
                let c = if f == 1 { cifra(&s) } else { None };
                Some((f, s, true, c))
            }
            (None, Some((n, c))) => Some((n, junta(n), false, Some(c))),
            (None, None) => None,
        };
        match hecho {
            Some((n, s, fecha, cifra_)) => {
                if en_texto {
                    *grupo += 1;
                    en_texto = false;
                }
                out.push(Pieza {
                    x0: p[i].x0,
                    x1: p[i + n - 1].x1,
                    texto: s,
                    fecha,
                    cifra: cifra_,
                    trozo: it,
                    grupo: *grupo,
                });
                *grupo += 1;
                i += n;
            }
            None => {
                en_texto = true;
                out.push(Pieza {
                    x0: p[i].x0,
                    x1: p[i].x1,
                    texto: p[i].texto.clone(),
                    fecha: false,
                    cifra: None,
                    trozo: it,
                    grupo: *grupo,
                });
                i += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(s: &str) -> Option<(i64, bool, Option<&'static str>)> {
        cifra(s).map(|c| (c.importe.cents(), c.con_signo, c.divisa))
    }

    #[test]
    fn importes_de_una_o_varias_palabras() {
        assert_eq!(c("1.234,56"), Some((123_456, false, None)));
        assert_eq!(c("-1.234,56 €"), Some((-123_456, true, Some("EUR"))));
        assert_eq!(c("€2 345.67"), Some((234_567, false, Some("EUR"))));
        assert_eq!(c("- €123 456.78"), Some((-12_345_678, true, Some("EUR"))));
        assert_eq!(c("4.321,5 EUR"), Some((432_150, false, Some("EUR"))));
        assert_eq!(c("12,00 D"), Some((-1200, true, None)));
        assert_eq!(c("12.00 CR"), Some((1200, true, None)));
        assert_eq!(c("1.234,56-"), Some((-123_456, true, None)));
        assert_eq!(c("(12.00)"), Some((-1200, true, None)));
        assert_eq!(c("$41.20"), Some((4120, false, Some("USD"))));
        assert_eq!(c("1 234,56"), Some((123_456, false, None)));
        assert_eq!(c("−5,10 USD"), Some((-510, true, Some("USD"))));
        for no in [
            "0999",
            "00042",
            "2023",
            "12 345",
            "1.183274",
            "ABC 12,00",
            "12/05/2025",
        ] {
            assert_eq!(cifra(no), None, "{no}");
        }
    }

    #[test]
    fn fechas_de_una_o_varias_palabras() {
        for si in [
            "14/10/2019",
            "17/11",
            "28.05.2020",
            "2024-08-27",
            "8/4/2022",
            "4 abr 2024",
            "24 sept",
            "1 de enero de 2025",
            "Jan 15, 2025",
            "15 Jan 2025",
            "01-ENE-25",
            "28.02.",
        ] {
            assert!(es_fecha(si), "{si}");
        }
        for no in [
            "31/02", "12,50", "0999", "Saldo", "FEE", "2026", "13/45", "1 EUR",
        ] {
            assert!(!es_fecha(no), "{no}");
        }
    }

    #[test]
    fn palabras_pegadas() {
        let l = Linea::de_trozos(
            1,
            100,
            &[
                (500, "20/01 COMISION 20,002.434,70"),
                (3000, "42,10Crédito"),
            ],
        );
        let p = de_linea(&l);
        let textos: Vec<&str> = p.iter().map(|p| p.texto.as_str()).collect();
        assert_eq!(
            textos,
            ["20/01", "COMISION", "20,00", "2.434,70", "42,10", "Crédito"]
        );
        assert!(p[2].x1 <= p[3].x0);
        assert_eq!(p[3].cifra.map(|c| c.importe.cents()), Some(243_470));
        let l = Linea::de_trozos(1, 100, &[(500, "03/01RECIBO DEMO")]);
        let textos: Vec<String> = de_linea(&l).into_iter().map(|p| p.texto).collect();
        assert_eq!(textos, ["03/01", "RECIBO", "DEMO"]);
    }

    #[test]
    fn piezas_de_una_linea_con_columnas_juntas() {
        let l = Linea::de_trozos(
            1,
            100,
            &[
                (420, "05/06/2024 03/06"),
                (1142, "0999"),
                (1406, "PAGO FACTURA 2024 0007"),
                (4746, "-12,00 € 45.678,90 €"),
            ],
        );
        let p = de_linea(&l);
        let tipos: Vec<(&str, bool, bool)> = p
            .iter()
            .map(|p| (p.texto.as_str(), p.fecha, p.cifra.is_some()))
            .collect();
        assert_eq!(
            tipos,
            [
                ("05/06/2024", true, false),
                ("03/06", true, false),
                ("0999", false, false),
                ("PAGO", false, false),
                ("FACTURA", false, false),
                ("2024", false, false),
                ("0007", false, false),
                ("-12,00 €", false, true),
                ("45.678,90 €", false, true),
            ]
        );
        // Las palabras de texto seguidas comparten grupo; cada fecha o importe va en el suyo.
        assert_eq!(p[3].grupo, p[6].grupo);
        assert_ne!(p[0].grupo, p[1].grupo);
        assert_ne!(p[7].grupo, p[8].grupo);
    }
}
