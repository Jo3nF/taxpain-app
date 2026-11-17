//! Facturas recibidas: ingesta de PDF y Facturae, duplicados, carpeta vigilada y revisión.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::invoicing;
use taxpain_app::received::{
    Nature, Reviewed, Source, discard, ingest_file, list_received, review, scan_folder, undiscard,
};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::invoice::{
    Address, Invoice, InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment,
    compute_totals,
};
use taxpain_core::money::{Money, Rate};
use taxpain_invoicing::facturae::{CorrectionReason, to_facturae};
use taxpain_invoicing::pdf::{PdfOptions, render};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

fn addr() -> Option<Address> {
    Some(Address {
        street: "Calle del Mar 3".into(),
        post_code: "46001".into(),
        town: "València".into(),
        province: "Valencia".into(),
        country: "ES".into(),
    })
}

/// Factura que nos manda un proveedor (B12345674) a nosotros (12345678Z).
fn supplier_invoice() -> Invoice {
    Invoice {
        kind: InvoiceKind::Completa,
        number: "FV-2026-118".into(),
        issue_date: date!(2026 - 10 - 05),
        operation_date: None,
        issuer: Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Suministros Levante SL".into(),
            address: addr(),
            person: None,
        },
        recipient: Some(Party {
            id: PartyId::Nif("12345678Z".into()),
            name: "Carmen Española".into(),
            address: addr(),
            person: Some(taxpain_core::invoice::PersonName {
                name: "Carmen".into(),
                first_surname: "Española".into(),
                second_surname: None,
            }),
        }),
        lines: vec![InvoiceLine {
            description: "Papel A4".into(),
            quantity_milli: 10_000,
            unit_price: Money::from_cents(450),
            discount: Rate::ZERO,
            vat: VatTreatment::Taxed {
                rate: Rate::percent(21),
            },
            surcharge: None,
            withholding: None,
            advance: false,
        }],
        rectified: vec![],
        description: "Material de oficina".into(),
        notes: None,
    }
}

fn vault_with_settings() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("12345678Z".into()),
            name: "Carmen Española".into(),
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
    invoicing::save_settings(&v, &s, datetime!(2026-10-13 10:00 UTC)).unwrap();
    v
}

#[test]
fn pdf_de_proveedor_se_extrae_y_se_revisa() {
    let v = vault_with_settings();
    let now = datetime!(2026-10-13 10:00 UTC);
    let inv = supplier_invoice();
    let t = compute_totals(&inv.lines, RoundingMode::PerRate);
    let pdf = render(&inv, &t, &PdfOptions::default()).unwrap();

    let out = ingest_file(&v, "factura-levante.pdf", &pdf, Source::Manual, now).unwrap();
    assert!(!out.duplicate);
    let e = &out.extraction;
    assert_eq!(
        e.supplier_nif.as_ref().map(|f| f.value.as_str()),
        Some("B12345674"),
        "{e:?}"
    );
    assert_eq!(
        e.recipient_nif.as_ref().map(|f| f.value.as_str()),
        Some("12345678Z")
    );
    assert_eq!(
        e.number.as_ref().map(|f| f.value.as_str()),
        Some("FV-2026-118")
    );
    assert_eq!(
        e.issue_date.as_ref().map(|f| f.value),
        Some(date!(2026 - 10 - 05))
    );
    assert_eq!(
        e.total.as_ref().map(|f| f.value),
        Some(Money::from_cents(5445))
    );

    // El mismo fichero otra vez: no se duplica.
    let again = ingest_file(&v, "copia.pdf", &pdf, Source::Manual, now).unwrap();
    assert!(again.duplicate && again.id == out.id);

    let pend = list_received(&v, Some("pendiente")).unwrap();
    assert_eq!(pend.len(), 1);

    let mut r = Reviewed {
        supplier_nif: "B12345675".into(),
        supplier_name: "Suministros Levante SL".into(),
        number: "FV-2026-118".into(),
        issue_date: date!(2026 - 10 - 05),
        base: Money::from_cents(4500),
        vat: Money::from_cents(945),
        withholding: Money::ZERO,
        total: Money::from_cents(5445),
        deductible_pct: 100,
        capital_good: false,
        nature: Nature::General,
        asset_class: None,
        vat_regime: Default::default(),
        supplier_country: None,
        self_assessed_rate: None,
        intra_goods: false,
        cash_basis: false,
        import_dua: false,
        prorata_use: Default::default(),
    };
    assert!(
        review(&v, out.id, &r, now).is_err(),
        "NIF con control erróneo"
    );
    r.supplier_nif = "B12345674".into();
    r.total = Money::from_cents(5000);
    assert!(
        review(&v, out.id, &r, now).is_err(),
        "importes que no cuadran"
    );
    r.total = Money::from_cents(5445);
    review(&v, out.id, &r, now).unwrap();
    assert_eq!(list_received(&v, Some("revisada")).unwrap().len(), 1);
    // Revisada = contabilizada: ni se revisa otra vez (asientos duplicados) ni se descarta
    // (dejaría el asiento huérfano y los libros distintos de los modelos).
    r.issue_date = date!(2027 - 01 - 05);
    let again = review(&v, out.id, &r, now).unwrap_err().to_string();
    assert!(again.contains("ya está revisada"), "{again}");
    assert!(discard(&v, out.id, now).is_err());
    assert_eq!(list_received(&v, Some("revisada")).unwrap().len(), 1);
}

#[test]
fn facturae_carpeta_vigilada_y_duplicados_de_numero() {
    let v = vault_with_settings();
    let now = datetime!(2026-10-13 10:00 UTC);
    let inv = supplier_invoice();
    let t = compute_totals(&inv.lines, RoundingMode::PerRate);
    let xml = to_facturae(&inv, &t, &CorrectionReason::default()).unwrap();

    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.xml"), &xml).unwrap();
    std::fs::write(dir.path().join("notas.txt"), "no es factura").unwrap();
    std::fs::write(dir.path().join(".oculto.pdf"), "%PDF-1.4").unwrap();
    // Un enlace simbólico a un fichero de fuera de la carpeta no se sigue.
    let fuera = tempfile::tempdir().unwrap();
    std::fs::write(fuera.path().join("privado.xml"), format!("{xml} ")).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        fuera.path().join("privado.xml"),
        dir.path().join("enlace.xml"),
    )
    .unwrap();
    assert_eq!(scan_folder(&v, dir.path(), now).unwrap(), 1);
    assert_eq!(scan_folder(&v, dir.path(), now).unwrap(), 0, "idempotente");

    let all = list_received(&v, None).unwrap();
    assert_eq!(all.len(), 1);
    let first = &all[0];
    assert_eq!(first.supplier_nif.as_deref(), Some("B12345674"));
    assert_eq!(first.total, Some(Money::from_cents(5445)));

    // Otra copia distinta (otro fichero) de la misma factura: al revisar se detecta.
    let other = ingest_file(
        &v,
        "b.xml",
        format!("{xml}\n").as_bytes(),
        Source::Manual,
        now,
    )
    .unwrap();
    assert!(!other.duplicate);
    let r = Reviewed {
        supplier_nif: "B12345674".into(),
        supplier_name: "Suministros Levante SL".into(),
        number: "FV-2026-118".into(),
        issue_date: date!(2026 - 10 - 05),
        base: Money::from_cents(4500),
        vat: Money::from_cents(945),
        withholding: Money::ZERO,
        total: Money::from_cents(5445),
        deductible_pct: 100,
        capital_good: false,
        nature: Nature::General,
        asset_class: None,
        vat_regime: Default::default(),
        supplier_country: None,
        self_assessed_rate: None,
        intra_goods: false,
        cash_basis: false,
        import_dua: false,
        prorata_use: Default::default(),
    };
    review(&v, first.id, &r, now).unwrap();
    assert!(
        review(&v, other.id, &r, now).is_err(),
        "misma factura revisada dos veces"
    );
    discard(&v, other.id, now).unwrap();
    assert_eq!(list_received(&v, Some("descartada")).unwrap().len(), 1);
    // Descartada por error: se recupera y vuelve a pendiente (reimportar el fichero no vale,
    // porque la huella lo da por duplicado).
    undiscard(&v, other.id, now).unwrap();
    assert_eq!(list_received(&v, Some("descartada")).unwrap().len(), 0);
    assert_eq!(list_received(&v, Some("pendiente")).unwrap().len(), 1);
    assert!(
        undiscard(&v, other.id, now).is_err(),
        "sólo se recupera una descartada"
    );
    assert!(
        undiscard(&v, first.id, now).is_err(),
        "una revisada no se toca"
    );
}

#[test]
fn un_ticket_en_pdf_con_texto_avisa_de_que_no_lleva_tu_nif() {
    use taxpain_invoicing::pdf::{Para, text_document};

    let v = vault_with_settings();
    let now = datetime!(2026-10-13 10:00 UTC);
    let ticket = text_document(
        "Factura simplificada",
        &[vec![
            Para::Bold("Restaurante Ejemplo SL".into()),
            Para::Text("NIF B12345674".into()),
            Para::Text("Factura simplificada nº T-77".into()),
            Para::Text("Fecha 05/10/2026".into()),
            Para::Text("Base 22,00 € IVA 10 % 2,20 € Total 24,20 €".into()),
        ]],
    )
    .unwrap();
    let out = ingest_file(&v, "ticket.pdf", &ticket, Source::Manual, now).unwrap();
    assert!(
        out.extraction
            .warnings
            .iter()
            .any(|w| w.contains("no aparece tu NIF") && w.contains("art. 97 LIVA")),
        "{:?}",
        out.extraction.warnings
    );
    // La factura completa del proveedor, con nuestro NIF, no avisa.
    let inv = supplier_invoice();
    let t = compute_totals(&inv.lines, RoundingMode::PerRate);
    let pdf = render(&inv, &t, &PdfOptions::default()).unwrap();
    let out = ingest_file(&v, "completa.pdf", &pdf, Source::Manual, now).unwrap();
    assert!(
        !out.extraction
            .warnings
            .iter()
            .any(|w| w.contains("no aparece tu NIF")),
        "{:?}",
        out.extraction.warnings
    );
}

#[test]
fn imagenes_y_escaneados_quedan_pendientes_con_aviso() {
    let v = vault_with_settings();
    let out = ingest_file(
        &v,
        "foto.jpg",
        &[0xFF, 0xD8, 0xFF, 0xE0, 1, 2, 3],
        Source::Manual,
        datetime!(2026-10-13 10:00 UTC),
    )
    .unwrap();
    assert!(
        out.extraction
            .warnings
            .iter()
            .any(|w| w.contains("rellena"))
    );
    assert!(
        ingest_file(
            &v,
            "vacio.pdf",
            &[],
            Source::Manual,
            datetime!(2026-10-13 10:00 UTC)
        )
        .is_err()
    );
}

#[test]
fn factura_tecleada_sin_documento() {
    use taxpain_app::received::register_typed;
    let v = vault_with_settings();
    let now = datetime!(2026-03-10 9:00 UTC);
    let mut r = Reviewed {
        supplier_nif: "B12345674".into(),
        supplier_name: "Taller Mecánico SL".into(),
        number: "T-77".into(),
        issue_date: date!(2026 - 03 - 08),
        base: Money::from_euros(100, 0),
        vat: Money::from_euros(21, 0),
        withholding: Money::ZERO,
        total: Money::from_euros(121, 0),
        deductible_pct: 100,
        capital_good: false,
        nature: Nature::General,
        asset_class: None,
        vat_regime: Default::default(),
        supplier_country: None,
        self_assessed_rate: None,
        intra_goods: false,
        cash_basis: false,
        import_dua: false,
        prorata_use: Default::default(),
    };
    // Datos que no cuadran: no queda nada registrado.
    r.total = Money::from_euros(1, 0);
    assert!(register_typed(&v, &r, now).is_err());
    assert!(list_received(&v, None).unwrap().is_empty());
    r.total = Money::from_euros(121, 0);
    let id = register_typed(&v, &r, now).unwrap();
    let l = list_received(&v, Some("revisada")).unwrap();
    assert_eq!(l.len(), 1);
    assert_eq!(l[0].id, id);
    assert!(register_typed(&v, &r, now).is_err(), "duplicada");
    // Con su asiento.
    let j = taxpain_app::accounting::journal(&v, 2026).unwrap();
    assert_eq!(j.len(), 1);
}
