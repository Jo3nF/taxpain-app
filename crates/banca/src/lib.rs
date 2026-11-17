//! Lectura de cuentas bancarias **en solo lectura** a través de un proveedor de información de
//! cuentas autorizado (PSD2), sin servidor de TaxPain: Enable Banking (AISP autorizado por la
//! FIN-FSA finlandesa, con pasaporte para España).
//!
//! Cada empresa registra **su propia aplicación** en Enable Banking y guarda en su Mac la clave
//! privada de esa aplicación: TaxPain firma en local cada petición (JWT RS256) y habla
//! directamente con `api.enablebanking.com`. El banco pide al titular que autorice el acceso
//! (con su app o su firma) y devuelve un código; con él se abre una sesión que deja leer las
//! cuentas, sus saldos y sus movimientos durante el plazo autorizado (hasta 180 días). Nada de
//! esto permite mover dinero: el permiso que se pide es sólo de información de cuentas.
//!
//! Este crate sólo hace la red y no sabe nada de la contabilidad: `taxpain-app` convierte los
//! movimientos en extractos y los importa como los de un fichero.
//!
//! También descarga, con el token del propio usuario, los extractos del bróker Interactive
//! Brokers por su Flex Web Service ([`ibkr`]), en solo lectura y sólo cuando se pide.

pub mod enable;
mod error;
pub mod ibkr;
pub mod jwt;
mod lectura;
pub mod vuelta;

pub use error::{BancaError, Result};
