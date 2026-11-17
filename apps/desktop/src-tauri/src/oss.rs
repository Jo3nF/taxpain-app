//! Ventanilla única (OSS, modelos 035 y 369): la lógica está en `taxpain-app::oss`.

use taxpain_app::oss::{self, Ajustes, Borrador369, Clase, Estado, OperacionOss, Tipos, VentaUe};
use taxpain_core::money::Rate;
use time::OffsetDateTime;

use crate::state::AppState;
use crate::{CmdResult, salida};

/// Umbral del art. 73 LIVA, situación y avisos del año.
#[tauri::command(async)]
pub fn oss_estado(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<Estado> {
    Ok(oss::estado(&*state.vault()?, year)?)
}

/// Facturas del año a particulares de otros Estados miembros, con su clase.
#[tauri::command(async)]
pub fn oss_ventas(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<Vec<VentaUe>> {
    Ok(oss::ventas(&*state.vault()?, year)?)
}

/// Clasifica una factura para el umbral (`None`: sin clasificar).
#[tauri::command]
pub fn oss_clasificar(
    state: tauri::State<'_, AppState>,
    invoice_id: i64,
    clase: Option<Clase>,
) -> CmdResult<()> {
    Ok(oss::clasificar(
        &*state.vault()?,
        invoice_id,
        clase,
        OffsetDateTime::now_utc(),
    )?)
}

/// Tabla de tipos generales por Estado miembro (con fecha y fuente) y los confirmados.
#[tauri::command(async)]
pub fn oss_tipos(state: tauri::State<'_, AppState>) -> CmdResult<Tipos> {
    Ok(oss::tipos(&*state.vault()?)?)
}

/// El usuario confirma el tipo general de un país (en centésimas de punto).
#[tauri::command]
pub fn oss_confirmar_tipo(
    state: tauri::State<'_, AppState>,
    pais: String,
    tipo_bp: i32,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(oss::confirmar_tipo(
        &*state.vault()?,
        &pais,
        Rate::from_bp(tipo_bp),
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn oss_ajustes_guardar(state: tauri::State<'_, AppState>, ajustes: Ajustes) -> CmdResult<()> {
    Ok(oss::guardar_ajustes(
        &*state.vault()?,
        &ajustes,
        OffsetDateTime::now_utc(),
    )?)
}

/// Libro de la ventanilla única del trimestre.
#[tauri::command(async)]
pub fn oss_libro(
    state: tauri::State<'_, AppState>,
    year: i32,
    quarter: u8,
) -> CmdResult<Vec<OperacionOss>> {
    Ok(oss::libro(&*state.vault()?, year, quarter)?)
}

/// Borrador del 369 del trimestre.
#[tauri::command(async)]
pub fn oss_369(
    state: tauri::State<'_, AppState>,
    year: i32,
    quarter: u8,
) -> CmdResult<Borrador369> {
    Ok(oss::borrador_369(&*state.vault()?, year, quarter)?)
}

/// Guarda la hoja (XLSX) del 369 y del libro del trimestre en `dir`; devuelve la ruta.
#[tauri::command]
pub fn oss_369_hoja(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    year: i32,
    quarter: u8,
    dir: String,
) -> CmdResult<String> {
    let (name, bytes) = oss::hoja_369(&*state.vault()?, year, quarter)?;
    let path = salida::en_carpeta(&app, &dir, &name, &["xlsx"])?;
    salida::escribir(&path, bytes)?;
    Ok(path.to_string_lossy().into_owned())
}
