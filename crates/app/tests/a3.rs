//! Exportación a a3 (`SUENLACE.DAT`): el diario del ejercicio, cuadrado y con los terceros.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::a3::{A3Options, suenlace};
use taxpain_app::accounting::{close_year, journal, manual_entry};
use taxpain_app::invoicing::{self, InvoiceDraft, issue};
use taxpain_app::received::{AssetClass, Nature, Reviewed, Source, ingest_file, review};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::suenlace::Format;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment};
use taxpain_core::invoice::{Rectification, RectificationMethod};
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

fn field(rec: &[u8], pos: usize, len: usize) -> String {
    String::from_utf8_lossy(&rec[pos - 1..pos - 1 + len]).into_owned()
}

#[test]
fn diario_del_ejercicio_en_suenlace() {
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    manual_entry(
        &v,
        &JournalEntry {
            date: date!(2026 - 01 - 02),
            concept: "Aportación inicial".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("5720001"), Money::from_euros(3000, 0)),
                EntryLine::credit(acc("102"), Money::from_euros(3000, 0)),
            ],
        },
        now,
    )
    .unwrap();
    issue(
        &v,
        &draft("B12345674", "Cliente Uno SL", 1000, date!(2026 - 03 - 10)),
        now,
        true,
    )
    .unwrap();
    close_year(&v, 2026, now, true).unwrap();
    let journal_len = journal(&v, 2026).unwrap().len();

    let o = A3Options {
        company: 12,
        digits: 8,
        format: Format::F4,
        invoices: false,
    };
    let x = suenlace(&v, 2026, &o).unwrap();
    // Aportación y factura; la regularización y el cierre no.
    assert_eq!(x.entries, 2);
    assert!(journal_len > 2);
    assert_eq!(x.accounts, 1);
    assert!(x.warnings.iter().any(|w| w.contains("cierre")));
    assert_eq!(x.file.len() % 256, 0);
    let recs: Vec<&[u8]> = x.file.chunks(256).collect();
    assert!(
        recs.iter()
            .all(|r| &r[254..] == b"\r\n" && field(r, 2, 5) == "00012")
    );

    // El cliente, con su NIF, a 8 dígitos.
    let c = recs[0];
    assert_eq!(field(c, 15, 1), "C");
    assert_eq!(field(c, 16, 12), "43000001    ");
    assert_eq!(field(c, 28, 14), "Cliente Uno SL");
    assert_eq!(field(c, 78, 9), "B12345674");

    // Apuntes: cada asiento empieza por I y termina por U, y debe = haber.
    let apuntes: Vec<&[u8]> = recs[1..].to_vec();
    assert!(apuntes.iter().all(|r| field(r, 15, 1) == "0"));
    assert_eq!(field(apuntes[0], 69, 1), "I");
    assert_eq!(field(apuntes[0], 16, 8), "57200001");
    assert_eq!(field(apuntes[1], 16, 8), "10200000");
    assert_eq!(field(apuntes[1], 69, 1), "U");
    let (mut debe, mut haber) = (0i64, 0i64);
    for r in &apuntes {
        let s = field(r, 100, 14);
        let cents: i64 = s.replace('.', "").parse().unwrap();
        if field(r, 58, 1) == "D" {
            debe += cents;
        } else {
            haber += cents;
        }
    }
    assert_eq!(debe, haber);
    assert!(apuntes.iter().any(|r| field(r, 16, 8) == "43000001"));
    assert!(apuntes.iter().any(|r| field(r, 16, 8) == "70500000"));

    assert!(
        suenlace(
            &v,
            2026,
            &A3Options {
                company: 0,
                ..o.clone()
            }
        )
        .is_err()
    );
}

fn reviewed(nif: &str, number: &str, base: i64, vat: i64, pct: u8) -> Reviewed {
    Reviewed {
        supplier_nif: nif.into(),
        supplier_name: "Proveedor SA".into(),
        number: number.into(),
        issue_date: date!(2026 - 04 - 01),
        base: Money::from_euros(base, 0),
        vat: Money::from_euros(vat, 0),
        withholding: Money::ZERO,
        total: Money::from_euros(base + vat, 0),
        deductible_pct: pct,
        capital_good: false,
        nature: Nature::General,
        asset_class: None::<AssetClass>,
        vat_regime: Default::default(),
        supplier_country: None,
        self_assessed_rate: None,
        intra_goods: false,
        cash_basis: false,
        import_dua: false,
        prorata_use: Default::default(),
    }
}

#[test]
fn facturas_con_iva_en_formato_5() {
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    let a = issue(
        &v,
        &draft("B12345674", "Cliente Uno SL", 1000, date!(2026 - 03 - 10)),
        now,
        true,
    )
    .unwrap();
    let mut r = draft("B12345674", "Cliente Uno SL", -100, date!(2026 - 03 - 20));
    r.kind = InvoiceKind::Rectificativa(Rectification {
        code: 4,
        method: RectificationMethod::Diferencias,
    });
    r.rectified = vec![(a.number.clone(), date!(2026 - 03 - 10))];
    issue(&v, &r, now, true).unwrap();
    for (i, (num, pct)) in [("P-1", 100u8), ("P-2", 50)].iter().enumerate() {
        let rec = ingest_file(
            &v,
            &format!("f{i}.xml"),
            format!("<?xml version=\"1.0\"?><x n=\"{i}\"/>").as_bytes(),
            Source::Manual,
            now,
        )
        .unwrap();
        review(&v, rec.id, &reviewed("A22222228", num, 200, 42, *pct), now).unwrap();
    }

    let o = A3Options {
        company: 3,
        digits: 8,
        format: Format::F5,
        invoices: true,
    };
    let x = suenlace(&v, 2026, &o).unwrap();
    assert_eq!(x.entries, 4);
    assert_eq!(x.invoices, 3, "{:?}", x.warnings);
    assert!(
        x.warnings
            .iter()
            .any(|w| w.contains("1 factura(s) van como asiento") && w.contains("no coincide")),
        "{:?}",
        x.warnings
    );
    assert!(x.warnings.iter().any(|w| w.contains("una sola vez")));
    assert_eq!(x.file.len() % 512, 0);
    let recs: Vec<&[u8]> = x.file.chunks(512).collect();
    assert!(
        recs.iter()
            .all(|r| &r[510..] == b"\r\n" && field(r, 1, 6) == "500003")
    );
    let kinds: String = recs.iter().map(|r| field(r, 15, 1)).collect();
    // Dos terceros, factura (1+9), rectificativa (2+9+4), recibida (1+9) y la del IVA no
    // deducible como apuntes.
    assert_eq!(&kinds[..9], "CC1929419");
    assert!(kinds[9..].chars().all(|c| c == '0'));

    // Factura emitida: total a cobrar 1.060 (1.000 + 210 − 150), número completo en la 253.
    let h = recs[2];
    assert_eq!(field(h, 58, 1), "1");
    assert_eq!(field(h, 100, 14), "+0000001060.00");
    assert_eq!(field(h, 253, 10), a.number);
    assert_eq!(field(h, 509, 2), "EN");
    let l = recs[3];
    assert_eq!(field(l, 16, 8), "70500000");
    assert_eq!(field(l, 69, 1), "U");
    assert_eq!(field(l, 100, 16), "01+0000001000.00");
    assert_eq!(field(l, 116, 5), "21.00");
    assert_eq!(field(l, 154, 19), "15.00+0000000150.00");
    assert_eq!(field(l, 173, 3), "01S");
    assert_eq!(field(l, 192, 8), "47700000");
    assert_eq!(field(l, 216, 8), "47300000");

    // Rectificativa: cabecera 2 con importes en positivo (reduce la original) y registro 4.
    let h = recs[4];
    assert_eq!(field(h, 15, 1), "2");
    assert_eq!(field(h, 100, 14), "+0000000106.00");
    assert_eq!(field(recs[5], 102, 14), "+0000000100.00");
    let r4 = recs[6];
    assert_eq!(field(r4, 15, 1), "4");
    assert_eq!(field(r4, 59, 8), "20260310");
    assert_eq!(field(r4, 67, 10), a.number);
    assert_eq!(field(r4, 157, 1), "R");

    // Recibida: libro 2, gasto 629, IVA soportado 472 al 21 %.
    let h = recs[7];
    assert_eq!(field(h, 58, 1), "2");
    assert_eq!(field(h, 100, 14), "+0000000242.00");
    assert_eq!(field(h, 176, 9), "A22222228");
    let l = recs[8];
    assert_eq!(field(l, 16, 3), "629");
    assert_eq!(field(l, 116, 5), "21.00");
    assert_eq!(field(l, 121, 14), "+0000000042.00");
    assert_eq!(field(l, 192, 3), "472");
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
