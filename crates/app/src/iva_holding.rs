//! IVA de una sociedad con cartera o holding (carencia G28): afectación de los gastos comunes,
//! sectores diferenciados y libro de bienes de inversión con su regularización.
//!
//! TaxPain no asesora: detecta la situación, enseña el cálculo con la norma y aplica en el 303 el
//! criterio que el usuario anota. Textos cotejados el 2026-11-17 con el texto consolidado de la
//! Ley 37/1992 en el BOE (BOE-A-1992-28740).
//!
//! **Holding y operaciones no sujetas.** La mera tenencia de participaciones y el cobro de
//! dividendos no son entregas ni prestaciones de servicios (arts. 4 y 5 LIVA): los dividendos no
//! entran en la prorrata (art. 104.Tres.5.º excluye las operaciones no sujetas del art. 7) y el
//! IVA de los gastos destinados sólo a la tenencia no se deduce (art. 94.Uno: sólo se deduce el
//! de lo utilizado en operaciones que originan el derecho). Los gastos comunes a la actividad
//! sujeta y a la no sujeta se reparten con un criterio razonable y homogéneo, mantenido en el
//! tiempo, como el del art. 93.Cinco LIVA (sujetas / total de ingresos del año).
//! TODO(VERIFY): el art. 93.Cinco habla sólo de las operaciones no sujetas del art. 7.8.º (entes
//! públicos); su uso para holdings mixtas es doctrina (DGT; STJUE Larentia + Minerva,
//! C-108/14 y C-109/14) que no he cotejado.
//!
//! **Operaciones financieras.** Los préstamos a participadas son operaciones exentas (art.
//! 20.Uno.18.º LIVA). Si no son actividad habitual, no cuentan en ninguno de los términos de la
//! prorrata (art. 104.Tres.4.º, que menciona expresamente las del art. 20.Uno.18.º); si lo son,
//! van al denominador y bajan el porcentaje.
//!
//! **Sectores diferenciados** (art. 9.1.º.c.a' LIVA): actividades de grupos distintos de la CNAE
//! cuyos porcentajes de deducción difieren en más de 50 puntos del de la principal (la de mayor
//! volumen de operaciones del año anterior). La accesoria (volumen del año anterior no superior
//! al 15 % del de la principal y que contribuye a su realización) sigue a la principal. Cada
//! sector deduce por separado (art. 101 LIVA).
//!
//! **Bienes de inversión** (arts. 107 a 110 LIVA): durante los cuatro años siguientes al de la
//! compra (nueve los terrenos y edificaciones), si la prorrata definitiva del año difiere en más
//! de 10 puntos de la del año de compra, se regulariza la quinta (o décima) parte de la
//! diferencia en la declaración del último periodo del año (arts. 107.Uno, 107.Siete y 109). Si
//! se entrega el bien dentro del periodo, se regulariza de una vez por lo que quede (art. 110):
//! como usado sólo en operaciones con derecho si la entrega está sujeta y no exenta (sin superar
//! el IVA repercutido en ella), o sin derecho si está exenta o no sujeta; también sin prorrata
//! (art. 110.Dos); nunca en las transmisiones del art. 7.1.º (el adquirente se subroga, art.
//! 107.Cinco) ni después de una pérdida o inutilización no imputable (art. 107.Seis). No son
//! bienes de inversión los de valor de adquisición inferior a 3.005,06 € (art. 108.Dos.5.º).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::Origin;
use taxpain_core::money::Money;
use taxpain_models::m303::{self, ProrataUse};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, Month, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;
use crate::libros_societarios::parse_date;
use crate::models::{self, ModelSettings};
use crate::vinculadas::clave_nif;

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

/// Valor de adquisición por debajo del cual un bien no es de inversión (art. 108.Dos.5.º LIVA).
pub const MINIMO_BIEN_INVERSION: Money = Money::from_cents(300_506);

// ---------------------------------------------------------------------------------------------
// Sectores diferenciados
// ---------------------------------------------------------------------------------------------

/// Actividad anotada por el usuario para ver si forma un sector diferenciado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sector {
    #[serde(default)]
    pub id: i64,
    pub nombre: String,
    /// Código CNAE (sólo cuentan las tres primeras cifras: el grupo).
    #[serde(default)]
    pub cnae: String,
    /// Porcentaje de deducción propio de la actividad (0–100).
    pub pct: u8,
    /// Volumen de operaciones del año anterior (para saber cuál es la principal).
    #[serde(default)]
    pub volumen: Money,
    #[serde(default)]
    pub notas: String,
}

/// Qué es cada actividad frente a la principal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Papel {
    /// La de mayor volumen de operaciones del año anterior.
    Principal,
    /// Mismo grupo CNAE o porcentaje a 50 puntos o menos: mismo sector que la principal.
    MismoSector,
    /// Volumen no superior al 15 % del de la principal: si contribuye a ella, la sigue.
    Accesoria,
    /// Grupo CNAE distinto y porcentaje a más de 50 puntos: otro sector diferenciado.
    Diferenciado,
    /// Sin CNAE: no se puede comprobar.
    SinCnae,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FilaSector {
    pub id: i64,
    pub nombre: String,
    pub cnae: String,
    /// Grupo CNAE (tres cifras).
    pub grupo: String,
    pub pct: u8,
    pub volumen: Money,
    /// Diferencia de porcentaje con la principal, en puntos.
    pub diferencia: i32,
    pub papel: Papel,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ComprobacionSectores {
    pub filas: Vec<FilaSector>,
    /// Hay al menos una actividad que forma otro sector diferenciado.
    pub hay_diferenciados: bool,
    pub explicacion: String,
}

/// Grupo CNAE: las tres primeras cifras del código.
pub fn grupo_cnae(cnae: &str) -> String {
    cnae.chars().filter(char::is_ascii_digit).take(3).collect()
}

/// Comprueba los requisitos del art. 9.1.º.c.a' LIVA con las actividades anotadas.
pub fn comprobar_sectores(sectores: &[Sector]) -> ComprobacionSectores {
    let Some(principal) = sectores
        .iter()
        .enumerate()
        .max_by(|(i, a), (j, b)| a.volumen.cmp(&b.volumen).then(j.cmp(i)))
        .map(|(_, s)| s)
    else {
        return ComprobacionSectores {
            filas: vec![],
            hay_diferenciados: false,
            explicacion: "No hay actividades anotadas.".into(),
        };
    };
    let grupo_p = grupo_cnae(&principal.cnae);
    let mut filas = Vec::new();
    for s in sectores {
        let grupo = grupo_cnae(&s.cnae);
        let diferencia = (i32::from(s.pct) - i32::from(principal.pct)).abs();
        let papel = if s.id == principal.id && s.nombre == principal.nombre {
            Papel::Principal
        } else if grupo.len() < 3 || grupo_p.len() < 3 {
            Papel::SinCnae
        } else if grupo == grupo_p || diferencia <= 50 {
            Papel::MismoSector
        } else if i128::from(s.volumen.cents()) * 100 <= i128::from(principal.volumen.cents()) * 15
        {
            Papel::Accesoria
        } else {
            Papel::Diferenciado
        };
        filas.push(FilaSector {
            id: s.id,
            nombre: s.nombre.clone(),
            cnae: s.cnae.clone(),
            grupo,
            pct: s.pct,
            volumen: s.volumen,
            diferencia,
            papel,
        });
    }
    let hay = filas.iter().any(|f| f.papel == Papel::Diferenciado);
    let explicacion = if hay {
        format!(
            "Hay actividades de otro grupo de la CNAE con un porcentaje de deducción a más de 50 \
             puntos del de la principal («{}», {} %): forman otro sector diferenciado y deducen \
             por separado (arts. 9.1.º.c.a' y 101 LIVA). Todas las que estén en ese caso forman \
             un único sector distinto del de la principal.",
            principal.nombre, principal.pct
        )
    } else {
        format!(
            "Con lo anotado no hay sectores diferenciados: todas las actividades siguen a la \
             principal («{}», {} %) y se aplica un solo porcentaje.",
            principal.nombre, principal.pct
        )
    };
    ComprobacionSectores {
        filas,
        hay_diferenciados: hay,
        explicacion,
    }
}

pub fn sectores(v: &Vault) -> Result<Vec<Sector>> {
    let mut stmt = v.connection().prepare(
        "SELECT id, nombre, cnae, pct, volumen, notas FROM iva_sector ORDER BY volumen DESC, id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Sector {
            id: r.get(0)?,
            nombre: r.get(1)?,
            cnae: r.get(2)?,
            pct: u8::try_from(r.get::<_, i64>(3)?.clamp(0, 100)).unwrap_or(100),
            volumen: Money::from_cents(r.get(4)?),
            notas: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Alta (id 0) o cambio de una actividad. Devuelve su id.
pub fn guardar_sector(v: &Vault, s: &Sector, now: OffsetDateTime) -> Result<i64> {
    if s.nombre.trim().is_empty() {
        return Err(other("pon un nombre a la actividad"));
    }
    if s.pct > 100 {
        return Err(other("el porcentaje de deducción va de 0 a 100"));
    }
    if s.volumen.cents() < 0 {
        return Err(other("el volumen de operaciones no puede ser negativo"));
    }
    let cnae = s.cnae.trim();
    if !cnae.is_empty() && grupo_cnae(cnae).len() < 3 {
        return Err(other("el CNAE necesita al menos tres cifras (el grupo)"));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let at = now_str(now);
    let id = if s.id == 0 {
        tx.execute(
            "INSERT INTO iva_sector(nombre, cnae, pct, volumen, notas, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                s.nombre.trim(),
                cnae,
                s.pct,
                s.volumen.cents(),
                s.notas.trim(),
                at
            ],
        )?;
        tx.last_insert_rowid()
    } else {
        let n = tx.execute(
            "UPDATE iva_sector SET nombre = ?2, cnae = ?3, pct = ?4, volumen = ?5, notas = ?6,
                    updated_at = ?7 WHERE id = ?1",
            params![
                s.id,
                s.nombre.trim(),
                cnae,
                s.pct,
                s.volumen.cents(),
                s.notas.trim(),
                at
            ],
        )?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        s.id
    };
    audit::append(
        &tx,
        &at,
        Actor::User,
        "iva.sector",
        &id.to_string(),
        &serde_json::json!({
            "nombre": s.nombre.trim(), "cnae": cnae, "pct": s.pct, "volumen": s.volumen.cents()
        }),
    )?;
    tx.commit()?;
    Ok(id)
}

/// Borra una actividad (pide confirmación): las reglas que la usaban se quedan sin sector.
pub fn borrar_sector(v: &Vault, id: i64, confirmed: bool, now: OffsetDateTime) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE iva_afectacion SET sector_id = NULL WHERE sector_id = ?1",
        [id],
    )?;
    if tx.execute("DELETE FROM iva_sector WHERE id = ?1", [id])? == 0 {
        return Err(AppError::NotFound);
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "iva.sector.borrar",
        &id.to_string(),
        &serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}

/// Lo que el usuario decide aplicar.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Deducir por separado en cada sector diferenciado (las reglas con sector llevan su
    /// porcentaje y no la prorrata general).
    #[serde(default)]
    pub sectores_diferenciados: bool,
}

const CLAVE_CONFIG: &str = "iva_holding";

pub fn config(v: &Vault) -> Result<Config> {
    Ok(v.setting::<Config>(CLAVE_CONFIG)?.unwrap_or_default())
}

pub fn guardar_config(v: &Vault, c: &Config, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    v.set_setting(CLAVE_CONFIG, c)?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "iva.config",
        "sectores",
        &serde_json::json!({ "sectores_diferenciados": c.sectores_diferenciados }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Aviso para el 303: hay sectores diferenciados con lo anotado y no se separan.
pub(crate) fn aviso_sectores(v: &Vault) -> Result<Option<String>> {
    if config(v)?.sectores_diferenciados {
        return Ok(None);
    }
    let lista = sectores(v)?;
    if lista.len() < 2 || !comprobar_sectores(&lista).hay_diferenciados {
        return Ok(None);
    }
    Ok(Some(
        "Con las actividades anotadas en Contabilidad → IVA y libros hay sectores diferenciados \
         (art. 9.1.º.c LIVA) y la deducción no se separa por sector (art. 101 LIVA): revísalo \
         allí."
            .into(),
    ))
}

// ---------------------------------------------------------------------------------------------
// Reglas de afectación
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ambito {
    /// Todas las recibidas de un proveedor (por su NIF).
    Proveedor,
    /// Las recibidas cuya cuenta de gasto principal empieza por la clave (6230, 629…).
    Cuenta,
}

impl Ambito {
    fn code(self) -> &'static str {
        match self {
            Ambito::Proveedor => "proveedor",
            Ambito::Cuenta => "cuenta",
        }
    }
    fn parse(s: &str) -> Ambito {
        if s == "cuenta" {
            Ambito::Cuenta
        } else {
            Ambito::Proveedor
        }
    }
}

/// Porcentaje de la cuota que se deduce de las recibidas de un proveedor o una cuenta, con el
/// criterio que lo justifica.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Regla {
    #[serde(default)]
    pub id: i64,
    pub ambito: Ambito,
    /// NIF del proveedor o prefijo de la cuenta.
    pub clave: String,
    /// Porcentaje deducible (0–100).
    pub pct: u8,
    /// Sector diferenciado al que se lleva la compra (sólo si se separan los sectores).
    #[serde(default)]
    pub sector_id: Option<i64>,
    /// Por qué: el criterio de reparto (art. 93.Cinco LIVA, mantenido en el tiempo).
    pub criterio: String,
    /// Se aplica a las recibidas registradas desde este día.
    pub desde: Date,
}

/// Clave normalizada: NIF en mayúsculas sin signos, o las cifras de la cuenta.
fn normalizar(ambito: Ambito, clave: &str) -> Result<String> {
    match ambito {
        Ambito::Proveedor => {
            let k = clave_nif(clave);
            if k.len() < 8 {
                return Err(other("el NIF del proveedor no parece completo"));
            }
            Ok(k)
        }
        Ambito::Cuenta => {
            let k: String = clave.chars().filter(|c| !c.is_whitespace()).collect();
            if k.len() < 2
                || k.len() > 12
                || !k.chars().all(|c| c.is_ascii_digit())
                || !(k.starts_with('6') || k.starts_with('2'))
            {
                return Err(other(
                    "la cuenta va con sus cifras y debe ser de gasto (6) o de inmovilizado (2)",
                ));
            }
            Ok(k)
        }
    }
}

pub fn reglas(v: &Vault) -> Result<Vec<Regla>> {
    let mut stmt = v.connection().prepare(
        "SELECT id, ambito, clave, pct, sector_id, criterio, desde FROM iva_afectacion
         ORDER BY ambito DESC, clave",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, Option<i64>>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, ambito, clave, pct, sector_id, criterio, desde) = row?;
        out.push(Regla {
            id,
            ambito: Ambito::parse(&ambito),
            clave,
            pct: u8::try_from(pct.clamp(0, 100)).unwrap_or(100),
            sector_id,
            criterio,
            desde: parse_date(&desde)?,
        });
    }
    Ok(out)
}

/// Alta (id 0) o cambio de una regla. Devuelve su id. Queda en la auditoría con el criterio.
pub fn guardar_regla(v: &Vault, r: &Regla, now: OffsetDateTime) -> Result<i64> {
    let clave = normalizar(r.ambito, &r.clave)?;
    if r.pct > 100 {
        return Err(other("el porcentaje deducible va de 0 a 100"));
    }
    if r.criterio.trim().len() < 10 {
        return Err(other(
            "explica el criterio de reparto (p. ej. «proporción de ingresos sujetos sobre el \
             total del año anterior»): queda documentado",
        ));
    }
    let conn = v.connection();
    if let Some(s) = r.sector_id {
        let existe: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM iva_sector WHERE id = ?1)",
            [s],
            |x| x.get(0),
        )?;
        if !existe {
            return Err(other("esa actividad ya no existe"));
        }
    }
    let repetida: Option<i64> = conn
        .query_row(
            "SELECT id FROM iva_afectacion WHERE ambito = ?1 AND clave = ?2 AND id <> ?3",
            params![r.ambito.code(), clave, r.id],
            |x| x.get(0),
        )
        .optional()?;
    if repetida.is_some() {
        return Err(other(format!(
            "ya hay una regla para {} {clave}: cámbiala en vez de añadir otra",
            r.ambito.code()
        )));
    }
    let tx = conn.unchecked_transaction()?;
    let at = now_str(now);
    let id = if r.id == 0 {
        tx.execute(
            "INSERT INTO iva_afectacion(ambito, clave, pct, sector_id, criterio, desde,
                                        created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
            params![
                r.ambito.code(),
                clave,
                r.pct,
                r.sector_id,
                r.criterio.trim(),
                r.desde.to_string(),
                at
            ],
        )?;
        tx.last_insert_rowid()
    } else {
        let n = tx.execute(
            "UPDATE iva_afectacion SET ambito = ?2, clave = ?3, pct = ?4, sector_id = ?5,
                    criterio = ?6, desde = ?7, updated_at = ?8 WHERE id = ?1",
            params![
                r.id,
                r.ambito.code(),
                clave,
                r.pct,
                r.sector_id,
                r.criterio.trim(),
                r.desde.to_string(),
                at
            ],
        )?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        r.id
    };
    audit::append(
        &tx,
        &at,
        Actor::User,
        "iva.afectacion",
        &format!("{} {clave}", r.ambito.code()),
        &serde_json::json!({
            "id": id, "pct": r.pct, "sector": r.sector_id,
            "criterio": r.criterio.trim(), "desde": r.desde.to_string()
        }),
    )?;
    tx.commit()?;
    Ok(id)
}

/// Borra una regla (pide confirmación). Los 303 que se calculen después ya no la aplican.
pub fn borrar_regla(v: &Vault, id: i64, confirmed: bool, now: OffsetDateTime) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let antes: Option<(String, String, i64)> = tx
        .query_row(
            "SELECT ambito, clave, pct FROM iva_afectacion WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((ambito, clave, pct)) = antes else {
        return Err(AppError::NotFound);
    };
    tx.execute("DELETE FROM iva_afectacion WHERE id = ?1", [id])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "iva.afectacion.borrar",
        &format!("{ambito} {clave}"),
        &serde_json::json!({ "id": id, "pct": pct }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Resultado de aplicar las reglas a una recibida.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Aplicada {
    /// Porcentaje deducible que va al 303.
    pub pct: u8,
    /// Lleva el porcentaje de su sector diferenciado: sin prorrata general.
    pub fuera_prorrata: bool,
    pub regla: Option<i64>,
}

/// La regla que toca a una recibida: la del proveedor y, si no la hay, la de la cuenta con el
/// prefijo más largo; sólo si la recibida se registró desde el día de la regla.
pub fn regla_para<'a>(
    reglas: &'a [Regla],
    nif: &str,
    cuenta: Option<&str>,
    registro: Date,
) -> Option<&'a Regla> {
    let nif = clave_nif(nif);
    let vigentes = || reglas.iter().filter(|r| r.desde <= registro);
    vigentes()
        .find(|r| r.ambito == Ambito::Proveedor && !nif.is_empty() && r.clave == nif)
        .or_else(|| {
            let c = cuenta?;
            vigentes()
                .filter(|r| r.ambito == Ambito::Cuenta && c.starts_with(&r.clave))
                .max_by_key(|r| r.clave.len())
        })
}

/// Porcentaje que va al 303: el menor del de la factura y el de la regla (si la factura ya
/// tenía uno menor, p. ej. un vehículo al 50 %, se queda) y, si la regla lleva la compra a un
/// sector diferenciado que se separa, por el porcentaje de ese sector.
pub fn aplicar_regla(
    regla: Option<&Regla>,
    sectores: &BTreeMap<i64, u8>,
    separar: bool,
    pct_factura: u8,
) -> Aplicada {
    let Some(r) = regla else {
        return Aplicada {
            pct: pct_factura,
            fuera_prorrata: false,
            regla: None,
        };
    };
    let base = pct_factura.min(r.pct);
    let sector = r
        .sector_id
        .filter(|_| separar)
        .and_then(|s| sectores.get(&s));
    match sector {
        Some(sp) => {
            let pct = (u16::from(base) * u16::from(*sp) + 50) / 100;
            Aplicada {
                pct: u8::try_from(pct.min(100)).unwrap_or(100),
                fuera_prorrata: true,
                regla: Some(r.id),
            }
        }
        None => Aplicada {
            pct: base,
            fuera_prorrata: false,
            regla: Some(r.id),
        },
    }
}

/// Cuenta de gasto (6) o de inmovilizado (2) con más importe en el asiento de la recibida.
fn cuenta_principal(conn: &Connection, recibida: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT l.account FROM journal_line l JOIN journal_entry e ON e.id = l.entry_id
              WHERE e.origin_kind = 'FacturaRecibida' AND e.origin_id = ?1 AND l.debit > 0
                AND (l.account LIKE '6%' OR l.account LIKE '2%')
              ORDER BY l.debit DESC, l.id LIMIT 1",
            [recibida],
            |r| r.get::<_, String>(0),
        )
        .optional()?)
}

/// Reglas y sectores cargados una vez para un cálculo del 303 o del 390.
#[derive(Debug)]
pub(crate) struct Afectacion {
    reglas: Vec<Regla>,
    sectores: BTreeMap<i64, u8>,
    separar: bool,
}

impl Afectacion {
    pub(crate) fn load(v: &Vault) -> Result<Self> {
        let reglas = reglas(v)?;
        if reglas.is_empty() {
            return Ok(Afectacion {
                reglas,
                sectores: BTreeMap::new(),
                separar: false,
            });
        }
        Ok(Afectacion {
            reglas,
            sectores: sectores(v)?.into_iter().map(|s| (s.id, s.pct)).collect(),
            separar: config(v)?.sectores_diferenciados,
        })
    }

    pub(crate) fn aplicar(
        &self,
        v: &Vault,
        recibida: i64,
        nif: &str,
        registro: Date,
        pct: u8,
    ) -> Result<Aplicada> {
        if self.reglas.is_empty() {
            return Ok(aplicar_regla(None, &self.sectores, false, pct));
        }
        let por_cuenta = self.reglas.iter().any(|r| r.ambito == Ambito::Cuenta);
        let cuenta = if por_cuenta {
            cuenta_principal(v.connection(), recibida)?
        } else {
            None
        };
        let regla = regla_para(&self.reglas, nif, cuenta.as_deref(), registro);
        Ok(aplicar_regla(regla, &self.sectores, self.separar, pct))
    }
}

/// Una recibida del año a la que se aplica una regla (lo que queda documentado).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Aplicacion {
    pub recibida_id: i64,
    pub numero: String,
    pub proveedor: String,
    pub nif: String,
    pub registro: Date,
    pub cuota: Money,
    pub pct_factura: u8,
    pub pct_aplicado: u8,
    pub regla_id: i64,
    pub criterio: String,
    pub sector: Option<String>,
    /// IVA que deja de deducirse por la regla (antes de la prorrata).
    pub no_deducido: Money,
}

/// Recibidas revisadas del año (por fecha de registro) con su cuota.
struct Recibida {
    id: i64,
    nif: String,
    nombre: String,
    numero: String,
    registro: Date,
    base: Money,
    cuota: Money,
    pct: u8,
    property: bool,
    uso: ProrataUse,
}

fn recibidas(v: &Vault, desde: i32, hasta: i32, solo_bienes: bool) -> Result<Vec<Recibida>> {
    let filtro = if solo_bienes {
        "AND capital_good = 1"
    } else {
        ""
    };
    let mut stmt = v.connection().prepare(&format!(
        "SELECT id, COALESCE(supplier_nif, ''), COALESCE(supplier_name, ''), COALESCE(number, ''),
                reviewed_at, COALESCE(base, 0), COALESCE(vat, 0), deductible_pct, capital_good,
                property, prorata_use, vat_regime, self_assessed_rate
           FROM invoice_received
          WHERE status = 'revisada' AND reviewed_at IS NOT NULL
            AND CAST(substr(reviewed_at, 1, 4) AS INTEGER) BETWEEN ?1 AND ?2 {filtro}
          ORDER BY reviewed_at, id"
    ))?;
    let rows = stmt.query_map([desde - 1, hasta + 1], |r| {
        Ok((
            (
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ),
            (
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, bool>(8)?,
                r.get::<_, bool>(9)?,
                r.get::<_, String>(10)?,
                r.get::<_, String>(11)?,
                r.get::<_, Option<i64>>(12)?,
            ),
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let ((id, nif, nombre, numero, at), (base, vat, pct, _cg, prop, uso, regimen, tipo)) = row?;
        let registro = models::local_date(&at)?;
        if registro.year() < desde || registro.year() > hasta {
            continue;
        }
        let base = Money::from_cents(base);
        // Con inversión del sujeto pasivo la factura no lleva IVA: la cuota es la autorrepercutida.
        let cuota = if regimen == "interior" {
            Money::from_cents(vat)
        } else {
            let bp = i32::try_from(tipo.unwrap_or(2100)).unwrap_or(2100);
            base.apply_rate(taxpain_core::money::Rate::from_bp(bp))
        };
        out.push(Recibida {
            id,
            nif,
            nombre,
            numero,
            registro,
            base,
            cuota,
            pct: u8::try_from(pct.clamp(0, 100)).unwrap_or(100),
            property: prop,
            uso: ProrataUse::parse(&uso),
        });
    }
    Ok(out)
}

/// Las recibidas del año a las que se aplica una regla, con lo que deja de deducirse.
pub fn aplicaciones(v: &Vault, year: i32) -> Result<Vec<Aplicacion>> {
    let afect = Afectacion::load(v)?;
    if afect.reglas.is_empty() {
        return Ok(vec![]);
    }
    let nombres: BTreeMap<i64, String> =
        sectores(v)?.into_iter().map(|s| (s.id, s.nombre)).collect();
    let mut out = Vec::new();
    for r in recibidas(v, year, year, false)? {
        let ap = afect.aplicar(v, r.id, &r.nif, r.registro, r.pct)?;
        let Some(regla_id) = ap.regla else {
            continue;
        };
        let Some(regla) = afect.reglas.iter().find(|x| x.id == regla_id) else {
            continue;
        };
        let antes = r
            .cuota
            .apply_rate(taxpain_core::money::Rate::percent(i32::from(r.pct)));
        let despues = r
            .cuota
            .apply_rate(taxpain_core::money::Rate::percent(i32::from(ap.pct)));
        out.push(Aplicacion {
            recibida_id: r.id,
            numero: r.numero,
            proveedor: r.nombre,
            nif: r.nif,
            registro: r.registro,
            cuota: r.cuota,
            pct_factura: r.pct,
            pct_aplicado: ap.pct,
            regla_id,
            criterio: regla.criterio.clone(),
            sector: if ap.fuera_prorrata {
                regla.sector_id.and_then(|s| nombres.get(&s).cloned())
            } else {
                None
            },
            no_deducido: antes - despues,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// Bienes de inversión
// ---------------------------------------------------------------------------------------------

/// Cómo tributó la entrega de un bien de inversión.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VentaIva {
    /// Sujeta y no exenta (o exenta con derecho a deducir, como una exportación).
    Sujeta,
    /// Exenta sin derecho a deducir (p. ej. segunda entrega de una edificación sin renuncia).
    Exenta,
    /// No sujeta (salvo la transmisión de un patrimonio empresarial).
    NoSujeta,
    /// Transmisión de un patrimonio empresarial o profesional (art. 7.1.º): el adquirente se
    /// subroga y no se regulariza (art. 107.Cinco).
    TransmisionEmpresa,
    /// Pérdida o inutilización definitiva no imputable al sujeto pasivo: no se regulariza en los
    /// años siguientes (art. 107.Seis).
    Perdida,
}

impl VentaIva {
    fn code(self) -> &'static str {
        match self {
            VentaIva::Sujeta => "sujeta",
            VentaIva::Exenta => "exenta",
            VentaIva::NoSujeta => "no_sujeta",
            VentaIva::TransmisionEmpresa => "transmision_empresa",
            VentaIva::Perdida => "perdida",
        }
    }
    fn parse(s: &str) -> Option<VentaIva> {
        Some(match s {
            "sujeta" => VentaIva::Sujeta,
            "exenta" => VentaIva::Exenta,
            "no_sujeta" => VentaIva::NoSujeta,
            "transmision_empresa" => VentaIva::TransmisionEmpresa,
            "perdida" => VentaIva::Perdida,
            _ => return None,
        })
    }
}

/// Años del periodo de regularización contando el de la compra (art. 107.Uno).
pub fn anios_periodo(inmueble: bool) -> i32 {
    if inmueble { 10 } else { 5 }
}

/// Lo que necesita el cálculo de un bien.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BienCalculo {
    /// Año en que se soportó (y dedujo) la cuota.
    pub adquirido: i32,
    pub inmueble: bool,
    /// Cuota soportada por el porcentaje deducible de la factura (antes de la prorrata).
    pub deduccion_base: Money,
    /// Porcentaje con que se dedujo el año de compra (prorrata definitiva de ese año, o 100 / 0
    /// con la especial según su destino). `None` si no se sabe.
    pub referencia: Option<u8>,
    /// Entrega: fecha, cómo tributó (si se sabe) e IVA repercutido en ella.
    pub venta: Option<(Date, Option<VentaIva>, Money)>,
}

/// Regularización anual del año `year` con la prorrata definitiva `def` (arts. 107 y 109): la
/// diferencia en puntos y el importe (cero si no pasa de 10 puntos). `None` si el año no está en
/// el periodo, si el bien ya se entregó o si no se sabe la referencia.
pub fn regularizacion_anual_de(b: &BienCalculo, year: i32, def: u8) -> Option<(i32, Money)> {
    if year <= b.adquirido || year > b.adquirido + anios_periodo(b.inmueble) - 1 {
        return None;
    }
    if b.venta.is_some_and(|(f, _, _)| f.year() <= year) {
        return None;
    }
    let referencia = b.referencia?;
    let diff = i32::from(def.min(100)) - i32::from(referencia);
    if diff.abs() <= 10 {
        return Some((diff, Money::ZERO));
    }
    let anios = i128::from(anios_periodo(b.inmueble));
    Some((
        diff,
        models::scale(b.deduccion_base, i128::from(diff), 100 * anios),
    ))
}

/// Años del periodo que quedan desde el de la entrega (incluido). Si se entrega el mismo año de
/// la compra, cuentan los siguientes: ese año ya se ajusta con la prorrata definitiva [44].
/// TODO(VERIFY): entrega en el mismo año de la compra.
pub fn anios_restantes(b: &BienCalculo, entrega: i32) -> i32 {
    let fin = b.adquirido + anios_periodo(b.inmueble) - 1;
    if entrega > fin {
        return 0;
    }
    fin - entrega.max(b.adquirido + 1) + 1
}

/// Regularización única por la entrega del bien (art. 110): `None` si no se entregó dentro del
/// periodo, no se sabe cómo tributó o no procede (art. 7.1.º o pérdida).
pub fn regularizacion_venta_de(b: &BienCalculo) -> Option<Money> {
    let (fecha, tipo, iva) = b.venta?;
    let objetivo: i32 = match tipo? {
        VentaIva::Sujeta => 100,
        VentaIva::Exenta | VentaIva::NoSujeta => 0,
        VentaIva::TransmisionEmpresa | VentaIva::Perdida => return None,
    };
    let restantes = anios_restantes(b, fecha.year());
    if restantes <= 0 {
        return None;
    }
    let referencia = b.referencia?;
    let anios = i128::from(anios_periodo(b.inmueble));
    let reg = models::scale(
        b.deduccion_base,
        i128::from((objetivo - i32::from(referencia)) * restantes),
        100 * anios,
    );
    // Sujeta y no exenta: la deducción complementaria no supera el IVA repercutido en la
    // entrega (art. 110.Uno.1.º).
    Some(if tipo == Some(VentaIva::Sujeta) {
        reg.min(iva)
    } else {
        reg
    })
}

/// Un bien de inversión con sus datos de IVA.
#[derive(Clone, Debug)]
struct Bien {
    asset_id: Option<i64>,
    received_id: Option<i64>,
    nombre: String,
    factura: String,
    nif: String,
    registro: Date,
    inmueble: bool,
    base: Money,
    cuota: Money,
    pct: u8,
    uso: ProrataUse,
    venta: Option<(Date, Option<VentaIva>, Money)>,
    origen_venta: String,
}

impl Bien {
    fn calculo(&self, referencia: Option<u8>) -> BienCalculo {
        BienCalculo {
            adquirido: self.registro.year(),
            inmueble: self.inmueble,
            deduccion_base: self
                .cuota
                .apply_rate(taxpain_core::money::Rate::percent(i32::from(self.pct))),
            referencia,
            venta: self.venta,
        }
    }
    /// Por debajo del mínimo del art. 108.Dos.5.º no es bien de inversión.
    fn es_bien_de_inversion(&self) -> bool {
        self.base >= MINIMO_BIEN_INVERSION
    }
}

struct Activo {
    id: i64,
    nombre: String,
    cuenta: String,
    adquirido: Date,
    baja: Option<Date>,
}

struct DatoManual {
    received_id: Option<i64>,
    cuota: Option<Money>,
    deducible: Option<u8>,
    inmueble: bool,
    venta: Option<VentaIva>,
    iva_venta: Option<Money>,
}

fn activos(conn: &Connection) -> Result<Vec<Activo>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, account, acquired_on, disposed_on
           FROM fixed_asset ORDER BY acquired_on, id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, nombre, cuenta, adq, baja) = row?;
        out.push(Activo {
            id,
            nombre,
            cuenta,
            adquirido: parse_date(&adq)?,
            baja: baja.as_deref().map(parse_date).transpose()?,
        });
    }
    Ok(out)
}

fn datos_manuales(conn: &Connection) -> Result<BTreeMap<i64, DatoManual>> {
    let mut stmt = conn.prepare(
        "SELECT asset_id, received_id, cuota, deducible, inmueble, venta, iva_venta
           FROM iva_bien_inversion",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            DatoManual {
                received_id: r.get(1)?,
                cuota: r.get::<_, Option<i64>>(2)?.map(Money::from_cents),
                deducible: r
                    .get::<_, Option<i64>>(3)?
                    .map(|p| u8::try_from(p.clamp(0, 100)).unwrap_or(100)),
                inmueble: r.get(4)?,
                venta: r
                    .get::<_, Option<String>>(5)?
                    .as_deref()
                    .and_then(VentaIva::parse),
                iva_venta: r.get::<_, Option<i64>>(6)?.map(Money::from_cents),
            },
        ))
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Entrega de un bien: fecha, cómo tributó (si se sabe), IVA repercutido y de dónde sale.
type EntregaConOrigen = (Date, Option<VentaIva>, Money, String);

/// Cómo tributó la venta de un activo: lo anotado a mano o lo que dice su factura de venta.
fn venta_de(
    v: &Vault,
    a: &Activo,
    manual: Option<&DatoManual>,
) -> Result<Option<EntregaConOrigen>> {
    let Some(baja) = a.baja else {
        return Ok(None);
    };
    if let Some(t) = manual.and_then(|m| m.venta) {
        let iva = manual.and_then(|m| m.iva_venta).unwrap_or_default();
        return Ok(Some((baja, Some(t), iva, "anotado a mano".into())));
    }
    let facturas: Vec<(i64, i64)> = {
        let mut stmt = v.connection().prepare(
            "SELECT l.invoice_id, l.amount FROM asset_sale_invoice l
               JOIN invoice_issued i ON i.id = l.invoice_id
              WHERE l.asset_id = ?1 AND i.status = 'emitida' AND i.kind NOT LIKE 'R%'",
        )?;
        let rows = stmt.query_map([a.id], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let Some((factura, parte)) = facturas.first().copied() else {
        return Ok(Some((baja, None, Money::ZERO, String::new())));
    };
    let Some((con_derecho, iva, exenta, no_sujeta)) = models::sale_invoice_vat(v, factura)? else {
        return Ok(Some((baja, None, Money::ZERO, String::new())));
    };
    let parte = Money::from_cents(parte);
    let tipo = if con_derecho >= exenta && con_derecho >= no_sujeta && !con_derecho.is_zero() {
        VentaIva::Sujeta
    } else if exenta >= no_sujeta {
        VentaIva::Exenta
    } else {
        VentaIva::NoSujeta
    };
    // El IVA de la parte de la factura que corresponde al bien.
    let iva = if con_derecho.is_zero() {
        Money::ZERO
    } else {
        models::scale(
            iva,
            i128::from(parte.cents().min(con_derecho.cents())),
            i128::from(con_derecho.cents()),
        )
    };
    Ok(Some((baja, Some(tipo), iva, "factura de venta".into())))
}

/// Los bienes de inversión con cuota soportada entre `desde` y `hasta`: los de las recibidas
/// marcadas como bien de inversión (con su elemento del inmovilizado, si lo hay) y los elementos
/// con la cuota anotada a mano.
fn bienes(v: &Vault, desde: i32, hasta: i32) -> Result<Vec<Bien>> {
    let conn = v.connection();
    let activos = activos(conn)?;
    let manual = datos_manuales(conn)?;
    let por_recibida: BTreeMap<i64, i64> = manual
        .iter()
        .filter_map(|(a, m)| m.received_id.map(|r| (r, *a)))
        .collect();
    let enlazados: BTreeSet<i64> = por_recibida.values().copied().collect();
    let mut out = Vec::new();
    let mut usados = BTreeSet::new();
    for r in recibidas(v, desde, hasta, true)? {
        // El elemento: el enlazado o, para los de antes de enlazarlos, el del mismo nombre.
        let nombre_activo = format!("{} · {}", r.nombre.trim(), r.numero.trim());
        let activo = por_recibida
            .get(&r.id)
            .and_then(|id| activos.iter().find(|a| a.id == *id))
            .or_else(|| {
                activos.iter().find(|a| {
                    a.nombre == nombre_activo
                        && !enlazados.contains(&a.id)
                        && !usados.contains(&a.id)
                })
            });
        if let Some(a) = activo {
            usados.insert(a.id);
        }
        let m = activo.and_then(|a| manual.get(&a.id));
        let venta = match activo {
            Some(a) => venta_de(v, a, m)?,
            None => None,
        };
        out.push(Bien {
            asset_id: activo.map(|a| a.id),
            received_id: Some(r.id),
            nombre: activo.map_or(nombre_activo.clone(), |a| a.nombre.clone()),
            factura: r.numero,
            nif: r.nif,
            registro: r.registro,
            inmueble: r.property || m.is_some_and(|m| m.inmueble),
            base: r.base,
            cuota: r.cuota,
            pct: r.pct,
            uso: r.uso,
            venta: venta.as_ref().map(|(f, t, i, _)| (*f, *t, *i)),
            origen_venta: venta.map(|x| x.3).unwrap_or_default(),
        });
    }
    // Elementos sin recibida con la cuota anotada (p. ej. de los saldos iniciales).
    for a in &activos {
        let Some(m) = manual.get(&a.id) else {
            continue;
        };
        let Some(cuota) = m.cuota else {
            continue;
        };
        if m.received_id.is_some() || usados.contains(&a.id) {
            continue;
        }
        if a.adquirido.year() < desde || a.adquirido.year() > hasta {
            continue;
        }
        let venta = venta_de(v, a, Some(m))?;
        out.push(Bien {
            asset_id: Some(a.id),
            received_id: None,
            nombre: a.nombre.clone(),
            factura: String::new(),
            nif: String::new(),
            registro: a.adquirido,
            inmueble: m.inmueble || a.cuenta.starts_with("211") || a.cuenta.starts_with("210"),
            // Sin la base de la factura se toma el valor del elemento.
            base: cuota_base(conn, a.id)?,
            cuota,
            pct: m.deducible.unwrap_or(100),
            uso: ProrataUse::Comun,
            venta: venta.as_ref().map(|(f, t, i, _)| (*f, *t, *i)),
            origen_venta: venta.map(|x| x.3).unwrap_or_default(),
        });
    }
    Ok(out)
}

/// Coste del elemento (para el mínimo del art. 108.Dos.5.º cuando no hay factura).
fn cuota_base(conn: &Connection, asset: i64) -> Result<Money> {
    Ok(Money::from_cents(conn.query_row(
        "SELECT cost FROM fixed_asset WHERE id = ?1",
        [asset],
        |r| r.get::<_, i64>(0),
    )?))
}

/// Prorratas definitivas por año, calculadas una vez.
struct Prorratas<'a> {
    v: &'a Vault,
    s: &'a ModelSettings,
    cache: BTreeMap<i32, Option<u8>>,
}

impl<'a> Prorratas<'a> {
    fn new(v: &'a Vault, s: &'a ModelSettings) -> Self {
        Prorratas {
            v,
            s,
            cache: BTreeMap::new(),
        }
    }
    fn definitiva(&mut self, year: i32) -> Result<Option<u8>> {
        if let Some(p) = self.cache.get(&year) {
            return Ok(*p);
        }
        let p = models::definitive_prorata(self.v, self.s, year)?;
        self.cache.insert(year, p);
        Ok(p)
    }
    /// Porcentaje con que se dedujo el bien el año de compra: sin prorrata, el 100 % (o el
    /// anotado para ese año); con la especial, el 100 % o el 0 % si era de uso exclusivo.
    fn referencia(&mut self, b: &Bien) -> Result<Option<u8>> {
        let year = b.registro.year();
        if self.s.prorata.is_none() {
            return Ok(Some(
                self.s.prorata_history.get(&year).copied().unwrap_or(100),
            ));
        }
        if self.s.prorata_special {
            match b.uso {
                ProrataUse::Plena => return Ok(Some(100)),
                ProrataUse::Ninguna => return Ok(Some(0)),
                ProrataUse::Comun => {}
            }
        }
        self.definitiva(year)
    }
}

/// [43] del último periodo de `year`: regularización anual de los bienes de inversión de años
/// anteriores con la prorrata definitiva `definitive` (sólo con prorrata, la llama
/// `models::prorata_for`). Con la especial, sólo los de uso común. Devuelve importe y avisos.
pub(crate) fn regularizacion_anual(
    v: &Vault,
    s: &ModelSettings,
    year: i32,
    definitive: u8,
) -> Result<(Money, Vec<String>)> {
    let mut total = Money::ZERO;
    let mut avisos = Vec::new();
    let mut sin_prorrata = BTreeSet::new();
    let mut p = Prorratas::new(v, s);
    for b in bienes(v, year - 9, year - 1)? {
        if !b.es_bien_de_inversion() {
            continue;
        }
        if s.prorata_special && b.uso != ProrataUse::Comun {
            continue;
        }
        let adq = b.registro.year();
        if !(b.inmueble || year - adq <= 4) {
            continue;
        }
        if b.venta.is_some_and(|(f, _, _)| f.year() <= year) {
            continue;
        }
        let Some(referencia) = p.definitiva(adq)? else {
            if sin_prorrata.insert(adq) {
                avisos.push(format!(
                    "Hay bienes de inversión de {adq} pero no sé la prorrata definitiva de ese año: anótala en Ajustes de modelos para regularizar [43]."
                ));
            }
            continue;
        };
        if let Some((_, m)) =
            regularizacion_anual_de(&b.calculo(Some(referencia)), year, definitive)
        {
            total += m;
        }
    }
    Ok((total, avisos))
}

/// [43] del periodo de `start` a `end`: regularización única de los bienes de inversión
/// entregados en él (art. 110 LIVA). TODO(VERIFY): el art. 110 no dice en qué declaración; las
/// instrucciones del 303 la llevan a la del periodo de la entrega (no cotejado) y el art.
/// 107.Siete deja las anuales en la del último periodo.
pub(crate) fn regularizacion_ventas(
    v: &Vault,
    s: &ModelSettings,
    start: Date,
    end: Date,
) -> Result<(Money, Vec<String>)> {
    let year = start.year();
    // Una sola consulta rápida antes de calcular nada.
    let hay: bool = v.connection().query_row(
        "SELECT EXISTS(SELECT 1 FROM fixed_asset WHERE disposed_on BETWEEN ?1 AND ?2)",
        [start.to_string(), end.to_string()],
        |r| r.get(0),
    )?;
    if !hay {
        return Ok((Money::ZERO, vec![]));
    }
    let mut total = Money::ZERO;
    let mut avisos = Vec::new();
    let mut p = Prorratas::new(v, s);
    for b in bienes(v, year - 9, end.year())? {
        let Some((fecha, tipo, _)) = b.venta else {
            continue;
        };
        if fecha < start || fecha > end || !b.es_bien_de_inversion() {
            continue;
        }
        let calc = b.calculo(p.referencia(&b)?);
        if anios_restantes(&calc, fecha.year()) <= 0 {
            continue;
        }
        if tipo.is_none() {
            avisos.push(format!(
                "{}: se dio de baja dentro de su periodo de regularización y no sé cómo tributó la \
                 entrega: anótalo en IVA y libros para regularizar [43] (art. 110 LIVA).",
                b.nombre
            ));
            continue;
        }
        if calc.referencia.is_none() {
            avisos.push(format!(
                "{}: no sé la prorrata definitiva de {} para regularizar su venta (art. 110 LIVA).",
                b.nombre,
                b.registro.year()
            ));
            continue;
        }
        if let Some(m) = regularizacion_venta_de(&calc)
            && !m.is_zero()
        {
            total += m;
            avisos.push(format!(
                "{}: regularización por su entrega de {} por {} años (art. 110 LIVA)",
                b.nombre,
                m,
                anios_restantes(&calc, fecha.year())
            ));
        }
    }
    Ok((total, avisos))
}

/// Un año del periodo de regularización de un bien.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AnioBien {
    pub year: i32,
    pub prorrata: Option<u8>,
    pub diferencia: Option<i32>,
    pub importe: Money,
    pub nota: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VentaBien {
    pub fecha: Date,
    pub tipo: Option<VentaIva>,
    pub iva: Money,
    pub regularizacion: Option<Money>,
    pub anios_restantes: i32,
    pub origen: String,
}

/// Línea del libro registro de bienes de inversión (art. 65 RIVA).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FilaBien {
    pub asset_id: Option<i64>,
    pub received_id: Option<i64>,
    pub nombre: String,
    pub factura: String,
    pub nif: String,
    /// Fecha de registro de la factura (la de la deducción).
    pub fecha: Date,
    pub inmueble: bool,
    pub base: Money,
    pub cuota: Money,
    pub pct_factura: u8,
    /// Porcentaje con que se dedujo el año de compra.
    pub prorrata_compra: Option<u8>,
    pub deduccion_inicial: Option<Money>,
    pub fin_periodo: i32,
    pub anios: Vec<AnioBien>,
    pub venta: Option<VentaBien>,
    pub avisos: Vec<String>,
}

/// Elemento del inmovilizado sin datos de IVA (sin recibida ni cuota anotada).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ActivoSinIva {
    pub asset_id: i64,
    pub nombre: String,
    pub cuenta: String,
    pub adquirido: Date,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LibroBienes {
    pub year: i32,
    pub con_prorrata: bool,
    pub bienes: Vec<FilaBien>,
    /// Regularización anual de `year` ([43] del último periodo) y la de las entregas del año.
    pub total_anual: Money,
    pub total_ventas: Money,
    pub sin_datos: Vec<ActivoSinIva>,
    pub avisos: Vec<String>,
}

/// Libro registro de bienes de inversión del ejercicio `year` con su regularización.
pub fn libro(v: &Vault, year: i32, hoy: Date) -> Result<LibroBienes> {
    let s = models::settings(v)?;
    let mut p = Prorratas::new(v, &s);
    let mut filas = Vec::new();
    let (mut total_anual, mut total_ventas) = (Money::ZERO, Money::ZERO);
    let mut avisos = Vec::new();
    let mut con_bien = BTreeSet::new();
    for b in bienes(v, year - 9, year)? {
        if let Some(a) = b.asset_id {
            con_bien.insert(a);
        }
        let adq = b.registro.year();
        let fin = adq + anios_periodo(b.inmueble) - 1;
        if fin < year {
            continue;
        }
        let mut av = Vec::new();
        if !b.es_bien_de_inversion() {
            av.push(format!(
                "Su base ({}) no llega a 3.005,06 €: no es bien de inversión a efectos del IVA \
                 (art. 108.Dos.5.º LIVA) y no se regulariza.",
                b.base
            ));
        }
        let referencia = p.referencia(&b)?;
        if referencia.is_none() {
            av.push(format!(
                "No sé la prorrata definitiva de {adq}: anótala en Ajustes de modelos."
            ));
        }
        let calc = b.calculo(referencia);
        let regulariza_anual = s.prorata.is_some()
            && b.es_bien_de_inversion()
            && (!s.prorata_special || b.uso == ProrataUse::Comun);
        let mut anios = Vec::new();
        for y in (adq + 1)..=fin.min(year) {
            if b.venta.is_some_and(|(f, _, _)| f.year() <= y) {
                anios.push(AnioBien {
                    year: y,
                    prorrata: None,
                    diferencia: None,
                    importe: Money::ZERO,
                    nota: "Entregado: se regulariza de una vez por la entrega".into(),
                });
                break;
            }
            if !regulariza_anual {
                anios.push(AnioBien {
                    year: y,
                    prorrata: None,
                    diferencia: None,
                    importe: Money::ZERO,
                    nota: if s.prorata.is_none() {
                        "Sin prorrata: no hay regularización anual".into()
                    } else {
                        "Uso exclusivo con prorrata especial: no se regulariza".into()
                    },
                });
                continue;
            }
            let def = p.definitiva(y)?;
            let (diferencia, importe, nota) = match def
                .and_then(|d| regularizacion_anual_de(&calc, y, d).map(|(diff, m)| (d, diff, m)))
            {
                Some((_, diff, m)) => {
                    let nota = if diff.abs() <= 10 {
                        "Diferencia de 10 puntos o menos: no se regulariza".to_owned()
                    } else if y == year && hoy.year() <= year {
                        "En el 303 del último periodo (provisional hasta cerrar el año)".to_owned()
                    } else {
                        "En el 303 del último periodo [43]".to_owned()
                    };
                    (Some(diff), m, nota)
                }
                None => (
                    None,
                    Money::ZERO,
                    "Sin prorrata definitiva de ese año".into(),
                ),
            };
            if y == year {
                total_anual += importe;
            }
            anios.push(AnioBien {
                year: y,
                prorrata: def,
                diferencia,
                importe,
                nota,
            });
        }
        let venta = b.venta.map(|(fecha, tipo, iva)| {
            let reg = if b.es_bien_de_inversion() {
                regularizacion_venta_de(&calc)
            } else {
                None
            };
            if fecha.year() == year {
                total_ventas += reg.unwrap_or_default();
            }
            if tipo.is_none() {
                av.push(
                    "Dado de baja sin factura de venta: anota cómo tributó la entrega para \
                     regularizarla (art. 110 LIVA)."
                        .into(),
                );
            }
            VentaBien {
                fecha,
                tipo,
                iva,
                regularizacion: reg,
                anios_restantes: anios_restantes(&calc, fecha.year()),
                origen: b.origen_venta.clone(),
            }
        });
        filas.push(FilaBien {
            asset_id: b.asset_id,
            received_id: b.received_id,
            nombre: b.nombre.clone(),
            factura: b.factura.clone(),
            nif: b.nif.clone(),
            fecha: b.registro,
            inmueble: b.inmueble,
            base: b.base,
            cuota: b.cuota,
            pct_factura: b.pct,
            prorrata_compra: referencia,
            deduccion_inicial: referencia.map(|r| {
                calc.deduccion_base
                    .apply_rate(taxpain_core::money::Rate::percent(i32::from(r)))
            }),
            fin_periodo: fin,
            anios,
            venta,
            avisos: av,
        });
    }
    // Elementos materiales de los últimos diez años sin datos de IVA.
    let conn = v.connection();
    let manual = datos_manuales(conn)?;
    let sin_datos: Vec<ActivoSinIva> = activos(conn)?
        .into_iter()
        .filter(|a| {
            a.cuenta.starts_with("21")
                && a.adquirido.year() > year - 10
                && a.adquirido.year() <= year
                && !con_bien.contains(&a.id)
                && manual.get(&a.id).is_none_or(|m| m.cuota.is_none())
        })
        .map(|a| ActivoSinIva {
            asset_id: a.id,
            nombre: a.nombre,
            cuenta: a.cuenta,
            adquirido: a.adquirido,
        })
        .collect();
    if !sin_datos.is_empty() {
        avisos.push(format!(
            "{} elemento(s) del inmovilizado sin datos de IVA: si se dedujo su IVA, anota la cuota \
             para seguir su regularización.",
            sin_datos.len()
        ));
    }
    Ok(LibroBienes {
        year,
        con_prorrata: s.prorata.is_some(),
        bienes: filas,
        total_anual,
        total_ventas,
        sin_datos,
        avisos,
    })
}

/// Datos de IVA de un elemento que se anotan a mano.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatosBien {
    /// Cuota soportada (sólo si el elemento no viene de una recibida).
    #[serde(default)]
    pub cuota: Option<Money>,
    #[serde(default)]
    pub deducible: Option<u8>,
    #[serde(default)]
    pub inmueble: bool,
    /// Cómo tributó la entrega, si no hay factura de venta o para corregirla.
    #[serde(default)]
    pub venta: Option<VentaIva>,
    #[serde(default)]
    pub iva_venta: Option<Money>,
}

/// Enlaza el elemento creado al revisar una recibida de bien de inversión.
pub(crate) fn enlazar_bien(
    conn: &Connection,
    asset_id: i64,
    received_id: i64,
    inmueble: bool,
    now: OffsetDateTime,
) -> Result<()> {
    conn.execute(
        "INSERT INTO iva_bien_inversion(asset_id, received_id, inmueble, updated_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(asset_id) DO UPDATE SET received_id = excluded.received_id",
        params![asset_id, received_id, inmueble, now_str(now)],
    )?;
    Ok(())
}

/// Guarda los datos de IVA de un elemento del inmovilizado.
pub fn guardar_bien(v: &Vault, asset_id: i64, d: &DatosBien, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let existe: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM fixed_asset WHERE id = ?1)",
        [asset_id],
        |r| r.get(0),
    )?;
    if !existe {
        return Err(AppError::NotFound);
    }
    if d.cuota.is_some_and(|c| c.cents() < 0) || d.iva_venta.is_some_and(|c| c.cents() < 0) {
        return Err(other("las cuotas no pueden ser negativas"));
    }
    if d.deducible.is_some_and(|p| p > 100) {
        return Err(other("el porcentaje deducible va de 0 a 100"));
    }
    let tx = conn.unchecked_transaction()?;
    let at = now_str(now);
    tx.execute(
        "INSERT INTO iva_bien_inversion(asset_id, cuota, deducible, inmueble, venta, iva_venta,
                                        updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(asset_id) DO UPDATE SET cuota = excluded.cuota,
           deducible = excluded.deducible, inmueble = excluded.inmueble,
           venta = excluded.venta, iva_venta = excluded.iva_venta,
           updated_at = excluded.updated_at",
        params![
            asset_id,
            d.cuota.map(Money::cents),
            d.deducible,
            d.inmueble,
            d.venta.map(VentaIva::code),
            d.iva_venta.map(Money::cents),
            at
        ],
    )?;
    audit::append(
        &tx,
        &at,
        Actor::User,
        "iva.bien_inversion",
        &asset_id.to_string(),
        &serde_json::json!({
            "cuota": d.cuota.map(Money::cents), "deducible": d.deducible,
            "inmueble": d.inmueble, "venta": d.venta.map(VentaIva::code),
            "iva_venta": d.iva_venta.map(Money::cents)
        }),
    )?;
    tx.commit()?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Diagnóstico: ¿sociedad holding o con cartera?
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Perfil {
    /// Sin participadas, dividendos ni préstamos a participadas.
    SinCartera,
    /// Actividad propia y, además, participaciones (sin servicios a las participadas).
    ActividadConCartera,
    /// Participaciones y servicios facturados a las participadas (intervención en su gestión).
    HoldingMixta,
    /// Sólo participaciones (y, si acaso, préstamos): sin ingresos de una actividad sujeta.
    HoldingPura,
}

/// Lo que la contabilidad y las fichas dicen del año.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Datos {
    pub participadas: usize,
    pub coste_participadas: Money,
    /// 760: dividendos y otros ingresos de participaciones.
    pub dividendos: Money,
    /// 761, 762 y 769: intereses y otros ingresos financieros.
    pub financieros: Money,
    /// Préstamos concedidos a participadas o al grupo: saldo vivo al cierre e intereses.
    pub prestamos_participadas: Money,
    pub intereses_participadas: Money,
    /// 70x: ventas y prestaciones de servicios.
    pub ingresos_actividad: Money,
    /// Bases facturadas a las participadas o al grupo.
    pub servicios_participadas: Money,
}

/// Perfil a partir de los datos (no es una calificación: orienta la explicación).
pub fn perfil(d: &Datos) -> Perfil {
    let cartera = d.participadas > 0
        || d.coste_participadas.cents() > 0
        || d.dividendos.cents() > 0
        || d.prestamos_participadas.cents() > 0;
    if !cartera {
        Perfil::SinCartera
    } else if d.ingresos_actividad.cents() <= 0 {
        Perfil::HoldingPura
    } else if d.servicios_participadas.cents() > 0 {
        Perfil::HoldingMixta
    } else {
        Perfil::ActividadConCartera
    }
}

/// Porcentaje entero redondeado hacia abajo (`None` si el total es cero).
fn pct_abajo(parte: Money, total: Money) -> Option<u8> {
    if total.cents() <= 0 {
        return None;
    }
    let p = i128::from(parte.cents().clamp(0, total.cents())) * 100 / i128::from(total.cents());
    u8::try_from(p).ok()
}

/// Proporción del art. 93.Cinco: operaciones sujetas (con o sin derecho, más los intereses, que
/// son operaciones sujetas y exentas) sobre el total de ingresos (con los dividendos, que no
/// son contraprestación de ninguna operación). Hacia abajo: es un criterio, no la prorrata.
pub fn proporcion_93(operaciones: Money, financieros: Money, dividendos: Money) -> Option<u8> {
    let sujetas = operaciones + financieros;
    pct_abajo(sujetas, sujetas + dividendos.max(Money::ZERO))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Senal {
    pub clave: String,
    pub titulo: String,
    pub importe: Money,
    pub detalle: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Nota {
    pub titulo: String,
    pub texto: String,
    pub norma: String,
    /// La norma o su alcance no está cotejado: hay que comprobarlo.
    pub por_cotejar: bool,
}

fn nota(titulo: &str, texto: &str, norma: &str, por_cotejar: bool) -> Nota {
    Nota {
        titulo: titulo.into(),
        texto: texto.into(),
        norma: norma.into(),
        por_cotejar,
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Diagnostico {
    pub year: i32,
    pub perfil: Perfil,
    pub perfil_texto: String,
    pub datos: Datos,
    pub senales: Vec<Senal>,
    pub notas: Vec<Nota>,
    /// Prorrata del año con las facturas emitidas (art. 104.Dos), sin las operaciones
    /// financieras que no se facturan.
    pub prorrata_facturas: Option<u8>,
    /// La misma con los ingresos financieros en el denominador (si son habituales).
    pub prorrata_con_financieros: Option<u8>,
    /// Proporción del art. 93.Cinco para los gastos comunes a lo sujeto y lo no sujeto.
    pub proporcion_93: Option<u8>,
    pub prorrata_configurada: Option<u8>,
    pub prorrata_especial: bool,
    pub config: Config,
    pub reglas: Vec<Regla>,
    pub sectores: Vec<Sector>,
    pub comprobacion: ComprobacionSectores,
    pub aplicaciones: Vec<Aplicacion>,
    pub no_deducido: Money,
    pub avisos: Vec<String>,
}

fn datos_del_anio(v: &Vault, year: i32) -> Result<Datos> {
    use taxpain_core::accounting::prestamo::{OrigenContraparte, Sentido, Vinculo};
    let mut d = Datos::default();
    let participadas: Vec<_> = crate::vinculadas::participadas(v)?
        .into_iter()
        .filter(|p| p.active)
        .collect();
    d.participadas = participadas.len();
    d.coste_participadas = participadas.iter().map(|p| p.cost).sum();
    let mut nifs: BTreeSet<String> = participadas.iter().map(|p| clave_nif(&p.nif)).collect();
    for x in crate::vinculadas::vinculadas(v)? {
        use crate::vinculadas::Vinculo as V;
        if x.active
            && matches!(
                x.vinculo,
                V::Participada | V::ParticipadaIndirecta | V::Grupo
            )
        {
            nifs.insert(clave_nif(&x.nif));
        }
    }
    for e in crate::accounting::journal(v, year)? {
        if matches!(e.entry.origin, Origin::Regularizacion | Origin::Cierre) {
            continue;
        }
        for l in &e.entry.lines {
            let a = l.account.as_str();
            if a.starts_with("760") {
                d.dividendos -= l.net();
            } else if a.starts_with("761") || a.starts_with("762") || a.starts_with("769") {
                d.financieros -= l.net();
            } else if a.starts_with("70") {
                d.ingresos_actividad -= l.net();
            }
        }
    }
    let fy = crate::accounting::fiscal_year(v, year)?;
    for p in crate::prestamos::del_ejercicio(v.connection(), fy.start, fy.end)? {
        let c = &p.prestamo.contraparte;
        let a_participada = c.origen == OrigenContraparte::Participada
            || matches!(c.vinculo, Vinculo::Grupo | Vinculo::Asociada)
            || nifs.contains(&clave_nif(&p.nif));
        if p.prestamo.sentido == Sentido::Concedido && a_participada {
            d.prestamos_participadas += p.saldo_vivo;
            d.intereses_participadas += p.intereses;
        }
    }
    let por_cliente = models::issued_bases_by_client(v, year)?;
    d.servicios_participadas = nifs
        .iter()
        .filter_map(|n| por_cliente.get(n))
        .copied()
        .sum();
    Ok(d)
}

fn texto_perfil(p: Perfil) -> &'static str {
    match p {
        Perfil::SinCartera => {
            "No hay participadas, dividendos ni préstamos a participadas: lo de esta pantalla no \
             parece aplicarse, salvo que quieras repartir algún gasto común."
        }
        Perfil::ActividadConCartera => {
            "Hay actividad propia y, además, una cartera de participaciones. El IVA de lo que se \
             usa sólo para la cartera (gestión de la participación, asesoría de compra de \
             acciones, comisiones de valores) no se deduce; el de los gastos comunes, con un \
             criterio de reparto."
        }
        Perfil::HoldingMixta => {
            "Parece una holding mixta: tiene participaciones y factura servicios a sus \
             participadas. Los servicios dan derecho a deducir; la tenencia, no. Los gastos \
             comunes se reparten con un criterio razonable mantenido en el tiempo."
        }
        Perfil::HoldingPura => {
            "Parece una holding pura: no hay ingresos de una actividad sujeta al IVA. La mera \
             tenencia de participaciones no es actividad empresarial y su IVA soportado no se \
             deduce. Si presta servicios a las participadas o tiene otra actividad, factúralos o \
             regístralos para que se vean aquí."
        }
    }
}

fn notas_de(d: &Datos, diag_prorrata: Option<u8>) -> Vec<Nota> {
    let mut out = vec![nota(
        "Dividendos y tenencia de participaciones",
        "Cobrar dividendos o tener participaciones no es una entrega de bienes ni una prestación \
         de servicios: no lleva IVA, no entra en la prorrata y el IVA de los gastos destinados \
         sólo a la tenencia no se deduce.",
        "Arts. 4, 5, 94.Uno y 104.Tres.5.º LIVA",
        false,
    )];
    if d.servicios_participadas.cents() > 0 || d.ingresos_actividad.cents() > 0 {
        out.push(nota(
            "Gastos comunes a lo sujeto y lo no sujeto",
            "Los gastos que sirven a la vez a la actividad y a la tenencia (asesoría general, \
             oficina, programas) se deducen en la parte que corresponde a la actividad sujeta, \
             con un criterio razonable y homogéneo que se mantiene en el tiempo. Una opción es la \
             proporción de las operaciones sujetas sobre el total de ingresos del año (con los \
             dividendos). Anota el porcentaje en una regla por proveedor o por cuenta: se aplica \
             en el 303 y queda el criterio escrito.",
            "Art. 93.Cinco LIVA (por cotejar: se refiere a las operaciones del art. 7.8.º; la DGT \
             y el TJUE lo extienden a las holdings mixtas, STJUE Larentia + Minerva C-108/14)",
            true,
        ));
    }
    if d.financieros.cents() > 0 || d.prestamos_participadas.cents() > 0 {
        out.push(nota(
            "Préstamos e intereses de las participadas",
            "Prestar dinero es una operación financiera exenta. Si no es tu actividad habitual, \
             sus intereses no entran en la prorrata (ni arriba ni abajo); si lo es, van al \
             denominador y bajan el porcentaje de deducción. Arriba tienes las dos prorratas para \
             compararlas.",
            "Arts. 20.Uno.18.º y 104.Tres.4.º LIVA",
            false,
        ));
    }
    if diag_prorrata.is_some_and(|p| p < 100) {
        out.push(nota(
            "Prorrata",
            "Si hay operaciones sujetas sin derecho a deducir (exentas), se aplica la prorrata: \
             durante el año el porcentaje provisional (el definitivo del año anterior) y en el \
             último 303 la regularización con el definitivo [44] y, para los bienes de inversión, \
             la de los años siguientes [43]. Se configura en Ajustes de modelos.",
            "Arts. 102 a 106 y 107 a 110 LIVA",
            false,
        ));
    }
    out.push(nota(
        "Sectores diferenciados",
        "Si dos actividades están en grupos distintos de la CNAE y sus porcentajes de deducción \
         difieren en más de 50 puntos del de la principal (la de más volumen del año anterior), \
         forman sectores distintos y cada uno deduce por separado. La accesoria (no más del 15 % \
         del volumen de la principal y que contribuye a ella) sigue a la principal.",
        "Arts. 9.1.º.c.a' y 101 LIVA",
        false,
    ));
    out
}

/// Diagnóstico del año con las reglas, los sectores y lo que se ha aplicado.
pub fn diagnostico(v: &Vault, year: i32) -> Result<Diagnostico> {
    let s = models::settings(v)?;
    let datos = datos_del_anio(v, year)?;
    let perfil = perfil(&datos);
    let (ops, con_derecho, _) = models::prorata_year(v, year, s.prorata_special)?;
    let prorrata_facturas = (ops.cents() > 0).then(|| m303::prorata_percent(con_derecho, ops));
    let con_fin = ops + datos.financieros.max(Money::ZERO);
    let prorrata_con_financieros =
        (con_fin.cents() > 0).then(|| m303::prorata_percent(con_derecho, con_fin));
    let proporcion = proporcion_93(ops, datos.financieros.max(Money::ZERO), datos.dividendos);
    let sectores = sectores(v)?;
    let comprobacion = comprobar_sectores(&sectores);
    let config = config(v)?;
    let aplicaciones = aplicaciones(v, year)?;
    let no_deducido = aplicaciones.iter().map(|a| a.no_deducido).sum();
    let mut avisos = Vec::new();
    if perfil == Perfil::HoldingPura {
        let soportado: Money = recibidas(v, year, year, false)?
            .iter()
            .map(|r| r.cuota)
            .sum();
        if soportado.cents() > 0 && reglas(v)?.is_empty() {
            avisos.push(format!(
                "Hay {soportado} de IVA soportado en {year} y no hay ingresos de una actividad \
                 sujeta: si todo es de la tenencia, no se deduce. Revisa las recibidas o anota \
                 una regla."
            ));
        }
    }
    if comprobacion.hay_diferenciados && !config.sectores_diferenciados {
        avisos.push(
            "Con las actividades anotadas hay sectores diferenciados y la deducción no se separa: \
             márcalo si lo aplicas y lleva las compras de cada sector con una regla."
                .into(),
        );
    }
    if config.sectores_diferenciados
        && s.prorata.is_none()
        && prorrata_facturas.is_some_and(|p| p < 100)
    {
        avisos.push(
            "Separas sectores, pero no hay prorrata configurada para los gastos comunes a varios \
             sectores (Ajustes de modelos)."
                .into(),
        );
    }
    let mut senales = Vec::new();
    let mut senal = |clave: &str, titulo: &str, importe: Money, detalle: String| {
        senales.push(Senal {
            clave: clave.into(),
            titulo: titulo.into(),
            importe,
            detalle,
        });
    };
    if datos.participadas > 0 {
        senal(
            "participadas",
            "Participadas",
            datos.coste_participadas,
            format!("{} en la ficha de Vinculadas (coste)", datos.participadas),
        );
    }
    if datos.dividendos.cents() != 0 {
        senal(
            "dividendos",
            "Dividendos (760)",
            datos.dividendos,
            "No sujetos: fuera de la prorrata".into(),
        );
    }
    if datos.prestamos_participadas.cents() > 0 {
        senal(
            "prestamos",
            "Préstamos a participadas",
            datos.prestamos_participadas,
            format!(
                "Saldo vivo al cierre; intereses del año {}",
                datos.intereses_participadas
            ),
        );
    }
    if datos.financieros.cents() != 0 {
        senal(
            "financieros",
            "Ingresos financieros (761, 762, 769)",
            datos.financieros,
            "Operaciones financieras exentas".into(),
        );
    }
    if datos.servicios_participadas.cents() != 0 {
        senal(
            "servicios",
            "Servicios facturados a participadas",
            datos.servicios_participadas,
            "Facturas emitidas a participadas y al grupo".into(),
        );
    }
    senal(
        "actividad",
        "Ventas y servicios (70)",
        datos.ingresos_actividad,
        "Ingresos de la actividad".into(),
    );
    Ok(Diagnostico {
        year,
        perfil,
        perfil_texto: texto_perfil(perfil).into(),
        notas: notas_de(&datos, prorrata_con_financieros),
        datos,
        senales,
        prorrata_facturas,
        prorrata_con_financieros,
        proporcion_93: proporcion,
        prorrata_configurada: s.prorata,
        prorrata_especial: s.prorata_special,
        config,
        reglas: reglas(v)?,
        sectores,
        comprobacion,
        aplicaciones,
        no_deducido,
        avisos,
    })
}

/// Primer día del trimestre de `d` (fecha por defecto de una regla nueva).
pub fn inicio_trimestre(d: Date) -> Date {
    let m = (u8::from(d.month()) - 1) / 3 * 3 + 1;
    Month::try_from(m)
        .ok()
        .and_then(|m| Date::from_calendar_date(d.year(), m, 1).ok())
        .unwrap_or(d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn eur(e: i64) -> Money {
        Money::from_euros(e, 0)
    }

    fn sector(id: i64, nombre: &str, cnae: &str, pct: u8, vol: i64) -> Sector {
        Sector {
            id,
            nombre: nombre.into(),
            cnae: cnae.into(),
            pct,
            volumen: eur(vol),
            notas: String::new(),
        }
    }

    #[test]
    fn sectores_diferenciados_con_grupo_distinto_y_mas_de_50_puntos() {
        // Asesoría a filiales (70.2, 100 %) y financiación del grupo (64.9, 0 %).
        let s = [
            sector(1, "Servicios a filiales", "7022", 100, 200_000),
            sector(2, "Financiación al grupo", "6499", 0, 80_000),
        ];
        let c = comprobar_sectores(&s);
        assert!(c.hay_diferenciados);
        assert_eq!(c.filas[0].papel, Papel::Principal);
        assert_eq!(c.filas[1].papel, Papel::Diferenciado);
        assert_eq!(c.filas[1].diferencia, 100);
    }

    #[test]
    fn mismo_grupo_poca_diferencia_o_accesoria_no_son_sector_aparte() {
        let s = [
            sector(1, "Principal", "7022", 100, 100_000),
            sector(2, "Mismo grupo", "7021", 0, 50_000),
            sector(3, "Cerca", "6820", 60, 50_000),
            sector(4, "Pequeña", "6499", 0, 15_000),
            sector(5, "Sin código", "", 0, 50_000),
        ];
        let c = comprobar_sectores(&s);
        let papel: Vec<Papel> = c.filas.iter().map(|f| f.papel).collect();
        assert_eq!(
            papel,
            [
                Papel::Principal,
                Papel::MismoSector,
                Papel::MismoSector,
                Papel::Accesoria,
                Papel::SinCnae
            ]
        );
        assert!(!c.hay_diferenciados);
    }

    fn regla(id: i64, ambito: Ambito, clave: &str, pct: u8, desde: Date) -> Regla {
        Regla {
            id,
            ambito,
            clave: clave.into(),
            pct,
            sector_id: None,
            criterio: "Proporción de ingresos sujetos".into(),
            desde,
        }
    }

    #[test]
    fn la_regla_del_proveedor_gana_a_la_de_la_cuenta_y_cuenta_la_fecha() {
        let reglas = [
            regla(1, Ambito::Cuenta, "62", 80, date!(2026 - 01 - 01)),
            regla(2, Ambito::Cuenta, "6230", 60, date!(2026 - 01 - 01)),
            regla(3, Ambito::Proveedor, "B12345674", 40, date!(2026 - 07 - 01)),
        ];
        let d = date!(2026 - 08 - 01);
        assert_eq!(
            regla_para(&reglas, "b-12345674", Some("6230001"), d).map(|r| r.id),
            Some(3)
        );
        // Antes del día de la regla del proveedor: la cuenta más larga.
        assert_eq!(
            regla_para(&reglas, "B12345674", Some("6230001"), date!(2026 - 03 - 01)).map(|r| r.id),
            Some(2)
        );
        assert_eq!(
            regla_para(&reglas, "A22222228", Some("6290"), d).map(|r| r.id),
            Some(1)
        );
        assert!(regla_para(&reglas, "A22222228", Some("6400"), d).is_none());
    }

    #[test]
    fn porcentaje_menor_y_sector_que_se_separa() {
        let mut r = regla(1, Ambito::Cuenta, "62", 60, date!(2026 - 01 - 01));
        let sec = [(7, 50u8)].into_iter().collect();
        assert_eq!(aplicar_regla(Some(&r), &sec, false, 100).pct, 60);
        // La factura ya estaba al 50 %: se queda el 50.
        assert_eq!(aplicar_regla(Some(&r), &sec, false, 50).pct, 50);
        r.sector_id = Some(7);
        let a = aplicar_regla(Some(&r), &sec, true, 100);
        assert_eq!((a.pct, a.fuera_prorrata), (30, true));
        // Sin separar sectores el sector no cuenta.
        assert!(!aplicar_regla(Some(&r), &sec, false, 100).fuera_prorrata);
        assert_eq!(aplicar_regla(None, &sec, true, 90).pct, 90);
    }

    fn bien(inmueble: bool, referencia: u8) -> BienCalculo {
        BienCalculo {
            adquirido: 2024,
            inmueble,
            deduccion_base: eur(10_000),
            referencia: Some(referencia),
            venta: None,
        }
    }

    #[test]
    fn regularizacion_anual_quinta_parte_por_encima_de_10_puntos() {
        let b = bien(false, 80);
        // 2025 al 60 %: −20 puntos → 10.000 × −20 % / 5 = −400.
        assert_eq!(
            regularizacion_anual_de(&b, 2025, 60),
            Some((-20, -eur(400)))
        );
        // 10 puntos justos: no se regulariza.
        assert_eq!(
            regularizacion_anual_de(&b, 2026, 90),
            Some((10, Money::ZERO))
        );
        // Fuera del periodo (2024 + 4 = 2028).
        assert_eq!(regularizacion_anual_de(&b, 2029, 20), None);
        assert_eq!(regularizacion_anual_de(&b, 2024, 20), None);
        // Inmueble: décima parte durante nueve años.
        let i = bien(true, 80);
        assert_eq!(
            regularizacion_anual_de(&i, 2033, 50),
            Some((-30, -eur(300)))
        );
    }

    #[test]
    fn regularizacion_por_venta_exenta_y_sujeta() {
        // Inmueble de 2024 deducido al 100 %, vendido exento en 2026: quedan 2026–2033 (8 años)
        // → 10.000 × (0 − 100) % × 8 / 10 = −8.000.
        let mut b = bien(true, 100);
        b.venta = Some((date!(2026 - 05 - 10), Some(VentaIva::Exenta), Money::ZERO));
        assert_eq!(anios_restantes(&b, 2026), 8);
        assert_eq!(regularizacion_venta_de(&b), Some(-eur(8_000)));
        // Sin regularización anual el año de la venta ni después.
        assert_eq!(regularizacion_anual_de(&b, 2026, 50), None);
        assert!(regularizacion_anual_de(&b, 2025, 50).is_some());
        // Mueble de 2024 deducido al 40 %, vendido con IVA en 2026: 3 años (2026–2028) →
        // 10.000 × 60 % × 3 / 5 = 3.600, sin superar el IVA repercutido (2.100).
        let mut m = bien(false, 40);
        m.venta = Some((date!(2026 - 05 - 10), Some(VentaIva::Sujeta), eur(2_100)));
        assert_eq!(regularizacion_venta_de(&m), Some(eur(2_100)));
        m.venta = Some((date!(2026 - 05 - 10), Some(VentaIva::Sujeta), eur(5_000)));
        assert_eq!(regularizacion_venta_de(&m), Some(eur(3_600)));
        // Transmisión de empresa, pérdida o fuera del periodo: nada.
        m.venta = Some((
            date!(2026 - 05 - 10),
            Some(VentaIva::TransmisionEmpresa),
            eur(0),
        ));
        assert_eq!(regularizacion_venta_de(&m), None);
        m.venta = Some((date!(2029 - 05 - 10), Some(VentaIva::Exenta), eur(0)));
        assert_eq!(regularizacion_venta_de(&m), None);
        // Sin saber cómo tributó: nada (se avisa).
        m.venta = Some((date!(2026 - 05 - 10), None, eur(0)));
        assert_eq!(regularizacion_venta_de(&m), None);
    }

    #[test]
    fn perfil_y_proporcion_del_93_cinco() {
        let mut d = Datos::default();
        assert_eq!(perfil(&d), Perfil::SinCartera);
        d.participadas = 1;
        d.dividendos = eur(50_000);
        assert_eq!(perfil(&d), Perfil::HoldingPura);
        d.ingresos_actividad = eur(100_000);
        assert_eq!(perfil(&d), Perfil::ActividadConCartera);
        d.servicios_participadas = eur(60_000);
        assert_eq!(perfil(&d), Perfil::HoldingMixta);
        // 100.000 sujetas + 10.000 de intereses sobre 160.000 con los dividendos: 68,75 → 68.
        assert_eq!(
            proporcion_93(eur(100_000), eur(10_000), eur(50_000)),
            Some(68)
        );
        assert_eq!(proporcion_93(Money::ZERO, Money::ZERO, Money::ZERO), None);
    }

    #[test]
    fn inicio_del_trimestre() {
        assert_eq!(
            inicio_trimestre(date!(2026 - 11 - 17)),
            date!(2026 - 10 - 01)
        );
        assert_eq!(
            inicio_trimestre(date!(2026 - 03 - 31)),
            date!(2026 - 01 - 01)
        );
    }
}
