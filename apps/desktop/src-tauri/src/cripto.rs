//! Comandos de Criptomonedas (la lógica está en `taxpain-app::cripto`).

use taxpain_app::cripto::{
    self, Asset, Op, Platform, Price, Report, Settings,
    asientos::{self, Contabilizado},
    importar::{self, Lectura},
    inicio::{self, Inicio, Propuesta},
};
use time::{Date, OffsetDateTime};

use crate::state::AppState;
use crate::{CmdResult, read_limited};

/// Tope de los ficheros que se eligen con el diálogo.
const MAX_FILE: u64 = 64 * 1024 * 1024;

#[tauri::command]
pub fn cripto_settings(state: tauri::State<'_, AppState>) -> CmdResult<Settings> {
    Ok(cripto::settings(&*state.vault()?)?)
}

#[tauri::command]
pub fn cripto_save_settings(
    state: tauri::State<'_, AppState>,
    settings: Settings,
) -> CmdResult<()> {
    Ok(cripto::save_settings(
        &*state.vault()?,
        &settings,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn cripto_assets(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Asset>> {
    Ok(cripto::assets(&*state.vault()?)?)
}

#[tauri::command]
pub fn cripto_save_asset(
    state: tauri::State<'_, AppState>,
    ticker: String,
    name: String,
) -> CmdResult<i64> {
    Ok(cripto::save_asset(
        &*state.vault()?,
        &ticker,
        &name,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn cripto_platforms(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Platform>> {
    Ok(cripto::platforms(&*state.vault()?)?)
}

#[tauri::command]
pub fn cripto_save_platform(
    state: tauri::State<'_, AppState>,
    platform: Platform,
) -> CmdResult<i64> {
    Ok(cripto::save_platform(
        &*state.vault()?,
        &platform,
        OffsetDateTime::now_utc(),
    )?)
}

/// Touch ID con el nombre de la plataforma.
#[tauri::command(async)]
pub fn cripto_delete_platform(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    crate::confirmar::borrar_plataforma_cripto(&state, id)
}

#[tauri::command]
pub fn cripto_ops(state: tauri::State<'_, AppState>, year: Option<i32>) -> CmdResult<Vec<Op>> {
    Ok(cripto::ops(&*state.vault()?, year)?)
}

#[tauri::command]
pub fn cripto_save_op(state: tauri::State<'_, AppState>, op: Op) -> CmdResult<i64> {
    Ok(cripto::save_op(
        &*state.vault()?,
        &op,
        OffsetDateTime::now_utc(),
    )?)
}

/// Touch ID con la fecha, el tipo y la moneda de la operación.
#[tauri::command(async)]
pub fn cripto_delete_op(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    crate::confirmar::borrar_op_cripto(&state, id)
}

#[tauri::command]
pub fn cripto_prices(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Price>> {
    Ok(cripto::prices(&*state.vault()?)?)
}

#[tauri::command]
pub fn cripto_set_prices(
    state: tauri::State<'_, AppState>,
    prices: Vec<Price>,
) -> CmdResult<usize> {
    Ok(cripto::set_prices(
        &*state.vault()?,
        &prices,
        OffsetDateTime::now_utc(),
    )?)
}

/// Touch ID con la moneda y la fecha del precio.
#[tauri::command(async)]
pub fn cripto_delete_price(
    state: tauri::State<'_, AppState>,
    date: Date,
    ticker: String,
) -> CmdResult<()> {
    crate::confirmar::borrar_precio_cripto(&state, date, &ticker)
}

#[tauri::command]
pub fn cripto_report(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<Report> {
    Ok(cripto::report(&*state.vault()?, year)?)
}

/// Lee un fichero de operaciones o de saldos (no guarda nada).
#[tauri::command]
pub fn cripto_read_file(state: tauri::State<'_, AppState>, path: String) -> CmdResult<Lectura> {
    let p = std::path::Path::new(&path);
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let bytes = read_limited(p, MAX_FILE)?;
    Ok(importar::leer(&*state.vault()?, &name, &bytes)?)
}

/// Guarda lo confirmado de una lectura: (operaciones nuevas, duplicadas, precios).
#[tauri::command]
pub fn cripto_import(
    state: tauri::State<'_, AppState>,
    lectura: Lectura,
) -> CmdResult<(usize, usize, usize)> {
    Ok(importar::aplicar(
        &*state.vault()?,
        &lectura,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn cripto_post_entries(
    state: tauri::State<'_, AppState>,
    year: i32,
    redo: bool,
) -> CmdResult<Contabilizado> {
    Ok(asientos::contabilizar(
        &*state.vault()?,
        year,
        redo,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn cripto_impairment(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<Option<i64>> {
    Ok(asientos::deterioro(
        &*state.vault()?,
        year,
        OffsetDateTime::now_utc(),
    )?)
}

/// Guarda el informe en `dir` como XLSX o PDF; devuelve la ruta.
#[tauri::command]
pub fn cripto_export(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    year: i32,
    dir: String,
    pdf: bool,
) -> CmdResult<String> {
    let v = state.vault()?;
    let (name, bytes) = if pdf {
        cripto::export::pdf_bytes(&v, year)?
    } else {
        cripto::export::workbook(&v, year)?
    };
    let path = crate::salida::en_carpeta(&app, &dir, &name, &["pdf", "xlsx"])?;
    crate::salida::escribir(&path, bytes)?;
    Ok(path.to_string_lossy().into_owned())
}

/// Punto de partida (posiciones y coste a la fecha de corte), si lo hay.
#[tauri::command]
pub fn cripto_inicio(state: tauri::State<'_, AppState>) -> CmdResult<Option<Inicio>> {
    Ok(inicio::inicio(&*state.vault()?)?)
}

#[tauri::command]
/// Guarda el punto de partida; devuelve avisos si afecta a asientos ya hechos.
pub fn cripto_save_inicio(
    state: tauri::State<'_, AppState>,
    inicio: Inicio,
) -> CmdResult<Vec<String>> {
    Ok(inicio::guardar(
        &*state.vault()?,
        &inicio,
        OffsetDateTime::now_utc(),
    )?)
}

/// Quita el punto de partida: Touch ID con su fecha.
#[tauri::command(async)]
pub fn cripto_delete_inicio(state: tauri::State<'_, AppState>) -> CmdResult<Vec<String>> {
    crate::confirmar::borrar_inicio_cripto(&state)
}

/// Propuesta con los cálculos de la gestoría (diario, hoja de saldos y 200); no guarda nada.
/// `cuenta`: la del diario donde está el coste, si el usuario la sabe.
#[tauri::command]
pub fn cripto_propuesta_inicio(
    state: tauri::State<'_, AppState>,
    cuenta: Option<String>,
) -> CmdResult<Propuesta> {
    Ok(inicio::propuesta(
        &*state.vault()?,
        OffsetDateTime::now_utc().date(),
        cuenta.as_deref(),
    )?)
}
