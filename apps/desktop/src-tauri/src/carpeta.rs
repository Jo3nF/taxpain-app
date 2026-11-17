//! Importar la carpeta de la empresa y comparar con los modelos del gestor.

use std::collections::BTreeMap;
use std::path::Path;

use taxpain_app::carpeta::{self, Inventory, Outcome};
use taxpain_app::documentos;
use taxpain_app::presentados::{self, Comparison, Filed};
use taxpain_app::revision::{self, Revision};
use taxpain_core::money::Money;
use time::OffsetDateTime;

use crate::state::AppState;
use crate::{CmdResult, CommandError};

#[tauri::command]
pub fn folder_get(state: tauri::State<'_, AppState>) -> CmdResult<Option<String>> {
    Ok(carpeta::folder(&*state.vault()?)?)
}

/// Lee la carpeta (sin cambiar nada) y la recuerda. En un hilo aparte: puede tardar. Coge el
/// almacén fichero a fichero (así la app sigue respondiendo) y siempre el de la empresa con la
/// que empezó: si se cambia de empresa a mitad, para en vez de seguir en la otra.
#[tauri::command(async)]
pub fn folder_scan(state: tauri::State<'_, AppState>, root: String) -> CmdResult<Inventory> {
    let company = state.active_id()?;
    let inv = carpeta::scan_with(Path::new(&root), state.vault_fn(&company))?;
    let v = state.vault_of(&company)?;
    carpeta::set_folder(&v, &root)?;
    // Los avisos de las carpetas vigiladas de lo que hay aquí ya se ven en la lista.
    taxpain_app::vigiladas::dismiss_under(&v, &root)?;
    Ok(inv)
}

/// Revisa la carpeta: errores de la empresa y de la gestoría, lo que falta y las tablas. No
/// cambia nada. En un hilo aparte: con muchas facturas puede tardar.
#[tauri::command(async)]
pub fn folder_review(state: tauri::State<'_, AppState>, root: String) -> CmdResult<Revision> {
    let hoy = taxpain_core::spain_time::peninsular_date(OffsetDateTime::now_utc());
    let company = state.active_id()?;
    let r = revision::revisar_with(Path::new(&root), state.vault_fn(&company), hoy)?;
    let v = state.vault_of(&company)?;
    carpeta::set_folder(&v, &root)?;
    revision::guardar_ultima(&v, &r)?;
    Ok(r)
}

/// La última revisión de la carpeta, para enseñarla al volver a la pestaña sin esperar.
#[tauri::command]
pub fn folder_review_last(state: tauri::State<'_, AppState>) -> CmdResult<Option<Revision>> {
    Ok(revision::ultima(&*state.vault()?))
}

/// Guarda el informe de la revisión en PDF, Excel o Markdown. El contenido no lo manda la
/// interfaz: `source` dice cuál, la última revisión de la carpeta (`carpeta`, la de
/// [`folder_review_last`]) o la revisión con lo que tiene la AEAT (`aeat`, la de
/// `company_review`, que se vuelve a hacer).
#[tauri::command]
pub fn folder_review_save(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    source: Option<String>,
    path: String,
    format: String,
) -> CmdResult<()> {
    let ext = match format.as_str() {
        "pdf" => "pdf",
        "xlsx" => "xlsx",
        "md" => "md",
        _ => return Err(CommandError::from("formato desconocido")),
    };
    let path = crate::salida::salida_permitida(&app, &path, &[ext])?;
    let review = {
        let v = state.vault()?;
        match source.as_deref() {
            None | Some("carpeta") => revision::ultima(&v)
                .ok_or_else(|| CommandError::from("todavía no hay ninguna revisión guardada"))?,
            Some("aeat") => revision::revisar_aeat(&v, crate::calendar::today())?,
            Some(_) => return Err(CommandError::from("revisión desconocida")),
        }
    };
    let bytes = match ext {
        "pdf" => revision::informe::pdf(&review)?,
        "xlsx" => revision::informe::excel(&review)?,
        _ => revision::informe::markdown(&review).into_bytes(),
    };
    crate::salida::escribir(&path, bytes)?;
    Ok(())
}

#[tauri::command(async)]
pub fn folder_apply(
    state: tauri::State<'_, AppState>,
    root: String,
    paths: Vec<String>,
) -> CmdResult<Vec<Outcome>> {
    let company = state.active_id()?;
    Ok(carpeta::apply_with(
        Path::new(&root),
        &paths,
        state.vault_fn(&company),
        OffsetDateTime::now_utc(),
    )?)
}

/// Importaciones de facturas a los libros (libros del gestor y facturas leídas con el
/// asistente), la última primero.
#[tauri::command]
pub fn folder_book_imports(
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<taxpain_app::libros::BookBatch>> {
    Ok(taxpain_app::libros::batches(&*state.vault()?)?)
}

/// Deshace una importación de facturas (con confirmación). Devuelve cuántas se quitan.
#[tauri::command]
pub fn folder_book_undo(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<usize> {
    Ok(taxpain_app::libros::undo(
        &*state.vault()?,
        id,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn filed_list(state: tauri::State<'_, AppState>, year: Option<i32>) -> CmdResult<Vec<Filed>> {
    Ok(presentados::list(&*state.vault()?, year)?)
}

/// En un hilo aparte: calcula los modelos de los periodos anteriores para encadenarlos.
#[tauri::command(async)]
pub fn filed_compare(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<Comparison> {
    Ok(presentados::compare(&*state.vault()?, id)?)
}

#[tauri::command]
pub fn filed_save_manual(
    state: tauri::State<'_, AppState>,
    model: String,
    year: i32,
    period: String,
    boxes: BTreeMap<u16, i64>,
) -> CmdResult<i64> {
    let boxes: BTreeMap<u16, Money> = boxes
        .into_iter()
        .map(|(k, v)| (k, Money::from_cents(v)))
        .collect();
    Ok(presentados::save_manual(
        &*state.vault()?,
        &model,
        year,
        &period,
        &boxes,
        OffsetDateTime::now_utc(),
    )?)
}

/// Borra un modelo presentado guardado: Touch ID con el modelo y el periodo.
#[tauri::command(async)]
pub fn filed_delete(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    crate::confirmar::borrar_presentado(&state, id)
}

// ---------- Documentos de la empresa ----------

#[tauri::command]
pub fn company_docs_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<documentos::Stored>> {
    Ok(documentos::list(&*state.vault()?)?)
}

/// Abre el documento con la app del sistema (Vista Previa), desde una copia temporal.
#[tauri::command]
pub fn company_doc_open(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: i64,
) -> CmdResult<()> {
    let (name, _, bytes) = documentos::content(&*state.vault()?, id)?;
    // Copia descifrada en una carpeta privada, con nombre al azar; se borra pasada una hora o
    // al volver a arrancar la app.
    let path = documentos::temp_copy(&name, &bytes)?;
    crate::salida::abrir_copia(&app, &path)
}

/// Borra un documento de la empresa: Touch ID con su título.
#[tauri::command(async)]
pub fn company_doc_delete(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    crate::confirmar::borrar_documento(&state, id)
}

#[tauri::command]
pub fn company_suggestions(
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<documentos::Suggestion>> {
    Ok(documentos::suggestions(&*state.vault()?)?)
}

#[tauri::command]
pub fn company_apply_suggestion(state: tauri::State<'_, AppState>, field: String) -> CmdResult<()> {
    Ok(documentos::apply_suggestion(
        &*state.vault()?,
        &field,
        OffsetDateTime::now_utc(),
    )?)
}

/// Completa con el asistente los datos de un documento: su texto sale anonimizado (los datos
/// vuelven como marcadores y se restauran en el Mac) y sólo se añaden los que faltaban.
#[tauri::command]
pub async fn company_doc_ai(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> CmdResult<documentos::Facts> {
    use taxpain_app::assistant;
    let (company, status, key, out) = {
        let (company, vault) = state.vault_with_id()?;
        let status = assistant::status(&vault)?;
        if status.remote && !status.settings.consent {
            return Err(crate::CommandError::from(
                "Activa el asistente en sus ajustes y acepta qué datos se envían al proveedor.",
            ));
        }
        let key = assistant::api_key(&vault)?.map(zeroize::Zeroizing::new);
        let text = documentos::text(&vault, id)?;
        let message = taxpain_assistant::Message {
            role: taxpain_assistant::Role::User,
            content: format!(
                "{}{}",
                documentos::AI_PROMPT,
                taxpain_assistant::external_document("documento de la empresa", &text)
            ),
        };
        let known = assistant::known_values(&vault)?;
        let out = taxpain_assistant::prepare_with("", &[message], &known)?;
        // En la auditoría antes de que salga (aunque luego falle la respuesta).
        assistant::record_chat(
            &vault,
            &status.settings.provider,
            out.replaced(),
            OffsetDateTime::now_utc(),
        )?;
        (company, status, key, out)
    };
    let client = taxpain_assistant::Client::new()?;
    let answer = taxpain_assistant::ask(
        &client,
        &status.settings.provider,
        key.as_deref().map(String::as_str),
        &out,
    )
    .await?;
    let vault = state.vault_of(&company)?;
    Ok(documentos::merge_facts(
        &vault,
        id,
        &documentos::parse_ai(&answer.text),
    )?)
}

// ---------- Lectura con el asistente («Full IA») ----------

use taxpain_app::lectura_ia;

#[tauri::command]
pub fn lectura_ia_get(state: tauri::State<'_, AppState>) -> CmdResult<lectura_ia::Settings> {
    Ok(lectura_ia::settings(&*state.vault()?)?)
}

/// Guarda los ajustes de la lectura con IA: activar «Full IA» pide Touch ID.
#[tauri::command(async)]
pub fn lectura_ia_save(
    state: tauri::State<'_, AppState>,
    settings: lectura_ia::Settings,
) -> CmdResult<lectura_ia::Settings> {
    crate::confirmar::guardar_lectura_ia(&state, &settings)
}

#[derive(serde::Serialize, Clone)]
struct AiProgress {
    done: usize,
    total: usize,
    path: String,
}

#[derive(serde::Serialize)]
pub struct AiReadOutcome {
    pub read: usize,
    pub reused: usize,
    pub failed: Vec<(String, String)>,
}

/// Lee con el asistente los ficheros elegidos de la carpeta. Sólo con «Full IA» activado: las
/// hojas salen como texto anonimizado; los PDF y las imágenes, tal cual. Cada lectura se guarda
/// y no se repite. Avisa del progreso con el evento `folder-ai-progress`.
#[tauri::command]
pub async fn folder_ai_read(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    root: String,
    paths: Vec<String>,
    again: bool,
) -> CmdResult<AiReadOutcome> {
    use tauri::Emitter;
    use taxpain_app::assistant;
    let (company, provider, key, own) = {
        let (company, v) = state.vault_with_id()?;
        let s = lectura_ia::settings(&v)?;
        if !s.full {
            return Err(crate::CommandError::from(
                "Activa «Full IA» para que el asistente lea los ficheros.",
            ));
        }
        let status = assistant::status(&v)?;
        if status.remote && !status.settings.consent {
            return Err(crate::CommandError::from(
                "Activa el asistente en sus ajustes y acepta qué datos se envían al proveedor.",
            ));
        }
        let mut provider = status.settings.provider.clone();
        if let (taxpain_assistant::Provider::Anthropic { model }, Some(m)) = (
            &mut provider,
            s.model.clone().filter(|m| !m.trim().is_empty()),
        ) {
            *model = m;
        }
        let key = assistant::api_key(&v)?.map(zeroize::Zeroizing::new);
        let info = taxpain_app::companies::info(&v)?;
        (
            company,
            provider,
            key,
            (info.name, info.nif.unwrap_or_default()),
        )
    };
    let client = taxpain_assistant::Client::new()?;
    let root = std::path::PathBuf::from(root);
    let mut out = AiReadOutcome {
        read: 0,
        reused: 0,
        failed: vec![],
    };
    let total = paths.len();
    for (i, rel) in paths.iter().enumerate() {
        // Si se desactiva «Full IA» mientras tanto, no sale ningún fichero más.
        if !lectura_ia::settings(&*state.vault_of(&company)?)?.full {
            out.failed.push((
                "(los que quedaban)".to_owned(),
                format!(
                    "se ha desactivado «Full IA»: no se han enviado los {} ficheros que quedaban",
                    total - i
                ),
            ));
            break;
        }
        let _ = app.emit(
            "folder-ai-progress",
            AiProgress {
                done: i,
                total,
                path: rel.clone(),
            },
        );
        let result: CmdResult<bool> = async {
            let (name, bytes) = carpeta::read_file(&root, rel)?;
            if !again && lectura_ia::stored(&*state.vault_of(&company)?, &bytes)?.is_some() {
                return Ok(false);
            }
            let excerpt = lectura_ia::excerpt(&name, &bytes).ok_or_else(|| {
                crate::CommandError::from("tipo de fichero que el asistente no lee")
            })?;
            // El nombre del fichero (que suele llevar el del cliente o el proveedor) va dentro
            // de las marcas de documento externo, como el contenido: son datos, no instrucciones.
            let reply = match excerpt {
                lectura_ia::Excerpt::Sheet(text) => {
                    let message = taxpain_assistant::Message {
                        role: taxpain_assistant::Role::User,
                        content: format!(
                            "{}\n\nMi empresa: {} (NIF {}).\n\n{}",
                            lectura_ia::PROMPT,
                            own.0,
                            own.1,
                            taxpain_assistant::external_document(
                                &format!("hoja de cálculo «{name}»"),
                                &text
                            )
                        ),
                    };
                    let prepared = {
                        let v = state.vault_of(&company)?;
                        let p = taxpain_assistant::prepare_with(
                            "",
                            &[message],
                            &assistant::known_values(&v)?,
                        )?;
                        // En la auditoría antes de que salga (aunque luego falle la respuesta).
                        assistant::record_chat(
                            &v,
                            &provider,
                            p.replaced(),
                            OffsetDateTime::now_utc(),
                        )?;
                        p
                    };
                    taxpain_assistant::ask(
                        &client,
                        &provider,
                        key.as_deref().map(String::as_str),
                        &prepared,
                    )
                    .await?
                    .text
                }
                lectura_ia::Excerpt::File { media_type } => {
                    let file = taxpain_assistant::Attachment {
                        filename: name.clone(),
                        media_type,
                        data: lectura_ia::attachment_bytes(&name, &bytes),
                    };
                    // El fichero entero (sin anonimizar) queda en la auditoría con su huella
                    // antes de salir, aunque luego falle la respuesta (docs/PRIVACIDAD_APP.md
                    // §2, fila «Full IA»).
                    assistant::record_file(
                        &*state.vault_of(&company)?,
                        &provider,
                        &bytes,
                        OffsetDateTime::now_utc(),
                    )?;
                    client
                        .ask_attachment(
                            &provider,
                            key.as_deref().map(String::as_str),
                            lectura_ia::PROMPT,
                            &format!(
                                "Mi empresa: {} (NIF {}). {}",
                                own.0,
                                own.1,
                                taxpain_assistant::external_document("nombre del fichero", &name)
                            ),
                            &file,
                        )
                        .await?
                }
            };
            let reading = lectura_ia::parse_reply(&reply).ok_or_else(|| {
                crate::CommandError::from("el asistente no devolvió una lectura válida")
            })?;
            let v = state.vault_of(&company)?;
            lectura_ia::store(&v, &bytes, &reading, true, OffsetDateTime::now_utc())?;
            // (La auditoría se apunta antes de enviar.)
            Ok(true)
        }
        .await;
        match result {
            Ok(true) => out.read += 1,
            Ok(false) => out.reused += 1,
            Err(e) => out.failed.push((rel.clone(), e.message)),
        }
    }
    let _ = app.emit(
        "folder-ai-progress",
        AiProgress {
            done: total,
            total,
            path: String::new(),
        },
    );
    Ok(out)
}
