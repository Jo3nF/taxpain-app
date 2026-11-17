//! Fachada del almacén: BD cifrada + certificados sellados + auditoría.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::audit::{self, Actor, AnchorProblem, PendingRollback};
use crate::certificate::{
    CertificateInfo, CertificateKind, IssuerKind, decode_secret, encode_secret, parse_pkcs12,
};
use crate::db;
use crate::error::{Result, VaultError};
use crate::identity::ClientIdentity;
use crate::{backup, recovery, seal};
use taxpain_secrets::UserPresence;
use taxpain_secrets::keys;
use taxpain_secrets::{KeyId, KeyProvider};

pub struct Vault {
    conn: Connection,
    keys: Arc<dyn KeyProvider>,
    presence: Arc<dyn UserPresence>,
    /// Carpeta de datos (`taxpain.db`, `audit.head`, `recovery.sealed`); `None` en memoria.
    dir: Option<PathBuf>,
    /// `audit.head` (ancla de la auditoría fuera de la BD); `None` en memoria y en los
    /// almacenes abiertos con [`Vault::open_existing`], que no la mueven.
    anchor_path: Option<PathBuf>,
    /// Última cabeza escrita en el ancla (para no reescribirla si no cambia).
    anchored: RefCell<Option<(i64, String)>>,
    /// Retroceso sin reconocer (la BD está por detrás del ancla): el detectado al abrir o el que
    /// quedó guardado en la BD en una apertura anterior (ver [`Vault::audit_rollback_pending`]).
    rollback: Option<audit::Rollback>,
    /// Aviso detectado al abrir que no se ha guardado en la BD ([`Vault::open_existing`] no
    /// escribe nada al abrir).
    unsaved: Option<PendingRollback>,
    /// El usuario ha reconocido el aviso en esta sesión.
    acknowledged: Cell<bool>,
}

/// Qué hace una apertura con el ancla de la auditoría.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AnchorMode {
    /// La comprueba, anota y guarda lo que vea, y la mantiene al día.
    Sync,
    /// Sólo la comprueba: ni la mueve ni escribe nada en la BD.
    ReadOnly,
}

/// Petición de uso de un certificado pendiente de la confirmación del usuario.
pub struct IdentityRequest {
    fingerprint: String,
    purpose: String,
    reason: String,
    presence: Arc<dyn UserPresence>,
}

impl std::fmt::Debug for IdentityRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentityRequest")
            .field("fingerprint", &self.fingerprint)
            .field("purpose", &self.purpose)
            .finish_non_exhaustive()
    }
}

/// Petición confirmada por el usuario (sólo se obtiene con [`IdentityRequest::confirm`]).
#[derive(Debug)]
pub struct ConfirmedIdentity(IdentityRequest);

impl IdentityRequest {
    /// Paso 2: pide la confirmación (Touch ID). No necesita el almacén.
    pub fn confirm(self) -> Result<ConfirmedIdentity> {
        self.presence.confirm(&self.reason)?;
        Ok(ConfirmedIdentity(self))
    }
}

/// Petición de borrar un certificado, pendiente de la confirmación del usuario (ver
/// [`Vault::prepare_delete`]).
#[derive(Debug)]
pub struct DeleteRequest(IdentityRequest);

/// Borrado confirmado por el usuario: sólo lo crea [`DeleteRequest::confirm`].
#[derive(Debug)]
pub struct ConfirmedDelete(IdentityRequest);

impl DeleteRequest {
    /// Paso 2: pide la confirmación (Touch ID). No necesita el almacén.
    pub fn confirm(self) -> Result<ConfirmedDelete> {
        self.0.presence.confirm(&self.0.reason)?;
        Ok(ConfirmedDelete(self.0))
    }
}

impl Drop for Vault {
    fn drop(&mut self) {
        if let Err(e) = self.sync_anchor() {
            tracing::warn!(error = %e, "no se pudo actualizar el ancla de la auditoría");
        }
    }
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("keys_secure", &self.keys.is_secure())
            .finish_non_exhaustive()
    }
}

fn now_str(now: OffsetDateTime) -> String {
    now.format(&Rfc3339).unwrap_or_default()
}

fn seal_context(fingerprint: &str) -> Vec<u8> {
    format!("certificate:{fingerprint}").into_bytes()
}

const DB_FILE: &str = "taxpain.db";
const ANCHOR_FILE: &str = "audit.head";
const OPEN_REASON: &str = "Abrir los datos de TaxPain";

/// Escribe en `dir/audit.head` la cabeza `head` de la auditoría. Escritura atómica: fichero
/// temporal y renombrado.
pub(crate) fn write_anchor(dir: &Path, head: &(i64, String)) -> Result<()> {
    let path = dir.join(ANCHOR_FILE);
    let text = audit::Anchor {
        seq: head.0,
        hash: head.1.clone(),
    }
    .encode();
    let tmp = path.with_extension("head.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// El aviso de auditoría sin reconocer que hay guardado en la BD. Un valor que no se entiende no
/// impide abrir: se avisa en el registro y se trata como si no hubiera.
fn read_pending(conn: &Connection) -> Result<Option<PendingRollback>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            [audit::PENDING_KEY],
            |r| r.get(0),
        )
        .optional()?;
    Ok(raw.and_then(|v| match serde_json::from_str(&v) {
        Ok(p) => Some(p),
        Err(e) => {
            tracing::warn!(error = %e, "el aviso de auditoría pendiente no se entiende");
            None
        }
    }))
}

fn write_pending(conn: &Connection, p: &PendingRollback) -> Result<()> {
    let v = serde_json::to_string(p).map_err(|e| VaultError::InvalidInput(e.to_string()))?;
    conn.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![audit::PENDING_KEY, v],
    )?;
    Ok(())
}

impl Vault {
    /// Abre (o crea) `dir/taxpain.db` con la clave `db-master` y verifica la auditoría.
    ///
    /// Si ya hay datos y el Llavero no tiene la clave, no se crea otra (abriría con una clave
    /// que no es la suya y parecería un almacén dañado): [`VaultError::KeyMissing`], con
    /// `recoverable` si junto a los datos está el código de recuperación
    /// ([`Vault::open_recovered`]).
    pub fn open(
        dir: &Path,
        keys: Arc<dyn KeyProvider>,
        presence: Arc<dyn UserPresence>,
    ) -> Result<Vault> {
        std::fs::create_dir_all(dir)?;
        keys::restrict_permissions(dir, 0o700)?;
        let key = if dir.join(DB_FILE).is_file() {
            keys.get(KeyId::DbMaster, OPEN_REASON)?
                .ok_or(VaultError::KeyMissing {
                    recoverable: recovery::exists(dir),
                })?
        } else {
            keys.get_or_create(KeyId::DbMaster, OPEN_REASON)?
        };
        let conn = db::open(&dir.join(DB_FILE), &key)?;
        let vault = Self::from_connection(dir, conn, keys, presence, AnchorMode::Sync)?;
        // Datos anteriores a los códigos de recuperación (o fichero borrado): se repone. Si no
        // se puede (disco lleno, carpeta de sólo lectura), se abre igual y se reintenta otro día.
        if let Err(e) = backup::ensure_recovery(&vault, &key) {
            tracing::warn!(error = %e, "no se pudo escribir el código de recuperación");
        }
        Ok(vault)
    }

    /// Abre `dir` sin el Llavero, con la contraseña de las copias y el código de recuperación
    /// (`recovery.sealed`): desella `db-master`, la vuelve a dejar en el proveedor de claves y
    /// abre. Si el proveedor ya tiene otra `db-master` (un Mac con otras empresas), la BD se
    /// recifra con ésa y el código de recuperación se reescribe. La contraseña no se guarda.
    pub fn open_recovered(
        dir: &Path,
        keys: Arc<dyn KeyProvider>,
        presence: Arc<dyn UserPresence>,
        passphrase: &str,
    ) -> Result<Vault> {
        if !dir.join(DB_FILE).is_file() {
            return Err(VaultError::NotFound);
        }
        let recovered = recovery::unseal(dir, passphrase)?;
        let mode = match keys.get(KeyId::DbMaster, OPEN_REASON)? {
            None => {
                keys.put(KeyId::DbMaster, &recovered)?;
                "llavero"
            }
            Some(current) if current.expose() == recovered.expose() => "igual",
            Some(current) => {
                db::rekey(&dir.join(DB_FILE), &recovered, &current)?;
                let _ = std::fs::remove_file(recovery::path(dir));
                "recifrado"
            }
        };
        let vault = Self::open(dir, keys, presence)?;
        audit::append(
            &vault.conn,
            &now_str(OffsetDateTime::now_utc()),
            Actor::User,
            "vault.recovered",
            recovery::FILE,
            &json!({ "mode": mode }),
        )?;
        vault.sync_anchor()?;
        Ok(vault)
    }

    /// Abre un almacén **que ya existe** sin cambiar nada por el hecho de abrirlo: para quien
    /// consulta los datos desde fuera de la app (el servidor MCP).
    ///
    /// A diferencia de [`Vault::open`]:
    /// - no crea nada: sin `taxpain.db`, [`VaultError::NotFound`]; sin la clave en el Llavero,
    ///   [`VaultError::KeyMissing`] (nunca crea otra clave ni la carpeta);
    /// - no migra: si el esquema no es el de esta compilación, [`VaultError::InvalidInput`] con
    ///   lo que hay que actualizar (la app o este programa);
    /// - no escribe el código de recuperación (`recovery.sealed`);
    /// - no mueve el ancla de la auditoría (`audit.head`), ni al abrir ni al cerrar, y si la BD
    ///   está por detrás de ella (o falta) no lo anota ni lo da por visto: lo dice
    ///   [`Vault::audit_rollback_pending`] y lo volverá a ver, anotará y enseñará la app la
    ///   próxima vez que abra la empresa.
    ///
    /// El almacén que devuelve sí se puede escribir (los borradores que deja el asistente): lo
    /// que se anote en la auditoría queda en la BD, y el ancla la adelantará la app.
    pub fn open_existing(
        dir: &Path,
        keys: Arc<dyn KeyProvider>,
        presence: Arc<dyn UserPresence>,
    ) -> Result<Vault> {
        let path = dir.join(DB_FILE);
        if !path.is_file() {
            return Err(VaultError::NotFound);
        }
        let key = keys
            .get(KeyId::DbMaster, OPEN_REASON)?
            .ok_or(VaultError::KeyMissing {
                recoverable: recovery::exists(dir),
            })?;
        let conn = db::open_existing(&path, &key)?;
        Self::from_connection(dir, conn, keys, presence, AnchorMode::ReadOnly)
    }

    /// Verifica la auditoría de `conn`, la compara con el ancla de `dir` y monta el almacén.
    ///
    /// Si la BD está por detrás del ancla, o tiene entradas de auditoría y no hay ancla válida
    /// (alguien la ha borrado o estropeado: sin ella un retroceso no se vería), queda anotado en
    /// la cadena y guardado en la BD como aviso pendiente, en la misma transacción, hasta que el
    /// usuario lo reconozca ([`Vault::acknowledge_audit_rollback`]): da igual quién abra (la
    /// app, una tarea de fondo), el aviso no se pierde aunque el ancla avance. Un almacén recién
    /// creado no tiene entradas, y una copia recién restaurada trae su ancla
    /// ([`write_anchor`]): ninguno de los dos avisa.
    fn from_connection(
        dir: &Path,
        conn: Connection,
        keys: Arc<dyn KeyProvider>,
        presence: Arc<dyn UserPresence>,
        mode: AnchorMode,
    ) -> Result<Vault> {
        audit::verify(&conn)?;
        if !keys.is_secure() {
            tracing::warn!("las claves no están protegidas por el sistema (modo desarrollo)");
        }
        let anchor_path = dir.join(ANCHOR_FILE);
        let current_seq = audit::head(&conn)?.map_or(0, |(s, _)| s);
        let detected: Option<(AnchorProblem, Option<i64>)> =
            match std::fs::read_to_string(&anchor_path) {
                Ok(text) => match audit::Anchor::decode(&text) {
                    Some(anchor) => audit::check_anchor(&conn, &anchor)?
                        .map(|r| (AnchorProblem::Rollback, Some(r.anchored_seq))),
                    None if current_seq > 0 => Some((AnchorProblem::AnchorInvalid, None)),
                    None => None,
                },
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    (current_seq > 0).then_some((AnchorProblem::AnchorMissing, None))
                }
                Err(e) => return Err(e.into()),
            };
        let mut pending = read_pending(&conn)?;
        let mut unsaved = None;
        if let Some((kind, anchored_seq)) = detected {
            tracing::warn!(
                problema = kind.action(),
                anchored = anchored_seq,
                current = current_seq,
                "la auditoría no coincide con su ancla: la base de datos está por detrás de la \
                 última auditoría vista o el ancla falta"
            );
            let at = now_str(OffsetDateTime::now_utc());
            let found = PendingRollback {
                kind,
                anchored_seq,
                current_seq,
                detected_at: at.clone(),
            };
            // Un retroceso ya pendiente no se rebaja a «falta el ancla» (borrarla después de
            // retroceder la BD no tapa el aviso).
            let keep_old = kind != AnchorProblem::Rollback
                && pending
                    .as_ref()
                    .is_some_and(|p| p.kind == AnchorProblem::Rollback);
            match mode {
                AnchorMode::Sync => {
                    // Queda en la propia cadena y como aviso pendiente, todo o nada; el ancla
                    // pasa a la nueva cabeza al terminar de abrir.
                    let tx = conn.unchecked_transaction()?;
                    audit::append(
                        &tx,
                        &at,
                        Actor::System,
                        kind.action(),
                        ANCHOR_FILE,
                        &json!({ "anchored_seq": anchored_seq, "current_seq": current_seq }),
                    )?;
                    if !keep_old {
                        write_pending(&tx, &found)?;
                    }
                    tx.commit()?;
                    if !keep_old {
                        pending = Some(found);
                    }
                }
                AnchorMode::ReadOnly => {
                    if !keep_old {
                        unsaved = Some(found);
                    }
                }
            }
        }
        let rollback = unsaved
            .as_ref()
            .or(pending.as_ref())
            .and_then(PendingRollback::rollback);
        let vault = Vault {
            conn,
            keys,
            presence,
            dir: Some(dir.to_owned()),
            anchor_path: (mode == AnchorMode::Sync).then_some(anchor_path),
            anchored: RefCell::new(None),
            rollback,
            unsaved,
            acknowledged: Cell::new(false),
        };
        vault.sync_anchor()?;
        Ok(vault)
    }

    /// Carpeta de datos del almacén (`None` en memoria).
    pub fn dir(&self) -> Option<&Path> {
        self.dir.as_deref()
    }

    pub(crate) fn keys(&self) -> &Arc<dyn KeyProvider> {
        &self.keys
    }

    /// Si la BD ha estado por detrás del ancla de la auditoría (le faltaban entradas que ya se
    /// habían visto) y el usuario aún no lo ha reconocido, cuánto. Es lo que había al abrir este
    /// almacén: lo detectado entonces o lo que quedó guardado en una apertura anterior (de la
    /// app, de una tarea de fondo…). Para leerlo de la BD en el momento, con los avisos de ancla
    /// ausente o ilegible, [`Vault::audit_rollback_pending`].
    pub fn audit_rollback(&self) -> Option<&audit::Rollback> {
        if self.acknowledged.get() {
            return None;
        }
        self.rollback.as_ref()
    }

    /// El aviso de auditoría que el usuario aún no ha reconocido (retroceso, ancla ausente o
    /// ancla ilegible), leído ahora de la BD: también ve el que haya guardado otro proceso
    /// después de abrir éste. En un almacén abierto con [`Vault::open_existing`], que no guarda
    /// nada al abrir, incluye lo que se detectó entonces.
    pub fn audit_rollback_pending(&self) -> Result<Option<PendingRollback>> {
        if let Some(p) = read_pending(&self.conn)? {
            return Ok(Some(p));
        }
        if self.acknowledged.get() {
            return Ok(None);
        }
        Ok(self.unsaved.clone())
    }

    /// El usuario ha visto el aviso de [`Vault::audit_rollback_pending`]: se borra de la BD y
    /// queda anotado en la auditoría quién lo reconoció y cuándo. Devuelve `false` si no había
    /// ninguno guardado. Sólo se ha de llamar por una acción del usuario en la interfaz.
    pub fn acknowledge_audit_rollback(&self, now: OffsetDateTime) -> Result<bool> {
        let Some(p) = read_pending(&self.conn)? else {
            return Ok(false);
        };
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM settings WHERE key = ?1", [audit::PENDING_KEY])?;
        audit::append(
            &tx,
            &now_str(now),
            Actor::User,
            "audit.rollback_acknowledged",
            ANCHOR_FILE,
            &json!({
                "kind": p.kind,
                "anchored_seq": p.anchored_seq,
                "current_seq": p.current_seq,
                "detected_at": p.detected_at,
            }),
        )?;
        tx.commit()?;
        self.acknowledged.set(true);
        Ok(true)
    }

    /// Escribe en el ancla la cabeza actual de la auditoría si ha cambiado. No hace nada en
    /// mitad de una transacción (la cabeza aún no es definitiva) ni con el almacén en memoria.
    pub fn sync_anchor(&self) -> Result<()> {
        let Some(path) = &self.anchor_path else {
            return Ok(());
        };
        if !self.conn.is_autocommit() {
            return Ok(());
        }
        let Some(head) = audit::head(&self.conn)? else {
            return Ok(());
        };
        if self.anchored.borrow().as_ref() == Some(&head) {
            return Ok(());
        }
        let Some(dir) = path.parent() else {
            return Ok(());
        };
        write_anchor(dir, &head)?;
        *self.anchored.borrow_mut() = Some(head);
        Ok(())
    }

    /// Almacén en memoria (tests).
    pub fn open_in_memory(
        keys: Arc<dyn KeyProvider>,
        presence: Arc<dyn UserPresence>,
    ) -> Result<Vault> {
        let key = keys.get_or_create(KeyId::DbMaster, "")?;
        let conn = db::open_in_memory(&key)?;
        Ok(Vault {
            conn,
            keys,
            presence,
            dir: None,
            anchor_path: None,
            anchored: RefCell::new(None),
            rollback: None,
            unsaved: None,
            acknowledged: Cell::new(false),
        })
    }

    pub fn keys_are_secure(&self) -> bool {
        self.keys.is_secure()
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// Importa un `.p12/.pfx`. La contraseña no se guarda. Si ya existía, devuelve el existente;
    /// y si lo guardado no se puede abrir con la clave de sellado de este Mac (los datos vienen
    /// de otro, cuya clave `cert-seal` se quedó en su Llavero), lo vuelve a sellar con la de
    /// éste y lo anota (`certificate.reseal`): reimportar el mismo fichero lo arregla.
    pub fn import_certificate(
        &self,
        p12: &[u8],
        passphrase: &str,
        now: OffsetDateTime,
    ) -> Result<CertificateInfo> {
        let parsed = parse_pkcs12(p12, passphrase, now)?;
        let info = parsed.info.clone();
        let existing = self.certificate(&info.fingerprint_sha256)?;
        // Importar es el único sitio donde se crea la clave de sellado si falta.
        let seal_key = self.keys.get_or_create(
            KeyId::CertSeal,
            &format!("Guardar el certificado de {}", display_holder(&info)),
        )?;
        let context = seal_context(&info.fingerprint_sha256);
        let secret = encode_secret(&parsed.key_pkcs8, &parsed.chain_der);
        let at = now_str(now);
        if let Some(existing) = existing {
            let stored: Option<Vec<u8>> = self
                .conn
                .query_row(
                    "SELECT s.sealed FROM certificate_secret s
                     JOIN certificate_ref c ON c.id = s.certificate_id
                     WHERE c.fingerprint_sha256 = ?1",
                    [&info.fingerprint_sha256],
                    |r| r.get(0),
                )
                .optional()?;
            let usable = stored.as_deref().is_some_and(|sealed| {
                seal::unseal(&seal_key, &context, sealed)
                    .and_then(|s| decode_secret(s.expose()).map(|_| ()))
                    .is_ok()
            });
            if usable {
                return Ok(existing);
            }
            let sealed = seal::seal(&seal_key, &context, secret.expose())?;
            let tx = self.conn.unchecked_transaction()?;
            tx.execute(
                "DELETE FROM certificate_secret WHERE certificate_id =
                   (SELECT id FROM certificate_ref WHERE fingerprint_sha256 = ?1)",
                [&info.fingerprint_sha256],
            )?;
            tx.execute(
                "INSERT INTO certificate_secret(certificate_id, sealed)
                 SELECT id, ?2 FROM certificate_ref WHERE fingerprint_sha256 = ?1",
                params![info.fingerprint_sha256, sealed],
            )?;
            audit::append(
                &tx,
                &at,
                Actor::User,
                "certificate.reseal",
                &info.fingerprint_sha256,
                &json!({ "had_secret": stored.is_some() }),
            )?;
            tx.commit()?;
            tracing::info!(
                fingerprint = %info.fingerprint_sha256,
                "certificado vuelto a sellar con la clave de este Mac"
            );
            return Ok(existing);
        }
        let sealed = seal::seal(&seal_key, &context, secret.expose())?;

        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO certificate_ref(fingerprint_sha256, subject_dn, issuer_dn, serial,
               valid_from, valid_to, kind, issuer_kind, holder_name, holder_nif, entity_nif,
               entity_name, key_algorithm, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                info.fingerprint_sha256,
                info.subject_dn,
                info.issuer_dn,
                info.serial,
                info.valid_from,
                info.valid_to,
                info.kind.as_str(),
                info.issuer_kind.as_str(),
                info.holder_name,
                info.holder_nif,
                info.entity_nif,
                info.entity_name,
                info.key_algorithm,
                at,
            ],
        )?;
        let id = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO certificate_secret(certificate_id, sealed) VALUES (?1, ?2)",
            params![id, sealed],
        )?;
        // Dentro de la transacción (la auditoría usa SAVEPOINT): si no se puede anotar, el
        // certificado tampoco se guarda; si no, quedaría para siempre sin rastro.
        audit::append(
            &tx,
            &at,
            Actor::User,
            "certificate.import",
            &info.fingerprint_sha256,
            &json!({ "kind": info.kind.as_str(), "holder_nif": info.holder_nif,
                     "entity_nif": info.entity_nif, "valid_to": info.valid_to }),
        )?;
        tx.commit()?;
        tracing::info!(fingerprint = %info.fingerprint_sha256, "certificado importado");
        Ok(info)
    }

    pub fn certificates(&self) -> Result<Vec<CertificateInfo>> {
        let mut stmt = self
            .conn
            .prepare(&format!("{SELECT_INFO} ORDER BY valid_to DESC"))?;
        let rows = stmt.query_map([], row_to_info)?;
        Ok(rows.collect::<std::result::Result<_, _>>()?)
    }

    pub fn certificate(&self, fingerprint: &str) -> Result<Option<CertificateInfo>> {
        Ok(self
            .conn
            .query_row(
                &format!("{SELECT_INFO} WHERE fingerprint_sha256 = ?1"),
                [fingerprint],
                row_to_info,
            )
            .optional()?)
    }

    /// Desella el certificado para una conexión mTLS, con confirmación de presencia.
    ///
    /// Equivale a [`Vault::prepare_identity`] + [`IdentityRequest::confirm`] +
    /// [`Vault::finish_identity`]. La app usa los tres pasos para no tener el almacén bloqueado
    /// mientras el usuario responde a Touch ID.
    pub fn client_identity(
        &self,
        fingerprint: &str,
        purpose: &str,
        now: OffsetDateTime,
    ) -> Result<ClientIdentity> {
        let confirmed = self
            .prepare_identity(fingerprint, purpose, now)?
            .confirm()?;
        self.finish_identity(confirmed, now)
    }

    /// Paso 1: comprueba el certificado y prepara la petición de presencia (sin pedirla).
    pub fn prepare_identity(
        &self,
        fingerprint: &str,
        purpose: &str,
        now: OffsetDateTime,
    ) -> Result<IdentityRequest> {
        let info = self.certificate(fingerprint)?.ok_or(VaultError::NotFound)?;
        if now_str(now) >= info.valid_to {
            return Err(VaultError::ExpiredCertificate(
                info.valid_to[..10].to_owned(),
            ));
        }
        Ok(IdentityRequest {
            fingerprint: fingerprint.to_owned(),
            purpose: purpose.to_owned(),
            reason: format!("{purpose} con el certificado de {}", display_holder(&info)),
            presence: self.presence.clone(),
        })
    }

    /// Paso 3: con la presencia confirmada, desella la clave y lo anota en la auditoría.
    pub fn finish_identity(
        &self,
        req: ConfirmedIdentity,
        now: OffsetDateTime,
    ) -> Result<ClientIdentity> {
        let IdentityRequest {
            fingerprint,
            purpose,
            reason,
            ..
        } = req.0;
        // Nunca se crea aquí la clave de sellado: si falta (los datos vienen de otro Mac y la
        // clave se quedó en su Llavero), una nueva no abriría lo sellado con la otra y además
        // dejaría de saberse que faltaba.
        let seal_key = self
            .keys
            .get(KeyId::CertSeal, &reason)?
            .ok_or(VaultError::CertificateSealedElsewhere)?;
        let sealed: Option<Vec<u8>> = self
            .conn
            .query_row(
                "SELECT s.sealed FROM certificate_secret s
                 JOIN certificate_ref c ON c.id = s.certificate_id
                 WHERE c.fingerprint_sha256 = ?1",
                [&fingerprint],
                |r| r.get(0),
            )
            .optional()?;
        // Sellado con otra clave (la de otro Mac) o sin su parte secreta: lo mismo para el
        // usuario, y se arregla igual.
        let (key, chain) = sealed
            .ok_or(VaultError::CertificateSealedElsewhere)
            .and_then(|sealed| {
                seal::unseal(&seal_key, &seal_context(&fingerprint), &sealed)
                    .and_then(|secret| decode_secret(secret.expose()))
                    .map_err(|_| VaultError::CertificateSealedElsewhere)
            })?;
        let at = now_str(now);
        self.conn.execute(
            "UPDATE certificate_ref SET last_used_at = ?1 WHERE fingerprint_sha256 = ?2",
            params![at, fingerprint],
        )?;
        audit::append(
            &self.conn,
            &at,
            Actor::User,
            "certificate.use",
            &fingerprint,
            &json!({ "purpose": purpose }),
        )?;
        Ok(ClientIdentity::new(fingerprint, key, chain))
    }

    /// Borra un certificado (con confirmación del usuario).
    ///
    /// Equivale a [`Vault::prepare_delete`] + [`DeleteRequest::confirm`] +
    /// [`Vault::finish_delete`]. La app usa los tres pasos para no tener el almacén bloqueado
    /// (hasta dos minutos) mientras el usuario responde a Touch ID.
    pub fn delete_certificate(&self, fingerprint: &str, now: OffsetDateTime) -> Result<()> {
        let confirmed = self.prepare_delete(fingerprint)?.confirm()?;
        self.finish_delete(confirmed, now)
    }

    /// Paso 1 de borrar: comprueba que el certificado existe y prepara la petición de presencia
    /// (sin pedirla).
    pub fn prepare_delete(&self, fingerprint: &str) -> Result<DeleteRequest> {
        let info = self.certificate(fingerprint)?.ok_or(VaultError::NotFound)?;
        Ok(DeleteRequest(IdentityRequest {
            fingerprint: fingerprint.to_owned(),
            purpose: "eliminar".to_owned(),
            reason: format!("Eliminar el certificado de {}", display_holder(&info)),
            presence: self.presence.clone(),
        }))
    }

    /// Paso 3 de borrar: con la presencia confirmada, borra el certificado y lo anota.
    pub fn finish_delete(&self, req: ConfirmedDelete, now: OffsetDateTime) -> Result<()> {
        let fingerprint = req.0.fingerprint;
        self.conn.execute(
            "DELETE FROM certificate_ref WHERE fingerprint_sha256 = ?1",
            [&fingerprint],
        )?;
        audit::append(
            &self.conn,
            &now_str(now),
            Actor::User,
            "certificate.delete",
            &fingerprint,
            &json!({}),
        )?;
        Ok(())
    }

    /// Lee un ajuste JSON (`None` si no existe).
    pub fn setting<T: serde::de::DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let raw: Option<String> = self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .optional()?;
        raw.map(|v| serde_json::from_str(&v).map_err(|e| VaultError::InvalidInput(e.to_string())))
            .transpose()
    }

    /// Guarda un ajuste JSON. Nunca guardar secretos aquí.
    pub fn set_setting<T: serde::Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let v =
            serde_json::to_string(value).map_err(|e| VaultError::InvalidInput(e.to_string()))?;
        self.conn.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, v],
        )?;
        Ok(())
    }

    /// Marca un aviso como enviado. Devuelve `false` si ya lo estaba.
    pub fn mark_reminder_sent(&self, key: &str, now: OffsetDateTime) -> Result<bool> {
        let n = self.conn.execute(
            "INSERT OR IGNORE INTO reminder_sent(reminder_key, sent_at) VALUES (?1, ?2)",
            params![key, now_str(now)],
        )?;
        Ok(n == 1)
    }

    pub fn verify_audit(&self) -> Result<usize> {
        audit::verify(&self.conn)
    }

    /// Consulta de sólo lectura del registro de auditoría (pantalla «Auditoría»).
    pub fn audit_query(&self, f: &audit::Filtro) -> Result<audit::Pagina> {
        audit::consultar(&self.conn, f)
    }

    /// Tipos de acción del registro, con cuántas entradas hay de cada uno.
    pub fn audit_actions(&self) -> Result<Vec<(String, i64)>> {
        audit::acciones(&self.conn)
    }

    /// Comprueba que la BD está sana. Ver [`integridad`]. Vacío si todo está bien.
    pub fn integrity_check(&self) -> Result<Vec<String>> {
        integridad(&self.conn)
    }
}

/// Problemas de la BD de `conn` (vacío si está sana), con un tope de 20:
///
/// - `PRAGMA cipher_integrity_check` (SQLCipher): el código de autenticación (HMAC) de cada
///   página; una página que no lo cumple está dañada en el disco o se ha tocado por fuera.
/// - `PRAGMA integrity_check`: la estructura de SQLite (árboles, índices, restricciones).
///
/// Sólo lee. En una BD en memoria SQLCipher no tiene páginas que comprobar y la primera no dice
/// nada.
pub(crate) fn integridad(conn: &Connection) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let cipher = (|| -> rusqlite::Result<Vec<String>> {
        let mut stmt = conn.prepare("PRAGMA cipher_integrity_check")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        rows.collect()
    })();
    match cipher {
        Ok(rows) => out.extend(rows.into_iter().map(|r| format!("cifrado: {r}"))),
        Err(e) => tracing::warn!(error = %e, "no se pudo comprobar el cifrado de las páginas"),
    }
    let mut stmt = conn.prepare("PRAGMA integrity_check(20)")?;
    let rows: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<_, _>>()?;
    out.extend(rows.into_iter().filter(|r| r != "ok"));
    out.truncate(20);
    Ok(out)
}

fn display_holder(info: &CertificateInfo) -> String {
    let name = info
        .holder_name
        .clone()
        .unwrap_or_else(|| info.holder_nif.clone());
    match &info.entity_name {
        Some(e) => format!("{name} (representante de {e})"),
        None => name,
    }
}

const SELECT_INFO: &str = "SELECT fingerprint_sha256, subject_dn, issuer_dn, serial, valid_from,
    valid_to, kind, issuer_kind, holder_name, holder_nif, entity_nif, entity_name, key_algorithm
    FROM certificate_ref";

fn row_to_info(r: &rusqlite::Row<'_>) -> rusqlite::Result<CertificateInfo> {
    let kind: String = r.get(6)?;
    let issuer_kind: String = r.get(7)?;
    Ok(CertificateInfo {
        fingerprint_sha256: r.get(0)?,
        subject_dn: r.get(1)?,
        issuer_dn: r.get(2)?,
        serial: r.get(3)?,
        valid_from: r.get(4)?,
        valid_to: r.get(5)?,
        kind: CertificateKind::parse(&kind).unwrap_or(CertificateKind::Otro),
        issuer_kind: if issuer_kind == "fnmt" {
            IssuerKind::Fnmt
        } else {
            IssuerKind::Otro
        },
        holder_name: r.get(8)?,
        holder_nif: r.get(9)?,
        entity_nif: r.get(10)?,
        entity_name: r.get(11)?,
        key_algorithm: r.get(12)?,
        policy_oids: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::Rollback;
    use crate::backup::KdfParams;
    use taxpain_secrets::{AlwaysConfirm, MemoryKeyProvider, NeverConfirm};

    const PASS: &str = "caballo grapa batería";
    const FAST: KdfParams = KdfParams {
        log_n: 12,
        r: 8,
        p: 1,
    };

    fn keys() -> Arc<MemoryKeyProvider> {
        Arc::new(MemoryKeyProvider::default())
    }

    fn log(v: &Vault, action: &str) {
        audit::append(
            v.connection(),
            "2026-10-14T10:00:00Z",
            Actor::User,
            action,
            "s",
            &json!({}),
        )
        .unwrap();
    }

    fn anchor(dir: &Path) -> String {
        std::fs::read_to_string(dir.join(ANCHOR_FILE)).unwrap()
    }

    /// Una BD sana no tiene problemas; una con una página tocada en el disco los tiene.
    #[test]
    fn la_comprobacion_de_integridad_ve_una_pagina_danada() {
        let tmp = tempfile::tempdir().unwrap();
        let k = keys();
        {
            let v = Vault::open(tmp.path(), k.clone(), Arc::new(AlwaysConfirm)).unwrap();
            assert_eq!(v.integrity_check().unwrap(), Vec::<String>::new());
            for i in 0..400 {
                log(&v, &format!("prueba.{i}"));
            }
            v.connection()
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .unwrap();
        }
        let db = tmp.path().join(DB_FILE);
        let mut bytes = std::fs::read(&db).unwrap();
        assert!(bytes.len() > 4096 * 6);
        // Un byte de la página 5 cambiado (no la primera: así la BD aún se abre).
        bytes[4096 * 4 + 100] ^= 0xFF;
        std::fs::write(&db, bytes).unwrap();
        let conn = db::open_unmigrated(&db, &k.get(KeyId::DbMaster, "").unwrap().unwrap()).unwrap();
        let problemas = integridad(&conn).unwrap();
        assert!(!problemas.is_empty(), "debe ver la página dañada");
    }

    /// Sólo abre lo que ya existe: ni la carpeta, ni la BD, ni la clave del Llavero.
    #[test]
    fn open_existing_no_crea_nada() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("empresa");
        let k = keys();
        assert!(matches!(
            Vault::open_existing(&dir, k.clone(), Arc::new(NeverConfirm)),
            Err(VaultError::NotFound)
        ));
        assert!(!dir.exists(), "no crea la carpeta");
        assert!(k.get(KeyId::DbMaster, "").unwrap().is_none());

        // Hay datos (de otro Llavero) pero aquí no está la clave: no se crea otra.
        drop(Vault::open(&dir, keys(), Arc::new(AlwaysConfirm)).unwrap());
        assert!(matches!(
            Vault::open_existing(&dir, k.clone(), Arc::new(NeverConfirm)),
            Err(VaultError::KeyMissing { recoverable: false })
        ));
        assert!(k.get(KeyId::DbMaster, "").unwrap().is_none());
        // Una carpeta sin BD tampoco se estrena.
        let empty = tmp.path().join("vacia");
        std::fs::create_dir_all(&empty).unwrap();
        assert!(matches!(
            Vault::open_existing(&empty, k, Arc::new(NeverConfirm)),
            Err(VaultError::NotFound)
        ));
        assert_eq!(std::fs::read_dir(&empty).unwrap().count(), 0);
    }

    /// No migra: si el esquema no es el de esta compilación, lo dice y deja la BD como estaba.
    #[test]
    fn open_existing_no_migra() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("empresa");
        std::fs::create_dir_all(&dir).unwrap();
        let k = keys();
        let key = k.get_or_create(KeyId::DbMaster, "").unwrap();
        let current = db::schema_version();
        drop(db::open_at_version(&dir.join(DB_FILE), &key, current - 1).unwrap());
        let e = Vault::open_existing(&dir, k.clone(), Arc::new(NeverConfirm))
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("versión anterior") && e.contains("abre antes la app"),
            "{e}"
        );
        let conn = db::open_at_version(&dir.join(DB_FILE), &key, current - 1).unwrap();
        assert_eq!(db::user_version(&conn).unwrap(), current - 1, "sin migrar");

        conn.pragma_update(None, "user_version", i64::try_from(current + 1).unwrap())
            .unwrap();
        drop(conn);
        let e = Vault::open_existing(&dir, k, Arc::new(NeverConfirm))
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("más nueva") && e.contains("actualiza este programa"),
            "{e}"
        );
    }

    /// Abrir así no mueve el ancla (ni al abrir, ni al escribir, ni al cerrar) ni repone el
    /// código de recuperación; lo que se escribe queda, y la app lo ve bien al abrir después.
    #[test]
    fn open_existing_no_mueve_el_ancla_ni_escribe_el_codigo_de_recuperacion() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("empresa");
        let k = keys();
        {
            let v = Vault::open(&dir, k.clone(), Arc::new(AlwaysConfirm)).unwrap();
            backup::set_passphrase_with(&v, PASS, FAST, OffsetDateTime::now_utc()).unwrap();
        }
        assert!(recovery::exists(&dir));
        std::fs::remove_file(recovery::path(&dir)).unwrap();
        let before = anchor(&dir);
        {
            let v = Vault::open_existing(&dir, k.clone(), Arc::new(NeverConfirm)).unwrap();
            assert_eq!(v.dir(), Some(dir.as_path()));
            assert!(v.audit_rollback_pending().unwrap().is_none());
            // Se puede escribir: los borradores que deja el asistente.
            v.set_setting("borrador", &1).unwrap();
            log(&v, "quote.draft");
            v.sync_anchor().unwrap();
            assert_eq!(anchor(&dir), before);
        }
        assert_eq!(anchor(&dir), before, "tampoco al cerrar");
        assert!(!recovery::exists(&dir));

        // La app abre después: la BD va por delante del ancla, que no es un retroceso.
        let v = Vault::open(&dir, k, Arc::new(AlwaysConfirm)).unwrap();
        assert!(v.audit_rollback_pending().unwrap().is_none());
        assert_eq!(v.setting::<i32>("borrador").unwrap(), Some(1));
        assert!(recovery::exists(&dir), "la app sí repone el código");
        drop(v);
        assert_ne!(anchor(&dir), before, "y adelanta el ancla");
    }

    /// Si la BD está por detrás del ancla, quien abre sin escribir lo ve pero no lo anota ni
    /// lo da por visto: el ancla sigue donde estaba y la app lo detecta y lo guarda después.
    #[test]
    fn open_existing_ve_un_retroceso_sin_darlo_por_visto() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("empresa");
        let k = keys();
        {
            let v = Vault::open(&dir, k.clone(), Arc::new(AlwaysConfirm)).unwrap();
            log(&v, "a");
        }
        let old = tmp.path().join("antigua.db");
        std::fs::copy(dir.join(DB_FILE), &old).unwrap();
        {
            let v = Vault::open(&dir, k.clone(), Arc::new(AlwaysConfirm)).unwrap();
            log(&v, "b");
            log(&v, "c");
        }
        std::fs::copy(&old, dir.join(DB_FILE)).unwrap();
        let before = anchor(&dir);
        let rollback = Rollback {
            anchored_seq: 3,
            current_seq: 1,
        };
        {
            let v = Vault::open_existing(&dir, k.clone(), Arc::new(NeverConfirm)).unwrap();
            assert_eq!(v.audit_rollback(), Some(&rollback));
            let p = v.audit_rollback_pending().unwrap().unwrap();
            assert_eq!(p.kind, AnchorProblem::Rollback);
            assert_eq!(p.rollback(), Some(rollback.clone()));
            // Nada escrito: ni la entrada en la cadena ni el aviso.
            assert_eq!(v.verify_audit().unwrap(), 1);
            assert!(read_pending(v.connection()).unwrap().is_none());
            assert!(
                !v.acknowledge_audit_rollback(OffsetDateTime::now_utc())
                    .unwrap(),
                "no hay nada guardado que reconocer"
            );
            assert!(v.audit_rollback_pending().unwrap().is_some());
        }
        assert_eq!(anchor(&dir), before);

        let v = Vault::open(&dir, k, Arc::new(AlwaysConfirm)).unwrap();
        assert_eq!(v.audit_rollback(), Some(&rollback));
        assert_eq!(
            read_pending(v.connection()).unwrap().unwrap().rollback(),
            Some(rollback)
        );
        let last = audit::entries(v.connection()).unwrap().pop().unwrap();
        assert_eq!(last.action, "audit.rollback");
    }
}
