//! Regularización de existencias al cierre.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::{delete_manual_entry, journal, manual_entry, trial_balance};
use taxpain_app::existencias::{regularize, stock};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::money::Money;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

#[test]
fn existencias_iniciales_y_finales() {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let now = datetime!(2026-12-31 10:00 UTC);
    let m = |e: i64| Money::from_euros(e, 0);
    // Existencias iniciales de mercaderías 5.000 (apertura) y compras 20.000.
    for (de, ha, eur) in [("300", "5720001", 5000), ("600", "5720001", 20000)] {
        manual_entry(
            &v,
            &JournalEntry {
                date: date!(2026 - 01 - 02),
                concept: "x".into(),
                origin: Origin::Manual,
                lines: vec![
                    EntryLine::debit(acc(de), m(eur)),
                    EntryLine::credit(acc(ha), m(eur)),
                ],
            },
            now,
        )
        .unwrap();
    }
    let s = stock(&v, 2026).unwrap();
    assert_eq!(s.len(), 1);
    assert_eq!((s[0].account.as_str(), s[0].initial), ("300", m(5000)));
    assert!(regularize(&v, 2026, &[("600".into(), m(1))], now).is_err());

    // Inventario final 7.000 de mercaderías y 1.000 de productos terminados.
    regularize(
        &v,
        2026,
        &[("300".into(), m(7000)), ("350".into(), m(1000))],
        now,
    )
    .unwrap();
    let bal = |a: &str| {
        trial_balance(&v, 2026, None)
            .unwrap()
            .iter()
            .find(|r| r.account.as_str() == a)
            .map(|r| r.debit - r.credit)
            .unwrap_or(Money::ZERO)
    };
    assert_eq!(bal("300"), m(7000));
    assert_eq!(bal("350"), m(1000));
    // 610: +5.000 (baja iniciales) −7.000 (alta finales) = −2.000 (menos consumo).
    assert_eq!(bal("610"), -m(2000));
    assert_eq!(bal("712"), -m(1000));
    let s = stock(&v, 2026).unwrap();
    assert_eq!(
        s.iter().find(|x| x.account == "300").unwrap().final_amount,
        m(7000)
    );
    assert_eq!(
        s.iter().find(|x| x.account == "300").unwrap().initial,
        m(5000)
    );

    // Rehacerla sustituye la anterior.
    let id = regularize(&v, 2026, &[("300".into(), m(6000))], now)
        .unwrap()
        .unwrap();
    assert_eq!(bal("300"), m(6000));
    assert_eq!(bal("350"), Money::ZERO);
    assert_eq!(bal("610"), -m(1000));
    assert_eq!(bal("712"), Money::ZERO);
    // Tiene origen propio: no se borra desde el diario ni se localiza por el concepto.
    let entries = journal(&v, 2026).unwrap();
    let reg: Vec<_> = entries
        .iter()
        .filter(|e| e.entry.origin == Origin::Existencias(2026))
        .collect();
    assert_eq!(reg.len(), 1);
    assert_eq!(reg[0].id, id);
    assert!(delete_manual_entry(&v, id, now, true).is_err());
    // Una regularización grabada por versiones anteriores (manual, con el concepto de la app)
    // también se sustituye al rehacerla.
    let legacy = manual_entry(
        &v,
        &JournalEntry {
            date: date!(2026 - 12 - 31),
            concept: "Regularización de existencias 2026".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("300"), m(100)),
                EntryLine::credit(acc("610"), m(100)),
            ],
        },
        now,
    )
    .unwrap();
    regularize(&v, 2026, &[("300".into(), m(6000))], now).unwrap();
    assert!(journal(&v, 2026).unwrap().iter().all(|e| e.id != legacy));
    assert_eq!(bal("300"), m(6000));
    assert!(v.verify_audit().is_ok());
}
