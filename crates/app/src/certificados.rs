//! Certificados de retenciones e ingresos a cuenta para los perceptores: una hoja por perceptor
//! con lo que el borrador del 190, del 180 o del 193 declara de él (trabajadores,
//! administradores y profesionales; arrendadores; socios que cobran dividendos).
//!
//! Base legal: el retenedor tiene que expedir al perceptor una certificación de las retenciones
//! e ingresos a cuenta y de los demás datos que incluye en la declaración anual, y ponerla a su
//! disposición antes de que se abra el plazo de la Renta (art. 108.3 RIRPF, RD 439/2007). Si el
//! perceptor es una sociedad, la misma obligación está en el Reglamento del Impuesto sobre
//! Sociedades (RD 634/2015). `TODO(VERIFY)`: el artículo exacto del RIS (el art. 68 regula las
//! obligaciones formales del retenedor; no se ha cotejado su apartado con el BOE), por eso el
//! pie sólo cita el reglamento.
//!
//! No hay modelo oficial obligatorio de certificado (la AEAT publica modelos orientativos); el
//! texto sigue el contenido de la declaración anual. Las cifras salen de las mismas funciones
//! que el borrador ([`crate::models::result_190`], [`crate::models::result_180`] y
//! [`crate::models::result_193`]): el certificado cuadra siempre con lo que se declara.
//!
//! Alcance: lo que TaxPain declara en esos modelos. Sin retribuciones en especie ni ingresos a
//! cuenta (TaxPain no los registra: salen a cero), sin reducciones de los arts. 18.2 y 18.3
//! LIRPF; en el 180, el inmueble guardado de cada arrendador (`inmuebles`) o el que se escriba al
//! generar.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::invoice::PartyId;
use taxpain_core::money::{Money, Rate};
use taxpain_invoicing::pdf::{Para, text_document};
use taxpain_models::informative::{Key190, Result180, Result190, Result193};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;

const MESES: [&str; 12] = [
    "enero",
    "febrero",
    "marzo",
    "abril",
    "mayo",
    "junio",
    "julio",
    "agosto",
    "septiembre",
    "octubre",
    "noviembre",
    "diciembre",
];

/// Modelos de los que se sacan certificados.
pub const MODELOS: [&str; 4] = ["190", "180", "193", "296"];

/// Lo que se elige al generar los certificados.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opciones {
    /// Lugar de expedición («València»).
    pub place: String,
    /// Fecha de expedición.
    pub date: Date,
    /// Quien firma (nombre del administrador) y su cargo («Administrador único»).
    #[serde(default)]
    pub signer: String,
    #[serde(default)]
    pub signer_role: String,
    /// NIF de los perceptores elegidos; vacío, todos.
    #[serde(default)]
    pub nifs: Vec<String>,
    /// 180: descripción del inmueble de cada arrendador (NIF → dirección y referencia
    /// catastral) tecleada al generar; sin ella, la del inmueble guardado del arrendador.
    #[serde(default)]
    pub properties: BTreeMap<String, String>,
}

/// Una percepción del certificado, con las cifras del registro del modelo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind")]
pub enum Detalle {
    /// 190: un registro de perceptor (clave y subclave).
    Trabajo {
        key: Key190,
        /// Percepciones dinerarias íntegras y su retención.
        dinerarias: Money,
        retenciones: Money,
        /// Percepciones en especie (valoración) y su ingreso a cuenta (TaxPain: cero).
        especie: Money,
        ingresos_a_cuenta: Money,
        /// Gastos deducibles: cotizaciones del perceptor a la Seguridad Social (art. 19.2.a
        /// LIRPF), en las claves A, E.01 y E.02.
        gastos: Money,
        /// Reducciones de los arts. 18.2 y 18.3 LIRPF (TaxPain: cero; sólo se imprimen si hay).
        reducciones: Money,
    },
    /// 180: un arrendador e inmueble.
    Arrendamiento {
        inmueble: String,
        rentas: Money,
        tipo: Rate,
        retenciones: Money,
    },
    /// 193: un dividendo (naturaleza y fecha de devengo).
    Capital {
        devengo: Date,
        /// 02 con retención; 08 no sometido a retención.
        naturaleza: u8,
        integro: Money,
        base: Money,
        tipo: Rate,
        retenciones: Money,
    },
    /// 296: rentas de un no residente sin establecimiento permanente (naturaleza, régimen y
    /// tipo).
    NoResidente {
        naturaleza: String,
        pais: String,
        /// «tipo general», «convenio» o la exención aplicada.
        regimen: String,
        desde: Date,
        hasta: Date,
        integro: Money,
        tipo: Rate,
        retenciones: Money,
    },
}

impl Detalle {
    /// Importe íntegro que suma en el total del certificado.
    pub fn integro(&self) -> Money {
        match self {
            Detalle::Trabajo {
                dinerarias,
                especie,
                ..
            } => *dinerarias + *especie,
            Detalle::Arrendamiento { rentas, .. } => *rentas,
            Detalle::Capital { integro, .. } | Detalle::NoResidente { integro, .. } => *integro,
        }
    }

    /// Retenciones e ingresos a cuenta.
    pub fn retenido(&self) -> Money {
        match self {
            Detalle::Trabajo {
                retenciones,
                ingresos_a_cuenta,
                ..
            } => *retenciones + *ingresos_a_cuenta,
            Detalle::Arrendamiento { retenciones, .. }
            | Detalle::Capital { retenciones, .. }
            | Detalle::NoResidente { retenciones, .. } => *retenciones,
        }
    }
}

/// El certificado de un perceptor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Certificado {
    pub model: String,
    pub year: i32,
    pub nif: String,
    pub name: String,
    /// Domicilio del perceptor, si TaxPain lo tiene (ficha del contacto o del socio).
    pub address: Option<String>,
    pub email: Option<String>,
    pub detalles: Vec<Detalle>,
}

impl Certificado {
    pub fn integro(&self) -> Money {
        self.detalles.iter().map(Detalle::integro).sum()
    }

    pub fn retenido(&self) -> Money {
        self.detalles.iter().map(Detalle::retenido).sum()
    }

    /// Perceptor persona jurídica (por su NIF): la obligación está en el RIS, no en el RIRPF.
    fn sociedad(&self) -> bool {
        taxpain_core::Nif::parse(&self.nif).is_ok_and(|n| !n.is_persona_fisica())
    }
}

/// Fila de la lista de perceptores de la pantalla.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Perceptor {
    pub nif: String,
    pub name: String,
    /// «clave G.01», «arrendamiento», «dividendos»…
    pub concept: String,
    pub gross: Money,
    pub withheld: Money,
    pub email: Option<String>,
}

/// Lo que necesita la pantalla para elegir: perceptores y lo propuesto para firmar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Preparacion {
    pub model: String,
    pub year: i32,
    pub payees: Vec<Perceptor>,
    /// Lugar propuesto: la población del domicilio de la empresa.
    pub place: String,
    /// Firmante propuesto: el administrador de la ficha de Nóminas, si lo hay.
    pub signer: String,
    pub signer_role: String,
    pub notes: Vec<String>,
}

/// Datos del retenedor que van en cada hoja.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Retenedor {
    pub name: String,
    pub nif: String,
    pub address: Option<String>,
    pub town: String,
}

fn retenedor(v: &Vault) -> Result<Retenedor> {
    let s = crate::invoicing::settings(v)?.ok_or(AppError::NotConfigured)?;
    let who = crate::models::declarant(v)?;
    Ok(Retenedor {
        name: if s.issuer.name.trim().is_empty() {
            who.full_name()
        } else {
            s.issuer.name.trim().to_owned()
        },
        nif: who.nif,
        address: s.issuer.address.as_ref().map(|a| a.lines().join(", ")),
        town: s
            .issuer
            .address
            .as_ref()
            .map(|a| a.town.clone())
            .unwrap_or_default(),
    })
}

fn check_model(model: &str) -> Result<()> {
    if MODELOS.contains(&model) {
        Ok(())
    } else {
        Err(AppError::Other(format!(
            "no hay certificados de retenciones del modelo {model} (sólo 190, 180, 193 y 296)"
        )))
    }
}

/// Certificados del 190: uno por perceptor, con un bloque por clave y subclave.
pub fn de_190(r: &Result190) -> Vec<Certificado> {
    let mut by: BTreeMap<String, Certificado> = BTreeMap::new();
    for l in &r.lines {
        let key = l.effective_key();
        let gastos = match (&l.employee, key) {
            (Some(e), _) => e.deductible_expenses,
            (None, Key190::E(1 | 2)) => l.deductible,
            _ => Money::ZERO,
        };
        by.entry(l.nif.clone())
            .or_insert_with(|| empty("190", r.year, &l.nif, &l.name))
            .detalles
            .push(Detalle::Trabajo {
                key,
                dinerarias: l.gross,
                retenciones: l.withheld,
                especie: Money::ZERO,
                ingresos_a_cuenta: Money::ZERO,
                gastos,
                reducciones: Money::ZERO,
            });
    }
    by.into_values().collect()
}

/// Certificados del 180: uno por arrendador, con un bloque por inmueble. `properties` da la
/// descripción del inmueble (NIF → texto) cuando el registro no la tiene.
pub fn de_180(r: &Result180, properties: &BTreeMap<String, String>) -> Vec<Certificado> {
    let mut by: BTreeMap<String, Certificado> = BTreeMap::new();
    for l in &r.lines {
        let inmueble = match &l.property {
            Some(p) => format!(
                "{} {}, {} · ref. catastral {}",
                p.street, p.number, p.municipality, p.cadastral_ref
            ),
            None => properties
                .get(&l.nif)
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
                .unwrap_or_default(),
        };
        let tipo = if l.base.is_zero() {
            Rate::ZERO
        } else {
            Rate::from_bp(
                i32::try_from(i128::from(l.withheld.cents()) * 10_000 / i128::from(l.base.cents()))
                    .unwrap_or(0),
            )
        };
        by.entry(l.nif.clone())
            .or_insert_with(|| empty("180", r.year, &l.nif, &l.name))
            .detalles
            .push(Detalle::Arrendamiento {
                inmueble,
                rentas: l.base,
                tipo,
                retenciones: l.withheld,
            });
    }
    by.into_values().collect()
}

/// Certificados del 296: uno por perceptor no residente, con un bloque por naturaleza, régimen
/// y tipo.
pub fn de_296(r: &taxpain_models::irnr::Result296) -> Vec<Certificado> {
    use taxpain_models::irnr::Regimen;
    let mut by: BTreeMap<String, Certificado> = BTreeMap::new();
    for l in &r.lines {
        by.entry(l.nif.clone())
            .or_insert_with(|| empty("296", r.year, &l.nif, &l.nombre))
            .detalles
            .push(Detalle::NoResidente {
                naturaleza: l.naturaleza.label().into(),
                pais: l.pais.clone(),
                regimen: match (l.regimen, l.exencion) {
                    (Regimen::Exenta, Some(e)) => e.label().into(),
                    (Regimen::Exenta, None) => "exenta".into(),
                    (Regimen::Convenio, _) => "tipo reducido del convenio".into(),
                    (Regimen::General, _) => "tipo general (art. 25.1 TRLIRNR)".into(),
                },
                desde: l.desde,
                hasta: l.hasta,
                integro: l.bruto,
                tipo: l.tipo,
                retenciones: l.retencion,
            });
    }
    by.into_values().collect()
}

/// Certificados del 193: uno por socio, con un bloque por dividendo (fecha de devengo y
/// naturaleza).
pub fn de_193(r: &Result193) -> Vec<Certificado> {
    let mut by: BTreeMap<String, Certificado> = BTreeMap::new();
    for l in &r.lines {
        by.entry(l.nif.clone())
            .or_insert_with(|| empty("193", r.year, &l.nif, &l.name))
            .detalles
            .push(Detalle::Capital {
                devengo: l.accrual,
                naturaleza: l.nature,
                integro: l.gross,
                base: l.base,
                tipo: l.rate,
                retenciones: l.withheld,
            });
    }
    by.into_values().collect()
}

fn empty(model: &str, year: i32, nif: &str, name: &str) -> Certificado {
    Certificado {
        model: model.into(),
        year,
        nif: nif.into(),
        name: name.into(),
        address: None,
        email: None,
        detalles: Vec::new(),
    }
}

/// Todos los certificados del modelo y año (sin filtrar), con el domicilio y el correo de cada
/// perceptor si TaxPain los tiene.
fn all(v: &Vault, model: &str, year: i32, opts: Option<&Opciones>) -> Result<Vec<Certificado>> {
    check_model(model)?;
    let mut certs = match model {
        "190" => de_190(&crate::models::result_190(v, year)?),
        "180" => {
            // Lo tecleado al generar manda; si no, el inmueble guardado del arrendador (el
            // registro del 180 ya lo trae cuando está guardado).
            let mut props = opts.map(|o| o.properties.clone()).unwrap_or_default();
            for i in crate::inmuebles::list(v)? {
                props.entry(i.nif.clone()).or_insert_with(|| i.texto());
            }
            let mut certs = de_180(&crate::models::result_180(v, year)?, &props);
            if let Some(o) = opts {
                for c in &mut certs {
                    if let Some(t) = o.properties.get(&c.nif).filter(|t| !t.trim().is_empty()) {
                        for d in &mut c.detalles {
                            if let Detalle::Arrendamiento { inmueble, .. } = d {
                                *inmueble = t.trim().to_owned();
                            }
                        }
                    }
                }
            }
            certs
        }
        "296" => de_296(&crate::no_residentes::result_296(v, year)?),
        _ => de_193(&crate::models::result_193(v, year)?),
    };
    let socios: BTreeMap<String, String> = if model == "193" || model == "296" {
        crate::socios::shareholders(v)?
            .into_iter()
            .filter(|s| !s.address.trim().is_empty())
            .map(|s| (s.nif.to_uppercase(), s.address))
            .collect()
    } else {
        BTreeMap::new()
    };
    for c in &mut certs {
        if let Some(d) = crate::contacts::find(v, &PartyId::Nif(c.nif.clone()))? {
            c.email = d.email.filter(|e| !e.trim().is_empty());
            c.address = d.party.address.as_ref().map(|a| a.lines().join(", "));
        }
        if let Some(a) = socios.get(&c.nif) {
            c.address = Some(a.clone());
        }
    }
    Ok(certs)
}

/// Perceptores del borrador para elegir en la pantalla, con lo propuesto para la firma.
pub fn prepare(v: &Vault, model: &str, year: i32) -> Result<Preparacion> {
    let certs = all(v, model, year, None)?;
    let who = retenedor(v)?;
    let admin = crate::payroll::employees(v)?
        .into_iter()
        .find(|e| e.administrator && e.active);
    let payees = certs
        .iter()
        .map(|c| Perceptor {
            nif: c.nif.clone(),
            name: c.name.clone(),
            concept: concept(c),
            gross: c.integro(),
            withheld: c.retenido(),
            email: c.email.clone(),
        })
        .collect();
    let mut notes = Vec::new();
    if model == "190" {
        notes.push(
            "TaxPain no registra retribuciones en especie, ingresos a cuenta ni reducciones: \
             salen a cero. Si los hubo, corrige el certificado antes de entregarlo."
                .into(),
        );
    }
    if model == "180" {
        let guardados: Vec<String> = crate::inmuebles::list(v)?
            .into_iter()
            .map(|i| i.nif)
            .collect();
        let sin: Vec<&str> = certs
            .iter()
            .filter(|c| !guardados.contains(&c.nif))
            .map(|c| c.name.as_str())
            .collect();
        if !sin.is_empty() {
            notes.push(format!(
                "Sin inmueble guardado ({}): guárdalo en Modelos → 180 → Inmuebles arrendados, o \
                 escribe la dirección y la referencia catastral aquí, o saldrán en blanco.",
                sin.join(", ")
            ));
        }
    }
    Ok(Preparacion {
        model: model.into(),
        year,
        payees,
        place: who.town,
        signer: admin.as_ref().map(|e| e.name.clone()).unwrap_or_default(),
        signer_role: if taxpain_core::Nif::parse(&who.nif).is_ok_and(|n| n.is_persona_fisica()) {
            String::new()
        } else {
            "Administrador".into()
        },
        notes,
    })
}

fn concept(c: &Certificado) -> String {
    let mut keys: Vec<String> = Vec::new();
    for d in &c.detalles {
        let k = match d {
            Detalle::Trabajo { key, .. } => key.label(),
            Detalle::Arrendamiento { .. } => "arrendamiento".into(),
            Detalle::Capital { naturaleza: 3, .. } => "intereses de préstamos".into(),
            Detalle::Capital { .. } => "dividendos".into(),
            Detalle::NoResidente { naturaleza, .. } => naturaleza.to_lowercase(),
        };
        if !keys.contains(&k) {
            keys.push(k);
        }
    }
    keys.join(", ")
}

/// Certificados elegidos (todos si `opts.nifs` está vacío).
pub fn certificates(
    v: &Vault,
    model: &str,
    year: i32,
    opts: &Opciones,
) -> Result<Vec<Certificado>> {
    let wanted: Vec<String> = opts.nifs.iter().map(|n| n.trim().to_uppercase()).collect();
    let certs: Vec<Certificado> = all(v, model, year, Some(opts))?
        .into_iter()
        .filter(|c| wanted.is_empty() || wanted.contains(&c.nif))
        .collect();
    if certs.is_empty() {
        return Err(AppError::Other(format!(
            "el {model} de {year} no tiene perceptores{}: no hay certificados que expedir",
            if wanted.is_empty() {
                ""
            } else {
                " con esos NIF"
            }
        )));
    }
    if opts.place.trim().is_empty() {
        return Err(AppError::Other("pon el lugar de expedición".into()));
    }
    Ok(certs)
}

/// «16 de noviembre de 2026».
pub fn long_date(d: Date) -> String {
    format!(
        "{} de {} de {}",
        d.day(),
        MESES[usize::from(u8::from(d.month())) - 1],
        d.year()
    )
}

fn key_title(k: Key190) -> String {
    let what = match k {
        Key190::A => "Rendimientos del trabajo: empleados por cuenta ajena",
        Key190::E(_) => "Rendimientos del trabajo: retribuciones de consejeros y administradores",
        Key190::G(_) => "Rendimientos de actividades económicas: actividades profesionales",
        Key190::H(_) => {
            "Rendimientos de actividades económicas: agrícolas, ganaderas, forestales o en \
             estimación objetiva"
        }
        Key190::L01 => "Dietas y asignaciones para gastos de viaje exceptuadas de gravamen",
    };
    format!("{what} ({})", k.label())
}

fn heading(model: &str) -> &'static str {
    match model {
        "190" => "Rendimientos del trabajo y de actividades económicas",
        "180" => "Rendimientos procedentes del arrendamiento de inmuebles urbanos",
        "296" => "Rentas obtenidas sin mediación de establecimiento permanente",
        _ => "Rendimientos del capital mobiliario: dividendos y participaciones en beneficios",
    }
}

fn dmy(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

/// Hoja del certificado de un perceptor.
pub fn paras(who: &Retenedor, c: &Certificado, opts: &Opciones) -> Vec<Para> {
    let m = |x: Money| x.to_string();
    let tax = if c.model == "296" {
        "del Impuesto sobre la Renta de no Residentes"
    } else if c.sociedad() {
        "del Impuesto sobre Sociedades"
    } else {
        "del Impuesto sobre la Renta de las Personas Físicas"
    };
    let mut p = vec![
        Para::Title(format!(
            "Certificado de retenciones e ingresos a cuenta · Ejercicio {}",
            c.year
        )),
        Para::Bold(format!("{} · Modelo {}", heading(&c.model), c.model)),
        Para::Gap,
        Para::Bold("Retenedor o pagador".into()),
        Para::Text(format!("Nombre o razón social: {}", who.name)),
        Para::Text(format!("NIF: {}", who.nif)),
    ];
    if let Some(a) = &who.address {
        p.push(Para::Text(format!("Domicilio: {a}")));
    }
    p.push(Para::Gap);
    p.push(Para::Bold("Perceptor".into()));
    p.push(Para::Text(format!(
        "Apellidos y nombre o razón social: {}",
        c.name
    )));
    p.push(Para::Text(format!("NIF: {}", c.nif)));
    if let Some(a) = &c.address {
        p.push(Para::Text(format!("Domicilio: {a}")));
    }
    p.push(Para::Gap);
    p.push(Para::Text(format!(
        "{}, con NIF {}, CERTIFICA que durante el ejercicio {} ha satisfecho a {}, con NIF {}, \
         las rentas que se detallan a continuación, sobre las que ha practicado las retenciones e \
         ingresos a cuenta {tax} que se indican, incluidas en su declaración anual (modelo {}):",
        who.name, who.nif, c.year, c.name, c.nif, c.model
    )));
    for d in &c.detalles {
        p.push(Para::Gap);
        match d {
            Detalle::Trabajo {
                key,
                dinerarias,
                retenciones,
                especie,
                ingresos_a_cuenta,
                gastos,
                reducciones,
            } => {
                p.push(Para::Bold(key_title(*key)));
                if *key == Key190::L01 {
                    p.push(Para::Row(
                        "Importe satisfecho (exceptuado de gravamen, art. 9 RIRPF)".into(),
                        m(*dinerarias),
                    ));
                    continue;
                }
                p.push(Para::Row(
                    "Percepciones dinerarias: importe íntegro satisfecho".into(),
                    m(*dinerarias),
                ));
                p.push(Para::Row(
                    "Percepciones dinerarias: retenciones practicadas".into(),
                    m(*retenciones),
                ));
                p.push(Para::Row(
                    "Percepciones en especie: valoración".into(),
                    m(*especie),
                ));
                p.push(Para::Row(
                    "Percepciones en especie: ingresos a cuenta efectuados".into(),
                    m(*ingresos_a_cuenta),
                ));
                if matches!(key, Key190::A | Key190::E(_)) {
                    p.push(Para::Row(
                        "Gastos deducibles (cotizaciones a la Seguridad Social, art. 19.2.a LIRPF)"
                            .into(),
                        m(*gastos),
                    ));
                }
                if !reducciones.is_zero() {
                    p.push(Para::Row(
                        "Reducciones (art. 18.2 y 18.3 LIRPF)".into(),
                        m(*reducciones),
                    ));
                }
            }
            Detalle::Arrendamiento {
                inmueble,
                rentas,
                tipo,
                retenciones,
            } => {
                p.push(Para::Bold("Arrendamiento de inmueble urbano".into()));
                p.push(Para::Text(format!(
                    "Inmueble: {}",
                    if inmueble.is_empty() {
                        "______________________________________________"
                    } else {
                        inmueble
                    }
                )));
                p.push(Para::Row(
                    "Rentas satisfechas (base de retención, sin IVA)".into(),
                    m(*rentas),
                ));
                p.push(Para::Row("Tipo de retención".into(), tipo.to_string()));
                p.push(Para::Row("Retenciones practicadas".into(), m(*retenciones)));
            }
            Detalle::Capital {
                devengo,
                naturaleza,
                integro,
                base,
                tipo,
                retenciones,
            } => {
                // Naturaleza 03 (clave B): intereses de un préstamo; no tienen fecha de devengo
                // propia en el 193.
                p.push(Para::Bold(if *naturaleza == 3 {
                    "Intereses de préstamos (rendimientos del capital mobiliario por la cesión a \
                     terceros de capitales propios)"
                        .to_string()
                } else {
                    format!(
                        "Dividendos y participaciones en beneficios · devengo {}",
                        dmy(*devengo)
                    )
                }));
                p.push(Para::Row("Rendimiento íntegro".into(), m(*integro)));
                if *naturaleza == 8 {
                    p.push(Para::Text(
                        "No sometido a retención: dividendo de una sociedad con derecho a la \
                         exención del art. 21 LIS (art. 128.4.d LIS y art. 61.p RIS)."
                            .into(),
                    ));
                } else {
                    p.push(Para::Row("Base de retención".into(), m(*base)));
                    p.push(Para::Row("Tipo de retención".into(), tipo.to_string()));
                    p.push(Para::Row("Retención practicada".into(), m(*retenciones)));
                }
            }
            Detalle::NoResidente {
                naturaleza,
                pais,
                regimen,
                desde,
                hasta,
                integro,
                tipo,
                retenciones,
            } => {
                p.push(Para::Bold(format!(
                    "{naturaleza} · residente en {pais} · devengo {}{}",
                    dmy(*desde),
                    if desde == hasta {
                        String::new()
                    } else {
                        format!(" a {}", dmy(*hasta))
                    }
                )));
                p.push(Para::Row(
                    "Importe íntegro (base de retención)".into(),
                    m(*integro),
                ));
                p.push(Para::Row("Régimen".into(), regimen.clone()));
                p.push(Para::Row("Tipo de retención".into(), tipo.to_string()));
                p.push(Para::Row("Retención practicada".into(), m(*retenciones)));
            }
        }
    }
    p.push(Para::Gap);
    p.push(Para::Row("Total importe íntegro".into(), m(c.integro())));
    p.push(Para::Row(
        "Total retenciones e ingresos a cuenta".into(),
        m(c.retenido()),
    ));
    p.push(Para::Gap);
    p.push(Para::Text(format!(
        "Y para que conste, a efectos de su declaración {tax}, se expide el presente certificado."
    )));
    p.push(Para::Text(format!(
        "En {}, a {}.",
        opts.place.trim(),
        long_date(opts.date)
    )));
    p.push(Para::Gap);
    p.push(Para::Gap);
    p.push(Para::Text("Firma: ______________________________".into()));
    let signer = opts.signer.trim();
    let role = opts.signer_role.trim();
    p.push(Para::Text(match (signer.is_empty(), role.is_empty()) {
        (false, false) => format!("{signer}, {role} de {}", who.name),
        (false, true) => format!("{signer}, en nombre de {}", who.name),
        (true, false) => format!("{role} de {}", who.name),
        (true, true) => format!("En nombre de {}", who.name),
    }));
    p.push(Para::Gap);
    p.push(Para::Text(if c.model == "296" {
        footer_irnr()
    } else {
        footer(c.sociedad())
    }));
    p
}

/// Pie con la norma que obliga a expedir el certificado.
fn footer(sociedad: bool) -> String {
    let mut s = String::from(
        "Certificado expedido conforme al art. 108.3 del Reglamento del IRPF (Real Decreto \
         439/2007): el retenedor debe expedir en favor del perceptor certificación acreditativa \
         de las retenciones e ingresos a cuenta efectuados y de los demás datos incluidos en la \
         declaración anual, y ponerla a su disposición antes de la apertura del plazo de \
         declaración del Impuesto.",
    );
    if sociedad {
        // TODO(VERIFY): artículo y apartado del RIS (RD 634/2015) con la misma obligación.
        s.push_str(
            " Para perceptores sujetos al Impuesto sobre Sociedades, en los mismos términos del \
             Reglamento del Impuesto (Real Decreto 634/2015).",
        );
    }
    s
}

/// Pie del certificado del 296. `TODO(VERIFY)`: el artículo del TRLIRNR o del RIRNR que obliga
/// a expedirlo (el art. 31 TRLIRNR regula la obligación de retener; no se ha cotejado el
/// apartado que pide el certificado), por eso sólo se citan las normas.
fn footer_irnr() -> String {
    "Certificado expedido a efectos del Impuesto sobre la Renta de no Residentes (texto \
     refundido aprobado por el Real Decreto Legislativo 5/2004 y su Reglamento, Real Decreto \
     1776/2004), con los datos incluidos en el resumen anual de retenciones (modelo 296)."
        .into()
}

fn stem(c: &Certificado) -> String {
    format!("Certificado retenciones {} {} {}", c.model, c.year, c.nif)
}

/// Un PDF con todos los certificados elegidos (una hoja por perceptor).
pub fn pdf_all(v: &Vault, model: &str, year: i32, opts: &Opciones) -> Result<(String, Vec<u8>)> {
    let who = retenedor(v)?;
    let certs = certificates(v, model, year, opts)?;
    let docs: Vec<Vec<Para>> = certs.iter().map(|c| paras(&who, c, opts)).collect();
    let pdf = text_document(
        &format!("Certificados de retenciones del {model} de {year}"),
        &docs,
    )
    .map_err(AppError::Other)?;
    Ok((
        format!("Certificados retenciones {model} {year} {}.pdf", who.nif),
        pdf,
    ))
}

/// Un PDF por perceptor: (nombre del fichero, PDF).
pub fn pdf_each(
    v: &Vault,
    model: &str,
    year: i32,
    opts: &Opciones,
) -> Result<Vec<(String, Vec<u8>)>> {
    let who = retenedor(v)?;
    certificates(v, model, year, opts)?
        .iter()
        .map(|c| {
            let pdf = text_document(
                &format!("Certificado de retenciones {year} · {}", c.name),
                &[paras(&who, c, opts)],
            )
            .map_err(AppError::Other)?;
            Ok((format!("{}.pdf", stem(c)), pdf))
        })
        .collect()
}

/// Correo `.eml` sin enviar con el certificado de un perceptor adjunto, para abrirlo en el
/// programa de correo (ver [`crate::correo`]). `to` vacío: el de su ficha, si lo tiene.
pub fn email(
    v: &Vault,
    model: &str,
    year: i32,
    opts: &Opciones,
    nif: &str,
    to: Option<&str>,
) -> Result<(String, Vec<u8>)> {
    let who = retenedor(v)?;
    let one = Opciones {
        nifs: vec![nif.to_owned()],
        ..opts.clone()
    };
    let c = certificates(v, model, year, &one)?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Other(format!("{nif} no está en el {model} de {year}")))?;
    let to = to
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .or_else(|| c.email.clone());
    if let Some(t) = &to
        && t.split(',')
            .any(|a| !a.contains('@') || a.trim().contains(char::is_whitespace))
    {
        return Err(AppError::Other(format!("el correo {t} no es válido")));
    }
    let pdf = text_document(
        &format!("Certificado de retenciones {year} · {}", c.name),
        &[paras(&who, &c, opts)],
    )
    .map_err(AppError::Other)?;
    let subject = format!("Certificado de retenciones de {year} ({})", who.name);
    let body = format!(
        "Hola:\n\nTe adjuntamos el certificado de las retenciones e ingresos a cuenta de {year} \
         ({}), para tu declaración {}.\n\nSi algún dato no coincide con lo que tienes, dínoslo.\n\n\
         Un saludo,\n{}\n",
        heading(model).to_lowercase(),
        if c.sociedad() {
            "del Impuesto sobre Sociedades"
        } else {
            "de la Renta"
        },
        who.name
    );
    let mut b = [0u8; 12];
    getrandom::fill(&mut b).map_err(|e| AppError::Other(e.to_string()))?;
    let boundary = format!("TaxPain-{}", hex::encode(b));
    let stem = stem(&c);
    let bytes = crate::correo::eml(
        to.as_deref(),
        &subject,
        &body,
        &[(format!("{stem}.pdf"), "application/pdf", pdf)],
        &boundary,
    );
    Ok((format!("{stem}.eml"), bytes))
}

/// Deja en la auditoría los certificados expedidos (`how`: «carpeta» o «correo»).
pub fn record_issued(
    v: &Vault,
    model: &str,
    year: i32,
    nifs: &[String],
    how: &str,
    now: OffsetDateTime,
) -> Result<()> {
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        "certificados_retenciones.expedidos",
        &format!("Certificados de retenciones del {model} de {year}"),
        &serde_json::json!({ "model": model, "year": year, "nifs": nifs, "how": how }),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use taxpain_models::informative::{
        Dividend193, Employee190, Payment190, Rent180, compute_180, compute_190, compute_193,
    };
    use time::macros::date;

    fn eur(e: i64) -> Money {
        Money::from_euros(e, 0)
    }

    fn opts() -> Opciones {
        Opciones {
            place: "València".into(),
            date: date!(2027 - 01 - 20),
            signer: "Carmen Española Española".into(),
            signer_role: "Administradora única".into(),
            nifs: vec![],
            properties: BTreeMap::new(),
        }
    }

    fn who() -> Retenedor {
        Retenedor {
            name: "Estudio Demo SL".into(),
            nif: "B12345674".into(),
            address: Some("Calle Inventada 1, 46001 València".into()),
            town: "València".into(),
        }
    }

    fn pay(nif: &str, key: Key190, gross: i64, withheld: i64, deductible: i64) -> Payment190 {
        Payment190 {
            nif: nif.into(),
            name: format!("Perceptor {nif}"),
            province: "46".into(),
            gross: eur(gross),
            withheld: eur(withheld),
            employee: None,
            key,
            deductible: eur(deductible),
        }
    }

    #[test]
    fn el_190_da_un_certificado_por_perceptor_con_sus_claves() {
        let mut emp = pay("00000000T", Key190::A, 18_000, 2_000, 0);
        emp.employee = Some(Employee190 {
            birth_year: 1990,
            family: 3,
            contract: 1,
            deductible_expenses: eur(1_150),
            ..Employee190::default()
        });
        let r = compute_190(
            2026,
            &[
                pay("12345678Z", Key190::E(2), 6_000, 1_140, 380),
                pay("12345678Z", Key190::E(2), 6_000, 1_140, 380),
                pay("12345678Z", Key190::L01, 300, 0, 0),
                pay("A22222228", Key190::G(1), 2_000, 300, 0),
                emp,
            ],
        );
        let certs = de_190(&r);
        assert_eq!(certs.len(), 3);
        // Cuadra con los registros del 190, perceptor a perceptor.
        for c in &certs {
            let lines: Vec<_> = r.lines.iter().filter(|l| l.nif == c.nif).collect();
            assert_eq!(c.detalles.len(), lines.len());
            assert_eq!(c.integro(), lines.iter().map(|l| l.gross).sum());
            assert_eq!(c.retenido(), lines.iter().map(|l| l.withheld).sum());
        }
        let admin = certs.iter().find(|c| c.nif == "12345678Z").unwrap();
        assert!(admin.detalles.contains(&Detalle::Trabajo {
            key: Key190::E(2),
            dinerarias: eur(12_000),
            retenciones: eur(2_280),
            especie: Money::ZERO,
            ingresos_a_cuenta: Money::ZERO,
            gastos: eur(760),
            reducciones: Money::ZERO,
        }));
        let emp = certs.iter().find(|c| c.nif == "00000000T").unwrap();
        assert!(matches!(
            emp.detalles[0],
            Detalle::Trabajo { gastos, .. } if gastos == eur(1_150)
        ));
        let total: Money = certs.iter().map(Certificado::retenido).sum();
        assert_eq!(total, r.total_withheld);

        let text = format!("{:?}", paras(&who(), admin, &opts()));
        assert!(text.contains("clave E.02"), "{text}");
        assert!(text.contains("clave L.01"));
        assert!(text.contains("12.000,00"));
        assert!(text.contains("2.280,00"));
        assert!(text.contains("760,00"));
        assert!(text.contains("En València, a 20 de enero de 2027."));
        assert!(text.contains("Carmen Española Española, Administradora única de Estudio Demo SL"));
        assert!(text.contains("art. 108.3"));
        assert!(text.contains("Personas Físicas"));
    }

    #[test]
    fn el_180_y_el_193_cuadran_con_sus_registros() {
        let r = compute_180(
            2026,
            &[
                Rent180 {
                    nif: "A22222228".into(),
                    name: "Inmuebles Ejemplo SA".into(),
                    province: "46".into(),
                    base: eur(1_000),
                    withheld: eur(190),
                    property: None,
                },
                Rent180 {
                    nif: "A22222228".into(),
                    name: "Inmuebles Ejemplo SA".into(),
                    province: "46".into(),
                    base: eur(1_000),
                    withheld: eur(190),
                    property: None,
                },
            ],
        );
        let mut props = BTreeMap::new();
        props.insert(
            "A22222228".to_owned(),
            "Calle Inventada 2, local".to_owned(),
        );
        let c = de_180(&r, &props);
        assert_eq!(c.len(), 1);
        assert_eq!(
            c[0].detalles[0],
            Detalle::Arrendamiento {
                inmueble: "Calle Inventada 2, local".into(),
                rentas: eur(2_000),
                tipo: Rate::percent(19),
                retenciones: eur(380),
            }
        );
        let text = format!("{:?}", paras(&who(), &c[0], &opts()));
        assert!(text.contains("19 %") && text.contains("Calle Inventada 2"));
        // Arrendador sociedad: también cita el Reglamento del IS.
        assert!(text.contains("634/2015") && text.contains("Impuesto sobre Sociedades"));

        let d = |nif: &str, gross: i64, withheld: i64, rate: i32| Dividend193 {
            nif: nif.into(),
            name: format!("Socio {nif}"),
            province: "46".into(),
            accrual: date!(2026 - 07 - 15),
            gross: eur(gross),
            withheld: eur(withheld),
            rate: Rate::percent(rate),
        };
        let r = compute_193(
            2026,
            &[
                d("12345678Z", 6_000, 1_140, 19),
                d("B87654323", 2_500, 0, 0),
            ],
        );
        let c = de_193(&r);
        assert_eq!(c.len(), 2);
        for (cert, line) in c.iter().zip(&r.lines) {
            assert_eq!(cert.nif, line.nif);
            assert_eq!(cert.integro(), line.gross);
            assert_eq!(cert.retenido(), line.withheld);
        }
        let exenta = c.iter().find(|c| c.nif == "B87654323").unwrap();
        let text = format!("{:?}", paras(&who(), exenta, &opts()));
        assert!(text.contains("No sometido a retención") && text.contains("15/07/2026"));
    }

    #[test]
    fn fecha_larga() {
        assert_eq!(long_date(date!(2026 - 11 - 16)), "16 de noviembre de 2026");
        assert_eq!(long_date(date!(2027 - 01 - 01)), "1 de enero de 2027");
    }
}
