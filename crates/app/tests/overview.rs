//! Resumen de una empresa: plazos próximos y vencidos sin presentar.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::overview::company_status;
use taxpain_core::calendar::TaxpayerProfile;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::date;

#[test]
fn proximos_y_vencidos_sin_presentar() {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    assert!(
        !company_status(&v, date!(2026 - 10 - 05))
            .unwrap()
            .has_profile
    );
    v.set_setting("taxpayer_profile", &TaxpayerProfile::autonomo())
        .unwrap();
    // El 303 del 2T se presentó desde TaxPain; el 130 del 2T no.
    v.connection()
        .execute(
            "INSERT INTO tax_return(model, year, period, environment, declaration, result, boxes,
               file_sha256, file, csv, receipt_number, receipt, presented_at)
             VALUES ('303', 2026, '2T', 'produccion', 'I', 100, '[]', 'x', 'x', 'CSV', '1', '{}',
               '2026-07-15T10:00:00Z')",
            [],
        )
        .unwrap();
    let s = company_status(&v, date!(2026 - 10 - 05)).unwrap();
    let keys = |l: &[taxpain_app::overview::DeadlineStatus]| {
        l.iter()
            .map(|d| format!("{} {}", d.model, d.period))
            .collect::<Vec<_>>()
    };
    assert!(
        keys(&s.upcoming).contains(&"303 3T".to_owned()),
        "{:?}",
        keys(&s.upcoming)
    );
    assert!(keys(&s.upcoming).contains(&"130 3T".to_owned()));
    assert_eq!(keys(&s.overdue), ["130 2T"]);
    // Con título legible para la pantalla.
    assert_eq!(s.overdue[0].label, "Modelo 130 · 2.º trimestre 2026");
    assert!(
        s.upcoming
            .iter()
            .any(|d| d.label == "Modelo 303 · 3.er trimestre 2026")
    );
}

/// Una empresa que se empieza a usar hoy no sale con lo ya vencido como «sin presentar».
#[test]
fn lo_vencido_antes_de_empezar_no_sale() {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    v.set_setting("taxpayer_profile", &TaxpayerProfile::autonomo())
        .unwrap();
    taxpain_vault::audit::append(
        v.connection(),
        &time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap(),
        taxpain_vault::audit::Actor::User,
        "company.update",
        "EMPRESA DEMO SL",
        &serde_json::json!({}),
    )
    .unwrap();
    let today = time::OffsetDateTime::now_utc().date();
    let s = company_status(&v, today).unwrap();
    assert!(s.overdue.is_empty(), "{:?}", s.overdue);
}

/// Lo que la AEAT tiene según «Comprobar lo presentado» (lo presentó la gestoría) no sale como
/// vencido sin presentar, y se sabe cuándo se comprobó.
#[test]
fn lo_presentado_segun_la_aeat_no_sale_como_vencido() {
    use taxpain_app::presentado_aeat::{self, Answer, Presented};
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    v.set_setting("taxpayer_profile", &TaxpayerProfile::autonomo())
        .unwrap();
    let today = date!(2026 - 10 - 05);
    let before = company_status(&v, today).unwrap();
    assert_eq!(before.aeat_checked_at, None, "nunca comprobado");
    assert!(
        before
            .overdue
            .iter()
            .any(|d| d.model == "130" && d.period == "2T")
    );
    let p = |model: &str, csv: &str, receipt: &str| Presented {
        model: model.into(),
        year: 2026,
        period: "2T".into(),
        csv: csv.into(),
        file_number: String::new(),
        receipt: receipt.into(),
        previous_receipt: None,
        presented_at: "2026-07-14T10:00:00".into(),
    };
    let a = Answer {
        models: vec!["130".into(), "303".into()],
        list: vec![
            p("130", "AAAAAAAAAAAAAAA1", "1300000000001"),
            p("303", "AAAAAAAAAAAAAAA2", "3030000000001"),
        ],
        ..Answer::default()
    };
    presentado_aeat::record(
        &v,
        "12345678Z",
        2026,
        &a,
        time::macros::datetime!(2026-10-04 09:00 UTC),
    )
    .unwrap();
    let s = company_status(&v, today).unwrap();
    assert!(
        !s.overdue.iter().any(|d| d.period == "2T"),
        "{:?}",
        s.overdue
    );
    assert_eq!(
        s.aeat_checked_at.as_deref().map(|x| &x[..10]),
        Some("2026-10-04")
    );
}
