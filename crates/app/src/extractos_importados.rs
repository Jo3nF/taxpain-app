//! Los extractos importados de cada cuenta: la lista (fichero, periodo, movimientos y saldos),
//! los huecos entre ellos y deshacer uno importado por error.
//!
//! - Cada importación de un extracto queda en `bank_statement` con los movimientos que traía
//!   (`bank_statement_tx`): los que entraron nuevos y los que ya estaban por otro extracto que se
//!   solapa. Los importados antes de la migración 0091 se agrupan por su fichero de origen
//!   (`bank_tx.source`).
//! - **Deshacer** un extracto borra sólo sus movimientos propios (los que no trae ningún otro
//!   extracto); los compartidos se quedan con el otro. Si alguno está conciliado, no se deshace
//!   salvo que se pida desconciliarlo: entonces se deshace cada conciliación como en «Movimientos
//!   conciliados» ([`crate::conciliados::unreconcile`]), con sus mismos bloqueos (ejercicio
//!   cerrado, modelo presentado…), y después se borran.
//! - **Huecos**: entre dos extractos seguidos de la misma cuenta que no se solapan, si los saldos
//!   no enlazan (el final de uno no es el inicial del siguiente) faltan movimientos seguro; si no
//!   se conocen los saldos, se avisa cuando pasan más de [`DIAS_SIN_SALDOS`] días.
//!
//! Norma: art. 25 del Código de Comercio (contabilidad ordenada) y art. 29.2.d LGT (llevar los
//! registros): un periodo sin extracto es un periodo cuyo banco no se ha comprobado.

use serde::Serialize;
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, Duration, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::extractos::Extracto;
use crate::invoicing::now_str;

/// Días entre dos extractos sin saldos a partir de los cuales se avisa de un posible hueco (un
/// extracto sin periodo propio va del primer al último movimiento: unos días sin movimientos no
/// son un hueco).
pub const DIAS_SIN_SALDOS: i64 = 7;

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

fn parse_date(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]"))
        .map_err(|e| other(e.to_string()))
}

fn dmy(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

/// Un extracto importado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ExtractoGuardado {
    pub id: i64,
    pub cuenta_id: i64,
    /// Nombre de la cuenta en la app.
    pub cuenta: String,
    pub divisa: String,
    pub fichero: String,
    pub formato: String,
    pub desde: Option<Date>,
    pub hasta: Option<Date>,
    pub saldo_inicial: Option<Money>,
    pub saldo_final: Option<Money>,
    /// Movimientos que traía el extracto.
    pub movimientos: usize,
    /// Los que entraron nuevos al importarlo.
    pub nuevos: usize,
    /// Los que siguen en la cuenta sólo por este extracto (se borrarían al deshacerlo).
    pub propios: usize,
    /// De ésos, los conciliados.
    pub conciliados: usize,
    pub importado_en: String,
    /// Rehecho al actualizar la app con los movimientos importados antes (sin saldos del
    /// extracto: los del primer y el último movimiento).
    pub heredado: bool,
}

/// Un periodo entre dos extractos de una cuenta sin extracto que lo cubra.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Hueco {
    pub cuenta_id: i64,
    pub cuenta: String,
    /// Primer y último día sin extracto.
    pub desde: Date,
    pub hasta: Date,
    /// Faltan movimientos seguro (los saldos no enlazan); si no, es posible.
    pub seguro: bool,
    pub detalle: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ListaExtractos {
    /// Por cuenta y fecha.
    pub extractos: Vec<ExtractoGuardado>,
    pub huecos: Vec<Hueco>,
}

/// Guarda el extracto que se acaba de importar en la cuenta `cuenta` con los movimientos
/// `tocados` (nuevos y ya existentes), dentro de la transacción de la importación. Si ya estaba
/// el mismo (mismo fichero, cuenta, periodo y número de movimientos: se ha vuelto a importar),
/// se reutiliza. Devuelve su id.
pub(crate) fn registrar(
    conn: &Connection,
    cuenta: i64,
    fichero: &str,
    e: &Extracto,
    tocados: &[i64],
    nuevos: usize,
    now: OffsetDateTime,
) -> Result<i64> {
    let desde = e.desde.map(|d| d.to_string());
    let hasta = e.hasta.map(|d| d.to_string());
    let n = i64::try_from(e.movimientos.len()).unwrap_or(i64::MAX);
    let previo: Option<i64> = conn
        .query_row(
            "SELECT id FROM bank_statement
              WHERE bank_account = ?1 AND fichero = ?2 AND desde IS ?3 AND hasta IS ?4
                AND movimientos = ?5 AND heredado = 0
              ORDER BY id DESC LIMIT 1",
            params![cuenta, fichero, desde, hasta, n],
            |r| r.get(0),
        )
        .optional()?;
    let id = match previo {
        Some(id) => {
            conn.execute(
                "UPDATE bank_statement SET nuevos = nuevos + ?1 WHERE id = ?2",
                params![i64::try_from(nuevos).unwrap_or(0), id],
            )?;
            id
        }
        None => {
            conn.execute(
                "INSERT INTO bank_statement(bank_account, fichero, formato, desde, hasta,
                   saldo_inicial, saldo_final, movimientos, nuevos, importado_en)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    cuenta,
                    fichero,
                    e.formato,
                    desde,
                    hasta,
                    e.saldo_inicial.map(Money::cents),
                    e.saldo_final.map(Money::cents),
                    n,
                    i64::try_from(nuevos).unwrap_or(0),
                    now_str(now)
                ],
            )?;
            conn.last_insert_rowid()
        }
    };
    for t in tocados {
        conn.execute(
            "INSERT OR IGNORE INTO bank_statement_tx(statement_id, tx_id) VALUES (?1, ?2)",
            params![id, t],
        )?;
    }
    Ok(id)
}

/// Condición SQL: el movimiento `t` está conciliado o lo usa una remesa o un pago de un modelo.
const USADO: &str = "(t.matched_kind IS NOT NULL OR t.entry_id IS NOT NULL
     OR EXISTS(SELECT 1 FROM remittance r WHERE r.bank_tx_id = t.id)
     OR EXISTS(SELECT 1 FROM tax_payment p WHERE p.bank_tx = t.id))";

/// Condición SQL: el movimiento `t` del extracto `s` no lo trae ningún otro extracto.
const PROPIO: &str = "NOT EXISTS(SELECT 1 FROM bank_statement_tx o
     WHERE o.tx_id = t.id AND o.statement_id <> s.id)";

/// Los extractos importados (de una cuenta o de todas), del más antiguo al más nuevo por cuenta,
/// con los huecos entre ellos.
pub fn lista(v: &Vault, cuenta: Option<i64>) -> Result<ListaExtractos> {
    let conn = v.connection();
    let mut st = conn.prepare(&format!(
        "SELECT s.id, s.bank_account, b.name, b.currency, s.fichero, s.formato, s.desde, s.hasta,
                s.saldo_inicial, s.saldo_final, s.movimientos, s.nuevos, s.importado_en,
                s.heredado,
                (SELECT count(*) FROM bank_statement_tx x JOIN bank_tx t ON t.id = x.tx_id
                  WHERE x.statement_id = s.id AND {PROPIO}),
                (SELECT count(*) FROM bank_statement_tx x JOIN bank_tx t ON t.id = x.tx_id
                  WHERE x.statement_id = s.id AND {PROPIO} AND {USADO})
           FROM bank_statement s JOIN bank_account b ON b.id = s.bank_account
          WHERE ?1 IS NULL OR s.bank_account = ?1
          ORDER BY b.name, s.bank_account, s.desde, s.hasta, s.id"
    ))?;
    type Fila = (
        (i64, i64, String, String, String, String),
        (Option<String>, Option<String>, Option<i64>, Option<i64>),
        (i64, i64, String, bool, i64, i64),
    );
    let filas = st.query_map([cuenta], |r| -> taxpain_vault::rusqlite::Result<Fila> {
        Ok((
            (
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ),
            (r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?),
            (
                r.get(10)?,
                r.get(11)?,
                r.get(12)?,
                r.get(13)?,
                r.get(14)?,
                r.get(15)?,
            ),
        ))
    })?;
    let usize_de = |n: i64| usize::try_from(n).unwrap_or(0);
    let mut extractos = Vec::new();
    for f in filas {
        let (
            (id, cuenta_id, nombre, divisa, fichero, formato),
            (desde, hasta, si, sf),
            (movs, nuevos, en, heredado, propios, conciliados),
        ) = f?;
        extractos.push(ExtractoGuardado {
            id,
            cuenta_id,
            cuenta: nombre,
            divisa,
            fichero,
            formato,
            desde: desde.as_deref().map(parse_date).transpose()?,
            hasta: hasta.as_deref().map(parse_date).transpose()?,
            saldo_inicial: si.map(Money::from_cents),
            saldo_final: sf.map(Money::from_cents),
            movimientos: usize_de(movs),
            nuevos: usize_de(nuevos),
            propios: usize_de(propios),
            conciliados: usize_de(conciliados),
            importado_en: en,
            heredado,
        });
    }
    let huecos = huecos(&extractos);
    Ok(ListaExtractos { extractos, huecos })
}

/// Los huecos entre los extractos de cada cuenta (ordenados por cuenta y fecha).
pub fn huecos(extractos: &[ExtractoGuardado]) -> Vec<Hueco> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < extractos.len() {
        let cuenta = extractos[i].cuenta_id;
        let mut de_la_cuenta: Vec<&ExtractoGuardado> = extractos[i..]
            .iter()
            .take_while(|e| e.cuenta_id == cuenta)
            .collect();
        i += de_la_cuenta.len();
        de_la_cuenta.retain(|e| e.desde.is_some() && e.hasta.is_some());
        de_la_cuenta.sort_by_key(|e| (e.desde, e.hasta, e.id));
        // Hasta dónde llegan los extractos vistos y el saldo de ese día.
        let mut cubierto: Option<(Date, Option<Money>)> = None;
        for e in de_la_cuenta {
            let (Some(desde), Some(hasta)) = (e.desde, e.hasta) else {
                continue;
            };
            if let Some((fin, saldo)) = cubierto
                && desde > fin
            {
                let (h_desde, h_hasta) = (fin + Duration::days(1), desde - Duration::days(1));
                match (saldo, e.saldo_inicial) {
                    (Some(a), Some(b)) if a != b => out.push(Hueco {
                        cuenta_id: cuenta,
                        cuenta: e.cuenta.clone(),
                        desde: if h_desde > h_hasta { fin } else { h_desde },
                        hasta: if h_desde > h_hasta { desde } else { h_hasta },
                        seguro: true,
                        detalle: format!(
                            "El extracto anterior acaba el {} con un saldo de {a} y el siguiente \
                             empieza el {} con {b}: faltan movimientos por {} entre medias.",
                            dmy(fin),
                            dmy(desde),
                            b - a
                        ),
                    }),
                    (Some(_), Some(_)) => {}
                    _ if (desde - fin).whole_days() > DIAS_SIN_SALDOS => out.push(Hueco {
                        cuenta_id: cuenta,
                        cuenta: e.cuenta.clone(),
                        desde: h_desde,
                        hasta: h_hasta,
                        seguro: false,
                        detalle: format!(
                            "No hay extracto del {} al {} ({} días) y los saldos no permiten \
                             saber si faltan movimientos.",
                            dmy(h_desde),
                            dmy(h_hasta),
                            (h_hasta - h_desde).whole_days() + 1
                        ),
                    }),
                    _ => {}
                }
            }
            cubierto = match cubierto {
                Some((fin, saldo)) if fin >= hasta => Some((fin, saldo)),
                _ => Some((hasta, e.saldo_final)),
            };
        }
    }
    out
}

/// Lo que ha hecho deshacer un extracto.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Deshecho {
    /// Movimientos borrados (los que sólo traía este extracto).
    pub borrados: usize,
    /// De ésos, los que estaban conciliados y se han desconciliado antes.
    pub desconciliados: usize,
    /// Movimientos del extracto que se quedan porque los trae otro.
    pub compartidos: usize,
}

/// Deshace la importación del extracto `id`: borra sus movimientos propios (los que no trae otro
/// extracto) y lo quita de la lista. Si alguno está conciliado, hace falta `desconciliar`: se
/// deshace antes cada conciliación (con sus bloqueos) y queda en la auditoría como siempre. Pide
/// confirmación.
pub fn deshacer(
    v: &Vault,
    id: i64,
    desconciliar: bool,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<Deshecho> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let (fichero, cuenta): (String, String) = conn
        .query_row(
            "SELECT s.fichero, b.name FROM bank_statement s
               JOIN bank_account b ON b.id = s.bank_account WHERE s.id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let propios = |conn: &Connection, solo_usados: bool| -> Result<Vec<i64>> {
        let mut st = conn.prepare(&format!(
            "SELECT t.id FROM bank_statement s JOIN bank_statement_tx x ON x.statement_id = s.id
               JOIN bank_tx t ON t.id = x.tx_id
              WHERE s.id = ?1 AND {PROPIO} {}
              ORDER BY t.id",
            if solo_usados {
                format!("AND {USADO}")
            } else {
                String::new()
            }
        ))?;
        let ids = st
            .query_map([id], |r| r.get(0))?
            .collect::<std::result::Result<Vec<i64>, _>>()?;
        Ok(ids)
    };
    let usados = propios(conn, true)?;
    if !usados.is_empty() && !desconciliar {
        return Err(other(format!(
            "{} del extracto {fichero} {} conciliados: deshaz antes las conciliaciones o \
             confirma que se desconcilien",
            if usados.len() == 1 {
                "1 movimiento".to_owned()
            } else {
                format!("{} movimientos", usados.len())
            },
            if usados.len() == 1 { "está" } else { "están" }
        )));
    }
    // Antes de tocar nada, que se puedan deshacer todas.
    for t in &usados {
        let conciliado: bool = conn.query_row(
            "SELECT matched_kind IS NOT NULL FROM bank_tx WHERE id = ?1",
            [t],
            |r| r.get(0),
        )?;
        if !conciliado {
            return Err(other(format!(
                "el movimiento {t} del extracto está en una remesa o en el pago de un modelo sin \
                 estar conciliado: quítalo de ahí antes"
            )));
        }
        if let Some(why) = crate::conciliados::blocker(v, conn, *t)? {
            let (fecha, concepto): (String, String) = conn.query_row(
                "SELECT op_date, description FROM bank_tx WHERE id = ?1",
                [t],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            return Err(other(format!(
                "no se puede desconciliar «{concepto}» del {}: {why}",
                dmy(parse_date(&fecha)?)
            )));
        }
    }
    let mut desconciliados = 0;
    for t in &usados {
        // La otra cara de un traspaso ya puede haber vuelto a pendiente.
        let sigue: bool = conn.query_row(
            "SELECT matched_kind IS NOT NULL FROM bank_tx WHERE id = ?1",
            [t],
            |r| r.get(0),
        )?;
        if sigue {
            crate::conciliados::unreconcile(v, *t, now, true)?;
        }
        desconciliados += 1;
    }
    let tx = conn.unchecked_transaction()?;
    let borrar = propios(&tx, false)?;
    let todos: i64 = tx.query_row(
        "SELECT count(*) FROM bank_statement_tx WHERE statement_id = ?1",
        [id],
        |r| r.get(0),
    )?;
    let usados_ahora = propios(&tx, true)?;
    if !usados_ahora.is_empty() {
        return Err(other(
            "algún movimiento del extracto sigue conciliado: vuelve a intentarlo",
        ));
    }
    for t in &borrar {
        tx.execute("DELETE FROM bank_tx WHERE id = ?1", [t])?;
    }
    tx.execute("DELETE FROM bank_statement WHERE id = ?1", [id])?;
    let compartidos = usize::try_from(todos)
        .unwrap_or(0)
        .saturating_sub(borrar.len());
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "bank.statement_undo",
        &id.to_string(),
        &serde_json::json!({
            "fichero": fichero,
            "cuenta": cuenta,
            "borrados": borrar,
            "desconciliados": desconciliados,
            "compartidos": compartidos,
        }),
    )?;
    tx.commit()?;
    Ok(Deshecho {
        borrados: borrar.len(),
        desconciliados,
        compartidos,
    })
}
