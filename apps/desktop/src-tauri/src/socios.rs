//! Comandos de socios y dividendos, administradores y gastos de viaje (la lógica está en
//! `taxpain_app::socios`, `taxpain_app::payroll` y `taxpain_app::viajes`).

use taxpain_app::actas::{self, Certificate};
use taxpain_app::dividendos::{self, Agreement, AgreementCheck, AgreementInput, Recorded};
use taxpain_app::payroll::{self, DirectorTurnover};
use taxpain_app::socios::{self, DividendPayment, PayoutInput, PayoutProposal, Shareholder};
use taxpain_app::viajes::{self, StoredTrip, Trip, TripCalc};
use time::{Date, OffsetDateTime};

use crate::CmdResult;
use crate::state::AppState;

// ---------- Socios y dividendos ----------

#[tauri::command]
pub fn shareholders_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Shareholder>> {
    Ok(socios::shareholders(&*state.vault()?)?)
}

#[tauri::command]
pub fn shareholder_save(
    state: tauri::State<'_, AppState>,
    shareholder: Shareholder,
) -> CmdResult<i64> {
    Ok(socios::save_shareholder(
        &*state.vault()?,
        &shareholder,
        OffsetDateTime::now_utc(),
    )?)
}

/// Borra un socio: Touch ID con su nombre y NIF.
#[tauri::command(async)]
pub fn shareholder_delete(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    crate::confirmar::borrar_socio(&state, id)
}

#[tauri::command]
pub fn dividend_propose(
    state: tauri::State<'_, AppState>,
    input: PayoutInput,
) -> CmdResult<PayoutProposal> {
    Ok(socios::propose(&*state.vault()?, &input)?)
}

/// Contabiliza el pago de dividendos (efectos fiscales): confirmación en la interfaz y Touch ID
/// con el importe bruto y la retención, calculados aquí.
#[tauri::command(async)]
pub fn dividend_pay(
    state: tauri::State<'_, AppState>,
    input: PayoutInput,
    confirmed: bool,
) -> CmdResult<i64> {
    let company = if confirmed {
        let (company, reason) = {
            let (company, v) = state.vault_with_id()?;
            let p = socios::propose(&v, &input)?;
            (
                company,
                format!(
                    "Contabilizar el pago de dividendos de {} (retención de {})",
                    p.gross, p.withheld
                ),
            )
        };
        state.confirm_presence(&reason)?;
        Some(company)
    } else {
        None
    };
    let v = match &company {
        Some(c) => state.vault_of(c)?,
        None => state.vault()?,
    };
    Ok(socios::pay(
        &v,
        &input,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn dividend_payments(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<Vec<DividendPayment>> {
    Ok(socios::payments_of_year(&*state.vault()?, year)?)
}

/// Certificados de retenciones de dividendos de `year`, uno por socio (art. 108.3 RIRPF).
#[tauri::command]
pub fn dividend_certificates(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<Vec<Certificate>> {
    Ok(actas::certificates(&*state.vault()?, year)?)
}

/// Guarda un acta (aprobación de cuentas, dividendos o reservas) o los certificados de los
/// socios en RTF (editable) o PDF.
#[tauri::command]
pub fn actas_save(
    state: tauri::State<'_, AppState>,
    request: actas::Request,
    path: String,
    pdf: bool,
) -> CmdResult<()> {
    let docs = actas::docs(&*state.vault()?, &request)?;
    if pdf {
        std::fs::write(&path, actas::pdf("Actas", &docs)?)?;
    } else {
        std::fs::write(&path, actas::rtf(&docs))?;
    }
    Ok(())
}

/// Deshace un pago de dividendos y borra su asiento: confirmación en la interfaz y Touch ID con
/// la fecha, el bruto y la retención guardados.
#[tauri::command(async)]
pub fn dividend_undo(state: tauri::State<'_, AppState>, id: i64, confirmed: bool) -> CmdResult<()> {
    crate::confirmar::deshacer_dividendo(&state, id, confirmed)
}

// ---------- Acuerdos de dividendos ----------

/// Semáforo de las comprobaciones legales, estado de liquidez y asiento de un acuerdo.
#[tauri::command]
pub fn dividend_agreement_check(
    state: tauri::State<'_, AppState>,
    input: AgreementInput,
) -> CmdResult<AgreementCheck> {
    Ok(dividendos::check(&*state.vault()?, &input)?)
}

/// Contabiliza el acuerdo (557 o reservas a 526) y crea el acta en borrador: confirmación en la
/// interfaz y Touch ID con la clase y el importe, calculados aquí.
#[tauri::command(async)]
pub fn dividend_agreement_record(
    state: tauri::State<'_, AppState>,
    input: AgreementInput,
    confirmed: bool,
) -> CmdResult<Recorded> {
    let company = if confirmed {
        let (company, reason) = {
            let (company, v) = state.vault_with_id()?;
            let c = dividendos::check(&v, &input)?;
            let what = match c.kind {
                dividendos::AgreementKind::ACuenta => "un dividendo a cuenta",
                dividendos::AgreementKind::Reservas => "un dividendo con cargo a reservas",
                dividendos::AgreementKind::Resultado => "el dividendo del resultado",
            };
            (
                company,
                format!("Acordar {what} de {} y crear su acta", c.amount),
            )
        };
        state.confirm_presence(&reason)?;
        Some(company)
    } else {
        None
    };
    let v = match &company {
        Some(c) => state.vault_of(c)?,
        None => state.vault()?,
    };
    Ok(dividendos::record(
        &v,
        &input,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn dividend_agreements(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Agreement>> {
    Ok(dividendos::agreements(&*state.vault()?)?)
}

/// Deshace un acuerdo y su asiento: confirmación en la interfaz y Touch ID.
#[tauri::command(async)]
pub fn dividend_agreement_undo(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    crate::confirmar::deshacer_acuerdo_dividendo(&state, id, confirmed)
}

// ---------- Administradores ----------

/// Cifra de negocios del último ejercicio cerrado antes de `date` (35 % o 19 %).
#[tauri::command]
pub fn director_turnover(
    state: tauri::State<'_, AppState>,
    date: Date,
) -> CmdResult<DirectorTurnover> {
    Ok(payroll::director_turnover(&*state.vault()?, date)?)
}

// ---------- Gastos de viaje ----------

#[tauri::command]
pub fn travel_preview(state: tauri::State<'_, AppState>, trip: Trip) -> CmdResult<TripCalc> {
    Ok(viajes::preview(&*state.vault()?, &trip)?)
}

#[tauri::command]
pub fn travel_register(state: tauri::State<'_, AppState>, trip: Trip) -> CmdResult<i64> {
    Ok(viajes::register(
        &*state.vault()?,
        &trip,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn travel_list(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<Vec<StoredTrip>> {
    Ok(viajes::trips_of_year(&*state.vault()?, year)?)
}

/// Borra un viaje y su asiento: Touch ID con quién, adónde y cuándo.
#[tauri::command(async)]
pub fn travel_delete(state: tauri::State<'_, AppState>, id: i64, confirmed: bool) -> CmdResult<()> {
    crate::confirmar::borrar_viaje(&state, id, confirmed)
}
