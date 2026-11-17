//! Bloqueo de periodos (G46): un trimestre con el 303 presentado o un ejercicio cerrado se marcan
//! como «cerrados». Emitir una factura, registrar una recibida o crear, cambiar o borrar un
//! asiento con fecha de un periodo cerrado no se impide (la ley deja rectificar: facturas
//! rectificativas, art. 15 RD 1619/2012; autoliquidaciones complementarias o rectificación, arts.
//! 122 y 120.3 LGT), pero pide antes una confirmación explícita que queda en la auditoría.
//!
//! Cómo se confirma: la operación falla con un error que empieza por [`MARCA`] y el id del
//! periodo. La interfaz enseña el aviso, y si el usuario confirma llama a [`autorizar`], que
//! deja una excepción para ese periodo durante [`MINUTOS_EXCEPCION`] minutos (anotada en la
//! auditoría con el motivo), y repite la operación. Así el aviso vale para cualquier camino (la
//! interfaz, el asistente), no sólo para una pantalla.

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite;
use time::format_description::well_known::Rfc3339;
use time::{Date, Duration, Month, OffsetDateTime};

use crate::error::{AppError, Result};

/// Prefijo del error de periodo cerrado: `[periodo-cerrado:<id>] texto`.
pub const MARCA: &str = "[periodo-cerrado:";
/// Lo que dura la excepción que deja [`autorizar`].
pub const MINUTOS_EXCEPCION: i64 = 10;
const AJUSTE_EXCEPCION: &str = "periodo_excepcion";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tipo {
    Trimestre,
    Ejercicio,
}

impl Tipo {
    fn as_str(self) -> &'static str {
        match self {
            Tipo::Trimestre => "trimestre",
            Tipo::Ejercicio => "ejercicio",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Periodo {
    pub id: i64,
    pub tipo: Tipo,
    pub ejercicio: i32,
    pub trimestre: Option<u8>,
    pub desde: Date,
    pub hasta: Date,
    pub motivo: String,
    pub cerrado_en: String,
    /// «3T 2026» o «ejercicio 2026».
    pub etiqueta: String,
}

fn etiqueta(tipo: Tipo, ejercicio: i32, trimestre: Option<u8>) -> String {
    match (tipo, trimestre) {
        (Tipo::Trimestre, Some(t)) => format!("{t}T {ejercicio}"),
        _ => format!("ejercicio {ejercicio}"),
    }
}

fn now_str(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

fn fecha(s: &str) -> rusqlite::Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]")).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

const COLS: &str = "id, tipo, ejercicio, trimestre, desde, hasta, motivo, cerrado_en";

fn de_fila(r: &rusqlite::Row<'_>) -> rusqlite::Result<Periodo> {
    let tipo = if r.get::<_, String>(1)? == "trimestre" {
        Tipo::Trimestre
    } else {
        Tipo::Ejercicio
    };
    let ejercicio: i32 = r.get(2)?;
    let trimestre: Option<u8> = r.get(3)?;
    Ok(Periodo {
        id: r.get(0)?,
        tipo,
        ejercicio,
        trimestre,
        desde: fecha(&r.get::<_, String>(4)?)?,
        hasta: fecha(&r.get::<_, String>(5)?)?,
        motivo: r.get(6)?,
        cerrado_en: r.get(7)?,
        etiqueta: etiqueta(tipo, ejercicio, trimestre),
    })
}

/// Periodos cerrados, del más reciente al más antiguo.
pub fn listar(v: &Vault) -> Result<Vec<Periodo>> {
    let mut stmt = v.connection().prepare(&format!(
        "SELECT {COLS} FROM periodo_cerrado ORDER BY hasta DESC, tipo"
    ))?;
    let rows = stmt.query_map([], de_fila)?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Primer y último día del trimestre natural `t` de `year` (los del 303).
fn trimestre_natural(year: i32, t: u8) -> Result<(Date, Date)> {
    let (m1, m3) = match t {
        1 => (Month::January, Month::March),
        2 => (Month::April, Month::June),
        3 => (Month::July, Month::September),
        4 => (Month::October, Month::December),
        _ => return Err(AppError::Other("el trimestre va del 1 al 4".into())),
    };
    let ini = Date::from_calendar_date(year, m1, 1).map_err(|e| AppError::Other(e.to_string()))?;
    let fin = Date::from_calendar_date(year, m3, m3.length(year))
        .map_err(|e| AppError::Other(e.to_string()))?;
    Ok((ini, fin))
}

/// Cierra un periodo (con confirmación). Un trimestre es el natural (el del 303); un ejercicio,
/// el contable de la empresa (puede no ser el año natural). Queda en la auditoría.
pub fn cerrar(
    v: &Vault,
    tipo: Tipo,
    ejercicio: i32,
    trimestre: Option<u8>,
    motivo: &str,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<Periodo> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let (desde, hasta, trimestre) = match tipo {
        Tipo::Trimestre => {
            let t =
                trimestre.ok_or_else(|| AppError::Other("di qué trimestre se cierra".into()))?;
            let (a, b) = trimestre_natural(ejercicio, t)?;
            (a, b, Some(t))
        }
        Tipo::Ejercicio => {
            let fy = crate::accounting::fiscal_year(v, ejercicio)?;
            (fy.start, fy.end, None)
        }
    };
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let ya: Option<i64> = tx
        .query_row(
            "SELECT id FROM periodo_cerrado WHERE tipo = ?1 AND ejercicio = ?2
               AND coalesce(trimestre, 0) = coalesce(?3, 0)",
            params![tipo.as_str(), ejercicio, trimestre],
            |r| r.get(0),
        )
        .optional()?;
    if ya.is_some() {
        return Err(AppError::Other(format!(
            "el {} ya está cerrado",
            etiqueta(tipo, ejercicio, trimestre)
        )));
    }
    tx.execute(
        "INSERT INTO periodo_cerrado(tipo, ejercicio, trimestre, desde, hasta, motivo, cerrado_en)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            tipo.as_str(),
            ejercicio,
            trimestre,
            desde.to_string(),
            hasta.to_string(),
            motivo.trim(),
            now_str(now)
        ],
    )?;
    let id = tx.last_insert_rowid();
    let p = tx.query_row(
        &format!("SELECT {COLS} FROM periodo_cerrado WHERE id = ?1"),
        [id],
        de_fila,
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "periodo.cerrar",
        &p.etiqueta,
        &serde_json::json!({ "id": id, "desde": desde.to_string(), "hasta": hasta.to_string(),
                             "motivo": motivo.trim() }),
    )?;
    tx.commit()?;
    Ok(p)
}

/// Reabre un periodo cerrado (con confirmación). Queda en la auditoría.
pub fn reabrir(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let p = tx
        .query_row(
            &format!("SELECT {COLS} FROM periodo_cerrado WHERE id = ?1"),
            [id],
            de_fila,
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    tx.execute("DELETE FROM periodo_cerrado WHERE id = ?1", [id])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "periodo.reabrir",
        &p.etiqueta,
        &serde_json::json!({ "id": id, "desde": p.desde.to_string(), "hasta": p.hasta.to_string() }),
    )?;
    tx.commit()?;
    Ok(())
}

/// El periodo cerrado que contiene `fecha`, si hay alguno (el ejercicio antes que el trimestre).
pub fn cerrado_en(conn: &Connection, fecha: Date) -> Result<Option<Periodo>> {
    let d = fecha.to_string();
    Ok(conn
        .query_row(
            &format!(
                "SELECT {COLS} FROM periodo_cerrado WHERE desde <= ?1 AND hasta >= ?1
                 ORDER BY CASE tipo WHEN 'ejercicio' THEN 0 ELSE 1 END LIMIT 1"
            ),
            [d],
            de_fila,
        )
        .optional()?)
}

#[derive(Serialize, Deserialize)]
struct Excepcion {
    /// Periodos con excepción y hasta cuándo vale cada una.
    periodos: Vec<(i64, String)>,
}

fn excepciones(conn: &Connection) -> Result<Excepcion> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [AJUSTE_EXCEPCION],
            |r| r.get(0),
        )
        .optional()?;
    Ok(raw
        .and_then(|r| serde_json::from_str(&r).ok())
        .unwrap_or(Excepcion {
            periodos: Vec::new(),
        }))
}

fn vigente(e: &Excepcion, id: i64, now: OffsetDateTime) -> bool {
    e.periodos.iter().any(|(p, hasta)| {
        *p == id && OffsetDateTime::parse(hasta, &Rfc3339).is_ok_and(|h| now <= h)
    })
}

/// Texto del aviso de un periodo cerrado para `que` («emitir una factura con fecha 2026-05-04»).
pub fn aviso(p: &Periodo, que: &str) -> String {
    let motivo = if p.motivo.is_empty() {
        String::new()
    } else {
        format!(" ({})", p.motivo)
    };
    format!(
        "{MARCA}{}] El {}{motivo} está cerrado y vas a {que}. Se puede hacer, pero cambia lo ya \
         declarado: si afecta a un modelo presentado tendrás que presentar una complementaria o \
         pedir la rectificación (arts. 122 y 120.3 LGT), y una factura emitida no se cambia: se \
         corrige con una rectificativa (art. 15 RD 1619/2012).",
        p.id, p.etiqueta
    )
}

/// Falla con el aviso de [`aviso`] si `fecha` cae en un periodo cerrado sin excepción vigente.
/// Para llamar al principio de lo que emite, registra o cambia algo con fecha.
pub fn exigir_abierto(
    conn: &Connection,
    fecha: Date,
    que: &str,
    now: OffsetDateTime,
) -> Result<()> {
    let Some(p) = cerrado_en(conn, fecha)? else {
        return Ok(());
    };
    if vigente(&excepciones(conn)?, p.id, now) {
        return Ok(());
    }
    Err(AppError::Other(aviso(&p, que)))
}

/// El usuario ha visto el aviso y confirma que sigue: excepción para el periodo `id` durante
/// [`MINUTOS_EXCEPCION`] minutos, anotada en la auditoría con lo que iba a hacer.
pub fn autorizar(v: &Vault, id: i64, que: &str, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let p = tx
        .query_row(
            &format!("SELECT {COLS} FROM periodo_cerrado WHERE id = ?1"),
            [id],
            de_fila,
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let mut e = excepciones(&tx)?;
    e.periodos.retain(|(pid, hasta)| {
        *pid != id && OffsetDateTime::parse(hasta, &Rfc3339).is_ok_and(|h| now <= h)
    });
    e.periodos
        .push((id, now_str(now + Duration::minutes(MINUTOS_EXCEPCION))));
    let raw = serde_json::to_string(&e).map_err(|e| AppError::Other(e.to_string()))?;
    tx.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![AJUSTE_EXCEPCION, raw],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "periodo.excepcion",
        &p.etiqueta,
        &serde_json::json!({ "id": id, "que": que.chars().take(300).collect::<String>(),
                             "minutos": MINUTOS_EXCEPCION }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Periodo que conviene cerrar: un trimestre con el 303 presentado o un ejercicio cerrado en la
/// contabilidad que aún no están marcados.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Sugerido {
    pub tipo: Tipo,
    pub ejercicio: i32,
    pub trimestre: Option<u8>,
    pub etiqueta: String,
    pub motivo: String,
}

/// Lo que se sugiere cerrar (ver [`Sugerido`]).
pub fn sugeridos(v: &Vault) -> Result<Vec<Sugerido>> {
    let conn = v.connection();
    let mut out = Vec::new();
    // 303 presentados: por TaxPain (en producción), consultados en la AEAT o importados.
    let mut stmt = conn.prepare(
        "SELECT DISTINCT year, period FROM (
           SELECT year, period FROM tax_return WHERE model = '303' AND environment = 'produccion'
           UNION SELECT year, period FROM aeat_filing WHERE model = '303'
           UNION SELECT year, period FROM filed_model WHERE model = '303')
         WHERE period IN ('1T', '2T', '3T', '4T')
           AND NOT EXISTS (SELECT 1 FROM periodo_cerrado c WHERE c.tipo = 'trimestre'
                           AND c.ejercicio = year AND c.trimestre = CAST(substr(period, 1, 1) AS INTEGER))
         ORDER BY year DESC, period DESC",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i32>(0)?, r.get::<_, String>(1)?)))?;
    for row in rows {
        let (year, period) = row?;
        let t = period
            .chars()
            .next()
            .and_then(|c| c.to_digit(10))
            .and_then(|d| u8::try_from(d).ok());
        out.push(Sugerido {
            tipo: Tipo::Trimestre,
            ejercicio: year,
            trimestre: t,
            etiqueta: etiqueta(Tipo::Trimestre, year, t),
            motivo: "303 presentado".into(),
        });
    }
    let mut stmt = conn.prepare(
        "SELECT year FROM fiscal_year WHERE closed = 1 AND NOT EXISTS (
           SELECT 1 FROM periodo_cerrado c WHERE c.tipo = 'ejercicio' AND c.ejercicio = year)
         ORDER BY year DESC",
    )?;
    let years = stmt.query_map([], |r| r.get::<_, i32>(0))?;
    for y in years {
        let y = y?;
        out.push(Sugerido {
            tipo: Tipo::Ejercicio,
            ejercicio: y,
            trimestre: None,
            etiqueta: etiqueta(Tipo::Ejercicio, y, None),
            motivo: "ejercicio cerrado en la contabilidad".into(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider};
    use time::macros::{date, datetime};

    fn vault() -> Vault {
        Vault::open_in_memory(
            Arc::new(MemoryKeyProvider::default()),
            Arc::new(AlwaysConfirm),
        )
        .unwrap()
    }

    const AHORA: OffsetDateTime = datetime!(2026-11-17 10:00 UTC);

    #[test]
    fn cerrar_un_trimestre_avisa_en_sus_fechas_y_no_fuera() {
        let v = vault();
        assert!(matches!(
            cerrar(&v, Tipo::Trimestre, 2026, Some(3), "", AHORA, false),
            Err(AppError::NotConfirmed)
        ));
        let p = cerrar(
            &v,
            Tipo::Trimestre,
            2026,
            Some(3),
            "303 presentado",
            AHORA,
            true,
        )
        .unwrap();
        assert_eq!(
            (p.desde, p.hasta),
            (date!(2026 - 07 - 01), date!(2026 - 09 - 30))
        );
        assert_eq!(p.etiqueta, "3T 2026");
        let c = v.connection();
        assert!(exigir_abierto(c, date!(2026 - 06 - 30), "x", AHORA).is_ok());
        assert!(exigir_abierto(c, date!(2026 - 10 - 01), "x", AHORA).is_ok());
        let e = exigir_abierto(c, date!(2026 - 08 - 15), "emitir una factura", AHORA)
            .unwrap_err()
            .to_string();
        assert!(e.starts_with(&format!("{MARCA}{}]", p.id)), "{e}");
        assert!(e.contains("rectificativa"));
        assert!(
            cerrar(&v, Tipo::Trimestre, 2026, Some(3), "", AHORA, true).is_err(),
            "no se cierra dos veces"
        );
        assert_eq!(listar(&v).unwrap().len(), 1);
    }

    #[test]
    fn la_excepcion_vale_unos_minutos_y_queda_en_la_auditoria() {
        let v = vault();
        let p = cerrar(&v, Tipo::Trimestre, 2026, Some(1), "", AHORA, true).unwrap();
        let d = date!(2026 - 02 - 10);
        autorizar(&v, p.id, "registrar un asiento", AHORA).unwrap();
        let c = v.connection();
        assert!(exigir_abierto(c, d, "x", AHORA + Duration::minutes(5)).is_ok());
        assert!(exigir_abierto(c, d, "x", AHORA + Duration::minutes(11)).is_err());
        let log = taxpain_vault::audit::entries(c).unwrap();
        let acciones: Vec<&str> = log.iter().map(|e| e.action.as_str()).collect();
        assert!(acciones.contains(&"periodo.cerrar"));
        assert!(acciones.contains(&"periodo.excepcion"));
        reabrir(&v, p.id, AHORA, true).unwrap();
        assert!(exigir_abierto(c, d, "x", AHORA + Duration::minutes(30)).is_ok());
        assert!(
            taxpain_vault::audit::entries(c)
                .unwrap()
                .iter()
                .any(|e| e.action == "periodo.reabrir")
        );
    }

    #[test]
    fn el_ejercicio_usa_las_fechas_de_la_contabilidad_y_se_sugiere_el_303() {
        let v = vault();
        let p = cerrar(&v, Tipo::Ejercicio, 2025, None, "", AHORA, true).unwrap();
        assert_eq!(
            (p.desde, p.hasta),
            (date!(2025 - 01 - 01), date!(2025 - 12 - 31))
        );
        v.connection()
            .execute(
                "INSERT INTO filed_model(model, year, period, filename, file_sha256, source, boxes,
                   imported_at)
                 VALUES ('303', 2026, '2T', '303.pdf', 'abc', 'manual', '{}', '2026-07-20')",
                [],
            )
            .unwrap();
        let s = sugeridos(&v).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].etiqueta, "2T 2026");
        cerrar(&v, Tipo::Trimestre, 2026, Some(2), "", AHORA, true).unwrap();
        assert!(sugeridos(&v).unwrap().is_empty());
    }
}
