//! Libros derivados del diario: mayor y balance de sumas y saldos.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::account::AccountCode;
use super::entry::JournalEntry;
use crate::money::Money;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountBalance {
    pub account: AccountCode,
    pub debit: Money,
    pub credit: Money,
}

impl AccountBalance {
    /// Saldo deudor (positivo) o acreedor (negativo).
    pub fn balance(&self) -> Money {
        self.debit - self.credit
    }
}

/// Balance de sumas y saldos por cuenta (al nivel de detalle de los asientos).
pub fn trial_balance<'a>(
    entries: impl IntoIterator<Item = &'a JournalEntry>,
) -> Vec<AccountBalance> {
    let mut map: BTreeMap<AccountCode, (Money, Money)> = BTreeMap::new();
    for e in entries {
        for l in &e.lines {
            let v = map.entry(l.account.clone()).or_default();
            v.0 += l.debit;
            v.1 += l.credit;
        }
    }
    map.into_iter()
        .map(|(account, (debit, credit))| AccountBalance {
            account,
            debit,
            credit,
        })
        .collect()
}

/// Agrega saldos a `digits` dígitos (p. ej. 3 → cuentas del PGC: 4300001 + 4300002 → 430).
pub fn aggregate(balances: &[AccountBalance], digits: usize) -> Vec<AccountBalance> {
    let mut map: BTreeMap<String, (Money, Money)> = BTreeMap::new();
    for b in balances {
        let key: String = b.account.as_str().chars().take(digits).collect();
        let v = map.entry(key).or_default();
        v.0 += b.debit;
        v.1 += b.credit;
    }
    map.into_iter()
        .filter_map(|(k, (debit, credit))| {
            // También grupos (1 cifra) y subgrupos (2): «Sumas y saldos» por grupos.
            AccountCode::agregado(&k).map(|account| AccountBalance {
                account,
                debit,
                credit,
            })
        })
        .collect()
}

/// Suma de saldos de las cuentas que empiezan por alguno de `prefixes`.
pub fn sum_prefixes(balances: &[AccountBalance], prefixes: &[&str]) -> Money {
    balances
        .iter()
        .filter(|b| prefixes.iter().any(|p| b.account.starts_with(p)))
        .map(AccountBalance::balance)
        .sum()
}

/// Movimientos de una cuenta (o de sus subcuentas) con saldo acumulado: el libro mayor.
pub fn ledger<'a>(
    entries: impl IntoIterator<Item = &'a JournalEntry>,
    prefix: &str,
) -> Vec<(&'a JournalEntry, Money, Money, Money)> {
    let mut running = Money::ZERO;
    let mut out = Vec::new();
    for e in entries {
        for l in e.lines.iter().filter(|l| l.account.starts_with(prefix)) {
            running += l.net();
            out.push((e, l.debit, l.credit, running));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounting::account::acc;
    use crate::accounting::entry::{EntryLine, Origin};
    use time::macros::date;

    #[test]
    fn sumas_y_saldos_y_mayor() {
        let m = Money::from_euros;
        let e1 = JournalEntry {
            date: date!(2026 - 01 - 01),
            concept: "Aportación".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("5720001"), m(3000, 0)),
                EntryLine::credit(acc("100"), m(3000, 0)),
            ],
        };
        let e2 = JournalEntry {
            date: date!(2026 - 01 - 02),
            concept: "Gasto".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("629"), m(100, 0)),
                EntryLine::credit(acc("5720001"), m(100, 0)),
            ],
        };
        let tb = trial_balance([&e1, &e2]);
        let total_d: Money = tb.iter().map(|b| b.debit).sum();
        let total_c: Money = tb.iter().map(|b| b.credit).sum();
        assert_eq!(total_d, total_c);
        assert_eq!(sum_prefixes(&tb, &["57"]), m(2900, 0));
        assert_eq!(
            aggregate(&tb, 3)
                .iter()
                .find(|b| b.account.as_str() == "572")
                .unwrap()
                .balance(),
            m(2900, 0)
        );
        let mayor = ledger([&e1, &e2], "572");
        assert_eq!(mayor.len(), 2);
        assert_eq!(mayor[1].3, m(2900, 0));
    }

    #[test]
    fn agrega_por_grupos_y_subgrupos() {
        use crate::accounting::account::acc;
        let b = |a: &str, d: i64, c: i64| AccountBalance {
            account: acc(a),
            debit: Money::from_cents(d),
            credit: Money::from_cents(c),
        };
        let saldos = [
            b("4300001", 1_000, 0),
            b("4720001", 210, 0),
            b("7050001", 0, 1_210),
        ];
        let grupos = aggregate(&saldos, 1);
        assert_eq!(
            grupos
                .iter()
                .map(|g| (g.account.as_str(), g.debit.cents(), g.credit.cents()))
                .collect::<Vec<_>>(),
            [("4", 1_210, 0), ("7", 0, 1_210)]
        );
        assert_eq!(aggregate(&saldos, 2).len(), 3);
    }
}
