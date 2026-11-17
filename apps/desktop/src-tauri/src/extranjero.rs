//! Modelos 720 y 721: borrador con los registros, datos de cada bien, lo declarado otros años
//! y la hoja de registros en Excel. La lógica está en `taxpain-app::extranjero`. Nada se
//! presenta: TaxPain no tiene el diseño de registro de estos modelos.

use taxpain_app::extranjero::{self, Borrador, DatosBien, Declarado};
use time::OffsetDateTime;

use crate::CmdResult;
use crate::state::AppState;

/// Borrador del 720 o del 721 del ejercicio.
#[tauri::command(async)]
pub fn extranjero_borrador(
    state: tauri::State<'_, AppState>,
    modelo: String,
    year: i32,
) -> CmdResult<Borrador> {
    Ok(extranjero::borrador(
        &*state.vault()?,
        &modelo,
        year,
        crate::calendar::today(),
    )?)
}

/// Guarda los datos de un bien (condición, fechas, entidad, NIF, BIC, domicilio, país).
#[tauri::command]
pub fn extranjero_guardar_bien(
    state: tauri::State<'_, AppState>,
    datos: DatosBien,
) -> CmdResult<()> {
    Ok(extranjero::guardar_bien(
        &*state.vault()?,
        &datos,
        OffsetDateTime::now_utc(),
    )?)
}

/// Guarda lo declarado de un bloque en otro año.
#[tauri::command]
pub fn extranjero_guardar_declarado(
    state: tauri::State<'_, AppState>,
    declarado: Declarado,
) -> CmdResult<()> {
    Ok(extranjero::guardar_declarado(
        &*state.vault()?,
        &declarado,
        OffsetDateTime::now_utc(),
    )?)
}

/// Quita lo declarado de un bloque (con la confirmación del diálogo).
#[tauri::command]
pub fn extranjero_quitar_declarado(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(extranjero::quitar_declarado(
        &*state.vault()?,
        id,
        confirmed,
        OffsetDateTime::now_utc(),
    )?)
}

/// Apunta el borrador como presentado (con la confirmación del diálogo).
#[tauri::command]
pub fn extranjero_marcar_presentado(
    state: tauri::State<'_, AppState>,
    modelo: String,
    year: i32,
    confirmed: bool,
) -> CmdResult<Vec<Declarado>> {
    Ok(extranjero::marcar_presentado(
        &*state.vault()?,
        &modelo,
        year,
        confirmed,
        crate::calendar::today(),
        OffsetDateTime::now_utc(),
    )?)
}

/// Guarda la hoja de registros en `dir`; devuelve la ruta.
#[tauri::command]
pub fn extranjero_exportar(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    modelo: String,
    year: i32,
    dir: String,
) -> CmdResult<String> {
    let v = state.vault()?;
    let (name, bytes) = extranjero::export::workbook(&v, &modelo, year, crate::calendar::today())?;
    let path = crate::salida::en_carpeta(&app, &dir, &name, &["xlsx"])?;
    crate::salida::escribir(&path, bytes)?;
    Ok(path.to_string_lossy().into_owned())
}
