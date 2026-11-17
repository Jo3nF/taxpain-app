//! Firma XAdES-EPES de una factura Facturae 3.2.x con la política de firma de Facturae v3.1
//! (la que exigen FACe y FACeB2B): firma envuelta en el elemento raíz, RSA-SHA512, digests
//! SHA-512 y C14N inclusiva 1.0.
//!
//! Adaptado de `FacturaeSigner.php` y `XmlTools.php` de Facturae-PHP:
//! Copyright (c) 2018-present José M. Moreno <josemmo@pm.me> (https://github.com/josemmo),
//! licencia MIT (ver `THIRD_PARTY_NOTICES.md`).
//!
//! Como TaxPain genera la Facturae (sin atributos salvo los espacios de nombres de la raíz, sin
//! elementos vacíos abreviados ni comentarios), su forma canónica se obtiene ordenando esos
//! espacios de nombres y dejando las comillas sin escapar en el texto. Las partes de la firma se
//! escriben ya en forma canónica: los digests se calculan sobre ellas con los espacios de nombres
//! heredados (`ds`, `fe`, `xades`) declarados en el elemento, como manda C14N.
//!
//! La clave privada no pasa por aquí: se recibe una función que firma (la del almacén, que pide
//! Touch ID antes de desellar el certificado).

use std::fmt::Write as _;

use ::time::OffsetDateTime;
use sha2::{Digest, Sha512};
use taxpain_core::base64;
use x509_parser::prelude::{FromDer, X509Certificate};

const XMLNS_DS: &str = "http://www.w3.org/2000/09/xmldsig#";
const XMLNS_XADES: &str = "http://uri.etsi.org/01903/v1.3.2#";
const SIGN_POLICY_NAME: &str = "Política de Firma FacturaE v3.1";
const SIGN_POLICY_URL: &str = "http://www.facturae.es/politica_de_firma_formato_facturae/politica_de_firma_formato_facturae_v3_1.pdf";
/// SHA-1 del PDF de la política, en Base64 (el valor que publica Facturae).
const SIGN_POLICY_DIGEST: &str = "Ohixl6upD6av8N7pEvDABhEL6hM=";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum XadesError {
    #[error("el XML no es una Facturae (falta <fe:Facturae>)")]
    NotFacturae,
    #[error("la Facturae ya está firmada")]
    AlreadySigned,
    #[error("certificado no válido: {0}")]
    Certificate(String),
    #[error("la firma necesita un certificado con clave RSA")]
    NotRsa,
    #[error("firma: {0}")]
    Sign(String),
}

/// Identificadores de los elementos de la firma (aleatorios en cada firma).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureIds {
    pub signature: String,
    pub signed_info: String,
    pub signed_properties_ref: String,
    pub signature_value: String,
    pub certificate: String,
    pub reference: String,
    pub signed_properties: String,
    pub object: String,
}

impl SignatureIds {
    /// A partir de ocho números (en la app, aleatorios).
    pub fn from_numbers(n: [u32; 8]) -> Self {
        let signature = format!("Signature{}", n[0]);
        SignatureIds {
            signed_info: format!("Signature-SignedInfo{}", n[1]),
            signed_properties_ref: format!("SignedPropertiesID{}", n[2]),
            signature_value: format!("SignatureValue{}", n[3]),
            certificate: format!("Certificate{}", n[4]),
            reference: format!("Reference-ID-{}", n[5]),
            signed_properties: format!("{signature}-SignedProperties{}", n[6]),
            object: format!("{signature}-Object{}", n[7]),
            signature,
        }
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn sha512_b64(data: &[u8]) -> String {
    base64::encode(&Sha512::digest(data))
}

/// Nombre distinguido del emisor como lo escribe Facturae-PHP: del último RDN al primero,
/// separados por «, », con los tipos conocidos (y `OID.2.5.4.x` para los que dan problemas).
fn issuer_name(cert: &X509Certificate<'_>) -> String {
    let mut parts = Vec::new();
    for rdn in cert.issuer().iter() {
        for atv in rdn.iter() {
            let oid = atv.attr_type().to_id_string();
            let ty = match oid.as_str() {
                "2.5.4.3" => "CN",
                "2.5.4.7" => "L",
                "2.5.4.8" => "ST",
                "2.5.4.10" => "O",
                "2.5.4.11" => "OU",
                "2.5.4.6" => "C",
                "2.5.4.9" => "STREET",
                "0.9.2342.19200300.100.1.25" => "DC",
                "0.9.2342.19200300.100.1.1" => "UID",
                "2.5.4.42" => "GN",
                "2.5.4.4" => "SN",
                "2.5.4.97" => "OID.2.5.4.97",
                "2.5.4.5" => "OID.2.5.4.5",
                "2.5.4.12" => "OID.2.5.4.12",
                _ => continue,
            };
            if let Ok(v) = atv.as_str() {
                parts.push(format!("{ty}={v}"));
            }
        }
    }
    parts.reverse();
    parts.join(", ")
}

fn strip_zeros(b: &[u8]) -> &[u8] {
    let i = b
        .iter()
        .position(|x| *x != 0)
        .unwrap_or(b.len().saturating_sub(1));
    &b[i..]
}

/// Elemento con los espacios de nombres heredados declarados (C14N de un subárbol): `xmlns`
/// ordenados por prefijo y después los atributos que ya tenía.
fn with_namespaces(element: &str, ns: &[(&str, &str)]) -> String {
    let end = element.find('>').unwrap_or(element.len());
    let (head, rest) = element.split_at(end);
    let (name, attrs) = head.split_once(' ').unwrap_or((head, ""));
    let mut decl: Vec<String> = ns
        .iter()
        .map(|(p, u)| format!("xmlns:{p}=\"{u}\""))
        .collect();
    decl.sort();
    let mut out = format!("{name} {}", decl.join(" "));
    if !attrs.is_empty() {
        out.push(' ');
        out.push_str(attrs);
    }
    out.push_str(rest);
    out
}

/// Firma la Facturae `xml` (tal como la genera TaxPain). `chain` son los certificados en DER
/// (el del firmante primero) y `sign` firma con RSA-SHA512 los bytes que recibe.
pub fn sign_facturae(
    xml: &str,
    chain: &[Vec<u8>],
    time: OffsetDateTime,
    ids: &SignatureIds,
    sign: impl FnOnce(&[u8]) -> Result<Vec<u8>, String>,
) -> Result<String, XadesError> {
    if xml.contains("<ds:Signature") {
        return Err(XadesError::AlreadySigned);
    }
    let xml = xml.replace("\r\n", "\n").replace('\r', "\n");
    let open = xml.find("<fe:Facturae ").ok_or(XadesError::NotFacturae)?;
    let close = xml.find("</fe:Facturae>").ok_or(XadesError::NotFacturae)? + "</fe:Facturae>".len();
    // Raíz en forma canónica: espacios de nombres ordenados y comillas sin escapar en el texto
    // (la Facturae de TaxPain no tiene más atributos que esos).
    let root = &xml[open..close];
    let start_end = root.find('>').ok_or(XadesError::NotFacturae)?;
    let mut fe_ns = None;
    for part in root[..start_end].split_whitespace() {
        if let Some(v) = part.strip_prefix("xmlns:fe=\"") {
            fe_ns = Some(v.trim_end_matches('"').to_owned());
        }
    }
    let fe_ns = fe_ns.ok_or(XadesError::NotFacturae)?;
    let body = root[start_end + 1..]
        .replace("&quot;", "\"")
        .replace("&apos;", "'");
    let root = format!(
        "{}{body}",
        with_namespaces("<fe:Facturae>", &[("ds", XMLNS_DS), ("fe", &fe_ns)])
    );
    let ns: [(&str, &str); 3] = [("ds", XMLNS_DS), ("fe", &fe_ns), ("xades", XMLNS_XADES)];

    let leaf = chain
        .first()
        .ok_or_else(|| XadesError::Certificate("sin certificado".into()))?;
    let (_, cert) =
        X509Certificate::from_der(leaf).map_err(|e| XadesError::Certificate(e.to_string()))?;
    let (modulus, exponent) = match cert.public_key().parsed() {
        Ok(x509_parser::public_key::PublicKey::RSA(k)) => (
            strip_zeros(k.modulus).to_vec(),
            strip_zeros(k.exponent).to_vec(),
        ),
        _ => return Err(XadesError::NotRsa),
    };

    let signing_time = time
        .format(&::time::format_description::well_known::Rfc3339)
        .map_err(|e| XadesError::Sign(e.to_string()))?;
    let signed_properties = format!(
        "<xades:SignedProperties Id=\"{sp}\">\
         <xades:SignedSignatureProperties>\
         <xades:SigningTime>{signing_time}</xades:SigningTime>\
         <xades:SigningCertificate><xades:Cert><xades:CertDigest>\
         <ds:DigestMethod Algorithm=\"http://www.w3.org/2001/04/xmlenc#sha512\"></ds:DigestMethod>\
         <ds:DigestValue>{cert_digest}</ds:DigestValue>\
         </xades:CertDigest><xades:IssuerSerial>\
         <ds:X509IssuerName>{issuer}</ds:X509IssuerName>\
         <ds:X509SerialNumber>{serial}</ds:X509SerialNumber>\
         </xades:IssuerSerial></xades:Cert></xades:SigningCertificate>\
         <xades:SignaturePolicyIdentifier><xades:SignaturePolicyId><xades:SigPolicyId>\
         <xades:Identifier>{SIGN_POLICY_URL}</xades:Identifier>\
         <xades:Description>{SIGN_POLICY_NAME}</xades:Description>\
         </xades:SigPolicyId><xades:SigPolicyHash>\
         <ds:DigestMethod Algorithm=\"http://www.w3.org/2000/09/xmldsig#sha1\"></ds:DigestMethod>\
         <ds:DigestValue>{SIGN_POLICY_DIGEST}</ds:DigestValue>\
         </xades:SigPolicyHash></xades:SignaturePolicyId></xades:SignaturePolicyIdentifier>\
         <xades:SignerRole><xades:ClaimedRoles><xades:ClaimedRole>emisor</xades:ClaimedRole>\
         </xades:ClaimedRoles></xades:SignerRole>\
         </xades:SignedSignatureProperties>\
         <xades:SignedDataObjectProperties>\
         <xades:DataObjectFormat ObjectReference=\"#{reference}\">\
         <xades:Description>Factura electrónica</xades:Description>\
         <xades:ObjectIdentifier>\
         <xades:Identifier Qualifier=\"OIDAsURN\">urn:oid:1.2.840.10003.5.109.10</xades:Identifier>\
         </xades:ObjectIdentifier>\
         <xades:MimeType>text/xml</xades:MimeType>\
         </xades:DataObjectFormat></xades:SignedDataObjectProperties>\
         </xades:SignedProperties>",
        sp = ids.signed_properties,
        cert_digest = sha512_b64(leaf),
        issuer = esc(&issuer_name(&cert)),
        serial = cert.serial,
        reference = ids.reference,
    );

    let mut key_info = format!("<ds:KeyInfo Id=\"{}\">\n<ds:X509Data>\n", ids.certificate);
    for c in chain {
        let _ = write!(
            key_info,
            "<ds:X509Certificate>\n{}</ds:X509Certificate>\n",
            base64::encode_wrapped(c, 76)
        );
    }
    let _ = write!(
        key_info,
        "</ds:X509Data>\n<ds:KeyValue>\n<ds:RSAKeyValue>\n<ds:Modulus>\n{}</ds:Modulus>\n\
         <ds:Exponent>{}</ds:Exponent>\n</ds:RSAKeyValue>\n</ds:KeyValue>\n</ds:KeyInfo>",
        base64::encode_wrapped(&modulus, 76),
        base64::encode(&exponent),
    );

    let signed_info = format!(
        "<ds:SignedInfo Id=\"{si}\">\n\
         <ds:CanonicalizationMethod Algorithm=\"http://www.w3.org/TR/2001/REC-xml-c14n-20010315\"></ds:CanonicalizationMethod>\n\
         <ds:SignatureMethod Algorithm=\"http://www.w3.org/2001/04/xmldsig-more#rsa-sha512\"></ds:SignatureMethod>\n\
         <ds:Reference Id=\"{spr}\" Type=\"http://uri.etsi.org/01903#SignedProperties\" URI=\"#{sp}\">\n\
         <ds:DigestMethod Algorithm=\"http://www.w3.org/2001/04/xmlenc#sha512\"></ds:DigestMethod>\n\
         <ds:DigestValue>{d_sp}</ds:DigestValue>\n\
         </ds:Reference>\n\
         <ds:Reference URI=\"#{cert}\">\n\
         <ds:DigestMethod Algorithm=\"http://www.w3.org/2001/04/xmlenc#sha512\"></ds:DigestMethod>\n\
         <ds:DigestValue>{d_ki}</ds:DigestValue>\n\
         </ds:Reference>\n\
         <ds:Reference Id=\"{reference}\" Type=\"http://www.w3.org/2000/09/xmldsig#Object\" URI=\"\">\n\
         <ds:Transforms>\n\
         <ds:Transform Algorithm=\"http://www.w3.org/2000/09/xmldsig#enveloped-signature\"></ds:Transform>\n\
         </ds:Transforms>\n\
         <ds:DigestMethod Algorithm=\"http://www.w3.org/2001/04/xmlenc#sha512\"></ds:DigestMethod>\n\
         <ds:DigestValue>{d_doc}</ds:DigestValue>\n\
         </ds:Reference>\n\
         </ds:SignedInfo>",
        si = ids.signed_info,
        spr = ids.signed_properties_ref,
        sp = ids.signed_properties,
        cert = ids.certificate,
        reference = ids.reference,
        d_sp = sha512_b64(with_namespaces(&signed_properties, &ns).as_bytes()),
        d_ki = sha512_b64(with_namespaces(&key_info, &ns).as_bytes()),
        d_doc = sha512_b64(root.as_bytes()),
    );

    let signature =
        sign(with_namespaces(&signed_info, &ns).as_bytes()).map_err(XadesError::Sign)?;
    let ds_signature = format!(
        "<ds:Signature xmlns:xades=\"{XMLNS_XADES}\" Id=\"{sig}\">\n{signed_info}\n\
         <ds:SignatureValue Id=\"{sv}\">\n{value}</ds:SignatureValue>\n{key_info}\n\
         <ds:Object Id=\"{obj}\"><xades:QualifyingProperties Target=\"#{sig}\">{signed_properties}\
         </xades:QualifyingProperties></ds:Object></ds:Signature>",
        sig = ids.signature,
        sv = ids.signature_value,
        value = base64::encode_wrapped(&signature, 76),
        obj = ids.object,
    );
    let root = root.replacen(
        "</fe:Facturae>",
        &format!("{ds_signature}</fe:Facturae>"),
        1,
    );
    Ok(format!("{}{root}{}", &xml[..open], &xml[close..]))
}
