//! Clientes y proveedores (ver `taxpain_app::contacts`).

use taxpain_app::contacts::{self, ContactDetail, ContactInput, ContactRow, Suggestion};
use time::OffsetDateTime;

use crate::CmdResult;
use crate::state::AppState;

fn today() -> time::Date {
    taxpain_core::spain_time::peninsular_date(OffsetDateTime::now_utc())
}

#[tauri::command]
pub fn contacts_list(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<Vec<ContactRow>> {
    Ok(contacts::list(&*state.vault()?, year, today())?)
}

#[tauri::command]
pub fn contact_detail(
    state: tauri::State<'_, AppState>,
    key: String,
    year: i32,
) -> CmdResult<ContactDetail> {
    Ok(contacts::detail(&*state.vault()?, &key, year, today())?)
}

#[tauri::command]
pub fn contact_save(state: tauri::State<'_, AppState>, input: ContactInput) -> CmdResult<i64> {
    Ok(contacts::save(
        &*state.vault()?,
        &input,
        OffsetDateTime::now_utc(),
    )?)
}

/// Borra un contacto: Touch ID con su nombre y NIF.
#[tauri::command(async)]
pub fn contact_delete(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    crate::confirmar::borrar_contacto(&state, id)
}

#[tauri::command]
pub fn contacts_search(
    state: tauri::State<'_, AppState>,
    query: String,
) -> CmdResult<Vec<Suggestion>> {
    Ok(contacts::search(&*state.vault()?, &query, 8)?)
}

/// Búsqueda global (⌘K).
#[tauri::command]
pub fn global_search(
    state: tauri::State<'_, AppState>,
    query: String,
) -> CmdResult<Vec<taxpain_app::search::Hit>> {
    Ok(taxpain_app::search::search(&*state.vault()?, &query, 6)?)
}

/// Comprueba un NIF-IVA en VIES (sólo cuando el usuario lo pide) y guarda la prueba en la ficha
/// del tercero (si no la tiene, la crea como `role`: cliente o proveedor). Se envían el NIF-IVA
/// del tercero y el de la empresa; nada más. Si VIES no responde, no se guarda nada.
#[tauri::command]
pub async fn vies_check(
    state: tauri::State<'_, AppState>,
    country: String,
    vat: String,
    name: Option<String>,
    role: Option<String>,
) -> CmdResult<taxpain_aeat::vies::ViesResult> {
    // El NIF propio, sin tener el almacén bloqueado mientras se espera a la red.
    let (company, requester) = {
        let (company, v) = state.vault_with_id()?;
        let requester = taxpain_app::invoicing::settings(&v)?.and_then(|s| match s.issuer.id {
            taxpain_core::invoice::PartyId::Nif(n) => Some(n),
            taxpain_core::invoice::PartyId::Foreign { .. } => None,
        });
        (company, requester)
    };
    let r = taxpain_aeat::vies::check(&country, &vat, requester.as_deref()).await?;
    let now = OffsetDateTime::now_utc();
    // En la ficha de la empresa que lo pidió (si se ha cambiado de empresa, se avisa).
    contacts::record_vies(
        &*state.vault_of(&company)?,
        &country,
        &vat,
        name.as_deref(),
        role.as_deref(),
        contacts::ViesProof {
            valid: r.valid,
            checked_at: taxpain_app::invoicing::now_str(now),
            request_id: r.request_id.clone(),
            name: r.name.clone(),
        },
        now,
    )?;
    Ok(r)
}
