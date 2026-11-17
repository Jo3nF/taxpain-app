//! Punto de partida de Criptomonedas: posiciones y coste pendiente a una fecha de corte.
//!
//! Para quien operó en exchanges que ya no existen o en DeFi sin registros: el cálculo no puede
//! salir de unas operaciones incompletas. Se toma lo que calculó la gestoría y figura en la
//! contabilidad cerrada y declarada (el coste pendiente a la fecha de cierre del último
//! ejercicio con el 200 presentado) y desde el día siguiente se sigue con el método de los
//! ajustes (precio medio ponderado o FIFO). Las operaciones hasta la fecha de corte quedan como
//! historial: se ven, pero no cuentan para el coste ni para las ganancias, ni se contabilizan.
//!
//! De dónde sale la propuesta ([`propuesta`]), que el usuario confirma:
//! - el **diario contable** importado: saldo a la fecha de corte de las cuentas de
//!   criptomonedas (por su nombre, «Criptomonedas», o las subcuentas que les dio TaxPain), sin el
//!   asiento de cierre: es el coste total;
//! - la **hoja de saldos** de la gestoría o de CoinTracking (importada en Criptomonedas o desde la
//!   carpeta, [`HojaSaldos`]): cantidades de cada moneda a esa fecha y, si la hoja lo da, el
//!   reparto del coste por moneda (celdas «BTC», «ETH»… con su importe y un «TOTAL» que es su
//!   suma);
//! - el **balance del 200** del ejercicio (casilla [00102], inmovilizado intangible), para
//!   cotejar: puede incluir otro intangible.
//!
//! Si las cantidades y el coste por moneda no se pueden deducir con seguridad (el reparto de la
//! hoja no cuadra con el diario, o no hay reparto y hay varias monedas), se propone lo que haya y
//! se dice qué falta repartir.
//!
//! Fiscalmente (sociedad): el resultado de las criptomonedas es contable y entra en la base del
//! Impuesto sobre Sociedades (art. 10.3 LIS); el precio medio ponderado es el criterio del PGC
//! para «bienes concretos que forman parte de un inventario de bienes intercambiables entre sí»
//! (NRV 10.ª.1.3, existencias) y, una vez elegido, se mantiene (principio de uniformidad del
//! Marco Conceptual: «Adoptado un criterio dentro de las alternativas que, en su caso, se
//! permitan, deberá mantenerse en el tiempo…»; los dos leídos en el BOE el 2026-11-02).
//! TODO(VERIFY): para el inmovilizado intangible (NRV 5.ª y 6.ª) el PGC no dice cómo asignar el
//! coste de unidades intercambiables; se aplica el de la NRV 10.ª por analogía, como la gestoría.
//! La deuda de los ejercicios prescritos ya no se puede liquidar (art.
//! 66.a LGT, cuatro años), pero la AEAT puede comprobar hechos de esos ejercicios que tengan
//! efectos en otros no prescritos (arts. 66 bis.1 y 115.1 LGT, leídos en el texto consolidado
//! del BOE el 2026-11-02), como el coste de las monedas que se vendan después: por eso conviene
//! guardar los soportes que haya (hoja de la gestoría, historiales, extractos).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::cripto::{self as motor, Metodo, Qty, SIN_PLATAFORMA};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, Duration, OffsetDateTime};

use super::importar::{celda, col, cols_saldos, fecha_hora};
use super::{Naturaleza, ticker_norm};
use crate::error::{AppError, Result};
use crate::import::{Hoja, norm, parse_amount};
use crate::invoicing::now_str;

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

/// Fecha como la lee el usuario (31/12/2025).
pub(crate) fn dmy(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

/// Ajuste donde se guarda la última hoja de saldos leída.
const HOJA_KEY: &str = "cripto_hoja_saldos";

/// Una moneda del punto de partida.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PosicionInicio {
    pub ticker: String,
    /// Plataforma o monedero donde está (`None`: no se sabe; sirve para cualquiera).
    #[serde(default)]
    pub platform_id: Option<i64>,
    pub cantidad: Qty,
    /// Coste pendiente (el que queda por restar en ventas futuras).
    pub coste: Money,
}

/// Punto de partida: fecha de corte y posiciones con su coste pendiente.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inicio {
    /// Último día que no cuenta: el cálculo sigue desde el día siguiente.
    pub fecha: Date,
    /// Cuenta del diario donde está ese coste (la de la gestoría); las ventas posteriores de
    /// estas monedas se contabilizan contra ella. Vacía: la subcuenta de cada moneda.
    #[serde(default)]
    pub cuenta: String,
    /// De dónde sale (para el usuario).
    #[serde(default)]
    pub origen: String,
    #[serde(default)]
    pub nota: String,
    pub posiciones: Vec<PosicionInicio>,
}

impl Inicio {
    pub fn coste_total(&self) -> Money {
        self.posiciones.iter().map(|p| p.coste).sum()
    }

    /// Lo que necesita el motor de cálculo.
    pub(crate) fn motor(&self) -> motor::Inicio {
        motor::Inicio {
            fecha: self.fecha,
            posiciones: self
                .posiciones
                .iter()
                .map(|p| motor::Inicial {
                    ticker: ticker_norm(&p.ticker),
                    plataforma: p.platform_id.unwrap_or(SIN_PLATAFORMA),
                    cantidad: p.cantidad,
                    coste: p.coste,
                })
                .collect(),
        }
    }

    /// ¿Es una moneda del punto de partida?
    pub(crate) fn tiene(&self, ticker: &str) -> bool {
        let t = ticker_norm(ticker);
        self.posiciones.iter().any(|p| ticker_norm(&p.ticker) == t)
    }
}

pub fn inicio(v: &Vault) -> Result<Option<Inicio>> {
    inicio_conn(v.connection())
}

pub(crate) fn inicio_conn(conn: &Connection) -> Result<Option<Inicio>> {
    let cab: Option<(String, String, String, String)> = conn
        .query_row(
            "SELECT date, account, source, note FROM crypto_opening WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((date, cuenta, origen, nota)) = cab else {
        return Ok(None);
    };
    let fecha = Date::parse(&date, &time::format_description::well_known::Iso8601::DATE)
        .map_err(|_| other(format!("fecha del punto de partida no válida: {date}")))?;
    let mut st = conn.prepare(
        "SELECT ticker, platform_id, qty, cost FROM crypto_opening_position ORDER BY ticker, id",
    )?;
    let posiciones = st
        .query_map([], |r| {
            let qty: String = r.get(2)?;
            Ok(PosicionInicio {
                ticker: r.get(0)?,
                platform_id: r.get(1)?,
                cantidad: Qty::parse(&qty).unwrap_or(Qty::ZERO),
                coste: Money::from_cents(r.get(3)?),
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(Some(Inicio {
        fecha,
        cuenta,
        origen,
        nota,
        posiciones,
    }))
}

/// Fecha de corte del punto de partida, si lo hay.
pub(crate) fn fecha_corte(conn: &Connection) -> Result<Option<Date>> {
    Ok(inicio_conn(conn)?.map(|i| i.fecha))
}

/// Lo que cuenta para el cálculo y los asientos: fecha, cuenta y posiciones en orden.
type Esencial = (Date, String, Vec<(String, Option<i64>, Qty, Money)>);

/// Lo que cuenta para el cálculo y los asientos (sin el origen ni la nota), en orden.
fn esencial(i: &Inicio) -> Esencial {
    let mut p: Vec<_> = i
        .posiciones
        .iter()
        .map(|p| (ticker_norm(&p.ticker), p.platform_id, p.cantidad, p.coste))
        .collect();
    p.sort();
    (i.fecha, i.cuenta.trim().to_owned(), p)
}

/// Avisos al cambiar el punto de partida (o quitarlo) si ya hay asientos de operaciones: los de
/// después del corte se hicieron con otro coste (hay que rehacerlos) y los de hasta el corte
/// pasan a ser historial (los contabilizó la gestoría).
fn avisos_cambio(
    conn: &Connection,
    antes: Option<&Inicio>,
    despues: Option<&Inicio>,
) -> Result<Vec<String>> {
    if antes.map(esencial) == despues.map(esencial) {
        return Ok(Vec::new());
    }
    let corte = despues.map(|i| i.fecha);
    let fechas: Vec<String> = conn
        .prepare("SELECT date FROM crypto_op WHERE entry_id IS NOT NULL")?
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    let fechas: Vec<Date> = fechas
        .iter()
        .filter_map(|d| Date::parse(d, &time::format_description::well_known::Iso8601::DATE).ok())
        .collect();
    let mut out = Vec::new();
    let mut años: Vec<i32> = fechas
        .iter()
        .filter(|d| corte.is_none_or(|c| **d > c))
        .map(|d| d.year())
        .collect();
    let n = años.len();
    años.sort_unstable();
    años.dedup();
    if n > 0 {
        out.push(format!(
            "{n} {} de después del corte ya {} asiento, hecho con el coste de antes del cambio: \
             rehaz los asientos (y el deterioro, si lo hay) de {} en Criptomonedas («Rehacer los \
             asientos del ejercicio»).",
            if n == 1 { "operación" } else { "operaciones" },
            if n == 1 { "tiene" } else { "tienen" },
            años
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if let Some(c) = corte {
        let h = fechas.iter().filter(|d| **d <= c).count();
        if h > 0 {
            out.push(format!(
                "{h} {} de hasta el {} {} asiento de TaxPain y ahora {} historial (lo contabilizó \
                 la gestoría): revisa en el diario que no estén dos veces.",
                if h == 1 { "operación" } else { "operaciones" },
                dmy(c),
                if h == 1 { "tiene" } else { "tienen" },
                if h == 1 { "es" } else { "son" },
            ));
        }
    }
    Ok(out)
}

/// Guarda el punto de partida (sustituye al que hubiera). Devuelve avisos si el cambio afecta a
/// asientos ya hechos.
pub fn guardar(v: &Vault, i: &Inicio, now: OffsetDateTime) -> Result<Vec<String>> {
    if i.fecha.year() < 2009 {
        return Err(other(
            "revisa la fecha: no había criptomonedas antes de 2009",
        ));
    }
    if i.fecha >= now.date() {
        return Err(other(format!(
            "la fecha de corte ({}) ha de ser anterior a hoy: es el cierre del último ejercicio \
             cerrado y declarado",
            dmy(i.fecha)
        )));
    }
    let cuenta = i.cuenta.trim();
    if !cuenta.is_empty() {
        taxpain_core::accounting::AccountCode::new(cuenta)
            .map_err(|_| other(format!("cuenta no válida: «{cuenta}»")))?;
    }
    let conn = v.connection();
    let antes = inicio_conn(conn)?;
    let tx = conn.unchecked_transaction()?;
    let ts = now_str(now);
    let mut vistas: Vec<(String, Option<i64>)> = Vec::new();
    for p in &i.posiciones {
        let t = ticker_norm(&p.ticker);
        if p.cantidad.is_zero() || p.cantidad.is_negative() {
            return Err(other(format!(
                "{t}: la cantidad ha de ser mayor que cero (quita la fila si no queda nada)"
            )));
        }
        if p.coste.is_negative() {
            return Err(other(format!("{t}: el coste no puede ser negativo")));
        }
        if vistas.contains(&(t.clone(), p.platform_id)) {
            return Err(other(format!(
                "{t} está dos veces en la misma plataforma: júntalas en una fila"
            )));
        }
        if let Some(id) = p.platform_id {
            let existe: Option<i64> = tx
                .query_row("SELECT 1 FROM crypto_platform WHERE id = ?1", [id], |r| {
                    r.get(0)
                })
                .optional()?;
            if existe.is_none() {
                return Err(other(format!("{t}: esa plataforma ya no existe")));
            }
        }
        super::ensure_asset(&tx, &t, None, &ts)?;
        vistas.push((t, p.platform_id));
    }
    tx.execute("DELETE FROM crypto_opening_position", [])?;
    tx.execute(
        "INSERT INTO crypto_opening(id, date, account, source, note, updated_at)
         VALUES (1, ?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(id) DO UPDATE SET date = excluded.date, account = excluded.account,
           source = excluded.source, note = excluded.note, updated_at = excluded.updated_at",
        params![
            i.fecha.to_string(),
            cuenta,
            i.origen.trim(),
            i.nota.trim(),
            ts
        ],
    )?;
    for p in &i.posiciones {
        tx.execute(
            "INSERT INTO crypto_opening_position(ticker, platform_id, qty, cost)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                ticker_norm(&p.ticker),
                p.platform_id,
                p.cantidad.to_text(),
                p.coste.cents()
            ],
        )?;
    }
    audit::append(
        &tx,
        &ts,
        Actor::User,
        "cripto.punto_de_partida",
        &i.fecha.to_string(),
        &serde_json::json!({
            "monedas": i.posiciones.len(),
            "coste": i.coste_total().cents(),
            "cuenta": cuenta,
        }),
    )?;
    tx.commit()?;
    avisos_cambio(conn, antes.as_ref(), inicio_conn(conn)?.as_ref())
}

/// Quita el punto de partida: el cálculo vuelve a salir de todas las operaciones. Devuelve avisos
/// si ya hay asientos hechos con él.
pub fn borrar(v: &Vault, now: OffsetDateTime) -> Result<Vec<String>> {
    let conn = v.connection();
    let antes = inicio_conn(conn)?;
    let tx = conn.unchecked_transaction()?;
    let posiciones = tx.execute("DELETE FROM crypto_opening_position", [])?;
    let n = tx.execute("DELETE FROM crypto_opening", [])?;
    // También si sólo había saldos sueltos, sin la cabecera (una importación a medias).
    if n > 0 || posiciones > 0 {
        audit::append(
            &tx,
            &now_str(now),
            Actor::User,
            "cripto.punto_de_partida_quitado",
            "",
            &serde_json::json!({}),
        )?;
    }
    tx.commit()?;
    avisos_cambio(conn, antes.as_ref(), None)
}

// ---------------- Hoja de saldos ----------------

/// Unidades de una moneda a una fecha, de una hoja de saldos.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaldoHoja {
    pub fecha: Date,
    pub ticker: String,
    pub cantidad: Qty,
    /// Valor en euros que da la hoja ese día, si lo da.
    #[serde(default)]
    pub valor: Option<Money>,
}

/// Lo que sirve de una hoja de saldos por fecha (la de la gestoría, la de CoinTracking) para el
/// punto de partida: las unidades de cada moneda en cada fecha y, si la hoja lo da, el coste
/// pendiente por moneda.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct HojaSaldos {
    pub fichero: String,
    pub saldos: Vec<SaldoHoja>,
    /// Coste por moneda: celdas con el nombre de la moneda y su importe al lado, encima de un
    /// «TOTAL» que es su suma.
    #[serde(default)]
    pub costes: Vec<(String, Money)>,
    #[serde(default)]
    pub total: Option<Money>,
}

fn es_dinero(t: &str) -> bool {
    matches!(t, "EUR" | "USD" | "GBP" | "CHF" | "JPY")
}

/// Lee una hoja de saldos por fecha y el reparto del coste, si los hay.
pub(crate) fn leer_hoja(fichero: &str, hojas: &[Hoja]) -> Option<HojaSaldos> {
    let mut saldos = Vec::new();
    'hojas: for h in hojas {
        let filas = &h.filas;
        for (i, cab) in filas.iter().enumerate().take(15) {
            let cols = cols_saldos(filas, i);
            let Some(c_fecha) = col(cab, &["fecha", "date"]) else {
                continue;
            };
            if cols.is_empty() {
                continue;
            }
            for r in filas.iter().skip(i + 1) {
                let Some((fecha, _)) = fecha_hora(&celda(r, Some(c_fecha))) else {
                    continue;
                };
                for (ticker, cj, vj) in &cols {
                    if es_dinero(ticker) {
                        continue;
                    }
                    let Some(q) = Qty::parse(&celda(r, Some(*cj))) else {
                        continue;
                    };
                    if q.is_zero() || q.is_negative() {
                        continue;
                    }
                    let valor = parse_amount(&celda(r, Some(*vj)));
                    // La misma moneda en dos columnas (SOL y SOL2 de CoinTracking): una fila.
                    if let Some(s) = saldos
                        .iter_mut()
                        .find(|s: &&mut SaldoHoja| s.fecha == fecha && s.ticker == *ticker)
                    {
                        s.cantidad = s.cantidad.checked_add(q).unwrap_or(s.cantidad);
                        s.valor = match (s.valor, valor) {
                            (Some(a), Some(b)) => Some(a + b),
                            _ => None,
                        };
                        continue;
                    }
                    saldos.push(SaldoHoja {
                        fecha,
                        ticker: ticker.clone(),
                        cantidad: q,
                        valor,
                    });
                }
            }
            if !saldos.is_empty() {
                break 'hojas;
            }
        }
    }
    if saldos.is_empty() {
        return None;
    }
    let monedas: Vec<String> = saldos.iter().map(|s| s.ticker.clone()).collect();
    let (costes, total) = reparto(hojas, &monedas).unwrap_or_default();
    Some(HojaSaldos {
        fichero: fichero.to_owned(),
        saldos,
        costes,
        total,
    })
}

/// Reparto del coste por moneda en cualquier hoja: en una misma columna, filas seguidas con el
/// nombre de una moneda de `monedas` y su importe en la siguiente celda con algo, y debajo un
/// «TOTAL» igual a su suma (al céntimo, con dos de margen por el redondeo).
/// Coste por moneda y su total.
type Reparto = (Vec<(String, Money)>, Option<Money>);

fn reparto(hojas: &[Hoja], monedas: &[String]) -> Option<Reparto> {
    for h in hojas {
        // (fila, columna) → (etiqueta, importe)
        let mut celdas: BTreeMap<(usize, usize), (String, Money)> = BTreeMap::new();
        for (i, fila) in h.filas.iter().enumerate() {
            for (j, c) in fila.iter().enumerate() {
                let t = super::importar::ticker_cointracking(c);
                if t != "TOTAL" && !monedas.contains(&t) {
                    continue;
                }
                let Some(valor) = fila.iter().skip(j + 1).find(|x| !x.trim().is_empty()) else {
                    continue;
                };
                if let Some(m) = parse_amount(valor) {
                    celdas.insert((i, j), (t, m));
                }
            }
        }
        for (&(i, j), (t, total)) in &celdas {
            if t != "TOTAL" || total.cents() <= 0 {
                continue;
            }
            let mut costes = Vec::new();
            let mut k = i;
            while k > 0 {
                k -= 1;
                match celdas.get(&(k, j)) {
                    Some((m, c)) if m != "TOTAL" => costes.push((m.clone(), *c)),
                    _ => break,
                }
            }
            let suma: Money = costes.iter().map(|(_, c)| *c).sum();
            if costes.len() >= 2 && (suma - *total).abs().cents() <= 2 {
                costes.reverse();
                return Some((costes, Some(*total)));
            }
        }
    }
    None
}

/// Guarda la hoja leída para la propuesta del punto de partida.
pub(crate) fn recordar_hoja(v: &Vault, h: &HojaSaldos) -> Result<()> {
    v.set_setting(HOJA_KEY, h)?;
    Ok(())
}

/// La última hoja de saldos importada.
pub fn hoja(v: &Vault) -> Result<Option<HojaSaldos>> {
    Ok(v.setting(HOJA_KEY)?)
}

// ---------------- Propuesta ----------------

/// Saldo de una cuenta de criptomonedas en el diario.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CuentaCripto {
    pub cuenta: String,
    pub nombre: String,
    pub saldo: Money,
}

/// Punto de partida propuesto con lo que hay (el usuario lo confirma o lo corrige).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Propuesta {
    pub fecha: Date,
    pub ejercicio: i32,
    /// Por qué ese ejercicio.
    pub motivo: String,
    pub posiciones: Vec<PosicionInicio>,
    /// Cuenta del diario con el coste (si es una).
    pub cuenta: String,
    pub cuentas: Vec<CuentaCripto>,
    /// Coste total según el diario a la fecha de corte.
    pub coste_diario: Option<Money>,
    /// Inmovilizado intangible del balance del 200 de ese ejercicio ([00102]).
    pub casilla_102: Option<Money>,
    /// Hoja de saldos de la que salen las cantidades.
    pub hoja: Option<String>,
    pub fecha_cantidades: Option<Date>,
    /// Las cantidades salen de las operaciones importadas (pueden estar incompletas).
    pub cantidades_de_operaciones: bool,
    /// El coste por moneda está deducido con seguridad (la hoja lo reparte y cuadra con el
    /// diario, o hay una sola moneda).
    pub reparto_seguro: bool,
    /// Coste del diario que queda por repartir entre las monedas.
    pub por_repartir: Money,
    /// Monedas sin coste que valen menos de [`UMBRAL_POLVO`] a la fecha de las cantidades
    /// (restos de operaciones): no van en `posiciones`, pero se pueden añadir.
    pub polvo: Vec<PosicionInicio>,
    pub origen: String,
    pub notas: Vec<String>,
}

/// Por debajo de este valor, una moneda sin coste es polvo: restos que no merece la pena seguir.
pub const UMBRAL_POLVO: Money = Money::from_cents(100);

fn nombre_cripto(nombre: &str) -> bool {
    let n = norm(nombre);
    [
        "cripto",
        "crypto",
        "bitcoin",
        "ethereum",
        "moneda virtual",
        "monedas virtuales",
    ]
    .iter()
    .any(|k| n.contains(k))
}

/// Cuenta de activo con criptomonedas: por su nombre o porque TaxPain se la dio a una moneda.
/// Ni amortizaciones ni deterioros (28, 29, 39).
fn es_cuenta_cripto(cuenta: &str, nombre: &str, asignadas: &[String]) -> bool {
    let activo = cuenta.starts_with('2') || cuenta.starts_with('3');
    let correccion = ["28", "29", "39"].iter().any(|p| cuenta.starts_with(p));
    activo && !correccion && (asignadas.iter().any(|a| a == cuenta) || nombre_cripto(nombre))
}

/// Saldos de las cuentas de criptomonedas a `fecha` (sin el asiento de cierre). Si el diario de
/// ese ejercicio no está, el asiento de apertura del siguiente.
fn saldos_diario(v: &Vault, fecha: Date, elegida: Option<&str>) -> Result<Vec<CuentaCripto>> {
    let conn = v.connection();
    let fy = crate::accounting::fiscal_year_at(v, fecha)?;
    let asignadas: Vec<String> = asignadas(conn)?.into_keys().collect();
    let consulta = |sql: &str, year: i32, hasta: Option<&str>| -> Result<Vec<(String, i64)>> {
        let mut st = conn.prepare(sql)?;
        let rows = st
            .query_map(params![year, hasta], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    };
    let mut filas = consulta(
        "SELECT l.account, SUM(l.debit - l.credit) FROM journal_line l
         JOIN journal_entry e ON e.id = l.entry_id
         WHERE e.fiscal_year = ?1 AND e.date <= ?2 AND e.origin_kind <> 'Cierre'
         GROUP BY l.account",
        fy.year,
        Some(&fecha.to_string()),
    )?;
    if filas.is_empty() {
        filas = consulta(
            "SELECT l.account, SUM(l.debit - l.credit) FROM journal_line l
             JOIN journal_entry e ON e.id = l.entry_id
             WHERE e.fiscal_year = ?1 AND e.origin_kind = 'Apertura' AND ?2 IS NULL
             GROUP BY l.account",
            fy.year + 1,
            None,
        )?;
    }
    let mut out = Vec::new();
    for (cuenta, saldo) in filas {
        if saldo == 0 {
            continue;
        }
        let nombre = crate::accounting::account_name(conn, &cuenta)?.unwrap_or_default();
        let es = match elegida {
            Some(c) => cuenta.starts_with(c),
            None => es_cuenta_cripto(&cuenta, &nombre, &asignadas),
        };
        if es {
            out.push(CuentaCripto {
                cuenta,
                nombre,
                saldo: Money::from_cents(saldo),
            });
        }
    }
    Ok(out)
}

/// Subcuentas que TaxPain dio a cada moneda al contabilizar: cuenta → moneda.
fn asignadas(conn: &Connection) -> Result<BTreeMap<String, String>> {
    let mut st = conn.prepare("SELECT account, ticker FROM crypto_asset WHERE account <> ''")?;
    let rows = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(rows)
}

/// Coste por moneda que dice el propio diario: cada cuenta es de una moneda (la subcuenta que le
/// dio TaxPain, o su nombre dice cuál, «Criptomonedas BTC»). `None` si alguna no se sabe.
fn reparto_del_diario(
    conn: &Connection,
    cuentas: &[CuentaCripto],
    monedas: &[String],
) -> Result<Option<Vec<(String, Money)>>> {
    let asignadas = asignadas(conn)?;
    let mut out: Vec<(String, Money)> = Vec::new();
    for c in cuentas {
        let por_nombre = || {
            let palabras: Vec<String> = c
                .nombre
                .split(|ch: char| !ch.is_ascii_alphanumeric())
                .map(ticker_norm)
                .collect();
            let mut en: Vec<&String> = monedas.iter().filter(|m| palabras.contains(m)).collect();
            en.dedup();
            match en.as_slice() {
                [m] => Some((*m).clone()),
                _ => None,
            }
        };
        let Some(t) = asignadas.get(&c.cuenta).cloned().or_else(por_nombre) else {
            return Ok(None);
        };
        match out.iter_mut().find(|(x, _)| *x == t) {
            Some(e) => e.1 += c.saldo,
            None => out.push((t, c.saldo)),
        }
    }
    Ok((!out.is_empty()).then_some(out))
}

/// Último ejercicio cerrado y declarado antes de `hoy`: el último con el 200 presentado (según
/// la AEAT o la gestoría) o cerrado en TaxPain; si no hay, el último con diario; si no, el año
/// anterior.
fn ejercicio_de_corte(v: &Vault, hoy: Date) -> Result<(i32, String)> {
    let mut cands: Vec<(i32, &'static str)> = Vec::new();
    for x in crate::presentado_aeat::vigentes(v)? {
        if x.model == "200" {
            cands.push((x.year, "es el último con el 200 presentado"));
        }
    }
    for f in crate::presentados::list(v, None)? {
        if f.model == "200" {
            cands.push((f.year, "es el último con el 200 presentado"));
        }
    }
    let cerrados: Vec<i32> = v
        .connection()
        .prepare("SELECT year FROM fiscal_year WHERE closed = 1")?
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    cands.extend(
        cerrados
            .into_iter()
            .map(|y| (y, "es el último ejercicio cerrado")),
    );
    let antes = |y: i32| super::fin_ejercicio(v, y) < hoy;
    if let Some((y, m)) = cands
        .into_iter()
        .filter(|(y, _)| antes(*y))
        .max_by_key(|(y, _)| *y)
    {
        return Ok((y, m.to_owned()));
    }
    let con_diario: Vec<i32> = v
        .connection()
        .prepare("SELECT DISTINCT fiscal_year FROM journal_entry")?
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    if let Some(y) = con_diario.into_iter().filter(|y| antes(*y)).max() {
        return Ok((
            y,
            "es el último con diario (no hay ningún 200 presentado ni ejercicio cerrado)".into(),
        ));
    }
    Ok((
        hoy.year() - 1,
        "es el año anterior (no hay 200, ejercicio cerrado ni diario)".into(),
    ))
}

/// Propuesta de punto de partida con lo que hay en el almacén. `cuenta`: la del diario donde
/// está el coste, si el usuario la sabe (si no, se buscan las de criptomonedas por su nombre).
pub fn propuesta(v: &Vault, hoy: Date, cuenta: Option<&str>) -> Result<Propuesta> {
    let (ejercicio, motivo) = ejercicio_de_corte(v, hoy)?;
    let fecha = super::fin_ejercicio(v, ejercicio);
    let mut notas = Vec::new();
    let mut origen = Vec::new();

    // Diario.
    let elegida = cuenta.map(str::trim).filter(|c| !c.is_empty());
    let cuentas = saldos_diario(v, fecha, elegida)?;
    let coste_diario = (!cuentas.is_empty()).then(|| cuentas.iter().map(|c| c.saldo).sum());
    let cuenta = match cuentas.as_slice() {
        [c] => c.cuenta.clone(),
        _ => String::new(),
    };
    match coste_diario {
        Some(c) => origen.push(format!(
            "diario a {} ({}: {c})",
            dmy(fecha),
            cuentas
                .iter()
                .map(|x| format!("{} {}", x.cuenta, x.nombre))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        None => notas.push(match elegida {
            Some(c) => format!(
                "La cuenta {c} no tiene saldo en el diario a {}: revisa la cuenta o importa el \
                 diario de {ejercicio}.",
                dmy(fecha)
            ),
            None => format!(
                "El diario no tiene a {} ninguna cuenta de criptomonedas con saldo (se buscan \
                 por el nombre, «Criptomonedas», o las subcuentas que les dio TaxPain): pon la \
                 cuenta y vuelve a pedir los cálculos, importa el diario de {ejercicio} o pon el \
                 coste a mano.",
                dmy(fecha)
            ),
        }),
    }
    if cuentas.iter().any(|c| c.saldo.is_negative()) {
        notas.push(
            "Alguna cuenta de criptomonedas tiene saldo acreedor: revisa el diario antes de \
             tomarlo como coste."
                .into(),
        );
    }

    // Balance del 200.
    let casilla_102 = crate::presentado_aeat::vigentes(v)?
        .into_iter()
        .filter(|x| x.model == "200" && x.year == ejercicio)
        .find_map(|x| x.boxes?.casillas.get(&102).copied());
    match (casilla_102, coste_diario) {
        (Some(b), Some(d)) if b == d => {
            notas.push(format!(
                "Cuadra con el inmovilizado intangible del balance del 200 de {ejercicio} \
                 ([00102]: {b})."
            ));
            origen.push(format!("200 de {ejercicio}"));
        }
        (Some(b), Some(d)) => notas.push(format!(
            "El balance del 200 de {ejercicio} declara {b} de inmovilizado intangible ([00102]) \
             y el diario {d} en cuentas de criptomonedas: si hay otro intangible (aplicaciones \
             informáticas, marcas…), la diferencia es eso; si no, revisa cuál vale."
        )),
        _ => {}
    }

    // Cantidades: de la hoja de saldos o, si no, de las operaciones importadas.
    let mut posiciones: Vec<PosicionInicio> = Vec::new();
    let mut fecha_cantidades = None;
    let mut hoja_nombre = None;
    let mut cantidades_de_operaciones = false;
    let h = hoja(v)?;
    if let Some(h) = &h {
        let limite = fecha + Duration::days(1);
        let mejor = h
            .saldos
            .iter()
            .map(|s| s.fecha)
            .filter(|f| *f <= limite)
            .max();
        match mejor {
            Some(f) if (fecha - f).whole_days() <= 366 => {
                posiciones = h
                    .saldos
                    .iter()
                    .filter(|s| s.fecha == f)
                    .map(|s| PosicionInicio {
                        ticker: s.ticker.clone(),
                        platform_id: None,
                        cantidad: s.cantidad,
                        coste: Money::ZERO,
                    })
                    .collect();
                fecha_cantidades = Some(f);
                hoja_nombre = Some(h.fichero.clone());
                origen.push(format!("hoja de saldos «{}» a {}", h.fichero, dmy(f)));
                if f < fecha {
                    notas.push(format!(
                        "Las cantidades son las de la hoja a {} (la última fecha anterior al \
                         corte): comprueba que no hubo movimientos hasta el {}.",
                        dmy(f),
                        dmy(fecha)
                    ));
                }
            }
            _ => notas.push(format!(
                "La hoja de saldos «{}» no tiene cantidades cerca del {}.",
                h.fichero,
                dmy(fecha)
            )),
        }
    }
    if posiciones.is_empty() {
        let (ops, _) = super::operaciones(v)?;
        let c = motor::calcular(&ops, Metodo::Pmp, Some(fecha), false);
        posiciones = c
            .posiciones
            .iter()
            .map(|p| PosicionInicio {
                ticker: p.ticker.clone(),
                platform_id: None,
                cantidad: p.cantidad,
                coste: Money::ZERO,
            })
            .collect();
        if !posiciones.is_empty() {
            cantidades_de_operaciones = true;
            notas.push(format!(
                "Las cantidades salen de las operaciones importadas hasta el {}: si faltan \
                 operaciones (exchanges que cerraron, DeFi), no son de fiar; corrígelas con las \
                 de la gestoría o importa su hoja de saldos.",
                dmy(fecha)
            ));
        } else {
            notas.push(
                "No hay cantidades: importa la hoja de saldos de la gestoría (Importar → Carpeta \
                 o «Importar fichero…») o añade las monedas a mano."
                    .into(),
            );
        }
    }

    // Coste por moneda: el de la hoja o, si cada cuenta del diario es de una moneda, el suyo.
    let mut reparto_seguro = false;
    let monedas: Vec<String> = posiciones.iter().map(|p| p.ticker.clone()).collect();
    let mut costes = h.as_ref().map(|h| h.costes.clone()).unwrap_or_default();
    let mut fuente = "la hoja";
    if costes.is_empty()
        && cuentas.len() > 1
        && let Some(r) = reparto_del_diario(v.connection(), &cuentas, &monedas)?
    {
        costes = r;
        fuente = "el saldo de su cuenta del diario";
    }
    if !costes.is_empty() && !cantidades_de_operaciones {
        let mut completo = true;
        for (t, c) in &costes {
            match posiciones.iter_mut().find(|p| &p.ticker == t) {
                Some(p) => p.coste = *c,
                None => {
                    completo = false;
                    notas.push(format!(
                        "La hoja da a {t} un coste de {c}, pero no hay cantidad de {t} a esa \
                         fecha."
                    ));
                }
            }
        }
        let suma: Money = costes.iter().map(|(_, c)| *c).sum();
        match coste_diario {
            Some(d) if (d - suma).abs().cents() <= 2 => {
                reparto_seguro = completo;
                notas.push(format!(
                    "El coste por moneda sale de {fuente} ({}) y suma lo que dice el diario.",
                    costes
                        .iter()
                        .map(|(t, c)| format!("{t} {c}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ));
            }
            Some(d) => notas.push(format!(
                "La hoja reparte {suma} y el diario dice {d}: comprueba cuál vale antes de \
                 guardar."
            )),
            None => notas.push(format!(
                "El coste por moneda es el de la hoja (en total {suma}); no hay diario con el \
                 que cuadrarlo."
            )),
        }
    } else if let Some(d) = coste_diario
        && let [p] = posiciones.as_mut_slice()
    {
        p.coste = d;
        reparto_seguro = !cantidades_de_operaciones;
    }
    // Polvo: sin coste y por menos de 1 € con el valor de la hoja o el precio de cierre.
    let fecha_valor = fecha_cantidades.unwrap_or(fecha);
    let mut polvo = Vec::new();
    let mut quedan = Vec::new();
    for p in posiciones {
        let de_hoja = h
            .as_ref()
            .filter(|_| fecha_cantidades.is_some())
            .and_then(|h| {
                h.saldos
                    .iter()
                    .find(|s| s.fecha == fecha_valor && s.ticker == p.ticker)
                    .and_then(|s| s.valor)
            });
        let valor = match de_hoja {
            Some(x) => Some(x),
            None => super::precio_a(v, &p.ticker, fecha)?.and_then(|(_, pr)| p.cantidad.valor(pr)),
        };
        if p.coste.is_zero() && valor.is_some_and(|x| x < UMBRAL_POLVO) {
            polvo.push(p);
        } else {
            quedan.push(p);
        }
    }
    let posiciones = quedan;
    if !polvo.is_empty() {
        notas.push(format!(
            "{} {} menos de 1 € a {} y no {} coste: es polvo y no se propone. Si quieres \
             seguirlo, añádelo con «Añadir el polvo».",
            polvo
                .iter()
                .map(|p| p.ticker.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            if polvo.len() == 1 { "vale" } else { "valen" },
            dmy(fecha_valor),
            if polvo.len() == 1 { "tiene" } else { "tienen" },
        ));
    }
    let asignado: Money = posiciones.iter().map(|p| p.coste).sum();
    let por_repartir = coste_diario
        .map(|d| d - asignado)
        .filter(|r| r.cents() > 0)
        .unwrap_or(Money::ZERO);
    if por_repartir.cents() > 0 {
        notas.push(format!(
            "Falta repartir {por_repartir} del diario entre las monedas: pon a cada una su \
             coste pendiente (el de la hoja de la gestoría). Las que no estén en el balance van \
             con coste cero."
        ));
    }
    if !posiciones.is_empty() && posiciones.iter().any(|p| p.coste.is_zero()) {
        let cero: Vec<&str> = posiciones
            .iter()
            .filter(|p| p.coste.is_zero())
            .map(|p| p.ticker.as_str())
            .collect();
        if reparto_seguro || por_repartir.is_zero() {
            let una = cero.len() == 1;
            notas.push(format!(
                "{} {} con coste cero: no {} en el balance, así que lo que se cobre por {} será \
                 ganancia.",
                cero.join(", "),
                if una { "va" } else { "van" },
                if una { "está" } else { "están" },
                if una { "ella" } else { "ellas" },
            ));
        }
    }
    if cuenta.starts_with('2') && super::settings(v)?.naturaleza == Naturaleza::Existencias {
        notas.push(format!(
            "La gestoría las tiene en el inmovilizado (cuenta {cuenta}): en «Cómo se valoran» \
             elige «Inmovilizado intangible» para que las ventas se contabilicen igual."
        ));
    }
    Ok(Propuesta {
        fecha,
        ejercicio,
        motivo: format!("{ejercicio} {motivo}"),
        posiciones,
        cuenta,
        cuentas,
        coste_diario,
        casilla_102,
        hoja: hoja_nombre,
        fecha_cantidades,
        cantidades_de_operaciones,
        reparto_seguro,
        por_repartir,
        polvo,
        origen: if origen.is_empty() {
            "a mano".into()
        } else {
            format!("Cálculos de la gestoría: {}", origen.join("; "))
        },
        notas,
    })
}

// ---------------- Huecos ----------------

/// Operaciones incompletas sin punto de partida: ventas o traspasos de más unidades de las que
/// había (falta una compra, un traspaso o lo de antes). Vacío si hay punto de partida.
pub fn huecos(v: &Vault) -> Result<Vec<motor::Hueco>> {
    if inicio(v)?.is_some() {
        return Ok(Vec::new());
    }
    let (ops, _) = super::operaciones(v)?;
    let por_plataforma = !super::es_persona_fisica(v);
    Ok(motor::calcular(&ops, Metodo::Pmp, None, por_plataforma).huecos)
}

/// Texto del aviso de operaciones incompletas sin punto de partida, si toca.
pub(crate) fn texto_huecos(h: &[motor::Hueco]) -> Option<String> {
    let primero = h.iter().map(|x| x.fecha).min()?;
    Some(format!(
        "faltan operaciones: {} {} de más unidades de las que había (la primera el {}). Si \
         operaste en exchanges que ya no existen o en DeFi sin registros, pon el punto de \
         partida en Criptomonedas con «Usar los cálculos de la gestoría»: lo anterior queda como \
         historial y el cálculo sigue desde lo declarado",
        h.len(),
        if h.len() == 1 {
            "venta o traspaso sale"
        } else {
            "ventas o traspasos salen"
        },
        dmy(primero)
    ))
}

/// Aviso para la importación de la carpeta: operaciones incompletas y sin punto de partida.
pub fn aviso_sin_inicio(v: &Vault) -> Result<Option<String>> {
    Ok(texto_huecos(&huecos(v)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hoja(nombre: &str, filas: &[&[&str]]) -> Hoja {
        Hoja {
            nombre: nombre.into(),
            filas: filas
                .iter()
                .map(|f| f.iter().map(|c| (*c).to_owned()).collect())
                .collect(),
        }
    }

    #[test]
    fn hoja_de_saldos_con_reparto_del_coste() {
        let control = hoja(
            "Control",
            &[
                &["", "ENTRADAS", "SALIDAS"],
                &["", "50000", "", "Saldo", "50000"],
                &["", "", "", "BTC", "30000.004"],
                &["", "", "", "ETH", "4999.996"],
                &["", "", "", "TOTAL", "35000"],
            ],
        );
        let datos = hoja(
            "Datos",
            &[
                &["Saldo por día"],
                &[
                    "Fecha",
                    "EUR Valor en EUR",
                    "EUR Cantidad",
                    "BTC Valor en EUR",
                    "BTC Cantidad",
                    "ETH Valor en EUR",
                    "ETH Cantidad",
                ],
                &["now", "1", "1", "90000", "1", "3000", "2"],
                &["01.01.2026", "1", "1", "80000", "1", "2500", "2"],
                &["01.12.2025", "1", "1", "85000", "1", "0", "0"],
            ],
        );
        let h = leer_hoja("control.xlsx", &[control, datos]).unwrap();
        assert_eq!(h.fichero, "control.xlsx");
        assert_eq!(h.saldos.len(), 3, "{:?}", h.saldos);
        assert!(h.saldos.iter().all(|s| s.ticker != "EUR"));
        assert_eq!(
            h.costes,
            vec![
                ("BTC".to_owned(), Money::from_euros(30_000, 0)),
                ("ETH".to_owned(), Money::from_euros(5_000, 0)),
            ]
        );
        assert_eq!(h.total, Some(Money::from_euros(35_000, 0)));
    }

    #[test]
    fn sin_total_que_cuadre_no_hay_reparto() {
        let control = hoja(
            "C",
            &[
                &["BTC", "30000"],
                &["ETH", "5000"],
                &["TOTAL", "40000"],
                &[
                    "Fecha",
                    "BTC Valor en EUR",
                    "BTC Cantidad",
                    "ETH Valor en EUR",
                    "ETH Cantidad",
                ],
                &["31.12.2025", "80000", "1", "3000", "1,5"],
            ],
        );
        let h = leer_hoja("c.xlsx", &[control]).unwrap();
        assert!(h.costes.is_empty() && h.total.is_none());
        assert_eq!(h.saldos.len(), 2);
        assert_eq!(h.saldos[1].cantidad, Qty::parse("1.5").unwrap());
    }

    #[test]
    fn cuentas_de_criptomonedas() {
        let asignadas = vec!["2070001".to_owned()];
        assert!(es_cuenta_cripto("2050004", "CRIPTOACTIVOS", &[]));
        assert!(es_cuenta_cripto("2070001", "Otra cosa", &asignadas));
        assert!(es_cuenta_cripto("3090002", "Existencias de bitcoin", &[]));
        assert!(!es_cuenta_cripto(
            "29000001",
            "Deterioro criptomonedas",
            &[]
        ));
        assert!(!es_cuenta_cripto(
            "7690009",
            "Beneficios de criptoactivos",
            &[]
        ));
        assert!(!es_cuenta_cripto(
            "2060000",
            "Aplicaciones informáticas",
            &[]
        ));
    }
}
