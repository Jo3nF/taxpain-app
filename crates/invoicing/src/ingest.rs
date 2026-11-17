//! Extracción de datos de facturas **recibidas**, siempre para revisión humana.
//!
//! - Facturae (XML): lectura exacta de emisor, número, fecha, bases, cuotas y totales.
//! - Texto (de un PDF con capa de texto): heurísticas con confianza por campo.
//!
//! - Asistente (Fase 7, a petición del usuario): `taxpain_app::received::parse_ai_extraction`
//!   produce esta misma estructura desde la respuesta del modelo (texto anonimizado).
//!
//! El OCR de documentos escaneados (Vision de macOS) se enchufará igual.

use quick_xml::Reader;
use quick_xml::events::Event;
use serde::{Deserialize, Serialize};
use taxpain_core::Nif;
use taxpain_core::fx::ExchangeRate;
use taxpain_core::money::{Money, Rate};
use time::{Date, Month};

/// Grado de confianza de un campo extraído.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Confidence {
    Baja,
    Media,
    Alta,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Field<T> {
    pub value: T,
    pub confidence: Confidence,
}

fn f<T>(value: T, confidence: Confidence) -> Option<Field<T>> {
    Some(Field { value, confidence })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxLine {
    pub rate: Rate,
    pub base: Money,
    pub amount: Money,
}

/// Datos extraídos de una factura recibida (todo opcional: se completa en la revisión).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Extraction {
    pub source: String,
    pub supplier_nif: Option<Field<String>>,
    pub supplier_name: Option<Field<String>>,
    pub recipient_nif: Option<Field<String>>,
    pub number: Option<Field<String>>,
    pub issue_date: Option<Field<Date>>,
    pub base: Option<Field<Money>>,
    pub vat: Option<Field<Money>>,
    pub withholding: Option<Field<Money>>,
    pub total: Option<Field<Money>>,
    pub taxes: Vec<TaxLine>,
    /// Avisos para el revisor (p. ej. "los importes no cuadran").
    pub warnings: Vec<String>,
    /// Factura en otra moneda (los importes de arriba son en ella): moneda, tipo y contravalor
    /// en euros tal como figuran en la factura, para preseleccionarlos en la revisión.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fx: Option<ExtractedFx>,
}

/// Moneda extranjera de una factura recibida, leída de la factura.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractedFx {
    /// Código ISO 4217 de la moneda («USD»).
    pub code: String,
    /// Tipo de la factura como lo publica el BCE (unidades de la moneda por 1 €; «1.1193»).
    pub rate: Option<String>,
    /// Fecha del tipo.
    pub rate_date: Option<Date>,
    /// IVA en euros que figura en la factura (art. 12 RD 1619/2012): la suma de los contravalores
    /// de las cuotas.
    pub vat_eur: Option<Money>,
}

impl Extraction {
    /// Comprueba que base + IVA − retención = total y lo anota.
    fn check_consistency(&mut self) {
        if let (Some(b), Some(t)) = (&self.base, &self.total) {
            let vat = self.vat.as_ref().map_or(Money::ZERO, |v| v.value);
            let wh = self.withholding.as_ref().map_or(Money::ZERO, |v| v.value);
            let gross = b.value + vat;
            if t.value != gross && t.value != gross - wh {
                self.warnings.push(format!(
                    "los importes no cuadran: base {} + IVA {} ≠ total {}",
                    b.value, vat, t.value
                ));
            }
        }
    }
}

// ---------------- Facturae ----------------

/// Lee un Facturae 3.2.x. Devuelve `None` si no lo parece.
pub fn from_facturae(xml: &str) -> Option<Extraction> {
    if !xml.contains("facturae.gob.es/formato/Versiones/Facturae") {
        return None;
    }
    let mut r = Reader::from_str(xml);
    let mut path: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut e = Extraction {
        source: "facturae".into(),
        ..Extraction::default()
    };
    let mut tax: (Option<Rate>, Option<Money>, Option<Money>) = (None, None, None);
    let mut withheld = Money::ZERO;
    let mut withheld_seen = false;
    // Moneda y tipo de cambio si la factura no es en euros.
    let mut currency: Option<String> = None;
    let mut exchange_rate: Option<String> = None;
    let mut rate_date: Option<Date> = None;
    // Contravalor en euros de las cuotas y el del total (importe en la moneda y en euros).
    let mut vat_eur: Option<Money> = None;
    let mut total_pair: (Option<Money>, Option<Money>) = (None, None);
    loop {
        match r.read_event() {
            Ok(Event::Start(s)) => {
                path.push(s.local_name().as_ref().to_owned());
                text.clear();
            }
            Ok(Event::Text(t)) => text.push_str(&t.xml10_content()),
            Ok(Event::GeneralRef(g)) => text.push_str(match g.as_ref() {
                "amp" => "&",
                "lt" => "<",
                "gt" => ">",
                "quot" => "\"",
                "apos" => "'",
                _ => "",
            }),
            Ok(Event::End(end)) => {
                let name = end.local_name().as_ref().to_owned();
                let v = text.trim().to_owned();
                text.clear();
                let in_ = |p: &str| path.iter().any(|x| x == p);
                let seller = in_("SellerParty");
                let buyer = in_("BuyerParty");
                let invoice_level = in_("Invoice") && !in_("Items");
                match name.as_str() {
                    "TaxIdentificationNumber" if seller => e.supplier_nif = f(v, Confidence::Alta),
                    "TaxIdentificationNumber" if buyer => e.recipient_nif = f(v, Confidence::Alta),
                    "CorporateName" if seller => e.supplier_name = f(v, Confidence::Alta),
                    "Name" if seller && e.supplier_name.is_none() => {
                        e.supplier_name = f(v, Confidence::Alta)
                    }
                    "FirstSurname" | "SecondSurname" if seller => {
                        if let Some(n) = e.supplier_name.as_mut() {
                            n.value = format!("{} {v}", n.value);
                        }
                    }
                    "InvoiceNumber" if in_("InvoiceHeader") && !in_("Corrective") => {
                        let series = e.number.take().map(|n| n.value).unwrap_or_default();
                        e.number = f(format!("{series}{v}"), Confidence::Alta);
                    }
                    "InvoiceSeriesCode" if in_("InvoiceHeader") && !in_("Corrective") => {
                        let num = e.number.take().map(|n| n.value).unwrap_or_default();
                        e.number = f(format!("{v}{num}"), Confidence::Alta);
                    }
                    "InvoiceCurrencyCode" if in_("InvoiceIssueData") => currency = Some(v),
                    "ExchangeRate" if in_("InvoiceIssueData") => exchange_rate = Some(v),
                    "ExchangeRateDate" if in_("InvoiceIssueData") => rate_date = parse_iso_date(&v),
                    "EquivalentInEuros"
                        if invoice_level
                            && in_("TaxesOutputs")
                            && (in_("TaxAmount") || in_("EquivalenceSurchargeAmount")) =>
                    {
                        if let Some(a) = parse_amount(&v) {
                            vat_eur = Some(vat_eur.unwrap_or(Money::ZERO) + a);
                        }
                    }
                    "TotalAmount" if in_("Batch") && in_("TotalInvoicesAmount") => {
                        total_pair.0 = parse_amount(&v);
                    }
                    "EquivalentInEuros" if in_("Batch") && in_("TotalInvoicesAmount") => {
                        total_pair.1 = parse_amount(&v);
                    }
                    "IssueDate" if in_("InvoiceIssueData") => {
                        e.issue_date = parse_iso_date(&v).and_then(|d| f(d, Confidence::Alta));
                    }
                    "TaxRate" if invoice_level && in_("TaxesOutputs") => tax.0 = parse_rate(&v),
                    "TotalAmount" if invoice_level && in_("TaxesOutputs") && in_("TaxableBase") => {
                        tax.1 = parse_amount(&v);
                    }
                    "TotalAmount" if invoice_level && in_("TaxesOutputs") && in_("TaxAmount") => {
                        tax.2 = parse_amount(&v);
                    }
                    "Tax" if invoice_level && in_("TaxesOutputs") => {
                        if let (Some(rate), Some(base), amount) = tax {
                            e.taxes.push(TaxLine {
                                rate,
                                base,
                                amount: amount.unwrap_or(Money::ZERO),
                            });
                        }
                        tax = (None, None, None);
                    }
                    "TotalTaxesWithheld" if invoice_level => {
                        if let Some(a) = parse_amount(&v) {
                            withheld = a;
                            withheld_seen = true;
                        }
                    }
                    "TotalGrossAmountBeforeTaxes" if invoice_level => {
                        e.base = parse_amount(&v).and_then(|a| f(a, Confidence::Alta))
                    }
                    "TotalTaxOutputs" if invoice_level => {
                        e.vat = parse_amount(&v).and_then(|a| f(a, Confidence::Alta))
                    }
                    "InvoiceTotal" if invoice_level => {
                        e.total = parse_amount(&v).and_then(|a| f(a, Confidence::Alta))
                    }
                    _ => {}
                }
                path.pop();
            }
            Ok(Event::Eof) => break,
            Err(_) => return None,
            _ => {}
        }
    }
    if withheld_seen && !withheld.is_zero() {
        e.withholding = f(withheld, Confidence::Alta);
    }
    e.check_consistency();
    if let Some(c) = currency.filter(|c| !c.eq_ignore_ascii_case("EUR")) {
        let (rate, inverted) = exchange_rate
            .as_deref()
            .and_then(ExchangeRate::parse)
            .map_or((None, false), |r| {
                let (r, inverted) = orient_rate(r, total_pair);
                (Some(r), inverted)
            });
        e.warnings.push(format!(
            "la factura está en {c}{}: los importes leídos son en esa moneda; comprueba la \
             moneda y el tipo de cambio{} al revisarla",
            exchange_rate
                .as_deref()
                .map(|r| format!(" (tipo de la factura: {r})"))
                .unwrap_or_default(),
            if inverted {
                " (la factura lo da en euros por unidad de la moneda: se ha invertido)"
            } else {
                ""
            }
        ));
        e.fx = Some(ExtractedFx {
            code: c.trim().to_uppercase(),
            rate: rate.map(ExchangeRate::decimal),
            rate_date,
            vat_eur,
        });
    }
    Some(e)
}

/// El tipo de `ExchangeRate` en el sentido del BCE (unidades de la moneda por 1 €): el esquema
/// no fija el sentido; si el contravalor en euros del total (`total` = importe en la moneda y en
/// euros) cuadra mejor con el inverso, se invierte (`true`).
fn orient_rate(r: ExchangeRate, total: (Option<Money>, Option<Money>)) -> (ExchangeRate, bool) {
    let (Some(amount), Some(eur)) = total else {
        return (r, false);
    };
    let m = r.micros();
    let inverse = (1_000_000_000_000_i64 + m / 2)
        .checked_div(m)
        .and_then(ExchangeRate::from_micros);
    let err = |x: ExchangeRate| (x.to_eur(amount) - eur).abs();
    match inverse {
        Some(i) if err(i) < err(r) => (i, true),
        _ => (r, false),
    }
}

fn parse_iso_date(s: &str) -> Option<Date> {
    let mut it = s.split('-');
    let y: i32 = it.next()?.parse().ok()?;
    let m: u8 = it.next()?.parse().ok()?;
    let d: u8 = it.next()?.parse().ok()?;
    Date::from_calendar_date(y, Month::try_from(m).ok()?, d).ok()
}

/// "21.00", "21", "5.2" → Rate.
fn parse_rate(s: &str) -> Option<Rate> {
    let m = Money::parse(s)?;
    i32::try_from(m.cents()).ok().map(Rate::from_bp)
}

/// Importe con punto decimal y hasta 8 decimales (Facturae), redondeado al céntimo.
fn parse_amount(s: &str) -> Option<Money> {
    let s = s.trim();
    let (neg, s) = s.strip_prefix('-').map_or((false, s), |r| (true, r));
    let (int, frac) = s.split_once('.').unwrap_or((s, ""));
    let int: i64 = int.parse().ok()?;
    let frac = format!("{frac:0<3}");
    let cents: i64 = frac[..2].parse().ok()?;
    let third: i64 = frac[2..3].parse().ok()?;
    let mut total = int * 100 + cents + i64::from(third >= 5);
    if neg {
        total = -total;
    }
    Some(Money::from_cents(total))
}

// ---------------- Texto (PDF) ----------------

/// Busca NIF/CIF/NIE válidos en el texto, en orden de aparición y sin repetir.
fn find_nifs(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let upper = text.to_uppercase();
    let tokens = upper.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '.'));
    for tok in tokens {
        let t = tok.trim_matches(|c: char| c == '.' || c == '-');
        let t = t.strip_prefix("ES").filter(|r| r.len() == 9).unwrap_or(t);
        if (9..=12).contains(&t.len())
            && let Ok(n) = Nif::parse(t)
        {
            let s = n.as_str().to_owned();
            if !out.contains(&s) {
                out.push(s);
            }
        }
    }
    out
}

/// Fechas dd/mm/aaaa, dd-mm-aaaa o dd.mm.aaaa.
fn find_dates(text: &str) -> Vec<(usize, Date)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 10 <= b.len() {
        let w = &text[i..];
        let ok = |k: usize| w.as_bytes().get(k).is_some_and(u8::is_ascii_digit);
        let sep = w.as_bytes().get(2).copied();
        if ok(0)
            && ok(1)
            && matches!(sep, Some(b'/' | b'-' | b'.'))
            && w.as_bytes().get(5).copied() == sep
            && ok(3)
            && ok(4)
            && (6..10).all(ok)
        {
            let d: u8 = w[0..2].parse().unwrap_or(0);
            let m: u8 = w[3..5].parse().unwrap_or(0);
            let y: i32 = w[6..10].parse().unwrap_or(0);
            if let Some(date) = Month::try_from(m)
                .ok()
                .and_then(|m| Date::from_calendar_date(y, m, d).ok())
            {
                out.push((i, date));
                i += 10;
                continue;
            }
        }
        i += text[i..].chars().next().map_or(1, char::len_utf8);
    }
    out
}

/// Importes en formato español (`1.234,56`, `-150,00 €`) en una línea.
fn amounts_in(line: &str) -> Vec<Money> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<Money>| {
        let s = cur.trim_matches(|c: char| c == '.' || c == ',');
        if s.contains(',')
            && s.rsplit(',').next().is_some_and(|d| d.len() == 2)
            && let Some(m) = Money::parse(s)
        {
            out.push(m);
        }
        cur.clear();
    };
    let chars: Vec<char> = line.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        let digit_part = c.is_ascii_digit() || ((c == '.' || c == ',') && !cur.is_empty());
        let sign = c == '-' && cur.is_empty() && chars.get(i + 1).is_some_and(char::is_ascii_digit);
        if digit_part || sign {
            cur.push(c);
        } else {
            flush(&mut cur, &mut out);
        }
    }
    flush(&mut cur, &mut out);
    out
}

fn line_amount(text: &str, labels: &[&str]) -> Option<Money> {
    for line in text.lines() {
        let l = line.to_lowercase();
        if labels.iter().any(|lab| l.contains(lab))
            && let Some(m) = amounts_in(line).last()
        {
            return Some(*m);
        }
    }
    None
}

/// Número de factura tras etiquetas habituales ("Factura nº", "Número", "Invoice no").
fn find_number(text: &str) -> Option<String> {
    const LABELS: [&str; 20] = [
        "factura nº",
        "factura n.º",
        "factura no",
        "factura simplificada nº",
        "simplificada nº",
        "nº factura",
        "n.º factura",
        "nº de factura",
        "número de factura",
        "número factura",
        "num. factura",
        "núm. factura",
        "fra. nº",
        "fra nº",
        "ticket nº",
        "nº ticket",
        "número:",
        "nº:",
        "invoice no",
        "invoice number",
    ];
    for line in text.lines() {
        let l = line.to_lowercase();
        for lab in LABELS {
            if let Some(pos) = l.find(lab) {
                // Mismo índice en bytes: las etiquetas no cambian de longitud al pasar a minúsculas
                // salvo caracteres no ASCII, así que se recalcula sobre la línea original.
                let rest_l = &l[pos + lab.len()..];
                let start = line.len() - rest_l.len();
                let rest = line.get(start..).unwrap_or("");
                let token = rest
                    .trim_start_matches(|c: char| c == ':' || c == '.' || c.is_whitespace())
                    .split_whitespace()
                    .next()
                    .unwrap_or("");
                let token = token.trim_end_matches([',', ';']);
                if token.chars().any(|c| c.is_ascii_digit()) && token.len() <= 60 {
                    return Some(token.to_owned());
                }
            }
        }
    }
    None
}

/// ¿Es el NIF propio el del emisor? Sí cuando es el primer NIF del texto y no va tras una
/// etiqueta de cliente («Cliente:», «Facturar a»…) en su línea o en las dos anteriores: en las
/// facturas el emisor va en la cabecera y el cliente después.
fn own_issues(text: &str, own_nif: Option<&str>) -> bool {
    const CLIENT: [&str; 6] = [
        "cliente",
        "destinatario",
        "facturar a",
        "facturado a",
        "datos de facturación",
        "bill to",
    ];
    let Some(own) = own_nif else { return false };
    let lines: Vec<&str> = text.lines().collect();
    let Some(i) = lines.iter().position(|l| !find_nifs(l).is_empty()) else {
        return false;
    };
    if find_nifs(lines[i]).first().map(String::as_str) != Some(own) {
        return false;
    }
    !lines[i.saturating_sub(2)..=i].iter().any(|l| {
        let l = l.to_lowercase();
        CLIENT.iter().any(|c| l.contains(c))
    })
}

/// Extracción heurística desde el texto de un PDF. `own_nif` (el del usuario) ayuda a
/// distinguir proveedor y destinatario.
pub fn from_text(text: &str, own_nif: Option<&str>) -> Extraction {
    let mut e = Extraction {
        source: "texto".into(),
        ..Extraction::default()
    };
    let nifs = find_nifs(text);
    let (mine, others): (Vec<_>, Vec<_>) = nifs
        .iter()
        .cloned()
        .partition(|n| Some(n.as_str()) == own_nif);
    if own_issues(text, own_nif) {
        // Factura emitida: el NIF propio va en la cabecera, el otro es el cliente.
        e.supplier_nif = mine.first().and_then(|n| f(n.clone(), Confidence::Alta));
        e.recipient_nif = others.first().and_then(|n| f(n.clone(), Confidence::Media));
        e.warnings.push(
            "el emisor es tu propio NIF: parece una factura emitida por ti, no recibida".into(),
        );
    } else {
        if let Some(n) = others.first() {
            e.supplier_nif = f(
                n.clone(),
                if own_nif.is_some() {
                    Confidence::Alta
                } else {
                    Confidence::Media
                },
            );
        }
        if let Some(n) = mine.first() {
            e.recipient_nif = f(n.clone(), Confidence::Alta);
        } else if others.len() > 1 {
            e.recipient_nif = f(others[1].clone(), Confidence::Baja);
        }
    }
    // Nombre del proveedor: primera línea no vacía (encabezado habitual).
    if let Some(first) = text.lines().map(str::trim).find(|l| l.len() > 2) {
        let name: String = first.split("  ").next().unwrap_or(first).trim().to_owned();
        e.supplier_name = f(name, Confidence::Baja);
    }
    e.number = find_number(text).and_then(|n| f(n, Confidence::Media));
    // Fecha: la que sigue a "fecha", si no la primera.
    let dates = find_dates(text);
    let lower = text.to_lowercase();
    let labelled = lower
        .find("fecha")
        .and_then(|p| dates.iter().find(|(i, _)| *i >= p).map(|(_, d)| *d));
    e.issue_date = match labelled {
        Some(d) => f(d, Confidence::Media),
        None => dates.first().and_then(|(_, d)| f(*d, Confidence::Baja)),
    };
    e.base = line_amount(text, &["base imponible", "subtotal", "taxable base"])
        .and_then(|m| f(m, Confidence::Media));
    e.vat = line_amount(text, &["cuota iva", "iva ", "i.v.a", "vat "])
        .filter(|v| e.base.as_ref().is_none_or(|b| *v != b.value))
        .and_then(|m| f(m, Confidence::Media));
    e.withholding = line_amount(text, &["retención", "retencion", "irpf"])
        .map(|m| m.abs())
        .and_then(|m| f(m, Confidence::Media));
    e.total = line_amount(
        text,
        &["total a pagar", "total factura", "importe total", "total"],
    )
    .and_then(|m| f(m, Confidence::Media));
    e.check_consistency();
    if e.supplier_nif.is_none() {
        e.warnings
            .push("no se encontró el NIF del proveedor".into());
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    const TEXTO: &str = "Suministros Levante SL                                   FACTURA
CIF B12345674                                      Factura nº: FV-2026/118
Calle del Mar 3, 46001 València                    Fecha: 05/10/2026

Cliente: Carmen Española Española  NIF 12345678-Z

Descripción                    Cant.      Precio        Importe
Papel A4                          10      4,50 €        45,00 €
Tóner                              1     80,00 €        80,00 €

                                    Base imponible       125,00 €
                                          IVA 21 %        26,25 €
                                     TOTAL FACTURA       151,25 €
";

    #[test]
    fn texto_de_pdf_tipico() {
        let e = from_text(TEXTO, Some("12345678Z"));
        assert_eq!(e.supplier_nif.as_ref().unwrap().value, "B12345674");
        assert_eq!(
            e.supplier_nif.as_ref().unwrap().confidence,
            Confidence::Alta
        );
        assert_eq!(e.recipient_nif.as_ref().unwrap().value, "12345678Z");
        assert_eq!(e.number.unwrap().value, "FV-2026/118");
        assert_eq!(e.issue_date.unwrap().value, date!(2026 - 10 - 05));
        assert_eq!(e.base.unwrap().value, Money::from_cents(12_500));
        assert_eq!(e.vat.unwrap().value, Money::from_cents(2_625));
        assert_eq!(e.total.unwrap().value, Money::from_cents(15_125));
        assert!(e.warnings.is_empty(), "{:?}", e.warnings);
        assert_eq!(e.supplier_name.unwrap().value, "Suministros Levante SL");
    }

    #[test]
    fn factura_emitida_con_el_nif_propio_en_la_cabecera() {
        let t = "EMPRESA DEMO SL\nCIF B12345674\nFactura nº: A2026-0001\nFecha: 10/07/2026\n\nCliente: CLIENTE UNO SL NIF B98765431\n\nBase imponible 5.000,00 €\nIVA 21 % 1.050,00 €\nTOTAL FACTURA 6.050,00 €\n";
        let e = from_text(t, Some("B12345674"));
        assert_eq!(e.supplier_nif.unwrap().value, "B12345674");
        assert_eq!(e.recipient_nif.unwrap().value, "B98765431");
        assert_eq!(e.total.unwrap().value, Money::from_cents(605_000));
        assert!(e.warnings.iter().any(|w| w.contains("emitida")));
    }

    #[test]
    fn recibida_con_el_nif_propio_primero_tras_cliente() {
        // El cliente (nosotros) va arriba y el CIF del proveedor al pie: sigue siendo recibida.
        let t = "PROVEEDOR GRANDE\nCliente:\nEMPRESA DEMO SL B12345674\nFactura nº: PG-1\nTotal 121,00\nInscrita en el Registro Mercantil. CIF B98765431\n";
        let e = from_text(t, Some("B12345674"));
        assert_eq!(e.supplier_nif.unwrap().value, "B98765431");
        assert_eq!(e.recipient_nif.unwrap().value, "B12345674");
    }

    #[test]
    fn con_retencion_y_descuadre() {
        let t = "Asesoría Pérez\nNIF 00000000T\nFactura nº 7\nFecha 01-09-2026\nBase imponible 1.000,00\nIVA 21% 210,00\nRetención IRPF 15% -150,00\nTotal a pagar 1.060,00\n";
        let e = from_text(t, None);
        assert_eq!(
            e.withholding.as_ref().unwrap().value,
            Money::from_euros(150, 0)
        );
        assert!(e.warnings.is_empty(), "{:?}", e.warnings);
        let mal = t.replace("1.060,00", "1.070,00");
        assert!(!from_text(&mal, None).warnings.is_empty());
        assert_eq!(from_text(t, None).number.unwrap().value, "7");
    }

    #[test]
    fn sin_datos_avisa() {
        let e = from_text("documento sin nada útil", None);
        assert!(e.supplier_nif.is_none() && e.total.is_none());
        assert!(e.warnings.iter().any(|w| w.contains("NIF")));
    }

    #[test]
    fn importes_y_fechas() {
        assert_eq!(
            amounts_in("Total 1.234,56 € y -7,10"),
            [Money::from_cents(123_456), Money::from_cents(-710)]
        );
        assert!(amounts_in("Teléfono 963 123 456").is_empty());
        assert_eq!(find_dates("del 31/12/2026 al 32/01/2026").len(), 1);
        assert_eq!(parse_amount("123.4567"), Some(Money::from_cents(12_346)));
        assert_eq!(parse_amount("-0.5"), Some(Money::from_cents(-50)));
        assert_eq!(
            find_nifs("ESB12345674 y B12345675 y 12345678Z"),
            ["B12345674", "12345678Z"]
        );
    }
}
