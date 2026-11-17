//! Conversaciones con la IA guardadas por tema, de punta a punta con una sociedad inventada y un
//! proveedor falso «compatible con OpenAI» en 127.0.0.1 que sigue un guion (nada sale a la red).

#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex, MutexGuard};

use taxpain_app::accounting::record_entry;
use taxpain_app::ahorro::{Estado, Marca, marcar};
use taxpain_app::assistant::{self, AssistantSettings, tool_call};
use taxpain_app::companies::{self, CompanyInfo};
use taxpain_app::conversaciones::{
    self, AskRequest, MAX_CONVERSATIONS, MAX_DOCUMENT_CHARS, MAX_QUESTIONS, MaterialKind, ReadBy,
    Topic, TopicOpened,
};
use taxpain_app::invoicing;
use taxpain_app::received::{Nature, Reviewed, Source, ingest_file, review};
use taxpain_app::revision::{
    self, Area, Gravedad, Hallazgo, Origen, Responsable, Resumen, Revision,
};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::viajes::{self, Trip};
use taxpain_assistant::{Client, LocalTools, Provider, Role};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const HOY: Date = date!(2026 - 10 - 30);
const AHORA: OffsetDateTime = datetime!(2026-10-30 10:00 UTC);

fn e(euros: i64) -> Money {
    Money::from_euros(euros, 0)
}

fn asiento(v: &Vault, fecha: Date, origin: Origin, lineas: Vec<EntryLine>) {
    record_entry(
        v.connection(),
        &JournalEntry {
            date: fecha,
            concept: "Prueba".into(),
            origin,
            lines: lineas,
        },
        datetime!(2026-10-30 9:00 UTC),
    )
    .unwrap();
}

/// Una microempresa inventada con beneficio en 2025 que se queda en parte en reservas (la
/// reserva de capitalización le conviene).
fn empresa() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("B12345674".into()),
            name: "CLIENTE UNO SL".into(),
            address: addr(),
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
            facturae: false,
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
    invoicing::save_settings(&v, &s, datetime!(2025-01-01 9:00 UTC)).unwrap();
    let d = EntryLine::debit;
    let h = EntryLine::credit;
    asiento(
        &v,
        date!(2025 - 01 - 02),
        Origin::Manual,
        vec![d(acc("5720001"), e(3_000)), h(acc("100"), e(3_000))],
    );
    asiento(
        &v,
        date!(2025 - 06 - 30),
        Origin::Manual,
        vec![d(acc("5720001"), e(120_000)), h(acc("705"), e(120_000))],
    );
    asiento(
        &v,
        date!(2025 - 07 - 31),
        Origin::Manual,
        vec![d(acc("629"), e(40_000)), h(acc("5720001"), e(40_000))],
    );
    asiento(
        &v,
        date!(2026 - 01 - 01),
        Origin::Apertura,
        vec![
            d(acc("5720001"), e(83_000)),
            h(acc("100"), e(3_000)),
            h(acc("129"), e(80_000)),
        ],
    );
    asiento(
        &v,
        date!(2026 - 09 - 30),
        Origin::Manual,
        vec![d(acc("5720001"), e(100_000)), h(acc("705"), e(100_000))],
    );
    asiento(
        &v,
        date!(2026 - 09 - 30),
        Origin::Manual,
        vec![d(acc("629"), e(30_000)), h(acc("5720001"), e(30_000))],
    );
    v
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

fn text(t: &str) -> String {
    serde_json::json!({"choices":[{"message":{"content":t}}]}).to_string()
}

/// Herramientas de sólo lectura sobre el almacén de la prueba.
struct Tools<'a>(&'a Mutex<Vault>);

impl LocalTools for Tools<'_> {
    fn call(&self, name: &str, input: &serde_json::Value) -> Result<String, String> {
        tool_call(&self.0.lock().unwrap(), HOY, name, input)
    }
}

/// La empresa con el proveedor falso configurado y lo que éste recibe.
struct Env {
    v: Mutex<Vault>,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Env {
    async fn new(replies: &[&str]) -> Env {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let url = scripted(replies.iter().map(|t| text(t)).collect(), seen.clone()).await;
        let v = empresa();
        assistant::save_settings(
            &v,
            &AssistantSettings {
                provider: Provider::OpenAiCompatible {
                    base_url: url,
                    model: "falso".into(),
                },
                consent: false,
            },
            AHORA,
        )
        .unwrap();
        Env {
            v: Mutex::new(v),
            seen,
        }
    }

    fn vault(&self) -> MutexGuard<'_, Vault> {
        self.v.lock().unwrap()
    }

    async fn ask_at(
        &self,
        topic: &Topic,
        id: Option<i64>,
        q: &str,
        hoy: Date,
        now: OffsetDateTime,
    ) -> taxpain_app::Result<TopicOpened> {
        let req = AskRequest {
            topic,
            conversation: id,
            question: q,
            hoy,
        };
        conversaciones::ask(
            || Ok(self.vault()),
            || Ok(self.vault()),
            &Client::new().unwrap(),
            &Tools(&self.v),
            &req,
            || now,
        )
        .await
    }

    async fn ask(
        &self,
        topic: &Topic,
        id: Option<i64>,
        q: &str,
    ) -> taxpain_app::Result<TopicOpened> {
        self.ask_at(topic, id, q, HOY, AHORA).await
    }

    fn requests(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }

    fn chats(&self) -> Vec<serde_json::Value> {
        taxpain_vault::audit::entries(self.vault().connection())
            .unwrap()
            .into_iter()
            .filter(|e| e.action == "assistant.chat")
            .map(|e| serde_json::from_str(&e.details).unwrap())
            .collect()
    }

    fn rows(&self, table: &str) -> i64 {
        self.vault()
            .connection()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }
}

fn capitalizacion() -> Topic {
    Topic::Ahorro {
        id: "capitalizacion".into(),
        year: 2026,
    }
}

#[tokio::test]
async fn abrir_un_tema_no_crea_nada() {
    let env = Env::new(&[]).await;
    let o = conversaciones::open(&env.vault(), HOY, &capitalizacion()).unwrap();
    assert_eq!(
        o.header.title,
        "Reserva de capitalización: deja el beneficio en la empresa"
    );
    assert_eq!(o.header.group, "Ahorrar impuestos · 2026");
    assert!(o.header.intro.starts_with("Con tus datos, te conviene."));
    assert_eq!(o.header.suggestions.len(), 3);
    assert!(o.conversation.is_none() && !o.changed);
    let plan = conversaciones::open(&env.vault(), HOY, &Topic::AhorroPlan { year: 2026 }).unwrap();
    assert_eq!(plan.header.title, "Tu plan de ahorro de 2026");
    let g = conversaciones::open(&env.vault(), HOY, &Topic::General).unwrap();
    assert_eq!(
        (g.header.group.as_str(), g.header.title.as_str()),
        ("Asistente", "Nueva conversación")
    );
    assert!(conversaciones::list(&env.vault()).unwrap().is_empty());
    assert_eq!(env.rows("assistant_conversation"), 0);
    // Una ficha que no es para una sociedad, una que no existe y un ejercicio que ya no se
    // puede calcular.
    let autonomo = Topic::Ahorro {
        id: "plan_pensiones_autonomo".into(),
        year: 2026,
    };
    let e = conversaciones::open(&env.vault(), HOY, &autonomo).unwrap_err();
    assert_eq!(
        e.to_string(),
        "Esta recomendación no es para tu tipo de contribuyente."
    );
    let e = conversaciones::open(
        &env.vault(),
        HOY,
        &Topic::Ahorro {
            id: "no_existe".into(),
            year: 2026,
        },
    )
    .unwrap_err();
    assert_eq!(e.to_string(), "recomendación desconocida: no_existe");
    let e = conversaciones::open(
        &env.vault(),
        HOY,
        &Topic::Ahorro {
            id: "capitalizacion".into(),
            year: 2025,
        },
    )
    .unwrap_err();
    assert!(e.to_string().contains("es del ejercicio 2025"), "{e}");
    assert!(env.requests().is_empty());
}

#[tokio::test]
async fn la_primera_pregunta_crea_la_conversacion_y_la_siguiente_la_sigue() {
    let env = Env::new(&[
        "Sí: con tus datos te ahorras unos 2.520,00 € si no repartes el beneficio.",
        "Entonces la rebaja baja a la mitad.",
    ])
    .await;
    let t = capitalizacion();
    let o = env
        .ask(&t, None, "  ¿Me conviene dejar el beneficio?  ")
        .await
        .unwrap();
    let c = o.conversation.unwrap();
    assert_eq!(
        c.title,
        "Reserva de capitalización: deja el beneficio en la empresa"
    );
    assert_eq!(c.messages.len(), 2);
    assert_eq!(
        (c.messages[0].role, c.messages[0].content.as_str()),
        (Role::User, "¿Me conviene dejar el beneficio?")
    );
    assert_eq!(c.messages[1].role, Role::Assistant);
    assert!(c.messages[1].content.starts_with("Sí: con tus datos"));
    let notes = c.messages[1].notes.clone().unwrap();
    assert_eq!(notes.model, "falso");
    assert!(notes.used.is_empty());
    assert!(c.messages[0].notes.is_none());
    assert!(!c.full && c.materials.is_empty());
    // Lo que se envió: el tema en las instrucciones, con sus reglas, y sólo la pregunta.
    let reqs = env.requests();
    assert_eq!(reqs.len(), 1);
    assert!(reqs[0].contains("### Tema de esta conversación"));
    assert!(reqs[0].contains("Esta conversación es sobre un tema concreto de TaxPain"));
    assert!(reqs[0].contains("BOE-A-2014-12328, artículo 25"));
    assert!(reqs[0].contains("¿Me conviene dejar el beneficio?"));
    assert!(
        !reqs[0].contains("Pregunta lo que quieras"),
        "la presentación no se envía"
    );

    // La siguiente sigue la misma conversación y lleva la anterior.
    let o = env
        .ask(&t, Some(c.id), "¿Y si reparto la mitad?")
        .await
        .unwrap();
    let c2 = o.conversation.unwrap();
    assert_eq!(c2.id, c.id);
    assert_eq!(c2.messages.len(), 4);
    assert_eq!(c2.messages[3].seq, 4);
    let reqs = env.requests();
    assert!(reqs[1].contains("¿Me conviene dejar el beneficio?"));
    assert!(reqs[1].contains("si no repartes el beneficio."));
    assert!(reqs[1].contains("¿Y si reparto la mitad?"));
    // Sin id, la del tema también sigue la misma.
    assert_eq!(conversaciones::list(&env.vault()).unwrap().len(), 1);

    // En la auditoría, antes de enviar: el modelo, el tipo de tema y cuántos datos, sin texto.
    let chats = env.chats();
    assert_eq!(chats.len(), 2);
    assert_eq!(chats[0]["tema"], "ahorro");
    assert_eq!(chats[0]["datos"], 0);
    assert_eq!(chats[0]["documentos"], 0);
    assert_eq!(chats[0]["remote"], false);
    assert!(!chats[0].to_string().contains("beneficio"));
    let resumen = &conversaciones::list(&env.vault()).unwrap()[0];
    assert_eq!((resumen.questions, resumen.materials), (2, 0));
    assert_eq!(resumen.topic, t);
}

#[tokio::test]
async fn una_general_toma_el_titulo_de_su_primera_pregunta() {
    let env = Env::new(&["Este trimestre, 410,00 €.", "Hasta el 20 de enero."]).await;
    let o = env
        .ask(
            &Topic::General,
            None,
            "¿Cuánto IVA me toca pagar este trimestre y por qué me sale tanto más que el anterior?",
        )
        .await
        .unwrap();
    let c = o.conversation.unwrap();
    assert_eq!(
        c.title,
        "¿Cuánto IVA me toca pagar este trimestre y por qué me sale…"
    );
    assert_eq!(o.header.title, c.title);
    assert_eq!(o.header.group, "Asistente");
    let reqs = env.requests();
    assert!(!reqs[0].contains("### Tema de esta conversación"));
    // Sin id, otra general es otra conversación.
    env.ask(&Topic::General, None, "¿Y el plazo?")
        .await
        .unwrap();
    assert_eq!(conversaciones::list(&env.vault()).unwrap().len(), 2);
}

#[tokio::test]
async fn reabrir_por_tema_y_por_id_da_lo_mismo() {
    let env = Env::new(&["Te conviene."]).await;
    let t = capitalizacion();
    let id = env
        .ask(&t, None, "¿Me conviene?")
        .await
        .unwrap()
        .conversation
        .unwrap()
        .id;
    let por_tema = conversaciones::open(&env.vault(), HOY, &t).unwrap();
    let por_id = conversaciones::open_by_id(&env.vault(), HOY, id).unwrap();
    assert_eq!(por_tema, por_id);
    assert_eq!(por_tema.conversation.unwrap().messages.len(), 2);
    assert!(!por_tema.changed);
    let e = conversaciones::open_by_id(&env.vault(), HOY, id + 1).unwrap_err();
    assert_eq!(
        e.to_string(),
        "Esta conversación ya no existe: se ha borrado."
    );
}

#[tokio::test]
async fn la_conversacion_no_es_de_este_tema() {
    let env = Env::new(&["Te conviene."]).await;
    let id = env
        .ask(&capitalizacion(), None, "¿Me conviene?")
        .await
        .unwrap()
        .conversation
        .unwrap()
        .id;
    let plan = Topic::AhorroPlan { year: 2026 };
    let e = env
        .ask(&plan, Some(id), "¿Por cuál empiezo?")
        .await
        .unwrap_err();
    assert_eq!(e.to_string(), "La conversación no es de este tema.");
    let e = conversaciones::preview(|| Ok(env.vault()), HOY, &Topic::General, Some(id), None)
        .unwrap_err();
    assert_eq!(e.to_string(), "La conversación no es de este tema.");
    assert_eq!(env.requests().len(), 1);
}

#[tokio::test]
async fn ver_que_se_envia_sin_preguntar() {
    let env = Env::new(&[]).await;
    let out =
        conversaciones::preview(|| Ok(env.vault()), HOY, &capitalizacion(), None, None).unwrap();
    assert!(out.system.contains("### Tema de esta conversación"));
    assert_eq!(out.messages.len(), 1);
    assert_eq!(out.messages[0].content, "(aquí irá tu pregunta)");
    assert_eq!(out.omitted, 0);
    // El nombre y el NIF de la empresa no salen.
    assert!(!out.system.contains("CLIENTE UNO"));
    let out = conversaciones::preview(
        || Ok(env.vault()),
        HOY,
        &capitalizacion(),
        None,
        Some("¿Y para CLIENTE UNO SL?"),
    )
    .unwrap();
    assert!(
        !out.messages[0].content.contains("CLIENTE UNO"),
        "{:?}",
        out.messages
    );
    assert!(out.replaced >= 1);
    let e = conversaciones::preview(
        || Ok(env.vault()),
        HOY,
        &capitalizacion(),
        None,
        Some(&"x".repeat(4_001)),
    )
    .unwrap_err();
    assert_eq!(
        e.to_string(),
        "Tu pregunta es demasiado larga: como mucho 4.000 caracteres."
    );
    // Nada se guarda ni se envía.
    assert_eq!(env.rows("assistant_conversation"), 0);
    assert!(env.requests().is_empty() && env.chats().is_empty());
}

#[tokio::test]
async fn sin_consentimiento_no_sale_ni_se_guarda() {
    let env = Env::new(&["no debería llegar"]).await;
    assistant::save_settings(
        &env.vault(),
        &AssistantSettings {
            provider: Provider::Anthropic {
                model: "claude-opus-5-5".into(),
            },
            consent: false,
        },
        AHORA,
    )
    .unwrap();
    let e = env
        .ask(&capitalizacion(), None, "¿Me conviene?")
        .await
        .unwrap_err();
    assert!(e.to_string().contains("acepta qué datos se envían"), "{e}");
    assert!(env.requests().is_empty());
    assert_eq!(env.rows("assistant_conversation"), 0);
    assert_eq!(env.rows("assistant_message"), 0);
    assert!(env.chats().is_empty());
}

#[tokio::test]
async fn sin_clave_no_queda_en_la_auditoria() {
    let env = Env::new(&["no debería llegar"]).await;
    // Dos veces: lo aceptado vale para el host ya guardado, así que con un proveedor nuevo la
    // primera guarda el proveedor y la segunda, la aceptación.
    for _ in 0..2 {
        assistant::save_settings(
            &env.vault(),
            &AssistantSettings {
                provider: Provider::Anthropic {
                    model: "claude-opus-5-5".into(),
                },
                consent: true,
            },
            AHORA,
        )
        .unwrap();
    }
    let e = env
        .ask(&capitalizacion(), None, "¿Me conviene?")
        .await
        .unwrap_err();
    assert_eq!(e.to_string(), "falta la clave de API del proveedor");
    assert!(env.chats().is_empty());
    assert_eq!(env.rows("assistant_conversation"), 0);
    let e = env.ask(&capitalizacion(), None, "   ").await.unwrap_err();
    assert_eq!(e.to_string(), "Escribe tu pregunta.");
}

/// Mete `n` preguntas con su respuesta en la conversación `id` sin pasar por el proveedor.
fn rellenar(v: &Vault, id: i64, n: usize) {
    let max: i64 = v
        .connection()
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM assistant_message WHERE conversation_id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    for i in 0..n {
        let seq = max + i64::try_from(i).unwrap() * 2;
        for (s, role) in [(seq + 1, "user"), (seq + 2, "assistant")] {
            v.connection()
                .execute(
                    "INSERT INTO assistant_message(conversation_id, seq, role, content, created_at)
                     VALUES (?1, ?2, ?3, 'Pregunta o respuesta de relleno.', '2026-10-30T09:00:00Z')",
                    taxpain_vault::rusqlite::params![id, s, role],
                )
                .unwrap();
        }
    }
}

#[tokio::test]
async fn cien_preguntas_llenan_la_conversacion() {
    let env = Env::new(&["Primera.", "La número cien."]).await;
    let t = capitalizacion();
    let id = env
        .ask(&t, None, "¿Me conviene?")
        .await
        .unwrap()
        .conversation
        .unwrap()
        .id;
    // Con 99 preguntas aún se puede: va sólo lo último del historial.
    rellenar(&env.vault(), id, MAX_QUESTIONS - 2);
    let out =
        conversaciones::preview(|| Ok(env.vault()), HOY, &t, Some(id), Some("¿Y ahora?")).unwrap();
    assert_eq!(out.messages.len(), 31);
    assert_eq!(out.omitted, 2 * (MAX_QUESTIONS - 1) - 30);
    assert!(
        out.system
            .contains("no van sus 168 intervenciones más antiguas")
    );
    let o = env.ask(&t, Some(id), "¿Y ahora?").await.unwrap();
    let c = o.conversation.unwrap();
    assert!(c.full);
    assert_eq!(c.messages.len(), 2 * MAX_QUESTIONS);
    let e = env.ask(&t, Some(id), "¿Una más?").await.unwrap_err();
    assert!(
        e.to_string()
            .starts_with("Esta conversación ha llegado a 100 preguntas")
    );
    assert_eq!(env.requests().len(), 2);
    assert!(
        conversaciones::open(&env.vault(), HOY, &t)
            .unwrap()
            .conversation
            .unwrap()
            .full
    );
}

#[tokio::test]
async fn cien_conversaciones_no_dejan_empezar_otra() {
    let env = Env::new(&["Hola."]).await;
    for _ in 0..MAX_CONVERSATIONS {
        env.vault()
            .connection()
            .execute(
                "INSERT INTO assistant_conversation(kind, topic_key, topic, title, created_at,
                   updated_at) VALUES ('general', '', '{\"kind\":\"general\"}', 'Vieja',
                   '2026-10-01T09:00:00Z', '2026-10-01T09:00:00Z')",
                [],
            )
            .unwrap();
    }
    let e = env
        .ask(&capitalizacion(), None, "¿Me conviene?")
        .await
        .unwrap_err();
    assert!(
        e.to_string()
            .starts_with("Ya tienes 100 conversaciones guardadas"),
        "{e}"
    );
    let e = env.ask(&Topic::General, None, "Hola").await.unwrap_err();
    assert!(
        e.to_string()
            .starts_with("Ya tienes 100 conversaciones guardadas"),
        "{e}"
    );
    // No se ha pagado ninguna respuesta que luego no se pudiera guardar.
    assert!(env.requests().is_empty() && env.chats().is_empty());
    // Una que ya existe sí se puede seguir.
    let id = conversaciones::list(&env.vault()).unwrap()[0].id;
    env.ask(&Topic::General, Some(id), "Hola").await.unwrap();
    assert_eq!(env.requests().len(), 1);
}

#[tokio::test]
async fn borrar_una_y_todas() {
    let env = Env::new(&["Una.", "Dos."]).await;
    let a = env
        .ask(&capitalizacion(), None, "¿Me conviene?")
        .await
        .unwrap()
        .conversation
        .unwrap()
        .id;
    env.ask(&Topic::General, None, "¿Cuándo presento el 303?")
        .await
        .unwrap();
    conversaciones::delete(&env.vault(), a).unwrap();
    assert_eq!(conversaciones::list(&env.vault()).unwrap().len(), 1);
    assert_eq!(env.rows("assistant_message"), 2);
    let e = conversaciones::delete(&env.vault(), a).unwrap_err();
    assert_eq!(
        e.to_string(),
        "Esta conversación ya no existe: se ha borrado."
    );
    // Al borrarla, el tema vuelve a empezar.
    let o = conversaciones::open(&env.vault(), HOY, &capitalizacion()).unwrap();
    assert!(o.conversation.is_none());
    assert_eq!(conversaciones::delete_all(&env.vault()).unwrap(), 1);
    assert_eq!(env.rows("assistant_conversation"), 0);
    assert_eq!(env.rows("assistant_message"), 0);
    assert_eq!(conversaciones::delete_all(&env.vault()).unwrap(), 0);
}

#[tokio::test]
async fn si_cambian_los_datos_del_tema_se_avisa() {
    let env = Env::new(&["Te conviene."]).await;
    let t = capitalizacion();
    env.ask(&t, None, "¿Me conviene?").await.unwrap();
    assert!(!conversaciones::open(&env.vault(), HOY, &t).unwrap().changed);
    let m = Marca {
        estado: Estado::EnMarcha,
        pasos: [0].into_iter().collect(),
    };
    marcar(&env.vault(), HOY, "capitalizacion", &m).unwrap();
    let o = conversaciones::open(&env.vault(), HOY, &t).unwrap();
    assert!(o.changed);
    assert!(
        o.header
            .facts
            .contains(&("Estado".into(), "En marcha".into()))
    );
    // El plan también cambia, pero no tiene conversación: sin aviso.
    assert!(
        !conversaciones::open(&env.vault(), HOY, &Topic::AhorroPlan { year: 2026 })
            .unwrap()
            .changed
    );
}

#[tokio::test]
async fn conversacion_de_otro_ejercicio_usa_la_foto() {
    let env = Env::new(&["Te conviene.", "Sigue valiendo lo de entonces."]).await;
    let t = capitalizacion();
    env.ask_at(
        &t,
        None,
        "¿Me conviene?",
        date!(2026 - 12 - 15),
        datetime!(2026-12-15 10:00 UTC),
    )
    .await
    .unwrap();
    let enero = date!(2027 - 01 - 10);
    let o = conversaciones::open(&env.vault(), enero, &t).unwrap();
    assert_eq!(
        o.header.notice.as_deref(),
        Some(
            "Esta conversación es del ejercicio 2026: la IA verá los datos del tema de tu última \
             pregunta (15/12/2026) y lo que has añadido."
        )
    );
    assert_eq!(
        o.header.title,
        "Reserva de capitalización: deja el beneficio en la empresa"
    );
    assert!(!o.changed);
    let o = env
        .ask_at(
            &t,
            None,
            "¿Y ahora?",
            enero,
            datetime!(2027-01-10 10:00 UTC),
        )
        .await
        .unwrap();
    assert_eq!(o.conversation.unwrap().messages.len(), 4);
    let reqs = env.requests();
    assert!(reqs[1].contains("para el ejercicio 2026: «Reserva de capitalización"));
    // La foto sigue siendo la de la pregunta de 2026.
    let o = conversaciones::open(&env.vault(), enero, &t).unwrap();
    assert!(o.header.notice.unwrap().contains("(15/12/2026)"));
}

#[tokio::test]
async fn la_respuesta_se_guarda_donde_dice_keep() {
    let env = Env::new(&["Te conviene."]).await;
    let otra = Mutex::new(empresa());
    let t = capitalizacion();
    let req = AskRequest {
        topic: &t,
        conversation: None,
        question: "¿Me conviene?",
        hoy: HOY,
    };
    let o = conversaciones::ask(
        || Ok(env.vault()),
        || Ok(otra.lock().unwrap()),
        &Client::new().unwrap(),
        &Tools(&env.v),
        &req,
        || AHORA,
    )
    .await
    .unwrap();
    assert_eq!(o.conversation.unwrap().messages.len(), 2);
    // En la que se preguntó queda la auditoría; la respuesta, en la que dice `keep`.
    assert_eq!(env.rows("assistant_conversation"), 0);
    assert_eq!(env.chats().len(), 1);
    assert_eq!(
        conversaciones::list(&otra.lock().unwrap()).unwrap().len(),
        1
    );
}

#[tokio::test]
async fn datos_y_documentos_con_sus_limites() {
    let env = Env::new(&["Con lo que has añadido, sí.", "Unos 1.200,00 € al año."]).await;
    let t = capitalizacion();
    // Añadir crea la conversación del tema (sin preguntar todavía).
    let c = conversaciones::add_fact(
        &env.vault(),
        AHORA,
        &t,
        None,
        "  En 2025 dejamos 30.000 € del beneficio en reservas voluntarias.  ",
    )
    .unwrap();
    assert_eq!(
        c.title,
        "Reserva de capitalización: deja el beneficio en la empresa"
    );
    assert!(c.messages.is_empty());
    assert_eq!(c.materials.len(), 1);
    let m = &c.materials[0];
    assert_eq!(m.kind, MaterialKind::Dato);
    assert_eq!(
        m.content,
        "En 2025 dejamos 30.000 € del beneficio en reservas voluntarias."
    );
    assert_eq!((m.filename.as_deref(), m.read_by), (None, None));
    let doc = conversaciones::read_document(
        "certificado-aportaciones.txt",
        "CERTIFICADO DE APORTACIONES 2026\nPartícipe: Lucía Villalobos Cuesta\nAportado: \
         1.500,00 €"
            .as_bytes(),
    )
    .unwrap();
    let c = conversaciones::add_document(&env.vault(), AHORA, &t, Some(c.id), &doc, ReadBy::Mac)
        .unwrap();
    assert_eq!(c.materials.len(), 2);
    assert_eq!(c.materials[1].kind, MaterialKind::Documento);
    assert_eq!(
        c.materials[1].filename.as_deref(),
        Some("certificado-aportaciones.txt")
    );
    assert_eq!(c.materials[1].read_by, Some(ReadBy::Mac));
    let e =
        conversaciones::add_document(&env.vault(), AHORA, &t, None, &doc, ReadBy::Mac).unwrap_err();
    assert_eq!(e.to_string(), "Ese documento ya está en esta conversación.");

    // Lo añadido va con la pregunta, al principio y sin el nombre del fichero.
    let o = env.ask(&t, Some(c.id), "¿Me conviene?").await.unwrap();
    assert_eq!(o.conversation.unwrap().messages.len(), 2);
    let reqs = env.requests();
    assert!(
        reqs[0]
            .contains("Datos que he añadido a esta conversación (TaxPain no los ha comprobado):")
    );
    assert!(reqs[0].contains("30.000 € del beneficio en reservas voluntarias"));
    assert!(
        reqs[0].contains("<<<DOCUMENTO EXTERNO: documento 2 que he añadido, leído en el Mac>>>")
    );
    assert!(reqs[0].contains("Mi pregunta: ¿Me conviene?"));
    assert!(
        !reqs[0].contains("certificado-aportaciones"),
        "el nombre del fichero no sale"
    );
    assert!(
        !reqs[0].contains("Villalobos"),
        "el nombre del documento se tapa"
    );
    let chats = env.chats();
    assert_eq!(
        (chats[0]["datos"].clone(), chats[0]["documentos"].clone()),
        (1.into(), 1.into())
    );

    // Quitar: deja de ir desde la próxima pregunta.
    let c = conversaciones::remove_material(&env.vault(), c.id, c.materials[1].id).unwrap();
    assert_eq!(c.materials.len(), 1);
    let e = conversaciones::remove_material(&env.vault(), c.id, 999).unwrap_err();
    assert_eq!(e.to_string(), "Eso ya no está en la conversación.");
    let out = conversaciones::preview(|| Ok(env.vault()), HOY, &t, Some(c.id), None).unwrap();
    assert!(
        !out.messages
            .last()
            .unwrap()
            .content
            .contains("DOCUMENTO EXTERNO")
    );

    // Límites: vacío, largo, un secreto, 20 cosas y 30.000 caracteres entre todas.
    let g = {
        let v = env.vault();
        let add = |text: &str| conversaciones::add_fact(&v, AHORA, &t, None, text);
        assert_eq!(add("   ").unwrap_err().to_string(), "Escribe el dato.");
        assert_eq!(
            add(&"x".repeat(2_001)).unwrap_err().to_string(),
            "El dato es demasiado largo: como mucho 2.000 caracteres."
        );
        assert_eq!(
            add("-----BEGIN PRIVATE KEY-----\nMIIE")
                .unwrap_err()
                .to_string(),
            "No se añade: contiene un bloque PEM (clave o certificado)."
        );
        for i in 0..19 {
            add(&format!("Dato número {i}.")).unwrap();
        }
        assert_eq!(
            add("Uno más.").unwrap_err().to_string(),
            "Ya hay 20 datos y documentos en esta conversación: quita alguno para añadir otro."
        );
        let plan = Topic::AhorroPlan { year: 2026 };
        let largo = |n: char| conversaciones::DocumentReading {
            filename: format!("{n}.txt"),
            sha256: n.to_string().repeat(64),
            size: MAX_DOCUMENT_CHARS,
            text: "palabra ".repeat(MAX_DOCUMENT_CHARS / 8),
            truncated: false,
        };
        conversaciones::add_document(&v, AHORA, &plan, None, &largo('a'), ReadBy::Mac).unwrap();
        conversaciones::add_document(&v, AHORA, &plan, None, &largo('b'), ReadBy::Mac).unwrap();
        assert_eq!(
            conversaciones::add_fact(&v, AHORA, &plan, None, "Un dato más.")
                .unwrap_err()
                .to_string(),
            "Entre todo lo que has añadido no caben más de 30.000 caracteres: quita algo."
        );
        // Una general nueva también se crea al añadir algo, con su título por defecto, que cambia
        // por el de su primera pregunta.
        let g =
            conversaciones::add_fact(&v, AHORA, &Topic::General, None, "Tenemos 3 trabajadores.")
                .unwrap();
        assert_eq!(g.title, "Conversación del 30/10/2026");
        g
    };
    let o = env
        .ask(&Topic::General, Some(g.id), "¿Cuánto me cuesta la nómina?")
        .await
        .unwrap();
    assert_eq!(
        o.conversation.unwrap().title,
        "¿Cuánto me cuesta la nómina?"
    );
}

#[test]
fn leer_documentos_en_el_mac() {
    use taxpain_invoicing::pdf::{Para, text_document};

    // Un PDF con texto: sin pasar por Vision.
    taxpain_app::carpeta::set_ocr(Box::new(|name: &str, _bytes: &[u8]| {
        // Lo que no es una imagen ni un PDF, Vision no lo lee.
        if name.starts_with("renombrado") {
            return None;
        }
        Some(format!(
            "Texto leído con Vision de {name}: TICKET 0042 · Gasolinera Ejemplo · Total 60,00 €"
        ))
    }));
    let pdf = text_document(
        "Certificado",
        &[vec![Para::Text(
            "CERTIFICADO DE APORTACIONES 2026. Plan de Pensiones Individual Ejemplo. Aportado del \
             01/01/2026 al 30/09/2026: 1.500,00 €."
                .into(),
        )]],
    )
    .unwrap();
    let d = conversaciones::read_document("certificado.pdf", &pdf).unwrap();
    assert!(
        d.text.contains("CERTIFICADO DE APORTACIONES 2026"),
        "{}",
        d.text
    );
    assert!(!d.text.contains("Vision"));
    assert_eq!((d.size, d.truncated), (pdf.len(), false));
    assert_eq!(d.sha256.len(), 64);
    // Una foto (y un PDF escaneado, sin texto): con el lector de texto del Mac.
    let foto = [&[0xFF, 0xD8, 0xFF, 0xE0][..], &[0u8; 64]].concat();
    let d = conversaciones::read_document("ticket.jpg", &foto).unwrap();
    assert!(d.text.starts_with("Texto leído con Vision de ticket.jpg"));
    let escaneado = text_document("Escaneado", &[vec![Para::Text(String::new())]]).unwrap();
    let d = conversaciones::read_document("escaneado.pdf", &escaneado).unwrap();
    assert!(d.text.contains("Vision de escaneado.pdf"), "{}", d.text);

    // Un Excel con dos hojas, un CSV y un Markdown.
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet().set_name("Aportaciones").unwrap();
    ws.write_string(0, 0, "Fecha").unwrap();
    ws.write_string(0, 1, "Importe").unwrap();
    ws.write_string(1, 0, "15/03/2026").unwrap();
    ws.write_number(1, 1, 1500.0).unwrap();
    let ws = wb.add_worksheet().set_name("Notas").unwrap();
    ws.write_string(0, 0, "Plan individual del banco del cliente")
        .unwrap();
    let xlsx = wb.save_to_buffer().unwrap();
    let d = conversaciones::read_document("aportaciones.xlsx", &xlsx).unwrap();
    assert!(
        d.text.starts_with(
            "Hoja «Aportaciones»:\nfila 1: A=Fecha | B=Importe\nfila 2: A=15/03/2026 | B=1500"
        ),
        "{}",
        d.text
    );
    assert!(d.text.contains("Hoja «Notas»:\nfila 1: A=Plan individual"));
    let d = conversaciones::read_document(
        "movimientos.csv",
        b"Fecha;Concepto;Importe\n01/02/2026;Aportacion plan;1.500,00\n",
    )
    .unwrap();
    assert!(
        d.text
            .contains("A=01/02/2026 | B=Aportacion plan | C=1.500,00"),
        "{}",
        d.text
    );
    let d = conversaciones::read_document(
        "notas.md",
        "# Notas\n\n\n\n\nEn 2025 dejamos 30.000 € en reservas voluntarias.   \n".as_bytes(),
    )
    .unwrap();
    assert_eq!(
        d.text,
        "# Notas\n\nEn 2025 dejamos 30.000 € en reservas voluntarias."
    );
    // Uno largo se corta (y lo dice).
    let d =
        conversaciones::read_document("largo.txt", "palabra ".repeat(3_000).as_bytes()).unwrap();
    assert!(d.truncated);
    assert_eq!(d.text.chars().count(), MAX_DOCUMENT_CHARS);

    // Lo que no se lee.
    let e = |name: &str, bytes: &[u8]| {
        conversaciones::read_document(name, bytes)
            .unwrap_err()
            .to_string()
    };
    assert_eq!(
        e("certificado.p12", b"\x30\x82\x05"),
        "Un certificado o una clave no se añade nunca a una conversación."
    );
    assert_eq!(
        e("clave.PEM", b"-----BEGIN PRIVATE KEY-----"),
        "Un certificado o una clave no se añade nunca a una conversación."
    );
    assert_eq!(
        e("contrato.docx", b"PK\x03\x04"),
        "Ese tipo de fichero no se lee: usa PDF, una foto, Excel, CSV o texto."
    );
    // Un «.pdf» que no lo es: se intenta leer como PDF y se dice que no se ha podido.
    assert!(e("renombrado.pdf", b"PK\x03\x04 no soy un PDF").starts_with("No se ha podido leer"));
    assert_eq!(
        e("corto.txt", b"Hola."),
        "No se ha podido leer texto de «corto.txt» en tu Mac."
    );
    assert_eq!(
        e(
            "pegado.txt",
            b"Mi clave: -----BEGIN PRIVATE KEY----- MIIEvQIBADANBg"
        ),
        "No se añade: contiene un bloque PEM (clave o certificado)."
    );
    assert_eq!(
        e(
            "enorme.txt",
            &vec![b'a'; conversaciones::MAX_FILE_BYTES + 1]
        ),
        "el fichero pasa de 20 MB: elige otro"
    );
}

// ---------- Hallazgos de «Qué revisar» y de la revisión de la carpeta ----------

/// El hallazgo que da «Qué revisar» a una empresa sin perfil fiscal (sin ejercicio).
fn sin_perfil() -> Topic {
    Topic::Hallazgo {
        origin: Origen::Aeat,
        code: "modelos.sin_perfil".into(),
        year: None,
        title: "Sin perfil fiscal: no se sabe qué modelos tocaban".into(),
    }
}

#[tokio::test]
async fn un_hallazgo_de_que_revisar_se_pregunta_y_sigue_con_su_foto() {
    let env = Env::new(&[
        "Es para saber: sin perfil no se sabe qué modelos tocaban.",
        "Sigue siendo lo mismo.",
    ])
    .await;
    let t = sin_perfil();
    let o = conversaciones::open(&env.vault(), HOY, &t).unwrap();
    assert_eq!(
        (o.header.group.as_str(), o.header.title.as_str()),
        (
            "Qué revisar",
            "Sin perfil fiscal: no se sabe qué modelos tocaban"
        )
    );
    assert!(
        o.header
            .intro
            .starts_with("Al comparar lo que tiene la AEAT con lo que sabe TaxPain sale esto:")
    );
    assert_eq!(
        o.header.suggestions,
        ["¿Qué significa y es grave?", "¿Qué le pido a mi gestoría?"]
    );
    assert!(o.conversation.is_none());
    assert_eq!(env.rows("assistant_conversation"), 0);

    let c = env
        .ask(&t, None, "¿Qué hago con esto?")
        .await
        .unwrap()
        .conversation
        .unwrap();
    assert_eq!(c.title, "Sin perfil fiscal: no se sabe qué modelos tocaban");
    assert_eq!(c.topic, t);
    let reqs = env.requests();
    assert!(reqs[0].contains("### Tema de esta conversación"));
    assert!(reqs[0].contains(
        "Hallazgo de «Qué revisar» de TaxPain (lo que tiene la AEAT frente a lo que sabe \
         TaxPain): «Sin perfil fiscal: no se sabe qué modelos tocaban»."
    ));
    assert!(reqs[0].contains("Configura el perfil fiscal"));
    assert_eq!(env.chats()[0]["tema"], "hallazgo");

    // Con el perfil puesto, el hallazgo ya no sale: se sigue con la foto de la última pregunta.
    env.vault()
        .set_setting(
            "taxpayer_profile",
            &taxpain_core::calendar::TaxpayerProfile::sociedad_limitada(),
        )
        .unwrap();
    let o = conversaciones::open(&env.vault(), HOY, &t).unwrap();
    assert_eq!(
        o.header.notice.as_deref(),
        Some(
            "Este hallazgo ya no sale en la última revisión: la IA lo verá como estaba en tu \
             última pregunta (30/10/2026) y lo que has añadido."
        )
    );
    assert_eq!(o.conversation.as_ref().map(|c| c.id), Some(c.id));
    assert!(!o.changed);
    env.ask(&t, Some(c.id), "¿Y ahora?").await.unwrap();
    assert!(env.requests()[1].contains("«Sin perfil fiscal: no se sabe qué modelos tocaban»"));

    // Uno que no sale y no tiene conversación no se abre.
    let otro = Topic::Hallazgo {
        origin: Origen::Aeat,
        code: "modelos.sin_perfil".into(),
        year: None,
        title: "Otro título".into(),
    };
    assert_eq!(
        conversaciones::open(&env.vault(), HOY, &otro)
            .unwrap_err()
            .to_string(),
        "Este hallazgo ya no sale en «Qué revisar»: vuelve a abrir Evolución para verlo con tus \
         datos de ahora."
    );
}

/// Una revisión de la carpeta con un hallazgo que cita sus ficheros (con el nombre de un
/// proveedor en la ruta).
fn revision_carpeta(detalle: &str) -> Revision {
    Revision {
        generada: "2026-10-29".into(),
        empresa: "CLIENTE UNO SL".into(),
        nif: Some("B12345674".into()),
        carpeta: "/Users/prueba/Gestoría".into(),
        resumen: Resumen {
            ficheros: 2,
            con_detalle: 2,
            sin_leer: 0,
            no_admitidos: 0,
            por_tipo: vec![],
            errores: 0,
            avisos: 1,
            info: 0,
            urgentes: 0,
            en_juego_urgente: Money::ZERO,
            recuperable: Money::ZERO,
            a_favor_proximo_200: Money::ZERO,
            retenciones_proximo_200: Money::ZERO,
            exencion_proximo_200: Money::ZERO,
            impuesto_dividendos_proximo_200: Money::ZERO,
            pendiente_cobro: Money::ZERO,
        },
        hallazgos: vec![Hallazgo {
            codigo: "facturas.nif_control".into(),
            area: Area::Facturas,
            gravedad: Gravedad::Aviso,
            responsable: Responsable::Empresa,
            ejercicio: Some(2026),
            titulo: "1 NIF con la letra de control mal".into(),
            detalle: detalle.into(),
            ficheros: vec!["2026/Recibidas/Imprenta Villalobos 2026-06.pdf".into()],
            importe: None,
            rectificable_hasta: None,
            rectificable_aprox: false,
            terceros: Some(vec![]),
            urgente: None,
        }],
        tablas: vec![],
        pedir: vec![],
        devoluciones_pendientes: vec![],
        dividendos: vec![],
    }
}

fn nif_mal() -> Topic {
    Topic::Hallazgo {
        origin: Origen::Carpeta,
        code: "facturas.nif_control".into(),
        year: Some(2026),
        title: "1 NIF con la letra de control mal".into(),
    }
}

#[tokio::test]
async fn un_hallazgo_de_la_carpeta_va_sin_rutas_y_avisa_si_cambia() {
    let env = Env::new(&["Pide al emisor una factura corregida."]).await;
    let t = nif_mal();
    // Sin una revisión de la carpeta guardada, no hay hallazgo.
    assert_eq!(
        conversaciones::open(&env.vault(), HOY, &t)
            .unwrap_err()
            .to_string(),
        "Este hallazgo ya no sale en la última revisión de la carpeta: vuelve a revisarla en \
         Importar → Revisión."
    );
    revision::guardar_ultima(
        &env.vault(),
        &revision_carpeta(
            "La factura 2026/Recibidas/Imprenta Villalobos 2026-06.pdf lleva un NIF que no es \
             válido. Pide al emisor una factura corregida.",
        ),
    )
    .unwrap();
    let o = conversaciones::open(&env.vault(), HOY, &t).unwrap();
    assert_eq!(o.header.group, "Revisión de la carpeta · 2026");
    // El usuario ve la ruta (no se envía).
    assert!(o.header.intro.contains("Imprenta Villalobos 2026-06.pdf"));
    env.ask(&t, None, "¿Es grave?").await.unwrap();
    let r = &env.requests()[0];
    assert!(r.contains("<<<DOCUMENTO EXTERNO: detalle del hallazgo>>>"));
    assert!(r.contains("La factura fichero 1 lleva un NIF que no es válido."));
    assert!(r.contains("1 fichero afectado"));
    for fuera in ["Villalobos", "Recibidas", "2026-06.pdf", "/Users/prueba"] {
        assert!(!r.contains(fuera), "llega «{fuera}» al proveedor");
    }
    // Una revisión nueva con otro detalle: avisa de que ha cambiado.
    revision::guardar_ultima(
        &env.vault(),
        &revision_carpeta("Lleva un NIF que no es válido: pide una factura corregida."),
    )
    .unwrap();
    assert!(conversaciones::open(&env.vault(), HOY, &t).unwrap().changed);
}

// ---------- Borradores de modelos ----------

fn m303() -> Topic {
    Topic::Modelo {
        model: "303".into(),
        year: 2026,
        period: 3,
    }
}

#[tokio::test]
async fn un_borrador_se_pregunta_con_sus_casillas_y_sin_su_fichero() {
    let env = Env::new(&[
        "Te sale a cero porque no hay facturas.",
        "Cuenta lo presentado.",
    ])
    .await;
    let t = m303();
    let o = conversaciones::open(&env.vault(), HOY, &t).unwrap();
    assert_eq!(
        (o.header.group.as_str(), o.header.title.as_str()),
        ("Modelos · 2026", "Borrador del modelo 303 · 3T 2026")
    );
    assert_eq!(o.header.facts[0].0, "Resultado");
    assert_eq!(o.header.facts[1], ("Periodo".into(), "3T 2026".into()));
    assert_eq!(o.header.suggestions.len(), 3);
    assert!(o.conversation.is_none());

    let c = env
        .ask(&t, None, "¿Por qué me sale así?")
        .await
        .unwrap()
        .conversation
        .unwrap();
    assert_eq!(c.title, "Borrador del modelo 303 · 3T 2026");
    let d = taxpain_app::models::draft(&env.vault(), "303", 2026, 3).unwrap();
    let r = &env.requests()[0];
    assert!(r.contains("Borrador del modelo 303 de TaxPain del 3T 2026"));
    assert!(r.contains("- Resultado: "));
    // Ni el fichero ni su nombre (llevan el NIF y el nombre de la empresa).
    assert!(!d.file_name.is_empty() && !r.contains(&d.file_name));
    assert!(!r.contains("B12345674") && !r.contains("CLIENTE UNO"));
    assert_eq!(env.chats()[0]["tema"], "modelo");

    // Presentado desde TaxPain: se dice (sin su CSV) y cuenta lo presentado.
    taxpain_app::models::record_filing(
        &env.vault(),
        &d,
        true,
        &taxpain_app::models::FiledReceipt {
            csv: "ABCD1234EFGH5678".into(),
            receipt_number: "3030000000001".into(),
            ..Default::default()
        },
        AHORA,
    )
    .unwrap();
    let o = conversaciones::open(&env.vault(), HOY, &t).unwrap();
    assert!(o.changed);
    assert!(
        o.header
            .facts
            .contains(&("Presentado desde TaxPain".into(), "el 30/10/2026".into()))
    );
    env.ask(&t, Some(c.id), "¿Y ahora?").await.unwrap();
    let r = &env.requests()[1];
    assert!(r.contains("Ya presentado desde TaxPain el 30/10/2026"));
    assert!(!r.contains("ABCD1234EFGH5678") && !r.contains("3030000000001"));

    // Sin los datos de facturación ya no se calcula: sigue con la foto y lo dice.
    env.vault()
        .connection()
        .execute("DELETE FROM settings WHERE key = 'invoicing'", [])
        .unwrap();
    let o = conversaciones::open(&env.vault(), HOY, &t).unwrap();
    assert_eq!(
        o.header.notice.as_deref(),
        Some(
            "Este borrador ya no se puede calcular con tus datos de ahora: la IA verá el de tu \
             última pregunta (30/10/2026) y lo que has añadido."
        )
    );
    assert!(o.conversation.is_some());

    // Un periodo que el modelo no tiene, o un modelo que TaxPain no calcula.
    let e = |t: Topic| {
        conversaciones::open(&env.vault(), HOY, &t)
            .unwrap_err()
            .to_string()
    };
    assert_eq!(
        e(Topic::Modelo {
            model: "303".into(),
            year: 2026,
            period: 0
        }),
        "periodo no válido para el modelo 303: 0"
    );
    assert_eq!(
        e(Topic::Modelo {
            model: "999".into(),
            year: 2026,
            period: 1
        }),
        "modelo 999 aún no disponible"
    );
}

#[tokio::test]
async fn una_respuesta_con_un_certificado_de_ejemplo_no_bloquea_la_conversacion() {
    let env = Env::new(&[
        "Con openssl; el fichero empieza por -----BEGIN CERTIFICATE-----.",
        "Para firmar.",
    ])
    .await;
    let c = env
        .ask(&Topic::General, None, "¿Cómo paso el .p12 a .pem?")
        .await
        .unwrap()
        .conversation
        .unwrap();
    let o = env
        .ask(&Topic::General, Some(c.id), "¿Y para qué sirve?")
        .await
        .unwrap();
    assert_eq!(o.conversation.unwrap().messages.len(), 4);
    let r = &env.requests()[1];
    assert!(!r.contains("-----BEGIN"), "{r}");
    assert!(r.contains("no se reenvía"), "{r}");
}

/// Una factura recibida ya revisada, con su retención.
fn recibida(v: &Vault, nif: &str, nombre: &str, numero: &str, importes: (i64, i64, i64), d: Date) {
    let (base, iva, retencion) = importes;
    let rec = ingest_file(
        v,
        &format!("{numero}.xml"),
        format!("<?xml version=\"1.0\"?><x n=\"{numero}\"/>").as_bytes(),
        Source::Manual,
        AHORA,
    )
    .unwrap();
    review(
        v,
        rec.id,
        &Reviewed {
            supplier_nif: nif.into(),
            supplier_name: nombre.into(),
            number: numero.into(),
            issue_date: d,
            base: e(base),
            vat: e(iva),
            withholding: e(retencion),
            total: e(base + iva - retencion),
            deductible_pct: 100,
            capital_good: false,
            nature: Nature::General,
            asset_class: None,
            vat_regime: Default::default(),
            supplier_country: None,
            self_assessed_rate: None,
            intra_goods: false,
            cash_basis: false,
            import_dua: false,
            prorata_use: Default::default(),
        },
        AHORA,
    )
    .unwrap();
}

#[tokio::test]
async fn los_terceros_de_los_avisos_de_un_borrador_van_con_marcadores() {
    let env = Env::new(&[
        "Revisa la subclave de [DATO_1].",
        "Sigue igual.",
        "Con la foto, lo mismo.",
    ])
    .await;
    {
        // Un profesional que retiene al 7 % y otro al 1 %: los avisos del 190 los nombran (en
        // mayúsculas y con el apellido delante, como no los ve el detector de nombres).
        let v = env.vault();
        recibida(
            &v,
            "12345678Z",
            "VILLALOBOS CUESTA LUCIA",
            "P-7",
            (1_000, 210, 70),
            date!(2026 - 09 - 10),
        );
        recibida(
            &v,
            "B98765431",
            "IMPRENTA EJEMPLO SL",
            "P-1",
            (2_000, 420, 20),
            date!(2026 - 09 - 12),
        );
    }
    let d = taxpain_app::models::draft(&env.vault(), "190", 2026, 0).unwrap();
    assert!(
        d.warnings
            .iter()
            .any(|w| w.starts_with("VILLALOBOS CUESTA LUCIA (12345678Z) retiene al 7 %")),
        "{:?}",
        d.warnings
    );
    let t = Topic::Modelo {
        model: "190".into(),
        year: 2026,
        period: 0,
    };
    let fuera = [
        "VILLALOBOS",
        "Villalobos",
        "LUCIA",
        "12345678",
        "IMPRENTA",
        "Imprenta",
        "B98765431",
    ];
    let c = env
        .ask(&t, None, "¿Qué significan los avisos?")
        .await
        .unwrap()
        .conversation
        .unwrap();
    let r = &env.requests()[0];
    assert!(r.contains("retiene al 7 %"), "{r}");
    for f in fuera {
        assert!(!r.contains(f), "llega «{f}» al proveedor:\n{r}");
    }
    // En el Mac, la respuesta lleva el nombre.
    assert_eq!(
        c.messages[1].content,
        "Revisa la subclave de VILLALOBOS CUESTA LUCIA."
    );
    // La respuesta guardada vuelve con el historial, también con su marcador.
    env.ask(&t, Some(c.id), "¿Y ahora?").await.unwrap();
    let r = &env.requests()[1];
    for f in fuera {
        assert!(!r.contains(f), "llega «{f}» al proveedor:\n{r}");
    }
    // La herramienta del borrador tampoco los da.
    let tool = tool_call(
        &env.vault(),
        HOY,
        "borrador_modelo",
        &serde_json::json!({"modelo": "190", "ejercicio": 2026}),
    )
    .unwrap();
    assert!(tool.contains("Aviso: un tercero retiene al 7 %"), "{tool}");
    for f in fuera {
        assert!(!tool.contains(f), "«{f}» en la herramienta:\n{tool}");
    }
    // Con la foto de la última pregunta (el borrador ya no se calcula), igual.
    env.vault()
        .connection()
        .execute("DELETE FROM settings WHERE key = 'invoicing'", [])
        .unwrap();
    let o = conversaciones::open(&env.vault(), HOY, &t).unwrap();
    assert!(o.header.notice.is_some());
    env.ask(&t, Some(c.id), "¿Sigue igual?").await.unwrap();
    let r = &env.requests()[2];
    assert!(r.contains("retiene al 7 %"), "{r}");
    for f in fuera {
        assert!(!r.contains(f), "llega «{f}» al proveedor:\n{r}");
    }
}

#[tokio::test]
async fn el_resumen_no_saca_los_nombres_de_sus_avisos() {
    let env = Env::new(&["Mira las nóminas."]).await;
    {
        // Un 111 del trimestre con retenciones y un viaje que es retribución: su aviso nombra
        // al viajero.
        let v = env.vault();
        recibida(
            &v,
            "B98765431",
            "Asesoría Ejemplo SL",
            "A-1",
            (1_000, 210, 150),
            date!(2026 - 10 - 05),
        );
        viajes::register(
            &v,
            &Trip {
                date: date!(2026 - 10 - 15),
                employee_id: None,
                nif: "12345678Z".into(),
                name: "VIAJERO EJEMPLO LUIS".into(),
                labor: false,
                origin: "València".into(),
                destination: "Barcelona".into(),
                reason: "Visita a un cliente".into(),
                abroad: false,
                overnight: false,
                days: 1,
                km: 350,
                km_amount: Money::from_cents(9_100),
                meals: e(30),
                lodging: Money::ZERO,
                tolls_parking: Money::ZERO,
                transport: Money::ZERO,
                counter_account: "551".into(),
            },
            AHORA,
        )
        .unwrap();
    }
    let (ctx, terceros) = assistant::context_with_third_parties(&env.vault(), HOY).unwrap();
    assert!(
        ctx.contains(
            "Gastos de viaje que son retribución sujeta a retención: VIAJERO EJEMPLO LUIS"
        ),
        "{ctx}"
    );
    assert!(terceros.contains(&"VIAJERO EJEMPLO LUIS".to_owned()));
    env.ask(&Topic::General, None, "¿Qué avisos tengo?")
        .await
        .unwrap();
    let r = &env.requests()[0];
    assert!(r.contains("Gastos de viaje que son retribución"), "{r}");
    assert!(!r.contains("VIAJERO") && !r.contains("Viajero"), "{r}");
}

#[tokio::test]
async fn una_respuesta_que_llega_tarde_no_acaba_en_otra_conversacion() {
    let env = Env::new(&["El 20 de enero.", "Ésta no se guarda.", "Ni ésta."]).await;
    let c = env
        .ask(&Topic::General, None, "¿Cuándo presento el 303?")
        .await
        .unwrap()
        .conversation
        .unwrap();
    let otra_con_su_id = |v: &Vault| {
        conversaciones::delete(v, c.id).unwrap();
        v.connection()
            .execute(
                "INSERT INTO assistant_conversation(id, kind, topic_key, topic, title, \
                 created_at, updated_at) VALUES (?1, 'general', '', '{\"kind\":\"general\"}', \
                 'Otra', '2026-10-30T11:00:00Z', '2026-10-30T11:00:00Z')",
                [c.id],
            )
            .unwrap();
    };
    let req = AskRequest {
        topic: &Topic::General,
        conversation: Some(c.id),
        question: "¿Y el 130?",
        hoy: HOY,
    };
    // Mientras responde, se borra la conversación y otra acaba con su mismo id: la respuesta no
    // se guarda en ella.
    let e = conversaciones::ask(
        || Ok(env.vault()),
        || {
            let v = env.vault();
            otra_con_su_id(&v);
            Ok(v)
        },
        &Client::new().unwrap(),
        &Tools(&env.v),
        &req,
        || AHORA,
    )
    .await
    .unwrap_err();
    assert_eq!(
        e.to_string(),
        "Esta conversación se ha borrado mientras respondía: la respuesta no se ha guardado."
    );
    assert_eq!(env.rows("assistant_message"), 0);
    // Lo normal: al borrarla y empezar otra (al añadirle un dato), la nueva tiene otro id.
    let otra = conversaciones::open_by_id(&env.vault(), HOY, c.id)
        .unwrap()
        .conversation
        .unwrap();
    let req = AskRequest {
        conversation: Some(otra.id),
        ..req
    };
    let e = conversaciones::ask(
        || Ok(env.vault()),
        || {
            let v = env.vault();
            conversaciones::delete(&v, otra.id).unwrap();
            let nueva =
                conversaciones::add_fact(&v, AHORA, &Topic::General, None, "Tenemos 3 empleados.")
                    .unwrap();
            assert_ne!(nueva.id, otra.id);
            Ok(v)
        },
        &Client::new().unwrap(),
        &Tools(&env.v),
        &req,
        || AHORA,
    )
    .await
    .unwrap_err();
    assert!(
        e.to_string()
            .starts_with("Esta conversación se ha borrado mientras respondía")
    );
    assert_eq!(env.rows("assistant_message"), 0);
}

// ---------- Lo que se tapó una vez se sigue tapando ----------

const VIAJERO: &str = "IBARGÜEN SOLOZÁBAL MIREN";

/// Un viaje con dietas y sin relación laboral del 15/10/2026 (el 111 del 4T nombra al viajero en
/// sus avisos), con una recibida con retención para que haya 111.
fn viaje(v: &Vault) -> i64 {
    recibida(
        v,
        "B98765431",
        "Asesoría Ejemplo SL",
        "A-1",
        (1_000, 210, 150),
        date!(2026 - 10 - 05),
    );
    viajes::register(
        v,
        &Trip {
            date: date!(2026 - 10 - 15),
            employee_id: None,
            nif: "11111111H".into(),
            name: VIAJERO.into(),
            labor: false,
            origin: "València".into(),
            destination: "Bilbao".into(),
            reason: "Visita a un cliente".into(),
            abroad: false,
            overnight: false,
            days: 1,
            km: 350,
            km_amount: Money::from_cents(9_100),
            meals: e(30),
            lodging: Money::ZERO,
            tolls_parking: Money::ZERO,
            transport: Money::ZERO,
            counter_account: "551".into(),
        },
        AHORA,
    )
    .unwrap()
}

/// Lo que no puede llegar al proveedor del viajero.
const DEL_VIAJERO: [&str; 6] = [
    "IBARGÜEN",
    "Ibargüen",
    "IBARGUEN",
    "SOLOZÁBAL",
    "MIREN",
    "11111111",
];

fn sin(r: &str, fuera: &[&str]) {
    for f in fuera {
        assert!(!r.contains(f), "llega «{f}» al proveedor:\n{r}");
    }
}

#[tokio::test]
async fn lo_tapado_en_una_respuesta_se_tapa_aunque_el_resumen_ya_no_lo_nombre() {
    let env = Env::new(&[
        "El viajero [DATO_1] tiene 121,00 € de gastos que son retribución: inclúyelos en su \
         nómina.",
        "Sí, con lo que te dije.",
    ])
    .await;
    viaje(&env.vault());
    let c = env
        .ask(&Topic::General, None, "¿Qué avisos tengo?")
        .await
        .unwrap()
        .conversation
        .unwrap();
    sin(&env.requests()[0], &DEL_VIAJERO);
    // En el Mac, la respuesta lleva su nombre.
    assert!(c.messages[1].content.contains(VIAJERO), "{:?}", c.messages);
    // En 2027 el resumen ya no lo nombra, pero la respuesta guardada vuelve con el historial.
    env.ask_at(
        &Topic::General,
        Some(c.id),
        "¿Lo hago en la de enero?",
        date!(2027 - 01 - 12),
        datetime!(2027-01-12 10:00 UTC),
    )
    .await
    .unwrap();
    let r = &env.requests()[1];
    assert!(r.contains("inclúyelos en su"), "{r}");
    sin(r, &DEL_VIAJERO);
}

#[tokio::test]
async fn lo_tapado_en_una_pregunta_se_tapa_aunque_el_tema_ya_no_lo_nombre() {
    let env = Env::new(&["No: va en el 111 como rendimiento.", "Ya no sale."]).await;
    let id = viaje(&env.vault());
    let t = Topic::Modelo {
        model: "111".into(),
        year: 2026,
        period: 4,
    };
    let c = env
        .ask(
            &t,
            None,
            &format!("¿Tengo que hacerle una nómina a {VIAJERO}?"),
        )
        .await
        .unwrap()
        .conversation
        .unwrap();
    sin(&env.requests()[0], &DEL_VIAJERO);
    // El viaje era un error: se borra y el borrador ya no lo nombra.
    viajes::delete(&env.vault(), id, AHORA, true).unwrap();
    env.ask(&t, Some(c.id), "¿Y ahora?").await.unwrap();
    let r = &env.requests()[1];
    assert!(r.contains("¿Tengo que hacerle una nómina a"), "{r}");
    sin(r, &DEL_VIAJERO);
}

/// Cambia el nombre de la empresa (en la facturación y en su ficha).
fn renombrar(v: &Vault, nombre: &str) {
    let mut s = invoicing::settings(v).unwrap().unwrap();
    s.issuer.name = nombre.into();
    invoicing::save_settings(v, &s, AHORA).unwrap();
    companies::set_info(
        v,
        &CompanyInfo {
            name: nombre.into(),
            nif: Some("B12345674".into()),
        },
        AHORA,
    )
    .unwrap();
}

#[tokio::test]
async fn el_nombre_anterior_de_la_empresa_se_sigue_tapando() {
    let env = Env::new(&["Los del calendario de la empresa.", "Los mismos."]).await;
    renombrar(&env.vault(), "Quiñones Canario Ybarra SL");
    let c = env
        .ask(
            &Topic::General,
            None,
            "¿Qué plazos tiene Quiñones Canario Ybarra SL?",
        )
        .await
        .unwrap()
        .conversation
        .unwrap();
    let fuera = ["Quiñones", "QUIÑONES", "Quinones", "Ybarra"];
    sin(&env.requests()[0], &fuera);
    renombrar(&env.vault(), "Nombre Nuevo Ejemplo SL");
    env.ask(&Topic::General, Some(c.id), "¿Y el próximo?")
        .await
        .unwrap();
    let r = &env.requests()[1];
    assert!(r.contains("¿Qué plazos tiene"), "{r}");
    sin(r, &fuera);
}

// ---------- Hallazgos de la revisión de la carpeta que nombran a terceros ----------

fn documento(texto: &str) -> Vec<u8> {
    taxpain_invoicing::pdf::text_document(
        "Documento",
        &[vec![taxpain_invoicing::pdf::Para::Text(texto.into())]],
    )
    .unwrap()
}

/// Revisa una carpeta inventada con las lecturas de sus ficheros (como las deja Claude Code).
fn revisar_carpeta(v: &Vault, ficheros: Vec<(&str, serde_json::Value)>) -> Revision {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut lecturas = Vec::new();
    for (ruta, lectura) in ficheros {
        let p = root.join(ruta);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, documento(&format!("Documento {ruta}"))).unwrap();
        lecturas.push(serde_json::json!({ "fichero": ruta, "lectura": lectura }));
    }
    let paquete =
        serde_json::json!({ "version": 1, "generado_por": "Claude Code", "lecturas": lecturas });
    std::fs::write(
        root.join("taxpain-lecturas.json"),
        serde_json::to_vec_pretty(&paquete).unwrap(),
    )
    .unwrap();
    revision::revisar(v, root, HOY).unwrap()
}

fn factura(
    tipo: &str,
    (nif_emisor, emisor): (&str, &str),
    (nif_destinatario, destinatario): (&str, &str),
    numero: &str,
    fecha: &str,
    importes: [&str; 4],
) -> serde_json::Value {
    serde_json::json!({
        "tipo": tipo,
        "descripcion": format!("Factura {numero}"),
        "factura": {
            "nif_emisor": nif_emisor, "nombre_emisor": emisor,
            "nif_destinatario": nif_destinatario, "nombre_destinatario": destinatario,
            "numero": numero, "fecha": fecha,
            "base": importes[0], "iva": importes[1], "total": importes[2],
            "retencion": importes[3]
        }
    })
}

#[tokio::test]
async fn los_hallazgos_de_la_carpeta_no_sacan_a_los_terceros_que_nombran() {
    let env = Env::new(&[
        "Pide la complementaria.",
        "Revisa la nómina.",
        "Pide el 349.",
    ])
    .await;
    const PROPIA: (&str, &str) = ("B12345674", "CLIENTE UNO SL");
    taxpain_app::payroll::save_employee(
        &env.vault(),
        &taxpain_app::payroll::Employee {
            nif: "87654321X".into(),
            name: "QUINTANILLA EZCURRA, RAMIRO".into(),
            province: "46".into(),
            birth_year: 1970,
            family: 3,
            contract: 1,
            administrator: true,
            active: true,
            ..Default::default()
        },
        AHORA,
    )
    .unwrap();
    let mut m190 = serde_json::json!({
        "tipo": "modelo_aeat",
        "descripcion": "Modelo 190 0A 2025",
        "modelo": { "modelo": "190", "ejercicio": 2025, "periodo": "0A",
                    "casillas": {"01": "1", "02": "1000.00", "03": "150.00"} }
    });
    m190["modelo"]["registros"] = serde_json::json!([
        {"nif": "B11111119", "nombre": "Asesores Demo SL", "clave": "G", "importe": "1000.00",
         "retencion": "150.00"}
    ]);
    let mut m349 = serde_json::json!({
        "tipo": "modelo_aeat",
        "descripcion": "Modelo 349 1T 2025",
        "modelo": { "modelo": "349", "ejercicio": 2025, "periodo": "1T", "casillas": {"02": "0"} }
    });
    m349["modelo"]["registros"] = serde_json::json!([
        {"nif": "DE123456789", "nombre": "Kunde Demo", "clave": "S", "importe": "100.00"}
    ]);
    let r = revisar_carpeta(
        &env.vault(),
        vec![
            (
                "GASTOS/villalobos.pdf",
                factura(
                    "factura_recibida",
                    ("12345678Z", "VILLALOBOS CUESTA LUCIA"),
                    PROPIA,
                    "V-1",
                    "2025-03-10",
                    ["1000.00", "210.00", "1060.00", "150.00"],
                ),
            ),
            (
                "GASTOS/asesores.pdf",
                factura(
                    "factura_recibida",
                    ("B11111119", "Asesores Demo SL"),
                    PROPIA,
                    "A-1",
                    "2025-04-10",
                    ["1000.00", "210.00", "1060.00", "150.00"],
                ),
            ),
            ("MODELOS/190.pdf", m190),
            (
                "INGRESOS/a2.pdf",
                factura(
                    "factura_emitida",
                    PROPIA,
                    ("PT509876543", "CANÁRIO ESTEVES JOAQUIM"),
                    "A-2",
                    "2025-02-20",
                    ["2000.00", "0", "2000.00", "0"],
                ),
            ),
            ("MODELOS/349-1T.pdf", m349),
            (
                "EMPRESA/poderes.pdf",
                serde_json::json!({
                    "tipo": "escritura_poderes",
                    "descripcion": "Escritura de nombramiento de administradora",
                    "empresa": {"nif": "B12345674", "administrator": "María López García",
                                "date": "2023-05-04"}
                }),
            ),
        ],
    );
    revision::guardar_ultima(&env.vault(), &r).unwrap();
    for codigo in [
        "cruces.190_falta",
        "cruces.administrador_no_coincide",
        "cruces.349_falta",
    ] {
        let h = r
            .hallazgos
            .iter()
            .find(|h| h.codigo == codigo)
            .unwrap_or_else(|| panic!("falta {codigo}: {:?}", r.hallazgos));
        let t = Topic::Hallazgo {
            origin: Origen::Carpeta,
            code: h.codigo.clone(),
            year: h.ejercicio,
            title: h.titulo.clone(),
        };
        env.ask(&t, None, "¿Qué hago?").await.unwrap();
    }
    let reqs = env.requests();
    assert_eq!(reqs.len(), 3);
    assert!(
        reqs[0].contains("no están en el 190 de 2025"),
        "{}",
        reqs[0]
    );
    assert!(reqs[1].contains("cobra como administrador"), "{}", reqs[1]);
    assert!(reqs[2].contains("2.000,00 €"), "{}", reqs[2]);
    for r in &reqs {
        sin(
            r,
            &[
                "VILLALOBOS",
                "LUCIA",
                "12345678",
                "QUINTANILLA",
                "EZCURRA",
                "RAMIRO",
                "87654321",
                "CANÁRIO",
                "ESTEVES",
                "JOAQUIM",
                "509876543",
                "López García",
            ],
        );
    }
    // Y la foto del tema guarda los terceros: si el hallazgo deja de salir, siguen tapados.
    let vacia = revisar_carpeta(&env.vault(), vec![]);
    revision::guardar_ultima(&env.vault(), &vacia).unwrap();
    let t = Topic::Hallazgo {
        origin: Origen::Carpeta,
        code: "cruces.349_falta".into(),
        year: Some(2025),
        title: r
            .hallazgos
            .iter()
            .find(|h| h.codigo == "cruces.349_falta")
            .unwrap()
            .titulo
            .clone(),
    };
    let out = conversaciones::preview(|| Ok(env.vault()), HOY, &t, None, None).unwrap();
    assert!(out.system.contains("2.000,00 €"), "{}", out.system);
    sin(&out.system, &["CANÁRIO", "ESTEVES", "509876543"]);
}

// ---------- Certificados y claves con otra extensión ----------

#[test]
fn un_certificado_o_una_clave_no_se_añade_aunque_cambie_la_extension() {
    use taxpain_testkit::{P12Format, Subject, TEST_PASSWORD, TestPki, random_key_pkcs8};

    let cert = TestPki::fnmt().valid_client(&Subject::persona_fisica());
    let ficheros: [(&str, Vec<u8>); 5] = [
        ("clave", random_key_pkcs8()),
        ("certificado", cert.cert_der.clone()),
        ("p12", cert.to_p12(TEST_PASSWORD, P12Format::Legacy3Des)),
        (
            "p12-rsa",
            include_bytes!("../../../fixtures/certs/rsa-persona-fisica-aes.p12").to_vec(),
        ),
        (
            "certificado-rsa",
            include_bytes!("../../../fixtures/certs/rsa-app-banca.der").to_vec(),
        ),
    ];
    for (que, bytes) in &ficheros {
        for ext in ["csv", "txt", "md", "xlsx", "xls", "ods", "pdf", "jpg"] {
            let e = conversaciones::read_document(&format!("{que}.{ext}"), bytes)
                .map(|d| d.text)
                .unwrap_err()
                .to_string();
            assert_eq!(
                e, "Un certificado o una clave no se añade nunca a una conversación.",
                "{que}.{ext}"
            );
        }
    }
    // Un CSV que empieza por «0» no es una clave.
    let d = conversaciones::read_document(
        "cuotas.csv",
        b"0;Cuota de enero de la asesoria;10,00\n1;Cuota de febrero de la asesoria;10,00\n",
    )
    .unwrap();
    assert!(d.text.contains("Cuota de enero"), "{}", d.text);
}

// ---------- Hojas de cálculo que no se leen enteras ----------

#[test]
fn una_hoja_que_no_se_lee_entera_lo_dice() {
    // Siete hojas: van las cinco primeras y se dice cuáles no.
    let mut wb = rust_xlsxwriter::Workbook::new();
    for mes in [
        "Enero", "Febrero", "Marzo", "Abril", "Mayo", "Junio", "Julio",
    ] {
        let ws = wb.add_worksheet().set_name(mes).unwrap();
        ws.write_string(0, 0, "Concepto").unwrap();
        ws.write_string(1, 0, format!("Cuota de {mes} de la asesoría"))
            .unwrap();
    }
    let d = conversaciones::read_document("cuotas.xlsx", &wb.save_to_buffer().unwrap()).unwrap();
    assert!(d.text.contains("Hoja «Mayo»:"), "{}", d.text);
    assert!(!d.text.contains("Hoja «Junio»:"), "{}", d.text);
    assert!(d.text.contains("«Junio» y «Julio»"), "{}", d.text);
    assert!(d.truncated);

    // 400 filas: van 300 y se dice.
    let csv: String = (1..=400)
        .map(|i| format!("{i};Cuota {i};10,00\n"))
        .collect();
    let d = conversaciones::read_document("cuotas.csv", csv.as_bytes()).unwrap();
    assert!(d.text.contains("fila 300: "), "{}", d.text);
    assert!(!d.text.contains("fila 301: "), "{}", d.text);
    assert!(d.text.contains("300 de sus 400 filas"), "{}", d.text);
    assert!(d.truncated);

    // Un concepto largo va entero, y una fecha de Excel va como fecha.
    let largo = "Aportación al plan de pensiones individual de la socia, segundo trimestre";
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet();
    ws.write_string(0, 0, "Fecha").unwrap();
    ws.write_string(0, 1, "Concepto").unwrap();
    ws.write_datetime_with_format(
        1,
        0,
        rust_xlsxwriter::ExcelDateTime::from_ymd(2026, 3, 15).unwrap(),
        &rust_xlsxwriter::Format::new().set_num_format("dd/mm/yyyy"),
    )
    .unwrap();
    ws.write_string(1, 1, largo).unwrap();
    let d =
        conversaciones::read_document("aportacion.xlsx", &wb.save_to_buffer().unwrap()).unwrap();
    assert!(d.text.contains(&format!("B={largo}")), "{}", d.text);
    assert!(d.text.contains("A=15/03/2026"), "{}", d.text);
    assert!(!d.truncated, "{}", d.text);
}

// ---------- «Los datos de este tema han cambiado» ----------

#[tokio::test]
async fn con_inmovilizado_no_se_avisa_de_cambios_cada_dia() {
    let env = Env::new(&["Te conviene.", "Por la reserva."]).await;
    taxpain_app::accounting::add_fixed_asset(
        &env.vault(),
        &taxpain_core::accounting::depreciation::FixedAsset {
            id: 0,
            name: "Portátil".into(),
            account: acc("217"),
            depreciation_account: acc("2817"),
            expense_account: acc("681"),
            acquired_on: date!(2026 - 01 - 15),
            cost: e(1_200),
            residual: Money::ZERO,
            rate: Rate::percent(25),
            disposed_on: None,
            method: Default::default(),
            tax_rate: None,
        },
        AHORA,
    )
    .unwrap();
    let t = capitalizacion();
    let plan = Topic::AhorroPlan { year: 2026 };
    env.ask(&t, None, "¿Me conviene?").await.unwrap();
    env.ask(&plan, None, "¿Por cuál empiezo?").await.unwrap();
    // La amortización se devenga cada día: eso no es que hayan cambiado los datos.
    for dia in [date!(2026 - 10 - 31), date!(2026 - 11 - 06)] {
        assert!(
            !conversaciones::open(&env.vault(), dia, &t).unwrap().changed,
            "{dia}"
        );
        assert!(
            !conversaciones::open(&env.vault(), dia, &plan)
                .unwrap()
                .changed,
            "{dia}"
        );
    }
    // Lo que sí cambia (un paso marcado), se avisa.
    let m = Marca {
        estado: Estado::EnMarcha,
        pasos: [0].into_iter().collect(),
    };
    marcar(&env.vault(), HOY, "capitalizacion", &m).unwrap();
    assert!(
        conversaciones::open(&env.vault(), date!(2026 - 11 - 06), &t)
            .unwrap()
            .changed
    );
}

// ---------- El límite de conversaciones con varias primeras preguntas a la vez ----------

#[tokio::test]
async fn varias_primeras_preguntas_a_la_vez_no_pasan_del_limite() {
    let env = Env::new(&["Una.", "Dos.", "Tres."]).await;
    for _ in 0..MAX_CONVERSATIONS - 1 {
        env.vault()
            .connection()
            .execute(
                "INSERT INTO assistant_conversation(kind, topic_key, topic, title, created_at,
                   updated_at) VALUES ('general', '', '{\"kind\":\"general\"}', 'Vieja',
                   '2026-10-01T09:00:00Z', '2026-10-01T09:00:00Z')",
                [],
            )
            .unwrap();
    }
    let (t, plan) = (capitalizacion(), Topic::AhorroPlan { year: 2026 });
    let (a, b, c) = tokio::join!(
        env.ask(&t, None, "¿Me conviene?"),
        env.ask(&plan, None, "¿Por cuál empiezo?"),
        env.ask(&Topic::General, None, "Hola"),
    );
    let hechas = [&a, &b, &c].iter().filter(|r| r.is_ok()).count();
    assert_eq!(hechas, 1, "{a:?} {b:?} {c:?}");
    for r in [a, b, c].into_iter().filter_map(Result::err) {
        assert!(
            r.to_string()
                .starts_with("Ya tienes 100 conversaciones guardadas"),
            "{r}"
        );
    }
    assert_eq!(env.rows("assistant_conversation"), 100);
    // Las que no caben no se envían: no se paga una respuesta que no se pueda guardar.
    assert_eq!(env.requests().len(), 1);
    assert_eq!(env.chats().len(), 1);

    // Mientras espera la primera pregunta de un tema, lo que se le añade va a su conversación (es
    // la misma); a otro tema nuevo ya no le cabe.
    let env = Env::new(&["Una."]).await;
    for _ in 0..MAX_CONVERSATIONS - 1 {
        env.vault()
            .connection()
            .execute(
                "INSERT INTO assistant_conversation(kind, topic_key, topic, title, created_at,
                   updated_at) VALUES ('general', '', '{\"kind\":\"general\"}', 'Vieja',
                   '2026-10-01T09:00:00Z', '2026-10-01T09:00:00Z')",
                [],
            )
            .unwrap();
    }
    let (r, mismo, otro) = tokio::join!(
        env.ask(&t, None, "¿Me conviene?"),
        async { conversaciones::add_fact(&env.vault(), AHORA, &t, None, "Tenemos 3 empleados.") },
        async { conversaciones::add_fact(&env.vault(), AHORA, &plan, None, "Otro dato.") },
    );
    let c = r.unwrap().conversation.unwrap();
    assert_eq!(c.id, mismo.unwrap().id);
    assert_eq!(c.materials.len(), 1);
    assert!(
        otro.unwrap_err()
            .to_string()
            .starts_with("Ya tienes 100 conversaciones guardadas")
    );
    assert_eq!(env.rows("assistant_conversation"), 100);
}

// ---------- Un hallazgo que deja de salir sin haber preguntado ----------

/// Nóminas con un administrador inventado y una revisión de la carpeta con unas escrituras que
/// nombran a otra administradora: sale `cruces.administrador_no_coincide`, con su nombre en el
/// título.
fn administrador_que_no_coincide(v: &Vault) -> Hallazgo {
    taxpain_app::payroll::save_employee(
        v,
        &taxpain_app::payroll::Employee {
            nif: "87654321X".into(),
            name: "QUINTANILLA EZCURRA, RAMIRO".into(),
            province: "46".into(),
            birth_year: 1970,
            family: 3,
            contract: 1,
            administrator: true,
            active: true,
            ..Default::default()
        },
        AHORA,
    )
    .unwrap();
    let r = revisar_carpeta(
        v,
        vec![(
            "EMPRESA/poderes.pdf",
            serde_json::json!({
                "tipo": "escritura_poderes",
                "descripcion": "Escritura de nombramiento de administradora",
                "empresa": {"nif": "B12345674", "administrator": "María López García",
                            "date": "2023-05-04"}
            }),
        )],
    );
    revision::guardar_ultima(v, &r).unwrap();
    r.hallazgos
        .into_iter()
        .find(|h| h.codigo == "cruces.administrador_no_coincide")
        .unwrap()
}

#[tokio::test]
async fn un_hallazgo_sin_foto_no_envia_su_titulo() {
    let env = Env::new(&["Pide a la gestoría las escrituras del cese."]).await;
    let h = administrador_que_no_coincide(&env.vault());
    assert!(
        h.titulo.contains("QUINTANILLA EZCURRA, RAMIRO"),
        "{}",
        h.titulo
    );
    let t = Topic::Hallazgo {
        origin: Origen::Carpeta,
        code: h.codigo.clone(),
        year: h.ejercicio,
        title: h.titulo.clone(),
    };
    // Se añade un dato sin preguntar: la conversación existe, sin foto del tema.
    let c = conversaciones::add_fact(&env.vault(), AHORA, &t, None, "El cese se firmó en 2024.")
        .unwrap();
    // Se vuelve a revisar la carpeta y el hallazgo ya no sale.
    let vacia = revisar_carpeta(&env.vault(), vec![]);
    revision::guardar_ultima(&env.vault(), &vacia).unwrap();
    let fuera = [
        "QUINTANILLA",
        "Quintanilla",
        "EZCURRA",
        "RAMIRO",
        "87654321",
    ];
    let out = conversaciones::preview(|| Ok(env.vault()), HOY, &t, Some(c.id), None).unwrap();
    sin(&out.system, &fuera);
    // De qué va sí lo sabe la IA: un hallazgo de la revisión de la carpeta, con su código.
    assert!(
        out.system.contains("cruces.administrador_no_coincide"),
        "{}",
        out.system
    );
    // Desde Asistente → «Conversaciones guardadas».
    let o = conversaciones::open_by_id(&env.vault(), HOY, c.id).unwrap();
    assert_eq!(
        o.header.title, h.titulo,
        "el usuario ve el título (no se envía)"
    );
    env.ask(&t, Some(c.id), "¿Sigue pendiente?").await.unwrap();
    let r = &env.requests()[0];
    assert!(r.contains("El cese se firmó en 2024."), "{r}");
    sin(r, &fuera);
}

// ---------- Claves y certificados en base64 ----------

#[tokio::test]
async fn una_clave_o_un_p12_en_base64_no_se_añade_ni_sale() {
    use taxpain_core::base64::{encode, encode_wrapped};
    use taxpain_testkit::{P12Format, Subject, TEST_PASSWORD, TestPki, random_key_pkcs8};

    let clave = random_key_pkcs8();
    let cert = TestPki::fnmt().valid_client(&Subject::persona_fisica());
    let p12 = cert.to_p12(TEST_PASSWORD, P12Format::Legacy3Des);
    let una_linea = encode(&clave);
    let config = format!(
        "Conexión con el banco de pruebas\nusuario = demo\nprivate_key = {una_linea}\nentorno = \
         pruebas\n"
    );
    // Sin las cabeceras PEM: partida en líneas de 64 (PEM, openssl base64), de 76 (MIME) o en
    // una sola (corta: una clave EC entera son 184 caracteres).
    let ficheros: [(&str, String); 7] = [
        ("clave-64.txt", encode_wrapped(&clave, 64)),
        ("clave-64.md", encode_wrapped(&clave, 64)),
        ("certificado-76.txt", encode_wrapped(&p12, 76)),
        ("p12-64.csv", encode_wrapped(&p12, 64)),
        ("certificado.txt", encode_wrapped(&cert.cert_der, 64)),
        ("clave.txt", una_linea.clone()),
        ("config-banco.txt", config.clone()),
    ];
    for (nombre, texto) in &ficheros {
        let e = conversaciones::read_document(nombre, texto.as_bytes())
            .map(|d| d.text)
            .unwrap_err()
            .to_string();
        assert_eq!(
            e, "Un certificado o una clave no se añade nunca a una conversación.",
            "{nombre}"
        );
    }
    // En una hoja de cálculo, una fila por línea.
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet();
    for (i, l) in encode_wrapped(&p12, 64).lines().enumerate() {
        ws.write_string(u32::try_from(i).unwrap(), 0, l).unwrap();
    }
    assert_eq!(
        conversaciones::read_document("p12.xlsx", &wb.save_to_buffer().unwrap())
            .map(|d| d.text)
            .unwrap_err()
            .to_string(),
        "Un certificado o una clave no se añade nunca a una conversación."
    );

    let env = Env::new(&["No la necesito."]).await;
    let t = capitalizacion();
    // Tampoco como un dato, ni el texto ya leído de un documento.
    for texto in [
        encode_wrapped(&clave, 64),
        una_linea.clone(),
        format!("La clave del banco es {una_linea}"),
    ] {
        let e = conversaciones::add_fact(&env.vault(), AHORA, &t, None, &texto)
            .unwrap_err()
            .to_string();
        assert_eq!(
            e, "No se añade: contiene un certificado o una clave en base64.",
            "{texto}"
        );
    }
    let leido = conversaciones::DocumentReading {
        filename: "config-banco.txt".into(),
        sha256: "0".repeat(64),
        size: config.len(),
        text: config.clone(),
        truncated: false,
    };
    assert!(
        conversaciones::add_document(&env.vault(), AHORA, &t, None, &leido, ReadBy::Mac).is_err()
    );
    // Ni en la pregunta: no sale nada.
    let e = env
        .ask(&t, None, &format!("¿Está bien esta clave? {una_linea}"))
        .await
        .unwrap_err();
    assert!(e.to_string().starts_with("no se envía"), "{e}");
    assert!(env.requests().is_empty());
    assert_eq!(env.rows("assistant_conversation"), 0);
}

// ---------- La plaza de una primera pregunta que espera ----------

#[tokio::test]
async fn la_plaza_que_espera_no_cuenta_dos_veces_si_se_le_añade_un_dato() {
    let env = Env::new(&["Una.", "Dos."]).await;
    for _ in 0..MAX_CONVERSATIONS - 2 {
        env.vault()
            .connection()
            .execute(
                "INSERT INTO assistant_conversation(kind, topic_key, topic, title, created_at,
                   updated_at) VALUES ('general', '', '{\"kind\":\"general\"}', 'Vieja',
                   '2026-10-01T09:00:00Z', '2026-10-01T09:00:00Z')",
                [],
            )
            .unwrap();
    }
    let (t, plan) = (capitalizacion(), Topic::AhorroPlan { year: 2026 });
    // Mientras espera la primera pregunta de capitalización, un dato crea su conversación (99) y
    // la primera pregunta del plan es la 100: cabe.
    let (a, (dato, b)) = tokio::join!(env.ask(&t, None, "¿Me conviene?"), async {
        let dato = conversaciones::add_fact(&env.vault(), AHORA, &t, None, "Tenemos 3 empleados.");
        (dato, env.ask(&plan, None, "¿Por cuál empiezo?").await)
    });
    let dato = dato.unwrap();
    let b = b.unwrap();
    let a = a.unwrap().conversation.unwrap();
    assert_eq!(a.id, dato.id);
    assert_eq!((a.messages.len(), a.materials.len()), (2, 1));
    assert_eq!(b.conversation.unwrap().topic, plan);
    assert_eq!(env.rows("assistant_conversation"), 100);
}

/// Domicilio inventado: la factura completa lo exige (art. 6.1.e RD 1619/2012).
fn addr() -> Option<taxpain_core::invoice::Address> {
    Some(taxpain_core::invoice::Address {
        street: "Calle Inventada 1".into(),
        post_code: "46001".into(),
        town: "València".into(),
        province: "Valencia".into(),
        country: "ES".into(),
    })
}
