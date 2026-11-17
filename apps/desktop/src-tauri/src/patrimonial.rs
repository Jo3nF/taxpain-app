//! Entidad patrimonial (art. 5.2 LIS) y patrimonio neto frente al capital (arts. 327 y 363.1.e
//! LSC): la lógica está en `taxpain-app::patrimonial`.

use taxpain_app::patrimonial::{self, Ajustes, Clase, Informe};
use taxpain_core::money::Money;
use time::{Date, OffsetDateTime};

use crate::CmdResult;
use crate::state::AppState;

/// El cálculo del ejercicio con las partidas, los trimestres y los avisos.
#[tauri::command(async)]
pub fn patrimonial_report(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<Informe> {
    Ok(patrimonial::informe(
        &*state.vault()?,
        year,
        crate::calendar::today(),
    )?)
}

/// Marca la clase de una partida del activo desde el ejercicio `year`.
#[tauri::command]
pub fn patrimonial_mark(
    state: tauri::State<'_, AppState>,
    year: i32,
    cuenta: String,
    clase: Clase,
    motivo: String,
) -> CmdResult<()> {
    Ok(patrimonial::marcar(
        &*state.vault()?,
        year,
        &cuenta,
        clase,
        &motivo,
        OffsetDateTime::now_utc(),
    )?)
}

/// Guarda la tesorería justificada, los beneficios de actividad, los préstamos participativos y
/// si la sociedad es dominante.
#[tauri::command]
pub fn patrimonial_settings(
    state: tauri::State<'_, AppState>,
    year: i32,
    ajustes: Ajustes,
) -> CmdResult<()> {
    Ok(patrimonial::guardar_ajustes(
        &*state.vault()?,
        year,
        &ajustes,
        OffsetDateTime::now_utc(),
    )?)
}

/// Anota la venta de un elemento afecto o de una participación que no computa.
#[tauri::command]
pub fn patrimonial_sale_add(
    state: tauri::State<'_, AppState>,
    fecha: Date,
    importe: i64,
    motivo: String,
) -> CmdResult<i64> {
    Ok(patrimonial::anadir_venta(
        &*state.vault()?,
        fecha,
        Money::from_cents(importe),
        &motivo,
        OffsetDateTime::now_utc(),
    )?)
}

/// Quita una venta anotada (con la confirmación del diálogo).
#[tauri::command]
pub fn patrimonial_sale_delete(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(patrimonial::quitar_venta(
        &*state.vault()?,
        id,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}
