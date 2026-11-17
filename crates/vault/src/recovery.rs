//! Código de recuperación del almacén: la clave `db-master` sellada con la clave derivada de la
//! contraseña de las copias, en `recovery.sealed` junto a `taxpain.db`.
//!
//! `db-master` vive sólo en el Llavero del Mac y no viaja con la carpeta de datos (Time Machine,
//! Asistente de Migración, una copia de la carpeta): sin ella la BD no se abre. Con este fichero,
//! **carpeta de datos + contraseña de las copias** bastan para abrir el almacén en otro Mac o
//! tras perder el elemento del Llavero. Sin la contraseña el fichero no sirve de nada (AES-256-GCM
//! con una clave scrypt de la contraseña; la contraseña no se guarda en ningún sitio).
//!
//! Formato (JSON, sin datos de la empresa): `format`, parámetros y sal de scrypt y la clave
//! sellada en hex. Se escribe al definir o cambiar la contraseña de las copias y, si falta, al
//! abrir un almacén que ya tiene contraseña. Al restaurar una copia no: la empresa restaurada no
//! hereda la contraseña de la copia (ver `backup::restore`) y no tiene código de recuperación
//! hasta que el usuario elige una nueva.
//!
//! `db-master` es la misma para todas las empresas del Mac: el código de recuperación de
//! cualquiera de ellas, con su contraseña, abre todas. Por eso al borrar una empresa se borra su
//! `recovery.sealed` antes de mandar la carpeta a la Papelera ([`remove`]).

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::backup::{self, BackupKey, KdfParams};
use crate::error::{Result, VaultError};
use crate::seal;
use taxpain_secrets::Key256;

/// Nombre del fichero, junto a `taxpain.db`.
pub const FILE: &str = "recovery.sealed";
const FORMAT: u32 = 1;
const SEAL_CONTEXT: &[u8] = b"taxpain-recovery-v1";
/// Un fichero manipulado no se lee entero: el real ocupa unos cientos de bytes.
const MAX_SIZE: u64 = 16 * 1024;

#[derive(Debug, Serialize, Deserialize)]
struct RecoveryFile {
    format: u32,
    kdf: KdfParams,
    /// Sal de scrypt (hex).
    salt: String,
    /// `db-master` sellada con la clave derivada de la contraseña (hex).
    sealed: String,
}

pub fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// ¿Hay código de recuperación junto a los datos?
pub fn exists(dir: &Path) -> bool {
    path(dir).is_file()
}

/// Borra el código de recuperación de `dir` (y su temporal, si quedó alguno): al borrar una
/// empresa, para que la carpeta que va a la Papelera no lleve un fichero que, con la contraseña
/// de esa empresa, abre las demás. No es un error que no exista.
pub fn remove(dir: &Path) -> Result<()> {
    for p in [path(dir), dir.join(format!(".{FILE}.tmp"))] {
        match std::fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

/// Escribe (o sustituye) el código de recuperación: `db_key` sellada con la clave de las copias.
/// Escritura atómica y sólo legible por el usuario.
pub(crate) fn write(dir: &Path, key: &BackupKey, db_key: &Key256) -> Result<()> {
    let file = RecoveryFile {
        format: FORMAT,
        kdf: key.params,
        salt: hex::encode(key.salt),
        sealed: hex::encode(seal::seal(&key.kek, SEAL_CONTEXT, db_key.expose())?),
    };
    let json =
        serde_json::to_vec_pretty(&file).map_err(|e| VaultError::InvalidInput(e.to_string()))?;
    let dest = path(dir);
    let tmp = dir.join(format!(".{FILE}.tmp"));
    {
        let mut o = std::fs::OpenOptions::new();
        o.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let mut f = o.open(&tmp)?;
        f.write_all(&json)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, &dest)?;
    // Lo escrito se vuelve a leer: con la misma clave derivada ha de salir la misma `db-master`.
    let back = read(dir)?;
    if unseal_with(&back, &key.kek)?.expose() != db_key.expose() {
        let _ = std::fs::remove_file(&dest);
        return Err(VaultError::InvalidInput(
            "el código de recuperación no ha salido bien: vuelve a intentarlo".into(),
        ));
    }
    Ok(())
}

fn read(dir: &Path) -> Result<RecoveryFile> {
    let p = path(dir);
    let not_valid = || VaultError::InvalidInput("el código de recuperación no es válido".into());
    if std::fs::metadata(&p)?.len() > MAX_SIZE {
        return Err(not_valid());
    }
    let bytes = std::fs::read(&p)?;
    let file: RecoveryFile = serde_json::from_slice(&bytes).map_err(|_| not_valid())?;
    if file.format != FORMAT {
        return Err(VaultError::InvalidInput(
            "el código de recuperación es de una versión más nueva de TaxPain: actualiza la app"
                .into(),
        ));
    }
    Ok(file)
}

fn unseal_with(file: &RecoveryFile, kek: &Key256) -> Result<Key256> {
    let sealed = hex::decode(&file.sealed).map_err(|_| VaultError::Corrupted)?;
    let raw = seal::unseal(kek, SEAL_CONTEXT, &sealed).map_err(|_| VaultError::BackupPassphrase)?;
    Ok(Key256::from_bytes(raw.expose())?)
}

/// Devuelve la clave `db-master` de los datos de `dir` a partir de la contraseña de las copias.
/// Con otra contraseña, [`VaultError::BackupPassphrase`].
pub fn unseal(dir: &Path, passphrase: &str) -> Result<Key256> {
    let file = read(dir)?;
    let salt = Zeroizing::new(hex::decode(&file.salt).map_err(|_| VaultError::Corrupted)?);
    let kek = backup::derive(passphrase, &salt, file.kdf)?;
    unseal_with(&file, &kek)
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAST: KdfParams = KdfParams {
        log_n: 12,
        r: 8,
        p: 1,
    };

    fn key(pass: &str) -> BackupKey {
        let salt = [7u8; 16];
        BackupKey {
            kek: backup::derive(pass, &salt, FAST).unwrap(),
            salt,
            params: FAST,
        }
    }

    #[test]
    fn sella_y_desella_con_la_contrasena() {
        let dir = tempfile::tempdir().unwrap();
        let db_key = Key256::random().unwrap();
        assert!(!exists(dir.path()));
        write(dir.path(), &key("una frase que recuerdo"), &db_key).unwrap();
        assert!(exists(dir.path()));
        // Ni la clave ni la contraseña en claro en el fichero.
        let raw = std::fs::read(path(dir.path())).unwrap();
        assert!(!raw.windows(32).any(|w| w == db_key.expose()));
        let hex = hex::encode(db_key.expose());
        assert!(!String::from_utf8_lossy(&raw).contains(&hex));
        assert!(!raw.windows(5).any(|w| w == b"frase"));
        assert_eq!(
            unseal(dir.path(), "una frase que recuerdo")
                .unwrap()
                .expose(),
            db_key.expose()
        );
        assert!(matches!(
            unseal(dir.path(), "otra contraseña"),
            Err(VaultError::BackupPassphrase)
        ));
        // Al cambiar la contraseña se sustituye.
        write(dir.path(), &key("la segunda frase"), &db_key).unwrap();
        assert!(unseal(dir.path(), "una frase que recuerdo").is_err());
        assert_eq!(
            unseal(dir.path(), "la segunda frase").unwrap().expose(),
            db_key.expose()
        );
        // Un fichero que no es el nuestro no se lee.
        std::fs::write(path(dir.path()), b"hola").unwrap();
        assert!(matches!(
            unseal(dir.path(), "la segunda frase"),
            Err(VaultError::InvalidInput(_))
        ));
        std::fs::remove_file(path(dir.path())).unwrap();
        assert!(matches!(
            unseal(dir.path(), "la segunda frase"),
            Err(VaultError::Io(_))
        ));
    }
}
