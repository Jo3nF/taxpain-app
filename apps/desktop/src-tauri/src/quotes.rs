//! Presupuestos y proformas (ver `taxpain_app::quotes`).

use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use taxpain_app::correo::{self, EmailDraft};
use taxpain_app::invoicing::{InvoiceDraft, IssuedInvoice};
use taxpain_app::quotes::{self, Quote, QuoteInput};
use taxpain_app::settings::Language;
use time::OffsetDateTime;

use crate::state::AppState;
use crate::{CmdResult, CommandError};

fn today() -> time::Date {
    taxpain_core::spain_time::peninsular_date(OffsetDateTime::now_utc())
}

#[tauri::command]
pub fn quotes_list(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<Vec<Quote>> {
    Ok(quotes::list(&*state.vault()?, year, today())?)
}

#[tauri::command]
pub fn quote_create(state: tauri::State<'_, AppState>, input: QuoteInput) -> CmdResult<Quote> {
    Ok(quotes::create(
        &*state.vault()?,
        &input,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn quote_update(
    state: tauri::State<'_, AppState>,
    id: i64,
    input: QuoteInput,
) -> CmdResult<Quote> {
    Ok(quotes::update(
        &*state.vault()?,
        id,
        &input,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn quote_status(state: tauri::State<'_, AppState>, id: i64, status: String) -> CmdResult<()> {
    Ok(quotes::set_status(
        &*state.vault()?,
        id,
        &status,
        OffsetDateTime::now_utc(),
    )?)
}

/// Borra un presupuesto en borrador: Touch ID con su número.
#[tauri::command(async)]
pub fn quote_delete(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    crate::confirmar::borrar_presupuesto(&state, id)
}

/// Abre el PDF del presupuesto (copia en la carpeta temporal privada).
#[tauri::command]
pub fn quote_open(app: AppHandle, state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    let (name, bytes) = quotes::document(&*state.vault()?, id, today())?;
    let path = crate::invoicing::private_file("taxpain-presupuestos", &name)?;
    std::fs::write(&path, bytes)?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)?;
    Ok(())
}

/// Guarda el PDF donde diga el usuario.
#[tauri::command]
pub fn quote_save(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    id: i64,
    path: String,
) -> CmdResult<()> {
    let path = crate::salida::salida_permitida(&app, &path, &["pdf"])?;
    let (_, bytes) = quotes::document(&*state.vault()?, id, today())?;
    crate::salida::escribir(&path, bytes)?;
    Ok(())
}

#[derive(serde::Serialize)]
pub struct QuoteInvoice {
    draft: InvoiceDraft,
    language: Option<Language>,
}

/// Datos para la factura del presupuesto (se abre el formulario; el usuario la emite).
#[tauri::command]
pub fn quote_invoice_draft(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<QuoteInvoice> {
    let (draft, language) = quotes::invoice_draft(&*state.vault()?, id, today())?;
    Ok(QuoteInvoice { draft, language })
}

/// Emite la factura del presupuesto y lo deja facturado (en una sola transacción). `confirmed`
/// sólo lo envía el diálogo de confirmación y se pide Touch ID, como en `invoice_issue`.
#[tauri::command(async)]
pub fn quote_issue_invoice(
    state: tauri::State<'_, AppState>,
    id: i64,
    draft: InvoiceDraft,
    confirmed: bool,
    language: Option<Language>,
) -> CmdResult<IssuedInvoice> {
    let company = if confirmed {
        let (company, reason) = {
            let (company, v) = state.vault_with_id()?;
            (
                company,
                crate::invoicing::issue_reason(&v, &draft, language)?,
            )
        };
        state.confirm_presence(&reason)?;
        Some(company)
    } else {
        None
    };
    let v = match &company {
        Some(c) => state.vault_of(c)?,
        None => state.vault()?,
    };
    Ok(quotes::issue_invoice(
        &v,
        id,
        &draft,
        language,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn quote_email_compose(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<EmailDraft> {
    Ok(correo::compose_quote(&*state.vault()?, id, today())?)
}

#[tauri::command]
pub fn quote_email_open(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    id: i64,
    to: Option<String>,
    subject: String,
    body: String,
) -> CmdResult<()> {
    let (name, bytes) = correo::build_quote(
        &*state.vault()?,
        id,
        to.as_deref(),
        &subject,
        &body,
        today(),
        OffsetDateTime::now_utc(),
    )?;
    let path = crate::invoicing::private_file("taxpain-correo", &name)?;
    std::fs::write(&path, bytes)?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)
        .map_err(|e| CommandError::from(e.to_string()))?;
    Ok(())
}
