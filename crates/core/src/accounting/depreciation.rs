//! Amortización del inmovilizado: lineal (coeficiente anual sobre el valor amortizable, con
//! prorrateo por días en el año de alta y tope en el valor amortizable), degresiva de porcentaje
//! constante o por suma de dígitos (art. 12.1.b y c LIS). Los coeficientes máximos fiscales están
//! en `fixtures/accounting/amortizacion.json` (Ley 27/2014 art. 12).
//!
//! Plan fiscal distinto del contable: si el elemento tiene coeficiente fiscal propio (tablas,
//! libertad de amortización, amortización acelerada), la amortización fiscal del ejercicio se
//! calcula aparte (lineal con ese coeficiente) y la diferencia es un ajuste del modelo 200.
//!
//! Los métodos distintos del lineal y el plan fiscal se reparten por años de vida útil contados
//! desde el alta (el primero, del alta al día anterior a su aniversario) y cada ejercicio recibe
//! la parte de los días que le tocan. El acumulado se calcula entero y se redondea una sola vez,
//! así que al final de la vida útil suma exactamente el valor amortizable.

use serde::{Deserialize, Serialize};
use time::Date;

use super::account::AccountCode;
use super::entry::{EntryLine, JournalEntry, Origin};
use crate::money::{Money, Rate};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixedAsset {
    pub id: i64,
    pub name: String,
    pub account: AccountCode,
    pub depreciation_account: AccountCode,
    pub expense_account: AccountCode,
    pub acquired_on: Date,
    pub cost: Money,
    pub residual: Money,
    /// Coeficiente anual (25 % = 2500).
    pub rate: Rate,
    pub disposed_on: Option<Date>,
    /// Método de la amortización contable.
    #[serde(default)]
    pub method: Method,
    /// Coeficiente fiscal anual si difiere del contable (lineal); `None` = el mismo plan.
    #[serde(default)]
    pub tax_rate: Option<Rate>,
}

/// Método de amortización (art. 12.1 LIS).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    #[default]
    Lineal,
    /// Degresivo de porcentaje constante sobre el valor pendiente: el coeficiente lineal por
    /// 1,5 (vida útil inferior a 5 años), 2 (de 5 a 8) o 2,5 (8 o más), nunca menos del 11 %; el
    /// último año de vida útil se amortiza lo pendiente. No vale para edificios ni mobiliario.
    Constante,
    /// Suma de dígitos: vida útil de N años; el año k amortiza (N − k + 1) / (N·(N+1)/2).
    Digitos,
}

impl Method {
    pub fn code(self) -> &'static str {
        match self {
            Method::Lineal => "lineal",
            Method::Constante => "constante",
            Method::Digitos => "digitos",
        }
    }

    pub fn parse(s: &str) -> Method {
        match s {
            "constante" => Method::Constante,
            "digitos" => Method::Digitos,
            _ => Method::Lineal,
        }
    }
}

/// Años de vida útil con un coeficiente lineal: 100 / coeficiente, redondeado hacia arriba.
pub fn useful_life(rate: Rate) -> i64 {
    let bp = i64::from(rate.bp().max(1));
    (10_000 + bp - 1) / bp
}

/// Porcentaje constante del método degresivo (en puntos básicos).
pub fn constant_percentage(rate: Rate) -> i64 {
    let bp = i64::from(rate.bp().max(1));
    let life = useful_life(rate);
    let pct = match life {
        ..5 => bp * 3 / 2,
        5..8 => bp * 2,
        _ => bp * 5 / 2,
    };
    pct.clamp(1_100, 10_000)
}

/// Cuota de cada año de vida útil (desde el alta) con el método y el coeficiente dados.
fn life_charges(base: Money, method: Method, rate: Rate) -> Vec<Money> {
    let total = base.max(Money::ZERO);
    let mut out = Vec::new();
    let mut done = Money::ZERO;
    let life = useful_life(rate);
    match method {
        Method::Lineal => {
            let year = total.apply_rate(rate);
            while done < total && out.len() < 200 {
                let c = year.min(total - done).max(Money::from_cents(1));
                out.push(c);
                done += c;
            }
        }
        Method::Constante => {
            let pc = Rate::from_bp(i32::try_from(constant_percentage(rate)).unwrap_or(10_000));
            for k in 1..=life {
                let c = if k == life {
                    total - done
                } else {
                    (total - done).apply_rate(pc)
                };
                out.push(c);
                done += c;
            }
        }
        Method::Digitos => {
            let sum = i128::from(life * (life + 1) / 2);
            let mut acc_num = 0i128;
            for k in 1..=life {
                // Acumulado redondeado una vez por año: los redondeos no se arrastran.
                acc_num += i128::from(life - k + 1);
                let cum = round_div(i128::from(total.cents()) * acc_num, sum);
                let c = Money::from_cents(cum) - done;
                out.push(c);
                done += c;
            }
        }
    }
    out
}

fn round_div(num: i128, den: i128) -> i64 {
    let q = num / den;
    let r = num % den;
    let q = if 2 * r.abs() >= den {
        q + num.signum()
    } else {
        q
    };
    i64::try_from(q).unwrap_or(i64::MAX)
}

/// Mismo día `n` años después (el 29 de febrero pasa al 28).
fn plus_years(d: Date, n: i64) -> Date {
    let y = d.year() + i32::try_from(n).unwrap_or(0);
    d.replace_year(y)
        .or_else(|_| Date::from_calendar_date(y, d.month(), 28))
        .unwrap_or(d)
}

/// Amortización acumulada del plan (método y coeficiente) desde el alta hasta `until`
/// (inclusive), sin pasar de la baja.
pub fn accumulated_plan(a: &FixedAsset, method: Method, rate: Rate, until: Date) -> Money {
    let until = match a.disposed_on {
        Some(d) if d < until => d,
        _ => until,
    };
    if until < a.acquired_on {
        return Money::ZERO;
    }
    // Denominador común de años de 365 y 366 días: cada tramo es exacto.
    const DEN: i128 = 365 * 366;
    let mut num = 0i128;
    for (k, c) in life_charges(a.cost - a.residual, method, rate)
        .into_iter()
        .enumerate()
    {
        let k = i64::try_from(k).unwrap_or(0);
        let ks = plus_years(a.acquired_on, k);
        if ks > until {
            break;
        }
        let ke = plus_years(a.acquired_on, k + 1) - time::Duration::days(1);
        let days = i128::from((ke - ks).whole_days() + 1);
        let overlap = i128::from((until.min(ke) - ks).whole_days() + 1);
        num += i128::from(c.cents()) * overlap * (DEN / days);
    }
    Money::from_cents(round_div(num, DEN))
}

/// Cuota de un plan en el ejercicio `fy`: acumulado al cierre menos acumulado al inicio.
pub fn plan_charge(a: &FixedAsset, method: Method, rate: Rate, fy: &FiscalYear) -> Money {
    let before = fy.start - time::Duration::days(1);
    accumulated_plan(a, method, rate, fy.end) - accumulated_plan(a, method, rate, before)
}

/// Amortización fiscal del ejercicio: la contable si no hay coeficiente fiscal propio y, si lo
/// hay, la lineal con ese coeficiente. Con el 100 % (libertad de amortización, art. 12.3 LIS)
/// toda la base amortizable va al ejercicio del alta, sin prorrateo por días: el precepto no
/// lo impone (el prorrateo es de los coeficientes de tablas) y prorratear dejaría media base
/// para el año siguiente, que no es lo que pide quien pone el 100 %.
pub fn tax_charge_in(a: &FixedAsset, fy: &FiscalYear, accounting: Money) -> Money {
    match a.tax_rate {
        None => accounting,
        Some(r) if r.bp() >= 10_000 => {
            if fy.contains(a.acquired_on) {
                (a.cost - a.residual).max(Money::ZERO)
            } else {
                Money::ZERO
            }
        }
        Some(r) => plan_charge(a, Method::Lineal, r, fy),
    }
}

/// Amortización fiscal acumulada antes del ejercicio `fy` (desde el alta) con el plan fiscal del
/// elemento; sin plan fiscal propio, la contable `accounting_before`. Sirve para revertir en la
/// baja la diferencia acumulada entre la amortización contable y la fiscal.
pub fn tax_accumulated_before(a: &FixedAsset, fy: &FiscalYear, accounting_before: Money) -> Money {
    match a.tax_rate {
        None => accounting_before,
        Some(r) if r.bp() >= 10_000 => {
            if a.acquired_on < fy.start {
                (a.cost - a.residual).max(Money::ZERO)
            } else {
                Money::ZERO
            }
        }
        Some(r) => accumulated_plan(a, Method::Lineal, r, fy.start - time::Duration::days(1)),
    }
}

use super::fiscal_year::FiscalYear;

/// Cuota del ejercicio natural `year`, dado lo ya amortizado en ejercicios anteriores.
pub fn annual_charge(a: &FixedAsset, year: i32, accumulated_before: Money) -> Money {
    charge_in(a, &FiscalYear::natural(year), accumulated_before)
}

/// Cuota del ejercicio `fy` (natural o no): coeficiente anual prorrateado por los días en que
/// el elemento está en uso dentro del ejercicio, sobre un año completo (365 o 366 días). Un
/// ejercicio de transición más corto también prorratea.
pub fn charge_in(a: &FixedAsset, fy: &FiscalYear, accumulated_before: Money) -> Money {
    let base = a.cost - a.residual;
    let remaining = base - accumulated_before;
    if a.method != Method::Lineal {
        return plan_charge(a, a.method, a.rate, fy)
            .min(remaining)
            .max(Money::ZERO);
    }
    if remaining.cents() <= 0 || a.acquired_on > fy.end {
        return Money::ZERO;
    }
    let full = base.apply_rate(a.rate);
    let start = a.acquired_on.max(fy.start);
    let end = match a.disposed_on {
        Some(d) if d < fy.start => return Money::ZERO,
        Some(d) if d <= fy.end => d,
        _ => fy.end,
    };
    let days = (end - start).whole_days() + 1;
    let year_days = fy.full_year_days();
    let charge = if days >= year_days {
        full
    } else {
        // Prorrateo por días: full × días / días del año, redondeado al céntimo.
        let num = i128::from(full.cents()) * i128::from(days.max(0));
        let den = i128::from(year_days);
        let q = num / den;
        let q = if 2 * (num % den) >= den { q + 1 } else { q };
        Money::from_cents(i64::try_from(q).unwrap_or(i64::MAX))
    };
    if charge > remaining {
        remaining
    } else {
        charge
    }
}

/// Asiento de amortización del ejercicio natural `year` (68x a 28x), o `None` si no hay cuota.
pub fn depreciation_entry(
    a: &FixedAsset,
    year: i32,
    accumulated_before: Money,
) -> Option<JournalEntry> {
    depreciation_entry_in(a, &FiscalYear::natural(year), accumulated_before)
}

/// Asiento de amortización del ejercicio `fy`, fechado al cierre (o a la baja).
pub fn depreciation_entry_in(
    a: &FixedAsset,
    fy: &FiscalYear,
    accumulated_before: Money,
) -> Option<JournalEntry> {
    let c = charge_in(a, fy, accumulated_before);
    if c.is_zero() {
        return None;
    }
    let date = a.disposed_on.filter(|d| fy.contains(*d)).unwrap_or(fy.end);
    Some(JournalEntry {
        date,
        concept: format!("Amortización {} {}", fy.label(), a.name),
        origin: Origin::Amortizacion(a.id),
        lines: vec![
            EntryLine::debit(a.expense_account.clone(), c),
            EntryLine::credit(a.depreciation_account.clone(), c),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounting::account::acc;
    use time::Month;
    use time::macros::date;

    fn laptop() -> FixedAsset {
        FixedAsset {
            id: 1,
            name: "Portátil".into(),
            account: acc("217"),
            depreciation_account: acc("2817"),
            expense_account: acc("681"),
            acquired_on: date!(2026 - 07 - 01),
            cost: Money::from_euros(1200, 0),
            residual: Money::ZERO,
            rate: Rate::percent(25),
            disposed_on: None,
            method: Method::Lineal,
            tax_rate: None,
        }
    }

    #[test]
    fn degresiva_y_digitos() {
        let mut a = laptop();
        a.acquired_on = date!(2026 - 01 - 01);
        // 25 %: vida útil 4 años (< 5) → porcentaje constante 37,5 %.
        a.method = Method::Constante;
        assert_eq!(constant_percentage(a.rate), 3_750);
        let plan: Vec<i64> = (2026..=2030)
            .map(|y| charge_in(&a, &FiscalYear::natural(y), Money::ZERO).cents())
            .collect();
        // 450; 281,25; 175,78; y el resto el último año (293,04 − 0,01 por redondeo).
        assert_eq!(plan.iter().sum::<i64>(), 120_000);
        assert_eq!(plan[0], 45_000);
        assert_eq!(plan[1], 28_125);
        assert_eq!(plan[4], 0);
        // Suma de dígitos: 4 años, 4/10, 3/10, 2/10, 1/10.
        a.method = Method::Digitos;
        let plan: Vec<i64> = (2026..=2029)
            .map(|y| charge_in(&a, &FiscalYear::natural(y), Money::ZERO).cents())
            .collect();
        assert_eq!(plan, [48_000, 36_000, 24_000, 12_000]);
        // A mitad de año: el primer año de vida se reparte entre dos ejercicios.
        a.acquired_on = date!(2026 - 07 - 01);
        let total: i64 = (2026..=2030)
            .map(|y| charge_in(&a, &FiscalYear::natural(y), Money::ZERO).cents())
            .sum();
        assert_eq!(total, 120_000);
    }

    #[test]
    fn plan_fiscal_distinto() {
        let mut a = laptop();
        a.acquired_on = date!(2026 - 01 - 01);
        // Contable al 25 %; fiscal con libertad de amortización (100 %).
        a.tax_rate = Some(Rate::percent(100));
        let fy = FiscalYear::natural(2026);
        let acc_ = charge_in(&a, &fy, Money::ZERO);
        assert_eq!(acc_, Money::from_euros(300, 0));
        assert_eq!(tax_charge_in(&a, &fy, acc_), Money::from_euros(1200, 0));
        assert_eq!(
            tax_charge_in(&a, &FiscalYear::natural(2027), acc_),
            Money::ZERO
        );
        a.tax_rate = None;
        assert_eq!(tax_charge_in(&a, &fy, acc_), acc_);
        // Alta a mitad de año: la libertad de amortización no se prorratea por días (toda la
        // base en 2026, nada en 2027); un coeficiente fiscal menor sí.
        a.acquired_on = date!(2026 - 07 - 01);
        a.tax_rate = Some(Rate::percent(100));
        let acc_ = charge_in(&a, &fy, Money::ZERO);
        assert_eq!(acc_, Money::from_cents(15_123));
        assert_eq!(tax_charge_in(&a, &fy, acc_), Money::from_euros(1200, 0));
        assert_eq!(
            tax_charge_in(&a, &FiscalYear::natural(2027), acc_),
            Money::ZERO
        );
        a.tax_rate = Some(Rate::percent(50));
        assert_eq!(
            tax_charge_in(&a, &fy, acc_),
            Money::from_cents(30_247),
            "600 × 184/365"
        );
    }

    #[test]
    fn plan_de_amortizacion_completo() {
        let a = laptop();
        // 2026: 300 × 184/365 = 151,23; luego 300 al año hasta agotar 1.200.
        let mut acc_ = Money::ZERO;
        let mut plan = Vec::new();
        for y in 2026..=2031 {
            let c = annual_charge(&a, y, acc_);
            acc_ += c;
            plan.push(c.cents());
        }
        assert_eq!(plan, [15_123, 30_000, 30_000, 30_000, 14_877, 0]);
        assert_eq!(acc_, a.cost);
        let e = depreciation_entry(&a, 2027, Money::from_cents(15_123)).unwrap();
        e.validate().unwrap();
        assert_eq!(e.date, date!(2027 - 12 - 31));
        assert_eq!(annual_charge(&a, 2025, Money::ZERO), Money::ZERO);
    }

    #[test]
    fn ejercicio_no_natural() {
        let a = laptop();
        let junio = (Month::June, 30);
        // Alta el 1/7/2026, ejercicio de julio a junio: año completo desde el primer ejercicio.
        let f = FiscalYear::numbered(2026, junio);
        assert_eq!(charge_in(&a, &f, Money::ZERO), Money::from_euros(300, 0));
        let e = depreciation_entry_in(&a, &f, Money::ZERO).unwrap();
        assert_eq!(e.date, date!(2027 - 06 - 30));
        assert_eq!(e.concept, "Amortización 2026/2027 Portátil");
        // Ejercicio de transición del 1/1 al 30/6/2026: el elemento aún no estaba.
        let t = FiscalYear {
            year: 2026,
            start: date!(2026 - 01 - 01),
            end: date!(2026 - 06 - 30),
        };
        assert_eq!(charge_in(&a, &t, Money::ZERO), Money::ZERO);
    }

    #[test]
    fn baja_y_valor_residual() {
        let mut a = laptop();
        a.residual = Money::from_euros(200, 0);
        a.acquired_on = date!(2026 - 01 - 01);
        a.disposed_on = Some(date!(2027 - 03 - 31));
        assert_eq!(
            annual_charge(&a, 2026, Money::ZERO),
            Money::from_euros(250, 0)
        );
        // 2027 hasta el 31/3: 250 × 90/365 = 61,64
        assert_eq!(
            annual_charge(&a, 2027, Money::from_euros(250, 0)),
            Money::from_cents(6164)
        );
        assert_eq!(
            annual_charge(&a, 2028, Money::from_euros(312, 0)),
            Money::ZERO
        );
        assert_eq!(
            depreciation_entry(&a, 2027, Money::from_euros(250, 0))
                .unwrap()
                .date,
            date!(2027 - 03 - 31)
        );
    }
}
