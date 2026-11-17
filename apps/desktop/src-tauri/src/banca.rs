//! Comandos de la conexión de solo lectura con los bancos (Enable Banking). La clave de la
//! aplicación no sale nunca hacia la interfaz; las peticiones al banco se hacen sin el almacén
//! cogido.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tauri_plugin_opener::OpenerExt;
use taxpain_app::banca::{self, Actualizacion, Ajustes, Estado};
use taxpain_banca::enable::Aspsp;
use taxpain_banca::vuelta::{self, Vuelta};
use time::OffsetDateTime;

use crate::state::AppState;
use crate::{CmdResult, CommandError, read_limited};

/// Cuánto se espera la vuelta del banco en 127.0.0.1.
const ESPERA: Duration = Duration::from_secs(10 * 60);

/// Cada conexión que se empieza, se cancela o se termina cambia el turno: una espera de la
/// vuelta en 127.0.0.1 de un turno anterior se cierra sola (y suelta el puerto).
static TURNO: AtomicU64 = AtomicU64::new(0);

fn otro_turno() -> u64 {
    TURNO.fetch_add(1, Ordering::SeqCst) + 1
}

fn ahora() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

#[tauri::command]
pub fn banca_estado(state: tauri::State<'_, AppState>) -> CmdResult<Estado> {
    Ok(banca::estado(&*state.vault()?, ahora())?)
}

#[tauri::command]
pub fn banca_ajustes(state: tauri::State<'_, AppState>, ajustes: Ajustes) -> CmdResult<Estado> {
    let v = state.vault()?;
    banca::guardar_ajustes(&v, &ajustes, ahora())?;
    Ok(banca::estado(&v, ahora())?)
}

/// Guarda el id de la aplicación y su clave privada, leída del fichero `.pem` de `path`
/// (elegido con el diálogo). La clave se borra de memoria al terminar.
#[tauri::command]
pub fn banca_credenciales(
    state: tauri::State<'_, AppState>,
    app_id: String,
    path: String,
) -> CmdResult<Estado> {
    let bytes = zeroize::Zeroizing::new(read_limited(&path, 64 * 1024)?);
    let pem = zeroize::Zeroizing::new(
        String::from_utf8(bytes.to_vec())
            .map_err(|_| CommandError::from("ese fichero no es una clave .pem"))?,
    );
    let v = state.vault()?;
    banca::guardar_credenciales(&v, &app_id, &pem, ahora())?;
    Ok(banca::estado(&v, ahora())?)
}

#[tauri::command]
pub fn banca_quitar_credenciales(state: tauri::State<'_, AppState>) -> CmdResult<Estado> {
    let v = state.vault()?;
    banca::quitar_credenciales(&v, ahora())?;
    Ok(banca::estado(&v, ahora())?)
}

/// BBVA y Revolut tal como los ofrece Enable Banking (con sus tipos de cuenta y el permiso más
/// largo que admiten). Revolut se busca también en Lituania (Revolut Bank UAB) si no sale en
/// España.
#[tauri::command]
pub async fn banca_bancos(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Aspsp>> {
    let cliente = banca::cliente(&*state.vault()?)?;
    let mut lista = banca::admitidos(cliente.bancos("ES").await?);
    if !lista
        .iter()
        .any(|a| a.name.to_lowercase().starts_with("revolut"))
        && let Ok(lt) = cliente.bancos("LT").await
    {
        lista.extend(
            banca::admitidos(lt)
                .into_iter()
                .filter(|a| a.name.to_lowercase().starts_with("revolut")),
        );
    }
    if lista.is_empty() {
        return Err(CommandError::from(
            "Enable Banking no ofrece ahora BBVA ni Revolut: importa sus extractos en fichero",
        ));
    }
    Ok(lista)
}

#[derive(serde::Serialize)]
pub struct Inicio {
    /// TaxPain recoge sola la vuelta del banco (dirección de vuelta en 127.0.0.1).
    pub escucha: bool,
}

/// Empieza la autorización: pide a Enable Banking la dirección del banco y la abre en el
/// navegador.
#[tauri::command]
pub async fn banca_conectar(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    banco: String,
    pais: String,
    tipo: String,
    maximo: Option<i64>,
) -> CmdResult<Inicio> {
    otro_turno();
    let (company, cliente, peticion) = {
        let (id, v) = state.vault_with_id()?;
        let cliente = banca::cliente(&v)?;
        let p = banca::preparar(&v, &banco, &pais, &tipo, maximo, ahora())?;
        (id, cliente, p)
    };
    let auth = match cliente.autorizar(&peticion).await {
        Ok(a) => a,
        Err(e) => {
            banca::descartar_pendiente(&*state.vault_of(&company)?)?;
            if let taxpain_banca::BancaError::Api {
                codigo: Some(c), ..
            } = &e
                && c == "REDIRECT_URI_NOT_ALLOWED"
            {
                return Err(CommandError::from(format!(
                    "Enable Banking no admite la dirección de vuelta {}: añádela a las \
                     direcciones de vuelta de tu aplicación en su panel (o cambia aquí la \
                     dirección por una que tengas registrada)",
                    peticion.url_vuelta
                )));
            }
            return Err(e.into());
        }
    };
    let url = url::Url::parse(&auth.url)
        .ok()
        .filter(|u| u.scheme() == "https")
        .ok_or_else(|| {
            CommandError::from("Enable Banking ha dado una dirección que no es https")
        })?;
    app.opener().open_url(url.as_str(), None::<&str>)?;
    Ok(Inicio {
        escucha: vuelta::local(&peticion.url_vuelta).is_some(),
    })
}

/// La autorización pendiente a la que corresponde la vuelta (por su `state`).
fn pendiente_de(state: &AppState, company: &str, v: &Vuelta) -> CmdResult<i64> {
    Ok(banca::pendiente(
        &*state.vault_of(company)?,
        v.state(),
        ahora(),
    )?)
}

/// Con la vuelta del banco (ya comprobado que es la de la autorización `link`): abre la sesión,
/// guarda sus cuentas y baja los movimientos.
async fn terminar(
    state: &AppState,
    company: &str,
    link: i64,
    v: Vuelta,
) -> CmdResult<Actualizacion> {
    let code = match v {
        Vuelta::Error {
            error, descripcion, ..
        } => {
            banca::descartar_pendiente(&*state.vault_of(company)?)?;
            return Err(CommandError::from(format!(
                "el banco no ha dado acceso ({error}{}): vuelve a conectar si quieres",
                descripcion.map(|d| format!(": {d}")).unwrap_or_default()
            )));
        }
        Vuelta::Codigo { code, .. } => code,
    };
    let cliente = banca::cliente(&*state.vault_of(company)?)?;
    let sesion = cliente.sesion(&code).await?;
    let guardada = state
        .vault_of(company)
        .and_then(|v| Ok(banca::guardar_sesion(&v, link, &sesion, ahora())?));
    let viejas = match guardada {
        Ok(v) => v,
        Err(e) => {
            // Que no quede abierta en Enable Banking una sesión que TaxPain no ha guardado.
            if let Err(x) = cliente.borrar_sesion(&sesion.session_id).await {
                tracing::warn!(error = %x, "no se ha podido cerrar la sesión no guardada");
            }
            return Err(e);
        }
    };
    for s in viejas {
        if let Err(e) = cliente.borrar_sesion(&s).await {
            tracing::warn!(error = %e, "no se ha podido retirar la sesión anterior");
        }
    }
    Ok(banca::actualizar(state.vault_fn(company), &cliente, Some(link), ahora()).await?)
}

/// Espera la vuelta del banco en 127.0.0.1 (si la dirección de vuelta es de ahí) y termina.
/// `None` si se ha dejado de esperar (se ha pegado la dirección o se ha cancelado).
#[tauri::command]
pub async fn banca_esperar(state: tauri::State<'_, AppState>) -> CmdResult<Option<Actualizacion>> {
    let turno = TURNO.load(Ordering::SeqCst);
    let (company, url, esperado) = {
        let (id, v) = state.vault_with_id()?;
        let esperado = banca::state_pendiente(&v, ahora())?
            .ok_or_else(|| CommandError::from("no hay ninguna autorización empezada"))?;
        (id, banca::ajustes(&v)?.url_vuelta, esperado)
    };
    let (puerto, ruta) = vuelta::local(&url).ok_or_else(|| {
        CommandError::from("la dirección de vuelta no es de 127.0.0.1: pega la dirección del banco")
    })?;
    let seguir = || TURNO.load(Ordering::SeqCst) == turno;
    let v = match vuelta::esperar(puerto, &ruta, &esperado, ESPERA, seguir).await {
        Ok(v) => v,
        Err(_) if !seguir() => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    otro_turno();
    let link = pendiente_de(&state, &company, &v)?;
    terminar(&state, &company, link, v).await.map(Some)
}

/// Termina con la dirección a la que ha llevado el banco, pegada por el usuario. Si no es la de
/// la autorización empezada, no cambia nada (ni para la espera en 127.0.0.1).
#[tauri::command]
pub async fn banca_terminar(
    state: tauri::State<'_, AppState>,
    direccion: String,
) -> CmdResult<Actualizacion> {
    let v = vuelta::leer(&direccion)?;
    let company = state.active_id()?;
    let link = pendiente_de(&state, &company, &v)?;
    otro_turno();
    terminar(&state, &company, link, v).await
}

#[tauri::command]
pub fn banca_cancelar(state: tauri::State<'_, AppState>) -> CmdResult<Estado> {
    otro_turno();
    let v = state.vault()?;
    banca::descartar_pendiente(&v)?;
    Ok(banca::estado(&v, ahora())?)
}

/// Baja los movimientos nuevos de las cuentas activas (de una conexión o de todas).
#[tauri::command]
pub async fn banca_actualizar(
    state: tauri::State<'_, AppState>,
    conexion: Option<i64>,
) -> CmdResult<Actualizacion> {
    let (company, cliente) = {
        let (id, v) = state.vault_with_id()?;
        (id, banca::cliente(&v)?)
    };
    Ok(banca::actualizar(state.vault_fn(&company), &cliente, conexion, ahora()).await?)
}

#[tauri::command]
pub fn banca_activar_cuenta(
    state: tauri::State<'_, AppState>,
    cuenta: i64,
    activa: bool,
) -> CmdResult<Estado> {
    let v = state.vault()?;
    banca::activar_cuenta(&v, cuenta, activa)?;
    Ok(banca::estado(&v, ahora())?)
}

#[derive(serde::Serialize)]
pub struct Desconexion {
    pub estado: Estado,
    /// Lo que tiene que hacer el usuario si no se ha podido retirar el permiso.
    pub aviso: Option<String>,
}

/// Retira el permiso en Enable Banking y, si se ha podido (o ya no existía), quita la conexión.
/// Sin la clave de la aplicación no se puede retirar: se quita igual y se avisa de que hay que
/// retirarlo en el banco. Los movimientos ya importados se quedan.
#[tauri::command]
pub async fn banca_desconectar(
    state: tauri::State<'_, AppState>,
    conexion: i64,
) -> CmdResult<Desconexion> {
    let (company, sesion, cliente) = {
        let (id, v) = state.vault_with_id()?;
        let sesion = banca::sesion_de(&v, conexion)?;
        (id, sesion, banca::cliente_para_retirar(&v))
    };
    let mut aviso = None;
    if let Some(s) = sesion {
        match cliente {
            Ok(c) => match c.borrar_sesion(&s).await {
                Ok(()) | Err(taxpain_banca::BancaError::Caducado) => {}
                Err(e) => {
                    return Err(CommandError::from(format!(
                        "no se ha podido retirar el permiso en Enable Banking ({e}): la conexión \
                         sigue; inténtalo más tarde o retira el acceso desde tu banco"
                    )));
                }
            },
            Err(_) => {
                aviso = Some(
                    "Sin la clave de la aplicación no se ha podido retirar el permiso en Enable \
                     Banking: retira el acceso de TaxPain desde tu banco (o deja que caduque)."
                        .to_owned(),
                );
            }
        }
    }
    let v = state.vault_of(&company)?;
    banca::quitar(&v, conexion, ahora())?;
    Ok(Desconexion {
        estado: banca::estado(&v, ahora())?,
        aviso,
    })
}
