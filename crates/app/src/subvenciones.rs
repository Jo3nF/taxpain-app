//! Subvenciones, donaciones y legados recibidos (G41): el registro (concedente, importe,
//! finalidad, tipo y condiciones), sus asientos y su efecto impositivo.
//!
//! Norma: PGC y PGC de PYMES, NRV 18.ª (RD 1514/2007 y RD 1515/2007), y las definiciones del
//! cuadro de cuentas (130, 131, 132, 172, 4708, 4758, 479, 740, 746 y 747).
//!
//! - **No reintegrables** (se han cumplido las condiciones de la concesión y no hay dudas de
//!   que se recibirán): las de capital (130) y las donaciones y legados de capital (131) se
//!   reconocen en el patrimonio neto y se imputan a resultados (746) en proporción a la
//!   amortización del bien que financian, y lo pendiente cuando se da de baja; las que
//!   financian gastos específicos de ejercicios futuros (132) van a la 747 cuando se devengan
//!   esos gastos; las de explotación van directamente a resultados (740).
//! - **Reintegrables**: mientras no se cumplan las condiciones son una deuda transformable en
//!   subvención (172 a largo plazo; 522 si es de explotación), y pasan al patrimonio neto (o a la
//!   740) cuando dejan de serlo.
//! - **Efecto impositivo**: la subvención en el patrimonio neto se presenta neta de su efecto
//!   fiscal; la parte del impuesto va a la 479 (pasivo por diferencias temporarias imponibles,
//!   NRV 13.ª y 15.ª PYMES) y se revierte contra la 13x al ritmo de la imputación. Fiscalmente la
//!   subvención tributa cuando se imputa a resultados (art. 19 LIS: sigue a la contabilidad), sin
//!   ajuste en el 200. Hay quien no registra el impuesto diferido: la ficha lo deja elegir.
//! - Las cuentas del PGC normal pasan por los grupos 9 y 8 (940, 8301, 840, 8301) para el estado
//!   de ingresos y gastos reconocidos; TaxPain registra directamente en la 13x y la 479, como
//!   en PYMES, porque el cierre no admite saldos en los grupos 8 y 9, y el ECPN reconoce estos
//!   asientos. `TODO(VERIFY)`: comprobar con el cuadro de cuentas de PYMES si la imputación debe
//!   pasar por la 8301/8302 (no existen en PYMES) o si basta con 479 contra 130, como aquí.
//! - El deudor es la 4708 (Hacienda Pública, deudora por subvenciones concedidas) si concede una
//!   Administración; si es un particular (donación), la ficha deja poner otra (440, 44x).
//!   El reintegro (172 o 13x a la 4758, Hacienda Pública, acreedora por subvenciones a
//!   reintegrar) no se asienta desde aquí todavía: se hace a mano.
//! - Ayudas de minimis: el acumulado de los tres últimos años móviles no puede pasar de
//!   300.000 € (Reglamento (UE) 2023/2831, art. 3.2); la ficha marca cuáles lo son.

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::account::AccountCode;
use taxpain_core::accounting::depreciation::{FixedAsset, accumulated_plan};
use taxpain_core::accounting::entry::{EntryLine, JournalEntry, Origin};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, OffsetDateTime};

use crate::accounting;
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

fn fecha(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]"))
        .map_err(|e| other(e.to_string()))
}

fn cuenta(c: &str) -> Result<AccountCode> {
    AccountCode::new(c.trim()).map_err(|e| other(format!("cuenta {c}: {e}")))
}

/// Tope de las ayudas de minimis en tres años (Reglamento (UE) 2023/2831, art. 3.2).
pub const TOPE_MINIMIS: Money = Money::from_cents(30_000_000);

/// Clase de subvención por lo que financia.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tipo {
    /// Oficial de capital: financia un inmovilizado (130 → 746).
    Capital,
    /// Donación o legado de capital (131 → 746).
    Donacion,
    /// Financia gastos específicos de ejercicios futuros (132 → 747).
    Gastos,
    /// A la explotación: directamente a resultados (740).
    Explotacion,
}

impl Tipo {
    fn code(self) -> &'static str {
        match self {
            Tipo::Capital => "capital",
            Tipo::Donacion => "donacion",
            Tipo::Gastos => "gastos",
            Tipo::Explotacion => "explotacion",
        }
    }

    fn of(s: &str) -> Result<Self> {
        Ok(match s {
            "capital" => Tipo::Capital,
            "donacion" => Tipo::Donacion,
            "gastos" => Tipo::Gastos,
            "explotacion" => Tipo::Explotacion,
            _ => return Err(other("tipo de subvención no válido")),
        })
    }

    /// Cuenta del patrimonio neto (`None`: va directamente a resultados).
    pub fn cuenta_pn(self) -> Option<&'static str> {
        match self {
            Tipo::Capital => Some("130"),
            Tipo::Donacion => Some("131"),
            Tipo::Gastos => Some("132"),
            Tipo::Explotacion => None,
        }
    }

    /// Cuenta de ingresos a la que se imputa.
    pub fn cuenta_resultado(self) -> &'static str {
        match self {
            Tipo::Capital | Tipo::Donacion => "746",
            Tipo::Gastos => "747",
            Tipo::Explotacion => "740",
        }
    }

    /// Deuda transformable en subvención mientras sea reintegrable.
    pub fn cuenta_deuda(self) -> &'static str {
        match self {
            Tipo::Explotacion => "522",
            _ => "172",
        }
    }
}

/// Ficha de una subvención.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subvencion {
    #[serde(default)]
    pub id: i64,
    pub concedente: String,
    #[serde(default)]
    pub concedente_nif: String,
    pub fecha_concesion: Date,
    pub importe: Money,
    #[serde(default)]
    pub finalidad: String,
    pub tipo: Tipo,
    #[serde(default)]
    pub condiciones: String,
    /// Al concederse quedan condiciones por cumplir: es reintegrable hasta que se cumplen.
    #[serde(default)]
    pub reintegrable: bool,
    /// Bien del inmovilizado que financia (capital y donación).
    #[serde(default)]
    pub fixed_asset_id: Option<i64>,
    /// Registra el impuesto diferido (479) y deja la 13x neta.
    #[serde(default)]
    pub efecto_impositivo: bool,
    /// Tipo del Impuesto sobre Sociedades para el efecto impositivo (25 % = 2500).
    pub tipo_impositivo: Rate,
    /// Ayuda de minimis.
    #[serde(default)]
    pub minimis: bool,
    /// Cuenta del deudor (4708 si concede una Administración).
    pub cuenta_deudora: String,
    #[serde(default)]
    pub notas: String,
}

/// Qué asiento se hace.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "accion", rename_all = "snake_case")]
pub enum Accion {
    /// Concesión (en la fecha de la ficha).
    Concesion,
    /// Se cumplen las condiciones: deja de ser reintegrable.
    NoReintegrable { fecha: Date },
    /// Cobro (total o parcial) en una cuenta de tesorería.
    Cobro {
        fecha: Date,
        importe: Money,
        tesoreria: String,
    },
    /// Imputación a resultados del ejercicio (al cierre). Sin importe, el que propone TaxPain
    /// (al ritmo de la amortización del bien).
    Imputacion {
        year: i32,
        #[serde(default)]
        importe: Option<Money>,
    },
}

impl Accion {
    fn code(&self) -> &'static str {
        match self {
            Accion::Concesion => "concesion",
            Accion::NoReintegrable { .. } => "no_reintegrable",
            Accion::Cobro { .. } => "cobro",
            Accion::Imputacion { .. } => "imputacion",
        }
    }
}

/// Un asiento ya hecho.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AsientoHecho {
    pub id: i64,
    pub tipo: String,
    pub fecha: Date,
    pub ejercicio: i32,
    pub importe: Money,
    pub entry_id: Option<i64>,
}

/// La ficha con su estado en un ejercicio.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Vista {
    pub subvencion: Subvencion,
    pub bien: Option<String>,
    pub asientos: Vec<AsientoHecho>,
    pub concedida: bool,
    /// Es reintegrable todavía (en el pasivo).
    pub en_deuda: bool,
    pub cobrado: Money,
    pub pendiente_cobro: Money,
    /// Imputado a resultados hasta el cierre del ejercicio (incluido).
    pub imputado: Money,
    /// Imputado en el ejercicio.
    pub imputado_ejercicio: Money,
    /// Bruto que queda en el patrimonio neto al cierre.
    pub pendiente_imputar: Money,
    /// Saldo de la 13x al cierre (neto del efecto impositivo).
    pub saldo_pn: Money,
    /// Saldo de la 479 al cierre por esta subvención.
    pub pasivo_diferido: Money,
    /// Imputación que propone TaxPain para el ejercicio (`None`: no toca o hay que teclearla).
    pub propuesta_imputacion: Option<Money>,
    pub avisos: Vec<String>,
}

/// Lo que el año tiene en conjunto, y el acumulado de minimis.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Resumen {
    pub year: i32,
    pub subvenciones: Vec<Vista>,
    pub imputado_ejercicio: Money,
    pub saldo_pn: Money,
    pub pasivo_diferido: Money,
    /// Ayudas de minimis concedidas en el ejercicio y los dos anteriores.
    pub minimis_tres_años: Money,
    pub tope_minimis: Money,
    pub notas: Vec<String>,
}

const COLS: &str = "id, concedente, concedente_nif, fecha_concesion, importe, finalidad, tipo, \
     condiciones, reintegrable, fixed_asset_id, efecto_impositivo, tipo_impositivo_bp, minimis, \
     cuenta_deudora, notas";

type Fila = (
    i64,
    String,
    String,
    String,
    i64,
    String,
    String,
    String,
    bool,
    Option<i64>,
    bool,
    i32,
    bool,
    String,
    String,
);

fn de_fila(f: Fila) -> Result<Subvencion> {
    Ok(Subvencion {
        id: f.0,
        concedente: f.1,
        concedente_nif: f.2,
        fecha_concesion: fecha(&f.3)?,
        importe: Money::from_cents(f.4),
        finalidad: f.5,
        tipo: Tipo::of(&f.6)?,
        condiciones: f.7,
        reintegrable: f.8,
        fixed_asset_id: f.9,
        efecto_impositivo: f.10,
        tipo_impositivo: Rate::from_bp(f.11),
        minimis: f.12,
        cuenta_deudora: f.13,
        notas: f.14,
    })
}

fn leer(r: &taxpain_vault::rusqlite::Row<'_>) -> taxpain_vault::rusqlite::Result<Fila> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
        r.get(9)?,
        r.get(10)?,
        r.get(11)?,
        r.get(12)?,
        r.get(13)?,
        r.get(14)?,
    ))
}

/// Todas las fichas, por fecha de concesión.
pub fn lista(v: &Vault) -> Result<Vec<Subvencion>> {
    let conn = v.connection();
    let mut st = conn.prepare(&format!(
        "SELECT {COLS} FROM subvencion ORDER BY fecha_concesion, id"
    ))?;
    let filas: Vec<Fila> = st
        .query_map([], leer)?
        .collect::<std::result::Result<_, _>>()?;
    filas.into_iter().map(de_fila).collect()
}

fn cargar(conn: &Connection, id: i64) -> Result<Subvencion> {
    let f = conn
        .query_row(
            &format!("SELECT {COLS} FROM subvencion WHERE id = ?1"),
            [id],
            leer,
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    de_fila(f)
}

fn asientos(conn: &Connection, id: i64) -> Result<Vec<AsientoHecho>> {
    let mut st = conn.prepare(
        "SELECT id, tipo, fecha, ejercicio, importe, entry_id FROM subvencion_asiento
         WHERE subvencion_id = ?1 ORDER BY fecha, id",
    )?;
    let filas: Vec<(i64, String, String, i32, i64, Option<i64>)> = st
        .query_map([id], |r| {
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
    filas
        .into_iter()
        .map(|(id, tipo, f, ejercicio, importe, entry_id)| {
            Ok(AsientoHecho {
                id,
                tipo,
                fecha: fecha(&f)?,
                ejercicio,
                importe: Money::from_cents(importe),
                entry_id,
            })
        })
        .collect()
}

fn comprobar(s: &Subvencion) -> Result<()> {
    if s.concedente.trim().is_empty() {
        return Err(other("falta el concedente"));
    }
    if s.importe.cents() <= 0 {
        return Err(other("el importe tiene que ser positivo"));
    }
    if !(0..=10_000).contains(&s.tipo_impositivo.bp()) {
        return Err(other("el tipo impositivo va de 0 a 100 %"));
    }
    let deudora = cuenta(&s.cuenta_deudora)?;
    if !(deudora.as_str().starts_with("47") || deudora.as_str().starts_with("44")) {
        return Err(other(
            "el deudor es una cuenta 4708 (Administración) o 44x (otros deudores)",
        ));
    }
    if s.fixed_asset_id.is_some() && !matches!(s.tipo, Tipo::Capital | Tipo::Donacion) {
        return Err(other(
            "sólo las subvenciones de capital y las donaciones financian un bien",
        ));
    }
    Ok(())
}

/// Guarda una ficha (id 0: nueva). Con asientos hechos sólo se cambian los textos.
pub fn guardar(v: &Vault, s: &Subvencion, now: OffsetDateTime) -> Result<i64> {
    comprobar(s)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let n = now_str(now);
    let id = if s.id == 0 {
        tx.execute(
            "INSERT INTO subvencion(concedente, concedente_nif, fecha_concesion, importe,
               finalidad, tipo, condiciones, reintegrable, fixed_asset_id, efecto_impositivo,
               tipo_impositivo_bp, minimis, cuenta_deudora, notas, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)",
            params![
                s.concedente.trim(),
                s.concedente_nif.trim().to_uppercase(),
                s.fecha_concesion.to_string(),
                s.importe.cents(),
                s.finalidad.trim(),
                s.tipo.code(),
                s.condiciones.trim(),
                s.reintegrable,
                s.fixed_asset_id,
                s.efecto_impositivo,
                s.tipo_impositivo.bp(),
                s.minimis,
                s.cuenta_deudora.trim(),
                s.notas.trim(),
                n,
            ],
        )?;
        tx.last_insert_rowid()
    } else {
        let antes = cargar(&tx, s.id)?;
        let con_asientos = !asientos(&tx, s.id)?.is_empty();
        let cambia_cifras = antes.importe != s.importe
            || antes.tipo != s.tipo
            || antes.fecha_concesion != s.fecha_concesion
            || antes.reintegrable != s.reintegrable
            || antes.fixed_asset_id != s.fixed_asset_id
            || antes.efecto_impositivo != s.efecto_impositivo
            || antes.tipo_impositivo != s.tipo_impositivo
            || antes.cuenta_deudora.trim() != s.cuenta_deudora.trim();
        if con_asientos && cambia_cifras {
            return Err(other(
                "la subvención ya tiene asientos: deshazlos antes de cambiar el importe, el \
                 tipo, las fechas, el bien o las cuentas",
            ));
        }
        tx.execute(
            "UPDATE subvencion SET concedente = ?1, concedente_nif = ?2, fecha_concesion = ?3,
               importe = ?4, finalidad = ?5, tipo = ?6, condiciones = ?7, reintegrable = ?8,
               fixed_asset_id = ?9, efecto_impositivo = ?10, tipo_impositivo_bp = ?11,
               minimis = ?12, cuenta_deudora = ?13, notas = ?14, updated_at = ?15
             WHERE id = ?16",
            params![
                s.concedente.trim(),
                s.concedente_nif.trim().to_uppercase(),
                s.fecha_concesion.to_string(),
                s.importe.cents(),
                s.finalidad.trim(),
                s.tipo.code(),
                s.condiciones.trim(),
                s.reintegrable,
                s.fixed_asset_id,
                s.efecto_impositivo,
                s.tipo_impositivo.bp(),
                s.minimis,
                s.cuenta_deudora.trim(),
                s.notas.trim(),
                n,
                s.id,
            ],
        )?;
        s.id
    };
    audit::append(
        &tx,
        &n,
        Actor::User,
        "subvencion.save",
        &id.to_string(),
        &serde_json::json!({ "importe": s.importe.cents(), "tipo": s.tipo.code() }),
    )?;
    tx.commit()?;
    Ok(id)
}

/// Borra una ficha sin asientos (con confirmación).
pub fn borrar(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    cargar(&tx, id)?;
    if !asientos(&tx, id)?.is_empty() {
        return Err(other(
            "la subvención tiene asientos: deshazlos antes de borrarla",
        ));
    }
    tx.execute("DELETE FROM subvencion WHERE id = ?1", [id])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "subvencion.delete",
        &id.to_string(),
        &serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

/// Reparte un importe bruto en la parte neta (13x) y la del impuesto diferido (479).
fn reparto(s: &Subvencion, bruto: Money) -> (Money, Money) {
    if !s.efecto_impositivo || s.tipo.cuenta_pn().is_none() {
        return (bruto, Money::ZERO);
    }
    let impuesto = bruto.apply_rate(s.tipo_impositivo);
    (bruto - impuesto, impuesto)
}

/// Bien que financia, si lo hay.
fn bien(v: &Vault, s: &Subvencion) -> Result<Option<FixedAsset>> {
    let Some(id) = s.fixed_asset_id else {
        return Ok(None);
    };
    Ok(accounting::fixed_assets(v)?
        .into_iter()
        .find(|a| a.id == id))
}

fn sum(a: &[AsientoHecho], tipo: &str, hasta: Option<i32>) -> Money {
    a.iter()
        .filter(|x| x.tipo == tipo && hasta.is_none_or(|y| x.ejercicio <= y))
        .map(|x| x.importe)
        .sum()
}

/// Imputación que toca en `year` con la amortización del bien: el importe por la amortización
/// acumulada del plan al cierre sobre la base amortizable, menos lo ya imputado; todo lo
/// pendiente si el bien se ha dado de baja (NRV 18.ª.1.3.a). `None`: no se puede proponer
/// (sin bien, o no es de capital).
fn propuesta(
    v: &Vault,
    s: &Subvencion,
    hechos: &[AsientoHecho],
    year: i32,
) -> Result<Option<Money>> {
    if !matches!(s.tipo, Tipo::Capital | Tipo::Donacion) {
        return Ok(None);
    }
    let Some(a) = bien(v, s)? else {
        return Ok(None);
    };
    let fy = accounting::fiscal_year(v, year)?;
    let antes = sum(hechos, "imputacion", Some(year - 1));
    let objetivo = if a.disposed_on.is_some_and(|d| d <= fy.end) {
        s.importe
    } else {
        let base = a.cost - a.residual;
        if base.cents() <= 0 {
            return Ok(None);
        }
        let acumulada = accumulated_plan(&a, a.method, a.rate, fy.end).min(base);
        let c = i128::from(s.importe.cents()) * i128::from(acumulada.cents())
            / i128::from(base.cents());
        Money::from_cents(i64::try_from(c).unwrap_or(0))
    };
    Ok(Some((objetivo - antes).max(Money::ZERO)))
}

/// Estado de una ficha al cierre de `year`.
fn vista(v: &Vault, s: Subvencion, year: i32) -> Result<Vista> {
    let conn = v.connection();
    let hechos = asientos(conn, s.id)?;
    let concedida = hechos.iter().any(|a| a.tipo == "concesion");
    let no_reint = hechos.iter().any(|a| a.tipo == "no_reintegrable");
    let en_deuda = concedida && s.reintegrable && !no_reint;
    let cobrado = sum(&hechos, "cobro", None);
    let imputado = sum(&hechos, "imputacion", Some(year));
    let imputado_ejercicio: Money = hechos
        .iter()
        .filter(|a| a.tipo == "imputacion" && a.ejercicio == year)
        .map(|a| a.importe)
        .sum();
    let en_pn = concedida && !en_deuda && s.tipo.cuenta_pn().is_some();
    let pendiente_imputar = if en_pn {
        s.importe - imputado
    } else {
        Money::ZERO
    };
    let (saldo_pn, pasivo_diferido) = reparto(&s, pendiente_imputar);
    let ya = hechos
        .iter()
        .any(|a| a.tipo == "imputacion" && a.ejercicio == year);
    let propuesta_imputacion = if en_pn && !ya {
        propuesta(v, &s, &hechos, year)?.filter(|m| !m.is_zero())
    } else {
        None
    };
    let b = bien(v, &s)?;
    let mut avisos = Vec::new();
    if !concedida {
        avisos.push("Falta el asiento de la concesión.".to_owned());
    }
    if en_deuda {
        avisos.push(format!(
            "Es reintegrable: está en la {} hasta que se cumplan las condiciones ({}).",
            s.tipo.cuenta_deuda(),
            if s.condiciones.trim().is_empty() {
                "anótalas en la ficha"
            } else {
                s.condiciones.trim()
            }
        ));
    }
    if matches!(s.tipo, Tipo::Capital | Tipo::Donacion) && s.fixed_asset_id.is_none() {
        avisos.push(
            "No está ligada a ningún bien del inmovilizado: la imputación anual hay que \
             teclearla."
                .to_owned(),
        );
    }
    if s.tipo == Tipo::Gastos && en_pn && !pendiente_imputar.is_zero() {
        avisos.push(
            "Financia gastos de ejercicios futuros: imputa a la 747 lo que se haya gastado en \
             cada ejercicio."
                .to_owned(),
        );
    }
    if concedida && cobrado < s.importe {
        avisos.push(format!(
            "Pendiente de cobro: {} en la {}.",
            s.importe - cobrado,
            s.cuenta_deudora
        ));
    }
    Ok(Vista {
        bien: b.map(|a| a.name),
        subvencion: s.clone(),
        asientos: hechos,
        concedida,
        en_deuda,
        cobrado,
        pendiente_cobro: if concedida {
            (s.importe - cobrado).max(Money::ZERO)
        } else {
            Money::ZERO
        },
        imputado,
        imputado_ejercicio,
        pendiente_imputar,
        saldo_pn,
        pasivo_diferido,
        propuesta_imputacion,
        avisos,
    })
}

/// Todas las subvenciones con su estado al cierre de `year`.
pub fn resumen(v: &Vault, year: i32) -> Result<Resumen> {
    let subvenciones: Vec<Vista> = lista(v)?
        .into_iter()
        .filter(|s| s.fecha_concesion.year() <= year)
        .map(|s| vista(v, s, year))
        .collect::<Result<_>>()?;
    let minimis_tres_años: Money = subvenciones
        .iter()
        .filter(|x| {
            let y = x.subvencion.fecha_concesion.year();
            x.subvencion.minimis && y > year - 3 && y <= year
        })
        .map(|x| x.subvencion.importe)
        .sum();
    let mut notas = vec![
        "Las subvenciones no reintegrables de capital van al patrimonio neto (130, 131) y a \
         resultados (746) al ritmo de la amortización del bien; las de gastos futuros (132) a la \
         747 cuando se devengan; las de explotación, a la 740 (PGC y PGC de PYMES, NRV 18.ª)."
            .to_owned(),
        "Fiscalmente tributan cuando se imputan a resultados (art. 19 LIS): no hay ajuste en el \
         200. La parte del impuesto que corresponde a lo que queda en el patrimonio neto es un \
         pasivo por impuesto diferido (479)."
            .to_owned(),
    ];
    if minimis_tres_años > TOPE_MINIMIS {
        notas.push(format!(
            "Las ayudas de minimis de {}-{year} suman {minimis_tres_años}: pasan del tope de \
             {TOPE_MINIMIS} en tres años (Reglamento (UE) 2023/2831, art. 3.2).",
            year - 2
        ));
    }
    Ok(Resumen {
        year,
        imputado_ejercicio: subvenciones.iter().map(|x| x.imputado_ejercicio).sum(),
        saldo_pn: subvenciones.iter().map(|x| x.saldo_pn).sum(),
        pasivo_diferido: subvenciones.iter().map(|x| x.pasivo_diferido).sum(),
        subvenciones,
        minimis_tres_años,
        tope_minimis: TOPE_MINIMIS,
        notas,
    })
}

fn linea(c: &str, debe: bool, m: Money, desc: &str) -> Result<Option<EntryLine>> {
    if m.is_zero() {
        return Ok(None);
    }
    let a = cuenta(c)?;
    let mut l = if debe {
        EntryLine::debit(a, m)
    } else {
        EntryLine::credit(a, m)
    };
    l.description = Some(desc.to_owned());
    Ok(Some(l))
}

/// El asiento que saldría (sin grabarlo), con el importe bruto que se anota.
pub fn propuesta_asiento(v: &Vault, id: i64, accion: &Accion) -> Result<(JournalEntry, Money)> {
    let conn = v.connection();
    let s = cargar(conn, id)?;
    let hechos = asientos(conn, id)?;
    let concedida = hechos.iter().any(|a| a.tipo == "concesion");
    let no_reint = hechos.iter().any(|a| a.tipo == "no_reintegrable");
    let desc = s.concedente.trim().to_owned();
    let mut lines: Vec<Option<EntryLine>> = Vec::new();
    // El reconocimiento en el patrimonio neto (o en resultados) cuando deja de ser reintegrable.
    let reconocer = |lines: &mut Vec<Option<EntryLine>>| -> Result<()> {
        match s.tipo.cuenta_pn() {
            Some(pn) => {
                let (neto, impuesto) = reparto(&s, s.importe);
                lines.push(linea(pn, false, neto, &desc)?);
                lines.push(linea("479", false, impuesto, &desc)?);
            }
            None => lines.push(linea(s.tipo.cuenta_resultado(), false, s.importe, &desc)?),
        }
        Ok(())
    };
    let (date, concept, importe) = match accion {
        Accion::Concesion => {
            if concedida {
                return Err(other("la concesión ya está contabilizada"));
            }
            lines.push(linea(&s.cuenta_deudora, true, s.importe, &desc)?);
            if s.reintegrable {
                lines.push(linea(s.tipo.cuenta_deuda(), false, s.importe, &desc)?);
            } else {
                reconocer(&mut lines)?;
            }
            (
                s.fecha_concesion,
                format!("Concesión de subvención: {desc}"),
                s.importe,
            )
        }
        Accion::NoReintegrable { fecha } => {
            if !concedida {
                return Err(other("primero hay que contabilizar la concesión"));
            }
            if !s.reintegrable || no_reint {
                return Err(other("la subvención ya no es reintegrable"));
            }
            if *fecha < s.fecha_concesion {
                return Err(other("la fecha es anterior a la concesión"));
            }
            lines.push(linea(s.tipo.cuenta_deuda(), true, s.importe, &desc)?);
            reconocer(&mut lines)?;
            (
                *fecha,
                format!("Subvención no reintegrable: {desc}"),
                s.importe,
            )
        }
        Accion::Cobro {
            fecha,
            importe,
            tesoreria,
        } => {
            if !concedida {
                return Err(other("primero hay que contabilizar la concesión"));
            }
            let cobrado = sum(&hechos, "cobro", None);
            if importe.cents() <= 0 || cobrado + *importe > s.importe {
                return Err(other(format!(
                    "el cobro tiene que ser positivo y no pasar de lo pendiente ({})",
                    s.importe - cobrado
                )));
            }
            if !tesoreria.trim().starts_with('5') {
                return Err(other("la tesorería es una cuenta del grupo 5 (572…)"));
            }
            lines.push(linea(tesoreria, true, *importe, &desc)?);
            lines.push(linea(&s.cuenta_deudora, false, *importe, &desc)?);
            (*fecha, format!("Cobro de subvención: {desc}"), *importe)
        }
        Accion::Imputacion { year, importe } => {
            let Some(pn) = s.tipo.cuenta_pn() else {
                return Err(other(
                    "las subvenciones de explotación ya van a resultados al concederse",
                ));
            };
            if !concedida || (s.reintegrable && !no_reint) {
                return Err(other(
                    "sólo se imputa una subvención concedida y no reintegrable",
                ));
            }
            if hechos
                .iter()
                .any(|a| a.tipo == "imputacion" && a.ejercicio == *year)
            {
                return Err(other(format!("la imputación de {year} ya está hecha")));
            }
            let pendiente = s.importe - sum(&hechos, "imputacion", None);
            let m = match importe {
                Some(m) => *m,
                None => propuesta(v, &s, &hechos, *year)?.ok_or_else(|| {
                    other("sin bien ligado: teclea el importe que se imputa este año")
                })?,
            };
            if m.cents() <= 0 || m > pendiente {
                return Err(other(format!(
                    "la imputación tiene que ser positiva y no pasar de lo pendiente ({pendiente})"
                )));
            }
            let (neto, impuesto) = reparto(&s, m);
            lines.push(linea(pn, true, neto, &desc)?);
            lines.push(linea("479", true, impuesto, &desc)?);
            lines.push(linea(s.tipo.cuenta_resultado(), false, m, &desc)?);
            let fy = accounting::fiscal_year(v, *year)?;
            (
                fy.end,
                format!("Imputación a resultados de subvención: {desc}"),
                m,
            )
        }
    };
    let e = JournalEntry {
        date,
        concept,
        origin: Origin::Subvencion(id),
        lines: lines.into_iter().flatten().collect(),
    };
    e.validate().map_err(|err| other(err.to_string()))?;
    Ok((e, importe))
}

/// Graba el asiento de una acción (con confirmación) y queda en la auditoría.
pub fn registrar(
    v: &Vault,
    id: i64,
    accion: &Accion,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<i64> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let (e, importe) = propuesta_asiento(v, id, accion)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let year = accounting::fiscal_year_for_date(&tx, e.date)?.year;
    if accounting::year_closed(&tx, year)? {
        return Err(other(format!(
            "el ejercicio {year} está cerrado: reábrelo desde Cierre"
        )));
    }
    let entry = accounting::record_entry(&tx, &e, now)?;
    let n = now_str(now);
    tx.execute(
        "INSERT INTO subvencion_asiento(subvencion_id, tipo, fecha, ejercicio, importe, entry_id,
           created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            id,
            accion.code(),
            e.date.to_string(),
            year,
            importe.cents(),
            entry,
            n
        ],
    )?;
    audit::append(
        &tx,
        &n,
        Actor::User,
        "subvencion.asiento",
        &format!("{id}/{}", accion.code()),
        &serde_json::json!({ "entry": entry, "importe": importe.cents() }),
    )?;
    tx.commit()?;
    Ok(entry)
}

/// Deshace el último asiento de una subvención (con confirmación): borra el asiento del diario
/// si su ejercicio está abierto.
pub fn deshacer(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let ultimo: Option<(i64, String, i32, Option<i64>)> = tx
        .query_row(
            "SELECT id, tipo, ejercicio, entry_id FROM subvencion_asiento
             WHERE subvencion_id = ?1 ORDER BY id DESC LIMIT 1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((aid, tipo, year, entry)) = ultimo else {
        return Err(other("la subvención no tiene asientos"));
    };
    if accounting::year_closed(&tx, year)? {
        return Err(other(format!(
            "el ejercicio {year} está cerrado: reábrelo desde Cierre"
        )));
    }
    tx.execute("DELETE FROM subvencion_asiento WHERE id = ?1", [aid])?;
    if let Some(e) = entry {
        tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [e])?;
        tx.execute("DELETE FROM journal_entry WHERE id = ?1", [e])?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "subvencion.deshacer",
        &format!("{id}/{tipo}"),
        &serde_json::json!({ "entry": entry }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Párrafos de la nota de la memoria (subvenciones, donaciones y legados) con el registro.
pub fn memoria(v: &Vault, year: i32) -> Result<Vec<String>> {
    let r = resumen(v, year)?;
    let mut out = Vec::new();
    for x in &r.subvenciones {
        let s = &x.subvencion;
        let clase = match s.tipo {
            Tipo::Capital => "de capital",
            Tipo::Donacion => "donación o legado de capital",
            Tipo::Gastos => "para gastos de ejercicios futuros",
            Tipo::Explotacion => "a la explotación",
        };
        let mut t = format!(
            "{} concedida por {}{} el {:02}/{:02}/{}, por {}{}",
            capital_primera(clase),
            s.concedente.trim(),
            if s.concedente_nif.is_empty() {
                String::new()
            } else {
                format!(" (NIF {})", s.concedente_nif)
            },
            s.fecha_concesion.day(),
            u8::from(s.fecha_concesion.month()),
            s.fecha_concesion.year(),
            s.importe,
            if s.finalidad.trim().is_empty() {
                String::new()
            } else {
                format!(", para {}", s.finalidad.trim())
            }
        );
        if let Some(b) = &x.bien {
            t.push_str(&format!(" (financia «{b}»)"));
        }
        t.push('.');
        if x.en_deuda {
            t.push_str(" Es reintegrable: se presenta como deuda transformable en subvención.");
        } else if s.tipo.cuenta_pn().is_some() {
            t.push_str(&format!(
                " Imputado a resultados en el ejercicio: {}; acumulado: {}; pendiente en el \
                 patrimonio neto: {} (neto de efecto impositivo: {}).",
                x.imputado_ejercicio, x.imputado, x.pendiente_imputar, x.saldo_pn
            ));
        }
        t.push_str(if s.condiciones.trim().is_empty() {
            " [Indica si se han cumplido las condiciones.]"
        } else {
            " Condiciones: "
        });
        if !s.condiciones.trim().is_empty() {
            t.push_str(s.condiciones.trim());
            t.push_str(if x.en_deuda {
                " [pendientes de cumplir]."
            } else {
                " [cumplidas]."
            });
        }
        if s.minimis {
            t.push_str(" Ayuda de minimis.");
        }
        out.push(t);
    }
    Ok(out)
}

fn capital_primera(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => format!("Subvención {}{}", f, c.as_str()),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuentas_por_tipo() {
        assert_eq!(Tipo::Capital.cuenta_pn(), Some("130"));
        assert_eq!(Tipo::Donacion.cuenta_resultado(), "746");
        assert_eq!(Tipo::Gastos.cuenta_resultado(), "747");
        assert_eq!(Tipo::Explotacion.cuenta_pn(), None);
        assert_eq!(Tipo::Explotacion.cuenta_deuda(), "522");
        assert_eq!(Tipo::of("capital").unwrap_or(Tipo::Gastos), Tipo::Capital);
    }
}
