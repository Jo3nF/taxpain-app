//! Criptomonedas: activos, plataformas, operaciones, precios de cierre, informe del ejercicio,
//! asientos automáticos, 721 y Renta. El cálculo puro está en `taxpain_core::cripto`.
//!
//! Normativa (en llano; lo que no se ha podido leer en la fuente lleva `TODO(VERIFY)`):
//! - **Sociedad (IS)**: ICAC, BOICAC 120 (diciembre de 2019), **consulta 4** «Emisión de
//!   criptomoneda. NRV 10.ª» (así figura en el índice de consultas de icac.gob.es, comprobado el
//!   2026-10-27; el encargo la citaba como consulta 9 por error): son **existencias** si se tienen
//!   para venderlas en el curso ordinario de la actividad y, si no, **inmovilizado intangible**;
//!   nunca tesorería ni activo financiero. Existencias: NRV 10.ª PGC (precio de adquisición; PMP
//!   o FIFO).
//!   Intangible: NRV 5.ª y 6.ª (coste; vida útil, deterioro). Permuta: NRV 2.ª.1.3. Deterioro:
//!   art. 13.2.a LIS (Ley 27/2014, comprobado en el BOE el 26-10-2026): «No serán deducibles:
//!   a) Las pérdidas por deterioro del inmovilizado material, inversiones inmobiliarias e
//!   inmovilizado intangible»; el de existencias no está en esa lista y sí es deducible. El
//!   resultado contable entra en la base imponible del 200 (art. 10.3 LIS).
//! - **Persona física (IRPF)**: ganancia o pérdida patrimonial de la base del ahorro (arts.
//!   33, 46 y 49 LIRPF), no rendimiento de la actividad salvo trading profesional habitual
//!   (TODO(VERIFY): criterio DGT para considerarlo actividad económica). Permuta: art. 37.1.h
//!   LIRPF (comprobado en el BOE): «la ganancia o pérdida patrimonial se determinará por la
//!   diferencia entre el valor de adquisición del bien o derecho que se cede y el mayor de los
//!   dos siguientes: el valor de mercado del bien o derecho entregado; el valor de mercado del
//!   bien o derecho que se recibe a cambio». Qué unidades se transmiten: art. 37.2 LIRPF («se
//!   considerará que los transmitidos por el contribuyente son aquéllos que adquirió en primer
//!   lugar»; literalmente para valores; la DGT lo aplica a las monedas virtuales por analogía:
//!   V0999-18, V0975-22 y V2520-22, de 7-12-2022, leída en petete el 2026-10-27: «en el caso de
//!   efectuarse ventas parciales de criptomonedas de un mismo tipo que hubieran sido adquiridas
//!   en diferentes momentos, debe considerarse que las criptomonedas que se transmiten son las
//!   adquiridas en primer lugar». La ayuda de Renta Web 2025 (7.6.6.2) añade que vale aunque las
//!   operaciones se hicieran en distintos exchanges: FIFO **global** por moneda. En contra, la
//!   STSJ del País Vasco 37/2025, de 9-1-2025 (rec. 75/2024, ECLI:ES:TSJPV:2025:41): niega el
//!   FIFO con la norma foral de Bizkaia; no afecta al territorio común y Bizkaia y Gipuzkoa
//!   tramitan incluirlo en sus normas forales). Comisiones: art. 35 LIRPF (mayor coste de
//!   adquisición / menor valor de transmisión). Casillas del modelo 100 (anexo I de la Orden
//!   HAC/277/2026, Renta 2025, BOE-A-2026-7041, cotejado el 2026-10-27): apartado «Ganancias y
//!   pérdidas patrimoniales derivadas de la transmisión o permuta de monedas virtuales por
//!   particulares», [1800] titular, [1801] operaciones a plazos, [1802] denominación, [1803]
//!   «Identificación de lo recibido a cambio. Clave tipo de contraprestación», [1804] valor de
//!   transmisión, [1805] destinado a renta vitalicia, [1806] valor de adquisición, [1807]/[1808]
//!   pérdida y pérdida imputable al año, [1809] ganancia, [1810] exenta por reinversión en renta
//!   vitalicia, [1811]/[1812] ganancia no exenta e imputable al año, [1813] suma de pérdidas y
//!   [1814] suma de ganancias (van a las [0423] y [0422]). TODO(VERIFY): las letras de la clave
//!   de la [1803] (F venta a euros, N permuta, B pago de bienes o servicios) no están en la
//!   orden ni en la ayuda (es un desplegable de Renta Web): cotejarlas en Renta Web.
//! - **Modelo 721** (Orden HFP/886/2023, BOE de 29-7-2023; art. 42 quater RGAT, RD 1065/2007,
//!   leído en el BOE el 26-10-2026): saldos a 31 de diciembre en plataformas del extranjero
//!   (custodios que no presentan el 172/173 en España) valorados en euros con «la cotización a
//!   31 de diciembre que ofrezcan las principales plataformas de negociación»; no hay que
//!   declarar si «los saldos a 31 de diciembre valorados en euros no superen, conjuntamente, los
//!   50.000 euros»; plazo «entre el 1 de enero y el 31 de marzo del año siguiente»; años
//!   sucesivos sólo si el saldo conjunto «hubiese experimentado un incremento superior a 20.000
//!   euros». Y una excepción clave del art. 42 quater.5: **no están obligadas** las personas
//!   jurídicas (b) ni las personas físicas con actividad que lleven contabilidad según el Código
//!   de Comercio (c) que tengan las monedas «registradas en su contabilidad de forma
//!   individualizada e identificadas por su denominación, valor y entidad de custodia y país».
//! - **Modelos 172 y 173** (Orden HFP/887/2023): los presentan las plataformas españolas, no el
//!   usuario; sirven para cotejar.

pub mod asientos;
pub mod export;
pub mod importar;
pub mod inicio;
pub mod paquete;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::calendar::{Persona, TaxpayerProfile};
use taxpain_core::cripto::{self, Metodo, Operacion, Precio, Qty, Resumen, SIN_PLATAFORMA, Tipo};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, Month, OffsetDateTime};

use crate::accounting;
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

pub const SETTINGS_KEY: &str = "cripto_settings";
/// Saldo conjunto en el extranjero a 31/12 que obliga al 721 (art. 42 quater.5.d RGAT).
pub const UMBRAL_721: Money = Money::from_euros(50_000, 0);
/// Incremento que obliga a repetir el 721 (art. 42 quater.6 RGAT).
pub const INCREMENTO_721: Money = Money::from_euros(20_000, 0);

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

/// Naturaleza contable en la sociedad (consulta del BOICAC 120/2019).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Naturaleza {
    /// Se tienen para venderlas en el curso ordinario (grupo 3; NRV 10.ª).
    #[default]
    Existencias,
    /// Inversión o uso propio (grupo 20; NRV 5.ª y 6.ª).
    Intangible,
}

impl Naturaleza {
    pub fn label(self) -> &'static str {
        match self {
            Naturaleza::Existencias => "existencias",
            Naturaleza::Intangible => "inmovilizado intangible",
        }
    }
}

/// Cuentas por defecto de los asientos (configurables).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cuentas {
    /// Base de las subcuentas de existencias por activo (3090001, 3090002…).
    pub existencias: String,
    /// Base de las subcuentas de intangible por activo. El PGC no tiene cuenta para
    /// criptomonedas y la 209 ya es «Anticipos para inmovilizaciones intangibles» (cuadro de
    /// cuentas del PGC y del PGC PYMES), así que por defecto se usa la 207, libre en el
    /// subgrupo 20 (configurable).
    pub intangible: String,
    /// Contrapartida de los euros (banco o saldo en euros de la plataforma).
    pub tesoreria: String,
    /// Cuenta puente de los pagos y cobros en cripto (se salda contra el proveedor o cliente).
    pub puente: String,
    /// Existencias: ingreso por la transmisión (700) y salida del coste (610).
    pub ventas: String,
    pub variacion: String,
    /// Intangible: beneficio (771) o pérdida (671) por la transmisión.
    pub beneficio: String,
    pub perdida: String,
    pub comisiones: String,
    pub recompensas: String,
    /// Deterioro al cierre: gasto y corrección (existencias 693/390; intangible 690/290) y su
    /// reversión (793/790).
    pub deterioro_gasto: String,
    pub deterioro: String,
    pub reversion: String,
}

impl Default for Cuentas {
    fn default() -> Self {
        Cuentas {
            existencias: "309".into(),
            intangible: "207".into(),
            tesoreria: "5720001".into(),
            puente: "5550001".into(),
            ventas: "700".into(),
            variacion: "610".into(),
            beneficio: "771".into(),
            perdida: "671".into(),
            comisiones: "626".into(),
            recompensas: "769".into(),
            deterioro_gasto: "693".into(),
            deterioro: "390".into(),
            reversion: "793".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub metodo: Metodo,
    #[serde(default)]
    pub naturaleza: Naturaleza,
    #[serde(default)]
    pub cuentas: Cuentas,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            metodo: Metodo::Pmp,
            naturaleza: Naturaleza::Existencias,
            cuentas: Cuentas::default(),
        }
    }
}

pub fn settings(v: &Vault) -> Result<Settings> {
    Ok(v.setting(SETTINGS_KEY)?.unwrap_or_default())
}

pub fn save_settings(v: &Vault, s: &Settings, now: OffsetDateTime) -> Result<()> {
    for c in [
        &s.cuentas.existencias,
        &s.cuentas.intangible,
        &s.cuentas.tesoreria,
        &s.cuentas.puente,
        &s.cuentas.ventas,
        &s.cuentas.variacion,
        &s.cuentas.beneficio,
        &s.cuentas.perdida,
        &s.cuentas.comisiones,
        &s.cuentas.recompensas,
        &s.cuentas.deterioro_gasto,
        &s.cuentas.deterioro,
        &s.cuentas.reversion,
    ] {
        taxpain_core::accounting::AccountCode::new(c)
            .map_err(|_| other(format!("cuenta no válida: «{c}»")))?;
    }
    v.set_setting(SETTINGS_KEY, s)?;
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        "cripto.ajustes",
        "",
        &serde_json::json!({ "metodo": s.metodo, "naturaleza": s.naturaleza }),
    )?;
    Ok(())
}

/// Ajustes con las cuentas de deterioro coherentes con la naturaleza (si el usuario cambió de
/// naturaleza sin tocar las cuentas).
pub fn cuentas_deterioro(s: &Settings) -> (String, String, String) {
    match s.naturaleza {
        Naturaleza::Existencias if s.cuentas.deterioro == "290" => {
            ("693".into(), "390".into(), "793".into())
        }
        Naturaleza::Intangible if s.cuentas.deterioro == "390" => {
            ("690".into(), "290".into(), "790".into())
        }
        _ => (
            s.cuentas.deterioro_gasto.clone(),
            s.cuentas.deterioro.clone(),
            s.cuentas.reversion.clone(),
        ),
    }
}

// ---------------- Activos y plataformas ----------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub id: i64,
    pub ticker: String,
    pub name: String,
    /// Subcuenta asignada al contabilizar (vacía hasta entonces).
    #[serde(default)]
    pub account: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Platform {
    pub id: i64,
    pub name: String,
    /// `exchange` o `monedero`.
    pub kind: String,
    /// ISO 3166-1 alfa-2.
    pub country: String,
    /// Custodio que no informa a la AEAT (172/173): cuenta para el 721.
    pub abroad: bool,
    /// Contrapartida de los euros en esa plataforma (vacía = la de los ajustes).
    #[serde(default)]
    pub account: String,
}

pub fn ticker_norm(t: &str) -> String {
    t.trim().to_uppercase()
}

pub fn assets(v: &Vault) -> Result<Vec<Asset>> {
    let mut st = v
        .connection()
        .prepare("SELECT id, ticker, name, account FROM crypto_asset ORDER BY ticker")?;
    let rows = st.query_map([], |r| {
        Ok(Asset {
            id: r.get(0)?,
            ticker: r.get(1)?,
            name: r.get(2)?,
            account: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Crea el activo si no existe (nombre = ticker) y devuelve su id.
fn ensure_asset(conn: &Connection, ticker: &str, name: Option<&str>, now: &str) -> Result<i64> {
    let t = ticker_norm(ticker);
    if t.is_empty() || t.len() > 16 || !t.chars().all(|c| c.is_ascii_alphanumeric() || c == '.') {
        return Err(other(format!("ticker no válido: «{ticker}»")));
    }
    if let Some(id) = conn
        .query_row("SELECT id FROM crypto_asset WHERE ticker = ?1", [&t], |r| {
            r.get::<_, i64>(0)
        })
        .optional()?
    {
        if let Some(n) = name.filter(|n| !n.trim().is_empty()) {
            conn.execute(
                "UPDATE crypto_asset SET name = ?1 WHERE id = ?2",
                params![n.trim(), id],
            )?;
        }
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO crypto_asset(ticker, name, account, created_at) VALUES (?1, ?2, '', ?3)",
        params![
            t,
            name.map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&t),
            now
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn save_asset(v: &Vault, ticker: &str, name: &str, now: OffsetDateTime) -> Result<i64> {
    ensure_asset(v.connection(), ticker, Some(name), &now_str(now))
}

pub fn platforms(v: &Vault) -> Result<Vec<Platform>> {
    let mut st = v.connection().prepare(
        "SELECT id, name, kind, country, abroad, account FROM crypto_platform ORDER BY name",
    )?;
    let rows = st.query_map([], |r| {
        Ok(Platform {
            id: r.get(0)?,
            name: r.get(1)?,
            kind: r.get(2)?,
            country: r.get(3)?,
            abroad: r.get::<_, i64>(4)? == 1,
            account: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn check_platform(p: &Platform) -> Result<()> {
    if p.name.trim().is_empty() {
        return Err(other("la plataforma necesita un nombre"));
    }
    if !matches!(p.kind.as_str(), "exchange" | "monedero") {
        return Err(other("tipo de plataforma: exchange o monedero"));
    }
    if p.country.len() != 2 || !p.country.chars().all(|c| c.is_ascii_alphabetic()) {
        return Err(other("país: código de dos letras (ES, DE, US…)"));
    }
    if !p.account.is_empty() {
        taxpain_core::accounting::AccountCode::new(&p.account)
            .map_err(|_| other(format!("cuenta no válida: «{}»", p.account)))?;
    }
    Ok(())
}

/// Alta (`id` 0) o cambio de una plataforma. Devuelve su id.
pub fn save_platform(v: &Vault, p: &Platform, now: OffsetDateTime) -> Result<i64> {
    check_platform(p)?;
    let conn = v.connection();
    let country = p.country.to_uppercase();
    if p.id == 0 {
        conn.execute(
            "INSERT INTO crypto_platform(name, kind, country, abroad, account, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                p.name.trim(),
                p.kind,
                country,
                i64::from(p.abroad),
                p.account,
                now_str(now)
            ],
        )?;
        Ok(conn.last_insert_rowid())
    } else {
        let n = conn.execute(
            "UPDATE crypto_platform SET name = ?1, kind = ?2, country = ?3, abroad = ?4, account = ?5
             WHERE id = ?6",
            params![
                p.name.trim(),
                p.kind,
                country,
                i64::from(p.abroad),
                p.account,
                p.id
            ],
        )?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        Ok(p.id)
    }
}

/// Plataforma por nombre (sin distinguir mayúsculas); la crea si no existe. Las importaciones
/// la marcan como del extranjero cuando el nombre es de un exchange conocido fuera de España.
pub(crate) fn ensure_platform(conn: &Connection, name: &str, now: &str) -> Result<i64> {
    let n = name.trim();
    if n.is_empty() {
        return Err(other("falta la plataforma"));
    }
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM crypto_platform WHERE lower(name) = lower(?1)",
            [n],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    let (country, abroad) = importar::pais_de_plataforma(n);
    conn.execute(
        "INSERT INTO crypto_platform(name, kind, country, abroad, account, created_at)
         VALUES (?1, 'exchange', ?2, ?3, '', ?4)",
        params![n, country, i64::from(abroad), now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn delete_platform(v: &Vault, id: i64) -> Result<()> {
    let conn = v.connection();
    let used: i64 = conn.query_row(
        "SELECT COUNT(*) FROM crypto_op WHERE platform_id = ?1 OR to_platform_id = ?1",
        [id],
        |r| r.get(0),
    )?;
    if used > 0 {
        return Err(other(format!(
            "esa plataforma tiene {used} operaciones: bórralas o cámbialas antes"
        )));
    }
    let en_inicio: i64 = conn.query_row(
        "SELECT COUNT(*) FROM crypto_opening_position WHERE platform_id = ?1",
        [id],
        |r| r.get(0),
    )?;
    if en_inicio > 0 {
        return Err(other(
            "esa plataforma está en el punto de partida: quítasela allí antes",
        ));
    }
    if conn.execute("DELETE FROM crypto_platform WHERE id = ?1", [id])? == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

// ---------------- Operaciones ----------------

/// Una operación tal como se guarda y se edita.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Op {
    #[serde(default)]
    pub id: i64,
    pub date: Date,
    /// `HH:MM:SS`.
    #[serde(default = "medianoche")]
    pub time: String,
    pub kind: Tipo,
    pub platform_id: i64,
    #[serde(default)]
    pub to_platform_id: Option<i64>,
    pub ticker: String,
    pub qty: Qty,
    #[serde(default)]
    pub ticker2: Option<String>,
    #[serde(default)]
    pub qty2: Option<Qty>,
    /// Contravalor en euros sin la comisión.
    #[serde(default)]
    pub eur: Money,
    #[serde(default)]
    pub fee_eur: Money,
    #[serde(default)]
    pub ref_price: Option<Precio>,
    #[serde(default)]
    pub note: String,
    #[serde(default = "manual")]
    pub source: String,
    #[serde(default)]
    pub fingerprint: Option<String>,
    #[serde(default)]
    pub entry_id: Option<i64>,
}

fn medianoche() -> String {
    "00:00:00".into()
}

fn manual() -> String {
    "manual".into()
}

fn check_op(op: &Op) -> Result<()> {
    if op.qty.is_negative() || op.qty.is_zero() {
        return Err(other("la cantidad ha de ser mayor que cero"));
    }
    if op.eur.is_negative() || op.fee_eur.is_negative() {
        return Err(other("los euros y la comisión no pueden ser negativos"));
    }
    if op.kind.transmite() && op.kind != Tipo::Perdida && op.fee_eur > op.eur && !op.eur.is_zero() {
        return Err(other("la comisión no puede ser mayor que el importe"));
    }
    if op.time.len() != 8 || op.time.split(':').count() != 3 {
        return Err(other("hora en formato HH:MM:SS"));
    }
    match op.kind {
        Tipo::Permuta => {
            if op
                .ticker2
                .as_deref()
                .map(ticker_norm)
                .unwrap_or_default()
                .is_empty()
                || op.qty2.is_none_or(|q| q.is_zero() || q.is_negative())
            {
                return Err(other(
                    "la permuta necesita el activo y la cantidad que se recibe",
                ));
            }
            if op.ticker2.as_deref().map(ticker_norm) == Some(ticker_norm(&op.ticker)) {
                return Err(other("en una permuta el activo recibido es otro"));
            }
        }
        Tipo::Traspaso if op.to_platform_id.is_none_or(|d| d == op.platform_id) => {
            return Err(other(
                "el traspaso necesita una plataforma de destino distinta de la de origen",
            ));
        }
        _ => {}
    }
    if op.date.year() < 2009 {
        return Err(other(
            "revisa la fecha: no había criptomonedas antes de 2009",
        ));
    }
    Ok(())
}

fn row_op(r: &taxpain_vault::rusqlite::Row<'_>) -> taxpain_vault::rusqlite::Result<Op> {
    let date: String = r.get(1)?;
    let kind: String = r.get(3)?;
    let qty: String = r.get(7)?;
    let qty2: Option<String> = r.get(9)?;
    let ref_price: Option<String> = r.get(12)?;
    Ok(Op {
        id: r.get(0)?,
        date: Date::parse(&date, &time::format_description::well_known::Iso8601::DATE)
            .unwrap_or(Date::MIN),
        time: r.get(2)?,
        kind: Tipo::parse(&kind).unwrap_or(Tipo::Compra),
        platform_id: r.get(4)?,
        to_platform_id: r.get(5)?,
        ticker: r.get(6)?,
        qty: Qty::parse(&qty).unwrap_or(Qty::ZERO),
        ticker2: r.get(8)?,
        qty2: qty2.as_deref().and_then(Qty::parse),
        eur: Money::from_cents(r.get(10)?),
        fee_eur: Money::from_cents(r.get(11)?),
        ref_price: ref_price.as_deref().and_then(Precio::parse),
        note: r.get(13)?,
        source: r.get(14)?,
        fingerprint: r.get(15)?,
        entry_id: r.get(16)?,
    })
}

const OP_COLS: &str =
    "id, date, time, kind, platform_id, to_platform_id, ticker, qty, ticker2, qty2,
    eur, fee_eur, ref_price, note, source, fingerprint, entry_id";

/// Todas las operaciones (ordenadas por fecha y hora) o sólo las del ejercicio `year` (el de la
/// contabilidad, aunque no sea el año natural).
pub fn ops(v: &Vault, year: Option<i32>) -> Result<Vec<Op>> {
    let conn = v.connection();
    let sql = format!("SELECT {OP_COLS} FROM crypto_op ORDER BY date, time, id");
    let mut st = conn.prepare(&sql)?;
    let rows = st.query_map([], row_op)?;
    let all: Vec<Op> = rows.collect::<std::result::Result<_, _>>()?;
    Ok(match year {
        Some(y) => {
            let (desde, hasta) = ejercicio(v, y);
            all.into_iter()
                .filter(|o| (desde..=hasta).contains(&o.date))
                .collect()
        }
        None => all,
    })
}

fn op_by_id(conn: &Connection, id: i64) -> Result<Op> {
    let sql = format!("SELECT {OP_COLS} FROM crypto_op WHERE id = ?1");
    conn.query_row(&sql, [id], row_op)
        .optional()?
        .ok_or(AppError::NotFound)
}

/// Ejercicio cerrado de una fecha (no se tocan sus asientos ni sus operaciones contabilizadas).
pub(crate) fn ejercicio_cerrado(conn: &Connection, date: Date) -> Result<bool> {
    let fy = accounting::fiscal_year_for_date(conn, date)?;
    let closed: Option<i64> = conn
        .query_row(
            "SELECT closed FROM fiscal_year WHERE year = ?1",
            [fy.year],
            |r| r.get(0),
        )
        .optional()?;
    Ok(closed == Some(1))
}

fn insert_op(conn: &Connection, op: &Op, now: &str) -> Result<i64> {
    ensure_asset(conn, &op.ticker, None, now)?;
    if let Some(t2) = &op.ticker2 {
        ensure_asset(conn, t2, None, now)?;
    }
    conn.execute(
        "INSERT INTO crypto_op(date, time, kind, platform_id, to_platform_id, ticker, qty, ticker2,
            qty2, eur, fee_eur, ref_price, note, source, fingerprint, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            op.date.to_string(),
            op.time,
            op.kind.code(),
            op.platform_id,
            op.to_platform_id,
            ticker_norm(&op.ticker),
            op.qty.to_text(),
            op.ticker2.as_deref().map(ticker_norm),
            op.qty2.map(Qty::to_text),
            op.eur.cents(),
            op.fee_eur.cents(),
            op.ref_price.map(Precio::to_text),
            op.note.trim(),
            op.source,
            op.fingerprint,
            now,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Alta (`id` 0) o cambio de una operación. Al cambiar una ya contabilizada se borra su asiento
/// (se vuelve a generar desde «Contabilizar»); si su ejercicio está cerrado no se toca.
pub fn save_op(v: &Vault, op: &Op, now: OffsetDateTime) -> Result<i64> {
    check_op(op)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let ts = now_str(now);
    let id = if op.id == 0 {
        insert_op(&tx, op, &ts)?
    } else {
        let old = op_by_id(&tx, op.id)?;
        if let Some(e) = old.entry_id {
            if ejercicio_cerrado(&tx, old.date)? {
                return Err(other(
                    "esa operación está contabilizada en un ejercicio cerrado: reábrelo antes",
                ));
            }
            asientos::borrar_asiento(&tx, e)?;
        }
        ensure_asset(&tx, &op.ticker, None, &ts)?;
        if let Some(t2) = &op.ticker2 {
            ensure_asset(&tx, t2, None, &ts)?;
        }
        tx.execute(
            "UPDATE crypto_op SET date = ?1, time = ?2, kind = ?3, platform_id = ?4,
                to_platform_id = ?5, ticker = ?6, qty = ?7, ticker2 = ?8, qty2 = ?9, eur = ?10,
                fee_eur = ?11, ref_price = ?12, note = ?13, entry_id = NULL
             WHERE id = ?14",
            params![
                op.date.to_string(),
                op.time,
                op.kind.code(),
                op.platform_id,
                op.to_platform_id,
                ticker_norm(&op.ticker),
                op.qty.to_text(),
                op.ticker2.as_deref().map(ticker_norm),
                op.qty2.map(Qty::to_text),
                op.eur.cents(),
                op.fee_eur.cents(),
                op.ref_price.map(Precio::to_text),
                op.note.trim(),
                op.id,
            ],
        )?;
        op.id
    };
    audit::append(
        &tx,
        &ts,
        Actor::User,
        if op.id == 0 {
            "cripto.alta"
        } else {
            "cripto.cambio"
        },
        &id.to_string(),
        &serde_json::json!({ "tipo": op.kind.code(), "ticker": ticker_norm(&op.ticker), "fecha": op.date.to_string() }),
    )?;
    tx.commit()?;
    Ok(id)
}

pub fn delete_op(v: &Vault, id: i64, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let old = op_by_id(&tx, id)?;
    if let Some(e) = old.entry_id {
        if ejercicio_cerrado(&tx, old.date)? {
            return Err(other(
                "esa operación está contabilizada en un ejercicio cerrado: reábrelo antes",
            ));
        }
        asientos::borrar_asiento(&tx, e)?;
    }
    tx.execute("DELETE FROM crypto_op WHERE id = ?1", [id])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "cripto.baja",
        &id.to_string(),
        &serde_json::json!({ "tipo": old.kind.code(), "ticker": old.ticker }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Qué pasa con una operación leída cuya huella ya está guardada.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Reimportada {
    /// La misma (o con cambios que no se tocan): se salta.
    Igual,
    /// Importada antes del arreglo de las comisiones en la misma cripto (CoinTracking y Kraken):
    /// la guardada tiene otra cantidad. No está contabilizada ni en un ejercicio cerrado, así que
    /// se corrigen las cantidades con las de la lectura nueva. Los euros (`eur`, `fee_eur`) son
    /// los que hay que guardar: los de la lectura nueva, salvo que la operación se haya editado a
    /// mano o la lectura no traiga valor (las permutas de Kraken), que se quedan los guardados.
    /// Lleva el aviso.
    Corregir {
        id: i64,
        eur: Money,
        fee_eur: Money,
        aviso: String,
    },
    /// Con otra cantidad, pero contabilizada o con un ejercicio cerrado de por medio: no se
    /// toca y el aviso dice qué hacer.
    NoSeToca { aviso: String },
}

/// Compara una operación leída con la guardada con su misma huella (`None` si no hay ninguna).
///
/// Las compras, ventas y permutas de CoinTracking y Kraken importadas antes de descontar la
/// comisión en la misma cripto tienen la cantidad bruta y la misma huella, así que se saltaban
/// como duplicadas y el saldo quedaba inflado. Sólo se corrige si coinciden fecha, tipo y
/// activos y cambia la cantidad; lo contabilizado o de un ejercicio cerrado (el suyo o uno
/// posterior, cuyas ventas dependen de este coste) no se toca: se avisa para borrarla y volver
/// a importar. En Kraken se admite un día de diferencia en la fecha: antes se guardaba la del
/// libro (UTC) y ahora la de Madrid, así que lo de cerca de medianoche cambia de día.
///
/// Sólo lee: la vista previa la usa sin crear ejercicios.
pub(crate) fn reimportada(conn: &Connection, op: &Op) -> Result<Option<Reimportada>> {
    let Some(f) = &op.fingerprint else {
        return Ok(None);
    };
    let sql = format!("SELECT {OP_COLS} FROM crypto_op WHERE fingerprint = ?1");
    let Some(old) = conn.query_row(&sql, [f], row_op).optional()? else {
        return Ok(None);
    };
    let de_exchange = matches!(op.source.as_str(), "cointracking" | "kraken");
    let con_cantidades = matches!(op.kind, Tipo::Compra | Tipo::Venta | Tipo::Permuta);
    // Kraken: la fecha guardada puede ser la de UTC y la nueva la de Madrid.
    let dias = (old.date - op.date).whole_days().abs();
    let misma_fecha = dias == 0 || (op.source == "kraken" && dias == 1);
    let mismos = old.kind == op.kind
        && misma_fecha
        && old.source == op.source
        && old.ticker == ticker_norm(&op.ticker)
        && old.ticker2 == op.ticker2.as_deref().map(ticker_norm);
    let otra_cantidad = old.qty != op.qty || old.qty2 != op.qty2;
    if !(de_exchange && con_cantidades && mismos && otra_cantidad) {
        return Ok(Some(Reimportada::Igual));
    }
    let que = format!(
        "{} de {} del {}: está guardada con {}{} y la lectura nueva da {}{} (la comisión en la \
         misma cripto no se descontaba)",
        op.kind.label().to_lowercase(),
        old.ticker,
        inicio::dmy(old.date),
        old.qty,
        old.qty2.map(|q| format!(" → {q}")).unwrap_or_default(),
        op.qty,
        op.qty2.map(|q| format!(" → {q}")).unwrap_or_default(),
    );
    // Un ejercicio cerrado que acaba ese día o después (el suyo o uno posterior), sin crear
    // ninguno: esto corre también en la vista previa.
    let cerrado: Option<i64> = conn
        .query_row(
            "SELECT year FROM fiscal_year WHERE closed = 1 AND end_date >= ?1
             ORDER BY end_date LIMIT 1",
            [old.date.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(y) = cerrado {
        return Ok(Some(Reimportada::NoSeToca {
            aviso: format!(
                "{que}. No se corrige porque el ejercicio {y} está cerrado: reábrelo, borra esa \
                 operación y vuelve a importar el fichero"
            ),
        }));
    }
    if old.entry_id.is_some() {
        return Ok(Some(Reimportada::NoSeToca {
            aviso: format!(
                "{que}. No se corrige porque ya está contabilizada: borra esa operación (se \
                 borra su asiento), vuelve a importar el fichero y contabiliza otra vez"
            ),
        }));
    }
    // Los euros puestos a mano no se pierden: si se editó la operación, o si la lectura no
    // trae valor (las permutas de Kraken van sin él), se quedan los guardados.
    let editada: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM audit_log WHERE action = 'cripto.cambio' AND subject = ?1)",
        [old.id.to_string()],
        |r| r.get(0),
    )?;
    let eur = if editada || op.eur.is_zero() {
        old.eur
    } else {
        op.eur
    };
    let fee_eur = if editada || op.fee_eur.is_zero() {
        old.fee_eur
    } else {
        op.fee_eur
    };
    let euros = if (eur, fee_eur) != (op.eur, op.fee_eur) && !(eur.is_zero() && fee_eur.is_zero()) {
        format!(
            " (sólo las cantidades: se conserva el valor guardado de {eur}{})",
            if fee_eur.is_zero() {
                String::new()
            } else {
                format!(" y la comisión de {fee_eur}")
            }
        )
    } else {
        String::new()
    };
    Ok(Some(Reimportada::Corregir {
        id: old.id,
        eur,
        fee_eur,
        aviso: format!(
            "{que}. Se corrige al importar{euros}; si ya habías contabilizado ventas posteriores \
             de {}, contabilízalas otra vez",
            old.ticker
        ),
    }))
}

/// Importa operaciones ya leídas (vista previa confirmada). Las que tengan una huella ya
/// guardada se saltan, salvo las importadas con la cantidad bruta antes de descontar la
/// comisión en la misma cripto, que se corrigen si es seguro (ver [`reimportada`]); cuentan
/// como duplicadas. Devuelve (importadas, duplicadas).
pub fn import_ops(v: &Vault, ops: &[Op], now: OffsetDateTime) -> Result<(usize, usize)> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let ts = now_str(now);
    let (mut nuevas, mut dup, mut corregidas) = (0, 0, 0);
    for op in ops {
        check_op(op)?;
        match reimportada(&tx, op)? {
            None => {}
            Some(Reimportada::Corregir {
                id, eur, fee_eur, ..
            }) => {
                tx.execute(
                    "UPDATE crypto_op SET qty = ?1, qty2 = ?2, eur = ?3, fee_eur = ?4
                     WHERE id = ?5 AND entry_id IS NULL",
                    params![
                        op.qty.to_text(),
                        op.qty2.map(Qty::to_text),
                        eur.cents(),
                        fee_eur.cents(),
                        id,
                    ],
                )?;
                corregidas += 1;
                dup += 1;
                continue;
            }
            Some(_) => {
                dup += 1;
                continue;
            }
        }
        insert_op(&tx, op, &ts)?;
        nuevas += 1;
    }
    audit::append(
        &tx,
        &ts,
        Actor::User,
        "cripto.importar",
        "",
        &serde_json::json!({ "nuevas": nuevas, "duplicadas": dup, "corregidas": corregidas }),
    )?;
    tx.commit()?;
    Ok((nuevas, dup))
}

// ---------------- Precios de cierre ----------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Price {
    pub date: Date,
    pub ticker: String,
    pub price: Precio,
    #[serde(default = "manual")]
    pub source: String,
}

pub fn prices(v: &Vault) -> Result<Vec<Price>> {
    let mut st = v.connection().prepare(
        "SELECT date, ticker, price, source FROM crypto_price ORDER BY date DESC, ticker",
    )?;
    let rows = st.query_map([], |r| {
        let d: String = r.get(0)?;
        let p: String = r.get(2)?;
        Ok(Price {
            date: Date::parse(&d, &time::format_description::well_known::Iso8601::DATE)
                .unwrap_or(Date::MIN),
            ticker: r.get(1)?,
            price: Precio::parse(&p).unwrap_or(Precio::ZERO),
            source: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn set_prices(v: &Vault, ps: &[Price], now: OffsetDateTime) -> Result<usize> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let ts = now_str(now);
    let mut n = 0;
    for p in ps {
        if p.price.is_zero() || p.price < Precio::ZERO {
            return Err(other(format!(
                "precio de {} el {}: ha de ser mayor que cero",
                p.ticker, p.date
            )));
        }
        ensure_asset(&tx, &p.ticker, None, &ts)?;
        tx.execute(
            "INSERT INTO crypto_price(date, ticker, price, source, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(date, ticker) DO UPDATE SET price = excluded.price, source = excluded.source,
             updated_at = excluded.updated_at",
            params![
                p.date.to_string(),
                ticker_norm(&p.ticker),
                p.price.to_text(),
                p.source,
                ts
            ],
        )?;
        n += 1;
    }
    tx.commit()?;
    Ok(n)
}

pub fn delete_price(v: &Vault, date: Date, ticker: &str) -> Result<()> {
    v.connection().execute(
        "DELETE FROM crypto_price WHERE date = ?1 AND ticker = ?2",
        params![date.to_string(), ticker_norm(ticker)],
    )?;
    Ok(())
}

/// Precio de cierre de `ticker` a `date`: el de ese día o, si no, el último anterior (con su
/// fecha). También sirve el precio de referencia de una compra o venta de ese día.
pub fn precio_a(v: &Vault, ticker: &str, date: Date) -> Result<Option<(Date, Precio)>> {
    let row: Option<(String, String)> = v
        .connection()
        .query_row(
            "SELECT date, price FROM crypto_price WHERE ticker = ?1 AND date <= ?2
             ORDER BY date DESC LIMIT 1",
            params![ticker_norm(ticker), date.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(row.and_then(|(d, p)| {
        Some((
            Date::parse(&d, &time::format_description::well_known::Iso8601::DATE).ok()?,
            Precio::parse(&p)?,
        ))
    }))
}

// ---------------- Cálculo ----------------

fn a_operacion(o: &Op) -> Operacion {
    Operacion {
        id: o.id,
        fecha: o.date,
        hora: o.time.clone(),
        tipo: o.kind,
        plataforma: o.platform_id,
        destino: o.to_platform_id,
        ticker: ticker_norm(&o.ticker),
        cantidad: o.qty,
        ticker2: o.ticker2.as_deref().map(ticker_norm),
        cantidad2: o.qty2,
        euros: o.eur,
        comision: o.fee_eur,
    }
}

/// Las operaciones del almacén para el motor. Las permutas, pagos y comisiones sin contravalor
/// lo toman del precio de cierre del día (o del precio de referencia) si lo hay. Las de hasta
/// la fecha de corte del punto de partida van también (el motor no las cuenta), sin avisos.
pub(crate) fn operaciones(v: &Vault) -> Result<(Vec<Operacion>, Vec<String>)> {
    let corte = inicio::fecha_corte(v.connection())?;
    let mut avisos = Vec::new();
    let mut out = Vec::new();
    let ops_guardadas = ops(v, None)?;
    for o in &ops_guardadas {
        let mut op = a_operacion(o);
        if corte.is_some_and(|c| o.date <= c) {
            out.push(op);
            continue;
        }
        if op.euros.is_zero()
            && matches!(
                op.tipo,
                Tipo::Permuta | Tipo::Pago | Tipo::Comision | Tipo::Recompensa
            )
        {
            let precio = match o.ref_price {
                Some(p) if !p.is_zero() => Some(p),
                _ => precio_a(v, &op.ticker, o.date)?
                    .filter(|(d, _)| *d == o.date)
                    .map(|(_, p)| p),
            };
            match precio.and_then(|p| op.cantidad.valor(p)) {
                Some(valor) => op.euros = valor,
                None => avisos.push(format!(
                    "{} de {} {} del {}: sin valor en euros ni precio de ese día; añade el \
                     precio en «Precios de cierre» o edita la operación.",
                    op.tipo.label(),
                    op.cantidad,
                    op.ticker,
                    op.fecha
                )),
            }
        }
        out.push(op);
    }
    vincular_comisiones(&ops_guardadas, &mut out);
    Ok((out, avisos))
}

/// Huella de la compra o la venta a la que pertenece una comisión pagada en otra cripto (la
/// importación le pone la huella de la operación más «:fee»).
fn huella_principal(o: &Op) -> Option<&str> {
    if o.kind != Tipo::Comision {
        return None;
    }
    o.fingerprint.as_deref()?.strip_suffix(":fee")
}

/// Las comisiones de una compra o una venta pagadas en una tercera cripto (BNB, p. ej.): su
/// valor en euros se suma al coste de la compra o se resta del valor de la venta (art. 35.1.b y
/// 35.2 LIRPF; NRV 10.ª PGC), y la entrega de esas unidades es una transmisión sin gasto aparte
/// (si no, la comisión se contaría dos veces). Devuelve, por id de la comisión, la operación
/// principal.
pub(crate) fn comisiones_vinculadas(guardadas: &[Op]) -> BTreeMap<i64, Op> {
    let principales: BTreeMap<&str, &Op> = guardadas
        .iter()
        .filter(|o| matches!(o.kind, Tipo::Compra | Tipo::Venta))
        .filter_map(|o| o.fingerprint.as_deref().map(|f| (f, o)))
        .collect();
    guardadas
        .iter()
        .filter_map(|c| {
            let p = principales.get(huella_principal(c)?)?;
            (p.date == c.date).then(|| (c.id, (*p).clone()))
        })
        .collect()
}

fn vincular_comisiones(guardadas: &[Op], out: &mut [Operacion]) {
    let vinculos = comisiones_vinculadas(guardadas);
    for (comision, principal) in vinculos {
        let Some(valor) = out.iter_mut().find(|o| o.id == comision).map(|c| {
            c.tipo = Tipo::Pago;
            c.euros
        }) else {
            continue;
        };
        if let Some(p) = out.iter_mut().find(|o| o.id == principal.id) {
            p.comision += valor;
        }
    }
}

/// Inicio y fin del ejercicio `year` (el año natural si no hay ejercicio contable).
pub(crate) fn ejercicio(v: &Vault, year: i32) -> (Date, Date) {
    crate::valores::informe::ejercicio(v, year)
}

/// Último día del ejercicio `year` (31/12 si no hay ejercicio contable).
pub(crate) fn fin_ejercicio(v: &Vault, year: i32) -> Date {
    accounting::fiscal_year(v, year)
        .map(|f| f.end)
        .unwrap_or_else(|_| {
            Date::from_calendar_date(year, Month::December, 31).unwrap_or(Date::MIN)
        })
}

/// Persona física según el perfil fiscal (o el NIF del titular si no hay perfil).
pub(crate) fn es_persona_fisica(v: &Vault) -> bool {
    if let Ok(Some(p)) = v.setting::<TaxpayerProfile>("taxpayer_profile") {
        return p.persona == Persona::Fisica;
    }
    crate::models::declarant(v)
        .ok()
        .and_then(|d| taxpain_core::nif::Nif::parse(&d.nif).ok())
        .is_some_and(|n| n.is_persona_fisica())
}

// ---------------- Informe ----------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PosicionView {
    pub ticker: String,
    pub platform_id: i64,
    pub platform: String,
    pub abroad: bool,
    pub cantidad: Qty,
    pub coste: Money,
    /// Precio de cierre usado (y su fecha si es anterior al cierre).
    pub precio: Option<Precio>,
    pub precio_fecha: Option<Date>,
    pub valor: Option<Money>,
    pub latente: Option<Money>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RealizadoView {
    pub op: i64,
    pub fecha: Date,
    pub tipo: Tipo,
    pub ticker: String,
    pub platform: String,
    pub cantidad: Qty,
    pub valor: Money,
    pub coste: Money,
    pub resultado: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PlataformaExtranjero {
    pub platform: String,
    pub country: String,
    pub valor: Money,
    pub activos: Vec<(String, Qty, Money)>,
}

/// Modelo 721 del ejercicio.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct M721 {
    pub year: i32,
    pub fecha: Date,
    pub total_extranjero: Money,
    pub umbral: Money,
    pub supera: bool,
    pub plataformas: Vec<PlataformaExtranjero>,
    /// Activos en el extranjero sin precio de cierre (la valoración está incompleta).
    pub sin_precio: Vec<String>,
    /// Monedas del punto de partida sin plataforma: no se sabe si están en el extranjero y no
    /// cuentan en el total hasta que se les ponga.
    pub sin_plataforma: Vec<String>,
    /// Marcado en el perfil fiscal.
    pub en_perfil: bool,
    /// Sociedad o persona física con contabilidad: exenta si las tiene contabilizadas
    /// (art. 42 quater.5.b/c RGAT).
    pub exenta_por_contabilidad: bool,
    pub nota: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Report {
    pub year: i32,
    pub fecha: Date,
    pub settings: Settings,
    pub persona_fisica: bool,
    /// Método con el que se contabiliza y declara (en IRPF siempre FIFO).
    pub metodo_declaracion: Metodo,
    pub posiciones: Vec<PosicionView>,
    pub valor_total: Money,
    pub coste_total: Money,
    pub resumen_pmp: Resumen,
    pub resumen_fifo: Resumen,
    pub realizados: Vec<RealizadoView>,
    pub m721: M721,
    pub ops_ejercicio: usize,
    pub ops_sin_asiento: usize,
    pub ops_con_asiento: usize,
    pub deterioro_registrado: Option<i64>,
    pub avisos: Vec<String>,
    /// Punto de partida (posiciones y coste a la fecha de corte), si lo hay.
    pub inicio: Option<inicio::Inicio>,
    /// El ejercicio acaba en la fecha de corte o antes: sus resultados los calculó la gestoría.
    pub antes_del_inicio: bool,
    /// Operaciones del ejercicio hasta la fecha de corte (historial: se ven, no cuentan).
    pub ops_historial: usize,
    /// Sin punto de partida: ventas o traspasos de más unidades de las que había.
    pub huecos: usize,
}

fn nombre_plataforma(ps: &[Platform], id: i64) -> String {
    if id == SIN_PLATAFORMA {
        return "Sin plataforma".into();
    }
    ps.iter()
        .find(|p| p.id == id)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| format!("plataforma {id}"))
}

/// Punto de partida para el motor, si lo hay.
pub(crate) fn inicio_motor(v: &Vault) -> Result<Option<cripto::Inicio>> {
    Ok(inicio::inicio(v)?.map(|i| i.motor()))
}

/// Posiciones a `fecha` con el método `metodo` (desde el punto de partida, si lo hay) y su
/// valoración con los precios de cierre.
pub(crate) fn posiciones_valoradas(
    v: &Vault,
    operaciones: &[Operacion],
    metodo: Metodo,
    fecha: Date,
    plataformas: &[Platform],
    inicio: Option<&cripto::Inicio>,
) -> Result<(Vec<PosicionView>, Vec<String>)> {
    posiciones_valoradas_con(v, operaciones, metodo, fecha, plataformas, true, inicio)
}

/// Posiciones a `fecha` como las ve la Renta: FIFO global por moneda, sin separar plataformas
/// (como `renta`), valoradas con los precios de cierre. Para saber qué monedas pierden de verdad
/// al venderlas todas.
pub(crate) fn posiciones_renta(v: &Vault, fecha: Date) -> Result<Vec<PosicionView>> {
    let (operaciones, _) = operaciones(v)?;
    let plataformas = platforms(v)?;
    let inicio = inicio_motor(v)?;
    Ok(posiciones_valoradas_con(
        v,
        &operaciones,
        Metodo::Fifo,
        fecha,
        &plataformas,
        false,
        inicio.as_ref(),
    )?
    .0)
}

fn posiciones_valoradas_con(
    v: &Vault,
    operaciones: &[Operacion],
    metodo: Metodo,
    fecha: Date,
    plataformas: &[Platform],
    por_plataforma: bool,
    inicio: Option<&cripto::Inicio>,
) -> Result<(Vec<PosicionView>, Vec<String>)> {
    let c = cripto::calcular_desde(operaciones, metodo, Some(fecha), por_plataforma, inicio);
    let mut out = Vec::new();
    for p in &c.posiciones {
        let precio = precio_a(v, &p.ticker, fecha)?;
        let (valor, latente) = match precio {
            Some((_, pr)) => {
                cripto::valorar(p, pr).map_or((None, None), |(a, b)| (Some(a), Some(b)))
            }
            None => (None, None),
        };
        let pl = plataformas.iter().find(|x| x.id == p.plataforma);
        out.push(PosicionView {
            ticker: p.ticker.clone(),
            platform_id: p.plataforma,
            platform: nombre_plataforma(plataformas, p.plataforma),
            abroad: pl.is_some_and(|x| x.abroad),
            cantidad: p.cantidad,
            coste: p.coste,
            precio: precio.map(|(_, pr)| pr),
            precio_fecha: precio.map(|(d, _)| d).filter(|d| *d != fecha),
            valor,
            latente,
        });
    }
    Ok((out, c.avisos))
}

fn m721_de(
    v: &Vault,
    year: i32,
    fecha: Date,
    posiciones: &[PosicionView],
    plataformas: &[Platform],
) -> M721 {
    let mut por: BTreeMap<i64, PlataformaExtranjero> = BTreeMap::new();
    let mut sin_precio = Vec::new();
    for p in posiciones.iter().filter(|p| p.abroad) {
        let pl = plataformas.iter().find(|x| x.id == p.platform_id);
        let e = por
            .entry(p.platform_id)
            .or_insert_with(|| PlataformaExtranjero {
                platform: p.platform.clone(),
                country: pl.map(|x| x.country.clone()).unwrap_or_default(),
                valor: Money::ZERO,
                activos: Vec::new(),
            });
        match p.valor {
            Some(val) => {
                e.valor += val;
                e.activos.push((p.ticker.clone(), p.cantidad, val));
            }
            None => {
                e.activos.push((p.ticker.clone(), p.cantidad, Money::ZERO));
                if !sin_precio.contains(&p.ticker) {
                    sin_precio.push(p.ticker.clone());
                }
            }
        }
    }
    let total: Money = por.values().map(|p| p.valor).sum();
    let supera = total > UMBRAL_721;
    let en_perfil = v
        .setting::<TaxpayerProfile>("taxpayer_profile")
        .ok()
        .flatten()
        .is_some_and(|p| p.monedas_virtuales_extranjero);
    let fisica = es_persona_fisica(v);
    let nota = if por.is_empty() {
        "No hay saldos en plataformas marcadas como del extranjero: el 721 no toca. Si un \
         exchange de fuera no está marcado así, edítalo en «Plataformas»."
            .to_owned()
    } else if !sin_precio.is_empty() {
        format!(
            "Faltan precios de cierre a {fecha} de {}: la valoración en euros está incompleta. \
             Añádelos en «Precios de cierre» (la cotización a 31/12 de la plataforma, art. 42 \
             quater.3.c RGAT).",
            sin_precio.join(", ")
        )
    } else if supera {
        format!(
            "Los saldos en el extranjero a {fecha} suman {total}, más de 50.000 €: toca el 721 \
             de {year} entre el 1 de enero y el 31 de marzo de {} (art. 42 quater RGAT; Orden \
             HFP/886/2023), salvo {}. Se declara por cada plataforma su nombre, NIF del país, \
             domicilio o web, y por cada moneda el saldo en unidades y en euros con la \
             cotización usada. Los años siguientes sólo si el saldo conjunto sube más de \
             20.000 € respecto al último declarado.",
            year + 1,
            if fisica {
                "que lleves contabilidad por el Código de Comercio con las monedas identificadas \
                 (art. 42 quater.5.c)"
            } else {
                "que las tengas en la contabilidad identificadas por denominación, valor, \
                 custodio y país (art. 42 quater.5.b): contabilizarlas desde aquí lo cumple"
            }
        )
    } else {
        format!(
            "Los saldos en el extranjero a {fecha} suman {total}: no llegan a 50.000 €, no hay \
             que presentar el 721 de {year} (art. 42 quater.5.d RGAT)."
        )
    };
    let mut sin_plataforma: Vec<String> = posiciones
        .iter()
        .filter(|p| p.platform_id == SIN_PLATAFORMA)
        .map(|p| p.ticker.clone())
        .collect();
    sin_plataforma.dedup();
    let nota = if sin_plataforma.is_empty() {
        nota
    } else {
        format!(
            "{nota} Ojo: {} del punto de partida no {} plataforma y no se cuenta{} aquí; si \
             está{} en un exchange o custodio de fuera, cuenta{} para el 721: pónsela en el punto \
             de partida.",
            sin_plataforma.join(", "),
            if sin_plataforma.len() == 1 {
                "tiene"
            } else {
                "tienen"
            },
            if sin_plataforma.len() == 1 { "" } else { "n" },
            if sin_plataforma.len() == 1 { "" } else { "n" },
            if sin_plataforma.len() == 1 { "" } else { "n" },
        )
    };
    M721 {
        year,
        fecha,
        total_extranjero: total,
        umbral: UMBRAL_721,
        supera,
        plataformas: por.into_values().collect(),
        sin_precio,
        sin_plataforma,
        en_perfil,
        exenta_por_contabilidad: !fisica,
        nota,
    }
}

/// Informe del ejercicio `year`. Con punto de partida, el cálculo empieza en él: un ejercicio
/// que acaba en la fecha de corte o antes no tiene resultados calculados aquí (los calculó la
/// gestoría) y sus operaciones son historial.
pub fn report(v: &Vault, year: i32) -> Result<Report> {
    let settings = settings(v)?;
    let persona_fisica = es_persona_fisica(v);
    let metodo_declaracion = if persona_fisica {
        Metodo::Fifo
    } else {
        settings.metodo
    };
    let (desde, fecha) = ejercicio(v, year);
    let plataformas = platforms(v)?;
    let inicio = inicio::inicio(v)?;
    let motor = inicio.as_ref().map(inicio::Inicio::motor);
    let corte = motor.as_ref().map(|i| i.fecha);
    let antes_del_inicio = corte.is_some_and(|c| fecha <= c);
    let (operaciones, mut avisos) = operaciones(v)?;
    let (posiciones, avisos_calc) = posiciones_valoradas(
        v,
        &operaciones,
        settings.metodo,
        fecha,
        &plataformas,
        motor.as_ref(),
    )?;
    avisos.extend(avisos_calc);
    // Una persona física declara con FIFO de todas sus monedas iguales juntas, estén en la
    // plataforma que estén (como `renta`); una sociedad sigue las unidades de cada plataforma
    // (con precio medio, el de todas las unidades de la moneda).
    let por_plataforma = !persona_fisica;
    let pmp = cripto::calcular_desde(
        &operaciones,
        Metodo::Pmp,
        None,
        por_plataforma,
        motor.as_ref(),
    );
    let fifo = cripto::calcular_desde(
        &operaciones,
        Metodo::Fifo,
        None,
        por_plataforma,
        motor.as_ref(),
    );
    let elegido = if metodo_declaracion == Metodo::Fifo {
        &fifo
    } else {
        &pmp
    };
    let realizados = elegido
        .realizados
        .iter()
        .filter(|r| (desde..=fecha).contains(&r.fecha))
        .map(|r| RealizadoView {
            op: r.op,
            fecha: r.fecha,
            tipo: r.tipo,
            ticker: r.ticker.clone(),
            platform: nombre_plataforma(&plataformas, r.plataforma),
            cantidad: r.cantidad,
            valor: r.valor,
            coste: r.coste,
            resultado: r.resultado,
        })
        .collect();
    // El 721 es a 31 de diciembre (art. 42 quater.3 RGAT), aunque el ejercicio no sea natural.
    let dic31 = Date::from_calendar_date(year, Month::December, 31).unwrap_or(fecha);
    let pos_721 = if dic31 == fecha {
        posiciones.clone()
    } else {
        posiciones_valoradas(
            v,
            &operaciones,
            settings.metodo,
            dic31,
            &plataformas,
            motor.as_ref(),
        )?
        .0
    };
    let mut m721 = m721_de(v, year, dic31, &pos_721, &plataformas);
    if persona_fisica && settings.metodo == Metodo::Pmp {
        avisos.insert(
            0,
            "Eres persona física: en el IRPF las monedas que se transmiten son las adquiridas \
             en primer lugar (FIFO; art. 37.2 LIRPF por analogía, DGT V0999-18). El método \
             «precio medio ponderado» de los ajustes sólo se muestra como comparación; la Renta \
             y los asientos usan FIFO."
                .into(),
        );
    }
    let ops_year = ops(v, Some(year))?;
    let ops_historial = ops_year
        .iter()
        .filter(|o| corte.is_some_and(|c| o.date <= c))
        .count();
    if let Some(c) = corte {
        if antes_del_inicio {
            avisos.insert(
                0,
                format!(
                    "Ejercicio {year}: calculado por la gestoría. El punto de partida es el {}: \
                     {}.",
                    inicio::dmy(c),
                    if fecha == c {
                        "las posiciones a esa fecha son las suyas, con el coste pendiente que \
                         figura en la contabilidad; las operaciones de este ejercicio son \
                         historial (se ven, no cuentan para el coste ni para las ganancias)"
                    } else {
                        "las operaciones de este ejercicio son historial (se ven, no cuentan) y \
                         aquí no se calculan posiciones ni ganancias: valen las que se declararon"
                    }
                ),
            );
            if fecha < c {
                m721.nota = format!(
                    "Ejercicio anterior al punto de partida ({}): los saldos de entonces no se \
                     calculan aquí porque faltan operaciones.",
                    inicio::dmy(c)
                );
            }
        } else if ops_historial > 0 {
            avisos.insert(
                0,
                format!(
                    "Lo realizado hasta el {} lo calculó la gestoría (punto de partida): aquí \
                     cuentan las operaciones desde el día siguiente; las {ops_historial} de \
                     antes son historial.",
                    inicio::dmy(c)
                ),
            );
        }
        let sin: Vec<&str> = posiciones
            .iter()
            .filter(|p| p.platform_id == SIN_PLATAFORMA)
            .map(|p| p.ticker.as_str())
            .collect();
        if !sin.is_empty() && fecha >= c {
            avisos.push(format!(
                "{} del punto de partida no {} plataforma: no cuentan para el 721 hasta que se la \
                 pongas (una sociedad que las tiene contabilizadas no presenta el 721, art. 42 \
                 quater.5.b RGAT).",
                sin.join(", "),
                if sin.len() == 1 { "tiene" } else { "tienen" }
            ));
        }
    }
    let huecos = if corte.is_none() {
        inicio::huecos(v)?
    } else {
        Vec::new()
    };
    if let Some(t) = inicio::texto_huecos(&huecos) {
        let mut t = t;
        if let Some(c) = t.get_mut(..1) {
            c.make_ascii_uppercase();
        }
        avisos.insert(0, format!("{t}."));
    }
    let sin_precio: Vec<&PosicionView> = posiciones.iter().filter(|p| p.valor.is_none()).collect();
    if !sin_precio.is_empty() {
        let mut t: Vec<String> = sin_precio.iter().map(|p| p.ticker.clone()).collect();
        t.dedup();
        avisos.push(format!(
            "Sin precio de cierre a {fecha} de {}: no se puede valorar ni calcular el deterioro. \
             Añádelo en «Precios de cierre».",
            t.join(", ")
        ));
    }
    let ops_con_asiento = ops_year.iter().filter(|o| o.entry_id.is_some()).count();
    let deterioro_registrado = asientos::asiento_deterioro(v.connection(), year)?;
    Ok(Report {
        year,
        fecha,
        valor_total: posiciones.iter().filter_map(|p| p.valor).sum(),
        coste_total: posiciones.iter().map(|p| p.coste).sum(),
        settings,
        persona_fisica,
        metodo_declaracion,
        posiciones,
        resumen_pmp: cripto::resumen_entre(&pmp, year, desde, fecha),
        resumen_fifo: cripto::resumen_entre(&fifo, year, desde, fecha),
        realizados,
        m721,
        ops_ejercicio: ops_year.len(),
        // Las de hasta la fecha de corte no se contabilizan desde aquí.
        ops_sin_asiento: ops_year
            .iter()
            .filter(|o| o.entry_id.is_none() && !corte.is_some_and(|c| o.date <= c))
            .count(),
        ops_con_asiento,
        deterioro_registrado,
        avisos,
        inicio,
        antes_del_inicio,
        ops_historial,
        huecos: huecos.len(),
    })
}

// ---------------- Renta (modelo 100) ----------------

/// Una transmisión para las casillas de monedas virtuales del modelo 100.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RentaLinea {
    pub fecha: Date,
    pub ticker: String,
    /// Clave de contraprestación de la casilla [1803]: F (euros), N (otra moneda virtual), B
    /// (bienes o servicios). TODO(VERIFY): las letras son del desplegable de Renta Web, no de la
    /// Orden HAC/277/2026 ni de la ayuda 7.6.6.2; cotejarlas en Renta Web.
    pub clave: &'static str,
    pub cantidad: Qty,
    pub valor_transmision: Money,
    pub valor_adquisicion: Money,
    pub resultado: Money,
}

/// Ganancias y pérdidas por criptomonedas de la persona física (FIFO global por activo).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CriptoRenta {
    pub year: i32,
    pub ganancias: Money,
    pub perdidas: Money,
    pub neto: Money,
    pub recompensas: Money,
    pub lineas: Vec<RentaLinea>,
    pub notas: Vec<String>,
}

/// `None` si no hay operaciones ese año o si el año acaba en la fecha de corte del punto de
/// partida o antes (lo declarado entonces es lo que vale).
pub fn renta(v: &Vault, year: i32) -> Result<Option<CriptoRenta>> {
    let (operaciones, avisos) = operaciones(v)?;
    if !operaciones.iter().any(|o| o.fecha.year() == year) {
        return Ok(None);
    }
    let inicio = inicio_motor(v)?;
    let fin =
        Date::from_calendar_date(year, Month::December, 31).map_err(|e| other(e.to_string()))?;
    if inicio.as_ref().is_some_and(|i| fin <= i.fecha) {
        return Ok(None);
    }
    let c = cripto::calcular_desde(&operaciones, Metodo::Fifo, None, false, inicio.as_ref());
    let r = cripto::resumen(&c, year);
    let lineas = c
        .realizados
        .iter()
        .filter(|x| x.fecha.year() == year)
        .map(|x| RentaLinea {
            fecha: x.fecha,
            ticker: x.ticker.clone(),
            clave: match x.tipo {
                Tipo::Permuta => "N",
                Tipo::Pago | Tipo::Comision => "B",
                _ => "F",
            },
            cantidad: x.cantidad,
            valor_transmision: x.valor,
            valor_adquisicion: x.coste,
            resultado: x.resultado,
        })
        .collect();
    let mut notas = vec![
        "Ganancias y pérdidas patrimoniales de la base del ahorro (arts. 33, 46 y 49 LIRPF), \
         calculadas con FIFO global por moneda (art. 37.2 LIRPF por analogía; DGT V2520-22 y \
         ayuda de Renta Web): van en el apartado «transmisión o permuta de monedas virtuales por \
         particulares» del modelo 100 (casillas 1800 a 1814; las sumas en la 1813 y la 1814), \
         una línea por transmisión o agrupadas por moneda y clave; no son rendimiento de la \
         actividad salvo trading profesional."
            .to_owned(),
    ];
    if !r.recompensas.is_zero() {
        notas.push(format!(
            "Las recompensas de staking o airdrops ({}) se valoran por su valor de mercado en \
             euros el día en que se reciben; la DGT califica el staking como rendimiento del \
             capital mobiliario de la base del ahorro (art. 25.2 LIRPF; V1766-22) y los airdrops \
             como ganancia patrimonial sin transmisión de la base general (art. 37.1.l LIRPF; \
             V1948-21). Decláralas en su apartado, no aquí.",
            r.recompensas
        ));
    }
    notas.extend(avisos);
    notas.extend(c.avisos);
    Ok(Some(CriptoRenta {
        year,
        ganancias: r.ganancias,
        perdidas: r.perdidas,
        neto: r.neto,
        recompensas: r.recompensas,
        lineas,
        notas,
    }))
}

// ---------------- Para los avisos de umbral ----------------

/// Saldo en el extranjero a 31/12 de `year` y activos sin precio, o `None` sin operaciones (ni
/// punto de partida) o antes de la fecha de corte (no se sabe).
pub fn saldo_extranjero(v: &Vault, year: i32) -> Result<Option<M721>> {
    let (operaciones, _) = operaciones(v)?;
    let inicio = inicio_motor(v)?;
    if operaciones.is_empty() && inicio.is_none() {
        return Ok(None);
    }
    let fecha =
        Date::from_calendar_date(year, Month::December, 31).map_err(|e| other(e.to_string()))?;
    if inicio.as_ref().is_some_and(|i| fecha < i.fecha) {
        return Ok(None);
    }
    let plataformas = platforms(v)?;
    let s = settings(v)?;
    let (posiciones, _) = posiciones_valoradas(
        v,
        &operaciones,
        s.metodo,
        fecha,
        &plataformas,
        inicio.as_ref(),
    )?;
    if posiciones.is_empty() {
        return Ok(None);
    }
    Ok(Some(m721_de(v, year, fecha, &posiciones, &plataformas)))
}
