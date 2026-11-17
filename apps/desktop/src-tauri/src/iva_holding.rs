//! IVA de holding y cartera (afectación, sectores diferenciados y bienes de inversión): la lógica
//! está en `taxpain-app::iva_holding`.

use taxpain_app::iva_holding::{self, Config, DatosBien, Diagnostico, LibroBienes, Regla, Sector};
use time::OffsetDateTime;

use crate::CmdResult;
use crate::state::AppState;

/// Diagnóstico del ejercicio con las reglas, las actividades y lo aplicado.
#[tauri::command(async)]
pub fn iva_holding_informe(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<Diagnostico> {
    Ok(iva_holding::diagnostico(&*state.vault()?, year)?)
}

/// Libro registro de bienes de inversión del ejercicio con su regularización.
#[tauri::command(async)]
pub fn iva_holding_libro(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<LibroBienes> {
    Ok(iva_holding::libro(
        &*state.vault()?,
        year,
        crate::calendar::today(),
    )?)
}

/// Alta o cambio de una regla de afectación.
#[tauri::command]
pub fn iva_holding_regla_guardar(
    state: tauri::State<'_, AppState>,
    regla: Regla,
) -> CmdResult<i64> {
    Ok(iva_holding::guardar_regla(
        &*state.vault()?,
        &regla,
        OffsetDateTime::now_utc(),
    )?)
}

/// Borra una regla (con la confirmación del diálogo).
#[tauri::command]
pub fn iva_holding_regla_borrar(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(iva_holding::borrar_regla(
        &*state.vault()?,
        id,
        confirmed,
        OffsetDateTime::now_utc(),
    )?)
}

/// Alta o cambio de una actividad (sector).
#[tauri::command]
pub fn iva_holding_sector_guardar(
    state: tauri::State<'_, AppState>,
    sector: Sector,
) -> CmdResult<i64> {
    Ok(iva_holding::guardar_sector(
        &*state.vault()?,
        &sector,
        OffsetDateTime::now_utc(),
    )?)
}

/// Borra una actividad (con la confirmación del diálogo).
#[tauri::command]
pub fn iva_holding_sector_borrar(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(iva_holding::borrar_sector(
        &*state.vault()?,
        id,
        confirmed,
        OffsetDateTime::now_utc(),
    )?)
}

/// Separar o no la deducción por sectores diferenciados.
#[tauri::command]
pub fn iva_holding_config(state: tauri::State<'_, AppState>, config: Config) -> CmdResult<()> {
    Ok(iva_holding::guardar_config(
        &*state.vault()?,
        &config,
        OffsetDateTime::now_utc(),
    )?)
}

/// Datos de IVA de un elemento del inmovilizado (cuota, inmueble, cómo tributó su venta).
#[tauri::command]
pub fn iva_holding_bien_guardar(
    state: tauri::State<'_, AppState>,
    asset_id: i64,
    datos: DatosBien,
) -> CmdResult<()> {
    Ok(iva_holding::guardar_bien(
        &*state.vault()?,
        asset_id,
        &datos,
        OffsetDateTime::now_utc(),
    )?)
}
