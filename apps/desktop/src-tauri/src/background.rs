//! TaxPain en segundo plano, para que los avisos de plazos lleguen aunque no se tenga abierta:
//! en el Mac, cerrar la ventana la esconde (la app sigue en el Dock y ⌘Q la cierra) y, si el
//! usuario lo activa, se abre sola al iniciar sesión, sin ventana (`--hidden`).

use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

use crate::CmdResult;

/// Argumento con el que la abre el inicio de sesión: arranca sin ventana.
pub const HIDDEN_ARG: &str = "--hidden";

/// Muestra la ventana principal (al volver desde el Dock).
#[cfg(target_os = "macos")]
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// ¿La ha abierto el inicio de sesión (sin ventana)?
fn started_hidden(mut args: impl Iterator<Item = String>) -> bool {
    args.any(|a| a == HIDDEN_ARG)
}

/// La ventana principal arranca escondida (`"visible": false` en `tauri.conf.json`): se enseña
/// al abrir la app, y no si la ha abierto el inicio de sesión (antes se enseñaba un momento y se
/// escondía).
pub fn show_unless_started_hidden(app: &AppHandle) {
    if started_hidden(std::env::args()) {
        return;
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[tauri::command]
pub fn autostart_get(app: AppHandle) -> CmdResult<bool> {
    Ok(app.autolaunch().is_enabled()?)
}

#[tauri::command]
pub fn autostart_set(app: AppHandle, enabled: bool) -> CmdResult<bool> {
    let a = app.autolaunch();
    if enabled {
        a.enable()?;
    } else {
        a.disable()?;
    }
    Ok(a.is_enabled()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sin_ventana_solo_con_el_argumento() {
        let args = |a: &[&str]| {
            a.iter()
                .map(|s| (*s).to_owned())
                .collect::<Vec<_>>()
                .into_iter()
        };
        assert!(!started_hidden(args(&["TaxPain"])));
        assert!(started_hidden(args(&["TaxPain", "--hidden"])));
        assert!(!started_hidden(args(&["TaxPain", "--hidden=no"])));
    }
}
