//! La factura UBL (EN 16931, Anexo I de la Orden HAC/1028/2026) valida contra el esquema
//! oficial OASIS UBL 2.1 (vendorizado) y lleva los datos que exige el anexo.

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use taxpain_core::invoice::{
    Address, Exemption, IdType, Invoice, InvoiceKind, InvoiceLine, Party, PartyId, PersonName,
    Rectification, RectificationMethod, RoundingMode, VatTreatment, compute_totals,
};
use taxpain_core::money::{Money, Rate};
use taxpain_invoicing::ubl::{
    CUSTOMIZATION_ID, PROFILE_ID, UblCurrency, UblError, UblOptions, to_ubl,
};
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

fn schema() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/oasis/ubl-2.1/maindoc/UBL-Invoice-2.1.xsd")
}

fn valid(xml: &str) {
    if let Err(e) = xsd::validate(&schema(), xml) {
        panic!("UBL no válido:\n{}\n\n{xml}", e.join("\n"));
    }
}

#[test]
fn factura_ordinaria_con_retencion_recargo_y_exenta() {
    let f = invoice();
    let t = compute_totals(&f.lines, RoundingMode::PerRate);
    let xml = to_ubl(
        &f,
        &t,
        &UblOptions {
            verifactu_qr: Some("https://prewww2.aeat.es/wlpl/TIKE-CONT/ValidarQR?nif=12345678Z&numserie=A2026-0042&fecha=13-10-2026&importe=1.00".into()),
            withholding_key: None,
            currency: None,
        },
    )
    .unwrap();
    valid(&xml);
    for needle in [
        PROFILE_ID,
        CUSTOMIZATION_ID,
        "<cbc:InvoiceTypeCode>380</cbc:InvoiceTypeCode>",
        "<cbc:CompanyID schemeID=\"FC\">12345678Z</cbc:CompanyID>",
        "<cac:TaxScheme><cbc:ID>LOC</cbc:ID></cac:TaxScheme>",
        "<cbc:TaxPointDate>2026-10-01</cbc:TaxPointDate>",
        // Retención de IRPF (BT-ES-17) con esquema WTH y clave 01.
        "<cac:TaxScheme><cbc:ID>WTH</cbc:ID><cbc:TaxTypeCode>01</cbc:TaxTypeCode>",
        // Recargo de equivalencia del 5,2 % como desglose propio.
        "<cbc:ID>N</cbc:ID><cbc:Percent>5.20</cbc:Percent>",
        // Exenta con su motivo y mención obligatoria.
        "<cbc:TaxExemptionReasonCode>VATEX-EU-132</cbc:TaxExemptionReasonCode>",
        "<cbc:Note>#REG#Operación exenta (art. 20 Ley 37/1992)</cbc:Note>",
        // Clave de régimen en cada línea.
        "<cbc:Name>REGI</cbc:Name><cbc:Value>01</cbc:Value><cbc:ValueQualifier>IVA</cbc:ValueQualifier>",
        "<cbc:ID>QR</cbc:ID>",
        "Consultoría &lt;fiscal&gt; &amp; contable",
    ] {
        assert!(xml.contains(needle), "falta {needle}");
    }
    assert_eq!(xml.matches("<cac:InvoiceLine>").count(), 3);
    assert!(xml.contains(&format!(
        "<cbc:PayableAmount currencyID=\"EUR\">{}.{:02}</cbc:PayableAmount>",
        t.payable.cents() / 100,
        t.payable.cents() % 100
    )));
}

#[test]
fn rectificativa_y_cliente_intracomunitario() {
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
            id: "12345678901".into(),
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
    let xml = to_ubl(&f, &t, &UblOptions::default()).unwrap();
    valid(&xml);
    for needle in [
        "<cbc:InvoiceTypeCode>384</cbc:InvoiceTypeCode>",
        "<cac:InvoiceDocumentReference><cbc:ID>A2026-0041</cbc:ID><cbc:IssueDate>2026-09-15</cbc:IssueDate>",
        "<cbc:ID>RECT:TIPO</cbc:ID><cbc:DocumentTypeCode>R1</cbc:DocumentTypeCode>",
        "<cbc:ID>RECT:MODALIDAD</cbc:ID><cbc:DocumentTypeCode>I</cbc:DocumentTypeCode>",
        "<cbc:CompanyID>FR12345678901</cbc:CompanyID><cac:TaxScheme><cbc:ID>VAT</cbc:ID>",
        "<cbc:TaxExemptionReasonCode>VATEX-EU-IC</cbc:TaxExemptionReasonCode>",
    ] {
        assert!(xml.contains(needle), "falta {needle}");
    }
}

#[test]
fn factura_en_libras_con_cuota_en_euros() {
    let f = invoice();
    let t = compute_totals(&f.lines, RoundingMode::PerRate);
    let rate = taxpain_core::fx::ExchangeRate::parse("0,83728").unwrap();
    let eur = taxpain_core::fx::convert(&f.lines, rate).totals;
    let xml = to_ubl(
        &f,
        &t,
        &UblOptions {
            currency: Some(UblCurrency {
                code: "GBP".into(),
                rate,
                rate_date: date!(2026 - 10 - 01),
                eur_tax_total: eur.tax_total,
            }),
            ..UblOptions::default()
        },
    )
    .unwrap();
    valid(&xml);
    let a = |m: Money| format!("{}.{:02}", m.cents() / 100, m.cents() % 100);
    for needle in [
        "<cbc:DocumentCurrencyCode>GBP</cbc:DocumentCurrencyCode><cbc:TaxCurrencyCode>EUR</cbc:TaxCurrencyCode>".to_owned(),
        "<cac:TaxExchangeRate><cbc:SourceCurrencyCode>GBP</cbc:SourceCurrencyCode><cbc:TargetCurrencyCode>EUR</cbc:TargetCurrencyCode><cbc:CalculationRate>0.83728</cbc:CalculationRate><cbc:MathematicOperatorCode>Divide</cbc:MathematicOperatorCode><cbc:Date>2026-10-01</cbc:Date></cac:TaxExchangeRate>".to_owned(),
        // BT-110 en la moneda y BT-111 en euros (BR-53).
        format!("<cac:TaxTotal><cbc:TaxAmount currencyID=\"GBP\">{}</cbc:TaxAmount>", a(t.tax_total)),
        format!("<cac:TaxTotal><cbc:TaxAmount currencyID=\"EUR\">{}</cbc:TaxAmount></cac:TaxTotal>", a(eur.tax_total)),
        format!("<cbc:PayableAmount currencyID=\"GBP\">{}</cbc:PayableAmount>", a(t.payable)),
    ] {
        assert!(xml.contains(&needle), "falta {needle}\n{xml}");
    }
    // Ningún otro importe en euros.
    assert_eq!(xml.matches("currencyID=\"EUR\"").count(), 1);
}

#[test]
fn errores() {
    let mut f = invoice();
    let t = compute_totals(&f.lines, RoundingMode::PerRate);
    f.issuer.address = None;
    assert_eq!(
        to_ubl(&f, &t, &UblOptions::default()),
        Err(UblError::MissingAddress("emisor"))
    );
    f.recipient = None;
    assert_eq!(
        to_ubl(&f, &t, &UblOptions::default()),
        Err(UblError::MissingRecipient)
    );
}
