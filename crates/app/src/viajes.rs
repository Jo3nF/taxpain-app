//! Gastos de viaje: dietas, kilometraje, alojamiento, transporte, peajes y aparcamiento de un
//! empleado o un administrador, con la parte exceptuada de gravamen y la que es retribución.
//!
//! Límites del art. 9 RIRPF (RD 439/2007), vigentes en 2026:
//! - Kilometraje en vehículo propio: 0,26 €/km (Orden HFP/792/2023, en vigor desde el
//!   17-07-2023; antes, 0,19 €/km) más los peajes y el aparcamiento que se justifiquen
//!   (art. 9.A.2.b). Transporte público: lo que se justifique con factura (art. 9.A.2.a).
//! - Manutención (art. 9.A.3): en España 26,67 € al día sin pernoctar y 53,34 € pernoctando; en
//!   el extranjero 48,08 € y 91,35 €. Estancia (hotel): lo que se justifique.
//! - Sólo en un municipio distinto del lugar de trabajo habitual y de la residencia, sin pasar
//!   de nueve meses seguidos en el mismo; el pagador ha de acreditar el día, el lugar y el
//!   motivo del desplazamiento (art. 9.A.3).
//!
//! Lo exceptuado se declara en la clave L, subclave 01, del 190 (DR 190, pág. 23); el exceso es
//! rendimiento del trabajo sujeto a retención y va en la clave del perceptor (A o E). TaxPain
//! no lo calcula aquí: sólo contabiliza la parte exenta (629 «Otros servicios»: «los gastos de
//! viaje del personal de la empresa, incluidos los de transporte», PGC) y avisa de que el resto
//! ha de ir en la nómina, con su retención, para que llegue al 111 y al 190.
//!
//! Administradores sin relación laboral: el régimen del art. 9 sólo se aplica a quien cobra
//! rendimientos del trabajo por una relación laboral o estatutaria (DGT V1796-25, V1070-25;
//! TEAC 30-1-2025, RG 1475/2024). Sus dietas y su kilometraje a tanto alzado son retribución;
//! los gastos que paga por cuenta de la sociedad y justifica (hotel, billetes, peajes,
//! aparcamiento) no son renta y no se declaran.

use serde::{Deserialize, Serialize};
use taxpain_core::Nif;
use taxpain_core::accounting::account::AccountCode;
use taxpain_core::accounting::entry::{EntryLine, JournalEntry, Origin};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, Row, params};
use time::macros::date;
use time::{Date, OffsetDateTime};

use crate::accounting;
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

/// Cuenta de gasto de los viajes (PGC: 629 «Otros servicios»).
pub const EXPENSE_ACCOUNT: &str = "629";

fn other(e: impl ToString) -> AppError {
    AppError::Other(e.to_string())
}

fn parse_date(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]")).map_err(other)
}

/// Céntimos exceptuados por kilómetro en la fecha del viaje (art. 9.A.2.b RIRPF).
pub fn km_rate_cents(d: Date) -> i64 {
    if d >= date!(2023 - 07 - 17) { 26 } else { 19 }
}

/// Dieta de manutención exceptuada por día (art. 9.A.3 RIRPF).
pub fn meal_limit(abroad: bool, overnight: bool) -> Money {
    Money::from_cents(match (abroad, overnight) {
        (false, false) => 2_667,
        (false, true) => 5_334,
        (true, false) => 4_808,
        (true, true) => 9_135,
    })
}

/// Un viaje y lo que se ha pagado por él.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trip {
    pub date: Date,
    /// Ficha de Nóminas del perceptor, si la tiene.
    #[serde(default)]
    pub employee_id: Option<i64>,
    pub nif: String,
    pub name: String,
    /// Relación laboral (contrato de trabajo) con la sociedad: sin ella no se aplica el art. 9.
    pub labor: bool,
    pub origin: String,
    pub destination: String,
    pub reason: String,
    pub abroad: bool,
    pub overnight: bool,
    /// Días de dieta de manutención.
    pub days: u16,
    pub km: u32,
    /// Pagado por kilometraje.
    pub km_amount: Money,
    /// Pagado por manutención (dieta).
    pub meals: Money,
    /// Alojamiento justificado.
    pub lodging: Money,
    /// Peajes y aparcamiento justificados.
    pub tolls_parking: Money,
    /// Transporte público justificado (billetes).
    pub transport: Money,
    /// Cuenta a la que se le debe o con la que se paga: 551 (administrador), 465 (empleado) o
    /// 57x (pagado ya).
    pub counter_account: String,
}

/// Reparto de lo pagado.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TripCalc {
    pub total: Money,
    /// Exceptuado por el art. 9 RIRPF (clave L.01 del 190).
    pub exempt: Money,
    /// Gastos justificados pagados por cuenta de la sociedad que no son renta (administrador
    /// sin relación laboral): no se declaran.
    pub not_income: Money,
    /// Retribución sujeta a retención (exceso sobre los límites, o dietas y kilometraje de un
    /// administrador sin relación laboral): ha de ir en la nómina.
    pub taxable: Money,
    pub km_limit: Money,
    pub meals_limit: Money,
    /// Lo que se contabiliza en la 629 (exento + no renta).
    pub posted: Money,
    pub warnings: Vec<String>,
}

/// Calcula la parte exenta, la que no es renta y la que es retribución.
pub fn calculate(t: &Trip) -> TripCalc {
    let justified = t.lodging + t.tolls_parking + t.transport;
    let total = t.km_amount + t.meals + justified;
    let km_limit = Money::from_cents(i64::from(t.km) * km_rate_cents(t.date));
    let meals_limit =
        Money::from_cents(meal_limit(t.abroad, t.overnight).cents() * i64::from(t.days));
    let mut warnings = Vec::new();
    let (exempt, not_income, taxable) = if t.labor {
        let km_exempt = t.km_amount.min(km_limit);
        let meals_exempt = t.meals.min(meals_limit);
        (
            km_exempt + meals_exempt + justified,
            Money::ZERO,
            (t.km_amount - km_exempt) + (t.meals - meals_exempt),
        )
    } else {
        (Money::ZERO, justified, t.km_amount + t.meals)
    };
    if t.labor && !taxable.is_zero() {
        warnings.push(format!(
            "{taxable} pasan de los límites del art. 9 RIRPF (0,{:02} €/km y {} al día de \
             manutención): son retribución sujeta a retención. No se contabilizan aquí: \
             inclúyelos en la nómina de {} para que lleven su retención y vayan al 111 y al 190.",
            km_rate_cents(t.date),
            meal_limit(t.abroad, t.overnight),
            t.name
        ));
    }
    if !t.labor && !taxable.is_zero() {
        warnings.push(format!(
            "Sin relación laboral no se aplican las dietas exentas del art. 9 RIRPF (DGT \
             V1796-25; TEAC 30-1-2025): los {taxable} de dietas y kilometraje son retribución de \
             {} sujeta a retención (35 % o 19 % si es administrador). No se contabilizan aquí: \
             inclúyelos en su retribución (Nóminas). Los gastos justificados que pagó por cuenta \
             de la sociedad (hotel, billetes, peajes, aparcamiento) no son renta.",
            t.name
        ));
    }
    if !t.lodging.is_zero() && !t.overnight {
        warnings.push("Hay alojamiento pero no se marcó que pernoctara.".into());
    }
    if !t.meals.is_zero() && t.days == 0 {
        warnings.push("Hay dieta de manutención sin días: pon cuántos.".into());
    }
    TripCalc {
        total,
        exempt,
        not_income,
        taxable,
        km_limit,
        meals_limit,
        posted: exempt + not_income,
        warnings,
    }
}

fn validate(v: &Vault, t: &Trip) -> Result<String> {
    let nif = Nif::parse(t.nif.trim())
        .map_err(|e| other(format!("NIF del perceptor: {e}")))?
        .to_string();
    if t.name.trim().is_empty() {
        return Err(other("pon el nombre de quien viaja"));
    }
    if t.destination.trim().is_empty() || t.reason.trim().is_empty() {
        return Err(other(
            "pon el destino y el motivo: hay que poder acreditar el día, el lugar y la razón del \
             desplazamiento (art. 9.A.3 RIRPF)",
        ));
    }
    let money = [
        t.km_amount,
        t.meals,
        t.lodging,
        t.tolls_parking,
        t.transport,
    ];
    if money.iter().any(|m| m.is_negative()) {
        return Err(other("los importes no pueden ser negativos"));
    }
    if money.iter().all(|m| m.is_zero()) {
        return Err(other("no hay ningún importe"));
    }
    if !t.km_amount.is_zero() && t.km == 0 {
        return Err(other("pon los kilómetros recorridos"));
    }
    let c = t.counter_account.trim();
    if !(c.starts_with("551") || c.starts_with("465") || c.starts_with("57")) {
        return Err(other(
            "la contrapartida es la 551 (administrador), la 465 (empleado) o una cuenta de \
             tesorería (57x)",
        ));
    }
    AccountCode::new(c).map_err(other)?;
    if let Some(id) = t.employee_id {
        crate::payroll::employee(v.connection(), id)?;
    }
    Ok(nif)
}

/// Más de nueve meses entre `start` y `end` (por meses del calendario).
fn over_nine_months(start: Date, end: Date) -> bool {
    let months = (end.year() - start.year()) * 12 + i32::from(u8::from(end.month()))
        - i32::from(u8::from(start.month()));
    months > 9 || (months == 9 && end.day() > start.day())
}

/// Inicio de la estancia continuada en el destino de `t` de quien viaja (`nif`, ya validado)
/// si con este viaje pasa de nueve meses: sus viajes con pernoctación al mismo destino que se
/// suceden con menos de una semana entre uno y otro (fines de semana, permisos) cuentan como
/// una sola estancia.
fn long_stay(v: &Vault, t: &Trip, nif: &str) -> Result<Option<Date>> {
    if !t.overnight || !t.labor {
        return Ok(None);
    }
    let dest = t.destination.trim().to_lowercase();
    let end_of = |d: Date, days: u16| d + time::Duration::days(i64::from(days.max(1)) - 1);
    let mut stays: Vec<(Date, Date)> = trips_between(v, Date::MIN, t.date)?
        .into_iter()
        .filter(|s| {
            s.trip.overnight
                && s.trip.nif == nif
                && s.trip.destination.trim().to_lowercase() == dest
        })
        .map(|s| (s.trip.date, end_of(s.trip.date, s.trip.days)))
        .collect();
    let this = (t.date, end_of(t.date, t.days));
    stays.push(this);
    stays.sort();
    // Hacia atrás desde este viaje mientras no haya un hueco de más de una semana.
    let mut start = this.0;
    let mut reach = this.0;
    for &(from, to) in stays.iter().rev() {
        if from > this.0 {
            continue;
        }
        if to + time::Duration::days(7) < reach {
            break;
        }
        start = start.min(from);
        reach = reach.min(from);
    }
    Ok(over_nine_months(start, this.1).then_some(start))
}

/// Cálculo del viaje antes de registrarlo (con los avisos y las comprobaciones).
pub fn preview(v: &Vault, t: &Trip) -> Result<TripCalc> {
    let nif = validate(v, t)?;
    let mut c = calculate(t);
    if let Some(since) = long_stay(v, t, &nif)? {
        c.warnings.push(format!(
            "{} lleva desde el {since} desplazado en {}: más de nueve meses seguidos en el mismo \
             municipio. Si es así, la manutención y la estancia no están exentas (art. 9.A.3.b \
             RIRPF; no cuentan las vacaciones o la enfermedad que no cambian el destino): \
             son retribución y van en la nómina con su retención.",
            t.name.trim(),
            t.destination.trim()
        ));
    }
    if let Some(id) = t.employee_id {
        let e = crate::payroll::employee(v.connection(), id)?;
        if e.administrator && t.labor {
            c.warnings.push(format!(
                "{} es administrador: sus dietas sólo son exentas si además tiene una relación \
                 laboral real con la sociedad (TEAC 30-1-2025, RG 1475/2024).",
                e.name
            ));
        }
    }
    Ok(c)
}

/// Viaje registrado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StoredTrip {
    pub id: i64,
    pub trip: Trip,
    pub exempt: Money,
    pub taxable: Money,
    pub entry_id: Option<i64>,
}

/// Registra el viaje y contabiliza la parte que no es retribución (629 contra la contrapartida).
pub fn register(v: &Vault, t: &Trip, now: OffsetDateTime) -> Result<i64> {
    let nif = validate(v, t)?;
    let c = calculate(t);
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO travel_expense(date, nif, name, employee_id, origin, destination, reason,
           labor, abroad, overnight, days, km, km_amount, meals, lodging, tolls_parking, transport,
           exempt, taxable, counter_account, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
           ?19, ?20, ?21)",
        params![
            t.date.to_string(),
            nif,
            t.name.trim(),
            t.employee_id,
            t.origin.trim(),
            t.destination.trim(),
            t.reason.trim(),
            t.labor,
            t.abroad,
            t.overnight,
            t.days,
            t.km,
            t.km_amount.cents(),
            t.meals.cents(),
            t.lodging.cents(),
            t.tolls_parking.cents(),
            t.transport.cents(),
            c.exempt.cents(),
            c.taxable.cents(),
            t.counter_account.trim(),
            now_str(now)
        ],
    )?;
    let id = tx.last_insert_rowid();
    let entry_id = if c.posted.is_zero() {
        None
    } else {
        let mut debit =
            EntryLine::debit(AccountCode::new(EXPENSE_ACCOUNT).map_err(other)?, c.posted);
        debit.description = Some(format!("{} → {}", t.origin.trim(), t.destination.trim()));
        let e = JournalEntry {
            date: t.date,
            concept: format!("Gastos de viaje {}: {}", t.name.trim(), t.reason.trim()),
            origin: Origin::GastosViaje(id),
            lines: vec![
                debit,
                EntryLine::credit(
                    AccountCode::new(t.counter_account.trim()).map_err(other)?,
                    c.posted,
                ),
            ],
        };
        Some(accounting::record_entry(&tx, &e, now)?)
    };
    tx.execute(
        "UPDATE travel_expense SET entry_id = ?1 WHERE id = ?2",
        params![entry_id, id],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "travel.register",
        &id.to_string(),
        &serde_json::json!({
            "entry": entry_id,
            "exempt": c.exempt.cents(),
            "taxable": c.taxable.cents(),
        }),
    )?;
    tx.commit()?;
    Ok(id)
}

fn trip_of(
    r: &Row<'_>,
) -> taxpain_vault::rusqlite::Result<(i64, String, Trip, i64, i64, Option<i64>)> {
    let m =
        |i: usize| -> taxpain_vault::rusqlite::Result<Money> { Ok(Money::from_cents(r.get(i)?)) };
    Ok((
        r.get(0)?,
        r.get(1)?,
        Trip {
            date: Date::MIN,
            nif: r.get(2)?,
            name: r.get(3)?,
            employee_id: r.get(4)?,
            origin: r.get(5)?,
            destination: r.get(6)?,
            reason: r.get(7)?,
            labor: r.get(8)?,
            abroad: r.get(9)?,
            overnight: r.get(10)?,
            days: r.get(11)?,
            km: r.get(12)?,
            km_amount: m(13)?,
            meals: m(14)?,
            lodging: m(15)?,
            tolls_parking: m(16)?,
            transport: m(17)?,
            counter_account: r.get(20)?,
        },
        r.get(18)?,
        r.get(19)?,
        r.get(21)?,
    ))
}

/// Viajes con fecha entre `start` y `end`, por fecha.
pub fn trips_between(v: &Vault, start: Date, end: Date) -> Result<Vec<StoredTrip>> {
    let mut st = v.connection().prepare(
        "SELECT id, date, nif, name, employee_id, origin, destination, reason, labor, abroad,
                overnight, days, km, km_amount, meals, lodging, tolls_parking, transport, exempt,
                taxable, counter_account, entry_id
         FROM travel_expense WHERE date BETWEEN ?1 AND ?2 ORDER BY date, id",
    )?;
    let rows = st.query_map(params![start.to_string(), end.to_string()], trip_of)?;
    let mut out = Vec::new();
    for row in rows {
        let (id, date, mut trip, exempt, taxable, entry_id) = row?;
        trip.date = parse_date(&date)?;
        out.push(StoredTrip {
            id,
            trip,
            exempt: Money::from_cents(exempt),
            taxable: Money::from_cents(taxable),
            entry_id,
        });
    }
    Ok(out)
}

/// Viajes del año natural `year`.
pub fn trips_of_year(v: &Vault, year: i32) -> Result<Vec<StoredTrip>> {
    let start = Date::from_calendar_date(year, time::Month::January, 1).map_err(other)?;
    let end = Date::from_calendar_date(year, time::Month::December, 31).map_err(other)?;
    trips_between(v, start, end)
}

/// Borra un viaje y su asiento (si el ejercicio sigue abierto), con confirmación.
pub fn delete(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let entry: Option<Option<i64>> = conn
        .query_row(
            "SELECT entry_id FROM travel_expense WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    let entry = entry.ok_or(AppError::NotFound)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM travel_expense WHERE id = ?1", [id])?;
    if let Some(e) = entry {
        tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [e])
            .map_err(|_| other("el ejercicio de ese viaje está cerrado: reábrelo"))?;
        tx.execute("DELETE FROM journal_entry WHERE id = ?1", [e])?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "travel.delete",
        &id.to_string(),
        &serde_json::json!({ "entry": entry }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Totales de un perceptor en un periodo: lo exceptuado (clave L.01) y lo que es retribución.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PersonTotals {
    pub nif: String,
    pub name: String,
    pub employee_id: Option<i64>,
    pub exempt: Money,
    pub taxable: Money,
}

/// Totales por NIF de los viajes entre `start` y `end`.
pub fn totals_by_person(v: &Vault, start: Date, end: Date) -> Result<Vec<PersonTotals>> {
    let mut acc: std::collections::BTreeMap<String, PersonTotals> = Default::default();
    for t in trips_between(v, start, end)? {
        let p = acc
            .entry(t.trip.nif.clone())
            .or_insert_with(|| PersonTotals {
                nif: t.trip.nif.clone(),
                name: t.trip.name.clone(),
                employee_id: t.trip.employee_id,
                exempt: Money::ZERO,
                taxable: Money::ZERO,
            });
        p.exempt += t.exempt;
        p.taxable += t.taxable;
        if p.employee_id.is_none() {
            p.employee_id = t.trip.employee_id;
        }
    }
    Ok(acc.into_values().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trip() -> Trip {
        Trip {
            date: date!(2026 - 03 - 10),
            nif: "12345678Z".into(),
            name: "Viajera Uno".into(),
            labor: true,
            origin: "València".into(),
            destination: "Madrid".into(),
            reason: "Reunión con cliente".into(),
            overnight: true,
            days: 2,
            km: 700,
            km_amount: Money::from_euros(210, 0),
            meals: Money::from_euros(120, 0),
            lodging: Money::from_euros(95, 0),
            tolls_parking: Money::from_euros(18, 50),
            counter_account: "465".into(),
            employee_id: None,
            abroad: false,
            transport: Money::ZERO,
        }
    }

    #[test]
    fn empleado_dentro_y_fuera_de_los_limites() {
        let c = calculate(&trip());
        // 700 km × 0,26 = 182 €; manutención 2 × 53,34 = 106,68 €.
        assert_eq!(c.km_limit, Money::from_euros(182, 0));
        assert_eq!(c.meals_limit, Money::from_cents(10_668));
        assert_eq!(c.total, Money::from_cents(44_350));
        assert_eq!(c.taxable, Money::from_cents(2_800 + 1_332));
        assert_eq!(c.exempt, Money::from_cents(18_200 + 10_668 + 9_500 + 1_850));
        assert_eq!(c.posted, c.exempt);
        assert_eq!(c.exempt + c.taxable, c.total);
        assert_eq!(c.warnings.len(), 1, "{:?}", c.warnings);
    }

    #[test]
    fn administrador_sin_relacion_laboral() {
        let mut t = trip();
        t.labor = false;
        let c = calculate(&t);
        assert_eq!(c.exempt, Money::ZERO);
        assert_eq!(c.not_income, Money::from_cents(9_500 + 1_850));
        assert_eq!(c.taxable, Money::from_euros(330, 0));
        assert_eq!(c.posted, c.not_income);
    }

    #[test]
    fn tarifas_por_fecha() {
        assert_eq!(km_rate_cents(date!(2023 - 07 - 16)), 19);
        assert_eq!(km_rate_cents(date!(2023 - 07 - 17)), 26);
        assert_eq!(meal_limit(true, true), Money::from_cents(9_135));
        assert_eq!(meal_limit(true, false), Money::from_cents(4_808));
        assert_eq!(meal_limit(false, false), Money::from_cents(2_667));
    }
}
