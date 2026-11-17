//! Modelos 720 y 721: borrador con los registros de cada bien en el extranjero y la decisión de
//! si hay que presentar.
//!
//! - 720 (bienes y derechos en el extranjero, DA 18.ª LGT; arts. 42 bis y 42 ter RGAT; Orden
//!   HAP/72/2013): bloque de **cuentas** (las cuentas bancarias con IBAN de otro país o marcadas
//!   como del extranjero), con el saldo a 31 de diciembre y el saldo medio del último trimestre,
//!   y bloque de **valores** (posiciones de las cuentas de bróker del extranjero a 31/12,
//!   clave V para acciones y bonos, clave I para fondos y ETF). Los inmuebles no están.
//! - 721 (monedas virtuales en el extranjero, art. 42 quater RGAT; Orden HFP/886/2023): saldos
//!   por moneda en las plataformas del extranjero, en unidades y en euros a 31/12.
//!
//! Por bloque, la primera vez se presenta si el valor conjunto pasa de 50.000 € (en las cuentas,
//! basta con que lo pase el saldo a 31/12 o el saldo medio del cuarto trimestre). Los años
//! siguientes sólo si ese valor sube más de 20.000 € respecto del que determinó la última
//! declaración o si se ha extinguido la titularidad de algún bien declarado. Lo declarado sale
//! de [`marcar_presentado`] (un borrador presentado) o de lo tecleado en [`guardar_declarado`].
//!
//! El fichero: TaxPain no tiene el diseño de registro de ninguno de los dos (no está en
//! `fixtures/aeat/dr` ni en `crates/models`), así que no lo genera: deja la hoja de registros en
//! Excel ([`export`]) para pasarlos al formulario de la Sede.
//!
//! Una persona jurídica con la contabilidad en TaxPain no presenta los bienes que tiene
//! registrados de forma individualizada (arts. 42 bis.4.a, 42 ter.4.b y 42 quater.5.b RGAT): el
//! borrador lo dice y comprueba que estén la subcuenta, la entidad, su NIF y el país.

pub mod export;
#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::{Date, Month, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;

/// Valor conjunto de un bloque a partir del cual se presenta la primera vez.
pub const UMBRAL: Money = Money::from_euros(50_000, 0);
/// Incremento del valor de un bloque que obliga a volver a presentar.
pub const INCREMENTO: Money = Money::from_euros(20_000, 0);

/// Diseño de registro de cada modelo. Ninguno está en el proyecto.
// TODO(VERIFY): 720, Orden HAP/72/2013, de 30 de enero (BOE 31/01/2013), con sus
// modificaciones; 721, Orden HFP/886/2023, de 26 de julio (BOE 28/07/2023). Cuando el diseño
// esté en `fixtures/aeat/dr`, generar el fichero con `taxpain_models::dr` como en el 190.
const FICHERO_720: &str = "TaxPain no tiene el diseño de registro del 720 (Orden HAP/72/2013, \
     de 30 de enero, y sus modificaciones), así que no genera el fichero: pasa los registros de la \
     hoja al formulario del 720 en la Sede de la AEAT.";
const FICHERO_721: &str = "TaxPain no tiene el diseño de registro del 721 (Orden HFP/886/2023, \
     de 26 de julio), así que no genera el fichero: pasa los registros de la hoja al formulario \
     del 721 en la Sede de la AEAT.";

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

fn parse_date(s: &str) -> Option<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]")).ok()
}

fn dic31(year: i32) -> Result<Date> {
    Date::from_calendar_date(year, Month::December, 31).map_err(|e| other(e.to_string()))
}

// ---------- Tipos ----------

/// De dónde sale un bien.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origen {
    /// Cuenta bancaria (`bank_account`).
    Banco,
    /// Cuenta de valores en un bróker (`security_account`).
    Valores,
    /// Plataforma de criptomonedas (`crypto_platform`).
    Cripto,
}

impl Origen {
    fn sql(self) -> &'static str {
        match self {
            Origen::Banco => "banco",
            Origen::Valores => "valores",
            Origen::Cripto => "cripto",
        }
    }
}

/// Condición del declarante respecto del bien.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condicion {
    #[default]
    Titular,
    Representante,
    Autorizado,
    Beneficiario,
    Usufructuario,
    Tomador,
    PoderDisposicion,
    TitularReal,
    Otra,
}

impl Condicion {
    const TODAS: [Condicion; 9] = [
        Condicion::Titular,
        Condicion::Representante,
        Condicion::Autorizado,
        Condicion::Beneficiario,
        Condicion::Usufructuario,
        Condicion::Tomador,
        Condicion::PoderDisposicion,
        Condicion::TitularReal,
        Condicion::Otra,
    ];

    fn sql(self) -> &'static str {
        match self {
            Condicion::Titular => "titular",
            Condicion::Representante => "representante",
            Condicion::Autorizado => "autorizado",
            Condicion::Beneficiario => "beneficiario",
            Condicion::Usufructuario => "usufructuario",
            Condicion::Tomador => "tomador",
            Condicion::PoderDisposicion => "poder_disposicion",
            Condicion::TitularReal => "titular_real",
            Condicion::Otra => "otra",
        }
    }

    fn de(s: &str) -> Condicion {
        Condicion::TODAS
            .into_iter()
            .find(|c| c.sql() == s)
            .unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            Condicion::Titular => "Titular",
            Condicion::Representante => "Representante",
            Condicion::Autorizado => "Autorizado",
            Condicion::Beneficiario => "Beneficiario",
            Condicion::Usufructuario => "Usufructuario",
            Condicion::Tomador => "Tomador",
            Condicion::PoderDisposicion => "Con poder de disposición",
            Condicion::TitularReal => "Otras formas de titularidad real",
            Condicion::Otra => "Otra",
        }
    }

    /// Clave de la condición en el formulario.
    // TODO(VERIFY): claves 1 a 8 de la condición del declarante del 720 (anexo de la Orden
    // HAP/72/2013); el 721 usa su propia lista (Orden HFP/886/2023).
    pub fn clave(self) -> &'static str {
        match self {
            Condicion::Titular => "1",
            Condicion::Representante => "2",
            Condicion::Autorizado => "3",
            Condicion::Beneficiario => "4",
            Condicion::Usufructuario => "5",
            Condicion::Tomador => "6",
            Condicion::PoderDisposicion => "7",
            Condicion::TitularReal | Condicion::Otra => "8",
        }
    }
}

/// Lo que la contabilidad no sabe de un bien y teclea el usuario.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatosBien {
    pub origen: Origen,
    pub ref_id: i64,
    #[serde(default)]
    pub condicion: Condicion,
    /// Participación en centésimas de punto (10000 = 100 %).
    #[serde(default = "cien")]
    pub porcentaje_bp: i64,
    #[serde(default)]
    pub fecha_apertura: Option<Date>,
    /// Cancelación de la cuenta o extinción de la titularidad.
    #[serde(default)]
    pub fecha_extincion: Option<Date>,
    #[serde(default)]
    pub entidad: String,
    /// NIF de la entidad o del custodio en su país.
    #[serde(default)]
    pub nif_entidad: String,
    #[serde(default)]
    pub bic: String,
    #[serde(default)]
    pub domicilio: String,
    /// ISO 3166-1 alfa-2; vacío = el del IBAN o el de la cuenta o la plataforma.
    #[serde(default)]
    pub pais: String,
    /// Sólo cuentas bancarias: `Some` fuerza que sea (o no) del extranjero.
    #[serde(default)]
    pub extranjero: Option<bool>,
}

fn cien() -> i64 {
    10_000
}

impl DatosBien {
    fn vacios(origen: Origen, ref_id: i64) -> DatosBien {
        DatosBien {
            origen,
            ref_id,
            condicion: Condicion::Titular,
            porcentaje_bp: 10_000,
            fecha_apertura: None,
            fecha_extincion: None,
            entidad: String::new(),
            nif_entidad: String::new(),
            bic: String::new(),
            domicilio: String::new(),
            pais: String::new(),
            extranjero: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bloque {
    Cuentas,
    Valores,
    Monedas,
}

impl Bloque {
    fn sql(self) -> &'static str {
        match self {
            Bloque::Cuentas => "cuentas",
            Bloque::Valores => "valores",
            Bloque::Monedas => "monedas",
        }
    }

    fn de(s: &str) -> Option<Bloque> {
        [Bloque::Cuentas, Bloque::Valores, Bloque::Monedas]
            .into_iter()
            .find(|b| b.sql() == s)
    }

    fn modelo(self) -> &'static str {
        match self {
            Bloque::Cuentas | Bloque::Valores => "720",
            Bloque::Monedas => "721",
        }
    }

    pub fn titulo(self) -> &'static str {
        match self {
            Bloque::Cuentas => "Cuentas en entidades financieras",
            Bloque::Valores => "Valores, derechos e IIC",
            Bloque::Monedas => "Monedas virtuales",
        }
    }
}

/// Origen del bien en la declaración.
// TODO(VERIFY): claves A (alta: se declara por primera vez), M (ya declarado otro año) y C
// (extinguido en el ejercicio) del campo «origen del bien o derecho» (Orden HAP/72/2013; el 721
// tiene el mismo campo en la Orden HFP/886/2023).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Alta {
    A,
    M,
    C,
}

/// Un registro de la declaración: una cuenta, un valor en una cuenta o una moneda en una
/// plataforma.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Registro {
    pub origen: Origen,
    pub ref_id: i64,
    pub bloque: Bloque,
    /// C (cuenta), V (valores), I (IIC) o M (moneda virtual).
    pub clave: &'static str,
    pub descripcion: String,
    /// IBAN, número de cuenta, ISIN o la moneda.
    pub identificacion: String,
    pub entidad: String,
    pub nif_entidad: String,
    pub bic: String,
    pub domicilio: String,
    pub pais: String,
    pub condicion: Condicion,
    pub porcentaje_bp: i64,
    pub fecha_apertura: Option<Date>,
    /// La fecha no la ha tecleado el usuario: es la del primer movimiento.
    pub fecha_estimada: bool,
    pub fecha_extincion: Option<Date>,
    /// Saldo o valor a 31/12 en euros.
    pub saldo: Money,
    /// Cuentas: saldo medio del cuarto trimestre.
    pub saldo_medio: Option<Money>,
    /// Valores y monedas: unidades a 31/12.
    pub unidades: Option<String>,
    pub moneda: String,
    pub alta: Alta,
    /// Datos que faltan para rellenar el registro.
    pub faltan: Vec<String>,
}

/// Lo declarado de un bloque en un año.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Declarado {
    #[serde(default)]
    pub id: i64,
    pub modelo: String,
    pub year: i32,
    pub bloque: Bloque,
    pub valor: Money,
    #[serde(default)]
    pub saldo_medio: Option<Money>,
    /// Identificación de los bienes declarados (IBAN, ISIN, moneda@plataforma).
    #[serde(default)]
    pub bienes: Vec<String>,
    /// `borrador` o `tecleado`.
    #[serde(default)]
    pub origen: String,
    #[serde(default)]
    pub creado: String,
}

/// Un bloque del borrador con la decisión.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BloqueView {
    pub bloque: Bloque,
    pub titulo: &'static str,
    pub total: Money,
    pub total_medio: Option<Money>,
    pub supera: bool,
    pub anterior: Option<Declarado>,
    pub incremento: Option<Money>,
    pub incremento_medio: Option<Money>,
    /// Bienes declarados que ya no se tienen (o extinguidos en el ejercicio).
    pub extinguidos: Vec<String>,
    pub obligado: bool,
    pub motivo: String,
    pub registros: Vec<Registro>,
}

/// Un bien con sus datos, para editarlos.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BienView {
    pub nombre: String,
    /// Cuentas bancarias: si cuenta como del extranjero (por el IBAN o marcada).
    pub extranjero: bool,
    pub iban: String,
    pub datos: DatosBien,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Fichero {
    pub disponible: bool,
    pub nota: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Borrador {
    pub modelo: String,
    pub year: i32,
    pub fecha: Date,
    /// Último día de presentación (31 de marzo del año siguiente).
    pub plazo: Date,
    pub nif: String,
    pub nombre: String,
    pub persona_fisica: bool,
    pub bloques: Vec<BloqueView>,
    /// Algún bloque obliga a presentar.
    pub obligado: bool,
    /// Persona jurídica: no presenta lo que tiene contabilizado de forma individualizada.
    pub exenta_por_contabilidad: bool,
    /// Lo que falta en la contabilidad o en los datos para la exoneración.
    pub requisitos: Vec<String>,
    /// Años con este modelo presentado según la AEAT o importado.
    pub presentados_aeat: Vec<i32>,
    pub declarados: Vec<Declarado>,
    pub bienes: Vec<BienView>,
    pub fichero: Fichero,
    pub avisos: Vec<String>,
    pub notas: Vec<String>,
    pub norma: &'static str,
}

// ---------- La regla ----------

/// Lo que decide un bloque.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub supera: bool,
    pub incremento: Option<Money>,
    pub incremento_medio: Option<Money>,
    pub extinguidos: Vec<String>,
    pub obligado: bool,
    pub motivo: String,
}

/// Decide si se presenta un bloque: primera vez, más de 50.000 € (en las cuentas, a 31/12 o de
/// saldo medio del último trimestre); con una declaración anterior, incremento de más de
/// 20.000 € respecto de lo que la determinó o extinción de algún bien declarado.
// TODO(VERIFY): la extinción de la titularidad de un bien declarado obliga a presentar con
// origen C aunque no haya incremento (arts. 42 bis.3, 42 ter.3 y 42 quater.3 RGAT y FAQ de la
// AEAT sobre la declaración en años sucesivos); confirmar el apartado exacto.
pub fn decidir(
    total: Money,
    medio: Option<Money>,
    actuales: &BTreeSet<String>,
    extintos: &BTreeSet<String>,
    anterior: Option<&Declarado>,
) -> Decision {
    let supera = total > UMBRAL || medio.is_some_and(|m| m > UMBRAL);
    let Some(a) = anterior else {
        let motivo = if supera {
            format!(
                "Supera 50.000 € ({}{}) y no hay declaración anterior de este bloque: se \
                 presenta.",
                total,
                medio.map_or(String::new(), |m| format!(
                    "; saldo medio del 4.º trimestre {m}"
                ))
            )
        } else {
            format!(
                "No pasa de 50.000 € ({}{}): no se presenta.",
                total,
                medio.map_or(String::new(), |m| format!(
                    "; saldo medio del 4.º trimestre {m}"
                ))
            )
        };
        return Decision {
            supera,
            incremento: None,
            incremento_medio: None,
            extinguidos: Vec::new(),
            obligado: supera,
            motivo,
        };
    };
    let incremento = total - a.valor;
    let incremento_medio = match (medio, a.saldo_medio) {
        (Some(m), Some(p)) => Some(m - p),
        _ => None,
    };
    let extinguidos: Vec<String> = a
        .bienes
        .iter()
        .filter(|b| !actuales.contains(*b) || extintos.contains(*b))
        .cloned()
        .collect();
    let sube = incremento > INCREMENTO || incremento_medio.is_some_and(|i| i > INCREMENTO);
    let obligado = sube || !extinguidos.is_empty();
    let motivo = if sube {
        format!(
            "Ha subido más de 20.000 € respecto del {} de {} ({} → {}{}): se vuelve a presentar.",
            a.modelo,
            a.year,
            a.valor,
            total,
            incremento_medio
                .filter(|i| *i > INCREMENTO)
                .map_or(String::new(), |i| format!("; el saldo medio sube {i}"))
        )
    } else if !extinguidos.is_empty() {
        format!(
            "Se ha dejado de tener {} declarado en el {} de {}: se presenta para dar de baja \
             (origen C).",
            extinguidos.join(", "),
            a.modelo,
            a.year
        )
    } else {
        format!(
            "Ya se declaró en el {} de {} ({}) y no ha subido más de 20.000 € ({}): no hay que \
             volver a presentar.",
            a.modelo, a.year, a.valor, total
        )
    };
    Decision {
        supera,
        incremento: Some(incremento),
        incremento_medio,
        extinguidos,
        obligado,
        motivo,
    }
}

// ---------- Datos de los bienes ----------

fn datos_de(v: &Vault, origen: Origen, ref_id: i64) -> Result<DatosBien> {
    let d = v
        .connection()
        .query_row(
            "SELECT condition, share_bp, opened_on, closed_on, entity_name, entity_tax_id, bic,
                    address, country, abroad
             FROM foreign_asset_info WHERE source = ?1 AND ref_id = ?2",
            params![origen.sql(), ref_id],
            |r| {
                Ok(DatosBien {
                    origen,
                    ref_id,
                    condicion: Condicion::de(&r.get::<_, String>(0)?),
                    porcentaje_bp: r.get(1)?,
                    fecha_apertura: r.get::<_, Option<String>>(2)?.and_then(|s| parse_date(&s)),
                    fecha_extincion: r.get::<_, Option<String>>(3)?.and_then(|s| parse_date(&s)),
                    entidad: r.get(4)?,
                    nif_entidad: r.get(5)?,
                    bic: r.get(6)?,
                    domicilio: r.get(7)?,
                    pais: r.get(8)?,
                    extranjero: r.get::<_, Option<i64>>(9)?.map(|x| x == 1),
                })
            },
        )
        .optional()?;
    Ok(d.unwrap_or_else(|| DatosBien::vacios(origen, ref_id)))
}

/// Guarda los datos de un bien (queda en la auditoría).
pub fn guardar_bien(v: &Vault, d: &DatosBien, now: OffsetDateTime) -> Result<()> {
    if !(1..=10_000).contains(&d.porcentaje_bp) {
        return Err(other("el porcentaje va de 0,01 % a 100 %"));
    }
    let pais = d.pais.trim().to_uppercase();
    if !pais.is_empty() && (pais.len() != 2 || !pais.chars().all(|c| c.is_ascii_alphabetic())) {
        return Err(other("país: código de dos letras (LT, IE, US…)"));
    }
    if let (Some(a), Some(c)) = (d.fecha_apertura, d.fecha_extincion)
        && c < a
    {
        return Err(other("la extinción no puede ser anterior a la apertura"));
    }
    let tabla = match d.origen {
        Origen::Banco => "bank_account",
        Origen::Valores => "security_account",
        Origen::Cripto => "crypto_platform",
    };
    let conn = v.connection();
    let existe: bool = conn.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {tabla} WHERE id = ?1)"),
        [d.ref_id],
        |r| r.get(0),
    )?;
    if !existe {
        return Err(other("no existe ese bien"));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO foreign_asset_info(source, ref_id, condition, share_bp, opened_on, closed_on,
                entity_name, entity_tax_id, bic, address, country, abroad, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(source, ref_id) DO UPDATE SET condition = excluded.condition,
           share_bp = excluded.share_bp, opened_on = excluded.opened_on,
           closed_on = excluded.closed_on, entity_name = excluded.entity_name,
           entity_tax_id = excluded.entity_tax_id, bic = excluded.bic,
           address = excluded.address, country = excluded.country, abroad = excluded.abroad,
           updated_at = excluded.updated_at",
        params![
            d.origen.sql(),
            d.ref_id,
            d.condicion.sql(),
            d.porcentaje_bp,
            d.fecha_apertura.map(|x| x.to_string()),
            d.fecha_extincion.map(|x| x.to_string()),
            d.entidad.trim(),
            d.nif_entidad.trim(),
            d.bic.trim().to_uppercase(),
            d.domicilio.trim(),
            pais,
            d.extranjero.map(i64::from),
            now_str(now),
        ],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "extranjero.datos_bien",
        &format!("{}:{}", d.origen.sql(), d.ref_id),
        &serde_json::to_value(d).map_err(|e| other(e.to_string()))?,
    )?;
    tx.commit()?;
    Ok(())
}

// ---------- Lo declarado ----------

fn declarados_de(v: &Vault, modelo: &str) -> Result<Vec<Declarado>> {
    let mut st = v.connection().prepare(
        "SELECT id, model, year, block, value, avg_balance, assets, origin, created_at
         FROM foreign_asset_filing WHERE model = ?1 ORDER BY year DESC, block",
    )?;
    let rows = st.query_map([modelo], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i32>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, Option<i64>>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, String>(7)?,
            r.get::<_, String>(8)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, modelo, year, bloque, valor, medio, bienes, origen, creado) = row?;
        let Some(bloque) = Bloque::de(&bloque) else {
            continue;
        };
        out.push(Declarado {
            id,
            modelo,
            year,
            bloque,
            valor: Money::from_cents(valor),
            saldo_medio: medio.map(Money::from_cents),
            bienes: serde_json::from_str(&bienes).unwrap_or_default(),
            origen,
            creado,
        });
    }
    Ok(out)
}

fn escribir_declarado(
    tx: &taxpain_vault::rusqlite::Transaction<'_>,
    d: &Declarado,
    origen: &str,
    now: OffsetDateTime,
) -> Result<()> {
    let bienes = serde_json::to_string(&d.bienes).map_err(|e| other(e.to_string()))?;
    tx.execute(
        "INSERT INTO foreign_asset_filing(model, year, block, value, avg_balance, assets, origin,
                created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(model, year, block) DO UPDATE SET value = excluded.value,
           avg_balance = excluded.avg_balance, assets = excluded.assets,
           origin = excluded.origin, created_at = excluded.created_at",
        params![
            d.bloque.modelo(),
            d.year,
            d.bloque.sql(),
            d.valor.cents(),
            d.saldo_medio.map(Money::cents),
            bienes,
            origen,
            now_str(now),
        ],
    )?;
    Ok(())
}

/// Guarda lo declarado de un bloque en un año anterior (tecleado del 720 o el 721 presentado).
pub fn guardar_declarado(v: &Vault, d: &Declarado, now: OffsetDateTime) -> Result<()> {
    if d.modelo != d.bloque.modelo() {
        return Err(other(format!(
            "el bloque de {} es del {}",
            d.bloque.titulo().to_lowercase(),
            d.bloque.modelo()
        )));
    }
    if !(2012..=2100).contains(&d.year) {
        return Err(other("ejercicio no válido"));
    }
    if d.valor.is_negative() || d.saldo_medio.is_some_and(Money::is_negative) {
        return Err(other("los importes declarados no pueden ser negativos"));
    }
    if d.bloque != Bloque::Cuentas && d.saldo_medio.is_some() {
        return Err(other("el saldo medio sólo es de las cuentas"));
    }
    let mut d = d.clone();
    d.bienes = d
        .bienes
        .iter()
        .map(|b| b.trim().to_uppercase().replace(' ', ""))
        .filter(|b| !b.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    escribir_declarado(&tx, &d, "tecleado", now)?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "extranjero.declarado",
        &format!("{}-{}-{}", d.modelo, d.year, d.bloque.sql()),
        &serde_json::to_value(&d).map_err(|e| other(e.to_string()))?,
    )?;
    tx.commit()?;
    Ok(())
}

/// Quita lo declarado de un bloque (con la confirmación del diálogo).
pub fn quitar_declarado(v: &Vault, id: i64, confirmed: bool, now: OffsetDateTime) -> Result<()> {
    if !confirmed {
        return Err(other("hay que confirmar que se quita lo declarado"));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let fila: Option<(String, i32, String, i64)> = tx
        .query_row(
            "SELECT model, year, block, value FROM foreign_asset_filing WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((modelo, year, bloque, valor)) = fila else {
        return Err(other("no existe esa declaración"));
    };
    tx.execute("DELETE FROM foreign_asset_filing WHERE id = ?1", [id])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "extranjero.quitar_declarado",
        &format!("{modelo}-{year}-{bloque}"),
        &serde_json::json!({ "valor": valor }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Apunta como presentado el borrador de `year`: los bloques que obligan, con su valor, el
/// saldo medio y los bienes (sin los extinguidos). Sirve para la regla de los años siguientes.
pub fn marcar_presentado(
    v: &Vault,
    modelo: &str,
    year: i32,
    confirmed: bool,
    today: Date,
    now: OffsetDateTime,
) -> Result<Vec<Declarado>> {
    if !confirmed {
        return Err(other("hay que confirmar que el modelo está presentado"));
    }
    let b = borrador(v, modelo, year, today)?;
    let mut hechos = Vec::new();
    for bl in b.bloques.iter().filter(|x| x.obligado) {
        hechos.push(Declarado {
            id: 0,
            modelo: modelo.to_owned(),
            year,
            bloque: bl.bloque,
            valor: bl.total,
            saldo_medio: bl.total_medio,
            bienes: bl
                .registros
                .iter()
                .filter(|r| r.alta != Alta::C)
                .map(|r| clave_bien(r.bloque, &r.identificacion, &r.entidad))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            origen: "borrador".into(),
            creado: now_str(now),
        });
    }
    if hechos.is_empty() {
        return Err(other(format!(
            "el borrador del {modelo} de {year} no tiene ningún bloque que presentar"
        )));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    for d in &hechos {
        escribir_declarado(&tx, d, "borrador", now)?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "extranjero.presentado",
        &format!("{modelo}-{year}"),
        &serde_json::to_value(&hechos).map_err(|e| other(e.to_string()))?,
    )?;
    tx.commit()?;
    Ok(hechos)
}

/// Identificación de un bien para cotejarlo con lo declarado: el IBAN o la cuenta, el ISIN, o la
/// moneda con su plataforma («BTC@KRAKEN»).
fn clave_bien(bloque: Bloque, identificacion: &str, entidad: &str) -> String {
    let id = identificacion.trim().to_uppercase().replace(' ', "");
    match bloque {
        Bloque::Monedas => format!("{id}@{}", entidad.trim().to_uppercase()),
        _ => id,
    }
}

// ---------- Bienes ----------

struct Cuenta {
    id: i64,
    iban: String,
    number: String,
    account: String,
    name: String,
    currency: String,
    bank: String,
}

fn cuentas_bancarias(v: &Vault) -> Result<Vec<Cuenta>> {
    let mut st = v.connection().prepare(
        "SELECT id, COALESCE(iban, ''), number, account, name, currency, bank FROM bank_account
         WHERE kind IN ('cuenta', 'monedero') ORDER BY name",
    )?;
    let rows = st.query_map([], |r| {
        Ok(Cuenta {
            id: r.get(0)?,
            iban: r.get(1)?,
            number: r.get(2)?,
            account: r.get(3)?,
            name: r.get(4)?,
            currency: r.get(5)?,
            bank: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

fn pais_iban(iban: &str) -> String {
    let p: String = iban.trim().chars().take(2).collect();
    if p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic()) {
        p.to_uppercase()
    } else {
        String::new()
    }
}

fn es_extranjera(c: &Cuenta, d: &DatosBien) -> bool {
    d.extranjero.unwrap_or_else(|| {
        let p = pais_iban(&c.iban);
        !p.is_empty() && p != "ES"
    })
}

/// Saldo medio del cuarto trimestre: la media de los saldos al final de cada día del 1 de
/// octubre al 31 de diciembre (art. 42 bis.2 RGAT).
pub(crate) fn saldo_medio_4t(v: &Vault, account: &str, year: i32) -> Result<Money> {
    let sep30 =
        Date::from_calendar_date(year, Month::September, 30).map_err(|e| other(e.to_string()))?;
    let fin = dic31(year)?;
    let mut saldo = crate::umbrales::saldo(v, account, sep30)?.cents();
    let mut st = v.connection().prepare(
        "SELECT e.date, COALESCE(SUM(l.debit - l.credit), 0) FROM journal_line l
         JOIN journal_entry e ON e.id = l.entry_id
         WHERE e.date > ?1 AND e.date <= ?2 AND e.origin_kind NOT IN ('Cierre', 'Apertura')
           AND l.account = ?3
         GROUP BY e.date ORDER BY e.date",
    )?;
    let movs: Vec<(String, i64)> = st
        .query_map(params![sep30.to_string(), fin.to_string(), account], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    let mut movs = movs
        .into_iter()
        .filter_map(|(d, c)| parse_date(&d).map(|d| (d, c)))
        .peekable();
    let mut suma: i128 = 0;
    let mut dias: i128 = 0;
    let mut dia = sep30;
    while dia < fin {
        dia = dia.next_day().unwrap_or(fin);
        while let Some((d, c)) = movs.peek() {
            if *d > dia {
                break;
            }
            saldo += c;
            movs.next();
        }
        suma += i128::from(saldo);
        dias += 1;
    }
    // Redondeo al céntimo, la mitad hacia fuera del cero.
    let media = if suma >= 0 {
        (suma + dias / 2) / dias
    } else {
        (suma - dias / 2) / dias
    };
    Ok(Money::from_cents(i64::try_from(media).unwrap_or(0)))
}

fn primer_apunte(v: &Vault, account: &str) -> Result<Option<Date>> {
    let d: Option<String> = v.connection().query_row(
        "SELECT MIN(e.date) FROM journal_line l JOIN journal_entry e ON e.id = l.entry_id
         WHERE l.account = ?1",
        [account],
        |r| r.get(0),
    )?;
    Ok(d.and_then(|s| parse_date(&s)))
}

fn en_el_anio(d: Option<Date>, year: i32) -> bool {
    d.is_some_and(|d| d.year() == year)
}

fn alta(
    bloque: Bloque,
    ident: &str,
    entidad: &str,
    extincion: Option<Date>,
    year: i32,
    anterior: Option<&Declarado>,
) -> Alta {
    if extincion.is_some_and(|d| d.year() <= year) {
        Alta::C
    } else if anterior.is_some_and(|a| a.bienes.contains(&clave_bien(bloque, ident, entidad))) {
        Alta::M
    } else {
        Alta::A
    }
}

fn falta(faltan: &mut Vec<String>, cond: bool, que: &str) {
    if cond {
        faltan.push(que.to_owned());
    }
}

struct Bloques {
    registros: Vec<Registro>,
    bienes: Vec<BienView>,
    avisos: Vec<String>,
}

fn anterior_de(declarados: &[Declarado], bloque: Bloque, year: i32) -> Option<&Declarado> {
    declarados
        .iter()
        .filter(|d| d.bloque == bloque && d.year < year)
        .max_by_key(|d| d.year)
}

fn cuentas_720(v: &Vault, year: i32, anterior: Option<&Declarado>) -> Result<Bloques> {
    let fin = dic31(year)?;
    let mut out = Bloques {
        registros: Vec::new(),
        bienes: Vec::new(),
        avisos: Vec::new(),
    };
    let todas = cuentas_bancarias(v)?;
    for c in &todas {
        let d = datos_de(v, Origen::Banco, c.id)?;
        let extranjera = es_extranjera(c, &d);
        out.bienes.push(BienView {
            nombre: c.name.clone(),
            extranjero: extranjera,
            iban: c.iban.clone(),
            datos: d.clone(),
        });
        if !extranjera {
            continue;
        }
        if c.account.is_empty() {
            out.avisos.push(format!(
                "{}: la cuenta no tiene subcuenta contable, así que no hay saldo que declarar.",
                c.name
            ));
            continue;
        }
        let saldo = crate::umbrales::saldo(v, &c.account, fin)?;
        let medio = saldo_medio_4t(v, &c.account, year)?;
        let extinguida = en_el_anio(d.fecha_extincion, year);
        if saldo.is_zero() && medio.is_zero() && !extinguida {
            continue;
        }
        if c.currency != "EUR" {
            out.avisos.push(format!(
                "{}: está en {}; el saldo es el contable en euros. El 720 lo pide al tipo de \
                 cambio del 31 de diciembre: revisa la valoración de cierre en «Cuentas en \
                 divisa».",
                c.name, c.currency
            ));
        }
        let (fecha_apertura, estimada) = match d.fecha_apertura {
            Some(f) => (Some(f), false),
            None => (primer_apunte(v, &c.account)?, true),
        };
        let ident = if c.iban.is_empty() {
            c.number.clone()
        } else {
            c.iban.clone()
        };
        let entidad = if d.entidad.is_empty() {
            c.bank.clone()
        } else {
            d.entidad.clone()
        };
        let pais = if d.pais.is_empty() {
            pais_iban(&c.iban)
        } else {
            d.pais.clone()
        };
        let mut faltan = Vec::new();
        falta(&mut faltan, entidad.is_empty(), "entidad");
        falta(&mut faltan, d.bic.is_empty(), "BIC");
        falta(
            &mut faltan,
            d.domicilio.is_empty(),
            "domicilio de la entidad",
        );
        falta(&mut faltan, pais.is_empty(), "país");
        falta(&mut faltan, estimada, "fecha de apertura");
        out.registros.push(Registro {
            origen: Origen::Banco,
            ref_id: c.id,
            bloque: Bloque::Cuentas,
            clave: "C",
            descripcion: c.name.clone(),
            alta: alta(
                Bloque::Cuentas,
                &ident,
                &entidad,
                d.fecha_extincion,
                year,
                anterior,
            ),
            identificacion: ident,
            entidad,
            nif_entidad: d.nif_entidad.clone(),
            bic: d.bic.clone(),
            domicilio: d.domicilio.clone(),
            pais,
            condicion: d.condicion,
            porcentaje_bp: d.porcentaje_bp,
            fecha_apertura,
            fecha_estimada: estimada,
            fecha_extincion: d.fecha_extincion,
            saldo,
            saldo_medio: Some(medio),
            unidades: None,
            moneda: c.currency.clone(),
            faltan,
        });
    }
    Ok(out)
}

fn hay_cuentas_valores(v: &Vault) -> Result<bool> {
    Ok(v.connection()
        .query_row("SELECT EXISTS(SELECT 1 FROM security_account)", [], |r| {
            r.get(0)
        })?)
}

fn valores_720(v: &Vault, year: i32, anterior: Option<&Declarado>) -> Result<Bloques> {
    let mut out = Bloques {
        registros: Vec::new(),
        bienes: Vec::new(),
        avisos: Vec::new(),
    };
    if !hay_cuentas_valores(v)? {
        return Ok(out);
    }
    let cuentas = crate::valores::cuentas(v)?;
    for c in cuentas.iter().filter(|c| c.abroad) {
        out.bienes.push(BienView {
            nombre: c.name.clone(),
            extranjero: true,
            iban: String::new(),
            datos: datos_de(v, Origen::Valores, c.id)?,
        });
    }
    let r = crate::valores::report(v, year)?;
    let m = r.m720;
    if !m.sin_precio.is_empty() {
        out.avisos.push(format!(
            "Faltan precios a 31/12/{year} de {}: la valoración de los valores está incompleta.",
            m.sin_precio.join(", ")
        ));
    }
    let conn = v.connection();
    for mc in &m.cuentas {
        let Some(c) = cuentas.iter().find(|c| c.name == mc.account) else {
            continue;
        };
        let d = datos_de(v, Origen::Valores, c.id)?;
        let entidad = if d.entidad.is_empty() {
            c.name.clone()
        } else {
            d.entidad.clone()
        };
        let pais = if d.pais.is_empty() {
            c.country.clone()
        } else {
            d.pais.clone()
        };
        for (ticker, isin, qty, valor) in &mc.valores {
            let sec: Option<(i64, String, String)> = conn
                .query_row(
                    "SELECT id, kind, name FROM security
                     WHERE (?1 <> '' AND isin = ?1) OR (?1 = '' AND upper(ticker) = upper(?2))
                     ORDER BY id LIMIT 1",
                    params![isin, ticker],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            let (sec_id, kind, nombre) = sec.unwrap_or((0, String::new(), String::new()));
            let clave = if matches!(kind.as_str(), "fondo" | "etf") {
                "I"
            } else {
                "V"
            };
            let adquirido: Option<String> = conn.query_row(
                "SELECT MIN(date) FROM security_op
                 WHERE security_id = ?1 AND kind IN ('compra', 'traspaso')
                   AND (account_id = ?2 OR to_account_id = ?2)",
                params![sec_id, c.id],
                |r| r.get(0),
            )?;
            let fecha = adquirido.and_then(|s| parse_date(&s)).or(d.fecha_apertura);
            let ident = if isin.is_empty() {
                ticker.clone()
            } else {
                isin.clone()
            };
            let mut faltan = Vec::new();
            falta(&mut faltan, isin.is_empty(), "ISIN");
            falta(&mut faltan, d.nif_entidad.is_empty(), "NIF de la entidad");
            falta(
                &mut faltan,
                d.domicilio.is_empty(),
                "domicilio de la entidad",
            );
            falta(&mut faltan, pais.is_empty(), "país");
            falta(&mut faltan, fecha.is_none(), "fecha de adquisición");
            out.registros.push(Registro {
                origen: Origen::Valores,
                ref_id: c.id,
                bloque: Bloque::Valores,
                clave,
                descripcion: if nombre.is_empty() {
                    ticker.clone()
                } else {
                    format!("{nombre} ({ticker})")
                },
                alta: alta(
                    Bloque::Valores,
                    &ident,
                    &entidad,
                    d.fecha_extincion,
                    year,
                    anterior,
                ),
                identificacion: ident,
                entidad: entidad.clone(),
                nif_entidad: d.nif_entidad.clone(),
                bic: d.bic.clone(),
                domicilio: d.domicilio.clone(),
                pais: pais.clone(),
                condicion: d.condicion,
                porcentaje_bp: d.porcentaje_bp,
                fecha_apertura: fecha,
                fecha_estimada: d.fecha_apertura.is_none() || fecha != d.fecha_apertura,
                fecha_extincion: d.fecha_extincion,
                saldo: *valor,
                saldo_medio: None,
                unidades: Some(qty.to_string()),
                moneda: "EUR".into(),
                faltan,
            });
        }
    }
    Ok(out)
}

fn monedas_721(v: &Vault, year: i32, anterior: Option<&Declarado>) -> Result<Bloques> {
    let mut out = Bloques {
        registros: Vec::new(),
        bienes: Vec::new(),
        avisos: Vec::new(),
    };
    let plataformas = crate::cripto::platforms(v)?;
    for p in plataformas.iter().filter(|p| p.abroad) {
        out.bienes.push(BienView {
            nombre: p.name.clone(),
            extranjero: true,
            iban: String::new(),
            datos: datos_de(v, Origen::Cripto, p.id)?,
        });
    }
    let Some(m) = crate::cripto::saldo_extranjero(v, year)? else {
        return Ok(out);
    };
    if !m.sin_precio.is_empty() {
        out.avisos.push(format!(
            "Faltan precios de cierre a 31/12/{year} de {}: la valoración está incompleta.",
            m.sin_precio.join(", ")
        ));
    }
    if !m.sin_plataforma.is_empty() {
        out.avisos.push(format!(
            "{} sin plataforma en el punto de partida: no se sabe si están en el extranjero.",
            m.sin_plataforma.join(", ")
        ));
    }
    let conn = v.connection();
    for pe in &m.plataformas {
        let Some(p) = plataformas.iter().find(|p| p.name == pe.platform) else {
            continue;
        };
        let d = datos_de(v, Origen::Cripto, p.id)?;
        let entidad = if d.entidad.is_empty() {
            p.name.clone()
        } else {
            d.entidad.clone()
        };
        let pais = if d.pais.is_empty() {
            p.country.clone()
        } else {
            d.pais.clone()
        };
        for (ticker, qty, valor) in &pe.activos {
            let primera: Option<String> = conn.query_row(
                "SELECT MIN(date) FROM crypto_op
                 WHERE (platform_id = ?1 OR to_platform_id = ?1)
                   AND (upper(ticker) = upper(?2) OR upper(COALESCE(ticker2, '')) = upper(?2))",
                params![p.id, ticker],
                |r| r.get(0),
            )?;
            let fecha = d.fecha_apertura.or(primera.and_then(|s| parse_date(&s)));
            let mut faltan = Vec::new();
            falta(&mut faltan, d.nif_entidad.is_empty(), "NIF del custodio");
            falta(
                &mut faltan,
                d.domicilio.is_empty(),
                "domicilio del custodio",
            );
            falta(&mut faltan, pais.is_empty(), "país");
            falta(&mut faltan, valor.is_zero(), "precio de cierre");
            out.registros.push(Registro {
                origen: Origen::Cripto,
                ref_id: p.id,
                bloque: Bloque::Monedas,
                clave: "M",
                descripcion: ticker.clone(),
                alta: alta(
                    Bloque::Monedas,
                    ticker,
                    &entidad,
                    d.fecha_extincion,
                    year,
                    anterior,
                ),
                identificacion: ticker.clone(),
                entidad: entidad.clone(),
                nif_entidad: d.nif_entidad.clone(),
                bic: String::new(),
                domicilio: d.domicilio.clone(),
                pais: pais.clone(),
                condicion: d.condicion,
                porcentaje_bp: d.porcentaje_bp,
                fecha_apertura: fecha,
                fecha_estimada: d.fecha_apertura.is_none(),
                fecha_extincion: d.fecha_extincion,
                saldo: *valor,
                saldo_medio: None,
                unidades: Some(qty.to_string()),
                moneda: "EUR".into(),
                faltan,
            });
        }
    }
    Ok(out)
}

fn bloque_view(
    bloque: Bloque,
    registros: Vec<Registro>,
    anterior: Option<&Declarado>,
) -> BloqueView {
    let total: Money = registros.iter().map(|r| r.saldo).sum();
    let total_medio = (bloque == Bloque::Cuentas).then(|| {
        registros
            .iter()
            .filter_map(|r| r.saldo_medio)
            .sum::<Money>()
    });
    let actuales: BTreeSet<String> = registros
        .iter()
        .map(|r| clave_bien(bloque, &r.identificacion, &r.entidad))
        .collect();
    let extintos: BTreeSet<String> = registros
        .iter()
        .filter(|r| r.alta == Alta::C)
        .map(|r| clave_bien(bloque, &r.identificacion, &r.entidad))
        .collect();
    let d = decidir(total, total_medio, &actuales, &extintos, anterior);
    BloqueView {
        bloque,
        titulo: bloque.titulo(),
        total,
        total_medio,
        supera: d.supera,
        anterior: anterior.cloned(),
        incremento: d.incremento,
        incremento_medio: d.incremento_medio,
        extinguidos: d.extinguidos,
        obligado: d.obligado,
        motivo: d.motivo,
        registros,
    }
}

fn presentados_aeat(v: &Vault, modelo: &str) -> Result<Vec<i32>> {
    let mut st = v.connection().prepare(
        "SELECT year FROM aeat_filing WHERE model = ?1
         UNION SELECT year FROM filed_model WHERE model = ?1
         ORDER BY 1",
    )?;
    let rows = st.query_map([modelo], |r| r.get::<_, i32>(0))?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Requisitos de la exoneración de una persona jurídica: cada bien con su subcuenta, la
/// entidad, su NIF y el país (arts. 42 bis.4.a, 42 ter.4.b y 42 quater.5.b RGAT).
fn requisitos(v: &Vault, registros: &[&Registro]) -> Result<Vec<String>> {
    let conn = v.connection();
    let mut out = Vec::new();
    let mut vistos = BTreeSet::new();
    for r in registros {
        let subcuenta: String = match r.origen {
            Origen::Banco => conn.query_row(
                "SELECT account FROM bank_account WHERE id = ?1",
                [r.ref_id],
                |x| x.get(0),
            )?,
            Origen::Valores => conn
                .query_row(
                    "SELECT account FROM security WHERE isin = ?1 OR upper(ticker) = ?1
                     ORDER BY id LIMIT 1",
                    [&r.identificacion],
                    |x| x.get(0),
                )
                .optional()?
                .unwrap_or_default(),
            Origen::Cripto => conn.query_row(
                "SELECT account FROM crypto_platform WHERE id = ?1",
                [r.ref_id],
                |x| x.get(0),
            )?,
        };
        let mut f = Vec::new();
        if subcuenta.is_empty() {
            f.push("subcuenta propia");
        }
        if r.nif_entidad.is_empty() {
            f.push("NIF de la entidad");
        }
        if r.pais.is_empty() {
            f.push("país");
        }
        if r.entidad.is_empty() {
            f.push("entidad");
        }
        if !f.is_empty() && vistos.insert(format!("{}{}", r.descripcion, r.entidad)) {
            out.push(format!(
                "{} ({}): falta {}",
                r.descripcion,
                r.entidad,
                f.join(", ")
            ));
        }
    }
    Ok(out)
}

/// Borrador del 720 o del 721 del ejercicio `year` (bienes a 31 de diciembre).
pub fn borrador(v: &Vault, modelo: &str, year: i32, today: Date) -> Result<Borrador> {
    if !matches!(modelo, "720" | "721") {
        return Err(other("modelo: 720 o 721"));
    }
    let fecha = dic31(year)?;
    let plazo =
        Date::from_calendar_date(year + 1, Month::March, 31).map_err(|e| other(e.to_string()))?;
    let fisica = crate::cripto::es_persona_fisica(v);
    let declarados = declarados_de(v, modelo)?;
    let (nif, nombre) = crate::models::declarant(v)
        .map(|d| (d.nif.clone(), d.full_name()))
        .unwrap_or_default();
    let mut bloques = Vec::new();
    let mut bienes = Vec::new();
    let mut avisos = Vec::new();
    if modelo == "720" {
        let ac = anterior_de(&declarados, Bloque::Cuentas, year);
        let c = cuentas_720(v, year, ac)?;
        bienes.extend(c.bienes);
        avisos.extend(c.avisos);
        bloques.push(bloque_view(Bloque::Cuentas, c.registros, ac));
        let av = anterior_de(&declarados, Bloque::Valores, year);
        let s = valores_720(v, year, av)?;
        bienes.extend(s.bienes);
        avisos.extend(s.avisos);
        bloques.push(bloque_view(Bloque::Valores, s.registros, av));
    } else {
        let am = anterior_de(&declarados, Bloque::Monedas, year);
        let m = monedas_721(v, year, am)?;
        bienes.extend(m.bienes);
        avisos.extend(m.avisos);
        bloques.push(bloque_view(Bloque::Monedas, m.registros, am));
    }
    for b in &bloques {
        for e in &b.extinguidos {
            if !b.registros.iter().any(|r| {
                r.alta == Alta::C && clave_bien(b.bloque, &r.identificacion, &r.entidad) == *e
            }) {
                avisos.push(format!(
                    "{e} se declaró en el {} de {} y ya no aparece: si se canceló o se vendió, \
                     declárala con origen C y la fecha de extinción (pon la fecha en sus datos si \
                     sigue en TaxPain).",
                    modelo,
                    b.anterior.as_ref().map_or(0, |a| a.year)
                ));
            }
        }
    }
    let presentados = presentados_aeat(v, modelo)?;
    for y in presentados.iter().filter(|y| **y < year) {
        if !declarados.iter().any(|d| d.year == *y) {
            avisos.push(format!(
                "Consta un {modelo} de {y} presentado, pero TaxPain no tiene sus importes: \
                 tecléalos en «Lo declarado otros años» para aplicar la regla de los 20.000 €."
            ));
        }
    }
    if year >= today.year() {
        avisos.push(format!(
            "El ejercicio {year} aún no ha terminado: los saldos son los de hoy, no los del 31 \
             de diciembre."
        ));
    }
    let obligado = bloques.iter().any(|b| b.obligado);
    // Persona jurídica (por el perfil o, sin él, por el NIF).
    let exenta = !fisica;
    let todos: Vec<&Registro> = bloques.iter().flat_map(|b| b.registros.iter()).collect();
    let requisitos = if exenta {
        requisitos(v, &todos)?
    } else {
        Vec::new()
    };
    let mut notas = vec![format!(
        "Se presenta del 1 de enero al 31 de marzo de {}. Cada bloque va por separado: la primera \
         vez, si pasa de 50.000 €; después, si sube más de 20.000 € respecto de la última \
         declaración o se extingue algún bien declarado.",
        year + 1
    )];
    if modelo == "720" {
        notas.push(
            "Faltan los inmuebles (clave B) y los seguros y rentas (clave S): si los tienes, \
             añádelos en el formulario de la Sede. Las opciones y CFD no son valores del 720."
                .into(),
        );
        notas.push(
            "El efectivo que queda en una cuenta de bróker del extranjero puede ser una cuenta \
             del bloque de cuentas: revisa con el bróker si es una cuenta a tu nombre. \
             TODO(VERIFY): art. 42 bis.1 RGAT."
                .into(),
        );
    }
    if exenta {
        notas.push(format!(
            "Persona jurídica: no presenta el {modelo} de los bienes registrados en su \
             contabilidad de forma individualizada, con la entidad, su NIF y el país (arts. 42 \
             bis.4.a, 42 ter.4.b y 42 quater.5.b RGAT). {}",
            if requisitos.is_empty() {
                "Con lo que hay en TaxPain se cumple."
            } else {
                "Para cumplirlo falta lo que se indica en los requisitos."
            }
        ));
        notas.push(
            "Años en que la contabilidad la llevaba otro (la gestoría): comprueba que cada bien \
             tenía su subcuenta con la entidad y el país. Si no la tenía, había que presentar \
             el modelo y se puede presentar fuera de plazo; desde la Ley 5/2022 se aplica el \
             régimen sancionador general de la LGT (art. 198 por presentar tarde sin \
             requerimiento). TODO(VERIFY): recargos y sanción aplicables a cada año."
                .into(),
        );
    }
    Ok(Borrador {
        modelo: modelo.to_owned(),
        year,
        fecha,
        plazo,
        nif,
        nombre,
        persona_fisica: fisica,
        bloques,
        obligado,
        exenta_por_contabilidad: exenta,
        requisitos,
        presentados_aeat: presentados,
        declarados,
        bienes,
        fichero: Fichero {
            disponible: false,
            nota: if modelo == "720" {
                FICHERO_720
            } else {
                FICHERO_721
            }
            .to_owned(),
        },
        avisos,
        notas,
        norma: if modelo == "720" {
            "DA 18.ª LGT; arts. 42 bis y 42 ter RGAT; Orden HAP/72/2013"
        } else {
            "DA 18.ª LGT; art. 42 quater RGAT; Orden HFP/886/2023"
        },
    })
}
