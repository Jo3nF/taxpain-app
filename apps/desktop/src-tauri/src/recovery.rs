//! Apertura con el código de recuperación (DATOS-SEGURIDAD-01): si al arrancar el Llavero no
//! tiene la clave de los datos (Mac nuevo, cuenta de usuario nueva, elemento borrado) pero junto
//! a ellos está el código de recuperación, la app arranca «bloqueada» y la interfaz pide la
//! contraseña de las copias para recuperarlos.

use std::path::PathBuf;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::state::AppState;
use crate::{CmdResult, CommandError};
use taxpain_app::recuperacion::Diagnostico;

/// La app ha arrancado sin poder abrir los datos y espera la contraseña de las copias.
pub struct Locked {
    pub root: PathBuf,
    /// Empresas con código de recuperación: la contraseña de las copias de cualquiera de ellas
    /// recupera la clave (es la misma para todas).
    pub companies: Vec<String>,
}

/// La app no ha podido abrir ninguna empresa y no hay código de recuperación que pedir: la
/// ventana enseña la pantalla de recuperación (qué ha pasado, abrir una copia, exportar lo que se
/// pueda) en vez de llamar a comandos sin datos.
pub struct StartupFailed {
    pub text: String,
    pub root: PathBuf,
}

#[derive(Serialize)]
pub struct LockedInfo {
    /// Hay más de una empresa en este Mac.
    several: bool,
    /// No se pueden abrir los datos: la explicación (no hay nada que pedir).
    #[serde(skip_serializing_if = "Option::is_none")]
    failed: Option<String>,
    /// Con `failed`: el diagnóstico de cada empresa que no abre.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    broken: Vec<Diagnostico>,
}

/// `Some` si los datos están bloqueados a la espera de la contraseña de las copias (o no se han
/// podido abrir).
#[tauri::command]
pub fn vault_locked(app: AppHandle) -> Option<LockedInfo> {
    if app.try_state::<AppState>().is_some() {
        return None;
    }
    if let Some(failed) = app.try_state::<StartupFailed>() {
        return Some(LockedInfo {
            several: taxpain_app::companies::ids(&failed.root).is_ok_and(|ids| ids.len() > 1),
            failed: Some(failed.text.clone()),
            broken: crate::state::diagnose_all(&failed.root),
        });
    }
    let locked = app.try_state::<Locked>()?;
    Some(LockedInfo {
        several: taxpain_app::companies::ids(&locked.root).is_ok_and(|ids| ids.len() > 1),
        failed: None,
        broken: Vec::new(),
    })
}

/// Recupera la clave de los datos con la contraseña de las copias, la vuelve a guardar en el
/// Llavero y arranca la app. La interfaz se recarga después.
#[tauri::command(async)]
pub fn vault_recover(app: AppHandle, passphrase: String) -> CmdResult<()> {
    let passphrase = zeroize::Zeroizing::new(passphrase);
    if app.try_state::<AppState>().is_some() {
        return Ok(());
    }
    let locked = app
        .try_state::<Locked>()
        .ok_or_else(|| CommandError::from("los datos no están bloqueados"))?;
    let state =
        AppState::recover(&locked.root, &locked.companies, &passphrase).map_err(|e| match e {
            taxpain_vault::VaultError::BackupPassphrase => {
                CommandError::from(wrong_passphrase_message(locked.companies.len()))
            }
            other => CommandError::from(other),
        })?;
    app.manage(state);
    crate::start(&app);
    tracing::info!("datos recuperados con el código de recuperación");
    Ok(())
}

/// Empresas que no abren (con la app abierta en otra): su diagnóstico para la pantalla de
/// recuperación. Con la app sin arrancar, lo da [`vault_locked`].
#[tauri::command(async)]
pub fn recuperacion_empresas(app: AppHandle) -> CmdResult<Vec<Diagnostico>> {
    if let Some(state) = app.try_state::<AppState>() {
        return state.broken_companies();
    }
    if let Some(failed) = app.try_state::<StartupFailed>() {
        return Ok(crate::state::diagnose_all(&failed.root));
    }
    Ok(Vec::new())
}

/// Exporta lo que se pueda de la empresa `id`, que no abre, a un ZIP en `path` (elegido con el
/// diálogo de guardar): sus ficheros tal cual y, si el Llavero tiene la clave, sus tablas en
/// CSV y JSON. No toca la carpeta de la empresa. El ZIP lleva los datos sin cifrar: la interfaz
/// lo avisa antes.
#[tauri::command(async)]
pub fn recuperacion_rescatar(
    app: AppHandle,
    id: String,
    path: String,
) -> CmdResult<crate::state::RescueReport> {
    let dest = crate::salida::salida_permitida(&app, &path, &["zip"])?;
    if let Some(state) = app.try_state::<AppState>() {
        return state.rescue_company(&id, &dest);
    }
    let root = startup_root(&app)?;
    if !taxpain_app::companies::ids(&root)?.contains(&id) {
        return Err(CommandError::from("esa empresa no existe"));
    }
    let (keys, _) = crate::state::providers(&root)?;
    crate::state::rescue(&root, &id, keys.as_ref(), &dest)
}

/// Abre una copia de seguridad: la restaura como empresa nueva (la que no abre se queda como
/// está) y la deja abierta. Con la app sin arrancar, la arranca con ella; si no, cambia a ella.
/// La interfaz se recarga después.
#[tauri::command(async)]
pub fn recuperacion_restaurar(app: AppHandle, path: String, passphrase: String) -> CmdResult<()> {
    let passphrase = zeroize::Zeroizing::new(passphrase);
    if let Some(state) = app.try_state::<AppState>() {
        let c = state.restore_company(std::path::Path::new(&path), &passphrase)?;
        return state.switch_company(&c.id);
    }
    let root = startup_root(&app)?;
    let (keys, presence) = crate::state::providers(&root)?;
    let c = crate::state::restore_into(
        &root,
        keys,
        presence,
        std::path::Path::new(&path),
        &passphrase,
    )?;
    taxpain_app::companies::set_active(&root, &c.id)?;
    let state = AppState::open(&root)?;
    app.manage(state);
    crate::start(&app);
    tracing::info!("arrancada con una copia restaurada desde la pantalla de recuperación");
    Ok(())
}

fn startup_root(app: &AppHandle) -> CmdResult<PathBuf> {
    if let Some(f) = app.try_state::<StartupFailed>() {
        return Ok(f.root.clone());
    }
    if let Some(l) = app.try_state::<Locked>() {
        return Ok(l.root.clone());
    }
    Err(CommandError::from(
        "la app no está en la pantalla de recuperación",
    ))
}

/// Mensaje cuando la contraseña no abre ningún código de recuperación.
fn wrong_passphrase_message(recoverable: usize) -> String {
    if recoverable > 1 {
        format!(
            "No es la contraseña de las copias de ninguna de tus empresas (se ha probado con las \
             {recoverable} que la tienen). Prueba otra vez: es la que elegiste al dar de alta una \
             empresa o en Ajustes → Copias de seguridad."
        )
    } else {
        "No es la contraseña de las copias de esta empresa. Prueba otra vez: es la que elegiste \
         al darla de alta o en Ajustes → Copias de seguridad."
            .to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_mensaje_dice_cuantas_empresas_se_han_probado() {
        assert!(wrong_passphrase_message(1).contains("de esta empresa"));
        assert!(wrong_passphrase_message(3).contains("las 3 que la tienen"));
    }
}
