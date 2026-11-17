//! El asistente con herramientas de sólo lectura, contra un proveedor falso «compatible con
//! OpenAI» en 127.0.0.1 que sigue un guion: pide herramientas y después responde.

#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use taxpain_app::accounting::manual_entry;
use taxpain_app::assistant::tool_call;
use taxpain_assistant::{
    AssistantError, Client, LocalTools, Message, Provider, Role, ask_with_tools, prepare_with,
};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::money::Money;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Tools(Mutex<Vault>);

impl LocalTools for Tools {
    fn call(&self, name: &str, input: &serde_json::Value) -> Result<String, String> {
        tool_call(&self.0.lock().unwrap(), date!(2026 - 10 - 20), name, input)
    }
}

/// Proveedor falso: devuelve las respuestas del guion en orden y guarda las peticiones.
async fn scripted(replies: Vec<String>, seen: Arc<Mutex<Vec<String>>>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let replies = Arc::new(Mutex::new(replies.into_iter()));
    tokio::spawn(async move {
        while let Ok((mut sock, _)) = listener.accept().await {
            let (seen, replies) = (seen.clone(), replies.clone());
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    let t = String::from_utf8_lossy(&buf).into_owned();
                    if let Some(h) = t.find("\r\n\r\n") {
                        let len = t[..h]
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if buf.len() >= h + 4 + len {
                            break;
                        }
                    }
                }
                seen.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf).into_owned());
                let body = replies.lock().unwrap().next().unwrap_or_default();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = sock.write_all(resp.as_bytes()).await;
            });
        }
    });
    format!("http://{addr}/v1")
}

fn call(id: &str, name: &str, args: &str) -> String {
    serde_json::json!({"choices":[{"message":{"content":null,"tool_calls":[
        {"id":id,"type":"function","function":{"name":name,"arguments":args}}]}}]})
    .to_string()
}

fn text(t: &str) -> String {
    serde_json::json!({"choices":[{"message":{"content":t}}]}).to_string()
}

fn vault() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    manual_entry(
        &v,
        &JournalEntry {
            date: date!(2026 - 03 - 01),
            concept: "Capital".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("5720001"), Money::from_euros(3000, 0)),
                EntryLine::credit(acc("100"), Money::from_euros(3000, 0)),
            ],
        },
        datetime!(2026-03-01 9:00 UTC),
    )
    .unwrap();
    v
}

#[tokio::test]
async fn consulta_un_saldo_y_avisa_de_citas_sin_leer() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let url = scripted(
        vec![
            call("c1", "saldo_cuenta", r#"{"cuenta":"572","ejercicio":2026}"#),
            // Al BOE no se le mandan marcadores: se rechaza sin salir a la red.
            call("c2", "buscar_ley", r#"{"consulta":"impuesto de [DATO_1]"}"#),
            text("Tienes 3.000,00 € en bancos. El art. 99 LIVA dice «se deducirá en la declaración-liquidación del periodo en que se soporte»."),
        ],
        seen.clone(),
    )
    .await;
    let tools = Tools(Mutex::new(vault()));
    let q = [Message {
        role: Role::User,
        content: "¿Cuánto tengo en el banco?".into(),
    }];
    let out = prepare_with("", &q, &["Empresa Canario SL"]).unwrap();
    let provider = Provider::OpenAiCompatible {
        base_url: url,
        model: "falso".into(),
    };
    let a = ask_with_tools(&Client::new().unwrap(), &provider, None, &out, &tools)
        .await
        .unwrap();
    assert!(a.text.starts_with("Tienes 3.000,00 €"));
    assert_eq!(a.used, ["saldo_cuenta", "buscar_ley"]);
    assert!(a.sources.is_empty());
    assert_eq!(a.unverified.len(), 1, "cita sin leer el artículo");
    let reqs = seen.lock().unwrap().clone();
    assert_eq!(reqs.len(), 3);
    // Se ofrecen las herramientas y vuelve el resultado de la consulta local.
    assert!(reqs[0].contains("\"saldo_cuenta\"") && reqs[0].contains("\"leer_articulo\""));
    assert!(reqs[1].contains("debe 3.000,00 €") && reqs[1].contains("\"role\":\"tool\""));
    // El resultado va entre las marcas de documento externo: datos, no instrucciones.
    assert!(
        reqs[1].contains("<<<DOCUMENTO EXTERNO: resultado de saldo_cuenta>>>")
            && reqs[1].contains("<<<FIN DEL DOCUMENTO EXTERNO>>>"),
        "{}",
        reqs[1]
    );
    assert!(reqs[2].contains("no puede llevar datos personales"));
}

#[tokio::test]
async fn una_herramienta_que_no_existe_corta_la_consulta() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let url = scripted(
        vec![call("x", "presentar_modelo", r#"{"modelo":"303"}"#)],
        seen,
    )
    .await;
    let tools = Tools(Mutex::new(vault()));
    let q = [Message {
        role: Role::User,
        content: "Presenta el 303".into(),
    }];
    let out = prepare_with("", &q, &[] as &[&str]).unwrap();
    let provider = Provider::OpenAiCompatible {
        base_url: url,
        model: "falso".into(),
    };
    let r = ask_with_tools(&Client::new().unwrap(), &provider, None, &out, &tools).await;
    assert_eq!(
        r,
        Err(AssistantError::UnknownTool("presentar_modelo".into()))
    );
}

#[tokio::test]
async fn una_pregunta_sobre_un_tema_usa_herramientas_y_guarda_las_notas() {
    use taxpain_app::assistant::{AssistantSettings, save_settings};
    use taxpain_app::conversaciones::{self, AskRequest, Topic};

    let seen = Arc::new(Mutex::new(Vec::new()));
    let url = scripted(
        vec![
            call("c1", "saldo_cuenta", r#"{"cuenta":"572","ejercicio":2026}"#),
            text("Con 3.000,00 € en bancos puedes dejar el beneficio en reservas."),
        ],
        seen.clone(),
    )
    .await;
    let tools = Tools(Mutex::new(vault()));
    save_settings(
        &tools.0.lock().unwrap(),
        &AssistantSettings {
            provider: Provider::OpenAiCompatible {
                base_url: url,
                model: "falso".into(),
            },
            consent: false,
        },
        datetime!(2026-10-20 9:00 UTC),
    )
    .unwrap();
    let topic = Topic::Ahorro {
        id: "capitalizacion".into(),
        year: 2026,
    };
    let req = AskRequest {
        topic: &topic,
        conversation: None,
        question: "¿Tengo liquidez para no repartir?",
        hoy: date!(2026 - 10 - 20),
    };
    let o = conversaciones::ask(
        || Ok(tools.0.lock().unwrap()),
        || Ok(tools.0.lock().unwrap()),
        &Client::new().unwrap(),
        &tools,
        &req,
        || datetime!(2026-10-20 10:00 UTC),
    )
    .await
    .unwrap();
    let c = o.conversation.unwrap();
    let notes = c.messages[1].notes.clone().unwrap();
    assert_eq!(notes.model, "falso");
    assert_eq!(notes.used, ["saldo_cuenta"]);
    assert!(
        c.messages[1]
            .content
            .starts_with("Con 3.000,00 € en bancos")
    );
    let reqs = seen.lock().unwrap().clone();
    assert_eq!(reqs.len(), 2);
    assert!(reqs[0].contains("### Tema de esta conversación"));
    assert!(reqs[1].contains("debe 3.000,00 €"));
    // Lo guardado se vuelve a leer igual.
    let again =
        conversaciones::open_by_id(&tools.0.lock().unwrap(), date!(2026 - 10 - 20), c.id).unwrap();
    assert_eq!(again.conversation.unwrap().messages[1].notes, Some(notes));
}
