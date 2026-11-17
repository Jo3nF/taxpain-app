//! Comandos de modelos tributarios (la lógica está en `taxpain-app::models`). Nada se presenta:
//! se calcula el borrador y, si el usuario lo pide, se guarda el fichero para la Sede.

use serde::Serialize;
use taxpain_aeat::AeatClient;
use taxpain_aeat::declarations::{self, Validation};
use taxpain_app::models::{self, Draft, ModelSettings};
use taxpain_app::{notices, rectificativa};
use time::OffsetDateTime;

use crate::state::AppState;
use crate::{CmdResult, CommandError};

#[tauri::command]
pub fn model_settings(state: tauri::State<'_, AppState>) -> CmdResult<ModelSettings> {
    Ok(models::settings(&*state.vault()?)?)
}

#[tauri::command]
pub fn save_model_settings(
    state: tauri::State<'_, AppState>,
    settings: ModelSettings,
) -> CmdResult<()> {
    Ok(models::save_settings(
        &*state.vault()?,
        &settings,
        OffsetDateTime::now_utc(),
    )?)
}

/// ¿Se presenta el modelo cada mes según el perfil fiscal? (`None`: sin perfil.) La pantalla de
/// Modelos ofrece entonces los meses en «Periodo».
#[tauri::command]
pub fn model_monthly(state: tauri::State<'_, AppState>, model: String) -> CmdResult<Option<bool>> {
    Ok(models::monthly_model(&*state.vault()?, &model)?)
}

/// Si el SII es obligatorio para la empresa, y por qué. No envía nada.
#[tauri::command]
pub fn sii_estado(state: tauri::State<'_, AppState>) -> CmdResult<taxpain_app::sii::Estado> {
    Ok(taxpain_app::sii::estado(
        &*state.vault()?,
        crate::calendar::today(),
    )?)
}

/// Plazo de envío al SII de las facturas expedidas o registradas entre `desde` y `hasta`
/// (`aaaa-mm-dd`). No envía nada.
#[tauri::command]
pub fn sii_plazos(
    state: tauri::State<'_, AppState>,
    desde: String,
    hasta: String,
) -> CmdResult<Vec<taxpain_app::sii::Plazo>> {
    let fecha = |s: &str| {
        time::Date::parse(s, time::macros::format_description!("[year]-[month]-[day]"))
            .map_err(|_| CommandError::from(format!("fecha no válida: {s}")))
    };
    Ok(taxpain_app::sii::plazos(
        &*state.vault()?,
        fecha(&desde)?,
        fecha(&hasta)?,
        crate::calendar::today(),
    )?)
}

#[tauri::command]
pub fn model_draft(
    state: tauri::State<'_, AppState>,
    model: String,
    year: i32,
    quarter: u8,
) -> CmdResult<Draft> {
    Ok(models::draft(&*state.vault()?, &model, year, quarter)?)
}

/// Guarda el fichero del borrador en `dir` y devuelve la ruta.
#[tauri::command]
pub fn model_export(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    model: String,
    year: i32,
    quarter: u8,
    dir: String,
) -> CmdResult<String> {
    let vault = state.vault()?;
    let draft = models::draft(&vault, &model, year, quarter)?;
    let bytes = models::export(&vault, &draft, OffsetDateTime::now_utc())?;
    let path = crate::salida::en_carpeta(&app, &dir, &draft.file_name, &["txt"])?;
    crate::salida::escribir(&path, bytes)?;
    Ok(path.to_string_lossy().into_owned())
}

/// Lo presentado del periodo (para ofrecer «Preparar rectificativa…»), o `None` si no consta.
#[tauri::command]
pub fn rectificativa_anterior(
    state: tauri::State<'_, AppState>,
    model: String,
    year: i32,
    quarter: u8,
) -> CmdResult<Option<rectificativa::Anterior>> {
    if !rectificativa::MODELOS.contains(&model.as_str()) {
        return Ok(None);
    }
    Ok(rectificativa::anterior(
        &*state.vault()?,
        &model,
        year,
        quarter,
    )?)
}

/// Prepara la rectificativa (o complementaria) del periodo presentado: lo presentado al lado de
/// lo que sale hoy, la diferencia, el recargo y el fichero. No guarda ni presenta nada.
#[tauri::command]
pub fn rectificativa_preparar(
    state: tauri::State<'_, AppState>,
    model: String,
    year: i32,
    quarter: u8,
    opciones: rectificativa::Opciones,
) -> CmdResult<rectificativa::Rectificativa> {
    let hoy = taxpain_core::spain_time::peninsular_date(OffsetDateTime::now_utc());
    Ok(rectificativa::preparar(
        &*state.vault()?,
        &model,
        year,
        quarter,
        &opciones,
        hoy,
    )?)
}

/// Guarda el fichero de la rectificativa en `dir` (queda en la auditoría) y devuelve la ruta.
#[tauri::command]
pub fn rectificativa_exportar(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    model: String,
    year: i32,
    quarter: u8,
    opciones: rectificativa::Opciones,
    dir: String,
) -> CmdResult<String> {
    let vault = state.vault()?;
    let now = OffsetDateTime::now_utc();
    let hoy = taxpain_core::spain_time::peninsular_date(now);
    let r = rectificativa::preparar(&vault, &model, year, quarter, &opciones, hoy)?;
    let bytes = rectificativa::exportar(&vault, &r, now)?;
    let path = crate::salida::en_carpeta(&app, &dir, &r.file_name, &["txt"])?;
    crate::salida::escribir(&path, bytes)?;
    Ok(path.to_string_lossy().into_owned())
}

#[derive(Serialize)]
pub struct PrevalidationResult {
    valid: bool,
    messages: Vec<String>,
    /// Ruta del borrador oficial en PDF, si la AEAT lo devolvió y se pidió guardarlo.
    pdf_path: Option<String>,
}

/// Prevalida el fichero con el servicio ServValiDos de la AEAT (entorno de validación, sin
/// efectos: no presenta nada). Envía los datos de la declaración, así que exige confirmación y
/// usa el certificado elegido (Touch ID). El borrador oficial en PDF se guarda en `pdf_dir`.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // Los argumentos del comando, tal como llegan de la interfaz.
pub async fn model_prevalidate(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    model: String,
    year: i32,
    quarter: u8,
    fingerprint: String,
    pdf_dir: Option<String>,
    confirmed: bool,
) -> CmdResult<PrevalidationResult> {
    if !confirmed {
        return Err(CommandError::from(
            "hace falta confirmar el envío a la AEAT",
        ));
    }
    if !declarations::VALIDATABLE.contains(&model.as_str()) {
        return Err(CommandError::from(
            "la AEAT no prevalida este modelo con ServValiDos",
        ));
    }
    // La empresa del borrador queda fija: si se cambia a mitad, no se apunta en otra.
    let (company, draft, request) = {
        let (company, vault) = state.vault_with_id()?;
        let draft = models::draft(&vault, &model, year, quarter)?;
        // Sin fichero (p. ej. un 202 de más de 6 millones) no hay nada que prevalidar: se para
        // antes de pedir el certificado.
        if draft.file.is_empty() {
            return Err(CommandError::from("este modelo aún no genera fichero"));
        }
        let request = vault.prepare_identity(
            &fingerprint,
            "Prevalidar una declaración con la AEAT (sin presentarla)",
            OffsetDateTime::now_utc(),
        )?;
        (company, draft, request)
    };
    let identity = state.identity_of(&company, request)?;
    let client = AeatClient::with_platform_trust(&identity, taxpain_aeat::hosts::VALIDACION)?;
    let period = models::period_code(&model, quarter);
    let outcome = declarations::prevalidate(
        &client,
        declarations::SERV_VALIDOS,
        &model,
        year,
        &period,
        &draft.file,
    )
    .await?;
    let (valid, messages, pdf) = match outcome {
        Validation::Valid { warnings, pdf } => (true, warnings, pdf),
        Validation::Invalid { errors } => (false, errors, None),
    };
    models::record_prevalidation(
        &*state.vault_of(&company)?,
        &draft,
        valid,
        messages.len(),
        OffsetDateTime::now_utc(),
    )?;
    let pdf_path = match (pdf, pdf_dir) {
        (Some(bytes), Some(dir)) => {
            let name = format!(
                "{}_borrador_AEAT.pdf",
                draft.file_name.trim_end_matches(".txt")
            );
            let path = crate::salida::en_carpeta(&app, &dir, &name, &["pdf"])?;
            crate::salida::escribir(&path, bytes)?;
            Some(path.to_string_lossy().into_owned())
        }
        _ => None,
    };
    Ok(PrevalidationResult {
        valid,
        messages,
        pdf_path,
    })
}

#[derive(Serialize)]
pub struct FilingResult {
    accepted: bool,
    /// Errores de la AEAT si la rechaza.
    errors: Vec<String>,
    receipt: Option<models::FiledReceipt>,
}

/// Presenta la declaración en el **entorno de pruebas** de la AEAT (`prewww1.aeat.es`).
/// Producción no se ofrece todavía (y `Destination::production` la bloquea en desarrollo).
/// Sólo tras la conformidad expresa del usuario con el borrador mostrado: el envío supone la
/// aceptación de los datos (especificación de presentación directa v30, §1.4).
// Los argumentos son los del comando que llama la interfaz.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn model_file_testing(
    state: tauri::State<'_, AppState>,
    model: String,
    year: i32,
    quarter: u8,
    fingerprint: String,
    nrc: Option<String>,
    confirmed: bool,
    reviewed_file: String,
    retry_after_unknown: Option<bool>,
) -> CmdResult<FilingResult> {
    use models::FilingEvent;
    use taxpain_aeat::declarations::{Destination, Filing, FilingOutcome, file_return};
    if !confirmed {
        return Err(CommandError::from(
            "hace falta tu conformidad con los datos para presentar",
        ));
    }
    if !declarations::PRESENTABLE.contains(&model.as_str()) {
        return Err(CommandError::from(
            "este modelo no se puede presentar por envío directo",
        ));
    }
    let (company, draft, request, presenter_nif, presenter_name) = {
        let (company, vault) = state.vault_with_id()?;
        let cert = vault
            .certificate(&fingerprint)?
            .ok_or_else(|| CommandError::from("certificado no encontrado"))?;
        let represented = cert.entity_nif.clone().unwrap_or(cert.holder_nif.clone());
        models::check_presenter(&vault, &represented)?;
        let draft = models::draft(&vault, &model, year, quarter)?;
        if draft.file.is_empty() {
            return Err(CommandError::from("este modelo aún no genera fichero"));
        }
        // Se presenta exactamente lo revisado, nunca un borrador recalculado distinto.
        models::ensure_reviewed(&draft, &reviewed_file)?;
        let now = OffsetDateTime::now_utc();
        if models::filing_in_doubt(&vault, &draft, false)? {
            if retry_after_unknown != Some(true) {
                return Err(CommandError::from(
                    "Una presentación anterior de este modelo y periodo no tuvo respuesta. \
                     Consulta en la AEAT si consta antes de volver a presentarla.",
                ));
            }
            models::record_filing_event(
                &vault,
                &draft,
                false,
                FilingEvent::RetryAfterUnknown,
                "",
                now,
            )?;
        }
        let request = vault.prepare_identity(
            &fingerprint,
            "Presentar una declaración en el entorno de PRUEBAS de la AEAT",
            OffsetDateTime::now_utc(),
        )?;
        let name = cert
            .holder_name
            .clone()
            .unwrap_or_else(|| cert.subject_dn.clone());
        (company, draft, request, cert.holder_nif, name)
    };
    let identity = state.identity_of(&company, request)?;
    let client = AeatClient::with_platform_trust(&identity, taxpain_aeat::hosts::PRUEBAS)?;
    let period = models::period_code(&model, quarter);
    let filing = Filing {
        model: &model,
        year,
        period: &period,
        nrc: nrc.as_deref().filter(|n| !n.trim().is_empty()),
        file: &draft.file,
        presenter_nif: &presenter_nif,
        presenter_name: &presenter_name,
    };
    let dest = Destination::testing();
    // Constancia antes de enviar: si la app se cierra a mitad, queda «en duda».
    models::record_filing_event(
        &*state.vault_of(&company)?,
        &draft,
        dest.is_production(),
        FilingEvent::Request,
        "",
        OffsetDateTime::now_utc(),
    )?;
    let outcome = match file_return(&client, &dest, &filing).await {
        Ok(o) => o,
        // La conexión no llegó a abrirse: nada salió del Mac, no hay nada que consultar.
        Err(taxpain_aeat::AeatError::Unreachable(e)) => {
            models::record_filing_event(
                &*state.vault_of(&company)?,
                &draft,
                dest.is_production(),
                FilingEvent::NotSent,
                &e,
                OffsetDateTime::now_utc(),
            )?;
            return Err(CommandError::from(format!(
                "No se pudo conectar con la AEAT ({e}): no se ha enviado nada. Comprueba la \
                 conexión y vuelve a intentarlo."
            )));
        }
        Err(e) => {
            models::record_filing_event(
                &*state.vault_of(&company)?,
                &draft,
                dest.is_production(),
                FilingEvent::Unknown,
                &e.to_string(),
                OffsetDateTime::now_utc(),
            )?;
            return Err(CommandError::from(format!(
                "No se sabe si la AEAT recibió la declaración ({e}). Usa «Consultar en la AEAT» \
                 antes de volver a presentarla."
            )));
        }
    };
    match outcome {
        FilingOutcome::Rejected { errors } => {
            models::record_filing_event(
                &*state.vault_of(&company)?,
                &draft,
                dest.is_production(),
                FilingEvent::Rejected,
                &errors.join(" | "),
                OffsetDateTime::now_utc(),
            )?;
            Ok(FilingResult {
                accepted: false,
                errors,
                receipt: None,
            })
        }
        FilingOutcome::Accepted(r) => {
            let receipt = models::FiledReceipt {
                csv: r.csv,
                receipt_number: r.receipt_number,
                date: r.date,
                time: r.time,
                pdf_url: r.pdf_url,
                notices: r.notices,
                warnings: r.warnings,
            };
            let id = models::record_filing(
                &*state.vault_of(&company)?,
                &draft,
                dest.is_production(),
                &receipt,
                OffsetDateTime::now_utc(),
            )?;
            // El justificante legal es el PDF: se intenta descargar ya (en pruebas puede no
            // existir, §1.4). Si falla, la presentación queda grabada y se reintenta luego.
            let mut errors = vec![];
            if let Err(e) = download_receipt(
                &state,
                &company,
                &fingerprint,
                id,
                &receipt.pdf_url,
                dest.is_production(),
            )
            .await
            {
                errors.push(format!(
                    "No se pudo descargar el justificante: {}",
                    e.message
                ));
            }
            Ok(FilingResult {
                accepted: true,
                errors,
                receipt: Some(receipt),
            })
        }
    }
}

#[tauri::command]
pub fn filed_returns(
    state: tauri::State<'_, AppState>,
    year: Option<i32>,
) -> CmdResult<Vec<models::FiledReturn>> {
    Ok(models::filed_returns(&*state.vault()?, year)?)
}

/// Una declaración presentada, para la interfaz: sin el CSV, que da acceso al documento en la
/// Sede y sólo se guarda en el almacén cifrado.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PresentedView {
    pub year: String,
    pub model: String,
    pub period: String,
    pub receipt_number: String,
    pub previous_receipt: Option<String>,
    pub presented_at: String,
}

impl From<declarations::Presented> for PresentedView {
    fn from(p: declarations::Presented) -> Self {
        PresentedView {
            year: p.year,
            model: p.model,
            period: p.period,
            receipt_number: p.receipt_number,
            previous_receipt: p.previous_receipt.filter(|s| !s.trim().is_empty()),
            presented_at: p.presented_at,
        }
    }
}

/// Consulta en la AEAT (producción, sólo lectura) las declaraciones presentadas de un modelo y
/// ejercicio con el certificado elegido.
#[tauri::command]
pub async fn aeat_presented(
    state: tauri::State<'_, AppState>,
    model: String,
    year: i32,
    fingerprint: String,
) -> CmdResult<Vec<PresentedView>> {
    let (company, request, nif, name) = {
        let (company, vault) = state.vault_with_id()?;
        let cert = vault
            .certificate(&fingerprint)?
            .ok_or_else(|| CommandError::from("certificado no encontrado"))?;
        let nif = cert.entity_nif.clone().unwrap_or(cert.holder_nif.clone());
        let name = cert
            .entity_name
            .clone()
            .or(cert.holder_name.clone())
            .unwrap_or_default();
        let request = vault.prepare_identity(
            &fingerprint,
            "Consultar tus declaraciones presentadas en la AEAT",
            OffsetDateTime::now_utc(),
        )?;
        (company, request, nif, name)
    };
    let identity = state.identity_of(&company, request)?;
    let client = AeatClient::with_platform_trust(&identity, taxpain_aeat::hosts::PRODUCCION)?;
    match declarations::query_presented(
        &client,
        declarations::CONSULTA_PRODUCCION,
        &nif,
        &name,
        &model,
        year,
    )
    .await?
    {
        Ok(list) => Ok(list.into_iter().map(PresentedView::from).collect()),
        // Sin declaraciones de ese modelo y año no es un error.
        Err(msg) if declarations::no_declarations(&msg) => Ok(Vec::new()),
        Err(msg) => Err(CommandError::from(format!("AEAT: {msg}"))),
    }
}

/// Descarga el PDF justificante con el certificado `fingerprint` y lo guarda en el almacén.
/// Descarga el PDF justificante con el certificado `fingerprint` y lo guarda en el almacén de
/// la empresa `company` (la de la presentación, aunque se cambie de empresa a mitad). El
/// certificado sólo se presenta a los hosts del entorno de esa presentación (`production`).
async fn download_receipt(
    state: &tauri::State<'_, AppState>,
    company: &str,
    fingerprint: &str,
    id: i64,
    url: &str,
    production: bool,
) -> CmdResult<()> {
    if url.is_empty() {
        return Err(CommandError::from("la AEAT no dio URL del justificante"));
    }
    let request = state.vault_of(company)?.prepare_identity(
        fingerprint,
        "Descargar el justificante de una declaración",
        OffsetDateTime::now_utc(),
    )?;
    let identity = state.identity_of(company, request)?;
    let client =
        AeatClient::with_platform_trust(&identity, taxpain_aeat::hosts::justificantes(production))?;
    let pdf = client.get_bytes(url).await?;
    models::store_receipt_pdf(
        &*state.vault_of(company)?,
        id,
        &pdf,
        OffsetDateTime::now_utc(),
    )?;
    Ok(())
}

/// Reintenta la descarga del justificante en PDF de una presentación.
#[tauri::command]
pub async fn filed_receipt_download(
    state: tauri::State<'_, AppState>,
    id: i64,
    fingerprint: String,
) -> CmdResult<()> {
    let (company, url, production) = {
        let (company, v) = state.vault_with_id()?;
        let (url, production) = models::filed_returns(&v, None)?
            .into_iter()
            .find(|r| r.id == id)
            .map(|r| (r.pdf_url, r.environment == "produccion"))
            .ok_or_else(|| CommandError::from("presentación no encontrada"))?;
        (company, url, production)
    };
    download_receipt(&state, &company, &fingerprint, id, &url, production).await
}

/// Guarda en `dir` el justificante en PDF ya descargado y devuelve la ruta.
#[tauri::command]
pub fn filed_receipt_save(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: i64,
    dir: String,
) -> CmdResult<String> {
    let vault = state.vault()?;
    let pdf = models::receipt_pdf(&vault, id)?
        .ok_or_else(|| CommandError::from("todavía no hay justificante descargado"))?;
    let r = models::filed_returns(&vault, None)?
        .into_iter()
        .find(|r| r.id == id)
        .ok_or_else(|| CommandError::from("presentación no encontrada"))?;
    let name = format!(
        "{}_{}_{}_{}.pdf",
        notices::safe_file_component(&r.model),
        r.year,
        notices::safe_file_component(&r.period),
        notices::safe_file_component(&r.csv)
    );
    let path = crate::salida::en_carpeta(&app, &dir, &name, &["pdf"])?;
    crate::salida::escribir(&path, pdf)?;
    Ok(path.to_string_lossy().into_owned())
}

// ---------- Datos fiscales de Renta ----------

/// Descarga los datos fiscales de Renta del ejercicio con el certificado del propio
/// contribuyente (sólo lectura) y los guarda cifrados para el cotejo.
#[tauri::command]
pub async fn renta_download(
    state: tauri::State<'_, AppState>,
    fingerprint: String,
    year: i32,
) -> CmdResult<models::CrossCheck> {
    use taxpain_aeat::renta;
    let (company, request, nif) = {
        let (company, vault) = state.vault_with_id()?;
        let cert = vault
            .certificate(&fingerprint)?
            .ok_or_else(|| CommandError::from("certificado no encontrado"))?;
        if cert.entity_nif.is_some() {
            return Err(CommandError::from(
                "los datos fiscales de Renta son de personas físicas: usa tu certificado personal",
            ));
        }
        models::check_presenter(&vault, &cert.holder_nif)?;
        let request = vault.prepare_identity(
            &fingerprint,
            "Descargar tus datos fiscales de Renta de la AEAT (sólo lectura)",
            OffsetDateTime::now_utc(),
        )?;
        (company, request, cert.holder_nif)
    };
    let identity = state.identity_of(&company, request)?;
    let client = AeatClient::with_platform_trust(&identity, taxpain_aeat::hosts::PRODUCCION)?;
    if !renta::is_ratified(&client, year, &nif)
        .await?
        .map_err(|e| CommandError::from(e.to_string()))?
    {
        return Err(CommandError::from(
            renta::TaxDataError::NotRatified.to_string(),
        ));
    }
    let (bytes, data) = renta::download(&client, year, &nif)
        .await?
        .map_err(|e| CommandError::from(e.to_string()))?;
    let vault = state.vault_of(&company)?;
    models::store_tax_data(&vault, &bytes, &data, OffsetDateTime::now_utc())?;
    models::cross_check(&vault, year)?
        .ok_or_else(|| CommandError::from("no se pudieron guardar los datos fiscales"))
}

/// Cotejo con los datos fiscales ya descargados (`None` si aún no se han descargado).
#[tauri::command]
pub fn renta_cross_check(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<Option<models::CrossCheck>> {
    Ok(models::cross_check(&*state.vault()?, year)?)
}

/// Renta del autónomo: rendimiento de la actividad del ejercicio y sus casillas (borrador).
#[tauri::command]
pub fn renta_activity(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<taxpain_app::renta::ActivityDraft> {
    Ok(taxpain_app::renta::activity(&*state.vault()?, year)?)
}

/// Pago registrado de un modelo y los cargos del banco sin conciliar por su importe exacto.
#[derive(Serialize)]
pub struct TaxPaymentView {
    pub payable: bool,
    pub payment: Option<taxpain_app::tax_payments::TaxPayment>,
    pub candidates: Vec<taxpain_app::bank::BankTx>,
}

#[tauri::command]
pub fn tax_payment(
    state: tauri::State<'_, AppState>,
    model: String,
    year: i32,
    quarter: u8,
) -> CmdResult<TaxPaymentView> {
    use taxpain_app::tax_payments;
    let v = state.vault()?;
    let payable = tax_payments::payable_account(&model).is_some();
    let payment = tax_payments::payment(&v, &model, year, quarter)?;
    let candidates = if payable && payment.is_none() {
        tax_payments::bank_candidates(&v, &model, year, quarter).unwrap_or_default()
    } else {
        Vec::new()
    };
    Ok(TaxPaymentView {
        payable,
        payment,
        candidates,
    })
}

/// Registra el pago del modelo (efectivo, otra cuenta o el cargo de una domiciliación).
#[tauri::command]
pub fn tax_pay(
    state: tauri::State<'_, AppState>,
    model: String,
    year: i32,
    quarter: u8,
    date: time::Date,
    treasury: String,
) -> CmdResult<taxpain_app::tax_payments::TaxPayment> {
    Ok(taxpain_app::tax_payments::pay(
        &*state.vault()?,
        &model,
        year,
        quarter,
        date,
        &treasury,
        OffsetDateTime::now_utc(),
    )?)
}

/// Registra el pago con un cargo del banco ya importado (queda conciliado).
#[tauri::command]
pub fn tax_pay_bank(
    state: tauri::State<'_, AppState>,
    model: String,
    year: i32,
    quarter: u8,
    tx_id: i64,
) -> CmdResult<taxpain_app::tax_payments::TaxPayment> {
    Ok(taxpain_app::tax_payments::pay_from_bank(
        &*state.vault()?,
        &model,
        year,
        quarter,
        tx_id,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn tax_unpay(
    state: tauri::State<'_, AppState>,
    model: String,
    year: i32,
    quarter: u8,
) -> CmdResult<()> {
    Ok(taxpain_app::tax_payments::undo(
        &*state.vault()?,
        &model,
        year,
        quarter,
        OffsetDateTime::now_utc(),
    )?)
}

/// Casillas fijadas a mano de un modelo y periodo.
#[tauri::command]
pub fn model_adjustments(
    state: tauri::State<'_, AppState>,
    model: String,
    year: i32,
    quarter: u8,
) -> CmdResult<Vec<(u16, taxpain_core::money::Money)>> {
    Ok(
        models::adjustments(&*state.vault()?, &model, year, quarter)?
            .into_iter()
            .collect(),
    )
}

/// Fija una casilla a mano (`amount`) o vuelve al cálculo automático (`None`).
#[tauri::command]
pub fn model_adjust(
    state: tauri::State<'_, AppState>,
    model: String,
    year: i32,
    quarter: u8,
    box_number: u16,
    amount: Option<taxpain_core::money::Money>,
) -> CmdResult<()> {
    Ok(models::set_adjustment(
        &*state.vault()?,
        &model,
        year,
        quarter,
        box_number,
        amount,
        OffsetDateTime::now_utc(),
    )?)
}

/// 202: comparación de las modalidades de la cuota y de la base (art. 40.2 y 40.3 LIS) en el
/// ejercicio `year`, con la elegida y el plazo de la opción en el censo.
#[tauri::command]
pub fn model_202_simulation(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<models::pago202::Simulacion202> {
    Ok(models::pago202::simulate(
        &*state.vault()?,
        year,
        crate::calendar::today(),
    )?)
}

/// 202: guarda la modalidad elegida para el ejercicio (`None`: sin elegir, la de la cuota).
#[tauri::command]
pub fn model_202_set_modality(
    state: tauri::State<'_, AppState>,
    year: i32,
    modality: Option<models::pago202::Modalidad>,
) -> CmdResult<()> {
    Ok(models::pago202::set_modalidad(
        &*state.vault()?,
        year,
        modality,
        OffsetDateTime::now_utc(),
    )?)
}

/// Cartas del 347 (una por tercero) en un PDF guardado en `dir`; devuelve la ruta.
#[tauri::command]
pub fn model_letters_347(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    year: i32,
    place_date: String,
    dir: String,
) -> CmdResult<String> {
    let (name, pdf) = models::letters_347(&*state.vault()?, year, &place_date)?;
    let path = crate::salida::en_carpeta(&app, &dir, &name, &["pdf"])?;
    crate::salida::escribir(&path, pdf)?;
    Ok(path.to_string_lossy().into_owned())
}

// ---------- Certificados de retenciones para los perceptores (190, 180 y 193) ----------

use taxpain_app::certificados::{self, Opciones, Preparacion};

/// Perceptores del 190, 180 o 193 del año y lo propuesto para firmar los certificados.
#[tauri::command]
pub fn model_certificates_prepare(
    state: tauri::State<'_, AppState>,
    model: String,
    year: i32,
) -> CmdResult<Preparacion> {
    Ok(certificados::prepare(&*state.vault()?, &model, year)?)
}

/// Guarda en `dir` los certificados elegidos: uno por perceptor (`each`) y/o uno con todos
/// (`combined`). Devuelve las rutas y lo deja en la auditoría.
#[allow(clippy::too_many_arguments)] // Los argumentos del comando, tal como llegan de la interfaz.
#[tauri::command]
pub fn model_certificates_save(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    model: String,
    year: i32,
    options: Opciones,
    dir: String,
    each: bool,
    combined: bool,
) -> CmdResult<Vec<String>> {
    if !each && !combined {
        return Err(CommandError::from(
            "elige un PDF por perceptor, uno con todos o los dos",
        ));
    }
    let vault = state.vault()?;
    let mut files = Vec::new();
    if each {
        files.extend(certificados::pdf_each(&vault, &model, year, &options)?);
    }
    if combined {
        files.push(certificados::pdf_all(&vault, &model, year, &options)?);
    }
    // Se comprueban todas las rutas antes de escribir nada.
    let mut out = Vec::new();
    for (name, pdf) in files {
        out.push((crate::salida::en_carpeta(&app, &dir, &name, &["pdf"])?, pdf));
    }
    for (path, pdf) in &out {
        crate::salida::escribir(path, pdf)?;
    }
    let nifs: Vec<String> = certificados::certificates(&vault, &model, year, &options)?
        .into_iter()
        .map(|c| c.nif)
        .collect();
    certificados::record_issued(
        &vault,
        &model,
        year,
        &nifs,
        "carpeta",
        OffsetDateTime::now_utc(),
    )?;
    Ok(out
        .into_iter()
        .map(|(p, _)| p.to_string_lossy().into_owned())
        .collect())
}

/// Prepara el `.eml` con el certificado de un perceptor y lo abre en el programa de correo
/// (Apple Mail lo abre como mensaje nuevo sin enviar). TaxPain no envía nada.
#[tauri::command]
pub fn model_certificate_email(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    model: String,
    year: i32,
    options: Opciones,
    nif: String,
    to: Option<String>,
) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let vault = state.vault()?;
    let (name, bytes) = certificados::email(&vault, &model, year, &options, &nif, to.as_deref())?;
    let path = crate::invoicing::private_file("taxpain-correo", &name)?;
    std::fs::write(&path, bytes)?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)?;
    certificados::record_issued(
        &vault,
        &model,
        year,
        &[nif],
        "correo",
        OffsetDateTime::now_utc(),
    )?;
    Ok(())
}

// ---------- 184: socios y datos de la entidad en atribución de rentas ----------

use taxpain_app::atribucion::{self, Partner, Settings184};

#[tauri::command]
pub fn partners_list(state: tauri::State<'_, AppState>) -> CmdResult<Vec<Partner>> {
    Ok(atribucion::partners(&*state.vault()?)?)
}

#[tauri::command]
pub fn partner_save(state: tauri::State<'_, AppState>, partner: Partner) -> CmdResult<i64> {
    Ok(atribucion::save_partner(
        &*state.vault()?,
        &partner,
        OffsetDateTime::now_utc(),
    )?)
}

/// Borra un partícipe del 184: Touch ID con su nombre y NIF.
#[tauri::command(async)]
pub fn partner_delete(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    crate::confirmar::borrar_participe(&state, id)
}

#[tauri::command]
pub fn settings_184(state: tauri::State<'_, AppState>) -> CmdResult<Settings184> {
    Ok(atribucion::settings(&*state.vault()?)?)
}

#[tauri::command]
pub fn save_settings_184(
    state: tauri::State<'_, AppState>,
    settings: Settings184,
) -> CmdResult<()> {
    Ok(atribucion::save_settings(
        &*state.vault()?,
        &settings,
        OffsetDateTime::now_utc(),
    )?)
}
