//! Presupuestos (`docs/research/A3.md` §2.17): importe mensual por cuenta o grupo de gastos (6)
//! e ingresos (7), opcionalmente por centro de coste; se pueden generar con lo real de otro
//! ejercicio más un porcentaje, y se comparan con lo real hasta un mes (desviaciones).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::params;
use time::OffsetDateTime;

use crate::analitica;
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetLine {
    /// Cuenta o grupo (`62`, `705`…).
    pub account: String,
    /// Centro de coste (vacío: toda la empresa).
    #[serde(default)]
    pub center: String,
    /// Enero a diciembre.
    pub months: Vec<Money>,
}

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

fn check(l: &BudgetLine) -> Result<()> {
    let a = l.account.trim();
    if a.len() < 2
        || a.len() > 12
        || !a.bytes().all(|b| b.is_ascii_digit())
        || !matches!(a.as_bytes()[0], b'6' | b'7')
    {
        return Err(other(
            "se presupuestan gastos (6) e ingresos (7): pon un grupo o cuenta de al menos 2 dígitos",
        ));
    }
    if l.months.len() != 12 {
        return Err(other("hacen falta los doce meses"));
    }
    Ok(())
}

pub fn list(v: &Vault, year: i32) -> Result<Vec<BudgetLine>> {
    let mut stmt = v.connection().prepare(
        "SELECT account, center, month, amount FROM budget WHERE year = ?1 ORDER BY account, center, month",
    )?;
    let rows = stmt.query_map([year], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })?;
    let mut map: BTreeMap<(String, String), Vec<Money>> = BTreeMap::new();
    for row in rows {
        let (account, center, month, amount) = row?;
        let m = map
            .entry((account, center))
            .or_insert_with(|| vec![Money::ZERO; 12]);
        if let Some(slot) = usize::try_from(month - 1).ok().and_then(|i| m.get_mut(i)) {
            *slot = Money::from_cents(amount);
        }
    }
    Ok(map
        .into_iter()
        .map(|((account, center), months)| BudgetLine {
            account,
            center,
            months,
        })
        .collect())
}

fn write(conn: &taxpain_vault::rusqlite::Connection, year: i32, l: &BudgetLine) -> Result<()> {
    conn.execute(
        "DELETE FROM budget WHERE year = ?1 AND account = ?2 AND center = ?3",
        params![year, l.account.trim(), l.center.trim()],
    )?;
    for (i, m) in l.months.iter().enumerate() {
        if !m.is_zero() {
            conn.execute(
                "INSERT INTO budget(year, account, center, month, amount) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![year, l.account.trim(), l.center.trim(), i64::try_from(i + 1).unwrap_or(1), m.cents()],
            )?;
        }
    }
    Ok(())
}

/// Guarda una línea de presupuesto (todo a cero la borra).
pub fn set(v: &Vault, year: i32, l: &BudgetLine, now: OffsetDateTime) -> Result<()> {
    check(l)?;
    if !l.center.trim().is_empty()
        && !analitica::centers(v)?
            .iter()
            .any(|c| c.code == l.center.trim())
    {
        return Err(other(format!("no existe el centro {}", l.center)));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    write(&tx, year, l)?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "presupuesto.linea",
        &format!("{year}/{}", l.account),
        &serde_json::json!({ "center": l.center, "months": l.months.iter().map(|m| m.cents()).collect::<Vec<_>>() }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Real por cuenta agregada a `digits` y mes (gastos en positivo al cargo; ingresos, al abono).
fn actuals(v: &Vault, year: i32, digits: usize) -> Result<BTreeMap<String, [Money; 12]>> {
    let mut out: BTreeMap<String, [Money; 12]> = BTreeMap::new();
    for l in analitica::lines(v, year, None, None)? {
        let key: String = l.account.chars().take(digits).collect();
        let m = usize::from(u8::from(l.date.month())) - 1;
        out.entry(key).or_insert([Money::ZERO; 12])[m] += l.amount;
    }
    Ok(out)
}

/// Presupuesto de `year` con lo real de `source` (por cuenta a `digits` dígitos y mes) más
/// `change_bp` centésimas de punto (300 = +3 %). Sustituye las líneas de toda la empresa de esas
/// cuentas. Devuelve cuántas líneas escribe.
pub fn from_actuals(
    v: &Vault,
    year: i32,
    source: i32,
    digits: usize,
    change_bp: i32,
    now: OffsetDateTime,
) -> Result<usize> {
    if !(2..=12).contains(&digits) {
        return Err(other("los dígitos van de 2 a 12"));
    }
    let real = actuals(v, source, digits)?;
    if real.is_empty() {
        return Err(other(format!(
            "el ejercicio {source} no tiene gastos ni ingresos"
        )));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let mut n = 0;
    for (account, months) in real {
        let scaled: Vec<Money> = months
            .iter()
            .map(|m| {
                let c = i128::from(m.cents()) * i128::from(10_000 + change_bp);
                // Redondeo al céntimo, alejándose del cero.
                let r = if c >= 0 {
                    (c + 5_000) / 10_000
                } else {
                    (c - 5_000) / 10_000
                };
                Money::from_cents(i64::try_from(r).unwrap_or(0))
            })
            .collect();
        write(
            &tx,
            year,
            &BudgetLine {
                account,
                center: String::new(),
                months: scaled,
            },
        )?;
        n += 1;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "presupuesto.desde_real",
        &year.to_string(),
        &serde_json::json!({ "source": source, "digits": digits, "change_bp": change_bp, "lines": n }),
    )?;
    tx.commit()?;
    Ok(n)
}

/// Comparación de una línea de presupuesto con lo real.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Deviation {
    pub account: String,
    pub name: String,
    pub center: String,
    pub budget: Money,
    pub actual: Money,
    /// Real − presupuesto.
    pub deviation: Money,
    /// Desviación sobre el presupuesto, en centésimas de punto (si hay presupuesto).
    pub deviation_bp: Option<i64>,
    /// La desviación es desfavorable (más gasto o menos ingreso de lo previsto).
    pub unfavourable: bool,
}

/// Presupuesto frente a real de enero a `through` (1–12).
pub fn report(v: &Vault, year: i32, through: u8) -> Result<Vec<Deviation>> {
    if !(1..=12).contains(&through) {
        return Err(other("el mes va de 1 a 12"));
    }
    let budget = list(v, year)?;
    let lines: Vec<_> = analitica::lines(v, year, None, None)?
        .into_iter()
        .filter(|l| u8::from(l.date.month()) <= through)
        .collect();
    let mut out = Vec::new();
    for b in budget {
        let planned = b
            .months
            .iter()
            .take(usize::from(through))
            .fold(Money::ZERO, |a, m| a + *m);
        let actual = lines
            .iter()
            .filter(|l| l.account.starts_with(&b.account))
            .fold(Money::ZERO, |a, l| {
                if b.center.is_empty() {
                    a + l.amount
                } else {
                    a + analitica::split(l.amount, &l.shares)
                        .into_iter()
                        .filter(|(c, _)| *c == b.center)
                        .fold(Money::ZERO, |x, (_, m)| x + m)
                }
            });
        let deviation = actual - planned;
        let expense = b.account.starts_with('6');
        out.push(Deviation {
            name: crate::accounting::account_name(v.connection(), &b.account)?.unwrap_or_default(),
            deviation_bp: (!planned.is_zero())
                .then(|| deviation.cents() * 10_000 / planned.cents()),
            unfavourable: if expense {
                deviation.cents() > 0
            } else {
                deviation.is_negative()
            },
            account: b.account,
            center: b.center,
            budget: planned,
            actual,
            deviation,
        });
    }
    Ok(out)
}
