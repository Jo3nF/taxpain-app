//! Cantidades de criptomonedas y precios en euros sin coma flotante: enteros escalados.
//!
//! - [`Qty`]: unidades de un activo con hasta 18 decimales (la precisión del wei de Ethereum),
//!   como `i128` escalado por 10^18.
//! - [`Precio`]: euros por unidad con hasta 8 decimales (los tokens de precio ínfimo cotizan con
//!   esa precisión en las plataformas), como `i128` escalado por 10^8.
//!
//! Los importes en euros siguen siendo [`Money`] (céntimos). El producto cantidad × precio se
//! hace en `i128` con comprobación de desbordamiento y se redondea al céntimo (mitad hacia
//! arriba).

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::money::Money;

/// Decimales de [`Qty`].
pub const DECIMALES_QTY: u32 = 18;
/// Decimales de [`Precio`].
pub const DECIMALES_PRECIO: u32 = 8;
const ESCALA_QTY: i128 = 10i128.pow(DECIMALES_QTY);
const ESCALA_PRECIO: i128 = 10i128.pow(DECIMALES_PRECIO);

/// Cantidad de un activo (18 decimales).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Qty(i128);

/// Precio en euros por unidad (8 decimales).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Precio(i128);

/// Lee un decimal escrito a mano o exportado («1.234,5678», «1234.5678», «0,00000001»,
/// «-2») como entero escalado por 10^`decimales`. Más decimales de los admitidos → `None`.
fn parse_escalado(s: &str, decimales: u32) -> Option<i128> {
    let t: String = s
        .trim()
        .chars()
        .filter(|c| !matches!(c, ' ' | '\u{a0}' | '€'))
        .collect();
    if t.is_empty() {
        return None;
    }
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r.to_owned()),
        None => (false, t.trim_start_matches('+').to_owned()),
    };
    // Con los dos separadores, el último es el decimal («1.234,56», «1,234.56»). Con uno solo,
    // repetido es de miles («1.234.567»); si no, es el decimal aunque le sigan tres cifras: a
    // diferencia de los importes en euros, «0.001» o «1.500» son cantidades legítimas (BTC con
    // tres decimales) y los exportadores de cripto escriben el punto decimal.
    let puntos = t.matches('.').count();
    let comas = t.matches(',').count();
    let normalizado = match (puntos, comas) {
        (0, 0) | (1, 0) => t,
        (_, 0) => t.replace('.', ""),
        (0, 1) => t.replace(',', "."),
        (0, _) => t.replace(',', ""),
        _ => {
            let (decimal, miles) = if t.rfind('.') > t.rfind(',') {
                ('.', ',')
            } else {
                (',', '.')
            };
            let (ent, dec) = t.rsplit_once(decimal)?;
            if ent.contains(decimal) {
                return None;
            }
            format!("{}.{dec}", ent.replace(miles, ""))
        }
    };
    let (ent, dec) = normalizado
        .split_once('.')
        .unwrap_or((normalizado.as_str(), ""));
    if ent.is_empty() && dec.is_empty() {
        return None;
    }
    if !ent.bytes().all(|b| b.is_ascii_digit()) || !dec.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let decimales = decimales as usize;
    if dec.len() > decimales {
        // Se admite «1.50000000000000000000» (ceros de más), no una precisión mayor.
        if !dec[decimales..].bytes().all(|b| b == b'0') {
            return None;
        }
    }
    let ent: i128 = if ent.is_empty() { 0 } else { ent.parse().ok()? };
    let dec_str = format!(
        "{:0<width$}",
        dec.get(..decimales).unwrap_or(dec),
        width = decimales
    );
    let dec: i128 = if dec_str.is_empty() {
        0
    } else {
        dec_str.parse().ok()?
    };
    let escala = 10i128.checked_pow(decimales as u32)?;
    let v = ent.checked_mul(escala)?.checked_add(dec)?;
    Some(if neg { -v } else { v })
}

fn fmt_escalado(v: i128, decimales: u32, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let escala = 10i128.pow(decimales);
    let neg = v < 0;
    let abs = v.unsigned_abs();
    let ent = abs / escala.unsigned_abs();
    let dec = abs % escala.unsigned_abs();
    let dec = format!("{dec:0>width$}", width = decimales as usize);
    let dec = dec.trim_end_matches('0');
    if neg {
        f.write_str("-")?;
    }
    if dec.is_empty() {
        write!(f, "{ent}")
    } else {
        write!(f, "{ent},{dec}")
    }
}

/// `a × b / c` en `i128` redondeando a la unidad (mitad hacia arriba). Si el producto no cabe,
/// se reducen `b` y `c` (su cociente es lo que importa) hasta que quepa.
pub(crate) fn mul_div(a: i128, b: i128, c: i128) -> Option<i128> {
    if c == 0 {
        return None;
    }
    let (mut b, mut c) = (b, c);
    loop {
        if let Some(p) = a.checked_mul(b) {
            let q = p / c;
            let r = p % c;
            // Mitad hacia arriba en valor absoluto.
            let ajuste = if r.unsigned_abs() >= c.unsigned_abs().div_ceil(2) {
                if (p < 0) != (c < 0) { -1 } else { 1 }
            } else {
                0
            };
            return Some(q + ajuste);
        }
        if b.abs() < 1000 || c.abs() < 1000 {
            return None;
        }
        b /= 1000;
        c /= 1000;
    }
}

impl Qty {
    pub const ZERO: Qty = Qty(0);

    pub fn parse(s: &str) -> Option<Qty> {
        parse_escalado(s, DECIMALES_QTY).map(Qty)
    }

    /// Unidades enteras.
    pub const fn from_units(n: i64) -> Qty {
        Qty(n as i128 * ESCALA_QTY)
    }

    /// Valor escalado por 10^18 (para cálculos).
    pub const fn raw(self) -> i128 {
        self.0
    }

    pub const fn from_raw(v: i128) -> Qty {
        Qty(v)
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    pub fn abs(self) -> Qty {
        Qty(self.0.abs())
    }

    pub fn checked_add(self, o: Qty) -> Option<Qty> {
        self.0.checked_add(o.0).map(Qty)
    }

    pub fn checked_sub(self, o: Qty) -> Option<Qty> {
        self.0.checked_sub(o.0).map(Qty)
    }

    /// Parte proporcional de un importe: `importe × self / total`, al céntimo.
    pub fn parte_de(self, importe: Money, total: Qty) -> Option<Money> {
        mul_div(i128::from(importe.cents()), self.0, total.0)
            .and_then(|c| i64::try_from(c).ok())
            .map(Money::from_cents)
    }

    /// Valor en euros a un precio por unidad, redondeado al céntimo.
    pub fn valor(self, precio: Precio) -> Option<Money> {
        // céntimos = qty/1e18 × precio/1e8 × 100
        mul_div(self.0, precio.0, ESCALA_QTY * ESCALA_PRECIO / 100)
            .and_then(|c| i64::try_from(c).ok())
            .map(Money::from_cents)
    }

    /// Precio implícito de un importe por esta cantidad (`importe / self`).
    pub fn precio_de(self, importe: Money) -> Option<Precio> {
        if self.0 == 0 {
            return None;
        }
        // precio = importe/100 / (qty/1e18) → escalado por 1e8: importe × 1e18 × 1e8 / (100 × qty)
        mul_div(
            i128::from(importe.cents()),
            ESCALA_QTY * ESCALA_PRECIO / 100,
            self.0,
        )
        .map(Precio)
    }
}

impl Precio {
    pub const ZERO: Precio = Precio(0);

    pub fn parse(s: &str) -> Option<Precio> {
        parse_escalado(s, DECIMALES_PRECIO).map(Precio)
    }

    pub const fn from_raw(v: i128) -> Precio {
        Precio(v)
    }

    pub const fn raw(self) -> i128 {
        self.0
    }

    /// Precio en euros enteros (para constantes de tests y demos).
    pub const fn from_euros(e: i64) -> Precio {
        Precio(e as i128 * ESCALA_PRECIO)
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl fmt::Display for Qty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_escalado(self.0, DECIMALES_QTY, f)
    }
}

impl fmt::Debug for Qty {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Qty({self})")
    }
}

impl fmt::Display for Precio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt_escalado(self.0, DECIMALES_PRECIO, f)
    }
}

impl fmt::Debug for Precio {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Precio({self})")
    }
}

// Por el cable y en la base de datos van como texto decimal con punto («1.5»), nunca como
// número de coma flotante.
fn texto_punto(v: i128, decimales: u32) -> String {
    let escala = 10i128.pow(decimales);
    let neg = v < 0;
    let abs = v.unsigned_abs();
    let ent = abs / escala.unsigned_abs();
    let dec = format!(
        "{:0>width$}",
        abs % escala.unsigned_abs(),
        width = decimales as usize
    );
    let dec = dec.trim_end_matches('0');
    let signo = if neg { "-" } else { "" };
    if dec.is_empty() {
        format!("{signo}{ent}")
    } else {
        format!("{signo}{ent}.{dec}")
    }
}

impl Qty {
    /// Texto decimal con punto («0.00000001»), el formato de almacenamiento.
    pub fn to_text(self) -> String {
        texto_punto(self.0, DECIMALES_QTY)
    }
}

impl Precio {
    /// Texto decimal con punto, el formato de almacenamiento.
    pub fn to_text(self) -> String {
        texto_punto(self.0, DECIMALES_PRECIO)
    }
}

impl Serialize for Qty {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_text())
    }
}

impl<'de> Deserialize<'de> for Qty {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Qty::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("cantidad no válida: {s}")))
    }
}

impl Serialize for Precio {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_text())
    }
}

impl<'de> Deserialize<'de> for Precio {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Precio::parse(&s).ok_or_else(|| serde::de::Error::custom(format!("precio no válido: {s}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_y_escribe_cantidades_sin_perder_decimales() {
        let q = Qty::parse("1.234,567890123456789012").unwrap();
        assert_eq!(q.to_text(), "1234.567890123456789012");
        assert_eq!(q.to_string(), "1234,567890123456789012");
        assert_eq!(Qty::parse("0.00000001").unwrap().raw(), 10_000_000_000);
        assert_eq!(Qty::parse("2").unwrap(), Qty::from_units(2));
        assert_eq!(Qty::parse("-0,5").unwrap().raw(), -ESCALA_QTY / 2);
        assert!(
            Qty::parse("1,0000000000000000001").is_none(),
            "19 decimales"
        );
        assert!(Qty::parse("1e3").is_none());
        assert!(Qty::parse("").is_none());
        assert_eq!(
            Qty::parse("1,500000000000000000000").unwrap().to_text(),
            "1.5"
        );
    }

    #[test]
    fn separadores_de_miles_y_decimal() {
        let u = |s: &str| Qty::parse(s).map(|q| q.to_text());
        assert_eq!(u("1,234.56").as_deref(), Some("1234.56"));
        assert_eq!(u("20,000.00").as_deref(), Some("20000"));
        assert_eq!(u("1.234,56").as_deref(), Some("1234.56"));
        assert_eq!(u("1,234,567").as_deref(), Some("1234567"));
        assert_eq!(u("1.234.567").as_deref(), Some("1234567"));
        // Un único punto (o coma) es el decimal en las cantidades de cripto.
        assert_eq!(u("1.500").as_deref(), Some("1.5"));
        assert_eq!(u("0.001").as_deref(), Some("0.001"));
        assert_eq!(u("0,001").as_deref(), Some("0.001"));
        assert!(Qty::parse("1.234,5.6").is_none());
        assert_eq!(
            Precio::parse("1,234.56").map(|p| p.to_text()).as_deref(),
            Some("1234.56")
        );
    }

    #[test]
    fn valor_al_cento_y_precio_implicito() {
        let q = Qty::parse("0.5").unwrap();
        let p = Precio::parse("60000.12345678").unwrap();
        assert_eq!(q.valor(p).unwrap(), Money::from_cents(3_000_006)); // 30.000,0617… → ,06
        assert_eq!(
            Qty::parse("1")
                .unwrap()
                .precio_de(Money::from_cents(3_000_006))
                .unwrap(),
            Precio::parse("30000.06").unwrap()
        );
        assert_eq!(
            Qty::parse("0.1")
                .unwrap()
                .parte_de(Money::from_cents(1001), Qty::parse("0.3").unwrap()),
            Some(Money::from_cents(334))
        );
        // Un producto que no cabe en i128 se reduce en vez de fallar.
        let grande = Qty::parse("10000000000").unwrap();
        assert_eq!(
            grande.valor(Precio::from_euros(1_000_000)).unwrap(),
            Money::from_cents(1_000_000_000_000_000_000)
        );
    }

    #[test]
    fn serde_como_texto() {
        let q = Qty::parse("0,25").unwrap();
        assert_eq!(serde_json::to_string(&q).unwrap(), "\"0.25\"");
        let back: Qty = serde_json::from_str("\"0.25\"").unwrap();
        assert_eq!(back, q);
        assert!(
            serde_json::from_str::<Qty>("0.25").is_err(),
            "nunca un número JSON"
        );
    }
}
