//! Contabilidad analítica por centros de coste (`docs/research/A3.md` §2.17): los gastos (6) y
//! los ingresos (7) se reparten entre centros por porcentaje, con una regla por defecto por
//! cuenta (o grupo de cuentas) y, si hace falta, un reparto propio en una línea concreta.
//! Salida: la cuenta de resultados de cada centro.
//!
//! Los centros pueden colgar unos de otros (hasta [`MAX_LEVELS`] niveles: p. ej. zona → tienda →
//! sección) y la cuenta de resultados se puede pedir por nivel, sumando lo de cada centro en su
//! antecesor de ese nivel. Reparto secundario: lo de un centro (servicios generales,
//! administración…) se reparte al final entre otros por porcentaje.
//!
//! Va aparte del diario: repartir no cambia los libros (como el punteo).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;

/// 100 % en centésimas de punto.
pub const FULL: u16 = 10_000;

/// Niveles de centros como máximo (como a3: cuatro).
pub const MAX_LEVELS: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Center {
    pub code: String,
    pub name: String,
    /// Centro del que cuelga (nivel superior).
    #[serde(default)]
    pub parent: Option<String>,
}

/// Reparto secundario de un centro entre otros.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Redistribution {
    pub source: String,
    pub shares: Vec<Share>,
}

/// Parte de un centro en un reparto (en centésimas de punto: 2500 = 25 %).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Share {
    pub center: String,
    pub pct_bp: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    /// Cuenta o principio de cuenta (`62`, `6290001`, `705`…).
    pub prefix: String,
    pub shares: Vec<Share>,
}

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

pub fn centers(v: &Vault) -> Result<Vec<Center>> {
    let mut stmt = v
        .connection()
        .prepare("SELECT code, name, parent FROM cost_center ORDER BY code")?;
    let rows = stmt.query_map([], |r| {
        Ok(Center {
            code: r.get(0)?,
            name: r.get(1)?,
            parent: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Cadena de un centro hasta la raíz (él primero). Corta si hay un ciclo.
fn chain(parents: &BTreeMap<String, Option<String>>, code: &str) -> Vec<String> {
    let mut out = vec![code.to_owned()];
    let mut cur = parents.get(code).cloned().flatten();
    while let Some(p) = cur {
        if out.contains(&p) || out.len() > MAX_LEVELS + 1 {
            break;
        }
        out.push(p.clone());
        cur = parents.get(&p).cloned().flatten();
    }
    out
}

/// Nivel de un centro (1 = sin padre).
pub fn level(centers: &[Center], code: &str) -> usize {
    let parents: BTreeMap<String, Option<String>> = centers
        .iter()
        .map(|c| (c.code.clone(), c.parent.clone()))
        .collect();
    chain(&parents, code).len()
}

/// Da de alta un centro o le cambia el nombre.
pub fn save_center(v: &Vault, c: &Center, now: OffsetDateTime) -> Result<()> {
    let code = c.code.trim();
    if code.is_empty()
        || code.len() > 12
        || !code
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
    {
        return Err(other(
            "el código del centro tiene de 1 a 12 letras, números o guiones",
        ));
    }
    if c.name.trim().is_empty() {
        return Err(other("pon el nombre del centro"));
    }
    let code = code.to_uppercase();
    let parent = c
        .parent
        .as_deref()
        .map(|p| p.trim().to_uppercase())
        .filter(|p| !p.is_empty());
    let mut all = centers(v)?;
    if let Some(p) = &parent {
        if !all.iter().any(|x| &x.code == p) {
            return Err(other(format!("no existe el centro {p}")));
        }
        // Con el cambio aplicado: sin ciclos y como mucho cuatro niveles (también los que
        // cuelgan de éste).
        match all.iter_mut().find(|x| x.code == code) {
            Some(x) => x.parent = Some(p.clone()),
            None => all.push(Center {
                code: code.clone(),
                name: String::new(),
                parent: Some(p.clone()),
            }),
        }
        let parents: BTreeMap<String, Option<String>> = all
            .iter()
            .map(|c| (c.code.clone(), c.parent.clone()))
            .collect();
        for x in &all {
            // Subiendo desde el centro: si se vuelve a él, hay un ciclo.
            let mut cur = parents.get(&x.code).cloned().flatten();
            let mut steps = 0;
            let mut cyclic = false;
            while let Some(p) = cur {
                if p == x.code || steps > MAX_LEVELS {
                    cyclic = true;
                    break;
                }
                steps += 1;
                cur = parents.get(&p).cloned().flatten();
            }
            if cyclic || chain(&parents, &x.code).len() > MAX_LEVELS {
                return Err(other(format!(
                    "{code} no puede colgar de {p}: haría un ciclo o más de {MAX_LEVELS} niveles"
                )));
            }
        }
    }
    v.connection().execute(
        "INSERT INTO cost_center(code, name, parent, created_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(code) DO UPDATE SET name = excluded.name, parent = excluded.parent",
        params![code, c.name.trim(), parent, now_str(now)],
    )?;
    Ok(())
}

pub fn redistributions(v: &Vault) -> Result<Vec<Redistribution>> {
    let mut stmt = v.connection().prepare(
        "SELECT source, target, pct_bp FROM cost_redistribution ORDER BY source, target",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, u16>(2)?,
        ))
    })?;
    let mut map: BTreeMap<String, Vec<Share>> = BTreeMap::new();
    for row in rows {
        let (source, center, pct_bp) = row?;
        map.entry(source)
            .or_default()
            .push(Share { center, pct_bp });
    }
    Ok(map
        .into_iter()
        .map(|(source, shares)| Redistribution { source, shares })
        .collect())
}

/// Guarda el reparto secundario de un centro (sin partes, lo quita).
pub fn set_redistribution(
    v: &Vault,
    source: &str,
    shares: &[Share],
    now: OffsetDateTime,
) -> Result<()> {
    let source = source.trim().to_uppercase();
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    if !shares.is_empty() {
        check_shares(&tx, shares)?;
        if shares.iter().any(|s| s.center == source) {
            return Err(other("un centro no se reparte a sí mismo"));
        }
        let others = redistributions(v)?;
        if others
            .iter()
            .any(|r| r.source != source && r.shares.iter().any(|s| s.center == source))
        {
            return Err(other(format!(
                "{source} ya recibe el reparto de otro centro: los repartos secundarios no se encadenan"
            )));
        }
        if let Some(t) = shares
            .iter()
            .find(|s| others.iter().any(|r| r.source == s.center))
        {
            return Err(other(format!(
                "{} también se reparte: los repartos secundarios no se encadenan",
                t.center
            )));
        }
    }
    tx.execute(
        "DELETE FROM cost_redistribution WHERE source = ?1",
        [&source],
    )?;
    for s in shares {
        tx.execute(
            "INSERT INTO cost_redistribution(source, target, pct_bp) VALUES (?1, ?2, ?3)",
            params![source, s.center, s.pct_bp],
        )?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "analitica.reparto_secundario",
        &source,
        &serde_json::json!({ "shares": shares }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Borra un centro (y sus repartos).
pub fn delete_center(v: &Vault, code: &str, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let n = v
        .connection()
        .execute("DELETE FROM cost_center WHERE code = ?1", [code])?;
    if n == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

fn check_prefix(p: &str) -> Result<()> {
    if p.is_empty()
        || p.len() > 12
        || !p.bytes().all(|b| b.is_ascii_digit())
        || !matches!(p.as_bytes()[0], b'6' | b'7')
    {
        return Err(other(
            "la analítica reparte gastos (6) e ingresos (7): pon una cuenta o grupo que empiece por 6 o 7",
        ));
    }
    Ok(())
}

fn check_shares(conn: &Connection, shares: &[Share]) -> Result<()> {
    let total: u32 = shares.iter().map(|s| u32::from(s.pct_bp)).sum();
    if total != u32::from(FULL) {
        return Err(other(format!(
            "los porcentajes suman {},{:02} % y han de sumar 100 %",
            total / 100,
            total % 100
        )));
    }
    for s in shares {
        if s.pct_bp == 0 || s.pct_bp > FULL {
            return Err(other("cada porcentaje va de 0,01 % a 100 %"));
        }
        let exists: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM cost_center WHERE code = ?1",
                [&s.center],
                |r| r.get(0),
            )
            .optional()?;
        if exists.is_none() {
            return Err(other(format!("no existe el centro {}", s.center)));
        }
    }
    let mut seen: Vec<&str> = shares.iter().map(|s| s.center.as_str()).collect();
    seen.sort_unstable();
    if seen.windows(2).any(|w| w[0] == w[1]) {
        return Err(other("un centro aparece dos veces"));
    }
    Ok(())
}

pub fn rules(v: &Vault) -> Result<Vec<Rule>> {
    let mut stmt = v
        .connection()
        .prepare("SELECT prefix, center, pct_bp FROM cost_rule ORDER BY prefix, center")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, u16>(2)?,
        ))
    })?;
    let mut map: BTreeMap<String, Vec<Share>> = BTreeMap::new();
    for row in rows {
        let (prefix, center, pct_bp) = row?;
        map.entry(prefix)
            .or_default()
            .push(Share { center, pct_bp });
    }
    Ok(map
        .into_iter()
        .map(|(prefix, shares)| Rule { prefix, shares })
        .collect())
}

/// Guarda la regla de reparto de una cuenta o grupo (sin partes, la borra).
pub fn set_rule(v: &Vault, prefix: &str, shares: &[Share], now: OffsetDateTime) -> Result<()> {
    let prefix = prefix.trim();
    check_prefix(prefix)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    if !shares.is_empty() {
        check_shares(&tx, shares)?;
    }
    tx.execute("DELETE FROM cost_rule WHERE prefix = ?1", [prefix])?;
    for s in shares {
        tx.execute(
            "INSERT INTO cost_rule(prefix, center, pct_bp) VALUES (?1, ?2, ?3)",
            params![prefix, s.center, s.pct_bp],
        )?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "analitica.regla",
        prefix,
        &serde_json::json!({ "shares": shares }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Reparte una línea del diario (sin partes, vuelve a la regla).
pub fn set_line(v: &Vault, line_id: i64, shares: &[Share], now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let account: String = tx
        .query_row(
            "SELECT account FROM journal_line WHERE id = ?1",
            [line_id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    check_prefix(&account)?;
    if !shares.is_empty() {
        check_shares(&tx, shares)?;
    }
    tx.execute("DELETE FROM line_cost WHERE line_id = ?1", [line_id])?;
    for s in shares {
        tx.execute(
            "INSERT INTO line_cost(line_id, center, pct_bp) VALUES (?1, ?2, ?3)",
            params![line_id, s.center, s.pct_bp],
        )?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "analitica.linea",
        &line_id.to_string(),
        &serde_json::json!({ "shares": shares }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Línea de gasto o ingreso con su reparto.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AnalyticLine {
    pub line_id: i64,
    pub date: Date,
    pub concept: String,
    pub account: String,
    /// Gasto (6) en positivo si es cargo; ingreso (7) en positivo si es abono.
    pub amount: Money,
    pub shares: Vec<Share>,
    /// `linea` (reparto propio), `regla` o `sin` (sin repartir).
    pub source: String,
}

/// Reparte `amount` según `shares`; el último se lleva los céntimos que sobren.
pub fn split(amount: Money, shares: &[Share]) -> Vec<(String, Money)> {
    let mut out = Vec::new();
    let mut given = 0i64;
    for (i, s) in shares.iter().enumerate() {
        let part = if i + 1 == shares.len() {
            amount.cents() - given
        } else {
            let p = (i128::from(amount.cents()) * i128::from(s.pct_bp)) / i128::from(FULL);
            i64::try_from(p).unwrap_or(0)
        };
        given += part;
        out.push((s.center.clone(), Money::from_cents(part)));
    }
    out
}

fn rule_for<'a>(rules: &'a [Rule], account: &str) -> Option<&'a Rule> {
    rules
        .iter()
        .filter(|r| account.starts_with(&r.prefix))
        .max_by_key(|r| r.prefix.len())
}

/// Gastos e ingresos del ejercicio (sin regularización ni cierre) entre dos fechas, con su
/// reparto.
pub fn lines(
    v: &Vault,
    year: i32,
    from: Option<Date>,
    to: Option<Date>,
) -> Result<Vec<AnalyticLine>> {
    let conn = v.connection();
    let rules = rules(v)?;
    let mut own: BTreeMap<i64, Vec<Share>> = BTreeMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT c.line_id, c.center, c.pct_bp FROM line_cost c ORDER BY c.line_id, c.center",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u16>(2)?,
            ))
        })?;
        for row in rows {
            let (id, center, pct_bp) = row?;
            own.entry(id).or_default().push(Share { center, pct_bp });
        }
    }
    let mut stmt = conn.prepare(
        "SELECT l.id, e.date, COALESCE(l.description, e.concept), l.account, l.debit, l.credit
         FROM journal_line l JOIN journal_entry e ON e.id = l.entry_id
         WHERE e.fiscal_year = ?1 AND e.origin_kind NOT IN ('Regularizacion', 'Cierre')
           AND (l.account LIKE '6%' OR l.account LIKE '7%')
           AND (?2 IS NULL OR e.date >= ?2) AND (?3 IS NULL OR e.date <= ?3)
         ORDER BY e.date, l.id",
    )?;
    let rows = stmt.query_map(
        params![year, from.map(|d| d.to_string()), to.map(|d| d.to_string())],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
            ))
        },
    )?;
    let mut out = Vec::new();
    for row in rows {
        let (id, date, concept, account, debit, credit) = row?;
        let amount = if account.starts_with('6') {
            debit - credit
        } else {
            credit - debit
        };
        let (shares, source) = match own.remove(&id) {
            Some(s) => (s, "linea"),
            None => match rule_for(&rules, &account) {
                Some(r) => (r.shares.clone(), "regla"),
                None => (Vec::new(), "sin"),
            },
        };
        out.push(AnalyticLine {
            line_id: id,
            date: Date::parse(
                &date,
                time::macros::format_description!("[year]-[month]-[day]"),
            )
            .map_err(|e| other(e.to_string()))?,
            concept,
            account,
            amount: Money::from_cents(amount),
            shares,
            source: source.into(),
        });
    }
    Ok(out)
}

/// Resultado de un centro (o de lo que está sin repartir, con `center` vacío).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CenterResult {
    pub center: String,
    pub name: String,
    pub income: Money,
    pub expenses: Money,
    pub result: Money,
}

/// Cuenta de resultados analítica: ingresos, gastos y resultado de cada centro, después del
/// reparto secundario. Con `level`, lo de cada centro se suma en su antecesor de ese nivel (los
/// de niveles superiores salen tal cual).
pub fn report(
    v: &Vault,
    year: i32,
    from: Option<Date>,
    to: Option<Date>,
    level: Option<usize>,
) -> Result<Vec<CenterResult>> {
    let all = centers(v)?;
    let names: BTreeMap<String, String> = all
        .iter()
        .map(|c| (c.code.clone(), c.name.clone()))
        .collect();
    let mut acc: BTreeMap<String, (Money, Money)> = BTreeMap::new();
    for c in names.keys() {
        acc.insert(c.clone(), (Money::ZERO, Money::ZERO));
    }
    for l in lines(v, year, from, to)? {
        let parts = if l.shares.is_empty() {
            vec![(String::new(), l.amount)]
        } else {
            split(l.amount, &l.shares)
        };
        for (center, m) in parts {
            let slot = acc.entry(center).or_insert((Money::ZERO, Money::ZERO));
            if l.account.starts_with('7') {
                slot.0 += m;
            } else {
                slot.1 += m;
            }
        }
    }
    // Reparto secundario: lo de cada centro que se reparte pasa a sus destinos.
    for r in redistributions(v)? {
        let Some((income, expenses)) = acc.get(&r.source).copied() else {
            continue;
        };
        for ((center, i), (_, e)) in split(income, &r.shares)
            .into_iter()
            .zip(split(expenses, &r.shares))
        {
            let slot = acc.entry(center).or_insert((Money::ZERO, Money::ZERO));
            slot.0 += i;
            slot.1 += e;
        }
        acc.insert(r.source, (Money::ZERO, Money::ZERO));
    }
    // Por niveles: cada centro suma en su antecesor del nivel pedido.
    if let Some(n) = level.filter(|n| *n >= 1) {
        let parents: BTreeMap<String, Option<String>> = all
            .iter()
            .map(|c| (c.code.clone(), c.parent.clone()))
            .collect();
        let mut rolled: BTreeMap<String, (Money, Money)> = BTreeMap::new();
        for (center, (i, e)) in acc {
            let key = if center.is_empty() {
                center
            } else {
                let ch = chain(&parents, &center);
                // ch: [centro, padre, …, raíz]; el de nivel n está a len − n desde el principio.
                ch.len()
                    .checked_sub(n)
                    .and_then(|k| ch.get(k).cloned())
                    .unwrap_or(center)
            };
            let slot = rolled.entry(key).or_insert((Money::ZERO, Money::ZERO));
            slot.0 += i;
            slot.1 += e;
        }
        acc = rolled;
    }
    let mut out: Vec<CenterResult> = acc
        .into_iter()
        .map(|(center, (income, expenses))| CenterResult {
            name: if center.is_empty() {
                "Sin repartir".into()
            } else {
                names.get(&center).cloned().unwrap_or_default()
            },
            center,
            income,
            expenses,
            result: income - expenses,
        })
        .collect();
    // «Sin repartir» al final.
    out.sort_by_key(|c| (c.center.is_empty(), c.center.clone()));
    if out
        .last()
        .is_some_and(|c| c.center.is_empty() && c.income.is_zero() && c.expenses.is_zero())
    {
        out.pop();
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reparto_sin_perder_centimos() {
        let s = |c: &str, p: u16| Share {
            center: c.into(),
            pct_bp: p,
        };
        let parts = split(
            Money::from_cents(1000),
            &[s("A", 3333), s("B", 3333), s("C", 3334)],
        );
        assert_eq!(
            parts.iter().map(|p| p.1.cents()).collect::<Vec<_>>(),
            vec![333, 333, 334]
        );
        let parts = split(Money::from_cents(-101), &[s("A", 5000), s("B", 5000)]);
        assert_eq!(parts.iter().map(|p| p.1.cents()).sum::<i64>(), -101);
    }
}
