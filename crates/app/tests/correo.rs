//! Recordatorios de pago de una factura en plazos: sólo se reclama lo vencido.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::correo;
use taxpain_app::invoicing::{self, InvoiceDraft};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::invoice::{InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

fn settings() -> InvoicingSettings {
    InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("B12345674".into()),
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
            enabled: false,
            installation_id: "TP-test".into(),
            production: false,
        },
        footer: None,
        activity: None,
        simplified_up_to_3000: false,
    }
}

#[test]
fn en_plazos_solo_reclama_lo_vencido() {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let now = datetime!(2026-06-01 10:00 UTC);
    invoicing::save_settings(&v, &settings(), now).unwrap();
    let inv = invoicing::issue(
        &v,
        &InvoiceDraft {
            kind: InvoiceKind::Completa,
            series: None,
            issue_date: date!(2026 - 06 - 01),
            operation_date: None,
            recipient: Some(Party {
                id: PartyId::Nif("12345678Z".into()),
                name: "Carmen Española".into(),
                address: addr(),
                person: None,
            }),
            lines: vec![InvoiceLine {
                description: "Proyecto".into(),
                quantity_milli: 1000,
                unit_price: Money::from_euros(1000, 0),
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
            description: "Proyecto".into(),
            notes: None,
            currency: None,
            oss: None,
        },
        now,
        true,
    )
    .unwrap();
    // 1.210 € a 30, 60 y 90 días.
    taxpain_app::cartera::set_schedule(
        &v,
        "emitida",
        inv.id,
        Some(&[
            (date!(2026 - 07 - 01), Money::from_cents(40_333)),
            (date!(2026 - 07 - 31), Money::from_cents(40_333)),
            (date!(2026 - 08 - 30), Money::from_cents(40_334)),
        ]),
        now,
    )
    .unwrap();

    // A los 40 días sólo ha vencido el primer plazo: se reclaman 403,33 €, no 1.210 €.
    let r = correo::compose(&v, inv.id, true, date!(2026 - 07 - 11)).unwrap();
    assert_eq!(r.pending, Money::from_cents(121_000));
    assert_eq!(r.overdue, Money::from_cents(40_333));
    assert_eq!(r.overdue_days, 10);
    assert!(
        r.body.contains(
            "que venció el 01/07/2026, sigue pendiente de pago por 403,33 € (lo vencido hasta hoy)"
        ),
        "{}",
        r.body
    );
    assert!(!r.body.contains("1.210,00"), "{}", r.body);
    // En Clientes y proveedores, «Vencido» es también sólo el primer plazo.
    let row = taxpain_app::contacts::list(&v, 2026, date!(2026 - 07 - 11))
        .unwrap()
        .into_iter()
        .find(|r| r.name == "Carmen Española")
        .unwrap();
    assert_eq!(row.to_collect, Money::from_cents(121_000));
    assert_eq!(row.overdue, Money::from_cents(40_333));

    // El día del vencimiento aún no está vencido.
    let r = correo::compose(&v, inv.id, true, date!(2026 - 07 - 31)).unwrap();
    assert_eq!(r.overdue, Money::from_cents(40_333));

    // Vencido todo: el tercer aviso (Ley 3/2004) es por el total.
    for (day, at) in [
        (date!(2026 - 09 - 05), datetime!(2026-09-05 10:00 UTC)),
        (date!(2026 - 09 - 12), datetime!(2026-09-12 10:00 UTC)),
    ] {
        correo::compose(&v, inv.id, true, day).unwrap();
        correo::record_sent(&v, inv.id, true, at).unwrap();
    }
    let r = correo::compose(&v, inv.id, true, date!(2026 - 09 - 20)).unwrap();
    assert_eq!(r.reminder, Some(3));
    assert_eq!(r.overdue, r.pending);
    assert!(
        r.body.contains("por 1.210,00 €. Es el tercer aviso"),
        "{}",
        r.body
    );
    assert!(r.body.contains("Ley 3/2004"));

    // Antes de que venza nada, un aviso amable (sin tono de reclamación) de lo pendiente.
    let v2 = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    invoicing::save_settings(&v2, &settings(), now).unwrap();
    let id = invoicing::issue(
        &v2,
        &invoicing::InvoiceDraft {
            issue_date: date!(2026 - 06 - 01),
            ..correo_draft(&v, inv.id)
        },
        now,
        true,
    )
    .unwrap()
    .id;
    let r = correo::compose(&v2, id, true, date!(2026 - 06 - 10)).unwrap();
    assert_eq!(r.reminder, Some(1));
    assert!(r.overdue.is_zero());
    assert!(r.body.contains("que vence el 01/07/2026"), "{}", r.body);
}

fn correo_draft(v: &Vault, id: i64) -> InvoiceDraft {
    taxpain_app::recurring::draft_of(v, id).unwrap()
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
