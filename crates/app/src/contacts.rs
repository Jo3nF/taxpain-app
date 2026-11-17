//! Clientes y proveedores: la ficha de cada uno (tabla `contact`: datos fiscales, domicilio,
//! correo, idioma de sus facturas y notas) y su historial, calculado de las facturas emitidas y
//! recibidas y de la cartera. Sin ficha, un cliente o proveedor sale igual en la lista a partir
//! de sus facturas; la ficha se crea sola al emitirle una factura, o a mano.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::eu::vies_prefix;
use taxpain_core::invoice::{IdType, Party, PartyId};
use taxpain_core::money::Money;
use taxpain_core::nif::Nif;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;
use crate::settings::Language;

const ISO_DATE: &[time::format_description::BorrowedFormatItem<'_>] =
    time::macros::format_description!("[year]-[month]-[day]");

/// Países de la UE (prefijo del NIF-IVA: Grecia es `EL`; `XI`, Irlanda del Norte, para bienes).
pub const EU: &[&str] = crate::received::EU_COUNTRIES;

/// Lo que se guarda de cada cliente o proveedor (`contact.data`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContactData {
    pub party: Party,
    #[serde(default)]
    pub email: Option<String>,
    /// Idioma de sus facturas (si no, el de los ajustes).
    #[serde(default)]
    pub language: Option<Language>,
    #[serde(default)]
    pub notes: Option<String>,
    /// Última comprobación del NIF-IVA en VIES (prueba para no repercutir IVA).
    #[serde(default)]
    pub vies: Option<ViesProof>,
}

/// Resultado guardado de una consulta a VIES.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViesProof {
    pub valid: bool,
    pub checked_at: String,
    /// Número de consulta que da VIES (la prueba).
    pub request_id: Option<String>,
    pub name: Option<String>,
}

/// Ficha que llega de la interfaz.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContactInput {
    /// `None` para una ficha nueva.
    pub id: Option<i64>,
    /// `cliente`, `proveedor` o `ambos`.
    pub role: String,
    pub data: ContactData,
}

/// Fila de la lista: la ficha (si hay) y las cifras de sus facturas.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ContactRow {
    /// Clave estable: el identificador fiscal normalizado (con el prefijo del país si es de
    /// fuera de España).
    pub key: String,
    /// Id de la ficha, si la tiene.
    pub id: Option<i64>,
    pub role: String,
    pub name: String,
    pub tax_id: String,
    pub country: String,
    pub email: Option<String>,
    /// Facturado (total de las emitidas, sin anuladas) en el año pedido y en total.
    pub invoiced_year: Money,
    pub invoiced_total: Money,
    /// Comprado (total de las recibidas revisadas) en el año pedido y en total.
    pub purchased_year: Money,
    pub purchased_total: Money,
    /// Pendiente de cobro y de pago según la cartera, y la parte vencida.
    pub to_collect: Money,
    pub to_pay: Money,
    pub overdue: Money,
    pub last_date: Option<Date>,
    /// Facturas emitidas y recibidas.
    pub invoices: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HistoryItem {
    /// `emitida` o `recibida`.
    pub kind: String,
    pub id: i64,
    pub number: String,
    pub date: Date,
    pub total: Money,
    pub status: String,
    /// Lo que queda por cobrar o pagar de ella.
    pub pending: Money,
    /// La parte de lo pendiente ya vencida (los plazos que aún no vencen no cuentan).
    pub overdue: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct YearTotals {
    pub year: i32,
    pub invoiced: Money,
    pub purchased: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ContactDetail {
    pub row: ContactRow,
    /// La ficha guardada.
    pub data: Option<ContactData>,
    /// Datos para una factura nueva: los de la ficha o, si no hay, los de su última factura.
    pub party: Option<Party>,
    pub history: Vec<HistoryItem>,
    pub years: Vec<YearTotals>,
}

/// Solo letras y cifras, en mayúsculas.
fn norm(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_uppercase()
}

/// Clave de un tercero a partir de su país y su identificador.
pub fn key(country: &str, id: &str) -> String {
    let id = norm(id);
    let c = country.trim().to_uppercase();
    if c.is_empty() || c == "ES" {
        // «ESB12345674» es el NIF-IVA de un NIF español: la clave es el NIF.
        return match id.strip_prefix("ES") {
            Some(rest) if rest.len() == 9 && Nif::parse(rest).is_ok() => rest.to_owned(),
            _ => id,
        };
    }
    let p = vies_prefix(&c);
    if id.starts_with(&p) {
        id
    } else {
        format!("{p}{id}")
    }
}

/// País e identificador de una parte.
pub fn party_key(p: &PartyId) -> (String, String) {
    match p {
        PartyId::Nif(n) => ("ES".into(), key("ES", n)),
        PartyId::Foreign { country, id, .. } => (country.to_uppercase(), key(country, id)),
    }
}

fn valid_role(role: &str) -> Result<&str> {
    match role {
        "cliente" | "proveedor" | "ambos" => Ok(role),
        _ => Err(AppError::Other("tipo de contacto no válido".into())),
    }
}

/// Comprueba y limpia la ficha.
pub fn normalize(d: &ContactData) -> Result<ContactData> {
    let mut d = d.clone();
    d.party.name = d.party.name.trim().to_owned();
    if d.party.name.is_empty() {
        return Err(AppError::Other("pon el nombre o la razón social".into()));
    }
    d.party.id = match &d.party.id {
        PartyId::Nif(n) => {
            let n = norm(n);
            let n = n
                .strip_prefix("ES")
                .filter(|r| r.len() == 9)
                .map_or(n.clone(), str::to_owned);
            Nif::parse(&n).map_err(|_| AppError::Other(format!("el NIF {n} no es válido")))?;
            PartyId::Nif(n)
        }
        PartyId::Foreign {
            country,
            id_type,
            id,
        } => {
            let country = country.trim().to_uppercase();
            if country.len() != 2 || !country.chars().all(|c| c.is_ascii_alphabetic()) {
                return Err(AppError::Other(
                    "el país va con su código de dos letras (FR, DE, US…)".into(),
                ));
            }
            if country == "ES" {
                return Err(AppError::Other(
                    "para España, usa el NIF (no un identificador extranjero)".into(),
                ));
            }
            let id = norm(id);
            if id.len() < 2 {
                return Err(AppError::Other("falta el identificador fiscal".into()));
            }
            let id = if *id_type == IdType::NifIva {
                if !EU.contains(&vies_prefix(&country).as_str()) {
                    return Err(AppError::Other(format!(
                        "{country} no es de la UE: no tiene NIF-IVA intracomunitario"
                    )));
                }
                // Con la estructura de su Estado (VERI*FACTU la valida y rechaza el registro).
                let k = key(&country, &id);
                taxpain_core::eu::check_vat_id_structure(&k).map_err(AppError::Other)?;
                k
            } else {
                id
            };
            PartyId::Foreign {
                country,
                id_type: *id_type,
                id,
            }
        }
    };
    d.email = d
        .email
        .as_deref()
        .map(str::trim)
        .filter(|e| !e.is_empty())
        .map(str::to_owned);
    if let Some(e) = &d.email
        && (!e.contains('@') || e.contains(char::is_whitespace))
    {
        return Err(AppError::Other(format!("el correo {e} no es válido")));
    }
    d.notes = d
        .notes
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_owned);
    Ok(d)
}

/// Fichas guardadas por clave: (id, papel, datos).
fn saved(v: &Vault) -> Result<BTreeMap<String, (i64, String, ContactData)>> {
    let mut stmt = v
        .connection()
        .prepare("SELECT id, kind, data FROM contact ORDER BY id")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut out = BTreeMap::new();
    for row in rows {
        let (id, role, data) = row?;
        // Una ficha ilegible no impide ver las demás.
        let Ok(d) = serde_json::from_str::<ContactData>(&data) else {
            tracing::warn!(id, "ficha de contacto ilegible");
            continue;
        };
        out.insert(party_key(&d.party.id).1, (id, role, d));
    }
    Ok(out)
}

/// Factura de la historia con su clave y su parte (de los datos guardados).
struct Hist {
    key: String,
    country: String,
    tax_id: String,
    name: String,
    item: HistoryItem,
    party: Option<Party>,
}

fn history(v: &Vault, today: Date) -> Result<Vec<Hist>> {
    let conn = v.connection();
    let mut out = Vec::new();
    let mut stmt = conn.prepare(
        "SELECT id, number, issue_date, total, status, recipient_name, recipient_tax_id,
                json_extract(data, '$.invoice.recipient')
           FROM invoice_issued WHERE recipient_tax_id IS NOT NULL",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, Option<String>>(7)?,
        ))
    })?;
    for row in rows {
        let (id, number, date, total, status, name, tax_id, recipient) = row?;
        let Ok(date) = Date::parse(&date, ISO_DATE) else {
            continue;
        };
        let party: Option<Party> = recipient.and_then(|r| serde_json::from_str(&r).ok());
        let (country, key) = match &party {
            Some(p) => party_key(&p.id),
            None => ("ES".into(), key("ES", &tax_id)),
        };
        out.push(Hist {
            key,
            country,
            tax_id,
            name: name.unwrap_or_default(),
            item: HistoryItem {
                kind: "emitida".into(),
                id,
                number,
                date,
                total: Money::from_cents(total),
                status,
                pending: Money::ZERO,
                overdue: Money::ZERO,
            },
            party,
        });
    }
    let mut stmt = conn.prepare(
        "SELECT id, coalesce(number, ''), issue_date, coalesce(total, 0), supplier_name,
                supplier_nif, supplier_country
           FROM invoice_received
          WHERE status = 'revisada' AND supplier_nif IS NOT NULL AND issue_date IS NOT NULL",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, Option<String>>(6)?,
        ))
    })?;
    for row in rows {
        let (id, number, date, total, name, tax_id, country) = row?;
        let Ok(date) = Date::parse(&date, ISO_DATE) else {
            continue;
        };
        let country = country.unwrap_or_else(|| "ES".into()).to_uppercase();
        out.push(Hist {
            key: key(&country, &tax_id),
            country,
            tax_id,
            name: name.unwrap_or_default(),
            item: HistoryItem {
                kind: "recibida".into(),
                id,
                number,
                date,
                total: Money::from_cents(total),
                status: "revisada".into(),
                pending: Money::ZERO,
                overdue: Money::ZERO,
            },
            party: None,
        });
    }
    // Lo pendiente de cada factura, de la cartera.
    let mut pending: BTreeMap<(String, i64), (Money, Money)> = BTreeMap::new();
    for d in crate::cartera::list(v, today)? {
        if d.status != "pendiente" {
            continue;
        }
        let e = pending
            .entry((d.kind.clone(), d.invoice_id))
            .or_insert((Money::ZERO, Money::ZERO));
        e.0 += d.amount;
        if d.overdue_days > 0 {
            e.1 += d.amount;
        }
    }
    for h in &mut out {
        if let Some((p, o)) = pending.get(&(h.item.kind.clone(), h.item.id)) {
            h.item.pending = *p;
            h.item.overdue = *o;
        }
    }
    // Lo vencido se guarda aparte: sólo cuenta para la fila.
    for h in &mut out {
        if let Some((_, overdue)) = pending.get(&(h.item.kind.clone(), h.item.id))
            && overdue.cents() != 0
        {
            h.item.status = format!("{} · vencida", h.item.status);
        }
    }
    Ok(out)
}

fn display_id(p: &PartyId) -> String {
    match p {
        PartyId::Nif(n) => n.clone(),
        PartyId::Foreign { id, .. } => id.clone(),
    }
}

fn rows(v: &Vault, year: i32, today: Date) -> Result<(Vec<ContactRow>, Vec<Hist>)> {
    let saved = saved(v)?;
    let hist = history(v, today)?;
    let mut map: BTreeMap<String, ContactRow> = BTreeMap::new();
    for (key, (id, role, d)) in &saved {
        let (country, _) = party_key(&d.party.id);
        map.insert(
            key.clone(),
            ContactRow {
                key: key.clone(),
                id: Some(*id),
                role: role.clone(),
                name: d.party.name.clone(),
                tax_id: display_id(&d.party.id),
                country,
                email: d.email.clone(),
                invoiced_year: Money::ZERO,
                invoiced_total: Money::ZERO,
                purchased_year: Money::ZERO,
                purchased_total: Money::ZERO,
                to_collect: Money::ZERO,
                to_pay: Money::ZERO,
                overdue: Money::ZERO,
                last_date: None,
                invoices: 0,
            },
        );
    }
    for h in &hist {
        let issued = h.item.kind == "emitida";
        let row = map.entry(h.key.clone()).or_insert_with(|| ContactRow {
            key: h.key.clone(),
            id: None,
            role: if issued { "cliente" } else { "proveedor" }.into(),
            name: h.name.clone(),
            tax_id: h.tax_id.clone(),
            country: h.country.clone(),
            email: None,
            invoiced_year: Money::ZERO,
            invoiced_total: Money::ZERO,
            purchased_year: Money::ZERO,
            purchased_total: Money::ZERO,
            to_collect: Money::ZERO,
            to_pay: Money::ZERO,
            overdue: Money::ZERO,
            last_date: None,
            invoices: 0,
        });
        if row.id.is_none() {
            let other = if issued { "proveedor" } else { "cliente" };
            if row.role == other {
                row.role = "ambos".into();
            }
            // El nombre más reciente manda (si no hay ficha).
            if row.last_date.is_none_or(|d| h.item.date >= d) && !h.name.is_empty() {
                row.name = h.name.clone();
            }
        }
        row.invoices += 1;
        row.last_date = Some(row.last_date.map_or(h.item.date, |d| d.max(h.item.date)));
        if issued {
            row.to_collect += h.item.pending;
            if h.item.status.starts_with("anulada") {
                continue;
            }
            row.invoiced_total += h.item.total;
            if h.item.date.year() == year {
                row.invoiced_year += h.item.total;
            }
        } else {
            row.to_pay += h.item.pending;
            row.purchased_total += h.item.total;
            if h.item.date.year() == year {
                row.purchased_year += h.item.total;
            }
        }
        row.overdue += h.item.overdue;
    }
    let mut list: Vec<ContactRow> = map.into_values().collect();
    list.sort_by_key(|r| r.name.to_lowercase());
    Ok((list, hist))
}

/// Clientes y proveedores (con ficha o con facturas) y sus cifras de `year`.
pub fn list(v: &Vault, year: i32, today: Date) -> Result<Vec<ContactRow>> {
    Ok(rows(v, year, today)?.0)
}

/// Ficha e historial de un cliente o proveedor.
pub fn detail(v: &Vault, key: &str, year: i32, today: Date) -> Result<ContactDetail> {
    let (list, hist) = rows(v, year, today)?;
    let row = list
        .into_iter()
        .find(|r| r.key == key)
        .ok_or(AppError::NotFound)?;
    let data = saved(v)?.remove(key).map(|(_, _, d)| d);
    let mut history: Vec<&Hist> = hist.iter().filter(|h| h.key == key).collect();
    history.sort_by(|a, b| {
        b.item
            .date
            .cmp(&a.item.date)
            .then(b.item.id.cmp(&a.item.id))
    });
    let party = data.as_ref().map(|d| d.party.clone()).or_else(|| {
        history
            .iter()
            .filter(|h| h.item.kind == "emitida")
            .find_map(|h| h.party.clone())
    });
    let mut years: BTreeMap<i32, YearTotals> = BTreeMap::new();
    for h in &history {
        let y = h.item.date.year();
        let t = years.entry(y).or_insert(YearTotals {
            year: y,
            invoiced: Money::ZERO,
            purchased: Money::ZERO,
        });
        if h.item.kind == "emitida" {
            if !h.item.status.starts_with("anulada") {
                t.invoiced += h.item.total;
            }
        } else {
            t.purchased += h.item.total;
        }
    }
    Ok(ContactDetail {
        row,
        data,
        party,
        history: history.into_iter().map(|h| h.item.clone()).collect(),
        years: years.into_values().rev().collect(),
    })
}

/// Guarda una ficha: nueva (`input.id` vacío; error si ya hay una con el mismo identificador)
/// o existente.
pub fn save(v: &Vault, input: &ContactInput, now: OffsetDateTime) -> Result<i64> {
    let role = valid_role(&input.role)?;
    let mut d = normalize(&input.data)?;
    let (country, key) = party_key(&d.party.id);
    // La comprobación de VIES no la manda el formulario: se conserva la que hubiera.
    if d.vies.is_none() {
        d.vies = find(v, &d.party.id)?.and_then(|old| old.vies);
    }
    let json = serde_json::to_string(&d).map_err(|e| AppError::Other(e.to_string()))?;
    let conn = v.connection();
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM contact WHERE tax_id = ?1 AND country = ?2",
            params![key, country],
            |r| r.get(0),
        )
        .optional()?;
    let id = match (input.id, existing) {
        (Some(id), Some(other)) if id != other => {
            return Err(AppError::Other(format!(
                "ya hay otra ficha con el identificador {key}"
            )));
        }
        // «Nuevo contacto» con un NIF que ya tiene ficha: no se pisa (correo, notas, domicilio
        // y tipo se perderían sin avisar); se edita la que hay.
        (None, Some(_)) => {
            return Err(AppError::Other(format!(
                "ya hay una ficha con ese NIF ({key}): búscala en la lista y edítala"
            )));
        }
        (Some(id), _) => {
            let n = conn.execute(
                "UPDATE contact SET kind = ?1, tax_id = ?2, country = ?3, name = ?4, data = ?5
                  WHERE id = ?6",
                params![role, key, country, d.party.name, json, id],
            )?;
            if n == 0 {
                return Err(AppError::NotFound);
            }
            id
        }
        (None, None) => {
            conn.execute(
                "INSERT INTO contact(kind, tax_id, country, name, data, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![role, key, country, d.party.name, json, now_str(now)],
            )?;
            conn.last_insert_rowid()
        }
    };
    audit::append(
        conn,
        &now_str(now),
        Actor::User,
        "contact.save",
        &key,
        &serde_json::json!({ "role": role }),
    )?;
    Ok(id)
}

/// Borra la ficha (las facturas no se tocan: el tercero sigue saliendo por ellas).
pub fn delete(v: &Vault, id: i64, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let key: Option<String> = conn
        .query_row("SELECT tax_id FROM contact WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    let Some(key) = key else {
        return Err(AppError::NotFound);
    };
    conn.execute(
        "UPDATE invoice_issued SET contact_id = NULL WHERE contact_id = ?1",
        [id],
    )?;
    conn.execute("DELETE FROM contact WHERE id = ?1", [id])?;
    audit::append(
        conn,
        &now_str(now),
        Actor::User,
        "contact.delete",
        &key,
        &serde_json::json!({}),
    )?;
    Ok(())
}

/// Ficha guardada de una parte (por su identificador), si la tiene.
pub fn find(v: &Vault, id: &PartyId) -> Result<Option<ContactData>> {
    let (country, key) = party_key(id);
    let data: Option<String> = v
        .connection()
        .query_row(
            "SELECT data FROM contact WHERE tax_id = ?1 AND country = ?2",
            params![key, country],
            |r| r.get(0),
        )
        .optional()?;
    Ok(data.and_then(|d| serde_json::from_str(&d).ok()))
}

/// Guarda en la ficha del tercero (la crea si no la tiene) el resultado de VIES, y lo deja en la
/// auditoría. `name`: el del formulario, o el que dé VIES. `role`: el de la ficha nueva
/// (`cliente` si no se dice); una ficha que ya existe conserva el suyo.
pub fn record_vies(
    v: &Vault,
    country: &str,
    vat: &str,
    name: Option<&str>,
    role: Option<&str>,
    proof: ViesProof,
    now: OffsetDateTime,
) -> Result<()> {
    let role = valid_role(role.unwrap_or("cliente"))?;
    let id = PartyId::Foreign {
        country: country.trim().to_uppercase(),
        id_type: IdType::NifIva,
        id: key(country, vat),
    };
    let (c, k) = party_key(&id);
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        "vies.check",
        &k,
        &serde_json::json!({ "valid": proof.valid, "request_id": proof.request_id }),
    )?;
    let existing: Option<(i64, String)> = v
        .connection()
        .query_row(
            "SELECT id, data FROM contact WHERE tax_id = ?1 AND country = ?2",
            params![k, c],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match existing {
        Some((id, data)) => {
            let mut d: ContactData =
                serde_json::from_str(&data).map_err(|e| AppError::Other(e.to_string()))?;
            d.vies = Some(proof);
            let json = serde_json::to_string(&d).map_err(|e| AppError::Other(e.to_string()))?;
            v.connection().execute(
                "UPDATE contact SET data = ?1 WHERE id = ?2",
                params![json, id],
            )?;
        }
        None => {
            let label = name
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .map(str::to_owned)
                .or_else(|| proof.name.clone())
                .unwrap_or_else(|| k.clone());
            let d = ContactData {
                party: Party {
                    id,
                    name: label.clone(),
                    address: None,
                    person: None,
                },
                email: None,
                // El idioma de sus facturas, el de los ajustes mientras no se diga otro.
                language: None,
                notes: None,
                vies: Some(proof),
            };
            let json = serde_json::to_string(&d).map_err(|e| AppError::Other(e.to_string()))?;
            v.connection().execute(
                "INSERT INTO contact(kind, tax_id, country, name, data, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![role, k, c, label, json, now_str(now)],
            )?;
        }
    }
    Ok(())
}

/// Propuesta para el formulario de una factura.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Suggestion {
    pub key: String,
    pub party: Party,
    pub email: Option<String>,
    pub language: Option<Language>,
}

/// Clientes que encajan con `query` (nombre o identificador), para rellenar una factura: los
/// que tienen ficha y los que ya tienen facturas emitidas.
pub fn search(v: &Vault, query: &str, limit: usize) -> Result<Vec<Suggestion>> {
    let q = query.trim().to_lowercase();
    let qn = norm(query);
    let matches = |name: &str, key: &str| {
        q.is_empty() || name.to_lowercase().contains(&q) || (!qn.is_empty() && key.contains(&qn))
    };
    let mut out: BTreeMap<String, Suggestion> = BTreeMap::new();
    for (key, (_, role, d)) in saved(v)? {
        if role != "proveedor" && matches(&d.party.name, &key) {
            out.insert(
                key.clone(),
                Suggestion {
                    key,
                    party: d.party,
                    email: d.email,
                    language: d.language,
                },
            );
        }
    }
    // Clientes sin ficha: su última factura.
    let mut stmt = v.connection().prepare(
        "SELECT json_extract(data, '$.invoice.recipient') FROM invoice_issued
          WHERE recipient_tax_id IS NOT NULL ORDER BY issue_date DESC, id DESC LIMIT 2000",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, Option<String>>(0))?;
    for row in rows {
        let Some(p) = row?.and_then(|s| serde_json::from_str::<Party>(&s).ok()) else {
            continue;
        };
        let (_, key) = party_key(&p.id);
        if !out.contains_key(&key) && matches(&p.name, &key) {
            out.insert(
                key.clone(),
                Suggestion {
                    key,
                    party: p,
                    email: None,
                    language: None,
                },
            );
        }
    }
    let mut list: Vec<Suggestion> = out.into_values().collect();
    list.sort_by_key(|s| s.party.name.to_lowercase());
    list.truncate(limit);
    Ok(list)
}

/// Ficha puesta al día con los datos del cliente de una factura: su nombre e identificación y,
/// de lo demás, lo que la factura trae; lo que no trae (domicilio, nombre de la persona) se
/// queda como estaba. Un NIF-IVA va siempre con el prefijo del país.
fn merge_party(stored: &Party, invoice: &Party) -> Party {
    let id = match &invoice.id {
        PartyId::Foreign {
            country,
            id_type: IdType::NifIva,
            id,
        } => PartyId::Foreign {
            country: country.trim().to_uppercase(),
            id_type: IdType::NifIva,
            id: key(country, id),
        },
        other => other.clone(),
    };
    Party {
        id,
        name: invoice.name.clone(),
        address: invoice.address.clone().or_else(|| stored.address.clone()),
        person: invoice.person.clone().or_else(|| stored.person.clone()),
    }
}

/// Tras emitir: crea o pone al día la ficha del cliente con los datos de la factura (sin tocar
/// el correo, el idioma ni las notas, ni borrar lo que la factura no trae) y la enlaza con la
/// factura.
pub fn remember_client(
    v: &Vault,
    party: &Party,
    invoice_id: i64,
    now: OffsetDateTime,
) -> Result<()> {
    let (country, key) = party_key(&party.id);
    let conn = v.connection();
    let existing: Option<(i64, String, String)> = conn
        .query_row(
            "SELECT id, kind, data FROM contact WHERE tax_id = ?1 AND country = ?2",
            params![key, country],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let id = match existing {
        Some((id, role, data)) => {
            let mut d: ContactData = serde_json::from_str(&data).unwrap_or(ContactData {
                party: party.clone(),
                email: None,
                language: None,
                notes: None,
                vies: None,
            });
            d.party = merge_party(&d.party, party);
            let role = if role == "proveedor" { "ambos" } else { &role };
            let json = serde_json::to_string(&d).map_err(|e| AppError::Other(e.to_string()))?;
            conn.execute(
                "UPDATE contact SET kind = ?1, name = ?2, data = ?3 WHERE id = ?4",
                params![role, party.name, json, id],
            )?;
            id
        }
        None => {
            let d = ContactData {
                party: party.clone(),
                email: None,
                language: None,
                notes: None,
                vies: None,
            };
            let json = serde_json::to_string(&d).map_err(|e| AppError::Other(e.to_string()))?;
            conn.execute(
                "INSERT INTO contact(kind, tax_id, country, name, data, created_at)
                 VALUES ('cliente', ?1, ?2, ?3, ?4, ?5)",
                params![key, country, party.name, json, now_str(now)],
            )?;
            conn.last_insert_rowid()
        }
    };
    conn.execute(
        "UPDATE invoice_issued SET contact_id = ?1 WHERE id = ?2",
        params![id, invoice_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claves() {
        assert_eq!(key("ES", "b-12345674"), "B12345674");
        assert_eq!(key("ES", "ESB12345674"), "B12345674");
        assert_eq!(key("FR", "12345678901"), "FR12345678901");
        assert_eq!(key("FR", "FR 123 456 789 01"), "FR12345678901");
        assert_eq!(key("GR", "123456789"), "EL123456789");
        assert_eq!(key("US", "12-3456789"), "US123456789");
    }

    #[test]
    fn fichas_validas() {
        let party = |id| Party {
            id,
            name: " Cliente ".into(),
            address: None,
            person: None,
        };
        let d = |id| ContactData {
            party: party(id),
            email: Some(" a@b.es ".into()),
            language: None,
            notes: Some("  ".into()),
            vies: None,
        };
        let ok = normalize(&d(PartyId::Nif("b12345674".into()))).unwrap();
        assert_eq!(ok.party.id, PartyId::Nif("B12345674".into()));
        assert_eq!(ok.party.name, "Cliente");
        assert_eq!(ok.email.as_deref(), Some("a@b.es"));
        assert_eq!(ok.notes, None);
        assert!(normalize(&d(PartyId::Nif("B12345670".into()))).is_err());
        let fr = normalize(&d(PartyId::Foreign {
            country: "fr".into(),
            id_type: IdType::NifIva,
            id: "12345678901".into(),
        }))
        .unwrap();
        assert_eq!(
            fr.party.id,
            PartyId::Foreign {
                country: "FR".into(),
                id_type: IdType::NifIva,
                id: "FR12345678901".into()
            }
        );
        // EE. UU. no tiene NIF-IVA.
        assert!(
            normalize(&d(PartyId::Foreign {
                country: "US".into(),
                id_type: IdType::NifIva,
                id: "123".into(),
            }))
            .is_err()
        );
        // Con la estructura de su Estado: el francés lleva 11 caracteres tras «FR».
        let e = normalize(&d(PartyId::Foreign {
            country: "FR".into(),
            id_type: IdType::NifIva,
            id: "FR1".into(),
        }))
        .unwrap_err()
        .to_string();
        assert!(e.contains("11 caracteres"), "{e}");
        // Un documento que no es NIF-IVA no se comprueba (pasaportes, EIN…).
        assert!(
            normalize(&d(PartyId::Foreign {
                country: "FR".into(),
                id_type: IdType::DocumentoOficial,
                id: "X1".into(),
            }))
            .is_ok()
        );
        let mut bad = d(PartyId::Nif("B12345674".into()));
        bad.email = Some("no es un correo".into());
        assert!(normalize(&bad).is_err());
    }
}
