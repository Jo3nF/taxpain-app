//! Venta de un elemento del inmovilizado con su factura emitida (CONTABILIDAD-08).
//!
//! La baja ([`crate::accounting::dispose_asset_with_invoice`]) deja el precio en la 543 (créditos
//! a corto plazo por enajenación de inmovilizado) y el resultado en 77x/67x. La factura de venta
//! se contabiliza como todas (430 / 70x / 477), así que, sin más, el ingreso contaría dos veces
//! (en la 705 y en la 771) y quedarían abiertas la 543 y la 430. Al vincular la factura con la
//! venta (al dar de baja o después), un asiento con la fecha de la factura y origen «Baja de
//! inmovilizado» pasa su base de la cuenta de ventas a la 543 (Debe 70x, Haber 543) y, si no
//! coincide con el precio de la baja, la diferencia al resultado. El asiento de la factura no se
//! toca: su IVA sigue en la 477 y el importe en la 430, que se cobra y concilia como siempre.
//! Vale con los dos órdenes (factura antes o después de la baja) y con facturas ya emitidas,
//! también de otro ejercicio: el asiento va siempre en el de la factura. Con una de un ejercicio
//! anterior (facturada antes de la entrega), las ventas de ese ejercicio quedan sin ella y la 543
//! queda allí con saldo acreedor hasta la baja, como lo facturado por adelantado.
//!
//! Las rectificativas de la factura pasan su diferencia al resultado de la venta; si la factura
//! vende sólo ese elemento y entero, se vinculan solas al emitirlas. Una que anula la venta no se
//! pasa sola: TaxPain no deshace la baja. Mientras está vinculada, una factura no se puede anular
//! (ver `invoicing::cancel`).
//!
//! Cada cambio del precio va primero contra el resultado que la venta ya tiene en ese ejercicio
//! (una rebaja quita beneficio antes de dar pérdida, y una subida quita pérdida antes de dar
//! beneficio): una venta acaba con beneficio (77x) o con pérdida (67x), nunca con los dos. Por
//! eso las facturas se desvinculan en orden inverso al que se vincularon.
//!
//! La 543 es una para todas las ventas: lo que una venta sin factura vinculada tiene aún en ella
//! sale de su saldo (ver [`unbilled_in_543`]). Si un cobro conciliado contra la 543 o un asiento
//! manual ya lo ha bajado, la factura de venta no se vincula (ni al dar de baja ni después): la
//! 543 quedaría en negativo y el cobro contaría también en la 430 de la factura.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use serde::Serialize;
use taxpain_core::accounting::{AccountCode, EntryLine, JournalEntry, Origin};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};

use crate::accounting::{self, record_entry};
use crate::error::{AppError, Result};
use crate::extractos::numeros::sin_tildes;

/// Facturas que se proponen como mucho (las más cercanas a la baja; sin baja, las más recientes).
const MAX_CANDIDATES: usize = 200;

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

fn now_str(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

fn parse_date(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]"))
        .map_err(|e| other(e.to_string()))
}

fn dmy(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

fn code(s: &str) -> Result<AccountCode> {
    AccountCode::new(s).map_err(|e| other(e.to_string()))
}

/// Factura vinculada a una venta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SaleLink {
    /// Asiento que pasa la factura de ventas a la venta.
    pub entry_id: i64,
    /// Importe pasado (negativo si la rectificativa rebaja el precio).
    pub amount: Money,
    /// Cambio del resultado de la venta: positivo, más beneficio (o menos pérdida).
    pub result_change: Money,
    pub warnings: Vec<String>,
}

/// Factura emitida que puede ser la de la venta de un elemento (o una rectificativa de ésta).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SaleCandidate {
    pub invoice_id: i64,
    pub number: String,
    pub issue_date: Date,
    pub operation_date: Option<Date>,
    pub recipient_name: Option<String>,
    /// Lo que la factura lleva a ventas en su asiento (la base sin anticipos).
    pub base: Money,
    /// Lo que aún no está vinculado a ninguna venta.
    pub open: Money,
    /// Importe que se propone para este elemento; `None` si hay que decirlo (la factura de venta
    /// vende más cosas).
    pub suggested: Option<Money>,
    /// Factura de venta que rectifica, si es una rectificativa.
    pub rectifies: Option<String>,
    /// La rectificativa deja la venta a cero o menos: la anula (no se pasa sola). Sólo se sabe si
    /// la factura vende este elemento y entero; si vende más, la parte de cada uno la dice el
    /// usuario.
    pub cancels_sale: bool,
}

/// Factura vinculada a la venta de un elemento.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SaleInvoice {
    pub invoice_id: i64,
    pub number: String,
    pub issue_date: Date,
    pub amount: Money,
    /// Factura de venta que rectifica, si es una rectificativa.
    pub rectifies: Option<String>,
    pub entry_id: i64,
}

/// Baja de un elemento con su venta: precio, facturas y lo que queda en la 543.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AssetSale {
    pub asset_id: i64,
    pub date: Date,
    /// Precio de la baja (sin IVA; 0 si se tiró).
    pub price: Money,
    /// Lo facturado: la factura de venta y sus rectificativas vinculadas.
    pub invoiced: Money,
    /// Sin factura de venta vinculada, lo que la venta tiene aún en la 543 según su saldo (el
    /// precio, salvo lo que un cobro o un asiento ya haya bajado; ver [`unbilled_in_543`]). Con
    /// ella, 0.
    pub unbilled: Money,
    /// Beneficio (positivo) o pérdida (negativo) de la venta, con los ajustes de sus facturas.
    pub result: Money,
    pub invoices: Vec<SaleInvoice>,
    /// Rectificativas de la factura de venta que aún no se han pasado a la venta.
    pub pending: Vec<SaleCandidate>,
}

/// Lo que hace falta de una factura emitida para vincularla.
struct Issued {
    number: String,
    issue_date: Date,
    rectifies_id: Option<i64>,
    /// F1, F2, F3, R1…R5.
    kind: String,
    status: String,
    origin: String,
}

impl Issued {
    fn rectifying(&self) -> bool {
        self.kind.starts_with('R')
    }
}

fn issued(conn: &Connection, id: i64) -> Result<Issued> {
    let (number, date, rectifies_id, kind, status, origin): (
        String,
        String,
        Option<i64>,
        String,
        String,
        String,
    ) = conn
        .query_row(
            "SELECT number, issue_date, rectifies_id, kind, status, origin
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
        .optional()?
        .ok_or(AppError::NotFound)?;
    Ok(Issued {
        number,
        issue_date: parse_date(&date)?,
        rectifies_id,
        kind,
        status,
        origin,
    })
}

/// Lo que la factura `invoice_id` lleva a ventas en su asiento (cuentas del grupo 7, haber menos
/// debe), por cuenta y en el orden de sus líneas. Se lee del asiento, no de los ajustes de hoy (la
/// cuenta de ventas pudo cambiar después). Vacío si no tiene asiento (importada del libro de la
/// gestoría).
pub(crate) fn invoice_revenue(
    conn: &Connection,
    invoice_id: i64,
) -> Result<Vec<(AccountCode, Money)>> {
    let mut st = conn.prepare(
        "SELECT l.account, SUM(l.credit - l.debit) FROM journal_line l
           JOIN journal_entry e ON e.id = l.entry_id
          WHERE e.origin_kind = 'FacturaEmitida' AND e.origin_id = ?1
            AND substr(l.account, 1, 1) = '7'
          GROUP BY l.account ORDER BY MIN(l.id)",
    )?;
    let rows = st.query_map([invoice_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (account, cents) = row?;
        if cents != 0 {
            out.push((code(&account)?, Money::from_cents(cents)));
        }
    }
    Ok(out)
}

fn total(parts: &[(AccountCode, Money)]) -> Money {
    parts.iter().map(|(_, m)| *m).sum()
}

/// Lo ya vinculado de la factura a ventas del inmovilizado (de cualquier elemento).
fn linked(conn: &Connection, invoice_id: i64) -> Result<Money> {
    let cents: i64 = conn.query_row(
        "SELECT COALESCE(SUM(amount), 0) FROM asset_sale_invoice WHERE invoice_id = ?1",
        [invoice_id],
        |r| r.get(0),
    )?;
    Ok(Money::from_cents(cents))
}

/// Lo facturado de la venta de un elemento: su factura de venta y las rectificativas vinculadas.
fn invoiced(conn: &Connection, asset_id: i64) -> Result<Money> {
    let cents: i64 = conn.query_row(
        "SELECT COALESCE(SUM(amount), 0) FROM asset_sale_invoice WHERE asset_id = ?1",
        [asset_id],
        |r| r.get(0),
    )?;
    Ok(Money::from_cents(cents))
}

fn round_div(num: i128, den: i128) -> i64 {
    let q = num / den;
    let r = num % den;
    let q = if 2 * r.abs() >= den.abs() {
        q + num.signum() * den.signum()
    } else {
        q
    };
    i64::try_from(q).unwrap_or(i64::MAX)
}

/// Reparte `amount` entre las cuentas de ventas de la factura en proporción a lo que lleva a cada
/// una (hoy siempre una sola); la última se queda el redondeo.
fn split(amount: Money, parts: &[(AccountCode, Money)]) -> Vec<(AccountCode, Money)> {
    let whole = total(parts);
    let mut done = Money::ZERO;
    let mut out = Vec::new();
    for (i, (account, part)) in parts.iter().enumerate() {
        let share = if i + 1 == parts.len() || whole.is_zero() {
            amount - done
        } else {
            Money::from_cents(round_div(
                i128::from(amount.cents()) * i128::from(part.cents()),
                i128::from(whole.cents()),
            ))
        };
        done += share;
        out.push((account.clone(), share));
    }
    out
}

/// Elemento dado de baja en TaxPain.
struct Sold {
    name: String,
    account: AccountCode,
    acquired: Date,
    date: Date,
    price: Money,
}

fn sold(conn: &Connection, asset_id: i64) -> Result<Sold> {
    let (name, account, acquired, disposed, price, entry): (
        String,
        String,
        String,
        Option<String>,
        Option<i64>,
        Option<i64>,
    ) = conn
        .query_row(
            "SELECT name, account, acquired_on, disposed_on, sale_price, disposal_entry
               FROM fixed_asset WHERE id = ?1",
            [asset_id],
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
        .optional()?
        .ok_or(AppError::NotFound)?;
    let (Some(date), Some(_)) = (disposed, entry) else {
        return Err(other(format!(
            "«{name}» no está dado de baja en TaxPain: véndelo (Dar de baja o vender) y elige ahí \
             su factura"
        )));
    };
    Ok(Sold {
        name,
        account: code(&account)?,
        acquired: parse_date(&acquired)?,
        date: parse_date(&date)?,
        price: Money::from_cents(price.unwrap_or(0)),
    })
}

/// Facturas emitidas (ids) que rectifica la rectificativa `invoice_id`, en el orden de su lista:
/// una rectificativa puede serlo de varias facturas (art. 15.2 RD 1619/2012).
fn rectified_ids(conn: &Connection, invoice_id: i64) -> Result<Vec<i64>> {
    let mut st = conn.prepare(
        "SELECT o.id FROM invoice_issued i
           JOIN json_each(i.data, '$.invoice.rectified') r
           JOIN invoice_issued o ON o.number = json_extract(r.value, '$[0]') AND o.id <> i.id
          WHERE i.id = ?1
          ORDER BY CAST(r.key AS INTEGER), o.id",
    )?;
    let mut out: Vec<i64> = Vec::new();
    for id in st.query_map([invoice_id], |r| r.get::<_, i64>(0))? {
        let id = id?;
        if !out.contains(&id) {
            out.push(id);
        }
    }
    Ok(out)
}

/// Condición SQL: la emitida `i` es una rectificativa de la factura `?2` (la primera de su lista
/// o cualquier otra).
const RECTIFIES_2: &str = "i.kind LIKE 'R%'
    AND (i.rectifies_id = ?2
         OR EXISTS (SELECT 1 FROM json_each(i.data, '$.invoice.rectified') r
                      JOIN invoice_issued o ON o.number = json_extract(r.value, '$[0]')
                     WHERE o.id = ?2))";

/// Factura de venta (no rectificativa) vinculada al elemento: (id, número).
fn sale_invoice_of(conn: &Connection, asset_id: i64) -> Result<Option<(i64, String)>> {
    Ok(conn
        .query_row(
            "SELECT s.invoice_id, i.number FROM asset_sale_invoice s
               JOIN invoice_issued i ON i.id = s.invoice_id
              WHERE s.asset_id = ?1 AND i.kind NOT LIKE 'R%'",
            [asset_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// ¿La factura vende este elemento y sólo éste, por todo lo que lleva a ventas? Entonces sus
/// rectificativas son enteras de esta venta.
fn sold_whole(conn: &Connection, asset_id: i64, invoice_id: i64) -> Result<bool> {
    let (assets, mine): (i64, i64) = conn.query_row(
        "SELECT count(*), COALESCE(SUM(CASE WHEN asset_id = ?2 THEN amount END), 0)
           FROM asset_sale_invoice WHERE invoice_id = ?1",
        params![invoice_id, asset_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(assets == 1
        && mine != 0
        && Money::from_cents(mine) == total(&invoice_revenue(conn, invoice_id)?))
}

/// Nombre del elemento del inmovilizado cuya venta lleva la factura `invoice_id` (como factura
/// de venta o como rectificativa de ésta), si alguno.
pub(crate) fn sale_of_invoice(conn: &Connection, invoice_id: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT a.name FROM asset_sale_invoice s JOIN fixed_asset a ON a.id = s.asset_id
              WHERE s.invoice_id = ?1 ORDER BY a.id LIMIT 1",
            [invoice_id],
            |r| r.get(0),
        )
        .optional()?)
}

/// Saldo deudor de la 543 hoy y lo que cada venta sin factura de venta vinculada (con precio)
/// tiene aún en ella, por elemento. El saldo suma todos los ejercicios (cada cierre lo salda y la
/// apertura siguiente lo repone) y se reparte de la venta más reciente a la más antigua: lo que
/// ya se cobró o se corrigió con otros asientos (un cobro conciliado contra la 543, un asiento
/// manual de antes de CONTABILIDAD-08) suele ser lo más antiguo.
pub(crate) fn unbilled_in_543(conn: &Connection) -> Result<(Money, BTreeMap<i64, Money>)> {
    let balance: i64 = conn.query_row(
        "SELECT COALESCE(SUM(debit - credit), 0) FROM journal_line WHERE account LIKE '543%'",
        [],
        |r| r.get(0),
    )?;
    let balance = Money::from_cents(balance);
    let mut st = conn.prepare(
        "SELECT a.id, a.sale_price FROM fixed_asset a
          WHERE a.disposal_entry IS NOT NULL AND a.disposed_on IS NOT NULL
            AND COALESCE(a.sale_price, 0) > 0
            AND NOT EXISTS (SELECT 1 FROM asset_sale_invoice s
                              JOIN invoice_issued i ON i.id = s.invoice_id
                             WHERE s.asset_id = a.id AND i.kind NOT LIKE 'R%')
          ORDER BY a.disposed_on DESC, a.id DESC",
    )?;
    let sales: Vec<(i64, i64)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let mut left = balance.max(Money::ZERO);
    let mut out = BTreeMap::new();
    for (id, price) in sales {
        let here = Money::from_cents(price).min(left);
        left -= here;
        out.insert(id, here);
    }
    Ok((balance, out))
}

/// La factura de venta salda en la 543 el precio de la baja: sólo se vincula si la 543 aún lo
/// tiene. Si un cobro conciliado contra la 543 o un asiento manual ya lo ha bajado, la dejaría en
/// negativo y el cobro contaría dos veces (en la 543 y en la 430 de la factura).
fn check_543(conn: &Connection, asset_id: i64, s: &Sold, number: &str) -> Result<()> {
    let (_, unbilled) = unbilled_in_543(conn)?;
    let left = unbilled.get(&asset_id).copied().unwrap_or(Money::ZERO);
    if left < s.price {
        return Err(other(format!(
            "la 543 ya no tiene el precio de «{}» ({}; le quedan {left}): un cobro o un asiento ya \
             la ha bajado. Si conciliaste el cobro del banco contra la 543, deshaz esa conciliación \
             en Banco, vincula {number} y concilia el cobro con ella; si lo corregiste con un \
             asiento manual, no la vincules",
            s.name, s.price
        )));
    }
    Ok(())
}

/// Resultado (beneficio positivo, pérdida negativa) que la venta de `asset_id` lleva ya en el
/// ejercicio `year`: el de la baja, si es de ese ejercicio, y los cambios de sus facturas.
fn result_in_year(
    conn: &Connection,
    asset_id: i64,
    year: i32,
    (gain, loss): (&str, &str),
) -> Result<Money> {
    let cents: i64 = conn.query_row(
        "SELECT COALESCE(SUM(l.credit - l.debit), 0) FROM journal_line l
           JOIN journal_entry e ON e.id = l.entry_id
          WHERE e.origin_kind = 'BajaInmovilizado' AND e.origin_id = ?1 AND e.fiscal_year = ?2
            AND l.account IN (?3, ?4)",
        params![asset_id, year, gain, loss],
        |r| r.get(0),
    )?;
    Ok(Money::from_cents(cents))
}

/// Líneas que cambian en `change` el resultado de una venta que en ese ejercicio ya lleva
/// `so_far`: primero contra lo que hay (una rebaja quita beneficio antes de dar pérdida; una
/// subida quita pérdida antes de dar beneficio) y sólo lo que pase, al otro lado. Así la venta
/// queda con beneficio o con pérdida, no con los dos (PGC: la 671 es la pérdida de la baja y la
/// 771 su beneficio).
fn result_lines(
    change: Money,
    so_far: Money,
    (gain, loss): (&str, &str),
) -> Result<Vec<EntryLine>> {
    let mut out = Vec::new();
    if change.is_negative() {
        let less_gain = (-change).min(so_far.max(Money::ZERO));
        out.extend(EntryLine::signed(code(gain)?, less_gain, true));
        out.extend(EntryLine::signed(code(loss)?, -change - less_gain, true));
    } else {
        let less_loss = change.min((-so_far).max(Money::ZERO));
        out.extend(EntryLine::signed(code(loss)?, less_loss, false));
        out.extend(EntryLine::signed(code(gain)?, change - less_loss, false));
    }
    Ok(out)
}

/// Vincula la factura emitida `invoice_id` a la venta del elemento `asset_id`, ya dado de baja
/// (dentro de la transacción del llamador). Con una factura de venta, `amount` es la parte de su
/// base que es de este elemento (por defecto, la que queda sin vincular, sin pasar del precio de
/// la baja) y el asiento salda toda la 543 de la baja (que ha de tenerla: ver [`check_543`]); con
/// una rectificativa de ésta, la parte de su diferencia (por defecto, toda si la factura vende
/// sólo este elemento y entero). El asiento va con la fecha de la factura, en su ejercicio.
pub(crate) fn link_in(
    conn: &Connection,
    asset_id: i64,
    invoice_id: i64,
    amount: Option<Money>,
    actor: Actor,
    now: OffsetDateTime,
) -> Result<SaleLink> {
    let s = sold(conn, asset_id)?;
    let inv = issued(conn, invoice_id)?;
    let n = &inv.number;
    if inv.status != "emitida" {
        return Err(other(format!(
            "{n} está anulada: no puede ser la factura de una venta"
        )));
    }
    if inv.origin != "app" {
        return Err(other(format!(
            "{n} viene del libro de la gestoría y no tiene asiento en TaxPain: no hay ventas que \
             pasar a la baja"
        )));
    }
    let parts = invoice_revenue(conn, invoice_id)?;
    if total(&parts).is_zero() {
        return Err(other(format!(
            "{n} no lleva nada a ventas (¿es una factura de anticipo?): vincula la factura final"
        )));
    }
    let already: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM asset_sale_invoice WHERE asset_id = ?1 AND invoice_id = ?2)",
        params![asset_id, invoice_id],
        |r| r.get(0),
    )?;
    if already {
        return Err(other(format!("{n} ya está vinculada a «{}»", s.name)));
    }
    if inv.issue_date < s.acquired {
        return Err(other(format!(
            "{n} es del {}, de antes del alta de «{}» ({}): no puede ser su venta",
            dmy(inv.issue_date),
            s.name,
            dmy(s.acquired)
        )));
    }
    let open = total(&parts) - linked(conn, invoice_id)?;
    let fy = accounting::fiscal_year_for_date(conn, inv.issue_date)?;
    if accounting::year_closed(conn, fy.year)? {
        return Err(other(format!(
            "{n} es del ejercicio {}, que está cerrado: reábrelo desde Cierre para vincularla",
            fy.label()
        )));
    }
    let (gain, loss) = accounting::result_accounts(&s.account);
    let mut warnings = Vec::new();
    let (amount, change, concept) = if inv.rectifying() {
        let rectified = rectified_ids(conn, invoice_id)?;
        let sale = sale_invoice_of(conn, asset_id)?.filter(|(id, _)| rectified.contains(id));
        let Some((sale, _)) = sale else {
            return Err(other(format!(
                "{n} no rectifica la factura de venta de «{}»",
                s.name
            )));
        };
        if open.is_zero() {
            return Err(other(format!("{n} ya está pasada entera a la venta")));
        }
        let a = match amount {
            Some(a) => a,
            None if rectified.len() == 1 && sold_whole(conn, asset_id, sale)? => open,
            None if rectified.len() > 1 => {
                return Err(other(format!(
                    "di qué parte de {n} es de «{}»: rectifica varias facturas",
                    s.name
                )));
            }
            None => {
                return Err(other(format!(
                    "di qué parte de {n} es de «{}»: su factura de venta vende más cosas",
                    s.name
                )));
            }
        };
        if a.is_zero() || a.is_negative() != open.is_negative() || a.abs() > open.abs() {
            return Err(other(format!(
                "el importe de {n} para «{}» ha de ir con su signo y no pasar de {open}",
                s.name
            )));
        }
        // Una rectificativa anula la venta como mucho: no la deja por debajo de cero.
        let so_far = invoiced(conn, asset_id)?;
        if (so_far + a).is_negative() {
            return Err(other(format!(
                "{n} no puede bajar de cero la venta de «{}» (lleva {so_far}): pasa a cada venta \
                 su parte",
                s.name
            )));
        }
        (a, a, format!("Venta de {}: rectificativa {n}", s.name))
    } else {
        if let Some((_, current)) = sale_invoice_of(conn, asset_id)? {
            return Err(other(format!(
                "«{}» ya tiene su factura de venta ({current}): desvincúlala antes",
                s.name
            )));
        }
        // Facturada antes de la entrega, en un ejercicio anterior: sus ventas no la cuentan (la
        // venta es del ejercicio de la baja) y, hasta la baja, la 543 queda en él con saldo
        // acreedor, como lo facturado por adelantado.
        if fy.start < accounting::fiscal_year_for_date(conn, s.date)?.start {
            warnings.push(format!(
                "{n} es del {}, de un ejercicio anterior al de la baja: su base sale de las ventas \
                 de ese ejercicio y allí la 543 queda con saldo acreedor hasta la baja (lo \
                 facturado por adelantado).",
                dmy(inv.issue_date)
            ));
        }
        if open.cents() <= 0 {
            return Err(other(format!(
                "{n} ya está vinculada entera a otras ventas del inmovilizado"
            )));
        }
        let a = amount.unwrap_or(if s.price.is_zero() {
            open
        } else {
            open.min(s.price)
        });
        if a.cents() <= 0 {
            return Err(other("el importe de este elemento ha de ser positivo"));
        }
        if a > open {
            return Err(other(format!(
                "el importe no puede pasar de lo que {n} lleva a ventas sin vincular ({open})"
            )));
        }
        check_543(conn, asset_id, &s, n)?;
        (a, a - s.price, format!("Venta de {}: factura {n}", s.name))
    };
    // Debe 70x por lo que la factura llevó a ventas (Haber si la rectificativa lo rebajó); la
    // factura de venta salda la 543 de la baja; la diferencia, al resultado de la venta en el
    // ejercicio del asiento (contra el que ya tiene).
    let mut lines: Vec<EntryLine> = split(amount, &parts)
        .into_iter()
        .filter_map(|(account, m)| EntryLine::signed(account, m, true))
        .collect();
    if !inv.rectifying() {
        lines.extend(EntryLine::signed(code("543")?, s.price, false));
    }
    let so_far = result_in_year(conn, asset_id, fy.year, (gain, loss))?;
    lines.extend(result_lines(change, so_far, (gain, loss))?);
    let entry = JournalEntry {
        date: inv.issue_date,
        concept,
        origin: Origin::BajaInmovilizado(asset_id),
        lines,
    };
    let entry_id = record_entry(conn, &entry, now)?;
    conn.execute(
        "INSERT INTO asset_sale_invoice(asset_id, invoice_id, amount, entry_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![asset_id, invoice_id, amount.cents(), entry_id, now_str(now)],
    )?;
    audit::append(
        conn,
        &now_str(now),
        actor,
        "asset.sale_link",
        &s.name,
        &serde_json::json!({
            "asset": asset_id,
            "invoice": n,
            "amount": amount.cents(),
            "entry": entry_id,
        }),
    )?;
    if !inv.rectifying() {
        let rest = open - amount;
        if rest.cents() > 0 {
            warnings.push(format!(
                "El resto de {n} ({rest}) sigue en ventas: si vende otro elemento, vincúlala \
                 también a esa venta."
            ));
        }
        warnings.extend(link_rectifications(conn, asset_id, invoice_id, now)?);
    }
    Ok(SaleLink {
        entry_id,
        amount,
        result_change: change,
        warnings,
    })
}

/// Pasa a la venta las rectificativas ya emitidas de su factura si ésta la vende entera (sólo
/// este elemento y por todo). Una que deja la venta a cero o menos (la anula) no se pasa sola.
fn link_rectifications(
    conn: &Connection,
    asset_id: i64,
    invoice_id: i64,
    now: OffsetDateTime,
) -> Result<Vec<String>> {
    let pending = rectification_candidates(conn, asset_id, invoice_id)?;
    let mut out = Vec::new();
    if pending.is_empty() {
        return Ok(out);
    }
    if !sold_whole(conn, asset_id, invoice_id)? {
        let numbers: Vec<&str> = pending.iter().map(|c| c.number.as_str()).collect();
        out.push(format!(
            "Tiene rectificativas ({}): pasa a esta venta la parte que le toque en Inmovilizado.",
            numbers.join(", ")
        ));
        return Ok(out);
    }
    for c in pending {
        // De varias facturas: no se sabe qué parte es de esta venta.
        if c.suggested.is_none() {
            out.push(format!(
                "La rectificativa {} rectifica varias facturas: pasa a esta venta la parte que le \
                 toque en Inmovilizado.",
                c.number
            ));
            continue;
        }
        if (invoiced(conn, asset_id)? + c.open).cents() <= 0 {
            out.push(cancels_warning(&c.number));
            continue;
        }
        let l = link_in(
            conn,
            asset_id,
            c.invoice_id,
            Some(c.open),
            Actor::System,
            now,
        )?;
        out.push(format!(
            "La rectificativa {} ({}) también pasa al resultado de la venta.",
            c.number, l.amount
        ));
    }
    Ok(out)
}

fn cancels_warning(number: &str) -> String {
    format!(
        "{number} anula la venta: no se pasa sola, porque TaxPain no deshace la baja. Si el bien \
         vuelve, dalo de alta otra vez y ajusta a mano; si no, pásala a la venta en Inmovilizado."
    )
}

/// Vincula la factura emitida `invoice_id` a la venta del elemento `asset_id` (ya dado de baja):
/// ver [`link_in`]. `amount` en céntimos; `None`, el propuesto.
pub fn link_sale_invoice(
    v: &Vault,
    asset_id: i64,
    invoice_id: i64,
    amount: Option<Money>,
    now: OffsetDateTime,
) -> Result<SaleLink> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let l = link_in(&tx, asset_id, invoice_id, amount, Actor::User, now)?;
    tx.commit()?;
    Ok(l)
}

/// Desvincula una factura de la venta de un elemento: borra el asiento que la pasaba y vuelve a
/// contar como ventas (la factura de venta deja otra vez el precio en la 543). Se desvinculan en
/// orden inverso al que se vincularon (el resultado de cada asiento cuenta con los anteriores):
/// la factura de venta, después de sus rectificativas. Los documentos y las observaciones del
/// asiento borrado pasan al de la factura.
pub fn unlink_sale_invoice(
    v: &Vault,
    asset_id: i64,
    invoice_id: i64,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let (amount, entry): (i64, i64) = conn
        .query_row(
            "SELECT amount, entry_id FROM asset_sale_invoice WHERE asset_id = ?1 AND invoice_id = ?2",
            params![asset_id, invoice_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let inv = issued(conn, invoice_id)?;
    let name: String = conn.query_row(
        "SELECT name FROM fixed_asset WHERE id = ?1",
        [asset_id],
        |r| r.get(0),
    )?;
    if !inv.rectifying() {
        let mut st = conn.prepare(&format!(
            "SELECT i.number FROM asset_sale_invoice s JOIN invoice_issued i ON i.id = s.invoice_id
              WHERE s.asset_id = ?1 AND {RECTIFIES_2}
              ORDER BY i.issue_date, i.id"
        ))?;
        let rects: Vec<String> = st
            .query_map(params![asset_id, invoice_id], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        if !rects.is_empty() {
            return Err(other(format!(
                "desvincula antes sus rectificativas ({})",
                rects.join(", ")
            )));
        }
    }
    let later: Option<String> = conn
        .query_row(
            "SELECT i.number FROM asset_sale_invoice s JOIN invoice_issued i ON i.id = s.invoice_id
              WHERE s.asset_id = ?1 AND s.entry_id > ?2 ORDER BY s.entry_id DESC LIMIT 1",
            params![asset_id, entry],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(later) = later {
        return Err(other(format!(
            "desvincula antes {later}: se pasó a la venta después y su asiento cuenta con el de {}",
            inv.number
        )));
    }
    let year: i32 = conn.query_row(
        "SELECT fiscal_year FROM journal_entry WHERE id = ?1",
        [entry],
        |r| r.get(0),
    )?;
    if accounting::year_closed(conn, year)? {
        return Err(other(format!(
            "el asiento que pasa {} a la venta es del ejercicio {}, que está cerrado: reábrelo \
             desde Cierre",
            inv.number,
            accounting::fiscal_year_conn(conn, year)?.label()
        )));
    }
    let before = accounting::load_entry(conn, entry)?;
    // Los adjuntos y las observaciones del asiento van al de la factura (del mismo ejercicio,
    // abierto) o, si no lo tuviera, al de la baja: el borrado no se los lleva.
    let target: Option<i64> = conn
        .query_row(
            "SELECT id FROM journal_entry WHERE origin_kind = 'FacturaEmitida' AND origin_id = ?1",
            [invoice_id],
            |r| r.get(0),
        )
        .optional()?;
    let target = match target {
        Some(t) => t,
        None => conn.query_row(
            "SELECT disposal_entry FROM fixed_asset WHERE id = ?1",
            [asset_id],
            |r| r.get(0),
        )?,
    };
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM asset_sale_invoice WHERE asset_id = ?1 AND invoice_id = ?2",
        params![asset_id, invoice_id],
    )?;
    let moved = accounting::move_attachments(&tx, entry, target)?;
    accounting::delete_entry_rows(&tx, entry)?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "asset.sale_unlink",
        &name,
        &serde_json::json!({
            "asset": asset_id,
            "invoice": inv.number,
            "amount": amount,
            "deleted": before,
            "moved": moved,
        }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Facturas que se pueden vincular a la venta de un elemento, sin las ya vinculadas a él ni las
/// de ejercicios cerrados. Sin factura de venta aún: las emitidas desde el alta que tienen ventas
/// sin vincular, las más cercanas a la baja primero (sin baja, las más recientes) y como mucho
/// [`MAX_CANDIDATES`]. Con `query` (número, cliente o NIF, sin distinguir mayúsculas ni tildes),
/// sólo las que lo llevan; sin ella, sin las simplificadas (F2), que en una tienda serían cientos.
/// Con factura de venta: sus rectificativas pendientes.
pub fn sale_invoice_candidates(
    v: &Vault,
    asset_id: i64,
    query: &str,
) -> Result<Vec<SaleCandidate>> {
    let conn = v.connection();
    let (acquired, disposed, entry, price): (String, Option<String>, Option<i64>, Option<i64>) =
        conn.query_row(
            "SELECT acquired_on, disposed_on, disposal_entry, sale_price FROM fixed_asset
              WHERE id = ?1",
            [asset_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    // Dado de baja fuera de TaxPain (en el inventario de la apertura): no hay venta que vincular.
    if disposed.is_some() && entry.is_none() {
        return Ok(Vec::new());
    }
    if let Some((sale, _)) = sale_invoice_of(conn, asset_id)? {
        return rectification_candidates(conn, asset_id, sale);
    }
    let disposed = disposed.as_deref().map(parse_date).transpose()?;
    let price = Money::from_cents(price.unwrap_or(0));
    let query = sin_tildes(query.trim());
    let filter = if query.is_empty() {
        "i.kind NOT LIKE 'R%' AND i.kind <> 'F2' AND i.issue_date >= ?2"
    } else {
        "i.kind NOT LIKE 'R%' AND i.issue_date >= ?2"
    };
    let mut found: Vec<Found> = candidates(conn, asset_id, filter, &acquired)?
        .into_iter()
        .filter(|f| f.c.open.cents() > 0)
        .filter(|f| {
            query.is_empty()
                || [
                    Some(&f.c.number),
                    f.c.recipient_name.as_ref(),
                    f.tax_id.as_ref(),
                ]
                .into_iter()
                .flatten()
                .any(|s| sin_tildes(s).contains(&query))
        })
        .collect();
    // La factura de la venta suele ser de cerca de la baja.
    match disposed {
        Some(d) => found.sort_by_key(|f| {
            (
                (f.c.issue_date - d).whole_days().abs(),
                Reverse(f.c.issue_date),
                Reverse(f.c.invoice_id),
            )
        }),
        None => found.sort_by_key(|f| (Reverse(f.c.issue_date), Reverse(f.c.invoice_id))),
    }
    found.truncate(MAX_CANDIDATES);
    Ok(found
        .into_iter()
        .map(|f| {
            let suggested = if disposed.is_some() && !price.is_zero() {
                f.c.open.min(price)
            } else {
                f.c.open
            };
            SaleCandidate {
                suggested: Some(suggested),
                ..f.c
            }
        })
        .collect())
}

/// ¿Se puede vincular la emitida `invoice_id` a la venta de `asset_id`, que aún no tiene factura?
/// Como en [`sale_invoice_candidates`]: es de TaxPain (no del libro de la gestoría), de después del
/// alta, su asiento es de un ejercicio abierto y le quedan ventas sin vincular.
pub(crate) fn linkable(conn: &Connection, asset_id: i64, invoice_id: i64) -> Result<bool> {
    Ok(candidates(
        conn,
        asset_id,
        "i.id = ?2 AND i.issue_date >= (SELECT acquired_on FROM fixed_asset WHERE id = ?1)",
        &invoice_id.to_string(),
    )?
    .iter()
    .any(|f| f.c.open.cents() > 0))
}

/// Ejercicio cerrado («2026», o «2025/2026» si no es el año natural) del día `date`, si lo está:
/// para vincular una factura o una rectificativa de ese día hay que reabrirlo antes (ver
/// [`link_in`]). Sólo lee: no crea el ejercicio.
pub(crate) fn closed_year_at(conn: &Connection, date: Date) -> Result<Option<String>> {
    let row: Option<(i32, String, String)> = conn
        .query_row(
            "SELECT year, start_date, end_date FROM fiscal_year
              WHERE closed = 1 AND start_date <= ?1 AND end_date >= ?1",
            [date.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    Ok(row.map(|(year, start, end)| {
        if start.get(..4) == end.get(..4) {
            year.to_string()
        } else {
            format!(
                "{}/{}",
                start.get(..4).unwrap_or_default(),
                end.get(..4).unwrap_or_default()
            )
        }
    }))
}

/// ¿Viene la emitida `invoice_id` del libro de la gestoría? Entonces no tiene asiento en TaxPain
/// y no se puede vincular.
pub(crate) fn imported(conn: &Connection, invoice_id: i64) -> Result<bool> {
    Ok(issued(conn, invoice_id)?.origin != "app")
}

/// Rectificativas sin pasar a la venta de `asset_id` de su factura de venta `invoice_id` (también
/// las que rectifican además otras facturas).
fn rectification_candidates(
    conn: &Connection,
    asset_id: i64,
    invoice_id: i64,
) -> Result<Vec<SaleCandidate>> {
    let whole = sold_whole(conn, asset_id, invoice_id)?;
    let so_far = invoiced(conn, asset_id)?;
    let sale = issued(conn, invoice_id)?.number;
    let mut found = candidates(conn, asset_id, RECTIFIES_2, &invoice_id.to_string())?;
    found.sort_by_key(|f| (f.c.issue_date, f.c.invoice_id));
    let mut out = Vec::new();
    for c in found.into_iter().map(|f| f.c).filter(|c| !c.open.is_zero()) {
        // Si la factura vende más elementos, o la rectificativa es de más facturas, no se sabe
        // qué parte es de éste.
        let whole = whole && rectified_ids(conn, c.invoice_id)?.len() == 1;
        out.push(SaleCandidate {
            suggested: whole.then_some(c.open),
            cancels_sale: whole && (so_far + c.open).cents() <= 0,
            rectifies: Some(sale.clone()),
            ..c
        });
    }
    Ok(out)
}

/// Factura candidata con lo que hace falta para buscarla.
struct Found {
    c: SaleCandidate,
    /// NIF del destinatario.
    tax_id: Option<String>,
}

/// Facturas emitidas en TaxPain, con asiento en un ejercicio abierto y sin vincular aún a
/// `asset_id`, que cumplen `filter` (con `?2` = `arg`).
fn candidates(conn: &Connection, asset_id: i64, filter: &str, arg: &str) -> Result<Vec<Found>> {
    let mut st = conn.prepare(&format!(
        "SELECT i.id, i.number, i.issue_date, json_extract(i.data, '$.invoice.operation_date'),
                i.recipient_name,
                (SELECT COALESCE(SUM(l.credit - l.debit), 0) FROM journal_line l
                  WHERE l.entry_id = e.id AND substr(l.account, 1, 1) = '7'),
                (SELECT COALESCE(SUM(s.amount), 0) FROM asset_sale_invoice s
                  WHERE s.invoice_id = i.id),
                (SELECT o.number FROM invoice_issued o WHERE o.id = i.rectifies_id),
                i.recipient_tax_id
           FROM invoice_issued i
           JOIN journal_entry e ON e.origin_kind = 'FacturaEmitida' AND e.origin_id = i.id
           JOIN fiscal_year f ON f.year = e.fiscal_year AND f.closed = 0
          WHERE i.status = 'emitida' AND i.origin = 'app'
            AND NOT EXISTS (SELECT 1 FROM asset_sale_invoice s
                             WHERE s.invoice_id = i.id AND s.asset_id = ?1)
            AND {filter}"
    ))?;
    let rows = st.query_map(params![asset_id, arg], |r| {
        Ok((
            (
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ),
            (
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, Option<String>>(8)?,
            ),
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (
            (invoice_id, number, date, operation, recipient_name),
            (revenue, linked, rectifies, tax_id),
        ) = row?;
        let base = Money::from_cents(revenue);
        out.push(Found {
            c: SaleCandidate {
                invoice_id,
                number,
                issue_date: parse_date(&date)?,
                operation_date: operation.as_deref().map(parse_date).transpose()?,
                recipient_name,
                base,
                open: base - Money::from_cents(linked),
                suggested: None,
                rectifies,
                cancels_sale: false,
            },
            tax_id,
        });
    }
    Ok(out)
}

/// Bajas de inmovilizado hechas en TaxPain con su venta: precio, facturas vinculadas, lo que
/// sigue en la 543 y las rectificativas pendientes.
pub fn asset_sales(v: &Vault) -> Result<Vec<AssetSale>> {
    let conn = v.connection();
    let (_, in_543) = unbilled_in_543(conn)?;
    let mut st = conn.prepare(
        "SELECT id, disposed_on, COALESCE(sale_price, 0) FROM fixed_asset
          WHERE disposal_entry IS NOT NULL AND disposed_on IS NOT NULL ORDER BY id",
    )?;
    let assets: Vec<(i64, String, i64)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let mut lines = conn.prepare(
        "SELECT s.invoice_id, i.number, i.issue_date, s.amount, s.entry_id,
                CASE WHEN i.kind LIKE 'R%'
                     THEN (SELECT o.number FROM invoice_issued o WHERE o.id = i.rectifies_id) END
           FROM asset_sale_invoice s JOIN invoice_issued i ON i.id = s.invoice_id
          WHERE s.asset_id = ?1 ORDER BY i.issue_date, i.id",
    )?;
    let mut out = Vec::new();
    for (asset_id, date, price) in assets {
        let rows = lines.query_map([asset_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?;
        let mut invoices = Vec::new();
        for row in rows {
            let (invoice_id, number, issue_date, amount, entry_id, rectifies) = row?;
            invoices.push(SaleInvoice {
                invoice_id,
                number,
                issue_date: parse_date(&issue_date)?,
                amount: Money::from_cents(amount),
                rectifies,
                entry_id,
            });
        }
        let price = Money::from_cents(price);
        // Sus rectificativas son de su factura de venta (la de varias facturas, también de otras).
        let number = invoices
            .iter()
            .find(|i| i.rectifies.is_none())
            .map(|i| i.number.clone());
        for i in invoices.iter_mut().filter(|i| i.rectifies.is_some()) {
            if let Some(n) = &number {
                i.rectifies = Some(n.clone());
            }
        }
        let sale = invoices.iter().find(|i| i.rectifies.is_none());
        let result: i64 = conn.query_row(
            "SELECT COALESCE(SUM(l.credit - l.debit), 0) FROM journal_line l
               JOIN journal_entry e ON e.id = l.entry_id
              WHERE e.origin_kind = 'BajaInmovilizado' AND e.origin_id = ?1
                AND (l.account LIKE '67%' OR l.account LIKE '77%')",
            [asset_id],
            |r| r.get(0),
        )?;
        out.push(AssetSale {
            asset_id,
            date: parse_date(&date)?,
            price,
            invoiced: invoices.iter().map(|i| i.amount).sum(),
            unbilled: match sale {
                Some(_) => Money::ZERO,
                None => in_543.get(&asset_id).copied().unwrap_or(Money::ZERO),
            },
            result: Money::from_cents(result),
            pending: match sale {
                Some(s) => rectification_candidates(conn, asset_id, s.invoice_id)?,
                None => Vec::new(),
            },
            invoices,
        });
    }
    Ok(out)
}

/// Tras emitir una rectificativa (después de confirmar la emisión, sin bloquearla): si rectifica
/// la factura de venta de un elemento que vende entero, su diferencia pasa sola al resultado de
/// la venta. Devuelve lo que conviene decir al usuario (también si no se ha pasado sola).
pub fn link_rectifying_sale(
    v: &Vault,
    invoice_id: i64,
    now: OffsetDateTime,
) -> Result<Option<String>> {
    let conn = v.connection();
    let inv = issued(conn, invoice_id)?;
    let Some(sale) = inv
        .rectifies_id
        .filter(|_| inv.rectifying() && inv.status == "emitida")
    else {
        return Ok(None);
    };
    // De varias facturas: no se pasa sola, porque no dice qué parte es de cada una.
    let rectified = rectified_ids(conn, invoice_id)?;
    if rectified.len() > 1 {
        let mut sold = false;
        for id in &rectified {
            sold |= sale_of_invoice(conn, *id)?.is_some();
        }
        return Ok(sold.then(|| {
            "rectifica varias facturas y alguna vende un elemento del inmovilizado: pasa a cada \
             venta su parte en Contabilidad › Inmovilizado"
                .to_owned()
        }));
    }
    let mut st = conn.prepare(
        "SELECT s.asset_id, a.name FROM asset_sale_invoice s JOIN fixed_asset a ON a.id = s.asset_id
          WHERE s.invoice_id = ?1 ORDER BY s.asset_id",
    )?;
    let assets: Vec<(i64, String)> = st
        .query_map([sale], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let [(asset_id, name)] = assets.as_slice() else {
        return Ok((!assets.is_empty()).then(|| {
            "rectifica una factura que vende varios elementos del inmovilizado: pasa a cada \
             venta su parte en Contabilidad › Inmovilizado"
                .to_owned()
        }));
    };
    let open = total(&invoice_revenue(conn, invoice_id)?) - linked(conn, invoice_id)?;
    if open.is_zero() {
        return Ok(None);
    }
    if !sold_whole(conn, *asset_id, sale)? {
        return Ok(Some(format!(
            "rectifica la venta de «{name}»: pasa su parte a la venta en Contabilidad › \
             Inmovilizado"
        )));
    }
    if (invoiced(conn, *asset_id)? + open).cents() <= 0 {
        return Ok(Some(cancels_warning(&inv.number)));
    }
    let tx = conn.unchecked_transaction()?;
    let l = link_in(&tx, *asset_id, invoice_id, Some(open), Actor::System, now)?;
    tx.commit()?;
    Ok(Some(format!(
        "su diferencia ({}) pasa al resultado de la venta de «{name}» (Contabilidad › \
         Inmovilizado)",
        l.amount
    )))
}

/// Ventas del inmovilizado dadas de baja de `from` a `to` (con precio o con factura): cuántas hay
/// y, de las que falta algo, qué y cómo arreglarlo. Para el cierre. Falta algo si la venta no
/// tiene factura vinculada y su precio sigue en la 543 (ver [`unbilled_in_543`]; si la 543 se
/// saldó con su cobro o con un asiento, ya no), si tiene rectificativas sin pasar a la venta o si
/// la 543 ha quedado en negativo (un cobro contado dos veces).
pub(crate) fn unfinished_sales(
    conn: &Connection,
    from: Date,
    to: Date,
) -> Result<(usize, Vec<String>)> {
    let (balance, in_543) = unbilled_in_543(conn)?;
    let mut st = conn.prepare(
        "SELECT a.id, a.name FROM fixed_asset a
          WHERE a.disposal_entry IS NOT NULL AND a.disposed_on BETWEEN ?1 AND ?2
            AND (COALESCE(a.sale_price, 0) > 0
                 OR EXISTS (SELECT 1 FROM asset_sale_invoice s WHERE s.asset_id = a.id))
          ORDER BY a.disposed_on, a.id",
    )?;
    let sales: Vec<(i64, String)> = st
        .query_map(params![from.to_string(), to.to_string()], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    let mut out = Vec::new();
    for (id, name) in &sales {
        match sale_invoice_of(conn, *id)? {
            None => {
                let left = in_543.get(id).copied().unwrap_or(Money::ZERO);
                if left.cents() > 0 {
                    out.push(format!(
                        "«{name}» sin factura vinculada ({left} en la 543): vincula su factura en \
                         Contabilidad → Inmovilizado o, si no la emitiste en TaxPain, salda la \
                         543 con su cobro o con un asiento"
                    ));
                }
            }
            Some((invoice, _)) => {
                let pending = rectification_candidates(conn, *id, invoice)?;
                if !pending.is_empty() {
                    let numbers: Vec<&str> = pending.iter().map(|c| c.number.as_str()).collect();
                    out.push(format!(
                        "«{name}» con rectificativas sin pasar a la venta ({}): pásalas en \
                         Contabilidad → Inmovilizado",
                        numbers.join(", ")
                    ));
                }
            }
        }
    }
    if !sales.is_empty() && balance.is_negative() {
        out.push(format!(
            "La 543 tiene saldo acreedor ({balance}): un cobro o un asiento la ha bajado de más \
             (¿un cobro conciliado contra la 543 de una venta que ya tiene su factura?); revisa su \
             mayor"
        ));
    }
    Ok((sales.len(), out))
}
