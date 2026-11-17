//! Asistente: ajustes, clave de API (nunca hacia la interfaz) y contexto sin datos personales.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::assistant::{self, AssistantSettings};
use taxpain_app::invoicing::{self, InvoiceDraft, issue};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_assistant::Provider;
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

#[test]
fn el_envio_de_un_fichero_entero_queda_en_la_auditoria_con_su_huella() {
    use sha2::{Digest, Sha256};

    let v = setup();
    let now = datetime!(2026-05-20 10:00 UTC);
    let pdf = b"%PDF-1.4 factura de prueba";
    assistant::record_file(&v, &Provider::default(), pdf, now).unwrap();
    let log = taxpain_vault::audit::entries(v.connection()).unwrap();
    let e = log.last().unwrap();
    assert_eq!(e.action, "assistant.file");
    let d: serde_json::Value = serde_json::from_str(&e.details).unwrap();
    assert_eq!(d["sha256"], hex::encode(Sha256::digest(pdf)));
    assert_eq!(d["size"], pdf.len());
    assert_eq!(d["remote"], true);
}

#[test]
fn ajustes_clave_y_contexto() {
    let v = setup();
    let now = datetime!(2026-05-20 10:00 UTC);
    let st = assistant::status(&v).unwrap();
    assert!(!st.has_api_key && !st.settings.consent && st.remote);

    // Un proveedor fuera de la lista no se guarda.
    let bad = AssistantSettings {
        provider: Provider::OpenAiCompatible {
            base_url: "https://evil.example/v1".into(),
            model: "x".into(),
        },
        consent: true,
    };
    assert!(assistant::save_settings(&v, &bad, now).is_err());
    let ok = AssistantSettings {
        consent: true,
        ..AssistantSettings::default()
    };
    assistant::save_settings(&v, &ok, now).unwrap();

    assistant::set_api_key(&v, Some("  sk-ant-prueba-123  "), now).unwrap();
    let st = assistant::status(&v).unwrap();
    assert!(st.has_api_key && st.settings.consent);
    // El estado que va a la interfaz no lleva la clave.
    assert!(!serde_json::to_string(&st).unwrap().contains("sk-ant"));
    assert_eq!(
        assistant::api_key(&v).unwrap().as_deref(),
        Some("sk-ant-prueba-123")
    );
    // Ni la auditoría.
    let log = taxpain_vault::audit::entries(v.connection()).unwrap();
    assert!(
        log.iter()
            .all(|e| !e.details.contains("sk-ant") && !e.subject.contains("sk-ant"))
    );
    // La clave va ligada al proveedor: al cambiar a otro, no se usa (ni se envía).
    let mistral = AssistantSettings {
        provider: Provider::OpenAiCompatible {
            base_url: "https://api.mistral.ai/v1".into(),
            model: "mistral-large-latest".into(),
        },
        consent: true,
    };
    assistant::save_settings(&v, &mistral, now).unwrap();
    assert!(assistant::api_key(&v).unwrap().is_none());
    assert!(!assistant::status(&v).unwrap().has_api_key);
    let local = AssistantSettings {
        provider: Provider::OpenAiCompatible {
            base_url: "http://localhost:11434/v1".into(),
            model: "llama3.1".into(),
        },
        consent: false,
    };
    assistant::save_settings(&v, &local, now).unwrap();
    assert!(assistant::api_key(&v).unwrap().is_none());
    assert!(assistant::set_api_key(&v, Some("x"), now).is_err());
    // De vuelta a Anthropic, su clave sigue ahí; y se puede borrar.
    assistant::save_settings(&v, &ok, now).unwrap();
    assert!(assistant::api_key(&v).unwrap().is_some());
    assistant::set_api_key(&v, None, now).unwrap();
    assert!(assistant::api_key(&v).unwrap().is_none());

    issue(
        &v,
        &draft("B12345674", "Cliente Uno SL", 1000, date!(2026 - 04 - 10)),
        now,
        true,
    )
    .unwrap();
    v.set_setting(
        "taxpayer_profile",
        &taxpain_core::calendar::TaxpayerProfile::sociedad_limitada(),
    )
    .unwrap();
    let ctx = assistant::context(&v, date!(2026 - 05 - 20)).unwrap();
    assert!(ctx.contains("- Plazo: modelo"), "{ctx}");
    assert!(ctx.contains("sociedad"), "{ctx}");
    assert!(ctx.contains("ingresos 1.000,00 €"), "{ctx}");
    assert!(
        ctx.contains("modelo 303 del 2T: 210,00 € (a ingresar)"),
        "{ctx}"
    );
    // Sin NIF ni nombres.
    for raw in ["12345678Z", "B12345674", "Cliente Uno", "Carmen"] {
        assert!(!ctx.contains(raw), "{raw} en el contexto: {ctx}");
    }
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
