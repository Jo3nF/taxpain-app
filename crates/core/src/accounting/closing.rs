//! Fin de ejercicio: regularización (grupos 6 y 7 contra 129), cierre (saldos a cero) y
//! apertura (inversa del cierre). Ver `docs/ACCOUNTING_RULES.md`.

use time::Date;

use super::account::{AccountCode, acc};
use super::entry::{EntryLine, JournalEntry, Origin};
use super::ledger::trial_balance;
use crate::money::Money;

/// Asiento de regularización: salda las cuentas 6 y 7 contra 129 (Resultado del ejercicio).
/// `None` si no hay saldos de gestión.
pub fn regularization(entries: &[JournalEntry], date: Date) -> Option<JournalEntry> {
    let mut lines = Vec::new();
    let mut result = Money::ZERO; // positivo = beneficio
    for b in trial_balance(entries) {
        if b.account.is_income_statement() && !b.balance().is_zero() {
            result -= b.balance();
            lines.extend(EntryLine::signed(b.account.clone(), -b.balance(), true));
        }
    }
    if lines.is_empty() {
        return None;
    }
    lines.extend(EntryLine::signed(acc("129"), result, false));
    Some(JournalEntry {
        date,
        concept: "Regularización del ejercicio".into(),
        origin: Origin::Regularizacion,
        lines,
    })
}

/// Cuentas de los grupos 8 y 9 con saldo. Deben estar saldadas antes del cierre: el PGC
/// (quinta parte) las traspasa a su cuenta del grupo 13 (p. ej. 840/940 a la 130), y si no, el
/// asiento de cierre las arrastraría a la apertura del ejercicio siguiente.
pub fn unsettled_equity_income_expense(entries: &[JournalEntry]) -> Vec<(AccountCode, Money)> {
    trial_balance(entries)
        .into_iter()
        .filter(|b| b.account.is_equity_income_expense() && !b.balance().is_zero())
        .map(|b| (b.account.clone(), b.balance()))
        .collect()
}

/// Asiento de cierre: deja a cero todas las cuentas con saldo (tras la regularización).
pub fn closing(entries: &[JournalEntry], date: Date) -> Option<JournalEntry> {
    let lines: Vec<EntryLine> = trial_balance(entries)
        .into_iter()
        .filter_map(|b| EntryLine::signed(b.account.clone(), -b.balance(), true))
        .collect();
    (lines.len() >= 2).then(|| JournalEntry {
        date,
        concept: "Cierre del ejercicio".into(),
        origin: Origin::Cierre,
        lines,
    })
}

/// Asiento de apertura: el inverso del cierre, el primer día del ejercicio siguiente.
pub fn opening(closing: &JournalEntry, date: Date) -> JournalEntry {
    JournalEntry {
        date,
        concept: "Apertura del ejercicio".into(),
        origin: Origin::Apertura,
        lines: closing
            .lines
            .iter()
            .map(|l| EntryLine {
                account: l.account.clone(),
                debit: l.credit,
                credit: l.debit,
                description: None,
            })
            .collect(),
    }
}

/// Resultado del ejercicio (antes de regularizar): ingresos − gastos.
pub fn result_of_year(entries: &[JournalEntry]) -> Money {
    -trial_balance(entries)
        .iter()
        .filter(|b| b.account.is_income_statement())
        .map(super::ledger::AccountBalance::balance)
        .sum::<Money>()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounting::ledger::trial_balance;
    use proptest::prelude::*;
    use time::macros::date;

    fn entry(lines: Vec<EntryLine>) -> JournalEntry {
        JournalEntry {
            date: date!(2026 - 06 - 01),
            concept: "x".into(),
            origin: Origin::Manual,
            lines,
        }
    }

    #[test]
    fn grupos_8_y_9_con_saldo() {
        let m = Money::from_euros;
        let mut entries = vec![entry(vec![
            EntryLine::debit(acc("572"), m(1000, 0)),
            EntryLine::credit(acc("940"), m(1000, 0)),
        ])];
        assert_eq!(pending(&entries), [("940".to_string(), -m(1000, 0))]);
        // Traspasada a la 130: ya no queda saldo.
        entries.push(entry(vec![
            EntryLine::debit(acc("940"), m(1000, 0)),
            EntryLine::credit(acc("130"), m(1000, 0)),
        ]));
        assert!(unsettled_equity_income_expense(&entries).is_empty());
    }

    fn pending(entries: &[JournalEntry]) -> Vec<(String, Money)> {
        unsettled_equity_income_expense(entries)
            .into_iter()
            .map(|(a, b)| (a.as_str().to_owned(), b))
            .collect()
    }

    #[test]
    fn ciclo_completo() {
        let m = Money::from_euros;
        let entries = vec![
            entry(vec![
                EntryLine::debit(acc("572"), m(3000, 0)),
                EntryLine::credit(acc("100"), m(3000, 0)),
            ]),
            entry(vec![
                EntryLine::debit(acc("4300001"), m(1210, 0)),
                EntryLine::credit(acc("705"), m(1000, 0)),
                EntryLine::credit(acc("477"), m(210, 0)),
            ]),
            entry(vec![
                EntryLine::debit(acc("629"), m(300, 0)),
                EntryLine::debit(acc("472"), m(63, 0)),
                EntryLine::credit(acc("4100001"), m(363, 0)),
            ]),
        ];
        assert_eq!(result_of_year(&entries), m(700, 0));
        let mut all = entries.clone();
        let reg = regularization(&all, date!(2026 - 12 - 31)).unwrap();
        reg.validate().unwrap();
        assert_eq!(
            reg.lines
                .iter()
                .find(|l| l.account.as_str() == "129")
                .unwrap()
                .credit,
            m(700, 0)
        );
        all.push(reg);
        assert!(
            trial_balance(&all)
                .iter()
                .filter(|b| b.account.is_income_statement())
                .all(|b| b.balance().is_zero())
        );
        let close = closing(&all, date!(2026 - 12 - 31)).unwrap();
        close.validate().unwrap();
        all.push(close.clone());
        assert!(trial_balance(&all).iter().all(|b| b.balance().is_zero()));
        let open = opening(&close, date!(2027 - 01 - 01));
        open.validate().unwrap();
        let tb_open = trial_balance([&open]);
        assert_eq!(
            tb_open
                .iter()
                .find(|b| b.account.as_str() == "129")
                .unwrap()
                .balance(),
            -m(700, 0)
        );
        assert_eq!(
            tb_open
                .iter()
                .find(|b| b.account.as_str() == "572")
                .unwrap()
                .balance(),
            m(3000, 0)
        );
    }

    proptest! {
        /// Con asientos cuadrados arbitrarios: tras regularizar y cerrar, todo saldo es cero y la
        /// apertura reproduce los saldos de balance (grupos 1–5) más el resultado en 129.
        #[test]
        fn cierre_deja_todo_a_cero(moves in prop::collection::vec((0usize..8, 0usize..8, 1i64..10_000_000), 1..40)) {
            let accounts = ["100", "572", "4300001", "4100001", "477", "472", "629", "705"];
            let entries: Vec<_> = moves.iter().filter(|(a, b, _)| a != b).map(|&(a, b, c)| entry(vec![
                EntryLine::debit(acc(accounts[a]), Money::from_cents(c)),
                EntryLine::credit(acc(accounts[b]), Money::from_cents(c)),
            ])).collect();
            prop_assume!(!entries.is_empty());
            let profit = result_of_year(&entries);
            let mut all = entries.clone();
            if let Some(r) = regularization(&all, date!(2026 - 12 - 31)) {
                prop_assert_eq!(r.validate(), Ok(()));
                all.push(r);
            }
            let before_close = trial_balance(&all);
            if let Some(c) = closing(&all, date!(2026 - 12 - 31)) {
                prop_assert_eq!(c.validate(), Ok(()));
                all.push(c.clone());
                prop_assert!(trial_balance(&all).iter().all(|b| b.balance().is_zero()));
                let open = trial_balance([&opening(&c, date!(2027 - 01 - 01))]);
                for b in before_close.iter().filter(|b| !b.balance().is_zero()) {
                    let o = open.iter().find(|x| x.account == b.account).unwrap();
                    prop_assert_eq!(o.balance(), b.balance());
                }
                let r129 = open.iter().find(|x| x.account.as_str() == "129").map_or(Money::ZERO, |x| x.balance());
                prop_assert_eq!(-r129, profit);
            }
        }
    }
}

/// Datos del 303 del periodo con los que se contabiliza su liquidación.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VatSettlement {
    /// IVA repercutido devengado en el periodo (saldo acreedor de la 477 que se salda).
    pub output_vat: Money,
    /// IVA soportado deducido en el periodo (saldo deudor de la 472 que se salda).
    pub input_vat: Money,
    /// Cuotas a compensar de periodos anteriores aplicadas en esta autoliquidación ([78]).
    pub applied: Money,
    /// Resultado de la autoliquidación ([71]): a ingresar si es positivo; si es negativo, el
    /// nuevo exceso a compensar o a devolver.
    pub result: Money,
}

/// Asiento de liquidación del IVA de un periodo a partir de su 303 (PGC PYMES, quinta parte,
/// cuentas 4700 y 4750; art. 99.Cinco LIVA):
///
/// - 477 al debe y 472 al haber por lo devengado y deducido en el periodo;
/// - 4700 al haber por las cuotas de periodos anteriores que esta autoliquidación compensa
///   ([78]: «se abonará … en caso de compensación en declaración-liquidación posterior, con
///   cargo a la cuenta 477»);
/// - 4750 al haber sólo por el resultado a ingresar ([71] > 0), de modo que su saldo coincide
///   siempre con lo que se paga; o 4700 al debe por el nuevo exceso ([71] < 0), ya sea a
///   compensar o a devolver;
/// - si el resultado del 303 no coincide con 477 − 472 (regularización de bienes de inversión
///   [43], de la prorrata [44] o casillas fijadas a mano), la diferencia es un ajuste de la
///   imposición indirecta: 634 «ajustes negativos» (más gasto: se deduce menos de lo
///   contabilizado) o 639 «ajustes positivos» (menos gasto).
pub fn vat_settlement(
    s: &VatSettlement,
    date: Date,
    period_key: i64,
    label: &str,
) -> Option<JournalEntry> {
    let mut lines = Vec::new();
    lines.extend(EntryLine::signed(acc("477"), s.output_vat, true));
    lines.extend(EntryLine::signed(acc("472"), s.input_vat, false));
    lines.extend(EntryLine::signed(acc("4700"), s.applied, false));
    if s.result.is_negative() {
        lines.extend(EntryLine::signed(acc("4700"), -s.result, true));
    } else {
        lines.extend(EntryLine::signed(acc("4750"), s.result, false));
    }
    // [46] = [78] + [71]: lo que el 303 liquida frente a lo que dice el diario.
    let diff = (s.output_vat - s.input_vat) - (s.applied + s.result);
    if diff.is_negative() {
        lines.extend(EntryLine::signed(acc("634"), -diff, true));
    } else {
        lines.extend(EntryLine::signed(acc("639"), diff, false));
    }
    (lines.len() >= 2).then(|| JournalEntry {
        date,
        concept: format!("Liquidación IVA {label}"),
        origin: Origin::LiquidacionIva(period_key),
        lines,
    })
}

#[cfg(test)]
mod vat_tests {
    use super::*;
    use time::macros::date;

    fn line(e: &JournalEntry, account: &str) -> (Money, Money) {
        e.lines
            .iter()
            .filter(|l| l.account.as_str() == account)
            .fold((Money::ZERO, Money::ZERO), |(d, c), l| {
                (d + l.debit, c + l.credit)
            })
    }

    #[test]
    fn liquidacion_a_ingresar_y_a_compensar() {
        let m = Money::from_euros;
        let e = vat_settlement(
            &VatSettlement {
                output_vat: m(315, 0),
                input_vat: m(63, 0),
                applied: Money::ZERO,
                result: m(252, 0),
            },
            date!(2026 - 03 - 31),
            202601,
            "1T 2026",
        )
        .unwrap();
        e.validate().unwrap();
        assert_eq!(line(&e, "4750"), (Money::ZERO, m(252, 0)));
        let e = vat_settlement(
            &VatSettlement {
                output_vat: m(10, 0),
                input_vat: m(63, 0),
                applied: Money::ZERO,
                result: -m(53, 0),
            },
            date!(2026 - 03 - 31),
            202601,
            "1T",
        )
        .unwrap();
        e.validate().unwrap();
        assert_eq!(line(&e, "4700"), (m(53, 0), Money::ZERO));
        assert!(vat_settlement(&VatSettlement::default(), date!(2026 - 03 - 31), 1, "x").is_none());
    }

    /// 2T con 315 − 63 = 252 de cuota y 53 pendientes del 1T: el 303 ingresa 199 y la 4700 se
    /// abona por los 53 compensados (PGC PYMES, cuenta 4700, b1).
    #[test]
    fn compensa_el_exceso_anterior_y_la_4750_es_lo_que_se_paga() {
        let m = Money::from_euros;
        let e = vat_settlement(
            &VatSettlement {
                output_vat: m(315, 0),
                input_vat: m(63, 0),
                applied: m(53, 0),
                result: m(199, 0),
            },
            date!(2026 - 06 - 30),
            202602,
            "2T",
        )
        .unwrap();
        e.validate().unwrap();
        assert_eq!(line(&e, "4700"), (Money::ZERO, m(53, 0)));
        assert_eq!(line(&e, "4750"), (Money::ZERO, m(199, 0)));
        assert!(
            e.lines
                .iter()
                .all(|l| !l.account.as_str().starts_with("63"))
        );
    }

    /// El 303 deduce 10 menos que la 472 (regularización de la prorrata, [44]): la diferencia
    /// va a la 634 y la 4750 sigue siendo el resultado del modelo.
    #[test]
    fn la_diferencia_con_el_303_va_a_ajustes_de_la_imposicion_indirecta() {
        let m = Money::from_euros;
        let e = vat_settlement(
            &VatSettlement {
                output_vat: m(315, 0),
                input_vat: m(63, 0),
                applied: Money::ZERO,
                result: m(262, 0),
            },
            date!(2026 - 12 - 31),
            202604,
            "4T",
        )
        .unwrap();
        e.validate().unwrap();
        assert_eq!(line(&e, "634"), (m(10, 0), Money::ZERO));
        assert_eq!(line(&e, "4750"), (Money::ZERO, m(262, 0)));
        let e = vat_settlement(
            &VatSettlement {
                output_vat: m(315, 0),
                input_vat: m(63, 0),
                applied: Money::ZERO,
                result: m(240, 0),
            },
            date!(2026 - 12 - 31),
            202604,
            "4T",
        )
        .unwrap();
        e.validate().unwrap();
        assert_eq!(line(&e, "639"), (Money::ZERO, m(12, 0)));
    }
}
