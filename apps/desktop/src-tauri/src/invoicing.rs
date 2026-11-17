//! Comandos de facturación: ajustes (onboarding), borrador, emisión, listado, anulación,
//! envío VERI*FACTU y facturas recibidas. La lógica está en `taxpain-app`.

use std::path::PathBuf;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;
use taxpain_aeat::AeatClient;
use taxpain_aeat::verifactu::{Destino, URL_PRUEBAS, enviar};
use taxpain_app::corregir_recibida;
use taxpain_app::invoicing::{self, InvoiceDraft, IssuedInvoice, IssuedSummary};
use taxpain_app::received::{self, IngestOutcome, ReceivedSummary, Reviewed, Source};
use taxpain_app::settings::{self, Destination, InvoicingSettings, Language};
use taxpain_app::vigiladas;
use taxpain_core::invoice::Totals;
use time::OffsetDateTime;

use crate::state::AppState;
use crate::{CmdResult, CommandError};

#[tauri::command]
pub fn invoicing_settings(
    state: tauri::State<'_, AppState>,
) -> CmdResult<Option<InvoicingSettings>> {
    Ok(invoicing::settings(&*state.vault()?)?)
}

#[tauri::command]
pub fn save_invoicing_settings(
    state: tauri::State<'_, AppState>,
    settings: InvoicingSettings,
) -> CmdResult<()> {
    Ok(invoicing::save_settings(
        &*state.vault()?,
        &settings,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn new_installation_id() -> String {
    settings::new_installation_id()
}

#[tauri::command]
pub fn detect_destinations(app: AppHandle) -> CmdResult<Vec<Destination>> {
    let home = app.path().home_dir()?;
    Ok(settings::detect_destinations(&home))
}

#[derive(Serialize)]
pub struct Preview {
    /// En la moneda de la factura.
    totals: Totals,
    /// En euros, si la factura es en otra moneda (los que se contabilizan y declaran).
    eur_totals: Option<Totals>,
    /// PDF de borrador abierto con el visor del sistema.
    opened: bool,
}

/// Calcula totales y abre el PDF de borrador en un fichero temporal.
#[tauri::command]
pub fn invoice_preview(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    draft: InvoiceDraft,
    open: bool,
    language: Option<Language>,
) -> CmdResult<Preview> {
    let p = invoicing::preview_full(
        &*state.vault()?,
        &draft,
        language,
        OffsetDateTime::now_utc(),
    )?;
    if open {
        // Carpeta privada y nombre al azar (no uno fijo en /tmp que otro pueda adivinar o
        // pisar); se borra en una hora o al volver a arrancar.
        let path = taxpain_app::documentos::temp_copy("borrador.pdf", &p.pdf)?;
        app.opener()
            .open_path(path.to_string_lossy(), None::<&str>)?;
    }
    Ok(Preview {
        totals: p.totals,
        eur_totals: p.eur_totals,
        opened: open,
    })
}

/// Emite la factura definitiva. `confirmed` sólo lo envía el diálogo de confirmación; además
/// se pide Touch ID con a quién y por cuánto, calculado aquí con lo que se va a emitir.
#[tauri::command(async)]
pub fn invoice_issue(
    state: tauri::State<'_, AppState>,
    draft: InvoiceDraft,
    confirmed: bool,
    language: Option<Language>,
) -> CmdResult<IssuedInvoice> {
    // La empresa queda fija: si se cambia mientras se pide Touch ID, no se emite en otra.
    let company = if confirmed {
        let reason = {
            let (company, v) = state.vault_with_id()?;
            (company, issue_reason(&v, &draft, language)?)
        };
        state.confirm_presence(&reason.1)?;
        Some(reason.0)
    } else {
        None
    };
    let v = match &company {
        Some(c) => state.vault_of(c)?,
        None => state.vault()?,
    };
    Ok(invoicing::issue_in(
        &v,
        &draft,
        language,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

/// Lo que dice el aviso de Touch ID al emitir `draft`: «Emitir la factura a ACME SL por
/// 1.210,00 €». Con el total que calcula Rust (el de la vista previa), no el de la interfaz.
pub(crate) fn issue_reason(
    v: &taxpain_vault::Vault,
    draft: &InvoiceDraft,
    language: Option<Language>,
) -> CmdResult<String> {
    let (totals, _) = invoicing::preview_in(v, draft, language, OffsetDateTime::now_utc())?;
    let mut amount = totals.invoice_total.to_string();
    if let Some(c) = &draft.currency {
        amount = amount.replace(" €", &format!(" {}", c.code));
    }
    let what = match draft.kind {
        taxpain_core::invoice::InvoiceKind::Rectificativa(_) => "la factura rectificativa",
        _ => "la factura",
    };
    Ok(
        match draft.recipient.as_ref().map(|p| reason_text(&p.name)) {
            Some(n) if !n.is_empty() => format!("Emitir {what} a {n} por {amount}"),
            _ => format!("Emitir {what} por {amount}"),
        },
    )
}

/// Un dato para el aviso de Touch ID: en una línea y no muy largo.
pub(crate) fn reason_text(s: &str) -> String {
    let one_line: String = s
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    if one_line.chars().count() > 60 {
        format!(
            "{}…",
            one_line.chars().take(59).collect::<String>().trim_end()
        )
    } else {
        one_line
    }
}

#[tauri::command]
pub fn invoices_list(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<Vec<IssuedSummary>> {
    Ok(invoicing::list_issued(&*state.vault()?, year)?)
}

/// Abre el PDF (o el Facturae) guardado con la app del sistema.
#[tauri::command]
pub fn invoice_open(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    id: i64,
    facturae: bool,
) -> CmdResult<()> {
    let (name, _, bytes) = invoicing::invoice_document(&*state.vault()?, id, facturae)?;
    let path = taxpain_app::documentos::temp_copy(&name, &bytes)?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)?;
    Ok(())
}

/// Guarda en `path` (elegido con el diálogo del sistema) el PDF, el Facturae o el UBL.
#[tauri::command]
pub fn invoice_save(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    id: i64,
    format: String,
    path: String,
) -> CmdResult<()> {
    let ext = match format.as_str() {
        "pdf" => "pdf",
        "facturae" | "ubl" => "xml",
        _ => return Err(CommandError::from("formato desconocido")),
    };
    let path = crate::salida::salida_permitida(&app, &path, &[ext])?;
    let v = state.vault()?;
    let (_, _, bytes) = match format.as_str() {
        "pdf" => invoicing::invoice_document(&v, id, false)?,
        "facturae" => invoicing::invoice_document(&v, id, true)?,
        _ => invoicing::invoice_ubl(&v, id)?,
    };
    crate::salida::escribir(&path, bytes)?;
    Ok(())
}

/// Firma la Facturae de una factura con el certificado elegido (Touch ID) y la guarda en `path`
/// como `.xsig`. No envía nada a FACe.
#[tauri::command]
pub async fn invoice_sign_facturae(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    id: i64,
    fingerprint: String,
    path: String,
) -> CmdResult<()> {
    // Antes de pedir Touch ID: si la ruta no vale, no se firma para nada.
    let path = crate::salida::salida_permitida(&app, &path, &["xsig"])?;
    // La empresa de la factura: si se cambia mientras se pide Touch ID, no se firma en otra.
    let (company, request) = {
        let (company, v) = state.vault_with_id()?;
        let request = v.prepare_identity(
            &fingerprint,
            "Firmar la factura electrónica (Facturae)",
            OffsetDateTime::now_utc(),
        )?;
        (company, request)
    };
    let identity = state.identity_of(&company, request)?;
    let (_, bytes) = invoicing::signed_facturae(
        &*state.vault_of(&company)?,
        id,
        &identity,
        OffsetDateTime::now_utc(),
    )?;
    crate::salida::escribir(&path, bytes)?;
    Ok(())
}

/// Anula una factura emitida: además de `confirmed` (el diálogo), Touch ID con su número.
#[tauri::command(async)]
pub fn invoice_cancel(
    state: tauri::State<'_, AppState>,
    id: i64,
    confirmed: bool,
) -> CmdResult<()> {
    let company = if confirmed {
        let (company, number) = {
            let (company, v) = state.vault_with_id()?;
            let number: String = v
                .connection()
                .query_row(
                    "SELECT number FROM invoice_issued WHERE id = ?1",
                    [id],
                    |r| r.get(0),
                )
                .map_err(|_| CommandError::from("factura no encontrada"))?;
            (company, number)
        };
        state.confirm_presence(&format!("Anular la factura {}", reason_text(&number)))?;
        Some(company)
    } else {
        None
    };
    let v = match &company {
        Some(c) => state.vault_of(c)?,
        None => state.vault()?,
    };
    Ok(invoicing::cancel(
        &v,
        id,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

#[derive(Serialize)]
pub struct SendResult {
    /// Estado del último envío de la sesión.
    estado_envio: String,
    /// Registros enviados en esta sesión (todos los lotes).
    registros: usize,
    /// Lotes enviados.
    envios: usize,
    tiempo_espera_envio: u32,
    /// Algún lote llevó `Incidencia = S` (reenvío tras un corte).
    incidencia: bool,
    /// `pruebas` (hoy, siempre).
    entorno: &'static str,
    /// Si la sesión se cortó por un fallo: qué pasó (la cola lo reintenta más tarde).
    fallo: Option<String>,
}

/// Espera máxima entre lotes dentro de una misma sesión de remisión: si la AEAT pide más, la
/// sesión termina y el resto queda en cola para el siguiente envío.
const ESPERA_MAXIMA_SESION: i64 = 180;

/// Vacía la cola de remisión VERI*FACTU (`taxpain_app::verifactu_cola`) al entorno de
/// **pruebas** de la AEAT con el certificado elegido, con un solo Touch ID: lote a lote (hasta
/// 1.000 registros), respetando el `TiempoEsperaEnvio` que devuelve la AEAT, con
/// `Incidencia = S` tras un corte y anotando cada fallo para reintentarlo. Producción queda
/// cerrada (D6): si la cola pidiera producción, se rechaza aquí sin conectar.
#[tauri::command]
pub async fn verifactu_send(
    state: tauri::State<'_, AppState>,
    fingerprint: String,
    confirmed: bool,
) -> CmdResult<Option<SendResult>> {
    use taxpain_app::verifactu_cola::{self as cola, Siguiente};
    if !confirmed {
        return Err(CommandError::from("hace falta confirmar el envío"));
    }
    // Un solo envío a la vez: dos envíos simultáneos mandarían los mismos registros.
    static SENDING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if SENDING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return Err(CommandError::from("ya hay un envío VERI*FACTU en curso"));
    }
    struct Release;
    impl Drop for Release {
        fn drop(&mut self) {
            SENDING.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }
    let _release = Release;
    // La respuesta de la AEAT se aplica a la empresa que envió, aunque se cambie de empresa
    // mientras tanto (todo va con `vault_of(&company)`).
    let (company, mut lote, request) = {
        let (company, vault) = state.vault_with_id()?;
        // Los registros se envían en nombre del emisor: el certificado debe ser suyo (o de su
        // representante).
        let cert = vault
            .certificate(&fingerprint)?
            .ok_or_else(|| CommandError::from("certificado no encontrado"))?;
        let represented = cert.entity_nif.clone().unwrap_or(cert.holder_nif.clone());
        taxpain_app::models::check_presenter(&vault, &represented)?;
        let lote = match cola::siguiente_lote(&vault, OffsetDateTime::now_utc())? {
            Siguiente::Nada => return Ok(None),
            Siguiente::Esperar(s) => {
                return Err(CommandError::from(format!(
                    "La AEAT pide esperar entre envíos: el siguiente puede salir dentro de {s} s \
                     (Orden HAC/1177/2024 art. 16.2)."
                )));
            }
            Siguiente::Bloqueada(m) => {
                return Err(CommandError::from(format!(
                    "La AEAT rechazó el último envío entero ({m}). Corrígelo y pulsa «Reintentar»."
                )));
            }
            Siguiente::Enviar(l) => l,
        };
        if lote.entorno != invoicing::Entorno::Pruebas {
            return Err(CommandError::from(
                "el envío a producción no está habilitado en esta versión de TaxPain",
            ));
        }
        let request = vault.prepare_identity(
            &fingerprint,
            "Enviar registros VERI*FACTU a la AEAT (pruebas)",
            OffsetDateTime::now_utc(),
        )?;
        (company, lote, request)
    };
    let identity = state.identity_of(&company, request)?;
    let client = AeatClient::with_platform_trust(&identity, taxpain_aeat::hosts::PRUEBAS)?;
    debug_assert!(URL_PRUEBAS.contains("prewww1"));
    let mut out = SendResult {
        estado_envio: String::new(),
        registros: 0,
        envios: 0,
        tiempo_espera_envio: 60,
        incidencia: false,
        entorno: "pruebas",
        fallo: None,
    };
    loop {
        match enviar(&client, &Destino::pruebas(), &lote.body).await {
            Ok(resp) => {
                cola::registrar_respuesta(
                    &*state.vault_of(&company)?,
                    &lote,
                    &resp,
                    OffsetDateTime::now_utc(),
                )?;
                out.estado_envio = resp.estado_envio;
                out.tiempo_espera_envio = resp.tiempo_espera_envio;
            }
            Err(e) => {
                let msg = e.to_string();
                cola::registrar_fallo(
                    &*state.vault_of(&company)?,
                    &lote,
                    &msg,
                    taxpain_aeat::verifactu::fallo_del_mensaje(&e),
                    OffsetDateTime::now_utc(),
                )?;
                if out.envios == 0 {
                    return Err(e.into());
                }
                out.fallo = Some(msg);
                break;
            }
        }
        out.envios += 1;
        out.registros += lote.ids.len();
        out.incidencia |= lote.incidencia;
        // Siguiente lote: si la AEAT pide una espera corta, se espera aquí; si es larga, queda
        // en cola para el siguiente envío.
        let next = loop {
            let s = cola::siguiente_lote(&*state.vault_of(&company)?, OffsetDateTime::now_utc())?;
            match s {
                Siguiente::Esperar(secs) if secs <= ESPERA_MAXIMA_SESION => {
                    let d = Duration::from_secs(u64::try_from(secs).unwrap_or(0) + 1);
                    let _ =
                        tauri::async_runtime::spawn_blocking(move || std::thread::sleep(d)).await;
                }
                other => break other,
            }
        };
        match next {
            Siguiente::Enviar(l) if l.entorno == invoicing::Entorno::Pruebas => lote = l,
            _ => break,
        }
    }
    Ok(Some(out))
}

/// Estado de la cola de remisión VERI*FACTU (pendientes, espera, incidencias, rechazos).
#[tauri::command]
pub fn verifactu_queue_status(
    state: tauri::State<'_, AppState>,
) -> CmdResult<taxpain_app::verifactu_cola::EstadoCola> {
    Ok(taxpain_app::verifactu_cola::estado(
        &*state.vault()?,
        OffsetDateTime::now_utc(),
    )?)
}

/// Desbloquea la cola tras un rechazo del mensaje entero por la AEAT. No envía nada.
#[tauri::command]
pub fn verifactu_queue_reopen(state: tauri::State<'_, AppState>, confirmed: bool) -> CmdResult<()> {
    if !confirmed {
        return Err(CommandError::from("hace falta confirmar"));
    }
    Ok(taxpain_app::verifactu_cola::reabrir(
        &*state.vault()?,
        OffsetDateTime::now_utc(),
    )?)
}

/// La declaración responsable del SIF de esta versión (Ajustes › Acerca de TaxPain).
#[tauri::command]
pub fn declaracion_responsable() -> taxpain_app::declaracion_responsable::Declaracion {
    taxpain_app::declaracion_responsable::declaracion()
}

/// Guarda la declaración responsable en PDF para firmarla.
#[tauri::command]
pub fn declaracion_responsable_save(app: AppHandle, path: String) -> CmdResult<()> {
    let path = crate::salida::salida_permitida(&app, &path, &["pdf"])?;
    crate::salida::escribir(&path, taxpain_app::declaracion_responsable::pdf()?)?;
    Ok(())
}

/// Registros VERI*FACTU rechazados o aceptados con errores, pendientes de subsanar.
#[tauri::command]
pub fn verifactu_issues(
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<invoicing::RecordIssue>> {
    Ok(invoicing::verifactu_issues(&*state.vault()?)?)
}

/// Genera el registro de subsanación (queda pendiente de envío). No envía nada. Además de
/// `confirmed` (el diálogo), Touch ID con el número de la factura.
#[tauri::command(async)]
pub fn verifactu_resubmit(
    state: tauri::State<'_, AppState>,
    record_id: i64,
    confirmed: bool,
) -> CmdResult<i64> {
    let company = if confirmed {
        let (company, number) = {
            let (company, v) = state.vault_with_id()?;
            let number: String = v
                .connection()
                .query_row(
                    "SELECT num_serie FROM verifactu_record WHERE id = ?1",
                    [record_id],
                    |r| r.get(0),
                )
                .map_err(|_| CommandError::from("registro no encontrado"))?;
            (company, number)
        };
        state.confirm_presence(&format!(
            "Subsanar el registro VERI*FACTU de la factura {}",
            reason_text(&number)
        ))?;
        Some(company)
    } else {
        None
    };
    let v = match &company {
        Some(c) => state.vault_of(c)?,
        None => state.vault()?,
    };
    Ok(invoicing::resubmit(
        &v,
        record_id,
        OffsetDateTime::now_utc(),
        confirmed,
    )?)
}

// ---------- Recibidas ----------

/// Lo que ha pasado al importar recibidas: las importadas y, de las que no, por qué.
#[derive(Serialize)]
pub struct ReceivedImport {
    pub outcomes: Vec<IngestOutcome>,
    pub errors: Vec<String>,
}

/// Asíncrono: leer una foto o un escaneado con el Mac tarda un momento por fichero, y se lee
/// sin el almacén cogido (sólo para mirar si ya estaba y para guardarlo). Un fichero que no se
/// puede importar (demasiado grande, ilegible) no para los demás.
#[tauri::command(async)]
pub fn received_import(
    state: tauri::State<'_, AppState>,
    paths: Vec<String>,
) -> CmdResult<ReceivedImport> {
    let company = state.active_id()?;
    let mut out = ReceivedImport {
        outcomes: Vec::new(),
        errors: Vec::new(),
    };
    for p in paths {
        let path = PathBuf::from(&p);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "documento".into());
        if !crate::salida::extension_en(&name, &crate::salida::RECIBIDAS) {
            out.errors.push(format!(
                "{name}: no es una factura que TaxPain sepa leer (PDF, XML o una foto)"
            ));
            continue;
        }
        let r = crate::read_limited(&path, received::MAX_FILE_SIZE as u64).and_then(|bytes| {
            Ok(received::ingest_file_with(
                state.vault_fn(&company),
                &name,
                &bytes,
                Source::Manual,
                OffsetDateTime::now_utc(),
            )?)
        });
        match r {
            Ok(o) => out.outcomes.push(o),
            Err(e) => out.errors.push(format!("{name}: {}", e.message)),
        }
    }
    Ok(out)
}

#[tauri::command]
pub fn received_list(
    state: tauri::State<'_, AppState>,
    status: Option<String>,
) -> CmdResult<Vec<ReceivedSummary>> {
    Ok(received::list_received(
        &*state.vault()?,
        status.as_deref(),
    )?)
}

#[tauri::command]
pub fn received_review(
    state: tauri::State<'_, AppState>,
    id: i64,
    reviewed: Reviewed,
    currency: Option<received::ReceivedFx>,
) -> CmdResult<()> {
    Ok(received::review_fx(
        &*state.vault()?,
        id,
        &reviewed,
        currency.as_ref(),
        OffsetDateTime::now_utc(),
    )?)
}

/// Comprueba los datos de una recibida antes de confirmarlos (la primera revisión o una
/// corrección): el resumen que se enseña para confirmar y los avisos. No cambia nada.
#[tauri::command]
pub fn received_check(
    state: tauri::State<'_, AppState>,
    id: i64,
    reviewed: Reviewed,
    currency: Option<received::ReceivedFx>,
) -> CmdResult<corregir_recibida::Comprobacion> {
    Ok(corregir_recibida::comprobar(
        &*state.vault()?,
        id,
        &reviewed,
        currency.as_ref(),
        OffsetDateTime::now_utc(),
    )?)
}

/// Corrige una recibida ya revisada y rehace su asiento (queda en la auditoría con el antes y el
/// después).
#[tauri::command]
pub fn received_correct(
    state: tauri::State<'_, AppState>,
    id: i64,
    reviewed: Reviewed,
    currency: Option<received::ReceivedFx>,
) -> CmdResult<corregir_recibida::Corregida> {
    Ok(corregir_recibida::corregir(
        &*state.vault()?,
        id,
        &reviewed,
        currency.as_ref(),
        OffsetDateTime::now_utc(),
    )?)
}

/// Lo que conviene saber (o lo que impide) deshacer la revisión de una recibida.
#[tauri::command]
pub fn received_unreview_check(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> CmdResult<Vec<String>> {
    Ok(corregir_recibida::comprobar_deshacer(&*state.vault()?, id)?)
}

/// Una revisada sin pagos vuelve a pendiente (se borra su asiento).
#[tauri::command]
pub fn received_unreview(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    Ok(corregir_recibida::deshacer(
        &*state.vault()?,
        id,
        OffsetDateTime::now_utc(),
    )?)
}

/// Factura recibida tecleada desde el diario, sin documento.
#[tauri::command]
pub fn received_register_typed(
    state: tauri::State<'_, AppState>,
    reviewed: Reviewed,
    currency: Option<received::ReceivedFx>,
) -> CmdResult<i64> {
    Ok(received::register_typed_fx(
        &*state.vault()?,
        &reviewed,
        currency.as_ref(),
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn received_discard(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    Ok(received::discard(
        &*state.vault()?,
        id,
        OffsetDateTime::now_utc(),
    )?)
}

/// Una descartada por error vuelve a pendiente.
#[tauri::command]
pub fn received_undiscard(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    Ok(received::undiscard(
        &*state.vault()?,
        id,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn received_open(app: AppHandle, state: tauri::State<'_, AppState>, id: i64) -> CmdResult<()> {
    let (name, _, bytes) = received::received_document(&*state.vault()?, id)?;
    // Copia descifrada para abrirla: carpeta privada, nombre aleatorio, se borra en una hora.
    let dir = private_temp_dir("taxpain-recibidas")?;
    let mut rnd = [0u8; 6];
    getrandom::fill(&mut rnd).map_err(|e| CommandError::from(e.to_string()))?;
    let safe: String = name
        .chars()
        .map(|c| if c == '/' || c == '\\' { '-' } else { c })
        .collect();
    let path = dir.join(format!("{}-{safe}", hex::encode(rnd)));
    std::fs::write(&path, bytes)?;
    crate::salida::abrir_copia(&app, &path)
}

#[tauri::command]
pub fn watched_folders(state: tauri::State<'_, AppState>) -> CmdResult<Vec<String>> {
    Ok(vigiladas::folders(&*state.vault()?)?)
}

/// Cambia las carpetas vigiladas: añadir una pide Touch ID con cuál.
#[tauri::command(async)]
pub fn set_watched_folders(
    state: tauri::State<'_, AppState>,
    folders: Vec<String>,
) -> CmdResult<Vec<String>> {
    crate::confirmar::guardar_carpetas_vigiladas(&state, &folders)
}

/// Documentos nuevos de las carpetas vigiladas que no son facturas: se importan desde Importar.
#[tauri::command]
pub fn watched_pending(state: tauri::State<'_, AppState>) -> CmdResult<Vec<vigiladas::Pending>> {
    Ok(vigiladas::pending(&*state.vault()?)?)
}

#[tauri::command]
pub fn watched_dismiss(state: tauri::State<'_, AppState>, folder: Option<String>) -> CmdResult<()> {
    Ok(vigiladas::dismiss(&*state.vault()?, folder.as_deref())?)
}

/// Revisa las carpetas vigiladas cada 2 minutos, con sus subcarpetas (sólo los ficheros nuevos o
/// cambiados). Cada empresa, las suyas. La empresa abierta, cogiendo su almacén sólo para mirar
/// y guardar cada fichero (leer un escaneado con el Mac puede tardar y la app tiene que seguir
/// respondiendo); las demás, con su almacén abierto aparte.
pub fn spawn_watch_loop(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            // Un pánico en una pasada (un fichero raro que hace fallar un lector) no puede
            // parar la vigilancia para siempre: se anota y se sigue en la próxima.
            let pass = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                watch_pass(&app);
            }));
            if pass.is_err() {
                tracing::error!("fallo inesperado al revisar las carpetas vigiladas");
            }
            std::thread::sleep(Duration::from_secs(120));
        }
    });
}

/// Una pasada por las carpetas vigiladas de todas las empresas.
fn watch_pass(app: &AppHandle) {
    let state = app.state::<AppState>();
    // De paso, las copias que se dejaron a otras apps (correo, documentos) hace más de
    // una hora.
    purge_private_temp_dirs();
    let now = OffsetDateTime::now_utc();
    let others = state.for_each_company(|id, active, vault| {
        if active {
            return Ok(());
        }
        log_pass(id, &vigiladas::scan(vault, now)?);
        Ok(())
    });
    let active = (|| -> CmdResult<()> {
        let id = state.active_id()?;
        log_pass(&id, &vigiladas::scan_with(state.vault_fn(&id), now)?);
        Ok(())
    })();
    for e in [others.err(), active.err()].into_iter().flatten() {
        tracing::warn!(error = %e.message, "no se pudieron revisar las carpetas vigiladas");
    }
}

/// Al registro, sólo los números: el detalle de los errores lleva rutas y nombres de fichero
/// (que suelen llevar el nombre del cliente o del proveedor).
fn log_pass(company: &str, p: &vigiladas::Pass) {
    if p.invoices > 0 || p.pending > 0 {
        tracing::info!(
            empresa = %company,
            facturas = p.invoices,
            avisos = p.pending,
            "carpetas vigiladas revisadas"
        );
    }
    if p.error_count() > 0 || p.unreadable > 0 {
        tracing::warn!(
            empresa = %company,
            errores = p.error_count(),
            ilegibles = p.unreadable,
            "carpetas vigiladas con ficheros que no se han podido mirar"
        );
    }
}

/// Correo propuesto para una factura o un recordatorio de pago (ver `taxpain_app::correo`).
#[tauri::command]
pub fn invoice_email_compose(
    state: tauri::State<'_, AppState>,
    id: i64,
    reminder: bool,
) -> CmdResult<taxpain_app::correo::EmailDraft> {
    let today = taxpain_core::spain_time::peninsular_date(OffsetDateTime::now_utc());
    Ok(taxpain_app::correo::compose(
        &*state.vault()?,
        id,
        reminder,
        today,
    )?)
}

/// Prepara el `.eml` con la factura adjunta y lo abre en el programa de correo (Apple Mail lo
/// abre como mensaje nuevo sin enviar). TaxPain no envía nada.
#[tauri::command]
pub fn invoice_email_open(
    app: AppHandle,
    state: tauri::State<'_, AppState>,
    id: i64,
    reminder: bool,
    to: Option<String>,
    subject: String,
    body: String,
) -> CmdResult<()> {
    let (name, bytes) =
        taxpain_app::correo::build(&*state.vault()?, id, to.as_deref(), &subject, &body)?;
    let path = private_file("taxpain-correo", &name)?;
    std::fs::write(&path, bytes)?;
    app.opener()
        .open_path(path.to_string_lossy(), None::<&str>)?;
    // Se anota una vez abierto (si falla, el recordatorio no cuenta).
    taxpain_app::correo::record_sent(&*state.vault()?, id, reminder, OffsetDateTime::now_utc())?;
    Ok(())
}

/// Ruta para un fichero en la carpeta temporal privada `dir`, con un prefijo aleatorio.
pub(crate) fn private_file(dir: &str, name: &str) -> CmdResult<PathBuf> {
    let dir = private_temp_dir(dir)?;
    let mut rnd = [0u8; 6];
    getrandom::fill(&mut rnd).map_err(|e| CommandError::from(e.to_string()))?;
    let safe: String = name
        .chars()
        .map(|c| if c == '/' || c == '\\' { '-' } else { c })
        .collect();
    Ok(dir.join(format!("{}-{safe}", hex::encode(rnd))))
}

/// Carpetas temporales privadas de la app (las de [`private_temp_dir`]).
const PRIVATE_TEMP_DIRS: [&str; 3] = [
    "taxpain-correo",
    "taxpain-recibidas",
    "taxpain-presupuestos",
];

/// Lo que se deja a otra app se borra pasada una hora.
const PRIVATE_TEMP_MAX_AGE: Duration = Duration::from_secs(3600);

/// Carpeta temporal sólo para el usuario (0700) en la que se borran los ficheros de más de una
/// hora: lo que se deja para otra app (un correo con la factura) no se queda para siempre. Se
/// limpian también al arrancar y cada poco con la app abierta ([`purge_private_temp_dirs`]).
pub(crate) fn private_temp_dir(name: &str) -> CmdResult<PathBuf> {
    debug_assert!(
        PRIVATE_TEMP_DIRS.contains(&name),
        "añádela a PRIVATE_TEMP_DIRS"
    );
    let dir = std::env::temp_dir().join(name);
    std::fs::create_dir_all(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    taxpain_app::documentos::clean_dir(&dir, PRIVATE_TEMP_MAX_AGE);
    Ok(dir)
}

/// Borra de las carpetas temporales privadas lo que tenga más de una hora (un `.eml` con la
/// factura que no se volvió a usar no se queda hasta el siguiente correo).
pub(crate) fn purge_private_temp_dirs() {
    for name in PRIVATE_TEMP_DIRS {
        taxpain_app::documentos::clean_dir(&std::env::temp_dir().join(name), PRIVATE_TEMP_MAX_AGE);
    }
}

fn today_es() -> time::Date {
    taxpain_core::spain_time::peninsular_date(OffsetDateTime::now_utc())
}

#[tauri::command]
pub fn recurring_list(
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<taxpain_app::recurring::TemplateRow>> {
    Ok(taxpain_app::recurring::list(&*state.vault()?, today_es())?)
}

/// Repetir una factura emitida cada `every_months` meses.
#[tauri::command]
pub fn recurring_create(
    state: tauri::State<'_, AppState>,
    invoice_id: i64,
    every_months: u32,
) -> CmdResult<taxpain_app::recurring::InvoiceTemplate> {
    Ok(taxpain_app::recurring::create_from_invoice(
        &*state.vault()?,
        invoice_id,
        every_months,
        OffsetDateTime::now_utc(),
    )?)
}

#[tauri::command]
pub fn recurring_update(
    state: tauri::State<'_, AppState>,
    id: String,
    patch: taxpain_app::recurring::TemplatePatch,
) -> CmdResult<()> {
    Ok(taxpain_app::recurring::update(
        &*state.vault()?,
        &id,
        &patch,
        OffsetDateTime::now_utc(),
    )?)
}

/// Borra una factura periódica: Touch ID con su nombre.
#[tauri::command(async)]
pub fn recurring_delete(state: tauri::State<'_, AppState>, id: String) -> CmdResult<()> {
    crate::confirmar::borrar_periodica(&state, &id)
}

#[derive(Serialize)]
pub struct RecurringPreview {
    draft: InvoiceDraft,
    /// En la moneda de la plantilla.
    totals: Totals,
    /// Factura atrasada de un periodo de IVA ya presentado o vencido: por qué (hay que elegir
    /// cómo emitirla, `late` en [`recurring_issue`]).
    late_vat: Option<String>,
}

/// Lo que se emitiría hoy con la plantilla (para confirmarlo).
#[tauri::command]
pub fn recurring_preview(
    state: tauri::State<'_, AppState>,
    id: String,
) -> CmdResult<RecurringPreview> {
    let v = state.vault()?;
    let today = today_es();
    let (draft, language) = taxpain_app::recurring::preview_draft(&v, &id, today)?;
    let (totals, _) = invoicing::preview_in(&v, &draft, language, OffsetDateTime::now_utc())?;
    let late_vat = taxpain_app::recurring::late_vat(&v, &draft, today)?;
    Ok(RecurringPreview {
        draft,
        totals,
        late_vat,
    })
}

/// Emite la factura de una periódica. Además de `confirmed` (el diálogo), Touch ID con a quién
/// y por cuánto (la factura que toca, calculada aquí).
#[tauri::command(async)]
pub fn recurring_issue(
    state: tauri::State<'_, AppState>,
    id: String,
    confirmed: bool,
    currency: Option<taxpain_core::fx::ForeignCurrency>,
    late: Option<taxpain_app::recurring::LateVat>,
) -> CmdResult<IssuedInvoice> {
    let company = if confirmed {
        let (company, reason) = {
            let (company, v) = state.vault_with_id()?;
            let (mut draft, language) = taxpain_app::recurring::preview_draft(&v, &id, today_es())?;
            // Con el tipo de cambio que se va a usar (sólo cambia el tipo, no la moneda).
            if let (Some(c), Some(old)) = (&currency, &draft.currency)
                && c.code == old.code
            {
                draft.currency = Some(c.clone());
            }
            (company, issue_reason(&v, &draft, language)?)
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
    Ok(taxpain_app::recurring::issue_with(
        &v,
        &id,
        today_es(),
        OffsetDateTime::now_utc(),
        confirmed,
        currency,
        late,
    )?)
}

/// Tipo de cambio de referencia del BCE de `currency` vigente el `date` (botón «Tipo del BCE»):
/// sólo cuando el usuario lo pide, de la caché si ya se descargó. Se envían la moneda y las
/// fechas al BCE; nada más.
#[tauri::command]
pub async fn fx_bce_rate(
    state: tauri::State<'_, AppState>,
    currency: String,
    date: time::Date,
) -> CmdResult<taxpain_app::fx::BceRate> {
    let currency = currency.trim().to_ascii_uppercase();
    if taxpain_core::fx::currency(&currency).is_none_or(|c| !c.ecb) {
        return Err(CommandError::from(
            "el BCE no publica tipo de referencia para esa moneda: escríbelo a mano",
        ));
    }
    if let Some(r) = taxpain_app::fx::cached(&*state.vault()?, &currency, date)? {
        return Ok(r);
    }
    // Sin tener el almacén bloqueado mientras se espera a la red.
    let r = taxpain_aeat::bce::reference_rate(&currency, date).await?;
    let rate = taxpain_app::fx::BceRate {
        currency: r.currency,
        requested: date,
        date: r.date,
        rate: r.rate,
        cached: false,
    };
    taxpain_app::fx::record(
        &*state.vault()?,
        taxpain_aeat::bce::HOST,
        &rate,
        today_es(),
        OffsetDateTime::now_utc(),
    )?;
    Ok(rate)
}

/// Borrador de una factura emitida, para «Duplicar».
#[tauri::command]
pub fn invoice_draft_of(state: tauri::State<'_, AppState>, id: i64) -> CmdResult<InvoiceDraft> {
    Ok(taxpain_app::recurring::draft_of(&*state.vault()?, id)?)
}

/// Idioma en que se emitió una factura (para que «Duplicar» la saque igual).
#[tauri::command]
pub fn invoice_language_of(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> CmdResult<Option<Language>> {
    Ok(invoicing::language_of(&*state.vault()?, id)?)
}
