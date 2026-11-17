//! Proveedores de modelo de lenguaje: Anthropic (Claude) o cualquier API compatible con la de
//! OpenAI (OpenAI, Mistral —con servidores en la UE— o un modelo local con Ollama/LM Studio).
//!
//! Sólo se conecta a una lista cerrada de hosts (o a la propia máquina para modelos locales),
//! sin seguir redirecciones: el texto anonimizado no puede acabar en otro sitio.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::AssistantError;

pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-opus-5-5";
pub const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Hosts remotos permitidos (coincidencia exacta).
pub const REMOTE_HOSTS: &[&str] = &["api.anthropic.com", "api.openai.com", "api.mistral.ai"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum Provider {
    Anthropic {
        model: String,
    },
    /// `base_url` termina en `/v1` (p. ej. `https://api.mistral.ai/v1` o
    /// `http://localhost:11434/v1`).
    OpenAiCompatible {
        base_url: String,
        model: String,
    },
}

impl Default for Provider {
    fn default() -> Self {
        Provider::Anthropic {
            model: DEFAULT_ANTHROPIC_MODEL.into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

fn is_loopback(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

impl Provider {
    pub fn model(&self) -> &str {
        match self {
            Provider::Anthropic { model } | Provider::OpenAiCompatible { model, .. } => model,
        }
    }

    /// URL del endpoint de chat, validada contra la política de destinos.
    pub fn endpoint(&self) -> Result<Url, AssistantError> {
        let raw = match self {
            Provider::Anthropic { .. } => ANTHROPIC_URL.to_owned(),
            Provider::OpenAiCompatible { base_url, .. } => {
                format!("{}/chat/completions", base_url.trim_end_matches('/'))
            }
        };
        let url = Url::parse(&raw).map_err(|_| AssistantError::InvalidUrl)?;
        let host = url
            .host_str()
            .ok_or(AssistantError::InvalidUrl)?
            .to_ascii_lowercase();
        if !url.username().is_empty() || url.password().is_some() {
            return Err(AssistantError::InvalidUrl);
        }
        match (url.scheme(), is_loopback(&host)) {
            // Modelo local: http o https, sólo en esta máquina.
            ("http" | "https", true) => Ok(url),
            ("https", false) if REMOTE_HOSTS.contains(&host.as_str()) => Ok(url),
            _ => Err(AssistantError::HostNotAllowed(host)),
        }
    }

    /// ¿Los datos salen del Mac? (Un modelo local no los saca.)
    pub fn is_remote(&self) -> bool {
        self.endpoint()
            .ok()
            .and_then(|u| u.host_str().map(|h| !is_loopback(h)))
            .unwrap_or(true)
    }

    pub fn needs_key(&self) -> bool {
        self.is_remote()
    }

    /// Host (en minúsculas) al que se conecta; `None` si el endpoint no es válido.
    pub fn endpoint_host(&self) -> Option<String> {
        self.endpoint()
            .ok()
            .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
    }

    /// Si pasar de `previous` a este proveedor necesita que el usuario vuelva a aceptar qué se
    /// envía: los datos saldrían hacia otro host (o salen por primera vez del Mac). Volver a un
    /// modelo local o cambiar sólo de modelo en el mismo host no lo necesita.
    pub fn needs_new_consent(&self, previous: &Provider) -> bool {
        let host = self.endpoint_host();
        self.is_remote()
            && (host.is_none() || !previous.is_remote() || host != previous.endpoint_host())
    }

    /// Cuerpo JSON de la petición.
    pub fn request_body(
        &self,
        system: &str,
        messages: &[Message],
        max_tokens: u32,
    ) -> serde_json::Value {
        match self {
            Provider::Anthropic { model } => serde_json::json!({
                "model": model,
                "max_tokens": max_tokens,
                "system": system,
                "messages": messages,
            }),
            Provider::OpenAiCompatible { model, .. } => {
                let mut all = vec![serde_json::json!({ "role": "system", "content": system })];
                all.extend(messages.iter().map(|m| serde_json::json!(m)));
                serde_json::json!({
                    "model": model,
                    "max_tokens": max_tokens,
                    "messages": all,
                })
            }
        }
    }

    /// Texto de la respuesta.
    pub fn parse_reply(&self, v: &serde_json::Value) -> Result<String, AssistantError> {
        if let Some(e) = v.get("error") {
            let msg = e
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("error del proveedor");
            return Err(AssistantError::Provider(msg.to_owned()));
        }
        let text = match self {
            Provider::Anthropic { .. } => {
                v.get("content").and_then(|c| c.as_array()).map(|blocks| {
                    blocks
                        .iter()
                        .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
                        .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join("")
                })
            }
            Provider::OpenAiCompatible { .. } => v
                .pointer("/choices/0/message/content")
                .and_then(|c| c.as_str())
                .map(str::to_owned),
        };
        text.filter(|t| !t.trim().is_empty())
            .ok_or_else(|| AssistantError::Provider("respuesta vacía o con otro formato".into()))
    }
}

/// Fichero que se manda tal cual al modelo («Full IA»): un PDF o una imagen. No se puede
/// anonimizar; sólo se usa con el permiso expreso del usuario.
#[derive(Clone, PartialEq, Eq)]
pub struct Attachment {
    pub filename: String,
    /// `application/pdf`, `image/jpeg`, `image/png`, `image/gif` o `image/webp`.
    pub media_type: String,
    pub data: Vec<u8>,
}

impl std::fmt::Debug for Attachment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Attachment")
            .field("media_type", &self.media_type)
            .field("bytes", &self.data.len())
            .finish_non_exhaustive()
    }
}

/// Tamaños máximos que se mandan (los límites de la API de Anthropic son 32 MB por petición y
/// 5 MB por imagen).
pub const MAX_PDF_BYTES: usize = 20 * 1024 * 1024;

/// Longitud máxima de la respuesta al leer un fichero adjunto. Un extracto bancario en PDF
/// lleva todos sus movimientos en la respuesta: con 4096 no cabían más de un centenar.
pub const ATTACHMENT_MAX_TOKENS: u32 = 16_384;
pub const MAX_IMAGE_BYTES: usize = 5 * 1024 * 1024;

impl Provider {
    /// Petición con un fichero adjunto y un texto.
    pub fn attachment_body(
        &self,
        system: &str,
        text: &str,
        file: &Attachment,
        max_tokens: u32,
    ) -> Result<serde_json::Value, AssistantError> {
        let pdf = file.media_type == "application/pdf";
        let limit = if pdf { MAX_PDF_BYTES } else { MAX_IMAGE_BYTES };
        if file.data.len() > limit {
            return Err(AssistantError::Provider(format!(
                "{} es demasiado grande para el asistente ({} MB como máximo)",
                file.filename,
                limit / 1024 / 1024
            )));
        }
        let b64 = taxpain_core::base64::encode(&file.data);
        Ok(match self {
            Provider::Anthropic { model } => {
                let block = if pdf {
                    serde_json::json!({ "type": "document", "source": {
                        "type": "base64", "media_type": "application/pdf", "data": b64 } })
                } else {
                    serde_json::json!({ "type": "image", "source": {
                        "type": "base64", "media_type": file.media_type, "data": b64 } })
                };
                serde_json::json!({
                    "model": model,
                    "max_tokens": max_tokens,
                    "system": system,
                    "messages": [{ "role": "user", "content": [block, { "type": "text", "text": text }] }],
                })
            }
            Provider::OpenAiCompatible { model, .. } => {
                let data_url = format!("data:{};base64,{b64}", file.media_type);
                let block = if pdf {
                    serde_json::json!({ "type": "file", "file": {
                        "filename": file.filename, "file_data": data_url } })
                } else {
                    serde_json::json!({ "type": "image_url", "image_url": { "url": data_url } })
                };
                serde_json::json!({
                    "model": model,
                    "max_tokens": max_tokens,
                    "messages": [
                        { "role": "system", "content": system },
                        { "role": "user", "content": [block, { "type": "text", "text": text }] },
                    ],
                })
            }
        })
    }
}

/// Lo que se espera la respuesta del proveedor (lo dice [`AssistantError::Timeout`]).
pub const TIMEOUT: Duration = Duration::from_secs(180);
/// Lo que se espera a que se haga la conexión.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// Un error de red sin la URL y dicho en claro: no se ha podido conectar (con un modelo local,
/// que no está arrancado), el proveedor no ha respondido a tiempo o la conexión se ha cortado
/// antes de su respuesta. `remote`: el proveedor no es un modelo local.
fn network_error(e: &reqwest::Error, remote: bool) -> AssistantError {
    // Sin llegar a conectar no es que no haya respondido: un tiempo de conexión agotado también
    // es `is_timeout`.
    if e.is_connect() {
        AssistantError::Network(
            if remote {
                "no se ha podido conectar con el proveedor: comprueba la conexión a internet y \
                 vuelve a preguntar"
            } else {
                "no se ha podido conectar con el modelo local: arráncalo (p. ej. «ollama serve») \
                 y vuelve a preguntar"
            }
            .into(),
        )
    } else if e.is_timeout() {
        AssistantError::Timeout
    } else {
        AssistantError::Network(
            "se ha cortado la conexión con el proveedor antes de su respuesta: vuelve a \
             preguntar"
                .into(),
        )
    }
}

/// Topes de lo que se lee de una respuesta: un «modelo local» es cualquier proceso que escuche
/// en ese puerto y podría responder sin fin.
pub(crate) mod limits {
    /// Respuesta del proveedor de IA.
    pub const PROVIDER: usize = 10 * 1024 * 1024;
    /// Respuesta del BOE (una búsqueda o un artículo).
    pub const BOE: usize = 5 * 1024 * 1024;
}

/// Por qué no se ha leído una respuesta entera.
#[derive(Debug)]
pub(crate) enum Limited {
    /// Pasa del tope.
    TooLarge,
    /// Error de red al leerla.
    Http(reqwest::Error),
}

/// Lee el cuerpo de `res` hasta `max` bytes: rechaza antes de leer si el `Content-Length` ya
/// pasa del tope y, si no viene o miente, corta en cuanto lo leído lo pasa.
pub(crate) async fn read_limited(
    mut res: reqwest::Response,
    max: usize,
) -> Result<Vec<u8>, Limited> {
    if res
        .content_length()
        .is_some_and(|n| n > u64::try_from(max).unwrap_or(u64::MAX))
    {
        return Err(Limited::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = res.chunk().await.map_err(Limited::Http)? {
        if body.len().saturating_add(chunk.len()) > max {
            return Err(Limited::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// Cliente HTTP: TLS con el almacén de confianza del sistema, sin redirecciones ni proxy.
pub struct Client {
    http: reqwest::Client,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client").finish_non_exhaustive()
    }
}

impl Client {
    pub fn new() -> Result<Self, AssistantError> {
        Self::with_timeout(TIMEOUT)
    }

    /// Con otro tiempo de espera (las pruebas no esperan tres minutos).
    pub(crate) fn with_timeout(timeout: Duration) -> Result<Self, AssistantError> {
        Self::with_timeouts(timeout, CONNECT_TIMEOUT)
    }

    /// Con otros tiempos de espera de la respuesta y de la conexión.
    pub(crate) fn with_timeouts(
        timeout: Duration,
        connect: Duration,
    ) -> Result<Self, AssistantError> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let verifier = rustls_platform_verifier::Verifier::new(provider.clone())
            .map_err(|e| AssistantError::Network(e.to_string()))?;
        let tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| AssistantError::Network(e.to_string()))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .timeout(timeout)
            .connect_timeout(connect)
            .user_agent(concat!("TaxPain/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| AssistantError::Network(e.to_string()))?;
        Ok(Client { http })
    }

    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// Envía la conversación y devuelve el texto de la respuesta.
    pub async fn chat(
        &self,
        provider: &Provider,
        api_key: Option<&str>,
        system: &str,
        messages: &[Message],
    ) -> Result<String, AssistantError> {
        let body = provider.request_body(system, messages, 2048);
        let v = self.post_json(provider, api_key, &body).await?;
        provider.parse_reply(&v)
    }

    /// Manda un fichero (PDF o imagen) con un texto y devuelve la respuesta («Full IA»).
    pub async fn ask_attachment(
        &self,
        provider: &Provider,
        api_key: Option<&str>,
        system: &str,
        text: &str,
        file: &Attachment,
    ) -> Result<String, AssistantError> {
        if let Some(why) = crate::pii::looks_secret(text) {
            return Err(AssistantError::SecretDetected(why));
        }
        let body = provider.attachment_body(system, text, file, ATTACHMENT_MAX_TOKENS)?;
        let v = self.post_json(provider, api_key, &body).await?;
        provider.parse_reply(&v)
    }

    /// Envía un cuerpo JSON al endpoint del proveedor (validado) y devuelve la respuesta.
    pub async fn post_json(
        &self,
        provider: &Provider,
        api_key: Option<&str>,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, AssistantError> {
        let url = provider.endpoint()?;
        if provider.needs_key() && api_key.is_none_or(|k| k.trim().is_empty()) {
            return Err(AssistantError::MissingKey);
        }
        let mut req = self.http.post(url.clone()).json(body);
        // Nunca se envía una clave a un modelo local (podría escuchar cualquier proceso).
        let api_key = api_key.filter(|_| provider.needs_key());
        req = match (provider, api_key) {
            (Provider::Anthropic { .. }, Some(k)) => req
                .header("x-api-key", k)
                .header("anthropic-version", ANTHROPIC_VERSION),
            (Provider::OpenAiCompatible { .. }, Some(k)) if !k.trim().is_empty() => {
                req.bearer_auth(k)
            }
            _ => req,
        };
        tracing::info!(host = url.host_str().unwrap_or(""), "petición al asistente");
        let res = req
            .send()
            .await
            .map_err(|e| network_error(&e, provider.is_remote()))?;
        let status = res.status();
        let body = read_limited(res, limits::PROVIDER)
            .await
            .map_err(|e| match e {
                Limited::TooLarge => AssistantError::TooLarge,
                Limited::Http(e) if e.is_timeout() => AssistantError::Timeout,
                Limited::Http(_) => {
                    AssistantError::Provider(format!("respuesta HTTP {status} no válida"))
                }
            })?;
        let v: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|_| AssistantError::Provider(format!("respuesta HTTP {status} no válida")))?;
        if !status.is_success() && v.get("error").is_none() {
            return Err(AssistantError::Provider(format!("HTTP {status}")));
        }
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un servidor en 127.0.0.1 que acepta conexiones y, con `cortar`, las cierra sin responder;
    /// si no, las deja abiertas sin responder nunca.
    async fn mudo(cortar: bool) -> Provider {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        let addr = l.local_addr().unwrap_or_else(|e| panic!("{e}"));
        tokio::spawn(async move {
            let mut abiertas = Vec::new();
            while let Ok((mut s, _)) = l.accept().await {
                if cortar {
                    use tokio::io::AsyncReadExt;
                    let mut buf = [0u8; 4096];
                    let _ = s.read(&mut buf).await;
                    drop(s);
                } else {
                    abiertas.push(s);
                }
            }
        });
        Provider::OpenAiCompatible {
            base_url: format!("http://{addr}/v1"),
            model: "local".into(),
        }
    }

    /// Un «modelo local» que responde `head` y luego `len` bytes sin fin aparente.
    async fn parlanchin(head: &'static str, len: usize) -> Provider {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        let addr = l.local_addr().unwrap_or_else(|e| panic!("{e}"));
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            while let Ok((mut s, _)) = l.accept().await {
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf).await;
                let _ = s.write_all(head.as_bytes()).await;
                let trozo = vec![b' '; 64 * 1024];
                let mut enviado = 0;
                while enviado < len {
                    if s.write_all(&trozo).await.is_err() {
                        break;
                    }
                    enviado += trozo.len();
                }
                let _ = s.shutdown().await;
            }
        });
        Provider::OpenAiCompatible {
            base_url: format!("http://{addr}/v1"),
            model: "local".into(),
        }
    }

    #[tokio::test]
    async fn una_respuesta_enorme_no_se_lee_entera() {
        let msgs = [Message {
            role: Role::User,
            content: "Hola".into(),
        }];
        let client = Client::new().unwrap_or_else(|e| panic!("{e}"));
        // Lo anuncia en el Content-Length: ni se empieza a leer.
        let p = parlanchin(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 5000000000\r\n\r\n",
            0,
        )
        .await;
        assert_eq!(
            client.chat(&p, None, "s", &msgs).await.err(),
            Some(AssistantError::TooLarge)
        );
        // Sin Content-Length: se corta al pasar el tope.
        let p = parlanchin(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n",
            limits::PROVIDER + 256 * 1024,
        )
        .await;
        assert_eq!(
            client.chat(&p, None, "s", &msgs).await.err(),
            Some(AssistantError::TooLarge)
        );
        assert_eq!(
            AssistantError::TooLarge.to_string(),
            "respuesta demasiado grande: no se ha leído entera"
        );
    }

    #[test]
    fn host_del_proveedor_y_nuevo_consentimiento() {
        let anth = Provider::Anthropic { model: "m".into() };
        let anth2 = Provider::Anthropic {
            model: "otro".into(),
        };
        let oai = |base: &str| Provider::OpenAiCompatible {
            base_url: base.into(),
            model: "m".into(),
        };
        let mistral = oai("https://API.mistral.ai/v1");
        let local = oai("http://127.0.0.1:11434/v1");
        let malo = oai("https://ejemplo.invalid/v1");
        assert_eq!(anth.endpoint_host().as_deref(), Some("api.anthropic.com"));
        assert_eq!(mistral.endpoint_host().as_deref(), Some("api.mistral.ai"));
        assert_eq!(local.endpoint_host().as_deref(), Some("127.0.0.1"));
        assert_eq!(malo.endpoint_host(), None);
        // A otro host remoto, o de local a remoto: se vuelve a aceptar.
        assert!(mistral.needs_new_consent(&anth));
        assert!(anth.needs_new_consent(&local));
        assert!(malo.needs_new_consent(&anth));
        // Otro modelo en el mismo host, o a un modelo local: no.
        assert!(!anth2.needs_new_consent(&anth));
        assert!(!oai("https://api.mistral.ai/v1/").needs_new_consent(&mistral));
        assert!(!local.needs_new_consent(&anth));
    }

    #[tokio::test]
    async fn si_el_proveedor_no_responde_a_tiempo_lo_dice_claro() {
        let msgs = [Message {
            role: Role::User,
            content: "Hola".into(),
        }];
        let client =
            Client::with_timeout(Duration::from_millis(300)).unwrap_or_else(|e| panic!("{e}"));
        let e = client
            .chat(&mudo(false).await, None, "s", &msgs)
            .await
            .err();
        assert_eq!(e, Some(AssistantError::Timeout));
        assert_eq!(
            AssistantError::Timeout.to_string(),
            "El proveedor no ha respondido en 3 minutos: vuelve a preguntar."
        );
        // Una conexión que se corta sin respuesta es otra cosa (y lo dice en castellano).
        let e = client
            .chat(&mudo(true).await, None, "s", &msgs)
            .await
            .err()
            .map(|e| e.to_string());
        assert_eq!(
            e.as_deref(),
            Some(
                "error de red: se ha cortado la conexión con el proveedor antes de su respuesta: \
                 vuelve a preguntar"
            )
        );
    }

    /// Un servidor en 127.0.0.1 con la cola de conexiones llena y que no acepta ninguna: la
    /// conexión no llega a hacerse, como con un cortafuegos que tira los paquetes. Lo que llena
    /// la cola va con él.
    fn sin_sitio() -> (
        std::net::SocketAddr,
        tokio::net::TcpListener,
        Vec<std::net::TcpStream>,
    ) {
        let s = tokio::net::TcpSocket::new_v4().unwrap_or_else(|e| panic!("{e}"));
        s.bind(std::net::SocketAddr::from(([127, 0, 0, 1], 0)))
            .unwrap_or_else(|e| panic!("{e}"));
        let l = s.listen(1).unwrap_or_else(|e| panic!("{e}"));
        let addr = l.local_addr().unwrap_or_else(|e| panic!("{e}"));
        let mut cola = Vec::new();
        // Hasta que una ya no se conecta: la cola está llena.
        for _ in 0..16 {
            match std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(300)) {
                Ok(c) => cola.push(c),
                Err(_) => break,
            }
        }
        (addr, l, cola)
    }

    #[tokio::test]
    async fn sin_poder_conectar_lo_dice_y_con_un_modelo_local_pide_arrancarlo() {
        let msgs = [Message {
            role: Role::User,
            content: "Hola".into(),
        }];
        // La conexión no se llega a hacer: no es que el proveedor no haya respondido a tiempo.
        let (addr, _l, _cola) = sin_sitio();
        let client = Client::with_timeouts(Duration::from_secs(10), Duration::from_millis(300))
            .unwrap_or_else(|e| panic!("{e}"));
        let e = client
            .http()
            .post(format!("http://{addr}/v1/chat/completions"))
            .body("{}")
            .send()
            .await
            .err()
            .unwrap_or_else(|| panic!("ha conectado"));
        assert!(e.is_connect(), "{e:?}");
        assert_eq!(
            network_error(&e, true).to_string(),
            "error de red: no se ha podido conectar con el proveedor: comprueba la conexión a \
             internet y vuelve a preguntar"
        );
        let local = Provider::OpenAiCompatible {
            base_url: format!("http://{addr}/v1"),
            model: "llama3.1".into(),
        };
        let arrancalo = "error de red: no se ha podido conectar con el modelo local: arráncalo \
                         (p. ej. «ollama serve») y vuelve a preguntar";
        let e = client
            .chat(&local, None, "s", &msgs)
            .await
            .err()
            .map(|e| e.to_string());
        assert_eq!(e.as_deref(), Some(arrancalo));
        // Un modelo local que no está arrancado (nadie escucha en su puerto).
        let cerrado = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .unwrap_or_else(|e| panic!("{e}"));
        let local = Provider::OpenAiCompatible {
            base_url: format!("http://{cerrado}/v1"),
            model: "llama3.1".into(),
        };
        let e = Client::new()
            .unwrap_or_else(|e| panic!("{e}"))
            .chat(&local, None, "s", &msgs)
            .await
            .err()
            .map(|e| e.to_string());
        assert_eq!(e.as_deref(), Some(arrancalo));
    }

    #[test]
    fn adjuntos_en_cada_proveedor() {
        let pdf = Attachment {
            filename: "f.pdf".into(),
            media_type: "application/pdf".into(),
            data: b"%PDF-1.4".to_vec(),
        };
        let b = Provider::default()
            .attachment_body("sis", "lee", &pdf, 100)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(b["messages"][0]["content"][0]["type"], "document");
        assert_eq!(
            b["messages"][0]["content"][0]["source"]["media_type"],
            "application/pdf"
        );
        assert_eq!(b["messages"][0]["content"][1]["text"], "lee");
        let img = Attachment {
            filename: "f.jpg".into(),
            media_type: "image/jpeg".into(),
            data: vec![0xFF, 0xD8],
        };
        let local = Provider::OpenAiCompatible {
            base_url: "http://localhost:11434/v1".into(),
            model: "llava".into(),
        };
        let b = local
            .attachment_body("sis", "lee", &img, 100)
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(
            b["messages"][1]["content"][0]["image_url"]["url"]
                .as_str()
                .is_some_and(|u| u.starts_with("data:image/jpeg;base64,"))
        );
        let big = Attachment {
            data: vec![0; MAX_IMAGE_BYTES + 1],
            ..img
        };
        assert!(
            Provider::default()
                .attachment_body("s", "t", &big, 1)
                .is_err()
        );
    }

    #[test]
    fn destinos_permitidos() {
        assert!(Provider::default().endpoint().is_ok());
        assert!(Provider::default().is_remote());
        let local = Provider::OpenAiCompatible {
            base_url: "http://localhost:11434/v1".into(),
            model: "llama3.1".into(),
        };
        assert_eq!(
            local.endpoint().map(|u| u.to_string()).ok().as_deref(),
            Some("http://localhost:11434/v1/chat/completions")
        );
        assert!(!local.is_remote() && !local.needs_key());
        let mistral = Provider::OpenAiCompatible {
            base_url: "https://api.mistral.ai/v1".into(),
            model: "mistral-large-latest".into(),
        };
        assert!(mistral.endpoint().is_ok() && mistral.needs_key());
        for bad in [
            "https://evil.example/v1",
            "http://api.openai.com/v1",
            "https://user:pw@api.openai.com/v1",
            "https://api.openai.com.evil.example/v1",
        ] {
            let p = Provider::OpenAiCompatible {
                base_url: bad.into(),
                model: "x".into(),
            };
            assert!(p.endpoint().is_err(), "{bad}");
        }
    }

    #[test]
    fn cuerpos_y_respuestas() {
        let msgs = [Message {
            role: Role::User,
            content: "Hola".into(),
        }];
        let a = Provider::default();
        let body = a.request_body("sistema", &msgs, 100);
        assert_eq!(body["system"], "sistema");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["model"], DEFAULT_ANTHROPIC_MODEL);
        let reply = serde_json::json!({
            "content": [{"type": "text", "text": "Hola, "}, {"type": "text", "text": "Carmen"}]
        });
        assert_eq!(a.parse_reply(&reply).ok().as_deref(), Some("Hola, Carmen"));
        let err = serde_json::json!({"type": "error", "error": {"message": "invalid x-api-key"}});
        assert!(
            matches!(a.parse_reply(&err), Err(AssistantError::Provider(m)) if m.contains("api-key"))
        );

        let o = Provider::OpenAiCompatible {
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-x".into(),
        };
        let body = o.request_body("sistema", &msgs, 100);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "Hola");
        let reply = serde_json::json!({"choices": [{"message": {"content": "Vale"}}]});
        assert_eq!(o.parse_reply(&reply).ok().as_deref(), Some("Vale"));
        assert!(o.parse_reply(&serde_json::json!({})).is_err());
    }
}
