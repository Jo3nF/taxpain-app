//! Comandos de comunicaciones y notificaciones de la AEAT (servicio GNNO-JDIT, producción).
//! Consultar y descargar no tienen efectos. **Comparecer** a una notificación pendiente la
//! da por practicada y abre sus plazos: sólo con la confirmación expresa del usuario.

use taxpain_aeat::AeatClient;
use taxpain_aeat::notifications::{self, Contenido, Endpoints, Envio};
use taxpain_app::notices::{self, Notice, NoticeContent, NoticeMeta};
use time::{Duration, OffsetDateTime};

use crate::state::AppState;
use crate::{CmdResult, CommandError};

/// Cliente con el certificado elegido para las notificaciones de la empresa `company`, y el NIF
/// de la empresa (el que se consulta: un certificado personal, del administrador o de un
/// apoderado, consulta las de la empresa; la AEAT comprueba que puede). Uno de representante de
/// **otra** entidad, nunca.
fn client(
    state: &AppState,
    company: &str,
    fingerprint: &str,
    reason: &str,
) -> CmdResult<(AeatClient, String)> {
    let vault = state.vault_of(company)?;
    let t = crate::presentado::check_target(&vault, fingerprint)?;
    let nif = crate::presentado::allow_cert(&t, true, "consultar las notificaciones")?;
    let request = vault.prepare_identity(fingerprint, reason, OffsetDateTime::now_utc())?;
    drop(vault);
    let identity = state.identity_of(company, request)?;
    Ok((
        AeatClient::with_platform_trust(&identity, taxpain_aeat::hosts::PRODUCCION)?,
        nif,
    ))
}

fn meta(e: Envio) -> NoticeMeta {
    NoticeMeta {
        id: e.id,
        kind: e.kind,
        state: e.state,
        role: e.role,
        subject: e.subject,
        available_at: e.available_at,
        holder_nif: e.holder_nif,
    }
}

fn content(c: Contenido) -> NoticeContent {
    NoticeContent {
        metadata: c.metadata,
        document_pdf: c.document_pdf,
        certification_pdf: c.certification_pdf,
    }
}

#[tauri::command]
pub fn notices_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Notice>> {
    Ok(notices::list(&*state.vault()?)?)
}

/// Consulta en la AEAT los envíos de los últimos `days` días (sólo lectura). Devuelve cuántos
/// son nuevos.
#[tauri::command]
pub async fn notices_refresh(
    state: tauri::State<'_, AppState>,
    fingerprint: String,
    days: Option<i64>,
) -> CmdResult<usize> {
    let company = state.active_id()?;
    let (client, nif) = client(
        &state,
        &company,
        &fingerprint,
        "Consultar tus notificaciones de la AEAT",
    )?;
    let today = OffsetDateTime::now_utc().date();
    let from = today - Duration::days(days.unwrap_or(180).clamp(1, 730));
    let list = notifications::list(&client, &Endpoints::production(), &nif, from, today)
        .await?
        .map_err(|e| CommandError::from(e.to_string()))?;
    let metas: Vec<NoticeMeta> = list.into_iter().map(meta).collect();
    Ok(notices::record_listed(
        &*state.vault_of(&company)?,
        true,
        &metas,
        OffsetDateTime::now_utc(),
    )?)
}

/// Descarga una comunicación o una notificación ya comparecida (no tiene efectos).
#[tauri::command]
pub async fn notice_download(
    state: tauri::State<'_, AppState>,
    id: String,
    fingerprint: String,
) -> CmdResult<()> {
    let company = state.active_id()?;
    let (client, _) = client(
        &state,
        &company,
        &fingerprint,
        "Descargar una comunicación de la AEAT",
    )?;
    let c = notifications::download(&client, &Endpoints::production(), &id)
        .await?
        .map_err(|e| CommandError::from(e.to_string()))?;
    Ok(notices::record_content(
        &*state.vault_of(&company)?,
        &id,
        &content(c),
        false,
        OffsetDateTime::now_utc(),
    )?)
}

/// **Comparece** a una notificación pendiente: queda notificada y empiezan sus plazos.
#[tauri::command]
pub async fn notice_appear(
    state: tauri::State<'_, AppState>,
    id: String,
    fingerprint: String,
    confirmed: bool,
) -> CmdResult<()> {
    if !confirmed {
        return Err(CommandError::from(
            "comparecer abre los plazos de la notificación: hace falta tu confirmación",
        ));
    }
    let (company, subject) = {
        let (company, v) = state.vault_with_id()?;
        // El asunto, del almacén (no de la interfaz): en el aviso de Touch ID se ve qué se abre.
        let subject = notices::list(&v)?
            .into_iter()
            .find(|n| n.id == id)
            .map(|n| crate::invoicing::reason_text(&n.subject))
            .filter(|s| !s.is_empty());
        notices::begin_appear(&v, &id, OffsetDateTime::now_utc())?;
        (company, subject)
    };
    let reason = match subject {
        Some(s) => {
            format!("Abrir (comparecer) la notificación de la AEAT «{s}»: empiezan sus plazos")
        }
        None => "Abrir (comparecer) una notificación de la AEAT: empiezan sus plazos".to_owned(),
    };
    let (client, _) = client(&state, &company, &fingerprint, &reason)?;
    let c = notifications::appear(&client, &Endpoints::production(), &id, confirmed)
        .await?
        .map_err(|e| CommandError::from(e.to_string()))?;
    Ok(notices::record_content(
        &*state.vault_of(&company)?,
        &id,
        &content(c),
        true,
        OffsetDateTime::now_utc(),
    )?)
}

/// Guarda en `dir` el PDF del envío (o la certificación de comparecencia) y devuelve la ruta.
#[tauri::command]
pub fn notice_save_pdf(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
    certification: bool,
    dir: String,
) -> CmdResult<String> {
    let pdf = notices::pdf(&*state.vault()?, &id, certification)?
        .ok_or_else(|| CommandError::from("todavía no hay contenido descargado"))?;
    let name = notices::pdf_file_name(&id, certification);
    let path = crate::salida::en_carpeta(&app, &dir, &name, &["pdf"])?;
    crate::salida::escribir(&path, pdf)?;
    Ok(path.to_string_lossy().into_owned())
}

/// Días de cortesía marcados en la Sede (recordatorio local).
#[tauri::command]
pub fn notices_courtesy_days(state: tauri::State<'_, AppState>) -> CmdResult<Vec<time::Date>> {
    Ok(notices::courtesy_days(&*state.vault()?)?)
}

/// Guarda los días de cortesía (como mucho 30 por año) y los devuelve ordenados.
#[tauri::command]
pub fn notices_set_courtesy_days(
    state: tauri::State<'_, AppState>,
    days: Vec<time::Date>,
) -> CmdResult<Vec<time::Date>> {
    Ok(notices::set_courtesy_days(&*state.vault()?, &days)?)
}
