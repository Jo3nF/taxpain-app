//! Confirmación de presencia del usuario (Touch ID) antes de cada acto sensible (D4).
//! La implementación real (`TouchIdPresence`) está en el crate `taxpain-macos`.

use crate::{Error, Result};

pub trait UserPresence: Send + Sync + std::fmt::Debug {
    /// Pide confirmación mostrando `reason` (p. ej. "Usar el certificado de … para
    /// presentar el modelo 303"). Devuelve `Err(NotConfirmed)` si el usuario no confirma.
    fn confirm(&self, reason: &str) -> Result<()>;
}

/// Acepta siempre. Sólo para tests y desarrollo sin biometría.
#[derive(Debug, Default)]
pub struct AlwaysConfirm;

impl UserPresence for AlwaysConfirm {
    fn confirm(&self, _reason: &str) -> Result<()> {
        Ok(())
    }
}

/// Rechaza siempre. Para tests.
#[derive(Debug, Default)]
pub struct NeverConfirm;

impl UserPresence for NeverConfirm {
    fn confirm(&self, _reason: &str) -> Result<()> {
        Err(Error::NotConfirmed)
    }
}
