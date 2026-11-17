//! Reglas del banco (texto del concepto → cuenta). Al contabilizar un movimiento contra una
//! cuenta se aprende la regla de su concepto: las palabras sin cifras, fechas ni referencias
//! («COMISION MANTENIMIENTO 0312» → «comision mantenimiento»). Después, los movimientos con el
//! mismo texto y sentido proponen esa cuenta y se pueden contabilizar juntos tras revisarlos.

use serde::Serialize;
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, params};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::bank;
use crate::error::{AppError, Result};

/// Palabras que aportan poco para reconocer un movimiento.
const STOP: &[&str] = &[
    "de", "del", "la", "el", "los", "las", "en", "a", "y", "por", "con", "para", "sl", "sa", "slu",
    "s.l.", "s.a.", "ref", "num", "nº", "no",
];

/// Texto clave de un concepto: minúsculas sin acentos, sin palabras con cifras ni vacías, las
/// cuatro primeras que quedan.
pub fn key_of(description: &str) -> String {
    let folded: String = description
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' => 'a',
            'é' | 'è' | 'ë' => 'e',
            'í' | 'ì' | 'ï' => 'i',
            'ó' | 'ò' | 'ö' => 'o',
            'ú' | 'ù' | 'ü' => 'u',
            c if c.is_alphanumeric() || c == 'ñ' || c == 'ç' => c,
            _ => ' ',
        })
        .collect();
    folded
        .split_whitespace()
        .filter(|w| w.chars().all(char::is_alphabetic) && w.chars().count() > 1)
        .filter(|w| !STOP.contains(w))
        .take(4)
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Rule {
    pub id: i64,
    pub pattern: String,
    /// `cargo` o `abono`.
    pub sign: String,
    pub account: String,
    pub hits: i64,
}

fn sign_of(amount: Money) -> &'static str {
    if amount.cents() < 0 { "cargo" } else { "abono" }
}

fn now_str(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

/// Aprende (o refuerza) la regla del concepto de un movimiento contabilizado contra `account`.
pub(crate) fn learn(
    conn: &Connection,
    description: &str,
    amount: Money,
    account: &str,
    now: OffsetDateTime,
) -> Result<()> {
    let pattern = key_of(description);
    if pattern.chars().count() < 3 {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO bank_rule(pattern, sign, account, updated_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(pattern, sign) DO UPDATE SET
           hits = CASE WHEN account = excluded.account THEN hits + 1 ELSE 1 END,
           account = excluded.account, updated_at = excluded.updated_at",
        params![pattern, sign_of(amount), account, now_str(now)],
    )?;
    Ok(())
}

/// Olvida una vez lo aprendido al conciliar un movimiento contra `account` (al deshacer la
/// conciliación): la regla pierde un uso y, si era el único, desaparece. Si la regla ya apunta a
/// otra cuenta, no se toca.
pub(crate) fn forget(
    conn: &Connection,
    description: &str,
    amount: Money,
    account: &str,
) -> Result<()> {
    let pattern = key_of(description);
    conn.execute(
        "UPDATE bank_rule SET hits = hits - 1 WHERE pattern = ?1 AND sign = ?2 AND account = ?3",
        params![pattern, sign_of(amount), account],
    )?;
    conn.execute(
        "DELETE FROM bank_rule WHERE pattern = ?1 AND sign = ?2 AND hits <= 0",
        params![pattern, sign_of(amount)],
    )?;
    Ok(())
}

pub fn list(v: &Vault) -> Result<Vec<Rule>> {
    list_conn(v.connection())
}

pub(crate) fn list_conn(conn: &Connection) -> Result<Vec<Rule>> {
    let mut st = conn
        .prepare("SELECT id, pattern, sign, account, hits FROM bank_rule ORDER BY pattern, sign")?;
    let rows = st.query_map([], |r| {
        Ok(Rule {
            id: r.get(0)?,
            pattern: r.get(1)?,
            sign: r.get(2)?,
            account: r.get(3)?,
            hits: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn delete(v: &Vault, id: i64, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let n = tx.execute("DELETE FROM bank_rule WHERE id = ?1", [id])?;
    if n == 0 {
        return Err(AppError::NotFound);
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "bank.rule.delete",
        &id.to_string(),
        &serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

/// Regla que se aplica a un concepto e importe: la de texto más largo contenido en el concepto
/// y, a igualdad, la más usada.
pub(crate) fn matching<'a>(
    rules: &'a [Rule],
    description: &str,
    amount: Money,
) -> Option<&'a Rule> {
    let key = format!(" {} ", key_of(description));
    let sign = sign_of(amount);
    rules
        .iter()
        .filter(|r| r.sign == sign && key.contains(&format!(" {} ", r.pattern)))
        .max_by_key(|r| (r.pattern.len(), r.hits))
}

/// Cuenta que propone la regla para un movimiento, si hay.
pub fn rule_for(v: &Vault, tx_id: i64) -> Result<Option<Rule>> {
    let (desc, amount): (String, i64) = v.connection().query_row(
        "SELECT description, amount FROM bank_tx WHERE id = ?1",
        [tx_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let rules = list(v)?;
    Ok(matching(&rules, &desc, Money::from_cents(amount)).cloned())
}

/// Movimiento pendiente con la cuenta que propone su regla.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Proposal {
    pub tx: bank::BankTx,
    pub account: String,
    pub pattern: String,
}

/// Movimientos sin conciliar que tienen regla y ninguna factura o remesa por el mismo importe
/// (esos se concilian con su factura, no con la regla).
pub fn proposals(v: &Vault) -> Result<Vec<Proposal>> {
    let rules = list(v)?;
    let mut out = Vec::new();
    // La cartera entera se carga una vez (no por movimiento: con un extracto anual y miles de
    // facturas, la pantalla tardaba movimientos × facturas).
    let cartera = crate::cartera::list(v, OffsetDateTime::now_utc().date())?;
    // Las remesas sin apuntar, igual: una consulta, no una por movimiento.
    let remesas = crate::remesas::unmatched(v.connection())?;
    for t in bank::movements(v, true)? {
        // En otra divisa sólo con su contravalor (el asiento va en euros).
        if t.currency != "EUR" && t.amount_eur.is_none() {
            continue;
        }
        let Some(r) = matching(&rules, &t.description, t.amount) else {
            continue;
        };
        if !bank::suggestions_from(v, t.id, &cartera, &remesas)?.is_empty() {
            continue;
        }
        out.push(Proposal {
            account: r.account.clone(),
            pattern: r.pattern.clone(),
            tx: t,
        });
    }
    Ok(out)
}

/// Contabiliza los movimientos `ids` (revisados por el usuario) con la cuenta de su regla.
/// Devuelve cuántos.
pub fn apply(v: &Vault, ids: &[i64], now: OffsetDateTime) -> Result<usize> {
    let props = proposals(v)?;
    let mut n = 0;
    for id in ids {
        let p = props
            .iter()
            .find(|p| p.tx.id == *id)
            .ok_or_else(|| AppError::Other(format!("el movimiento {id} ya no tiene regla")))?;
        bank::reconcile_account(v, *id, &p.account, &p.tx.description, now)?;
        n += 1;
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claves() {
        assert_eq!(
            key_of("COMISION MANTENIMIENTO 0312"),
            "comision mantenimiento"
        );
        assert_eq!(
            key_of("RECIBO IBERDROLA CLIENTES SAU REF.2026-10 Nº 77"),
            "recibo iberdrola clientes sau"
        );
        assert_eq!(
            key_of("Transf. a favor de: Gestoría López S.L."),
            "transf favor gestoria lopez"
        );
        assert_eq!(key_of("12/10 1234"), "");
    }

    #[test]
    fn la_mas_concreta_gana() {
        let r = |id, p: &str, a: &str, hits| Rule {
            id,
            pattern: p.into(),
            sign: "cargo".into(),
            account: a.into(),
            hits,
        };
        let rules = [r(1, "recibo", "629", 9), r(2, "recibo iberdrola", "628", 1)];
        let m = |d: &str, c: i64| matching(&rules, d, Money::from_cents(c)).map(|r| r.id);
        assert_eq!(m("RECIBO IBERDROLA 1234", -100), Some(2));
        assert_eq!(m("RECIBO TELEFONICA", -100), Some(1));
        assert_eq!(m("RECIBO TELEFONICA", 100), None, "abono: otro sentido");
        assert_eq!(m("RECIBOS VARIOS", -100), None, "palabra completa");
    }
}
