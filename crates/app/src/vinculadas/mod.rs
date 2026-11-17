//! Operaciones vinculadas (art. 18 LIS; arts. 13 a 16 RIS): quién es vinculado, qué se ha hecho
//! con cada uno en el año, si hay que presentar el modelo 232 (Orden HFP/816/2017) y la
//! documentación simplificada (art. 16.4 RIS).
//!
//! **Quién es vinculado** (art. 18.2 LIS). Sin teclear nada: los socios con el 25 % o más
//! (`shareholder`; el 25 % lo exige el último párrafo del art. 18.2 LIS), los administradores
//! (`employee.administrator`) y las participadas con el 25 % o más (`participada`, la ficha mínima
//! de este módulo). A mano (`vinculada`): familiares de socios o administradores, entidades del
//! grupo del art. 42 CCom, administradores de otra entidad del grupo, entidades con los mismos
//! socios, participadas indirectas y establecimientos permanentes, o cualquiera de los anteriores
//! para añadir datos (grupo fiscal, clave de sus facturas).
//!
//! **Qué se ha hecho con cada uno** en el ejercicio, de lo que TaxPain ya sabe:
//! - facturas emitidas (ingreso) y recibidas revisadas (pago), por la base: servicios (clave 6)
//!   salvo que la ficha diga otra clave; las recibidas de alquiler van a la 8 y las de bienes de
//!   inversión a la 1;
//! - nóminas: las del administrador son «retribución por el ejercicio de sus funciones», que el
//!   art. 18.2.b LIS deja fuera; las de un socio o familiar empleado, clave 10;
//! - dividendos pagados (`dividend_line`) y cobrados (760 de una participada): para la memoria,
//!   no para el 232 (no tienen clave);
//! - movimientos del año de las subcuentas asignadas a una vinculada (`vinculada_cuenta`): las
//!   cuentas corrientes 551/552/553, los préstamos (16x, 171, 24x, 25x, 51x, 521, 53x, 54x),
//!   intereses (661/662, 761/762), alquileres (621/752) y dividendos (760);
//! - operaciones tecleadas (`vinculada_operacion`): compraventa de inmuebles o participaciones,
//!   intangibles, garantías…
//!
//! - préstamos de Contabilidad → Préstamos ([`crate::prestamos`]) marcados como vinculados o
//!   cuya contraparte es una vinculada (por NIF, o por el socio elegido): el principal dispuesto
//!   en el año (clave 5) y los intereses devengados (clave 9), como ingreso o pago según el
//!   sentido. Las subcuentas del préstamo asignadas a la misma persona ya no se cuentan por sus
//!   movimientos (serían las mismas operaciones dos veces). Una contraparte marcada como
//!   vinculada que no está en la lista se añade con el vínculo de la ficha del préstamo.
//!
//! **Umbrales del 232** (Orden HFP/816/2017, art. 2; RIS art. 13.4), comprobados con el texto
//! del BOE (BOE-A-2017-10042) el 16/11/2026: más de 250.000 € con la misma persona o entidad;
//! operaciones específicas (art. 18.3 LIS: las excluidas del contenido simplificado) cuyo importe
//! conjunto por tipo pase de 100.000 €; y las del mismo tipo y método de valoración que pasen del
//! 50 % de la cifra de negocios. No se declaran las del grupo de consolidación fiscal, las de
//! AIE y UTE con sus miembros ni las de OPV/OPA (art. 2.2). TODO(VERIFY): que el texto no haya
//! cambiado después y que el umbral de las específicas sea por tipo y no por persona.

pub mod documentacion;
pub mod hoja;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::money::Money;
use taxpain_core::nif::Nif;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;

/// Con la misma persona o entidad (Orden HFP/816/2017, art. 2.1; RIS art. 13.4).
pub const UMBRAL_PERSONA: Money = Money::from_euros(250_000, 0);
/// Operaciones específicas, por tipo (Orden HFP/816/2017, art. 2.1).
pub const UMBRAL_ESPECIFICAS: Money = Money::from_euros(100_000, 0);
/// Cifra de negocios por debajo de la cual la documentación tiene contenido simplificado (art.
/// 18.3 LIS; art. 16.4 RIS).
pub const INCN_SIMPLIFICADA: Money = Money::from_euros(45_000_000, 0);
/// Participación que hace vinculados a socio y sociedad (art. 18.2 LIS, último párrafo), en
/// millonésimas.
pub const PCT_VINCULADO: u32 = 250_000;

fn other(e: impl ToString) -> AppError {
    AppError::Other(e.to_string())
}

/// Participación en millonésimas como porcentaje («50 %», «33,3333 %»).
pub fn pct_texto(pct: u32) -> String {
    let (int, frac) = (pct / 10_000, pct % 10_000);
    if frac == 0 {
        format!("{int} %")
    } else {
        format!("{int},{} %", format!("{frac:04}").trim_end_matches('0'))
    }
}

/// Sólo letras y cifras, en mayúsculas: la clave con la que se casan los NIF.
pub fn clave_nif(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_uppercase()
}

// ---------- Claves ----------

/// Tipo de vinculación (art. 18.2 LIS). En el orden en que se elige el principal de una persona
/// con varios.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Vinculo {
    /// Socio o partícipe con el 25 % o más (art. 18.2.a).
    Socio,
    /// Entidad participada por la sociedad en el 25 % o más (art. 18.2.a, visto desde la socia).
    Participada,
    /// Consejero o administrador (art. 18.2.b), salvo su retribución por el cargo.
    Administrador,
    /// Cónyuge o pariente hasta el tercer grado de un socio o administrador (art. 18.2.c).
    Familiar,
    /// Entidad del mismo grupo del art. 42 CCom (art. 18.2.d).
    Grupo,
    /// Administrador de otra entidad del grupo (art. 18.2.e).
    AdministradorGrupo,
    /// Participada indirectamente en el 25 % o más (art. 18.2.f).
    ParticipadaIndirecta,
    /// Entidad con los mismos socios (o sus familiares) con el 25 % o más (art. 18.2.g).
    SociosComunes,
    /// Establecimiento permanente en el extranjero (art. 18.2.h).
    EstablecimientoPermanente,
}

impl Vinculo {
    pub const ALL: [Vinculo; 9] = [
        Vinculo::Socio,
        Vinculo::Participada,
        Vinculo::Administrador,
        Vinculo::Familiar,
        Vinculo::Grupo,
        Vinculo::AdministradorGrupo,
        Vinculo::ParticipadaIndirecta,
        Vinculo::SociosComunes,
        Vinculo::EstablecimientoPermanente,
    ];

    /// Letra del art. 18.2 LIS. TODO(VERIFY): el formulario del 232 pide «el tipo de
    /// vinculación de acuerdo con el artículo 18 LIS» (Orden HFP/816/2017, art. 3.1.d) sin
    /// publicar claves en la orden; se da la letra del artículo.
    pub fn letra(self) -> &'static str {
        match self {
            Vinculo::Socio | Vinculo::Participada => "a",
            Vinculo::Administrador => "b",
            Vinculo::Familiar => "c",
            Vinculo::Grupo => "d",
            Vinculo::AdministradorGrupo => "e",
            Vinculo::ParticipadaIndirecta => "f",
            Vinculo::SociosComunes => "g",
            Vinculo::EstablecimientoPermanente => "h",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Vinculo::Socio => "Socio con el 25 % o más",
            Vinculo::Participada => "Participada con el 25 % o más",
            Vinculo::Administrador => "Administrador",
            Vinculo::Familiar => "Familiar de socio o administrador",
            Vinculo::Grupo => "Entidad del grupo (art. 42 CCom)",
            Vinculo::AdministradorGrupo => "Administrador de otra entidad del grupo",
            Vinculo::ParticipadaIndirecta => "Participada indirecta con el 25 % o más",
            Vinculo::SociosComunes => "Entidad con los mismos socios (25 % o más)",
            Vinculo::EstablecimientoPermanente => "Establecimiento permanente en el extranjero",
        }
    }

    fn code(self) -> &'static str {
        match self {
            Vinculo::Socio => "socio",
            Vinculo::Participada => "participada",
            Vinculo::Administrador => "administrador",
            Vinculo::Familiar => "familiar",
            Vinculo::Grupo => "grupo",
            Vinculo::AdministradorGrupo => "administrador_grupo",
            Vinculo::ParticipadaIndirecta => "participada_indirecta",
            Vinculo::SociosComunes => "socios_comunes",
            Vinculo::EstablecimientoPermanente => "establecimiento_permanente",
        }
    }

    fn from_code(s: &str) -> Option<Vinculo> {
        Vinculo::ALL.into_iter().find(|v| v.code() == s)
    }

    /// Categoría de la nota de partes vinculadas de la memoria.
    fn categoria_memoria(self) -> &'static str {
        match self {
            Vinculo::Socio => "Socios",
            Vinculo::Administrador | Vinculo::AdministradorGrupo => {
                "Administradores (personal clave de la dirección)"
            }
            Vinculo::Participada | Vinculo::ParticipadaIndirecta => "Empresas participadas",
            Vinculo::Grupo => "Otras empresas del grupo",
            Vinculo::Familiar | Vinculo::SociosComunes | Vinculo::EstablecimientoPermanente => {
                "Otras partes vinculadas"
            }
        }
    }
}

/// Concepto de una operación: los once tipos del 232 (Orden HFP/816/2017, art. 3.1.f) y dos que
/// sólo van a la memoria.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Concepto {
    BienesTangibles,
    Intangibles,
    FondosPropios,
    CreditosDeuda,
    Financiacion,
    Servicios,
    RepartoCostes,
    Alquileres,
    Intereses,
    Trabajo,
    Otras,
    /// Dividendos pagados o cobrados: no son una operación del 232 (no tienen clave).
    /// TODO(VERIFY): criterio de la AEAT y la DGT de que el reparto de dividendos no se declara.
    Dividendos,
    /// Retribución del administrador por el cargo: fuera del régimen (art. 18.2.b LIS).
    RetribucionAdministrador,
}

impl Concepto {
    pub const DEL_232: [Concepto; 11] = [
        Concepto::BienesTangibles,
        Concepto::Intangibles,
        Concepto::FondosPropios,
        Concepto::CreditosDeuda,
        Concepto::Financiacion,
        Concepto::Servicios,
        Concepto::RepartoCostes,
        Concepto::Alquileres,
        Concepto::Intereses,
        Concepto::Trabajo,
        Concepto::Otras,
    ];

    /// Clave del tipo de operación del 232 (1 a 11).
    pub fn clave(self) -> Option<u8> {
        Concepto::DEL_232
            .iter()
            .position(|c| *c == self)
            .and_then(|i| u8::try_from(i + 1).ok())
    }

    pub fn from_clave(c: u8) -> Option<Concepto> {
        Concepto::DEL_232
            .get(usize::from(c).checked_sub(1)?)
            .copied()
    }

    pub fn label(self) -> &'static str {
        match self {
            Concepto::BienesTangibles => "Adquisición o transmisión de bienes tangibles",
            Concepto::Intangibles => "Adquisición, transmisión o cesión de uso de intangibles",
            Concepto::FondosPropios => {
                "Adquisición o transmisión de activos financieros representativos de fondos propios"
            }
            Concepto::CreditosDeuda => "Derechos de crédito y valores representativos de deuda",
            Concepto::Financiacion => "Operaciones financieras de deuda (sin intereses)",
            Concepto::Servicios => "Servicios",
            Concepto::RepartoCostes => "Acuerdos de reparto de costes",
            Concepto::Alquileres => "Alquileres y otras cesiones de uso de inmuebles",
            Concepto::Intereses => "Intereses de créditos y préstamos",
            Concepto::Trabajo => "Rendimientos del trabajo y pensiones",
            Concepto::Otras => "Otras operaciones",
            Concepto::Dividendos => "Dividendos",
            Concepto::RetribucionAdministrador => "Retribución del administrador por el cargo",
        }
    }

    fn code(self) -> &'static str {
        match self {
            Concepto::BienesTangibles => "bienes_tangibles",
            Concepto::Intangibles => "intangibles",
            Concepto::FondosPropios => "fondos_propios",
            Concepto::CreditosDeuda => "creditos_deuda",
            Concepto::Financiacion => "financiacion",
            Concepto::Servicios => "servicios",
            Concepto::RepartoCostes => "reparto_costes",
            Concepto::Alquileres => "alquileres",
            Concepto::Intereses => "intereses",
            Concepto::Trabajo => "trabajo",
            Concepto::Otras => "otras",
            Concepto::Dividendos => "dividendos",
            Concepto::RetribucionAdministrador => "retribucion_administrador",
        }
    }

    #[cfg(test)]
    fn from_code(s: &str) -> Option<Concepto> {
        Concepto::DEL_232
            .into_iter()
            .chain([Concepto::Dividendos, Concepto::RetribucionAdministrador])
            .find(|c| c.code() == s)
    }

    /// Por qué no va nunca al 232.
    fn excluida(self) -> Option<&'static str> {
        match self {
            Concepto::Dividendos => Some(
                "El reparto de dividendos no es una operación del 232 (no tiene clave): sólo va a \
                 la memoria",
            ),
            Concepto::RetribucionAdministrador => Some(
                "Retribución por el ejercicio del cargo de administrador: fuera del régimen de \
                 vinculadas (art. 18.2.b LIS)",
            ),
            _ => None,
        }
    }
}

/// Método de valoración (art. 18.4 LIS).
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Metodo {
    #[default]
    PrecioLibreComparable,
    CosteIncrementado,
    PrecioReventa,
    DistribucionResultado,
    MargenNetoOperacional,
    /// Otros métodos y técnicas generalmente aceptados (art. 18.4, último párrafo; art. 17 RIS).
    Otros,
}

impl Metodo {
    pub const ALL: [Metodo; 6] = [
        Metodo::PrecioLibreComparable,
        Metodo::CosteIncrementado,
        Metodo::PrecioReventa,
        Metodo::DistribucionResultado,
        Metodo::MargenNetoOperacional,
        Metodo::Otros,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Metodo::PrecioLibreComparable => "Precio libre comparable (art. 18.4.a LIS)",
            Metodo::CosteIncrementado => "Coste incrementado (art. 18.4.b LIS)",
            Metodo::PrecioReventa => "Precio de reventa (art. 18.4.c LIS)",
            Metodo::DistribucionResultado => "Distribución del resultado (art. 18.4.d LIS)",
            Metodo::MargenNetoOperacional => "Margen neto operacional (art. 18.4.e LIS)",
            Metodo::Otros => "Otros métodos y técnicas aceptados (art. 18.4 LIS)",
        }
    }

    fn code(self) -> &'static str {
        match self {
            Metodo::PrecioLibreComparable => "precio_libre_comparable",
            Metodo::CosteIncrementado => "coste_incrementado",
            Metodo::PrecioReventa => "precio_reventa",
            Metodo::DistribucionResultado => "distribucion_resultado",
            Metodo::MargenNetoOperacional => "margen_neto_operacional",
            Metodo::Otros => "otros",
        }
    }

    fn from_code(s: &str) -> Option<Metodo> {
        Metodo::ALL.into_iter().find(|m| m.code() == s)
    }
}

// ---------- Fichas ----------

/// Persona o entidad vinculada marcada a mano (o datos añadidos a un socio, administrador o
/// participada).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vinculada {
    #[serde(default)]
    pub id: i64,
    pub nif: String,
    pub name: String,
    #[serde(default)]
    pub legal_person: bool,
    /// Código de provincia (01-53) si reside en España.
    #[serde(default)]
    pub province: String,
    #[serde(default = "spain")]
    pub country: String,
    pub vinculo: Vinculo,
    /// «Cónyuge de la socia», «hermano del administrador»…
    #[serde(default)]
    pub detalle: String,
    /// Participación (millonésimas), si el vínculo va por ella.
    #[serde(default)]
    pub pct: Option<u32>,
    /// Del mismo grupo de consolidación fiscal, o AIE/UTE con sus miembros: sus operaciones no
    /// van al 232.
    #[serde(default)]
    pub grupo_fiscal: bool,
    /// Clave del 232 de las facturas que le emite la sociedad (6, servicios, si no).
    #[serde(default = "seis")]
    pub clave_emitidas: u8,
    /// Clave del 232 de sus facturas a la sociedad.
    #[serde(default = "seis")]
    pub clave_recibidas: u8,
    #[serde(default = "yes")]
    pub active: bool,
}

fn spain() -> String {
    "ES".into()
}
fn yes() -> bool {
    true
}
fn seis() -> u8 {
    6
}

/// Participada de la sociedad (ficha mínima).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Participada {
    #[serde(default)]
    pub id: i64,
    pub nif: String,
    pub name: String,
    #[serde(default)]
    pub province: String,
    #[serde(default = "spain")]
    pub country: String,
    /// Participación en millonésimas (100 % = 1.000.000).
    pub pct: u32,
    /// Fecha de adquisición (la primera, para la antigüedad del art. 21 LIS).
    pub acquired: Date,
    /// Coste contable de la participación.
    pub cost: Money,
    /// Subcuenta de la participación (2403, 2404, 250…).
    #[serde(default)]
    pub account: String,
    #[serde(default)]
    pub grupo_fiscal: bool,
    #[serde(default)]
    pub notes: String,
    #[serde(default = "yes")]
    pub active: bool,
}

/// Operación tecleada a mano.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperacionManual {
    #[serde(default)]
    pub id: i64,
    pub year: i32,
    pub nif: String,
    /// Clave del 232 (1 a 11).
    pub clave: u8,
    /// Ingreso (true) o pago para la sociedad.
    pub ingreso: bool,
    pub amount: Money,
    /// Operación específica (art. 18.3 LIS): transmisión de un negocio, de participaciones no
    /// cotizadas o de inmuebles, u operaciones sobre intangibles.
    #[serde(default)]
    pub especifica: bool,
    #[serde(default)]
    pub description: String,
}

/// Método y textos de un grupo de operaciones (año, vinculada, concepto, ingreso o pago).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Valoracion {
    pub year: i32,
    pub nif: String,
    pub concepto: Concepto,
    pub ingreso: bool,
    pub metodo: Metodo,
    #[serde(default)]
    pub descripcion: String,
    /// Comparables y valor o intervalo de mercado (art. 16.4.d RIS).
    #[serde(default)]
    pub justificacion: String,
}

/// Textos generales de la documentación del año.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Textos {
    pub year: i32,
    #[serde(default)]
    pub actividad: String,
    #[serde(default)]
    pub justificacion: String,
}

fn valid_province(p: &str) -> bool {
    p.len() == 2 && p.parse::<u8>().is_ok_and(|n| (1..=53).contains(&n))
}

/// NIF, país y provincia normalizados; con un NIF español válido si reside en España.
fn check_party(
    nif: &str,
    name: &str,
    country: &str,
    province: &str,
) -> Result<(String, String, String)> {
    let country = country.trim().to_uppercase();
    let province = province.trim().to_owned();
    if name.trim().is_empty() {
        return Err(other("pon el nombre o la razón social"));
    }
    if country == "ES" || country.is_empty() {
        let key = clave_nif(nif);
        Nif::parse(&key).map_err(|e| other(format!("NIF: {e}")))?;
        if !province.is_empty() && !valid_province(&province) {
            return Err(other(
                "la provincia es el código de dos cifras del domicilio (01 a 53)",
            ));
        }
        Ok((key, "ES".into(), province))
    } else {
        if country.len() != 2 {
            return Err(other("el país es un código de dos letras (FR, PT…)"));
        }
        let key = clave_nif(nif);
        if key.is_empty() {
            return Err(other("pon el identificador fiscal de su país"));
        }
        Ok((key, country, String::new()))
    }
}

fn audit_append(
    conn: &Connection,
    now: OffsetDateTime,
    action: &str,
    target: &str,
    detail: serde_json::Value,
) -> Result<()> {
    audit::append(conn, &now_str(now), Actor::User, action, target, &detail)?;
    Ok(())
}

const VINC_COLS: &str = "id, nif, name, legal_person, province, country, vinculo, detalle, pct, \
                         grupo_fiscal, clave_emitidas, clave_recibidas, active";

fn vinculada_of(
    r: &taxpain_vault::rusqlite::Row<'_>,
) -> taxpain_vault::rusqlite::Result<Vinculada> {
    let vinculo: String = r.get(6)?;
    Ok(Vinculada {
        id: r.get(0)?,
        nif: r.get(1)?,
        name: r.get(2)?,
        legal_person: r.get(3)?,
        province: r.get(4)?,
        country: r.get(5)?,
        vinculo: Vinculo::from_code(&vinculo).unwrap_or(Vinculo::Familiar),
        detalle: r.get(7)?,
        pct: r.get(8)?,
        grupo_fiscal: r.get(9)?,
        clave_emitidas: r.get(10)?,
        clave_recibidas: r.get(11)?,
        active: r.get(12)?,
    })
}

/// Las fichas de vinculadas tecleadas.
pub fn vinculadas(v: &Vault) -> Result<Vec<Vinculada>> {
    let mut st = v.connection().prepare(&format!(
        "SELECT {VINC_COLS} FROM vinculada ORDER BY name, id"
    ))?;
    let rows = st.query_map([], vinculada_of)?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// La ficha de vinculada de un NIF (para la ficha del cliente o proveedor).
pub fn vinculada_de(v: &Vault, nif: &str) -> Result<Option<Vinculada>> {
    Ok(v.connection()
        .query_row(
            &format!("SELECT {VINC_COLS} FROM vinculada WHERE nif = ?1"),
            [clave_nif(nif)],
            vinculada_of,
        )
        .optional()?)
}

/// Da de alta (id 0) o cambia una vinculada. Devuelve su id.
pub fn save_vinculada(v: &Vault, x: &Vinculada, now: OffsetDateTime) -> Result<i64> {
    let (nif, country, province) = check_party(&x.nif, &x.name, &x.country, &x.province)?;
    if country == "ES" {
        let fisica = Nif::parse(&nif).is_ok_and(|n| n.is_persona_fisica());
        if fisica == x.legal_person {
            return Err(other(if x.legal_person {
                "ese NIF es de una persona física: desmarca «entidad»"
            } else {
                "ese NIF es de una sociedad o entidad: marca «entidad»"
            }));
        }
    }
    if x.pct.is_some_and(|p| p == 0 || p > 1_000_000) {
        return Err(other("la participación va de 0,0001 % a 100 %"));
    }
    for c in [x.clave_emitidas, x.clave_recibidas] {
        if Concepto::from_clave(c).is_none() {
            return Err(other("la clave del tipo de operación va del 1 al 11"));
        }
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let p = params![
        nif,
        x.name.trim(),
        x.legal_person,
        province,
        country,
        x.vinculo.code(),
        x.detalle.trim(),
        x.pct,
        x.grupo_fiscal,
        x.clave_emitidas,
        x.clave_recibidas,
        x.active,
    ];
    let id =
        if x.id == 0 {
            let exists: Option<i64> = tx
                .query_row("SELECT id FROM vinculada WHERE nif = ?1", [&nif], |r| {
                    r.get(0)
                })
                .optional()?;
            if exists.is_some() {
                return Err(other("ese NIF ya está entre las vinculadas: edítalo"));
            }
            tx.execute(
            "INSERT INTO vinculada(nif, name, legal_person, province, country, vinculo, detalle,
               pct, grupo_fiscal, clave_emitidas, clave_recibidas, active, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7], p[8], p[9], p[10], p[11],
                now_str(now)
            ],
        )?;
            tx.last_insert_rowid()
        } else {
            let n = tx.execute(
                "UPDATE vinculada SET nif = ?1, name = ?2, legal_person = ?3, province = ?4,
               country = ?5, vinculo = ?6, detalle = ?7, pct = ?8, grupo_fiscal = ?9,
               clave_emitidas = ?10, clave_recibidas = ?11, active = ?12 WHERE id = ?13",
                params![
                    p[0], p[1], p[2], p[3], p[4], p[5], p[6], p[7], p[8], p[9], p[10], p[11], x.id
                ],
            )?;
            if n == 0 {
                return Err(AppError::NotFound);
            }
            x.id
        };
    // Los datos personales no van a la auditoría, sólo el cambio.
    audit_append(
        &tx,
        now,
        "vinculada.save",
        &id.to_string(),
        serde_json::json!({ "vinculo": x.vinculo.code(), "grupo_fiscal": x.grupo_fiscal }),
    )?;
    tx.commit()?;
    Ok(id)
}

/// Quita una vinculada de la lista (sus operaciones tecleadas y cuentas asignadas se quedan).
pub fn delete_vinculada(v: &Vault, id: i64, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    if tx.execute("DELETE FROM vinculada WHERE id = ?1", [id])? == 0 {
        return Err(AppError::NotFound);
    }
    audit_append(
        &tx,
        now,
        "vinculada.delete",
        &id.to_string(),
        serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

const PART_COLS: &str = "id, nif, name, province, country, pct, acquired, cost, account, \
                         grupo_fiscal, notes, active";

fn participada_of(
    r: &taxpain_vault::rusqlite::Row<'_>,
) -> taxpain_vault::rusqlite::Result<Participada> {
    let acquired: String = r.get(6)?;
    Ok(Participada {
        id: r.get(0)?,
        nif: r.get(1)?,
        name: r.get(2)?,
        province: r.get(3)?,
        country: r.get(4)?,
        pct: r.get(5)?,
        acquired: parse_date(&acquired).unwrap_or(Date::MIN),
        cost: Money::from_cents(r.get(7)?),
        account: r.get(8)?,
        grupo_fiscal: r.get(9)?,
        notes: r.get(10)?,
        active: r.get(11)?,
    })
}

fn parse_date(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]")).map_err(other)
}

pub fn participadas(v: &Vault) -> Result<Vec<Participada>> {
    let mut st = v.connection().prepare(&format!(
        "SELECT {PART_COLS} FROM participada ORDER BY active DESC, pct DESC, name"
    ))?;
    let rows = st.query_map([], participada_of)?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn save_participada(v: &Vault, x: &Participada, now: OffsetDateTime) -> Result<i64> {
    let (nif, country, province) = check_party(&x.nif, &x.name, &x.country, &x.province)?;
    if country == "ES" && Nif::parse(&nif).is_ok_and(|n| n.is_persona_fisica()) {
        return Err(other(
            "una participada es una sociedad o entidad, no una persona",
        ));
    }
    if x.pct == 0 || x.pct > 1_000_000 {
        return Err(other("la participación va de 0,0001 % a 100 %"));
    }
    if x.cost.is_negative() {
        return Err(other("el coste no puede ser negativo"));
    }
    let account = x.account.trim().to_owned();
    if !account.is_empty() && !account.chars().all(|c| c.is_ascii_digit()) {
        return Err(other("la subcuenta sólo lleva cifras (2403, 2404…)"));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let id = if x.id == 0 {
        tx.execute(
            "INSERT INTO participada(nif, name, province, country, pct, acquired, cost, account,
               grupo_fiscal, notes, active, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                nif,
                x.name.trim(),
                province,
                country,
                x.pct,
                x.acquired.to_string(),
                x.cost.cents(),
                account,
                x.grupo_fiscal,
                x.notes.trim(),
                x.active,
                now_str(now)
            ],
        )
        .map_err(|e| {
            if e.to_string().contains("UNIQUE") {
                other("esa participada ya está en la lista: edítala")
            } else {
                e.into()
            }
        })?;
        tx.last_insert_rowid()
    } else {
        let n = tx.execute(
            "UPDATE participada SET nif = ?1, name = ?2, province = ?3, country = ?4, pct = ?5,
               acquired = ?6, cost = ?7, account = ?8, grupo_fiscal = ?9, notes = ?10,
               active = ?11 WHERE id = ?12",
            params![
                nif,
                x.name.trim(),
                province,
                country,
                x.pct,
                x.acquired.to_string(),
                x.cost.cents(),
                account,
                x.grupo_fiscal,
                x.notes.trim(),
                x.active,
                x.id
            ],
        )?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        x.id
    };
    audit_append(
        &tx,
        now,
        "participada.save",
        &id.to_string(),
        serde_json::json!({ "pct": x.pct, "cost": x.cost.cents() }),
    )?;
    tx.commit()?;
    Ok(id)
}

pub fn delete_participada(v: &Vault, id: i64, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    if tx.execute("DELETE FROM participada WHERE id = ?1", [id])? == 0 {
        return Err(AppError::NotFound);
    }
    audit_append(
        &tx,
        now,
        "participada.delete",
        &id.to_string(),
        serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

/// Asigna una subcuenta a una vinculada (o la deja sin asignar con `nif: None`).
pub fn set_cuenta(v: &Vault, account: &str, nif: Option<&str>, now: OffsetDateTime) -> Result<()> {
    let account = account.trim();
    if account.len() < 3 || !account.chars().all(|c| c.is_ascii_digit()) {
        return Err(other("la subcuenta sólo lleva cifras"));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let nif = nif.map(clave_nif).filter(|n| !n.is_empty());
    match &nif {
        Some(n) => {
            tx.execute(
                "INSERT INTO vinculada_cuenta(account, nif, created_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(account) DO UPDATE SET nif = excluded.nif",
                params![account, n, now_str(now)],
            )?;
        }
        None => {
            tx.execute("DELETE FROM vinculada_cuenta WHERE account = ?1", [account])?;
        }
    }
    audit_append(
        &tx,
        now,
        "vinculada.cuenta",
        account,
        serde_json::json!({ "asignada": nif.is_some() }),
    )?;
    tx.commit()?;
    Ok(())
}

pub fn save_operacion(v: &Vault, x: &OperacionManual, now: OffsetDateTime) -> Result<i64> {
    if Concepto::from_clave(x.clave).is_none() {
        return Err(other("la clave del tipo de operación va del 1 al 11"));
    }
    if x.amount.cents() <= 0 {
        return Err(other("el importe tiene que ser mayor que cero"));
    }
    let nif = clave_nif(&x.nif);
    if !personas(v.connection())?.contains_key(&nif) {
        return Err(other(
            "ese NIF no está entre las vinculadas: añádelo antes a la lista",
        ));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let id = if x.id == 0 {
        tx.execute(
            "INSERT INTO vinculada_operacion(year, nif, clave, ingreso, amount, especifica,
               description, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                x.year,
                nif,
                x.clave,
                x.ingreso,
                x.amount.cents(),
                x.especifica,
                x.description.trim(),
                now_str(now)
            ],
        )?;
        tx.last_insert_rowid()
    } else {
        let n = tx.execute(
            "UPDATE vinculada_operacion SET year = ?1, nif = ?2, clave = ?3, ingreso = ?4,
               amount = ?5, especifica = ?6, description = ?7 WHERE id = ?8",
            params![
                x.year,
                nif,
                x.clave,
                x.ingreso,
                x.amount.cents(),
                x.especifica,
                x.description.trim(),
                x.id
            ],
        )?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        x.id
    };
    audit_append(
        &tx,
        now,
        "vinculada.operacion.save",
        &id.to_string(),
        serde_json::json!({ "year": x.year, "clave": x.clave, "amount": x.amount.cents() }),
    )?;
    tx.commit()?;
    Ok(id)
}

pub fn delete_operacion(v: &Vault, id: i64, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    if tx.execute("DELETE FROM vinculada_operacion WHERE id = ?1", [id])? == 0 {
        return Err(AppError::NotFound);
    }
    audit_append(
        &tx,
        now,
        "vinculada.operacion.delete",
        &id.to_string(),
        serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

fn operaciones_manuales(conn: &Connection, year: i32) -> Result<Vec<OperacionManual>> {
    let mut st = conn.prepare(
        "SELECT id, year, nif, clave, ingreso, amount, especifica, description
           FROM vinculada_operacion WHERE year = ?1 ORDER BY id",
    )?;
    let rows = st.query_map([year], |r| {
        Ok(OperacionManual {
            id: r.get(0)?,
            year: r.get(1)?,
            nif: r.get(2)?,
            clave: r.get(3)?,
            ingreso: r.get(4)?,
            amount: Money::from_cents(r.get(5)?),
            especifica: r.get(6)?,
            description: r.get(7)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn save_valoracion(v: &Vault, x: &Valoracion, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO vinculada_valoracion(year, nif, concepto, ingreso, metodo, descripcion,
           justificacion, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(year, nif, concepto, ingreso) DO UPDATE SET metodo = excluded.metodo,
           descripcion = excluded.descripcion, justificacion = excluded.justificacion,
           updated_at = excluded.updated_at",
        params![
            x.year,
            clave_nif(&x.nif),
            x.concepto.code(),
            x.ingreso,
            x.metodo.code(),
            x.descripcion.trim(),
            x.justificacion.trim(),
            now_str(now)
        ],
    )?;
    audit_append(
        &tx,
        now,
        "vinculada.valoracion",
        &x.year.to_string(),
        serde_json::json!({ "concepto": x.concepto.code(), "metodo": x.metodo.code() }),
    )?;
    tx.commit()?;
    Ok(())
}

pub fn textos(v: &Vault, year: i32) -> Result<Textos> {
    Ok(v.connection()
        .query_row(
            "SELECT actividad, justificacion FROM vinculadas_documentacion WHERE year = ?1",
            [year],
            |r| {
                Ok(Textos {
                    year,
                    actividad: r.get(0)?,
                    justificacion: r.get(1)?,
                })
            },
        )
        .optional()?
        .unwrap_or(Textos {
            year,
            ..Textos::default()
        }))
}

pub fn save_textos(v: &Vault, x: &Textos, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO vinculadas_documentacion(year, actividad, justificacion, updated_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(year) DO UPDATE SET actividad = excluded.actividad,
           justificacion = excluded.justificacion, updated_at = excluded.updated_at",
        params![
            x.year,
            x.actividad.trim(),
            x.justificacion.trim(),
            now_str(now)
        ],
    )?;
    audit_append(
        &tx,
        now,
        "vinculadas.documentacion",
        &x.year.to_string(),
        serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

// ---------- Registro del año ----------

/// Persona o entidad vinculada, con lo que se sabe de ella.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Persona {
    /// NIF normalizado.
    pub nif: String,
    pub name: String,
    pub legal_person: bool,
    pub province: String,
    pub country: String,
    /// Vínculos (el primero es el principal, el que va al 232).
    pub vinculos: Vec<Vinculo>,
    pub detalle: String,
    pub pct: Option<u32>,
    pub grupo_fiscal: bool,
    /// Ficha tecleada (`vinculada`), si la tiene.
    pub ficha: Option<Vinculada>,
    /// Socio (con su participación) de la ficha de socios.
    pub socio: bool,
    /// Administrador en Nóminas.
    pub administrador: bool,
    /// Participada de la ficha de participadas.
    pub participada: bool,
    /// Subcuentas asignadas.
    pub cuentas: Vec<String>,
    /// Lo que cuenta para los umbrales (sin las operaciones excluidas).
    pub total: Money,
    pub supera_umbral: bool,
}

impl Persona {
    pub fn vinculo(&self) -> Vinculo {
        self.vinculos.first().copied().unwrap_or(Vinculo::Familiar)
    }

    /// «Física» o «Jurídica» (Orden HFP/816/2017, art. 3.1.b).
    pub fn condicion(&self) -> &'static str {
        if self.legal_person {
            "Jurídica"
        } else {
            "Física"
        }
    }

    /// Provincia si reside en España, país si no (art. 3.1.e).
    pub fn residencia(&self) -> String {
        if self.country == "ES" {
            if self.province.is_empty() {
                "ES".into()
            } else {
                self.province.clone()
            }
        } else {
            self.country.clone()
        }
    }
}

/// Grupo de operaciones con una vinculada: un registro del 232 si se declara.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Operacion {
    pub nif: String,
    pub name: String,
    pub vinculo: Vinculo,
    pub concepto: Concepto,
    /// Clave del 232 (1 a 11), o `None` si no va nunca.
    pub clave: Option<u8>,
    /// Ingreso (true) o pago para la sociedad.
    pub ingreso: bool,
    /// Importe sin IVA.
    pub importe: Money,
    pub metodo: Metodo,
    /// El método lo ha elegido el usuario (si no, el de por defecto).
    pub metodo_elegido: bool,
    pub especifica: bool,
    /// De dónde sale: «3 facturas emitidas», «subcuenta 5510001»…
    pub origen: Vec<String>,
    /// Por qué no va al 232 aunque se pasen los umbrales.
    pub excluida: Option<String>,
    /// Va en el 232 (por alguno de los umbrales).
    pub en_232: bool,
    pub motivo: Option<String>,
    pub descripcion: String,
    pub justificacion: String,
}

/// Subcuenta con movimientos que puede ser de una vinculada.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Cuenta {
    pub account: String,
    pub name: String,
    pub debit: Money,
    pub credit: Money,
    pub nif: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Registro {
    pub year: i32,
    pub start: Date,
    pub end: Date,
    pub company: String,
    pub company_nif: String,
    /// Cifra de negocios del ejercicio (regla del 50 % y contenido simplificado).
    pub incn: Option<Money>,
    pub incn_fuente: String,
    pub personas: Vec<Persona>,
    pub operaciones: Vec<Operacion>,
    pub manuales: Vec<OperacionManual>,
    /// Subcuentas de los grupos que suelen ser de vinculadas, con o sin asignar.
    pub cuentas: Vec<Cuenta>,
    /// Hay que presentar el 232.
    pub obligado: bool,
    /// Por qué (umbrales superados).
    pub motivos: Vec<String>,
    pub avisos: Vec<String>,
    /// Mes en que se presenta el 232 («noviembre de 2027»).
    pub plazo: String,
    /// La documentación puede tener el contenido simplificado (INCN < 45 M€).
    pub simplificada: bool,
    /// Personas con las que hay que tener documentación (más de 250.000 €) u operaciones
    /// específicas.
    pub documentar: Vec<String>,
    pub textos: Textos,
}

/// Datos de una persona o entidad para darla de alta en la lista.
struct Alta {
    name: String,
    legal: bool,
    province: String,
    country: String,
}

/// Añade (o completa) una vinculada de la lista con un vínculo más.
fn alta<'a>(
    out: &'a mut BTreeMap<String, Persona>,
    nif: &str,
    a: Alta,
    vinculo: Vinculo,
    pct: Option<u32>,
) -> Option<&'a mut Persona> {
    let key = clave_nif(nif);
    if key.is_empty() {
        return None;
    }
    let p = out.entry(key.clone()).or_insert_with(|| Persona {
        nif: key,
        name: a.name,
        legal_person: a.legal,
        province: a.province,
        country: a.country,
        vinculos: vec![],
        detalle: String::new(),
        pct: None,
        grupo_fiscal: false,
        ficha: None,
        socio: false,
        administrador: false,
        participada: false,
        cuentas: vec![],
        total: Money::ZERO,
        supera_umbral: false,
    });
    if !p.vinculos.contains(&vinculo) {
        p.vinculos.push(vinculo);
    }
    if pct.is_some() {
        p.pct = pct;
    }
    match vinculo {
        Vinculo::Socio => p.socio = true,
        Vinculo::Administrador => p.administrador = true,
        Vinculo::Participada => p.participada = true,
        _ => {}
    }
    Some(p)
}

/// Vinculadas de hoy, por NIF normalizado.
fn personas(conn: &Connection) -> Result<BTreeMap<String, Persona>> {
    let mut out: BTreeMap<String, Persona> = BTreeMap::new();
    {
        let mut st = conn.prepare(
            "SELECT nif, name, legal_person, province, country, pct FROM shareholder
              WHERE active = 1 AND pct >= ?1",
        )?;
        let rows = st.query_map([PCT_VINCULADO], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, bool>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, u32>(5)?,
            ))
        })?;
        for row in rows {
            let (nif, name, legal, province, country, pct) = row?;
            let a = Alta {
                name,
                legal,
                province,
                country,
            };
            alta(&mut out, &nif, a, Vinculo::Socio, Some(pct));
        }
    }
    {
        let mut st =
            conn.prepare("SELECT nif, name, province FROM employee WHERE administrator = 1")?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (nif, name, province) = row?;
            let legal = Nif::parse(&nif).is_ok_and(|n| !n.is_persona_fisica());
            let a = Alta {
                name,
                legal,
                province,
                country: "ES".into(),
            };
            alta(&mut out, &nif, a, Vinculo::Administrador, None);
        }
    }
    {
        let mut st = conn.prepare(
            "SELECT nif, name, province, country, pct, grupo_fiscal FROM participada
              WHERE active = 1 AND pct >= ?1",
        )?;
        let rows = st.query_map([PCT_VINCULADO], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, u32>(4)?,
                r.get::<_, bool>(5)?,
            ))
        })?;
        for row in rows {
            let (nif, name, province, country, pct, grupo) = row?;
            let a = Alta {
                name,
                legal: true,
                province,
                country,
            };
            if let Some(p) = alta(&mut out, &nif, a, Vinculo::Participada, Some(pct)) {
                p.grupo_fiscal |= grupo;
            }
        }
    }
    {
        let mut st = conn.prepare(&format!(
            "SELECT {VINC_COLS} FROM vinculada WHERE active = 1 ORDER BY id"
        ))?;
        let rows = st.query_map([], vinculada_of)?;
        for row in rows {
            let f = row?;
            let a = Alta {
                name: f.name.clone(),
                legal: f.legal_person,
                province: f.province.clone(),
                country: f.country.clone(),
            };
            if let Some(p) = alta(&mut out, &f.nif.clone(), a, f.vinculo, f.pct) {
                // La ficha tecleada manda en los datos.
                p.name.clone_from(&f.name);
                p.legal_person = f.legal_person;
                if !f.province.is_empty() {
                    p.province.clone_from(&f.province);
                }
                p.country.clone_from(&f.country);
                p.detalle.clone_from(&f.detalle);
                p.grupo_fiscal |= f.grupo_fiscal;
                p.ficha = Some(f);
            }
        }
    }
    {
        let mut st = conn.prepare("SELECT account, nif FROM vinculada_cuenta ORDER BY account")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (account, nif) = row?;
            if let Some(p) = out.get_mut(&nif) {
                p.cuentas.push(account);
            }
        }
    }
    for p in out.values_mut() {
        p.vinculos.sort();
    }
    Ok(out)
}

/// Lo que se va sumando de un grupo de operaciones antes de valorarlo.
#[derive(Default)]
struct Suma {
    importe: Money,
    origen: Vec<String>,
}

type ClaveOp = (String, Concepto, bool, bool);

fn sumar(map: &mut BTreeMap<ClaveOp, Suma>, k: ClaveOp, importe: Money, origen: String) {
    let s = map.entry(k).or_default();
    s.importe += importe;
    if !s.origen.contains(&origen) {
        s.origen.push(origen);
    }
}

fn plural(n: i64, uno: &str, varios: &str) -> String {
    if n == 1 {
        format!("1 {uno}")
    } else {
        format!("{n} {varios}")
    }
}

/// Prefijos de las subcuentas que se proponen para asignar a una vinculada.
pub const CUENTAS_CANDIDATAS: &[&str] = &[
    "16", "171", "24", "25", "51", "521", "53", "54", "551", "552", "553", "621", "661", "662",
    "752", "760", "761", "762",
];

/// Cuentas corrientes con socios, administradores y otras vinculadas (ingresos y pagos por
/// separado, sin compensar: Orden HFP/816/2017, art. 3.1).
fn es_cuenta_corriente(a: &str) -> bool {
    ["551", "552", "553"].iter().any(|p| a.starts_with(p))
}

/// Concepto, sentido e importe de los movimientos del año de una subcuenta asignada. `None` si
/// no se sabe qué es (va a «Otras»).
///
/// TODO(VERIFY): importe de las operaciones de financiación en el 232. Aquí: el principal
/// dispuesto en el año (cargos en los préstamos concedidos, abonos en los recibidos) y, en las
/// cuentas corrientes, cargos como pagos y abonos como ingresos, sin compensar.
fn clasificar(account: &str, debit: Money, credit: Money) -> Vec<(Concepto, bool, Money)> {
    let a = account;
    let starts = |ps: &[&str]| ps.iter().any(|p| a.starts_with(p));
    if es_cuenta_corriente(a) {
        return vec![
            (Concepto::Financiacion, false, debit),
            (Concepto::Financiacion, true, credit),
        ];
    }
    if starts(&["24", "25", "53", "54"]) {
        // Préstamos y créditos concedidos (y participaciones: su compraventa es la clave 3, que
        // se teclea a mano).
        return vec![(Concepto::Financiacion, false, debit)];
    }
    if starts(&["16", "17", "51", "52"]) {
        return vec![(Concepto::Financiacion, true, credit)];
    }
    if starts(&["760"]) {
        return vec![(Concepto::Dividendos, true, credit - debit)];
    }
    if starts(&["761", "762", "763", "769"]) {
        return vec![(Concepto::Intereses, true, credit - debit)];
    }
    if starts(&["661", "662", "663", "669"]) {
        return vec![(Concepto::Intereses, false, debit - credit)];
    }
    if starts(&["752"]) {
        return vec![(Concepto::Alquileres, true, credit - debit)];
    }
    if starts(&["621"]) {
        return vec![(Concepto::Alquileres, false, debit - credit)];
    }
    if starts(&["640", "641", "642", "649"]) {
        return vec![(Concepto::Trabajo, false, debit - credit)];
    }
    if starts(&["700", "701", "702", "703", "704"]) {
        return vec![(Concepto::BienesTangibles, true, credit - debit)];
    }
    if starts(&["60"]) {
        return vec![(Concepto::BienesTangibles, false, debit - credit)];
    }
    if starts(&["705", "75"]) {
        return vec![(Concepto::Servicios, true, credit - debit)];
    }
    if starts(&["62"]) {
        return vec![(Concepto::Servicios, false, debit - credit)];
    }
    if a.starts_with('7') {
        return vec![(Concepto::Otras, true, credit - debit)];
    }
    if a.starts_with('6') {
        return vec![(Concepto::Otras, false, debit - credit)];
    }
    vec![
        (Concepto::Otras, false, debit),
        (Concepto::Otras, true, credit),
    ]
}

/// Movimientos del ejercicio por subcuenta (sin apertura, regularización ni cierre, y sin los
/// asientos de nóminas y dividendos, que se cuentan por su lado).
fn movimientos(conn: &Connection, year: i32) -> Result<Vec<(String, Money, Money)>> {
    let mut st = conn.prepare(
        "SELECT l.account, COALESCE(SUM(l.debit), 0), COALESCE(SUM(l.credit), 0)
           FROM journal_line l JOIN journal_entry e ON e.id = l.entry_id
          WHERE e.fiscal_year = ?1
            AND e.origin_kind NOT IN ('Apertura', 'Regularizacion', 'Cierre', 'Nomina', 'Dividendo')
          GROUP BY l.account ORDER BY l.account",
    )?;
    let rows = st.query_map([year], |r| {
        Ok((
            r.get::<_, String>(0)?,
            Money::from_cents(r.get(1)?),
            Money::from_cents(r.get(2)?),
        ))
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Cifra de negocios del ejercicio: la del 200 presentado o la contabilidad (la misma regla que
/// el tipo de retención del administrador) y, si no, las bases de las facturas emitidas.
fn cifra_de_negocios(v: &Vault, start: Date, end: Date) -> (Option<Money>, String) {
    if let Some(next) = end.next_day()
        && let Ok(t) = crate::payroll::director_turnover(v, next)
        && let Some(m) = t.turnover
        && t.small.is_some()
    {
        return (Some(m), t.source.label().to_owned());
    }
    let cents: Option<i64> = v
        .connection()
        .query_row(
            "SELECT SUM(base - COALESCE(replaced_base, 0)) FROM invoice_issued WHERE status = 'emitida'
              AND issue_date BETWEEN ?1 AND ?2",
            [start.to_string(), end.to_string()],
            |r| r.get(0),
        )
        .ok()
        .flatten();
    match cents {
        Some(c) if c > 0 => (
            Some(Money::from_cents(c)),
            "las facturas emitidas en TaxPain".into(),
        ),
        _ => (None, String::new()),
    }
}

/// Mes de presentación del 232: el siguiente a los diez meses posteriores al cierre (Orden
/// HFP/816/2017, art. 4).
pub fn plazo(end: Date) -> String {
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
    // Un cierre a fin de mes: el mes siguiente a los diez es el undécimo después del cierre.
    let m0 = i32::from(u8::from(end.month())) - 1 + 11;
    let year = end.year() + m0 / 12;
    let mes = MESES
        .get(usize::try_from(m0 % 12).unwrap_or(0))
        .copied()
        .unwrap_or("");
    format!("{mes} de {year}")
}

/// Préstamo de Contabilidad → Préstamos con una vinculada, en un ejercicio.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PrestamoVinculado {
    /// NIF normalizado de la vinculada.
    pub nif: String,
    pub name: String,
    pub vinculo: Vinculo,
    pub ejercicio: crate::prestamos::DelEjercicio,
}

/// Vínculo con que entra en la lista una contraparte marcada como vinculada en la ficha del
/// préstamo que no está entre las vinculadas.
///
/// TODO(VERIFY): «asociada» (NECA 13.ª, desde el 20 %) no siempre es vinculada del art. 18.2
/// LIS (25 %): si el usuario la marca, se toma como participada.
fn vinculo_de_prestamo(c: &taxpain_core::accounting::prestamo::Contraparte) -> Vinculo {
    use taxpain_core::accounting::prestamo::{OrigenContraparte, Vinculo as VinculoPrestamo};
    match c.vinculo {
        VinculoPrestamo::Grupo => Vinculo::Grupo,
        VinculoPrestamo::Asociada => Vinculo::Participada,
        _ if c.origen == OrigenContraparte::Socio => Vinculo::Socio,
        _ => Vinculo::Familiar,
    }
}

/// Préstamos con vinculadas del ejercicio: los marcados como vinculados en su ficha (su
/// contraparte se añade a `per` si no estaba) y los que tienen por contraparte una vinculada.
fn prestamos_con(
    conn: &Connection,
    per: &mut BTreeMap<String, Persona>,
    start: Date,
    end: Date,
    avisos: &mut Vec<String>,
) -> Result<Vec<PrestamoVinculado>> {
    let mut out = Vec::new();
    for d in crate::prestamos::del_ejercicio(conn, start, end)? {
        let c = &d.prestamo.contraparte;
        let key = clave_nif(&d.nif);
        if key.is_empty() {
            if c.vinculada() {
                avisos.push(format!(
                    "El préstamo «{}» está marcado como vinculado, pero su contraparte no tiene \
                     NIF: no se cuenta. Pon el NIF en la ficha del préstamo.",
                    d.prestamo.nombre
                ));
            }
            continue;
        }
        if !per.contains_key(&key) {
            if !c.vinculada() {
                continue;
            }
            let vinculo = vinculo_de_prestamo(c);
            let a = Alta {
                name: c.nombre.trim().to_owned(),
                legal: !c.persona_fisica,
                province: c.provincia.trim().to_owned(),
                country: "ES".into(),
            };
            alta(per, &key, a, vinculo, None);
            avisos.push(format!(
                "{} sale como vinculada porque el préstamo «{}» lo dice ({}): dale una ficha \
                 aquí si el vínculo es otro.",
                c.nombre.trim(),
                d.prestamo.nombre,
                vinculo.label()
            ));
        } else if !c.vinculada() {
            avisos.push(format!(
                "El préstamo «{}» no está marcado como vinculado, pero {} es una parte \
                 vinculada: se cuenta aquí. Márcalo en su ficha (Contabilidad → Préstamos) para \
                 que proponga las subcuentas y los avisos de vinculadas.",
                d.prestamo.nombre, per[&key].name
            ));
        }
        let p = &per[&key];
        out.push(PrestamoVinculado {
            name: p.name.clone(),
            vinculo: p.vinculo(),
            nif: key,
            ejercicio: d,
        });
    }
    Ok(out)
}

/// Préstamos con vinculadas del ejercicio `year` (para la memoria).
pub fn prestamos_vinculados(v: &Vault, year: i32) -> Result<Vec<PrestamoVinculado>> {
    let conn = v.connection();
    let fy = crate::accounting::fiscal_year(v, year)?;
    let mut per = personas(conn)?;
    prestamos_con(conn, &mut per, fy.start, fy.end, &mut Vec::new())
}

/// Registro de operaciones vinculadas del ejercicio `year`.
pub fn registro(v: &Vault, year: i32) -> Result<Registro> {
    let info = crate::companies::info(v)?;
    let company_nif = info.nif.clone().unwrap_or_default();
    if Nif::parse(&company_nif).is_ok_and(|n| n.is_persona_fisica()) {
        return Err(other(
            "las operaciones vinculadas del 232 son de las sociedades (Impuesto sobre Sociedades)",
        ));
    }
    let conn = v.connection();
    let fy = crate::accounting::fiscal_year(v, year)?;
    let (start, end) = (fy.start, fy.end);
    let mut per = personas(conn)?;
    let mut sumas: BTreeMap<ClaveOp, Suma> = BTreeMap::new();
    let mut avisos = Vec::new();
    let (s, e) = (start.to_string(), end.to_string());
    // Préstamos (antes que nada: una contraparte marcada como vinculada entra en la lista y se
    // le cuentan también sus facturas).
    let prestamos = prestamos_con(conn, &mut per, start, end, &mut avisos)?;
    let mut de_prestamos: std::collections::BTreeSet<(String, String)> = Default::default();
    for pv in &prestamos {
        let d = &pv.ejercicio;
        let p = &d.prestamo;
        let concedido = p.sentido == taxpain_core::accounting::prestamo::Sentido::Concedido;
        let origen = format!("préstamo «{}»", p.nombre);
        if !d.dispuesto.is_zero() {
            sumar(
                &mut sumas,
                (pv.nif.clone(), Concepto::Financiacion, !concedido, false),
                d.dispuesto,
                origen.clone(),
            );
        }
        if !d.intereses.is_zero() {
            sumar(
                &mut sumas,
                (pv.nif.clone(), Concepto::Intereses, concedido, false),
                d.intereses,
                origen,
            );
        }
        let k = &p.cuentas;
        for a in [&k.largo, &k.corto, &k.devengo, &k.intereses] {
            de_prestamos.insert((a.as_str().to_owned(), pv.nif.clone()));
        }
        if let Some(a) = crate::prestamos::aviso_tipo_vinculada(p) {
            avisos.push(format!("Préstamo «{}»: {a}", p.nombre));
        }
    }

    // Facturas emitidas.
    {
        let mut st = conn.prepare(
            "SELECT recipient_tax_id, SUM(base - COALESCE(replaced_base, 0)), COUNT(*) FROM invoice_issued
              WHERE status = 'emitida' AND issue_date BETWEEN ?1 AND ?2
                AND recipient_tax_id IS NOT NULL
              GROUP BY recipient_tax_id",
        )?;
        let rows = st.query_map([&s, &e], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        for row in rows {
            let (nif, base, n) = row?;
            let key = clave_nif(&nif);
            let Some(p) = per.get(&key) else { continue };
            let clave = p.ficha.as_ref().map_or(6, |f| f.clave_emitidas);
            let c = Concepto::from_clave(clave).unwrap_or(Concepto::Servicios);
            sumar(
                &mut sumas,
                (key, c, true, false),
                Money::from_cents(base),
                plural(n, "factura emitida", "facturas emitidas"),
            );
        }
    }
    // Facturas recibidas revisadas.
    {
        let mut st = conn.prepare(
            "SELECT supplier_nif, nature, capital_good, SUM(base), COUNT(*) FROM invoice_received
              WHERE status = 'revisada' AND issue_date BETWEEN ?1 AND ?2
                AND supplier_nif IS NOT NULL AND base IS NOT NULL
              GROUP BY supplier_nif, nature, capital_good",
        )?;
        let rows = st.query_map([&s, &e], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, bool>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?;
        for row in rows {
            let (nif, nature, capital, base, n) = row?;
            let key = clave_nif(&nif);
            let Some(p) = per.get(&key) else { continue };
            let c = if nature == "alquiler" {
                Concepto::Alquileres
            } else if capital {
                Concepto::BienesTangibles
            } else {
                let clave = p.ficha.as_ref().map_or(6, |f| f.clave_recibidas);
                Concepto::from_clave(clave).unwrap_or(Concepto::Servicios)
            };
            sumar(
                &mut sumas,
                (key, c, false, false),
                Money::from_cents(base),
                plural(n, "factura recibida", "facturas recibidas"),
            );
        }
    }
    // Nóminas.
    {
        let mut st = conn.prepare(
            "SELECT e.nif, e.name, e.province, e.administrator OR p.director_subkey IS NOT NULL,
                    SUM(p.gross), COUNT(*)
               FROM payslip p JOIN employee e ON e.id = p.employee_id
              WHERE p.date BETWEEN ?1 AND ?2
              GROUP BY e.nif, 4",
        )?;
        let rows = st.query_map([&s, &e], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, bool>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
            ))
        })?;
        for row in rows {
            let (nif, _, _, admin, gross, n) = row?;
            let key = clave_nif(&nif);
            if !per.contains_key(&key) {
                continue;
            }
            let c = if admin {
                Concepto::RetribucionAdministrador
            } else {
                Concepto::Trabajo
            };
            sumar(
                &mut sumas,
                (key, c, false, false),
                Money::from_cents(gross),
                plural(n, "nómina", "nóminas"),
            );
        }
    }
    // Dividendos pagados.
    {
        let mut st = conn.prepare(
            "SELECT l.nif, SUM(l.gross), COUNT(DISTINCT p.id)
               FROM dividend_line l JOIN dividend_payment p ON p.id = l.payment_id
              WHERE p.date BETWEEN ?1 AND ?2 GROUP BY l.nif",
        )?;
        let rows = st.query_map([&s, &e], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        for row in rows {
            let (nif, gross, n) = row?;
            let key = clave_nif(&nif);
            if !per.contains_key(&key) {
                continue;
            }
            sumar(
                &mut sumas,
                (key, Concepto::Dividendos, false, false),
                Money::from_cents(gross),
                plural(n, "pago de dividendos", "pagos de dividendos"),
            );
        }
    }
    // Subcuentas asignadas y candidatas (sin las de los préstamos de esa misma persona, que ya
    // se han contado arriba).
    let asignadas: BTreeMap<String, String> = {
        let mut st = conn.prepare("SELECT account, nif FROM vinculada_cuenta")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let mut cuentas = Vec::new();
    for (account, debit, credit) in movimientos(conn, year)? {
        let nif = asignadas.get(&account).cloned();
        if let Some(n) = &nif {
            if de_prestamos.contains(&(account.clone(), n.clone())) {
                // Contada por el préstamo.
            } else if per.contains_key(n) {
                for (c, ingreso, importe) in clasificar(&account, debit, credit) {
                    if !importe.is_zero() {
                        sumar(
                            &mut sumas,
                            (n.clone(), c, ingreso, false),
                            importe.abs(),
                            format!("subcuenta {account}"),
                        );
                    }
                }
            } else {
                avisos.push(format!(
                    "La subcuenta {account} está asignada a {n}, que ya no está entre las \
                     vinculadas: sus movimientos no se cuentan."
                ));
            }
        }
        if nif.is_some() || CUENTAS_CANDIDATAS.iter().any(|p| account.starts_with(p)) {
            cuentas.push(Cuenta {
                name: crate::accounting::account_name(conn, &account)?.unwrap_or_default(),
                account,
                debit,
                credit,
                nif,
            });
        }
    }
    // Tecleadas.
    let manuales = operaciones_manuales(conn, year)?;
    for m in &manuales {
        let key = clave_nif(&m.nif);
        if !per.contains_key(&key) {
            avisos.push(format!(
                "Hay una operación tecleada con {key}, que ya no está entre las vinculadas: no \
                 se cuenta."
            ));
            continue;
        }
        let c = Concepto::from_clave(m.clave).unwrap_or(Concepto::Otras);
        sumar(
            &mut sumas,
            (key, c, m.ingreso, m.especifica),
            m.amount,
            if m.description.is_empty() {
                "tecleada".into()
            } else {
                format!("tecleada: {}", m.description)
            },
        );
    }

    // Valoraciones guardadas.
    let valoraciones: BTreeMap<(String, String, bool), (String, String, String)> = {
        let mut st = conn.prepare(
            "SELECT nif, concepto, ingreso, metodo, descripcion, justificacion
               FROM vinculada_valoracion WHERE year = ?1",
        )?;
        let rows = st.query_map([year], |r| {
            Ok((
                (
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, bool>(2)?,
                ),
                (
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, String>(5)?,
                ),
            ))
        })?;
        rows.collect::<std::result::Result<_, _>>()?
    };

    let mut operaciones: Vec<Operacion> = sumas
        .into_iter()
        .filter(|(_, s)| !s.importe.is_zero())
        .filter_map(|((nif, concepto, ingreso, especifica), s)| {
            let p = per.get(&nif)?;
            let val = valoraciones.get(&(nif.clone(), concepto.code().to_owned(), ingreso));
            let metodo = val.and_then(|(m, _, _)| Metodo::from_code(m));
            let excluida = concepto.excluida().map(str::to_owned).or_else(|| {
                p.grupo_fiscal.then(|| {
                    "Del mismo grupo de consolidación fiscal o AIE/UTE con sus miembros (Orden \
                     HFP/816/2017, art. 2.2)"
                        .to_owned()
                })
            });
            Some(Operacion {
                name: p.name.clone(),
                vinculo: p.vinculo(),
                clave: concepto.clave(),
                importe: s.importe.abs(),
                metodo: metodo.unwrap_or_default(),
                metodo_elegido: metodo.is_some(),
                especifica: especifica || matches!(concepto, Concepto::Intangibles),
                origen: s.origen,
                excluida,
                en_232: false,
                motivo: None,
                descripcion: val.map(|(_, d, _)| d.clone()).unwrap_or_default(),
                justificacion: val.map(|(_, _, j)| j.clone()).unwrap_or_default(),
                nif,
                concepto,
                ingreso,
            })
        })
        .collect();

    let (incn, incn_fuente) = cifra_de_negocios(v, start, end);
    let motivos = evaluar(&mut operaciones, incn);
    for p in per.values_mut() {
        p.total = operaciones
            .iter()
            .filter(|o| o.nif == p.nif && o.excluida.is_none())
            .map(|o| o.importe)
            .sum();
        p.supera_umbral = p.total > UMBRAL_PERSONA;
    }
    if incn.is_none() {
        avisos.push(
            "No se sabe la cifra de negocios del ejercicio (ni 200 ni ventas en TaxPain): no se \
             ha podido aplicar la regla del 50 % ni saber si la documentación puede ser \
             simplificada."
                .into(),
        );
    }
    if cuentas
        .iter()
        .any(|c| c.nif.is_none() && es_cuenta_corriente(&c.account))
    {
        avisos.push(
            "Hay movimientos en cuentas corrientes con socios o administradores (551, 552, 553) \
             sin asignar: di de quién es cada subcuenta para que se cuenten."
                .into(),
        );
    }
    if operaciones
        .iter()
        .any(|o| o.concepto == Concepto::RetribucionAdministrador && per[&o.nif].socio)
    {
        // TODO(VERIFY): que la exclusión del art. 18.2.b LIS alcance también a la retribución
        // de un administrador que además es socio con el 25 % o más.
        avisos.push(
            "La retribución de un administrador que además es socio con el 25 % o más se deja \
             fuera del 232 por el art. 18.2.b LIS (por el ejercicio de sus funciones): si cobra \
             por otros trabajos, tecléalo como rendimientos del trabajo (clave 10)."
                .into(),
        );
    }
    let simplificada = incn.is_none_or(|m| m < INCN_SIMPLIFICADA);
    let mut documentar: Vec<String> = per
        .values()
        .filter(|p| p.supera_umbral)
        .map(|p| p.name.clone())
        .collect();
    for o in operaciones
        .iter()
        .filter(|o| o.especifica && o.excluida.is_none())
    {
        if !documentar.contains(&o.name) {
            documentar.push(o.name.clone());
        }
    }
    let mut personas: Vec<Persona> = per.into_values().collect();
    personas.sort_by(|a, b| b.total.cmp(&a.total).then(a.name.cmp(&b.name)));
    operaciones.sort_by(|a, b| {
        b.en_232
            .cmp(&a.en_232)
            .then(a.name.cmp(&b.name))
            .then(a.concepto.cmp(&b.concepto))
            .then(a.ingreso.cmp(&b.ingreso).reverse())
    });
    Ok(Registro {
        year,
        start,
        end,
        company: info.name,
        company_nif,
        incn,
        incn_fuente,
        obligado: operaciones.iter().any(|o| o.en_232),
        personas,
        operaciones,
        manuales,
        cuentas,
        motivos,
        avisos,
        plazo: plazo(end),
        simplificada,
        documentar,
        textos: textos(v, year)?,
    })
}

/// Aplica los tres umbrales del 232 (Orden HFP/816/2017, art. 2): marca `en_232` y su motivo y
/// devuelve los motivos por los que hay que presentar. Las excluidas no cuentan ni se marcan.
pub fn evaluar(ops: &mut [Operacion], incn: Option<Money>) -> Vec<String> {
    let mut motivos = Vec::new();
    let cuenta = |o: &Operacion| o.excluida.is_none();
    // 1. Más de 250.000 € con la misma persona o entidad.
    let mut por_persona: BTreeMap<String, (String, Money)> = BTreeMap::new();
    for o in ops.iter().filter(|o| cuenta(o)) {
        let e = por_persona
            .entry(o.nif.clone())
            .or_insert_with(|| (o.name.clone(), Money::ZERO));
        e.1 += o.importe;
    }
    for (nif, (name, total)) in &por_persona {
        if *total > UMBRAL_PERSONA {
            let m = format!("más de 250.000 € con {name} ({total})");
            for o in ops.iter_mut().filter(|o| cuenta(o) && &o.nif == nif) {
                o.en_232 = true;
                o.motivo.get_or_insert_with(|| m.clone());
            }
            motivos.push(m);
        }
    }
    // 2. Operaciones específicas: más de 100.000 € por tipo.
    let mut especificas: BTreeMap<Concepto, Money> = BTreeMap::new();
    for o in ops.iter().filter(|o| cuenta(o) && o.especifica) {
        *especificas.entry(o.concepto).or_default() += o.importe;
    }
    for (c, total) in &especificas {
        if *total > UMBRAL_ESPECIFICAS {
            let m = format!(
                "operaciones específicas de «{}» por más de 100.000 € ({total})",
                c.label()
            );
            for o in ops
                .iter_mut()
                .filter(|o| cuenta(o) && o.especifica && o.concepto == *c)
            {
                o.en_232 = true;
                o.motivo.get_or_insert_with(|| m.clone());
            }
            motivos.push(m);
        }
    }
    // 3. Mismo tipo y método por encima del 50 % de la cifra de negocios.
    if let Some(incn) = incn.filter(|m| m.cents() > 0) {
        let mitad = Money::from_cents(incn.cents() / 2);
        let mut por_tipo: BTreeMap<(Concepto, Metodo), Money> = BTreeMap::new();
        for o in ops.iter().filter(|o| cuenta(o)) {
            *por_tipo.entry((o.concepto, o.metodo)).or_default() += o.importe;
        }
        for ((c, metodo), total) in &por_tipo {
            if *total > mitad {
                let m = format!(
                    "«{}» con el mismo método ({total}) por encima del 50 % de la cifra de \
                     negocios ({incn})",
                    c.label()
                );
                for o in ops
                    .iter_mut()
                    .filter(|o| cuenta(o) && o.concepto == *c && o.metodo == *metodo)
                {
                    o.en_232 = true;
                    o.motivo.get_or_insert_with(|| m.clone());
                }
                motivos.push(m);
            }
        }
    }
    motivos
}

/// Párrafos de la nota de operaciones con partes vinculadas de la memoria, sacados del
/// registro: retribución del administrador y operaciones por categoría. `None` si no hay nada
/// en el registro (la memoria deja entonces sus corchetes).
pub fn nota_memoria(v: &Vault, year: i32) -> Option<Vec<taxpain_invoicing::pdf::Para>> {
    use taxpain_invoicing::pdf::Para;
    let r = registro(v, year).ok()?;
    if r.operaciones.is_empty() {
        return None;
    }
    let mut p = Vec::new();
    let retribucion: Money = r
        .operaciones
        .iter()
        .filter(|o| o.concepto == Concepto::RetribucionAdministrador)
        .map(|o| o.importe)
        .sum();
    if retribucion.is_zero() {
        p.push(Para::Text(
            "Retribuciones del órgano de administración: [importe, o «el cargo de administrador \
             es gratuito» según los estatutos]."
                .into(),
        ));
    } else {
        p.push(Para::Row(
            "Retribuciones del órgano de administración (nóminas del ejercicio)".into(),
            retribucion.to_string(),
        ));
    }
    let mut por_categoria: BTreeMap<&str, BTreeMap<(Concepto, bool), Money>> = BTreeMap::new();
    for o in r.operaciones.iter().filter(|o| {
        !matches!(
            o.concepto,
            Concepto::RetribucionAdministrador | Concepto::Dividendos
        )
    }) {
        *por_categoria
            .entry(o.vinculo.categoria_memoria())
            .or_default()
            .entry((o.concepto, o.ingreso))
            .or_default() += o.importe;
    }
    if !por_categoria.is_empty() {
        p.push(Para::Text(
            "Operaciones con partes vinculadas del ejercicio (según el registro de operaciones \
             vinculadas de TaxPain; importes sin IVA) [revisa la categoría de cada una]:"
                .into(),
        ));
        for (cat, ops) in por_categoria {
            p.push(Para::Text(format!("{cat}:")));
            for ((c, ingreso), m) in ops {
                p.push(Para::Row(
                    format!(
                        "  {} ({})",
                        c.label(),
                        if ingreso { "ingreso" } else { "gasto o pago" }
                    ),
                    m.to_string(),
                ));
            }
        }
        p.push(Para::Text(
            "Las operaciones con partes vinculadas se han realizado a valor de mercado [indica \
             el método de valoración y las condiciones]."
                .into(),
        ));
    }
    Some(p)
}

/// Párrafos de la memoria con los préstamos con partes vinculadas: saldo vivo al cierre, tipo e
/// intereses del ejercicio de cada uno. Vacío si no hay.
pub fn nota_prestamos(v: &Vault, year: i32) -> Vec<taxpain_invoicing::pdf::Para> {
    use taxpain_invoicing::pdf::Para;
    let Ok(lista) = prestamos_vinculados(v, year) else {
        return vec![];
    };
    if lista.is_empty() {
        return vec![];
    }
    let mut p = vec![Para::Text(
        "Préstamos con partes vinculadas (según Contabilidad → Préstamos de TaxPain):".into(),
    )];
    for pv in &lista {
        let d = &pv.ejercicio;
        let pr = &d.prestamo;
        let sentido = match pr.sentido {
            taxpain_core::accounting::prestamo::Sentido::Concedido => "concedido a",
            taxpain_core::accounting::prestamo::Sentido::Recibido => "recibido de",
        };
        p.push(Para::Text(format!(
            "  {} ({sentido} {}, {}): saldo vivo al cierre {}; {}; intereses del ejercicio {}.",
            pr.nombre,
            pv.name,
            pv.vinculo.categoria_memoria().to_lowercase(),
            d.saldo_vivo,
            if pr.tipo.bp() == 0 {
                "sin intereses".to_owned()
            } else {
                format!("interés nominal anual del {}", pr.tipo)
            },
            d.intereses
        )));
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Month;

    fn op(nif: &str, c: Concepto, importe: i64) -> Operacion {
        Operacion {
            nif: nif.into(),
            name: format!("Vinculada {nif}"),
            vinculo: Vinculo::Socio,
            concepto: c,
            clave: c.clave(),
            ingreso: true,
            importe: Money::from_euros(importe, 0),
            metodo: Metodo::default(),
            metodo_elegido: false,
            especifica: false,
            origen: vec![],
            excluida: c.excluida().map(str::to_owned),
            en_232: false,
            motivo: None,
            descripcion: String::new(),
            justificacion: String::new(),
        }
    }

    #[test]
    fn claves_del_232() {
        assert_eq!(Concepto::BienesTangibles.clave(), Some(1));
        assert_eq!(Concepto::Servicios.clave(), Some(6));
        assert_eq!(Concepto::Otras.clave(), Some(11));
        assert_eq!(Concepto::Dividendos.clave(), None);
        assert_eq!(Concepto::from_clave(8), Some(Concepto::Alquileres));
        assert_eq!(Concepto::from_clave(0), None);
        assert_eq!(Concepto::from_clave(12), None);
        for c in Concepto::DEL_232 {
            assert_eq!(Concepto::from_code(c.code()), Some(c));
        }
        for x in Vinculo::ALL {
            assert_eq!(Vinculo::from_code(x.code()), Some(x));
        }
        assert_eq!(Vinculo::Familiar.letra(), "c");
    }

    #[test]
    fn plazo_del_232() {
        let d = |y, m, dd| Date::from_calendar_date(y, m, dd).unwrap();
        assert_eq!(plazo(d(2025, Month::December, 31)), "noviembre de 2026");
        assert_eq!(plazo(d(2026, Month::June, 30)), "mayo de 2027");
        assert_eq!(plazo(d(2026, Month::January, 31)), "diciembre de 2026");
    }

    #[test]
    fn umbral_por_persona_suma_ingresos_y_pagos_sin_las_excluidas() {
        let mut v = vec![
            op("A", Concepto::Servicios, 200_000),
            op("A", Concepto::Alquileres, 50_000),
            op("A", Concepto::Dividendos, 900_000),
            op("B", Concepto::Servicios, 10_000),
        ];
        // 250.000 justos no pasan («supere»).
        assert!(evaluar(&mut v, None).is_empty());
        v[1].importe = Money::from_euros(50_001, 0);
        let m = evaluar(&mut v, None);
        assert_eq!(m.len(), 1);
        assert!(m[0].contains("Vinculada A"), "{m:?}");
        assert!(v[0].en_232 && v[1].en_232);
        assert!(!v[2].en_232, "los dividendos no van");
        assert!(!v[3].en_232);
    }

    #[test]
    fn especificas_por_tipo_por_encima_de_100_000() {
        let mut v = vec![
            op("A", Concepto::BienesTangibles, 60_000),
            op("B", Concepto::BienesTangibles, 50_000),
            op("C", Concepto::BienesTangibles, 90_000),
        ];
        v[0].especifica = true;
        v[1].especifica = true;
        let m = evaluar(&mut v, None);
        assert_eq!(m.len(), 1, "{m:?}");
        assert!(v[0].en_232 && v[1].en_232 && !v[2].en_232);
    }

    #[test]
    fn mismo_tipo_y_metodo_por_encima_de_la_mitad_de_la_cifra_de_negocios() {
        let mut v = vec![
            op("A", Concepto::Servicios, 30_000),
            op("B", Concepto::Servicios, 30_000),
            op("C", Concepto::Servicios, 10_000),
        ];
        v[2].metodo = Metodo::CosteIncrementado;
        // 60.000 con precio libre comparable frente a una cifra de 100.000: más del 50 %.
        let m = evaluar(&mut v, Some(Money::from_euros(100_000, 0)));
        assert_eq!(m.len(), 1, "{m:?}");
        assert!(v[0].en_232 && v[1].en_232 && !v[2].en_232);
        let mut w = vec![op("A", Concepto::Servicios, 50_000)];
        assert!(evaluar(&mut w, Some(Money::from_euros(100_000, 0))).is_empty());
    }

    #[test]
    fn el_grupo_fiscal_no_cuenta() {
        let mut v = vec![op("A", Concepto::Servicios, 900_000)];
        v[0].excluida = Some("grupo".into());
        assert!(evaluar(&mut v, Some(Money::from_euros(1, 0))).is_empty());
        assert!(!v[0].en_232);
    }
}
