//! Flex Web Service de Interactive Brokers (versión 3): descarga el extracto de una Activity
//! Flex Query con el token que el usuario genera en su Client Portal (Performance & Reports →
//! Flex Queries → Flex Web Service Configuration). Documentación de IBKR consultada el
//! 2026-11-02 (`interactivebrokers.com/docs/web-api/flex-web-service`):
//! - `GET …/FlexWebService/SendRequest?t={token}&q={queryId}&v=3[&fd=yyyyMMdd&td=yyyyMMdd]`
//!   responde `<FlexStatementResponse><Status>Success</Status><ReferenceCode>…` (o `Fail` con
//!   `<ErrorCode>` y `<ErrorMessage>`); la `<url>` de la respuesta se ignora, como pide IBKR.
//! - `GET …/FlexWebService/GetStatement?t={token}&q={ReferenceCode}&v=3` devuelve el informe
//!   (XML o CSV) o, mientras se genera, el error 1019 («Statement generation in progress»).
//! - Todas las peticiones han de llevar `User-Agent`; como mucho una por segundo y diez por
//!   minuto por token (error 1018); `fd`/`td` cubren como mucho 365 días.
//!
//! Mismas reglas que con los bancos y la AEAT: sólo `https`, sólo el host de IBKR, sin
//! redirecciones ni proxies, servidor verificado con el almacén de confianza del sistema y en
//! el registro sólo el host y el estado. El token va en la consulta (así lo pide el servicio):
//! los errores se dan sin la URL.

use std::sync::Arc;
use std::time::Duration;

use taxpain_vault::rustls::ClientConfig;
use time::Date;
use url::Url;

pub const HOST: &str = "ndcdyn.interactivebrokers.com";
const BASE: &str = "https://ndcdyn.interactivebrokers.com/AccountManagement/FlexWebService/";

/// Errores del Flex Web Service. Ninguno lleva la URL (que trae el token).
#[derive(Debug, thiserror::Error)]
pub enum FlexError {
    #[error("no se ha podido conectar con Interactive Brokers: {0}")]
    SinConexion(String),
    #[error("error de red con Interactive Brokers: {0}")]
    Red(String),
    #[error("Interactive Brokers ha respondido {0}")]
    Estado(u16),
    #[error("{mensaje} (error {codigo} del Flex Web Service)")]
    Servicio { codigo: u32, mensaje: String },
    #[error("respuesta del Flex Web Service no válida: {0}")]
    Respuesta(String),
    /// La respuesta pasa del tope de lo que se lee: no se termina de leer.
    #[error("respuesta del Flex Web Service demasiado grande: no se ha leído entera")]
    Grande,
    #[error(
        "el extracto aún no está listo tras {0} intentos: vuelve a sincronizar en unos minutos"
    )]
    NoListo(u32),
    #[error("{0}")]
    Datos(String),
    #[error("TLS: {0}")]
    Tls(String),
}

/// Errores que se arreglan esperando (el informe se está generando, demasiadas peticiones…).
fn reintentable(codigo: u32) -> bool {
    matches!(codigo, 1001 | 1004..=1009 | 1018 | 1019 | 1021)
}

/// Mensaje en llano de los errores que el usuario puede arreglar.
fn mensaje(codigo: u32, original: &str) -> String {
    match codigo {
        1010 => "es una Flex Query antigua (Legacy): crea una Activity Flex Query".into(),
        1011 => "la cuenta del Flex Web Service está inactiva en IBKR".into(),
        1012 => "el token del Flex Web Service ha caducado: genera uno nuevo en Client Portal \
                 (Flex Web Service Configuration) y guárdalo aquí"
            .into(),
        1013 => "IBKR no acepta peticiones desde esta dirección IP: quita la restricción de IP \
                 del token o añade la tuya"
            .into(),
        1014 => "la Flex Query no es válida: revisa el Query ID".into(),
        1015 => "el token no es válido: cópialo otra vez del Client Portal".into(),
        1016 => "la cuenta no es válida para esa Flex Query".into(),
        1025 => "demasiados intentos fallidos: espera un rato antes de volver a probar".into(),
        _ => original.chars().take(200).collect(),
    }
}

fn entre<'a>(texto: &'a str, a: &str, b: &str) -> Option<&'a str> {
    let i = texto.find(a)? + a.len();
    let j = texto[i..].find(b)? + i;
    Some(texto[i..j].trim())
}

/// Lo que dice un `FlexStatementResponse`: `Ok(código de referencia)` o `Err((código, mensaje))`.
fn respuesta(texto: &str) -> Result<std::result::Result<String, (u32, String)>, FlexError> {
    if !texto.contains("<FlexStatementResponse") {
        return Err(FlexError::Respuesta(
            "no es una respuesta del Flex Web Service".into(),
        ));
    }
    if let Some(r) = entre(texto, "<ReferenceCode>", "</ReferenceCode>")
        && !r.is_empty()
        && r.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return Ok(Ok(r.to_owned()));
    }
    let codigo = entre(texto, "<ErrorCode>", "</ErrorCode>")
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let msg = entre(texto, "<ErrorMessage>", "</ErrorMessage>").unwrap_or("error desconocido");
    Ok(Err((codigo, msg.to_owned())))
}

/// Mensaje del error sin la URL (lleva el token).
fn sin_url(e: &reqwest::Error) -> String {
    let mut msg = e.to_string();
    if let Some(u) = e.url() {
        msg = msg.replace(u.as_str(), "<url>");
    }
    let mut fuente = std::error::Error::source(e);
    while let Some(s) = fuente {
        msg.push_str(": ");
        msg.push_str(&s.to_string());
        fuente = s.source();
    }
    msg
}

fn red(e: &reqwest::Error) -> FlexError {
    if e.is_connect() {
        FlexError::SinConexion(sin_url(e))
    } else {
        FlexError::Red(sin_url(e))
    }
}

fn yyyymmdd(d: Date) -> String {
    format!("{:04}{:02}{:02}", d.year(), u8::from(d.month()), d.day())
}

/// Tramos de como mucho 365 días que cubren `desde..=hasta` (lo que admite `fd`/`td`).
pub fn tramos(desde: Date, hasta: Date) -> Vec<(Date, Date)> {
    let mut out = Vec::new();
    let mut a = desde;
    while a <= hasta {
        let b = (a + time::Duration::days(364)).min(hasta);
        out.push((a, b));
        let Some(siguiente) = b.next_day() else { break };
        a = siguiente;
    }
    out
}

/// Cliente del Flex Web Service.
pub struct ClienteFlex {
    http: reqwest::Client,
    base: Url,
    espera: Duration,
    intentos: u32,
}

impl std::fmt::Debug for ClienteFlex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClienteFlex")
            .field("host", &self.base.host_str())
            .finish_non_exhaustive()
    }
}

impl ClienteFlex {
    /// Cliente de producción (`ndcdyn.interactivebrokers.com`, servidor verificado con el
    /// almacén del sistema).
    pub fn nuevo() -> Result<Self, FlexError> {
        let proveedor = Arc::new(taxpain_vault::rustls::crypto::ring::default_provider());
        let verificador = rustls_platform_verifier::Verifier::new(proveedor.clone())
            .map_err(|e| FlexError::Tls(e.to_string()))?;
        let tls = ClientConfig::builder_with_provider(proveedor)
            .with_safe_default_protocol_versions()
            .map_err(|e| FlexError::Tls(e.to_string()))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verificador))
            .with_no_client_auth();
        Self::con_tls(tls, BASE)
    }

    /// Para las pruebas: contra `base` (p. ej. `https://localhost:<puerto>/…/`) con una CA
    /// propia. Sólo con la feature `mock`: la app no lo lleva.
    #[cfg(any(test, feature = "mock"))]
    pub fn con_raiz(raiz_der: &[u8], base: &str) -> Result<Self, FlexError> {
        let proveedor = Arc::new(taxpain_vault::rustls::crypto::ring::default_provider());
        let mut raices = taxpain_vault::rustls::RootCertStore::empty();
        raices
            .add(taxpain_vault::rustls::pki_types::CertificateDer::from(
                raiz_der.to_vec(),
            ))
            .map_err(|e| FlexError::Tls(e.to_string()))?;
        let tls = ClientConfig::builder_with_provider(proveedor)
            .with_safe_default_protocol_versions()
            .map_err(|e| FlexError::Tls(e.to_string()))?
            .with_root_certificates(raices)
            .with_no_client_auth();
        Self::con_tls(tls, base)
    }

    /// Espera entre intentos mientras se genera el extracto (por defecto, 5 s y 8 intentos: con la
    /// petición, menos de diez por minuto, el límite de IBKR por token).
    pub fn con_espera(mut self, espera: Duration, intentos: u32) -> Self {
        self.espera = espera;
        self.intentos = intentos.max(1);
        self
    }

    fn con_tls(mut tls: ClientConfig, base: &str) -> Result<Self, FlexError> {
        tls.alpn_protocols = vec![b"http/1.1".to_vec()];
        let base = Url::parse(base).map_err(|e| FlexError::Respuesta(e.to_string()))?;
        if base.scheme() != "https" {
            return Err(FlexError::Tls("sólo https".into()));
        }
        let http = reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .timeout(Duration::from_secs(120))
            .connect_timeout(Duration::from_secs(15))
            .user_agent(concat!("TaxPain/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| FlexError::Tls(e.to_string()))?;
        Ok(Self {
            http,
            base,
            espera: Duration::from_secs(5),
            intentos: 8,
        })
    }

    async fn pedir(&self, ruta: &str, consulta: &[(&str, &str)]) -> Result<String, FlexError> {
        let mut url = self
            .base
            .join(ruta)
            .map_err(|e| FlexError::Respuesta(e.to_string()))?;
        url.query_pairs_mut().extend_pairs(consulta.iter().copied());
        let r = self
            .http
            .get(url)
            .header("Accept", "application/xml, text/xml, text/plain")
            .send()
            .await
            .map_err(|e| red(&e))?;
        let estado = r.status().as_u16();
        tracing::info!(
            host = self.base.host_str(),
            peticion = ruta,
            estado,
            "flex web service"
        );
        if !(200..300).contains(&estado) {
            return Err(FlexError::Estado(estado));
        }
        crate::lectura::leer_texto_acotado(r, crate::lectura::topes::IBKR)
            .await
            .map_err(|e| match e {
                crate::lectura::Lectura::Grande => FlexError::Grande,
                crate::lectura::Lectura::Red(e) => red(&e),
            })
    }

    /// Pide el extracto de la Flex Query `query` entre `desde` y `hasta` (como mucho 365 días)
    /// y espera a que esté: devuelve el XML (o el CSV, según la query).
    pub async fn descargar(
        &self,
        token: &str,
        query: &str,
        desde: Date,
        hasta: Date,
    ) -> Result<String, FlexError> {
        if token.len() < 10 || !token.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(FlexError::Datos(
                "el token del Flex Web Service no es válido".into(),
            ));
        }
        if query.is_empty() || !query.chars().all(|c| c.is_ascii_digit()) {
            return Err(FlexError::Datos(
                "el Query ID de la Flex Query son sólo cifras".into(),
            ));
        }
        if hasta < desde || (hasta - desde).whole_days() > 365 {
            return Err(FlexError::Datos(
                "el Flex Web Service da como mucho 365 días por petición".into(),
            ));
        }
        let (fd, td) = (yyyymmdd(desde), yyyymmdd(hasta));
        let mut referencia = None;
        for intento in 0..self.intentos {
            let texto = self
                .pedir(
                    "SendRequest",
                    &[
                        ("t", token),
                        ("q", query),
                        ("v", "3"),
                        ("fd", &fd),
                        ("td", &td),
                    ],
                )
                .await?;
            match respuesta(&texto)? {
                Ok(r) => {
                    referencia = Some(r);
                    break;
                }
                Err((c, _)) if reintentable(c) && intento + 1 < self.intentos => {
                    tracing::info!(codigo = c, "flex web service: se reintenta");
                    tokio::time::sleep(self.espera * if c == 1018 { 3 } else { 1 }).await;
                }
                Err((c, m)) => {
                    return Err(FlexError::Servicio {
                        codigo: c,
                        mensaje: mensaje(c, &m),
                    });
                }
            }
        }
        let Some(referencia) = referencia else {
            return Err(FlexError::NoListo(self.intentos));
        };
        for intento in 0..self.intentos {
            // La primera vez también se espera: el extracto tarda unos segundos en generarse.
            tokio::time::sleep(self.espera).await;
            let texto = self
                .pedir(
                    "GetStatement",
                    &[("t", token), ("q", &referencia), ("v", "3")],
                )
                .await?;
            if !texto.contains("<FlexStatementResponse") {
                return Ok(texto);
            }
            match respuesta(&texto)? {
                Ok(_) => {
                    return Err(FlexError::Respuesta(
                        "GetStatement ha devuelto otra referencia".into(),
                    ));
                }
                Err((c, _)) if reintentable(c) && intento + 1 < self.intentos => {
                    if c == 1018 {
                        tokio::time::sleep(self.espera * 2).await;
                    }
                }
                Err((c, _)) if reintentable(c) => return Err(FlexError::NoListo(self.intentos)),
                Err((c, m)) => {
                    return Err(FlexError::Servicio {
                        codigo: c,
                        mensaje: mensaje(c, &m),
                    });
                }
            }
        }
        Err(FlexError::NoListo(self.intentos))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use time::macros::date;

    #[test]
    fn respuestas_del_servicio() {
        let ok = "<FlexStatementResponse timestamp=\"28 August, 2012 10:37 AM EDT\"><Status>Success</Status><ReferenceCode>1234567890</ReferenceCode><url>https://gdcdyn.interactivebrokers.com/Universal/servlet/FlexStatementService.GetStatement</url></FlexStatementResponse>";
        assert_eq!(respuesta(ok).unwrap(), Ok("1234567890".into()));
        let fail = "<FlexStatementResponse timestamp=\"x\"><Status>Fail</Status><ErrorCode>1012</ErrorCode><ErrorMessage>Token has expired.</ErrorMessage></FlexStatementResponse>";
        assert_eq!(
            respuesta(fail).unwrap(),
            Err((1012, "Token has expired.".into()))
        );
        assert!(mensaje(1012, "").contains("caducado"));
        assert!(reintentable(1019) && reintentable(1018) && !reintentable(1015));
        assert!(respuesta("<html>").is_err());
    }

    #[test]
    fn tramos_de_365_dias() {
        assert_eq!(
            tramos(date!(2026 - 01 - 01), date!(2026 - 12 - 31)),
            vec![(date!(2026 - 01 - 01), date!(2026 - 12 - 31))]
        );
        // Un año bisiesto entero no cabe en una petición.
        let t = tramos(date!(2028 - 01 - 01), date!(2028 - 12 - 31));
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].1, date!(2028 - 12 - 30));
        assert_eq!(t[1], (date!(2028 - 12 - 31), date!(2028 - 12 - 31)));
        assert_eq!(yyyymmdd(date!(2026 - 03 - 05)), "20260305");
    }
}
