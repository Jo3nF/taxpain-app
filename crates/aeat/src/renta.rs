//! Datos fiscales de Renta (modelo 100): ratificación del domicilio y descarga con el
//! certificado del contribuyente. La lectura del fichero está en
//! `taxpain_models::datos_fiscales`.
//!
//! Fuente: «Instrucciones para la integración de la obtención de Datos Fiscales de Renta 2025
//! en programas de ayuda», v1.2 (15/07/2026), `fixtures/aeat/renta/SOURCES.md`.
//! Sólo lectura: consultar y descargar no tiene efectos. `TODO(VERIFY)`: el nombre del servicio
//! cambia cada campaña (`SvDesDF25Pei` en 2025); se construye como `SvDesDF{aa}Pei`.

pub use taxpain_models::datos_fiscales::{TaxData, TaxDataError, parse};

use crate::http::{AeatClient, AeatError};

pub const RATIFICATION_URL: &str =
    "https://www1.agenciatributaria.gob.es/wlpl/TOAG-JDIT/IsRatificadoExJson";

/// URL del servicio de entrega de datos fiscales del ejercicio (sólo fiscales, `pdp=N`).
pub fn download_url(year: i32, nif: &str) -> String {
    let mut q = url::form_urlencoded::Serializer::new(String::new());
    q.append_pair("nif", nif).append_pair("pdp", "N");
    format!(
        "https://www1.agenciatributaria.gob.es/wlpl/DFPA-D182/SvDesDF{:02}Pei?{}",
        year.rem_euclid(100),
        q.finish()
    )
}

pub fn ratification_form(year: i32, nif: &str) -> String {
    let mut q = url::form_urlencoded::Serializer::new(String::new());
    q.append_pair("EJERCICIO", &year.to_string())
        .append_pair("NIF", nif);
    q.finish()
}

/// Respuesta del servlet de ratificación: `Ok(true)` si está ratificado.
pub fn parse_ratification(json: &str) -> Result<bool, TaxDataError> {
    let v: serde_json::Value = serde_json::from_str(json)
        .map_err(|_| TaxDataError::Format("respuesta de ratificación no es JSON".into()))?;
    match v.get("status").and_then(|s| s.as_str()) {
        Some("OK") => v
            .pointer("/respuesta/domicilioRatificado")
            .and_then(serde_json::Value::as_bool)
            .ok_or_else(|| TaxDataError::Format("falta domicilioRatificado".into())),
        Some("KO") => Err(TaxDataError::Service(
            v.get("mensaje")
                .and_then(|m| m.as_str())
                .unwrap_or("error de la AEAT")
                .to_owned(),
        )),
        _ => Err(TaxDataError::Format(
            "respuesta de ratificación sin status".into(),
        )),
    }
}

/// ¿Está ratificado el domicilio fiscal para la campaña? (Sólo lectura.)
pub async fn is_ratified(
    client: &AeatClient,
    year: i32,
    nif: &str,
) -> Result<Result<bool, TaxDataError>, AeatError> {
    let body = client
        .post_form(RATIFICATION_URL, &ratification_form(year, nif))
        .await?;
    Ok(parse_ratification(&body))
}

/// Descarga los datos fiscales (bytes tal cual, para guardarlos) y los interpreta.
pub async fn download(
    client: &AeatClient,
    year: i32,
    nif: &str,
) -> Result<Result<(Vec<u8>, TaxData), TaxDataError>, AeatError> {
    let bytes = client.get_bytes(&download_url(year, nif)).await?;
    Ok(parse(&bytes).map(|d| (bytes, d)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_y_ratificacion() {
        assert_eq!(
            parse_ratification(
                r#"{"status":"OK","visible":"N","crashlytics":"N","respuesta":{"domicilioRatificado":false}}"#
            ),
            Ok(false)
        );
        assert!(matches!(
            parse_ratification(r#"{"status":"KO","mensaje":"103 No existen registros","codigo_error":"26001","respuesta":{}}"#),
            Err(TaxDataError::Service(m)) if m.contains("No existen")
        ));
        assert!(parse_ratification("<html>").is_err());
        assert_eq!(
            download_url(2025, "12345678Z"),
            "https://www1.agenciatributaria.gob.es/wlpl/DFPA-D182/SvDesDF25Pei?nif=12345678Z&pdp=N"
        );
        assert_eq!(
            ratification_form(2025, "12345678Z"),
            "EJERCICIO=2025&NIF=12345678Z"
        );
    }
}
