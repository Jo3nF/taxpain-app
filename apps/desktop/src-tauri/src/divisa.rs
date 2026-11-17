//! Comandos de moneda extranjera: contravalor de los movimientos de las cuentas en divisa,
//! saldos de las cuentas y valoración al cierre (la lógica está en `taxpain-app::divisa`).

use taxpain_app::divisa::{self, Contravalor, EntradaValoracion, SaldoCuenta, Valoracion};
use taxpain_core::fx::ExchangeRate;
use taxpain_core::money::Money;
use time::OffsetDateTime;

use crate::CmdResult;
use crate::state::AppState;

fn ahora() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

/// Contravalor tecleado de un movimiento de una cuenta en divisa (los euros o el tipo); sin
/// ninguno, se quita.
#[tauri::command]
pub fn bank_set_fx(
    state: tauri::State<'_, AppState>,
    tx: i64,
    eur: Option<Money>,
    rate: Option<String>,
) -> CmdResult<Option<Contravalor>> {
    let rate = rate
        .filter(|r| !r.trim().is_empty())
        .map(|r| {
            ExchangeRate::parse(&r).ok_or_else(|| {
                taxpain_app::error::AppError::Other(format!("tipo de cambio no válido: {r}"))
            })
        })
        .transpose()?;
    Ok(divisa::set_contravalor(
        &*state.vault()?,
        tx,
        eur,
        rate,
        ahora(),
    )?)
}

/// Saldos de las cuentas del banco hoy, en su divisa y en euros.
#[tauri::command]
pub fn bank_balances(state: tauri::State<'_, AppState>) -> CmdResult<Vec<SaldoCuenta>> {
    Ok(divisa::saldos(
        &*state.vault()?,
        taxpain_core::spain_time::peninsular_date(ahora()),
    )?)
}

#[tauri::command]
pub fn closing_fx_valuation(
    state: tauri::State<'_, AppState>,
    year: i32,
    input: Option<EntradaValoracion>,
) -> CmdResult<Valoracion> {
    Ok(divisa::valoracion(&*state.vault()?, year, input)?)
}

#[tauri::command]
pub fn closing_fx_valuation_post(
    state: tauri::State<'_, AppState>,
    year: i32,
    input: EntradaValoracion,
    confirmed: bool,
) -> CmdResult<i64> {
    Ok(divisa::post_valoracion(
        &*state.vault()?,
        year,
        input,
        ahora(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn closing_fx_valuation_undo(
    state: tauri::State<'_, AppState>,
    year: i32,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(divisa::undo_valoracion(
        &*state.vault()?,
        year,
        ahora(),
        confirmed,
    )?)
}
