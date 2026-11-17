//! Regularización de existencias al cierre (PGC, quinta parte, grupos 3, 61 y 71): se da de
//! baja el saldo de las cuentas de existencias (las iniciales) contra su cuenta de variación y se
//! da de alta el inventario final que indica la empresa, en un solo asiento a 31 de diciembre.
//!
//! - 30 mercaderías → 610; 31 materias primas → 611; 32 otros aprovisionamientos → 612;
//! - 33 productos en curso → 710; 34 semiterminados → 711; 35 terminados → 712;
//!   36 subproductos y residuos → 713.
//!
//! Va antes de regularizar el resultado (las 61/71 son de PyG). Se puede rehacer: el asiento
//! anterior del mismo ejercicio se sustituye.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::{AccountCode, EntryLine, JournalEntry, Origin};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::params;
use time::OffsetDateTime;

use crate::accounting;
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

/// Cuenta de variación de existencias de una cuenta del grupo 3.
pub fn variation_account(account: &str) -> Option<&'static str> {
    match account.get(..2)? {
        "30" => Some("610"),
        "31" => Some("611"),
        "32" => Some("612"),
        "33" => Some("710"),
        "34" => Some("711"),
        "35" => Some("712"),
        "36" => Some("713"),
        _ => None,
    }
}

fn concept(year: i32) -> String {
    format!("Regularización de existencias {year}")
}

/// Cuenta de existencias con su saldo actual (inicial) y el final que se propone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StockLine {
    pub account: String,
    pub name: String,
    /// Saldo antes de regularizar (existencias iniciales).
    pub initial: Money,
    /// Existencias finales (inventario a 31/12).
    pub final_amount: Money,
}

/// Cuentas de existencias (30–36) con saldo en el ejercicio, sin contar la regularización ya
/// hecha; `final_amount` es el de esa regularización si existe, o cero.
pub fn stock(v: &Vault, year: i32) -> Result<Vec<StockLine>> {
    let conn = v.connection();
    let mut out: BTreeMap<String, (Money, Money)> = BTreeMap::new();
    let previous = previous_entries(v, year)?;
    for e in accounting::journal(v, year)? {
        if matches!(e.entry.origin, Origin::Regularizacion | Origin::Cierre) {
            continue;
        }
        let is_reg = previous.contains(&e.id);
        for l in &e.entry.lines {
            let a = l.account.as_str();
            if variation_account(a).is_none() {
                continue;
            }
            let slot = out.entry(a.to_owned()).or_default();
            if is_reg {
                // En el asiento de regularización, lo que entra al debe es el inventario final.
                slot.1 += l.debit;
            } else {
                slot.0 += l.debit - l.credit;
            }
        }
    }
    let mut lines = Vec::new();
    for (account, (initial, final_amount)) in out {
        lines.push(StockLine {
            name: accounting::account_name(conn, &account)?.unwrap_or_default(),
            account,
            initial,
            final_amount,
        });
    }
    Ok(lines)
}

/// Regularizaciones anteriores del ejercicio: por su origen y, para las grabadas antes de tener
/// origen propio, las manuales con el concepto de la app.
fn previous_entries(v: &Vault, year: i32) -> Result<Vec<i64>> {
    let conn = v.connection();
    let mut st = conn.prepare(
        "SELECT id FROM journal_entry WHERE fiscal_year = ?1
           AND ((origin_kind = 'Existencias' AND origin_id = ?1)
                OR (origin_kind = 'Manual' AND concept = ?2))",
    )?;
    let rows = st.query_map(params![year, concept(year)], |r| r.get(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Asiento de regularización con las existencias finales `finals` (cuenta → importe). Las
/// cuentas con saldo que no aparezcan se consideran a cero. Sustituye uno anterior del mismo
/// ejercicio. Devuelve el asiento (o `None` si no hay nada que regularizar).
pub fn regularize(
    v: &Vault,
    year: i32,
    finals: &[(String, Money)],
    now: OffsetDateTime,
) -> Result<Option<i64>> {
    for (a, m) in finals {
        let code = AccountCode::new(a).map_err(|e| other(e.to_string()))?;
        if variation_account(code.as_str()).is_none() {
            return Err(other(format!(
                "{a} no es una cuenta de existencias (30 a 36)"
            )));
        }
        if m.is_negative() {
            return Err(other("las existencias finales no pueden ser negativas"));
        }
    }
    let current = stock(v, year)?;
    let date = crate::accounting::fiscal_year(v, year)?.end;
    let mut lines = Vec::new();
    // Baja de las iniciales: variación al debe, existencias al haber.
    for s in &current {
        let var = AccountCode::new(variation_account(&s.account).unwrap_or("610"))
            .map_err(|e| other(e.to_string()))?;
        let acc = AccountCode::new(&s.account).map_err(|e| other(e.to_string()))?;
        lines.extend(EntryLine::signed(var, s.initial, true));
        lines.extend(EntryLine::signed(acc, s.initial, false));
    }
    // Alta de las finales: existencias al debe, variación al haber.
    for (a, m) in finals {
        let var = AccountCode::new(variation_account(a).unwrap_or("610"))
            .map_err(|e| other(e.to_string()))?;
        let acc = AccountCode::new(a).map_err(|e| other(e.to_string()))?;
        lines.extend(EntryLine::signed(acc, *m, true));
        lines.extend(EntryLine::signed(var, *m, false));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    for id in previous_entries(v, year)? {
        tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [id])?;
        tx.execute("DELETE FROM journal_entry WHERE id = ?1", [id])?;
    }
    let id = if lines.is_empty() {
        None
    } else {
        Some(accounting::record_entry(
            &tx,
            &JournalEntry {
                date,
                concept: concept(year),
                // Origen propio: se localiza por él para rehacerla (no por el concepto) y no se
                // retoca ni se borra desde el diario.
                origin: Origin::Existencias(i64::from(year)),
                lines,
            },
            now,
        )?)
    };
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "existencias.regularizar",
        &year.to_string(),
        &serde_json::json!({ "finales": finals.iter().map(|(a, m)| (a.clone(), m.cents())).collect::<Vec<_>>(), "entry": id }),
    )?;
    tx.commit()?;
    Ok(id)
}
