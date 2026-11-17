//! El texto de las celdas: palabras sin tildes para comparar títulos y etiquetas, y los
//! importes y las fechas que van dentro de un texto.

use std::sync::LazyLock;

use regex::Regex;
use taxpain_core::money::Money;
use time::Date;

use super::super::numeros::{Orden, fecha, importe, importes_en, inferir_orden, sin_tildes};

/// ¿Cuenta como letra o cifra de una palabra (también las vocales con tilde, la ñ y la ç)?
fn de_palabra(c: char) -> bool {
    if c.is_ascii_alphanumeric() {
        return true;
    }
    let mut b = [0u8; 4];
    let s = sin_tildes(c.encode_utf8(&mut b));
    s.len() == 1 && s.bytes().all(|x| x.is_ascii_alphanumeric())
}

/// Las palabras de un texto con su posición (en bytes) y en minúsculas sin tildes. Todo lo que
/// no es letra o cifra las separa.
pub(super) fn trozos(s: &str) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    let mut ini: Option<usize> = None;
    for (i, c) in s.char_indices() {
        match (de_palabra(c), ini) {
            (true, None) => ini = Some(i),
            (false, Some(a)) => {
                out.push((a, i, sin_tildes(&s[a..i])));
                ini = None;
            }
            _ => {}
        }
    }
    if let Some(a) = ini {
        out.push((a, s.len(), sin_tildes(&s[a..])));
    }
    out
}

/// Las palabras de un texto, en minúsculas y sin tildes.
pub(super) fn palabras(s: &str) -> Vec<String> {
    trozos(s).into_iter().map(|(_, _, p)| p).collect()
}

/// El texto como clave para compararlo: sus palabras separadas por un espacio
/// («F. Operación» → «f operacion»).
pub(super) fn clave(s: &str) -> String {
    palabras(s).join(" ")
}

/// El texto sin espacios de más (también los de no separación) ni delante ni detrás.
pub(super) fn limpio(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// El primer importe que hay en un texto («61.234,57 € Retenciones», «-1.234,56 EUR»,
/// «1500,00»).
pub(super) fn primer_importe(s: &str) -> Option<Money> {
    let t = s.trim().trim_end_matches(['.', ';']);
    if t.is_empty() {
        return None;
    }
    if let Some(m) = importe(t) {
        return Some(m);
    }
    if let Some(tok) = importes_en(t).first() {
        return Some(tok.importe);
    }
    let partes: Vec<&str> = t.split_whitespace().collect();
    // «1 234,56 €» o «- 12,00»: dos trozos juntos; si no, el primero que sea un importe.
    for w in partes.windows(2) {
        if let Some(m) = importe(&format!("{}{}", w[0], w[1]))
            && w[0].len() <= 3
        {
            return Some(m);
        }
    }
    partes.iter().find_map(|p| importe(p))
}

/// Fechas que hay en un texto, en orden («Del 01/02/2025 al 28/02/2025»,
/// «01/09/2024-05/03/2026», «15 ene 2025», «2025-01-31»).
pub(super) fn fechas_en(s: &str) -> Vec<Date> {
    static RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(
            r"(?ix)
            \b(?:
                \d{4}-\d{1,2}-\d{1,2}
              | \d{1,2}[/.-]\d{1,2}[/.-]\d{2,4}
              | \d{1,2}(?:\s+de)?\s+\p{L}{3,10}\.?,?(?:\s+de)?\s+\d{4}
              | \p{L}{3,10}\.?\s+\d{1,2},?\s+\d{4}
            )\b",
        )
        .ok()
    });
    let Some(re) = RE.as_ref() else {
        return Vec::new();
    };
    let crudas: Vec<&str> = re.find_iter(s).map(|m| m.as_str()).collect();
    let orden = inferir_orden(crudas.iter().copied());
    crudas
        .iter()
        .filter_map(|c| fecha(c, None, orden))
        .collect()
}

/// La fecha de una celda; sin año («31/12», «15 ene»), la del año más cercano a `cerca`.
pub(super) fn fecha_cerca(s: &str, orden: Orden, cerca: Option<Date>) -> Option<Date> {
    if let Some(f) = fecha(s, None, orden) {
        return Some(f);
    }
    let c = cerca?;
    (c.year() - 1..=c.year() + 1)
        .filter_map(|y| fecha(s, Some(y), orden))
        .min_by_key(|f| (*f - c).whole_days().abs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn palabras_y_claves() {
        assert_eq!(clave(" F. Operación "), "f operacion");
        assert_eq!(clave("Nº de tarjeta"), "n de tarjeta");
        assert_eq!(clave("BENEFICIARIO/ORDENANTE"), "beneficiario ordenante");
        assert_eq!(clave("Amount (EUR)"), "amount eur");
        assert_eq!(clave("Währung"), "wahrung");
        let t = trozos("Saldo actual: 1,00");
        assert_eq!(t[1], (6, 12, "actual".to_owned()));
    }

    #[test]
    fn importes_y_fechas_dentro_de_un_texto() {
        let m = |s: &str| primer_importe(s).map(Money::cents);
        assert_eq!(m("61.234,57 € Retenciones"), Some(6_123_457));
        assert_eq!(m(" 1500,00"), Some(150_000));
        assert_eq!(m("-1.234,56 EUR"), Some(-123_456));
        assert_eq!(m("1 234,56 €"), Some(123_456));
        assert_eq!(m("Todos"), None);
        assert_eq!(
            fechas_en("Periodo 01/09/2024-05/03/2026"),
            [date!(2024 - 09 - 01), date!(2026 - 03 - 05)]
        );
        assert_eq!(
            fechas_en("Fecha de 01/12/2024 hasta 15/01/2025"),
            [date!(2024 - 12 - 01), date!(2025 - 01 - 15)]
        );
        assert_eq!(
            fechas_en("Statement from 1 Jan 2025 to Jan 31, 2025"),
            [date!(2025 - 01 - 01), date!(2025 - 01 - 31)]
        );
        assert_eq!(
            fechas_en("mié, 15 ene 2025, 10:20 CET"),
            [date!(2025 - 01 - 15)]
        );
        assert_eq!(
            fecha_cerca("31/12", Orden::DiaMes, Some(date!(2025 - 01 - 01))),
            Some(date!(2024 - 12 - 31))
        );
        assert_eq!(
            fecha_cerca("02/01", Orden::DiaMes, Some(date!(2024 - 12 - 30))),
            Some(date!(2025 - 01 - 02))
        );
    }
}
