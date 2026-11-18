//! App de escritorio TaxPain. Los comandos Tauri sólo devuelven resúmenes: nunca claves,
//! contraseñas ni el certificado (ver `docs/ARCHITECTURE.md` §4).
//!
//! `TODO(VERIFY)`: este crate no se puede compilar en el entorno Linux de desarrollo (sin
//! webkit2gtk); se compila en macOS (`./scripts/ci.sh`).

mod accounting;
mod ahorro;
mod ajustes_cierre;
mod apertura;
mod archivo;
mod assistant;
mod background;
mod backup;
mod banca;
mod calendar;
mod carpeta;
mod censo;
mod companies;
mod confirmar;
mod contacts;
mod cripto;
mod dashboard;
mod divisa;
mod expedientes;
mod extractos;
mod extranjero;
mod forecast;
mod gestor;
mod incobrables;
mod inmovilizado;
mod invoicing;
mod iva_holding;
mod libros_societarios;
mod mcp;
mod modelo200;
mod models;
mod no_residentes;
mod nominas;
mod notices;
mod oss;
mod patrimonial;
mod presentado;
mod prestamos;
mod quotes;
mod recovery;
mod salida;
mod socios;
mod state;
mod subvenciones;
mod trimestre;
mod updates;
mod valores;
mod ventana;
mod verifactu_lab;
mod vinculadas;

use serde::Serialize;
use state::AppState;
use tauri::Manager;
use taxpain_vault::{CertificateInfo, VaultError};

/// Error serializable hacia el WebView: sólo el mensaje (sin secretos por construcción).
#[derive(Debug, Serialize)]
pub(crate) struct CommandError {
    pub(crate) message: String,
}

impl<E: std::fmt::Display> From<E> for CommandError {
    fn from(e: E) -> Self {
        let message = e.to_string();
        // El prefijo técnico del almacén («dato no válido: …») no dice nada al usuario: el
        // mensaje de detrás ya explica qué pasa.
        let message = match message.strip_prefix("dato no válido: ") {
            Some(rest) => rest.to_owned(),
            None => message,
        };
        CommandError { message }
    }
}

pub(crate) type CmdResult<T> = Result<T, CommandError>;

const MB: u64 = 1024 * 1024;

/// Lee un fichero elegido por el usuario mirando antes su tamaño: un fichero equivocado de
/// varios GB (un vídeo, una imagen de disco) no se carga entero en memoria para rechazarlo
/// después. `max` en bytes.
pub(crate) fn read_limited(path: impl AsRef<std::path::Path>, max: u64) -> CmdResult<Vec<u8>> {
    let path = path.as_ref();
    let len = std::fs::metadata(path)?.len();
    if len > max {
        return Err(CommandError::from(format!(
            "el fichero pasa de {} MB (tiene {} MB): elige otro",
            max.div_ceil(MB),
            len.div_ceil(MB)
        )));
    }
    Ok(std::fs::read(path)?)
}

/// Título y texto del aviso cuando el almacén principal no se puede abrir al arrancar: qué ha
/// pasado y qué hacer, en llano. Sin esto la app muere sin ningún mensaje.
fn startup_error(e: &VaultError) -> (&'static str, String) {
    let title = "TaxPain no puede abrir tus datos";
    let text = match e {
        VaultError::KeyUnavailable(detail) => format!(
            "El Llavero del Mac no ha dado la clave que cifra tus datos ({detail}).\n\nVuelve a \
             abrir TaxPain y, cuando el Mac pregunte si permites el acceso al Llavero, pulsa \
             «Permitir siempre». Tus datos no se han tocado."
        ),
        VaultError::KeyMissing { .. } => "El Llavero de este Mac no tiene la clave que cifra tus \
             datos (pasa al cambiar de Mac, con una cuenta de usuario nueva o si se borró del \
             Llavero), y junto a los datos no hay código de recuperación.\n\nSi tienes una copia \
             de seguridad (.taxpain), abre TaxPain en un Mac que tenga la clave o en una carpeta \
             de datos nueva y restáurala con su contraseña. Los ficheros no se han borrado: pide \
             ayuda antes de hacer nada más."
            .to_owned(),
        VaultError::Corrupted => "El almacén está dañado o la clave del Llavero no es la suya.\n\n\
             Restaura la última copia de seguridad (al abrir TaxPain con una copia a mano) o pide \
             ayuda antes de hacer nada más. Los ficheros no se han borrado."
            .to_owned(),
        other => format!(
            "No se ha podido abrir la base de datos: {other}.\n\nComprueba que el disco tiene \
             espacio y que la carpeta de datos de TaxPain se puede escribir. Si acabas de \
             actualizar la app, vuelve a la versión anterior y pide ayuda."
        ),
    };
    (title, text)
}

#[derive(Serialize)]
struct AppInfo {
    version: &'static str,
    /// `false` en desarrollo sin Keychain: la UI muestra un aviso.
    keys_secure: bool,
    /// La BD estaba por detrás de la última auditoría vista (copia antigua restaurada…).
    audit_rollback: Option<taxpain_vault::audit::Rollback>,
    /// Aviso de auditoría sin reconocer (retroceso, ancla ausente o ilegible), guardado en la BD
    /// hasta que el usuario pulsa «Entendido» (`audit_rollback_acknowledge`).
    audit_warning: Option<taxpain_vault::audit::PendingRollback>,
    /// La empresa abierta es la de pruebas VERI*FACTU.
    verifactu_lab: bool,
    /// Esta compilación emite facturas (`taxpain_core::FACTURACION`). La que se descarga de la
    /// web, sin productor, no.
    facturacion: bool,
}

#[tauri::command]
fn app_info(state: tauri::State<'_, AppState>) -> CmdResult<AppInfo> {
    let v = state.vault()?;
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        keys_secure: v.keys_are_secure(),
        audit_rollback: v.audit_rollback().cloned(),
        audit_warning: v.audit_rollback_pending()?,
        verifactu_lab: taxpain_app::verifactu_lab::info(&v)?.is_some(),
        facturacion: taxpain_core::FACTURACION,
    })
}

/// El usuario ha visto el aviso de auditoría de la empresa abierta («Entendido»): deja de estar
/// pendiente y queda anotado. `false` si no había ninguno.
#[tauri::command]
fn audit_rollback_acknowledge(state: tauri::State<'_, AppState>) -> CmdResult<bool> {
    Ok(state.acknowledge_audit_rollback()?)
}

#[tauri::command]
fn list_certificates(state: tauri::State<'_, AppState>) -> CmdResult<Vec<CertificateInfo>> {
    Ok(state.vault()?.certificates()?)
}

/// Importa el `.p12/.pfx` de `path` (elegido con el diálogo nativo). La contraseña se borra de
/// memoria al terminar y no se guarda.
#[tauri::command]
async fn import_certificate(
    state: tauri::State<'_, AppState>,
    path: String,
    passphrase: String,
) -> CmdResult<CertificateInfo> {
    let passphrase = zeroize::Zeroizing::new(passphrase);
    let bytes = zeroize::Zeroizing::new(read_limited(
        &path,
        taxpain_vault::certificate::MAX_P12_SIZE as u64,
    )?);
    let info =
        state
            .vault()?
            .import_certificate(&bytes, &passphrase, time::OffsetDateTime::now_utc())?;
    Ok(info)
}

/// Borra un certificado (pide Touch ID). La confirmación se pide sin el almacén cogido: con
/// él, la interfaz y los hilos de fondo se quedarían parados hasta dos minutos esperando.
#[tauri::command]
async fn delete_certificate(
    state: tauri::State<'_, AppState>,
    fingerprint: String,
) -> CmdResult<()> {
    let request = state.vault()?.prepare_delete(&fingerprint)?;
    Ok(state.delete_certificate(request)?)
}

/// Lo que se pone en marcha con los datos ya abiertos: al arrancar o tras recuperarlos con el
/// código de recuperación.
pub(crate) fn start(app: &tauri::AppHandle) {
    // Copias descifradas de documentos que se abrieron en la sesión anterior, y lo que
    // se dejó a otras apps (el `.eml` de un correo…) hace más de una hora.
    taxpain_app::documentos::clean_temp_copies(std::time::Duration::ZERO);
    invoicing::purge_private_temp_dirs();
    // Fotos y PDF escaneados de la carpeta de la empresa: texto con Vision, en el Mac.
    #[cfg(target_os = "macos")]
    taxpain_app::carpeta::set_ocr(Box::new(|_name: &str, bytes: &[u8]| {
        let text = if bytes.starts_with(b"%PDF") {
            taxpain_macos::ocr::pdf_text(bytes)
        } else {
            taxpain_macos::ocr::image_text(bytes)
        };
        text.ok()
    }));
    calendar::spawn_reminder_loop(app.clone());
    invoicing::spawn_watch_loop(app.clone());
    backup::spawn_auto_loop(app.clone());
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("info".parse().expect("directiva válida")),
        )
        .init();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(ventana::navegacion())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![background::HIDDEN_ARG]),
        ))
        // En el Mac, cerrar la ventana la esconde: la app sigue en el Dock y los avisos de
        // plazos siguen llegando. ⌘Q la cierra del todo.
        .on_window_event(|window, event| {
            if cfg!(target_os = "macos")
                && let tauri::WindowEvent::CloseRequested { api, .. } = event
            {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            let state = match AppState::open(&dir) {
                Ok(s) => s,
                // Falta la clave en el Llavero pero hay código de recuperación: la interfaz
                // pide la contraseña de las copias (`recovery::vault_recover`).
                Err(VaultError::KeyMissing { .. })
                    if !state::recoverable_companies(&dir).is_empty() =>
                {
                    tracing::warn!("falta la clave en el Llavero: se pide recuperarla");
                    let companies = state::recoverable_companies(&dir);
                    app.manage(recovery::Locked {
                        root: dir,
                        companies,
                    });
                    background::show_unless_started_hidden(app.handle());
                    return Ok(());
                }
                Err(e) => {
                    // Si ninguna empresa abre (Llavero denegado, BD dañada, migración que falla),
                    // devolver el error haría entrar en pánico a Tauri y la app moriría sin
                    // ningún mensaje. La ventana enseña la pantalla de recuperación: qué ha
                    // pasado, abrir una copia y exportar lo que se pueda, sin tocar nada.
                    tracing::error!(error = %e, "no se puede abrir ninguna empresa");
                    let (_, text) = startup_error(&e);
                    app.manage(recovery::StartupFailed { text, root: dir });
                    background::show_unless_started_hidden(app.handle());
                    return Ok(());
                }
            };
            app.manage(state);
            start(app.handle());
            // La ventana arranca escondida (tauri.conf.json): se enseña salvo si la ha abierto
            // el inicio de sesión (`--hidden`), y así no aparece un momento para esconderse.
            background::show_unless_started_hidden(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            audit_rollback_acknowledge,
            assistant::received_ai_fill_pending,
            invoicing::recurring_list,
            invoicing::recurring_create,
            invoicing::recurring_update,
            invoicing::recurring_delete,
            invoicing::recurring_preview,
            invoicing::recurring_issue,
            invoicing::fx_bce_rate,
            invoicing::invoice_draft_of,
            invoicing::invoice_language_of,
            invoicing::invoice_email_compose,
            invoicing::invoice_email_open,
            accounting::memoria_save,
            accounting::memoria_textos,
            accounting::memoria_textos_guardar,
            accounting::cuentas_modelo_aviso,
            accounting::cuentas_modelo_elegir,
            accounting::ecpn,
            accounting::ecpn_save,
            accounting::closing_checklist,
            accounting::closing_corporate_tax,
            accounting::closing_corporate_tax_post,
            accounting::closing_corporate_tax_undo,
            accounting::closing_vat_balances,
            accounting::closing_vat_balances_post,
            accounting::closing_allocation,
            accounting::closing_allocation_post,
            accounting::closing_allocation_undo,
            trimestre::cierre_trimestre,
            trimestre::cierre_trimestre_resumen,
            trimestre::cierre_trimestre_marcar,
            trimestre::puesta_marcha,
            trimestre::puesta_marcha_marcar,
            trimestre::obligaciones_no_fiscales,
            trimestre::obligacion_no_fiscal_marcar,
            ajustes_cierre::ajustes_cierre,
            ajustes_cierre::ajustes_cierre_comprobaciones,
            ajustes_cierre::ajustes_cierre_propuesta,
            ajustes_cierre::ajustes_cierre_crear,
            ajustes_cierre::ajustes_cierre_deshacer,
            ajustes_cierre::ajustes_cierre_revertir,
            ajustes_cierre::ajustes_cierre_deshacer_reversion,
            quotes::quotes_list,
            quotes::quote_create,
            quotes::quote_update,
            quotes::quote_status,
            quotes::quote_delete,
            quotes::quote_open,
            quotes::quote_save,
            quotes::quote_invoice_draft,
            quotes::quote_issue_invoice,
            quotes::quote_email_compose,
            quotes::quote_email_open,
            contacts::contacts_list,
            contacts::contact_detail,
            contacts::contact_save,
            contacts::contact_delete,
            contacts::contacts_search,
            contacts::global_search,
            contacts::vies_check,
            background::autostart_get,
            background::autostart_set,
            recovery::vault_locked,
            recovery::vault_recover,
            recovery::recuperacion_empresas,
            recovery::recuperacion_rescatar,
            recovery::recuperacion_restaurar,
            archivo::auditoria_consultar,
            archivo::auditoria_acciones,
            archivo::auditoria_verificar,
            archivo::integridad_empresa,
            archivo::periodos_listar,
            archivo::periodos_sugeridos,
            archivo::periodo_cerrar,
            archivo::periodo_reabrir,
            archivo::periodo_autorizar,
            archivo::archivo_conservacion,
            archivo::archivo_aviso,
            archivo::archivo_ejercicio_contenido,
            archivo::archivo_ejercicio_exportar,
            archivo::exportacion_completa,
            backup::backup_status,
            backup::backup_set_passphrase,
            backup::backup_verify_passphrase,
            backup::backup_save_settings,
            backup::backup_create,
            backup::backup_inspect,
            backup::backup_restore,
            backup::restore_check,
            backup::restore_acknowledge,
            verifactu_lab::verifactu_lab_status,
            verifactu_lab::verifactu_lab_create,
            verifactu_lab::verifactu_lab_use_cert_nif,
            verifactu_lab::verifactu_lab_generate,
            verifactu_lab::verifactu_lab_cancel,
            verifactu_lab::verifactu_lab_query,
            verifactu_lab::verifactu_lab_report_save,
            carpeta::lectura_ia_get,
            carpeta::lectura_ia_save,
            carpeta::folder_ai_read,
            carpeta::folder_get,
            carpeta::folder_scan,
            carpeta::folder_review,
            carpeta::folder_review_last,
            carpeta::folder_review_save,
            carpeta::folder_apply,
            carpeta::folder_book_imports,
            carpeta::folder_book_undo,
            carpeta::filed_list,
            carpeta::filed_compare,
            carpeta::filed_save_manual,
            carpeta::filed_delete,
            carpeta::company_docs_list,
            carpeta::company_doc_open,
            carpeta::company_doc_delete,
            carpeta::company_suggestions,
            carpeta::company_apply_suggestion,
            carpeta::company_doc_ai,
            companies::companies_list,
            companies::companies_overview,
            companies::consolidated_accounts,
            companies::copy_entries_to,
            companies::company_create,
            companies::company_switch,
            companies::company_update,
            companies::company_update_id,
            companies::company_delete,
            list_certificates,
            import_certificate,
            delete_certificate,
            calendar::get_profile,
            calendar::deduced_profile,
            calendar::profile_presets,
            calendar::ccaa_list,
            calendar::set_profile,
            calendar::fiscal_calendar,
            calendar::vigencia_ejercicio,
            calendar::aviso_cambio_ejercicio,
            calendar::export_ical,
            calendar::threshold_alerts,
            invoicing::invoicing_settings,
            invoicing::save_invoicing_settings,
            invoicing::new_installation_id,
            invoicing::detect_destinations,
            invoicing::invoice_preview,
            invoicing::invoice_issue,
            invoicing::invoices_list,
            invoicing::invoice_open,
            invoicing::invoice_save,
            invoicing::invoice_cancel,
            invoicing::invoice_sign_facturae,
            invoicing::verifactu_send,
            invoicing::verifactu_issues,
            invoicing::verifactu_queue_status,
            invoicing::verifactu_queue_reopen,
            invoicing::declaracion_responsable,
            invoicing::declaracion_responsable_save,
            invoicing::verifactu_resubmit,
            invoicing::received_import,
            invoicing::received_list,
            invoicing::received_review,
            invoicing::received_discard,
            invoicing::received_undiscard,
            invoicing::received_check,
            invoicing::received_correct,
            invoicing::received_unreview_check,
            invoicing::received_unreview,
            invoicing::received_register_typed,
            invoicing::received_open,
            invoicing::watched_folders,
            invoicing::set_watched_folders,
            invoicing::watched_pending,
            invoicing::watched_dismiss,
            accounting::journal,
            accounting::fiscal_year_info,
            accounting::trial_balance,
            accounting::ledger,
            accounting::manual_entry,
            accounting::close_year,
            accounting::reopen_year,
            accounting::fixed_assets,
            accounting::add_fixed_asset,
            apertura::apertura_draft,
            apertura::apertura_save,
            apertura::apertura_discard,
            apertura::apertura_read_balance,
            apertura::apertura_read_pending,
            apertura::apertura_banks,
            apertura::apertura_summary,
            apertura::apertura_apply,
            apertura::apertura_applied,
            apertura::apertura_undo,
            apertura::fixed_asset_add_with_prior,
            apertura::fixed_asset_priors,
            gestor::gestor_preview,
            gestor::gestor_import,
            gestor::gestor_lots,
            gestor::gestor_undo,
            gestor::gestor_compare_balance,
            gestor::gestor_compare_models,
            gestor::apertura_read_inventory,
            gestor::fixed_asset_inventory_preview,
            gestor::fixed_asset_inventory_import,
            cripto::cripto_settings,
            cripto::cripto_save_settings,
            cripto::cripto_assets,
            cripto::cripto_save_asset,
            cripto::cripto_platforms,
            cripto::cripto_save_platform,
            cripto::cripto_delete_platform,
            cripto::cripto_ops,
            cripto::cripto_save_op,
            cripto::cripto_delete_op,
            cripto::cripto_prices,
            cripto::cripto_set_prices,
            cripto::cripto_delete_price,
            cripto::cripto_report,
            cripto::cripto_read_file,
            cripto::cripto_import,
            cripto::cripto_post_entries,
            cripto::cripto_impairment,
            cripto::cripto_export,
            cripto::cripto_inicio,
            cripto::cripto_save_inicio,
            cripto::cripto_delete_inicio,
            cripto::cripto_propuesta_inicio,
            valores::valores_report,
            valores::valores_settings,
            valores::valores_save_settings,
            valores::valores_accounts,
            valores::valores_save_account,
            valores::valores_delete_account,
            valores::valores_securities,
            valores::valores_save_security,
            valores::valores_ops,
            valores::valores_save_op,
            valores::valores_delete_op,
            valores::valores_prices,
            valores::valores_set_prices,
            valores::valores_delete_price,
            valores::valores_read_files,
            valores::valores_import,
            valores::valores_ibkr_connect,
            valores::valores_ibkr_disconnect,
            valores::valores_ibkr_sync,
            valores::valores_export,
            valores::valores_bce,
            valores::valores_post_entries,
            valores::valores_undo_entries,
            valores::valores_closing,
            valores::valores_closing_post,
            valores::valores_closing_undo,
            divisa::bank_set_fx,
            divisa::bank_balances,
            divisa::closing_fx_valuation,
            divisa::closing_fx_valuation_post,
            divisa::closing_fx_valuation_undo,
            accounting::set_depreciation_plan,
            accounting::post_depreciation,
            accounting::annual_accounts,
            accounting::settle_vat,
            accounting::redo_vat,
            accounting::export_books,
            accounting::export_listings,
            accounting::posting_accounts_get,
            accounting::posting_accounts_save,
            accounting::party_counterpart_set,
            accounting::entry_notes_set,
            accounting::entry_documents,
            accounting::entry_attach,
            accounting::entry_detach,
            accounting::entry_document_open,
            accounting::asset_dispose,
            inmovilizado::asset_edit,
            inmovilizado::asset_delete,
            inmovilizado::asset_undispose,
            inmovilizado::depreciation_preview,
            inmovilizado::depreciation_undo,
            inmovilizado::asset_incentives,
            inmovilizado::asset_incentive_usage,
            inmovilizado::asset_set_incentive,
            inmovilizado::depreciation_differences,
            accounting::asset_sales,
            accounting::asset_sale_candidates,
            accounting::asset_sale_link,
            accounting::asset_sale_unlink,
            accounting::cartera_mismatches,
            accounting::payment_period,
            accounting::set_tick,
            accounting::cartera_list,
            incobrables::invoice_numbering,
            incobrables::incobrables_list,
            incobrables::incobrable_open,
            incobrables::incobrable_adjust,
            incobrables::incobrable_claim,
            incobrables::incobrable_preview,
            incobrables::incobrable_issue,
            incobrables::incobrable_communication,
            incobrables::incobrable_communicated,
            incobrables::incobrable_close,
            accounting::payment_terms,
            accounting::set_payment_terms,
            accounting::set_due_date,
            accounting::set_installments,
            accounting::party_terms_list,
            accounting::set_party_terms,
            accounting::sepa_settings,
            accounting::save_sepa_settings,
            accounting::remittance_candidates,
            accounting::remittance_banks,
            accounting::remittance_list,
            accounting::remittance_create,
            accounting::remittance_save,
            accounting::remittance_undo,
            accounting::remittance_return,
            accounting::remittance_undo_return,
            accounting::import_preview,
            accounting::import_entries,
            accounting::import_batches,
            accounting::import_undo,
            accounting::legalization_build,
            accounting::cost_centers,
            accounting::cost_center_save,
            accounting::cost_center_delete,
            accounting::cost_rules,
            accounting::cost_rule_set,
            accounting::analytic_lines,
            accounting::analytic_line_set,
            accounting::analytic_report,
            accounting::cost_redistributions,
            accounting::cost_redistribution_set,
            accounting::budget_list,
            accounting::budget_set,
            accounting::budget_from_actuals,
            accounting::budget_report,
            accounting::financial_ratios,
            accounting::stock_lines,
            accounting::stock_regularize,
            censo::censo_cotejo,
            censo::censo_guardar,
            censo::censo_guia,
            censo::censo_programar,
            censo::censo_presentada,
            censo::censo_quitar,
            extranjero::extranjero_borrador,
            extranjero::extranjero_guardar_bien,
            extranjero::extranjero_guardar_declarado,
            extranjero::extranjero_quitar_declarado,
            extranjero::extranjero_marcar_presentado,
            extranjero::extranjero_exportar,
            no_residentes::irnr_resumen,
            no_residentes::irnr_guardar_certificado,
            no_residentes::irnr_quitar_certificado,
            no_residentes::irnr_proponer,
            no_residentes::irnr_apuntar,
            no_residentes::irnr_quitar_renta,
            no_residentes::inmuebles_list,
            no_residentes::inmueble_guardar,
            no_residentes::inmueble_quitar,
            patrimonial::patrimonial_report,
            patrimonial::patrimonial_mark,
            patrimonial::patrimonial_settings,
            patrimonial::patrimonial_sale_add,
            patrimonial::patrimonial_sale_delete,
            oss::oss_estado,
            oss::oss_ventas,
            oss::oss_clasificar,
            oss::oss_tipos,
            oss::oss_confirmar_tipo,
            oss::oss_ajustes_guardar,
            oss::oss_libro,
            oss::oss_369,
            oss::oss_369_hoja,
            iva_holding::iva_holding_informe,
            iva_holding::iva_holding_libro,
            iva_holding::iva_holding_regla_guardar,
            iva_holding::iva_holding_regla_borrar,
            iva_holding::iva_holding_sector_guardar,
            iva_holding::iva_holding_sector_borrar,
            iva_holding::iva_holding_config,
            iva_holding::iva_holding_bien_guardar,
            accounting::deposit_build,
            accounting::settle_invoice,
            accounting::unsettle_invoice,
            accounting::update_entry,
            accounting::delete_entry,
            accounting::delete_entries,
            accounting::chart_copy_from,
            accounting::transfer_balance,
            accounting::leasing_list,
            accounting::leasing_create,
            accounting::leasing_post_due,
            accounting::leasing_set_lessor,
            accounting::leasing_regularize,
            accounting::leasing_fix_due_dates,
            accounting::leasing_delete,
            accounting::leasing_reclassify,
            prestamos::prestamos_list,
            prestamos::prestamo_propuesta,
            prestamos::prestamo_create,
            prestamos::prestamo_update,
            prestamos::prestamo_bank_candidates,
            prestamos::prestamo_post_start,
            prestamos::prestamo_start_outside,
            prestamos::prestamo_undo_start,
            prestamos::prestamo_post_installment,
            prestamos::prestamo_mark_outside,
            prestamos::prestamo_undo_installment,
            prestamos::prestamo_accrue,
            prestamos::prestamo_undo_accrual,
            prestamos::prestamos_reclassify,
            prestamos::prestamo_delete,
            prestamos::prestamo_contract_save,
            accounting::templates_list,
            accounting::templates_due,
            accounting::template_save,
            accounting::template_delete,
            accounting::template_post,
            accounting::chart,
            accounting::pgc_accounts,
            accounting::set_account,
            accounting::a3_options,
            accounting::export_a3,
            accounting::bank_import,
            extractos::bank_statements,
            extractos::bank_statement_undo,
            banca::banca_estado,
            banca::banca_ajustes,
            banca::banca_credenciales,
            banca::banca_quitar_credenciales,
            banca::banca_bancos,
            banca::banca_conectar,
            banca::banca_esperar,
            banca::banca_terminar,
            banca::banca_cancelar,
            banca::banca_actualizar,
            banca::banca_activar_cuenta,
            banca::banca_desconectar,
            accounting::bank_movements,
            accounting::bank_suggestions,
            accounting::bank_reconcile_invoice,
            accounting::bank_reconcile_account,
            accounting::bank_reconciled,
            accounting::bank_unreconcile,
            accounting::bank_rules,
            accounting::bank_rule_delete,
            accounting::bank_rule_for,
            accounting::bank_rule_proposals,
            accounting::bank_rule_apply,
            accounting::employees,
            accounting::employee_save,
            accounting::payslips,
            accounting::payslip_register,
            accounting::payslip_delete,
            accounting::reta_resumen,
            accounting::reta_ficha_save,
            accounting::reta_cuota_register,
            accounting::reta_cuota_delete,
            accounting::admin_receipt_pdf,
            accounting::bank_reconcile_remittance,
            accounting::bank_reconcile_return,
            models::model_settings,
            models::save_model_settings,
            models::model_draft,
            models::model_monthly,
            models::sii_estado,
            models::sii_plazos,
            models::tax_payment,
            models::model_adjustments,
            models::model_adjust,
            models::model_202_simulation,
            models::model_202_set_modality,
            models::model_letters_347,
            models::model_certificates_prepare,
            models::model_certificates_save,
            models::model_certificate_email,
            models::partners_list,
            models::partner_save,
            models::partner_delete,
            models::settings_184,
            models::save_settings_184,
            models::tax_pay,
            models::tax_pay_bank,
            models::tax_unpay,
            models::model_export,
            models::rectificativa_anterior,
            models::rectificativa_preparar,
            models::rectificativa_exportar,
            models::model_prevalidate,
            models::model_file_testing,
            models::filed_returns,
            models::aeat_presented,
            models::filed_receipt_download,
            models::filed_receipt_save,
            presentado::aeat_check_target,
            presentado::aeat_check_running,
            presentado::aeat_csv_running,
            presentado::aeat_check_year,
            presentado::aeat_check_years,
            presentado::aeat_check_report,
            presentado::aeat_check_summaries,
            presentado::company_evolution,
            ahorro::tax_savings,
            ahorro::tax_savings_mark,
            presentado::company_review,
            presentado::aeat_check_save,
            presentado::aeat_check_save_years,
            presentado::aeat_csv_download,
            dashboard::dashboard_get,
            dashboard::dashboard_receivables,
            forecast::cash_forecast,
            models::renta_download,
            models::renta_cross_check,
            models::renta_activity,
            updates::update_check,
            updates::update_install,
            assistant::assistant_status,
            assistant::assistant_save,
            assistant::assistant_set_key,
            mcp::mcp_estado,
            mcp::mcp_guardar,
            assistant::assistant_topic_open,
            assistant::assistant_conversation_open,
            assistant::assistant_conversations,
            assistant::assistant_topic_preview,
            assistant::assistant_topic_ask,
            assistant::assistant_fact_add,
            assistant::assistant_file_add,
            assistant::assistant_material_delete,
            assistant::assistant_conversation_delete,
            assistant::assistant_conversations_delete_all,
            assistant::received_ai_extract,
            expedientes::expedientes_list,
            expedientes::expediente_preview,
            expedientes::expediente_create,
            expedientes::expediente_update,
            expedientes::expediente_delete,
            expedientes::expediente_desde_notificacion,
            expedientes::expediente_desde_documento,
            expedientes::expediente_actuacion_add,
            expedientes::expediente_actuacion_delete,
            expedientes::expediente_escrito_nombre,
            expedientes::expediente_escrito_save,
            expedientes::expedientes_avisos,
            expedientes::certificados_admin_list,
            expedientes::certificado_admin_save,
            expedientes::certificado_admin_desde_documento,
            expedientes::certificado_admin_delete,
            notices::notices_list,
            notices::notices_refresh,
            notices::notice_download,
            notices::notice_appear,
            notices::notice_save_pdf,
            notices::notices_courtesy_days,
            notices::notices_set_courtesy_days,
            libros_societarios::partners_register,
            libros_societarios::share_movement_save,
            libros_societarios::share_movement_delete,
            libros_societarios::partners_register_apply,
            libros_societarios::sole_contracts,
            libros_societarios::sole_contract_proposals,
            libros_societarios::sole_contract_save,
            libros_societarios::sole_contract_delete,
            libros_societarios::minutes_list,
            libros_societarios::minute_template,
            libros_societarios::minute_save,
            libros_societarios::minute_delete,
            libros_societarios::minute_approve,
            libros_societarios::minute_reopen,
            libros_societarios::minute_document_save,
            libros_societarios::minute_certificate_save,
            libros_societarios::corporate_book_save,
            socios::shareholders_list,
            socios::shareholder_save,
            socios::shareholder_delete,
            socios::dividend_propose,
            socios::dividend_pay,
            socios::dividend_payments,
            socios::dividend_certificates,
            socios::actas_save,
            socios::dividend_undo,
            socios::dividend_agreement_check,
            socios::dividend_agreement_record,
            socios::dividend_agreements,
            socios::dividend_agreement_undo,
            socios::director_turnover,
            socios::travel_preview,
            socios::travel_register,
            socios::travel_list,
            socios::travel_delete,
            nominas::nomina_ficha,
            nominas::nomina_ficha_save,
            nominas::nomina_tabla,
            nominas::nomina_calcular,
            nominas::nomina_registrar,
            nominas::nominas_calculadas,
            nominas::nomina_recibo_pdf,
            nominas::seguros_sociales,
            nominas::seguros_sociales_pagar,
            nominas::finiquito_calcular,
            subvenciones::subvenciones,
            subvenciones::subvencion_guardar,
            subvenciones::subvencion_borrar,
            subvenciones::subvencion_propuesta,
            subvenciones::subvencion_registrar,
            subvenciones::subvencion_deshacer,
            subvenciones::renta_socio_personas,
            subvenciones::renta_socio,
            subvenciones::renta_socio_save,
            vinculadas::vinculadas_registro,
            vinculadas::vinculada_de,
            vinculadas::vinculada_save,
            vinculadas::vinculada_delete,
            vinculadas::participadas_list,
            vinculadas::participada_save,
            vinculadas::participada_delete,
            vinculadas::vinculada_cuenta_set,
            vinculadas::vinculada_operacion_save,
            vinculadas::vinculada_operacion_delete,
            vinculadas::vinculada_valoracion_save,
            vinculadas::vinculadas_textos_save,
            vinculadas::vinculadas_documentacion_save,
            vinculadas::vinculadas_hoja_232_save,
            modelo200::modelo200_hoja,
            modelo200::modelo200_ajuste_guardar,
            modelo200::modelo200_ajuste_quitar,
            modelo200::modelo200_datos_guardar,
            modelo200::modelo200_arrastre_guardar,
            modelo200::modelo200_arrastre_borrar,
            modelo200::modelo200_pasar_siguiente,
            modelo200::participadas_fichas,
            modelo200::participada_fiscal_guardar,
            modelo200::participada_fondos_guardar,
            modelo200::participada_dividendo_guardar,
            modelo200::participada_dividendo_borrar
        ])
        .build(tauri::generate_context!())
        .expect("error al arrancar TaxPain")
        .run(|_app, _event| {
            // Clic en el icono del Dock con la ventana escondida: se vuelve a enseñar.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } = _event
            {
                background::show_main(_app);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_aviso_de_arranque_dice_que_hacer() {
        let (title, text) = startup_error(&VaultError::KeyUnavailable("denegado".into()));
        assert_eq!(title, "TaxPain no puede abrir tus datos");
        assert!(
            text.contains("Permitir siempre") && text.contains("denegado"),
            "{text}"
        );
        let (_, text) = startup_error(&VaultError::KeyMissing { recoverable: false });
        assert!(
            text.contains("Llavero") && text.contains(".taxpain"),
            "{text}"
        );
        let (_, text) = startup_error(&VaultError::Corrupted);
        assert!(text.contains("copia de seguridad"), "{text}");
        let (_, text) = startup_error(&VaultError::InvalidInput("x".into()));
        assert!(text.contains("dato no válido: x"), "{text}");
    }

    #[test]
    fn los_errores_no_llevan_el_prefijo_tecnico() {
        let e = CommandError::from(VaultError::InvalidInput(
            "la contraseña de las copias es demasiado fácil de adivinar".into(),
        ));
        assert_eq!(
            e.message,
            "la contraseña de las copias es demasiado fácil de adivinar"
        );
        assert_eq!(CommandError::from("otro").message, "otro");
    }

    #[test]
    fn un_fichero_mayor_que_el_limite_no_se_lee() {
        let dir = std::env::temp_dir().join(format!("taxpain-read-limited-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("grande.bin");
        std::fs::write(&p, vec![0u8; 3000]).unwrap();
        let err = read_limited(&p, 2048).unwrap_err();
        assert!(err.message.contains("pasa de 1 MB"), "{}", err.message);
        assert_eq!(read_limited(&p, 4096).unwrap().len(), 3000);
        assert!(read_limited(dir.join("no existe"), 4096).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
