//! Firma XAdES-EPES de la Facturae con un certificado RSA de prueba (`fixtures/certs`): la firma
//! RSA-SHA512 del SignedInfo y los tres digests se comprueban por separado.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use sha2::{Digest, Sha512};
use taxpain_app::invoicing::{self, InvoiceDraft, issue, signed_facturae};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::invoice::{InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment};
use taxpain_core::money::{Money, Rate};
use taxpain_testkit::{OffsetDateTime, TEST_PASSWORD};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use x509_parser::prelude::{FromDer, X509Certificate};

fn b64(s: &str) -> Vec<u8> {
    let t = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in s.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        acc = (acc << 6) | u32::try_from(t.iter().position(|x| *x == c).unwrap()).unwrap();
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xFF).unwrap());
        }
    }
    out
}

fn between<'a>(s: &'a str, a: &str, b: &str) -> &'a str {
    let i = s.find(a).unwrap();
    let j = s[i..].find(b).unwrap() + i + b.len();
    &s[i..j]
}

fn inner<'a>(s: &'a str, a: &str, b: &str) -> &'a str {
    let i = s.find(a).unwrap() + a.len();
    let j = s[i..].find(b).unwrap() + i;
    &s[i..j]
}

const NS: &str = "xmlns:ds=\"http://www.w3.org/2000/09/xmldsig#\" xmlns:fe=\"http://www.facturae.gob.es/formato/Versiones/Facturaev3_2_2.xml\" xmlns:xades=\"http://uri.etsi.org/01903/v1.3.2#\"";

/// C14N de un elemento de la firma: los espacios de nombres heredados en la etiqueta de apertura.
fn c14n(element: &str) -> String {
    let (name, rest) = element.split_once(' ').unwrap();
    format!("{name} {NS} {rest}")
}

#[test]
fn facturae_firmada_xades_epes() {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let p12 = std::fs::read(format!(
        "{}/../../fixtures/certs/rsa-persona-fisica-aes.p12",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let info = v
        .import_certificate(&p12, TEST_PASSWORD, OffsetDateTime::now_utc())
        .unwrap();
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("A22222228".into()),
            name: "Carmen \"La Peña\"".into(),
            address: Some(taxpain_core::invoice::Address {
                street: "Calle Mayor 1".into(),
                post_code: "46001".into(),
                town: "València".into(),
                province: "Valencia".into(),
                country: "ES".into(),
            }),
            person: None,
        },
        default_series: "A".into(),
        rectifying_series: "R".into(),
        simplified_series: "S".into(),
        simplified_rectifying_series: "RS".into(),
        language: Language::Es,
        default_vat: Rate::percent(21),
        default_withholding: None,
        outputs: Outputs {
            pdf: false,
            facturae: true,
            ubl: false,
        },
        destination: None,
        rounding: RoundingMode::PerRate,
        verifactu: VerifactuSettings {
            enabled: false,
            installation_id: "x".into(),
            production: false,
        },
        footer: None,
        activity: None,
        simplified_up_to_3000: false,
    };
    invoicing::save_settings(&v, &s, datetime!(2026-01-01 9:00 UTC)).unwrap();
    let inv = issue(
        &v,
        &InvoiceDraft {
            kind: InvoiceKind::Completa,
            series: None,
            issue_date: date!(2026 - 10 - 20),
            operation_date: None,
            recipient: Some(Party {
                id: PartyId::Nif("B12345674".into()),
                name: "Cliente & Hijos SL".into(),
                address: Some(taxpain_core::invoice::Address {
                    street: "Calle Mayor 1".into(),
                    post_code: "46001".into(),
                    town: "València".into(),
                    province: "Valencia".into(),
                    country: "ES".into(),
                }),
                person: None,
            }),
            lines: vec![InvoiceLine {
                description: "Diseño <web>".into(),
                quantity_milli: 1000,
                unit_price: Money::from_euros(100, 0),
                discount: Rate::ZERO,
                vat: VatTreatment::Taxed {
                    rate: Rate::percent(21),
                },
                surcharge: None,
                withholding: None,
                advance: false,
            }],
            rectified: vec![],
            rectified_amounts: None,
            description: "Servicios".into(),
            notes: None,
            currency: None,
            oss: None,
        },
        OffsetDateTime::now_utc(),
        true,
    )
    .unwrap();
    let id = v
        .client_identity(
            &info.fingerprint_sha256,
            "Firmar factura",
            OffsetDateTime::now_utc(),
        )
        .unwrap();
    let (name, bytes) = signed_facturae(&v, inv.id, &id, OffsetDateTime::now_utc()).unwrap();
    assert!(name.ends_with(".xsig"), "{name}");
    let x = String::from_utf8(bytes).unwrap();
    assert!(x.starts_with("<?xml"));
    assert_eq!(x.matches("<ds:Signature ").count(), 1);
    assert!(x.contains("<xades:Identifier>http://www.facturae.es/politica_de_firma_formato_facturae/politica_de_firma_formato_facturae_v3_1.pdf</xades:Identifier>"));
    // Comillas sin escapar en el texto (forma canónica); & y < siguen escapados.
    assert!(x.contains("Carmen \"La Peña\""));
    assert!(x.contains("Cliente &amp; Hijos SL"));

    // 1. Firma RSA-SHA512 del SignedInfo canónico con la clave pública del certificado.
    let signed_info = between(&x, "<ds:SignedInfo ", "</ds:SignedInfo>");
    let sig = b64(inner(&x, "<ds:SignatureValue", "</ds:SignatureValue>")
        .split_once('>')
        .unwrap()
        .1);
    let cert_der = b64(inner(&x, "<ds:X509Certificate>", "</ds:X509Certificate>"));
    let (_, cert) = X509Certificate::from_der(&cert_der).unwrap();
    let pk = ring::signature::UnparsedPublicKey::new(
        &ring::signature::RSA_PKCS1_2048_8192_SHA512,
        cert.public_key().subject_public_key.data.to_vec(),
    );
    pk.verify(c14n(signed_info).as_bytes(), &sig)
        .expect("la firma no verifica");

    // 2. Digests de SignedProperties y KeyInfo (con los espacios de nombres heredados).
    let digests: Vec<&str> = signed_info
        .split("<ds:DigestValue>")
        .skip(1)
        .map(|p| p.split('<').next().unwrap())
        .collect();
    assert_eq!(digests.len(), 3);
    let sp = between(&x, "<xades:SignedProperties ", "</xades:SignedProperties>");
    let ki = between(&x, "<ds:KeyInfo ", "</ds:KeyInfo>");
    let d = |s: &str| taxpain_core::base64::encode(&Sha512::digest(s.as_bytes()));
    assert_eq!(digests[0], d(&c14n(sp)));
    assert_eq!(digests[1], d(&c14n(ki)));

    // 3. Digest del documento sin la firma (transformación enveloped).
    let root = between(&x, "<fe:Facturae ", "</fe:Facturae>");
    let sig_el = between(root, "<ds:Signature ", "</ds:Signature>");
    let doc = root.replace(sig_el, "");
    assert!(
        doc.starts_with("<fe:Facturae xmlns:ds=\"http://www.w3.org/2000/09/xmldsig#\" xmlns:fe=")
    );
    assert_eq!(digests[2], d(&doc));

    // Dos veces no.
    assert!(
        taxpain_invoicing::xades::sign_facturae(
            &x,
            &id.chain_der(),
            OffsetDateTime::now_utc(),
            &taxpain_invoicing::xades::SignatureIds::from_numbers([1; 8]),
            |_| Ok(vec![])
        )
        .is_err()
    );
}
