//! Comandos de Operaciones vinculadas y participadas (la lógica está en
//! `taxpain_app::vinculadas`). Nada se presenta: el 232 sale como hoja para teclearlo en la Sede.

use taxpain_app::vinculadas::{
    self, OperacionManual, Participada, Registro, Textos, Valoracion, Vinculada, documentacion,
    hoja,
};
use time::OffsetDateTime;

use crate::CmdResult;
use crate::salida;
use crate::state::AppState;

fn ahora() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

#[tauri::command]
pub fn vinculadas_registro(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<Registro> {
    Ok(vinculadas::registro(&*state.vault()?, year)?)
}

#[tauri::command]
pub fn vinculada_de(
    state: tauri::State<'_, AppState>,
    nif: String,
) -> CmdResult<Option<Vinculada>> {
    Ok(vinculadas::vinculada_de(&*state.vault()?, &nif)?)
}

#[tauri::command]
pub fn vinculada_save(state: tauri::State<'_, AppState>, vinculada: Vinculada) -> CmdResult<i64> {
    Ok(vinculadas::save_vinculada(
        &*state.vault()?,
        &vinculada,
        ahora(),
    )?)
}

/// Quita una vinculada (confirmación en la interfaz; queda en la auditoría).
#[tauri::command]
pub fn vinculada_delete(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    Ok(vinculadas::delete_vinculada(&*state.vault()?, id, ahora())?)
}

#[tauri::command]
pub fn participadas_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Participada>> {
    Ok(vinculadas::participadas(&*state.vault()?)?)
}

#[tauri::command]
pub fn participada_save(
    state: tauri::State<'_, AppState>,
    participada: Participada,
) -> CmdResult<i64> {
    Ok(vinculadas::save_participada(
        &*state.vault()?,
        &participada,
        ahora(),
    )?)
}

#[tauri::command]
pub fn participada_delete(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    Ok(vinculadas::delete_participada(
        &*state.vault()?,
        id,
        ahora(),
    )?)
}

#[tauri::command]
pub fn vinculada_cuenta_set(
    state: tauri::State<'_, AppState>,
    account: String,
    nif: Option<String>,
) -> CmdResult<()> {
    Ok(vinculadas::set_cuenta(
        &*state.vault()?,
        &account,
        nif.as_deref(),
        ahora(),
    )?)
}

#[tauri::command]
pub fn vinculada_operacion_save(
    state: tauri::State<'_, AppState>,
    operacion: OperacionManual,
) -> CmdResult<i64> {
    Ok(vinculadas::save_operacion(
        &*state.vault()?,
        &operacion,
        ahora(),
    )?)
}

#[tauri::command]
pub fn vinculada_operacion_delete(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    Ok(vinculadas::delete_operacion(&*state.vault()?, id, ahora())?)
}

#[tauri::command]
pub fn vinculada_valoracion_save(
    state: tauri::State<'_, AppState>,
    valoracion: Valoracion,
) -> CmdResult<()> {
    Ok(vinculadas::save_valoracion(
        &*state.vault()?,
        &valoracion,
        ahora(),
    )?)
}

#[tauri::command]
pub fn vinculadas_textos_save(state: tauri::State<'_, AppState>, textos: Textos) -> CmdResult<()> {
    Ok(vinculadas::save_textos(&*state.vault()?, &textos, ahora())?)
}

/// Guarda la documentación simplificada de `year` en PDF o RTF (ruta elegida con el diálogo).
#[tauri::command]
pub fn vinculadas_documentacion_save(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    year: i32,
    path: String,
    pdf: bool,
) -> CmdResult<()> {
    let path = salida::salida_permitida(&app, &path, &[if pdf { "pdf" } else { "rtf" }])?;
    let v = state.vault()?;
    if pdf {
        salida::escribir(&path, documentacion::pdf(&v, year)?)?;
    } else {
        salida::escribir(&path, documentacion::rtf(&v, year)?)?;
    }
    Ok(())
}

/// Guarda el borrador del 232 de `year` como hoja de cálculo (ruta elegida con el diálogo).
#[tauri::command]
pub fn vinculadas_hoja_232_save(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    year: i32,
    path: String,
) -> CmdResult<()> {
    let path = salida::salida_permitida(&app, &path, &["xlsx"])?;
    let (_, bytes) = hoja::workbook(&*state.vault()?, year)?;
    salida::escribir(&path, bytes)?;
    Ok(())
}
