//! Comandos de los expedientes (requerimientos, liquidaciones, sanciones, apremios) y de los
//! certificados con caducidad. La lógica está en `taxpain_app::expedientes`. Nada se presenta.

use taxpain_app::expedientes::certificados::{self, CertificadoAdmin, CertificadoVista};
use taxpain_app::expedientes::escritos::{self, Escrito, Opciones};
use taxpain_app::expedientes::{self, Actuacion, Aviso, Calculo, Expediente, Vista};
use time::OffsetDateTime;

use crate::calendar::today;
use crate::state::AppState;
use crate::{CmdResult, salida};

#[tauri::command]
pub fn expedientes_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Vista>> {
    Ok(expedientes::list(&*state.vault()?, today())?)
}

/// Plazos de un expediente sin guardarlo (mientras se teclea).
#[tauri::command]
pub fn expediente_preview(
    state: tauri::State<'_, AppState>,
    expediente: Expediente,
) -> CmdResult<Calculo> {
    Ok(expedientes::preview(
        &*state.vault()?,
        &expediente,
        today(),
    )?)
}

#[tauri::command]
pub fn expediente_create(
    state: tauri::State<'_, AppState>,
    expediente: Expediente,
) -> CmdResult<i64> {
    Ok(expedientes::create(
        &*state.vault()?,
        &expediente,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn expediente_update(
    state: tauri::State<'_, AppState>,
    id: i64,
    expediente: Expediente,
) -> CmdResult<()> {
    Ok(expedientes::update(
        &*state.vault()?,
        id,
        &expediente,
        OffsetDateTime::now_utc(),
    )?)
}

/// Borra el expediente (con confirmación y Touch ID).
#[tauri::command]
pub fn expediente_delete(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    crate::confirmar::borrar_expediente(&state, id, confirmed)
}

/// Borrador desde una notificación del buzón de la AEAT (no se guarda).
#[tauri::command]
pub fn expediente_desde_notificacion(
    state: tauri::State<'_, AppState>,
    notice_id: String,
) -> CmdResult<Expediente> {
    Ok(expedientes::desde_notificacion(
        &*state.vault()?,
        &notice_id,
    )?)
}

/// Borrador desde un documento de Documentos AEAT (no se guarda).
#[tauri::command]
pub fn expediente_desde_documento(
    state: tauri::State<'_, AppState>,
    document_id: i64,
) -> CmdResult<Expediente> {
    Ok(expedientes::desde_documento(&*state.vault()?, document_id)?)
}

#[tauri::command]
pub fn expediente_actuacion_add(
    state: tauri::State<'_, AppState>,
    id: i64,
    actuacion: Actuacion,
) -> CmdResult<i64> {
    Ok(expedientes::add_actuacion(
        &*state.vault()?,
        id,
        &actuacion,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn expediente_actuacion_delete(
    state: tauri::State<'_, AppState>,
    actuacion: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(expedientes::delete_actuacion(
        &*state.vault()?,
        actuacion,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Nombre propuesto para guardar el escrito.
#[tauri::command]
pub fn expediente_escrito_nombre(
    state: tauri::State<'_, AppState>,
    id: i64,
    escrito: Escrito,
    pdf: bool,
) -> CmdResult<String> {
    Ok(escritos::nombre_fichero(
        &*state.vault()?,
        id,
        escrito,
        if pdf { "pdf" } else { "rtf" },
    )?)
}

/// Guarda el borrador del escrito en RTF o PDF (según la extensión de `path`).
#[tauri::command]
pub fn expediente_escrito_save(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: i64,
    escrito: Escrito,
    opciones: Opciones,
    path: String,
) -> CmdResult<()> {
    let path = salida::salida_permitida(&app, &path, &["rtf", "pdf"])?;
    let pdf = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
    let v = state.vault()?;
    let bytes = escritos::generar(
        &v,
        id,
        escrito,
        &opciones,
        today(),
        pdf,
        OffsetDateTime::now_utc(),
    )?;
    salida::escribir(&path, bytes)?;
    Ok(())
}

/// Avisos del Panel: plazos que acaban pronto y certificados que caducan.
#[tauri::command]
pub fn expedientes_avisos(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Aviso>> {
    Ok(expedientes::avisos(&*state.vault()?, today())?)
}

#[tauri::command]
pub fn certificados_admin_list(
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<CertificadoVista>> {
    Ok(certificados::list(&*state.vault()?, today())?)
}

#[tauri::command]
pub fn certificado_admin_save(
    state: tauri::State<'_, AppState>,
    certificado: CertificadoAdmin,
) -> CmdResult<i64> {
    Ok(certificados::save(
        &*state.vault()?,
        &certificado,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn certificado_admin_desde_documento(
    state: tauri::State<'_, AppState>,
    document_id: i64,
) -> CmdResult<CertificadoAdmin> {
    Ok(certificados::desde_documento(
        &*state.vault()?,
        document_id,
    )?)
}

#[tauri::command]
pub fn certificado_admin_delete(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    crate::confirmar::borrar_certificado_admin(&state, id, confirmed)
}
