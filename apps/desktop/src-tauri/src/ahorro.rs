//! «Ahorrar impuestos»: recomendaciones según la situación de la empresa y las marcas del usuario.

use crate::CmdResult;
use crate::state::AppState;

/// Las recomendaciones para la empresa abierta (no consulta nada fuera del almacén).
#[tauri::command(async)]
pub fn tax_savings(state: tauri::State<'_, AppState>) -> CmdResult<taxpain_app::ahorro::Ahorro> {
    let vault = state.vault()?;
    Ok(taxpain_app::ahorro::ahorro(
        &vault,
        crate::calendar::today(),
    )?)
}

/// Guarda el estado de una recomendación («en marcha», «hecho», «no me interesa») y sus pasos
/// hechos, para el ejercicio en curso.
#[tauri::command(async)]
pub fn tax_savings_mark(
    state: tauri::State<'_, AppState>,
    id: String,
    marca: taxpain_app::ahorro::Marca,
) -> CmdResult<()> {
    let vault = state.vault()?;
    Ok(taxpain_app::ahorro::marcar(
        &vault,
        crate::calendar::today(),
        &id,
        &marca,
    )?)
}
