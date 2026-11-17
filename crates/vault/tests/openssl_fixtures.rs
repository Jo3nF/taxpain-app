//! `.p12` RSA generados con OpenSSL (`fixtures/certs/generate.sh`): AES moderno, 3DES y RC2-40.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_testkit::{OffsetDateTime, TEST_CIF, TEST_DNI, TEST_PASSWORD};
use taxpain_vault::{AlwaysConfirm, CertificateKind, IssuerKind, MemoryKeyProvider, Vault};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!(
        "{}/../../fixtures/certs/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn vault() -> Vault {
    Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap()
}

#[test]
fn rsa_persona_fisica_aes_y_rc2() {
    for name in ["rsa-persona-fisica-aes.p12", "rsa-persona-fisica-rc2.p12"] {
        let info = vault()
            .import_certificate(&fixture(name), TEST_PASSWORD, OffsetDateTime::now_utc())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(info.kind, CertificateKind::PersonaFisica, "{name}");
        assert_eq!(info.issuer_kind, IssuerKind::Fnmt);
        assert_eq!(info.holder_nif, TEST_DNI);
        assert_eq!(
            info.holder_name.as_deref(),
            Some("CARMEN ESPAÑOLA ESPAÑOLA")
        );
        assert_eq!(info.key_algorithm, "RSA 2048");
    }
}

#[test]
fn rsa_representante_3des() {
    let info = vault()
        .import_certificate(
            &fixture("rsa-representante-3des.p12"),
            TEST_PASSWORD,
            OffsetDateTime::now_utc(),
        )
        .unwrap();
    assert_eq!(info.kind, CertificateKind::RepresentantePj);
    assert_eq!(info.entity_nif.as_deref(), Some(TEST_CIF));
    assert_eq!(info.entity_name.as_deref(), Some("EMPRESA DE PRUEBA SL"));
}
