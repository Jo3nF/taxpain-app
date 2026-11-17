//! Los datos de la empresa se sustituyen en todo lo que se envía al asistente.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::assistant::{context, known_values};
use taxpain_app::invoicing;
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_assistant::{Message, Role, prepare_with};
use taxpain_core::invoice::{Address, Party, PartyId, RoundingMode};
use taxpain_core::money::Rate;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

#[test]
fn nombre_nif_y_domicilio_no_salen() {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Talleres Quiñones Ybarra SL".into(),
            address: Some(Address {
                street: "Calle del Canario 77".into(),
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
    invoicing::save_settings(&v, &s, datetime!(2026-01-01 9:00 UTC)).unwrap();
    let known = known_values(&v).unwrap();
    assert!(known.contains(&"B12345674".to_string()));
    let ctx = context(&v, date!(2026 - 10 - 20)).unwrap();
    let q = [Message {
        role: Role::User,
        content: "¿TALLERES QUIÑONES YBARRA, S.L. (B-12345674), calle del canario, 77, paga IVA?"
            .into(),
    }];
    let out = prepare_with(&ctx, &q, &known).unwrap();
    let sent = format!("{}\n{}", out.system, out.messages[0].content).to_lowercase();
    for leak in ["quiñones", "ybarra", "12345674", "canario"] {
        assert!(!sent.contains(leak), "{leak}: {sent}");
    }
}
