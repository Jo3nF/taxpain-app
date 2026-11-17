//! Almacén cifrado local de TaxPain (Fase 1): BD SQLCipher, claves inyectadas (`KeyProvider`),
//! confirmación del usuario (`UserPresence`), certificados sellados e identidad mTLS, y
//! registro de auditoría encadenado. Ver `docs/ARCHITECTURE.md` §3–4 y `docs/DECISIONS.md` D3–D5.

pub mod audit;
pub mod backup;
pub mod certificate;
pub mod db;
pub mod error;
pub mod identity;
pub mod recovery;
pub mod seal;
mod vault;
pub mod volcado;

pub use certificate::{CertificateInfo, CertificateKind, IssuerKind};
pub use error::{Result, VaultError};
pub use identity::ClientIdentity;
pub use taxpain_secrets::{
    AlwaysConfirm, FileKeyProvider, Key256, KeyId, KeyProvider, MemoryKeyProvider, NeverConfirm,
    SecretBytes, UserPresence,
};
pub use vault::{ConfirmedDelete, ConfirmedIdentity, DeleteRequest, IdentityRequest, Vault};

/// Reexportados para que los consumidores usen la misma versión.
pub use rusqlite;
pub use rustls;
