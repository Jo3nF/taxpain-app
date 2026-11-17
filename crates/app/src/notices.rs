//! Comunicaciones y notificaciones de la AEAT guardadas en el almacén.
//!
//! La app (que es quien habla con la AEAT) pasa aquí lo que devuelve el servicio GNNO-JDIT: los
//! metadatos públicos al consultar y el contenido al acceder. Comparecer a una notificación abre
//! sus plazos (art. 43 Ley 39/2015), así que queda en la auditoría como acto del usuario.

use serde::{Deserialize, Serialize};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{self, OptionalExtension, params};
use time::{Date, Duration, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing;

/// Metadatos públicos de un envío (consulta).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoticeMeta {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub role: String,
    pub subject: String,
    pub available_at: String,
    pub holder_nif: String,
}

/// Contenido de un envío accedido.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NoticeContent {
    pub metadata: Vec<(String, String)>,
    pub document_pdf: Vec<u8>,
    pub certification_pdf: Vec<u8>,
}

/// Envío guardado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Notice {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub subject: String,
    pub available_at: String,
    pub environment: String,
    pub accessed_at: Option<String>,
    /// Metadatos privados (concepto, modelo, importe…) si ya se accedió.
    pub metadata: Vec<(String, String)>,
    pub has_content: bool,
    /// Notificación pendiente: día en que se dará por notificada si no se abre (diez días
    /// naturales desde la puesta a disposición, Ley 39/2015 art. 43.2).
    pub deemed_notified_on: Option<Date>,
}

/// Días naturales para abrir una notificación electrónica antes de que se entienda rechazada
/// (Ley 39/2015 art. 43.2; LGT art. 104). Agosto cuenta: no hay días inhábiles aquí.
pub const ACCESS_DAYS: i64 = 10;

/// Día en que se entiende rechazada (y practicada) una notificación puesta a disposición el
/// `available_at` (`AAAA-MM-DDThh:mm:ss`). Ley 39/2015 art. 43.2 (leído en el BOE el
/// 2026-10-27): «se entenderá rechazada cuando hayan transcurrido diez días naturales desde la
/// puesta a disposición de la notificación sin que se acceda a su contenido». TODO(VERIFY): ni
/// la ley ni las preguntas frecuentes de notificaciones de la Sede dicen la hora en que vence
/// (las resoluciones del TEAC fechan el rechazo «a las 00:00» del día siguiente al décimo, lo
/// que apunta a que el décimo día cuenta entero); por prudencia la app pide abrirla como tarde la
/// víspera.
pub fn deemed_notified_on(available_at: &str) -> Option<Date> {
    let day = available_at.get(..10)?;
    let d = Date::parse(
        day,
        time::macros::format_description!("[year]-[month]-[day]"),
    )
    .ok()?;
    Some(d + Duration::days(ACCESS_DAYS))
}

/// Aviso del sistema para una notificación pendiente.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoticeReminder {
    /// Clave para no repetir el aviso.
    pub key: String,
    pub title: String,
    pub body: String,
}

/// Avisos de hoy: notificaciones pendientes a las que les quedan 5, 2 o 0 días para el último
/// día prudente para abrirlas (la víspera del día en que se dan por notificadas).
pub fn reminders_due(v: &Vault, today: Date) -> Result<Vec<NoticeReminder>> {
    let mut out = Vec::new();
    for n in list(v)? {
        let Some(deemed) = n.deemed_notified_on else {
            continue;
        };
        let open_by = deemed - Duration::days(1);
        let left = (open_by - today).whole_days();
        if ![5, 2, 0].contains(&left) {
            continue;
        }
        let when = if left == 0 {
            "hoy es el último día prudente para abrirla".to_owned()
        } else {
            format!("ábrela antes del {}", es(open_by))
        };
        out.push(NoticeReminder {
            key: format!("notificacion|{}|{left}", n.id),
            title: "Notificación de la AEAT sin abrir".into(),
            body: format!(
                "«{}»: {when}. El {} se dará por notificada aunque no la abras y empezarán \
                 los plazos para contestar o recurrir.",
                n.subject,
                es(deemed)
            ),
        });
    }
    Ok(out)
}

fn es(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

/// Clave de los días de cortesía en los ajustes de la empresa.
const COURTESY_KEY: &str = "aeat_dias_cortesia";

/// Máximo de días de cortesía por año natural (RD 1363/2010, DA 3.ª).
pub const COURTESY_MAX: usize = 30;

/// Días de cortesía marcados en la Sede (en ellos la AEAT no pone notificaciones a
/// disposición). Sólo es un recordatorio: se marcan en la Sede, no desde TaxPain.
pub fn courtesy_days(v: &Vault) -> Result<Vec<Date>> {
    Ok(v.setting::<Vec<Date>>(COURTESY_KEY)?.unwrap_or_default())
}

/// Guarda los días de cortesía (ordenados y sin repetir); no más de 30 por año natural.
pub fn set_courtesy_days(v: &Vault, days: &[Date]) -> Result<Vec<Date>> {
    let mut days = days.to_vec();
    days.sort();
    days.dedup();
    let mut years: std::collections::BTreeMap<i32, usize> = std::collections::BTreeMap::new();
    for d in &days {
        *years.entry(d.year()).or_default() += 1;
    }
    if let Some((y, n)) = years.into_iter().find(|(_, n)| *n > COURTESY_MAX) {
        return Err(AppError::Other(format!(
            "en {y} hay {n} días de cortesía: el máximo son {COURTESY_MAX} al año"
        )));
    }
    v.set_setting(COURTESY_KEY, &days)?;
    Ok(days)
}

impl Notice {
    /// Notificación pendiente de comparecer: es lo urgente.
    pub fn is_pending_notification(&self) -> bool {
        self.kind == "N" && self.state == "P"
    }
}

fn env_name(production: bool) -> &'static str {
    if production { "produccion" } else { "pruebas" }
}

/// Guarda o actualiza los envíos consultados. Devuelve cuántos son nuevos.
pub fn record_listed(
    v: &Vault,
    production: bool,
    list: &[NoticeMeta],
    now: OffsetDateTime,
) -> Result<usize> {
    let conn = v.connection();
    let mut new = 0;
    for n in list {
        let exists: Option<String> = conn
            .query_row(
                "SELECT state FROM aeat_notice WHERE id = ?1",
                [&n.id],
                |r| r.get(0),
            )
            .optional()?;
        match exists {
            Some(_) => {
                conn.execute(
                    "UPDATE aeat_notice SET state = ?2, subject = ?3 WHERE id = ?1",
                    params![n.id, n.state, n.subject],
                )?;
            }
            None => {
                new += 1;
                conn.execute(
                    "INSERT INTO aeat_notice(id, kind, state, role, subject, available_at, holder_nif,
                       environment, seen_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        n.id,
                        n.kind,
                        n.state,
                        n.role,
                        n.subject,
                        n.available_at,
                        n.holder_nif,
                        env_name(production),
                        invoicing::now_str(now)
                    ],
                )?;
            }
        }
    }
    Ok(new)
}

/// Deja sólo caracteres seguros para un nombre de fichero (los ids y CSV vienen de la AEAT).
pub fn safe_file_component(s: &str) -> String {
    let out: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        .take(64)
        .collect();
    if out.is_empty() { "sin_id".into() } else { out }
}

/// Nombre del PDF guardado de un envío.
pub fn pdf_file_name(id: &str, certification: bool) -> String {
    let kind = if certification {
        "comparecencia"
    } else {
        "notificacion"
    };
    format!("{kind}_{}.pdf", safe_file_component(id))
}

/// Antes de comparecer: el envío debe existir en local y ser una notificación pendiente. Deja
/// constancia en la auditoría **antes** de llamar a la AEAT, para que el acto quede registrado
/// aunque la respuesta se pierda.
pub fn begin_appear(v: &Vault, id: &str, now: OffsetDateTime) -> Result<()> {
    let row: Option<(String, String)> = v
        .connection()
        .query_row(
            "SELECT kind, state FROM aeat_notice WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match row {
        None => return Err(AppError::NotFound),
        Some((k, st)) if k != "N" || st != "P" => {
            return Err(AppError::Other(
                "sólo se comparece a notificaciones pendientes".into(),
            ));
        }
        Some(_) => {}
    }
    audit::append(
        v.connection(),
        &invoicing::now_str(now),
        Actor::User,
        "notices.appear.request",
        id,
        &serde_json::json!({}),
    )?;
    Ok(())
}

/// Guarda el contenido de un envío accedido. `appeared`: se compareció (abre plazos).
pub fn record_content(
    v: &Vault,
    id: &str,
    content: &NoticeContent,
    appeared: bool,
    now: OffsetDateTime,
) -> Result<()> {
    let conn = v.connection();
    for pdf in [&content.document_pdf, &content.certification_pdf] {
        if !pdf.is_empty() && !pdf.starts_with(b"%PDF") {
            if appeared {
                // La comparecencia ya se produjo en la AEAT: queda constancia aunque el
                // contenido no sirva (se podrá descargar de nuevo con la operación D).
                conn.execute(
                    "UPDATE aeat_notice SET state = 'A', accessed_at = ?2 WHERE id = ?1",
                    params![id, invoicing::now_str(now)],
                )?;
                audit::append(
                    conn,
                    &invoicing::now_str(now),
                    Actor::User,
                    "notices.appear",
                    id,
                    &serde_json::json!({ "error": "contenido no PDF" }),
                )?;
            }
            return Err(AppError::Other("el contenido recibido no es un PDF".into()));
        }
    }
    let already: Option<Option<i64>> = conn
        .query_row(
            "SELECT document_id FROM aeat_notice WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    match already {
        None => return Err(AppError::NotFound),
        Some(Some(_)) => return Ok(()), // ya guardado: el contenido no cambia
        Some(None) => {}
    }
    let tx = conn.unchecked_transaction()?;
    let store = |kind: &str, bytes: &[u8]| -> Result<Option<i64>> {
        if bytes.is_empty() {
            return Ok(None);
        }
        Ok(Some(invoicing::store_document(
            &tx,
            kind,
            &format!("{kind}_{id}.pdf"),
            "application/pdf",
            bytes,
            now,
        )?))
    };
    let doc = store("notificacion_aeat", &content.document_pdf)?;
    let cert = store("comparecencia_aeat", &content.certification_pdf)?;
    let metadata =
        serde_json::to_string(&content.metadata).map_err(|e| AppError::Other(e.to_string()))?;
    tx.execute(
        "UPDATE aeat_notice SET state = 'A', accessed_at = ?2, metadata = ?3, document_id = ?4,
           certification_id = ?5 WHERE id = ?1",
        params![id, invoicing::now_str(now), metadata, doc, cert],
    )?;
    audit::append(
        &tx,
        &invoicing::now_str(now),
        Actor::User,
        if appeared {
            "notices.appear"
        } else {
            "notices.download"
        },
        id,
        &serde_json::json!({ "csv": content.metadata.iter().find(|(k, _)| k == "CSVDocumentoPDF").map(|(_, v)| v) }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Envíos guardados: primero las notificaciones pendientes, luego por fecha.
pub fn list(v: &Vault) -> Result<Vec<Notice>> {
    let mut stmt = v.connection().prepare(
        "SELECT id, kind, state, subject, available_at, environment, accessed_at, metadata,
                document_id IS NOT NULL
         FROM aeat_notice
         ORDER BY (kind = 'N' AND state = 'P') DESC, available_at DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        let metadata: Option<String> = r.get(7)?;
        let (kind, state, available_at): (String, String, String) =
            (r.get(1)?, r.get(2)?, r.get(4)?);
        let deemed_notified_on = (kind == "N" && state == "P")
            .then(|| deemed_notified_on(&available_at))
            .flatten();
        Ok(Notice {
            id: r.get(0)?,
            kind,
            state,
            subject: r.get(3)?,
            available_at,
            environment: r.get(5)?,
            accessed_at: r.get(6)?,
            metadata: metadata
                .and_then(|m| serde_json::from_str(&m).ok())
                .unwrap_or_default(),
            has_content: r.get(8)?,
            deemed_notified_on,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

/// PDF guardado de un envío: el documento o la certificación de comparecencia.
pub fn pdf(v: &Vault, id: &str, certification: bool) -> Result<Option<Vec<u8>>> {
    let column = if certification {
        "certification_id"
    } else {
        "document_id"
    };
    Ok(v.connection()
        .query_row(
            &format!(
                "SELECT d.content FROM aeat_notice n JOIN document d ON d.id = n.{column} WHERE n.id = ?1"
            ),
            [id],
            |r| r.get::<_, Vec<u8>>(0),
        )
        .optional()?)
}
