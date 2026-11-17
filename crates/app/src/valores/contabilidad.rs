//! Contabilidad de la cartera de valores de una sociedad: grabar los asientos de las
//! operaciones del ejercicio como lote deshacible, la valoración al cierre (paso del cierre) y lo
//! que de ella pasa al modelo 200.
//!
//! - **Operaciones** (`Origin::Valores(id de security_op)`): los asientos de
//!   [`super::asientos::proponer`] (compras, ventas con su resultado, dividendos íntegros,
//!   retenciones en origen a 6300 y españolas a 473, intereses, comisiones e impuestos). Cada
//!   operación guarda su asiento (`security_op.entry_id`); una operación con asiento no se edita
//!   ni se borra hasta deshacer el lote. «Rehacer» borra los del ejercicio y los genera de
//!   nuevo (con los costes al día).
//! - **Cierre** (`Origin::Valores(−año)`, el último día del ejercicio), según la clasificación
//!   elegida en los ajustes (PGC PYMES, NRV 9.ª):
//!   - *Mantenidos para negociar* (NRV 9.ª 2.2): a valor razonable con cambios en la cuenta de
//!     pérdidas y ganancias. Ajuste por valor desde el valor en libros (el valor razonable del
//!     cierre anterior más lo comprado después) a la cotización del cierre: 540x / 7630 o
//!     6630 / 540x (subcuentas de la 763 y la 663 del PGC PYMES). Sin reversión: el ejercicio
//!     siguiente parte de ese valor.
//!   - *A coste* (NRV 9.ª 2.3 «Activos financieros a coste»): sólo deterioro, si el valor
//!     recuperable queda por debajo del valor en libros. Para títulos cotizados se toma la
//!     cotización del cierre como valor razonable (`TODO(VERIFY)`: la NRV 9.ª 2.3.3 del PGC
//!     PYMES toma, «salvo mejor evidencia», el patrimonio neto de la participada; un descenso
//!     puntual de la cotización puede no ser deterioro: por eso cada valor se puede excluir). El
//!     asiento revierte el deterioro dotado aquí en cierres anteriores (596 o 296 / 798 o 7963)
//!     y dota el nuevo (698 o 6963 / 596 o 296). Los bonos (coste amortizado: el deterioro es
//!     por riesgo de crédito) y los derivados no se tocan: se avisa.
//! - **Modelo 200** ([`fiscal`]): el deterioro de participaciones no es deducible (art. 13.2.b
//!   LIS: corrección en aumento [00325]) y su reversión, si se sumó en su día, se resta
//!   ([00326]). El valor razonable de la cartera de negociación que va a resultados sí tributa
//!   (art. 17.1 LIS), salvo las disminuciones de participaciones que cumplan los requisitos del
//!   art. 21.1 (al menos un 5 %), que no son deducibles (art. 15.l LIS, leído en el BOE
//!   consolidado el 2026-11-16): se proponen sin incluir, para que el usuario confirme si
//!   alguna lo es.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::{AccountCode, EntryLine, JournalEntry, Origin};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, OffsetDateTime};

use super::asientos::subcuenta;
use super::{Cartera, Clase, Plazo, Settings};
use crate::accounting;
use crate::cierre::ProposedLine;
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

fn code(s: &str) -> Result<AccountCode> {
    AccountCode::new(s).map_err(|_| other(format!("cuenta no válida: «{s}»")))
}

fn exige_sociedad(v: &Vault) -> Result<()> {
    if crate::cripto::es_persona_fisica(v) {
        return Err(other(
            "una persona física no contabiliza su cartera de valores: las ganancias y los \
             dividendos van a la Renta",
        ));
    }
    Ok(())
}

/// Marca de las líneas de derivados en el ajuste del cierre (no son participaciones: el art.
/// 15.l LIS no les afecta).
const DERIVADO: &str = " (derivado)";

// ---------------- Operaciones ----------------

/// ¿Hay algún asiento de la cartera grabado (de operaciones o de un cierre)?
pub(crate) fn hay_asientos(v: &Vault) -> Result<bool> {
    let n: i64 = v.connection().query_row(
        "SELECT COUNT(*) FROM journal_entry WHERE origin_kind = 'Valores'",
        [],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Error si la operación tiene asiento grabado.
pub(crate) fn sin_asiento(conn: &Connection, op: i64) -> Result<()> {
    let e: Option<i64> = conn
        .query_row(
            "SELECT entry_id FROM security_op WHERE id = ?1",
            [op],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    if e.is_some() {
        return Err(other(
            "la operación ya está contabilizada: deshaz los asientos del ejercicio (en \
             «Asientos») antes de cambiarla o borrarla",
        ));
    }
    Ok(())
}

fn borrar_asiento(conn: &Connection, entry_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM line_tick WHERE line_id IN (SELECT id FROM journal_line WHERE entry_id = ?1)",
        [entry_id],
    )?;
    conn.execute("DELETE FROM entry_document WHERE entry_id = ?1", [entry_id])?;
    conn.execute(
        "UPDATE security_op SET entry_id = NULL WHERE entry_id = ?1",
        [entry_id],
    )?;
    // Los disparadores impiden borrar en un ejercicio cerrado.
    conn.execute("DELETE FROM journal_line WHERE entry_id = ?1", [entry_id])
        .map_err(|_| other("el ejercicio del asiento está cerrado: reábrelo"))?;
    conn.execute("DELETE FROM journal_entry WHERE id = ?1", [entry_id])?;
    Ok(())
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Contabilizado {
    pub asientos: usize,
    pub borrados: usize,
    /// Operaciones del ejercicio que deberían tener asiento y no lo tienen (sin euros, venta
    /// sin coste…).
    pub saltadas: usize,
    pub avisos: Vec<String>,
}

/// Da de alta en el plan las subcuentas de los valores con su nombre (5400001 «ETFX …»).
fn alta_subcuentas(conn: &Connection, s: &Settings, vs: &[super::Valor], now: &str) -> Result<()> {
    for x in vs {
        let c = subcuenta(s, Some(x), x.id);
        let nombre = if x.name.is_empty() {
            x.ticker.clone()
        } else {
            format!("{} {}", x.ticker, x.name)
        };
        conn.execute(
            "INSERT OR IGNORE INTO account(code, name, created_at) VALUES (?1, ?2, ?3)",
            params![c, nombre, now],
        )?;
    }
    Ok(())
}

fn lineas_de(a: &super::asientos::Propuesto) -> Result<Vec<EntryLine>> {
    a.lineas
        .iter()
        .map(|l| {
            Ok(EntryLine {
                account: code(&l.cuenta)?,
                debit: l.debe,
                credit: l.haber,
                description: None,
            })
        })
        .collect()
}

/// Graba los asientos de las operaciones de `year` que no lo tienen (`rehacer` borra antes los
/// del ejercicio y los genera todos de nuevo). Con confirmación.
pub fn contabilizar(
    v: &Vault,
    year: i32,
    rehacer: bool,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<Contabilizado> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    exige_sociedad(v)?;
    let (inicio, fin) = super::informe::ejercicio(v, year);
    let conn = v.connection();
    if crate::cripto::ejercicio_cerrado(conn, fin)? {
        return Err(other(format!(
            "el ejercicio {year} está cerrado: reábrelo para contabilizar"
        )));
    }
    let s = super::settings(v)?;
    let vs = super::valores(v)?;
    let ts = now_str(now);
    let tx = conn.unchecked_transaction()?;
    let mut out = Contabilizado::default();
    if rehacer {
        let ids: Vec<i64> = {
            let mut st = tx.prepare(
                "SELECT DISTINCT entry_id FROM security_op
                  WHERE entry_id IS NOT NULL AND date >= ?1 AND date <= ?2",
            )?;
            st.query_map(params![inicio.to_string(), fin.to_string()], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?
        };
        for id in ids {
            borrar_asiento(&tx, id)?;
            out.borrados += 1;
        }
    }
    // El informe se calcula dentro de la transacción: ya ve los enlaces borrados.
    let r = super::report(v, year)?;
    alta_subcuentas(&tx, &s, &vs, &ts)?;
    for a in r.asientos.iter().filter(|a| a.entry_id.is_none()) {
        let Some(op) = a.op else { continue };
        let id = accounting::record_entry(
            &tx,
            &JournalEntry {
                date: a.fecha,
                concept: a.concepto.clone(),
                origin: Origin::Valores(op),
                lines: lineas_de(a)?,
            },
            now,
        )?;
        tx.execute(
            "UPDATE security_op SET entry_id = ?1 WHERE id = ?2",
            params![id, op],
        )?;
        out.asientos += 1;
    }
    // Lo que debería tener asiento y no lo tiene.
    let con_asiento: Vec<i64> = r.asientos.iter().filter_map(|a| a.op).collect();
    for o in super::ops(v, Some(year))?.iter().filter(|o| {
        o.date >= inicio
            && o.date <= fin
            && !con_asiento.contains(&o.id)
            && !matches!(
                o.kind,
                super::TipoOp::Deposito
                    | super::TipoOp::Retirada
                    | super::TipoOp::Traspaso
                    | super::TipoOp::Split
            )
    }) {
        out.saltadas += 1;
        out.avisos.push(format!(
            "{} del {} ({}): sin asiento (le faltan los euros o el coste).",
            o.kind.label(),
            crate::cripto::inicio::dmy(o.date),
            super::informe::nombre_valor(&vs, o.security_id.unwrap_or_default())
        ));
    }
    if r.sin_euros > 0 {
        out.avisos.insert(
            0,
            format!(
                "{} operaciones en otra divisa sin euros: pásalas a euros con el BCE y vuelve a \
                 contabilizar.",
                r.sin_euros
            ),
        );
    }
    audit::append(
        &tx,
        &ts,
        Actor::User,
        "valores.contabilizar",
        &year.to_string(),
        &serde_json::json!({
            "asientos": out.asientos,
            "borrados": out.borrados,
            "saltadas": out.saltadas,
            "cartera": s.cartera,
        }),
    )?;
    tx.commit()?;
    Ok(out)
}

/// Quita los asientos de las operaciones de `year` (no el del cierre). Con confirmación.
pub fn deshacer(v: &Vault, year: i32, now: OffsetDateTime, confirmed: bool) -> Result<usize> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let (inicio, fin) = super::informe::ejercicio(v, year);
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let ids: Vec<i64> = {
        let mut st = tx.prepare(
            "SELECT DISTINCT entry_id FROM security_op
              WHERE entry_id IS NOT NULL AND date >= ?1 AND date <= ?2",
        )?;
        st.query_map(params![inicio.to_string(), fin.to_string()], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?
    };
    if ids.is_empty() {
        return Err(AppError::NotFound);
    }
    for id in &ids {
        borrar_asiento(&tx, *id)?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "valores.deshacer",
        &year.to_string(),
        &serde_json::json!({ "asientos": ids }),
    )?;
    tx.commit()?;
    Ok(ids.len())
}

// ---------------- Valoración al cierre ----------------

/// Lo que el usuario decide en el paso del cierre: los valores que no se deterioran (cartera a
/// coste) aunque coticen por debajo del coste.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntradaCierre {
    #[serde(default)]
    pub excluidos: Vec<i64>,
}

/// Cómo se trata un valor al cierre.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tratamiento {
    /// Ajuste a valor razonable (negociación).
    ValorRazonable,
    /// Deterioro (a coste, cotiza por debajo del coste).
    Deterioro,
    /// A coste y sin deterioro (cotiza por encima del coste).
    SinDeterioro,
    /// A coste, por debajo del coste, pero el usuario dice que no es deterioro.
    Excluido,
    /// Bonos y derivados en una cartera a coste: no se valoran aquí.
    NoAplica,
    /// Falta el precio o el cambio.
    SinPrecio,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ValorCierre {
    pub security_id: i64,
    pub ticker: String,
    pub name: String,
    pub kind: Clase,
    /// Subcuenta del valor.
    pub cuenta: String,
    /// Valor en libros antes del ajuste (coste o valor razonable anterior).
    pub libros: Money,
    /// Valor razonable al cierre (cotización por cambio).
    pub valor: Option<Money>,
    /// Ajuste: positivo sube el valor (7630), negativo lo baja (6630 o deterioro).
    pub ajuste: Money,
    pub tratamiento: Tratamiento,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CierreValores {
    pub year: i32,
    pub fecha: Date,
    pub cartera: Cartera,
    pub closed: bool,
    pub valores: Vec<ValorCierre>,
    pub lines: Vec<ProposedLine>,
    /// Negociación: subidas y bajadas de valor razonable.
    pub beneficios: Money,
    pub perdidas: Money,
    /// A coste: deterioro nuevo y el de cierres anteriores que se revierte.
    pub deterioro: Money,
    pub reversion: Money,
    pub entry_id: Option<i64>,
    /// Lo grabado ya no coincide con lo que sale ahora.
    pub stale: bool,
    pub input: EntradaCierre,
    pub avisos: Vec<String>,
    pub notas: Vec<String>,
}

fn setting_key(year: i32) -> String {
    format!("valores_cierre:{year}")
}

fn entry_cierre(conn: &Connection, year: i32) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT id FROM journal_entry WHERE origin_kind = 'Valores' AND origin_id = ?1",
            [-i64::from(year)],
            |r| r.get(0),
        )
        .optional()?)
}

/// Saldo acreedor de `cuenta` en los cierres de valores anteriores a `antes`.
fn deterioro_previo(conn: &Connection, cuenta: &str, antes: Date) -> Result<Money> {
    let c: i64 = conn.query_row(
        "SELECT COALESCE(SUM(l.credit - l.debit), 0) FROM journal_line l
           JOIN journal_entry e ON e.id = l.entry_id
          WHERE e.origin_kind = 'Valores' AND e.origin_id < 0 AND e.date < ?1 AND l.account = ?2",
        params![antes.to_string(), cuenta],
        |r| r.get(0),
    )?;
    Ok(Money::from_cents(c))
}

fn linea(conn: &Connection, account: &str, debit: Money, credit: Money) -> Result<ProposedLine> {
    Ok(ProposedLine {
        name: accounting::account_name(conn, account)?.unwrap_or_default(),
        account: account.into(),
        debit,
        credit,
    })
}

/// Líneas con descripción (la del ajuste de cada valor), para grabarlas.
struct Borrador {
    lineas: Vec<(String, Money, Money, Option<String>)>,
}

impl Borrador {
    fn push(&mut self, cuenta: &str, importe: Money, debe: bool, desc: Option<String>) {
        if importe.is_zero() {
            return;
        }
        let (d, h) = if debe != importe.is_negative() {
            (importe.abs(), Money::ZERO)
        } else {
            (Money::ZERO, importe.abs())
        };
        self.lineas.push((cuenta.to_owned(), d, h, desc));
    }
}

fn calcular(
    v: &Vault,
    year: i32,
    input: Option<EntradaCierre>,
) -> Result<(CierreValores, Borrador)> {
    exige_sociedad(v)?;
    let conn = v.connection();
    let s = super::settings(v)?;
    let c = &s.cuentas;
    let r = super::report(v, year)?;
    let (inicio, fecha) = (r.inicio, r.fecha);
    let entry_id = entry_cierre(conn, year)?;
    let guardado: Option<EntradaCierre> = v.setting(&setting_key(year))?;
    let input = match (entry_id, input) {
        (Some(_), _) => guardado.unwrap_or_default(),
        (None, Some(i)) => i,
        (None, None) => guardado.unwrap_or_default(),
    };
    let vs = super::valores(v)?;
    // Posiciones juntando cuentas de bróker (la subcuenta es por valor).
    let mut por: BTreeMap<i64, (Money, Option<Money>)> = BTreeMap::new();
    for p in &r.posiciones {
        let e = por
            .entry(p.security_id)
            .or_insert((Money::ZERO, Some(Money::ZERO)));
        e.0 += p.coste;
        e.1 = e.1.zip(p.valor).map(|(a, b)| a + b);
    }
    let mut valores = Vec::new();
    let mut avisos = Vec::new();
    let mut notas = Vec::new();
    let mut b = Borrador { lineas: Vec::new() };
    let (mut beneficios, mut perdidas, mut deterioro) = (Money::ZERO, Money::ZERO, Money::ZERO);
    for (id, (libros, valor)) in por {
        let x = vs.iter().find(|x| x.id == id);
        let kind = x.map(|x| x.kind).unwrap_or_default();
        let cuenta = subcuenta(&s, x, id);
        let ticker = x.map(|x| x.ticker.clone()).unwrap_or_default();
        let (tratamiento, ajuste) = match (s.cartera, valor) {
            (_, None) => (Tratamiento::SinPrecio, Money::ZERO),
            (Cartera::Negociacion, Some(val)) => (Tratamiento::ValorRazonable, val - libros),
            (Cartera::Coste, Some(_)) if kind == Clase::Derivado || kind.deuda() => {
                (Tratamiento::NoAplica, Money::ZERO)
            }
            (Cartera::Coste, Some(val)) if val >= libros => {
                (Tratamiento::SinDeterioro, Money::ZERO)
            }
            (Cartera::Coste, Some(_)) if input.excluidos.contains(&id) => {
                (Tratamiento::Excluido, Money::ZERO)
            }
            (Cartera::Coste, Some(val)) => (Tratamiento::Deterioro, val - libros),
        };
        match tratamiento {
            Tratamiento::ValorRazonable if !ajuste.is_zero() => {
                let desc = Some(if kind == Clase::Derivado {
                    format!("{ticker}{DERIVADO}")
                } else {
                    ticker.clone()
                });
                if ajuste.is_negative() {
                    perdidas += -ajuste;
                    b.push(&c.perdida_vr, -ajuste, true, desc.clone());
                    b.push(&cuenta, -ajuste, false, desc);
                } else {
                    beneficios += ajuste;
                    b.push(&cuenta, ajuste, true, desc.clone());
                    b.push(&c.beneficio_vr, ajuste, false, desc);
                }
            }
            Tratamiento::Deterioro => deterioro += -ajuste,
            Tratamiento::SinPrecio => avisos.push(format!(
                "{ticker}: falta el precio de cierre o el tipo de cambio a {fecha}: ponlo en \
                 Inversiones → «Precios de cierre»."
            )),
            Tratamiento::NoAplica if kind == Clase::Derivado => avisos.push(format!(
                "{ticker}: los derivados son siempre de negociación (NRV 9.ª): con la cartera a \
                 coste, valóralo a mano a valor razonable (7630/6630)."
            )),
            Tratamiento::NoAplica => notas.push(format!(
                "{ticker}: los bonos van a coste amortizado; su deterioro es por riesgo de \
                 crédito del emisor, no por la cotización (NRV 9.ª). Si lo hay, contabilízalo a \
                 mano (6988/597 o 6968/297)."
            )),
            _ => {}
        }
        valores.push(ValorCierre {
            security_id: id,
            ticker,
            name: x.map(|x| x.name.clone()).unwrap_or_default(),
            kind,
            cuenta,
            libros,
            valor,
            ajuste,
            tratamiento,
        });
    }
    // Cartera a coste: revierte lo dotado en cierres anteriores y dota lo de ahora.
    let mut reversion = Money::ZERO;
    if s.cartera == Cartera::Coste {
        for (correctora, rev) in [
            (&c.correctora_corto, &c.reversion_corto),
            (&c.correctora_largo, &c.reversion_largo),
        ] {
            let previo = deterioro_previo(conn, correctora, inicio)?;
            if previo.cents() > 0 {
                reversion += previo;
                b.push(correctora, previo, true, None);
                b.push(rev, previo, false, None);
            }
        }
        let (gasto, correctora) = match s.plazo {
            Plazo::Corto => (&c.deterioro_corto, &c.correctora_corto),
            Plazo::Largo => (&c.deterioro_largo, &c.correctora_largo),
        };
        b.push(gasto, deterioro, true, None);
        b.push(correctora, deterioro, false, None);
        notas.push(
            "Cartera a coste (NRV 9.ª 2.3 del PGC PYMES): sólo se registra el deterioro si el \
             valor recuperable baja del coste. Aquí se toma la cotización del cierre; si crees \
             que la bajada es pasajera y el valor se recupera, excluye ese valor (TODO(VERIFY): \
             la norma, salvo mejor evidencia, mira el patrimonio neto de la participada). El \
             deterioro no es deducible (art. 13.2.b LIS): pasa al 200 como corrección en \
             aumento y su reversión, como disminución."
                .into(),
        );
    } else {
        notas.push(
            "Mantenidos para negociar (NRV 9.ª 2.2 del PGC PYMES): cada valor se lleva a su \
             cotización del cierre y la diferencia va a resultados (7630/6630, subcuentas de la \
             763 y la 663). Tributa con el resultado (art. 17.1 LIS), salvo las bajadas de \
             participaciones de al menos el 5 %, que no son deducibles (art. 15.l LIS): el 200 \
             lo propone sin incluir."
                .into(),
        );
    }
    let lines = b
        .lineas
        .iter()
        .map(|(a, d, h, _)| linea(conn, a, *d, *h))
        .collect::<Result<Vec<_>>>()?;
    let stale = match entry_id {
        Some(e) => comparable(&lines) != grabado(conn, e)?,
        None => false,
    };
    if r.posiciones.is_empty() && entry_id.is_none() && reversion.is_zero() {
        notas.clear();
    }
    let closed = crate::cripto::ejercicio_cerrado(conn, fecha)?;
    Ok((
        CierreValores {
            year,
            fecha,
            cartera: s.cartera,
            closed,
            valores,
            lines,
            beneficios,
            perdidas,
            deterioro,
            reversion,
            entry_id,
            stale,
            input,
            avisos,
            notas,
        },
        b,
    ))
}

fn comparable(lines: &[ProposedLine]) -> Vec<(String, i64, i64)> {
    let mut out: Vec<_> = lines
        .iter()
        .map(|l| (l.account.clone(), l.debit.cents(), l.credit.cents()))
        .collect();
    out.sort();
    out
}

fn grabado(conn: &Connection, entry: i64) -> Result<Vec<(String, i64, i64)>> {
    let mut st =
        conn.prepare("SELECT account, debit, credit FROM journal_line WHERE entry_id = ?1")?;
    let mut out = st
        .query_map([entry], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<Vec<(String, i64, i64)>, _>>()?;
    out.sort();
    Ok(out)
}

/// Valoración al cierre de `year` (lo grabado o la propuesta con `input`).
pub fn cierre(v: &Vault, year: i32, input: Option<EntradaCierre>) -> Result<CierreValores> {
    Ok(calcular(v, year, input)?.0)
}

/// Graba la valoración al cierre (con confirmación). Devuelve el asiento.
pub fn contabilizar_cierre(
    v: &Vault,
    year: i32,
    input: EntradaCierre,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<i64> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let (p, b) = calcular(v, year, Some(input.clone()))?;
    if p.closed {
        return Err(other(
            "el ejercicio está cerrado: reábrelo para valorar la cartera",
        ));
    }
    if p.entry_id.is_some() {
        return Err(other(
            "la valoración de ese cierre ya está grabada: deshazla para rehacerla",
        ));
    }
    if p.valores
        .iter()
        .any(|x| x.tratamiento == Tratamiento::SinPrecio)
    {
        return Err(other(format!(
            "faltan precios de cierre a {}: ponlos antes en Inversiones",
            p.fecha
        )));
    }
    if b.lineas.len() < 2 {
        return Err(other(
            "no hay nada que contabilizar: la cartera ya está a su valor del cierre",
        ));
    }
    let lines = b
        .lineas
        .iter()
        .map(|(a, d, h, desc)| {
            Ok(EntryLine {
                account: code(a)?,
                debit: *d,
                credit: *h,
                description: desc.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let concepto = match p.cartera {
        Cartera::Negociacion => {
            format!("Cartera de negociación a valor razonable al cierre de {year}")
        }
        Cartera::Coste => format!("Deterioro de la cartera de valores al cierre de {year}"),
    };
    let id = accounting::record_entry(
        &tx,
        &JournalEntry {
            date: p.fecha,
            concept: concepto,
            origin: Origin::Valores(-i64::from(year)),
            lines,
        },
        now,
    )?;
    tx.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![
            setting_key(year),
            serde_json::to_string(&input).map_err(|e| other(e.to_string()))?
        ],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "valores.cierre",
        &year.to_string(),
        &serde_json::json!({
            "entry": id,
            "cartera": p.cartera,
            "beneficios": p.beneficios.cents(),
            "perdidas": p.perdidas.cents(),
            "deterioro": p.deterioro.cents(),
            "reversion": p.reversion.cents(),
            "excluidos": input.excluidos,
        }),
    )?;
    tx.commit()?;
    Ok(id)
}

/// Quita la valoración al cierre de `year` (con confirmación y el ejercicio abierto).
pub fn deshacer_cierre(v: &Vault, year: i32, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let Some(id) = entry_cierre(conn, year)? else {
        return Err(AppError::NotFound);
    };
    let tx = conn.unchecked_transaction()?;
    borrar_asiento(&tx, id)?;
    tx.execute("DELETE FROM settings WHERE key = ?1", [setting_key(year)])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "valores.cierre_deshacer",
        &year.to_string(),
        &serde_json::json!({ "entry": id }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Punto de la lista del cierre: `None` si no hay cartera; si no, (hecho, detalle).
pub(crate) fn estado_cierre(v: &Vault, year: i32) -> Result<Option<(bool, String)>> {
    if crate::cripto::es_persona_fisica(v) {
        return Ok(None);
    }
    let p = cierre(v, year, None)?;
    if p.valores.is_empty() && p.entry_id.is_none() && p.reversion.is_zero() {
        return Ok(None);
    }
    let sin_precio = p
        .valores
        .iter()
        .filter(|x| x.tratamiento == Tratamiento::SinPrecio)
        .count();
    Ok(Some(if p.entry_id.is_some() && !p.stale {
        (true, "Valoración de la cartera contabilizada.".into())
    } else if p.stale {
        (
            false,
            "La cartera ha cambiado desde que se valoró: deshaz la valoración y rehazla (abajo)."
                .into(),
        )
    } else if sin_precio > 0 {
        (
            false,
            format!("Faltan precios de cierre de {sin_precio} valores (Inversiones)."),
        )
    } else if p.lines.len() < 2 {
        (true, "La cartera ya está a su valor del cierre.".into())
    } else {
        (
            false,
            "Contabiliza la valoración de la cartera al cierre (abajo).".into(),
        )
    }))
}

// ---------------- Modelo 200 ----------------

/// Lo que la valoración de la cartera del ejercicio lleva al 200.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fiscal {
    /// Deterioro de participaciones dotado en el cierre (no deducible, art. 13.2.b LIS).
    pub deterioro: Money,
    /// Reversión de deterioros de cierres anteriores.
    pub reversion: Money,
    /// Bajadas de valor razonable de acciones y ETF (no de derivados) a resultados.
    pub perdidas_vr: Money,
    /// Saldo deudor que el cierre de valores deja en cada cuenta (para no proponerlo dos veces
    /// desde las cuentas 696, 698 y 796).
    pub saldos: BTreeMap<String, Money>,
}

/// Lo grabado en el cierre de valores de `year` (vacío si no hay o es persona física).
pub fn fiscal(v: &Vault, year: i32) -> Result<Fiscal> {
    let mut f = Fiscal::default();
    if crate::cripto::es_persona_fisica(v) {
        return Ok(f);
    }
    let conn = v.connection();
    let Some(e) = entry_cierre(conn, year)? else {
        return Ok(f);
    };
    let c = super::settings(v)?.cuentas;
    let mut st = conn.prepare(
        "SELECT account, debit, credit, COALESCE(description, '') FROM journal_line
          WHERE entry_id = ?1",
    )?;
    let rows = st
        .query_map([e], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (cuenta, d, h, desc) in rows {
        let (d, h) = (Money::from_cents(d), Money::from_cents(h));
        *f.saldos.entry(cuenta.clone()).or_default() += d - h;
        if cuenta == c.deterioro_corto || cuenta == c.deterioro_largo {
            f.deterioro += d - h;
        } else if cuenta == c.reversion_corto || cuenta == c.reversion_largo {
            f.reversion += h - d;
        } else if cuenta == c.perdida_vr && !desc.ends_with(DERIVADO) {
            f.perdidas_vr += d - h;
        }
    }
    Ok(f)
}
