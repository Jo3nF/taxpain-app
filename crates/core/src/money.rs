//! Importes en céntimos enteros y tipos porcentuales en centésimas de punto (D7).
//!
//! Nunca se usan floats: `Money` envuelve un `i64` de céntimos y `Rate` un `i32` de
//! centésimas de punto porcentual (`2100` = 21 %). Las multiplicaciones por un tipo se hacen en
//! `i128` y se redondean al céntimo con *half away from zero* (0,005 → 0,01; −0,005 → −0,01).
//!
//! `TODO(VERIFY)`: criterio de redondeo de la cuota por línea o por factura (RD 1619/2012).

use std::fmt;

use serde::{Deserialize, Serialize};
use std::iter::Sum;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};

/// Importe en céntimos de euro.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Money(i64);

impl Money {
    pub const ZERO: Money = Money(0);

    pub const fn from_cents(cents: i64) -> Self {
        Money(cents)
    }

    /// `Money::from_euros(12, 34)` = 12,34 €. Para negativos usar `Neg`.
    pub const fn from_euros(euros: i64, cents: u8) -> Self {
        assert!(cents < 100, "cents must be < 100");
        Money(euros * 100 + cents as i64)
    }

    pub const fn cents(self) -> i64 {
        self.0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }

    pub fn checked_add(self, other: Money) -> Option<Money> {
        self.0.checked_add(other.0).map(Money)
    }

    pub fn checked_sub(self, other: Money) -> Option<Money> {
        self.0.checked_sub(other.0).map(Money)
    }

    pub fn abs(self) -> Money {
        Money(self.0.abs())
    }

    /// Aplica un tipo (`base × rate`), redondeando al céntimo half away from zero.
    pub fn apply_rate(self, rate: Rate) -> Money {
        Money(div_round_half_away(
            i128::from(self.0) * i128::from(rate.0),
            i128::from(Rate::ONE_HUNDRED_PERCENT),
        ))
    }

    /// Analiza importes escritos en formato español o con punto decimal:
    /// `"1.234,56"`, `"1234,5"`, `"-0,05"`, `"1234.56"`. Máximo dos decimales.
    pub fn parse(input: &str) -> Option<Money> {
        let s = input.trim().trim_end_matches('€').trim();
        let (negative, s) = match s.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, s),
        };
        // Si hay coma, es el separador decimal y los puntos son de miles.
        let (int_part, frac_part) = if s.contains(',') {
            let (i, f) = s.split_once(',')?;
            (i.replace('.', ""), f.to_owned())
        } else if let Some((i, f)) = s.split_once('.') {
            if f.len() == 3 && !f.contains('.') && i.len() <= 3 {
                // "1.234" → miles en formato español
                (format!("{i}{f}"), String::new())
            } else {
                (i.to_owned(), f.to_owned())
            }
        } else {
            (s.to_owned(), String::new())
        };
        if int_part.is_empty() || !int_part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if frac_part.len() > 2 || !frac_part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let euros: i64 = int_part.parse().ok()?;
        let cents: i64 = match frac_part.len() {
            0 => 0,
            1 => frac_part.parse::<i64>().ok()? * 10,
            _ => frac_part.parse().ok()?,
        };
        let total = euros.checked_mul(100)?.checked_add(cents)?;
        Some(Money(if negative { -total } else { total }))
    }
}

/// División entera redondeando la mitad alejándose de cero. `den > 0`.
fn div_round_half_away(num: i128, den: i128) -> i64 {
    debug_assert!(den > 0);
    let q = num / den;
    let r = num % den;
    let q = if 2 * r.abs() >= den {
        q + num.signum()
    } else {
        q
    };
    i64::try_from(q).expect("importe fuera de rango")
}

impl Add for Money {
    type Output = Money;
    fn add(self, rhs: Money) -> Money {
        self.checked_add(rhs)
            .expect("desbordamiento al sumar importes")
    }
}

impl AddAssign for Money {
    fn add_assign(&mut self, rhs: Money) {
        *self = *self + rhs;
    }
}

impl Sub for Money {
    type Output = Money;
    fn sub(self, rhs: Money) -> Money {
        self.checked_sub(rhs)
            .expect("desbordamiento al restar importes")
    }
}

impl SubAssign for Money {
    fn sub_assign(&mut self, rhs: Money) {
        *self = *self - rhs;
    }
}

impl Neg for Money {
    type Output = Money;
    fn neg(self) -> Money {
        Money(
            self.0
                .checked_neg()
                .expect("desbordamiento al negar importe"),
        )
    }
}

impl Sum for Money {
    fn sum<I: Iterator<Item = Money>>(iter: I) -> Money {
        iter.fold(Money::ZERO, Add::add)
    }
}

/// Formato español: `1.234,56 €`, `-0,05 €`.
impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        let euros = (abs / 100).to_string();
        let mut grouped = String::with_capacity(euros.len() + euros.len() / 3);
        for (i, c) in euros.chars().enumerate() {
            if i > 0 && (euros.len() - i).is_multiple_of(3) {
                grouped.push('.');
            }
            grouped.push(c);
        }
        write!(f, "{sign}{grouped},{:02} €", abs % 100)
    }
}

/// Tipo porcentual en centésimas de punto: `Rate::from_bp(2100)` = 21 %.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Rate(i32);

impl Rate {
    const ONE_HUNDRED_PERCENT: i32 = 10_000;

    pub const ZERO: Rate = Rate(0);

    pub const fn from_bp(hundredths_of_percent: i32) -> Self {
        Rate(hundredths_of_percent)
    }

    /// `Rate::percent(21)` = 21 %.
    pub const fn percent(p: i32) -> Self {
        Rate(p * 100)
    }

    pub const fn bp(self) -> i32 {
        self.0
    }
}

impl fmt::Display for Rate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = self.0 / 100;
        let frac = (self.0 % 100).unsigned_abs();
        if frac == 0 {
            write!(f, "{whole} %")
        } else if frac.is_multiple_of(10) {
            write!(f, "{whole},{} %", frac / 10)
        } else {
            write!(f, "{whole},{frac:02} %")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn display_spanish_format() {
        assert_eq!(Money::from_cents(123_456).to_string(), "1.234,56 €");
        assert_eq!(Money::from_cents(-5).to_string(), "-0,05 €");
        assert_eq!(Money::from_cents(100_000_000).to_string(), "1.000.000,00 €");
        assert_eq!(Money::ZERO.to_string(), "0,00 €");
    }

    #[test]
    fn parse_formats() {
        assert_eq!(Money::parse("1.234,56"), Some(Money::from_cents(123_456)));
        assert_eq!(Money::parse("1234,5"), Some(Money::from_cents(123_450)));
        assert_eq!(Money::parse("1234.56"), Some(Money::from_cents(123_456)));
        assert_eq!(Money::parse("1.234"), Some(Money::from_cents(123_400)));
        assert_eq!(Money::parse("-0,05 €"), Some(Money::from_cents(-5)));
        assert_eq!(Money::parse("12"), Some(Money::from_cents(1200)));
        assert_eq!(Money::parse("1,234"), None);
        assert_eq!(Money::parse("abc"), None);
        assert_eq!(Money::parse(""), None);
    }

    #[test]
    fn apply_rate_rounds_half_away_from_zero() {
        // 0,10 € × 5 % = 0,005 → 0,01
        assert_eq!(
            Money::from_cents(10).apply_rate(Rate::percent(5)),
            Money::from_cents(1)
        );
        assert_eq!(
            Money::from_cents(-10).apply_rate(Rate::percent(5)),
            Money::from_cents(-1)
        );
        // 100,00 € × 21 % = 21,00
        assert_eq!(
            Money::from_euros(100, 0).apply_rate(Rate::percent(21)),
            Money::from_euros(21, 0)
        );
        // 33,33 € × 21 % = 6,9993 → 7,00
        assert_eq!(
            Money::from_cents(3333).apply_rate(Rate::percent(21)),
            Money::from_cents(700)
        );
        // 5,2 % recargo de equivalencia
        assert_eq!(
            Money::from_euros(100, 0).apply_rate(Rate::from_bp(520)),
            Money::from_cents(520)
        );
    }

    #[test]
    fn rate_display() {
        assert_eq!(Rate::percent(21).to_string(), "21 %");
        assert_eq!(Rate::from_bp(520).to_string(), "5,2 %");
        assert_eq!(Rate::from_bp(175).to_string(), "1,75 %");
    }

    proptest! {
        #[test]
        fn display_parse_roundtrip(c in -1_000_000_000_000i64..1_000_000_000_000) {
            let m = Money::from_cents(c);
            let shown = m.to_string();
            prop_assert_eq!(Money::parse(&shown), Some(m));
        }

        #[test]
        fn rate_is_odd_symmetric(c in -1_000_000_000i64..1_000_000_000, bp in 0i32..10_000) {
            let r = Rate::from_bp(bp);
            prop_assert_eq!(Money::from_cents(-c).apply_rate(r), -Money::from_cents(c).apply_rate(r));
        }

        #[test]
        fn rate_error_below_half_cent(c in -1_000_000_000i64..1_000_000_000, bp in 0i32..10_000) {
            let got = i128::from(Money::from_cents(c).apply_rate(Rate::from_bp(bp)).cents());
            let exact_times_10000 = i128::from(c) * i128::from(bp);
            prop_assert!((got * 10_000 - exact_times_10000).abs() <= 5_000);
        }
    }
}
