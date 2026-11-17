//! Asientos automáticos de las operaciones con criptomonedas y deterioro al cierre.
//!
//! Con naturaleza **existencias** (grupo 3, NRV 10.ª PGC):
//! - Compra: 309x (euros + comisión) / 572. La comisión es mayor precio de adquisición.
//! - Venta: 572 (euros − comisión) y 626 (comisión) / 700 (euros); 610 (coste) / 309x (coste).
//!   El coste sale del método elegido (PMP o FIFO). El resultado queda en 700 − 610 − 626.
//! - Permuta: 309y (valor de mercado) / 700; 610 (coste) / 309x (coste); comisión 626 / 572.
//! - Pago de un gasto en cripto: como la venta, con la 555 (puente) en vez del banco: después
//!   se salda la 555 contra el proveedor. Cobro en cripto: 309x / 555.
//! - Comisión pagada en cripto: 626 (valor) / 700 (valor); 610 (coste) / 309x (coste). Si es la
//!   comisión de una compra pagada en otra cripto (BNB), el cargo va a la subcuenta de lo
//!   comprado en vez de a la 626 (mayor precio de adquisición).
//! - Recompensa: 309x (valor de mercado) / 769.
//! - Pérdida o robo: 610 (coste) / 309x (coste).
//! - Traspaso: nada (la comisión en euros, 626 / 572).
//! - Cierre: deterioro si el precio de cierre deja el valor por debajo del coste (NRV 10.ª.2):
//!   693 / 390 por la diferencia, revirtiendo antes el del año anterior (390 / 793). El
//!   deterioro de existencias es deducible (no está en el art. 13.2 LIS).
//!
//! Con naturaleza **inmovilizado intangible** (NRV 5.ª y 6.ª): las mismas cuentas de activo en
//! la 207x (la 209 del PGC son anticipos) y el resultado de cada transmisión neto en la 771
//! (beneficio) o la 671 (pérdida); deterioro 690 / 290 (no deducible: art. 13.2.a LIS; ajuste
//! positivo en el 200). La app **no amortiza** el intangible: sólo comprueba el deterioro en
//! cada cierre. Comprobado el 2026-10-27: el art. 39.4 del Código de Comercio dice que «los
//! inmovilizados intangibles son activos de vida útil definida» y que «cuando la vida útil de
//! estos activos no pueda estimarse de manera fiable se amortizarán en un plazo de diez años»
//! (NRV 5.ª PGC y PGC PYMES), y el art. 12.2 LIS admite como deducible «el límite anual máximo
//! de la veinteava parte» (5 %) cuando no se puede estimar; la consulta 4 del BOICAC 120 no
//! fija plazo. Pendiente de decisión: amortizar a diez años
//! (contable) con ajuste al 5 % (fiscal) o mantener sólo el deterioro.
//!
//! Las subcuentas de activo son una por criptomoneda (3090001 BTC, 3090002 ETH…), lo que deja
//! cada moneda «registrada en la contabilidad de forma individualizada» (art. 42 quater.5.b
//! RGAT). Los asientos nunca se tocan desde el diario: se rehacen desde esta pantalla.
//!
//! Con punto de partida ([`super::inicio`]) sólo se contabilizan las operaciones posteriores a
//! la fecha de corte (lo anterior lo contabilizó la gestoría) y las monedas del punto de partida
//! van contra la cuenta donde la gestoría tiene su coste, si se dijo cuál.

use std::collections::BTreeMap;

use serde::Serialize;
use taxpain_core::accounting::{AccountCode, EntryLine, JournalEntry, Origin};
use taxpain_core::cripto::{self, Metodo, Tipo};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::OffsetDateTime;

use super::{Naturaleza, Op, Platform, Settings, cuentas_deterioro, ejercicio_cerrado};
use crate::accounting;
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

fn code(s: &str) -> Result<AccountCode> {
    AccountCode::new(s).map_err(|_| other(format!("cuenta no válida: «{s}»")))
}

/// Borra un asiento de criptomonedas (líneas, punteos y enlace con la operación).
pub(crate) fn borrar_asiento(conn: &Connection, entry_id: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM line_tick WHERE line_id IN (SELECT id FROM journal_line WHERE entry_id = ?1)",
        [entry_id],
    )?;
    conn.execute("DELETE FROM entry_document WHERE entry_id = ?1", [entry_id])?;
    conn.execute(
        "UPDATE crypto_op SET entry_id = NULL WHERE entry_id = ?1",
        [entry_id],
    )?;
    conn.execute("DELETE FROM journal_line WHERE entry_id = ?1", [entry_id])?;
    conn.execute("DELETE FROM journal_entry WHERE id = ?1", [entry_id])?;
    Ok(())
}

/// Asiento de deterioro del ejercicio, si existe.
pub(crate) fn asiento_deterioro(conn: &Connection, year: i32) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT id FROM journal_entry WHERE origin_kind = 'Cripto' AND origin_id = ?1",
            [-i64::from(year)],
            |r| r.get(0),
        )
        .optional()?)
}

/// Subcuenta de activo de una criptomoneda: la guardada o la siguiente libre de la base de la
/// naturaleza actual (3090001, 3090002…), que se da de alta en el plan con su nombre.
fn subcuenta(
    conn: &Connection,
    s: &Settings,
    inicio: Option<&super::inicio::Inicio>,
    ticker: &str,
    now: &str,
) -> Result<AccountCode> {
    if let Some(i) = inicio.filter(|i| !i.cuenta.is_empty() && i.tiene(ticker)) {
        return code(&i.cuenta);
    }
    let base = match s.naturaleza {
        Naturaleza::Existencias => &s.cuentas.existencias,
        Naturaleza::Intangible => &s.cuentas.intangible,
    };
    let base_code = code(base)?;
    let stored: Option<String> = conn
        .query_row(
            "SELECT account FROM crypto_asset WHERE ticker = ?1",
            [ticker],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(a) = stored.filter(|a| a.starts_with(base_code.as_str()) && a.len() > base.len()) {
        return code(&a);
    }
    let width = base.len() + 4;
    let mut n = 1u32;
    loop {
        let cand = base_code
            .subaccount(width, n)
            .map_err(|e| other(e.to_string()))?;
        let taken: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM crypto_asset WHERE account = ?1
                 UNION ALL SELECT 1 FROM account WHERE code = ?1
                 UNION ALL SELECT 1 FROM journal_line WHERE account = ?1 LIMIT 1",
                [cand.as_str()],
                |r| r.get(0),
            )
            .optional()?;
        if taken.is_none() {
            conn.execute(
                "INSERT OR IGNORE INTO account(code, name, created_at) VALUES (?1, ?2, ?3)",
                params![cand.as_str(), format!("Criptomonedas {ticker}"), now],
            )?;
            conn.execute(
                "UPDATE crypto_asset SET account = ?1 WHERE ticker = ?2",
                params![cand.as_str(), ticker],
            )?;
            return Ok(cand);
        }
        n += 1;
        if n > 9999 {
            return Err(other("no quedan subcuentas libres para criptomonedas"));
        }
    }
}

fn tesoreria(s: &Settings, plataformas: &[Platform], id: i64) -> Result<AccountCode> {
    let p = plataformas.iter().find(|p| p.id == id);
    match p.map(|p| p.account.as_str()).filter(|a| !a.is_empty()) {
        Some(a) => code(a),
        None => code(&s.cuentas.tesoreria),
    }
}

/// Líneas de la transmisión de `coste` por `valor` (sin la comisión) según la naturaleza:
/// existencias → ingreso bruto y salida del coste; intangible → resultado neto.
fn transmision(
    s: &Settings,
    activo: &AccountCode,
    valor: Money,
    coste: Money,
    contrapartida_valor: Vec<EntryLine>,
) -> Result<Vec<EntryLine>> {
    let mut lines = contrapartida_valor;
    match s.naturaleza {
        Naturaleza::Existencias => {
            lines.extend(EntryLine::signed(code(&s.cuentas.ventas)?, valor, false));
            lines.extend(EntryLine::signed(code(&s.cuentas.variacion)?, coste, true));
            lines.extend(EntryLine::signed(activo.clone(), coste, false));
        }
        Naturaleza::Intangible => {
            lines.extend(EntryLine::signed(activo.clone(), coste, false));
            let dif = valor - coste;
            if dif.is_negative() {
                lines.extend(EntryLine::signed(
                    code(&s.cuentas.perdida)?,
                    dif.abs(),
                    true,
                ));
            } else {
                lines.extend(EntryLine::signed(code(&s.cuentas.beneficio)?, dif, false));
            }
        }
    }
    Ok(lines)
}

/// Asiento de una operación con el coste de lo transmitido (`coste`, cero si no transmite).
/// `None` si la operación no genera asiento (traspaso sin comisión, importes a cero).
#[allow(clippy::too_many_arguments)]
pub(crate) fn asiento_de(
    s: &Settings,
    op: &Op,
    coste: Money,
    activo: &AccountCode,
    activo2: Option<&AccountCode>,
    tesoreria: &AccountCode,
    plataforma: &str,
    capitaliza: Option<&AccountCode>,
) -> Result<Option<JournalEntry>> {
    let t = super::ticker_norm(&op.ticker);
    let concepto = |que: &str| format!("{que} {} {t} ({plataforma})", op.qty);
    let comisiones = code(&s.cuentas.comisiones)?;
    let puente = code(&s.cuentas.puente)?;
    let d = |a: &AccountCode, m: Money| EntryLine::signed(a.clone(), m, true);
    let h = |a: &AccountCode, m: Money| EntryLine::signed(a.clone(), m, false);
    let (concept, lines): (String, Vec<EntryLine>) = match op.kind {
        Tipo::Compra | Tipo::Cobro => {
            let total = op.eur + op.fee_eur;
            let contra = if op.kind == Tipo::Compra {
                tesoreria
            } else {
                &puente
            };
            (
                concepto(if op.kind == Tipo::Compra {
                    "Compra"
                } else {
                    "Cobro en cripto"
                }),
                d(activo, total)
                    .into_iter()
                    .chain(h(contra, total))
                    .collect(),
            )
        }
        Tipo::Venta | Tipo::Pago => {
            let contra = if op.kind == Tipo::Venta {
                tesoreria
            } else {
                &puente
            };
            let mut contrap = Vec::new();
            contrap.extend(d(contra, op.eur - op.fee_eur));
            contrap.extend(d(&comisiones, op.fee_eur));
            (
                concepto(if op.kind == Tipo::Venta {
                    "Venta"
                } else {
                    "Pago en cripto"
                }),
                transmision(s, activo, op.eur, coste, contrap)?,
            )
        }
        // La comisión de una compra pagada en otra cripto es mayor precio de adquisición de lo
        // comprado (`capitaliza`, su subcuenta); si no, gasto.
        Tipo::Comision => (
            concepto("Comisión pagada en"),
            transmision(
                s,
                activo,
                op.eur,
                coste,
                d(capitaliza.unwrap_or(&comisiones), op.eur)
                    .into_iter()
                    .collect(),
            )?,
        ),
        Tipo::Permuta => {
            let a2 = activo2.ok_or_else(|| other("falta la subcuenta del activo recibido"))?;
            let mut lines = transmision(
                s,
                activo,
                op.eur,
                coste,
                d(a2, op.eur).into_iter().collect(),
            )?;
            lines.extend(d(&comisiones, op.fee_eur));
            lines.extend(h(tesoreria, op.fee_eur));
            (
                format!(
                    "Permuta {} {t} por {} {} ({plataforma})",
                    op.qty,
                    op.qty2.unwrap_or_default(),
                    op.ticker2
                        .as_deref()
                        .map(super::ticker_norm)
                        .unwrap_or_default()
                ),
                lines,
            )
        }
        Tipo::Recompensa => (
            concepto("Recompensa"),
            d(activo, op.eur)
                .into_iter()
                .chain(h(&code(&s.cuentas.recompensas)?, op.eur))
                .collect(),
        ),
        Tipo::Perdida => {
            let gasto = match s.naturaleza {
                Naturaleza::Existencias => code(&s.cuentas.variacion)?,
                Naturaleza::Intangible => code(&s.cuentas.perdida)?,
            };
            (
                concepto("Pérdida de"),
                d(&gasto, coste)
                    .into_iter()
                    .chain(h(activo, coste))
                    .collect(),
            )
        }
        Tipo::Traspaso => (
            concepto("Comisión del traspaso de"),
            d(&comisiones, op.fee_eur)
                .into_iter()
                .chain(h(tesoreria, op.fee_eur))
                .collect(),
        ),
    };
    if lines.len() < 2 {
        return Ok(None);
    }
    Ok(Some(JournalEntry {
        date: op.date,
        concept,
        origin: Origin::Cripto(op.id),
        lines,
    }))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Default)]
pub struct Contabilizado {
    pub asientos: usize,
    pub borrados: usize,
    pub saltadas: usize,
    pub avisos: Vec<String>,
}

/// Contabiliza las operaciones del ejercicio `year` que no tienen asiento (`rehacer` borra
/// antes los del ejercicio y los genera todos de nuevo, con los costes al día). El coste de
/// cada transmisión sale del método de declaración (FIFO en persona física).
pub fn contabilizar(
    v: &Vault,
    year: i32,
    rehacer: bool,
    now: OffsetDateTime,
) -> Result<Contabilizado> {
    let s = super::settings(v)?;
    let metodo = if super::es_persona_fisica(v) {
        Metodo::Fifo
    } else {
        s.metodo
    };
    let plataformas = super::platforms(v)?;
    let inicio = super::inicio::inicio(v)?;
    let corte = inicio.as_ref().map(|i| i.fecha);
    if let Some(c) = corte
        && super::fin_ejercicio(v, year) <= c
    {
        return Err(other(format!(
            "el ejercicio {year} acaba en el punto de partida ({}) o antes: lo contabilizó la \
             gestoría",
            super::inicio::dmy(c)
        )));
    }
    let despues = |d: time::Date| corte.is_none_or(|c| d > c);
    let (operaciones, mut avisos) = super::operaciones(v)?;
    let vinculos = super::comisiones_vinculadas(&super::ops(v, None)?);
    let calc = cripto::calcular_desde(
        &operaciones,
        metodo,
        None,
        true,
        inicio.as_ref().map(super::inicio::Inicio::motor).as_ref(),
    );
    let costes: BTreeMap<i64, Money> = calc.realizados.iter().map(|r| (r.op, r.coste)).collect();
    let valores: BTreeMap<i64, Money> = operaciones.iter().map(|o| (o.id, o.euros)).collect();
    avisos.extend(calc.avisos);
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let ts = now_str(now);
    let mut out = Contabilizado::default();
    let mut ops_year = super::ops(v, Some(year))?;
    ops_year.sort_by(|a, b| (a.date, &a.time, a.id).cmp(&(b.date, &b.time, b.id)));
    if rehacer {
        for o in ops_year.iter().filter(|o| despues(o.date)) {
            if let Some(e) = o.entry_id {
                if ejercicio_cerrado(&tx, o.date)? {
                    continue;
                }
                borrar_asiento(&tx, e)?;
                out.borrados += 1;
            }
        }
        // Las operaciones se vuelven a leer con el enlace borrado.
        ops_year = super::ops(v, Some(year))?;
        ops_year.sort_by(|a, b| (a.date, &a.time, a.id).cmp(&(b.date, &b.time, b.id)));
    }
    let mut historial = 0;
    for mut o in ops_year {
        if o.entry_id.is_some() {
            continue;
        }
        if !despues(o.date) {
            historial += 1;
            continue;
        }
        if ejercicio_cerrado(&tx, o.date)? {
            out.saltadas += 1;
            continue;
        }
        // El contravalor que usó el motor (de la operación o del precio del día).
        if let Some(e) = valores.get(&o.id) {
            o.eur = *e;
        }
        if o.eur.is_zero()
            && matches!(
                o.kind,
                Tipo::Permuta | Tipo::Pago | Tipo::Comision | Tipo::Recompensa
            )
        {
            out.saltadas += 1;
            avisos.push(format!(
                "{} del {} de {} {}: sin valor en euros, no se contabiliza.",
                o.kind.label(),
                o.date,
                o.qty,
                o.ticker
            ));
            continue;
        }
        let coste = costes.get(&o.id).copied().unwrap_or(Money::ZERO);
        let activo = subcuenta(
            &tx,
            &s,
            inicio.as_ref(),
            &super::ticker_norm(&o.ticker),
            &ts,
        )?;
        let activo2 = match &o.ticker2 {
            Some(t2) if o.kind == Tipo::Permuta => Some(subcuenta(
                &tx,
                &s,
                inicio.as_ref(),
                &super::ticker_norm(t2),
                &ts,
            )?),
            _ => None,
        };
        let teso = tesoreria(&s, &plataformas, o.platform_id)?;
        let nombre = plataformas
            .iter()
            .find(|p| p.id == o.platform_id)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        let capitaliza = match vinculos.get(&o.id) {
            Some(p) if p.kind == Tipo::Compra => Some(subcuenta(
                &tx,
                &s,
                inicio.as_ref(),
                &super::ticker_norm(&p.ticker),
                &ts,
            )?),
            _ => None,
        };
        let Some(entry) = asiento_de(
            &s,
            &o,
            coste,
            &activo,
            activo2.as_ref(),
            &teso,
            &nombre,
            capitaliza.as_ref(),
        )?
        else {
            continue;
        };
        let id = accounting::record_entry(&tx, &entry, now)?;
        tx.execute(
            "UPDATE crypto_op SET entry_id = ?1 WHERE id = ?2",
            params![id, o.id],
        )?;
        out.asientos += 1;
    }
    audit::append(
        &tx,
        &ts,
        Actor::User,
        "cripto.contabilizar",
        &year.to_string(),
        &serde_json::json!({ "asientos": out.asientos, "borrados": out.borrados, "metodo": metodo }),
    )?;
    tx.commit()?;
    if let Some(c) = corte.filter(|_| historial > 0) {
        avisos.insert(
            0,
            format!(
                "{historial} operaciones de hasta el {} no se contabilizan: son anteriores al \
                 punto de partida.",
                super::inicio::dmy(c)
            ),
        );
    }
    out.avisos = avisos;
    Ok(out)
}

/// Deterioro al cierre del ejercicio `year` con los precios de cierre: por cada posición cuyo
/// valor queda por debajo del coste, la diferencia. Revierte el deterioro del año anterior y
/// sustituye el asiento del mismo ejercicio. Devuelve el asiento (`None` si no hay deterioro ni
/// reversión).
pub fn deterioro(v: &Vault, year: i32, now: OffsetDateTime) -> Result<Option<i64>> {
    let s = super::settings(v)?;
    let (gasto, cuenta, reversion) = cuentas_deterioro(&s);
    let fecha = super::fin_ejercicio(v, year);
    let conn = v.connection();
    if ejercicio_cerrado(conn, fecha)? {
        return Err(other("el ejercicio está cerrado"));
    }
    let inicio = super::inicio_motor(v)?;
    if let Some(i) = inicio.as_ref().filter(|i| fecha <= i.fecha) {
        return Err(other(format!(
            "el cierre de {year} es anterior al punto de partida ({}) o coincide con él: lo hizo \
             la gestoría",
            super::inicio::dmy(i.fecha)
        )));
    }
    let plataformas = super::platforms(v)?;
    let (operaciones, _) = super::operaciones(v)?;
    let metodo = if super::es_persona_fisica(v) {
        Metodo::Fifo
    } else {
        s.metodo
    };
    let (posiciones, _) = super::posiciones_valoradas(
        v,
        &operaciones,
        metodo,
        fecha,
        &plataformas,
        inicio.as_ref(),
    )?;
    let sin_precio: Vec<&str> = posiciones
        .iter()
        .filter(|p| p.valor.is_none())
        .map(|p| p.ticker.as_str())
        .collect();
    if !sin_precio.is_empty() {
        return Err(other(format!(
            "faltan los precios de cierre a {fecha} de {}",
            sin_precio.join(", ")
        )));
    }
    // Deterioro por moneda (juntando plataformas): sólo si el valor queda por debajo del coste.
    let mut por_moneda: BTreeMap<String, (Money, Money)> = BTreeMap::new();
    for p in &posiciones {
        let e = por_moneda.entry(p.ticker.clone()).or_default();
        e.0 += p.coste;
        e.1 += p.valor.unwrap_or(Money::ZERO);
    }
    let nuevo: Money = por_moneda
        .values()
        .map(|(coste, valor)| (*coste - *valor).max(Money::ZERO))
        .sum();
    let tx = conn.unchecked_transaction()?;
    let ts = now_str(now);
    if let Some(e) = asiento_deterioro(&tx, year)? {
        borrar_asiento(&tx, e)?;
    }
    // Saldo del deterioro registrado desde aquí antes de este ejercicio (acreedor).
    let fy = accounting::fiscal_year(v, year)?;
    let previo_cents: i64 = tx.query_row(
        "SELECT COALESCE(SUM(l.credit - l.debit), 0) FROM journal_line l
         JOIN journal_entry e ON e.id = l.entry_id
         WHERE e.origin_kind = 'Cripto' AND e.origin_id < 0 AND e.date < ?1 AND l.account = ?2",
        params![fy.start.to_string(), cuenta],
        |r| r.get(0),
    )?;
    let previo = Money::from_cents(previo_cents);
    let mut lines = Vec::new();
    lines.extend(EntryLine::signed(code(&cuenta)?, previo, true));
    lines.extend(EntryLine::signed(code(&reversion)?, previo, false));
    lines.extend(EntryLine::signed(code(&gasto)?, nuevo, true));
    lines.extend(EntryLine::signed(code(&cuenta)?, nuevo, false));
    let id = if lines.len() < 2 {
        None
    } else {
        Some(accounting::record_entry(
            &tx,
            &JournalEntry {
                date: fecha,
                concept: format!("Deterioro de criptomonedas al cierre de {year}"),
                origin: Origin::Cripto(-i64::from(year)),
                lines,
            },
            now,
        )?)
    };
    audit::append(
        &tx,
        &ts,
        Actor::User,
        "cripto.deterioro",
        &year.to_string(),
        &serde_json::json!({ "deterioro": nuevo.cents(), "revertido": previo.cents(), "entry": id }),
    )?;
    tx.commit()?;
    Ok(id)
}
