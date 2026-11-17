//! Depósito de cuentas: ZIP con DEPOSITO.XML (validado con el XSD del BOE) para importar en el D2.

#![allow(clippy::unwrap_used)]

use std::io::Read;
use std::sync::Arc;

use taxpain_app::accounting::{close_year, manual_entry};
use taxpain_app::deposito::{LegalForm, Request, build};
use taxpain_app::invoicing;
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::statements::ModelKind;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{Address, Party, PartyId, RoundingMode};
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
            id: PartyId::Nif("B12345674".into()),
            name: "Ejemplo & Hijos SL".into(),
            address: Some(Address {
                street: "Calle Mayor 1".into(),
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
    invoicing::save_settings(&v, &s, datetime!(2025-01-01 9:00 UTC)).unwrap();
    v
}

fn entry(d: time::Date, debit: &str, credit: &str, euros: i64) -> JournalEntry {
    JournalEntry {
        date: d,
        concept: "x".into(),
        origin: Origin::Manual,
        lines: vec![
            EntryLine::debit(acc(debit), Money::from_euros(euros, 0)),
            EntryLine::credit(acc(credit), Money::from_euros(euros, 0)),
        ],
    }
}

fn claves(xml: &str) -> Vec<(String, String)> {
    xml.split("<Clave><Codigo>")
        .skip(1)
        .map(|p| {
            let (k, rest) = p.split_once("</Codigo><Valor>").unwrap();
            (
                k.to_owned(),
                rest.split("</Valor>").next().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
fn deposito_pymes_con_ejercicio_anterior() {
    let v = setup();
    let now = datetime!(2026-05-10 10:00 UTC);
    for e in [
        entry(date!(2025 - 01 - 02), "5720001", "100", 3000),
        entry(date!(2025 - 06 - 10), "5720001", "705", 2000),
        entry(date!(2025 - 06 - 11), "629", "5720001", 500),
    ] {
        manual_entry(&v, &e, now).unwrap();
    }
    close_year(&v, 2025, now, true).unwrap();
    let req = Request {
        year: 2025,
        model: ModelKind::Pymes,
        registry_code: "46030".into(),
        legal_form: LegalForm::Sl,
        activity: "Diseño gráfico".into(),
        cnae: "7410".into(),
        signers: vec!["CARMEN EJEMPLO EJEMPLO".into()],
        memoria_pdf: Some(b"%PDF-1.4\n%prueba\n".to_vec()),
        ..Default::default()
    };
    assert!(
        build(
            &v,
            &Request {
                registry_code: "46".into(),
                ..req.clone()
            }
        )
        .is_err()
    );
    assert!(
        build(
            &v,
            &Request {
                cnae: String::new(),
                ..req.clone()
            }
        )
        .is_err()
    );
    let p = build(&v, &req).unwrap();
    assert_eq!(p.filename, "46030B1234567420251.ZIP");
    assert_eq!(p.huella.len(), 44);

    let mut z = zip::ZipArchive::new(std::io::Cursor::new(p.zip.as_slice())).unwrap();
    let names: Vec<String> = (0..z.len())
        .map(|i| z.by_index(i).unwrap().name().unwrap().into_owned())
        .collect();
    assert_eq!(names, ["DEPOSITO.XML", "MEMORIA.PDF"]);
    let mut xml = String::new();
    z.by_name("DEPOSITO.XML")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();

    let xsd = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/deposito/DEPOSITO.xsd");
    taxpain_testkit::xsd::validate(&xsd, &xml).unwrap_or_else(|e| panic!("{e:?}\n{xml}"));
    assert!(xml.contains("<TipoCuestionario>Pymes</TipoCuestionario>"));
    assert!(xml.contains("<RazonSocial>Ejemplo &amp; Hijos SL</RazonSocial>"));

    let c = claves(&xml);
    let get = |k: &str| c.iter().find(|x| x.0 == k).map(|x| x.1.clone());
    assert_eq!(get("01010").as_deref(), Some("B12345674"));
    assert_eq!(get("01012").as_deref(), Some("1"));
    assert_eq!(get("01025").as_deref(), Some("46"));
    assert_eq!(get("01101").as_deref(), Some("31.12.2025"));
    // Activo total = patrimonio neto y pasivo (PBL_002) y resultado en balance = PyG.
    assert_eq!(get("10000").as_deref(), Some("4500.00"));
    assert_eq!(get("10000"), get("30000"));
    assert_eq!(get("21700").as_deref(), Some("1500.00"));
    assert_eq!(get("49500").as_deref(), Some("1500.00"));
    assert_eq!(get("40100").as_deref(), Some("2000.00"));
    assert_eq!(get("91000").as_deref(), Some("1500.00"));
    assert_eq!(get("8080852").as_deref(), Some("1"), "memoria PYME");
    assert_eq!(get("8081010").as_deref(), Some("333001001001E00010000000"));
    assert_eq!(get("8081997").as_deref(), Some("B12311225"));
    assert_eq!(get("8091001").as_deref(), Some("CARMEN EJEMPLO EJEMPLO"));
    // Sin ejercicio anterior en TaxPain: no hay claves con sufijo 9.
    assert!(get("100009").is_none());
    assert!(p.warnings.iter().any(|w| w.contains("titular real")));

    // 2026, con 2025 como ejercicio anterior (claves con sufijo 9) y sin cerrar (aviso).
    manual_entry(
        &v,
        &entry(date!(2026 - 03 - 01), "5720001", "705", 1000),
        now,
    )
    .unwrap();
    let p = build(
        &v,
        &Request {
            year: 2026,
            memoria_pdf: None,
            ..req
        },
    )
    .unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(p.zip.as_slice())).unwrap();
    let mut xml = String::new();
    z.by_name("DEPOSITO.XML")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let c = claves(&xml);
    let get = |k: &str| c.iter().find(|x| x.0 == k).map(|x| x.1.clone());
    assert_eq!(get("10000").as_deref(), Some("5500.00"));
    assert_eq!(get("100009").as_deref(), Some("4500.00"));
    assert_eq!(get("495009").as_deref(), Some("1500.00"));
    assert_eq!(get("011019").as_deref(), Some("31.12.2025"));
    assert!(p.warnings.iter().any(|w| w.contains("no está cerrado")));
    assert!(p.warnings.iter().any(|w| w.contains("memoria")));
}

#[test]
fn deposito_abreviado_con_la_memoria_de_taxpain() {
    let v = setup();
    let now = datetime!(2026-05-10 10:00 UTC);
    for e in [
        entry(date!(2025 - 01 - 02), "5720001", "100", 3000),
        entry(date!(2025 - 06 - 10), "5720001", "705", 2000),
    ] {
        manual_entry(&v, &e, now).unwrap();
    }
    close_year(&v, 2025, now, true).unwrap();
    let p = build(
        &v,
        &Request {
            year: 2025,
            model: ModelKind::Abreviado,
            registry_code: "46030".into(),
            legal_form: LegalForm::Sl,
            activity: "Diseño gráfico".into(),
            cnae: "7410".into(),
            signers: vec!["CARMEN ESPAÑOLA ESPAÑOLA".into()],
            memoria_taxpain: true,
            ..Default::default()
        },
    )
    .unwrap();
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(p.zip.as_slice())).unwrap();
    let mut pdf = Vec::new();
    z.by_name("MEMORIA.PDF")
        .unwrap()
        .read_to_end(&mut pdf)
        .unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    let mut xml = String::new();
    z.by_name("DEPOSITO.XML")
        .unwrap()
        .read_to_string(&mut xml)
        .unwrap();
    let c = claves(&xml);
    let get = |k: &str| c.iter().find(|x| x.0 == k).map(|x| x.1.clone());
    assert_eq!(get("8080805").as_deref(), Some("1"), "memoria abreviada");
    assert!(p.warnings.iter().any(|w| w.contains("borrador de TaxPain")));
    assert!(p.warnings.iter().any(|w| w.contains("patrimonio neto")));
}
