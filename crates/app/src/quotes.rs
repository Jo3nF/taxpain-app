//! Presupuestos y facturas proforma. No son facturas: no llevan VERI*FACTU, no se contabilizan
//! y no cuentan en ningún modelo. Tienen numeración propia (`P2026-0001`, `PF2026-0001`), un
//! estado (borrador, enviado, aceptado, rechazado, facturado) y se convierten en factura
//! abriendo el formulario de factura con sus datos (el usuario la revisa y la emite con
//! [`issue_invoice`], que la emite y deja el presupuesto facturado en la misma transacción). Si
//! esa factura se anula, el presupuesto vuelve a «aceptado» ([`unlink_invoice`]). Sólo se borran
//! los borradores: lo que se ha enviado conserva su número.
//!
//! En moneda extranjera (`draft.currency`) el PDF va en esa moneda, sin contravalor (no es una
//! factura), y `total` es su equivalente en euros al tipo del presupuesto, sólo para el panel.
//! Al facturarlo hay que poner el tipo del día de la operación (la emisión lo exige).

use serde::{Deserialize, Serialize};
use taxpain_core::invoice::{InvoiceKind, PartyId, compute_totals};
use taxpain_core::money::Money;
use taxpain_invoicing::pdf::{self, QuoteKind};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::{self, InvoiceDraft, IssuedInvoice, now_str};
use crate::settings::Language;

const ISO_DATE: &[time::format_description::BorrowedFormatItem<'_>] =
    time::macros::format_description!("[year]-[month]-[day]");

pub const STATUSES: &[&str] = &["borrador", "enviado", "aceptado", "rechazado", "facturado"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quote {
    pub id: i64,
    /// `presupuesto` o `proforma`.
    pub kind: String,
    pub number: String,
    pub issue_date: Date,
    pub valid_until: Option<Date>,
    pub status: String,
    pub recipient_name: Option<String>,
    /// Total en euros (en moneda extranjera, su equivalente al tipo del presupuesto).
    pub total: Money,
    /// En moneda extranjera, el total en esa moneda.
    pub currency_total: Option<Money>,
    /// Total a pagar en euros: el total menos la retención (lo que cobra la lista).
    pub payable: Money,
    /// En moneda extranjera, el total a pagar en esa moneda.
    pub currency_payable: Option<Money>,
    pub draft: InvoiceDraft,
    pub language: Option<Language>,
    /// Factura en la que se convirtió.
    pub invoice_id: Option<i64>,
    pub invoice_number: Option<String>,
    /// Presupuesto enviado y ya caducado sin respuesta.
    pub expired: bool,
}

/// Lo que llega de la interfaz al crear o cambiar un presupuesto.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct QuoteInput {
    pub kind: String,
    pub draft: InvoiceDraft,
    pub valid_until: Option<Date>,
    pub language: Option<Language>,
}

fn prefix(kind: &str) -> Result<&'static str> {
    match kind {
        "presupuesto" => Ok("P"),
        "proforma" => Ok("PF"),
        _ => Err(AppError::Other("tipo de documento no válido".into())),
    }
}

fn lang_str(l: Option<Language>) -> Option<&'static str> {
    l.map(|l| match l {
        Language::Es => "Es",
        Language::En => "En",
    })
}

fn check(v: &Vault, input: &QuoteInput) -> Result<Money> {
    prefix(&input.kind)?;
    let d = &input.draft;
    if d.lines.is_empty() {
        return Err(AppError::Other("el presupuesto no tiene líneas".into()));
    }
    if d.lines
        .iter()
        .any(|l| l.description.trim().is_empty() || l.quantity_milli == 0)
    {
        return Err(AppError::Other(
            "revisa el concepto y la cantidad de cada línea".into(),
        ));
    }
    if d.recipient
        .as_ref()
        .is_none_or(|r| r.name.trim().is_empty())
    {
        return Err(AppError::Other("pon el nombre del cliente".into()));
    }
    if let Some(until) = input.valid_until
        && until < d.issue_date
    {
        return Err(AppError::Other(
            "la validez no puede acabar antes de la fecha del presupuesto".into(),
        ));
    }
    let s = invoicing::require_settings(v)?;
    let total = compute_totals(&d.lines, s.rounding).invoice_total;
    Ok(match &d.currency {
        Some(c) => {
            c.validate().map_err(|e| AppError::Other(e.to_string()))?;
            c.to_eur(total)
        }
        None => total,
    })
}

/// Crea un presupuesto o una proforma (estado «borrador») con el número siguiente.
pub fn create(v: &Vault, input: &QuoteInput, now: OffsetDateTime) -> Result<Quote> {
    create_by(v, input, Actor::User, now)
}

/// Como [`create`], para el borrador que deja Claude por el servidor MCP: en la auditoría queda
/// como hecho por el asistente (`via: mcp`), no por el usuario.
pub fn create_mcp(v: &Vault, input: &QuoteInput, now: OffsetDateTime) -> Result<Quote> {
    create_by(v, input, Actor::Assistant, now)
}

fn create_by(v: &Vault, input: &QuoteInput, actor: Actor, now: OffsetDateTime) -> Result<Quote> {
    let total = check(v, input)?;
    let p = prefix(&input.kind)?;
    let year = input.draft.issue_date.year();
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let seq: i64 = tx.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM quote WHERE kind = ?1 AND year = ?2",
        params![input.kind, year],
        |r| r.get(0),
    )?;
    let number = format!("{p}{year}-{seq:04}");
    let json = serde_json::to_string(&input.draft).map_err(|e| AppError::Other(e.to_string()))?;
    tx.execute(
        "INSERT INTO quote(kind, number, year, seq, issue_date, valid_until, status,
           recipient_name, total, draft, language, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'borrador', ?7, ?8, ?9, ?10, ?11, ?11)",
        params![
            input.kind,
            number,
            year,
            seq,
            input.draft.issue_date.to_string(),
            input.valid_until.map(|d| d.to_string()),
            input
                .draft
                .recipient
                .as_ref()
                .map(|r| r.name.trim().to_owned()),
            total.cents(),
            json,
            lang_str(input.language),
            now_str(now),
        ],
    )?;
    let id = tx.last_insert_rowid();
    let mut details = serde_json::json!({ "total": total.cents() });
    if actor == Actor::Assistant {
        details["via"] = "mcp".into();
    }
    audit::append(&tx, &now_str(now), actor, "quote.create", &number, &details)?;
    tx.commit()?;
    get(v, id, input.draft.issue_date)
}

/// Cambia un presupuesto que aún no se ha facturado (mismo número).
pub fn update(v: &Vault, id: i64, input: &QuoteInput, now: OffsetDateTime) -> Result<Quote> {
    let q = get(v, id, input.draft.issue_date)?;
    if q.status == "facturado" {
        return Err(AppError::Other("ya está facturado: no se cambia".into()));
    }
    if q.kind != input.kind {
        return Err(AppError::Other(
            "no se puede cambiar el tipo de documento".into(),
        ));
    }
    let total = check(v, input)?;
    let json = serde_json::to_string(&input.draft).map_err(|e| AppError::Other(e.to_string()))?;
    let conn = v.connection();
    conn.execute(
        "UPDATE quote SET issue_date = ?1, valid_until = ?2, recipient_name = ?3, total = ?4,
           draft = ?5, language = ?6, updated_at = ?7 WHERE id = ?8",
        params![
            input.draft.issue_date.to_string(),
            input.valid_until.map(|d| d.to_string()),
            input
                .draft
                .recipient
                .as_ref()
                .map(|r| r.name.trim().to_owned()),
            total.cents(),
            json,
            lang_str(input.language),
            now_str(now),
            id
        ],
    )?;
    audit::append(
        conn,
        &now_str(now),
        Actor::User,
        "quote.update",
        &q.number,
        &serde_json::json!({ "total": total.cents() }),
    )?;
    get(v, id, input.draft.issue_date)
}

fn row(
    r: &taxpain_vault::rusqlite::Row<'_>,
    today: Date,
) -> taxpain_vault::rusqlite::Result<Quote> {
    let parse = |s: String| Date::parse(&s, ISO_DATE).ok();
    let draft: String = r.get(10)?;
    let issue: String = r.get(4)?;
    let valid: Option<String> = r.get(5)?;
    let lang: Option<String> = r.get(11)?;
    let status: String = r.get(6)?;
    let valid_until = valid.and_then(parse);
    Ok(Quote {
        id: r.get(0)?,
        kind: r.get(1)?,
        number: r.get(2)?,
        issue_date: parse(issue).unwrap_or(today),
        expired: matches!(status.as_str(), "borrador" | "enviado")
            && valid_until.is_some_and(|d| d < today),
        valid_until,
        status,
        recipient_name: r.get(7)?,
        total: Money::from_cents(r.get(8)?),
        currency_total: None,
        payable: Money::from_cents(r.get(8)?),
        currency_payable: None,
        draft: serde_json::from_str(&draft).map_err(|e| {
            taxpain_vault::rusqlite::Error::FromSqlConversionFailure(
                10,
                taxpain_vault::rusqlite::types::Type::Text,
                Box::new(e),
            )
        })?,
        language: match lang.as_deref() {
            Some("En") => Some(Language::En),
            Some("Es") => Some(Language::Es),
            _ => None,
        },
        invoice_id: r.get(9)?,
        invoice_number: r.get(12)?,
    })
}

const SELECT: &str = "SELECT q.id, q.kind, q.number, q.year, q.issue_date, q.valid_until, q.status,
    q.recipient_name, q.total, q.invoice_id, q.draft, q.language, i.number
    FROM quote q LEFT JOIN invoice_issued i ON i.id = q.invoice_id";

pub fn get(v: &Vault, id: i64, today: Date) -> Result<Quote> {
    let mut q = v
        .connection()
        .query_row(&format!("{SELECT} WHERE q.id = ?1"), [id], |r| {
            row(r, today)
        })
        .optional()?
        .ok_or(AppError::NotFound)?;
    with_currency_total(v, std::slice::from_mut(&mut q))?;
    Ok(q)
}

/// Total en la moneda de los presupuestos que no son en euros, y el total a pagar (sin la
/// retención) de todos.
fn with_currency_total(v: &Vault, quotes: &mut [Quote]) -> Result<()> {
    if quotes.is_empty() {
        return Ok(());
    }
    let s = invoicing::require_settings(v)?;
    for q in quotes.iter_mut() {
        let t = compute_totals(&q.draft.lines, s.rounding);
        match &q.draft.currency {
            Some(c) => {
                q.currency_total = Some(t.invoice_total);
                q.currency_payable = Some(t.payable);
                q.payable = c.to_eur(t.payable);
            }
            None => q.payable = t.payable,
        }
    }
    Ok(())
}

/// Presupuestos y proformas de `year`, los últimos primero.
pub fn list(v: &Vault, year: i32, today: Date) -> Result<Vec<Quote>> {
    let mut stmt = v.connection().prepare(&format!(
        "{SELECT} WHERE q.year = ?1 ORDER BY q.issue_date DESC, q.id DESC"
    ))?;
    let rows = stmt.query_map([year], |r| row(r, today))?;
    let mut out: Vec<Quote> = rows.collect::<std::result::Result<_, _>>()?;
    with_currency_total(v, &mut out)?;
    Ok(out)
}

/// Cambia el estado (enviado, aceptado, rechazado; «facturado» sólo al facturarlo).
pub fn set_status(v: &Vault, id: i64, status: &str, now: OffsetDateTime) -> Result<()> {
    if !STATUSES.contains(&status) || status == "facturado" {
        return Err(AppError::Other("estado no válido".into()));
    }
    let conn = v.connection();
    let (number, current): (String, String) = conn
        .query_row(
            "SELECT number, status FROM quote WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    if current == "facturado" {
        return Err(AppError::Other("ya está facturado".into()));
    }
    conn.execute(
        "UPDATE quote SET status = ?1, updated_at = ?2 WHERE id = ?3",
        params![status, now_str(now), id],
    )?;
    audit::append(
        conn,
        &now_str(now),
        Actor::User,
        "quote.status",
        &number,
        &serde_json::json!({ "status": status }),
    )?;
    Ok(())
}

pub fn delete(v: &Vault, id: i64, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let (number, status): (String, String) = conn
        .query_row(
            "SELECT number, status FROM quote WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    // Uno enviado (o aceptado, rechazado, facturado) lo tiene el cliente con su número: no se
    // borra (el número quedaría libre para otro presupuesto); se marca como rechazado.
    if status != "borrador" {
        return Err(AppError::Other(format!(
            "{number} ya no es un borrador (está {status}): se conserva con su número; si no \
             sigue adelante, márcalo como rechazado"
        )));
    }
    conn.execute("DELETE FROM quote WHERE id = ?1", [id])?;
    audit::append(
        conn,
        &now_str(now),
        Actor::User,
        "quote.delete",
        &number,
        &serde_json::json!({}),
    )?;
    Ok(())
}

/// PDF del presupuesto o la proforma.
pub fn document(v: &Vault, id: i64, today: Date) -> Result<(String, Vec<u8>)> {
    let q = get(v, id, today)?;
    let s = invoicing::require_settings(v)?;
    let inv = invoicing::build_invoice(&s, &q.draft, q.number.clone());
    let totals = compute_totals(&inv.lines, s.rounding);
    let kind = if q.kind == "proforma" {
        QuoteKind::Proforma
    } else {
        QuoteKind::Presupuesto
    };
    let mut opts = invoicing::pdf_options(&s, q.language, None);
    // En moneda extranjera, los importes en esa moneda (sin contravalor: no es una factura).
    opts.currency = q.draft.currency.as_ref().map(|c| pdf::PdfCurrency {
        code: c.code.clone(),
        eur: None,
    });
    let bytes =
        pdf::render_quote(&inv, &totals, &opts, kind, q.valid_until).map_err(AppError::Other)?;
    Ok((format!("{}.pdf", q.number), bytes))
}

/// Borrador de factura con los datos del presupuesto (fecha de hoy) para emitirla.
pub fn invoice_draft(v: &Vault, id: i64, today: Date) -> Result<(InvoiceDraft, Option<Language>)> {
    let q = get(v, id, today)?;
    if q.status == "facturado" {
        return Err(AppError::Other(format!(
            "ya está facturado ({})",
            q.invoice_number.unwrap_or_default()
        )));
    }
    let mut d = q.draft;
    d.issue_date = today;
    d.series = None;
    Ok((d, q.language))
}

/// Enlaza el presupuesto con la factura emitida a partir de él (sólo si aún no está facturado).
pub fn mark_invoiced(v: &Vault, id: i64, invoice_id: i64, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let number: String = conn
        .query_row("SELECT number FROM quote WHERE id = ?1", [id], |r| r.get(0))
        .optional()?
        .ok_or(AppError::NotFound)?;
    link(conn, id, &number, invoice_id, now)
}

/// Deja el presupuesto `id` facturado con `invoice_id`, si no lo estaba ya.
fn link(
    conn: &Connection,
    id: i64,
    number: &str,
    invoice_id: i64,
    now: OffsetDateTime,
) -> Result<()> {
    let invoice: String = conn
        .query_row(
            "SELECT number FROM invoice_issued WHERE id = ?1",
            [invoice_id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let changed = conn.execute(
        "UPDATE quote SET status = 'facturado', invoice_id = ?1, updated_at = ?2
          WHERE id = ?3 AND status != 'facturado'",
        params![invoice_id, now_str(now), id],
    )?;
    if changed == 0 {
        return Err(AppError::Other(format!("{number} ya está facturado")));
    }
    audit::append(
        conn,
        &now_str(now),
        Actor::User,
        "quote.invoiced",
        number,
        &serde_json::json!({ "invoice": invoice }),
    )?;
    Ok(())
}

/// Emite la factura de un presupuesto (el borrador `d`, revisado en el formulario) y deja el
/// presupuesto facturado, en la misma transacción: si ya estaba facturado (p. ej. desde otra
/// ventana), no se emite nada. La factura lleva los datos del cliente con su NIF (no puede ser
/// una simplificada).
pub fn issue_invoice(
    v: &Vault,
    id: i64,
    d: &InvoiceDraft,
    language: Option<Language>,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<IssuedInvoice> {
    let q = get(v, id, taxpain_core::spain_time::peninsular_date(now))?;
    match q.status.as_str() {
        "facturado" => {
            return Err(AppError::Other(format!(
                "{} ya está facturado ({})",
                q.number,
                q.invoice_number.unwrap_or_default()
            )));
        }
        "rechazado" => {
            return Err(AppError::Other(format!(
                "{} está rechazado: cámbialo a aceptado para facturarlo",
                q.number
            )));
        }
        _ => {}
    }
    let identified = d.recipient.as_ref().is_some_and(|r| match &r.id {
        PartyId::Nif(n) => !n.trim().is_empty(),
        PartyId::Foreign { id, .. } => !id.trim().is_empty(),
    });
    if d.kind != InvoiceKind::Completa || !identified {
        return Err(AppError::Other(
            "la factura de un presupuesto lleva los datos del cliente: pon su NIF (sin él sería \
             una factura simplificada, sin cliente)"
                .into(),
        ));
    }
    invoicing::issue_linked(v, d, language, now, confirmed, |tx, invoice_id| {
        link(tx, id, &q.number, invoice_id, now)
    })
}

/// Al anular la factura `invoice_id` (número `invoice`), su presupuesto vuelve a «aceptado» y
/// sin factura, para poder facturarlo de nuevo. Va en la transacción de la anulación.
pub(crate) fn unlink_invoice(
    conn: &Connection,
    invoice_id: i64,
    invoice: &str,
    now: OffsetDateTime,
) -> Result<()> {
    let numbers: Vec<String> = {
        let mut stmt = conn.prepare("SELECT number FROM quote WHERE invoice_id = ?1")?;
        let rows = stmt.query_map([invoice_id], |r| r.get(0))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    conn.execute(
        "UPDATE quote SET status = 'aceptado', invoice_id = NULL, updated_at = ?1
          WHERE invoice_id = ?2",
        params![now_str(now), invoice_id],
    )?;
    for number in numbers {
        audit::append(
            conn,
            &now_str(now),
            Actor::User,
            "quote.reopened",
            &number,
            &serde_json::json!({ "invoice_cancelled": invoice }),
        )?;
    }
    Ok(())
}

/// Suma de lo aceptado y pendiente de facturar (para el panel).
pub fn accepted_pending(v: &Vault) -> Result<(i64, Money)> {
    let (n, total): (i64, i64) = v.connection().query_row(
        "SELECT count(*), COALESCE(SUM(total), 0) FROM quote WHERE status = 'aceptado'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok((n, Money::from_cents(total)))
}
