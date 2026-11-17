//! Previsión de tesorería (sólo lectura, todo en local).

use taxpain_app::forecast::{self, Forecast};

use crate::CmdResult;
use crate::state::AppState;

/// Saldo previsto de bancos y caja desde hoy: 13 semanas y 12 meses.
#[tauri::command]
pub fn cash_forecast(state: tauri::State<'_, AppState>) -> CmdResult<Forecast> {
    let today = crate::calendar::today();
    Ok(forecast::forecast(&*state.vault()?, today)?)
}
