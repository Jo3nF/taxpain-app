//! Legalización de libros: ZIP con el Diario y el de Inventarios y Cuentas anuales.

#![allow(clippy::unwrap_used)]

use std::io::Read;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use taxpain_app::accounting::{close_year, manual_entry};
use taxpain_app::invoicing;
use taxpain_app::legalizacion::{Request, base64, build};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::calendar::TaxpayerProfile;
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
            name: "Ejemplo SL".into(),
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

fn entry(d: time::Date, debit: &str, credit: &str, euros: i64) -> JournalEntry {
    JournalEntry {
        date: d,
        concept: "Operación".into(),
        origin: Origin::Manual,
        lines: vec![
            EntryLine::debit(acc(debit), Money::from_euros(euros, 0)),
            EntryLine::credit(acc(credit), Money::from_euros(euros, 0)),
        ],
    }
}

fn unzip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    (0..z.len())
        .map(|i| {
            let mut f = z.by_index(i).unwrap();
            assert_eq!(f.compression(), zip::CompressionMethod::Deflated);
            let mut b = Vec::new();
            f.read_to_end(&mut b).unwrap();
            (f.name().unwrap().into_owned(), b)
        })
        .collect()
}

#[test]
fn paquete_de_legalizacion() {
    let v = setup();
    let now = datetime!(2027-02-10 10:00 UTC);
    for e in [
        entry(date!(2026 - 01 - 02), "5720001", "100", 3000),
        entry(date!(2026 - 05 - 10), "629", "5720001", 400),
        entry(date!(2026 - 11 - 20), "5720001", "705", 1000),
    ] {
        manual_entry(&v, &e, now).unwrap();
    }
    let req = Request {
        year: 2026,
        diary_number: 3,
        inventory_number: 3,
        registry_code: Some("46001".into()),
        ..Default::default()
    };
    assert!(
        build(&v, &req, now).is_err(),
        "el ejercicio ha de estar cerrado"
    );
    close_year(&v, 2026, now, true).unwrap();

    let p = build(&v, &req, now).unwrap();
    assert_eq!(p.filename, "LEGALIZACION_2026_B12345674.ZIP");
    assert_eq!(p.deadline, date!(2027 - 04 - 30));
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    let files = unzip(&p.zip);
    let names: Vec<&str> = files.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(
        names,
        [
            "DIARIO_003.xlsx",
            "INV_CUEN_003.xlsx",
            "NOMBRES.TXT",
            "DATOS.TXT"
        ]
    );
    // Huellas: SHA-256 en Base64 de cada libro.
    for (b, (_, bytes)) in p.books.iter().zip(&files) {
        assert_eq!(b.huella, base64(&Sha256::digest(bytes)));
        assert_eq!(b.huella.len(), 44);
    }
    let text =
        |n: &str| String::from_utf8(files.iter().find(|f| f.0 == n).unwrap().1.clone()).unwrap();
    assert_eq!(
        text("NOMBRES.TXT"),
        "DIARIO_003.xlsx\r\nINV_CUEN_003.xlsx\r\n"
    );
    let datos = text("DATOS.TXT");
    assert!(datos.starts_with("10110022027\r\n102Ejemplo SL\r\n105B12345674\r\n"));
    assert!(datos.contains("\r\n107Valencia\r\n10846001\r\n10946\r\n11246001\r\n"));
    assert!(datos.contains("\r\n305Calle Mayor 1\r\n"));
    assert!(datos.contains("\r\n401NO\r\n5012\r\n00101Libro Diario 2026\r\n001023\r\n0010301012026\r\n0010431122026\r\n00105\r\n"));
    assert!(
        !datos.contains("\r\n201"),
        "sin datos registrales no se escriben"
    );
    assert!(datos.contains(&format!("00106{}", p.books[0].huella)));
    // El libro de inventarios lleva sus hojas.
    let inv = &files[1].1;
    assert!(datos.ends_with(&format!("00206{}\r\n", p.books[1].huella)));
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(inv.as_slice())).unwrap();
    let mut wb = String::new();
    z.by_name("xl/workbook.xml")
        .unwrap()
        .read_to_string(&mut wb)
        .unwrap();
    for sheet in [
        "Balance inicial",
        "Sumas y saldos 1T",
        "Sumas y saldos 4T",
        "Inventario de cierre",
        "Balance",
        "Pérdidas y ganancias",
    ] {
        assert!(wb.contains(&format!("name=\"{sheet}\"")), "{sheet}");
    }

    // Diario resumido: más pequeño, mismas hojas.
    let s = build(
        &v,
        &Request {
            summarized: true,
            ..req.clone()
        },
        now,
    )
    .unwrap();
    assert_ne!(s.books[0].huella, p.books[0].huella);
    // Fuera de plazo, aviso.
    let late = build(&v, &req, datetime!(2027-06-01 10:00 UTC)).unwrap();
    assert!(late.warnings.iter().any(|w| w.contains("plazo")));
}

/// Ejercicio de julio a junio: los balances de sumas y saldos van por trimestres del ejercicio
/// (30/9, 31/12, 31/3 y 30/6), no por los del año natural (art. 28.1 CCom).
#[test]
fn inventario_por_trimestres_del_ejercicio_no_natural() {
    let v = setup();
    let mut p = TaxpayerProfile::sociedad_limitada();
    p.cierre_ejercicio = (time::Month::June, 30);
    v.set_setting("taxpayer_profile", &p).unwrap();
    let now = datetime!(2027-08-10 10:00 UTC);
    for e in [
        entry(date!(2026 - 09 - 10), "5720001", "705", 1000),
        entry(date!(2027 - 03 - 05), "629", "5720001", 400),
    ] {
        manual_entry(&v, &e, now).unwrap();
    }
    close_year(&v, 2026, now, true).unwrap();
    let req = Request {
        year: 2026,
        diary_number: 1,
        inventory_number: 1,
        ..Default::default()
    };
    let p = build(&v, &req, now).unwrap();
    let files = unzip(&p.zip);
    assert_eq!(files[1].0, "INV_CUEN_001.xlsx");
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(files[1].1.as_slice())).unwrap();
    let mut all = String::new();
    for i in 0..z.len() {
        let mut s = String::new();
        if z.by_index(i).unwrap().read_to_string(&mut s).is_ok() {
            all.push_str(&s);
        }
    }
    for d in ["30/09/2026", "31/12/2026", "31/03/2027", "30/06/2027"] {
        assert!(all.contains(&format!("Sumas y saldos a {d}")), "{d}");
    }
    for d in ["31/03/2026", "30/06/2026", "30/09/2027", "31/12/2027"] {
        assert!(!all.contains(&format!("Sumas y saldos a {d}")), "{d}");
    }
    assert!(all.contains("name=\"Sumas y saldos 4T\""));
}
