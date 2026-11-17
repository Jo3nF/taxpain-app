//! Ejercicio no natural (de julio a junio): numeración, amortización, cierre y apertura, y
//! ejercicio de transición al cambiar la fecha de cierre.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::{
    add_fixed_asset, close_year, fiscal_year, fiscal_year_info, journal, manual_entry,
    post_depreciation, update_manual_entry,
};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::depreciation::FixedAsset;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::calendar::TaxpayerProfile;
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::Month;
use time::macros::{date, datetime};

fn sociedad(v: &Vault, close: (Month, u8)) {
    let mut p = TaxpayerProfile::sociedad_limitada();
    p.cierre_ejercicio = close;
    v.set_setting("taxpayer_profile", &p).unwrap();
}

fn entry(d: time::Date, euros: i64) -> JournalEntry {
    JournalEntry {
        date: d,
        concept: format!("Venta {d}"),
        origin: Origin::Manual,
        lines: vec![
            EntryLine::debit(acc("572"), Money::from_euros(euros, 0)),
            EntryLine::credit(acc("705"), Money::from_euros(euros, 0)),
        ],
    }
}

fn vault() -> Vault {
    Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap()
}

#[test]
fn de_julio_a_junio() {
    let v = vault();
    sociedad(&v, (Month::June, 30));
    let now = datetime!(2027-07-15 10:00 UTC);

    // Septiembre de 2026 y marzo de 2027 son del mismo ejercicio, el 2026 (2026/2027).
    manual_entry(&v, &entry(date!(2026 - 09 - 10), 1000), now).unwrap();
    let id = manual_entry(&v, &entry(date!(2027 - 03 - 05), 500), now).unwrap();
    let f = fiscal_year(&v, 2026).unwrap();
    assert_eq!(
        (f.start, f.end),
        (date!(2026 - 07 - 01), date!(2027 - 06 - 30))
    );
    let j = journal(&v, 2026).unwrap();
    assert_eq!(j.len(), 2);
    assert_eq!(j.iter().map(|e| e.number).collect::<Vec<_>>(), [1, 2]);
    let info = fiscal_year_info(&v, None, date!(2027 - 02 - 01)).unwrap();
    assert_eq!((info.year, info.label.as_str()), (2026, "2026/2027"));
    assert!(!info.natural);

    // Mover un asiento dentro del ejercicio vale; sacarlo del ejercicio, no.
    update_manual_entry(&v, id, &entry(date!(2027 - 06 - 30), 500), now).unwrap();
    assert!(update_manual_entry(&v, id, &entry(date!(2027 - 07 - 01), 500), now).is_err());

    // Amortización: alta el 1/7/2026, año completo en el primer ejercicio, fechada al cierre.
    add_fixed_asset(
        &v,
        &FixedAsset {
            id: 0,
            name: "Portátil".into(),
            account: acc("217"),
            depreciation_account: acc("2817"),
            expense_account: acc("681"),
            acquired_on: date!(2026 - 07 - 01),
            cost: Money::from_euros(1200, 0),
            residual: Money::ZERO,
            rate: Rate::percent(25),
            disposed_on: None,
            method: Default::default(),
            tax_rate: None,
        },
        now,
    )
    .unwrap();
    assert_eq!(post_depreciation(&v, 2026, now).unwrap().len(), 1);
    let dep = journal(&v, 2026)
        .unwrap()
        .into_iter()
        .find(|e| matches!(e.entry.origin, Origin::Amortizacion(_)))
        .unwrap();
    assert_eq!(dep.entry.date, date!(2027 - 06 - 30));
    assert_eq!(dep.entry.lines[0].debit, Money::from_euros(300, 0));

    // Cierre a 30/6/2027 y apertura el 1/7/2027 en el ejercicio 2027.
    let s = close_year(&v, 2026, now, true).unwrap();
    assert_eq!(s.result, Money::from_euros(1200, 0));
    let next = journal(&v, 2027).unwrap();
    assert_eq!(next.len(), 1);
    assert_eq!(next[0].entry.origin, Origin::Apertura);
    assert_eq!(next[0].entry.date, date!(2027 - 07 - 01));
    // El ejercicio cerrado no admite asientos, ni siquiera en marzo de 2027.
    assert!(manual_entry(&v, &entry(date!(2027 - 03 - 01), 10), now).is_err());
}

#[test]
fn cambio_de_fecha_de_cierre() {
    let v = vault();
    let now = datetime!(2027-01-10 10:00 UTC);
    // Ejercicios naturales hasta 2025; después la sociedad pasa a cerrar el 30 de junio.
    sociedad(&v, (Month::December, 31));
    manual_entry(&v, &entry(date!(2025 - 05 - 01), 100), now).unwrap();
    sociedad(&v, (Month::June, 30));
    manual_entry(&v, &entry(date!(2026 - 03 - 01), 100), now).unwrap();
    manual_entry(&v, &entry(date!(2026 - 09 - 01), 100), now).unwrap();

    let old = fiscal_year(&v, 2025).unwrap();
    assert_eq!(
        (old.start, old.end),
        (date!(2025 - 01 - 01), date!(2025 - 12 - 31))
    );
    // Transición: del 1/1 al 30/6/2026.
    let t = fiscal_year(&v, 2026).unwrap();
    assert_eq!(
        (t.start, t.end),
        (date!(2026 - 01 - 01), date!(2026 - 06 - 30))
    );
    // El siguiente empieza el 1/7/2026; como el número 2026 ya está, es el 2027.
    let n = fiscal_year(&v, 2027).unwrap();
    assert_eq!(
        (n.start, n.end),
        (date!(2026 - 07 - 01), date!(2027 - 06 - 30))
    );
    assert_eq!(journal(&v, 2026).unwrap().len(), 1);
    assert_eq!(journal(&v, 2027).unwrap().len(), 1);
}

/// Art. 40.2 LIS: si el último 200 vencido es de un ejercicio corto, la base del 202 se completa
/// con la parte proporcional de la cuota de los anteriores hasta doce meses.
#[test]
fn la_base_del_202_se_completa_tras_un_ejercicio_de_transicion() {
    use std::collections::BTreeMap;
    use taxpain_app::{models, presentados};
    let v = vault();
    let now = datetime!(2027-10-10 10:00 UTC);
    sociedad(&v, (Month::December, 31));
    manual_entry(&v, &entry(date!(2025 - 05 - 01), 100), now).unwrap();
    sociedad(&v, (Month::June, 30));
    manual_entry(&v, &entry(date!(2026 - 03 - 01), 100), now).unwrap();
    manual_entry(&v, &entry(date!(2026 - 09 - 01), 100), now).unwrap();
    let cuota = |c: i64| BTreeMap::from([(599u16, Money::from_euros(c, 0))]);
    presentados::save_manual(&v, "200", 2025, "0A", &cuota(3650), now).unwrap();
    presentados::save_manual(&v, "200", 2026, "0A", &cuota(6000), now).unwrap();
    // 2P de 2027 (1 de octubre): el último 200 vencido es el de la transición 1/1–30/6/2026
    // (181 días): 6.000 + 184/365 de los 3.650 de 2025 (1.840) = 7.840.
    let (base, note) = models::corporate_tax_base(&v, 2027, 2).unwrap();
    assert_eq!(base, Some(Money::from_euros(7840, 0)));
    let note = note.unwrap();
    assert!(
        note.contains("art. 40.2") && note.contains("1.840,00 €"),
        "{note}"
    );
}

#[test]
fn personas_fisicas_siempre_natural() {
    let v = vault();
    let mut p = TaxpayerProfile::autonomo();
    p.cierre_ejercicio = (Month::June, 30);
    v.set_setting("taxpayer_profile", &p).unwrap();
    manual_entry(
        &v,
        &entry(date!(2026 - 09 - 01), 100),
        datetime!(2026-10-01 9:00 UTC),
    )
    .unwrap();
    let f = fiscal_year(&v, 2026).unwrap();
    assert_eq!(
        (f.start, f.end),
        (date!(2026 - 01 - 01), date!(2026 - 12 - 31))
    );
}
