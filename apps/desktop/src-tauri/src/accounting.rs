//! Comandos de contabilidad (la lógica está en `taxpain-app::accounting`).

use taxpain_app::accounting::{self, CloseSummary, FiscalYearInfo, LedgerLine, StoredEntry};
use taxpain_app::venta_inmovilizado;
use taxpain_core::accounting::depreciation::FixedAsset;
use taxpain_core::accounting::{AccountBalance, JournalEntry};
use time::OffsetDateTime;

use crate::state::AppState;
use crate::{CmdResult, read_limited, salida};

/// Tope de los ficheros que se eligen con el diálogo (diarios, extractos, memorias en PDF).
const MAX_CHOSEN_FILE: u64 = 64 * 1024 * 1024;

/// Ejercicio número `year` (o el actual si no se indica), con sus fechas.
#[tauri::command]
pub fn fiscal_year_info(
    state: tauri::State<'_, AppState>,
    year: Option<i32>,
) -> CmdResult<FiscalYearInfo> {
    let today = crate::calendar::today();
    Ok(accounting::fiscal_year_info(&*state.vault()?, year, today)?)
}

#[tauri::command]
pub fn journal(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<Vec<StoredEntry>> {
    Ok(accounting::journal(&*state.vault()?, year)?)
}

#[tauri::command]
pub fn trial_balance(
    state: tauri::State<'_, AppState>,
    year: i32,
    digits: Option<usize>,
) -> CmdResult<Vec<AccountBalance>> {
    Ok(accounting::trial_balance(&*state.vault()?, year, digits)?)
}

#[tauri::command]
pub fn ledger(
    state: tauri::State<'_, AppState>,
    year: i32,
    account: String,
) -> CmdResult<Vec<LedgerLine>> {
    Ok(accounting::ledger_of(&*state.vault()?, year, &account)?)
}

#[tauri::command]
pub fn manual_entry(state: tauri::State<'_, AppState>, entry: JournalEntry) -> CmdResult<i64> {
    Ok(accounting::manual_entry(
        &*state.vault()?,
        &entry,
        OffsetDateTime::now_utc(),
    )?)
}

/// Plan de cuentas de la empresa con los movimientos del ejercicio.
#[tauri::command]
pub fn chart(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<Vec<accounting::ChartAccount>> {
    Ok(accounting::chart(&*state.vault()?, year)?)
}

/// Cuadro de cuentas del PGC de PYMES (para buscar al teclear).
#[tauri::command]
pub fn pgc_accounts() -> Vec<(&'static str, &'static str)> {
    taxpain_core::accounting::pgc::ACCOUNTS.to_vec()
}

#[tauri::command]
pub fn set_account(state: tauri::State<'_, AppState>, code: String, name: String) -> CmdResult<()> {
    Ok(accounting::set_account(
        &*state.vault()?,
        &code,
        &name,
        OffsetDateTime::now_utc(),
    )?)
}

/// Puntea o despuntea una línea del mayor.
#[tauri::command]
pub fn set_tick(state: tauri::State<'_, AppState>, line_id: i64, ticked: bool) -> CmdResult<()> {
    Ok(accounting::set_tick(
        &*state.vault()?,
        line_id,
        ticked,
        OffsetDateTime::now_utc(),
    )?)
}

/// Cambia un asiento manual.
#[tauri::command]
pub fn update_entry(
    state: tauri::State<'_, AppState>,
    id: i64,
    entry: JournalEntry,
) -> CmdResult<()> {
    Ok(accounting::update_manual_entry(
        &*state.vault()?,
        id,
        &entry,
        OffsetDateTime::now_utc(),
    )?)
}

/// Borra un asiento manual: además de `confirmed` (el diálogo), Touch ID con su número.
#[tauri::command(async)]
pub fn delete_entry(state: tauri::State<'_, AppState>, id: i64, confirmed: bool) -> CmdResult<()> {
    crate::confirmar::borrar_asiento(&state, id, confirmed)
}

/// Cierra el ejercicio. Además de `confirmed` (el diálogo), Touch ID con el ejercicio y la
/// empresa.
#[tauri::command(async)]
pub fn close_year(
    state: tauri::State<'_, AppState>,
    year: i32,
    confirmed: bool,
) -> CmdResult<CloseSummary> {
    let v = year_presence(&state, year, confirmed, "Cerrar")?;
    Ok(accounting::close_year(
        &v,
        year,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn fixed_assets(state: tauri::State<'_, AppState>) -> CmdResult<Vec<FixedAsset>> {
    Ok(accounting::fixed_assets(&*state.vault()?)?)
}

#[tauri::command]
pub fn add_fixed_asset(state: tauri::State<'_, AppState>, asset: FixedAsset) -> CmdResult<i64> {
    Ok(accounting::add_fixed_asset(
        &*state.vault()?,
        &asset,
        OffsetDateTime::now_utc(),
    )?)
}

/// Método de amortización contable y coeficiente fiscal propio (puntos básicos) de un elemento.
#[tauri::command]
pub fn set_depreciation_plan(
    state: tauri::State<'_, AppState>,
    id: i64,
    method: taxpain_core::accounting::depreciation::Method,
    tax_rate_bp: Option<i32>,
) -> CmdResult<()> {
    Ok(accounting::set_depreciation_plan(
        &*state.vault()?,
        id,
        method,
        tax_rate_bp.map(taxpain_core::money::Rate::from_bp),
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn post_depreciation(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<usize> {
    Ok(accounting::post_depreciation(&*state.vault()?, year, OffsetDateTime::now_utc())?.len())
}

#[tauri::command]
pub fn annual_accounts(
    state: tauri::State<'_, AppState>,
    year: i32,
    model: Option<taxpain_core::accounting::statements::ModelKind>,
) -> CmdResult<accounting::AnnualAccounts> {
    Ok(accounting::annual_accounts_model(
        &*state.vault()?,
        year,
        model.unwrap_or_default(),
    )?)
}

#[tauri::command]
pub fn settle_vat(
    state: tauri::State<'_, AppState>,
    year: i32,
    quarter: u8,
) -> CmdResult<Option<i64>> {
    Ok(accounting::settle_vat_quarter(
        &*state.vault()?,
        year,
        quarter,
        OffsetDateTime::now_utc(),
    )?)
}

/// Rehace la liquidación del IVA del trimestre con el 303 de ahora.
#[tauri::command]
pub fn redo_vat(
    state: tauri::State<'_, AppState>,
    year: i32,
    quarter: u8,
    confirmed: bool,
) -> CmdResult<Option<i64>> {
    Ok(accounting::redo_vat_quarter(
        &*state.vault()?,
        year,
        quarter,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Exporta los libros registro del ejercicio a `dir` (carpeta elegida); devuelve la ruta.
#[tauri::command]
pub fn export_books(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    year: i32,
    dir: String,
) -> CmdResult<String> {
    let (name, bytes) = taxpain_app::books::lsi_workbook(&*state.vault()?, year)?;
    let path = salida::en_carpeta(&app, &dir, &name, &["xlsx"])?;
    salida::escribir(&path, bytes)?;
    Ok(path.to_string_lossy().into_owned())
}

/// Con `confirmed`, pide Touch ID («Cerrar el ejercicio 2025 de ACME SL») sin el almacén
/// cogido y devuelve el almacén de esa misma empresa (si se ha cambiado mientras, error).
fn year_presence<'a>(
    state: &'a AppState,
    year: i32,
    confirmed: bool,
    verb: &str,
) -> CmdResult<crate::state::VaultGuard<'a>> {
    if !confirmed {
        return Ok(state.vault()?);
    }
    let (company, reason) = {
        let (company, v) = state.vault_with_id()?;
        let label = accounting::fiscal_year_info(&v, Some(year), crate::calendar::today())?.label;
        let name = taxpain_app::companies::info(&v)?.name;
        (
            company,
            format!(
                "{verb} el ejercicio {label} de {}",
                crate::invoicing::reason_text(&name)
            ),
        )
    };
    state.confirm_presence(&reason)?;
    state.vault_of(&company)
}

// ---------- Asientos predefinidos y periódicos ----------

use taxpain_app::templates::{self, Template};

#[tauri::command]
pub fn templates_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Template>> {
    Ok(templates::list(&*state.vault()?)?)
}

/// Periódicos con la fecha ya cumplida (`today` es la fecha local del usuario).
#[tauri::command]
pub fn templates_due(
    state: tauri::State<'_, AppState>,
    today: time::Date,
) -> CmdResult<Vec<Template>> {
    Ok(templates::due(&*state.vault()?, today)?)
}

#[tauri::command]
pub fn template_save(state: tauri::State<'_, AppState>, template: Template) -> CmdResult<i64> {
    Ok(templates::save(
        &*state.vault()?,
        &template,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn template_delete(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    Ok(templates::delete(
        &*state.vault()?,
        id,
        OffsetDateTime::now_utc(),
    )?)
}

/// Registra el asiento revisado hecho con la plantilla `id`.
#[tauri::command]
pub fn template_post(
    state: tauri::State<'_, AppState>,
    id: i64,
    entry: JournalEntry,
) -> CmdResult<i64> {
    Ok(templates::post(
        &*state.vault()?,
        id,
        &entry,
        OffsetDateTime::now_utc(),
    )?)
}

// ---------- Cartera de cobros y pagos ----------

use taxpain_app::cartera::{self, DueItem, PartyRow, PartyTerms, PaymentTerms};

#[tauri::command]
pub fn cartera_list(
    state: tauri::State<'_, AppState>,
    today: time::Date,
) -> CmdResult<Vec<DueItem>> {
    Ok(cartera::list(&*state.vault()?, today)?)
}

#[tauri::command]
pub fn payment_terms(state: tauri::State<'_, AppState>) -> CmdResult<PaymentTerms> {
    Ok(cartera::terms(&*state.vault()?)?)
}

#[tauri::command]
pub fn set_payment_terms(state: tauri::State<'_, AppState>, terms: PaymentTerms) -> CmdResult<()> {
    Ok(cartera::set_terms(&*state.vault()?, &terms)?)
}

#[tauri::command]
pub fn set_due_date(
    state: tauri::State<'_, AppState>,
    kind: String,
    id: i64,
    seq: i64,
    due: time::Date,
) -> CmdResult<()> {
    Ok(cartera::set_due_date(
        &*state.vault()?,
        &kind,
        id,
        seq,
        due,
    )?)
}

#[derive(serde::Deserialize)]
pub struct InstallmentInput {
    due_date: time::Date,
    amount: i64,
}

/// Desglosa lo pendiente de una factura en vencimientos (o, sin ellos, vuelve a calcularlos).
#[tauri::command]
pub fn set_installments(
    state: tauri::State<'_, AppState>,
    kind: String,
    id: i64,
    pending: Option<Vec<InstallmentInput>>,
) -> CmdResult<()> {
    let rows: Option<Vec<(time::Date, taxpain_core::money::Money)>> = pending.map(|p| {
        p.into_iter()
            .map(|i| (i.due_date, taxpain_core::money::Money::from_cents(i.amount)))
            .collect()
    });
    Ok(cartera::set_schedule(
        &*state.vault()?,
        &kind,
        id,
        rows.as_deref(),
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn party_terms_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<PartyRow>> {
    Ok(cartera::parties(&*state.vault()?)?)
}

#[tauri::command]
pub fn set_party_terms(
    state: tauri::State<'_, AppState>,
    account: String,
    terms: Option<PartyTerms>,
) -> CmdResult<()> {
    Ok(cartera::set_party_terms(
        &*state.vault()?,
        &account,
        terms.as_ref(),
        OffsetDateTime::now_utc(),
    )?)
}

/// Registra el cobro o pago de un vencimiento en caja u otra cuenta de tesorería. En una
/// factura en moneda extranjera, `amount_eur` son los euros cobrados o pagados si no son los
/// contabilizados (la diferencia va a 768/668).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn settle_invoice(
    state: tauri::State<'_, AppState>,
    kind: String,
    id: i64,
    seq: i64,
    date: time::Date,
    account: String,
    amount_eur: Option<taxpain_core::money::Money>,
    method: Option<String>,
) -> CmdResult<i64> {
    Ok(cartera::settle_with(
        &*state.vault()?,
        &kind,
        id,
        seq,
        date,
        &account,
        method.as_deref(),
        amount_eur,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn unsettle_invoice(
    state: tauri::State<'_, AppState>,
    kind: String,
    id: i64,
    seq: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(cartera::unsettle(
        &*state.vault()?,
        &kind,
        id,
        seq,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

// ---------- Remesas SEPA ----------

use taxpain_app::remesas::{self, BankChoice, Candidate, Remittance, SepaSettings};

#[tauri::command]
pub fn sepa_settings(state: tauri::State<'_, AppState>) -> CmdResult<SepaSettings> {
    Ok(remesas::settings(&*state.vault()?)?)
}

#[tauri::command]
pub fn save_sepa_settings(
    state: tauri::State<'_, AppState>,
    settings: SepaSettings,
) -> CmdResult<()> {
    Ok(remesas::save_settings(&*state.vault()?, &settings)?)
}

#[tauri::command]
pub fn remittance_candidates(
    state: tauri::State<'_, AppState>,
    kind: String,
    today: time::Date,
) -> CmdResult<Vec<Candidate>> {
    Ok(remesas::candidates(&*state.vault()?, &kind, today)?)
}

#[tauri::command]
pub fn remittance_banks(state: tauri::State<'_, AppState>) -> CmdResult<Vec<BankChoice>> {
    Ok(remesas::banks(&*state.vault()?)?)
}

#[tauri::command]
pub fn remittance_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Remittance>> {
    Ok(remesas::list(&*state.vault()?)?)
}

#[derive(serde::Deserialize)]
pub struct RemittancePick {
    invoice_id: i64,
    seq: i64,
}

/// Genera la remesa (no la envía: el fichero lo sube al banco la persona usuaria).
#[tauri::command]
pub fn remittance_create(
    state: tauri::State<'_, AppState>,
    kind: String,
    items: Vec<RemittancePick>,
    bank_iban: String,
    charge_date: time::Date,
) -> CmdResult<Remittance> {
    let picks: Vec<(i64, i64)> = items.iter().map(|i| (i.invoice_id, i.seq)).collect();
    Ok(remesas::create(
        &*state.vault()?,
        &kind,
        &picks,
        &bank_iban,
        charge_date,
        OffsetDateTime::now_utc(),
    )?)
}

/// Guarda el XML de la remesa en `dir` como `<referencia>.xml`.
#[tauri::command]
pub fn remittance_save(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: i64,
    dir: String,
) -> CmdResult<String> {
    let (reference, xml) = remesas::file(&*state.vault()?, id)?;
    let path = salida::en_carpeta(&app, &dir, &format!("{reference}.xml"), &["xml"])?;
    salida::escribir(&path, xml)?;
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn remittance_undo(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(remesas::undo(
        &*state.vault()?,
        id,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn remittance_return(
    state: tauri::State<'_, AppState>,
    id: i64,
    invoice_id: i64,
    seq: i64,
    date: time::Date,
    account: Option<String>,
) -> CmdResult<Option<i64>> {
    Ok(remesas::return_item(
        &*state.vault()?,
        id,
        invoice_id,
        seq,
        date,
        account.as_deref(),
        OffsetDateTime::now_utc(),
    )?)
}

/// Deshace un recibo devuelto anotado por error: vuelve a la remesa como cobrado.
#[tauri::command]
pub fn remittance_undo_return(
    state: tauri::State<'_, AppState>,
    id: i64,
    invoice_id: i64,
    seq: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(remesas::undo_return(
        &*state.vault()?,
        id,
        invoice_id,
        seq,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

// ---------- Importar asientos ----------

use taxpain_app::import::{self as importer, Batch, Preview};

fn read_import(path: &str) -> CmdResult<(String, Vec<u8>)> {
    let p = std::path::Path::new(path);
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok((name, read_limited(p, MAX_CHOSEN_FILE)?))
}

/// Lee un SUENLACE, Excel o CSV y dice qué se importaría (no toca nada).
#[tauri::command]
pub fn import_preview(state: tauri::State<'_, AppState>, path: String) -> CmdResult<Preview> {
    let (name, bytes) = read_import(&path)?;
    Ok(importer::preview(&*state.vault()?, &name, &bytes)?)
}

#[tauri::command]
pub fn import_entries(state: tauri::State<'_, AppState>, path: String) -> CmdResult<Preview> {
    let (name, bytes) = read_import(&path)?;
    Ok(importer::import(&*state.vault()?, &name, &bytes, OffsetDateTime::now_utc())?.1)
}

#[tauri::command]
pub fn import_batches(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Batch>> {
    Ok(importer::batches(&*state.vault()?)?)
}

#[tauri::command]
pub fn import_undo(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<usize> {
    Ok(importer::undo(
        &*state.vault()?,
        id,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

// ---------- Legalización de libros ----------

#[derive(serde::Serialize)]
pub struct LegalizationSaved {
    path: String,
    #[serde(flatten)]
    package: taxpain_app::legalizacion::Package,
}

/// Guarda en `dir` el ZIP de legalización del ejercicio (no lo presenta: eso se hace en
/// registradores.org con certificado).
#[tauri::command]
pub fn legalization_build(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    request: taxpain_app::legalizacion::Request,
    dir: String,
) -> CmdResult<LegalizationSaved> {
    let p =
        taxpain_app::legalizacion::build(&*state.vault()?, &request, OffsetDateTime::now_utc())?;
    let path = salida::en_carpeta(&app, &dir, &p.filename, &["zip"])?;
    salida::escribir(&path, &p.zip)?;
    Ok(LegalizationSaved {
        path: path.to_string_lossy().into_owned(),
        package: p,
    })
}

// ---------- Analítica y presupuestos ----------

use taxpain_app::analitica::{
    self, AnalyticLine, Center, CenterResult, Redistribution, Rule, Share,
};
use taxpain_app::presupuestos::{self, BudgetLine, Deviation};

#[tauri::command]
pub fn cost_centers(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Center>> {
    Ok(analitica::centers(&*state.vault()?)?)
}

#[tauri::command]
pub fn cost_center_save(state: tauri::State<'_, AppState>, center: Center) -> CmdResult<()> {
    Ok(analitica::save_center(
        &*state.vault()?,
        &center,
        OffsetDateTime::now_utc(),
    )?)
}

/// Borra un centro de coste (y sus repartos): Touch ID con su código y nombre.
#[tauri::command(async)]
pub fn cost_center_delete(
    state: tauri::State<'_, AppState>,
    code: String,
    confirmed: bool,
) -> CmdResult<()> {
    crate::confirmar::borrar_centro_coste(&state, &code, confirmed)
}

#[tauri::command]
pub fn cost_rules(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Rule>> {
    Ok(analitica::rules(&*state.vault()?)?)
}

#[tauri::command]
pub fn cost_rule_set(
    state: tauri::State<'_, AppState>,
    prefix: String,
    shares: Vec<Share>,
) -> CmdResult<()> {
    Ok(analitica::set_rule(
        &*state.vault()?,
        &prefix,
        &shares,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn analytic_lines(
    state: tauri::State<'_, AppState>,
    year: i32,
    from: Option<time::Date>,
    to: Option<time::Date>,
) -> CmdResult<Vec<AnalyticLine>> {
    Ok(analitica::lines(&*state.vault()?, year, from, to)?)
}

#[tauri::command]
pub fn analytic_line_set(
    state: tauri::State<'_, AppState>,
    line_id: i64,
    shares: Vec<Share>,
) -> CmdResult<()> {
    Ok(analitica::set_line(
        &*state.vault()?,
        line_id,
        &shares,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn analytic_report(
    state: tauri::State<'_, AppState>,
    year: i32,
    from: Option<time::Date>,
    to: Option<time::Date>,
    level: Option<usize>,
) -> CmdResult<Vec<CenterResult>> {
    Ok(analitica::report(&*state.vault()?, year, from, to, level)?)
}

#[tauri::command]
pub fn cost_redistributions(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Redistribution>> {
    Ok(analitica::redistributions(&*state.vault()?)?)
}

#[tauri::command]
pub fn cost_redistribution_set(
    state: tauri::State<'_, AppState>,
    source: String,
    shares: Vec<Share>,
) -> CmdResult<()> {
    Ok(analitica::set_redistribution(
        &*state.vault()?,
        &source,
        &shares,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn budget_list(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<Vec<BudgetLine>> {
    Ok(presupuestos::list(&*state.vault()?, year)?)
}

#[tauri::command]
pub fn budget_set(state: tauri::State<'_, AppState>, year: i32, line: BudgetLine) -> CmdResult<()> {
    Ok(presupuestos::set(
        &*state.vault()?,
        year,
        &line,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn budget_from_actuals(
    state: tauri::State<'_, AppState>,
    year: i32,
    source: i32,
    digits: usize,
    change_bp: i32,
) -> CmdResult<usize> {
    Ok(presupuestos::from_actuals(
        &*state.vault()?,
        year,
        source,
        digits,
        change_bp,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn budget_report(
    state: tauri::State<'_, AppState>,
    year: i32,
    through: u8,
) -> CmdResult<Vec<Deviation>> {
    Ok(presupuestos::report(&*state.vault()?, year, through)?)
}

// ---------- Existencias ----------

#[tauri::command]
pub fn stock_lines(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<Vec<taxpain_app::existencias::StockLine>> {
    Ok(taxpain_app::existencias::stock(&*state.vault()?, year)?)
}

#[derive(serde::Deserialize)]
pub struct StockFinal {
    account: String,
    amount: i64,
}

#[tauri::command]
pub fn stock_regularize(
    state: tauri::State<'_, AppState>,
    year: i32,
    finals: Vec<StockFinal>,
) -> CmdResult<Option<i64>> {
    let f: Vec<(String, taxpain_core::money::Money)> = finals
        .into_iter()
        .map(|x| (x.account, taxpain_core::money::Money::from_cents(x.amount)))
        .collect();
    Ok(taxpain_app::existencias::regularize(
        &*state.vault()?,
        year,
        &f,
        OffsetDateTime::now_utc(),
    )?)
}

// ---------- Depósito de cuentas ----------

#[derive(serde::Serialize)]
pub struct DepositSaved {
    path: String,
    #[serde(flatten)]
    package: taxpain_app::deposito::Package,
}

/// Guarda en `dir` el ZIP para importar en el programa D2 del Registro (no presenta nada).
#[tauri::command]
pub fn deposit_build(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    mut request: taxpain_app::deposito::Request,
    memoria: Option<String>,
    dir: String,
) -> CmdResult<DepositSaved> {
    if let Some(m) = memoria.filter(|m| !m.is_empty()) {
        request.memoria_pdf = Some(read_limited(m, MAX_CHOSEN_FILE)?);
    }
    let p = taxpain_app::deposito::build(&*state.vault()?, &request)?;
    let path = salida::en_carpeta(&app, &dir, &p.filename, &["zip"])?;
    salida::escribir(&path, &p.zip)?;
    Ok(DepositSaved {
        path: path.to_string_lossy().into_owned(),
        package: p,
    })
}

// ---------- Ratios ----------

#[tauri::command]
pub fn financial_ratios(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<Vec<taxpain_app::ratios::Ratio>> {
    Ok(taxpain_app::ratios::ratios(&*state.vault()?, year)?)
}

// ---------- a3 ----------

use taxpain_app::a3::{self, A3Options};

#[tauri::command]
pub fn a3_options(state: tauri::State<'_, AppState>) -> CmdResult<A3Options> {
    Ok(a3::options(&*state.vault()?)?)
}

#[derive(serde::Serialize)]
pub struct A3Saved {
    path: String,
    entries: usize,
    invoices: usize,
    accounts: usize,
    warnings: Vec<String>,
}

/// Escribe `SUENLACE.DAT` (a3 exige ese nombre) en `dir` con el diario del ejercicio.
#[tauri::command]
pub fn export_a3(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    year: i32,
    options: A3Options,
    dir: String,
) -> CmdResult<A3Saved> {
    let x = a3::suenlace(&*state.vault()?, year, &options)?;
    let path = salida::en_carpeta(&app, &dir, "SUENLACE.DAT", &["dat"])?;
    salida::escribir(&path, &x.file)?;
    Ok(A3Saved {
        path: path.to_string_lossy().into_owned(),
        entries: x.entries,
        invoices: x.invoices,
        accounts: x.accounts,
        warnings: x.warnings,
    })
}

// ---------- Banco ----------

use taxpain_app::bank::{self, BankTx, Suggestion};
use taxpain_app::extractos::importar::Resumen;

/// Lo que ha pasado con un fichero de extractos.
#[derive(serde::Serialize)]
pub struct BankFileResult {
    pub fichero: String,
    pub resumen: Option<Resumen>,
    pub error: Option<String>,
}

/// Importa extractos de cualquier banco y formato (Norma 43, camt, OFX, QIF, MT940, Excel,
/// CSV, PDF…). Cada fichero va por su lado: si uno falla, los demás entran igual. Con
/// `forzar`, también los que no cuadran con sus saldos. En un hilo aparte: leer un PDF largo
/// (o escaneado) tarda, y se lee antes de coger el almacén.
#[tauri::command(async)]
pub fn bank_import(
    state: tauri::State<'_, AppState>,
    paths: Vec<String>,
    forzar: Option<bool>,
) -> CmdResult<Vec<BankFileResult>> {
    let mut out = Vec::new();
    for p in paths {
        let fichero = std::path::Path::new(&p)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.clone());
        let bytes = match read_limited(&p, MAX_CHOSEN_FILE) {
            Ok(b) => b,
            Err(e) => {
                out.push(BankFileResult {
                    fichero,
                    resumen: None,
                    error: Some(e.message),
                });
                continue;
            }
        };
        // Lo lento, sin el almacén: lo leído se recuerda por la huella del fichero.
        let _ = taxpain_app::extractos::leer(&fichero, &bytes);
        let r = taxpain_app::extractos::importar::importar_fichero(
            &*state.vault()?,
            &fichero,
            &bytes,
            None,
            forzar.unwrap_or(false),
            OffsetDateTime::now_utc(),
        );
        out.push(match r {
            Ok(r) => BankFileResult {
                fichero,
                resumen: Some(r),
                error: None,
            },
            Err(e) => BankFileResult {
                fichero,
                resumen: None,
                error: Some(e.to_string()),
            },
        });
    }
    Ok(out)
}

#[tauri::command]
pub fn bank_movements(
    state: tauri::State<'_, AppState>,
    only_pending: bool,
) -> CmdResult<Vec<BankTx>> {
    Ok(bank::movements(&*state.vault()?, only_pending)?)
}

#[tauri::command]
pub fn bank_suggestions(state: tauri::State<'_, AppState>, tx: i64) -> CmdResult<Vec<Suggestion>> {
    Ok(bank::suggestions(&*state.vault()?, tx)?)
}

#[tauri::command]
pub fn bank_reconcile_invoice(
    state: tauri::State<'_, AppState>,
    tx: i64,
    kind: String,
    invoice: i64,
    seq: Option<i64>,
) -> CmdResult<i64> {
    Ok(bank::reconcile_invoice(
        &*state.vault()?,
        tx,
        &kind,
        invoice,
        seq,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn bank_reconcile_remittance(
    state: tauri::State<'_, AppState>,
    tx: i64,
    remittance: i64,
) -> CmdResult<i64> {
    Ok(bank::reconcile_remittance(
        &*state.vault()?,
        tx,
        remittance,
        OffsetDateTime::now_utc(),
    )?)
}

/// Casa un movimiento con el asiento ya hecho de un recibo devuelto de una remesa.
#[tauri::command]
pub fn bank_reconcile_return(
    state: tauri::State<'_, AppState>,
    tx: i64,
    entry: i64,
) -> CmdResult<()> {
    Ok(bank::reconcile_return(
        &*state.vault()?,
        tx,
        entry,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn bank_reconcile_account(
    state: tauri::State<'_, AppState>,
    tx: i64,
    account: String,
    concept: String,
) -> CmdResult<i64> {
    Ok(bank::reconcile_account(
        &*state.vault()?,
        tx,
        &account,
        &concept,
        OffsetDateTime::now_utc(),
    )?)
}

/// Movimientos conciliados (filtrados por subcuenta del banco y fechas) con lo que se casó cada
/// uno.
#[tauri::command]
pub fn bank_reconciled(
    state: tauri::State<'_, AppState>,
    account: Option<String>,
    from: Option<time::Date>,
    to: Option<time::Date>,
) -> CmdResult<taxpain_app::conciliados::ReconciledList> {
    Ok(taxpain_app::conciliados::reconciled(
        &*state.vault()?,
        account.as_deref(),
        from,
        to,
    )?)
}

#[tauri::command]
pub fn bank_unreconcile(
    state: tauri::State<'_, AppState>,
    tx: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(taxpain_app::conciliados::unreconcile(
        &*state.vault()?,
        tx,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

// ---------- Reglas del banco ----------

#[tauri::command]
pub fn bank_rules(
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<taxpain_app::bank_rules::Rule>> {
    Ok(taxpain_app::bank_rules::list(&*state.vault()?)?)
}

#[tauri::command]
pub fn bank_rule_delete(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    Ok(taxpain_app::bank_rules::delete(
        &*state.vault()?,
        id,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn bank_rule_for(
    state: tauri::State<'_, AppState>,
    tx: i64,
) -> CmdResult<Option<taxpain_app::bank_rules::Rule>> {
    Ok(taxpain_app::bank_rules::rule_for(&*state.vault()?, tx)?)
}

#[tauri::command]
pub fn bank_rule_proposals(
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<taxpain_app::bank_rules::Proposal>> {
    Ok(taxpain_app::bank_rules::proposals(&*state.vault()?)?)
}

/// Contabiliza con su regla los movimientos que el usuario ha revisado.
#[tauri::command]
pub fn bank_rule_apply(state: tauri::State<'_, AppState>, ids: Vec<i64>) -> CmdResult<usize> {
    Ok(taxpain_app::bank_rules::apply(
        &*state.vault()?,
        &ids,
        OffsetDateTime::now_utc(),
    )?)
}

// ---------- Nóminas ----------

#[tauri::command]
pub fn employees(
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<taxpain_app::payroll::Employee>> {
    Ok(taxpain_app::payroll::employees(&*state.vault()?)?)
}

#[tauri::command]
pub fn employee_save(
    state: tauri::State<'_, AppState>,
    employee: taxpain_app::payroll::Employee,
) -> CmdResult<i64> {
    Ok(taxpain_app::payroll::save_employee(
        &*state.vault()?,
        &employee,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn payslips(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<Vec<taxpain_app::payroll::StoredPayslip>> {
    Ok(taxpain_app::payroll::payslips_of_year(
        &*state.vault()?,
        year,
    )?)
}

#[tauri::command]
pub fn payslip_register(
    state: tauri::State<'_, AppState>,
    payslip: taxpain_app::payroll::PayslipInput,
) -> CmdResult<i64> {
    Ok(taxpain_app::payroll::register(
        &*state.vault()?,
        &payslip,
        OffsetDateTime::now_utc(),
    )?)
}

/// Borra una nómina y su asiento: Touch ID con el trabajador y la fecha.
#[tauri::command(async)]
pub fn payslip_delete(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    crate::confirmar::borrar_nomina(&state, id, confirmed)
}

// ---------- Autónomo societario (RETA del administrador) ----------

use taxpain_app::autonomo_societario::{self as reta, CuotaInput, FichaReta, ResumenReta};

/// Ficha del RETA, rendimiento computable, tramo y regularización estimada del año.
#[tauri::command]
pub fn reta_resumen(
    state: tauri::State<'_, AppState>,
    employee_id: i64,
    year: i32,
) -> CmdResult<ResumenReta> {
    let today = crate::calendar::today();
    Ok(reta::resumen(&*state.vault()?, employee_id, year, today)?)
}

#[tauri::command]
pub fn reta_ficha_save(state: tauri::State<'_, AppState>, ficha: FichaReta) -> CmdResult<()> {
    Ok(reta::save_ficha(
        &*state.vault()?,
        &ficha,
        OffsetDateTime::now_utc(),
    )?)
}

/// Registra una cuota del RETA (con su asiento si la paga la sociedad y se pide).
#[tauri::command]
pub fn reta_cuota_register(state: tauri::State<'_, AppState>, cuota: CuotaInput) -> CmdResult<i64> {
    Ok(reta::register_cuota(
        &*state.vault()?,
        &cuota,
        OffsetDateTime::now_utc(),
    )?)
}

/// Borra una cuota y su asiento: Touch ID con el administrador y la fecha.
#[tauri::command(async)]
pub fn reta_cuota_delete(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    crate::confirmar::borrar_cuota_reta(&state, id, confirmed)
}

/// Recibo de la retribución del administrador (PDF) guardado en `dir`; devuelve la ruta.
#[tauri::command]
pub fn admin_receipt_pdf(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    payslip_id: i64,
    place_date: String,
    dir: String,
) -> CmdResult<String> {
    let (name, pdf) = reta::recibo_pdf(&*state.vault()?, payslip_id, &place_date)?;
    let path = salida::en_carpeta(&app, &dir, &name, &["pdf"])?;
    salida::escribir(&path, pdf)?;
    Ok(path.to_string_lossy().into_owned())
}

/// Exporta los listados del ejercicio (diario, mayor, sumas y saldos, balance, PyG y cartera) a
/// un XLSX en `dir`; devuelve la ruta.
#[tauri::command]
pub fn export_listings(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    year: i32,
    dir: String,
) -> CmdResult<String> {
    let v = state.vault()?;
    let model = taxpain_app::models::settings(&v)?.accounts_model;
    let today = crate::calendar::today();
    let (name, bytes) = taxpain_app::listings::workbook(&v, year, model, today)?;
    let path = salida::en_carpeta(&app, &dir, &name, &["xlsx"])?;
    salida::escribir(&path, bytes)?;
    Ok(path.to_string_lossy().into_owned())
}

// ---------- Cuentas por defecto y por tercero ----------

#[tauri::command]
pub fn posting_accounts_get(
    state: tauri::State<'_, AppState>,
) -> CmdResult<taxpain_core::accounting::PostingAccounts> {
    let v = state.vault()?;
    Ok(accounting::posting_accounts(v.connection())?)
}

#[tauri::command]
pub fn posting_accounts_save(
    state: tauri::State<'_, AppState>,
    accounts: taxpain_core::accounting::PostingAccounts,
) -> CmdResult<()> {
    Ok(accounting::save_posting_accounts(
        &*state.vault()?,
        &accounts,
    )?)
}

#[tauri::command]
pub fn party_counterpart_set(
    state: tauri::State<'_, AppState>,
    account: String,
    counterpart: Option<String>,
) -> CmdResult<()> {
    Ok(accounting::set_party_counterpart(
        &*state.vault()?,
        &account,
        counterpart.as_deref(),
        OffsetDateTime::now_utc(),
    )?)
}

// ---------- Observaciones y adjuntos de los asientos ----------

#[tauri::command]
pub fn entry_notes_set(
    state: tauri::State<'_, AppState>,
    entry_id: i64,
    notes: String,
) -> CmdResult<()> {
    Ok(accounting::set_entry_notes(
        &*state.vault()?,
        entry_id,
        &notes,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn entry_documents(
    state: tauri::State<'_, AppState>,
    entry_id: i64,
) -> CmdResult<Vec<accounting::EntryDocument>> {
    Ok(accounting::entry_documents(&*state.vault()?, entry_id)?)
}

/// Adjunta al asiento el fichero `path` elegido por el usuario.
#[tauri::command]
pub fn entry_attach(
    state: tauri::State<'_, AppState>,
    entry_id: i64,
    path: String,
) -> CmdResult<i64> {
    let bytes = read_limited(&path, taxpain_app::accounting::MAX_ATTACHMENT as u64)?;
    Ok(accounting::attach_document(
        &*state.vault()?,
        entry_id,
        &path,
        &bytes,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn entry_detach(
    state: tauri::State<'_, AppState>,
    entry_id: i64,
    document_id: i64,
) -> CmdResult<()> {
    Ok(accounting::detach_document(
        &*state.vault()?,
        entry_id,
        document_id,
        OffsetDateTime::now_utc(),
    )?)
}

/// Abre un adjunto con la aplicación del sistema (copia temporal).
#[tauri::command]
pub fn entry_document_open(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    document_id: i64,
) -> CmdResult<()> {
    let (name, _, bytes) = accounting::entry_document(&*state.vault()?, document_id)?;
    // Carpeta temporal privada, nombre al azar, 0600 y borrado pasada una hora (como el resto
    // de documentos): no una ruta fija y predecible que se queda días en claro.
    let path = taxpain_app::documentos::temp_copy(&name, &bytes)?;
    salida::abrir_copia(&app, &path)
}

/// Baja o venta de un elemento del inmovilizado (amortización hasta la baja y asiento de baja);
/// con `invoice_id`, vendido con esa factura emitida (su base deja de contar como ventas).
#[tauri::command]
pub fn asset_dispose(
    state: tauri::State<'_, AppState>,
    id: i64,
    date: time::Date,
    price: taxpain_core::money::Money,
    invoice_id: Option<i64>,
) -> CmdResult<accounting::Disposal> {
    Ok(accounting::dispose_asset_with_invoice(
        &*state.vault()?,
        id,
        date,
        price,
        invoice_id,
        OffsetDateTime::now_utc(),
    )?)
}

/// Bajas del inmovilizado con su venta: facturas vinculadas, lo que sigue en la 543 y las
/// rectificativas pendientes.
#[tauri::command]
pub fn asset_sales(
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<venta_inmovilizado::AssetSale>> {
    Ok(venta_inmovilizado::asset_sales(&*state.vault()?)?)
}

/// Facturas emitidas que pueden ser la de la venta del elemento `id` (o las rectificativas
/// pendientes de su factura de venta); con `query`, las que llevan ese número, cliente o NIF.
#[tauri::command]
pub fn asset_sale_candidates(
    state: tauri::State<'_, AppState>,
    id: i64,
    query: Option<String>,
) -> CmdResult<Vec<venta_inmovilizado::SaleCandidate>> {
    Ok(venta_inmovilizado::sale_invoice_candidates(
        &*state.vault()?,
        id,
        query.as_deref().unwrap_or(""),
    )?)
}

/// Vincula una factura emitida a la venta del elemento `id` (`amount` en céntimos; sin él, el
/// propuesto).
#[tauri::command]
pub fn asset_sale_link(
    state: tauri::State<'_, AppState>,
    id: i64,
    invoice_id: i64,
    amount: Option<taxpain_core::money::Money>,
) -> CmdResult<venta_inmovilizado::SaleLink> {
    Ok(venta_inmovilizado::link_sale_invoice(
        &*state.vault()?,
        id,
        invoice_id,
        amount,
        OffsetDateTime::now_utc(),
    )?)
}

/// Desvincula una factura de la venta del elemento `id` (vuelve a contar como ventas).
#[tauri::command]
pub fn asset_sale_unlink(
    state: tauri::State<'_, AppState>,
    id: i64,
    invoice_id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(venta_inmovilizado::unlink_sale_invoice(
        &*state.vault()?,
        id,
        invoice_id,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Clientes y proveedores cuyo saldo contable no coincide con lo pendiente en la cartera.
#[tauri::command]
pub fn cartera_mismatches(
    state: tauri::State<'_, AppState>,
    today: time::Date,
) -> CmdResult<Vec<taxpain_app::cartera::Mismatch>> {
    Ok(taxpain_app::cartera::ledger_mismatches(
        &*state.vault()?,
        today,
    )?)
}

/// Periodo medio de pago a proveedores del ejercicio (para la memoria).
#[tauri::command]
pub fn payment_period(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<taxpain_app::cartera::PaymentPeriod> {
    let v = state.vault()?;
    let fy = accounting::fiscal_year(&v, year)?;
    Ok(taxpain_app::cartera::payment_period(&v, fy.start, fy.end)?)
}

/// Reabre un ejercicio cerrado (quita regularización, cierre y la apertura del siguiente).
/// Además de `confirmed` (el diálogo), Touch ID con el ejercicio y la empresa.
#[tauri::command(async)]
pub fn reopen_year(
    state: tauri::State<'_, AppState>,
    year: i32,
    confirmed: bool,
) -> CmdResult<usize> {
    let v = year_presence(&state, year, confirmed, "Reabrir")?;
    Ok(accounting::reopen_year(
        &v,
        year,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Borra varios asientos manuales o importados a la vez (todos o ninguno), con Touch ID.
#[tauri::command(async)]
pub fn delete_entries(
    state: tauri::State<'_, AppState>,
    ids: Vec<i64>,
    confirmed: bool,
) -> CmdResult<usize> {
    crate::confirmar::borrar_asientos(&state, &ids, confirmed)
}

/// Copia en la empresa abierta el plan de cuentas (nombres propios y cuentas por defecto) de
/// otra empresa del mismo Mac.
#[tauri::command]
pub fn chart_copy_from(
    state: tauri::State<'_, AppState>,
    company_id: String,
) -> CmdResult<accounting::ChartCopy> {
    if company_id == state.active_id()? {
        return Err(taxpain_app::error::AppError::Other(
            "elige otra empresa: ésa es la abierta".into(),
        )
        .into());
    }
    let from = state.open_company(&company_id)?;
    let to = state.vault()?;
    Ok(accounting::copy_chart(
        &from,
        &to,
        OffsetDateTime::now_utc(),
    )?)
}

/// Traspasa el saldo de una cuenta a otra con un asiento manual.
#[tauri::command]
pub fn transfer_balance(
    state: tauri::State<'_, AppState>,
    from: String,
    to: String,
    date: time::Date,
) -> CmdResult<Option<i64>> {
    Ok(accounting::transfer_balance(
        &*state.vault()?,
        &from,
        &to,
        date,
        OffsetDateTime::now_utc(),
    )?)
}

// ---------- Leasing ----------

#[tauri::command]
pub fn leasing_list(
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<taxpain_app::leasing::LeasingView>> {
    Ok(taxpain_app::leasing::list(&*state.vault()?)?)
}

#[tauri::command]
pub fn leasing_create(
    state: tauri::State<'_, AppState>,
    leasing: taxpain_core::accounting::leasing::Leasing,
    depreciation: taxpain_core::money::Rate,
) -> CmdResult<i64> {
    Ok(taxpain_app::leasing::create(
        &*state.vault()?,
        &leasing,
        depreciation,
        OffsetDateTime::now_utc(),
    )?)
}

/// Reclasifica a la 174 la deuda de los leasings que vence a más de un año al cierre de `year`.
#[tauri::command]
pub fn leasing_reclassify(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<usize> {
    Ok(taxpain_app::leasing::reclassify(
        &*state.vault()?,
        year,
        OffsetDateTime::now_utc(),
    )?)
}

/// Registra como facturas del arrendador las cuotas vencidas hasta `until` que falten.
#[tauri::command]
pub fn leasing_post_due(
    state: tauri::State<'_, AppState>,
    id: i64,
    until: time::Date,
) -> CmdResult<taxpain_app::leasing::Posted> {
    Ok(taxpain_app::leasing::post_due(
        &*state.vault()?,
        id,
        until,
        OffsetDateTime::now_utc(),
    )?)
}

/// Regulariza las cuotas de un leasing contabilizadas con el sistema antiguo (con confirmación).
#[tauri::command]
pub fn leasing_regularize(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<taxpain_app::leasing::Regularized> {
    Ok(taxpain_app::leasing::regularize(
        &*state.vault()?,
        id,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Pone el vencimiento de las cuotas sin pagar de un leasing en su fecha de cuota.
#[tauri::command]
pub fn leasing_fix_due_dates(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<usize> {
    Ok(taxpain_app::leasing::fix_due_dates(
        &*state.vault()?,
        id,
        OffsetDateTime::now_utc(),
    )?)
}

/// Borra un contrato de leasing dado de alta por error (con confirmación y Touch ID).
#[tauri::command(async)]
pub fn leasing_delete(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    crate::confirmar::borrar_leasing(&state, id, confirmed)
}

/// Pone el arrendador de un contrato de leasing.
#[tauri::command]
pub fn leasing_set_lessor(
    state: tauri::State<'_, AppState>,
    id: i64,
    lessor: taxpain_core::accounting::leasing::Lessor,
) -> CmdResult<()> {
    Ok(taxpain_app::leasing::set_lessor(
        &*state.vault()?,
        id,
        &lessor,
        OffsetDateTime::now_utc(),
    )?)
}

// ---------- Cierre guiado (taxpain_app::cierre) ----------

#[tauri::command]
pub fn closing_checklist(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<taxpain_app::cierre::Checklist> {
    Ok(taxpain_app::cierre::checklist(&*state.vault()?, year)?)
}

#[tauri::command]
pub fn closing_corporate_tax(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<taxpain_app::cierre::CorporateTax> {
    Ok(taxpain_app::cierre::corporate_tax(&*state.vault()?, year)?)
}

#[tauri::command]
pub fn closing_corporate_tax_post(
    state: tauri::State<'_, AppState>,
    year: i32,
    confirmed: bool,
) -> CmdResult<i64> {
    Ok(taxpain_app::cierre::post_corporate_tax(
        &*state.vault()?,
        year,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn closing_corporate_tax_undo(
    state: tauri::State<'_, AppState>,
    year: i32,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(taxpain_app::cierre::undo_corporate_tax(
        &*state.vault()?,
        year,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Saldos simultáneos de la 4700 y la 4750 (liquidaciones antiguas) y el ajuste propuesto.
#[tauri::command]
pub fn closing_vat_balances(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<Option<taxpain_app::cierre::VatBalances>> {
    Ok(taxpain_app::cierre::vat_balances(&*state.vault()?, year)?)
}

#[tauri::command]
pub fn closing_vat_balances_post(
    state: tauri::State<'_, AppState>,
    year: i32,
    confirmed: bool,
) -> CmdResult<i64> {
    Ok(taxpain_app::cierre::post_vat_balances(
        &*state.vault()?,
        year,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn closing_allocation(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<taxpain_app::cierre::AllocationProposal> {
    Ok(taxpain_app::cierre::allocation(&*state.vault()?, year)?)
}

#[tauri::command]
pub fn closing_allocation_post(
    state: tauri::State<'_, AppState>,
    year: i32,
    date: time::Date,
    allocation: taxpain_app::cierre::Allocation,
    confirmed: bool,
) -> CmdResult<i64> {
    Ok(taxpain_app::cierre::post_allocation(
        &*state.vault()?,
        year,
        date,
        &allocation,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

#[tauri::command]
pub fn closing_allocation_undo(
    state: tauri::State<'_, AppState>,
    year: i32,
    confirmed: bool,
) -> CmdResult<()> {
    Ok(taxpain_app::cierre::undo_allocation(
        &*state.vault()?,
        year,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Guarda el borrador de la memoria de `year` en RTF (para editarla en Pages o Word) o en PDF,
/// con el modelo `model` (si no se indica, el elegido en los ajustes).
#[tauri::command]
pub fn memoria_save(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    year: i32,
    path: String,
    pdf: bool,
    model: Option<taxpain_core::accounting::statements::ModelKind>,
) -> CmdResult<()> {
    let path = salida::salida_permitida(&app, &path, &[if pdf { "pdf" } else { "rtf" }])?;
    let v = state.vault()?;
    let kind = match model {
        Some(k) => k,
        None => taxpain_app::models::settings(&v)?.accounts_model,
    };
    if pdf {
        salida::escribir(&path, taxpain_app::memoria::pdf_modelo(&v, year, kind)?)?;
    } else {
        salida::escribir(&path, taxpain_app::memoria::rtf_modelo(&v, year, kind)?)?;
    }
    Ok(())
}

/// Textos de la memoria que no salen de la contabilidad (hechos posteriores, auditoría, firmas…).
#[tauri::command]
pub fn memoria_textos(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<taxpain_app::memoria::TextosMemoria> {
    Ok(taxpain_app::memoria::textos(&*state.vault()?, year)?)
}

#[tauri::command]
pub fn memoria_textos_guardar(
    state: tauri::State<'_, AppState>,
    year: i32,
    textos: taxpain_app::memoria::TextosMemoria,
) -> CmdResult<()> {
    Ok(taxpain_app::memoria::guardar_textos(
        &*state.vault()?,
        year,
        &textos,
        OffsetDateTime::now_utc(),
    )?)
}

/// Qué modelo de cuentas toca por tamaño (arts. 257 y 258 LSC, RD 1515/2007).
#[tauri::command]
pub fn cuentas_modelo_aviso(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<taxpain_app::modelo_cuentas::Aviso> {
    Ok(taxpain_app::modelo_cuentas::aviso(&*state.vault()?, year)?)
}

/// Guarda el modelo de cuentas de la sociedad (memoria, ECPN, 200 y depósito).
#[tauri::command]
pub fn cuentas_modelo_elegir(
    state: tauri::State<'_, AppState>,
    model: taxpain_core::accounting::statements::ModelKind,
) -> CmdResult<()> {
    Ok(taxpain_app::modelo_cuentas::elegir(
        &*state.vault()?,
        model,
        OffsetDateTime::now_utc(),
    )?)
}

/// Estado de cambios en el patrimonio neto de `year`.
#[tauri::command]
pub fn ecpn(
    state: tauri::State<'_, AppState>,
    year: i32,
    model: Option<taxpain_core::accounting::statements::ModelKind>,
) -> CmdResult<taxpain_app::ecpn::Ecpn> {
    Ok(taxpain_app::ecpn::ecpn(
        &*state.vault()?,
        year,
        model.unwrap_or_default(),
    )?)
}

/// Guarda el ECPN en PDF o RTF.
#[tauri::command]
pub fn ecpn_save(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    year: i32,
    path: String,
    pdf: bool,
    model: Option<taxpain_core::accounting::statements::ModelKind>,
) -> CmdResult<()> {
    let path = salida::salida_permitida(&app, &path, &[if pdf { "pdf" } else { "rtf" }])?;
    let v = state.vault()?;
    let kind = model.unwrap_or_default();
    if pdf {
        salida::escribir(&path, taxpain_app::ecpn::pdf(&v, year, kind)?)?;
    } else {
        salida::escribir(&path, taxpain_app::ecpn::rtf(&v, year, kind)?)?;
    }
    Ok(())
}
