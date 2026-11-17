//! Fotos y escaneados en Recibidas: el texto lo lee el Mac (aquí, un lector simulado) y pasa por
//! la misma extracción que un PDF; avisos de guardar el papel y del ticket sin NIF.
//! Va en su propio binario: el lector se instala una vez por proceso.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::received::{self, Source};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::{carpeta, invoicing};
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::datetime;

const OWN: &str = "B98765431";

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
                id: PartyId::Nif(OWN.into()),
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
        },
        datetime!(2026-01-01 9:00 UTC),
    )
    .unwrap();
    v
}

#[test]
fn fotos_y_escaneados_se_leen_con_el_mac() {
    carpeta::set_ocr(Box::new(|name: &str, _bytes: &[u8]| {
        if name.starts_with("ticket") {
            Some(
                "Papelería Ejemplo SA\nNIF A22222228\nFactura simplificada nº T-123\n\
                 Fecha 03/02/2026\nBase imponible 10,00\nIVA 21% 2,10\nTotal 12,10 €"
                    .to_owned(),
            )
        } else if name.starts_with("escaneada") {
            Some(format!(
                "Imprenta Ejemplo SL\nNIF A22222228\nFactura nº F-77\nFecha 10/03/2026\n\
                 Cliente: Agencia de Prueba SL, NIF {OWN}\nBase imponible 100,00\nIVA 21% 21,00\n\
                 Total 121,00 €"
            ))
        } else {
            None
        }
    }));
    let v = setup();
    let now = datetime!(2026-03-15 10:00 UTC);

    // Foto de un ticket sin el NIF de la empresa.
    let out = received::ingest_file(
        &v,
        "ticket papeleria.jpg",
        &[0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3],
        Source::Manual,
        now,
    )
    .unwrap();
    let e = &out.extraction;
    assert_eq!(e.source, "ocr");
    assert_eq!(
        e.total.as_ref().map(|f| f.value),
        Some(Money::from_cents(1210))
    );
    assert!(
        e.warnings.iter().any(|w| w.contains("guarda el papel")),
        "{:?}",
        e.warnings
    );
    assert!(
        e.warnings.iter().any(|w| w.contains("art. 97")),
        "{:?}",
        e.warnings
    );
    // Se guarda como imagen (se abre con Vista Previa).
    let (_, mime, _) = received::received_document(&v, out.id).unwrap();
    assert_eq!(mime, "image/jpeg");

    // PDF escaneado (sin capa de texto) con el NIF de la empresa: sin el aviso del ticket.
    let out = received::ingest_file(
        &v,
        "escaneada imprenta.pdf",
        b"%PDF-1.4\n%sin texto\n",
        Source::Manual,
        now,
    )
    .unwrap();
    let e = &out.extraction;
    assert_eq!(e.source, "ocr");
    assert_eq!(
        e.total.as_ref().map(|f| f.value),
        Some(Money::from_cents(12_100))
    );
    assert!(
        !e.warnings.iter().any(|w| w.contains("art. 97")),
        "{:?}",
        e.warnings
    );

    // Una foto que el Mac no sabe leer: a mano.
    let out = received::ingest_file(
        &v,
        "IMG_0001.heic",
        b"....ftypheic....",
        Source::Manual,
        now,
    )
    .unwrap();
    assert_eq!(out.extraction.source, "imagen");
    assert!(out.extraction.total.is_none());
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
