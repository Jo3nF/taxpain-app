//! Censo (modelo 036): lo que consta en el censo de la AEAT frente al perfil fiscal de TaxPain y
//! a lo que dice la contabilidad, con las diferencias que habría que comunicar, y el asistente
//! «Quiero comunicar…» ([`guia`]) con los apartados del 036, el plazo y el aviso en el
//! calendario. Nada se presenta: el 036 se rellena en la Sede.
//!
//! Lo que consta en el censo sale del último certificado de situación censal guardado en los
//! documentos de la empresa (sus obligaciones periódicas, sus epígrafes y su domicilio) o de lo
//! que el usuario teclea, que manda si lo hay (el certificado puede ser una foto, o antiguo).
//!
//! Lo que dice la contabilidad, de los dos últimos años naturales (el anterior y el actual):
//! - 111: nóminas con retención o facturas recibidas de profesionales con retención;
//! - 115: facturas recibidas del alquiler con retención (las marcadas como alquiler o, sin
//!   marcar, con una retención del 19 % de la base, como el borrador del 115);
//! - 123: dividendos pagados con retención;
//! - 349 y ROI: emitidas que van al 349 y recibidas intracomunitarias;
//! - epígrafe de alquiler (861): ingresos de la 752.
//!
//! y de los modelos presentados (los importados y los de «Comprobar lo presentado»).
//!
//! Norma: RGAT arts. 9 a 14 (declaraciones censales) y arts. 3.3 y 25 (ROI); LIS art. 40.3.

pub mod guia;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use taxpain_core::calendar::{IrpfMethod, Persona, TaxpayerProfile};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::params;
use time::{Date, Month, OffsetDateTime};

use crate::documentos::{DocKind, fold};
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

const CLAVE: &str = "censo_036";
const PERFIL: &str = "taxpayer_profile";

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

/// Lo que consta en el censo según el usuario (lo que ve en «Mis datos censales» o en el
/// certificado).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CensoTecleado {
    /// Fecha de los datos (la del certificado o la de la consulta).
    #[serde(default)]
    pub fecha: Option<Date>,
    /// Obligaciones periódicas, por su modelo: 303, 111, 115, 123, 349, 130, 200.
    #[serde(default)]
    pub obligaciones: Vec<String>,
    /// Epígrafes del IAE que constan («843.9», «861.2»).
    #[serde(default)]
    pub epigrafes: Vec<String>,
    /// Alta en el ROI (`None`: no se sabe).
    #[serde(default)]
    pub roi: Option<bool>,
    #[serde(default)]
    pub sii: Option<bool>,
    #[serde(default)]
    pub criterio_caja: Option<bool>,
    #[serde(default)]
    pub domicilio_fiscal: Option<String>,
}

/// Un cambio apuntado en el calendario para comunicarlo con el 036.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comunicacion {
    pub id: i64,
    pub supuesto: String,
    pub titulo: String,
    pub fecha_hecho: Option<Date>,
    pub vence: Date,
    #[serde(default)]
    pub nota: String,
    pub creada: String,
    /// Cuándo se marcó como presentada (`None`: pendiente).
    #[serde(default)]
    pub hecha: Option<String>,
}

/// Lo que se guarda del censo (en los ajustes de la empresa).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guardado {
    #[serde(default)]
    pub tecleado: Option<CensoTecleado>,
    /// Epígrafes de lo que la empresa hace de verdad (para cotejar los del censo).
    #[serde(default)]
    pub epigrafes_actividad: Vec<String>,
    #[serde(default)]
    pub comunicaciones: Vec<Comunicacion>,
    #[serde(default)]
    pub siguiente: i64,
}

fn guardado(v: &Vault) -> Result<Guardado> {
    Ok(v.setting::<Guardado>(CLAVE)?.unwrap_or_default())
}

fn escribir(
    v: &Vault,
    g: &Guardado,
    accion: &str,
    sujeto: &str,
    detalle: serde_json::Value,
    now: OffsetDateTime,
) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let valor = serde_json::to_string(g).map_err(|e| other(e.to_string()))?;
    tx.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![CLAVE, valor],
    )?;
    audit::append(&tx, &now_str(now), Actor::User, accion, sujeto, &detalle)?;
    tx.commit()?;
    Ok(())
}

// ---------- Lo que consta en el censo ----------

/// De dónde sale lo que consta en el censo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origen {
    Tecleado,
    Certificado,
    Ninguno,
}

/// Lo que consta en el censo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Situacion {
    pub origen: Origen,
    /// Título del certificado del que sale.
    pub documento: Option<String>,
    pub fecha: Option<Date>,
    /// Obligaciones por su modelo (303, 111, 115, 123, 349, 130, 200).
    pub obligaciones: BTreeSet<String>,
    /// Las líneas de las obligaciones como las escribe el certificado.
    pub lineas: Vec<String>,
    pub epigrafes: Vec<String>,
    pub roi: Option<bool>,
    pub sii: Option<bool>,
    pub criterio_caja: Option<bool>,
    pub domicilio_fiscal: Option<String>,
}

impl Situacion {
    fn ninguna() -> Self {
        Situacion {
            origen: Origen::Ninguno,
            documento: None,
            fecha: None,
            obligaciones: BTreeSet::new(),
            lineas: vec![],
            epigrafes: vec![],
            roi: None,
            sii: None,
            criterio_caja: None,
            domicilio_fiscal: None,
        }
    }

    /// Se sabe qué obligaciones constan.
    fn conocida(&self) -> bool {
        match self.origen {
            Origen::Tecleado => true,
            Origen::Certificado => !self.lineas.is_empty(),
            Origen::Ninguno => false,
        }
    }
}

/// Un epígrafe del IAE normalizado: cifras y punto («843.9»).
fn epigrafe(s: &str) -> Option<String> {
    let e: String = s
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let e = e.trim_matches('.').to_owned();
    (e.len() >= 3 && e.chars().next().is_some_and(|c| c.is_ascii_digit())).then_some(e)
}

/// Normaliza una obligación tecleada a su modelo («111», «Modelo 115» → «115»; «390» → «303»).
fn obligacion(s: &str) -> Option<String> {
    let d: String = s.chars().filter(char::is_ascii_digit).collect();
    let m = match d.as_str() {
        "303" | "390" => "303",
        "111" | "190" => "111",
        "115" | "180" => "115",
        "123" | "193" => "123",
        "349" => "349",
        "130" | "131" => "130",
        "200" | "202" => "200",
        _ => return None,
    };
    Some(m.to_owned())
}

/// Lo que se lee de un certificado de situación censal (su texto, con los saltos de línea).
/// `TODO(VERIFY)`: el formato de los epígrafes y del ROI se ha supuesto por cómo los escribe la
/// AEAT en «Mis datos censales»; las obligaciones, como las lee la revisión.
pub fn situacion_de_texto(texto: &str, fecha: Option<Date>, documento: &str) -> Situacion {
    static EPIGRAFE: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"(?i)ep[ií]grafe[^0-9\n]{0,40}(\d{3}(?:\.\d{1,2})?)").ok());
    let o = crate::oficiales::leer(texto);
    let obligaciones = o
        .modelos_obligados()
        .into_iter()
        .filter_map(obligacion)
        .collect();
    let mut epigrafes: Vec<String> = EPIGRAFE
        .as_ref()
        .map(|r| {
            r.captures_iter(texto)
                .filter_map(|c| epigrafe(&c[1]))
                .collect()
        })
        .unwrap_or_default();
    epigrafes.dedup();
    let mut vistos = BTreeSet::new();
    epigrafes.retain(|e| vistos.insert(e.clone()));
    // ROI: la línea del registro de operadores intracomunitarios dice si está de alta.
    let roi = texto
        .lines()
        .map(fold)
        .find(|l| l.contains("operadores intracomunitarios"))
        .and_then(|l| {
            if l.contains("baja") || l.contains(": no") || l.ends_with(" no") {
                Some(false)
            } else if l.contains("alta") || l.contains(": si") || l.ends_with(" si") {
                Some(true)
            } else {
                None
            }
        });
    Situacion {
        origen: Origen::Certificado,
        documento: Some(documento.to_owned()),
        fecha: o.fecha.or(fecha),
        obligaciones,
        lineas: o.obligaciones,
        epigrafes,
        roi,
        sii: None,
        criterio_caja: None,
        domicilio_fiscal: None,
    }
}

fn de_tecleado(t: &CensoTecleado) -> Situacion {
    let mut ep = BTreeSet::new();
    Situacion {
        origen: Origen::Tecleado,
        documento: None,
        fecha: t.fecha,
        obligaciones: t
            .obligaciones
            .iter()
            .filter_map(|o| obligacion(o))
            .collect(),
        lineas: vec![],
        epigrafes: t
            .epigrafes
            .iter()
            .filter_map(|e| epigrafe(e))
            .filter(|e| ep.insert(e.clone()))
            .collect(),
        roi: t.roi,
        sii: t.sii,
        criterio_caja: t.criterio_caja,
        domicilio_fiscal: t.domicilio_fiscal.clone().filter(|d| !d.trim().is_empty()),
    }
}

/// Lo que consta en el censo: lo tecleado o, si no, el último certificado de situación censal.
pub fn situacion(v: &Vault) -> Result<Situacion> {
    let g = guardado(v)?;
    if let Some(t) = &g.tecleado {
        return Ok(de_tecleado(t));
    }
    let docs = crate::documentos::list(v)?;
    let certificado = docs
        .iter()
        .filter(|d| d.kind == DocKind::CertificadoSituacionCensal)
        .filter_map(|d| {
            let (_, _, bytes) = crate::documentos::content(v, d.id).ok()?;
            let texto = if bytes.starts_with(b"%PDF") {
                crate::pdf_text::text(&bytes).unwrap_or_default()
            } else {
                String::from_utf8(bytes).unwrap_or_default()
            };
            let mut s = situacion_de_texto(&texto, d.date, &d.title);
            s.domicilio_fiscal = d.facts.street.as_ref().map(|c| {
                [
                    Some(c.clone()),
                    d.facts.post_code.clone(),
                    d.facts.town.clone(),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(", ")
            });
            if s.epigrafes.is_empty()
                && let Some(e) = d.facts.iae.as_deref().and_then(epigrafe)
            {
                s.epigrafes.push(e);
            }
            Some(s)
        })
        .max_by_key(|s| s.fecha);
    let Some(mut s) = certificado else {
        return Ok(Situacion::ninguna());
    };
    // El justificante del alta en el ROI dice que está de alta si el certificado no lo dice.
    if s.roi.is_none() && docs.iter().any(|d| d.kind == DocKind::AltaRoi) {
        s.roi = Some(true);
    }
    Ok(s)
}

// ---------- Lo que dicen la contabilidad y lo presentado ----------

/// Cuántos y cuánto.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Cuenta {
    pub n: usize,
    pub importe: Money,
}

impl Cuenta {
    fn sumar(&mut self, m: Money) {
        self.n += 1;
        self.importe += m;
    }
}

/// Lo que TaxPain ve que pasa en la empresa entre `desde` y `hasta`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Hechos {
    pub desde: Option<Date>,
    pub hasta: Option<Date>,
    /// Nóminas con retención del IRPF (retención).
    pub nominas: Cuenta,
    /// Facturas recibidas de profesionales con retención (retención).
    pub profesionales: Cuenta,
    /// Facturas recibidas del alquiler con retención (retención).
    pub alquileres: Cuenta,
    /// Dividendos pagados con retención (retención).
    pub dividendos: Cuenta,
    /// Emitidas que van al 349.
    pub intracom_emitidas: usize,
    /// Recibidas intracomunitarias.
    pub intracom_recibidas: usize,
    /// Ingresos de la 752 (arrendamientos).
    pub ingresos_alquiler: Money,
    /// Obligación → ejercicios con algún modelo presentado.
    pub presentados: BTreeMap<String, BTreeSet<i32>>,
}

fn sql_fecha(d: Date) -> String {
    d.to_string()
}

/// Lee lo que pasa en la empresa entre `desde` y `hasta`.
pub fn hechos(v: &Vault, desde: Date, hasta: Date) -> Result<Hechos> {
    let conn = v.connection();
    let (a, b) = (sql_fecha(desde), sql_fecha(hasta));
    let mut h = Hechos {
        desde: Some(desde),
        hasta: Some(hasta),
        ..Hechos::default()
    };
    let mut st =
        conn.prepare("SELECT irpf FROM payslip WHERE irpf > 0 AND date BETWEEN ?1 AND ?2")?;
    for r in st.query_map(params![a, b], |r| r.get::<_, i64>(0))? {
        h.nominas.sumar(Money::from_cents(r?));
    }
    let mut st = conn.prepare(
        "SELECT COALESCE(base, 0), withholding, nature FROM invoice_received
         WHERE status = 'revisada' AND withholding > 0 AND issue_date BETWEEN ?1 AND ?2",
    )?;
    for r in st.query_map(params![a, b], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
        ))
    })? {
        let (base, ret, nature) = r?;
        let (base, ret) = (Money::from_cents(base), Money::from_cents(ret));
        // Como el borrador del 115: lo marcado o, sin marcar, una retención del 19 %.
        let alquiler = match nature.as_str() {
            "alquiler" => true,
            "profesional" => false,
            _ => {
                base.apply_rate(Rate::from_bp(1900))
                    .cents()
                    .abs_diff(ret.cents())
                    <= 1
            }
        };
        if alquiler {
            h.alquileres.sumar(ret);
        } else {
            h.profesionales.sumar(ret);
        }
    }
    let mut st = conn.prepare(
        "SELECT withheld FROM dividend_payment WHERE withheld > 0 AND date BETWEEN ?1 AND ?2",
    )?;
    for r in st.query_map(params![a, b], |r| r.get::<_, i64>(0))? {
        h.dividendos.sumar(Money::from_cents(r?));
    }
    h.intracom_recibidas = conn.query_row(
        "SELECT COUNT(*) FROM invoice_received WHERE status = 'revisada'
           AND vat_regime = 'intracomunitaria' AND issue_date BETWEEN ?1 AND ?2",
        params![a, b],
        |r| r.get::<_, i64>(0),
    )? as usize;
    h.intracom_emitidas = crate::models::intracomunitarias_emitidas(v, desde, hasta)?;
    h.ingresos_alquiler = Money::from_cents(conn.query_row(
        "SELECT COALESCE(SUM(l.credit - l.debit), 0) FROM journal_line l
           JOIN journal_entry e ON e.id = l.entry_id
         WHERE l.account LIKE '752%' AND e.date BETWEEN ?1 AND ?2
           AND e.origin_kind NOT IN ('Regularizacion', 'Cierre')",
        params![a, b],
        |r| r.get::<_, i64>(0),
    )?);
    for (m, y, _) in crate::perfil_deducido::datos(v)?.vistas {
        if let Some(o) = obligacion(&m)
            && y >= desde.year()
            && y <= hasta.year()
        {
            h.presentados.entry(o).or_default().insert(y);
        }
    }
    Ok(h)
}

// ---------- El cotejo ----------

/// Las obligaciones que se cotejan, con su nombre.
pub const OBLIGACIONES: &[(&str, &str)] = &[
    ("303", "IVA (303 y 390)"),
    ("111", "Retenciones de trabajo y profesionales (111 y 190)"),
    ("115", "Retenciones de alquileres (115 y 180)"),
    ("123", "Retenciones del capital mobiliario (123 y 193)"),
    ("349", "Operaciones intracomunitarias (349)"),
    ("130", "Pagos fraccionados del IRPF (130 o 131)"),
    ("200", "Impuesto sobre Sociedades (200 y 202)"),
];

/// Una fila del cuadro: una obligación en el censo, en el perfil, en la contabilidad y en lo
/// presentado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Fila {
    pub codigo: String,
    pub nombre: String,
    /// `None`: no se sabe qué consta.
    pub censo: Option<bool>,
    pub perfil: Option<bool>,
    /// Lo que la contabilidad ve («3 nóminas con retención, 450,00 €»), si algo.
    pub contabilidad: Option<String>,
    pub presentado: Vec<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Gravedad {
    Alta,
    Media,
    Info,
}

/// Una diferencia y qué hacer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Diferencia {
    pub clave: String,
    pub gravedad: Gravedad,
    pub titulo: String,
    pub detalle: String,
    pub que_hacer: String,
    /// Supuesto del asistente que lo resuelve (si va por el 036).
    pub supuesto: Option<String>,
}

/// Lo que se coteja, sin la base de datos.
#[derive(Clone, Debug, Default)]
pub struct Entrada {
    pub situacion: Option<Situacion>,
    pub perfil: Option<TaxpayerProfile>,
    pub hechos: Hechos,
    pub epigrafes_actividad: Vec<String>,
    /// Domicilio de las facturas (Ajustes → Facturación).
    pub domicilio_facturas: Option<String>,
    /// Criterio de caja marcado en los datos de los modelos.
    pub criterio_caja: bool,
    /// Ejercicio para el que está elegida la base del 202 sin ser obligatoria.
    pub base_202: Option<i32>,
}

fn contabilidad(o: &str, h: &Hechos) -> Option<String> {
    let c = |n: usize, que: &str, m: Money| format!("{n} {que} ({m} de retención)");
    let partes: Vec<String> = match o {
        "111" => [
            (h.nominas.n > 0).then(|| c(h.nominas.n, "nóminas con retención", h.nominas.importe)),
            (h.profesionales.n > 0).then(|| {
                c(
                    h.profesionales.n,
                    "facturas de profesionales con retención",
                    h.profesionales.importe,
                )
            }),
        ]
        .into_iter()
        .flatten()
        .collect(),
        "115" => (h.alquileres.n > 0)
            .then(|| {
                c(
                    h.alquileres.n,
                    "facturas del alquiler con retención",
                    h.alquileres.importe,
                )
            })
            .into_iter()
            .collect(),
        "123" => (h.dividendos.n > 0)
            .then(|| {
                c(
                    h.dividendos.n,
                    "pagos de dividendos con retención",
                    h.dividendos.importe,
                )
            })
            .into_iter()
            .collect(),
        "349" => [
            (h.intracom_emitidas > 0)
                .then(|| format!("{} facturas emitidas que van al 349", h.intracom_emitidas)),
            (h.intracom_recibidas > 0).then(|| {
                format!(
                    "{} facturas recibidas intracomunitarias",
                    h.intracom_recibidas
                )
            }),
        ]
        .into_iter()
        .flatten()
        .collect(),
        _ => vec![],
    };
    (!partes.is_empty()).then(|| partes.join("; "))
}

fn en_perfil(o: &str, p: &TaxpayerProfile) -> bool {
    match o {
        "303" => p.iva.is_some(),
        "111" => p.retiene_trabajo_profesionales,
        "115" => p.retiene_alquileres,
        "123" => p.retiene_capital_mobiliario,
        "349" => p.intracomunitarias.is_some(),
        "130" => {
            p.persona == Persona::Fisica
                && p.irpf_method.is_some()
                && !(p.exento_pagos_fraccionados
                    && p.irpf_method == Some(IrpfMethod::EstimacionDirecta))
        }
        "200" => p.persona == Persona::Juridica,
        _ => false,
    }
}

fn años(s: &BTreeSet<i32>) -> String {
    s.iter().map(i32::to_string).collect::<Vec<_>>().join(", ")
}

/// Código postal de un domicilio (cinco cifras seguidas).
fn codigo_postal(s: &str) -> Option<String> {
    static CP: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\b(\d{5})\b").ok());
    CP.as_ref()?.captures(s).map(|c| c[1].to_owned())
}

/// El cuadro y las diferencias.
pub fn cotejar(e: &Entrada) -> (Vec<Fila>, Vec<Diferencia>) {
    let s = e.situacion.clone().unwrap_or_else(Situacion::ninguna);
    let conocida = s.conocida();
    let h = &e.hechos;
    let mut filas = Vec::new();
    let mut dif = Vec::new();
    let fuente = match s.origen {
        Origen::Tecleado => "lo que has tecleado del censo".to_owned(),
        _ => match s.fecha {
            Some(f) => format!(
                "el certificado de situación censal del {}",
                taxpain_core::calendar::reminders::format_es(f)
            ),
            None => "el certificado de situación censal".to_owned(),
        },
    };
    let persona = e.perfil.as_ref().map(|p| p.persona);
    for (o, n) in OBLIGACIONES {
        // El IRPF no es de sociedades ni el IS de personas físicas.
        if (*o == "130" && persona == Some(Persona::Juridica))
            || (*o == "200" && persona == Some(Persona::Fisica))
        {
            continue;
        }
        let censo = conocida.then(|| s.obligaciones.contains(*o));
        let perfil = e.perfil.as_ref().map(|p| en_perfil(o, p));
        let conta = contabilidad(o, h);
        let pres = h.presentados.get(*o).cloned().unwrap_or_default();
        filas.push(Fila {
            codigo: (*o).into(),
            nombre: (*n).into(),
            censo,
            perfil,
            contabilidad: conta.clone(),
            presentado: pres.iter().copied().collect(),
        });
        let uso = conta.is_some() || !pres.is_empty();
        let mut pruebas = Vec::new();
        if let Some(c) = &conta {
            pruebas.push(format!("la contabilidad tiene {c}"));
        }
        if !pres.is_empty() {
            pruebas.push(format!("hay {o} presentados de {}", años(&pres)));
        }
        let pruebas = pruebas.join(" y ");
        let modelo = if *o == "303" { "IVA" } else { o };
        let supuesto_alta = ["111", "115", "123"]
            .contains(o)
            .then(|| format!("alta-{o}"))
            .or_else(|| (*o == "349").then(|| "alta-roi".to_owned()));
        let supuesto_baja = ["111", "115", "123"]
            .contains(o)
            .then(|| format!("baja-{o}"));
        match (censo, uso, perfil) {
            (Some(false), true, _) => dif.push(Diferencia {
                clave: format!("{o}-sin-alta"),
                gravedad: Gravedad::Alta,
                titulo: format!("{n}: no consta en el censo y {pruebas}"),
                detalle: format!(
                    "Según {fuente}, la empresa no tiene la obligación del {modelo}. Si \
                     retienes o declaras sin la obligación dada de alta, la AEAT acaba pidiendo \
                     una rectificación censal."
                ),
                que_hacer: format!(
                    "Comunica en el 036 el alta de la obligación del {modelo} con la fecha de \
                     la primera operación."
                ),
                supuesto: supuesto_alta,
            }),
            (Some(false), false, Some(true)) => dif.push(Diferencia {
                clave: format!("{o}-solo-perfil"),
                gravedad: Gravedad::Media,
                titulo: format!("{n}: el perfil fiscal lo tiene y el censo no"),
                detalle: format!(
                    "El calendario te avisa del {modelo} porque está en el perfil, pero según \
                     {fuente} no consta, y TaxPain no ve operaciones."
                ),
                que_hacer:
                    "Si de verdad lo tienes, comunica el alta en el 036; si no, quítalo del \
                     perfil fiscal (Calendario → Editar perfil)."
                        .into(),
                supuesto: supuesto_alta,
            }),
            (Some(true), false, Some(false) | None) if *o != "303" && *o != "200" => {
                dif.push(Diferencia {
                    clave: format!("{o}-sin-uso"),
                    gravedad: Gravedad::Media,
                    titulo: format!("{n}: consta en el censo y no hay operaciones"),
                    detalle: format!(
                        "Según {fuente} tienes la obligación del {modelo}, pero ni la \
                         contabilidad ni lo presentado de los dos últimos años dicen que la \
                         uses. Mientras conste, la AEAT espera el modelo cada periodo."
                    ),
                    que_hacer: match o {
                        &"349" => "Si ya no haces operaciones intracomunitarias, puedes pedir la \
                                   baja en el ROI con el 036."
                            .into(),
                        _ => format!(
                            "Si ya no pagas esas rentas, comunica la baja de la obligación del \
                             {modelo} en el 036."
                        ),
                    },
                    supuesto: supuesto_baja.or_else(|| (*o == "349").then(|| "baja-roi".into())),
                });
            }
            _ => {}
        }
        // El perfil, frente a lo que se ve (con o sin censo): no es del 036, pero el calendario
        // no avisa.
        if perfil == Some(false) && uso {
            dif.push(Diferencia {
                clave: format!("{o}-perfil"),
                gravedad: Gravedad::Info,
                titulo: format!("{n}: falta en el perfil fiscal"),
                detalle: format!("{} y el perfil fiscal no lo tiene.", capital(&pruebas)),
                que_hacer: "Corrígelo en Calendario → Editar perfil, para que el calendario \
                            avise de sus plazos."
                    .into(),
                supuesto: None,
            });
        }
    }
    // ROI.
    let intracom = h.intracom_emitidas + h.intracom_recibidas;
    match s.roi {
        Some(false) if intracom > 0 => dif.push(Diferencia {
            clave: "roi-sin-alta".into(),
            gravedad: Gravedad::Alta,
            titulo: "Operaciones intracomunitarias sin alta en el ROI".into(),
            detalle: format!(
                "Según {fuente} no estás en el Registro de operadores intracomunitarios, y hay \
                 {intracom} facturas intracomunitarias. Sin ROI, tu NIF-IVA no es válido en el \
                 VIES."
            ),
            que_hacer: "Pide el alta en el ROI con el 036 cuanto antes.".into(),
            supuesto: Some("alta-roi".into()),
        }),
        Some(true) if intracom == 0 && !s.obligaciones.contains("349") => {
            dif.push(Diferencia {
                clave: "roi-sin-uso".into(),
                gravedad: Gravedad::Info,
                titulo: "Alta en el ROI sin operaciones intracomunitarias".into(),
                detalle: "Constas en el ROI pero TaxPain no ve operaciones intracomunitarias en \
                          los dos últimos años: la AEAT puede darte de baja de oficio."
                    .into(),
                que_hacer: "Si no vas a tener clientes ni proveedores de la UE, puedes pedir la \
                            baja; si los vas a tener, mantente."
                    .into(),
                supuesto: Some("baja-roi".into()),
            });
        }
        None if intracom > 0 && !conocida => dif.push(Diferencia {
            clave: "roi-sin-saber".into(),
            gravedad: Gravedad::Media,
            titulo: "Operaciones intracomunitarias: ¿estás en el ROI?".into(),
            detalle: format!(
                "Hay {intracom} facturas intracomunitarias y TaxPain no sabe si constas en el \
                 ROI."
            ),
            que_hacer: "Compruébalo en la Sede (Mis datos censales) o en el VIES con ES + tu \
                        NIF, y teclea lo que consta."
                .into(),
            supuesto: Some("alta-roi".into()),
        }),
        _ => {}
    }
    // Epígrafes.
    let actividad: BTreeSet<String> = e
        .epigrafes_actividad
        .iter()
        .filter_map(|x| epigrafe(x))
        .collect();
    let censo_ep: BTreeSet<String> = s.epigrafes.iter().cloned().collect();
    if !actividad.is_empty() && !censo_ep.is_empty() {
        for x in censo_ep.difference(&actividad) {
            dif.push(Diferencia {
                clave: format!("epigrafe-sobra-{x}"),
                gravedad: Gravedad::Media,
                titulo: format!("Consta el epígrafe {x} y no es de lo que haces"),
                detalle: format!(
                    "Según {fuente} estás de alta en el epígrafe {x}, que no está entre las \
                     actividades que has dicho que haces."
                ),
                que_hacer: "Si ya no haces esa actividad (o nunca la hiciste), comunica la baja \
                            del epígrafe en el 036."
                    .into(),
                supuesto: Some("baja-epigrafe".into()),
            });
        }
        for x in actividad.difference(&censo_ep) {
            dif.push(Diferencia {
                clave: format!("epigrafe-falta-{x}"),
                gravedad: Gravedad::Alta,
                titulo: format!("Haces la actividad del epígrafe {x} y no consta"),
                detalle: format!("Según {fuente} no estás de alta en el epígrafe {x}."),
                que_hacer: "Comunica el alta del epígrafe en el 036.".into(),
                supuesto: Some("alta-epigrafe".into()),
            });
        }
    }
    if h.ingresos_alquiler > Money::ZERO
        && !censo_ep.is_empty()
        && !censo_ep.iter().any(|x| x.starts_with("861"))
        && !actividad.iter().any(|x| x.starts_with("861"))
    {
        dif.push(Diferencia {
            clave: "epigrafe-alquiler".into(),
            gravedad: Gravedad::Media,
            titulo: "Cobras alquileres y no consta el epígrafe del alquiler (861)".into(),
            detalle: format!(
                "La cuenta 752 (ingresos por arrendamientos) suma {} en los dos últimos años y \
                 en el censo no hay ningún epígrafe 861 (alquiler de viviendas o de locales).",
                h.ingresos_alquiler
            ),
            que_hacer: "Si el alquiler es una actividad de la empresa, comunica el alta del \
                        epígrafe 861 que corresponda en el 036."
                .into(),
            supuesto: Some("alta-epigrafe".into()),
        });
    }
    // Domicilio fiscal frente al de las facturas.
    if let (Some(c), Some(f)) = (&s.domicilio_fiscal, &e.domicilio_facturas) {
        let distinto = match (codigo_postal(c), codigo_postal(f)) {
            (Some(a), Some(b)) => a != b,
            _ => {
                let (a, b) = (fold(c), fold(f));
                !a.contains(&b) && !b.contains(&a)
            }
        };
        if distinto {
            dif.push(Diferencia {
                clave: "domicilio".into(),
                gravedad: Gravedad::Media,
                titulo: "El domicilio fiscal del censo no es el de tus facturas".into(),
                detalle: format!("En el censo: {c}. En las facturas: {f}."),
                que_hacer: "Si te has mudado, comunica el cambio de domicilio fiscal en el 036 \
                            (un mes de plazo); si el bueno es el del censo, corrige Ajustes → \
                            Facturación."
                    .into(),
                supuesto: Some("domicilio-fiscal".into()),
            });
        }
    }
    // SII y criterio de caja (sólo si se tecleó lo que consta).
    if let (Some(c), Some(p)) = (s.sii, e.perfil.as_ref().and_then(|p| p.iva))
        && c != p.sii()
    {
        dif.push(Diferencia {
            clave: "sii".into(),
            gravedad: Gravedad::Media,
            titulo: if c {
                "Consta el SII y el perfil fiscal no lo tiene".into()
            } else {
                "El perfil fiscal lleva el SII y en el censo no consta".into()
            },
            detalle: "El SII cambia los plazos del IVA (303 hasta el día 30, sin 390 ni \
                          347)."
                .into(),
            que_hacer: if c {
                "Corrige el perfil fiscal; si quieres dejar el SII voluntario, la renuncia \
                     va en noviembre."
                    .into()
            } else {
                "Si llevas el SII voluntario, comunica la opción en el 036; si no, corrige \
                     el perfil."
                    .into()
            },
            supuesto: None,
        });
    }
    if let Some(c) = s.criterio_caja
        && c != e.criterio_caja
    {
        dif.push(Diferencia {
            clave: "criterio-caja".into(),
            gravedad: Gravedad::Alta,
            titulo: if c {
                "Consta el criterio de caja y TaxPain no lo aplica".into()
            } else {
                "TaxPain aplica el criterio de caja y en el censo no consta".into()
            },
            detalle: "El criterio de caja cambia cuándo se declara el IVA de cada factura: \
                      tiene que coincidir con el censo."
                .into(),
            que_hacer: if c {
                "Márcalo en los datos de los modelos o, si ya no lo quieres, renuncia en \
                 diciembre."
                    .into()
            } else {
                "Opta por él en el 036 en diciembre (para el año siguiente) o desmárcalo en los \
                 datos de los modelos."
                    .into()
            },
            supuesto: Some(if c {
                "criterio-caja-renuncia".into()
            } else {
                "criterio-caja-opcion".into()
            }),
        });
    }
    if let Some(y) = e.base_202 {
        dif.push(Diferencia {
            clave: "202-base".into(),
            gravedad: Gravedad::Info,
            titulo: format!("El 202 de {y} por la base: falta comunicarlo"),
            detalle: format!(
                "En Modelos → 202 tienes elegida la base del periodo para {y}. Sin la opción en \
                 el 036 (en febrero, en un ejercicio natural), vale la modalidad de la cuota."
            ),
            que_hacer: "Si ya la comunicaste, no hace falta nada; si no, preséntala en su plazo."
                .into(),
            supuesto: Some("opcion-202-base".into()),
        });
    }
    if !conocida {
        dif.push(Diferencia {
            clave: "sin-censo".into(),
            gravedad: Gravedad::Info,
            titulo: "Sin datos del censo no se puede cotejar".into(),
            detalle: "No hay ningún certificado de situación censal entre los documentos de la \
                      empresa (o no se lee) ni lo has tecleado."
                .into(),
            que_hacer: "Descarga el certificado de la Sede y guárdalo en Importar, o teclea lo \
                        que consta en «Mis datos censales»."
                .into(),
            supuesto: None,
        });
    }
    dif.sort_by_key(|d| d.gravedad);
    (filas, dif)
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// Lo que enseña la pantalla.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Cotejo {
    pub hoy: Date,
    pub situacion: Situacion,
    /// Lo tecleado (para el formulario).
    pub tecleado: Option<CensoTecleado>,
    pub epigrafes_actividad: Vec<String>,
    pub hechos: Hechos,
    pub filas: Vec<Fila>,
    pub diferencias: Vec<Diferencia>,
    pub comunicaciones: Vec<Comunicacion>,
    pub supuestos: Vec<guia::Supuesto>,
}

/// El cotejo de la empresa a `hoy` (los dos últimos años naturales).
pub fn cotejo(v: &Vault, hoy: Date) -> Result<Cotejo> {
    let g = guardado(v)?;
    let situacion = situacion(v)?;
    let desde = Date::from_calendar_date(hoy.year() - 1, Month::January, 1)
        .map_err(|e| other(e.to_string()))?;
    let hechos = hechos(v, desde, hoy)?;
    let perfil: Option<TaxpayerProfile> = v.setting(PERFIL)?;
    let domicilio_facturas = crate::invoicing::settings(v)?.and_then(|s| {
        s.issuer
            .address
            .map(|a| format!("{}, {} {}", a.street, a.post_code, a.town))
    });
    let ms = crate::models::settings(v)?;
    let base_202 = [hoy.year(), hoy.year() + 1].into_iter().find(|y| {
        crate::models::pago202::modalidad(v, *y).is_ok_and(|m| {
            !m.obligatoria && m.elegida == Some(crate::models::pago202::Modalidad::Base)
        })
    });
    let (filas, diferencias) = cotejar(&Entrada {
        situacion: Some(situacion.clone()),
        perfil,
        hechos: hechos.clone(),
        epigrafes_actividad: g.epigrafes_actividad.clone(),
        domicilio_facturas,
        criterio_caja: ms.cash_basis,
        base_202,
    });
    Ok(Cotejo {
        hoy,
        situacion,
        tecleado: g.tecleado,
        epigrafes_actividad: g.epigrafes_actividad,
        hechos,
        filas,
        diferencias,
        comunicaciones: g.comunicaciones,
        supuestos: guia::supuestos().to_vec(),
    })
}

/// Guarda lo tecleado del censo (`None`: vuelve al certificado) y los epígrafes de la actividad.
pub fn guardar(
    v: &Vault,
    tecleado: Option<CensoTecleado>,
    epigrafes_actividad: Vec<String>,
    now: OffsetDateTime,
) -> Result<()> {
    let mut g = guardado(v)?;
    let tecleado = tecleado.map(|mut t| {
        t.obligaciones = t
            .obligaciones
            .iter()
            .filter_map(|o| obligacion(o))
            .collect();
        t.obligaciones.sort();
        t.obligaciones.dedup();
        t.epigrafes = t.epigrafes.iter().filter_map(|e| epigrafe(e)).collect();
        t
    });
    let actividad: Vec<String> = epigrafes_actividad
        .iter()
        .filter_map(|e| epigrafe(e))
        .collect();
    if epigrafes_actividad.iter().any(|e| !e.trim().is_empty()) && actividad.is_empty() {
        return Err(other("los epígrafes del IAE son cifras, como 843.9"));
    }
    let detalle = serde_json::json!({
        "tecleado": tecleado.is_some(),
        "obligaciones": tecleado.as_ref().map(|t| t.obligaciones.clone()),
        "epigrafes_actividad": actividad,
    });
    g.tecleado = tecleado;
    g.epigrafes_actividad = actividad;
    escribir(v, &g, "censo.guardar", "censo", detalle, now)
}

/// El contexto de la guía con el perfil y la modalidad del 202 del ejercicio que viene.
pub fn contexto(v: &Vault, hoy: Date) -> Result<guia::Contexto> {
    let perfil: Option<TaxpayerProfile> = v.setting(PERFIL)?;
    let siguiente = hoy.year() + i32::from(hoy.month() > Month::February);
    let m = crate::models::pago202::modalidad(v, siguiente).ok();
    Ok(guia::Contexto {
        persona: perfil.as_ref().map(|p| p.persona),
        cierre: perfil.as_ref().map(|p| p.cierre_ejercicio),
        ccaa: perfil.as_ref().and_then(|p| p.ccaa),
        festivos_locales: perfil
            .as_ref()
            .map(|p| p.festivos_locales.clone())
            .unwrap_or_default(),
        base_202_obligatoria: m.as_ref().is_some_and(|m| m.obligatoria),
        base_202_elegida: m
            .as_ref()
            .is_some_and(|m| m.elegida == Some(crate::models::pago202::Modalidad::Base)),
        avisos_opciones: perfil.as_ref().is_some_and(|p| p.opciones_censales),
    })
}

/// La guía de un supuesto para la empresa.
pub fn guia_de(v: &Vault, id: &str, hecho: Option<Date>, hoy: Date) -> Result<guia::Guia> {
    guia::guia(id, hecho, hoy, &contexto(v, hoy)?)
        .ok_or_else(|| other(format!("no existe el supuesto «{id}»")))
}

/// Apunta en el calendario el plazo de comunicar un cambio.
pub fn programar(
    v: &Vault,
    id: &str,
    hecho: Option<Date>,
    nota: &str,
    hoy: Date,
    now: OffsetDateTime,
) -> Result<Comunicacion> {
    let gu = guia_de(v, id, hecho, hoy)?;
    let vence = gu.plazo.vence.ok_or_else(|| {
        other(if gu.plazo.pide_fecha {
            "pon la fecha del hecho para calcular el plazo"
        } else {
            "este supuesto no tiene un último día que apuntar"
        })
    })?;
    let mut g = guardado(v)?;
    if g.comunicaciones
        .iter()
        .any(|c| c.supuesto == id && c.hecha.is_none())
    {
        return Err(other(format!(
            "«{}» ya está en el calendario: márcalo como presentado o quítalo antes",
            gu.titulo
        )));
    }
    g.siguiente += 1;
    let c = Comunicacion {
        id: g.siguiente,
        supuesto: id.into(),
        titulo: gu.titulo.clone(),
        fecha_hecho: hecho,
        vence,
        nota: nota.trim().chars().take(500).collect(),
        creada: now_str(now),
        hecha: None,
    };
    g.comunicaciones.push(c.clone());
    escribir(
        v,
        &g,
        "censo.programar",
        id,
        serde_json::json!({ "id": c.id, "vence": vence.to_string() }),
        now,
    )?;
    Ok(c)
}

/// Marca una comunicación como presentada (deja de salir en el calendario).
pub fn marcar_hecha(v: &Vault, id: i64, now: OffsetDateTime) -> Result<()> {
    let mut g = guardado(v)?;
    let c = g
        .comunicaciones
        .iter_mut()
        .find(|c| c.id == id)
        .ok_or_else(|| other("no existe ese aviso"))?;
    c.hecha = Some(now_str(now));
    let sujeto = c.supuesto.clone();
    escribir(
        v,
        &g,
        "censo.presentada",
        &sujeto,
        serde_json::json!({ "id": id }),
        now,
    )
}

/// Quita un aviso del calendario (con la confirmación de la interfaz).
pub fn quitar(v: &Vault, id: i64, confirmed: bool, now: OffsetDateTime) -> Result<()> {
    if !confirmed {
        return Err(other("hace falta confirmar para quitar el aviso"));
    }
    let mut g = guardado(v)?;
    let Some(i) = g.comunicaciones.iter().position(|c| c.id == id) else {
        return Err(other("no existe ese aviso"));
    };
    let c = g.comunicaciones.remove(i);
    escribir(
        v,
        &g,
        "censo.quitar",
        &c.supuesto,
        serde_json::json!({ "id": id }),
        now,
    )
}

/// Los avisos pendientes que vencen en `year`, para el calendario.
pub fn avisos_calendario(v: &Vault, year: i32) -> Result<Vec<Comunicacion>> {
    Ok(guardado(v)?
        .comunicaciones
        .into_iter()
        .filter(|c| c.hecha.is_none() && c.vence.year() == year)
        .collect())
}

#[cfg(test)]
mod tests;
