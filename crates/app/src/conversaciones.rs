//! Conversaciones con la IA, guardadas en el almacén de la empresa: generales (Asistente) o sobre
//! un tema de TaxPain (una recomendación de «Ahorrar impuestos», el plan de ahorro del año, un
//! hallazgo de «Qué revisar» o de la revisión de la carpeta, o el borrador de un modelo), con lo
//! que el usuario añade (datos escritos y el texto de documentos; nunca el fichero).
//!
//! - Una conversación por tema y ejercicio; generales, las que se quieran (hasta
//!   [`MAX_CONVERSATIONS`] entre todas).
//! - Los datos del tema se rehacen con los de ahora en cada pregunta y lo último que se envió se
//!   guarda (`snapshot`): sirve cuando el ejercicio ya no se puede calcular y para avisar si los
//!   datos han cambiado desde la última pregunta.
//! - La pregunta y su respuesta se guardan juntas, sólo cuando llega la respuesta: si algo falla
//!   no queda nada a medias.
//! - Lo que sale va anonimizado ([`taxpain_assistant::prepare_conversation`]) y queda en la
//!   auditoría antes de enviarlo, sin su contenido.

use std::collections::BTreeMap;
use std::ops::Deref;

use serde::{Deserialize, Serialize};
use taxpain_assistant::{
    Answered, AssistantError, Client, ConversationInput, LocalTools, Material, Message, Outgoing,
    Provider, Role,
};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, Row, params};
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};
use zeroize::Zeroizing;

use crate::error::{AppError, Result};
use crate::revision::Origen;
use crate::{accounting, ahorro, assistant, invoicing, models, revision};

/// Conversaciones guardadas por empresa, entre todas (al llegar, no se empieza otra: nunca se
/// borra ninguna sin que lo pida el usuario).
pub const MAX_CONVERSATIONS: usize = 100;
/// Preguntas por conversación.
pub const MAX_QUESTIONS: usize = 100;
pub const MAX_QUESTION_CHARS: usize = 4_000;
/// Lo que se envía del historial en cada pregunta (las intervenciones más recientes).
pub const HISTORY_MESSAGES: usize = 30;
pub const HISTORY_CHARS: usize = 30_000;
/// Lo que puede añadir el usuario a una conversación.
pub const MAX_MATERIALS: usize = 20;
pub const MAX_FACT_CHARS: usize = 2_000;
pub const MAX_DOCUMENT_CHARS: usize = 15_000;
pub const MAX_MATERIALS_CHARS: usize = 30_000;
pub const MAX_FILE_BYTES: usize = 20 * 1024 * 1024;

const GONE: &str = "Esta conversación ya no existe: se ha borrado.";
const FULL: &str = "Esta conversación ha llegado a 100 preguntas: bórrala para empezar otra \
                    (copia antes lo que quieras guardar).";
const TOO_MANY: &str = "Ya tienes 100 conversaciones guardadas en esta empresa: borra alguna en \
                        Asistente → «Conversaciones guardadas» para empezar otra.";
/// Lo que va en lugar de la pregunta en «Ver qué se envía» antes de escribirla.
const PLACEHOLDER: &str = "(aquí irá tu pregunta)";

/// De qué va una conversación.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Topic {
    /// Del Asistente, sin tema.
    General,
    /// Una recomendación de «Ahorrar impuestos» de un ejercicio.
    Ahorro { id: String, year: i32 },
    /// El plan de ahorro de un ejercicio (todas sus recomendaciones).
    AhorroPlan { year: i32 },
    /// Un hallazgo de «Qué revisar» (lo que tiene la AEAT) o de la última revisión de la
    /// carpeta. El código y el ejercicio no bastan para distinguirlo: va también su título.
    Hallazgo {
        origin: Origen,
        code: String,
        year: Option<i32>,
        title: String,
    },
    /// El borrador de un modelo de un ejercicio y periodo (trimestre, periodo del 202 o, en los
    /// anuales, 0).
    Modelo {
        model: String,
        year: i32,
        period: u8,
    },
}

/// Lo más largo que puede ser el título de un hallazgo para hablar de él.
const MAX_TITLE_CHARS: usize = 500;

impl Topic {
    pub fn kind(&self) -> &'static str {
        match self {
            Topic::General => "general",
            Topic::Ahorro { .. } => "ahorro",
            Topic::AhorroPlan { .. } => "ahorro_plan",
            Topic::Hallazgo { .. } => "hallazgo",
            Topic::Modelo { .. } => "modelo",
        }
    }

    /// Clave del tema con su ejercicio: `capitalizacion@2026`, `2026`,
    /// `aeat/cruces.390_303/2024/<huella del título>`, `303/2026/3`; vacía en las generales.
    pub fn key(&self) -> String {
        match self {
            Topic::General => String::new(),
            Topic::Ahorro { id, year } => format!("{id}@{year}"),
            Topic::AhorroPlan { year } => year.to_string(),
            Topic::Hallazgo {
                origin,
                code,
                year,
                title,
            } => {
                use sha2::{Digest, Sha256};
                let huella = hex::encode(Sha256::digest(title.as_bytes()));
                format!(
                    "{}/{code}/{}/{}",
                    origin.as_str(),
                    year.map_or_else(|| "-".to_owned(), |y| y.to_string()),
                    &huella[..12]
                )
            }
            Topic::Modelo {
                model,
                year,
                period,
            } => format!("{model}/{year}/{period}"),
        }
    }

    fn validate(&self) -> Result<()> {
        let year = |y: i32| {
            if (2000..=2100).contains(&y) {
                Ok(())
            } else {
                Err(other(format!("ejercicio no válido: {y}")))
            }
        };
        match self {
            Topic::General => Ok(()),
            Topic::Ahorro { id, year: y } => {
                let bien = (1..=40).contains(&id.len())
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
                if !bien || ahorro::ficha(id).is_none() {
                    return Err(other(format!("recomendación desconocida: {id}")));
                }
                year(*y)
            }
            Topic::AhorroPlan { year: y } => year(*y),
            Topic::Hallazgo {
                code,
                year: y,
                title,
                ..
            } => {
                let bien = (1..=60).contains(&code.len())
                    && code.bytes().all(|b| {
                        b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.'
                    });
                if !bien {
                    return Err(other(format!("hallazgo desconocido: {code}")));
                }
                if title.trim().is_empty() || title.chars().count() > MAX_TITLE_CHARS {
                    return Err(other("el título del hallazgo no es válido"));
                }
                y.map_or(Ok(()), year)
            }
            Topic::Modelo {
                model,
                year: y,
                period,
            } => {
                if model.len() != 3 || !model.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(other(format!("modelo desconocido: {model}")));
                }
                // Los anuales van con 0; el 202 tiene tres periodos y los demás, cuatro.
                let periodos = if models::period_code(model, 0) == "0A" {
                    0..=0
                } else if model == "202" {
                    1..=3
                } else {
                    1..=4
                };
                if !periodos.contains(period) {
                    return Err(other(format!(
                        "periodo no válido para el modelo {model}: {period}"
                    )));
                }
                year(*y)
            }
        }
    }

    /// La línea de arriba del panel.
    fn group(&self) -> String {
        match self {
            Topic::General => "Asistente".into(),
            Topic::Ahorro { year, .. } => format!("Ahorrar impuestos · {year}"),
            Topic::AhorroPlan { year } => format!("Ahorrar impuestos · tu plan de {year}"),
            Topic::Hallazgo { origin, year, .. } => revision::grupo(*origin, *year),
            Topic::Modelo { year, .. } => format!("Modelos · {year}"),
        }
    }
}

/// Lo que ve el usuario arriba del panel: de qué va, lo que sabe TaxPain, su primer mensaje y
/// preguntas para empezar. Nada de esto se envía tal cual (el tema va aparte).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopicHeader {
    /// «Ahorrar impuestos · 2026», «Asistente».
    pub group: String,
    pub title: String,
    /// Lo que sabe TaxPain del tema (etiqueta, valor).
    pub facts: Vec<(String, String)>,
    /// El primer mensaje de TaxPain (no se envía).
    pub intro: String,
    /// Preguntas para empezar: rellenan el cuadro, no se envían solas.
    pub suggestions: Vec<String>,
    /// El ejercicio ya está cerrado o el tema ya no sale con los datos de ahora.
    pub notice: Option<String>,
}

/// Lo que se apunta de una respuesta (para enseñarlo debajo).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MessageNotes {
    pub model: String,
    /// Datos personales distintos que se sustituyeron por marcadores.
    pub replaced: usize,
    /// Herramientas de sólo lectura que usó.
    pub used: Vec<String>,
    /// Normas del BOE que leyó.
    pub sources: Vec<String>,
    /// Citas que no aparecen en lo que leyó.
    pub unverified: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StoredMessage {
    pub seq: i64,
    pub role: Role,
    /// Como se ve en el Mac (con los datos restaurados).
    pub content: String,
    pub created_at: String,
    /// Sólo en las respuestas.
    pub notes: Option<MessageNotes>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialKind {
    /// Un dato escrito por el usuario.
    Dato,
    /// El texto de un documento.
    Documento,
}

impl MaterialKind {
    fn as_str(self) -> &'static str {
        match self {
            MaterialKind::Dato => "dato",
            MaterialKind::Documento => "documento",
        }
    }
}

/// Quién leyó el texto de un documento.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadBy {
    /// En el Mac (texto del PDF, Vision, hoja de cálculo o texto): el fichero no sale.
    Mac,
    /// La IA, con el fichero completo («Full IA»).
    Ia,
}

impl ReadBy {
    fn as_str(self) -> &'static str {
        match self {
            ReadBy::Mac => "mac",
            ReadBy::Ia => "ia",
        }
    }
}

/// Algo que el usuario ha añadido a una conversación.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ConversationMaterial {
    pub id: i64,
    pub kind: MaterialKind,
    pub content: String,
    /// Sólo para enseñarlo en el Mac: no se envía.
    pub filename: Option<String>,
    pub read_by: Option<ReadBy>,
    /// El texto se ha cortado en [`MAX_DOCUMENT_CHARS`].
    pub truncated: bool,
    pub chars: usize,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Conversation {
    pub id: i64,
    pub topic: Topic,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub messages: Vec<StoredMessage>,
    pub materials: Vec<ConversationMaterial>,
    /// Ha llegado a [`MAX_QUESTIONS`]: no se puede preguntar más.
    pub full: bool,
}

/// Un tema abierto en el panel: la cabecera y, si la hay, su conversación guardada.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TopicOpened {
    pub header: TopicHeader,
    pub conversation: Option<Conversation>,
    /// Los datos del tema de ahora no son los de la última pregunta.
    pub changed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ConversationSummary {
    pub id: i64,
    pub topic: Topic,
    pub title: String,
    pub updated_at: String,
    pub questions: usize,
    pub materials: usize,
}

/// Exactamente lo que se enviaría (anonimizado), para «Ver qué se envía».
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TopicOutgoing {
    pub system: String,
    pub messages: Vec<Message>,
    pub replaced: usize,
    /// Intervenciones antiguas que no van.
    pub omitted: usize,
}

/// El texto de un documento leído en el Mac, listo para añadirlo (el fichero no se guarda).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DocumentReading {
    /// Sólo para enseñarlo en el Mac: no se envía.
    pub filename: String,
    pub sha256: String,
    pub size: usize,
    pub text: String,
    /// Se ha cortado en [`MAX_DOCUMENT_CHARS`].
    pub truncated: bool,
}

/// Una pregunta de una conversación.
#[derive(Clone, Copy, Debug)]
pub struct AskRequest<'a> {
    pub topic: &'a Topic,
    /// La conversación guardada; `None` para un tema que aún no tiene o una general nueva.
    pub conversation: Option<i64>,
    pub question: &'a str,
    pub hoy: Date,
}

fn other(m: impl Into<String>) -> AppError {
    AppError::Other(m.into())
}

fn fecha(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

/// El día (dd/mm/aaaa, en la España peninsular) de un instante guardado.
fn dia(at: &str) -> String {
    match OffsetDateTime::parse(at, &Rfc3339) {
        Ok(t) => fecha(taxpain_core::spain_time::peninsular_date(t)),
        Err(_) => at.chars().take(10).collect(),
    }
}

/// Lo que se envió del tema en la última pregunta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Snapshot {
    block: String,
    header: TopicHeader,
    /// dd/mm/aaaa.
    at: String,
    /// Los terceros que nombra el bloque (se tapan con marcadores al reenviarlo).
    #[serde(default)]
    terceros: Vec<String>,
    /// El día de los datos del bloque (aaaa-mm-dd): para ver si han cambiado se rehace ese día.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hoy: Option<String>,
}

/// Las notas de una respuesta como se guardan: las que se enseñan y lo que se tapó con
/// marcadores al preguntar (para taparlo también en las preguntas siguientes aunque ya no salga
/// en los datos; no va a la interfaz).
#[derive(Serialize)]
struct SavedNotes<'a> {
    #[serde(flatten)]
    notes: &'a MessageNotes,
    masked: &'a [String],
}

/// Lo tapado en las respuestas guardadas de una conversación.
fn masked(v: &Vault, id: i64) -> Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Masked {
        #[serde(default)]
        masked: Vec<String>,
    }
    let conn = v.connection();
    let mut q = conn.prepare(
        "SELECT notes FROM assistant_message
         WHERE conversation_id = ?1 AND role = 'assistant' AND notes IS NOT NULL",
    )?;
    let mut out: Vec<String> = Vec::new();
    for notes in q.query_map([id], |r| r.get::<_, String>(0))? {
        if let Ok(m) = serde_json::from_str::<Masked>(&notes?) {
            out.extend(m.masked);
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// Una conversación tal como está en el almacén (sin mensajes).
#[derive(Clone, Debug)]
struct Stored {
    id: i64,
    topic: Topic,
    title: String,
    snapshot: Option<Snapshot>,
    created_at: String,
    updated_at: String,
}

const COLUMNS: &str = "id, topic, title, snapshot, created_at, updated_at";

type RawStored = (i64, String, String, Option<String>, String, String);

fn raw_stored(r: &Row<'_>) -> taxpain_vault::rusqlite::Result<RawStored> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
    ))
}

fn stored(raw: RawStored) -> Result<Stored> {
    let (id, topic, title, snapshot, created_at, updated_at) = raw;
    let topic: Topic = serde_json::from_str(&topic)
        .map_err(|_| other("esta conversación es de un tema que esta versión no conoce"))?;
    Ok(Stored {
        id,
        topic,
        title,
        // Una foto que no se entiende es como no tenerla.
        snapshot: snapshot.and_then(|s| serde_json::from_str(&s).ok()),
        created_at,
        updated_at,
    })
}

fn by_id(v: &Vault, id: i64) -> Result<Option<Stored>> {
    v.connection()
        .query_row(
            &format!("SELECT {COLUMNS} FROM assistant_conversation WHERE id = ?1"),
            [id],
            raw_stored,
        )
        .optional()?
        .map(stored)
        .transpose()
}

/// La conversación de un tema (las generales no tienen una por tema).
fn by_key(v: &Vault, topic: &Topic) -> Result<Option<Stored>> {
    if *topic == Topic::General {
        return Ok(None);
    }
    v.connection()
        .query_row(
            &format!(
                "SELECT {COLUMNS} FROM assistant_conversation WHERE kind = ?1 AND topic_key = ?2"
            ),
            params![topic.kind(), topic.key()],
            raw_stored,
        )
        .optional()?
        .map(stored)
        .transpose()
}

/// La conversación de `topic`: la de `id` (que tiene que ser de ese tema) o, sin id, la del tema.
fn find(v: &Vault, topic: &Topic, id: Option<i64>) -> Result<Option<Stored>> {
    topic.validate()?;
    match id {
        Some(id) => {
            let s = by_id(v, id)?.ok_or_else(|| other(GONE))?;
            if s.topic != *topic {
                return Err(other("La conversación no es de este tema."));
            }
            Ok(Some(s))
        }
        None => by_key(v, topic),
    }
}

fn messages(v: &Vault, id: i64) -> Result<Vec<StoredMessage>> {
    let conn = v.connection();
    let mut q = conn.prepare(
        "SELECT seq, role, content, notes, created_at FROM assistant_message
         WHERE conversation_id = ?1 ORDER BY seq",
    )?;
    let rows = q.query_map([id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (seq, role, content, notes, created_at) = row?;
        out.push(StoredMessage {
            seq,
            role: if role == "assistant" {
                Role::Assistant
            } else {
                Role::User
            },
            content,
            created_at,
            notes: notes.and_then(|n| serde_json::from_str(&n).ok()),
        });
    }
    Ok(out)
}

fn materials(v: &Vault, id: i64) -> Result<Vec<ConversationMaterial>> {
    let conn = v.connection();
    let mut q = conn.prepare(
        "SELECT id, kind, content, filename, read_by, truncated, created_at FROM assistant_material
         WHERE conversation_id = ?1 ORDER BY id",
    )?;
    let rows = q.query_map([id], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, bool>(5)?,
            r.get::<_, String>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, kind, content, filename, read_by, truncated, created_at) = row?;
        out.push(ConversationMaterial {
            id,
            kind: if kind == "documento" {
                MaterialKind::Documento
            } else {
                MaterialKind::Dato
            },
            chars: content.chars().count(),
            content,
            filename,
            read_by: read_by.map(|r| if r == "ia" { ReadBy::Ia } else { ReadBy::Mac }),
            truncated,
            created_at,
        });
    }
    Ok(out)
}

fn questions(msgs: &[StoredMessage]) -> usize {
    msgs.iter().filter(|m| m.role == Role::User).count()
}

/// La conversación entera.
fn load(v: &Vault, s: Stored) -> Result<Conversation> {
    let messages = messages(v, s.id)?;
    Ok(Conversation {
        full: questions(&messages) >= MAX_QUESTIONS,
        materials: materials(v, s.id)?,
        messages,
        id: s.id,
        topic: s.topic,
        title: s.title,
        created_at: s.created_at,
        updated_at: s.updated_at,
    })
}

/// Desde qué mensaje del historial se envía: los últimos [`HISTORY_MESSAGES`] que quepan en
/// [`HISTORY_CHARS`], empezando siempre por una pregunta. Los de antes se quedan en el Mac.
fn window(msgs: &[StoredMessage]) -> usize {
    let mut start = msgs.len();
    let mut chars = 0;
    while start > 0 && msgs.len() - start < HISTORY_MESSAGES {
        let c = msgs[start - 1].content.chars().count();
        if chars + c > HISTORY_CHARS {
            break;
        }
        chars += c;
        start -= 1;
    }
    while start < msgs.len() && msgs[start].role == Role::Assistant {
        start += 1;
    }
    start
}

const GENERAL_INTRO: &str = "Pregunta sobre tus impuestos: la IA ve un resumen de tus cifras, \
                             plazos y avisos de TaxPain. Puedes añadir datos o documentos que \
                             TaxPain no tenga.";

fn general_header(title: Option<&str>) -> TopicHeader {
    TopicHeader {
        group: Topic::General.group(),
        title: title.unwrap_or("Nueva conversación").to_owned(),
        facts: Vec::new(),
        intro: GENERAL_INTRO.into(),
        suggestions: vec![
            "¿Cuánto IVA me toca pagar este trimestre y por qué?".into(),
            "¿Qué me falta por revisar antes del próximo plazo?".into(),
        ],
        notice: None,
    }
}

/// Los datos de un tema para el panel y para la pregunta.
struct TopicData {
    header: TopicHeader,
    /// Lo que se envía del tema (`None` en las generales).
    block: Option<String>,
    /// Son los datos de ahora (no la foto de la última pregunta).
    live: bool,
    /// Nombres e identificadores de los terceros que nombra el bloque (los avisos de un
    /// borrador): van al anonimizador como valores conocidos, para que salgan con marcadores.
    terceros: Vec<String>,
}

impl TopicData {
    fn live((header, block): (TopicHeader, String)) -> Self {
        TopicData {
            header,
            block: Some(block),
            live: true,
            terceros: Vec::new(),
        }
    }
}

/// Por qué un tema ya no se puede rehacer con los datos de ahora.
enum Gone {
    /// La ficha (o el plan) no sale; `closed`: el ejercicio ya no es el de ahora.
    Ahorro { year: i32, closed: bool, plan: bool },
    /// El hallazgo ya no sale en la revisión.
    Hallazgo(Origen),
    /// La revisión de la carpeta guardada es de una versión anterior: no dice a qué terceros
    /// nombran sus hallazgos, así que no se pregunta por ellos hasta rehacerla.
    RevisionAntigua,
    /// El borrador ya no se puede calcular.
    Modelo,
}

impl Gone {
    /// El error al abrir un tema así que aún no tiene conversación.
    fn error(&self) -> AppError {
        other(match self {
            Gone::Ahorro { closed: false, .. } => {
                "Esta recomendación no es para tu tipo de contribuyente.".to_owned()
            }
            Gone::Ahorro {
                year, plan: false, ..
            } => format!(
                "Esta recomendación es del ejercicio {year} y ya no se puede calcular: vuelve a \
                 abrir «Ahorrar impuestos»."
            ),
            Gone::Ahorro {
                year, plan: true, ..
            } => format!(
                "Este plan es del ejercicio {year} y ya no se puede calcular: vuelve a abrir \
                 «Ahorrar impuestos»."
            ),
            Gone::Hallazgo(Origen::Aeat) => "Este hallazgo ya no sale en «Qué revisar»: vuelve a \
                                             abrir Evolución para verlo con tus datos de ahora."
                .to_owned(),
            Gone::Hallazgo(Origen::Carpeta) => "Este hallazgo ya no sale en la última revisión \
                                                de la carpeta: vuelve a revisarla en Importar → \
                                                Revisión."
                .to_owned(),
            Gone::RevisionAntigua => "Esta revisión de la carpeta es de una versión anterior de \
                                      TaxPain: vuelve a revisarla en Importar → Revisión para \
                                      preguntar a la IA por sus hallazgos."
                .to_owned(),
            // Sin conversación, el error es el del borrador (no se llega aquí).
            Gone::Modelo => "Este borrador ya no se puede calcular.".to_owned(),
        })
    }

    /// El aviso cuando se sigue con la foto de la última pregunta (del día `at`).
    fn notice(&self, at: &str) -> String {
        match self {
            Gone::Ahorro { closed: false, .. } => format!(
                "Este tema ya no está en TaxPain con tus datos de ahora: la IA verá los de tu \
                 última pregunta ({at})."
            ),
            Gone::Ahorro { year, .. } => format!(
                "Esta conversación es del ejercicio {year}: la IA verá los datos del tema de tu \
                 última pregunta ({at}) y lo que has añadido."
            ),
            Gone::Hallazgo(_) | Gone::RevisionAntigua => format!(
                "Este hallazgo ya no sale en la última revisión: la IA lo verá como estaba en tu \
                 última pregunta ({at}) y lo que has añadido."
            ),
            Gone::Modelo => format!(
                "Este borrador ya no se puede calcular con tus datos de ahora: la IA verá el de tu \
                 última pregunta ({at}) y lo que has añadido."
            ),
        }
    }
}

/// Rehace el tema con los datos de ahora; si ya no se puede (otro ejercicio, una ficha o un
/// hallazgo que ya no sale), usa la foto de la última pregunta.
fn topic_data(v: &Vault, hoy: Date, topic: &Topic, stored: Option<&Stored>) -> Result<TopicData> {
    let gone = match topic {
        Topic::General => {
            return Ok(TopicData {
                header: general_header(stored.map(|s| s.title.as_str())),
                block: None,
                live: true,
                terceros: Vec::new(),
            });
        }
        Topic::Ahorro { year, .. } | Topic::AhorroPlan { year } => {
            let current = accounting::fiscal_year_at(v, hoy)?.year;
            if *year == current {
                let a = ahorro::ahorro(v, hoy)?;
                let actual = match topic {
                    Topic::Ahorro { id, .. } => ahorro::tema_tarjeta(&a, id),
                    _ => Some(ahorro::tema_plan(&a)),
                };
                if let Some(t) = actual {
                    return Ok(TopicData::live(t));
                }
            }
            Gone::Ahorro {
                year: *year,
                closed: *year != current,
                plan: matches!(topic, Topic::AhorroPlan { .. }),
            }
        }
        Topic::Hallazgo {
            origin,
            code,
            year,
            title,
        } => {
            let r = match revision::de_origen(v, hoy, *origin) {
                Ok(r) => r,
                // Con una conversación guardada se sigue con su foto.
                Err(_) if stored.is_some() => None,
                Err(e) => return Err(e),
            };
            if r.as_ref().is_some_and(revision::sin_terceros) {
                Gone::RevisionAntigua
            } else if let Some((header, block, terceros)) =
                r.and_then(|r| revision::tema_hallazgo(&r, *origin, code, *year, title))
            {
                return Ok(TopicData {
                    terceros,
                    ..TopicData::live((header, block))
                });
            } else {
                Gone::Hallazgo(*origin)
            }
        }
        Topic::Modelo {
            model,
            year,
            period,
        } => match models::draft(v, model, *year, *period) {
            Ok(d) => {
                let code = models::period_code(model, *period);
                let presentado = models::filed_returns(v, Some(*year))?
                    .into_iter()
                    .filter(|f| f.model == *model && f.period == code)
                    // El de verdad cuenta antes que uno de pruebas.
                    .min_by_key(|f| f.environment != "produccion");
                return Ok(TopicData {
                    terceros: d.terceros.clone(),
                    ..TopicData::live(models::tema_borrador(&d, presentado.as_ref()))
                });
            }
            // Con una conversación guardada se sigue con su foto.
            Err(_) if stored.is_some() => Gone::Modelo,
            Err(e) => return Err(e),
        },
    };
    let Some(s) = stored else {
        return Err(gone.error());
    };
    Ok(match &s.snapshot {
        Some(snap) => {
            let mut header = snap.header.clone();
            header.notice = Some(gone.notice(&snap.at));
            TopicData {
                header,
                block: Some(snap.block.clone()),
                live: false,
                terceros: snap.terceros.clone(),
            }
        }
        // Sin foto (se añadieron datos sin llegar a preguntar): sólo de qué va el tema. El título
        // de un hallazgo no va: puede nombrar a terceros y ficheros, y sin el hallazgo no se sabe
        // cuáles para taparlos; va su código.
        None => {
            let group = topic.group();
            let tema = match topic {
                Topic::Hallazgo { code, .. } => format!("un hallazgo con el código «{code}»"),
                _ => format!("«{}»", s.title),
            };
            TopicData {
                block: Some(format!(
                    "Tema de TaxPain: {tema} ({group}). TaxPain ya no lo calcula con los datos de \
                     ahora: usa lo que diga la conversación y lo que ha añadido el usuario, y \
                     pídele lo que falte."
                )),
                header: TopicHeader {
                    group,
                    title: s.title.clone(),
                    facts: Vec::new(),
                    intro: "Pregunta lo que quieras o añade datos que TaxPain no tenga (cifras, \
                            decisiones o documentos)."
                        .into(),
                    suggestions: Vec::new(),
                    notice: Some(
                        "Este tema ya no está en TaxPain con tus datos de ahora: la IA sólo verá \
                         la conversación y lo que has añadido."
                            .into(),
                    ),
                },
                live: false,
                terceros: Vec::new(),
            }
        }
    })
}

/// ¿Los datos del tema ya no son los de la última pregunta? Una recomendación de «Ahorrar
/// impuestos» se rehace con los datos del día de su foto: la amortización que se devenga cada
/// día cambia sus cifras sin que el usuario haya cambiado nada.
fn changed(
    v: &Vault,
    hoy: Date,
    topic: &Topic,
    stored: Option<&Stored>,
    data: &TopicData,
) -> Result<bool> {
    let Some(snap) = stored.and_then(|s| s.snapshot.as_ref()) else {
        return Ok(false);
    };
    if !data.live {
        return Ok(false);
    }
    let dia = snap
        .hoy
        .as_deref()
        .and_then(|d| Date::parse(d, &time::format_description::well_known::Iso8601::DATE).ok())
        .filter(|d| *d < hoy);
    let block = match (topic, dia) {
        (Topic::Ahorro { .. } | Topic::AhorroPlan { .. }, Some(d)) => {
            let then = topic_data(v, d, topic, stored)?;
            then.block.filter(|_| then.live)
        }
        _ => data.block.clone(),
    };
    Ok(block.as_ref() != Some(&snap.block))
}

fn opened(v: &Vault, hoy: Date, topic: &Topic, stored: Option<Stored>) -> Result<TopicOpened> {
    let data = topic_data(v, hoy, topic, stored.as_ref())?;
    let changed = changed(v, hoy, topic, stored.as_ref(), &data)?;
    let conversation = stored.map(|s| load(v, s)).transpose()?;
    Ok(TopicOpened {
        header: data.header,
        conversation,
        changed,
    })
}

/// Abre un tema en el panel (no crea nada): la cabecera y la conversación guardada, si la hay.
/// Una general sin id es una nueva.
pub fn open(v: &Vault, hoy: Date, topic: &Topic) -> Result<TopicOpened> {
    let stored = find(v, topic, None)?;
    opened(v, hoy, topic, stored)
}

/// Abre una conversación guardada (desde la lista del Asistente).
pub fn open_by_id(v: &Vault, hoy: Date, id: i64) -> Result<TopicOpened> {
    let s = by_id(v, id)?.ok_or_else(|| other(GONE))?;
    let topic = s.topic.clone();
    opened(v, hoy, &topic, Some(s))
}

/// Las conversaciones guardadas, la más reciente primero.
pub fn list(v: &Vault) -> Result<Vec<ConversationSummary>> {
    let conn = v.connection();
    let mut q = conn.prepare(
        "SELECT c.id, c.topic, c.title, c.updated_at,
                (SELECT count(*) FROM assistant_message m
                 WHERE m.conversation_id = c.id AND m.role = 'user'),
                (SELECT count(*) FROM assistant_material a WHERE a.conversation_id = c.id)
         FROM assistant_conversation c ORDER BY c.updated_at DESC, c.id DESC",
    )?;
    let rows = q.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, i64>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, topic, title, updated_at, questions, materials) = row?;
        // La de un tema que esta versión no conoce (de una versión más nueva) no sale.
        let Ok(topic) = serde_json::from_str::<Topic>(&topic) else {
            continue;
        };
        out.push(ConversationSummary {
            id,
            topic,
            title,
            updated_at,
            questions: usize::try_from(questions).unwrap_or(0),
            materials: usize::try_from(materials).unwrap_or(0),
        });
    }
    Ok(out)
}

struct DraftMaterial {
    kind: MaterialKind,
    text: String,
    /// dd/mm/aaaa.
    added: String,
    by_ai: bool,
    /// El documento no va entero.
    cut: bool,
}

/// Lo que se enviaría en una pregunta, antes de anonimizarlo: se reúne con el almacén cogido y
/// se anonimiza sin él (el detector de nombres tarda).
struct Draft {
    topic: Topic,
    /// La conversación guardada (`None`: se crea al guardar la primera respuesta).
    id: Option<i64>,
    /// Cuándo se creó la guardada: si al guardar la respuesta ya no es la misma, no se guarda.
    created_at: Option<String>,
    /// El día de los datos.
    hoy: Date,
    header: TopicHeader,
    block: Option<String>,
    live: bool,
    /// Los terceros que nombra el bloque del tema (van en su foto).
    terceros: Vec<String>,
    context: String,
    /// Valores que se tapan con marcadores: los de la empresa, los terceros del tema y del
    /// resumen y lo que se tapó en las preguntas anteriores de la conversación.
    known: Vec<String>,
    materials: Vec<DraftMaterial>,
    /// Ya recortado; la última es la pregunta.
    history: Vec<Message>,
    omitted: usize,
}

/// La pregunta, sin espacios alrededor y con su límite.
fn check_question(q: &str) -> Result<&str> {
    let q = q.trim();
    if q.is_empty() {
        return Err(other("Escribe tu pregunta."));
    }
    if q.chars().count() > MAX_QUESTION_CHARS {
        return Err(other(
            "Tu pregunta es demasiado larga: como mucho 4.000 caracteres.",
        ));
    }
    Ok(q)
}

/// Reúne todo lo de una pregunta (`None`: «Ver qué se envía» sin pregunta todavía).
fn draft(
    v: &Vault,
    hoy: Date,
    topic: &Topic,
    conversation: Option<i64>,
    question: Option<&str>,
) -> Result<Draft> {
    let stored = find(v, topic, conversation)?;
    let msgs = match &stored {
        Some(s) => messages(v, s.id)?,
        None => Vec::new(),
    };
    if questions(&msgs) >= MAX_QUESTIONS {
        return Err(other(FULL));
    }
    let data = topic_data(v, hoy, topic, stored.as_ref())?;
    let start = window(&msgs);
    let mut history: Vec<Message> = msgs[start..]
        .iter()
        .map(|m| Message {
            role: m.role,
            content: m.content.clone(),
        })
        .collect();
    history.push(Message {
        role: Role::User,
        content: question.unwrap_or(PLACEHOLDER).to_owned(),
    });
    let materials = match &stored {
        Some(s) => materials(v, s.id)?
            .into_iter()
            .map(|m| DraftMaterial {
                kind: m.kind,
                added: dia(&m.created_at),
                by_ai: m.read_by == Some(ReadBy::Ia),
                cut: m.truncated,
                text: m.content,
            })
            .collect(),
        None => Vec::new(),
    };
    let (context, del_resumen) = assistant::context_with_third_parties(v, hoy)?;
    let mut known = assistant::known_values(v)?;
    known.extend(data.terceros.iter().cloned());
    known.extend(del_resumen);
    // El historial se guarda con los datos restaurados: lo que se tapó en una pregunta anterior
    // (un tercero que ya no está en el tema, el nombre de antes de la empresa…) se vuelve a tapar
    // aunque ya no salga en los datos de ahora.
    if let Some(s) = &stored {
        known.extend(masked(v, s.id)?);
        if let Some(snap) = &s.snapshot {
            known.extend(snap.terceros.iter().cloned());
        }
    }
    Ok(Draft {
        topic: topic.clone(),
        id: stored.as_ref().map(|s| s.id),
        created_at: stored.map(|s| s.created_at),
        hoy,
        header: data.header,
        block: data.block,
        live: data.live,
        terceros: data.terceros,
        context,
        known,
        materials,
        history,
        omitted: start,
    })
}

/// Anonimiza lo reunido (sin el almacén cogido: el detector de nombres tarda).
fn outgoing(d: &Draft) -> Result<Outgoing> {
    let materials: Vec<Material<'_>> = d
        .materials
        .iter()
        .map(|m| match m.kind {
            MaterialKind::Dato => Material::Fact {
                text: &m.text,
                added: &m.added,
            },
            MaterialKind::Documento => Material::Document {
                text: &m.text,
                added: &m.added,
                by_ai: m.by_ai,
                cut: m.cut,
            },
        })
        .collect();
    let input = ConversationInput {
        topic: d.block.as_deref(),
        context: &d.context,
        materials: &materials,
        history: &d.history,
        omitted: d.omitted,
    };
    Ok(taxpain_assistant::prepare_conversation(&input, &d.known)?)
}

/// Exactamente lo que se enviaría con `question` (o, sin ella, con un texto en su lugar). Reúne
/// los datos con `vault()` y los anonimiza después, sin el almacén.
pub fn preview<G: Deref<Target = Vault>>(
    vault: impl Fn() -> Result<G>,
    hoy: Date,
    topic: &Topic,
    conversation: Option<i64>,
    question: Option<&str>,
) -> Result<TopicOutgoing> {
    let question = match question.map(str::trim).filter(|q| !q.is_empty()) {
        Some(q) => Some(check_question(q)?),
        None => None,
    };
    let d = {
        let v = vault()?;
        draft(&v, hoy, topic, conversation, question)?
    };
    let out = outgoing(&d)?;
    Ok(TopicOutgoing {
        replaced: out.replaced(),
        system: out.system,
        messages: out.messages,
        omitted: d.omitted,
    })
}

/// La auditoría de una pregunta, antes de enviarla: el modelo, si es remoto, cuántos datos se
/// sustituyeron, el tipo de tema y cuántos datos y documentos van. Nunca el texto.
fn record_send(
    v: &Vault,
    provider: &Provider,
    out: &Outgoing,
    d: &Draft,
    now: OffsetDateTime,
) -> Result<()> {
    let datos = d
        .materials
        .iter()
        .filter(|m| m.kind == MaterialKind::Dato)
        .count();
    audit::append(
        v.connection(),
        &invoicing::now_str(now),
        Actor::User,
        "assistant.chat",
        provider.model(),
        &serde_json::json!({
            "remote": provider.is_remote(),
            "anonymized": out.replaced(),
            "tema": d.topic.kind(),
            "datos": datos,
            "documentos": d.materials.len() - datos,
        }),
    )?;
    Ok(())
}

/// Título por defecto de una conversación nueva.
fn new_title(topic: &Topic, now: OffsetDateTime) -> String {
    match topic {
        Topic::General => format!(
            "Conversación del {}",
            fecha(taxpain_core::spain_time::peninsular_date(now))
        ),
        Topic::Ahorro { id, .. } => ahorro::ficha(id)
            .map(|f| f.titulo.to_owned())
            .unwrap_or_else(|| id.clone()),
        Topic::AhorroPlan { year } => format!("Tu plan de ahorro de {year}"),
        Topic::Hallazgo { title, .. } => title.clone(),
        Topic::Modelo {
            model,
            year,
            period,
        } => format!(
            "Borrador del modelo {model} · {}",
            models::periodo(model, *period, *year)
        ),
    }
}

/// ¿Es el título por defecto de una general («Conversación del dd/mm/aaaa»)?
fn default_title(t: &str) -> bool {
    t.strip_prefix("Conversación del ").is_some_and(|d| {
        let b = d.as_bytes();
        b.len() == 10 && b[2] == b'/' && b[5] == b'/'
    })
}

/// Título de una general sacado de su primera pregunta: los primeros 60 caracteres, cortados en
/// una palabra.
fn title_from_question(q: &str) -> String {
    let q = q.split_whitespace().collect::<Vec<_>>().join(" ");
    if q.chars().count() <= 60 {
        return q;
    }
    let cut: String = q.chars().take(60).collect();
    let cut = match cut.rfind(' ') {
        Some(i) if i > 20 => &cut[..i],
        _ => cut.as_str(),
    };
    format!("{}…", cut.trim_end_matches([',', ';', ':', '.', ' ']))
}

/// Primeras preguntas de conversaciones nuevas que esperan su respuesta, por almacén (con su
/// tema): cada una creará una conversación al llegar (una respuesta pagada siempre se guarda),
/// así que cuentan para el límite; si no, varias a la vez lo pasarían.
type Waiting = BTreeMap<String, Vec<WaitingId>>;
static WAITING_NEW: std::sync::Mutex<Waiting> = std::sync::Mutex::new(BTreeMap::new());

fn waiting_new() -> std::sync::MutexGuard<'static, Waiting> {
    WAITING_NEW
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// De qué almacén es una espera: su carpeta (o, en memoria, el propio almacén).
fn vault_key(v: &Vault) -> String {
    v.dir().map_or_else(
        || format!("{:p}", std::ptr::from_ref(v)),
        |d| d.display().to_string(),
    )
}

/// Las conversaciones que crearán las primeras preguntas que esperan, salvo la de `topic` (con la
/// suya ya contada, otra conversación del mismo tema es la misma) y las de los temas que ya
/// tienen la suya (se les ha añadido algo mientras esperaban: ya cuenta entre las guardadas).
/// Cada general es otra.
fn waiting_for(w: &Waiting, v: &Vault, topic: &Topic) -> Result<usize> {
    let Some(temas) = w.get(&vault_key(v)) else {
        return Ok(0);
    };
    let mine = waiting_id(topic);
    let generales = temas.iter().filter(|t| t.is_none()).count();
    let otros: std::collections::BTreeSet<&(&str, String)> = temas
        .iter()
        .flatten()
        .filter(|t| mine.as_ref() != Some(*t))
        .collect();
    let mut n = generales;
    for (kind, key) in otros {
        let guardada: bool = v.connection().query_row(
            "SELECT EXISTS(SELECT 1 FROM assistant_conversation
             WHERE kind = ?1 AND topic_key = ?2)",
            params![kind, key],
            |r| r.get(0),
        )?;
        if !guardada {
            n += 1;
        }
    }
    Ok(n)
}

/// El tema de una espera (tipo y clave), o `None` en una general (cada una es otra
/// conversación).
type WaitingId = Option<(&'static str, String)>;

fn waiting_id(topic: &Topic) -> WaitingId {
    match topic {
        Topic::General => None,
        _ => Some((topic.kind(), topic.key())),
    }
}

/// Error si ya no caben más conversaciones, contando las que crearán las primeras preguntas que
/// esperan (se mira antes de crear una, y antes de enviar la primera pregunta de una nueva: una
/// respuesta pagada siempre se guarda).
fn check_room(v: &Vault, waiting: usize) -> Result<()> {
    let n: i64 =
        v.connection()
            .query_row("SELECT count(*) FROM assistant_conversation", [], |r| {
                r.get(0)
            })?;
    if usize::try_from(n).unwrap_or(usize::MAX) + waiting >= MAX_CONVERSATIONS {
        return Err(other(TOO_MANY));
    }
    Ok(())
}

/// La plaza de una conversación nueva mientras su primera pregunta espera la respuesta: el
/// almacén y el tema.
struct Reserved(String, WaitingId);

impl Reserved {
    /// Reserva una plaza si cabe (con el almacén cogido).
    fn new(v: &Vault, topic: &Topic) -> Result<Self> {
        let mut w = waiting_new();
        check_room(v, waiting_for(&w, v, topic)?)?;
        let (almacen, tema) = (vault_key(v), waiting_id(topic));
        w.entry(almacen.clone()).or_default().push(tema.clone());
        Ok(Reserved(almacen, tema))
    }
}

impl Drop for Reserved {
    fn drop(&mut self) {
        let mut w = waiting_new();
        if let Some(temas) = w.get_mut(&self.0) {
            if let Some(i) = temas.iter().position(|t| *t == self.1) {
                temas.swap_remove(i);
            }
            if temas.is_empty() {
                w.remove(&self.0);
            }
        }
    }
}

/// Crea la conversación de un tema (o una general).
fn create(v: &Vault, topic: &Topic, title: &str, at: &str) -> Result<i64> {
    let conn = v.connection();
    let json = serde_json::to_string(topic).map_err(|e| other(e.to_string()))?;
    if *topic == Topic::General {
        conn.execute(
            "INSERT INTO assistant_conversation(kind, topic_key, topic, title, created_at,
               updated_at) VALUES ('general', '', ?1, ?2, ?3, ?3)",
            params![json, title, at],
        )?;
        return Ok(conn.last_insert_rowid());
    }
    conn.execute(
        "INSERT INTO assistant_conversation(kind, topic_key, topic, title, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(kind, topic_key) WHERE kind <> 'general' DO NOTHING",
        params![topic.kind(), topic.key(), json, title, at],
    )?;
    by_key(v, topic)?
        .map(|s| s.id)
        .ok_or_else(|| other("no se ha podido guardar la conversación"))
}

/// Guarda la pregunta y su respuesta juntas, en la conversación en la que se preguntó (o en una
/// nueva). Si la conversación se borró mientras respondía, no se guarda nada.
fn save_exchange(
    v: &Vault,
    d: &Draft,
    question: &str,
    answer: &Answered,
    provider: &Provider,
    now: OffsetDateTime,
) -> Result<Conversation> {
    let at = invoicing::now_str(now);
    let tx = v.connection().unchecked_transaction()?;
    let (id, title) = match d.id {
        Some(id) => {
            // Tiene que ser la misma (del mismo tema y creada a la vez), no otra con su id.
            let s = by_id(v, id)?
                .filter(|s| s.topic == d.topic && Some(&s.created_at) == d.created_at.as_ref())
                .ok_or_else(|| {
                    other(
                        "Esta conversación se ha borrado mientras respondía: la respuesta no se \
                         ha guardado.",
                    )
                })?;
            (id, s.title)
        }
        None => match by_key(v, &d.topic)? {
            // Se ha creado mientras respondía (al añadir un dato): sigue en ella.
            Some(s) => (s.id, s.title),
            None => {
                let title = match d.topic {
                    Topic::General => title_from_question(question),
                    _ => new_title(&d.topic, now),
                };
                (create(v, &d.topic, &title, &at)?, title)
            }
        },
    };
    // Una general con el título por defecto (se añadió algo antes de preguntar) toma el de su
    // primera pregunta.
    let title = if d.topic == Topic::General && default_title(&title) {
        title_from_question(question)
    } else {
        title
    };
    let seq: i64 = tx.query_row(
        "SELECT COALESCE(MAX(seq), 0) FROM assistant_message WHERE conversation_id = ?1",
        [id],
        |r| r.get(0),
    )?;
    let notes = MessageNotes {
        model: provider.model().to_owned(),
        replaced: answer.replaced,
        used: answer.used.clone(),
        sources: answer.sources.clone(),
        unverified: answer.unverified.clone(),
    };
    tx.execute(
        "INSERT INTO assistant_message(conversation_id, seq, role, content, notes, created_at)
         VALUES (?1, ?2, 'user', ?3, NULL, ?4)",
        params![id, seq + 1, question, at],
    )?;
    tx.execute(
        "INSERT INTO assistant_message(conversation_id, seq, role, content, notes, created_at)
         VALUES (?1, ?2, 'assistant', ?3, ?4, ?5)",
        params![
            id,
            seq + 2,
            answer.text,
            serde_json::to_string(&SavedNotes {
                notes: &notes,
                masked: &answer.masked,
            })
            .map_err(|e| other(e.to_string()))?,
            at
        ],
    )?;
    // La foto del tema, sólo si es la de ahora (con la de una pregunta anterior, se queda la que
    // había, con su fecha). Las generales no tienen tema.
    let snapshot = match (&d.block, d.live) {
        (Some(block), true) => Some(
            serde_json::to_string(&Snapshot {
                block: block.clone(),
                header: TopicHeader {
                    notice: None,
                    ..d.header.clone()
                },
                at: fecha(taxpain_core::spain_time::peninsular_date(now)),
                terceros: d.terceros.clone(),
                hoy: Some(d.hoy.to_string()),
            })
            .map_err(|e| other(e.to_string()))?,
        ),
        _ => None,
    };
    tx.execute(
        "UPDATE assistant_conversation SET updated_at = ?2, title = ?3,
           snapshot = COALESCE(?4, snapshot)
         WHERE id = ?1",
        params![id, at, title, snapshot],
    )?;
    tx.commit()?;
    let s = by_id(v, id)?.ok_or_else(|| other(GONE))?;
    load(v, s)
}

/// Pregunta en una conversación y guarda la pregunta con su respuesta.
///
/// - `vault()`: el almacén de la empresa en la que se pregunta, cogido sólo mientras se reúnen
///   los datos y se anota la auditoría (las herramientas de `tools` también lo cogen sólo
///   mientras consultan). Si se ha cambiado de empresa, tiene que fallar.
/// - `keep()`: dónde se guarda la respuesta, aunque ya no sea la empresa abierta (una respuesta
///   pagada no se pierde ni acaba en otra empresa).
///
/// Ningún cerrojo se mantiene mientras se espera al proveedor.
pub async fn ask<G, K>(
    vault: impl Fn() -> Result<G>,
    keep: impl FnOnce() -> Result<K>,
    client: &Client,
    tools: &dyn LocalTools,
    req: &AskRequest<'_>,
    clock: impl Fn() -> OffsetDateTime,
) -> Result<TopicOpened>
where
    G: Deref<Target = Vault>,
    K: Deref<Target = Vault>,
{
    let question = check_question(req.question)?;
    let (provider, key, d, _reserved) = {
        let v = vault()?;
        let status = assistant::status(&v)?;
        assistant::require_consent(&status)?;
        let key = assistant::api_key(&v)?.map(Zeroizing::new);
        if status.remote && key.is_none() {
            return Err(AssistantError::MissingKey.into());
        }
        let d = draft(&v, req.hoy, req.topic, req.conversation, Some(question))?;
        // Una conversación nueva reserva su plaza hasta que se guarda (o falla) la respuesta.
        let reserved =
            d.id.is_none()
                .then(|| Reserved::new(&v, req.topic))
                .transpose()?;
        (status.settings.provider, key, d, reserved)
    };
    let out = outgoing(&d)?;
    {
        let v = vault()?;
        record_send(&v, &provider, &out, &d, clock())?;
    }
    let answer = taxpain_assistant::ask_with_tools(
        client,
        &provider,
        key.as_deref().map(String::as_str),
        &out,
        tools,
    )
    .await?;
    let v = keep()?;
    let conversation = save_exchange(&v, &d, question, &answer, &provider, clock())?;
    let header = match req.topic {
        Topic::General => general_header(Some(&conversation.title)),
        _ => d.header,
    };
    Ok(TopicOpened {
        header,
        conversation: Some(conversation),
        changed: false,
    })
}

// ---------- Lo que añade el usuario ----------

/// Añade a la conversación de `topic` (la de `conversation` o la del tema; si no hay, la crea)
/// un dato o el texto de un documento, con los límites de cada conversación.
fn add_material(
    v: &Vault,
    now: OffsetDateTime,
    topic: &Topic,
    conversation: Option<i64>,
    kind: MaterialKind,
    text: &str,
    doc: Option<(&DocumentReading, ReadBy)>,
) -> Result<Conversation> {
    let stored = find(v, topic, conversation)?;
    if let Some(s) = &stored {
        let ms = materials(v, s.id)?;
        if ms.len() >= MAX_MATERIALS {
            return Err(other(
                "Ya hay 20 datos y documentos en esta conversación: quita alguno para añadir \
                 otro.",
            ));
        }
        let total: usize = ms.iter().map(|m| m.chars).sum();
        if total + text.chars().count() > MAX_MATERIALS_CHARS {
            return Err(other(
                "Entre todo lo que has añadido no caben más de 30.000 caracteres: quita algo.",
            ));
        }
        if let Some((d, _)) = doc {
            let repetido: bool = v.connection().query_row(
                "SELECT EXISTS(SELECT 1 FROM assistant_material
                 WHERE conversation_id = ?1 AND file_sha256 = ?2)",
                params![s.id, d.sha256],
                |r| r.get(0),
            )?;
            if repetido {
                return Err(other("Ese documento ya está en esta conversación."));
            }
        }
    } else {
        let waiting = waiting_for(&waiting_new(), v, topic)?;
        check_room(v, waiting)?;
    }
    let at = invoicing::now_str(now);
    let tx = v.connection().unchecked_transaction()?;
    let id = match &stored {
        Some(s) => s.id,
        None => create(v, topic, &new_title(topic, now), &at)?,
    };
    tx.execute(
        "INSERT INTO assistant_material(conversation_id, kind, content, filename, file_sha256,
           file_size, read_by, truncated, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            kind.as_str(),
            text,
            doc.map(|(d, _)| d.filename.as_str()),
            doc.map(|(d, _)| d.sha256.as_str()),
            doc.map(|(d, _)| i64::try_from(d.size).unwrap_or(i64::MAX)),
            doc.map(|(_, by)| by.as_str()),
            doc.is_some_and(|(d, _)| d.truncated),
            at
        ],
    )?;
    tx.execute(
        "UPDATE assistant_conversation SET updated_at = ?2 WHERE id = ?1",
        params![id, at],
    )?;
    tx.commit()?;
    let s = by_id(v, id)?.ok_or_else(|| other(GONE))?;
    load(v, s)
}

/// Añade un dato escrito por el usuario (va con cada pregunta, anonimizado; TaxPain no lo
/// comprueba).
pub fn add_fact(
    v: &Vault,
    now: OffsetDateTime,
    topic: &Topic,
    conversation: Option<i64>,
    text: &str,
) -> Result<Conversation> {
    let t = text.trim();
    if t.is_empty() {
        return Err(other("Escribe el dato."));
    }
    if t.chars().count() > MAX_FACT_CHARS {
        return Err(other(
            "El dato es demasiado largo: como mucho 2.000 caracteres.",
        ));
    }
    if let Some(why) = taxpain_assistant::pii::looks_secret(t) {
        return Err(other(format!("No se añade: {why}.")));
    }
    add_material(v, now, topic, conversation, MaterialKind::Dato, t, None)
}

/// Extensiones de certificados y claves: nunca se leen para una conversación.
const KEY_EXTENSIONS: &[&str] = &["p12", "pfx", "pem", "key", "cer", "crt", "der", "p7b"];

/// Hojas de un libro que se leen, filas de cada una y caracteres de cada celda.
const MAX_SHEETS: usize = 5;
const SHEET_ROWS: usize = 300;
const SHEET_CELL_CHARS: usize = 200;

/// ¿Lleva caracteres de control que no tiene un texto (es un fichero binario con la extensión
/// de un CSV o de un texto)?
fn binary(bytes: &[u8]) -> bool {
    bytes
        .iter()
        .any(|b| matches!(b, 0x00..=0x08 | 0x0B | 0x0E..=0x19 | 0x1B..=0x1F))
}

/// Los nombres de unas hojas entre comillas: «Junio» y «Julio»; «Junio», «Julio» y «Agosto».
fn hojas(nombres: &[&str]) -> String {
    let q: Vec<String> = nombres.iter().map(|n| format!("«{n}»")).collect();
    match q.split_last() {
        Some((last, rest)) if !rest.is_empty() => format!("{} y {last}", rest.join(", ")),
        _ => q.concat(),
    }
}

/// Sin espacios al final de las líneas ni más de una línea en blanco seguida.
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank = false;
    for line in text.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            if blank || out.is_empty() {
                continue;
            }
            blank = true;
        } else {
            blank = false;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim_end().to_owned()
}

/// Lee en el Mac el texto de un documento (PDF con texto o escaneado, foto, hoja de cálculo,
/// CSV o texto). No toca el almacén ni la red: el fichero no sale del Mac ni se guarda.
pub fn read_document(filename: &str, bytes: &[u8]) -> Result<DocumentReading> {
    use sha2::{Digest, Sha256};

    if bytes.len() > MAX_FILE_BYTES {
        return Err(other("el fichero pasa de 20 MB: elige otro"));
    }
    let lower = filename.to_lowercase();
    let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
    // Un certificado o una clave se reconoce también por su contenido, en binario (DER o BER) o
    // en base64 (más abajo, en el texto).
    let es_clave = || other("Un certificado o una clave no se añade nunca a una conversación.");
    if KEY_EXTENSIONS.contains(&ext) || taxpain_assistant::pii::der_key_or_cert(bytes) {
        return Err(es_clave());
    }
    let unreadable = || {
        other(format!(
            "No se ha podido leer texto de «{filename}» en tu Mac."
        ))
    };
    let pdf = crate::lectura_ia::attachment_bytes(filename, bytes);
    // Lo que no se lee entero (hojas, filas, columnas o celdas de una hoja de cálculo).
    let mut sheet_cut = false;
    let text = if ext == "pdf" || pdf.starts_with(b"%PDF") {
        match crate::pdf_text::text(&pdf) {
            Some(t) if t.split_whitespace().count() > 3 => Some(t),
            _ => crate::carpeta::ocr_text(filename, &pdf),
        }
    } else if crate::received::is_image(&lower, bytes) {
        crate::carpeta::ocr_text(filename, bytes)
    } else if ["xlsx", "xlsm", "xls", "ods", "csv"].contains(&ext) {
        if ext == "csv" && binary(bytes) {
            return Err(unreadable());
        }
        let libro = crate::import::sheets_as_text(filename, bytes).map_err(|_| unreadable())?;
        let mut t = String::new();
        for h in libro.iter().take(MAX_SHEETS) {
            if !h.nombre.is_empty() {
                t.push_str(&format!("Hoja «{}»:\n", h.nombre));
            }
            let (texto, cut) =
                crate::lectura_ia::sheet_text_cut(&h.filas, SHEET_ROWS, SHEET_CELL_CHARS);
            t.push_str(&texto);
            for nota in cut.notes() {
                t.push_str(&nota);
                t.push('\n');
                sheet_cut = true;
            }
            t.push('\n');
        }
        if libro.len() > MAX_SHEETS {
            let resto: Vec<&str> = libro[MAX_SHEETS..]
                .iter()
                .map(|h| h.nombre.as_str())
                .collect();
            t.push_str(&format!("(Sin leer: las hojas {}.)\n", hojas(&resto)));
            sheet_cut = true;
        }
        Some(t)
    } else if ["txt", "md"].contains(&ext) {
        if binary(bytes) {
            return Err(unreadable());
        }
        Some(String::from_utf8(bytes.to_vec()).map_err(|_| unreadable())?)
    } else {
        return Err(other(
            "Ese tipo de fichero no se lee: usa PDF, una foto, Excel, CSV o texto.",
        ));
    };
    let text = tidy(&text.unwrap_or_default());
    if text.chars().filter(|c| !c.is_whitespace()).count() < 20 {
        return Err(unreadable());
    }
    if taxpain_assistant::pii::base64_key_or_cert(&text) {
        return Err(es_clave());
    }
    if let Some(why) = taxpain_assistant::pii::looks_secret(&text) {
        return Err(other(format!("No se añade: {why}.")));
    }
    let long = text.chars().count() > MAX_DOCUMENT_CHARS;
    let text = if long {
        text.chars().take(MAX_DOCUMENT_CHARS).collect()
    } else {
        text
    };
    Ok(DocumentReading {
        filename: filename.to_owned(),
        sha256: hex::encode(Sha256::digest(bytes)),
        size: bytes.len(),
        text,
        truncated: long || sheet_cut,
    })
}

/// Añade el texto de un documento (leído en el Mac con [`read_document`] o por la IA). Va con
/// cada pregunta, anonimizado y como documento externo, sin el nombre del fichero.
pub fn add_document(
    v: &Vault,
    now: OffsetDateTime,
    topic: &Topic,
    conversation: Option<i64>,
    d: &DocumentReading,
    by: ReadBy,
) -> Result<Conversation> {
    if let Some(why) = taxpain_assistant::pii::looks_secret(&d.text) {
        return Err(other(format!("No se añade: {why}.")));
    }
    let mut d = d.clone();
    if d.text.chars().count() > MAX_DOCUMENT_CHARS {
        d.text = d.text.chars().take(MAX_DOCUMENT_CHARS).collect();
        d.truncated = true;
    }
    add_material(
        v,
        now,
        topic,
        conversation,
        MaterialKind::Documento,
        &d.text,
        Some((&d, by)),
    )
}

/// Quita algo de lo añadido (deja de enviarse desde la próxima pregunta).
pub fn remove_material(v: &Vault, conversation: i64, material: i64) -> Result<Conversation> {
    let s = by_id(v, conversation)?.ok_or_else(|| other(GONE))?;
    let n = v.connection().execute(
        "DELETE FROM assistant_material WHERE id = ?1 AND conversation_id = ?2",
        params![material, conversation],
    )?;
    if n == 0 {
        return Err(other("Eso ya no está en la conversación."));
    }
    checkpoint(v);
    load(v, s)
}

/// Lo borrado no se queda en el diario del almacén (las páginas libres ya se sobrescriben con
/// `secure_delete`).
fn checkpoint(v: &Vault) {
    if let Err(e) = v
        .connection()
        .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
    {
        tracing::debug!(error = %e, "no se pudo vaciar el diario tras borrar una conversación");
    }
}

/// Borra una conversación con sus mensajes y lo que se añadió.
pub fn delete(v: &Vault, id: i64) -> Result<()> {
    let n = v
        .connection()
        .execute("DELETE FROM assistant_conversation WHERE id = ?1", [id])?;
    if n == 0 {
        return Err(other(GONE));
    }
    checkpoint(v);
    Ok(())
}

/// Borra todas las conversaciones de la empresa. Devuelve cuántas había.
pub fn delete_all(v: &Vault) -> Result<usize> {
    let n = v
        .connection()
        .execute("DELETE FROM assistant_conversation", [])?;
    checkpoint(v);
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clave_y_json_del_tema() {
        let t = Topic::Ahorro {
            id: "capitalizacion".into(),
            year: 2026,
        };
        assert_eq!(
            (t.kind(), t.key().as_str()),
            ("ahorro", "capitalizacion@2026")
        );
        assert_eq!(
            serde_json::to_value(&t).unwrap(),
            serde_json::json!({"kind": "ahorro", "id": "capitalizacion", "year": 2026})
        );
        let p: Topic = serde_json::from_str(r#"{"kind":"ahorro_plan","year":2026}"#).unwrap();
        assert_eq!(p, Topic::AhorroPlan { year: 2026 });
        assert_eq!((p.kind(), p.key().as_str()), ("ahorro_plan", "2026"));
        let g: Topic = serde_json::from_str(r#"{"kind":"general"}"#).unwrap();
        assert_eq!(g, Topic::General);
        assert_eq!((g.kind(), g.key().as_str()), ("general", ""));
        assert!(serde_json::from_str::<Topic>(r#"{"kind":"modelo"}"#).is_err());
        // El borrador de un modelo: el periodo es el trimestre, el del 202 o 0 en los anuales.
        let m: Topic =
            serde_json::from_str(r#"{"kind":"modelo","model":"303","year":2026,"period":3}"#)
                .unwrap();
        assert_eq!((m.kind(), m.key().as_str()), ("modelo", "303/2026/3"));
        assert!(m.validate().is_ok());
        for bien in [("390", 0), ("202", 3), ("130", 4)] {
            let t = Topic::Modelo {
                model: bien.0.into(),
                year: 2026,
                period: bien.1,
            };
            assert!(t.validate().is_ok(), "{t:?}");
        }
        for mal in [("303", 0), ("390", 1), ("202", 4), ("30A", 1), ("3030", 1)] {
            let t = Topic::Modelo {
                model: mal.0.into(),
                year: 2026,
                period: mal.1,
            };
            assert!(t.validate().is_err(), "{t:?}");
        }
        assert_eq!(
            new_title(&m, OffsetDateTime::UNIX_EPOCH),
            "Borrador del modelo 303 · 3T 2026"
        );
        // Un hallazgo: el código y el ejercicio se repiten, así que la clave lleva la huella del
        // título (corta: el título entero va en el tema).
        let h: Topic = serde_json::from_str(
            r#"{"kind":"hallazgo","origin":"aeat","code":"cruces.390_303","year":2024,
                "title":"El 390 de 2024 no cuadra con la suma de sus 303"}"#,
        )
        .unwrap();
        assert_eq!(h.kind(), "hallazgo");
        let k = h.key();
        assert!(k.starts_with("aeat/cruces.390_303/2024/"), "{k}");
        assert_eq!(k.len(), "aeat/cruces.390_303/2024/".len() + 12);
        let otro = Topic::Hallazgo {
            origin: Origen::Aeat,
            code: "cruces.390_303".into(),
            year: Some(2024),
            title: "El 390 de 2024 no cuadra con la suma de sus 303 (2.ª)".into(),
        };
        assert_ne!(otro.key(), k);
        let sin_año = Topic::Hallazgo {
            origin: Origen::Carpeta,
            code: "ficheros.copias".into(),
            year: None,
            title: "1 copia de un fichero con el mismo contenido".into(),
        };
        assert!(sin_año.key().starts_with("carpeta/ficheros.copias/-/"));
        assert_eq!(
            serde_json::to_value(&sin_año).unwrap(),
            serde_json::json!({"kind": "hallazgo", "origin": "carpeta", "code": "ficheros.copias",
                               "year": null,
                               "title": "1 copia de un fichero con el mismo contenido"})
        );
        assert!(h.validate().is_ok() && sin_año.validate().is_ok());
        for mal in [
            Topic::Hallazgo {
                origin: Origen::Aeat,
                code: "Cruces/390".into(),
                year: Some(2024),
                title: "x".into(),
            },
            Topic::Hallazgo {
                origin: Origen::Aeat,
                code: "cruces.390_303".into(),
                year: Some(1999),
                title: "x".into(),
            },
            Topic::Hallazgo {
                origin: Origen::Aeat,
                code: "cruces.390_303".into(),
                year: None,
                title: "  ".into(),
            },
            Topic::Hallazgo {
                origin: Origen::Aeat,
                code: "cruces.390_303".into(),
                year: None,
                title: "x".repeat(501),
            },
        ] {
            assert!(mal.validate().is_err(), "{mal:?}");
        }
        // Sólo fichas que existen, con un id corto y un ejercicio razonable.
        assert!(t.validate().is_ok());
        for mal in [
            Topic::Ahorro {
                id: "no_existe".into(),
                year: 2026,
            },
            Topic::Ahorro {
                id: "Capitalizacion".into(),
                year: 2026,
            },
            Topic::Ahorro {
                id: "capitalizacion".into(),
                year: 1999,
            },
            Topic::AhorroPlan { year: 2101 },
        ] {
            assert!(mal.validate().is_err(), "{mal:?}");
        }
    }

    fn msg(role: Role, chars: usize) -> StoredMessage {
        StoredMessage {
            seq: 0,
            role,
            content: "x".repeat(chars),
            created_at: String::new(),
            notes: None,
        }
    }

    #[test]
    fn historial_que_se_envia() {
        // Corto: va entero.
        let h: Vec<StoredMessage> = (0..10)
            .map(|i| {
                msg(
                    if i % 2 == 0 {
                        Role::User
                    } else {
                        Role::Assistant
                    },
                    10,
                )
            })
            .collect();
        assert_eq!(window(&h), 0);
        // 40 intervenciones: van las 30 últimas (empieza en una pregunta).
        let h: Vec<StoredMessage> = (0..40)
            .map(|i| {
                msg(
                    if i % 2 == 0 {
                        Role::User
                    } else {
                        Role::Assistant
                    },
                    10,
                )
            })
            .collect();
        assert_eq!(window(&h), 10);
        assert_eq!(h[window(&h)].role, Role::User);
        // Por caracteres: 4 de 9.000 no caben (30.000); la ventana no empieza en una respuesta.
        let h = vec![
            msg(Role::User, 9_000),
            msg(Role::Assistant, 9_000),
            msg(Role::User, 9_000),
            msg(Role::Assistant, 9_000),
        ];
        // Caben las 3 últimas (27.000), pero la primera de ellas es una respuesta: se quita.
        assert_eq!(window(&h), 2);
        // Nada guardado: nada que omitir.
        assert_eq!(window(&[]), 0);
    }

    #[test]
    fn titulos() {
        assert_eq!(
            title_from_question("  ¿Cuánto IVA\npago?  "),
            "¿Cuánto IVA pago?"
        );
        let largo = title_from_question(
            "¿Qué pasa si este año reparto dividendos y además doto la reserva de \
             capitalización con lo que queda?",
        );
        assert_eq!(
            largo,
            "¿Qué pasa si este año reparto dividendos y además doto la…"
        );
        assert!(largo.chars().count() <= 61);
        assert!(default_title("Conversación del 31/10/2026"));
        assert!(!default_title("Conversación del lunes"));
    }
}
