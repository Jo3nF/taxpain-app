//! Comandos del asistente IA. Sin efectos jurídicos: sólo explica y comprueba. Lo que se envía
//! al proveedor va anonimizado y nunca incluye el certificado, contraseñas ni claves.

use taxpain_app::assistant::{self, AssistantSettings, AssistantStatus};
use taxpain_app::conversaciones::{
    self, AskRequest, Conversation, ConversationSummary, ReadBy, Topic, TopicOpened, TopicOutgoing,
};
use taxpain_assistant::{Client, Message, prepare_with};
use time::OffsetDateTime;

use crate::state::AppState;
use crate::{CmdResult, CommandError};

/// Herramientas de sólo lectura sobre el almacén: cada llamada lo bloquea sólo mientras consulta.
/// Siempre de la empresa en la que se preguntó: si se cambia de empresa mientras responde, las
/// herramientas fallan en vez de leer (y enviar) datos de la otra.
struct AppTools<'a>(&'a AppState, &'a str);

impl taxpain_assistant::LocalTools for AppTools<'_> {
    fn call(&self, name: &str, input: &serde_json::Value) -> Result<String, String> {
        let v = self.0.vault_of(self.1).map_err(|e| e.message)?;
        assistant::tool_call(&v, today(), name, input)
    }
}

fn today() -> time::Date {
    taxpain_core::spain_time::peninsular_date(OffsetDateTime::now_utc())
}

#[tauri::command]
pub fn assistant_status(state: tauri::State<'_, AppState>) -> CmdResult<AssistantStatus> {
    Ok(assistant::status(&*state.vault()?)?)
}

/// Guarda los ajustes y, si llega `key`, la clave de API del proveedor nuevo a la vez: con
/// clave, Touch ID con el servidor al que irá, y si se cancela no se guarda nada (ni el
/// proveedor). `false`: con un proveedor de otro host, lo aceptado no se ha guardado (hay que
/// aceptarlo otra vez, ya con este proveedor).
#[tauri::command(async)]
pub fn assistant_save(
    state: tauri::State<'_, AppState>,
    settings: AssistantSettings,
    key: Option<String>,
) -> CmdResult<bool> {
    let key = zeroize::Zeroizing::new(key);
    crate::confirmar::guardar_asistente(&state, &settings, key.as_deref())
}

/// Guarda la clave de API del proveedor guardado (o la borra con `null`), con Touch ID. Nunca
/// se devuelve.
#[tauri::command(async)]
pub fn assistant_set_key(state: tauri::State<'_, AppState>, key: Option<String>) -> CmdResult<()> {
    let key = zeroize::Zeroizing::new(key);
    crate::confirmar::guardar_clave_asistente(&state, key.as_deref())
}

// ---------- Conversaciones con la IA (por tema o generales, guardadas en la empresa) ----------

/// Abre un tema en el panel (no crea nada): la cabecera y su conversación, si la hay.
#[tauri::command(async)]
pub fn assistant_topic_open(
    state: tauri::State<'_, AppState>,
    topic: Topic,
) -> CmdResult<TopicOpened> {
    Ok(conversaciones::open(&*state.vault()?, today(), &topic)?)
}

/// Abre una conversación guardada.
#[tauri::command(async)]
pub fn assistant_conversation_open(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> CmdResult<TopicOpened> {
    Ok(conversaciones::open_by_id(&*state.vault()?, today(), id)?)
}

/// Las conversaciones guardadas de la empresa abierta.
#[tauri::command(async)]
pub fn assistant_conversations(
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<ConversationSummary>> {
    Ok(conversaciones::list(&*state.vault()?)?)
}

/// Exactamente lo que se enviaría con la pregunta (o sin ella, todavía). Los datos se reúnen con
/// el almacén cogido y se anonimizan después, sin él.
#[tauri::command(async)]
pub fn assistant_topic_preview(
    state: tauri::State<'_, AppState>,
    topic: Topic,
    conversation_id: Option<i64>,
    question: Option<String>,
) -> CmdResult<TopicOutgoing> {
    Ok(conversaciones::preview(
        || Ok(state.vault()?),
        today(),
        &topic,
        conversation_id,
        question.as_deref(),
    )?)
}

/// Pregunta en una conversación. Fijada a la empresa en la que se pregunta: si se cambia de
/// empresa mientras responde, las herramientas fallan en vez de leer la otra, y la respuesta se
/// guarda en la empresa en la que se preguntó (aunque ya no sea la abierta).
#[tauri::command]
pub async fn assistant_topic_ask(
    state: tauri::State<'_, AppState>,
    topic: Topic,
    conversation_id: Option<i64>,
    question: String,
) -> CmdResult<TopicOpened> {
    let company = state.active_id()?;
    let client = Client::new()?;
    let req = AskRequest {
        topic: &topic,
        conversation: conversation_id,
        question: &question,
        hoy: today(),
    };
    Ok(conversaciones::ask(
        state.vault_fn(&company),
        || {
            state
                .company_vault(&company)
                .map_err(|e| taxpain_app::AppError::Other(e.message))
        },
        &client,
        &AppTools(&state, &company),
        &req,
        OffsetDateTime::now_utc,
    )
    .await?)
}

/// Añade un dato escrito por el usuario a la conversación (la crea si no la había).
#[tauri::command(async)]
pub fn assistant_fact_add(
    state: tauri::State<'_, AppState>,
    topic: Topic,
    conversation_id: Option<i64>,
    text: String,
) -> CmdResult<Conversation> {
    Ok(conversaciones::add_fact(
        &*state.vault()?,
        OffsetDateTime::now_utc(),
        &topic,
        conversation_id,
        &text,
    )?)
}

/// Lee en el Mac el documento de `path` (elegido con el diálogo nativo) y añade su texto a la
/// conversación. El fichero no se guarda ni sale del Mac; se lee sin el almacén cogido.
#[tauri::command(async)]
pub fn assistant_file_add(
    state: tauri::State<'_, AppState>,
    topic: Topic,
    conversation_id: Option<i64>,
    path: String,
) -> CmdResult<Conversation> {
    let company = state.active_id()?;
    let bytes = crate::read_limited(&path, conversaciones::MAX_FILE_BYTES as u64)?;
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "documento".into());
    let doc = conversaciones::read_document(&name, &bytes)?;
    Ok(conversaciones::add_document(
        &*state.vault_of(&company)?,
        OffsetDateTime::now_utc(),
        &topic,
        conversation_id,
        &doc,
        ReadBy::Mac,
    )?)
}

/// Quita algo de lo añadido a una conversación.
#[tauri::command(async)]
pub fn assistant_material_delete(
    state: tauri::State<'_, AppState>,
    conversation_id: i64,
    material_id: i64,
) -> CmdResult<Conversation> {
    Ok(conversaciones::remove_material(
        &*state.vault()?,
        conversation_id,
        material_id,
    )?)
}

/// Borra una conversación (con lo que se añadió).
#[tauri::command(async)]
pub fn assistant_conversation_delete(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    Ok(conversaciones::delete(&*state.vault()?, id)?)
}

/// Borra todas las conversaciones de la empresa abierta.
#[tauri::command(async)]
pub fn assistant_conversations_delete_all(state: tauri::State<'_, AppState>) -> CmdResult<usize> {
    Ok(conversaciones::delete_all(&*state.vault()?)?)
}

/// Propone los datos de una factura recibida con el asistente: el texto del documento sale
/// anonimizado (NIF, IBAN, emails y teléfonos como marcadores, que se restauran al volver) y el
/// resultado sólo rellena el formulario de revisión; no se guarda nada sin confirmar.
#[tauri::command]
pub async fn received_ai_extract(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> CmdResult<taxpain_invoicing::ingest::Extraction> {
    let company = state.active_id()?;
    ai_extract(&state, &company, id).await
}

/// Lee una recibida con el asistente. `company`: la empresa de la recibida (si se cambia a
/// mitad, se para en vez de mezclar datos de otra).
async fn ai_extract(
    state: &AppState,
    company: &str,
    id: i64,
) -> CmdResult<taxpain_invoicing::ingest::Extraction> {
    let (status, key, out) = {
        let vault = state.vault_of(company)?;
        let status = assistant::status(&vault)?;
        if status.remote && !status.settings.consent {
            return Err(CommandError::from(
                "Activa el asistente en sus ajustes y acepta qué datos se envían al proveedor.",
            ));
        }
        let key = assistant::api_key(&vault)?.map(zeroize::Zeroizing::new);
        let text = taxpain_app::received::document_text(&vault, id)?;
        // El texto del PDF es ajeno: sin invisibles y entre marcas, para que el modelo no
        // obedezca lo que diga dentro.
        let message = Message {
            role: taxpain_assistant::Role::User,
            content: format!(
                "{}{}",
                taxpain_app::received::AI_EXTRACTION_PROMPT,
                taxpain_assistant::external_document("factura recibida", &text)
            ),
        };
        let known = assistant::known_values(&vault)?;
        let out = prepare_with("", &[message], &known)?;
        // En la auditoría antes de que salga (aunque luego falle la respuesta).
        assistant::record_chat(
            &vault,
            &status.settings.provider,
            out.replaced(),
            OffsetDateTime::now_utc(),
        )?;
        (status, key, out)
    };
    let client = Client::new()?;
    let answer = taxpain_assistant::ask(
        &client,
        &status.settings.provider,
        key.as_deref().map(String::as_str),
        &out,
    )
    .await?;
    Ok(taxpain_app::received::parse_ai_extraction(&answer.text))
}

/// Lo que hizo el asistente al completar las recibidas pendientes.
#[derive(serde::Serialize)]
pub struct FillOutcome {
    pub filled: usize,
    pub failed: Vec<String>,
}

/// Completa con el asistente las recibidas pendientes a las que les faltan datos (como mucho
/// `limit`). Cada una sale anonimizada como en [`received_ai_extract`] y sigue pendiente de
/// revisar: no se contabiliza nada.
#[tauri::command]
pub async fn received_ai_fill_pending(
    state: tauri::State<'_, AppState>,
    limit: usize,
) -> CmdResult<FillOutcome> {
    let (company, ids) = {
        let (company, v) = state.vault_with_id()?;
        (company, taxpain_app::received::pending_incomplete(&v)?)
    };
    let mut out = FillOutcome {
        filled: 0,
        failed: Vec::new(),
    };
    for id in ids.into_iter().take(limit.clamp(1, 200)) {
        match ai_extract(&state, &company, id).await {
            Ok(e) => {
                // Si la recibida ya no está pendiente (se ha revisado o descartado mientras
                // tanto), se apunta como fallida en vez de parar el lote.
                match taxpain_app::received::merge_extraction(&*state.vault_of(&company)?, id, &e) {
                    Ok(_) => out.filled += 1,
                    Err(e) => out.failed.push(e.to_string()),
                }
            }
            Err(e) => out.failed.push(e.message),
        }
    }
    Ok(out)
}
