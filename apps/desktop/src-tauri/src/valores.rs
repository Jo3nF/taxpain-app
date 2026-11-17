//! Comandos de Inversiones en valores (la lógica está en `taxpain-app::valores`). El token del
//! Flex Web Service de Interactive Brokers entra una vez y no vuelve a la interfaz; la descarga
//! se hace sólo al pulsar «Sincronizar» y sin el almacén cogido.

use taxpain_app::valores::importar::{self, Aplicado, Lectura};
use taxpain_app::valores::{
    self, Cuenta, Op, PrecioCierre, Report, Settings, Valor, contabilidad, export,
};
use time::{Date, OffsetDateTime};

use crate::state::AppState;
use crate::{CmdResult, read_limited};

/// Tope de los ficheros que se eligen con el diálogo.
const MAX_FILE: u64 = 64 * 1024 * 1024;

fn ahora() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

fn hoy() -> Date {
    taxpain_core::spain_time::peninsular_date(ahora())
}

#[tauri::command]
pub fn valores_report(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<Report> {
    Ok(valores::report(&*state.vault()?, year)?)
}

#[tauri::command]
pub fn valores_settings(state: tauri::State<'_, AppState>) -> CmdResult<Settings> {
    Ok(valores::settings(&*state.vault()?)?)
}

#[tauri::command]
pub fn valores_save_settings(
    state: tauri::State<'_, AppState>,
    settings: Settings,
) -> CmdResult<()> {
    Ok(valores::save_settings(
        &*state.vault()?,
        &settings,
        ahora(),
    )?)
}

#[tauri::command]
pub fn valores_accounts(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Cuenta>> {
    Ok(valores::cuentas(&*state.vault()?)?)
}

#[tauri::command]
pub fn valores_save_account(state: tauri::State<'_, AppState>, account: Cuenta) -> CmdResult<i64> {
    Ok(valores::guardar_cuenta(
        &*state.vault()?,
        &account,
        ahora(),
    )?)
}

/// Touch ID con el nombre de la cuenta.
#[tauri::command(async)]
pub fn valores_delete_account(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    crate::confirmar::borrar_cuenta_valores(&state, id)
}

#[tauri::command]
pub fn valores_securities(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Valor>> {
    Ok(valores::valores(&*state.vault()?)?)
}

#[tauri::command]
pub fn valores_save_security(state: tauri::State<'_, AppState>, security: Valor) -> CmdResult<i64> {
    Ok(valores::guardar_valor(
        &*state.vault()?,
        &security,
        ahora(),
    )?)
}

#[tauri::command]
pub fn valores_ops(state: tauri::State<'_, AppState>, year: Option<i32>) -> CmdResult<Vec<Op>> {
    Ok(valores::ops(&*state.vault()?, year)?)
}

#[tauri::command]
pub fn valores_save_op(state: tauri::State<'_, AppState>, op: Op) -> CmdResult<i64> {
    Ok(valores::guardar_op(&*state.vault()?, &op, ahora())?)
}

/// Touch ID con la fecha, el tipo y el valor de la operación.
#[tauri::command(async)]
pub fn valores_delete_op(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    crate::confirmar::borrar_op_valores(&state, id)
}

#[tauri::command]
pub fn valores_prices(state: tauri::State<'_, AppState>) -> CmdResult<Vec<PrecioCierre>> {
    Ok(valores::precios(&*state.vault()?)?)
}

#[tauri::command]
pub fn valores_set_prices(
    state: tauri::State<'_, AppState>,
    prices: Vec<PrecioCierre>,
) -> CmdResult<usize> {
    Ok(valores::guardar_precios(
        &*state.vault()?,
        &prices,
        ahora(),
    )?)
}

/// Touch ID con el valor y la fecha del precio.
#[tauri::command(async)]
pub fn valores_delete_price(
    state: tauri::State<'_, AppState>,
    date: Date,
    security_id: i64,
) -> CmdResult<()> {
    crate::confirmar::borrar_precio_valores(&state, date, security_id)
}

/// Lee uno o varios ficheros de bróker (PDF de XTB, Flex de IBKR) sin guardar nada. El texto de
/// los PDF (y, si no lo tienen, el que reconoce el Mac) se saca sin el almacén cogido.
#[tauri::command]
pub async fn valores_read_files(
    state: tauri::State<'_, AppState>,
    paths: Vec<String>,
) -> CmdResult<Lectura> {
    let mut ficheros = Vec::new();
    for path in &paths {
        let p = std::path::Path::new(path);
        let name = p
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        ficheros.push((name, read_limited(p, MAX_FILE)?));
    }
    let empresa = state.active_id()?;
    let mut lectura =
        tauri::async_runtime::spawn_blocking(move || importar::leer_ficheros(&ficheros))
            .await
            .map_err(|e| crate::CommandError::from(e.to_string()))?;
    importar::contar(&*state.vault_of(&empresa)?, &mut lectura)?;
    Ok(lectura)
}

/// Guarda lo leído (sin duplicar lo que ya estaba).
#[tauri::command]
pub fn valores_import(state: tauri::State<'_, AppState>, lectura: Lectura) -> CmdResult<Aplicado> {
    let kind = if lectura
        .extractos
        .iter()
        .any(|e| e.broker == valores::Broker::Xtb)
    {
        "xtb-pdf"
    } else {
        "flex"
    };
    Ok(importar::aplicar(
        &*state.vault()?,
        &lectura.extractos,
        kind,
        "",
        ahora(),
    )?)
}

/// Guarda el token del Flex Web Service y el Query ID de una cuenta de IBKR. El token se borra
/// de memoria al terminar.
#[tauri::command]
pub fn valores_ibkr_connect(
    state: tauri::State<'_, AppState>,
    id: i64,
    token: String,
    query: String,
) -> CmdResult<Cuenta> {
    let token = zeroize::Zeroizing::new(token);
    let v = state.vault()?;
    valores::guardar_token_ibkr(&v, id, &token, &query, ahora())?;
    Ok(valores::cuenta(&v, id)?)
}

#[tauri::command]
pub fn valores_ibkr_disconnect(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<Cuenta> {
    let v = state.vault()?;
    valores::quitar_token_ibkr(&v, id, ahora())?;
    Ok(valores::cuenta(&v, id)?)
}

/// Una sola sincronización con IBKR a la vez: dos a la par duplicarían las peticiones al Flex
/// Web Service (límite por token, error 1018) aunque no las operaciones.
static SINCRONIZANDO: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Turno de sincronización: se suelta al terminar (también si falla).
struct Turno(&'static std::sync::atomic::AtomicBool);

impl Turno {
    fn tomar(bandera: &'static std::sync::atomic::AtomicBool) -> Option<Turno> {
        (!bandera.swap(true, std::sync::atomic::Ordering::SeqCst)).then_some(Turno(bandera))
    }
}

impl Drop for Turno {
    fn drop(&mut self) {
        self.0.store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Descarga del Flex Web Service el año `year` de la cuenta `id` y lo guarda sin duplicar. La
/// red, sin el almacén cogido; sólo con `ndcdyn.interactivebrokers.com`.
#[tauri::command]
pub async fn valores_ibkr_sync(
    state: tauri::State<'_, AppState>,
    id: i64,
    year: i32,
) -> CmdResult<Aplicado> {
    let Some(_turno) = Turno::tomar(&SINCRONIZANDO) else {
        return Err(crate::CommandError::from(
            "ya hay una sincronización con Interactive Brokers en curso: espera a que termine",
        ));
    };
    let (empresa, token, query) = {
        let (empresa, v) = state.vault_with_id()?;
        let (token, query) = valores::credenciales_ibkr(&v, id)?;
        (empresa, token, query)
    };
    let (desde, hasta) = importar::periodo_sincronizacion(year, hoy())?;
    let cliente = taxpain_banca::ibkr::ClienteFlex::nuevo()?;
    let mut textos = Vec::new();
    for (a, b) in taxpain_banca::ibkr::tramos(desde, hasta) {
        textos.push(cliente.descargar(&token, &query, a, b).await?);
    }
    drop(token);
    let v = state.vault_of(&empresa)?;
    Ok(importar::aplicar_descarga(&v, id, &textos, ahora())?)
}

/// Lo que hizo «Pasar a euros con el BCE».
#[derive(Clone, Debug, serde::Serialize)]
pub struct PasoBce {
    /// Divisas consultadas.
    pub divisas: Vec<String>,
    /// Tipos guardados.
    pub tipos: usize,
    /// Operaciones que ya tienen euros.
    pub operaciones: usize,
}

/// «Pasar a euros con el BCE»: para cada divisa con operaciones sin euros, descarga en una sola
/// consulta los tipos de referencia del BCE de su periodo (sólo la moneda y las fechas van al
/// BCE) y pasa a euros esas operaciones. Sólo cuando el usuario lo pulsa; la red, sin el almacén
/// cogido.
#[tauri::command]
pub async fn valores_bce(state: tauri::State<'_, AppState>) -> CmdResult<PasoBce> {
    let (empresa, pendientes) = {
        let (empresa, v) = state.vault_with_id()?;
        (empresa, valores::pendientes_bce(&v)?)
    };
    let mut out = PasoBce {
        divisas: Vec::new(),
        tipos: 0,
        operaciones: 0,
    };
    for (divisa, desde, hasta) in pendientes {
        if taxpain_core::fx::currency(&divisa).is_none_or(|c| !c.ecb) {
            continue;
        }
        let tipos = taxpain_aeat::bce::reference_rates(&divisa, desde, hasta).await?;
        let tipos: Vec<(Date, taxpain_core::fx::ExchangeRate)> =
            tipos.into_iter().map(|r| (r.date, r.rate)).collect();
        let v = state.vault_of(&empresa)?;
        out.operaciones +=
            valores::aplicar_bce(&v, &divisa, &tipos, taxpain_aeat::bce::HOST, ahora())?;
        out.tipos += tipos.len();
        out.divisas.push(divisa);
    }
    Ok(out)
}

/// Guarda el informe en `dir` como XLSX o PDF; devuelve la ruta.
#[tauri::command]
pub fn valores_export(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    year: i32,
    dir: String,
    pdf: bool,
) -> CmdResult<String> {
    let v = state.vault()?;
    let (name, bytes) = if pdf {
        export::pdf_bytes(&v, year)?
    } else {
        export::workbook(&v, year)?
    };
    let path = crate::salida::en_carpeta(&app, &dir, &name, &["pdf", "xlsx"])?;
    crate::salida::escribir(&path, bytes)?;
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn una_sincronizacion_a_la_vez() {
        static B: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        let t = Turno::tomar(&B);
        assert!(t.is_some());
        assert!(Turno::tomar(&B).is_none());
        drop(t);
        assert!(Turno::tomar(&B).is_some());
    }
}

// ---------------- Contabilidad ----------------

/// Graba los asientos de las operaciones del ejercicio (lote deshacible).
#[tauri::command]
pub fn valores_post_entries(
    state: tauri::State<'_, AppState>,
    year: i32,
    redo: bool,
    confirmed: bool,
) -> CmdResult<contabilidad::Contabilizado> {
    Ok(contabilidad::contabilizar(
        &*state.vault()?,
        year,
        redo,
        ahora(),
        confirmed,
    )?)
}

/// Quita los asientos de las operaciones del ejercicio.
#[tauri::command]
pub fn valores_undo_entries(
    state: tauri::State<'_, AppState>,
    year: i32,
    confirmed: bool,
) -> CmdResult<usize> {
    Ok(contabilidad::deshacer(
        &*state.vault()?,
        year,
        ahora(),
        confirmed,
    )?)
}

/// Valoración de la cartera al cierre (paso del cierre).
#[tauri::command]
pub fn valores_closing(
    state: tauri::State<'_, AppState>,
    year: i32,
    input: Option<contabilidad::EntradaCierre>,
) -> CmdResult<contabilidad::CierreValores> {
    Ok(contabilidad::cierre(&*state.vault()?, year, input)?)
}

#[tauri::command]
pub fn valores_closing_post(
    state: tauri::State<'_, AppState>,
    year: i32,
    input: contabilidad::EntradaCierre,
    confirmed: bool,
) -> CmdResult<i64> {
    Ok(contabilidad::contabilizar_cierre(
        &*state.vault()?,
        year,
        input,
        ahora(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn valores_closing_undo(
    state: tauri::State<'_, AppState>,
    year: i32,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(contabilidad::deshacer_cierre(
        &*state.vault()?,
        year,
        ahora(),
        confirmed,
    )?)
}
