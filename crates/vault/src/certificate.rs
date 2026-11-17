//! Importación y validación de certificados `.p12/.pfx` (D5).
//!
//! Comprueba la contraseña (MAC PKCS#12), que haya clave privada y que corresponda al
//! certificado, la vigencia, el emisor (FNMT u otro), el tipo (persona física / representante
//! de persona jurídica) y el NIF del titular y de la entidad representada.
//!
//! La interpretación del sujeto (`parse_holder`) adapta `parseHolder` de Spain Assist
//! (`ref/spainassist/packages/vault/src/certificate.ts`, MIT, © 2026 Pablo Rodríguez Caballero;
//! ver `THIRD_PARTY_NOTICES.md`).
//!
//! `TODO(VERIFY)`: OIDs de política de la FNMT (persona física `1.3.6.1.4.1.5734.3.10.1`;
//! representante `1.3.6.1.4.1.5734.3.11.*`) contra la Declaración de Prácticas de
//! Certificación vigente. Hoy sólo se guardan como dato informativo; el tipo se decide por el
//! sujeto (NIF de entidad presente o no).

use ::time::OffsetDateTime;
use ::time::format_description::well_known::Rfc3339;
use p12_keystore::{KeyStore, Pkcs12ImportPolicy};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use serde::Serialize;
use sha2::{Digest, Sha256};
use taxpain_core::{Nif, NifKind};
use x509_parser::prelude::*;

use crate::error::{Result, VaultError};
use taxpain_secrets::SecretBytes;

/// Tamaño máximo aceptado. Un certificado personal ocupa unos pocos KB.
pub const MAX_P12_SIZE: usize = 256 * 1024;

const OID_CN: &str = "2.5.4.3";
const OID_SN: &str = "2.5.4.4";
const OID_SERIALNUMBER: &str = "2.5.4.5";
const OID_O: &str = "2.5.4.10";
const OID_GN: &str = "2.5.4.42";
const OID_ORGANIZATION_IDENTIFIER: &str = "2.5.4.97";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CertificateKind {
    PersonaFisica,
    /// Representante de persona jurídica o de entidad sin personalidad (necesario para SL).
    RepresentantePj,
    Otro,
}

impl CertificateKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CertificateKind::PersonaFisica => "persona_fisica",
            CertificateKind::RepresentantePj => "representante_pj",
            CertificateKind::Otro => "otro",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "persona_fisica" => Some(CertificateKind::PersonaFisica),
            "representante_pj" => Some(CertificateKind::RepresentantePj),
            "otro" => Some(CertificateKind::Otro),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IssuerKind {
    Fnmt,
    Otro,
}

impl IssuerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            IssuerKind::Fnmt => "fnmt",
            IssuerKind::Otro => "otro",
        }
    }
}

/// Resumen público del certificado: es lo único que llega al WebView.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CertificateInfo {
    /// SHA-256 del certificado (DER), hexadecimal minúsculas.
    pub fingerprint_sha256: String,
    pub subject_dn: String,
    pub issuer_dn: String,
    pub serial: String,
    /// RFC 3339 (UTC).
    pub valid_from: String,
    pub valid_to: String,
    pub kind: CertificateKind,
    pub issuer_kind: IssuerKind,
    pub holder_name: Option<String>,
    pub holder_nif: String,
    pub entity_nif: Option<String>,
    pub entity_name: Option<String>,
    /// p. ej. `"RSA 2048"`, `"EC 256"`.
    pub key_algorithm: String,
    pub policy_oids: Vec<String>,
}

/// Resultado de leer un `.p12`: metadatos + material secreto (sólo en el proceso nativo).
#[derive(Debug)]
pub struct ParsedCertificate {
    pub info: CertificateInfo,
    pub(crate) key_pkcs8: SecretBytes,
    pub(crate) chain_der: Vec<Vec<u8>>,
}

/// Lee y valida un `.p12/.pfx`. `now` decide la vigencia.
pub fn parse_pkcs12(
    data: &[u8],
    passphrase: &str,
    now: OffsetDateTime,
) -> Result<ParsedCertificate> {
    if data.is_empty() || data.len() > MAX_P12_SIZE {
        return Err(VaultError::InvalidCertificate(
            "el fichero no es un certificado .p12/.pfx".into(),
        ));
    }
    let store = KeyStore::from_pkcs12(data, passphrase, Pkcs12ImportPolicy::Strict)
        .map_err(map_p12_error)?;
    let (_, chain) = store.private_key_chain().ok_or_else(|| {
        VaultError::InvalidCertificate(
            "el fichero no contiene la clave privada: vuelve a exportarlo incluyéndola".into(),
        )
    })?;
    let key_pkcs8 = SecretBytes::new(chain.key().as_der().to_vec());
    let chain_der: Vec<Vec<u8>> = chain.certs().iter().map(|c| c.as_der().to_vec()).collect();
    let leaf_der = chain_der.first().ok_or_else(|| {
        VaultError::InvalidCertificate("el fichero no contiene certificado".into())
    })?;

    check_key_matches(leaf_der, &key_pkcs8)?;

    let (_, cert) = X509Certificate::from_der(leaf_der)
        .map_err(|_| VaultError::InvalidCertificate("certificado X.509 ilegible".into()))?;

    let not_before = cert.validity().not_before.timestamp();
    let not_after = cert.validity().not_after.timestamp();
    let valid_from = rfc3339(not_before)?;
    let valid_to = rfc3339(not_after)?;
    let now_ts = now.unix_timestamp();
    if now_ts < not_before {
        return Err(VaultError::NotYetValidCertificate(
            valid_from[..10].to_owned(),
        ));
    }
    if now_ts >= not_after {
        return Err(VaultError::ExpiredCertificate(valid_to[..10].to_owned()));
    }

    let subject = attributes(cert.subject());
    let issuer = attributes(cert.issuer());
    let holder = parse_holder(&subject);
    let holder_nif = holder.holder_nif.ok_or_else(|| {
        VaultError::InvalidCertificate(
            "no se encontró un NIF válido del titular en el certificado".into(),
        )
    })?;

    let issuer_kind = if first(&issuer, OID_O).is_some_and(|o| o.to_uppercase().contains("FNMT")) {
        IssuerKind::Fnmt
    } else {
        IssuerKind::Otro
    };
    let kind = if holder.entity_nif.is_some() {
        CertificateKind::RepresentantePj
    } else if Nif::parse(&holder_nif).is_ok_and(|n| n.is_persona_fisica()) {
        CertificateKind::PersonaFisica
    } else {
        CertificateKind::Otro
    };

    let key_algorithm = match cert.public_key().parsed() {
        Ok(x509_parser::public_key::PublicKey::RSA(k)) => format!("RSA {}", k.key_size()),
        Ok(x509_parser::public_key::PublicKey::EC(k)) => format!("EC {}", k.key_size()),
        _ => "desconocido".to_owned(),
    };
    let policy_oids = cert
        .extensions()
        .iter()
        .filter_map(|e| match e.parsed_extension() {
            ParsedExtension::CertificatePolicies(p) => Some(p),
            _ => None,
        })
        .flat_map(|p| p.iter().map(|pi| pi.policy_id.to_id_string()))
        .collect();

    let info = CertificateInfo {
        fingerprint_sha256: hex::encode(Sha256::digest(leaf_der)),
        subject_dn: cert.subject().to_string(),
        issuer_dn: cert.issuer().to_string(),
        serial: cert.raw_serial_as_string().replace(':', "").to_uppercase(),
        valid_from,
        valid_to,
        kind,
        issuer_kind,
        holder_name: holder.holder_name,
        holder_nif,
        entity_nif: holder.entity_nif,
        entity_name: holder.entity_name,
        key_algorithm,
        policy_oids,
    };
    tracing::debug!(fingerprint = %info.fingerprint_sha256, kind = ?info.kind, "certificado leído");
    Ok(ParsedCertificate {
        info,
        key_pkcs8,
        chain_der,
    })
}

fn map_p12_error(e: p12_keystore::error::Error) -> VaultError {
    use p12_keystore::error::Error as E;
    match e {
        // Sin MAC, una contraseña errónea se manifiesta como relleno o datos inválidos.
        E::MacError(_) | E::UnpadError | E::Pkcs5Error(_) => VaultError::InvalidPassphrase,
        E::UnsupportedEncryptionScheme | E::UnsupportedMacAlgorithm => {
            VaultError::InvalidCertificate("cifrado del fichero no soportado".into())
        }
        _ => VaultError::InvalidCertificate(
            "el fichero no es un certificado .p12/.pfx válido".into(),
        ),
    }
}

/// Comprueba con rustls que la clave privada corresponde a la clave pública del certificado.
fn check_key_matches(leaf_der: &[u8], key: &SecretBytes) -> Result<()> {
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.expose()));
    let signing = rustls::crypto::ring::sign::any_supported_type(&key_der)
        .map_err(|_| VaultError::InvalidCertificate("tipo de clave privada no soportado".into()))?;
    rustls::sign::CertifiedKey::new(vec![CertificateDer::from(leaf_der.to_vec())], signing)
        .keys_match()
        .map_err(|_| {
            VaultError::InvalidCertificate("la clave privada no corresponde al certificado".into())
        })
}

fn rfc3339(ts: i64) -> Result<String> {
    OffsetDateTime::from_unix_timestamp(ts)
        .ok()
        .and_then(|t| t.format(&Rfc3339).ok())
        .ok_or_else(|| VaultError::InvalidCertificate("fecha de validez fuera de rango".into()))
}

/// Atributos de un nombre X.500 como pares (OID, valor), en orden DER.
fn attributes(name: &X509Name<'_>) -> Vec<(String, String)> {
    name.iter_attributes()
        .filter_map(|a| {
            Some((
                a.attr_type().to_id_string(),
                a.as_str().ok()?.trim().to_owned(),
            ))
        })
        .filter(|(_, v)| !v.is_empty())
        .collect()
}

fn first<'a>(attrs: &'a [(String, String)], oid: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(o, _)| o == oid)
        .map(|(_, v)| v.as_str())
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Holder {
    pub holder_name: Option<String>,
    pub holder_nif: Option<String>,
    pub entity_nif: Option<String>,
    pub entity_name: Option<String>,
}

fn valid_persona_fisica(id: &str) -> Option<String> {
    Nif::parse(id)
        .ok()
        .filter(Nif::is_persona_fisica)
        .map(|n| n.as_str().to_owned())
}

fn valid_entity(id: &str) -> Option<String> {
    Nif::parse(id)
        .ok()
        .filter(|n| n.kind() == NifKind::Entidad)
        .map(|n| n.as_str().to_owned())
}

/// Busca un DNI/NIE en un texto ("… - 99999999R", "99999999R NOMBRE …").
fn id_in_text(text: &str) -> Option<String> {
    text.to_uppercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .find_map(valid_persona_fisica)
}

/// Titular y entidad a partir del sujeto. Ejemplos FNMT:
/// - persona física: CN "ESPAÑOLA ESPAÑOLA CARMEN - 99999999R", SERIALNUMBER "IDCES-99999999R"
/// - antiguos: CN "NOMBRE ESPAÑOLA ESPAÑOLA CARMEN - NIF 99999999R"
/// - representante: CN "99999999R CARMEN ESPAÑOLA (R: B12345674)",
///   organizationIdentifier "VATES-B12345674", O "EMPRESA DE PRUEBA SL"
pub(crate) fn parse_holder(subject: &[(String, String)]) -> Holder {
    let cn = first(subject, OID_CN);
    let serial = first(subject, OID_SERIALNUMBER);
    let gn = first(subject, OID_GN);
    let sn = first(subject, OID_SN);

    // ETSI EN 319 412-1: IDCES-, PASES-, PNOES-, TINES-.
    let holder_nif = serial
        .and_then(|s| {
            let up = s.to_uppercase();
            let id = ["IDC", "PAS", "PNO", "TIN"]
                .iter()
                .find_map(|p| {
                    up.strip_prefix(p)
                        .filter(|r| r.len() > 3 && r.get(2..3) == Some("-"))
                })
                .map_or(up.as_str(), |r| &r[3..]);
            let id = id
                .trim_start_matches("NIF")
                .trim_start_matches([' ', ':', '-']);
            valid_persona_fisica(id)
        })
        .or_else(|| cn.and_then(id_in_text));

    // Entidad: organizationIdentifier "VATES-B12345674" o "(R: B12345674)" en el CN.
    let entity_nif = first(subject, OID_ORGANIZATION_IDENTIFIER)
        .and_then(|v| valid_entity(v.to_uppercase().trim_start_matches("VATES-")))
        .or_else(|| {
            let up = cn?.to_uppercase();
            let start = up.find("(R:")? + 3;
            let end = up[start..].find(')')? + start;
            valid_entity(up[start..end].trim())
        });
    let entity_name = if entity_nif.is_some() {
        first(subject, OID_O).map(str::to_owned)
    } else {
        None
    };

    let holder_name = match (gn, sn) {
        (Some(g), Some(s)) => Some(format!("{g} {s}")),
        _ => cn.and_then(|cn| {
            let mut name = cn.to_owned();
            if let Some(i) = name.find("(R:") {
                name.truncate(i);
            }
            if let Some(i) = name.rfind(" - ") {
                name.truncate(i);
            }
            let mut name = name.trim().trim_start_matches("NOMBRE ").to_owned();
            if let Some(rest) = holder_nif.as_deref().and_then(|id| name.strip_prefix(id)) {
                name = rest.trim().to_owned();
            }
            (!name.is_empty()).then_some(name)
        }),
    };
    Holder {
        holder_name,
        holder_nif,
        entity_nif,
        entity_name,
    }
}

/// Serializa clave + cadena para sellarlas: `u32 n` y `n` × (`u32 len` ‖ bytes), big-endian.
/// El primer elemento es la clave PKCS#8; el resto, la cadena empezando por el certificado.
pub(crate) fn encode_secret(key: &SecretBytes, chain: &[Vec<u8>]) -> SecretBytes {
    let parts: Vec<&[u8]> = std::iter::once(key.expose())
        .chain(chain.iter().map(Vec::as_slice))
        .collect();
    let mut out = Vec::with_capacity(4 + parts.iter().map(|p| 4 + p.len()).sum::<usize>());
    out.extend_from_slice(&u32::try_from(parts.len()).unwrap_or(0).to_be_bytes());
    for p in parts {
        out.extend_from_slice(&u32::try_from(p.len()).unwrap_or(0).to_be_bytes());
        out.extend_from_slice(p);
    }
    SecretBytes::new(out)
}

pub(crate) fn decode_secret(data: &[u8]) -> Result<(SecretBytes, Vec<Vec<u8>>)> {
    fn take<'a>(data: &mut &'a [u8], n: usize) -> Result<&'a [u8]> {
        if data.len() < n {
            return Err(VaultError::Corrupted);
        }
        let (head, tail) = data.split_at(n);
        *data = tail;
        Ok(head)
    }
    let mut rest = data;
    let read_u32 = |d: &mut &[u8]| -> Result<usize> {
        let b: [u8; 4] = take(d, 4)?.try_into().map_err(|_| VaultError::Corrupted)?;
        Ok(u32::from_be_bytes(b) as usize)
    };
    let n = read_u32(&mut rest)?;
    if n < 2 {
        return Err(VaultError::Corrupted);
    }
    let len = read_u32(&mut rest)?;
    let key = SecretBytes::new(take(&mut rest, len)?.to_vec());
    let mut chain = Vec::with_capacity(n - 1);
    for _ in 1..n {
        let len = read_u32(&mut rest)?;
        chain.push(take(&mut rest, len)?.to_vec());
    }
    if !rest.is_empty() {
        return Err(VaultError::Corrupted);
    }
    Ok((key, chain))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(o, v)| ((*o).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn persona_fisica_fnmt() {
        let h = parse_holder(&attrs(&[
            (OID_SERIALNUMBER, "IDCES-12345678Z"),
            (OID_GN, "CARMEN"),
            (OID_SN, "ESPAÑOLA ESPAÑOLA"),
            (OID_CN, "ESPAÑOLA ESPAÑOLA CARMEN - 12345678Z"),
        ]));
        assert_eq!(h.holder_nif.as_deref(), Some("12345678Z"));
        assert_eq!(h.holder_name.as_deref(), Some("CARMEN ESPAÑOLA ESPAÑOLA"));
        assert_eq!(h.entity_nif, None);
    }

    #[test]
    fn persona_fisica_antigua_sin_gn() {
        let h = parse_holder(&attrs(&[(
            OID_CN,
            "NOMBRE ESPAÑOLA ESPAÑOLA CARMEN - NIF 12345678Z",
        )]));
        assert_eq!(h.holder_nif.as_deref(), Some("12345678Z"));
        assert_eq!(h.holder_name.as_deref(), Some("ESPAÑOLA ESPAÑOLA CARMEN"));
    }

    #[test]
    fn representante_persona_juridica() {
        let h = parse_holder(&attrs(&[
            (OID_CN, "12345678Z CARMEN ESPAÑOLA (R: B12345674)"),
            (OID_SERIALNUMBER, "IDCES-12345678Z"),
            (OID_ORGANIZATION_IDENTIFIER, "VATES-B12345674"),
            (OID_O, "EMPRESA DE PRUEBA SL"),
        ]));
        assert_eq!(h.holder_nif.as_deref(), Some("12345678Z"));
        assert_eq!(h.entity_nif.as_deref(), Some("B12345674"));
        assert_eq!(h.entity_name.as_deref(), Some("EMPRESA DE PRUEBA SL"));
        assert_eq!(h.holder_name.as_deref(), Some("CARMEN ESPAÑOLA"));
    }

    #[test]
    fn representante_solo_con_cn() {
        let h = parse_holder(&attrs(&[(OID_CN, "X1234567L ANNA SMITH (R: B12345674)")]));
        assert_eq!(h.holder_nif.as_deref(), Some("X1234567L"));
        assert_eq!(h.entity_nif.as_deref(), Some("B12345674"));
        assert_eq!(h.holder_name.as_deref(), Some("ANNA SMITH"));
    }

    #[test]
    fn nif_invalido_no_se_acepta() {
        let h = parse_holder(&attrs(&[
            (OID_SERIALNUMBER, "IDCES-12345678A"),
            (OID_CN, "X"),
        ]));
        assert_eq!(h.holder_nif, None);
    }

    #[test]
    fn secret_encoding_roundtrip_and_rejects_garbage() {
        let key = SecretBytes::new(vec![1, 2, 3]);
        let chain = vec![vec![4, 5], vec![6]];
        let enc = encode_secret(&key, &chain);
        let (k, c) = decode_secret(enc.expose()).unwrap();
        assert_eq!(k.expose(), key.expose());
        assert_eq!(c, chain);
        assert!(decode_secret(&enc.expose()[..enc.expose().len() - 1]).is_err());
        assert!(decode_secret(&[0, 0, 0, 1, 0, 0, 0, 0]).is_err());
    }
}
