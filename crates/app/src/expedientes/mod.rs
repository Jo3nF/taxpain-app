//! Expedientes con la Administración (carencias G26 y G27): requerimientos, propuestas,
//! liquidaciones, sanciones, apremios y embargos de la AEAT o de otros buzones (DEHú, Seguridad
//! Social, comunidad autónoma, ayuntamiento) tecleados a mano o creados desde una notificación
//! del buzón de la AEAT o desde un documento reconocido en Documentos AEAT.
//!
//! - [`calculo`]: los plazos que abre la notificación ([`plazos`]: días hábiles con los
//!   festivos del perfil, meses de fecha a fecha), el más próximo, el recargo del periodo
//!   ejecutivo (art. 28 LGT) y lo que se pagaría de una sanción con sus reducciones (art. 188).
//! - [`escritos`]: borradores en RTF o PDF (contestación, alegaciones, reposición, aplazamiento o
//!   fraccionamiento, ampliación del plazo) con los datos del expediente y de la empresa y
//!   huecos «[…]» para lo que escribe el usuario. Nada se presenta ni se envía.
//! - [`certificados`]: certificados administrativos y apoderamientos con caducidad (G27).
//! - [`reminders_due`] y [`avisos`]: los avisos del Mac y del Panel.
//!
//! Las plantillas no sustituyen el asesoramiento: en una inspección o una sanción conviene un
//! asesor (la pantalla lo dice).

pub mod certificados;
pub mod escritos;
pub mod plazos;

use serde::{Deserialize, Serialize};
use taxpain_core::calendar::{HolidayCalendar, TaxpayerProfile, calendar_for, holidays};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{self, Connection, OptionalExtension, Row, params};
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;
use crate::notices::NoticeReminder;
pub use plazos::{Escenario, Plazo, Reducciones, Tipo};

/// Buzón u organismo del que viene la notificación.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Organismo {
    #[default]
    Aeat,
    /// Dirección Electrónica Habilitada única (cualquier organismo que notifique por ella).
    Dehu,
    SeguridadSocial,
    Autonomica,
    Ayuntamiento,
    Otro,
}

impl Organismo {
    pub fn code(self) -> &'static str {
        match self {
            Organismo::Aeat => "aeat",
            Organismo::Dehu => "dehu",
            Organismo::SeguridadSocial => "seguridad_social",
            Organismo::Autonomica => "autonomica",
            Organismo::Ayuntamiento => "ayuntamiento",
            Organismo::Otro => "otro",
        }
    }

    fn from_code(s: &str) -> Option<Self> {
        Some(match s {
            "aeat" => Organismo::Aeat,
            "dehu" => Organismo::Dehu,
            "seguridad_social" => Organismo::SeguridadSocial,
            "autonomica" => Organismo::Autonomica,
            "ayuntamiento" => Organismo::Ayuntamiento,
            "otro" => Organismo::Otro,
            _ => return None,
        })
    }

    pub fn nombre(self) -> &'static str {
        match self {
            Organismo::Aeat => "Agencia Tributaria (AEAT)",
            Organismo::Dehu => "DEHú (Dirección Electrónica Habilitada única)",
            Organismo::SeguridadSocial => "Seguridad Social (TGSS)",
            Organismo::Autonomica => "Agencia tributaria de la comunidad autónoma",
            Organismo::Ayuntamiento => "Ayuntamiento",
            Organismo::Otro => "Otro organismo",
        }
    }

    /// Se le aplican los recursos de la LGT (reposición y reclamación económico-administrativa).
    /// `TODO(VERIFY)`: los tributos cedidos o propios de una comunidad también se rigen por la
    /// LGT, con su tribunal económico-administrativo regional o autonómico.
    fn lgt(self) -> bool {
        matches!(self, Organismo::Aeat | Organismo::Autonomica)
    }
}

/// En qué punto está.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Estado {
    #[default]
    Abierto,
    /// Contestado o con alegaciones presentadas: el plazo del trámite ya no corre.
    Contestado,
    /// Recurrido: los plazos de recurso ya no corren.
    Recurrido,
    Cerrado,
}

impl Estado {
    fn code(self) -> &'static str {
        match self {
            Estado::Abierto => "abierto",
            Estado::Contestado => "contestado",
            Estado::Recurrido => "recurrido",
            Estado::Cerrado => "cerrado",
        }
    }

    fn from_code(s: &str) -> Option<Self> {
        Some(match s {
            "abierto" => Estado::Abierto,
            "contestado" => Estado::Contestado,
            "recurrido" => Estado::Recurrido,
            "cerrado" => Estado::Cerrado,
            _ => return None,
        })
    }
}

/// Un documento de la relación de los que se aportan.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocAportar {
    pub descripcion: String,
    /// Ya está reunido.
    #[serde(default)]
    pub listo: bool,
}

/// Datos de un expediente (los que se teclean o se leen de la notificación).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Expediente {
    #[serde(default)]
    pub id: i64,
    #[serde(default)]
    pub organismo: Organismo,
    /// Órgano o dependencia («Dependencia Regional de Inspección», «Gestión Tributaria»…).
    #[serde(default)]
    pub organo: String,
    /// Número de expediente o referencia.
    #[serde(default)]
    pub referencia: String,
    pub tipo: Tipo,
    #[serde(default)]
    pub asunto: String,
    /// Impuesto, modelo y ejercicio («IVA 2024», «Modelo 303 2T 2025»).
    #[serde(default)]
    pub concepto: String,
    /// Puesta a disposición en la sede (notificación electrónica sin abrir).
    #[serde(default)]
    pub puesto_el: Option<Date>,
    /// Fecha en que se notificó (se abrió o se recibió en papel).
    #[serde(default)]
    pub notificado_el: Option<Date>,
    /// Plazo en días hábiles que da el documento (si no, el general).
    #[serde(default)]
    pub plazo_dias: Option<u32>,
    /// Último día fijado a mano.
    #[serde(default)]
    pub vence_manual: Option<Date>,
    /// Deuda o cuota liquidada.
    #[serde(default)]
    pub importe: Option<Money>,
    /// Sanción sin reducciones.
    #[serde(default)]
    pub sancion: Option<Money>,
    /// Fin del periodo voluntario de pago de la deuda (de una providencia de apremio).
    #[serde(default)]
    pub fin_voluntario: Option<Date>,
    #[serde(default)]
    pub estado: Estado,
    /// Notificación del buzón de la AEAT de la que sale.
    #[serde(default)]
    pub notice_id: Option<String>,
    /// Documento de Documentos AEAT del que sale.
    #[serde(default)]
    pub document_id: Option<i64>,
    #[serde(default)]
    pub documentos: Vec<DocAportar>,
    #[serde(default)]
    pub notas: String,
}

/// Lo que se ha hecho en el expediente.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TipoActuacion {
    Contestacion,
    Alegaciones,
    RecursoReposicion,
    ReclamacionTea,
    Aplazamiento,
    Ampliacion,
    Pago,
    Otro,
}

impl TipoActuacion {
    fn code(self) -> &'static str {
        match self {
            TipoActuacion::Contestacion => "contestacion",
            TipoActuacion::Alegaciones => "alegaciones",
            TipoActuacion::RecursoReposicion => "recurso_reposicion",
            TipoActuacion::ReclamacionTea => "reclamacion_tea",
            TipoActuacion::Aplazamiento => "aplazamiento",
            TipoActuacion::Ampliacion => "ampliacion",
            TipoActuacion::Pago => "pago",
            TipoActuacion::Otro => "otro",
        }
    }

    fn from_code(s: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(s.to_owned())).ok()
    }

    /// Estado al que pasa un expediente abierto con esta actuación.
    fn estado(self) -> Option<Estado> {
        match self {
            TipoActuacion::Contestacion | TipoActuacion::Alegaciones => Some(Estado::Contestado),
            TipoActuacion::RecursoReposicion | TipoActuacion::ReclamacionTea => {
                Some(Estado::Recurrido)
            }
            TipoActuacion::Pago => Some(Estado::Cerrado),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Actuacion {
    #[serde(default)]
    pub id: i64,
    pub fecha: Date,
    pub tipo: TipoActuacion,
    /// Número de registro o justificante.
    #[serde(default)]
    pub registro: String,
    #[serde(default)]
    pub descripcion: String,
}

/// Plazos y cálculos de un expediente.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Calculo {
    /// Fecha desde la que corren los plazos.
    pub notificado: Option<Date>,
    /// Sin fecha de notificación: se da por notificada a los diez días de la puesta a
    /// disposición.
    pub presunta: bool,
    pub plazos: Vec<Plazo>,
    /// El próximo plazo que corre (según el estado).
    pub proximo: Option<Plazo>,
    /// Días naturales que quedan para el próximo.
    pub dias: Option<i64>,
    /// Hay plazos terminados sin actuación que los cierre (abierto y sin próximo).
    pub vencido: Option<Plazo>,
    /// Se han contado los festivos oficiales del año y los de la comunidad del perfil.
    pub festivos_completos: bool,
    pub recargo: Vec<Escenario>,
    pub reducciones: Option<Reducciones>,
    pub avisos: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Vista {
    pub expediente: Expediente,
    pub actuaciones: Vec<Actuacion>,
    pub calculo: Calculo,
}

fn other(e: impl ToString) -> AppError {
    AppError::Other(e.to_string())
}

pub(crate) fn fecha(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

fn parse_fecha(s: &str) -> Option<Date> {
    Date::parse(
        s.get(..10)?,
        time::macros::format_description!("[year]-[month]-[day]"),
    )
    .ok()
}

fn perfil(v: &Vault) -> Result<Option<TaxpayerProfile>> {
    Ok(v.setting::<TaxpayerProfile>("taxpayer_profile")?)
}

fn calendario(perfil: Option<&TaxpayerProfile>, year: i32) -> (HolidayCalendar, bool) {
    calendar_for(
        year,
        perfil.and_then(|p| p.ccaa),
        perfil.map_or(&[][..], |p| &p.festivos_locales),
    )
}

/// Claves de los plazos que ya no corren en cada estado.
fn cerrado_por(estado: Estado, clave: &str) -> bool {
    match estado {
        Estado::Abierto => false,
        Estado::Contestado => matches!(clave, "contestar" | "alegar" | "pedir_ampliacion"),
        Estado::Recurrido => matches!(clave, "reposicion" | "reclamacion"),
        Estado::Cerrado => true,
    }
}

/// Plazos y cálculos del expediente `e` a día `hoy` con los festivos del `perfil`.
pub fn calculo(e: &Expediente, hoy: Date, perfil: Option<&TaxpayerProfile>) -> Calculo {
    let mut c = Calculo::default();
    let (notificado, presunta) = match (e.notificado_el, e.puesto_el) {
        (Some(n), _) => (n, false),
        (None, Some(p)) => (plazos::notificacion_presunta(p), true),
        (None, None) => {
            c.avisos
                .push("Falta la fecha de notificación (o la de puesta a disposición).".into());
            return c;
        }
    };
    c.notificado = Some(notificado);
    c.presunta = presunta;
    if presunta {
        c.avisos.push(format!(
            "Sin fecha de notificación: si no la abres, se entiende notificada el {} (diez días \
             naturales desde la puesta a disposición, art. 43.2 Ley 39/2015) y los plazos se \
             cuentan desde entonces. Si la abres antes, anota el día en que la abriste.",
            fecha(notificado)
        ));
    }
    let (cal, cargados) = calendario(perfil, notificado.year());
    let mut ps = plazos::plazos(&cal, e.tipo, notificado, e.plazo_dias, e.vence_manual);
    if e.tipo.recurrible() && !e.organismo.lgt() {
        // Fuera de la LGT el recurso es el que diga el pie del documento.
        ps.retain(|p| p.clave != "reclamacion");
        for p in ps.iter_mut().filter(|p| p.clave == "reposicion") {
            let (titulo, norma) = match e.organismo {
                Organismo::Ayuntamiento => (
                    "Recurso de reposición (obligatorio en los tributos locales)",
                    "Art. 14.2 TRLRHL (RDLeg. 2/2004). TODO(VERIFY)",
                ),
                Organismo::SeguridadSocial => (
                    "Recurso de alzada o el que indique el documento",
                    "Arts. 121 y 122 Ley 39/2015; RD 1415/2004. TODO(VERIFY)",
                ),
                _ => (
                    "Recurso (el que indique el pie de recursos del documento)",
                    "Arts. 30, 112 y 124 Ley 39/2015. TODO(VERIFY)",
                ),
            };
            p.titulo = titulo.into();
            p.norma = norma;
        }
    }
    let todos_cargados = cargados
        && ps
            .iter()
            .all(|p| holidays::national(p.vence.year()).is_some());
    c.festivos_completos = todos_cargados && perfil.is_some_and(|p| p.ccaa.is_some());
    if !todos_cargados {
        c.avisos.push(
            "Faltan los festivos oficiales de algún año del plazo: el último día podría ser \
             otro."
                .into(),
        );
    } else if !c.festivos_completos {
        c.avisos.push(
            "Sin la comunidad autónoma en el perfil fiscal sólo se han contado los festivos \
             nacionales: añádela (y los festivos locales) en Calendario → Editar perfil."
                .into(),
        );
    }
    if let Some(p) = perfil
        && p.ccaa.is_some()
        && !p
            .festivos_locales
            .iter()
            .any(|d| d.year() == notificado.year())
    {
        c.avisos.push(format!(
            "No hay festivos locales de {} en el perfil: si el plazo cruza uno, acaba un día \
             hábil después (art. 30.6 Ley 39/2015).",
            notificado.year()
        ));
    }
    let mut vivos: Vec<&Plazo> = ps
        .iter()
        .filter(|p| !cerrado_por(e.estado, p.clave))
        .collect();
    vivos.sort_by_key(|p| p.vence);
    c.proximo = vivos.iter().find(|p| p.vence >= hoy).map(|p| (*p).clone());
    c.dias = c.proximo.as_ref().map(|p| (p.vence - hoy).whole_days());
    if c.proximo.is_none() && e.estado == Estado::Abierto {
        c.vencido = vivos.last().map(|p| (*p).clone());
    }
    // Recargos del periodo ejecutivo.
    let deuda = match e.tipo {
        Tipo::Liquidacion | Tipo::ProvidenciaApremio => e.importe,
        Tipo::AcuerdoSancionador => e.importe.or(e.sancion),
        _ => None,
    };
    if let Some(deuda) = deuda.filter(|d| *d > Money::ZERO)
        && e.estado != Estado::Cerrado
    {
        match e.tipo {
            Tipo::ProvidenciaApremio => {
                let fin = ps.iter().find(|p| p.clave == "pago").map(|p| p.vence);
                match (e.fin_voluntario, fin) {
                    (Some(fv), Some(fa)) => {
                        c.recargo =
                            plazos::recargo_ejecutivo(deuda, fv, Some((notificado, fa)), hoy);
                    }
                    _ => c.avisos.push(
                        "Teclea el último día del periodo voluntario de la deuda (lo dice la \
                         providencia) para calcular los intereses de demora del recargo del 20 %."
                            .into(),
                    ),
                }
            }
            _ => {
                let fin = e
                    .fin_voluntario
                    .or_else(|| ps.iter().find(|p| p.clave == "pago").map(|p| p.vence));
                if let Some(fv) = fin {
                    c.recargo = plazos::recargo_ejecutivo(deuda, fv, None, hoy);
                }
            }
        }
    }
    if let Some(s) = e.sancion.filter(|s| *s > Money::ZERO)
        && matches!(e.tipo, Tipo::PropuestaSancion | Tipo::AcuerdoSancionador)
    {
        c.reducciones = Some(plazos::reducciones(s));
    }
    // Avisos del tipo.
    match e.tipo {
        Tipo::Liquidacion => c.avisos.push(
            "Recurrir no suspende el pago: para no pagar mientras se resuelve hay que pedir la \
             suspensión con garantía (art. 224 LGT en reposición, art. 233 en la reclamación). \
             Si no puedes pagar en plazo, pide el aplazamiento o fraccionamiento antes de que \
             acabe el periodo voluntario (art. 65 LGT)."
                .into(),
        ),
        Tipo::AcuerdoSancionador => c.avisos.push(
            "Si recurres la sanción, queda suspendida sin garantía hasta que sea firme en vía \
             administrativa (art. 212.3 LGT), pero pierdes la reducción por pronto pago."
                .into(),
        ),
        Tipo::ProvidenciaApremio => c.avisos.push(
            "Contra la providencia de apremio sólo caben los motivos del art. 167.3 LGT \
             (extinción o prescripción de la deuda, aplazamiento pedido en periodo voluntario, \
             falta de notificación de la liquidación, anulación de la liquidación o error en la \
             identificación)."
                .into(),
        ),
        Tipo::DiligenciaEmbargo => c.avisos.push(
            "Contra la diligencia de embargo sólo caben los motivos del art. 170.3 LGT \
             (extinción de la deuda, falta de notificación de la providencia, incumplimiento de \
             las normas del embargo o suspensión del procedimiento)."
                .into(),
        ),
        _ => {}
    }
    if e.tipo.recurrible() && e.organismo.lgt() {
        c.avisos.push(
            "El recurso de reposición y la reclamación económico-administrativa no pueden ir a \
             la vez: si recurres en reposición, la reclamación se presenta cuando se resuelva o \
             se entienda desestimado (art. 222.2 LGT)."
                .into(),
        );
    }
    if !e.organismo.lgt() {
        c.avisos.push(format!(
            "{}: los plazos se cuentan con las reglas generales de la Ley 39/2015; sigue lo que \
             diga el propio documento (TODO(VERIFY) la norma de cada organismo).",
            e.organismo.nombre()
        ));
    }
    c.plazos = ps;
    c
}

// ---------- Almacén ----------

const COLUMNAS: &str = "id, organismo, organo, referencia, tipo, asunto, concepto, puesto_el, \
     notificado_el, plazo_dias, vence_manual, importe, sancion, fin_voluntario, estado, \
     notice_id, document_id, documentos, notas";

fn de_fila(r: &Row<'_>) -> rusqlite::Result<Expediente> {
    let opt_fecha = |i: usize| -> rusqlite::Result<Option<Date>> {
        Ok(r.get::<_, Option<String>>(i)?
            .as_deref()
            .and_then(parse_fecha))
    };
    let documentos: String = r.get(17)?;
    Ok(Expediente {
        id: r.get(0)?,
        organismo: Organismo::from_code(&r.get::<_, String>(1)?).unwrap_or_default(),
        organo: r.get(2)?,
        referencia: r.get(3)?,
        tipo: Tipo::from_code(&r.get::<_, String>(4)?).unwrap_or(Tipo::Otro),
        asunto: r.get(5)?,
        concepto: r.get(6)?,
        puesto_el: opt_fecha(7)?,
        notificado_el: opt_fecha(8)?,
        plazo_dias: r
            .get::<_, Option<i64>>(9)?
            .and_then(|n| u32::try_from(n).ok()),
        vence_manual: opt_fecha(10)?,
        importe: r.get::<_, Option<i64>>(11)?.map(Money::from_cents),
        sancion: r.get::<_, Option<i64>>(12)?.map(Money::from_cents),
        fin_voluntario: opt_fecha(13)?,
        estado: Estado::from_code(&r.get::<_, String>(14)?).unwrap_or_default(),
        notice_id: r.get(15)?,
        document_id: r.get(16)?,
        documentos: serde_json::from_str(&documentos).unwrap_or_default(),
        notas: r.get(18)?,
    })
}

fn cargar(conn: &Connection, id: i64) -> Result<Expediente> {
    conn.query_row(
        &format!("SELECT {COLUMNAS} FROM expediente WHERE id = ?1"),
        [id],
        de_fila,
    )
    .optional()?
    .ok_or(AppError::NotFound)
}

fn actuaciones(conn: &Connection, id: i64) -> Result<Vec<Actuacion>> {
    let mut stmt = conn.prepare(
        "SELECT id, fecha, tipo, registro, descripcion FROM expediente_actuacion
         WHERE expediente_id = ?1 ORDER BY fecha, id",
    )?;
    let rows = stmt.query_map([id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, f, t, registro, descripcion) = row?;
        let (Some(fecha), Some(tipo)) = (parse_fecha(&f), TipoActuacion::from_code(&t)) else {
            continue;
        };
        out.push(Actuacion {
            id,
            fecha,
            tipo,
            registro,
            descripcion,
        });
    }
    Ok(out)
}

/// Comprueba y limpia los datos antes de guardarlos.
fn normalizar(e: &Expediente) -> Result<Expediente> {
    let mut e = e.clone();
    for s in [
        &mut e.organo,
        &mut e.referencia,
        &mut e.asunto,
        &mut e.concepto,
        &mut e.notas,
    ] {
        *s = s.trim().to_owned();
    }
    e.documentos.retain(|d| !d.descripcion.trim().is_empty());
    for d in &mut e.documentos {
        d.descripcion = d.descripcion.trim().to_owned();
    }
    if e.notificado_el.is_none() && e.puesto_el.is_none() {
        return Err(other(
            "falta la fecha de notificación (o la de puesta a disposición en la sede)",
        ));
    }
    if let (Some(n), Some(p)) = (e.notificado_el, e.puesto_el)
        && n < p
    {
        return Err(other(
            "la fecha de notificación no puede ser anterior a la de puesta a disposición",
        ));
    }
    if let Some(n) = e.plazo_dias
        && !(1..=365).contains(&n)
    {
        return Err(other("el plazo en días tiene que estar entre 1 y 365"));
    }
    if let (Some(v), Some(n)) = (e.vence_manual, e.notificado_el.or(e.puesto_el))
        && v < n
    {
        return Err(other(
            "el último día no puede ser anterior a la notificación",
        ));
    }
    for m in [e.importe, e.sancion].into_iter().flatten() {
        if m < Money::ZERO {
            return Err(other("los importes no pueden ser negativos"));
        }
    }
    if e.asunto.is_empty() && e.referencia.is_empty() {
        e.asunto = e.tipo.nombre().to_owned();
    }
    Ok(e)
}

fn opt(d: Option<Date>) -> Option<String> {
    d.map(|d| d.to_string())
}

fn guardar(conn: &Connection, e: &Expediente, id: Option<i64>, now: OffsetDateTime) -> Result<i64> {
    let documentos = serde_json::to_string(&e.documentos).map_err(other)?;
    let p = params![
        e.organismo.code(),
        e.organo,
        e.referencia,
        e.tipo.code(),
        e.asunto,
        e.concepto,
        opt(e.puesto_el),
        opt(e.notificado_el),
        e.plazo_dias.map(i64::from),
        opt(e.vence_manual),
        e.importe.map(Money::cents),
        e.sancion.map(Money::cents),
        opt(e.fin_voluntario),
        e.estado.code(),
        e.notice_id,
        e.document_id,
        documentos,
        e.notas,
        now_str(now),
    ];
    match id {
        None => {
            conn.execute(
                "INSERT INTO expediente(organismo, organo, referencia, tipo, asunto, concepto,
                   puesto_el, notificado_el, plazo_dias, vence_manual, importe, sancion,
                   fin_voluntario, estado, notice_id, document_id, documentos, notas,
                   created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
                   ?17, ?18, ?19, ?19)",
                p,
            )?;
            Ok(conn.last_insert_rowid())
        }
        Some(id) => {
            let n = conn.execute(
                &format!(
                    "UPDATE expediente SET organismo = ?1, organo = ?2, referencia = ?3,
                       tipo = ?4, asunto = ?5, concepto = ?6, puesto_el = ?7,
                       notificado_el = ?8, plazo_dias = ?9, vence_manual = ?10, importe = ?11,
                       sancion = ?12, fin_voluntario = ?13, estado = ?14, notice_id = ?15,
                       document_id = ?16, documentos = ?17, notas = ?18, updated_at = ?19
                     WHERE id = {id}"
                ),
                p,
            )?;
            if n == 0 {
                return Err(AppError::NotFound);
            }
            Ok(id)
        }
    }
}

fn resumen(e: &Expediente) -> serde_json::Value {
    serde_json::json!({
        "organismo": e.organismo.code(),
        "tipo": e.tipo.code(),
        "referencia": e.referencia,
        "notificado_el": opt(e.notificado_el),
        "puesto_el": opt(e.puesto_el),
        "estado": e.estado.code(),
    })
}

/// Expedientes con sus plazos: primero los que tienen un plazo que corre (el más cercano
/// primero), luego los demás y los cerrados al final.
pub fn list(v: &Vault, hoy: Date) -> Result<Vec<Vista>> {
    let conn = v.connection();
    let perfil = perfil(v)?;
    let mut stmt = conn.prepare(&format!("SELECT {COLUMNAS} FROM expediente ORDER BY id"))?;
    let es = stmt
        .query_map([], de_fila)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::new();
    for e in es {
        let calculo = calculo(&e, hoy, perfil.as_ref());
        out.push(Vista {
            actuaciones: actuaciones(conn, e.id)?,
            expediente: e,
            calculo,
        });
    }
    out.sort_by_key(|x| {
        (
            x.expediente.estado == Estado::Cerrado,
            x.calculo.proximo.is_none(),
            x.calculo.proximo.as_ref().map(|p| p.vence),
            std::cmp::Reverse(x.expediente.notificado_el.or(x.expediente.puesto_el)),
        )
    });
    Ok(out)
}

/// Plazos de un expediente sin guardarlo (para el formulario).
pub fn preview(v: &Vault, e: &Expediente, hoy: Date) -> Result<Calculo> {
    Ok(calculo(e, hoy, perfil(v)?.as_ref()))
}

pub fn create(v: &Vault, e: &Expediente, now: OffsetDateTime) -> Result<i64> {
    let e = normalizar(e)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let id = guardar(&tx, &e, None, now)?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "expediente.create",
        &id.to_string(),
        &resumen(&e),
    )?;
    tx.commit()?;
    Ok(id)
}

pub fn update(v: &Vault, id: i64, e: &Expediente, now: OffsetDateTime) -> Result<()> {
    let e = normalizar(e)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    guardar(&tx, &e, Some(id), now)?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "expediente.update",
        &id.to_string(),
        &resumen(&e),
    )?;
    tx.commit()?;
    Ok(())
}

/// Nombre del expediente (para el aviso de Touch ID).
pub fn name(v: &Vault, id: i64) -> Result<Option<String>> {
    let e = match cargar(v.connection(), id) {
        Ok(e) => e,
        Err(AppError::NotFound) => return Ok(None),
        Err(e) => return Err(e),
    };
    Ok(Some(titulo(&e)))
}

/// «Requerimiento de información 2026GRC12345678X».
pub fn titulo(e: &Expediente) -> String {
    if e.referencia.is_empty() {
        format!("{} · {}", e.tipo.nombre(), e.asunto)
    } else {
        format!("{} {}", e.tipo.nombre(), e.referencia)
    }
}

/// Borra el expediente y sus actuaciones (la notificación y los documentos no se tocan).
pub fn delete(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let e = cargar(conn, id)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM expediente_actuacion WHERE expediente_id = ?1",
        [id],
    )?;
    tx.execute("DELETE FROM expediente WHERE id = ?1", [id])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "expediente.delete",
        &id.to_string(),
        &resumen(&e),
    )?;
    tx.commit()?;
    Ok(())
}

/// Anota una actuación. Si el expediente está abierto, pasa a contestado (contestación o
/// alegaciones), recurrido (reposición o reclamación) o cerrado (pago).
pub fn add_actuacion(v: &Vault, id: i64, a: &Actuacion, now: OffsetDateTime) -> Result<i64> {
    let conn = v.connection();
    let e = cargar(conn, id)?;
    if let Some(n) = e.notificado_el.or(e.puesto_el)
        && a.fecha < n
    {
        return Err(other(
            "la actuación no puede ser anterior a la notificación",
        ));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO expediente_actuacion(expediente_id, fecha, tipo, registro, descripcion,
           created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            id,
            a.fecha.to_string(),
            a.tipo.code(),
            a.registro.trim(),
            a.descripcion.trim(),
            now_str(now)
        ],
    )?;
    let aid = tx.last_insert_rowid();
    let nuevo = a.tipo.estado().filter(|_| e.estado == Estado::Abierto);
    if let Some(s) = nuevo {
        tx.execute(
            "UPDATE expediente SET estado = ?1, updated_at = ?2 WHERE id = ?3",
            params![s.code(), now_str(now), id],
        )?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "expediente.actuacion",
        &id.to_string(),
        &serde_json::json!({
            "tipo": a.tipo.code(),
            "fecha": a.fecha.to_string(),
            "registro": a.registro.trim(),
            "estado": nuevo.map(Estado::code),
        }),
    )?;
    tx.commit()?;
    Ok(aid)
}

/// Quita una actuación (el estado del expediente no cambia: se cambia a mano).
pub fn delete_actuacion(
    v: &Vault,
    actuacion: i64,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let (exp, tipo, f): (i64, String, String) = conn
        .query_row(
            "SELECT expediente_id, tipo, fecha FROM expediente_actuacion WHERE id = ?1",
            [actuacion],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM expediente_actuacion WHERE id = ?1",
        [actuacion],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "expediente.actuacion_delete",
        &exp.to_string(),
        &serde_json::json!({ "tipo": tipo, "fecha": f }),
    )?;
    tx.commit()?;
    Ok(())
}

// ---------- Desde una notificación o un documento ----------

/// Tipo que se adivina del texto de una notificación o de un documento.
pub fn adivinar_tipo(texto: &str) -> Tipo {
    let t = crate::documentos::fold(texto);
    let hay = |xs: &[&str]| xs.iter().any(|x| t.contains(x));
    if hay(&["diligencia de embargo"]) {
        Tipo::DiligenciaEmbargo
    } else if hay(&["providencia de apremio"]) {
        Tipo::ProvidenciaApremio
    } else if hay(&[
        "acuerdo de imposicion de sancion",
        "resolucion sancionadora",
        "acuerdo de resolucion del procedimiento sancionador",
        "imposicion de sancion",
    ]) {
        Tipo::AcuerdoSancionador
    } else if hay(&[
        "propuesta de imposicion de sancion",
        "propuesta de resolucion del procedimiento sancionador",
        "propuesta de sancion",
        "inicio de expediente sancionador",
        "inicio del procedimiento sancionador",
    ]) {
        Tipo::PropuestaSancion
    } else if hay(&["propuesta de liquidacion", "propuesta de regularizacion"]) {
        Tipo::PropuestaLiquidacion
    } else if hay(&["tramite de audiencia", "puesta de manifiesto"]) {
        Tipo::Audiencia
    } else if hay(&["requerimiento", "se le requiere"]) {
        Tipo::Requerimiento
    } else if hay(&["liquidacion"]) {
        Tipo::Liquidacion
    } else {
        Tipo::Otro
    }
}

/// Borrador de expediente (sin guardar) con los datos de una notificación del buzón de la AEAT.
pub fn desde_notificacion(v: &Vault, notice_id: &str) -> Result<Expediente> {
    let n = crate::notices::list(v)?
        .into_iter()
        .find(|n| n.id == notice_id)
        .ok_or(AppError::NotFound)?;
    let meta = |k: &str| {
        n.metadata
            .iter()
            .find(|(kk, _)| kk == k)
            .map(|(_, v)| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    let texto = [
        Some(n.subject.clone()),
        meta("Concepto"),
        meta("DescripcionProcedimiento"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" · ");
    let concepto = [
        meta("Modelo").map(|m| format!("Modelo {m}")),
        meta("Ejercicio"),
        meta("Periodo"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    // La AEAT manda el importe en céntimos.
    let importe = meta("Importe")
        .and_then(|i| i.parse::<i64>().ok())
        .filter(|c| *c > 0)
        .map(Money::from_cents);
    let accedida = meta("FechaAcceso")
        .as_deref()
        .and_then(parse_fecha)
        .or_else(|| n.accessed_at.as_deref().and_then(parse_fecha));
    let pendiente = n.is_pending_notification();
    Ok(Expediente {
        organismo: Organismo::Aeat,
        organo: meta("NombreOficinaRemitente").unwrap_or_default(),
        tipo: adivinar_tipo(&texto),
        asunto: n.subject.clone(),
        concepto: if concepto.is_empty() {
            meta("Concepto").unwrap_or_default()
        } else {
            concepto
        },
        puesto_el: parse_fecha(&n.available_at),
        notificado_el: if pendiente || n.kind != "N" {
            None
        } else {
            accedida
        },
        importe,
        notice_id: Some(n.id.clone()),
        ..Expediente::default()
    })
}

/// Borrador de expediente (sin guardar) con lo que se lee de un documento de Documentos AEAT
/// (requerimiento, liquidación, resolución o notificación).
pub fn desde_documento(v: &Vault, document_id: i64) -> Result<Expediente> {
    let doc = crate::documentos::list(v)?
        .into_iter()
        .find(|d| d.id == document_id)
        .ok_or(AppError::NotFound)?;
    let texto = crate::documentos::text(v, document_id)?;
    let o = crate::oficiales::leer(&texto);
    let mut tipo = adivinar_tipo(texto.get(..texto.len().min(1500)).unwrap_or(&texto));
    if tipo == Tipo::Otro {
        tipo = match doc.kind {
            crate::documentos::DocKind::RequerimientoAeat => Tipo::Requerimiento,
            crate::documentos::DocKind::LiquidacionAeat => Tipo::Liquidacion,
            _ => Tipo::Otro,
        };
    }
    let mut concepto: Vec<String> = o.conceptos.clone();
    if let Some((m, y, p)) = &o.modelo {
        concepto.push(format!("Modelo {m} {p} {y}"));
    }
    concepto.extend(o.ejercicios.iter().map(i32::to_string));
    let organismo = if crate::oficiales::de_la_aeat(&texto) {
        Organismo::Aeat
    } else if crate::oficiales::de_otra_administracion(&texto) {
        Organismo::Autonomica
    } else {
        Organismo::Otro
    };
    Ok(Expediente {
        organismo,
        organo: o.organo.clone().unwrap_or_default(),
        referencia: o
            .expediente
            .clone()
            .or_else(|| o.referencia.clone())
            .unwrap_or_default(),
        tipo,
        asunto: doc.title.clone(),
        concepto: concepto.join(", "),
        puesto_el: o.puesta,
        notificado_el: o.accedida,
        plazo_dias: o.plazo_dias,
        vence_manual: o.nuevo_plazo,
        importe: o.importe,
        document_id: Some(document_id),
        ..Expediente::default()
    })
}

// ---------- Avisos ----------

/// Días antes del último día de un plazo en que avisa el Mac.
pub const AVISOS_PLAZO: [i64; 4] = [7, 3, 1, 0];

/// Avisos del Mac de hoy: plazos de expedientes que acaban en 7, 3 o 1 días u hoy, y
/// certificados o apoderamientos que caducan ([`certificados::reminders_due`]).
pub fn reminders_due(v: &Vault, hoy: Date) -> Result<Vec<NoticeReminder>> {
    let mut out = Vec::new();
    for x in list(v, hoy)? {
        let (Some(p), Some(d)) = (&x.calculo.proximo, x.calculo.dias) else {
            continue;
        };
        if !AVISOS_PLAZO.contains(&d) {
            continue;
        }
        let cuando = match d {
            0 => "hoy es el último día".to_owned(),
            1 => "mañana es el último día".to_owned(),
            n => format!("quedan {n} días (hasta el {})", fecha(p.vence)),
        };
        out.push(NoticeReminder {
            key: format!("expediente|{}|{}|{}|{d}", x.expediente.id, p.clave, p.vence),
            title: format!("Plazo: {}", p.titulo),
            body: format!("{}: {cuando}.", titulo(&x.expediente)),
        });
    }
    out.extend(certificados::reminders_due(v, hoy)?);
    Ok(out)
}

/// Aviso para el Panel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Aviso {
    pub texto: String,
    pub fecha: Date,
    /// Días naturales que quedan (negativo: ya pasó).
    pub dias: i64,
    pub urgente: bool,
}

/// Días antes en que el Panel enseña un plazo.
pub const PANEL_DIAS: i64 = 15;

/// Avisos del Panel: plazos que acaban en 15 días o menos, plazos terminados en expedientes
/// abiertos (los últimos 30 días) y certificados que caducan.
pub fn avisos(v: &Vault, hoy: Date) -> Result<Vec<Aviso>> {
    let mut out = Vec::new();
    for x in list(v, hoy)? {
        if let (Some(p), Some(d)) = (&x.calculo.proximo, x.calculo.dias) {
            if d <= PANEL_DIAS {
                out.push(Aviso {
                    texto: format!("{}: {}", titulo(&x.expediente), p.titulo),
                    fecha: p.vence,
                    dias: d,
                    urgente: d <= 3,
                });
            }
        } else if let Some(p) = &x.calculo.vencido {
            let d = (p.vence - hoy).whole_days();
            if d >= -30 {
                out.push(Aviso {
                    texto: format!(
                        "{}: el plazo terminó (anota si se contestó o recurrió)",
                        titulo(&x.expediente)
                    ),
                    fecha: p.vence,
                    dias: d,
                    urgente: true,
                });
            }
        }
    }
    out.extend(certificados::avisos(v, hoy)?);
    out.sort_by_key(|a| a.fecha);
    Ok(out)
}
