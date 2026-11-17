//! Saldos iniciales: el asistente para la empresa o el autónomo que empieza a usar TaxPain a
//! mitad de camino (Código de Comercio, art. 28.1: el diario se abre con el balance inicial).
//!
//! El borrador se guarda en los ajustes y se puede seguir otro día. Tiene:
//!
//! - el **balance** del gestor (sumas y saldos o su diario, o a mano): una línea por cuenta;
//! - los **clientes y proveedores pendientes** de cobro o pago, que entran en la cartera para
//!   cobrarlos, pagarlos y conciliarlos con el banco ([`crate::cartera`]);
//! - el **inmovilizado** con lo ya amortizado, para que la amortización siga desde ahí sin
//!   repetir lo de antes (CONTABILIDAD-05, PGC NRV 2.ª; art. 12.1 LIS);
//! - los saldos con **Hacienda** (4700, 4750, 4751) y de cada **banco** (57x).
//!
//! Cómo se juntan en el asiento de apertura: manda el balance. Lo de los otros pasos sólo añade
//! líneas para las cuentas que el balance no trae; si las trae, se compara y se avisa cuando no
//! coincide. Al contabilizar se comprueba que cuadra (debe = haber) y que no hay ya otro asiento
//! de apertura en el ejercicio (si lo hay, hay que confirmarlo).

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::depreciation::{FixedAsset, Method};
use taxpain_core::accounting::{AccountCode, EntryLine, FiscalYear, JournalEntry, Origin};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, Duration, OffsetDateTime};

use crate::accounting;
use crate::error::{AppError, Result};
use crate::import::{norm, parse_amount, parse_date};
use crate::invoicing::now_str;

/// Clave del borrador en los ajustes.
pub const DRAFT_KEY: &str = "apertura_borrador";

/// Clave de los ajustes con la cuota a compensar del 303 que anotó una apertura (para quitarla
/// al deshacerla).
const COMPENSATION_KEY: &str = "apertura_compensacion";

/// Una cuenta del balance de apertura.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub account: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub debit: Money,
    #[serde(default)]
    pub credit: Money,
}

/// Factura de antes de TaxPain pendiente de cobro (cliente) o de pago (proveedor).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    /// Subcuenta del tercero (la del balance del gestor); vacía: la que ya tenga en TaxPain o una
    /// nueva.
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub tax_id: Option<String>,
    pub name: String,
    pub number: String,
    pub issue_date: Date,
    pub due_date: Date,
    /// Lo que queda por cobrar o pagar (negativo en un abono).
    pub amount: Money,
}

/// Elemento del inmovilizado comprado antes de TaxPain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub name: String,
    pub account: String,
    pub depreciation_account: String,
    pub expense_account: String,
    pub acquired_on: Date,
    pub cost: Money,
    /// Coeficiente anual de amortización.
    pub rate: Rate,
    #[serde(default)]
    pub method: Method,
    /// Amortización acumulada hasta el día anterior a la apertura.
    #[serde(default)]
    pub accumulated: Money,
}

/// Saldo de una cuenta de banco (positivo: a favor de la empresa).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bank {
    pub account: String,
    #[serde(default)]
    pub name: String,
    pub balance: Money,
}

/// Saldos con Hacienda.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tax {
    /// 4700 Hacienda Pública, deudora por IVA (IVA a compensar o a devolver).
    #[serde(default)]
    pub vat_refund: Money,
    /// 4750 Hacienda Pública, acreedora por IVA (IVA por pagar).
    #[serde(default)]
    pub vat_due: Money,
    /// 4751 Hacienda Pública, acreedora por retenciones practicadas (111, 115… por pagar).
    #[serde(default)]
    pub withholdings: Money,
}

/// Borrador del asistente.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    /// Fecha del asiento de apertura (normalmente el primer día del ejercicio).
    #[serde(default)]
    pub date: Option<Date>,
    /// Último paso visto, para seguir por ahí.
    #[serde(default)]
    pub step: u8,
    /// Fichero del que salió el balance, si se importó.
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub lines: Vec<Line>,
    #[serde(default)]
    pub customers: Vec<Pending>,
    #[serde(default)]
    pub suppliers: Vec<Pending>,
    #[serde(default)]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub banks: Vec<Bank>,
    #[serde(default)]
    pub tax: Tax,
}

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

fn dmy(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

fn code(s: &str) -> Result<AccountCode> {
    AccountCode::new(s).map_err(|_| other(format!("la cuenta «{s}» no es válida")))
}

// ---------------- Borrador ----------------

/// El borrador guardado (o uno vacío).
pub fn draft(v: &Vault) -> Result<Draft> {
    Ok(v.setting(DRAFT_KEY)?.unwrap_or_default())
}

/// Guarda el borrador para seguir después.
pub fn save_draft(v: &Vault, d: &Draft) -> Result<()> {
    Ok(v.set_setting(DRAFT_KEY, d)?)
}

/// Tira el borrador.
pub fn discard_draft(v: &Vault) -> Result<()> {
    v.connection()
        .execute("DELETE FROM settings WHERE key = ?1", [DRAFT_KEY])?;
    Ok(())
}

// ---------------- Leer el balance del gestor ----------------

/// Balance leído de un fichero.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BalanceRead {
    /// `sumas_saldos` (balance de sumas y saldos), `apertura` (el asiento de apertura de un
    /// diario), `cierre` (el inverso del asiento de cierre) o `diario` (saldos de todo el diario).
    pub kind: String,
    pub lines: Vec<Line>,
    /// Fecha que propone el fichero para la apertura, si la hay.
    pub date: Option<Date>,
    pub warnings: Vec<String>,
}

#[derive(Default)]
struct BalanceCols {
    account: Option<usize>,
    /// Una columna de fecha es de un diario, no de un balance.
    date: Option<usize>,
    name: Option<usize>,
    debit_balance: Option<usize>,
    credit_balance: Option<usize>,
    balance: Option<usize>,
    debit: Option<usize>,
    credit: Option<usize>,
}

fn balance_cols(row: &[String]) -> BalanceCols {
    let mut c = BalanceCols::default();
    for (i, h) in row.iter().enumerate() {
        let n = norm(h);
        let slot = match n.as_str() {
            "cuenta" | "subcuenta" | "codigo" | "codigocuenta" | "codcuenta" | "ncuenta" => {
                &mut c.account
            }
            "descripcion" | "nombre" | "titulo" | "denominacion" | "nombrecuenta"
            | "descripcioncuenta" | "titulocuenta" => &mut c.name,
            "saldodeudor" | "deudor" | "saldodebe" | "saldod" | "saldosdeudores" => {
                &mut c.debit_balance
            }
            "saldoacreedor" | "acreedor" | "saldohaber" | "saldoa" | "saldosacreedores" => {
                &mut c.credit_balance
            }
            "saldo" | "saldofinal" | "saldoactual" => &mut c.balance,
            "fecha" | "fechaasiento" | "fechaapunte" => &mut c.date,
            "debe" | "sumadebe" | "sumasdebe" | "totaldebe" | "sumasdeudoras" => &mut c.debit,
            "haber" | "sumahaber" | "sumashaber" | "totalhaber" | "sumasacreedoras" => {
                &mut c.credit
            }
            _ => continue,
        };
        if slot.is_none() {
            *slot = Some(i);
        }
    }
    c
}

/// Quita las cuentas que son la suma de otras de la lista (los listados traen el grupo, la
/// cuenta y la subcuenta: 4, 43, 430, 4300001…), para no contar el mismo saldo varias veces.
fn leaves(lines: Vec<Line>) -> Vec<Line> {
    let codes: Vec<String> = lines.iter().map(|l| l.account.clone()).collect();
    lines
        .into_iter()
        .filter(|l| {
            !codes
                .iter()
                .any(|c| c.len() > l.account.len() && c.starts_with(&l.account))
        })
        .collect()
}

/// Balance de sumas y saldos en una hoja: una fila por cuenta con su saldo.
pub(crate) fn parse_trial_balance(rows: &[Vec<String>]) -> Option<(Vec<Line>, Vec<String>)> {
    let (head, cols) = rows
        .iter()
        .enumerate()
        .take(25)
        .map(|(i, r)| (i, balance_cols(r)))
        .find(|(_, c)| {
            c.account.is_some()
                && ((c.debit_balance.is_some() && c.credit_balance.is_some())
                    || c.balance.is_some()
                    || (c.debit.is_some() && c.credit.is_some() && c.date.is_none()))
        })?;
    let cell = |r: &[String], c: Option<usize>| -> String {
        c.and_then(|i| r.get(i))
            .map(|s| s.trim().to_owned())
            .unwrap_or_default()
    };
    let mut warnings = Vec::new();
    let mut found: BTreeMap<String, Line> = BTreeMap::new();
    for (i, r) in rows.iter().enumerate().skip(head + 1) {
        let raw: String = cell(r, cols.account)
            .chars()
            .filter(|c| !matches!(c, '.' | ' ' | '-'))
            .collect();
        if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
            continue; // títulos, totales
        }
        let Ok(account) = AccountCode::new(&raw) else {
            continue; // grupos de 1 o 2 dígitos: los da la suma de sus cuentas
        };
        let amount = |c: Option<usize>| parse_amount(&cell(r, c));
        let net = if cols.debit_balance.is_some() && cols.credit_balance.is_some() {
            amount(cols.debit_balance)
                .zip(amount(cols.credit_balance))
                .map(|(d, c)| d - c)
        } else if cols.balance.is_some() {
            amount(cols.balance)
        } else {
            amount(cols.debit)
                .zip(amount(cols.credit))
                .map(|(d, c)| d - c)
        };
        let Some(net) = net else {
            warnings.push(format!(
                "fila {}: el importe de la cuenta {} no se entiende; no entra",
                i + 1,
                account.as_str()
            ));
            continue;
        };
        let e = found.entry(account.as_str().to_owned()).or_insert(Line {
            account: account.as_str().to_owned(),
            name: cell(r, cols.name),
            ..Line::default()
        });
        let total = e.debit - e.credit + net;
        (e.debit, e.credit) = split(total);
    }
    if found.is_empty() {
        return None;
    }
    let lines = leaves(found.into_values().collect())
        .into_iter()
        .filter(|l| !(l.debit - l.credit).is_zero())
        .collect();
    Some((lines, warnings))
}

/// Saldo deudor positivo → (debe, haber).
fn split(net: Money) -> (Money, Money) {
    if net.is_negative() {
        (Money::ZERO, -net)
    } else {
        (net, Money::ZERO)
    }
}

fn lines_of(
    entries: &[&JournalEntry],
    names: &HashMap<String, String>,
    inverse: bool,
) -> Vec<Line> {
    let mut net: BTreeMap<String, Money> = BTreeMap::new();
    for e in entries {
        for l in &e.lines {
            *net.entry(l.account.as_str().to_owned()).or_default() += l.debit - l.credit;
        }
    }
    net.into_iter()
        .filter(|(_, m)| !m.is_zero())
        .map(|(account, m)| {
            let (debit, credit) = split(if inverse { -m } else { m });
            Line {
                name: names.get(&account).cloned().unwrap_or_default(),
                account,
                debit,
                credit,
            }
        })
        .collect()
}

/// Lee el balance del gestor: un balance de sumas y saldos (Excel, OpenDocument o CSV) o su
/// diario (SUENLACE de a3, Excel o CSV), del que se toma el asiento de apertura, el inverso del
/// de cierre o, si no hay ninguno, los saldos de todo el diario.
pub fn read_balance(filename: &str, bytes: &[u8]) -> Result<BalanceRead> {
    if let Ok(hojas) = crate::import::sheets(filename, bytes) {
        for h in &hojas {
            if let Some((lines, warnings)) = parse_trial_balance(&h.filas) {
                return Ok(BalanceRead {
                    kind: "sumas_saldos".into(),
                    lines,
                    date: None,
                    warnings,
                });
            }
        }
    }
    let (entries, accounts, mut warnings) =
        crate::import::read_journal(filename, bytes).map_err(|e| {
            other(format!(
                "no lo reconozco como balance de sumas y saldos (columnas Cuenta y Saldo, o Saldo \
                 deudor y Saldo acreedor) ni como diario: {e}"
            ))
        })?;
    let names: HashMap<String, String> = accounts
        .into_iter()
        .map(|(c, n)| (c.as_str().to_owned(), n))
        .collect();
    let concept = |e: &JournalEntry| norm(&e.concept);
    if let Some(a) = entries.iter().find(|e| concept(e).contains("apertura")) {
        return Ok(BalanceRead {
            kind: "apertura".into(),
            lines: lines_of(&[a], &names, false),
            date: Some(a.date),
            warnings,
        });
    }
    if let Some(c) = entries.iter().rev().find(|e| concept(e).contains("cierre")) {
        return Ok(BalanceRead {
            kind: "cierre".into(),
            lines: lines_of(&[c], &names, true),
            date: Some(c.date + Duration::days(1)),
            warnings,
        });
    }
    let all: Vec<&JournalEntry> = entries.iter().collect();
    let last = entries.iter().map(|e| e.date).max();
    let lines = lines_of(&all, &names, false);
    if lines
        .iter()
        .any(|l| l.account.starts_with('6') || l.account.starts_with('7'))
    {
        warnings.push(
            "son los saldos de todo el diario y llevan gastos e ingresos (grupos 6 y 7): si \
             abres a principio de ejercicio, pásalos a la 129 (resultado) o importa el diario \
             con su cierre"
                .into(),
        );
    }
    Ok(BalanceRead {
        kind: "diario".into(),
        lines,
        date: last.map(|d| d + Duration::days(1)),
        warnings,
    })
}

// ---------------- Leer pendientes de cobro o pago ----------------

/// Pendientes leídos de un fichero.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PendingRead {
    pub items: Vec<Pending>,
    /// Vienen de un libro registro: son todas las facturas, no sólo las pendientes.
    pub from_book: bool,
    pub warnings: Vec<String>,
}

#[derive(Default)]
struct PendingCols {
    name: Option<usize>,
    tax_id: Option<usize>,
    account: Option<usize>,
    number: Option<usize>,
    date: Option<usize>,
    due: Option<usize>,
    amount: Option<usize>,
}

fn pending_cols(row: &[String]) -> PendingCols {
    let mut c = PendingCols::default();
    for (i, h) in row.iter().enumerate() {
        let n = norm(h);
        let slot =
            match n.as_str() {
                "cliente" | "proveedor" | "acreedor" | "nombre" | "razonsocial" | "tercero"
                | "titular" | "nombrecliente" | "nombreproveedor" => &mut c.name,
                "nif" | "cif" | "dni" | "nifcif" | "cifnif" => &mut c.tax_id,
                "cuenta" | "subcuenta" => &mut c.account,
                "factura" | "numero" | "documento" | "numfactura" | "nfactura"
                | "numerofactura" => &mut c.number,
                "fecha" | "fechafactura" | "fechaemision" | "fechaexpedicion" => &mut c.date,
                "vencimiento" | "fechavencimiento" | "vto" | "vencim" => &mut c.due,
                "importe" | "pendiente" | "importependiente" | "saldo" | "saldopendiente"
                | "total" => &mut c.amount,
                _ => continue,
            };
        if slot.is_none() {
            *slot = Some(i);
        }
    }
    c
}

/// Listado de cartera (vencimientos pendientes) del programa anterior.
fn parse_pending_table(rows: &[Vec<String>]) -> Option<(Vec<Pending>, Vec<String>)> {
    let (head, cols) = rows
        .iter()
        .enumerate()
        .take(25)
        .map(|(i, r)| (i, pending_cols(r)))
        .find(|(_, c)| {
            (c.name.is_some() || c.account.is_some())
                && c.amount.is_some()
                && c.due.is_some()
                && (c.number.is_some() || c.date.is_some())
        })?;
    let cell = |r: &[String], c: Option<usize>| -> String {
        c.and_then(|i| r.get(i))
            .map(|s| s.trim().to_owned())
            .unwrap_or_default()
    };
    let mut items = Vec::new();
    let mut warnings = Vec::new();
    for (i, r) in rows.iter().enumerate().skip(head + 1) {
        let amount = parse_amount(&cell(r, cols.amount)).unwrap_or(Money::ZERO);
        let due = parse_date(&cell(r, cols.due));
        let date = parse_date(&cell(r, cols.date));
        let name = cell(r, cols.name);
        let account = Some(cell(r, cols.account)).filter(|a| AccountCode::new(a).is_ok());
        if amount.is_zero() || (name.is_empty() && account.is_none()) {
            continue; // vacías, totales
        }
        let Some(due) = due.or(date) else {
            // Una fila de totales («TOTAL», «Total proveedores») sin fechas: fuera sin avisar.
            if !r.iter().any(|c| norm(c).starts_with("total")) {
                warnings.push(format!("fila {}: sin fecha; no entra", i + 1));
            }
            continue;
        };
        items.push(Pending {
            account,
            tax_id: Some(cell(r, cols.tax_id)).filter(|t| !t.is_empty()),
            name,
            number: cell(r, cols.number),
            issue_date: date.unwrap_or(due),
            due_date: due,
            amount,
        });
    }
    (!items.is_empty()).then_some((items, warnings))
}

/// Lee los pendientes de cobro (`emitida`) o de pago (`recibida`): un listado de cartera con
/// columnas como Cliente, NIF, Factura, Fecha, Vencimiento e Importe, o un libro registro de
/// facturas del gestor (entonces salen todas y hay que dejar las que siguen pendientes).
pub fn read_pending(v: &Vault, kind: &str, filename: &str, bytes: &[u8]) -> Result<PendingRead> {
    if !matches!(kind, "emitida" | "recibida") {
        return Err(other("tipo de pendiente no válido"));
    }
    let hojas = crate::import::sheets(filename, bytes)?;
    for h in &hojas {
        if let Some((items, warnings)) = parse_pending_table(&h.filas) {
            return Ok(PendingRead {
                items,
                from_book: false,
                warnings,
            });
        }
    }
    let book = crate::libros::parse_hojas(filename, &hojas).ok_or_else(|| {
        other(
            "no lo reconozco: hace falta un listado con columnas Cliente o Proveedor, Factura, \
             Vencimiento e Importe, o un libro registro de facturas",
        )
    })?;
    let mut warnings = book.warnings;
    let wanted = if kind == "emitida" {
        crate::libros::BookKind::Emitidas
    } else {
        crate::libros::BookKind::Recibidas
    };
    if book.kind != wanted {
        warnings.push(if kind == "emitida" {
            "parece un libro de facturas recibidas, no de emitidas".into()
        } else {
            "parece un libro de facturas emitidas, no de recibidas".into()
        });
    }
    let terms = crate::cartera::terms(v)?;
    let days = i64::from(if kind == "emitida" {
        terms.customers_days
    } else {
        terms.suppliers_days
    });
    let items = book
        .invoices
        .into_iter()
        .map(|i| Pending {
            account: None,
            amount: i.base() + i.vat() + i.surcharge() - i.withholding,
            tax_id: i.nif.clone(),
            name: i.name.clone(),
            number: i.number.clone(),
            issue_date: i.issue_date,
            due_date: i.issue_date + Duration::days(days),
        })
        .filter(|p| !p.amount.is_zero())
        .collect();
    Ok(PendingRead {
        items,
        from_book: true,
        warnings,
    })
}

// ---------------- Bancos ----------------

/// Cuenta de banco de TaxPain, con el saldo que dicen sus extractos el día anterior a la
/// apertura (si se importaron).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BankInfo {
    pub account: String,
    pub name: String,
    pub iban: Option<String>,
    pub currency: String,
    pub kind: String,
    pub statement_balance: Option<Money>,
}

pub fn banks(v: &Vault, date: Option<Date>) -> Result<Vec<BankInfo>> {
    let conn = v.connection();
    let mut st = conn.prepare(
        "SELECT b.account, b.name, b.iban, b.currency, b.kind,
                (SELECT t.balance FROM bank_tx t WHERE t.bank_account = b.id
                   AND t.balance IS NOT NULL AND (?1 IS NULL OR t.op_date < ?1)
                 ORDER BY t.op_date DESC, t.id DESC LIMIT 1)
           FROM bank_account b WHERE b.account <> '' ORDER BY b.account, b.id",
    )?;
    let rows = st.query_map([date.map(|d| d.to_string())], |r| {
        Ok(BankInfo {
            account: r.get(0)?,
            name: r.get(1)?,
            iban: r.get(2)?,
            currency: r.get(3)?,
            kind: r.get(4)?,
            statement_balance: r.get::<_, Option<i64>>(5)?.map(Money::from_cents),
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

// ---------------- Resumen y comprobaciones ----------------

/// Una línea del asiento de apertura que saldría.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SummaryLine {
    pub account: String,
    pub name: String,
    pub debit: Money,
    pub credit: Money,
    /// De qué paso sale: `balance`, `clientes`, `proveedores`, `inmovilizado`, `hacienda` o
    /// `bancos`.
    pub from: String,
    /// Subcuenta nueva de un cliente o proveedor: se crea al contabilizar (el número es el de
    /// la cuenta principal).
    pub new_account: bool,
}

/// Lo que se contabilizaría y lo que hay que revisar antes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub fiscal_year: Option<i32>,
    pub fiscal_year_label: Option<String>,
    pub fiscal_year_start: Option<Date>,
    pub lines: Vec<SummaryLine>,
    pub debit: Money,
    pub credit: Money,
    /// Debe − haber: tiene que ser cero.
    pub difference: Money,
    /// Lo que impide contabilizar.
    pub errors: Vec<String>,
    /// Lo que conviene mirar.
    pub warnings: Vec<String>,
    /// Ya hay un asiento de apertura en el ejercicio: hay que confirmarlo para hacer otro.
    pub duplicate: Option<String>,
    /// Lo que se hará además del asiento (p. ej. anotar la cuota a compensar del 303).
    #[serde(default)]
    pub also: Vec<String>,
}

/// Cómo se busca la subcuenta de un cliente o proveedor sin cuenta puesta: sólo mirando (el
/// resumen) o creándola si hace falta (al contabilizar).
type Resolver<'a> = dyn FnMut(&Pending, &str) -> Result<Option<String>> + 'a;

fn mapped_account(conn: &Connection, p: &Pending, role: &str) -> Result<Option<String>> {
    let Some(t) = p.tax_id.as_deref().filter(|t| !t.trim().is_empty()) else {
        return Ok(None);
    };
    Ok(conn
        .query_row(
            "SELECT account FROM third_party_account WHERE tax_id = ?1 AND role = ?2",
            params![t.trim(), role],
            |r| r.get(0),
        )
        .optional()?)
}

/// ¿La cuenta `a` del balance es la `c` o una de sus subcuentas (o al revés)?
fn matches(a: &str, c: &str) -> bool {
    a.starts_with(c) || c.starts_with(a)
}

struct Plan {
    lines: Vec<SummaryLine>,
    errors: Vec<String>,
    warnings: Vec<String>,
    /// Subcuenta de cada pendiente (clientes y luego proveedores), si se sabe.
    customers: Vec<Option<String>>,
    suppliers: Vec<Option<String>>,
}

fn plan(conn: &Connection, d: &Draft, resolve: &mut Resolver<'_>) -> Result<Plan> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut lines: Vec<SummaryLine> = Vec::new();
    let date = d.date;
    // Balance.
    for (i, l) in d.lines.iter().enumerate() {
        if AccountCode::new(&l.account).is_err() {
            errors.push(format!(
                "balance, línea {}: la cuenta «{}» no es válida (sólo números, de 3 a 12 cifras)",
                i + 1,
                l.account
            ));
            continue;
        }
        if l.debit.is_negative() || l.credit.is_negative() {
            errors.push(format!(
                "balance, cuenta {}: pon el importe en el debe o en el haber, sin signo menos",
                l.account
            ));
        }
        if (l.debit - l.credit).is_zero() {
            continue;
        }
        lines.push(SummaryLine {
            account: l.account.clone(),
            name: l.name.clone(),
            debit: l.debit,
            credit: l.credit,
            from: "balance".into(),
            new_account: false,
        });
    }
    let balance: Vec<SummaryLine> = lines.clone();
    let in_balance = |c: &str| -> Option<Money> {
        let hits: Vec<&SummaryLine> = balance.iter().filter(|l| matches(&l.account, c)).collect();
        (!hits.is_empty()).then(|| hits.iter().map(|l| l.debit - l.credit).sum())
    };
    let group_in_balance = |prefixes: &[&str]| {
        balance
            .iter()
            .any(|l| prefixes.iter().any(|p| l.account.starts_with(p)))
    };
    let pa = accounting::posting_accounts(conn)?;

    // Clientes y proveedores pendientes.
    let mut resolved: [Vec<Option<String>>; 2] = [Vec::new(), Vec::new()];
    for (side, (items, role, from, prefixes, base)) in [
        (
            &d.customers,
            "cliente",
            "clientes",
            &["43"][..],
            &pa.customers,
        ),
        (
            &d.suppliers,
            "proveedor",
            "proveedores",
            &["40", "41"][..],
            &pa.suppliers,
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let collect = side == 0;
        let balance_rules = group_in_balance(prefixes);
        let mut per_account: BTreeMap<String, Money> = BTreeMap::new();
        let mut missing = Vec::new();
        for p in items.iter() {
            let label = if p.number.trim().is_empty() {
                p.name.clone()
            } else {
                format!("{} ({})", p.number.trim(), p.name)
            };
            if p.name.trim().is_empty() && p.account.is_none() {
                errors.push(format!("{from}: falta el nombre de {label}"));
            }
            if p.amount.is_zero() {
                errors.push(format!("{from}: {label} no tiene importe"));
            }
            if date.is_some_and(|dt| p.issue_date >= dt) {
                errors.push(format!(
                    "{from}: {label} es del {}, no anterior a la apertura: regístrala como factura \
                     en TaxPain",
                    dmy(p.issue_date)
                ));
            }
            if p.due_date < p.issue_date {
                warnings.push(format!("{from}: {label} vence antes de su fecha"));
            }
            let explicit = p
                .account
                .as_deref()
                .map(str::trim)
                .filter(|a| !a.is_empty());
            let account = match explicit {
                Some(a) => {
                    if AccountCode::new(a).is_err()
                        || !prefixes.iter().any(|pre| a.starts_with(pre))
                    {
                        errors.push(format!(
                            "{from}: la cuenta {a} de {label} no es de {}",
                            if collect {
                                "clientes (43x)"
                            } else {
                                "proveedores o acreedores (40x o 41x)"
                            }
                        ));
                    }
                    Some(a.to_owned())
                }
                None => resolve(p, role)?,
            };
            match &account {
                Some(a) => *per_account.entry(a.clone()).or_default() += p.amount,
                None if balance_rules => missing.push(label),
                None => lines.push(SummaryLine {
                    account: base.as_str().to_owned(),
                    name: format!("{} (subcuenta nueva)", p.name.trim()),
                    debit: if collect { p.amount } else { Money::ZERO },
                    credit: if collect { Money::ZERO } else { p.amount },
                    from: from.into(),
                    new_account: true,
                }),
            }
            resolved[side].push(account);
        }
        if !missing.is_empty() {
            errors.push(format!(
                "{from}: el balance ya trae sus subcuentas; di cuál es la de {}",
                missing.join(", ")
            ));
        }
        for (account, amount) in per_account {
            // Saldo natural: deudor en clientes, acreedor en proveedores.
            let natural = |m: Money| if collect { m } else { -m };
            match in_balance(&account) {
                Some(b) if natural(b) != amount => warnings.push(format!(
                    "{from}: en el balance la {account} tiene {} y lo pendiente suma {amount}",
                    natural(b)
                )),
                Some(_) => {}
                None if balance_rules => warnings.push(format!(
                    "{from}: la {account} no está en el balance; sus pendientes ({amount}) no \
                     entran en el asiento"
                )),
                None => {
                    let (debit, credit) = split(natural(amount));
                    lines.push(SummaryLine {
                        name: items
                            .iter()
                            .zip(&resolved[side])
                            .find(|(_, a)| a.as_deref() == Some(account.as_str()))
                            .map(|(p, _)| p.name.clone())
                            .unwrap_or_default(),
                        account,
                        debit,
                        credit,
                        from: from.into(),
                        new_account: false,
                    });
                }
            }
        }
    }

    // Inmovilizado: coste al debe y amortización acumulada al haber.
    let mut costs: BTreeMap<String, Money> = BTreeMap::new();
    let mut accumulated: BTreeMap<String, Money> = BTreeMap::new();
    for a in &d.assets {
        let label = &a.name;
        if a.name.trim().is_empty() {
            errors.push("inmovilizado: hay un elemento sin nombre".into());
        }
        let ok_codes = [
            (
                &a.account,
                &["20", "21", "22"][..],
                "de inmovilizado (20x, 21x o 22x)",
            ),
            (
                &a.depreciation_account,
                &["28"][..],
                "de amortización acumulada (28x)",
            ),
            (
                &a.expense_account,
                &["68"][..],
                "de dotación a la amortización (68x)",
            ),
        ];
        for (c, prefixes, what) in ok_codes {
            if AccountCode::new(c).is_err() || !prefixes.iter().any(|p| c.starts_with(p)) {
                errors.push(format!(
                    "inmovilizado, {label}: la cuenta «{c}» no es {what}"
                ));
            }
        }
        if a.cost.cents() <= 0 {
            errors.push(format!("inmovilizado, {label}: falta el precio de compra"));
        }
        if a.accumulated.is_negative() || a.accumulated > a.cost {
            errors.push(format!(
                "inmovilizado, {label}: lo ya amortizado no puede pasar del precio de compra"
            ));
        }
        if a.rate.bp() <= 0 || a.rate.bp() > 10_000 {
            errors.push(format!(
                "inmovilizado, {label}: el coeficiente ha de estar entre 0 y 100 %"
            ));
        }
        if date.is_some_and(|dt| a.acquired_on >= dt) {
            errors.push(format!(
                "inmovilizado, {label}: se compró el {}, no antes de la apertura: dalo de alta \
                 en Inmovilizado o con su factura recibida",
                dmy(a.acquired_on)
            ));
        }
        *costs.entry(a.account.clone()).or_default() += a.cost;
        if !a.accumulated.is_zero() {
            *accumulated
                .entry(a.depreciation_account.clone())
                .or_default() += a.accumulated;
        }
    }
    for (map, debit_side, what) in [
        (&costs, true, "el precio de compra"),
        (&accumulated, false, "lo ya amortizado"),
    ] {
        for (account, amount) in map {
            let natural = |m: Money| if debit_side { m } else { -m };
            match in_balance(account) {
                Some(b) if natural(b) != *amount => warnings.push(format!(
                    "inmovilizado: en el balance la {account} tiene {} y {what} de los elementos \
                     suma {amount}; si falta alguno, añádelo para que se amortice",
                    natural(b)
                )),
                Some(_) => {}
                None => {
                    let (debit, credit) = split(natural(*amount));
                    lines.push(SummaryLine {
                        account: account.clone(),
                        name: String::new(),
                        debit,
                        credit,
                        from: "inmovilizado".into(),
                        new_account: false,
                    });
                }
            }
        }
    }

    // Hacienda: 4700 deudora; 4750 y 4751 acreedoras.
    for (account, amount, debit_side, name) in [
        (
            "4700",
            d.tax.vat_refund,
            true,
            "Hacienda Pública, deudora por IVA",
        ),
        (
            "4750",
            d.tax.vat_due,
            false,
            "Hacienda Pública, acreedora por IVA",
        ),
        (
            "4751",
            d.tax.withholdings,
            false,
            "Hacienda Pública, acreedora por retenciones practicadas",
        ),
    ] {
        if amount.is_negative() {
            errors.push(format!(
                "Hacienda: el saldo de la {account} no puede ser negativo"
            ));
        }
        if amount.is_zero() {
            continue;
        }
        let natural = |m: Money| if debit_side { m } else { -m };
        match in_balance(account) {
            Some(b) if natural(b) != amount => warnings.push(format!(
                "Hacienda: en el balance la {account} tiene {} y has puesto {amount}",
                natural(b)
            )),
            Some(_) => {}
            None => {
                let (debit, credit) = split(natural(amount));
                lines.push(SummaryLine {
                    account: account.into(),
                    name: name.into(),
                    debit,
                    credit,
                    from: "hacienda".into(),
                    new_account: false,
                });
            }
        }
    }

    // Bancos: saldo a favor al debe (un descubierto o una tarjeta de crédito, al haber).
    let banks_in_balance = group_in_balance(&["57"]);
    for b in &d.banks {
        if b.balance.is_zero() {
            continue;
        }
        if AccountCode::new(&b.account).is_err() {
            errors.push(format!("bancos: la cuenta «{}» no es válida", b.account));
            continue;
        }
        let exact: Vec<&SummaryLine> = balance.iter().filter(|l| l.account == b.account).collect();
        if !exact.is_empty() {
            let bal: Money = exact.iter().map(|l| l.debit - l.credit).sum();
            if bal != b.balance {
                warnings.push(format!(
                    "bancos: en el balance la {} tiene {bal} y has puesto {}",
                    b.account, b.balance
                ));
            }
        } else if banks_in_balance && b.account.starts_with("57") {
            warnings.push(format!(
                "bancos: tu banco {} es la {} en TaxPain y el balance no la trae (trae otras \
                 57x): cambia en el balance la cuenta del gestor por la {} para que sus \
                 movimientos cuadren",
                b.name, b.account, b.account
            ));
        } else {
            let (debit, credit) = split(b.balance);
            lines.push(SummaryLine {
                account: b.account.clone(),
                name: b.name.clone(),
                debit,
                credit,
                from: "bancos".into(),
                new_account: false,
            });
        }
    }
    let [customers, suppliers] = resolved;
    Ok(Plan {
        lines,
        errors,
        warnings,
        customers,
        suppliers,
    })
}

/// Asientos de apertura que ya hay en el ejercicio `year` (los del cierre del anterior, los de
/// este asistente o uno importado del diario del gestor que se llame «apertura»).
fn existing_openings(conn: &Connection, year: i32) -> Result<Vec<String>> {
    let mut st = conn.prepare(
        "SELECT e.number, e.date, e.origin_kind,
                EXISTS (SELECT 1 FROM opening_balance o WHERE o.entry_id = e.id)
           FROM journal_entry e
          WHERE e.fiscal_year = ?1
            AND (e.origin_kind = 'Apertura'
                 OR (e.origin_kind = 'Importado' AND lower(e.concept) LIKE '%apertura%'))
          ORDER BY e.number",
    )?;
    let rows = st.query_map([year], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, bool>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (n, date, kind, ours) = row?;
        let date = Date::parse(
            &date,
            time::macros::format_description!("[year]-[month]-[day]"),
        )
        .map(dmy)
        .unwrap_or(date);
        let what = if ours {
            "hecho con este asistente"
        } else if kind == "Importado" {
            "importado del diario del gestor"
        } else {
            "del cierre del ejercicio anterior"
        };
        out.push(format!("nº {n} del {date}, {what}"));
    }
    Ok(out)
}

fn summarize(v: &Vault, d: &Draft, plan: Plan) -> Result<Summary> {
    let conn = v.connection();
    let Plan {
        lines,
        mut errors,
        mut warnings,
        ..
    } = plan;
    let debit: Money = lines.iter().map(|l| l.debit).sum();
    let credit: Money = lines.iter().map(|l| l.credit).sum();
    let difference = debit - credit;
    let mut s = Summary {
        fiscal_year: None,
        fiscal_year_label: None,
        fiscal_year_start: None,
        lines,
        debit,
        credit,
        difference,
        errors: Vec::new(),
        warnings: Vec::new(),
        duplicate: None,
        also: Vec::new(),
    };
    let Some(date) = d.date else {
        errors.insert(0, "elige la fecha de los saldos (paso 1)".into());
        s.errors = errors;
        s.warnings = warnings;
        return Ok(s);
    };
    let fy = accounting::fiscal_year_at(v, date)?;
    s.fiscal_year = Some(fy.year);
    s.fiscal_year_label = Some(fy.label());
    s.fiscal_year_start = Some(fy.start);
    let closed: bool = conn
        .query_row(
            "SELECT closed FROM fiscal_year WHERE year = ?1",
            [fy.year],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        == Some(1);
    if closed {
        errors.push(format!(
            "el ejercicio {} está cerrado: no se le pueden añadir asientos",
            fy.label()
        ));
    }
    if s.lines.is_empty() {
        errors.push("no hay ningún saldo: importa el balance o pon las cuentas a mano".into());
    }
    if !difference.is_zero() {
        errors.push(format!(
            "no cuadra: el debe suma {debit} y el haber {credit} (diferencia {}); revisa el \
             balance, que suele faltar el capital (100), las reservas (113) o el resultado (129 \
             o 120/121)",
            difference.abs()
        ));
    }
    if date == fy.start
        && s.lines
            .iter()
            .any(|l| l.account.starts_with('6') || l.account.starts_with('7'))
    {
        warnings.push(
            "la apertura es el primer día del ejercicio y hay cuentas de gastos o ingresos \
             (grupos 6 y 7): un balance de apertura no las lleva; su saldo va a la 129 \
             (resultado del ejercicio anterior)"
                .into(),
        );
    }
    if date != fy.start {
        warnings.push(format!(
            "la apertura no es el primer día del ejercicio ({}): lo de antes del {} (gastos, \
             ingresos, IVA) tiene que estar en estos saldos o en el diario importado",
            dmy(fy.start),
            dmy(date)
        ));
    }
    let before: i64 = conn.query_row(
        "SELECT count(*) FROM journal_entry WHERE fiscal_year = ?1 AND date < ?2",
        params![fy.year, date.to_string()],
        |r| r.get(0),
    )?;
    if before > 0 {
        warnings.push(format!(
            "ya hay {before} asiento(s) en {} antes del {}: si son del gestor, sus saldos ya \
             están y se contarían dos veces",
            fy.label(),
            dmy(date)
        ));
    }
    match compensation(v, date, &fy, &s.lines)? {
        Compensation::None => {}
        Compensation::Set { year, amount } => s.also.push(format!(
            "la 4700 ({amount}) se anotará en Modelos › «Datos de los modelos» como IVA a \
             compensar al empezar {year}: el 303 del 1T la descontará en la casilla [110]. Si en \
             el último 303 pediste la devolución en vez de compensar, quítala de ahí"
        )),
        Compensation::Keep(w) => warnings.push(w),
    }
    let existing = existing_openings(conn, fy.year)?;
    if !existing.is_empty() {
        s.duplicate = Some(format!(
            "ya hay un asiento de apertura en {}: {}",
            fy.label(),
            existing.join("; ")
        ));
    }
    s.errors = errors;
    s.warnings = warnings;
    Ok(s)
}

/// Qué pasa con la 4700 de la apertura en el 303 (la cuota a compensar de la casilla [110]).
enum Compensation {
    None,
    /// Se anota en los ajustes de los modelos como lo pendiente al empezar `year`.
    Set {
        year: i32,
        amount: Money,
    },
    /// No se toca (ya hay otra cifra o la apertura no es el 1 de enero): el aviso.
    Keep(String),
}

/// El saldo deudor de la 4700 en una apertura del 1 de enero es lo que quedaba a compensar del
/// 4T anterior (art. 99.Cinco LIVA): el 303 del 1T lo lee de `vat_compensation_carried`. Sólo
/// se anota si los ajustes están vacíos; si ya tienen otra cifra, se avisa y no se pisa.
fn compensation(
    v: &Vault,
    date: Date,
    fy: &FiscalYear,
    lines: &[SummaryLine],
) -> Result<Compensation> {
    let amount: Money = lines
        .iter()
        .filter(|l| l.account.starts_with("4700"))
        .map(|l| l.debit - l.credit)
        .sum();
    if amount.cents() <= 0 {
        return Ok(Compensation::None);
    }
    if date != fy.start || date.ordinal() != 1 {
        return Ok(Compensation::Keep(format!(
            "Hacienda: la apertura no es el 1 de enero, así que la 4700 ({amount}) no se pasa \
             sola al 303; si es IVA a compensar, ponlo en Modelos › «Datos de los \
             modelos»"
        )));
    }
    let year = date.year();
    let s = crate::models::settings(v)?;
    if s.vat_compensation_year == Some(year) && s.vat_compensation_carried == amount {
        return Ok(Compensation::None);
    }
    if s.vat_compensation_carried.is_zero() && s.vat_compensation_year.is_none() {
        return Ok(Compensation::Set { year, amount });
    }
    let since = s
        .vat_compensation_year
        .map(|y| format!(" al empezar {y}"))
        .unwrap_or_default();
    Ok(Compensation::Keep(format!(
        "Hacienda: en Modelos › «Datos de los modelos» ya hay {} a compensar{since} y no se \
         cambia; la 4700 de la apertura es {amount}: revisa cuál vale para la casilla [110] \
         del 303 del 1T",
        s.vat_compensation_carried
    )))
}

/// Lo anotado por una apertura en los ajustes de los modelos (para deshacerlo).
#[derive(Serialize, Deserialize)]
struct CompensationSet {
    opening: i64,
    year: i32,
    amount: Money,
}

/// Lo que se contabilizaría con el borrador y lo que hay que revisar (no cambia nada).
pub fn summary(v: &Vault, d: &Draft) -> Result<Summary> {
    let conn = v.connection();
    let p = plan(conn, d, &mut |p, role| mapped_account(conn, p, role))?;
    summarize(v, d, p)
}

// ---------------- Contabilizar y deshacer ----------------

/// Apertura hecha con el asistente.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Applied {
    pub id: i64,
    pub fiscal_year: i32,
    pub date: Date,
    pub entry_id: Option<i64>,
    pub entry_number: Option<i64>,
    pub pending: i64,
    pub assets: i64,
    pub created_at: String,
}

/// Contabiliza el asiento de apertura del borrador y da de alta los pendientes (en la cartera)
/// y el inmovilizado (con su amortización anterior). Si ya hay otro asiento de apertura en el
/// ejercicio, sólo con `allow_duplicate`. El borrador se borra (queda una copia para deshacer).
pub fn apply(v: &Vault, d: &Draft, allow_duplicate: bool, now: OffsetDateTime) -> Result<Applied> {
    let conn = v.connection();
    let s = summary(v, d)?;
    if !s.errors.is_empty() {
        return Err(other(format!(
            "antes de contabilizar: {}",
            s.errors.join("; ")
        )));
    }
    if let Some(dup) = &s.duplicate
        && !allow_duplicate
    {
        return Err(other(format!(
            "{dup}. Otro asiento de apertura duplicaría los saldos: si de verdad lo quieres, \
             confírmalo"
        )));
    }
    let date = d.date.ok_or_else(|| other("falta la fecha"))?;
    let tx = conn.unchecked_transaction()?;
    let pa = accounting::posting_accounts(&tx)?;
    let mut create = |p: &Pending, role: &str| -> Result<Option<String>> {
        if let Some(a) = mapped_account(&tx, p, role)? {
            return Ok(Some(a));
        }
        let base = if role == "cliente" {
            &pa.customers
        } else {
            &pa.suppliers
        };
        let tax_id = p
            .tax_id
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map_or_else(|| format!("APERTURA:{}", p.name.trim()), str::to_owned);
        Ok(Some(
            accounting::third_party_account(
                &tx,
                &tax_id,
                role,
                p.name.trim(),
                base,
                pa.subaccount_width,
            )?
            .as_str()
            .to_owned(),
        ))
    };
    let p = plan(&tx, d, &mut create)?;
    tx.execute(
        "INSERT INTO opening_balance(fiscal_year, date, data, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![
            accounting::fiscal_year_for_date(&tx, date)?.year,
            date.to_string(),
            serde_json::to_string(d).map_err(|e| other(e.to_string()))?,
            now_str(now)
        ],
    )?;
    let opening = tx.last_insert_rowid();
    // Nombres de las cuentas del balance que aún no tiene el plan de cuentas.
    for l in d.lines.iter().filter(|l| !l.name.trim().is_empty()) {
        tx.execute(
            "INSERT OR IGNORE INTO account(code, name, created_at) VALUES (?1, ?2, ?3)",
            params![l.account, l.name.trim(), now_str(now)],
        )?;
    }
    let mut lines: Vec<EntryLine> = Vec::new();
    for l in &p.lines {
        lines.extend(EntryLine::signed(
            code(&l.account)?,
            l.debit - l.credit,
            true,
        ));
    }
    let entry = JournalEntry {
        date,
        concept: "Asiento de apertura (saldos iniciales)".into(),
        origin: Origin::Apertura,
        lines,
    }
    .compact();
    let entry_id = accounting::record_entry(&tx, &entry, now)?;
    tx.execute(
        "UPDATE opening_balance SET entry_id = ?1 WHERE id = ?2",
        params![entry_id, opening],
    )?;
    // Pendientes: a la cartera, con su vencimiento fijado.
    let mut pending = 0i64;
    for (items, accounts, kind, role) in [
        (&d.customers, &p.customers, "emitida", "cliente"),
        (&d.suppliers, &p.suppliers, "recibida", "proveedor"),
    ] {
        for (it, account) in items.iter().zip(accounts) {
            let account = account
                .clone()
                .ok_or_else(|| other(format!("falta la subcuenta de {}", it.name)))?;
            let tax_id = it
                .tax_id
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty());
            if let Some(t) = tax_id {
                // La subcuenta del gestor queda como la del tercero para sus próximas facturas.
                tx.execute(
                    "INSERT INTO third_party_account(tax_id, role, account, name)
                     SELECT ?1, ?2, ?3, ?4
                      WHERE NOT EXISTS (SELECT 1 FROM third_party_account
                                         WHERE (tax_id = ?1 AND role = ?2) OR account = ?3)",
                    params![t, role, account, it.name.trim()],
                )?;
            }
            tx.execute(
                "INSERT INTO opening_item(opening_id, kind, account, tax_id, name, number,
                                          issue_date, due_date, amount, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    opening,
                    kind,
                    account,
                    tax_id,
                    it.name.trim(),
                    it.number.trim(),
                    it.issue_date.to_string(),
                    it.due_date.to_string(),
                    it.amount.cents(),
                    now_str(now)
                ],
            )?;
            let id = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO invoice_installment(kind, invoice_id, seq, due_date, amount)
                 VALUES (?1, ?2, 1, ?3, ?4)",
                params![kind, -id, it.due_date.to_string(), it.amount.cents()],
            )?;
            pending += 1;
        }
    }
    // Inmovilizado con lo amortizado hasta el día anterior a la apertura.
    let until = date - Duration::days(1);
    for a in &d.assets {
        let id = insert_asset(v, a, now)?;
        tx.execute(
            "UPDATE fixed_asset SET prior_depreciation = ?1, prior_until = ?2, opening_id = ?3
              WHERE id = ?4",
            params![a.accumulated.cents(), until.to_string(), opening, id],
        )?;
    }
    // La 4700 del 1 de enero: cuota a compensar del 303 del 1T.
    let fy = accounting::fiscal_year_at(v, date)?;
    if let Compensation::Set { year, amount } = compensation(v, date, &fy, &p.lines)? {
        let mut ms = crate::models::settings(v)?;
        ms.vat_compensation_carried = amount;
        ms.vat_compensation_year = Some(year);
        crate::models::save_settings(v, &ms, now)?;
        v.set_setting(
            COMPENSATION_KEY,
            &CompensationSet {
                opening,
                year,
                amount,
            },
        )?;
    }
    tx.execute("DELETE FROM settings WHERE key = ?1", [DRAFT_KEY])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "apertura.contabilizar",
        &opening.to_string(),
        &serde_json::json!({
            "date": date.to_string(),
            "entry": entry_id,
            "pending": pending,
            "assets": d.assets.len(),
            "duplicate": s.duplicate.is_some(),
        }),
    )?;
    tx.commit()?;
    applied(v)?
        .into_iter()
        .find(|a| a.id == opening)
        .ok_or(AppError::NotFound)
}

fn insert_asset(v: &Vault, a: &Asset, now: OffsetDateTime) -> Result<i64> {
    accounting::add_fixed_asset(
        v,
        &FixedAsset {
            id: 0,
            name: a.name.trim().to_owned(),
            account: code(&a.account)?,
            depreciation_account: code(&a.depreciation_account)?,
            expense_account: code(&a.expense_account)?,
            acquired_on: a.acquired_on,
            cost: a.cost,
            residual: Money::ZERO,
            rate: a.rate,
            disposed_on: None,
            method: a.method,
            tax_rate: None,
        },
        now,
    )
}

/// Las aperturas hechas con el asistente.
pub fn applied(v: &Vault) -> Result<Vec<Applied>> {
    let conn = v.connection();
    let mut st = conn.prepare(
        "SELECT o.id, o.fiscal_year, o.date, o.entry_id, e.number,
                (SELECT count(*) FROM opening_item i WHERE i.opening_id = o.id),
                (SELECT count(*) FROM fixed_asset a WHERE a.opening_id = o.id),
                o.created_at
           FROM opening_balance o LEFT JOIN journal_entry e ON e.id = o.entry_id
          ORDER BY o.id",
    )?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i32>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<i64>>(3)?,
            r.get::<_, Option<i64>>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, i64>(6)?,
            r.get::<_, String>(7)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, fiscal_year, date, entry_id, entry_number, pending, assets, created_at) = row?;
        out.push(Applied {
            id,
            fiscal_year,
            date: parse_iso(&date)?,
            entry_id,
            entry_number,
            pending,
            assets,
            created_at,
        });
    }
    Ok(out)
}

fn parse_iso(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]"))
        .map_err(|e| other(e.to_string()))
}

/// Deshace una apertura del asistente: borra su asiento, sus pendientes y su inmovilizado y
/// vuelve a dejar el borrador para corregirlo. No vale si ya se cobró o pagó algún pendiente,
/// si algún elemento tiene amortizaciones o baja contabilizadas, o si el ejercicio está cerrado.
pub fn undo(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let (entry, data): (Option<i64>, String) = conn
        .query_row(
            "SELECT entry_id, data FROM opening_balance WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let used: Option<String> = conn
        .query_row(
            "SELECT i.number || ' (' || i.name || ')' FROM opening_item i
              WHERE i.opening_id = ?1
                AND (EXISTS (SELECT 1 FROM settlement s WHERE s.kind = i.kind AND s.invoice_id = -i.id)
                  OR EXISTS (SELECT 1 FROM bank_tx t WHERE t.matched_kind = i.kind AND t.matched_id = -i.id)
                  OR EXISTS (SELECT 1 FROM remittance_item r WHERE r.kind = i.kind AND r.invoice_id = -i.id))
              LIMIT 1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(u) = used {
        return Err(other(format!(
            "{u} ya tiene su cobro o pago registrado o conciliado: deshazlo antes"
        )));
    }
    let depreciated: Option<String> = conn
        .query_row(
            "SELECT a.name FROM fixed_asset a WHERE a.opening_id = ?1
                AND EXISTS (SELECT 1 FROM journal_entry e
                             WHERE e.origin_id = a.id
                               AND e.origin_kind IN ('Amortizacion', 'BajaInmovilizado'))
              LIMIT 1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(n) = depreciated {
        return Err(other(format!(
            "«{n}» ya tiene amortizaciones o su baja contabilizadas y esos asientos no se \
             pueden borrar, así que la apertura ya no se puede deshacer. Si algún saldo está \
             mal, corrígelo con un asiento manual en Contabilidad › Libro diario › «Nuevo \
             asiento»"
        )));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM invoice_installment
          WHERE EXISTS (SELECT 1 FROM opening_item i
                         WHERE i.opening_id = ?1 AND i.kind = invoice_installment.kind
                           AND -i.id = invoice_installment.invoice_id)",
        [id],
    )?;
    tx.execute("DELETE FROM opening_item WHERE opening_id = ?1", [id])?;
    tx.execute(
        "DELETE FROM iva_bien_inversion WHERE asset_id IN (SELECT id FROM fixed_asset WHERE opening_id = ?1)",
        [id],
    )?;
    tx.execute("DELETE FROM fixed_asset WHERE opening_id = ?1", [id])?;
    tx.execute(
        "UPDATE opening_balance SET entry_id = NULL WHERE id = ?1",
        [id],
    )?;
    if let Some(e) = entry {
        tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [e])
            .and_then(|_| tx.execute("DELETE FROM journal_entry WHERE id = ?1", [e]))
            .map_err(|err| {
                if err.to_string().contains("cerrado") {
                    other("el ejercicio de la apertura está cerrado: reábrelo antes")
                } else {
                    AppError::from(taxpain_vault::VaultError::from(err))
                }
            })?;
    }
    tx.execute("DELETE FROM opening_balance WHERE id = ?1", [id])?;
    // La cuota a compensar que anotó, si nadie la ha cambiado después.
    if let Some(c) = v.setting::<CompensationSet>(COMPENSATION_KEY)?
        && c.opening == id
    {
        let mut ms = crate::models::settings(v)?;
        if ms.vat_compensation_year == Some(c.year) && ms.vat_compensation_carried == c.amount {
            ms.vat_compensation_carried = Money::ZERO;
            ms.vat_compensation_year = None;
            crate::models::save_settings(v, &ms, now)?;
        }
        tx.execute("DELETE FROM settings WHERE key = ?1", [COMPENSATION_KEY])?;
    }
    tx.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![DRAFT_KEY, data],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "apertura.deshacer",
        &id.to_string(),
        &serde_json::json!({ "entry": entry }),
    )?;
    tx.commit()?;
    Ok(())
}

// ---------------- Cartera: los pendientes de antes de TaxPain ----------------

/// Pendiente de apertura para la cartera: (número, tercero, fecha, importe, subcuenta).
pub(crate) struct CarteraItem {
    pub kind: &'static str,
    /// Id de la cartera: el del pendiente con el signo cambiado.
    pub id: i64,
    pub number: String,
    pub party: String,
    pub issue: Date,
    pub amount: Money,
    pub account: String,
}

/// Los pendientes de apertura de la cartera (todos, o el `-id` de `only`).
pub(crate) fn cartera_items(
    conn: &Connection,
    only: Option<(&str, i64)>,
) -> Result<Vec<CarteraItem>> {
    let mut st = conn.prepare(
        "SELECT id, kind, number, name, issue_date, amount, account FROM opening_item
          WHERE (?1 IS NULL OR (kind = ?1 AND id = -?2)) ORDER BY id",
    )?;
    let (k, id) = match only {
        Some((k, id)) => (Some(k), Some(id)),
        None => (None, None),
    };
    let rows = st.query_map(params![k, id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, i64>(5)?,
            r.get::<_, String>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, kind, number, party, issue, amount, account) = row?;
        out.push(CarteraItem {
            kind: if kind == "emitida" {
                "emitida"
            } else {
                "recibida"
            },
            id: -id,
            number: if number.is_empty() {
                "Saldo inicial".into()
            } else {
                number
            },
            party,
            issue: parse_iso(&issue)?,
            amount: Money::from_cents(amount),
            account,
        });
    }
    Ok(out)
}

/// Subcuenta y concepto del cobro o pago de un pendiente de apertura (id de la cartera, < 0).
pub(crate) fn cartera_party(
    conn: &Connection,
    kind: &str,
    cartera_id: i64,
) -> Result<(AccountCode, String)> {
    let (account, number, name): (String, String, String) = conn
        .query_row(
            "SELECT account, number, name FROM opening_item WHERE kind = ?1 AND id = -?2",
            params![kind, cartera_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| other("ese pendiente de apertura no existe"))?;
    let what = if kind == "emitida" { "Cobro" } else { "Pago" };
    let doc = if number.is_empty() {
        "saldo inicial".to_owned()
    } else {
        format!("factura {number}")
    };
    Ok((code(&account)?, format!("{what} {doc} {name}")))
}

// ---------------- Inmovilizado: amortización anterior a TaxPain (CONTABILIDAD-05) ----------------

/// Lo amortizado de un elemento antes de TaxPain y el día hasta el que llega.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prior {
    pub id: i64,
    pub amount: Money,
    pub until: Option<Date>,
}

/// La amortización anterior de cada elemento que la tiene.
pub fn priors(v: &Vault) -> Result<Vec<Prior>> {
    let mut st = v.connection().prepare(
        "SELECT id, prior_depreciation, prior_until FROM fixed_asset
          WHERE prior_depreciation <> 0 OR prior_until IS NOT NULL ORDER BY id",
    )?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, amount, until) = row?;
        out.push(Prior {
            id,
            amount: Money::from_cents(amount),
            until: until.as_deref().map(parse_iso).transpose()?,
        });
    }
    Ok(out)
}

fn prior_of(conn: &Connection, id: i64) -> Result<(Money, Option<Date>)> {
    let row: Option<(i64, Option<String>)> = conn
        .query_row(
            "SELECT prior_depreciation, prior_until FROM fixed_asset WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (amount, until) = row.unwrap_or((0, None));
    Ok((
        Money::from_cents(amount),
        until.as_deref().map(parse_iso).transpose()?,
    ))
}

/// Alta de un elemento comprado antes de usar TaxPain con lo que ya se amortizó hasta `until`
/// (inclusive): la amortización de TaxPain empieza al día siguiente y nunca pasa del coste.
pub fn add_asset_with_prior(
    v: &Vault,
    a: &FixedAsset,
    prior: Money,
    until: Option<Date>,
    now: OffsetDateTime,
) -> Result<i64> {
    if prior.is_negative() || prior > a.cost - a.residual {
        return Err(other(
            "lo ya amortizado no puede ser negativo ni pasar del precio de compra",
        ));
    }
    if !prior.is_zero() && until.is_none() {
        return Err(other("di hasta qué día llega lo ya amortizado"));
    }
    if let Some(u) = until
        && u < a.acquired_on
    {
        return Err(other(
            "lo ya amortizado no puede llegar a un día anterior a la compra",
        ));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let id = accounting::add_fixed_asset(v, a, now)?;
    tx.execute(
        "UPDATE fixed_asset SET prior_depreciation = ?1, prior_until = ?2 WHERE id = ?3",
        params![prior.cents(), until.map(|d| d.to_string()), id],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "asset.prior",
        &a.name,
        &serde_json::json!({ "id": id, "prior": prior.cents(), "until": until.map(|d| d.to_string()) }),
    )?;
    tx.commit()?;
    Ok(id)
}

/// Periodo del ejercicio `fy` que amortiza TaxPain para el elemento y amortización acumulada al
/// empezarlo (la de TaxPain de ejercicios anteriores más la de antes de TaxPain). El periodo es
/// `None` si lo amortizado antes de TaxPain ya cubre todo el ejercicio, y empieza a mitad si la
/// amortización anterior llega a un día del ejercicio (apertura a mitad de año).
pub(crate) fn depreciation_basis(
    conn: &Connection,
    a: &FixedAsset,
    fy: &FiscalYear,
) -> Result<(Option<FiscalYear>, Money)> {
    let ours = accounting::accumulated_before(conn, a, fy.year)?;
    let (prior, until) = prior_of(conn, a.id)?;
    let before = ours + prior;
    Ok(match until {
        Some(u) if u >= fy.end => (None, before),
        Some(u) if u >= fy.start => (
            Some(FiscalYear {
                start: u + Duration::days(1),
                ..*fy
            }),
            before,
        ),
        _ => (Some(*fy), before),
    })
}
