//! Pago de los modelos a ingresar, contabilizado: 4750 (303), 4751 (111, 115 y 123), 473 (130 y 202)
//! o 4752 (200) contra la tesorería. Se registra a mano (caja u otra cuenta, o el cargo de una
//! domiciliación) o con el movimiento del banco ya importado, que queda conciliado. El importe es
//! el de la presentación en producción o, si aún no hay, el del borrador.

use serde::Serialize;
use taxpain_core::accounting::{AccountCode, EntryLine, JournalEntry, Origin};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::{accounting, bank, models};

/// Cuenta de Hacienda que se salda al pagar el modelo, o `None` si el modelo no se paga.
pub fn payable_account(model: &str) -> Option<&'static str> {
    match model {
        "303" => Some("4750"),
        "111" | "115" | "123" => Some("4751"),
        "130" | "202" => Some("473"),
        "200" => Some("4752"),
        _ => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TaxPayment {
    pub id: i64,
    pub model: String,
    pub year: i32,
    pub period: String,
    pub date: Date,
    pub account: String,
    pub treasury: String,
    pub amount: Money,
    pub entry_id: Option<i64>,
    pub bank_tx: Option<i64>,
}

fn other(e: impl ToString) -> AppError {
    AppError::Other(e.to_string())
}

fn now_str(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

/// Importe a ingresar del modelo y si sale de una presentación en producción (si no, del
/// borrador).
pub fn amount_due(v: &Vault, model: &str, year: i32, quarter: u8) -> Result<(Money, bool)> {
    let period = models::period_code(model, quarter);
    let filed: Option<i64> = v
        .connection()
        .query_row(
            "SELECT result FROM tax_return
             WHERE model = ?1 AND year = ?2 AND period = ?3 AND environment = 'produccion'
             ORDER BY presented_at DESC, id DESC LIMIT 1",
            params![model, year, period],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match filed {
        Some(c) => (Money::from_cents(c), true),
        None => (models::draft(v, model, year, quarter)?.result, false),
    })
}

/// Pago registrado de un modelo y periodo, si lo hay.
pub fn payment(v: &Vault, model: &str, year: i32, quarter: u8) -> Result<Option<TaxPayment>> {
    let period = models::period_code(model, quarter);
    let row = v
        .connection()
        .query_row(
            "SELECT id, date, account, treasury, amount, entry_id, bank_tx FROM tax_payment
             WHERE model = ?1 AND year = ?2 AND period = ?3",
            params![model, year, period],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Option<i64>>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                ))
            },
        )
        .optional()?;
    row.map(|(id, date, account, treasury, amount, entry_id, bank_tx)| {
        Ok(TaxPayment {
            id,
            model: model.to_owned(),
            year,
            period: period.clone(),
            date: Date::parse(
                &date,
                time::macros::format_description!("[year]-[month]-[day]"),
            )
            .map_err(other)?,
            account,
            treasury,
            amount: Money::from_cents(amount),
            entry_id,
            bank_tx,
        })
    })
    .transpose()
}

/// Comprueba el modelo y devuelve (cuenta de Hacienda, importe). En el 303 registra antes la
/// liquidación del trimestre si falta (477/472 → 4750).
fn prepare(
    v: &Vault,
    model: &str,
    year: i32,
    quarter: u8,
    now: OffsetDateTime,
) -> Result<(AccountCode, Money)> {
    let account =
        payable_account(model).ok_or_else(|| other(format!("el modelo {model} no tiene pago")))?;
    if payment(v, model, year, quarter)?.is_some() {
        return Err(other(format!(
            "el pago del {model} de {} ya está registrado",
            models::period_code(model, quarter)
        )));
    }
    let (amount, _) = amount_due(v, model, year, quarter)?;
    if amount.cents() <= 0 {
        return Err(other(format!(
            "el {model} no sale a ingresar ({amount}): no hay nada que pagar"
        )));
    }
    if model == "303" {
        accounting::settle_vat_quarter(v, year, quarter, now)?;
    }
    Ok((AccountCode::new(account).map_err(other)?, amount))
}

fn concept(model: &str, year: i32, quarter: u8) -> String {
    format!(
        "Pago modelo {model} {} {year}",
        models::period_code(model, quarter)
    )
}

/// Registra el pago del modelo el día `date` desde `treasury` (570 caja, 572x banco…), con su
/// asiento. Para un cargo que ya está en los extractos del banco, usa [`pay_from_bank`].
pub fn pay(
    v: &Vault,
    model: &str,
    year: i32,
    quarter: u8,
    date: Date,
    treasury: &str,
    now: OffsetDateTime,
) -> Result<TaxPayment> {
    let treasury = AccountCode::new(treasury).map_err(other)?;
    if !["57", "52", "55"]
        .iter()
        .any(|p| treasury.as_str().starts_with(p))
    {
        return Err(other(
            "la cuenta de pago ha de ser de tesorería (570, 572…) o 52/55",
        ));
    }
    let (account, amount) = prepare(v, model, year, quarter, now)?;
    let period = models::period_code(model, quarter);
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO tax_payment(model, year, period, date, account, treasury, amount, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            model,
            year,
            period,
            date.to_string(),
            account.as_str(),
            treasury.as_str(),
            amount.cents(),
            now_str(now)
        ],
    )?;
    let id = tx.last_insert_rowid();
    let entry = JournalEntry {
        date,
        concept: concept(model, year, quarter),
        origin: Origin::PagoModelo(id),
        lines: vec![
            EntryLine::debit(account, amount),
            EntryLine::credit(treasury, amount),
        ],
    };
    let entry_id = accounting::record_entry(&tx, &entry, now)?;
    tx.execute(
        "UPDATE tax_payment SET entry_id = ?1 WHERE id = ?2",
        params![entry_id, id],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "tax.pay",
        &format!("{model} {period} {year}"),
        &serde_json::json!({ "amount": amount.cents(), "date": date.to_string() }),
    )?;
    tx.commit()?;
    payment(v, model, year, quarter)?.ok_or(AppError::NotFound)
}

/// Registra el pago con un cargo del banco ya importado: lo concilia contra la cuenta de
/// Hacienda (el asiento es el de la conciliación). El cargo ha de ser exactamente el importe.
pub fn pay_from_bank(
    v: &Vault,
    model: &str,
    year: i32,
    quarter: u8,
    tx_id: i64,
    now: OffsetDateTime,
) -> Result<TaxPayment> {
    let (account, amount) = prepare(v, model, year, quarter, now)?;
    let (date, charged): (String, i64) = v
        .connection()
        .query_row(
            "SELECT op_date, amount FROM bank_tx WHERE id = ?1",
            [tx_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    if charged != -amount.cents() {
        return Err(other(format!(
            "el movimiento es de {} y el {model} sale a ingresar {amount}",
            Money::from_cents(charged)
        )));
    }
    let entry_id = bank::reconcile_account(
        v,
        tx_id,
        account.as_str(),
        &concept(model, year, quarter),
        now,
    )?;
    let period = models::period_code(model, quarter);
    let treasury: String = v.connection().query_row(
        "SELECT b.account FROM bank_tx t JOIN bank_account b ON b.id = t.bank_account WHERE t.id = ?1",
        [tx_id],
        |r| r.get(0),
    )?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO tax_payment(model, year, period, date, account, treasury, amount, entry_id, bank_tx, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            model,
            year,
            period,
            date,
            account.as_str(),
            treasury,
            amount.cents(),
            entry_id,
            tx_id,
            now_str(now)
        ],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "tax.pay",
        &format!("{model} {period} {year}"),
        &serde_json::json!({ "amount": amount.cents(), "bank_tx": tx_id }),
    )?;
    tx.commit()?;
    payment(v, model, year, quarter)?.ok_or(AppError::NotFound)
}

/// Cargos del banco sin conciliar por el importe exacto del modelo (candidatos para
/// [`pay_from_bank`]). Sólo de cuentas en euros: el modelo se paga en euros.
pub fn bank_candidates(
    v: &Vault,
    model: &str,
    year: i32,
    quarter: u8,
) -> Result<Vec<bank::BankTx>> {
    let (amount, _) = amount_due(v, model, year, quarter)?;
    Ok(bank::movements(v, true)?
        .into_iter()
        .filter(|t| t.currency == "EUR" && t.amount == -amount)
        .collect())
}

/// Deshace un pago registrado a mano (con su asiento, si el ejercicio sigue abierto). Uno
/// conciliado con el banco se deshace desconciliando el movimiento.
pub fn undo(v: &Vault, model: &str, year: i32, quarter: u8, now: OffsetDateTime) -> Result<()> {
    let p = payment(v, model, year, quarter)?.ok_or(AppError::NotFound)?;
    if p.bank_tx.is_some() {
        return Err(other(
            "este pago está conciliado con un movimiento del banco: deshazlo desde el banco",
        ));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    if let Some(e) = p.entry_id {
        tx.execute(
            "UPDATE tax_payment SET entry_id = NULL WHERE id = ?1",
            [p.id],
        )?;
        tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [e])?;
        tx.execute("DELETE FROM journal_entry WHERE id = ?1", [e])?;
    }
    tx.execute("DELETE FROM tax_payment WHERE id = ?1", [p.id])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "tax.unpay",
        &format!("{model} {} {year}", p.period),
        &serde_json::json!({ "amount": p.amount.cents(), "date": p.date.to_string() }),
    )?;
    tx.commit()?;
    Ok(())
}
