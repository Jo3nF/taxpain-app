//! Firma de las peticiones a Enable Banking: un JWT RS256 con la clave privada de la aplicación
//! de la empresa (cabecera `kid` = id de la aplicación; `iss` «enablebanking.com», `aud`
//! «api.enablebanking.com»; como mucho 24 h de validez, según su referencia de la API).

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use taxpain_vault::rustls::SignatureScheme;
use taxpain_vault::rustls::pki_types::PrivateKeyDer;
use taxpain_vault::rustls::pki_types::pem::PemObject;
use taxpain_vault::rustls::sign::SigningKey;

use crate::{BancaError, Result};

/// Emisor y destinatario que exige Enable Banking.
const ISS: &str = "enablebanking.com";
const AUD: &str = "api.enablebanking.com";
/// Validez de cada token: corta (una hora), aunque Enable Banking admite hasta 24 h.
pub const VALIDEZ_S: i64 = 3_600;

/// Firma JWT con la clave de la aplicación. La clave no sale de aquí (ni en `Debug`).
pub struct Firmante {
    kid: String,
    clave: Arc<dyn SigningKey>,
}

impl std::fmt::Debug for Firmante {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Firmante")
            .field("kid", &self.kid)
            .finish_non_exhaustive()
    }
}

impl Firmante {
    /// Con el id de la aplicación (`kid`) y su clave privada en PEM (PKCS#8 «BEGIN PRIVATE
    /// KEY», la que descarga el navegador al registrar la aplicación, o PKCS#1 «BEGIN RSA
    /// PRIVATE KEY», la de OpenSSL). Comprueba que es RSA (RS256 es el único algoritmo que
    /// admite Enable Banking).
    pub fn nuevo(kid: &str, pem: &str) -> Result<Self> {
        let kid = kid.trim();
        if kid.is_empty() || kid.len() > 200 || kid.chars().any(char::is_whitespace) {
            return Err(BancaError::Clave(
                "falta el id de la aplicación (el que da Enable Banking al registrarla)".into(),
            ));
        }
        let der = PrivateKeyDer::from_pem_slice(pem.trim().as_bytes()).map_err(|_| {
            BancaError::Clave(
                "no es una clave privada en formato PEM (el fichero .pem de la aplicación)".into(),
            )
        })?;
        let clave = taxpain_vault::rustls::crypto::ring::sign::any_supported_type(&der)
            .map_err(|_| BancaError::Clave("tipo de clave no admitido".into()))?;
        if clave
            .choose_scheme(&[SignatureScheme::RSA_PKCS1_SHA256])
            .is_none()
        {
            return Err(BancaError::Clave(
                "la clave tiene que ser RSA (Enable Banking firma con RS256)".into(),
            ));
        }
        Ok(Self {
            kid: kid.to_owned(),
            clave,
        })
    }

    /// Id de la aplicación.
    pub fn kid(&self) -> &str {
        &self.kid
    }

    /// Token firmado válido desde `ahora` (segundos Unix) durante [`VALIDEZ_S`].
    pub fn token(&self, ahora: i64) -> Result<String> {
        let cabecera = serde_json::json!({ "typ": "JWT", "alg": "RS256", "kid": self.kid });
        let cuerpo = serde_json::json!({
            "iss": ISS,
            "aud": AUD,
            "iat": ahora,
            "exp": ahora + VALIDEZ_S,
        });
        let entrada = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(cabecera.to_string()),
            URL_SAFE_NO_PAD.encode(cuerpo.to_string())
        );
        let firmador = self
            .clave
            .choose_scheme(&[SignatureScheme::RSA_PKCS1_SHA256])
            .ok_or_else(|| BancaError::Clave("la clave no firma con RS256".into()))?;
        let firma = firmador
            .sign(entrada.as_bytes())
            .map_err(|e| BancaError::Clave(format!("no se ha podido firmar: {e}")))?;
        Ok(format!("{entrada}.{}", URL_SAFE_NO_PAD.encode(firma)))
    }
}
