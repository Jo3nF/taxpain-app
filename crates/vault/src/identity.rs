//! Identidad de cliente TLS desellada en memoria (D5). Sólo vive mientras dura la operación.
//!
//! El trait-objeto de firma de rustls se crea **prestando** la clave PKCS#8 (sin copiarla a un
//! `Vec` que nadie borraría); la clave en claro sólo vive en `SecretBytes`, que se borra
//! (`zeroize`) al soltarse. `ring` guarda su propia representación interna de la clave mientras
//! dura la configuración TLS; eso no se puede borrar desde fuera.
//! Alternativa futura: identidad no exportable del Keychain con un `SigningKey` propio.

use std::fmt;
use std::sync::Arc;

use rustls::client::ResolvesClientCert;
use rustls::client::danger::ServerCertVerifier;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::sign::CertifiedKey;
use rustls::{ClientConfig, RootCertStore, SignatureScheme};

use crate::error::{Result, VaultError};
use taxpain_secrets::SecretBytes;

pub struct ClientIdentity {
    fingerprint: String,
    chain: Vec<CertificateDer<'static>>,
    key: SecretBytes,
}

impl ClientIdentity {
    pub(crate) fn new(fingerprint: String, key: SecretBytes, chain: Vec<Vec<u8>>) -> Self {
        ClientIdentity {
            fingerprint,
            chain: chain.into_iter().map(CertificateDer::from).collect(),
            key,
        }
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    /// Configuración rustls (proveedor `ring`, TLS 1.2 y 1.3) que presenta esta identidad y
    /// confía sólo en `roots` para el servidor.
    pub fn client_config(&self, roots: RootCertStore) -> Result<ClientConfig> {
        let resolver = self.resolver()?;
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map(|b| {
                b.with_root_certificates(roots)
                    .with_client_cert_resolver(resolver)
            })
            .map_err(|e| VaultError::InvalidCertificate(format!("identidad TLS: {e}")))
    }

    /// Clave de firma creada a partir de la clave prestada (sin copias en claro).
    fn resolver(&self) -> Result<Arc<dyn ResolvesClientCert>> {
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.expose()));
        let signing = rustls::crypto::ring::sign::any_supported_type(&key).map_err(|_| {
            VaultError::InvalidCertificate("tipo de clave privada no soportado".into())
        })?;
        let certified = CertifiedKey::new(self.chain.clone(), signing);
        certified
            .keys_match()
            .map_err(|e| VaultError::InvalidCertificate(format!("identidad TLS: {e}")))?;
        Ok(Arc::new(FixedClientCert(Arc::new(certified))))
    }
}

/// Presenta siempre la misma identidad (como hace rustls con `with_client_auth_cert`).
#[derive(Debug)]
struct FixedClientCert(Arc<CertifiedKey>);

impl ResolvesClientCert for FixedClientCert {
    fn resolve(
        &self,
        _root_hint_subjects: &[&[u8]],
        _sigschemes: &[SignatureScheme],
    ) -> Option<Arc<CertifiedKey>> {
        Some(Arc::clone(&self.0))
    }

    fn has_certs(&self) -> bool {
        true
    }
}

impl ClientIdentity {
    /// Certificados de la cadena en DER (el del titular primero).
    pub fn chain_der(&self) -> Vec<Vec<u8>> {
        self.chain.iter().map(|c| c.as_ref().to_vec()).collect()
    }

    /// Firma `data` con la clave del certificado: RSA PKCS#1 v1.5 con SHA-512 (la que usa la
    /// firma XAdES de Facturae). Sólo claves RSA.
    pub fn sign_rsa_sha512(&self, data: &[u8]) -> Result<Vec<u8>> {
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key.expose()));
        let signing = rustls::crypto::ring::sign::any_supported_type(&key).map_err(|_| {
            VaultError::InvalidCertificate("tipo de clave privada no soportado".into())
        })?;
        let signer = signing
            .choose_scheme(&[SignatureScheme::RSA_PKCS1_SHA512])
            .ok_or_else(|| {
                VaultError::InvalidCertificate(
                    "la firma de facturas necesita un certificado con clave RSA".into(),
                )
            })?;
        signer
            .sign(data)
            .map_err(|e| VaultError::InvalidCertificate(format!("firma: {e}")))
    }

    /// Igual que `client_config`, verificando el servidor con `verifier` (p. ej. el almacén de
    /// confianza del sistema operativo).
    pub fn client_config_with_verifier(
        &self,
        verifier: Arc<dyn ServerCertVerifier>,
    ) -> Result<ClientConfig> {
        let resolver = self.resolver()?;
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_safe_default_protocol_versions()
            .map(|b| {
                b.dangerous()
                    .with_custom_certificate_verifier(verifier)
                    .with_client_cert_resolver(resolver)
            })
            .map_err(|e| VaultError::InvalidCertificate(format!("identidad TLS: {e}")))
    }
}

impl fmt::Debug for ClientIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClientIdentity")
            .field("fingerprint", &self.fingerprint)
            .field("chain_len", &self.chain.len())
            .field("key", &"[REDACTED]")
            .finish()
    }
}
