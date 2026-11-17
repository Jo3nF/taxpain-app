//! Cartera de cobros y pagos: vencimientos, cobro en caja, pago, deshacer y banco.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::{journal, trial_balance};
use taxpain_app::bank::{import_n43, movements, reconcile_invoice, suggestions};
use taxpain_app::cartera::{
    PartyTerms, PaymentTerms, auto_schedule, list, parties, set_due_date, set_party_terms,
    set_schedule, set_terms, settle, to_pay_day, unsettle,
};
use taxpain_app::invoicing::{self, InvoiceDraft, issue};
use taxpain_app::received::{AssetClass, Nature, Reviewed, Source, ingest_file, review};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
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
        default_withholding: None,
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

fn draft(euros: i64, date: time::Date) -> InvoiceDraft {
    InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: date,
        operation_date: None,
        recipient: Some(Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Cliente Ejemplo SA".into(),
            address: addr(),
            person: None,
        }),
        lines: vec![InvoiceLine {
            description: "Servicios".into(),
            quantity_milli: 1000,
            unit_price: Money::from_euros(euros, 0),
            discount: Rate::ZERO,
            vat: VatTreatment::Taxed {
                rate: Rate::percent(21),
            },
            surcharge: None,
            withholding: None,
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

#[test]
fn vencimientos_cobros_y_pagos() {
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    let a = issue(&v, &draft(1000, date!(2026 - 08 - 01)), now, true).unwrap();
    let b = issue(&v, &draft(500, date!(2026 - 09 - 20)), now, true).unwrap();
    let rec = ingest_file(
        &v,
        "f.xml",
        b"<?xml version=\"1.0\"?><x/>",
        Source::Manual,
        now,
    )
    .unwrap();
    review(
        &v,
        rec.id,
        &Reviewed {
            supplier_nif: "A22222228".into(),
            supplier_name: "Proveedor SA".into(),
            number: "P-7".into(),
            issue_date: date!(2026 - 09 - 01),
            base: Money::from_euros(200, 0),
            vat: Money::from_euros(42, 0),
            withholding: Money::ZERO,
            total: Money::from_euros(242, 0),
            deductible_pct: 100,
            capital_good: false,
            nature: Nature::General,
            asset_class: None::<AssetClass>,
            vat_regime: Default::default(),
            supplier_country: None,
            self_assessed_rate: None,
            intra_goods: false,
            cash_basis: false,
            import_dua: false,
            prorata_use: Default::default(),
        },
        now,
    )
    .unwrap();

    let today = date!(2026 - 10 - 13);
    let c = list(&v, today).unwrap();
    assert_eq!(c.len(), 3);
    // 1/8 + 30 días = 31/8: vencida hace 43 días.
    let first = c
        .iter()
        .find(|i| i.invoice_id == a.id && i.kind == "emitida")
        .unwrap();
    assert_eq!(first.due_date, date!(2026 - 08 - 31));
    assert_eq!(first.overdue_days, 43);
    assert_eq!(first.amount, Money::from_euros(1210, 0));
    assert_eq!(first.status, "pendiente");
    // Límite de pagos en efectivo (art. 7 Ley 7/2012): 1.210 € sí llega; 605 € no.
    assert!(first.cash_limited);
    assert!(
        !c.iter()
            .find(|i| i.invoice_id == b.id && i.kind == "emitida")
            .unwrap()
            .cash_limited
    );
    // Orden por vencimiento.
    assert!(c.windows(2).all(|w| w[0].due_date <= w[1].due_date));

    // Plazos: máximo legal 60 días.
    assert!(
        set_terms(
            &v,
            &PaymentTerms {
                customers_days: 90,
                suppliers_days: 30
            }
        )
        .is_err()
    );
    set_terms(
        &v,
        &PaymentTerms {
            customers_days: 60,
            suppliers_days: 30,
        },
    )
    .unwrap();
    set_due_date(&v, "emitida", b.id, 1, date!(2026 - 12 - 31)).unwrap();
    let c = list(&v, today).unwrap();
    let get = |id: i64, kind: &str| {
        c.iter()
            .find(|i| i.invoice_id == id && i.kind == kind)
            .unwrap()
            .clone()
    };
    assert_eq!(get(a.id, "emitida").due_date, date!(2026 - 09 - 30));
    assert_eq!(get(b.id, "emitida").due_date, date!(2026 - 12 - 31));
    assert_eq!(get(b.id, "emitida").overdue_days, 0);

    // Cobro en caja de la primera y pago por banco del proveedor.
    assert!(
        settle(&v, "emitida", a.id, 1, today, "629", now).is_err(),
        "no es tesorería"
    );
    settle(&v, "emitida", a.id, 1, today, "570", now).unwrap();
    assert!(
        settle(&v, "emitida", a.id, 1, today, "570", now).is_err(),
        "dos veces no"
    );
    settle(&v, "recibida", rec.id, 1, today, "5720001", now).unwrap();
    let c = list(&v, today).unwrap();
    assert_eq!(c.iter().filter(|i| i.status == "registrada").count(), 2);
    let tb = trial_balance(&v, 2026, None).unwrap();
    let bal = |acc: &str| {
        tb.iter()
            .find(|r| r.account.as_str() == acc)
            .map(|r| r.debit - r.credit)
            .unwrap_or(Money::ZERO)
    };
    assert_eq!(bal("570"), Money::from_euros(1210, 0));
    assert_eq!(bal("5720001"), -Money::from_euros(242, 0));
    // El cliente queda sólo con la segunda factura pendiente.
    assert_eq!(bal("4300001"), Money::from_euros(605, 0));
    assert!(
        journal(&v, 2026)
            .unwrap()
            .iter()
            .any(|e| matches!(e.entry.origin, taxpain_core::accounting::Origin::Cartera(_)))
    );

    // Deshacer el cobro: vuelve a pendiente y desaparece el asiento.
    assert!(unsettle(&v, "emitida", a.id, 1, now, false).is_err());
    unsettle(&v, "emitida", a.id, 1, now, true).unwrap();
    let c = list(&v, today).unwrap();
    assert_eq!(c.iter().filter(|i| i.status == "pendiente").count(), 2);
    let tb = trial_balance(&v, 2026, None).unwrap();
    assert!(
        tb.iter()
            .all(|r| r.account.as_str() != "570" || r.debit == r.credit)
    );
    assert!(v.verify_audit().is_ok());
}

#[test]
fn dia_de_pago_y_reparto() {
    assert_eq!(to_pay_day(date!(2026 - 01 - 31), 15), date!(2026 - 02 - 15));
    assert_eq!(to_pay_day(date!(2026 - 01 - 10), 31), date!(2026 - 01 - 31));
    assert_eq!(to_pay_day(date!(2026 - 02 - 20), 30), date!(2026 - 02 - 28));
    assert_eq!(to_pay_day(date!(2026 - 12 - 20), 5), date!(2027 - 01 - 05));
    assert_eq!(to_pay_day(date!(2026 - 03 - 05), 5), date!(2026 - 03 - 05));
    let s = auto_schedule(
        date!(2026 - 09 - 01),
        Money::from_cents(10001),
        &[30, 60],
        None,
    );
    assert_eq!(
        s,
        vec![
            (date!(2026 - 10 - 01), Money::from_cents(5000)),
            (date!(2026 - 10 - 31), Money::from_cents(5001))
        ]
    );
    // Rectificativas: importes negativos.
    let s = auto_schedule(
        date!(2026 - 09 - 01),
        Money::from_cents(-301),
        &[0, 30, 60],
        None,
    );
    assert_eq!(
        s.iter().map(|x| x.1.cents()).collect::<Vec<_>>(),
        vec![-100, -100, -101]
    );
}

const N43: &[u8] = include_bytes!("../../../fixtures/accounting/n43-ejemplo.txt");

#[test]
fn condiciones_del_cliente_y_vencimientos_fraccionados() {
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    let today = date!(2026 - 10 - 13);
    // Una factura antigua cobrada entera antes de poner condiciones al cliente.
    let old = issue(&v, &draft(100, date!(2026 - 07 - 01)), now, true).unwrap();
    settle(&v, "emitida", old.id, 1, date!(2026 - 07 - 20), "570", now).unwrap();

    let p = parties(&v).unwrap();
    assert_eq!(p.len(), 1);
    assert_eq!(
        (p[0].account.as_str(), p[0].role.as_str()),
        ("4300001", "cliente")
    );
    assert!(p[0].terms.is_none());
    let terms = |days: Vec<u16>| PartyTerms {
        days,
        pay_day: Some(5),
        method: "recibo".into(),
        treasury: Some("5720001".into()),
        iban: None,
        mandate_ref: None,
        mandate_date: None,
        key_190: None,
    };
    assert!(set_party_terms(&v, "4300001", Some(&terms(vec![60, 30])), now).is_err());
    assert!(set_party_terms(&v, "4300001", Some(&terms(vec![30, 90])), now).is_err());
    assert!(set_party_terms(&v, "6290001", Some(&terms(vec![30])), now).is_err());
    let mut bad = terms(vec![30]);
    bad.treasury = Some("430".into());
    assert!(set_party_terms(&v, "4300001", Some(&bad), now).is_err());
    set_party_terms(&v, "4300001", Some(&terms(vec![30, 60])), now).unwrap();
    assert_eq!(parties(&v).unwrap()[0].terms, Some(terms(vec![30, 60])));

    // 2.420 € del 1/9 a 30 y 60 días, día de pago 5: 5/10 y 5/11.
    let inv = issue(&v, &draft(2000, date!(2026 - 09 - 01)), now, true).unwrap();
    let c = list(&v, today).unwrap();
    let of = |c: &[taxpain_app::cartera::DueItem], id: i64| {
        c.iter()
            .filter(|i| i.invoice_id == id && i.kind == "emitida")
            .cloned()
            .collect::<Vec<_>>()
    };
    // La antigua sigue con un solo vencimiento, cobrado.
    let o = of(&c, old.id);
    assert_eq!(o.len(), 1);
    assert_eq!(o[0].status, "registrada");
    let i = of(&c, inv.id);
    assert_eq!(i.len(), 2);
    assert_eq!(
        i.iter()
            .map(|x| (x.seq, x.due_date, x.amount))
            .collect::<Vec<_>>(),
        vec![
            (1, date!(2026 - 10 - 05), Money::from_euros(1210, 0)),
            (2, date!(2026 - 11 - 05), Money::from_euros(1210, 0))
        ]
    );
    assert!(i.iter().all(|x| x.installments == 2 && !x.fixed));
    assert_eq!(i[0].method.as_deref(), Some("recibo"));
    assert_eq!(i[0].treasury.as_deref(), Some("5720001"));
    assert_eq!(i[0].overdue_days, 8);

    // El ingreso de 1.210 € del 1/10 casa con cualquiera de los dos; primero el más cercano.
    import_n43(&v, N43, now).unwrap();
    let cobro = movements(&v, true)
        .unwrap()
        .into_iter()
        .find(|m| m.amount == Money::from_euros(1210, 0))
        .unwrap();
    let sug = suggestions(&v, cobro.id).unwrap();
    assert_eq!(
        sug.iter().map(|s| (s.id, s.seq)).collect::<Vec<_>>(),
        vec![(inv.id, 1), (inv.id, 2)]
    );
    reconcile_invoice(&v, cobro.id, "emitida", inv.id, Some(1), now).unwrap();

    // Cambiar ahora las condiciones no mueve la factura: sus vencimientos quedaron fijados.
    set_party_terms(&v, "4300001", Some(&terms(vec![0])), now).unwrap();
    let i = of(&list(&v, today).unwrap(), inv.id);
    assert_eq!(i.len(), 2);
    assert!(i.iter().all(|x| x.fixed));
    assert_eq!(
        (i[0].status.as_str(), i[1].status.as_str()),
        ("banco", "pendiente")
    );
    assert!(
        reconcile_invoice(&v, cobro.id, "emitida", inv.id, Some(2), now).is_err(),
        "el movimiento ya está conciliado"
    );

    // Recalcular no vale con algo cobrado; desglosar lo pendiente sí, si cuadra.
    assert!(set_schedule(&v, "emitida", inv.id, None, now).is_err());
    let half = Money::from_euros(605, 0);
    assert!(
        set_schedule(
            &v,
            "emitida",
            inv.id,
            Some(&[(date!(2026 - 11 - 15), half)]),
            now
        )
        .is_err(),
        "no suma lo pendiente"
    );
    set_schedule(
        &v,
        "emitida",
        inv.id,
        Some(&[(date!(2026 - 12 - 15), half), (date!(2026 - 11 - 15), half)]),
        now,
    )
    .unwrap();
    let i = of(&list(&v, today).unwrap(), inv.id);
    assert_eq!(
        i.iter()
            .map(|x| (x.seq, x.due_date, x.amount, x.status.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (
                1,
                date!(2026 - 10 - 05),
                Money::from_euros(1210, 0),
                "banco"
            ),
            (2, date!(2026 - 11 - 15), half, "pendiente"),
            (3, date!(2026 - 12 - 15), half, "pendiente")
        ]
    );
    set_due_date(&v, "emitida", inv.id, 3, date!(2026 - 12 - 20)).unwrap();
    assert!(set_due_date(&v, "emitida", inv.id, 1, date!(2026 - 12 - 20)).is_err());

    // Cobro del segundo plazo en caja: asiento por su importe, con «(2/3)» en el concepto.
    settle(&v, "emitida", inv.id, 2, today, "570", now).unwrap();
    assert!(settle(&v, "emitida", inv.id, 2, today, "570", now).is_err());
    let tb = trial_balance(&v, 2026, None).unwrap();
    let bal = |acc: &str| {
        tb.iter()
            .find(|r| r.account.as_str() == acc)
            .map(|r| r.debit - r.credit)
            .unwrap_or(Money::ZERO)
    };
    assert_eq!(bal("4300001"), half);
    assert!(
        journal(&v, 2026)
            .unwrap()
            .iter()
            .any(|e| e.entry.concept.ends_with("(2/3)"))
    );
    unsettle(&v, "emitida", inv.id, 2, now, true).unwrap();
    assert_eq!(of(&list(&v, today).unwrap(), inv.id)[1].status, "pendiente");
    assert!(v.verify_audit().is_ok());
}

fn received(v: &Vault, number: &str, base: i64, d: time::Date) {
    let now = datetime!(2026-12-31 10:00 UTC);
    let rec = ingest_file(
        v,
        &format!("{number}.xml"),
        format!("<?xml version=\"1.0\"?><x n=\"{number}\"/>").as_bytes(),
        Source::Manual,
        now,
    )
    .unwrap();
    review(
        v,
        rec.id,
        &Reviewed {
            supplier_nif: "A22222228".into(),
            supplier_name: "Proveedor SA".into(),
            number: number.into(),
            issue_date: d,
            base: Money::from_euros(base, 0),
            vat: Money::ZERO,
            withholding: Money::ZERO,
            total: Money::from_euros(base, 0),
            deductible_pct: 100,
            capital_good: false,
            nature: Nature::General,
            asset_class: None::<AssetClass>,
            vat_regime: Default::default(),
            supplier_country: None,
            self_assessed_rate: None,
            intra_goods: false,
            cash_basis: false,
            import_dua: false,
            prorata_use: Default::default(),
        },
        now,
    )
    .unwrap();
}

#[test]
fn periodo_medio_de_pago_y_cuadre() {
    use taxpain_app::accounting::manual_entry;
    use taxpain_app::cartera::{ledger_mismatches, payment_period};
    use taxpain_core::accounting::account::acc;
    use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
    let v = setup();
    let now = datetime!(2026-12-31 10:00 UTC);
    // 1.000 pagados a los 30 días y 500 pendientes 60 días al cierre:
    // (30 × 1.000 + 60 × 500) / 1.500 = 40 días.
    received(&v, "P-1", 1000, date!(2026 - 03 - 01));
    received(&v, "P-2", 500, date!(2026 - 11 - 01));
    let id = |n: &str| {
        list(&v, date!(2026 - 12 - 31))
            .unwrap()
            .into_iter()
            .find(|d| d.number == n)
            .unwrap()
            .invoice_id
    };
    settle(
        &v,
        "recibida",
        id("P-1"),
        1,
        date!(2026 - 03 - 31),
        "572",
        now,
    )
    .unwrap();
    let p = payment_period(&v, date!(2026 - 01 - 01), date!(2026 - 12 - 31)).unwrap();
    assert_eq!(
        (p.paid_ratio, p.pending_ratio, p.average),
        (3000, 6000, 4000)
    );
    assert_eq!(p.paid_total, Money::from_euros(1000, 0));
    assert_eq!(p.pending_total, Money::from_euros(500, 0));

    // Cuadra mientras la cartera y la contabilidad coinciden; un pago a mano al proveedor, no.
    assert!(
        ledger_mismatches(&v, date!(2026 - 12 - 31))
            .unwrap()
            .is_empty()
    );
    let supplier = parties(&v).unwrap()[0].account.clone();
    manual_entry(
        &v,
        &JournalEntry {
            date: date!(2026 - 12 - 15),
            concept: "Pago a cuenta".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc(&supplier), Money::from_euros(100, 0)),
                EntryLine::credit(acc("570"), Money::from_euros(100, 0)),
            ],
        },
        now,
    )
    .unwrap();
    let m = ledger_mismatches(&v, date!(2026 - 12 - 31)).unwrap();
    assert_eq!(m.len(), 1);
    assert_eq!(
        (m[0].ledger, m[0].pending),
        (Money::from_euros(400, 0), Money::from_euros(500, 0))
    );
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
