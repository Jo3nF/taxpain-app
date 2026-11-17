//! Libro registro de contratos del socio único con la sociedad (art. 16 LSC).
//!
//! «Los contratos celebrados entre el socio único y la sociedad deberán constar por escrito o
//! en la forma documental que exija la ley de acuerdo con su naturaleza, y se transcribirán a un
//! libro-registro de la sociedad que habrá de ser legalizado conforme a lo dispuesto para los
//! libros de actas de las sociedades» (art. 16.1). En la memoria anual se hace referencia
//! expresa e individualizada a ellos (art. 16.2) y, en el concurso del socio único o de la
//! sociedad, los no transcritos ni referenciados en la memoria no son oponibles a la masa
//! (art. 16.3).
//!
//! TaxPain propone como contratos a transcribir lo que ya conoce con el socio único:
//! - las nóminas que la sociedad le paga (retribución como administrador o trabajador);
//! - los dividendos que cobra (son decisiones del socio único que constan en el libro de actas,
//!   pero conviene transcribirlos si hay pago aplazado o compensación).
//!
//! - los préstamos de Contabilidad → Préstamos concedidos al socio único o recibidos de él
//!   (por su NIF o por el socio elegido en la ficha) que empiezan en el ejercicio: fecha,
//!   principal y condiciones (plazo, interés, cuotas, retención), con `source = "prestamo:<id>"`.

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::prestamo::Sentido;
use taxpain_core::money::Money;
use taxpain_invoicing::pdf::Para;
use taxpain_models::m184::FULL_PCT;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::params;
use time::{Date, OffsetDateTime};

use super::{company, dmy, other, parse_date};
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

/// Un contrato transcrito.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoleContract {
    #[serde(default)]
    pub id: i64,
    pub date: Date,
    /// Objeto: «Préstamo del socio a la sociedad», «Arrendamiento del local de la calle…».
    pub object: String,
    #[serde(default)]
    pub amount: Option<Money>,
    /// Condiciones: plazo, interés, garantías, forma de pago…
    #[serde(default)]
    pub conditions: String,
    /// Documento en que consta: contrato privado de fecha…, escritura ante el notario…
    #[serde(default)]
    pub document: String,
    /// Propuesta de TaxPain de la que sale (`nominas:2026:12345678Z`, `dividendo:3`).
    #[serde(default)]
    pub source: Option<String>,
}

/// Socio único según la ficha de socios: el único activo, con el 100 %.
pub fn sole_partner(v: &Vault) -> Result<Option<(String, String)>> {
    let active: Vec<_> = crate::socios::shareholders(v)?
        .into_iter()
        .filter(|s| s.active)
        .collect();
    Ok(match active.as_slice() {
        [s] if s.pct == FULL_PCT => Some((s.nif.clone(), s.name.clone())),
        _ => None,
    })
}

pub fn contracts(v: &Vault) -> Result<Vec<SoleContract>> {
    let mut st = v.connection().prepare(
        "SELECT id, date, object, amount, conditions, document, source
           FROM sole_partner_contract ORDER BY date, id",
    )?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<i64>>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, Option<String>>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, date, object, amount, conditions, document, source) = row?;
        out.push(SoleContract {
            id,
            date: parse_date(&date)?,
            object,
            amount: amount.map(Money::from_cents),
            conditions,
            document,
            source,
        });
    }
    Ok(out)
}

/// Transcribe (id 0) o corrige un contrato.
pub fn save_contract(v: &Vault, c: &SoleContract, now: OffsetDateTime) -> Result<i64> {
    company(v)?;
    let object = c.object.trim();
    if object.is_empty() {
        return Err(other("pon el objeto del contrato"));
    }
    if c.amount.is_some_and(|a| a.is_negative()) {
        return Err(other("el importe no puede ser negativo"));
    }
    let source = c.source.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let id = if c.id == 0 {
        tx.execute(
            "INSERT INTO sole_partner_contract(date, object, amount, conditions, document, source,
               created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                c.date.to_string(),
                object,
                c.amount.map(Money::cents),
                c.conditions.trim(),
                c.document.trim(),
                source,
                now_str(now)
            ],
        )
        .map_err(|e| {
            if e.to_string().contains("UNIQUE") {
                other("esa propuesta ya está transcrita")
            } else {
                e.into()
            }
        })?;
        tx.last_insert_rowid()
    } else {
        let n = tx.execute(
            "UPDATE sole_partner_contract SET date = ?1, object = ?2, amount = ?3,
               conditions = ?4, document = ?5 WHERE id = ?6",
            params![
                c.date.to_string(),
                object,
                c.amount.map(Money::cents),
                c.conditions.trim(),
                c.document.trim(),
                c.id
            ],
        )?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        c.id
    };
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "sole_contract.save",
        &id.to_string(),
        &serde_json::json!({ "date": c.date.to_string(), "amount": c.amount.map(Money::cents) }),
    )?;
    tx.commit()?;
    Ok(id)
}

pub fn delete_contract(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    if tx.execute("DELETE FROM sole_partner_contract WHERE id = ?1", [id])? == 0 {
        return Err(AppError::NotFound);
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "sole_contract.delete",
        &id.to_string(),
        &serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

/// Propuestas de contratos a transcribir en el ejercicio.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Proposals {
    /// NIF y nombre del socio único (`None` si la sociedad no es unipersonal según la ficha).
    pub sole_partner: Option<(String, String)>,
    pub proposals: Vec<SoleContract>,
}

pub fn proposals(v: &Vault, year: i32) -> Result<Proposals> {
    company(v)?;
    let Some((nif, name)) = sole_partner(v)? else {
        return Ok(Proposals {
            sole_partner: None,
            proposals: vec![],
        });
    };
    let fy = crate::accounting::fiscal_year(v, year)?;
    let (start, end) = (fy.start.to_string(), fy.end.to_string());
    let taken: Vec<String> = contracts(v)?.into_iter().filter_map(|c| c.source).collect();
    let conn = v.connection();
    let mut out = Vec::new();

    // Nóminas del socio único (como administrador o como trabajador).
    let pay: Option<(i64, String, String, i64, bool)> = conn
        .query_row(
            "SELECT COUNT(*), MIN(p.date), MAX(p.date), SUM(p.gross), MAX(e.administrator)
               FROM payslip p JOIN employee e ON e.id = p.employee_id
              WHERE upper(e.nif) = upper(?1) AND p.date BETWEEN ?2 AND ?3
             HAVING COUNT(*) > 0",
            params![nif, start, end],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .map(Some)
        .or_else(|e| match e {
            taxpain_vault::rusqlite::Error::QueryReturnedNoRows => Ok(None),
            e => Err(e),
        })?;
    if let Some((n, first, last, gross, admin)) = pay {
        let source = format!("nominas:{year}:{nif}");
        if !taken.contains(&source) {
            let first = parse_date(&first)?;
            let last = parse_date(&last)?;
            out.push(SoleContract {
                id: 0,
                date: first,
                object: if admin {
                    format!("Retribución de {name}, socio único, por el ejercicio del cargo de administrador")
                } else {
                    format!("Relación laboral de {name}, socio único, con la sociedad")
                },
                amount: Some(Money::from_cents(gross)),
                conditions: format!(
                    "{n} recibos de salarios del {} al {} (ejercicio {year}), retribución bruta \
                     total.{}",
                    dmy(first),
                    dmy(last),
                    if admin {
                        " Importe máximo anual aprobado por el socio único (art. 217.3 LSC): \
                         [importe y fecha de la decisión]."
                    } else {
                        ""
                    }
                ),
                document: "[Contrato o decisión del socio único que lo recoge, con su fecha]"
                    .into(),
                source: Some(source),
            });
        }
    }

    // Dividendos cobrados por el socio único.
    let mut st = conn.prepare(
        "SELECT p.id, p.date, l.gross, l.withheld FROM dividend_payment p
           JOIN dividend_line l ON l.payment_id = p.id
          WHERE upper(l.nif) = upper(?1) AND p.date BETWEEN ?2 AND ?3 ORDER BY p.date, p.id",
    )?;
    let rows = st.query_map(params![nif, start, end], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })?;
    for row in rows {
        let (id, date, gross, withheld) = row?;
        let source = format!("dividendo:{id}");
        if taken.contains(&source) {
            continue;
        }
        out.push(SoleContract {
            id: 0,
            date: parse_date(&date)?,
            object: format!("Dividendo a favor de {name}, socio único"),
            amount: Some(Money::from_cents(gross)),
            conditions: format!(
                "Bruto {}, retención {} (modelo 123), exigible el {}.",
                Money::from_cents(gross),
                Money::from_cents(withheld),
                dmy(parse_date(&date)?)
            ),
            document: "Decisión del socio único (libro de actas)".into(),
            source: Some(source),
        });
    }

    // Préstamos con el socio único que empiezan en el ejercicio.
    let key = crate::vinculadas::clave_nif(&nif);
    for d in crate::prestamos::del_ejercicio(conn, fy.start, fy.end)? {
        let p = &d.prestamo;
        let source = format!("prestamo:{}", p.id);
        if p.inicio < fy.start
            || crate::vinculadas::clave_nif(&d.nif) != key
            || taken.contains(&source)
        {
            continue;
        }
        out.push(SoleContract {
            id: 0,
            date: p.inicio,
            object: match p.sentido {
                Sentido::Concedido => format!("Préstamo de la sociedad a {name}, socio único"),
                Sentido::Recibido => format!("Préstamo de {name}, socio único, a la sociedad"),
            },
            amount: Some(p.principal),
            conditions: crate::prestamos::condiciones(p),
            document: format!(
                "Contrato de préstamo de {} (el borrador sale de Contabilidad → Préstamos)",
                dmy(p.inicio)
            ),
            source: Some(source),
        });
    }
    Ok(Proposals {
        sole_partner: Some((nif, name)),
        proposals: out,
    })
}

/// El libro en párrafos (los del ejercicio, o todos si `year` es `None`).
pub fn book_paras(v: &Vault, year: Option<i32>, number: Option<u16>) -> Result<Vec<Para>> {
    let c = company(v)?;
    let range: Option<(Date, Date)> = match year {
        Some(y) => {
            let fy = crate::accounting::fiscal_year(v, y)?;
            Some((fy.start, fy.end))
        }
        None => None,
    };
    let list: Vec<SoleContract> = contracts(v)?
        .into_iter()
        .filter(|k| range.is_none_or(|(s, e)| k.date >= s && k.date <= e))
        .collect();
    let mut p = vec![Para::Title(match number {
        Some(n) => format!("Libro registro de contratos del socio único nº {n}"),
        None => "Libro registro de contratos del socio único".into(),
    })];
    p.push(Para::Text(format!("{} · NIF {}", c.name, c.nif)));
    if let (Some(y), Some((s, e))) = (year, range) {
        p.push(Para::Text(format!(
            "Ejercicio {y}: del {} al {}",
            dmy(s),
            dmy(e)
        )));
    }
    if let Some((nif, name)) = sole_partner(v)? {
        p.push(Para::Text(format!("Socio único: {name} (NIF {nif})")));
    }
    p.push(Para::Text(
        "Contratos celebrados entre el socio único y la sociedad (art. 16 de la Ley de \
         Sociedades de Capital)."
            .into(),
    ));
    p.push(Para::Gap);
    if list.is_empty() {
        p.push(Para::Text("Sin contratos en el periodo.".into()));
    }
    for (i, k) in list.iter().enumerate() {
        p.push(Para::Bold(format!(
            "Contrato {} · {} · {}",
            i + 1,
            dmy(k.date),
            k.object
        )));
        if let Some(a) = k.amount {
            p.push(Para::Row("Importe".into(), a.to_string()));
        }
        if !k.conditions.is_empty() {
            p.push(Para::Text(format!("Condiciones: {}", k.conditions)));
        }
        if !k.document.is_empty() {
            p.push(Para::Text(format!("Documento: {}", k.document)));
        }
        p.push(Para::Gap);
    }
    p.push(Para::Text(
        "El órgano de administración: ______________________________".into(),
    ));
    Ok(p)
}

/// Contratos con fecha dentro de `start..=end`.
pub(crate) fn contracts_between(v: &Vault, start: Date, end: Date) -> Result<usize> {
    Ok(contracts(v)?
        .iter()
        .filter(|c| c.date >= start && c.date <= end)
        .count())
}
