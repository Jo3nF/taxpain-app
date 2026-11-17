//! Lo que comparten los perfiles de banco: fechas, importes, limpieza del texto y orden de los
//! movimientos.

use taxpain_core::money::Money;
use time::{Date, Month};

use crate::extractos::Movimiento;
use crate::extractos::numeros::{self, Orden};

/// Importe en el formato español de los PDF («-1.234,56», «8.642,3», «0,00»), también con un
/// solo decimal (BBVA quita el cero final). `None` si no es un importe.
pub(super) fn importe(s: &str) -> Option<Money> {
    numeros::importe(s.trim())
}

/// Fecha numérica día/mes/año («31/01/2025», «31.01.2025», «2025-01-31») o con el mes en letra
/// («1 de enero de 2026», «03 julio 2024», «20 sept 2022»).
pub(super) fn fecha(s: &str) -> Option<Date> {
    numeros::fecha(s, None, Orden::DiaMes)
}

/// Fecha sin año («dd/mm») con el año que la deja más cerca de `cerca` (la fecha de operación):
/// la fecha valor del 31/12 de un movimiento del 01/01 es del año anterior.
pub(super) fn sin_año(dia: u8, mes: u8, cerca: Date) -> Option<Date> {
    let mes = Month::try_from(mes).ok()?;
    [cerca.year() - 1, cerca.year(), cerca.year() + 1]
        .into_iter()
        .filter_map(|y| Date::from_calendar_date(y, mes, dia).ok())
        .min_by_key(|d| (*d - cerca).whole_days().abs())
}

/// El texto con los espacios seguidos (también los de no separación) como uno solo y sin los de
/// los extremos.
pub(super) fn limpiar(s: &str) -> String {
    s.split(|c: char| c.is_whitespace() || c == '\u{a0}' || c == '\u{202f}')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Une trozos de texto con un espacio, sin los vacíos.
pub(super) fn unir<'a>(partes: impl IntoIterator<Item = &'a str>) -> String {
    limpiar(&partes.into_iter().collect::<Vec<_>>().join(" "))
}

/// Un importe con su divisa, para los avisos: «1.234,56 EUR».
pub(super) fn cantidad(m: Money, divisa: &str) -> String {
    let s = m.to_string();
    format!("{} {divisa}", s.trim_end_matches(" €"))
}

/// El titular sin el punto que algunos bancos le ponen detrás («EMPRESA DEMO SL.»).
pub(super) fn sin_punto(s: &str) -> String {
    let t = limpiar(s);
    t.strip_suffix('.').map(str::to_owned).unwrap_or(t)
}

/// Bloques del texto separados por líneas en blanco, cada uno con sus líneas sin los espacios
/// de los extremos.
pub(super) fn bloques(texto: &str) -> Vec<Vec<&str>> {
    let mut out: Vec<Vec<&str>> = Vec::new();
    let mut actual: Vec<&str> = Vec::new();
    for l in texto.lines() {
        let t = l.trim();
        if t.is_empty() {
            if !actual.is_empty() {
                out.push(std::mem::take(&mut actual));
            }
        } else {
            actual.push(t);
        }
    }
    if !actual.is_empty() {
        out.push(actual);
    }
    out
}

/// Pone en orden cronológico los movimientos de un extracto que puede venir del más nuevo al
/// más antiguo. Si las fechas no lo dicen (todos del mismo día), lo decide el saldo de cada
/// fila: en orden cronológico, saldo anterior + importe = saldo. Si tampoco, `nuevo_primero`
/// dice cómo los da el banco. Se le da la vuelta a la lista entera, para que también quede bien
/// el orden dentro de cada día.
pub(super) fn al_derecho(m: &mut [Movimiento], nuevo_primero: bool) {
    let (Some(primera), Some(ultima)) = (m.first(), m.last()) else {
        return;
    };
    let al_reves = if primera.fecha != ultima.fecha {
        primera.fecha > ultima.fecha
    } else {
        let encadenan = |a: &Movimiento, b: &Movimiento| match (a.saldo, b.saldo) {
            (Some(sa), Some(sb)) => sa + b.importe == sb,
            _ => false,
        };
        let adelante = m.windows(2).filter(|w| encadenan(&w[0], &w[1])).count();
        let atras = m.windows(2).filter(|w| encadenan(&w[1], &w[0])).count();
        if adelante == atras {
            nuevo_primero
        } else {
            atras > adelante
        }
    };
    if al_reves {
        m.reverse();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn mov(fecha: Date, importe: i64, saldo: i64) -> Movimiento {
        Movimiento {
            importe_eur: None,
            fecha,
            fecha_valor: None,
            concepto: String::new(),
            importe: Money::from_cents(importe),
            saldo: Some(Money::from_cents(saldo)),
            referencia: String::new(),
        }
    }

    #[test]
    fn fecha_sin_año_cerca_de_la_operacion() {
        let op = date!(2022 - 01 - 01);
        assert_eq!(sin_año(31, 12, op), Some(date!(2021 - 12 - 31)));
        assert_eq!(sin_año(3, 1, op), Some(date!(2022 - 01 - 03)));
        assert_eq!(
            sin_año(2, 1, date!(2024 - 12 - 30)),
            Some(date!(2025 - 01 - 02))
        );
        assert_eq!(
            sin_año(29, 2, date!(2024 - 03 - 01)),
            Some(date!(2024 - 02 - 29))
        );
        assert_eq!(sin_año(31, 2, op), None);
    }

    #[test]
    fn importes_y_textos() {
        assert_eq!(importe("8.642,3").map(Money::cents), Some(864_230));
        assert_eq!(importe("-11,4").map(Money::cents), Some(-1_140));
        assert_eq!(importe("-1.234,56").map(Money::cents), Some(-123_456));
        assert_eq!(limpiar("  A \u{a0} B   C "), "A B C");
        assert_eq!(
            cantidad(Money::from_cents(-123_456), "USD"),
            "-1.234,56 USD"
        );
        assert_eq!(sin_punto("EMPRESA DEMO SL."), "EMPRESA DEMO SL");
        assert_eq!(
            bloques("\n\nuno\n dos \n\n\ntres"),
            vec![vec!["uno", "dos"], vec!["tres"]]
        );
    }

    #[test]
    fn orden_cronologico() {
        let d = date!(2025 - 01 - 02);
        // Del más nuevo al más antiguo, todo el mismo día: lo dice el saldo.
        let mut m = vec![mov(d, -500, 9_500), mov(d, 1_000, 10_000)];
        al_derecho(&mut m, false);
        assert_eq!(m[0].importe.cents(), 1_000);
        // Por las fechas.
        let mut m = vec![
            mov(d, -500, 9_000),
            mov(date!(2025 - 01 - 01), 1_000, 9_500),
        ];
        al_derecho(&mut m, false);
        assert_eq!(m[0].fecha, date!(2025 - 01 - 01));
        // Sin saldos ni fechas distintas: como lo da el banco.
        let mut m = vec![mov(d, -500, 0), mov(d, 1_000, 0)];
        al_derecho(&mut m, true);
        assert_eq!(m[0].importe.cents(), 1_000);
    }
}
