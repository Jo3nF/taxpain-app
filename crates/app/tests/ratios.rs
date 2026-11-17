//! Ratios financieros de una SL sencilla.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::manual_entry;
use taxpain_app::ratios::ratios;
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::money::Money;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

fn e(d: time::Date, lines: &[(&str, i64, i64)]) -> JournalEntry {
    JournalEntry {
        date: d,
        concept: "x".into(),
        origin: Origin::Manual,
        lines: lines
            .iter()
            .map(|(a, de, ha)| {
                if *de > 0 {
                    EntryLine::debit(acc(a), Money::from_euros(*de, 0))
                } else {
                    EntryLine::credit(acc(a), Money::from_euros(*ha, 0))
                }
            })
            .collect(),
    }
}

#[test]
fn ratios_de_una_sl() {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let now = datetime!(2026-10-20 10:00 UTC);
    for x in [
        // Capital 10.000 y préstamo 20.000 (5.000 a corto).
        e(
            date!(2026 - 01 - 02),
            &[
                ("5720001", 30000, 0),
                ("100", 0, 10000),
                ("170", 0, 15000),
                ("520", 0, 5000),
            ],
        ),
        // Ventas 100.000 (20.000 pendientes de cobro), compras 40.000 (5.000 por pagar).
        e(
            date!(2026 - 06 - 30),
            &[
                ("5720001", 80000, 0),
                ("4300001", 20000, 0),
                ("705", 0, 100000),
            ],
        ),
        // Por pagar: 2.500 a un proveedor (400) y 2.500 a un acreedor (410, la cuenta por
        // defecto de las recibidas).
        e(
            date!(2026 - 06 - 30),
            &[
                ("600", 40000, 0),
                ("5720001", 0, 35000),
                ("4000001", 0, 2500),
                ("4100001", 0, 2500),
            ],
        ),
        // Sueldos 30.000, amortización 2.000 (de un equipo de 10.000), intereses 1.000.
        e(
            date!(2026 - 07 - 01),
            &[("217", 10000, 0), ("5720001", 0, 10000)],
        ),
        e(
            date!(2026 - 12 - 31),
            &[("640", 30000, 0), ("5720001", 0, 30000)],
        ),
        e(
            date!(2026 - 12 - 31),
            &[("681", 2000, 0), ("2817", 0, 2000)],
        ),
        e(
            date!(2026 - 12 - 31),
            &[("662", 1000, 0), ("5720001", 0, 1000)],
        ),
    ] {
        manual_entry(&v, &x, now).unwrap();
    }
    let r = ratios(&v, 2026).unwrap();
    let get = |id: &str| r.iter().find(|x| x.id == id).unwrap().clone();
    // Activo corriente: banco 34.000 + clientes 20.000 = 54.000; pasivo corriente 5.000 + 5.000.
    assert_eq!(
        get("fondo_maniobra").value,
        Some(Money::from_euros(44_000, 0).cents())
    );
    assert_eq!(get("liquidez").value, Some(540));
    assert_eq!(
        get("liquidez").status,
        "atencion",
        "demasiado dinero parado"
    );
    // Explotación: 100.000 − 40.000 − 30.000 − 2.000 = 28.000; resultado 27.000.
    assert_eq!(
        get("ebitda").value,
        Some(Money::from_euros(30_000, 0).cents())
    );
    assert_eq!(get("margen_explotacion").value, Some(2800));
    assert_eq!(get("margen_neto").value, Some(2700));
    assert_eq!(get("cobertura_intereses").value, Some(2800));
    // Clientes 20.000 / ventas 100.000 × 365 = 73 días.
    assert_eq!(get("pmc").value, Some(7300));
    assert_eq!(get("pmc").status, "atencion");
    // Proveedores y acreedores (400 + 410) 5.000 / compras 40.000 × 365 = 45,63 días.
    assert_eq!(get("pmp").value, Some(4563));
    assert_eq!(get("pmp").status, "bien");
    // Sin ejercicio anterior, no hay comparación.
    assert!(r.iter().all(|x| x.previous.is_none()));
    assert!(r.iter().all(|x| x.status != "sin_datos"));
}
