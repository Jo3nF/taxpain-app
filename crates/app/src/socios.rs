//! Socios de una sociedad y pago de dividendos.
//!
//! - Ficha de socios (`shareholder`): NIF, nombre, domicilio y provincia, participación,
//!   persona física o jurídica, residencia y, para las sociedades socias, si han acreditado el
//!   derecho a la exención del art. 21 LIS. No se reutiliza `partner` (socios de entidades en
//!   atribución de rentas para el 184): son otra figura, con otros datos y otro modelo.
//! - Pago de dividendos: tras la aplicación del resultado (129 → 526 «Dividendo activo a
//!   pagar», `cierre::post_allocation`), el bruto (como mucho, lo que queda en la 526) se
//!   reparte por participación y se retiene el 19 % (art. 101.4 LIRPF para personas físicas,
//!   art. 128.6 LIS para sociedades), salvo a las sociedades con derecho a la exención del
//!   art. 21 LIS (art. 128.4.d LIS y art. 61.p RIS, previa comunicación de la socia con los
//!   documentos que lo justifican). Asiento: 526 al debe por el bruto; 4751
//!   «Hacienda Pública, acreedora por retenciones practicadas» y la tesorería (57x, o la 551 si
//!   se compensa con la cuenta corriente del socio) al haber.
//! - La retención se ingresa con el 123 del trimestre del pago y cada pago va en el 193 de
//!   enero (`models::draft_123` y `models::draft_193`). Los socios no residentes tributan por el
//!   IRNR (19 % salvo convenio o Directiva matriz-filial; modelos 216 y 296): se les retiene el
//!   19 % y se avisa, pero no van en el 123/193.
//!
//! Cada pago se liga al acuerdo de reparto del que sale (`dividendos`: a cuenta, con cargo a
//! reservas o al resultado, con sus comprobaciones legales y su acta): el elegido o el más
//! antiguo con algo pendiente. Sin acuerdo registrado o con el acta sin aprobar, se avisa.
//!
//! Que la retención no se cuele en el 111: el 111 sólo lee la 4751 de los asientos de nómina
//! (`Origin::Nomina`) y de los importados con sueldos (640); este asiento es
//! `Origin::Dividendo`. Tampoco toca la 473, que es la del 200.
//!
//! La obligación de retener nace con la exigibilidad del dividendo: la fecha fijada en el
//! acuerdo de distribución o, si no la fija, el día siguiente al acuerdo (art. 94.1 RIRPF y
//! art. 65.1 RIS). Esa fecha (la que pone el usuario) decide el 123 en que se ingresa; si se
//! paga más tarde, el asiento lleva igualmente esa fecha (`TODO(VERIFY)`: separar devengo y
//! pago si hace falta).

use serde::{Deserialize, Serialize};
use taxpain_core::Nif;
use taxpain_core::accounting::account::AccountCode;
use taxpain_core::accounting::entry::{EntryLine, JournalEntry, Origin};
use taxpain_core::money::{Money, Rate};
use taxpain_models::m184::FULL_PCT;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, OffsetDateTime};

use crate::accounting;
use crate::cierre::ProposedLine;
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

/// Retención general sobre dividendos (art. 101.4 LIRPF; art. 128.6 LIS; art. 25.1.f TRLIRNR).
pub const DIVIDEND_RATE: Rate = Rate::percent(19);

/// Cuenta de las retenciones practicadas (la misma que la del 111 y el 115, que paga el 123).
pub const WITHHOLDING_ACCOUNT: &str = "4751";

/// Participación mínima (directa o indirecta) para la exención del art. 21.1.a LIS: el 5 %, en
/// millonésimas.
pub const EXEMPTION_MIN_PCT: u32 = 50_000;

/// Participación en millonésimas como porcentaje («3», «4,5», «33,3333»).
fn pct_text(pct: u32) -> String {
    let (int, frac) = (pct / 10_000, pct % 10_000);
    if frac == 0 {
        int.to_string()
    } else {
        format!("{int},{}", format!("{frac:04}").trim_end_matches('0'))
    }
}

fn other(e: impl ToString) -> AppError {
    AppError::Other(e.to_string())
}

fn code(s: &str) -> Result<AccountCode> {
    AccountCode::new(s).map_err(other)
}

fn parse_date(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]")).map_err(other)
}

fn is_company(v: &Vault) -> Result<bool> {
    let nif = crate::companies::info(v)?.nif;
    Ok(nif
        .as_deref()
        .and_then(|n| Nif::parse(n).ok())
        .is_some_and(|n| !n.is_persona_fisica()))
}

/// Socio de la sociedad.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shareholder {
    #[serde(default)]
    pub id: i64,
    /// NIF español (o el identificador fiscal de su país si no es residente).
    pub nif: String,
    /// Apellidos y nombre (persona física) o razón social.
    pub name: String,
    #[serde(default)]
    pub address: String,
    /// Código de provincia del domicilio (01-53); vacío si no es residente.
    #[serde(default)]
    pub province: String,
    /// Código ISO del país de residencia.
    #[serde(default = "spain")]
    pub country: String,
    /// Participación en millonésimas (100 % = 1.000.000).
    pub pct: u32,
    /// Persona jurídica (sociedad) en vez de persona física.
    #[serde(default)]
    pub legal_person: bool,
    /// Residente en España.
    #[serde(default = "yes")]
    pub resident: bool,
    /// Sociedad residente con derecho a la exención del art. 21 LIS que lo ha comunicado con los
    /// documentos que lo justifican: sus dividendos no llevan retención (art. 128.4.d LIS; art.
    /// 61.p RIS).
    #[serde(default)]
    pub exempt: bool,
    #[serde(default = "yes")]
    pub active: bool,
}

fn yes() -> bool {
    true
}

fn spain() -> String {
    "ES".into()
}

impl Shareholder {
    /// Tipo de retención de sus dividendos.
    pub fn rate(&self) -> Rate {
        if self.resident && self.legal_person && self.exempt {
            Rate::ZERO
        } else {
            DIVIDEND_RATE
        }
    }
}

const COLS: &str =
    "id, nif, name, address, province, country, pct, legal_person, resident, exempt, active";

fn shareholder_of(
    r: &taxpain_vault::rusqlite::Row<'_>,
) -> taxpain_vault::rusqlite::Result<Shareholder> {
    Ok(Shareholder {
        id: r.get(0)?,
        nif: r.get(1)?,
        name: r.get(2)?,
        address: r.get(3)?,
        province: r.get(4)?,
        country: r.get(5)?,
        pct: r.get(6)?,
        legal_person: r.get(7)?,
        resident: r.get(8)?,
        exempt: r.get(9)?,
        active: r.get(10)?,
    })
}

pub fn shareholders(v: &Vault) -> Result<Vec<Shareholder>> {
    let mut st = v.connection().prepare(&format!(
        "SELECT {COLS} FROM shareholder ORDER BY active DESC, pct DESC, id"
    ))?;
    let rows = st.query_map([], shareholder_of)?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn validate(s: &Shareholder) -> Result<(String, String, String)> {
    let country = s.country.trim().to_uppercase();
    let nif = s.nif.trim().to_uppercase();
    if s.resident {
        let n = Nif::parse(&nif).map_err(|e| other(format!("NIF del socio: {e}")))?;
        if n.is_persona_fisica() == s.legal_person {
            return Err(other(if s.legal_person {
                "ese NIF es de una persona física: desmarca «sociedad»"
            } else {
                "ese NIF es de una sociedad o entidad: marca «sociedad»"
            }));
        }
    } else if nif.is_empty() {
        return Err(other("pon el identificador fiscal del socio en su país"));
    }
    if s.name.trim().is_empty() {
        return Err(other("pon el nombre del socio"));
    }
    let province = s.province.trim().to_owned();
    if s.resident {
        let ok = province.len() == 2 && province.parse::<u8>().is_ok_and(|n| (1..=53).contains(&n));
        if !ok {
            return Err(other(
                "la provincia es el código de dos cifras del domicilio (01 a 53; 46 Valencia…)",
            ));
        }
        if country != "ES" {
            return Err(other("un socio residente en España tiene país ES"));
        }
    } else if country.len() != 2 || country == "ES" {
        return Err(other(
            "indica el país de residencia del socio (código de 2 letras)",
        ));
    }
    if s.pct == 0 || s.pct > FULL_PCT {
        return Err(other("la participación va de 0,0001 % a 100 %"));
    }
    if s.exempt && !(s.resident && s.legal_person) {
        return Err(other(
            "la exclusión de retención del art. 128.4.d LIS es sólo para sociedades residentes",
        ));
    }
    Ok((nif, province, country))
}

/// Da de alta (id 0) o modifica un socio. Devuelve su id.
pub fn save_shareholder(v: &Vault, s: &Shareholder, now: OffsetDateTime) -> Result<i64> {
    let (nif, province, country) = validate(s)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let id = if s.id == 0 {
        tx.execute(
            "INSERT INTO shareholder(nif, name, address, province, country, pct, legal_person,
               resident, exempt, active, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                nif,
                s.name.trim(),
                s.address.trim(),
                province,
                country,
                s.pct,
                s.legal_person,
                s.resident,
                s.exempt,
                s.active,
                now_str(now)
            ],
        )?;
        tx.last_insert_rowid()
    } else {
        let n = tx.execute(
            "UPDATE shareholder SET nif = ?1, name = ?2, address = ?3, province = ?4, country = ?5,
               pct = ?6, legal_person = ?7, resident = ?8, exempt = ?9, active = ?10 WHERE id = ?11",
            params![
                nif,
                s.name.trim(),
                s.address.trim(),
                province,
                country,
                s.pct,
                s.legal_person,
                s.resident,
                s.exempt,
                s.active,
                s.id
            ],
        )?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        s.id
    };
    // Los datos personales del socio no van a la auditoría, sólo el cambio.
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "shareholder.save",
        &id.to_string(),
        &serde_json::json!({ "pct": s.pct }),
    )?;
    tx.commit()?;
    Ok(id)
}

/// Borra un socio de la ficha (los pagos ya hechos guardan sus datos).
pub fn delete_shareholder(v: &Vault, id: i64, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    if tx.execute("DELETE FROM shareholder WHERE id = ?1", [id])? == 0 {
        return Err(AppError::NotFound);
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "shareholder.delete",
        &id.to_string(),
        &serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

/// Dividendo acordado pendiente de pago a `date`: saldo acreedor de la 526 ese día (diario
/// hasta entonces, sin cierres; la apertura sólo la del primer ejercicio, que trae los saldos
/// anteriores), pero no más que el de hoy: un pago con fecha posterior ya contabilizado también
/// se lleva lo acordado (si no, dos pagos fuera de orden pasarían los dos).
pub fn pending_dividends(conn: &Connection, date: Date) -> Result<Money> {
    let balance = |until: Option<String>| -> Result<i64> {
        Ok(conn.query_row(
            "SELECT COALESCE(SUM(l.credit - l.debit), 0) FROM journal_line l
               JOIN journal_entry e ON e.id = l.entry_id
              WHERE (?1 IS NULL OR e.date <= ?1) AND e.origin_kind <> 'Cierre'
                AND (e.origin_kind <> 'Apertura'
                     OR e.fiscal_year = (SELECT MIN(fiscal_year) FROM journal_entry))
                AND l.account LIKE '526%'",
            [until],
            |r| r.get(0),
        )?)
    };
    let at_date = balance(Some(date.to_string()))?;
    let latest = balance(None)?;
    Ok(Money::from_cents(at_date.min(latest)))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayoutInput {
    /// Fecha de exigibilidad: la de pago fijada en el acuerdo de la junta o, si no la fija, el
    /// día siguiente al acuerdo (art. 94.1 RIRPF; art. 65.1 RIS).
    pub date: Date,
    /// Dividendo bruto total.
    pub gross: Money,
    /// Cuenta de tesorería que paga (57x) o la 551 si se compensa con los socios.
    pub treasury: String,
    /// Acuerdo de reparto que se paga (`dividendos::record`). Si no se da, el más antiguo con
    /// algo pendiente a la fecha.
    #[serde(default)]
    pub agreement: Option<i64>,
}

/// Parte de un socio en un pago.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayoutLine {
    pub shareholder_id: Option<i64>,
    pub nif: String,
    pub name: String,
    pub province: String,
    pub legal_person: bool,
    pub resident: bool,
    pub pct: u32,
    pub gross: Money,
    pub rate: Rate,
    pub withheld: Money,
    pub net: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PayoutProposal {
    pub date: Date,
    pub gross: Money,
    /// Lo que queda por pagar en la 526 ese día.
    pub available: Money,
    pub lines: Vec<PayoutLine>,
    pub withheld: Money,
    pub net: Money,
    /// Asiento que se contabilizaría.
    pub entry: Vec<ProposedLine>,
    /// Acuerdo de reparto que se paga (el elegido o el más antiguo pendiente).
    pub agreement: Option<crate::dividendos::Agreement>,
    pub warnings: Vec<String>,
}

/// Reparte `gross` por participación entre `holders`: los céntimos que sobran del redondeo van
/// al socio con más participación (el primero si empatan).
pub fn split(gross: Money, holders: &[Shareholder]) -> Vec<PayoutLine> {
    let total: i128 = holders.iter().map(|h| i128::from(h.pct)).sum();
    if total == 0 {
        return vec![];
    }
    let mut lines: Vec<PayoutLine> = holders
        .iter()
        .map(|h| {
            let part = i128::from(gross.cents()) * i128::from(h.pct) / total;
            let g = Money::from_cents(i64::try_from(part).unwrap_or(0));
            PayoutLine {
                shareholder_id: Some(h.id),
                nif: h.nif.clone(),
                name: h.name.clone(),
                province: h.province.clone(),
                legal_person: h.legal_person,
                resident: h.resident,
                pct: h.pct,
                gross: g,
                rate: h.rate(),
                withheld: Money::ZERO,
                net: Money::ZERO,
            }
        })
        .collect();
    let rest = gross - lines.iter().map(|l| l.gross).sum::<Money>();
    if let Some(i) = holders
        .iter()
        .enumerate()
        .max_by(|(ia, a), (ib, b)| a.pct.cmp(&b.pct).then(ib.cmp(ia)))
        .map(|(i, _)| i)
        && let Some(l) = lines.get_mut(i)
    {
        l.gross += rest;
    }
    for l in &mut lines {
        l.withheld = l.gross.apply_rate(l.rate);
        l.net = l.gross - l.withheld;
    }
    lines
}

fn proposed(conn: &Connection, account: &str, debit: Money, credit: Money) -> Result<ProposedLine> {
    Ok(ProposedLine {
        name: accounting::account_name(conn, account)?.unwrap_or_default(),
        account: account.into(),
        debit,
        credit,
    })
}

/// Reparto y asiento de un pago de dividendos, sin contabilizar nada.
pub fn propose(v: &Vault, input: &PayoutInput) -> Result<PayoutProposal> {
    if !is_company(v)? {
        return Err(other("los dividendos los reparten las sociedades"));
    }
    let treasury = input.treasury.trim();
    if !(treasury.starts_with("57") || treasury.starts_with("551")) {
        return Err(other(
            "el pago sale de una cuenta de tesorería (57x) o de la cuenta corriente con los socios (551)",
        ));
    }
    code(treasury)?;
    if input.gross.cents() <= 0 {
        return Err(other("pon el dividendo bruto a pagar"));
    }
    let holders: Vec<Shareholder> = shareholders(v)?.into_iter().filter(|s| s.active).collect();
    if holders.is_empty() {
        return Err(other("da de alta antes a los socios con su participación"));
    }
    let conn = v.connection();
    let available = pending_dividends(conn, input.date)?;
    let lines = split(input.gross, &holders);
    let withheld: Money = lines.iter().map(|l| l.withheld).sum();
    let net = input.gross - withheld;
    let mut warnings = Vec::new();
    if input.gross > available {
        warnings.push(format!(
            "En la 526 «Dividendo activo a pagar» quedan {available} por pagar a esa fecha: \
             contabiliza antes la aplicación del resultado que acuerda la junta (Cierre)."
        ));
    }
    let agreement = match input.agreement {
        Some(id) => Some(crate::dividendos::agreement(conn, id)?),
        None => crate::dividendos::oldest_pending(conn, input.date)?,
    };
    match &agreement {
        None => warnings.push(
            "Este pago no está ligado a ningún acuerdo de reparto registrado: registra antes el \
             acuerdo (Acordar un dividendo), que comprueba los límites legales y crea el acta, \
             para que el pago quede documentado."
                .into(),
        ),
        Some(a) => {
            if input.gross > a.pending {
                warnings.push(format!(
                    "Del acuerdo del {} quedan {} por pagar: el pago no puede ser mayor.",
                    a.date, a.pending
                ));
            }
            if !a.minute_approved {
                warnings.push(format!(
                    "El acta del acuerdo del {} no está aprobada en el libro de actas (Libros \
                     societarios): apruébala antes de pagar.",
                    a.date
                ));
            }
        }
    }
    let sum_pct: u64 = holders.iter().map(|h| u64::from(h.pct)).sum();
    if sum_pct != u64::from(FULL_PCT) {
        warnings.push(format!(
            "Las participaciones de los socios activos suman {},{:04} %: se reparte en \
             proporción a ellas (revisa la ficha si no hay autocartera).",
            sum_pct / 10_000,
            sum_pct % 10_000
        ));
    }
    for l in &lines {
        if !l.resident {
            warnings.push(format!(
                "{}: no residente. Se le retiene el 19 % del IRNR (un convenio o la Directiva \
                 matriz-filial pueden reducirlo o eximirlo); no va en el 123 ni en el 193, sino en \
                 el 216 y el 296, que TaxPain aún no prepara.",
                l.name
            ));
        } else if l.rate == Rate::ZERO {
            warnings.push(format!(
                "{}: sin retención por tener derecho a la exención del art. 21 LIS (art. 128.4.d \
                 LIS y art. 61.p RIS). Hace falta su comunicación con los documentos que lo \
                 justifican (participación de al menos el 5 % poseída de forma ininterrumpida \
                 durante el año anterior); va en el 193 como dividendo no sometido a retención.",
                l.name
            ));
            if l.pct < EXEMPTION_MIN_PCT {
                warnings.push(format!(
                    "{}: su participación directa en la sociedad es del {} %, menos del 5 % que \
                     pide el art. 21.1.a LIS. Sólo tiene derecho a la exención (y a cobrar sin \
                     retención) si con la indirecta llega al 5 %; si no, desmarca la exención \
                     en su ficha y se le retendrá el 19 %.",
                    l.name,
                    pct_text(l.pct)
                ));
            }
        }
    }
    let mut entry = vec![proposed(conn, "526", input.gross, Money::ZERO)?];
    if !withheld.is_zero() {
        entry.push(proposed(conn, WITHHOLDING_ACCOUNT, Money::ZERO, withheld)?);
    }
    if !net.is_zero() {
        entry.push(proposed(conn, treasury, Money::ZERO, net)?);
    }
    Ok(PayoutProposal {
        date: input.date,
        gross: input.gross,
        available,
        lines,
        withheld,
        net,
        entry,
        agreement,
        warnings,
    })
}

/// Contabiliza el pago de dividendos (con confirmación: tiene efectos fiscales). Devuelve el id
/// del pago.
pub fn pay(v: &Vault, input: &PayoutInput, now: OffsetDateTime, confirmed: bool) -> Result<i64> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let p = propose(v, input)?;
    if p.gross > p.available {
        return Err(other(format!(
            "en la 526 quedan {} por pagar: el dividendo no puede ser mayor",
            p.available
        )));
    }
    if let Some(a) = &p.agreement {
        if p.gross > a.pending {
            return Err(other(format!(
                "del acuerdo del {} quedan {} por pagar: el dividendo no puede ser mayor",
                a.date, a.pending
            )));
        }
        if p.date < a.date {
            return Err(other(format!(
                "el pago es anterior al acuerdo del {}: la fecha de pago es la del acuerdo o \
                 posterior",
                a.date
            )));
        }
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    // El reparto del que sale el dividendo: la última aplicación del resultado contabilizada a la
    // fecha de exigibilidad (la 526 se paga por orden). Así deshacer esa aplicación sabe qué
    // pagos la bloquean, y no otros repartos.
    let allocation_year: Option<i64> = tx.query_row(
        "SELECT MAX(origin_id) FROM journal_entry
          WHERE origin_kind = 'AplicacionResultado' AND date <= ?1",
        [p.date.to_string()],
        |r| r.get(0),
    )?;
    tx.execute(
        "INSERT INTO dividend_payment(date, gross, withheld, treasury, created_at, allocation_year,
           agreement_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            p.date.to_string(),
            p.gross.cents(),
            p.withheld.cents(),
            input.treasury.trim(),
            now_str(now),
            allocation_year,
            p.agreement.as_ref().map(|a| a.id)
        ],
    )?;
    let id = tx.last_insert_rowid();
    for l in &p.lines {
        tx.execute(
            "INSERT INTO dividend_line(payment_id, shareholder_id, nif, name, province,
               legal_person, resident, pct, gross, rate_bp, withheld)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                l.shareholder_id,
                l.nif,
                l.name,
                l.province,
                l.legal_person,
                l.resident,
                l.pct,
                l.gross.cents(),
                l.rate.bp(),
                l.withheld.cents()
            ],
        )?;
    }
    let resident_wh: Money = p
        .lines
        .iter()
        .filter(|l| l.resident)
        .map(|l| l.withheld)
        .sum();
    let foreign_wh = p.withheld - resident_wh;
    let mut lines = vec![EntryLine::debit(code("526")?, p.gross)];
    for (m, what) in [
        (resident_wh, "Retención dividendos (modelo 123)"),
        (
            foreign_wh,
            "Retención dividendos no residentes (modelo 216)",
        ),
    ] {
        if !m.is_zero() {
            let mut l = EntryLine::credit(code(WITHHOLDING_ACCOUNT)?, m);
            l.description = Some(what.into());
            lines.push(l);
        }
    }
    if !p.net.is_zero() {
        lines.push(EntryLine::credit(code(input.treasury.trim())?, p.net));
    }
    let entry = JournalEntry {
        date: p.date,
        concept: "Pago de dividendos".into(),
        origin: Origin::Dividendo(id),
        lines,
    };
    let entry_id = accounting::record_entry(&tx, &entry, now)?;
    tx.execute(
        "UPDATE dividend_payment SET entry_id = ?1 WHERE id = ?2",
        params![entry_id, id],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "dividend.pay",
        &id.to_string(),
        &serde_json::json!({
            "entry": entry_id,
            "gross": p.gross.cents(),
            "withheld": p.withheld.cents(),
        }),
    )?;
    tx.commit()?;
    Ok(id)
}

/// Un pago de dividendos ya contabilizado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DividendPayment {
    pub id: i64,
    pub date: Date,
    pub gross: Money,
    pub withheld: Money,
    pub net: Money,
    pub treasury: String,
    pub entry_id: Option<i64>,
    /// Acuerdo de reparto del que sale.
    pub agreement_id: Option<i64>,
    pub lines: Vec<PayoutLine>,
}

fn lines_of(conn: &Connection, payment: i64) -> Result<Vec<PayoutLine>> {
    let mut st = conn.prepare(
        "SELECT shareholder_id, nif, name, province, legal_person, resident, pct, gross, rate_bp,
                withheld
         FROM dividend_line WHERE payment_id = ?1 ORDER BY id",
    )?;
    let rows = st.query_map([payment], |r| {
        let gross = Money::from_cents(r.get(7)?);
        let withheld = Money::from_cents(r.get(9)?);
        Ok(PayoutLine {
            shareholder_id: r.get(0)?,
            nif: r.get(1)?,
            name: r.get(2)?,
            province: r.get(3)?,
            legal_person: r.get(4)?,
            resident: r.get(5)?,
            pct: r.get(6)?,
            gross,
            rate: Rate::from_bp(r.get(8)?),
            withheld,
            net: gross - withheld,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Pagos de dividendos con fecha entre `start` y `end`, por fecha.
pub fn payments_between(v: &Vault, start: Date, end: Date) -> Result<Vec<DividendPayment>> {
    let conn = v.connection();
    let mut st = conn.prepare(
        "SELECT id, date, gross, withheld, treasury, entry_id, agreement_id FROM dividend_payment
         WHERE date BETWEEN ?1 AND ?2 ORDER BY date, id",
    )?;
    let rows = st.query_map(params![start.to_string(), end.to_string()], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, Option<i64>>(5)?,
            r.get::<_, Option<i64>>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, date, gross, withheld, treasury, entry_id, agreement_id) = row?;
        let (gross, withheld) = (Money::from_cents(gross), Money::from_cents(withheld));
        out.push(DividendPayment {
            id,
            date: parse_date(&date)?,
            gross,
            withheld,
            net: gross - withheld,
            treasury,
            entry_id,
            agreement_id,
            lines: lines_of(conn, id)?,
        });
    }
    Ok(out)
}

/// Pagos de dividendos del año natural `year`.
pub fn payments_of_year(v: &Vault, year: i32) -> Result<Vec<DividendPayment>> {
    let start = Date::from_calendar_date(year, time::Month::January, 1).map_err(other)?;
    let end = Date::from_calendar_date(year, time::Month::December, 31).map_err(other)?;
    payments_between(v, start, end)
}

/// Deshace un pago de dividendos y su asiento (si el ejercicio sigue abierto).
pub fn undo(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let entry: Option<Option<i64>> = conn
        .query_row(
            "SELECT entry_id FROM dividend_payment WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    let entry = entry.ok_or(AppError::NotFound)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM dividend_line WHERE payment_id = ?1", [id])?;
    tx.execute("DELETE FROM dividend_payment WHERE id = ?1", [id])?;
    if let Some(e) = entry {
        // Los disparadores impiden borrar en un ejercicio cerrado: el error lo explica.
        tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [e])
            .map_err(|_| other("el ejercicio de ese pago está cerrado: reábrelo"))?;
        tx.execute("DELETE FROM journal_entry WHERE id = ?1", [e])?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "dividend.undo",
        &id.to_string(),
        &serde_json::json!({ "entry": entry }),
    )?;
    tx.commit()?;
    Ok(())
}
