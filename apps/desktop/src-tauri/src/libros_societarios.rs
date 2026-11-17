//! Comandos de los libros societarios: libro registro de socios, contratos con el socio único y
//! libro de actas (la lógica está en `taxpain_app::libros_societarios`). Lo que borra, aprueba,
//! reabre o cambia la ficha de socios llega con `confirmed` desde la confirmación de la
//! interfaz y queda en la auditoría.

use taxpain_app::libros_societarios::actas::{
    self, CertificateInput, Minute, Template, TemplateDraft,
};
use taxpain_app::libros_societarios::contratos::{self, Proposals, SoleContract};
use taxpain_app::libros_societarios::registro::{self, Register, ShareMovement};
use taxpain_app::libros_societarios::{self as ls, BookKind};
use time::{Date, OffsetDateTime};

use crate::CmdResult;
use crate::salida;
use crate::state::AppState;

fn ext(pdf: bool) -> &'static str {
    if pdf { "pdf" } else { "rtf" }
}

// ---------- Libro registro de socios ----------

#[tauri::command]
pub fn partners_register(state: tauri::State<'_, AppState>) -> CmdResult<Register> {
    Ok(registro::register(
        &*state.vault()?,
        OffsetDateTime::now_utc().date(),
    )?)
}

#[tauri::command]
pub fn share_movement_save(
    state: tauri::State<'_, AppState>,
    movement: ShareMovement,
) -> CmdResult<i64> {
    Ok(registro::save_movement(
        &*state.vault()?,
        &movement,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn share_movement_delete(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(registro::delete_movement(
        &*state.vault()?,
        id,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Pasa a la ficha de socios los porcentajes del libro. Devuelve cuántos socios cambió.
#[tauri::command]
pub fn partners_register_apply(
    state: tauri::State<'_, AppState>,
    confirmed: bool,
) -> CmdResult<usize> {
    Ok(registro::apply_to_shareholders(
        &*state.vault()?,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

// ---------- Contratos con el socio único ----------

#[tauri::command]
pub fn sole_contracts(state: tauri::State<'_, AppState>) -> CmdResult<Vec<SoleContract>> {
    Ok(contratos::contracts(&*state.vault()?)?)
}

#[tauri::command]
pub fn sole_contract_proposals(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<Proposals> {
    Ok(contratos::proposals(&*state.vault()?, year)?)
}

#[tauri::command]
pub fn sole_contract_save(
    state: tauri::State<'_, AppState>,
    contract: SoleContract,
) -> CmdResult<i64> {
    Ok(contratos::save_contract(
        &*state.vault()?,
        &contract,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn sole_contract_delete(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(contratos::delete_contract(
        &*state.vault()?,
        id,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

// ---------- Libro de actas ----------

#[tauri::command]
pub fn minutes_list(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<Vec<Minute>> {
    Ok(actas::minutes(&*state.vault()?, Some(year))?)
}

/// Borrador desde una plantilla (no guarda nada).
#[tauri::command]
pub fn minute_template(
    state: tauri::State<'_, AppState>,
    template: Template,
    date: Date,
) -> CmdResult<TemplateDraft> {
    Ok(actas::template(&*state.vault()?, &template, date)?)
}

#[tauri::command]
pub fn minute_save(state: tauri::State<'_, AppState>, minute: Minute) -> CmdResult<i64> {
    Ok(actas::save_minute(
        &*state.vault()?,
        &minute,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn minute_delete(state: tauri::State<'_, AppState>, id: i64, confirmed: bool) -> CmdResult<()> {
    Ok(actas::delete_minute(
        &*state.vault()?,
        id,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Aprueba el acta: entra en el libro con el siguiente número. Devuelve el número.
#[tauri::command]
pub fn minute_approve(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<u32> {
    Ok(actas::approve_minute(
        &*state.vault()?,
        id,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn minute_reopen(state: tauri::State<'_, AppState>, id: i64, confirmed: bool) -> CmdResult<()> {
    Ok(actas::reopen_minute(
        &*state.vault()?,
        id,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Guarda un acta suelta en PDF o RTF.
#[tauri::command]
pub fn minute_document_save(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: i64,
    path: String,
    pdf: bool,
) -> CmdResult<()> {
    let path = salida::salida_permitida(&app, &path, &[ext(pdf)])?;
    salida::escribir(&path, actas::minute_document(&*state.vault()?, id, pdf)?)
}

/// Guarda la certificación de los acuerdos de un acta aprobada en PDF o RTF.
#[tauri::command]
pub fn minute_certificate_save(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: i64,
    certificate: CertificateInput,
    path: String,
    pdf: bool,
) -> CmdResult<()> {
    let path = salida::salida_permitida(&app, &path, &[ext(pdf)])?;
    salida::escribir(
        &path,
        actas::certificate_document(&*state.vault()?, id, &certificate, pdf)?,
    )
}

/// Guarda uno de los tres libros del ejercicio en PDF (con folios) o RTF.
#[tauri::command]
pub fn corporate_book_save(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    book: BookKind,
    year: Option<i32>,
    path: String,
    pdf: bool,
) -> CmdResult<()> {
    let path = salida::salida_permitida(&app, &path, &[ext(pdf)])?;
    salida::escribir(&path, ls::book(&*state.vault()?, book, year, None, pdf)?)
}
