//! Registro de auditoría append-only con hash SHA-256 encadenado.
//!
//! Cada entrada lleva la versión de su hash (`audit_log.hash_version`):
//!
//! - **v2** (las entradas nuevas): `hash = SHA256("taxpain-audit-v2" ‖ campo…)`, donde cada
//!   campo (`prev_hash`, `seq` en decimal, `at`, `actor`, `action`, `subject`, `details`) va
//!   precedido de su longitud en bytes (u64 en little-endian). Con las longitudes delante, dos
//!   entradas distintas no pueden dar la misma secuencia de bytes.
//! - **v1** (las anteriores a la migración 0065, que quedan como están): `hash =
//!   SHA256(prev_hash ‖ "\n" ‖ seq ‖ "\n" ‖ at ‖ "\n" ‖ actor ‖ "\n" ‖ action ‖ "\n" ‖ subject ‖
//!   "\n" ‖ details)`. Sin longitudes, un salto de línea dentro de un campo se confunde con el
//!   separador (p. ej. `subject = "a\nb"`, `details = "c"` y `subject = "a"`, `details = "b\nc"`).
//!
//! En hexadecimal minúsculas; la primera entrada encadena con 64 ceros. [`verify`] comprueba cada
//! entrada con la versión que declara, así que una cadena puede empezar en v1 y seguir en v2 (y
//! un programa anterior que siga abierto mientras se actualiza la app añade entradas v1 válidas:
//! no escribe la columna y su valor por defecto es 1).
//!
//! Los triggers de la migración impiden UPDATE/DELETE; `verify` detecta además manipulaciones
//! hechas por fuera (p. ej. editando el fichero con la clave).
//! `details` es JSON y **nunca** debe contener secretos.
//!
//! Ancla: la cabeza de la cadena (seq, hash) se guarda también fuera de la BD
//! (`audit.head`, junto a `taxpain.db`). Al abrir, si la BD ya no contiene esa entrada (copia
//! antigua restaurada, conflicto de sincronización, entradas finales borradas) se avisa y se
//! deja constancia en la propia cadena ([`check_anchor`]).
//!
//! Límite conocido: ni la cadena ni el ancla llevan clave. Quien pueda escribir en la carpeta
//! de datos y tenga la clave de la BD (acceso al Keychain del usuario) puede reescribir ambas;
//! el ancla protege frente a retrocesos accidentales, no frente a ese atacante.

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::error::{Result, VaultError};

pub const GENESIS_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Actor {
    User,
    System,
    /// Un asistente que actúa por el usuario desde fuera de la app: hoy, Claude por el servidor
    /// MCP (`taxpain-mcp`), en sus llamadas (`mcp.call`) y en los borradores que deja. No hay
    /// actor aparte para él porque la columna `audit_log.actor` sólo admite estos tres valores
    /// (CHECK de la migración 0001) y añadir otro pediría migrar el esquema.
    Assistant,
}

impl Actor {
    fn as_str(self) -> &'static str {
        match self {
            Actor::User => "user",
            Actor::System => "system",
            Actor::Assistant => "assistant",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct AuditEntry {
    pub seq: i64,
    pub at: String,
    pub actor: String,
    pub action: String,
    pub subject: String,
    pub details: String,
    pub prev_hash: String,
    pub hash: String,
}

/// Versión del hash de las entradas que se escriben ahora.
const HASH_VERSION: i64 = 2;
/// Separa el dominio del hash v2 del de v1 (que empieza por el hash anterior, en hexadecimal).
const HASH_V2_DOMAIN: &[u8] = b"taxpain-audit-v2";

/// Los campos de una entrada que entran en su hash.
#[derive(Clone, Copy)]
struct Fields<'a> {
    prev: &'a str,
    seq: i64,
    at: &'a str,
    actor: &'a str,
    action: &'a str,
    subject: &'a str,
    details: &'a str,
}

impl Fields<'_> {
    /// Hash con el formato `version`; `None` si esta compilación no conoce esa versión.
    fn hash(&self, version: i64) -> Option<String> {
        let seq = self.seq.to_string();
        let parts = [
            self.prev,
            &seq,
            self.at,
            self.actor,
            self.action,
            self.subject,
            self.details,
        ];
        let mut h = Sha256::new();
        match version {
            1 => {
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        h.update(b"\n");
                    }
                    h.update(part.as_bytes());
                }
            }
            2 => {
                h.update(HASH_V2_DOMAIN);
                for part in parts {
                    // `usize` cabe siempre en `u64` en las plataformas que se compilan.
                    let len = u64::try_from(part.len()).unwrap_or(u64::MAX);
                    h.update(len.to_le_bytes());
                    h.update(part.as_bytes());
                }
            }
            _ => return None,
        }
        Some(hex::encode(h.finalize()))
    }
}

/// Añade una entrada y devuelve su hash.
pub fn append(
    conn: &Connection,
    at: &str,
    actor: Actor,
    action: &str,
    subject: &str,
    details: &serde_json::Value,
) -> Result<String> {
    // SAVEPOINT en vez de transacción: funciona suelto y dentro de la transacción del llamador.
    conn.execute_batch("SAVEPOINT audit_append")?;
    let result = (|| -> Result<String> {
        let (seq, prev) = match head(conn)? {
            Some((s, h)) => (s + 1, h),
            None => (1, GENESIS_HASH.to_owned()),
        };
        let details = details.to_string();
        let hash = Fields {
            prev: &prev,
            seq,
            at,
            actor: actor.as_str(),
            action,
            subject,
            details: &details,
        }
        .hash(HASH_VERSION)
        .ok_or(VaultError::Corrupted)?;
        conn.execute(
            "INSERT INTO audit_log(seq, at, actor, action, subject, details, prev_hash, hash,
               hash_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                seq,
                at,
                actor.as_str(),
                action,
                subject,
                details,
                prev,
                hash,
                HASH_VERSION
            ],
        )?;
        Ok(hash)
    })();
    match &result {
        Ok(_) => conn.execute_batch("RELEASE audit_append")?,
        Err(_) => conn.execute_batch("ROLLBACK TO audit_append; RELEASE audit_append")?,
    }
    let hash = result?;
    Ok(hash)
}

pub fn entries(conn: &Connection) -> Result<Vec<AuditEntry>> {
    let mut stmt = conn.prepare(
        "SELECT seq, at, actor, action, subject, details, prev_hash, hash
         FROM audit_log ORDER BY seq",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(AuditEntry {
            seq: r.get(0)?,
            at: r.get(1)?,
            actor: r.get(2)?,
            action: r.get(3)?,
            subject: r.get(4)?,
            details: r.get(5)?,
            prev_hash: r.get(6)?,
            hash: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Filtro de la consulta del registro (pantalla «Auditoría»): todo opcional.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Filtro {
    /// Desde este día (`AAAA-MM-DD`, incluido), por la fecha de la entrada (UTC).
    pub desde: Option<String>,
    /// Hasta este día (`AAAA-MM-DD`, incluido).
    pub hasta: Option<String>,
    /// Tipo de acción: la acción exacta (`invoice.issue`) o su familia (`invoice`, que coge
    /// `invoice.*`).
    pub accion: Option<String>,
    /// `user`, `system` o `assistant`.
    pub actor: Option<String>,
    /// Texto que aparece en el asunto o en los detalles.
    pub texto: Option<String>,
    /// Página: entradas anteriores a esta (`seq`), para ir hacia atrás desde la última.
    pub antes_de: Option<i64>,
    /// Entradas por página (1–500; 100 si no se dice).
    pub limite: Option<u32>,
}

/// Una página de la consulta, de la más reciente a la más antigua.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Pagina {
    pub entradas: Vec<AuditEntry>,
    /// Entradas que cumplen el filtro (todas las páginas).
    pub total: i64,
    /// `antes_de` de la página siguiente, si hay más.
    pub siguiente: Option<i64>,
}

/// `LIKE` con `\` de escape para un texto que escribe el usuario.
fn like_literal(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// Consulta de sólo lectura del registro con [`Filtro`] (no toca la cadena).
pub fn consultar(conn: &Connection, f: &Filtro) -> Result<Pagina> {
    let limite = i64::from(f.limite.unwrap_or(100).clamp(1, 500));
    let mut cond = vec!["1 = 1".to_owned()];
    let mut args: Vec<rusqlite::types::Value> = Vec::new();
    fn arg(cond: &mut Vec<String>, args: &mut Vec<rusqlite::types::Value>, sql: &str, v: &str) {
        args.push(rusqlite::types::Value::Text(v.to_owned()));
        cond.push(sql.replace('?', &format!("?{}", args.len())));
    }
    if let Some(d) = f.desde.as_deref().filter(|d| !d.is_empty()) {
        arg(&mut cond, &mut args, "substr(at, 1, 10) >= ?", d);
    }
    if let Some(d) = f.hasta.as_deref().filter(|d| !d.is_empty()) {
        arg(&mut cond, &mut args, "substr(at, 1, 10) <= ?", d);
    }
    if let Some(a) = f.accion.as_deref().filter(|a| !a.is_empty()) {
        let n = args.len() + 1;
        args.push(rusqlite::types::Value::Text(a.to_owned()));
        args.push(rusqlite::types::Value::Text(format!(
            "{}.%",
            like_literal(a)
        )));
        cond.push(format!(
            "(action = ?{n} OR action LIKE ?{} ESCAPE '\\')",
            n + 1
        ));
    }
    if let Some(a) = f.actor.as_deref().filter(|a| !a.is_empty()) {
        arg(&mut cond, &mut args, "actor = ?", a);
    }
    if let Some(t) = f.texto.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        let n = args.len() + 1;
        args.push(rusqlite::types::Value::Text(format!(
            "%{}%",
            like_literal(t)
        )));
        cond.push(format!(
            "(subject LIKE ?{n} ESCAPE '\\' OR details LIKE ?{n} ESCAPE '\\')"
        ));
    }
    let filtro = cond.join(" AND ");
    let total: i64 = conn.query_row(
        &format!("SELECT count(*) FROM audit_log WHERE {filtro}"),
        rusqlite::params_from_iter(args.iter()),
        |r| r.get(0),
    )?;
    let mut pagina = filtro.clone();
    let mut page_args = args.clone();
    if let Some(s) = f.antes_de {
        page_args.push(rusqlite::types::Value::Integer(s));
        pagina.push_str(&format!(" AND seq < ?{}", page_args.len()));
    }
    page_args.push(rusqlite::types::Value::Integer(limite + 1));
    let mut stmt = conn.prepare(&format!(
        "SELECT seq, at, actor, action, subject, details, prev_hash, hash
         FROM audit_log WHERE {pagina} ORDER BY seq DESC LIMIT ?{}",
        page_args.len()
    ))?;
    let mut entradas: Vec<AuditEntry> = stmt
        .query_map(rusqlite::params_from_iter(page_args.iter()), |r| {
            Ok(AuditEntry {
                seq: r.get(0)?,
                at: r.get(1)?,
                actor: r.get(2)?,
                action: r.get(3)?,
                subject: r.get(4)?,
                details: r.get(5)?,
                prev_hash: r.get(6)?,
                hash: r.get(7)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;
    let mas = entradas.len() > usize::try_from(limite).unwrap_or(usize::MAX);
    entradas.truncate(usize::try_from(limite).unwrap_or(usize::MAX));
    let siguiente = if mas {
        entradas.last().map(|e| e.seq)
    } else {
        None
    };
    Ok(Pagina {
        entradas,
        total,
        siguiente,
    })
}

/// Tipos de acción que hay en el registro, con cuántas entradas tiene cada uno (para el
/// desplegable del filtro).
pub fn acciones(conn: &Connection) -> Result<Vec<(String, i64)>> {
    let mut stmt =
        conn.prepare("SELECT action, count(*) FROM audit_log GROUP BY action ORDER BY action")?;
    let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Última entrada de la cadena (seq, hash), si hay alguna.
pub fn head(conn: &Connection) -> Result<Option<(i64, String)>> {
    Ok(conn
        .query_row(
            "SELECT seq, hash FROM audit_log ORDER BY seq DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// Cabeza anclada fuera de la BD.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Anchor {
    pub seq: i64,
    pub hash: String,
}

const ANCHOR_MAGIC: &str = "taxpain-audit-head v1";

impl Anchor {
    pub fn encode(&self) -> String {
        format!("{ANCHOR_MAGIC}\n{} {}\n", self.seq, self.hash)
    }

    /// `None` si el contenido no es un ancla válida.
    pub fn decode(text: &str) -> Option<Anchor> {
        let mut lines = text.lines();
        if lines.next()? != ANCHOR_MAGIC {
            return None;
        }
        let (seq, hash) = lines.next()?.split_once(' ')?;
        let seq: i64 = seq.parse().ok()?;
        (seq > 0 && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit())).then(|| {
            Anchor {
                seq,
                hash: hash.to_owned(),
            }
        })
    }
}

/// La BD está por detrás del ancla: le faltan entradas o la cadena es otra.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Rollback {
    /// Última entrada que se había visto.
    pub anchored_seq: i64,
    /// Última entrada que hay ahora en la BD (0 si ninguna).
    pub current_seq: i64,
}

/// Compara la BD con el ancla: `None` si la BD contiene la entrada anclada con el mismo hash.
pub fn check_anchor(conn: &Connection, anchor: &Anchor) -> Result<Option<Rollback>> {
    let at: Option<String> = conn
        .query_row(
            "SELECT hash FROM audit_log WHERE seq = ?1",
            [anchor.seq],
            |r| r.get(0),
        )
        .optional()?;
    if at.as_deref() == Some(anchor.hash.as_str()) {
        return Ok(None);
    }
    Ok(Some(Rollback {
        anchored_seq: anchor.seq,
        current_seq: head(conn)?.map_or(0, |(s, _)| s),
    }))
}

/// Qué se vio al comparar la BD con el ancla al abrir.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorProblem {
    /// La BD está por detrás del ancla (copia antigua de la BD, entradas finales borradas, otra
    /// cadena): ver [`Rollback`].
    Rollback,
    /// Hay entradas de auditoría pero no hay ancla: alguien la ha borrado (quizá para tapar un
    /// retroceso) o la carpeta de datos se ha copiado a medias.
    AnchorMissing,
    /// Hay entradas de auditoría y el ancla no se puede leer.
    AnchorInvalid,
}

impl AnchorProblem {
    /// Acción con la que queda anotado en la propia cadena.
    pub(crate) fn action(self) -> &'static str {
        match self {
            AnchorProblem::Rollback => "audit.rollback",
            AnchorProblem::AnchorMissing => "audit.anchor_missing",
            AnchorProblem::AnchorInvalid => "audit.anchor_invalid",
        }
    }
}

/// Aviso sobre la auditoría que el usuario aún no ha reconocido. Se guarda en la propia BD
/// (ajuste [`PENDING_KEY`]) al detectarlo, lo abra quien lo abra (la app, una tarea de fondo, el
/// servidor MCP), y sólo se borra cuando el usuario lo reconoce desde la interfaz.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PendingRollback {
    pub kind: AnchorProblem,
    /// Última entrada que se había visto (`None` si no había ancla legible).
    pub anchored_seq: Option<i64>,
    /// Última entrada que había en la BD al detectarlo (0 si ninguna).
    pub current_seq: i64,
    /// Cuándo se detectó (RFC 3339, UTC).
    pub detected_at: String,
}

impl PendingRollback {
    /// El retroceso, si el aviso es de un retroceso (y no de un ancla ausente o ilegible).
    pub fn rollback(&self) -> Option<Rollback> {
        match (self.kind, self.anchored_seq) {
            (AnchorProblem::Rollback, Some(anchored_seq)) => Some(Rollback {
                anchored_seq,
                current_seq: self.current_seq,
            }),
            _ => None,
        }
    }
}

/// Ajuste (tabla `settings`) con el [`PendingRollback`] sin reconocer.
pub const PENDING_KEY: &str = "audit_rollback_pending";

/// Comprueba la cadena completa. Devuelve el número de entradas.
pub fn verify(conn: &Connection) -> Result<usize> {
    let mut stmt = conn.prepare(
        "SELECT seq, at, actor, action, subject, details, prev_hash, hash, hash_version
         FROM audit_log ORDER BY seq",
    )?;
    let mut rows = stmt.query([])?;
    let mut prev = GENESIS_HASH.to_owned();
    let mut n: usize = 0;
    while let Some(r) = rows.next()? {
        let seq: i64 = r.get(0)?;
        let at: String = r.get(1)?;
        let actor: String = r.get(2)?;
        let action: String = r.get(3)?;
        let subject: String = r.get(4)?;
        let details: String = r.get(5)?;
        let prev_hash: String = r.get(6)?;
        let hash: String = r.get(7)?;
        let version: i64 = r.get(8)?;
        let expected_seq = i64::try_from(n).map_err(|_| VaultError::Corrupted)? + 1;
        // Cada entrada se comprueba con la versión que declara; una que esta compilación no
        // conoce no se puede dar por buena.
        let h = Fields {
            prev: &prev,
            seq,
            at: &at,
            actor: &actor,
            action: &action,
            subject: &subject,
            details: &details,
        }
        .hash(version);
        if seq != expected_seq || prev_hash != prev || h.as_deref() != Some(hash.as_str()) {
            return Err(VaultError::AuditTampered(seq));
        }
        prev = hash;
        n += 1;
    }
    Ok(n)
}

/// Añade una entrada como lo hacían las versiones anteriores a la migración 0065 (hash v1 y sin
/// escribir `hash_version`): para probar almacenes y copias que ya existen.
#[cfg(test)]
pub(crate) fn append_v1(
    conn: &Connection,
    at: &str,
    actor: Actor,
    action: &str,
    subject: &str,
    details: &str,
) -> String {
    let (seq, prev) = match head(conn).unwrap() {
        Some((s, h)) => (s + 1, h),
        None => (1, GENESIS_HASH.to_owned()),
    };
    // El hash v1, escrito aparte del de producción: los campos unidos con saltos de línea.
    let joined = [
        prev.as_str(),
        &seq.to_string(),
        at,
        actor.as_str(),
        action,
        subject,
        details,
    ]
    .join("\n");
    let hash = hex::encode(Sha256::digest(joined.as_bytes()));
    conn.execute(
        "INSERT INTO audit_log(seq, at, actor, action, subject, details, prev_hash, hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            seq,
            at,
            actor.as_str(),
            action,
            subject,
            details,
            prev,
            hash
        ],
    )
    .unwrap();
    hash
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use serde_json::json;
    use taxpain_secrets::Key256;

    #[test]
    fn la_consulta_filtra_por_fecha_accion_y_texto_y_pagina() {
        let c = conn();
        append(
            &c,
            "2026-01-10T09:00:00Z",
            Actor::User,
            "invoice.issue",
            "F-1",
            &json!({}),
        )
        .unwrap();
        append(
            &c,
            "2026-02-10T09:00:00Z",
            Actor::User,
            "invoice.void",
            "F-1",
            &json!({}),
        )
        .unwrap();
        append(
            &c,
            "2026-03-10T09:00:00Z",
            Actor::System,
            "journal.manual",
            "Nómina 50%",
            &json!({"entry": 3}),
        )
        .unwrap();
        append(
            &c,
            "2026-04-10T09:00:00Z",
            Actor::User,
            "invoiceX",
            "otra",
            &json!({}),
        )
        .unwrap();
        let todo = consultar(&c, &Filtro::default()).unwrap();
        assert_eq!(todo.total, 4);
        assert_eq!(
            todo.entradas[0].seq, 4,
            "de la más reciente a la más antigua"
        );
        let fam = consultar(
            &c,
            &Filtro {
                accion: Some("invoice".into()),
                ..Filtro::default()
            },
        )
        .unwrap();
        assert_eq!(
            fam.entradas.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![2, 1]
        );
        let fechas = consultar(
            &c,
            &Filtro {
                desde: Some("2026-02-01".into()),
                hasta: Some("2026-03-10".into()),
                ..Filtro::default()
            },
        )
        .unwrap();
        assert_eq!(fechas.total, 2);
        let texto = consultar(
            &c,
            &Filtro {
                texto: Some("50%".into()),
                ..Filtro::default()
            },
        )
        .unwrap();
        assert_eq!(texto.total, 1);
        let actor = consultar(
            &c,
            &Filtro {
                actor: Some("system".into()),
                ..Filtro::default()
            },
        )
        .unwrap();
        assert_eq!(actor.entradas[0].action, "journal.manual");
        let p1 = consultar(
            &c,
            &Filtro {
                limite: Some(3),
                ..Filtro::default()
            },
        )
        .unwrap();
        assert_eq!(p1.entradas.len(), 3);
        assert_eq!(p1.siguiente, Some(2));
        let p2 = consultar(
            &c,
            &Filtro {
                limite: Some(3),
                antes_de: p1.siguiente,
                ..Filtro::default()
            },
        )
        .unwrap();
        assert_eq!(
            p2.entradas.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(p2.siguiente, None);
        assert_eq!(p2.total, 4);
        let a = acciones(&c).unwrap();
        assert_eq!(a.len(), 4);
        assert!(a.contains(&("invoice.issue".to_owned(), 1)));
    }

    fn conn() -> Connection {
        db::open_in_memory(&Key256::random().unwrap()).unwrap()
    }

    fn fields<'a>(subject: &'a str, details: &'a str) -> Fields<'a> {
        Fields {
            prev: GENESIS_HASH,
            seq: 1,
            at: "2026-10-13T10:00:00Z",
            actor: "user",
            action: "certificate.import",
            subject,
            details,
        }
    }

    /// Los dos formatos, contra valores calculados fuera de este código (Python, hashlib): si
    /// cambia cualquiera de los dos, las cadenas que ya existen dejarían de verificar.
    #[test]
    fn hash_v1_y_v2_no_cambian() {
        let f = fields("abc", r#"{"kind":"x"}"#);
        assert_eq!(
            f.hash(1).unwrap(),
            "9982d8bfe06ac3e3f26a2aeb9c56b0cfb357c39f0693bd43ea4fd85c39c707bd"
        );
        assert_eq!(
            f.hash(2).unwrap(),
            "1dbc24120a68c37e70443544dc9af355958c159af9b16c1bb6b05e198eb5d8f8"
        );
        assert_eq!(f.hash(0), None);
        assert_eq!(f.hash(3), None);
    }

    /// El fallo de v1: un salto de línea dentro de un campo se confunde con el separador. En v2,
    /// con la longitud de cada campo delante, no.
    #[test]
    fn v2_distingue_entradas_que_v1_confunde() {
        let a = fields("a\nb", "c");
        let b = fields("a", "b\nc");
        assert_eq!(a.hash(1), b.hash(1), "v1 las confunde");
        assert_ne!(a.hash(2), b.hash(2));
        // Tampoco se confunde un campo vacío con su vecino.
        assert_ne!(fields("", "ab").hash(2), fields("a", "b").hash(2));
        assert_ne!(fields("ab", "").hash(2), fields("a", "b").hash(2));
    }

    /// Un almacén de antes de la migración 0065 (entradas v1, sin la columna) sigue verificando
    /// después de migrar, y la cadena sigue con entradas v2. También si un programa anterior que
    /// seguía abierto añade después otra v1 (no escribe la columna: vale 1 por defecto).
    #[test]
    fn un_almacen_con_entradas_v1_sigue_verificando_y_se_mezcla_con_v2() {
        let c = db::open_in_memory_at_version(&Key256::random().unwrap(), 64).unwrap();
        let h1 = append_v1(&c, "t1", Actor::User, "a", "s", "{}");
        let h2 = append_v1(&c, "t2", Actor::System, "b", "con\nsalto", r#"{"n":1}"#);
        db::migrate_pending(&c).unwrap();
        assert_eq!(db::user_version(&c).unwrap(), db::schema_version());
        assert_eq!(verify(&c).unwrap(), 2, "las v1 quedan como están");
        assert_eq!(head(&c).unwrap(), Some((2, h2.clone())));

        let h3 = append(&c, "t3", Actor::User, "c", "s", &json!({"n": 2})).unwrap();
        assert_eq!(verify(&c).unwrap(), 3);
        let all = entries(&c).unwrap();
        assert_eq!(all[0].hash, h1);
        assert_eq!(all[2].prev_hash, h2, "la v2 encadena con la última v1");
        // La nueva es v2: su hash no es el que daría v1.
        let third = Fields {
            prev: &h2,
            seq: 3,
            at: "t3",
            actor: "user",
            action: "c",
            subject: "s",
            details: r#"{"n":2}"#,
        };
        assert_eq!(third.hash(2).unwrap(), h3);
        assert_ne!(third.hash(1).unwrap(), h3);

        append_v1(&c, "t4", Actor::User, "d", "s", "{}");
        append(&c, "t5", Actor::User, "e", "s", &json!({})).unwrap();
        assert_eq!(verify(&c).unwrap(), 5);
        let versions: Vec<i64> = c
            .prepare("SELECT hash_version FROM audit_log ORDER BY seq")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert_eq!(versions, [1, 1, 2, 1, 2]);
    }

    /// Una entrada v2 manipulada se detecta, y también una a la que se le cambia la versión que
    /// declara o que declara una que no existe.
    #[test]
    fn manipular_una_entrada_v2_o_su_version_se_detecta() {
        let tampered = |sql: &str| {
            let c = conn();
            append_v1(&c, "t1", Actor::User, "a", "s", "{}");
            append(&c, "t2", Actor::User, "b", "s", &json!({})).unwrap();
            append(&c, "t3", Actor::User, "c", "s", &json!({})).unwrap();
            assert_eq!(verify(&c).unwrap(), 3);
            c.execute_batch("DROP TRIGGER audit_log_no_update").unwrap();
            c.execute_batch(sql).unwrap();
            verify(&c)
        };
        assert!(matches!(
            tampered("UPDATE audit_log SET subject = 'otro' WHERE seq = 2"),
            Err(VaultError::AuditTampered(2))
        ));
        assert!(matches!(
            tampered("UPDATE audit_log SET hash_version = 1 WHERE seq = 2"),
            Err(VaultError::AuditTampered(2))
        ));
        assert!(matches!(
            tampered("UPDATE audit_log SET hash_version = 2 WHERE seq = 1"),
            Err(VaultError::AuditTampered(1))
        ));
        assert!(matches!(
            tampered("UPDATE audit_log SET hash_version = 3 WHERE seq = 3"),
            Err(VaultError::AuditTampered(3))
        ));
    }

    #[test]
    fn chain_verifies_and_is_append_only() {
        let c = conn();
        let h1 = append(
            &c,
            "2026-10-13T10:00:00Z",
            Actor::User,
            "a",
            "s",
            &json!({}),
        )
        .unwrap();
        let h2 = append(
            &c,
            "2026-10-13T10:00:01Z",
            Actor::System,
            "b",
            "s",
            &json!({"n": 1}),
        )
        .unwrap();
        assert_ne!(h1, h2);
        assert_eq!(verify(&c).unwrap(), 2);
        assert_eq!(entries(&c).unwrap()[1].prev_hash, h1);
        assert!(
            c.execute("UPDATE audit_log SET action='x' WHERE seq=1", [])
                .is_err()
        );
        assert!(c.execute("DELETE FROM audit_log WHERE seq=1", []).is_err());
    }

    #[test]
    fn tampering_is_detected() {
        let c = conn();
        append(&c, "t1", Actor::User, "a", "s", &json!({})).unwrap();
        append(&c, "t2", Actor::User, "b", "s", &json!({})).unwrap();
        // Simula una manipulación externa saltándose los triggers.
        c.execute_batch(
            "DROP TRIGGER audit_log_no_update;
             UPDATE audit_log SET details='{\"x\":1}' WHERE seq=1;",
        )
        .unwrap();
        assert!(matches!(verify(&c), Err(VaultError::AuditTampered(1))));
    }

    #[test]
    fn anchor_detects_truncation_and_divergence() {
        let c = conn();
        append(&c, "t1", Actor::User, "a", "s", &json!({})).unwrap();
        let h2 = append(&c, "t2", Actor::User, "b", "s", &json!({})).unwrap();
        let anchor = Anchor { seq: 2, hash: h2 };
        assert_eq!(Anchor::decode(&anchor.encode()), Some(anchor.clone()));
        assert_eq!(Anchor::decode("otra cosa\n2 abc\n"), None);
        assert_eq!(check_anchor(&c, &anchor).unwrap(), None);
        // Más entradas después del ancla: bien.
        append(&c, "t3", Actor::User, "c", "s", &json!({})).unwrap();
        assert_eq!(check_anchor(&c, &anchor).unwrap(), None);
        // Una copia más antigua (sólo la 1.ª entrada): retroceso.
        let old = conn();
        append(&old, "t1", Actor::User, "a", "s", &json!({})).unwrap();
        assert_eq!(
            check_anchor(&old, &anchor).unwrap(),
            Some(Rollback {
                anchored_seq: 2,
                current_seq: 1
            })
        );
        // Otra cadena con las mismas posiciones: también.
        let other = conn();
        append(&other, "t1", Actor::User, "x", "s", &json!({})).unwrap();
        append(&other, "t2", Actor::User, "y", "s", &json!({})).unwrap();
        assert!(check_anchor(&other, &anchor).unwrap().is_some());
    }
}
