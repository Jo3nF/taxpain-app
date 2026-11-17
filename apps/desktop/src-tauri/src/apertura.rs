//! Comandos del asistente de saldos iniciales (la lógica está en `taxpain-app::apertura`).

use taxpain_app::apertura::{
    self, Applied, BalanceRead, BankInfo, Draft, PendingRead, Prior, Summary,
};
use taxpain_core::accounting::depreciation::FixedAsset;
use taxpain_core::money::Money;
use time::{Date, OffsetDateTime};

use crate::state::AppState;
use crate::{CmdResult, read_limited};

/// Tope de los ficheros del gestor que se eligen con el diálogo.
const MAX_FILE: u64 = 64 * 1024 * 1024;

pub(crate) fn read_file(path: &str) -> CmdResult<(String, Vec<u8>)> {
    let p = std::path::Path::new(path);
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok((name, read_limited(p, MAX_FILE)?))
}

#[tauri::command]
pub fn apertura_draft(state: tauri::State<'_, AppState>) -> CmdResult<Draft> {
    Ok(apertura::draft(&*state.vault()?)?)
}

#[tauri::command]
pub fn apertura_save(state: tauri::State<'_, AppState>, draft: Draft) -> CmdResult<()> {
    Ok(apertura::save_draft(&*state.vault()?, &draft)?)
}

#[tauri::command]
pub fn apertura_discard(state: tauri::State<'_, AppState>) -> CmdResult<()> {
    Ok(apertura::discard_draft(&*state.vault()?)?)
}

/// Lee el balance de sumas y saldos o el diario del gestor (no cambia nada).
#[tauri::command]
pub fn apertura_read_balance(path: String) -> CmdResult<BalanceRead> {
    let (name, bytes) = read_file(&path)?;
    Ok(apertura::read_balance(&name, &bytes)?)
}

/// Lee los pendientes de cobro (`emitida`) o pago (`recibida`) de un listado o un libro.
#[tauri::command]
pub fn apertura_read_pending(
    state: tauri::State<'_, AppState>,
    kind: String,
    path: String,
) -> CmdResult<PendingRead> {
    let (name, bytes) = read_file(&path)?;
    Ok(apertura::read_pending(
        &*state.vault()?,
        &kind,
        &name,
        &bytes,
    )?)
}

#[tauri::command]
pub fn apertura_banks(
    state: tauri::State<'_, AppState>,
    date: Option<Date>,
) -> CmdResult<Vec<BankInfo>> {
    Ok(apertura::banks(&*state.vault()?, date)?)
}

#[tauri::command]
pub fn apertura_summary(state: tauri::State<'_, AppState>, draft: Draft) -> CmdResult<Summary> {
    Ok(apertura::summary(&*state.vault()?, &draft)?)
}

#[tauri::command]
pub fn apertura_apply(
    state: tauri::State<'_, AppState>,
    draft: Draft,
    allow_duplicate: bool,
) -> CmdResult<Applied> {
    Ok(apertura::apply(
        &*state.vault()?,
        &draft,
        allow_duplicate,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn apertura_applied(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Applied>> {
    Ok(apertura::applied(&*state.vault()?)?)
}

#[tauri::command]
pub fn apertura_undo(state: tauri::State<'_, AppState>, id: i64, confirmed: bool) -> CmdResult<()> {
    Ok(apertura::undo(
        &*state.vault()?,
        id,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Alta de un elemento del inmovilizado con lo amortizado antes de TaxPain (CONTABILIDAD-05).
#[tauri::command]
pub fn fixed_asset_add_with_prior(
    state: tauri::State<'_, AppState>,
    asset: FixedAsset,
    prior: Money,
    until: Option<Date>,
) -> CmdResult<i64> {
    Ok(apertura::add_asset_with_prior(
        &*state.vault()?,
        &asset,
        prior,
        until,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn fixed_asset_priors(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Prior>> {
    Ok(apertura::priors(&*state.vault()?)?)
}
