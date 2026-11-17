//! Modelos tributarios: cálculo desde la contabilidad y las facturas, borrador casilla a casilla
//! y fichero según el diseño de registro (DR) oficial de la AEAT.
//!
//! Nada de este crate presenta nada: genera el fichero y el borrador para que el usuario los
//! revise. La presentación (Fase 6) siempre exige su confirmación.

use serde::{Deserialize, Serialize};

pub mod datos_fiscales;
pub mod dr;
pub mod dr_versions;
pub mod informative;
pub mod irnr;
pub mod m100;
pub mod m123;
pub mod m130;
pub mod m184;
pub mod m200;
pub mod m200_catalogo;
pub mod m202;
pub mod m303;
pub mod m349;
pub mod m390;
pub mod periodo;
pub mod rectificativa;
pub mod withholding;

/// Identificación del declarante.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Declarant {
    pub nif: String,
    /// Apellidos (persona física) o razón social.
    pub name: String,
    /// Nombre de pila (sólo personas físicas).
    pub given_name: Option<String>,
}

impl Declarant {
    /// «Apellidos y nombre o razón social» en un único campo (303).
    pub fn full_name(&self) -> String {
        match &self.given_name {
            Some(g) => format!("{} {g}", self.name),
            None => self.name.clone(),
        }
    }
}

/// Versión del programa (4 posiciones) para la cabecera `<AUX>` de los ficheros.
pub const SOFTWARE_VERSION: &str = "0100";
/// NIF de la entidad desarrolladora del software (el productor, [`taxpain_core::productor`])
/// para `<AUX>`. `TODO(VERIFY)`: si hace falta darse de alta como entidad desarrolladora ante
/// la AEAT.
pub const DEVELOPER_NIF: &str = taxpain_core::productor::NIF;
