//! Conversación completa contra un servidor local que imita la API de chat (modelo local):
//! lo que sale va anonimizado, la respuesta vuelve con los datos y no se envía ninguna clave.

#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use taxpain_assistant::{
    Client, ConversationInput, LocalTools, Material, Message, Provider, Role, ask, ask_with_tools,
    prepare, prepare_conversation,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn read_request(stream: &mut tokio::net::TcpStream) -> String {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = stream.read(&mut chunk).await.unwrap();
        buf.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&buf).to_string();
        if let Some(i) = text.find("\r\n\r\n") {
            let len = text
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            if buf.len() >= i + 4 + len {
                return text;
            }
        }
        if n == 0 {
            return text;
        }
    }
}

#[tokio::test]
async fn ida_anonimizada_y_vuelta_restaurada() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        let req = read_request(&mut s).await;
        // El modelo contesta usando el marcador.
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"El cliente [NIF_2] te debe 1.210,00 €."}}]}"#;
        let resp = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        s.write_all(resp.as_bytes()).await.unwrap();
        req
    });

    let provider = Provider::OpenAiCompatible {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        model: "local".into(),
    };
    let out = prepare(
        "Titular 12345678Z. Pendiente de cobro: 1.210,00 €.",
        &[Message {
            role: Role::User,
            content: "¿Qué me debe B12345674?".into(),
        }],
    )
    .unwrap();
    let client = Client::new().unwrap();
    let answer = ask(&client, &provider, Some("clave-local-de-prueba"), &out)
        .await
        .unwrap();
    assert_eq!(answer.text, "El cliente B12345674 te debe 1.210,00 €.");
    assert_eq!(answer.replaced, 2);

    let req = server.await.unwrap();
    let (head, body) = req.split_once("\r\n\r\n").unwrap();
    assert!(head.starts_with("POST /v1/chat/completions"));
    // A un modelo local nunca se le manda la clave (aunque haya una guardada).
    assert!(!head.to_ascii_lowercase().contains("authorization"));
    assert!(!head.contains("clave-local-de-prueba"));
    for raw in ["12345678Z", "B12345674", "clave-local-de-prueba"] {
        assert!(!body.contains(raw), "{raw} no debe ir en el cuerpo");
    }
    assert!(body.contains("[NIF_1]") && body.contains("[NIF_2]"));
}

/// Servidor que contesta las respuestas del guion en orden y guarda cada petición.
async fn scripted(replies: Vec<String>, seen: Arc<Mutex<Vec<String>>>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        for body in replies {
            let Ok((mut s, _)) = listener.accept().await else {
                return;
            };
            let req = read_request(&mut s).await;
            seen.lock().unwrap().push(req);
            let resp = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            s.write_all(resp.as_bytes()).await.unwrap();
        }
    });
    port
}

/// Facturas emitidas de prueba: devuelve una a nombre de lo que se busca (ya restaurado).
struct Facturas;

impl LocalTools for Facturas {
    fn call(&self, name: &str, input: &serde_json::Value) -> Result<String, String> {
        assert_eq!(name, "buscar_facturas");
        let texto = input["texto"].as_str().unwrap_or("");
        Ok(format!(
            "2026-03-01 · F-2026-001 · {texto} · base 1.000,00 € · total 1.210,00 € · emitida"
        ))
    }
}

#[tokio::test]
async fn lo_tapado_en_la_conversacion_se_tapa_en_lo_que_devuelve_una_herramienta() {
    // El usuario nombra a su clienta en un dato: va con marcador. El modelo busca sus facturas
    // por ese marcador y la herramienta (en el Mac) devuelve una línea con el nombre, donde el
    // detector no lo ve: tiene que volver al modelo con el mismo marcador.
    let fact = "Nuestra clienta principal es Ingrid Svensson, de Suecia: nos compra cada mes.";
    let materials = [Material::Fact {
        text: fact,
        added: "31/10/2026",
    }];
    let history = [Message {
        role: Role::User,
        content: "¿Cuánto le he facturado este año?".into(),
    }];
    let out = prepare_conversation(
        &ConversationInput {
            topic: None,
            context: "- Tipo de contribuyente: sociedad (persona jurídica).",
            materials: &materials,
            history: &history,
            omitted: 0,
        },
        &[] as &[&str],
    )
    .unwrap();
    let last = &out.messages[0].content;
    assert!(
        last.contains("[NOMBRE_1]") && !last.contains("Svensson"),
        "{last}"
    );

    let seen = Arc::new(Mutex::new(Vec::new()));
    let call = serde_json::json!({"choices":[{"message":{"content":null,"tool_calls":[
        {"id":"c1","type":"function","function":{"name":"buscar_facturas",
         "arguments":"{\"tipo\":\"emitidas\",\"ejercicio\":2026,\"texto\":\"[NOMBRE_1]\"}"}}]}}]})
    .to_string();
    let done =
        r#"{"choices":[{"message":{"content":"A [NOMBRE_1] le has facturado 1.210,00 €."}}]}"#;
    let port = scripted(vec![call, done.to_owned()], seen.clone()).await;
    let provider = Provider::OpenAiCompatible {
        base_url: format!("http://127.0.0.1:{port}/v1"),
        model: "local".into(),
    };
    let answer = ask_with_tools(&Client::new().unwrap(), &provider, None, &out, &Facturas)
        .await
        .unwrap();
    assert_eq!(
        answer.text,
        "A Ingrid Svensson le has facturado 1.210,00 €."
    );
    let requests = seen.lock().unwrap().clone();
    assert_eq!(requests.len(), 2);
    let (_, body) = requests[1].split_once("\r\n\r\n").unwrap();
    assert!(!body.contains("Svensson"), "sale el nombre:\n{body}");
    assert!(
        body.contains("F-2026-001 · [NOMBRE_1] · base 1.000,00 €"),
        "{body}"
    );
}
