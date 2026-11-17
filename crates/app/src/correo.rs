//! Facturas y recordatorios de pago por correo, sin servidor ni contraseñas: TaxPain prepara un
//! mensaje `.eml` con el PDF adjunto y lo abre en el programa de correo del Mac. Apple Mail lo
//! abre como un mensaje nuevo, sin enviar (cabecera `X-Unsent: 1`); el usuario lo revisa y lo
//! envía él. TaxPain no envía nada ni guarda credenciales.
//!
//! Los recordatorios suben de tono (1.º, 2.º y 3.º, este con la mención de la Ley 3/2004) y se
//! cuentan con la auditoría (`invoice.reminder`), sin tabla propia.

use base64::Engine;
use serde::{Deserialize, Serialize};
use taxpain_core::invoice::{Party, PartyId};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::OptionalExtension;
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;
use crate::settings::Language;

/// Propuesta de correo para una factura.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailDraft {
    pub to: Option<String>,
    pub subject: String,
    pub body: String,
    /// Nombres de los adjuntos.
    pub attachments: Vec<String>,
    pub language: Language,
    /// Recordatorio: cuál sería (1, 2, 3…) y cuándo se preparó el anterior.
    pub reminder: Option<u32>,
    pub last_reminder: Option<String>,
    /// Lo que queda por cobrar, lo ya vencido (sin los plazos que aún no han llegado) y los
    /// días de retraso según la cartera.
    pub pending: Money,
    #[serde(default)]
    pub overdue: Money,
    pub overdue_days: i64,
}

struct InvoiceInfo {
    number: String,
    date: Date,
    total: Money,
    /// A cobrar en euros (lo de la cartera).
    payable: Money,
    recipient: Option<Party>,
    status: String,
    /// Factura en moneda extranjera: el correo da los importes en esa moneda.
    fx: Option<crate::invoicing::StoredFx>,
}

fn load(v: &Vault, id: i64) -> Result<InvoiceInfo> {
    let row: Option<(String, String, i64, String, Option<String>, i64)> = v
        .connection()
        .query_row(
            "SELECT number, issue_date, total, status, json_extract(data, '$.invoice.recipient'),
                    payable
               FROM invoice_issued WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .optional()?;
    let (number, date, total, status, recipient, payable) = row.ok_or(AppError::NotFound)?;
    Ok(InvoiceInfo {
        number,
        date: Date::parse(
            &date,
            time::macros::format_description!("[year]-[month]-[day]"),
        )
        .map_err(|e| AppError::Other(e.to_string()))?,
        total: Money::from_cents(total),
        payable: Money::from_cents(payable),
        recipient: recipient.and_then(|r| serde_json::from_str(&r).ok()),
        status,
        fx: crate::invoicing::stored_fx(v.connection(), id)?,
    })
}

/// DD/MM/AAAA (también en inglés: es el formato europeo).
fn fmt_date(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

/// «1.234,56 €» o «€1,234.56».
fn fmt_money(m: Money, lang: Language) -> String {
    match lang {
        Language::Es => m.to_string(),
        Language::En => {
            let s = m.to_string();
            let s = s.trim_end_matches(" €");
            let swapped: String = s
                .chars()
                .map(|c| match c {
                    '.' => ',',
                    ',' => '.',
                    c => c,
                })
                .collect();
            match swapped.strip_prefix('-') {
                Some(rest) => format!("-€{rest}"),
                None => format!("€{swapped}"),
            }
        }
    }
}

/// Importe en una moneda que no es el euro: «1.234,56 USD» o «USD 1,234.56».
fn fmt_currency(m: Money, code: &str, lang: Language) -> String {
    match lang {
        Language::Es => format!("{} {code}", m.to_string().trim_end_matches(" €")),
        Language::En => {
            let s = fmt_money(m, Language::En).replacen('€', "", 1);
            match s.strip_prefix('-') {
                Some(rest) => format!("-{code} {rest}"),
                None => format!("{code} {s}"),
            }
        }
    }
}

/// Recordatorios ya preparados para la factura (número y fecha del último).
fn reminders(v: &Vault, number: &str) -> Result<(u32, Option<String>)> {
    let (n, last): (i64, Option<String>) = v.connection().query_row(
        "SELECT count(*), max(at) FROM audit_log WHERE action = 'invoice.reminder' AND subject = ?1",
        [number],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok((u32::try_from(n).unwrap_or(0), last))
}

/// Lo pendiente de cobro de una factura según la cartera.
#[derive(Debug, Default)]
struct Dues {
    pending: Money,
    /// Primer vencimiento pendiente.
    first: Option<Date>,
    /// Lo vencido (vencimientos anteriores a hoy) y el primero de ellos: en una factura en
    /// plazos, los que aún no han llegado no se reclaman.
    overdue: Money,
    first_overdue: Option<Date>,
    overdue_days: i64,
}

fn dues(v: &Vault, id: i64, today: Date) -> Result<Dues> {
    let mut out = Dues::default();
    for d in crate::cartera::list(v, today)? {
        if d.kind != "emitida" || d.invoice_id != id || d.status != "pendiente" {
            continue;
        }
        out.pending += d.amount;
        out.first = Some(out.first.map_or(d.due_date, |f| f.min(d.due_date)));
        if d.due_date < today {
            out.overdue += d.amount;
            out.first_overdue = Some(out.first_overdue.map_or(d.due_date, |f| f.min(d.due_date)));
        }
        out.overdue_days = out.overdue_days.max(d.overdue_days);
    }
    Ok(out)
}

/// Fecha (en España) de un instante guardado en la auditoría.
fn audit_day(at: &str) -> Option<Date> {
    OffsetDateTime::parse(at, &time::format_description::well_known::Rfc3339)
        .ok()
        .map(taxpain_core::spain_time::peninsular_date)
}

/// Propone el correo de la factura `id` (o un recordatorio de pago si `reminder`).
pub fn compose(v: &Vault, id: i64, reminder: bool, today: Date) -> Result<EmailDraft> {
    let inv = load(v, id)?;
    if inv.status != "emitida" {
        return Err(AppError::Other("la factura está anulada".into()));
    }
    let s = crate::invoicing::settings(v)?.ok_or(AppError::NotConfigured)?;
    let contact = match &inv.recipient {
        Some(p) => crate::contacts::find(v, &p.id)?,
        None => None,
    };
    let foreign = matches!(
        inv.recipient.as_ref().map(|p| &p.id),
        Some(PartyId::Foreign { .. })
    );
    let lang = contact
        .as_ref()
        .and_then(|c| c.language)
        .unwrap_or(if foreign { Language::En } else { s.language });
    let dues = dues(v, id, today)?;
    let (sent, last_reminder) = reminders(v, &inv.number)?;
    if reminder && dues.pending == Money::ZERO {
        return Err(AppError::Other(
            "la factura no tiene nada pendiente de cobro".into(),
        ));
    }
    // El recordatorio reclama lo vencido (no los plazos que aún no han llegado); si no ha
    // vencido nada, es un aviso amable de lo pendiente.
    let overdue = reminder && !dues.overdue.is_zero();
    let (claimed, due) = if overdue {
        (dues.overdue, dues.first_overdue)
    } else {
        (dues.pending, dues.first)
    };
    // Volver a prepararlo el mismo día (se cerró Mail sin enviarlo) no es otro aviso.
    let same_day = last_reminder.as_deref().and_then(audit_day) == Some(today);
    let level = reminder.then(|| {
        if !overdue {
            1
        } else if same_day {
            sent.clamp(1, 3)
        } else {
            (sent + 1).min(3)
        }
    });
    let n = &inv.number;
    let date = fmt_date(inv.date);
    let (total, pend) = match &inv.fx {
        // En moneda extranjera, lo que el cliente paga en esa moneda: la cartera va en euros.
        Some(f) => {
            let code = f.currency.code.as_str();
            let claimed_cur = if claimed == inv.payable {
                f.totals.payable
            } else {
                f.currency.rate.from_eur(claimed)
            };
            (
                fmt_currency(f.totals.invoice_total, code, lang),
                fmt_currency(claimed_cur, code, lang),
            )
        }
        None => (fmt_money(inv.total, lang), fmt_money(claimed, lang)),
    };
    let due_s = due.map(fmt_date);
    // Datos de pago del pie de factura (IBAN…), si los hay.
    let pay = s
        .footer
        .as_deref()
        .filter(|f| f.to_uppercase().contains("IBAN"))
        .map(str::trim);
    let issuer = &s.issuer.name;
    let (subject, mut body) = match (lang, level) {
        (Language::Es, None) => (
            format!("Factura {n}"),
            format!(
                "Hola:\n\nAdjuntamos la factura {n}, del {date}, por un importe de {total}.{}\n",
                due_s
                    .as_deref()
                    .map(|d| format!(" Vence el {d}."))
                    .unwrap_or_default()
            ),
        ),
        (Language::En, None) => (
            format!("Invoice {n}"),
            format!(
                "Hello,\n\nPlease find attached invoice {n}, dated {date}, for {total}.{}\n",
                due_s
                    .as_deref()
                    .map(|d| format!(" Payment is due on {d}."))
                    .unwrap_or_default()
            ),
        ),
        (Language::Es, Some(l)) => {
            let venc = due_s
                .as_deref()
                .map(|d| {
                    if overdue {
                        format!(", que venció el {d},")
                    } else {
                        format!(", que vence el {d},")
                    }
                })
                .unwrap_or_default();
            let part = if overdue && dues.overdue != dues.pending {
                " (lo vencido hasta hoy)"
            } else {
                ""
            };
            let mut b = format!(
                "Hola:\n\nSegún nuestros registros, la factura {n}, del {date}{venc} sigue \
                 pendiente de pago por {pend}{part}."
            );
            b.push_str(match l {
                1 => {
                    " Si ya se ha pagado, rogamos que no se tenga en cuenta este mensaje; si no, \
                     agradeceríamos el pago en los próximos días.\n"
                }
                2 => {
                    " Ya lo recordamos anteriormente. Rogamos que se realice el pago a la mayor \
                     brevedad o que nos indiquen si hay alguna incidencia con la factura.\n"
                }
                _ => {
                    " Es el tercer aviso. Rogamos el pago inmediato. Recordamos que, en las \
                     operaciones entre empresas, la Ley 3/2004 de lucha contra la morosidad \
                     prevé intereses de demora desde el vencimiento y una indemnización de 40 € \
                     por los costes de cobro.\n"
                }
            });
            (format!("Recordatorio de pago: factura {n}"), b)
        }
        (Language::En, Some(l)) => {
            let venc = due_s
                .as_deref()
                .map(|d| format!(", due on {d},"))
                .unwrap_or_default();
            let part = if overdue && dues.overdue != dues.pending {
                " now due"
            } else {
                " outstanding"
            };
            let mut b = format!(
                "Hello,\n\nAccording to our records, invoice {n}, dated {date}{venc} is still \
                 unpaid ({pend}{part})."
            );
            b.push_str(match l {
                1 => {
                    " If you have already paid it, please disregard this message; otherwise we \
                     would appreciate payment within the next few days.\n"
                }
                2 => {
                    " This is our second reminder. Please arrange payment as soon as possible \
                     or let us know if there is any issue with the invoice.\n"
                }
                _ => {
                    " This is our final reminder. Please pay immediately. Under EU Directive \
                     2011/7/EU on late payment, statutory interest and a fixed compensation of \
                     €40 for recovery costs may apply.\n"
                }
            });
            (format!("Payment reminder: invoice {n}"), b)
        }
    };
    if let Some(p) = pay {
        body.push_str(&match lang {
            Language::Es => format!("\nDatos de pago: {p}\n"),
            Language::En => format!("\nPayment details: {p}\n"),
        });
    }
    body.push_str(&match lang {
        Language::Es => format!("\nUn saludo,\n{issuer}\n"),
        Language::En => format!("\nKind regards,\n{issuer}\n"),
    });
    Ok(EmailDraft {
        to: contact.and_then(|c| c.email),
        subject,
        body,
        attachments: vec![format!("{}.pdf", file_stem(n))],
        language: lang,
        reminder: level,
        last_reminder,
        pending: dues.pending,
        overdue: dues.overdue,
        overdue_days: dues.overdue_days,
    })
}

/// Nombre de fichero seguro a partir del número de factura.
fn file_stem(number: &str) -> String {
    number
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Bytes de texto por palabra codificada: 39 → 52 de base64 → 64 caracteres con
/// `=?UTF-8?B?…?=` (el máximo es 75, RFC 2047 §2), y «Subject: » + 64 cabe en una línea de 76.
const ENCODED_WORD_BYTES: usize = 39;
/// Longitud máxima de una línea de cabecera (RFC 2047 §2 para las que llevan palabras
/// codificadas; RFC 5322 §2.1.1 recomienda 78).
const HEADER_LINE: usize = 76;

/// Texto no ASCII en palabras codificadas (RFC 2047) de como mucho [`ENCODED_WORD_BYTES`]
/// bytes cada una, sin partir ningún carácter (cada palabra tiene que ser UTF-8 válido).
fn encoded_words(s: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut chunk = String::new();
    for c in s.chars() {
        if chunk.len() + c.len_utf8() > ENCODED_WORD_BYTES {
            words.push(std::mem::take(&mut chunk));
        }
        chunk.push(c);
    }
    if !chunk.is_empty() || words.is_empty() {
        words.push(chunk);
    }
    words
        .into_iter()
        .map(|w| {
            format!(
                "=?UTF-8?B?{}?=",
                base64::engine::general_purpose::STANDARD.encode(w.as_bytes())
            )
        })
        .collect()
}

/// Cabecera `name: value` con su fin de línea: sin saltos de línea colados (una cabecera no
/// puede meter otra), el texto no ASCII codificado (RFC 2047) y las líneas largas plegadas por
/// los espacios (RFC 5322 §2.2.3; entre palabras codificadas el espacio no cuenta).
fn header(name: &str, value: &str) -> String {
    let clean: String = value.chars().filter(|c| *c != '\r' && *c != '\n').collect();
    let words: Vec<String> = if clean.is_ascii() {
        clean.split(' ').map(str::to_owned).collect()
    } else {
        encoded_words(&clean)
    };
    let mut out = format!("{name}:");
    let mut line = out.len();
    for w in words {
        if line > name.len() + 1 && line + 1 + w.len() > HEADER_LINE {
            out.push_str("\r\n");
            line = 0;
        }
        out.push(' ');
        out.push_str(&w);
        line += 1 + w.len();
    }
    out.push_str("\r\n");
    out
}

/// Base64 en líneas de 76 caracteres.
fn b64_lines(bytes: &[u8]) -> String {
    let enc = base64::engine::general_purpose::STANDARD.encode(bytes);
    let mut out = String::with_capacity(enc.len() + enc.len() / 76 * 2 + 2);
    for chunk in enc.as_bytes().chunks(76) {
        out.push_str(&String::from_utf8_lossy(chunk));
        out.push_str("\r\n");
    }
    out
}

/// Mensaje `.eml` (RFC 5322 / MIME) sin enviar, con adjuntos.
pub fn eml(
    to: Option<&str>,
    subject: &str,
    body: &str,
    attachments: &[(String, &str, Vec<u8>)],
    boundary: &str,
) -> Vec<u8> {
    let mut m = String::new();
    m.push_str("X-Unsent: 1\r\n");
    if let Some(t) = to.filter(|t| !t.trim().is_empty()) {
        m.push_str(&header("To", t.trim()));
    }
    m.push_str(&header("Subject", subject));
    m.push_str("MIME-Version: 1.0\r\n");
    m.push_str(&format!(
        "Content-Type: multipart/mixed; boundary=\"{boundary}\"\r\n\r\n"
    ));
    m.push_str(&format!("--{boundary}\r\n"));
    m.push_str("Content-Type: text/plain; charset=UTF-8\r\n");
    m.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
    m.push_str(&b64_lines(body.replace('\n', "\r\n").as_bytes()));
    for (name, mime, bytes) in attachments {
        m.push_str(&format!("--{boundary}\r\n"));
        m.push_str(&format!("Content-Type: {mime}; name=\"{name}\"\r\n"));
        m.push_str(&format!(
            "Content-Disposition: attachment; filename=\"{name}\"\r\n"
        ));
        m.push_str("Content-Transfer-Encoding: base64\r\n\r\n");
        m.push_str(&b64_lines(bytes));
    }
    m.push_str(&format!("--{boundary}--\r\n"));
    m.into_bytes()
}

/// Prepara el `.eml` de la factura con lo que el usuario ha revisado. Devuelve el nombre y el
/// contenido. No anota nada: eso lo hace [`record_sent`] cuando ya se ha abierto en el programa
/// de correo (si no se llega a abrir, el recordatorio no cuenta).
pub fn build(
    v: &Vault,
    id: i64,
    to: Option<&str>,
    subject: &str,
    body: &str,
) -> Result<(String, Vec<u8>)> {
    let inv = load(v, id)?;
    if inv.status != "emitida" {
        return Err(AppError::Other("la factura está anulada".into()));
    }
    if let Some(t) = to.map(str::trim).filter(|t| !t.is_empty())
        && t.split(',')
            .any(|a| !a.contains('@') || a.trim().contains(char::is_whitespace))
    {
        return Err(AppError::Other(format!("el correo {t} no es válido")));
    }
    if subject.trim().is_empty() {
        return Err(AppError::Other("pon un asunto".into()));
    }
    let (_, _, pdf) = crate::invoicing::invoice_document(v, id, false)?;
    let stem = file_stem(&inv.number);
    let mut b = [0u8; 12];
    getrandom::fill(&mut b).map_err(|e| AppError::Other(e.to_string()))?;
    let boundary = format!("TaxPain-{}", hex::encode(b));
    let bytes = eml(
        to,
        subject,
        body,
        &[(format!("{stem}.pdf"), "application/pdf", pdf)],
        &boundary,
    );
    Ok((format!("{stem}.eml"), bytes))
}

/// Deja anotado en la auditoría el correo de la factura `id` (`invoice.email`) o su
/// recordatorio (`invoice.reminder`, con su nivel), una vez abierto en el programa de correo.
/// Un recordatorio vuelto a abrir el mismo día (se cerró Mail sin enviarlo) no cuenta como otro:
/// el siguiente no sube de tono por eso.
pub fn record_sent(v: &Vault, id: i64, reminder: bool, now: OffsetDateTime) -> Result<()> {
    let inv = load(v, id)?;
    let (sent, last) = reminders(v, &inv.number)?;
    let today = taxpain_core::spain_time::peninsular_date(now);
    if reminder && last.as_deref().and_then(audit_day) == Some(today) {
        return Ok(());
    }
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        if reminder {
            "invoice.reminder"
        } else {
            "invoice.email"
        },
        &inv.number,
        &if reminder {
            serde_json::json!({ "level": (sent + 1).min(3) })
        } else {
            serde_json::json!({})
        },
    )?;
    Ok(())
}

/// Correo propuesto para un presupuesto o una proforma.
pub fn compose_quote(v: &Vault, id: i64, today: Date) -> Result<EmailDraft> {
    let q = crate::quotes::get(v, id, today)?;
    let s = crate::invoicing::settings(v)?.ok_or(AppError::NotConfigured)?;
    let contact = match &q.draft.recipient {
        Some(p) => crate::contacts::find(v, &p.id)?,
        None => None,
    };
    let foreign = matches!(
        q.draft.recipient.as_ref().map(|p| &p.id),
        Some(PartyId::Foreign { .. })
    );
    let lang = q
        .language
        .or(contact.as_ref().and_then(|c| c.language))
        .unwrap_or(if foreign { Language::En } else { s.language });
    let n = &q.number;
    let total = match (&q.draft.currency, q.currency_total) {
        (Some(c), Some(t)) => fmt_currency(t, &c.code, lang),
        _ => fmt_money(q.total, lang),
    };
    let proforma = q.kind == "proforma";
    let valid = q.valid_until.map(fmt_date);
    let issuer = &s.issuer.name;
    let (subject, body) = match lang {
        Language::Es => (
            if proforma {
                format!("Factura proforma {n}")
            } else {
                format!("Presupuesto {n}")
            },
            format!(
                "Hola:\n\nAdjuntamos {} {n} por un importe de {total}{}.\n\nQuedamos a \
                 disposición para cualquier duda.\n\nUn saludo,\n{issuer}\n",
                if proforma {
                    "la factura proforma"
                } else {
                    "el presupuesto"
                },
                valid
                    .as_deref()
                    .map(|d| format!(", válido hasta el {d}"))
                    .unwrap_or_default()
            ),
        ),
        Language::En => (
            if proforma {
                format!("Proforma invoice {n}")
            } else {
                format!("Quote {n}")
            },
            format!(
                "Hello,\n\nPlease find attached {} {n} for {total}{}.\n\nDo not hesitate to \
                 contact us with any questions.\n\nKind regards,\n{issuer}\n",
                if proforma {
                    "proforma invoice"
                } else {
                    "quote"
                },
                valid
                    .as_deref()
                    .map(|d| format!(", valid until {d}"))
                    .unwrap_or_default()
            ),
        ),
    };
    Ok(EmailDraft {
        to: contact.and_then(|c| c.email),
        subject,
        body,
        attachments: vec![format!("{}.pdf", file_stem(n))],
        language: lang,
        reminder: None,
        last_reminder: None,
        pending: q.total,
        overdue: Money::ZERO,
        overdue_days: 0,
    })
}

/// `.eml` del presupuesto con su PDF; si estaba en borrador, pasa a «enviado».
pub fn build_quote(
    v: &Vault,
    id: i64,
    to: Option<&str>,
    subject: &str,
    body: &str,
    today: Date,
    now: OffsetDateTime,
) -> Result<(String, Vec<u8>)> {
    if let Some(t) = to.map(str::trim).filter(|t| !t.is_empty())
        && t.split(',')
            .any(|a| !a.contains('@') || a.trim().contains(char::is_whitespace))
    {
        return Err(AppError::Other(format!("el correo {t} no es válido")));
    }
    if subject.trim().is_empty() {
        return Err(AppError::Other("pon un asunto".into()));
    }
    let q = crate::quotes::get(v, id, today)?;
    let (pdf_name, pdf) = crate::quotes::document(v, id, today)?;
    let mut b = [0u8; 12];
    getrandom::fill(&mut b).map_err(|e| AppError::Other(e.to_string()))?;
    let bytes = eml(
        to,
        subject,
        body,
        &[(pdf_name, "application/pdf", pdf)],
        &format!("TaxPain-{}", hex::encode(b)),
    );
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        "quote.email",
        &q.number,
        &serde_json::json!({}),
    )?;
    if q.status == "borrador" {
        crate::quotes::set_status(v, id, "enviado", now)?;
    }
    Ok((format!("{}.eml", file_stem(&q.number)), bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mensaje_sin_enviar_con_adjunto() {
        let m = eml(
            Some("cliente@example.com"),
            "Factura A-1 · año",
            "Hola:\nAdjunta.",
            &[("A-1.pdf".into(), "application/pdf", b"%PDF-1.7".to_vec())],
            "B",
        );
        let s = String::from_utf8(m).unwrap();
        assert!(s.starts_with("X-Unsent: 1\r\nTo: cliente@example.com\r\n"));
        assert!(s.contains("Subject: =?UTF-8?B?"));
        assert!(s.contains("Content-Disposition: attachment; filename=\"A-1.pdf\""));
        assert!(s.contains("JVBERi0xLjc="), "PDF en base64");
        assert!(s.ends_with("--B--\r\n"));
        // Sin destinatario, Mail pide uno.
        let s = String::from_utf8(eml(None, "x", "y", &[], "B")).unwrap();
        assert!(!s.contains("To:"));
        // Una cabecera no puede colar otra.
        assert_eq!(header("Subject", "a\r\nBcc: x@y"), "Subject: aBcc: x@y\r\n");
        assert_eq!(
            header("Subject", "ñ\r\nBcc: x@y").matches("\r\n").count(),
            1
        );
    }

    /// Las palabras codificadas de un asunto largo con tildes: de 75 caracteres como mucho,
    /// cada una UTF-8 válido por sí sola y líneas plegadas de 76 como mucho.
    #[test]
    fn asunto_largo_con_tildes() {
        let subject = "Recordatorio de pago: factura A2026-0001 de Señalización Ñandú, \
                       S.L. — cuota de mantenimiento de diciembre (2.º aviso) ✓";
        let h = header("Subject", subject);
        assert!(h.ends_with("\r\n"));
        let lines: Vec<&str> = h.trim_end_matches("\r\n").split("\r\n").collect();
        assert!(lines.len() > 2, "{h}");
        assert!(lines[0].starts_with("Subject: =?UTF-8?B?"));
        let mut decoded = Vec::new();
        for (i, l) in lines.iter().enumerate() {
            assert!(l.len() <= 76, "línea {i} de {} caracteres: {l}", l.len());
            if i > 0 {
                assert!(l.starts_with(' '), "plegada con un espacio delante");
            }
            for w in l.split(' ').filter(|w| w.starts_with("=?")) {
                assert!(w.len() <= 75, "{w}");
                let b64 = w
                    .strip_prefix("=?UTF-8?B?")
                    .and_then(|w| w.strip_suffix("?="))
                    .unwrap();
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .unwrap();
                assert!(
                    String::from_utf8(bytes.clone()).is_ok(),
                    "sin partir caracteres"
                );
                decoded.extend(bytes);
            }
        }
        assert_eq!(String::from_utf8(decoded).unwrap(), subject);
        // Uno corto, en una sola palabra; uno ASCII largo, plegado por los espacios.
        assert_eq!(
            header("Subject", "Factura A-1 · año")
                .matches("=?UTF-8?B?")
                .count(),
            1
        );
        let ascii = header("Subject", &"Invoice A-1 ".repeat(12));
        assert!(ascii.split("\r\n").all(|l| l.len() <= 76), "{ascii}");
        assert_eq!(
            ascii.replace("\r\n", ""),
            format!("Subject: {}\r\n", "Invoice A-1 ".repeat(12)).replace("\r\n", "")
        );
    }

    #[test]
    fn importes_en_ingles() {
        assert_eq!(
            fmt_money(Money::from_cents(123_456), Language::En),
            "€1,234.56"
        );
        assert_eq!(fmt_money(Money::from_cents(-500), Language::En), "-€5.00");
        assert_eq!(
            fmt_money(Money::from_cents(123_456), Language::Es),
            "1.234,56 €"
        );
        assert_eq!(file_stem("A 2026/01"), "A-2026-01");
        assert_eq!(
            fmt_currency(Money::from_cents(121_000), "USD", Language::En),
            "USD 1,210.00"
        );
        assert_eq!(
            fmt_currency(Money::from_cents(-500), "GBP", Language::En),
            "-GBP 5.00"
        );
        assert_eq!(
            fmt_currency(Money::from_cents(121_000), "USD", Language::Es),
            "1.210,00 USD"
        );
    }
}
