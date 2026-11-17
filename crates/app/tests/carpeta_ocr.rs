//! Importar un documento de la empresa con texto no pasa por el reconocimiento de texto del Mac
//! (que dibuja hasta diez páginas con el almacén cogido): sólo se usa si el PDF no tiene texto.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use taxpain_app::carpeta::{self, Found};
use taxpain_invoicing::pdf::{Para, text_document};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::datetime;

static OCR_CALLS: AtomicUsize = AtomicUsize::new(0);

#[test]
fn un_pdf_con_texto_no_pasa_por_el_reconocimiento_de_texto_al_importarlo() {
    carpeta::set_ocr(Box::new(|_name: &str, _bytes: &[u8]| {
        OCR_CALLS.fetch_add(1, Ordering::SeqCst);
        None
    }));
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let escritura = text_document(
        "Escritura",
        &[vec![Para::Text(
            "ESCRITURA DE CONSTITUCIÓN DE SOCIEDAD DE RESPONSABILIDAD LIMITADA. Ante mí, Don \
             Ramón Pérez Notario, Notario de Valencia. Que constituye una sociedad denominada \
             EJEMPLO SERVICIOS, S.L., con domicilio social en Calle de la Paz 15, 46003 \
             Valencia. CNAE 6201. N.I.F.: B98765431."
                .into(),
        )]],
    )
    .unwrap();
    std::fs::write(dir.path().join("escritura.pdf"), &escritura).unwrap();
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    assert!(
        matches!(inv.items[0].found, Found::Documento { by_name: false, .. }),
        "{:?}",
        inv.items
    );
    let out = carpeta::apply(
        &v,
        dir.path(),
        &["escritura.pdf".to_owned()],
        datetime!(2026-10-26 10:00 UTC),
    )
    .unwrap();
    assert!(out[0].ok, "{out:?}");
    assert_eq!(
        OCR_CALLS.load(Ordering::SeqCst),
        0,
        "el PDF tiene texto: Vision no hace falta"
    );
}
