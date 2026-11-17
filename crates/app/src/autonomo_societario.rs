//! Autónomo societario (administrador o socio con control que cotiza en el RETA, art. 305.2.b y
//! e TRLGSS) y recibo de la retribución del administrador.
//!
//! - Ficha anual del RETA (`reta_ficha`): base elegida, rendimiento neto previsto comunicado a la
//!   TGSS, meses de alta, cómputo de los dividendos, otros rendimientos y gastos, quién paga la
//!   cuota y con qué cuentas, y el soporte societario de la retribución (estatutos y acuerdo de
//!   la junta, art. 217 LSC).
//! - Rendimiento neto computable del año con lo que paga la sociedad (nóminas del administrador,
//!   cuotas que paga ella y dividendos que cobra como socio) y lo que añada el usuario; tramo
//!   resultante frente a la base elegida y regularización estimada de la TGSS al año siguiente.
//!   El cálculo está en `taxpain_core::reta` (tablas de 2025 y 2026 cotejadas con el BOE; la
//!   de 2027 es la de 2026 prorrogada y lo dice).
//! - Cuotas pagadas (`reta_cuota`) con su asiento si las paga la sociedad.
//! - Recibo de la retribución del administrador en PDF.
//!
//! Cuota pagada por la sociedad: dos tratamientos (el usuario elige en la ficha).
//! 1. **La sociedad paga la cuota (por defecto)**. Es retribución del administrador por su
//!    cargo: gasto deducible de la sociedad si el cargo es retribuido según los estatutos
//!    (art. 15.e LIS; art. 217 LSC) y, para él, rendimiento del trabajo (art. 17.2.e LIRPF) que
//!    a la vez es gasto deducible de esos rendimientos (art. 19.2.a LIRPF). Se trata como
//!    retribución en especie (art. 42.1 LIRPF), con su ingreso a cuenta al 19 o 35 % (arts. 43
//!    LIRPF y 102 RIRPF). Asiento: cuenta de gasto de personal (642 por defecto, como la
//!    Seguridad Social a cargo de la empresa; hay quien usa la 649 o la 640) contra tesorería.
//!    `TODO(VERIFY)`: la DGT ha calificado también el pago de una deuda del perceptor como renta
//!    dineraria (sumada al bruto, con retención); con ese criterio, elige la opción 2.
//! 2. **El administrador la paga y la sociedad se la reembolsa**: el reembolso es retribución
//!    dineraria y va en el bruto de su nómina (con la retención del 19 o 35 %); aquí sólo se
//!    anota la cuota para el cálculo, sin asiento.
//!
//! Fuera de alcance: llevar la especie y su ingreso a cuenta al 111 (casillas 04 a 06) y al 190
//! (percepciones en especie): hoy hay que añadirlos al revisar los borradores (se avisa).
//!
//! Punto de extensión para el libro de actas (G08, en otra rama): [`acuerdo_retribucion`]
//! devuelve hoy lo que el usuario anota en la ficha; cuando exista el libro de actas, debe
//! buscar primero el acta que fija la retribución de los administradores y caer en la ficha si
//! no hay ninguna.

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::{AccountCode, EntryLine, JournalEntry, Origin};
use taxpain_core::money::{Money, Rate};
use taxpain_core::reta::{
    self, EntradaRendimiento, Regularizacion, Rendimiento, TablaAnual, Tramo, VentanaCambio,
};
use taxpain_models::informative::{DirectorSs, director_rate};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::format_description::well_known::Rfc3339;
use time::{Date, Month, OffsetDateTime};

use crate::accounting;
use crate::error::{AppError, Result};
use crate::payroll::{self, Employee};

/// Quién paga la cuota del RETA.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Pagador {
    /// La sociedad (retribución del administrador: asiento a la cuenta de gasto).
    #[default]
    Sociedad,
    /// El administrador (si la sociedad se la reembolsa, va en el bruto de la nómina).
    Administrador,
}

impl Pagador {
    fn as_str(self) -> &'static str {
        match self {
            Pagador::Sociedad => "Sociedad",
            Pagador::Administrador => "Administrador",
        }
    }

    fn of(s: &str) -> Self {
        if s == "Administrador" {
            Pagador::Administrador
        } else {
            Pagador::Sociedad
        }
    }
}

/// Ficha del RETA de un administrador para un año.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FichaReta {
    pub employee_id: i64,
    pub year: i32,
    /// Base de cotización mensual elegida.
    pub base_elegida: Money,
    /// Rendimiento neto mensual previsto que se comunicó a la TGSS (decide el tramo).
    #[serde(default)]
    pub rendimiento_previsto: Option<Money>,
    /// Meses de alta en el año.
    pub meses_alta: u8,
    /// Fecha de alta en el RETA como societario (`None`: de años anteriores). Con menos de
    /// noventa días de alta como societario en el año no hay deducción del 3 % ni base mínima de
    /// los societarios (art. 308.1.a, regla 4.ª, y c, regla 2.ª, TRLGSS; DT 7.ª RDL 13/2022).
    #[serde(default)]
    pub alta_societario: Option<Date>,
    /// Socio con control efectivo de la sociedad: sus dividendos computan.
    pub control_efectivo: bool,
    /// Parte de los dividendos que computa: 100 % con al menos el 33 % del capital, o el 25 %
    /// siendo administrador (art. 308.1.c, regla 1.ª, TRLGSS).
    pub pct_dividendos: Rate,
    /// Rendimientos netos de fuera de la sociedad (otras actividades) del año.
    #[serde(default)]
    pub otros_rendimientos: Money,
    /// Gastos deducibles de las retribuciones, sin la cuota del RETA.
    #[serde(default)]
    pub gastos_deducibles: Money,
    pub pagador: Pagador,
    /// Cuenta de gasto de la cuota que paga la sociedad (642 por defecto).
    pub cuenta_gasto: String,
    /// Tesorería con la que se paga (572 o su subcuenta).
    pub cuenta_tesoreria: String,
    /// Los estatutos dicen que el cargo es retribuido (art. 217.1 LSC).
    #[serde(default)]
    pub estatutos_retribuido: bool,
    /// Fecha del acuerdo de la junta que fija el importe máximo anual (art. 217.3 LSC).
    #[serde(default)]
    pub acuerdo_fecha: Option<Date>,
    /// Importe máximo anual aprobado por la junta.
    #[serde(default)]
    pub acuerdo_importe: Option<Money>,
    #[serde(default)]
    pub acuerdo_nota: String,
    /// Guardada (si no, es la propuesta: la del año anterior o la de partida).
    #[serde(default)]
    pub guardada: bool,
}

impl FichaReta {
    fn nueva(employee_id: i64, year: i32) -> Self {
        FichaReta {
            employee_id,
            year,
            base_elegida: reta::tabla(year).map_or(Money::ZERO, |t| t.base_min_societarios),
            rendimiento_previsto: None,
            meses_alta: 12,
            alta_societario: None,
            control_efectivo: true,
            pct_dividendos: Rate::percent(100),
            otros_rendimientos: Money::ZERO,
            gastos_deducibles: Money::ZERO,
            pagador: Pagador::Sociedad,
            cuenta_gasto: "642".into(),
            cuenta_tesoreria: "572".into(),
            estatutos_retribuido: false,
            acuerdo_fecha: None,
            acuerdo_importe: None,
            acuerdo_nota: String::new(),
            guardada: false,
        }
    }
}

/// Acuerdo de la junta sobre la retribución del cargo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AcuerdoRetribucion {
    pub estatutos_retribuido: bool,
    pub fecha: Option<Date>,
    pub importe_maximo: Option<Money>,
    /// De dónde sale («ficha del RETA»; cuando exista, «libro de actas»).
    pub fuente: &'static str,
}

/// Una cuota del RETA pagada.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CuotaReta {
    pub id: i64,
    pub employee_id: i64,
    pub date: Date,
    pub amount: Money,
    pub pagador: Pagador,
    pub entry_id: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CuotaInput {
    pub employee_id: i64,
    pub date: Date,
    pub amount: Money,
    pub pagador: Pagador,
    /// Contabilizarla desde aquí (sólo si la paga la sociedad). No, si el cargo del banco ya se
    /// contabiliza con una regla.
    pub asiento: bool,
}

/// Resumen del año para la pantalla.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ResumenReta {
    pub employee_id: i64,
    pub name: String,
    pub nif: String,
    pub ficha: FichaReta,
    /// Tabla del año (con lo que falta por cotejar); `None` si el año no tiene tabla.
    pub tabla: Option<TablaAnual>,
    /// Nóminas del año (bruto).
    pub dinerarias: Money,
    /// Cuotas del RETA pagadas por la sociedad (especie).
    pub especie: Money,
    /// Dividendos íntegros cobrados de la sociedad.
    pub dividendos: Money,
    pub rendimiento: Rendimiento,
    /// Días de alta como societario en el año (de la fecha de alta de la ficha).
    pub dias_alta_societario: u16,
    /// Llega a los noventa días: deducción del 3 % y base mínima de los societarios.
    pub noventa_dias: bool,
    /// Tramo del rendimiento previsto (el que se comunicó a la TGSS).
    pub tramo_previsto: Option<Tramo>,
    /// Tramo y regularización con los rendimientos que salen de TaxPain.
    pub regularizacion: Option<Regularizacion>,
    /// Base más baja con la que no habría regularización (la mínima del tramo real).
    pub base_recomendada: Option<Money>,
    pub cuota_mensual: Money,
    pub cuotas: Vec<CuotaReta>,
    pub cuotas_total: Money,
    /// Tipo de retención del administrador (19 o 35 %), si se sabe.
    pub tipo_retencion: Option<Rate>,
    /// Ingreso a cuenta de la especie.
    pub ingreso_a_cuenta: Money,
    /// Retribución total del año (dineraria y en especie) frente al acuerdo de la junta.
    pub retribucion_total: Money,
    pub acuerdo: AcuerdoRetribucion,
    pub ventana_abierta: VentanaCambio,
    pub proximas_ventanas: Vec<VentanaCambio>,
    pub avisos: Vec<String>,
}

fn other(e: impl ToString) -> AppError {
    AppError::Other(e.to_string())
}

fn now_str(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

fn parse_date(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]")).map_err(other)
}

fn year_bounds(year: i32) -> Result<(Date, Date)> {
    Ok((
        Date::from_calendar_date(year, Month::January, 1).map_err(other)?,
        Date::from_calendar_date(year, Month::December, 31).map_err(other)?,
    ))
}

/// El empleado tiene que ser administrador en el RETA.
fn administrador_reta(v: &Vault, employee_id: i64) -> Result<Employee> {
    let e = payroll::employee(v.connection(), employee_id)?;
    if !e.administrator || e.ss_regime != DirectorSs::Autonomo {
        return Err(other(
            "sólo para un administrador o socio con «Autónomos (RETA)» en su ficha de Nóminas",
        ));
    }
    Ok(e)
}

const FICHA_COLS: &str = "employee_id, year, base_elegida, rendimiento_previsto, meses_alta, \
     control_efectivo, pct_dividendos_bp, otros_rendimientos, gastos_deducibles, pagador, \
     cuenta_gasto, cuenta_tesoreria, estatutos_retribuido, acuerdo_fecha, acuerdo_importe, \
     acuerdo_nota, alta_societario";

fn ficha_guardada(
    v: &Vault,
    employee_id: i64,
    year: i32,
    exact: bool,
) -> Result<Option<FichaReta>> {
    let sql = if exact {
        format!("SELECT {FICHA_COLS} FROM reta_ficha WHERE employee_id = ?1 AND year = ?2")
    } else {
        format!(
            "SELECT {FICHA_COLS} FROM reta_ficha WHERE employee_id = ?1 AND year < ?2
             ORDER BY year DESC LIMIT 1"
        )
    };
    let row = v
        .connection()
        .query_row(&sql, params![employee_id, year], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i32>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, u8>(4)?,
                r.get::<_, bool>(5)?,
                r.get::<_, i32>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?,
                r.get::<_, String>(9)?,
                r.get::<_, String>(10)?,
                r.get::<_, String>(11)?,
                r.get::<_, bool>(12)?,
                r.get::<_, Option<String>>(13)?,
                r.get::<_, Option<i64>>(14)?,
                r.get::<_, String>(15)?,
                r.get::<_, Option<String>>(16)?,
            ))
        })
        .optional()?;
    let Some((
        id,
        y,
        base,
        prev,
        meses,
        control,
        pct,
        otros,
        gastos,
        pag,
        cg,
        ct,
        est,
        af,
        ai,
        an,
        alta,
    )) = row
    else {
        return Ok(None);
    };
    Ok(Some(FichaReta {
        employee_id: id,
        year: y,
        base_elegida: Money::from_cents(base),
        rendimiento_previsto: prev.map(Money::from_cents),
        meses_alta: meses,
        alta_societario: alta.as_deref().map(parse_date).transpose()?,
        control_efectivo: control,
        pct_dividendos: Rate::from_bp(pct),
        otros_rendimientos: Money::from_cents(otros),
        gastos_deducibles: Money::from_cents(gastos),
        pagador: Pagador::of(&pag),
        cuenta_gasto: cg,
        cuenta_tesoreria: ct,
        estatutos_retribuido: est,
        acuerdo_fecha: af.as_deref().map(parse_date).transpose()?,
        acuerdo_importe: ai.map(Money::from_cents),
        acuerdo_nota: an,
        guardada: true,
    }))
}

/// Ficha del año: la guardada o, si no, la del último año anterior (como propuesta, con la
/// base de ese año) o la de partida.
pub fn ficha(v: &Vault, employee_id: i64, year: i32) -> Result<FichaReta> {
    if let Some(f) = ficha_guardada(v, employee_id, year, true)? {
        return Ok(f);
    }
    Ok(match ficha_guardada(v, employee_id, year, false)? {
        Some(prev) => FichaReta {
            year,
            guardada: false,
            ..prev
        },
        None => FichaReta::nueva(employee_id, year),
    })
}

fn cuenta(code: &str, what: &str) -> Result<AccountCode> {
    AccountCode::new(code.trim()).map_err(|e| other(format!("{what}: {e}")))
}

/// Guarda la ficha del año.
pub fn save_ficha(v: &Vault, f: &FichaReta, now: OffsetDateTime) -> Result<()> {
    administrador_reta(v, f.employee_id)?;
    if !(1..=12).contains(&f.meses_alta) {
        return Err(other("los meses de alta van de 1 a 12"));
    }
    if !(0..=10_000).contains(&f.pct_dividendos.bp()) {
        return Err(other("el porcentaje de los dividendos va de 0 a 100"));
    }
    if f.base_elegida.is_negative() || f.gastos_deducibles.is_negative() {
        return Err(other("la base y los gastos no pueden ser negativos"));
    }
    if f.acuerdo_importe.is_some_and(Money::is_negative) {
        return Err(other("el importe máximo de la junta no puede ser negativo"));
    }
    let gasto = cuenta(&f.cuenta_gasto, "cuenta de gasto")?;
    let tesoreria = cuenta(&f.cuenta_tesoreria, "cuenta de tesorería")?;
    if !gasto.as_str().starts_with('6') || !tesoreria.as_str().starts_with('5') {
        return Err(other(
            "la cuenta de gasto es del grupo 6 (642, 649, 640) y la de tesorería del 5 (572…)",
        ));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO reta_ficha(employee_id, year, base_elegida, rendimiento_previsto, meses_alta,
           control_efectivo, pct_dividendos_bp, otros_rendimientos, gastos_deducibles, pagador,
           cuenta_gasto, cuenta_tesoreria, estatutos_retribuido, acuerdo_fecha, acuerdo_importe,
           acuerdo_nota, updated_at, alta_societario)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
         ON CONFLICT(employee_id, year) DO UPDATE SET base_elegida = ?3,
           rendimiento_previsto = ?4, meses_alta = ?5, control_efectivo = ?6,
           pct_dividendos_bp = ?7, otros_rendimientos = ?8, gastos_deducibles = ?9, pagador = ?10,
           cuenta_gasto = ?11, cuenta_tesoreria = ?12, estatutos_retribuido = ?13,
           acuerdo_fecha = ?14, acuerdo_importe = ?15, acuerdo_nota = ?16, updated_at = ?17,
           alta_societario = ?18",
        params![
            f.employee_id,
            f.year,
            f.base_elegida.cents(),
            f.rendimiento_previsto.map(Money::cents),
            f.meses_alta,
            f.control_efectivo,
            f.pct_dividendos.bp(),
            f.otros_rendimientos.cents(),
            f.gastos_deducibles.cents(),
            f.pagador.as_str(),
            gasto.as_str(),
            tesoreria.as_str(),
            f.estatutos_retribuido,
            f.acuerdo_fecha.map(|d| d.to_string()),
            f.acuerdo_importe.map(Money::cents),
            f.acuerdo_nota.trim(),
            now_str(now),
            f.alta_societario.map(|d| d.to_string()),
        ],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "reta_ficha.save",
        &format!("{}/{}", f.employee_id, f.year),
        &serde_json::json!({ "base": f.base_elegida.cents(), "pagador": f.pagador.as_str() }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Acuerdo de la junta sobre la retribución del administrador en `year`.
///
/// Punto de extensión: cuando exista el libro de actas (G08), buscar aquí primero el acta que
/// fija la retribución de los administradores (fecha e importe máximo anual, art. 217.3 LSC) y
/// devolverla con `fuente: "libro de actas"`; si no hay ninguna, lo de la ficha.
pub fn acuerdo_retribucion(v: &Vault, employee_id: i64, year: i32) -> Result<AcuerdoRetribucion> {
    let f = ficha(v, employee_id, year)?;
    Ok(AcuerdoRetribucion {
        estatutos_retribuido: f.estatutos_retribuido,
        fecha: f.acuerdo_fecha,
        importe_maximo: f.acuerdo_importe,
        fuente: "ficha del RETA",
    })
}

/// Cuotas de un administrador entre dos fechas.
pub fn cuotas(v: &Vault, employee_id: i64, start: Date, end: Date) -> Result<Vec<CuotaReta>> {
    let mut st = v.connection().prepare(
        "SELECT id, employee_id, date, amount, pagador, entry_id FROM reta_cuota
         WHERE employee_id = ?1 AND date BETWEEN ?2 AND ?3 ORDER BY date, id",
    )?;
    let rows = st.query_map(
        params![employee_id, start.to_string(), end.to_string()],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Option<i64>>(5)?,
            ))
        },
    )?;
    let mut out = Vec::new();
    for row in rows {
        let (id, employee_id, date, amount, pagador, entry_id) = row?;
        out.push(CuotaReta {
            id,
            employee_id,
            date: parse_date(&date)?,
            amount: Money::from_cents(amount),
            pagador: Pagador::of(&pagador),
            entry_id,
        });
    }
    Ok(out)
}

/// Asiento de la cuota que paga la sociedad: cuenta de gasto al debe y tesorería al haber.
pub fn asiento_cuota(f: &FichaReta, name: &str, date: Date, amount: Money) -> Result<JournalEntry> {
    let gasto = cuenta(&f.cuenta_gasto, "cuenta de gasto")?;
    let tesoreria = cuenta(&f.cuenta_tesoreria, "cuenta de tesorería")?;
    Ok(JournalEntry {
        date,
        concept: format!(
            "Cuota RETA {name} {:02}/{} (retribución del administrador)",
            u8::from(date.month()),
            date.year()
        ),
        origin: Origin::Manual,
        lines: [
            EntryLine::signed(gasto, amount, true),
            EntryLine::signed(tesoreria, amount, false),
        ]
        .into_iter()
        .flatten()
        .collect(),
    })
}

/// Registra una cuota pagada y, si la paga la sociedad y se pide, su asiento. Devuelve su id.
pub fn register_cuota(v: &Vault, c: &CuotaInput, now: OffsetDateTime) -> Result<i64> {
    let e = administrador_reta(v, c.employee_id)?;
    if c.amount.is_negative() || c.amount.is_zero() {
        return Err(other("la cuota tiene que ser mayor que cero"));
    }
    let f = ficha(v, c.employee_id, c.date.year())?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let entry_id = if c.asiento && c.pagador == Pagador::Sociedad {
        let entry = asiento_cuota(&f, &e.name, c.date, c.amount)?;
        Some(accounting::record_entry(&tx, &entry, now).map_err(|err| {
            if err.to_string().contains("cerrado") {
                other("el ejercicio de esa cuota está cerrado: reábrelo en Cierre")
            } else {
                err
            }
        })?)
    } else {
        None
    };
    tx.execute(
        "INSERT INTO reta_cuota(employee_id, date, amount, pagador, entry_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            c.employee_id,
            c.date.to_string(),
            c.amount.cents(),
            c.pagador.as_str(),
            entry_id,
            now_str(now)
        ],
    )?;
    let id = tx.last_insert_rowid();
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "reta_cuota.register",
        &id.to_string(),
        &serde_json::json!({ "entry": entry_id, "amount": c.amount.cents() }),
    )?;
    tx.commit()?;
    Ok(id)
}

/// Borra una cuota y su asiento, con confirmación.
pub fn delete_cuota(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let entry: Option<Option<i64>> = tx
        .query_row("SELECT entry_id FROM reta_cuota WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    let entry = entry.ok_or(AppError::NotFound)?;
    tx.execute("DELETE FROM reta_cuota WHERE id = ?1", [id])?;
    if let Some(e) = entry {
        tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [e])
            .map_err(|err| {
                if err.to_string().contains("cerrado") {
                    other("el ejercicio de esa cuota está cerrado: reábrelo en Cierre")
                } else {
                    err.into()
                }
            })?;
        tx.execute("DELETE FROM journal_entry WHERE id = ?1", [e])?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "reta_cuota.delete",
        &id.to_string(),
        &serde_json::json!({ "entry": entry }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Dividendos íntegros cobrados de la sociedad por el NIF en `year`.
fn dividendos_de(v: &Vault, nif: &str, year: i32) -> Result<Money> {
    let nif = nif.trim().to_uppercase();
    Ok(crate::socios::payments_of_year(v, year)?
        .iter()
        .flat_map(|p| p.lines.iter())
        .filter(|l| l.nif.trim().to_uppercase() == nif)
        .map(|l| l.gross)
        .sum())
}

/// Tipo de retención del administrador (19 o 35 %) según la cifra de negocios del último
/// ejercicio cerrado antes de `date`; `None` si TaxPain no la sabe.
fn tipo_retencion(v: &Vault, date: Date) -> Option<Rate> {
    payroll::director_turnover(v, date)
        .ok()
        .and_then(|t| t.small)
        .map(director_rate)
}

fn fmt_fecha(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

/// Resumen del año de un administrador en el RETA.
pub fn resumen(v: &Vault, employee_id: i64, year: i32, hoy: Date) -> Result<ResumenReta> {
    let e = administrador_reta(v, employee_id)?;
    let f = ficha(v, employee_id, year)?;
    let (start, end) = year_bounds(year)?;
    let dinerarias: Money = payroll::payslips(v.connection(), start, end)?
        .iter()
        .filter(|s| s.employee_id == employee_id)
        .map(|s| s.gross)
        .sum();
    let cuotas = cuotas(v, employee_id, start, end)?;
    let cuotas_total: Money = cuotas.iter().map(|c| c.amount).sum();
    let especie: Money = cuotas
        .iter()
        .filter(|c| c.pagador == Pagador::Sociedad)
        .map(|c| c.amount)
        .sum();
    let dividendos = dividendos_de(v, &e.nif, year)?;
    let dias_alta_societario = reta::dias_alta_societario(year, f.alta_societario, f.meses_alta);
    let noventa_dias = reta::cumple_dias_societario(dias_alta_societario);
    let rendimiento = reta::rendimiento(&EntradaRendimiento {
        dinerarias,
        especie,
        gastos: f.gastos_deducibles,
        dividendos,
        pct_dividendos: if f.control_efectivo {
            f.pct_dividendos
        } else {
            Rate::ZERO
        },
        otros: f.otros_rendimientos,
        societario: noventa_dias,
        meses: f.meses_alta,
    });
    let tabla = reta::tabla(year).copied();
    let tramo_previsto = tabla
        .as_ref()
        .zip(f.rendimiento_previsto)
        .map(|(t, r)| *reta::tramo_de(t, r));
    let regularizacion = tabla.as_ref().map(|t| {
        reta::regularizacion(
            t,
            rendimiento.mensual,
            f.base_elegida,
            f.meses_alta,
            noventa_dias,
        )
    });
    let base_recomendada = regularizacion.as_ref().map(|r| r.base_min);
    let cuota_mensual = tabla
        .as_ref()
        .map_or(Money::ZERO, |t| reta::cuota(f.base_elegida, t.tipo));
    let fecha_tipo = hoy.clamp(start, end);
    let tipo = tipo_retencion(v, fecha_tipo);
    let ingreso_a_cuenta = tipo.map_or(Money::ZERO, |t| especie.apply_rate(t));
    let retribucion_total = dinerarias + especie;
    let acuerdo = acuerdo_retribucion(v, employee_id, year)?;
    let ventana = reta::ventana_abierta(hoy);
    let proximas_ventanas: Vec<VentanaCambio> = reta::ventanas_cambio(hoy.year())
        .into_iter()
        .chain(reta::ventanas_cambio(hoy.year() + 1))
        .filter(|w| w.hasta >= hoy)
        .take(3)
        .collect();

    let mut avisos = Vec::new();
    match &tabla {
        None => avisos.push(format!(
            "TaxPain no tiene la tabla de tramos del RETA de {year}: no se puede calcular el tramo \
             ni la regularización"
        )),
        Some(t) if t.year != year => avisos.push(format!(
            "No hay tabla de {year}: se usa la de {} hasta que se publique la orden de cotización",
            t.year
        )),
        Some(_) => {}
    }
    if !f.guardada {
        avisos.push(format!(
            "La ficha de {year} no está guardada: es una propuesta. Pon la base por la que cotiza \
             y el rendimiento que comunicó a la TGSS y guárdala"
        ));
    }
    if !noventa_dias {
        avisos.push(format!(
            "En {year} sólo hay {dias_alta_societario} días de alta como societario (hacen falta \
             {}): no se aplican la deducción del 3 % ni la base mínima de los societarios, sino \
             el 7 % y la mínima del tramo (art. 308.1.a, regla 4.ª, y c, regla 2.ª, TRLGSS; DT \
             7.ª RDL 13/2022)",
            reta::DIAS_ALTA_SOCIETARIO
        ));
    } else if f.alta_societario.is_none() && f.meses_alta < 12 {
        avisos.push(format!(
            "Sin fecha de alta como societario, los {} meses de alta se cuentan como {} días: \
             pon la fecha para saber si llega a los noventa días",
            f.meses_alta, dias_alta_societario
        ));
    }
    if dinerarias.is_zero() {
        avisos.push(format!(
            "No hay nóminas de {} en {year}: el rendimiento sólo lleva lo demás",
            e.name
        ));
    }
    if let (Some(t), Some(tp), Some(r)) = (&tabla, &tramo_previsto, &regularizacion) {
        let (min, max) = reta::bases_aplicables(t, tp, noventa_dias);
        if f.base_elegida < min || f.base_elegida > max {
            avisos.push(format!(
                "La base elegida ({}) no está entre la mínima ({min}) y la máxima ({max}) del \
                 tramo del rendimiento previsto ({}): la TGSS no la admite",
                f.base_elegida,
                tp.nombre()
            ));
        }
        if tp.tabla != r.tramo.tabla || tp.numero != r.tramo.numero {
            avisos.push(format!(
                "El rendimiento previsto (tramo {}) no coincide con el que sale de TaxPain (tramo \
                 {}): puedes cambiarlo hasta el {} con efectos del {}",
                tp.nombre(),
                r.tramo_nombre,
                fmt_fecha(ventana.hasta),
                fmt_fecha(ventana.efectos)
            ));
        }
    }
    if let Some(r) = &regularizacion {
        match r.sentido {
            reta::Sentido::AIngresar => avisos.push(format!(
                "Con estos rendimientos la TGSS reclamará unos {} al regularizar {year} (la base \
                 tendría que ser de {} como poco)",
                r.importe, r.base_min
            )),
            reta::Sentido::ADevolver => avisos.push(format!(
                "Con estos rendimientos la TGSS devolverá unos {} al regularizar {year} (la base \
                 máxima del tramo es {})",
                r.importe, r.base_max
            )),
            reta::Sentido::Ninguna => {}
        }
    }
    if f.pagador == Pagador::Sociedad && !especie.is_zero() {
        avisos.push(
            "TaxPain todavía no lleva la retribución en especie (la cuota que paga la sociedad) \
             ni su ingreso a cuenta al 111 (casillas 04 a 06) ni al 190: añádelos al revisar los \
             borradores"
                .into(),
        );
        if tipo.is_none() {
            avisos.push(
                "Para el ingreso a cuenta hace falta saber si la cifra de negocios del último \
                 ejercicio cerrado fue inferior a 100.000 € (19 %) o no (35 %)"
                    .into(),
            );
        }
    }
    if f.pagador == Pagador::Sociedad && cuotas.is_empty() && year <= hoy.year() {
        avisos.push(format!("No hay cuotas del RETA registradas en {year}"));
    }
    if !acuerdo.estatutos_retribuido {
        avisos.push(
            "No consta que los estatutos digan que el cargo de administrador es retribuido (art. \
             217.1 LSC): sin eso Hacienda puede negar el gasto (art. 15.e LIS)"
                .into(),
        );
    }
    match acuerdo.importe_maximo {
        None => avisos.push(
            "No consta el acuerdo de la junta que fija el importe máximo anual de la retribución \
             de los administradores (art. 217.3 LSC)"
                .into(),
        ),
        Some(max) if retribucion_total > max => avisos.push(format!(
            "Lo pagado en {year} ({retribucion_total}) supera el máximo aprobado por la junta \
             ({max})"
        )),
        Some(_) => {}
    }
    Ok(ResumenReta {
        employee_id,
        name: e.name,
        nif: e.nif,
        ficha: f,
        tabla,
        dinerarias,
        especie,
        dividendos,
        rendimiento,
        dias_alta_societario,
        noventa_dias,
        tramo_previsto,
        regularizacion,
        base_recomendada,
        cuota_mensual,
        cuotas,
        cuotas_total,
        tipo_retencion: tipo,
        ingreso_a_cuenta,
        retribucion_total,
        acuerdo,
        ventana_abierta: ventana,
        proximas_ventanas,
        avisos,
    })
}

/// Importes del recibo de la retribución del administrador.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Recibo {
    pub date: Date,
    pub name: String,
    pub nif: String,
    pub dineraria: Money,
    pub especie: Money,
    pub retencion: Money,
    pub cotizacion: Money,
    pub tipo_retencion: Option<Rate>,
    pub ingreso_a_cuenta: Money,
    pub liquido: Money,
    pub subclave: Option<u8>,
    pub acuerdo: Option<AcuerdoRetribucion>,
}

/// Datos del recibo de una nómina de administrador: la especie es la cuota del RETA que pagó
/// la sociedad ese mes.
pub fn recibo(v: &Vault, payslip_id: i64) -> Result<Recibo> {
    let date: String = v
        .connection()
        .query_row(
            "SELECT date FROM payslip WHERE id = ?1",
            [payslip_id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let date = parse_date(&date)?;
    let slip = payroll::payslips(v.connection(), date, date)?
        .into_iter()
        .find(|s| s.id == payslip_id)
        .ok_or(AppError::NotFound)?;
    let e = payroll::employee(v.connection(), slip.employee_id)?;
    if !e.administrator {
        return Err(other(
            "el recibo de retribución es para administradores; el de un empleado lo hace la \
             gestoría laboral",
        ));
    }
    let (especie, acuerdo) = if e.ss_regime == DirectorSs::Autonomo {
        let first = date.replace_day(1).map_err(other)?;
        let last = date
            .replace_day(date.month().length(date.year()))
            .map_err(other)?;
        let especie: Money = cuotas(v, e.id, first, last)?
            .iter()
            .filter(|c| c.pagador == Pagador::Sociedad)
            .map(|c| c.amount)
            .sum();
        (especie, Some(acuerdo_retribucion(v, e.id, date.year())?))
    } else {
        (Money::ZERO, None)
    };
    // Subclaves 02 y 03: 19 %; 01: 35 %; 04 (autónomo en una entidad grande o sin alta): 35 %
    // si es autónomo, y si no, según la cifra de negocios.
    let tipo = match slip.director_subkey {
        Some(2 | 3) => Some(director_rate(true)),
        Some(1) => Some(director_rate(false)),
        Some(_) if e.ss_regime == DirectorSs::Autonomo => Some(director_rate(false)),
        _ => tipo_retencion(v, date),
    };
    Ok(Recibo {
        date,
        name: e.name,
        nif: e.nif,
        dineraria: slip.gross,
        especie,
        retencion: slip.irpf,
        cotizacion: slip.ss_employee,
        tipo_retencion: tipo,
        ingreso_a_cuenta: tipo.map_or(Money::ZERO, |t| especie.apply_rate(t)),
        liquido: slip.net,
        subclave: slip.director_subkey,
        acuerdo,
    })
}

/// Recibo de la retribución del administrador en PDF. Devuelve el nombre del fichero y el PDF.
pub fn recibo_pdf(v: &Vault, payslip_id: i64, place_date: &str) -> Result<(String, Vec<u8>)> {
    use taxpain_invoicing::pdf::{Para, text_document};
    let r = recibo(v, payslip_id)?;
    let who = crate::models::declarant(v)?;
    let mes = format!("{:02}/{}", u8::from(r.date.month()), r.date.year());
    let mut doc = vec![
        Para::Bold(format!("{} · NIF {}", who.full_name(), who.nif)),
        Para::Gap,
        Para::Title("Recibo de retribución del administrador".into()),
        Para::Text(format!(
            "Perceptor: {} · NIF {} · cargo de administrador{}",
            r.name,
            r.nif,
            r.subclave
                .map(|s| format!(" (modelo 190, clave E, subclave {s:02})"))
                .unwrap_or_default()
        )),
        Para::Text(format!(
            "Periodo: {mes} · fecha de pago: {}",
            fmt_fecha(r.date)
        )),
        Para::Gap,
        Para::Row("Retribución dineraria".into(), r.dineraria.to_string()),
    ];
    if !r.especie.is_zero() {
        doc.push(Para::Row(
            "Retribución en especie: cuota del RETA pagada por la sociedad".into(),
            r.especie.to_string(),
        ));
    }
    doc.push(Para::Row(
        "Total retribución".into(),
        (r.dineraria + r.especie).to_string(),
    ));
    doc.push(Para::Row(
        match r.tipo_retencion {
            Some(t) => format!("Retención del IRPF ({t})"),
            None => "Retención del IRPF".into(),
        },
        format!("-{}", r.retencion),
    ));
    if !r.cotizacion.is_zero() {
        doc.push(Para::Row(
            "Cotización a la Seguridad Social descontada".into(),
            format!("-{}", r.cotizacion),
        ));
    }
    if !r.especie.is_zero() {
        doc.push(Para::Row(
            "Especie (no se cobra en dinero)".into(),
            format!("-{}", r.especie),
        ));
    }
    doc.push(Para::Row(
        "Líquido a percibir".into(),
        r.liquido.to_string(),
    ));
    doc.push(Para::Gap);
    if !r.especie.is_zero() {
        doc.push(Para::Text(format!(
            "Ingreso a cuenta sobre la retribución en especie, a cargo de la sociedad: {} (arts. \
             43 LIRPF y 102 RIRPF).",
            r.ingreso_a_cuenta
        )));
    }
    doc.push(Para::Text(
        "Retribución del cargo de administrador (art. 217 LSC), rendimiento del trabajo (art. \
         17.2.e LIRPF) con la retención fija del art. 101.2 LIRPF y el art. 80.1.3.º RIRPF."
            .into(),
    ));
    match &r.acuerdo {
        Some(AcuerdoRetribucion {
            fecha: Some(f),
            importe_maximo: Some(m),
            ..
        }) => doc.push(Para::Text(format!(
            "Importe máximo anual aprobado por la junta general el {}: {m}.",
            fmt_fecha(*f)
        ))),
        _ => doc.push(Para::Bold(
            "Aviso: no consta en TaxPain el acuerdo de la junta que fija la retribución de los \
             administradores (art. 217.3 LSC)."
                .into(),
        )),
    }
    doc.push(Para::Gap);
    doc.push(Para::Text(place_date.to_owned()));
    doc.push(Para::Gap);
    doc.push(Para::Text(format!("Recibí: {}", r.name)));
    let pdf =
        text_document(&format!("Recibo de retribución {mes}"), &[doc]).map_err(AppError::Other)?;
    Ok((
        format!(
            "Recibo administrador {} {}-{:02}.pdf",
            r.nif,
            r.date.year(),
            u8::from(r.date.month())
        ),
        pdf,
    ))
}
