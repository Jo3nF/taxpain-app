//! Moneda extranjera en el banco y al cierre: cuentas en divisa (USD, GBP…), contravalor en
//! euros de sus movimientos y valoración al cierre de las partidas monetarias.
//!
//! Normas:
//! - **PGC PYMES (RD 1515/2007), NRV 11.ª «Moneda extranjera»**: las partidas monetarias
//!   (tesorería, clientes, proveedores, préstamos… que se cobran o pagan en una cantidad fija de
//!   divisa) se reconocen al tipo de contado de la fecha de la operación y, al cierre, «se
//!   valorarán aplicando el tipo de cambio de cierre»; las diferencias de cambio, positivas o
//!   negativas, de la valoración y de la liquidación «se reconocerán en la cuenta de pérdidas y
//!   ganancias del ejercicio en el que surjan» (668 / 768). Las partidas no monetarias (anticipos
//!   438/407, inmovilizado, existencias) se quedan al tipo histórico. TODO(VERIFY): numeración y
//!   redacción exactas de la NRV en el PGC PYMES (en el PGC normal es la 11.ª, apartado 2.1;
//!   la carencia citaba la 13.ª, que es la del Impuesto sobre beneficios).
//! - **Tipo de cierre**: el tipo de referencia del BCE del último día del ejercicio (o del último
//!   día hábil anterior), que publica el Banco de España como cambio oficial (art. 36 Ley
//!   46/1998). Sin red: se usa el ya descargado con «Tipo del BCE» o el tecleado.
//!
//! Decisiones:
//! - Cada movimiento de una cuenta en divisa guarda su contravalor en euros al conciliarse: el
//!   tecleado (el del propio extracto, si lo trae) o, si no, el del tipo del BCE de su fecha ya
//!   descargado. En un traspaso con una cuenta en euros, los euros reales de la otra cara.
//! - La valoración al cierre se contabiliza el último día del ejercicio (origen
//!   `DiferenciasCambio(año)`) y **se revierte el primer día del ejercicio siguiente**
//!   (`DiferenciasCambio(−año)`). El resultado es el que pide la NRV 11.ª (el ejercicio recoge la
//!   diferencia hasta el tipo de cierre, el siguiente la que va del tipo de cierre al de cobro o
//!   pago) y la app sigue cobrando y pagando las facturas por los euros con que se
//!   contabilizaron, así que el 668/768 del cobro o pago (realizado) más la reversión dan la
//!   diferencia del tipo de cierre al de liquidación. Sin la reversión habría que cambiar en la
//!   cartera los euros contabilizados de cada factura pendiente.
//! - Se valoran: las cuentas del banco en divisa (por su saldo en divisa: el que da el último
//!   extracto o el tecleado) y los vencimientos de facturas emitidas y recibidas en divisa
//!   pendientes a la fecha de cierre (su parte en divisa, en proporción a lo pendiente en euros).
//!   Los préstamos de la app van en euros: no hay préstamos en divisa que valorar.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::{AccountCode, EntryLine, JournalEntry, Origin};
use taxpain_core::fx::ExchangeRate;
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, Duration, OffsetDateTime};

use crate::accounting;
use crate::cierre::ProposedLine;
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

fn code(s: &str) -> Result<AccountCode> {
    AccountCode::new(s).map_err(|e| other(e.to_string()))
}

fn parse_date(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]"))
        .map_err(|e| other(e.to_string()))
}

/// `1.234,56 USD`.
pub fn fmt_divisa(m: Money, currency: &str) -> String {
    let s = m.to_string();
    if currency == "EUR" {
        s
    } else {
        format!("{} {currency}", s.trim_end_matches(" €"))
    }
}

// ---------------- Contravalor de los movimientos ----------------

/// Euros de un movimiento de una cuenta en divisa.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Contravalor {
    /// Con el signo del movimiento.
    pub amount_eur: Money,
    /// Unidades de la divisa por 1 € (el tecleado o el implícito en los euros).
    pub rate: ExchangeRate,
    /// Fecha del tipo.
    pub date: Date,
    /// `bce`, `manual` o `traspaso`.
    pub source: String,
    /// Guardado en el movimiento. Si no, es el del BCE ya descargado (se guarda al conciliar).
    pub stored: bool,
}

struct TxRow {
    currency: String,
    amount: Money,
    date: Date,
    reconciled: bool,
    stored: Option<Contravalor>,
}

type Raw = (
    String,
    i64,
    String,
    bool,
    Option<i64>,
    Option<i64>,
    Option<String>,
    Option<String>,
);

fn tx_row(conn: &Connection, tx_id: i64) -> Result<TxRow> {
    let (currency, amount, date, reconciled, eur, rate, fx_date, source): Raw = conn
        .query_row(
            "SELECT b.currency, t.amount, t.op_date,
                    t.matched_kind IS NOT NULL OR t.entry_id IS NOT NULL,
                    t.amount_eur, t.fx_rate, t.fx_date, t.fx_source
             FROM bank_tx t JOIN bank_account b ON b.id = t.bank_account WHERE t.id = ?1",
            [tx_id],
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
                ))
            },
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let date = parse_date(&date)?;
    Ok(TxRow {
        currency,
        amount: Money::from_cents(amount),
        date,
        reconciled,
        stored: stored_of(eur, rate, fx_date.as_deref(), source, date),
    })
}

pub(crate) fn stored_of(
    eur: Option<i64>,
    rate: Option<i64>,
    date: Option<&str>,
    source: Option<String>,
    op_date: Date,
) -> Option<Contravalor> {
    Some(Contravalor {
        amount_eur: Money::from_cents(eur?),
        rate: ExchangeRate::from_micros(rate?)?,
        date: date.and_then(|d| parse_date(d).ok()).unwrap_or(op_date),
        source: source.unwrap_or_else(|| "manual".into()),
        stored: true,
    })
}

/// Tipo implícito en unos euros: |divisa| / |euros| (unidades de la divisa por 1 €).
pub(crate) fn implied_rate(amount: Money, eur: Money) -> Option<ExchangeRate> {
    if eur.is_zero() {
        return None;
    }
    let num = i128::from(amount.cents().abs()) * 1_000_000;
    let den = i128::from(eur.cents().abs());
    ExchangeRate::from_micros(i64::try_from((num + den / 2) / den).ok()?)
}

/// Contravalor al tipo del BCE ya descargado para `date` (en la caché leída).
pub(crate) fn from_cache(
    cache: &BTreeMap<String, crate::fx::BceRate>,
    currency: &str,
    amount: Money,
    date: Date,
) -> Option<Contravalor> {
    crate::fx::rate_in(cache, currency, date).map(|r| Contravalor {
        amount_eur: r.rate.to_eur(amount),
        rate: r.rate,
        date: r.date,
        source: "bce".into(),
        stored: false,
    })
}

/// Contravalor de un movimiento de una cuenta en divisa: el guardado o, si no hay, el del tipo
/// del BCE de su fecha ya descargado. `None` en las cuentas en euros o si falta el tipo.
pub fn contravalor(conn: &Connection, tx_id: i64) -> Result<Option<Contravalor>> {
    let row = tx_row(conn, tx_id)?;
    if row.currency == "EUR" {
        return Ok(None);
    }
    if row.stored.is_some() {
        return Ok(row.stored);
    }
    Ok(from_cache(
        &crate::fx::cache_conn(conn)?,
        &row.currency,
        row.amount,
        row.date,
    ))
}

fn store(conn: &Connection, tx_id: i64, c: &Contravalor) -> Result<()> {
    conn.execute(
        "UPDATE bank_tx SET amount_eur = ?1, fx_rate = ?2, fx_date = ?3, fx_source = ?4
         WHERE id = ?5",
        params![
            c.amount_eur.cents(),
            c.rate.micros(),
            c.date.to_string(),
            c.source,
            tx_id
        ],
    )?;
    Ok(())
}

/// Euros con que se contabiliza el movimiento `tx_id` de una cuenta en divisa. Si aún no los
/// tenía guardados, guarda los del tipo del BCE de su fecha (dentro de la transacción de la
/// conciliación). Error si falta el tipo.
pub(crate) fn fix(conn: &Connection, tx_id: i64) -> Result<Money> {
    let row = tx_row(conn, tx_id)?;
    if let Some(c) = row.stored {
        return Ok(c.amount_eur);
    }
    let c = crate::fx::rate_on(conn, &row.currency, row.date)?
        .map(|r| Contravalor {
            amount_eur: r.rate.to_eur(row.amount),
            rate: r.rate,
            date: r.date,
            source: "bce".into(),
            stored: true,
        })
        .ok_or_else(|| {
            other(format!(
                "falta el contravalor en euros del movimiento del {} ({}): pulsa «Tipo del BCE» \
                 en el movimiento o escribe los euros que da el extracto",
                row.date,
                fmt_divisa(row.amount, &row.currency)
            ))
        })?;
    if c.amount_eur.is_zero() {
        return Err(other("el contravalor en euros del movimiento es cero"));
    }
    store(conn, tx_id, &c)?;
    Ok(c.amount_eur)
}

/// Guarda como contravalor de `tx_id` los euros reales `eur` de la otra cara de un traspaso.
pub(crate) fn store_transfer(conn: &Connection, tx_id: i64, eur: Money) -> Result<()> {
    let row = tx_row(conn, tx_id)?;
    if row.currency == "EUR" {
        return Ok(());
    }
    let rate = implied_rate(row.amount, eur)
        .ok_or_else(|| other("el traspaso no tiene importe en euros"))?;
    store(
        conn,
        tx_id,
        &Contravalor {
            amount_eur: eur,
            rate,
            date: row.date,
            source: "traspaso".into(),
            stored: true,
        },
    )
}

/// Fija a mano el contravalor de un movimiento sin conciliar de una cuenta en divisa: los euros
/// (los que da el propio extracto) o el tipo (unidades de la divisa por 1 €). Sin ninguno de los
/// dos, quita el tecleado (vuelve a usarse el del BCE). Queda en la auditoría.
pub fn set_contravalor(
    v: &Vault,
    tx_id: i64,
    eur: Option<Money>,
    rate: Option<ExchangeRate>,
    now: OffsetDateTime,
) -> Result<Option<Contravalor>> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let row = tx_row(&tx, tx_id)?;
    if row.currency == "EUR" {
        return Err(other(
            "el movimiento es de una cuenta en euros: no lleva contravalor",
        ));
    }
    if row.reconciled {
        return Err(other(
            "el movimiento ya está conciliado: deshaz la conciliación para cambiar sus euros",
        ));
    }
    let c = match (eur, rate) {
        (Some(e), _) => {
            if e.is_zero() || e.is_negative() != row.amount.is_negative() {
                return Err(other(
                    "los euros han de llevar el signo del movimiento (ingreso o cargo) y no ser cero",
                ));
            }
            let rate = implied_rate(row.amount, e).ok_or_else(|| other("euros no válidos"))?;
            Some(Contravalor {
                amount_eur: e,
                rate,
                date: row.date,
                source: "manual".into(),
                stored: true,
            })
        }
        (None, Some(r)) => Some(Contravalor {
            amount_eur: r.to_eur(row.amount),
            rate: r,
            date: row.date,
            source: "manual".into(),
            stored: true,
        }),
        (None, None) => None,
    };
    match &c {
        Some(c) => store(&tx, tx_id, c)?,
        None => {
            tx.execute(
                "UPDATE bank_tx SET amount_eur = NULL, fx_rate = NULL, fx_date = NULL,
                        fx_source = NULL WHERE id = ?1",
                [tx_id],
            )?;
        }
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "bank.fx",
        &tx_id.to_string(),
        &serde_json::json!({
            "currency": row.currency,
            "amount": row.amount.cents(),
            "amount_eur": c.as_ref().map(|c| c.amount_eur.cents()),
            "rate": c.as_ref().map(|c| c.rate.decimal()),
        }),
    )?;
    tx.commit()?;
    Ok(c)
}

// ---------------- Saldos de las cuentas ----------------

/// Saldo en divisa de la cuenta del banco `bank_account` al final de `date`: el que da el
/// último movimiento con saldo de ese día o anterior (el del extracto) o, si los movimientos no
/// traen saldo, la suma de los importes (más el saldo de partida del primer movimiento, si lo
/// trae). `None` si la cuenta no tiene movimientos hasta esa fecha.
fn currency_balance(conn: &Connection, bank_account: i64, date: Date) -> Result<Option<Money>> {
    let last: Option<(i64, Option<i64>)> = conn
        .query_row(
            "SELECT amount, balance FROM bank_tx WHERE bank_account = ?1 AND op_date <= ?2
             ORDER BY op_date DESC, id DESC LIMIT 1",
            params![bank_account, date.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((_, last_balance)) = last else {
        return Ok(None);
    };
    if let Some(b) = last_balance {
        return Ok(Some(Money::from_cents(b)));
    }
    let (first_amount, first_balance): (i64, Option<i64>) = conn.query_row(
        "SELECT amount, balance FROM bank_tx WHERE bank_account = ?1
         ORDER BY op_date, id LIMIT 1",
        [bank_account],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let sum: i64 = conn.query_row(
        "SELECT COALESCE(SUM(amount), 0) FROM bank_tx WHERE bank_account = ?1 AND op_date <= ?2",
        params![bank_account, date.to_string()],
        |r| r.get(0),
    )?;
    let opening = first_balance.map_or(0, |b| b - first_amount);
    Ok(Some(Money::from_cents(opening + sum)))
}

/// Saldo contable acumulado (debe − haber) de la subcuenta `account` al final de `date`: el
/// diario sin cierres ni aperturas (que se anulan de un ejercicio al siguiente), salvo la
/// apertura del primer ejercicio con asientos, como la previsión de tesorería.
fn ledger_balance(conn: &Connection, account: &str, date: Date) -> Result<Money> {
    let cents: i64 = conn.query_row(
        "SELECT COALESCE(SUM(l.debit - l.credit), 0) FROM journal_line l
         JOIN journal_entry e ON e.id = l.entry_id
         WHERE e.date <= ?1 AND e.origin_kind <> 'Cierre' AND l.account = ?2
           AND (e.origin_kind <> 'Apertura'
                OR e.fiscal_year = (SELECT MIN(fiscal_year) FROM journal_entry))",
        params![date.to_string(), account],
        |r| r.get(0),
    )?;
    Ok(Money::from_cents(cents))
}

/// Saldo de una cuenta del banco en su divisa y en euros.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SaldoCuenta {
    pub name: String,
    pub account: String,
    pub currency: String,
    pub kind: String,
    /// Saldo en la divisa de la cuenta según el extracto (`None` sin movimientos).
    pub saldo: Option<Money>,
    /// Saldo contable de la subcuenta (euros).
    pub contable: Money,
    /// Último tipo del BCE descargado (cuentas en divisa).
    pub tipo: Option<ExchangeRate>,
    pub tipo_fecha: Option<Date>,
    /// El saldo en divisa a ese tipo.
    pub al_tipo: Option<Money>,
    /// Movimientos en divisa sin contravalor (falta el tipo del BCE de su día o teclearlo).
    pub sin_contravalor: i64,
}

/// Saldos de las cuentas del banco (sin las tarjetas de débito, que sólo informan) a `date`, en
/// su divisa y en euros: el contable y, en las de otra divisa, al último tipo del BCE descargado.
pub fn saldos(v: &Vault, date: Date) -> Result<Vec<SaldoCuenta>> {
    let conn = v.connection();
    let cache = crate::fx::cache_conn(conn)?;
    let mut st = conn.prepare(
        "SELECT id, name, account, currency, kind FROM bank_account
         WHERE kind <> ?1 AND account <> '' ORDER BY currency <> 'EUR', name",
    )?;
    let rows: Vec<(i64, String, String, String, String)> = st
        .query_map([crate::bank::INFORMATIVE], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    let mut out = Vec::new();
    for (id, name, account, currency, kind) in rows {
        let saldo = currency_balance(conn, id, date)?;
        let contable = ledger_balance(conn, &account, date)?;
        let last = if currency == "EUR" {
            None
        } else {
            cache
                .values()
                .filter(|r| r.currency == currency && r.date <= date)
                .max_by_key(|r| r.date)
                .cloned()
        };
        let mut missing = 0;
        if currency != "EUR" {
            let mut s = conn.prepare(
                "SELECT amount, op_date FROM bank_tx
                 WHERE bank_account = ?1 AND amount_eur IS NULL AND entry_id IS NULL",
            )?;
            let pending: Vec<(i64, String)> = s
                .query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<_, _>>()?;
            for (a, d) in pending {
                if from_cache(&cache, &currency, Money::from_cents(a), parse_date(&d)?).is_none() {
                    missing += 1;
                }
            }
        }
        out.push(SaldoCuenta {
            al_tipo: match (&last, saldo) {
                (Some(r), Some(s)) => Some(r.rate.to_eur(s)),
                _ => None,
            },
            tipo: last.as_ref().map(|r| r.rate),
            tipo_fecha: last.as_ref().map(|r| r.date),
            name,
            account,
            currency,
            kind,
            saldo,
            contable,
            sin_contravalor: missing,
        });
    }
    Ok(out)
}

/// Notas para la previsión de tesorería: las cuentas en divisa cuentan por su contravalor
/// contable, y cuánto serían al último tipo del BCE descargado.
pub(crate) fn notas_tesoreria(v: &Vault, date: Date) -> Result<Vec<String>> {
    Ok(saldos(v, date)?
        .into_iter()
        .filter(|s| s.currency != "EUR" && s.saldo.is_some_and(|m| !m.is_zero()))
        .map(|s| {
            let saldo = s.saldo.unwrap_or(Money::ZERO);
            let mut t = format!(
                "{} tiene {}: cuenta por su contravalor contable ({})",
                s.name,
                fmt_divisa(saldo, &s.currency),
                s.contable
            );
            if let (Some(r), Some(d), Some(e)) = (s.tipo, s.tipo_fecha, s.al_tipo) {
                t.push_str(&format!(
                    "; al último tipo del BCE descargado ({r} del {d}) serían {e}"
                ));
            }
            t.push('.');
            t
        })
        .collect())
}

// ---------------- Valoración al cierre ----------------

/// Lo que el usuario teclea para la valoración: tipos de cierre (si no hay el del BCE
/// descargado o prefiere otro) y saldos en divisa de las cuentas del banco (por subcuenta) si
/// el extracto no llega al cierre o no trae saldos.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntradaValoracion {
    #[serde(default)]
    pub tipos: BTreeMap<String, ExchangeRate>,
    #[serde(default)]
    pub saldos: BTreeMap<String, Money>,
}

/// Tipo de cierre de una divisa.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TipoCierre {
    pub currency: String,
    pub rate: Option<ExchangeRate>,
    /// Fecha de publicación (la del cierre o el último día hábil anterior).
    pub date: Option<Date>,
    /// `bce`, `manual` o `None` si falta.
    pub source: Option<String>,
}

/// Una partida monetaria en divisa valorada al cierre.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PartidaDivisa {
    /// `banco`, `cliente` o `proveedor`.
    pub kind: String,
    /// Subcuenta (57x, 43x, 40x/41x).
    pub account: String,
    pub label: String,
    pub currency: String,
    /// Importe en divisa (saldo deudor del banco, lo pendiente de cobrar o de pagar).
    pub amount: Money,
    /// De dónde sale el importe en divisa: `extracto`, `tecleado` o `factura`.
    pub amount_source: String,
    /// Euros contabilizados.
    pub booked: Money,
    /// Al tipo de cierre (`None` si falta el tipo).
    pub valued: Option<Money>,
    /// Diferencia de cambio: positiva a favor (768), negativa en contra (668).
    pub difference: Option<Money>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Valoracion {
    pub year: i32,
    pub date: Date,
    /// Fecha de la reversión: el primer día del ejercicio siguiente.
    pub reversal_date: Date,
    pub closed: bool,
    pub rates: Vec<TipoCierre>,
    pub items: Vec<PartidaDivisa>,
    /// Asiento propuesto (o el contabilizado, con lo tecleado entonces).
    pub lines: Vec<ProposedLine>,
    pub gains: Money,
    pub losses: Money,
    pub entry_id: Option<i64>,
    pub reversal_id: Option<i64>,
    /// Lo contabilizado ya no coincide con lo que sale ahora.
    pub stale: bool,
    pub input: EntradaValoracion,
    pub warnings: Vec<String>,
}

fn setting_key(year: i32) -> String {
    format!("fx_valoracion:{year}")
}

fn entry_of(conn: &Connection, id: i64) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT id FROM journal_entry WHERE origin_kind = 'DiferenciasCambio' AND origin_id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?)
}

/// Saldo deudor de `account` en el ejercicio `year` (con su apertura, sin el cierre ni la
/// valoración al cierre de ese ejercicio).
fn year_balance(conn: &Connection, year: i32, account: &str) -> Result<Money> {
    let cents: i64 = conn.query_row(
        "SELECT COALESCE(SUM(l.debit - l.credit), 0) FROM journal_line l
           JOIN journal_entry e ON e.id = l.entry_id
          WHERE e.fiscal_year = ?1 AND e.origin_kind <> 'Cierre' AND l.account = ?2
            AND NOT (e.origin_kind = 'DiferenciasCambio' AND e.origin_id = ?1)",
        params![year, account],
        |r| r.get(0),
    )?;
    Ok(Money::from_cents(cents))
}

/// Importe en divisa a cobrar o pagar (total menos retenciones) de una factura, si es en
/// divisa: (moneda, importe).
fn invoice_fx(conn: &Connection, kind: &str, id: i64) -> Result<Option<(String, Money)>> {
    if id < 0 {
        return Ok(None);
    }
    Ok(match kind {
        "emitida" => crate::invoicing::stored_fx(conn, id)?
            .map(|fx| (fx.currency.code.clone(), fx.totals.payable)),
        "recibida" => {
            let (raw, dua): (Option<String>, bool) = conn.query_row(
                "SELECT fx, COALESCE(import_dua, 0) = 1 FROM invoice_received WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            match (raw, dua) {
                (Some(raw), false) => {
                    let fx: crate::received::ReceivedFx =
                        serde_json::from_str(&raw).map_err(|e| other(e.to_string()))?;
                    Some((fx.currency.code, fx.base + fx.vat - fx.withholding))
                }
                _ => None,
            }
        }
        _ => None,
    })
}

/// Parte en divisa de un vencimiento: la de la factura en proporción a sus euros.
pub(crate) fn installment_fx(total_fx: Money, item_eur: Money, invoice_eur: Money) -> Money {
    if invoice_eur.is_zero() || item_eur == invoice_eur {
        return total_fx;
    }
    let num = i128::from(total_fx.cents()) * i128::from(item_eur.cents());
    let den = i128::from(invoice_eur.cents());
    let q = num / den;
    let r = num % den;
    let q = if 2 * r.abs() >= den.abs() {
        q + (num.signum() * den.signum())
    } else {
        q
    };
    Money::from_cents(i64::try_from(q).unwrap_or(0))
}

/// Divisa e importe en ella de cada factura en divisa de la cartera, por (tipo, id).
pub(crate) type FxTotals = HashMap<(String, i64), Option<(String, Money)>>;

pub(crate) fn fx_of(
    conn: &Connection,
    cache: &mut FxTotals,
    kind: &str,
    id: i64,
) -> Result<Option<(String, Money)>> {
    if let Some(x) = cache.get(&(kind.to_owned(), id)) {
        return Ok(x.clone());
    }
    let x = invoice_fx(conn, kind, id)?;
    cache.insert((kind.to_owned(), id), x.clone());
    Ok(x)
}

/// Valoración al cierre de `year` de las partidas monetarias en divisa. Con `input` (lo
/// tecleado); sin él, lo que se tecleó al contabilizarla (si lo está) o nada.
pub fn valoracion(v: &Vault, year: i32, input: Option<EntradaValoracion>) -> Result<Valoracion> {
    let conn = v.connection();
    let fy = accounting::fiscal_year(v, year)?;
    let closed = accounting::year_closed(conn, year)?;
    let entry_id = entry_of(conn, i64::from(year))?;
    let reversal_id = entry_of(conn, -i64::from(year))?;
    let input = match input {
        Some(i) => i,
        None => v
            .setting::<EntradaValoracion>(&setting_key(year))?
            .unwrap_or_default(),
    };
    let end = fy.end;
    let mut items = Vec::new();
    let mut warnings = Vec::new();

    // Cuentas del banco en divisa.
    let mut st = conn.prepare(
        "SELECT id, name, account, currency FROM bank_account
         WHERE currency <> 'EUR' AND kind <> ?1 AND account <> '' ORDER BY account, id",
    )?;
    let banks: Vec<(i64, String, String, String)> = st
        .query_map([crate::bank::INFORMATIVE], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    let mut seen: HashMap<String, String> = HashMap::new();
    for (id, name, account, currency) in banks {
        if let Some(c) = seen.get(&account) {
            if *c != currency {
                warnings.push(format!(
                    "La subcuenta {account} es de cuentas en {c} y en {currency}: no se valora. \
                     Dale a cada divisa su subcuenta."
                ));
                items.retain(|i: &PartidaDivisa| i.account != account);
            }
            continue;
        }
        seen.insert(account.clone(), currency.clone());
        let booked = year_balance(conn, year, &account)?;
        let (amount, source) = match input.saldos.get(&account) {
            Some(m) => (Some(*m), "tecleado"),
            None => (currency_balance(conn, id, end)?, "extracto"),
        };
        let amount = match amount {
            Some(a) => a,
            None if booked.is_zero() => continue,
            None => {
                warnings.push(format!(
                    "{name} ({account}) tiene {booked} en el diario y ningún movimiento hasta el \
                     {end}: escribe su saldo en {currency} a esa fecha."
                ));
                Money::ZERO
            }
        };
        if amount.is_zero() && booked.is_zero() {
            continue;
        }
        items.push(PartidaDivisa {
            kind: "banco".into(),
            account,
            label: name,
            currency,
            amount,
            amount_source: source.into(),
            booked,
            valued: None,
            difference: None,
        });
    }

    // Clientes y proveedores: vencimientos de facturas en divisa pendientes al cierre.
    let mut fx_cache = FxTotals::new();
    for i in crate::cartera::list(v, end)? {
        let pending_at_end = i.status == "pendiente" || i.settled_on.is_some_and(|d| d > end);
        if i.currency.is_none() || i.issue_date > end || !pending_at_end || i.invoice_id < 0 {
            continue;
        }
        let Some(account) = i.party_account.clone() else {
            continue;
        };
        let Some((currency, total_fx)) = fx_of(conn, &mut fx_cache, &i.kind, i.invoice_id)? else {
            continue;
        };
        let amount = installment_fx(total_fx, i.amount, i.invoice_amount);
        let plazo = if i.installments > 1 {
            format!(" (plazo {}/{})", i.seq, i.installments)
        } else {
            String::new()
        };
        items.push(PartidaDivisa {
            kind: if i.kind == "emitida" {
                "cliente"
            } else {
                "proveedor"
            }
            .into(),
            account,
            label: format!("{}{plazo} · {}", i.number, i.party),
            currency,
            amount,
            amount_source: "factura".into(),
            booked: i.amount,
            valued: None,
            difference: None,
        });
    }

    // Tipos de cierre.
    let cache = crate::fx::cache_conn(conn)?;
    let mut currencies: Vec<String> = items.iter().map(|i| i.currency.clone()).collect();
    currencies.sort();
    currencies.dedup();
    let rates: Vec<TipoCierre> = currencies
        .into_iter()
        .map(|c| match input.tipos.get(&c) {
            Some(r) => TipoCierre {
                currency: c,
                rate: Some(*r),
                date: Some(end),
                source: Some("manual".into()),
            },
            None => match crate::fx::rate_in(&cache, &c, end) {
                Some(r) => TipoCierre {
                    currency: c,
                    rate: Some(r.rate),
                    date: Some(r.date),
                    source: Some("bce".into()),
                },
                None => TipoCierre {
                    currency: c,
                    rate: None,
                    date: None,
                    source: None,
                },
            },
        })
        .collect();
    for r in rates.iter().filter(|r| r.rate.is_none()) {
        warnings.push(format!(
            "Falta el tipo de cierre del {}: pulsa «Tipo del BCE del {end}» o escríbelo.",
            r.currency
        ));
    }

    let mut gains = Money::ZERO;
    let mut losses = Money::ZERO;
    // Ajuste de cada subcuenta (debe − haber).
    let mut adjust: BTreeMap<String, Money> = BTreeMap::new();
    for item in &mut items {
        let Some(rate) = rates
            .iter()
            .find(|r| r.currency == item.currency)
            .and_then(|r| r.rate)
        else {
            continue;
        };
        let valued = rate.to_eur(item.amount);
        // Activo (banco, cliente): a favor si vale más; pasivo (proveedor): si se debe menos.
        let (difference, net) = if item.kind == "proveedor" {
            (item.booked - valued, item.booked - valued)
        } else {
            (valued - item.booked, valued - item.booked)
        };
        item.valued = Some(valued);
        item.difference = Some(difference);
        if difference.is_negative() {
            losses -= difference;
        } else {
            gains += difference;
        }
        *adjust.entry(item.account.clone()).or_default() += net;
    }
    let complete = rates.iter().all(|r| r.rate.is_some());
    let mut lines = Vec::new();
    if complete {
        let name = |a: &str| -> Result<String> {
            Ok(accounting::account_name(conn, a)?.unwrap_or_default())
        };
        for (account, net) in &adjust {
            if net.is_zero() {
                continue;
            }
            lines.push(ProposedLine {
                name: name(account)?,
                account: account.clone(),
                debit: if net.is_negative() { Money::ZERO } else { *net },
                credit: if net.is_negative() {
                    -*net
                } else {
                    Money::ZERO
                },
            });
        }
        if !losses.is_zero() {
            lines.push(ProposedLine {
                account: "668".into(),
                name: name("668")?,
                debit: losses,
                credit: Money::ZERO,
            });
        }
        if !gains.is_zero() {
            lines.push(ProposedLine {
                account: "768".into(),
                name: name("768")?,
                debit: Money::ZERO,
                credit: gains,
            });
        }
    }
    let stale = match entry_id {
        Some(id) => complete && posted(conn, id)? != comparable(&lines),
        None => false,
    };
    if stale {
        warnings.push(
            "Los saldos en divisa han cambiado desde que se contabilizó la valoración: deshazla y \
             vuelve a contabilizarla."
                .into(),
        );
    }
    Ok(Valoracion {
        year,
        date: end,
        reversal_date: end + Duration::days(1),
        closed,
        rates,
        items,
        lines,
        gains,
        losses,
        entry_id,
        reversal_id,
        stale,
        input,
        warnings,
    })
}

fn comparable(lines: &[ProposedLine]) -> Vec<(String, i64, i64)> {
    let mut out: Vec<_> = lines
        .iter()
        .map(|l| (l.account.clone(), l.debit.cents(), l.credit.cents()))
        .collect();
    out.sort();
    out
}

fn posted(conn: &Connection, entry: i64) -> Result<Vec<(String, i64, i64)>> {
    let mut st =
        conn.prepare("SELECT account, debit, credit FROM journal_line WHERE entry_id = ?1")?;
    let mut out = st
        .query_map([entry], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<Vec<(String, i64, i64)>, _>>()?;
    out.sort();
    Ok(out)
}

/// Contabiliza la valoración al cierre de `year` (con confirmación): el asiento del último día
/// del ejercicio y su reversión el primer día del siguiente. Devuelve el id del primero.
pub fn post_valoracion(
    v: &Vault,
    year: i32,
    input: EntradaValoracion,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<i64> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let p = valoracion(v, year, Some(input.clone()))?;
    if p.closed {
        return Err(other(
            "el ejercicio está cerrado: reábrelo para valorar los saldos",
        ));
    }
    if p.entry_id.is_some() || p.reversal_id.is_some() {
        return Err(other(
            "la valoración de ese ejercicio ya está contabilizada: deshazla para rehacerla",
        ));
    }
    if let Some(r) = p.rates.iter().find(|r| r.rate.is_none()) {
        return Err(other(format!("falta el tipo de cierre del {}", r.currency)));
    }
    if p.lines.len() < 2 {
        return Err(other(
            "no hay diferencias de cambio que contabilizar: los saldos en divisa ya están al \
             tipo de cierre",
        ));
    }
    let lines = p
        .lines
        .iter()
        .map(|l| {
            Ok(EntryLine {
                account: code(&l.account)?,
                debit: l.debit,
                credit: l.credit,
                description: None,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let reversed = lines
        .iter()
        .map(|l| EntryLine {
            debit: l.credit,
            credit: l.debit,
            ..l.clone()
        })
        .collect();
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let id = accounting::record_entry(
        &tx,
        &JournalEntry {
            date: p.date,
            concept: format!(
                "Diferencias de cambio {year}: saldos en moneda extranjera al tipo de cierre"
            ),
            origin: Origin::DiferenciasCambio(i64::from(year)),
            lines,
        },
        now,
    )?;
    let rev = accounting::record_entry(
        &tx,
        &JournalEntry {
            date: p.reversal_date,
            concept: format!("Reversión de las diferencias de cambio al cierre de {year}"),
            origin: Origin::DiferenciasCambio(-i64::from(year)),
            lines: reversed,
        },
        now,
    )?;
    tx.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![
            setting_key(year),
            serde_json::to_string(&input).map_err(|e| other(e.to_string()))?
        ],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "closing.fx_valuation",
        &year.to_string(),
        &serde_json::json!({
            "entry": id,
            "reversal": rev,
            "gains": p.gains.cents(),
            "losses": p.losses.cents(),
            "rates": p.rates.iter().map(|r| (r.currency.clone(), r.rate.map(|x| x.decimal()), r.source.clone())).collect::<Vec<_>>(),
            "items": p.items.iter().map(|i| serde_json::json!({
                "account": i.account, "currency": i.currency, "amount": i.amount.cents(),
                "booked": i.booked.cents(), "valued": i.valued.map(Money::cents),
            })).collect::<Vec<_>>(),
        }),
    )?;
    tx.commit()?;
    Ok(id)
}

/// Quita la valoración al cierre de `year` y su reversión (con confirmación; los dos
/// ejercicios han de estar abiertos).
pub fn undo_valoracion(v: &Vault, year: i32, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let ids: Vec<i64> = [
        entry_of(conn, -i64::from(year))?,
        entry_of(conn, i64::from(year))?,
    ]
    .into_iter()
    .flatten()
    .collect();
    if ids.is_empty() {
        return Err(AppError::NotFound);
    }
    let tx = conn.unchecked_transaction()?;
    for id in &ids {
        // Los disparadores impiden borrar en un ejercicio cerrado: el error lo explica.
        tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [id])
            .map_err(|_| {
                other("el ejercicio de la valoración o de su reversión está cerrado: reábrelo")
            })?;
        tx.execute("DELETE FROM journal_entry WHERE id = ?1", [id])?;
    }
    tx.execute("DELETE FROM settings WHERE key = ?1", [setting_key(year)])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "closing.fx_valuation_undo",
        &year.to_string(),
        &serde_json::json!({ "entries": ids }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Punto de la lista del cierre: `None` si no hay partidas en divisa; si no, (hecho, detalle).
pub(crate) fn estado_cierre(v: &Vault, year: i32) -> Result<Option<(bool, String)>> {
    let p = valoracion(v, year, None)?;
    if p.items.is_empty() && p.entry_id.is_none() {
        return Ok(None);
    }
    let nothing = p.rates.iter().all(|r| r.rate.is_some()) && p.lines.len() < 2;
    Ok(Some(if p.entry_id.is_some() && !p.stale {
        (
            true,
            format!(
                "Contabilizada al tipo de cierre (768: {}, 668: {}) y revertida el {}.",
                p.gains, p.losses, p.reversal_date
            ),
        )
    } else if p.stale {
        (
            false,
            "Los saldos han cambiado desde que se valoraron: deshaz la valoración y rehazla \
             (abajo)."
                .into(),
        )
    } else if nothing {
        (
            true,
            "Los saldos en divisa ya están al tipo de cierre.".into(),
        )
    } else {
        (
            false,
            format!(
                "{} en moneda extranjera sin valorar al tipo de cierre (abajo).",
                if p.items.len() == 1 {
                    "1 saldo".to_owned()
                } else {
                    format!("{} saldos", p.items.len())
                }
            ),
        )
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tipo_implicito_y_parte_en_divisa() {
        // 1.000 USD por 900 € → 1,111111.
        let r = implied_rate(Money::from_euros(1000, 0), Money::from_euros(900, 0)).unwrap();
        assert_eq!(r.decimal(), "1.111111");
        assert!(implied_rate(Money::from_euros(1, 0), Money::ZERO).is_none());
        // Un plazo de 300 € de una factura de 900 € (1.000 USD): 333,33 USD.
        assert_eq!(
            installment_fx(
                Money::from_euros(1000, 0),
                Money::from_euros(300, 0),
                Money::from_euros(900, 0)
            ),
            Money::from_cents(33_333)
        );
        assert_eq!(
            installment_fx(
                Money::from_euros(1000, 0),
                Money::from_euros(900, 0),
                Money::from_euros(900, 0)
            ),
            Money::from_euros(1000, 0)
        );
        assert_eq!(
            fmt_divisa(Money::from_cents(123_456), "USD"),
            "1.234,56 USD"
        );
    }
}
