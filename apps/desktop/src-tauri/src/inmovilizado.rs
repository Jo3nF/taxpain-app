//! Comandos para corregir el inmovilizado (editar, borrar, deshacer bajas y amortizaciones) y
//! sus incentivos fiscales (la lógica está en `taxpain-app::inmovilizado`).

use taxpain_app::accounting::{self, DepreciationDifference};
use taxpain_app::inmovilizado::{
    self, AmortizacionDeshecha, BajaDeshecha, CambiosBien, Incentivo, IncentivoBien,
    PrevisionAmortizacion, UsoIncentivos,
};
use taxpain_core::money::Rate;
use time::OffsetDateTime;

use crate::CmdResult;
use crate::state::AppState;

fn ahora() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

/// Cambia el nombre, la fecha de puesta en funcionamiento, el coeficiente y el valor residual.
#[tauri::command]
pub fn asset_edit(
    state: tauri::State<'_, AppState>,
    id: i64,
    cambios: CambiosBien,
) -> CmdResult<Vec<String>> {
    Ok(inmovilizado::edit_asset(
        &*state.vault()?,
        id,
        &cambios,
        ahora(),
    )?)
}

/// Borra un elemento sin asientos (dado de alta por error).
#[tauri::command]
pub fn asset_delete(state: tauri::State<'_, AppState>, id: i64, confirmed: bool) -> CmdResult<()> {
    Ok(inmovilizado::delete_asset(
        &*state.vault()?,
        id,
        ahora(),
        confirmed,
    )?)
}

/// Deshace la baja o la venta de un elemento.
#[tauri::command]
pub fn asset_undispose(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<BajaDeshecha> {
    Ok(inmovilizado::undo_disposal(
        &*state.vault()?,
        id,
        ahora(),
        confirmed,
    )?)
}

/// Lo que registraría «Registrar amortizaciones» del ejercicio y lo ya registrado.
#[tauri::command]
pub fn depreciation_preview(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<PrevisionAmortizacion> {
    Ok(inmovilizado::depreciation_preview(&*state.vault()?, year)?)
}

/// Deshace la amortización registrada del ejercicio (de todos o de un elemento).
#[tauri::command]
pub fn depreciation_undo(
    state: tauri::State<'_, AppState>,
    year: i32,
    asset: Option<i64>,
    confirmed: bool,
) -> CmdResult<AmortizacionDeshecha> {
    Ok(inmovilizado::undo_depreciation(
        &*state.vault()?,
        year,
        asset,
        ahora(),
        confirmed,
    )?)
}

/// Los incentivos fiscales de los elementos.
#[tauri::command]
pub fn asset_incentives(state: tauri::State<'_, AppState>) -> CmdResult<Vec<IncentivoBien>> {
    Ok(inmovilizado::incentives(&*state.vault()?)?)
}

/// Uso de los topes de los incentivos del ejercicio.
#[tauri::command]
pub fn asset_incentive_usage(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<UsoIncentivos> {
    Ok(inmovilizado::incentive_usage(&*state.vault()?, year)?)
}

/// Pone o quita el incentivo fiscal de un elemento (con su coeficiente fiscal, en puntos
/// básicos).
#[tauri::command]
pub fn asset_set_incentive(
    state: tauri::State<'_, AppState>,
    id: i64,
    incentivo: Option<Incentivo>,
    tax_rate_bp: Option<i32>,
) -> CmdResult<Vec<String>> {
    Ok(inmovilizado::set_incentive(
        &*state.vault()?,
        id,
        incentivo,
        tax_rate_bp.map(Rate::from_bp),
        ahora(),
    )?)
}

/// Diferencias entre la amortización contable y la fiscal de cada elemento en el ejercicio (las
/// que van al 200), con la reversión de los dados de baja.
#[tauri::command]
pub fn depreciation_differences(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<Vec<DepreciationDifference>> {
    Ok(accounting::depreciation_differences_detail(
        &*state.vault()?,
        year,
    )?)
}
