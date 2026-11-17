//! El cliente de Enable Banking de extremo a extremo contra un servidor simulado (TLS con CA
//! propia): lista de bancos, autorización, sesión, movimientos con varias páginas, saldos,
//! retirada del permiso, permiso caducado y redirecciones que no se siguen. Cada petición tiene
//! que llevar un JWT RS256 bien firmado con la clave de la aplicación.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use taxpain_banca::BancaError;
use taxpain_banca::enable::{Cliente, PeticionAuth};
use taxpain_banca::jwt::Firmante;
use taxpain_testkit::{TestCert, TestPki};
use time::macros::{date, datetime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio_rustls::rustls::{self, ServerConfig};

const APP: &str = "0a1b2c3d-0000-4000-8000-00000000a1b2";

/// Clave sintética de la aplicación (fixtures/certs/rsa-app-banca.der, PKCS#8).
fn clave_der() -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/certs/rsa-app-banca.der"),
    )
    .unwrap()
}

/// La misma clave en PEM, como la descarga el navegador al registrar la aplicación.
fn clave_pem() -> String {
    let b64 = STANDARD.encode(clave_der());
    let lineas: Vec<&str> = b64
        .as_bytes()
        .chunks(64)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect();
    let tipo = ["PRIVATE", "KEY"].join(" ");
    format!(
        "-----BEGIN {tipo}-----\n{}\n-----END {tipo}-----\n",
        lineas.join("\n")
    )
}

#[derive(Clone, Debug)]
struct Peticion {
    metodo: String,
    ruta: String,
    consulta: Vec<(String, String)>,
    auth: Option<String>,
    cuerpo: String,
}

impl Peticion {
    fn param(&self, k: &str) -> Option<&str> {
        self.consulta
            .iter()
            .find(|(c, _)| c == k)
            .map(|(_, v)| v.as_str())
    }
}

fn sesion_json() -> serde_json::Value {
    serde_json::json!({
        "session_id": "s-1",
        "accounts": [
            {
                "uid": "c-1",
                "account_id": { "iban": "ES9121000418450200051332" },
                "name": "EMPRESA DEMO SL",
                "currency": "EUR",
                "product": "Cuenta negocios",
                "cash_account_type": "CACC",
                "identification_hash": "h-cuenta-1"
            },
            { "uid": "c-2", "currency": "EUR", "cash_account_type": "CARD" }
        ],
        "aspsp": { "name": "BBVA", "country": "ES" },
        "psu_type": "business",
        "access": { "valid_until": "2027-04-29T10:00:00+00:00" }
    })
}

fn responder(p: &Peticion) -> (u16, String) {
    let ok = |v: serde_json::Value| (200, v.to_string());
    match (p.metodo.as_str(), p.ruta.as_str()) {
        ("GET", "/aspsps") => {
            assert_eq!(p.param("country"), Some("ES"));
            ok(serde_json::json!({ "aspsps": [
                { "name": "BBVA", "country": "ES", "psu_types": ["business", "personal"],
                  "maximum_consent_validity": 15_552_000, "beta": false, "bic": "BBVAESMM" },
                { "name": "Revolut", "country": "ES", "psu_types": ["business", "personal"] }
            ]}))
        }
        ("POST", "/auth") => {
            let b: serde_json::Value = serde_json::from_str(&p.cuerpo).unwrap();
            assert_eq!(b["aspsp"]["name"], "BBVA");
            assert_eq!(b["aspsp"]["country"], "ES");
            assert_eq!(b["psu_type"], "business");
            assert_eq!(b["state"], "e5");
            assert_eq!(b["redirect_url"], "https://localhost/taxpain/banco");
            assert_eq!(b["access"]["valid_until"], "2027-04-29T10:00:00Z");
            ok(serde_json::json!({
                "url": "https://banco.example/autorizar?id=a-1",
                "authorization_id": "a-1",
                "psu_id_hash": "h"
            }))
        }
        ("POST", "/sessions") => {
            let b: serde_json::Value = serde_json::from_str(&p.cuerpo).unwrap();
            if b["code"] == "c0d1g0" {
                ok(sesion_json())
            } else {
                (
                    422,
                    r#"{"message":"Wrong request parameters","code":422,"error":"WRONG_REQUEST_PARAMETERS"}"#
                        .into(),
                )
            }
        }
        ("GET", "/sessions/s-1") => ok(serde_json::json!({
            "status": "AUTHORIZED",
            "access": { "valid_until": "2027-04-29T10:00:00+00:00" },
            "accounts": ["c-1", "c-2"]
        })),
        ("DELETE", "/sessions/s-1") => ok(serde_json::json!({ "message": "OK" })),
        ("GET", "/accounts/c-1/balances") => ok(serde_json::json!({ "balances": [
            { "name": "Saldo contable", "balance_amount": { "currency": "EUR", "amount": "1234.56" },
              "balance_type": "CLBD", "reference_date": "2026-10-30" }
        ]})),
        ("GET", "/accounts/c-1/transactions") => match p.param("continuation_key") {
            None => ok(serde_json::json!({
                "transactions": [
                    { "entry_reference": "E1", "transaction_id": "T1",
                      "transaction_amount": { "currency": "EUR", "amount": "1210.00" },
                      "credit_debit_indicator": "CRDT", "status": "BOOK",
                      "booking_date": "2026-10-02", "value_date": "2026-10-02",
                      "remittance_information": ["TRANSFERENCIA FRA 2026-0007"],
                      "debtor": { "name": "CLIENTE DEMO SL" } },
                    { "entry_reference": "E2",
                      "transaction_amount": { "currency": "EUR", "amount": "60.50" },
                      "credit_debit_indicator": "DBIT", "status": "BOOK",
                      "booking_date": "2026-10-05",
                      "creditor": { "name": "PROVEEDOR DEMO SA" } }
                ],
                "continuation_key": "k2"
            })),
            Some("k2") => ok(serde_json::json!({
                "transactions": [
                    { "transaction_id": "T3",
                      "transaction_amount": { "currency": "EUR", "amount": "9.99" },
                      "credit_debit_indicator": "DBIT", "status": "PDNG" }
                ],
                "continuation_key": null
            })),
            Some(k) => panic!("clave de continuación inesperada: {k}"),
        },
        ("GET", "/accounts/caducada/transactions") => (
            401,
            r#"{"message":"Session is revoked","code":401,"error":"REVOKED_SESSION"}"#.into(),
        ),
        _ => (404, r#"{"message":"Not found","code":404}"#.into()),
    }
}

/// Servidor simulado: guarda cada petición y responde según la ruta. `/accounts/movida/…`
/// responde con una redirección a otro sitio (que el cliente no tiene que seguir).
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
                let (fin_cabecera, largo) = loop {
                    let n = tls.read(&mut buf).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    req.extend_from_slice(&buf[..n]);
                    if let Some(i) = req.windows(4).position(|w| w == b"\r\n\r\n") {
                        let cab = String::from_utf8_lossy(&req[..i]).to_lowercase();
                        let largo = cab
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .map(|v| v.trim().parse::<usize>().unwrap())
                            .unwrap_or(0);
                        break (i + 4, largo);
                    }
                };
                while req.len() < fin_cabecera + largo {
                    let n = tls.read(&mut buf).await.unwrap();
                    req.extend_from_slice(&buf[..n]);
                }
                let cabecera = String::from_utf8_lossy(&req[..fin_cabecera]).into_owned();
                let mut lineas = cabecera.lines();
                let mut primera = lineas.next().unwrap().split_whitespace();
                let metodo = primera.next().unwrap().to_owned();
                let destino = primera.next().unwrap().to_owned();
                let (ruta, consulta) = destino.split_once('?').unwrap_or((&destino, ""));
                let auth = lineas.find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("authorization")
                        .then(|| v.trim().to_owned())
                });
                let p = Peticion {
                    metodo,
                    ruta: ruta.to_owned(),
                    consulta: url::form_urlencoded::parse(consulta.as_bytes())
                        .map(|(k, v)| (k.into_owned(), v.into_owned()))
                        .collect(),
                    auth,
                    cuerpo: String::from_utf8(req[fin_cabecera..fin_cabecera + largo].to_vec())
                        .unwrap(),
                };
                vistas.lock().unwrap().push(p.clone());
                let salida = if p.ruta.starts_with("/accounts/movida/") {
                    "HTTP/1.1 302 Found\r\nlocation: https://otro.example/\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_owned()
                } else {
                    let (estado, cuerpo) = responder(&p);
                    format!(
                        "HTTP/1.1 {estado} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{cuerpo}",
                        cuerpo.len()
                    )
                };
                tls.write_all(salida.as_bytes()).await.unwrap();
                tls.shutdown().await.ok();
            });
        }
    });
    dir
}

/// Comprueba el JWT de una petición: cabecera, contenido y firma RS256 con la clave pública.
fn comprobar_token(auth: Option<&str>) {
    let token = auth
        .and_then(|a| a.strip_prefix("Bearer "))
        .expect("sin token");
    let partes: Vec<&str> = token.split('.').collect();
    assert_eq!(partes.len(), 3, "{token}");
    let json = |s: &str| -> serde_json::Value {
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(s).unwrap()).unwrap()
    };
    let cab = json(partes[0]);
    assert_eq!(cab["alg"], "RS256");
    assert_eq!(cab["typ"], "JWT");
    assert_eq!(cab["kid"], APP);
    let cuerpo = json(partes[1]);
    assert_eq!(cuerpo["iss"], "enablebanking.com");
    assert_eq!(cuerpo["aud"], "api.enablebanking.com");
    let iat = cuerpo["iat"].as_i64().unwrap();
    let exp = cuerpo["exp"].as_i64().unwrap();
    assert!(exp > iat && exp - iat <= 86_400, "validez {}", exp - iat);
    let par = ring::signature::RsaKeyPair::from_pkcs8(&clave_der()).unwrap();
    let publica = ring::signature::UnparsedPublicKey::new(
        &ring::signature::RSA_PKCS1_2048_8192_SHA256,
        ring::signature::KeyPair::public_key(&par).as_ref().to_vec(),
    );
    publica
        .verify(
            format!("{}.{}", partes[0], partes[1]).as_bytes(),
            &URL_SAFE_NO_PAD.decode(partes[2]).unwrap(),
        )
        .expect("la firma del token no verifica");
}

#[tokio::test]
async fn conexion_completa_contra_el_servidor_simulado() {
    let pki = TestPki::new("TaxPain Pruebas", "CA Pruebas Banca");
    let vistas = Arc::new(Mutex::new(Vec::new()));
    let dir = servidor(pki.server("localhost"), vistas.clone()).await;
    let firmante = Firmante::nuevo(APP, &clave_pem()).unwrap();
    // La clave no sale ni en Debug.
    assert!(!format!("{firmante:?}").contains("MII"));
    let cliente = Cliente::con_raiz(
        firmante,
        &pki.ca_der,
        &format!("https://localhost:{}/", dir.port()),
    )
    .unwrap();

    // Bancos.
    let bancos = cliente.bancos("ES").await.unwrap();
    assert_eq!(
        bancos.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(),
        ["BBVA", "Revolut"]
    );
    assert_eq!(bancos[0].maximum_consent_validity, Some(15_552_000));

    // Autorización.
    let auth = cliente
        .autorizar(&PeticionAuth {
            banco: "BBVA".into(),
            pais: "ES".into(),
            tipo: "business".into(),
            url_vuelta: "https://localhost/taxpain/banco".into(),
            state: "e5".into(),
            hasta: datetime!(2027-04-29 10:00 UTC),
        })
        .await
        .unwrap();
    assert_eq!(auth.url, "https://banco.example/autorizar?id=a-1");

    // Sesión con el código de vuelta; un código malo es un error de la API.
    let s = cliente.sesion("c0d1g0").await.unwrap();
    assert_eq!(s.session_id, "s-1");
    assert_eq!(s.accounts.len(), 2);
    assert_eq!(s.accounts[0].iban(), Some("ES9121000418450200051332"));
    assert_eq!(s.accounts[1].iban(), None);
    assert_eq!(s.access.valid_until, "2027-04-29T10:00:00+00:00");
    match cliente.sesion("otro").await.unwrap_err() {
        BancaError::Api { estado, codigo, .. } => {
            assert_eq!(estado, 422);
            assert_eq!(codigo.as_deref(), Some("WRONG_REQUEST_PARAMETERS"));
        }
        e => panic!("{e:?}"),
    }
    let estado = cliente.estado_sesion("s-1").await.unwrap();
    assert_eq!(estado.status.as_deref(), Some("AUTHORIZED"));

    // Movimientos: la primera descarga pide el historial más largo y sigue las páginas.
    let movs = cliente.movimientos("c-1", None).await.unwrap();
    assert_eq!(movs.len(), 3);
    assert_eq!(movs[0].centimos(), Some(121_000));
    assert_eq!(movs[0].concepto(), "TRANSFERENCIA FRA 2026-0007");
    assert_eq!(movs[1].centimos(), Some(-6_050));
    assert_eq!(movs[1].concepto(), "PROVEEDOR DEMO SA");
    assert!(!movs[2].contabilizado());
    // Las siguientes, desde una fecha.
    cliente
        .movimientos("c-1", Some(date!(2026 - 10 - 01)))
        .await
        .unwrap();

    let saldos = cliente.saldos("c-1").await.unwrap();
    assert_eq!(saldos[0].balance_amount.amount, "1234.56");

    // Permiso retirado o caducado.
    assert!(matches!(
        cliente.movimientos("caducada", None).await.unwrap_err(),
        BancaError::Caducado
    ));
    // Una redirección no se sigue (y el error no lleva la dirección).
    let e = cliente.movimientos("movida", None).await.unwrap_err();
    assert!(matches!(e, BancaError::Api { estado: 302, .. }), "{e:?}");
    assert!(!e.to_string().contains("otro.example"), "{e}");
    // Un identificador raro no llega a salir.
    assert!(cliente.saldos("../sessions/s-1").await.is_err());

    cliente.borrar_sesion("s-1").await.unwrap();

    // Lo que ha visto el servidor.
    let vistas = vistas.lock().unwrap().clone();
    let movimientos: Vec<&Peticion> = vistas
        .iter()
        .filter(|p| p.ruta == "/accounts/c-1/transactions")
        .collect();
    assert_eq!(movimientos.len(), 4);
    assert_eq!(movimientos[0].param("strategy"), Some("longest"));
    assert_eq!(movimientos[0].param("date_from"), None);
    // La página siguiente repite la consulta con la clave de continuación.
    assert_eq!(movimientos[1].param("continuation_key"), Some("k2"));
    assert_eq!(movimientos[1].param("strategy"), Some("longest"));
    assert_eq!(movimientos[2].param("date_from"), Some("2026-10-01"));
    assert_eq!(movimientos[2].param("strategy"), None);
    assert_eq!(movimientos[3].param("date_from"), Some("2026-10-01"));
    assert_eq!(movimientos[3].param("continuation_key"), Some("k2"));
    assert!(
        vistas
            .iter()
            .any(|p| p.metodo == "DELETE" && p.ruta == "/sessions/s-1")
    );
    assert!(!vistas.iter().any(|p| p.ruta.contains("..")));
    for p in &vistas {
        comprobar_token(p.auth.as_deref());
    }
}

#[test]
fn claves_que_no_sirven() {
    // Sin id de aplicación.
    assert!(matches!(
        Firmante::nuevo(" ", &clave_pem()),
        Err(BancaError::Clave(_))
    ));
    // No es PEM.
    assert!(matches!(
        Firmante::nuevo(APP, "hola"),
        Err(BancaError::Clave(_))
    ));
    // Una clave que no es RSA (la de un certificado de prueba es ECDSA P-256).
    let ec = taxpain_testkit::random_key_pkcs8();
    let tipo = ["PRIVATE", "KEY"].join(" ");
    let pem = format!(
        "-----BEGIN {tipo}-----\n{}\n-----END {tipo}-----\n",
        STANDARD.encode(ec)
    );
    match Firmante::nuevo(APP, &pem) {
        Err(BancaError::Clave(m)) => assert!(m.contains("RSA"), "{m}"),
        r => panic!("{r:?}"),
    }
}
