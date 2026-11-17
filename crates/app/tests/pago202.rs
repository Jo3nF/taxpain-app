//! 202 por la base del periodo (art. 40.3 LIS) desde la contabilidad, modalidad por ejercicio y
//! simulador de la opción. Sociedad inventada (Estudio Demo SL) con cifras redondas.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::record_entry;
use taxpain_app::invoicing;
use taxpain_app::models::pago202::{self, Fuente, Modalidad};
use taxpain_app::models::{self, ModelSettings, YearSettings};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::date;
use time::{Date, OffsetDateTime, Time};

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn at(d: Date) -> OffsetDateTime {
    d.with_time(Time::from_hms(10, 0, 0).unwrap()).assume_utc()
}

fn setup() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Estudio Demo SL".into(),
            address: Some(taxpain_core::invoice::Address {
                street: "Calle Inventada 1".into(),
                post_code: "46001".into(),
                town: "València".into(),
                province: "Valencia".into(),
                country: "ES".into(),
            }),
            person: None,
        },
        default_series: "A".into(),
        rectifying_series: "R".into(),
        simplified_series: "S".into(),
        simplified_rectifying_series: "RS".into(),
        language: Language::Es,
        default_vat: Rate::percent(21),
        default_withholding: None,
        outputs: Outputs {
            pdf: false,
            facturae: false,
            ubl: false,
        },
        destination: None,
        rounding: RoundingMode::PerRate,
        verifactu: VerifactuSettings {
            enabled: false,
            installation_id: "x".into(),
            production: false,
        },
        footer: None,
        activity: None,
        simplified_up_to_3000: false,
    };
    invoicing::save_settings(&v, &s, at(date!(2026 - 01 - 01))).unwrap();
    v
}

fn entry(v: &Vault, d: Date, lines: Vec<EntryLine>) {
    record_entry(
        v.connection(),
        &JournalEntry {
            date: d,
            concept: "Asiento de prueba".into(),
            origin: Origin::Manual,
            lines,
        },
        at(d),
    )
    .unwrap();
}

/// Ventas y gastos de 2026: 30.000 de resultado en el 1T, 60.000 de ventas con 1.000 de
/// retención en mayo, 20.000 de dividendos en junio y 50.000 de ventas en octubre. Cuota del IS
/// de los ejercicios anteriores: 20.000; bases negativas pendientes: 10.000.
fn empresa() -> Vault {
    let v = setup();
    entry(
        &v,
        date!(2026 - 02 - 10),
        vec![
            EntryLine::debit(acc("572"), eur(40_000)),
            EntryLine::credit(acc("705"), eur(40_000)),
        ],
    );
    entry(
        &v,
        date!(2026 - 02 - 20),
        vec![
            EntryLine::debit(acc("629"), eur(10_000)),
            EntryLine::credit(acc("572"), eur(10_000)),
        ],
    );
    entry(
        &v,
        date!(2026 - 05 - 10),
        vec![
            EntryLine::debit(acc("572"), eur(59_000)),
            EntryLine::debit(acc("473"), eur(1_000)),
            EntryLine::credit(acc("705"), eur(60_000)),
        ],
    );
    entry(
        &v,
        date!(2026 - 06 - 15),
        vec![
            EntryLine::debit(acc("572"), eur(20_000)),
            EntryLine::credit(acc("760"), eur(20_000)),
        ],
    );
    entry(
        &v,
        date!(2026 - 10 - 15),
        vec![
            EntryLine::debit(acc("572"), eur(50_000)),
            EntryLine::credit(acc("705"), eur(50_000)),
        ],
    );
    let mut years = std::collections::BTreeMap::new();
    years.insert(
        2026,
        YearSettings {
            corporate_tax_base: Some(eur(20_000)),
            corporate_tax_base_1p: Some(eur(20_000)),
            pending_losses: eur(10_000),
            ..YearSettings::default()
        },
    );
    models::save_settings(
        &v,
        &ModelSettings {
            cnae: "6201".into(),
            corporate_tax_rate: "25".into(),
            years,
            ..ModelSettings::default()
        },
        at(date!(2026 - 01 - 02)),
    )
    .unwrap();
    v
}

fn get(d: &models::Draft, b: u16) -> Money {
    d.boxes
        .iter()
        .find(|(k, _)| *k == b)
        .map_or(Money::ZERO, |(_, v)| *v)
}

#[test]
fn sin_elegir_va_por_la_cuota() {
    let v = empresa();
    let m = pago202::modalidad(&v, 2026).unwrap();
    assert_eq!(
        (m.elegida, m.efectiva, m.obligatoria),
        (None, Modalidad::Cuota, false)
    );
    let d = models::draft(&v, "202", 2026, 2).unwrap();
    assert_eq!(d.result, eur(3_600));
    assert_eq!(get(&d, 3), eur(3_600));
}

#[test]
fn borrador_por_la_base_de_los_tres_pagos() {
    let v = empresa();
    pago202::set_modalidad(&v, 2026, Some(Modalidad::Base), at(date!(2026 - 02 - 10))).unwrap();
    assert_eq!(
        pago202::modalidad(&v, 2026).unwrap().efectiva,
        Modalidad::Base
    );
    // 1P: 30.000 − 10.000 de bases negativas = 20.000 al 17 % = 3.400.
    let p1 = models::draft(&v, "202", 2026, 1).unwrap();
    for (b, m) in [
        (4, eur(30_000)),
        (13, eur(30_000)),
        (14, eur(10_000)),
        (16, eur(20_000)),
        (17, eur(17)),
        (18, eur(3_400)),
        (34, eur(3_400)),
    ] {
        assert_eq!(get(&p1, b), m, "1P [{b}]");
    }
    assert_eq!(get(&p1, 3), Money::ZERO);
    assert!(
        p1.file.starts_with("<T202020261P0000>"),
        "{}",
        &p1.file[..20]
    );
    // 2P: 110.000 − 10.000 = 100.000 → 17.000 − 1.000 de retenciones − 3.400 del 1P = 12.600, con
    // aviso de los dividendos.
    let p2 = models::draft(&v, "202", 2026, 2).unwrap();
    assert_eq!(get(&p2, 13), eur(110_000));
    assert_eq!(get(&p2, 28), eur(1_000));
    assert_eq!(get(&p2, 30), eur(3_400));
    assert_eq!(p2.result, eur(12_600));
    assert!(
        p2.warnings.iter().any(|w| w.contains("art. 21 LIS")),
        "{:?}",
        p2.warnings
    );
    // El 95 % exento de los dividendos fijado en [08] del 2P: 91.000 − 10.000 = 81.000 → 13.770
    // − 1.000 − 3.400 = 9.370.
    models::set_adjustment(
        &v,
        "202",
        2026,
        2,
        8,
        Some(eur(19_000)),
        at(date!(2026 - 10 - 05)),
    )
    .unwrap();
    let p2 = models::draft(&v, "202", 2026, 2).unwrap();
    assert_eq!(get(&p2, 8), eur(19_000));
    assert_eq!(p2.result, eur(9_370));
    // Una casilla que es un total no se fija.
    assert!(
        models::set_adjustment(
            &v,
            "202",
            2026,
            2,
            34,
            Some(eur(1)),
            at(date!(2026 - 10 - 05))
        )
        .is_err()
    );
    // 3P: 160.000 − 10.000 = 150.000 → 25.500 − 1.000 − (3.400 + 9.370) = 11.730.
    let p3 = models::draft(&v, "202", 2026, 3).unwrap();
    assert_eq!(get(&p3, 30), eur(12_770));
    assert_eq!(p3.result, eur(11_730));
    // El 200 resta los tres pagos por la base.
    let d200 = models::draft(&v, "200", 2026, 0).unwrap();
    assert_eq!(get(&d200, 601), eur(3_400));
    assert_eq!(get(&d200, 603), eur(9_370));
    assert_eq!(get(&d200, 605), eur(11_730));
}

#[test]
fn con_mas_de_seis_millones_la_base_es_obligatoria() {
    let v = empresa();
    pago202::set_modalidad(&v, 2026, Some(Modalidad::Cuota), at(date!(2026 - 02 - 10))).unwrap();
    let mut s = models::settings(&v).unwrap();
    s.turnover_over_6m = true;
    models::save_settings(&v, &s, at(date!(2026 - 02 - 10))).unwrap();
    let m = pago202::modalidad(&v, 2026).unwrap();
    assert!(m.obligatoria);
    assert_eq!(m.efectiva, Modalidad::Base);
    assert!(
        m.aviso.as_deref().unwrap().contains("no vale"),
        "{:?}",
        m.aviso
    );
    let d = models::draft(&v, "202", 2026, 2).unwrap();
    assert!(!d.file.is_empty());
    assert_eq!(d.result, eur(12_600));
    assert!(
        d.warnings[0].contains("obligatoriamente"),
        "{:?}",
        d.warnings
    );
}

#[test]
fn simulador_con_la_contabilidad_del_año() {
    let v = empresa();
    // A mitad de diciembre los tres periodos están cerrados: 3.400 + 12.600 + 8.500 por la base
    // frente a 3 × 3.600 por la cuota.
    let s = pago202::simulate(&v, 2026, date!(2026 - 12 - 15)).unwrap();
    assert_eq!(s.periodos.len(), 3);
    assert!(s.periodos.iter().all(|p| p.fuente == Fuente::Contabilidad));
    assert_eq!(
        s.periodos.iter().map(|p| p.base).collect::<Vec<_>>(),
        [eur(3_400), eur(12_600), eur(8_500)]
    );
    assert_eq!(s.total_cuota, Some(eur(10_800)));
    assert_eq!(s.total_base, eur(24_500));
    assert!(
        s.conclusion.contains("Por la cuota pagas"),
        "{}",
        s.conclusion
    );
    assert_eq!(
        (s.opcion_desde, s.opcion_hasta),
        (date!(2026 - 02 - 01), date!(2026 - 02 - 28))
    );
    assert!(
        s.notas.iter().any(|n| n.contains("ya pasó")),
        "{:?}",
        s.notas
    );
}

#[test]
fn simulador_en_febrero_con_el_año_anterior() {
    let v = empresa();
    // 2025: 12.000 de ventas en febrero. En febrero de 2026 los tres pagos se estiman con los
    // mismos meses de 2025: 12.000 − 10.000 de bases negativas = 2.000 al 17 % = 340 en el 1P.
    entry(
        &v,
        date!(2025 - 02 - 01),
        vec![
            EntryLine::debit(acc("572"), eur(12_000)),
            EntryLine::credit(acc("705"), eur(12_000)),
        ],
    );
    let s = pago202::simulate(&v, 2026, date!(2026 - 02 - 10)).unwrap();
    assert!(s.periodos.iter().all(|p| p.fuente == Fuente::AnoAnterior));
    assert_eq!(s.periodos[0].base, eur(340));
    assert_eq!(s.total_base, eur(340));
    assert!(
        s.conclusion.contains("Por la base pagarías"),
        "{}",
        s.conclusion
    );
    assert!(s.conclusion.contains("036"), "{}", s.conclusion);
    assert!(
        !s.notas.iter().any(|n| n.contains("ya pasó")),
        "{:?}",
        s.notas
    );
    assert!(
        s.notas.iter().any(|n| n.contains("previsión")),
        "{:?}",
        s.notas
    );
}

#[test]
fn la_modalidad_se_guarda_por_ejercicio_y_queda_en_la_auditoria() {
    let v = empresa();
    pago202::set_modalidad(&v, 2027, Some(Modalidad::Base), at(date!(2027 - 02 - 03))).unwrap();
    assert_eq!(
        pago202::modalidad(&v, 2026).unwrap().efectiva,
        Modalidad::Cuota
    );
    assert_eq!(
        pago202::modalidad(&v, 2027).unwrap().efectiva,
        Modalidad::Base
    );
    // Los demás datos del ejercicio no se pierden al guardar la modalidad.
    let s = models::settings(&v).unwrap();
    assert_eq!(s.years[&2026].pending_losses, eur(10_000));
    let n: i64 = v
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE action = 'models.202.modalidad'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
}
