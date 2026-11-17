//! Copias de seguridad cifradas (ver `taxpain_app::backup`): estado, contraseña, copia a mano,
//! restauración como empresa nueva y copias automáticas en segundo plano.

use std::path::Path;
use std::time::Duration;

use tauri::{AppHandle, Manager};
use taxpain_app::backup::{self, BackupSettings, BackupStatus, BackupSummary, RestoreCheck};
use taxpain_app::companies::{self, Company};
use taxpain_vault::backup::BackupReport;
use time::OffsetDateTime;

use crate::CmdResult;
use crate::state::AppState;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tauri::command]
pub fn backup_status(app: AppHandle, state: tauri::State<'_, AppState>) -> CmdResult<BackupStatus> {
    let home = app.path().home_dir().ok();
    let v = state.vault()?;
    let company = companies::info(&v)?.name;
    Ok(backup::status(
        &v,
        &company,
        home.as_deref(),
        OffsetDateTime::now_utc(),
    )?)
}

/// Define o cambia la contraseña de las copias (scrypt: tarda un momento). Si ya hay una,
/// cambiarla pide antes Touch ID (lo decide Rust, no la interfaz: ver
/// [`AppState::set_backup_passphrase`]).
#[tauri::command(async)]
pub fn backup_set_passphrase(
    state: tauri::State<'_, AppState>,
    passphrase: String,
) -> CmdResult<()> {
    let passphrase = zeroize::Zeroizing::new(passphrase);
    state.set_backup_passphrase(&passphrase)
}

/// Comprueba que el usuario recuerda la contraseña.
#[tauri::command(async)]
pub fn backup_verify_passphrase(
    state: tauri::State<'_, AppState>,
    passphrase: String,
) -> CmdResult<bool> {
    let passphrase = zeroize::Zeroizing::new(passphrase);
    Ok(backup::verify_passphrase(&*state.vault()?, &passphrase)?)
}

#[tauri::command]
pub fn backup_save_settings(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    mut settings: BackupSettings,
) -> CmdResult<BackupSettings> {
    // Las copias automáticas escriben (y borran las antiguas) en esa carpeta sin preguntar.
    if let Some(f) = &settings.folder {
        settings.folder = Some(crate::salida::carpeta_permitida(
            &app,
            &f.to_string_lossy(),
        )?);
    }
    Ok(backup::save_settings(&*state.vault()?, &settings)?)
}

/// Hace una copia en `path` (elegido con el diálogo de guardar).
#[tauri::command(async)]
pub fn backup_create(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    path: String,
) -> CmdResult<BackupReport> {
    let path = crate::salida::salida_permitida(&app, &path, &["taxpain"])?;
    Ok(backup::create(
        &*state.vault()?,
        &path,
        false,
        VERSION,
        OffsetDateTime::now_utc(),
    )?)
}

/// Qué hay en una copia (no necesita la empresa abierta) y si ya hay en este Mac una empresa
/// con el mismo NIF.
#[tauri::command(async)]
pub fn backup_inspect(
    state: tauri::State<'_, AppState>,
    path: String,
    passphrase: String,
) -> CmdResult<BackupSummary> {
    let passphrase = zeroize::Zeroizing::new(passphrase);
    let mut summary = backup::inspect(Path::new(&path), &passphrase)?;
    if let Some(nif) = summary.nif.clone() {
        summary.same_nif_company = state.companies().ok().and_then(|list| {
            list.into_iter()
                .find(|c| c.nif.as_deref().is_some_and(|n| backup::same_nif(n, &nif)))
                .map(|c| c.name)
        });
    }
    Ok(summary)
}

/// Restaura la copia como empresa nueva (no cambia a ella).
#[tauri::command(async)]
pub fn backup_restore(
    state: tauri::State<'_, AppState>,
    path: String,
    passphrase: String,
) -> CmdResult<Company> {
    let passphrase = zeroize::Zeroizing::new(passphrase);
    state.restore_company(Path::new(&path), &passphrase)
}

#[tauri::command]
pub fn restore_check(state: tauri::State<'_, AppState>) -> CmdResult<Option<RestoreCheck>> {
    Ok(backup::restore_check(&*state.vault()?)?)
}

#[tauri::command]
pub fn restore_acknowledge(state: tauri::State<'_, AppState>) -> CmdResult<()> {
    Ok(backup::acknowledge_restore(
        &*state.vault()?,
        OffsetDateTime::now_utc(),
    )?)
}

/// Copias automáticas de todas las empresas que las tengan activadas: se mira al minuto de
/// abrir la app y luego cada 30 minutos (sólo con la app abierta).
pub fn spawn_auto_loop(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(60));
        loop {
            let state = app.state::<AppState>();
            let r = state.for_each_company(|id, _, v| {
                // Una carpeta guardada antes de comprobarlas al guardar los ajustes.
                if let Some(f) = backup::settings(v)?.folder
                    && crate::salida::carpeta_permitida(&app, &f.to_string_lossy()).is_err()
                {
                    tracing::warn!(empresa = %id, "carpeta de las copias automáticas no válida");
                    return Ok(());
                }
                let name = companies::info(v)?.name;
                if let Some(r) = backup::run_auto(v, &name, VERSION, OffsetDateTime::now_utc())? {
                    tracing::info!(empresa = %id, bytes = r.bytes, "copia automática hecha");
                }
                Ok(())
            });
            if let Err(e) = r {
                tracing::warn!(error = %e.message, "fallo en las copias automáticas");
            }
            std::thread::sleep(Duration::from_secs(30 * 60));
        }
    });
}
