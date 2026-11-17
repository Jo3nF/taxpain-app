//! Entidades en régimen de atribución de rentas (comunidades de bienes, sociedades civiles sin
//! personalidad a efectos del IS): ficha de socios o comuneros con su participación y datos de
//! la entidad para el modelo 184 (`models::draft_184`).

use serde::{Deserialize, Serialize};
use taxpain_core::Nif;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::params;
use time::{Date, Month, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;

const SETTINGS_KEY: &str = "m184";

/// Socio, comunero, heredero o partícipe.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Partner {
    #[serde(default)]
    pub id: i64,
    pub nif: String,
    pub name: String,
    /// Código de provincia del domicilio (01-52).
    pub province: String,
    #[serde(default)]
    pub address: String,
    /// Participación en diezmilésimas de punto (50 % = 500.000).
    pub pct: u32,
    #[serde(default)]
    pub joined_on: Option<Date>,
    #[serde(default)]
    pub left_on: Option<Date>,
}

/// Datos de la entidad para el 184 que no salen de la contabilidad.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings184 {
    /// 1 sociedad civil, 2 comunidad de bienes, 3 herencia yacente, 5 otras.
    pub entity_type: u8,
    /// 1 empresarial, 2 profesional, 3 alquiler de inmuebles.
    pub main_activity: u8,
    /// Estimación directa: 1 normal, 2 simplificada.
    pub regime: u8,
    /// Tipo de actividad (1-5, ver `m184::Activity184::kind`).
    pub activity_kind: u8,
    /// Epígrafe del IAE.
    pub iae: String,
    /// Criterio de cobros y pagos.
    pub cash: bool,
    pub representative_nif: String,
    pub representative_name: String,
    /// Alquiler: situación (1-4) y referencia catastral del inmueble, urbano y días alquilado.
    pub rent_situation: u8,
    pub cadastral_ref: String,
    pub urban: bool,
    pub rent_days: u16,
}

impl Default for Settings184 {
    fn default() -> Self {
        Settings184 {
            entity_type: 2,
            main_activity: 1,
            regime: 2,
            activity_kind: 1,
            iae: String::new(),
            cash: false,
            representative_nif: String::new(),
            representative_name: String::new(),
            rent_situation: 1,
            cadastral_ref: String::new(),
            urban: true,
            rent_days: 365,
        }
    }
}

pub fn settings(v: &Vault) -> Result<Settings184> {
    Ok(v.setting(SETTINGS_KEY)?.unwrap_or_default())
}

pub fn save_settings(v: &Vault, s: &Settings184, now: OffsetDateTime) -> Result<()> {
    if !matches!(s.entity_type, 1..=5)
        || !matches!(s.main_activity, 1..=3)
        || !matches!(s.regime, 1 | 2)
        || !matches!(s.activity_kind, 1..=5)
        || !matches!(s.rent_situation, 1..=4)
    {
        return Err(AppError::Other("revisa los datos de la entidad".into()));
    }
    v.set_setting(SETTINGS_KEY, s)?;
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        "m184.settings",
        "184",
        &serde_json::json!({}),
    )?;
    Ok(())
}

fn parse_date(s: Option<String>) -> Option<Date> {
    s.and_then(|d| {
        Date::parse(
            &d,
            time::macros::format_description!("[year]-[month]-[day]"),
        )
        .ok()
    })
}

pub fn partners(v: &Vault) -> Result<Vec<Partner>> {
    let mut st = v.connection().prepare(
        "SELECT id, nif, name, province, address, pct, joined_on, left_on FROM partner ORDER BY id",
    )?;
    let rows = st.query_map([], |r| {
        Ok(Partner {
            id: r.get(0)?,
            nif: r.get(1)?,
            name: r.get(2)?,
            province: r.get(3)?,
            address: r.get(4)?,
            pct: r.get(5)?,
            joined_on: parse_date(r.get(6)?),
            left_on: parse_date(r.get(7)?),
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Da de alta (id 0) o modifica un socio. Devuelve su id.
pub fn save_partner(v: &Vault, p: &Partner, now: OffsetDateTime) -> Result<i64> {
    let nif = p.nif.trim().to_uppercase();
    Nif::parse(&nif).map_err(|e| AppError::Other(format!("NIF del socio: {e}")))?;
    if p.name.trim().is_empty() {
        return Err(AppError::Other("pon el nombre del socio".into()));
    }
    let province = p.province.trim();
    let prov_ok =
        province.len() == 2 && province.parse::<u8>().is_ok_and(|n| (1..=52).contains(&n));
    if !prov_ok {
        return Err(AppError::Other(
            "la provincia es el código de dos cifras (01 a 52)".into(),
        ));
    }
    if p.pct == 0 || p.pct > taxpain_models::m184::FULL_PCT {
        return Err(AppError::Other(
            "la participación va de 0,0001 % a 100 %".into(),
        ));
    }
    if let (Some(a), Some(b)) = (p.joined_on, p.left_on)
        && b < a
    {
        return Err(AppError::Other("la baja es anterior al alta".into()));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let id = if p.id == 0 {
        tx.execute(
            "INSERT INTO partner(nif, name, province, address, pct, joined_on, left_on, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                nif,
                p.name.trim(),
                province,
                p.address.trim(),
                p.pct,
                p.joined_on.map(|d| d.to_string()),
                p.left_on.map(|d| d.to_string()),
                now_str(now)
            ],
        )?;
        tx.last_insert_rowid()
    } else {
        let n = tx.execute(
            "UPDATE partner SET nif = ?1, name = ?2, province = ?3, address = ?4, pct = ?5,
               joined_on = ?6, left_on = ?7 WHERE id = ?8",
            params![
                nif,
                p.name.trim(),
                province,
                p.address.trim(),
                p.pct,
                p.joined_on.map(|d| d.to_string()),
                p.left_on.map(|d| d.to_string()),
                p.id
            ],
        )?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        p.id
    };
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "partner.save",
        &nif,
        &serde_json::json!({ "pct": p.pct }),
    )?;
    tx.commit()?;
    Ok(id)
}

pub fn delete_partner(v: &Vault, id: i64, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let n = tx.execute("DELETE FROM partner WHERE id = ?1", [id])?;
    if n == 0 {
        return Err(AppError::NotFound);
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "partner.delete",
        &id.to_string(),
        &serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

/// Socios con algún día en el año, como los pide el 184 (días y si siguen a 31 de diciembre).
pub fn partners_184(v: &Vault, year: i32) -> Result<Vec<taxpain_models::m184::Partner184>> {
    let first = Date::from_calendar_date(year, Month::January, 1)
        .map_err(|e| AppError::Other(e.to_string()))?;
    let last = Date::from_calendar_date(year, Month::December, 31)
        .map_err(|e| AppError::Other(e.to_string()))?;
    let mut out = Vec::new();
    for p in partners(v)? {
        let from = p.joined_on.unwrap_or(first).max(first);
        let to = p.left_on.unwrap_or(last).min(last);
        if to < from {
            continue;
        }
        out.push(taxpain_models::m184::Partner184 {
            nif: p.nif,
            name: p.name,
            province: p.province,
            address: p.address,
            pct: p.pct,
            member_at_end: p.left_on.is_none_or(|d| d >= last),
            days: u16::try_from((to - from).whole_days() + 1).unwrap_or(366),
        });
    }
    Ok(out)
}
