//! Secretos en memoria (`Key256`, `SecretBytes`), proveedores de claves (`KeyProvider`) y
//! confirmación del usuario (`UserPresence`). Sin dependencias C, para que el crate
//! `taxpain-macos` se pueda comprobar desde Linux (D4).

pub mod keys;
pub mod presence;
pub mod secret;

pub use keys::{FileKeyProvider, KeyId, KeyProvider, MemoryKeyProvider};
pub use presence::{AlwaysConfirm, NeverConfirm, UserPresence};
pub use secret::{Key256, SecretBytes};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// El sistema no puede proteger o devolver la clave (Keychain no disponible).
    #[error("la clave no está disponible: {0}")]
    KeyUnavailable(String),
    /// El usuario canceló o falló la confirmación (Touch ID).
    #[error("operación no confirmada por el usuario")]
    NotConfirmed,
    #[error("E/S: {0}")]
    Io(#[from] std::io::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
