//! Dominio fiscal y contable de TaxPain: tipos y reglas puras, sin IO.
//!
//! Cada regla lleva su referencia normativa. Las que aún no se han contrastado con la fuente
//! oficial se marcan `TODO(VERIFY)` y se listan en `PROGRESS.md`.

pub mod accounting;
pub mod bank;
pub mod base64;
pub mod calendar;
pub mod camt;
pub mod cripto;
pub mod eu;
pub mod fx;
pub mod invoice;
pub mod iso3166;
pub mod money;
pub mod nif;
pub mod nomina;
pub mod parametros;
pub mod productor;
pub mod reta;
pub mod sepa;
pub mod spain_time;

pub use money::{Money, Rate};

/// ¿Emite facturas esta compilación? Sí, salvo que se compile con `TAXPAIN_SIN_FACTURACION`
/// no vacía. Así sale la versión que se descarga de la web mientras quien la distribuye no
/// figure como productor del sistema de facturación (RD 1007/2023 art. 13 y
/// [`productor`]): sin emitir facturas, no es un sistema de facturación. Quien compila TaxPain
/// desde el código, con sus datos en [`productor`], tiene la facturación completa
/// (docs/RELEASE.md §2.2).
pub const FACTURACION: bool = match option_env!("TAXPAIN_SIN_FACTURACION") {
    Some(v) => v.is_empty(),
    None => true,
};
pub use nif::{Nif, NifError, NifKind};
