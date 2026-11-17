//! El PDF se genera y su texto (extraído con `pdftotext`) contiene lo exigido por el art. 6.

#![allow(clippy::unwrap_used)]

use std::process::Command;

use taxpain_core::fx::{self, ExchangeRate, RateSource};
use taxpain_core::invoice::{
    Address, Exemption, Invoice, InvoiceKind, InvoiceLine, Party, PartyId, PersonName,
    Rectification, RectificationMethod, RoundingMode, VatTreatment, compute_totals,
};
use taxpain_core::money::{Money, Rate};
use taxpain_invoicing::pdf::{
    EurEquivalent, Language, PdfCurrency, PdfOptions, VerifactuMark, render, render_quote,
};
use time::macros::date;

fn invoice() -> Invoice {
    let line = |d: &str, cents: i64, vat: VatTreatment| InvoiceLine {
        description: d.into(),
        quantity_milli: 1000,
        unit_price: Money::from_cents(cents),
        discount: Rate::ZERO,
        vat,
        surcharge: None,
        withholding: None,
        advance: false,
    };
    let mut honorarios = line(
        "Consultoría fiscal — ñandú y acentos áéíóú",
        100_000,
        VatTreatment::Taxed {
            rate: Rate::percent(21),
        },
    );
    honorarios.withholding = Some(Rate::percent(15));
    let mut re = line(
        "Mercancía con recargo",
        10_000,
        VatTreatment::Taxed {
            rate: Rate::percent(21),
        },
    );
    re.surcharge = Some(Rate::from_bp(520));
    Invoice {
        kind: InvoiceKind::Completa,
        number: "A2026-0042".into(),
        issue_date: date!(2026 - 10 - 13),
        operation_date: Some(date!(2026 - 10 - 01)),
        issuer: Party {
            id: PartyId::Nif("12345678Z".into()),
            name: "Carmen Española Española".into(),
            address: Some(Address {
                street: "Calle Mayor 1".into(),
                post_code: "46001".into(),
                town: "València".into(),
                province: "Valencia".into(),
                country: "ES".into(),
            }),
            person: Some(PersonName {
                name: "Carmen".into(),
                first_surname: "Española".into(),
                second_surname: Some("Española".into()),
            }),
        },
        recipient: Some(Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Empresa de Prueba SL".into(),
            address: Some(Address {
                street: "Avenida del Puerto 2".into(),
                post_code: "46021".into(),
                town: "València".into(),
                province: "Valencia".into(),
                country: "ES".into(),
            }),
            person: None,
        }),
        lines: vec![
            honorarios,
            re,
            line(
                "Curso de formación",
                5_000,
                VatTreatment::Exempt(Exemption::E1),
            ),
        ],
        rectified: vec![],
        description: "Servicios".into(),
        notes: Some("Pago por transferencia a ES00 0000 0000 0000 0000 0000".into()),
    }
}

fn pdf_text(pdf: &[u8]) -> Option<String> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.pdf");
    std::fs::write(&path, pdf).unwrap();
    let out = Command::new("pdftotext")
        .args(["-layout"])
        .arg(&path)
        .arg("-")
        .output()
        .ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn factura_completa_con_menciones() {
    let f = invoice();
    let t = compute_totals(&f.lines, RoundingMode::PerRate);
    let pdf = render(
        &f,
        &t,
        &PdfOptions {
            footer: None,
            ..PdfOptions::default()
        },
    )
    .unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    let Some(text) = pdf_text(&pdf) else {
        eprintln!("pdftotext no disponible: sólo se comprueba la cabecera");
        return;
    };
    for needle in [
        "FACTURA",
        "A2026-0042",
        "13/10/2026",
        "01/10/2026",
        "NIF 12345678Z",
        "NIF B12345674",
        "Carmen Española Española",
        "ñandú y acentos áéíóú",
        "1.000,00 €",
        "IVA 21 %",
        "210,00 €",
        "Recargo de equivalencia 5,2 %",
        "5,20 €",
        "Retención IRPF 15 %",
        "-150,00 €",
        "TOTAL FACTURA",
        "1.386,20 €", // bases 1.150 + IVA 231 + RE 5,20
        "TOTAL A PAGAR",
        "1.236,20 €", // 1.386,20 − 150
        "exenta de IVA (art. 20 Ley 37/1992)",
        "Pago por transferencia",
    ] {
        assert!(text.contains(needle), "falta «{needle}» en:\n{text}");
    }
    assert_eq!(t.invoice_total, Money::from_cents(138_620));
}

#[test]
fn rectificativa_en_ingles_con_qr_y_varias_paginas() {
    let mut f = invoice();
    f.kind = InvoiceKind::Rectificativa(Rectification {
        code: 1,
        method: RectificationMethod::Diferencias,
    });
    f.rectified = vec![("A2026-0041".into(), date!(2026 - 10 - 02))];
    let extra = f.lines[2].clone();
    f.lines.extend(std::iter::repeat_n(extra, 60));
    let t = compute_totals(&f.lines, RoundingMode::PerRate);
    let opts = PdfOptions {
        language: Language::En,
        verifactu: Some(VerifactuMark {
            qr_url: "https://prewww2.aeat.es/wlpl/TIKE-CONT/ValidarQR?nif=12345678Z&numserie=R2026-0001&fecha=13-10-2026&importe=121.00".into(),
            legend: taxpain_invoicing::verifactu::LEGEND.into(),
        }),
        footer: Some("IBAN ES00".into()),
        currency: None,
    };
    let pdf = render(&f, &t, &opts).unwrap();
    let Some(text) = pdf_text(&pdf) else { return };
    assert!(text.contains("CORRECTIVE INVOICE"));
    assert!(text.contains("Corrects invoice A2026-0041 (02/10/2026)"));
    assert!(text.contains("QR tributario:"));
    assert!(text.contains("Factura verificable en la"), "{text}");
    assert!(
        text.contains("Page 1/2") || text.contains("Page 1/3"),
        "{text}"
    );
}

#[test]
fn factura_en_dolares_con_contravalor_en_euros() {
    let mut f = invoice();
    f.lines = vec![InvoiceLine {
        description: "Desarrollo web".into(),
        quantity_milli: 1000,
        unit_price: Money::from_euros(1000, 0),
        discount: Rate::ZERO,
        vat: VatTreatment::Taxed {
            rate: Rate::percent(21),
        },
        surcharge: None,
        withholding: None,
        advance: false,
    }];
    let t = compute_totals(&f.lines, RoundingMode::PerRate);
    let rate = ExchangeRate::parse("1,1193").unwrap();
    let eur = fx::convert(&f.lines, rate).totals;
    let opts = PdfOptions {
        currency: Some(PdfCurrency {
            code: "USD".into(),
            eur: Some(EurEquivalent {
                rate,
                rate_date: date!(2026 - 10 - 09),
                source: RateSource::Bce,
                totals: eur.clone(),
            }),
        }),
        ..PdfOptions::default()
    };
    let pdf = render(&f, &t, &opts).unwrap();
    assert_eq!(eur.vat, Money::from_cents(18_762));
    let Some(text) = pdf_text(&pdf) else { return };
    for needle in [
        "1.000,00 USD",
        "210,00 USD",
        "1.210,00 USD",
        "Contravalor en euros (art. 12.1 RD 1619/2012)",
        "1 € = 1,1193 USD, tipo de referencia del BCE del 09/10/2026",
        "Base imponible (21 %) en euros",
        "893,42 €",
        "IVA 21 % en euros",
        "187,62 €",
        "1.081,04 €",
    ] {
        assert!(text.contains(needle), "falta «{needle}» en:\n{text}");
    }
    // Un presupuesto en dólares: sólo la moneda, sin contravalor.
    let q = render_quote(
        &f,
        &t,
        &PdfOptions {
            language: Language::En,
            currency: Some(PdfCurrency {
                code: "USD".into(),
                eur: None,
            }),
            ..PdfOptions::default()
        },
        taxpain_invoicing::pdf::QuoteKind::Presupuesto,
        None,
    )
    .unwrap();
    let Some(text) = pdf_text(&q) else { return };
    assert!(text.contains("1.210,00 USD"), "{text}");
    assert!(!text.contains("in EUR"), "{text}");
    // No es una factura: el total no dice «INVOICE TOTAL».
    assert!(text.contains("TOTAL"), "{text}");
    assert!(!text.contains("INVOICE TOTAL"), "{text}");
}
