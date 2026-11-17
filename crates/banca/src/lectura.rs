//! Lectura acotada de una respuesta: un servidor averiado (o uno que no es el que se espera) no
//! puede llenar la memoria respondiendo sin fin.

/// Topes de lo que se lee de una respuesta.
pub(crate) mod topes {
    /// Una página de la API de Enable Banking (JSON).
    pub const ENABLE: usize = 5 * 1024 * 1024;
    /// Un extracto del Flex Web Service de Interactive Brokers (XML o CSV de hasta un año).
    pub const IBKR: usize = 10 * 1024 * 1024;
}

/// Por qué no se ha leído una respuesta entera.
#[derive(Debug)]
pub(crate) enum Lectura {
    /// Pasa del tope.
    Grande,
    /// Error de red al leerla.
    Red(reqwest::Error),
}

/// Lee el cuerpo de `res` hasta `max` bytes: rechaza antes de leer si el `Content-Length` ya
/// pasa del tope y, si no viene o miente, corta en cuanto lo leído lo pasa.
pub(crate) async fn leer_acotado(
    mut res: reqwest::Response,
    max: usize,
) -> Result<Vec<u8>, Lectura> {
    if res
        .content_length()
        .is_some_and(|n| n > u64::try_from(max).unwrap_or(u64::MAX))
    {
        return Err(Lectura::Grande);
    }
    let mut cuerpo = Vec::new();
    while let Some(trozo) = res.chunk().await.map_err(Lectura::Red)? {
        if cuerpo.len().saturating_add(trozo.len()) > max {
            return Err(Lectura::Grande);
        }
        cuerpo.extend_from_slice(&trozo);
    }
    Ok(cuerpo)
}

/// Como [`leer_acotado`], como texto (lo que no sea UTF-8 se sustituye, igual que
/// `Response::text` sin la feature `charset` de reqwest).
pub(crate) async fn leer_texto_acotado(
    res: reqwest::Response,
    max: usize,
) -> Result<String, Lectura> {
    let cuerpo = leer_acotado(res, max).await?;
    Ok(match String::from_utf8(cuerpo) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Servidor HTTP en 127.0.0.1 que contesta `cabecera` y luego `cuerpo` (sin TLS: sólo se
    /// prueba la lectura).
    async fn responde(cabecera: &'static str, cuerpo: Vec<u8>) -> String {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let dir = l.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf).await;
            let _ = s.write_all(cabecera.as_bytes()).await;
            let _ = s.write_all(&cuerpo).await;
            let _ = s.shutdown().await;
        });
        format!("http://{dir}/")
    }

    async fn pide(url: &str) -> reqwest::Response {
        let proveedor =
            std::sync::Arc::new(taxpain_vault::rustls::crypto::ring::default_provider());
        let tls = taxpain_vault::rustls::ClientConfig::builder_with_provider(proveedor)
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_root_certificates(taxpain_vault::rustls::RootCertStore::empty())
            .with_no_client_auth();
        reqwest::Client::builder()
            .tls_backend_preconfigured(tls)
            .no_proxy()
            .build()
            .unwrap()
            .get(url)
            .send()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn lectura_con_tope() {
        let url = responde(
            "HTTP/1.1 200 OK\r\ncontent-length: 7\r\nconnection: close\r\n\r\n",
            br#"{"a":1}"#.to_vec(),
        )
        .await;
        assert_eq!(
            leer_texto_acotado(pide(&url).await, 7).await.unwrap(),
            r#"{"a":1}"#
        );

        // Un Content-Length por encima del tope se rechaza sin leer el cuerpo.
        let url = responde(
            "HTTP/1.1 200 OK\r\ncontent-length: 1000000000\r\nconnection: close\r\n\r\n",
            Vec::new(),
        )
        .await;
        assert!(matches!(
            leer_acotado(pide(&url).await, 1024).await,
            Err(Lectura::Grande)
        ));

        // Sin Content-Length (hasta que se cierra) se corta al pasar el tope.
        let url = responde(
            "HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n",
            vec![b'a'; 64 * 1024],
        )
        .await;
        assert!(matches!(
            leer_acotado(pide(&url).await, 1024).await,
            Err(Lectura::Grande)
        ));
    }
}
