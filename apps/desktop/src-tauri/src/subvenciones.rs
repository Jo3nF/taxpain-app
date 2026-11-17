//! Comandos de las subvenciones (G41) y de los datos para la Renta y el Patrimonio del socio
//! (G34). La lógica está en `taxpain_app::subvenciones` y `taxpain_app::renta_socio`. Los
//! asientos piden confirmación en la interfaz y quedan en la auditoría.

use serde::Serialize;
use taxpain_app::renta_socio::{self, InformeSocio, Persona};
use taxpain_app::subvenciones::{self, Accion, Resumen, Subvencion};
use taxpain_core::accounting::entry::JournalEntry;
use taxpain_core::money::Money;
use time::OffsetDateTime;

use crate::CmdResult;
use crate::salida;
use crate::state::AppState;

fn ahora() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

#[tauri::command]
pub fn subvenciones(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<Resumen> {
    Ok(subvenciones::resumen(&*state.vault()?, year)?)
}

#[tauri::command]
pub fn subvencion_guardar(
    state: tauri::State<'_, AppState>,
    subvencion: Subvencion,
) -> CmdResult<i64> {
    Ok(subvenciones::guardar(
        &*state.vault()?,
        &subvencion,
        ahora(),
    )?)
}

#[tauri::command]
pub fn subvencion_borrar(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(subvenciones::borrar(
        &*state.vault()?,
        id,
        ahora(),
        confirmed,
    )?)
}

/// El asiento que saldría, para enseñarlo antes de confirmar.
#[derive(Serialize)]
pub struct PropuestaSubvencion {
    pub asiento: JournalEntry,
    pub importe: Money,
}

#[tauri::command]
pub fn subvencion_propuesta(
    state: tauri::State<'_, AppState>,
    id: i64,
    accion: Accion,
) -> CmdResult<PropuestaSubvencion> {
    let (asiento, importe) = subvenciones::propuesta_asiento(&*state.vault()?, id, &accion)?;
    Ok(PropuestaSubvencion { asiento, importe })
}

#[tauri::command]
pub fn subvencion_registrar(
    state: tauri::State<'_, AppState>,
    id: i64,
    accion: Accion,
    confirmed: bool,
) -> CmdResult<i64> {
    Ok(subvenciones::registrar(
        &*state.vault()?,
        id,
        &accion,
        ahora(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn subvencion_deshacer(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(subvenciones::deshacer(
        &*state.vault()?,
        id,
        ahora(),
        confirmed,
    )?)
}

// ---------- Datos para la Renta y el Patrimonio del socio ----------

#[tauri::command]
pub fn renta_socio_personas(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Persona>> {
    Ok(renta_socio::personas(&*state.vault()?)?)
}

#[tauri::command]
pub fn renta_socio(
    state: tauri::State<'_, AppState>,
    year: i32,
    nif: String,
    otros: Option<Money>,
) -> CmdResult<InformeSocio> {
    Ok(renta_socio::informe(
        &*state.vault()?,
        year,
        &nif,
        otros,
        crate::calendar::today(),
    )?)
}

/// Guarda el informe en PDF en la ruta elegida.
#[tauri::command]
pub fn renta_socio_save(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    year: i32,
    nif: String,
    otros: Option<Money>,
    path: String,
) -> CmdResult<()> {
    let path = salida::salida_permitida(&app, &path, &["pdf"])?;
    let (_, bytes) = renta_socio::pdf(
        &*state.vault()?,
        year,
        &nif,
        otros,
        crate::calendar::today(),
    )?;
    salida::escribir(&path, bytes)?;
    Ok(())
}
