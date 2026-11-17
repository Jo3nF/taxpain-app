//! Envío VERI*FACTU de extremo a extremo contra un servicio AEAT simulado (mTLS con CA propia):
//! el mock valida el cuerpo SOAP recibido contra `SuministroLR.xsd` y responde como la AEAT.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use taxpain_aeat::verifactu::{Destino, enviar};
use taxpain_aeat::{AeatClient, AeatError};
use taxpain_core::invoice::{
    Invoice, InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment, compute_totals,
};
use taxpain_core::money::{Money, Rate};
use taxpain_invoicing::verifactu::{self, AltaOptions, SistemaInformatico};
use taxpain_testkit::{OffsetDateTime, P12Format, Subject, TEST_PASSWORD, TestCert, TestPki, xsd};
use taxpain_vault::rustls::RootCertStore;
use taxpain_vault::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::{self, ServerConfig, server::WebPkiClientVerifier};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/aeat/verifactu")
        .join(name)
}

const RESPONSE_OK: &str = "<env:Envelope xmlns:env=\"http://schemas.xmlsoap.org/soap/envelope/\"><env:Body>\
<tikR:RespuestaRegFactuSistemaFacturacion xmlns:tikR=\"https://www2.agenciatributaria.gob.es/static_files/common/internet/dep/aplicaciones/es/aeat/tike/cont/ws/RespuestaSuministro.xsd\" xmlns:tik=\"https://www2.agenciatributaria.gob.es/static_files/common/internet/dep/aplicaciones/es/aeat/tike/cont/ws/SuministroInformacion.xsd\">\
<tikR:Cabecera><tik:ObligadoEmision><tik:NombreRazon>X</tik:NombreRazon><tik:NIF>12345678Z</tik:NIF></tik:ObligadoEmision></tikR:Cabecera>\
<tikR:TiempoEsperaEnvio>60</tikR:TiempoEsperaEnvio><tikR:EstadoEnvio>Correcto</tikR:EstadoEnvio>\
<tikR:RespuestaLinea><tikR:IDFactura><tik:IDEmisorFactura>12345678Z</tik:IDEmisorFactura><tik:NumSerieFactura>A2026-0001</tik:NumSerieFactura><tik:FechaExpedicionFactura>13-10-2026</tik:FechaExpedicionFactura></tikR:IDFactura>\
<tikR:Operacion><tik:TipoOperacion>Alta</tik:TipoOperacion></tikR:Operacion><tikR:EstadoRegistro>Correcto</tikR:EstadoRegistro></tikR:RespuestaLinea>\
</tikR:RespuestaRegFactuSistemaFacturacion></env:Body></env:Envelope>";

const RESPONSE_FAULT: &str = "<env:Envelope xmlns:env=\"http://schemas.xmlsoap.org/soap/envelope/\"><env:Body><env:Fault>\
<faultcode>env:Client</faultcode><faultstring>Codigo[4102].El XML no cumple el esquema</faultstring></env:Fault></env:Body></env:Envelope>";

/// Mock: exige certificado de cliente, valida el cuerpo y guarda lo recibido.
async fn spawn_mock(
    server: TestCert,
    client_ca: &[u8],
    seen: Arc<Mutex<Vec<String>>>,
) -> SocketAddr {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut roots = RootCertStore::empty();
    roots.add(CertificateDer::from(client_ca.to_vec())).unwrap();
    let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone())
        .build()
        .unwrap();
    let config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_client_cert_verifier(verifier)
        .with_single_cert(
            vec![CertificateDer::from(server.cert_der.clone())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(server.key_pkcs8.clone())),
        )
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let schema_dir = xsd::local_schema_dir(
        &fixture(""),
        &[(
            "http://www.w3.org/TR/xmldsig-core/xmldsig-core-schema.xsd",
            "xmldsig-core-schema.xsd",
        )],
    );
    let schema_dir = Arc::new(schema_dir);
    tokio::spawn(async move {
        loop {
            let (tcp, _) = listener.accept().await.unwrap();
            let acceptor = acceptor.clone();
            let seen = seen.clone();
            let schema_dir = schema_dir.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let mut req = Vec::new();
                let mut buf = [0u8; 8192];
                let (head_end, len) = loop {
                    let n = tls.read(&mut buf).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    req.extend_from_slice(&buf[..n]);
                    if let Some(i) = req.windows(4).position(|w| w == b"\r\n\r\n") {
                        let head = String::from_utf8_lossy(&req[..i]).to_lowercase();
                        let len = head
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .map(|v| v.trim().parse::<usize>().unwrap())
                            .unwrap_or(0);
                        break (i + 4, len);
                    }
                };
                while req.len() < head_end + len {
                    let n = tls.read(&mut buf).await.unwrap();
                    req.extend_from_slice(&buf[..n]);
                }
                let body = String::from_utf8(req[head_end..head_end + len].to_vec()).unwrap();
                seen.lock().unwrap().push(body.clone());
                // Extrae el contenido de soapenv:Body y lo valida contra el esquema oficial.
                let start = body.find("<soapenv:Body>").unwrap() + "<soapenv:Body>".len();
                let end = body.find("</soapenv:Body>").unwrap();
                let inner = format!("<?xml version=\"1.0\"?>{}", &body[start..end]);
                let valid =
                    xsd::validate(&schema_dir.path().join("SuministroLR.xsd"), &inner).is_ok();
                let (status, resp) = if valid {
                    ("200 OK", RESPONSE_OK)
                } else {
                    ("500 Internal Server Error", RESPONSE_FAULT)
                };
                let out = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: text/xml; charset=UTF-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{resp}",
                    resp.len()
                );
                tls.write_all(out.as_bytes()).await.unwrap();
                tls.shutdown().await.ok();
            });
        }
    });
    addr
}

fn invoice() -> Invoice {
    Invoice {
        kind: InvoiceKind::Completa,
        number: "A2026-0001".into(),
        issue_date: date!(2026 - 10 - 13),
        operation_date: None,
        issuer: Party {
            id: PartyId::Nif("12345678Z".into()),
            name: "Carmen Española".into(),
            address: None,
            person: None,
        },
        recipient: Some(Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Cliente SL".into(),
            address: None,
            person: None,
        }),
        lines: vec![InvoiceLine {
            description: "Servicio".into(),
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
        description: "Servicios".into(),
        notes: None,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn envio_verifactu_con_certificado() {
    let pki = TestPki::fnmt();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let addr = spawn_mock(pki.server("localhost"), &pki.ca_der, seen.clone()).await;

    let vault = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let p12 = pki
        .valid_client(&Subject::persona_fisica())
        .to_p12(TEST_PASSWORD, P12Format::Modern);
    let info = vault
        .import_certificate(&p12, TEST_PASSWORD, OffsetDateTime::now_utc())
        .unwrap();
    let identity = vault
        .client_identity(
            &info.fingerprint_sha256,
            "Enviar registros VERI*FACTU (pruebas)",
            OffsetDateTime::now_utc(),
        )
        .unwrap();
    let mut roots = RootCertStore::empty();
    roots.add(CertificateDer::from(pki.ca_der.clone())).unwrap();
    let client = AeatClient::new(&identity, roots, &["localhost"]).unwrap();
    let destino = Destino::mock(&format!(
        "https://localhost:{}/wlpl/TIKE-CONT/ws/SistemaFacturacion/VerifactuSOAP",
        addr.port()
    ));

    let f = invoice();
    let t = compute_totals(&f.lines, RoundingMode::PerRate);
    let sif = SistemaInformatico::taxpain("instalacion-test");
    let rec = verifactu::alta(
        &f,
        &t,
        &sif,
        None,
        datetime!(2026-10-13 10:00 +02:00),
        &AltaOptions::default(),
    )
    .unwrap();
    let body = verifactu::submission("Carmen Española", "12345678Z", &[&rec]).unwrap();

    let resp = enviar(&client, &destino, &body).await.unwrap();
    assert_eq!(resp.estado_envio, "Correcto");
    assert_eq!(resp.lineas[0].estado, "Correcto");
    assert_eq!(resp.tiempo_espera_envio, 60);
    let received = seen.lock().unwrap()[0].clone();
    assert!(received.contains(&rec.huella));

    // Un cuerpo que no cumple el esquema → SOAP Fault que llega como error legible.
    let broken = body.replace(
        "<sf:TipoFactura>F1</sf:TipoFactura>",
        "<sf:TipoFactura>ZZ</sf:TipoFactura>",
    );
    match enviar(&client, &destino, &broken).await {
        Err(AeatError::InvalidResponse(m)) => assert!(m.contains("4102"), "{m}"),
        other => panic!("esperaba SOAP Fault, no {other:?}"),
    }
}

#[test]
fn produccion_bloqueada_en_desarrollo() {
    // Los tests se compilan con debug_assertions: producción nunca es alcanzable.
    assert!(matches!(
        Destino::produccion(true),
        Err(AeatError::ProductionBlocked(_))
    ));
    assert!(
        Destino::pruebas()
            .url()
            .starts_with("https://prewww1.aeat.es/")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn con_el_almacen_del_sistema_una_ca_desconocida_se_rechaza() {
    let pki = TestPki::fnmt();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let addr = spawn_mock(pki.server("localhost"), &pki.ca_der, seen.clone()).await;
    let vault = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let p12 = pki
        .valid_client(&Subject::persona_fisica())
        .to_p12(TEST_PASSWORD, P12Format::Modern);
    let info = vault
        .import_certificate(&p12, TEST_PASSWORD, OffsetDateTime::now_utc())
        .unwrap();
    let identity = vault
        .client_identity(&info.fingerprint_sha256, "x", OffsetDateTime::now_utc())
        .unwrap();
    let client = AeatClient::with_platform_trust(&identity, &["localhost"]).unwrap();
    let destino = Destino::mock(&format!("https://localhost:{}/x", addr.port()));
    // El TLS falla antes de enviar nada: la conexión no se abre.
    assert!(matches!(
        enviar(&client, &destino, "<x/>").await,
        Err(AeatError::Unreachable(_))
    ));
    assert!(
        seen.lock().unwrap().is_empty(),
        "no debe llegar ninguna petición"
    );
}
