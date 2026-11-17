//! Comandos del modelo 200 completo y de la ficha fiscal de las participadas (la lógica está en
//! `taxpain_app::modelo200`). Nada se presenta: el fichero se importa en Sociedades WEB.

use taxpain_app::modelo200::participadas::{
    self, DatosFiscales, Dividendo, FondosParticipada, ParticipadaVista,
};
use taxpain_app::modelo200::{
    self, AjusteGuardado, ArrastreGuardado, Datos200, Hoja200, TipoArrastre,
};
use time::OffsetDateTime;

use crate::CmdResult;
use crate::state::AppState;

fn ahora() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

/// Hoja del 200 del ejercicio: bloques con cada cifra, su origen y su norma.
#[tauri::command]
pub fn modelo200_hoja(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<Hoja200> {
    Ok(modelo200::hoja(&*state.vault()?, year)?)
}

/// Cambia o descarta una corrección propuesta, o añade una a mano; devuelve su clave.
#[tauri::command]
pub fn modelo200_ajuste_guardar(
    state: tauri::State<'_, AppState>,
    year: i32,
    ajuste: AjusteGuardado,
) -> CmdResult<String> {
    Ok(modelo200::guardar_ajuste(
        &*state.vault()?,
        year,
        &ajuste,
        ahora(),
    )?)
}

/// Vuelve a la propuesta de TaxPain o quita la corrección tecleada (con confirmación).
#[tauri::command]
pub fn modelo200_ajuste_quitar(
    state: tauri::State<'_, AppState>,
    year: i32,
    clave: String,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(modelo200::quitar_ajuste(
        &*state.vault()?,
        year,
        &clave,
        ahora(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn modelo200_datos_guardar(
    state: tauri::State<'_, AppState>,
    year: i32,
    datos: Datos200,
) -> CmdResult<()> {
    Ok(modelo200::guardar_datos(
        &*state.vault()?,
        year,
        &datos,
        ahora(),
    )?)
}

/// Guarda una base negativa, reserva o deducción pendiente de un ejercicio de origen.
#[tauri::command]
pub fn modelo200_arrastre_guardar(
    state: tauri::State<'_, AppState>,
    arrastre: ArrastreGuardado,
) -> CmdResult<()> {
    Ok(modelo200::guardar_arrastre(
        &*state.vault()?,
        &arrastre,
        ahora(),
    )?)
}

#[tauri::command]
pub fn modelo200_arrastre_borrar(
    state: tauri::State<'_, AppState>,
    tipo: TipoArrastre,
    origen: i32,
    ejercicio: i32,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(modelo200::borrar_arrastre(
        &*state.vault()?,
        tipo,
        origen,
        ejercicio,
        ahora(),
        confirmed,
    )?)
}

/// Guarda lo pendiente tras el 200 de `year` como arrastres del ejercicio siguiente.
#[tauri::command]
pub fn modelo200_pasar_siguiente(
    state: tauri::State<'_, AppState>,
    year: i32,
    confirmed: bool,
) -> CmdResult<usize> {
    Ok(modelo200::pasar_al_siguiente(
        &*state.vault()?,
        year,
        ahora(),
        confirmed,
    )?)
}

/// Fichas fiscales de las participadas con los dividendos del ejercicio y el art. 21.
#[tauri::command]
pub fn participadas_fichas(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<Vec<ParticipadaVista>> {
    Ok(participadas::fichas(&*state.vault()?, year)?)
}

#[tauri::command]
pub fn participada_fiscal_guardar(
    state: tauri::State<'_, AppState>,
    datos: DatosFiscales,
) -> CmdResult<()> {
    Ok(participadas::guardar_fiscal(
        &*state.vault()?,
        &datos,
        ahora(),
    )?)
}

#[tauri::command]
pub fn participada_fondos_guardar(
    state: tauri::State<'_, AppState>,
    fondos: FondosParticipada,
) -> CmdResult<()> {
    Ok(participadas::guardar_fondos(
        &*state.vault()?,
        &fondos,
        ahora(),
    )?)
}

#[tauri::command]
pub fn participada_dividendo_guardar(
    state: tauri::State<'_, AppState>,
    dividendo: Dividendo,
) -> CmdResult<i64> {
    Ok(participadas::guardar_dividendo(
        &*state.vault()?,
        &dividendo,
        ahora(),
    )?)
}

#[tauri::command]
pub fn participada_dividendo_borrar(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(participadas::borrar_dividendo(
        &*state.vault()?,
        id,
        ahora(),
        confirmed,
    )?)
}
