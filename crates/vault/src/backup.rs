//! Copias de seguridad cifradas de una empresa, para restaurarlas en este Mac o en otro.
//!
//! La BD de cada empresa está cifrada con la clave `db-master` del Keychain, que no sale del
//! Mac: una copia del fichero (Time Machine, una carpeta sincronizada) sólo se abre en otro
//! equipo con el código de recuperación y la contraseña de las copias ([`crate::recovery`]). La
//! copia de TaxPain es un fichero `.taxpain` que no depende del Keychain:
//!
//! - `TPBACKUP` ‖ longitud de la cabecera (u32 LE) ‖ cabecera JSON ‖ BD SQLCipher.
//! - La BD de la copia (`sqlcipher_export`) va cifrada con una clave aleatoria nueva en cada
//!   copia. Esa clave va sellada (AES-256-GCM, [`crate::seal`]) en la cabecera con otra que sale
//!   de la contraseña de las copias (scrypt con sal aleatoria).
//! - La cabecera no lleva datos de la empresa: versión, fecha, parámetros y la huella SHA-256 de
//!   la BD (para distinguir un fichero dañado de una contraseña equivocada).
//!
//! La contraseña no se guarda. Se guarda la clave derivada (en la propia BD cifrada, tabla
//! `backup_key`) para hacer copias, también automáticas, sin pedirla cada vez: quien puede leer
//! esa tabla ya puede leer todos los datos de la empresa.
//!
//! No van en la copia los certificados (sellados con una clave del Keychain que exige Touch ID y
//! no sale del Mac: se vuelven a importar tras restaurar) ni la propia clave de las copias.
//!
//! La misma clave derivada sella `db-master` en el código de recuperación que se guarda junto a la
//! BD ([`crate::recovery`]): se escribe al definir o cambiar la contraseña.
//!
//! Una copia puede venir de otra persona (la gestoría, un socio, alguien que quiere colar algo):
//! al restaurarla no se hereda su contraseña (la empresa restaurada queda sin contraseña de
//! copias y la app pide una nueva: `db-master` es la misma para todas las empresas del Mac y no
//! se sella con una contraseña que eligió otro), se vuelven a vaciar las tablas que no viajan en
//! las copias y no se admite un esquema con triggers, vistas o índices que no sean los de
//! TaxPain (ver [`restore`]).

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;
use zeroize::Zeroizing;

use crate::audit::{self, Actor};
use crate::error::{Result, VaultError};
use crate::{Vault, db, recovery, seal, vault};
use taxpain_secrets::{Key256, KeyId, KeyProvider, UserPresence};

const MAGIC: &[u8; 8] = b"TPBACKUP";
const FORMAT: u32 = 1;
const SEAL_CONTEXT: &[u8] = b"taxpain-backup-v1";
/// Nombre con el que se adjunta la BD de la copia.
const SCHEMA: &str = "taxpain_backup";
/// Longitud mínima de la contraseña de las copias.
pub const MIN_PASSPHRASE: usize = 10;
/// Caracteres distintos mínimos de la contraseña (sin contar mayúsculas).
const MIN_DISTINCT: usize = 5;
/// Extensión de los ficheros de copia.
pub const EXTENSION: &str = "taxpain";
/// Memoria máxima que puede pedir scrypt al abrir una copia (256 MiB).
const MAX_KDF_MEMORY: u64 = 256 * 1024 * 1024;
/// Longitud máxima de la cabecera JSON.
const MAX_HEADER: usize = 64 * 1024;

/// Parámetros de scrypt. Los de la copia van en su cabecera.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    pub log_n: u8,
    pub r: u32,
    pub p: u32,
}

impl KdfParams {
    /// Los recomendados por scrypt (128 MiB, menos de un segundo en un Mac).
    pub const DEFAULT: KdfParams = KdfParams {
        log_n: 17,
        r: 8,
        p: 1,
    };

    /// Límites al leer una cabecera: un fichero manipulado no puede pedir gigas de memoria
    /// (scrypt usa `128·r·N` bytes; se admite hasta el doble de los recomendados) ni minutos.
    fn check(self) -> Result<scrypt::Params> {
        let memory = u64::from(self.r) << (u32::from(self.log_n.min(20)) + 7);
        if !(10..=20).contains(&self.log_n)
            || !(1..=16).contains(&self.r)
            || !(1..=2).contains(&self.p)
            || memory > MAX_KDF_MEMORY
        {
            return Err(VaultError::InvalidInput(
                "parámetros de la copia no válidos".into(),
            ));
        }
        scrypt::Params::new(self.log_n, self.r, self.p)
            .map_err(|_| VaultError::InvalidInput("parámetros de la copia no válidos".into()))
    }
}

/// Cabecera de una copia (en claro: no lleva datos de la empresa).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupHeader {
    pub format: u32,
    /// Fecha y hora de la copia (RFC 3339, UTC).
    pub created_at: String,
    /// Versión de TaxPain que la hizo.
    pub app_version: String,
    /// Versión del esquema de la BD.
    pub schema_version: i64,
    pub kdf: KdfParams,
    /// Sal de scrypt (hex).
    salt: String,
    /// Clave de la BD de la copia, sellada con la clave derivada de la contraseña (hex).
    key: String,
    /// SHA-256 de la BD de la copia (hex).
    pub sha256: String,
}

/// Resultado de hacer una copia.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BackupReport {
    pub path: PathBuf,
    pub bytes: u64,
    pub created_at: String,
    pub sha256: String,
    /// Entradas de la auditoría en la copia (comprobada al terminar).
    pub audit_entries: i64,
}

/// Clave derivada de la contraseña de las copias.
pub(crate) struct BackupKey {
    pub(crate) kek: Key256,
    pub(crate) salt: [u8; 16],
    pub(crate) params: KdfParams,
}

fn now_str(now: OffsetDateTime) -> String {
    now.format(&Rfc3339).unwrap_or_default()
}

pub(crate) fn derive(passphrase: &str, salt: &[u8], params: KdfParams) -> Result<Key256> {
    let p = params.check()?;
    let mut out = Zeroizing::new([0u8; 32]);
    scrypt::scrypt(passphrase.as_bytes(), salt, &p, out.as_mut())
        .map_err(|_| VaultError::InvalidInput("parámetros de la copia no válidos".into()))?;
    Ok(Key256::from_bytes(out.as_ref())?)
}

/// Comprueba que la contraseña sirve para las copias.
pub fn check_passphrase(passphrase: &str) -> Result<()> {
    if passphrase.chars().count() < MIN_PASSPHRASE {
        return Err(VaultError::InvalidInput(format!(
            "la contraseña de las copias ha de tener al menos {MIN_PASSPHRASE} caracteres"
        )));
    }
    if passphrase.trim() != passphrase {
        return Err(VaultError::InvalidInput(
            "la contraseña de las copias no puede empezar ni acabar con espacios".into(),
        ));
    }
    // «aaaaaaaaaa», «1212121212», «0123456789»…: largas pero fáciles de adivinar.
    let mut distinct: Vec<char> = passphrase.to_lowercase().chars().collect();
    distinct.sort_unstable();
    distinct.dedup();
    if distinct.len() < MIN_DISTINCT || is_sequence(passphrase) || is_common(passphrase) {
        return Err(VaultError::InvalidInput(
            "la contraseña de las copias es demasiado fácil de adivinar: usa una frase de varias \
             palabras"
                .into(),
        ));
    }
    Ok(())
}

/// Filas del teclado y series que la gente escribe de corrido.
const SEQUENCES: [&str; 6] = [
    "0123456789",
    "abcdefghijklmnopqrstuvwxyz",
    "abcdefghijklmnñopqrstuvwxyz",
    "qwertyuiop",
    "asdfghjklñ",
    "zxcvbnm",
];

/// ¿Es un trozo seguido de una serie (en cualquier sentido y dando la vuelta: «1234567890»,
/// «0987654321», «qwertyuiop»)?
fn is_sequence(passphrase: &str) -> bool {
    let p: String = passphrase.to_lowercase();
    SEQUENCES.iter().any(|seq| {
        let fwd = seq.repeat(1 + p.chars().count() / seq.chars().count() + 1);
        let back: String = fwd.chars().rev().collect();
        fwd.contains(&p) || back.contains(&p)
    })
}

/// Contraseñas típicas (con o sin números detrás: «password123», «contraseña2026»).
const COMMON: [&str; 8] = [
    "password",
    "contraseña",
    "contrasena",
    "qwerty",
    "iloveyou",
    "tequiero",
    "taxpain",
    "admin",
];

fn is_common(passphrase: &str) -> bool {
    let p = passphrase.to_lowercase();
    let word = p.trim_end_matches(|c: char| !c.is_alphabetic());
    COMMON.contains(&word)
}

/// Fila de `backup_key`: clave, sal y parámetros de scrypt.
type KeyRow = (Vec<u8>, Vec<u8>, i64, i64, i64);

fn load_key(conn: &Connection) -> Result<Option<BackupKey>> {
    let row: Option<KeyRow> = conn
        .query_row(
            "SELECT kek, salt, log_n, r, p FROM backup_key WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;
    let Some((kek, salt, log_n, r, p)) = row else {
        return Ok(None);
    };
    let kek = Zeroizing::new(kek);
    Ok(Some(BackupKey {
        kek: Key256::from_bytes(&kek)?,
        salt: salt.try_into().map_err(|_| VaultError::Corrupted)?,
        params: KdfParams {
            log_n: u8::try_from(log_n).map_err(|_| VaultError::Corrupted)?,
            r: u32::try_from(r).map_err(|_| VaultError::Corrupted)?,
            p: u32::try_from(p).map_err(|_| VaultError::Corrupted)?,
        },
    }))
}

fn store_key(conn: &Connection, k: &BackupKey, now: OffsetDateTime) -> Result<()> {
    conn.execute(
        "INSERT INTO backup_key(id, kek, salt, log_n, r, p, created_at)
         VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(id) DO UPDATE SET kek = excluded.kek, salt = excluded.salt,
           log_n = excluded.log_n, r = excluded.r, p = excluded.p, created_at = excluded.created_at",
        params![
            k.kek.expose().as_slice(),
            k.salt.as_slice(),
            k.params.log_n,
            k.params.r,
            k.params.p,
            now_str(now)
        ],
    )?;
    Ok(())
}

/// Define (o cambia) la contraseña de las copias de esta empresa. Las copias anteriores siguen
/// abriéndose con la contraseña con la que se hicieron.
pub fn set_passphrase(v: &Vault, passphrase: &str, now: OffsetDateTime) -> Result<()> {
    set_passphrase_with(v, passphrase, KdfParams::DEFAULT, now)
}

/// Como [`set_passphrase`] con otros parámetros de scrypt (tests).
pub fn set_passphrase_with(
    v: &Vault,
    passphrase: &str,
    params: KdfParams,
    now: OffsetDateTime,
) -> Result<()> {
    check_passphrase(passphrase)?;
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|e| VaultError::KeyUnavailable(e.to_string()))?;
    let kek = derive(passphrase, &salt, params)?;
    let conn = v.connection();
    let replaced = load_key(conn)?.is_some();
    let key = BackupKey { kek, salt, params };
    store_key(conn, &key, now)?;
    write_recovery(v, &key)?;
    audit::append(
        conn,
        &now_str(now),
        Actor::User,
        if replaced {
            "backup.passphrase_changed"
        } else {
            "backup.passphrase_set"
        },
        "copias",
        &json!({}),
    )?;
    Ok(())
}

/// ¿Tiene esta empresa contraseña de copias?
pub fn has_passphrase(v: &Vault) -> Result<bool> {
    Ok(load_key(v.connection())?.is_some())
}

/// Escribe el código de recuperación ([`crate::recovery`]) de un almacén en disco: `db-master`
/// sellada con `key`. En memoria no hay nada que recuperar.
fn write_recovery(v: &Vault, key: &BackupKey) -> Result<()> {
    let Some(dir) = v.dir() else {
        return Ok(());
    };
    let db_key = v
        .keys()
        .get_or_create(KeyId::DbMaster, "Abrir los datos de TaxPain")?;
    recovery::write(dir, key, &db_key)
}

/// Si la empresa tiene contraseña de copias y le falta el código de recuperación (datos de una
/// versión anterior, fichero borrado), lo escribe. Se llama al abrir, con la clave ya en mano.
pub(crate) fn ensure_recovery(v: &Vault, db_key: &Key256) -> Result<()> {
    let Some(dir) = v.dir() else {
        return Ok(());
    };
    if recovery::exists(dir) {
        return Ok(());
    }
    match load_key(v.connection())? {
        Some(key) => recovery::write(dir, &key, db_key),
        None => Ok(()),
    }
}

/// Comprueba que `passphrase` es la contraseña de las copias (para que el usuario se asegure de
/// que la recuerda).
pub fn verify_passphrase(v: &Vault, passphrase: &str) -> Result<bool> {
    let Some(k) = load_key(v.connection())? else {
        return Ok(false);
    };
    let other = derive(passphrase, &k.salt, k.params)?;
    Ok(other.expose() == k.kek.expose())
}

/// Ficheros temporales que se borran al salir (también si algo falla).
struct Cleanup(Vec<PathBuf>);

impl Drop for Cleanup {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// `dir/.nombre<suffix>`: temporal junto al destino (el renombrado final es atómico). Sólo para
/// el fichero final de la copia, ya sin secretos.
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "copia".into());
    path.with_file_name(format!(".{name}{suffix}"))
}

fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| VaultError::InvalidInput("la ruta no es válida".into()))
}

/// Adjunta `path` como [`SCHEMA`] con la clave bruta `key`.
fn attach(conn: &Connection, path: &Path, key: &Key256) -> Result<()> {
    let raw = Zeroizing::new(format!("x'{}'", key.to_hex().as_str()));
    conn.execute(
        &format!("ATTACH DATABASE ?1 AS {SCHEMA} KEY ?2"),
        params![path_str(path)?, raw.as_str()],
    )?;
    Ok(())
}

/// Adjunta la BD de una copia, que puede venir de otra persona, y comprueba que está entera
/// (`quick_check`) antes de leer nada de ella. La conexión ha de estar endurecida
/// ([`db::harden`]). Con la clave buena SQLCipher siempre abre: cualquier fallo es que la copia
/// está mal.
fn attach_untrusted(conn: &Connection, path: &Path, key: &Key256) -> Result<()> {
    attach(conn, path, key)
        .and_then(|()| {
            let ok: String =
                conn.query_row(&format!("PRAGMA {SCHEMA}.quick_check(1)"), [], |r| r.get(0))?;
            if ok == "ok" {
                Ok(())
            } else {
                Err(VaultError::Corrupted)
            }
        })
        .map_err(|e| match e {
            VaultError::Db(_) => VaultError::Corrupted,
            e => e,
        })
}

fn not_ours() -> VaultError {
    VaultError::InvalidInput(
        "la copia contiene elementos que no crea TaxPain (triggers, vistas, índices o tablas \
         virtuales): no se restaura"
            .into(),
    )
}

/// SQL sin diferencias de espacios ni saltos de línea.
fn normalize_sql(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Índices de una copia que se rehacen tras restaurarla con la definición de esta compilación.
#[derive(Debug, Default, PartialEq, Eq)]
struct IndexRepairs {
    /// Índices cuya definición no es la de TaxPain: (nombre, `CREATE INDEX` de TaxPain). Se
    /// quitan y se vuelven a crear; si no se puede, la copia no se restaura.
    replace: Vec<(String, String)>,
    /// `CREATE INDEX` de los índices de TaxPain que la copia no trae. Se intentan crear; si no
    /// se puede (al almacén de origen ya le faltaba, y quizá su tabla), se restaura sin ellos:
    /// que falte un índice no es un riesgo, y la copia queda como estaba el origen.
    missing: Vec<String>,
}

/// Compara el esquema de la copia adjunta con el que dejan las migraciones de esta compilación
/// en una BD nueva hasta la versión `version` de la copia:
///
/// - **Triggers y vistas**: han de ser exactamente los de TaxPain (mismo nombre, tabla y SQL,
///   salvo espacios). Un trigger de más o cambiado ejecutaría SQL de quien hizo la copia cada vez
///   que la app escribe; uno de menos quitaría una protección (auditoría de sólo anexado,
///   ejercicios cerrados). `sqlcipher_export` los copia tal cual y SQLite guarda su texto tal
///   como se escribió: en una copia legítima coinciden.
/// - **Índices**: sólo los de TaxPain (por nombre y tabla); uno de más se rechaza. Si la
///   definición de uno no coincide (un almacén de una versión de desarrollo anterior) o falta
///   alguno, no se rechaza: se rehace con la de TaxPain tras restaurar ([`IndexRepairs`]).
/// - **Tablas**: no se admiten tablas virtuales. La definición de las demás no se compara (el
///   orden de las columnas y los comentarios pueden variar entre almacenes legítimos según por
///   qué versiones pasaron): lo que pueda llevar una tabla (CHECK, DEFAULT, columnas generadas)
///   queda limitado por `trusted_schema = OFF`.
fn check_schema(conn: &Connection, version: usize) -> Result<IndexRepairs> {
    let clean = db::clean_schema(version)?;
    let copy = db::schema_objects(conn, SCHEMA)?;
    let strict = |objs: &[db::SchemaObject]| -> BTreeSet<(String, String, String, String)> {
        objs.iter()
            .filter(|o| o.kind == "trigger" || o.kind == "view")
            .map(|o| {
                (
                    o.kind.clone(),
                    o.name.clone(),
                    o.table.clone(),
                    normalize_sql(o.sql.as_deref().unwrap_or_default()),
                )
            })
            .collect()
    };
    if strict(&clean) != strict(&copy) {
        return Err(not_ours());
    }
    // Los índices con SQL (los automáticos de UNIQUE y PRIMARY KEY no lo llevan).
    let indexes = |objs: &[db::SchemaObject]| -> BTreeMap<String, (String, String)> {
        objs.iter()
            .filter(|o| o.kind == "index")
            .filter_map(|o| Some((o.name.clone(), (o.table.clone(), o.sql.clone()?))))
            .collect()
    };
    let (ours, theirs) = (indexes(&clean), indexes(&copy));
    let mut repairs = IndexRepairs::default();
    for (name, (table, sql)) in &theirs {
        match ours.get(name) {
            Some((t, s)) if t == table => {
                if normalize_sql(s) != normalize_sql(sql) {
                    repairs.replace.push((name.clone(), s.clone()));
                }
            }
            _ => return Err(not_ours()),
        }
    }
    for (name, (_, sql)) in &ours {
        if !theirs.contains_key(name) {
            repairs.missing.push(sql.clone());
        }
    }
    for o in &copy {
        match o.kind.as_str() {
            "table" => {
                let sql = normalize_sql(o.sql.as_deref().unwrap_or_default()).to_ascii_uppercase();
                if o.rootpage <= 0 || !sql.starts_with("CREATE TABLE ") {
                    return Err(not_ours());
                }
            }
            "index" | "trigger" | "view" => {}
            _ => return Err(not_ours()),
        }
    }
    Ok(repairs)
}

/// Ejecuta `f` con las claves ajenas desactivadas (`sqlcipher_export` copia las tablas en el
/// orden del esquema, no en el de sus dependencias) y las vuelve a activar pase lo que pase.
fn without_foreign_keys<T>(conn: &Connection, f: impl FnOnce() -> Result<T>) -> Result<T> {
    conn.execute_batch("PRAGMA foreign_keys = OFF")?;
    let r = f();
    let on = conn.execute_batch("PRAGMA foreign_keys = ON");
    let out = r?;
    on?;
    Ok(out)
}

/// Exporta la BD de `conn` (esquema `main`) a `path`, cifrada con `key`, sin certificados, clave
/// de copias ni secretos de la app. Devuelve la versión del esquema y las entradas de auditoría
/// que había al copiar (contadas en la misma lectura que la copia: si otro proceso escribe a la
/// vez, no descuadra la comprobación).
fn export_db(conn: &Connection, path: &Path, key: &Key256) -> Result<(i64, i64)> {
    let version = i64::try_from(db::user_version(conn)?).map_err(|_| VaultError::Corrupted)?;
    without_foreign_keys(conn, || {
        attach(conn, path, key)?;
        let r = (|| -> Result<i64> {
            conn.execute_batch("BEGIN")?;
            let copied = (|| -> Result<i64> {
                conn.query_row(&format!("SELECT sqlcipher_export('{SCHEMA}')"), [], |_| {
                    Ok(())
                })?;
                Ok(conn.query_row("SELECT count(*) FROM main.audit_log", [], |r| r.get(0))?)
            })();
            match copied {
                Ok(n) => {
                    conn.execute_batch("COMMIT")?;
                    Ok(n)
                }
                Err(e) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    Err(e)
                }
            }
        })()
        .and_then(|expected| {
            conn.pragma_update(Some(SCHEMA), "secure_delete", true)?;
            // Siempre con el esquema delante: sin él se borrarían los certificados del Mac.
            // Los certificados no salen del Mac; la clave de API del asistente tampoco viaja en
            // las copias (se vuelve a poner tras restaurar).
            conn.execute_batch(&format!(
                "DELETE FROM {SCHEMA}.certificate_secret;
                 DELETE FROM {SCHEMA}.certificate_ref;
                 DELETE FROM {SCHEMA}.backup_key;
                 DELETE FROM {SCHEMA}.app_secret;"
            ))?;
            conn.pragma_update(Some(SCHEMA), "user_version", version)?;
            conn.execute_batch(&format!("VACUUM {SCHEMA}"))?;
            Ok(expected)
        });
        let detached = conn.execute_batch(&format!("DETACH DATABASE {SCHEMA}"));
        let expected = r?;
        detached?;
        Ok((version, expected))
    })
}

/// SHA-256 (hex) de un fichero, leyéndolo por partes.
fn sha256_file(path: &Path) -> Result<String> {
    let mut f = BufReader::new(std::fs::File::open(path)?);
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        let Some(chunk) = buf.get(..n).filter(|c| !c.is_empty()) else {
            break;
        };
        hasher.update(chunk);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Comprueba una BD de copia ya escrita (`path`, cifrada con `key`): se abre, pasa
/// `quick_check` y tiene `expected` entradas de auditoría. Sin cargarla en memoria.
fn verify_db(path: &Path, key: &Key256, expected: i64) -> Result<()> {
    let conn = db::open_in_memory_unmigrated(&Key256::random()?)?;
    db::harden(&conn)?;
    attach(&conn, path, key)?;
    let checked = (|| -> Result<(i64, String)> {
        let n: i64 = conn.query_row(
            &format!("SELECT count(*) FROM {SCHEMA}.audit_log"),
            [],
            |r| r.get(0),
        )?;
        let ok: String =
            conn.query_row(&format!("PRAGMA {SCHEMA}.quick_check"), [], |r| r.get(0))?;
        Ok((n, ok))
    })();
    let _ = conn.execute_batch(&format!("DETACH DATABASE {SCHEMA}"));
    match checked {
        Ok((n, ok)) if n == expected && ok == "ok" => Ok(()),
        _ => Err(VaultError::InvalidInput(
            "la copia no ha salido bien: vuelve a intentarlo".into(),
        )),
    }
}

/// Un fichero de BD temporal y los que SQLite deja a su lado mientras escribe.
fn with_sidecars(path: &Path) -> Vec<PathBuf> {
    let mut out = vec![path.to_owned()];
    for suffix in ["-journal", "-wal", "-shm"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        out.push(PathBuf::from(name));
    }
    out
}

/// Hace una copia de la empresa en `dest` (`.taxpain`) y la comprueba abriéndola.
///
/// La BD de la copia se construye y se comprueba en la carpeta privada de la empresa (la del
/// almacén, `0700`; en memoria, la carpeta temporal del usuario), nunca en la de destino, que
/// suele ser una carpeta sincronizada con la nube: hasta que no se le quitan los certificados,
/// la clave de las copias y los secretos de la app, ese fichero no debe poder subirse a ningún
/// sitio. En el destino sólo se escribe el fichero final (primero con otro nombre, y un
/// renombrado atómico al acabar).
pub fn export(
    v: &Vault,
    dest: &Path,
    app_version: &str,
    now: OffsetDateTime,
) -> Result<BackupReport> {
    let conn = v.connection();
    if !conn.is_autocommit() {
        return Err(VaultError::InvalidInput(
            "no se puede copiar en mitad de una operación".into(),
        ));
    }
    let stored = load_key(conn)?.ok_or_else(|| {
        VaultError::InvalidInput("antes define la contraseña de las copias".into())
    })?;
    let tmp_db = match v.dir() {
        Some(dir) => dir.join("export.db.tmp"),
        None => temp_file()?,
    };
    let (header, expected) = write_backup(conn, &stored, &tmp_db, dest, app_version, now)?;
    let audit_entries = expected;

    let bytes = std::fs::metadata(dest)?.len();
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    audit::append(
        conn,
        &now_str(now),
        Actor::User,
        "backup.export",
        &name,
        &json!({ "bytes": bytes, "sha256": header.sha256 }),
    )?;
    Ok(BackupReport {
        path: dest.to_owned(),
        bytes,
        created_at: header.created_at,
        sha256: header.sha256,
        audit_entries,
    })
}

/// Escribe en `dest` la copia de la BD de `conn`, con la clave de las copias `stored`. `tmp_db`
/// es dónde se construye la BD de la copia: un fichero en una carpeta privada (ver [`export`]).
/// Devuelve la cabecera y las entradas de auditoría de la copia.
fn write_backup(
    conn: &Connection,
    stored: &BackupKey,
    tmp_db: &Path,
    dest: &Path,
    app_version: &str,
    now: OffsetDateTime,
) -> Result<(BackupHeader, i64)> {
    if let Some(dir) = dest.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let tmp_out = sibling(dest, ".tmp");
    let mut temporaries = with_sidecars(tmp_db);
    temporaries.push(tmp_out.clone());
    for p in &temporaries {
        match std::fs::remove_file(p) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    let _cleanup = Cleanup(temporaries);
    // Sólo legible por el usuario desde antes de que SQLite escriba en él.
    drop(private_file(tmp_db)?);

    let file_key = Key256::random()?;
    let (schema_version, expected) = export_db(conn, tmp_db, &file_key)?;
    // Se comprueba antes de escribir la copia: una mala nunca sustituye a una buena.
    verify_db(tmp_db, &file_key, expected)?;
    let header = write_container(
        tmp_db,
        &file_key,
        schema_version,
        stored,
        dest,
        app_version,
        now,
    )?;
    Ok((header, expected))
}

/// Escribe en `dest` el fichero de la copia: la cabecera y la BD `db` (ya sin secretos y
/// cifrada con `file_key`, que va sellada en la cabecera con la clave de las copias `stored`).
/// Primero en un temporal junto al destino, que se vuelve a leer, y después un renombrado
/// atómico.
fn write_container(
    db: &Path,
    file_key: &Key256,
    schema_version: i64,
    stored: &BackupKey,
    dest: &Path,
    app_version: &str,
    now: OffsetDateTime,
) -> Result<BackupHeader> {
    let tmp_db = db;
    let tmp_out = sibling(dest, ".tmp");
    let header = BackupHeader {
        format: FORMAT,
        created_at: now_str(now),
        app_version: app_version.to_owned(),
        schema_version,
        kdf: stored.params,
        salt: hex::encode(stored.salt),
        key: hex::encode(seal::seal(&stored.kek, SEAL_CONTEXT, file_key.expose())?),
        sha256: sha256_file(tmp_db)?,
    };
    let json = serde_json::to_vec(&header).map_err(|e| VaultError::InvalidInput(e.to_string()))?;
    let len = u32::try_from(json.len()).map_err(|_| VaultError::Corrupted)?;
    {
        let mut f = BufWriter::new(std::fs::File::create(&tmp_out)?);
        f.write_all(MAGIC)?;
        f.write_all(&len.to_le_bytes())?;
        f.write_all(&json)?;
        std::io::copy(&mut BufReader::new(std::fs::File::open(tmp_db)?), &mut f)?;
        f.into_inner()
            .map_err(|e| VaultError::Io(e.into_error()))?
            .sync_all()?;
    }
    // Lo escrito se vuelve a leer: cabecera legible, huella correcta y la clave de la BD se
    // recupera con la de la contraseña.
    let written = read_into(&tmp_out, &mut std::io::sink())?;
    let unsealed = unseal_file_key(&written, &stored.kek)?;
    if written != header || unsealed.expose() != file_key.expose() {
        return Err(VaultError::InvalidInput(
            "la copia no ha salido bien: vuelve a intentarlo".into(),
        ));
    }
    std::fs::rename(&tmp_out, dest)?;
    Ok(header)
}

/// Lee la cabecera de la copia `src` y copia su BD en `out` comprobando la huella, por partes
/// (una copia puede ocupar gigas: no se carga entera en memoria).
fn read_into(src: &Path, out: &mut dyn Write) -> Result<BackupHeader> {
    let not_backup =
        || VaultError::InvalidInput("el fichero no es una copia de seguridad de TaxPain".into());
    let eof = |e: std::io::Error| {
        if e.kind() == std::io::ErrorKind::UnexpectedEof {
            not_backup()
        } else {
            VaultError::Io(e)
        }
    };
    let mut f = BufReader::new(std::fs::File::open(src)?);
    let mut magic = [0u8; 8];
    f.read_exact(&mut magic).map_err(eof)?;
    if &magic != MAGIC {
        return Err(not_backup());
    }
    let mut len = [0u8; 4];
    f.read_exact(&mut len).map_err(eof)?;
    let len = usize::try_from(u32::from_le_bytes(len)).map_err(|_| not_backup())?;
    if len > MAX_HEADER {
        return Err(not_backup());
    }
    let mut json = vec![0u8; len];
    f.read_exact(&mut json).map_err(eof)?;
    let header: BackupHeader = serde_json::from_slice(&json).map_err(|_| not_backup())?;
    if header.format != FORMAT {
        return Err(VaultError::InvalidInput(
            "la copia es de un formato que esta versión de TaxPain no conoce: actualiza la app"
                .into(),
        ));
    }
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        let Some(chunk) = buf.get(..n).filter(|c| !c.is_empty()) else {
            break;
        };
        hasher.update(chunk);
        out.write_all(chunk)?;
    }
    out.flush()?;
    if hex::encode(hasher.finalize()) != header.sha256 {
        return Err(VaultError::InvalidInput(
            "la copia está dañada (el contenido no coincide con su huella)".into(),
        ));
    }
    Ok(header)
}

/// Escribe la BD de la copia `src` en el fichero `path` (con permisos sólo para el usuario).
fn extract_db(src: &Path, path: &Path) -> Result<BackupHeader> {
    let mut f = BufWriter::new(private_file(path)?);
    let header = read_into(src, &mut f)?;
    f.into_inner()
        .map_err(|e| VaultError::Io(e.into_error()))?
        .sync_all()?;
    Ok(header)
}

/// Crea `path` sólo legible por el usuario.
fn private_file(path: &Path) -> Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    Ok(o.open(path)?)
}

/// Lee la cabecera de una copia sin la contraseña (fecha, versión) y comprueba su huella.
pub fn read_header(src: &Path) -> Result<BackupHeader> {
    read_into(src, &mut std::io::sink())
}

fn unseal_file_key(header: &BackupHeader, kek: &Key256) -> Result<Key256> {
    let sealed = hex::decode(&header.key).map_err(|_| VaultError::Corrupted)?;
    let raw = seal::unseal(kek, SEAL_CONTEXT, &sealed).map_err(|_| VaultError::BackupPassphrase)?;
    Ok(Key256::from_bytes(raw.expose())?)
}

/// La clave de la BD de la copia, con la contraseña con la que se hizo. La clave derivada de esa
/// contraseña (y los parámetros de scrypt de la cabecera, que elige quien hizo la copia) sólo
/// sirven para esto: no se guardan ni se reutilizan.
fn unlock(header: &BackupHeader, passphrase: &str) -> Result<Key256> {
    let salt: [u8; 16] = hex::decode(&header.salt)
        .ok()
        .and_then(|s| s.try_into().ok())
        .ok_or(VaultError::Corrupted)?;
    let kek = derive(passphrase, &salt, header.kdf)?;
    unseal_file_key(header, &kek)
}

/// Copia abierta para consultarla sin restaurarla.
pub struct OpenedBackup {
    pub header: BackupHeader,
    /// Conexión con la BD de la copia adjunta (las consultas sin esquema van a ella). La BD
    /// sigue cifrada en un temporal que se borra al soltar la copia.
    pub conn: Connection,
    _tmp: Cleanup,
}

impl OpenedBackup {
    /// ¿Es `name` una tabla de la copia? (Y no una vista con ese nombre: quien enseña el
    /// contenido de una copia sólo ha de leer tablas.)
    pub fn has_table(&self, name: &str) -> Result<bool> {
        let n: i64 = self.conn.query_row(
            &format!(
                "SELECT count(*) FROM {SCHEMA}.sqlite_master
                 WHERE type = 'table' AND name = ?1 AND coalesce(rootpage, 0) > 0"
            ),
            [name],
            |r| r.get(0),
        )?;
        Ok(n == 1)
    }
}

impl std::fmt::Debug for OpenedBackup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenedBackup")
            .field("header", &self.header)
            .finish_non_exhaustive()
    }
}

/// Abre una copia con su contraseña para ver qué contiene antes de restaurarla.
pub fn open(src: &Path, passphrase: &str) -> Result<OpenedBackup> {
    let tmp = temp_file()?;
    let guard = Cleanup(vec![tmp.clone()]);
    let header = extract_db(src, &tmp)?;
    let key = unlock(&header, passphrase)?;
    // BD principal vacía en memoria: las consultas sin esquema resuelven en la adjunta.
    let conn = db::open_in_memory_unmigrated(&Key256::random()?)?;
    // La copia puede venir de otra persona: conexión endurecida, comprobación de integridad
    // antes de leer y sólo consultas.
    db::harden(&conn)?;
    attach_untrusted(&conn, &tmp, &key)?;
    conn.execute_batch("PRAGMA query_only = ON")?;
    Ok(OpenedBackup {
        header,
        conn,
        _tmp: guard,
    })
}

/// Fichero temporal con nombre aleatorio en la carpeta temporal del usuario.
fn temp_file() -> Result<PathBuf> {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).map_err(|e| VaultError::KeyUnavailable(e.to_string()))?;
    Ok(std::env::temp_dir().join(format!("taxpain-copia-{}.tmp", hex::encode(b))))
}

/// Restaura una copia en `dir` (una carpeta de empresa nueva, sin BD) con la clave `db-master`
/// de este Mac y la abre.
///
/// La copia puede venir de otra persona, así que no se da por bueno todo lo que trae:
///
/// - **La contraseña de la copia no se hereda.** La empresa restaurada queda sin contraseña de
///   copias y sin código de recuperación hasta que el usuario elija una nueva
///   ([`set_passphrase`]; la app la pide al abrirla): la contraseña de la copia es de quien la
///   hizo, y sellar con ella `db-master` (la misma clave para todas las empresas del Mac) le
///   daría la llave de todas. Tampoco se heredan los parámetros de scrypt de su cabecera.
/// - **Esquema**: sólo el de TaxPain para la versión de la copia ([`check_schema`]); si trae
///   triggers, vistas, índices o tablas virtuales de más, no se restaura.
/// - **Tablas que no viajan en las copias** (certificados, clave de las copias, secretos de la
///   app): una copia legítima las trae vacías ([`export_db`]); se vuelven a vaciar por si no.
///
/// Los ajustes que son rutas de este Mac (carpetas vigiladas, destino de las facturas) los
/// limpia quien restaura desde la app (`taxpain_app::backup::mark_restored`).
pub fn restore(
    src: &Path,
    passphrase: &str,
    dir: &Path,
    keys: Arc<dyn KeyProvider>,
    presence: Arc<dyn UserPresence>,
    now: OffsetDateTime,
) -> Result<(Vault, BackupHeader)> {
    let target = dir.join("taxpain.db");
    if target.exists() {
        return Err(VaultError::InvalidInput(
            "ya hay datos en esa carpeta: la copia se restaura como empresa nueva".into(),
        ));
    }
    let header = read_header(src)?;
    // La clave derivada de la contraseña de la copia sólo sirve para abrirla: no se guarda.
    let file_key = unlock(&header, passphrase)?;
    let schema = usize::try_from(header.schema_version).map_err(|_| VaultError::Corrupted)?;
    if schema > db::schema_version() {
        return Err(VaultError::InvalidInput(
            "la copia es de una versión más nueva de TaxPain: actualiza la app".into(),
        ));
    }
    std::fs::create_dir_all(dir)?;
    taxpain_secrets::keys::restrict_permissions(dir, 0o700)?;
    let tmp = dir.join("restore.db.tmp");
    let _cleanup = Cleanup(with_sidecars(&tmp));
    if extract_db(src, &tmp)? != header {
        return Err(VaultError::InvalidInput(
            "la copia ha cambiado mientras se restauraba: vuelve a intentarlo".into(),
        ));
    }

    let result = (|| -> Result<Vault> {
        let key = keys.get_or_create(KeyId::DbMaster, "Abrir los datos de TaxPain")?;
        {
            let conn = db::open_unmigrated(&target, &key)?;
            db::harden(&conn)?;
            without_foreign_keys(&conn, || {
                let attached = attach_untrusted(&conn, &tmp, &file_key);
                let r = attached.and_then(|()| import_attached(&conn, schema));
                let detached = conn.execute_batch(&format!("DETACH DATABASE {SCHEMA}"));
                r?;
                detached?;
                Ok(())
            })?;
            // La empresa restaurada nace con su ancla: la cabeza de la auditoría que trae la
            // copia. Sin ella, la primera apertura vería entradas sin ancla y avisaría.
            if let Some(head) = audit::head(&conn)? {
                vault::write_anchor(dir, &head)?;
            }
        }
        let v = Vault::open(dir, keys, presence)?;
        let name = src
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        audit::append(
            v.connection(),
            &now_str(now),
            Actor::User,
            "backup.restore",
            &name,
            &json!({
                "created_at": header.created_at,
                "app_version": header.app_version,
                "sha256": header.sha256,
            }),
        )?;
        Ok(v)
    })();
    if result.is_err() {
        for f in [
            "taxpain.db",
            "taxpain.db-wal",
            "taxpain.db-shm",
            "audit.head",
            "audit.head.tmp",
            recovery::FILE,
        ] {
            let _ = std::fs::remove_file(dir.join(f));
        }
    }
    result.map(|v| (v, header))
}

/// Vuelca en `main` (vacía) la BD de la copia adjunta, de la versión de esquema `version`, tras
/// comprobar su esquema, y le quita lo que no ha de traer. Ver [`restore`].
fn import_attached(conn: &Connection, version: usize) -> Result<()> {
    let copied: i64 = conn.query_row(&format!("PRAGMA {SCHEMA}.user_version"), [], |r| r.get(0))?;
    if usize::try_from(copied).ok() != Some(version) {
        // La cabecera dice una versión y la BD otra: no es una copia hecha por TaxPain.
        return Err(VaultError::Corrupted);
    }
    let repairs = check_schema(conn, version)?;
    conn.query_row(
        &format!("SELECT sqlcipher_export('main', '{SCHEMA}')"),
        [],
        |_| Ok(()),
    )?;
    conn.pragma_update(None, "user_version", copied)?;
    // Índices con una definición que no es la de TaxPain: se rehacen con la nuestra. Si los
    // datos de la copia no caben en ella (p. ej. duplicados en uno único), no es una copia
    // nuestra.
    for (name, sql) in &repairs.replace {
        conn.execute_batch(&format!(
            "DROP INDEX main.\"{}\"",
            name.replace('"', "\"\"")
        ))
        .and_then(|()| conn.execute_batch(sql))
        .map_err(|_| not_ours())?;
    }
    for sql in &repairs.missing {
        if let Err(e) = conn.execute_batch(sql) {
            tracing::warn!(error = %e, "a la copia le falta un índice que no se ha podido crear");
        }
    }
    // Lo que no viaja en las copias: una legítima lo trae vacío. Siempre con el esquema
    // delante. El aviso de auditoría pendiente era del almacén de origen y de su ancla.
    conn.execute_batch(
        "DELETE FROM main.certificate_secret;
         DELETE FROM main.certificate_ref;
         DELETE FROM main.backup_key;
         DELETE FROM main.app_secret;",
    )
    .map_err(|_| not_ours())?;
    conn.execute(
        "DELETE FROM main.settings WHERE key = ?1",
        [audit::PENDING_KEY],
    )
    .map_err(|_| not_ours())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use taxpain_secrets::{AlwaysConfirm, MemoryKeyProvider};

    const PASS: &str = "caballo grapa batería";
    /// scrypt más ligero para que los tests vayan rápido (el formato guarda los parámetros).
    const FAST: KdfParams = KdfParams {
        log_n: 12,
        r: 8,
        p: 1,
    };

    fn now() -> OffsetDateTime {
        OffsetDateTime::now_utc()
    }

    /// Empresa de origen con contraseña de copias.
    fn source(dir: &Path) -> Vault {
        let v = Vault::open(
            &dir.join("origen"),
            Arc::new(MemoryKeyProvider::default()),
            Arc::new(AlwaysConfirm),
        )
        .unwrap();
        v.set_setting(
            "company",
            &json!({ "name": "EJEMPLO SL", "nif": "B12345674" }),
        )
        .unwrap();
        set_passphrase_with(&v, PASS, FAST, now()).unwrap();
        v
    }

    /// Restaura en «otro Mac» (otro proveedor de claves).
    fn restore_to(copy: &Path, target: &Path) -> Result<(Vault, BackupHeader)> {
        restore(
            copy,
            PASS,
            target,
            Arc::new(MemoryKeyProvider::default()),
            Arc::new(AlwaysConfirm),
            now(),
        )
    }

    /// Lo que [`check_schema`] dice de la BD de una copia.
    fn repairs_of(copy: &Path) -> Result<IndexRepairs> {
        let tmp = temp_file().unwrap();
        let _cleanup = Cleanup(with_sidecars(&tmp));
        let header = extract_db(copy, &tmp).unwrap();
        let key = unlock(&header, PASS).unwrap();
        let conn = db::open_in_memory_unmigrated(&Key256::random().unwrap()).unwrap();
        db::harden(&conn).unwrap();
        attach_untrusted(&conn, &tmp, &key).unwrap();
        check_schema(&conn, usize::try_from(header.schema_version).unwrap())
    }

    /// (tipo, nombre, tabla, SQL sin espacios de más) de cada objeto del esquema.
    fn schema_of(objs: &[db::SchemaObject]) -> Vec<(String, String, String, String)> {
        objs.iter()
            .map(|o| {
                (
                    o.kind.clone(),
                    o.name.clone(),
                    o.table.clone(),
                    normalize_sql(o.sql.as_deref().unwrap_or_default()),
                )
            })
            .collect()
    }

    fn rows(v: &Vault, table: &str) -> i64 {
        v.connection()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// Ida y vuelta de una copia legítima de cada versión del esquema desde que hay copias (la
    /// 39, que crea `backup_key`): un almacén de esa versión, con entradas de auditoría como las
    /// escribía (v1 hasta la 64), se copia y se restaura. El esquema de la copia es exactamente
    /// el de TaxPain (nada que rechazar ni que rehacer) y, restaurada y migrada, queda con el
    /// esquema de una BD nueva de esta versión.
    #[test]
    fn una_copia_legitima_de_cada_version_se_restaura() {
        let tmp = tempfile::tempdir().unwrap();
        let current = db::schema_version();
        let clean = schema_of(&db::clean_schema(current).unwrap());
        for version in 39..=current {
            let dir = tmp.path().join(format!("v{version}"));
            std::fs::create_dir_all(&dir).unwrap();
            let conn =
                db::open_at_version(&dir.join("taxpain.db"), &Key256::random().unwrap(), version)
                    .unwrap();
            conn.execute(
                "INSERT INTO settings(key, value) VALUES ('company', ?1)",
                [r#"{"name":"EJEMPLO SL","nif":"B12345674"}"#],
            )
            .unwrap();
            for action in ["company.set", "invoice.issue"] {
                if version < 65 {
                    audit::append_v1(
                        &conn,
                        "2026-10-24T10:00:00Z",
                        Actor::User,
                        action,
                        "x",
                        "{}",
                    );
                } else {
                    audit::append(
                        &conn,
                        "2026-10-24T10:00:00Z",
                        Actor::User,
                        action,
                        "x",
                        &json!({}),
                    )
                    .unwrap();
                }
            }
            let salt = [3u8; 16];
            let stored = BackupKey {
                kek: derive(PASS, &salt, FAST).unwrap(),
                salt,
                params: FAST,
            };
            store_key(&conn, &stored, now()).unwrap();
            let copy = tmp.path().join(format!("copias/v{version}.taxpain"));
            let (header, entries) = write_backup(
                &conn,
                &stored,
                &dir.join("export.db.tmp"),
                &copy,
                "0.1.0",
                now(),
            )
            .unwrap();
            assert_eq!(header.schema_version, i64::try_from(version).unwrap());
            assert_eq!(entries, 2);
            assert_eq!(
                repairs_of(&copy).unwrap(),
                IndexRepairs::default(),
                "versión {version}"
            );

            let target = tmp.path().join(format!("restaurada-v{version}"));
            let (v, restored_header) =
                restore_to(&copy, &target).unwrap_or_else(|e| panic!("versión {version}: {e}"));
            assert_eq!(restored_header, header);
            assert_eq!(db::user_version(v.connection()).unwrap(), current);
            assert_eq!(
                v.setting::<serde_json::Value>("company").unwrap().unwrap()["nif"],
                "B12345674"
            );
            // Las dos de la copia y la de la restauración.
            assert_eq!(v.verify_audit().unwrap(), 3, "versión {version}");
            assert!(v.audit_rollback_pending().unwrap().is_none());
            assert!(!has_passphrase(&v).unwrap());
            assert!(!recovery::exists(&target));
            assert_eq!(
                schema_of(&db::schema_objects(v.connection(), "main").unwrap()),
                clean,
                "versión {version}"
            );
            // No queda ningún temporal junto a los datos restaurados ni junto a los de origen.
            drop(v);
            drop(conn);
            let left = |d: &Path| -> Vec<String> {
                names_in(d)
                    .into_iter()
                    .filter(|n| !n.ends_with("-wal") && !n.ends_with("-shm"))
                    .collect()
            };
            assert_eq!(left(&target), ["audit.head", "taxpain.db"]);
            assert_eq!(left(&dir), ["taxpain.db"]);
        }
    }

    /// Una copia con triggers, vistas, índices o tablas virtuales que no son de TaxPain no se
    /// restaura, y no deja nada en la carpeta de la empresa nueva.
    #[test]
    fn una_copia_con_el_esquema_cambiado_se_rechaza() {
        let tmp = tempfile::tempdir().unwrap();
        let rejected = |name: &str, sql: &str| {
            let dir = tmp.path().join(name);
            let v = source(&dir);
            if let Err(e) = v.connection().execute_batch(sql) {
                panic!("{name}: {e}");
            }
            let copy = dir.join("copia.taxpain");
            export(&v, &copy, "0.1.0", now()).unwrap();
            let target = dir.join("restaurada");
            let e = restore_to(&copy, &target).unwrap_err();
            assert!(
                matches!(&e, VaultError::InvalidInput(m) if m.contains("no crea TaxPain")),
                "{name}: {e}"
            );
            assert_eq!(names_in(&target), Vec::<String>::new(), "{name}");
        };
        rejected(
            "trigger-de-mas",
            "CREATE TRIGGER colado AFTER INSERT ON invoice_issued
             BEGIN UPDATE settings SET value = 'x' WHERE key = 'company'; END;",
        );
        rejected(
            "trigger-cambiado",
            "DROP TRIGGER audit_log_no_delete;
             CREATE TRIGGER audit_log_no_delete BEFORE DELETE ON audit_log
             BEGIN SELECT 1; END;",
        );
        rejected("trigger-que-falta", "DROP TRIGGER audit_log_no_update;");
        rejected("vista", "CREATE VIEW colada AS SELECT * FROM settings;");
        rejected("indice-de-mas", "CREATE INDEX colado ON settings(value);");
        rejected(
            "indice-en-otra-tabla",
            "DROP INDEX invoice_issued_date;
             CREATE INDEX invoice_issued_date ON settings(value);",
        );
        // Una tabla virtual (si esta compilación de SQLite trae FTS5).
        let probe = db::open_in_memory_unmigrated(&Key256::random().unwrap()).unwrap();
        if probe
            .execute_batch("CREATE VIRTUAL TABLE prueba USING fts5(x)")
            .is_ok()
        {
            rejected(
                "tabla-virtual",
                "CREATE VIRTUAL TABLE colada USING fts5(x);",
            );
        }
    }

    /// Las diferencias de espacios no son un cambio: SQLite guarda el SQL tal como se escribió.
    #[test]
    fn un_trigger_con_otros_espacios_es_el_mismo() {
        let tmp = tempfile::tempdir().unwrap();
        let v = source(tmp.path());
        v.connection()
            .execute_batch(
                "DROP TRIGGER audit_log_no_delete;
                 CREATE TRIGGER audit_log_no_delete   BEFORE DELETE
                   ON audit_log

                 BEGIN SELECT RAISE(ABORT, 'audit_log es de solo anexado'); END;",
            )
            .unwrap();
        let copy = tmp.path().join("copia.taxpain");
        export(&v, &copy, "0.1.0", now()).unwrap();
        assert_eq!(repairs_of(&copy).unwrap(), IndexRepairs::default());
        assert!(restore_to(&copy, &tmp.path().join("restaurada")).is_ok());
    }

    /// Un índice de TaxPain con otra definición (un almacén de una versión de desarrollo) o que
    /// falta no impide restaurar: se rehace con la definición de esta compilación.
    #[test]
    fn un_indice_distinto_o_que_falta_se_rehace() {
        let tmp = tempfile::tempdir().unwrap();
        let clean = db::clean_schema(db::schema_version()).unwrap();
        let clean_sql = |name: &str| {
            clean
                .iter()
                .find(|o| o.kind == "index" && o.name == name)
                .and_then(|o| o.sql.clone())
                .unwrap_or_else(|| panic!("{name} ya no está en el esquema"))
        };
        let v = source(tmp.path());
        v.connection()
            .execute_batch(
                "DROP INDEX journal_entry_origin;
                 CREATE UNIQUE INDEX journal_entry_origin ON journal_entry(origin_kind, origin_id)
                   WHERE origin_id IS NOT NULL;
                 DROP INDEX invoice_issued_date;",
            )
            .unwrap();
        let copy = tmp.path().join("copia.taxpain");
        export(&v, &copy, "0.1.0", now()).unwrap();
        assert_eq!(
            repairs_of(&copy).unwrap(),
            IndexRepairs {
                replace: vec![(
                    "journal_entry_origin".to_owned(),
                    clean_sql("journal_entry_origin")
                )],
                missing: vec![clean_sql("invoice_issued_date")],
            }
        );
        let (restored, _) = restore_to(&copy, &tmp.path().join("restaurada")).unwrap();
        assert_eq!(
            schema_of(&db::schema_objects(restored.connection(), "main").unwrap()),
            schema_of(&clean)
        );
    }

    /// Una copia fabricada que trae filas en las tablas que no viajan en las copias (la clave de
    /// API de otro, certificados, la clave de las copias con scrypt débil) y un aviso de
    /// auditoría pendiente: nada de eso llega a la empresa restaurada.
    #[test]
    fn lo_que_no_viaja_en_las_copias_no_llega_aunque_la_copia_lo_traiga() {
        let tmp = tempfile::tempdir().unwrap();
        let v = source(tmp.path());
        let conn = v.connection();
        conn.execute_batch(
            "INSERT INTO app_secret(name, value, updated_at)
               VALUES ('assistant.api_key@api.anthropic.com', 'clave-de-otro', 'x');
             INSERT INTO certificate_ref(id, fingerprint_sha256, subject_dn, issuer_dn, serial,
               valid_from, valid_to, kind, issuer_kind, holder_nif, key_algorithm, created_at)
               VALUES (7, 'aa', 'CN=FANTASMA', 'CN=AC', '01', '2026-01-01T00:00:00Z',
                 '2030-01-01T00:00:00Z', 'otro', 'otro', '12345678Z', 'EC 256', 'x');
             INSERT INTO certificate_secret(certificate_id, sealed) VALUES (7, x'00');",
        )
        .unwrap();
        v.set_setting(
            audit::PENDING_KEY,
            &audit::PendingRollback {
                kind: audit::AnchorProblem::Rollback,
                anchored_seq: Some(9),
                current_seq: 3,
                detected_at: "2026-10-24T10:00:00Z".into(),
            },
        )
        .unwrap();
        // La BD de la copia, tal cual, sin pasar por la limpieza de `export_db`.
        let raw = tmp.path().join("cruda.db");
        let file_key = Key256::random().unwrap();
        let version = i64::try_from(db::schema_version()).unwrap();
        without_foreign_keys(conn, || {
            attach(conn, &raw, &file_key)?;
            conn.query_row(&format!("SELECT sqlcipher_export('{SCHEMA}')"), [], |_| {
                Ok(())
            })?;
            conn.pragma_update(Some(SCHEMA), "user_version", version)?;
            conn.execute_batch(&format!("DETACH DATABASE {SCHEMA}"))?;
            Ok(())
        })
        .unwrap();
        let stored = load_key(conn).unwrap().unwrap();
        let copy = tmp.path().join("fabricada.taxpain");
        write_container(&raw, &file_key, version, &stored, &copy, "0.1.0", now()).unwrap();
        // La copia las trae…
        let opened = open(&copy, PASS).unwrap();
        for table in [
            "app_secret",
            "certificate_ref",
            "certificate_secret",
            "backup_key",
        ] {
            let n: i64 = opened
                .conn
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(n, 1, "{table}");
        }
        drop(opened);

        // …y la empresa restaurada no.
        let target = tmp.path().join("restaurada");
        let (restored, _) = restore_to(&copy, &target).unwrap();
        for table in [
            "app_secret",
            "certificate_ref",
            "certificate_secret",
            "backup_key",
        ] {
            assert_eq!(rows(&restored, table), 0, "{table}");
        }
        assert!(restored.certificates().unwrap().is_empty());
        assert!(!has_passphrase(&restored).unwrap());
        assert!(!recovery::exists(&target));
        assert!(restored.audit_rollback_pending().unwrap().is_none());
        assert!(restored.audit_rollback().is_none());
        assert_eq!(
            restored
                .setting::<serde_json::Value>("company")
                .unwrap()
                .unwrap()["name"],
            "EJEMPLO SL"
        );
    }

    /// La BD de la copia se construye en la carpeta privada de la empresa, no junto al destino
    /// (que suele estar sincronizado con la nube), y no quedan temporales en ninguna de las dos,
    /// tampoco si la copia falla.
    #[test]
    fn la_bd_de_la_copia_no_se_construye_en_la_carpeta_de_destino() {
        let tmp = tempfile::tempdir().unwrap();
        let v = source(tmp.path());
        let private = tmp.path().join("origen");
        let before = names_in(&private);
        let cloud = tmp.path().join("nube");
        std::fs::create_dir_all(&cloud).unwrap();
        // Donde antes se construía la BD sin limpiar (junto al destino) hay ahora una carpeta:
        // si se siguiera usando ese sitio, la copia fallaría.
        std::fs::create_dir(cloud.join(".EJEMPLO.taxpain.db.tmp")).unwrap();
        let dest = cloud.join("EJEMPLO.taxpain");
        export(&v, &dest, "0.1.0", now()).unwrap();
        assert_eq!(
            names_in(&cloud),
            [".EJEMPLO.taxpain.db.tmp", "EJEMPLO.taxpain"]
        );
        assert_eq!(names_in(&private), before);

        // El destino es una carpeta: el renombrado final falla.
        let bad = cloud.join("carpeta.taxpain");
        std::fs::create_dir(&bad).unwrap();
        std::fs::write(bad.join("algo"), b"x").unwrap();
        assert!(export(&v, &bad, "0.1.0", now()).is_err());
        assert_eq!(
            names_in(&cloud),
            [
                ".EJEMPLO.taxpain.db.tmp",
                "EJEMPLO.taxpain",
                "carpeta.taxpain"
            ]
        );
        assert_eq!(names_in(&private), before);
        // Y la copia buena se sigue abriendo.
        assert!(open(&dest, PASS).is_ok());
    }

    /// Al enseñar una copia sólo se leen tablas: una vista con el nombre de una tabla no cuenta.
    #[test]
    fn una_vista_con_nombre_de_tabla_no_es_una_tabla() {
        let tmp = tempfile::tempdir().unwrap();
        let v = source(tmp.path());
        v.connection()
            .execute_batch(
                "ALTER TABLE settings RENAME TO settings_de_verdad;
                 CREATE VIEW settings AS SELECT key, 'colado' AS value FROM settings_de_verdad;",
            )
            .unwrap();
        let copy = tmp.path().join("copia.taxpain");
        export(&v, &copy, "0.1.0", now()).unwrap();
        let opened = open(&copy, PASS).unwrap();
        assert!(!opened.has_table("settings").unwrap());
        assert!(opened.has_table("settings_de_verdad").unwrap());
        assert!(opened.has_table("audit_log").unwrap());
        assert!(!opened.has_table("no_existe").unwrap());
        // La conexión de la vista previa es de sólo consulta.
        assert!(
            opened
                .conn
                .execute("DELETE FROM settings_de_verdad", [])
                .is_err()
        );
        drop(opened);
        // Y no se restaura.
        assert!(restore_to(&copy, &tmp.path().join("restaurada")).is_err());
    }

    #[test]
    fn rechaza_ficheros_que_no_son_copias() {
        let dir = tempfile::tempdir().unwrap();
        let parse = |bytes: &[u8]| {
            let p = dir.path().join("x.taxpain");
            std::fs::write(&p, bytes).unwrap();
            read_header(&p)
        };
        assert!(parse(b"hola").is_err());
        assert!(parse(b"TPBACKUP\xff\xff\xff\xff{}").is_err());
        let mut f = MAGIC.to_vec();
        f.extend_from_slice(&2u32.to_le_bytes());
        f.extend_from_slice(b"{}");
        assert!(parse(&f).is_err(), "cabecera sin campos");
    }

    #[test]
    fn limites_de_scrypt() {
        assert!(
            KdfParams {
                log_n: 30,
                r: 8,
                p: 1
            }
            .check()
            .is_err()
        );
        assert!(KdfParams::DEFAULT.check().is_ok());
        // Un fichero manipulado no puede pedir 2 GiB de memoria ni cuatro pasadas.
        for (log_n, r, p) in [(20, 16, 1), (19, 8, 1), (17, 8, 4)] {
            assert!(
                KdfParams { log_n, r, p }.check().is_err(),
                "{log_n} {r} {p}"
            );
        }
        assert!(
            KdfParams {
                log_n: 18,
                r: 8,
                p: 2
            }
            .check()
            .is_ok()
        );
    }

    #[test]
    fn contrasena_minima() {
        assert!(check_passphrase("corta").is_err());
        assert!(check_passphrase(" con espacios al principio").is_err());
        assert!(check_passphrase("una contraseña larga").is_ok());
        // Largas pero fáciles de adivinar.
        assert!(check_passphrase("aaaaaaaaaaaa").is_err());
        assert!(check_passphrase("1212121212").is_err());
        assert!(check_passphrase("AaAaBbBbAaAa").is_err());
        assert!(check_passphrase("0123456789").is_err());
        assert!(check_passphrase("abcdefghijk").is_err());
        assert!(check_passphrase("caballo grapa batería").is_ok());
        // Series que dan la vuelta o van hacia atrás, filas del teclado y contraseñas típicas.
        for weak in [
            "1234567890",
            "0987654321",
            "9876543210",
            "qwertyuiop",
            "POIUYTREWQ",
            "asdfghjklñ",
            "xyzabcdefgh",
            "lmnopqrstu",
            "password123",
            "Contraseña2026",
        ] {
            assert!(check_passphrase(weak).is_err(), "{weak}");
        }
        for ok in [
            "cebra verde salta 2026",
            "tortuga lenta 2026 azul",
            "1234567890 gato",
        ] {
            assert!(check_passphrase(ok).is_ok(), "{ok}");
        }
    }
}
