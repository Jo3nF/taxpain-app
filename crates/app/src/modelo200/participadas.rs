//! Ficha fiscal de las participadas (G15): valor nominal y fiscal, fondos propios por ejercicio,
//! dividendos cobrados y comprobación de la exención del art. 21 LIS en cada uno.
//!
//! La ficha mínima (NIF, porcentaje, fecha de adquisición, coste, subcuenta) es la de
//! `vinculadas::Participada`; aquí van los datos que la completan (`participada_fiscal`,
//! `participada_ejercicio`, `participada_dividendo`, migración 0077).
//!
//! **Exención de dividendos (art. 21.1 LIS)**: a) participación directa o indirecta de al menos
//! el 5 % «poseída de manera ininterrumpida durante el año anterior al día en que sea exigible el
//! beneficio que se distribuya o, en su defecto, que se mantenga posteriormente durante el
//! tiempo necesario para completar dicho plazo»; b) si la participada no es residente, que haya
//! tributado por un impuesto análogo a un tipo nominal de al menos el 10 % (y no en un paraíso
//! fiscal). Desde 2021 la exención es del 95 %: el 5 % del dividendo se considera gastos de
//! gestión (art. 21.10, Ley 11/2020). La alternativa del valor de adquisición de más de 20 M€
//! sólo vale para participaciones adquiridas antes de 2021 hasta 2025 (DT 40.ª LIS).
//! `TODO(VERIFY)`: la exención plena del art. 21.11 (filiales nuevas de empresas de menos de
//! 40 M€) y los requisitos de las participadas indirectas y holding (art. 21.1, segundo párrafo)
//! no se comprueban: se teclean en las casillas [01764]/[01765].

use serde::{Deserialize, Serialize};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;
use crate::vinculadas::{self, Participada};

/// 5 % en millonésimas (art. 21.1.a LIS).
pub const PCT_ART21: u32 = 50_000;

fn other(e: impl ToString) -> AppError {
    AppError::Other(e.to_string())
}

fn parse_date(s: &str) -> Option<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]")).ok()
}

/// Cómo se adquirió la participación.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrigenParticipacion {
    #[default]
    Compra,
    Constitucion,
    /// Aportación no dineraria (art. 87 LIS): el valor fiscal es el que tenían los bienes.
    Aportacion,
    /// Canje de valores (art. 80 LIS): el valor fiscal es el de las entregadas.
    Canje,
}

impl OrigenParticipacion {
    fn as_str(self) -> &'static str {
        match self {
            Self::Compra => "compra",
            Self::Constitucion => "constitucion",
            Self::Aportacion => "aportacion",
            Self::Canje => "canje",
        }
    }
    fn parse(s: &str) -> Self {
        match s {
            "constitucion" => Self::Constitucion,
            "aportacion" => Self::Aportacion,
            "canje" => Self::Canje,
            _ => Self::Compra,
        }
    }
}

/// Datos fiscales de una participada.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatosFiscales {
    pub participada_id: i64,
    #[serde(default)]
    pub nominal: Money,
    /// `None`: igual al coste contable.
    #[serde(default)]
    pub valor_fiscal: Option<Money>,
    #[serde(default)]
    pub origen: OrigenParticipacion,
    /// Una no residente tributa al 10 % nominal o más por un impuesto análogo (art. 21.1.b).
    #[serde(default = "yes")]
    pub tributacion_minima: bool,
}

fn yes() -> bool {
    true
}

/// Fondos propios y resultado de la participada en un ejercicio.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FondosParticipada {
    pub participada_id: i64,
    pub year: i32,
    pub capital: Money,
    pub reservas: Money,
    pub otras_pn: Money,
    pub resultado: Money,
}

/// Dividendo cobrado de una participada.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dividendo {
    #[serde(default)]
    pub id: i64,
    pub participada_id: i64,
    /// Fecha del acuerdo de reparto (cuando el dividendo es exigible).
    pub fecha_acuerdo: Date,
    #[serde(default)]
    pub fecha_cobro: Option<Date>,
    pub bruto: Money,
    #[serde(default)]
    pub retencion: Money,
    /// Certificado o acta en Documentos (nombre o referencia).
    #[serde(default)]
    pub documento: String,
    #[serde(default)]
    pub notas: String,
}

/// Situación del art. 21 en una fecha.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EstadoArt21 {
    Cumple,
    /// Aún no ha pasado un año desde la adquisición: la exención vale si se mantiene la
    /// participación hasta completarlo.
    Condicionada,
    NoCumple,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Art21 {
    pub estado: EstadoArt21,
    /// Fecha en que se cumple el año de posesión.
    pub ano_cumplido: Date,
    /// Porcentaje exento del dividendo (95 % desde 2021, art. 21.10 LIS).
    pub exento_pct: u8,
    pub motivos: Vec<String>,
}

/// Comprueba el art. 21 para un dividendo exigible el día `fecha`.
pub fn art21(p: &Participada, f: &DatosFiscales, fecha: Date) -> Art21 {
    let ano_cumplido = p
        .acquired
        .replace_year(p.acquired.year() + 1)
        .unwrap_or_else(|_| p.acquired + time::Duration::days(365));
    let mut motivos = Vec::new();
    let mut estado = EstadoArt21::Cumple;
    // Alternativa del valor de adquisición de más de 20 M€: sólo adquiridas antes de 2021, en
    // periodos iniciados hasta el 31/12/2025 (DT 40.ª LIS).
    let veinte_millones = p.cost >= Money::from_euros(20_000_000, 0)
        && p.acquired.year() < 2021
        && fecha.year() <= 2025;
    if p.pct < PCT_ART21 && !veinte_millones {
        estado = EstadoArt21::NoCumple;
        motivos.push(format!(
            "La participación es del {}: hace falta al menos el 5 % (art. 21.1.a LIS)",
            vinculadas::pct_texto(p.pct)
        ));
    } else if veinte_millones && p.pct < PCT_ART21 {
        motivos.push(
            "Menos del 5 %, pero con valor de adquisición de más de 20 M€ anterior a 2021 (DT \
             40.ª LIS)."
                .into(),
        );
    }
    if fecha < ano_cumplido && estado == EstadoArt21::Cumple {
        estado = EstadoArt21::Condicionada;
        motivos.push(format!(
            "Aún no hace un año que se tiene: la exención vale si se mantiene hasta el {} (art. \
             21.1.a LIS)",
            super::dmy(ano_cumplido)
        ));
    }
    if p.country != "ES" && !f.tributacion_minima {
        estado = EstadoArt21::NoCumple;
        motivos.push(
            "Participada no residente sin tributación de al menos el 10 % nominal por un \
             impuesto análogo (art. 21.1.b LIS)"
                .into(),
        );
    }
    Art21 {
        estado,
        ano_cumplido,
        exento_pct: if fecha.year() >= 2021 { 95 } else { 100 },
        motivos,
    }
}

/// Dividendo con su comprobación.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DividendoVista {
    pub dividendo: Dividendo,
    pub art21: Art21,
    /// Importe exento (bruto × porcentaje), si se cumple o está condicionada.
    pub exento: Money,
    /// Retención que procede: ninguna si se cumple el art. 21 (y el año de posesión), el 19 % si
    /// no (art. 128 LIS; art. 61 RIS). `TODO(VERIFY)`: la letra del art. 61 RIS.
    pub retencion_procede: Money,
}

/// Ficha completa de una participada para un ejercicio.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ParticipadaVista {
    pub participada: Participada,
    pub fiscal: DatosFiscales,
    /// Valor fiscal (el tecleado o el coste).
    pub valor_fiscal: Money,
    pub fondos: Option<FondosParticipada>,
    pub dividendos: Vec<DividendoVista>,
    pub bruto: Money,
    pub retenido: Money,
    pub exento: Money,
    /// Patrimonio neto de la participada × porcentaje (prueba de deterioro, PGC NRV 9.ª/8.ª
    /// Pymes), si hay fondos propios del ejercicio.
    pub pn_proporcional: Option<Money>,
    pub avisos: Vec<String>,
}

fn fiscal_de(v: &Vault, id: i64) -> Result<DatosFiscales> {
    let row = v
        .connection()
        .query_row(
            "SELECT nominal, valor_fiscal, origen, tributacion_minima FROM participada_fiscal
             WHERE participada_id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<i64>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, bool>(3)?,
                ))
            },
        )
        .optional()?;
    Ok(match row {
        Some((n, vf, o, t)) => DatosFiscales {
            participada_id: id,
            nominal: Money::from_cents(n),
            valor_fiscal: vf.map(Money::from_cents),
            origen: OrigenParticipacion::parse(&o),
            tributacion_minima: t,
        },
        None => DatosFiscales {
            participada_id: id,
            nominal: Money::ZERO,
            valor_fiscal: None,
            origen: OrigenParticipacion::Compra,
            tributacion_minima: true,
        },
    })
}

fn fondos_de(v: &Vault, id: i64, year: i32) -> Result<Option<FondosParticipada>> {
    Ok(v.connection()
        .query_row(
            "SELECT capital, reservas, otras_pn, resultado FROM participada_ejercicio
             WHERE participada_id = ?1 AND year = ?2",
            params![id, year],
            |r| {
                Ok(FondosParticipada {
                    participada_id: id,
                    year,
                    capital: Money::from_cents(r.get(0)?),
                    reservas: Money::from_cents(r.get(1)?),
                    otras_pn: Money::from_cents(r.get(2)?),
                    resultado: Money::from_cents(r.get(3)?),
                })
            },
        )
        .optional()?)
}

/// Dividendos de una participada con el acuerdo entre `desde` y `hasta`.
fn dividendos_de(v: &Vault, id: i64, desde: Date, hasta: Date) -> Result<Vec<Dividendo>> {
    let mut st = v.connection().prepare(
        "SELECT id, fecha_acuerdo, fecha_cobro, bruto, retencion, documento, notas
         FROM participada_dividendo
         WHERE participada_id = ?1 AND fecha_acuerdo >= ?2 AND fecha_acuerdo <= ?3
         ORDER BY fecha_acuerdo, id",
    )?;
    let rows = st.query_map(params![id, desde.to_string(), hasta.to_string()], |r| {
        let fa: String = r.get(1)?;
        let fc: Option<String> = r.get(2)?;
        Ok(Dividendo {
            id: r.get(0)?,
            participada_id: id,
            fecha_acuerdo: parse_date(&fa).unwrap_or(Date::MIN),
            fecha_cobro: fc.as_deref().and_then(parse_date),
            bruto: Money::from_cents(r.get(3)?),
            retencion: Money::from_cents(r.get(4)?),
            documento: r.get(5)?,
            notas: r.get(6)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Fichas de las participadas activas con los dividendos del ejercicio `year`.
pub fn fichas(v: &Vault, year: i32) -> Result<Vec<ParticipadaVista>> {
    let fy = crate::accounting::fiscal_year(v, year)?;
    let mut out = Vec::new();
    for p in vinculadas::participadas(v)?
        .into_iter()
        .filter(|p| p.active)
    {
        let fiscal = fiscal_de(v, p.id)?;
        let fondos = fondos_de(v, p.id, year)?;
        let mut avisos = Vec::new();
        let mut dividendos = Vec::new();
        for d in dividendos_de(v, p.id, fy.start, fy.end)? {
            let a = art21(&p, &fiscal, d.fecha_acuerdo);
            let exento = if a.estado == EstadoArt21::NoCumple {
                Money::ZERO
            } else {
                d.bruto.apply_rate(Rate::percent(i32::from(a.exento_pct)))
            };
            let retencion_procede = match (a.estado, p.country == "ES") {
                (EstadoArt21::Cumple, true) | (_, false) => Money::ZERO,
                _ => d.bruto.apply_rate(Rate::percent(19)),
            };
            if p.country == "ES" && d.retencion > retencion_procede {
                avisos.push(format!(
                    "Dividendo del {}: se retuvieron {} y no procedía retención (art. 21 LIS \
                     cumplido). Comunica la participación a la pagadora y pide que la regularice; \
                     mientras, la retención se resta en el 200 ([01785]).",
                    super::dmy(d.fecha_acuerdo),
                    d.retencion
                ));
            }
            if a.estado == EstadoArt21::Condicionada {
                avisos.push(format!(
                    "Dividendo del {}: exención condicionada a mantener la participación hasta el {}.",
                    super::dmy(d.fecha_acuerdo),
                    super::dmy(a.ano_cumplido)
                ));
            }
            dividendos.push(DividendoVista {
                dividendo: d,
                art21: a,
                exento,
                retencion_procede,
            });
        }
        let bruto: Money = dividendos.iter().map(|d| d.dividendo.bruto).sum();
        let retenido = dividendos.iter().map(|d| d.dividendo.retencion).sum();
        let exento = dividendos.iter().map(|d| d.exento).sum();
        let pn_proporcional = fondos.as_ref().map(|f| {
            let pn = f.capital + f.reservas + f.otras_pn + f.resultado;
            Money::from_cents(
                i64::try_from(i128::from(pn.cents()) * i128::from(p.pct) / 1_000_000).unwrap_or(0),
            )
        });
        if let Some(pnp) = pn_proporcional
            && p.cost > pnp
        {
            avisos.push(format!(
                "El coste ({}) supera la parte del patrimonio neto de la participada ({pnp}): \
                 revisa si hay deterioro (PGC, NRV 9.ª; Pymes, NRV 8.ª). Fiscalmente no es \
                 deducible (art. 13.2.b LIS).",
                p.cost
            ));
        }
        if let Some(f) = &fondos
            && bruto > (f.reservas + f.resultado).max(Money::ZERO)
            && bruto.cents() > 0
        {
            avisos.push(format!(
                "Los dividendos del año ({bruto}) superan las reservas y el resultado de la \
                 participada: comprueba que el reparto respeta el art. 273 LSC."
            ));
        }
        out.push(ParticipadaVista {
            valor_fiscal: fiscal.valor_fiscal.unwrap_or(p.cost),
            participada: p,
            fiscal,
            fondos,
            dividendos,
            bruto,
            retenido,
            exento,
            pn_proporcional,
            avisos,
        });
    }
    Ok(out)
}

fn existe(v: &Vault, id: i64) -> Result<()> {
    let n: i64 = v.connection().query_row(
        "SELECT COUNT(*) FROM participada WHERE id = ?1",
        [id],
        |r| r.get(0),
    )?;
    if n == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

fn auditar(
    tx: &taxpain_vault::rusqlite::Transaction<'_>,
    now: OffsetDateTime,
    accion: &str,
    objetivo: &str,
    detalle: serde_json::Value,
) -> Result<()> {
    audit::append(tx, &now_str(now), Actor::User, accion, objetivo, &detalle)?;
    Ok(())
}

pub fn guardar_fiscal(v: &Vault, f: &DatosFiscales, now: OffsetDateTime) -> Result<()> {
    existe(v, f.participada_id)?;
    if f.nominal.is_negative() || f.valor_fiscal.is_some_and(|x| x.is_negative()) {
        return Err(other("los importes no pueden ser negativos"));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO participada_fiscal(participada_id, nominal, valor_fiscal, origen,
           tributacion_minima, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(participada_id) DO UPDATE SET nominal = excluded.nominal,
           valor_fiscal = excluded.valor_fiscal, origen = excluded.origen,
           tributacion_minima = excluded.tributacion_minima, updated_at = excluded.updated_at",
        params![
            f.participada_id,
            f.nominal.cents(),
            f.valor_fiscal.map(Money::cents),
            f.origen.as_str(),
            f.tributacion_minima,
            now_str(now)
        ],
    )?;
    auditar(
        &tx,
        now,
        "participada.fiscal",
        &f.participada_id.to_string(),
        serde_json::json!({
            "nominal": f.nominal.cents(),
            "valor_fiscal": f.valor_fiscal.map(Money::cents),
            "origen": f.origen.as_str(),
        }),
    )?;
    tx.commit()?;
    Ok(())
}

pub fn guardar_fondos(v: &Vault, f: &FondosParticipada, now: OffsetDateTime) -> Result<()> {
    existe(v, f.participada_id)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO participada_ejercicio(participada_id, year, capital, reservas, otras_pn,
           resultado, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(participada_id, year) DO UPDATE SET capital = excluded.capital,
           reservas = excluded.reservas, otras_pn = excluded.otras_pn,
           resultado = excluded.resultado, updated_at = excluded.updated_at",
        params![
            f.participada_id,
            f.year,
            f.capital.cents(),
            f.reservas.cents(),
            f.otras_pn.cents(),
            f.resultado.cents(),
            now_str(now)
        ],
    )?;
    auditar(
        &tx,
        now,
        "participada.fondos",
        &f.participada_id.to_string(),
        serde_json::json!({ "year": f.year }),
    )?;
    tx.commit()?;
    Ok(())
}

pub fn guardar_dividendo(v: &Vault, d: &Dividendo, now: OffsetDateTime) -> Result<i64> {
    existe(v, d.participada_id)?;
    if d.bruto.cents() <= 0 {
        return Err(other("el dividendo bruto tiene que ser mayor que cero"));
    }
    if d.retencion.is_negative() || d.retencion > d.bruto {
        return Err(other("la retención va de cero al bruto"));
    }
    if d.fecha_cobro.is_some_and(|c| c < d.fecha_acuerdo) {
        return Err(other("el cobro no puede ser anterior al acuerdo"));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let cobro = d.fecha_cobro.map(|x| x.to_string());
    let id = if d.id == 0 {
        tx.execute(
            "INSERT INTO participada_dividendo(participada_id, fecha_acuerdo, fecha_cobro, bruto,
               retencion, documento, notas, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                d.participada_id,
                d.fecha_acuerdo.to_string(),
                cobro,
                d.bruto.cents(),
                d.retencion.cents(),
                d.documento.trim(),
                d.notas.trim(),
                now_str(now)
            ],
        )?;
        tx.last_insert_rowid()
    } else {
        let n = tx.execute(
            "UPDATE participada_dividendo SET participada_id = ?1, fecha_acuerdo = ?2,
               fecha_cobro = ?3, bruto = ?4, retencion = ?5, documento = ?6, notas = ?7
             WHERE id = ?8",
            params![
                d.participada_id,
                d.fecha_acuerdo.to_string(),
                cobro,
                d.bruto.cents(),
                d.retencion.cents(),
                d.documento.trim(),
                d.notas.trim(),
                d.id
            ],
        )?;
        if n == 0 {
            return Err(AppError::NotFound);
        }
        d.id
    };
    auditar(
        &tx,
        now,
        "participada.dividendo",
        &id.to_string(),
        serde_json::json!({ "bruto": d.bruto.cents(), "retencion": d.retencion.cents() }),
    )?;
    tx.commit()?;
    Ok(id)
}

/// Borra un dividendo (pide confirmación en la interfaz).
pub fn borrar_dividendo(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    if tx.execute("DELETE FROM participada_dividendo WHERE id = ?1", [id])? == 0 {
        return Err(AppError::NotFound);
    }
    auditar(
        &tx,
        now,
        "participada.dividendo.delete",
        &id.to_string(),
        serde_json::json!({}),
    )?;
    tx.commit()?;
    Ok(())
}
