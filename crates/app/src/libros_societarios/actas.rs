//! Libro de actas: decisiones del socio único, actas de junta, de consejo y decisiones del
//! órgano de administración (art. 26 CCom; arts. 15, 202 y 245 LSC; arts. 97-106 RRM), con
//! plantillas para lo más habitual y la certificación de los acuerdos (arts. 109-112 RRM).
//!
//! Un acta se redacta en borrador (a mano o desde una plantilla) y se aprueba: al aprobarla
//! recibe el número correlativo del ejercicio y ya no se edita ni se borra. Si hay que
//! corregirla, se reabre (con confirmación; conserva su número). Las aprobadas del ejercicio
//! forman el libro de actas que se legaliza (`ACTASCON_nnn`).
//!
//! Plantillas ([`Template`]):
//! - Aprobación de las cuentas, aplicación del resultado y gestión social (arts. 164 y 272-273
//!   LSC), con el reparto contabilizado en el cierre o, si aún no, con el propuesto.
//! - Dividendo con cargo a reservas (art. 273.2 LSC).
//! - Dividendo a cuenta con el estado contable de liquidez del art. 277 LSC, calculado de la
//!   contabilidad a la fecha del estado.
//! - Nombramiento y cese de administrador (arts. 214-215, 221 y 223 LSC).
//! - Retribución del administrador (art. 217 LSC).
//! - Cambio de domicilio en territorio nacional (art. 285.2 LSC).
//!
//! Los textos son orientativos: lo que no sale de los datos va entre «[…]» y hay que adaptarlos
//! a los estatutos.

use serde::{Deserialize, Serialize};
use taxpain_core::money::{Money, Rate};
use taxpain_invoicing::pdf::Para;
use taxpain_models::m184::FULL_PCT;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, OffsetDateTime};

use super::{company, credit_balance_at, dmy, long_date, other, parse_date, text_paras};
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

/// Órgano y forma del acta.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MinuteKind {
    JuntaOrdinaria,
    JuntaExtraordinaria,
    /// Junta universal (art. 178 LSC): presente todo el capital y todos aceptan celebrarla.
    JuntaUniversal,
    /// Decisión del socio único (art. 15 LSC).
    SocioUnico,
    /// Consejo de administración (art. 245 LSC).
    Consejo,
    /// Decisión del administrador único o de los solidarios o mancomunados.
    Administrador,
}

impl MinuteKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::JuntaOrdinaria => "junta_ordinaria",
            Self::JuntaExtraordinaria => "junta_extraordinaria",
            Self::JuntaUniversal => "junta_universal",
            Self::SocioUnico => "socio_unico",
            Self::Consejo => "consejo",
            Self::Administrador => "administrador",
        }
    }

    fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "junta_ordinaria" => Self::JuntaOrdinaria,
            "junta_extraordinaria" => Self::JuntaExtraordinaria,
            "junta_universal" => Self::JuntaUniversal,
            "socio_unico" => Self::SocioUnico,
            "consejo" => Self::Consejo,
            "administrador" => Self::Administrador,
            _ => return Err(other(format!("tipo de acta desconocido: {s}"))),
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::JuntaOrdinaria => "Junta general ordinaria",
            Self::JuntaExtraordinaria => "Junta general extraordinaria",
            Self::JuntaUniversal => "Junta general universal",
            Self::SocioUnico => "Decisión del socio único",
            Self::Consejo => "Consejo de administración",
            Self::Administrador => "Decisión del órgano de administración",
        }
    }

    fn is_meeting(self) -> bool {
        matches!(
            self,
            Self::JuntaOrdinaria | Self::JuntaExtraordinaria | Self::JuntaUniversal | Self::Consejo
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MinuteStatus {
    #[default]
    Borrador,
    Aprobada,
}

/// Un acta del libro.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Minute {
    #[serde(default)]
    pub id: i64,
    /// Número en el libro del ejercicio (al aprobarla).
    #[serde(default)]
    pub number: Option<u32>,
    /// Ejercicio al que pertenece por su fecha (lo calcula TaxPain).
    #[serde(default)]
    pub fiscal_year: i32,
    pub date: Date,
    #[serde(default)]
    pub place: String,
    pub kind: MinuteKind,
    /// Plantilla de la que salió (`cuentas`, `dividendo`… o `libre`).
    #[serde(default = "libre")]
    pub template: String,
    #[serde(default)]
    pub attendees: String,
    #[serde(default)]
    pub chair: String,
    #[serde(default)]
    pub secretary: String,
    /// Texto de los acuerdos (o de la decisión), tal como va al libro.
    pub agreements: String,
    #[serde(default)]
    pub status: MinuteStatus,
    #[serde(default)]
    pub approved_at: Option<String>,
    /// Acuerdo de dividendos que documenta (Cierre → Socios y dividendos), si lo hay. Lo pone
    /// TaxPain: al guardar se ignora.
    #[serde(default)]
    pub dividend_agreement: Option<i64>,
}

fn libre() -> String {
    "libre".into()
}

const COLS: &str = "id, number, fiscal_year, date, place, kind, template, attendees, chair, \
    secretary, agreements, status, approved_at, \
    (SELECT MIN(d.id) FROM dividend_agreement d WHERE d.minute_id = minute.id)";

fn minute_of(
    r: &taxpain_vault::rusqlite::Row<'_>,
) -> taxpain_vault::rusqlite::Result<(Minute, String, String, String)> {
    Ok((
        Minute {
            id: r.get(0)?,
            number: r.get(1)?,
            fiscal_year: r.get(2)?,
            date: time::macros::date!(2000 - 01 - 01),
            place: r.get(4)?,
            kind: MinuteKind::Administrador,
            template: r.get(6)?,
            attendees: r.get(7)?,
            chair: r.get(8)?,
            secretary: r.get(9)?,
            agreements: r.get(10)?,
            status: MinuteStatus::Borrador,
            approved_at: r.get(12)?,
            dividend_agreement: r.get(13)?,
        },
        r.get(3)?,
        r.get(5)?,
        r.get(11)?,
    ))
}

fn finish(row: (Minute, String, String, String)) -> Result<Minute> {
    let (mut m, date, kind, status) = row;
    m.date = parse_date(&date)?;
    m.kind = MinuteKind::parse(&kind)?;
    m.status = if status == "aprobada" {
        MinuteStatus::Aprobada
    } else {
        MinuteStatus::Borrador
    };
    Ok(m)
}

/// Actas del ejercicio `year` (todas si es `None`), por número y fecha.
pub fn minutes(v: &Vault, year: Option<i32>) -> Result<Vec<Minute>> {
    let mut st = v.connection().prepare(&format!(
        "SELECT {COLS} FROM minute WHERE ?1 IS NULL OR fiscal_year = ?1
          ORDER BY fiscal_year, number IS NULL, number, date, id"
    ))?;
    let rows = st.query_map([year], minute_of)?;
    rows.map(|r| finish(r?)).collect()
}

pub fn minute(v: &Vault, id: i64) -> Result<Minute> {
    let row = v
        .connection()
        .query_row(
            &format!("SELECT {COLS} FROM minute WHERE id = ?1"),
            [id],
            minute_of,
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    finish(row)
}

/// Guarda un borrador (id 0: nuevo). Las aprobadas no se cambian: hay que reabrirlas.
pub fn save_minute(v: &Vault, m: &Minute, now: OffsetDateTime) -> Result<i64> {
    company(v)?;
    if m.agreements.trim().is_empty() {
        return Err(other("escribe los acuerdos o la decisión"));
    }
    if m.id != 0 && minute(v, m.id)?.status == MinuteStatus::Aprobada {
        return Err(other(
            "el acta está aprobada y forma parte del libro: reábrela para corregirla",
        ));
    }
    let fy = crate::accounting::fiscal_year_at(v, m.date)?.year;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let id = write_minute(&tx, fy, m, now)?;
    tx.commit()?;
    Ok(id)
}

/// Guarda el acta `m` (id 0: nueva) del ejercicio `fy` dentro de la transacción de quien llama
/// (también la usa el acuerdo de dividendos, que crea el acta con su asiento).
pub(crate) fn write_minute(
    tx: &Connection,
    fy: i32,
    m: &Minute,
    now: OffsetDateTime,
) -> Result<i64> {
    let id = if m.id == 0 {
        tx.execute(
            "INSERT INTO minute(fiscal_year, date, kind, agreements, created_at, updated_at)
             VALUES (?1, ?2, ?3, '', ?4, ?4)",
            params![fy, m.date.to_string(), m.kind.as_str(), now_str(now)],
        )?;
        tx.last_insert_rowid()
    } else {
        m.id
    };
    // Si cambia de ejercicio, pierde el número que tuviera en el otro.
    tx.execute(
        "UPDATE minute SET number = CASE WHEN fiscal_year = ?1 THEN number END,
           fiscal_year = ?1, date = ?2, place = ?3, kind = ?4, template = ?5, attendees = ?6,
           chair = ?7, secretary = ?8, agreements = ?9, updated_at = ?10
         WHERE id = ?11",
        params![
            fy,
            m.date.to_string(),
            m.place.trim(),
            m.kind.as_str(),
            m.template.trim(),
            m.attendees.trim(),
            m.chair.trim(),
            m.secretary.trim(),
            m.agreements.trim(),
            now_str(now),
            id
        ],
    )?;
    audit::append(
        tx,
        &now_str(now),
        Actor::User,
        "minute.save",
        &id.to_string(),
        &serde_json::json!({ "date": m.date.to_string(), "kind": m.kind.as_str(), "template": m.template }),
    )?;
    Ok(id)
}

/// Borra un borrador que nunca tuvo número.
pub fn delete_minute(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let m = minute(v, id)?;
    if m.status == MinuteStatus::Aprobada {
        return Err(other("un acta aprobada no se borra: es parte del libro"));
    }
    if let Some(n) = m.number {
        return Err(other(format!(
            "esta acta ya tuvo el número {n} del libro de {}: corrígela y apruébala de nuevo",
            m.fiscal_year
        )));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM minute WHERE id = ?1", [id])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "minute.delete",
        &id.to_string(),
        &serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

/// Aprueba el acta (queda en el libro con el siguiente número del ejercicio). Devuelve el
/// número.
pub fn approve_minute(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<u32> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let m = minute(v, id)?;
    if m.status == MinuteStatus::Aprobada {
        return Err(other("el acta ya está aprobada"));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let number = match m.number {
        Some(n) => n,
        None => tx.query_row(
            "SELECT COALESCE(MAX(number), 0) + 1 FROM minute WHERE fiscal_year = ?1",
            [m.fiscal_year],
            |r| r.get(0),
        )?,
    };
    tx.execute(
        "UPDATE minute SET status = 'aprobada', number = ?1, approved_at = ?2, updated_at = ?2
         WHERE id = ?3",
        params![number, now_str(now), id],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "minute.approve",
        &id.to_string(),
        &serde_json::json!({ "year": m.fiscal_year, "number": number }),
    )?;
    tx.commit()?;
    Ok(number)
}

/// Vuelve a borrador un acta aprobada (conserva su número). Si el libro ya se legalizó, el
/// acta corregida no coincidirá con la legalizada.
pub fn reopen_minute(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let m = minute(v, id)?;
    if m.status != MinuteStatus::Aprobada {
        return Err(other("el acta no está aprobada"));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE minute SET status = 'borrador', approved_at = NULL, updated_at = ?1 WHERE id = ?2",
        params![now_str(now), id],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "minute.reopen",
        &id.to_string(),
        &serde_json::json!({ "year": m.fiscal_year, "number": m.number }),
    )?;
    tx.commit()?;
    Ok(())
}

// ---------- Plantillas ----------

/// Plantilla y sus datos.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "template", rename_all = "snake_case")]
pub enum Template {
    /// Aprobación de las cuentas, aplicación del resultado y gestión del ejercicio `year`.
    Cuentas {
        year: i32,
    },
    /// Dividendo con cargo a reservas voluntarias.
    Dividendo {
        amount: Money,
        #[serde(default)]
        payment_date: Option<Date>,
    },
    /// Dividendo a cuenta del resultado del ejercicio en curso, con el estado de liquidez a
    /// `statement_date`. `tax_rate`: tipo del IS estimado (25 % si no se da).
    DividendoCuenta {
        amount: Money,
        #[serde(default)]
        payment_date: Option<Date>,
        statement_date: Date,
        #[serde(default)]
        tax_rate: Option<Rate>,
    },
    Nombramiento {
        name: String,
        nif: String,
        #[serde(default)]
        address: String,
        /// «administrador único» si se deja vacío.
        #[serde(default)]
        position: String,
    },
    Cese {
        name: String,
        nif: String,
        #[serde(default)]
        position: String,
    },
    /// Importe máximo anual de la retribución de los administradores (art. 217.3 LSC).
    Retribucion {
        year: i32,
        max_amount: Money,
        #[serde(default)]
        concept: String,
    },
    /// Traslado del domicilio social a `address`.
    Domicilio {
        address: String,
    },
    Libre,
}

impl Template {
    fn key(&self) -> &'static str {
        match self {
            Self::Cuentas { .. } => "cuentas",
            Self::Dividendo { .. } => "dividendo",
            Self::DividendoCuenta { .. } => "dividendo_cuenta",
            Self::Nombramiento { .. } => "nombramiento",
            Self::Cese { .. } => "cese",
            Self::Retribucion { .. } => "retribucion",
            Self::Domicilio { .. } => "domicilio",
            Self::Libre => "libre",
        }
    }
}

/// Estado contable de liquidez para un dividendo a cuenta (art. 277 LSC).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LiquidityStatement {
    pub date: Date,
    /// Primer día del ejercicio en curso (fin del último cierre + 1).
    pub period_start: Date,
    /// Resultado contable desde el cierre del último ejercicio hasta `date`.
    pub result: Money,
    /// Pérdidas de ejercicios anteriores (121).
    pub prior_losses: Money,
    /// Dotación obligatoria a la reserva legal (art. 274 LSC) sobre ese resultado.
    pub legal_reserve: Money,
    pub tax_rate: Rate,
    /// Estimación del impuesto sobre ese resultado.
    pub estimated_tax: Money,
    /// Dividendos a cuenta del mismo ejercicio ya acordados (557).
    pub already_distributed: Money,
    /// Cantidad máxima a distribuir (art. 277.b LSC).
    pub limit: Money,
    /// Tesorería (grupo 57) a la fecha.
    pub treasury: Money,
    /// Deudas a corto plazo con proveedores, acreedores, Hacienda, Seguridad Social y entidades
    /// de crédito (40, 41, 465, 475, 476 y 52).
    pub short_term_debts: Money,
    pub amount: Money,
    pub treasury_after: Money,
}

/// Borrador de un acta desde una plantilla, con avisos y, en el dividendo a cuenta, el estado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TemplateDraft {
    pub minute: Minute,
    pub warnings: Vec<String>,
    pub liquidity: Option<LiquidityStatement>,
}

struct Ctx {
    company: super::Company,
    town: String,
    holders: Vec<crate::socios::Shareholder>,
    /// Socio único (si la ficha tiene un solo socio activo con el 100 %).
    sole: Option<crate::socios::Shareholder>,
    /// Administradores (Nóminas, con la marca de administrador).
    administrators: Vec<(String, String)>,
}

fn context(v: &Vault) -> Result<Ctx> {
    let company = company(v)?;
    let town = crate::invoicing::settings(v)?
        .and_then(|s| s.issuer.address)
        .map(|a| a.town)
        .unwrap_or_default();
    let holders: Vec<_> = crate::socios::shareholders(v)?
        .into_iter()
        .filter(|s| s.active)
        .collect();
    let sole = match holders.as_slice() {
        [s] if s.pct == FULL_PCT => Some(s.clone()),
        _ => None,
    };
    let mut st = v.connection().prepare(
        "SELECT name, nif FROM employee WHERE administrator = 1 AND active = 1 ORDER BY id",
    )?;
    let administrators = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(Ctx {
        company,
        town,
        holders,
        sole,
        administrators,
    })
}

fn pct_text(p: u32) -> String {
    let (i, f) = (p / 10_000, p % 10_000);
    if f == 0 {
        i.to_string()
    } else {
        format!("{i},{}", format!("{f:04}").trim_end_matches('0'))
    }
}

impl Ctx {
    fn admin_name(&self) -> String {
        self.administrators
            .first()
            .map_or_else(|| "[nombre del administrador]".into(), |(n, _)| n.clone())
    }

    /// Órgano de la junta: el socio único o la junta universal.
    fn meeting_kind(&self) -> MinuteKind {
        if self.sole.is_some() {
            MinuteKind::SocioUnico
        } else {
            MinuteKind::JuntaUniversal
        }
    }

    fn attendees(&self, kind: MinuteKind) -> String {
        match kind {
            MinuteKind::SocioUnico => match &self.sole {
                Some(s) => format!(
                    "{} (NIF {}), socio único, titular de la totalidad de las participaciones \
                     sociales.",
                    s.name, s.nif
                ),
                None => "[socio único]".into(),
            },
            MinuteKind::Administrador => format!("{}, administrador.", self.admin_name()),
            _ if self.holders.is_empty() => {
                "[socios asistentes, presentes o representados, con su participación]".into()
            }
            _ => self
                .holders
                .iter()
                .map(|h| {
                    format!(
                        "{} (NIF {}), titular del {} % del capital.",
                        h.name,
                        h.nif,
                        pct_text(h.pct)
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    /// Frase con la que empiezan los acuerdos.
    fn opening(&self, kind: MinuteKind) -> String {
        match kind {
            MinuteKind::SocioUnico => format!(
                "{}, socio único de {}, en ejercicio de las competencias de la junta general \
                 (art. 15 de la Ley de Sociedades de Capital), DECIDE:",
                self.sole
                    .as_ref()
                    .map_or("[socio único]", |s| s.name.as_str()),
                self.company.name
            ),
            MinuteKind::JuntaUniversal => format!(
                "Reunidos en el domicilio social los socios de {}, presentes o representados, \
                 titulares de la totalidad del capital social, aceptan por unanimidad celebrar \
                 junta general con carácter universal (art. 178 de la Ley de Sociedades de \
                 Capital) y el siguiente orden del día, y ACUERDAN por unanimidad:",
                self.company.name
            ),
            MinuteKind::JuntaOrdinaria | MinuteKind::JuntaExtraordinaria => format!(
                "Reunida la junta general de {}, convocada el [fecha] mediante [forma de \
                 convocatoria según los estatutos] con el orden del día que se transcribe, y con \
                 la asistencia, presentes o representados, de socios titulares del [ ] % del \
                 capital social, se ACUERDA [por unanimidad / con los votos a favor del [ ] %]:",
                self.company.name
            ),
            MinuteKind::Consejo => format!(
                "Reunido el consejo de administración de {}, con la asistencia de los consejeros \
                 que se indican, se ACUERDA:",
                self.company.name
            ),
            MinuteKind::Administrador => format!(
                "{}, administrador de {}, DECIDE:",
                self.admin_name(),
                self.company.name
            ),
        }
    }

    fn closing(&self, kind: MinuteKind) -> &'static str {
        match kind {
            MinuteKind::SocioUnico => {
                "Las decisiones anteriores se consignan en acta, bajo la firma del socio único \
                 (art. 15.2 de la Ley de Sociedades de Capital)."
            }
            MinuteKind::Administrador => "Firma el administrador en el lugar y fecha indicados.",
            _ => {
                "Y no habiendo más asuntos que tratar, se levanta la sesión, extendiéndose la \
                 presente acta, que es leída y aprobada por unanimidad al final de la reunión \
                 (art. 202 de la Ley de Sociedades de Capital) y firmada por el presidente y el \
                 secretario."
            }
        }
    }

    fn compose(&self, kind: MinuteKind, agreements: &[String]) -> String {
        let ordinals = [
            "PRIMERO", "SEGUNDO", "TERCERO", "CUARTO", "QUINTO", "SEXTO", "SÉPTIMO", "OCTAVO",
        ];
        let mut out = vec![self.opening(kind), String::new()];
        for (i, a) in agreements.iter().enumerate() {
            out.push(format!(
                "{}. {a}",
                ordinals.get(i).copied().unwrap_or("OTRO")
            ));
            out.push(String::new());
        }
        out.push(self.closing(kind).into());
        out.join("\n")
    }

    fn minute(
        &self,
        kind: MinuteKind,
        template: &Template,
        date: Date,
        agreements: &[String],
    ) -> Minute {
        Minute {
            id: 0,
            number: None,
            fiscal_year: 0,
            date,
            place: if self.town.is_empty() {
                "[lugar]".into()
            } else {
                self.town.clone()
            },
            kind,
            template: template.key().into(),
            attendees: self.attendees(kind),
            chair: String::new(),
            secretary: String::new(),
            agreements: self.compose(kind, agreements),
            status: MinuteStatus::Borrador,
            approved_at: None,
            dividend_agreement: None,
        }
    }

    /// Reparto de un dividendo por socio (líneas del acuerdo).
    fn split_lines(&self, amount: Money) -> Vec<String> {
        crate::socios::split(amount, &self.holders)
            .into_iter()
            .map(|l| {
                format!(
                    "- {} (NIF {}), {} %: {}",
                    l.name,
                    l.nif,
                    pct_text(l.pct),
                    l.gross
                )
            })
            .collect()
    }
}

fn debit_balance_at(conn: &Connection, prefix: &str, date: Date) -> Result<Money> {
    Ok(Money::from_cents(-credit_balance_at(conn, prefix, date)?))
}

/// Estado contable del art. 277 LSC a `date` para repartir `amount` a cuenta.
pub fn liquidity_statement(
    v: &Vault,
    date: Date,
    amount: Money,
    tax_rate: Option<Rate>,
) -> Result<LiquidityStatement> {
    liquidity_statement_until(v, date, date, amount, tax_rate)
}

/// [`liquidity_statement`] con lo ya repartido a cuenta (557) hasta `agreed`, la fecha del
/// acuerdo: un dividendo a cuenta acordado después del estado y antes de este acuerdo también
/// se descuenta del límite.
pub fn liquidity_statement_until(
    v: &Vault,
    date: Date,
    agreed: Date,
    amount: Money,
    tax_rate: Option<Rate>,
) -> Result<LiquidityStatement> {
    let conn = v.connection();
    let fy = crate::accounting::fiscal_year_at(v, date)?;
    let result = Money::from_cents(conn.query_row(
        "SELECT COALESCE(SUM(l.credit - l.debit), 0) FROM journal_line l
           JOIN journal_entry e ON e.id = l.entry_id
          WHERE e.fiscal_year = ?1 AND e.date <= ?2
            AND e.origin_kind NOT IN ('Regularizacion', 'Cierre', 'Apertura')
            AND (l.account LIKE '6%' OR l.account LIKE '7%')",
        params![fy.year, date.to_string()],
        |r| r.get(0),
    )?);
    let prior_losses = debit_balance_at(conn, "121", date)?.max(Money::ZERO);
    let capital = Money::from_cents(credit_balance_at(conn, "100", date)?);
    let reserve = Money::from_cents(credit_balance_at(conn, "112", date)?);
    let positive = result.cents() > 0;
    let legal_reserve = if positive {
        result
            .apply_rate(Rate::percent(10))
            .min((capital.apply_rate(Rate::percent(20)) - reserve).max(Money::ZERO))
    } else {
        Money::ZERO
    };
    // TODO(VERIFY): tipo del IS para la estimación. El general es el 25 % (art. 29.1 LIS); las
    // empresas de reducida dimensión y las microempresas tienen tipos menores (Ley 7/2024). Se
    // deja elegir; por defecto, el general.
    let tax_rate = tax_rate.unwrap_or(Rate::percent(25));
    let estimated_tax = if positive {
        result.apply_rate(tax_rate)
    } else {
        Money::ZERO
    };
    let already_distributed = debit_balance_at(conn, "557", date.max(agreed))?.max(Money::ZERO);
    let limit = (result - prior_losses - legal_reserve - estimated_tax - already_distributed)
        .max(Money::ZERO);
    let treasury = debit_balance_at(conn, "57", date)?;
    let mut short_term_debts = Money::ZERO;
    for p in ["40", "41", "465", "475", "476", "52"] {
        short_term_debts += Money::from_cents(credit_balance_at(conn, p, date)?).max(Money::ZERO);
    }
    Ok(LiquidityStatement {
        date,
        period_start: fy.start,
        result,
        prior_losses,
        legal_reserve,
        tax_rate,
        estimated_tax,
        already_distributed,
        limit,
        treasury,
        short_term_debts,
        amount,
        treasury_after: treasury - amount,
    })
}

fn liquidity_text(l: &LiquidityStatement) -> Vec<String> {
    let row = |label: &str, m: Money| format!("{label}: {m}");
    vec![
        format!(
            "ANEXO. Estado contable formulado por el órgano de administración a {} (art. 277 de \
             la Ley de Sociedades de Capital)",
            long_date(l.date)
        ),
        String::new(),
        format!(
            "a) Resultados obtenidos desde el fin del último ejercicio (del {} al {}): {}",
            dmy(l.period_start),
            dmy(l.date),
            l.result
        ),
        row("Menos pérdidas de ejercicios anteriores", l.prior_losses),
        row(
            "Menos dotación obligatoria a la reserva legal (art. 274 LSC)",
            l.legal_reserve,
        ),
        format!(
            "Menos impuesto estimado sobre esos resultados ({}): {}",
            l.tax_rate, l.estimated_tax
        ),
        row(
            "Menos cantidades ya distribuidas a cuenta en el ejercicio",
            l.already_distributed,
        ),
        row("Cantidad máxima distribuible", l.limit),
        String::new(),
        row("b) Tesorería disponible (bancos y caja)", l.treasury),
        row(
            "Deudas a corto plazo (proveedores, acreedores, Hacienda, Seguridad Social y \
             entidades de crédito)",
            l.short_term_debts,
        ),
        row("Dividendo a cuenta que se acuerda", l.amount),
        row("Tesorería tras el pago", l.treasury_after),
        String::new(),
        "[Previsión de cobros y pagos de los doce meses siguientes, si se incluye: ...]".into(),
        "De lo anterior resulta que existe liquidez suficiente para la distribución.".into(),
    ]
}

/// Borrador de acta de la plantilla, con fecha `date`. No guarda nada.
pub fn template(v: &Vault, t: &Template, date: Date) -> Result<TemplateDraft> {
    template_by(v, t, date, None)
}

/// [`template`] con el órgano que adopta el acuerdo (`None`: el que propone la plantilla). El
/// dividendo a cuenta lo pueden acordar la junta o los administradores (art. 277 LSC).
pub fn template_by(
    v: &Vault,
    t: &Template,
    date: Date,
    organ: Option<MinuteKind>,
) -> Result<TemplateDraft> {
    let ctx = context(v)?;
    let mut warnings = Vec::new();
    let mut liquidity = None;
    let socio = ctx.sole.is_some();
    let socios = if socio {
        "el socio único"
    } else {
        "los socios"
    };
    let (kind, agreements) = match t {
        Template::Cuentas { year } => {
            let p = crate::cierre::allocation(v, *year)?;
            let fy = crate::accounting::fiscal_year(v, *year)?;
            warnings.extend(p.warnings.iter().cloned());
            if date < p.earliest {
                warnings.push(format!(
                    "La aprobación de las cuentas es posterior al cierre ({}).",
                    dmy(p.earliest - time::Duration::days(1))
                ));
            }
            if date > p.meeting_deadline {
                warnings.push(format!(
                    "La junta ordinaria se celebra dentro de los seis meses siguientes al cierre \
                     (art. 164 LSC: hasta el {}). Fuera de plazo es válida, pero las cuentas \
                     llegan tarde al depósito.",
                    dmy(p.meeting_deadline)
                ));
            }
            let lines: Vec<(String, Money)> = match crate::memoria::applied_result(v, *year)? {
                Some(l) => l,
                None => {
                    warnings.push(
                        "La aplicación del resultado aún no está contabilizada: el acta lleva la \
                         propuesta de TaxPain. Contabilízala en Cierre con la misma fecha."
                            .into(),
                    );
                    let a = p.suggested;
                    if p.result.cents() < 0 {
                        vec![(
                            "A resultados negativos de ejercicios anteriores".into(),
                            -p.result,
                        )]
                    } else {
                        [
                            ("A reserva legal", a.legal),
                            (
                                "A compensar resultados negativos de ejercicios anteriores",
                                a.losses,
                            ),
                            (
                                "A reserva de capitalización (art. 25 LIS)",
                                a.capitalization,
                            ),
                            ("A reservas voluntarias", a.voluntary),
                            ("A dividendos", a.dividends),
                            ("A remanente", a.remanente),
                        ]
                        .into_iter()
                        .filter(|(_, m)| m.cents() > 0)
                        .map(|(l, m)| (l.to_owned(), m))
                        .collect()
                    }
                }
            };
            let what = if p.result.cents() >= 0 {
                "un beneficio"
            } else {
                "unas pérdidas"
            };
            let mut second = vec![format!(
                "Aprobar la siguiente aplicación del resultado del ejercicio {year}:\nBase de \
                 reparto: pérdidas y ganancias, {}.\nAplicación:",
                p.result
            )];
            for (l, m) in &lines {
                second.push(format!("- {l}: {m}"));
            }
            let dividends: Money = lines
                .iter()
                .filter(|(l, _)| l.starts_with("A dividendos"))
                .map(|(_, m)| *m)
                .sum();
            if dividends.cents() > 0 {
                second.push(format!(
                    "El dividendo de {dividends} se paga a {socios} [el día …], con la retención \
                     a cuenta que corresponda, en proporción a su participación:"
                ));
                second.extend(ctx.split_lines(dividends));
                if p.interim_dividend.cents() > 0 {
                    second.push(format!(
                        "De él, {} ya se repartieron a cuenta y se dan por recibidos.",
                        p.interim_dividend.min(dividends)
                    ));
                }
            }
            let agreements = vec![
                format!(
                    "Aprobar las cuentas anuales del ejercicio {year} (del {} al {}), \
                     formuladas por el órgano de administración el [fecha de formulación], que \
                     comprenden el balance, la cuenta de pérdidas y ganancias, [el estado de \
                     cambios en el patrimonio neto] y la memoria, y que arrojan {what} de {}.",
                    dmy(fy.start),
                    dmy(fy.end),
                    p.result.abs()
                ),
                second.join("\n"),
                format!(
                    "Aprobar la gestión del órgano de administración durante el ejercicio {year}."
                ),
                format!(
                    "Facultar a {} para depositar las cuentas anuales en el Registro Mercantil \
                     (art. 279 LSC) y expedir las certificaciones de estos acuerdos.",
                    ctx.admin_name()
                ),
            ];
            (ctx.meeting_kind(), agreements)
        }
        Template::Dividendo {
            amount,
            payment_date,
        } => {
            if amount.cents() <= 0 {
                return Err(other("pon el importe del dividendo"));
            }
            let conn = v.connection();
            let free = Money::from_cents(
                credit_balance_at(conn, "113", date)? + credit_balance_at(conn, "120", date)?,
            );
            if *amount > free {
                warnings.push(format!(
                    "Las reservas voluntarias y el remanente suman {free} a esa fecha: el \
                     dividendo con cargo a reservas no puede superarlos."
                ));
            }
            let equity = Money::from_cents(
                credit_balance_at(conn, "10", date)?
                    + credit_balance_at(conn, "11", date)?
                    + credit_balance_at(conn, "12", date)?,
            ) - debit_balance_at(conn, "557", date)?.max(Money::ZERO);
            let capital = Money::from_cents(credit_balance_at(conn, "100", date)?);
            if equity - *amount < capital {
                warnings.push(format!(
                    "Tras el reparto el patrimonio neto ({}) quedaría por debajo del capital \
                     ({capital}): el art. 273.2 LSC no lo permite.",
                    equity - *amount
                ));
            }
            warnings.push(
                "Contabiliza el acuerdo en el Diario (113 Reservas voluntarias a 526 Dividendo \
                 activo a pagar) y el pago en Cierre → Socios y dividendos, que calcula la \
                 retención."
                    .into(),
            );
            if ctx.holders.is_empty() {
                warnings.push("Da de alta a los socios para repartir el dividendo.".into());
            }
            let mut first = vec![format!(
                "Distribuir a {socios} un dividendo de {amount} brutos con cargo a reservas \
                 voluntarias de libre disposición, que se pagará el {}, con la retención a \
                 cuenta que corresponda, en proporción a su participación en el capital:",
                payment_date.map_or_else(|| "[fecha de pago]".into(), long_date)
            )];
            first.extend(ctx.split_lines(*amount));
            let agreements = vec![
                first.join("\n"),
                "Hacer constar que el patrimonio neto de la sociedad no es ni resulta, como \
                     consecuencia del reparto, inferior al capital social (art. 273.2 LSC)."
                    .into(),
            ];
            (ctx.meeting_kind(), agreements)
        }
        Template::DividendoCuenta {
            amount,
            payment_date,
            statement_date,
            tax_rate,
        } => {
            if amount.cents() <= 0 {
                return Err(other("pon el importe del dividendo a cuenta"));
            }
            if *statement_date > date {
                return Err(other(
                    "el estado contable es de la fecha del acuerdo o anterior",
                ));
            }
            let l = liquidity_statement_until(v, *statement_date, date, *amount, *tax_rate)?;
            if l.result.cents() <= 0 {
                warnings.push(
                    "No hay beneficios desde el último cierre: no se puede repartir a cuenta \
                     (art. 277.b LSC)."
                        .into(),
                );
            } else if *amount > l.limit {
                warnings.push(format!(
                    "El dividendo supera la cantidad máxima distribuible a cuenta ({}) del art. \
                     277.b LSC.",
                    l.limit
                ));
            }
            if *amount > l.treasury {
                warnings.push(format!(
                    "La tesorería a esa fecha ({}) no cubre el dividendo: el art. 277.a LSC pide \
                     liquidez suficiente.",
                    l.treasury
                ));
            }
            warnings.push(
                "Contabiliza el acuerdo en el Diario (557 Dividendo activo a cuenta a 526 \
                 Dividendo activo a pagar) y el pago en Cierre → Socios y dividendos."
                    .into(),
            );
            // TODO(VERIFY): contenido exacto del estado contable. El art. 277.a LSC sólo exige
            // que «se ponga de manifiesto que existe liquidez suficiente»; la práctica añade una
            // previsión de tesorería de un año, que aquí queda para completar.
            let year = crate::accounting::fiscal_year_at(v, *statement_date)?.year;
            let mut first = vec![format!(
                "Distribuir a {socios} la cantidad de {amount} brutos a cuenta del dividendo del \
                 ejercicio {year}, que se pagará el {}, con la retención a cuenta que \
                 corresponda, en proporción a su participación en el capital:",
                payment_date.map_or_else(|| "[fecha de pago]".into(), long_date)
            )];
            first.extend(ctx.split_lines(*amount));
            let mut second = vec![format!(
                "Aprobar el estado contable formulado a {} por el órgano de administración, que \
                 pone de manifiesto que existe liquidez suficiente para la distribución y que la \
                 cantidad distribuida no excede de los resultados obtenidos desde el fin del \
                 último ejercicio, deducidas las pérdidas de ejercicios anteriores, las \
                 cantidades con las que deben dotarse las reservas obligatorias y la estimación \
                 del impuesto a pagar sobre dichos resultados (art. 277 LSC). El estado se une a \
                 esta acta como anexo.",
                long_date(*statement_date)
            )];
            second.push(String::new());
            second.extend(liquidity_text(&l));
            liquidity = Some(l);
            (
                ctx.meeting_kind(),
                vec![first.join("\n"), second.join("\n")],
            )
        }
        Template::Nombramiento {
            name,
            nif,
            address,
            position,
        } => {
            if name.trim().is_empty() || nif.trim().is_empty() {
                return Err(other("pon el nombre y el NIF del administrador"));
            }
            let position = if position.trim().is_empty() {
                "administrador único"
            } else {
                position.trim()
            };
            let domicile = if address.trim().is_empty() {
                ", con domicilio en [domicilio]".to_owned()
            } else {
                format!(", con domicilio en {}", address.trim())
            };
            warnings.push(
                "El nombramiento se eleva a público y se presenta a inscripción en el Registro \
                 Mercantil en los diez días siguientes a la aceptación (art. 215 LSC). Si el \
                 cargo es retribuido, fija también la retribución (art. 217 LSC)."
                    .into(),
            );
            let agreements = vec![
                format!(
                    "Nombrar {position} de la sociedad, por plazo indefinido (art. 221.2 LSC), a \
                     {}, mayor de edad, con NIF {}{domicile}.\nEl nombrado, presente en este \
                     acto, acepta el cargo y declara no estar incurso en ninguna de las \
                     prohibiciones ni incompatibilidades legales para ejercerlo, en particular \
                     las del art. 213 LSC.",
                    name.trim(),
                    nif.trim().to_uppercase()
                ),
                format!(
                    "Facultar a {} para elevar a público estos acuerdos y solicitar su \
                     inscripción en el Registro Mercantil.",
                    name.trim()
                ),
            ];
            (ctx.meeting_kind(), agreements)
        }
        Template::Cese {
            name,
            nif,
            position,
        } => {
            if name.trim().is_empty() || nif.trim().is_empty() {
                return Err(other("pon el nombre y el NIF del administrador"));
            }
            let position = if position.trim().is_empty() {
                "administrador único"
            } else {
                position.trim()
            };
            warnings.push(
                "Si era el único administrador, nombra a otro en la misma decisión: la \
                 sociedad no puede quedarse sin órgano de administración. El cese se inscribe \
                 en el Registro Mercantil."
                    .into(),
            );
            let agreements = vec![
                format!(
                    "Cesar a {}, con NIF {}, en su cargo de {position} de la sociedad, \
                     agradeciéndole los servicios prestados (art. 223 LSC).",
                    name.trim(),
                    nif.trim().to_uppercase()
                ),
                format!(
                    "Facultar a {} para elevar a público este acuerdo y solicitar su \
                     inscripción en el Registro Mercantil.",
                    ctx.admin_name()
                ),
            ];
            (ctx.meeting_kind(), agreements)
        }
        Template::Retribucion {
            year,
            max_amount,
            concept,
        } => {
            if max_amount.cents() <= 0 {
                return Err(other("pon el importe máximo anual de la retribución"));
            }
            let fy = crate::accounting::fiscal_year(v, *year)?;
            let paid = Money::from_cents(v.connection().query_row(
                "SELECT COALESCE(SUM(p.gross), 0) FROM payslip p
                   JOIN employee e ON e.id = p.employee_id
                  WHERE e.administrator = 1 AND p.date BETWEEN ?1 AND ?2",
                params![fy.start.to_string(), fy.end.to_string()],
                |r| r.get(0),
            )?);
            if paid > *max_amount {
                warnings.push(format!(
                    "Las nóminas de los administradores de {year} ya suman {paid}, más que el \
                     máximo que se fija."
                ));
            }
            warnings.push(
                "La retribución del cargo exige que los estatutos digan que es retribuido y su \
                 sistema (art. 217.1 y 2 LSC); si no lo dicen, hay que modificarlos antes. Sin \
                 ese soporte se discute que sea gasto deducible en el Impuesto sobre Sociedades \
                 (art. 15.e LIS)."
                    .into(),
            );
            let concept = if concept.trim().is_empty() {
                "asignación fija".to_owned()
            } else {
                concept.trim().to_owned()
            };
            let agreements = vec![
                format!(
                    "De acuerdo con el artículo [número] de los estatutos sociales, que establece \
                     el carácter retribuido del cargo de administrador y su sistema de \
                     retribución ({concept}), fijar en {max_amount} brutos el importe máximo de \
                     la remuneración anual del conjunto de los administradores en su condición \
                     de tales, para el ejercicio {year} (del {} al {}) y siguientes mientras no \
                     se apruebe su modificación (art. 217.3 LSC).",
                    dmy(fy.start),
                    dmy(fy.end)
                ),
                "La distribución de la retribución entre los administradores se establece por \
                 acuerdo de estos (art. 217.3 LSC), teniendo en cuenta las funciones y \
                 responsabilidades de cada uno."
                    .into(),
            ];
            (ctx.meeting_kind(), agreements)
        }
        Template::Domicilio { address } => {
            if address.trim().is_empty() {
                return Err(other("pon el nuevo domicilio social"));
            }
            let old = if ctx.company.address.is_empty() {
                "[domicilio actual]".to_owned()
            } else {
                ctx.company.address.clone()
            };
            warnings.push(
                "El cambio se eleva a escritura pública y se inscribe en el Registro Mercantil. \
                 Comunica el nuevo domicilio fiscal a la AEAT (modelo 036) en el plazo de tres \
                 meses (art. 17 del Reglamento de gestión e inspección, RD 1065/2007) y cambia \
                 los datos de facturación en TaxPain."
                    .into(),
            );
            warnings.push(
                "El órgano de administración puede trasladar el domicilio dentro del territorio \
                 nacional salvo que los estatutos digan otra cosa (art. 285.2 LSC); si los \
                 estatutos lo reservan a la junta, cambia el tipo de acta."
                    .into(),
            );
            let agreements = vec![
                format!(
                    "Trasladar el domicilio social de {old} a {}, en ejercicio de la competencia \
                     que atribuye al órgano de administración el art. 285.2 de la Ley de \
                     Sociedades de Capital, y modificar en consecuencia el artículo [número] de \
                     los estatutos sociales, que queda redactado así: «El domicilio social se \
                     fija en {}».",
                    address.trim(),
                    address.trim()
                ),
                "Elevar a público este acuerdo y solicitar su inscripción en el Registro \
                 Mercantil."
                    .into(),
            ];
            (MinuteKind::Administrador, agreements)
        }
        Template::Libre => (ctx.meeting_kind(), vec!["[Acuerdo]".into()]),
    };
    let mut minute = ctx.minute(organ.unwrap_or(kind), t, date, &agreements);
    minute.fiscal_year = crate::accounting::fiscal_year_at(v, date)?.year;
    Ok(TemplateDraft {
        minute,
        warnings,
        liquidity,
    })
}

// ---------- Documentos ----------

fn signatures(kind: MinuteKind) -> Vec<Para> {
    match kind {
        MinuteKind::SocioUnico => vec![Para::Text(
            "Firma del socio único: ______________________________".into(),
        )],
        MinuteKind::Administrador => vec![Para::Text(
            "Firma del administrador: ______________________________".into(),
        )],
        _ => vec![
            Para::Text("El presidente: ______________________________".into()),
            Para::Text("El secretario: ______________________________".into()),
        ],
    }
}

fn minute_paras(m: &Minute) -> Vec<Para> {
    let mut p = Vec::new();
    if m.status == MinuteStatus::Borrador {
        p.push(Para::Bold("BORRADOR (no aprobada)".into()));
    }
    p.push(Para::Bold(match m.number {
        Some(n) => format!("Acta nº {n} · {}", m.kind.label()),
        None => format!("Acta · {}", m.kind.label()),
    }));
    p.push(Para::Text(format!(
        "Fecha: {}.{}",
        long_date(m.date),
        if m.place.is_empty() {
            String::new()
        } else {
            format!(" Lugar: {}.", m.place)
        }
    )));
    if !m.attendees.is_empty() {
        p.push(Para::Text(
            if m.kind.is_meeting() {
                "Asistentes:"
            } else {
                "Interviene:"
            }
            .into(),
        ));
        text_paras(&mut p, &m.attendees);
    }
    let mut roles = Vec::new();
    if !m.chair.is_empty() {
        roles.push(format!("Presidente: {}", m.chair));
    }
    if !m.secretary.is_empty() {
        roles.push(format!("Secretario: {}", m.secretary));
    }
    if !roles.is_empty() {
        p.push(Para::Text(format!("{}.", roles.join(". "))));
    }
    p.push(Para::Gap);
    text_paras(&mut p, &m.agreements);
    p.push(Para::Gap);
    p.extend(signatures(m.kind));
    p
}

/// Un acta suelta (también en borrador), para revisarla o firmarla.
pub fn minute_document(v: &Vault, id: i64, as_pdf: bool) -> Result<Vec<u8>> {
    let c = company(v)?;
    let m = minute(v, id)?;
    let mut p = vec![
        Para::Title(format!("{} · {}", m.kind.label(), dmy(m.date))),
        Para::Text(format!("{} · NIF {}", c.name, c.nif)),
        Para::Gap,
    ];
    p.extend(minute_paras(&m));
    super::render(&format!("Acta {}", dmy(m.date)), None, &[p], as_pdf)
}

/// Quién firma la certificación y dónde.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CertificateInput {
    /// Nombre de quien certifica (el administrador si se deja vacío).
    #[serde(default)]
    pub name: String,
    /// Cargo: «administrador único», «secretario del consejo»…
    #[serde(default)]
    pub position: String,
    #[serde(default)]
    pub place: String,
    pub date: Date,
}

/// Certificación de los acuerdos de un acta aprobada (arts. 109-112 RRM).
pub fn certificate_paras(v: &Vault, id: i64, input: &CertificateInput) -> Result<Vec<Para>> {
    let ctx = context(v)?;
    let m = minute(v, id)?;
    if m.status != MinuteStatus::Aprobada {
        return Err(other(
            "sólo se certifica un acta aprobada: apruébala antes en el libro de actas",
        ));
    }
    if input.date < m.date {
        return Err(other(
            "la certificación es de la fecha del acta o posterior",
        ));
    }
    let name = if input.name.trim().is_empty() {
        ctx.admin_name()
    } else {
        input.name.trim().to_owned()
    };
    let position = if input.position.trim().is_empty() {
        "administrador único".to_owned()
    } else {
        input.position.trim().to_owned()
    };
    let place = if input.place.trim().is_empty() {
        m.place.clone()
    } else {
        input.place.trim().to_owned()
    };
    let c = &ctx.company;
    let mut p = vec![
        Para::Title("Certificación de acuerdos".into()),
        Para::Text(format!(
            "{name}, {position} de {}, con NIF {}{},",
            c.name,
            c.nif,
            if c.address.is_empty() {
                String::new()
            } else {
                format!(" y domicilio social en {}", c.address)
            }
        )),
        Para::Gap,
        Para::Bold("CERTIFICA:".into()),
        Para::Text(format!(
            "Que en el libro de actas de la sociedad consta el acta nº {} del ejercicio {}, \
             correspondiente a la {} de fecha {}{}, que fue aprobada y firmada{}, y en la que se \
             adoptaron, entre otros, los siguientes acuerdos, que se transcriben literalmente:",
            m.number.unwrap_or_default(),
            m.fiscal_year,
            m.kind.label().to_lowercase(),
            long_date(m.date),
            if m.place.is_empty() {
                String::new()
            } else {
                format!(", celebrada en {}", m.place)
            },
            match m.kind {
                MinuteKind::SocioUnico => " por el socio único",
                MinuteKind::Administrador => " por el administrador",
                _ => " al final de la reunión",
            }
        )),
    ];
    if m.kind.is_meeting() && !m.attendees.is_empty() {
        p.push(Para::Text("Asistentes:".into()));
        text_paras(&mut p, &m.attendees);
    }
    p.push(Para::Gap);
    text_paras(&mut p, &m.agreements);
    p.push(Para::Gap);
    if m.template == "cuentas" {
        // TODO(VERIFY): menciones de la certificación para el depósito (art. 366.1.2º RRM):
        // firma de las cuentas por todos los administradores y si la sociedad está obligada a
        // auditarlas. Revisar con el modelo de certificación del Registro.
        p.push(Para::Text(
            "Que las cuentas anuales aprobadas fueron firmadas por todos los administradores [o \
             indica la causa por la que falta alguna firma] (art. 253.2 LSC), y que la sociedad \
             [no] está obligada a someterlas a auditoría."
                .into(),
        ));
        p.push(Para::Gap);
    }
    p.push(Para::Text(format!(
        "Y para que así conste, expido la presente certificación en {}, a {}.",
        if place.is_empty() { "[lugar]" } else { &place },
        long_date(input.date)
    )));
    p.push(Para::Gap);
    p.push(Para::Text(format!(
        "Fdo.: {name}, {position} ______________________________"
    )));
    if m.kind == MinuteKind::Consejo {
        p.push(Para::Text(
            "Vº Bº El presidente: ______________________________".into(),
        ));
    }
    p.push(Para::Gap);
    p.push(Para::Text(
        "Para el depósito de cuentas, la firma se legitima ante notario salvo que la \
         certificación se presente con firma electrónica (art. 366.1.2º RRM)."
            .into(),
    ));
    Ok(p)
}

pub fn certificate_document(
    v: &Vault,
    id: i64,
    input: &CertificateInput,
    as_pdf: bool,
) -> Result<Vec<u8>> {
    let p = certificate_paras(v, id, input)?;
    super::render("Certificación de acuerdos", None, &[p], as_pdf)
}

/// El libro de actas del ejercicio (sólo las aprobadas, por número): cada acta en página nueva.
pub fn book_docs(v: &Vault, year: i32, number: Option<u16>) -> Result<Vec<Vec<Para>>> {
    let c = company(v)?;
    let fy = crate::accounting::fiscal_year(v, year)?;
    let list: Vec<Minute> = minutes(v, Some(year))?
        .into_iter()
        .filter(|m| m.status == MinuteStatus::Aprobada)
        .collect();
    let mut first = vec![
        Para::Title(match number {
            Some(n) => format!("Libro de actas nº {n}"),
            None => "Libro de actas".into(),
        }),
        Para::Text(format!("{} · NIF {}", c.name, c.nif)),
        Para::Text(format!(
            "Ejercicio {year}: fecha de apertura {} y de cierre {}.",
            dmy(fy.start),
            dmy(fy.end)
        )),
        Para::Text(
            "Actas de la junta general, decisiones del socio único y del órgano de \
             administración (art. 26 del Código de Comercio y arts. 97 y siguientes del \
             Reglamento del Registro Mercantil)."
                .into(),
        ),
        Para::Gap,
    ];
    if list.is_empty() {
        first.push(Para::Text("Sin actas aprobadas en el ejercicio.".into()));
        return Ok(vec![first]);
    }
    let mut docs = Vec::new();
    for (i, m) in list.iter().enumerate() {
        let mut d = if i == 0 {
            std::mem::take(&mut first)
        } else {
            vec![]
        };
        d.extend(minute_paras(m));
        docs.push(d);
    }
    Ok(docs)
}

/// Actas aprobadas y borradores del ejercicio.
pub(crate) fn counts(v: &Vault, year: i32) -> Result<(usize, usize)> {
    let list = minutes(v, Some(year))?;
    let approved = list
        .iter()
        .filter(|m| m.status == MinuteStatus::Aprobada)
        .count();
    Ok((approved, list.len() - approved))
}
