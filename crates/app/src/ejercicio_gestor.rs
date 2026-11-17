//! Importar un ejercicio entero del gestor: la empresa que llevaba la contabilidad en una
//! gestoría (a3, Sage, ContaSOL…) carga en TaxPain un ejercicio ya cerrado, asiento a asiento,
//! para tener el histórico, comparar ejercicios, cruzarlo con los modelos presentados y que el
//! siguiente arranque con la apertura que TaxPain mismo genera al cerrarlo (Código de Comercio,
//! art. 28: diario con todas las operaciones y balance de apertura; PGC, Quinta parte).
//!
//! Lo que se lee ([`read`]): el **diario completo** en hoja de cálculo o CSV (una hoja por mes o
//! todo junto; el «Diario.» de a3 con sus cuatro filas de cabecera y la fila «Total en <mes>»),
//! el **diario resumido** de a3 (una línea por cuenta y mes, con cuenta del debe y del haber:
//! cada mes se agrupa en un asiento por fecha), el **SUENLACE** de a3 y un CSV genérico con las
//! mismas columnas. Se reconocen la apertura, la regularización y el cierre.
//!
//! Lo que se hace ([`import`]): el ejercicio no puede tener asientos (salvo la apertura que creó
//! el cierre del diario del anterior, si éste no trae la suya); se crean las cuentas que
//! falten con el nombre del gestor y los asientos en un lote (`gestor_import`, origen
//! [`Origin::Gestor`]) que se puede deshacer entero ([`undo`]). Los asientos del gestor no entran
//! en la cartera ni en los borradores que se hacen con facturas: son el histórico. Si el diario
//! trae el cierre, el ejercicio queda cerrado y se abre el siguiente; si no, queda abierto para
//! cerrarlo con TaxPain (Contabilidad → Cierre).
//!
//! Después, el **cotejo** ([`compare_balance`], [`compare_models`]): saldo a saldo con el balance
//! de sumas y saldos del gestor y con los modelos presentados que haya (303 por trimestre, 200).
//! Y el **inventario de activos** del gestor ([`read_inventory`]) para el asistente de saldos
//! iniciales, con lo ya amortizado.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use taxpain_core::accounting::closing::result_of_year;
use taxpain_core::accounting::depreciation::FixedAsset;
use taxpain_core::accounting::ledger::trial_balance as tb;
use taxpain_core::accounting::{AccountCode, EntryLine, FiscalYear, JournalEntry, Origin};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, Duration, Month, OffsetDateTime};

use crate::accounting;
use crate::apertura::{self, Asset};
use crate::error::{AppError, Result};
use crate::import::{Hoja, año_de_cabecera, norm, parse_amount, parse_date_in};
use crate::invoicing::now_str;

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

fn dmy(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// ¿Todas las líneas son de cuentas de balance (grupos 1 a 5)?
fn balance_only(e: &JournalEntry) -> bool {
    e.lines.iter().all(|l| (1..=5).contains(&l.account.group()))
}

// ---------------- Leer ----------------

/// El diario del gestor leído, con la apertura, la regularización y el cierre reconocidos (su
/// `origin`: [`Origin::Apertura`], [`Origin::Regularizacion`], [`Origin::Cierre`]; el resto,
/// [`Origin::Gestor`] con lote 0 hasta que se importe).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Parsed {
    /// `diario`, `resumido` o `suenlace`.
    pub source: String,
    pub filename: String,
    pub sha256: String,
    pub entries: Vec<JournalEntry>,
    /// Nombres de cuenta que trae el fichero.
    pub accounts: Vec<(String, String)>,
    pub warnings: Vec<String>,
}

impl Parsed {
    pub fn opening(&self) -> Option<&JournalEntry> {
        self.entries.iter().find(|e| e.origin == Origin::Apertura)
    }

    pub fn regularization(&self) -> Option<&JournalEntry> {
        self.entries
            .iter()
            .find(|e| e.origin == Origin::Regularizacion)
    }

    pub fn closing(&self) -> Option<&JournalEntry> {
        self.entries.iter().find(|e| e.origin == Origin::Cierre)
    }

    /// Asientos corrientes (los que irán con el lote).
    pub fn current(&self) -> impl Iterator<Item = &JournalEntry> {
        self.entries
            .iter()
            .filter(|e| matches!(e.origin, Origin::Gestor(_)))
    }

    pub fn lines(&self) -> usize {
        self.entries.iter().map(|e| e.lines.len()).sum()
    }

    pub fn first_date(&self) -> Option<Date> {
        self.entries.iter().map(|e| e.date).min()
    }

    pub fn last_date(&self) -> Option<Date> {
        self.entries.iter().map(|e| e.date).max()
    }

    /// Resultado del ejercicio (ingresos − gastos) según los asientos corrientes y la apertura.
    pub fn result(&self) -> Money {
        let without: Vec<JournalEntry> = self
            .entries
            .iter()
            .filter(|e| !matches!(e.origin, Origin::Regularizacion | Origin::Cierre))
            .cloned()
            .collect();
        result_of_year(&without)
    }
}

/// Asientos y nombres de cuenta de un diario resumido.
type SummaryParsed = (Vec<JournalEntry>, BTreeMap<String, String>);

/// Fila de un lote (`gestor_import`) tal como está en la tabla: ejercicio, fichero, los tres
/// asientos del ejercicio, la apertura del siguiente y si lo cerró.
type LotRow = (
    i32,
    String,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    i64,
);

#[derive(Default)]
struct SummaryCols {
    date: Option<usize>,
    debit_account: Option<usize>,
    credit_account: Option<usize>,
    debit: Option<usize>,
    credit: Option<usize>,
    name: Option<usize>,
}

/// Columnas del diario resumido de a3: `Fecha | Importe Debe | Cuenta Debe | Descripción Cuenta
/// | Cuenta Haber | Importe Haber`.
fn summary_cols(row: &[String]) -> SummaryCols {
    let mut c = SummaryCols::default();
    for (i, h) in row.iter().enumerate() {
        let n = norm(h);
        let slot = match n.as_str() {
            "fecha" | "fechaasiento" => &mut c.date,
            "cuentadebe" | "ctadebe" | "cuentadeudora" => &mut c.debit_account,
            "cuentahaber" | "ctahaber" | "cuentaacreedora" => &mut c.credit_account,
            "importedebe" | "debe" => &mut c.debit,
            "importehaber" | "haber" => &mut c.credit,
            "descripcioncuenta" | "descripcion" | "nombrecuenta" | "concepto" => &mut c.name,
            _ => continue,
        };
        if slot.is_none() {
            *slot = Some(i);
        }
    }
    c
}

/// Diario resumido: cada fila es una cuenta de un mes, con su importe en el debe o en el haber;
/// las filas de la misma fecha forman un asiento. `None` si la hoja no tiene esas columnas.
fn parse_summary(rows: &[Vec<String>]) -> Option<Result<SummaryParsed>> {
    let (head, cols) = rows
        .iter()
        .enumerate()
        .take(25)
        .map(|(i, r)| (i, summary_cols(r)))
        .find(|(_, c)| {
            c.date.is_some()
                && c.debit_account.is_some()
                && c.credit_account.is_some()
                && c.debit.is_some()
                && c.credit.is_some()
        })?;
    let año = año_de_cabecera(&rows[..head]);
    let cell = |r: &[String], c: Option<usize>| -> String {
        c.and_then(|i| r.get(i))
            .map(|s| s.trim().to_owned())
            .unwrap_or_default()
    };
    let mut by_date: BTreeMap<Date, (Vec<EntryLine>, usize)> = BTreeMap::new();
    let mut names = BTreeMap::new();
    let mut last_date: Option<Date> = None;
    for (i, r) in rows.iter().enumerate().skip(head + 1) {
        let fila = i + 1;
        let date_s = cell(r, cols.date);
        let da = cell(r, cols.debit_account);
        let ca = cell(r, cols.credit_account);
        if da.is_empty() && ca.is_empty() {
            continue; // totales, títulos
        }
        let date = if date_s.is_empty() {
            match last_date {
                Some(d) => d,
                None => return Some(Err(other(format!("fila {fila}: falta la fecha")))),
            }
        } else {
            match parse_date_in(&date_s, año) {
                Some(d) => d,
                None => {
                    return Some(Err(other(format!(
                        "fila {fila}: fecha no válida «{date_s}»"
                    ))));
                }
            }
        };
        last_date = Some(date);
        let name = cell(r, cols.name);
        let e = by_date.entry(date).or_insert((Vec::new(), fila));
        for (account_s, amount_s, debit) in [
            (&da, cell(r, cols.debit), true),
            (&ca, cell(r, cols.credit), false),
        ] {
            if account_s.is_empty() {
                continue;
            }
            let account = match AccountCode::new(account_s) {
                Ok(a) => a,
                Err(_) => {
                    return Some(Err(other(format!(
                        "fila {fila}: cuenta no válida «{account_s}»"
                    ))));
                }
            };
            let Some(amount) = parse_amount(&amount_s) else {
                return Some(Err(other(format!(
                    "fila {fila}: importe no válido «{amount_s}»"
                ))));
            };
            if !name.is_empty() {
                names
                    .entry(account.as_str().to_owned())
                    .or_insert(name.clone());
            }
            e.0.extend(EntryLine::signed(account, amount, debit));
        }
    }
    if by_date.is_empty() {
        return None;
    }
    let mut out = Vec::new();
    for (date, (lines, fila)) in by_date {
        let e = JournalEntry {
            date,
            concept: format!("Diario resumido del gestor ({})", month_label(date)),
            origin: Origin::Gestor(0),
            lines,
        }
        .compact();
        if let Err(err) = e.validate() {
            return Some(Err(other(format!(
                "fila {fila}: el resumen del {} no cuadra: {err}",
                dmy(date)
            ))));
        }
        out.push(e);
    }
    Some(Ok((out, names)))
}

fn month_label(d: Date) -> String {
    let m = match d.month() {
        Month::January => "enero",
        Month::February => "febrero",
        Month::March => "marzo",
        Month::April => "abril",
        Month::May => "mayo",
        Month::June => "junio",
        Month::July => "julio",
        Month::August => "agosto",
        Month::September => "septiembre",
        Month::October => "octubre",
        Month::November => "noviembre",
        Month::December => "diciembre",
    };
    format!("{m} de {}", d.year())
}

/// Las hojas con diario resumido, seguidas. `None` si ninguna lo es.
fn parse_summary_sheets(hojas: &[Hoja]) -> Option<Result<(SummaryParsed, Vec<String>)>> {
    let mut entries = Vec::new();
    let mut names = BTreeMap::new();
    let mut skipped = Vec::new();
    let mut found = false;
    for h in hojas {
        match parse_summary(&h.filas) {
            Some(Ok((e, n))) => {
                found = true;
                entries.extend(e);
                names.extend(n);
            }
            Some(Err(e)) => {
                return Some(Err(if h.nombre.is_empty() {
                    e
                } else {
                    other(format!("hoja «{}»: {e}", h.nombre))
                }));
            }
            None => skipped.push(h.nombre.clone()),
        }
    }
    if !found {
        return None;
    }
    let mut warnings = Vec::new();
    if hojas.len() > 1 && !skipped.is_empty() {
        warnings.push(format!(
            "hojas sin diario resumido, que no se importan: {}",
            skipped.join(", ")
        ));
    }
    Some(Ok(((entries, names), warnings)))
}

/// Aviso de un diario sin asiento de apertura.
const NO_OPENING: &str = "no se reconoce el asiento de apertura (documento o concepto «Apertura», \
                          o el primero del 1 de enero sólo con cuentas de balance): el ejercicio \
                          empezará sin saldos iniciales";

/// Reconoce la apertura, la regularización y el cierre y ordena por fecha.
fn classify(entries: &mut [JournalEntry], warnings: &mut Vec<String>) {
    entries.sort_by_key(|e| e.date);
    let first = entries.first().map(|e| e.date);
    let last = entries.last().map(|e| e.date);
    let mut opening = None;
    let mut regularization = None;
    let mut closing = None;
    for (i, e) in entries.iter().enumerate() {
        let c = norm(&e.concept);
        if opening.is_none()
            && balance_only(e)
            && (c.contains("apertura")
                || (i == 0
                    && Some(e.date) == first
                    && e.date.day() == 1
                    && e.date.month() == Month::January))
        {
            opening = Some(i);
            continue;
        }
        if c.contains("regularizacion") && e.lines.iter().any(|l| l.account.starts_with("129")) {
            regularization = Some(i);
            continue;
        }
        if c.contains("cierre") && balance_only(e) && Some(e.date) == last {
            closing = Some(i);
        }
    }
    for (i, e) in entries.iter_mut().enumerate() {
        e.origin = if Some(i) == opening {
            Origin::Apertura
        } else if Some(i) == regularization {
            Origin::Regularizacion
        } else if Some(i) == closing {
            Origin::Cierre
        } else {
            Origin::Gestor(0)
        };
    }
    if opening.is_none() {
        warnings.push(NO_OPENING.into());
    }
    if regularization.is_none() {
        warnings.push(
            "no viene la regularización (concepto «Regularización», con la 129): la hará \
             TaxPain al cerrar el ejercicio"
                .into(),
        );
    }
    if closing.is_none() {
        warnings.push(
            "no viene el asiento de cierre (concepto «Cierre»): el ejercicio queda abierto y se \
             cierra con TaxPain (Contabilidad → Cierre), que hará también la apertura del \
             siguiente"
                .into(),
        );
    } else if regularization.is_none() {
        warnings.push(
            "viene el cierre pero no la regularización: comprueba que el cierre salda también \
             las cuentas de gastos e ingresos"
                .into(),
        );
    }
}

/// Lee el diario del gestor sin tocar nada: diario completo (a3 «Diario.», Sage, ContaSOL…),
/// diario resumido de a3, SUENLACE o CSV con columnas reconocibles. Cada asiento tiene que
/// cuadrar: si uno no cuadra, falla diciendo la fila y no se lee nada.
pub fn read(filename: &str, bytes: &[u8]) -> Result<Parsed> {
    let lower = filename.to_lowercase();
    let suenlace = lower.ends_with(".dat")
        || (matches!(bytes.first(), Some(b'4' | b'5'))
            && !lower.ends_with(".csv")
            && !crate::import::is_spreadsheet(filename, bytes));
    let mut parsed = Parsed {
        source: String::new(),
        filename: filename.to_owned(),
        sha256: sha(bytes),
        entries: Vec::new(),
        accounts: Vec::new(),
        warnings: Vec::new(),
    };
    let mut done = false;
    if !suenlace
        && let Ok(hojas) = crate::import::sheets(filename, bytes)
        && let Some(r) = parse_summary_sheets(&hojas)
    {
        let ((entries, names), warnings) = r?;
        parsed.source = "resumido".into();
        parsed.entries = entries;
        parsed.accounts = names.into_iter().collect();
        parsed.warnings = warnings;
        parsed.warnings.push(
            "es un diario resumido (una línea por cuenta y mes): cada mes entra como un asiento \
             con la fecha del listado, sin el detalle de cada operación"
                .into(),
        );
        done = true;
    }
    if !done {
        let (entries, accounts, warnings) =
            crate::import::read_journal(filename, bytes).map_err(|e| {
                other(format!(
                    "no lo reconozco como diario del gestor (columnas Fecha, Asiento, Cuenta, \
                     Debe y Haber; o Fecha, Cuenta Debe, Cuenta Haber e importes; o SUENLACE): \
                     {e}"
                ))
            })?;
        parsed.source = if suenlace { "suenlace" } else { "diario" }.into();
        parsed.entries = entries;
        parsed.accounts = accounts
            .into_iter()
            .map(|(c, n)| (c.as_str().to_owned(), n))
            .collect();
        parsed.warnings = warnings;
    }
    if parsed.entries.is_empty() {
        return Err(other("el fichero no tiene asientos"));
    }
    let mut warnings = std::mem::take(&mut parsed.warnings);
    classify(&mut parsed.entries, &mut warnings);
    parsed.warnings = warnings;
    Ok(parsed)
}

// ---------------- Vista previa ----------------

/// Cuenta que el lote daría de alta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NewAccount {
    pub code: String,
    pub name: String,
}

/// Lo que se importaría y lo que lo impide.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Preview {
    pub source: String,
    pub filename: String,
    /// Ejercicio al que van los asientos (el que contiene la primera fecha).
    pub fiscal_year: Option<i32>,
    pub fiscal_year_label: Option<String>,
    pub entries: usize,
    pub lines: usize,
    pub first_date: Option<Date>,
    pub last_date: Option<Date>,
    /// Suma del debe de todos los asientos.
    pub total: Money,
    /// Resultado del ejercicio según el diario (ingresos − gastos; negativo: pérdida).
    pub result: Money,
    pub has_opening: bool,
    pub has_regularization: bool,
    pub has_closing: bool,
    /// Se creará la apertura del ejercicio siguiente (el diario trae el cierre y el siguiente
    /// no la tiene).
    pub next_opening: bool,
    /// El ejercicio siguiente tiene su propio diario (cargado en TaxPain o junto al fichero
    /// elegido) y trae su apertura: cuál. Entonces la del cierre no se propone, porque los saldos
    /// iniciales irían dos veces. Si ese diario no trae apertura, sí se propone: se carga después
    /// sobre ella.
    pub next_ledger: Option<String>,
    pub accounts: usize,
    pub new_accounts: Vec<NewAccount>,
    /// Los primeros asientos, para echarles un vistazo.
    pub sample: Vec<JournalEntry>,
    /// Lo que impide importar.
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

fn closed(conn: &Connection, year: i32) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT closed FROM fiscal_year WHERE year = ?1",
            [year],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
        == Some(1))
}

fn year_entries(conn: &Connection, year: i32) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT count(*) FROM journal_entry WHERE fiscal_year = ?1",
        [year],
        |r| r.get(0),
    )?)
}

/// La apertura de `fy` (su primer día) que creó el cierre del diario del gestor del ejercicio
/// anterior, si la tiene: (ejercicio de ese diario, su fichero).
fn inherited_opening(conn: &Connection, fy: &FiscalYear) -> Result<Option<(String, String)>> {
    let row: Option<(i32, String)> = conn
        .query_row(
            "SELECT g.fiscal_year, g.filename FROM gestor_import g
               JOIN journal_entry e ON e.id = g.next_opening_entry
              WHERE e.fiscal_year = ?1 AND e.date = ?2
              LIMIT 1",
            params![fy.year, fy.start.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    Ok(match row {
        Some((year, file)) => Some((accounting::fiscal_year_conn(conn, year)?.label(), file)),
        None => None,
    })
}

/// Apertura que ya tiene el ejercicio `year` el día `date` (la de un cierre anterior o la del
/// asistente de saldos iniciales).
fn opening_exists(conn: &Connection, year: i32, date: Date) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT count(*) FROM journal_entry WHERE fiscal_year = ?1 AND origin_kind = 'Apertura'
            AND date = ?2",
        params![year, date.to_string()],
        |r| r.get::<_, i64>(0),
    )? > 0)
}

/// Cuentas del fichero que TaxPain no tiene (ni con nombre propio ni de un tercero), con el
/// nombre del gestor o, si no lo da, el de su cuenta del PGC.
fn new_accounts(conn: &Connection, p: &Parsed) -> Result<Vec<NewAccount>> {
    let names: BTreeMap<&str, &str> = p
        .accounts
        .iter()
        .map(|(c, n)| (c.as_str(), n.as_str()))
        .collect();
    let codes: BTreeSet<&str> = p
        .entries
        .iter()
        .flat_map(|e| e.lines.iter().map(|l| l.account.as_str()))
        .collect();
    let mut out = Vec::new();
    for code in codes {
        let known: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM account WHERE code = ?1
                 UNION ALL SELECT 1 FROM third_party_account WHERE account = ?1 LIMIT 1",
                [code],
                |r| r.get(0),
            )
            .optional()?;
        if known.is_some() {
            continue;
        }
        let name = match names.get(code).map(|n| n.trim()).filter(|n| !n.is_empty()) {
            Some(n) => n.to_owned(),
            None if code.len() > 4 => taxpain_core::accounting::pgc::name_for(code)
                .unwrap_or("")
                .to_owned(),
            // Una cuenta del cuadro del PGC sin nombre propio: ya la conoce TaxPain.
            None => continue,
        };
        out.push(NewAccount {
            code: code.to_owned(),
            name,
        });
    }
    Ok(out)
}

/// Comprueba el diario contra el almacén y dice qué se importaría (no cambia nada).
pub fn preview(v: &Vault, p: &Parsed) -> Result<Preview> {
    let conn = v.connection();
    let mut errors = Vec::new();
    let mut warnings = p.warnings.clone();
    let first = p.first_date();
    let last = p.last_date();
    let fy: Option<FiscalYear> = match first {
        Some(d) => Some(accounting::fiscal_year_at(v, d)?),
        None => None,
    };
    let mut next_opening = false;
    let mut next_ledger = None;
    if let Some(fy) = &fy {
        if let Some(e) = p.entries.iter().find(|e| !fy.contains(e.date)) {
            errors.push(format!(
                "el asiento «{}» del {} está fuera del ejercicio {} ({} a {}): un fichero por \
                 ejercicio",
                e.concept,
                dmy(e.date),
                fy.label(),
                dmy(fy.start),
                dmy(fy.end)
            ));
        }
        if closed(conn, fy.year)? {
            errors.push(format!(
                "el ejercicio {} está cerrado en TaxPain: no se le pueden añadir asientos",
                fy.label()
            ));
        }
        let n = year_entries(conn, fy.year)?;
        // La apertura que creó el cierre del diario del ejercicio anterior es la que le falta a
        // un diario que no trae la suya: se carga sobre ella. Si trae la suya, irían dos.
        match inherited_opening(conn, fy)? {
            Some((prev, file)) if n == 1 && p.opening().is_none() => {
                warnings.retain(|w| w != NO_OPENING);
                warnings.push(format!(
                    "el ejercicio {} ya tiene la apertura que creó el cierre del diario de {prev} \
                     («{file}»); este diario no trae la suya, así que se carga con ésa",
                    fy.label()
                ));
            }
            Some((prev, file)) if n == 1 => errors.push(format!(
                "el ejercicio {} ya tiene 1 asiento(s) en TaxPain: la apertura que creó el cierre \
                 del diario de {prev} («{file}»), y este diario trae la suya, así que irían dos. \
                 Deshaz la carga de {prev} y vuelve a hacerla sin «Crear la apertura de {}»",
                fy.label(),
                fy.label()
            )),
            _ if n > 0 => errors.push(format!(
                "el ejercicio {} ya tiene {n} asiento(s) en TaxPain: un ejercicio del gestor \
                 sólo se carga en uno vacío. Si son del propio gestor (otro lote, un diario \
                 importado o la apertura del asistente), deshazlos antes; si son de TaxPain, \
                 ese ejercicio ya no es del gestor",
                fy.label()
            )),
            _ => {}
        }
        if let Some(o) = p.opening()
            && o.date != fy.start
        {
            warnings.push(format!(
                "la apertura es del {} y el ejercicio empieza el {}",
                dmy(o.date),
                dmy(fy.start)
            ));
        }
        if p.closing().is_some() {
            let next = fy.end + Duration::days(1);
            let ny = accounting::fiscal_year_at(v, next)?;
            if closed(conn, ny.year)? {
                errors.push(format!(
                    "el ejercicio siguiente ({}) está cerrado en TaxPain y el diario trae el \
                     cierre: no se puede enlazar la apertura",
                    ny.label()
                ));
            } else if opening_exists(conn, ny.year, next)? {
                warnings.push(format!(
                    "el ejercicio {} ya tiene su asiento de apertura: no se crea otro con el \
                     cierre del gestor; comprueba que coincidan",
                    ny.label()
                ));
            } else {
                next_opening = true;
                next_ledger = ledger_in_vault(conn, ny.year, next)?
                    .map(|f| format!("«{f}», ya cargado en TaxPain"));
                warnings.push(closing_warning(
                    fy,
                    &ny,
                    next_ledger.as_deref().map(NextLedger::Loaded),
                ));
            }
        }
    } else {
        errors.push("el fichero no tiene asientos".into());
    }
    let already: Option<(String, String)> = conn
        .query_row(
            "SELECT filename, created_at FROM gestor_import WHERE sha256 = ?1",
            [&p.sha256],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((f, at)) = already {
        errors.push(format!(
            "este fichero ya se importó: {f} (el {})",
            at.get(..10).unwrap_or(&at)
        ));
    }
    let new_accounts = new_accounts(conn, p)?;
    let codes: BTreeSet<&str> = p
        .entries
        .iter()
        .flat_map(|e| e.lines.iter().map(|l| l.account.as_str()))
        .collect();
    Ok(Preview {
        source: p.source.clone(),
        filename: p.filename.clone(),
        fiscal_year: fy.as_ref().map(|f| f.year),
        fiscal_year_label: fy.as_ref().map(FiscalYear::label),
        entries: p.entries.len(),
        lines: p.lines(),
        first_date: first,
        last_date: last,
        total: p
            .entries
            .iter()
            .fold(Money::ZERO, |a, e| a + e.total_debit()),
        result: p.result(),
        has_opening: p.opening().is_some(),
        has_regularization: p.regularization().is_some(),
        has_closing: p.closing().is_some(),
        next_opening,
        next_ledger,
        accounts: codes.len(),
        new_accounts,
        sample: p.entries.iter().take(5).cloned().collect(),
        errors,
        warnings,
    })
}

/// Vista previa de un fichero (lo lee y lo comprueba).
pub fn preview_file(v: &Vault, filename: &str, bytes: &[u8]) -> Result<Preview> {
    let p = read(filename, bytes)?;
    preview(v, &p)
}

/// Como [`preview_file`] con el fichero elegido en su sitio: si trae el cierre, mira también si
/// el ejercicio siguiente tiene su propio diario junto a él ([`ledger_nearby`]). Si ese diario
/// trae su apertura, la del cierre no se propone: con las dos, irían dos veces (y ese diario ya
/// no se podría cargar). Si no la trae, se propone, y ese diario se carga después sobre ella.
pub fn preview_file_at(v: &Vault, path: &std::path::Path, bytes: &[u8]) -> Result<Preview> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut pv = preview_file(v, &name, bytes)?;
    if !pv.next_opening || pv.next_ledger.is_some() {
        return Ok(pv);
    }
    let Some(year) = pv.fiscal_year else {
        return Ok(pv);
    };
    let fy = accounting::fiscal_year(v, year)?;
    let ny = accounting::fiscal_year_at(v, fy.end + Duration::days(1))?;
    if let Some((found, opening)) = ledger_nearby(v, path, ny.year) {
        let before = closing_warning(&fy, &ny, None);
        let where_ = format!("«{found}», junto al fichero elegido");
        let after = closing_warning(
            &fy,
            &ny,
            Some(NextLedger::Nearby {
                at: &where_,
                opening,
            }),
        );
        for w in &mut pv.warnings {
            if *w == before {
                w.clone_from(&after);
            }
        }
        if opening {
            pv.next_ledger = Some(where_);
        }
    }
    Ok(pv)
}

/// El diario propio del ejercicio siguiente, para el aviso del cierre.
#[derive(Clone, Copy)]
enum NextLedger<'a> {
    /// Ya cargado en TaxPain, con su apertura: dónde.
    Loaded(&'a str),
    /// Junto al fichero elegido: dónde y si trae su apertura.
    Nearby { at: &'a str, opening: bool },
}

/// Aviso del diario que trae el cierre: se cierra el ejercicio y, salvo que el siguiente tenga su
/// propio diario con su apertura, se abre el siguiente con sus saldos.
fn closing_warning(fy: &FiscalYear, ny: &FiscalYear, next_ledger: Option<NextLedger>) -> String {
    let (fy, ny) = (fy.label(), ny.label());
    match next_ledger {
        None => format!(
            "el diario trae el cierre: el ejercicio {fy} quedará cerrado y se creará la apertura \
             de {ny} con sus saldos"
        ),
        Some(NextLedger::Nearby { at, opening: false }) => format!(
            "el diario trae el cierre: el ejercicio {fy} quedará cerrado y se creará la apertura \
             de {ny} con sus saldos. El diario de {ny} ({at}) no trae apertura: después se carga \
             con ésta"
        ),
        Some(NextLedger::Nearby { at, opening: true }) => format!(
            "el diario trae el cierre: el ejercicio {fy} quedará cerrado. La apertura de {ny} no \
             se marca: el diario de {ny} ({at}) trae la suya y, con las dos, los saldos \
             iniciales irían dos veces (y ese diario ya no se podría cargar)"
        ),
        Some(NextLedger::Loaded(at)) => format!(
            "el diario trae el cierre: el ejercicio {fy} quedará cerrado. La apertura de {ny} no \
             se marca: {ny} ya tiene su propio diario ({at}) con su apertura y, con las dos, los \
             saldos iniciales irían dos veces"
        ),
    }
}

/// Fichero del diario del ejercicio `year` (que empieza el `start`) que ya está en TaxPain y trae
/// su propia apertura: un ejercicio cargado con este importador con su apertura, o un lote de
/// asientos importados con un asiento sólo de cuentas de balance con el concepto «apertura» o del
/// primer día del ejercicio. Un lote sin apertura (unas nóminas, un diario que no la trae) no
/// cuenta: entonces la apertura que sale del cierre es la que le falta.
fn ledger_in_vault(conn: &Connection, year: i32, start: Date) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT filename FROM gestor_import WHERE fiscal_year = ?1 AND opening_entry IS NOT NULL
             UNION ALL
             SELECT b.filename FROM import_batch b
              WHERE EXISTS (SELECT 1 FROM journal_entry e WHERE e.origin_kind = 'Importado'
                             AND e.origin_id = b.id AND e.fiscal_year = ?1
                             AND (lower(e.concept) LIKE '%apertura%' OR e.date = ?2)
                             AND NOT EXISTS (SELECT 1 FROM journal_line l WHERE l.entry_id = e.id
                                  AND substr(l.account, 1, 1) NOT IN ('1', '2', '3', '4', '5')))
             LIMIT 1",
            params![year, start.to_string()],
            |r| r.get(0),
        )
        .optional()?)
}

/// Extensiones de los diarios que se leen ([`read`]).
const LEDGER_EXTENSIONS: [&str; 7] = ["xlsx", "xlsm", "xls", "ods", "csv", "txt", "dat"];

/// Como mucho se miran tantos ficheros al buscar el diario del ejercicio siguiente.
const NEARBY_FILES: usize = 40;

/// Diario del gestor del ejercicio `year` cerca del fichero elegido, si lo hay: en su carpeta,
/// en las de al lado (la carpeta de cada año) o en la carpeta de la empresa (Importar → Carpeta)
/// si el fichero está dentro. Sólo se leen las hojas de cálculo y los CSV o SUENLACE con el año
/// en la ruta, y como mucho [`NEARBY_FILES`]. Devuelve la ruta desde la carpeta en la que se ha
/// buscado y si ese diario trae su asiento de apertura.
pub fn ledger_nearby(v: &Vault, chosen: &std::path::Path, year: i32) -> Option<(String, bool)> {
    use std::path::PathBuf;
    let dir = chosen.parent()?;
    let mut roots: Vec<(PathBuf, u8)> = vec![(dir.to_path_buf(), 0)];
    if let Some(up) = dir.parent() {
        roots.push((up.to_path_buf(), 1));
    }
    if let Some(company) = crate::carpeta::folder(v).ok().flatten().map(PathBuf::from)
        && chosen.starts_with(&company)
    {
        roots.push((company, 4));
    }
    let token = year.to_string();
    let mut seen = BTreeSet::new();
    let mut tried = 0;
    for (root, depth) in roots {
        let mut files = Vec::new();
        nearby_files(&root, &root, depth, &token, &mut files);
        // Primero los que por el nombre parecen un diario.
        files.sort_by_key(|f| {
            let n = norm(&f.to_string_lossy());
            !["diario", "contab", "asiento", "suenlace"]
                .iter()
                .any(|w| n.contains(w))
        });
        for f in files {
            if f == chosen || !seen.insert(f.clone()) {
                continue;
            }
            tried += 1;
            if tried > NEARBY_FILES {
                return None;
            }
            let name = f.file_name()?.to_string_lossy().into_owned();
            let Ok(bytes) = std::fs::read(&f) else {
                continue;
            };
            let Ok(p) = read(&name, &bytes) else {
                continue;
            };
            if p.first_date()
                .and_then(|d| accounting::fiscal_year_at(v, d).ok())
                .is_some_and(|fy| fy.year == year)
            {
                return Some((
                    f.strip_prefix(&root)
                        .unwrap_or(&f)
                        .to_string_lossy()
                        .into_owned(),
                    p.opening().is_some(),
                ));
            }
        }
    }
    None
}

/// Ficheros de `dir` (y de sus subcarpetas, hasta `depth` niveles) que pueden ser un diario y
/// llevan el año `token` en la ruta desde `root`. No se siguen enlaces ni se miran los ocultos.
fn nearby_files(
    root: &std::path::Path,
    dir: &std::path::Path,
    depth: u8,
    token: &str,
    out: &mut Vec<std::path::PathBuf>,
) {
    /// Lo que se recorre como mucho (una carpeta de años de facturas cabe de sobra).
    const MAX_ENTRIES: usize = 5000;
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().take(MAX_ENTRIES).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.starts_with("~$") {
            continue;
        }
        let path = e.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            if depth > 0 {
                nearby_files(root, &path, depth - 1, token, out);
            }
            continue;
        }
        let ext = path
            .extension()
            .map(|x| x.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let rel = path.strip_prefix(root).unwrap_or(&path).to_string_lossy();
        if meta.is_file()
            && meta.len() <= 30 * 1024 * 1024
            && LEDGER_EXTENSIONS.contains(&ext.as_str())
            && has_year(&rel, token)
        {
            out.push(path);
        }
    }
}

/// ¿Lleva `s` el año `token` suelto (no dentro de otro número más largo)?
fn has_year(s: &str, token: &str) -> bool {
    s.match_indices(token).any(|(i, _)| {
        let before = s[..i].chars().next_back();
        let after = s[i + token.len()..].chars().next();
        !before.is_some_and(|c| c.is_ascii_digit()) && !after.is_some_and(|c| c.is_ascii_digit())
    })
}

// ---------------- Importar ----------------

/// Opciones de la importación.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Options {
    /// Si el diario trae el cierre, crear la apertura del ejercicio siguiente con sus saldos
    /// (salvo que ya la tenga).
    #[serde(default = "yes")]
    pub next_opening: bool,
}

fn yes() -> bool {
    true
}

impl Default for Options {
    fn default() -> Self {
        Options { next_opening: true }
    }
}

/// Lote importado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Lot {
    pub id: i64,
    pub fiscal_year: i32,
    pub source: String,
    pub filename: String,
    pub entries: i64,
    pub lines: i64,
    pub accounts_created: i64,
    pub has_opening: bool,
    pub has_regularization: bool,
    pub has_closing: bool,
    pub next_opening: bool,
    /// El lote dejó el ejercicio cerrado (traía el cierre).
    pub closed_year: bool,
    pub created_at: String,
}

/// Importa el ejercicio `year` del diario leído, en una transacción: cuentas nuevas, asientos
/// con el lote como origen, la apertura, la regularización y el cierre con los suyos. Con el
/// cierre, el ejercicio queda cerrado y se abre el siguiente (si no tenía apertura).
pub fn import(
    v: &Vault,
    year: i32,
    p: &Parsed,
    options: &Options,
    now: OffsetDateTime,
) -> Result<Lot> {
    let pv = preview(v, p)?;
    if !pv.errors.is_empty() {
        return Err(other(format!(
            "no se puede importar: {}",
            pv.errors.join("; ")
        )));
    }
    if pv.fiscal_year != Some(year) {
        return Err(other(format!(
            "el fichero es del ejercicio {}, no de {year}",
            pv.fiscal_year_label.unwrap_or_default()
        )));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let fy = accounting::fiscal_year_for_date(&tx, pv.first_date.ok_or(AppError::NotFound)?)?;
    tx.execute(
        "INSERT INTO gestor_import(fiscal_year, source, filename, sha256, entries, lines,
                                   accounts_created, created_at)
         VALUES (?1, ?2, ?3, ?4, 0, 0, 0, ?5)",
        params![fy.year, p.source, p.filename, p.sha256, now_str(now)],
    )?;
    let lot = tx.last_insert_rowid();
    let mut created = 0i64;
    for a in &pv.new_accounts {
        if a.name.is_empty() {
            continue;
        }
        created += i64::try_from(tx.execute(
            "INSERT OR IGNORE INTO account(code, name, created_at) VALUES (?1, ?2, ?3)",
            params![a.code, a.name, now_str(now)],
        )?)
        .unwrap_or(0);
    }
    let mut opening = None;
    let mut regularization = None;
    let mut closing = None;
    let mut n = 0i64;
    let closed_err = |e: AppError, date: Date| match e {
        AppError::Vault(_) if e.to_string().contains("cerrado") => other(format!(
            "el ejercicio del {} está cerrado: no se le pueden añadir asientos",
            dmy(date)
        )),
        e => e,
    };
    for e in &p.entries {
        let mut e = e.clone();
        if matches!(e.origin, Origin::Gestor(_)) {
            e.origin = Origin::Gestor(lot);
        }
        let id = accounting::record_entry(&tx, &e, now).map_err(|err| closed_err(err, e.date))?;
        match e.origin {
            Origin::Apertura => opening = Some(id),
            Origin::Regularizacion => regularization = Some(id),
            Origin::Cierre => closing = Some(id),
            _ => n += 1,
        }
    }
    let mut next_opening = None;
    let mut closed_year = false;
    if let Some(c) = p.closing() {
        let next = fy.end + Duration::days(1);
        if options.next_opening && pv.next_opening {
            let o = taxpain_core::accounting::closing::opening(c, next);
            next_opening =
                Some(accounting::record_entry(&tx, &o, now).map_err(|err| closed_err(err, next))?);
        }
        tx.execute(
            "UPDATE fiscal_year SET closed = 1, closed_at = ?2 WHERE year = ?1",
            params![fy.year, now_str(now)],
        )?;
        closed_year = true;
    }
    tx.execute(
        "UPDATE gestor_import SET entries = ?2, lines = ?3, accounts_created = ?4,
                opening_entry = ?5, regularization_entry = ?6, closing_entry = ?7,
                next_opening_entry = ?8, closed_year = ?9
          WHERE id = ?1",
        params![
            lot,
            n,
            i64::try_from(p.lines()).unwrap_or(0),
            created,
            opening,
            regularization,
            closing,
            next_opening,
            i64::from(closed_year)
        ],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "gestor.importar",
        &lot.to_string(),
        &serde_json::json!({
            "year": fy.year,
            "source": p.source,
            "file": p.filename,
            "entries": n,
            "accounts": created,
            "opening": opening,
            "regularization": regularization,
            "closing": closing,
            "next_opening": next_opening,
        }),
    )?;
    tx.commit()?;
    lots(v)?
        .into_iter()
        .find(|l| l.id == lot)
        .ok_or(AppError::NotFound)
}

/// Importa un fichero (lectura, comprobación e importación de una vez).
pub fn import_file(
    v: &Vault,
    year: i32,
    filename: &str,
    bytes: &[u8],
    options: &Options,
    now: OffsetDateTime,
) -> Result<Lot> {
    let p = read(filename, bytes)?;
    import(v, year, &p, options, now)
}

/// Los lotes importados, el último primero.
pub fn lots(v: &Vault) -> Result<Vec<Lot>> {
    let mut st = v.connection().prepare(
        "SELECT id, fiscal_year, source, filename, entries, lines, accounts_created,
                opening_entry IS NOT NULL, regularization_entry IS NOT NULL,
                closing_entry IS NOT NULL, next_opening_entry IS NOT NULL, closed_year,
                created_at
           FROM gestor_import ORDER BY id DESC",
    )?;
    let rows = st.query_map([], |r| {
        Ok(Lot {
            id: r.get(0)?,
            fiscal_year: r.get(1)?,
            source: r.get(2)?,
            filename: r.get(3)?,
            entries: r.get(4)?,
            lines: r.get(5)?,
            accounts_created: r.get(6)?,
            has_opening: r.get(7)?,
            has_regularization: r.get(8)?,
            has_closing: r.get(9)?,
            next_opening: r.get(10)?,
            closed_year: r.get::<_, i64>(11)? == 1,
            created_at: r.get(12)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Ejercicios del gestor ya cargados con asientos entre dos fechas: (ejercicio, fichero). Un
/// diario de esas fechas importado por otro sitio (la carpeta, «Importar asientos») los
/// duplicaría.
pub fn lots_between(v: &Vault, first: Date, last: Date) -> Result<Vec<(i32, String)>> {
    let mut st = v.connection().prepare(
        "SELECT g.fiscal_year, g.filename FROM gestor_import g
          WHERE EXISTS (SELECT 1 FROM journal_entry e WHERE e.date BETWEEN ?1 AND ?2
                   AND ((e.origin_kind = 'Gestor' AND e.origin_id = g.id)
                     OR e.id IN (g.opening_entry, g.regularization_entry, g.closing_entry)))
          ORDER BY g.fiscal_year, g.id",
    )?;
    let rows = st.query_map(params![first.to_string(), last.to_string()], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Deshace un lote: borra sus asientos (también la apertura, la regularización, el cierre y la
/// apertura del siguiente que trajo) y deja el ejercicio como estaba: vacío, o sólo con la
/// apertura que creó el cierre del anterior. No vale si el ejercicio se cerró en TaxPain
/// (reábrelo antes) ni si algo apunta a sus asientos.
pub fn undo(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<usize> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let row: Option<LotRow> = conn
        .query_row(
            "SELECT fiscal_year, filename, opening_entry, regularization_entry, closing_entry,
                    next_opening_entry, closed_year
               FROM gestor_import WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .optional()?;
    let (year, filename, opening, regularization, closing, next_opening, closed_year) =
        row.ok_or(AppError::NotFound)?;
    if closed(conn, year)? && closed_year == 0 {
        return Err(other(format!(
            "el ejercicio {year} se cerró en TaxPain: reábrelo antes (Contabilidad → Cierre) y \
             vuelve a deshacer el lote"
        )));
    }
    if closed(conn, year)? && closed_year == 1 {
        // Lo cerró el lote: si después se hizo algo que dependa del cierre, no se toca.
        let after: i64 = conn.query_row(
            "SELECT count(*) FROM journal_entry WHERE fiscal_year = ?1
                AND origin_kind IN ('AplicacionResultado', 'ImpuestoSociedades')",
            [year],
            |r| r.get(0),
        )?;
        if after > 0 {
            return Err(other(format!(
                "en {year} hay asientos de TaxPain posteriores al lote (aplicación del \
                 resultado o Impuesto sobre Sociedades): deshazlos antes"
            )));
        }
    }
    if let Some(no) = next_opening {
        let ny: Option<i32> = conn
            .query_row(
                "SELECT fiscal_year FROM journal_entry WHERE id = ?1",
                [no],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(ny) = ny
            && closed(conn, ny)?
        {
            return Err(other(format!(
                "el ejercicio {ny} (abierto con el cierre del gestor) está cerrado: reábrelo \
                 antes"
            )));
        }
    }
    let tx = conn.unchecked_transaction()?;
    let mut ids: Vec<i64> = {
        let mut st = tx.prepare(
            "SELECT id FROM journal_entry WHERE origin_kind = 'Gestor' AND origin_id = ?1",
        )?;
        let rows = st.query_map([id], |r| r.get(0))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    ids.extend(
        [opening, regularization, closing, next_opening]
            .into_iter()
            .flatten(),
    );
    for (table, column, why) in crate::import::ENTRY_REFERENCES {
        let blocked: Option<(i64, String, String)> = tx
            .query_row(
                &format!(
                    "SELECT e.id, e.date, e.concept FROM journal_entry e
                      WHERE e.id IN (SELECT id FROM journal_entry
                                      WHERE origin_kind = 'Gestor' AND origin_id = ?1)
                        AND EXISTS(SELECT 1 FROM {table} x WHERE x.{column} = e.id)
                      ORDER BY e.id LIMIT 1"
                ),
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((entry, date, concept)) = blocked {
            return Err(other(format!(
                "no se puede deshacer: el asiento {entry} ({date}, {concept}) {why}; deshaz \
                 antes eso"
            )));
        }
    }
    if closed_year == 1 {
        tx.execute(
            "UPDATE fiscal_year SET closed = 0, closed_at = NULL WHERE year = ?1",
            [year],
        )?;
    }
    // Primero se desenlaza el lote de sus asientos (claves foráneas) y luego se borran.
    tx.execute(
        "UPDATE gestor_import SET opening_entry = NULL, regularization_entry = NULL,
                closing_entry = NULL, next_opening_entry = NULL WHERE id = ?1",
        [id],
    )?;
    for e in &ids {
        tx.execute(
            "DELETE FROM line_tick WHERE line_id IN (SELECT id FROM journal_line WHERE entry_id = ?1)",
            [e],
        )?;
        tx.execute("DELETE FROM entry_document WHERE entry_id = ?1", [e])?;
        tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [e])
            .map_err(|err| other(format!("no se puede deshacer: {err}")))?;
        tx.execute("DELETE FROM journal_entry WHERE id = ?1", [e])
            .map_err(|err| other(format!("no se puede deshacer: {err}")))?;
    }
    tx.execute("DELETE FROM gestor_import WHERE id = ?1", [id])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "gestor.deshacer",
        &id.to_string(),
        &serde_json::json!({ "file": filename, "year": year, "entries": ids.len() }),
    )?;
    tx.commit()?;
    Ok(ids.len())
}

// ---------------- Cotejo con el balance de sumas y saldos ----------------

/// Una cuenta en el cotejo con el balance del gestor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BalanceRow {
    pub account: String,
    pub name: String,
    /// Saldo según el gestor (deudor positivo, acreedor negativo).
    pub theirs: Option<Money>,
    /// Saldo según TaxPain (la cuenta y sus subcuentas).
    pub ours: Money,
    /// `ours − theirs`.
    pub diff: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BalanceComparison {
    pub fiscal_year: i32,
    /// `tras_regularizacion` (el balance del gestor lleva la 129 y no los grupos 6 y 7) o
    /// `antes_regularizacion`.
    pub basis: String,
    pub rows: Vec<BalanceRow>,
    pub mismatches: usize,
    pub warnings: Vec<String>,
}

fn balances_of(entries: &[JournalEntry], skip: &[Origin]) -> BTreeMap<String, Money> {
    let kept: Vec<&JournalEntry> = entries
        .iter()
        .filter(|e| !skip.contains(&e.origin))
        .collect();
    tb(kept)
        .into_iter()
        .filter(|b| !b.balance().is_zero())
        .map(|b| (b.account.as_str().to_owned(), b.balance()))
        .collect()
}

/// ¿`a` es `b` o una subcuenta suya (o al revés)?
fn related(a: &str, b: &str) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

fn compare_with(
    theirs: &[apertura::Line],
    ours: &BTreeMap<String, Money>,
) -> (Vec<BalanceRow>, usize) {
    let mut rows = Vec::new();
    let mut used: BTreeSet<&str> = BTreeSet::new();
    for l in theirs {
        let t = l.debit - l.credit;
        let o: Money = ours
            .iter()
            .filter(|(c, _)| related(c, &l.account))
            .map(|(c, m)| {
                used.insert(c.as_str());
                *m
            })
            .sum();
        rows.push(BalanceRow {
            account: l.account.clone(),
            name: l.name.clone(),
            theirs: Some(t),
            ours: o,
            diff: o - t,
        });
    }
    for (c, m) in ours {
        if !used.contains(c.as_str()) {
            rows.push(BalanceRow {
                account: c.clone(),
                name: String::new(),
                theirs: None,
                ours: *m,
                diff: *m,
            });
        }
    }
    rows.sort_by(|a, b| a.account.cmp(&b.account));
    let mismatches = rows.iter().filter(|r| r.diff.cents().abs() > 1).count();
    (rows, mismatches)
}

/// Compara los saldos del ejercicio `year` en TaxPain con un balance de sumas y saldos del
/// gestor (a3 «Sumas y saldos.», Excel, OpenDocument o CSV). Se compara sin el asiento de
/// cierre; con o sin la regularización, según lo que lleve el balance.
pub fn compare_balance(
    v: &Vault,
    year: i32,
    filename: &str,
    bytes: &[u8],
) -> Result<BalanceComparison> {
    let hojas = crate::import::sheets(filename, bytes)?;
    let (theirs, mut warnings) = hojas
        .iter()
        .find_map(|h| apertura::parse_trial_balance(&h.filas))
        .ok_or_else(|| {
            other(
                "no lo reconozco como balance de sumas y saldos: hacen falta las columnas \
                 Cuenta y Saldo deudor y Saldo acreedor (o Saldo)",
            )
        })?;
    let entries: Vec<JournalEntry> = accounting::journal(v, year)?
        .into_iter()
        .map(|s| s.entry)
        .collect();
    if entries.is_empty() {
        warnings.push(format!("el ejercicio {year} no tiene asientos en TaxPain"));
    }
    let before = balances_of(&entries, &[Origin::Regularizacion, Origin::Cierre]);
    let after = balances_of(&entries, &[Origin::Cierre]);
    let (rows_b, mm_b) = compare_with(&theirs, &before);
    let (rows_a, mm_a) = compare_with(&theirs, &after);
    let (basis, rows, mismatches) = if mm_a <= mm_b {
        ("tras_regularizacion", rows_a, mm_a)
    } else {
        ("antes_regularizacion", rows_b, mm_b)
    };
    if mismatches > 0 {
        warnings.push(format!(
            "{mismatches} cuenta(s) no coinciden: si el balance es a otra fecha o el diario no \
             era el definitivo, pide al gestor los dos del mismo cierre"
        ));
    }
    Ok(BalanceComparison {
        fiscal_year: year,
        basis: basis.into(),
        rows,
        mismatches,
        warnings,
    })
}

// ---------------- Cotejo con los modelos presentados ----------------

/// Una cifra de un modelo frente al diario del gestor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelRow {
    pub model: String,
    pub period: String,
    #[serde(rename = "box")]
    pub box_: u16,
    pub label: String,
    /// Lo que sale del diario.
    pub ours: Money,
    /// Lo presentado (si hay modelo de ese periodo).
    pub theirs: Option<Money>,
    pub diff: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ModelsComparison {
    pub fiscal_year: i32,
    pub rows: Vec<ModelRow>,
    pub mismatches: usize,
    pub warnings: Vec<String>,
}

/// ¿Es el asiento de liquidación del IVA del periodo (477 al debe contra 472 al haber)?
fn is_vat_settlement(e: &JournalEntry) -> bool {
    matches!(e.origin, Origin::LiquidacionIva(_))
        || (e
            .lines
            .iter()
            .any(|l| l.account.starts_with("477") && !l.debit.is_zero())
            && e.lines
                .iter()
                .any(|l| l.account.starts_with("472") && !l.credit.is_zero()))
}

fn net_of(entries: &[&JournalEntry], prefix: &str, natural_credit: bool) -> Money {
    entries
        .iter()
        .flat_map(|e| e.lines.iter())
        .filter(|l| l.account.starts_with(prefix))
        .map(|l| if natural_credit { -l.net() } else { l.net() })
        .sum()
}

/// Compara el diario del ejercicio con los modelos presentados que haya guardados: el 303 de
/// cada trimestre (IVA devengado de la 477 frente a la [27] y deducible de la 472 frente a la
/// [45]), el 130 del autónomo (rendimiento acumulado de los grupos 6 y 7 frente a [01], [02] y
/// [03]) y el 200 (resultado contable frente a la [500]).
pub fn compare_models(v: &Vault, year: i32) -> Result<ModelsComparison> {
    let fy = accounting::fiscal_year(v, year)?;
    let entries: Vec<JournalEntry> = accounting::journal(v, year)?
        .into_iter()
        .map(|s| s.entry)
        .collect();
    let mut rows = Vec::new();
    let mut warnings = Vec::new();
    if entries.is_empty() {
        warnings.push(format!("el ejercicio {year} no tiene asientos en TaxPain"));
    }
    if fy.is_natural() {
        let y = fy.start.year();
        for q in 1u8..=4 {
            let m0 = Month::try_from((q - 1) * 3 + 1).map_err(|e| other(e.to_string()))?;
            let start = Date::from_calendar_date(y, m0, 1).map_err(|e| other(e.to_string()))?;
            let end = if q == 4 {
                fy.end
            } else {
                Date::from_calendar_date(y, m0.nth_next(3), 1).map_err(|e| other(e.to_string()))?
                    - Duration::days(1)
            };
            let in_q: Vec<&JournalEntry> = entries
                .iter()
                .filter(|e| e.date >= start && e.date <= end && !is_vat_settlement(e))
                .filter(|e| !matches!(e.origin, Origin::Regularizacion | Origin::Cierre))
                .collect();
            let devengado = net_of(&in_q, "477", true);
            let deducible = net_of(&in_q, "472", false);
            let period = format!("{q}T");
            let filed = crate::models::filed_boxes(v, "303", year, &period)?.map(|(_, b)| b);
            if filed.is_none() && devengado.is_zero() && deducible.is_zero() {
                continue;
            }
            if filed.is_none() {
                warnings.push(format!(
                    "no hay 303 del {period} de {year} guardado (Modelos → Presentados): \
                     impórtalo o teclea sus casillas para cotejarlo"
                ));
            }
            for (box_, label, ours) in [
                (27u16, "IVA devengado (477)", devengado),
                (45u16, "IVA deducible (472)", deducible),
            ] {
                let theirs = filed
                    .as_ref()
                    .map(|b| b.get(&box_).copied().unwrap_or(Money::ZERO));
                rows.push(ModelRow {
                    model: "303".into(),
                    period: period.clone(),
                    box_,
                    label: label.into(),
                    ours,
                    theirs,
                    diff: ours - theirs.unwrap_or(Money::ZERO),
                });
            }
        }
        // 130 del autónomo (pagos fraccionados, art. 110 RIRPF): el rendimiento acumulado desde
        // el 1 de enero hasta el fin de cada trimestre —ingresos del grupo 7 [01], gastos del
        // grupo 6 [02] y su diferencia [03]— frente a las casillas de los 130 presentados que
        // haya. Sólo cuando hay un 130 guardado del trimestre (a una sociedad no le toca).
        let mut any_130 = false;
        for q in 1u8..=4 {
            let period = format!("{q}T");
            let Some((_, filed)) = crate::models::filed_boxes(v, "130", year, &period)? else {
                continue;
            };
            any_130 = true;
            let m = Month::try_from(q * 3).map_err(|e| other(e.to_string()))?;
            let end =
                Date::from_calendar_date(y, m, m.length(y)).map_err(|e| other(e.to_string()))?;
            let (income, expenses) = crate::models::activity_income_expenses(v, year, end)?;
            for (box_, label, ours) in [
                (1u16, "Ingresos acumulados (grupo 7)", income),
                (2u16, "Gastos acumulados (grupo 6)", expenses),
                (3u16, "Rendimiento neto acumulado", income - expenses),
            ] {
                let theirs = filed.get(&box_).copied().unwrap_or(Money::ZERO);
                rows.push(ModelRow {
                    model: "130".into(),
                    period: period.clone(),
                    box_,
                    label: label.into(),
                    ours,
                    theirs: Some(theirs),
                    diff: ours - theirs,
                });
            }
        }
        if any_130
            && rows
                .iter()
                .any(|r| r.model == "130" && r.diff.cents().abs() > 1)
        {
            warnings.push(
                "las diferencias del 130 suelen ser el 5 % de gastos de difícil justificación de \
                 la estimación directa simplificada (va dentro de la [02]), la amortización que \
                 el gestor contabiliza al cierre o gastos de otro trimestre"
                    .into(),
            );
        }
    } else {
        warnings.push(
            "el ejercicio no coincide con el año natural: el 303 y el 130 por trimestres no se \
             cotejan"
                .into(),
        );
    }
    let without: Vec<JournalEntry> = entries
        .iter()
        .filter(|e| !matches!(e.origin, Origin::Regularizacion | Origin::Cierre))
        .cloned()
        .collect();
    let result = result_of_year(&without);
    match crate::models::filed_boxes(v, "200", year, "0A")? {
        Some((_, b)) => {
            let theirs = b.get(&500).copied().unwrap_or(Money::ZERO);
            rows.push(ModelRow {
                model: "200".into(),
                period: "0A".into(),
                box_: 500,
                label: "Resultado contable (grupos 6 y 7)".into(),
                ours: result,
                theirs: Some(theirs),
                diff: result - theirs,
            });
        }
        None => warnings.push(format!(
            "no hay 200 de {year} guardado: el resultado contable del diario es {result}"
        )),
    }
    let mismatches = rows
        .iter()
        .filter(|r| r.theirs.is_some() && r.diff.cents().abs() > 1)
        .count();
    if mismatches > 0 {
        warnings.push(
            "las diferencias del 303 suelen ser IVA no deducible, prorrata o facturas de otro \
             periodo; las del 200, ajustes extracontables: pregúntale al gestor"
                .into(),
        );
    }
    Ok(ModelsComparison {
        fiscal_year: year,
        rows,
        mismatches,
        warnings,
    })
}

// ---------------- Inventario de activos del gestor ----------------

/// Inventario de inmovilizado leído, listo para el asistente de saldos iniciales.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InventoryRead {
    pub assets: Vec<Asset>,
    pub warnings: Vec<String>,
    /// El periodo que dice la cabecera del listado: lo ya amortizado llega hasta su final.
    #[serde(skip)]
    period: Option<HeaderPeriod>,
}

/// El periodo de un listado según su cabecera.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeaderPeriod {
    /// «Período: de 01/07/2024 a 30/06/2025»: su último día.
    End(Date),
    /// «Ejercicio 2024/2025»: el ejercicio que empieza en 2024.
    Years(i32),
    /// «Ejercicio 2025».
    Year(i32),
}

/// El periodo que dice la cabecera de un listado (las filas de encima de los títulos): en la
/// fila que habla del periodo o del ejercicio, la última fecha completa o, si no la hay, los años.
fn header_period(rows: &[Vec<String>]) -> Option<HeaderPeriod> {
    rows.iter().take(12).find_map(|fila| {
        let texto = fila.join(" ").replace(" / ", "/").replace(" - ", "-");
        let n = norm(&texto);
        if !(n.contains("periodo") || n.contains("ejercicio")) {
            return None;
        }
        let tokens: Vec<&str> = texto
            .split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | ':' | '(' | ')'))
            .filter(|t| !t.is_empty())
            .collect();
        if let Some(d) = tokens
            .iter()
            .filter(|t| t.matches(['/', '-', '.']).count() == 2)
            .filter_map(|t| crate::import::parse_date(t))
            .next_back()
        {
            return Some(HeaderPeriod::End(d));
        }
        let year = |t: &str| t.parse::<i32>().ok().filter(|y| (1900..2100).contains(y));
        for t in &tokens {
            if let Some((a, b)) = t.split_once(['/', '-'])
                && let (Some(a), Some(b)) = (year(a), year(b))
                && b == a + 1
            {
                return Some(HeaderPeriod::Years(a));
            }
        }
        tokens
            .iter()
            .filter_map(|t| year(t))
            .next_back()
            .map(HeaderPeriod::Year)
    })
}

#[derive(Default)]
struct InventoryCols {
    code: Option<usize>,
    name: Option<usize>,
    extra: Option<usize>,
    cost: Option<usize>,
    book: Option<usize>,
    date: Option<usize>,
    account: Option<usize>,
}

/// Columnas del «Inventario de Activos» de a3: `Código de Activo | Descripción | Ampliación
/// Activo | Valor Adquisición | Valor contable | Fecha Ini Amort.`
fn inventory_cols(row: &[String]) -> InventoryCols {
    let mut c = InventoryCols::default();
    for (i, h) in row.iter().enumerate() {
        let n = norm(h);
        let slot = match n.as_str() {
            "codigodeactivo" | "codigoactivo" | "codigo" | "activo" | "nactivo" => &mut c.code,
            "descripcion" | "nombre" | "elemento" | "bien" | "denominacion" => &mut c.name,
            "ampliacionactivo" | "ampliacion" | "observaciones" => &mut c.extra,
            "valoradquisicion" | "valordeadquisicion" | "precioadquisicion" | "coste"
            | "importe" | "valorcompra" | "preciocompra" => &mut c.cost,
            "valorcontable" | "valorneto" | "valornetocontable" | "vnc" | "pendiente"
            | "pendienteamortizar" => &mut c.book,
            "fechainiamort"
            | "fechainicioamort"
            | "fechainicioamortizacion"
            | "fechaamort"
            | "fechaamortizacion"
            | "fechaadquisicion"
            | "fechacompra"
            | "fechaalta"
            | "fecha" => &mut c.date,
            "cuenta" | "cuentainmovilizado" | "ctainmov" | "cuentacontable" => &mut c.account,
            _ => continue,
        };
        if slot.is_none() {
            *slot = Some(i);
        }
    }
    c
}

/// Coeficiente lineal máximo que propone TaxPain según la cuenta (tabla del art. 12.1.a LIS,
/// `fixtures/accounting/amortizacion.json`); `None` si la cuenta no dice de qué es el bien.
pub fn proposed_rate(account: &str) -> Option<Rate> {
    let bp = match account.get(..3)? {
        "206" => 3300,
        "211" => 200,
        "212" | "215" => 1000,
        "213" => 1200,
        "214" => 2500,
        "216" => 1000,
        "217" => 2500,
        "218" => 1600,
        _ => return None,
    };
    Some(Rate::from_bp(bp))
}

/// Cuentas de amortización acumulada (28x) y de dotación (68x) que van con la del bien.
fn companions(account: &str) -> (String, String) {
    let mid = account.get(1..3).unwrap_or("17");
    let group = account.get(1..2).unwrap_or("1");
    (format!("28{mid}"), format!("68{group}"))
}

/// Lee el inventario de inmovilizado del gestor (a3 «Inventario de Activos», o un listado con
/// columnas parecidas) para el asistente de saldos iniciales. Las filas «Total Cta. Inmov.»
/// llevan la cuenta de las filas de encima. Cada bien sale con lo ya amortizado (adquisición −
/// valor contable) y el coeficiente que propone TaxPain por su cuenta (a elegir por el usuario; 0
/// si no se sabe). Los terrenos no piden coeficiente: no se amortizan.
pub fn read_inventory(filename: &str, bytes: &[u8]) -> Result<InventoryRead> {
    let mut r = read_inventory_rows(filename, bytes)?;
    for a in r.assets.iter().filter(|a| is_land(&a.account)) {
        r.warnings.push(format!(
            "«{}»: es un terreno, que no se amortiza (PGC, norma de registro y valoración 3.ª a): \
             quítalo de la lista",
            a.name
        ));
    }
    let unknown = r
        .assets
        .iter()
        .filter(|a| a.rate.bp() == 0 && !is_land(&a.account))
        .count();
    if unknown > 0 {
        r.warnings.push(unknown_rate_warning(unknown));
    }
    r.warnings.push(
        "lo ya amortizado es adquisición − valor contable del listado; el coeficiente es una \
         propuesta por la cuenta (art. 12.1.a LIS): revísalo con el plan del gestor"
            .into(),
    );
    Ok(r)
}

/// Aviso de los bienes sin coeficiente propuesto.
fn unknown_rate_warning(n: usize) -> String {
    format!(
        "{n} bien(es) sin coeficiente propuesto (la cuenta no dice de qué son): elige el \
         porcentaje anual de la tabla de amortización"
    )
}

/// El inventario leído, con los avisos de sus filas.
fn read_inventory_rows(filename: &str, bytes: &[u8]) -> Result<InventoryRead> {
    let hojas = crate::import::sheets(filename, bytes)?;
    for h in &hojas {
        if let Some(r) = parse_inventory(&h.filas) {
            return Ok(r);
        }
    }
    Err(other(
        "no lo reconozco como inventario de activos: hacen falta las columnas Descripción, Valor \
         de adquisición, Valor contable y Fecha de inicio de amortización",
    ))
}

fn parse_inventory(rows: &[Vec<String>]) -> Option<InventoryRead> {
    let (head, cols) = rows
        .iter()
        .enumerate()
        .take(25)
        .map(|(i, r)| (i, inventory_cols(r)))
        .find(|(_, c)| {
            c.name.is_some() && c.cost.is_some() && c.book.is_some() && c.date.is_some()
        })?;
    let año = año_de_cabecera(&rows[..head]);
    let period = header_period(&rows[..head]);
    let cell = |r: &[String], c: Option<usize>| -> String {
        c.and_then(|i| r.get(i))
            .map(|s| s.trim().to_owned())
            .unwrap_or_default()
    };
    let mut assets: Vec<Asset> = Vec::new();
    let mut warnings = Vec::new();
    // Bienes a la espera de su fila de total con la cuenta.
    let mut pending: Vec<usize> = Vec::new();
    for (i, r) in rows.iter().enumerate().skip(head + 1) {
        let fila = i + 1;
        if r.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        let total = r.iter().any(|c| {
            let n = norm(c);
            n.starts_with("totalcta") || n.starts_with("totalcuenta") || n.starts_with("totalinmov")
        });
        if total {
            let account = r
                .iter()
                .map(|c| c.trim())
                .find(|c| c.len() >= 3 && c.bytes().all(|b| b.is_ascii_digit()))
                .and_then(|c| AccountCode::new(c).ok());
            match account {
                Some(a) => {
                    let (dep, exp) = companions(a.as_str());
                    let rate = proposed_rate(a.as_str());
                    for j in pending.drain(..) {
                        let asset = &mut assets[j];
                        asset.account = a.as_str().to_owned();
                        asset.depreciation_account = dep.clone();
                        asset.expense_account = exp.clone();
                        if let Some(r) = rate {
                            asset.rate = r;
                        }
                    }
                }
                None => warnings.push(format!(
                    "fila {fila}: la fila de total no dice la cuenta; pon la cuenta de los bienes \
                     de encima a mano"
                )),
            }
            continue;
        }
        let name = cell(r, cols.name);
        let cost_s = cell(r, cols.cost);
        if name.is_empty() || cost_s.is_empty() {
            continue;
        }
        let Some(cost) = parse_amount(&cost_s) else {
            warnings.push(format!(
                "fila {fila}: el valor de adquisición no se entiende; no entra"
            ));
            continue;
        };
        if cost.cents() <= 0 {
            continue;
        }
        let book = parse_amount(&cell(r, cols.book)).unwrap_or(Money::ZERO);
        let mut accumulated = cost - book;
        if accumulated.is_negative() || book.is_negative() {
            warnings.push(format!(
                "fila {fila}: «{name}» tiene un valor contable mayor que el de adquisición; lo ya \
                 amortizado se deja a cero"
            ));
            accumulated = Money::ZERO;
        }
        let Some(date) = parse_date_in(&cell(r, cols.date), año) else {
            warnings.push(format!(
                "fila {fila}: «{name}» no tiene fecha de inicio de amortización; no entra"
            ));
            continue;
        };
        let extra = cell(r, cols.extra);
        let code = cell(r, cols.code);
        let mut full = name.clone();
        if !extra.is_empty() {
            full = format!("{full} {extra}");
        }
        if !code.is_empty() {
            full = format!("{full} ({code})");
        }
        let account = cell(r, cols.account);
        let account = AccountCode::new(&account)
            .ok()
            .map(|a| a.as_str().to_owned())
            .unwrap_or_default();
        let (dep, exp) = companions(if account.is_empty() { "217" } else { &account });
        assets.push(Asset {
            name: full.trim().to_owned(),
            depreciation_account: if account.is_empty() {
                String::new()
            } else {
                dep
            },
            expense_account: if account.is_empty() {
                String::new()
            } else {
                exp
            },
            rate: if account.is_empty() {
                Rate::from_bp(0)
            } else {
                proposed_rate(&account).unwrap_or(Rate::from_bp(0))
            },
            account,
            acquired_on: date,
            cost,
            method: Default::default(),
            accumulated,
        });
        if assets.last().is_some_and(|a| a.account.is_empty()) {
            pending.push(assets.len() - 1);
        }
    }
    if assets.is_empty() {
        return None;
    }
    if !pending.is_empty() {
        warnings.push(format!(
            "{} bien(es) sin fila de total con la cuenta: ponles la cuenta (217, 216, 218…) a \
             mano",
            pending.len()
        ));
    }
    Some(InventoryRead {
        assets,
        warnings,
        period,
    })
}

// ---------------- El inventario del gestor en Inmovilizado ----------------

/// Un bien del inventario del gestor, para darlo de alta en Inmovilizado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InventoryItem {
    pub asset: Asset,
    /// Ya está en Inmovilizado (con la misma fecha y el mismo coste, y el mismo nombre o la misma
    /// cuenta): no se da de alta otra vez.
    pub existing: Option<i64>,
    /// Por qué no se da de alta (un terreno, que no se amortiza; un bien sin cuenta).
    pub skip: Option<String>,
}

/// El inventario leído, con lo que ya está en Inmovilizado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InventoryPreview {
    pub items: Vec<InventoryItem>,
    /// Hasta qué día llega lo ya amortizado: el cierre del ejercicio del listado, si lo dice.
    pub until: Option<Date>,
    pub warnings: Vec<String>,
}

/// Lo que se ha hecho al dar de alta el inventario.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct InventoryImported {
    pub added: usize,
    /// Ya estaban en Inmovilizado.
    pub existing: usize,
    /// No entran (terrenos, bienes sin cuenta o sin coeficiente).
    pub skipped: usize,
    pub warnings: Vec<String>,
}

/// ¿Es un terreno (cuentas 210 y 220 del PGC)? Los terrenos no se amortizan: «normalmente los
/// terrenos tienen una vida ilimitada» (PGC, norma de registro y valoración 3.ª a).
fn is_land(account: &str) -> bool {
    account.starts_with("210") || account.starts_with("220")
}

/// Por qué un bien del inventario no se da de alta, si no entra.
fn inventory_skip(a: &Asset) -> Option<String> {
    if is_land(&a.account) {
        Some(
            "es un terreno: no se amortiza (PGC, norma de registro y valoración 3.ª a), así que \
             no va en Inmovilizado"
                .into(),
        )
    } else if a.account.is_empty() {
        Some("el listado no dice su cuenta: dalo de alta a mano".into())
    } else {
        None
    }
}

/// Lo que ya está en Inmovilizado de cada bien del listado (`None` en los que no entran): misma
/// fecha y mismo coste, y mismo nombre o la misma cuenta del PGC (la de tres cifras: el que se dio
/// de alta a mano va a la 217 y el del listado, a su subcuenta). Cada elemento casa con un solo
/// bien, en orden: dos bienes iguales del listado (dos portátiles del mismo día y precio) son dos.
fn match_existing(current: &[FixedAsset], assets: &[Asset]) -> Vec<Option<i64>> {
    let mut used = BTreeSet::new();
    assets
        .iter()
        .map(|a| {
            if inventory_skip(a).is_some() {
                return None;
            }
            let group = a.account.get(..3).unwrap_or(&a.account);
            let f = current.iter().find(|f| {
                !used.contains(&f.id)
                    && f.acquired_on == a.acquired_on
                    && f.cost == a.cost
                    && (f.name.trim().eq_ignore_ascii_case(a.name.trim())
                        || (!group.is_empty() && f.account.as_str().starts_with(group)))
            })?;
            used.insert(f.id);
            Some(f.id)
        })
        .collect()
}

/// «10», «12,5», «6,67»: un coeficiente en puntos básicos, en tanto por ciento.
fn pct(bp: i64) -> String {
    let s = format!("{},{:02}", bp / 100, bp % 100);
    s.trim_end_matches('0').trim_end_matches(',').to_owned()
}

/// Coeficiente anual, en puntos básicos, que da lo ya amortizado de un bien hasta `until`: lo
/// amortizado ÷ coste ÷ años desde que empezó a amortizarse. `None` si no está a medio amortizar
/// o si empezó hace menos de tres meses (con tan pocos días, su redondeo pesa demasiado).
fn implied_bp(a: &Asset, until: Date) -> Option<i64> {
    if a.accumulated.cents() <= 0 || a.accumulated >= a.cost || a.acquired_on >= until {
        return None;
    }
    let days = (until - a.acquired_on).whole_days() + 1;
    if days < 90 {
        return None;
    }
    // amortizado / coste / (días / 365,25) × 10.000
    let num = i128::from(a.accumulated.cents()) * 3_652_500;
    let den = i128::from(a.cost.cents()) * i128::from(days);
    i64::try_from((num + den / 2) / den).ok()
}

/// El coeficiente de tabla más cercano a `bp` (un tanto por ciento entero o el de un número entero
/// de años: 12,5 %, 6,67 %…), si está a menos de un 2 % de él.
fn table_rate(bp: i64) -> Option<i64> {
    let best = (1..=100)
        .map(|p| p * 100)
        .chain((1..=100).map(|n| (10_000 + n / 2) / n))
        .min_by_key(|c| (c - bp).abs())?;
    ((best - bp).abs() * 50 <= bp).then_some(best)
}

/// ¿Ya está esta cuenta en TaxPain (con nombre propio o con apuntes)?
fn known_account(conn: &Connection, code: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM account WHERE code = ?1)
             OR EXISTS(SELECT 1 FROM journal_line WHERE account = ?1)",
        [code],
        |r| r.get(0),
    )?)
}

/// ¿Lleva TaxPain subcuentas de `prefix` (por ejemplo, de la 2817)?
fn has_subaccounts(conn: &Connection, prefix: &str) -> Result<bool> {
    let like = format!("{prefix}_%");
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM account WHERE code LIKE ?1)
             OR EXISTS(SELECT 1 FROM journal_line WHERE account LIKE ?1)",
        [like],
        |r| r.get(0),
    )?)
}

/// Las subcuentas del gestor para la amortización acumulada y el gasto de un bien, si ya están en
/// TaxPain (las trajo su diario o la apertura): la del bien («28170003» y «68170003» para el
/// «21700003») o la genérica («28170000», «68100000»). Así la amortización de TaxPain sigue en la
/// subcuenta en la que está lo ya amortizado, y una baja o una venta la salda. Si el gestor lleva
/// subcuentas de la 2817 pero no hay ninguna de ésas, se dice.
fn gestor_accounts(conn: &Connection, a: &mut Asset) -> Result<Option<String>> {
    let n = a.account.len();
    let (Some(g1), Some(g2), Some(tail)) =
        (a.account.get(1..2), a.account.get(1..3), a.account.get(4..))
    else {
        return Ok(None);
    };
    if n < 5 || !a.account.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(None);
    }
    let zeros = |k: usize| "0".repeat(n - k);
    let first = |codes: &[String]| -> Result<Option<String>> {
        for c in codes {
            if known_account(conn, c)? {
                return Ok(Some(c.clone()));
            }
        }
        Ok(None)
    };
    let dep = [format!("28{g2}{tail}"), format!("28{g2}{}", zeros(4))];
    let exp = [
        format!("68{g2}{tail}"),
        format!("68{g1}{}", zeros(3)),
        format!("68{g2}{}", zeros(4)),
    ];
    let mut warning = None;
    match first(&dep)? {
        Some(d) => a.depreciation_account = d,
        None if has_subaccounts(conn, &format!("28{g2}"))? => {
            warning = Some(format!(
                "«{}»: el gestor lleva la amortización acumulada en subcuentas de la 28{g2}, pero \
                 en TaxPain no está la {} ni la {}: la de TaxPain irá a la {}; si el gestor la \
                 lleva en otra, dalo de alta a mano con ella",
                a.name, dep[0], dep[1], a.depreciation_account
            ));
        }
        None => {}
    }
    if let Some(e) = first(&exp)? {
        a.expense_account = e;
    }
    Ok(warning)
}

/// Lee el inventario de activos del gestor para darlo de alta en Inmovilizado (no cambia nada):
/// dice qué bienes ya están y cuáles no entran, hasta qué día llega lo ya amortizado (el final
/// del periodo o del ejercicio del listado), el coeficiente de cada bien (el que da lo ya
/// amortizado, que es el plan del gestor, o si no se sabe el que propone su cuenta) y, si su
/// diario ya está en TaxPain, sus subcuentas de amortización.
pub fn preview_inventory(v: &Vault, filename: &str, bytes: &[u8]) -> Result<InventoryPreview> {
    let r = read_inventory_rows(filename, bytes)?;
    let conn = v.connection();
    let mut warnings = r.warnings;
    let until = match r.period {
        Some(HeaderPeriod::End(d)) => Some(d),
        Some(HeaderPeriod::Years(y)) => Some(accounting::fiscal_year(v, y)?.end),
        Some(HeaderPeriod::Year(y)) => {
            let fy = accounting::fiscal_year(v, y)?;
            if !fy.is_natural() {
                warnings.push(format!(
                    "el listado sólo dice el año {y} y el ejercicio de la empresa no es el año \
                     natural: se toma el que empieza en {y}, hasta el {}; comprueba «Amortizado \
                     hasta el»",
                    dmy(fy.end)
                ));
            }
            Some(fy.end)
        }
        None => None,
    };
    let existing = match_existing(&accounting::fixed_assets(v)?, &r.assets);
    let mut items = Vec::new();
    for (mut a, existing) in r.assets.into_iter().zip(existing) {
        let skip = inventory_skip(&a);
        if skip.is_none() && existing.is_none() {
            if let Some(w) = gestor_accounts(conn, &mut a)? {
                warnings.push(w);
            }
            if let Some(bp) = until.and_then(|u| implied_bp(&a, u)) {
                let proposed = i64::from(a.rate.bp());
                match table_rate(bp) {
                    Some(t) if t != proposed => {
                        warnings.push(format!(
                            "«{}»: lo ya amortizado desde el {} da un {} % anual (el plan del \
                             gestor), no el {} % que propone su cuenta: se deja el {} %",
                            a.name,
                            dmy(a.acquired_on),
                            pct(t),
                            pct(proposed),
                            pct(t)
                        ));
                        a.rate = Rate::from_bp(i32::try_from(t).unwrap_or(0));
                    }
                    Some(_) => {}
                    None => warnings.push(format!(
                        "«{}»: lo ya amortizado desde el {} da un {} % anual, que no es un \
                         coeficiente de tabla: se propone el {} % de su cuenta; revísalo con el \
                         plan del gestor",
                        a.name,
                        dmy(a.acquired_on),
                        pct(bp),
                        pct(proposed)
                    )),
                }
            }
        }
        items.push(InventoryItem {
            asset: a,
            existing,
            skip,
        });
    }
    let unknown = items
        .iter()
        .filter(|i| i.existing.is_none() && i.skip.is_none() && i.asset.rate.bp() == 0)
        .count();
    if unknown > 0 {
        warnings.push(unknown_rate_warning(unknown));
    }
    warnings.push(
        "lo ya amortizado es adquisición − valor contable del listado; el coeficiente es el que da \
         lo ya amortizado o, si no se sabe (bienes amortizados del todo o desde hace menos de tres \
         meses), el que propone su cuenta (art. 12.1.a LIS): revísalo con el plan del gestor"
            .into(),
    );
    Ok(InventoryPreview {
        items,
        until,
        warnings,
    })
}

/// Da de alta en Inmovilizado los bienes del inventario del gestor con lo que ya se amortizó
/// hasta `until` (normalmente el cierre del ejercicio del listado): TaxPain sigue amortizando
/// desde el día siguiente y nunca pasa del coste. No contabiliza nada: los saldos de esos bienes
/// ya están en el diario del gestor o en la apertura, y otra apertura los contaría dos veces.
/// Los que ya están en Inmovilizado no se repiten; los terrenos y los bienes sin cuenta o sin
/// coeficiente no entran.
pub fn import_inventory(
    v: &Vault,
    assets: &[Asset],
    until: Date,
    now: OffsetDateTime,
) -> Result<InventoryImported> {
    let mut out = InventoryImported::default();
    let existing = match_existing(&accounting::fixed_assets(v)?, assets);
    for (a, existing) in assets.iter().zip(existing) {
        if let Some(why) = inventory_skip(a) {
            out.skipped += 1;
            out.warnings.push(format!("«{}»: {why}", a.name));
            continue;
        }
        if existing.is_some() {
            out.existing += 1;
            continue;
        }
        if a.rate.bp() <= 0 {
            out.skipped += 1;
            out.warnings.push(format!(
                "«{}»: sin coeficiente de amortización; dalo de alta a mano con el suyo",
                a.name
            ));
            continue;
        }
        let code = |s: &str| {
            AccountCode::new(s).map_err(|_| other(format!("la cuenta «{s}» no es válida")))
        };
        let asset = FixedAsset {
            id: 0,
            name: a.name.trim().to_owned(),
            account: code(&a.account)?,
            depreciation_account: code(&a.depreciation_account)?,
            expense_account: code(&a.expense_account)?,
            acquired_on: a.acquired_on,
            cost: a.cost,
            residual: Money::ZERO,
            rate: a.rate,
            disposed_on: None,
            method: a.method,
            tax_rate: None,
        };
        // Comprado después del listado: no hay nada amortizado antes.
        let (prior, to) = if a.acquired_on > until {
            (Money::ZERO, None)
        } else {
            (a.accumulated.min(a.cost), Some(until))
        };
        match apertura::add_asset_with_prior(v, &asset, prior, to, now) {
            Ok(_) => out.added += 1,
            Err(e) => {
                out.skipped += 1;
                out.warnings.push(format!("«{}»: {e}", a.name));
            }
        }
    }
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        "asset.inventory",
        &until.to_string(),
        &serde_json::json!({
            "added": out.added,
            "existing": out.existing,
            "skipped": out.skipped,
        }),
    )?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coeficiente_y_cuentas_companeras() {
        assert_eq!(proposed_rate("21700001").map(|r| r.bp()), Some(2500));
        assert_eq!(proposed_rate("210"), None);
        assert_eq!(
            companions("21700001"),
            ("2817".to_owned(), "681".to_owned())
        );
        assert_eq!(companions("206"), ("2806".to_owned(), "680".to_owned()));
    }

    #[test]
    fn diario_resumido_por_fecha() {
        let csv = b"Fecha;Importe Debe;Cuenta Debe;Descripcion Cuenta;Cuenta Haber;Importe Haber\n\
                    31/01/2025;1210,00;43000001;CLIENTE;;\n\
                    ;;;VENTAS;70500000;1000,00\n\
                    ;;;IVA REP;47700000;210,00\n\
                    28/02/2025;1210,00;57200001;BANCO;;\n\
                    ;;;CLIENTE;43000001;1210,00\n";
        let p = read("resumido.csv", csv).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(p.source, "resumido");
        assert_eq!(p.entries.len(), 2);
        assert_eq!(p.entries[0].lines.len(), 3);
        assert!(p.entries[0].validate().is_ok());
        assert!(p.warnings.iter().any(|w| w.contains("resumido")));
    }

    #[test]
    fn resumido_descuadrado() {
        let csv = b"Fecha;Importe Debe;Cuenta Debe;Descripcion Cuenta;Cuenta Haber;Importe Haber\n\
                    31/01/2025;1210,00;43000001;CLIENTE;;\n\
                    ;;;VENTAS;70500000;1000,00\n";
        let e = read("resumido.csv", csv).err().map(|e| e.to_string());
        assert!(
            e.as_deref().is_some_and(|e| e.contains("no cuadra")),
            "{e:?}"
        );
    }
}
