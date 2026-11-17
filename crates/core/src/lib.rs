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
pub use nif::{Nif, NifError, NifKind};
