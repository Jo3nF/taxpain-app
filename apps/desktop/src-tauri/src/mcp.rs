//! Comandos del ajuste «Claude (MCP)»: si la empresa abierta deja que Claude la consulte por el
//! servidor MCP (`taxpain-mcp`) y hasta dónde. El ajuste vive en la BD cifrada de la empresa
//! (`taxpain_app::mcp_ajustes`); el servidor es un programa aparte y sólo ve las empresas que lo
//! tienen activado.

use serde::Serialize;
use taxpain_app::mcp_ajustes::{self, Ajustes};
use time::OffsetDateTime;

use crate::CmdResult;
use crate::state::AppState;

/// Lo que pinta la sección de Ajustes.
#[derive(Debug, Serialize)]
pub struct McpEstado {
    pub ajustes: Ajustes,
    /// Ruta del programa `taxpain-mcp` si está junto a la app (para la configuración de Claude);
    /// `None` si hay que compilarlo aparte (`docs/CLAUDE_CODE.md`).
    pub binario: Option<String>,
}

/// `taxpain-mcp` junto al ejecutable de la app, si está.
fn binario() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let mcp = exe.parent()?.join("taxpain-mcp");
    mcp.is_file().then(|| mcp.to_string_lossy().into_owned())
}

fn estado(ajustes: Ajustes) -> McpEstado {
    McpEstado {
        ajustes,
        binario: binario(),
    }
}

#[tauri::command]
pub fn mcp_estado(state: tauri::State<'_, AppState>) -> CmdResult<McpEstado> {
    Ok(estado(mcp_ajustes::ajustes(&*state.vault()?)?))
}

/// Activa, apaga o cambia de nivel el acceso de Claude a la empresa abierta. Queda en la
/// auditoría (`mcp.ajustes`).
#[tauri::command]
pub fn mcp_guardar(state: tauri::State<'_, AppState>, ajustes: Ajustes) -> CmdResult<McpEstado> {
    let v = state.vault()?;
    mcp_ajustes::guardar(&v, &ajustes, OffsetDateTime::now_utc())?;
    Ok(estado(mcp_ajustes::ajustes(&v)?))
}
