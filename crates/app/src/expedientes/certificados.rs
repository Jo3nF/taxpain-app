//! Certificados administrativos y apoderamientos con caducidad (carencia G27): estar al
//! corriente con la AEAT o con la Seguridad Social, residencia fiscal, situación censal,
//! apoderamientos en el registro de la AEAT o en el de la DEHú (REA)… Se anotan a mano o desde
//! un certificado de Documentos AEAT, y el Panel y el Mac avisan antes de que caduquen.
//!
//! El certificado electrónico (el de firmar) ya avisa en el Panel desde Ajustes →
//! Certificados: aquí no se repite.

use serde::{Deserialize, Serialize};
use taxpain_core::calendar::rules::add_months;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{self, OptionalExtension, params};
use time::{Date, OffsetDateTime};

use super::{Aviso, fecha, other, parse_fecha};
use crate::documentos::DocKind;
use crate::error::{AppError, Result};
use crate::invoicing::now_str;
use crate::notices::NoticeReminder;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TipoCertificado {
    CorrienteAeat,
    CorrienteSs,
    ResidenciaFiscal,
    SituacionCensal,
    Apoderamiento,
    Otro,
}

impl TipoCertificado {
    fn code(self) -> &'static str {
        match self {
            TipoCertificado::CorrienteAeat => "corriente_aeat",
            TipoCertificado::CorrienteSs => "corriente_ss",
            TipoCertificado::ResidenciaFiscal => "residencia_fiscal",
            TipoCertificado::SituacionCensal => "situacion_censal",
            TipoCertificado::Apoderamiento => "apoderamiento",
            TipoCertificado::Otro => "otro",
        }
    }

    fn from_code(s: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(s.to_owned())).ok()
    }

    pub fn nombre(self) -> &'static str {
        match self {
            TipoCertificado::CorrienteAeat => "Certificado de estar al corriente con la AEAT",
            TipoCertificado::CorrienteSs => {
                "Certificado de estar al corriente con la Seguridad Social"
            }
            TipoCertificado::ResidenciaFiscal => "Certificado de residencia fiscal",
            TipoCertificado::SituacionCensal => "Certificado de situación censal",
            TipoCertificado::Apoderamiento => "Apoderamiento",
            TipoCertificado::Otro => "Certificado",
        }
    }

    /// Días antes de caducar en que se avisa: 30 para un apoderamiento (renovarlo pide la
    /// firma del poderdante), 15 para los demás.
    pub fn aviso_dias(self) -> i64 {
        if self == TipoCertificado::Apoderamiento {
            30
        } else {
            15
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CertificadoAdmin {
    #[serde(default)]
    pub id: i64,
    pub tipo: TipoCertificado,
    #[serde(default)]
    pub descripcion: String,
    #[serde(default)]
    pub emitido_el: Option<Date>,
    /// Hasta cuándo vale (obligatorio al guardar).
    #[serde(default)]
    pub caduca_el: Option<Date>,
    #[serde(default)]
    pub document_id: Option<i64>,
    #[serde(default)]
    pub notas: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CertificadoVista {
    pub certificado: CertificadoAdmin,
    /// Días naturales hasta que caduca (negativo: ya caducó).
    pub dias: Option<i64>,
    /// `vigente`, `caduca_pronto` o `caducado`.
    pub estado: &'static str,
}

fn vista(c: CertificadoAdmin, hoy: Date) -> CertificadoVista {
    let dias = c.caduca_el.map(|d| (d - hoy).whole_days());
    let estado = match dias {
        Some(d) if d < 0 => "caducado",
        Some(d) if d <= c.tipo.aviso_dias() => "caduca_pronto",
        _ => "vigente",
    };
    CertificadoVista {
        certificado: c,
        dias,
        estado,
    }
}

pub fn list(v: &Vault, hoy: Date) -> Result<Vec<CertificadoVista>> {
    let mut stmt = v.connection().prepare(
        "SELECT id, tipo, descripcion, emitido_el, caduca_el, document_id, notas
         FROM certificado_admin ORDER BY caduca_el, id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(CertificadoAdmin {
            id: r.get(0)?,
            tipo: TipoCertificado::from_code(&r.get::<_, String>(1)?)
                .unwrap_or(TipoCertificado::Otro),
            descripcion: r.get(2)?,
            emitido_el: r
                .get::<_, Option<String>>(3)?
                .as_deref()
                .and_then(parse_fecha),
            caduca_el: parse_fecha(&r.get::<_, String>(4)?),
            document_id: r.get(5)?,
            notas: r.get(6)?,
        })
    })?;
    Ok(rows
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .map(|c| vista(c, hoy))
        .collect())
}

/// Guarda (alta si `id` es 0) y devuelve el id.
pub fn save(v: &Vault, c: &CertificadoAdmin, now: OffsetDateTime) -> Result<i64> {
    let caduca = c
        .caduca_el
        .ok_or_else(|| other("falta la fecha de caducidad"))?;
    if let Some(e) = c.emitido_el
        && e > caduca
    {
        return Err(other("la caducidad no puede ser anterior a la emisión"));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let p = params![
        c.tipo.code(),
        c.descripcion.trim(),
        c.emitido_el.map(|d| d.to_string()),
        caduca.to_string(),
        c.document_id,
        c.notas.trim(),
        now_str(now),
    ];
    let id = if c.id == 0 {
        tx.execute(
            "INSERT INTO certificado_admin(tipo, descripcion, emitido_el, caduca_el, document_id,
               notas, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            p,
        )?;
        tx.last_insert_rowid()
    } else {
        let n = tx.execute(
            &format!(
                "UPDATE certificado_admin SET tipo = ?1, descripcion = ?2, emitido_el = ?3,
                   caduca_el = ?4, document_id = ?5, notas = ?6, updated_at = ?7
                 WHERE id = {}",
                c.id
            ),
            p,
        )?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        c.id
    };
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "certificado_admin.save",
        &id.to_string(),
        &serde_json::json!({ "tipo": c.tipo.code(), "caduca_el": caduca.to_string() }),
    )?;
    tx.commit()?;
    Ok(id)
}

/// Nombre del certificado (para el aviso de Touch ID).
pub fn name(v: &Vault, id: i64) -> Result<Option<String>> {
    let row: Option<(String, String)> = v
        .connection()
        .query_row(
            "SELECT tipo, descripcion FROM certificado_admin WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(row.map(|(t, d)| {
        let t = TipoCertificado::from_code(&t).unwrap_or(TipoCertificado::Otro);
        if d.is_empty() {
            t.nombre().to_owned()
        } else {
            format!("{} ({d})", t.nombre())
        }
    }))
}

pub fn delete(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let nombre = name(v, id)?.ok_or(AppError::NotFound)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM certificado_admin WHERE id = ?1", [id])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "certificado_admin.delete",
        &id.to_string(),
        &serde_json::json!({ "nombre": nombre }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Validez del certificado de estar al corriente con la AEAT si no dice otra: doce meses desde
/// que se expide (art. 75.2 RGAT, RD 1065/2007).
pub const MESES_CORRIENTE_AEAT: u32 = 12;

/// Borrador (sin guardar) con lo que se lee de un certificado de Documentos AEAT: tipo, fecha
/// de emisión y, si se sabe, la caducidad (la que dice el certificado o la de la norma).
pub fn desde_documento(v: &Vault, document_id: i64) -> Result<CertificadoAdmin> {
    let doc = crate::documentos::list(v)?
        .into_iter()
        .find(|d| d.id == document_id)
        .ok_or(AppError::NotFound)?;
    let tipo = match doc.kind {
        DocKind::CertificadoAeat => TipoCertificado::CorrienteAeat,
        DocKind::CertificadoSeguridadSocial => TipoCertificado::CorrienteSs,
        DocKind::CertificadoResidencia => TipoCertificado::ResidenciaFiscal,
        DocKind::CertificadoSituacionCensal => TipoCertificado::SituacionCensal,
        _ => TipoCertificado::Otro,
    };
    let o = crate::documentos::text(v, document_id)
        .map(|t| crate::oficiales::leer(&t))
        .unwrap_or_default();
    let emitido = o.fecha.or(doc.date);
    // Sólo se propone la caducidad que se sabe: la del certificado o la del art. 75.2 RGAT.
    let meses = o.validez_meses.or(match tipo {
        TipoCertificado::CorrienteAeat => Some(MESES_CORRIENTE_AEAT),
        _ => None,
    });
    let caduca = emitido
        .zip(meses)
        .map(|(e, m)| add_months(e, i32::try_from(m).unwrap_or(12)));
    Ok(CertificadoAdmin {
        id: 0,
        tipo,
        descripcion: doc.title.clone(),
        emitido_el: emitido,
        caduca_el: caduca,
        document_id: Some(document_id),
        notas: if caduca.is_none() {
            "Teclea hasta cuándo vale: lo dice el propio certificado.".into()
        } else {
            String::new()
        },
    })
}

/// Avisos del Mac: a los días de aviso del tipo, a los 7, al día siguiente y el de caducar.
pub fn reminders_due(v: &Vault, hoy: Date) -> Result<Vec<NoticeReminder>> {
    let mut out = Vec::new();
    for c in list(v, hoy)? {
        let (Some(d), Some(hasta)) = (c.dias, c.certificado.caduca_el) else {
            continue;
        };
        if ![c.certificado.tipo.aviso_dias(), 7, 1, 0].contains(&d) {
            continue;
        }
        out.push(NoticeReminder {
            key: format!("certificado_admin|{}|{hasta}|{d}", c.certificado.id),
            title: format!("{} caduca", c.certificado.tipo.nombre()),
            body: format!(
                "{} vale hasta el {}: pide otro antes si lo vas a necesitar.",
                if c.certificado.descripcion.is_empty() {
                    c.certificado.tipo.nombre()
                } else {
                    &c.certificado.descripcion
                },
                fecha(hasta)
            ),
        });
    }
    Ok(out)
}

/// Avisos del Panel: caducados en los últimos 30 días o que caducan pronto.
pub fn avisos(v: &Vault, hoy: Date) -> Result<Vec<Aviso>> {
    Ok(list(v, hoy)?
        .into_iter()
        .filter_map(|c| {
            let d = c.dias?;
            let hasta = c.certificado.caduca_el?;
            if c.estado == "vigente" || d < -30 {
                return None;
            }
            Some(Aviso {
                texto: format!(
                    "{} {} el {}",
                    c.certificado.tipo.nombre(),
                    if d < 0 { "caducó" } else { "caduca" },
                    fecha(hasta)
                ),
                fecha: hasta,
                dias: d,
                urgente: d <= 3,
            })
        })
        .collect())
}
