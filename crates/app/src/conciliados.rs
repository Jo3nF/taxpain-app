//! Movimientos del banco ya conciliados y cómo deshacer una conciliación equivocada
//! (CONTABILIDAD-03).
//!
//! Deshacer es lo simétrico de [`crate::bank::reconcile_invoice`],
//! [`crate::bank::reconcile_remittance`] y [`crate::bank::reconcile_account`] (también cuando
//! la llama [`crate::tax_payments::pay_from_bank`]): el movimiento vuelve a pendiente, se borra
//! el asiento que creó la conciliación y lo que colgaba de ella (el pago del modelo, la marca
//! de la remesa «apuntada por el banco», la otra cara de un traspaso) y la factura o el
//! vencimiento vuelven a pendientes en la cartera. Los errores de los libros se corrigen así
//! mientras el periodo esté abierto (Código de Comercio, arts. 25 y 29); si no, con un asiento
//! en el periodo actual.

use serde::Serialize;
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};

use crate::bank::BankTx;
use crate::error::{AppError, Result};

fn now_str(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

fn parse_date(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]"))
        .map_err(|e| AppError::Other(e.to_string()))
}

/// Movimiento conciliado, con su contrapartida en llano, para el listado de conciliados.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Reconciled {
    pub tx: BankTx,
    /// Subcuenta de la cuenta del banco del movimiento (5720001…).
    pub account: String,
    /// Asiento de la conciliación.
    pub entry_id: Option<i64>,
    /// `emitida`, `recibida`, `remesa`, `devolucion`, `impuesto`, `traspaso` o `cuenta`.
    pub kind: String,
    /// «Factura A2026-0009 (plazo 2)», «Remesa REM-2026-0001», «Modelo 303 3T 2026», «626».
    pub counterpart: String,
    /// Cliente, proveedor o concepto del asiento.
    pub party: String,
    /// Cuándo se concilió (alta del asiento).
    pub reconciled_at: Option<String>,
    /// Texto de la regla del banco que llevó el movimiento a esa cuenta, si la hay.
    pub rule: Option<String>,
    /// Por qué no se puede deshacer (`None` si se puede).
    pub blocked: Option<String>,
}

/// Cuenta del banco con movimientos conciliados (para el filtro del listado).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReconciledAccount {
    pub account: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReconciledList {
    pub accounts: Vec<ReconciledAccount>,
    pub items: Vec<Reconciled>,
}

/// Movimientos conciliados, del más nuevo al más antiguo, con lo que se casó cada uno. Se
/// filtran por la subcuenta de la cuenta del banco y por fecha de operación (ambas incluidas).
pub fn reconciled(
    v: &Vault,
    account: Option<&str>,
    from: Option<Date>,
    to: Option<Date>,
) -> Result<ReconciledList> {
    let conn = v.connection();
    let mut stmt = conn.prepare(
        "SELECT DISTINCT b.account, b.name FROM bank_tx t
         JOIN bank_account b ON b.id = t.bank_account
         WHERE t.matched_kind IS NOT NULL ORDER BY b.account",
    )?;
    let accounts = stmt
        .query_map([], |r| {
            Ok(ReconciledAccount {
                account: r.get(0)?,
                name: r.get(1)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    let rules = crate::bank_rules::list_conn(conn)?;
    let (from, to) = (from.map(|d| d.to_string()), to.map(|d| d.to_string()));
    let mut items = Vec::new();
    for t in crate::bank::movements(v, false)? {
        let Some(kind) = t.matched.clone() else {
            continue;
        };
        if from.as_ref().is_some_and(|f| t.op_date < *f)
            || to.as_ref().is_some_and(|d| t.op_date > *d)
        {
            continue;
        }
        type Row = (Option<i64>, Option<i64>, Option<i64>, String);
        let (entry_id, matched_id, matched_seq, bank): Row = conn.query_row(
            "SELECT t.entry_id, t.matched_id, t.matched_seq, b.account FROM bank_tx t
             JOIN bank_account b ON b.id = t.bank_account WHERE t.id = ?1",
            [t.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        if account.is_some_and(|a| a != bank) {
            continue;
        }
        let (concept, reconciled_at) = match entry_id {
            Some(e) => conn
                .query_row(
                    "SELECT concept, created_at FROM journal_entry WHERE id = ?1",
                    [e],
                    |r| Ok((r.get::<_, String>(0)?, Some(r.get::<_, String>(1)?))),
                )
                .optional()?
                .unwrap_or_default(),
            None => (String::new(), None),
        };
        let mut rule = None;
        let (kind, counterpart, party) = match kind.as_str() {
            "emitida" | "recibida" => {
                let id = matched_id.unwrap_or_default();
                // Un id negativo es un pendiente de antes de TaxPain (saldos iniciales).
                let (number, name): (String, String) = conn
                    .query_row(
                        if id < 0 {
                            "SELECT number, name FROM opening_item WHERE id = -?1"
                        } else if kind == "emitida" {
                            "SELECT number, COALESCE(recipient_name, 'Clientes varios')
                             FROM invoice_issued WHERE id = ?1"
                        } else {
                            "SELECT COALESCE(number, ''), COALESCE(supplier_name, '') FROM invoice_received
                             WHERE id = ?1"
                        },
                        [id],
                        |r| Ok((r.get(0)?, r.get(1)?)),
                    )
                    .optional()?
                    .unwrap_or_default();
                let plazo = matched_seq.map_or(String::new(), |s| format!(" (plazo {s})"));
                (kind, format!("Factura {number}{plazo}"), name)
            }
            _ => account_counterpart(conn, &t, &bank, entry_id, &concept, &rules, &mut rule)?,
        };
        let blocked = blocker(v, conn, t.id)?;
        items.push(Reconciled {
            tx: t,
            account: bank,
            entry_id,
            kind,
            counterpart,
            party,
            reconciled_at,
            rule,
            blocked,
        });
    }
    Ok(ReconciledList { accounts, items })
}

/// Contrapartida de un movimiento conciliado «contra cuenta»: remesa, pago de un modelo,
/// traspaso entre cuentas propias o la cuenta del asiento (con su regla, si la tiene).
fn account_counterpart(
    conn: &Connection,
    t: &BankTx,
    bank: &str,
    entry_id: Option<i64>,
    concept: &str,
    rules: &[crate::bank_rules::Rule],
    rule: &mut Option<String>,
) -> Result<(String, String, String)> {
    if let Some(reference) = conn
        .query_row(
            "SELECT reference FROM remittance WHERE bank_tx_id = ?1",
            [t.id],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        // El concepto del asiento ya empieza por la remesa: sólo lo que dice el banco.
        let prefix = format!("Remesa {reference} · ");
        return Ok((
            "remesa".into(),
            format!("Remesa {reference}"),
            concept.strip_prefix(&prefix).unwrap_or(concept).to_owned(),
        ));
    }
    if let Some((model, year, period, account)) = conn
        .query_row(
            "SELECT model, year, period, account FROM tax_payment WHERE bank_tx = ?1",
            [t.id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?
    {
        return Ok((
            "impuesto".into(),
            format!("Modelo {model} {period} {year}"),
            format!("contra la cuenta {account}"),
        ));
    }
    let Some(e) = entry_id else {
        return Ok(("cuenta".into(), String::new(), concept.to_owned()));
    };
    // El cargo de un recibo devuelto, casado con el asiento de la devolución de la remesa: su
    // concepto ya lo dice todo («Devolución recibo A2026-0002 · remesa REM-2026-0001»).
    let returned: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM journal_entry WHERE id = ?1
           AND origin_kind = 'DevolucionRemesa')",
        [e],
        |r| r.get(0),
    )?;
    if returned {
        return Ok(("devolucion".into(), concept.to_owned(), String::new()));
    }
    let account: String = conn
        .query_row(
            "SELECT account FROM journal_line WHERE entry_id = ?1 AND account <> ?2
             ORDER BY id LIMIT 1",
            params![e, bank],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or_default();
    let shared: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM bank_tx o WHERE o.entry_id = ?1 AND o.id <> ?2)",
        params![e, t.id],
        |r| r.get(0),
    )?;
    if shared {
        return Ok((
            "traspaso".into(),
            format!("Traspaso con {account}"),
            concept.to_owned(),
        ));
    }
    *rule = crate::bank_rules::matching(rules, &t.description, t.amount)
        .filter(|r| r.account == account)
        .map(|r| r.pattern.clone());
    Ok(("cuenta".into(), account, concept.to_owned()))
}

/// Primer modelo de `models` presentado (en producción desde TaxPain o importado de la
/// gestoría) del trimestre o mes de `date`: (modelo, «3T»).
fn filed(conn: &Connection, models: &[&str], date: Date) -> Result<Option<(String, String)>> {
    let month = u8::from(date.month());
    let quarter = (month - 1) / 3 + 1;
    let (quarterly, monthly) = (format!("{quarter}T"), format!("{month:02}"));
    for model in models {
        let found: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM tax_return WHERE model = ?1 AND year = ?2
                             AND period IN (?3, ?4) AND environment = 'produccion')
                 OR EXISTS(SELECT 1 FROM filed_model WHERE model = ?1 AND year = ?2
                             AND period IN (?3, ?4))",
            params![model, date.year(), quarterly, monthly],
            |r| r.get(0),
        )?;
        if found {
            return Ok(Some(((*model).to_owned(), quarterly)));
        }
    }
    Ok(None)
}

/// Por qué no se puede deshacer la conciliación del movimiento `tx_id` (`None` si se puede):
/// - el ejercicio de su asiento está cerrado;
/// - el asiento entra en un 303 o 130 ya presentados (toca los grupos 6 o 7, la 472 o la 477:
///   diferencias de cambio, gastos o ingresos contabilizados directamente contra el banco);
/// - el cobro o pago es de una factura con criterio de caja y su 303 ya está presentado (el IVA
///   se declara al cobrar o pagar, arts. 163 decies y terdecies LIVA), o de una factura con
///   retención cuyo 111 o 115 se presentó con la fecha de ese pago (art. 78 RIRPF);
/// - de la remesa ya se apuntaron recibos devueltos después del abono del banco;
/// - otra cosa apunta al asiento (una nómina, un inmovilizado…).
pub(crate) fn blocker(v: &Vault, conn: &Connection, tx_id: i64) -> Result<Option<String>> {
    type Row = (
        Option<String>,
        Option<i64>,
        Option<i64>,
        String,
        Option<(i32, String, bool, String)>,
    );
    let (kind, entry, matched_id, op_date, info): Row = conn
        .query_row(
            "SELECT t.matched_kind, t.entry_id, t.matched_id, t.op_date, e.fiscal_year, e.date,
                    COALESCE((SELECT f.closed FROM fiscal_year f WHERE f.year = e.fiscal_year), 0),
                    e.origin_kind
             FROM bank_tx t LEFT JOIN journal_entry e ON e.id = t.entry_id WHERE t.id = ?1",
            [tx_id],
            |r| {
                let year: Option<i32> = r.get(4)?;
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    match year {
                        Some(y) => Some((y, r.get(5)?, r.get(6)?, r.get(7)?)),
                        None => None,
                    },
                ))
            },
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let Some(kind) = kind else {
        return Ok(Some("el movimiento no está conciliado".into()));
    };
    let op_date = parse_date(&op_date)?;
    // Movimiento de un ejercicio cerrado (aunque no tenga asiento propio).
    let closed_year: Option<i32> = conn
        .query_row(
            "SELECT year FROM fiscal_year WHERE closed = 1 AND ?1 BETWEEN start_date AND end_date",
            [op_date.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    let (entry_year, entry_date, entry_closed, origin) = match info {
        Some((y, d, c, o)) => (Some(y), Some(parse_date(&d)?), c, o),
        None => (None, None, false, String::new()),
    };
    if entry_closed || closed_year.is_some() {
        let year = entry_year.filter(|_| entry_closed).or(closed_year);
        return Ok(Some(format!(
            "es del ejercicio {}, que está cerrado: para deshacer la conciliación hay que \
             reabrirlo desde Cierre",
            year.unwrap_or_else(|| op_date.year())
        )));
    }
    let presented = |model: &str, q: &str, y: i32| {
        format!(
            "el modelo {model} del {q} de {y} ya está presentado y cambiaría: lo que haya que \
             corregir se corrige con un asiento en el periodo actual"
        )
    };
    // El asiento sólo se borra si lo creó el banco; si no, sólo se desenlaza.
    if let (Some(e), Some(date), "Banco") = (entry, entry_date, origin.as_str()) {
        let declared: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM journal_line WHERE entry_id = ?1
               AND (account LIKE '6%' OR account LIKE '7%' OR account LIKE '472%'
                    OR account LIKE '477%'))",
            [e],
            |r| r.get(0),
        )?;
        if declared && let Some((model, q)) = filed(conn, &["303", "130"], date)? {
            return Ok(Some(presented(&model, &q, date.year())));
        }
        for (table, column, why) in crate::import::ENTRY_REFERENCES {
            if table == "bank_tx" || table == "tax_payment" {
                continue;
            }
            let used: bool = conn.query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {column} = ?1)"),
                [e],
                |r| r.get(0),
            )?;
            if used {
                return Ok(Some(format!("su asiento {why}: deshaz eso primero")));
            }
        }
    }
    match kind.as_str() {
        "emitida" | "recibida" => {
            let id = matched_id.unwrap_or_default();
            // Un pendiente de antes de TaxPain (id negativo) no tiene IVA de caja ni retención
            // que mirar: su IVA y su retención ya se declararon con la gestoría.
            if id < 0 {
                return Ok(None);
            }
            let company_cash = crate::models::settings(v)?.cash_basis;
            let fila: Option<(bool, bool, String)> = if kind == "emitida" {
                conn.query_row(
                    "SELECT 0, 0, issue_date FROM invoice_issued WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?
            } else {
                conn.query_row(
                    "SELECT cash_basis AND vat_regime = 'interior' AND COALESCE(vat, 0) <> 0,
                            COALESCE(withholding, 0) <> 0,
                            COALESCE(issue_date, substr(reviewed_at, 1, 10), created_at)
                     FROM invoice_received WHERE id = ?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?
            };
            let Some((cash, withholding, issue)) = fila else {
                return Ok(None);
            };
            if (cash || company_cash)
                && let Some((model, q)) = filed(conn, &["303"], op_date)?
            {
                return Ok(Some(presented(&model, &q, op_date.year())));
            }
            // La retención se declara en el trimestre del pago; si se deshace, en el de la
            // factura: sólo importa si son distintos.
            let issue = parse_date(issue.get(..10).unwrap_or_default())?;
            let quarter = |d: Date| (d.year(), (u8::from(d.month()) - 1) / 3);
            if withholding && quarter(issue) != quarter(op_date) {
                for d in [op_date, issue] {
                    if let Some((model, q)) = filed(conn, &["111", "115"], d)? {
                        return Ok(Some(presented(&model, &q, d.year())));
                    }
                }
            }
        }
        _ => {
            let returned: Option<String> = conn
                .query_row(
                    "SELECT r.reference FROM remittance r WHERE r.bank_tx_id = ?1
                       AND EXISTS(SELECT 1 FROM journal_entry e
                                  WHERE e.origin_kind = 'DevolucionRemesa' AND e.origin_id = r.id)",
                    [tx_id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(reference) = returned {
                return Ok(Some(format!(
                    "de la remesa {reference} ya se apuntaron recibos devueltos después de que \
                     el banco la abonara: deshaz antes esas devoluciones (Contabilidad → Cobros \
                     y pagos → Remesas, «Deshacer devolución» en cada recibo)"
                )));
            }
        }
    }
    Ok(None)
}

/// Deshace una conciliación: el movimiento vuelve a pendiente y se borra el asiento que creó
/// (si era el pago de un modelo, también el pago; si era una remesa, el banco deja de tenerla
/// apuntada y sus recibos siguen cobrados por la remesa; si era un traspaso entre cuentas
/// propias, la otra cara vuelve también a pendiente). La factura o el vencimiento vuelven a
/// pendientes en la cartera; los demás plazos de la factura no se tocan. La regla del banco
/// aprendida con esa conciliación pierde un uso (y desaparece si era el único).
///
/// Si el movimiento se casó con un asiento que no creó el banco, sólo se desenlaza. No se
/// deshace en los casos de [`blocker`]. Queda en la auditoría con el asiento borrado.
pub fn unreconcile(v: &Vault, tx_id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    if let Some(why) = blocker(v, &tx, tx_id)? {
        return Err(AppError::Other(format!("no se puede deshacer: {why}")));
    }
    type Row = (
        String,
        Option<i64>,
        Option<i64>,
        Option<i64>,
        String,
        i64,
        String,
    );
    let (kind, entry, matched_id, matched_seq, desc, amount, bank): Row = tx.query_row(
        "SELECT t.matched_kind, t.entry_id, t.matched_id, t.matched_seq, t.description,
                t.amount, b.account
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
            ))
        },
    )?;
    let own_entry = match entry {
        Some(e) => tx
            .query_row(
                "SELECT origin_kind = 'Banco' FROM journal_entry WHERE id = ?1",
                [e],
                |r| r.get::<_, bool>(0),
            )
            .optional()?
            .unwrap_or(false)
            .then_some(e),
        None => None,
    };
    // Remesa: el banco deja de tenerla apuntada (sus recibos siguen en la remesa).
    let remittance: Option<String> = tx
        .query_row(
            "SELECT reference FROM remittance WHERE bank_tx_id = ?1",
            [tx_id],
            |r| r.get(0),
        )
        .optional()?;
    tx.execute(
        "UPDATE remittance SET bank_tx_id = NULL WHERE bank_tx_id = ?1",
        [tx_id],
    )?;
    // Pago de un modelo: el modelo vuelve a estar pendiente de pago.
    let tax: Option<String> = tx
        .query_row(
            "SELECT model || ' ' || period || ' ' || year FROM tax_payment WHERE bank_tx = ?1",
            [tx_id],
            |r| r.get(0),
        )
        .optional()?;
    tx.execute("DELETE FROM tax_payment WHERE bank_tx = ?1", [tx_id])?;
    // Las caras de un traspaso comparten asiento: vuelven todas a pendientes.
    let mut txs = vec![tx_id];
    if let Some(e) = own_entry {
        let mut stmt =
            tx.prepare("SELECT id FROM bank_tx WHERE entry_id = ?1 AND id <> ?2 ORDER BY id")?;
        let others: Vec<i64> = stmt
            .query_map(params![e, tx_id], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        txs.extend(others);
    }
    for id in &txs {
        // El contravalor que se guardó al conciliar (del BCE o de la otra cara de un traspaso)
        // se recalcula al volver a conciliar; el tecleado se queda.
        tx.execute(
            "UPDATE bank_tx SET entry_id = NULL, matched_kind = NULL, matched_id = NULL,
                    matched_seq = NULL,
                    amount_eur = CASE WHEN fx_source = 'manual' THEN amount_eur END,
                    fx_rate = CASE WHEN fx_source = 'manual' THEN fx_rate END,
                    fx_date = CASE WHEN fx_source = 'manual' THEN fx_date END,
                    fx_source = CASE WHEN fx_source = 'manual' THEN fx_source END
             WHERE id = ?1",
            [id],
        )?;
    }
    let mut deleted = None;
    if let Some(e) = own_entry {
        let before = crate::accounting::load_entry(&tx, e)?;
        if kind == "cuenta" && remittance.is_none() {
            // La regla aprendida al contabilizar contra esa cuenta pierde este uso.
            if let Some(line) = before.lines.iter().find(|l| l.account.as_str() != bank) {
                crate::bank_rules::forget(
                    &tx,
                    &desc,
                    Money::from_cents(amount),
                    line.account.as_str(),
                )?;
            }
        }
        tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [e])?;
        tx.execute("DELETE FROM journal_entry WHERE id = ?1", [e])?;
        deleted = Some(before);
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "bank.unreconcile",
        &tx_id.to_string(),
        &serde_json::json!({
            "kind": kind,
            "id": matched_id,
            "seq": matched_seq,
            "entry": entry,
            "deleted": deleted,
            "remittance": remittance,
            "tax_payment": tax,
            "txs": txs,
        }),
    )?;
    tx.commit()?;
    Ok(())
}
