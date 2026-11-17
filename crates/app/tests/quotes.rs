//! Presupuestos y proformas: numeración propia, estados, PDF sin VERI*FACTU y conversión en
//! factura.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::invoicing::{self, InvoiceDraft};
use taxpain_app::quotes::{self, QuoteInput};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::invoice::{InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::Date;
use time::macros::{date, datetime};

fn setup() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    invoicing::save_settings(
        &v,
        &InvoicingSettings {
            issuer: Party {
                id: PartyId::Nif("B98765431".into()),
                name: "Agencia de Prueba SL".into(),
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
                enabled: true,
                installation_id: "x".into(),
                production: false,
            },
            footer: None,
            activity: None,
            simplified_up_to_3000: false,
        },
        datetime!(2026-01-01 9:00 UTC),
    )
    .unwrap();
    v
}

fn input(kind: &str, euros: i64, day: Date, valid: Option<Date>) -> QuoteInput {
    QuoteInput {
        kind: kind.into(),
        draft: InvoiceDraft {
            kind: InvoiceKind::Completa,
            series: None,
            issue_date: day,
            operation_date: None,
            recipient: Some(Party {
                id: PartyId::Nif("12345678Z".into()),
                name: "Carmen Española".into(),
                address: addr(),
                person: None,
            }),
            lines: vec![InvoiceLine {
                description: "Diseño de la web".into(),
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
            description: "Diseño de la web".into(),
            notes: None,
            currency: None,
            oss: None,
        },
        valid_until: valid,
        language: None,
    }
}

#[test]
fn del_presupuesto_a_la_factura() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let today = date!(2026 - 10 - 24);
    let p1 = quotes::create(
        &v,
        &input("presupuesto", 2000, today, Some(date!(2026 - 11 - 24))),
        now,
    )
    .unwrap();
    assert_eq!(p1.number, "P2026-0001");
    assert_eq!(p1.status, "borrador");
    assert_eq!(p1.total, Money::from_euros(2420, 0));
    let p2 = quotes::create(
        &v,
        &input("presupuesto", 500, today, Some(date!(2026 - 10 - 30))),
        now,
    )
    .unwrap();
    assert_eq!(p2.number, "P2026-0002");
    let pf = quotes::create(&v, &input("proforma", 800, today, None), now).unwrap();
    assert_eq!(pf.number, "PF2026-0001");
    // Validez antes de la fecha: no.
    assert!(
        quotes::create(
            &v,
            &input("presupuesto", 1, today, Some(date!(2026 - 10 - 01))),
            now
        )
        .is_err()
    );

    // No son facturas: ni VERI*FACTU, ni asiento, ni factura emitida.
    let n: i64 = v
        .connection()
        .query_row("SELECT count(*) FROM invoice_issued", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
    let (name, pdf) = quotes::document(&v, p1.id, today).unwrap();
    assert_eq!(name, "P2026-0001.pdf");
    assert!(pdf.starts_with(b"%PDF"));

    // Estados; caduca el segundo.
    quotes::set_status(&v, p1.id, "enviado", now).unwrap();
    quotes::set_status(&v, p2.id, "enviado", now).unwrap();
    assert!(quotes::set_status(&v, p1.id, "facturado", now).is_err());
    let list = quotes::list(&v, 2026, date!(2026 - 11 - 05)).unwrap();
    assert!(list.iter().find(|q| q.id == p2.id).unwrap().expired);
    assert!(!list.iter().find(|q| q.id == p1.id).unwrap().expired);
    quotes::set_status(&v, p1.id, "aceptado", now).unwrap();
    assert_eq!(
        quotes::accepted_pending(&v).unwrap(),
        (1, Money::from_euros(2420, 0))
    );

    // A factura: con la fecha de hoy, se emite y queda enlazada.
    let (draft, _) = quotes::invoice_draft(&v, p1.id, date!(2026 - 11 - 02)).unwrap();
    assert_eq!(draft.issue_date, date!(2026 - 11 - 02));
    let inv = invoicing::issue(&v, &draft, datetime!(2026-11-02 10:00 UTC), true).unwrap();
    quotes::mark_invoiced(&v, p1.id, inv.id, now).unwrap();
    // Otra vez, no (ni a otra factura).
    assert!(quotes::mark_invoiced(&v, p1.id, inv.id, now).is_err());
    let q = quotes::get(&v, p1.id, today).unwrap();
    assert_eq!(q.status, "facturado");
    assert_eq!(q.invoice_number.as_deref(), Some(inv.number.as_str()));
    assert!(
        quotes::invoice_draft(&v, p1.id, today).is_err(),
        "no dos veces"
    );
    assert!(quotes::update(&v, p1.id, &input("presupuesto", 1, today, None), now).is_err());
    assert!(quotes::delete(&v, p1.id, now).is_err());
    assert_eq!(quotes::accepted_pending(&v).unwrap().0, 0);

    // Un borrador se cambia (mismo número) y se borra.
    let changed = quotes::update(&v, pf.id, &input("proforma", 900, today, None), now).unwrap();
    assert_eq!(changed.number, "PF2026-0001");
    assert_eq!(changed.total, Money::from_euros(1089, 0));
    quotes::delete(&v, pf.id, now).unwrap();
    assert_eq!(quotes::list(&v, 2026, today).unwrap().len(), 2);
    // Uno enviado no se borra: su número no queda libre para otro.
    let e = quotes::delete(&v, p2.id, now).unwrap_err().to_string();
    assert!(e.contains("rechazado"), "{e}");
    let p3 = quotes::create(&v, &input("presupuesto", 10, today, None), now).unwrap();
    assert_eq!(p3.number, "P2026-0003");
}

fn issued_count(v: &Vault) -> i64 {
    v.connection()
        .query_row("SELECT count(*) FROM invoice_issued", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn la_factura_del_presupuesto_se_emite_y_lo_enlaza_de_una_vez() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let today = date!(2026 - 10 - 24);
    let q = quotes::create(&v, &input("presupuesto", 1000, today, None), now).unwrap();
    quotes::set_status(&v, q.id, "aceptado", now).unwrap();
    let (draft, lang) = quotes::invoice_draft(&v, q.id, today).unwrap();

    // Sin NIF (o como simplificada): no se emite nada.
    let mut no_nif = draft.clone();
    no_nif.recipient.as_mut().unwrap().id = PartyId::Nif(String::new());
    let mut simplified = draft.clone();
    simplified.kind = InvoiceKind::Simplificada;
    simplified.recipient = None;
    for d in [&no_nif, &simplified] {
        let e = quotes::issue_invoice(&v, q.id, d, lang, now, true)
            .unwrap_err()
            .to_string();
        assert!(e.contains("pon su NIF"), "{e}");
    }
    // Sin confirmar, tampoco.
    assert!(quotes::issue_invoice(&v, q.id, &draft, lang, now, false).is_err());
    assert_eq!(issued_count(&v), 0);

    let inv = quotes::issue_invoice(&v, q.id, &draft, lang, now, true).unwrap();
    let got = quotes::get(&v, q.id, today).unwrap();
    assert_eq!(got.status, "facturado");
    assert_eq!(got.invoice_id, Some(inv.id));
    // Dos veces (otra ventana, doble clic): la segunda no emite otra factura.
    let e = quotes::issue_invoice(&v, q.id, &draft, lang, now, true)
        .unwrap_err()
        .to_string();
    assert!(e.contains("ya está facturado"), "{e}");
    assert_eq!(issued_count(&v), 1);

    // Si la factura se anula, el presupuesto vuelve a «aceptado» y se puede facturar otra vez.
    invoicing::cancel(&v, inv.id, now, true).unwrap();
    let got = quotes::get(&v, q.id, today).unwrap();
    assert_eq!(got.status, "aceptado");
    assert_eq!(got.invoice_id, None);
    let again = quotes::issue_invoice(&v, q.id, &draft, lang, now, true).unwrap();
    assert_ne!(again.id, inv.id);
    assert_eq!(
        quotes::get(&v, q.id, today)
            .unwrap()
            .invoice_number
            .as_deref(),
        Some(again.number.as_str())
    );

    // Uno rechazado no se factura.
    let r = quotes::create(&v, &input("presupuesto", 50, today, None), now).unwrap();
    quotes::set_status(&v, r.id, "rechazado", now).unwrap();
    let (d, _) = quotes::invoice_draft(&v, r.id, today).unwrap();
    assert!(quotes::issue_invoice(&v, r.id, &d, None, now, true).is_err());
}

#[test]
fn presupuesto_por_correo() {
    use taxpain_app::correo;
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let today = date!(2026 - 10 - 24);
    let q = quotes::create(
        &v,
        &input("presupuesto", 1000, today, Some(date!(2026 - 11 - 23))),
        now,
    )
    .unwrap();
    let d = correo::compose_quote(&v, q.id, today).unwrap();
    assert_eq!(d.subject, "Presupuesto P2026-0001");
    assert!(
        d.body.contains("1.210,00 €") && d.body.contains("23/11/2026"),
        "{}",
        d.body
    );
    let (name, eml) = correo::build_quote(
        &v,
        q.id,
        Some("cliente@example.com"),
        &d.subject,
        &d.body,
        today,
        now,
    )
    .unwrap();
    assert_eq!(name, "P2026-0001.eml");
    let text = String::from_utf8(eml).unwrap();
    assert!(text.contains("filename=\"P2026-0001.pdf\""));
    // Enviarlo lo deja como «enviado».
    assert_eq!(quotes::get(&v, q.id, today).unwrap().status, "enviado");
}

/// Con retención, la lista enseña el total a pagar (total menos retención), como el PDF.
#[test]
fn el_total_a_pagar_resta_la_retencion() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let today = date!(2026 - 10 - 24);
    let mut i = input("presupuesto", 1000, today, None);
    i.draft.lines[0].withholding = Some(Rate::percent(15));
    let q = quotes::create(&v, &i, now).unwrap();
    assert_eq!(q.total, Money::from_euros(1210, 0));
    assert_eq!(q.payable, Money::from_euros(1060, 0));
    let l = quotes::list(&v, 2026, today).unwrap();
    assert_eq!(l[0].payable, Money::from_euros(1060, 0));
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
