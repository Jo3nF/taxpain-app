//! No residentes (modelos 216 y 296) e inmuebles arrendados (115 y 180). La lógica está en
//! `taxpain-app::no_residentes` y `taxpain-app::inmuebles`. Nada se presenta.

use taxpain_app::inmuebles::{self, Inmueble};
use taxpain_app::no_residentes::{
    self, Certificado, NuevaRenta, PropuestaIrnr as Propuesta, Resumen,
};
use time::OffsetDateTime;

use crate::CmdResult;
use crate::state::AppState;

/// Certificados de residencia, rentas apuntadas, dividendos a no residentes y candidatas del año.
#[tauri::command(async)]
pub fn irnr_resumen(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<Resumen> {
    Ok(no_residentes::resumen(
        &*state.vault()?,
        year,
        crate::calendar::today(),
    )?)
}

/// Guarda un certificado de residencia fiscal (nuevo si `id` es 0).
#[tauri::command]
pub fn irnr_guardar_certificado(
    state: tauri::State<'_, AppState>,
    certificado: Certificado,
) -> CmdResult<i64> {
    Ok(no_residentes::guardar_certificado(
        &*state.vault()?,
        &certificado,
        OffsetDateTime::now_utc(),
    )?)
}

/// Quita un certificado de residencia (con la confirmación del diálogo).
#[tauri::command]
pub fn irnr_quitar_certificado(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(no_residentes::quitar_certificado(
        &*state.vault()?,
        id,
        confirmed,
        OffsetDateTime::now_utc(),
    )?)
}

/// Tipo propuesto para una renta (sin guardar).
#[tauri::command]
pub fn irnr_proponer(state: tauri::State<'_, AppState>, renta: NuevaRenta) -> CmdResult<Propuesta> {
    Ok(no_residentes::proponer(&*state.vault()?, &renta)?)
}

/// Apunta una renta pagada a un no residente con el tipo propuesto (con la confirmación del
/// diálogo).
#[tauri::command]
pub fn irnr_apuntar(
    state: tauri::State<'_, AppState>,
    renta: NuevaRenta,
    confirmed: bool,
) -> CmdResult<i64> {
    Ok(no_residentes::apuntar(
        &*state.vault()?,
        &renta,
        confirmed,
        OffsetDateTime::now_utc(),
    )?
    .0)
}

/// Quita una renta apuntada (con la confirmación del diálogo).
#[tauri::command]
pub fn irnr_quitar_renta(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(no_residentes::quitar_renta(
        &*state.vault()?,
        id,
        confirmed,
        OffsetDateTime::now_utc(),
    )?)
}

/// Inmuebles arrendados guardados (uno por arrendador).
#[tauri::command]
pub fn inmuebles_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Inmueble>> {
    Ok(inmuebles::list(&*state.vault()?)?)
}

/// Guarda el inmueble de un arrendador.
#[tauri::command]
pub fn inmueble_guardar(state: tauri::State<'_, AppState>, inmueble: Inmueble) -> CmdResult<()> {
    Ok(inmuebles::save(
        &*state.vault()?,
        &inmueble,
        OffsetDateTime::now_utc(),
    )?)
}

/// Quita el inmueble de un arrendador (con la confirmación del diálogo).
#[tauri::command]
pub fn inmueble_quitar(
    state: tauri::State<'_, AppState>,
    nif: String,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(inmuebles::delete(
        &*state.vault()?,
        &nif,
        confirmed,
        OffsetDateTime::now_utc(),
    )?)
}
