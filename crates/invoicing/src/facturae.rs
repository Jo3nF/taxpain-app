//! Facturae 3.2.2 (esquema en `fixtures/facturae/`), modalidad individual, emitida por el
//! propio emisor (`EM`). Se genera sin firmar; la firma XAdES-EPES (la que pide FACe) se añade
//! aparte con el certificado del usuario (`crate::xades`).
//!
//! `TODO(VERIFY)`: representación de operaciones exentas, no sujetas e inversión del sujeto
//! pasivo (aquí: impuesto 01 al 0 % + literal legal) y elección del `ReasonCode` de las
//! rectificativas según la guía de FACe.
//!
//! Moneda extranjera ([`to_facturae_fx`]): `InvoiceCurrencyCode` es la de la factura,
//! `TaxCurrencyCode` el euro y `ExchangeRateDetails` el tipo (art. 79.Once LIVA) y su fecha; las
//! bases, cuotas y totales del lote llevan su `EquivalentInEuros`, como pide el esquema.
//! `ExchangeRate` va como lo publica el BCE (unidades de la moneda por 1 €): el esquema no
//! dice el sentido, `TODO(VERIFY)` con la guía de FACe.

use std::fmt::Write;

use taxpain_core::fx::ExchangeRate;
use taxpain_core::invoice::{
    Address, Invoice, InvoiceKind, InvoiceLine, Party, PartyId, RectificationMethod, Totals,
    VatTreatment, totals::line_base,
};
use taxpain_core::money::{Money, Rate};
use taxpain_core::nif::Nif;
use time::Date;

pub const NAMESPACE: &str = "http://www.facturae.gob.es/formato/Versiones/Facturaev3_2_2.xml";

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum FacturaeError {
    #[error("Facturae exige destinatario")]
    MissingRecipient,
    #[error("Facturae exige el domicilio de {0}")]
    MissingAddress(&'static str),
    #[error("país no admitido en Facturae: {0}")]
    UnknownCountry(String),
    #[error("{0} es persona física: indica nombre y primer apellido")]
    MissingPersonName(&'static str),
    #[error("el número de factura supera 20 caracteres")]
    NumberTooLong,
}

/// Motivo de rectificación para Facturae (`ReasonCode` + descripción literal del esquema).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CorrectionReason {
    pub code: &'static str,
    pub description: &'static str,
}

impl Default for CorrectionReason {
    fn default() -> Self {
        CorrectionReason {
            code: "16",
            description: "Base imponible",
        }
    }
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// Recorta a `max` caracteres (los tipos `TextMaxNN` del esquema).
fn cut(s: &str, max: usize) -> String {
    esc(&s.chars().take(max).collect::<String>())
}

fn amount(m: Money) -> String {
    let c = m.cents();
    let sign = if c < 0 { "-" } else { "" };
    format!(
        "{sign}{}.{:02}",
        c.unsigned_abs() / 100,
        c.unsigned_abs() % 100
    )
}

/// Contenido de un `AmountType`: importe y, en moneda extranjera, su contravalor en euros.
fn amount_type(m: Money, eur: Option<Money>) -> String {
    match eur {
        Some(e) => format!(
            "<TotalAmount>{}</TotalAmount><EquivalentInEuros>{}</EquivalentInEuros>",
            amount(m),
            amount(e)
        ),
        None => format!("<TotalAmount>{}</TotalAmount>", amount(m)),
    }
}

fn rate(r: Rate) -> String {
    let bp = r.bp();
    format!("{}.{:02}", bp / 100, (bp % 100).abs())
}

fn quantity(milli: i64) -> String {
    let sign = if milli < 0 { "-" } else { "" };
    let (i, f) = (milli.unsigned_abs() / 1000, milli.unsigned_abs() % 1000);
    if f == 0 {
        format!("{sign}{i}.0")
    } else {
        format!("{sign}{i}.{f:03}")
    }
}

fn date(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), d.month() as u8, d.day())
}

/// ISO 3166-1 alfa-2 → alfa-3 (UE, EEE y principales socios).
fn alpha3(code: &str) -> Option<&'static str> {
    Some(match code {
        "ES" => "ESP",
        "PT" => "PRT",
        "FR" => "FRA",
        "DE" => "DEU",
        "IT" => "ITA",
        "NL" => "NLD",
        "BE" => "BEL",
        "LU" => "LUX",
        "IE" => "IRL",
        "AT" => "AUT",
        "DK" => "DNK",
        "SE" => "SWE",
        "FI" => "FIN",
        "PL" => "POL",
        "CZ" => "CZE",
        "SK" => "SVK",
        "SI" => "SVN",
        "HU" => "HUN",
        "RO" => "ROU",
        "BG" => "BGR",
        "GR" => "GRC",
        "HR" => "HRV",
        "CY" => "CYP",
        "MT" => "MLT",
        "EE" => "EST",
        "LV" => "LVA",
        "LT" => "LTU",
        // Irlanda del Norte («XI» en el NIF-IVA) es Reino Unido.
        "GB" | "XI" => "GBR",
        "US" => "USA",
        "CH" => "CHE",
        "NO" => "NOR",
        "IS" => "ISL",
        "LI" => "LIE",
        "AD" => "AND",
        "MC" => "MCO",
        "MX" => "MEX",
        "AR" => "ARG",
        "CO" => "COL",
        "CL" => "CHL",
        "BR" => "BRA",
        "CA" => "CAN",
        "MA" => "MAR",
        "CN" => "CHN",
        "JP" => "JPN",
        _ => return None,
    })
}

fn party(out: &mut String, tag: &str, who: &'static str, p: &Party) -> Result<(), FacturaeError> {
    let (person_type, residence, tax_id) = match &p.id {
        PartyId::Nif(n) => {
            let fisica = Nif::parse(n)
                .map(|n| n.is_persona_fisica())
                .unwrap_or(false);
            (if fisica { "F" } else { "J" }, "R", n.clone())
        }
        PartyId::Foreign { country, id, .. } => {
            // U = residente en la UE (no España); E = extranjero.
            let eu = alpha3(country).is_some_and(|_| {
                !matches!(
                    country.as_str(),
                    "GB" | "US" | "CH" | "NO" | "IS" | "LI" | "AD" | "MC"
                ) && country.len() == 2
                    && !matches!(
                        country.as_str(),
                        "MX" | "AR" | "CO" | "CL" | "BR" | "CA" | "MA" | "CN" | "JP"
                    )
            });
            let fisica = p.person.is_some();
            (
                if fisica { "F" } else { "J" },
                if eu { "U" } else { "E" },
                id.clone(),
            )
        }
    };
    let addr = p
        .address
        .as_ref()
        .ok_or(FacturaeError::MissingAddress(who))?;
    let country3 =
        alpha3(&addr.country).ok_or_else(|| FacturaeError::UnknownCountry(addr.country.clone()))?;
    let _ = write!(
        out,
        "<{tag}><TaxIdentification><PersonTypeCode>{person_type}</PersonTypeCode>\
         <ResidenceTypeCode>{residence}</ResidenceTypeCode>\
         <TaxIdentificationNumber>{}</TaxIdentificationNumber></TaxIdentification>",
        cut(&tax_id, 30)
    );
    let address_xml = address(addr, country3);
    if person_type == "F" {
        let n = p
            .person
            .as_ref()
            .ok_or(FacturaeError::MissingPersonName(who))?;
        let _ = write!(
            out,
            "<Individual><Name>{}</Name><FirstSurname>{}</FirstSurname>",
            cut(&n.name, 40),
            cut(&n.first_surname, 40)
        );
        if let Some(s2) = &n.second_surname {
            let _ = write!(out, "<SecondSurname>{}</SecondSurname>", cut(s2, 40));
        }
        let _ = write!(out, "{address_xml}</Individual>");
    } else {
        let _ = write!(
            out,
            "<LegalEntity><CorporateName>{}</CorporateName>{address_xml}</LegalEntity>",
            cut(&p.name, 80)
        );
    }
    let _ = write!(out, "</{tag}>");
    Ok(())
}

fn address(a: &Address, country3: &str) -> String {
    if a.country == "ES" {
        format!(
            "<AddressInSpain><Address>{}</Address><PostCode>{}</PostCode><Town>{}</Town>\
             <Province>{}</Province><CountryCode>ESP</CountryCode></AddressInSpain>",
            cut(&a.street, 80),
            cut(&a.post_code, 5),
            cut(&a.town, 50),
            cut(&a.province, 20)
        )
    } else {
        format!(
            "<OverseasAddress><Address>{}</Address><PostCodeAndTown>{}</PostCodeAndTown>\
             <Province>{}</Province><CountryCode>{country3}</CountryCode></OverseasAddress>",
            cut(&a.street, 80),
            cut(&format!("{} {}", a.post_code, a.town), 50),
            cut(&a.province, 20)
        )
    }
}

/// Base, cuota de IVA y cuota del recargo de un impuesto repercutido.
#[derive(Clone, Copy)]
struct TaxAmounts {
    base: Money,
    vat: Money,
    re: Money,
}

fn tax_output(
    out: &mut String,
    vat: VatTreatment,
    surcharge: Option<Rate>,
    a: TaxAmounts,
    eur: Option<TaxAmounts>,
) {
    let r = match vat {
        VatTreatment::Taxed { rate } => rate,
        _ => Rate::ZERO,
    };
    let _ = write!(
        out,
        "<Tax><TaxTypeCode>01</TaxTypeCode><TaxRate>{}</TaxRate>\
         <TaxableBase>{}</TaxableBase><TaxAmount>{}</TaxAmount>",
        rate(r),
        amount_type(a.base, eur.map(|e| e.base)),
        amount_type(a.vat, eur.map(|e| e.vat))
    );
    if let Some(s) = surcharge {
        let _ = write!(
            out,
            "<EquivalenceSurcharge>{}</EquivalenceSurcharge>\
             <EquivalenceSurchargeAmount>{}</EquivalenceSurchargeAmount>",
            rate(s),
            amount_type(a.re, eur.map(|e| e.re))
        );
    }
    out.push_str("</Tax>");
}

/// Impuestos de una línea sobre su base `gross`.
fn line_taxes(l: &InvoiceLine, gross: Money) -> TaxAmounts {
    TaxAmounts {
        base: gross,
        vat: match l.vat {
            VatTreatment::Taxed { rate } => gross.apply_rate(rate),
            _ => Money::ZERO,
        },
        re: l.surcharge.map_or(Money::ZERO, |r| gross.apply_rate(r)),
    }
}

/// Factura en moneda extranjera: tipo aplicado y líneas y totales en euros (los de
/// `taxpain_core::fx::convert`, en el mismo orden que los de la factura).
#[derive(Clone, Copy, Debug)]
pub struct FacturaeFx<'a> {
    /// ISO 4217.
    pub code: &'a str,
    /// Unidades de la moneda por 1 €.
    pub rate: ExchangeRate,
    /// Fecha de publicación del tipo.
    pub rate_date: Date,
    pub eur_lines: &'a [InvoiceLine],
    pub eur_totals: &'a Totals,
}

fn line_xml(out: &mut String, l: &InvoiceLine, eur: Option<&InvoiceLine>) {
    let total_cost = line_base(&InvoiceLine {
        discount: Rate::ZERO,
        ..l.clone()
    });
    let gross = line_base(l);
    let _ = write!(
        out,
        "<InvoiceLine><ItemDescription>{}</ItemDescription><Quantity>{}</Quantity>\
         <UnitOfMeasure>01</UnitOfMeasure><UnitPriceWithoutTax>{}</UnitPriceWithoutTax>\
         <TotalCost>{}</TotalCost>",
        cut(&l.description, 2500),
        quantity(l.quantity_milli),
        amount(l.unit_price),
        amount(total_cost)
    );
    if l.discount != Rate::ZERO {
        let _ = write!(
            out,
            "<DiscountsAndRebates><Discount><DiscountReason>Descuento</DiscountReason>\
             <DiscountRate>{}</DiscountRate><DiscountAmount>{}</DiscountAmount></Discount></DiscountsAndRebates>",
            rate(l.discount),
            amount(total_cost - gross)
        );
    }
    let _ = write!(out, "<GrossAmount>{}</GrossAmount>", amount(gross));
    let eur_gross = eur.map(line_base);
    if let Some(w) = l.withholding {
        let _ = write!(
            out,
            "<TaxesWithheld><Tax><TaxTypeCode>04</TaxTypeCode><TaxRate>{}</TaxRate>\
             <TaxableBase>{}</TaxableBase><TaxAmount>{}</TaxAmount></Tax></TaxesWithheld>",
            rate(w),
            amount_type(gross, eur_gross),
            amount_type(gross.apply_rate(w), eur_gross.map(|g| g.apply_rate(w)))
        );
    }
    out.push_str("<TaxesOutputs>");
    tax_output(
        out,
        l.vat,
        l.surcharge,
        line_taxes(l, gross),
        eur.zip(eur_gross).map(|(e, g)| line_taxes(e, g)),
    );
    out.push_str("</TaxesOutputs></InvoiceLine>");
}

/// Genera el XML Facturae 3.2.2 (UTF-8, sin firma).
pub fn to_facturae(
    invoice: &Invoice,
    totals: &Totals,
    reason: &CorrectionReason,
) -> Result<String, FacturaeError> {
    render(invoice, totals, reason, None)
}

/// [`to_facturae`] de una factura en moneda extranjera: `invoice` y `totals` en la moneda y
/// `fx` con el tipo y los importes en euros.
pub fn to_facturae_fx(
    invoice: &Invoice,
    totals: &Totals,
    reason: &CorrectionReason,
    fx: &FacturaeFx<'_>,
) -> Result<String, FacturaeError> {
    render(invoice, totals, reason, Some(fx))
}

fn render(
    invoice: &Invoice,
    totals: &Totals,
    reason: &CorrectionReason,
    fx: Option<&FacturaeFx<'_>>,
) -> Result<String, FacturaeError> {
    let recipient = invoice
        .recipient
        .as_ref()
        .ok_or(FacturaeError::MissingRecipient)?;
    if invoice.number.chars().count() > 20 {
        return Err(FacturaeError::NumberTooLong);
    }
    // InvoiceTotal de Facturae = base + impuestos repercutidos − retenciones.
    let invoice_total = totals.payable;
    let currency = fx.map_or("EUR", |f| f.code);
    let eur_totals = fx.map(|f| f.eur_totals);
    let mut x = String::with_capacity(4096);
    x.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = write!(
        x,
        "<fe:Facturae xmlns:fe=\"{NAMESPACE}\" xmlns:ds=\"http://www.w3.org/2000/09/xmldsig#\">\
         <FileHeader><SchemaVersion>3.2.2</SchemaVersion><Modality>I</Modality>\
         <InvoiceIssuerType>EM</InvoiceIssuerType><Batch>\
         <BatchIdentifier>{}</BatchIdentifier><InvoicesCount>1</InvoicesCount>\
         <TotalInvoicesAmount>{t}</TotalInvoicesAmount>\
         <TotalOutstandingAmount>{t}</TotalOutstandingAmount>\
         <TotalExecutableAmount>{t}</TotalExecutableAmount>\
         <InvoiceCurrencyCode>{}</InvoiceCurrencyCode></Batch></FileHeader><Parties>",
        cut(
            &format!("{}{}", issuer_id(&invoice.issuer), invoice.number),
            70
        ),
        esc(currency),
        t = amount_type(invoice_total, eur_totals.map(|e| e.payable))
    );
    party(&mut x, "SellerParty", "el emisor", &invoice.issuer)?;
    party(&mut x, "BuyerParty", "el destinatario", recipient)?;

    let (class, corrective) = match invoice.kind {
        InvoiceKind::Rectificativa(r) => ("OR", Some(r)),
        _ => ("OO", None),
    };
    let doc_type = if matches!(invoice.kind, InvoiceKind::Simplificada) {
        "FA"
    } else {
        "FC"
    };
    let _ = write!(
        x,
        "</Parties><Invoices><Invoice><InvoiceHeader><InvoiceNumber>{}</InvoiceNumber>\
         <InvoiceDocumentType>{doc_type}</InvoiceDocumentType><InvoiceClass>{class}</InvoiceClass>",
        cut(&invoice.number, 20)
    );
    if let Some(r) = corrective {
        let (num, d) = invoice
            .rectified
            .first()
            .cloned()
            .unwrap_or_else(|| (String::new(), invoice.issue_date));
        let month_start = d.replace_day(1).unwrap_or(d);
        let month_end = d.replace_day(d.month().length(d.year())).unwrap_or(d);
        let (method, method_desc) = match r.method {
            RectificationMethod::Sustitucion => ("01", "Rectificación íntegra"),
            RectificationMethod::Diferencias => ("02", "Rectificación por diferencias"),
        };
        let _ = write!(
            x,
            "<Corrective><InvoiceNumber>{}</InvoiceNumber><ReasonCode>{}</ReasonCode>\
             <ReasonDescription>{}</ReasonDescription><TaxPeriod><StartDate>{}</StartDate>\
             <EndDate>{}</EndDate></TaxPeriod><CorrectionMethod>{method}</CorrectionMethod>\
             <CorrectionMethodDescription>{method_desc}</CorrectionMethodDescription>\
             <InvoiceIssueDate>{}</InvoiceIssueDate></Corrective>",
            cut(&num, 20),
            reason.code,
            esc(reason.description),
            date(month_start),
            date(month_end),
            date(d)
        );
    }
    let _ = write!(
        x,
        "</InvoiceHeader><InvoiceIssueData><IssueDate>{}</IssueDate>",
        date(invoice.issue_date)
    );
    if let Some(od) = invoice.operation_date.filter(|d| *d != invoice.issue_date) {
        let _ = write!(x, "<OperationDate>{}</OperationDate>", date(od));
    }
    let _ = write!(
        x,
        "<InvoiceCurrencyCode>{}</InvoiceCurrencyCode>",
        esc(currency)
    );
    // Tipo de cambio (art. 79.Once LIVA) y fecha de su publicación.
    if let Some(f) = fx {
        let _ = write!(
            x,
            "<ExchangeRateDetails><ExchangeRate>{}</ExchangeRate>\
             <ExchangeRateDate>{}</ExchangeRateDate></ExchangeRateDetails>",
            f.rate.decimal(),
            date(f.rate_date)
        );
    }
    let _ = write!(
        x,
        "<TaxCurrencyCode>EUR</TaxCurrencyCode>\
         <LanguageName>es</LanguageName><InvoiceDescription>{}</InvoiceDescription></InvoiceIssueData><TaxesOutputs>",
        cut(&invoice.description, 2500)
    );
    for (i, b) in totals.breakdown.iter().enumerate() {
        let eur = eur_totals
            .and_then(|e| e.breakdown.get(i))
            .map(|e| TaxAmounts {
                base: e.base,
                vat: e.vat_amount,
                re: e.surcharge_amount,
            });
        tax_output(
            &mut x,
            b.vat,
            b.surcharge_rate,
            TaxAmounts {
                base: b.base,
                vat: b.vat_amount,
                re: b.surcharge_amount,
            },
            eur,
        );
    }
    x.push_str("</TaxesOutputs>");
    if !totals.withholdings.is_empty() {
        x.push_str("<TaxesWithheld>");
        for (i, (r, base, amt)) in totals.withholdings.iter().enumerate() {
            let eur = eur_totals.and_then(|e| e.withholdings.get(i));
            let _ = write!(
                x,
                "<Tax><TaxTypeCode>04</TaxTypeCode><TaxRate>{}</TaxRate>\
                 <TaxableBase>{}</TaxableBase><TaxAmount>{}</TaxAmount></Tax>",
                rate(*r),
                amount_type(*base, eur.map(|e| e.1)),
                amount_type(*amt, eur.map(|e| e.2))
            );
        }
        x.push_str("</TaxesWithheld>");
    }
    let _ = write!(
        x,
        "<InvoiceTotals><TotalGrossAmount>{b}</TotalGrossAmount>\
         <TotalGrossAmountBeforeTaxes>{b}</TotalGrossAmountBeforeTaxes>\
         <TotalTaxOutputs>{}</TotalTaxOutputs><TotalTaxesWithheld>{}</TotalTaxesWithheld>\
         <InvoiceTotal>{t}</InvoiceTotal><TotalOutstandingAmount>{t}</TotalOutstandingAmount>\
         <TotalExecutableAmount>{t}</TotalExecutableAmount></InvoiceTotals><Items>",
        amount(totals.tax_total),
        amount(totals.withholding),
        b = amount(totals.base),
        t = amount(invoice_total)
    );
    for (i, l) in invoice.lines.iter().enumerate() {
        line_xml(&mut x, l, fx.and_then(|f| f.eur_lines.get(i)));
    }
    x.push_str("</Items>");
    let literals: Vec<String> = totals
        .breakdown
        .iter()
        .filter_map(|b| match b.vat {
            VatTreatment::Exempt(e) => Some(e.legal_mention().to_owned()),
            VatTreatment::ReverseCharge => {
                Some("Inversión del sujeto pasivo (art. 84.Uno.2.º Ley 37/1992)".to_owned())
            }
            VatTreatment::NotSubject(_) => Some("Operación no sujeta a IVA".to_owned()),
            VatTreatment::Taxed { .. } => None,
        })
        .collect();
    if !literals.is_empty() {
        x.push_str("<LegalLiterals>");
        for l in literals {
            let _ = write!(x, "<LegalReference>{}</LegalReference>", cut(&l, 250));
        }
        x.push_str("</LegalLiterals>");
    }
    x.push_str("</Invoice></Invoices></fe:Facturae>");
    Ok(x)
}

fn issuer_id(p: &Party) -> String {
    match &p.id {
        PartyId::Nif(n) => n.clone(),
        PartyId::Foreign { id, .. } => id.clone(),
    }
}
