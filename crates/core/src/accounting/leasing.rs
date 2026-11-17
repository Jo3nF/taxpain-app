//! Arrendamiento financiero (leasing) según la NRV 8ª del PGC: el bien se registra en el
//! inmovilizado por su valor al contado contra la deuda (524) y cada cuota se reparte entre
//! intereses (662) y devolución del capital, más el IVA de la cuota (472), con la factura que el
//! arrendador emite por cada cuota (la registra la app como recibida). Al cierre, el capital
//! que vence a más de doce meses se reclasifica a la 174 (largo plazo) con [`long_term_at`].
//!
//! Cuotas constantes (sistema francés) calculadas con aritmética entera: la cuota es
//! `P·i / (1 − (1 + i)^−n)` con `i` el tipo nominal anual / 12, en coma fija de 10⁻¹² (la
//! aritmética es la de los préstamos, [`super::prestamo`]).

use serde::{Deserialize, Serialize};
use time::Date;

use super::account::AccountCode;
use super::entry::{EntryLine, JournalEntry, Origin};
use super::prestamo::{ESCALA as SCALE, potencia, redondear as round, sumar_meses};
use crate::money::{Money, Rate};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Leasing {
    #[serde(default)]
    pub id: i64,
    pub name: String,
    /// Fecha del contrato (y del alta del bien).
    pub start: Date,
    /// Valor al contado del bien (sin IVA).
    pub principal: Money,
    /// Número de cuotas mensuales (la opción de compra va en la última).
    pub months: u16,
    /// Tipo de interés nominal anual (5 % = 500).
    pub rate: Rate,
    /// Opción de compra (valor residual) que se paga con la última cuota.
    pub purchase_option: Money,
    /// IVA de las cuotas (21 % = 2100).
    pub vat: Rate,
    /// Cuenta del bien (218, 213…) y de la deuda (5240001…).
    pub asset_account: AccountCode,
    pub debt_account: AccountCode,
    /// Arrendador: emite una factura por cada cuota (los contratos antiguos no lo tienen).
    #[serde(default)]
    pub lessor: Option<Lessor>,
}

/// Arrendador (entidad de leasing) de un contrato.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lessor {
    pub nif: String,
    pub name: String,
    /// Porcentaje deducible del IVA de las cuotas: 100 salvo uso mixto (p. ej. el 50 % de un
    /// turismo, art. 95.Tres LIVA).
    #[serde(default = "full")]
    pub deductible_pct: u8,
}

fn full() -> u8 {
    100
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installment {
    /// Número de cuota (1…n).
    pub n: u16,
    pub date: Date,
    pub interest: Money,
    pub capital: Money,
    pub vat: Money,
    /// Cuota sin IVA (intereses + capital).
    pub amount: Money,
    /// Capital pendiente tras pagarla.
    pub outstanding: Money,
}

fn add_months(d: Date, months: u16) -> Date {
    sumar_meses(d, u32::from(months))
}

/// Cuota mensual constante (intereses + capital, sin IVA) para amortizar `principal` menos el
/// valor actual de la opción de compra.
pub fn payment(l: &Leasing) -> Money {
    let n = u32::from(l.months.max(1));
    // i en coma fija: bp / 10000 / 12.
    let i = i128::from(l.rate.bp()) * SCALE / 120_000;
    if i == 0 {
        let to_amortize = l.principal - l.purchase_option;
        return Money::from_cents(to_amortize.cents() / i64::from(n));
    }
    // (1 + i)^n en coma fija.
    let pow = potencia(i, n);
    // Valor actual de la opción de compra pagada con la última cuota: OC / (1+i)^n.
    let pv_option = i128::from(l.purchase_option.cents()) * SCALE / pow;
    let to_amortize = i128::from(l.principal.cents()) * SCALE - pv_option * SCALE;
    // cuota = A · i · (1+i)^n / ((1+i)^n − 1)
    let num = to_amortize / SCALE * i * pow / SCALE;
    let den = pow - SCALE;
    Money::from_cents(round(num * SCALE / den))
}

/// Cuadro de amortización: cuotas mensuales desde un mes después del contrato. La última
/// incluye la opción de compra y ajusta el redondeo.
pub fn schedule(l: &Leasing) -> Vec<Installment> {
    let pay = payment(l);
    let i = i128::from(l.rate.bp()) * SCALE / 120_000;
    let mut outstanding = l.principal;
    let mut out = Vec::new();
    for n in 1..=l.months {
        let interest = Money::from_cents(round(i128::from(outstanding.cents()) * i));
        let mut capital = pay - interest;
        if n == l.months {
            capital = outstanding;
        }
        outstanding -= capital;
        let amount = interest + capital;
        out.push(Installment {
            n,
            date: add_months(l.start, n),
            interest,
            capital,
            vat: amount.apply_rate(l.vat),
            amount,
            outstanding,
        });
    }
    out
}

/// Capital que vence a más de doce meses desde `date` (deuda a largo plazo, cuenta 174).
pub fn long_term_at(l: &Leasing, date: Date) -> Money {
    let limit = add_months(date, 12);
    schedule(l)
        .iter()
        .filter(|q| q.date > limit)
        .map(|q| q.capital)
        .sum()
}

/// Asiento del alta: bien al debe, deuda al haber, por el valor al contado.
pub fn start_entry(l: &Leasing) -> JournalEntry {
    JournalEntry {
        date: l.start,
        concept: format!("Leasing {}: alta del bien", l.name),
        origin: Origin::Leasing(l.id),
        lines: vec![
            EntryLine::debit(l.asset_account.clone(), l.principal),
            EntryLine::credit(l.debt_account.clone(), l.principal),
        ],
    }
}

/// Asiento del pago de una cuota: deuda (capital), 662 (intereses) y 472 (IVA) contra la
/// cuenta de pago.
pub fn installment_entry(
    l: &Leasing,
    q: &Installment,
    interest_account: &AccountCode,
    vat_account: &AccountCode,
    treasury: &AccountCode,
) -> JournalEntry {
    let lines = [
        EntryLine::signed(l.debt_account.clone(), q.capital, true),
        EntryLine::signed(interest_account.clone(), q.interest, true),
        EntryLine::signed(vat_account.clone(), q.vat, true),
        EntryLine::signed(treasury.clone(), q.amount + q.vat, false),
    ];
    JournalEntry {
        date: q.date,
        concept: format!("Leasing {}: cuota {}/{}", l.name, q.n, l.months),
        origin: Origin::Leasing(l.id),
        lines: lines.into_iter().flatten().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounting::account::acc;
    use time::macros::date;

    fn coche() -> Leasing {
        Leasing {
            id: 1,
            name: "Furgoneta".into(),
            start: date!(2026 - 01 - 15),
            principal: Money::from_euros(30_000, 0),
            months: 48,
            rate: Rate::from_bp(500),
            purchase_option: Money::from_euros(600, 0),
            vat: Rate::percent(21),
            asset_account: acc("218"),
            debt_account: acc("5240001"),
            lessor: None,
        }
    }

    #[test]
    fn cuadro_frances() {
        let l = coche();
        let s = schedule(&l);
        assert_eq!(s.len(), 48);
        // Cuota de referencia (cálculo exacto con fracciones): 679,5612…
        let p = payment(&l);
        assert_eq!(p, Money::from_cents(67_956));
        assert_eq!(s[0].interest, Money::from_cents(12_500));
        assert_eq!(s[0].date, date!(2026 - 02 - 15));
        assert_eq!(s[47].outstanding, Money::ZERO);
        // Todo el capital se devuelve: la suma de capital es el principal.
        let capital: Money = s.iter().map(|q| q.capital).sum();
        assert_eq!(capital, l.principal);
        // La última incluye la opción de compra (≈ cuota + 600).
        assert!((s[47].amount - p - Money::from_euros(600, 0)).abs() < Money::from_euros(3, 0));
        let e = installment_entry(&l, &s[0], &acc("662"), &acc("472"), &acc("572"));
        e.validate().unwrap();
        start_entry(&l).validate().unwrap();
        // Al 31/12/2026, largo plazo = capital de las cuotas que vencen después del 31/12/2027.
        let lt = long_term_at(&l, date!(2026 - 12 - 31));
        let expected: Money = s
            .iter()
            .filter(|q| q.date > date!(2027 - 12 - 31))
            .map(|q| q.capital)
            .sum();
        assert_eq!(lt, expected);
        assert_eq!(long_term_at(&l, date!(2029 - 12 - 31)), Money::ZERO);
    }

    #[test]
    fn sin_intereses() {
        let mut l = coche();
        l.rate = Rate::ZERO;
        l.purchase_option = Money::ZERO;
        l.months = 3;
        l.principal = Money::from_euros(100, 0);
        let s = schedule(&l);
        assert_eq!(
            s.iter().map(|q| q.capital.cents()).collect::<Vec<_>>(),
            [3333, 3333, 3334]
        );
    }
}
