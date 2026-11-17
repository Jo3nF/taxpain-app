//! Importar una foto o un escaneado en Recibidas no tiene el almacén cogido mientras el Mac lee
//! el texto (puede tardar: hasta diez páginas por PDF): el resto de la app sigue respondiendo.
//! Va en su propio binario: el lector se instala una vez por proceso.

#![allow(clippy::unwrap_used)]

use std::ops::Deref;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use taxpain_app::received::{self, Source};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::{carpeta, invoicing};
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::Rate;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::datetime;

/// Cerrojos del almacén en vigor y si el lector llegó a correr con alguno cogido.
static HELD: AtomicUsize = AtomicUsize::new(0);
static READ: AtomicUsize = AtomicUsize::new(0);
static READ_WHILE_HELD: AtomicBool = AtomicBool::new(false);

/// Como el cerrojo de la app: mientras exista, nadie más puede usar el almacén.
struct Guard<'a>(&'a Vault);

impl Deref for Guard<'_> {
    type Target = Vault;
    fn deref(&self) -> &Vault {
        self.0
    }
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        HELD.fetch_sub(1, Ordering::SeqCst);
    }
}

fn lock(v: &Vault) -> taxpain_app::Result<Guard<'_>> {
    HELD.fetch_add(1, Ordering::SeqCst);
    Ok(Guard(v))
}

#[test]
fn el_reconocimiento_de_texto_va_sin_el_almacen() {
    carpeta::set_ocr(Box::new(|_name: &str, _bytes: &[u8]| {
        READ.fetch_add(1, Ordering::SeqCst);
        if HELD.load(Ordering::SeqCst) > 0 {
            READ_WHILE_HELD.store(true, Ordering::SeqCst);
        }
        Some(
            "Papelería Ejemplo SA\nNIF A22222228\nFactura simplificada nº T-123\n\
             Fecha 03/02/2026\nBase imponible 10,00\nIVA 21% 2,10\nTotal 12,10 €"
                .to_owned(),
        )
    }));
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
    let now = datetime!(2026-03-15 10:00 UTC);

    // Arrastrar una foto.
    let photo = [0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3];
    let out =
        received::ingest_file_with(|| lock(&v), "ticket.jpg", &photo, Source::Manual, now).unwrap();
    assert!(!out.duplicate);
    assert_eq!(out.extraction.source, "ocr");
    assert_eq!(READ.load(Ordering::SeqCst), 1);
    // La misma otra vez: ya estaba, sin volver a leerla.
    let again =
        received::ingest_file_with(|| lock(&v), "copia.jpg", &photo, Source::Manual, now).unwrap();
    assert!(again.duplicate && again.id == out.id);
    assert_eq!(READ.load(Ordering::SeqCst), 1);

    // La carpeta vigilada, igual.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("escaneada.jpg"),
        [0xFF, 0xD8, 0xFF, 0xE0, 9],
    )
    .unwrap();
    std::fs::write(dir.path().join("ticket.jpg"), photo).unwrap();
    assert_eq!(
        received::scan_folder_with(dir.path(), || lock(&v), now).unwrap(),
        1
    );
    assert_eq!(READ.load(Ordering::SeqCst), 2);

    assert_eq!(HELD.load(Ordering::SeqCst), 0);
    assert!(
        !READ_WHILE_HELD.load(Ordering::SeqCst),
        "el texto se leyó con el almacén cogido"
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
