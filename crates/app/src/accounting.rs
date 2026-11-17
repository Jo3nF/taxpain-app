//! Contabilidad: registro de asientos, contabilización automática de facturas, libros y
//! cierre del ejercicio. Los ejercicios pueden no ser naturales: la fecha de cierre sale del
//! perfil fiscal (sociedades) y cada ejercicio guarda sus fechas (ver
//! `taxpain_core::accounting::fiscal_year`).

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::closing::{closing, opening, regularization};
use taxpain_core::accounting::ledger::{self, AccountBalance};
use taxpain_core::accounting::{
    AccountCode, EntryLine, FiscalYear, JournalEntry, Origin, PostingAccounts, ReceivedInvoice,
    post_issued_with_advance, post_received, trial_balance as tb,
};
use taxpain_core::invoice::{Invoice, PartyId, Totals};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::format_description::well_known::Rfc3339;
use time::{Date, Month, OffsetDateTime};

use crate::error::{AppError, Result};

pub const SETTINGS_KEY: &str = "accounting";

fn now_str(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

/// Cuentas de contabilización configuradas (o las de por defecto).
pub fn posting_accounts(conn: &Connection) -> Result<PostingAccounts> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [SETTINGS_KEY],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match raw {
        Some(s) => serde_json::from_str(&s).map_err(|e| AppError::Other(e.to_string()))?,
        None => PostingAccounts::default(),
    })
}

/// Guarda las cuentas por defecto de la contabilización automática. Comprueba que cada una es
/// del grupo que toca y que los dígitos de las subcuentas de terceros no cambian si ya hay
/// clientes o proveedores con subcuenta (mezclaría longitudes en el plan).
pub fn save_posting_accounts(v: &Vault, a: &PostingAccounts) -> Result<()> {
    let checks: [(&AccountCode, &[&str], &str); 9] = [
        (&a.revenue, &["7"], "ventas o ingresos (grupo 7)"),
        (&a.expense, &["6"], "compras o gastos (grupo 6)"),
        (&a.customers, &["43", "44"], "clientes (43x o 44x)"),
        (
            &a.suppliers,
            &["40", "41"],
            "proveedores o acreedores (40x o 41x)",
        ),
        (&a.vat_output, &["477"], "IVA repercutido (477)"),
        (&a.vat_input, &["472"], "IVA soportado (472)"),
        (
            &a.withholding_receivable,
            &["473"],
            "retenciones a cuenta (473)",
        ),
        (
            &a.withholding_payable,
            &["4751"],
            "retenciones practicadas (4751)",
        ),
        (&a.bank, &["57"], "tesorería (57x)"),
    ];
    for (code, prefixes, what) in checks {
        if !prefixes.iter().any(|p| code.as_str().starts_with(p)) {
            return Err(AppError::Other(format!(
                "la cuenta {} no es de {what}",
                code.as_str()
            )));
        }
    }
    if !(6..=12).contains(&a.subaccount_width) {
        return Err(AppError::Other(
            "las subcuentas de terceros tienen entre 6 y 12 dígitos".into(),
        ));
    }
    let conn = v.connection();
    let current = posting_accounts(conn)?;
    if current.subaccount_width != a.subaccount_width {
        let used: i64 =
            conn.query_row("SELECT COUNT(*) FROM third_party_account", [], |r| r.get(0))?;
        if used > 0 {
            return Err(AppError::Other(format!(
                "ya hay {used} subcuentas de clientes y proveedores con {} dígitos: no se puede \
                 cambiar la longitud",
                current.subaccount_width
            )));
        }
    }
    Ok(v.set_setting(SETTINGS_KEY, a)?)
}

/// Cuenta de ventas o gastos propia del tercero con subcuenta `account`, si tiene.
pub fn counterpart_of(conn: &Connection, account: &str) -> Result<Option<AccountCode>> {
    conn.query_row(
        "SELECT counterpart FROM party_counterpart WHERE account = ?1",
        [account],
        |r| r.get::<_, String>(0),
    )
    .optional()?
    .map(|c| AccountCode::new(&c).map_err(|e| AppError::Other(e.to_string())))
    .transpose()
}

/// Fija (o quita, con `None`) la cuenta de ventas de un cliente (grupo 7) o de gastos de un
/// proveedor (grupo 6 o inmovilizado, grupo 2).
pub fn set_party_counterpart(
    v: &Vault,
    account: &str,
    counterpart: Option<&str>,
    now: OffsetDateTime,
) -> Result<()> {
    let conn = v.connection();
    let role: Option<String> = conn
        .query_row(
            "SELECT role FROM third_party_account WHERE account = ?1",
            [account],
            |r| r.get(0),
        )
        .optional()?;
    let role = role
        .ok_or_else(|| AppError::Other("esa subcuenta no es de un cliente o proveedor".into()))?;
    let tx = conn.unchecked_transaction()?;
    match counterpart.map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) => {
            let code = AccountCode::new(c).map_err(|e| AppError::Other(e.to_string()))?;
            let ok = if role == "cliente" {
                code.as_str().starts_with('7')
            } else {
                code.as_str().starts_with('6') || code.as_str().starts_with('2')
            };
            if !ok {
                return Err(AppError::Other(if role == "cliente" {
                    "la cuenta de ventas de un cliente es del grupo 7 (700, 705…)".into()
                } else {
                    "la cuenta de un proveedor es de gastos (grupo 6) o de inmovilizado (grupo 2)"
                        .into()
                }));
            }
            tx.execute(
                "INSERT INTO party_counterpart(account, counterpart, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(account) DO UPDATE SET counterpart = excluded.counterpart,
                   updated_at = excluded.updated_at",
                params![account, code.as_str(), now_str(now)],
            )?;
        }
        None => {
            tx.execute(
                "DELETE FROM party_counterpart WHERE account = ?1",
                [account],
            )?;
        }
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "tercero.cuenta",
        account,
        &serde_json::json!({ "counterpart": counterpart }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Subcuenta del tercero (la crea si no existe): 4300001, 4300002…
pub fn third_party_account(
    conn: &Connection,
    tax_id: &str,
    role: &str,
    name: &str,
    base: &AccountCode,
    width: usize,
) -> Result<AccountCode> {
    if let Some(a) = conn
        .query_row(
            "SELECT account FROM third_party_account WHERE tax_id = ?1 AND role = ?2",
            params![tax_id, role],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        return AccountCode::new(&a).map_err(|e| AppError::Other(e.to_string()));
    }
    let used: i64 = conn.query_row(
        "SELECT COUNT(*) FROM third_party_account WHERE account LIKE ?1 || '%'",
        [base.as_str()],
        |r| r.get(0),
    )?;
    let mut n = u32::try_from(used).unwrap_or(0) + 1;
    loop {
        let candidate = base
            .subaccount(width, n)
            .map_err(|e| AppError::Other(e.to_string()))?;
        let taken: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM third_party_account WHERE account = ?1",
                [candidate.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        if taken.is_none() {
            conn.execute(
                "INSERT INTO third_party_account(tax_id, role, account, name) VALUES (?1, ?2, ?3, ?4)",
                params![tax_id, role, candidate.as_str(), name],
            )?;
            return Ok(candidate);
        }
        n += 1;
    }
}

fn origin_parts(o: &Origin) -> (&'static str, Option<i64>) {
    match o {
        Origin::FacturaEmitida(id) => ("FacturaEmitida", Some(*id)),
        Origin::FacturaRecibida(id) => ("FacturaRecibida", Some(*id)),
        Origin::Banco(id) => ("Banco", Some(*id)),
        Origin::Amortizacion(id) => ("Amortizacion", Some(*id)),
        Origin::LiquidacionIva(k) => ("LiquidacionIva", Some(*k)),
        Origin::Cartera(id) => ("Cartera", Some(*id)),
        Origin::Importado(id) => ("Importado", Some(*id)),
        Origin::Gestor(id) => ("Gestor", Some(*id)),
        Origin::PagoModelo(id) => ("PagoModelo", Some(*id)),
        Origin::BajaInmovilizado(id) => ("BajaInmovilizado", Some(*id)),
        Origin::Leasing(id) => ("Leasing", Some(*id)),
        Origin::Prestamo(id) => ("Prestamo", Some(*id)),
        Origin::Nomina => ("Nomina", None),
        Origin::Manual => ("Manual", None),
        Origin::Apertura => ("Apertura", None),
        Origin::Regularizacion => ("Regularizacion", None),
        Origin::Cierre => ("Cierre", None),
        Origin::ImpuestoSociedades(y) => ("ImpuestoSociedades", Some(*y)),
        Origin::AplicacionResultado(y) => ("AplicacionResultado", Some(*y)),
        Origin::Dividendo(id) => ("Dividendo", Some(*id)),
        Origin::GastosViaje(id) => ("GastosViaje", Some(*id)),
        Origin::AnulacionFactura(id) => ("AnulacionFactura", Some(*id)),
        Origin::DevolucionRemesa(id) => ("DevolucionRemesa", Some(*id)),
        Origin::Existencias(y) => ("Existencias", Some(*y)),
        Origin::Cripto(i) => ("Cripto", Some(*i)),
        Origin::AjusteCierre(i) => ("AjusteCierre", Some(*i)),
        Origin::DiferenciasCambio(y) => ("DiferenciasCambio", Some(*y)),
        Origin::Valores(i) => ("Valores", Some(*i)),
        Origin::AcuerdoDividendo(i) => ("AcuerdoDividendo", Some(*i)),
        Origin::Subvencion(i) => ("Subvencion", Some(*i)),
    }
}

fn origin_from(kind: &str, id: Option<i64>) -> Origin {
    match (kind, id) {
        ("FacturaEmitida", Some(i)) => Origin::FacturaEmitida(i),
        ("FacturaRecibida", Some(i)) => Origin::FacturaRecibida(i),
        ("Banco", Some(i)) => Origin::Banco(i),
        ("Amortizacion", Some(i)) => Origin::Amortizacion(i),
        ("LiquidacionIva", Some(i)) => Origin::LiquidacionIva(i),
        ("Cartera", Some(i)) => Origin::Cartera(i),
        ("Importado", Some(i)) => Origin::Importado(i),
        ("Gestor", Some(i)) => Origin::Gestor(i),
        ("PagoModelo", Some(i)) => Origin::PagoModelo(i),
        ("BajaInmovilizado", Some(i)) => Origin::BajaInmovilizado(i),
        ("Leasing", Some(i)) => Origin::Leasing(i),
        ("Prestamo", Some(i)) => Origin::Prestamo(i),
        ("Nomina", _) => Origin::Nomina,
        ("Apertura", _) => Origin::Apertura,
        ("Regularizacion", _) => Origin::Regularizacion,
        ("Cierre", _) => Origin::Cierre,
        ("ImpuestoSociedades", Some(y)) => Origin::ImpuestoSociedades(y),
        ("AplicacionResultado", Some(y)) => Origin::AplicacionResultado(y),
        ("Dividendo", Some(i)) => Origin::Dividendo(i),
        ("GastosViaje", Some(i)) => Origin::GastosViaje(i),
        ("AnulacionFactura", Some(i)) => Origin::AnulacionFactura(i),
        ("DevolucionRemesa", Some(i)) => Origin::DevolucionRemesa(i),
        ("Existencias", Some(y)) => Origin::Existencias(y),
        ("Cripto", Some(i)) => Origin::Cripto(i),
        ("AjusteCierre", Some(i)) => Origin::AjusteCierre(i),
        ("DiferenciasCambio", Some(y)) => Origin::DiferenciasCambio(y),
        ("Valores", Some(i)) => Origin::Valores(i),
        ("AcuerdoDividendo", Some(i)) => Origin::AcuerdoDividendo(i),
        ("Subvencion", Some(i)) => Origin::Subvencion(i),
        _ => Origin::Manual,
    }
}

// ---------------- Ejercicios ----------------

/// Fecha de cierre del ejercicio (mes, día) según el perfil fiscal. Las personas físicas
/// siempre tienen ejercicio natural.
pub fn year_close(conn: &Connection) -> Result<(Month, u8)> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'taxpayer_profile'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    let natural = (Month::December, 31);
    let Some(raw) = raw else { return Ok(natural) };
    let Ok(p) = serde_json::from_str::<taxpain_core::calendar::TaxpayerProfile>(&raw) else {
        return Ok(natural);
    };
    if p.persona != taxpain_core::calendar::Persona::Juridica
        || !taxpain_core::accounting::fiscal_year::valid_close(p.cierre_ejercicio)
    {
        return Ok(natural);
    }
    Ok(p.cierre_ejercicio)
}

fn stored_years(conn: &Connection) -> Result<Vec<(FiscalYear, bool)>> {
    let mut st = conn.prepare(
        "SELECT year, start_date, end_date, closed FROM fiscal_year
         WHERE start_date IS NOT NULL AND end_date IS NOT NULL ORDER BY start_date",
    )?;
    let rows = st
        .query_map([], |r| {
            Ok((
                r.get::<_, i32>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(year, a, b, closed)| {
            Ok((
                FiscalYear {
                    year,
                    start: parse_date(&a)?,
                    end: parse_date(&b)?,
                },
                closed == 1,
            ))
        })
        .collect()
}

/// Ejercicio que contiene `date`; si aún no existe, lo crea con la fecha de cierre del perfil.
/// Si la fecha de cierre cambió, el nuevo ejercicio empieza justo después del anterior y acaba
/// en la nueva fecha de cierre (ejercicio de transición), sin huecos ni solapes.
pub fn fiscal_year_for_date(conn: &Connection, date: Date) -> Result<FiscalYear> {
    let years = stored_years(conn)?;
    if let Some((f, _)) = years.iter().find(|(f, _)| f.contains(date)) {
        return Ok(*f);
    }
    let mut f = FiscalYear::containing(date, year_close(conn)?);
    if let Some((prev, _)) = years.iter().rev().find(|(y, _)| y.end < date) {
        let after = prev.end + time::Duration::days(1);
        if after > f.start || (f.start - after).whole_days() < 366 {
            f.start = after;
        }
    }
    if let Some((next, _)) = years.iter().find(|(y, _)| y.start > date) {
        let before = next.start - time::Duration::days(1);
        if before < f.end || (before - f.end).whole_days() < 366 {
            f.end = before;
        }
    }
    let taken = |y: i32| years.iter().any(|(f, _)| f.year == y);
    let year = (f.start.year()..=f.end.year() + 1)
        .find(|y| !taken(*y))
        .ok_or_else(|| AppError::Other("no queda número libre para el ejercicio".into()))?;
    f.year = year;
    conn.execute(
        "INSERT INTO fiscal_year(year, start_date, end_date) VALUES (?1, ?2, ?3)",
        params![f.year, f.start.to_string(), f.end.to_string()],
    )?;
    Ok(f)
}

/// Ejercicio número `year`: el guardado o, si aún no hay asientos, el que corresponde a la
/// fecha de cierre del perfil.
pub fn fiscal_year_conn(conn: &Connection, year: i32) -> Result<FiscalYear> {
    let years = stored_years(conn)?;
    if let Some((f, _)) = years.iter().find(|(f, _)| f.year == year) {
        return Ok(*f);
    }
    Ok(FiscalYear::numbered(year, year_close(conn)?))
}

/// Ejercicio que contiene `date`, sin crearlo: el guardado o el que toca por el perfil.
pub fn fiscal_year_at(v: &Vault, date: Date) -> Result<FiscalYear> {
    let conn = v.connection();
    Ok(
        match stored_years(conn)?
            .into_iter()
            .find(|(f, _)| f.contains(date))
        {
            Some((f, _)) => f,
            None => FiscalYear::containing(date, year_close(conn)?),
        },
    )
}

pub fn fiscal_year(v: &Vault, year: i32) -> Result<FiscalYear> {
    fiscal_year_conn(v.connection(), year)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FiscalYearInfo {
    pub year: i32,
    pub start: Date,
    pub end: Date,
    pub label: String,
    pub natural: bool,
    pub closed: bool,
}

/// Ejercicio número `year` (o el que contiene `today` si es `None`) para la interfaz.
pub fn fiscal_year_info(v: &Vault, year: Option<i32>, today: Date) -> Result<FiscalYearInfo> {
    let conn = v.connection();
    let f = match year {
        Some(y) => fiscal_year_conn(conn, y)?,
        None => fiscal_year_at(v, today)?,
    };
    let closed = stored_years(conn)?
        .iter()
        .any(|(y, c)| y.year == f.year && *c);
    Ok(FiscalYearInfo {
        year: f.year,
        start: f.start,
        end: f.end,
        label: f.label(),
        natural: f.is_natural(),
        closed,
    })
}

/// Registra un asiento validado con el siguiente número del ejercicio. Usa la conexión del
/// llamador (puede estar dentro de su transacción).
pub fn record_entry(conn: &Connection, e: &JournalEntry, now: OffsetDateTime) -> Result<i64> {
    e.validate()
        .map_err(|err| AppError::Other(err.to_string()))?;
    let year = fiscal_year_for_date(conn, e.date)?.year;
    let number: i64 = conn.query_row(
        "SELECT COALESCE(MAX(number), 0) + 1 FROM journal_entry WHERE fiscal_year = ?1",
        [year],
        |r| r.get(0),
    )?;
    let (kind, id) = origin_parts(&e.origin);
    conn.execute(
        "INSERT INTO journal_entry(fiscal_year, number, date, concept, origin_kind, origin_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![year, number, e.date.to_string(), e.concept, kind, id, now_str(now)],
    )?;
    let entry_id = conn.last_insert_rowid();
    for l in &e.lines {
        conn.execute(
            "INSERT INTO journal_line(entry_id, account, debit, credit, description) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![entry_id, l.account.as_str(), l.debit.cents(), l.credit.cents(), l.description],
        )?;
    }
    Ok(entry_id)
}

fn party_key(p: Option<&taxpain_core::invoice::Party>) -> (String, String) {
    match p {
        Some(p) => (
            match &p.id {
                PartyId::Nif(n) => n.clone(),
                PartyId::Foreign { country, id, .. } => format!("{country}{id}"),
            },
            p.name.clone(),
        ),
        None => ("VARIOS".into(), "Clientes varios".into()),
    }
}

/// Contabiliza una factura emitida (dentro de la transacción de emisión).
/// `advance`: euros de la 438 si no son la base de las líneas de anticipo (anticipo en moneda
/// extranjera descontado en la final; ver `post_issued_with_advance`).
pub fn post_issued_invoice(
    conn: &Connection,
    invoice_id: i64,
    invoice: &Invoice,
    totals: &Totals,
    advance: Option<Money>,
    now: OffsetDateTime,
) -> Result<i64> {
    let mut a = posting_accounts(conn)?;
    let (tax_id, name) = party_key(invoice.recipient.as_ref());
    let customer = third_party_account(
        conn,
        &tax_id,
        "cliente",
        &name,
        &a.customers,
        a.subaccount_width,
    )?;
    if let Some(c) = counterpart_of(conn, customer.as_str())? {
        a.revenue = c;
    }
    use taxpain_core::invoice::{InvoiceKind, RectificationMethod};
    let entry = match invoice.kind {
        // Concurso (R2) o crédito incobrable (R3): la rectificativa sólo modifica la base del
        // IVA; la venta no se anula y el cliente sigue debiendo la base (que se deteriora, 694/490,
        // o se da por perdida, 650). Sólo sale la cuota: 477 al debe y cliente al haber.
        // `TODO(VERIFY)`: el ICAC no tiene consulta sobre la cuenta; es el tratamiento usual de
        // los manuales (477 a 430/436).
        InvoiceKind::Rectificativa(r)
            if matches!(r.code, 2 | 3) && r.method == RectificationMethod::Diferencias =>
        {
            let tax = totals.vat + totals.surcharge;
            JournalEntry {
                date: invoice.issue_date,
                concept: format!(
                    "Modificación de la base del IVA {} ({})",
                    invoice.number,
                    if r.code == 2 {
                        "concurso del cliente, art. 80.Tres LIVA"
                    } else {
                        "crédito incobrable, art. 80.Cuatro LIVA"
                    }
                ),
                origin: Origin::FacturaEmitida(invoice_id),
                lines: [
                    EntryLine::signed(customer.clone(), tax, true),
                    EntryLine::signed(a.vat_output.clone(), tax, false),
                ]
                .into_iter()
                .flatten()
                .collect(),
            }
        }
        // Por sustitución: sólo la diferencia con las facturas que sustituye (su asiento ya está).
        InvoiceKind::Rectificativa(r) if r.method == RectificationMethod::Sustitucion => {
            let (net, old_advance) =
                crate::rectificativas_emitidas::net_totals(conn, invoice, totals)?;
            let new_advance: Money = advance.unwrap_or_else(|| {
                invoice
                    .lines
                    .iter()
                    .filter(|l| l.advance)
                    .map(taxpain_core::invoice::totals::line_base)
                    .sum()
            });
            let mut e = post_issued_with_advance(
                invoice,
                &net,
                &a,
                &customer,
                invoice_id,
                Some(new_advance - old_advance),
            );
            let numbers: Vec<&str> = invoice.rectified.iter().map(|(n, _)| n.as_str()).collect();
            e.concept = format!("{} (sustituye a {})", e.concept, numbers.join(", "));
            e
        }
        _ => post_issued_with_advance(invoice, totals, &a, &customer, invoice_id, advance),
    };
    // Una sustitución que sólo corrige datos (mismos importes) no cambia nada en la contabilidad.
    if entry.lines.is_empty() {
        return Ok(0);
    }
    record_entry(conn, &entry, now)
}

/// Contraasiento de una factura anulada: el asiento original con los lados invertidos.
pub fn reverse_issued_invoice(
    conn: &Connection,
    invoice_id: i64,
    date: Date,
    now: OffsetDateTime,
) -> Result<Option<i64>> {
    let Some(entry_id): Option<i64> = conn
        .query_row(
            "SELECT id FROM journal_entry WHERE origin_kind = 'FacturaEmitida' AND origin_id = ?1",
            [invoice_id],
            |r| r.get(0),
        )
        .optional()?
    else {
        return Ok(None);
    };
    let original = load_entry(conn, entry_id)?;
    let reversed = JournalEntry {
        date,
        concept: format!("Anulación: {}", original.concept),
        // Origen propio: se enlaza con la factura y no se puede retocar ni borrar desde el diario.
        origin: Origin::AnulacionFactura(invoice_id),
        lines: original
            .lines
            .iter()
            .map(|l| EntryLine {
                account: l.account.clone(),
                debit: l.credit,
                credit: l.debit,
                description: None,
            })
            .collect(),
    };
    Ok(Some(record_entry(conn, &reversed, now)?))
}

/// Por qué un contraasiento no puede ir en `date` (`None` si puede): su ejercicio está cerrado
/// o la liquidación del IVA de su trimestre ya está contabilizada (cambiaría la 477 de un
/// periodo ya liquidado). Lo usa la anulación de facturas.
pub(crate) fn reversal_blocked(conn: &Connection, date: Date) -> Result<Option<String>> {
    if let Some((f, _)) = stored_years(conn)?
        .into_iter()
        .find(|(f, closed)| *closed && f.contains(date))
    {
        return Ok(Some(format!("el ejercicio {} está cerrado", f.year)));
    }
    let quarter = (u8::from(date.month()) - 1) / 3 + 1;
    let key = i64::from(date.year()) * 100 + i64::from(quarter);
    let settled: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM journal_entry
                        WHERE origin_kind = 'LiquidacionIva' AND origin_id = ?1)",
        [key],
        |r| r.get(0),
    )?;
    Ok(settled.then(|| {
        format!(
            "la liquidación del IVA del {quarter}T de {} ya está contabilizada",
            date.year()
        )
    }))
}

/// Contabiliza una factura recibida revisada (dentro de la transacción de revisión).
pub fn post_received_invoice(
    conn: &Connection,
    r: &ReceivedInvoice,
    supplier_nif: &str,
    now: OffsetDateTime,
) -> Result<i64> {
    post_received_invoice_split(conn, r, supplier_nif, None, now)
}

/// [`post_received_invoice`] con la base repartida en las cuentas de `split` (que suman la base)
/// en lugar de la de gasto: la cuota de un leasing (capital a la 524, intereses a la 662). El IVA
/// no deducible va con la última: el ICAC (BOICAC 143, octubre de 2025, consulta 4, que mantiene
/// la Resolución de 1992) lo trata como **gasto del ejercicio en que se devenga**, no como mayor
/// valor del activo (NRV 12.ª PGC: sólo el IVA no deducible devengado al inicio forma parte del
/// precio de adquisición). TODO(VERIFY): la consulta no fija la cuenta (aquí la 662 de la
/// cuota; cabría la 631); sólo se ha leído en resúmenes (CEF, primeralectura), no en el BOICAC.
pub(crate) fn post_received_invoice_split(
    conn: &Connection,
    r: &ReceivedInvoice,
    supplier_nif: &str,
    split: Option<&[(AccountCode, Money)]>,
    now: OffsetDateTime,
) -> Result<i64> {
    let e = received_invoice_entry(conn, r, supplier_nif, split)?;
    record_entry(conn, &e, now)
}

/// El asiento de una factura recibida, sin grabarlo (ver [`post_received_invoice_split`]). Crea
/// la subcuenta del proveedor si aún no la tiene.
pub(crate) fn received_invoice_entry(
    conn: &Connection,
    r: &ReceivedInvoice,
    supplier_nif: &str,
    split: Option<&[(AccountCode, Money)]>,
) -> Result<JournalEntry> {
    let a = posting_accounts(conn)?;
    let supplier = third_party_account(
        conn,
        supplier_nif,
        "proveedor",
        &r.supplier_name,
        &a.suppliers,
        a.subaccount_width,
    )?;
    let mut a = a;
    if let Some(c) = counterpart_of(conn, supplier.as_str())? {
        a.expense = c;
    }
    let Some(((last, _), rest)) = split.and_then(<[_]>::split_last) else {
        return Ok(post_received(r, &a, &supplier));
    };
    if split.into_iter().flatten().map(|(_, m)| *m).sum::<Money>() != r.base {
        return Err(AppError::Other(
            "el reparto de la factura no suma su base".into(),
        ));
    }
    let r = ReceivedInvoice {
        expense_account: Some(last.clone()),
        ..r.clone()
    };
    let mut e = post_received(&r, &a, &supplier);
    let others: Money = rest.iter().map(|(_, m)| *m).sum();
    if let Some(l) = e
        .lines
        .iter_mut()
        .find(|l| &l.account == last && l.credit.is_zero())
    {
        l.debit -= others;
    }
    e.lines
        .retain(|l| !(l.debit.is_zero() && l.credit.is_zero()));
    e.lines.splice(
        0..0,
        rest.iter()
            .flat_map(|(acc, m)| EntryLine::signed(acc.clone(), *m, true)),
    );
    Ok(e)
}

/// Lo que cambió al rehacer un asiento automático en su sitio ([`rewrite_entry`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Rewritten {
    /// El asiento como estaba.
    pub before: Option<JournalEntry>,
    /// Ejercicio del que sale, si la nueva fecha lo cambia de ejercicio.
    pub moved_from_year: Option<i32>,
    /// Líneas punteadas cuyo importe cambió: se les quita el punteo (era del importe de antes).
    pub unticked: usize,
}

/// Rehace en su sitio el asiento automático `id` con la fecha, el concepto y las líneas de `e`
/// (dentro de la transacción del llamador), como se cambia un asiento manual: el asiento
/// conserva su id y, con él, sus adjuntos, sus observaciones y lo que lo enlaza (la cuota de un
/// leasing, por ejemplo). Las líneas que siguen (misma cuenta y mismo lado) se actualizan, no se
/// borran: conservan el reparto analítico y, si el importe no cambia, el punteo del mayor. Las que
/// sobran se borran y las nuevas se añaden. Si la fecha es de otro ejercicio, el asiento pasa a
/// él con el siguiente número. Los dos ejercicios deben estar abiertos (lo impiden también los
/// disparadores de la base de datos).
pub(crate) fn rewrite_entry(conn: &Connection, id: i64, e: &JournalEntry) -> Result<Rewritten> {
    e.validate()
        .map_err(|err| AppError::Other(err.to_string()))?;
    let before = load_entry(conn, id)?;
    let old_year: i32 = conn.query_row(
        "SELECT fiscal_year FROM journal_entry WHERE id = ?1",
        [id],
        |r| r.get(0),
    )?;
    let new_year = fiscal_year_for_date(conn, e.date)?.year;
    for y in [old_year, new_year] {
        if year_closed(conn, y)? {
            return Err(AppError::Other(format!(
                "el ejercicio {y} está cerrado: hay que reabrirlo desde Cierre"
            )));
        }
    }
    if new_year == old_year {
        conn.execute(
            "UPDATE journal_entry SET date = ?1, concept = ?2 WHERE id = ?3",
            params![e.date.to_string(), e.concept, id],
        )?;
    } else {
        conn.execute(
            "UPDATE journal_entry SET date = ?1, concept = ?2, fiscal_year = ?3,
               number = (SELECT COALESCE(MAX(number), 0) + 1 FROM journal_entry WHERE fiscal_year = ?3)
             WHERE id = ?4",
            params![e.date.to_string(), e.concept, new_year, id],
        )?;
    }
    let old: Vec<(i64, String, i64, i64)> = {
        let mut st = conn.prepare(
            "SELECT id, account, debit, credit FROM journal_line WHERE entry_id = ?1 ORDER BY id",
        )?;
        let rows = st.query_map([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let mut used = vec![false; old.len()];
    let mut unticked = 0;
    for l in &e.lines {
        let (debit, credit) = (l.debit.cents(), l.credit.cents());
        let same_side = |d: i64| (d != 0) == (debit != 0);
        // Primero la misma cuenta, lado e importe; si no, la misma cuenta y lado.
        let found = old
            .iter()
            .enumerate()
            .filter(|(i, (_, acc, d, _))| !used[*i] && acc == l.account.as_str() && same_side(*d))
            .min_by_key(|(_, (_, _, d, c))| *d != debit || *c != credit)
            .map(|(i, _)| i);
        match found {
            Some(i) => {
                used[i] = true;
                let (line_id, _, d, c) = &old[i];
                if *d != debit || *c != credit {
                    unticked +=
                        conn.execute("DELETE FROM line_tick WHERE line_id = ?1", [line_id])?;
                }
                conn.execute(
                    "UPDATE journal_line SET debit = ?1, credit = ?2, description = ?3 WHERE id = ?4",
                    params![debit, credit, l.description, line_id],
                )?;
            }
            None => {
                conn.execute(
                    "INSERT INTO journal_line(entry_id, account, debit, credit, description) VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id, l.account.as_str(), debit, credit, l.description],
                )?;
            }
        }
    }
    for (i, (line_id, ..)) in old.iter().enumerate() {
        if !used[i] {
            conn.execute("DELETE FROM journal_line WHERE id = ?1", [line_id])?;
        }
    }
    Ok(Rewritten {
        before: Some(before),
        moved_from_year: (new_year != old_year).then_some(old_year),
        unticked,
    })
}

fn load_lines(conn: &Connection, entry_id: i64) -> Result<Vec<EntryLine>> {
    let mut stmt = conn.prepare("SELECT account, debit, credit, description FROM journal_line WHERE entry_id = ?1 ORDER BY id")?;
    let rows = stmt.query_map([entry_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, Option<String>>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (a, d, c, desc) = row?;
        out.push(EntryLine {
            account: AccountCode::new(&a).map_err(|e| AppError::Other(e.to_string()))?,
            debit: Money::from_cents(d),
            credit: Money::from_cents(c),
            description: desc,
        });
    }
    Ok(out)
}

fn parse_date(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]"))
        .map_err(|e| AppError::Other(e.to_string()))
}

pub(crate) fn load_entry(conn: &Connection, id: i64) -> Result<JournalEntry> {
    let (date, concept, kind, oid): (String, String, String, Option<i64>) = conn.query_row(
        "SELECT date, concept, origin_kind, origin_id FROM journal_entry WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    Ok(JournalEntry {
        date: parse_date(&date)?,
        concept,
        origin: origin_from(&kind, oid),
        lines: load_lines(conn, id)?,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StoredEntry {
    pub id: i64,
    pub number: i64,
    pub entry: JournalEntry,
    /// Observaciones del asiento.
    pub notes: Option<String>,
    /// Documentos adjuntos.
    pub attachments: i64,
}

/// Libro diario del ejercicio en orden cronológico (art. 28 del Código de Comercio): por fecha,
/// con la apertura al principio y la regularización y el cierre al final de su día. `number` es
/// el número correlativo en ese orden (el de registro interno puede no serlo si se contabiliza
/// algo con fecha anterior, p. ej. una recibida revisada tarde).
pub fn journal(v: &Vault, year: i32) -> Result<Vec<StoredEntry>> {
    let conn = v.connection();
    let mut stmt = conn.prepare(
        "SELECT id, notes, (SELECT COUNT(*) FROM entry_document d WHERE d.entry_id = journal_entry.id)
         FROM journal_entry WHERE fiscal_year = ?1
         ORDER BY date,
                  CASE origin_kind WHEN 'Apertura' THEN 0 WHEN 'Regularizacion' THEN 2
                                   WHEN 'Cierre' THEN 3 ELSE 1 END,
                  number",
    )?;
    let rows: Vec<(i64, Option<String>, i64)> = stmt
        .query_map([year], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<_, _>>()?;
    rows.into_iter()
        .zip(1..)
        .map(|((id, notes, attachments), number)| {
            Ok(StoredEntry {
                id,
                number,
                entry: load_entry(conn, id)?,
                notes,
                attachments,
            })
        })
        .collect()
}

/// Balance de sumas y saldos del ejercicio (opcionalmente agregado a `digits` dígitos).
pub fn trial_balance(v: &Vault, year: i32, digits: Option<usize>) -> Result<Vec<AccountBalance>> {
    let entries: Vec<JournalEntry> = journal(v, year)?.into_iter().map(|s| s.entry).collect();
    let balances = tb(&entries);
    Ok(match digits {
        Some(d) => ledger::aggregate(&balances, d),
        None => balances,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LedgerLine {
    pub number: i64,
    pub date: Date,
    pub concept: String,
    pub account: AccountCode,
    pub debit: Money,
    pub credit: Money,
    pub balance: Money,
    /// Línea del diario (para puntearla).
    pub line_id: i64,
    pub ticked: bool,
}

/// Libro mayor de una cuenta (y sus subcuentas) en el ejercicio.
pub fn ledger_of(v: &Vault, year: i32, prefix: &str) -> Result<Vec<LedgerLine>> {
    let entries = journal(v, year)?;
    let conn = v.connection();
    let ticked: std::collections::HashSet<i64> = conn
        .prepare("SELECT line_id FROM line_tick")?
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    let mut ids_stmt =
        conn.prepare("SELECT id FROM journal_line WHERE entry_id = ?1 ORDER BY id")?;
    let mut running = Money::ZERO;
    let mut out = Vec::new();
    for s in &entries {
        // Mismo orden que `load_lines`.
        let ids: Vec<i64> = ids_stmt
            .query_map([s.id], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        for (l, line_id) in s.entry.lines.iter().zip(ids) {
            if !l.account.starts_with(prefix) {
                continue;
            }
            running += l.net();
            out.push(LedgerLine {
                number: s.number,
                date: s.entry.date,
                concept: s.entry.concept.clone(),
                account: l.account.clone(),
                debit: l.debit,
                credit: l.credit,
                balance: running,
                line_id,
                ticked: ticked.contains(&line_id),
            });
        }
    }
    Ok(out)
}

/// Puntea o despuntea una línea del mayor.
pub fn set_tick(v: &Vault, line_id: i64, ticked: bool, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    if ticked {
        let n = conn.execute(
            "INSERT OR IGNORE INTO line_tick(line_id, ticked_at)
             SELECT id, ?2 FROM journal_line WHERE id = ?1",
            params![line_id, now_str(now)],
        )?;
        let exists: i64 = conn.query_row(
            "SELECT count(*) FROM line_tick WHERE line_id = ?1",
            [line_id],
            |r| r.get(0),
        )?;
        if n == 0 && exists == 0 {
            return Err(AppError::NotFound);
        }
    } else {
        conn.execute("DELETE FROM line_tick WHERE line_id = ?1", [line_id])?;
    }
    Ok(())
}

/// Una fecha mal tecleada (2062, 0226…) crearía un ejercicio que después no se puede quitar y
/// condicionaría las fechas de los de alrededor: los asientos manuales no van más de un año por
/// delante de hoy ni antes del año 2000.
fn check_manual_date(date: Date, now: OffsetDateTime) -> Result<()> {
    if date > now.date() + time::Duration::days(366) || date.year() < 2000 {
        return Err(AppError::Other(format!(
            "revisa la fecha del asiento ({date}): no puede ser anterior al año 2000 ni ir más \
             de un año por delante de hoy"
        )));
    }
    Ok(())
}

/// Asiento manual (p. ej. nóminas, préstamos) tras confirmación del usuario.
pub fn manual_entry(v: &Vault, e: &JournalEntry, now: OffsetDateTime) -> Result<i64> {
    check_manual_date(e.date, now)?;
    let conn = v.connection();
    crate::periodos::exigir_abierto(
        conn,
        e.date,
        &format!("grabar un asiento con fecha {}", e.date),
        now,
    )?;
    let tx = conn.unchecked_transaction()?;
    let id = record_entry(&tx, e, now)?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "journal.manual",
        &e.concept,
        &serde_json::json!({ "entry": id }),
    )?;
    tx.commit()?;
    Ok(id)
}

/// Asiento manual `id` (los automáticos se cambian desde su factura, banco o amortización).
fn manual_of(conn: &Connection, id: i64) -> Result<(JournalEntry, i32)> {
    let year: Option<(i32, String)> = conn
        .query_row(
            "SELECT fiscal_year, origin_kind FROM journal_entry WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (year, kind) = year.ok_or(AppError::NotFound)?;
    if kind != "Manual" && kind != "Importado" {
        return Err(AppError::Other(
            "sólo se modifican o borran los asientos manuales o importados: este sale de una factura, del banco o de otro proceso y se cambia desde allí"
                .into(),
        ));
    }
    Ok((load_entry(conn, id)?, year))
}

/// Cambia un asiento manual (fecha dentro del mismo ejercicio, concepto y líneas). El número se
/// conserva; queda en la auditoría con el antes y el después.
pub fn update_manual_entry(
    v: &Vault,
    id: i64,
    e: &JournalEntry,
    now: OffsetDateTime,
) -> Result<()> {
    e.validate()
        .map_err(|err| AppError::Other(err.to_string()))?;
    // Un asiento importado se modifica igual que uno manual y conserva su origen.
    if !matches!(e.origin, Origin::Manual | Origin::Importado(_)) {
        return Err(AppError::Other(
            "el asiento debe seguir siendo manual".into(),
        ));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let (before, year) = manual_of(&tx, id)?;
    for d in [before.date, e.date] {
        crate::periodos::exigir_abierto(
            &tx,
            d,
            &format!("cambiar el asiento {id} (fecha {d})"),
            now,
        )?;
    }
    if !fiscal_year_conn(&tx, year)?.contains(e.date) {
        return Err(AppError::Other(
            "la fecha cambia de ejercicio: borra el asiento y créalo en el otro".into(),
        ));
    }
    tx.execute(
        "UPDATE journal_entry SET date = ?1, concept = ?2 WHERE id = ?3",
        params![e.date.to_string(), e.concept, id],
    )?;
    tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [id])?;
    for l in &e.lines {
        tx.execute(
            "INSERT INTO journal_line(entry_id, account, debit, credit, description) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, l.account.as_str(), l.debit.cents(), l.credit.cents(), l.description],
        )?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "journal.update",
        &e.concept,
        &serde_json::json!({ "entry": id, "before": before, "after": e }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Borra varios asientos manuales o importados a la vez (con confirmación); todos o ninguno.
/// Devuelve cuántos.
pub fn delete_entries(
    v: &Vault,
    ids: &[i64],
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<usize> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let mut deleted = Vec::new();
    for id in ids {
        let (before, _) = manual_of(&tx, *id)?;
        crate::periodos::exigir_abierto(
            &tx,
            before.date,
            &format!("borrar el asiento {id} (fecha {})", before.date),
            now,
        )?;
        tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [id])?;
        tx.execute("DELETE FROM journal_entry WHERE id = ?1", [id])?;
        deleted.push(serde_json::json!({ "entry": id, "deleted": before }));
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "journal.delete_many",
        &ids.len().to_string(),
        &serde_json::json!({ "entries": deleted }),
    )?;
    tx.commit()?;
    Ok(ids.len())
}

/// Borra un asiento manual (con confirmación). Queda en la auditoría completo.
pub fn delete_manual_entry(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let (before, _) = manual_of(&tx, id)?;
    crate::periodos::exigir_abierto(
        &tx,
        before.date,
        &format!("borrar el asiento {id} (fecha {})", before.date),
        now,
    )?;
    tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [id])?;
    tx.execute("DELETE FROM journal_entry WHERE id = ?1", [id])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "journal.delete",
        &before.concept,
        &serde_json::json!({ "entry": id, "deleted": before }),
    )?;
    tx.commit()?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloseSummary {
    pub result: Money,
    pub regularization_entry: Option<i64>,
    pub closing_entry: Option<i64>,
    pub opening_entry: Option<i64>,
    /// Lo que conviene revisar (p. ej. que el ejercicio siguiente ya tenía apertura).
    pub aviso: Option<String>,
}

/// Regulariza, cierra el ejercicio `year` (bloqueándolo) y abre el siguiente.
pub fn close_year(
    v: &Vault,
    year: i32,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<CloseSummary> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let closed: Option<i64> = conn
        .query_row(
            "SELECT closed FROM fiscal_year WHERE year = ?1",
            [year],
            |r| r.get(0),
        )
        .optional()?;
    if closed == Some(1) {
        return Err(AppError::Other(format!(
            "el ejercicio {year} ya está cerrado"
        )));
    }
    let fy = fiscal_year_conn(conn, year)?;
    // El cierre enlaza cada ejercicio con el siguiente (Código de Comercio, art. 28): si uno
    // anterior con asientos sigue abierto, éste se cerraría sin sus saldos iniciales y, al cerrar
    // aquél después, su apertura no podría entrar en un ejercicio ya cerrado.
    let open_before: Option<i32> = conn
        .query_row(
            "SELECT f.year FROM fiscal_year f
              WHERE f.closed = 0 AND f.end_date < ?1
                AND EXISTS (SELECT 1 FROM journal_entry e WHERE e.fiscal_year = f.year)
              ORDER BY f.end_date LIMIT 1",
            [fy.start.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(prev) = open_before {
        return Err(AppError::Other(format!(
            "cierra antes el ejercicio {prev}: el cierre de {year} necesita la apertura con sus \
             saldos"
        )));
    }
    let mut entries: Vec<JournalEntry> = journal(v, year)?.into_iter().map(|s| s.entry).collect();
    // Los grupos 8 y 9 no se regularizan contra la 129: se traspasan a su cuenta del grupo 13
    // (PGC, quinta parte). Con saldo, el cierre los llevaría a la apertura del año siguiente.
    let pending = taxpain_core::accounting::closing::unsettled_equity_income_expense(&entries);
    if !pending.is_empty() {
        let list: Vec<String> = pending
            .iter()
            .map(|(a, b)| {
                let side = if b.is_negative() {
                    "acreedor"
                } else {
                    "deudor"
                };
                format!("{a} (saldo {side} de {})", b.abs())
            })
            .collect();
        return Err(AppError::Other(format!(
            "no se puede cerrar {year}: las cuentas de los grupos 8 y 9 (gastos e ingresos \
             imputados al patrimonio neto) tienen saldo: {}. Traspásalas antes a su cuenta del \
             grupo 13 con un asiento manual (p. ej. 840 y 940 contra la 130, 800 y 900 contra la \
             133; PGC, quinta parte).",
            list.join(", ")
        )));
    }
    let end = fy.end;
    let start_next = fy.end + time::Duration::days(1);
    let result = taxpain_core::accounting::closing::result_of_year(&entries);
    let tx = conn.unchecked_transaction()?;
    let mut summary = CloseSummary {
        result,
        regularization_entry: None,
        closing_entry: None,
        opening_entry: None,
        aviso: None,
    };
    // ¿El ejercicio siguiente ya tiene apertura (la del asistente de saldos iniciales o la del
    // diario de la gestoría)? Entonces no se añade otra: duplicaría todos los saldos.
    let apertura_previa: Option<String> = tx
        .query_row(
            "SELECT date FROM journal_entry
              WHERE origin_kind = 'Apertura' AND date >= ?1
                AND fiscal_year = (SELECT year FROM fiscal_year
                                    WHERE start_date <= ?1 AND end_date >= ?1)
              ORDER BY date LIMIT 1",
            [start_next.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(r) = regularization(&entries, end) {
        summary.regularization_entry = Some(record_entry(&tx, &r, now)?);
        entries.push(r);
    }
    if let Some(c) = closing(&entries, end) {
        summary.closing_entry = Some(record_entry(&tx, &c, now)?);
        match &apertura_previa {
            None => {
                summary.opening_entry = Some(record_entry(&tx, &opening(&c, start_next), now)?);
            }
            Some(fecha) => {
                summary.aviso = Some(format!(
                    "El ejercicio siguiente ya tenía un asiento de apertura (del {fecha}): no se \
                     ha creado otro. Comprueba que sus saldos son los del cierre de {year}."
                ));
            }
        }
    }
    tx.execute(
        "INSERT INTO fiscal_year(year, start_date, end_date, closed, closed_at)
         VALUES (?1, ?2, ?3, 1, ?4)
         ON CONFLICT(year) DO UPDATE SET closed = 1, closed_at = excluded.closed_at",
        params![year, fy.start.to_string(), fy.end.to_string(), now_str(now)],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "fiscal_year.close",
        &year.to_string(),
        &serde_json::json!({ "result": result.cents() }),
    )?;
    tx.commit()?;
    Ok(summary)
}

/// Reabre un ejercicio cerrado para corregirlo: quita su regularización y su cierre y la apertura
/// del siguiente (que no puede estar cerrado). Después se vuelve a cerrar. Si ya se legalizaron
/// los libros o se depositaron las cuentas, habrá que rehacerlos.
pub fn reopen_year(v: &Vault, year: i32, now: OffsetDateTime, confirmed: bool) -> Result<usize> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let fy = fiscal_year_conn(conn, year)?;
    let closed: Option<i64> = conn
        .query_row(
            "SELECT closed FROM fiscal_year WHERE year = ?1",
            [year],
            |r| r.get(0),
        )
        .optional()?;
    if closed != Some(1) {
        return Err(AppError::Other(format!(
            "el ejercicio {year} no está cerrado"
        )));
    }
    // La aplicación del resultado (en el ejercicio siguiente) parte del resultado cerrado: si se
    // reabre, hay que deshacerla antes y volver a contabilizarla después del nuevo cierre.
    let allocated: i64 = conn.query_row(
        "SELECT count(*) FROM journal_entry WHERE origin_kind = 'AplicacionResultado'
           AND origin_id = ?1",
        [year],
        |r| r.get(0),
    )?;
    if allocated > 0 {
        return Err(AppError::Other(format!(
            "la aplicación del resultado de {year} ya está contabilizada: deshazla antes (Cierre) \
             y vuelve a contabilizarla cuando cierres de nuevo"
        )));
    }
    let next_start = fy.end + time::Duration::days(1);
    let next: Option<(i32, i64)> = conn
        .query_row(
            "SELECT year, closed FROM fiscal_year WHERE start_date <= ?1 AND end_date >= ?1",
            [next_start.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((n, 1)) = next {
        return Err(AppError::Other(format!(
            "el ejercicio siguiente ({n}) también está cerrado: reábrelo antes"
        )));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE fiscal_year SET closed = 0, closed_at = NULL WHERE year = ?1",
        [year],
    )?;
    // Sólo los asientos que hizo el cierre de TaxPain: los de los saldos iniciales y los que
    // vinieron con el ejercicio de la gestoría (su regularización, su cierre o su apertura) se
    // quedan; se quitan deshaciendo esas importaciones.
    const AJENOS: &str = "id NOT IN (SELECT entry_id FROM opening_balance WHERE entry_id IS NOT NULL
                                  UNION SELECT opening_entry FROM gestor_import WHERE opening_entry IS NOT NULL
                                  UNION SELECT regularization_entry FROM gestor_import
                                        WHERE regularization_entry IS NOT NULL
                                  UNION SELECT closing_entry FROM gestor_import WHERE closing_entry IS NOT NULL
                                  UNION SELECT next_opening_entry FROM gestor_import
                                        WHERE next_opening_entry IS NOT NULL)";
    let mut ids: Vec<i64> = {
        let mut st = tx.prepare(&format!(
            "SELECT id FROM journal_entry WHERE fiscal_year = ?1
               AND origin_kind IN ('Regularizacion', 'Cierre') AND {AJENOS}"
        ))?;
        let rows = st.query_map([year], |r| r.get(0))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    if let Some((n, _)) = next {
        let mut st = tx.prepare(&format!(
            "SELECT id FROM journal_entry WHERE fiscal_year = ?1 AND origin_kind = 'Apertura'
               AND date = ?2 AND {AJENOS}"
        ))?;
        let rows = st.query_map(params![n, next_start.to_string()], |r| r.get(0))?;
        for r in rows {
            ids.push(r?);
        }
    }
    for id in &ids {
        tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [id])?;
        tx.execute("DELETE FROM journal_entry WHERE id = ?1", [id])?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "fiscal_year.reopen",
        &year.to_string(),
        &serde_json::json!({ "deleted_entries": ids }),
    )?;
    tx.commit()?;
    Ok(ids.len())
}

// ---------------- Inmovilizado ----------------

use taxpain_core::accounting::depreciation::{FixedAsset, Method, depreciation_entry_in};
use taxpain_core::money::Rate;

/// Comprueba el método y el coeficiente fiscal de un elemento.
fn check_plan(account: &AccountCode, method: Method, tax_rate: Option<Rate>) -> Result<()> {
    // Art. 12.1.b LIS: el porcentaje constante no se aplica a edificios, mobiliario y enseres.
    if method == Method::Constante && (account.starts_with("211") || account.starts_with("216")) {
        return Err(AppError::Other(
            "el método de porcentaje constante no vale para edificios ni mobiliario (art. 12.1.b LIS)"
                .into(),
        ));
    }
    if tax_rate.is_some_and(|r| r.bp() <= 0 || r.bp() > 10_000) {
        return Err(AppError::Other(
            "el coeficiente fiscal debe estar entre 0 y 100 %".into(),
        ));
    }
    Ok(())
}

/// Alta de un elemento del inmovilizado. Devuelve su id.
pub fn add_fixed_asset(v: &Vault, a: &FixedAsset, now: OffsetDateTime) -> Result<i64> {
    if a.cost.cents() <= 0 || a.rate.bp() <= 0 || a.rate.bp() > 10_000 || a.residual > a.cost {
        return Err(AppError::Other("datos de inmovilizado no válidos".into()));
    }
    check_plan(&a.account, a.method, a.tax_rate)?;
    let conn = v.connection();
    conn.execute(
        "INSERT INTO fixed_asset(name, account, depreciation_acc, expense_acc, acquired_on, cost, residual, rate_bp, disposed_on, created_at, method, tax_rate_bp)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            a.name,
            a.account.as_str(),
            a.depreciation_account.as_str(),
            a.expense_account.as_str(),
            a.acquired_on.to_string(),
            a.cost.cents(),
            a.residual.cents(),
            a.rate.bp(),
            a.disposed_on.map(|d| d.to_string()),
            now_str(now),
            a.method.code(),
            a.tax_rate.map(Rate::bp)
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Cambia el método de amortización contable y el coeficiente fiscal de un elemento. Sólo
/// afecta a los ejercicios que aún no tienen su amortización contabilizada.
pub fn set_depreciation_plan(
    v: &Vault,
    id: i64,
    method: Method,
    tax_rate: Option<Rate>,
    now: OffsetDateTime,
) -> Result<()> {
    let a = fixed_assets(v)?
        .into_iter()
        .find(|a| a.id == id)
        .ok_or(AppError::NotFound)?;
    check_plan(&a.account, method, tax_rate)?;
    let conn = v.connection();
    crate::inmovilizado::check_plan(conn, id, tax_rate)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE fixed_asset SET method = ?1, tax_rate_bp = ?2 WHERE id = ?3",
        params![method.code(), tax_rate.map(Rate::bp), id],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "asset.plan",
        &a.name,
        &serde_json::json!({ "method": method.code(), "tax_rate": tax_rate.map(Rate::bp) }),
    )?;
    tx.commit()?;
    Ok(())
}

pub fn fixed_assets(v: &Vault) -> Result<Vec<FixedAsset>> {
    let conn = v.connection();
    let mut stmt = conn.prepare(
        "SELECT id, name, account, depreciation_acc, expense_acc, acquired_on, cost, residual, rate_bp, disposed_on,
                method, tax_rate_bp
         FROM fixed_asset ORDER BY acquired_on, id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, i64>(6)?,
            r.get::<_, i64>(7)?,
            r.get::<_, i64>(8)?,
            r.get::<_, Option<String>>(9)?,
            (r.get::<_, String>(10)?, r.get::<_, Option<i32>>(11)?),
        ))
    })?;
    let code = |s: &str| AccountCode::new(s).map_err(|e| AppError::Other(e.to_string()));
    let mut out = Vec::new();
    for row in rows {
        let (id, name, a, d, e, acq, cost, residual, rate, disposed, (method, tax_rate)) = row?;
        out.push(FixedAsset {
            id,
            name,
            account: code(&a)?,
            depreciation_account: code(&d)?,
            expense_account: code(&e)?,
            acquired_on: parse_date(&acq)?,
            cost: Money::from_cents(cost),
            residual: Money::from_cents(residual),
            rate: Rate::from_bp(i32::try_from(rate).unwrap_or(0)),
            disposed_on: disposed.as_deref().map(parse_date).transpose()?,
            method: Method::parse(&method),
            tax_rate: tax_rate.map(Rate::from_bp),
        });
    }
    Ok(out)
}

/// Diferencia entre la amortización contable y la fiscal de un elemento en un ejercicio.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DepreciationDifference {
    pub asset_id: i64,
    pub name: String,
    /// Amortización contable del ejercicio (la de TaxPain: sin lo de antes de la apertura).
    pub accounting: Money,
    /// Amortización fiscal del ejercicio.
    pub tax: Money,
    /// Contable − fiscal: positiva, aumento del resultado contable en el 200; negativa,
    /// disminución. En el ejercicio de la baja es la reversión de toda la diferencia acumulada.
    pub difference: Money,
    /// El elemento se da de baja en el ejercicio: la diferencia acumulada se revierte entera.
    pub reversal: bool,
}

/// Diferencias del ejercicio entre la amortización contable y la fiscal de cada elemento con plan
/// fiscal propio (coeficiente fiscal, libertad de amortización, amortización acelerada).
///
/// En el ejercicio de la baja o la venta se revierte toda la diferencia acumulada: el resultado
/// fiscal de la baja se calcula con el valor fiscal del elemento (coste menos amortización
/// fiscal), así que lo que se dedujo de más (o de menos) en años anteriores vuelve a la base
/// imponible (art. 11.1 y 12 LIS: la diferencia es temporaria y se revierte con la transmisión).
/// Entonces la diferencia del ejercicio es la fiscal acumulada antes del ejercicio menos la
/// contable acumulada antes del ejercicio: con la del propio ejercicio, la suma de todas es cero.
pub fn depreciation_differences_detail(
    v: &Vault,
    year: i32,
) -> Result<Vec<DepreciationDifference>> {
    let fy = fiscal_year(v, year)?;
    let conn = v.connection();
    let mut out = Vec::new();
    for a in fixed_assets(v)?.iter().filter(|a| a.tax_rate.is_some()) {
        let (period, before) = crate::apertura::depreciation_basis(conn, a, &fy)?;
        let accounting = period.map_or(Money::ZERO, |p| {
            taxpain_core::accounting::depreciation::charge_in(a, &p, before)
        });
        if a.disposed_on.is_some_and(|d| fy.contains(d)) {
            let tax_before =
                taxpain_core::accounting::depreciation::tax_accumulated_before(a, &fy, before);
            let difference = tax_before - before;
            out.push(DepreciationDifference {
                asset_id: a.id,
                name: a.name.clone(),
                accounting,
                tax: accounting - difference,
                difference,
                reversal: true,
            });
            continue;
        }
        // Sólo el tramo que amortiza TaxPain (sin lo de antes de la apertura).
        let Some(p) = period else {
            continue;
        };
        let tax = taxpain_core::accounting::depreciation::tax_charge_in(a, &p, accounting);
        out.push(DepreciationDifference {
            asset_id: a.id,
            name: a.name.clone(),
            accounting,
            tax,
            difference: accounting - tax,
            reversal: false,
        });
    }
    Ok(out)
}

/// Diferencias del ejercicio entre la amortización contable y la fiscal de los elementos con
/// plan fiscal propio: (aumentos, disminuciones) del resultado contable para el 200 (con la
/// reversión de la diferencia acumulada de los dados de baja, ver
/// [`depreciation_differences_detail`]).
pub fn depreciation_differences(v: &Vault, year: i32) -> Result<(Money, Money)> {
    let (mut inc, mut dec) = (Money::ZERO, Money::ZERO);
    for d in depreciation_differences_detail(v, year)? {
        if d.difference.cents() > 0 {
            inc += d.difference;
        } else {
            dec -= d.difference;
        }
    }
    Ok((inc, dec))
}

/// Amortización acumulada de un activo en ejercicios anteriores a `year` contabilizada por
/// TaxPain (lo de antes de usar la app lo suma [`crate::apertura::depreciation_basis`]).
pub(crate) fn accumulated_before(
    conn: &Connection,
    asset: &FixedAsset,
    year: i32,
) -> Result<Money> {
    let start = fiscal_year_conn(conn, year)?.start;
    let cents: i64 = conn.query_row(
        "SELECT COALESCE(SUM(l.credit - l.debit), 0) FROM journal_line l JOIN journal_entry e ON e.id = l.entry_id
         WHERE e.origin_kind = 'Amortizacion' AND e.origin_id = ?1 AND e.date < ?2 AND l.account = ?3",
        params![asset.id, start.to_string(), asset.depreciation_account.as_str()],
        |r| r.get(0),
    )?;
    Ok(Money::from_cents(cents))
}

/// Amortización devengada en `year` hasta `end` (inclusive) que todavía no está en el diario:
/// la de los activos cuya cuota del ejercicio aún no se ha contabilizado con fecha ≤ `end`,
/// prorrateada por días. Sirve para que los pagos fraccionados la tengan en cuenta antes del
/// asiento de cierre del ejercicio.
pub fn accrued_depreciation(v: &Vault, year: i32, end: Date) -> Result<Money> {
    let assets = fixed_assets(v)?;
    let conn = v.connection();
    let fy = fiscal_year_conn(conn, year)?;
    let mut total = Money::ZERO;
    for a in &assets {
        if a.acquired_on > end {
            continue;
        }
        let posted: Option<i64> = conn
            .query_row(
                "SELECT id FROM journal_entry WHERE origin_kind = 'Amortizacion' AND origin_id = ?1
                   AND fiscal_year = ?2 AND date <= ?3",
                params![a.id, year, end.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        if posted.is_some() {
            continue;
        }
        let mut until = a.clone();
        if until.disposed_on.is_none_or(|d| d > end) {
            until.disposed_on = Some(end);
        }
        if let (Some(period), before) = crate::apertura::depreciation_basis(conn, a, &fy)? {
            total += taxpain_core::accounting::depreciation::charge_in(&until, &period, before);
        }
    }
    Ok(total)
}

/// Registra las amortizaciones del ejercicio que falten. Devuelve los asientos creados.
pub fn post_depreciation(v: &Vault, year: i32, now: OffsetDateTime) -> Result<Vec<i64>> {
    let assets = fixed_assets(v)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let fy = fiscal_year_conn(&tx, year)?;
    let mut ids = Vec::new();
    for a in &assets {
        let done: Option<i64> = tx
            .query_row(
                "SELECT id FROM journal_entry WHERE origin_kind = 'Amortizacion' AND origin_id = ?1 AND fiscal_year = ?2",
                params![a.id, year],
                |r| r.get(0),
            )
            .optional()?;
        if done.is_some() {
            continue;
        }
        if let (Some(period), before) = crate::apertura::depreciation_basis(&tx, a, &fy)?
            && let Some(e) = depreciation_entry_in(a, &period, before)
        {
            ids.push(record_entry(&tx, &e, now)?);
        }
    }
    if !ids.is_empty() {
        audit::append(
            &tx,
            &now_str(now),
            Actor::User,
            "depreciation.post",
            &year.to_string(),
            &serde_json::json!({ "entries": ids.len() }),
        )?;
    }
    tx.commit()?;
    Ok(ids)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Disposal {
    /// Amortización del ejercicio hasta la baja: la creada o la ya registrada, rehecha hasta ese
    /// día.
    pub depreciation_entry: Option<i64>,
    /// Lo que se ha quitado de la amortización ya registrada del ejercicio (la de después de la
    /// baja).
    pub depreciation_removed: Money,
    pub disposal_entry: i64,
    /// Asiento que pasa la factura de venta de ventas a la 543 (si se vende con ella).
    pub sale_entry: Option<i64>,
    /// Beneficio (positivo) o pérdida (negativo) de la baja.
    pub result: Money,
    /// Lo que conviene saber: amortización de ejercicios posteriores quitada, parte de la factura
    /// que sigue en ventas…
    pub warnings: Vec<String>,
}

/// Cuentas del resultado de la baja según el grupo del elemento (PGC, quinta parte): (beneficio,
/// pérdida). Intangible (20) 770/670, inversiones inmobiliarias (22) 772/672 y el resto
/// (material) 771/671.
pub(crate) fn result_accounts(asset: &AccountCode) -> (&'static str, &'static str) {
    if asset.starts_with("20") {
        ("770", "670")
    } else if asset.starts_with("22") {
        ("772", "672")
    } else {
        ("771", "671")
    }
}

/// ¿Está cerrado el ejercicio número `year`?
pub(crate) fn year_closed(conn: &Connection, year: i32) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT closed FROM fiscal_year WHERE year = ?1",
            [year],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        == Some(1))
}

/// Borra un asiento automático y sus líneas (dentro de la transacción del llamador). Sus
/// adjuntos se van con él (disparadores de la migración 0027): antes, [`move_attachments`].
pub(crate) fn delete_entry_rows(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM journal_line WHERE entry_id = ?1", [id])?;
    conn.execute("DELETE FROM journal_entry WHERE id = ?1", [id])?;
    Ok(())
}

/// Pasa los documentos adjuntos y las observaciones del asiento automático `from`, que se va a
/// borrar, al asiento `to` (dentro de la transacción del llamador), para que el borrado no se los
/// lleve. Las observaciones se añaden a las de `to` con el concepto y la fecha de `from`. Si `to`
/// es de un ejercicio cerrado y hay algo que pasar, no se borra: se pide quitarlo antes. Devuelve
/// lo pasado, para la auditoría (`null` si nada).
pub(crate) fn move_attachments(conn: &Connection, from: i64, to: i64) -> Result<serde_json::Value> {
    let documents: Vec<String> = {
        let mut st = conn.prepare(
            "SELECT d.filename FROM entry_document x JOIN document d ON d.id = x.document_id
              WHERE x.entry_id = ?1 ORDER BY d.id",
        )?;
        let rows = st.query_map([from], |r| r.get(0))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let (date, concept, notes): (String, String, Option<String>) = conn.query_row(
        "SELECT date, concept, notes FROM journal_entry WHERE id = ?1",
        [from],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let notes = notes.filter(|n| !n.trim().is_empty());
    if documents.is_empty() && notes.is_none() {
        return Ok(serde_json::Value::Null);
    }
    if entry_closed(conn, to)? {
        return Err(AppError::Other(format!(
            "el asiento «{concept}» del {} tiene documentos u observaciones y el asiento al que \
             pasarían es de un ejercicio cerrado: guárdalos y quítalos antes en el Diario",
            dmy(parse_date(&date)?)
        )));
    }
    conn.execute(
        "UPDATE entry_document SET entry_id = ?1 WHERE entry_id = ?2",
        params![to, from],
    )?;
    if let Some(n) = &notes {
        let line = format!("De «{concept}» ({}): {n}", dmy(parse_date(&date)?));
        conn.execute(
            "UPDATE journal_entry
                SET notes = CASE WHEN COALESCE(notes, '') = '' THEN ?1 ELSE notes || char(10) || ?1 END
              WHERE id = ?2",
            params![line, to],
        )?;
    }
    Ok(serde_json::json!({ "to": to, "documents": documents, "notes": notes }))
}

/// Baja (o venta por `price`) de un elemento del inmovilizado el día `date`, sin factura de venta
/// (se vincula después): ver [`dispose_asset_with_invoice`].
pub fn dispose_asset(
    v: &Vault,
    id: i64,
    date: Date,
    price: Money,
    now: OffsetDateTime,
) -> Result<Disposal> {
    dispose_asset_with_invoice(v, id, date, price, None, now)
}

/// Baja (o venta por `price`, sin IVA) de un elemento del inmovilizado el día `date`: amortiza
/// hasta ese día y da de baja el coste y la amortización acumulada; el precio va a la 543
/// (créditos por enajenación de inmovilizado) y la diferencia con el valor neto al resultado
/// (771/671; 770/670 en el intangible y 772/672 en las inversiones inmobiliarias). Con la factura
/// emitida `invoice`, en la misma transacción su base pasa de ventas a la 543 (ver
/// [`crate::venta_inmovilizado`]): el ingreso cuenta una vez y el cobro sigue en la 430. Si la
/// 543 ya no puede tener el precio (un cobro conciliado contra ella antes de la baja), no se da de
/// baja con la factura.
///
/// Si la amortización del ejercicio ya estaba registrada hasta el cierre, su asiento se rehace
/// hasta el día de la baja (fecha y cuota que habría tenido de darse de baja antes): es anular
/// lo que pasaba de ese día (28x a 68x) sin que la 68x quede negativa en los trimestres ni la
/// memoria cuente una dotación que no fue. La de ejercicios posteriores a la baja, si se registró,
/// se quita. Los documentos y observaciones de un asiento de amortización que se quita pasan al
/// de la baja.
pub fn dispose_asset_with_invoice(
    v: &Vault,
    id: i64,
    date: Date,
    price: Money,
    invoice: Option<i64>,
    now: OffsetDateTime,
) -> Result<Disposal> {
    if price.is_negative() {
        return Err(AppError::Other(
            "el precio de venta no puede ser negativo".into(),
        ));
    }
    if invoice.is_some() && price.is_zero() {
        return Err(AppError::Other(
            "una venta con factura tiene precio: pon el de este elemento (sin IVA)".into(),
        ));
    }
    let mut asset = fixed_assets(v)?
        .into_iter()
        .find(|a| a.id == id)
        .ok_or(AppError::NotFound)?;
    if asset.disposed_on.is_some() {
        return Err(AppError::Other("ese elemento ya está dado de baja".into()));
    }
    if date < asset.acquired_on {
        return Err(AppError::Other(
            "la baja no puede ser anterior al alta".into(),
        ));
    }
    let conn = v.connection();
    let prior_until: Option<String> = conn.query_row(
        "SELECT prior_until FROM fixed_asset WHERE id = ?1",
        [id],
        |r| r.get(0),
    )?;
    if let Some(u) = prior_until.as_deref().map(parse_date).transpose()?
        && date < u
    {
        return Err(AppError::Other(format!(
            "lo amortizado antes de TaxPain llega al {}: la baja no puede ser anterior",
            dmy(u)
        )));
    }
    let tx = conn.unchecked_transaction()?;
    let fy = fiscal_year_for_date(&tx, date)?;
    if year_closed(&tx, fy.year)? {
        return Err(AppError::Other(format!(
            "el ejercicio {} está cerrado: reábrelo desde Cierre para dar de baja con esa fecha",
            fy.label()
        )));
    }
    let mut warnings = Vec::new();
    // Amortización de ejercicios posteriores a la baja (registrada antes de saber de la venta):
    // ya no toca.
    let later: Vec<(i64, i32, bool)> = {
        let mut st = tx.prepare(
            "SELECT e.id, e.fiscal_year, COALESCE(f.closed, 0) = 1 FROM journal_entry e
               LEFT JOIN fiscal_year f ON f.year = e.fiscal_year
              WHERE e.origin_kind = 'Amortizacion' AND e.origin_id = ?1 AND e.date > ?2
              ORDER BY e.date",
        )?;
        let rows = st.query_map(params![id, fy.end.to_string()], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    if let Some((_, y, _)) = later.iter().find(|(_, _, closed)| *closed) {
        return Err(AppError::Other(format!(
            "la amortización de {y} de «{}» está en un ejercicio cerrado: la baja no puede ser \
             de antes",
            asset.name
        )));
    }
    asset.disposed_on = Some(date);
    let (period, before) = crate::apertura::depreciation_basis(&tx, &asset, &fy)?;
    let charge = period.map_or(Money::ZERO, |p| {
        taxpain_core::accounting::depreciation::charge_in(&asset, &p, before)
    });
    let posted: Option<i64> = tx
        .query_row(
            "SELECT id FROM journal_entry WHERE origin_kind = 'Amortizacion' AND origin_id = ?1 AND fiscal_year = ?2",
            params![id, fy.year],
            |r| r.get(0),
        )
        .optional()?;
    let mut depreciation_entry = None;
    let mut depreciation_removed = Money::ZERO;
    let mut accumulated = before;
    let mut rewritten = serde_json::Value::Null;
    // Asiento del ejercicio que se queda sin cuota: se borra después de la baja (sus adjuntos
    // pasan a ésta).
    let mut drop_posted = None;
    match posted {
        // Ya registrada hasta el cierre: se rehace hasta la baja (nunca más de lo registrado).
        Some(e) => {
            let old = load_entry(&tx, e)?;
            let booked: Money = old
                .lines
                .iter()
                .filter(|l| l.account == asset.depreciation_account)
                .map(|l| l.credit - l.debit)
                .sum();
            if old.lines.len() != 2 || booked.cents() <= 0 {
                return Err(AppError::Other(format!(
                    "el asiento de amortización {} de «{}» no es el que hace TaxPain: revísalo \
                     antes de dar de baja",
                    fy.label(),
                    asset.name
                )));
            }
            let new = charge.min(booked);
            depreciation_removed = booked - new;
            accumulated += new;
            if new.is_zero() {
                drop_posted = Some(e);
            } else {
                tx.execute(
                    "UPDATE journal_entry SET date = ?1 WHERE id = ?2",
                    params![date.to_string(), e],
                )?;
                tx.execute(
                    "UPDATE journal_line SET debit = ?1 WHERE entry_id = ?2 AND debit > 0",
                    params![new.cents(), e],
                )?;
                tx.execute(
                    "UPDATE journal_line SET credit = ?1 WHERE entry_id = ?2 AND credit > 0",
                    params![new.cents(), e],
                )?;
                depreciation_entry = Some(e);
            }
            rewritten = serde_json::json!({ "entry": e, "before": old, "after": new.cents() });
        }
        None => {
            if let Some(e) = period.and_then(|p| depreciation_entry_in(&asset, &p, before)) {
                accumulated += charge;
                depreciation_entry = Some(record_entry(&tx, &e, now)?);
            }
        }
    }
    let (gain, loss) = result_accounts(&asset.account);
    let net = asset.cost - accumulated;
    let result = price - net;
    let code = |c: &str| AccountCode::new(c).map_err(|e| AppError::Other(e.to_string()));
    let mut lines = vec![EntryLine::credit(asset.account.clone(), asset.cost)];
    lines.extend(EntryLine::signed(
        asset.depreciation_account.clone(),
        accumulated,
        true,
    ));
    lines.extend(EntryLine::signed(code("543")?, price, true));
    if result.is_negative() {
        lines.push(EntryLine::debit(code(loss)?, -result));
    } else if !result.is_zero() {
        lines.push(EntryLine::credit(code(gain)?, result));
    }
    let entry = JournalEntry {
        date,
        concept: if price.is_zero() {
            format!("Baja de {}", asset.name)
        } else {
            format!("Venta de {}", asset.name)
        },
        origin: Origin::BajaInmovilizado(id),
        lines,
    };
    let disposal_entry = record_entry(&tx, &entry, now)?;
    // Los asientos de amortización que sobran se borran ahora, con sus documentos y observaciones
    // pasados al de la baja (el borrado se los llevaría).
    let mut moved_any = false;
    if let Some(e) = drop_posted {
        let moved = move_attachments(&tx, e, disposal_entry)?;
        moved_any |= !moved.is_null();
        delete_entry_rows(&tx, e)?;
        if let Some(r) = rewritten.as_object_mut() {
            r.insert("moved".into(), moved);
        }
    }
    let mut later_removed = Vec::new();
    for (e, y, _) in &later {
        let deleted = load_entry(&tx, *e)?;
        let moved = move_attachments(&tx, *e, disposal_entry)?;
        moved_any |= !moved.is_null();
        delete_entry_rows(&tx, *e)?;
        later_removed
            .push(serde_json::json!({ "entry": e, "year": y, "deleted": deleted, "moved": moved }));
    }
    if !later.is_empty() {
        let years: Vec<String> = later.iter().map(|(_, y, _)| y.to_string()).collect();
        warnings.push(format!(
            "Se ha quitado la amortización de {} de «{}»: es de después de la baja.",
            years.join(", "),
            asset.name
        ));
    }
    if moved_any {
        warnings.push(
            "Los documentos y las observaciones de la amortización quitada pasan al asiento de \
             la baja."
                .into(),
        );
    }
    tx.execute(
        "UPDATE fixed_asset SET disposed_on = ?1, sale_price = ?2, disposal_entry = ?3 WHERE id = ?4",
        params![date.to_string(), price.cents(), disposal_entry, id],
    )?;
    let mut sale_entry = None;
    if let Some(invoice) = invoice {
        let link =
            crate::venta_inmovilizado::link_in(&tx, id, invoice, Some(price), Actor::User, now)?;
        sale_entry = Some(link.entry_id);
        warnings.extend(link.warnings);
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "asset.dispose",
        &id.to_string(),
        &serde_json::json!({
            "date": date.to_string(),
            "price": price.cents(),
            "result": result.cents(),
            "invoice": invoice,
            "depreciation": rewritten,
            "later_removed": later_removed,
        }),
    )?;
    tx.commit()?;
    Ok(Disposal {
        depreciation_entry,
        depreciation_removed,
        disposal_entry,
        sale_entry,
        result,
        warnings,
    })
}

fn dmy(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

// ---------------- Liquidación del IVA ----------------

/// Saldo deudor neto (debe − haber) de las cuentas `prefix` (477, 472) movido en el periodo, sin
/// los asientos de liquidación, regularización, cierre ni apertura. Una factura recibida cuenta
/// por su fecha de registro (la peninsular de `reviewed_at`, la que la lleva al 303 y al libro
/// registro, art. 99.Tres LIVA), no por la del asiento (la de la factura): así lo que se salda es
/// lo que el 303 del periodo deduce, y el IVA de una factura registrada tarde sigue en la 472
/// hasta el periodo en que se deduce.
fn period_net(conn: &Connection, prefix: &str, from: Date, to: Date) -> Result<Money> {
    // SQL criba con un día de margen (la fecha de registro guardada en UTC puede ser la del día
    // anterior en España) y el filtro exacto se hace aquí.
    let day = time::Duration::days(1);
    let mut st = conn.prepare(
        "SELECT l.debit - l.credit, e.date, r.reviewed_at FROM journal_line l
           JOIN journal_entry e ON e.id = l.entry_id
           LEFT JOIN invoice_received r
             ON e.origin_kind = 'FacturaRecibida' AND r.id = e.origin_id
         WHERE l.account LIKE ?1 || '%'
           AND COALESCE(substr(r.reviewed_at, 1, 10), e.date) BETWEEN ?2 AND ?3
           AND e.origin_kind NOT IN ('LiquidacionIva', 'Regularizacion', 'Cierre', 'Apertura')",
    )?;
    let rows = st.query_map(
        params![prefix, (from - day).to_string(), (to + day).to_string()],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        },
    )?;
    let mut total = Money::ZERO;
    for row in rows {
        let (cents, date, reviewed) = row?;
        let d = match reviewed {
            Some(ts) => crate::models::local_date(&ts)?,
            None => parse_date(&date)?,
        };
        if (from..=to).contains(&d) {
            total += Money::from_cents(cents);
        }
    }
    Ok(total)
}

/// Registra la liquidación del IVA del trimestre `quarter` (1–4) de `year` a partir de su 303:
/// 477/472 por lo devengado y deducido, 4700 al haber por la compensación de periodos
/// anteriores aplicada ([78]) y 4750 sólo por el resultado a ingresar ([71]), o 4700 al debe
/// por el nuevo exceso (ver `taxpain_core::accounting::closing::vat_settlement`). Devuelve el
/// asiento creado (o `None` si no hay IVA). Idempotente por periodo.
pub fn settle_vat_quarter(
    v: &Vault,
    year: i32,
    quarter: u8,
    now: OffsetDateTime,
) -> Result<Option<i64>> {
    let (from, to) = quarter_range(year, quarter)?;
    let conn = v.connection();
    let key = i64::from(year) * 100 + i64::from(quarter);
    if let Some(existing) = conn
        .query_row(
            "SELECT id FROM journal_entry WHERE origin_kind = 'LiquidacionIva' AND origin_id = ?1",
            [key],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
    {
        return Ok(Some(existing));
    }
    let Some((e, settlement)) = vat_settlement_entry(v, year, quarter, from, to)? else {
        return Ok(None);
    };
    let tx = conn.unchecked_transaction()?;
    let id = record_entry(&tx, &e, now)?;
    audit_vat(&tx, "vat.settle", year, quarter, &settlement, now)?;
    tx.commit()?;
    Ok(Some(id))
}

/// Primer y último día del trimestre.
fn quarter_range(year: i32, quarter: u8) -> Result<(Date, Date)> {
    if !(1..=4).contains(&quarter) {
        return Err(AppError::Other("trimestre no válido".into()));
    }
    let from_m = Month::try_from(quarter * 3 - 2).map_err(|e| AppError::Other(e.to_string()))?;
    let to_m = Month::try_from(quarter * 3).map_err(|e| AppError::Other(e.to_string()))?;
    let from =
        Date::from_calendar_date(year, from_m, 1).map_err(|e| AppError::Other(e.to_string()))?;
    let to = Date::from_calendar_date(year, to_m, to_m.length(year))
        .map_err(|e| AppError::Other(e.to_string()))?;
    Ok((from, to))
}

fn audit_vat(
    conn: &Connection,
    action: &str,
    year: i32,
    quarter: u8,
    s: &taxpain_core::accounting::closing::VatSettlement,
    now: OffsetDateTime,
) -> Result<()> {
    audit::append(
        conn,
        &now_str(now),
        Actor::User,
        action,
        &format!("{quarter}T {year}"),
        &serde_json::json!({
            "output": s.output_vat.cents(),
            "input": s.input_vat.cents(),
            "applied": s.applied.cents(),
            "result": s.result.cents(),
        }),
    )?;
    Ok(())
}

/// Rehace la liquidación del IVA de un trimestre (borra su asiento y la vuelve a calcular con
/// el 303 de ahora), p. ej. tras registrar una factura más. Sólo si el 303 no está presentado,
/// no tiene el pago registrado, el asiento no está conciliado con el banco y el ejercicio no
/// está cerrado. Devuelve el asiento nuevo (o `None` si ya no hay IVA).
pub fn redo_vat_quarter(
    v: &Vault,
    year: i32,
    quarter: u8,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<Option<i64>> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let (from, to) = quarter_range(year, quarter)?;
    let conn = v.connection();
    let key = i64::from(year) * 100 + i64::from(quarter);
    let old: i64 = conn
        .query_row(
            "SELECT id FROM journal_entry WHERE origin_kind = 'LiquidacionIva' AND origin_id = ?1",
            [key],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| {
            AppError::Other(format!(
                "el IVA del {quarter}T de {year} aún no está liquidado: liquídalo"
            ))
        })?;
    let period = format!("{quarter}T");
    let why: Option<String> = conn.query_row(
        "SELECT CASE
            WHEN EXISTS(SELECT 1 FROM fiscal_year f JOIN journal_entry e ON e.fiscal_year = f.year
                         WHERE e.id = ?1 AND f.closed = 1)
              THEN 'su ejercicio está cerrado'
            WHEN EXISTS(SELECT 1 FROM tax_return WHERE model = '303' AND year = ?2
                         AND period = ?3 AND environment = 'produccion')
              OR EXISTS(SELECT 1 FROM filed_model WHERE model = '303' AND year = ?2
                         AND period = ?3)
              THEN 'el 303 ya está presentado'
            WHEN EXISTS(SELECT 1 FROM tax_payment WHERE model = '303' AND year = ?2
                         AND period = ?3)
              THEN 'ya tiene el pago o la devolución registrados: deshazlo antes'
            WHEN EXISTS(SELECT 1 FROM bank_tx WHERE entry_id = ?1)
              THEN 'el asiento está conciliado con el banco: deshaz la conciliación antes'
          END",
        params![old, year, period],
        |r| r.get(0),
    )?;
    if let Some(w) = why {
        return Err(AppError::Other(format!(
            "no se puede rehacer la liquidación del {quarter}T de {year}: {w}"
        )));
    }
    let new = vat_settlement_entry(v, year, quarter, from, to)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [old])?;
    tx.execute("DELETE FROM journal_entry WHERE id = ?1", [old])?;
    let id = match &new {
        Some((e, s)) => {
            let id = record_entry(&tx, e, now)?;
            audit_vat(&tx, "vat.redo", year, quarter, s, now)?;
            Some(id)
        }
        None => None,
    };
    tx.commit()?;
    Ok(id)
}

/// El asiento de liquidación del IVA del trimestre según su 303 (sin guardarlo).
fn vat_settlement_entry(
    v: &Vault,
    year: i32,
    quarter: u8,
    from: Date,
    to: Date,
) -> Result<
    Option<(
        JournalEntry,
        taxpain_core::accounting::closing::VatSettlement,
    )>,
> {
    let conn = v.connection();
    let key = i64::from(year) * 100 + i64::from(quarter);
    // La liquidación sale del 303 del periodo (PGC PYMES, cuentas 4700 y 4750; art. 99.Cinco
    // LIVA): la 4700 se abona por las cuotas de periodos anteriores que el modelo compensa
    // ([78]) y la 4750 sólo por el resultado a ingresar ([71]), que es lo que después paga
    // `tax_payments::pay`; un resultado negativo carga la 4700 por el nuevo exceso.
    // Si el 303 ya está presentado, la compensación es la de la declaración; el resultado es el
    // mismo importe que cobra `tax_payments::pay` (el presentado o, si no, el del borrador).
    let d = crate::models::draft(v, "303", year, quarter)?;
    let get = |b: u16| {
        d.boxes
            .iter()
            .find(|(k, _)| *k == b)
            .map_or(Money::ZERO, |(_, m)| *m)
    };
    let applied = match crate::models::filed_303_boxes(v, year, quarter)? {
        Some(f) => f.get(&78).copied().unwrap_or(Money::ZERO),
        None => get(78),
    };
    let (result, _) = crate::tax_payments::amount_due(v, "303", year, quarter)?;
    // Con criterio de caja el IVA del trimestre no es el de las facturas del trimestre, sino el
    // devengado al cobrar y el deducible al pagar: se toma del 303 ([27] y [45]). Lo que aún no
    // se ha devengado sigue en la 477/472. TODO(VERIFY): subcuentas de IVA pendiente de devengo.
    let (output, input) = if crate::models::settings(v)?.cash_basis {
        (get(27), get(45))
    } else {
        (
            // El IVA de otros Estados miembros (ventanilla única, 4779) no es del 303: se
            // paga con el 369.
            -(period_net(conn, "477", from, to)?
                - period_net(conn, taxpain_core::invoice::oss::CUENTA_OSS_RAIZ, from, to)?),
            period_net(conn, "472", from, to)?,
        )
    };
    let settlement = taxpain_core::accounting::closing::VatSettlement {
        output_vat: output,
        input_vat: input,
        applied,
        result,
    };
    Ok(taxpain_core::accounting::closing::vat_settlement(
        &settlement,
        to,
        key,
        &format!("{quarter}T {year}"),
    )
    .map(|e| (e, settlement)))
}

// ---------------- Cuentas anuales ----------------

use taxpain_core::accounting::statements::{
    ModelKind, Statement, balance_sheet_for, income_statement_for,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AnnualAccounts {
    pub balance: Statement,
    pub income: Statement,
    /// Activo = patrimonio neto + pasivo.
    pub balanced: bool,
}

/// Balance y PyG (modelos PYMES) del ejercicio, antes de regularizar y cerrar.
pub fn annual_accounts(v: &Vault, year: i32) -> Result<AnnualAccounts> {
    annual_accounts_model(v, year, ModelKind::Pymes)
}

/// Balance y PyG del ejercicio con el modelo `kind` (PYMES, abreviado o normal).
pub fn annual_accounts_model(v: &Vault, year: i32, kind: ModelKind) -> Result<AnnualAccounts> {
    let entries: Vec<JournalEntry> = journal(v, year)?
        .into_iter()
        .map(|s| s.entry)
        .filter(|e| !matches!(e.origin, Origin::Regularizacion | Origin::Cierre))
        .collect();
    let balances = tb(&entries);
    let income = income_statement_for(kind, &balances);
    let balance = balance_sheet_for(kind, &balances, income.get(kind.result_id()));
    let balanced = balance.get("TA") == balance.get("TP");
    Ok(AnnualAccounts {
        balance,
        income,
        balanced,
    })
}

// ---------------- Plan de cuentas ----------------

/// Cuenta del plan de la empresa, con sus movimientos del ejercicio.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ChartAccount {
    pub code: String,
    pub name: String,
    /// `propia` (nombre dado por la empresa), `tercero` (cliente o proveedor) o `pgc`.
    pub source: &'static str,
    pub nif: Option<String>,
    pub debit: Money,
    pub credit: Money,
}

/// Nombre de una cuenta: el propio de la empresa, el del cliente o proveedor, o el del cuadro
/// del PGC de PYMES (para subcuentas, el de su cuenta).
pub fn account_name(conn: &Connection, code: &str) -> Result<Option<String>> {
    if let Some(n) = conn
        .query_row("SELECT name FROM account WHERE code = ?1", [code], |r| {
            r.get::<_, String>(0)
        })
        .optional()?
    {
        return Ok(Some(n));
    }
    if let Some(n) = conn
        .query_row(
            "SELECT name FROM third_party_account WHERE account = ?1",
            [code],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        return Ok(Some(n));
    }
    Ok(taxpain_core::accounting::pgc::name_for(code).map(str::to_owned))
}

/// Plan de cuentas de la empresa: las cuentas con nombre propio, los clientes y proveedores y
/// las que tienen movimientos en `year`, ordenadas por código.
pub fn chart(v: &Vault, year: i32) -> Result<Vec<ChartAccount>> {
    use std::collections::BTreeMap;
    let conn = v.connection();
    let mut out: BTreeMap<String, ChartAccount> = BTreeMap::new();
    let blank = |code: &str, name: String, source: &'static str| ChartAccount {
        code: code.to_owned(),
        name,
        source,
        nif: None,
        debit: Money::ZERO,
        credit: Money::ZERO,
    };
    {
        let mut stmt = conn.prepare(
            "SELECT l.account, SUM(l.debit), SUM(l.credit) FROM journal_line l
             JOIN journal_entry e ON e.id = l.entry_id
             WHERE e.fiscal_year = ?1 AND e.origin_kind NOT IN ('Regularizacion', 'Cierre')
             GROUP BY l.account",
        )?;
        let rows = stmt.query_map([year], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        for row in rows {
            let (code, d, c) = row?;
            let name = taxpain_core::accounting::pgc::name_for(&code)
                .unwrap_or("")
                .to_owned();
            let mut a = blank(&code, name, "pgc");
            a.debit = Money::from_cents(d);
            a.credit = Money::from_cents(c);
            out.insert(code, a);
        }
    }
    {
        let mut stmt = conn.prepare("SELECT account, name, tax_id FROM third_party_account")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (code, name, nif) = row?;
            let a = out
                .entry(code.clone())
                .or_insert_with(|| blank(&code, String::new(), "tercero"));
            a.name = name;
            a.source = "tercero";
            a.nif = Some(nif);
        }
    }
    {
        let mut stmt = conn.prepare("SELECT code, name FROM account")?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (code, name) = row?;
            let a = out
                .entry(code.clone())
                .or_insert_with(|| blank(&code, String::new(), "propia"));
            a.name = name;
            a.source = "propia";
        }
    }
    Ok(out.into_values().collect())
}

/// Da de alta una cuenta o cambia su nombre en el plan de la empresa.
pub fn set_account(v: &Vault, code: &str, name: &str, now: OffsetDateTime) -> Result<()> {
    let code = AccountCode::new(code.trim()).map_err(|e| AppError::Other(e.to_string()))?;
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Other("pon el nombre de la cuenta".into()));
    }
    if code.as_str().len() < 3 {
        return Err(AppError::Other(
            "los grupos y subgrupos del PGC no se cambian: usa una cuenta de 3 o más dígitos"
                .into(),
        ));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO account(code, name, created_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(code) DO UPDATE SET name = excluded.name",
        params![code.as_str(), name, now_str(now)],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "account.set",
        code.as_str(),
        &serde_json::json!({ "name": name }),
    )?;
    tx.commit()?;
    Ok(())
}

// ---------------- Observaciones y adjuntos de los asientos ----------------

/// Tamaño máximo de un documento adjunto.
pub const MAX_ATTACHMENT: usize = 25 * 1024 * 1024;

fn entry_closed(conn: &Connection, entry_id: i64) -> Result<bool> {
    let closed: Option<i64> = conn
        .query_row(
            "SELECT f.closed FROM journal_entry e JOIN fiscal_year f ON f.year = e.fiscal_year
             WHERE e.id = ?1",
            [entry_id],
            |r| r.get(0),
        )
        .optional()?;
    match closed {
        None => {
            let exists: Option<i64> = conn
                .query_row(
                    "SELECT 1 FROM journal_entry WHERE id = ?1",
                    [entry_id],
                    |r| r.get(0),
                )
                .optional()?;
            exists.map(|_| false).ok_or(AppError::NotFound)
        }
        Some(c) => Ok(c == 1),
    }
}

fn open_entry(conn: &Connection, entry_id: i64) -> Result<()> {
    if entry_closed(conn, entry_id)? {
        return Err(AppError::Other(
            "el ejercicio de ese asiento está cerrado".into(),
        ));
    }
    Ok(())
}

/// Cambia las observaciones de un asiento (vacías = sin observaciones).
pub fn set_entry_notes(v: &Vault, entry_id: i64, notes: &str, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    open_entry(conn, entry_id)?;
    let notes = notes.trim();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE journal_entry SET notes = ?1 WHERE id = ?2",
        params![(!notes.is_empty()).then_some(notes), entry_id],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "journal.notes",
        &entry_id.to_string(),
        &serde_json::json!({ "notes": notes }),
    )?;
    tx.commit()?;
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EntryDocument {
    pub id: i64,
    pub filename: String,
    pub size: i64,
}

fn mime_of(filename: &str, bytes: &[u8]) -> &'static str {
    let lower = filename.to_lowercase();
    if bytes.starts_with(b"%PDF") {
        "application/pdf"
    } else if bytes.starts_with(b"\x89PNG") {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if lower.ends_with(".xml") {
        "application/xml"
    } else {
        "application/octet-stream"
    }
}

/// Adjunta un documento a un asiento. Devuelve su id.
pub fn attach_document(
    v: &Vault,
    entry_id: i64,
    filename: &str,
    bytes: &[u8],
    now: OffsetDateTime,
) -> Result<i64> {
    use sha2::{Digest, Sha256};
    if bytes.is_empty() || bytes.len() > MAX_ATTACHMENT {
        return Err(AppError::Other(
            "el documento está vacío o pasa de 25 MB".into(),
        ));
    }
    let conn = v.connection();
    open_entry(conn, entry_id)?;
    let sha: String = Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let name = std::path::Path::new(filename)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("documento")
        .to_owned();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO document(uuid, kind, filename, mime, sha256, size, content, created_at)
         VALUES (?1, 'asiento', ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            format!("asiento-{entry_id}-{}", &sha[..32]),
            name,
            mime_of(&name, bytes),
            sha,
            i64::try_from(bytes.len()).unwrap_or(0),
            bytes,
            now_str(now)
        ],
    )
    .map_err(|e| match e {
        taxpain_vault::rusqlite::Error::SqliteFailure(f, _)
            if f.code == taxpain_vault::rusqlite::ErrorCode::ConstraintViolation =>
        {
            AppError::Other("ese documento ya está adjunto a este asiento".into())
        }
        e => e.into(),
    })?;
    let doc = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO entry_document(entry_id, document_id) VALUES (?1, ?2)",
        params![entry_id, doc],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "journal.attach",
        &entry_id.to_string(),
        &serde_json::json!({ "document": doc, "sha256": sha }),
    )?;
    tx.commit()?;
    Ok(doc)
}

pub fn entry_documents(v: &Vault, entry_id: i64) -> Result<Vec<EntryDocument>> {
    let mut st = v.connection().prepare(
        "SELECT d.id, d.filename, d.size FROM entry_document e JOIN document d ON d.id = e.document_id
         WHERE e.entry_id = ?1 ORDER BY d.id",
    )?;
    let rows = st.query_map([entry_id], |r| {
        Ok(EntryDocument {
            id: r.get(0)?,
            filename: r.get(1)?,
            size: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Nombre, tipo y contenido de un documento adjunto a un asiento.
pub fn entry_document(v: &Vault, document_id: i64) -> Result<(String, String, Vec<u8>)> {
    v.connection()
        .query_row(
            "SELECT d.filename, d.mime, d.content FROM document d
             JOIN entry_document e ON e.document_id = d.id WHERE d.id = ?1",
            [document_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)
}

/// Quita un documento de un asiento (y lo borra del almacén).
pub fn detach_document(
    v: &Vault,
    entry_id: i64,
    document_id: i64,
    now: OffsetDateTime,
) -> Result<()> {
    let conn = v.connection();
    open_entry(conn, entry_id)?;
    let tx = conn.unchecked_transaction()?;
    let n = tx.execute(
        "DELETE FROM entry_document WHERE entry_id = ?1 AND document_id = ?2",
        params![entry_id, document_id],
    )?;
    if n == 0 {
        return Err(AppError::NotFound);
    }
    // El documento lo borra el disparador `entry_document_gone`.
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "journal.detach",
        &entry_id.to_string(),
        &serde_json::json!({ "document": document_id }),
    )?;
    tx.commit()?;
    Ok(())
}

// ---------------- Copiar el plan de cuentas de otra empresa ----------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ChartCopy {
    /// Cuentas con nombre propio copiadas (las que ya existían no se tocan).
    pub accounts: usize,
    /// Se copiaron también las cuentas de la contabilización automática.
    pub posting_accounts: bool,
}

/// Copia en `to` las cuentas con nombre propio de `from` y, si `to` aún no tiene clientes ni
/// proveedores, sus cuentas de la contabilización automática (con los dígitos de subcuenta).
pub fn copy_chart(from: &Vault, to: &Vault, now: OffsetDateTime) -> Result<ChartCopy> {
    let src: Vec<(String, String)> = {
        let mut st = from
            .connection()
            .prepare("SELECT code, name FROM account ORDER BY code")?;
        let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let conn = to.connection();
    let third: i64 =
        conn.query_row("SELECT COUNT(*) FROM third_party_account", [], |r| r.get(0))?;
    let posting = posting_accounts(from.connection())?;
    let tx = conn.unchecked_transaction()?;
    let mut accounts = 0;
    for (code, name) in &src {
        accounts += tx.execute(
            "INSERT OR IGNORE INTO account(code, name, created_at) VALUES (?1, ?2, ?3)",
            params![code, name, now_str(now)],
        )?;
    }
    let posting_accounts = third == 0;
    if posting_accounts {
        tx.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![
                SETTINGS_KEY,
                serde_json::to_string(&posting).map_err(|e| AppError::Other(e.to_string()))?
            ],
        )?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "chart.copy",
        &accounts.to_string(),
        &serde_json::json!({ "accounts": accounts, "posting_accounts": posting_accounts }),
    )?;
    tx.commit()?;
    Ok(ChartCopy {
        accounts,
        posting_accounts,
    })
}

// ---------------- Traspaso de saldo entre cuentas ----------------

/// Traspasa el saldo de la cuenta `from` a `to` el día `date` (asiento manual), p. ej. para
/// corregir una subcuenta creada por error. Devuelve el asiento, o `None` si no tenía saldo.
pub fn transfer_balance(
    v: &Vault,
    from: &str,
    to: &str,
    date: Date,
    now: OffsetDateTime,
) -> Result<Option<i64>> {
    let code = |c: &str| AccountCode::new(c.trim()).map_err(|e| AppError::Other(e.to_string()));
    let (from, to) = (code(from)?, code(to)?);
    if from == to {
        return Err(AppError::Other("las dos cuentas son la misma".into()));
    }
    let conn = v.connection();
    let fy = fiscal_year_for_date(conn, date)?;
    let net: i64 = conn.query_row(
        "SELECT COALESCE(SUM(l.debit - l.credit), 0) FROM journal_line l
         JOIN journal_entry e ON e.id = l.entry_id
         WHERE l.account = ?1 AND e.fiscal_year = ?2 AND e.date <= ?3
           AND e.origin_kind <> 'Cierre'",
        params![from.as_str(), fy.year, date.to_string()],
        |r| r.get(0),
    )?;
    let balance = Money::from_cents(net);
    if balance.is_zero() {
        return Ok(None);
    }
    let lines: Vec<EntryLine> = [
        EntryLine::signed(from.clone(), balance, false),
        EntryLine::signed(to.clone(), balance, true),
    ]
    .into_iter()
    .flatten()
    .collect();
    let e = JournalEntry {
        date,
        concept: format!("Traspaso de saldo de {} a {}", from.as_str(), to.as_str()),
        origin: Origin::Manual,
        lines,
    };
    Ok(Some(manual_entry(v, &e, now)?))
}

// ---------------- Traspaso de asientos a otra empresa ----------------

/// Resultado de copiar asientos a otra empresa.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CopyOutcome {
    pub copied: usize,
    /// Subcuentas de clientes o proveedores de la empresa de origen que aparecen en lo copiado:
    /// en la de destino pueden ser de otro tercero; conviene revisarlas.
    pub third_party_accounts: Vec<String>,
}

/// Asientos `ids` del ejercicio `year` de `from`, listos para copiarlos (sin apertura,
/// regularización ni cierre), y las subcuentas de terceros que usan.
pub fn entries_to_copy(
    v: &Vault,
    year: i32,
    ids: &[i64],
) -> Result<(Vec<JournalEntry>, Vec<String>)> {
    let mut out = Vec::new();
    for s in journal(v, year)?
        .into_iter()
        .filter(|s| ids.contains(&s.id))
    {
        if matches!(
            s.entry.origin,
            Origin::Apertura | Origin::Regularizacion | Origin::Cierre
        ) {
            return Err(AppError::Other(format!(
                "el asiento {} es de apertura, regularización o cierre: no se copia",
                s.number
            )));
        }
        out.push(JournalEntry {
            origin: Origin::Manual,
            ..s.entry
        });
    }
    if out.len() != ids.len() {
        return Err(AppError::Other(
            "algún asiento no es de este ejercicio".into(),
        ));
    }
    let conn = v.connection();
    let mut third = std::collections::BTreeSet::new();
    for e in &out {
        for l in &e.lines {
            let is_third: Option<i64> = conn
                .query_row(
                    "SELECT 1 FROM third_party_account WHERE account = ?1",
                    [l.account.as_str()],
                    |r| r.get(0),
                )
                .optional()?;
            if is_third.is_some() {
                third.insert(l.account.as_str().to_owned());
            }
        }
    }
    Ok((out, third.into_iter().collect()))
}

/// Graba en `to` como asientos manuales los asientos copiados de otra empresa (todos o
/// ninguno), con su rastro en la auditoría.
pub fn paste_entries(
    to: &Vault,
    entries: &[JournalEntry],
    source: &str,
    now: OffsetDateTime,
) -> Result<usize> {
    let conn = to.connection();
    let tx = conn.unchecked_transaction()?;
    for e in entries {
        record_entry(&tx, e, now)?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "journal.copy_in",
        source,
        &serde_json::json!({ "entries": entries.len() }),
    )?;
    tx.commit()?;
    Ok(entries.len())
}
