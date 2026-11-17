//! Errores del almacén. Los mensajes nunca incluyen valores secretos.

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("dato no válido: {0}")]
    InvalidInput(String),
    #[error("no encontrado")]
    NotFound,
    #[error("la contraseña del certificado no es correcta")]
    InvalidPassphrase,
    #[error("la contraseña de la copia de seguridad no es correcta")]
    BackupPassphrase,
    #[error("certificado no válido: {0}")]
    InvalidCertificate(String),
    #[error("el certificado caducó el {0}")]
    ExpiredCertificate(String),
    #[error("el certificado todavía no es válido (desde {0})")]
    NotYetValidCertificate(String),
    /// El certificado está en la lista pero su clave privada no se puede abrir con la clave de
    /// sellado (`cert-seal`) de este Mac: los datos vienen de otro Mac (la clave se quedó en su
    /// Llavero) o la clave se ha borrado. Se arregla importando otra vez el mismo `.p12/.pfx`.
    #[error(
        "este certificado se guardó en otro Mac y aquí no se puede usar: vuelve a importarlo \
         (el mismo fichero .p12 o .pfx)"
    )]
    CertificateSealedElsewhere,
    /// Clave equivocada (p. ej. la BD viene de otro Mac) o datos manipulados.
    #[error("el almacén está dañado o la clave no corresponde")]
    Corrupted,
    /// Hay datos pero el Llavero no tiene el elemento `db-master` (Mac nuevo, cuenta de usuario
    /// nueva, elemento borrado). `recoverable`: junto a los datos está el código de recuperación
    /// (`recovery.sealed`), que con la contraseña de las copias devuelve la clave.
    #[error("no se encuentra en el Llavero la clave que cifra los datos")]
    KeyMissing { recoverable: bool },
    /// El sistema no puede proteger o devolver la clave (Keychain no disponible).
    #[error("la clave no está disponible: {0}")]
    KeyUnavailable(String),
    /// El usuario canceló o falló la confirmación (Touch ID).
    #[error("operación no confirmada por el usuario")]
    NotConfirmed,
    #[error("registro de auditoría alterado en la entrada {0}")]
    AuditTampered(i64),
    #[error("base de datos: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("E/S: {0}")]
    Io(#[from] std::io::Error),
}

impl From<taxpain_secrets::Error> for VaultError {
    fn from(e: taxpain_secrets::Error) -> Self {
        match e {
            taxpain_secrets::Error::KeyUnavailable(m) => VaultError::KeyUnavailable(m),
            taxpain_secrets::Error::NotConfirmed => VaultError::NotConfirmed,
            taxpain_secrets::Error::Io(e) => VaultError::Io(e),
        }
    }
}

pub type Result<T, E = VaultError> = std::result::Result<T, E>;
