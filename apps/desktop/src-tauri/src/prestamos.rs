//! Comandos de préstamos concedidos y recibidos (la lógica está en `taxpain_app::prestamos`).

use taxpain_app::prestamos::{self, Devengado, Movimiento, Pago, PrestamoVista, Propuesta};
use taxpain_core::accounting::prestamo::{Contraparte, Prestamo, Sentido};
use time::{Date, OffsetDateTime};

use crate::state::AppState;
use crate::{CmdResult, salida};

fn hoy() -> Date {
    taxpain_core::spain_time::peninsular_date(OffsetDateTime::now_utc())
}

#[tauri::command]
pub fn prestamos_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<PrestamoVista>> {
    Ok(prestamos::list(&*state.vault()?, hoy())?)
}

/// Cuentas y retención que se proponen para un préstamo con esa contraparte.
#[tauri::command]
pub fn prestamo_propuesta(sentido: Sentido, contraparte: Contraparte) -> Propuesta {
    prestamos::propuesta(sentido, &contraparte)
}

#[tauri::command]
pub fn prestamo_create(state: tauri::State<'_, AppState>, prestamo: Prestamo) -> CmdResult<i64> {
    Ok(prestamos::create(
        &*state.vault()?,
        &prestamo,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn prestamo_update(
    state: tauri::State<'_, AppState>,
    id: i64,
    prestamo: Prestamo,
) -> CmdResult<()> {
    Ok(prestamos::update(
        &*state.vault()?,
        id,
        &prestamo,
        OffsetDateTime::now_utc(),
    )?)
}

/// Movimientos del banco sin conciliar que pueden ser los de la cuota `n` (0: el alta).
#[tauri::command]
pub fn prestamo_bank_candidates(
    state: tauri::State<'_, AppState>,
    id: i64,
    n: u16,
) -> CmdResult<Vec<Movimiento>> {
    Ok(prestamos::bank_candidates(&*state.vault()?, id, n, hoy())?)
}

/// Contabiliza el alta (con confirmación).
#[tauri::command]
pub fn prestamo_post_start(
    state: tauri::State<'_, AppState>,
    id: i64,
    pago: Pago,
    confirmed: bool,
) -> CmdResult<i64> {
    Ok(prestamos::post_start(
        &*state.vault()?,
        id,
        &pago,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Marca el alta como ya contabilizada fuera de TaxPain (o lo deshace).
#[tauri::command]
pub fn prestamo_start_outside(
    state: tauri::State<'_, AppState>,
    id: i64,
    fuera: bool,
) -> CmdResult<()> {
    Ok(prestamos::mark_start_outside(
        &*state.vault()?,
        id,
        fuera,
        OffsetDateTime::now_utc(),
    )?)
}

/// Deshace el asiento del alta (con confirmación).
#[tauri::command]
pub fn prestamo_undo_start(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(prestamos::undo_start(
        &*state.vault()?,
        id,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Registra la cuota `n` (con confirmación).
#[tauri::command]
pub fn prestamo_post_installment(
    state: tauri::State<'_, AppState>,
    id: i64,
    n: u16,
    pago: Pago,
    confirmed: bool,
) -> CmdResult<i64> {
    Ok(prestamos::post_installment(
        &*state.vault()?,
        id,
        n,
        &pago,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Marca como ya contabilizadas fuera las cuotas que vencen hasta `hasta` (con confirmación).
#[tauri::command]
pub fn prestamo_mark_outside(
    state: tauri::State<'_, AppState>,
    id: i64,
    hasta: Date,
    confirmed: bool,
) -> CmdResult<usize> {
    Ok(prestamos::mark_outside(
        &*state.vault()?,
        id,
        hasta,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Deshace la última cuota registrada (con confirmación).
#[tauri::command]
pub fn prestamo_undo_installment(
    state: tauri::State<'_, AppState>,
    id: i64,
    n: u16,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(prestamos::undo_installment(
        &*state.vault()?,
        id,
        n,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Devenga a `fecha` los intereses del préstamo `id` (o de todos) que falten (con confirmación).
#[tauri::command]
pub fn prestamo_accrue(
    state: tauri::State<'_, AppState>,
    id: Option<i64>,
    fecha: Date,
    confirmed: bool,
) -> CmdResult<Vec<Devengado>> {
    Ok(prestamos::accrue(
        &*state.vault()?,
        id,
        fecha,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Deshace un devengo de intereses (con confirmación).
#[tauri::command]
pub fn prestamo_undo_accrual(
    state: tauri::State<'_, AppState>,
    devengo: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(prestamos::undo_accrual(
        &*state.vault()?,
        devengo,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Reclasifica al cierre de `year` el capital entre el largo y el corto plazo (con
/// confirmación).
#[tauri::command]
pub fn prestamos_reclassify(
    state: tauri::State<'_, AppState>,
    year: i32,
    confirmed: bool,
) -> CmdResult<usize> {
    Ok(prestamos::reclassify(
        &*state.vault()?,
        year,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Borra un préstamo dado de alta por error (con confirmación y Touch ID).
#[tauri::command(async)]
pub fn prestamo_delete(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    crate::confirmar::borrar_prestamo(&state, id, confirmed)
}

/// Guarda el borrador del contrato de préstamo en RTF (para revisarlo en Pages o Word y
/// firmarlo).
#[tauri::command]
pub fn prestamo_contract_save(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: i64,
    path: String,
) -> CmdResult<()> {
    let path = salida::salida_permitida(&app, &path, &["rtf"])?;
    let v = state.vault()?;
    salida::escribir(&path, prestamos::contract_rtf(&v, id)?)?;
    Ok(())
}
