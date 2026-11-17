//! Empresas: lista, alta, cambio de empresa, datos de cada una y borrado.

use taxpain_app::companies::{self, Company, CompanyInfo};
use time::OffsetDateTime;

use crate::state::AppState;
use crate::{CmdResult, CommandError};

#[tauri::command]
pub fn companies_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Company>> {
    state.companies()
}

/// Da de alta otra empresa (vacía) sin cambiar a ella, con la contraseña de sus copias (scrypt:
/// tarda un momento).
#[tauri::command(async)]
pub fn company_create(
    state: tauri::State<'_, AppState>,
    info: CompanyInfo,
    passphrase: Option<String>,
) -> CmdResult<Company> {
    let passphrase = passphrase.map(zeroize::Zeroizing::new);
    state.create_company(&info, passphrase.as_deref().map(String::as_str))
}

/// Cambia de empresa. La interfaz se recarga después.
#[tauri::command]
pub fn company_switch(state: tauri::State<'_, AppState>, id: String) -> CmdResult<()> {
    state.switch_company(&id)
}

/// Nombre y NIF de la empresa activa.
#[tauri::command]
pub fn company_update(
    state: tauri::State<'_, AppState>,
    info: CompanyInfo,
) -> CmdResult<CompanyInfo> {
    Ok(companies::set_info(
        &*state.vault()?,
        &info,
        OffsetDateTime::now_utc(),
    )?)
}

/// Nombre y NIF de cualquier empresa (la abierta o no).
#[tauri::command]
pub fn company_update_id(
    state: tauri::State<'_, AppState>,
    id: String,
    info: CompanyInfo,
) -> CmdResult<CompanyInfo> {
    state.update_company(&id, &info)
}

/// Borra una empresa (su carpeta va a la Papelera del Mac). `confirm`: su nombre o su NIF,
/// escrito por el usuario; como eso lo puede leer la propia interfaz, además se pide Touch ID
/// con el nombre de la empresa. Devuelve `true` si era la abierta y se ha abierto otra: la
/// interfaz se recarga después.
#[tauri::command(async)]
pub fn company_delete(
    state: tauri::State<'_, AppState>,
    id: String,
    confirm: String,
) -> CmdResult<bool> {
    let name = {
        let info = companies::info(&*state.company_vault(&id)?)?;
        // Si lo escrito no vale, se dice ya, sin pedir Touch ID para nada.
        if !companies::confirms(&info, &confirm) {
            return Err(CommandError::from(
                "escribe el nombre o el NIF de la empresa para confirmar",
            ));
        }
        info.name
    };
    state.confirm_presence(&format!(
        "Borrar la empresa {} (va a la Papelera)",
        crate::invoicing::reason_text(&name)
    ))?;
    state.delete_company(&id, &confirm)
}

/// Resumen de una empresa en la vista de todas las empresas.
#[derive(serde::Serialize)]
pub struct CompanyOverview {
    pub id: String,
    pub name: String,
    pub nif: Option<String>,
    pub active: bool,
    pub status: taxpain_app::overview::CompanyStatus,
}

/// Plazos próximos y vencidos sin presentar de todas las empresas del Mac.
#[tauri::command]
pub fn companies_overview(state: tauri::State<'_, AppState>) -> CmdResult<Vec<CompanyOverview>> {
    let today = crate::calendar::today();
    let mut out = Vec::new();
    state.for_each_company(|id, active, v| {
        let info = companies::info(v)?;
        out.push(CompanyOverview {
            id: id.to_owned(),
            name: info.name,
            nif: info.nif,
            active,
            status: taxpain_app::overview::company_status(v, today)?,
        });
        Ok(())
    })?;
    Ok(out)
}

/// Cuentas agregadas de varias empresas de este Mac (con eliminaciones dentro del grupo).
#[tauri::command]
pub fn consolidated_accounts(
    state: tauri::State<'_, AppState>,
    ids: Vec<String>,
    year: i32,
    model: Option<taxpain_core::accounting::statements::ModelKind>,
    eliminate: bool,
) -> CmdResult<taxpain_app::consolidation::Consolidated> {
    use taxpain_app::consolidation::{company_data, consolidate};
    if ids.len() < 2 {
        return Err(CommandError::from("elige al menos dos empresas"));
    }
    let mut chosen = Vec::new();
    state.for_each_company(|id, _, v| {
        if ids.iter().any(|i| i == id) {
            let info = companies::info(v)?;
            let nif = info.nif.unwrap_or_default().to_uppercase();
            chosen.push((id.to_owned(), info.name, nif));
        }
        Ok(())
    })?;
    let group: Vec<String> = chosen.iter().map(|c| c.2.clone()).collect();
    let mut data = Vec::new();
    state.for_each_company(|id, _, v| {
        if let Some((_, name, nif)) = chosen.iter().find(|c| c.0 == id) {
            data.push(company_data(v, name, nif, year, &group)?);
        }
        Ok(())
    })?;
    Ok(consolidate(&data, model.unwrap_or_default(), eliminate))
}

/// Copia asientos del ejercicio de la empresa abierta a otra empresa del Mac.
#[tauri::command]
pub fn copy_entries_to(
    state: tauri::State<'_, AppState>,
    ids: Vec<i64>,
    year: i32,
    target: String,
) -> CmdResult<taxpain_app::accounting::CopyOutcome> {
    use taxpain_app::accounting::{CopyOutcome, entries_to_copy, paste_entries};
    if state.active_id()? == target {
        return Err(CommandError::from(
            "elige otra empresa: es la que tienes abierta",
        ));
    }
    let (entries, third_party_accounts, source) = {
        let v = state.vault()?;
        let (e, t) = entries_to_copy(&v, year, &ids)?;
        (e, t, companies::info(&v)?.name)
    };
    let to = state.open_company(&target)?;
    let copied = paste_entries(&to, &entries, &source, OffsetDateTime::now_utc())?;
    Ok(CopyOutcome {
        copied,
        third_party_accounts,
    })
}
