//! Factura electrónica UBL 2.1 conforme a la EN 16931 y al Anexo I de la Orden HAC/1028/2026
//! (solución pública de facturación electrónica, factura B2B del RD 238/2026).
//!
//! Fuente: `fixtures/aeat/ubl/anexo-1-campos.md` (transcripción del anexo) y esquemas OASIS UBL
//! 2.1 (`fixtures/oasis/ubl-2.1/`, con los que se valida en los tests).
//!
//! `TODO(VERIFY)` (el anexo no lo concreta o no cabe en UBL 2.1):
//! - BG-34 «RETENCIONES» (`cac:CollectionInvoiceLine`, `RETE`) no existe en UBL 2.1: la
//!   retención va sólo en `cac:WithholdingTaxTotal` (BT-ES-17), que sí existe.
//! - Signo de la retención en el total a pagar (BT-115): aquí, total − retenciones.
//! - Ubicación de BT-160/BT-161 en `cac:AdditionalItemProperty`: `cbc:Name` = `REGI`,
//!   `cbc:Value` = clave L3A y `cbc:ValueQualifier` = `IVA`.
//! - Entregas intracomunitarias: L4 no tiene `K`; se usa `E` con `VATEX-EU-IC`.
//! - Recargo de equivalencia: desglose propio con categoría `N` (L4) y el tipo del recargo.
//! - Exenciones: código VATEX-EU según el motivo (el anexo no lista los VATEX-ES).
//! - Moneda extranjera: BT-5 = la moneda, BT-6 = `EUR`, BT-111 = cuota total en euros (un
//!   segundo `cac:TaxTotal` sólo con `cbc:TaxAmount` en EUR, regla BR-53) y BT-167 (tipo de
//!   cambio del anexo, que no existe en la EN 16931-1:2017) en `cac:TaxExchangeRate` con
//!   `MathematicOperatorCode` = `Divide` (euros = importe ÷ tipo del BCE).

use std::fmt::Write;

use taxpain_core::fx::ExchangeRate;
use taxpain_core::invoice::{
    Address, Exemption, IdType, Invoice, InvoiceKind, NotSubject, Party, PartyId,
    RectificationMethod, Totals, VatTreatment,
};
use taxpain_core::money::{Money, Rate};
use taxpain_core::nif::Nif;

pub const PROFILE_ID: &str = "urn:aeat:names:specification:ubl:schema:profile:B2B";
pub const CUSTOMIZATION_ID: &str =
    "urn:cen.eu:en16931:2026#conformant#urn:aeat.es:spfe:1.0.0:extended";

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum UblError {
    #[error("la factura UBL necesita un destinatario identificado")]
    MissingRecipient,
    #[error("falta la dirección del {0} (obligatoria en la factura electrónica)")]
    MissingAddress(&'static str),
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Importe con dos decimales y punto.
fn amount(m: Money) -> String {
    let c = m.cents();
    let sign = if c < 0 { "-" } else { "" };
    format!(
        "{sign}{}.{:02}",
        c.unsigned_abs() / 100,
        c.unsigned_abs() % 100
    )
}

fn pct(r: Rate) -> String {
    format!("{}.{:02}", r.bp() / 100, (r.bp() % 100).abs())
}

fn quantity(milli: i64) -> String {
    let sign = if milli < 0 { "-" } else { "" };
    format!(
        "{sign}{}.{:03}",
        milli.unsigned_abs() / 1000,
        milli.unsigned_abs() % 1000
    )
}

fn date(d: time::Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
}

/// Categoría (L4), tipo y, si procede, motivo de exención (VATEX) de un tratamiento de IVA.
fn category(v: VatTreatment) -> (&'static str, Rate, Option<(&'static str, &'static str)>) {
    match v {
        VatTreatment::Taxed { rate } => ("S", rate, None),
        VatTreatment::ReverseCharge => (
            "AE",
            Rate::ZERO,
            Some(("VATEX-EU-AE", "Inversión del sujeto pasivo")),
        ),
        VatTreatment::Exempt(Exemption::E5) => (
            "E",
            Rate::ZERO,
            Some((
                "VATEX-EU-IC",
                "Entrega intracomunitaria exenta (art. 25 Ley 37/1992)",
            )),
        ),
        VatTreatment::Exempt(Exemption::E2 | Exemption::E3 | Exemption::E4) => (
            "G",
            Rate::ZERO,
            Some((
                "VATEX-EU-G",
                "Exportación exenta (arts. 21 a 24 Ley 37/1992)",
            )),
        ),
        VatTreatment::Exempt(Exemption::E1 | Exemption::E6) => (
            "E",
            Rate::ZERO,
            Some(("VATEX-EU-132", "Operación exenta (art. 20 Ley 37/1992)")),
        ),
        VatTreatment::NotSubject(NotSubject::N1 | NotSubject::N2) => (
            "O",
            Rate::ZERO,
            Some(("VATEX-EU-O", "Operación no sujeta a IVA")),
        ),
    }
}

/// Clave de régimen del IVA (L3A, BT-161) de una línea.
fn regime_key(v: VatTreatment) -> &'static str {
    match v {
        VatTreatment::Exempt(Exemption::E2 | Exemption::E3 | Exemption::E4) => "02",
        _ => "01",
    }
}

fn address_xml(x: &mut String, a: &Address) {
    let _ = write!(
        x,
        "<cac:PostalAddress><cbc:StreetName>{}</cbc:StreetName><cbc:CityName>{}</cbc:CityName>\
         <cbc:PostalZone>{}</cbc:PostalZone><cbc:CountrySubentity>{}</cbc:CountrySubentity>\
         <cac:Country><cbc:IdentificationCode>{}</cbc:IdentificationCode></cac:Country>\
         </cac:PostalAddress>",
        esc(&a.street),
        esc(&a.town),
        esc(&a.post_code),
        esc(&a.province),
        esc(&a.country.to_uppercase())
    );
}

/// `PartyTaxScheme` y `PartyLegalEntity` según la identificación de la parte.
fn party_xml(x: &mut String, p: &Party, who: &'static str) -> Result<(), UblError> {
    let a = p.address.as_ref().ok_or(UblError::MissingAddress(who))?;
    let _ = write!(
        x,
        "<cac:Party><cac:PartyName><cbc:Name>{}</cbc:Name></cac:PartyName>",
        esc(&p.name)
    );
    address_xml(x, a);
    match &p.id {
        // NIF español: `CompanyID@schemeID="FC"` con esquema `LOC` (BT-31/BT-47) y la
        // inscripción legal con el NIF (BT-30/BT-47).
        PartyId::Nif(nif) => {
            let nif =
                Nif::parse(nif).map_or_else(|_| nif.to_uppercase(), |n| n.as_str().to_owned());
            let _ = write!(
                x,
                "<cac:PartyTaxScheme><cbc:CompanyID schemeID=\"FC\">{nif}</cbc:CompanyID>\
                 <cac:TaxScheme><cbc:ID>LOC</cbc:ID></cac:TaxScheme></cac:PartyTaxScheme>\
                 <cac:PartyLegalEntity><cbc:RegistrationName>{}</cbc:RegistrationName>\
                 <cbc:CompanyID>{nif}</cbc:CompanyID></cac:PartyLegalEntity>",
                esc(&p.name)
            );
        }
        // NIF-IVA de otro Estado miembro: esquema `VAT` (BT-48), con el prefijo del país.
        PartyId::Foreign {
            country,
            id_type: IdType::NifIva,
            id,
        } => {
            // Grecia usa «EL» como prefijo del NIF-IVA.
            let c = match country.to_uppercase().as_str() {
                "GR" => "EL".to_owned(),
                c => c.to_owned(),
            };
            let vat = if id.to_uppercase().starts_with(&c) {
                id.to_uppercase()
            } else {
                format!("{c}{}", id.to_uppercase())
            };
            let _ = write!(
                x,
                "<cac:PartyTaxScheme><cbc:CompanyID>{}</cbc:CompanyID>\
                 <cac:TaxScheme><cbc:ID>VAT</cbc:ID></cac:TaxScheme></cac:PartyTaxScheme>\
                 <cac:PartyLegalEntity><cbc:RegistrationName>{}</cbc:RegistrationName>\
                 </cac:PartyLegalEntity>",
                esc(&vat),
                esc(&p.name)
            );
        }
        // Otros documentos extranjeros: sólo la entidad legal con su identificador.
        PartyId::Foreign { id, .. } => {
            let _ = write!(
                x,
                "<cac:PartyLegalEntity><cbc:RegistrationName>{}</cbc:RegistrationName>\
                 <cbc:CompanyID>{}</cbc:CompanyID></cac:PartyLegalEntity>",
                esc(&p.name),
                esc(id)
            );
        }
    }
    x.push_str("</cac:Party>");
    Ok(())
}

fn tax_category_xml(
    x: &mut String,
    element: &str,
    cat: &str,
    rate: Rate,
    reason: Option<(&str, &str)>,
    scheme: &str,
) {
    let _ = write!(
        x,
        "<cac:{element}><cbc:ID>{cat}</cbc:ID><cbc:Percent>{}</cbc:Percent>",
        pct(rate)
    );
    if let Some((code, text)) = reason {
        let _ = write!(
            x,
            "<cbc:TaxExemptionReasonCode>{code}</cbc:TaxExemptionReasonCode>\
             <cbc:TaxExemptionReason>{}</cbc:TaxExemptionReason>",
            esc(text)
        );
    }
    let _ = write!(
        x,
        "<cac:TaxScheme><cbc:ID>{scheme}</cbc:ID></cac:TaxScheme></cac:{element}>"
    );
}

/// Datos opcionales para la factura UBL.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UblOptions {
    /// URL del QR tributario (VERI*FACTU), si la factura lo lleva.
    pub verifactu_qr: Option<String>,
    /// Clave de retención (L5) para las retenciones de IRPF: `01` actividades profesionales
    /// (por defecto), `04` actividades empresariales, `06` arrendamiento de locales.
    pub withholding_key: Option<String>,
    /// Factura en moneda extranjera (importes de `invoice` y `totals` en esa moneda).
    pub currency: Option<UblCurrency>,
}

/// Moneda de la factura si no es el euro, con el tipo y la cuota total en euros.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UblCurrency {
    /// ISO 4217.
    pub code: String,
    /// Unidades de la moneda por 1 €.
    pub rate: ExchangeRate,
    pub rate_date: time::Date,
    /// IVA + recargo en euros (BT-111).
    pub eur_tax_total: Money,
}

/// Factura UBL 2.1 (`Invoice`) de una factura emitida.
pub fn to_ubl(invoice: &Invoice, totals: &Totals, opts: &UblOptions) -> Result<String, UblError> {
    let recipient = invoice
        .recipient
        .as_ref()
        .ok_or(UblError::MissingRecipient)?;
    let rectificativa = match invoice.kind {
        InvoiceKind::Rectificativa(r) => Some(r),
        _ => None,
    };
    let fx = opts.currency.as_ref();
    let cur = esc(fx.map_or("EUR", |f| f.code.as_str()));
    let mut x = String::with_capacity(8192);
    x.push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = write!(
        x,
        "<Invoice xmlns=\"urn:oasis:names:specification:ubl:schema:xsd:Invoice-2\" \
         xmlns:cac=\"urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2\" \
         xmlns:cbc=\"urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2\">\
         <cbc:CustomizationID>{CUSTOMIZATION_ID}</cbc:CustomizationID>\
         <cbc:ProfileID>{PROFILE_ID}</cbc:ProfileID>\
         <cbc:ID>{}</cbc:ID><cbc:IssueDate>{}</cbc:IssueDate>\
         <cbc:InvoiceTypeCode>{}</cbc:InvoiceTypeCode>",
        esc(&invoice.number),
        date(invoice.issue_date),
        if rectificativa.is_some() {
            "384"
        } else {
            "380"
        }
    );
    // Menciones obligatorias (BG-1, BT-21 = REG).
    let mut notes: Vec<String> = Vec::new();
    for t in &totals.breakdown {
        if let (_, _, Some((_, text))) = category(t.vat)
            && !notes.iter().any(|n| n == text)
        {
            notes.push(text.to_owned());
        }
    }
    for n in &notes {
        let _ = write!(x, "<cbc:Note>#REG#{}</cbc:Note>", esc(n));
    }
    if let Some(op) = invoice.operation_date.filter(|d| *d != invoice.issue_date) {
        let _ = write!(x, "<cbc:TaxPointDate>{}</cbc:TaxPointDate>", date(op));
    }
    let _ = write!(
        x,
        "<cbc:DocumentCurrencyCode>{cur}</cbc:DocumentCurrencyCode>"
    );
    if fx.is_some() {
        x.push_str("<cbc:TaxCurrencyCode>EUR</cbc:TaxCurrencyCode>");
    }
    // Rectificativa: factura rectificada (BG-3) y tipo/modalidad (BT-ES-27/28).
    if let Some(r) = rectificativa {
        for (num, d) in &invoice.rectified {
            let _ = write!(
                x,
                "<cac:BillingReference><cac:InvoiceDocumentReference><cbc:ID>{}</cbc:ID>\
                 <cbc:IssueDate>{}</cbc:IssueDate></cac:InvoiceDocumentReference></cac:BillingReference>",
                esc(num),
                date(*d)
            );
        }
        let _ = write!(
            x,
            "<cac:AdditionalDocumentReference><cbc:ID>RECT:TIPO</cbc:ID>\
             <cbc:DocumentTypeCode>R{}</cbc:DocumentTypeCode></cac:AdditionalDocumentReference>\
             <cac:AdditionalDocumentReference><cbc:ID>RECT:MODALIDAD</cbc:ID>\
             <cbc:DocumentTypeCode>{}</cbc:DocumentTypeCode></cac:AdditionalDocumentReference>",
            r.code.clamp(1, 4),
            match r.method {
                RectificationMethod::Diferencias => "I",
                RectificationMethod::Sustitucion => "S",
            }
        );
    }
    if let Some(qr) = &opts.verifactu_qr {
        let _ = write!(
            x,
            "<cac:AdditionalDocumentReference><cbc:ID>QR</cbc:ID><cbc:UUID>{}</cbc:UUID>\
             <cbc:DocumentDescription>VERIFACTU</cbc:DocumentDescription>\
             </cac:AdditionalDocumentReference>",
            esc(qr)
        );
    }
    x.push_str("<cac:AccountingSupplierParty>");
    party_xml(&mut x, &invoice.issuer, "emisor")?;
    x.push_str("</cac:AccountingSupplierParty><cac:AccountingCustomerParty>");
    party_xml(&mut x, recipient, "destinatario")?;
    x.push_str("</cac:AccountingCustomerParty>");

    // Tipo de cambio (BT-167): euros = importe en la moneda ÷ tipo.
    if let Some(f) = fx {
        let _ = write!(
            x,
            "<cac:TaxExchangeRate><cbc:SourceCurrencyCode>{cur}</cbc:SourceCurrencyCode>\
             <cbc:TargetCurrencyCode>EUR</cbc:TargetCurrencyCode>\
             <cbc:CalculationRate>{}</cbc:CalculationRate>\
             <cbc:MathematicOperatorCode>Divide</cbc:MathematicOperatorCode>\
             <cbc:Date>{}</cbc:Date></cac:TaxExchangeRate>",
            f.rate.decimal(),
            date(f.rate_date)
        );
    }
    // IVA (BG-23): un desglose por tratamiento y tipo; el recargo, aparte.
    let _ = write!(
        x,
        "<cac:TaxTotal><cbc:TaxAmount currencyID=\"{cur}\">{}</cbc:TaxAmount>",
        amount(totals.tax_total)
    );
    for t in &totals.breakdown {
        let (cat, rate, reason) = category(t.vat);
        let _ = write!(
            x,
            "<cac:TaxSubtotal><cbc:TaxableAmount currencyID=\"{cur}\">{}</cbc:TaxableAmount>\
             <cbc:TaxAmount currencyID=\"{cur}\">{}</cbc:TaxAmount>",
            amount(t.base),
            amount(t.vat_amount)
        );
        tax_category_xml(&mut x, "TaxCategory", cat, rate, reason, "VAT");
        x.push_str("</cac:TaxSubtotal>");
        if let Some(sr) = t.surcharge_rate {
            let _ = write!(
                x,
                "<cac:TaxSubtotal><cbc:TaxableAmount currencyID=\"{cur}\">{}</cbc:TaxableAmount>\
                 <cbc:TaxAmount currencyID=\"{cur}\">{}</cbc:TaxAmount>",
                amount(t.base),
                amount(t.surcharge_amount)
            );
            tax_category_xml(&mut x, "TaxCategory", "N", sr, None, "VAT");
            x.push_str("</cac:TaxSubtotal>");
        }
    }
    x.push_str("</cac:TaxTotal>");
    // Cuota total en euros (BT-111), sin desglose.
    if let Some(f) = fx {
        let _ = write!(
            x,
            "<cac:TaxTotal><cbc:TaxAmount currencyID=\"EUR\">{}</cbc:TaxAmount></cac:TaxTotal>",
            amount(f.eur_tax_total)
        );
    }

    // Retenciones de IRPF (BT-ES-17): total y una subtotal por tipo.
    if !totals.withholding.is_zero() {
        let key = opts.withholding_key.as_deref().unwrap_or("01");
        let _ = write!(
            x,
            "<cac:WithholdingTaxTotal><cbc:TaxAmount currencyID=\"{cur}\">{}</cbc:TaxAmount>",
            amount(totals.withholding)
        );
        for (rate, base, wh) in &totals.withholdings {
            let _ = write!(
                x,
                "<cac:TaxSubtotal><cbc:TaxableAmount currencyID=\"{cur}\">{}</cbc:TaxableAmount>\
                 <cbc:TaxAmount currencyID=\"{cur}\">{}</cbc:TaxAmount>\
                 <cac:TaxCategory><cbc:ID>S</cbc:ID><cbc:Percent>{}</cbc:Percent>\
                 <cac:TaxScheme><cbc:ID>WTH</cbc:ID><cbc:TaxTypeCode>{key}</cbc:TaxTypeCode>\
                 </cac:TaxScheme></cac:TaxCategory></cac:TaxSubtotal>",
                amount(*base),
                amount(*wh),
                pct(*rate)
            );
        }
        x.push_str("</cac:WithholdingTaxTotal>");
    }

    let _ = write!(
        x,
        "<cac:LegalMonetaryTotal>\
         <cbc:LineExtensionAmount currencyID=\"{cur}\">{}</cbc:LineExtensionAmount>\
         <cbc:TaxExclusiveAmount currencyID=\"{cur}\">{}</cbc:TaxExclusiveAmount>\
         <cbc:TaxInclusiveAmount currencyID=\"{cur}\">{}</cbc:TaxInclusiveAmount>\
         <cbc:PayableAmount currencyID=\"{cur}\">{}</cbc:PayableAmount></cac:LegalMonetaryTotal>",
        amount(totals.base),
        amount(totals.base),
        amount(totals.invoice_total),
        amount(totals.payable)
    );

    for (i, l) in invoice.lines.iter().enumerate() {
        // Bruto = base sin descuento (mismo redondeo que `line_base`).
        let gross = taxpain_core::invoice::totals::line_base(&taxpain_core::invoice::InvoiceLine {
            discount: Rate::ZERO,
            ..l.clone()
        });
        let net = taxpain_core::invoice::totals::line_base(l);
        let (cat, rate, _) = category(l.vat);
        let _ = write!(
            x,
            "<cac:InvoiceLine><cbc:ID>{}</cbc:ID>\
             <cbc:InvoicedQuantity unitCode=\"C62\">{}</cbc:InvoicedQuantity>\
             <cbc:LineExtensionAmount currencyID=\"{cur}\">{}</cbc:LineExtensionAmount>",
            i + 1,
            quantity(l.quantity_milli),
            amount(net)
        );
        if l.discount.bp() != 0 {
            let _ = write!(
                x,
                "<cac:AllowanceCharge><cbc:ChargeIndicator>false</cbc:ChargeIndicator>\
                 <cbc:AllowanceChargeReason>Descuento</cbc:AllowanceChargeReason>\
                 <cbc:MultiplierFactorNumeric>{}</cbc:MultiplierFactorNumeric>\
                 <cbc:Amount currencyID=\"{cur}\">{}</cbc:Amount>\
                 <cbc:BaseAmount currencyID=\"{cur}\">{}</cbc:BaseAmount></cac:AllowanceCharge>",
                pct(l.discount),
                amount(gross - net),
                amount(gross)
            );
        }
        let _ = write!(
            x,
            "<cac:Item><cbc:Name>{}</cbc:Name>",
            esc(&l.description.chars().take(250).collect::<String>())
        );
        tax_category_xml(&mut x, "ClassifiedTaxCategory", cat, rate, None, "VAT");
        // Clave de régimen del IVA en cada línea (BG-32, BT-160/161, BT-ES-24).
        let _ = write!(
            x,
            "<cac:AdditionalItemProperty><cbc:Name>REGI</cbc:Name><cbc:Value>{}</cbc:Value>\
             <cbc:ValueQualifier>IVA</cbc:ValueQualifier></cac:AdditionalItemProperty></cac:Item>\
             <cac:Price><cbc:PriceAmount currencyID=\"{cur}\">{}</cbc:PriceAmount></cac:Price>\
             </cac:InvoiceLine>",
            regime_key(l.vat),
            amount(l.unit_price)
        );
    }
    x.push_str("</Invoice>");
    Ok(x)
}
