//! Banco: importación de extractos (Norma 43, camt y, con [`crate::extractos::importar`],
//! cualquier otro formato) y conciliación con facturas o cuentas.
//!
//! Las cuentas son de una divisa ([`BankTx::currency`]): los importes de sus movimientos van en
//! ella. Los asientos van en euros: los movimientos de una cuenta en otra divisa se contabilizan
//! por su contravalor ([`crate::divisa`]: el tecleado, el del extracto o el del tipo del BCE de
//! su día ya descargado), que se guarda en el movimiento al conciliarlo; sin él, no se concilian.
//! Las tarjetas de débito sólo informan: sus compras ya salen una a una en la cuenta, así que ni
//! se proponen ni se concilian.

use serde::Serialize;
use taxpain_core::accounting::{AccountCode, EntryLine, JournalEntry, Origin};
use taxpain_core::bank::{N43Account, decode, parse_n43};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};

use crate::accounting::{posting_accounts, record_entry, third_party_account};
use crate::error::{AppError, Result};
use crate::extractos::desde_n43;
use crate::extractos::importar::{self, Entrada};

/// `kind` de las cuentas que sólo informan (tarjetas de débito).
pub(crate) const INFORMATIVE: &str = "tarjeta_debito";

/// Condición SQL, sobre el movimiento `t`, de los que ya están contabilizados en otro sitio:
/// los de días que ya cubre un diario importado (del gestor, de a3…, o un ejercicio entero del
/// gestor) con asientos de tesorería
/// (57x) y los de ejercicios cerrados. Se guardan para consultarlos y para no duplicar
/// extractos, pero no se proponen ni se concilian (se contabilizarían dos veces) ni cuentan
/// como pendientes.
pub(crate) const BOOKED_ELSEWHERE: &str = "(t.op_date <= COALESCE((SELECT max(e.date)
      FROM journal_entry e WHERE e.origin_kind IN ('Importado', 'Gestor') AND EXISTS (SELECT 1
      FROM journal_line l WHERE l.entry_id = e.id AND l.account LIKE '57%')), '')
    OR EXISTS (SELECT 1 FROM fiscal_year f WHERE f.closed = 1
      AND t.op_date BETWEEN f.start_date AND f.end_date))";

/// Días de margen entre las dos caras de un traspaso entre cuentas propias (el cargo de la
/// liquidación de una tarjeta y su abono en la tarjeta pueden ir con uno o dos días de
/// diferencia).
const TRANSFER_DAYS: i64 = 5;

/// Días de margen entre la fecha anotada de un recibo devuelto (o una transferencia rechazada)
/// de una remesa y el movimiento del banco que la refleja.
const RETURN_DAYS: i64 = 10;

fn now_str(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

fn parse_date(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]"))
        .map_err(|e| AppError::Other(e.to_string()))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ImportSummary {
    pub accounts: usize,
    pub new_movements: usize,
    pub duplicates: usize,
}

/// Importa un extracto Norma 43. Reimportar el mismo fichero no duplica movimientos, ni
/// importar después el mismo periodo en otro formato (PDF, Excel…).
pub fn import_n43(v: &Vault, bytes: &[u8], now: OffsetDateTime) -> Result<ImportSummary> {
    let accounts =
        parse_n43(&decode(bytes)).map_err(|e| AppError::Other(format!("Norma 43: {e}")))?;
    import_accounts(v, "Norma 43", bytes, &accounts, "Norma 43", false, now).map(summary)
}

fn summary(r: importar::Resumen) -> ImportSummary {
    ImportSummary {
        accounts: r.extractos.len(),
        new_movements: r.movimientos_nuevos,
        duplicates: r.ya_estaban,
    }
}

/// Como [`import_statement`], con el nombre del fichero y el resumen de cada cuenta. Con
/// `forzar` entran también las cuentas que no cuadran con sus saldos («Importar igualmente»).
pub(crate) fn import_statement_named(
    v: &Vault,
    nombre: &str,
    bytes: &[u8],
    forzar: bool,
    now: OffsetDateTime,
) -> Result<importar::Resumen> {
    let text = decode(bytes);
    if text
        .trim_start_matches('\u{feff}')
        .trim_start()
        .starts_with('<')
    {
        let accounts = taxpain_core::camt::parse_camt(&text)
            .map_err(|e| AppError::Other(format!("camt: {e}")))?;
        return import_accounts(v, nombre, bytes, &accounts, "camt.053", forzar, now);
    }
    let accounts =
        parse_n43(&decode(bytes)).map_err(|e| AppError::Other(format!("Norma 43: {e}")))?;
    import_accounts(v, nombre, bytes, &accounts, "Norma 43", forzar, now)
}

/// Importa un extracto Norma 43 o ISO 20022 (camt.052/053/054), según su contenido.
pub fn import_statement(v: &Vault, bytes: &[u8], now: OffsetDateTime) -> Result<ImportSummary> {
    let text = decode(bytes);
    if text
        .trim_start_matches('\u{feff}')
        .trim_start()
        .starts_with('<')
    {
        let accounts = taxpain_core::camt::parse_camt(&text)
            .map_err(|e| AppError::Other(format!("camt: {e}")))?;
        return import_accounts(v, "camt.053", bytes, &accounts, "camt.053", false, now)
            .map(summary);
    }
    import_n43(v, bytes, now)
}

/// Importa las cuentas de un Norma 43 o camt con el importador de todos los extractos (así no
/// se duplican con los de otros formatos). Una cuenta que no cuadra con sus saldos se queda sin
/// importar, con su motivo, como en los demás formatos (el Norma 43 ya no llega aquí: su lector
/// rechaza el fichero; un camt no valida los saldos), salvo con `forzar`.
fn import_accounts(
    v: &Vault,
    nombre: &str,
    bytes: &[u8],
    accounts: &[N43Account],
    format: &str,
    forzar: bool,
    now: OffsetDateTime,
) -> Result<importar::Resumen> {
    let read: Vec<(crate::extractos::Leido, Option<Vec<String>>)> = accounts
        .iter()
        .map(|a| {
            let e = desde_n43(a, format);
            let before = e.movimientos.clone();
            let leido = importar::comprobado(e);
            // Los códigos de concepto común van en el orden del fichero: sólo si no se ha
            // reordenado nada.
            let codes = (leido.extracto.movimientos == before).then(|| {
                a.movements
                    .iter()
                    .map(|m| m.common_concept.clone())
                    .collect()
            });
            (leido, codes)
        })
        .collect();
    let entries: Vec<Entrada<'_>> = read
        .iter()
        .map(|(leido, codes)| Entrada {
            leido,
            codigos: codes.clone(),
            ids_externos: false,
        })
        .collect();
    importar::importar_entradas(v, nombre, bytes, &entries, None, forzar, now)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BankTx {
    pub id: i64,
    pub bank_account: String,
    pub op_date: String,
    /// En la divisa de la cuenta (`currency`).
    pub amount: Money,
    pub description: String,
    pub matched: Option<String>,
    /// Divisa de la cuenta (ISO 4217).
    pub currency: String,
    /// `cuenta`, `tarjeta_credito`, `tarjeta_debito` (sólo informa) o `monedero`.
    pub account_kind: String,
    /// Saldo después del movimiento, si el extracto lo daba.
    pub balance: Option<Money>,
    /// De un periodo ya contabilizado en otro sitio (diario importado o ejercicio cerrado):
    /// no se concilia.
    pub booked_elsewhere: bool,
    /// Cuentas en otra divisa: contravalor en euros (el guardado o, si no, el del tipo del BCE
    /// de su día ya descargado), tipo y de dónde sale (`bce`, `manual`, `traspaso`).
    pub amount_eur: Option<Money>,
    pub fx_rate: Option<taxpain_core::fx::ExchangeRate>,
    pub fx_source: Option<String>,
}

/// Movimientos del banco, del más nuevo al más antiguo. Con `only_pending`, los que falta
/// conciliar (sin los de las tarjetas de débito, que sólo informan, ni los ya contabilizados
/// en otro sitio).
pub fn movements(v: &Vault, only_pending: bool) -> Result<Vec<BankTx>> {
    let conn = v.connection();
    let mut stmt = conn.prepare(&format!(
        "SELECT t.id, b.name, t.op_date, t.amount, t.description, t.matched_kind, b.currency,
                b.kind, t.balance, {BOOKED_ELSEWHERE}, t.amount_eur, t.fx_rate, t.fx_date,
                t.fx_source FROM bank_tx t
         JOIN bank_account b ON b.id = t.bank_account
         WHERE (?1 = 0 OR (t.matched_kind IS NULL AND b.kind <> ?2 AND NOT {BOOKED_ELSEWHERE}))
         ORDER BY t.op_date DESC, t.id DESC"
    ))?;
    let cache = crate::fx::cache_conn(conn)?;
    let rows = stmt.query_map(params![i64::from(only_pending), INFORMATIVE], |r| {
        let stored = crate::divisa::stored_of(
            r.get(10)?,
            r.get(11)?,
            r.get::<_, Option<String>>(12)?.as_deref(),
            r.get(13)?,
            time::Date::MIN,
        );
        Ok((
            BankTx {
                id: r.get(0)?,
                bank_account: r.get(1)?,
                op_date: r.get(2)?,
                amount: Money::from_cents(r.get(3)?),
                description: r.get(4)?,
                matched: r.get(5)?,
                currency: r.get(6)?,
                account_kind: r.get(7)?,
                balance: r.get::<_, Option<i64>>(8)?.map(Money::from_cents),
                booked_elsewhere: r.get(9)?,
                amount_eur: None,
                fx_rate: None,
                fx_source: None,
            },
            stored,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (mut t, stored) = row?;
        if t.currency != "EUR" {
            let c = match stored {
                Some(c) => Some(c),
                None => crate::divisa::from_cache(
                    &cache,
                    &t.currency,
                    t.amount,
                    parse_date(&t.op_date)?,
                ),
            };
            if let Some(c) = c {
                t.amount_eur = Some(c.amount_eur);
                t.fx_rate = Some(c.rate);
                t.fx_source = Some(c.source);
            }
        }
        out.push(t);
    }
    Ok(out)
}

/// Borra un movimiento que sobra: uno que la importación no ha podido saber que era el mismo
/// que otro (un cargo pendiente de tarjeta que luego cambió de importe, por ejemplo; la
/// importación avisa de los parecidos). Sólo si no está conciliado ni en una remesa o un pago
/// de un modelo. Queda en la auditoría.
pub fn delete_movement(v: &Vault, tx_id: i64, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    type Fila = (String, i64, String, Option<i64>, bool, i64, Option<String>);
    let (date, amount, desc, entry, used, cuenta, externo): Fila = tx
        .query_row(
            "SELECT op_date, amount, description, entry_id,
                    EXISTS(SELECT 1 FROM remittance r WHERE r.bank_tx_id = t.id)
                    OR EXISTS(SELECT 1 FROM tax_payment p WHERE p.bank_tx = t.id),
                    bank_account, external_id
             FROM bank_tx t WHERE id = ?1",
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
                ))
            },
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    if entry.is_some() || used {
        return Err(AppError::Other(
            "el movimiento está conciliado: no se puede borrar".into(),
        ));
    }
    tx.execute("DELETE FROM bank_tx WHERE id = ?1", [tx_id])?;
    // Si llegó por una conexión con el banco, que la descarga siguiente no lo vuelva a meter.
    if let Some(e) = &externo {
        tx.execute(
            "INSERT OR IGNORE INTO bank_tx_ignored(bank_account, external_id, deleted_at)
             VALUES (?1, ?2, ?3)",
            params![cuenta, e, now_str(now)],
        )?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "bank.delete",
        &tx_id.to_string(),
        &serde_json::json!({ "date": date, "amount": amount, "description": desc }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Vencimiento candidato a casar con un movimiento.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Suggestion {
    /// `emitida` o `recibida`.
    pub kind: String,
    pub id: i64,
    /// Vencimiento (1, 2…) y cuántos tiene la factura.
    pub seq: i64,
    pub installments: i64,
    pub number: String,
    pub party: String,
    pub date: String,
    pub due_date: String,
    pub amount: Money,
}

/// Vencimientos pendientes de cobro (ingresos) o de pago (cargos) con el mismo importe que el
/// movimiento, de facturas de esa fecha o anteriores; primero los que vencen más cerca de él.
///
/// En una cuenta en otra divisa se proponen las facturas en esa divisa por su importe en ella
/// (lo pendiente en divisa, con 2 céntimos de margen por el reparto en plazos) y, si se sabe el
/// contravalor del movimiento, las demás cuyos euros se separan como mucho un 10 % de él.
pub fn suggestions(v: &Vault, tx_id: i64) -> Result<Vec<Suggestion>> {
    let today = OffsetDateTime::now_utc().date();
    let remesas = crate::remesas::unmatched(v.connection())?;
    suggestions_from(v, tx_id, &crate::cartera::list(v, today)?, &remesas)
}

/// [`suggestions`] con la cartera y las remesas sin apuntar ya cargadas: quien recorre muchos
/// movimientos (las propuestas por regla) las carga una vez en vez de una consulta por
/// movimiento.
pub(crate) fn suggestions_from(
    v: &Vault,
    tx_id: i64,
    cartera: &[crate::cartera::DueItem],
    remesas_sin_apuntar: &[crate::remesas::Remittance],
) -> Result<Vec<Suggestion>> {
    let conn = v.connection();
    let (amount, date, currency, kind, booked): (i64, String, String, String, bool) = conn
        .query_row(
            &format!(
                "SELECT t.amount, t.op_date, b.currency, b.kind, {BOOKED_ELSEWHERE} FROM bank_tx t
                 JOIN bank_account b ON b.id = t.bank_account WHERE t.id = ?1"
            ),
            [tx_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )?;
    if kind == INFORMATIVE || booked {
        return Ok(Vec::new());
    }
    let date = parse_date(&date)?;
    if currency != "EUR" {
        return foreign_suggestions(conn, tx_id, &currency, amount, date, cartera);
    }
    // Remesas SEPA cuyo total coincide (el banco abona o carga la remesa entera).
    let mut remesas: Vec<Suggestion> = crate::remesas::matching_in(remesas_sin_apuntar, amount)
        .into_iter()
        .map(|r| Suggestion {
            kind: "remesa".into(),
            id: r.id,
            seq: 0,
            installments: i64::try_from(r.items.iter().filter(|i| i.returned_on.is_none()).count())
                .unwrap_or(0),
            number: r.reference,
            party: if r.kind == "cobros" {
                "Remesa de recibos".into()
            } else {
                "Orden de transferencias".into()
            },
            date: r.charge_date.to_string(),
            due_date: r.charge_date.to_string(),
            amount: r.total,
        })
        .collect();
    // Recibos devueltos de una remesa ya anotados (con su asiento) cuyo movimiento falta.
    remesas.extend(return_suggestions(conn, tx_id, amount, date)?);
    let mut out: Vec<Suggestion> = cartera
        .iter()
        .filter(|i| {
            // Con signo: el cobro de una emitida es un ingreso y el pago de una recibida, un
            // cargo; en una rectificativa en negativo (abono) es al revés (el abono se devuelve
            // al cliente con un cargo; el de un proveedor llega como ingreso).
            let expected = if i.kind == "emitida" {
                i.amount
            } else {
                -i.amount
            };
            // En moneda extranjera el banco abona o carga otros euros (el tipo del día):
            // se proponen las que se separan como mucho un 10 % de lo contabilizado.
            let close = expected.cents() == amount
                || (i.currency.is_some()
                    && expected.is_negative() == (amount < 0)
                    && crate::cartera::within_exchange_tolerance(
                        expected.abs(),
                        Money::from_cents(amount.abs()),
                    ));
            i.status == "pendiente" && close && i.issue_date <= date
        })
        .map(|i| Suggestion {
            kind: i.kind.clone(),
            id: i.invoice_id,
            seq: i.seq,
            installments: i.installments,
            number: i.number.clone(),
            party: i.party.clone(),
            date: i.issue_date.to_string(),
            due_date: i.due_date.to_string(),
            amount: i.amount,
        })
        .collect();
    out.sort_by_key(|s| {
        parse_date(&s.due_date)
            .map(|d| (d - date).whole_days().abs())
            .unwrap_or(i64::MAX)
    });
    remesas.append(&mut out);
    let out = remesas;
    Ok(out)
}

/// Facturas pendientes que casan con un movimiento de una cuenta en divisa (ver
/// [`suggestions`]).
fn foreign_suggestions(
    conn: &Connection,
    tx_id: i64,
    currency: &str,
    amount: i64,
    date: Date,
    cartera: &[crate::cartera::DueItem],
) -> Result<Vec<Suggestion>> {
    let eur = crate::divisa::contravalor(conn, tx_id)?.map(|c| c.amount_eur);
    let mut fx = crate::divisa::FxTotals::new();
    let mut out = Vec::new();
    for i in cartera {
        if i.status != "pendiente" || i.issue_date > date {
            continue;
        }
        // Con signo: el cobro de una emitida es un ingreso y el pago de una recibida, un cargo.
        let expected = if i.kind == "emitida" {
            i.amount
        } else {
            -i.amount
        };
        if expected.is_negative() != (amount < 0) {
            continue;
        }
        let same = match &i.currency {
            Some(c) if c == currency => crate::divisa::fx_of(conn, &mut fx, &i.kind, i.invoice_id)?
                .map(|(_, total)| crate::divisa::installment_fx(total, i.amount, i.invoice_amount)),
            _ => None,
        };
        let close = match (same, eur) {
            (Some(fx_amount), _) => (fx_amount.cents().abs() - amount.abs()).abs() <= 2,
            (None, Some(e)) => crate::cartera::within_exchange_tolerance(expected.abs(), e.abs()),
            (None, None) => false,
        };
        if close {
            out.push(Suggestion {
                kind: i.kind.clone(),
                id: i.invoice_id,
                seq: i.seq,
                installments: i.installments,
                number: i.number.clone(),
                party: i.party.clone(),
                date: i.issue_date.to_string(),
                due_date: i.due_date.to_string(),
                amount: i.amount,
            });
        }
    }
    out.sort_by_key(|s| {
        parse_date(&s.due_date)
            .map(|d| (d - date).whole_days().abs())
            .unwrap_or(i64::MAX)
    });
    Ok(out)
}

/// Importe de un movimiento en la divisa de su cuenta, en céntimos.
fn in_currency(conn: &Connection, tx_id: i64) -> Result<i64> {
    Ok(
        conn.query_row("SELECT amount FROM bank_tx WHERE id = ?1", [tx_id], |r| {
            r.get(0)
        })?,
    )
}

/// Divisa de la cuenta de un movimiento.
fn tx_currency(conn: &Connection, tx_id: i64) -> Result<String> {
    Ok(conn.query_row(
        "SELECT b.currency FROM bank_tx t JOIN bank_account b ON b.id = t.bank_account
         WHERE t.id = ?1",
        [tx_id],
        |r| r.get(0),
    )?)
}

/// Fecha, importe en euros, concepto y subcuenta de un movimiento que se va a conciliar. En una
/// cuenta en otra divisa, el importe es su contravalor (que queda guardado en el movimiento).
/// Error si ya está conciliado, si es de una tarjeta de débito (sólo informa) o si es de una
/// cuenta en otra divisa y no se sabe su contravalor.
fn tx_data(conn: &Connection, tx_id: i64) -> Result<(Date, Money, String, AccountCode)> {
    let (date, amount, desc, bank, currency) = tx_data_raw(conn, tx_id)?;
    let amount = if currency == "EUR" {
        amount
    } else {
        crate::divisa::fix(conn, tx_id)?
    };
    Ok((date, amount, desc, bank))
}

/// Como [`tx_data`], con el importe en la divisa de la cuenta y la divisa.
fn tx_data_raw(
    conn: &Connection,
    tx_id: i64,
) -> Result<(Date, Money, String, AccountCode, String)> {
    type Row = (
        String,
        i64,
        String,
        String,
        Option<String>,
        String,
        String,
        bool,
    );
    let (date, amount, desc, acc, matched, currency, kind, booked): Row = conn
        .query_row(
            &format!(
                "SELECT t.op_date, t.amount, t.description, b.account, t.matched_kind, b.currency,
                        b.kind, {BOOKED_ELSEWHERE} FROM bank_tx t
                 JOIN bank_account b ON b.id = t.bank_account WHERE t.id = ?1"
            ),
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
    if matched.is_some() {
        return Err(AppError::Other("el movimiento ya está conciliado".into()));
    }
    if booked {
        return Err(AppError::Other(
            "ese movimiento es de un periodo ya contabilizado (un diario importado o un \
             ejercicio cerrado): no se vuelve a contabilizar"
                .into(),
        ));
    }
    if kind == INFORMATIVE {
        return Err(AppError::Other(
            "es una compra con tarjeta de débito: ya sale en la cuenta del banco, concilia allí \
             su cargo"
                .into(),
        ));
    }
    Ok((
        parse_date(&date)?,
        Money::from_cents(amount),
        desc,
        AccountCode::new(&acc).map_err(|e| AppError::Other(e.to_string()))?,
        currency,
    ))
}

fn finish(
    conn: &Connection,
    tx_id: i64,
    entry: &JournalEntry,
    kind: &str,
    matched: Option<(i64, Option<i64>)>,
    now: OffsetDateTime,
) -> Result<i64> {
    let entry_id = record_entry(conn, entry, now)?;
    let (matched_id, matched_seq) = match matched {
        Some((id, seq)) => (Some(id), seq),
        None => (None, None),
    };
    conn.execute(
        "UPDATE bank_tx SET entry_id = ?1, matched_kind = ?2, matched_id = ?3, matched_seq = ?4 WHERE id = ?5",
        params![entry_id, kind, matched_id, matched_seq, tx_id],
    )?;
    audit::append(
        conn,
        &now_str(now),
        Actor::User,
        "bank.reconcile",
        &tx_id.to_string(),
        &serde_json::json!({ "kind": kind, "id": matched_id, "seq": matched_seq }),
    )?;
    Ok(entry_id)
}

/// Subcuenta de tercero (con los prefijos dados) usada en el asiento de una factura.
fn posted_party_account(
    conn: &Connection,
    origin_kind: &str,
    invoice_id: i64,
    prefixes: &[&str],
) -> Result<Option<AccountCode>> {
    let mut stmt = conn.prepare(
        "SELECT l.account FROM journal_line l JOIN journal_entry e ON e.id = l.entry_id
         WHERE e.origin_kind = ?1 AND e.origin_id = ?2 ORDER BY l.id",
    )?;
    let accounts: Vec<String> = stmt
        .query_map(params![origin_kind, invoice_id], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    accounts
        .into_iter()
        .find(|a| prefixes.iter().any(|p| a.starts_with(p)))
        .map(|a| AccountCode::new(&a))
        .transpose()
        .map_err(|e| AppError::Other(e.to_string()))
}

/// Subcuenta del cliente o proveedor de una factura (la de su asiento) y el concepto del cobro
/// o pago. `kind` es `emitida` o `recibida`.
pub(crate) fn invoice_party(
    tx: &Connection,
    kind: &str,
    invoice_id: i64,
) -> Result<(AccountCode, String)> {
    if invoice_id < 0 {
        // Pendiente de antes de TaxPain (saldos iniciales).
        return crate::apertura::cartera_party(tx, kind, invoice_id);
    }
    let a = posting_accounts(tx)?;
    Ok(match kind {
        "emitida" => {
            let (tax_id, name, number): (Option<String>, Option<String>, String) = tx
                .query_row(
                    "SELECT recipient_tax_id, recipient_name, number FROM invoice_issued
                     WHERE id = ?1 AND status = 'emitida'",
                    [invoice_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?
                .ok_or_else(|| AppError::Other("la factura no existe o está anulada".into()))?;
            let (tax_id, name) = match (tax_id, name) {
                (Some(t), Some(n)) => (t, n),
                _ => ("VARIOS".into(), "Clientes varios".into()),
            };
            // La subcuenta del cliente es la del asiento de la factura (así coincide también
            // con clientes extranjeros, cuya clave incluye el país).
            let account = match posted_party_account(tx, "FacturaEmitida", invoice_id, &["43"])? {
                Some(acc) => acc,
                None => third_party_account(
                    tx,
                    &tax_id,
                    "cliente",
                    &name,
                    &a.customers,
                    a.subaccount_width,
                )?,
            };
            (account, format!("Cobro factura {number}"))
        }
        "recibida" => {
            let (nif, name, number): (String, String, String) = tx.query_row(
                "SELECT COALESCE(supplier_nif, ''), COALESCE(supplier_name, ''), number FROM invoice_received WHERE id = ?1 AND status = 'revisada'",
                [invoice_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )?;
            let account =
                match posted_party_account(tx, "FacturaRecibida", invoice_id, &["40", "41"])? {
                    Some(acc) => acc,
                    None => third_party_account(
                        tx,
                        &nif,
                        "proveedor",
                        &name,
                        &a.suppliers,
                        a.subaccount_width,
                    )?,
                };
            (account, format!("Pago factura {number} {name}"))
        }
        _ => return Err(AppError::Other("tipo de conciliación no válido".into())),
    })
}

/// Concilia un movimiento con una factura: cobro (572 a 430) o pago (400/410 a 572). Con `seq`,
/// con ese vencimiento; sin él, con la factura entera (todos sus vencimientos pendientes).
///
/// Si la factura es en moneda extranjera y el banco abona o carga otros euros que los
/// contabilizados, el cliente o proveedor se salda por lo contabilizado y la diferencia va a
/// 768 o 668 (diferencias de cambio, PGC NRV 11.ª). Igual si la cuenta del banco es en otra
/// divisa: la tesorería va por el contravalor del movimiento y la diferencia con los euros
/// contabilizados de la factura, a 768/668.
pub fn reconcile_invoice(
    v: &Vault,
    tx_id: i64,
    kind: &str,
    invoice_id: i64,
    seq: Option<i64>,
    now: OffsetDateTime,
) -> Result<i64> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let (date, amount, desc, bank) = tx_data(&tx, tx_id)?;
    let account_currency = tx_currency(&tx, tx_id)?;
    let foreign_account = account_currency != "EUR";
    // Cada vencimiento se concilia una sola vez y en su sentido: un cobro es un ingreso en el
    // banco y un pago, un cargo.
    let items = crate::cartera::installments(&tx, kind, invoice_id)?;
    let seq = match seq {
        Some(s) => {
            let item = items
                .iter()
                .find(|i| i.seq == s)
                .ok_or_else(|| AppError::Other("ese vencimiento no existe".into()))?;
            match item.status.as_str() {
                "banco" => {
                    return Err(AppError::Other(
                        "ese vencimiento ya está conciliado con otro movimiento".into(),
                    ));
                }
                "registrada" => {
                    return Err(AppError::Other(
                        "ese vencimiento ya tiene registrado su cobro o pago en la cartera".into(),
                    ));
                }
                _ => {}
            }
            crate::cartera::freeze_invoice(&tx, kind, invoice_id)?;
            Some(s)
        }
        None => {
            // Con algo ya cobrado o pagado (un cobro parcial, p. ej.) se concilia lo pendiente;
            // en moneda extranjera hay que elegir el vencimiento.
            let open = items.iter().any(|i| i.status == "pendiente");
            let foreign = foreign_account || items.iter().any(|i| i.currency.is_some());
            if items.iter().any(|i| i.status == "banco") && (!open || foreign) {
                return Err(AppError::Other(
                    "esa factura ya está conciliada con otro movimiento".into(),
                ));
            }
            if items.iter().any(|i| i.status == "registrada") && (!open || foreign) {
                return Err(AppError::Other(
                    "esa factura ya tiene registrado su cobro o pago en la cartera".into(),
                ));
            }
            None
        }
    };
    // El sentido lo marca el signo del vencimiento, no sólo el tipo: una rectificativa en
    // negativo (abono) se devuelve al cliente con un cargo y la de un proveedor llega como
    // ingreso.
    let booked: Money = items
        .iter()
        .filter(|i| seq.is_none_or(|s| i.seq == s))
        .map(|i| i.amount)
        .sum();
    let credit_note = booked.is_negative();
    let income = amount.cents() > 0;
    if amount.is_zero() {
        return Err(AppError::Other("el movimiento no tiene importe".into()));
    }
    match (kind, credit_note, income) {
        ("emitida", false, false) => {
            return Err(AppError::Other(
                "un cargo no puede ser el cobro de una factura emitida".into(),
            ));
        }
        ("emitida", true, true) => {
            return Err(AppError::Other(
                "la devolución de un abono al cliente es un cargo, no un ingreso".into(),
            ));
        }
        ("recibida", false, true) => {
            return Err(AppError::Other(
                "un ingreso no puede ser el pago de una factura recibida".into(),
            ));
        }
        ("recibida", true, false) => {
            return Err(AppError::Other(
                "el abono de un proveedor llega como ingreso, no como cargo".into(),
            ));
        }
        _ => {}
    }
    let (party_account, mut concept) = invoice_party(&tx, kind, invoice_id)?;
    // El vencimiento elegido; el «(s/n)» del concepto se pone al final, tras dividir.
    let chosen = seq;
    // Moneda extranjera: lo contabilizado de lo que se concilia (el vencimiento o los
    // pendientes) frente a lo que mueve el banco (cobro positivo, pago con el signo cambiado).
    let collect = kind == "emitida";
    let pending: Vec<&crate::cartera::DueItem> = items
        .iter()
        .filter(|i| seq.is_none_or(|s| i.seq == s) && i.status == "pendiente")
        .collect();
    let (party_amount, difference) = match pending.first() {
        Some(first) if first.currency.is_some() || foreign_account => {
            let booked: Money = pending.iter().map(|i| i.amount).sum();
            let actual = if collect { amount } else { -amount };
            // En una cuenta de la misma divisa que la factura, si el movimiento es lo pendiente
            // en divisa, es el cobro o pago entero: la diferencia es de cambio por grande que
            // sea (el tope del 10 % es para los importes en euros, que no dicen si es parcial).
            let whole = foreign_account
                && first.currency.as_deref() == Some(account_currency.as_str())
                && crate::divisa::fx_of(
                    &tx,
                    &mut crate::divisa::FxTotals::new(),
                    kind,
                    invoice_id,
                )?
                .is_some_and(|(_, total)| {
                    let due = crate::divisa::installment_fx(total, booked, first.invoice_amount);
                    (due.cents().abs() - in_currency(&tx, tx_id).unwrap_or(0).abs()).abs() <= 2
                });
            if !whole {
                crate::cartera::check_exchange(
                    &crate::cartera::DueItem {
                        amount: booked,
                        currency: first
                            .currency
                            .clone()
                            .or_else(|| Some(account_currency.clone())),
                        ..(*first).clone()
                    },
                    actual,
                )?;
            }
            (
                if collect { booked } else { -booked },
                crate::cartera::exchange_difference(collect, booked, actual),
            )
        }
        _ => (amount, None),
    };
    // En euros, un movimiento por menos de lo pendiente (más de 1 céntimo) es un cobro o pago
    // parcial: esa parte pasa a ser un vencimiento propio, conciliado con el movimiento, y lo
    // que falta sigue pendiente. La factura queda cobrada o pagada sólo cuando todo lo está.
    let mut seq = seq;
    let mut party_amount = party_amount;
    let mut difference = difference;
    let mut split = false;
    let mut excess_note = None;
    let mut partial_note = None;
    if !foreign_account && pending.first().is_some_and(|p| p.currency.is_none()) {
        let booked: Money = pending.iter().map(|i| i.amount).sum();
        let actual = if collect { amount } else { -amount };
        // Un céntimo de diferencia es redondeo: el tercero se salda por lo contabilizado y el
        // céntimo va a 669/769 (otros gastos o ingresos financieros).
        // TODO(VERIFY): cuenta del redondeo (669/769, PGC); la gestoría puede preferir 678/778.
        if (booked - actual).cents().abs() == 1 && actual.is_negative() == booked.is_negative() {
            party_amount = if collect { booked } else { -booked };
            let gain = if collect {
                actual - booked
            } else {
                booked - actual
            };
            difference = if gain.is_negative() {
                EntryLine::signed(acc_code("669")?, -gain, true)
            } else {
                EntryLine::signed(acc_code("769")?, gain, false)
            };
        }
        // Un movimiento por más de lo pendiente (más de 1 céntimo): el tercero se salda por lo
        // pendiente, así la cartera y el 430/400 cuadran, y lo de más va a anticipos (438 del
        // cliente, que habrá que devolver o aplicar; 407 del proveedor). El concepto lo dice.
        // TODO(VERIFY): cuenta del exceso (438/407, PGC); la gestoría puede preferir dejarlo en
        // el propio 430/400 o en la 555 hasta aclararlo.
        if let Some(excess) = overpaid(booked, actual) {
            party_amount = if collect { booked } else { -booked };
            let (code, what) = if collect {
                ("438", "cobrado")
            } else {
                ("407", "pagado")
            };
            difference = EntryLine::signed(
                acc_code(code)?,
                if collect { excess } else { -excess },
                false,
            );
            excess_note = Some(format!(
                "{what} de más {}, a la {code}",
                Money::from_cents(excess.cents().abs())
            ));
        }
        let rest = partial_rest(booked, actual);
        // Sin vencimiento y con algo ya cobrado, lo pendiente se junta en un vencimiento (el
        // movimiento casa con él y no con la factura entera).
        let some_done = seq.is_none() && items.iter().any(|i| i.status != "pendiente");
        if rest.is_some() || some_done {
            let parts: Vec<(i64, Date, Money)> = pending
                .iter()
                .map(|i| (i.seq, i.due_date, i.amount))
                .collect();
            let all = crate::cartera::freeze_invoice(&tx, kind, invoice_id)?;
            let paid = if rest.is_some() { actual } else { booked };
            seq = Some(split_partial(&tx, kind, invoice_id, &all, &parts, paid)?);
            split = true;
        }
        partial_note = rest.map(|rest| format!("parcial; quedan {rest}"));
    }
    // «(s/n)» con el vencimiento que casa de verdad y los que hay ahora: tras dividir, el
    // movimiento casa con el vencimiento nuevo y hay uno más. Se numera por fecha de
    // vencimiento y luego por número (los números internos dejan huecos al dividir). Tras
    // dividir, lo cobrado o pagado lleva el número más alto pero va antes que lo que queda del
    // mismo vencimiento, que vence el mismo día.
    if let Some(s) = seq
        && (chosen.is_some() || split)
    {
        let seqs: Vec<i64> = if split {
            let mut st = tx.prepare(
                "SELECT seq FROM invoice_installment WHERE kind = ?1 AND invoice_id = ?2
                 ORDER BY due_date, seq <> ?3, seq",
            )?;
            st.query_map(params![kind, invoice_id, s], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?
        } else {
            let mut x: Vec<(Date, i64)> = items.iter().map(|i| (i.due_date, i.seq)).collect();
            x.sort_unstable();
            x.into_iter().map(|(_, q)| q).collect()
        };
        if seqs.len() > 1
            && let Some(pos) = seqs.iter().position(|x| *x == s)
        {
            concept = format!("{concept} ({}/{})", pos + 1, seqs.len());
        }
    }
    for note in [partial_note, excess_note].into_iter().flatten() {
        concept = format!("{concept} ({note})");
    }
    let entry = JournalEntry {
        date,
        concept: format!("{concept} · {desc}"),
        origin: Origin::Banco(tx_id),
        lines: [
            EntryLine::signed(bank, amount, true),
            EntryLine::signed(party_account, party_amount, false),
            difference,
        ]
        .into_iter()
        .flatten()
        .collect(),
    };
    let id = finish(&tx, tx_id, &entry, kind, Some((invoice_id, seq)), now)?;
    tx.commit()?;
    Ok(id)
}

fn acc_code(c: &str) -> Result<AccountCode> {
    AccountCode::new(c).map_err(|e| AppError::Other(e.to_string()))
}

/// Lo cobrado o pagado de más si `actual` (con el signo de lo contabilizado) supera `booked` en
/// más de 1 céntimo; `None` si no.
pub(crate) fn overpaid(booked: Money, actual: Money) -> Option<Money> {
    if booked.is_zero() || actual.is_negative() != booked.is_negative() {
        return None;
    }
    let excess = actual - booked;
    (!excess.is_zero() && excess.is_negative() == booked.is_negative() && excess.cents().abs() > 1)
        .then_some(excess)
}

/// Lo que queda pendiente si `actual` (con el signo de lo contabilizado) cubre sólo una parte
/// de `booked`; `None` si lo cubre entero (con 1 céntimo de margen por redondeo) o lo supera.
pub(crate) fn partial_rest(booked: Money, actual: Money) -> Option<Money> {
    if actual.is_zero() || actual.is_negative() != booked.is_negative() {
        return None;
    }
    let rest = booked - actual;
    (rest.is_negative() == booked.is_negative() && rest.cents().abs() > 1).then_some(rest)
}

/// Separa de los vencimientos pendientes `parts` (seq, fecha, importe, en su orden) la parte
/// `paid` (mismo signo): los que cubre enteros se juntan en un vencimiento nuevo por `paid`, con
/// la fecha del primero, y el que cubre en parte se queda con lo que falta. Devuelve el número
/// del vencimiento nuevo. `all` son todos los vencimientos de la factura (ya fijados).
fn split_partial(
    tx: &Connection,
    kind: &str,
    invoice_id: i64,
    all: &[crate::cartera::DueItem],
    parts: &[(i64, Date, Money)],
    paid: Money,
) -> Result<i64> {
    let sign = if paid.is_negative() { -1 } else { 1 };
    let mut left = paid.cents().abs();
    for (seq, _, amount) in parts {
        if left == 0 {
            break;
        }
        let a = amount.cents().abs();
        if left >= a {
            tx.execute(
                "DELETE FROM invoice_installment WHERE kind = ?1 AND invoice_id = ?2 AND seq = ?3",
                params![kind, invoice_id, seq],
            )?;
            left -= a;
        } else {
            tx.execute(
                "UPDATE invoice_installment SET amount = ?1
                 WHERE kind = ?2 AND invoice_id = ?3 AND seq = ?4",
                params![sign * (a - left), kind, invoice_id, seq],
            )?;
            left = 0;
        }
    }
    let rows: i64 = tx.query_row(
        "SELECT COUNT(*) FROM invoice_installment WHERE kind = ?1 AND invoice_id = ?2",
        params![kind, invoice_id],
        |r| r.get(0),
    )?;
    if rows >= 24 {
        return Err(AppError::Other(
            "la factura ya tiene demasiados vencimientos para otro cobro o pago parcial".into(),
        ));
    }
    let new_seq = all.iter().map(|i| i.seq).max().unwrap_or(0) + 1;
    let due = parts.first().map_or_else(|| all[0].due_date, |p| p.1);
    tx.execute(
        "INSERT INTO invoice_installment(kind, invoice_id, seq, due_date, amount)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![kind, invoice_id, new_seq, due.to_string(), paid.cents()],
    )?;
    Ok(new_seq)
}

/// Casa un movimiento con el abono o cargo de una remesa SEPA (572 contra 4312 o 555).
pub fn reconcile_remittance(
    v: &Vault,
    tx_id: i64,
    remittance_id: i64,
    now: OffsetDateTime,
) -> Result<i64> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let (date, amount, desc, bank) = tx_data(&tx, tx_id)?;
    let (bridge, reference, total) = crate::remesas::bridge_of(&tx, remittance_id)?;
    if amount.cents().abs() != total.cents() {
        return Err(AppError::Other(format!(
            "el movimiento es de {} y la remesa {reference} suma {total}",
            Money::from_cents(amount.cents().abs())
        )));
    }
    let bridge = AccountCode::new(&bridge).map_err(|e| AppError::Other(e.to_string()))?;
    let entry = JournalEntry {
        date,
        concept: format!("Remesa {reference} · {desc}"),
        origin: Origin::Banco(tx_id),
        lines: [
            EntryLine::signed(bank, amount, true),
            EntryLine::signed(bridge, amount, false),
        ]
        .into_iter()
        .flatten()
        .collect(),
    };
    let id = finish(&tx, tx_id, &entry, "cuenta", None, now)?;
    tx.execute(
        "UPDATE remittance SET bank_tx_id = ?1 WHERE id = ?2",
        params![tx_id, remittance_id],
    )?;
    tx.commit()?;
    Ok(id)
}

/// Asientos de devoluciones de remesa sin movimiento del banco que mueven la subcuenta del
/// movimiento `tx_id` por su importe, del más cercano en fecha al más lejano.
fn return_suggestions(
    conn: &Connection,
    tx_id: i64,
    amount: i64,
    date: Date,
) -> Result<Vec<Suggestion>> {
    let mut stmt = conn.prepare(
        "SELECT e.id, e.concept, e.date, COALESCE(r.reference, '') FROM journal_entry e
           JOIN journal_line l ON l.entry_id = e.id
           JOIN bank_tx t ON t.id = ?1
           JOIN bank_account b ON b.id = t.bank_account AND b.account = l.account
           LEFT JOIN remittance r ON r.id = e.origin_id
         WHERE e.origin_kind = 'DevolucionRemesa' AND l.debit - l.credit = ?2
           AND abs(julianday(e.date) - julianday(?3)) <= ?4
           AND NOT EXISTS (SELECT 1 FROM bank_tx o WHERE o.entry_id = e.id)
         ORDER BY abs(julianday(e.date) - julianday(?3)), e.id",
    )?;
    let rows = stmt.query_map(params![tx_id, amount, date.to_string(), RETURN_DAYS], |r| {
        Ok(Suggestion {
            kind: "devolucion".into(),
            id: r.get(0)?,
            seq: 0,
            installments: 0,
            number: r.get(3)?,
            party: r.get(1)?,
            date: r.get(2)?,
            due_date: r.get(2)?,
            amount: Money::from_cents(amount.abs()),
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Movimiento pendiente de la cuenta del banco con subcuenta `bank` por `amount` (con signo),
/// a [`RETURN_DAYS`] días o menos de `date`: el cargo de un recibo devuelto o el abono de una
/// transferencia rechazada de una remesa.
pub(crate) fn pending_return_movement(
    conn: &Connection,
    bank: &str,
    amount: Money,
    date: Date,
) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT t.id FROM bank_tx t JOIN bank_account b ON b.id = t.bank_account
                 WHERE b.account = ?1 AND b.currency = 'EUR' AND b.kind <> ?5
                   AND t.amount = ?2 AND t.matched_kind IS NULL AND t.entry_id IS NULL
                   AND abs(julianday(t.op_date) - julianday(?3)) <= ?4
                   AND NOT {BOOKED_ELSEWHERE}
                 ORDER BY abs(julianday(t.op_date) - julianday(?3)), t.id LIMIT 1"
            ),
            params![
                bank,
                amount.cents(),
                date.to_string(),
                RETURN_DAYS,
                INFORMATIVE
            ],
            |r| r.get(0),
        )
        .optional()?)
}

/// Concilia un movimiento con el asiento ya hecho de un recibo devuelto (o una transferencia
/// rechazada) de una remesa: no se contabiliza nada nuevo, el movimiento queda casado con ese
/// asiento. Sólo si el asiento mueve la cuenta del banco del movimiento por su importe.
pub fn reconcile_return(v: &Vault, tx_id: i64, entry_id: i64, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let (_, amount, _, bank) = tx_data(&tx, tx_id)?;
    let fits: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM journal_entry e JOIN journal_line l ON l.entry_id = e.id
           WHERE e.id = ?1 AND e.origin_kind = 'DevolucionRemesa' AND l.account = ?2
             AND l.debit - l.credit = ?3)
           AND NOT EXISTS(SELECT 1 FROM bank_tx WHERE entry_id = ?1)",
        params![entry_id, bank.as_str(), amount.cents()],
        |r| r.get(0),
    )?;
    if !fits {
        return Err(AppError::Other(
            "esa devolución no es de esta cuenta o de este importe, o ya tiene su movimiento"
                .into(),
        ));
    }
    tx.execute(
        "UPDATE bank_tx SET entry_id = ?1, matched_kind = 'cuenta' WHERE id = ?2",
        params![entry_id, tx_id],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "bank.reconcile",
        &tx_id.to_string(),
        &serde_json::json!({ "kind": "devolucion", "entry": entry_id }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Concilia un movimiento contra una cuenta (comisiones 626, nóminas 465, impuestos 4750…).
///
/// Si la cuenta es la subcuenta de otra cuenta propia del banco en la misma divisa (un traspaso
/// entre cuentas, o el cargo mensual de una tarjeta de crédito contra su 5201), el movimiento
/// de la otra cara (importe contrario, unos días antes o después) queda conciliado con el mismo
/// asiento; y si esa otra cara ya se contabilizó contra esta cuenta, este movimiento se casa
/// con su asiento en vez de contabilizar el traspaso dos veces.
///
/// Si la otra cuenta propia es de otra divisa (euros a una cuenta en dólares, p. ej.), el
/// traspaso va por los euros reales: los de la cara en euros. La otra cara (de signo contrario,
/// unos días antes o después, y cuyos euros se separan como mucho un 10 % de los de ésta, o la
/// única que hay si no se saben) queda conciliada con el mismo asiento y guarda esos euros como
/// su contravalor.
pub fn reconcile_account(
    v: &Vault,
    tx_id: i64,
    account: &str,
    concept: &str,
    now: OffsetDateTime,
) -> Result<i64> {
    let account = AccountCode::new(account).map_err(|e| AppError::Other(e.to_string()))?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let other = own_account(&tx, account.as_str(), tx_id)?;
    if other.is_none()
        && let Some(cross) = own_account_cross(&tx, account.as_str(), tx_id)?
    {
        let id = reconcile_cross_transfer(&tx, tx_id, &account, cross, concept, now)?;
        tx.commit()?;
        return Ok(id);
    }
    let (date, amount, desc, bank) = tx_data(&tx, tx_id)?;
    // El otro lado se busca por el importe en la divisa de las dos cuentas (en una cuenta en
    // divisa, `amount` es el contravalor).
    let in_currency = Money::from_cents(tx.query_row(
        "SELECT amount FROM bank_tx WHERE id = ?1",
        [tx_id],
        |r| r.get(0),
    )?);
    let desc_for_rule = desc.clone();
    let account_for_rule = account.as_str().to_owned();
    if let Some(other) = other
        && let Some((other_tx, entry_id)) =
            posted_counterpart(&tx, other, tx_id, in_currency, date)?
    {
        // En divisa, los euros de este lado son los que el asiento ya lleva a su subcuenta.
        if in_currency != amount {
            let net: i64 = tx.query_row(
                "SELECT COALESCE(SUM(debit - credit), 0) FROM journal_line
                 WHERE entry_id = ?1 AND account = ?2",
                params![entry_id, bank.as_str()],
                |r| r.get(0),
            )?;
            crate::divisa::store_transfer(&tx, tx_id, Money::from_cents(net))?;
        }
        tx.execute(
            "UPDATE bank_tx SET entry_id = ?1, matched_kind = 'cuenta' WHERE id = ?2",
            params![entry_id, tx_id],
        )?;
        audit::append(
            &tx,
            &now_str(now),
            Actor::User,
            "bank.reconcile",
            &tx_id.to_string(),
            &serde_json::json!({ "kind": "traspaso", "entry": entry_id, "tx": other_tx }),
        )?;
        crate::bank_rules::learn(&tx, &desc_for_rule, in_currency, &account_for_rule, now)?;
        tx.commit()?;
        return Ok(entry_id);
    }
    let entry = JournalEntry {
        date,
        concept: if concept.trim().is_empty() {
            desc
        } else {
            concept.trim().to_owned()
        },
        origin: Origin::Banco(tx_id),
        lines: [
            EntryLine::signed(bank, amount, true),
            EntryLine::signed(account, amount, false),
        ]
        .into_iter()
        .flatten()
        .collect(),
    };
    let id = finish(&tx, tx_id, &entry, "cuenta", None, now)?;
    if let Some(other) = other
        && let Some(other_tx) = pending_counterpart(&tx, other, in_currency, date)?
    {
        crate::divisa::store_transfer(&tx, other_tx, -amount)?;
        tx.execute(
            "UPDATE bank_tx SET entry_id = ?1, matched_kind = 'cuenta' WHERE id = ?2",
            params![id, other_tx],
        )?;
        audit::append(
            &tx,
            &now_str(now),
            Actor::User,
            "bank.reconcile",
            &other_tx.to_string(),
            &serde_json::json!({ "kind": "traspaso", "entry": id, "tx": tx_id }),
        )?;
    }
    // La próxima vez, los movimientos con este concepto propondrán esta cuenta.
    crate::bank_rules::learn(&tx, &desc_for_rule, in_currency, &account_for_rule, now)?;
    tx.commit()?;
    Ok(id)
}

/// Otra cuenta propia del banco, en la misma divisa que la del movimiento, cuya subcuenta es
/// `account` (no las tarjetas de débito, que no tienen).
fn own_account(conn: &Connection, account: &str, tx_id: i64) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT o.id FROM bank_account o, bank_tx t JOIN bank_account b ON b.id = t.bank_account
             WHERE t.id = ?2 AND o.account = ?1 AND o.id <> b.id AND o.currency = b.currency
               AND o.kind <> ?3
             ORDER BY o.id LIMIT 1",
            params![account, tx_id, INFORMATIVE],
            |r| r.get(0),
        )
        .optional()?)
}

/// Otra cuenta propia del banco, en otra divisa que la del movimiento, cuya subcuenta es
/// `account`.
fn own_account_cross(conn: &Connection, account: &str, tx_id: i64) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT o.id FROM bank_account o, bank_tx t JOIN bank_account b ON b.id = t.bank_account
             WHERE t.id = ?2 AND o.account = ?1 AND o.id <> b.id AND o.currency <> b.currency
               AND o.kind <> ?3
             ORDER BY o.id LIMIT 1",
            params![account, tx_id, INFORMATIVE],
            |r| r.get(0),
        )
        .optional()?)
}

/// Traspaso entre cuentas propias de distinta divisa (ver [`reconcile_account`]), dentro de la
/// transacción `tx`.
fn reconcile_cross_transfer(
    tx: &Connection,
    tx_id: i64,
    account: &AccountCode,
    other: i64,
    concept: &str,
    now: OffsetDateTime,
) -> Result<i64> {
    let (date, amount, desc, bank, currency) = tx_data_raw(tx, tx_id)?;
    let eur_side = currency == "EUR";
    // La otra cara ya contabilizada contra esta subcuenta: este movimiento se casa con su
    // asiento, por los euros que ese asiento lleva a esta subcuenta.
    let posted: Option<(i64, i64, i64)> = tx
        .query_row(
            "SELECT t.id, t.entry_id, (SELECT COALESCE(SUM(l.debit - l.credit), 0)
                                        FROM journal_line l
                                        WHERE l.entry_id = t.entry_id AND l.account = b.account)
             FROM bank_tx t, bank_tx me JOIN bank_account b ON b.id = me.bank_account
             WHERE me.id = ?2 AND t.bank_account = ?1 AND t.matched_kind = 'cuenta'
               AND t.entry_id IS NOT NULL AND (t.amount < 0) <> (me.amount < 0)
               AND abs(julianday(t.op_date) - julianday(?3)) <= ?4
               AND EXISTS (SELECT 1 FROM journal_line l
                           WHERE l.entry_id = t.entry_id AND l.account = b.account)
               AND NOT EXISTS (SELECT 1 FROM bank_tx o
                               WHERE o.entry_id = t.entry_id AND o.bank_account = me.bank_account)
             ORDER BY abs(julianday(t.op_date) - julianday(?3)), t.id LIMIT 1",
            params![other, tx_id, date.to_string(), TRANSFER_DAYS],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    if let Some((other_tx, entry_id, net)) = posted {
        let net = Money::from_cents(net);
        if (!eur_side || net == amount)
            && !net.is_zero()
            && net.is_negative() == amount.is_negative()
        {
            crate::divisa::store_transfer(tx, tx_id, net)?;
            tx.execute(
                "UPDATE bank_tx SET entry_id = ?1, matched_kind = 'cuenta' WHERE id = ?2",
                params![entry_id, tx_id],
            )?;
            audit::append(
                tx,
                &now_str(now),
                Actor::User,
                "bank.reconcile",
                &tx_id.to_string(),
                &serde_json::json!({ "kind": "traspaso", "entry": entry_id, "tx": other_tx }),
            )?;
            return Ok(entry_id);
        }
    }
    // Los euros de este lado (los suyos, si es la cuenta en euros; si no, su contravalor).
    let mine = if eur_side {
        Some(amount)
    } else {
        crate::divisa::contravalor(tx, tx_id)?.map(|c| c.amount_eur)
    };
    // La otra cara sin conciliar: de signo contrario y a pocos días.
    let mut st = tx.prepare(&format!(
        "SELECT t.id, t.amount, b.currency FROM bank_tx t JOIN bank_account b ON b.id = t.bank_account
         WHERE t.bank_account = ?1 AND (t.amount < 0) <> (?2 < 0) AND t.amount <> 0
           AND t.matched_kind IS NULL AND t.entry_id IS NULL
           AND abs(julianday(t.op_date) - julianday(?3)) <= ?4 AND NOT {BOOKED_ELSEWHERE}
         ORDER BY abs(julianday(t.op_date) - julianday(?3)), t.id"
    ))?;
    let candidates: Vec<(i64, i64, String)> = st
        .query_map(
            params![other, amount.cents(), date.to_string(), TRANSFER_DAYS],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?
        .collect::<std::result::Result<_, _>>()?;
    let single = candidates.len() == 1;
    let mut chosen: Option<(i64, Money, bool)> = None;
    for (id, a, cur) in candidates {
        let theirs = if cur == "EUR" {
            Some(-Money::from_cents(a))
        } else {
            crate::divisa::contravalor(tx, id)?.map(|c| -c.amount_eur)
        };
        let fits = match (mine, theirs) {
            (Some(m), Some(t)) => crate::cartera::within_exchange_tolerance(m.abs(), t.abs()),
            _ => single,
        };
        if fits {
            chosen = Some((id, theirs.unwrap_or(Money::ZERO), cur == "EUR"));
            break;
        }
    }
    // Los euros reales: los de la cara en euros; entre dos divisas, el contravalor de ésta.
    let eur = match (&chosen, mine) {
        (Some((_, theirs, true)), _) if !eur_side => *theirs,
        (_, Some(m)) => m,
        (Some((_, theirs, _)), None) if !theirs.is_zero() => *theirs,
        _ => crate::divisa::fix(tx, tx_id)?,
    };
    if !eur_side {
        crate::divisa::store_transfer(tx, tx_id, eur)?;
    }
    let entry = JournalEntry {
        date,
        concept: if concept.trim().is_empty() {
            desc.clone()
        } else {
            concept.trim().to_owned()
        },
        origin: Origin::Banco(tx_id),
        lines: [
            EntryLine::signed(bank, eur, true),
            EntryLine::signed(account.clone(), eur, false),
        ]
        .into_iter()
        .flatten()
        .collect(),
    };
    let id = finish(tx, tx_id, &entry, "cuenta", None, now)?;
    if let Some((other_tx, _, other_eur)) = chosen {
        if !other_eur {
            crate::divisa::store_transfer(tx, other_tx, -eur)?;
        }
        tx.execute(
            "UPDATE bank_tx SET entry_id = ?1, matched_kind = 'cuenta' WHERE id = ?2",
            params![id, other_tx],
        )?;
        audit::append(
            tx,
            &now_str(now),
            Actor::User,
            "bank.reconcile",
            &other_tx.to_string(),
            &serde_json::json!({ "kind": "traspaso", "entry": id, "tx": tx_id }),
        )?;
    }
    crate::bank_rules::learn(tx, &desc, amount, account.as_str(), now)?;
    Ok(id)
}

/// La otra cara de un traspaso, ya contabilizada contra la subcuenta de este movimiento (y sin
/// otro movimiento de esta cuenta casado con su asiento): (movimiento, asiento).
fn posted_counterpart(
    conn: &Connection,
    other: i64,
    tx_id: i64,
    amount: Money,
    date: Date,
) -> Result<Option<(i64, i64)>> {
    Ok(conn
        .query_row(
            "SELECT t.id, t.entry_id FROM bank_tx t, bank_tx me
               JOIN bank_account b ON b.id = me.bank_account
             WHERE me.id = ?2 AND t.bank_account = ?1 AND t.amount = ?3
               AND t.matched_kind = 'cuenta' AND t.entry_id IS NOT NULL
               AND abs(julianday(t.op_date) - julianday(?4)) <= ?5
               AND EXISTS (SELECT 1 FROM journal_line l
                           WHERE l.entry_id = t.entry_id AND l.account = b.account)
               AND NOT EXISTS (SELECT 1 FROM bank_tx o
                               WHERE o.entry_id = t.entry_id AND o.bank_account = me.bank_account)
             ORDER BY abs(julianday(t.op_date) - julianday(?4)), t.id LIMIT 1",
            params![
                other,
                tx_id,
                -amount.cents(),
                date.to_string(),
                TRANSFER_DAYS
            ],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// La otra cara de un traspaso, aún sin conciliar.
fn pending_counterpart(
    conn: &Connection,
    other: i64,
    amount: Money,
    date: Date,
) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT t.id FROM bank_tx t
                 WHERE t.bank_account = ?1 AND t.amount = ?2 AND t.matched_kind IS NULL
                   AND t.entry_id IS NULL AND abs(julianday(t.op_date) - julianday(?3)) <= ?4
                   AND NOT {BOOKED_ELSEWHERE}
                 ORDER BY abs(julianday(t.op_date) - julianday(?3)), t.id LIMIT 1"
            ),
            params![other, -amount.cents(), date.to_string(), TRANSFER_DAYS],
            |r| r.get(0),
        )
        .optional()?)
}
