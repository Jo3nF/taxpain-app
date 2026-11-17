//! Comandos de «Importar un ejercicio entero del gestor» (la lógica está en
//! `taxpain-app::ejercicio_gestor`).

use taxpain_app::apertura::Asset;
use taxpain_app::ejercicio_gestor::{
    self, BalanceComparison, InventoryImported, InventoryPreview, InventoryRead, Lot,
    ModelsComparison, Options, Preview,
};
use time::{Date, OffsetDateTime};

use crate::CmdResult;
use crate::apertura::read_file;
use crate::state::AppState;

/// Lee el diario del gestor y dice qué se importaría (no cambia nada). Mira también si el
/// ejercicio siguiente tiene su propio diario junto al fichero elegido.
#[tauri::command]
pub fn gestor_preview(state: tauri::State<'_, AppState>, path: String) -> CmdResult<Preview> {
    let (_, bytes) = read_file(&path)?;
    Ok(ejercicio_gestor::preview_file_at(
        &*state.vault()?,
        std::path::Path::new(&path),
        &bytes,
    )?)
}

/// Importa el ejercicio `year` del diario, en un lote.
#[tauri::command]
pub fn gestor_import(
    state: tauri::State<'_, AppState>,
    path: String,
    year: i32,
    options: Options,
) -> CmdResult<Lot> {
    let (name, bytes) = read_file(&path)?;
    Ok(ejercicio_gestor::import_file(
        &*state.vault()?,
        year,
        &name,
        &bytes,
        &options,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn gestor_lots(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Lot>> {
    Ok(ejercicio_gestor::lots(&*state.vault()?)?)
}

#[tauri::command]
pub fn gestor_undo(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<usize> {
    Ok(ejercicio_gestor::undo(
        &*state.vault()?,
        id,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Coteja los saldos del ejercicio con el balance de sumas y saldos del gestor.
#[tauri::command]
pub fn gestor_compare_balance(
    state: tauri::State<'_, AppState>,
    year: i32,
    path: String,
) -> CmdResult<BalanceComparison> {
    let (name, bytes) = read_file(&path)?;
    Ok(ejercicio_gestor::compare_balance(
        &*state.vault()?,
        year,
        &name,
        &bytes,
    )?)
}

/// Coteja el diario con los modelos presentados guardados (303 por trimestre y 200).
#[tauri::command]
pub fn gestor_compare_models(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<ModelsComparison> {
    Ok(ejercicio_gestor::compare_models(&*state.vault()?, year)?)
}

/// Lee el inventario de activos del gestor para el asistente de saldos iniciales.
#[tauri::command]
pub fn apertura_read_inventory(path: String) -> CmdResult<InventoryRead> {
    let (name, bytes) = read_file(&path)?;
    Ok(ejercicio_gestor::read_inventory(&name, &bytes)?)
}

/// Lee el inventario de activos del gestor para darlo de alta en Inmovilizado: qué bienes ya
/// están y cuáles no entran (no cambia nada).
#[tauri::command]
pub fn fixed_asset_inventory_preview(
    state: tauri::State<'_, AppState>,
    path: String,
) -> CmdResult<InventoryPreview> {
    let (name, bytes) = read_file(&path)?;
    Ok(ejercicio_gestor::preview_inventory(
        &*state.vault()?,
        &name,
        &bytes,
    )?)
}

/// Da de alta en Inmovilizado los bienes del inventario del gestor, con lo ya amortizado hasta
/// `until` y sin ningún asiento.
#[tauri::command]
pub fn fixed_asset_inventory_import(
    state: tauri::State<'_, AppState>,
    assets: Vec<Asset>,
    until: Date,
) -> CmdResult<InventoryImported> {
    Ok(ejercicio_gestor::import_inventory(
        &*state.vault()?,
        &assets,
        until,
        OffsetDateTime::now_utc(),
    )?)
}
