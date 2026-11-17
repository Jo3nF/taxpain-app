//! Libro registro de socios (arts. 104-106 LSC).
//!
//! «La sociedad de responsabilidad limitada llevará un libro registro de socios, cuya custodia
//! y llevanza corresponde al órgano de administración, en el que se harán constar la
//! titularidad originaria y las sucesivas transmisiones, voluntarias o forzosas, de las
//! participaciones sociales, y la constitución de derechos reales y otros gravámenes sobre las
//! mismas» (art. 104.1 LSC). En cada anotación, la identidad y el domicilio del titular o del
//! beneficiario de la carga; la Instrucción DGRN de 12/2/2015 pide además la nacionalidad (su
//! falta no impide legalizar, pero se hace constar).
//!
//! TaxPain guarda los movimientos y saca de ellos la situación a una fecha (quién tiene qué
//! numeración, cuánto nominal y qué porcentaje). Cada movimiento se comprueba contra lo
//! anterior (no se transmiten participaciones que no son del transmitente, no se crean números
//! que ya existen): el libro no puede quedar incoherente. La situación se coteja con la ficha
//! de socios, que reparte los dividendos, y se le pueden pasar los porcentajes.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::money::Money;
use taxpain_invoicing::pdf::Para;
use taxpain_models::m184::FULL_PCT;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::{Date, OffsetDateTime};

use super::{company, credit_balance_at, dmy, other, parse_date};
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

/// Tipo de movimiento de las participaciones.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MovementKind {
    /// Titularidad originaria de los fundadores.
    Constitucion,
    /// Participaciones nuevas (aumento de capital).
    Ampliacion,
    /// Amortización de participaciones (reducción de capital).
    Reduccion,
    /// Transmisión voluntaria o forzosa (compraventa, donación, herencia, adjudicación…).
    Transmision,
    /// Constitución de un derecho real o gravamen (prenda, usufructo, embargo…).
    Carga,
    CancelacionCarga,
}

impl MovementKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Constitucion => "constitucion",
            Self::Ampliacion => "ampliacion",
            Self::Reduccion => "reduccion",
            Self::Transmision => "transmision",
            Self::Carga => "carga",
            Self::CancelacionCarga => "cancelacion_carga",
        }
    }

    fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "constitucion" => Self::Constitucion,
            "ampliacion" => Self::Ampliacion,
            "reduccion" => Self::Reduccion,
            "transmision" => Self::Transmision,
            "carga" => Self::Carga,
            "cancelacion_carga" => Self::CancelacionCarga,
            _ => return Err(other(format!("movimiento desconocido: {s}"))),
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Constitucion => "Constitución (titularidad originaria)",
            Self::Ampliacion => "Ampliación de capital",
            Self::Reduccion => "Reducción de capital (amortización)",
            Self::Transmision => "Transmisión",
            Self::Carga => "Constitución de carga o gravamen",
            Self::CancelacionCarga => "Cancelación de carga",
        }
    }

    /// Tiene transmitente o titular previo (`from`).
    fn has_from(self) -> bool {
        !matches!(self, Self::Constitucion | Self::Ampliacion)
    }

    /// Tiene adquirente, suscriptor o beneficiario (`to`).
    fn has_to(self) -> bool {
        !matches!(self, Self::Reduccion | Self::CancelacionCarga)
    }
}

/// Un asiento del libro registro de socios.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareMovement {
    #[serde(default)]
    pub id: i64,
    pub date: Date,
    pub kind: MovementKind,
    /// Transmitente, o titular de las participaciones que se gravan, amortizan o liberan.
    #[serde(default)]
    pub from_nif: String,
    #[serde(default)]
    pub from_name: String,
    /// Suscriptor, adquirente o beneficiario de la carga.
    #[serde(default)]
    pub to_nif: String,
    #[serde(default)]
    pub to_name: String,
    #[serde(default)]
    pub to_address: String,
    #[serde(default)]
    pub to_nationality: String,
    /// Numeración de las participaciones (ambos incluidos).
    pub number_from: u32,
    pub number_to: u32,
    /// Valor nominal de cada participación.
    pub unit_nominal: Money,
    /// Precio o valor de la transmisión, si consta.
    #[serde(default)]
    pub price: Option<Money>,
    /// Carga o gravamen (prenda, usufructo, embargo…).
    #[serde(default)]
    pub charge: String,
    /// Título: escritura de constitución, de ampliación, de compraventa; póliza; documento
    /// privado…
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub notary: String,
    /// Número de protocolo.
    #[serde(default)]
    pub protocol: String,
    #[serde(default)]
    pub title_date: Option<Date>,
    #[serde(default)]
    pub notes: String,
}

impl ShareMovement {
    pub fn count(&self) -> u32 {
        self.number_to.saturating_sub(self.number_from) + 1
    }
}

/// Carga vigente sobre unas participaciones.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Charge {
    pub charge: String,
    pub beneficiary_nif: String,
    pub beneficiary_name: String,
    pub date: Date,
}

/// Bloque de numeración de un titular.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Block {
    pub from: u32,
    pub to: u32,
    pub unit_nominal: Money,
    pub charges: Vec<Charge>,
}

/// Participaciones de un socio en una fecha.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Holding {
    pub nif: String,
    pub name: String,
    pub address: String,
    pub nationality: String,
    pub blocks: Vec<Block>,
    pub count: u32,
    pub nominal: Money,
    /// Porcentaje del capital (por nominal) en millonésimas; suman 1.000.000 (resto mayor).
    pub pct: u32,
}

/// Situación del capital en una fecha.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Situation {
    pub count: u32,
    pub capital: Money,
    pub holders: Vec<Holding>,
}

#[derive(Clone, Debug)]
struct Seg {
    from: u32,
    to: u32,
    owner: String,
    nominal: Money,
    charges: Vec<Charge>,
}

#[derive(Default)]
struct Book {
    segs: Vec<Seg>,
    /// NIF → (nombre, domicilio, nacionalidad) del último movimiento que lo nombra.
    people: BTreeMap<String, (String, String, String)>,
}

impl Book {
    /// Parte el bloque que contiene `n` para que `n` empiece uno.
    fn split_at(&mut self, n: u32) {
        if let Some(i) = self.segs.iter().position(|s| s.from < n && n <= s.to) {
            let mut right = self.segs[i].clone();
            right.from = n;
            self.segs[i].to = n - 1;
            self.segs.insert(i + 1, right);
        }
    }

    /// Índices de los bloques que cubren exactamente `a..=b`, o `None` si falta algún número.
    fn cover(&mut self, a: u32, b: u32) -> Option<std::ops::Range<usize>> {
        self.split_at(a);
        if let Some(next) = b.checked_add(1) {
            self.split_at(next);
        }
        let first = self.segs.iter().position(|s| s.from == a)?;
        let mut expected = a;
        let mut end = first;
        while end < self.segs.len() && self.segs[end].to <= b {
            if self.segs[end].from != expected {
                return None;
            }
            expected = self.segs[end].to.saturating_add(1);
            end += 1;
        }
        (self.segs[end - 1].to == b).then_some(first..end)
    }

    /// Bloques de `a..=b`, todos de `owner`.
    fn owned(
        &mut self,
        a: u32,
        b: u32,
        owner: &str,
    ) -> std::result::Result<std::ops::Range<usize>, String> {
        let r = self
            .cover(a, b)
            .ok_or_else(|| format!("las participaciones {a} a {b} no existen todas"))?;
        if self.segs[r.clone()].iter().any(|s| s.owner != owner) {
            return Err(format!(
                "las participaciones {a} a {b} no son todas de {owner}"
            ));
        }
        Ok(r)
    }

    fn apply(&mut self, m: &ShareMovement) -> std::result::Result<(), String> {
        let (a, b) = (m.number_from, m.number_to);
        if m.kind.has_to() && !m.to_nif.is_empty() {
            self.people.insert(
                m.to_nif.clone(),
                (
                    m.to_name.clone(),
                    m.to_address.clone(),
                    m.to_nationality.clone(),
                ),
            );
        }
        if m.kind.has_from() && !m.from_nif.is_empty() {
            self.people
                .entry(m.from_nif.clone())
                .or_insert_with(|| (m.from_name.clone(), String::new(), String::new()));
        }
        match m.kind {
            MovementKind::Constitucion | MovementKind::Ampliacion => {
                if self.segs.iter().any(|s| s.from <= b && a <= s.to) {
                    return Err(format!(
                        "ya hay participaciones con números entre el {a} y el {b}: las nuevas \
                         siguen la numeración"
                    ));
                }
                let at = self
                    .segs
                    .iter()
                    .position(|s| s.from > b)
                    .unwrap_or(self.segs.len());
                self.segs.insert(
                    at,
                    Seg {
                        from: a,
                        to: b,
                        owner: m.to_nif.clone(),
                        nominal: m.unit_nominal,
                        charges: vec![],
                    },
                );
            }
            MovementKind::Reduccion => {
                let r = self.owned(a, b, &m.from_nif)?;
                self.segs.drain(r);
            }
            MovementKind::Transmision => {
                let r = self.owned(a, b, &m.from_nif)?;
                for s in &mut self.segs[r] {
                    s.owner = m.to_nif.clone();
                }
            }
            MovementKind::Carga => {
                let r = self.owned(a, b, &m.from_nif)?;
                for s in &mut self.segs[r] {
                    s.charges.push(Charge {
                        charge: m.charge.clone(),
                        beneficiary_nif: m.to_nif.clone(),
                        beneficiary_name: m.to_name.clone(),
                        date: m.date,
                    });
                }
            }
            MovementKind::CancelacionCarga => {
                let r = self.owned(a, b, &m.from_nif)?;
                let wanted = m.charge.trim().to_lowercase();
                let mut found = false;
                for s in &mut self.segs[r] {
                    let before = s.charges.len();
                    s.charges
                        .retain(|c| !wanted.is_empty() && c.charge.trim().to_lowercase() != wanted);
                    found |= s.charges.len() != before;
                }
                if !found {
                    return Err(format!(
                        "las participaciones {a} a {b} no tienen la carga «{}»",
                        m.charge.trim()
                    ));
                }
            }
        }
        Ok(())
    }

    fn situation(&self) -> Situation {
        let mut by_owner: BTreeMap<&str, Vec<&Seg>> = BTreeMap::new();
        for s in &self.segs {
            by_owner.entry(&s.owner).or_default().push(s);
        }
        let capital: Money = self
            .segs
            .iter()
            .map(|s| Money::from_cents(s.nominal.cents() * i64::from(s.to - s.from + 1)))
            .sum();
        let mut holders: Vec<Holding> = by_owner
            .into_iter()
            .map(|(nif, segs)| {
                let mut blocks: Vec<Block> = Vec::new();
                for s in segs {
                    match blocks.last_mut() {
                        Some(last)
                            if last.to + 1 == s.from
                                && last.unit_nominal == s.nominal
                                && last.charges == s.charges =>
                        {
                            last.to = s.to;
                        }
                        _ => blocks.push(Block {
                            from: s.from,
                            to: s.to,
                            unit_nominal: s.nominal,
                            charges: s.charges.clone(),
                        }),
                    }
                }
                let count = blocks.iter().map(|b| b.to - b.from + 1).sum();
                let nominal = blocks
                    .iter()
                    .map(|b| {
                        Money::from_cents(b.unit_nominal.cents() * i64::from(b.to - b.from + 1))
                    })
                    .sum();
                let (name, address, nationality) =
                    self.people.get(nif).cloned().unwrap_or_default();
                Holding {
                    nif: nif.to_owned(),
                    name,
                    address,
                    nationality,
                    blocks,
                    count,
                    nominal,
                    pct: 0,
                }
            })
            .collect();
        // Porcentajes por nominal, en millonésimas, que suman el 100 % (resto mayor).
        if capital.cents() > 0 {
            let total = i128::from(capital.cents());
            let full = i128::from(FULL_PCT);
            let mut rests: Vec<(usize, i128)> = Vec::new();
            let mut given: i128 = 0;
            for (i, h) in holders.iter_mut().enumerate() {
                let exact = i128::from(h.nominal.cents()) * full;
                h.pct = u32::try_from(exact / total).unwrap_or(0);
                given += exact / total;
                rests.push((i, exact % total));
            }
            rests.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
            for (i, _) in rests
                .into_iter()
                .take(usize::try_from(full - given).unwrap_or(0))
            {
                holders[i].pct += 1;
            }
        }
        holders.sort_by(|a, b| b.nominal.cmp(&a.nominal).then(a.nif.cmp(&b.nif)));
        Situation {
            count: self.segs.iter().map(|s| s.to - s.from + 1).sum(),
            capital,
            holders,
        }
    }
}

/// Reproduce los movimientos (ya en orden) hasta `until` (incluido; todos si es `None`). Error
/// con el primero que no cuadra con lo anterior.
pub fn replay(movements: &[ShareMovement], until: Option<Date>) -> Result<Situation> {
    let mut book = Book::default();
    for m in movements {
        if until.is_some_and(|u| m.date > u) {
            break;
        }
        book.apply(m)
            .map_err(|e| other(format!("{} del {}: {e}", m.kind.label(), dmy(m.date))))?;
    }
    Ok(book.situation())
}

const COLS: &str = "id, date, kind, from_nif, from_name, to_nif, to_name, to_address, \
    to_nationality, number_from, number_to, unit_nominal, price, charge, title, notary, protocol, \
    title_date, notes";

/// Movimientos en orden cronológico (y de alta, el mismo día).
pub fn movements(v: &Vault) -> Result<Vec<ShareMovement>> {
    let mut st = v.connection().prepare(&format!(
        "SELECT {COLS} FROM share_movement ORDER BY date, id"
    ))?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            [
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
                r.get::<_, String>(7)?,
                r.get::<_, String>(8)?,
            ],
            (r.get::<_, u32>(9)?, r.get::<_, u32>(10)?),
            (r.get::<_, i64>(11)?, r.get::<_, Option<i64>>(12)?),
            [
                r.get::<_, String>(13)?,
                r.get::<_, String>(14)?,
                r.get::<_, String>(15)?,
                r.get::<_, String>(16)?,
            ],
            r.get::<_, Option<String>>(17)?,
            r.get::<_, String>(18)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, date, kind, who, (number_from, number_to), (nominal, price), t, td, notes) = row?;
        let [
            from_nif,
            from_name,
            to_nif,
            to_name,
            to_address,
            to_nationality,
        ] = who;
        let [charge, title, notary, protocol] = t;
        out.push(ShareMovement {
            id,
            date: parse_date(&date)?,
            kind: MovementKind::parse(&kind)?,
            from_nif,
            from_name,
            to_nif,
            to_name,
            to_address,
            to_nationality,
            number_from,
            number_to,
            unit_nominal: Money::from_cents(nominal),
            price: price.map(Money::from_cents),
            charge,
            title,
            notary,
            protocol,
            title_date: td.as_deref().map(parse_date).transpose()?,
            notes,
        });
    }
    Ok(out)
}

fn clean(m: &ShareMovement) -> Result<ShareMovement> {
    let up = |s: &str| s.trim().to_uppercase();
    let mut c = m.clone();
    for s in [
        &mut c.from_name,
        &mut c.to_name,
        &mut c.to_address,
        &mut c.to_nationality,
        &mut c.charge,
        &mut c.title,
        &mut c.notary,
        &mut c.protocol,
        &mut c.notes,
    ] {
        *s = s.trim().to_owned();
    }
    c.from_nif = up(&m.from_nif);
    c.to_nif = up(&m.to_nif);
    if !c.kind.has_from() {
        c.from_nif.clear();
        c.from_name.clear();
    }
    if !c.kind.has_to() {
        c.to_nif.clear();
        c.to_name.clear();
        c.to_address.clear();
        c.to_nationality.clear();
    }
    if c.number_from == 0 || c.number_to < c.number_from {
        return Err(other(
            "la numeración va del primer número al último (desde 1, y el último no puede ser menor)",
        ));
    }
    if c.unit_nominal.cents() <= 0 {
        return Err(other("pon el valor nominal de cada participación"));
    }
    if c.price.is_some_and(|p| p.is_negative()) {
        return Err(other("el precio no puede ser negativo"));
    }
    if c.kind.has_from() && (c.from_nif.is_empty() || c.from_name.is_empty()) {
        return Err(other(match c.kind {
            MovementKind::Transmision => "pon el NIF y el nombre del transmitente",
            _ => "pon el NIF y el nombre del titular de las participaciones",
        }));
    }
    if c.kind.has_to() && (c.to_nif.is_empty() || c.to_name.is_empty()) {
        return Err(other(match c.kind {
            MovementKind::Carga => "pon el NIF y el nombre del beneficiario de la carga",
            MovementKind::Transmision => "pon el NIF y el nombre del adquirente",
            _ => "pon el NIF y el nombre del socio que suscribe las participaciones",
        }));
    }
    if c.kind == MovementKind::Transmision && c.from_nif == c.to_nif {
        return Err(other("el adquirente es el mismo que el transmitente"));
    }
    if matches!(c.kind, MovementKind::Carga | MovementKind::CancelacionCarga) && c.charge.is_empty()
    {
        return Err(other(
            "indica la carga (prenda, usufructo, embargo…), con el mismo nombre al cancelarla",
        ));
    }
    Ok(c)
}

/// Da de alta (id 0) o modifica un movimiento. Se rechaza si deja el libro incoherente.
pub fn save_movement(v: &Vault, m: &ShareMovement, now: OffsetDateTime) -> Result<i64> {
    company(v)?;
    let m = clean(m)?;
    // El libro con este movimiento (en su sitio) ha de cuadrar entero.
    let existing = movements(v)?;
    if m.id != 0 && !existing.iter().any(|x| x.id == m.id) {
        return Err(AppError::NotFound);
    }
    let mut all: Vec<ShareMovement> = existing.into_iter().filter(|x| x.id != m.id).collect();
    let mut probe = m.clone();
    if probe.id == 0 {
        probe.id = i64::MAX;
    }
    all.push(probe);
    all.sort_by(|a, b| a.date.cmp(&b.date).then(a.id.cmp(&b.id)));
    replay(&all, None)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    // Alta: la fila y luego, como en una modificación, todos sus datos.
    let id = if m.id == 0 {
        tx.execute(
            "INSERT INTO share_movement(date, kind, number_from, number_to, unit_nominal,
               created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                m.date.to_string(),
                m.kind.as_str(),
                m.number_from,
                m.number_to,
                m.unit_nominal.cents(),
                now_str(now)
            ],
        )?;
        tx.last_insert_rowid()
    } else {
        m.id
    };
    tx.execute(
        "UPDATE share_movement SET date = ?1, kind = ?2, from_nif = ?3, from_name = ?4,
           to_nif = ?5, to_name = ?6, to_address = ?7, to_nationality = ?8,
           number_from = ?9, number_to = ?10, unit_nominal = ?11, price = ?12, charge = ?13,
           title = ?14, notary = ?15, protocol = ?16, title_date = ?17, notes = ?18
         WHERE id = ?19",
        params![
            m.date.to_string(),
            m.kind.as_str(),
            m.from_nif,
            m.from_name,
            m.to_nif,
            m.to_name,
            m.to_address,
            m.to_nationality,
            m.number_from,
            m.number_to,
            m.unit_nominal.cents(),
            m.price.map(Money::cents),
            m.charge,
            m.title,
            m.notary,
            m.protocol,
            m.title_date.map(|d| d.to_string()),
            m.notes,
            id
        ],
    )?;
    // Sin los datos personales: sólo qué se anotó.
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "share_movement.save",
        &id.to_string(),
        &serde_json::json!({
            "kind": m.kind.as_str(),
            "date": m.date.to_string(),
            "from": m.number_from,
            "to": m.number_to,
        }),
    )?;
    tx.commit()?;
    Ok(id)
}

/// Borra un movimiento (si lo que queda sigue cuadrando: antes los posteriores que dependan de
/// él).
pub fn delete_movement(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let all = movements(v)?;
    if !all.iter().any(|m| m.id == id) {
        return Err(AppError::NotFound);
    }
    let rest: Vec<ShareMovement> = all.into_iter().filter(|m| m.id != id).collect();
    replay(&rest, None).map_err(|e| {
        other(format!(
            "sin ese movimiento el libro no cuadra ({e}): borra antes los posteriores que dependen de él"
        ))
    })?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM share_movement WHERE id = ?1", [id])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "share_movement.delete",
        &id.to_string(),
        &serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

/// Libro, situación actual y cotejo con la ficha de socios y con la contabilidad.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Register {
    pub movements: Vec<ShareMovement>,
    pub situation: Situation,
    /// Diferencias con la ficha de socios y con la cuenta 100 (capital).
    pub checks: Vec<String>,
    /// La ficha de socios tiene los porcentajes del libro (y nadie más activo).
    pub in_sync: bool,
    /// Capital social de la contabilidad (saldo de la 100), si hay asientos.
    pub capital_books: Option<Money>,
}

/// Cotejo de la situación con la ficha de socios: diferencias y si cuadra.
fn compare(situation: &Situation, ficha: &[crate::socios::Shareholder]) -> (Vec<String>, bool) {
    let show = |p: u32| {
        let (i, f) = (p / 10_000, p % 10_000);
        if f == 0 {
            i.to_string()
        } else {
            format!("{i},{}", format!("{f:04}").trim_end_matches('0'))
        }
    };
    let mut checks = Vec::new();
    let mut ok = true;
    for h in &situation.holders {
        match ficha.iter().find(|s| s.nif.eq_ignore_ascii_case(&h.nif)) {
            None => {
                ok = false;
                checks.push(format!(
                    "{} ({}) tiene {} participaciones ({} %) en el libro y no está en la ficha de \
                     socios: dalo de alta allí (Cierre → Socios y dividendos) para repartirle \
                     dividendos.",
                    h.name,
                    h.nif,
                    h.count,
                    show(h.pct)
                ));
            }
            Some(s) if !s.active || s.pct != h.pct => {
                ok = false;
                checks.push(format!(
                    "{}: la ficha de socios dice {} y el libro, {} %.",
                    h.name,
                    if s.active {
                        format!("{} %", show(s.pct))
                    } else {
                        "que está de baja".into()
                    },
                    show(h.pct)
                ));
            }
            Some(_) => {}
        }
    }
    for s in ficha.iter().filter(|s| s.active) {
        if !situation
            .holders
            .iter()
            .any(|h| h.nif.eq_ignore_ascii_case(&s.nif))
        {
            ok = false;
            checks.push(format!(
                "{} ({}) está en la ficha de socios con el {} % y no tiene participaciones en el \
                 libro.",
                s.name,
                s.nif,
                show(s.pct)
            ));
        }
    }
    (checks, ok)
}

pub fn register(v: &Vault, today: Date) -> Result<Register> {
    company(v)?;
    let movements = movements(v)?;
    let situation = replay(&movements, None)?;
    let ficha = crate::socios::shareholders(v)?;
    let (mut checks, in_sync) = if movements.is_empty() {
        (vec![], false)
    } else {
        compare(&situation, &ficha)
    };
    let conn = v.connection();
    let has_entries: bool = conn
        .query_row(
            "SELECT 1 FROM journal_line WHERE account LIKE '100%' LIMIT 1",
            [],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    let capital_books = if has_entries {
        Some(Money::from_cents(credit_balance_at(conn, "100", today)?))
    } else {
        None
    };
    if let Some(c) = capital_books
        && !movements.is_empty()
        && c != situation.capital
    {
        checks.push(format!(
            "El capital del libro ({}) no es el de la contabilidad (cuenta 100: {c}): revisa \
             los movimientos o el asiento de la ampliación o reducción.",
            situation.capital
        ));
    }
    if situation.holders.len() == 1 && !movements.is_empty() {
        checks.push(
            "Sociedad unipersonal: la unipersonalidad y el cambio de socio único se inscriben en \
             el Registro Mercantil (art. 13 LSC), la sociedad lo indica en su documentación (art. \
             13.2) y sus contratos con el socio único van al libro de contratos (art. 16)."
                .into(),
        );
    }
    Ok(Register {
        movements,
        situation,
        checks,
        in_sync,
        capital_books,
    })
}

/// Pasa a la ficha de socios los porcentajes del libro: actualiza a los que están (por NIF) y
/// da de baja a los activos que ya no tienen participaciones. A los que faltan en la ficha hay
/// que darlos de alta a mano (piden provincia y residencia). Devuelve cuántos cambió.
pub fn apply_to_shareholders(v: &Vault, now: OffsetDateTime, confirmed: bool) -> Result<usize> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    company(v)?;
    let movements = movements(v)?;
    if movements.is_empty() {
        return Err(other("el libro registro de socios está vacío"));
    }
    let situation = replay(&movements, None)?;
    let ficha = crate::socios::shareholders(v)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let mut changed = 0;
    for s in &ficha {
        match situation
            .holders
            .iter()
            .find(|h| h.nif.eq_ignore_ascii_case(&s.nif))
        {
            Some(h) if h.pct > 0 && (h.pct != s.pct || !s.active) => {
                tx.execute(
                    "UPDATE shareholder SET pct = ?1, active = 1 WHERE id = ?2",
                    params![h.pct, s.id],
                )?;
                changed += 1;
            }
            None if s.active => {
                tx.execute("UPDATE shareholder SET active = 0 WHERE id = ?1", [s.id])?;
                changed += 1;
            }
            _ => {}
        }
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "partners_register.apply",
        "",
        &serde_json::json!({ "changed": changed }),
    )?;
    tx.commit()?;
    Ok(changed)
}

fn block_text(b: &Block) -> String {
    let n = b.to - b.from + 1;
    let mut s = if n == 1 {
        format!("nº {} (1 participación de {})", b.from, b.unit_nominal)
    } else {
        format!(
            "nº {} a {} ({n} participaciones de {} cada una)",
            b.from, b.to, b.unit_nominal
        )
    };
    for c in &b.charges {
        s.push_str(&format!(
            "; {} a favor de {} ({}) desde el {}",
            c.charge,
            c.beneficiary_name,
            c.beneficiary_nif,
            dmy(c.date)
        ));
    }
    s
}

fn situation_paras(p: &mut Vec<Para>, s: &Situation) {
    if s.holders.is_empty() {
        p.push(Para::Text("Sin participaciones.".into()));
        return;
    }
    for h in &s.holders {
        p.push(Para::Bold(format!(
            "{} · NIF {} · {} participaciones · {} · {} %",
            h.name,
            h.nif,
            h.count,
            h.nominal,
            pct_text(h.pct)
        )));
        let mut who = Vec::new();
        if !h.address.is_empty() {
            who.push(format!("Domicilio: {}", h.address));
        }
        if !h.nationality.is_empty() {
            who.push(format!("Nacionalidad: {}", h.nationality));
        }
        if !who.is_empty() {
            p.push(Para::Text(who.join(" · ")));
        }
        for b in &h.blocks {
            p.push(Para::Text(format!("Participaciones {}", block_text(b))));
        }
    }
    p.push(Para::Row(
        format!("Capital social: {} participaciones", s.count),
        s.capital.to_string(),
    ));
}

fn pct_text(p: u32) -> String {
    let (i, f) = (p / 10_000, p % 10_000);
    if f == 0 {
        i.to_string()
    } else {
        format!("{i},{}", format!("{f:04}").trim_end_matches('0'))
    }
}

fn movement_paras(p: &mut Vec<Para>, n: usize, m: &ShareMovement) {
    p.push(Para::Bold(format!(
        "Asiento {n} · {} · {}",
        dmy(m.date),
        m.kind.label()
    )));
    let numbers = if m.count() == 1 {
        format!("participación nº {}", m.number_from)
    } else {
        format!(
            "participaciones nº {} a {} ({} participaciones)",
            m.number_from,
            m.number_to,
            m.count()
        )
    };
    let total = Money::from_cents(m.unit_nominal.cents() * i64::from(m.count()));
    p.push(Para::Text(format!(
        "{numbers}, de {} de valor nominal cada una ({total} en total).",
        m.unit_nominal
    )));
    let to = || {
        let mut s = format!("{} (NIF {})", m.to_name, m.to_nif);
        if !m.to_nationality.is_empty() {
            s.push_str(&format!(", de nacionalidad {}", m.to_nationality));
        }
        if !m.to_address.is_empty() {
            s.push_str(&format!(", con domicilio en {}", m.to_address));
        }
        s
    };
    let from = format!("{} (NIF {})", m.from_name, m.from_nif);
    p.push(Para::Text(match m.kind {
        MovementKind::Constitucion | MovementKind::Ampliacion => format!("Suscriptor: {}.", to()),
        MovementKind::Reduccion => format!("Titular de las participaciones amortizadas: {from}."),
        MovementKind::Transmision => format!("Transmitente: {from}. Adquirente: {}.", to()),
        MovementKind::Carga => format!(
            "Titular: {from}. Carga: {}. Beneficiario: {}.",
            m.charge,
            to()
        ),
        MovementKind::CancelacionCarga => {
            format!("Titular: {from}. Se cancela: {}.", m.charge)
        }
    }));
    if let Some(price) = m.price {
        p.push(Para::Text(format!("Precio o valor: {price}.")));
    }
    let mut title = Vec::new();
    if !m.title.is_empty() {
        title.push(m.title.clone());
    }
    if let Some(d) = m.title_date {
        title.push(format!("de fecha {}", dmy(d)));
    }
    if !m.notary.is_empty() {
        title.push(format!("autorizada por el notario {}", m.notary));
    }
    if !m.protocol.is_empty() {
        title.push(format!("nº de protocolo {}", m.protocol));
    }
    if !title.is_empty() {
        p.push(Para::Text(format!("Título: {}.", title.join(", "))));
    }
    if !m.notes.is_empty() {
        p.push(Para::Text(m.notes.clone()));
    }
    p.push(Para::Gap);
}

/// El libro en párrafos: si se da el ejercicio, la situación al empezar, los asientos del
/// ejercicio y la situación al cierre; si no, todos los asientos y la situación actual.
pub fn book_paras(v: &Vault, year: Option<i32>, number: Option<u16>) -> Result<Vec<Para>> {
    let c = company(v)?;
    let movements = movements(v)?;
    let mut p = vec![Para::Title(match number {
        Some(n) => format!("Libro registro de socios nº {n}"),
        None => "Libro registro de socios".into(),
    })];
    p.push(Para::Text(format!("{} · NIF {}", c.name, c.nif)));
    if !c.address.is_empty() {
        p.push(Para::Text(format!("Domicilio social: {}", c.address)));
    }
    let (start, end) = match year {
        Some(y) => {
            let fy = crate::accounting::fiscal_year(v, y)?;
            p.push(Para::Text(format!(
                "Ejercicio {y}: del {} al {}",
                dmy(fy.start),
                dmy(fy.end)
            )));
            (Some(fy.start), Some(fy.end))
        }
        None => (None, None),
    };
    p.push(Para::Text(
        "Titularidad originaria y sucesivas transmisiones de las participaciones sociales y \
         constitución de derechos reales y otros gravámenes sobre ellas (arts. 104 a 106 de la \
         Ley de Sociedades de Capital)."
            .into(),
    ));
    p.push(Para::Gap);
    if let Some(s) = start {
        let before: Vec<ShareMovement> = movements.iter().filter(|m| m.date < s).cloned().collect();
        if !before.is_empty() {
            p.push(Para::Bold(format!("Situación al {}", dmy(s))));
            situation_paras(&mut p, &replay(&before, None)?);
            p.push(Para::Gap);
        }
    }
    p.push(Para::Bold("Asientos".into()));
    let mut n = 0;
    for (i, m) in movements.iter().enumerate() {
        if start.is_some_and(|s| m.date < s) || end.is_some_and(|e| m.date > e) {
            continue;
        }
        n += 1;
        movement_paras(&mut p, i + 1, m);
    }
    if n == 0 {
        p.push(Para::Text(
            "Sin cambios de titularidad ni gravámenes en el periodo.".into(),
        ));
        p.push(Para::Gap);
    }
    p.push(Para::Bold(match end {
        Some(e) => format!("Situación al {}", dmy(e)),
        None => "Situación actual".into(),
    }));
    situation_paras(&mut p, &replay(&movements, end)?);
    p.push(Para::Gap);
    p.push(Para::Text(
        "El órgano de administración: ______________________________".into(),
    ));
    Ok(p)
}

/// Movimientos con fecha dentro de `start..=end`.
pub(crate) fn movements_between(v: &Vault, start: Date, end: Date) -> Result<usize> {
    Ok(movements(v)?
        .iter()
        .filter(|m| m.date >= start && m.date <= end)
        .count())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn mov(kind: MovementKind, d: Date, from: &str, to: &str, a: u32, b: u32) -> ShareMovement {
        ShareMovement {
            id: 0,
            date: d,
            kind,
            from_nif: from.into(),
            from_name: from.into(),
            to_nif: to.into(),
            to_name: to.into(),
            to_address: String::new(),
            to_nationality: String::new(),
            number_from: a,
            number_to: b,
            unit_nominal: Money::from_euros(1, 0),
            price: None,
            charge: "Prenda".into(),
            title: String::new(),
            notary: String::new(),
            protocol: String::new(),
            title_date: None,
            notes: String::new(),
        }
    }

    #[test]
    fn transmision_parcial_carga_y_porcentajes() {
        use MovementKind::*;
        let m = vec![
            mov(Constitucion, date!(2024 - 01 - 10), "", "A", 1, 3000),
            mov(Transmision, date!(2025 - 03 - 01), "A", "B", 1001, 2000),
            mov(Carga, date!(2025 - 04 - 01), "B", "BANCO", 1501, 2000),
            mov(
                Ampliacion,
                date!(2025 - 06 - 01),
                "",
                "C",
                3001,
                3000 + 1000,
            ),
        ];
        let s = replay(&m, None).unwrap();
        assert_eq!(s.count, 4000);
        assert_eq!(s.capital, Money::from_euros(4000, 0));
        let pcts: Vec<(&str, u32)> = s.holders.iter().map(|h| (h.nif.as_str(), h.pct)).collect();
        assert_eq!(pcts, vec![("A", 500_000), ("B", 250_000), ("C", 250_000)]);
        let a = &s.holders[0];
        assert_eq!(
            a.blocks.iter().map(|b| (b.from, b.to)).collect::<Vec<_>>(),
            vec![(1, 1000), (2001, 3000)]
        );
        let b = &s.holders[1];
        assert_eq!(b.blocks.len(), 2, "la parte con prenda va aparte");
        assert_eq!(b.blocks[1].charges[0].beneficiary_nif, "BANCO");
        // Hasta antes de la transmisión, todo de A.
        let s0 = replay(&m, Some(date!(2025 - 02 - 28))).unwrap();
        assert_eq!(s0.holders.len(), 1);
        assert_eq!(s0.holders[0].pct, FULL_PCT);
    }

    #[test]
    fn tercios_suman_el_cien_por_cien() {
        use MovementKind::*;
        let m = vec![
            mov(Constitucion, date!(2024 - 01 - 10), "", "A", 1, 1),
            mov(Constitucion, date!(2024 - 01 - 10), "", "B", 2, 2),
            mov(Constitucion, date!(2024 - 01 - 10), "", "C", 3, 3),
        ];
        let s = replay(&m, None).unwrap();
        assert_eq!(s.holders.iter().map(|h| h.pct).sum::<u32>(), FULL_PCT);
    }

    #[test]
    fn incoherencias() {
        use MovementKind::*;
        let base = mov(Constitucion, date!(2024 - 01 - 10), "", "A", 1, 100);
        // Números repetidos.
        let e = replay(
            &[
                base.clone(),
                mov(Ampliacion, date!(2024 - 05 - 01), "", "B", 100, 150),
            ],
            None,
        )
        .unwrap_err();
        assert!(e.to_string().contains("ya hay participaciones"), "{e}");
        // Transmite lo que no es suyo.
        let e = replay(
            &[
                base.clone(),
                mov(Transmision, date!(2024 - 05 - 01), "B", "C", 1, 10),
            ],
            None,
        )
        .unwrap_err();
        assert!(e.to_string().contains("no son todas de B"), "{e}");
        // Números que no existen.
        let e = replay(
            &[
                base.clone(),
                mov(Transmision, date!(2024 - 05 - 01), "A", "C", 90, 110),
            ],
            None,
        )
        .unwrap_err();
        assert!(e.to_string().contains("no existen"), "{e}");
        // Cancelar una carga que no hay.
        let e = replay(
            &[
                base.clone(),
                mov(CancelacionCarga, date!(2024 - 05 - 01), "A", "", 1, 10),
            ],
            None,
        )
        .unwrap_err();
        assert!(e.to_string().contains("no tienen la carga"), "{e}");
        // Reducción: se amortizan.
        let s = replay(
            &[
                base,
                mov(Reduccion, date!(2024 - 05 - 01), "A", "", 51, 100),
            ],
            None,
        )
        .unwrap();
        assert_eq!(s.count, 50);
    }
}
