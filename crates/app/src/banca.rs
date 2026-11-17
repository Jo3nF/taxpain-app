//! Conexión de **solo lectura** con los bancos (PSD2, información de cuentas) a través de Enable
//! Banking: lo que va al almacén. La red la hace `taxpain-banca`.
//!
//! - **Ajustes y clave**: la empresa acepta la conexión ([`guardar_ajustes`]) y guarda el id y
//!   la clave privada de *su* aplicación de Enable Banking ([`guardar_credenciales`]). Van en la
//!   BD cifrada, fuera de las copias de seguridad, y la clave no vuelve nunca a la interfaz.
//! - **Autorización**: [`preparar`] apunta una autorización pendiente con un valor al azar
//!   (`state`); el banco vuelve con un código y ese mismo valor, y [`guardar_sesion`] guarda la
//!   sesión y sus cuentas. Volver a autorizar el mismo banco sustituye a la conexión anterior
//!   (y conserva lo bajado de cada cuenta).
//! - **Descarga**: [`actualizar`] baja los movimientos contabilizados de cada cuenta activa (la
//!   primera vez, todo lo que dé el banco; después, desde unos días antes de la última descarga)
//!   y los importa como un extracto más ([`importar`]): la misma cuenta y subcuenta que sus
//!   extractos en fichero y sin duplicados (por el identificador del banco y, contra los de
//!   fichero, por fecha, importe y saldo).

use std::collections::BTreeSet;
use std::future::Future;
use std::ops::Deref;

use serde::{Deserialize, Serialize};
use taxpain_banca::BancaError;
use taxpain_banca::enable::{Aspsp, Cliente, PeticionAuth, Saldo, Sesion, Transaccion};
use taxpain_banca::jwt::Firmante;
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::format_description::well_known::Rfc3339;
use time::{Date, Duration, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::extractos::importar::{self, AVISO_SIN_SALDOS, Entrada, ExtractoImportado};
use crate::extractos::{Clase, Extracto, Movimiento};
use crate::invoicing::now_str;

const AJUSTES: &str = "banca";
const SECRETO_APP: &str = "banca.enable.app_id@api.enablebanking.com";
const SECRETO_CLAVE: &str = "banca.enable.key@api.enablebanking.com";

/// Bancos que se ofrecen por ahora (como los llama Enable Banking).
pub const BANCOS: &[&str] = &["BBVA", "Revolut"];

/// Dirección de vuelta que se propone registrar en la aplicación de Enable Banking: no la
/// escucha nadie, así que el navegador enseña un error, pero la barra de direcciones lleva el
/// código y se pega en TaxPain.
pub const URL_VUELTA: &str = "https://localhost/taxpain/banco";

/// Días que se vuelven a pedir antes de la última descarga: hay bancos que contabilizan un
/// movimiento con fecha de unos días antes.
const SOLAPE_DIAS: i64 = 10;

/// Una autorización pendiente vale una hora.
const PENDIENTE_MAX: Duration = Duration::hours(1);

/// Permiso más largo que se pide: 180 días (lo que deja PSD2 sin volver a autorizar).
const PERMISO_MAX_S: i64 = 180 * 86_400;

/// Permiso que se pide si el banco no dice su máximo.
const PERMISO_DEFECTO_S: i64 = 90 * 86_400;

fn otro(m: impl Into<String>) -> AppError {
    AppError::Other(m.into())
}

/// Ajustes de la conexión con los bancos.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ajustes {
    /// La empresa ha aceptado que TaxPain se conecte con Enable Banking para leer sus cuentas.
    pub aceptado: bool,
    /// Dirección de vuelta registrada en la aplicación de Enable Banking.
    pub url_vuelta: String,
}

impl Default for Ajustes {
    fn default() -> Self {
        Self {
            aceptado: false,
            url_vuelta: URL_VUELTA.to_owned(),
        }
    }
}

/// Una cuenta de una conexión.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CuentaConectada {
    pub id: i64,
    pub nombre: String,
    /// IBAN enmascarado («ES91 …1332»).
    pub iban: Option<String>,
    pub divisa: String,
    pub tarjeta: bool,
    pub activa: bool,
    /// Cuenta de TaxPain en la que entran sus movimientos, y su subcuenta.
    pub cuenta_id: Option<i64>,
    pub subcuenta: Option<String>,
    /// Último día descargado.
    pub hasta_dia: Option<String>,
    /// Saldo que da el banco y su fecha.
    pub saldo: Option<Money>,
    pub saldo_fecha: Option<String>,
}

/// Una conexión con un banco.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Conexion {
    pub id: i64,
    pub banco: String,
    pub pais: String,
    /// «business» (empresa) o «personal».
    pub tipo: String,
    /// «activa» o «caducada».
    pub estado: String,
    /// Fin del permiso (RFC 3339) y días que quedan.
    pub hasta: Option<String>,
    pub dias: Option<i64>,
    pub actualizada: Option<String>,
    pub error: Option<String>,
    pub cuentas: Vec<CuentaConectada>,
}

/// Una autorización empezada que espera la vuelta del banco.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Pendiente {
    pub banco: String,
    pub tipo: String,
    pub desde: String,
}

/// Todo lo de los bancos, tal como lo ve la interfaz (sin la clave).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Estado {
    pub ajustes: Ajustes,
    /// Id de la aplicación de Enable Banking (no es secreto: va en cada petición).
    pub app_id: Option<String>,
    /// Hay clave privada guardada.
    pub clave: bool,
    /// La dirección de vuelta la recoge TaxPain sola (`http://127.0.0.1:<puerto>/…`).
    pub vuelta_local: bool,
    pub conexiones: Vec<Conexion>,
    pub pendiente: Option<Pendiente>,
}

pub fn ajustes(v: &Vault) -> Result<Ajustes> {
    Ok(v.setting(AJUSTES)?.unwrap_or_default())
}

/// La dirección de vuelta ha de ser `https://…` o `http://127.0.0.1:<puerto>/…` (la que escucha
/// TaxPain).
fn comprobar_url_vuelta(u: &str) -> Result<()> {
    let url = url::Url::parse(u.trim())
        .map_err(|_| otro("la dirección de vuelta no es una dirección web válida"))?;
    let ok = match url.scheme() {
        "https" => url.host_str().is_some(),
        "http" => taxpain_banca::vuelta::local(u).is_some(),
        _ => false,
    };
    if ok && url.query().is_none() && url.fragment().is_none() {
        Ok(())
    } else {
        Err(otro(
            "la dirección de vuelta ha de empezar por https:// (o ser http://127.0.0.1:<puerto>/…) \
             y no llevar «?» ni «#»",
        ))
    }
}

pub fn guardar_ajustes(v: &Vault, a: &Ajustes, now: OffsetDateTime) -> Result<()> {
    comprobar_url_vuelta(&a.url_vuelta)?;
    let a = Ajustes {
        aceptado: a.aceptado,
        url_vuelta: a.url_vuelta.trim().to_owned(),
    };
    v.set_setting(AJUSTES, &a)?;
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        "banca.ajustes",
        "enable banking",
        &serde_json::json!({ "aceptado": a.aceptado, "url_vuelta": a.url_vuelta }),
    )?;
    Ok(())
}

fn secreto(v: &Vault, nombre: &str) -> Result<Option<String>> {
    Ok(v.connection()
        .query_row(
            "SELECT value FROM app_secret WHERE name = ?1",
            [nombre],
            |r| r.get(0),
        )
        .optional()?)
}

fn poner_secreto(v: &Vault, nombre: &str, valor: &str, now: OffsetDateTime) -> Result<()> {
    v.connection().execute(
        "INSERT INTO app_secret(name, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(name) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        params![nombre, valor, now_str(now)],
    )?;
    Ok(())
}

/// Guarda el id y la clave privada (PEM) de la aplicación de Enable Banking, después de
/// comprobar que la clave sirve para firmar (RSA).
pub fn guardar_credenciales(v: &Vault, app_id: &str, pem: &str, now: OffsetDateTime) -> Result<()> {
    let f = Firmante::nuevo(app_id, pem)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    poner_secreto(v, SECRETO_APP, f.kid(), now)?;
    poner_secreto(v, SECRETO_CLAVE, pem.trim(), now)?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "banca.clave",
        "guardada",
        &serde_json::json!({ "app_id": f.kid() }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Borra la clave de la aplicación (las conexiones quedan, pero no se pueden usar).
pub fn quitar_credenciales(v: &Vault, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM app_secret WHERE name IN (?1, ?2)",
        params![SECRETO_APP, SECRETO_CLAVE],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "banca.clave",
        "borrada",
        &serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

/// El firmante de las peticiones, si la conexión está aceptada y la clave guardada.
pub fn firmante(v: &Vault) -> Result<Firmante> {
    if !ajustes(v)?.aceptado {
        return Err(otro(
            "acepta primero la conexión con los bancos (en Contabilidad → Banco)",
        ));
    }
    let (Some(app), Some(pem)) = (secreto(v, SECRETO_APP)?, secreto(v, SECRETO_CLAVE)?) else {
        return Err(otro(
            "falta la clave de tu aplicación de Enable Banking (el fichero .pem y su id)",
        ));
    };
    let pem = zeroize::Zeroizing::new(pem);
    Ok(Firmante::nuevo(&app, &pem)?)
}

/// El cliente de Enable Banking de la empresa.
pub fn cliente(v: &Vault) -> Result<Cliente> {
    Ok(Cliente::nuevo(firmante(v)?)?)
}

/// El cliente sólo para retirar un permiso: con la clave guardada, aunque ya no se acepte la
/// conexión (retirar el permiso es justo lo que pide quien deja de aceptarla).
pub fn cliente_para_retirar(v: &Vault) -> Result<Cliente> {
    let (Some(app), Some(pem)) = (secreto(v, SECRETO_APP)?, secreto(v, SECRETO_CLAVE)?) else {
        return Err(otro(
            "sin la clave de la aplicación no se puede retirar el permiso",
        ));
    };
    let pem = zeroize::Zeroizing::new(pem);
    Ok(Cliente::nuevo(Firmante::nuevo(&app, &pem)?)?)
}

fn rfc3339(s: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(s, &Rfc3339).ok()
}

fn dia(s: &str) -> Option<Date> {
    Date::parse(
        s.get(..10)?,
        time::macros::format_description!("[year]-[month]-[day]"),
    )
    .ok()
}

fn enmascarar(iban: &str) -> String {
    let i: Vec<char> = iban.chars().filter(|c| !c.is_whitespace()).collect();
    if i.len() <= 8 {
        return i.into_iter().collect();
    }
    let ini: String = i[..4].iter().collect();
    let fin: String = i[i.len() - 4..].iter().collect();
    format!("{ini} …{fin}")
}

/// Caduca las conexiones cuyo permiso ya ha vencido y borra las autorizaciones pendientes que
/// llevan más de una hora.
fn al_dia(v: &Vault, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let mut stmt = conn.prepare(
        "SELECT id, status, valid_until, created_at FROM bank_link WHERE status <> 'caducada'",
    )?;
    let filas: Vec<(i64, String, Option<String>, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<std::result::Result<_, _>>()?;
    for (id, estado, hasta, creada) in filas {
        if estado == "pendiente" {
            if rfc3339(&creada).is_none_or(|c| now - c > PENDIENTE_MAX) {
                conn.execute("DELETE FROM bank_link WHERE id = ?1", [id])?;
            }
        } else if hasta.as_deref().and_then(rfc3339).is_some_and(|h| h <= now) {
            conn.execute(
                "UPDATE bank_link SET status = 'caducada' WHERE id = ?1",
                [id],
            )?;
        }
    }
    Ok(())
}

pub fn estado(v: &Vault, now: OffsetDateTime) -> Result<Estado> {
    al_dia(v, now)?;
    let ajustes = ajustes(v)?;
    let conn = v.connection();
    let mut conexiones = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT id, aspsp, country, psu_type, status, valid_until, synced_at, last_error
         FROM bank_link WHERE status <> 'pendiente' ORDER BY aspsp, id",
    )?;
    type FilaConexion = (
        i64,
        String,
        String,
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    );
    let filas: Vec<FilaConexion> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
            ))
        })?
        .collect::<std::result::Result<_, _>>()?;
    let mut cuentas = conn.prepare(
        "SELECT l.id, l.name, l.iban, l.currency, l.cash_type, l.enabled, l.bank_account,
                b.account, l.synced_until, l.balance, l.balance_date
         FROM bank_link_account l LEFT JOIN bank_account b ON b.id = l.bank_account
         WHERE l.link_id = ?1 ORDER BY l.id",
    )?;
    for (id, banco, pais, tipo, estado, hasta, actualizada, error) in filas {
        let dias = hasta
            .as_deref()
            .and_then(rfc3339)
            .map(|h| (h - now).whole_days().max(0));
        let cs = cuentas
            .query_map([id], |r| {
                Ok(CuentaConectada {
                    id: r.get(0)?,
                    nombre: r.get(1)?,
                    iban: r.get::<_, Option<String>>(2)?.map(|i| enmascarar(&i)),
                    divisa: r.get(3)?,
                    tarjeta: r.get::<_, String>(4)?.eq_ignore_ascii_case("CARD"),
                    activa: r.get(5)?,
                    cuenta_id: r.get(6)?,
                    subcuenta: r.get::<_, Option<String>>(7)?.filter(|s| !s.is_empty()),
                    hasta_dia: r.get(8)?,
                    saldo: r.get::<_, Option<i64>>(9)?.map(Money::from_cents),
                    saldo_fecha: r.get(10)?,
                })
            })?
            .collect::<std::result::Result<_, _>>()?;
        conexiones.push(Conexion {
            id,
            banco,
            pais,
            tipo,
            estado,
            hasta,
            dias,
            actualizada,
            error,
            cuentas: cs,
        });
    }
    let pendiente = conn
        .query_row(
            "SELECT aspsp, psu_type, created_at FROM bank_link WHERE status = 'pendiente'
             ORDER BY id DESC LIMIT 1",
            [],
            |r| {
                Ok(Pendiente {
                    banco: r.get(0)?,
                    tipo: r.get(1)?,
                    desde: r.get(2)?,
                })
            },
        )
        .optional()?;
    Ok(Estado {
        vuelta_local: taxpain_banca::vuelta::local(&ajustes.url_vuelta).is_some(),
        app_id: secreto(v, SECRETO_APP)?,
        clave: secreto(v, SECRETO_CLAVE)?.is_some(),
        ajustes,
        conexiones,
        pendiente,
    })
}

/// De la lista de bancos de Enable Banking, los que se ofrecen ([`BANCOS`]).
pub fn admitidos(lista: Vec<Aspsp>) -> Vec<Aspsp> {
    let mut out: Vec<Aspsp> = lista
        .into_iter()
        .filter(|a| {
            BANCOS
                .iter()
                .any(|b| a.name.to_lowercase().starts_with(&b.to_lowercase()))
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Empieza una autorización: borra las pendientes que hubiera, apunta esta con un valor al
/// azar y devuelve la petición para Enable Banking. `maximo` es el permiso más largo que admite
/// el banco, en segundos.
pub fn preparar(
    v: &Vault,
    banco: &str,
    pais: &str,
    tipo: &str,
    maximo: Option<i64>,
    now: OffsetDateTime,
) -> Result<PeticionAuth> {
    if !matches!(tipo, "business" | "personal") {
        return Err(otro("tipo de cuenta no válido (empresa o particular)"));
    }
    if banco.trim().is_empty() || pais.trim().len() != 2 {
        return Err(otro("falta el banco"));
    }
    let ajustes = ajustes(v)?;
    comprobar_url_vuelta(&ajustes.url_vuelta)?;
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).map_err(|e| otro(e.to_string()))?;
    let state = hex::encode(b);
    let segundos = maximo
        .filter(|m| *m > 86_400)
        .map_or(PERMISO_DEFECTO_S, |m| m - 3_600)
        .min(PERMISO_MAX_S);
    let hasta = (now + Duration::seconds(segundos))
        .replace_nanosecond(0)
        .unwrap_or(now);
    let conn = v.connection();
    conn.execute("DELETE FROM bank_link WHERE status = 'pendiente'", [])?;
    conn.execute(
        "INSERT INTO bank_link(aspsp, country, psu_type, state, status, created_at)
         VALUES (?1, ?2, ?3, ?4, 'pendiente', ?5)",
        params![
            banco.trim(),
            pais.trim().to_uppercase(),
            tipo,
            state,
            now_str(now)
        ],
    )?;
    Ok(PeticionAuth {
        banco: banco.trim().to_owned(),
        pais: pais.trim().to_uppercase(),
        tipo: tipo.to_owned(),
        url_vuelta: ajustes.url_vuelta,
        state,
        hasta,
    })
}

/// El `state` de la autorización pendiente, si la hay (y no ha caducado).
pub fn state_pendiente(v: &Vault, now: OffsetDateTime) -> Result<Option<String>> {
    al_dia(v, now)?;
    Ok(v.connection()
        .query_row(
            "SELECT state FROM bank_link WHERE status = 'pendiente' ORDER BY id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?)
}

/// Olvida la autorización pendiente (el titular la ha cancelado o ha fallado).
pub fn descartar_pendiente(v: &Vault) -> Result<()> {
    v.connection()
        .execute("DELETE FROM bank_link WHERE status = 'pendiente'", [])?;
    Ok(())
}

/// La autorización pendiente a la que corresponde la vuelta del banco (por su `state`).
pub fn pendiente(v: &Vault, state: Option<&str>, now: OffsetDateTime) -> Result<i64> {
    al_dia(v, now)?;
    let Some(state) = state.filter(|s| !s.is_empty()) else {
        return Err(otro(
            "esa dirección no trae el «state» de la autorización: copia la dirección completa",
        ));
    };
    v.connection()
        .query_row(
            "SELECT id FROM bank_link WHERE status = 'pendiente' AND state = ?1",
            [state],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| {
            otro(
                "esa dirección no es la de la autorización que has empezado (o ha pasado más de \
                 una hora): vuelve a conectar el banco",
            )
        })
}

/// Guarda la sesión que ha abierto el banco con su código: la conexión pasa a activa con sus
/// cuentas. Si ya había otra conexión con el mismo banco y tipo, ésta la sustituye y conserva
/// lo de cada cuenta (si se descarga, en qué cuenta de TaxPain entra y hasta dónde se ha
/// bajado). Devuelve las sesiones sustituidas, para retirarlas también en Enable Banking.
pub fn guardar_sesion(
    v: &Vault,
    link: i64,
    s: &Sesion,
    now: OffsetDateTime,
) -> Result<Vec<String>> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let (aspsp, pais, tipo): (String, String, String) = tx
        .query_row(
            "SELECT aspsp, country, psu_type FROM bank_link WHERE id = ?1 AND status = 'pendiente'",
            [link],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| otro("ya no hay ninguna autorización pendiente: vuelve a conectar"))?;
    if s.accounts.is_empty() {
        return Err(otro(
            "el banco no ha autorizado ninguna cuenta: vuelve a conectar y elige las cuentas",
        ));
    }
    // Lo de las conexiones anteriores con el mismo banco y tipo.
    let mut stmt = tx.prepare(
        "SELECT id, session_id FROM bank_link
         WHERE id <> ?1 AND status <> 'pendiente' AND aspsp = ?2 AND country = ?3 AND psu_type = ?4",
    )?;
    let viejas: Vec<(i64, Option<String>)> = stmt
        .query_map(params![link, aspsp, pais, tipo], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    drop(stmt);
    type Previa = (
        Option<String>,
        Option<String>,
        String,
        bool,
        Option<i64>,
        Option<String>,
        Option<String>,
    );
    let mut previas: Vec<Previa> = Vec::new();
    for (id, _) in &viejas {
        let mut q = tx.prepare(
            "SELECT ident_hash, iban, currency, enabled, bank_account, synced_until, pending_since
             FROM bank_link_account WHERE link_id = ?1",
        )?;
        let filas: Vec<Previa> = q
            .query_map([id], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            })?
            .collect::<std::result::Result<_, _>>()?;
        previas.extend(filas);
    }
    tx.execute(
        "UPDATE bank_link SET session_id = ?1, valid_until = ?2, status = 'activa',
           last_error = NULL WHERE id = ?3",
        params![s.session_id, s.access.valid_until, link],
    )?;
    for c in &s.accounts {
        let iban = c.iban().map(|i| i.split_whitespace().collect::<String>());
        let divisa = c
            .currency
            .as_deref()
            .map(str::trim)
            .filter(|d| d.len() == 3)
            .unwrap_or("EUR")
            .to_uppercase();
        let previa = previas.iter().find(|p| {
            p.2 == divisa
                && ((p.0.is_some() && p.0 == c.identification_hash)
                    || (p.1.is_some() && p.1 == iban))
        });
        let nombre = c
            .name
            .clone()
            .or_else(|| c.product.clone())
            .unwrap_or_default();
        tx.execute(
            "INSERT INTO bank_link_account(link_id, uid, iban, name, currency, cash_type,
               ident_hash, card_last, enabled, bank_account, synced_until, pending_since)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                link,
                c.uid,
                iban,
                nombre.trim(),
                divisa,
                c.cash_account_type.clone().unwrap_or_default(),
                c.identification_hash,
                c.ultimos(),
                previa.map_or(iban.is_some(), |p| p.3),
                previa.and_then(|p| p.4),
                previa.and_then(|p| p.5.clone()),
                previa.and_then(|p| p.6.clone()),
            ],
        )?;
    }
    for (id, _) in &viejas {
        tx.execute("DELETE FROM bank_link WHERE id = ?1", [id])?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "banca.conectar",
        &aspsp,
        &serde_json::json!({
            "pais": pais,
            "tipo": tipo,
            "cuentas": s.accounts.len(),
            "hasta": s.access.valid_until,
            "sustituye": viejas.len(),
        }),
    )?;
    tx.commit()?;
    Ok(viejas.into_iter().filter_map(|(_, s)| s).collect())
}

/// Activa o desactiva la descarga de una cuenta.
pub fn activar_cuenta(v: &Vault, cuenta: i64, activa: bool) -> Result<()> {
    let n = v.connection().execute(
        "UPDATE bank_link_account SET enabled = ?1 WHERE id = ?2",
        params![activa, cuenta],
    )?;
    if n == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// La sesión de una conexión (para retirar el permiso en Enable Banking antes de quitarla).
pub fn sesion_de(v: &Vault, link: i64) -> Result<Option<String>> {
    v.connection()
        .query_row(
            "SELECT session_id FROM bank_link WHERE id = ?1",
            [link],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(AppError::NotFound)
}

/// Quita una conexión (los movimientos ya importados se quedan). Devuelve su sesión.
pub fn quitar(v: &Vault, link: i64, now: OffsetDateTime) -> Result<Option<String>> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let (banco, sesion): (String, Option<String>) = tx
        .query_row(
            "SELECT aspsp, session_id FROM bank_link WHERE id = ?1",
            [link],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    tx.execute("DELETE FROM bank_link WHERE id = ?1", [link])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "banca.desconectar",
        &banco,
        &serde_json::json!({ "conexion": link }),
    )?;
    tx.commit()?;
    Ok(sesion)
}

/// Una cuenta que toca descargar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Descarga {
    pub conexion: i64,
    pub cuenta: i64,
    pub uid: String,
    pub banco: String,
    pub iban: Option<String>,
    pub nombre: String,
    pub divisa: String,
    pub tarjeta: bool,
    pub ultimos: Option<String>,
    pub huella: Option<String>,
    /// Cuenta de TaxPain en la que entró la vez anterior.
    pub boveda: Option<i64>,
    /// Día de la última descarga.
    pub ultima: Option<Date>,
    /// Desde qué día se piden los movimientos (`None`: todo lo que dé el banco): diez días antes
    /// de la última descarga o del pendiente más antiguo que había entonces.
    pub desde: Option<Date>,
}

/// Las cuentas activas de las conexiones activas (de `conexion` o de todas).
pub fn descargas(v: &Vault, conexion: Option<i64>, now: OffsetDateTime) -> Result<Vec<Descarga>> {
    al_dia(v, now)?;
    let mut stmt = v.connection().prepare(
        "SELECT k.id, a.id, a.uid, k.aspsp, a.iban, a.name, a.currency, a.cash_type, a.card_last,
                a.ident_hash, a.bank_account, a.synced_until, a.pending_since
         FROM bank_link_account a JOIN bank_link k ON k.id = a.link_id
         WHERE k.status = 'activa' AND k.session_id IS NOT NULL AND a.enabled = 1
           AND (?1 IS NULL OR k.id = ?1)
         ORDER BY k.id, a.id",
    )?;
    let filas = stmt.query_map([conexion], |r| {
        let ultima = r.get::<_, Option<String>>(11)?.as_deref().and_then(dia);
        let pendiente = r.get::<_, Option<String>>(12)?.as_deref().and_then(dia);
        // Sin descarga anterior, todo lo que dé el banco (los pendientes no cuentan).
        let base = ultima.map(|u| pendiente.map_or(u, |p| u.min(p)));
        Ok(Descarga {
            conexion: r.get(0)?,
            cuenta: r.get(1)?,
            uid: r.get(2)?,
            banco: r.get(3)?,
            iban: r.get(4)?,
            nombre: r.get(5)?,
            divisa: r.get(6)?,
            tarjeta: r.get::<_, String>(7)?.eq_ignore_ascii_case("CARD"),
            ultimos: r.get(8)?,
            huella: r.get(9)?,
            boveda: r.get(10)?,
            ultima,
            desde: base.and_then(|d| d.checked_sub(Duration::days(SOLAPE_DIAS))),
        })
    })?;
    Ok(filas.collect::<std::result::Result<_, _>>()?)
}

/// Código de entidad de las tarjetas de un banco (el de sus extractos en fichero).
fn entidad(banco: &str) -> Option<&'static str> {
    banco.to_uppercase().starts_with("BBVA").then_some("0182")
}

/// Saldo contable que da el banco (el contabilizado antes que el disponible), en la divisa de
/// la cuenta.
fn saldo_contable(saldos: &[Saldo], divisa: &str) -> Option<(i64, Option<String>)> {
    const ORDEN: &[&str] = &["CLBD", "ITBD", "XPCD", "CLAV", "ITAV", "OPBD"];
    let validos: Vec<&Saldo> = saldos
        .iter()
        .filter(|s| s.balance_amount.currency.eq_ignore_ascii_case(divisa))
        .collect();
    let elegido = ORDEN
        .iter()
        .find_map(|t| {
            validos.iter().find(|s| {
                s.balance_type
                    .as_deref()
                    .is_some_and(|b| b.eq_ignore_ascii_case(t))
            })
        })
        .or(validos.first())?;
    let c = taxpain_banca::enable::centimos(&elegido.balance_amount.amount)?;
    Some((c, elegido.reference_date.clone()))
}

/// Importa los movimientos bajados de una cuenta como un extracto (sólo los contabilizados en
/// la divisa de la cuenta) y apunta hasta dónde se ha bajado y el saldo del banco.
pub fn importar_cuenta(
    v: &Vault,
    d: &Descarga,
    txs: &[Transaccion],
    saldos: &[Saldo],
    now: OffsetDateTime,
) -> Result<ExtractoImportado> {
    let hoy = taxpain_core::spain_time::peninsular_date(now);
    let mut e = Extracto::nuevo("Conexión con el banco");
    e.banco = d.banco.clone();
    match (&d.iban, &d.ultimos) {
        (Some(iban), _) => e.con_iban(iban),
        (None, Some(u)) if d.tarjeta => e.con_tarjeta(entidad(&d.banco), u),
        _ => {
            e.cuenta = format!(
                "enable:{}",
                d.huella.clone().unwrap_or_else(|| d.uid.clone())
            )
        }
    }
    e.titular = d.nombre.clone();
    e.divisa = d.divisa.clone();
    e.clase = if d.tarjeta {
        Clase::TarjetaCredito
    } else {
        Clase::Cuenta
    };
    let (mut otra_divisa, mut sin_dato) = (0usize, 0usize);
    // El pendiente más antiguo (retenciones de tarjeta que se contabilizan días después): la
    // descarga siguiente pide desde antes de él.
    let pendiente = txs
        .iter()
        .filter(|t| {
            t.status
                .as_deref()
                .is_some_and(|s| s.eq_ignore_ascii_case("PDNG") || s.eq_ignore_ascii_case("HOLD"))
        })
        .filter_map(|t| {
            [&t.transaction_date, &t.value_date, &t.booking_date]
                .into_iter()
                .find_map(|f| f.as_deref().and_then(dia))
        })
        .min();
    for t in txs.iter().filter(|t| t.contabilizado()) {
        if !t
            .transaction_amount
            .currency
            .eq_ignore_ascii_case(&d.divisa)
        {
            otra_divisa += 1;
            continue;
        }
        let fecha = [&t.booking_date, &t.transaction_date, &t.value_date]
            .into_iter()
            .find_map(|f| f.as_deref().and_then(dia));
        let (Some(fecha), Some(c)) = (fecha, t.centimos()) else {
            sin_dato += 1;
            continue;
        };
        e.movimientos.push(Movimiento {
            importe_eur: None,
            fecha,
            fecha_valor: t.value_date.as_deref().and_then(dia),
            concepto: t.concepto(),
            importe: Money::from_cents(c),
            saldo: t.saldo(&d.divisa).map(Money::from_cents),
            referencia: t.id().unwrap_or_default().to_owned(),
        });
    }
    // El orden lo pone el importador (también si el banco los da del más nuevo al más antiguo).
    if otra_divisa > 0 {
        e.avisos.push(if otra_divisa == 1 {
            format!(
                "1 movimiento en otra divisa que la de la cuenta ({}) no se ha importado.",
                d.divisa
            )
        } else {
            format!(
                "{otra_divisa} movimientos en otra divisa que la de la cuenta ({}) no se han \
                 importado.",
                d.divisa
            )
        });
    }
    if sin_dato > 0 {
        e.avisos.push(if sin_dato == 1 {
            "1 movimiento sin fecha o sin importe no se ha importado.".to_owned()
        } else {
            format!("{sin_dato} movimientos sin fecha o sin importe no se han importado.")
        });
    }
    let leido = importar::comprobado(e);
    let entradas = [Entrada {
        leido: &leido,
        codigos: None,
        ids_externos: true,
    }];
    let nombre = format!("{} (conexión)", d.banco);
    let bytes = serde_json::to_vec(txs).unwrap_or_default();
    // Lo que da el banco por su conexión entra aunque sus saldos no encadenen (con el aviso).
    let mut resumen =
        importar::importar_entradas(v, &nombre, &bytes, &entradas, d.boveda, true, now)?;
    let mut r = resumen
        .extractos
        .pop()
        .ok_or_else(|| otro("no se ha importado nada"))?;
    r.avisos.retain(|a| a != AVISO_SIN_SALDOS);
    let saldo = saldo_contable(saldos, &d.divisa);
    let conn = v.connection();
    if r.importado || leido.extracto.movimientos.is_empty() {
        conn.execute(
            "UPDATE bank_link_account SET synced_until = ?1, pending_since = ?2,
               bank_account = coalesce(?3, bank_account) WHERE id = ?4",
            params![
                hoy.to_string(),
                pendiente.map(|p| p.to_string()),
                r.cuenta_id,
                d.cuenta
            ],
        )?;
    }
    if let Some((c, fecha)) = saldo {
        conn.execute(
            "UPDATE bank_link_account SET balance = ?1, balance_date = ?2 WHERE id = ?3",
            params![c, fecha.unwrap_or_else(|| hoy.to_string()), d.cuenta],
        )?;
    }
    Ok(r)
}

/// Apunta el error de la última descarga de una conexión; si el permiso ha caducado o se ha
/// retirado, la conexión pasa a caducada.
pub fn marcar_error(v: &Vault, conexion: i64, e: &BancaError) -> Result<()> {
    let conn = v.connection();
    conn.execute(
        "UPDATE bank_link SET last_error = ?1 WHERE id = ?2",
        params![e.to_string(), conexion],
    )?;
    if matches!(e, BancaError::Caducado) {
        conn.execute(
            "UPDATE bank_link SET status = 'caducada' WHERE id = ?1",
            [conexion],
        )?;
    }
    Ok(())
}

fn marcar_bien(v: &Vault, conexion: i64, now: OffsetDateTime) -> Result<()> {
    v.connection().execute(
        "UPDATE bank_link SET synced_at = ?1, last_error = NULL WHERE id = ?2",
        params![now_str(now), conexion],
    )?;
    Ok(())
}

/// Lo que pide [`actualizar`] al banco (en la app, [`Cliente`]; en las pruebas, datos fijos).
pub trait Api {
    fn movimientos(
        &self,
        uid: &str,
        desde: Option<Date>,
    ) -> impl Future<Output = taxpain_banca::Result<Vec<Transaccion>>> + Send;
    fn saldos(&self, uid: &str) -> impl Future<Output = taxpain_banca::Result<Vec<Saldo>>> + Send;
}

impl Api for Cliente {
    fn movimientos(
        &self,
        uid: &str,
        desde: Option<Date>,
    ) -> impl Future<Output = taxpain_banca::Result<Vec<Transaccion>>> + Send {
        Cliente::movimientos(self, uid, desde)
    }

    fn saldos(&self, uid: &str) -> impl Future<Output = taxpain_banca::Result<Vec<Saldo>>> + Send {
        Cliente::saldos(self, uid)
    }
}

/// Lo que ha pasado con una cuenta al actualizar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CuentaActualizada {
    pub conexion: i64,
    pub banco: String,
    /// Nombre de la cuenta en el banco.
    pub nombre: String,
    pub resultado: Option<ExtractoImportado>,
    pub error: Option<String>,
}

/// Resultado de [`actualizar`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Actualizacion {
    pub cuentas: Vec<CuentaActualizada>,
    pub movimientos_nuevos: usize,
    pub ya_estaban: usize,
}

/// Baja e importa los movimientos de las cuentas activas (de `conexion` o de todas). Coge el
/// almacén con `vault` sólo para leer y para guardar, nunca mientras espera al banco. Una
/// cuenta que falla no para las demás; un permiso caducado para las de su conexión.
pub async fn actualizar<G: Deref<Target = Vault>>(
    vault: impl Fn() -> Result<G>,
    api: &impl Api,
    conexion: Option<i64>,
    now: OffsetDateTime,
) -> Result<Actualizacion> {
    let pendientes = descargas(&*vault()?, conexion, now)?;
    let mut out = Actualizacion::default();
    let mut caducadas: BTreeSet<i64> = BTreeSet::new();
    let mut con_error: BTreeSet<i64> = BTreeSet::new();
    let mut hechas: BTreeSet<i64> = BTreeSet::new();
    for d in pendientes {
        if caducadas.contains(&d.conexion) {
            continue;
        }
        // El saldo, sólo en la primera descarga del día: cada petición cuenta para el límite
        // de consultas del banco.
        let hoy = taxpain_core::spain_time::peninsular_date(now);
        let bajado = match api.movimientos(&d.uid, d.desde).await {
            Ok(txs) if d.ultima == Some(hoy) => Ok((txs, Vec::new())),
            Ok(txs) => Ok((txs, api.saldos(&d.uid).await.unwrap_or_default())),
            Err(e) => Err(e),
        };
        let v = vault()?;
        let mut fila = CuentaActualizada {
            conexion: d.conexion,
            banco: d.banco.clone(),
            nombre: d
                .iban
                .as_deref()
                .map(enmascarar)
                .unwrap_or_else(|| d.nombre.clone()),
            resultado: None,
            error: None,
        };
        match bajado {
            // Una cuenta que no se puede importar no para las demás.
            Ok((txs, saldos)) => match importar_cuenta(&v, &d, &txs, &saldos, now) {
                Ok(r) => {
                    out.movimientos_nuevos += r.movimientos_nuevos;
                    out.ya_estaban += r.ya_estaban;
                    fila.resultado = Some(r);
                    hechas.insert(d.conexion);
                }
                Err(e) => {
                    con_error.insert(d.conexion);
                    fila.error = Some(format!("no se ha podido importar: {e}"));
                }
            },
            Err(e) => {
                marcar_error(&v, d.conexion, &e)?;
                if matches!(e, BancaError::Caducado) {
                    caducadas.insert(d.conexion);
                }
                con_error.insert(d.conexion);
                fila.error = Some(e.to_string());
            }
        }
        out.cuentas.push(fila);
    }
    let v = vault()?;
    for c in hechas.difference(&con_error) {
        marcar_bien(&v, *c, now)?;
    }
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        "banca.actualizar",
        "movimientos",
        &serde_json::json!({
            "cuentas": out.cuentas.len(),
            "nuevos": out.movimientos_nuevos,
            "ya_estaban": out.ya_estaban,
            "errores": out.cuentas.iter().filter(|c| c.error.is_some()).count(),
        }),
    )?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direcciones_de_vuelta() {
        assert!(comprobar_url_vuelta(URL_VUELTA).is_ok());
        assert!(comprobar_url_vuelta("http://127.0.0.1:47219/taxpain/banco").is_ok());
        assert!(comprobar_url_vuelta("http://localhost:47219/taxpain/banco").is_err());
        assert!(comprobar_url_vuelta("http://ejemplo.es/vuelta").is_err());
        assert!(comprobar_url_vuelta("https://localhost/taxpain/banco?x=1").is_err());
        assert!(comprobar_url_vuelta("javascript:alert(1)").is_err());
        assert!(comprobar_url_vuelta("").is_err());
    }

    #[test]
    fn iban_enmascarado() {
        assert_eq!(enmascarar("ES9121000418450200051332"), "ES91 …1332");
        assert_eq!(enmascarar("ES91 2100 0418 4502 0005 1332"), "ES91 …1332");
        // Sin romper con caracteres que no son ASCII.
        assert_eq!(enmascarar("ÉS91ñ2100041845020005133Ü"), "ÉS91 …133Ü");
    }

    #[test]
    fn saldo_contable_antes_que_el_disponible() {
        let s = |t: &str, a: &str| Saldo {
            name: None,
            balance_amount: taxpain_banca::enable::Importe {
                currency: "EUR".into(),
                amount: a.into(),
            },
            balance_type: Some(t.into()),
            reference_date: Some("2026-10-30".into()),
        };
        assert_eq!(
            saldo_contable(&[s("ITAV", "90.00"), s("CLBD", "100.00")], "EUR"),
            Some((10_000, Some("2026-10-30".into())))
        );
        assert_eq!(
            saldo_contable(&[s("OTHR", "5.00")], "EUR").map(|x| x.0),
            Some(500)
        );
        assert_eq!(saldo_contable(&[s("CLBD", "1.00")], "USD"), None);
    }

    #[test]
    fn solo_bbva_y_revolut() {
        let a = |n: &str| Aspsp {
            name: n.into(),
            country: "ES".into(),
            psu_types: vec!["business".into()],
            maximum_consent_validity: None,
            beta: false,
        };
        let l = admitidos(vec![
            a("Santander"),
            a("Revolut"),
            a("BBVA"),
            a("CaixaBank"),
        ]);
        assert_eq!(
            l.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(),
            ["BBVA", "Revolut"]
        );
    }
}
