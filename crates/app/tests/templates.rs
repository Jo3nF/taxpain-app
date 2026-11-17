//! Asientos predefinidos y periódicos.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::journal;
use taxpain_app::templates::{Template, TemplateLine, delete, due, list, post, save};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::money::Money;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

fn vault() -> Vault {
    Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap()
}

fn line(account: &str, debit: bool, euros: Option<i64>) -> TemplateLine {
    TemplateLine {
        account: account.into(),
        description: "$".into(),
        debit,
        amount: euros.map(|e| Money::from_euros(e, 0)),
    }
}

#[test]
fn alquiler_mensual_periodico() {
    let v = vault();
    let now = datetime!(2026-10-14 10:00 UTC);
    let t = Template {
        id: 0,
        name: "Alquiler oficina".into(),
        concept: "Alquiler nº #".into(),
        lines: vec![
            line("621", true, Some(1000)),
            line("4751", false, Some(190)),
            line("5720001", false, Some(810)),
        ],
        every_months: Some(1),
        next_date: Some(date!(2026 - 01 - 31)),
        counter: 0,
    };
    let id = save(&v, &t, now).unwrap();
    // Nombre único.
    assert!(save(&v, &t, now).is_err());

    assert!(due(&v, date!(2026 - 01 - 30)).unwrap().is_empty());
    let pending = due(&v, date!(2026 - 02 - 01)).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].lines[2].account, "5720001");

    // El usuario lo revisa y lo registra.
    let entry = JournalEntry {
        date: date!(2026 - 01 - 31),
        concept: "Alquiler nº 1".into(),
        origin: Origin::Manual,
        lines: vec![
            EntryLine::debit(acc("621"), Money::from_euros(1000, 0)),
            EntryLine::credit(acc("4751"), Money::from_euros(190, 0)),
            EntryLine::credit(acc("5720001"), Money::from_euros(810, 0)),
        ],
    };
    post(&v, id, &entry, now).unwrap();
    let t2 = &list(&v).unwrap()[0];
    assert_eq!(t2.counter, 1);
    // 31/1 + 1 mes → 28/2.
    assert_eq!(t2.next_date, Some(date!(2026 - 02 - 28)));
    assert!(due(&v, date!(2026 - 02 - 01)).unwrap().is_empty());
    assert_eq!(journal(&v, 2026).unwrap().len(), 1);

    // Un asiento descuadrado no se registra ni avanza la plantilla.
    let mut bad = entry.clone();
    bad.lines.pop();
    assert!(post(&v, id, &bad, now).is_err());
    assert_eq!(list(&v).unwrap()[0].counter, 1);

    delete(&v, id, now).unwrap();
    assert!(list(&v).unwrap().is_empty());
    assert!(v.verify_audit().is_ok());
}

#[test]
fn validaciones() {
    let v = vault();
    let now = datetime!(2026-10-14 10:00 UTC);
    let mut t = Template {
        id: 0,
        name: "Factura de luz".into(),
        concept: "Luz".into(),
        lines: vec![
            line("628", true, None),
            line("472", true, None),
            line("4100?", false, None),
        ],
        every_months: None,
        next_date: None,
        counter: 0,
    };
    save(&v, &t, now).unwrap();
    t.name = "Otra".into();
    t.lines[2].account = "41a?".into();
    assert!(save(&v, &t, now).is_err());
    t.lines[2].account = "4100?".into();
    t.every_months = Some(3);
    assert!(save(&v, &t, now).is_err(), "periódica sin fecha");
    t.every_months = None;
    t.lines = vec![line("628", true, Some(10)), line("572", false, Some(9))];
    assert!(save(&v, &t, now).is_err(), "importes fijos descuadrados");
}
