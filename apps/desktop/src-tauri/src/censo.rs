//! Censo (modelo 036): cotejo del censo con el perfil y la contabilidad, y asistente «Quiero
//! comunicar…». La lógica está en `taxpain-app::censo`. Nada se presenta.

use taxpain_app::censo::{self, CensoTecleado, Comunicacion, Cotejo, guia::Guia};
use time::{Date, OffsetDateTime};

use crate::CmdResult;
use crate::state::AppState;

/// Lo que consta en el censo frente al perfil y la contabilidad, con las diferencias.
#[tauri::command(async)]
pub fn censo_cotejo(state: tauri::State<'_, AppState>) -> CmdResult<Cotejo> {
    Ok(censo::cotejo(&*state.vault()?, crate::calendar::today())?)
}

/// Guarda lo tecleado del censo (`None`: vuelve al certificado) y los epígrafes de la actividad.
#[tauri::command]
pub fn censo_guardar(
    state: tauri::State<'_, AppState>,
    tecleado: Option<CensoTecleado>,
    epigrafes_actividad: Vec<String>,
) -> CmdResult<()> {
    Ok(censo::guardar(
        &*state.vault()?,
        tecleado,
        epigrafes_actividad,
        OffsetDateTime::now_utc(),
    )?)
}

/// La guía de un supuesto: apartados del 036, plazo y documentación.
#[tauri::command]
pub fn censo_guia(
    state: tauri::State<'_, AppState>,
    id: String,
    fecha: Option<Date>,
) -> CmdResult<Guia> {
    Ok(censo::guia_de(
        &*state.vault()?,
        &id,
        fecha,
        crate::calendar::today(),
    )?)
}

/// Apunta en el calendario el plazo de comunicar un cambio.
#[tauri::command]
pub fn censo_programar(
    state: tauri::State<'_, AppState>,
    id: String,
    fecha: Option<Date>,
    nota: String,
) -> CmdResult<Comunicacion> {
    Ok(censo::programar(
        &*state.vault()?,
        &id,
        fecha,
        &nota,
        crate::calendar::today(),
        OffsetDateTime::now_utc(),
    )?)
}

/// Marca un aviso como presentado.
#[tauri::command]
pub fn censo_presentada(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    Ok(censo::marcar_hecha(
        &*state.vault()?,
        id,
        OffsetDateTime::now_utc(),
    )?)
}

/// Quita un aviso del calendario (con la confirmación del diálogo).
#[tauri::command]
pub fn censo_quitar(state: tauri::State<'_, AppState>, id: i64, confirmed: bool) -> CmdResult<()> {
    Ok(censo::quitar(
        &*state.vault()?,
        id,
        confirmed,
        OffsetDateTime::now_utc(),
    )?)
}
