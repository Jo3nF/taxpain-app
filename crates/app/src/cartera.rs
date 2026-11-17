//! Cartera de cobros y pagos (vencimientos), como la de a3 (`docs/research/A3.md` §2.7).
//!
//! - Cada cliente o proveedor puede tener sus condiciones en su subcuenta (430…, 400…, 410…):
//!   días de cada plazo (`[30, 60]` fracciona la factura en dos), día de pago fijo, forma de pago
//!   y cuenta de cobro o pago por defecto. Sin condiciones, vale el plazo de la empresa.
//! - Los vencimientos de una factura se calculan con esas condiciones hasta que se fijan: al
//!   cambiar uno a mano, al desglosarlos o al cobrar o pagar uno de ellos (así, cambiar después
//!   las condiciones no mueve lo ya cobrado).
//! - Cada vencimiento queda cobrado o pagado al conciliarlo con el banco o al registrar aquí el
//!   cobro o pago (caja u otra cuenta), con su asiento.
//! - Facturas en moneda extranjera: el vencimiento está en los euros contabilizados (tipo del
//!   devengo); si se cobra o paga por otro importe en euros, la diferencia es una diferencia de
//!   cambio: 768 si es a favor y 668 si es en contra (PGC, NRV 11.ª «Moneda extranjera»: las
//!   partidas monetarias se liquidan al tipo del día y la diferencia va a resultados).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{AccountCode, EntryLine, JournalEntry, Origin};
use taxpain_core::money::Money;
use taxpain_core::sepa::normalize_iban;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, Duration, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;

const TERMS_KEY: &str = "payment_terms";
/// Ley 3/2004 de morosidad: plazo máximo de pago entre empresas.
const MAX_DAYS: u16 = 60;
const MAX_INSTALLMENTS: usize = 24;
pub const METHODS: [&str; 5] = ["transferencia", "recibo", "efectivo", "tarjeta", "otro"];

/// Limitación de pagos en efectivo (art. 7 Ley 7/2012, redacción de la Ley 11/2021): no se puede
/// pagar ni cobrar en efectivo una operación de 1.000 € o más cuando alguna de las partes actúa
/// como empresario o profesional (la empresa siempre lo es), sumando todos los pagos en que se
/// fraccione (art. 7.Uno.2); 10.000 € si el pagador es un particular no residente que lo
/// acredite. La sanción es el 25 % del importe, solidaria para pagador y cobrador (art. 7.Dos).
pub const CASH_LIMIT: Money = Money::from_euros(1_000, 0);

/// ¿Llega una factura (todos sus plazos) al límite de pagos en efectivo? Es un aviso, no un
/// bloqueo: el cobro o pago ocurrió y hay que contabilizarlo igual.
pub fn cash_limited(invoice_total: Money) -> bool {
    invoice_total.abs() >= CASH_LIMIT
}

/// Plazos de cobro y pago de la empresa, en días desde la fecha de la factura.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentTerms {
    pub customers_days: u16,
    pub suppliers_days: u16,
}

impl Default for PaymentTerms {
    fn default() -> Self {
        PaymentTerms {
            customers_days: 30,
            suppliers_days: 30,
        }
    }
}

pub fn terms(v: &Vault) -> Result<PaymentTerms> {
    Ok(v.setting(TERMS_KEY)?.unwrap_or_default())
}

fn too_long() -> AppError {
    AppError::Other(
        "la Ley 3/2004 de morosidad limita el plazo de pago entre empresas a 60 días".into(),
    )
}

pub fn set_terms(v: &Vault, t: &PaymentTerms) -> Result<()> {
    if t.customers_days > MAX_DAYS || t.suppliers_days > MAX_DAYS {
        return Err(too_long());
    }
    Ok(v.set_setting(TERMS_KEY, t)?)
}

/// Condiciones de cobro o pago de un cliente o proveedor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartyTerms {
    /// Días de cada plazo desde la fecha de la factura, de menor a mayor (`[0]` = al contado).
    pub days: Vec<u16>,
    /// Día de pago fijo del mes: el vencimiento pasa a ese día (o al último del mes si no lo
    /// tiene), en el mismo mes o en el siguiente.
    pub pay_day: Option<u8>,
    /// `transferencia`, `recibo` (domiciliación SEPA), `efectivo`, `tarjeta` u `otro`.
    pub method: String,
    /// Cuenta de cobro o pago por defecto (570, 572x…).
    pub treasury: Option<String>,
    /// IBAN del cliente (recibos domiciliados) o del proveedor (transferencias).
    #[serde(default)]
    pub iban: Option<String>,
    /// Mandato SEPA firmado por el cliente: referencia y fecha de firma (para los recibos).
    #[serde(default)]
    pub mandate_ref: Option<String>,
    #[serde(default)]
    pub mandate_date: Option<Date>,
    /// Clave y subclave del 190 de un proveedor profesional («G01»…«G04», «H01»…«H04»; ver
    /// `taxpain_models::informative::Key190`). `None`: G.01, el tipo general.
    #[serde(default)]
    pub key_190: Option<String>,
}

/// Cliente o proveedor con subcuenta, y sus condiciones si las tiene.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PartyRow {
    pub account: String,
    /// `cliente` o `proveedor`.
    pub role: String,
    pub tax_id: String,
    pub name: String,
    pub terms: Option<PartyTerms>,
    /// Cuenta de ventas (cliente) o de gastos (proveedor) propia, si tiene.
    pub counterpart: Option<String>,
}

fn treasury_code(account: &str) -> Result<AccountCode> {
    let a = AccountCode::new(account.trim()).map_err(|e| AppError::Other(e.to_string()))?;
    if !a.starts_with("57") {
        return Err(AppError::Other(
            "el cobro o pago va a una cuenta de tesorería (570 caja, 572 bancos…)".into(),
        ));
    }
    Ok(a)
}

fn validate_terms(t: &PartyTerms) -> Result<()> {
    if t.days.is_empty() || t.days.len() > 12 {
        return Err(AppError::Other("pon entre 1 y 12 plazos".into()));
    }
    if t.days.windows(2).any(|w| w[0] >= w[1]) {
        return Err(AppError::Other(
            "los días de los plazos van de menor a mayor y sin repetir".into(),
        ));
    }
    if t.days.iter().any(|d| *d > MAX_DAYS) {
        return Err(too_long());
    }
    if t.pay_day.is_some_and(|d| !(1..=31).contains(&d)) {
        return Err(AppError::Other("el día de pago va del 1 al 31".into()));
    }
    if !METHODS.contains(&t.method.as_str()) {
        return Err(AppError::Other("forma de pago no válida".into()));
    }
    if let Some(a) = &t.treasury {
        treasury_code(a)?;
    }
    if let Some(i) = t.iban.as_deref().filter(|i| !i.trim().is_empty()) {
        normalize_iban(i).map_err(|e| AppError::Other(e.to_string()))?;
    }
    if t.mandate_ref
        .as_deref()
        .is_some_and(|m| m.trim().len() > 35)
    {
        return Err(AppError::Other(
            "la referencia del mandato tiene como mucho 35 caracteres".into(),
        ));
    }
    if let Some(k) = t.key_190.as_deref().filter(|k| !k.trim().is_empty())
        && taxpain_models::informative::Key190::from_code(k).is_none()
    {
        return Err(AppError::Other(format!(
            "clave del 190 «{k}» no válida: G.01 a G.04 o H.01 a H.04"
        )));
    }
    Ok(())
}

/// Clave del 190 fijada en las condiciones del proveedor con ese NIF (subcuenta 40x/41x), si
/// la tiene.
pub(crate) fn key_190_of_supplier(conn: &Connection, nif: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT t.key_190 FROM party_terms t
               JOIN third_party_account a ON a.account = t.account
              WHERE a.tax_id = ?1 AND a.role = 'proveedor' AND t.key_190 IS NOT NULL
              LIMIT 1",
            [nif],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

fn load_party_terms(conn: &Connection) -> Result<HashMap<String, PartyTerms>> {
    let mut stmt =
        conn.prepare("SELECT account, days, pay_day, method, treasury, iban, mandate_ref, mandate_date, key_190 FROM party_terms")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            (
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<u8>>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
            ),
            (
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
                r.get::<_, Option<String>>(7)?,
                r.get::<_, Option<String>>(8)?,
            ),
        ))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (
            (account, days, pay_day, method, treasury),
            (iban, mandate_ref, mandate_date, key_190),
        ) = row?;
        let mandate_date = mandate_date.as_deref().map(parse).transpose()?;
        let days: Vec<u16> =
            serde_json::from_str(&days).map_err(|e| AppError::Other(e.to_string()))?;
        out.insert(
            account,
            PartyTerms {
                days,
                pay_day,
                method,
                treasury,
                iban,
                mandate_ref,
                mandate_date,
                key_190,
            },
        );
    }
    Ok(out)
}

/// Clientes y proveedores con subcuenta (los que tienen alguna factura contabilizada).
pub fn parties(v: &Vault) -> Result<Vec<PartyRow>> {
    let conn = v.connection();
    let terms = load_party_terms(conn)?;
    let mut stmt = conn.prepare(
        "SELECT account, role, tax_id, name FROM third_party_account ORDER BY role, name, account",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (account, role, tax_id, name) = row?;
        out.push(PartyRow {
            terms: terms.get(&account).cloned(),
            counterpart: crate::accounting::counterpart_of(conn, &account)?
                .map(|c| c.as_str().to_owned()),
            account,
            role,
            tax_id,
            name,
        });
    }
    Ok(out)
}

/// Guarda (o borra, con `None`) las condiciones de la subcuenta de un tercero. No cambia los
/// vencimientos ya fijados ni los cobrados o pagados.
pub fn set_party_terms(
    v: &Vault,
    account: &str,
    t: Option<&PartyTerms>,
    now: OffsetDateTime,
) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let exists: Option<i64> = tx
        .query_row(
            "SELECT 1 FROM third_party_account WHERE account = ?1",
            [account],
            |r| r.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Err(AppError::Other(
            "esa subcuenta no es de un cliente o proveedor".into(),
        ));
    }
    match t {
        Some(t) => {
            validate_terms(t)?;
            let treasury = t
                .treasury
                .as_deref()
                .map(|a| treasury_code(a).map(|c| c.as_str().to_owned()))
                .transpose()?;
            let iban = t
                .iban
                .as_deref()
                .filter(|i| !i.trim().is_empty())
                .map(normalize_iban)
                .transpose()
                .map_err(|e| AppError::Other(e.to_string()))?;
            let mandate = t
                .mandate_ref
                .as_deref()
                .map(str::trim)
                .filter(|m| !m.is_empty());
            let key_190 = t
                .key_190
                .as_deref()
                .map(|k| k.trim().to_ascii_uppercase())
                .filter(|k| !k.is_empty());
            tx.execute(
                "INSERT INTO party_terms(account, days, pay_day, method, treasury, iban, mandate_ref, mandate_date, updated_at, key_190)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(account) DO UPDATE SET days = excluded.days, pay_day = excluded.pay_day,
                   method = excluded.method, treasury = excluded.treasury, iban = excluded.iban,
                   mandate_ref = excluded.mandate_ref, mandate_date = excluded.mandate_date,
                   updated_at = excluded.updated_at, key_190 = excluded.key_190",
                params![
                    account,
                    serde_json::to_string(&t.days).map_err(|e| AppError::Other(e.to_string()))?,
                    t.pay_day,
                    t.method,
                    treasury,
                    iban,
                    mandate,
                    t.mandate_date.map(|d| d.to_string()),
                    now_str(now),
                    key_190
                ],
            )?;
        }
        None => {
            tx.execute("DELETE FROM party_terms WHERE account = ?1", [account])?;
        }
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "cartera.condiciones",
        account,
        &serde_json::json!({ "terms": t }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Fecha `d` llevada al día de pago `day` (el mismo mes si aún no ha pasado, si no el siguiente;
/// en meses más cortos, el último día).
pub fn to_pay_day(d: Date, day: u8) -> Date {
    let in_month = |year: i32, month: time::Month| {
        let last = month.length(year);
        Date::from_calendar_date(year, month, day.min(last)).unwrap_or(d)
    };
    let same = in_month(d.year(), d.month());
    if same >= d {
        return same;
    }
    let (y, m) = if d.month() == time::Month::December {
        (d.year() + 1, time::Month::January)
    } else {
        (d.year(), d.month().next())
    };
    in_month(y, m)
}

/// Vencimientos calculados: el importe se reparte a partes iguales y el resto de céntimos va al
/// último plazo.
pub fn auto_schedule(
    issue: Date,
    amount: Money,
    days: &[u16],
    pay_day: Option<u8>,
) -> Vec<(Date, Money)> {
    let days: &[u16] = if days.is_empty() { &[0] } else { days };
    let n = i64::try_from(days.len()).unwrap_or(1);
    let part = amount.cents() / n;
    days.iter()
        .enumerate()
        .map(|(i, d)| {
            let mut due = issue + Duration::days(i64::from(*d));
            if let Some(p) = pay_day {
                due = to_pay_day(due, p);
            }
            let cents = if i + 1 == days.len() {
                amount.cents() - part * (n - 1)
            } else {
                part
            };
            (due, Money::from_cents(cents))
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DueItem {
    /// `emitida` (cobro) o `recibida` (pago).
    pub kind: String,
    pub invoice_id: i64,
    /// Número del vencimiento (1, 2…) y cuántos tiene la factura.
    pub seq: i64,
    pub installments: i64,
    pub number: String,
    pub party: String,
    /// Subcuenta del tercero en el asiento de la factura.
    pub party_account: Option<String>,
    pub issue_date: Date,
    pub due_date: Date,
    pub amount: Money,
    /// Importe total de la factura.
    pub invoice_amount: Money,
    /// `pendiente`, `banco` (conciliado con un movimiento) o `registrada` (cobro/pago aquí).
    pub status: String,
    pub settled_on: Option<Date>,
    /// Días de retraso si está pendiente y vencido.
    pub overdue_days: i64,
    /// Forma de pago y cuenta de tesorería por defecto (de las condiciones del tercero).
    pub method: Option<String>,
    pub treasury: Option<String>,
    /// Los vencimientos están fijados (a mano, desglosados o por un cobro o pago).
    pub fixed: bool,
    /// Remesa SEPA en la que va (si no se devolvió).
    pub remittance: Option<String>,
    /// Moneda de la factura si no es el euro (los importes de aquí son los euros
    /// contabilizados).
    pub currency: Option<String>,
    /// La factura (todos sus plazos) llega al límite de pagos en efectivo: ver [`cash_limited`].
    pub cash_limited: bool,
}

/// Línea de diferencia de cambio al cobrar (`collect`) o pagar en euros `actual` lo
/// contabilizado por `booked` (mismo signo): 768 si es a favor, 668 si es en contra (PGC).
pub(crate) fn exchange_difference(
    collect: bool,
    booked: Money,
    actual: Money,
) -> Option<EntryLine> {
    let diff = actual - booked;
    let gain = if collect { diff } else { -diff };
    if gain.is_negative() {
        EntryLine::signed(acc("668"), -gain, true)
    } else {
        EntryLine::signed(acc("768"), gain, false)
    }
}

/// Diferencia de cambio admitida al cobrar o pagar una factura en moneda extranjera: un 10 %
/// de lo contabilizado (lo mismo que separa las sugerencias del banco). Más será un cobro o un
/// pago parcial (o un error al teclear), no una diferencia de cambio.
pub(crate) fn within_exchange_tolerance(booked: Money, actual: Money) -> bool {
    (actual - booked).abs().cents() * 10 <= booked.abs().cents()
}

/// Comprueba el importe en euros de un cobro o pago de una factura en moneda extranjera.
pub(crate) fn check_exchange(item: &DueItem, actual: Money) -> Result<()> {
    if actual == item.amount {
        return Ok(());
    }
    if item.currency.is_none() {
        return Err(AppError::Other(format!(
            "{} es en euros: el cobro o pago es por su importe ({}); un importe distinto sólo \
             se admite en facturas en moneda extranjera (diferencia de cambio)",
            item.number, item.amount
        )));
    }
    if actual.is_zero() || actual.is_negative() != item.amount.is_negative() {
        return Err(AppError::Other("importe en euros no válido".into()));
    }
    if !within_exchange_tolerance(item.amount, actual) {
        return Err(AppError::Other(format!(
            "¿Es un cobro o pago parcial? {actual} se separa más de un 10 % de lo contabilizado \
             ({}): la diferencia no se lleva a 768/668 como si fuera de cambio. Si es una parte, \
             divide antes el vencimiento («Vencimientos», en la Cartera: la parte cobrada o \
             pagada y lo que queda pendiente, en los euros contabilizados) y registra esa parte; \
             si no, revisa los euros",
            item.amount
        )));
    }
    Ok(())
}

fn parse(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]"))
        .map_err(|e| AppError::Other(e.to_string()))
}

fn check_kind(kind: &str) -> Result<()> {
    if matches!(kind, "emitida" | "recibida") {
        Ok(())
    } else {
        Err(AppError::Other("tipo de factura no válido".into()))
    }
}

struct Inv {
    kind: &'static str,
    id: i64,
    number: String,
    party: String,
    issue: Date,
    amount: Money,
    currency: Option<String>,
    /// Subcuenta del tercero si no sale del asiento de la factura (pendientes de apertura).
    account: Option<String>,
}

/// Facturas de la cartera. Las importadas de los libros del gestor no están: sus cobros y pagos
/// ya están en su contabilidad (en el diario importado), así que no salen como pendientes ni
/// vencidas, no se proponen al conciliar el banco y no se les registra otro cobro o pago.
fn invoices(conn: &Connection, only: Option<(&str, i64)>) -> Result<Vec<Inv>> {
    let mut out = Vec::new();
    let queries: [(&'static str, &str); 2] = [
        (
            "emitida",
            // Lo que cambia el saldo del cliente: una rectificativa por sustitución, la diferencia
            // con lo que sustituye; una R2/R3 (modificación de la base por concurso o impago), sólo
            // su cuota (la base se sigue debiendo). Ver `rectificativas_emitidas`.
            "SELECT i.id, i.number, COALESCE(i.recipient_name, 'Clientes varios'), i.issue_date,
                    CASE WHEN i.kind IN ('R2', 'R3') AND i.rect_method = 'I' THEN i.vat + i.surcharge
                         ELSE i.payable - COALESCE(i.replaced_payable, 0) END AS pending,
                    json_extract(i.data, '$.fx.currency.code')
             FROM invoice_issued i WHERE i.status = 'emitida' AND i.origin = 'app'
               AND (?1 IS NULL OR i.id = ?1) AND pending <> 0",
        ),
        (
            "recibida",
            "SELECT r.id, COALESCE(r.number, ''), COALESCE(r.supplier_name, ''), r.issue_date,
                    CASE WHEN r.import_dua = 1 THEN COALESCE(r.vat, 0)
                         ELSE COALESCE(r.base, 0) + COALESCE(r.vat, 0) - COALESCE(r.withholding, 0) END,
                    json_extract(r.fx, '$.currency.code')
             FROM invoice_received r
             WHERE r.status = 'revisada' AND r.posted = 1 AND r.issue_date IS NOT NULL
               AND (?1 IS NULL OR r.id = ?1)",
        ),
    ];
    for (kind, sql) in queries {
        let id = match only {
            Some((k, _)) if k != kind => continue,
            Some((_, id)) => Some(id),
            None => None,
        };
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })?;
        for row in rows {
            let (id, number, party, issue, amount, currency) = row?;
            out.push(Inv {
                kind,
                id,
                number,
                party,
                issue: parse(&issue)?,
                amount: Money::from_cents(amount),
                currency,
                account: None,
            });
        }
    }
    // Cobros y pagos pendientes de antes de TaxPain (saldos iniciales), con el id en negativo.
    if only.is_none_or(|(_, id)| id < 0) {
        for i in crate::apertura::cartera_items(conn, only)? {
            out.push(Inv {
                kind: i.kind,
                id: i.id,
                number: i.number,
                party: i.party,
                issue: i.issue,
                amount: i.amount,
                currency: None,
                account: Some(i.account),
            });
        }
    }
    Ok(out)
}

type Key = (String, i64);
/// Vencimientos fijados: (número, fecha, importe).
type Fixed = HashMap<Key, Vec<(i64, Date, Money)>>;
/// Casados con el banco: (vencimiento o `None` = la factura entera, fecha del movimiento).
type BankMatches = HashMap<Key, Vec<(Option<i64>, Date)>>;

/// Subcuenta del tercero en el asiento de cada factura.
fn party_accounts(conn: &Connection) -> Result<HashMap<Key, String>> {
    let mut stmt = conn.prepare(
        "SELECT e.origin_kind, e.origin_id, l.account FROM journal_line l
         JOIN journal_entry e ON e.id = l.entry_id
         WHERE (e.origin_kind = 'FacturaEmitida' AND l.account LIKE '43%')
            OR (e.origin_kind = 'FacturaRecibida' AND (l.account LIKE '40%' OR l.account LIKE '41%'))
         ORDER BY l.id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<i64>>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (origin, id, account) = row?;
        let Some(id) = id else { continue };
        let kind = if origin == "FacturaEmitida" {
            "emitida"
        } else {
            "recibida"
        };
        out.entry((kind.to_owned(), id)).or_insert(account);
    }
    Ok(out)
}

fn fixed_installments(conn: &Connection) -> Result<Fixed> {
    let mut stmt = conn.prepare(
        "SELECT kind, invoice_id, seq, due_date, amount FROM invoice_installment ORDER BY kind, invoice_id, seq",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
        ))
    })?;
    let mut out: Fixed = HashMap::new();
    for row in rows {
        let (kind, id, seq, due, amount) = row?;
        out.entry((kind, id))
            .or_default()
            .push((seq, parse(&due)?, Money::from_cents(amount)));
    }
    Ok(out)
}

/// Cobros y pagos registrados aquí: (tipo, factura, vencimiento) → fecha.
fn settlements(conn: &Connection) -> Result<HashMap<(String, i64, i64), Date>> {
    let mut stmt = conn.prepare("SELECT kind, invoice_id, seq, date FROM settlement")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (k, id, seq, d) = row?;
        out.insert((k, id, seq), parse(&d)?);
    }
    Ok(out)
}

/// Movimientos del banco casados con facturas: (vencimiento o `None` = la factura entera, fecha).
fn bank_matches(conn: &Connection) -> Result<BankMatches> {
    let mut stmt = conn.prepare(
        "SELECT matched_kind, matched_id, matched_seq, op_date FROM bank_tx
         WHERE matched_kind IN ('emitida','recibida') AND matched_id IS NOT NULL",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Option<i64>>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut out: BankMatches = HashMap::new();
    for row in rows {
        let (k, id, seq, d) = row?;
        out.entry((k, id)).or_default().push((seq, parse(&d)?));
    }
    Ok(out)
}

/// Vencimientos en una remesa no devuelta: (tipo, factura, vencimiento) → referencia.
fn remitted(conn: &Connection) -> Result<HashMap<(String, i64, i64), String>> {
    let mut stmt = conn.prepare(
        "SELECT i.kind, i.invoice_id, i.seq, r.reference FROM remittance_item i
         JOIN remittance r ON r.id = i.remittance_id WHERE i.returned_on IS NULL",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            (
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ),
            r.get::<_, String>(3)?,
        ))
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn load(conn: &Connection, today: Date, only: Option<(&str, i64)>) -> Result<Vec<DueItem>> {
    let company = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [TERMS_KEY],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .map(|s| serde_json::from_str::<PaymentTerms>(&s))
        .transpose()
        .map_err(|e| AppError::Other(e.to_string()))?
        .unwrap_or_default();
    let terms = load_party_terms(conn)?;
    let accounts = party_accounts(conn)?;
    let fixed = fixed_installments(conn)?;
    let settled = settlements(conn)?;
    let bank = bank_matches(conn)?;
    let remitted = remitted(conn)?;
    let mut out = Vec::new();
    for inv in invoices(conn, only)? {
        let key = (inv.kind.to_owned(), inv.id);
        let party_account = accounts.get(&key).cloned().or_else(|| inv.account.clone());
        let pt = party_account.as_ref().and_then(|a| terms.get(a));
        let matches = bank.get(&key).map(Vec::as_slice).unwrap_or(&[]);
        let any_settled = settled
            .keys()
            .any(|(k, id, _)| *k == key.0 && *id == inv.id);
        let (schedule, is_fixed): (Vec<(i64, Date, Money)>, bool) = match fixed.get(&key) {
            Some(rows) if !rows.is_empty() => {
                let mut rows = rows.clone();
                // Si la factura cambió de importe, la diferencia va al último vencimiento.
                let sum: i64 = rows.iter().map(|r| r.2.cents()).sum();
                if let Some(last) = rows.last_mut() {
                    last.2 = Money::from_cents(last.2.cents() + inv.amount.cents() - sum);
                }
                (rows, true)
            }
            _ => {
                let (days, pay_day) = match pt {
                    Some(t) => (t.days.clone(), t.pay_day),
                    None => (
                        vec![if inv.kind == "emitida" {
                            company.customers_days
                        } else {
                            company.suppliers_days
                        }],
                        None,
                    ),
                };
                let mut s = auto_schedule(inv.issue, inv.amount, &days, pay_day);
                // Cobros o pagos anteriores a los fraccionamientos eran de la factura entera.
                if s.len() > 1 && (any_settled || !matches.is_empty()) {
                    s = vec![(s[0].0, inv.amount)];
                }
                (
                    s.into_iter()
                        .enumerate()
                        .map(|(i, (d, m))| (i64::try_from(i).unwrap_or(0) + 1, d, m))
                        .collect(),
                    false,
                )
            }
        };
        let n = i64::try_from(schedule.len()).unwrap_or(1);
        for (seq, due, amount) in schedule {
            let by_bank = matches
                .iter()
                .find(|(s, _)| s.is_none() || *s == Some(seq))
                .map(|(_, d)| *d);
            let (status, settled_on) = match (by_bank, settled.get(&(key.0.clone(), inv.id, seq))) {
                (Some(d), _) => ("banco", Some(d)),
                (None, Some(d)) => ("registrada", Some(*d)),
                _ => ("pendiente", None),
            };
            let overdue_days = if status == "pendiente" {
                (today - due).whole_days().max(0)
            } else {
                0
            };
            out.push(DueItem {
                kind: inv.kind.to_owned(),
                invoice_id: inv.id,
                seq,
                installments: n,
                number: inv.number.clone(),
                party: inv.party.clone(),
                party_account: party_account.clone(),
                issue_date: inv.issue,
                due_date: due,
                amount,
                invoice_amount: inv.amount,
                status: status.into(),
                settled_on,
                overdue_days,
                method: pt.map(|t| t.method.clone()),
                treasury: pt.and_then(|t| t.treasury.clone()),
                fixed: is_fixed,
                remittance: remitted.get(&(key.0.clone(), inv.id, seq)).cloned(),
                currency: inv.currency.clone(),
                cash_limited: cash_limited(inv.amount),
            });
        }
    }
    out.sort_by(|a, b| {
        (a.due_date, &a.kind, a.invoice_id, a.seq).cmp(&(b.due_date, &b.kind, b.invoice_id, b.seq))
    });
    Ok(out)
}

/// Toda la cartera (facturas emitidas no anuladas y recibidas revisadas), un elemento por
/// vencimiento y ordenada por fecha de vencimiento.
pub fn list(v: &Vault, today: Date) -> Result<Vec<DueItem>> {
    load(v.connection(), today, None)
}

/// Vencimientos de una factura.
pub fn installments(conn: &Connection, kind: &str, invoice_id: i64) -> Result<Vec<DueItem>> {
    check_kind(kind)?;
    let today = OffsetDateTime::now_utc().date();
    let items = load(conn, today, Some((kind, invoice_id)))?;
    if items.is_empty() {
        let imported: bool = conn.query_row(
            if kind == "emitida" {
                "SELECT EXISTS(SELECT 1 FROM invoice_issued WHERE id = ?1 AND origin = 'importada')"
            } else {
                "SELECT EXISTS(SELECT 1 FROM invoice_received WHERE id = ?1 AND posted = 0)"
            },
            [invoice_id],
            |r| r.get(0),
        )?;
        return Err(AppError::Other(if imported {
            "la factura viene del libro del gestor: su cobro o pago ya está en su contabilidad"
                .into()
        } else {
            "la factura no existe, está anulada o sin revisar".into()
        }));
    }
    Ok(items)
}

/// Guarda los vencimientos calculados como fijos (si aún no lo estaban).
fn freeze(conn: &Connection, items: &[DueItem]) -> Result<()> {
    if items.first().is_none_or(|i| i.fixed) {
        return Ok(());
    }
    for i in items {
        conn.execute(
            "INSERT INTO invoice_installment(kind, invoice_id, seq, due_date, amount) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![i.kind, i.invoice_id, i.seq, i.due_date.to_string(), i.amount.cents()],
        )?;
    }
    Ok(())
}

/// Fija los vencimientos calculados de una factura y los devuelve (lo usa también el banco al
/// casar un movimiento con un vencimiento concreto).
pub(crate) fn freeze_invoice(
    conn: &Connection,
    kind: &str,
    invoice_id: i64,
) -> Result<Vec<DueItem>> {
    let items = installments(conn, kind, invoice_id)?;
    freeze(conn, &items)?;
    Ok(items)
}

/// Cambia la fecha de un vencimiento pendiente.
pub fn set_due_date(v: &Vault, kind: &str, invoice_id: i64, seq: i64, due: Date) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let items = freeze_invoice(&tx, kind, invoice_id)?;
    let item = items
        .iter()
        .find(|i| i.seq == seq)
        .ok_or_else(|| AppError::Other("ese vencimiento no existe".into()))?;
    if item.status != "pendiente" {
        return Err(AppError::Other(
            "ese vencimiento ya está cobrado o pagado".into(),
        ));
    }
    tx.execute(
        "UPDATE invoice_installment SET due_date = ?1 WHERE kind = ?2 AND invoice_id = ?3 AND seq = ?4",
        params![due.to_string(), kind, invoice_id, seq],
    )?;
    tx.commit()?;
    Ok(())
}

/// Desglosa lo que queda pendiente de una factura en los vencimientos `pending` (fecha e
/// importe); los cobrados o pagados se quedan como están. Con `None` vuelve a calcularlos con
/// las condiciones del tercero (sólo si no hay nada cobrado o pagado).
pub fn set_schedule(
    v: &Vault,
    kind: &str,
    invoice_id: i64,
    pending: Option<&[(Date, Money)]>,
    now: OffsetDateTime,
) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let items = installments(&tx, kind, invoice_id)?;
    let done: Vec<&DueItem> = items.iter().filter(|i| i.status != "pendiente").collect();
    match pending {
        None => {
            if !done.is_empty() {
                return Err(AppError::Other(
                    "tiene cobros o pagos registrados: deshazlos antes de recalcular los vencimientos"
                        .into(),
                ));
            }
            tx.execute(
                "DELETE FROM invoice_installment WHERE kind = ?1 AND invoice_id = ?2",
                params![kind, invoice_id],
            )?;
        }
        Some(rows) => {
            if rows.is_empty() || rows.len() + done.len() > MAX_INSTALLMENTS {
                return Err(AppError::Other(format!(
                    "pon entre 1 y {MAX_INSTALLMENTS} vencimientos"
                )));
            }
            let total = items[0].invoice_amount.cents();
            let paid: i64 = done.iter().map(|i| i.amount.cents()).sum();
            let new: i64 = rows.iter().map(|r| r.1.cents()).sum();
            if paid + new != total {
                return Err(AppError::Other(format!(
                    "los vencimientos suman {} y lo pendiente es {}",
                    Money::from_cents(new),
                    Money::from_cents(total - paid)
                )));
            }
            if rows.iter().any(|r| r.1.is_zero()) {
                return Err(AppError::Other("hay un vencimiento sin importe".into()));
            }
            freeze(&tx, &items)?;
            tx.execute(
                "DELETE FROM invoice_installment WHERE kind = ?1 AND invoice_id = ?2
                   AND seq NOT IN (SELECT seq FROM settlement WHERE kind = ?1 AND invoice_id = ?2)
                   AND seq NOT IN (SELECT matched_seq FROM bank_tx
                                   WHERE matched_kind = ?1 AND matched_id = ?2 AND matched_seq IS NOT NULL)",
                params![kind, invoice_id],
            )?;
            let mut seq = done.iter().map(|i| i.seq).max().unwrap_or(0);
            let mut rows = rows.to_vec();
            rows.sort_by_key(|r| r.0);
            for (due, amount) in rows {
                seq += 1;
                tx.execute(
                    "INSERT INTO invoice_installment(kind, invoice_id, seq, due_date, amount) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![kind, invoice_id, seq, due.to_string(), amount.cents()],
                )?;
            }
        }
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "cartera.vencimientos",
        &invoice_id.to_string(),
        &serde_json::json!({ "kind": kind, "pending": pending.map(|p| p.iter().map(|(d, m)| (d.to_string(), m.cents())).collect::<Vec<_>>()) }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Registra el cobro (emitida) o el pago (recibida) del vencimiento `seq` de una factura en la
/// cuenta `account` (570 caja, 572x banco…) el día `date`, con su asiento. No vale si ya está
/// cobrado o pagado (aquí o conciliado con el banco).
pub fn settle(
    v: &Vault,
    kind: &str,
    invoice_id: i64,
    seq: i64,
    date: Date,
    account: &str,
    now: OffsetDateTime,
) -> Result<i64> {
    settle_amount(v, kind, invoice_id, seq, date, account, None, now)
}

/// [`settle`] por `amount_eur` euros, si no son los contabilizados: sólo en facturas en moneda
/// extranjera, y la diferencia va a 768 o 668 (diferencias de cambio).
#[allow(clippy::too_many_arguments)]
pub fn settle_amount(
    v: &Vault,
    kind: &str,
    invoice_id: i64,
    seq: i64,
    date: Date,
    account: &str,
    amount_eur: Option<Money>,
    now: OffsetDateTime,
) -> Result<i64> {
    settle_with(
        v, kind, invoice_id, seq, date, account, None, amount_eur, now,
    )
}

/// [`settle_amount`] con la forma de cobro o pago de ese vencimiento (`method`, una de
/// [`METHODS`]): «efectivo» es lo que cuenta el 347 como importe percibido en metálico (art.
/// 34.1.h RGAT). `None`: no se dice (como hasta ahora).
#[allow(clippy::too_many_arguments)]
pub fn settle_with(
    v: &Vault,
    kind: &str,
    invoice_id: i64,
    seq: i64,
    date: Date,
    account: &str,
    method: Option<&str>,
    amount_eur: Option<Money>,
    now: OffsetDateTime,
) -> Result<i64> {
    check_kind(kind)?;
    let treasury = treasury_code(account)?;
    if method.is_some_and(|m| !METHODS.contains(&m)) {
        return Err(AppError::Other("forma de cobro o pago no válida".into()));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let entry_id = settle_in_with(
        &tx, kind, invoice_id, seq, date, &treasury, None, amount_eur, method, now,
    )?;
    tx.commit()?;
    Ok(entry_id)
}

/// Cobro o pago de un vencimiento contra `account` (tesorería, o la 4312/555 de una remesa),
/// dentro de la transacción del llamador, sin forma de cobro. Devuelve el asiento.
#[allow(clippy::too_many_arguments)]
pub(crate) fn settle_in(
    tx: &Connection,
    kind: &str,
    invoice_id: i64,
    seq: i64,
    date: Date,
    account: &AccountCode,
    note: Option<&str>,
    amount_eur: Option<Money>,
    now: OffsetDateTime,
) -> Result<i64> {
    settle_in_with(
        tx, kind, invoice_id, seq, date, account, note, amount_eur, None, now,
    )
}

/// [`settle_in`] con la forma de cobro o pago (`method`, ya comprobada).
#[allow(clippy::too_many_arguments)]
fn settle_in_with(
    tx: &Connection,
    kind: &str,
    invoice_id: i64,
    seq: i64,
    date: Date,
    account: &AccountCode,
    note: Option<&str>,
    amount_eur: Option<Money>,
    method: Option<&str>,
    now: OffsetDateTime,
) -> Result<i64> {
    let items = installments(tx, kind, invoice_id)?;
    let item = items
        .iter()
        .find(|i| i.seq == seq)
        .ok_or_else(|| AppError::Other("ese vencimiento no existe".into()))?;
    match item.status.as_str() {
        "banco" => {
            return Err(AppError::Other(format!(
                "{} ya está conciliado con un movimiento del banco",
                item.number
            )));
        }
        "registrada" => {
            return Err(AppError::Other(format!(
                "{} ya tiene registrado su cobro o pago",
                item.number
            )));
        }
        _ => {}
    }
    let amount = item.amount;
    let actual = amount_eur.unwrap_or(amount);
    check_exchange(item, actual)?;
    freeze(tx, &items)?;
    let (party, mut concept) = crate::bank::invoice_party(tx, kind, invoice_id)?;
    if item.installments > 1 {
        concept = format!("{concept} ({}/{})", seq, item.installments);
    }
    if let Some(n) = note {
        concept = format!("{concept} · {n}");
    }
    tx.execute(
        "INSERT INTO settlement(kind, invoice_id, seq, date, account, created_at, method)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            kind,
            invoice_id,
            seq,
            date.to_string(),
            account.as_str(),
            now_str(now),
            method
        ],
    )?;
    let settlement = tx.last_insert_rowid();
    // Cobro: tesorería al debe y cliente al haber; pago, al revés (importes con signo para las
    // rectificativas). En moneda extranjera, la tesorería por lo cobrado o pagado en euros y la
    // diferencia con lo contabilizado, a 768/668.
    let collect = kind == "emitida";
    let entry = JournalEntry {
        date,
        concept,
        origin: Origin::Cartera(settlement),
        lines: [
            EntryLine::signed(account.clone(), actual, collect),
            EntryLine::signed(party, amount, !collect),
            exchange_difference(collect, amount, actual),
        ]
        .into_iter()
        .flatten()
        .collect(),
    };
    let entry_id = crate::accounting::record_entry(tx, &entry, now)?;
    tx.execute(
        "UPDATE settlement SET entry_id = ?1 WHERE id = ?2",
        params![entry_id, settlement],
    )?;
    audit::append(
        tx,
        &now_str(now),
        Actor::User,
        if collect {
            "cartera.cobro"
        } else {
            "cartera.pago"
        },
        &invoice_id.to_string(),
        &serde_json::json!({
            "kind": kind,
            "seq": seq,
            "account": account.as_str(),
            "entry": entry_id,
            "amount_eur": (actual != amount).then_some(actual.cents()),
            "method": method,
        }),
    )?;
    Ok(entry_id)
}

/// Deshace un cobro o pago registrado aquí (borra su asiento; el ejercicio debe estar abierto).
pub fn unsettle(
    v: &Vault,
    kind: &str,
    invoice_id: i64,
    seq: i64,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    check_kind(kind)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    if let Some(r) = remitted(&tx)?.get(&(kind.to_owned(), invoice_id, seq)) {
        return Err(AppError::Other(format!(
            "va en la remesa {r}: deshaz la remesa o marca el recibo como devuelto"
        )));
    }
    let (id, entry): (i64, Option<i64>) = tx
        .query_row(
            "SELECT id, entry_id FROM settlement WHERE kind = ?1 AND invoice_id = ?2 AND seq = ?3",
            params![kind, invoice_id, seq],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| AppError::Other("no hay un cobro o pago registrado aquí".into()))?;
    tx.execute("DELETE FROM settlement WHERE id = ?1", [id])?;
    if let Some(e) = entry {
        tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [e])?;
        tx.execute("DELETE FROM journal_entry WHERE id = ?1", [e])?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "cartera.deshacer",
        &invoice_id.to_string(),
        &serde_json::json!({ "kind": kind, "seq": seq, "entry": entry }),
    )?;
    tx.commit()?;
    Ok(())
}

// ---------- Cuadre con la contabilidad y periodo medio de pago ----------

/// Diferencia entre el saldo contable de un cliente o proveedor y lo pendiente en la cartera.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Mismatch {
    pub account: String,
    pub name: String,
    /// Saldo en contabilidad (clientes deudor positivo; proveedores acreedor positivo).
    pub ledger: Money,
    /// Pendiente de cobro o pago en la cartera.
    pub pending: Money,
}

/// Clientes y proveedores cuyo saldo contable (todo el diario hasta `today`, sin cierres ni
/// aperturas, salvo la del primer ejercicio y la de los saldos iniciales, que traen lo de antes
/// de TaxPain) no coincide con lo pendiente en la cartera: cobros o pagos contabilizados a mano,
/// saldos iniciales o importados, anticipos…
pub fn ledger_mismatches(v: &Vault, today: Date) -> Result<Vec<Mismatch>> {
    let conn = v.connection();
    let mut pending: HashMap<String, Money> = HashMap::new();
    for d in list(v, today)? {
        if d.status != "pendiente" {
            continue;
        }
        if let Some(a) = d.party_account {
            *pending.entry(a).or_default() += d.amount;
        }
    }
    let mut out = Vec::new();
    for p in parties(v)? {
        let net: i64 = conn.query_row(
            "SELECT COALESCE(SUM(l.debit - l.credit), 0) FROM journal_line l
             JOIN journal_entry e ON e.id = l.entry_id
             WHERE l.account = ?1 AND e.date <= ?2 AND e.origin_kind <> 'Cierre'
               AND (e.origin_kind <> 'Apertura'
                    OR e.fiscal_year = (SELECT MIN(fiscal_year) FROM journal_entry)
                    OR e.id IN (SELECT entry_id FROM opening_balance WHERE entry_id IS NOT NULL))",
            params![p.account, today.to_string()],
            |r| r.get(0),
        )?;
        let ledger = if p.role == "cliente" {
            Money::from_cents(net)
        } else {
            Money::from_cents(-net)
        };
        let pend = pending.get(&p.account).copied().unwrap_or(Money::ZERO);
        if ledger != pend {
            out.push(Mismatch {
                account: p.account,
                name: p.name,
                ledger,
                pending: pend,
            });
        }
    }
    Ok(out)
}

/// Periodo medio de pago a proveedores del ejercicio (Resolución del ICAC de 29 de enero de
/// 2016, para la memoria): días en centésimas.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PaymentPeriod {
    /// Periodo medio de pago (días × 100).
    pub average: i64,
    /// Ratio de operaciones pagadas y de pendientes de pago (días × 100).
    pub paid_ratio: i64,
    pub pending_ratio: i64,
    pub paid_total: Money,
    pub pending_total: Money,
}

/// Días desde la factura (inicio del cómputo; el ICAC admite la fecha de recepción de la factura
/// si no hay otra fiable) hasta el pago, o hasta el cierre si sigue pendiente.
pub fn payment_period(v: &Vault, start: Date, end: Date) -> Result<PaymentPeriod> {
    let mut paid = (0_i128, 0_i128);
    let mut pend = (0_i128, 0_i128);
    for d in list(v, end)? {
        if d.kind != "recibida" || d.issue_date > end {
            continue;
        }
        let amount = i128::from(d.amount.cents());
        match d.settled_on {
            Some(s) if s >= start && s <= end => {
                let days = i128::from((s - d.issue_date).whole_days().max(0));
                paid.0 += days * amount;
                paid.1 += amount;
            }
            Some(_) => {}
            None => {
                let days = i128::from((end - d.issue_date).whole_days().max(0));
                pend.0 += days * amount;
                pend.1 += amount;
            }
        }
    }
    let ratio = |(num, den): (i128, i128)| {
        if den == 0 {
            0
        } else {
            i64::try_from((num * 100 + den / 2) / den).unwrap_or(0)
        }
    };
    let total = paid.1 + pend.1;
    let average = if total == 0 {
        0
    } else {
        i64::try_from((paid.0 * 100 + pend.0 * 100 + total / 2) / total).unwrap_or(0)
    };
    let money = |c: i128| Money::from_cents(i64::try_from(c).unwrap_or(0));
    Ok(PaymentPeriod {
        average,
        paid_ratio: ratio(paid),
        pending_ratio: ratio(pend),
        paid_total: money(paid.1),
        pending_total: money(pend.1),
    })
}
