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
/// para `<AUX>`. Con los datos de ejemplo de la versión pública (`B00000000`) va **en blanco**:
/// mejor un hueco que un NIF que no es de nadie. `TODO(VERIFY)`: que la prevalidación de la
/// AEAT acepta el `<AUX>` sin NIF de desarrollador, y si hace falta darse de alta como entidad
/// desarrolladora.
pub const DEVELOPER_NIF: &str = if mismo_texto(taxpain_core::productor::NIF, NIF_DE_EJEMPLO) {
    ""
} else {
    taxpain_core::productor::NIF
};

/// El NIF de ejemplo de `publico/productor.rs` (sólo cuadra su dígito de control).
const NIF_DE_EJEMPLO: &str = "B00000000";

const fn mismo_texto(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

#[cfg(test)]
mod developer_nif_tests {
    use super::*;

    #[test]
    fn el_nif_de_ejemplo_no_va_en_el_aux() {
        assert!(mismo_texto("B00000000", NIF_DE_EJEMPLO));
        assert!(!mismo_texto("B0000000", NIF_DE_EJEMPLO));
        assert!(!mismo_texto("B0000000Z", NIF_DE_EJEMPLO));
        assert_ne!(DEVELOPER_NIF, NIF_DE_EJEMPLO);
        let aux = dr::envelope("303", 2026, "1T", SOFTWARE_VERSION, "", &[]);
        assert_eq!(
            aux.len(),
            "<T303020261T0000>".len() + 5 + 300 + 6 + "</T303020261T0000>".len()
        );
    }
}
