//! Rentas pagadas a no residentes sin establecimiento permanente (G30): certificados de
//! residencia fiscal de los perceptores, registro de las rentas con el tipo propuesto
//! (`taxpain_models::irnr::proponer`), y borradores del 216 (trimestral) y del 296 (anual).
//!
//! De dónde salen las rentas:
//! - Las apuntadas aquí (`renta_no_residente`): intereses de préstamos de no residentes,
//!   cánones, servicios sujetos y otras. Se pueden apuntar desde una factura recibida de un
//!   proveedor extranjero (`candidatas`), que queda ligada.
//! - Los dividendos pagados a socios no residentes (`dividend_line` con resident = 0, Socios →
//!   Pagar dividendos), con la retención que se practicó al pagarlos (el 19 %).
//!
//! Apuntar una renta no hace asientos: la retención se contabiliza con el pago (la 4751 de no
//! residentes) como hasta ahora. `TODO(VERIFY)`: asiento de la retención en las recibidas de
//! proveedores extranjeros y cuadre con la 4751.
//!
//! Nada se presenta: no hay diseño de registro del 216 ni del 296 en `fixtures/aeat/dr`, así que
//! no se genera fichero; el borrador sirve para teclearlo en el formulario de la Sede.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::money::{Money, Rate};
use taxpain_models::irnr::{
    self, CertificadoResidencia, Entrada, Exencion, Naturaleza, Propuesta, Regimen, Renta,
};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, params};
use time::{Date, Month, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;
use crate::libros_societarios::parse_date;
use crate::models::Draft;

pub use taxpain_models::irnr::Propuesta as PropuestaIrnr;

fn other(e: impl ToString) -> AppError {
    AppError::Other(e.to_string())
}

/// Certificado de residencia fiscal guardado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Certificado {
    #[serde(default)]
    pub id: i64,
    pub nif: String,
    pub nombre: String,
    pub pais: String,
    pub expedido: Date,
    /// Sin indicar: un año desde la expedición.
    #[serde(default)]
    pub valido_hasta: Option<Date>,
    #[serde(default = "si")]
    pub convenio: bool,
    #[serde(default)]
    pub documento: String,
    #[serde(default)]
    pub notas: String,
}

fn si() -> bool {
    true
}

impl Certificado {
    fn residencia(&self) -> CertificadoResidencia {
        CertificadoResidencia {
            pais: self.pais.clone(),
            expedido: self.expedido,
            valido_hasta: self
                .valido_hasta
                .unwrap_or_else(|| irnr::valido_hasta(self.expedido)),
            convenio: self.convenio,
        }
    }
}

const CERT_COLS: &str = "id, nif, nombre, pais, expedido, valido_hasta, convenio, documento, notas";

fn cert_of(r: &taxpain_vault::rusqlite::Row<'_>) -> taxpain_vault::rusqlite::Result<Certificado> {
    let d = |s: String| parse_date(&s).ok();
    let expedido: String = r.get(4)?;
    let hasta: String = r.get(5)?;
    Ok(Certificado {
        id: r.get(0)?,
        nif: r.get(1)?,
        nombre: r.get(2)?,
        pais: r.get(3)?,
        expedido: d(expedido).unwrap_or(Date::MIN),
        valido_hasta: d(hasta),
        convenio: r.get(6)?,
        documento: r.get(7)?,
        notas: r.get(8)?,
    })
}

pub fn certificados(v: &Vault) -> Result<Vec<Certificado>> {
    let mut st = v.connection().prepare(&format!(
        "SELECT {CERT_COLS} FROM certificado_residencia ORDER BY nif, expedido DESC, id DESC"
    ))?;
    let rows = st.query_map([], cert_of)?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// El certificado del perceptor que cubre `fecha` o, si ninguno, el más reciente.
fn certificado_de(conn: &Connection, nif: &str, fecha: Date) -> Result<Option<Certificado>> {
    let mut st = conn.prepare(&format!(
        "SELECT {CERT_COLS} FROM certificado_residencia WHERE upper(nif) = ?1
          ORDER BY expedido DESC, id DESC"
    ))?;
    let all: Vec<Certificado> = st
        .query_map([nif.trim().to_uppercase()], cert_of)?
        .collect::<std::result::Result<_, _>>()?;
    Ok(all
        .iter()
        .find(|c| c.residencia().vigente(fecha))
        .or_else(|| all.first())
        .cloned())
}

pub fn guardar_certificado(v: &Vault, c: &Certificado, now: OffsetDateTime) -> Result<i64> {
    if c.nif.trim().is_empty() || c.nombre.trim().is_empty() {
        return Err(other(
            "pon el identificador fiscal y el nombre del perceptor",
        ));
    }
    let pais = irnr::pais(&c.pais);
    if pais.len() != 2 || !pais.chars().all(|x| x.is_ascii_uppercase()) {
        return Err(other("el país es el código de dos letras (FR, US…)"));
    }
    if pais == "ES" {
        return Err(other(
            "un certificado de residencia en España no sirve para el IRNR",
        ));
    }
    let hasta = c
        .valido_hasta
        .unwrap_or_else(|| irnr::valido_hasta(c.expedido));
    if hasta < c.expedido {
        return Err(other("la validez acaba antes de la expedición"));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let args = params![
        c.nif.trim().to_uppercase(),
        c.nombre.trim(),
        pais,
        c.expedido.to_string(),
        hasta.to_string(),
        c.convenio,
        c.documento.trim(),
        c.notas.trim(),
        now_str(now),
    ];
    let id = if c.id > 0 {
        let n = tx.execute(
            "UPDATE certificado_residencia SET nif = ?1, nombre = ?2, pais = ?3, expedido = ?4,
               valido_hasta = ?5, convenio = ?6, documento = ?7, notas = ?8 WHERE id = ?9",
            params![
                c.nif.trim().to_uppercase(),
                c.nombre.trim(),
                pais,
                c.expedido.to_string(),
                hasta.to_string(),
                c.convenio,
                c.documento.trim(),
                c.notas.trim(),
                c.id
            ],
        )?;
        if n == 0 {
            return Err(other("no existe ese certificado"));
        }
        c.id
    } else {
        tx.execute(
            "INSERT INTO certificado_residencia(nif, nombre, pais, expedido, valido_hasta,
               convenio, documento, notas, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            args,
        )?;
        tx.last_insert_rowid()
    };
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "irnr.certificado.save",
        &id.to_string(),
        &serde_json::json!({ "pais": pais, "valido_hasta": hasta.to_string() }),
    )?;
    tx.commit()?;
    Ok(id)
}

pub fn quitar_certificado(v: &Vault, id: i64, confirmed: bool, now: OffsetDateTime) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    if tx.execute("DELETE FROM certificado_residencia WHERE id = ?1", [id])? == 0 {
        return Err(other("no existe ese certificado"));
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "irnr.certificado.delete",
        &id.to_string(),
        &serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

/// Lo que teclea el usuario para apuntar una renta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NuevaRenta {
    pub fecha: Date,
    pub nif: String,
    pub nombre: String,
    pub pais: String,
    pub naturaleza: Naturaleza,
    pub bruto: Money,
    /// Tipo del convenio tecleado (si no, el de la tabla, si el país está).
    #[serde(default)]
    pub tipo_convenio: Option<Rate>,
    #[serde(default)]
    pub exencion: Option<Exencion>,
    #[serde(default)]
    pub concepto: String,
    #[serde(default)]
    pub recibida_id: Option<i64>,
}

/// Una renta apuntada.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RentaGuardada {
    pub id: i64,
    pub fecha: Date,
    pub nif: String,
    pub nombre: String,
    pub pais: String,
    pub naturaleza: Naturaleza,
    pub bruto: Money,
    pub tipo: Rate,
    pub retencion: Money,
    pub regimen: Regimen,
    pub exencion: Option<Exencion>,
    pub concepto: String,
    pub recibida_id: Option<i64>,
}

/// Tipo propuesto para la renta, con los avisos (sin guardar nada).
pub fn proponer(v: &Vault, r: &NuevaRenta) -> Result<Propuesta> {
    let cert = certificado_de(v.connection(), &r.nif, r.fecha)?;
    Ok(irnr::proponer(&Entrada {
        naturaleza: r.naturaleza,
        pais: r.pais.clone(),
        fecha: r.fecha,
        certificado: cert.map(|c| c.residencia()),
        tipo_convenio: r.tipo_convenio,
        exencion: r.exencion,
    }))
}

/// Apunta la renta con el tipo propuesto; devuelve su id y la propuesta (con los avisos).
pub fn apuntar(
    v: &Vault,
    r: &NuevaRenta,
    confirmed: bool,
    now: OffsetDateTime,
) -> Result<(i64, Propuesta)> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    if r.nif.trim().is_empty() || r.nombre.trim().is_empty() {
        return Err(other(
            "pon el identificador fiscal y el nombre del perceptor",
        ));
    }
    if r.bruto.cents() <= 0 {
        return Err(other("pon el importe íntegro de la renta"));
    }
    if r.naturaleza == Naturaleza::Dividendo {
        return Err(other(
            "los dividendos a socios no residentes salen del pago de dividendos (Socios): no \
             se apuntan aquí",
        ));
    }
    let pais = irnr::pais(&r.pais);
    if pais.len() != 2 || pais == "ES" {
        return Err(other(
            "el país del perceptor es el código de dos letras de su residencia (no ES)",
        ));
    }
    let p = proponer(v, r)?;
    let retencion = r.bruto.apply_rate(p.tipo);
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO renta_no_residente(fecha, nif, nombre, pais, naturaleza, bruto, tipo_bp,
           retencion, regimen, exencion, concepto, recibida_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            r.fecha.to_string(),
            r.nif.trim().to_uppercase(),
            r.nombre.trim(),
            pais,
            r.naturaleza.code(),
            r.bruto.cents(),
            p.tipo.bp(),
            retencion.cents(),
            p.regimen.code(),
            p.exencion.map(Exencion::code),
            r.concepto.trim(),
            r.recibida_id,
            now_str(now),
        ],
    )?;
    let id = tx.last_insert_rowid();
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "irnr.renta.add",
        &id.to_string(),
        &serde_json::json!({
            "naturaleza": r.naturaleza.code(),
            "bruto": r.bruto.cents(),
            "tipo_bp": p.tipo.bp(),
            "regimen": p.regimen.code(),
        }),
    )?;
    tx.commit()?;
    Ok((id, p))
}

pub fn quitar_renta(v: &Vault, id: i64, confirmed: bool, now: OffsetDateTime) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    if tx.execute("DELETE FROM renta_no_residente WHERE id = ?1", [id])? == 0 {
        return Err(other("no existe esa renta"));
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "irnr.renta.delete",
        &id.to_string(),
        &serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

fn year_range(year: i32) -> Result<(Date, Date)> {
    Ok((
        Date::from_calendar_date(year, Month::January, 1).map_err(other)?,
        Date::from_calendar_date(year, Month::December, 31).map_err(other)?,
    ))
}

fn quarter_range(year: i32, quarter: u8) -> Result<(Date, Date)> {
    if !(1..=4).contains(&quarter) {
        return Err(other(format!("trimestre {quarter} no válido")));
    }
    let first = Month::try_from(quarter * 3 - 2).map_err(other)?;
    let last = Month::try_from(quarter * 3).map_err(other)?;
    Ok((
        Date::from_calendar_date(year, first, 1).map_err(other)?,
        Date::from_calendar_date(year, last, last.length(year)).map_err(other)?,
    ))
}

/// Rentas apuntadas con devengo entre `start` y `end`.
pub fn rentas_apuntadas(v: &Vault, start: Date, end: Date) -> Result<Vec<RentaGuardada>> {
    let mut st = v.connection().prepare(
        "SELECT id, fecha, nif, nombre, pais, naturaleza, bruto, tipo_bp, retencion, regimen,
                exencion, concepto, recibida_id
         FROM renta_no_residente WHERE fecha BETWEEN ?1 AND ?2 ORDER BY fecha, id",
    )?;
    let rows = st.query_map(params![start.to_string(), end.to_string()], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, i64>(6)?,
            r.get::<_, i32>(7)?,
            r.get::<_, i64>(8)?,
            r.get::<_, String>(9)?,
            r.get::<_, Option<String>>(10)?,
            r.get::<_, String>(11)?,
            r.get::<_, Option<i64>>(12)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, fecha, nif, nombre, pais, nat, bruto, tipo, ret, reg, ex, concepto, recibida) =
            row?;
        out.push(RentaGuardada {
            id,
            fecha: parse_date(&fecha)?,
            nif,
            nombre,
            pais,
            naturaleza: Naturaleza::parse(&nat).unwrap_or(Naturaleza::Otros),
            bruto: Money::from_cents(bruto),
            tipo: Rate::from_bp(tipo),
            retencion: Money::from_cents(ret),
            regimen: Regimen::parse(&reg).unwrap_or(Regimen::General),
            exencion: ex.as_deref().and_then(Exencion::parse),
            concepto,
            recibida_id: recibida,
        });
    }
    Ok(out)
}

pub fn rentas_del_ano(v: &Vault, year: i32) -> Result<Vec<RentaGuardada>> {
    let (a, b) = year_range(year)?;
    rentas_apuntadas(v, a, b)
}

/// Dividendos a socios no residentes pagados entre `start` y `end` (de Socios), con avisos si
/// hay un certificado de residencia vigente que permitiría el convenio o la exención.
fn dividendos(v: &Vault, start: Date, end: Date, warnings: &mut Vec<String>) -> Result<Vec<Renta>> {
    let paises: BTreeMap<i64, String> = crate::socios::shareholders(v)?
        .into_iter()
        .map(|s| (s.id, s.country))
        .collect();
    let mut out = Vec::new();
    for p in crate::socios::payments_between(v, start, end)? {
        for l in p.lines.iter().filter(|l| !l.resident) {
            let pais = l
                .shareholder_id
                .and_then(|id| paises.get(&id).cloned())
                .unwrap_or_default();
            if let Some(c) = certificado_de(v.connection(), &l.nif, p.date)?
                && c.residencia().vigente(p.date)
                && !l.withheld.is_zero()
            {
                warnings.push(format!(
                    "{} ({}): el dividendo del {} se pagó con el {} y hay certificado de \
                     residencia vigente; con el convenio o la exención matriz-filial (art. \
                     14.1.h TRLIRNR) la retención podría ser menor. Revísalo con el pago de \
                     dividendos.",
                    l.name, l.nif, p.date, l.rate
                ));
            }
            out.push(Renta {
                fecha: p.date,
                nif: l.nif.clone(),
                nombre: l.name.clone(),
                pais,
                naturaleza: Naturaleza::Dividendo,
                bruto: l.gross,
                tipo: l.rate,
                retencion: l.withheld,
                regimen: if l.withheld.is_zero() {
                    Regimen::Exenta
                } else {
                    Regimen::General
                },
                exencion: None,
            });
        }
    }
    Ok(out)
}

/// Todas las rentas a no residentes con devengo entre `start` y `end`.
pub fn rentas(v: &Vault, start: Date, end: Date, warnings: &mut Vec<String>) -> Result<Vec<Renta>> {
    let mut out = dividendos(v, start, end, warnings)?;
    out.extend(rentas_apuntadas(v, start, end)?.into_iter().map(|r| Renta {
        fecha: r.fecha,
        nif: r.nif,
        nombre: r.nombre,
        pais: r.pais,
        naturaleza: r.naturaleza,
        bruto: r.bruto,
        tipo: r.tipo,
        retencion: r.retencion,
        regimen: r.regimen,
        exencion: r.exencion,
    }));
    out.sort_by(|a, b| a.fecha.cmp(&b.fecha).then_with(|| a.nif.cmp(&b.nif)));
    Ok(out)
}

fn file_name(v: &Vault, model: &str, year: i32, period: &str) -> Result<String> {
    let who = crate::models::declarant(v)?;
    Ok(format!("{}_{year}_{period}_{model}.txt", who.nif))
}

fn tercero(out: &mut Vec<String>, name: &str, id: &str) {
    for s in [name.trim(), id.trim()] {
        if !s.is_empty() && !out.iter().any(|x| x == s) {
            out.push(s.to_owned());
        }
    }
}

/// Borrador del 216 del trimestre.
pub fn draft_216(v: &Vault, year: i32, quarter: u8) -> Result<Draft> {
    let (start, end) = quarter_range(year, quarter)?;
    let mut warnings = Vec::new();
    let rs = rentas(v, start, end, &mut warnings)?;
    let period = format!("{quarter}T");
    let r = irnr::compute_216(year, &period, &rs);
    warnings.extend(r.warnings.iter().cloned());
    warnings.push(
        "TaxPain no tiene el diseño de registro del 216: no genera el fichero. Teclea estas \
         casillas en el formulario de la Sede. TODO(VERIFY): numeración de las casillas \
         (Orden EHA/3290/2008)."
            .into(),
    );
    let mut terceros = Vec::new();
    let details = rs
        .iter()
        .map(|x| {
            tercero(&mut terceros, &x.nombre, &x.nif);
            (
                format!(
                    "{} · {} · {} · {} · {} al {}",
                    x.fecha,
                    x.nombre,
                    x.nif,
                    x.pais,
                    x.naturaleza.label(),
                    x.tipo
                ),
                x.retencion,
            )
        })
        .collect();
    Ok(Draft {
        model: "216".into(),
        year,
        quarter,
        boxes: r.boxes(),
        payees: r.payees(),
        details,
        declaration: r.declaracion.clone(),
        result: r.resultado,
        warnings,
        file_name: file_name(v, "216", year, &period)?,
        file: String::new(),
        terceros,
    })
}

/// El 296 del año tal como lo declara el borrador.
pub fn result_296(v: &Vault, year: i32) -> Result<irnr::Result296> {
    let (a, b) = year_range(year)?;
    let mut w = Vec::new();
    let mut r = irnr::compute_296(year, &rentas(v, a, b, &mut w)?);
    r.warnings.extend(w);
    Ok(r)
}

/// Borrador del 296 del año, con el cuadre con los 216 del año.
pub fn draft_296(v: &Vault, year: i32) -> Result<Draft> {
    let r = result_296(v, year)?;
    let mut warnings = r.warnings.clone();
    let mut trimestres = Money::ZERO;
    for q in 1..=4 {
        trimestres += draft_216(v, year, q)?.result;
    }
    if trimestres != r.total_retencion {
        warnings.push(format!(
            "Las retenciones del 296 ({}) no cuadran con la suma de los 216 del año \
             ({trimestres}).",
            r.total_retencion
        ));
    }
    let mut terceros = Vec::new();
    let details = r
        .lines
        .iter()
        .map(|l| {
            tercero(&mut terceros, &l.nombre, &l.nif);
            (
                format!(
                    "{} · {} · {} · {} · {} {} · {} renta(s), íntegro {}",
                    l.nombre,
                    l.nif,
                    l.pais,
                    l.naturaleza.label(),
                    match l.regimen {
                        Regimen::General => "tipo general",
                        Regimen::Convenio => "convenio",
                        Regimen::Exenta => "exenta",
                    },
                    l.tipo,
                    l.rentas,
                    l.bruto
                ),
                l.retencion,
            )
        })
        .collect();
    Ok(Draft {
        model: "296".into(),
        year,
        quarter: 0,
        boxes: vec![],
        payees: vec![],
        details,
        declaration: String::new(),
        result: r.total_retencion,
        warnings,
        file_name: file_name(v, "296", year, "0A")?,
        file: String::new(),
        terceros,
    })
}

/// Factura recibida de un proveedor de otro país que puede ser una renta de no residente.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Candidata {
    pub recibida_id: i64,
    pub fecha: Date,
    pub nif: String,
    pub nombre: String,
    pub pais: String,
    pub numero: String,
    pub base: Money,
}

/// Recibidas del año de proveedores de otro país que todavía no están apuntadas como renta.
/// La mayoría (servicios con convenio, compras de bienes) no llevan retención: el usuario decide.
pub fn candidatas(v: &Vault, year: i32) -> Result<Vec<Candidata>> {
    let (a, b) = year_range(year)?;
    let mut st = v.connection().prepare(
        "SELECT r.id, COALESCE(r.issue_date, substr(r.reviewed_at, 1, 10)),
                COALESCE(r.supplier_nif, ''), COALESCE(r.supplier_name, ''),
                r.supplier_country, COALESCE(r.number, ''), COALESCE(r.base, 0)
         FROM invoice_received r
         WHERE r.status = 'revisada' AND r.supplier_country IS NOT NULL
           AND upper(r.supplier_country) <> 'ES'
           AND COALESCE(r.issue_date, substr(r.reviewed_at, 1, 10)) BETWEEN ?1 AND ?2
           AND NOT EXISTS (SELECT 1 FROM renta_no_residente n WHERE n.recibida_id = r.id)
         ORDER BY 2, r.id",
    )?;
    let rows = st.query_map(params![a.to_string(), b.to_string()], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, i64>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, fecha, nif, nombre, pais, numero, base) = row?;
        out.push(Candidata {
            recibida_id: id,
            fecha: parse_date(&fecha)?,
            nif,
            nombre,
            pais: irnr::pais(&pais),
            numero,
            base: Money::from_cents(base),
        });
    }
    Ok(out)
}

/// Lo que enseña la pantalla: certificados (con su estado), rentas apuntadas del año,
/// dividendos a no residentes, candidatas y la tabla de convenios.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Resumen {
    pub year: i32,
    pub certificados: Vec<EstadoCertificado>,
    pub rentas: Vec<RentaGuardada>,
    pub dividendos: Vec<Renta>,
    pub candidatas: Vec<Candidata>,
    pub convenios: Vec<irnr::Convenio>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EstadoCertificado {
    #[serde(flatten)]
    pub certificado: Certificado,
    pub valido_hasta: Date,
    pub vigente: bool,
    /// Caduca en los próximos 30 días.
    pub caduca_pronto: bool,
}

pub fn resumen(v: &Vault, year: i32, today: Date) -> Result<Resumen> {
    let (a, b) = year_range(year)?;
    let certificados = certificados(v)?
        .into_iter()
        .map(|c| {
            let r = c.residencia();
            EstadoCertificado {
                valido_hasta: r.valido_hasta,
                vigente: r.vigente(today),
                caduca_pronto: r.vigente(today)
                    && r.valido_hasta <= today + time::Duration::days(30),
                certificado: Certificado {
                    valido_hasta: Some(r.valido_hasta),
                    ..c
                },
            }
        })
        .collect();
    Ok(Resumen {
        year,
        certificados,
        rentas: rentas_apuntadas(v, a, b)?,
        dividendos: dividendos(v, a, b, &mut Vec::new())?,
        candidatas: candidatas(v, year)?,
        convenios: irnr::CONVENIOS.to_vec(),
    })
}

/// Certificados de residencia que caducan en los próximos 30 días o ya caducados de perceptores
/// con rentas este año (para el Panel o la pantalla).
pub fn avisos_certificados(v: &Vault, today: Date) -> Result<Vec<String>> {
    let (a, b) = year_range(today.year())?;
    let mut nifs: Vec<String> = rentas(v, a, b, &mut Vec::new())?
        .into_iter()
        .map(|r| r.nif.to_uppercase())
        .collect();
    nifs.sort();
    nifs.dedup();
    let mut out = Vec::new();
    for nif in nifs {
        if let Some(c) = certificado_de(v.connection(), &nif, today)? {
            let r = c.residencia();
            if !r.vigente(today) {
                out.push(format!(
                    "{} ({}): el certificado de residencia caducó el {}. Pide uno nuevo para \
                     seguir aplicando el convenio o la exención.",
                    c.nombre, c.nif, r.valido_hasta
                ));
            } else if r.valido_hasta <= today + time::Duration::days(30) {
                out.push(format!(
                    "{} ({}): el certificado de residencia caduca el {}.",
                    c.nombre, c.nif, r.valido_hasta
                ));
            }
        }
    }
    Ok(out)
}
