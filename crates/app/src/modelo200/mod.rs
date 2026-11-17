//! Modelo 200 completo (carencia G05): de la contabilidad, las fichas de participadas, la
//! entidad patrimonial, Valores y los arrastres de otros ejercicios a la liquidación del
//! Impuesto sobre Sociedades, con cada cifra con su origen y su norma.
//!
//! - Correcciones al resultado contable: propuestas desde la contabilidad ([`propuestas`]) y
//!   editables siempre (`is_ajuste`: cambiar el importe, descartar o añadir a mano con su
//!   casilla del catálogo `taxpain_models::m200_catalogo`).
//! - Bases negativas, minoraciones de la reserva de nivelación, reducciones de la de
//!   capitalización y deducción por doble imposición pendientes, por ejercicio de origen
//!   (`is_arrastre`), con su paso al ejercicio siguiente ([`pasar_al_siguiente`]).
//! - Reserva de capitalización (art. 25 LIS): incremento de fondos propios desde el balance
//!   ([`fondos_propios`]) y comprobación de que la aplicación del resultado dota la reserva.
//! - Entidad patrimonial (art. 5.2): del test de `patrimonial.rs`; quita el tipo reducido y los
//!   incentivos de reducida dimensión con aviso.
//! - Deducción por doble imposición internacional (art. 31): la de Valores y la de dividendos de
//!   participadas no residentes que no tengan la exención; I+D+i, otras deducciones y
//!   bonificaciones, a mano.
//! - Tipo de gravamen propuesto por la cifra de negocios del periodo anterior (art. 29.1 LIS,
//!   Ley 7/2024).
//!
//! Nada se presenta: el fichero se importa en Sociedades WEB.

pub mod participadas;
pub mod propuestas;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::{FiscalYear, JournalEntry, Origin};
use taxpain_core::money::{Money, Rate};
use taxpain_models::m200::{
    self, Arrastre, Capitalizacion, DetalleArrastre, Importe, Input200, Nivelacion, Result200,
    TaxRegime,
};
use taxpain_models::m200_catalogo::{self, CasillaCuota, Correccion, GrupoCuota};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::{Date, Month, OffsetDateTime};

use crate::accounting;
use crate::error::{AppError, Result};
use crate::invoicing::now_str;
use participadas::{EstadoArt21, ParticipadaVista};
use propuestas::Propuesta;

pub(crate) fn dmy(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

fn other(e: impl ToString) -> AppError {
    AppError::Other(e.to_string())
}

/// Datos del 200 de un ejercicio que teclea el usuario (`is_ejercicio`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Datos200 {
    /// Aplicar la reducción por reserva de capitalización (art. 25 LIS): obliga a dotar la
    /// reserva y a mantener el incremento cinco años.
    #[serde(default)]
    pub aplicar_capitalizacion: bool,
    /// Incremento de fondos propios corregido a mano (sin él, el del balance).
    #[serde(default)]
    pub incremento_fondos_propios: Option<Money>,
    /// Porcentaje de la reserva de capitalización (sin él, el del ejercicio: 10 % hasta 2023,
    /// 15 % en 2024 y 20 % desde 2025).
    #[serde(default)]
    pub porcentaje_capitalizacion: Option<u8>,
    /// Minoración por reserva de nivelación que se quiere aplicar (art. 105 LIS).
    #[serde(default)]
    pub nivelacion: Money,
    /// Tres primeros periodos con base positiva de una entidad de nueva creación (art. 26.3).
    #[serde(default)]
    pub bin_nueva_creacion: bool,
    /// Deducción por doble imposición internacional del ejercicio tecleada (sin ella, la
    /// calculada desde Valores y las participadas).
    #[serde(default)]
    pub doble_imposicion: Option<Money>,
    /// Bonificaciones y otras deducciones (I+D+i, donaciones…) con su casilla.
    #[serde(default)]
    pub cuota: Vec<Importe>,
    /// Desglose de las retenciones tecleado (sin él, el propuesto).
    #[serde(default)]
    pub retenciones: Option<Vec<Importe>>,
}

/// Tipo de importe que pasa de un ejercicio a otro.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TipoArrastre {
    Bin,
    Nivelacion,
    Capitalizacion,
    DobleImposicion,
}

impl TipoArrastre {
    fn as_str(self) -> &'static str {
        match self {
            Self::Bin => "bin",
            Self::Nivelacion => "nivelacion",
            Self::Capitalizacion => "capitalizacion",
            Self::DobleImposicion => "doble_imposicion",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "bin" => Self::Bin,
            "nivelacion" => Self::Nivelacion,
            "capitalizacion" => Self::Capitalizacion,
            "doble_imposicion" => Self::DobleImposicion,
            _ => return None,
        })
    }
}

/// Fila de `is_arrastre`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArrastreGuardado {
    pub tipo: TipoArrastre,
    pub origen: i32,
    /// Ejercicio al empezar el cual queda `pendiente`.
    pub ejercicio: i32,
    pub pendiente: Money,
    #[serde(default)]
    pub nota: String,
}

/// Corrección al resultado tal como se ve en pantalla.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AjusteVista {
    pub clave: String,
    pub casilla: u16,
    pub aumento: bool,
    pub concepto: String,
    pub norma: String,
    /// Importe que entra en el cálculo (cero si está descartada o sin incluir).
    pub importe: Money,
    /// Lo que propone TaxPain (`None` en las tecleadas a mano).
    pub propuesto: Option<Money>,
    pub origen: String,
    /// `propuesta`, `sin_incluir`, `editada`, `descartada` o `manual`.
    pub estado: String,
    pub nota: Option<String>,
}

/// Una cifra de la hoja con su casilla, origen y norma.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Linea {
    pub casilla: Option<u16>,
    pub concepto: String,
    pub importe: Money,
    pub origen: String,
    pub norma: String,
}

fn linea(casilla: u16, concepto: &str, importe: Money, origen: &str, norma: &str) -> Linea {
    Linea {
        casilla: Some(casilla),
        concepto: concepto.into(),
        importe,
        origen: origen.into(),
        norma: norma.into(),
    }
}

/// Partida de fondos propios para la reserva de capitalización.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PartidaFondos {
    pub cuenta: String,
    pub nombre: String,
    pub inicio: Money,
    pub fin: Money,
    pub computa: bool,
    pub motivo: Option<String>,
}

/// Incremento de fondos propios del ejercicio (art. 25.2 LIS).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct FondosPropios {
    pub inicio: Money,
    pub fin: Money,
    pub incremento: Money,
    pub partidas: Vec<PartidaFondos>,
    pub avisos: Vec<String>,
}

/// Comprobación de que la reserva consta dotada.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Dotacion {
    pub a_dotar: Money,
    pub dotada: Option<Money>,
    /// `dotada`, `en_acta`, `falta` o `pendiente` (la junta aún no ha aprobado las cuentas).
    pub estado: String,
    pub texto: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReservaVista {
    pub lineas: Vec<Linea>,
    pub detalle: Vec<DetalleArrastre>,
    pub dotacion: Option<Dotacion>,
}

/// Hoja del 200: los bloques de la pantalla Modelos → 200.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Hoja200 {
    pub year: i32,
    pub inicio: Date,
    pub fin: Date,
    /// Régimen de tipos guardado en «Datos de los modelos» y el que propone TaxPain.
    pub regimen_guardado: TaxRegime,
    pub regimen_propuesto: TaxRegime,
    pub regimen_motivo: String,
    pub regimen_aplicado: TaxRegime,
    /// Resultado del test de entidad patrimonial (`None` si no se ha podido hacer).
    pub patrimonial: Option<bool>,
    /// Cifra de negocios de los 12 meses anteriores (la del periodo anterior) y la del periodo.
    pub cifra_negocios_anterior: Option<Money>,
    pub cifra_negocios: Money,
    pub resultado: Vec<Linea>,
    pub ajustes: Vec<AjusteVista>,
    pub base_previa: Vec<Linea>,
    pub bins: Vec<DetalleArrastre>,
    pub bin_generada: Money,
    /// De dónde salen las bases negativas: `arrastres`, `datos` (un total sin año) o `ninguna`.
    pub bins_origen: String,
    pub base: Vec<Linea>,
    pub fondos_propios: FondosPropios,
    pub capitalizacion: ReservaVista,
    pub nivelacion: ReservaVista,
    pub cuota: Vec<Linea>,
    pub retenciones: Vec<Linea>,
    pub liquidacion: Vec<Linea>,
    pub participadas: Vec<ParticipadaVista>,
    pub arrastres: Vec<ArrastreGuardado>,
    /// Lo que pasaría al ejercicio siguiente.
    pub siguiente: Vec<ArrastreGuardado>,
    pub datos: Datos200,
    pub catalogo: Vec<Correccion>,
    pub catalogo_cuota: Vec<CasillaCuota>,
    pub notas: Vec<String>,
    pub avisos: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// Persistencia
// ---------------------------------------------------------------------------------------------

pub fn datos(v: &Vault, year: i32) -> Result<Datos200> {
    let s: Option<String> = v
        .connection()
        .query_row(
            "SELECT datos FROM is_ejercicio WHERE year = ?1",
            [year],
            |r| r.get(0),
        )
        .optional()?;
    Ok(s.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default())
}

pub fn guardar_datos(v: &Vault, year: i32, d: &Datos200, now: OffsetDateTime) -> Result<()> {
    if d.porcentaje_capitalizacion
        .is_some_and(|p| p == 0 || p > 30)
    {
        return Err(other(
            "el porcentaje de la reserva de capitalización va de 1 a 30",
        ));
    }
    for x in &d.cuota {
        match m200_catalogo::casilla_cuota(x.casilla) {
            Some(c) if c.grupo != GrupoCuota::Retencion => {}
            _ => {
                return Err(other(format!(
                    "la casilla [{:05}] no es una bonificación ni una deducción",
                    x.casilla
                )));
            }
        }
    }
    for x in d.retenciones.iter().flatten() {
        if m200_catalogo::casilla_cuota(x.casilla).is_none_or(|c| c.grupo != GrupoCuota::Retencion)
        {
            return Err(other(format!(
                "la casilla [{:05}] no es de retenciones",
                x.casilla
            )));
        }
    }
    let json = serde_json::to_string(d).map_err(other)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO is_ejercicio(year, datos, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(year) DO UPDATE SET datos = excluded.datos, updated_at = excluded.updated_at",
        params![year, json, now_str(now)],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "modelo200.datos",
        &year.to_string(),
        &serde_json::to_value(d).unwrap_or_default(),
    )?;
    tx.commit()?;
    Ok(())
}

pub fn arrastres(v: &Vault, ejercicio: i32) -> Result<Vec<ArrastreGuardado>> {
    let mut st = v.connection().prepare(
        "SELECT tipo, origen, ejercicio, pendiente, nota FROM is_arrastre WHERE ejercicio = ?1
         ORDER BY tipo, origen",
    )?;
    let rows = st.query_map([ejercicio], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i32>(1)?,
            r.get::<_, i32>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, String>(4)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (t, origen, ejercicio, p, nota) = row?;
        if let Some(tipo) = TipoArrastre::parse(&t) {
            out.push(ArrastreGuardado {
                tipo,
                origen,
                ejercicio,
                pendiente: Money::from_cents(p),
                nota,
            });
        }
    }
    Ok(out)
}

pub fn guardar_arrastre(v: &Vault, a: &ArrastreGuardado, now: OffsetDateTime) -> Result<()> {
    if a.pendiente.is_negative() {
        return Err(other("el importe pendiente no puede ser negativo"));
    }
    if a.origen > a.ejercicio {
        return Err(other(
            "el ejercicio de origen no puede ser posterior al de la liquidación",
        ));
    }
    if a.tipo == TipoArrastre::Bin && a.origen < 1997 {
        return Err(other("el 200 recoge bases negativas desde 1997"));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO is_arrastre(tipo, origen, ejercicio, pendiente, nota, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(tipo, origen, ejercicio) DO UPDATE SET pendiente = excluded.pendiente,
           nota = excluded.nota, updated_at = excluded.updated_at",
        params![
            a.tipo.as_str(),
            a.origen,
            a.ejercicio,
            a.pendiente.cents(),
            a.nota.trim(),
            now_str(now)
        ],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "modelo200.arrastre",
        &format!("{}:{}:{}", a.tipo.as_str(), a.origen, a.ejercicio),
        &serde_json::json!({ "pendiente": a.pendiente.cents() }),
    )?;
    tx.commit()?;
    Ok(())
}

pub fn borrar_arrastre(
    v: &Vault,
    tipo: TipoArrastre,
    origen: i32,
    ejercicio: i32,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    if tx.execute(
        "DELETE FROM is_arrastre WHERE tipo = ?1 AND origen = ?2 AND ejercicio = ?3",
        params![tipo.as_str(), origen, ejercicio],
    )? == 0
    {
        return Err(AppError::NotFound);
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "modelo200.arrastre.delete",
        &format!("{}:{origen}:{ejercicio}", tipo.as_str()),
        &serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

/// Corrección guardada por el usuario.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AjusteGuardado {
    /// La de la propuesta que se corrige; vacía para añadir una a mano.
    #[serde(default)]
    pub clave: String,
    pub casilla: u16,
    /// `None`: descartar la propuesta.
    pub importe: Option<Money>,
    #[serde(default)]
    pub concepto: String,
}

fn ajustes_guardados(v: &Vault, year: i32) -> Result<Vec<AjusteGuardado>> {
    let mut st = v.connection().prepare(
        "SELECT clave, casilla, importe, concepto FROM is_ajuste WHERE year = ?1 ORDER BY clave",
    )?;
    let rows = st.query_map([year], |r| {
        Ok(AjusteGuardado {
            clave: r.get(0)?,
            casilla: r.get(1)?,
            importe: r.get::<_, Option<i64>>(2)?.map(Money::from_cents),
            concepto: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Guarda una corrección (cambia o descarta una propuesta, o añade una a mano). Devuelve su
/// clave.
pub fn guardar_ajuste(
    v: &Vault,
    year: i32,
    a: &AjusteGuardado,
    now: OffsetDateTime,
) -> Result<String> {
    if m200_catalogo::correccion(a.casilla).is_none() {
        return Err(other(format!(
            "la casilla [{:05}] no está en el catálogo de correcciones del 200",
            a.casilla
        )));
    }
    if a.importe.is_some_and(|m| m.is_negative()) {
        return Err(other(
            "el importe va en positivo: la casilla dice si aumenta o disminuye",
        ));
    }
    let conn = v.connection();
    let clave = if a.clave.trim().is_empty() {
        if a.importe.is_none() {
            return Err(other("falta el importe"));
        }
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM is_ajuste WHERE year = ?1 AND clave LIKE 'manual:%'",
            [year],
            |r| r.get(0),
        )?;
        let mut k = n + 1;
        loop {
            let c = format!("manual:{k}");
            let existe: i64 = conn.query_row(
                "SELECT COUNT(*) FROM is_ajuste WHERE year = ?1 AND clave = ?2",
                params![year, c],
                |r| r.get(0),
            )?;
            if existe == 0 {
                break c;
            }
            k += 1;
        }
    } else {
        a.clave.trim().to_owned()
    };
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO is_ajuste(year, clave, casilla, importe, concepto, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(year, clave) DO UPDATE SET casilla = excluded.casilla,
           importe = excluded.importe, concepto = excluded.concepto,
           updated_at = excluded.updated_at",
        params![
            year,
            clave,
            a.casilla,
            a.importe.map(Money::cents),
            a.concepto.trim(),
            now_str(now)
        ],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "modelo200.ajuste",
        &format!("{year}:{clave}"),
        &serde_json::json!({ "casilla": a.casilla, "importe": a.importe.map(Money::cents) }),
    )?;
    tx.commit()?;
    Ok(clave)
}

/// Quita lo guardado de una corrección: vuelve la propuesta de TaxPain o desaparece la tecleada
/// a mano.
pub fn quitar_ajuste(
    v: &Vault,
    year: i32,
    clave: &str,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    if tx.execute(
        "DELETE FROM is_ajuste WHERE year = ?1 AND clave = ?2",
        params![year, clave],
    )? == 0
    {
        return Err(AppError::NotFound);
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "modelo200.ajuste.delete",
        &format!("{year}:{clave}"),
        &serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Cálculo
// ---------------------------------------------------------------------------------------------

/// Saldo de cada cuenta en el ejercicio (deudor positivo), sin regularización ni cierre; y, aparte,
/// el de los asientos de los ajustes de cierre (`Origin::AjusteCierre`: dotaciones de
/// deterioros y provisiones y sus reversiones), que las propuestas genéricas no deben volver a
/// contar (ver [`propuestas::Contexto::cubiertos`]).
fn saldos_ejercicio(
    v: &Vault,
    year: i32,
) -> Result<(BTreeMap<String, Money>, BTreeMap<String, Money>)> {
    let mut m: BTreeMap<String, Money> = BTreeMap::new();
    let mut cubiertos: BTreeMap<String, Money> = BTreeMap::new();
    for e in accounting::journal(v, year)? {
        if matches!(e.entry.origin, Origin::Regularizacion | Origin::Cierre) {
            continue;
        }
        let ajuste = matches!(e.entry.origin, Origin::AjusteCierre(_));
        for l in &e.entry.lines {
            *m.entry(l.account.as_str().to_owned()).or_default() += l.net();
            if ajuste {
                *cubiertos.entry(l.account.as_str().to_owned()).or_default() += l.net();
            }
        }
    }
    Ok((m, cubiertos))
}

fn nombre_cuenta(v: &Vault, cuenta: &str) -> String {
    let conn = v.connection();
    // La subcuenta y, si no tiene nombre, sus cuentas madre.
    for n in (3..=cuenta.len()).rev() {
        if let Ok(Some(s)) = accounting::account_name(conn, &cuenta[..n]) {
            return s;
        }
    }
    String::new()
}

/// Fondos propios computables para la reserva de capitalización (art. 25.2 LIS) al principio y
/// al final del ejercicio, sin el resultado de cada uno. No computan: las aportaciones de los
/// socios (118), las reservas legales y estatutarias (112, 1141), la reserva de nivelación
/// (subcuentas que lo digan en el nombre), las acciones o participaciones propias (108, 109) ni
/// los instrumentos compuestos (111). `TODO(VERIFY)`: los aumentos de capital con aportaciones
/// o por compensación de créditos (art. 25.2.a y b) no se distinguen: si el capital cambia, se
/// avisa.
pub fn fondos_propios(v: &Vault, year: i32) -> Result<FondosPropios> {
    let fy = accounting::fiscal_year(v, year)?;
    let actual: Vec<JournalEntry> = accounting::journal(v, year)?
        .into_iter()
        .map(|s| s.entry)
        .collect();
    let fin = crate::patrimonial::saldos_a(&actual, fy.end);
    let inicio = match accounting::journal(v, year - 1) {
        Ok(j) if !j.is_empty() => {
            let previos: Vec<JournalEntry> = j.into_iter().map(|s| s.entry).collect();
            let fa = accounting::fiscal_year(v, year - 1)?;
            crate::patrimonial::saldos_a(&previos, fa.end)
        }
        _ => {
            let apertura: Vec<JournalEntry> = actual
                .iter()
                .filter(|e| matches!(e.origin, Origin::Apertura))
                .cloned()
                .collect();
            crate::patrimonial::saldos_a(&apertura, fy.end)
        }
    };
    let mut cuentas: Vec<&String> = inicio.keys().chain(fin.keys()).collect();
    cuentas.sort();
    cuentas.dedup();
    let mut out = FondosPropios::default();
    for c in cuentas {
        if !(c.starts_with("10") || c.starts_with("11") || c.starts_with("12")) {
            continue;
        }
        if c.starts_with("129") {
            continue;
        }
        let i = -inicio.get(c).copied().unwrap_or(Money::ZERO);
        let f = -fin.get(c).copied().unwrap_or(Money::ZERO);
        if i.is_zero() && f.is_zero() {
            continue;
        }
        let nombre = nombre_cuenta(v, c);
        let motivo = if c.starts_with("118") {
            Some("aportaciones de socios (art. 25.2.a)")
        } else if c.starts_with("112") || c.starts_with("1141") {
            Some("reserva legal o estatutaria (art. 25.2.d)")
        } else if c.starts_with("108") || c.starts_with("109") {
            Some("acciones o participaciones propias (art. 25.2.c)")
        } else if c.starts_with("111") {
            Some("instrumentos financieros compuestos (art. 25.2.f)")
        } else if nombre.to_lowercase().contains("nivelaci") {
            Some("reserva de nivelación (art. 25.2.e)")
        } else {
            None
        };
        if c.starts_with("100") && i != f {
            out.avisos.push(format!(
                "El capital ha pasado de {i} a {f}: si el aumento fue con aportaciones de los \
                 socios o por compensación de créditos no computa (art. 25.2.a y b LIS); \
                 corrige el incremento a mano."
            ));
        }
        if motivo.is_none() {
            out.inicio += i;
            out.fin += f;
        }
        out.partidas.push(PartidaFondos {
            cuenta: c.clone(),
            nombre,
            inicio: i,
            fin: f,
            computa: motivo.is_none(),
            motivo: motivo.map(str::to_owned),
        });
    }
    out.incremento = (out.fin - out.inicio).max(Money::ZERO);
    Ok(out)
}

/// Comprueba si la reserva consta dotada: en la aplicación del resultado contabilizada (cuenta
/// 1149 o líneas «reserva de capitalización») o en un acta del ejercicio siguiente que la
/// mencione.
fn dotacion(v: &Vault, year: i32, a_dotar: Money, que: &str, palabra: &str) -> Dotacion {
    if a_dotar.cents() <= 0 {
        return Dotacion {
            a_dotar,
            dotada: None,
            estado: "dotada".into(),
            texto: format!("No hay que dotar reserva de {que}."),
        };
    }
    if let Ok(Some(lineas)) = crate::memoria::applied_result(v, year) {
        let dotada: Money = lineas
            .iter()
            .filter(|(l, _)| l.to_lowercase().contains(palabra))
            .map(|(_, m)| *m)
            .sum();
        if dotada.cents() > 0 || que == "capitalización" {
            let ok = dotada >= a_dotar;
            return Dotacion {
                a_dotar,
                dotada: Some(dotada),
                estado: if ok { "dotada" } else { "falta" }.into(),
                texto: if ok {
                    format!(
                        "La aplicación del resultado de {year} dota {dotada} a la reserva de \
                         {que}."
                    )
                } else {
                    format!(
                        "La aplicación del resultado de {year} dota {dotada} a la reserva de \
                         {que} y hacen falta {a_dotar}: sin dotarla se pierde la reducción."
                    )
                },
            };
        }
    }
    let actas = crate::libros_societarios::actas::minutes(v, Some(year + 1)).unwrap_or_default();
    if let Some(m) = actas
        .iter()
        .find(|m| m.agreements.to_lowercase().contains(palabra))
    {
        return Dotacion {
            a_dotar,
            dotada: None,
            estado: "en_acta".into(),
            texto: format!(
                "El acta del {} habla de la reserva de {que}: comprueba que dota al menos \
                 {a_dotar} y contabiliza la aplicación del resultado.",
                dmy(m.date)
            ),
        };
    }
    Dotacion {
        a_dotar,
        dotada: None,
        estado: "pendiente".into(),
        texto: format!(
            "Hay que dotar {a_dotar} de reserva de {que} indisponible con la aplicación del \
             resultado de {year} (junta de {}): aún no consta en la contabilidad ni en el libro \
             de actas.",
            year + 1
        ),
    }
}

/// Régimen de tipos que propone TaxPain con la cifra de negocios del periodo anterior.
fn regimen_propuesto(
    cifra: Option<Money>,
    patrimonial: Option<bool>,
    guardado: TaxRegime,
) -> (TaxRegime, String) {
    if patrimonial == Some(true) {
        return (
            TaxRegime::General,
            "Entidad patrimonial: tipo general (art. 29.1 LIS) y sin incentivos de reducida \
             dimensión (art. 101.1)."
                .into(),
        );
    }
    if guardado == TaxRegime::NuevaCreacion {
        return (
            TaxRegime::NuevaCreacion,
            "Nueva creación: 15 % en el primer periodo con base positiva y el siguiente (art. \
             29.1 LIS)."
                .into(),
        );
    }
    match cifra {
        Some(c) if c < Money::from_euros(1_000_000, 0) => (
            TaxRegime::Micro,
            format!(
                "Cifra de negocios del periodo anterior {c}: menos de 1 M€, tipos de las \
                 microempresas (art. 29.1 LIS, Ley 7/2024)."
            ),
        ),
        Some(c) if c < Money::from_euros(10_000_000, 0) => (
            TaxRegime::ReducidaDimension,
            format!(
                "Cifra de negocios del periodo anterior {c}: menos de 10 M€, entidad de reducida \
                 dimensión (arts. 29.1 y 101 LIS)."
            ),
        ),
        Some(c) => (
            TaxRegime::General,
            format!("Cifra de negocios del periodo anterior {c}: tipo general (art. 29.1 LIS)."),
        ),
        None => (
            guardado,
            "Sin la cifra de negocios del periodo anterior: se usa el régimen guardado.".into(),
        ),
    }
}

/// Todo lo que entra en el 200 de un ejercicio.
pub(crate) struct Preparado {
    pub input: Input200,
    pub fy: FiscalYear,
    pub legacy_year: bool,
    /// 202 sin presentar cuyo borrador se ha usado.
    pub estimated: Vec<String>,
    pub ajustes: Vec<AjusteVista>,
    pub participadas: Vec<ParticipadaVista>,
    pub fondos: FondosPropios,
    pub datos: Datos200,
    pub arrastres: Vec<ArrastreGuardado>,
    pub regimen_guardado: TaxRegime,
    pub regimen_propuesto: (TaxRegime, String),
    pub patrimonial: Option<bool>,
    pub cifra_negocios: Money,
    pub retenciones_473: Money,
    pub di_origen: String,
    pub bins_origen: String,
    pub notas: Vec<String>,
    pub avisos: Vec<String>,
}

pub(crate) fn preparar(v: &Vault, year: i32) -> Result<Preparado> {
    let s = crate::models::settings(v)?;
    let (ys, legacy_year) = s.year_settings(year);
    let fy = accounting::fiscal_year(v, year)?;
    let end = fy.end;
    let (income, expenses) = crate::models::income_expenses(v, year, end)?;
    let mut avisos = Vec::new();
    let mut notas = Vec::new();
    let (saldos, cubiertos) = saldos_ejercicio(v, year)?;
    // Gasto por IS: 630 y 633 al debe suman; 638 al haber resta.
    let tax_expense: Money = saldos
        .iter()
        .filter(|(c, _)| crate::models::is_profit_tax(c))
        .map(|(_, m)| *m)
        .sum();
    // Pagos fraccionados del ejercicio: los 202 a cuenta del período impositivo en curso el día
    // 1 de abril, octubre y diciembre (art. 40.1 LIS), en orden. En un ejercicio de julio a
    // junio (el 2026/2027) son el 2P y el 3P de 2026 y el 1P de 2027; en uno natural, los tres
    // del año; en uno más corto, los que caigan dentro. Los presentados (desde TaxPain o por el
    // gestor, importados o tecleados) o, si no, su borrador.
    let periods = (fy.start.year()..=fy.end.year())
        .flat_map(|y| {
            [
                (y, 1u8, Month::April),
                (y, 2, Month::October),
                (y, 3, Month::December),
            ]
        })
        .filter(|(y, _, m)| Date::from_calendar_date(*y, *m, 1).is_ok_and(|d| fy.contains(d)));
    let mut installments = [Money::ZERO; 3];
    let mut estimated = Vec::new();
    for (slot, (y, p, _)) in installments.iter_mut().zip(periods) {
        *slot = match crate::models::filed_boxes(v, "202", y, &format!("{p}P"))? {
            Some((_, boxes)) => taxpain_models::m202::amount_due(&boxes),
            None => {
                estimated.push(format!("{p}P {y}"));
                crate::models::draft_202(v, y, p).map_or(Money::ZERO, |d| d.result)
            }
        }
        .max(Money::ZERO);
    }
    let amortizacion = accounting::depreciation_differences(v, year)?;
    // Cifra de negocios de los 12 meses anteriores al inicio del periodo (la del último periodo
    // cerrado: su 200 o la contabilidad), para los límites de las grandes empresas (DA 15.ª LIS),
    // el tramo de la página 01 (campos 102 a 104) y el régimen propuesto.
    let turnover = crate::payroll::director_turnover(v, fy.start)
        .ok()
        .and_then(|t| t.turnover);
    let datos = datos(v, year)?;
    let participadas = participadas::fichas(v, year)?;
    let nombres: BTreeMap<String, String> = saldos
        .keys()
        .filter(|c| c.starts_with('6'))
        .map(|c| (c.clone(), nombre_cuenta(v, c)))
        .collect();
    let valores = crate::valores::contabilidad::fiscal(v, year)?;
    let (mut props, n) = propuestas::calcular(&propuestas::Contexto {
        saldos: &saldos,
        cubiertos: &cubiertos,
        nombres: &nombres,
        participadas: &participadas,
        amortizacion,
        dias: fy.days(),
        valores: &valores,
    });
    notas.extend(n);
    // Deterioros de créditos y provisiones no deducibles de los ajustes de cierre (Contabilidad →
    // Cierre): el ajuste ya dice si es deducible, así que entran incluidas, cada una en su
    // casilla ([00321]/[00322], [00337]/[00338]). Sus asientos no cuentan en las genéricas.
    for c in crate::ajustes_cierre::correcciones_200(v.connection(), year)? {
        let que = c.tipo.label().to_lowercase();
        let (clave, concepto, nota) = if c.reversion {
            (
                format!("cierre:{}:reversion", c.id),
                format!(
                    "Reversión de {que} no deducible de {}: {}",
                    c.dotado, c.descripcion
                ),
                Some(format!(
                    "No se dedujo al dotarlo en {}: la reversión no tributa (arts. 13.1 y 14 LIS).",
                    c.dotado
                )),
            )
        } else {
            (
                format!("cierre:{}", c.id),
                format!("{} no deducible: {}", c.tipo.label(), c.descripcion),
                None,
            )
        };
        props.push(Propuesta {
            clave,
            casilla: c.casilla(),
            importe: c.importe,
            concepto,
            origen: "Ajustes de cierre".into(),
            nota,
            incluir: true,
        });
    }
    // Lo tecleado en «Datos de los modelos» (antes de la hoja del 200) sigue valiendo.
    for (clave, casilla, importe, concepto) in [
        (
            "datos_multas",
            1815u16,
            ys.corporate_fines,
            "Multas y sanciones (Datos de los modelos)",
        ),
        (
            "datos_aumentos",
            413,
            ys.corporate_increases,
            "Otras correcciones en aumento (Datos de los modelos)",
        ),
        (
            "datos_disminuciones",
            414,
            ys.corporate_decreases,
            "Otras correcciones en disminución (Datos de los modelos)",
        ),
    ] {
        if importe.cents() > 0 {
            props.push(Propuesta {
                clave: clave.into(),
                casilla,
                importe,
                concepto: concepto.into(),
                origen: "Modelos → Datos de los modelos".into(),
                nota: None,
                incluir: true,
            });
        }
    }
    if ys.corporate_fines.cents() > 0
        && let Some(p) = props.iter_mut().find(|p| p.clave == "multas")
    {
        p.incluir = false;
        p.nota = Some(
            "Sin incluir: ya hay multas tecleadas en «Datos de los modelos». Inclúyela si son \
             otras."
                .into(),
        );
    }
    if ys.corporate_decreases.cents() > 0 && props.iter().any(|p| p.clave.starts_with("art21:")) {
        avisos.push(
            "Hay correcciones en disminución tecleadas en «Datos de los modelos» y la exención del \
             art. 21 sale ahora de la ficha de participadas: comprueba que no se resta dos veces."
                .into(),
        );
    }
    let guardados = ajustes_guardados(v, year)?;
    let mut ajustes = Vec::new();
    for p in &props {
        let g = guardados.iter().find(|g| g.clave == p.clave);
        let c = m200_catalogo::correccion(p.casilla);
        let (importe, estado) = match g {
            Some(g) => match g.importe {
                None => (Money::ZERO, "descartada"),
                Some(m) => (m, "editada"),
            },
            None if p.incluir => (p.importe, "propuesta"),
            None => (Money::ZERO, "sin_incluir"),
        };
        ajustes.push(AjusteVista {
            clave: p.clave.clone(),
            casilla: g.map_or(p.casilla, |g| g.casilla),
            aumento: c.is_some_and(|c| c.aumento),
            concepto: p.concepto.clone(),
            norma: c.map_or(String::new(), |c| c.norma.to_owned()),
            importe,
            propuesto: Some(p.importe),
            origen: p.origen.clone(),
            estado: estado.into(),
            nota: p.nota.clone(),
        });
    }
    for g in guardados
        .iter()
        .filter(|g| !props.iter().any(|p| p.clave == g.clave))
    {
        let Some(c) = m200_catalogo::correccion(g.casilla) else {
            continue;
        };
        if !g.clave.starts_with("manual:") {
            // Propuesta que ya no sale (p. ej. se borró el asiento): se conserva lo tecleado.
            avisos.push(format!(
                "La corrección «{}» guardada ya no sale de la contabilidad: se mantiene lo \
                 tecleado ({}).",
                g.clave,
                g.importe.unwrap_or(Money::ZERO)
            ));
        }
        ajustes.push(AjusteVista {
            clave: g.clave.clone(),
            casilla: g.casilla,
            aumento: c.aumento,
            concepto: if g.concepto.is_empty() {
                c.concepto.to_owned()
            } else {
                g.concepto.clone()
            },
            norma: c.norma.to_owned(),
            importe: g.importe.unwrap_or(Money::ZERO),
            propuesto: None,
            origen: "tecleada".into(),
            estado: "manual".into(),
            nota: None,
        });
    }

    // Dividendos de la contabilidad frente a la ficha de participadas.
    let d760 = crate::models::dividends_760(v, year)?;
    let fichas_bruto: Money = participadas.iter().map(|p| p.bruto).sum();
    if d760.cents() > 0
        && fichas_bruto.is_zero()
        && ys.corporate_decreases < d760.apply_rate(Rate::percent(95))
    {
        avisos.push(format!(
            "Hay {d760} de dividendos de participadas en la 760 y ninguno en la ficha de \
             participadas: si tienes al menos el 5 % desde hace un año, el 95 % está exento \
             (art. 21 LIS). Regístralos en Modelos → 200 → Participadas para que TaxPain lo \
             compruebe y proponga la exención ([00370])."
        ));
    } else if d760.cents() > 0 && (d760 - fichas_bruto).abs() > Money::from_euros(1, 0) {
        avisos.push(format!(
            "Los dividendos de la 760 ({d760}) no coinciden con los de la ficha de participadas \
             ({fichas_bruto}): revisa que estén todos (la 760 también recoge los de cartera)."
        ));
    }
    for p in &participadas {
        avisos.extend(
            p.avisos
                .iter()
                .map(|a| format!("{}: {a}", p.participada.name)),
        );
    }

    // Arrastres de otros ejercicios.
    let arrastres_y = arrastres(v, year)?;
    let de = |t: TipoArrastre| -> Vec<Arrastre> {
        arrastres_y
            .iter()
            .filter(|a| a.tipo == t && a.pendiente.cents() > 0)
            .map(|a| Arrastre {
                origen: a.origen,
                importe: a.pendiente,
            })
            .collect()
    };
    let bins = de(TipoArrastre::Bin);
    let bins_origen = if !bins.is_empty() {
        "arrastres"
    } else if ys.pending_losses.cents() > 0 {
        avisos.push(
            "Las bases negativas pendientes están como un total en «Datos de los modelos»: \
             añádelas por año de origen en el bloque de bases negativas (página 15 del 200)."
                .into(),
        );
        "datos"
    } else {
        "ninguna"
    };

    // Reserva de capitalización.
    let fondos = fondos_propios(v, year)?;
    let pend_cap = de(TipoArrastre::Capitalizacion);
    let capitalizacion = if datos.aplicar_capitalizacion || !pend_cap.is_empty() {
        if datos.aplicar_capitalizacion && ys.capitalization_reserve.cents() > 0 {
            avisos.push(
                "La reserva de capitalización se calcula ahora con el incremento de fondos \
                 propios: el importe de «Datos de los modelos» no se usa."
                    .into(),
            );
        }
        Some(Capitalizacion {
            incremento: if datos.aplicar_capitalizacion {
                datos.incremento_fondos_propios.unwrap_or(fondos.incremento)
            } else {
                Money::ZERO
            },
            porcentaje: datos.porcentaje_capitalizacion,
            pendientes: pend_cap,
        })
    } else {
        None
    };
    let pend_niv = de(TipoArrastre::Nivelacion);
    let nivelacion = (datos.nivelacion.cents() > 0 || !pend_niv.is_empty()).then_some(Nivelacion {
        minorar: datos.nivelacion,
        pendientes: pend_niv,
    });

    // Entidad patrimonial (art. 5.2 LIS), con el test del ejercicio ya cerrado.
    let patrimonial = if accounting::journal(v, year)?.is_empty() {
        None
    } else {
        crate::patrimonial::informe(v, year, fy.end + time::Duration::days(1))
            .ok()
            .map(|i| i.patrimonial)
    };
    let regimen_guardado = ys.corporate_regime.unwrap_or(s.corporate_regime);
    let propuesto = regimen_propuesto(turnover, patrimonial, regimen_guardado);
    if propuesto.0 != regimen_guardado && patrimonial != Some(true) && turnover.is_some() {
        avisos.push(format!(
            "El régimen de tipos guardado no es el que corresponde por la cifra de negocios: {} \
             Cámbialo en «Datos de los modelos» si es así.",
            propuesto.1
        ));
    }

    // Doble imposición internacional: Valores y dividendos de participadas no residentes sin
    // exención (art. 31 LIS: la menor de lo pagado fuera y la cuota española de esa renta).
    let (_, tipo) = m200::gross_tax(Money::from_euros(100, 0), regimen_guardado, year);
    let tipo = if tipo.bp() > 0 {
        tipo
    } else {
        Rate::percent(25)
    };
    let di_valores = crate::valores::informe::report(v, year)
        .map(|r| r.doble_imposicion.deducible)
        .unwrap_or(Money::ZERO);
    let di_participadas: Money = participadas
        .iter()
        .filter(|p| p.participada.country != "ES")
        .flat_map(|p| &p.dividendos)
        .filter(|d| d.art21.estado == EstadoArt21::NoCumple)
        .map(|d| {
            d.dividendo
                .retencion
                .min(d.dividendo.bruto.apply_rate(tipo))
        })
        .sum();
    let (di, di_origen) = match datos.doble_imposicion {
        Some(m) => (m, "tecleada".to_owned()),
        None => (
            di_valores + di_participadas,
            format!(
                "Valores ({di_valores}) y participadas no residentes sin exención \
                 ({di_participadas})"
            ),
        ),
    };
    let mut cuota: Vec<Importe> = datos.cuota.clone();
    if di.cents() > 0 {
        cuota.push(Importe {
            casilla: 573,
            importe: di,
        });
    }
    let di_ant: Money = de(TipoArrastre::DobleImposicion)
        .iter()
        .map(|a| a.importe)
        .sum();
    if di_ant.cents() > 0 {
        cuota.push(Importe {
            casilla: 571,
            importe: di_ant,
        });
    }

    // Retenciones: las de la 473 con su desglose propuesto (o el tecleado).
    let retenciones_473 = crate::models::withholdings_473(v, year)?;
    let retenciones = match &datos.retenciones {
        Some(r) => r.clone(),
        None => {
            let facturas =
                crate::models::issued_withholdings_between(v, fy.start, end)?.min(retenciones_473);
            let participadas_ret: Money = participadas
                .iter()
                .filter(|p| p.participada.country == "ES")
                .map(|p| p.retenido)
                .sum();
            let mobiliario = participadas_ret.min(retenciones_473 - facturas);
            let resto = retenciones_473 - facturas - mobiliario;
            let hay_76 = saldos
                .iter()
                .any(|(c, m)| c.starts_with("76") && m.cents() < 0);
            let mut out = vec![
                Importe {
                    casilla: 1793,
                    importe: facturas,
                },
                Importe {
                    casilla: 1785,
                    importe: mobiliario + if hay_76 { resto } else { Money::ZERO },
                },
            ];
            if !hay_76 {
                out.push(Importe {
                    casilla: 1798,
                    importe: resto,
                });
            }
            out.retain(|x| x.importe.cents() > 0);
            // TODO(VERIFY): a qué casilla van las retenciones que no son de facturas ni de
            // participadas.
            if resto.cents() > 0 {
                notas.push(format!(
                    "{resto} de retenciones de la 473 no vienen de facturas ni de participadas: \
                     se proponen como {}. Cámbialas si son de alquileres ([01787]) o de fondos \
                     ([01795]).",
                    if hay_76 {
                        "de capital mobiliario ([01785])"
                    } else {
                        "otras ([01798])"
                    }
                ));
            }
            out
        }
    };

    let input = Input200 {
        turnover,
        accounting_result: income - expenses - tax_expense,
        tax_expense: tax_expense.max(Money::ZERO),
        tax_income: (-tax_expense).max(Money::ZERO),
        capitalization_reserve: ys.capitalization_reserve,
        turnover_under_1m: s.turnover_under_1m,
        pending_losses: ys.pending_losses,
        regime: regimen_guardado,
        withholdings: retenciones_473,
        installments,
        ajustes: ajustes
            .iter()
            .filter(|a| a.importe.cents() > 0)
            .map(|a| Importe {
                casilla: a.casilla,
                importe: a.importe,
            })
            .collect(),
        bins,
        bin_nueva_creacion: datos.bin_nueva_creacion,
        capitalizacion,
        nivelacion,
        patrimonial: patrimonial == Some(true),
        cuota,
        retenciones,
        ..Input200::default()
    };
    Ok(Preparado {
        input,
        fy,
        legacy_year,
        estimated,
        ajustes,
        participadas,
        fondos,
        datos,
        arrastres: arrastres_y,
        regimen_guardado,
        regimen_propuesto: propuesto,
        patrimonial,
        cifra_negocios: propuestas::cifra_de_negocios(&saldos),
        retenciones_473,
        di_origen,
        bins_origen: bins_origen.into(),
        notas,
        avisos,
    })
}

/// Participadas para la página 02 del fichero.
pub(crate) fn participadas_200(ps: &[ParticipadaVista]) -> Vec<m200::Participada200> {
    ps.iter()
        .map(|p| {
            let f = p.fondos.clone().unwrap_or_default();
            m200::Participada200 {
                nif: p.participada.nif.clone(),
                nombre: p.participada.name.clone(),
                codigo: if p.participada.country == "ES" {
                    p.participada.province.clone()
                } else {
                    p.participada.country.clone()
                },
                pct: p.participada.pct,
                nominal: p.fiscal.nominal,
                valor_libros: p.participada.cost,
                dividendos: p.bruto,
                capital: f.capital,
                reservas: f.reservas,
                otras_pn: f.otras_pn,
                resultado: f.resultado,
            }
        })
        .collect()
}

/// Lo que pasa al ejercicio siguiente tras la liquidación `r`.
fn siguiente(year: i32, p: &Preparado, r: &Result200) -> Vec<ArrastreGuardado> {
    let mut out = Vec::new();
    let mut push = |tipo, origen, pendiente: Money, nota: &str| {
        if pendiente.cents() > 0 {
            out.push(ArrastreGuardado {
                tipo,
                origen,
                ejercicio: year + 1,
                pendiente,
                nota: nota.into(),
            });
        }
    };
    for d in &r.bins {
        push(TipoArrastre::Bin, d.origen, d.pendiente, "");
    }
    if r.bins.is_empty() && p.input.pending_losses.cents() > 0 {
        push(
            TipoArrastre::Bin,
            year - 1,
            p.input.pending_losses - r.get(547),
            "Total sin año de origen de «Datos de los modelos»: corrige el año.",
        );
    }
    push(TipoArrastre::Bin, year, r.bin_generada, "");
    if let Some(n) = &r.nivelacion {
        for d in &n.detalle {
            push(TipoArrastre::Nivelacion, d.origen, d.pendiente, "");
        }
    }
    if let Some(c) = &r.capitalizacion {
        for d in &c.detalle {
            push(TipoArrastre::Capitalizacion, d.origen, d.pendiente, "");
        }
    }
    push(TipoArrastre::DobleImposicion, year, r.di_pendiente, "");
    out
}

/// Liquidación del ejercicio con lo preparado.
pub(crate) fn liquidar(p: &Preparado) -> Result200 {
    m200::compute_for(p.fy.year, &p.input, p.fy.days())
}

/// La hoja del 200 del ejercicio `year`.
pub fn hoja(v: &Vault, year: i32) -> Result<Hoja200> {
    crate::models::exige_sociedad(v)?;
    let p = preparar(v, year)?;
    let r = liquidar(&p);
    let g = |b: u16| r.get(b);
    let resultado = vec![
        linea(
            500,
            "Resultado de la cuenta de pérdidas y ganancias",
            g(500),
            "contabilidad (grupos 6 y 7)",
            "art. 10.3 LIS",
        ),
        linea(
            301,
            "Impuesto sobre Sociedades contabilizado (aumento)",
            g(301),
            "cuentas 630 y 633",
            "art. 15.b LIS",
        ),
        linea(
            302,
            "Ingreso por Impuesto sobre Sociedades (disminución)",
            g(302),
            "cuenta 638",
            "art. 15.b LIS",
        ),
        linea(
            501,
            "Resultado antes del Impuesto sobre Sociedades",
            g(501),
            "[00500] + [00301] − [00302]",
            "art. 10.3 LIS",
        ),
    ];
    let base_previa = vec![
        linea(
            417,
            "Total correcciones en aumento",
            g(417),
            "suma de los aumentos",
            "art. 10.3 LIS",
        ),
        linea(
            418,
            "Total correcciones en disminución",
            g(418),
            "suma de las disminuciones",
            "art. 10.3 LIS",
        ),
        linea(
            550,
            "Base imponible previa",
            g(550),
            "[00501] + [00417] − [00418]",
            "art. 10 LIS",
        ),
    ];
    let mut base = vec![
        linea(
            1032,
            "Reserva de capitalización",
            g(1032),
            "bloque de reservas",
            "art. 25 LIS",
        ),
        linea(
            547,
            "Compensación de bases imponibles negativas",
            g(547),
            "bloque de bases negativas",
            "art. 26 LIS",
        ),
        linea(
            552,
            "Base imponible",
            g(552),
            "[00550] − [01032] − [00547]",
            "art. 10 LIS",
        ),
    ];
    if r.boxes.contains_key(&1330) {
        base.extend([
            linea(
                1033,
                "Reserva de nivelación: adiciones",
                g(1033),
                "minoraciones de ejercicios anteriores",
                "art. 105.1 LIS",
            ),
            linea(
                1034,
                "Reserva de nivelación: minoración",
                g(1034),
                "bloque de reservas",
                "art. 105.1 LIS",
            ),
            linea(
                1330,
                "Base imponible después de la reserva de nivelación",
                g(1330),
                "[00552] + [01033] − [01034]",
                "art. 105 LIS",
            ),
        ]);
    }
    let tipo = Money::from_cents(g(558).cents());
    let mut cuota = vec![
        linea(
            558,
            "Tipo de gravamen (%)",
            tipo,
            &format!("{:?}", r.regimen),
            "art. 29 LIS y DT 44.ª (Ley 7/2024)",
        ),
        linea(
            562,
            "Cuota íntegra",
            g(562),
            "base × tipo (micro: por tramos)",
            "art. 30 LIS",
        ),
    ];
    for c in m200_catalogo::CUOTA
        .iter()
        .filter(|c| c.grupo != GrupoCuota::Retencion)
    {
        if !g(c.casilla).is_zero() {
            let origen = if c.casilla == 573 {
                p.di_origen.clone()
            } else if c.casilla == 571 {
                "pendiente de ejercicios anteriores".into()
            } else {
                "tecleada".into()
            };
            cuota.push(linea(c.casilla, c.concepto, g(c.casilla), &origen, c.norma));
        }
        if c.casilla == 573 {
            cuota.push(linea(
                582,
                "Cuota íntegra ajustada positiva",
                g(582),
                "[00562] − bonificaciones − doble imposición",
                "art. 30 LIS",
            ));
        }
    }
    cuota.push(linea(
        592,
        "Cuota líquida",
        g(592),
        "[00582] − deducciones",
        "art. 30 LIS",
    ));
    let mut retenciones = Vec::new();
    for c in m200_catalogo::CUOTA
        .iter()
        .filter(|c| c.grupo == GrupoCuota::Retencion)
    {
        if !g(c.casilla).is_zero() {
            let origen = if p.datos.retenciones.is_some() {
                "tecleada"
            } else if c.casilla == 1793 {
                "facturas emitidas con retención"
            } else {
                "cuenta 473 y ficha de participadas"
            };
            retenciones.push(linea(c.casilla, c.concepto, g(c.casilla), origen, c.norma));
        }
    }
    retenciones.push(linea(
        1766,
        "Total retenciones e ingresos a cuenta",
        g(1766),
        &format!("cuenta 473 ({})", p.retenciones_473),
        "art. 41 LIS",
    ));
    let origen_202 = |i: usize| {
        if p.estimated.len() > i {
            "borrador del 202 (no consta presentado)"
        } else {
            "202 presentado"
        }
    };
    let liquidacion = vec![
        linea(
            599,
            "Cuota del ejercicio a ingresar o a devolver",
            g(599),
            "[00592] − [01766]",
            "art. 41 LIS",
        ),
        linea(
            601,
            "Primer pago fraccionado",
            g(601),
            origen_202(0),
            "art. 40 LIS",
        ),
        linea(
            603,
            "Segundo pago fraccionado",
            g(603),
            origen_202(1),
            "art. 40 LIS",
        ),
        linea(
            605,
            "Tercer pago fraccionado",
            g(605),
            origen_202(2),
            "art. 40 LIS",
        ),
        linea(
            611,
            "Cuota diferencial",
            g(611),
            "[00599] − pagos fraccionados",
            "art. 41 LIS",
        ),
        linea(
            1586,
            "Resultado de la autoliquidación",
            g(1586),
            "a ingresar si es positivo, a devolver si es negativo",
            "art. 124 LIS",
        ),
    ];
    let cap = match &r.capitalizacion {
        Some(c) => ReservaVista {
            lineas: vec![
                Linea {
                    casilla: None,
                    concepto: "Incremento de fondos propios".into(),
                    importe: p
                        .input
                        .capitalizacion
                        .as_ref()
                        .map_or(Money::ZERO, |x| x.incremento),
                    origen: if p.datos.incremento_fondos_propios.is_some() {
                        "tecleado".into()
                    } else {
                        "balance (fondos propios computables)".into()
                    },
                    norma: "art. 25.2 LIS".into(),
                },
                Linea {
                    casilla: None,
                    concepto: format!("Derecho del ejercicio ({} %)", c.porcentaje),
                    importe: c.derecho,
                    origen: "incremento × porcentaje".into(),
                    norma: "art. 25.1 LIS".into(),
                },
                Linea {
                    casilla: None,
                    concepto: "Tope (porcentaje de la base previa)".into(),
                    importe: c.tope,
                    origen: "[00550]".into(),
                    norma: "art. 25.1 LIS".into(),
                },
                linea(
                    1032,
                    "Reducción aplicada",
                    c.aplicado,
                    "derecho y pendientes, hasta el tope",
                    "art. 25.1 LIS",
                ),
            ],
            detalle: c.detalle.clone(),
            dotacion: Some(dotacion(v, year, c.a_dotar, "capitalización", "capitaliz")),
        },
        None => ReservaVista {
            lineas: if g(1032).is_zero() {
                vec![]
            } else {
                vec![linea(
                    1032,
                    "Reserva de capitalización (importe de «Datos de los modelos»)",
                    g(1032),
                    "Datos de los modelos",
                    "art. 25 LIS",
                )]
            },
            detalle: vec![],
            dotacion: None,
        },
    };
    let niv = match &r.nivelacion {
        Some(n) => ReservaVista {
            lineas: vec![
                Linea {
                    casilla: None,
                    concepto: "Tope (10 % de la base, hasta 1 M€)".into(),
                    importe: n.tope,
                    origen: "[00552]".into(),
                    norma: "art. 105.1 LIS".into(),
                },
                linea(
                    1034,
                    "Minoración del ejercicio",
                    n.minorado,
                    "tecleada (con el tope)",
                    "art. 105.1 LIS",
                ),
                linea(
                    1033,
                    "Adiciones de ejercicios anteriores",
                    n.adicionado,
                    "base negativa o quinto año",
                    "art. 105.1 LIS",
                ),
            ],
            detalle: n.detalle.clone(),
            dotacion: Some(dotacion(v, year, n.a_dotar, "nivelación", "nivelaci")),
        },
        None => ReservaVista {
            lineas: vec![],
            detalle: vec![],
            dotacion: None,
        },
    };
    let mut avisos = p.avisos.clone();
    avisos.extend(p.fondos.avisos.iter().cloned());
    avisos.extend(r.warnings.iter().cloned());
    for d in [&cap.dotacion, &niv.dotacion].into_iter().flatten() {
        if d.estado == "falta" {
            avisos.push(d.texto.clone());
        }
    }
    let siguiente = siguiente(year, &p, &r);
    Ok(Hoja200 {
        year,
        inicio: p.fy.start,
        fin: p.fy.end,
        regimen_guardado: p.regimen_guardado,
        regimen_propuesto: p.regimen_propuesto.0,
        regimen_motivo: p.regimen_propuesto.1.clone(),
        regimen_aplicado: r.regimen,
        patrimonial: p.patrimonial,
        cifra_negocios_anterior: p.input.turnover,
        cifra_negocios: p.cifra_negocios,
        resultado,
        ajustes: p.ajustes.clone(),
        base_previa,
        bins: r.bins.clone(),
        bin_generada: r.bin_generada,
        bins_origen: p.bins_origen.clone(),
        base,
        fondos_propios: p.fondos.clone(),
        capitalizacion: cap,
        nivelacion: niv,
        cuota,
        retenciones,
        liquidacion,
        participadas: p.participadas.clone(),
        arrastres: p.arrastres.clone(),
        siguiente,
        datos: p.datos.clone(),
        catalogo: m200_catalogo::CORRECCIONES.to_vec(),
        catalogo_cuota: m200_catalogo::CUOTA.to_vec(),
        notas: p.notas.clone(),
        avisos,
    })
}

/// Guarda como arrastres del ejercicio siguiente lo que queda pendiente tras el 200 de `year`
/// (sustituye los que hubiera de ese ejercicio). Devuelve cuántos.
pub fn pasar_al_siguiente(
    v: &Vault,
    year: i32,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<usize> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    crate::models::exige_sociedad(v)?;
    let p = preparar(v, year)?;
    let r = liquidar(&p);
    let filas = siguiente(year, &p, &r);
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM is_arrastre WHERE ejercicio = ?1", [year + 1])?;
    for a in &filas {
        tx.execute(
            "INSERT INTO is_arrastre(tipo, origen, ejercicio, pendiente, nota, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                a.tipo.as_str(),
                a.origen,
                a.ejercicio,
                a.pendiente.cents(),
                a.nota,
                now_str(now)
            ],
        )?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "modelo200.pasar",
        &year.to_string(),
        &serde_json::json!({ "filas": filas.len() }),
    )?;
    tx.commit()?;
    Ok(filas.len())
}
