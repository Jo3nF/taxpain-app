//! Quién produce y distribuye este programa. Va en cada registro VERI*FACTU (bloque
//! `SistemaInformatico`: art. 13 del Reglamento de sistemas de facturación, RD 1007/2023, y
//! Orden HAC/1177/2024) y en la cabecera `<AUX>` de los ficheros de los modelos (NIF de la
//! entidad desarrolladora).
//!
//! **Antes de distribuir TaxPain, pon aquí tus datos** y firma tu declaración responsable: estos
//! son de ejemplo y no identifican a nadie.

/// Nombre o razón social del productor.
pub const NOMBRE: &str = "PRODUCTOR SIN CONFIGURAR";

/// NIF del productor (de ejemplo: sólo cuadra su dígito de control).
pub const NIF: &str = "B00000000";

/// Dirección postal completa del productor (Orden HAC/1177/2024 art. 15.j). `None` mientras
/// quien distribuya TaxPain no la ponga: la declaración responsable sale con el hueco a la
/// vista y el envío a producción queda cerrado.
pub const DIRECCION: Option<&str> = None;

/// Interruptor de compilación del envío VERI*FACTU al entorno de **producción** de la AEAT.
/// Apagado: la app sólo envía al entorno de pruebas. Encenderlo no basta por sí solo: hace
/// falta además una build de release firmada (`Destino::produccion` lo rechaza en desarrollo),
/// los datos del productor completos ([`faltan_datos`]) y que el usuario lo active en su
/// empresa (`verifactu.production`) con confirmación.
pub const PRODUCCION_HABILITADA: bool = false;

/// Lo que falta de los datos del productor para firmar la declaración responsable y enviar a
/// producción (vacío si está todo).
#[must_use]
pub fn faltan_datos() -> Vec<&'static str> {
    let mut faltan = Vec::new();
    if NOMBRE.trim().is_empty() {
        faltan.push("nombre o razón social del productor");
    }
    if NIF.trim().len() != 9 {
        faltan.push("NIF del productor");
    }
    if DIRECCION.is_none_or(|d| d.trim().is_empty()) {
        faltan.push("dirección postal completa del productor");
    }
    faltan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn produccion_cerrada_mientras_falten_datos() {
        // Hoy: interruptor apagado y sin dirección. Si alguien enciende el interruptor, la
        // dirección tiene que estar puesta.
        assert!(!PRODUCCION_HABILITADA || faltan_datos().is_empty());
        assert!(
            faltan_datos().contains(&"dirección postal completa del productor")
                || DIRECCION.is_some()
        );
    }
}
