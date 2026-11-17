//! Integración con la AEAT (Fases 3, 5 y 6): diseños de registro (DR), prevalidación,
//! presentación con certificado, VERI*FACTU y consultas. Ver `docs/AEAT_INTEGRATION.md`.
//!
//! Fase 1: cliente HTTPS con certificado de cliente (mTLS) restringido a una allow-list exacta
//! de hosts (`http`). Fuera de la AEAT y sólo cuando el usuario lo pide: VIES (`vies`) y los
//! tipos de cambio de referencia del BCE (`bce`). El cotejo por CSV de los documentos de la AEAT
//! (`cotejo`): el servicio web oficial con certificado y, si falla, la Sede pública.

pub mod bce;
pub mod cotejo;
pub mod declarations;
pub mod http;
pub mod notifications;
pub mod renta;
pub mod verifactu;
pub mod vies;

pub use http::{AeatClient, AeatError, hosts};
