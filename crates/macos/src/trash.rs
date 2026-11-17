//! Papelera del Mac: mover una carpeta (p. ej. la de una empresa que se borra) en vez de borrarla.

use std::path::Path;

use objc2_foundation::{NSFileManager, NSString, NSURL};

/// Mueve `path` a la Papelera del usuario. Se puede recuperar arrastrándolo de vuelta hasta que
/// se vacíe la Papelera.
pub fn move_to_trash(path: &Path) -> Result<(), String> {
    let s = path.to_str().ok_or("la ruta no es válida")?;
    let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(s), path.is_dir());
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, None)
        .map_err(|e| e.localizedDescription().to_string())
}
