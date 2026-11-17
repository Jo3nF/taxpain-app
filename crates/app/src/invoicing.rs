//! Emisión y consulta de facturas.
//!
//! `issue` es un acto con efectos legales: exige `confirmed = true` (la UI lo pide tras mostrar
//! el borrador) y, en una única transacción, asigna el número correlativo, genera el registro
//! VERI*FACTU encadenado, el PDF (con QR) y el Facturae, los guarda cifrados y lo audita.
//! Después copia los ficheros a la carpeta destino (si falla, la factura sigue emitida y se
//! informa).
//!
//! Moneda extranjera (`InvoiceDraft::currency`): las líneas van en la moneda de la factura y
//! todo lo que cuenta (asiento, modelos, VERI*FACTU, cartera, columnas de `invoice_issued`) va
//! en euros con el tipo vigente al devengo (art. 79.Once LIVA; ver `taxpain_core::fx`). La
//! factura en la moneda (líneas y totales) se guarda aparte (`Stored::fx`) para el PDF, la
//! Facturae, la UBL y la interfaz; el PDF lleva además la base, la cuota y el total en euros con
//! el tipo y su fecha (art. 12.1 RD 1619/2012).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use taxpain_core::eu::is_other_member_state;
use taxpain_core::fx::{self, ForeignCurrency};
use taxpain_core::invoice::{
    Exemption, IdType, Invoice, InvoiceKind, InvoiceLine, Party, PartyId, Rectification, Totals,
    VatTreatment, compute_totals,
};
use taxpain_core::money::Money;
use taxpain_invoicing::facturae::{CorrectionReason, FacturaeFx, to_facturae, to_facturae_fx};
use taxpain_invoicing::pdf::{self, EurEquivalent, PdfCurrency, PdfOptions, VerifactuMark};
use taxpain_invoicing::verifactu::{
    self, AltaOptions, ChainLink, QrEnv, RechazoPrevio, Record, Response, SistemaInformatico,
};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::settings::{InvoicingSettings, Language, SETTINGS_KEY};

/// Datos que el usuario rellena; el número lo pone `issue`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvoiceDraft {
    pub kind: InvoiceKind,
    /// Serie; por defecto la de los ajustes (o la de rectificativas).
    pub series: Option<String>,
    pub issue_date: Date,
    pub operation_date: Option<Date>,
    pub recipient: Option<Party>,
    pub lines: Vec<InvoiceLine>,
    #[serde(default)]
    pub rectified: Vec<(String, Date)>,
    /// Rectificativas por sustitución: (base, cuota) rectificadas.
    #[serde(default)]
    pub rectified_amounts: Option<(Money, Money)>,
    pub description: String,
    pub notes: Option<String>,
    /// Moneda si la factura no es en euros: las líneas van en esa moneda y TaxPain guarda,
    /// contabiliza y declara los importes en euros con este tipo (art. 79.Once LIVA).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<ForeignCurrency>,
    /// Venta a un consumidor de otro Estado miembro que tributa allí (ventanilla única, OSS):
    /// las líneas van no sujetas (N2) y se suma el IVA de destino con este tipo (ver
    /// [`crate::oss`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oss: Option<crate::oss::OssDraft>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IssuedSummary {
    pub id: i64,
    pub number: String,
    pub issue_date: String,
    pub kind: String,
    pub recipient_name: Option<String>,
    pub base: Money,
    pub vat: Money,
    pub total: Money,
    pub payable: Money,
    pub status: String,
    /// Estado del último registro VERI*FACTU: `pendiente`, `pruebas` (aceptado sólo en el
    /// entorno de pruebas), `correcto`, `aceptado_con_errores`, `incorrecto` o `sin_respuesta`
    /// (la AEAT contestó dos veces sin línea para él).
    pub verifactu_status: Option<String>,
    /// Motivo del rechazo o del aviso de la AEAT del último registro, si lo hay.
    pub verifactu_error: Option<String>,
    pub exported_path: Option<String>,
    pub export_error: Option<String>,
    /// Hay Facturae / UBL guardados para descargar.
    pub has_facturae: bool,
    pub has_ubl: bool,
    /// `app` (emitida con TaxPain) o `importada` (del libro registro de la gestoría).
    pub origin: String,
    /// Factura en moneda extranjera: la moneda y lo que se cobra en ella (los importes de
    /// arriba son en euros).
    pub currency: Option<String>,
    pub currency_payable: Option<Money>,
    /// Moneda y tipo con que se emitió (para rectificarla al mismo tipo).
    pub currency_rate: Option<ForeignCurrency>,
}

/// Lo que se guarda en `invoice_issued.data`: la factura y sus totales **en euros** y, si se
/// emitió en otra moneda, la factura en esa moneda.
#[derive(Debug, Serialize, Deserialize)]
struct Stored {
    invoice: Invoice,
    totals: Totals,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fx: Option<StoredFx>,
    /// Idioma del PDF (las emitidas antes de guardarlo no lo tienen: ver [`language_of`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    language: Option<Language>,
}

/// Idioma en que se emitió la factura `id`, para repetirla o duplicarla igual: el guardado al
/// emitirla o, en las anteriores, el de la ficha del cliente, inglés si el cliente es de fuera
/// y, si no, `None` (el de los ajustes).
pub fn language_of(v: &Vault, id: i64) -> Result<Option<Language>> {
    let data: String =
        v.connection()
            .query_row("SELECT data FROM invoice_issued WHERE id = ?1", [id], |r| {
                r.get(0)
            })?;
    let s: Stored = serde_json::from_str(&data).map_err(|e| AppError::Other(e.to_string()))?;
    if s.language.is_some() {
        return Ok(s.language);
    }
    let Some(r) = &s.invoice.recipient else {
        return Ok(None);
    };
    if let Some(l) = crate::contacts::find(v, &r.id)?.and_then(|c| c.language) {
        return Ok(Some(l));
    }
    Ok(matches!(r.id, PartyId::Foreign { .. }).then_some(Language::En))
}

/// Factura emitida en moneda extranjera: tipo aplicado y líneas y totales en la moneda.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredFx {
    pub currency: ForeignCurrency,
    /// Líneas tal como se facturaron, en la moneda.
    pub lines: Vec<InvoiceLine>,
    /// Totales en la moneda (los del PDF).
    pub totals: Totals,
    /// Euros contabilizados en la 438 (con el signo de la base) si la factura lleva anticipos:
    /// un anticipo descontado se cancela por los euros con que se contabilizó, no al tipo de la
    /// final ([`advance_booked`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advance_eur: Option<Money>,
}

/// La moneda extranjera de una factura emitida (`None` si es en euros).
pub(crate) fn stored_fx(conn: &Connection, id: i64) -> Result<Option<StoredFx>> {
    let data: String =
        conn.query_row("SELECT data FROM invoice_issued WHERE id = ?1", [id], |r| {
            r.get(0)
        })?;
    let s: Stored = serde_json::from_str(&data).map_err(|e| AppError::Other(e.to_string()))?;
    Ok(s.fx)
}

/// Factura emitida y sus totales, tal como se guardaron al emitirla.
pub(crate) fn stored_invoice(
    conn: &taxpain_vault::rusqlite::Connection,
    id: i64,
) -> Result<(Invoice, Totals)> {
    let data: String =
        conn.query_row("SELECT data FROM invoice_issued WHERE id = ?1", [id], |r| {
            r.get(0)
        })?;
    let s: Stored = serde_json::from_str(&data).map_err(|e| AppError::Other(e.to_string()))?;
    Ok((s.invoice, s.totals))
}

pub fn now_str(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

pub fn settings(v: &Vault) -> Result<Option<InvoicingSettings>> {
    Ok(v.setting(SETTINGS_KEY)?)
}

pub(crate) fn require_settings(v: &Vault) -> Result<InvoicingSettings> {
    settings(v)?.ok_or(AppError::NotConfigured)
}

/// Guarda los ajustes y crea las series que falten.
pub fn save_settings(v: &Vault, s: &InvoicingSettings, now: OffsetDateTime) -> Result<()> {
    if let PartyId::Nif(n) = &s.issuer.id {
        taxpain_core::Nif::parse(n)
            .map_err(|_| AppError::Other("NIF del emisor no válido".into()))?;
    } else {
        return Err(AppError::Other("el emisor debe tener NIF español".into()));
    }
    if s.simplified_series.trim().is_empty() {
        return Err(AppError::Other(
            "la serie de facturas simplificadas no puede estar vacía".into(),
        ));
    }
    if s.simplified_rectifying_series.trim().is_empty() {
        return Err(AppError::Other(
            "la serie de rectificativas de simplificadas no puede estar vacía".into(),
        ));
    }
    let codes = [
        &s.default_series,
        &s.rectifying_series,
        &s.simplified_series,
        &s.simplified_rectifying_series,
    ];
    if codes
        .iter()
        .any(|c| codes.iter().filter(|d| d == &c).count() > 1)
    {
        return Err(AppError::Other(
            "las series de facturas, de rectificativas, de simplificadas y de rectificativas de \
             simplificadas deben ser distintas (RD 1619/2012 arts. 6.1.a y 7.1.a)"
                .into(),
        ));
    }
    v.set_setting(SETTINGS_KEY, s)?;
    crate::companies::adopt_issuer(v, &s.issuer, now)?;
    let conn = v.connection();
    ensure_series(conn, &s.default_series, SeriesKind::Completa, now)?;
    ensure_series(conn, &s.rectifying_series, SeriesKind::Rectificativa, now)?;
    ensure_series(conn, &s.simplified_series, SeriesKind::Simplificada, now)?;
    ensure_series(
        conn,
        &s.simplified_rectifying_series,
        SeriesKind::RectificativaSimplificada,
        now,
    )?;
    audit::append(
        conn,
        &now_str(now),
        Actor::User,
        "invoicing.settings",
        "invoicing",
        &serde_json::json!({}),
    )?;
    Ok(())
}

/// Qué facturas admite una serie (RD 1619/2012 arts. 6.1.a y 7.1.a: cada clase en la suya).
/// Las rectificativas de completas van en serie específica obligatoria (art. 6.1.a 2.º); las
/// simplificadas y sus rectificativas (R5) tienen que ir separadas de las completas (art. 7.1.a
/// último párrafo) y, entre sí, en series distintas (art. 7.1.a 4.º; texto consolidado del BOE,
/// RD 1619/2012, comprobado el 2026-10-27).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SeriesKind {
    Completa,
    Rectificativa,
    Simplificada,
    /// Rectificativas de facturas simplificadas (R5).
    RectificativaSimplificada,
}

impl SeriesKind {
    fn of(kind: InvoiceKind) -> SeriesKind {
        match kind {
            InvoiceKind::Rectificativa(Rectification { code: 5, .. }) => {
                SeriesKind::RectificativaSimplificada
            }
            InvoiceKind::Rectificativa(_) => SeriesKind::Rectificativa,
            InvoiceKind::Simplificada => SeriesKind::Simplificada,
            _ => SeriesKind::Completa,
        }
    }

    fn of_series(s: &taxpain_core::invoice::Series) -> SeriesKind {
        match (s.rectificativa, s.simplificada) {
            (true, true) => SeriesKind::RectificativaSimplificada,
            (true, false) => SeriesKind::Rectificativa,
            (false, true) => SeriesKind::Simplificada,
            (false, false) => SeriesKind::Completa,
        }
    }

    fn describe(self) -> &'static str {
        match self {
            SeriesKind::Completa => "facturas completas",
            SeriesKind::Rectificativa => "rectificativas",
            SeriesKind::Simplificada => "facturas simplificadas",
            SeriesKind::RectificativaSimplificada => "rectificativas de simplificadas",
        }
    }
}

/// Qué facturas admite la serie `s`, en palabras («rectificativas»…).
pub(crate) fn series_kind_label(s: &taxpain_core::invoice::Series) -> &'static str {
    SeriesKind::of_series(s).describe()
}

fn ensure_series(
    conn: &Connection,
    code: &str,
    kind: SeriesKind,
    now: OffsetDateTime,
) -> Result<()> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT id FROM invoice_series WHERE code = ?1",
            [code],
            |r| r.get(0),
        )
        .optional()?;
    if exists.is_none() {
        let s = taxpain_core::invoice::Series {
            code: code.into(),
            prefix: code.into(),
            rectificativa: matches!(
                kind,
                SeriesKind::Rectificativa | SeriesKind::RectificativaSimplificada
            ),
            simplificada: matches!(
                kind,
                SeriesKind::Simplificada | SeriesKind::RectificativaSimplificada
            ),
            ..taxpain_core::invoice::Series::ordinaria()
        };
        conn.execute(
            "INSERT INTO invoice_series(code, data, created_at) VALUES (?1, ?2, ?3)",
            params![
                code,
                serde_json::to_string(&s).map_err(|e| AppError::Other(e.to_string()))?,
                now_str(now)
            ],
        )?;
    }
    Ok(())
}

pub(crate) fn build_invoice(s: &InvoicingSettings, d: &InvoiceDraft, number: String) -> Invoice {
    Invoice {
        kind: d.kind,
        number,
        issue_date: d.issue_date,
        operation_date: d.operation_date,
        issuer: s.issuer.clone(),
        recipient: d.recipient.clone(),
        lines: d.lines.clone(),
        rectified: d.rectified.clone(),
        description: d.description.clone(),
        notes: d.notes.clone(),
    }
}

pub(crate) fn pdf_options(
    s: &InvoicingSettings,
    language: Option<Language>,
    verifactu_mark: Option<VerifactuMark>,
) -> PdfOptions {
    PdfOptions {
        language: match language.unwrap_or(s.language) {
            Language::Es => pdf::Language::Es,
            Language::En => pdf::Language::En,
        },
        verifactu: verifactu_mark,
        footer: s.footer.clone(),
        currency: None,
    }
}

/// Factura lista para emitir o previsualizar.
pub(crate) struct Built {
    /// En euros: la que se guarda, se contabiliza, va a VERI*FACTU y a los modelos.
    pub invoice: Invoice,
    pub totals: Totals,
    /// En la moneda de la factura (la del PDF, la Facturae y la UBL). En euros, la misma.
    pub doc: Invoice,
    pub doc_totals: Totals,
    pub fx: Option<StoredFx>,
}

impl Built {
    /// Opciones del PDF con la moneda y el contravalor en euros, si los hay.
    pub(crate) fn pdf_currency(&self) -> Option<PdfCurrency> {
        self.fx.as_ref().map(|f| PdfCurrency {
            code: f.currency.code.clone(),
            eur: Some(EurEquivalent {
                rate: f.currency.rate,
                rate_date: f.currency.rate_date,
                source: f.currency.source,
                totals: self.totals.clone(),
            }),
        })
    }
}

/// Identificación del destinatario tal como va en la factura: el NIF sin guiones ni «ES» y el
/// NIF-IVA con el prefijo de su país («FR…», «EL…» para Grecia), como lo exigen el art. 6.1.d
/// RD 1619/2012, VERI*FACTU (IDType 02), el 349 y la UBL. Sólo tiene NIF-IVA quien es de otro
/// Estado miembro de la UE.
/// `TODO(VERIFY)`: que VERI*FACTU acepte «XI» (Irlanda del Norte) como `CodigoPais`.
fn normalize_recipient_id(id: &PartyId) -> Result<PartyId> {
    Ok(match id {
        PartyId::Nif(n) => match taxpain_core::Nif::parse(&crate::contacts::key("ES", n)) {
            Ok(nif) => PartyId::Nif(nif.as_str().to_owned()),
            // Lo rechaza la validación de la factura con su mensaje.
            Err(_) => id.clone(),
        },
        PartyId::Foreign {
            country,
            id_type: IdType::NifIva,
            id: vat,
        } => {
            let country = country.trim().to_uppercase();
            if !is_other_member_state(&country, true) {
                return Err(AppError::Other(format!(
                    "«{country}» no es otro Estado miembro de la UE: el cliente no tiene NIF-IVA \
                     intracomunitario (identifícalo con otro documento)"
                )));
            }
            if !vat.chars().any(|c| c.is_ascii_alphanumeric()) {
                return Err(AppError::Other("falta el NIF-IVA del cliente".into()));
            }
            // Con la estructura de su Estado: la AEAT la valida en el registro VERI*FACTU
            // (Validaciones v1.2.2 §3.1.3.13) y en el 349, ya con la factura emitida.
            let id = crate::contacts::key(&country, vat);
            taxpain_core::eu::check_vat_id_structure(&id)
                .map_err(|e| AppError::Other(format!("NIF-IVA del cliente: {e}")))?;
            PartyId::Foreign {
                id,
                country,
                id_type: IdType::NifIva,
            }
        }
        other => other.clone(),
    })
}

/// Borrador listo para emitir: destinatario normalizado ([`normalize_recipient_id`]) y, si hay
/// una entrega intracomunitaria exenta (E5, art. 25 LIVA), un destinatario de otro Estado
/// miembro identificado con su NIF-IVA (si no, ni es exenta ni va al 349).
fn checked_draft(d: &InvoiceDraft) -> Result<InvoiceDraft> {
    let mut d = d.clone();
    if let Some(r) = &mut d.recipient {
        r.id = normalize_recipient_id(&r.id)?;
    }
    let eu_vat_id = matches!(
        d.recipient.as_ref().map(|r| &r.id),
        Some(PartyId::Foreign {
            country,
            id_type: IdType::NifIva,
            ..
        }) if is_other_member_state(country, true)
    );
    if let Some(i) = d
        .lines
        .iter()
        .position(|l| l.vat == VatTreatment::Exempt(Exemption::E5))
        && !eu_vat_id
    {
        return Err(AppError::Other(format!(
            "línea {}: una entrega intracomunitaria exenta (art. 25 LIVA) sólo se factura a una \
             empresa de otro país de la UE identificada con su NIF-IVA",
            i + 1
        )));
    }
    if let Some(o) = &d.oss {
        crate::oss::comprobar_borrador(&d, o)?;
    }
    Ok(d)
}

/// Factura en euros (y en su moneda) de un borrador, validada.
pub(crate) fn build(s: &InvoicingSettings, d: &InvoiceDraft, number: String) -> Result<Built> {
    let d = &checked_draft(d)?;
    let doc = build_invoice(s, d, number);
    let mut doc_totals = compute_totals(&doc.lines, s.rounding);
    // Ventanilla única: el IVA del Estado miembro de consumo, aparte del español (sólo en euros:
    // `comprobar_borrador` rechaza la moneda extranjera).
    if let (Some(o), Some(r)) = (&d.oss, &d.recipient)
        && let PartyId::Foreign { country, .. } = &r.id
    {
        taxpain_core::invoice::oss::apply(&mut doc_totals, country, o.supply, o.rate);
    }
    let Some(currency) = d.currency.clone() else {
        doc.validate(doc_totals.invoice_total, s.simplified_limit())?;
        return Ok(Built {
            invoice: doc.clone(),
            totals: doc_totals.clone(),
            doc,
            doc_totals,
            fx: None,
        });
    };
    currency
        .validate()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let c = fx::convert(&doc.lines, currency.rate);
    // Límites en euros (p. ej. los 400 € de la simplificada); las líneas, las de la factura.
    doc.validate(c.totals.invoice_total, s.simplified_limit())?;
    let invoice = Invoice {
        lines: c.lines,
        ..doc.clone()
    };
    Ok(Built {
        invoice,
        totals: c.totals,
        fx: Some(StoredFx {
            currency,
            lines: doc.lines.clone(),
            totals: doc_totals.clone(),
            advance_eur: None,
        }),
        doc,
        doc_totals,
    })
}

/// Totales y PDF de borrador (número «BORRADOR», sin registro ni QR). `now` decide el «hoy»
/// (peninsular) con el que se rechaza una fecha de expedición futura, como al emitir.
pub fn preview(v: &Vault, d: &InvoiceDraft, now: OffsetDateTime) -> Result<(Totals, Vec<u8>)> {
    preview_in(v, d, None, now)
}

/// [`preview`] con el PDF en `language` (si no, el idioma de los ajustes). Los totales son los
/// de la moneda de la factura.
pub fn preview_in(
    v: &Vault,
    d: &InvoiceDraft,
    language: Option<Language>,
    now: OffsetDateTime,
) -> Result<(Totals, Vec<u8>)> {
    let p = preview_full(v, d, language, now)?;
    Ok((p.totals, p.pdf))
}

/// Las fechas de la factura antes de emitirla (y en la vista previa, para que el aviso llegue
/// antes de confirmar): la factura se expide al realizar la operación (RD 1619/2012 art. 11), y
/// VERI*FACTU rechaza una fecha de expedición futura y una de operación posterior a la de
/// expedición (Validaciones v1.2.2 §3.1.3.1; la app no usa las claves de régimen 14 y 15).
fn check_dates(d: &InvoiceDraft, now: OffsetDateTime) -> Result<()> {
    let today = taxpain_core::spain_time::peninsular_date(now);
    if d.issue_date > today {
        return Err(AppError::Other(format!(
            "la fecha de la factura ({}) no puede ser posterior a hoy ({today}): la factura se \
             expide al realizar la operación",
            d.issue_date
        )));
    }
    if let Some(od) = d.operation_date
        && od > d.issue_date
    {
        return Err(AppError::Other(format!(
            "la fecha de la operación ({od}) no puede ser posterior a la de la factura ({}): la \
             factura se expide al realizar la operación o después",
            d.issue_date
        )));
    }
    Ok(())
}

/// Borrador: totales en la moneda de la factura, en euros (si es en otra moneda) y PDF.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DraftPreview {
    pub totals: Totals,
    pub eur_totals: Option<Totals>,
    pub pdf: Vec<u8>,
}

/// [`preview_in`] con los totales en euros de una factura en moneda extranjera.
pub fn preview_full(
    v: &Vault,
    d: &InvoiceDraft,
    language: Option<Language>,
    now: OffsetDateTime,
) -> Result<DraftPreview> {
    check_dates(d, now)?;
    let s = require_settings(v)?;
    let b = build(&s, d, "BORRADOR".into())?;
    // Lo que se comprobará al emitir (motivo, facturas rectificadas…), ya en el borrador.
    crate::rectificativas_emitidas::comprobar(v.connection(), d, &b.totals)?;
    // Un anticipo en moneda extranjera que no se puede descontar, ya en el borrador.
    advance_booked(v.connection(), &b)?;
    let mut opts = pdf_options(&s, language, None);
    opts.currency = b.pdf_currency();
    let pdf = pdf::render(&b.doc, &b.doc_totals, &opts).map_err(AppError::Other)?;
    Ok(DraftPreview {
        eur_totals: b.fx.is_some().then(|| b.totals.clone()),
        totals: b.doc_totals,
        pdf,
    })
}

/// El tipo de cambio de una factura que se emite es el vigente al devengo (art. 79.Once LIVA):
/// el de la fecha de la operación o, si no se indica, el de la expedición.
/// `TODO(VERIFY)`: qué tipo aplica a una rectificativa de una factura en moneda extranjera (el
/// de la original o el del día): aquí sólo se exige que no sea posterior a su expedición.
fn check_rate(d: &InvoiceDraft) -> Result<()> {
    let Some(c) = &d.currency else {
        return Ok(());
    };
    let r = if matches!(d.kind, InvoiceKind::Rectificativa(_)) {
        if c.rate_date > d.issue_date {
            Err(fx::FxError::RateAfterAccrual {
                rate_date: c.rate_date,
                accrual: d.issue_date,
            })
        } else {
            Ok(())
        }
    } else {
        c.check_rate_date(d.operation_date.unwrap_or(d.issue_date))
    };
    r.map_err(|e| AppError::Other(e.to_string()))
}

/// `a × b / c` redondeado al céntimo (mitad hacia fuera).
fn mul_div(a: i64, b: i64, c: i64) -> i64 {
    let num = i128::from(a) * i128::from(b);
    let den = i128::from(c);
    if den == 0 {
        return 0;
    }
    let q = (num.abs() * 2 + den.abs()) / (den.abs() * 2);
    let q = if (num < 0) != (den < 0) { -q } else { q };
    i64::try_from(q).unwrap_or(i64::MAX)
}

/// Euros que una factura en moneda extranjera lleva a la 438 (con el signo de la base), o
/// `None` si no tiene líneas de anticipo.
///
/// Un anticipo cobrado es una partida no monetaria (NRV 11.ª PGC): queda en la 438 por los
/// euros del día en que se facturó. Al descontarlo en la final (línea de anticipo negativa) se
/// cancela por esos euros, no al tipo de la final; la base en euros de la final (sobre la que va
/// el IVA, art. 75.Dos LIVA) no cambia y la diferencia va a ventas. Los euros salen de los
/// anticipos en la misma moneda facturados al cliente y aún no descontados (su tipo medio si se
/// descuenta sólo una parte); si se descuenta más de lo que queda, no se emite.
fn advance_booked(conn: &Connection, b: &Built) -> Result<Option<Money>> {
    use taxpain_core::invoice::totals::line_base;
    let recipient = b.invoice.recipient.as_ref().map(|p| match &p.id {
        PartyId::Nif(n) => n.as_str(),
        PartyId::Foreign { id, .. } => id.as_str(),
    });
    let Some(fx) = &b.fx else {
        return Ok(None);
    };
    if !fx.lines.iter().any(|l| l.advance) {
        return Ok(None);
    }
    // Anticipos nuevos de esta factura: al tipo de la factura.
    let new_eur: Money = b
        .invoice
        .lines
        .iter()
        .filter(|l| l.advance && !line_base(l).is_negative())
        .map(line_base)
        .sum();
    let deducted: Money = -fx
        .lines
        .iter()
        .filter(|l| l.advance && line_base(l).is_negative())
        .map(line_base)
        .sum::<Money>();
    if deducted.is_zero() {
        return Ok(Some(new_eur));
    }
    let code = &fx.currency.code;
    let Some(recipient) = recipient else {
        return Err(AppError::Other(format!(
            "para descontar un anticipo en {code} la factura necesita el cliente (el del anticipo)"
        )));
    };
    // Anticipos en la moneda pendientes de descontar al cliente y sus euros en la 438.
    let mut stmt = conn.prepare(
        "SELECT data FROM invoice_issued WHERE recipient_tax_id = ?1 AND status <> 'anulada'",
    )?;
    let rows = stmt.query_map([recipient], |r| r.get::<_, String>(0))?;
    let (mut pending, mut pending_eur) = (Money::ZERO, Money::ZERO);
    for data in rows {
        let s: Stored = serde_json::from_str(&data?).map_err(|e| AppError::Other(e.to_string()))?;
        let Some(f) = s.fx.filter(|f| f.currency.code == *code) else {
            continue;
        };
        pending += f
            .lines
            .iter()
            .filter(|l| l.advance)
            .map(line_base)
            .sum::<Money>();
        pending_eur += f.advance_eur.unwrap_or_else(|| {
            s.invoice
                .lines
                .iter()
                .filter(|l| l.advance)
                .map(line_base)
                .sum()
        });
    }
    if deducted > pending {
        return Err(AppError::Other(format!(
            "se descuentan {deducted} {code} de anticipos y a este cliente le quedan {pending} \
             {code} de anticipos facturados en {code} sin descontar: el anticipo se cancela por \
             los euros con que se contabilizó, así que tiene que estar facturado en la misma \
             moneda (revisa la línea de anticipo o emite antes la factura del anticipo)",
            deducted = deducted.to_string().trim_end_matches(" €"),
            pending = pending.to_string().trim_end_matches(" €"),
        )));
    }
    let historical = if deducted == pending {
        pending_eur
    } else {
        Money::from_cents(mul_div(
            pending_eur.cents(),
            deducted.cents(),
            pending.cents(),
        ))
    };
    Ok(Some(new_eur - historical))
}

fn last_link(conn: &Connection, emisor: &str) -> Result<Option<ChainLink>> {
    Ok(conn
        .query_row(
            "SELECT emisor_nif, num_serie, fecha, huella FROM verifactu_record
             WHERE emisor_nif = ?1 ORDER BY id DESC LIMIT 1",
            [emisor],
            |r| {
                Ok(ChainLink {
                    emisor_nif: r.get(0)?,
                    num_serie: r.get(1)?,
                    fecha: r.get(2)?,
                    huella: r.get(3)?,
                })
            },
        )
        .optional()?)
}

pub(crate) fn store_document(
    conn: &Connection,
    kind: &str,
    filename: &str,
    mime: &str,
    bytes: &[u8],
    now: OffsetDateTime,
) -> Result<i64> {
    let sha = hex::encode(Sha256::digest(bytes));
    let uuid = format!("{kind}-{}", &sha[..32]);
    conn.execute(
        "INSERT INTO document(uuid, kind, filename, mime, sha256, size, content, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            uuid,
            kind,
            filename,
            mime,
            sha,
            i64::try_from(bytes.len()).unwrap_or(i64::MAX),
            bytes,
            now_str(now)
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Nombre de fichero seguro a partir del número de factura.
fn safe_name(number: &str) -> String {
    number
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IssuedInvoice {
    pub id: i64,
    pub number: String,
    pub totals: Totals,
    pub verifactu_huella: Option<String>,
    pub qr_url: Option<String>,
    pub exported_to: Option<PathBuf>,
    pub export_error: Option<String>,
}

/// Emite una factura definitiva. Ver la documentación del módulo.
pub fn issue(
    v: &Vault,
    d: &InvoiceDraft,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<IssuedInvoice> {
    issue_in(v, d, None, now, confirmed)
}

/// [`issue`] con el PDF en `language` (si no, el idioma de los ajustes): p. ej. en inglés para
/// un cliente de fuera.
pub fn issue_in(
    v: &Vault,
    d: &InvoiceDraft,
    language: Option<Language>,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<IssuedInvoice> {
    issue_linked(v, d, language, now, confirmed, |_, _| Ok(()))
}

/// [`issue_in`] que, dentro de la misma transacción y con el id de la factura, hace `link` (p.
/// ej. dejar facturado el presupuesto del que sale): si `link` falla, no se emite nada.
pub(crate) fn issue_linked(
    v: &Vault,
    d: &InvoiceDraft,
    language: Option<Language>,
    now: OffsetDateTime,
    confirmed: bool,
    link: impl FnOnce(&Connection, i64) -> Result<()>,
) -> Result<IssuedInvoice> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    crate::backup::ensure_can_issue(v)?;
    check_rate(d)?;
    check_dates(d, now)?;
    crate::oss::exigir_tipo_confirmado(v, d)?;
    crate::periodos::exigir_abierto(
        v.connection(),
        d.issue_date,
        &format!("emitir una factura con fecha {}", d.issue_date),
        now,
    )?;
    crate::fx::check_bce(v, d.currency.as_ref())?;
    let s = require_settings(v)?;
    let PartyId::Nif(emisor) = s.issuer.id.clone() else {
        return Err(AppError::Other("el emisor debe tener NIF".into()));
    };
    // Cada clase de factura en su serie (RD 1619/2012 art. 6.1.a 2.º y art. 7.1.a: las
    // rectificativas en serie específica; las simplificadas, y sus rectificativas R5, separadas
    // de las completas).
    let wanted = SeriesKind::of(d.kind);
    let configured = match wanted {
        SeriesKind::Rectificativa => &s.rectifying_series,
        SeriesKind::Simplificada => &s.simplified_series,
        SeriesKind::RectificativaSimplificada => &s.simplified_rectifying_series,
        SeriesKind::Completa => &s.default_series,
    };
    let series_code = d.series.clone().unwrap_or_else(|| configured.clone());
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    // Los ajustes guardados antes de existir la serie de simplificadas no la tienen creada.
    if &series_code == configured {
        ensure_series(&tx, &series_code, wanted, now)?;
    }
    let (series_id, series_json): (i64, String) = tx
        .query_row(
            "SELECT id, data FROM invoice_series WHERE code = ?1",
            [&series_code],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::UnknownSeries(series_code.clone()))?;
    let series: taxpain_core::invoice::Series =
        serde_json::from_str(&series_json).map_err(|e| AppError::Other(e.to_string()))?;
    let actual = SeriesKind::of_series(&series);
    if actual != wanted {
        // Ajustes antiguos con la serie ordinaria llamada igual que la de simplificadas («S»).
        let hint = if &series_code == configured {
            ". Pon un código distinto para cada serie en Ajustes de facturación"
        } else {
            ""
        };
        return Err(AppError::Other(format!(
            "la serie {series_code} es de {} y esta factura es de {}: cada clase va en su serie \
             (RD 1619/2012 arts. 6.1.a y 7.1.a){hint}",
            actual.describe(),
            wanted.describe()
        )));
    }
    // Numeración correlativa: no se puede emitir con fecha anterior a la última de la serie.
    let last_date: Option<String> = tx
        .query_row(
            "SELECT max(issue_date) FROM invoice_issued WHERE series_id = ?1",
            [series_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    if let Some(last) = last_date
        && d.issue_date.to_string() < last
    {
        return Err(AppError::Other(format!(
            "la última factura de la serie {series_code} es del {last}: la nueva no puede tener \
             una fecha anterior (la numeración debe ser correlativa)"
        )));
    }
    let year = if series.yearly_reset {
        d.issue_date.year()
    } else {
        0
    };
    let last: Option<i64> = tx
        .query_row(
            "SELECT last_seq FROM invoice_counter WHERE series_id = ?1 AND year = ?2",
            params![series_id, year],
            |r| r.get(0),
        )
        .optional()?;
    let seq = last.unwrap_or(0) + 1;
    let number = series.format_number(d.issue_date.year(), u32::try_from(seq).unwrap_or(u32::MAX));

    let mut built = build(&s, d, number.clone())?;
    // Motivo, tipo y facturas rectificadas (por sustitución, lo que sustituye; ver
    // `rectificativas_emitidas`).
    let rect = crate::rectificativas_emitidas::comprobar(&tx, d, &built.totals)?;
    let advance_eur = advance_booked(&tx, &built)?;
    if let Some(f) = built.fx.as_mut() {
        f.advance_eur = advance_eur;
    }
    let invoice = built.invoice.clone();
    let totals = built.totals.clone();

    // VERI*FACTU: registro encadenado y, sólo cuando la factura es verificable de verdad, QR y
    // leyenda (en todas las facturas, en cuanto se remite a producción: ver
    // `verifactu_cola::remite_a_produccion`). La Orden HAC/1177/2024 art. 20.1.b reserva la frase «Factura verificable en la
    // sede electrónica de la AEAT» a las facturas de un SIF VERI*FACTU, y hoy TaxPain sólo
    // envía al entorno de pruebas: fuera de la empresa del banco de pruebas, una factura real no
    // lleva ni el QR (que apuntaría a prewww2.aeat.es) ni la leyenda hasta que exista el envío a
    // producción (`production`).
    let mut record: Option<Record> = None;
    let mut qr: Option<String> = None;
    if s.verifactu.enabled {
        let sif = SistemaInformatico::taxpain(&s.verifactu.installation_id);
        let prev = last_link(&tx, &emisor)?;
        let opts = AltaOptions {
            importe_rectificacion: rect.sustituido.map(|x| (x.base, x.vat)),
            recargo_rectificado: rect.sustituido.map(|x| x.surcharge),
            ..AltaOptions::default()
        };
        record = Some(verifactu::alta(
            &invoice,
            &totals,
            &sif,
            prev.as_ref(),
            now,
            &opts,
        )?);
        let env = if crate::verifactu_cola::remite_a_produccion(&s) {
            Some(QrEnv::Produccion)
        } else if crate::verifactu_lab::info(v)?.is_some() {
            Some(QrEnv::Pruebas)
        } else {
            None
        };
        qr = env.map(|env| {
            verifactu::qr_url(
                env,
                &emisor,
                &number,
                invoice.issue_date,
                totals.invoice_total,
            )
        });
    }
    let mark = qr.clone().map(|u| VerifactuMark {
        qr_url: u,
        legend: verifactu::LEGEND.into(),
    });
    let mut pdf_opts = pdf_options(&s, language, mark);
    pdf_opts.currency = built.pdf_currency();
    let pdf_bytes =
        pdf::render(&built.doc, &built.doc_totals, &pdf_opts).map_err(AppError::Other)?;
    let file = safe_name(&number);
    let pdf_id = store_document(
        &tx,
        "factura_pdf",
        &format!("{file}.pdf"),
        "application/pdf",
        &pdf_bytes,
        now,
    )?;
    // Facturae es una salida opcional: si no se puede generar (datos que su esquema exige y la
    // factura no tiene), la factura se emite igual y se avisa. La emisión es el acto legal.
    let mut facturae_error = None;
    // Con IVA de otro Estado miembro (OSS) la Facturae y la UBL no cuadrarían: sus impuestos son
    // los españoles. `TODO(VERIFY)`: cómo se declara el IVA de destino en Facturae 3.2.2 y UBL.
    let oss = totals.oss.is_some();
    if oss && s.outputs.facturae {
        facturae_error = Some(
            "no se generó la Facturae: TaxPain no la genera con el IVA de otro Estado miembro \
             (ventanilla única)"
                .into(),
        );
    }
    let facturae = if s.outputs.facturae
        && !oss
        && invoice
            .recipient
            .as_ref()
            .is_some_and(|r| r.address.is_some())
    {
        match facturae_xml(
            &built.doc,
            &built.doc_totals,
            &invoice,
            &totals,
            built.fx.as_ref(),
        ) {
            Ok(xml) => {
                let id = store_document(
                    &tx,
                    "facturae",
                    &format!("{file}.xml"),
                    "application/xml",
                    xml.as_bytes(),
                    now,
                )?;
                Some((id, xml))
            }
            Err(e) => {
                facturae_error = Some(format!("no se generó la Facturae: {e}"));
                None
            }
        }
    } else {
        None
    };

    // Factura electrónica UBL (opcional, como la Facturae: si falla, se avisa).
    let mut ubl_error = None;
    if oss && s.outputs.ubl {
        ubl_error = Some(
            "no se generó la factura UBL: TaxPain no la genera con el IVA de otro Estado miembro \
             (ventanilla única)"
                .into(),
        );
    }
    let ubl = if s.outputs.ubl && !oss {
        let opts = taxpain_invoicing::ubl::UblOptions {
            verifactu_qr: qr.clone(),
            withholding_key: None,
            currency: built
                .fx
                .as_ref()
                .map(|f| taxpain_invoicing::ubl::UblCurrency {
                    code: f.currency.code.clone(),
                    rate: f.currency.rate,
                    rate_date: f.currency.rate_date,
                    eur_tax_total: totals.tax_total,
                }),
        };
        match taxpain_invoicing::ubl::to_ubl(&built.doc, &built.doc_totals, &opts) {
            Ok(xml) => {
                let id = store_document(
                    &tx,
                    "ubl",
                    &format!("{file}.ubl.xml"),
                    "application/xml",
                    xml.as_bytes(),
                    now,
                )?;
                Some((id, xml))
            }
            Err(e) => {
                ubl_error = Some(format!("no se generó la factura UBL: {e}"));
                None
            }
        }
    } else {
        None
    };

    let rectifies_id: Option<i64> = match d.rectified.first() {
        Some((num, _)) => tx
            .query_row(
                "SELECT id FROM invoice_issued WHERE number = ?1",
                [num],
                |r| r.get(0),
            )
            .optional()?,
        None => None,
    };
    let stored = serde_json::to_string(&Stored {
        invoice: invoice.clone(),
        totals: totals.clone(),
        fx: built.fx.clone(),
        language: Some(language.unwrap_or(s.language)),
    })
    .map_err(|e| AppError::Other(e.to_string()))?;
    let (rname, rtax) = match &invoice.recipient {
        Some(p) => (
            Some(p.name.clone()),
            Some(match &p.id {
                PartyId::Nif(n) => n.clone(),
                PartyId::Foreign { id, .. } => id.clone(),
            }),
        ),
        None => (None, None),
    };
    tx.execute(
        "INSERT INTO invoice_issued(series_id, year, seq, number, issue_date, kind, recipient_name,
           recipient_tax_id, base, vat, surcharge, withholding, total, payable, status, rectifies_id,
           data, pdf_document_id, facturae_document_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 'emitida', ?15, ?16, ?17, ?18, ?19)",
        params![
            series_id,
            year,
            seq,
            number,
            invoice.issue_date.to_string(),
            invoice.kind.code(),
            rname,
            rtax,
            totals.base.cents(),
            totals.vat.cents(),
            totals.surcharge.cents(),
            totals.withholding.cents(),
            totals.invoice_total.cents(),
            totals.payable.cents(),
            rectifies_id,
            stored,
            pdf_id,
            facturae.as_ref().map(|f| f.0),
            now_str(now),
        ],
    )?;
    let invoice_id = tx.last_insert_rowid();
    if let Some((doc, _)) = &ubl {
        tx.execute(
            "UPDATE invoice_issued SET ubl_document_id = ?1 WHERE id = ?2",
            params![doc, invoice_id],
        )?;
    }
    tx.execute(
        "INSERT INTO invoice_counter(series_id, year, last_seq) VALUES (?1, ?2, ?3)
         ON CONFLICT(series_id, year) DO UPDATE SET last_seq = excluded.last_seq",
        params![series_id, year, seq],
    )?;
    crate::rectificativas_emitidas::guardar(&tx, invoice_id, invoice.kind, &rect)?;
    crate::accounting::post_issued_invoice(&tx, invoice_id, &invoice, &totals, advance_eur, now)?;
    if let Some(r) = &record {
        let prev = last_link(&tx, &emisor)?.map(|l| l.huella);
        tx.execute(
            "INSERT INTO verifactu_record(invoice_id, record_type, emisor_nif, num_serie, fecha, huella,
               prev_huella, generated_at, xml, qr_url)
             VALUES (?1, 'alta', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![invoice_id, emisor, number, r.link.fecha, r.huella, prev, verifactu::timestamp(now), r.xml, qr],
        )?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "invoice.issue",
        &number,
        &serde_json::json!({
            "total": totals.invoice_total.cents(),
            "huella": record.as_ref().map(|r| r.huella.clone()),
            "moneda": built.fx.as_ref().map(|f| serde_json::json!({
                "code": f.currency.code,
                "rate": f.currency.rate.decimal(),
                "rate_date": f.currency.rate_date.to_string(),
                "total": f.totals.invoice_total.cents(),
            })),
        }),
    )?;
    link(&tx, invoice_id)?;
    tx.commit()?;
    tracing::info!(number = %number, "factura emitida");
    // Ficha del cliente al día (si falla, la factura ya está emitida: sólo se avisa).
    if let Some(r) = &invoice.recipient
        && let Err(e) = crate::contacts::remember_client(v, r, invoice_id, now)
    {
        tracing::warn!(error = %e, "no se pudo guardar la ficha del cliente");
    }
    // Rectificativa de la factura con que se vendió un inmovilizado: su diferencia pasa al
    // resultado de la venta (si falla, la factura ya está emitida: sólo se avisa).
    let sale_note = if rectifies_id.is_some() {
        match crate::venta_inmovilizado::link_rectifying_sale(v, invoice_id, now) {
            Ok(note) => note,
            Err(e) => {
                tracing::warn!(error = %e, "no se pudo pasar la rectificativa a la venta");
                Some(format!(
                    "no se pudo pasar a la venta del inmovilizado ({e}): hazlo en Contabilidad › \
                     Inmovilizado"
                ))
            }
        }
    } else {
        None
    };

    // Exportación a la carpeta destino (fuera de la transacción).
    let (exported_to, export_error) = match &s.destination {
        Some(dest) => match export(
            &dest.path,
            d.issue_date.year(),
            &file,
            s.outputs.pdf.then_some(&pdf_bytes[..]),
            facturae.as_ref().map(|f| f.1.as_bytes()),
            ubl.as_ref().map(|u| u.1.as_bytes()),
        ) {
            Ok(p) => {
                conn.execute(
                    "UPDATE invoice_issued SET exported_path = ?1 WHERE id = ?2",
                    params![p.to_string_lossy(), invoice_id],
                )?;
                (Some(p), None)
            }
            Err(e) => (None, Some(e.to_string())),
        },
        None => (None, None),
    };
    let warnings: Vec<String> = [export_error, facturae_error, ubl_error, sale_note]
        .into_iter()
        .flatten()
        .collect();
    let export_error = (!warnings.is_empty()).then(|| warnings.join("; "));
    Ok(IssuedInvoice {
        id: invoice_id,
        number,
        totals,
        verifactu_huella: record.map(|r| r.huella),
        qr_url: qr,
        exported_to,
        export_error,
    })
}

/// Facturae de una factura: en euros o, si se emitió en otra moneda, en esa moneda con el
/// contravalor en euros (`doc` y `doc_totals` en la moneda; `invoice` y `totals` en euros).
fn facturae_xml(
    doc: &Invoice,
    doc_totals: &Totals,
    invoice: &Invoice,
    totals: &Totals,
    fx: Option<&StoredFx>,
) -> std::result::Result<String, taxpain_invoicing::facturae::FacturaeError> {
    match fx {
        None => to_facturae(invoice, totals, &CorrectionReason::default()),
        Some(f) => to_facturae_fx(
            doc,
            doc_totals,
            &CorrectionReason::default(),
            &FacturaeFx {
                code: &f.currency.code,
                rate: f.currency.rate,
                rate_date: f.currency.rate_date,
                eur_lines: &invoice.lines,
                eur_totals: totals,
            },
        ),
    }
}

fn export(
    root: &Path,
    year: i32,
    file: &str,
    pdf: Option<&[u8]>,
    xml: Option<&[u8]>,
    ubl: Option<&[u8]>,
) -> std::io::Result<PathBuf> {
    let dir = root.join(year.to_string());
    std::fs::create_dir_all(&dir)?;
    let mut main = dir.join(format!("{file}.pdf"));
    if let Some(p) = pdf {
        std::fs::write(&main, p)?;
    }
    if let Some(x) = xml {
        let path = dir.join(format!("{file}.xml"));
        std::fs::write(&path, x)?;
        if pdf.is_none() {
            main = path;
        }
    }
    if let Some(u) = ubl {
        let path = dir.join(format!("{file}.ubl.xml"));
        std::fs::write(&path, u)?;
        if pdf.is_none() && xml.is_none() {
            main = path;
        }
    }
    Ok(main)
}

/// Registro VERI*FACTU que necesita atención: rechazado o aceptado con errores, sin reenviar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RecordIssue {
    pub id: i64,
    pub invoice_id: i64,
    pub num_serie: String,
    pub record_type: String,
    pub send_status: String,
    pub error_code: Option<String>,
    pub error_text: Option<String>,
}

pub fn verifactu_issues(v: &Vault) -> Result<Vec<RecordIssue>> {
    let mut stmt = v.connection().prepare(
        "SELECT id, invoice_id, num_serie, record_type, send_status, error_code, error_text
         FROM verifactu_record r
         WHERE send_status IN ('incorrecto', 'aceptado_con_errores') AND superseded_by IS NULL
           -- Un alta de una factura ya anulada no se subsana (volvería a darla de alta).
           AND NOT (record_type = 'alta' AND EXISTS (
             SELECT 1 FROM verifactu_record a
             WHERE a.invoice_id = r.invoice_id AND a.record_type = 'anulacion'))
         ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(RecordIssue {
            id: r.get(0)?,
            invoice_id: r.get(1)?,
            num_serie: r.get(2)?,
            record_type: r.get(3)?,
            send_status: r.get(4)?,
            error_code: r.get(5)?,
            error_text: r.get(6)?,
        })
    })?;
    rows.collect::<taxpain_vault::rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// Genera el registro que **subsana** (o reenvía) uno rechazado o aceptado con errores: un
/// registro nuevo encadenado al último, con `Subsanacion`/`RechazoPrevio` según el caso
/// (docs/VERIFACTU.md §9). Queda pendiente de envío. Los datos son los de la factura emitida
/// (inmutable): si el error está en los datos, lo que procede es una rectificativa.
pub fn resubmit(v: &Vault, record_id: i64, now: OffsetDateTime, confirmed: bool) -> Result<i64> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    crate::backup::ensure_can_issue(v)?;
    let s = require_settings(v)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let (invoice_id, kind, status, subsanacion, rechazo, superseded, emisor, number, data): (
        i64,
        String,
        String,
        bool,
        Option<String>,
        Option<i64>,
        String,
        String,
        String,
    ) = tx
        .query_row(
            "SELECT r.invoice_id, r.record_type, r.send_status, r.subsanacion, r.rechazo_previo,
                    r.superseded_by, r.emisor_nif, r.num_serie, i.data
             FROM verifactu_record r JOIN invoice_issued i ON i.id = r.invoice_id
             WHERE r.id = ?1",
            [record_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                ))
            },
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    if superseded.is_some() {
        return Err(AppError::Other("ese registro ya se reenvió".into()));
    }
    let stored: Stored = serde_json::from_str(&data).map_err(|e| AppError::Other(e.to_string()))?;
    let sif = SistemaInformatico::taxpain(&s.verifactu.installation_id);
    let prev = last_link(&tx, &emisor)?;
    if kind == "alta" {
        let cancelled: i64 = tx.query_row(
            "SELECT count(*) FROM verifactu_record WHERE invoice_id = ?1 AND record_type = 'anulacion'",
            [invoice_id],
            |r| r.get(0),
        )?;
        if cancelled > 0 {
            return Err(AppError::Other(
                "la factura está anulada: su alta ya no se subsana".into(),
            ));
        }
    }
    // Por sustitución, el registro vuelve a llevar lo sustituido (ImporteRectificacion).
    let sustituido = if matches!(
        stored.invoice.kind,
        InvoiceKind::Rectificativa(r) if r.method == taxpain_core::invoice::RectificationMethod::Sustitucion
    ) {
        Some(
            crate::rectificativas_emitidas::sustituido_de(&tx, invoice_id)?.ok_or_else(|| {
                AppError::Other(
                    "esta rectificativa por sustitución no guarda lo que sustituye: no se puede \
                     subsanar desde TaxPain"
                        .into(),
                )
            })?,
        )
    } else {
        None
    };
    let (rec, sub, rp) = match (kind.as_str(), status.as_str()) {
        ("alta", "aceptado_con_errores") => {
            let opts = AltaOptions {
                subsanacion: true,
                importe_rectificacion: sustituido.map(|x| (x.base, x.vat)),
                recargo_rectificado: sustituido.map(|x| x.surcharge),
                ..AltaOptions::default()
            };
            let r = verifactu::alta(
                &stored.invoice,
                &stored.totals,
                &sif,
                prev.as_ref(),
                now,
                &opts,
            )?;
            (r, true, None)
        }
        ("alta", "incorrecto") => {
            // Si lo rechazado era la subsanación de un registro que ya existe en la AEAT: S;
            // si el registro no existe allí (alta rechazada o reenvío X rechazado): X.
            let rp = if subsanacion && rechazo.as_deref() != Some("X") {
                RechazoPrevio::S
            } else {
                RechazoPrevio::X
            };
            let opts = AltaOptions {
                subsanacion: true,
                rechazo_previo: Some(rp),
                importe_rectificacion: sustituido.map(|x| (x.base, x.vat)),
                recargo_rectificado: sustituido.map(|x| x.surcharge),
            };
            let r = verifactu::alta(
                &stored.invoice,
                &stored.totals,
                &sif,
                prev.as_ref(),
                now,
                &opts,
            )?;
            (
                r,
                true,
                Some(if rp == RechazoPrevio::S { "S" } else { "X" }),
            )
        }
        ("anulacion", "incorrecto") => {
            let r = verifactu::anulacion(
                &emisor,
                &number,
                stored.invoice.issue_date,
                &sif,
                prev.as_ref(),
                now,
                true,
            )?;
            (r, false, Some("S"))
        }
        ("anulacion", _) => {
            return Err(AppError::Other(
                "la anulación ya consta en la AEAT: no se subsana".into(),
            ));
        }
        _ => {
            return Err(AppError::Other(
                "sólo se reenvían registros rechazados o aceptados con errores".into(),
            ));
        }
    };
    tx.execute(
        "INSERT INTO verifactu_record(invoice_id, record_type, emisor_nif, num_serie, fecha, huella,
           prev_huella, generated_at, xml, subsanacion, rechazo_previo, resubmits)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            invoice_id,
            kind,
            emisor,
            number,
            rec.link.fecha,
            rec.huella,
            prev.map(|p| p.huella),
            verifactu::timestamp(now),
            rec.xml,
            sub,
            rp,
            record_id
        ],
    )?;
    let new_id = tx.last_insert_rowid();
    tx.execute(
        "UPDATE verifactu_record SET superseded_by = ?1 WHERE id = ?2",
        params![new_id, record_id],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "verifactu.resubmit",
        &number,
        &serde_json::json!({ "record": record_id, "new": new_id, "type": kind, "rechazo_previo": rp }),
    )?;
    tx.commit()?;
    Ok(new_id)
}

/// Facturas emitidas de un año, más recientes primero.
pub fn list_issued(v: &Vault, year: i32) -> Result<Vec<IssuedSummary>> {
    let conn = v.connection();
    let mut stmt = conn.prepare(
        "SELECT i.id, i.number, i.issue_date, i.kind, i.recipient_name, i.base, i.vat, i.total,
                i.payable, i.status, i.exported_path,
                (SELECT CASE WHEN r.send_status = 'pendiente' AND r.sin_linea >= ?2
                          THEN 'sin_respuesta' ELSE r.send_status END
                 FROM verifactu_record r WHERE r.invoice_id = i.id ORDER BY r.id DESC LIMIT 1),
                i.facturae_document_id IS NOT NULL, i.ubl_document_id IS NOT NULL, i.origin,
                json_extract(i.data, '$.fx.currency.code'), json_extract(i.data, '$.fx.totals.payable'),
                json_extract(i.data, '$.fx.currency'),
                (SELECT r.error_text FROM verifactu_record r WHERE r.invoice_id = i.id ORDER BY r.id DESC LIMIT 1)
         FROM invoice_issued i WHERE substr(i.issue_date, 1, 4) = ?1
         ORDER BY i.issue_date DESC, i.id DESC",
    )?;
    let rows = stmt.query_map(
        params![year.to_string(), crate::verifactu_cola::MAX_SIN_LINEA],
        |r| {
            Ok(IssuedSummary {
                id: r.get(0)?,
                number: r.get(1)?,
                issue_date: r.get(2)?,
                kind: r.get(3)?,
                recipient_name: r.get(4)?,
                base: Money::from_cents(r.get(5)?),
                vat: Money::from_cents(r.get(6)?),
                total: Money::from_cents(r.get(7)?),
                payable: Money::from_cents(r.get(8)?),
                status: r.get(9)?,
                exported_path: r.get(10)?,
                verifactu_status: r.get(11)?,
                export_error: None,
                has_facturae: r.get(12)?,
                has_ubl: r.get(13)?,
                origin: r.get(14)?,
                currency: r.get(15)?,
                currency_payable: r.get::<_, Option<i64>>(16)?.map(Money::from_cents),
                currency_rate: r
                    .get::<_, Option<String>>(17)?
                    .and_then(|j| serde_json::from_str(&j).ok()),
                verifactu_error: r.get(18)?,
            })
        },
    )?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Factura UBL guardada de una factura emitida: (nombre, MIME, bytes).
pub fn invoice_ubl(v: &Vault, invoice_id: i64) -> Result<(String, String, Vec<u8>)> {
    v.connection()
        .query_row(
            "SELECT d.filename, d.mime, d.content FROM invoice_issued i
             JOIN document d ON d.id = i.ubl_document_id WHERE i.id = ?1",
            [invoice_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)
}

/// PDF (o Facturae) guardado de una factura: (nombre, MIME, bytes).
pub fn invoice_document(
    v: &Vault,
    invoice_id: i64,
    facturae: bool,
) -> Result<(String, String, Vec<u8>)> {
    let col = if facturae {
        "facturae_document_id"
    } else {
        "pdf_document_id"
    };
    v.connection()
        .query_row(
            &format!(
                "SELECT d.filename, d.mime, d.content FROM invoice_issued i JOIN document d ON d.id = i.{col} WHERE i.id = ?1"
            ),
            [invoice_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)
}

/// Facturae de una factura emitida firmada con XAdES-EPES (política de Facturae v3.1) con el
/// certificado `identity` (ya confirmado con Touch ID). No cambia la factura guardada: devuelve
/// el nombre y el XML firmado, para guardarlo o enviarlo a FACe.
pub fn signed_facturae(
    v: &Vault,
    invoice_id: i64,
    identity: &taxpain_vault::ClientIdentity,
    now: OffsetDateTime,
) -> Result<(String, Vec<u8>)> {
    let (filename, xml) = match invoice_document(v, invoice_id, true) {
        Ok((name, _, bytes)) => (
            name,
            String::from_utf8(bytes).map_err(|e| AppError::Other(e.to_string()))?,
        ),
        Err(AppError::NotFound) => {
            let (invoice, totals) = stored_invoice(v.connection(), invoice_id)?;
            let fx = stored_fx(v.connection(), invoice_id)?;
            let doc = Invoice {
                lines: fx
                    .as_ref()
                    .map_or(invoice.lines.clone(), |f| f.lines.clone()),
                ..invoice.clone()
            };
            let doc_totals = fx.as_ref().map_or(totals.clone(), |f| f.totals.clone());
            let xml = facturae_xml(&doc, &doc_totals, &invoice, &totals, fx.as_ref())?;
            (format!("{}.xsig", invoice.number.replace('/', "-")), xml)
        }
        Err(e) => return Err(e),
    };
    let mut n = [0u32; 8];
    for x in &mut n {
        let mut b = [0u8; 4];
        getrandom::fill(&mut b).map_err(|e| AppError::Other(e.to_string()))?;
        *x = 0x1000_0000 + (u32::from_le_bytes(b) % 0x6FFF_FFFF);
    }
    let signed = taxpain_invoicing::xades::sign_facturae(
        &xml,
        &identity.chain_der(),
        now,
        &taxpain_invoicing::xades::SignatureIds::from_numbers(n),
        |data| identity.sign_rsa_sha512(data).map_err(|e| e.to_string()),
    )
    .map_err(|e| AppError::Other(format!("Facturae: {e}")))?;
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        "facturae.firma",
        &invoice_id.to_string(),
        &serde_json::json!({ "certificado": identity.fingerprint(), "sha256": hex::encode(Sha256::digest(signed.as_bytes())) }),
    )?;
    let base = filename.trim_end_matches(".xml").trim_end_matches(".xsig");
    Ok((format!("{base}.xsig"), signed.into_bytes()))
}

/// Lo que ya depende de una factura y que anularla dejaría colgando: la venta de un inmovilizado
/// que la lleva, una rectificativa que la cita, un cobro, una remesa sin devolver o un movimiento
/// del banco casado con ella.
fn consecuencias(
    conn: &taxpain_vault::rusqlite::Connection,
    invoice_id: i64,
    number: &str,
) -> Result<Option<String>> {
    // Su base ya no cuenta como ventas sino en la venta del elemento (CONTABILIDAD-08).
    if let Some(asset) = crate::venta_inmovilizado::sale_of_invoice(conn, invoice_id)? {
        return Ok(Some(format!(
            "es la venta de «{asset}» en el inmovilizado (desvincúlala en Contabilidad › \
             Inmovilizado)"
        )));
    }
    // Rectificativas sin anular que la citan (la primera va en `rectifies_id`; las demás, en su
    // lista de facturas rectificadas).
    let mut stmt = conn.prepare(
        "SELECT number, rectifies_id, data FROM invoice_issued
         WHERE id <> ?1 AND status <> 'anulada' AND (rectifies_id = ?1 OR instr(data, ?2) > 0)",
    )?;
    let candidatas = stmt
        .query_map(params![invoice_id, format!("\"{number}\"")], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (num, rect, data) in candidatas {
        let cita = rect == Some(invoice_id)
            || serde_json::from_str::<Stored>(&data)
                .is_ok_and(|s| s.invoice.rectified.iter().any(|(n, _)| n == number));
        if cita {
            return Ok(Some(format!("la rectifica {num}")));
        }
    }
    let cobros: i64 = conn.query_row(
        "SELECT count(*) FROM settlement WHERE kind = 'emitida' AND invoice_id = ?1",
        [invoice_id],
        |r| r.get(0),
    )?;
    if cobros > 0 {
        return Ok(Some("tiene cobros registrados (Cobros y pagos)".into()));
    }
    let remesas: i64 = conn.query_row(
        "SELECT count(*) FROM remittance_item
         WHERE kind = 'emitida' AND invoice_id = ?1 AND returned_on IS NULL",
        [invoice_id],
        |r| r.get(0),
    )?;
    if remesas > 0 {
        return Ok(Some("está en una remesa SEPA".into()));
    }
    let banco: i64 = conn.query_row(
        "SELECT count(*) FROM bank_tx WHERE matched_kind = 'emitida' AND matched_id = ?1",
        [invoice_id],
        |r| r.get(0),
    )?;
    if banco > 0 {
        return Ok(Some("está conciliada con un movimiento del banco".into()));
    }
    Ok(None)
}

/// Anula una factura emitida por error: registro de anulación encadenado y estado `anulada`.
///
/// Sólo mientras su periodo de IVA siga abierto (303 sin presentar y en plazo): anular la
/// borra de los modelos de ese periodo, y si ya se declaró (o venció) el IVA ingresado no se
/// recuperaría nunca y el 390, el 347 y el 349 cambiarían hacia atrás. Entonces lo que procede
/// es una factura rectificativa, que se declara en el periodo en que se expide (art. 89 LIVA;
/// art. 15 RD 1619/2012).
pub fn cancel(v: &Vault, invoice_id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    crate::backup::ensure_can_issue(v)?;
    let s = require_settings(v)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let (number, date, status, origin): (String, String, String, String) = tx
        .query_row(
            "SELECT number, issue_date, status, origin FROM invoice_issued WHERE id = ?1",
            [invoice_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    if origin == "importada" {
        return Err(AppError::Other(format!(
            "{number} viene del libro de la gestoría: se anula en el programa con el que se emitió"
        )));
    }
    if status == "anulada" {
        return Ok(());
    }
    if let Some(que) = consecuencias(&tx, invoice_id, &number)? {
        return Err(AppError::Other(format!(
            "No se puede anular {number}: {que}. Deshazlo antes o, si la operación no se hizo, \
             emite una factura rectificativa por el importe pendiente."
        )));
    }
    let issue_date = Date::parse(
        &date,
        time::macros::format_description!("[year]-[month]-[day]"),
    )
    .map_err(|e| AppError::Other(e.to_string()))?;
    let (invoice, totals) = stored_invoice(&tx, invoice_id)?;
    let accrual = crate::models::vat_accrual_date(&invoice, &totals.breakdown);
    let today = taxpain_core::spain_time::peninsular_date(now);
    // El contraasiento va en la fecha de la factura: el 303 la saca de su periodo de origen y
    // las ventas, de su ejercicio. Si ese ejercicio está cerrado o el IVA de ese trimestre ya
    // está liquidado en la contabilidad, no se anula: se rectifica (art. 15 RD 1619/2012).
    let blocked = match crate::models::vat_period_closed(v, accrual, today)? {
        Some(why) => Some(why),
        None => crate::accounting::reversal_blocked(&tx, issue_date)?,
    };
    if let Some(why) = blocked {
        return Err(AppError::Other(format!(
            "No se puede anular {number}: {why}. Emite una factura rectificativa por el importe \
             total en negativo (R1 si es un error fundado en derecho o la operación se ha \
             resuelto o anulado, art. 80 LIVA; R4 por otras causas): se declara en el periodo \
             actual (art. 89 LIVA; art. 15 RD 1619/2012)."
        )));
    }
    if s.verifactu.enabled {
        let PartyId::Nif(emisor) = &s.issuer.id else {
            return Err(AppError::Other("el emisor debe tener NIF".into()));
        };
        let sif = SistemaInformatico::taxpain(&s.verifactu.installation_id);
        let prev = last_link(&tx, emisor)?;
        let rec =
            verifactu::anulacion(emisor, &number, issue_date, &sif, prev.as_ref(), now, false)?;
        tx.execute(
            "INSERT INTO verifactu_record(invoice_id, record_type, emisor_nif, num_serie, fecha, huella,
               prev_huella, generated_at, xml)
             VALUES (?1, 'anulacion', ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![invoice_id, emisor, number, rec.link.fecha, rec.huella, prev.map(|p| p.huella), verifactu::timestamp(now), rec.xml],
        )?;
    }
    tx.execute(
        "UPDATE invoice_issued SET status = 'anulada' WHERE id = ?1",
        [invoice_id],
    )?;
    // El contraasiento, en la fecha de la factura (ver arriba): en el mismo periodo de IVA y
    // ejercicio que el asiento que anula.
    crate::accounting::reverse_issued_invoice(&tx, invoice_id, issue_date, now)?;
    // El presupuesto que se facturó con ella vuelve a «aceptado»: se puede facturar de nuevo.
    crate::quotes::unlink_invoice(&tx, invoice_id, &number, now)?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "invoice.cancel",
        &number,
        &serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

/// Entorno de la AEAT al que se envían los registros VERI*FACTU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entorno {
    /// `prewww1.aeat.es`: sin efectos. Lo que acepta queda como `pruebas`, no remitido.
    Pruebas,
    /// `www1.agenciatributaria.gob.es`: el envío real (art. 16 RD 1007/2023).
    Produccion,
}

/// Siguiente lote de registros pendientes de enviar: (ids, cuerpo `RegFactuSistemaFacturacion`).
/// A producción van también los aceptados sólo en el entorno de pruebas: la AEAT no los tiene y
/// la cadena allí debe empezar por el primero.
pub fn pending_submission(
    v: &Vault,
    limit: usize,
    entorno: Entorno,
) -> Result<Option<(Vec<i64>, String)>> {
    pending_submission_with(v, limit, entorno, false)
}

/// Como [`pending_submission`]; con `incidencia`, la cabecera del envío lleva
/// `Incidencia = S` (reenvío tras un fallo técnico: Orden HAC/1177/2024 art. 16.4). No entran
/// los registros a los que la AEAT ya contestó dos veces sin línea («sin respuesta»): se
/// consultan antes de volver a enviarlos.
pub fn pending_submission_with(
    v: &Vault,
    limit: usize,
    entorno: Entorno,
    incidencia: bool,
) -> Result<Option<(Vec<i64>, String)>> {
    crate::backup::ensure_can_issue(v)?;
    let s = require_settings(v)?;
    let PartyId::Nif(nif) = &s.issuer.id else {
        return Err(AppError::Other("el emisor debe tener NIF".into()));
    };
    let conn = v.connection();
    let statuses = match entorno {
        Entorno::Pruebas => "('pendiente')",
        Entorno::Produccion => "('pendiente', 'pruebas')",
    };
    let mut stmt = conn.prepare(&format!(
        "SELECT id, xml, huella, emisor_nif, num_serie, fecha FROM verifactu_record
         WHERE send_status IN {statuses} AND sin_linea < {max} ORDER BY id LIMIT ?1",
        max = crate::verifactu_cola::MAX_SIN_LINEA
    ))?;
    let rows: Vec<(i64, Record)> = stmt
        .query_map(
            [i64::try_from(limit.min(verifactu::MAX_RECORDS_PER_SUBMISSION)).unwrap_or(1000)],
            |r| {
                Ok((
                    r.get(0)?,
                    Record {
                        xml: r.get(1)?,
                        huella: r.get(2)?,
                        link: ChainLink {
                            emisor_nif: r.get(3)?,
                            num_serie: r.get(4)?,
                            fecha: r.get(5)?,
                            huella: r.get(2)?,
                        },
                    },
                ))
            },
        )?
        .collect::<std::result::Result<_, _>>()?;
    // Un solo registro por factura en cada envío (p. ej. alta y anulación de la misma factura
    // van en envíos sucesivos): la respuesta identifica cada línea por su número. El lote se
    // corta en el primer repetido: saltarlo y seguir mandaría los posteriores antes que él, y
    // la cadena se remite en orden de generación (Orden HAC/1177/2024 art. 16.4).
    let mut seen = std::collections::HashSet::new();
    let rows: Vec<(i64, Record)> = rows
        .into_iter()
        .take_while(|(_, r)| seen.insert(r.link.num_serie.clone()))
        .collect();
    if rows.is_empty() {
        return Ok(None);
    }
    let refs: Vec<&Record> = rows.iter().map(|(_, r)| r).collect();
    let body = verifactu::submission_with(&s.issuer.name, nif, &refs, incidencia)?;
    Ok(Some((rows.iter().map(|(id, _)| *id).collect(), body)))
}

/// Aplica la respuesta de la AEAT a los registros enviados.
pub fn apply_submission_response(
    v: &Vault,
    ids: &[i64],
    resp: &Response,
    entorno: Entorno,
    now: OffsetDateTime,
) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let mut used = vec![false; resp.lineas.len()];
    for id in ids {
        let (num, kind): (String, String) = tx.query_row(
            "SELECT num_serie, record_type FROM verifactu_record WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        // Cada línea de la respuesta se usa una vez, por número y tipo de operación
        // (`Alta` / `Anulacion`); si la AEAT no indica el tipo, sólo por número.
        let matches = |l: &taxpain_invoicing::verifactu::ResponseLine| {
            l.num_serie == num
                && (l.operacion.is_empty() || l.operacion.eq_ignore_ascii_case(&kind))
        };
        tx.execute(
            "UPDATE verifactu_record SET intentos = intentos + 1 WHERE id = ?1",
            [id],
        )?;
        let Some(i) = (0..resp.lineas.len()).find(|&i| !used[i] && matches(&resp.lineas[i])) else {
            // Sin línea para él: sigue pendiente y se reenvía una vez; a la segunda queda «sin
            // respuesta» (fuera de la cola) para consultarlo antes, no reenviarlo a ciegas.
            tx.execute(
                "UPDATE verifactu_record SET sin_linea = sin_linea + 1,
                   error_text = 'La AEAT no devolvió respuesta para este registro' WHERE id = ?1",
                [id],
            )?;
            continue;
        };
        used[i] = true;
        let line = &resp.lineas[i];
        let accepted = if entorno == Entorno::Pruebas {
            // Aceptado por el entorno de pruebas: no está remitido (sin efectos) y se vuelve a
            // enviar a producción.
            "pruebas"
        } else {
            "correcto"
        };
        // Rechazado por duplicado: la AEAT ya lo tiene (p. ej. se envió, se cortó la conexión
        // antes de la respuesta y se reenvió). Cuenta como remitido con el estado que allí
        // consta ([SW] 6.5.2; docs/VERIFACTU.md §8).
        let status = match (line.duplicado.as_deref(), line.estado.as_str()) {
            (Some("Correcta"), _) => accepted,
            (Some("AceptadaConErrores"), _) => "aceptado_con_errores",
            (Some("Anulada"), _) if kind == "anulacion" => accepted,
            (Some(_), _) => "incorrecto",
            (None, "Correcto") => accepted,
            (None, "AceptadoConErrores") => "aceptado_con_errores",
            _ => "incorrecto",
        };
        let error_text = match (line.duplicado.as_deref(), status) {
            (Some("Anulada"), "incorrecto") => Some(
                "La AEAT ya tiene esta factura como anulada: revisa si procede una rectificativa"
                    .to_owned(),
            ),
            (Some(_), "incorrecto") => line.descripcion_error.clone(),
            (Some(_), _) => None,
            _ => line.descripcion_error.clone(),
        };
        let error_code = if line.duplicado.is_some() && status != "incorrecto" {
            None
        } else {
            line.codigo_error.clone()
        };
        tx.execute(
            "UPDATE verifactu_record SET send_status = ?1, error_code = ?2, error_text = ?3, csv = ?4,
               sent_at = ?5, duplicado = ?6 WHERE id = ?7",
            params![status, error_code, error_text, resp.csv, now_str(now), line.duplicado, id],
        )?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "verifactu.submit",
        &resp.estado_envio,
        &serde_json::json!({ "records": ids.len(), "csv": resp.csv }),
    )?;
    tx.commit()?;
    Ok(())
}
