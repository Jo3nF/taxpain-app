//! Comandos de las nóminas calculadas (G45): ficha de cotización, cálculo y registro del mes,
//! recibo de salarios en PDF, seguros sociales del mes con su pago y finiquito. La lógica está
//! en `taxpain_app::nominas` y `taxpain_core::nomina`. Los asientos piden confirmación en la
//! interfaz y quedan en la auditoría. Nada se envía a la TGSS.

use taxpain_app::nominas::{self, FichaNomina, PagoSegurosInput, RegistroNomina, ResumenSeguros};
use taxpain_core::nomina::finiquito::{DatosFiniquito, Finiquito};
use taxpain_core::nomina::tablas::TablaCotizacion;
use taxpain_core::nomina::{DatosNomina, Nomina};
use time::OffsetDateTime;

use crate::CmdResult;
use crate::salida;
use crate::state::AppState;

fn ahora() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

#[tauri::command]
pub fn nomina_ficha(
    state: tauri::State<'_, AppState>,
    employee_id: i64,
) -> CmdResult<Option<FichaNomina>> {
    Ok(nominas::ficha(&*state.vault()?, employee_id)?)
}

#[tauri::command]
pub fn nomina_ficha_save(state: tauri::State<'_, AppState>, ficha: FichaNomina) -> CmdResult<()> {
    Ok(nominas::save_ficha(&*state.vault()?, &ficha, ahora())?)
}

/// Tabla de cotización del año (bases, topes, tipos, norma y lo que falta cotejar).
#[tauri::command]
pub fn nomina_tabla(year: i32) -> Option<TablaCotizacion> {
    taxpain_core::nomina::tablas::tabla(year).copied()
}

#[tauri::command]
pub fn nomina_calcular(datos: DatosNomina) -> CmdResult<Nomina> {
    Ok(nominas::calcular(&datos)?)
}

#[tauri::command]
pub fn nomina_registrar(
    state: tauri::State<'_, AppState>,
    registro: RegistroNomina,
) -> CmdResult<i64> {
    Ok(nominas::registrar(&*state.vault()?, &registro, ahora())?)
}

/// Recibos del año con nómina calculada (los que tienen recibo de salarios en PDF).
#[tauri::command]
pub fn nominas_calculadas(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<Vec<i64>> {
    Ok(nominas::calculadas(&*state.vault()?, year)?)
}

/// Recibo de salarios (PDF) guardado en `dir`; devuelve la ruta.
#[tauri::command]
pub fn nomina_recibo_pdf(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    payslip_id: i64,
    dir: String,
) -> CmdResult<String> {
    let (name, pdf) = nominas::recibo_pdf(&*state.vault()?, payslip_id)?;
    let path = salida::en_carpeta(&app, &dir, &name, &["pdf"])?;
    salida::escribir(&path, pdf)?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn seguros_sociales(
    state: tauri::State<'_, AppState>,
    year: i32,
    month: u8,
) -> CmdResult<ResumenSeguros> {
    Ok(nominas::seguros_sociales(&*state.vault()?, year, month)?)
}

/// Registra el pago de los seguros sociales del mes (476 contra tesorería).
#[tauri::command]
pub fn seguros_sociales_pagar(
    state: tauri::State<'_, AppState>,
    pago: PagoSegurosInput,
    confirmed: bool,
) -> CmdResult<i64> {
    Ok(nominas::pagar_seguros(
        &*state.vault()?,
        &pago,
        confirmed,
        ahora(),
    )?)
}

#[tauri::command]
pub fn finiquito_calcular(datos: DatosFiniquito) -> CmdResult<Finiquito> {
    Ok(nominas::calcular_finiquito(&datos)?)
}
