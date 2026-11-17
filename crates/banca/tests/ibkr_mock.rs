//! El cliente del Flex Web Service de Interactive Brokers contra un servidor simulado (TLS con
//! CA propia): petición del extracto, espera mientras se genera (1019), descarga, token
//! caducado y que el token no salga en los errores. Datos inventados.

#![allow(clippy::unwrap_used)]

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use taxpain_banca::ibkr::{ClienteFlex, FlexError};
use taxpain_testkit::{TestCert, TestPki};
use time::macros::date;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio_rustls::rustls::{self, ServerConfig};

const TOKEN: &str = "TOKEN0123456789abcdef";
const XML: &str = "<FlexQueryResponse queryName=\"q\" type=\"AF\"><FlexStatements count=\"1\"><FlexStatement accountId=\"U0000001\" fromDate=\"20260101\" toDate=\"20261231\"></FlexStatement></FlexStatements></FlexQueryResponse>";

#[derive(Clone, Debug)]
struct Peticion {
    ruta: String,
    consulta: Vec<(String, String)>,
    agente: Option<String>,
}

impl Peticion {
    fn param(&self, k: &str) -> Option<&str> {
        self.consulta
            .iter()
            .find(|(c, _)| c == k)
            .map(|(_, v)| v.as_str())
    }
}

fn responder(p: &Peticion, vistas: &[Peticion]) -> String {
    let fallo = |c: u32, m: &str| {
        format!(
            "<FlexStatementResponse timestamp=\"x\"><Status>Fail</Status><ErrorCode>{c}</ErrorCode><ErrorMessage>{m}</ErrorMessage></FlexStatementResponse>"
        )
    };
    match p.ruta.as_str() {
        "/fws/SendRequest" => {
            assert_eq!(p.param("v"), Some("3"));
            assert_eq!(p.param("q"), Some("123456"));
            assert_eq!(p.param("fd"), Some("20260101"));
            assert_eq!(p.param("td"), Some("20261231"));
            if p.param("t") == Some(TOKEN) {
                "<FlexStatementResponse timestamp=\"x\"><Status>Success</Status><ReferenceCode>5550001</ReferenceCode><url>https://otro.example/GetStatement</url></FlexStatementResponse>".into()
            } else {
                fallo(1012, "Token has expired.")
            }
        }
        "/fws/GetStatement" => {
            assert_eq!(p.param("q"), Some("5550001"));
            assert_eq!(p.param("t"), Some(TOKEN));
            // La primera vez aún se está generando.
            let antes = vistas
                .iter()
                .filter(|x| x.ruta == "/fws/GetStatement")
                .count();
            if antes <= 1 {
                fallo(
                    1019,
                    "Statement generation in progress. Please try again shortly.",
                )
            } else {
                XML.into()
            }
        }
        _ => "no".into(),
    }
}

async fn servidor(cert: TestCert, vistas: Arc<Mutex<Vec<Peticion>>>) -> SocketAddr {
    let proveedor = Arc::new(rustls::crypto::ring::default_provider());
    let config = ServerConfig::builder_with_provider(proveedor)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![CertificateDer::from(cert.cert_der.clone())],
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.key_pkcs8.clone())),
        )
        .unwrap();
    let aceptador = TlsAcceptor::from(Arc::new(config));
    let oyente = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dir = oyente.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (tcp, _) = oyente.accept().await.unwrap();
            let aceptador = aceptador.clone();
            let vistas = vistas.clone();
            tokio::spawn(async move {
                let Ok(mut tls) = aceptador.accept(tcp).await else {
                    return;
                };
                let mut req = Vec::new();
                let mut buf = [0u8; 8192];
                loop {
                    let n = tls.read(&mut buf).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    req.extend_from_slice(&buf[..n]);
                    if req.windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
                let cabecera = String::from_utf8_lossy(&req).into_owned();
                let mut lineas = cabecera.lines();
                let destino = lineas
                    .next()
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .to_owned();
                let (ruta, consulta) = destino.split_once('?').unwrap_or((&destino, ""));
                let agente = lineas.find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("user-agent")
                        .then(|| v.trim().to_owned())
                });
                let p = Peticion {
                    ruta: ruta.to_owned(),
                    consulta: url::form_urlencoded::parse(consulta.as_bytes())
                        .map(|(k, v)| (k.into_owned(), v.into_owned()))
                        .collect(),
                    agente,
                };
                let cuerpo = {
                    let mut v = vistas.lock().unwrap();
                    v.push(p.clone());
                    responder(&p, &v)
                };
                let salida = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: text/xml\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{cuerpo}",
                    cuerpo.len()
                );
                tls.write_all(salida.as_bytes()).await.unwrap();
                tls.shutdown().await.ok();
            });
        }
    });
    dir
}

#[tokio::test]
async fn descarga_el_extracto_esperando_a_que_se_genere() {
    let pki = TestPki::new("TaxPain Pruebas", "CA Pruebas IBKR");
    let vistas = Arc::new(Mutex::new(Vec::new()));
    let dir = servidor(pki.server("localhost"), vistas.clone()).await;
    let cliente = ClienteFlex::con_raiz(
        &pki.ca_der,
        &format!("https://localhost:{}/fws/", dir.port()),
    )
    .unwrap()
    .con_espera(Duration::from_millis(10), 5);
    let xml = cliente
        .descargar(
            TOKEN,
            "123456",
            date!(2026 - 01 - 01),
            date!(2026 - 12 - 31),
        )
        .await
        .unwrap();
    assert_eq!(xml, XML);
    let v = vistas.lock().unwrap().clone();
    // SendRequest y dos GetStatement (el primero, 1019); la <url> de la respuesta se ignora.
    assert_eq!(
        v.iter().map(|p| p.ruta.as_str()).collect::<Vec<_>>(),
        ["/fws/SendRequest", "/fws/GetStatement", "/fws/GetStatement"]
    );
    // IBKR pide User-Agent en todas.
    assert!(v.iter().all(|p| {
        p.agente
            .as_deref()
            .is_some_and(|a| a.starts_with("TaxPain/"))
    }));
}

#[tokio::test]
async fn token_caducado_con_su_mensaje_y_sin_el_token() {
    let pki = TestPki::new("TaxPain Pruebas", "CA Pruebas IBKR");
    let vistas = Arc::new(Mutex::new(Vec::new()));
    let dir = servidor(pki.server("localhost"), vistas.clone()).await;
    let cliente = ClienteFlex::con_raiz(
        &pki.ca_der,
        &format!("https://localhost:{}/fws/", dir.port()),
    )
    .unwrap()
    .con_espera(Duration::from_millis(10), 3);
    let err = cliente
        .descargar(
            "CADUCADO0000000000",
            "123456",
            date!(2026 - 01 - 01),
            date!(2026 - 12 - 31),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err, FlexError::Servicio { codigo: 1012, .. }),
        "{err:?}"
    );
    let msg = err.to_string();
    assert!(msg.contains("caducado"), "{msg}");
    assert!(!msg.contains("CADUCADO0000"), "{msg}");
    // Datos que no se mandan.
    assert!(
        cliente
            .descargar(
                "corto",
                "123456",
                date!(2026 - 01 - 01),
                date!(2026 - 12 - 31)
            )
            .await
            .is_err()
    );
    assert!(
        cliente
            .descargar(TOKEN, "12a", date!(2026 - 01 - 01), date!(2026 - 12 - 31))
            .await
            .is_err()
    );
    assert_eq!(vistas.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn sin_servidor_no_hay_conexion_y_el_error_no_lleva_el_token() {
    let pki = TestPki::new("TaxPain Pruebas", "CA Pruebas IBKR");
    // Un puerto cerrado.
    let oyente = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let puerto = oyente.local_addr().unwrap().port();
    drop(oyente);
    let cliente =
        ClienteFlex::con_raiz(&pki.ca_der, &format!("https://localhost:{puerto}/fws/")).unwrap();
    let err = cliente
        .descargar(
            TOKEN,
            "123456",
            date!(2026 - 01 - 01),
            date!(2026 - 12 - 31),
        )
        .await
        .unwrap_err();
    assert!(matches!(err, FlexError::SinConexion(_)), "{err:?}");
    assert!(!err.to_string().contains(TOKEN), "{err}");
}
