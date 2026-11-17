//! mTLS de extremo a extremo contra un servidor local con CA propia, y la garantía de que el
//! material secreto (clave privada, contraseña del .p12, claves del almacén) nunca aparece en
//! los logs (nivel TRACE, incluidos rustls/reqwest), en los bytes de red ni en disco en claro.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Once};

use base64::Engine;
use sha2::{Digest, Sha256};
use taxpain_aeat::{AeatClient, AeatError};
use taxpain_testkit::{OffsetDateTime, P12Format, Subject, TEST_PASSWORD, TestCert, TestPki};
use taxpain_vault::rustls::RootCertStore;
use taxpain_vault::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use taxpain_vault::{AlwaysConfirm, FileKeyProvider, KeyId, KeyProvider, Vault};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::{self, ServerConfig, server::WebPkiClientVerifier};

// ---------- captura de logs ----------

static LOGS: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static INIT: Once = Once::new();

struct LogSink;
impl Write for LogSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        LOGS.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn capture_logs() {
    INIT.call_once(|| {
        // Con la feature `tracing-log`, `init` también redirige el crate `log` (rustls, hyper).
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_writer(|| LogSink)
            .with_ansi(false)
            .init();
    });
}

fn logs() -> Vec<u8> {
    LOGS.lock().unwrap().clone()
}

// ---------- servidor mTLS + relé que graba la red ----------

/// Servidor HTTPS que exige certificado de cliente emitido por `client_ca` y responde con la
/// huella SHA-256 del certificado presentado.
async fn spawn_server(server: TestCert, client_ca: &[u8]) -> SocketAddr {
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
    tokio::spawn(async move {
        loop {
            let (tcp, _) = listener.accept().await.unwrap();
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = acceptor.accept(tcp).await else {
                    return;
                };
                let peer = tls.get_ref().1.peer_certificates().map(|c| c[0].to_vec());
                let mut req = Vec::new();
                let mut buf = [0u8; 4096];
                while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = tls.read(&mut buf).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    req.extend_from_slice(&buf[..n]);
                }
                let body = format!(
                    "{{\"peer_fingerprint\":\"{}\"}}",
                    peer.map(|d| hex(&Sha256::digest(d))).unwrap_or_default()
                );
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                tls.write_all(resp.as_bytes()).await.unwrap();
                tls.shutdown().await.ok();
            });
        }
    });
    addr
}

/// Relé TCP que reenvía a `upstream`, graba todos los bytes en ambos sentidos y cuenta las
/// conexiones aceptadas.
async fn spawn_recording_relay(
    upstream: SocketAddr,
    wire: Arc<Mutex<Vec<u8>>>,
    connections: Arc<AtomicUsize>,
) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (client, _) = listener.accept().await.unwrap();
            connections.fetch_add(1, Ordering::SeqCst);
            let server = TcpStream::connect(upstream).await.unwrap();
            let (cr, cw) = client.into_split();
            let (sr, sw) = server.into_split();
            tokio::spawn(pump(cr, sw, wire.clone()));
            tokio::spawn(pump(sr, cw, wire.clone()));
        }
    });
    addr
}

async fn pump(
    mut from: tokio::net::tcp::OwnedReadHalf,
    mut to: tokio::net::tcp::OwnedWriteHalf,
    wire: Arc<Mutex<Vec<u8>>>,
) {
    let mut buf = [0u8; 8192];
    loop {
        match from.read(&mut buf).await {
            Ok(0) | Err(_) => {
                to.shutdown().await.ok();
                return;
            }
            Ok(n) => {
                wire.lock().unwrap().extend_from_slice(&buf[..n]);
                if to.write_all(&buf[..n]).await.is_err() {
                    return;
                }
            }
        }
    }
}

// ---------- utilidades ----------

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

/// Representaciones de un secreto que no deben aparecer en ninguna parte.
fn forms(secret: &[u8]) -> Vec<Vec<u8>> {
    let b64 = base64::engine::general_purpose::STANDARD.encode(secret);
    let mid = &secret[secret.len() / 2 - 16..secret.len() / 2 + 16];
    vec![
        secret.to_vec(),
        mid.to_vec(),
        hex(secret).into_bytes(),
        hex(mid).into_bytes(),
        hex(mid).to_uppercase().into_bytes(),
        b64.as_bytes()[..b64.len().min(40)].to_vec(),
        format!("{mid:?}").into_bytes(), // Debug de un &[u8]
    ]
}

fn assert_absent(label: &str, haystack: &[u8], secrets: &[(&str, Vec<Vec<u8>>)]) {
    for (name, variants) in secrets {
        for v in variants {
            assert!(!contains(haystack, v), "{name} aparece en {label}");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mtls_funciona_y_ningun_secreto_sale() {
    capture_logs();
    let pki = TestPki::fnmt();
    let client_cert = pki.valid_client(&Subject::persona_fisica());
    let p12 = client_cert.to_p12(TEST_PASSWORD, P12Format::Modern);
    let server_addr = spawn_server(pki.server("localhost"), &pki.ca_der).await;
    let wire = Arc::new(Mutex::new(Vec::new()));
    let connections = Arc::new(AtomicUsize::new(0));
    let relay = spawn_recording_relay(server_addr, wire.clone(), connections.clone()).await;

    // Almacén en disco, como en la app.
    let dir = tempfile::tempdir().unwrap();
    let keys = Arc::new(FileKeyProvider::new(dir.path().join("keys")).unwrap());
    let data_dir = dir.path().join("data");
    let vault = Vault::open(&data_dir, keys.clone(), Arc::new(AlwaysConfirm)).unwrap();
    let info = vault
        .import_certificate(&p12, TEST_PASSWORD, OffsetDateTime::now_utc())
        .unwrap();
    let identity = vault
        .client_identity(
            &info.fingerprint_sha256,
            "Prueba de conexión",
            OffsetDateTime::now_utc(),
        )
        .unwrap();

    let mut roots = RootCertStore::empty();
    roots.add(CertificateDer::from(pki.ca_der.clone())).unwrap();
    let client = AeatClient::new(&identity, roots, &["localhost"]).unwrap();

    // 1) mTLS: el servidor ve exactamente nuestro certificado.
    let res = client
        .post_json(
            &format!("https://localhost:{}/wlpl/prueba", relay.port()),
            &serde_json::json!({"MODELO": "303"}),
        )
        .await
        .unwrap();
    assert_eq!(res["peer_fingerprint"], info.fingerprint_sha256);

    // 2) Allow-list: otro host (aunque sea el mismo servidor) no recibe ni una conexión.
    assert_eq!(connections.load(Ordering::SeqCst), 1);
    for url in [
        format!("https://127.0.0.1:{}/x", relay.port()),
        format!("http://localhost:{}/x", relay.port()),
        format!("https://user:pw@localhost:{}/x", relay.port()),
        "https://www1.agenciatributaria.gob.es/x".to_owned(),
    ] {
        let err = client
            .post_json(&url, &serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(
            matches!(err, AeatError::HostNotAllowed(_) | AeatError::InvalidUrl),
            "{url}: {err}"
        );
    }
    assert_eq!(
        connections.load(Ordering::SeqCst),
        1,
        "no debe abrirse ninguna conexión"
    );
    // En desarrollo, ni con el host de producción permitido se llega a presentar ni a enviar
    // registros VERI*FACTU de producción; consultar, sí (no tiene efectos).
    let prod = AeatClient::new(
        &identity,
        RootCertStore::empty(),
        taxpain_aeat::hosts::PRODUCCION,
    )
    .unwrap();
    for url in [
        taxpain_aeat::declarations::PRES_PRODUCCION,
        taxpain_aeat::verifactu::URL_PRODUCCION,
    ] {
        assert_eq!(
            prod.check_url(url).is_err(),
            cfg!(debug_assertions),
            "{url}"
        );
    }
    assert!(
        prod.check_url(taxpain_aeat::declarations::CONSULTA_PRODUCCION)
            .is_ok()
    );
    // Con el certificado sólo se va al puerto de siempre: una URL de la AEAT con otro puerto
    // (aunque sea de un host permitido) no se pide; el 443 escrito es el de siempre.
    for url in [
        "https://www1.agenciatributaria.gob.es:8443/wlpl/x",
        "https://www1.agenciatributaria.gob.es:80/wlpl/x",
        "https://www1.agenciatributaria.gob.es:444/wlpl/x",
    ] {
        assert!(
            matches!(prod.check_url(url), Err(AeatError::InvalidUrl)),
            "{url}"
        );
    }
    assert!(
        prod.check_url("https://www1.agenciatributaria.gob.es:443/wlpl/x")
            .is_ok()
    );

    // 3) Sin certificado de cliente el servidor rechaza el handshake.
    // (Se comprueba con un cliente rustls sin identidad.)
    let anon = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates({
        let mut r = RootCertStore::empty();
        r.add(CertificateDer::from(pki.ca_der.clone())).unwrap();
        r
    })
    .with_no_client_auth();
    let tcp = TcpStream::connect(server_addr).await.unwrap();
    let connector = tokio_rustls::TlsConnector::from(Arc::new(anon));
    let outcome = async {
        let mut tls = connector
            .connect("localhost".try_into().unwrap(), tcp)
            .await?;
        tls.write_all(b"GET / HTTP/1.1\r\nhost: localhost\r\n\r\n")
            .await?;
        let mut buf = Vec::new();
        tls.read_to_end(&mut buf).await?;
        Ok::<_, std::io::Error>(buf)
    }
    .await;
    assert!(
        outcome.map(|b| b.is_empty()).unwrap_or(true),
        "sin certificado no debe haber respuesta"
    );

    drop(client);
    drop(identity);
    drop(vault);

    // 4) Ningún secreto en logs, red ni disco.
    let db_key = keys.get_or_create(KeyId::DbMaster, "").unwrap();
    let seal_key = keys.get_or_create(KeyId::CertSeal, "").unwrap();
    let secrets = [
        ("clave privada PKCS#8", forms(&client_cert.key_pkcs8)),
        (
            "contraseña del .p12",
            vec![TEST_PASSWORD.as_bytes().to_vec()],
        ),
        ("clave de la BD", forms(db_key.expose())),
        ("clave de sellado", forms(seal_key.expose())),
    ];

    let log_bytes = logs();
    assert!(
        contains(&log_bytes, "certificado importado".as_bytes()),
        "la captura de logs no funciona"
    );
    assert!(contains(&log_bytes, b"POST AEAT"));
    assert_absent("los logs", &log_bytes, &secrets);

    let wire_bytes = wire.lock().unwrap().clone();
    assert!(
        wire_bytes.len() > 1000,
        "el relé debe haber visto el tráfico"
    );
    assert_absent("la red", &wire_bytes, &secrets);

    for entry in std::fs::read_dir(&data_dir).unwrap() {
        let path = entry.unwrap().path();
        let bytes = std::fs::read(&path).unwrap();
        assert!(
            !bytes.starts_with(b"SQLite format 3"),
            "{} no está cifrado",
            path.display()
        );
        assert_absent(&path.display().to_string(), &bytes, &secrets);
        // Tampoco los datos del certificado (la BD entera está cifrada).
        assert!(
            !contains(&bytes, info.holder_nif.as_bytes()),
            "NIF en claro en {}",
            path.display()
        );
    }
}
