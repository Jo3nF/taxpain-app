//! Panel financiero: cifras del ejercicio a partir de la contabilidad.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::manual_entry;
use taxpain_app::dashboard::dashboard;
use taxpain_app::invoicing::{self, InvoiceDraft, issue};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

fn setup() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("12345678Z".into()),
            name: "Carmen".into(),
            address: addr(),
            person: None,
        },
        default_series: "A".into(),
        rectifying_series: "R".into(),
        simplified_series: "S".into(),
        simplified_rectifying_series: "RS".into(),
        language: Language::Es,
        default_vat: Rate::percent(21),
        default_withholding: Some(Rate::percent(15)),
        outputs: Outputs {
            pdf: true,
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
    invoicing::save_settings(&v, &s, datetime!(2026-01-01 9:00 UTC)).unwrap();
    v
}

fn draft(nif: &str, name: &str, euros: i64, date: time::Date) -> InvoiceDraft {
    InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: date,
        operation_date: None,
        recipient: Some(Party {
            id: PartyId::Nif(nif.into()),
            name: name.into(),
            address: addr(),
            person: None,
        }),
        lines: vec![InvoiceLine {
            description: "Consultoría".into(),
            quantity_milli: 1000,
            unit_price: Money::from_euros(euros, 0),
            discount: Rate::ZERO,
            vat: VatTreatment::Taxed {
                rate: Rate::percent(21),
            },
            surcharge: None,
            withholding: Some(Rate::percent(15)),
            advance: false,
        }],
        rectified: vec![],
        rectified_amounts: None,
        description: "Servicios".into(),
        notes: None,
        currency: None,
        oss: None,
    }
}

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

#[test]
fn cifras_del_ejercicio_y_pendientes() {
    let v = setup();
    let now = datetime!(2026-05-20 10:00 UTC);
    let entry = |date, concept: &str, lines| JournalEntry {
        date,
        concept: concept.into(),
        origin: Origin::Manual,
        lines,
    };
    manual_entry(
        &v,
        &entry(
            date!(2026 - 01 - 02),
            "Aportación inicial",
            vec![
                EntryLine::debit(acc("5720001"), eur(3000)),
                EntryLine::credit(acc("102"), eur(3000)),
            ],
        ),
        now,
    )
    .unwrap();
    issue(
        &v,
        &draft("B12345674", "Cliente Uno SL", 1000, date!(2026 - 03 - 10)),
        now,
        true,
    )
    .unwrap();
    issue(
        &v,
        &draft("A22222228", "Cliente Dos SA", 500, date!(2026 - 04 - 15)),
        now,
        true,
    )
    .unwrap();
    // Alquiler de mayo pagado por banco: 200 + 42 de IVA.
    manual_entry(
        &v,
        &entry(
            date!(2026 - 05 - 05),
            "Alquiler mayo",
            vec![
                EntryLine::debit(acc("6210000"), eur(200)),
                EntryLine::debit(acc("4720000"), eur(42)),
                EntryLine::credit(acc("5720001"), eur(242)),
            ],
        ),
        now,
    )
    .unwrap();

    let d = dashboard(&v, 2026, 5).unwrap();
    assert_eq!(d.months.len(), 12);
    assert_eq!(d.months[2].income, eur(1000));
    assert_eq!(d.months[3].income, eur(500));
    assert_eq!(d.months[4].expenses, eur(200));
    assert_eq!(d.income, eur(1500));
    assert_eq!(d.expenses, eur(200));
    assert_eq!(d.profit, eur(1300));
    assert_eq!(d.cash, eur(2758));
    // Cobros pendientes: total de las facturas menos la retención (1000·1,06 + 500·1,06).
    assert_eq!(d.receivables, eur(1590));
    assert_eq!(d.payables, Money::ZERO);
    // Persona jurídica en los ajustes (sin `person`): sólo el 303 del 2T.
    assert_eq!(d.estimates.len(), 1, "{:?}", d.estimates);
    assert_eq!(d.estimates[0].model, "303");
    assert_eq!(d.estimates[0].quarter, 2);
    assert_eq!(d.estimates[0].result, eur(105));
    assert_eq!(d.pending.received_to_review, 0);

    // Otro ejercicio: vacío.
    let empty = dashboard(&v, 2025, 12).unwrap();
    assert_eq!(empty.income, Money::ZERO);
    assert_eq!(empty.cash, Money::ZERO);
    // …porque no hay contabilidad, no porque no haya actividad.
    assert!(empty.sin_contabilidad);
    assert!(!d.sin_contabilidad);
    assert!(d.presentado.is_empty());
}

/// Guarda declaraciones de la AEAT con sus casillas (JSON, en céntimos); `None`: sin leer.
fn presentadas(v: &Vault, year: i32, list: &[(&str, &str, Option<&str>, Option<&str>)]) {
    use taxpain_app::presentado_aeat::{self, Answer, Presented};
    use taxpain_vault::rusqlite::params;
    let list: Vec<(Presented, Option<&str>)> = list
        .iter()
        .enumerate()
        .map(|(i, (model, period, previous, boxes))| {
            let receipt = format!("{model}{year}{period}{i:04}");
            (
                Presented {
                    model: (*model).into(),
                    year,
                    period: (*period).into(),
                    csv: format!("{receipt:X<16}"),
                    file_number: String::new(),
                    receipt,
                    previous_receipt: previous.map(Into::into),
                    presented_at: format!("{year}-10-01T10:00:{i:02}"),
                },
                *boxes,
            )
        })
        .collect();
    let a = Answer {
        models: list.iter().map(|(x, _)| x.model.clone()).collect(),
        list: list.iter().map(|(x, _)| x.clone()).collect(),
        ..Answer::default()
    };
    presentado_aeat::record(v, "12345678Z", year, &a, datetime!(2026-10-27 10:00 UTC)).unwrap();
    for (x, boxes) in list {
        if let Some(b) = boxes {
            v.connection()
                .execute(
                    "UPDATE aeat_filing SET boxes = ?1, boxes_source = 'fichero' WHERE csv = ?2",
                    params![b, x.csv],
                )
                .unwrap();
        }
    }
}

#[test]
fn sin_contabilidad_lo_presentado_a_la_aeat() {
    let v = setup();
    let now = datetime!(2026-10-28 10:00 UTC);
    // Sólo el asiento de apertura: sigue sin haber contabilidad del año.
    manual_entry(
        &v,
        &JournalEntry {
            date: date!(2026 - 01 - 01),
            concept: "Apertura".into(),
            origin: Origin::Apertura,
            lines: vec![
                EntryLine::debit(acc("5720001"), eur(3000)),
                EntryLine::credit(acc("102"), eur(3000)),
            ],
        },
        now,
    )
    .unwrap();
    let d = dashboard(&v, 2026, 10).unwrap();
    assert!(d.sin_contabilidad);
    assert!(d.presentado.is_empty(), "{:?}", d.presentado);

    // Lo presentado de 2026: tres 303 (el 2T con una complementaria y el 3T a compensar), dos
    // 111 leídos y uno sin
    // leer, y los 202 de abril y octubre. Y un 303 de 2025, que no cuenta.
    presentadas(
        &v,
        2025,
        &[("303", "4T", None, Some(r#"{"casillas":{"27":999900}}"#))],
    );
    presentadas(
        &v,
        2026,
        &[
            (
                "303",
                "1T",
                None,
                Some(r#"{"casillas":{"27":100000,"45":40000,"69":60000,"71":60000}}"#),
            ),
            (
                "303",
                "2T",
                None,
                Some(r#"{"casillas":{"27":100000,"45":40000,"69":60000,"71":60000}}"#),
            ),
            // Complementaria del 2T: 800 en total, ya ingresados 600 ([70]); su [71] es 200.
            (
                "303",
                "2T",
                Some("30320262T0001"),
                Some(r#"{"casillas":{"27":120000,"45":40000,"69":80000,"70":60000,"71":20000}}"#),
            ),
            (
                "303",
                "3T",
                None,
                // El 3T sale a compensar: 200.
                Some(r#"{"casillas":{"27":90000,"45":110000,"69":-20000,"71":-20000}}"#),
            ),
            ("111", "1T", None, Some(r#"{"casillas":{"28":30000}}"#)),
            ("111", "2T", None, Some(r#"{"casillas":{"28":30000}}"#)),
            ("111", "3T", None, None),
            ("202", "1P", None, Some(r#"{"casillas":{"3":50000}}"#)),
            ("202", "2P", None, Some(r#"{"casillas":{"3":70000}}"#)),
        ],
    );
    let d = dashboard(&v, 2026, 10).unwrap();
    assert!(d.sin_contabilidad);
    let c = |id: &str| {
        d.presentado
            .iter()
            .find(|c| c.id == id)
            .unwrap_or_else(|| panic!("sin {id}: {:?}", d.presentado))
    };
    assert_eq!(c("iva_devengado").valor, eur(3100));
    assert_eq!(c("iva_devengado").fuente, "suma de los 303 [27]");
    assert_eq!(c("iva_devengado").periodos, "1T a 3T");
    assert_eq!(c("iva_deducible").valor, eur(1900));
    // El resultado del 2T es el del periodo entero (800), no sólo el de la complementaria; lo
    // que salió a compensar va aparte (no resta de lo ingresado).
    assert_eq!(c("iva_a_ingresar").valor, eur(1400));
    assert_eq!(c("iva_a_ingresar").periodos, "1T y 2T");
    assert_eq!(c("iva_a_compensar").valor, eur(200));
    assert_eq!(c("iva_a_compensar").periodos, "3T");
    assert_eq!(
        c("iva_a_compensar").fuente,
        "suma de los 303 [71] negativos"
    );
    assert_eq!(c("ret_trabajo").valor, eur(600));
    assert_eq!(
        c("ret_trabajo").fuente,
        "suma de los 111 [28] (2 de 3 periodos leídos)"
    );
    assert_eq!(c("ret_trabajo").periodos, "1T y 2T");
    assert_eq!(c("pagos_fraccionados").valor, eur(1200));
    assert_eq!(c("pagos_fraccionados").periodos, "1P y 2P");
    // Sin 115, no sale.
    assert!(d.presentado.iter().all(|c| c.id != "ret_alquiler"));
    // Las cifras de la contabilidad siguen a cero (no se inventan).
    assert_eq!(d.income, Money::ZERO);

    // Con un asiento del año ya hay contabilidad: el panel de siempre.
    manual_entry(
        &v,
        &JournalEntry {
            date: date!(2026 - 02 - 03),
            concept: "Alquiler".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("6210000"), eur(100)),
                EntryLine::credit(acc("5720001"), eur(100)),
            ],
        },
        now,
    )
    .unwrap();
    let d = dashboard(&v, 2026, 10).unwrap();
    assert!(!d.sin_contabilidad);
    assert!(d.presentado.is_empty());
    assert_eq!(d.expenses, eur(100));
}

#[test]
fn vencidas_sin_abonos_y_presupuestos_de_cualquier_año() {
    use taxpain_app::dashboard::receivables;
    use taxpain_core::invoice::{Rectification, RectificationMethod};
    let v = setup();
    let now = datetime!(2026-05-20 10:00 UTC);
    // A-1 (1.060 a cobrar) sigue debiéndose; A-2 se abona entera con una rectificativa.
    issue(
        &v,
        &draft("B12345674", "Cliente Uno SL", 1000, date!(2026 - 03 - 10)),
        now,
        true,
    )
    .unwrap();
    let a2 = issue(
        &v,
        &draft("A22222228", "Cliente Dos SA", 500, date!(2026 - 03 - 15)),
        now,
        true,
    )
    .unwrap();
    let mut r = draft("A22222228", "Cliente Dos SA", -500, date!(2026 - 03 - 20));
    r.kind = InvoiceKind::Rectificativa(Rectification {
        code: 1,
        method: RectificationMethod::Diferencias,
    });
    r.rectified = vec![(a2.number.clone(), date!(2026 - 03 - 15))];
    issue(&v, &r, now, true).unwrap();
    let got = receivables(&v, date!(2026 - 05 - 20)).unwrap();
    assert_eq!(
        got.overdue_invoices, 1,
        "ni el abono ni la abonada: {got:?}"
    );
    assert_eq!(got.overdue_amount, Money::from_cents(106_000));

    // Un presupuesto del año pasado aceptado y sin facturar también cuenta.
    let q = taxpain_app::quotes::create(
        &v,
        &taxpain_app::quotes::QuoteInput {
            kind: "presupuesto".into(),
            draft: draft("B12345674", "Cliente Uno SL", 2000, date!(2025 - 12 - 01)),
            valid_until: None,
            language: None,
        },
        datetime!(2025-12-01 10:00 UTC),
    )
    .unwrap();
    taxpain_app::quotes::set_status(&v, q.id, "enviado", now).unwrap();
    taxpain_app::quotes::set_status(&v, q.id, "aceptado", now).unwrap();
    let got = receivables(&v, date!(2026 - 05 - 20)).unwrap();
    assert_eq!(got.accepted_quotes, 1);
    assert!(got.accepted_total.cents() > 0);
}

#[test]
fn el_202_de_mas_de_6_millones_no_se_estima() {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let now = datetime!(2026-01-01 9:00 UTC);
    let mut s = invoicing::settings(&setup()).unwrap().unwrap();
    s.issuer = Party {
        id: PartyId::Nif("B12345674".into()),
        name: "Grande SA".into(),
        address: addr(),
        person: None,
    };
    invoicing::save_settings(&v, &s, now).unwrap();
    let mut m = taxpain_app::models::settings(&v).unwrap();
    m.turnover_over_6m = true;
    taxpain_app::models::save_settings(&v, &m, now).unwrap();
    // El 202 va por la base del periodo (art. 40.3 LIS): sin resultado en la contabilidad no hay
    // pago que estimar (y no sale el 18 % de la cuota).
    let d = dashboard(&v, 2026, 10).unwrap();
    assert!(
        !d.estimates.iter().any(|e| e.model == "202"),
        "{:?}",
        d.estimates
    );
    let b = taxpain_app::models::draft(&v, "202", 2026, 2).unwrap();
    assert!(b.boxes.iter().any(|(k, _)| *k == 34), "{:?}", b.boxes);
    assert_eq!(b.result, Money::ZERO);
}

/// Con VERI*FACTU activo pero sin envío a producción, una empresa real no ve «registros sin
/// enviar» (sólo se puede enviar a pruebas); la empresa de pruebas sí.
#[test]
fn registros_sin_enviar_solo_si_se_pueden_enviar() {
    let v = setup();
    let now = datetime!(2026-05-20 10:00 UTC);
    let mut s = invoicing::settings(&v).unwrap().unwrap();
    s.verifactu.enabled = true;
    invoicing::save_settings(&v, &s, now).unwrap();
    issue(
        &v,
        &draft("B12345674", "EMPRESA DEMO SL", 100, date!(2026 - 05 - 20)),
        now,
        true,
    )
    .unwrap();
    assert_eq!(dashboard(&v, 2026, 5).unwrap().pending.verifactu_unsent, 0);
    taxpain_app::verifactu_lab::mark(&v, now).unwrap();
    assert_eq!(dashboard(&v, 2026, 5).unwrap().pending.verifactu_unsent, 1);
}

/// Domicilio inventado: la factura completa lo exige (art. 6.1.e RD 1619/2012).
fn addr() -> Option<taxpain_core::invoice::Address> {
    Some(taxpain_core::invoice::Address {
        street: "Calle Inventada 1".into(),
        post_code: "46001".into(),
        town: "València".into(),
        province: "Valencia".into(),
        country: "ES".into(),
    })
}
