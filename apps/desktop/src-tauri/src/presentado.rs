//! «Comprobar lo presentado» (la lógica está en `taxpain-app::presentado_aeat`): la consulta de
//! declaraciones presentadas de la AEAT con el certificado, modelo a modelo, y la descarga por
//! CSV de los justificantes (servicio web oficial con el mismo certificado y, si falla, la Sede
//! pública) y del fichero de cada declaración. Todo es de sólo lectura.
//!
//! También «Descargar de la AEAT por CSV» ([`aeat_csv_download`]): cualquier documento de la
//! AEAT por su CSV, con la misma descarga, a la carpeta de la empresa
//! (`taxpain-app::descarga_csv`).
//!
//! La empresa se fija al empezar: si el usuario cambia de empresa a medias, la comprobación
//! termina sin guardar nada en la otra. Varios años se comprueban con una sola confirmación del
//! certificado (el mismo cliente), avisando del progreso con el evento `aeat-check-progress`.

use serde::Serialize;
use tauri::Emitter;
use taxpain_aeat::cotejo::{self, Outcome, PublicClient};
use taxpain_aeat::declarations;
use taxpain_aeat::{AeatClient, AeatError, hosts};
use taxpain_app::descarga_csv::{self, Fetched};
use taxpain_app::presentado_aeat::Visor;
use taxpain_app::presentado_aeat::{self, Answer, Download, Presented, Report, YearSummary};
use time::OffsetDateTime;

use crate::state::AppState;
use crate::{CmdResult, CommandError};

/// Respuestas definitivas del servicio web de cotejo tras las que se deja de usar en esta
/// comprobación (la AEAT limita los accesos fallidos al cotejo por día).
const SOAP_FINAL_LIMIT: usize = 3;
/// Fallos seguidos de la Sede pública (que no son de red) tras los que se deja de pedir.
const PUBLIC_FAIL_LIMIT: usize = 3;
/// En «Descargar por CSV», códigos seguidos que la Sede pública contesta sin documento tras los
/// que se deja de pedir. Los pega el usuario: uno mal copiado o anulado es normal y no cuenta
/// como fallo de la Sede, pero muchos seguidos apuntan a que la Sede no los da ahora (y la AEAT
/// limita los accesos fallidos al cotejo por día).
const PUBLIC_NONE_LIMIT: usize = 10;
/// Fallos seguidos **de red** (o HTTP) del visor de la declaración tras los que se deja de pedir
/// en esta comprobación. Una respuesta sin la declaración (las antiguas) no es un fallo: es
/// definitiva para ese documento y no corta nada.
const VISOR_FAIL_LIMIT: usize = 5;
/// Años que se pueden comprobar de una vez como máximo.
const MAX_YEARS: i32 = 15;

fn to_app(p: declarations::Presented, year: i32) -> Presented {
    Presented {
        model: p.model,
        year: p.year.trim().parse().unwrap_or(year),
        period: p.period,
        csv: p.csv,
        file_number: p.file_number,
        receipt: p.receipt_number,
        previous_receipt: p.previous_receipt.filter(|s| !s.trim().is_empty()),
        presented_at: p.presented_at,
    }
}

/// El certificado elegido frente a la empresa abierta (para la confirmación).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CheckTarget {
    pub company_nif: Option<String>,
    pub company_name: Option<String>,
    pub cert_nif: String,
    pub cert_name: String,
    /// El certificado es de la empresa (o de su representante).
    pub same: bool,
    /// El certificado es de representante de **otra** entidad: no se puede usar.
    pub other_entity: bool,
}

pub(crate) fn check_target(v: &taxpain_vault::Vault, fingerprint: &str) -> CmdResult<CheckTarget> {
    let cert = v
        .certificate(fingerprint)?
        .ok_or_else(|| CommandError::from("certificado no encontrado"))?;
    let cert_nif = cert
        .entity_nif
        .clone()
        .unwrap_or(cert.holder_nif.clone())
        .trim()
        .to_uppercase();
    let cert_name = cert
        .entity_name
        .clone()
        .or(cert.holder_name.clone())
        .unwrap_or_default();
    let company_nif = presentado_aeat::company_nif(v);
    let same = company_nif.as_deref() == Some(cert_nif.as_str());
    Ok(CheckTarget {
        other_entity: !same && cert.entity_nif.is_some(),
        same,
        company_name: presentado_aeat::company_name(v),
        company_nif,
        cert_nif,
        cert_name,
    })
}

/// ¿Se puede usar este certificado con la empresa abierta? Devuelve el NIF de la empresa. Uno de
/// representante de **otra** entidad nunca; uno personal que no es de la empresa (apoderado o
/// colaborador social), sólo si el usuario lo ha confirmado (`other_holder`). `para`: lo que se
/// iba a hacer, para el mensaje de que falta el NIF.
pub(crate) fn allow_cert(t: &CheckTarget, other_holder: bool, para: &str) -> CmdResult<String> {
    let Some(nif) = t.company_nif.clone() else {
        return Err(CommandError::from(format!(
            "Pon el NIF de la empresa (en Facturación o en la ficha de la empresa) antes de \
             {para}."
        )));
    };
    if t.other_entity {
        return Err(CommandError::from(format!(
            "El certificado es de representante de {} y la empresa abierta es {nif}: elige \
             el certificado de esta empresa.",
            t.cert_nif
        )));
    }
    if !t.same && !other_holder {
        return Err(CommandError::from(format!(
            "El certificado es de {} y no de la empresa ({nif}): confirma que quieres \
             consultar con él.",
            t.cert_nif
        )));
    }
    Ok(nif)
}

/// De quién es el certificado elegido y de quién la empresa abierta (sin consultar nada).
#[tauri::command]
pub fn aeat_check_target(
    state: tauri::State<'_, AppState>,
    fingerprint: String,
) -> CmdResult<CheckTarget> {
    check_target(&*state.vault()?, &fingerprint)
}

/// La operación con la AEAT en marcha: [`IDLE`], [`Op::Check`] u [`Op::Csv`]. Sólo una a la vez
/// (de cualquier empresa): dos a la vez pisarían el tope de peticiones a la Sede y la pantalla no
/// sabría de cuál es el progreso. Se guarda cuál es para que cada pantalla diga la suya.
static RUNNING: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(IDLE);
const IDLE: u8 = 0;

/// Qué operación con la AEAT está en marcha.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum Op {
    /// «Comprobar lo presentado» (Modelos).
    Check = 1,
    /// «Descargar de la AEAT por CSV» (Importar).
    Csv = 2,
}

impl Op {
    /// El evento con el que avisa a la ventana al acabar.
    fn done_event(self) -> &'static str {
        match self {
            Op::Check => "aeat-check-done",
            Op::Csv => "aeat-csv-done",
        }
    }
}

/// Por qué no se puede empezar si ya hay `running` en marcha.
fn busy_message(running: u8) -> &'static str {
    if running == Op::Csv as u8 {
        "Hay una descarga de documentos de la AEAT por CSV en marcha (en Importar): espera a que \
         termine."
    } else {
        "Ya hay una comprobación de lo presentado con la AEAT en marcha: espera a que termine."
    }
}

/// Marca la operación en marcha mientras vive; al soltarse (bien, con error o si se cancela la
/// tarea) la quita y avisa a la ventana (`aeat-check-done` o `aeat-csv-done`).
struct Running {
    app: tauri::AppHandle,
    op: Op,
}

impl Running {
    fn take(app: &tauri::AppHandle, op: Op) -> CmdResult<Self> {
        use std::sync::atomic::Ordering;
        RUNNING
            .compare_exchange(IDLE, op as u8, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|running| CommandError::from(busy_message(running)))?;
        Ok(Running {
            app: app.clone(),
            op,
        })
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        RUNNING.store(IDLE, std::sync::atomic::Ordering::SeqCst);
        let _ = self.app.emit(self.op.done_event(), ());
    }
}

/// ¿Hay una comprobación de lo presentado en marcha? (La pantalla, al volver a Modelos, espera a
/// que acabe.) Una descarga por CSV no cuenta: es otra operación.
#[tauri::command]
pub fn aeat_check_running() -> bool {
    RUNNING.load(std::sync::atomic::Ordering::SeqCst) == Op::Check as u8
}

/// ¿Hay una descarga por CSV en marcha? (Al volver a Importar, el recuadro espera a que acabe.)
#[tauri::command]
pub fn aeat_csv_running() -> bool {
    RUNNING.load(std::sync::atomic::Ordering::SeqCst) == Op::Csv as u8
}

/// Una comprobación en curso: la empresa fijada y los clientes.
struct Run<'a> {
    state: &'a AppState,
    company: String,
    nif: String,
    name: String,
    client: AeatClient,
    cotejo: Cotejo,
    visor_fail: usize,
    visor_stop: bool,
}

/// Qué se descarga por CSV, para los mensajes.
#[derive(Clone, Copy)]
enum What {
    /// El justificante de una declaración («Comprobar lo presentado»).
    Justificante,
    /// Cualquier documento de la AEAT («Descargar de la AEAT por CSV»).
    Documento,
}

impl What {
    fn noun(self) -> &'static str {
        match self {
            What::Justificante => "el justificante",
            What::Documento => "el documento",
        }
    }
}

/// La descarga de documentos por su CSV: el servicio web oficial con el certificado (si lo hay)
/// y, si no da el documento, la Sede pública, sin certificado. Lleva la cuenta de las respuestas
/// definitivas del servicio web y de los fallos de la Sede para no insistir (la AEAT limita los
/// accesos fallidos al cotejo por día). Nunca se registra el CSV.
struct Cotejo {
    public: Option<PublicClient>,
    soap: bool,
    soap_final: usize,
    public_fail: usize,
    /// Códigos seguidos sin documento en la Sede (sólo en [`What::Documento`]).
    public_none: usize,
    what: What,
}

/// Prepara la comprobación: empresa abierta, NIF, certificado (con Touch ID) y clientes.
/// `other_holder`: el usuario ha confirmado usar un certificado personal que no es de la empresa
/// (apoderado o colaborador social); la consulta va siempre con el NIF de la empresa.
fn start<'a>(
    state: &'a AppState,
    fingerprint: &str,
    other_holder: bool,
    purpose: &str,
) -> CmdResult<Run<'a>> {
    let (company, request, nif, name) = {
        let (company, v) = state.vault_with_id()?;
        let t = check_target(&v, fingerprint)?;
        let nif = allow_cert(&t, other_holder, "comprobar lo presentado")?;
        let name = t.company_name.clone().unwrap_or(t.cert_name.clone());
        let request = v.prepare_identity(fingerprint, purpose, OffsetDateTime::now_utc())?;
        (company, request, nif, name)
    };
    let identity = state.identity_of(&company, request)?;
    let client = AeatClient::with_platform_trust(&identity, hosts::PRODUCCION)?;
    Ok(Run {
        state,
        company,
        nif,
        name,
        client,
        cotejo: Cotejo::new(What::Justificante),
        visor_fail: 0,
        visor_stop: false,
    })
}

/// Progreso de una comprobación (evento `aeat-check-progress`).
#[derive(Clone, Debug, Serialize)]
pub struct Progress {
    pub year: i32,
    /// «consulta» o «descarga».
    pub step: &'static str,
    pub done: usize,
    pub total: usize,
}

/// Lo que dio el visor de la declaración (antes de guardarlo).
enum VisorGot {
    File(String),
    Stored,
    Final(&'static str),
    Unknown,
}

impl VisorGot {
    fn as_visor(&self) -> Visor<'_> {
        match self {
            VisorGot::File(f) => Visor::File(f),
            VisorGot::Stored => Visor::Stored,
            VisorGot::Final(s) => Visor::Final(s),
            VisorGot::Unknown => Visor::Unknown,
        }
    }
}

/// Lo descargado de un justificante o un documento (antes de guardarlo).
enum Got {
    Pdf(Vec<u8>),
    Failed(String),
    /// Respuesta definitiva del servicio web: estado («no_encontrado», «anulado»,
    /// «no_cotejable»), mensaje y, si está anulado, el CSV del que lo sustituye.
    Final(&'static str, String, Option<String>),
}

impl Cotejo {
    fn new(what: What) -> Self {
        Cotejo {
            public: PublicClient::new().ok(),
            soap: true,
            soap_final: 0,
            public_fail: 0,
            public_none: 0,
            what,
        }
    }

    /// El PDF de un CSV: el servicio web con el certificado de `client` y, si no da el
    /// documento (o no hay certificado), la Sede pública.
    async fn pdf(&mut self, client: Option<&AeatClient>, csv: &str) -> Got {
        if let Some(client) = client.filter(|_| self.soap) {
            let r = cotejo::fetch(client, cotejo::SOAP_PRODUCCION, csv).await;
            if let Some(got) = self.soap_answer(r) {
                return got;
            }
        }
        let Some(pc) = &self.public else {
            return Got::Failed(format!("No se pudo descargar {}.", self.what.noun()));
        };
        if let Some(got) = self.public_stop() {
            return got;
        }
        let r = pc.pdf_answer(csv).await;
        self.public_answer(r)
    }

    /// Lo que contestó el servicio web; `None` si hay que ir a la Sede pública.
    fn soap_answer(&mut self, r: Result<Outcome, AeatError>) -> Option<Got> {
        match r {
            Ok(Outcome::Document(b)) if b.starts_with(b"%PDF") => {
                // En la descarga por CSV sólo cuentan las respuestas definitivas seguidas.
                if matches!(self.what, What::Documento) {
                    self.soap_final = 0;
                }
                Some(Got::Pdf(b))
            }
            // En la descarga por CSV este texto no se enseña: lo pone la pantalla (`csvLine`).
            Ok(Outcome::NotFound) => Some(self.final_answer(
                "no_encontrado",
                "La AEAT no encuentra el documento de este justificante.",
                None,
            )),
            Ok(Outcome::Annulled { replacement }) => Some(self.final_answer(
                "anulado",
                "La AEAT tiene este documento como anulado.",
                replacement,
            )),
            Ok(Outcome::NotAvailable) => Some(self.final_answer(
                "no_cotejable",
                "La AEAT no permite descargar este documento.",
                None,
            )),
            // El servicio respondió sin el PDF (un error suyo o un documento que no es un
            // PDF): se registra qué dijo, sin el CSV, y se deja para esta comprobación,
            // porque contesta igual a todos (con el certificado de un representante, por
            // ejemplo). El resto va a la Sede pública.
            Ok(Outcome::Error { code, message }) => {
                tracing::warn!(%code, %message, "cotejo por servicio web sin documento; se usa la Sede");
                self.soap = false;
                None
            }
            Ok(Outcome::Document(b)) => {
                let kind = if b.starts_with(b"<") {
                    "XML"
                } else {
                    "otro formato"
                };
                tracing::warn!(
                    bytes = b.len(),
                    kind,
                    "cotejo por servicio web con un documento que no es PDF; se usa la Sede"
                );
                self.soap = false;
                None
            }
            // Si el servicio web no responde (red, certificado no admitido…), el resto va
            // directo a la Sede pública: no se insiste petición tras petición.
            Err(e) => {
                tracing::warn!(error = %e, "servicio web de cotejo no disponible; se usa la Sede");
                self.soap = false;
                None
            }
        }
    }

    /// Si ya no se pide a la Sede pública en esta tanda, por qué.
    fn public_stop(&self) -> Option<Got> {
        if self.public_fail >= PUBLIC_FAIL_LIMIT {
            return Some(Got::Failed(match self.what {
                What::Justificante => {
                    "La Sede no da los justificantes ahora: se reintenta al volver a comprobar."
                        .into()
                }
                What::Documento => {
                    "La Sede no da los documentos ahora: vuelve a intentarlo más tarde.".into()
                }
            }));
        }
        if matches!(self.what, What::Documento) && self.public_none >= PUBLIC_NONE_LIMIT {
            return Some(Got::Failed(format!(
                "{PUBLIC_NONE_LIMIT} códigos seguidos no han dado ningún documento, así que no \
                 se han pedido más. Revisa que estén bien copiados o vuelve a intentarlo más \
                 tarde."
            )));
        }
        None
    }

    /// Lo que contestó la Sede pública: `Ok(None)`, sin PDF.
    fn public_answer(&mut self, r: Result<Option<Vec<u8>>, AeatError>) -> Got {
        let noun = self.what.noun();
        // En la descarga por CSV, una respuesta sin documento (o un «no existe» HTTP) es de ese
        // código: lo normal es que esté mal copiado o anulado. No es un fallo de la Sede.
        let no_document = match (&r, self.what) {
            (Ok(None), What::Documento) => true,
            (Err(AeatError::Status(s)), What::Documento) => (400..500).contains(s) && *s != 429,
            _ => false,
        };
        if no_document {
            self.public_none += 1;
            return Got::Failed(
                "La Sede no ha dado ningún documento con este código: revisa que esté bien \
                 copiado. Si está anulado, con el certificado se sabe cuál lo sustituye."
                    .into(),
            );
        }
        match r {
            Ok(Some(b)) => {
                self.public_fail = 0;
                self.public_none = 0;
                Got::Pdf(b)
            }
            Ok(None) => {
                self.public_fail += 1;
                Got::Failed(format!(
                    "No se pudo descargar {noun}: la AEAT no devolvió el PDF de ese código \
                     (puede que no exista)"
                ))
            }
            Err(e) => {
                if !matches!(e, AeatError::Network(_)) {
                    self.public_fail += 1;
                }
                Got::Failed(format!("No se pudo descargar {noun}: {e}"))
            }
        }
    }

    /// Una respuesta definitiva del servicio web. En «Comprobar lo presentado» los CSV los da la
    /// propia AEAT y existen: varias definitivas en la comprobación apuntan a que el servicio no
    /// sirve con este certificado, y se deja de usar. En la descarga por CSV los pega el
    /// usuario: un código que no existe o anulado es normal y no corta nada (y así los anulados
    /// siguen dando el que los sustituye); sólo cuentan los «no cotejable» seguidos.
    fn final_answer(
        &mut self,
        state: &'static str,
        message: &str,
        replacement: Option<String>,
    ) -> Got {
        let counts = match self.what {
            What::Justificante => true,
            What::Documento => state == "no_cotejable",
        };
        if counts {
            self.soap_final += 1;
            if self.soap_final >= SOAP_FINAL_LIMIT {
                self.soap = false;
            }
        }
        Got::Final(state, message.to_owned(), replacement)
    }
}

impl Run<'_> {
    /// El fichero de la declaración (visor de la Sede), del que salen las casillas exactas. Su
    /// página de «Error interno» es definitiva para ese documento; los fallos de red y las
    /// respuestas inesperadas cuentan para dejar de pedir (y se reintenta en la próxima
    /// comprobación).
    async fn file(&mut self, csv: &str, nif: &str) -> VisorGot {
        if self.visor_stop {
            return VisorGot::Unknown;
        }
        let Some(pc) = self.cotejo.public.as_ref() else {
            return VisorGot::Unknown;
        };
        match pc.declaration(csv, nif).await {
            Ok(cotejo::Visor::File(f)) => {
                self.visor_fail = 0;
                VisorGot::File(f)
            }
            Ok(cotejo::Visor::NoFile) => {
                self.visor_fail = 0;
                VisorGot::Final("sin_fichero")
            }
            Ok(cotejo::Visor::NotAvailable) => {
                self.visor_fail = 0;
                VisorGot::Final("no_disponible")
            }
            // Una página de mantenimiento o de límite de accesos: no se guarda nada, cuenta como
            // fallo y se reintenta en la próxima comprobación.
            Ok(cotejo::Visor::Unexpected) => {
                tracing::info!("visor de la declaración con una respuesta inesperada");
                self.visor_fail += 1;
                if self.visor_fail >= VISOR_FAIL_LIMIT {
                    self.visor_stop = true;
                }
                VisorGot::Unknown
            }
            Err(e) => {
                tracing::info!(error = %e, "visor de la declaración sin respuesta");
                self.visor_fail += 1;
                // El tope de peticiones de la Sede: no tiene sentido seguir.
                if self.visor_fail >= VISOR_FAIL_LIMIT || matches!(e, AeatError::Service(_)) {
                    self.visor_stop = true;
                }
                VisorGot::Unknown
            }
        }
    }

    /// Consulta un año en la AEAT, guarda lo que devuelve y descarga lo que falte.
    async fn year(&mut self, year: i32, progress: &(dyn Fn(Progress) + Sync)) -> CmdResult<()> {
        let now = OffsetDateTime::now_utc();
        let models = presentado_aeat::models_to_query(&*self.state.vault_of(&self.company)?, year)?;
        let mut answer = Answer {
            models: models.clone(),
            ..Answer::default()
        };
        let mut transport = false;
        for (i, m) in models.iter().enumerate() {
            progress(Progress {
                year,
                step: "consulta",
                done: i,
                total: models.len(),
            });
            match declarations::query_presented(
                &self.client,
                declarations::CONSULTA_PRODUCCION,
                &self.nif,
                &self.name,
                m,
                year,
            )
            .await
            {
                Ok(Ok(found)) => answer
                    .list
                    .extend(found.into_iter().map(|p| to_app(p, year))),
                Ok(Err(msg)) if declarations::no_declarations(&msg) => {
                    answer.empty.insert(m.clone(), msg);
                }
                Ok(Err(msg)) if declarations::not_available(&msg) => {
                    answer.unavailable.insert(m.clone(), msg);
                }
                Ok(Err(msg)) => answer.errors.push(format!("{m}: {msg}")),
                Err(e) => {
                    transport = true;
                    answer.errors.push(format!("{m}: {e}"));
                }
            }
        }
        if answer.list.is_empty()
            && !answer.errors.is_empty()
            && answer.errors.len() + answer.unavailable.len() == models.len()
        {
            let first = answer.errors.first().cloned().unwrap_or_default();
            return Err(CommandError::from(if transport {
                format!("No se pudo conectar con la AEAT para consultar {year}. {first}")
            } else {
                format!("La AEAT no dio las declaraciones de {year}. {first}")
            }));
        }
        let pending = {
            let v = self.state.vault_of(&self.company)?;
            presentado_aeat::record(&v, &self.nif, year, &answer, now)?;
            presentado_aeat::pending(&v, year)?
        };
        let total = pending.len();
        for (i, p) in pending.into_iter().enumerate() {
            progress(Progress {
                year,
                step: "descarga",
                done: i,
                total,
            });
            let got = if p.has_pdf {
                None
            } else {
                Some(self.cotejo.pdf(Some(&self.client), &p.csv).await)
            };
            // Sin documento en la AEAT (no encontrado, anulado…) no hay fichero que pedir; si el
            // visor ya contestó que no lo tiene, tampoco (las casillas, del PDF).
            let file = match &got {
                Some(Got::Final(..)) => VisorGot::Unknown,
                _ if p.visor_done => VisorGot::Stored,
                _ => self.file(&p.csv, &p.nif).await,
            };
            let download = match &got {
                None => Download::Stored,
                Some(Got::Pdf(b)) => Download::Pdf(b.as_slice()),
                Some(Got::Failed(m)) => Download::Failed(m.as_str()),
                Some(Got::Final(state, message, _)) => Download::Final {
                    state,
                    message: message.as_str(),
                },
            };
            // Si se ha cambiado de empresa, se para aquí: nada de lo de ésta va a la otra.
            let v = self.state.vault_of(&self.company)?;
            if let Err(e) = presentado_aeat::attach(&v, &p.csv, download, file.as_visor(), now) {
                tracing::warn!(error = %e, model = %p.model, period = %p.period, "justificante no guardado");
            }
        }
        progress(Progress {
            year,
            step: "descarga",
            done: total,
            total,
        });
        Ok(())
    }
}

/// Consulta la AEAT y descarga lo que falte; devuelve la tabla del año.
#[tauri::command]
pub async fn aeat_check_year(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    year: i32,
    fingerprint: String,
    other_holder: Option<bool>,
) -> CmdResult<Report> {
    let _running = Running::take(&app, Op::Check)?;
    let mut run = start(
        state.inner(),
        &fingerprint,
        other_holder.unwrap_or(false),
        &format!("Comprobar en la AEAT las declaraciones presentadas de {year}"),
    )?;
    let emit = |p: Progress| {
        let _ = app.emit("aeat-check-progress", p);
    };
    run.year(year, &emit).await?;
    let v = state.vault_of(&run.company)?;
    Ok(presentado_aeat::report(&v, year, crate::calendar::today())?)
}

/// Resultado de un año en «Comprobar varios años».
#[derive(Clone, Debug, Serialize)]
pub struct YearOutcome {
    pub year: i32,
    pub summary: Option<YearSummary>,
    /// Por qué no se pudo comprobar ese año.
    pub error: Option<String>,
}

/// Comprueba los años de `from` a `to` con una sola confirmación del certificado.
#[tauri::command]
pub async fn aeat_check_years(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    from: i32,
    to: i32,
    fingerprint: String,
    other_holder: Option<bool>,
) -> CmdResult<Vec<YearOutcome>> {
    if from > to || to - from >= MAX_YEARS || from < 2000 {
        return Err(CommandError::from(format!(
            "Elige entre 1 y {MAX_YEARS} años, desde el 2000."
        )));
    }
    let _running = Running::take(&app, Op::Check)?;
    let mut run = start(
        state.inner(),
        &fingerprint,
        other_holder.unwrap_or(false),
        &format!("Comprobar en la AEAT las declaraciones presentadas de {from} a {to}"),
    )?;
    let emit = |p: Progress| {
        let _ = app.emit("aeat-check-progress", p);
    };
    let today = crate::calendar::today();
    let mut out = Vec::new();
    // Del más reciente al más antiguo, con un tope de peticiones a la Sede por año: si algo va
    // mal en los antiguos, los recientes ya tienen sus justificantes.
    for year in (from..=to).rev() {
        run.cotejo.public = PublicClient::new().ok();
        let r = run.year(year, &emit).await;
        let outcome = match r {
            Ok(()) => {
                let v = state.vault_of(&run.company)?;
                let report = presentado_aeat::report(&v, year, today)?;
                YearOutcome {
                    year,
                    summary: Some(YearSummary::from(&report)),
                    error: None,
                }
            }
            Err(e) => {
                // Cambiar de empresa a medias corta todo; lo demás sólo ese año.
                state.vault_of(&run.company)?;
                YearOutcome {
                    year,
                    summary: None,
                    error: Some(e.message),
                }
            }
        };
        out.push(outcome);
    }
    out.reverse();
    Ok(out)
}

/// La empresa año a año con lo que tiene la AEAT (sin consultarla).
#[tauri::command(async)]
pub fn company_evolution(
    state: tauri::State<'_, AppState>,
) -> CmdResult<taxpain_app::evolucion::Evolucion> {
    let vault = state.vault()?;
    Ok(taxpain_app::evolucion::evolucion(
        &vault,
        crate::calendar::today(),
    )?)
}

/// Qué revisar con lo que tiene la AEAT y lo que sabe TaxPain, sin carpeta (los cruces entre
/// modelos, lo que falta y lo presentado fuera de plazo).
#[tauri::command(async)]
pub fn company_review(
    state: tauri::State<'_, AppState>,
) -> CmdResult<taxpain_app::revision::Revision> {
    let vault = state.vault()?;
    Ok(taxpain_app::revision::revisar_aeat(
        &vault,
        crate::calendar::today(),
    )?)
}

/// El resumen de todos los años comprobados (sin consultar la AEAT).
#[tauri::command(async)]
pub fn aeat_check_summaries(state: tauri::State<'_, AppState>) -> CmdResult<Vec<YearSummary>> {
    let vault = state.vault()?;
    Ok(presentado_aeat::summaries(
        &vault,
        crate::calendar::today(),
    )?)
}

/// La tabla del año con lo ya guardado (sin consultar la AEAT).
#[tauri::command(async)]
pub fn aeat_check_report(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<Report> {
    let vault = state.vault()?;
    Ok(presentado_aeat::report(
        &vault,
        year,
        crate::calendar::today(),
    )?)
}

/// Guarda en `dir` los justificantes en PDF del año; devuelve las rutas.
#[tauri::command(async)]
pub fn aeat_check_save(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    year: i32,
    dir: String,
) -> CmdResult<Vec<String>> {
    let dir = crate::salida::carpeta_permitida(&app, &dir)?;
    let vault = state.vault()?;
    Ok(presentado_aeat::export(&vault, year, &dir)?)
}

/// Guarda en `dir` los justificantes de los años de `from` a `to`, cada año en su subcarpeta.
#[tauri::command(async)]
pub fn aeat_check_save_years(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    from: i32,
    to: i32,
    dir: String,
) -> CmdResult<Vec<String>> {
    // Sin tope de años: `export_years` se salta los que no tienen justificantes.
    let this_year = crate::calendar::today().year();
    if from > to || from < 2000 || to > this_year + 1 {
        return Err(CommandError::from("Rango de años no válido."));
    }
    let dir = crate::salida::carpeta_permitida(&app, &dir)?;
    let years: Vec<i32> = (from..=to).collect();
    let vault = state.vault()?;
    Ok(presentado_aeat::export_years(&vault, &years, &dir)?)
}

/// Progreso de «Descargar de la AEAT por CSV» (evento `aeat-csv-progress`). Sin los códigos.
#[derive(Clone, Debug, Serialize)]
pub struct CsvProgress {
    pub done: usize,
    pub total: usize,
}

/// Lo que dio la AEAT para un CSV, como lo guarda `descarga_csv`.
fn fetched(got: Got) -> Fetched {
    match got {
        Got::Pdf(b) => Fetched::Pdf(b),
        Got::Failed(m) => Fetched::Failed(m),
        Got::Final("no_encontrado", ..) => Fetched::NotFound,
        Got::Final("anulado", _, replacement) => Fetched::Annulled(replacement),
        Got::Final(_, message, _) => Fetched::NotAvailable(message),
    }
}

/// Descarga de la AEAT los documentos de los CSV de `text` (lo que el usuario pega) y los guarda
/// en la subcarpeta «Documentos AEAT» de la carpeta de la empresa. Con `fingerprint`, primero el
/// servicio web oficial con ese certificado (Touch ID) y, si no da el documento, la Sede
/// pública; sin certificado, sólo la Sede pública. Sólo cotejo: no abre notificaciones ni pide
/// nada más a la AEAT. Una operación con la AEAT a la vez; la empresa se fija al empezar.
///
/// El certificado pasa la misma comprobación que en «Comprobar lo presentado»: uno de
/// representante de otra entidad no se usa, y uno personal que no es de la empresa sólo con
/// `other_holder` (confirmado por el usuario). Lo que ya está en «Documentos AEAT» (un PDF con
/// ese CSV en el nombre) no se vuelve a pedir a la AEAT.
#[tauri::command]
pub async fn aeat_csv_download(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    text: String,
    fingerprint: Option<String>,
    other_holder: Option<bool>,
) -> CmdResult<Vec<descarga_csv::Item>> {
    let csvs = descarga_csv::extract(&text)?;
    if csvs.is_empty() {
        return Err(CommandError::from(
            "No hay ningún código CSV en lo que has pegado. Un CSV son 16 letras y cifras, como \
             ABCD1234EFGH5678.",
        ));
    }
    let _running = Running::take(&app, Op::Csv)?;
    let (company, root, request) = {
        let (company, v) = state.vault_with_id()?;
        let root = descarga_csv::root(&v)?;
        // Antes del Touch ID y de pedir nada: que se pueda escribir en la subcarpeta.
        descarga_csv::prepare(&root)?;
        let request = match fingerprint.as_deref().map(str::trim) {
            Some(fp) if !fp.is_empty() => {
                let t = check_target(&v, fp)?;
                allow_cert(
                    &t,
                    other_holder.unwrap_or(false),
                    "descargar documentos con el certificado",
                )?;
                Some(v.prepare_identity(
                    fp,
                    "Descargar documentos de la AEAT por su CSV",
                    OffsetDateTime::now_utc(),
                )?)
            }
            _ => None,
        };
        (company, root, request)
    };
    let client = match request {
        Some(r) => {
            let identity = state.identity_of(&company, r)?;
            Some(AeatClient::with_platform_trust(
                &identity,
                hosts::PRODUCCION,
            )?)
        }
        None => None,
    };
    let emit = |done: usize| {
        let _ = app.emit(
            "aeat-csv-progress",
            CsvProgress {
                done,
                total: csvs.len(),
            },
        );
    };
    let mut cotejo = Cotejo::new(What::Documento);
    let mut out = Vec::with_capacity(csvs.len());
    for (i, csv) in csvs.iter().enumerate() {
        emit(i);
        // Ya en la carpeta: no se gasta un acceso al cotejo.
        if let Some(path) = descarga_csv::already(&root, csv) {
            out.push(descarga_csv::Item {
                csv: csv.clone(),
                state: descarga_csv::State::YaEstaba { path },
            });
            continue;
        }
        let got = cotejo.pdf(client.as_ref(), csv).await;
        // Si se ha cambiado de empresa, se para aquí: lo que queda no va a ninguna carpeta.
        state.vault_of(&company)?;
        out.push(descarga_csv::store(&root, csv, fetched(got)));
    }
    emit(csvs.len());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lo_que_dio_la_aeat_para_cada_csv() {
        let anulado = Got::Final("anulado", "x".into(), Some("ZZZZZZZZ99999999".into()));
        assert_eq!(
            fetched(anulado),
            Fetched::Annulled(Some("ZZZZZZZZ99999999".into()))
        );
        assert_eq!(
            fetched(Got::Final("no_encontrado", "x".into(), None)),
            Fetched::NotFound
        );
        assert_eq!(
            fetched(Got::Final("no_cotejable", "no se puede".into(), None)),
            Fetched::NotAvailable("no se puede".into())
        );
        assert_eq!(
            fetched(Got::Failed("red".into())),
            Fetched::Failed("red".into())
        );
        assert_eq!(
            fetched(Got::Pdf(b"%PDF-1.4".to_vec())),
            Fetched::Pdf(b"%PDF-1.4".to_vec())
        );
    }

    fn cotejo(what: What) -> Cotejo {
        Cotejo {
            public: None,
            soap: true,
            soap_final: 0,
            public_fail: 0,
            public_none: 0,
            what,
        }
    }

    fn failed(got: &Got) -> &str {
        match got {
            Got::Failed(m) => m,
            _ => panic!("no es un fallo"),
        }
    }

    #[test]
    fn codigos_mal_copiados_no_cortan_los_buenos_que_vienen_detras() {
        // Sin certificado: los tres primeros no existen o están anulados (la Sede contesta sin
        // PDF), y los siguientes se siguen pidiendo.
        let mut c = cotejo(What::Documento);
        for _ in 0..5 {
            assert!(c.public_stop().is_none());
            let got = c.public_answer(Ok(None));
            assert!(failed(&got).contains("bien copiado"), "{}", failed(&got));
        }
        // Un «no existe» HTTP tampoco es un fallo de la Sede.
        failed(&c.public_answer(Err(AeatError::Status(404))));
        assert!(c.public_stop().is_none());
        assert!(matches!(
            c.public_answer(Ok(Some(b"%PDF-1.4".to_vec()))),
            Got::Pdf(_)
        ));
        assert_eq!((c.public_fail, c.public_none), (0, 0));
        // Muchos seguidos sin documento: se deja de pedir, diciendo la causa.
        for _ in 0..PUBLIC_NONE_LIMIT {
            c.public_answer(Ok(None));
        }
        let stop = c.public_stop().unwrap();
        assert!(failed(&stop).contains("seguidos"), "{}", failed(&stop));
    }

    #[test]
    fn la_sede_que_falla_si_corta_la_tanda() {
        for what in [What::Documento, What::Justificante] {
            let mut c = cotejo(what);
            for _ in 0..PUBLIC_FAIL_LIMIT {
                assert!(c.public_stop().is_none());
                c.public_answer(Err(AeatError::Status(503)));
            }
            assert!(failed(&c.public_stop().unwrap()).contains("La Sede no da"));
            // Los cortes de red no cuentan.
            let mut c = cotejo(what);
            for _ in 0..10 {
                c.public_answer(Err(AeatError::Network("corte".into())));
            }
            assert!(c.public_stop().is_none());
        }
        // En «Comprobar lo presentado» los CSV existen: sin PDF es un fallo de la Sede.
        let mut c = cotejo(What::Justificante);
        for _ in 0..PUBLIC_FAIL_LIMIT {
            c.public_answer(Ok(None));
        }
        assert!(c.public_stop().is_some());
    }

    #[test]
    fn con_certificado_los_no_encontrados_y_anulados_no_cortan_el_servicio_web() {
        let mut c = cotejo(What::Documento);
        for _ in 0..5 {
            assert!(matches!(
                c.soap_answer(Ok(Outcome::NotFound)),
                Some(Got::Final("no_encontrado", ..))
            ));
        }
        // Los anulados siguen dando el que los sustituye.
        let got = c.soap_answer(Ok(Outcome::Annulled {
            replacement: Some("ZZZZZZZZ99999999".into()),
        }));
        assert!(matches!(got, Some(Got::Final("anulado", _, Some(_)))));
        assert!(c.soap);
        // Los «no cotejable» cuentan sólo seguidos.
        for _ in 0..SOAP_FINAL_LIMIT - 1 {
            c.soap_answer(Ok(Outcome::NotAvailable));
        }
        c.soap_answer(Ok(Outcome::Document(b"%PDF-1.4".to_vec())));
        for _ in 0..SOAP_FINAL_LIMIT - 1 {
            c.soap_answer(Ok(Outcome::NotAvailable));
        }
        assert!(c.soap);
        c.soap_answer(Ok(Outcome::NotAvailable));
        assert!(!c.soap);
        // En «Comprobar lo presentado», como siempre: tres definitivas y se deja.
        let mut c = cotejo(What::Justificante);
        for _ in 0..SOAP_FINAL_LIMIT {
            c.soap_answer(Ok(Outcome::NotFound));
        }
        assert!(!c.soap);
    }

    fn target(same: bool, other_entity: bool, company_nif: Option<&str>) -> CheckTarget {
        CheckTarget {
            company_nif: company_nif.map(Into::into),
            company_name: Some("EMPRESA DEMO SL".into()),
            cert_nif: if same { "B12345674" } else { "B98765431" }.into(),
            cert_name: "OTRA DEMO SL".into(),
            same,
            other_entity,
        }
    }

    #[test]
    fn el_certificado_de_otra_entidad_no_se_usa() {
        let e = allow_cert(&target(false, true, Some("B12345674")), true, "x").unwrap_err();
        assert!(
            e.message.contains("representante de B98765431"),
            "{}",
            e.message
        );
        // Uno personal que no es de la empresa, sólo confirmado.
        let t = target(false, false, Some("B12345674"));
        assert!(
            allow_cert(&t, false, "x")
                .unwrap_err()
                .message
                .contains("confirma")
        );
        assert_eq!(allow_cert(&t, true, "x").unwrap(), "B12345674");
        assert_eq!(
            allow_cert(&target(true, false, Some("B12345674")), false, "x").unwrap(),
            "B12345674"
        );
        let e = allow_cert(&target(true, false, None), false, "descargar").unwrap_err();
        assert!(
            e.message.contains("NIF de la empresa") && e.message.ends_with("antes de descargar."),
            "{}",
            e.message
        );
    }

    #[test]
    fn cada_operacion_dice_la_suya() {
        assert!(busy_message(Op::Csv as u8).contains("descarga"));
        assert!(busy_message(Op::Check as u8).contains("comprobación"));
        assert_eq!(Op::Csv.done_event(), "aeat-csv-done");
        assert_eq!(Op::Check.done_event(), "aeat-check-done");
    }

    #[test]
    fn la_misma_regla_de_csv_en_los_dos_crates() {
        for csv in [
            "0123456789ABCDEF",
            "ABCD1234EFGH5678",
            "QWERTYUIOPASDFGH",
            "1234567812345678",
            "abcd1234efgh5678",
            "ABCD1234EFGH567",
            "ABCD1234EFGH56789",
            "ABCD1234EFGH567Ñ",
            "ABCD-234EFGH5678",
            "",
        ] {
            assert_eq!(
                descarga_csv::valid_csv(csv),
                cotejo::valid_csv(csv),
                "{csv}"
            );
        }
    }

    #[test]
    fn el_ejercicio_de_la_aeat_o_el_pedido() {
        let p = declarations::Presented {
            year: " 2025 ".into(),
            model: "303".into(),
            period: "1T".into(),
            previous_receipt: Some(" ".into()),
            ..Default::default()
        };
        let a = to_app(p.clone(), 2024);
        assert_eq!(a.year, 2025);
        assert_eq!(a.previous_receipt, None);
        assert_eq!(
            to_app(
                declarations::Presented {
                    year: String::new(),
                    ..p
                },
                2024
            )
            .year,
            2024
        );
    }
}
