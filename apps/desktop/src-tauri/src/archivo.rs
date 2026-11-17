//! Archivo y auditoría (G38 y G46): registro de auditoría visible, comprobación de integridad,
//! bloqueo de periodos, «Archivo del ejercicio», plazos de conservación y exportación completa.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use taxpain_app::archivo::conservacion::{self, AvisoConservacion, Conservacion};
use taxpain_app::archivo::ejercicio::{self, Hecho, Parte};
use taxpain_app::archivo::exportacion::{self, Exportado};
use taxpain_app::periodos::{self, Periodo, Sugerido, Tipo};
use taxpain_vault::VaultError;
use taxpain_vault::audit::{Filtro, Pagina};
use time::OffsetDateTime;

use crate::CmdResult;
use crate::state::AppState;

// ---------- Auditoría ----------

#[tauri::command]
pub fn auditoria_consultar(state: tauri::State<'_, AppState>, filtro: Filtro) -> CmdResult<Pagina> {
    Ok(state.vault()?.audit_query(&filtro)?)
}

#[tauri::command]
pub fn auditoria_acciones(state: tauri::State<'_, AppState>) -> CmdResult<Vec<(String, i64)>> {
    Ok(state.vault()?.audit_actions()?)
}

#[derive(Serialize)]
pub struct Verificacion {
    pub ok: bool,
    pub entradas: usize,
    /// Primera entrada que no cuadra.
    pub rota_en: Option<i64>,
    pub mensaje: String,
}

/// Comprueba la cadena de huellas entera (sólo lee).
#[tauri::command(async)]
pub fn auditoria_verificar(state: tauri::State<'_, AppState>) -> CmdResult<Verificacion> {
    Ok(match state.vault()?.verify_audit() {
        Ok(n) => Verificacion {
            ok: true,
            entradas: n,
            rota_en: None,
            mensaje: format!(
                "La cadena de huellas está completa: las {n} entradas encadenan bien, ninguna se \
                 ha cambiado ni borrado."
            ),
        },
        Err(VaultError::AuditTampered(seq)) => Verificacion {
            ok: false,
            entradas: 0,
            rota_en: Some(seq),
            mensaje: format!(
                "La entrada {seq} no cuadra con su huella: el registro se ha cambiado por fuera \
                 de TaxPain o está dañado. Restaura una copia de seguridad y pide ayuda."
            ),
        },
        Err(e) => return Err(e.into()),
    })
}

/// Resultado de la comprobación de integridad de cada empresa en esta sesión (es lenta en una BD
/// grande: una vez por empresa y arranque).
static INTEGRIDAD: Mutex<Option<HashMap<String, Vec<String>>>> = Mutex::new(None);

/// Comprueba la integridad de la BD de la empresa abierta (`PRAGMA cipher_integrity_check` e
/// `integrity_check`). Vacío si está sana. Sólo lee.
#[tauri::command(async)]
pub fn integridad_empresa(state: tauri::State<'_, AppState>) -> CmdResult<Vec<String>> {
    let (id, v) = state.vault_with_id()?;
    if let Some(r) = INTEGRIDAD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .and_then(|m| m.get(&id))
    {
        return Ok(r.clone());
    }
    let r = v.integrity_check()?;
    drop(v);
    if !r.is_empty() {
        tracing::error!(empresa = %id, problemas = r.len(), "la BD de la empresa tiene daños");
    }
    INTEGRIDAD
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get_or_insert_with(HashMap::new)
        .insert(id, r.clone());
    Ok(r)
}

// ---------- Periodos cerrados ----------

#[tauri::command]
pub fn periodos_listar(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Periodo>> {
    Ok(periodos::listar(&*state.vault()?)?)
}

#[tauri::command]
pub fn periodos_sugeridos(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Sugerido>> {
    Ok(periodos::sugeridos(&*state.vault()?)?)
}

/// Cierra un periodo (la interfaz pide antes la confirmación).
#[tauri::command]
pub fn periodo_cerrar(
    state: tauri::State<'_, AppState>,
    tipo: Tipo,
    ejercicio: i32,
    trimestre: Option<u8>,
    motivo: String,
    confirmed: bool,
) -> CmdResult<Periodo> {
    Ok(periodos::cerrar(
        &*state.vault()?,
        tipo,
        ejercicio,
        trimestre,
        &motivo,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Reabre un periodo cerrado: pide Touch ID (quita el aviso a todo lo de esas fechas).
#[tauri::command(async)]
pub fn periodo_reabrir(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    let (company, reason) = {
        let (company, v) = state.vault_with_id()?;
        let p = periodos::listar(&v)?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| crate::CommandError::from("ese periodo no está cerrado"))?;
        let name = taxpain_app::companies::info(&v)?.name;
        (
            company,
            format!(
                "Reabrir el {} de {}",
                p.etiqueta,
                crate::invoicing::reason_text(&name)
            ),
        )
    };
    state.confirm_presence(&reason)?;
    Ok(periodos::reabrir(
        &*state.vault_of(&company)?,
        id,
        OffsetDateTime::now_utc(),
        true,
    )?)
}

/// El usuario ha leído el aviso de periodo cerrado y sigue: excepción de unos minutos para ese
/// periodo, anotada en la auditoría.
#[tauri::command]
pub fn periodo_autorizar(state: tauri::State<'_, AppState>, id: i64, que: String) -> CmdResult<()> {
    Ok(periodos::autorizar(
        &*state.vault()?,
        id,
        &que,
        OffsetDateTime::now_utc(),
    )?)
}

// ---------- Archivo del ejercicio y conservación ----------

#[tauri::command]
pub fn archivo_conservacion(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Conservacion>> {
    Ok(conservacion::conservacion(
        &*state.vault()?,
        crate::calendar::today(),
    )?)
}

#[tauri::command]
pub fn archivo_aviso(state: tauri::State<'_, AppState>) -> CmdResult<AvisoConservacion> {
    Ok(conservacion::aviso(
        &*state.vault()?,
        crate::calendar::today(),
    )?)
}

/// Qué hay y qué falta del ejercicio (sin escribir nada).
#[tauri::command(async)]
pub fn archivo_ejercicio_contenido(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<Vec<Parte>> {
    Ok(ejercicio::contenido(
        &*state.vault()?,
        year,
        crate::calendar::today(),
    )?)
}

/// Touch ID antes de sacar datos sin cifrar; devuelve la empresa para seguir en la misma.
fn presencia_para_sacar(state: &AppState, que: &str) -> CmdResult<String> {
    let (company, name) = {
        let (company, v) = state.vault_with_id()?;
        (company, taxpain_app::companies::info(&v)?.name)
    };
    state.confirm_presence(&format!(
        "{que} de {} (sin cifrar)",
        crate::invoicing::reason_text(&name)
    ))?;
    Ok(company)
}

/// Escribe el «Archivo del ejercicio» en `path` (elegido con el diálogo de guardar). Pide Touch
/// ID: el ZIP va sin cifrar.
#[tauri::command(async)]
pub fn archivo_ejercicio_exportar(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    year: i32,
    path: String,
) -> CmdResult<Hecho> {
    let dest = crate::salida::salida_permitida(&app, &path, &["zip"])?;
    let company =
        presencia_para_sacar(&state, &format!("Exportar el archivo del ejercicio {year}"))?;
    Ok(ejercicio::exportar(
        &*state.vault_of(&company)?,
        year,
        &dest,
        crate::calendar::today(),
        OffsetDateTime::now_utc(),
        true,
    )?)
}

/// Exportación completa de la empresa a formatos abiertos en `path`. Pide Touch ID.
#[tauri::command(async)]
pub fn exportacion_completa(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    path: String,
) -> CmdResult<Exportado> {
    let dest = crate::salida::salida_permitida(&app, &path, &["zip"])?;
    let company = presencia_para_sacar(&state, "Exportar todos los datos")?;
    Ok(exportacion::completa(
        &*state.vault_of(&company)?,
        &dest,
        OffsetDateTime::now_utc(),
        true,
    )?)
}
