//! Clientes y proveedores de otros países de la UE: Grecia («GR», NIF-IVA «EL…»), el NIF-IVA con
//! el prefijo del país al emitir, las entregas intracomunitarias sólo a un NIF-IVA de la UE, el
//! particular de la UE con IVA español, y las fotos de Recibidas (NIF propio y extensiones).

#![allow(clippy::unwrap_used)]

use std::io::Read;
use std::sync::Arc;

use taxpain_app::invoicing::{self, InvoiceDraft};
use taxpain_app::models;
use taxpain_app::received::{self, Nature, Reviewed, Source, VatRegime};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::invoice::{
    Exemption, IdType, InvoiceKind, InvoiceLine, NotSubject, Party, PartyId, RoundingMode,
    VatTreatment,
};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

fn setup_with(issuer_nif: &str) -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    invoicing::save_settings(
        &v,
        &InvoicingSettings {
            issuer: Party {
                id: PartyId::Nif(issuer_nif.into()),
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
                installation_id: "TP-test".into(),
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

fn setup() -> Vault {
    setup_with("B98765431")
}

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn get(d: &models::Draft, b: u16) -> Money {
    d.boxes
        .iter()
        .find(|(k, _)| *k == b)
        .map_or(Money::ZERO, |(_, v)| *v)
}

fn foreign(country: &str, id_type: IdType, id: &str) -> Party {
    Party {
        id: PartyId::Foreign {
            country: country.into(),
            id_type,
            id: id.into(),
        },
        name: "Cliente de fuera".into(),
        address: addr(),
        person: None,
    }
}

fn draft(recipient: Party, vat: VatTreatment, day: time::Date) -> InvoiceDraft {
    InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: day,
        operation_date: None,
        recipient: Some(recipient),
        lines: vec![InvoiceLine {
            description: "Mercancía".into(),
            quantity_milli: 1000,
            unit_price: eur(1000),
            discount: Rate::ZERO,
            vat,
            surcharge: None,
            withholding: None,
            advance: false,
        }],
        rectified: vec![],
        rectified_amounts: None,
        description: "Mercancía".into(),
        notes: None,
        currency: None,
        oss: None,
    }
}

#[test]
fn compra_intracomunitaria_a_un_proveedor_griego() {
    let v = setup();
    let now = datetime!(2026-08-03 10:00 UTC);
    let id = received::ingest_file(
        &v,
        "gr.xml",
        b"<?xml version=\"1.0\"?><gr/>",
        Source::Manual,
        now,
    )
    .unwrap()
    .id;
    // País «GR» (el de la lista) y NIF-IVA con «EL»: antes se rechazaba («GR no es de la UE»).
    received::review(
        &v,
        id,
        &Reviewed {
            supplier_nif: "EL 094259216".into(),
            supplier_name: "Proveedor Hellas AE".into(),
            number: "GR-2026-5".into(),
            issue_date: date!(2026 - 08 - 01),
            base: eur(100),
            vat: Money::ZERO,
            withholding: Money::ZERO,
            total: eur(100),
            deductible_pct: 100,
            capital_good: false,
            nature: Nature::General,
            asset_class: None,
            vat_regime: VatRegime::Intracomunitaria,
            supplier_country: Some("GR".into()),
            self_assessed_rate: None,
            intra_goods: false,
            cash_basis: false,
            import_dua: false,
            prorata_use: Default::default(),
        },
        now,
    )
    .unwrap();
    let (nif, country): (String, String) = v
        .connection()
        .query_row(
            "SELECT supplier_nif, supplier_country FROM invoice_received WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((nif.as_str(), country.as_str()), ("EL094259216", "GR"));
    let d = models::draft(&v, "303", 2026, 3).unwrap();
    assert_eq!(
        (get(&d, 10), get(&d, 11)),
        (eur(100), eur(21)),
        "{:?}",
        d.boxes
    );
    let d349 = models::draft(&v, "349", 2026, 3).unwrap();
    assert!(
        d349.details
            .iter()
            .any(|(t, _)| t.contains("EL094259216") && t.ends_with("clave I")),
        "{:?}",
        d349.details
    );

    // Libro registro de recibidas: tipo 02 (NIF-IVA), país GR y el número sin el prefijo.
    let (_, bytes) = taxpain_app::books::lsi_workbook(&v, 2026).unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut shared = String::new();
    z.by_name("xl/sharedStrings.xml")
        .unwrap()
        .read_to_string(&mut shared)
        .unwrap();
    assert!(shared.contains(">094259216<"), "{shared}");
    assert!(shared.contains(">02<"), "{shared}");
    assert!(!shared.contains(">EL094259216<"), "{shared}");
}

#[test]
fn el_nif_iva_del_cliente_va_con_el_prefijo_del_pais() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    // Escrito sin prefijo y con espacios: la factura, VERI*FACTU y la ficha lo llevan con «FR».
    let out = invoicing::issue(
        &v,
        &draft(
            foreign("fr", IdType::NifIva, "123 456 789 01"),
            VatTreatment::NotSubject(NotSubject::N2),
            date!(2026 - 10 - 20),
        ),
        now,
        true,
    )
    .unwrap();
    let (tax_id, data): (String, String) = v
        .connection()
        .query_row(
            "SELECT recipient_tax_id, data FROM invoice_issued WHERE id = ?1",
            [out.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(tax_id, "FR12345678901");
    assert!(data.contains("\"country\":\"FR\""), "{data}");
    let xml: String = v
        .connection()
        .query_row(
            "SELECT xml FROM verifactu_record WHERE invoice_id = ?1",
            [out.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        xml.contains("<sf:IDType>02</sf:IDType><sf:ID>FR12345678901</sf:ID>"),
        "{xml}"
    );
    // Grecia: «EL».
    let gr = invoicing::issue(
        &v,
        &draft(
            foreign("GR", IdType::NifIva, "123456789"),
            VatTreatment::NotSubject(NotSubject::N2),
            date!(2026 - 10 - 21),
        ),
        now,
        true,
    )
    .unwrap();
    let tax_id: String = v
        .connection()
        .query_row(
            "SELECT recipient_tax_id FROM invoice_issued WHERE id = ?1",
            [gr.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tax_id, "EL123456789");
    // Un NIF español con guion se guarda limpio.
    let es = invoicing::issue(
        &v,
        &draft(
            Party {
                id: PartyId::Nif("12345678-z".into()),
                name: "Carmen".into(),
                address: addr(),
                person: None,
            },
            VatTreatment::Taxed {
                rate: Rate::percent(21),
            },
            date!(2026 - 10 - 22),
        ),
        now,
        true,
    )
    .unwrap();
    let tax_id: String = v
        .connection()
        .query_row(
            "SELECT recipient_tax_id FROM invoice_issued WHERE id = ?1",
            [es.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(tax_id, "12345678Z");
    // Fuera de la UE no hay NIF-IVA: ni al emitir ni en la vista previa.
    let us = draft(
        foreign("US", IdType::NifIva, "123456789"),
        VatTreatment::NotSubject(NotSubject::N2),
        date!(2026 - 10 - 23),
    );
    let e = invoicing::issue(&v, &us, now, true)
        .unwrap_err()
        .to_string();
    assert!(e.contains("no es otro Estado miembro"), "{e}");
    assert!(invoicing::preview(&v, &us, now).is_err());
    let n: i64 = v
        .connection()
        .query_row("SELECT count(*) FROM invoice_issued", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 3);
}

#[test]
fn una_entrega_intracomunitaria_exige_un_nif_iva_de_otro_estado_miembro() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let e5 = VatTreatment::Exempt(Exemption::E5);
    for (who, recipient) in [
        (
            "fuera de la UE",
            foreign("US", IdType::DocumentoOficial, "123456789"),
        ),
        (
            "particular de la UE",
            foreign("FR", IdType::DocumentoOficial, "123456789"),
        ),
        (
            "cliente español",
            Party {
                id: PartyId::Nif("12345678Z".into()),
                name: "Carmen".into(),
                address: addr(),
                person: None,
            },
        ),
    ] {
        let e = invoicing::issue(&v, &draft(recipient, e5, date!(2026 - 10 - 20)), now, true)
            .unwrap_err()
            .to_string();
        assert!(e.contains("entrega intracomunitaria"), "{who}: {e}");
    }
    // A una empresa francesa o de Irlanda del Norte (bienes) con NIF-IVA, sí.
    for recipient in [
        foreign("FR", IdType::NifIva, "FR12345678901"),
        foreign("XI", IdType::NifIva, "XI123456789"),
    ] {
        invoicing::issue(&v, &draft(recipient, e5, date!(2026 - 10 - 20)), now, true).unwrap();
    }
    let m349 = models::draft(&v, "349", 2026, 4).unwrap();
    assert_eq!(m349.result, eur(2000), "{:?}", m349.details);
}

#[test]
fn un_particular_de_la_ue_lleva_iva_espanol_y_no_va_al_349() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    // Servicio a un particular francés (sin NIF-IVA): IVA español (art. 69 LIVA).
    invoicing::issue(
        &v,
        &draft(
            foreign("FR", IdType::DocumentoOficial, "AB123456"),
            VatTreatment::Taxed {
                rate: Rate::percent(21),
            },
            date!(2026 - 10 - 20),
        ),
        now,
        true,
    )
    .unwrap();
    let d = models::draft(&v, "303", 2026, 4).unwrap();
    assert_eq!(
        (get(&d, 7), get(&d, 9)),
        (eur(1000), eur(210)),
        "{:?}",
        d.boxes
    );
    assert_eq!(get(&d, 59), Money::ZERO);
    let m349 = models::draft(&v, "349", 2026, 4).unwrap();
    assert_eq!(m349.result, Money::ZERO, "{:?}", m349.details);
}

/// Lector de texto simulado (en el Mac es Vision): el mismo para todos los tests del fichero.
fn ocr() {
    taxpain_app::carpeta::set_ocr(Box::new(|name: &str, _bytes: &[u8]| {
        name.starts_with("factura").then(|| {
            "Imprenta Ejemplo SL\nNIF A22222228\nFactura nº F-77\nFecha 10/03/2026\n\
             Cliente: Agencia de Prueba SL, NIF B98765431\nBase imponible 100,00\nIVA 21% 21,00\n\
             Total 121,00 €"
                .to_owned()
        })
    }));
}

#[test]
fn el_nif_propio_con_guion_se_reconoce_en_la_foto() {
    ocr();
    // El NIF de la empresa guardado con guion: en el texto aparece sin él.
    let v = setup_with("B-98765431");
    let out = received::ingest_file(
        &v,
        "factura imprenta.jpg",
        &[0xFF, 0xD8, 0xFF, 0xE0, 9, 9],
        Source::Manual,
        datetime!(2026-03-15 10:00 UTC),
    )
    .unwrap();
    let e = &out.extraction;
    assert_eq!(e.source, "ocr");
    assert!(
        !e.warnings.iter().any(|w| w.contains("no aparece tu NIF")),
        "{:?}",
        e.warnings
    );
}

#[test]
fn la_carpeta_lee_las_mismas_fotos_que_recibidas() {
    use taxpain_app::carpeta::{self, Found};
    ocr();
    let v = setup();
    let dir = tempfile::tempdir().unwrap();
    for ext in ["webp", "tif", "tiff", "heif"] {
        std::fs::write(
            dir.path().join(format!("factura-{ext}.{ext}")),
            ext.as_bytes(),
        )
        .unwrap();
    }
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    for ext in ["webp", "tif", "tiff", "heif"] {
        let item = inv
            .items
            .iter()
            .find(|i| i.path.ends_with(&format!(".{ext}")))
            .unwrap_or_else(|| panic!("{ext} no se mira: {:?}", inv.items));
        assert!(
            matches!(&item.found, Found::FacturaRecibida { number, .. } if number.as_deref() == Some("F-77")),
            "{ext}: {:?}",
            item.found
        );
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
