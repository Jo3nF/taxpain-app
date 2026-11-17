//! Panel financiero (sólo lectura, todo en local).

use taxpain_app::dashboard::{self, Dashboard, Receivables};

use crate::CmdResult;
use crate::state::AppState;

#[tauri::command]
pub fn dashboard_get(
    state: tauri::State<'_, AppState>,
    year: i32,
    month: u8,
) -> CmdResult<Dashboard> {
    Ok(dashboard::dashboard(&*state.vault()?, year, month)?)
}

/// Facturas vencidas sin cobrar (sin abonos) y presupuestos aceptados por facturar, a hoy.
#[tauri::command]
pub fn dashboard_receivables(state: tauri::State<'_, AppState>) -> CmdResult<Receivables> {
    let today = taxpain_core::spain_time::peninsular_date(time::OffsetDateTime::now_utc());
    Ok(dashboard::receivables(&*state.vault()?, today)?)
}
