//! Comprobación de un NIF-IVA en VIES (Comisión Europea), a petición del usuario.
//!
//! Antes de facturar sin IVA español a una empresa de otro país de la UE (servicio con
//! inversión del sujeto pasivo o entrega intracomunitaria) hay que comprobar que su NIF-IVA está
//! dado de alta en VIES; guardar el número de consulta es la prueba. Se envían el NIF-IVA del
//! cliente y el de la empresa (para que VIES dé número de consulta). Nada más.
//!
//! Mismas reglas que con la AEAT (D6): sólo `https`, sólo el host de la lista, sin
//! redirecciones ni proxies, servidor verificado con el almacén de confianza del sistema y sin
//! certificado de cliente. `TODO(VERIFY)`: nombres de los campos de la API REST con una
//! consulta real (aquí se leen las dos variantes conocidas).

use std::time::Duration;

use serde::Serialize;
use url::Url;

use crate::http::AeatError;

pub const HOST: &str = "ec.europa.eu";
pub const URL: &str = "https://ec.europa.eu/taxation_customs/vies/rest-api/check-vat-number";

/// Resultado de una consulta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ViesResult {
    pub country: String,
    pub vat_number: String,
    pub valid: bool,
    /// Nombre y dirección que devuelve VIES (algunos países no los dan).
    pub name: Option<String>,
    pub address: Option<String>,
    /// Número de consulta (la prueba de la comprobación).
    pub request_id: Option<String>,
    pub request_date: Option<String>,
}

/// Separa «FR12345678901» en («FR», «12345678901»); Grecia va como `EL`.
pub fn split(country: &str, vat: &str) -> (String, String) {
    let c = match country.trim().to_uppercase().as_str() {
        "GR" => "EL".to_owned(),
        c => c.to_owned(),
    };
    let clean: String = vat
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_uppercase();
    let number = clean.strip_prefix(&c).map_or(clean.clone(), str::to_owned);
    (c, number)
}

fn text(v: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| v.get(*k).and_then(|x| x.as_str()))
        .map(str::trim)
        .filter(|s| !s.is_empty() && *s != "---")
        .map(str::to_owned)
}

/// Código de error de VIES: `errorWrappers: [{ "error": "MS_UNAVAILABLE", "message": … }]`.
fn wrapper_error(body: &serde_json::Value) -> Option<String> {
    body.get("errorWrappers")
        .and_then(serde_json::Value::as_array)
        .and_then(|a| a.first())
        .and_then(|e| text(e, &["error", "message"]))
        .or_else(|| text(body, &["errorWrappers"]))
}

/// Mensaje para un error de VIES (no es una respuesta sobre el NIF-IVA: no se guarda).
fn service_error(code: &str) -> AeatError {
    AeatError::Service(match code {
        "INVALID_INPUT" | "INVALID_REQUESTER_INFO" => format!(
            "VIES no lo ha comprobado ({code}): revisa el país y el NIF-IVA (y tu NIF en los \
             ajustes)"
        ),
        "MS_UNAVAILABLE"
        | "MS_MAX_CONCURRENT_REQ"
        | "MS_MAX_CONCURRENT_REQ_TIME"
        | "GLOBAL_MAX_CONCURRENT_REQ"
        | "GLOBAL_MAX_CONCURRENT_REQ_TIME"
        | "SERVICE_UNAVAILABLE"
        | "TIMEOUT" => format!(
            "VIES no puede comprobarlo ahora ({code}): el servicio de ese país no responde; \
             inténtalo más tarde"
        ),
        _ => format!("VIES no ha podido comprobarlo ({code})"),
    })
}

/// Lee la respuesta de VIES (`valid` o `isValid`, `requestIdentifier`…). Un `userError` que no
/// sea `VALID` o `INVALID` (país no disponible, demasiadas consultas…) o un `errorWrappers` es
/// un error, aunque venga con `isValid: false`: no dice nada del NIF-IVA.
pub fn parse(body: &serde_json::Value) -> Result<ViesResult, AeatError> {
    if let Some(code) = wrapper_error(body) {
        return Err(service_error(&code));
    }
    if let Some(code) = text(body, &["userError"])
        && !matches!(code.as_str(), "VALID" | "INVALID")
    {
        return Err(service_error(&code));
    }
    let valid = body
        .get("valid")
        .or_else(|| body.get("isValid"))
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| AeatError::InvalidResponse("VIES no dijo si es válido".into()))?;
    Ok(ViesResult {
        country: text(body, &["countryCode"]).unwrap_or_default(),
        vat_number: text(body, &["vatNumber"]).unwrap_or_default(),
        valid,
        name: text(body, &["name", "traderName"]),
        address: text(body, &["address", "traderAddress"]),
        request_id: text(body, &["requestIdentifier"]),
        request_date: text(body, &["requestDate"]),
    })
}

/// Consulta `vat` de `country`. `requester`: NIF de la empresa (sin «ES»), para el número de
/// consulta.
pub async fn check(
    country: &str,
    vat: &str,
    requester: Option<&str>,
) -> Result<ViesResult, AeatError> {
    let (c, number) = split(country, vat);
    if number.is_empty() {
        return Err(AeatError::Service("falta el NIF-IVA que comprobar".into()));
    }
    if c.len() != 2 || !c.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return Err(AeatError::Service(
            "falta el país del NIF-IVA (código de dos letras)".into(),
        ));
    }
    let url = Url::parse(URL).map_err(|_| AeatError::InvalidUrl)?;
    if url.host_str() != Some(HOST) || url.scheme() != "https" {
        return Err(AeatError::HostNotAllowed(
            url.host_str().unwrap_or("").into(),
        ));
    }
    let provider = std::sync::Arc::new(taxpain_vault::rustls::crypto::ring::default_provider());
    let verifier = rustls_platform_verifier::Verifier::new(provider.clone())
        .map_err(|e| AeatError::Tls(e.to_string()))?;
    let mut tls = taxpain_vault::rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| AeatError::Tls(e.to_string()))?
        .dangerous()
        .with_custom_certificate_verifier(std::sync::Arc::new(verifier))
        .with_no_client_auth();
    tls.alpn_protocols = vec![b"http/1.1".to_vec()];
    let http = reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(15))
        .user_agent(concat!("TaxPain/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| AeatError::Tls(e.to_string()))?;
    let mut body = serde_json::json!({ "countryCode": c, "vatNumber": number });
    if let Some(r) = requester {
        let (_, own) = split("ES", r);
        body["requesterMemberStateCode"] = "ES".into();
        body["requesterNumber"] = own.into();
    }
    tracing::info!(host = HOST, "consulta VIES");
    let res = http
        .post(url)
        .json(&body)
        .send()
        .await
        .map_err(|e| AeatError::Network(crate::http::without_url(&e)))?;
    let status = res.status();
    tracing::info!(status = status.as_u16(), "respuesta VIES");
    if !status.is_success() {
        // Con el error de VIES si lo explica («MS_UNAVAILABLE»…); si no, el código HTTP.
        let body: Option<serde_json::Value> =
            crate::http::read_limited(res, crate::http::limits::SMALL)
                .await
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok());
        return Err(body
            .as_ref()
            .and_then(wrapper_error)
            .map_or(AeatError::Status(status.as_u16()), |code| {
                service_error(&code)
            }));
    }
    let body = crate::http::read_limited(res, crate::http::limits::SMALL).await?;
    let json: serde_json::Value =
        serde_json::from_slice(&body).map_err(|e| AeatError::InvalidResponse(e.to_string()))?;
    parse(&json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separa_el_pais() {
        assert_eq!(
            split("fr", "FR 123 456 789 01"),
            ("FR".into(), "12345678901".into())
        );
        assert_eq!(split("GR", "123456789"), ("EL".into(), "123456789".into()));
        assert_eq!(
            split("ES", "ESB12345674"),
            ("ES".into(), "B12345674".into())
        );
    }

    #[test]
    fn lee_las_dos_variantes_de_respuesta() {
        let a = parse(&serde_json::json!({
            "countryCode": "FR", "vatNumber": "12345678901", "valid": true,
            "name": "ACME SAS", "address": "1 RUE X\n75001 PARIS",
            "requestIdentifier": "WAPIAAAAZ1", "requestDate": "2026-10-24T10:00:00.000Z"
        }))
        .unwrap();
        assert!(a.valid);
        assert_eq!(a.request_id.as_deref(), Some("WAPIAAAAZ1"));
        let b =
            parse(&serde_json::json!({ "isValid": false, "name": "---", "userError": "INVALID" }))
                .unwrap();
        assert!(!b.valid && b.name.is_none());
        assert!(parse(&serde_json::json!({ "userError": "MS_UNAVAILABLE" })).is_err());
    }

    #[test]
    fn un_error_de_vies_no_es_un_no_valido() {
        // País caído, demasiadas consultas…: aunque diga `isValid: false`, es un error.
        for code in [
            "MS_UNAVAILABLE",
            "TIMEOUT",
            "MS_MAX_CONCURRENT_REQ",
            "SERVICE_UNAVAILABLE",
        ] {
            let e = parse(&serde_json::json!({
                "isValid": false, "userError": code, "vatNumber": "12345678901"
            }))
            .unwrap_err();
            assert!(
                matches!(&e, AeatError::Service(m) if m.contains(code) && m.contains("más tarde")),
                "{e}"
            );
        }
        // `errorWrappers` es una lista.
        let e = parse(&serde_json::json!({
            "actionSucceed": false,
            "errorWrappers": [{ "error": "MS_MAX_CONCURRENT_REQ", "message": "x" }]
        }))
        .unwrap_err();
        assert!(e.to_string().contains("MS_MAX_CONCURRENT_REQ"), "{e}");
        let e = parse(&serde_json::json!({ "valid": false, "userError": "INVALID_INPUT" }))
            .unwrap_err();
        assert!(e.to_string().contains("revisa"), "{e}");
        // Respuesta normal con `userError: VALID`.
        let ok = parse(&serde_json::json!({ "isValid": true, "userError": "VALID" })).unwrap();
        assert!(ok.valid);
    }

    #[test]
    fn sin_nif_iva_no_se_consulta() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        // Sin red: falla antes de conectar.
        let e = rt.block_on(check("FR", " - ", None)).unwrap_err();
        assert_eq!(e.to_string(), "falta el NIF-IVA que comprobar");
        let e = rt.block_on(check("", "12345678901", None)).unwrap_err();
        assert!(e.to_string().contains("país"), "{e}");
    }
}
