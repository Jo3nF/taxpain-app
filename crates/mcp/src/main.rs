//! `taxpain-mcp`: servidor MCP por stdio. Ver la documentación del crate (`lib.rs`).

use std::io::{BufRead, Write};
use std::sync::Arc;

use taxpain_mcp::{AppData, Server, default_root};

/// Fuera del Mac no hay Llavero: las claves irían en un fichero. Sólo vale para desarrollar.
#[cfg(all(not(target_os = "macos"), not(debug_assertions)))]
compile_error!(
    "taxpain-mcp sólo se distribuye para macOS: fuera de macOS no hay Keychain que guarde la \
     clave de los datos (compila en modo desarrollo para probar)"
);

fn main() {
    let Some(root) = default_root() else {
        eprintln!("taxpain-mcp: no encuentro la carpeta de datos de TaxPain (falta HOME)");
        std::process::exit(1);
    };
    let server = Server::new(AppData { root, keys: keys() });
    let stdin = std::io::stdin();
    let mut out = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if let Some(reply) = server.handle_line(&line)
            && writeln!(out, "{reply}").and_then(|()| out.flush()).is_err()
        {
            break;
        }
    }
}

/// La clave de los datos, del mismo Llavero que la app.
#[cfg(target_os = "macos")]
fn keys() -> Arc<dyn taxpain_vault::KeyProvider> {
    use taxpain_macos::{KeychainKeyProvider, KeychainMode};
    let mode = match option_env!("TAXPAIN_KEYCHAIN") {
        Some("data-protection") => KeychainMode::DataProtection,
        _ => KeychainMode::Legacy,
    };
    Arc::new(KeychainKeyProvider::new(mode))
}

/// Fuera del Mac (sólo desarrollo), las claves de fichero de la app de desarrollo.
#[cfg(not(target_os = "macos"))]
fn keys() -> Arc<dyn taxpain_vault::KeyProvider> {
    let root = default_root().unwrap_or_default();
    match taxpain_vault::FileKeyProvider::new(root.join("dev-keys")) {
        Ok(k) => Arc::new(k),
        Err(e) => {
            eprintln!("taxpain-mcp: {e}");
            std::process::exit(1);
        }
    }
}
