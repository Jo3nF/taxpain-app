//! Asientos predefinidos y periódicos (como los «apuntes predefinidos» de a3, ver
//! `docs/research/A3.md` §2.3):
//! - una plantilla guarda el concepto y las líneas (cuenta, descripción, debe/haber e importe);
//! - en la cuenta, un `?` final pide completarla (`6290?` → el usuario teclea la subcuenta);
//! - un importe vacío se pide al usarla;
//! - `#` en el concepto es el número correlativo de la plantilla (sube al registrar);
//! - `$` en la descripción de una línea es el nombre de su cuenta;
//! - si es periódica (cada N meses desde `next_date`), aparece como pendiente cuando llega la
//!   fecha; el usuario la revisa y la registra, y la siguiente fecha avanza.

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::JournalEntry;
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::{Date, Month, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateLine {
    /// Cuenta; con `?` al final, el usuario la completa.
    pub account: String,
    #[serde(default)]
    pub description: String,
    pub debit: bool,
    /// Importe fijo, o `None` para pedirlo.
    pub amount: Option<Money>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Template {
    #[serde(default)]
    pub id: i64,
    pub name: String,
    pub concept: String,
    pub lines: Vec<TemplateLine>,
    /// Periodicidad en meses (1 mensual, 3 trimestral, 12 anual), si es periódica.
    pub every_months: Option<u8>,
    /// Próxima fecha de contabilización, si es periódica.
    pub next_date: Option<Date>,
    /// Veces registrada (para `#`).
    #[serde(default)]
    pub counter: i64,
}

fn valid_account(a: &str) -> bool {
    let digits = a.strip_suffix('?').unwrap_or(a);
    (1..=12).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())
}

fn validate(t: &Template) -> Result<()> {
    let bad = |m: &str| Err(AppError::Other(m.into()));
    if t.name.trim().is_empty() {
        return bad("pon un nombre a la plantilla");
    }
    if t.lines.len() < 2 {
        return bad("una plantilla necesita al menos dos líneas");
    }
    for (i, l) in t.lines.iter().enumerate() {
        if !valid_account(l.account.trim()) {
            return Err(AppError::Other(format!(
                "línea {}: la cuenta son dígitos, con «?» al final si se completa al usarla",
                i + 1
            )));
        }
        if l.amount.is_some_and(|m| m.cents() <= 0) {
            return Err(AppError::Other(format!(
                "línea {}: el importe fijo debe ser positivo (o déjalo vacío para pedirlo)",
                i + 1
            )));
        }
    }
    if t.lines.iter().all(|l| l.amount.is_some()) {
        let d: Money = t
            .lines
            .iter()
            .filter(|l| l.debit)
            .filter_map(|l| l.amount)
            .sum();
        let c: Money = t
            .lines
            .iter()
            .filter(|l| !l.debit)
            .filter_map(|l| l.amount)
            .sum();
        if d != c {
            return bad("con todos los importes fijos, la plantilla debe cuadrar");
        }
    }
    match (t.every_months, t.next_date) {
        (None, None) => {}
        (Some(m), Some(_)) if (1..=12).contains(&m) => {}
        _ => return bad("una plantilla periódica necesita periodicidad (1–12 meses) y fecha"),
    }
    Ok(())
}

/// Fecha `months` meses después (el día se ajusta al último del mes si no existe: 31/1 → 28/2).
pub fn add_months(d: Date, months: u8) -> Date {
    let total = i32::from(d.month() as u8) - 1 + i32::from(months);
    let year = d.year() + total.div_euclid(12);
    let month = Month::try_from(u8::try_from(total.rem_euclid(12) + 1).unwrap_or(1))
        .unwrap_or(Month::January);
    let last = month.length(year);
    Date::from_calendar_date(year, month, d.day().min(last)).unwrap_or(d)
}

fn row_to_template(
    r: &taxpain_vault::rusqlite::Row<'_>,
) -> taxpain_vault::rusqlite::Result<(Template, String)> {
    let next: Option<String> = r.get(5)?;
    Ok((
        Template {
            id: r.get(0)?,
            name: r.get(1)?,
            concept: r.get(2)?,
            lines: Vec::new(),
            every_months: r.get::<_, Option<u8>>(4)?,
            next_date: next.and_then(|s| {
                Date::parse(
                    &s,
                    time::macros::format_description!("[year]-[month]-[day]"),
                )
                .ok()
            }),
            counter: r.get(6)?,
        },
        r.get(3)?,
    ))
}

const SELECT: &str =
    "SELECT id, name, concept, lines, every_months, next_date, counter FROM entry_template";

fn collect(
    rows: impl Iterator<Item = taxpain_vault::rusqlite::Result<(Template, String)>>,
) -> Result<Vec<Template>> {
    let mut out = Vec::new();
    for row in rows {
        let (mut t, lines) = row?;
        t.lines = serde_json::from_str(&lines).map_err(|e| AppError::Other(e.to_string()))?;
        out.push(t);
    }
    Ok(out)
}

pub fn list(v: &Vault) -> Result<Vec<Template>> {
    let mut stmt = v.connection().prepare(&format!("{SELECT} ORDER BY name"))?;
    let rows = stmt.query_map([], row_to_template)?;
    collect(rows)
}

/// Periódicas cuya fecha ya ha llegado (a `today`), por fecha.
pub fn due(v: &Vault, today: Date) -> Result<Vec<Template>> {
    let mut stmt = v.connection().prepare(&format!(
        "{SELECT} WHERE next_date IS NOT NULL AND next_date <= ?1 ORDER BY next_date, name"
    ))?;
    let rows = stmt.query_map([today.to_string()], row_to_template)?;
    collect(rows)
}

/// Crea (id 0) o modifica una plantilla. Devuelve su id.
pub fn save(v: &Vault, t: &Template, now: OffsetDateTime) -> Result<i64> {
    validate(t)?;
    let lines = serde_json::to_string(&t.lines).map_err(|e| AppError::Other(e.to_string()))?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let name = t.name.trim();
    let id = if t.id == 0 {
        tx.execute(
            "INSERT INTO entry_template(name, concept, lines, every_months, next_date, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                name,
                t.concept,
                lines,
                t.every_months,
                t.next_date.map(|d| d.to_string()),
                now_str(now)
            ],
        )
        .map_err(|e| match e {
            taxpain_vault::rusqlite::Error::SqliteFailure(f, _)
                if f.code == taxpain_vault::rusqlite::ErrorCode::ConstraintViolation =>
            {
                AppError::Other("ya hay una plantilla con ese nombre".into())
            }
            e => e.into(),
        })?;
        tx.last_insert_rowid()
    } else {
        let n = tx.execute(
            "UPDATE entry_template SET name = ?1, concept = ?2, lines = ?3, every_months = ?4,
               next_date = ?5 WHERE id = ?6",
            params![
                name,
                t.concept,
                lines,
                t.every_months,
                t.next_date.map(|d| d.to_string()),
                t.id
            ],
        )?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        t.id
    };
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "template.save",
        name,
        &serde_json::json!({ "id": id, "every_months": t.every_months }),
    )?;
    tx.commit()?;
    Ok(id)
}

pub fn delete(v: &Vault, id: i64, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let name: Option<String> = tx
        .query_row("SELECT name FROM entry_template WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    let Some(name) = name else {
        return Err(AppError::NotFound);
    };
    tx.execute("DELETE FROM entry_template WHERE id = ?1", [id])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "template.delete",
        &name,
        &serde_json::json!({ "id": id }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Registra el asiento hecho con la plantilla `id` (ya revisado por el usuario): sube el
/// correlativo y, si es periódica, pasa a la siguiente fecha. Todo en una transacción.
pub fn post(v: &Vault, id: i64, entry: &JournalEntry, now: OffsetDateTime) -> Result<i64> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let (every, next): (Option<u8>, Option<String>) = tx
        .query_row(
            "SELECT every_months, next_date FROM entry_template WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let entry_id = crate::accounting::record_entry(&tx, entry, now)?;
    let next = match (every, next) {
        (Some(m), Some(d)) => {
            let d = Date::parse(
                &d,
                time::macros::format_description!("[year]-[month]-[day]"),
            )
            .map_err(|e| AppError::Other(e.to_string()))?;
            Some(add_months(d, m).to_string())
        }
        _ => None,
    };
    tx.execute(
        "UPDATE entry_template SET counter = counter + 1, next_date = COALESCE(?1, next_date)
         WHERE id = ?2",
        params![next, id],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "journal.template",
        &entry.concept,
        &serde_json::json!({ "entry": entry_id, "template": id }),
    )?;
    tx.commit()?;
    Ok(entry_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn meses_y_cuentas() {
        assert_eq!(add_months(date!(2026 - 01 - 31), 1), date!(2026 - 02 - 28));
        assert_eq!(add_months(date!(2026 - 11 - 15), 3), date!(2027 - 02 - 15));
        assert_eq!(add_months(date!(2027 - 12 - 31), 12), date!(2028 - 12 - 31));
        assert!(valid_account("6290?"));
        assert!(valid_account("4100001"));
        assert!(!valid_account("62?9"));
        assert!(!valid_account("?"));
    }
}
