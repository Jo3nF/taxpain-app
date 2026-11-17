//! Nóminas: fichas de empleado y recibos de salarios contabilizados. Los importes salen del
//! recibo que prepara la gestoría laboral (TaxPain no calcula bases ni tipos de cotización): el
//! bruto, la retención de IRPF y las cotizaciones del trabajador y de la empresa. Cada recibo
//! genera su asiento (640 y 642 al debe; 4751, 476 y 465 al haber) y alimenta el 111 y la clave
//! A del 190 con los datos de la ficha.
//!
//! Consejeros y administradores (ficha con `administrator`): su retribución va al 111
//! (rendimientos del trabajo) y a la clave E del 190, con la subclave que toca al pagarla
//! (`taxpain_models::informative::director_subkey`: régimen de la Seguridad Social y cifra de
//! negocios de la entidad). La retención es fija (art. 101.2 LIRPF y art. 80.1.3.º RIRPF): 35 %,
//! o 19 % si el importe neto de la cifra de negocios del último periodo impositivo finalizado
//! antes del pago fue inferior a 100.000 € (elevado al año si duró menos; en el primer periodo
//! de una sociedad nueva, 19 % según la DGT, V1727-15: `TODO(VERIFY)` con el texto de la
//! consulta). Sólo la retribución del cargo (incluidas las funciones de dirección y gerencia,
//! que el Tribunal Supremo considera subsumidas en él) va a la clave E; un trabajo distinto
//! (p. ej. diseño) es clave A con el procedimiento general o actividad profesional (clave G).

use serde::{Deserialize, Serialize};
use taxpain_core::Nif;
use taxpain_core::accounting::payroll::Payslip;
use taxpain_core::money::Money;
use taxpain_models::informative::{DirectorSs, director_rate, director_subkey};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, Row, params};
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};

use crate::accounting;
use crate::error::{AppError, Result};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Employee {
    #[serde(default)]
    pub id: i64,
    pub nif: String,
    /// Apellidos y nombre (como en el 190: primer apellido, segundo y nombre).
    pub name: String,
    /// Provincia del domicilio (código INE de 2 cifras; 98 si reside en el extranjero).
    pub province: String,
    pub birth_year: u16,
    /// Situación familiar del art. 88 RIRPF: 1, 2 o 3.
    pub family: u8,
    pub spouse_nif: Option<String>,
    /// Discapacidad: 0 sin, 1 entre 33 y 65 %, 2 con movilidad reducida, 3 igual o más del 65 %.
    pub disability: u8,
    /// 1 general, 2 inferior al año, 3 relación especial, 4 jornaleros (190, pos. 168).
    pub contract: u8,
    pub mobility: bool,
    pub children_under3: u8,
    pub children_rest: u8,
    pub children_whole: bool,
    #[serde(default = "yes")]
    pub active: bool,
    /// Consejero o administrador (clave E del 190; retención fija del 35 % o del 19 %).
    #[serde(default)]
    pub administrator: bool,
    /// Régimen de Seguridad Social del administrador (decide la subclave E).
    #[serde(default)]
    pub ss_regime: DirectorSs,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PayslipInput {
    pub employee_id: i64,
    pub date: Date,
    pub gross: Money,
    pub irpf: Money,
    pub ss_employee: Money,
    pub ss_company: Money,
    /// Sólo administradores: la entidad tuvo una cifra de negocios inferior a 100.000 € en el
    /// último periodo impositivo finalizado antes del pago. Si es `None`, sale del 200 o de la
    /// contabilidad de ese periodo ([`director_turnover`]).
    #[serde(default)]
    pub small_entity: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct StoredPayslip {
    pub id: i64,
    pub employee_id: i64,
    pub employee: String,
    pub nif: String,
    pub date: Date,
    pub gross: Money,
    pub irpf: Money,
    pub ss_employee: Money,
    pub ss_company: Money,
    /// Líquido a percibir.
    pub net: Money,
    pub entry_id: Option<i64>,
    /// Subclave de la clave E del 190 si es la retribución de un administrador.
    pub director_subkey: Option<u8>,
}

/// Cifra de negocios que decide el tipo de retención de los administradores.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DirectorTurnover {
    /// Último periodo impositivo finalizado antes del pago.
    pub year: i32,
    /// Importe neto de la cifra de negocios de ese periodo (elevado al año si duró menos),
    /// o `None` si no se sabe (ni 200 ni ventas en la contabilidad de TaxPain). Con
    /// `small: None`, la que consta en TaxPain de un periodo que no cubre entero.
    pub turnover: Option<Money>,
    /// Inferior a 100.000 € (`None`: hay que indicarlo a mano).
    pub small: Option<bool>,
    /// De dónde sale la cifra.
    pub source: TurnoverSource,
}

/// De dónde sale la cifra de negocios del último periodo cerrado.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TurnoverSource {
    /// Casilla [00255] del 200 presentado en la AEAT («Comprobar lo presentado»).
    Aeat,
    /// Casilla [00255] de un 200 guardado en TaxPain (Importar → Comparar).
    Guardado,
    /// Línea 1 de la cuenta de pérdidas y ganancias de la contabilidad de TaxPain.
    Contabilidad,
}

impl TurnoverSource {
    /// Para los avisos: «según …».
    pub fn label(self) -> &'static str {
        match self {
            TurnoverSource::Aeat => "el 200 presentado en la AEAT, casilla [00255]",
            TurnoverSource::Guardado => "el 200 guardado en TaxPain, casilla [00255]",
            TurnoverSource::Contabilidad => "la contabilidad de TaxPain",
        }
    }
}

/// Umbral de la retención reducida de los administradores (art. 101.2 LIRPF).
pub const DIRECTOR_THRESHOLD: Money = Money::from_euros(100_000, 0);

fn other(e: impl ToString) -> AppError {
    AppError::Other(e.to_string())
}

fn now_str(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

fn parse_date(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]")).map_err(other)
}

fn validate(e: &Employee) -> Result<String> {
    let nif = Nif::parse(e.nif.trim())
        .map_err(|err| other(format!("NIF del empleado: {err}")))?
        .to_string();
    if e.name.trim().is_empty() {
        return Err(other("falta el nombre del empleado"));
    }
    if !(e.province.is_empty()
        || (e.province.len() == 2 && e.province.chars().all(|c| c.is_ascii_digit())))
    {
        return Err(other(
            "la provincia es el código de 2 cifras (46 Valencia, 28 Madrid…)",
        ));
    }
    if !(1..=3).contains(&e.family) || !(1..=4).contains(&e.contract) || e.disability > 3 {
        return Err(other(
            "situación familiar, contrato o discapacidad no válidos",
        ));
    }
    if let Some(s) = e.spouse_nif.as_deref().filter(|s| !s.trim().is_empty()) {
        Nif::parse(s.trim()).map_err(|err| other(format!("NIF del cónyuge: {err}")))?;
    }
    Ok(nif)
}

fn regime_str(r: DirectorSs) -> &'static str {
    match r {
        DirectorSs::Asimilado => "Asimilado",
        DirectorSs::Autonomo => "Autonomo",
        DirectorSs::Ninguno => "Ninguno",
    }
}

fn regime_of(s: &str) -> DirectorSs {
    match s {
        "Asimilado" => DirectorSs::Asimilado,
        "Autonomo" => DirectorSs::Autonomo,
        _ => DirectorSs::Ninguno,
    }
}

/// Cifra de negocios del último periodo impositivo finalizado antes de `date` (la que decide
/// el 35 % o el 19 % de los administradores, y el tipo del IS, el 202 y el IAE), elevada al año
/// si el periodo duró menos (art. 101.2 LIRPF). Por orden:
///
/// 1. La casilla [00255] del 200 de ese ejercicio presentado en la AEAT (el vigente, con las
///    casillas leídas en «Comprobar lo presentado»).
/// 2. La [00255] de un 200 guardado en TaxPain (Importar → Comparar), si la tiene.
/// 3. La contabilidad de TaxPain: la línea 1 de la cuenta de pérdidas y ganancias. Sin ningún
///    movimiento de ventas (70) en ese ejercicio, no se sabe (lo normal es que falte por
///    contabilizar, no que la cifra sea cero).
///
/// El 200 sólo se usa con ejercicio natural: `TODO(VERIFY)` con ejercicio partido (el número
/// del ejercicio del 200 frente al de TaxPain) y con periodos cortos que TaxPain no conoce (las
/// fechas del periodo van en la cabecera del 200 y no se guardan con las casillas).
///
/// En la contabilidad:
/// - No cuentan la regularización ni el cierre, tampoco los importados como asientos normales
///   (un asiento que lleva la 129 y sólo cuentas de los grupos 6 y 7).
/// - El periodo empieza con la constitución de la sociedad si cae dentro del ejercicio: la de la
///   escritura (Documentos) o, en el primer ejercicio con asientos en TaxPain y sin apertura, la
///   del asiento de aportación del capital (100). Vale también para elevar la del 200.
/// - Si no, en el primer ejercicio con asientos en TaxPain cuyo primer asiento es de más de un
///   mes después del inicio (sociedad que empezó a mitad de año sin asiento de capital, o
///   TaxPain empezado a usar a mitad de año), la cifra no cubre todo el periodo: se da la que
///   consta (sin elevar) y lo decide el usuario (`small: None`).
pub fn director_turnover(v: &Vault, date: Date) -> Result<DirectorTurnover> {
    use taxpain_core::accounting::statements::{ModelKind, income_statement_for};
    use taxpain_core::accounting::{JournalEntry, Origin, trial_balance};
    let current = accounting::fiscal_year_at(v, date)?;
    let year = current.year - 1;
    let prev = accounting::fiscal_year(v, year)?;
    let is_regularization = |e: &JournalEntry| {
        e.lines
            .iter()
            .any(|l| l.account.as_str().starts_with("129"))
            && e.lines.iter().all(|l| {
                let a = l.account.as_str();
                a.starts_with("129") || a.starts_with('6') || a.starts_with('7')
            })
    };
    let entries: Vec<JournalEntry> = accounting::journal(v, year)?
        .into_iter()
        .map(|s| s.entry)
        .filter(|e| {
            !matches!(e.origin, Origin::Regularizacion | Origin::Cierre) && !is_regularization(e)
        })
        .collect();
    let first = entries
        .iter()
        .filter(|e| e.origin != Origin::Apertura)
        .map(|e| e.date)
        .min();
    // Inicio del periodo y si la contabilidad lo cubre entero.
    let in_period = |d: &Date| *d > prev.start && *d <= prev.end;
    let mut start = prev.start;
    let mut complete = true;
    let constitution = crate::documentos::list(v)?
        .into_iter()
        .filter_map(|d| d.facts.constitution_date)
        .find(in_period);
    if let Some(c) = constitution {
        start = c;
    } else if let Some(first) = first {
        let earliest: Option<String> =
            v.connection()
                .query_row("SELECT MIN(date) FROM journal_entry", [], |r| r.get(0))?;
        if earliest.is_none_or(|d| d >= prev.start.to_string()) {
            let opening = entries.iter().any(|e| e.origin == Origin::Apertura);
            let capital = entries
                .iter()
                .filter(|e| {
                    e.lines
                        .iter()
                        .any(|l| l.account.as_str().starts_with("100") && !l.credit.is_zero())
                })
                .map(|e| e.date)
                .min()
                .filter(|_| !opening)
                .filter(in_period);
            match capital {
                Some(c) => start = c.min(first),
                None => complete = (first - prev.start).whole_days() <= 31,
            }
        }
    }
    let annual_from = |start: Date, t: Money| {
        let days = (prev.end - start).whole_days() + 1;
        if (1..365).contains(&days) {
            Money::from_cents(
                i64::try_from(i128::from(t.cents()) * 365 / i128::from(days)).unwrap_or(i64::MAX),
            )
        } else {
            t
        }
    };
    if prev.is_natural()
        && let Some((t, source)) = filed_200_turnover(v, year)?
    {
        // El 200 es de un año entero salvo que la escritura diga que la empresa empezó ese año:
        // con una contabilidad a medias, el primer asiento no dice cuándo empezó.
        let turnover = annual_from(constitution.unwrap_or(prev.start), t);
        return Ok(DirectorTurnover {
            year,
            turnover: Some(turnover),
            small: Some(turnover < DIRECTOR_THRESHOLD),
            source,
        });
    }
    let sales = entries
        .iter()
        .flat_map(|e| e.lines.iter())
        .any(|l| l.account.as_str().starts_with("70"));
    if first.is_none() || !sales {
        return Ok(DirectorTurnover {
            year,
            turnover: None,
            small: None,
            source: TurnoverSource::Contabilidad,
        });
    }
    let turnover = income_statement_for(ModelKind::Pymes, &trial_balance(&entries)).get("1");
    if !complete {
        return Ok(DirectorTurnover {
            year,
            turnover: Some(turnover),
            small: None,
            source: TurnoverSource::Contabilidad,
        });
    }
    let turnover = annual_from(start, turnover);
    Ok(DirectorTurnover {
        year,
        turnover: Some(turnover),
        small: Some(turnover < DIRECTOR_THRESHOLD),
        source: TurnoverSource::Contabilidad,
    })
}

/// Casilla [00255] (importe neto de la cifra de negocios) del 200 de `year`: la del presentado
/// en la AEAT (el vigente, con las casillas leídas) o, si no, la del guardado en TaxPain que vale
/// (no sustituido por una complementaria). Sólo si la casilla está: las casillas a cero no se
/// guardan, y sin ella no se distingue un cero de una página que no se leyó.
fn filed_200_turnover(v: &Vault, year: i32) -> Result<Option<(Money, TurnoverSource)>> {
    const CASILLA: u32 = 255;
    let aeat = crate::presentado_aeat::vigentes(v)?
        .into_iter()
        .filter(|x| x.model == "200" && x.year == year)
        .filter_map(|x| x.boxes)
        .rev()
        .find_map(|b| b.casillas.get(&CASILLA).copied());
    if let Some(m) = aeat {
        return Ok(Some((m, TurnoverSource::Aeat)));
    }
    Ok(crate::presentados::filed_by_gestor(v, "200", year, "0A")?
        .and_then(|(_, boxes)| boxes.get(&255).copied())
        .map(|m| (m, TurnoverSource::Guardado)))
}

/// Alta o cambio de un empleado. Devuelve su id.
pub fn save_employee(v: &Vault, e: &Employee, now: OffsetDateTime) -> Result<i64> {
    // La clave E no lleva los datos personales de la A (situación familiar, contrato…): en la
    // ficha de un administrador se guardan los valores neutros si faltan.
    let normalized;
    let e = if e.administrator {
        normalized = Employee {
            family: if (1..=3).contains(&e.family) {
                e.family
            } else {
                3
            },
            contract: if (1..=4).contains(&e.contract) {
                e.contract
            } else {
                1
            },
            ..e.clone()
        };
        &normalized
    } else {
        e
    };
    let nif = validate(e)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let spouse = e
        .spouse_nif
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_uppercase);
    let p = params![
        nif,
        e.name.trim(),
        e.province,
        e.birth_year,
        e.family,
        spouse,
        e.disability,
        e.contract,
        e.mobility,
        e.children_under3,
        e.children_rest,
        e.children_whole,
        e.active,
        now_str(now),
        e.administrator,
        regime_str(e.ss_regime)
    ];
    let id = if e.id > 0 {
        let n = tx.execute(
            "UPDATE employee SET nif = ?1, name = ?2, province = ?3, birth_year = ?4, family = ?5,
               spouse_nif = ?6, disability = ?7, contract = ?8, mobility = ?9,
               children_under3 = ?10, children_rest = ?11, children_whole = ?12, active = ?13,
               updated_at = ?14, administrator = ?16, ss_regime = ?17 WHERE id = ?15",
            params![
                nif,
                e.name.trim(),
                e.province,
                e.birth_year,
                e.family,
                spouse,
                e.disability,
                e.contract,
                e.mobility,
                e.children_under3,
                e.children_rest,
                e.children_whole,
                e.active,
                now_str(now),
                e.id,
                e.administrator,
                regime_str(e.ss_regime)
            ],
        )?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        e.id
    } else {
        tx.execute(
            "INSERT INTO employee(nif, name, province, birth_year, family, spouse_nif, disability,
               contract, mobility, children_under3, children_rest, children_whole, active, updated_at,
               administrator, ss_regime)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            p,
        )
        .map_err(|err| match err {
            taxpain_vault::rusqlite::Error::SqliteFailure(f, _)
                if f.code == taxpain_vault::rusqlite::ErrorCode::ConstraintViolation =>
            {
                other("ya hay un empleado con ese NIF")
            }
            err => err.into(),
        })?;
        tx.last_insert_rowid()
    };
    // Los datos personales del empleado no van a la auditoría, sólo el cambio.
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "employee.save",
        &id.to_string(),
        &serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(id)
}

fn employee_of(r: &Row<'_>) -> taxpain_vault::rusqlite::Result<Employee> {
    Ok(Employee {
        id: r.get(0)?,
        nif: r.get(1)?,
        name: r.get(2)?,
        province: r.get(3)?,
        birth_year: r.get(4)?,
        family: r.get(5)?,
        spouse_nif: r.get(6)?,
        disability: r.get(7)?,
        contract: r.get(8)?,
        mobility: r.get(9)?,
        children_under3: r.get(10)?,
        children_rest: r.get(11)?,
        children_whole: r.get(12)?,
        active: r.get(13)?,
        administrator: r.get(14)?,
        ss_regime: regime_of(&r.get::<_, String>(15)?),
    })
}

const EMPLOYEE_COLS: &str = "id, nif, name, province, birth_year, family, spouse_nif, disability, \
     contract, mobility, children_under3, children_rest, children_whole, active, administrator, \
     ss_regime";

pub fn employees(v: &Vault) -> Result<Vec<Employee>> {
    let mut st = v.connection().prepare(&format!(
        "SELECT {EMPLOYEE_COLS} FROM employee ORDER BY active DESC, name"
    ))?;
    let rows = st.query_map([], employee_of)?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub(crate) fn employee(conn: &Connection, id: i64) -> Result<Employee> {
    conn.query_row(
        &format!("SELECT {EMPLOYEE_COLS} FROM employee WHERE id = ?1"),
        [id],
        employee_of,
    )
    .optional()?
    .ok_or(AppError::NotFound)
}

/// Subclave E de la retribución de un administrador pagada el día `date` (o `None` si es un
/// empleado). Sin la cifra de negocios (del 200 o de la contabilidad), hay que indicarla
/// (`small_entity`).
fn director_subkey_for(v: &Vault, e: &Employee, p: &PayslipInput) -> Result<Option<u8>> {
    if !e.administrator {
        return Ok(None);
    }
    if e.ss_regime != DirectorSs::Asimilado && !(p.ss_employee.is_zero() && p.ss_company.is_zero())
    {
        return Err(other(
            "un administrador en el régimen de autónomos (o sin alta) no lleva cotizaciones en el \
             recibo: la cuota de autónomos la paga él (si la paga la sociedad, súmala al bruto)",
        ));
    }
    let small = match p.small_entity {
        Some(s) => s,
        None => director_turnover(v, p.date)?.small.ok_or_else(|| {
            other(
                "indica si la cifra de negocios del último ejercicio cerrado antes del pago fue \
                 inferior a 100.000 € (TaxPain no la sabe: ni el 200 ni una contabilidad completa \
                 con las ventas)",
            )
        })?,
    };
    Ok(Some(director_subkey(e.ss_regime, small)))
}

/// Retención que corresponde a la retribución bruta de un administrador: 19 % si la entidad
/// tuvo menos de 100.000 € de cifra de negocios en el último ejercicio cerrado; si no, 35 %.
pub fn director_expected_irpf(gross: Money, small_entity: bool) -> Money {
    gross.apply_rate(director_rate(small_entity))
}

/// Registra el recibo de salarios con su asiento. Devuelve su id.
pub fn register(v: &Vault, p: &PayslipInput, now: OffsetDateTime) -> Result<i64> {
    let tx = v.connection().unchecked_transaction()?;
    let id = register_in(v, &tx, p, now)?;
    tx.commit()?;
    Ok(id)
}

/// Como [`register`], dentro de la transacción `tx` del llamante (que la confirma).
pub(crate) fn register_in(
    v: &Vault,
    tx: &Connection,
    p: &PayslipInput,
    now: OffsetDateTime,
) -> Result<i64> {
    let conn = v.connection();
    let e = employee(conn, p.employee_id)?;
    let subkey = director_subkey_for(v, &e, p)?;
    let slip = Payslip {
        date: p.date,
        employee: format!(
            "{} {:02}/{}",
            e.name,
            u8::from(p.date.month()),
            p.date.year()
        ),
        gross: p.gross,
        irpf: p.irpf,
        ss_employee: p.ss_employee,
        ss_company: p.ss_company,
    };
    let entry = slip.entry().map_err(other)?;
    let entry_id = accounting::record_entry(tx, &entry, now).map_err(closed_year)?;
    tx.execute(
        "INSERT INTO payslip(employee_id, date, gross, irpf, ss_employee, ss_company, entry_id,
           created_at, director_subkey)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            e.id,
            p.date.to_string(),
            p.gross.cents(),
            p.irpf.cents(),
            p.ss_employee.cents(),
            p.ss_company.cents(),
            entry_id,
            now_str(now),
            subkey
        ],
    )?;
    let id = tx.last_insert_rowid();
    audit::append(
        tx,
        &now_str(now),
        Actor::User,
        "payslip.register",
        &id.to_string(),
        &serde_json::json!({ "entry": entry_id, "gross": p.gross.cents() }),
    )?;
    Ok(id)
}

/// Recibos con fecha entre `start` y `end`, por fecha.
pub fn payslips(conn: &Connection, start: Date, end: Date) -> Result<Vec<StoredPayslip>> {
    let mut st = conn.prepare(
        "SELECT p.id, p.employee_id, e.name, e.nif, p.date, p.gross, p.irpf, p.ss_employee,
                p.ss_company, p.entry_id, p.director_subkey
         FROM payslip p JOIN employee e ON e.id = p.employee_id
         WHERE p.date BETWEEN ?1 AND ?2 ORDER BY p.date, p.id",
    )?;
    let rows = st.query_map(params![start.to_string(), end.to_string()], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            [
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?,
            ],
            r.get::<_, Option<i64>>(9)?,
            r.get::<_, Option<u8>>(10)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, employee_id, employee, nif, date, [g, i, se, sc], entry_id, director_subkey) =
            row?;
        let gross = Money::from_cents(g);
        let irpf = Money::from_cents(i);
        let ss_employee = Money::from_cents(se);
        out.push(StoredPayslip {
            id,
            employee_id,
            employee,
            nif,
            date: parse_date(&date)?,
            gross,
            irpf,
            ss_employee,
            ss_company: Money::from_cents(sc),
            net: gross - irpf - ss_employee,
            entry_id,
            director_subkey,
        });
    }
    Ok(out)
}

/// Recibos del año natural `year`.
pub fn payslips_of_year(v: &Vault, year: i32) -> Result<Vec<StoredPayslip>> {
    let start = Date::from_calendar_date(year, time::Month::January, 1).map_err(other)?;
    let end = Date::from_calendar_date(year, time::Month::December, 31).map_err(other)?;
    payslips(v.connection(), start, end)
}

/// Los disparadores `journal_entry_closed_*` (migración 0005) impiden asentar o borrar en un
/// ejercicio cerrado: su error de base de datos se traduce al aviso de la app.
fn closed_year(e: AppError) -> AppError {
    if e.to_string().contains("cerrado") {
        other("el ejercicio de esa nómina está cerrado: reábrelo en Cierre")
    } else {
        e
    }
}

/// Borra un recibo y su asiento, con confirmación; en un ejercicio cerrado avisa de que hay
/// que reabrirlo.
pub fn delete(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let entry: Option<Option<i64>> = tx
        .query_row("SELECT entry_id FROM payslip WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    let entry = entry.ok_or(AppError::NotFound)?;
    tx.execute("DELETE FROM payslip WHERE id = ?1", [id])?;
    if let Some(e) = entry {
        tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [e])
            .map_err(|e| closed_year(e.into()))?;
        tx.execute("DELETE FROM journal_entry WHERE id = ?1", [e])?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "payslip.delete",
        &id.to_string(),
        &serde_json::json!({ "entry": entry }),
    )?;
    tx.commit()?;
    Ok(())
}
