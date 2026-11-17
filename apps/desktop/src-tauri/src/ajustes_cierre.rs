//! Comandos de los ajustes de cierre (la lógica está en `taxpain_app::ajustes_cierre`):
//! periodificaciones, facturas pendientes de recibir o de emitir, deterioro de clientes,
//! provisiones y las comprobaciones de saldos anómalos. Cada acción con asiento pide
//! confirmación en la interfaz y queda en la auditoría.

use taxpain_app::ajustes_cierre::{self, Comprobacion, Nuevo, Propuesta, Resumen};
use time::OffsetDateTime;

use crate::CmdResult;
use crate::state::AppState;

fn ahora() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

#[tauri::command]
pub fn ajustes_cierre(
    state: tauri::State<'_, AppState>,
    year: i32,
    todas: bool,
) -> CmdResult<Resumen> {
    Ok(ajustes_cierre::resumen(&*state.vault()?, year, todas)?)
}

#[tauri::command]
pub fn ajustes_cierre_comprobaciones(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<Vec<Comprobacion>> {
    Ok(ajustes_cierre::comprobaciones(&*state.vault()?, year)?)
}

/// El asiento que saldría de un ajuste, sin grabarlo.
#[tauri::command]
pub fn ajustes_cierre_propuesta(
    state: tauri::State<'_, AppState>,
    year: i32,
    nuevo: Nuevo,
) -> CmdResult<Propuesta> {
    Ok(ajustes_cierre::propuesta(&*state.vault()?, year, &nuevo)?)
}

#[tauri::command]
pub fn ajustes_cierre_crear(
    state: tauri::State<'_, AppState>,
    year: i32,
    nuevo: Nuevo,
    confirmed: bool,
) -> CmdResult<i64> {
    Ok(ajustes_cierre::crear(
        &*state.vault()?,
        year,
        &nuevo,
        ahora(),
        confirmed,
    )?)
}

/// Quita un ajuste y su reversión automática (se puede volver a crear: sin Touch ID).
#[tauri::command]
pub fn ajustes_cierre_deshacer(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(ajustes_cierre::deshacer(
        &*state.vault()?,
        id,
        ahora(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn ajustes_cierre_revertir(
    state: tauri::State<'_, AppState>,
    id: i64,
    year: i32,
    confirmed: bool,
) -> CmdResult<i64> {
    Ok(ajustes_cierre::revertir(
        &*state.vault()?,
        id,
        year,
        ahora(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn ajustes_cierre_deshacer_reversion(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(ajustes_cierre::deshacer_reversion(
        &*state.vault()?,
        id,
        ahora(),
        confirmed,
    )?)
}
