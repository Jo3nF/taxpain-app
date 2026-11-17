//! El XML Facturae generado valida contra el XSD oficial 3.2.2 (vendorizado, sin red).

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use taxpain_core::invoice::{
    Address, Exemption, IdType, Invoice, InvoiceKind, InvoiceLine, Party, PartyId, PersonName,
    Rectification, RectificationMethod, RoundingMode, VatTreatment, compute_totals,
};
use taxpain_core::money::{Money, Rate};
use taxpain_invoicing::facturae::{CorrectionReason, FacturaeError, to_facturae};
use taxpain_testkit::xsd;
use time::macros::date;

fn es(street: &str) -> Option<Address> {
    Some(Address {
        street: street.into(),
        post_code: "46001".into(),
        town: "València".into(),
        province: "Valencia".into(),
        country: "ES".into(),
    })
}

fn invoice() -> Invoice {
    let mut l1 = InvoiceLine {
        description: "Consultoría <fiscal> & contable".into(),
        quantity_milli: 2_500,
        unit_price: Money::from_euros(80, 0),
        discount: Rate::percent(10),
        vat: VatTreatment::Taxed {
            rate: Rate::percent(21),
        },
        surcharge: None,
        withholding: Some(Rate::percent(15)),
        advance: false,
    };
    l1.discount = Rate::percent(10);
    let mut l2 = l1.clone();
    l2.description = "Material".into();
    l2.withholding = None;
    l2.discount = Rate::ZERO;
    l2.surcharge = Some(Rate::from_bp(520));
    let l3 = InvoiceLine {
        description: "Formación".into(),
        quantity_milli: 1000,
        unit_price: Money::from_euros(50, 0),
        discount: Rate::ZERO,
        vat: VatTreatment::Exempt(Exemption::E1),
        surcharge: None,
        withholding: None,
        advance: false,
    };
    Invoice {
        kind: InvoiceKind::Completa,
        number: "A2026-0042".into(),
        issue_date: date!(2026 - 10 - 13),
        operation_date: Some(date!(2026 - 10 - 01)),
        issuer: Party {
            id: PartyId::Nif("12345678Z".into()),
            name: "Carmen Española Española".into(),
            address: es("Calle Mayor 1"),
            person: Some(PersonName {
                name: "Carmen".into(),
                first_surname: "Española".into(),
                second_surname: Some("Española".into()),
            }),
        },
        recipient: Some(Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Empresa de Prueba SL".into(),
            address: es("Avenida del Puerto 2"),
            person: None,
        }),
        lines: vec![l1, l2, l3],
        rectified: vec![],
        description: "Servicios profesionales".into(),
        notes: None,
    }
}

fn xsd_path() -> (xsd::tempfile_dir::Dir, PathBuf) {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/facturae");
    let dir = xsd::local_schema_dir(
        &src,
        &[(
            "http://www.w3.org/TR/xmldsig-core/xmldsig-core-schema.xsd",
            "xmldsig-core-schema.xsd",
        )],
    );
    let p = dir.path().join("Facturaev3_2_2.xsd");
    (dir, p)
}

#[test]
fn factura_completa_valida_contra_el_xsd() {
    let f = invoice();
    let t = compute_totals(&f.lines, RoundingMode::PerRate);
    let xml = to_facturae(&f, &t, &CorrectionReason::default()).unwrap();
    let (_dir, xsd_file) = xsd_path();
    if let Err(e) = xsd::validate(&xsd_file, &xml) {
        panic!("Facturae no válido:\n{}\n\n{xml}", e.join("\n"));
    }
    assert!(xml.contains("<PersonTypeCode>F</PersonTypeCode>"));
    assert!(xml.contains("<Name>Carmen</Name><FirstSurname>Española</FirstSurname>"));
    assert!(xml.contains("Consultoría &lt;fiscal&gt; &amp; contable"));
    assert!(xml.contains("<TaxTypeCode>04</TaxTypeCode><TaxRate>15.00</TaxRate>"));
    assert!(xml.contains("<EquivalenceSurcharge>5.20</EquivalenceSurcharge>"));
    // InvoiceTotal = base + impuestos − retenciones
    assert!(xml.contains(&format!("<InvoiceTotal>{}</InvoiceTotal>", {
        let c = t.payable.cents();
        format!("{}.{:02}", c / 100, c % 100)
    })));
    assert!(xml.contains(
        "<LegalReference>Operación exenta de IVA (art. 20 Ley 37/1992)</LegalReference>"
    ));
}

#[test]
fn rectificativa_y_cliente_extranjero_validan() {
    let mut f = invoice();
    f.kind = InvoiceKind::Rectificativa(Rectification {
        code: 1,
        method: RectificationMethod::Diferencias,
    });
    f.number = "R2026-0001".into();
    f.rectified = vec![("A2026-0041".into(), date!(2026 - 09 - 15))];
    f.recipient = Some(Party {
        id: PartyId::Foreign {
            country: "FR".into(),
            id_type: IdType::NifIva,
            id: "FR12345678901".into(),
        },
        name: "Client SARL".into(),
        address: Some(Address {
            street: "1 rue de la Paix".into(),
            post_code: "75002".into(),
            town: "Paris".into(),
            province: "Paris".into(),
            country: "FR".into(),
        }),
        person: None,
    });
    for l in &mut f.lines {
        l.vat = VatTreatment::Exempt(Exemption::E5);
        l.surcharge = None;
    }
    let t = compute_totals(&f.lines, RoundingMode::PerRate);
    let xml = to_facturae(&f, &t, &CorrectionReason::default()).unwrap();
    let (_dir, xsd_file) = xsd_path();
    if let Err(e) = xsd::validate(&xsd_file, &xml) {
        panic!("Facturae no válido:\n{}\n\n{xml}", e.join("\n"));
    }
    assert!(xml.contains("<InvoiceClass>OR</InvoiceClass>"));
    assert!(xml.contains("<ResidenceTypeCode>U</ResidenceTypeCode>"));
    assert!(xml.contains(
        "<TaxPeriod><StartDate>2026-09-01</StartDate><EndDate>2026-09-30</EndDate></TaxPeriod>"
    ));
    assert!(xml.contains("<CountryCode>FRA</CountryCode>"));
}

#[test]
fn errores_de_datos() {
    let mut f = invoice();
    let t = compute_totals(&f.lines, RoundingMode::PerRate);
    f.issuer.person = None;
    assert_eq!(
        to_facturae(&f, &t, &CorrectionReason::default()),
        Err(FacturaeError::MissingPersonName("el emisor"))
    );
    let mut f = invoice();
    f.recipient = None;
    assert_eq!(
        to_facturae(&f, &t, &CorrectionReason::default()),
        Err(FacturaeError::MissingRecipient)
    );
}

#[test]
fn el_validador_detecta_errores() {
    let f = invoice();
    let t = compute_totals(&f.lines, RoundingMode::PerRate);
    let xml = to_facturae(&f, &t, &CorrectionReason::default())
        .unwrap()
        .replace("<Modality>I</Modality>", "<Modality>X</Modality>");
    let (_dir, xsd_file) = xsd_path();
    assert!(xsd::validate(&xsd_file, &xml).is_err());
}

#[test]
fn ida_y_vuelta_con_el_extractor() {
    use taxpain_invoicing::ingest::{Confidence, from_facturae};
    let f = invoice();
    let t = compute_totals(&f.lines, RoundingMode::PerRate);
    let xml = to_facturae(&f, &t, &CorrectionReason::default()).unwrap();
    let e = from_facturae(&xml).unwrap();
    assert_eq!(e.supplier_nif.as_ref().unwrap().value, "12345678Z");
    assert_eq!(
        e.supplier_nif.as_ref().unwrap().confidence,
        Confidence::Alta
    );
    assert_eq!(e.supplier_name.unwrap().value, "Carmen Española Española");
    assert_eq!(e.recipient_nif.unwrap().value, "B12345674");
    assert_eq!(e.number.unwrap().value, "A2026-0042");
    assert_eq!(e.issue_date.unwrap().value, date!(2026 - 10 - 13));
    assert_eq!(e.base.unwrap().value, t.base);
    assert_eq!(e.vat.unwrap().value, t.tax_total);
    assert_eq!(e.withholding.unwrap().value, t.withholding);
    assert_eq!(e.total.unwrap().value, t.payable);
    assert_eq!(e.taxes.len(), t.breakdown.len());
    assert!(e.warnings.is_empty(), "{:?}", e.warnings);
    assert!(from_facturae("<otro/>").is_none());
}

#[test]
fn factura_en_dolares_valida_con_contravalor() {
    let mut f = invoice();
    f.recipient = Some(Party {
        id: PartyId::Foreign {
            country: "US".into(),
            id_type: IdType::DocumentoOficial,
            id: "12-3456789".into(),
        },
        name: "Client Inc.".into(),
        address: Some(Address {
            street: "1 Main Street".into(),
            post_code: "10001".into(),
            town: "New York".into(),
            province: "NY".into(),
            country: "US".into(),
        }),
        person: None,
    });
    let t = compute_totals(&f.lines, RoundingMode::PerRate);
    let rate = taxpain_core::fx::ExchangeRate::parse("1.1193").unwrap();
    let eur = taxpain_core::fx::convert(&f.lines, rate);
    let xml = taxpain_invoicing::facturae::to_facturae_fx(
        &f,
        &t,
        &CorrectionReason::default(),
        &taxpain_invoicing::facturae::FacturaeFx {
            code: "USD",
            rate,
            rate_date: date!(2026 - 10 - 01),
            eur_lines: &eur.lines,
            eur_totals: &eur.totals,
        },
    )
    .unwrap();
    let (_dir, xsd_file) = xsd_path();
    if let Err(e) = xsd::validate(&xsd_file, &xml) {
        panic!("Facturae no válido:\n{}\n\n{xml}", e.join("\n"));
    }
    // Si llega como factura recibida, se avisa de que los importes leídos son en dólares y se
    // preseleccionan la moneda, el tipo, su fecha y el IVA en euros de la factura.
    let e = taxpain_invoicing::ingest::from_facturae(&xml).unwrap();
    assert!(
        e.warnings
            .iter()
            .any(|w| w.contains("en USD (tipo de la factura: 1.1193)")),
        "{:?}",
        e.warnings
    );
    let fx = e.fx.as_ref().unwrap();
    assert_eq!(fx.code, "USD");
    assert_eq!(fx.rate.as_deref(), Some("1.1193"));
    assert_eq!(fx.rate_date, Some(date!(2026 - 10 - 01)));
    assert_eq!(fx.vat_eur, Some(eur.totals.vat + eur.totals.surcharge));
    // Un emisor que da el tipo al revés (euros por dólar): se invierte con el contravalor.
    let inverted = xml.replace(
        "<ExchangeRate>1.1193</ExchangeRate>",
        "<ExchangeRate>0.893416</ExchangeRate>",
    );
    let e = taxpain_invoicing::ingest::from_facturae(&inverted).unwrap();
    let rate = e.fx.as_ref().unwrap().rate.clone().unwrap();
    assert!(rate.starts_with("1.119"), "{rate}");
    assert!(e.warnings.iter().any(|w| w.contains("se ha invertido")));
    // En euros, sin moneda.
    assert!(
        taxpain_invoicing::ingest::from_facturae(
            &to_facturae(&f, &t, &CorrectionReason::default()).unwrap()
        )
        .unwrap()
        .fx
        .is_none()
    );
    let a = |m: Money| format!("{}.{:02}", m.cents() / 100, m.cents() % 100);
    assert!(xml.contains("<InvoiceCurrencyCode>USD</InvoiceCurrencyCode></Batch>"));
    assert!(xml.contains(
        "<InvoiceCurrencyCode>USD</InvoiceCurrencyCode><ExchangeRateDetails>\
         <ExchangeRate>1.1193</ExchangeRate><ExchangeRateDate>2026-10-01</ExchangeRateDate>\
         </ExchangeRateDetails><TaxCurrencyCode>EUR</TaxCurrencyCode>"
    ));
    // Lote: total en dólares y su contravalor en euros.
    assert!(xml.contains(&format!(
        "<TotalInvoicesAmount><TotalAmount>{}</TotalAmount><EquivalentInEuros>{}\
         </EquivalentInEuros></TotalInvoicesAmount>",
        a(t.payable),
        a(eur.totals.payable)
    )));
    // Base y cuota del 21 % con su contravalor (la cuota en euros, sobre la base en euros).
    let (b, e) = (&t.breakdown[0], &eur.totals.breakdown[0]);
    assert!(
        xml.contains(&format!(
            "<TaxableBase><TotalAmount>{}</TotalAmount><EquivalentInEuros>{}</EquivalentInEuros>\
             </TaxableBase><TaxAmount><TotalAmount>{}</TotalAmount><EquivalentInEuros>{}\
             </EquivalentInEuros></TaxAmount>",
            a(b.base),
            a(e.base),
            a(b.vat_amount),
            a(e.vat_amount)
        )),
        "{xml}"
    );
}
