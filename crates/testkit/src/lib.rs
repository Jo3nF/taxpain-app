//! PKI sintética para tests: CA propia, certificados de cliente con sujetos estilo FNMT
//! (persona física y representante de persona jurídica) empaquetados en `.p12`, y
//! certificados de servidor para `localhost`. Sólo NIF de prueba válidos por algoritmo.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    missing_debug_implementations
)]

pub mod xsd;

use p12_keystore::{
    Certificate, EncryptionAlgorithm, KeyStore, KeyStoreEntry, MacAlgorithm, PrivateKey,
    PrivateKeyChain,
};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose,
};
pub use time::{Duration, OffsetDateTime};

/// NIF de prueba válidos por algoritmo (no corresponden a personas reales conocidas).
pub const TEST_DNI: &str = "12345678Z";
pub const TEST_NIE: &str = "X1234567L";
pub const TEST_CIF: &str = "B12345674";
pub const TEST_PASSWORD: &str = "Prueba-1234";

/// Sujeto del certificado de cliente.
#[derive(Clone, Debug)]
pub enum Subject {
    /// FNMT persona física: CN "APELLIDOS NOMBRE - NIF", SERIALNUMBER "IDCES-NIF", GN, SN.
    PersonaFisica {
        nif: String,
        given_name: String,
        surname: String,
    },
    /// FNMT representante: CN "NIF NOMBRE (R: CIF)", organizationIdentifier "VATES-CIF", O.
    Representante {
        nif: String,
        name: String,
        entity_nif: String,
        entity_name: String,
    },
    /// Sin NIF reconocible.
    SinNif { common_name: String },
}

impl Subject {
    pub fn persona_fisica() -> Self {
        Subject::PersonaFisica {
            nif: TEST_DNI.into(),
            given_name: "CARMEN".into(),
            surname: "ESPAÑOLA ESPAÑOLA".into(),
        }
    }

    pub fn representante() -> Self {
        Subject::Representante {
            nif: TEST_DNI.into(),
            name: "CARMEN ESPAÑOLA".into(),
            entity_nif: TEST_CIF.into(),
            entity_name: "EMPRESA DE PRUEBA SL".into(),
        }
    }

    fn distinguished_name(&self) -> DistinguishedName {
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CountryName, "ES");
        match self {
            Subject::PersonaFisica {
                nif,
                given_name,
                surname,
            } => {
                dn.push(
                    DnType::CustomDnType(vec![2, 5, 4, 5]),
                    format!("IDCES-{nif}"),
                );
                dn.push(DnType::CustomDnType(vec![2, 5, 4, 42]), given_name.as_str());
                dn.push(DnType::CustomDnType(vec![2, 5, 4, 4]), surname.as_str());
                dn.push(
                    DnType::CommonName,
                    format!("{surname} {given_name} - {nif}"),
                );
            }
            Subject::Representante {
                nif,
                name,
                entity_nif,
                entity_name,
            } => {
                dn.push(DnType::OrganizationName, entity_name.as_str());
                dn.push(
                    DnType::CustomDnType(vec![2, 5, 4, 97]),
                    format!("VATES-{entity_nif}"),
                );
                dn.push(
                    DnType::CustomDnType(vec![2, 5, 4, 5]),
                    format!("IDCES-{nif}"),
                );
                dn.push(
                    DnType::CommonName,
                    format!("{nif} {name} (R: {entity_nif})"),
                );
            }
            Subject::SinNif { common_name } => dn.push(DnType::CommonName, common_name.as_str()),
        }
        dn
    }
}

/// Certificado + clave PKCS#8 (DER) + cadena hasta la CA.
#[derive(Clone)]
pub struct TestCert {
    pub cert_der: Vec<u8>,
    pub key_pkcs8: Vec<u8>,
    pub ca_der: Vec<u8>,
}

/// Algoritmo de cifrado del `.p12`.
#[derive(Clone, Copy, Debug)]
pub enum P12Format {
    /// PBES2 + AES-256 + HMAC-SHA256 (exportaciones modernas).
    Modern,
    /// PBE-SHA1-3DES + HMAC-SHA1 (exportaciones antiguas de navegadores/Windows).
    Legacy3Des,
}

impl TestCert {
    pub fn to_p12(&self, password: &str, format: P12Format) -> Vec<u8> {
        self.to_p12_with_key(&self.key_pkcs8, password, format)
    }

    /// `.p12` cuya clave privada no corresponde al certificado.
    pub fn to_p12_with_key(&self, key_pkcs8: &[u8], password: &str, format: P12Format) -> Vec<u8> {
        let chain = PrivateKeyChain::new(
            b"taxpain-test".as_slice(),
            PrivateKey::from_der(key_pkcs8).unwrap(),
            [
                Certificate::from_der(&self.cert_der).unwrap(),
                Certificate::from_der(&self.ca_der).unwrap(),
            ],
        );
        let mut ks = KeyStore::new();
        ks.add_entry("taxpain-test", KeyStoreEntry::PrivateKeyChain(chain));
        let w = ks.writer(password);
        let w = match format {
            P12Format::Modern => w,
            P12Format::Legacy3Des => w
                .encryption_algorithm(EncryptionAlgorithm::PbeWithShaAnd3KeyTripleDesCbc)
                .mac_algorithm(MacAlgorithm::HmacSha1),
        };
        w.write().unwrap()
    }
}

/// CA de pruebas.
pub struct TestPki {
    issuer: Issuer<'static, KeyPair>,
    pub ca_der: Vec<u8>,
}

impl TestPki {
    /// CA con `O` = `organization` (usa "FNMT-RCM PRUEBAS" para simular la FNMT).
    pub fn new(organization: &str, common_name: &str) -> Self {
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CountryName, "ES");
        dn.push(DnType::OrganizationName, organization);
        dn.push(DnType::CommonName, common_name);
        params.distinguished_name = dn;
        params.not_before = OffsetDateTime::now_utc() - Duration::days(365);
        params.not_after = OffsetDateTime::now_utc() + Duration::days(3650);
        let key = KeyPair::generate().unwrap();
        let ca = params.self_signed(&key).unwrap();
        TestPki {
            issuer: Issuer::new(params, key),
            ca_der: ca.der().to_vec(),
        }
    }

    pub fn fnmt() -> Self {
        TestPki::new("FNMT-RCM PRUEBAS", "AC FNMT Usuarios PRUEBAS")
    }

    /// Certificado de cliente válido entre `not_before` y `not_after`.
    pub fn client(
        &self,
        subject: &Subject,
        not_before: OffsetDateTime,
        not_after: OffsetDateTime,
    ) -> TestCert {
        let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
        params.distinguished_name = subject.distinguished_name();
        params.not_before = not_before;
        params.not_after = not_after;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![
            ExtendedKeyUsagePurpose::ClientAuth,
            ExtendedKeyUsagePurpose::EmailProtection,
        ];
        self.sign(params)
    }

    /// Certificado de cliente válido desde hace 30 días hasta dentro de 2 años.
    pub fn valid_client(&self, subject: &Subject) -> TestCert {
        let now = OffsetDateTime::now_utc();
        self.client(subject, now - Duration::days(30), now + Duration::days(730))
    }

    /// Certificado de servidor TLS para `host` (p. ej. "localhost").
    pub fn server(&self, host: &str) -> TestCert {
        let mut params = CertificateParams::new(vec![host.to_owned()]).unwrap();
        params.distinguished_name.push(DnType::CommonName, host);
        params.not_before = OffsetDateTime::now_utc() - Duration::days(1);
        params.not_after = OffsetDateTime::now_utc() + Duration::days(30);
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        self.sign(params)
    }

    fn sign(&self, params: CertificateParams) -> TestCert {
        let key = KeyPair::generate().unwrap();
        let cert = params.signed_by(&key, &self.issuer).unwrap();
        TestCert {
            cert_der: cert.der().to_vec(),
            key_pkcs8: key.serialize_der(),
            ca_der: self.ca_der.clone(),
        }
    }
}

/// Clave PKCS#8 nueva sin certificado (para probar claves que no corresponden).
pub fn random_key_pkcs8() -> Vec<u8> {
    KeyPair::generate().unwrap().serialize_der()
}
