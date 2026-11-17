//! Cliente HTTPS con certificado de cliente (mTLS) y allow-list exacta de hosts (D6).
//!
//! - Sólo `https`, sólo hosts de la lista (comparación exacta, sin comodines), sin
//!   credenciales en la URL, sin redirecciones (una redirección podría sacar la petición de
//!   la lista) y sin proxies del sistema.
//! - La identidad se usa sólo mientras vive el cliente; se crea por operación tras la
//!   confirmación del usuario (`Vault::client_identity`).
//! - Los logs registran host, ruta y estado; nunca cuerpos ni cabeceras.

use std::collections::BTreeSet;
use std::time::Duration;

use taxpain_vault::ClientIdentity;
use taxpain_vault::rustls::RootCertStore;
use url::Url;

/// Hosts verificados en la documentación oficial (`docs/AEAT_INTEGRATION.md`, §8).
pub mod hosts {
    /// Producción: presentación directa, consulta de declaraciones, VERI*FACTU (certificado
    /// personal o de representante).
    pub const PRODUCCION: &[&str] = &["www1.agenciatributaria.gob.es"];
    /// Entorno de pruebas para externos (otra CA de servidor; días laborables 8:00–15:00).
    pub const PRUEBAS: &[&str] = &["prewww1.aeat.es"];
    /// Prevalidación de autoliquidaciones (ServValiDos), sin efectos: sólo valida y devuelve el
    /// borrador en PDF.
    pub const VALIDACION: &[&str] = &["prewww2.aeat.es"];
    /// Descarga del justificante en PDF (`urlPdf` de la respuesta de presentación, §1.3), en
    /// los dos entornos. Mejor [`justificantes`], que sólo admite los del entorno de la
    /// presentación.
    /// `TODO(VERIFY)`: el host exacto en pruebas (se admiten los dos entornos).
    pub const JUSTIFICANTES: &[&str] = &[
        "www2.agenciatributaria.gob.es",
        "www1.agenciatributaria.gob.es",
        "prewww2.aeat.es",
        "prewww1.aeat.es",
    ];
    /// Justificantes de una presentación en producción.
    pub const JUSTIFICANTES_PRODUCCION: &[&str] = &[
        "www2.agenciatributaria.gob.es",
        "www1.agenciatributaria.gob.es",
    ];
    /// Justificantes de una presentación en el entorno de pruebas.
    pub const JUSTIFICANTES_PRUEBAS: &[&str] = &["prewww2.aeat.es", "prewww1.aeat.es"];

    /// Hosts del justificante según el entorno de la presentación: el certificado del usuario
    /// no se presenta a los de pruebas por una presentación real ni al revés.
    pub fn justificantes(production: bool) -> &'static [&'static str] {
        if production {
            JUSTIFICANTES_PRODUCCION
        } else {
            JUSTIFICANTES_PRUEBAS
        }
    }
}

/// Topes de lo que se lee de una respuesta (un servidor averiado no puede llenar la memoria).
pub mod limits {
    /// PDF y justificantes de la AEAT, respuestas con el certificado (SOAP, JSON, formularios).
    pub const DOCUMENT: usize = 30 * 1024 * 1024;
    /// JSON, XML o CSV pequeños de servicios públicos (VIES, BCE).
    pub const SMALL: usize = 5 * 1024 * 1024;
}

#[derive(Debug, thiserror::Error)]
pub enum AeatError {
    #[error("URL no válida")]
    InvalidUrl,
    #[error("host no permitido para usar el certificado: {0}")]
    HostNotAllowed(String),
    #[error("configuración TLS: {0}")]
    Tls(String),
    /// La conexión no llegó a abrirse (sin red, DNS, TLS con el servidor): nada salió del Mac,
    /// así que una presentación con este error no queda «en duda».
    #[error("no se pudo conectar con la AEAT: {0}")]
    Unreachable(String),
    /// Fallo con la conexión ya abierta (corte, tiempo agotado…): la petición pudo llegar.
    #[error("error de red: {0}")]
    Network(String),
    #[error("respuesta HTTP {0}")]
    Status(u16),
    #[error("respuesta no válida: {0}")]
    InvalidResponse(String),
    /// La respuesta pasa del tope de lo que se lee (`limits`): no se termina de leer.
    #[error("respuesta demasiado grande (más de {} MB)", .0 / (1024 * 1024))]
    TooLarge(usize),
    #[error("envío a producción bloqueado: {0}")]
    ProductionBlocked(&'static str),
    /// Un servicio no pudo responder a lo pedido o falta un dato (mensaje para el usuario).
    #[error("{0}")]
    Service(String),
}

/// Servicios de producción con efectos jurídicos (presentar, enviar registros de facturación).
fn legal_effect_in_production(url: &Url) -> bool {
    [
        crate::declarations::PRES_PRODUCCION,
        crate::verifactu::URL_PRODUCCION,
    ]
    .iter()
    .filter_map(|u| Url::parse(u).ok())
    .any(|p| p.host_str() == url.host_str() && p.path() == url.path())
}

/// Un puerto explícito sólo vale contra el servidor simulado de los tests, en esta máquina, y
/// sólo si se compila con la feature `mock` (nunca en la app).
fn mock_port_allowed(host: &str) -> bool {
    cfg!(any(test, feature = "mock")) && matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

/// Lee el cuerpo de `res` hasta `max` bytes: rechaza antes de leer si el `Content-Length` ya
/// pasa del tope y, si no viene o miente, corta en cuanto lo leído lo pasa.
pub(crate) async fn read_limited(
    mut res: reqwest::Response,
    max: usize,
) -> Result<Vec<u8>, AeatError> {
    if res
        .content_length()
        .is_some_and(|n| n > u64::try_from(max).unwrap_or(u64::MAX))
    {
        return Err(AeatError::TooLarge(max));
    }
    let mut body = Vec::new();
    while let Some(chunk) = res
        .chunk()
        .await
        .map_err(|e| AeatError::InvalidResponse(without_url(&e)))?
    {
        if body.len().saturating_add(chunk.len()) > max {
            return Err(AeatError::TooLarge(max));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Como [`read_limited`], como texto (UTF-8; lo que no lo sea se sustituye, igual que
/// `Response::text` sin la feature `charset` de reqwest).
pub(crate) async fn read_text_limited(
    res: reqwest::Response,
    max: usize,
) -> Result<String, AeatError> {
    let body = read_limited(res, max).await?;
    Ok(match String::from_utf8(body) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    })
}

/// Cliente mTLS hacia hosts AEAT permitidos.
#[derive(Debug)]
pub struct AeatClient {
    http: reqwest::Client,
    allowed_hosts: BTreeSet<String>,
}

impl AeatClient {
    /// `roots`: CAs en las que se confía para el **servidor** (en producción, las de la AEAT;
    /// en tests, la CA propia). `allowed_hosts`: lista exacta.
    pub fn new(
        identity: &ClientIdentity,
        roots: RootCertStore,
        allowed_hosts: &[&str],
    ) -> Result<Self, AeatError> {
        let tls = identity
            .client_config(roots)
            .map_err(|e| AeatError::Tls(e.to_string()))?;
        Self::from_tls(tls, allowed_hosts)
    }

    /// Cliente que verifica el servidor con el almacén de confianza del sistema (en macOS, el
    /// llavero del sistema). Es el que se usa contra la AEAT real.
    /// `TODO(VERIFY)`: que la cadena de `prewww1.aeat.es` (CA de pruebas distinta) y la de
    /// producción validen en macOS.
    pub fn with_platform_trust(
        identity: &ClientIdentity,
        allowed_hosts: &[&str],
    ) -> Result<Self, AeatError> {
        let provider = std::sync::Arc::new(taxpain_vault::rustls::crypto::ring::default_provider());
        let verifier = rustls_platform_verifier::Verifier::new(provider)
            .map_err(|e| AeatError::Tls(e.to_string()))?;
        let tls = identity
            .client_config_with_verifier(std::sync::Arc::new(verifier))
            .map_err(|e| AeatError::Tls(e.to_string()))?;
        Self::from_tls(tls, allowed_hosts)
    }

    fn from_tls(
        mut tls: taxpain_vault::rustls::ClientConfig,
        allowed_hosts: &[&str],
    ) -> Result<Self, AeatError> {
        if allowed_hosts.is_empty() {
            return Err(AeatError::HostNotAllowed("(lista vacía)".into()));
        }
        tls.alpn_protocols = vec![b"http/1.1".to_vec()];
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(20))
            .user_agent(concat!("TaxPain/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| AeatError::Tls(e.to_string()))?;
        Ok(AeatClient {
            http,
            allowed_hosts: allowed_hosts
                .iter()
                .map(|h| h.to_ascii_lowercase())
                .collect(),
        })
    }

    /// Valida la URL contra la política antes de abrir ninguna conexión.
    pub fn check_url(&self, url: &str) -> Result<Url, AeatError> {
        let url = Url::parse(url).map_err(|_| AeatError::InvalidUrl)?;
        if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
            return Err(AeatError::InvalidUrl);
        }
        let host = url
            .host_str()
            .ok_or(AeatError::InvalidUrl)?
            .to_ascii_lowercase();
        if !self.allowed_hosts.contains(&host) {
            return Err(AeatError::HostNotAllowed(host));
        }
        // Los servicios de la AEAT van por el 443: una URL devuelta por la AEAT con otro
        // puerto no se pide con el certificado del usuario (sólo el servidor local de pruebas).
        if url.port().is_some() && !mock_port_allowed(&host) {
            return Err(AeatError::InvalidUrl);
        }
        // Defensa en profundidad: en una compilación de desarrollo nunca se llama a los servicios
        // de producción con efectos jurídicos, aunque algo construya la URL a mano.
        if cfg!(debug_assertions) && legal_effect_in_production(&url) {
            return Err(AeatError::HostNotAllowed(format!(
                "{host}{} (producción, bloqueada en desarrollo)",
                url.path()
            )));
        }
        Ok(url)
    }

    /// POST con cuerpo JSON; devuelve el JSON de respuesta (2xx).
    pub async fn post_json(
        &self,
        url: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, AeatError> {
        let url = self.check_url(url)?;
        tracing::info!(
            host = url.host_str().unwrap_or(""),
            path = url.path(),
            "POST AEAT"
        );
        let res = self
            .http
            .post(url)
            .json(body)
            .send()
            .await
            .map_err(|e| network_error(&e))?;
        let status = res.status();
        tracing::info!(status = status.as_u16(), "respuesta AEAT");
        if !status.is_success() {
            return Err(AeatError::Status(status.as_u16()));
        }
        let body = read_limited(res, limits::DOCUMENT).await?;
        serde_json::from_slice(&body).map_err(|e| AeatError::InvalidResponse(e.to_string()))
    }
}

impl AeatClient {
    /// POST de un sobre SOAP 1.1. Devuelve el estado HTTP y el cuerpo (también en 500, donde
    /// la AEAT devuelve el SOAP Fault).
    pub async fn post_soap(&self, url: &str, envelope: &str) -> Result<(u16, String), AeatError> {
        let url = self.check_url(url)?;
        tracing::info!(
            host = url.host_str().unwrap_or(""),
            path = url.path(),
            "SOAP AEAT"
        );
        let res = self
            .http
            .post(url)
            .header("Content-Type", "text/xml; charset=UTF-8")
            .header("SOAPAction", "\"\"")
            .body(envelope.to_owned())
            .send()
            .await
            .map_err(|e| network_error(&e))?;
        let status = res.status().as_u16();
        tracing::info!(status, "respuesta AEAT");
        let body = read_text_limited(res, limits::DOCUMENT).await?;
        Ok((status, body))
    }
}

impl AeatClient {
    /// GET de un recurso binario (2xx), p. ej. el justificante en PDF.
    pub async fn get_bytes(&self, url: &str) -> Result<Vec<u8>, AeatError> {
        let url = self.check_url(url)?;
        tracing::info!(
            host = url.host_str().unwrap_or(""),
            path = url.path(),
            "GET AEAT"
        );
        let res = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| network_error(&e))?;
        let status = res.status();
        tracing::info!(status = status.as_u16(), "respuesta AEAT");
        if !status.is_success() {
            return Err(AeatError::Status(status.as_u16()));
        }
        read_limited(res, limits::DOCUMENT).await
    }

    /// POST `application/x-www-form-urlencoded`; devuelve el cuerpo de la respuesta (2xx).
    pub async fn post_form(&self, url: &str, form: &str) -> Result<String, AeatError> {
        let url = self.check_url(url)?;
        tracing::info!(
            host = url.host_str().unwrap_or(""),
            path = url.path(),
            "POST formulario AEAT"
        );
        let res = self
            .http
            .post(url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(form.to_owned())
            .send()
            .await
            .map_err(|e| network_error(&e))?;
        let status = res.status();
        tracing::info!(status = status.as_u16(), "respuesta AEAT");
        if !status.is_success() {
            return Err(AeatError::Status(status.as_u16()));
        }
        read_text_limited(res, limits::DOCUMENT).await
    }
}

/// Cliente HTTPS **sin certificado de cliente** para servicios públicos (BCE, cotejo por CSV):
/// servidor verificado con el almacén de confianza del sistema, sólo `https`, sin redirecciones
/// ni proxies. Quien lo usa comprueba el host antes de cada petición.
pub(crate) fn public_client(timeout: Duration) -> Result<reqwest::Client, AeatError> {
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
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(timeout)
        .connect_timeout(Duration::from_secs(10))
        .user_agent(concat!("TaxPain/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| AeatError::Tls(e.to_string()))
}

/// Error de red sin la URL: `Unreachable` si la conexión no llegó a abrirse, `Network` si falló
/// después (y la petición pudo llegar a la AEAT).
fn network_error(e: &reqwest::Error) -> AeatError {
    let msg = without_url(e);
    if e.is_connect() {
        AeatError::Unreachable(msg)
    } else {
        AeatError::Network(msg)
    }
}

/// Mensaje del error sin la URL (podría llevar NIF en la query).
pub(crate) fn without_url(e: &reqwest::Error) -> String {
    let mut msg = e.to_string();
    if let Some(u) = e.url() {
        msg = msg.replace(u.as_str(), "<url>");
    }
    let mut source = std::error::Error::source(e);
    while let Some(s) = source {
        msg.push_str(": ");
        msg.push_str(&s.to_string());
        source = s.source();
    }
    msg
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Servidor HTTP en 127.0.0.1 que contesta `head` y luego `body` (sin TLS: sólo se prueba
    /// la lectura).
    async fn respond(head: &'static str, body: Vec<u8>) -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf).await;
            let _ = s.write_all(head.as_bytes()).await;
            let _ = s.write_all(&body).await;
            let _ = s.shutdown().await;
        });
        format!("http://{addr}/")
    }

    async fn get(url: &str) -> reqwest::Response {
        let provider = std::sync::Arc::new(taxpain_vault::rustls::crypto::ring::default_provider());
        let tls = taxpain_vault::rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(RootCertStore::empty())
            .with_no_client_auth();
        reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .no_proxy()
            .build()
            .unwrap()
            .get(url)
            .send()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn lectura_con_tope() {
        // Dentro del tope, entera (como texto, con lo que no es UTF-8 sustituido).
        let url = respond(
            "HTTP/1.1 200 OK\r\ncontent-length: 5\r\nconnection: close\r\n\r\n",
            b"hol\xffa".to_vec(),
        )
        .await;
        assert_eq!(
            read_text_limited(get(&url).await, 5).await.unwrap(),
            "hol\u{fffd}a"
        );

        // Un Content-Length por encima del tope se rechaza sin leer el cuerpo.
        let url = respond(
            "HTTP/1.1 200 OK\r\ncontent-length: 1000000000\r\nconnection: close\r\n\r\n",
            Vec::new(),
        )
        .await;
        assert!(matches!(
            read_limited(get(&url).await, 1024).await,
            Err(AeatError::TooLarge(1024))
        ));

        // Sin Content-Length (hasta que se cierra) se corta al pasar el tope.
        let url = respond(
            "HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n",
            vec![b'a'; 64 * 1024],
        )
        .await;
        assert!(matches!(
            read_limited(get(&url).await, 1024).await,
            Err(AeatError::TooLarge(1024))
        ));

        // Por trozos, igual.
        let url = respond(
            "HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n",
            b"400\r\n"
                .iter()
                .copied()
                .chain(vec![b'a'; 0x400])
                .chain(b"\r\n1\r\nb\r\n0\r\n\r\n".iter().copied())
                .collect(),
        )
        .await;
        assert!(matches!(
            read_limited(get(&url).await, 0x400).await,
            Err(AeatError::TooLarge(_))
        ));
        assert_eq!(
            AeatError::TooLarge(limits::DOCUMENT).to_string(),
            "respuesta demasiado grande (más de 30 MB)"
        );
    }

    #[test]
    fn justificantes_del_entorno() {
        assert!(hosts::justificantes(true).contains(&"www1.agenciatributaria.gob.es"));
        assert!(
            hosts::justificantes(true)
                .iter()
                .all(|h| !h.contains("aeat.es"))
        );
        assert!(
            hosts::justificantes(false)
                .iter()
                .all(|h| h.ends_with(".aeat.es"))
        );
        // Las dos juntas son la lista de siempre.
        let mut all: Vec<&str> = [hosts::justificantes(true), hosts::justificantes(false)].concat();
        let mut old = hosts::JUSTIFICANTES.to_vec();
        all.sort_unstable();
        old.sort_unstable();
        assert_eq!(all, old);
    }

    #[test]
    fn puerto_solo_en_el_servidor_local_de_pruebas() {
        assert!(mock_port_allowed("localhost"));
        assert!(mock_port_allowed("127.0.0.1"));
        assert!(!mock_port_allowed("www1.agenciatributaria.gob.es"));
        assert!(!mock_port_allowed("prewww1.aeat.es"));
    }
}
