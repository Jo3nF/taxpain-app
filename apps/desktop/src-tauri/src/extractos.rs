//! Comandos de los extractos importados: la lista por cuenta con sus huecos y deshacer uno
//! importado por error (la lógica está en `taxpain-app::extractos_importados`).

use taxpain_app::extractos_importados::{self, Deshecho, ListaExtractos};
use time::OffsetDateTime;

use crate::CmdResult;
use crate::state::AppState;

/// Extractos importados (de la cuenta `cuenta` o de todas) con los huecos entre ellos.
#[tauri::command]
pub fn bank_statements(
    state: tauri::State<'_, AppState>,
    cuenta: Option<i64>,
) -> CmdResult<ListaExtractos> {
    Ok(extractos_importados::lista(&*state.vault()?, cuenta)?)
}

/// Deshace la importación de un extracto: borra los movimientos que sólo traía él; con
/// `desconciliar`, deshace antes las conciliaciones de los que estaban conciliados.
#[tauri::command]
pub fn bank_statement_undo(
    state: tauri::State<'_, AppState>,
    id: i64,
    desconciliar: bool,
    confirmed: bool,
) -> CmdResult<Deshecho> {
    Ok(extractos_importados::deshacer(
        &*state.vault()?,
        id,
        desconciliar,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}
