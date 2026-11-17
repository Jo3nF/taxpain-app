//! Comandos del cierre de trimestre guiado, de la guía «Dejar la gestoría» y de la lista de
//! obligaciones no fiscales (la lógica está en `taxpain_app::{trimestre, puesta_marcha,
//! obligaciones}`). Sólo leen o guardan lo que marca el usuario (queda en la auditoría): no
//! contabilizan, no presentan y no envían nada.

use taxpain_app::obligaciones::{self, EstadoObligacion, Obligaciones};
use taxpain_app::puesta_marcha::{self, EstadoPaso, Guia};
use taxpain_app::trimestre::{self, CierreTrimestre, Marca, ResumenTrimestre};
use time::{Date, OffsetDateTime};

use crate::CmdResult;
use crate::calendar::today;
use crate::state::AppState;

#[tauri::command]
pub fn cierre_trimestre(
    state: tauri::State<'_, AppState>,
    year: i32,
    quarter: u8,
) -> CmdResult<CierreTrimestre> {
    Ok(trimestre::cierre(&*state.vault()?, year, quarter, today())?)
}

#[tauri::command]
pub fn cierre_trimestre_resumen(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<Vec<ResumenTrimestre>> {
    Ok(trimestre::resumen(&*state.vault()?, year, today())?)
}

/// Marca a mano un paso del trimestre (`None` lo desmarca).
#[tauri::command]
pub fn cierre_trimestre_marcar(
    state: tauri::State<'_, AppState>,
    year: i32,
    quarter: u8,
    paso: String,
    marca: Option<Marca>,
) -> CmdResult<()> {
    Ok(trimestre::marcar(
        &*state.vault()?,
        year,
        quarter,
        &paso,
        marca,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn puesta_marcha(state: tauri::State<'_, AppState>) -> CmdResult<Guia> {
    Ok(puesta_marcha::guia(&*state.vault()?, today())?)
}

#[tauri::command]
pub fn puesta_marcha_marcar(
    state: tauri::State<'_, AppState>,
    paso: String,
    estado: EstadoPaso,
    fecha: Option<Date>,
    nota: String,
) -> CmdResult<()> {
    Ok(puesta_marcha::marcar(
        &*state.vault()?,
        &paso,
        estado,
        fecha,
        &nota,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn obligaciones_no_fiscales(state: tauri::State<'_, AppState>) -> CmdResult<Obligaciones> {
    Ok(obligaciones::lista(&*state.vault()?)?)
}

#[tauri::command]
pub fn obligacion_no_fiscal_marcar(
    state: tauri::State<'_, AppState>,
    id: String,
    estado: EstadoObligacion,
    fecha: Option<Date>,
    nota: String,
) -> CmdResult<()> {
    Ok(obligaciones::marcar(
        &*state.vault()?,
        &id,
        estado,
        fecha,
        &nota,
        OffsetDateTime::now_utc(),
    )?)
}
