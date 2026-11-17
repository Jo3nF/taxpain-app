//! Extractos en PDF de cualquier banco.
//!
//! Primero se mira si el PDF es de un banco cuya maqueta se conoce ([`perfiles`]): se lee con
//! sus reglas. Si no, el lector genérico ([`generico`]) entiende la maqueta de la página
//! (líneas, columnas, cabecera de la tabla) y deduce el signo de cada importe por su columna o
//! por el saldo. Los PDF escaneados se leen con el lector de texto del equipo, si lo hay.

pub(crate) mod generico;
mod perfiles;

use super::Extracto;
use crate::error::Result;

/// Extractos de un PDF (ya sacado de su sobre, si venía en uno).
pub(crate) fn leer(nombre: &str, bytes: &[u8]) -> Result<Vec<Extracto>> {
    let texto = crate::pdf_text::text(bytes)
        .filter(|t| t.trim().len() > 40)
        .or_else(|| crate::carpeta::ocr_text(nombre, bytes));
    if let Some(t) = &texto
        && let Some(e) = perfiles::leer(t, nombre)
    {
        return Ok(e);
    }
    generico::leer(nombre, bytes, texto.as_deref())
}

/// Extractos del texto de un extracto sin PDF detrás (una foto leída con el lector de texto del
/// equipo).
pub(crate) fn leer_texto(nombre: &str, texto: &str) -> Result<Vec<Extracto>> {
    if let Some(e) = perfiles::leer(texto, nombre) {
        return Ok(e);
    }
    generico::leer(nombre, &[], Some(texto))
}
