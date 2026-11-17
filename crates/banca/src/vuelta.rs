//! La vuelta del banco: después de autorizar, el navegador va a la dirección de vuelta registrada
//! en la aplicación de Enable Banking con `?code=…&state=…` (o `?error=…`). Hay dos maneras de
//! recogerla sin servidor:
//! - **Pegarla**: si la dirección registrada no la escucha nadie (por ejemplo
//!   `https://localhost/taxpain/banco`), el navegador enseña un error pero la barra de
//!   direcciones lleva el código; el usuario la copia y TaxPain saca el código ([`leer`]).
//! - **Escucharla**: si la dirección registrada es `http://127.0.0.1:<puerto>/…`, TaxPain escucha
//!   en ese puerto sólo mientras se conecta la cuenta y la recoge sola ([`esperar`]). Sólo
//!   acepta conexiones de la propia máquina (127.0.0.1) y termina con la primera petición que
//!   trae el código (o el error) con el `state` de la autorización empezada.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use url::Url;

use crate::{BancaError, Result};

/// Lo que trae la dirección de vuelta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Vuelta {
    Codigo {
        code: String,
        state: Option<String>,
    },
    /// El banco o el titular ha cancelado (o ha fallado la autorización).
    Error {
        error: String,
        descripcion: Option<String>,
        state: Option<String>,
    },
}

/// Saca el código (o el error) de lo que pega el usuario: la dirección completa a la que le ha
/// llevado el banco o sólo su parte de después de «?».
pub fn leer(texto: &str) -> Result<Vuelta> {
    let t = texto.trim();
    if t.is_empty() {
        return Err(BancaError::Vuelta(
            "pega la dirección completa a la que te ha llevado el banco".into(),
        ));
    }
    let consulta = match Url::parse(t) {
        Ok(u) => u.query().unwrap_or_default().to_owned(),
        Err(_) => t.trim_start_matches('?').to_owned(),
    };
    let pares: Vec<(String, String)> = url::form_urlencoded::parse(consulta.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let valor = |k: &str| {
        pares
            .iter()
            .find(|(c, _)| c == k)
            .map(|(_, v)| v.clone())
            .filter(|v| !v.is_empty())
    };
    if let Some(error) = valor("error") {
        return Ok(Vuelta::Error {
            error,
            descripcion: valor("error_description"),
            state: valor("state"),
        });
    }
    match valor("code") {
        Some(code) if code.len() <= 2_000 && !code.chars().any(char::is_whitespace) => {
            Ok(Vuelta::Codigo {
                code,
                state: valor("state"),
            })
        }
        _ => Err(BancaError::Vuelta(
            "esa dirección no trae el código de autorización («code=…»): copia la dirección \
             completa de la barra del navegador después de autorizar en el banco"
                .into(),
        )),
    }
}

/// Puerto y ruta si la dirección de vuelta es una que TaxPain puede escuchar
/// (`http://127.0.0.1:<puerto>/<ruta>`).
pub fn local(url_vuelta: &str) -> Option<(u16, String)> {
    let u = Url::parse(url_vuelta.trim()).ok()?;
    if u.scheme() != "http" || u.host_str() != Some("127.0.0.1") {
        return None;
    }
    Some((u.port()?, u.path().to_owned()))
}

const PAGINA_OK: &str = "<!doctype html><html lang=\"es\"><meta charset=\"utf-8\"><title>TaxPain</title>\
<body style=\"font-family:system-ui;margin:3rem;max-width:36rem\"><h1>Listo</h1>\
<p>El banco ha autorizado el acceso. Ya puedes cerrar esta pestaña y volver a TaxPain.</p></body></html>";

const PAGINA_ERROR: &str = "<!doctype html><html lang=\"es\"><meta charset=\"utf-8\"><title>TaxPain</title>\
<body style=\"font-family:system-ui;margin:3rem;max-width:36rem\"><h1>No se ha autorizado</h1>\
<p>Vuelve a TaxPain para ver qué ha pasado.</p></body></html>";

/// Abre el puerto; si lo tiene aún otra espera que se está cerrando, lo reintenta un poco.
async fn abrir(puerto: u16) -> Result<TcpListener> {
    let mut intentos = 0;
    loop {
        match TcpListener::bind(("127.0.0.1", puerto)).await {
            Ok(o) => return Ok(o),
            Err(_) if intentos < 6 => {
                intentos += 1;
                tokio::time::sleep(Duration::from_millis(300)).await;
            }
            Err(e) => {
                return Err(BancaError::Vuelta(format!(
                    "no se puede escuchar en 127.0.0.1:{puerto} ({e}); pega la dirección a mano"
                )));
            }
        }
    }
}

impl Vuelta {
    /// El `state` que trae la vuelta.
    pub fn state(&self) -> Option<&str> {
        match self {
            Vuelta::Codigo { state, .. } | Vuelta::Error { state, .. } => state.as_deref(),
        }
    }
}

/// Espera la vuelta del banco en `127.0.0.1:<puerto>` durante como mucho `espera`, mientras
/// `seguir()` diga que sí (se mira cada medio segundo). Atiende peticiones a `ruta` (las demás,
/// como el icono de la pestaña, reciben un 404) y termina con la primera que trae código o error
/// con el `state` de la autorización empezada (`esperado`); las demás no la paran.
pub async fn esperar(
    puerto: u16,
    ruta: &str,
    esperado: &str,
    espera: Duration,
    seguir: impl Fn() -> bool,
) -> Result<Vuelta> {
    let oyente = abrir(puerto).await?;
    let trabajo = async {
        loop {
            let aceptada =
                match tokio::time::timeout(Duration::from_millis(500), oyente.accept()).await {
                    Ok(a) => a,
                    Err(_) if seguir() => continue,
                    Err(_) => {
                        return Err(BancaError::Vuelta(
                            "se ha dejado de esperar la vuelta del banco".into(),
                        ));
                    }
                };
            let (mut tcp, origen) = aceptada.map_err(|e| BancaError::Vuelta(e.to_string()))?;
            if !origen.ip().is_loopback() {
                continue;
            }
            let mut buf = vec![0u8; 16 * 1024];
            let n = match tokio::time::timeout(Duration::from_secs(10), tcp.read(&mut buf)).await {
                Ok(Ok(n)) => n,
                _ => continue,
            };
            let peticion = String::from_utf8_lossy(&buf[..n]);
            let destino = peticion
                .lines()
                .next()
                .and_then(|l| l.split_whitespace().nth(1))
                .unwrap_or("/");
            let (camino, consulta) = destino.split_once('?').unwrap_or((destino, ""));
            if camino != ruta {
                let _ = tcp
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                continue;
            }
            let resultado = leer(&format!("?{consulta}"))
                .ok()
                .filter(|v| v.state() == Some(esperado));
            let pagina = if matches!(resultado, Some(Vuelta::Codigo { .. })) {
                PAGINA_OK
            } else {
                PAGINA_ERROR
            };
            let _ = tcp
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\n\
                         Content-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{pagina}",
                        pagina.len()
                    )
                    .as_bytes(),
                )
                .await;
            let _ = tcp.shutdown().await;
            match resultado {
                Some(v) => return Ok(v),
                None => continue,
            }
        }
    };
    tokio::time::timeout(espera, trabajo).await.map_err(|_| {
        BancaError::Vuelta(
            "el banco no ha vuelto a tiempo: si ya has autorizado, pega la dirección a mano".into(),
        )
    })?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lee_la_direccion_pegada() {
        assert_eq!(
            leer("https://localhost/taxpain/banco?state=abc&code=d1e2f3").unwrap(),
            Vuelta::Codigo {
                code: "d1e2f3".into(),
                state: Some("abc".into())
            }
        );
        // Sólo la consulta.
        assert_eq!(
            leer("?code=xyz").unwrap(),
            Vuelta::Codigo {
                code: "xyz".into(),
                state: None
            }
        );
        // El titular ha cancelado.
        assert_eq!(
            leer("https://localhost/taxpain/banco?error=access_denied&error_description=Cancelado")
                .unwrap(),
            Vuelta::Error {
                error: "access_denied".into(),
                descripcion: Some("Cancelado".into()),
                state: None
            }
        );
        assert!(leer("https://localhost/taxpain/banco").is_err());
        assert!(leer("").is_err());
    }

    #[test]
    fn solo_se_escucha_en_127_0_0_1() {
        assert_eq!(
            local("http://127.0.0.1:47219/taxpain/banco"),
            Some((47219, "/taxpain/banco".into()))
        );
        assert_eq!(local("https://127.0.0.1:47219/taxpain/banco"), None);
        assert_eq!(local("http://localhost:47219/taxpain/banco"), None);
        assert_eq!(local("https://localhost/taxpain/banco"), None);
    }

    #[tokio::test]
    async fn recoge_la_vuelta_en_local() {
        // Puerto libre al azar: se abre y se cierra un oyente para saber cuál está libre.
        let libre = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let puerto = libre.local_addr().unwrap().port();
        drop(libre);
        let espera = tokio::spawn(esperar(
            puerto,
            "/taxpain/banco",
            "e5",
            Duration::from_secs(5),
            || true,
        ));
        tokio::time::sleep(Duration::from_millis(100)).await;
        // Primero el icono de la pestaña (404), una vuelta con otro state (no la para) y después
        // la buena.
        for camino in [
            "/favicon.ico",
            "/taxpain/banco?error=access_denied&state=otro",
            "/taxpain/banco?code=c0d1g0&state=e5",
        ] {
            let mut tcp = tokio::net::TcpStream::connect(("127.0.0.1", puerto))
                .await
                .unwrap();
            tcp.write_all(format!("GET {camino} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                .await
                .unwrap();
            let mut r = String::new();
            tcp.read_to_string(&mut r).await.unwrap();
            assert!(r.starts_with("HTTP/1.1"), "{r}");
        }
        assert_eq!(
            espera.await.unwrap().unwrap(),
            Vuelta::Codigo {
                code: "c0d1g0".into(),
                state: Some("e5".into())
            }
        );
    }

    #[tokio::test]
    async fn deja_de_esperar_si_se_cancela() {
        let libre = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let puerto = libre.local_addr().unwrap().port();
        drop(libre);
        let seguir = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let s = seguir.clone();
        let espera = tokio::spawn(async move {
            esperar(
                puerto,
                "/taxpain/banco",
                "e5",
                Duration::from_secs(30),
                move || s.load(std::sync::atomic::Ordering::SeqCst),
            )
            .await
        });
        tokio::time::sleep(Duration::from_millis(200)).await;
        seguir.store(false, std::sync::atomic::Ordering::SeqCst);
        let r = tokio::time::timeout(Duration::from_secs(3), espera)
            .await
            .expect("no ha parado")
            .unwrap();
        assert!(matches!(r, Err(BancaError::Vuelta(_))), "{r:?}");
        // Y el puerto queda libre para la siguiente.
        assert!(TcpListener::bind(("127.0.0.1", puerto)).await.is_ok());
    }
}
