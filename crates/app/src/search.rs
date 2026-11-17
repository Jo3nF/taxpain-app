//! Búsqueda global (⌘K): facturas emitidas y recibidas, clientes y proveedores, cuentas,
//! asientos y documentos de la empresa, por texto o por importe. Todo en local, en la BD cifrada.

use serde::Serialize;
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::rusqlite::{Connection, params};

use crate::error::Result;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Hit {
    /// `emitida`, `recibida`, `contacto`, `cuenta`, `asiento` o `documento`.
    pub kind: String,
    pub title: String,
    pub subtitle: String,
    /// Id de la factura, asiento o documento; código de la cuenta; clave del contacto.
    pub target: String,
    /// Fecha (ISO) si la tiene, para ordenar.
    pub date: Option<String>,
}

/// Importe escrito en la búsqueda («1.210», «1210,50», «1210.5»), en céntimos.
fn amount(q: &str) -> Option<i64> {
    let t = q.trim().trim_end_matches('€').trim();
    if t.is_empty()
        || !t
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
    {
        return None;
    }
    let (int, dec) = match t.rsplit_once(',') {
        Some((i, d)) => (i.replace('.', ""), d.to_owned()),
        None => match t.rsplit_once('.') {
            // «1.210» son miles; «1210.5» decimales.
            Some((i, d)) if d.len() == 3 => (format!("{i}{d}").replace('.', ""), String::new()),
            Some((i, d)) => (i.replace('.', ""), d.to_owned()),
            None => (t.to_owned(), String::new()),
        },
    };
    if dec.len() > 2 || int.is_empty() {
        return None;
    }
    let euros: i64 = int.parse().ok()?;
    let cents: i64 = if dec.is_empty() {
        0
    } else {
        format!("{dec:0<2}").parse().ok()?
    };
    euros.checked_mul(100)?.checked_add(cents)
}

fn like(q: &str) -> String {
    let esc = q
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{esc}%")
}

fn query(
    conn: &Connection,
    sql: &str,
    p: impl taxpain_vault::rusqlite::Params,
    kind: &str,
) -> Result<Vec<Hit>> {
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(p, |r| {
        Ok(Hit {
            kind: kind.to_owned(),
            title: r.get(0)?,
            subtitle: r.get(1)?,
            target: r.get(2)?,
            date: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Resultados para `q` (como mucho `limit` de cada tipo).
pub fn search(v: &Vault, q: &str, limit: usize) -> Result<Vec<Hit>> {
    let q = q.trim();
    if q.chars().count() < 2 {
        return Ok(vec![]);
    }
    let conn = v.connection();
    let l = like(q);
    let cents = amount(q);
    let n = i64::try_from(limit).unwrap_or(10);
    let money = |c: i64| Money::from_cents(c).to_string();
    let mut out = Vec::new();

    // Facturas emitidas: número, cliente, NIF o importe total.
    let mut stmt = conn.prepare(
        "SELECT id, number, issue_date, coalesce(recipient_name, ''), total, status
           FROM invoice_issued
          WHERE number LIKE ?1 ESCAPE '\\' OR recipient_name LIKE ?1 ESCAPE '\\'
             OR recipient_tax_id LIKE ?1 ESCAPE '\\' OR total = ?2
          ORDER BY issue_date DESC LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![l, cents.unwrap_or(i64::MIN), n], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, String>(5)?,
        ))
    })?;
    for row in rows {
        let (id, number, date, name, total, status) = row?;
        out.push(Hit {
            kind: "emitida".into(),
            title: format!("{number} · {name}"),
            subtitle: format!(
                "Factura emitida · {} · {}{}",
                date,
                money(total),
                if status == "anulada" {
                    " · anulada"
                } else {
                    ""
                }
            ),
            target: id.to_string(),
            date: Some(date),
        });
    }

    // Recibidas.
    let mut stmt = conn.prepare(
        "SELECT id, coalesce(number, filename), coalesce(issue_date, ''), coalesce(supplier_name, ''),
                coalesce(total, 0), status
           FROM invoice_received
          WHERE status != 'descartada' AND (number LIKE ?1 ESCAPE '\\'
             OR supplier_name LIKE ?1 ESCAPE '\\' OR supplier_nif LIKE ?1 ESCAPE '\\'
             OR filename LIKE ?1 ESCAPE '\\' OR total = ?2)
          ORDER BY issue_date DESC LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![l, cents.unwrap_or(i64::MIN), n], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, String>(5)?,
        ))
    })?;
    for row in rows {
        let (id, number, date, name, total, status) = row?;
        out.push(Hit {
            kind: "recibida".into(),
            title: format!("{number} · {name}"),
            subtitle: format!(
                "Factura recibida · {} · {}{}",
                if date.is_empty() { "sin fecha" } else { &date },
                money(total),
                if status == "pendiente" {
                    " · por revisar"
                } else {
                    ""
                }
            ),
            target: id.to_string(),
            date: (!date.is_empty()).then_some(date),
        });
    }

    // Clientes y proveedores (con ficha o con facturas).
    for s in crate::contacts::search(v, q, limit)? {
        out.push(Hit {
            kind: "contacto".into(),
            subtitle: format!("Cliente · {}", s.key),
            title: s.party.name,
            target: s.key,
            date: None,
        });
    }
    // Proveedores con ficha, aunque aún no tengan facturas revisadas.
    let qn: String = q
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_uppercase();
    let mut stmt = conn.prepare(
        "SELECT name, tax_id FROM contact
          WHERE kind = 'proveedor'
            AND (name LIKE ?1 ESCAPE '\\' OR (?2 != '' AND tax_id LIKE ?3 ESCAPE '\\'))
          ORDER BY name LIMIT ?4",
    )?;
    let rows = stmt.query_map(params![l, qn, like(&qn), n], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (name, key) = row?;
        if out.iter().any(|h| h.kind == "contacto" && h.target == key) {
            continue;
        }
        out.push(Hit {
            kind: "contacto".into(),
            title: name,
            subtitle: format!("Proveedor · {key}"),
            target: key,
            date: None,
        });
    }
    let mut stmt = conn.prepare(
        "SELECT supplier_name, supplier_nif, coalesce(supplier_country, 'ES') FROM invoice_received
          WHERE status = 'revisada' AND supplier_nif IS NOT NULL
            AND (supplier_name LIKE ?1 ESCAPE '\\' OR supplier_nif LIKE ?1 ESCAPE '\\')
          GROUP BY supplier_nif LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![l, n], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (name, nif, country) = row?;
        let key = crate::contacts::key(&country, &nif);
        if out.iter().any(|h| h.kind == "contacto" && h.target == key) {
            continue;
        }
        out.push(Hit {
            kind: "contacto".into(),
            title: name.unwrap_or_else(|| nif.clone()),
            subtitle: format!("Proveedor · {key}"),
            target: key,
            date: None,
        });
    }

    // Cuentas: las del PGC, las propias y las de clientes y proveedores.
    {
        use std::collections::BTreeMap;
        let mut accounts: BTreeMap<String, String> = taxpain_core::accounting::pgc::ACCOUNTS
            .iter()
            .map(|(c, n)| ((*c).to_owned(), (*n).to_owned()))
            .collect();
        for sql in [
            "SELECT code, name FROM account",
            "SELECT account, name FROM third_party_account",
        ] {
            let mut stmt = conn.prepare(sql)?;
            let rows =
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
            for row in rows {
                let (c, n) = row?;
                accounts.insert(c, n);
            }
        }
        let ql = q.to_lowercase();
        let digits = q.chars().all(|c| c.is_ascii_digit());
        out.extend(
            accounts
                .into_iter()
                .filter(|(c, n)| {
                    if digits {
                        c.starts_with(q)
                    } else {
                        n.to_lowercase().contains(&ql)
                    }
                })
                .take(limit)
                .map(|(c, n)| Hit {
                    kind: "cuenta".into(),
                    title: format!("{c} {n}"),
                    subtitle: "Cuenta".into(),
                    target: c,
                    date: None,
                }),
        );
    }

    // Asientos por concepto o importe.
    let mut stmt = conn.prepare(
        "SELECT e.id, e.fiscal_year, e.number, e.date, e.concept,
                (SELECT sum(debit) FROM journal_line WHERE entry_id = e.id)
           FROM journal_entry e
          WHERE e.concept LIKE ?1 ESCAPE '\\'
             OR e.id IN (SELECT entry_id FROM journal_line WHERE debit = ?2 OR credit = ?2)
          ORDER BY e.date DESC LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![l, cents.unwrap_or(i64::MIN), n], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, Option<i64>>(5)?,
        ))
    })?;
    for row in rows {
        let (_id, year, number, date, concept, total) = row?;
        out.push(Hit {
            kind: "asiento".into(),
            title: concept,
            subtitle: format!(
                "Asiento {number}/{year} · {date} · {}",
                money(total.unwrap_or(0))
            ),
            // Número y ejercicio (para abrir el diario de ese año en ese asiento).
            target: format!("{number}:{year}"),
            date: Some(date),
        });
    }

    // Documentos de la empresa (escrituras, NIF…).
    out.extend(query(
        conn,
        "SELECT title, 'Documento · ' || filename, CAST(id AS TEXT), NULL FROM company_document
          WHERE title LIKE ?1 ESCAPE '\\' OR filename LIKE ?1 ESCAPE '\\'
          ORDER BY id DESC LIMIT ?2",
        params![l, n],
        "documento",
    )?);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn importes() {
        assert_eq!(amount("1.210"), Some(121_000));
        assert_eq!(amount("1210,5"), Some(121_050));
        assert_eq!(amount("1.210,50 €"), Some(121_050));
        assert_eq!(amount("1210.5"), Some(121_050));
        assert_eq!(amount("12"), Some(1200));
        assert_eq!(amount("A-12"), None);
        assert_eq!(amount("1,234,5"), None);
        assert_eq!(like("50%_x"), "%50\\%\\_x%");
    }
}
