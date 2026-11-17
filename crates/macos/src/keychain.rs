//! Claves de TaxPain en el Keychain del usuario (servicio `es.taxpain.app`, cuenta = `KeyId`).

use security_framework::access_control::{ProtectionMode, SecAccessControl};
use security_framework::base::Error as SfError;
use security_framework::passwords::{
    PasswordOptions, delete_generic_password_options, generic_password,
    set_generic_password_options,
};
use taxpain_secrets::{Error, Key256, KeyId, KeyProvider, Result};

const SERVICE: &str = "es.taxpain.app";
/// `errSecItemNotFound`.
const ERR_ITEM_NOT_FOUND: i32 = -25300;
/// `errSecMissingEntitlement`: el keychain de protección de datos exige app firmada.
const ERR_MISSING_ENTITLEMENT: i32 = -34018;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeychainMode {
    /// Keychain de protección de datos, `WhenUnlockedThisDeviceOnly`, sin sincronizar con
    /// iCloud. Requiere la app firmada con el entitlement `keychain-access-groups`.
    DataProtection,
    /// Keychain de sesión clásico (builds de desarrollo sin firmar). La ACL del elemento queda
    /// ligada a la firma de código del binario que lo creó.
    Legacy,
}

#[derive(Debug)]
pub struct KeychainKeyProvider {
    mode: KeychainMode,
    /// Servicio del elemento del Keychain (`es.taxpain.app` en la app; otro en las pruebas, para
    /// no tocar las claves reales del usuario).
    service: String,
}

impl KeychainKeyProvider {
    pub fn new(mode: KeychainMode) -> Self {
        Self::with_service(mode, SERVICE)
    }

    /// Proveedor con otro nombre de servicio: sólo para las pruebas contra el Keychain real
    /// (`crates/macos/tests/llavero.rs`), que crean y borran sus propios elementos.
    pub fn with_service(mode: KeychainMode, service: &str) -> Self {
        KeychainKeyProvider {
            mode,
            service: service.to_owned(),
        }
    }

    fn options(&self, id: KeyId) -> PasswordOptions {
        let mut o = PasswordOptions::new_generic_password(&self.service, id.name());
        if self.mode == KeychainMode::DataProtection {
            o.use_protected_keychain();
            o.set_access_synchronized(Some(false));
        }
        o
    }

    fn create(&self, id: KeyId) -> Result<Key256> {
        let key = Key256::random()?;
        self.store(id, &key)?;
        Ok(key)
    }

    /// Guarda `key` como elemento `id` (si ya existe, lo actualiza).
    fn store(&self, id: KeyId, key: &Key256) -> Result<()> {
        let mut o = self.options(id);
        if self.mode == KeychainMode::DataProtection {
            // TODO(D9): añadir USER_PRESENCE a `cert-seal` cuando la app esté firmada y se pase el
            // LAContext ya evaluado (kSecUseAuthenticationContext) para no pedir Touch ID dos veces.
            let ac = SecAccessControl::create_with_protection(
                Some(ProtectionMode::AccessibleWhenUnlockedThisDeviceOnly),
                0,
            )
            .map_err(sf_error)?;
            o.set_access_control(ac);
        }
        o.set_label(&format!("TaxPain ({})", id.name()));
        set_generic_password_options(key.expose(), o).map_err(sf_error)
    }

    /// Lee el elemento `id`; `None` si no existe.
    fn read(&self, id: KeyId) -> Result<Option<Key256>> {
        match generic_password(self.options(id)) {
            Ok(bytes) => {
                let key = Key256::from_bytes(&bytes);
                let mut bytes = bytes;
                zeroize_vec(&mut bytes);
                key.map(Some)
            }
            Err(e) if e.code() == ERR_ITEM_NOT_FOUND => Ok(None),
            Err(e) => Err(sf_error(e)),
        }
    }
}

impl KeyProvider for KeychainKeyProvider {
    fn get_or_create(&self, id: KeyId, _reason: &str) -> Result<Key256> {
        match self.read(id)? {
            Some(key) => Ok(key),
            None => self.create(id),
        }
    }

    fn get(&self, id: KeyId, _reason: &str) -> Result<Option<Key256>> {
        self.read(id)
    }

    fn put(&self, id: KeyId, key: &Key256) -> Result<()> {
        self.store(id, key)
    }

    fn delete(&self, id: KeyId) -> Result<()> {
        match delete_generic_password_options(self.options(id)) {
            Err(e) if e.code() != ERR_ITEM_NOT_FOUND => Err(sf_error(e)),
            _ => Ok(()),
        }
    }

    fn is_secure(&self) -> bool {
        true
    }
}

fn zeroize_vec(v: &mut [u8]) {
    for b in v.iter_mut() {
        // Escritura volátil para que el compilador no la elimine.
        // SAFETY: `b` es una referencia válida y alineada a un u8 del slice.
        #[allow(unsafe_code)]
        unsafe {
            std::ptr::write_volatile(b, 0);
        }
    }
}

fn sf_error(e: SfError) -> Error {
    if e.code() == ERR_MISSING_ENTITLEMENT {
        return Error::KeyUnavailable(
            "el Keychain de protección de datos exige la app firmada (usa KeychainMode::Legacy en desarrollo)"
                .into(),
        );
    }
    Error::KeyUnavailable(format!("Keychain: {} ({})", e, e.code()))
}
