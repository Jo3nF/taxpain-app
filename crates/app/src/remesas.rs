//! Remesas SEPA, como las de a3 (`docs/research/A3.md` §2.7): recibos domiciliados a clientes
//! (pain.008, Cuaderno 19.14) y órdenes de transferencia a proveedores (pain.001, 34.14).
//!
//! Contabilidad (PGC):
//! - Al generar una remesa de cobros, cada recibo pasa del cliente a la 4312 «Efectos
//!   comerciales en gestión de cobro»; en una orden de pagos, del proveedor a la 555 «Partidas
//!   pendientes de aplicación».
//! - Cuando el banco abona o carga el total, el movimiento se casa con la remesa (572 contra
//!   4312 o 555).
//! - Un recibo devuelto vuelve a la cartera: si el banco aún no había abonado la remesa, se
//!   borra su asiento; si ya lo había hecho, se anota la devolución (cliente contra banco).
//!
//! TaxPain sólo genera el fichero: lo sube al banco la persona usuaria.

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::{AccountCode, EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::PartyId;
use taxpain_core::money::Money;
use taxpain_core::sepa::{
    self, CreditTransfer, CreditTx, DebitTx, DirectDebit, Scheme, iban_from_ccc, iban_from_n43,
    normalize_iban,
};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, OffsetDateTime, PrimitiveDateTime};

use crate::cartera::{self, DueItem};
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

const SEPA_KEY: &str = "sepa";
/// Cuentas puente hasta que el banco apunta la remesa.
pub const BRIDGE_COLLECT: &str = "4312";
pub const BRIDGE_PAY: &str = "555";

/// Ajustes SEPA de la empresa.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SepaSettings {
    /// Sufijo del identificador de acreedor que da el banco (normalmente `000`).
    pub creditor_suffix: String,
    /// Adeudos B2B (Cuaderno 19.44, sólo entre empresas) en vez de CORE.
    pub b2b: bool,
}

impl Default for SepaSettings {
    fn default() -> Self {
        SepaSettings {
            creditor_suffix: "000".into(),
            b2b: false,
        }
    }
}

pub fn settings(v: &Vault) -> Result<SepaSettings> {
    Ok(v.setting(SEPA_KEY)?.unwrap_or_default())
}

pub fn save_settings(v: &Vault, s: &SepaSettings) -> Result<()> {
    sepa::creditor_id("B00000000", &s.creditor_suffix)
        .map_err(|e| AppError::Other(e.to_string()))?;
    Ok(v.set_setting(SEPA_KEY, s)?)
}

fn sepa_err(e: sepa::SepaError) -> AppError {
    AppError::Other(e.to_string())
}

/// `cobros` o `pagos`.
fn check_kind(kind: &str) -> Result<&'static str> {
    match kind {
        "cobros" => Ok("emitida"),
        "pagos" => Ok("recibida"),
        _ => Err(AppError::Other("tipo de remesa no válido".into())),
    }
}

/// Vencimiento que puede ir en una remesa, con lo que le falte para ello.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Candidate {
    pub item: DueItem,
    pub iban: Option<String>,
    pub mandate_ref: Option<String>,
    pub mandate_date: Option<Date>,
    /// Por qué no puede ir todavía (falta el IBAN o el mandato).
    pub problem: Option<String>,
}

/// Vencimientos pendientes para una remesa: en cobros, los de clientes con forma de pago
/// «recibo»; en pagos, los de proveedores que se pagan por transferencia (o sin forma fijada).
pub fn candidates(v: &Vault, kind: &str, today: Date) -> Result<Vec<Candidate>> {
    let invoice_kind = check_kind(kind)?;
    let parties = cartera::parties(v)?;
    let mut out = Vec::new();
    for item in cartera::list(v, today)? {
        if item.kind != invoice_kind || item.status != "pendiente" || item.amount.cents() <= 0 {
            continue;
        }
        let terms = item
            .party_account
            .as_deref()
            .and_then(|a| parties.iter().find(|p| p.account == a))
            .and_then(|p| p.terms.clone());
        let method = terms.as_ref().map(|t| t.method.as_str());
        let fits = if kind == "cobros" {
            method == Some("recibo")
        } else {
            matches!(method, None | Some("transferencia") | Some("otro"))
        };
        if !fits {
            continue;
        }
        let iban = terms.as_ref().and_then(|t| t.iban.clone());
        let mandate_ref = terms.as_ref().and_then(|t| t.mandate_ref.clone());
        let mandate_date = terms.as_ref().and_then(|t| t.mandate_date);
        let problem = if iban.is_none() {
            Some(format!(
                "falta el IBAN de {} (Condiciones de pago)",
                item.party
            ))
        } else if kind == "cobros" && (mandate_ref.is_none() || mandate_date.is_none()) {
            Some(format!(
                "falta el mandato SEPA de {} (referencia y fecha de firma)",
                item.party
            ))
        } else {
            None
        };
        out.push(Candidate {
            item,
            iban,
            mandate_ref,
            mandate_date,
            problem,
        });
    }
    Ok(out)
}

/// Cuenta bancaria de la empresa para remesas.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BankChoice {
    pub name: String,
    pub iban: String,
    pub account: String,
}

/// Cuentas bancarias importadas de los extractos (con su IBAN, calculado de la cuenta si hace
/// falta). Sólo las cuentas en euros: las remesas SEPA van en euros, y las tarjetas y los
/// monederos no las cargan ni las abonan.
pub fn banks(v: &Vault) -> Result<Vec<BankChoice>> {
    let mut stmt = v.connection().prepare(
        "SELECT name, iban, number, account FROM bank_account
         WHERE currency = 'EUR' AND kind = 'cuenta' ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (name, iban, number, account) = row?;
        let iban = match iban.as_deref().map(normalize_iban) {
            Some(Ok(i)) => i,
            _ => match iban_from_ccc(&number).or_else(|_| iban_from_n43(&number)) {
                Ok(i) => i,
                Err(_) => continue,
            },
        };
        out.push(BankChoice {
            name,
            iban,
            account,
        });
    }
    Ok(out)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RemittanceItem {
    pub kind: String,
    pub invoice_id: i64,
    pub seq: i64,
    pub number: String,
    pub party: String,
    pub amount: Money,
    pub returned_on: Option<Date>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Remittance {
    pub id: i64,
    /// `cobros` o `pagos`.
    pub kind: String,
    pub reference: String,
    pub bank_iban: String,
    pub charge_date: Date,
    /// Total de lo no devuelto (lo que el banco abona o carga).
    pub total: Money,
    pub items: Vec<RemittanceItem>,
    /// El banco ya la apuntó (movimiento casado).
    pub in_bank: bool,
    pub created_at: String,
}

fn parse(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]"))
        .map_err(|e| AppError::Other(e.to_string()))
}

fn load_items(conn: &Connection, id: i64) -> Result<Vec<RemittanceItem>> {
    let mut stmt = conn.prepare(
        "SELECT i.kind, i.invoice_id, i.seq, i.amount, i.returned_on,
                CASE WHEN i.invoice_id < 0 THEN (SELECT number FROM opening_item WHERE id = -i.invoice_id)
                     WHEN i.kind = 'emitida' THEN (SELECT number FROM invoice_issued WHERE id = i.invoice_id)
                     ELSE (SELECT COALESCE(number, '') FROM invoice_received WHERE id = i.invoice_id) END,
                CASE WHEN i.invoice_id < 0 THEN (SELECT name FROM opening_item WHERE id = -i.invoice_id)
                     WHEN i.kind = 'emitida' THEN (SELECT COALESCE(recipient_name, '') FROM invoice_issued WHERE id = i.invoice_id)
                     ELSE (SELECT COALESCE(supplier_name, '') FROM invoice_received WHERE id = i.invoice_id) END
         FROM remittance_item i WHERE i.remittance_id = ?1 ORDER BY i.rowid",
    )?;
    let rows = stmt.query_map([id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, Option<String>>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (kind, invoice_id, seq, amount, returned, number, party) = row?;
        out.push(RemittanceItem {
            kind,
            invoice_id,
            seq,
            number: number.unwrap_or_default(),
            party: party.unwrap_or_default(),
            amount: Money::from_cents(amount),
            returned_on: returned.as_deref().map(parse).transpose()?,
        });
    }
    Ok(out)
}

fn load_one(conn: &Connection, id: i64) -> Result<Remittance> {
    let (kind, reference, bank_iban, charge, bank_tx, created): (
        String,
        String,
        String,
        String,
        Option<i64>,
        String,
    ) = conn
        .query_row(
            "SELECT kind, reference, bank_iban, charge_date, bank_tx_id, created_at FROM remittance WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let items = load_items(conn, id)?;
    let total = items
        .iter()
        .filter(|i| i.returned_on.is_none())
        .fold(Money::ZERO, |a, i| a + i.amount);
    Ok(Remittance {
        id,
        kind,
        reference,
        bank_iban,
        charge_date: parse(&charge)?,
        total,
        items,
        in_bank: bank_tx.is_some(),
        created_at: created,
    })
}

/// Remesas generadas, la más reciente primero.
pub fn list(v: &Vault) -> Result<Vec<Remittance>> {
    let conn = v.connection();
    let ids: Vec<i64> = conn
        .prepare("SELECT id FROM remittance ORDER BY id DESC")?
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    ids.into_iter().map(|id| load_one(conn, id)).collect()
}

/// XML de una remesa (referencia y contenido), para guardarlo y subirlo al banco.
pub fn file(v: &Vault, id: i64) -> Result<(String, String)> {
    Ok(v.connection().query_row(
        "SELECT reference, file FROM remittance WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

/// Genera una remesa con los vencimientos elegidos (factura y número de vencimiento), la deja
/// contabilizada contra la cuenta puente y devuelve la remesa con su XML guardado.
pub fn create(
    v: &Vault,
    kind: &str,
    items: &[(i64, i64)],
    bank_iban: &str,
    charge_date: Date,
    now: OffsetDateTime,
) -> Result<Remittance> {
    let invoice_kind = check_kind(kind)?;
    if items.is_empty() {
        return Err(AppError::Other("elige al menos un vencimiento".into()));
    }
    if charge_date < now.date() {
        return Err(AppError::Other(
            "la fecha de cargo no puede ser anterior a hoy".into(),
        ));
    }
    let bank_iban = normalize_iban(bank_iban).map_err(sepa_err)?;
    let inv = crate::invoicing::settings(v)?.ok_or(AppError::NotConfigured)?;
    let PartyId::Nif(nif) = &inv.issuer.id else {
        return Err(AppError::Other("la empresa ha de tener NIF español".into()));
    };
    let sepa_settings = settings(v)?;
    let all = candidates(v, kind, now.date())?;
    let mut chosen: Vec<&Candidate> = Vec::new();
    for (id, seq) in items {
        let c = all
            .iter()
            .find(|c| c.item.invoice_id == *id && c.item.seq == *seq)
            .ok_or_else(|| {
                AppError::Other(
                    "un vencimiento elegido ya no está pendiente o no admite remesa".into(),
                )
            })?;
        if let Some(p) = &c.problem {
            return Err(AppError::Other(format!("{}: {p}", c.item.number)));
        }
        if chosen.iter().any(|x| std::ptr::eq(*x, c)) {
            continue;
        }
        chosen.push(c);
    }

    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let year = charge_date.year();
    let prefix = if kind == "cobros" { "REM" } else { "PAG" };
    // Siguiente número del año: el mayor usado más uno (no la cuenta de remesas, que tras
    // deshacer una que no sea la última repetiría una referencia ya usada y, con ella, el
    // MsgId del fichero SEPA).
    let n: i64 = tx.query_row(
        "SELECT COALESCE(MAX(CAST(substr(reference, -4) AS INTEGER)), 0) FROM remittance
         WHERE kind = ?1 AND reference LIKE ?2",
        params![kind, format!("{prefix}-{year}-%")],
        |r| r.get(0),
    )?;
    let reference = format!("{prefix}-{year}-{:04}", n + 1);
    let created = PrimitiveDateTime::new(now.date(), now.time());
    let remittance_text = |i: &DueItem| {
        if i.installments > 1 {
            format!("Factura {} plazo {}/{}", i.number, i.seq, i.installments)
        } else {
            format!("Factura {}", i.number)
        }
    };
    let xml = if kind == "cobros" {
        sepa::pain008(&DirectDebit {
            msg_id: reference.clone(),
            created,
            creditor_name: inv.issuer.name.clone(),
            creditor_iban: bank_iban.clone(),
            creditor_bic: None,
            creditor_id: sepa::creditor_id(nif, &sepa_settings.creditor_suffix)
                .map_err(sepa_err)?,
            collection_date: charge_date,
            scheme: if sepa_settings.b2b {
                Scheme::B2b
            } else {
                Scheme::Core
            },
            txs: chosen
                .iter()
                .map(|c| DebitTx {
                    end_to_end: format!("{}/{}", c.item.number, c.item.seq),
                    amount: c.item.amount,
                    mandate_id: c.mandate_ref.clone().unwrap_or_default(),
                    mandate_date: c.mandate_date.unwrap_or(charge_date),
                    debtor_name: c.item.party.clone(),
                    debtor_iban: c.iban.clone().unwrap_or_default(),
                    remittance: remittance_text(&c.item),
                })
                .collect(),
        })
    } else {
        sepa::pain001(&CreditTransfer {
            msg_id: reference.clone(),
            created,
            debtor_name: inv.issuer.name.clone(),
            debtor_id: nif.clone(),
            debtor_iban: bank_iban.clone(),
            debtor_bic: None,
            execution_date: charge_date,
            txs: chosen
                .iter()
                .map(|c| CreditTx {
                    end_to_end: format!("{}/{}", c.item.number, c.item.seq),
                    amount: c.item.amount,
                    creditor_name: c.item.party.clone(),
                    creditor_iban: c.iban.clone().unwrap_or_default(),
                    remittance: remittance_text(&c.item),
                })
                .collect(),
        })
    }
    .map_err(sepa_err)?;
    let total = chosen.iter().fold(Money::ZERO, |a, c| a + c.item.amount);
    let bridge = if kind == "cobros" {
        BRIDGE_COLLECT
    } else {
        BRIDGE_PAY
    };
    tx.execute(
        "INSERT INTO remittance(kind, reference, bank_iban, bridge, charge_date, total, file, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![kind, reference, bank_iban, bridge, charge_date.to_string(), total.cents(), xml, now_str(now)],
    )?;
    let id = tx.last_insert_rowid();
    let bridge_code = AccountCode::new(bridge).map_err(|e| AppError::Other(e.to_string()))?;
    for c in &chosen {
        tx.execute(
            "INSERT INTO remittance_item(remittance_id, kind, invoice_id, seq, amount) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, invoice_kind, c.item.invoice_id, c.item.seq, c.item.amount.cents()],
        )?;
        cartera::settle_in(
            &tx,
            invoice_kind,
            c.item.invoice_id,
            c.item.seq,
            charge_date,
            &bridge_code,
            Some(&format!("Remesa {reference}")),
            None,
            now,
        )?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "remesa.generar",
        &reference,
        &serde_json::json!({ "kind": kind, "items": chosen.len(), "total": total.cents() }),
    )?;
    let r = load_one(&tx, id)?;
    tx.commit()?;
    Ok(r)
}

/// Borra el cobro o pago (y su asiento) de un vencimiento remesado.
fn drop_settlement(tx: &Connection, kind: &str, invoice_id: i64, seq: i64) -> Result<()> {
    let found: Option<(i64, Option<i64>)> = tx
        .query_row(
            "SELECT id, entry_id FROM settlement WHERE kind = ?1 AND invoice_id = ?2 AND seq = ?3",
            params![kind, invoice_id, seq],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((id, entry)) = found {
        tx.execute("DELETE FROM settlement WHERE id = ?1", [id])?;
        if let Some(e) = entry {
            tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [e])?;
            tx.execute("DELETE FROM journal_entry WHERE id = ?1", [e])?;
        }
    }
    Ok(())
}

/// Deshace una remesa que el banco aún no ha apuntado: sus vencimientos vuelven a pendientes y
/// se borran sus asientos.
pub fn undo(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let r = load_one(&tx, id)?;
    if r.in_bank {
        return Err(AppError::Other(
            "el banco ya la apuntó: deshaz antes la conciliación o marca los recibos devueltos"
                .into(),
        ));
    }
    for i in r.items.iter().filter(|i| i.returned_on.is_none()) {
        drop_settlement(&tx, &i.kind, i.invoice_id, i.seq)?;
    }
    tx.execute("DELETE FROM remittance WHERE id = ?1", [id])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "remesa.deshacer",
        &r.reference,
        &serde_json::json!({ "items": r.items.len() }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Recibo devuelto (o transferencia rechazada): el vencimiento vuelve a la cartera. Si el banco
/// ya había apuntado la remesa, se anota la devolución contra `account` (572x) el día `date`
/// y, si el cargo (o abono) de la devolución ya está en el extracto, queda conciliado con ese
/// asiento; si llega después, el banco lo propone como «Devolución».
#[allow(clippy::too_many_arguments)]
pub fn return_item(
    v: &Vault,
    id: i64,
    invoice_id: i64,
    seq: i64,
    date: Date,
    account: Option<&str>,
    now: OffsetDateTime,
) -> Result<Option<i64>> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let r = load_one(&tx, id)?;
    let item = r
        .items
        .iter()
        .find(|i| i.invoice_id == invoice_id && i.seq == seq && i.returned_on.is_none())
        .ok_or_else(|| {
            AppError::Other("ese recibo no está en la remesa o ya se devolvió".into())
        })?;
    let mut entry_id = None;
    let mut bank_tx = None;
    if r.in_bank {
        let bank = AccountCode::new(account.unwrap_or("").trim())
            .map_err(|_| AppError::Other("indica la cuenta del banco (572…)".into()))?;
        if !bank.starts_with("57") {
            return Err(AppError::Other(
                "la devolución va contra la cuenta del banco (572…)".into(),
            ));
        }
        let (party, _) = crate::bank::invoice_party(&tx, &item.kind, invoice_id)?;
        // Cobro devuelto: el cliente vuelve a deber (430 al debe, banco al haber); en un pago
        // rechazado, al revés.
        let collect = item.kind == "emitida";
        let entry = JournalEntry {
            date,
            concept: format!(
                "Devolución {} {} · remesa {}",
                if collect { "recibo" } else { "transferencia" },
                item.number,
                r.reference
            ),
            // Origen propio: enlaza con la remesa y no se retoca ni se borra desde el diario.
            origin: Origin::DevolucionRemesa(id),
            lines: [
                EntryLine::signed(party, item.amount, collect),
                EntryLine::signed(bank.clone(), item.amount, !collect),
            ]
            .into_iter()
            .flatten()
            .collect(),
        };
        let entry = crate::accounting::record_entry(&tx, &entry, now)?;
        entry_id = Some(entry);
        // El cargo del recibo devuelto (o el abono de la transferencia rechazada) que ya está
        // en el extracto queda conciliado con este asiento: llevarlo a otra cuenta lo
        // contabilizaría dos veces.
        let movement = if collect { -item.amount } else { item.amount };
        if let Some(t) = crate::bank::pending_return_movement(&tx, bank.as_str(), movement, date)? {
            tx.execute(
                "UPDATE bank_tx SET entry_id = ?1, matched_kind = 'cuenta' WHERE id = ?2",
                params![entry, t],
            )?;
            bank_tx = Some(t);
        }
        // Su cobro queda en el diario (sí ocurrió); el vencimiento vuelve a pendiente.
        tx.execute(
            "DELETE FROM settlement WHERE kind = ?1 AND invoice_id = ?2 AND seq = ?3",
            params![item.kind, invoice_id, seq],
        )?;
    } else {
        drop_settlement(&tx, &item.kind, invoice_id, seq)?;
    }
    tx.execute(
        "UPDATE remittance_item SET returned_on = ?1
         WHERE remittance_id = ?2 AND invoice_id = ?3 AND seq = ?4",
        params![date.to_string(), id, invoice_id, seq],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "remesa.devolucion",
        &r.reference,
        &serde_json::json!({ "invoice": invoice_id, "seq": seq, "entry": entry_id, "bank_tx": bank_tx }),
    )?;
    tx.commit()?;
    Ok(entry_id)
}

/// Deshace un recibo devuelto (o una transferencia rechazada) anotado por error: el recibo
/// vuelve a la remesa, cobrado (o pagado) por ella como antes. Se borra el asiento de la
/// devolución (el movimiento del banco casado con él vuelve a pendiente) y, si la devolución
/// se anotó antes de que el banco apuntara la remesa, se vuelve a registrar su cobro contra la
/// cuenta puente. No vale si el recibo se ha vuelto a cobrar o va en otra remesa.
pub fn undo_return(
    v: &Vault,
    id: i64,
    invoice_id: i64,
    seq: i64,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let r = load_one(&tx, id)?;
    let item = r
        .items
        .iter()
        .find(|i| i.invoice_id == invoice_id && i.seq == seq && i.returned_on.is_some())
        .ok_or_else(|| AppError::Other("ese recibo no está devuelto en esta remesa".into()))?;
    let again: bool = tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM settlement WHERE kind = ?1 AND invoice_id = ?2 AND seq = ?3)
             OR EXISTS(SELECT 1 FROM bank_tx WHERE matched_kind = ?1 AND matched_id = ?2
                       AND COALESCE(matched_seq, 1) = ?3)",
        params![item.kind, invoice_id, seq],
        |row| row.get(0),
    )?;
    if again {
        return Err(AppError::Other(format!(
            "{} ya se ha vuelto a cobrar o pagar (o va en otra remesa): deshaz eso antes",
            item.number
        )));
    }
    let collect = item.kind == "emitida";
    let concept = format!(
        "Devolución {} {} · remesa {}",
        if collect { "recibo" } else { "transferencia" },
        item.number,
        r.reference
    );
    let entry: Option<(i64, bool)> = tx
        .query_row(
            "SELECT e.id, COALESCE(f.closed, 0) FROM journal_entry e
               LEFT JOIN fiscal_year f ON f.year = e.fiscal_year
             WHERE e.origin_kind = 'DevolucionRemesa' AND e.origin_id = ?1 AND e.concept = ?2
             ORDER BY e.id DESC LIMIT 1",
            params![id, concept],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let bridge: String =
        tx.query_row("SELECT bridge FROM remittance WHERE id = ?1", [id], |row| {
            row.get(0)
        })?;
    let bridge = AccountCode::new(&bridge).map_err(|e| AppError::Other(e.to_string()))?;
    // El cobro del recibo por la remesa: si la devolución se anotó con el banco ya apuntado,
    // sigue en el diario sin su vencimiento (vuelve a ser su cobro); si no, se borró.
    let (party, _) = crate::bank::invoice_party(&tx, &item.kind, invoice_id)?;
    let cobro: Option<(i64, i64, String)> = tx
        .query_row(
            "SELECT e.id, e.origin_id, e.date FROM journal_entry e
             WHERE e.origin_kind = 'Cartera'
               AND e.origin_id NOT IN (SELECT id FROM settlement)
               AND e.concept LIKE '%· Remesa ' || ?1
               AND EXISTS(SELECT 1 FROM journal_line l WHERE l.entry_id = e.id
                          AND l.account = ?2 AND l.debit + l.credit = ?3)
               AND EXISTS(SELECT 1 FROM journal_line l WHERE l.entry_id = e.id
                          AND l.account = ?4)
             ORDER BY e.id DESC LIMIT 1",
            params![
                r.reference,
                party.as_str(),
                item.amount.cents().abs(),
                bridge.as_str()
            ],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let mut unlinked = None;
    match (entry, cobro) {
        (Some((_, true)), _) => {
            return Err(AppError::Other(
                "la devolución es de un ejercicio cerrado: para deshacerla hay que reabrirlo \
                 desde Cierre"
                    .into(),
            ));
        }
        (Some((entry, false)), Some((cobro, settlement, date))) => {
            unlinked = tx
                .query_row(
                    "SELECT id FROM bank_tx WHERE entry_id = ?1",
                    [entry],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;
            tx.execute(
                "UPDATE bank_tx SET entry_id = NULL, matched_kind = NULL, matched_id = NULL,
                        matched_seq = NULL WHERE entry_id = ?1",
                [entry],
            )?;
            tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [entry])?;
            tx.execute("DELETE FROM journal_entry WHERE id = ?1", [entry])?;
            tx.execute(
                "INSERT INTO settlement(id, kind, invoice_id, seq, date, account, entry_id, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![settlement, item.kind, invoice_id, seq, date, bridge.as_str(), cobro, now_str(now)],
            )?;
        }
        (None, None) if r.in_bank => {
            return Err(AppError::Other(format!(
                "el banco apuntó la remesa {} sin este recibo: si al final se cobró, regístralo \
                 desde la cartera o concilia su movimiento",
                r.reference
            )));
        }
        (None, None) => {
            cartera::settle_in(
                &tx,
                &item.kind,
                invoice_id,
                seq,
                r.charge_date,
                &bridge,
                Some(&format!("Remesa {}", r.reference)),
                None,
                now,
            )?;
        }
        _ => {
            return Err(AppError::Other(format!(
                "no encuentro en el diario los asientos de la devolución de {}: corrígela con \
                 un asiento manual",
                item.number
            )));
        }
    }
    tx.execute(
        "UPDATE remittance_item SET returned_on = NULL
         WHERE remittance_id = ?1 AND invoice_id = ?2 AND seq = ?3",
        params![id, invoice_id, seq],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "remesa.deshacer_devolucion",
        &r.reference,
        &serde_json::json!({
            "invoice": invoice_id,
            "seq": seq,
            "entry": entry.map(|e| e.0),
            "bank_tx": unlinked,
        }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Remesas sin apuntar en el banco (de cobros y de pagos), de la más reciente a la más antigua.
/// Quien recorre muchos movimientos las carga una vez y las cruza con [`matching_in`].
pub(crate) fn unmatched(conn: &Connection) -> Result<Vec<Remittance>> {
    let ids: Vec<i64> = conn
        .prepare("SELECT id FROM remittance WHERE bank_tx_id IS NULL ORDER BY id DESC")?
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    ids.into_iter().map(|id| load_one(conn, id)).collect()
}

/// Remesas sin apuntar en el banco (ya cargadas con [`unmatched`]) cuyo total coincide con un
/// movimiento (ingreso para cobros, cargo para pagos).
pub(crate) fn matching_in(unmatched: &[Remittance], amount_cents: i64) -> Vec<Remittance> {
    let kind = if amount_cents > 0 { "cobros" } else { "pagos" };
    unmatched
        .iter()
        .filter(|r| r.kind == kind && r.total.cents() == amount_cents.abs() && r.total.cents() > 0)
        .cloned()
        .collect()
}

/// Cuenta puente de una remesa sin apuntar en el banco.
pub(crate) fn bridge_of(conn: &Connection, id: i64) -> Result<(String, String, Money)> {
    let (bridge, reference, in_bank): (String, String, Option<i64>) = conn
        .query_row(
            "SELECT bridge, reference, bank_tx_id FROM remittance WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    if in_bank.is_some() {
        return Err(AppError::Other(
            "esa remesa ya está casada con un movimiento".into(),
        ));
    }
    Ok((bridge, reference, load_one(conn, id)?.total))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remesa(id: i64, kind: &str, total: i64) -> Remittance {
        Remittance {
            id,
            kind: kind.into(),
            reference: format!("R-{id}"),
            bank_iban: String::new(),
            charge_date: time::macros::date!(2026 - 10 - 20),
            total: Money::from_cents(total),
            items: Vec::new(),
            in_bank: false,
            created_at: String::new(),
        }
    }

    /// Las remesas se cargan una vez y cada movimiento se cruza en memoria: un ingreso casa con
    /// las de cobros por su total y un cargo con las de pagos.
    #[test]
    fn cruce_de_remesas_cargadas_una_vez() {
        let todas = [
            remesa(3, "cobros", 50_000),
            remesa(2, "pagos", 50_000),
            remesa(1, "cobros", 0),
        ];
        let ids = |l: Vec<Remittance>| l.iter().map(|r| r.id).collect::<Vec<_>>();
        assert_eq!(ids(matching_in(&todas, 50_000)), [3]);
        assert_eq!(ids(matching_in(&todas, -50_000)), [2]);
        assert!(
            matching_in(&todas, 0).is_empty(),
            "un total de cero no casa"
        );
        assert!(matching_in(&todas, 49_999).is_empty());
    }
}
