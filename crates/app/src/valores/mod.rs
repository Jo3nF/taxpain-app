//! Inversiones en valores: cuentas de bróker (XTB, Interactive Brokers u otro), valores
//! (acciones, ETF, fondos, bonos), operaciones, precios de cierre, informe del ejercicio con el
//! coste medio ponderado (sociedad) o FIFO (persona física), dividendos y retenciones en origen,
//! modelo 720 y asientos propuestos. El cálculo puro está en [`calculo`].
//!
//! Conectores:
//! - **Interactive Brokers**: Flex Web Service (token y Query ID de una Activity Flex Query que
//!   el usuario crea en su Client Portal). La descarga la hace la app de escritorio sólo cuando
//!   se pulsa «Sincronizar» (`taxpain_banca::ibkr`); aquí se lee el XML ([`flex`]) y se aplica sin
//!   duplicar ([`importar`]). También un fichero Flex XML o CSV descargado a mano.
//! - **XTB**: no tiene API (cerró la xAPI el 14 de marzo de 2025, según su centro de ayuda y el
//!   aviso del repositorio `pawelkn/xapi-python`, consultados el 2026-11-02): se importan los PDF
//!   ([`xtb`]) y los Excel, uno por cuenta ([`xtb_xlsx`]), que exporta xStation 5 desde
//!   «Historial de la cuenta». Una cuenta en dólares se pasa a euros con el tipo del BCE del
//!   día ([`aplicar_bce`], al pulsar «Pasar a euros con el BCE»); las opciones son valores de
//!   clase [`Clase::Derivado`] y su resultado va aparte.
//!
//! Nada sale del Mac salvo la petición al propio bróker (IBKR) cuando el usuario la pide.

pub mod asientos;
pub mod calculo;
pub mod contabilidad;
pub mod export;
pub mod flex;
pub mod importar;
pub mod informe;
pub mod xtb;
pub mod xtb_xlsx;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_contabilidad;

use serde::{Deserialize, Serialize};
use taxpain_core::cripto::{Precio, Qty};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;

pub use informe::{Report, report};

pub const SETTINGS_KEY: &str = "valores_settings";

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

fn iso_date(s: &str) -> Date {
    Date::parse(s, &time::format_description::well_known::Iso8601::DATE).unwrap_or(Date::MIN)
}

// ---------------- Ajustes ----------------

/// Cómo se clasifica la cartera en la contabilidad de la sociedad (NRV 9.ª).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Cartera {
    /// A coste (PGC PYMES, NRV 9.ª 2.3: «los demás instrumentos de patrimonio») o a valor
    /// razonable con cambios en el patrimonio neto (PGC, NRV 9.ª 2.3): comisiones al coste y
    /// coste medio ponderado al vender.
    #[default]
    Coste,
    /// Mantenidos para negociar (NRV 9.ª 2.2 PYMES / 2.1 PGC): valor razonable con cambios en
    /// la cuenta de pérdidas y ganancias y comisiones a resultados.
    Negociacion,
}

/// A corto (grupo 5) o a largo plazo (grupo 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Plazo {
    #[default]
    Corto,
    Largo,
}

/// Cuentas de los asientos propuestos (configurables). Nombres del cuadro de cuentas del PGC y
/// del PGC PYMES (cuarta parte), comprobados en el BOE el 2026-11-02.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cuentas {
    /// 540 «Inversiones financieras a corto plazo en instrumentos de patrimonio».
    pub corto_patrimonio: String,
    /// 541 «Valores representativos de deuda a corto plazo».
    pub corto_deuda: String,
    /// 250 «Inversiones financieras a largo plazo en instrumentos de patrimonio».
    pub largo_patrimonio: String,
    /// 251 «Valores representativos de deuda a largo plazo».
    pub largo_deuda: String,
    /// Efectivo en el bróker (cada cuenta puede llevar la suya).
    pub tesoreria: String,
    /// 766 / 666 «Beneficios / Pérdidas en participaciones y valores representativos de deuda»
    /// (activos a coste).
    pub beneficio: String,
    pub perdida: String,
    /// 7630 / 6630 «Beneficios / Pérdidas de cartera de negociación» (en el PGC PYMES, 763 y 663
    /// sin subcuentas).
    pub beneficio_vr: String,
    pub perdida_vr: String,
    /// 760 «Ingresos de participaciones en instrumentos de patrimonio».
    pub dividendos: String,
    /// 769 «Otros ingresos financieros» (intereses del efectivo en el bróker).
    pub intereses: String,
    /// 669 «Otros gastos financieros» (comisiones a resultados e intereses pagados).
    pub comisiones: String,
    /// 473 «Hacienda Pública, retenciones y pagos a cuenta» (retenciones españolas).
    pub retencion_espana: String,
    /// 6300 «Impuesto corriente»: el impuesto pagado en el extranjero se registra como el gasto
    /// por impuesto sobre sociedades (Resolución del ICAC de 9-2-2016, art. 12.1; el 12.6 admite
    /// la 635 «Impuesto sobre beneficios extranjero»).
    pub impuesto_extranjero: String,
    /// 631 «Otros tributos».
    pub tributos: String,
    /// Deterioro de las acciones, ETF y fondos de una cartera a coste (PGC PYMES, cuarta parte,
    /// comprobado en el BOE el 2026-11-16): a corto plazo 698 «Pérdidas por deterioro de
    /// participaciones y valores representativos de deuda a corto plazo» contra la 596
    /// «Deterioro de valor de participaciones a corto plazo» y su reversión a la 798; a largo
    /// plazo 6963 «… instrumentos de patrimonio neto a largo plazo, otras empresas» contra la 296
    /// «Deterioro de valor de participaciones en el patrimonio neto a largo plazo» y su reversión
    /// a la 7963.
    #[serde(default = "def_deterioro_corto")]
    pub deterioro_corto: String,
    #[serde(default = "def_correctora_corto")]
    pub correctora_corto: String,
    #[serde(default = "def_reversion_corto")]
    pub reversion_corto: String,
    #[serde(default = "def_deterioro_largo")]
    pub deterioro_largo: String,
    #[serde(default = "def_correctora_largo")]
    pub correctora_largo: String,
    #[serde(default = "def_reversion_largo")]
    pub reversion_largo: String,
}

fn def_deterioro_corto() -> String {
    "698".into()
}
fn def_correctora_corto() -> String {
    "596".into()
}
fn def_reversion_corto() -> String {
    "798".into()
}
fn def_deterioro_largo() -> String {
    "6963".into()
}
fn def_correctora_largo() -> String {
    "296".into()
}
fn def_reversion_largo() -> String {
    "7963".into()
}

impl Default for Cuentas {
    fn default() -> Self {
        Cuentas {
            corto_patrimonio: "540".into(),
            corto_deuda: "541".into(),
            largo_patrimonio: "250".into(),
            largo_deuda: "251".into(),
            tesoreria: "5720090".into(),
            beneficio: "766".into(),
            perdida: "666".into(),
            beneficio_vr: "7630".into(),
            perdida_vr: "6630".into(),
            dividendos: "760".into(),
            intereses: "769".into(),
            comisiones: "669".into(),
            retencion_espana: "473".into(),
            impuesto_extranjero: "6300".into(),
            tributos: "631".into(),
            deterioro_corto: def_deterioro_corto(),
            correctora_corto: def_correctora_corto(),
            reversion_corto: def_reversion_corto(),
            deterioro_largo: def_deterioro_largo(),
            correctora_largo: def_correctora_largo(),
            reversion_largo: def_reversion_largo(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub cartera: Cartera,
    #[serde(default)]
    pub plazo: Plazo,
    #[serde(default)]
    pub cuentas: Cuentas,
}

pub fn settings(v: &Vault) -> Result<Settings> {
    Ok(v.setting(SETTINGS_KEY)?.unwrap_or_default())
}

fn check_code(c: &str) -> Result<()> {
    taxpain_core::accounting::AccountCode::new(c)
        .map(|_| ())
        .map_err(|_| other(format!("cuenta no válida: «{c}»")))
}

pub fn save_settings(v: &Vault, s: &Settings, now: OffsetDateTime) -> Result<()> {
    let c = &s.cuentas;
    for code in [
        &c.corto_patrimonio,
        &c.corto_deuda,
        &c.largo_patrimonio,
        &c.largo_deuda,
        &c.tesoreria,
        &c.beneficio,
        &c.perdida,
        &c.beneficio_vr,
        &c.perdida_vr,
        &c.dividendos,
        &c.intereses,
        &c.comisiones,
        &c.retencion_espana,
        &c.impuesto_extranjero,
        &c.tributos,
        &c.deterioro_corto,
        &c.correctora_corto,
        &c.reversion_corto,
        &c.deterioro_largo,
        &c.correctora_largo,
        &c.reversion_largo,
    ] {
        check_code(code)?;
    }
    // La clasificación se elige al reconocer los títulos y no se cambia por gusto (NRV 9.ª:
    // reclasificaciones tasadas): con asientos grabados, hay que deshacerlos antes.
    let antes = settings(v)?;
    if (antes.cartera != s.cartera || antes.plazo != s.plazo) && contabilidad::hay_asientos(v)? {
        return Err(other(
            "ya hay asientos de la cartera grabados: deshazlos en «Asientos» antes de cambiar la \
             clasificación o el plazo (NRV 9.ª: la clasificación no se cambia libremente)",
        ));
    }
    v.set_setting(SETTINGS_KEY, s)?;
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        "valores.ajustes",
        "",
        &serde_json::json!({ "cartera": s.cartera, "plazo": s.plazo }),
    )?;
    Ok(())
}

// ---------------- Cuentas de bróker ----------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Broker {
    Xtb,
    Ibkr,
    #[default]
    Otro,
}

impl Broker {
    pub fn code(self) -> &'static str {
        match self {
            Broker::Xtb => "xtb",
            Broker::Ibkr => "ibkr",
            Broker::Otro => "otro",
        }
    }

    pub fn parse(s: &str) -> Broker {
        match s {
            "xtb" => Broker::Xtb,
            "ibkr" => Broker::Ibkr,
            _ => Broker::Otro,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Broker::Xtb => "XTB",
            Broker::Ibkr => "Interactive Brokers",
            Broker::Otro => "Otro bróker",
        }
    }

    /// País del depositario y si cuenta como «en el extranjero» para el 720, por defecto (se
    /// puede cambiar). XTB opera en España como sucursal de XTB S.A. (Polonia; registro de
    /// sucursales de ESI del EEE de la CNMV, n.º 40) y los valores están en una cuenta ómnibus
    /// de un custodio para XTB (anexos al contrato, art. 4.36); Interactive Brokers da servicio
    /// a los clientes de la UE desde Interactive Brokers Ireland Limited. `TODO(VERIFY)`: el
    /// país del custodio de XTB y si la sucursal informa a la AEAT de los valores (lo que
    /// eximiría del 720 a una persona física; FAQ del 720 «otras causas de exoneración»).
    pub fn pais_por_defecto(self) -> (&'static str, bool) {
        match self {
            Broker::Xtb => ("PL", true),
            Broker::Ibkr => ("IE", true),
            Broker::Otro => ("", true),
        }
    }
}

/// Cuenta de bróker tal como se ve y se edita (sin el token: sólo si lo hay).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cuenta {
    #[serde(default)]
    pub id: i64,
    pub broker: Broker,
    pub name: String,
    #[serde(default)]
    pub number: String,
    #[serde(default = "eur")]
    pub currency: String,
    #[serde(default)]
    pub country: String,
    #[serde(default = "si")]
    pub abroad: bool,
    /// Subcuenta del efectivo en el bróker (vacía = la de los ajustes).
    #[serde(default)]
    pub account: String,
    /// IBKR: Query ID de la Activity Flex Query.
    #[serde(default)]
    pub flex_query: String,
    #[serde(default)]
    pub last_sync: Option<String>,
    #[serde(default)]
    pub last_sync_note: String,
    /// IBKR: hay token del Flex Web Service guardado (nunca se devuelve el token).
    #[serde(default)]
    pub token: bool,
}

fn eur() -> String {
    "EUR".into()
}

fn si() -> bool {
    true
}

/// Nombre del secreto con el token del Flex Web Service de una cuenta.
fn secreto_token(id: i64) -> String {
    format!("valores.ibkr.token.{id}")
}

fn tiene_token(conn: &Connection, id: i64) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM app_secret WHERE name = ?1",
            [secreto_token(id)],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .is_some())
}

const CUENTA_COLS: &str = "id, broker, name, number, currency, country, abroad, account, \
     flex_query, last_sync, last_sync_note";

fn row_cuenta(r: &taxpain_vault::rusqlite::Row<'_>) -> taxpain_vault::rusqlite::Result<Cuenta> {
    let broker: String = r.get(1)?;
    Ok(Cuenta {
        id: r.get(0)?,
        broker: Broker::parse(&broker),
        name: r.get(2)?,
        number: r.get(3)?,
        currency: r.get(4)?,
        country: r.get(5)?,
        abroad: r.get::<_, i64>(6)? == 1,
        account: r.get(7)?,
        flex_query: r.get(8)?,
        last_sync: r.get(9)?,
        last_sync_note: r.get(10)?,
        token: false,
    })
}

pub fn cuentas(v: &Vault) -> Result<Vec<Cuenta>> {
    let conn = v.connection();
    let mut st = conn.prepare(&format!(
        "SELECT {CUENTA_COLS} FROM security_account ORDER BY name"
    ))?;
    let rows: Vec<Cuenta> = st
        .query_map([], row_cuenta)?
        .collect::<std::result::Result<_, _>>()?;
    rows.into_iter()
        .map(|mut c| {
            c.token = tiene_token(conn, c.id)?;
            Ok(c)
        })
        .collect()
}

pub fn cuenta(v: &Vault, id: i64) -> Result<Cuenta> {
    let conn = v.connection();
    let mut c = conn
        .query_row(
            &format!("SELECT {CUENTA_COLS} FROM security_account WHERE id = ?1"),
            [id],
            row_cuenta,
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    c.token = tiene_token(conn, id)?;
    Ok(c)
}

fn check_cuenta(c: &Cuenta) -> Result<()> {
    if c.name.trim().is_empty() {
        return Err(other("la cuenta necesita un nombre"));
    }
    if !c.country.is_empty()
        && (c.country.len() != 2 || !c.country.chars().all(|x| x.is_ascii_alphabetic()))
    {
        return Err(other("país: código de dos letras (PL, IE, ES…)"));
    }
    if c.currency.len() != 3 || !c.currency.chars().all(|x| x.is_ascii_alphabetic()) {
        return Err(other("divisa: código de tres letras (EUR, USD…)"));
    }
    if !c.account.is_empty() {
        check_code(&c.account)?;
    }
    let q = c.flex_query.trim();
    if !q.is_empty() && (q.len() > 20 || !q.chars().all(|x| x.is_ascii_digit())) {
        return Err(other(
            "el Query ID de la Flex Query son sólo cifras (lo ves en el icono «Info» de la query)",
        ));
    }
    Ok(())
}

/// Alta (`id` 0) o cambio de una cuenta. Devuelve su id.
pub fn guardar_cuenta(v: &Vault, c: &Cuenta, now: OffsetDateTime) -> Result<i64> {
    check_cuenta(c)?;
    let conn = v.connection();
    let country = c.country.to_uppercase();
    let currency = c.currency.to_uppercase();
    let id = if c.id == 0 {
        conn.execute(
            "INSERT INTO security_account(broker, name, number, currency, country, abroad, account,
               flex_query, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                c.broker.code(),
                c.name.trim(),
                c.number.trim(),
                currency,
                country,
                i64::from(c.abroad),
                c.account,
                c.flex_query.trim(),
                now_str(now)
            ],
        )
        .map_err(|e| match e {
            taxpain_vault::rusqlite::Error::SqliteFailure(f, _)
                if f.code == taxpain_vault::rusqlite::ErrorCode::ConstraintViolation =>
            {
                other("ya hay una cuenta con ese nombre")
            }
            e => e.into(),
        })?;
        conn.last_insert_rowid()
    } else {
        let n = conn.execute(
            "UPDATE security_account SET broker = ?1, name = ?2, number = ?3, currency = ?4,
               country = ?5, abroad = ?6, account = ?7, flex_query = ?8 WHERE id = ?9",
            params![
                c.broker.code(),
                c.name.trim(),
                c.number.trim(),
                currency,
                country,
                i64::from(c.abroad),
                c.account,
                c.flex_query.trim(),
                c.id
            ],
        )?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        c.id
    };
    audit::append(
        conn,
        &now_str(now),
        Actor::User,
        "valores.cuenta",
        &id.to_string(),
        &serde_json::json!({ "broker": c.broker.code(), "pais": country }),
    )?;
    Ok(id)
}

pub fn borrar_cuenta(v: &Vault, id: i64, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let used: i64 = conn.query_row(
        "SELECT COUNT(*) FROM security_op WHERE account_id = ?1 OR to_account_id = ?1",
        [id],
        |r| r.get(0),
    )?;
    if used > 0 {
        return Err(other(format!(
            "esa cuenta tiene {used} operaciones: bórralas antes"
        )));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM security_import WHERE account_id = ?1", [id])?;
    tx.execute(
        "DELETE FROM app_secret WHERE name = ?1",
        [secreto_token(id)],
    )?;
    if tx.execute("DELETE FROM security_account WHERE id = ?1", [id])? == 0 {
        return Err(AppError::NotFound);
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "valores.cuenta.baja",
        &id.to_string(),
        &serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

/// Cuenta de un bróker por su número; la crea si no existe (nombre «XTB 1234…»).
pub(crate) fn asegurar_cuenta(
    conn: &Connection,
    broker: Broker,
    number: &str,
    currency: &str,
    now: &str,
) -> Result<(i64, bool)> {
    let number = number.trim();
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM security_account WHERE broker = ?1 AND number = ?2",
            params![broker.code(), number],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
    {
        return Ok((id, false));
    }
    // Una cuenta del mismo bróker dada de alta a mano sin número: se completa.
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM security_account WHERE broker = ?1 AND number = '' ORDER BY id LIMIT 1",
            [broker.code()],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
    {
        conn.execute(
            "UPDATE security_account SET number = ?1 WHERE id = ?2",
            params![number, id],
        )?;
        return Ok((id, false));
    }
    let (country, abroad) = broker.pais_por_defecto();
    let base = if number.is_empty() {
        broker.label().to_owned()
    } else {
        format!("{} {number}", broker.label())
    };
    let mut name = base.clone();
    let mut n = 2;
    while conn
        .query_row(
            "SELECT 1 FROM security_account WHERE name = ?1",
            [&name],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        .is_some()
    {
        name = format!("{base} ({n})");
        n += 1;
    }
    conn.execute(
        "INSERT INTO security_account(broker, name, number, currency, country, abroad, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            broker.code(),
            name,
            number,
            if currency.len() == 3 { currency } else { "EUR" },
            country,
            i64::from(abroad),
            now
        ],
    )?;
    Ok((conn.last_insert_rowid(), true))
}

// ---------------- Token del Flex Web Service (IBKR) ----------------

/// Guarda el token del Flex Web Service y el Query ID de una cuenta de Interactive Brokers.
/// El token va a la BD cifrada (clave maestra en el Llavero) y no vuelve a la interfaz.
pub fn guardar_token_ibkr(
    v: &Vault,
    id: i64,
    token: &str,
    query: &str,
    now: OffsetDateTime,
) -> Result<()> {
    let mut c = cuenta(v, id)?;
    if c.broker != Broker::Ibkr {
        return Err(other("el token es de una cuenta de Interactive Brokers"));
    }
    let token = token.trim();
    if token.len() < 10 || token.len() > 64 || !token.chars().all(|x| x.is_ascii_alphanumeric()) {
        return Err(other(
            "el token del Flex Web Service son letras y cifras (cópialo tal cual del Client Portal)",
        ));
    }
    c.flex_query = query.trim().to_owned();
    if c.flex_query.is_empty() {
        return Err(other("falta el Query ID de la Flex Query"));
    }
    check_cuenta(&c)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE security_account SET flex_query = ?1 WHERE id = ?2",
        params![c.flex_query, id],
    )?;
    tx.execute(
        "INSERT INTO app_secret(name, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(name) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![secreto_token(id), token, now_str(now)],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "valores.ibkr.token",
        &id.to_string(),
        &serde_json::json!({ "guardado": true }),
    )?;
    tx.commit()?;
    Ok(())
}

pub fn quitar_token_ibkr(v: &Vault, id: i64, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    conn.execute(
        "DELETE FROM app_secret WHERE name = ?1",
        [secreto_token(id)],
    )?;
    audit::append(
        conn,
        &now_str(now),
        Actor::User,
        "valores.ibkr.token",
        &id.to_string(),
        &serde_json::json!({ "guardado": false }),
    )?;
    Ok(())
}

/// Token y Query ID para sincronizar (el token se borra de memoria al soltarlo).
pub fn credenciales_ibkr(v: &Vault, id: i64) -> Result<(zeroize::Zeroizing<String>, String)> {
    let c = cuenta(v, id)?;
    let token: Option<String> = v
        .connection()
        .query_row(
            "SELECT value FROM app_secret WHERE name = ?1",
            [secreto_token(id)],
            |r| r.get(0),
        )
        .optional()?;
    match token {
        Some(t) if !c.flex_query.is_empty() => Ok((zeroize::Zeroizing::new(t), c.flex_query)),
        _ => Err(other(
            "conecta antes la cuenta: guarda el token del Flex Web Service y el Query ID",
        )),
    }
}

// ---------------- Valores ----------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Clase {
    #[default]
    Accion,
    Etf,
    Fondo,
    Bono,
    Cfd,
    /// Opciones y otros derivados con prima (compra de una opción, su venta o su vencimiento):
    /// su resultado va en una línea aparte del informe.
    Derivado,
    Otro,
}

impl Clase {
    pub fn code(self) -> &'static str {
        match self {
            Clase::Accion => "accion",
            Clase::Etf => "etf",
            Clase::Fondo => "fondo",
            Clase::Bono => "bono",
            Clase::Cfd => "cfd",
            Clase::Derivado => "derivado",
            Clase::Otro => "otro",
        }
    }

    pub fn parse(s: &str) -> Clase {
        match s {
            "etf" => Clase::Etf,
            "fondo" => Clase::Fondo,
            "bono" => Clase::Bono,
            "cfd" => Clase::Cfd,
            "derivado" => Clase::Derivado,
            "otro" => Clase::Otro,
            _ => Clase::Accion,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Clase::Accion => "Acción",
            Clase::Etf => "ETF",
            Clase::Fondo => "Fondo",
            Clase::Bono => "Bono",
            Clase::Cfd => "CFD",
            Clase::Derivado => "Derivado (opción)",
            Clase::Otro => "Otro",
        }
    }

    /// Valor representativo de deuda (541/251) en vez de instrumento de patrimonio (540/250).
    pub fn deuda(self) -> bool {
        self == Clase::Bono
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Valor {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub isin: String,
    pub ticker: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "eur")]
    pub currency: String,
    #[serde(default)]
    pub kind: Clase,
    #[serde(default)]
    pub country: String,
    #[serde(default)]
    pub account: String,
}

/// ISIN: dos letras de país, nueve caracteres y un dígito de control (ISO 6166).
pub fn isin_valido(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 12
        || !b[..2].iter().all(u8::is_ascii_uppercase)
        || !b[2..11].iter().all(u8::is_ascii_alphanumeric)
        || !b[11].is_ascii_digit()
    {
        return false;
    }
    // Luhn sobre los dígitos que salen de cambiar cada letra por su número (A = 10…).
    let digits: Vec<u32> = b
        .iter()
        .flat_map(|c| {
            let n = if c.is_ascii_digit() {
                u32::from(c - b'0')
            } else {
                u32::from(c - b'A') + 10
            };
            if n >= 10 {
                vec![n / 10, n % 10]
            } else {
                vec![n]
            }
        })
        .collect();
    let sum: u32 = digits
        .iter()
        .rev()
        .enumerate()
        .map(|(i, d)| {
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 { x - 9 } else { x }
            } else {
                *d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

fn row_valor(r: &taxpain_vault::rusqlite::Row<'_>) -> taxpain_vault::rusqlite::Result<Valor> {
    let kind: String = r.get(5)?;
    Ok(Valor {
        id: r.get(0)?,
        isin: r.get(1)?,
        ticker: r.get(2)?,
        name: r.get(3)?,
        currency: r.get(4)?,
        kind: Clase::parse(&kind),
        country: r.get(6)?,
        account: r.get(7)?,
    })
}

const VALOR_COLS: &str = "id, isin, ticker, name, currency, kind, country, account";

pub fn valores(v: &Vault) -> Result<Vec<Valor>> {
    let mut st = v.connection().prepare(&format!(
        "SELECT {VALOR_COLS} FROM security ORDER BY ticker, id"
    ))?;
    let rows = st.query_map([], row_valor)?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Cambia los datos de un valor. Si se le pone el ISIN de otro valor que ya existe (el mismo
/// ETF en XTB y en IBKR), se juntan: sus operaciones y precios pasan al que ya tenía el ISIN.
/// Devuelve el id con el que queda.
pub fn guardar_valor(v: &Vault, x: &Valor, now: OffsetDateTime) -> Result<i64> {
    let isin = x.isin.trim().to_uppercase();
    if !isin.is_empty() && !isin_valido(&isin) {
        return Err(other(format!("ISIN no válido: «{isin}»")));
    }
    if x.ticker.trim().is_empty() {
        return Err(other("el valor necesita un ticker o símbolo"));
    }
    if !x.account.is_empty() {
        check_code(&x.account)?;
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let otro: Option<i64> = if isin.is_empty() {
        None
    } else {
        tx.query_row(
            "SELECT id FROM security WHERE isin = ?1 AND id <> ?2",
            params![isin, x.id],
            |r| r.get(0),
        )
        .optional()?
    };
    // Alta: si ya hay un valor con ese ISIN, es ése.
    if x.id == 0 {
        let id = match otro {
            Some(id) => id,
            None => {
                let l = importar::ValorLeido {
                    clave: x.ticker.clone(),
                    isin: isin.clone(),
                    ticker: x.ticker.clone(),
                    nombre: x.name.clone(),
                    divisa: {
                        let c = x.currency.trim().to_uppercase();
                        if c.len() == 3 { c } else { "EUR".into() }
                    },
                    clase: x.kind,
                    pais: x.country.trim().to_uppercase(),
                };
                let id = asegurar_valor(&tx, &l, &now_str(now))?;
                if !x.account.is_empty() {
                    tx.execute(
                        "UPDATE security SET account = ?1 WHERE id = ?2",
                        params![x.account, id],
                    )?;
                }
                id
            }
        };
        tx.commit()?;
        return Ok(id);
    }
    let id = match otro {
        Some(destino) => {
            tx.execute(
                "UPDATE security_op SET security_id = ?1 WHERE security_id = ?2",
                params![destino, x.id],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO security_price(date, security_id, price, currency, source,
                   updated_at)
                 SELECT date, ?1, price, currency, source, updated_at FROM security_price
                 WHERE security_id = ?2",
                params![destino, x.id],
            )?;
            tx.execute("DELETE FROM security_price WHERE security_id = ?1", [x.id])?;
            tx.execute("DELETE FROM security WHERE id = ?1", [x.id])?;
            destino
        }
        None => {
            let n = tx.execute(
                "UPDATE security SET isin = ?1, ticker = ?2, name = ?3, currency = ?4, kind = ?5,
                   country = ?6, account = ?7 WHERE id = ?8",
                params![
                    isin,
                    x.ticker.trim().to_uppercase(),
                    x.name.trim(),
                    x.currency.trim().to_uppercase(),
                    x.kind.code(),
                    x.country.trim().to_uppercase(),
                    x.account,
                    x.id
                ],
            )?;
            if n == 0 {
                return Err(AppError::NotFound);
            }
            x.id
        }
    };
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "valores.valor",
        &id.to_string(),
        &serde_json::json!({ "isin": isin, "junto": otro.is_some() }),
    )?;
    tx.commit()?;
    Ok(id)
}

/// Valor por ISIN o, sin él, por ticker; lo crea si no existe. Si llega con ISIN y había uno con
/// el mismo ticker sin ISIN (de otra importación), se le pone.
pub(crate) fn asegurar_valor(
    conn: &Connection,
    l: &importar::ValorLeido,
    now: &str,
) -> Result<i64> {
    let ticker = l.ticker.trim().to_uppercase();
    let isin = l.isin.trim().to_uppercase();
    let isin = if isin_valido(&isin) {
        isin
    } else {
        String::new()
    };
    if !isin.is_empty() {
        if let Some(id) = conn
            .query_row("SELECT id FROM security WHERE isin = ?1", [&isin], |r| {
                r.get::<_, i64>(0)
            })
            .optional()?
        {
            completar_valor(conn, id, l)?;
            return Ok(id);
        }
        if let Some(id) = conn
            .query_row(
                "SELECT id FROM security WHERE ticker = ?1 AND isin = '' ORDER BY id LIMIT 1",
                [&ticker],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
        {
            conn.execute(
                "UPDATE security SET isin = ?1 WHERE id = ?2",
                params![isin, id],
            )?;
            completar_valor(conn, id, l)?;
            return Ok(id);
        }
    } else if let Some(id) = conn
        .query_row(
            "SELECT id FROM security WHERE ticker = ?1 ORDER BY id LIMIT 1",
            [&ticker],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO security(isin, ticker, name, currency, kind, country, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            isin,
            ticker,
            if l.nombre.trim().is_empty() {
                ticker.as_str()
            } else {
                l.nombre.trim()
            },
            l.divisa,
            l.clase.code(),
            l.pais,
            now
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Completa el nombre, la clase y el país de un valor con lo que trae el bróker (sin pisar lo
/// que el usuario haya cambiado: sólo si el nombre es el ticker).
fn completar_valor(conn: &Connection, id: i64, l: &importar::ValorLeido) -> Result<()> {
    if l.nombre.trim().is_empty() {
        return Ok(());
    }
    conn.execute(
        "UPDATE security SET name = ?1, kind = ?2,
           country = CASE WHEN country = '' THEN ?3 ELSE country END
         WHERE id = ?4 AND (name = '' OR name = ticker)",
        params![l.nombre.trim(), l.clase.code(), l.pais, id],
    )?;
    Ok(())
}

// ---------------- Operaciones ----------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TipoOp {
    Compra,
    Venta,
    Dividendo,
    /// Retención (en origen o española) sobre un dividendo o unos intereses.
    Retencion,
    Interes,
    /// Comisión que no va en una compra o venta (custodia, datos, conexión…).
    Comision,
    /// Otros tributos (p. ej. el impuesto sobre transacciones financieras suelto).
    Impuesto,
    /// Traspaso de títulos entre cuentas propias.
    Traspaso,
    /// Desdoblamiento o agrupación: títulos que se añaden (o quitan) sin coste.
    Split,
    Deposito,
    Retirada,
}

impl TipoOp {
    pub const TODOS: [TipoOp; 11] = [
        TipoOp::Compra,
        TipoOp::Venta,
        TipoOp::Dividendo,
        TipoOp::Retencion,
        TipoOp::Interes,
        TipoOp::Comision,
        TipoOp::Impuesto,
        TipoOp::Traspaso,
        TipoOp::Split,
        TipoOp::Deposito,
        TipoOp::Retirada,
    ];

    pub fn code(self) -> &'static str {
        match self {
            TipoOp::Compra => "compra",
            TipoOp::Venta => "venta",
            TipoOp::Dividendo => "dividendo",
            TipoOp::Retencion => "retencion",
            TipoOp::Interes => "interes",
            TipoOp::Comision => "comision",
            TipoOp::Impuesto => "impuesto",
            TipoOp::Traspaso => "traspaso",
            TipoOp::Split => "split",
            TipoOp::Deposito => "deposito",
            TipoOp::Retirada => "retirada",
        }
    }

    pub fn parse(s: &str) -> Option<TipoOp> {
        TipoOp::TODOS.into_iter().find(|t| t.code() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            TipoOp::Compra => "Compra",
            TipoOp::Venta => "Venta",
            TipoOp::Dividendo => "Dividendo",
            TipoOp::Retencion => "Retención",
            TipoOp::Interes => "Intereses",
            TipoOp::Comision => "Comisión",
            TipoOp::Impuesto => "Impuesto",
            TipoOp::Traspaso => "Traspaso",
            TipoOp::Split => "Desdoblamiento",
            TipoOp::Deposito => "Ingreso en el bróker",
            TipoOp::Retirada => "Retirada del bróker",
        }
    }

    /// Mueve títulos de un valor.
    pub fn con_titulos(self) -> bool {
        matches!(
            self,
            TipoOp::Compra | TipoOp::Venta | TipoOp::Traspaso | TipoOp::Split
        )
    }
}

/// Una operación tal como se guarda y se edita.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Op {
    #[serde(default)]
    pub id: i64,
    pub account_id: i64,
    #[serde(default)]
    pub to_account_id: Option<i64>,
    #[serde(default)]
    pub security_id: Option<i64>,
    pub date: Date,
    #[serde(default = "medianoche")]
    pub time: String,
    pub kind: TipoOp,
    #[serde(default)]
    pub qty: Qty,
    /// Precio por título en la divisa del valor.
    #[serde(default)]
    pub price: Option<Precio>,
    #[serde(default = "eur")]
    pub currency: String,
    /// Importe en `currency` (bruto, sin la comisión).
    #[serde(default)]
    pub amount: Money,
    #[serde(default)]
    pub fee: Money,
    /// Euros por unidad de `currency` (vacío en euros).
    #[serde(default)]
    pub fx: Option<Precio>,
    #[serde(default)]
    pub eur: Money,
    #[serde(default)]
    pub fee_eur: Money,
    /// País de la retención (ISO 3166-1 alfa-2).
    #[serde(default)]
    pub country: String,
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

/// `importe × euros por unidad`, al céntimo (mitad lejos del cero).
pub fn a_euros(importe: Money, fx: Precio) -> Money {
    let p = i128::from(importe.cents()) * fx.raw();
    let d: i128 = 100_000_000;
    let q = p / d;
    let r = p % d;
    let q = if 2 * r.abs() >= d { q + p.signum() } else { q };
    Money::from_cents(i64::try_from(q).unwrap_or(0))
}

/// Euros de una operación escrita a mano: en euros, lo mismo; en otra divisa, con su tipo.
fn completar_euros(op: &mut Op) -> Result<()> {
    op.currency = op.currency.trim().to_uppercase();
    if op.currency == "EUR" {
        op.fx = None;
        op.eur = op.amount;
        op.fee_eur = op.fee;
        return Ok(());
    }
    match op.fx.filter(|f| !f.is_zero() && *f > Precio::ZERO) {
        Some(fx) => {
            op.eur = a_euros(op.amount, fx);
            op.fee_eur = a_euros(op.fee, fx);
            Ok(())
        }
        None if op.amount.is_zero() && op.fee.is_zero() => Ok(()),
        None => Err(other(format!(
            "falta el tipo de cambio de {} (euros por unidad, o «Tipo del BCE»)",
            op.currency
        ))),
    }
}

pub(crate) fn check_op(op: &Op) -> Result<()> {
    if op.currency.len() != 3 || !op.currency.chars().all(|c| c.is_ascii_uppercase()) {
        return Err(other("divisa: código de tres letras (EUR, USD…)"));
    }
    if op.time.len() != 8 || op.time.split(':').count() != 3 {
        return Err(other("hora en formato HH:MM:SS"));
    }
    if op.date.year() < 1980 {
        return Err(other("revisa la fecha"));
    }
    if op.fee.is_negative() || op.fee_eur.is_negative() {
        return Err(other("la comisión no puede ser negativa"));
    }
    match op.kind {
        TipoOp::Compra | TipoOp::Venta => {
            if op.security_id.is_none() {
                return Err(other("falta el valor"));
            }
            if op.qty.is_zero() || op.qty.is_negative() {
                return Err(other("los títulos han de ser más que cero"));
            }
            if op.amount.is_negative() || op.eur.is_negative() {
                return Err(other("el importe no puede ser negativo"));
            }
        }
        TipoOp::Traspaso => {
            if op.security_id.is_none() || op.qty.is_zero() || op.qty.is_negative() {
                return Err(other("el traspaso necesita el valor y los títulos"));
            }
            if op.to_account_id.is_none_or(|d| d == op.account_id) {
                return Err(other(
                    "el traspaso necesita una cuenta de destino distinta de la de origen",
                ));
            }
        }
        TipoOp::Split => {
            if op.security_id.is_none() || op.qty.is_zero() {
                return Err(other(
                    "el desdoblamiento necesita el valor y los títulos que se añaden",
                ));
            }
        }
        TipoOp::Deposito | TipoOp::Retirada => {
            if op.amount.is_zero() || op.amount.is_negative() {
                return Err(other("el importe ha de ser mayor que cero"));
            }
        }
        TipoOp::Dividendo
        | TipoOp::Retencion
        | TipoOp::Interes
        | TipoOp::Comision
        | TipoOp::Impuesto => {
            if op.amount.is_zero() && op.eur.is_zero() {
                return Err(other("falta el importe"));
            }
        }
    }
    if op.country.len() > 2 {
        return Err(other("país: código de dos letras"));
    }
    Ok(())
}

const OP_COLS: &str = "id, account_id, to_account_id, security_id, date, time, kind, qty, price, \
     currency, amount, fee, fx, eur, fee_eur, country, note, source, fingerprint, entry_id";

fn row_op(r: &taxpain_vault::rusqlite::Row<'_>) -> taxpain_vault::rusqlite::Result<Op> {
    let date: String = r.get(4)?;
    let kind: String = r.get(6)?;
    let qty: String = r.get(7)?;
    let price: Option<String> = r.get(8)?;
    let fx: Option<String> = r.get(12)?;
    Ok(Op {
        id: r.get(0)?,
        account_id: r.get(1)?,
        to_account_id: r.get(2)?,
        security_id: r.get(3)?,
        date: iso_date(&date),
        time: r.get(5)?,
        kind: TipoOp::parse(&kind).unwrap_or(TipoOp::Comision),
        qty: Qty::parse(&qty).unwrap_or(Qty::ZERO),
        price: price.as_deref().and_then(Precio::parse),
        currency: r.get(9)?,
        amount: Money::from_cents(r.get(10)?),
        fee: Money::from_cents(r.get(11)?),
        fx: fx.as_deref().and_then(Precio::parse),
        eur: Money::from_cents(r.get(13)?),
        fee_eur: Money::from_cents(r.get(14)?),
        country: r.get(15)?,
        note: r.get(16)?,
        source: r.get(17)?,
        fingerprint: r.get(18)?,
        entry_id: r.get(19)?,
    })
}

/// Todas las operaciones (por fecha y hora) o sólo las del año `year`.
pub fn ops(v: &Vault, year: Option<i32>) -> Result<Vec<Op>> {
    let conn = v.connection();
    let mut st = conn.prepare(&format!(
        "SELECT {OP_COLS} FROM security_op ORDER BY date, time, id"
    ))?;
    let all: Vec<Op> = st
        .query_map([], row_op)?
        .collect::<std::result::Result<_, _>>()?;
    Ok(match year {
        Some(y) => all.into_iter().filter(|o| o.date.year() == y).collect(),
        None => all,
    })
}

pub(crate) fn insert_op(conn: &Connection, op: &Op, now: &str) -> Result<i64> {
    conn.execute(
        "INSERT INTO security_op(account_id, to_account_id, security_id, date, time, kind, qty,
           price, currency, amount, fee, fx, eur, fee_eur, country, note, source, fingerprint,
           created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
           ?19)",
        params![
            op.account_id,
            op.to_account_id,
            op.security_id,
            op.date.to_string(),
            op.time,
            op.kind.code(),
            op.qty.to_text(),
            op.price.map(Precio::to_text),
            op.currency,
            op.amount.cents(),
            op.fee.cents(),
            op.fx.map(Precio::to_text),
            op.eur.cents(),
            op.fee_eur.cents(),
            op.country.to_uppercase(),
            op.note.trim(),
            op.source,
            op.fingerprint,
            now,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Alta (`id` 0) o cambio de una operación escrita a mano.
pub fn guardar_op(v: &Vault, op: &Op, now: OffsetDateTime) -> Result<i64> {
    if op.id != 0 {
        contabilidad::sin_asiento(v.connection(), op.id)?;
    }
    let mut op = op.clone();
    completar_euros(&mut op)?;
    check_op(&op)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let ts = now_str(now);
    let id = if op.id == 0 {
        op.source = "manual".into();
        op.fingerprint = None;
        insert_op(&tx, &op, &ts)?
    } else {
        let n = tx.execute(
            "UPDATE security_op SET account_id = ?1, to_account_id = ?2, security_id = ?3,
               date = ?4, time = ?5, kind = ?6, qty = ?7, price = ?8, currency = ?9, amount = ?10,
               fee = ?11, fx = ?12, eur = ?13, fee_eur = ?14, country = ?15, note = ?16
             WHERE id = ?17",
            params![
                op.account_id,
                op.to_account_id,
                op.security_id,
                op.date.to_string(),
                op.time,
                op.kind.code(),
                op.qty.to_text(),
                op.price.map(Precio::to_text),
                op.currency,
                op.amount.cents(),
                op.fee.cents(),
                op.fx.map(Precio::to_text),
                op.eur.cents(),
                op.fee_eur.cents(),
                op.country.to_uppercase(),
                op.note.trim(),
                op.id
            ],
        )?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        op.id
    };
    audit::append(
        &tx,
        &ts,
        Actor::User,
        if op.id == 0 {
            "valores.alta"
        } else {
            "valores.cambio"
        },
        &id.to_string(),
        &serde_json::json!({ "tipo": op.kind.code(), "fecha": op.date.to_string() }),
    )?;
    tx.commit()?;
    Ok(id)
}

pub fn borrar_op(v: &Vault, id: i64, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let kind: Option<String> = tx
        .query_row("SELECT kind FROM security_op WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    let Some(kind) = kind else {
        return Err(AppError::NotFound);
    };
    contabilidad::sin_asiento(&tx, id)?;
    tx.execute("DELETE FROM security_op WHERE id = ?1", [id])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "valores.baja",
        &id.to_string(),
        &serde_json::json!({ "tipo": kind }),
    )?;
    tx.commit()?;
    Ok(())
}

// ---------------- Precios de cierre y tipos de cambio ----------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrecioCierre {
    pub date: Date,
    pub security_id: i64,
    pub price: Precio,
    #[serde(default = "eur")]
    pub currency: String,
    #[serde(default = "manual")]
    pub source: String,
}

pub fn precios(v: &Vault) -> Result<Vec<PrecioCierre>> {
    let mut st = v.connection().prepare(
        "SELECT date, security_id, price, currency, source FROM security_price
         ORDER BY date DESC, security_id",
    )?;
    let rows = st.query_map([], |r| {
        let d: String = r.get(0)?;
        let p: String = r.get(2)?;
        Ok(PrecioCierre {
            date: iso_date(&d),
            security_id: r.get(1)?,
            price: Precio::parse(&p).unwrap_or(Precio::ZERO),
            currency: r.get(3)?,
            source: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Guarda precios. Uno tecleado a mano no lo pisa luego el del bróker.
pub(crate) fn poner_precio(conn: &Connection, p: &PrecioCierre, now: &str) -> Result<bool> {
    if p.price.is_zero() || p.price < Precio::ZERO {
        return Err(other(format!(
            "precio del {}: ha de ser mayor que cero",
            p.date
        )));
    }
    let n = conn.execute(
        "INSERT INTO security_price(date, security_id, price, currency, source, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(date, security_id) DO UPDATE SET price = excluded.price,
           currency = excluded.currency, source = excluded.source, updated_at = excluded.updated_at
         WHERE security_price.source <> 'manual' OR excluded.source = 'manual'",
        params![
            p.date.to_string(),
            p.security_id,
            p.price.to_text(),
            p.currency.to_uppercase(),
            p.source,
            now
        ],
    )?;
    Ok(n > 0)
}

pub fn guardar_precios(v: &Vault, ps: &[PrecioCierre], now: OffsetDateTime) -> Result<usize> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let ts = now_str(now);
    let mut n = 0;
    for p in ps {
        let p = PrecioCierre {
            source: "manual".into(),
            ..p.clone()
        };
        if poner_precio(&tx, &p, &ts)? {
            n += 1;
        }
    }
    tx.commit()?;
    Ok(n)
}

pub fn borrar_precio(v: &Vault, date: Date, security_id: i64) -> Result<()> {
    v.connection().execute(
        "DELETE FROM security_price WHERE date = ?1 AND security_id = ?2",
        params![date.to_string(), security_id],
    )?;
    Ok(())
}

/// Precio de cierre de un valor a `date`: el de ese día o el último anterior (con su fecha y
/// divisa).
pub fn precio_a(v: &Vault, security_id: i64, date: Date) -> Result<Option<(Date, Precio, String)>> {
    let row: Option<(String, String, String)> = v
        .connection()
        .query_row(
            "SELECT date, price, currency FROM security_price WHERE security_id = ?1 AND date <= ?2
             ORDER BY date DESC LIMIT 1",
            params![security_id, date.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    Ok(row.and_then(|(d, p, c)| Some((iso_date(&d), Precio::parse(&p)?, c))))
}

/// Euros por unidad de `currency` a `date`: el del bróker de ese día o de la semana anterior y,
/// si no, el tipo del BCE ya descargado (`fx`). `None` si no hay ninguno.
pub fn cambio_a(v: &Vault, currency: &str, date: Date) -> Result<Option<(Date, Precio)>> {
    if currency == "EUR" {
        return Ok(Some((date, Precio::from_euros(1))));
    }
    let desde = date - time::Duration::days(7);
    let row: Option<(String, String)> = v
        .connection()
        .query_row(
            "SELECT date, eur_per_unit FROM security_fx WHERE currency = ?1 AND date <= ?2
               AND date >= ?3 ORDER BY date DESC LIMIT 1",
            params![currency, date.to_string(), desde.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((d, p)) = row
        && let Some(p) = Precio::parse(&p)
    {
        return Ok(Some((iso_date(&d), p)));
    }
    // El BCE da unidades por euro: euros por unidad = 1 / tipo (8 decimales).
    if let Some(c) = crate::fx::cached(v, currency, date)?
        .and_then(|r| eur_por_unidad(r.rate).map(|p| (r.date, p)))
    {
        return Ok(Some(c));
    }
    // Una fecha que aún no ha llegado (el cierre de un ejercicio abierto): el último tipo
    // conocido, del bróker o del BCE, con su fecha.
    let hoy = OffsetDateTime::now_utc().date();
    if date > hoy {
        return ultimo_cambio(v, currency, hoy);
    }
    Ok(None)
}

/// El último cambio conocido de `currency` hasta `hasta` (bróker o BCE, el más reciente).
pub(crate) fn ultimo_cambio(
    v: &Vault,
    currency: &str,
    hasta: Date,
) -> Result<Option<(Date, Precio)>> {
    let broker: Option<(String, String)> = v
        .connection()
        .query_row(
            "SELECT date, eur_per_unit FROM security_fx WHERE currency = ?1 AND date <= ?2
             ORDER BY date DESC LIMIT 1",
            params![currency, hasta.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let broker = broker.and_then(|(d, p)| Some((iso_date(&d), Precio::parse(&p)?)));
    let bce = crate::fx::ultimo_cached(v, currency, hasta)?
        .and_then(|r| eur_por_unidad(r.rate).map(|p| (r.date, p)));
    Ok(match (broker, bce) {
        (Some(a), Some(b)) => Some(if b.0 > a.0 { b } else { a }),
        (a, b) => a.or(b),
    })
}

pub(crate) fn poner_cambio(
    conn: &Connection,
    date: Date,
    currency: &str,
    eur_por_unidad: Precio,
    source: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO security_fx(date, currency, eur_per_unit, source) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(date, currency) DO UPDATE SET eur_per_unit = excluded.eur_per_unit,
           source = excluded.source",
        params![date.to_string(), currency, eur_por_unidad.to_text(), source],
    )?;
    Ok(())
}

/// Euros por unidad a partir del tipo del BCE (unidades por euro), con 8 decimales.
pub fn eur_por_unidad(rate: taxpain_core::fx::ExchangeRate) -> Option<Precio> {
    let micros = i128::from(rate.micros());
    (micros > 0)
        .then(|| Precio::from_raw((100_000_000i128 * 1_000_000 * 2 + micros) / (2 * micros)))
}

/// Operaciones en otra divisa que aún no tienen euros (sin el cambio del bróker ni el del BCE
/// guardado): por divisa, la primera y la última fecha. Es lo que se pide al BCE con «Pasar a
/// euros con el BCE».
pub fn pendientes_bce(v: &Vault) -> Result<Vec<(String, Date, Date)>> {
    let mut por: std::collections::BTreeMap<String, (Date, Date)> =
        std::collections::BTreeMap::new();
    for o in ops(v, None)?.iter().filter(|o| sin_euros(o)) {
        let e = por.entry(o.currency.clone()).or_insert((o.date, o.date));
        e.0 = e.0.min(o.date);
        e.1 = e.1.max(o.date);
    }
    Ok(por.into_iter().map(|(c, (a, b))| (c, a, b)).collect())
}

fn sin_euros(o: &Op) -> bool {
    o.currency != "EUR"
        && o.fx.is_none()
        && o.eur.is_zero()
        && o.fee_eur.is_zero()
        && (!o.amount.is_zero() || !o.fee.is_zero())
}

/// Guarda los tipos de referencia del BCE descargados de `currency` (fecha de publicación y
/// unidades por euro) y pasa a euros las operaciones que no los tenían. Devuelve cuántas.
pub fn aplicar_bce(
    v: &Vault,
    currency: &str,
    tipos: &[(Date, taxpain_core::fx::ExchangeRate)],
    host: &str,
    now: OffsetDateTime,
) -> Result<usize> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    for (d, r) in tipos {
        if let Some(p) = eur_por_unidad(*r) {
            // Un tipo del bróker de ese día no se pisa.
            tx.execute(
                "INSERT INTO security_fx(date, currency, eur_per_unit, source)
                 VALUES (?1, ?2, ?3, 'bce') ON CONFLICT(date, currency) DO NOTHING",
                params![d.to_string(), currency, p.to_text()],
            )?;
        }
    }
    let mut n = 0usize;
    for o in ops(v, None)?
        .iter()
        .filter(|o| o.currency == currency && sin_euros(o) && o.entry_id.is_none())
    {
        let Some((_, fx)) = cambio_a(v, currency, o.date)? else {
            continue;
        };
        tx.execute(
            "UPDATE security_op SET fx = ?1, eur = ?2, fee_eur = ?3 WHERE id = ?4",
            params![
                fx.to_text(),
                a_euros(o.amount, fx).cents(),
                a_euros(o.fee, fx).cents(),
                o.id
            ],
        )?;
        n += 1;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "valores.bce",
        currency,
        &serde_json::json!({ "host": host, "tipos": tipos.len(), "operaciones": n }),
    )?;
    tx.commit()?;
    Ok(n)
}
