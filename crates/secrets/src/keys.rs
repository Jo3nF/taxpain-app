//! Proveedores de claves (D4). El de producción en macOS es `KeychainKeyProvider`
//! (crate `taxpain-macos`); aquí están los de desarrollo y pruebas.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use crate::secret::Key256;
use crate::{Error, Result};

/// Claves que usa la app. Cada una es un elemento independiente en el Keychain.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyId {
    /// Clave de la base de datos SQLCipher.
    DbMaster,
    /// Clave de sellado de los certificados: leerla exige Touch ID / contraseña del usuario.
    CertSeal,
}

impl KeyId {
    pub const ALL: [KeyId; 2] = [KeyId::DbMaster, KeyId::CertSeal];

    /// Nombre estable (cuenta del elemento del Keychain, nombre de fichero en desarrollo).
    pub fn name(self) -> &'static str {
        match self {
            KeyId::DbMaster => "db-master",
            KeyId::CertSeal => "cert-seal",
        }
    }

    /// Si el sistema debe pedir presencia del usuario (biometría) para leerla.
    pub fn requires_user_presence(self) -> bool {
        matches!(self, KeyId::CertSeal)
    }
}

pub trait KeyProvider: Send + Sync + std::fmt::Debug {
    /// Devuelve la clave; si no existe, la crea aleatoria y la guarda.
    /// `reason` se muestra en el diálogo del sistema cuando la clave exige Touch ID.
    fn get_or_create(&self, id: KeyId, reason: &str) -> Result<Key256>;

    /// Devuelve la clave si existe; `None` si no hay elemento. No la crea: sirve para
    /// distinguir «no hay clave» de «la clave no corresponde» al abrir un almacén que ya existe.
    fn get(&self, id: KeyId, reason: &str) -> Result<Option<Key256>>;

    /// Guarda `key` como la clave `id`, sustituyendo la que hubiera (recuperación del almacén
    /// con el código de recuperación).
    fn put(&self, id: KeyId, key: &Key256) -> Result<()>;

    /// Borra la clave (destrucción criptográfica de lo que protegía).
    fn delete(&self, id: KeyId) -> Result<()>;

    /// `false` si la clave no está protegida por el sistema operativo (sólo desarrollo).
    fn is_secure(&self) -> bool;
}

/// Claves en memoria; se pierden al cerrar. Para tests.
#[derive(Debug, Default)]
pub struct MemoryKeyProvider {
    keys: Mutex<HashMap<KeyId, Key256>>,
}

impl KeyProvider for MemoryKeyProvider {
    fn get_or_create(&self, id: KeyId, _reason: &str) -> Result<Key256> {
        let mut keys = self.keys.lock().map_err(|_| poisoned())?;
        if let Some(k) = keys.get(&id) {
            return Ok(k.clone());
        }
        let k = Key256::random()?;
        keys.insert(id, k.clone());
        Ok(k)
    }

    fn get(&self, id: KeyId, _reason: &str) -> Result<Option<Key256>> {
        Ok(self.keys.lock().map_err(|_| poisoned())?.get(&id).cloned())
    }

    fn put(&self, id: KeyId, key: &Key256) -> Result<()> {
        self.keys
            .lock()
            .map_err(|_| poisoned())?
            .insert(id, key.clone());
        Ok(())
    }

    fn delete(&self, id: KeyId) -> Result<()> {
        self.keys.lock().map_err(|_| poisoned())?.remove(&id);
        Ok(())
    }

    fn is_secure(&self) -> bool {
        false
    }
}

/// Claves en ficheros `0600` de un directorio. **Inseguro**: sólo para desarrollo en Linux,
/// donde no hay Keychain. La app muestra un aviso si `is_secure()` es `false`.
#[derive(Debug)]
pub struct FileKeyProvider {
    dir: PathBuf,
}

impl FileKeyProvider {
    pub fn new(dir: impl Into<PathBuf>) -> Result<Self> {
        let dir = dir.into();
        fs::create_dir_all(&dir)?;
        restrict_permissions(&dir, 0o700)?;
        Ok(FileKeyProvider { dir })
    }

    fn path(&self, id: KeyId) -> PathBuf {
        self.dir.join(format!("{}.key", id.name()))
    }
}

impl FileKeyProvider {
    /// Escribe la clave en su fichero `0600` (`create_new`: nunca pisa una que exista).
    fn write(&self, id: KeyId, k: &Key256) -> Result<()> {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(self.path(id))?;
        f.write_all(k.expose())?;
        f.sync_all()?;
        Ok(())
    }
}

impl KeyProvider for FileKeyProvider {
    fn get_or_create(&self, id: KeyId, reason: &str) -> Result<Key256> {
        if let Some(k) = self.get(id, reason)? {
            return Ok(k);
        }
        let k = Key256::random()?;
        self.write(id, &k)?;
        Ok(k)
    }

    fn get(&self, id: KeyId, _reason: &str) -> Result<Option<Key256>> {
        match fs::read(self.path(id)) {
            Ok(bytes) => {
                let bytes = zeroize::Zeroizing::new(bytes);
                Key256::from_bytes(&bytes).map(Some)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn put(&self, id: KeyId, key: &Key256) -> Result<()> {
        self.delete(id)?;
        self.write(id, key)
    }

    fn delete(&self, id: KeyId) -> Result<()> {
        match fs::remove_file(self.path(id)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        }
    }

    fn is_secure(&self) -> bool {
        false
    }
}

fn poisoned() -> Error {
    Error::KeyUnavailable("estado interno envenenado".into())
}

pub fn restrict_permissions(path: &std::path::Path, mode: u32) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_provider_is_stable_and_deletable() {
        let p = MemoryKeyProvider::default();
        let a = p.get_or_create(KeyId::DbMaster, "").unwrap();
        assert_eq!(
            a.expose(),
            p.get_or_create(KeyId::DbMaster, "").unwrap().expose()
        );
        assert_ne!(
            a.expose(),
            p.get_or_create(KeyId::CertSeal, "").unwrap().expose()
        );
        p.delete(KeyId::DbMaster).unwrap();
        assert_ne!(
            a.expose(),
            p.get_or_create(KeyId::DbMaster, "").unwrap().expose()
        );
    }

    #[test]
    fn file_provider_persists_with_private_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let p = FileKeyProvider::new(dir.path().join("keys")).unwrap();
        let a = p.get_or_create(KeyId::CertSeal, "").unwrap();
        let p2 = FileKeyProvider::new(dir.path().join("keys")).unwrap();
        assert_eq!(
            a.expose(),
            p2.get_or_create(KeyId::CertSeal, "").unwrap().expose()
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(p.path(KeyId::CertSeal))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(!p.is_secure());
    }
}
