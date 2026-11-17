//! Prevalidación ServValiDos de extremo a extremo contra un servicio simulado (mTLS con CA
//! propia): comprueba el JSON que se envía y cómo se interpreta la respuesta.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use taxpain_aeat::declarations::{Validation, prevalidate};
use taxpain_aeat::{AeatClient, AeatError};
use taxpain_testkit::{OffsetDateTime, P12Format, Subject, TEST_PASSWORD, TestCert, TestPki};
use taxpain_vault::rustls::RootCertStore;
use taxpain_vault::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::{self, ServerConfig, server::WebPkiClientVerifier};

/// Mock: guarda el cuerpo y responde con errores si `F01` no empieza por `<T303`.
async fn spawn_mock(
    server: TestCert,
    client_ca: &[u8],
    seen: Arc<Mutex<Vec<String>>>,
) -> SocketAddr {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut roots = RootCertStore::empty();
    roots.add(CertificateDer::from(client_ca.to_vec())).unwrap();
    let verifier = WebPkiClientVerifier::builder_with_provider(Arc::new(roots), provider.clone())
        .allow_unauthenticated()
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
    tokio::spawn(async move {
        loop {
            let (tcp, _) = listener.accept().await.unwrap();
            let acceptor = acceptor.clone();
            let seen = seen.clone();
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
                let v: serde_json::Value = serde_json::from_str(&body).unwrap();
                let resp = if v["F01"].as_str().unwrap_or("").starts_with("<T303") {
                    r#"{"RESPUESTA":{"AVISOS":["Compruebe la casilla 28"],"PDF":"JVBERi0xLjQ="}}"#
                } else {
                    r#"{"respuesta":{"errores":["Fichero no valido"]}}"#
                };
                let out = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json;charset=UTF-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{resp}",
                    resp.len()
                );
                tls.write_all(out.as_bytes()).await.unwrap();
                tls.shutdown().await.ok();
            });
        }
    });
    addr
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn prevalidacion_contra_el_mock() {
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
            "Prevalidar (pruebas)",
            OffsetDateTime::now_utc(),
        )
        .unwrap();
    let mut roots = RootCertStore::empty();
    roots.add(CertificateDer::from(pki.ca_der.clone())).unwrap();
    let client = AeatClient::new(&identity, roots, &["localhost"]).unwrap();
    let url = format!(
        "https://localhost:{}/wlpl/PFTW-PICW/ServValiDos",
        addr.port()
    );

    let ok = prevalidate(
        &client,
        &url,
        "303",
        2026,
        "3T",
        "<T303020263T0000>\r\nresto",
    )
    .await
    .unwrap();
    assert_eq!(
        ok,
        Validation::Valid {
            warnings: vec!["Compruebe la casilla 28".into()],
            pdf: Some(b"%PDF-1.4".to_vec()),
        }
    );
    let sent: serde_json::Value = serde_json::from_str(&seen.lock().unwrap()[0]).unwrap();
    assert_eq!(sent["MODELO"], "303");
    assert_eq!(sent["PERIODO"], "3T");
    assert_eq!(sent["F01"], "<T303020263T0000>resto");

    let ko = prevalidate(&client, &url, "130", 2026, "3T", "x")
        .await
        .unwrap();
    assert_eq!(
        ko,
        Validation::Invalid {
            errors: vec!["Fichero no valido".into()]
        }
    );

    // Un host fuera de la lista no recibe nada.
    assert!(
        prevalidate(
            &client,
            "https://prewww2.aeat.es/wlpl/PFTW-PICW/ServValiDos",
            "303",
            2026,
            "3T",
            "x"
        )
        .await
        .is_err()
    );

    // Sin nadie escuchando (como sin red o sin DNS) la conexión no se abre: `Unreachable`, que
    // distingue «no salió nada» de un corte a medias (`Network`).
    let closed = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let url = format!("https://localhost:{port}/wlpl/PFTW-PICW/ServValiDos");
    match prevalidate(&client, &url, "303", 2026, "3T", "x").await {
        Err(AeatError::Unreachable(_)) => {}
        other => panic!("esperaba Unreachable, no {other:?}"),
    }
}
