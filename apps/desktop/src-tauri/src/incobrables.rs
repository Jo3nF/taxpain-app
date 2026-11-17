//! Créditos incobrables (art. 80.Cuatro LIVA) y control de numeración de las emitidas. La lógica
//! está en `taxpain_app::incobrables` y `taxpain_app::rectificativas_emitidas`.

use taxpain_app::incobrables::{self, Comunicacion, Credito, Exclusion, Medio, PropuestaR3};
use taxpain_app::invoicing::IssuedInvoice;
use taxpain_app::rectificativas_emitidas::{self, SerieNumeracion};
use time::{Date, OffsetDateTime};

use crate::state::AppState;
use crate::{CmdResult, CommandError};

#[tauri::command]
pub fn invoice_numbering(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<Vec<SerieNumeracion>> {
    Ok(rectificativas_emitidas::numeracion(&*state.vault()?, year)?)
}

#[tauri::command]
pub fn incobrables_list(state: tauri::State<'_, AppState>, today: Date) -> CmdResult<Vec<Credito>> {
    Ok(incobrables::list(&*state.vault()?, today)?)
}

#[tauri::command]
pub fn incobrable_open(
    state: tauri::State<'_, AppState>,
    invoice_id: i64,
    plazo_meses: Option<u32>,
    exclusion: Exclusion,
    confirmed: bool,
) -> CmdResult<i64> {
    Ok(incobrables::abrir(
        &*state.vault()?,
        invoice_id,
        plazo_meses,
        exclusion,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn incobrable_adjust(
    state: tauri::State<'_, AppState>,
    id: i64,
    plazo_meses: u32,
    exclusion: Exclusion,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(incobrables::ajustar(
        &*state.vault()?,
        id,
        plazo_meses,
        exclusion,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn incobrable_claim(
    state: tauri::State<'_, AppState>,
    id: i64,
    fecha: Date,
    medio: Medio,
    referencia: String,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(incobrables::reclamar(
        &*state.vault()?,
        id,
        fecha,
        medio,
        &referencia,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn incobrable_preview(
    state: tauri::State<'_, AppState>,
    id: i64,
    fecha: Date,
) -> CmdResult<PropuestaR3> {
    Ok(incobrables::propuesta(&*state.vault()?, id, fecha)?)
}

/// Emite la R3. Como cualquier factura, pide además Touch ID con a quién y por cuánto.
#[tauri::command(async)]
pub fn incobrable_issue(
    state: tauri::State<'_, AppState>,
    id: i64,
    fecha: Date,
    confirmed: bool,
) -> CmdResult<IssuedInvoice> {
    if !confirmed {
        return Err(CommandError::from(
            taxpain_app::error::AppError::NotConfirmed,
        ));
    }
    let (company, reason) = {
        let (company, v) = state.vault_with_id()?;
        let p = incobrables::propuesta(&v, id, fecha)?;
        let who = p
            .draft
            .recipient
            .as_ref()
            .map(|r| r.name.clone())
            .unwrap_or_default();
        (
            company,
            format!(
                "Emitir la rectificativa por crédito incobrable a {who} por {} de IVA",
                p.iva
            ),
        )
    };
    state.confirm_presence(&reason)?;
    let v = state.vault_of(&company)?;
    Ok(incobrables::emitir_r3(
        &v,
        id,
        fecha,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn incobrable_communication(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> CmdResult<Comunicacion> {
    Ok(incobrables::comunicacion(&*state.vault()?, id)?)
}

#[tauri::command]
pub fn incobrable_communicated(
    state: tauri::State<'_, AppState>,
    id: i64,
    fecha: Date,
    referencia: String,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(incobrables::comunicada(
        &*state.vault()?,
        id,
        fecha,
        &referencia,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn incobrable_close(
    state: tauri::State<'_, AppState>,
    id: i64,
    estado: String,
    fecha: Date,
    notas: String,
    confirmed: bool,
) -> CmdResult<String> {
    Ok(incobrables::cerrar(
        &*state.vault()?,
        id,
        &estado,
        fecha,
        &notas,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}
