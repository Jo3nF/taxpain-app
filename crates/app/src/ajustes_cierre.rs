//! Ajustes de cierre: lo que la gestoría hace «de oficio» antes de cerrar y que sin contable se
//! olvida. TaxPain propone, el usuario confirma y cada ajuste lleva su asiento (con la fecha de
//! cierre) y, cuando procede, su reversión.
//!
//! - **Periodificaciones** (principio de devengo, marco conceptual del PGC de PYMES): de una
//!   factura recibida (seguro, suscripción anual, alquiler pagado por adelantado) cuyo periodo de
//!   servicio pasa del cierre, la parte del ejercicio siguiente, por días, sale del gasto a la
//!   480 «Gastos anticipados»; de una emitida, del ingreso a la 485 «Ingresos anticipados». El
//!   periodo no está en la factura: lo indica el usuario. Se revierte el primer día del ejercicio
//!   siguiente (el gasto o el ingreso vuelve a su cuenta). Las dos cuentas existen en el PGC de
//!   PYMES y en el normal (subgrupo 48); la 567/568 son los intereses pagados o cobrados por
//!   anticipado (los de los préstamos los periodifica `prestamos`) y la 580 del PGC normal son
//!   activos no corrientes mantenidos para la venta, no una periodificación.
//!   `TODO(VERIFY)`: la parte que pase del ejercicio siguiente es a largo plazo; el PGC de PYMES
//!   no tiene cuenta propia para ella (se avisa y queda en la 480/485).
//! - **Facturas pendientes de recibir y de emitir**: un gasto devengado del que aún no hay
//!   factura va a la 4009 «Proveedores, facturas pendientes de recibir o de formalizar» y un
//!   ingreso devengado sin factura, a la 4309 «Clientes, facturas pendientes de formalizar»; por
//!   la base, sin IVA (el IVA se devenga con la factura). Se revierten el primer día del
//!   ejercicio siguiente: la factura, al llegar o al emitirla, se contabiliza como siempre y el
//!   gasto o el ingreso queda una sola vez, en el ejercicio en que se devengó.
//! - **Deterioro de créditos comerciales** (NRV 9.ª, 2.1.3): 694 a 490 por el cobro pendiente
//!   que se estima que no se cobrará, cliente a cliente. Se propone desde la cartera de cobros:
//!   lo que al cierre lleva más de seis meses vencido. Fiscalmente (art. 13.1 LIS) es deducible
//!   si al cierre han pasado seis meses desde el vencimiento, el deudor está en concurso o
//!   procesado por alzamiento de bienes, o el crédito está reclamado judicialmente o en
//!   arbitraje; no lo es, salvo en un procedimiento judicial o arbitral sobre su existencia o
//!   cuantía, si el deudor es una entidad de derecho público, si está afianzado por un banco o
//!   una SGR, garantizado con derechos reales o con un seguro de crédito o caución, o renovado o
//!   prorrogado; con una persona o entidad vinculada sólo si está en concurso con la fase de
//!   liquidación abierta. Lo no deducible es una corrección en aumento en el 200 del ejercicio y,
//!   al revertirlo, en disminución. `TODO(VERIFY)`: el detalle de las exclusiones, contra el texto
//!   consolidado del art. 13.1 LIS (BOE-A-2014-12328); las empresas de reducida dimensión pueden
//!   deducir además una dotación global del 1 % de los saldos de deudores (art. 104 LIS), que
//!   aquí no se propone. Se revierte (490 a 794) cuando se cobra o deja de hacer falta, en el
//!   cierre de un ejercicio posterior; la pérdida definitiva (650) se contabiliza a mano.
//!   `TODO(VERIFY)`: con partes vinculadas el PGC usa la 493; aquí va siempre a la 490.
//! - **Provisiones** (NRV 15.ª): una obligación probable de importe o fecha inciertos (un litigio,
//!   una inspección, una garantía) se dota contra su cuenta de gasto: 14x a largo plazo, 529 a
//!   corto y 499 las de operaciones comerciales. Muchas no son deducibles (art. 14 LIS: las
//!   derivadas de obligaciones implícitas o tácitas, las de reestructuración, las de contratos
//!   onerosos, las de devoluciones de ventas, las de retribuciones a largo plazo al personal…);
//!   por prudencia, TaxPain las trata como no deducibles salvo que el usuario diga lo contrario.
//!   El exceso se revierte a la 795 en un ejercicio posterior; su aplicación (el pago del
//!   litigio) se contabiliza a mano contra la provisión. `TODO(VERIFY)`: cuándo es deducible
//!   una provisión por litigio de una obligación legal o contractual.
//!
//! Antes de los ajustes, las comprobaciones de saldos anómalos ([`comprobaciones`]).

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::account::AccountCode;
use taxpain_core::accounting::entry::{EntryLine, JournalEntry, Origin};
use taxpain_core::accounting::fiscal_year::FiscalYear;
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, Duration, OffsetDateTime};

use crate::accounting::{self, record_entry};
use crate::cierre::ProposedLine;
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

/// Cuentas del balance de cada ajuste.
pub const GASTOS_ANTICIPADOS: &str = "480";
pub const INGRESOS_ANTICIPADOS: &str = "485";
pub const PENDIENTES_RECIBIR: &str = "4009";
pub const PENDIENTES_EMITIR: &str = "4309";
pub const DETERIORO: &str = "490";
pub const PERDIDA_DETERIORO: &str = "694";
pub const REVERSION_DETERIORO: &str = "794";

/// Cuentas de gasto o ingreso que suelen pagarse o cobrarse por adelantado (las que se
/// proponen sin pedir «todas»): arrendamientos, seguros, publicidad y otros servicios
/// (suscripciones, dominios, licencias); en las emitidas, las prestaciones de servicios y los
/// arrendamientos.
const PERIODIFICABLES_GASTO: [&str; 4] = ["621", "625", "627", "629"];
const PERIODIFICABLES_INGRESO: [&str; 2] = ["705", "752"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tipo {
    GastoAnticipado,
    IngresoAnticipado,
    PendienteRecibir,
    PendienteEmitir,
    Deterioro,
    Provision,
}

impl Tipo {
    pub fn code(self) -> &'static str {
        match self {
            Tipo::GastoAnticipado => "gasto_anticipado",
            Tipo::IngresoAnticipado => "ingreso_anticipado",
            Tipo::PendienteRecibir => "pendiente_recibir",
            Tipo::PendienteEmitir => "pendiente_emitir",
            Tipo::Deterioro => "deterioro",
            Tipo::Provision => "provision",
        }
    }

    pub fn from_code(s: &str) -> Option<Tipo> {
        [
            Tipo::GastoAnticipado,
            Tipo::IngresoAnticipado,
            Tipo::PendienteRecibir,
            Tipo::PendienteEmitir,
            Tipo::Deterioro,
            Tipo::Provision,
        ]
        .into_iter()
        .find(|t| t.code() == s)
    }

    /// Se revierte solo el primer día del ejercicio siguiente (se graba con el ajuste).
    pub fn automatica(self) -> bool {
        !matches!(self, Tipo::Deterioro | Tipo::Provision)
    }

    pub fn label(self) -> &'static str {
        match self {
            Tipo::GastoAnticipado => "Gasto anticipado",
            Tipo::IngresoAnticipado => "Ingreso anticipado",
            Tipo::PendienteRecibir => "Factura pendiente de recibir",
            Tipo::PendienteEmitir => "Factura pendiente de emitir",
            Tipo::Deterioro => "Deterioro de crédito comercial",
            Tipo::Provision => "Provisión",
        }
    }
}

/// Circunstancia del art. 13.1 LIS que hace deducible el deterioro de un crédito.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Motivo {
    /// Seis meses desde el vencimiento (art. 13.1.a): se comprueba con la fecha.
    #[default]
    SeisMeses,
    /// Deudor declarado en concurso (art. 13.1.b).
    Concurso,
    /// En concurso y con la fase de liquidación abierta (lo que exige si es vinculado).
    ConcursoLiquidacion,
    /// Deudor procesado por alzamiento de bienes (art. 13.1.c).
    Alzamiento,
    /// Reclamado judicialmente o en un litigio o arbitraje (art. 13.1.d).
    Judicial,
    /// Ninguna: el deterioro es sólo contable (no deducible).
    Ninguno,
}

/// Crédito que el art. 13.1 LIS excluye aunque se dé la circunstancia.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Exclusion {
    #[default]
    Ninguna,
    /// Adeudado o afianzado por una entidad de derecho público.
    Publica,
    /// Afianzado por un banco o una SGR, garantizado con derechos reales o reserva de dominio,
    /// o con un seguro de crédito o caución.
    Garantia,
    /// Renovado o prorrogado expresamente.
    Renovado,
    /// Persona o entidad vinculada (art. 18.2 LIS).
    Vinculada,
}

/// ¿Es deducible el deterioro (art. 13.1 LIS)? Devuelve también la explicación.
pub fn deducible_deterioro(
    motivo: Motivo,
    exclusion: Exclusion,
    seis_meses: bool,
) -> (bool, String) {
    let circunstancia = match motivo {
        Motivo::SeisMeses => seis_meses,
        Motivo::Ninguno => false,
        _ => true,
    };
    if !circunstancia {
        return (
            false,
            if motivo == Motivo::SeisMeses {
                "No deducible: al cierre aún no han pasado seis meses desde el vencimiento (art. \
                 13.1.a LIS)."
            } else {
                "No deducible: no se da ninguna de las circunstancias del art. 13.1 LIS."
            }
            .into(),
        );
    }
    match exclusion {
        Exclusion::Ninguna => (
            true,
            match motivo {
                Motivo::SeisMeses => "Deducible: más de seis meses vencido (art. 13.1.a LIS).",
                Motivo::Concurso | Motivo::ConcursoLiquidacion => {
                    "Deducible: deudor en concurso (art. 13.1.b LIS)."
                }
                Motivo::Alzamiento => {
                    "Deducible: deudor procesado por alzamiento de bienes (art. 13.1.c LIS)."
                }
                _ => "Deducible: reclamado judicialmente o en arbitraje (art. 13.1.d LIS).",
            }
            .into(),
        ),
        Exclusion::Vinculada if motivo == Motivo::ConcursoLiquidacion => (
            true,
            "Deducible: deudor vinculado en concurso con la fase de liquidación abierta (art. \
             13.1 LIS)."
                .into(),
        ),
        Exclusion::Vinculada => (
            false,
            "No deducible: deudor vinculado; sólo lo sería en concurso con la fase de liquidación \
             abierta (art. 13.1 LIS)."
                .into(),
        ),
        _ if motivo == Motivo::Judicial => (
            true,
            "Deducible: hay un procedimiento judicial o arbitral sobre su existencia o cuantía \
             (art. 13.1 LIS)."
                .into(),
        ),
        Exclusion::Publica => (
            false,
            "No deducible: adeudado o afianzado por una entidad de derecho público (art. 13.1 \
             LIS), salvo procedimiento judicial o arbitral."
                .into(),
        ),
        Exclusion::Garantia => (
            false,
            "No deducible: crédito afianzado o garantizado (art. 13.1 LIS), salvo procedimiento \
             judicial o arbitral."
                .into(),
        ),
        Exclusion::Renovado => (
            false,
            "No deducible: crédito renovado o prorrogado (art. 13.1 LIS), salvo procedimiento \
             judicial o arbitral."
                .into(),
        ),
    }
}

/// Parte de `importe` que corresponde a lo que va después del cierre `cierre` en un periodo de
/// servicio de `desde` a `hasta` (los dos incluidos), por días y redondeada al céntimo.
pub fn parte_siguiente(importe: Money, desde: Date, hasta: Date, cierre: Date) -> Money {
    if hasta <= cierre || hasta < desde {
        return Money::ZERO;
    }
    if desde > cierre {
        return importe;
    }
    let total = i128::from((hasta - desde).whole_days() + 1);
    let despues = i128::from((hasta - cierre).whole_days());
    let c = i128::from(importe.cents()) * despues;
    // Redondeo al céntimo más próximo (la mitad, hacia fuera).
    let r = (c.abs() * 2 + total) / (total * 2) * c.signum();
    Money::from_cents(i64::try_from(r).unwrap_or(i64::MAX))
}

/// Cuenta de exceso de provisiones (795x) que revierte la provisión de `cuenta`.
pub fn cuenta_exceso(cuenta: &str) -> &'static str {
    if cuenta.starts_with("141") || cuenta.starts_with("5291") {
        "7951"
    } else if cuenta.starts_with("145") || cuenta.starts_with("5295") {
        "7955"
    } else if cuenta.starts_with("499") {
        "7954"
    } else {
        "7952"
    }
}

fn provision_valida(cuenta: &str) -> bool {
    ["141", "142", "145", "499", "5291", "5292", "5295"]
        .iter()
        .any(|p| cuenta.starts_with(p))
}

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

fn code(s: &str) -> Result<AccountCode> {
    AccountCode::new(s).map_err(|e| other(e.to_string()))
}

fn fecha(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]"))
        .map_err(|e| other(e.to_string()))
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

fn proposed(conn: &Connection, lines: &[EntryLine]) -> Result<Vec<ProposedLine>> {
    lines
        .iter()
        .map(|l| {
            Ok(ProposedLine {
                name: accounting::account_name(conn, l.account.as_str())?.unwrap_or_default(),
                account: l.account.as_str().into(),
                debit: l.debit,
                credit: l.credit,
            })
        })
        .collect()
}

fn mirror(lines: &[EntryLine]) -> Vec<EntryLine> {
    lines
        .iter()
        .map(|l| EntryLine {
            account: l.account.clone(),
            debit: l.credit,
            credit: l.debit,
            description: l.description.clone(),
        })
        .collect()
}

/// Un ajuste ya contabilizado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Ajuste {
    pub id: i64,
    pub year: i32,
    pub tipo: Tipo,
    pub descripcion: String,
    pub importe: Money,
    pub cuenta: String,
    pub contrapartida: String,
    pub factura_kind: Option<String>,
    pub factura_id: Option<i64>,
    pub vencimiento: Option<i64>,
    pub servicio_desde: Option<Date>,
    pub servicio_hasta: Option<Date>,
    pub tercero_nif: Option<String>,
    pub tercero: Option<String>,
    pub deducible: bool,
    pub motivo_fiscal: String,
    pub entry_id: i64,
    pub fecha: Date,
    pub lines: Vec<ProposedLine>,
    pub reversion_entry_id: Option<i64>,
    pub reversion_fecha: Option<Date>,
    pub reversion_year: Option<i32>,
}

const COLS: &str = "a.id, a.year, a.tipo, a.descripcion, a.importe, a.cuenta, a.contrapartida,
    a.factura_kind, a.factura_id, a.vencimiento, a.servicio_desde, a.servicio_hasta,
    a.tercero_nif, a.tercero, a.deducible, a.motivo_fiscal, a.entry_id, e.date,
    a.reversal_entry_id, (SELECT date FROM journal_entry WHERE id = a.reversal_entry_id),
    a.reversal_year";

fn load(
    conn: &Connection,
    filter: &str,
    p: impl taxpain_vault::rusqlite::Params,
) -> Result<Vec<Ajuste>> {
    let mut st = conn.prepare(&format!(
        "SELECT {COLS} FROM ajuste_cierre a JOIN journal_entry e ON e.id = a.entry_id
          WHERE {filter} ORDER BY a.year, a.tipo, a.id"
    ))?;
    #[allow(clippy::type_complexity)]
    let rows: Vec<(
        (i64, i32, String, String, i64, String, String),
        (
            Option<String>,
            Option<i64>,
            Option<i64>,
            Option<String>,
            Option<String>,
        ),
        (Option<String>, Option<String>, i64, String, i64, String),
        (Option<i64>, Option<String>, Option<i32>),
    )> = st
        .query_map(p, |r| {
            Ok((
                (
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ),
                (r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?),
                (
                    r.get(12)?,
                    r.get(13)?,
                    r.get(14)?,
                    r.get(15)?,
                    r.get(16)?,
                    r.get(17)?,
                ),
                (r.get(18)?, r.get(19)?, r.get(20)?),
            ))
        })?
        .collect::<std::result::Result<_, _>>()?;
    rows.into_iter()
        .map(
            |(
                (id, year, tipo, descripcion, importe, cuenta, contrapartida),
                (factura_kind, factura_id, vencimiento, desde, hasta),
                (tercero_nif, tercero, deducible, motivo_fiscal, entry_id, date),
                (rev, rev_date, rev_year),
            )| {
                let entry = accounting::load_entry(conn, entry_id)?.lines;
                Ok(Ajuste {
                    id,
                    year,
                    tipo: Tipo::from_code(&tipo)
                        .ok_or_else(|| other("tipo de ajuste no válido"))?,
                    descripcion,
                    importe: Money::from_cents(importe),
                    cuenta,
                    contrapartida,
                    factura_kind,
                    factura_id,
                    vencimiento,
                    servicio_desde: desde.as_deref().map(fecha).transpose()?,
                    servicio_hasta: hasta.as_deref().map(fecha).transpose()?,
                    tercero_nif,
                    tercero,
                    deducible: deducible == 1,
                    motivo_fiscal,
                    entry_id,
                    fecha: fecha(&date)?,
                    lines: proposed(conn, &entry)?,
                    reversion_entry_id: rev,
                    reversion_fecha: rev_date.as_deref().map(fecha).transpose()?,
                    reversion_year: rev_year,
                })
            },
        )
        .collect()
}

/// Los ajustes de `year`.
pub fn ajustes(v: &Vault, year: i32) -> Result<Vec<Ajuste>> {
    load(v.connection(), "a.year = ?1", [year])
}

/// Deterioros y provisiones de ejercicios anteriores a `year` sin revertir (se pueden revertir
/// en el cierre de `year`) y los revertidos en `year` (para deshacer la reversión).
pub fn anteriores(v: &Vault, year: i32) -> Result<Vec<Ajuste>> {
    load(
        v.connection(),
        "a.year < ?1 AND a.tipo IN ('deterioro', 'provision')
           AND (a.reversal_year IS NULL OR a.reversal_year = ?1)",
        [year],
    )
}

// ---------- Propuestas ----------

/// Factura del ejercicio que se puede periodificar (o ya periodificada).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Periodificable {
    /// `recibida` (gasto anticipado, 480) o `emitida` (ingreso anticipado, 485).
    pub kind: String,
    pub factura_id: i64,
    pub numero: String,
    pub tercero: String,
    pub fecha: Date,
    /// Importe en cada cuenta de gasto (6xx) o ingreso (7xx) de su asiento.
    pub cuentas: Vec<ProposedLine>,
    pub importe: Money,
    /// Periodo propuesto: el de la periodificación del año anterior (servicio de varios años).
    pub desde: Option<Date>,
    pub hasta: Option<Date>,
    /// Ajuste de este ejercicio que ya la periodifica.
    pub ajuste_id: Option<i64>,
}

/// Cobro vencido hace más de seis meses al cierre sin deterioro.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Deteriorable {
    pub factura_id: i64,
    pub vencimiento: i64,
    pub numero: String,
    pub cliente: String,
    pub nif: Option<String>,
    pub cuenta: Option<String>,
    pub vence: Date,
    /// Días vencido al cierre.
    pub dias: i64,
    pub importe: Money,
    /// Cliente vinculado (socio, administrador, participada o ficha de vinculadas).
    pub vinculada: bool,
}

/// Factura de después del cierre que puede ser de un gasto o un ingreso del ejercicio
/// (propuesta de factura pendiente de recibir o de emitir).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Sugerencia {
    pub kind: String,
    pub factura_id: i64,
    pub numero: String,
    pub tercero: String,
    pub nif: Option<String>,
    pub fecha: Date,
    pub base: Money,
    /// Cuenta de gasto o ingreso de su asiento (si ya está contabilizada).
    pub cuenta: Option<String>,
}

/// Correcciones al resultado contable en el 200 por los ajustes no deducibles.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Fiscal {
    /// Deterioros y provisiones no deducibles dotados en el ejercicio.
    pub aumentos: Money,
    /// Reversiones en el ejercicio de deterioros y provisiones que no fueron deducibles.
    pub disminuciones: Money,
    pub detalle: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Resumen {
    pub year: i32,
    pub start: Date,
    pub end: Date,
    /// Fecha de las reversiones automáticas (el día siguiente al cierre).
    pub reversion: Date,
    pub closed: bool,
    pub ajustes: Vec<Ajuste>,
    pub anteriores: Vec<Ajuste>,
    pub periodificables: Vec<Periodificable>,
    pub deteriorables: Vec<Deteriorable>,
    pub sugerencias: Vec<Sugerencia>,
    /// Saldo de la 490 al empezar el ejercicio (deterioros de años anteriores).
    pub deterioro_anterior: Money,
    pub fiscal: Fiscal,
}

/// Lo que hay que revisar en los ajustes de cierre de `year`. `todas`: proponer como
/// periodificables todas las facturas del ejercicio, no sólo las de las cuentas habituales.
pub fn resumen(v: &Vault, year: i32, todas: bool) -> Result<Resumen> {
    let conn = v.connection();
    let fy = accounting::fiscal_year(v, year)?;
    let deterioro_anterior: i64 = conn.query_row(
        "SELECT COALESCE(SUM(l.credit - l.debit), 0) FROM journal_line l
           JOIN journal_entry e ON e.id = l.entry_id
          WHERE e.fiscal_year = ?1 AND e.origin_kind = 'Apertura' AND l.account LIKE '490%'",
        [year],
        |r| r.get(0),
    )?;
    Ok(Resumen {
        year,
        start: fy.start,
        end: fy.end,
        reversion: fy.end + Duration::days(1),
        closed: closed(conn, year)?,
        ajustes: ajustes(v, year)?,
        anteriores: anteriores(v, year)?,
        periodificables: periodificables(v, &fy, todas)?,
        deteriorables: deteriorables(v, &fy)?,
        sugerencias: sugerencias(conn, &fy)?,
        deterioro_anterior: Money::from_cents(deterioro_anterior),
        fiscal: fiscal(conn, year)?,
    })
}

/// Importe de cada cuenta de gasto (recibida) o de ingreso (emitida) en el asiento de la
/// factura, en su lado natural.
fn invoice_accounts(conn: &Connection, kind: &str, id: i64) -> Result<Vec<(String, Money)>> {
    let (origin, prefix, sign) = if kind == "recibida" {
        ("FacturaRecibida", "6", 1)
    } else {
        ("FacturaEmitida", "7", -1)
    };
    let mut st = conn.prepare(
        "SELECT l.account, SUM(l.debit - l.credit) FROM journal_line l
           JOIN journal_entry e ON e.id = l.entry_id
          WHERE e.origin_kind = ?1 AND e.origin_id = ?2 AND l.account LIKE ?3 || '%'
          GROUP BY l.account ORDER BY l.account",
    )?;
    let rows = st.query_map(params![origin, id, prefix], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (a, c) = r?;
        if c * sign > 0 {
            out.push((a, Money::from_cents(c * sign)));
        }
    }
    Ok(out)
}

/// (número, tercero, fecha) de una factura recibida o emitida.
fn invoice_head(conn: &Connection, kind: &str, id: i64) -> Result<Option<(String, String, Date)>> {
    let sql = if kind == "recibida" {
        "SELECT COALESCE(number, ''), COALESCE(supplier_name, ''), issue_date FROM invoice_received
          WHERE id = ?1 AND status = 'revisada' AND issue_date IS NOT NULL"
    } else {
        "SELECT number, COALESCE(recipient_name, 'Clientes varios'), issue_date FROM invoice_issued
          WHERE id = ?1 AND status = 'emitida'"
    };
    let row: Option<(String, String, String)> = conn
        .query_row(sql, [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .optional()?;
    row.map(|(n, t, d)| Ok((n, t, fecha(&d)?))).transpose()
}

fn periodificables(v: &Vault, fy: &FiscalYear, todas: bool) -> Result<Vec<Periodificable>> {
    let conn = v.connection();
    let mut out = Vec::new();
    for (kind, tipo, sql, habituales) in [
        (
            "recibida",
            Tipo::GastoAnticipado,
            "SELECT id FROM invoice_received WHERE status = 'revisada'
               AND issue_date BETWEEN ?1 AND ?2 ORDER BY issue_date, id",
            &PERIODIFICABLES_GASTO[..],
        ),
        (
            "emitida",
            Tipo::IngresoAnticipado,
            "SELECT id FROM invoice_issued WHERE status = 'emitida'
               AND issue_date BETWEEN ?1 AND ?2 ORDER BY issue_date, id",
            &PERIODIFICABLES_INGRESO[..],
        ),
    ] {
        let mut ids: Vec<(i64, Option<(Date, Date)>)> = {
            let mut st = conn.prepare(sql)?;
            let rows = st.query_map(params![fy.start.to_string(), fy.end.to_string()], |r| {
                r.get::<_, i64>(0)
            })?;
            rows.map(|r| r.map(|id| (id, None)))
                .collect::<std::result::Result<_, _>>()?
        };
        // Las periodificadas el año anterior cuyo servicio pasa también de este cierre (un
        // seguro de dos años): se vuelven a proponer con el mismo periodo.
        let mut st = conn.prepare(
            "SELECT factura_id, servicio_desde, servicio_hasta FROM ajuste_cierre
              WHERE year = ?1 AND tipo = ?2 AND factura_kind = ?3 AND servicio_hasta > ?4",
        )?;
        let rows = st.query_map(
            params![fy.year - 1, tipo.code(), kind, fy.end.to_string()],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )?;
        for r in rows {
            let (id, d, h) = r?;
            ids.retain(|(x, _)| *x != id);
            ids.insert(0, (id, Some((fecha(&d)?, fecha(&h)?))));
        }
        for (id, periodo) in ids {
            let cuentas = invoice_accounts(conn, kind, id)?;
            if cuentas.is_empty() {
                continue;
            }
            let ajuste_id: Option<i64> = conn
                .query_row(
                    "SELECT id FROM ajuste_cierre WHERE year = ?1 AND tipo = ?2
                       AND factura_kind = ?3 AND factura_id = ?4",
                    params![fy.year, tipo.code(), kind, id],
                    |r| r.get(0),
                )
                .optional()?;
            let habitual = cuentas
                .iter()
                .any(|(a, _)| habituales.iter().any(|p| a.starts_with(p)));
            if !(todas || habitual || periodo.is_some() || ajuste_id.is_some()) {
                continue;
            }
            let Some((numero, tercero, fecha)) = invoice_head(conn, kind, id)? else {
                continue;
            };
            let importe = cuentas.iter().map(|(_, m)| *m).sum();
            out.push(Periodificable {
                kind: kind.into(),
                factura_id: id,
                numero,
                tercero,
                fecha,
                cuentas: cuentas
                    .iter()
                    .map(|(a, m)| {
                        Ok(ProposedLine {
                            name: accounting::account_name(conn, a)?.unwrap_or_default(),
                            account: a.clone(),
                            debit: *m,
                            credit: Money::ZERO,
                        })
                    })
                    .collect::<Result<_>>()?,
                importe,
                desde: periodo.map(|p| p.0),
                hasta: periodo.map(|p| p.1),
                ajuste_id,
            });
        }
    }
    Ok(out)
}

/// ¿Es vinculado el NIF (ficha de vinculadas, socio o participada con el 25 % o más)?
pub(crate) fn vinculado(conn: &Connection, nif: &str) -> Result<bool> {
    let k = crate::vinculadas::clave_nif(nif);
    if k.is_empty() {
        return Ok(false);
    }
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM vinculada WHERE nif = ?1 AND active = 1)
             OR EXISTS(SELECT 1 FROM shareholder WHERE upper(replace(replace(nif, ' ', ''), '-', '')) = ?1
                          AND active = 1 AND pct >= 250000)
             OR EXISTS(SELECT 1 FROM participada WHERE nif = ?1 AND active = 1 AND pct >= 250000)",
        [k],
        |r| r.get(0),
    )?)
}

fn nif_of_account(conn: &Connection, account: Option<&str>) -> Result<Option<String>> {
    let Some(a) = account else { return Ok(None) };
    Ok(conn
        .query_row(
            "SELECT tax_id FROM third_party_account WHERE account = ?1",
            [a],
            |r| r.get(0),
        )
        .optional()?)
}

/// El cobro `seq` de la emitida `id` tal como estaba al cierre: (vencimiento, importe, número,
/// cliente, subcuenta) si seguía pendiente.
fn cobro_al_cierre(
    v: &Vault,
    fy: &FiscalYear,
    id: i64,
    seq: i64,
) -> Result<Option<crate::cartera::DueItem>> {
    Ok(crate::cartera::list(v, fy.end)?.into_iter().find(|i| {
        i.kind == "emitida"
            && i.invoice_id == id
            && i.seq == seq
            && i.amount.cents() > 0
            && i.issue_date <= fy.end
            && (i.status == "pendiente" || i.settled_on.is_some_and(|d| d > fy.end))
    }))
}

fn seis_meses(vence: Date, cierre: Date) -> bool {
    crate::recurring::months_after(vence, 6) <= cierre
}

/// Abonos pendientes al cierre de las rectificativas de cada emitida (en positivo): lo que hay
/// que restar de lo que se debe. Con una R3 o una R2, la cuota del IVA recuperado (art. 80.Tres
/// y Cuatro LIVA): el deterioro es sólo por la base.
fn abonos_al_cierre(
    conn: &Connection,
    items: &[crate::cartera::DueItem],
    fy: &FiscalYear,
) -> Result<std::collections::HashMap<i64, Money>> {
    let mut out = std::collections::HashMap::new();
    for i in items {
        let pendiente = i.status == "pendiente" || i.settled_on.is_some_and(|d| d > fy.end);
        if i.kind != "emitida" || !pendiente || !i.amount.is_negative() || i.issue_date > fy.end {
            continue;
        }
        let original: Option<i64> = conn
            .query_row(
                "SELECT rectifies_id FROM invoice_issued WHERE id = ?1",
                [i.invoice_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        if let Some(o) = original {
            *out.entry(o).or_insert(Money::ZERO) += -i.amount;
        }
    }
    Ok(out)
}

fn deteriorables(v: &Vault, fy: &FiscalYear) -> Result<Vec<Deteriorable>> {
    let conn = v.connection();
    let mut out = Vec::new();
    let items = crate::cartera::list(v, fy.end)?;
    let mut abonos = abonos_al_cierre(conn, &items, fy)?;
    for mut i in items {
        let pendiente = i.status == "pendiente" || i.settled_on.is_some_and(|d| d > fy.end);
        if i.kind != "emitida"
            || !pendiente
            || i.amount.cents() <= 0
            || i.issue_date > fy.end
            || !seis_meses(i.due_date, fy.end)
        {
            continue;
        }
        // Ya deteriorado (este año o antes, sin revertir).
        let hecho: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM ajuste_cierre WHERE tipo = 'deterioro'
               AND factura_kind = 'emitida' AND factura_id = ?1 AND vencimiento = ?2
               AND year <= ?3 AND (reversal_year IS NULL OR reversal_year > ?3))",
            params![i.invoice_id, i.seq, fy.year],
            |r| r.get(0),
        )?;
        if hecho {
            continue;
        }
        // Lo abonado por sus rectificativas (el IVA recuperado con una R3) sale de lo pendiente.
        if let Some(a) = abonos.get_mut(&i.invoice_id) {
            let menos = (*a).min(i.amount);
            i.amount -= menos;
            *a -= menos;
            if i.amount.cents() <= 0 {
                continue;
            }
        }
        let nif = nif_of_account(conn, i.party_account.as_deref())?;
        let vinculada = match &nif {
            Some(n) => vinculado(conn, n)?,
            None => false,
        };
        out.push(Deteriorable {
            factura_id: i.invoice_id,
            vencimiento: i.seq,
            numero: i.number,
            cliente: i.party,
            nif,
            cuenta: i.party_account,
            vence: i.due_date,
            dias: (fy.end - i.due_date).whole_days(),
            importe: i.amount,
            vinculada,
        });
    }
    Ok(out)
}

/// Facturas de los dos meses siguientes al cierre (recibidas, revisadas o no, y emitidas) que
/// pueden ser de un gasto o un ingreso devengado en el ejercicio.
fn sugerencias(conn: &Connection, fy: &FiscalYear) -> Result<Vec<Sugerencia>> {
    let desde = (fy.end + Duration::days(1)).to_string();
    let hasta = crate::recurring::months_after(fy.end + Duration::days(1), 2).to_string();
    let mut out = Vec::new();
    for (kind, tipo, sql) in [
        (
            "recibida",
            Tipo::PendienteRecibir,
            "SELECT id, COALESCE(number, ''), COALESCE(supplier_name, ''), supplier_nif,
                    issue_date, COALESCE(base, 0)
               FROM invoice_received WHERE status IN ('pendiente', 'revisada')
                AND issue_date >= ?1 AND issue_date < ?2 AND COALESCE(base, 0) > 0
              ORDER BY issue_date, id",
        ),
        (
            "emitida",
            Tipo::PendienteEmitir,
            "SELECT id, number, COALESCE(recipient_name, 'Clientes varios'), recipient_tax_id,
                    issue_date, base
               FROM invoice_issued WHERE status = 'emitida'
                AND issue_date >= ?1 AND issue_date < ?2 AND base > 0
              ORDER BY issue_date, id",
        ),
    ] {
        let mut st = conn.prepare(sql)?;
        #[allow(clippy::type_complexity)]
        let rows: Vec<(i64, String, String, Option<String>, String, i64)> = st
            .query_map(params![desde, hasta], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })?
            .collect::<std::result::Result<_, _>>()?;
        for (id, numero, tercero, nif, date, base) in rows {
            let hecho: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM ajuste_cierre WHERE year = ?1 AND tipo = ?2
                   AND factura_kind = ?3 AND factura_id = ?4)",
                params![fy.year, tipo.code(), kind, id],
                |r| r.get(0),
            )?;
            if hecho {
                continue;
            }
            out.push(Sugerencia {
                kind: kind.into(),
                factura_id: id,
                numero,
                tercero,
                nif,
                fecha: fecha(&date)?,
                base: Money::from_cents(base),
                cuenta: invoice_accounts(conn, kind, id)?
                    .into_iter()
                    .max_by_key(|(_, m)| *m)
                    .map(|(a, _)| a),
            });
        }
    }
    Ok(out)
}

/// Una corrección al resultado contable en el 200 por un ajuste no deducible: la dotación en
/// aumento en el ejercicio del ajuste y su reversión en disminución en el de la reversión.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Correccion200 {
    /// Id del ajuste (`ajuste_cierre`).
    pub id: i64,
    pub tipo: Tipo,
    pub descripcion: String,
    pub importe: Money,
    /// Ejercicio en que se dotó.
    pub dotado: i32,
    /// `false`: dotación (aumento); `true`: reversión (disminución).
    pub reversion: bool,
}

impl Correccion200 {
    /// Casilla de la página de correcciones del 200 (catálogo `m200_catalogo`): deterioro de
    /// créditos no deducible [00321] y su reversión [00322] (art. 13.1 LIS); provisiones no
    /// deducibles [00337] y su aplicación o exceso [00338] (art. 14 LIS).
    pub fn casilla(&self) -> u16 {
        match (self.tipo, self.reversion) {
            (Tipo::Deterioro, false) => 321,
            (Tipo::Deterioro, true) => 322,
            (_, false) => 337,
            (_, true) => 338,
        }
    }
}

/// Correcciones al resultado contable de `year` por los deterioros y provisiones no deducibles
/// (las propone el 200, `modelo200::preparar`).
pub fn correcciones_200(conn: &Connection, year: i32) -> Result<Vec<Correccion200>> {
    let mut st = conn.prepare(
        "SELECT id, tipo, descripcion, importe, year, reversal_year FROM ajuste_cierre
          WHERE deducible = 0 AND tipo IN ('deterioro', 'provision')
            AND (year = ?1 OR reversal_year = ?1) ORDER BY id",
    )?;
    let rows = st.query_map([year], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, i32>(4)?,
            r.get::<_, Option<i32>>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (id, tipo, descripcion, cents, y, rev) = r?;
        let tipo = Tipo::from_code(&tipo).ok_or_else(|| other("tipo de ajuste no válido"))?;
        let c = Correccion200 {
            id,
            tipo,
            descripcion,
            importe: Money::from_cents(cents),
            dotado: y,
            reversion: false,
        };
        if y == year {
            out.push(c.clone());
        }
        if rev == Some(year) {
            out.push(Correccion200 {
                reversion: true,
                ..c
            });
        }
    }
    Ok(out)
}

/// Correcciones extracontables de `year` por los deterioros y provisiones no deducibles.
pub fn fiscal(conn: &Connection, year: i32) -> Result<Fiscal> {
    let mut f = Fiscal::default();
    for c in correcciones_200(conn, year)? {
        let label = c.tipo.label();
        let (desc, m) = (&c.descripcion, c.importe);
        if c.reversion {
            f.disminuciones += m;
            f.detalle.push(format!(
                "Reversión de {} no deducible de {} «{desc}»: disminución de {m}",
                label.to_lowercase(),
                c.dotado
            ));
        } else {
            f.aumentos += m;
            f.detalle
                .push(format!("{label} no deducible «{desc}»: aumento de {m}"));
        }
    }
    Ok(f)
}

// ---------- Alta, deshacer y reversión ----------

/// Lo que teclea o elige el usuario para un ajuste nuevo.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct Nuevo {
    pub tipo: Option<Tipo>,
    pub descripcion: String,
    /// Importe: en las facturas pendientes y las provisiones; en un deterioro, si es menos que
    /// lo pendiente. En una periodificación se calcula.
    pub importe: Option<Money>,
    /// Cuenta de gasto o ingreso (facturas pendientes y provisiones).
    pub cuenta: Option<String>,
    /// Cuenta de la provisión (14x, 499, 529x).
    pub contrapartida: Option<String>,
    /// Factura periodificada (recibida o emitida según el tipo) o emitida deteriorada.
    pub factura_id: Option<i64>,
    /// Factura de enero que se propone como pendiente de recibir o de emitir.
    pub factura_kind: Option<String>,
    pub vencimiento: Option<i64>,
    pub desde: Option<Date>,
    pub hasta: Option<Date>,
    pub tercero_nif: Option<String>,
    pub tercero: Option<String>,
    /// Provisión deducible (art. 14 LIS); por defecto, no.
    pub deducible: Option<bool>,
    pub motivo: Option<Motivo>,
    pub exclusion: Option<Exclusion>,
}

/// Asiento propuesto de un ajuste (para enseñarlo antes de contabilizarlo).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Propuesta {
    pub tipo: Tipo,
    pub descripcion: String,
    pub importe: Money,
    pub fecha: Date,
    pub lines: Vec<ProposedLine>,
    /// Reversión automática (el día siguiente al cierre), si la lleva.
    pub reversion_fecha: Option<Date>,
    pub reversion: Vec<ProposedLine>,
    pub deducible: bool,
    pub motivo_fiscal: String,
    pub avisos: Vec<String>,
}

struct Construido {
    tipo: Tipo,
    descripcion: String,
    importe: Money,
    cuenta: String,
    contrapartida: String,
    factura: Option<(&'static str, i64)>,
    vencimiento: Option<i64>,
    periodo: Option<(Date, Date)>,
    tercero_nif: Option<String>,
    tercero: Option<String>,
    deducible: bool,
    motivo_fiscal: String,
    lines: Vec<EntryLine>,
    reversion: Option<Vec<EntryLine>>,
    avisos: Vec<String>,
}

fn importe_positivo(m: Option<Money>) -> Result<Money> {
    match m {
        Some(m) if m.cents() > 0 => Ok(m),
        _ => Err(other("indica un importe mayor que cero")),
    }
}

fn cuenta_de(s: Option<&str>, prefix: char, que: &str) -> Result<String> {
    let s = s.map(str::trim).unwrap_or_default();
    if s.is_empty() || !s.starts_with(prefix) {
        return Err(other(format!(
            "la cuenta de {que} ha de ser del grupo {prefix} (p. ej. {})",
            if prefix == '6' { "629" } else { "705" }
        )));
    }
    Ok(code(s)?.as_str().to_owned())
}

fn construir(v: &Vault, fy: &FiscalYear, n: &Nuevo) -> Result<Construido> {
    let conn = v.connection();
    let tipo = n.tipo.ok_or_else(|| other("elige el tipo de ajuste"))?;
    let descripcion = n.descripcion.trim().to_owned();
    let mut avisos = Vec::new();
    let nada = |tipo| Construido {
        tipo,
        descripcion: descripcion.clone(),
        importe: Money::ZERO,
        cuenta: String::new(),
        contrapartida: String::new(),
        factura: None,
        vencimiento: None,
        periodo: None,
        tercero_nif: n.tercero_nif.clone().filter(|s| !s.trim().is_empty()),
        tercero: n.tercero.clone().filter(|s| !s.trim().is_empty()),
        deducible: true,
        motivo_fiscal: String::new(),
        lines: Vec::new(),
        reversion: None,
        avisos: Vec::new(),
    };
    let mut c = nada(tipo);
    match tipo {
        Tipo::GastoAnticipado | Tipo::IngresoAnticipado => {
            let gasto = tipo == Tipo::GastoAnticipado;
            let kind = if gasto { "recibida" } else { "emitida" };
            let id = n
                .factura_id
                .ok_or_else(|| other("elige la factura que periodificas"))?;
            let (Some(desde), Some(hasta)) = (n.desde, n.hasta) else {
                return Err(other(
                    "indica el periodo de servicio de la factura (desde y hasta)",
                ));
            };
            if hasta < desde {
                return Err(other("el periodo de servicio acaba antes de empezar"));
            }
            if hasta <= fy.end {
                return Err(other(format!(
                    "el servicio acaba antes del cierre ({}): no hay nada que periodificar",
                    fy.end
                )));
            }
            let (numero, tercero, fecha_factura) = invoice_head(conn, kind, id)?
                .ok_or_else(|| other("la factura no existe, está anulada o sin revisar"))?;
            if fecha_factura > fy.end {
                return Err(other(
                    "la factura es posterior al cierre: no se periodifica en este ejercicio",
                ));
            }
            let cuentas = invoice_accounts(conn, kind, id)?;
            if cuentas.is_empty() {
                return Err(other(format!(
                    "la factura no tiene asiento con cuentas de {}",
                    if gasto { "gasto (6)" } else { "ingreso (7)" }
                )));
            }
            let partes: Vec<(String, Money)> = cuentas
                .iter()
                .map(|(a, m)| (a.clone(), parte_siguiente(*m, desde, hasta, fy.end)))
                .filter(|(_, m)| m.cents() > 0)
                .collect();
            let importe: Money = partes.iter().map(|(_, m)| *m).sum();
            if importe.cents() <= 0 {
                return Err(other("no hay importe que periodificar"));
            }
            let balance = if gasto {
                GASTOS_ANTICIPADOS
            } else {
                INGRESOS_ANTICIPADOS
            };
            let mut lines = Vec::new();
            if gasto {
                lines.push(EntryLine::debit(code(balance)?, importe));
                for (a, m) in &partes {
                    lines.push(EntryLine::credit(code(a)?, *m));
                }
            } else {
                for (a, m) in &partes {
                    lines.push(EntryLine::debit(code(a)?, *m));
                }
                lines.push(EntryLine::credit(code(balance)?, importe));
            }
            let siguiente = accounting::fiscal_year_at(v, fy.end + Duration::days(1))?;
            if hasta > siguiente.end {
                avisos.push(format!(
                    "El servicio pasa también del cierre de {} ({}): esa parte es a largo plazo. \
                     Queda en la {balance}; al cierre del año que viene se propone de nuevo.",
                    siguiente.year, siguiente.end
                ));
            }
            if desde > fy.end {
                avisos.push(
                    "Todo el servicio es del ejercicio siguiente: se periodifica entero.".into(),
                );
            }
            let dias = (hasta - desde).whole_days() + 1;
            let despues = (hasta - fy.end).whole_days().min(dias);
            c.descripcion = if descripcion.is_empty() {
                format!(
                    "{} {numero} {tercero}: {despues} de {dias} días del servicio son del \
                     ejercicio siguiente",
                    if gasto { "Recibida" } else { "Emitida" }
                )
            } else {
                descripcion.clone()
            };
            c.importe = importe;
            c.cuenta = partes
                .iter()
                .max_by_key(|(_, m)| *m)
                .map(|(a, _)| a.clone())
                .unwrap_or_default();
            c.contrapartida = balance.into();
            c.factura = Some((kind, id));
            c.periodo = Some((desde, hasta));
            c.tercero = Some(tercero);
            c.reversion = Some(mirror(&lines));
            c.lines = lines;
        }
        Tipo::PendienteRecibir | Tipo::PendienteEmitir => {
            let gasto = tipo == Tipo::PendienteRecibir;
            let importe = importe_positivo(n.importe)?;
            let cuenta = cuenta_de(
                n.cuenta.as_deref(),
                if gasto { '6' } else { '7' },
                if gasto { "gasto" } else { "ingreso" },
            )?;
            if descripcion.is_empty() {
                return Err(other("describe el gasto o el ingreso devengado"));
            }
            let balance = if gasto {
                PENDIENTES_RECIBIR
            } else {
                PENDIENTES_EMITIR
            };
            let lines = if gasto {
                vec![
                    EntryLine::debit(code(&cuenta)?, importe),
                    EntryLine::credit(code(balance)?, importe),
                ]
            } else {
                vec![
                    EntryLine::debit(code(balance)?, importe),
                    EntryLine::credit(code(&cuenta)?, importe),
                ]
            };
            if let (Some(k), Some(id)) = (n.factura_kind.as_deref(), n.factura_id) {
                let k: &'static str = match k {
                    "recibida" if gasto => "recibida",
                    "emitida" if !gasto => "emitida",
                    _ => return Err(other("la factura no es de ese tipo")),
                };
                c.factura = Some((k, id));
            }
            c.importe = importe;
            c.cuenta = cuenta;
            c.contrapartida = balance.into();
            c.reversion = Some(mirror(&lines));
            c.lines = lines;
        }
        Tipo::Deterioro => {
            let id = n
                .factura_id
                .ok_or_else(|| other("elige el cobro pendiente que deterioras"))?;
            let seq = n.vencimiento.unwrap_or(1);
            let item = cobro_al_cierre(v, fy, id, seq)?.ok_or_else(|| {
                other("ese cobro no estaba pendiente al cierre (o no es de una factura emitida)")
            })?;
            let hecho: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM ajuste_cierre WHERE tipo = 'deterioro'
                   AND factura_kind = 'emitida' AND factura_id = ?1 AND vencimiento = ?2
                   AND year <= ?3 AND (reversal_year IS NULL OR reversal_year > ?3))",
                params![id, seq, fy.year],
                |r| r.get(0),
            )?;
            if hecho {
                return Err(other("ese cobro ya tiene un deterioro sin revertir"));
            }
            // Menos lo abonado por sus rectificativas (el IVA recuperado con una R3), en los
            // primeros vencimientos.
            let mut item = item;
            {
                let items = crate::cartera::list(v, fy.end)?;
                let mut abono = abonos_al_cierre(conn, &items, fy)?
                    .get(&id)
                    .copied()
                    .unwrap_or(Money::ZERO);
                for i in items.iter().filter(|i| {
                    i.kind == "emitida" && i.invoice_id == id && i.amount.cents() > 0 && i.seq < seq
                }) {
                    abono -= abono.min(i.amount);
                }
                item.amount -= abono.min(item.amount);
            }
            if item.amount.cents() <= 0 {
                return Err(other(
                    "lo pendiente de ese cobro ya está abonado por una rectificativa",
                ));
            }
            let importe = match n.importe {
                None => item.amount,
                Some(m) if m.cents() > 0 && m <= item.amount => m,
                Some(_) => {
                    return Err(other(format!(
                        "el deterioro ha de ser mayor que cero y no más de lo pendiente ({})",
                        item.amount
                    )));
                }
            };
            let nif = nif_of_account(conn, item.party_account.as_deref())?;
            let vinc = match &nif {
                Some(x) => vinculado(conn, x)?,
                None => false,
            };
            let exclusion = match n.exclusion {
                Some(e) => e,
                None if vinc => Exclusion::Vinculada,
                None => Exclusion::Ninguna,
            };
            if vinc && exclusion != Exclusion::Vinculada {
                avisos.push(
                    "El cliente figura como vinculado (socio, participada o ficha de \
                     vinculadas): su deterioro sólo es deducible en concurso con la fase de \
                     liquidación abierta."
                        .into(),
                );
            }
            let motivo = n.motivo.unwrap_or_default();
            let six = seis_meses(item.due_date, fy.end);
            let (deducible, motivo_fiscal) = deducible_deterioro(motivo, exclusion, six);
            avisos.push(
                "Si recuperas el IVA de la factura con una rectificativa por crédito incobrable \
                 (art. 80.Cuatro LIVA, en Cobros y pagos › Créditos incobrables), el deterioro es \
                 sólo por la base; si la R3 ya está emitida, TaxPain ya lo ha descontado."
                    .into(),
            );
            let lines = vec![
                EntryLine::debit(code(PERDIDA_DETERIORO)?, importe),
                EntryLine::credit(code(DETERIORO)?, importe),
            ];
            c.descripcion = if descripcion.is_empty() {
                format!(
                    "Deterioro del cobro de la {} de {} (vencido el {})",
                    item.number, item.party, item.due_date
                )
            } else {
                descripcion.clone()
            };
            c.importe = importe;
            c.cuenta = PERDIDA_DETERIORO.into();
            c.contrapartida = DETERIORO.into();
            c.factura = Some(("emitida", id));
            c.vencimiento = Some(seq);
            c.tercero_nif = nif;
            c.tercero = Some(item.party);
            c.deducible = deducible;
            c.motivo_fiscal = motivo_fiscal;
            c.lines = lines;
        }
        Tipo::Provision => {
            let importe = importe_positivo(n.importe)?;
            let cuenta = cuenta_de(n.cuenta.as_deref(), '6', "gasto")?;
            let prov = n
                .contrapartida
                .as_deref()
                .map(str::trim)
                .unwrap_or_default();
            if !provision_valida(prov) {
                return Err(other(
                    "la cuenta de la provisión ha de ser la 141, 142 o 145 (largo plazo), la \
                     5291, 5292 o 5295 (corto plazo) o la 499 (operaciones comerciales)",
                ));
            }
            if descripcion.is_empty() {
                return Err(other("describe el riesgo o el litigio que provisionas"));
            }
            let deducible = n.deducible.unwrap_or(false);
            c.motivo_fiscal = if deducible {
                "Marcada como deducible (art. 14 LIS): guarda la justificación.".into()
            } else {
                "No deducible (art. 14 LIS): corrección en aumento en el 200 de este ejercicio y \
                 en disminución cuando se aplique o se revierta."
                    .into()
            };
            let lines = vec![
                EntryLine::debit(code(&cuenta)?, importe),
                EntryLine::credit(code(prov)?, importe),
            ];
            c.importe = importe;
            c.cuenta = cuenta;
            c.contrapartida = code(prov)?.as_str().to_owned();
            c.deducible = deducible;
            c.lines = lines;
        }
    }
    c.avisos = avisos;
    if c.descripcion.is_empty() {
        c.descripcion = tipo.label().into();
    }
    Ok(c)
}

/// El asiento que saldría del ajuste `n` en el cierre de `year`, sin grabarlo.
pub fn propuesta(v: &Vault, year: i32, n: &Nuevo) -> Result<Propuesta> {
    let fy = accounting::fiscal_year(v, year)?;
    let c = construir(v, &fy, n)?;
    let conn = v.connection();
    Ok(Propuesta {
        tipo: c.tipo,
        descripcion: c.descripcion,
        importe: c.importe,
        fecha: fy.end,
        lines: proposed(conn, &c.lines)?,
        reversion_fecha: c.reversion.as_ref().map(|_| fy.end + Duration::days(1)),
        reversion: c
            .reversion
            .as_deref()
            .map(|l| proposed(conn, l))
            .transpose()?
            .unwrap_or_default(),
        deducible: c.deducible,
        motivo_fiscal: c.motivo_fiscal,
        avisos: c.avisos,
    })
}

fn closed_error(year: i32) -> AppError {
    other(format!(
        "el ejercicio {year} está cerrado: reábrelo para cambiar sus ajustes de cierre"
    ))
}

/// Contabiliza el ajuste `n` en el cierre de `year` (con confirmación) y, si es de los que se
/// revierten solos, su reversión el día siguiente. Devuelve el id del ajuste.
pub fn crear(v: &Vault, year: i32, n: &Nuevo, now: OffsetDateTime, confirmed: bool) -> Result<i64> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    if closed(conn, year)? {
        return Err(closed_error(year));
    }
    let fy = accounting::fiscal_year(v, year)?;
    let c = construir(v, &fy, n)?;
    if let (Tipo::GastoAnticipado | Tipo::IngresoAnticipado, Some((k, id))) = (c.tipo, c.factura) {
        let ya: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM ajuste_cierre WHERE year = ?1 AND tipo = ?2
               AND factura_kind = ?3 AND factura_id = ?4)",
            params![year, c.tipo.code(), k, id],
            |r| r.get(0),
        )?;
        if ya {
            return Err(other(
                "esa factura ya está periodificada en este cierre: deshazlo para cambiar el periodo",
            ));
        }
    }
    let siguiente = fy.end + Duration::days(1);
    if c.reversion.is_some() {
        let next = accounting::fiscal_year_at(v, siguiente)?;
        if closed(conn, next.year)? {
            return Err(closed_error(next.year));
        }
    }
    let tx = conn.unchecked_transaction()?;
    // El asiento lleva el id del ajuste en su origen y el ajuste apunta al asiento: el id se
    // reserva antes (dentro de la transacción no escribe nadie más).
    let id: i64 = tx.query_row(
        "SELECT COALESCE(MAX(id), 0) + 1 FROM ajuste_cierre",
        [],
        |r| r.get(0),
    )?;
    let entry = record_entry(
        &tx,
        &JournalEntry {
            date: fy.end,
            concept: format!("{}: {}", c.tipo.label(), c.descripcion),
            origin: Origin::AjusteCierre(id),
            lines: c.lines.clone(),
        },
        now,
    )?;
    let reversal = match &c.reversion {
        Some(lines) => Some(record_entry(
            &tx,
            &JournalEntry {
                date: siguiente,
                concept: format!(
                    "Reversión de {}: {}",
                    c.tipo.label().to_lowercase(),
                    c.descripcion
                ),
                origin: Origin::AjusteCierre(id),
                lines: lines.clone(),
            },
            now,
        )?),
        None => None,
    };
    let reversal_year = match reversal {
        Some(_) => Some(accounting::fiscal_year_for_date(&tx, siguiente)?.year),
        None => None,
    };
    tx.execute(
        "INSERT INTO ajuste_cierre(id, year, tipo, descripcion, importe, cuenta, contrapartida,
            factura_kind, factura_id, vencimiento, servicio_desde, servicio_hasta, tercero_nif,
            tercero, deducible, motivo_fiscal, entry_id, reversal_entry_id, reversal_year,
            created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17,
                 ?18, ?19, ?20)",
        params![
            id,
            year,
            c.tipo.code(),
            c.descripcion,
            c.importe.cents(),
            c.cuenta,
            c.contrapartida,
            c.factura.map(|f| f.0),
            c.factura.map(|f| f.1),
            c.vencimiento,
            c.periodo.map(|p| p.0.to_string()),
            c.periodo.map(|p| p.1.to_string()),
            c.tercero_nif,
            c.tercero,
            i64::from(c.deducible),
            c.motivo_fiscal,
            entry,
            reversal,
            reversal_year,
            now_str(now),
        ],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "closing.adjustment",
        &year.to_string(),
        &serde_json::json!({
            "ajuste": id,
            "tipo": c.tipo.code(),
            "importe": c.importe.cents(),
            "entry": entry,
            "reversal": reversal,
            "deducible": c.deducible,
        }),
    )?;
    tx.commit()?;
    Ok(id)
}

/// (ejercicio, tipo, asiento, asiento de la reversión, ejercicio de la reversión).
type Fila = (i32, Tipo, i64, Option<i64>, Option<i32>);

fn row(conn: &Connection, id: i64) -> Result<Fila> {
    let (year, tipo, entry, rev, rev_year): (i32, String, i64, Option<i64>, Option<i32>) = conn
        .query_row(
            "SELECT year, tipo, entry_id, reversal_entry_id, reversal_year FROM ajuste_cierre
              WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let tipo = Tipo::from_code(&tipo).ok_or_else(|| other("tipo de ajuste no válido"))?;
    Ok((year, tipo, entry, rev, rev_year))
}

fn delete_entry(conn: &Connection, id: i64, year: i32) -> Result<()> {
    conn.execute("DELETE FROM journal_line WHERE entry_id = ?1", [id])
        .map_err(|_| closed_error(year))?;
    conn.execute("DELETE FROM journal_entry WHERE id = ?1", [id])
        .map_err(|_| closed_error(year))?;
    Ok(())
}

/// Quita un ajuste (con confirmación): su asiento y, si se revierte solo, la reversión. Un
/// deterioro o una provisión ya revertidos se deshacen antes de su reversión.
pub fn deshacer(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let (year, tipo, entry, rev, rev_year) = row(conn, id)?;
    if closed(conn, year)? {
        return Err(closed_error(year));
    }
    if rev.is_some() && !tipo.automatica() {
        return Err(other(
            "ya está revertido en un ejercicio posterior: deshaz antes la reversión",
        ));
    }
    if let Some(y) = rev_year
        && closed(conn, y)?
    {
        return Err(closed_error(y));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM ajuste_cierre WHERE id = ?1", [id])?;
    if let Some(r) = rev {
        delete_entry(&tx, r, rev_year.unwrap_or(year + 1))?;
    }
    delete_entry(&tx, entry, year)?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "closing.adjustment_undo",
        &year.to_string(),
        &serde_json::json!({ "ajuste": id, "tipo": tipo.code(), "entry": entry, "reversal": rev }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Revierte en el cierre de `year` un deterioro (490 a 794: se cobró o ya no hace falta) o una
/// provisión (14x a 795x: exceso) de un ejercicio anterior. Devuelve el id del asiento.
pub fn revertir(
    v: &Vault,
    id: i64,
    year: i32,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<i64> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let (origen, tipo, _, rev, _) = row(conn, id)?;
    if tipo.automatica() {
        return Err(other(
            "ese ajuste se revierte solo el día siguiente al cierre",
        ));
    }
    if rev.is_some() {
        return Err(other("ya está revertido"));
    }
    if year <= origen {
        return Err(other(format!(
            "se revierte en el cierre de un ejercicio posterior a {origen}"
        )));
    }
    if closed(conn, year)? {
        return Err(closed_error(year));
    }
    let a = load(conn, "a.id = ?1", [id])?
        .pop()
        .ok_or(AppError::NotFound)?;
    let fy = accounting::fiscal_year(v, year)?;
    let (debe, haber) = if tipo == Tipo::Deterioro {
        (a.contrapartida.clone(), REVERSION_DETERIORO.to_owned())
    } else {
        (
            a.contrapartida.clone(),
            cuenta_exceso(&a.contrapartida).to_owned(),
        )
    };
    let tx = conn.unchecked_transaction()?;
    let entry = record_entry(
        &tx,
        &JournalEntry {
            date: fy.end,
            concept: format!(
                "Reversión de {} de {origen}: {}",
                tipo.label().to_lowercase(),
                a.descripcion
            ),
            origin: Origin::AjusteCierre(id),
            lines: vec![
                EntryLine::debit(code(&debe)?, a.importe),
                EntryLine::credit(code(&haber)?, a.importe),
            ],
        },
        now,
    )?;
    tx.execute(
        "UPDATE ajuste_cierre SET reversal_entry_id = ?2, reversal_year = ?3 WHERE id = ?1",
        params![id, entry, year],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "closing.adjustment_reverse",
        &year.to_string(),
        &serde_json::json!({ "ajuste": id, "tipo": tipo.code(), "entry": entry }),
    )?;
    tx.commit()?;
    Ok(entry)
}

/// Quita la reversión de un deterioro o una provisión (con confirmación).
pub fn deshacer_reversion(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let (_, tipo, _, rev, rev_year) = row(conn, id)?;
    if tipo.automatica() {
        return Err(other(
            "la reversión de una periodificación o una factura pendiente va con el ajuste: \
             deshaz el ajuste",
        ));
    }
    let (Some(rev), Some(y)) = (rev, rev_year) else {
        return Err(other("no está revertido"));
    };
    if closed(conn, y)? {
        return Err(closed_error(y));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE ajuste_cierre SET reversal_entry_id = NULL, reversal_year = NULL WHERE id = ?1",
        [id],
    )?;
    delete_entry(&tx, rev, y)?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "closing.adjustment_reverse_undo",
        &y.to_string(),
        &serde_json::json!({ "ajuste": id, "entry": rev }),
    )?;
    tx.commit()?;
    Ok(())
}

// ---------- Comprobaciones previas: saldos anómalos ----------

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SaldoCuenta {
    pub account: String,
    pub name: String,
    /// Saldo deudor (positivo) o acreedor (negativo).
    pub saldo: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Comprobacion {
    pub id: String,
    pub label: String,
    pub ok: bool,
    pub detail: String,
    pub cuentas: Vec<SaldoCuenta>,
    /// Dónde se arregla: una vista de Contabilidad (`mayor`, `banco`, `impuestos`,
    /// `prestamos`, `cartera`…).
    pub ir: String,
    /// Cuenta que abrir en el mayor.
    pub cuenta: Option<String>,
}

/// Saldo de cada cuenta de `year` (con la apertura, sin el cierre) que empieza por alguno de
/// los prefijos.
fn saldos(conn: &Connection, year: i32, prefixes: &[&str]) -> Result<Vec<SaldoCuenta>> {
    let mut out = Vec::new();
    let mut st = conn.prepare(
        "SELECT l.account, SUM(l.debit - l.credit) FROM journal_line l
           JOIN journal_entry e ON e.id = l.entry_id
          WHERE e.fiscal_year = ?1 AND e.origin_kind NOT IN ('Cierre', 'Regularizacion')
            AND l.account LIKE ?2 || '%'
          GROUP BY l.account HAVING SUM(l.debit - l.credit) <> 0 ORDER BY l.account",
    )?;
    for p in prefixes {
        let rows = st.query_map(params![year, p], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })?;
        for r in rows {
            let (account, c) = r?;
            out.push(SaldoCuenta {
                name: accounting::account_name(conn, &account)?.unwrap_or_default(),
                account,
                saldo: Money::from_cents(c),
            });
        }
    }
    Ok(out)
}

/// Saldos que conviene mirar antes de cerrar `year`, con dónde se arreglan.
pub fn comprobaciones(v: &Vault, year: i32) -> Result<Vec<Comprobacion>> {
    let conn = v.connection();
    let mut out = Vec::new();
    let mut push = |id: &str,
                    label: &str,
                    cuentas: Vec<SaldoCuenta>,
                    ok_text: &str,
                    ko_text: &str,
                    ir: &str| {
        let ok = cuentas.is_empty();
        let cuenta = cuentas.first().map(|c| c.account.clone());
        out.push(Comprobacion {
            id: id.into(),
            label: label.into(),
            ok,
            detail: if ok { ok_text } else { ko_text }.into(),
            cuentas,
            ir: ir.into(),
            cuenta,
        });
    };
    let deudores = |v: Vec<SaldoCuenta>| -> Vec<SaldoCuenta> {
        v.into_iter().filter(|c| c.saldo.cents() > 0).collect()
    };
    let acreedores = |v: Vec<SaldoCuenta>| -> Vec<SaldoCuenta> {
        v.into_iter().filter(|c| c.saldo.cents() < 0).collect()
    };
    push(
        "proveedores",
        "Proveedores y acreedores sin saldo deudor",
        deudores(saldos(conn, year, &["400", "401", "410", "411"])?),
        "Ningún proveedor ni acreedor tiene saldo deudor.",
        "Tienen saldo deudor (se les ha pagado más de lo facturado): suele ser una factura sin \
         registrar, un pago duplicado o un anticipo, que va a la 407. Míralo en el mayor.",
        "mayor",
    );
    push(
        "clientes",
        "Clientes sin saldo acreedor",
        acreedores(saldos(conn, year, &["430", "431", "440"])?),
        "Ningún cliente tiene saldo acreedor.",
        "Tienen saldo acreedor (han pagado más de lo facturado): un cobro sin factura, uno \
         duplicado o un anticipo, que va a la 438. Míralo en el mayor.",
        "mayor",
    );
    push(
        "bancos",
        "Bancos y caja sin saldo negativo",
        acreedores(saldos(conn, year, &["570", "572", "573", "574"])?),
        "Ninguna cuenta de tesorería queda en negativo.",
        "Quedan en negativo: falta un cobro o sobra un pago, o es una cuenta de crédito (que va a \
         la 5201). Concilia el banco y compara con el extracto del 31 de diciembre.",
        "banco",
    );
    push(
        "partidas_pendientes",
        "Partidas pendientes de aplicación (555) a cero",
        saldos(conn, year, &["555"])?,
        "La 555 está a cero.",
        "La 555 es una cuenta puente: lo que quede hay que llevarlo a su cuenta (el cliente, el \
         proveedor, el gasto) antes de cerrar.",
        "mayor",
    );
    push(
        "socios",
        "Cuenta corriente con socios y administradores (551)",
        saldos(conn, year, &["551", "552"])?,
        "Sin saldo con socios, administradores ni otras vinculadas.",
        "Tiene saldo: si es un préstamo, dalo de alta en Préstamos (con su contrato e intereses \
         de mercado, art. 18 LIS); si son gastos pagados por el socio, liquídalos. Sale en la \
         memoria y en Vinculadas.",
        "prestamos",
    );
    push(
        "iva",
        "IVA soportado y repercutido liquidados (472 y 477)",
        saldos(conn, year, &["472", "477"])?,
        "La 472 y la 477 están a cero: el IVA del ejercicio está liquidado.",
        "Queda IVA sin liquidar: contabiliza la liquidación del último 303 (IVA y libros). Con el \
         criterio de caja o la prorrata puede quedar saldo a propósito: compruébalo.",
        "impuestos",
    );
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn parte_del_ejercicio_siguiente_por_dias() {
        let m = Money::from_euros(1_200, 0);
        let cierre = date!(2026 - 12 - 31);
        // Seguro del 1/10/2026 al 30/9/2027: 273 de 365 días son de 2027.
        assert_eq!(
            parte_siguiente(m, date!(2026 - 10 - 01), date!(2027 - 09 - 30), cierre),
            Money::from_cents(89_753)
        );
        assert_eq!(
            parte_siguiente(m, date!(2026 - 01 - 01), date!(2026 - 12 - 31), cierre),
            Money::ZERO
        );
        assert_eq!(
            parte_siguiente(m, date!(2027 - 01 - 01), date!(2027 - 12 - 31), cierre),
            m
        );
        // Mitad exacta: 1/7/2026 a 30/6/2027 (365 días, 181 después).
        assert_eq!(
            parte_siguiente(
                Money::from_cents(36_500),
                date!(2026 - 07 - 01),
                date!(2027 - 06 - 30),
                cierre
            ),
            Money::from_cents(18_100)
        );
    }

    #[test]
    fn deducibilidad_del_deterioro_art_13_1() {
        use Exclusion as E;
        use Motivo as M;
        assert!(deducible_deterioro(M::SeisMeses, E::Ninguna, true).0);
        assert!(!deducible_deterioro(M::SeisMeses, E::Ninguna, false).0);
        assert!(deducible_deterioro(M::Concurso, E::Ninguna, false).0);
        assert!(!deducible_deterioro(M::Ninguno, E::Ninguna, true).0);
        // Entidad pública o con garantía: sólo con procedimiento judicial o arbitral.
        assert!(!deducible_deterioro(M::SeisMeses, E::Publica, true).0);
        assert!(deducible_deterioro(M::Judicial, E::Publica, true).0);
        assert!(!deducible_deterioro(M::SeisMeses, E::Garantia, true).0);
        assert!(!deducible_deterioro(M::Concurso, E::Renovado, true).0);
        // Vinculada: sólo en concurso con la fase de liquidación abierta.
        assert!(!deducible_deterioro(M::Judicial, E::Vinculada, true).0);
        assert!(!deducible_deterioro(M::Concurso, E::Vinculada, true).0);
        assert!(deducible_deterioro(M::ConcursoLiquidacion, E::Vinculada, true).0);
    }

    #[test]
    fn exceso_de_cada_provision() {
        assert_eq!(cuenta_exceso("142"), "7952");
        assert_eq!(cuenta_exceso("1410001"), "7951");
        assert_eq!(cuenta_exceso("5295"), "7955");
        assert_eq!(cuenta_exceso("4999"), "7954");
        assert!(provision_valida("1420001") && !provision_valida("490") && !provision_valida("14"));
    }
}
