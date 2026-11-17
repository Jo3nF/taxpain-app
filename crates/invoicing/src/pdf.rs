//! PDF de la factura (A4) con fuentes estándar (Helvetica, codificación WinAnsi: acentos, ñ, €).
//!
//! Contenido según RD 1619/2012 art. 6: número y serie, fecha de expedición (y de operación si
//! difiere), emisor y destinatario con NIF y domicilio, descripción, base por tipo, tipo,
//! cuota, menciones de exención / inversión del sujeto pasivo y, en rectificativas, la
//! referencia a la factura rectificada. Con VERI*FACTU añade el QR y la leyenda.

// La maquetación usa coordenadas en coma flotante (mm); los importes siguen en céntimos.
#![allow(clippy::float_arithmetic, clippy::cast_precision_loss)]

use printpdf::{
    BuiltinFont, Color, Line, LinePoint, Mm, Op, PaintMode, PdfDocument, PdfFontHandle, PdfPage,
    PdfSaveOptions, Point, Polygon, PolygonRing, Pt, Rgb, TextItem, WindingOrder,
};
use qrcode::{Color as QrColor, EcLevel, QrCode};
use taxpain_core::fx::{ExchangeRate, RateSource};
use taxpain_core::invoice::{
    Exemption, IdType, Invoice, InvoiceKind, NotSubject, PartyId, Totals, VatTreatment,
};
use taxpain_core::money::{Money, Rate};
use time::Date;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language {
    #[default]
    Es,
    En,
}

/// Marca VERI*FACTU: URL de cotejo para el QR y leyenda.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifactuMark {
    pub qr_url: String,
    pub legend: String,
}

#[derive(Clone, Debug, Default)]
pub struct PdfOptions {
    pub language: Language,
    pub verifactu: Option<VerifactuMark>,
    /// Texto libre al pie (p. ej. forma de pago, IBAN).
    pub footer: Option<String>,
    /// Factura en moneda extranjera: importes en esa moneda y, en una factura, el contravalor
    /// en euros con el tipo aplicado (art. 12.1 RD 1619/2012).
    pub currency: Option<PdfCurrency>,
}

/// Moneda de la factura si no es el euro.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PdfCurrency {
    /// ISO 4217.
    pub code: String,
    /// Tipo y totales en euros. Sin ellos (presupuesto o proforma) sólo se muestra la moneda.
    pub eur: Option<EurEquivalent>,
}

/// Contravalor en euros de una factura en moneda extranjera.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EurEquivalent {
    /// Unidades de la moneda por 1 €.
    pub rate: ExchangeRate,
    pub rate_date: Date,
    pub source: RateSource,
    /// Totales en euros: los que se contabilizan y declaran.
    pub totals: Totals,
}

struct Labels {
    invoice: &'static str,
    rectifying: &'static str,
    simplified: &'static str,
    quote: &'static str,
    proforma: &'static str,
    not_invoice: &'static str,
    valid_until: &'static str,
    number: &'static str,
    date: &'static str,
    operation_date: &'static str,
    customer: &'static str,
    description: &'static str,
    qty: &'static str,
    price: &'static str,
    discount: &'static str,
    vat: &'static str,
    amount: &'static str,
    base: &'static str,
    surcharge: &'static str,
    withholding: &'static str,
    total: &'static str,
    payable: &'static str,
    rectifies: &'static str,
    reverse_charge: &'static str,
    /// Servicio a un empresario de otro país de la UE (art. 196 Directiva IVA).
    reverse_charge_eu: &'static str,
    not_subject_n1: &'static str,
    /// No sujeta por las reglas de localización (fuera de la UE o a no empresarios).
    not_subject_n2: &'static str,
    /// Columna IVA: «ISP» / «no sujeta».
    short_rc: &'static str,
    short_ns: &'static str,
    vat_id: &'static str,
    tax_id: &'static str,
    exempt_export: &'static str,
    exempt_intra: &'static str,
    page: &'static str,
    /// Bloque del contravalor en euros (moneda extranjera).
    eur_title: &'static str,
    in_eur: &'static str,
    ecb_rate: &'static str,
    manual_rate: &'static str,
}

const ES: Labels = Labels {
    invoice: "FACTURA",
    rectifying: "FACTURA RECTIFICATIVA",
    simplified: "FACTURA SIMPLIFICADA",
    quote: "PRESUPUESTO",
    proforma: "FACTURA PROFORMA",
    not_invoice: "Este documento no es una factura ni tiene efectos fiscales.",
    valid_until: "Válido hasta el",
    number: "Número",
    date: "Fecha",
    operation_date: "Fecha de la operación",
    customer: "Cliente",
    description: "Descripción",
    qty: "Cant.",
    price: "Precio",
    discount: "Dto.",
    vat: "IVA",
    amount: "Importe",
    base: "Base imponible",
    surcharge: "Recargo de equivalencia",
    withholding: "Retención IRPF",
    total: "TOTAL FACTURA",
    payable: "TOTAL A PAGAR",
    rectifies: "Rectifica la factura",
    reverse_charge: "Inversión del sujeto pasivo (art. 84.Uno.2.º Ley 37/1992)",
    reverse_charge_eu: "Inversión del sujeto pasivo (art. 196 Directiva 2006/112/CE): \
                        el IVA lo declara el destinatario",
    not_subject_n1: "Operación no sujeta a IVA (art. 7 Ley 37/1992)",
    not_subject_n2: "Operación no sujeta al IVA español por las reglas de localización \
                     (arts. 68 a 70 Ley 37/1992)",
    short_rc: "ISP",
    short_ns: "No suj.",
    vat_id: "NIF-IVA",
    tax_id: "Id. fiscal",
    exempt_export: "Exportación exenta de IVA (art. 21 Ley 37/1992)",
    exempt_intra: "Entrega intracomunitaria exenta de IVA (art. 25 Ley 37/1992)",
    page: "Página",
    eur_title: "Contravalor en euros (art. 12.1 RD 1619/2012)",
    in_eur: "en euros",
    ecb_rate: "tipo de referencia del BCE del",
    manual_rate: "tipo de cambio del",
};

const EN: Labels = Labels {
    invoice: "INVOICE",
    rectifying: "CORRECTIVE INVOICE",
    simplified: "SIMPLIFIED INVOICE",
    quote: "QUOTE",
    proforma: "PROFORMA INVOICE",
    not_invoice: "This document is not an invoice and has no tax effect.",
    valid_until: "Valid until",
    number: "Number",
    date: "Date",
    operation_date: "Date of supply",
    customer: "Customer",
    description: "Description",
    qty: "Qty",
    price: "Price",
    discount: "Disc.",
    vat: "VAT",
    amount: "Amount",
    base: "Taxable base",
    surcharge: "Equivalence surcharge",
    withholding: "Income tax withholding",
    total: "INVOICE TOTAL",
    payable: "AMOUNT DUE",
    rectifies: "Corrects invoice",
    reverse_charge: "Reverse charge (art. 84.Uno.2.º Spanish VAT Act)",
    reverse_charge_eu: "Reverse charge (Article 196, Council Directive 2006/112/EC): \
                        VAT to be accounted for by the recipient",
    not_subject_n1: "Not subject to VAT (art. 7 Spanish VAT Act)",
    not_subject_n2: "Not subject to Spanish VAT under the place-of-supply rules \
                     (arts. 68 to 70 Spanish VAT Act)",
    short_rc: "RC",
    short_ns: "O/S",
    vat_id: "VAT No.",
    tax_id: "Tax ID",
    exempt_export: "Exempt export (art. 21 Spanish VAT Act)",
    exempt_intra: "Exempt intra-Community supply (art. 25 Spanish VAT Act; \
                   art. 138 Council Directive 2006/112/EC)",
    page: "Page",
    eur_title: "Equivalent in euros (art. 12.1 Spanish Invoicing Regulation, RD 1619/2012)",
    in_eur: "in EUR",
    ecb_rate: "ECB reference rate of",
    manual_rate: "exchange rate of",
};

const PAGE_W: f32 = 210.0;
const PAGE_H: f32 = 297.0;
const MARGIN: f32 = 18.0;
const RIGHT: f32 = PAGE_W - MARGIN;

/// Anchura aproximada (en mm) de un texto en Helvetica: métricas AFM de dígitos y signos
/// (exactas) y medias para letras. Suficiente para alinear importes a la derecha.
fn text_width(s: &str, size: f32, bold: bool) -> f32 {
    let units: u32 = s
        .chars()
        .map(|c| match c {
            '0'..='9' | '€' | '$' => 556,
            '.' | ',' | ' ' | ':' | ';' | '!' | 'i' | 'l' | 'j' | 'I' => 278,
            '-' | '(' | ')' | 'r' | 'f' | 't' => 333,
            '%' => 889,
            'm' | 'M' | 'W' => 833,
            'w' => 722,
            c if c.is_uppercase() => 667,
            _ => {
                if bold {
                    611
                } else {
                    556
                }
            }
        })
        .sum();
    // 1 pt = 0,3528 mm; unidades por 1000 del cuerpo.
    units as f32 / 1000.0 * size * 0.3528
}

fn black() -> Color {
    Color::Rgb(Rgb {
        r: 0.1,
        g: 0.1,
        b: 0.1,
        icc_profile: None,
    })
}

fn gray(v: f32) -> Color {
    Color::Rgb(Rgb {
        r: v,
        g: v,
        b: v,
        icc_profile: None,
    })
}

struct Canvas {
    pages: Vec<Vec<Op>>,
    ops: Vec<Op>,
}

impl Canvas {
    fn text(&mut self, x: f32, y: f32, size: f32, bold: bool, s: &str) {
        let font = if bold {
            BuiltinFont::HelveticaBold
        } else {
            BuiltinFont::Helvetica
        };
        self.ops.extend([
            Op::StartTextSection,
            Op::SetFillColor { col: black() },
            Op::SetTextCursor {
                pos: Point::new(Mm(x), Mm(y)),
            },
            Op::SetFont {
                font: PdfFontHandle::Builtin(font),
                size: Pt(size),
            },
            Op::ShowText {
                items: vec![TextItem::Text(s.to_owned())],
            },
            Op::EndTextSection,
        ]);
    }

    fn text_right(&mut self, right: f32, y: f32, size: f32, bold: bool, s: &str) {
        self.text(right - text_width(s, size, bold), y, size, bold, s);
    }

    fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, col: Color) {
        let p = |x: f32, y: f32| LinePoint {
            p: Point::new(Mm(x), Mm(y)),
            bezier: false,
        };
        self.ops.extend([
            Op::SetFillColor { col },
            Op::DrawPolygon {
                polygon: Polygon {
                    rings: vec![PolygonRing {
                        points: vec![p(x, y), p(x + w, y), p(x + w, y + h), p(x, y + h)],
                    }],
                    mode: PaintMode::Fill,
                    winding_order: WindingOrder::NonZero,
                },
            },
        ]);
    }

    fn hline(&mut self, y: f32) {
        let p = |x: f32| LinePoint {
            p: Point::new(Mm(x), Mm(y)),
            bezier: false,
        };
        self.ops.extend([
            Op::SetOutlineColor { col: gray(0.6) },
            Op::SetOutlineThickness { pt: Pt(0.5) },
            Op::DrawLine {
                line: Line {
                    points: vec![p(MARGIN), p(RIGHT)],
                    is_closed: false,
                },
            },
        ]);
    }

    fn new_page(&mut self) {
        let ops = std::mem::take(&mut self.ops);
        self.pages.push(ops);
    }
}

/// Lado del símbolo QR, sin la zona de silencio: la Orden HAC/1177/2024 art. 21 exige «entre
/// 30x30 y 40x40 milímetros». (Repartir 35 mm entre símbolo y silencio dejaba el símbolo en
/// 29,7 mm con las URL de hasta 122 bytes, versión 7.)
const QR_SYMBOL_MM: f32 = 32.0;
/// Zona de silencio en blanco alrededor del símbolo: al menos 4 módulos (ISO/IEC 18004) desde la
/// versión 5 (37 módulos: 4 × 32 / 37 = 3,46 mm), que cubre las URL del QR tributario.
const QR_QUIET_MM: f32 = 3.5;

/// Lado de cada módulo para un símbolo de `n` × `n` módulos.
fn qr_module(n: usize) -> f32 {
    QR_SYMBOL_MM / n as f32
}

/// QR (ISO/IEC 18004, corrección M) como módulos vectoriales: símbolo de `QR_SYMBOL_MM` de lado
/// con la esquina superior izquierda en (`x`, `top`). Quien lo llama deja `QR_QUIET_MM` en blanco
/// alrededor.
fn draw_qr(c: &mut Canvas, url: &str, x: f32, top: f32) -> Result<(), String> {
    let code = QrCode::with_error_correction_level(url.as_bytes(), EcLevel::M)
        .map_err(|e| format!("QR: {e}"))?;
    let n = code.width();
    let module = qr_module(n);
    for (i, color) in code.to_colors().iter().enumerate() {
        if *color == QrColor::Dark {
            let (row, col) = ((i / n) as f32, (i % n) as f32);
            c.rect(
                x + col * module,
                top - (row + 1.0) * module,
                module,
                module,
                Color::Rgb(Rgb {
                    r: 0.0,
                    g: 0.0,
                    b: 0.0,
                    icc_profile: None,
                }),
            );
        }
    }
    Ok(())
}

/// Importe en euros (`1.234,56 €`) o en la moneda de la factura (`1.234,56 USD`).
fn fmt_money(m: Money, currency: Option<&str>) -> String {
    match currency {
        None => m.to_string(),
        Some(code) => format!("{} {code}", m.to_string().trim_end_matches(" €")),
    }
}

/// «1 € = 1,1193 USD, tipo de referencia del BCE del 23/10/2026».
fn rate_text(code: &str, e: &EurEquivalent, l: &Labels) -> String {
    let source = match e.source {
        RateSource::Bce => l.ecb_rate,
        RateSource::Manual => l.manual_rate,
    };
    format!(
        "1 € = {} {code}, {source} {}",
        e.rate.decimal_es(),
        fmt_date(e.rate_date)
    )
}

fn fmt_date(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), d.month() as u8, d.year())
}

fn fmt_qty(milli: i64) -> String {
    // Sobre el valor absoluto y el signo aparte: `-500 / 1000` da 0 y perdería el «-».
    let abs = milli.unsigned_abs();
    let (int, frac) = (abs / 1000, abs % 1000);
    let sign = if milli < 0 { "-" } else { "" };
    if frac == 0 {
        format!("{sign}{int}")
    } else {
        format!("{sign}{int},{}", format!("{frac:03}").trim_end_matches('0'))
    }
}

fn party_id(id: &PartyId, l: &Labels) -> String {
    match id {
        PartyId::Nif(n) => format!("NIF {n}"),
        PartyId::Foreign {
            id_type: IdType::NifIva,
            id,
            ..
        } => format!("{} {id}", l.vat_id),
        PartyId::Foreign { country, id, .. } => format!("{} {id} ({country})", l.tax_id),
    }
}

/// El destinatario es un empresario de otro Estado miembro (identificado con NIF-IVA) a efectos
/// de los servicios: un servicio no sujeto (N2) a él es una inversión del sujeto pasivo del art.
/// 196 de la Directiva. Irlanda del Norte (XI) sólo es UE para los bienes.
fn eu_business(invoice: &Invoice) -> bool {
    matches!(
        invoice.recipient.as_ref().map(|r| &r.id),
        Some(PartyId::Foreign {
            id_type: IdType::NifIva,
            country,
            ..
        }) if taxpain_core::eu::is_other_member_state(country, false)
    )
}

fn vat_label(v: VatTreatment, l: &Labels, eu: bool) -> String {
    match v {
        VatTreatment::Taxed { rate } => rate.to_string(),
        VatTreatment::ReverseCharge => l.short_rc.to_owned(),
        VatTreatment::Exempt(e) => format!("{e:?}"),
        VatTreatment::NotSubject(NotSubject::N2) if eu => l.short_rc.to_owned(),
        VatTreatment::NotSubject(_) => l.short_ns.to_owned(),
    }
}

/// Etiqueta de las líneas no sujetas (N2) de una venta por la ventanilla única: el país y el
/// tipo de IVA de destino («FR 20 %»).
fn oss_tag(totals: &Totals, v: VatTreatment) -> Option<String> {
    match (&totals.oss, v) {
        (Some(o), VatTreatment::NotSubject(NotSubject::N2)) => {
            Some(format!("{} {}", o.country, o.rate))
        }
        _ => None,
    }
}

/// Mención de la factura con IVA de otro Estado miembro (ventanilla única).
/// `TODO(VERIFY)`: el texto no es obligatorio en el RD 1619/2012; las reglas de facturación son
/// las del Estado de identificación (art. 219 bis Directiva 2006/112/CE).
fn oss_mention(o: &taxpain_core::invoice::OssVat, english: bool) -> String {
    let pais = taxpain_core::invoice::oss::nombre_pais(&o.country);
    if english {
        format!(
            "VAT of {} ({}), Member State of consumption, charged under the EU One-Stop Shop \
             (OSS, Union scheme) and declared in Spain",
            o.country, o.rate
        )
    } else {
        format!(
            "IVA de {pais} ({}), Estado miembro de consumo, repercutido por el régimen de \
             ventanilla única (OSS, régimen de la Unión) y declarado en España (arts. 73 y 163 \
             unvicies Ley 37/1992)",
            o.rate
        )
    }
}

/// Mención legal de cada tratamiento (RD 1619/2012, art. 6.1.j y m).
fn mention(v: VatTreatment, l: &Labels, eu: bool, english: bool) -> Option<String> {
    Some(match v {
        VatTreatment::Taxed { .. } => return None,
        VatTreatment::ReverseCharge => l.reverse_charge.to_owned(),
        VatTreatment::Exempt(Exemption::E2) => l.exempt_export.to_owned(),
        VatTreatment::Exempt(Exemption::E5) => l.exempt_intra.to_owned(),
        VatTreatment::Exempt(e) if english => format!("VAT exempt ({e:?})"),
        VatTreatment::Exempt(e) => e.legal_mention().to_owned(),
        VatTreatment::NotSubject(NotSubject::N2) if eu => l.reverse_charge_eu.to_owned(),
        VatTreatment::NotSubject(NotSubject::N2) => l.not_subject_n2.to_owned(),
        VatTreatment::NotSubject(NotSubject::N1) => l.not_subject_n1.to_owned(),
    })
}

/// Parte una descripción en líneas de como mucho `max` caracteres.
fn wrap(s: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for word in s.split_whitespace() {
        if !cur.is_empty() && cur.chars().count() + 1 + word.chars().count() > max {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push(' ');
        }
        cur.push_str(word);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Genera el PDF. `totals` debe ser el resultado de `compute_totals` sobre `invoice.lines`.
pub fn render(invoice: &Invoice, totals: &Totals, opts: &PdfOptions) -> Result<Vec<u8>, String> {
    render_inner(invoice, totals, opts, None)
}

/// Presupuesto o factura proforma (no son facturas: sin VERI*FACTU ni efectos fiscales).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuoteKind {
    Presupuesto,
    Proforma,
}

/// PDF de un presupuesto o una proforma: mismo diseño que la factura, otro título, sin QR y con
/// la mención de que no es una factura (y la validez del presupuesto, si la tiene).
pub fn render_quote(
    invoice: &Invoice,
    totals: &Totals,
    opts: &PdfOptions,
    kind: QuoteKind,
    valid_until: Option<Date>,
) -> Result<Vec<u8>, String> {
    let mut o = opts.clone();
    o.verifactu = None;
    render_inner(invoice, totals, &o, Some((kind, valid_until)))
}

fn render_inner(
    invoice: &Invoice,
    totals: &Totals,
    opts: &PdfOptions,
    quote: Option<(QuoteKind, Option<Date>)>,
) -> Result<Vec<u8>, String> {
    let l = match opts.language {
        Language::Es => &ES,
        Language::En => &EN,
    };
    let mut c = Canvas {
        pages: Vec::new(),
        ops: Vec::new(),
    };
    let mut y = PAGE_H - MARGIN;
    let cur = opts.currency.as_ref().map(|c| c.code.as_str());
    let eur = opts.currency.as_ref().and_then(|c| c.eur.as_ref());
    let fm = |m: Money| fmt_money(m, cur);

    // Cabecera: QR VERI*FACTU arriba a la izquierda, emisor y título.
    let mut x_issuer = MARGIN;
    if let Some(v) = &opts.verifactu {
        // Orden HAC/1177/2024 art. 21: «QR tributario:» encima, QR de 30–40 mm con margen
        // y la leyenda debajo con letra no menor que la del resto de la factura (9 pt).
        c.text(MARGIN, y - 3.0, 9.0, true, "QR tributario:");
        // Símbolo de 32 mm de y − 8 a y − 40, con 3,5 mm de silencio por cada lado (el de la
        // izquierda cae en el margen de la página); la leyenda empieza bajo el silencio.
        draw_qr(&mut c, &v.qr_url, MARGIN, y - 4.5 - QR_QUIET_MM)?;
        for (i, line) in wrap(&v.legend, 30).iter().enumerate() {
            c.text(MARGIN, y - 47.0 - 4.0 * i as f32, 9.0, true, line);
        }
        x_issuer = MARGIN + 42.0;
    }
    let title = match (quote, invoice.kind) {
        (Some((QuoteKind::Presupuesto, _)), _) => l.quote,
        (Some((QuoteKind::Proforma, _)), _) => l.proforma,
        (None, InvoiceKind::Rectificativa(_)) => l.rectifying,
        (None, InvoiceKind::Simplificada) => l.simplified,
        (None, _) => l.invoice,
    };
    c.text_right(RIGHT, y - 6.0, 16.0, true, title);
    c.text_right(
        RIGHT,
        y - 13.0,
        10.0,
        false,
        &format!("{}: {}", l.number, invoice.number),
    );
    c.text_right(
        RIGHT,
        y - 18.0,
        10.0,
        false,
        &format!("{}: {}", l.date, fmt_date(invoice.issue_date)),
    );
    if let Some(od) = invoice.operation_date.filter(|d| *d != invoice.issue_date) {
        c.text_right(
            RIGHT,
            y - 23.0,
            9.0,
            false,
            &format!("{}: {}", l.operation_date, fmt_date(od)),
        );
    }
    c.text(x_issuer, y - 6.0, 12.0, true, &invoice.issuer.name);
    c.text(
        x_issuer,
        y - 11.5,
        9.0,
        false,
        &party_id(&invoice.issuer.id, l),
    );
    if let Some(a) = &invoice.issuer.address {
        for (i, line) in a.lines().iter().enumerate().take(3) {
            c.text(x_issuer, y - 16.0 - 4.5 * i as f32, 9.0, false, line);
        }
    }
    y -= if opts.verifactu.is_some() { 56.0 } else { 34.0 };

    if let Some(r) = &invoice.recipient {
        c.text(MARGIN, y, 8.0, true, l.customer);
        c.text(MARGIN, y - 5.0, 10.0, true, &r.name);
        c.text(MARGIN, y - 10.0, 9.0, false, &party_id(&r.id, l));
        let mut yy = y - 14.5;
        if let Some(a) = &r.address {
            for line in a.lines().iter().take(3) {
                c.text(MARGIN, yy, 9.0, false, line);
                yy -= 4.5;
            }
        }
        y = yy - 6.0;
    }

    // Tabla de líneas.
    let cols = [
        RIGHT - 70.0,
        RIGHT - 50.0,
        RIGHT - 34.0,
        RIGHT - 22.0,
        RIGHT,
    ];
    let header = |c: &mut Canvas, y: f32| {
        c.rect(MARGIN, y - 2.0, RIGHT - MARGIN, 7.0, gray(0.92));
        c.text(MARGIN + 1.5, y, 8.5, true, l.description);
        c.text_right(cols[0], y, 8.5, true, l.qty);
        c.text_right(cols[1], y, 8.5, true, l.price);
        c.text_right(cols[2], y, 8.5, true, l.discount);
        c.text_right(cols[3], y, 8.5, true, l.vat);
        c.text_right(cols[4] - 1.5, y, 8.5, true, l.amount);
    };
    header(&mut c, y);
    y -= 8.0;
    for line in &invoice.lines {
        let desc = wrap(&line.description, 60);
        let needed = 5.0 * desc.len() as f32;
        // Sitio para los totales (y para el contravalor en euros, si lo hay).
        let reserve = if eur.is_some() { 115.0 } else { 70.0 };
        if y - needed < MARGIN + reserve {
            c.new_page();
            y = PAGE_H - MARGIN;
            header(&mut c, y);
            y -= 8.0;
        }
        for (i, d) in desc.iter().enumerate() {
            c.text(MARGIN + 1.5, y - 5.0 * i as f32, 9.0, false, d);
        }
        let base = taxpain_core::invoice::totals::line_base(line);
        c.text_right(cols[0], y, 9.0, false, &fmt_qty(line.quantity_milli));
        c.text_right(cols[1], y, 9.0, false, &fm(line.unit_price));
        if line.discount != Rate::ZERO {
            c.text_right(cols[2], y, 9.0, false, &line.discount.to_string());
        }
        c.text_right(
            cols[3],
            y,
            9.0,
            false,
            &oss_tag(totals, line.vat)
                .unwrap_or_else(|| vat_label(line.vat, l, eu_business(invoice))),
        );
        c.text_right(cols[4] - 1.5, y, 9.0, false, &fm(base));
        y -= needed + 1.5;
    }
    c.hline(y + 2.0);
    y -= 5.0;

    // Desglose y totales.
    let label_x = RIGHT - 45.0;
    let row = |c: &mut Canvas, y: &mut f32, label: String, amount: Money, bold: bool| {
        c.text_right(label_x, *y, 9.0, bold, &label);
        c.text_right(RIGHT - 1.5, *y, 9.0, bold, &fm(amount));
        *y -= 5.0;
    };
    for b in &totals.breakdown {
        let oss = oss_tag(totals, b.vat);
        let tag = oss
            .clone()
            .unwrap_or_else(|| vat_label(b.vat, l, eu_business(invoice)));
        row(&mut c, &mut y, format!("{} ({tag})", l.base), b.base, false);
        if let (Some(t), Some(o)) = (oss, &totals.oss) {
            row(&mut c, &mut y, format!("{} {t}", l.vat), o.vat, false);
        }
        if let VatTreatment::Taxed { rate } = b.vat {
            row(
                &mut c,
                &mut y,
                format!("{} {rate}", l.vat),
                b.vat_amount,
                false,
            );
        }
        if let Some(r) = b.surcharge_rate {
            row(
                &mut c,
                &mut y,
                format!("{} {r}", l.surcharge),
                b.surcharge_amount,
                false,
            );
        }
    }
    // Un presupuesto o una proforma no es una factura: «TOTAL», no «TOTAL FACTURA».
    let total_label = if quote.is_some() { "TOTAL" } else { l.total };
    row(
        &mut c,
        &mut y,
        total_label.to_owned(),
        totals.invoice_total,
        true,
    );
    for (rate, _, amount) in &totals.withholdings {
        row(
            &mut c,
            &mut y,
            format!("{} {rate}", l.withholding),
            -*amount,
            false,
        );
    }
    if !totals.withholdings.is_empty() {
        row(&mut c, &mut y, l.payable.to_owned(), totals.payable, true);
    }
    // Moneda extranjera: base, cuota y total en euros con el tipo aplicado (art. 12.1 RD
    // 1619/2012: la cuota repercutida se expresa en euros).
    if let (Some(code), Some(e)) = (cur, eur) {
        y -= 2.0;
        c.text_right(RIGHT - 1.5, y, 8.5, true, l.eur_title);
        y -= 4.5;
        c.text_right(RIGHT - 1.5, y, 8.0, false, &rate_text(code, e, l));
        y -= 5.5;
        let erow = |c: &mut Canvas, y: &mut f32, label: String, amount: Money, bold: bool| {
            c.text_right(label_x, *y, 9.0, bold, &label);
            c.text_right(RIGHT - 1.5, *y, 9.0, bold, &amount.to_string());
            *y -= 5.0;
        };
        for b in &e.totals.breakdown {
            let tag = vat_label(b.vat, l, eu_business(invoice));
            erow(
                &mut c,
                &mut y,
                format!("{} ({tag}) {}", l.base, l.in_eur),
                b.base,
                false,
            );
            if let VatTreatment::Taxed { rate } = b.vat {
                erow(
                    &mut c,
                    &mut y,
                    format!("{} {rate} {}", l.vat, l.in_eur),
                    b.vat_amount,
                    false,
                );
            }
            if let Some(r) = b.surcharge_rate {
                erow(
                    &mut c,
                    &mut y,
                    format!("{} {r} {}", l.surcharge, l.in_eur),
                    b.surcharge_amount,
                    false,
                );
            }
        }
        erow(
            &mut c,
            &mut y,
            format!("{} {}", l.total, l.in_eur),
            e.totals.invoice_total,
            true,
        );
        for (rate, _, amount) in &e.totals.withholdings {
            erow(
                &mut c,
                &mut y,
                format!("{} {rate} {}", l.withholding, l.in_eur),
                -*amount,
                false,
            );
        }
        if !e.totals.withholdings.is_empty() {
            erow(
                &mut c,
                &mut y,
                format!("{} {}", l.payable, l.in_eur),
                e.totals.payable,
                true,
            );
        }
    }
    y -= 4.0;

    // Menciones legales.
    let mut mentions: Vec<String> = Vec::new();
    let english = matches!(opts.language, Language::En);
    if let Some((_, valid)) = quote {
        mentions.push(l.not_invoice.to_owned());
        if let Some(d) = valid {
            mentions.push(format!("{} {}", l.valid_until, fmt_date(d)));
        }
    }
    for b in &totals.breakdown {
        let m = match (&totals.oss, b.vat) {
            (Some(o), VatTreatment::NotSubject(NotSubject::N2)) => Some(oss_mention(o, english)),
            _ => mention(b.vat, l, eu_business(invoice), english),
        };
        if let Some(m) = m
            && !mentions.contains(&m)
        {
            mentions.push(m);
        }
    }
    for (num, date) in &invoice.rectified {
        mentions.push(format!("{} {num} ({})", l.rectifies, fmt_date(*date)));
    }
    if let Some(n) = &invoice.notes {
        mentions.push(n.clone());
    }
    if let Some(f) = &opts.footer {
        mentions.push(f.clone());
    }
    mentions.dedup();
    for m in mentions {
        for line in wrap(&m, 100) {
            if y < MARGIN + 8.0 {
                c.new_page();
                y = PAGE_H - MARGIN;
            }
            c.text(MARGIN, y, 8.0, false, &line);
            y -= 4.2;
        }
    }
    c.new_page();

    // Numeración de páginas.
    let total_pages = c.pages.len();
    let pages: Vec<PdfPage> = c
        .pages
        .into_iter()
        .enumerate()
        .map(|(i, mut ops)| {
            if total_pages > 1 {
                let mut pc = Canvas {
                    pages: Vec::new(),
                    ops: Vec::new(),
                };
                pc.text_right(
                    RIGHT,
                    MARGIN - 8.0,
                    8.0,
                    false,
                    &format!("{} {}/{total_pages}", l.page, i + 1),
                );
                ops.extend(pc.ops);
            }
            PdfPage::new(Mm(PAGE_W), Mm(PAGE_H), ops)
        })
        .collect();
    let mut doc = PdfDocument::new(&format!("{title} {}", invoice.number));
    let mut warnings = Vec::new();
    Ok(doc
        .with_pages(pages)
        .save(&PdfSaveOptions::default(), &mut warnings))
}

/// Párrafo de un documento de texto (cartas, avisos).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Para {
    /// Título en negrita grande.
    Title(String),
    Text(String),
    Bold(String),
    /// Línea con texto a la izquierda e importe alineado a la derecha.
    Row(String, String),
    /// Separación vertical.
    Gap,
}

/// Documento A4 de texto: cada elemento de `docs` empieza en página nueva (p. ej. una carta por
/// destinatario). Los párrafos largos se parten en líneas.
pub fn text_document(title: &str, docs: &[Vec<Para>]) -> Result<Vec<u8>, String> {
    text_document_inner(title, None, docs)
}

/// Como [`text_document`], con un pie en cada página: `footer` y «Folio n de N». Para los libros
/// que se legalizan (libro de actas, registro de socios), que se numeran correlativamente.
pub fn text_document_foliated(
    title: &str,
    footer: &str,
    docs: &[Vec<Para>],
) -> Result<Vec<u8>, String> {
    text_document_inner(title, Some(footer), docs)
}

fn text_document_inner(
    title: &str,
    footer: Option<&str>,
    docs: &[Vec<Para>],
) -> Result<Vec<u8>, String> {
    let mut c = Canvas {
        pages: Vec::new(),
        ops: Vec::new(),
    };
    for (i, doc) in docs.iter().enumerate() {
        if i > 0 {
            c.new_page();
        }
        let mut y = PAGE_H - MARGIN - 4.0;
        for p in doc {
            if y < MARGIN + 10.0 {
                c.new_page();
                y = PAGE_H - MARGIN - 4.0;
            }
            match p {
                Para::Title(t) => {
                    c.text(MARGIN, y, 14.0, true, t);
                    y -= 9.0;
                }
                Para::Text(t) | Para::Bold(t) => {
                    let bold = matches!(p, Para::Bold(_));
                    for line in wrap(t, 95) {
                        c.text(MARGIN, y, 10.0, bold, &line);
                        y -= 5.0;
                    }
                    y -= 1.5;
                }
                Para::Row(l, r) => {
                    c.text(MARGIN + 4.0, y, 10.0, false, l);
                    c.text_right(RIGHT - 30.0, y, 10.0, false, r);
                    y -= 5.5;
                }
                Para::Gap => y -= 5.0,
            }
        }
    }
    c.new_page();
    let pages: Vec<Vec<Op>> = c.pages.into_iter().filter(|ops| !ops.is_empty()).collect();
    let total = pages.len();
    let pages: Vec<PdfPage> = pages
        .into_iter()
        .enumerate()
        .map(|(i, mut ops)| {
            if let Some(f) = footer {
                let mut pc = Canvas {
                    pages: Vec::new(),
                    ops: Vec::new(),
                };
                pc.text(MARGIN, MARGIN - 8.0, 8.0, false, f);
                pc.text_right(
                    RIGHT,
                    MARGIN - 8.0,
                    8.0,
                    false,
                    &format!("Folio {} de {total}", i + 1),
                );
                ops.extend(pc.ops);
            }
            PdfPage::new(Mm(PAGE_W), Mm(PAGE_H), ops)
        })
        .collect();
    let mut doc = PdfDocument::new(title);
    let mut warnings = Vec::new();
    Ok(doc
        .with_pages(pages)
        .save(&PdfSaveOptions::default(), &mut warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use taxpain_core::invoice::Party;
    use time::macros::date;

    #[test]
    fn cantidades_negativas_menores_que_uno_conservan_el_signo() {
        assert_eq!(fmt_qty(-500), "-0,5");
        assert_eq!(fmt_qty(-1500), "-1,5");
        assert_eq!(fmt_qty(2500), "2,5");
        assert_eq!(fmt_qty(-2000), "-2");
        assert_eq!(fmt_qty(1000), "1");
        assert_eq!(fmt_qty(125), "0,125");
    }

    #[test]
    fn el_qr_mide_entre_30_y_40_mm_con_silencio_de_4_modulos() {
        // URL del QR tributario de 100 a 200 bytes (pruebas y producción, series cortas y
        // largas): versiones 6 a 10. Orden HAC/1177/2024 art. 21 e ISO/IEC 18004.
        let base = "https://prewww2.aeat.es/wlpl/TIKE-CONT/ValidarQR?nif=B12345674&numserie=";
        for len in 100..=200 {
            let url = format!("{base}{}", "A".repeat(len - base.len()));
            let n = QrCode::with_error_correction_level(url.as_bytes(), EcLevel::M)
                .unwrap()
                .width();
            let side = n as f32 * qr_module(n);
            assert!((30.0..=40.0).contains(&side), "{len} bytes: {side} mm");
            assert!(QR_QUIET_MM >= 4.0 * qr_module(n), "{len} bytes: silencio");
        }
    }

    fn invoice(recipient: PartyId) -> Invoice {
        Invoice {
            kind: InvoiceKind::Completa,
            number: "A-1".into(),
            issue_date: date!(2026 - 10 - 24),
            operation_date: None,
            issuer: Party {
                id: PartyId::Nif("B12345674".into()),
                name: "Emisor".into(),
                address: None,
                person: None,
            },
            recipient: Some(Party {
                id: recipient,
                name: "Cliente".into(),
                address: None,
                person: None,
            }),
            lines: vec![],
            rectified: vec![],
            description: String::new(),
            notes: None,
        }
    }

    #[test]
    fn menciones_de_clientes_de_fuera() {
        let eu = invoice(PartyId::Foreign {
            country: "FR".into(),
            id_type: IdType::NifIva,
            id: "FR12345678901".into(),
        });
        let us = invoice(PartyId::Foreign {
            country: "US".into(),
            id_type: IdType::DocumentoOficial,
            id: "123456789".into(),
        });
        let n2 = VatTreatment::NotSubject(NotSubject::N2);
        // Servicio a una empresa de la UE: inversión del sujeto pasivo (art. 196 Directiva).
        let m = mention(n2, &ES, eu_business(&eu), false).unwrap();
        assert!(
            m.starts_with("Inversión del sujeto pasivo (art. 196"),
            "{m}"
        );
        let m = mention(n2, &EN, eu_business(&eu), true).unwrap();
        assert!(m.starts_with("Reverse charge (Article 196"), "{m}");
        assert_eq!(vat_label(n2, &EN, eu_business(&eu)), "RC");
        // Fuera de la UE: no sujeta por las reglas de localización.
        let m = mention(n2, &ES, eu_business(&us), false).unwrap();
        assert!(m.contains("reglas de localización"), "{m}");
        assert_eq!(vat_label(n2, &ES, eu_business(&us)), "No suj.");
        // Particular de la UE (sin NIF-IVA) y servicio a Irlanda del Norte (Reino Unido para los
        // servicios): sin inversión del sujeto pasivo.
        for id in [
            PartyId::Foreign {
                country: "FR".into(),
                id_type: IdType::DocumentoOficial,
                id: "123456789".into(),
            },
            PartyId::Foreign {
                country: "XI".into(),
                id_type: IdType::NifIva,
                id: "XI123456789".into(),
            },
        ] {
            assert!(!eu_business(&invoice(id)));
        }
        assert!(
            mention(VatTreatment::Exempt(Exemption::E2), &EN, false, true)
                .unwrap()
                .starts_with("Exempt export")
        );
        assert_eq!(
            mention(
                VatTreatment::Taxed {
                    rate: Rate::percent(21)
                },
                &ES,
                false,
                false
            ),
            None
        );
        // Identificación del cliente.
        assert_eq!(
            party_id(&eu.recipient.as_ref().unwrap().id, &ES),
            "NIF-IVA FR12345678901"
        );
        assert_eq!(
            party_id(&us.recipient.as_ref().unwrap().id, &EN),
            "Tax ID 123456789 (US)"
        );
    }

    #[test]
    fn factura_oss_lleva_el_pais_y_el_tipo_de_destino() {
        use taxpain_core::invoice::{InvoiceLine, OssSupply, RoundingMode, compute_totals};
        let n2 = VatTreatment::NotSubject(NotSubject::N2);
        let line = InvoiceLine {
            description: "Curso en línea".into(),
            quantity_milli: 1000,
            unit_price: Money::from_euros(100, 0),
            discount: Rate::ZERO,
            vat: n2,
            surcharge: None,
            withholding: None,
            advance: false,
        };
        let mut t = compute_totals(&[line], RoundingMode::PerRate);
        assert_eq!(oss_tag(&t, n2), None);
        taxpain_core::invoice::oss::apply(&mut t, "FR", OssSupply::Servicios, Rate::percent(20));
        assert_eq!(oss_tag(&t, n2).as_deref(), Some("FR 20 %"));
        let o = t.oss.as_ref().unwrap();
        assert!(oss_mention(o, false).starts_with("IVA de Francia (20 %)"));
        assert!(oss_mention(o, true).contains("One-Stop Shop"));
    }
}
