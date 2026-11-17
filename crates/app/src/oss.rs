//! Ventanilla única (OSS, régimen de la Unión): ventas a consumidores finales de otros Estados
//! miembros de servicios de telecomunicaciones, radiodifusión y televisión o electrónicos (TBE)
//! y ventas a distancia de bienes.
//!
//! - **Umbral** (art. 73 LIVA): 10.000 € al año, sin IVA, sumando esas operaciones a todos los
//!   Estados miembros. Se mira el año anterior y el año en curso; superado en el año en curso,
//!   tributa en destino la operación con la que se supera y las siguientes. Sólo cuentan las
//!   facturas emitidas a un particular de otro Estado miembro (sin NIF-IVA) que el usuario (o la
//!   propia factura OSS) clasifica como servicio TBE o venta a distancia de bienes, más lo que se
//!   haya vendido fuera de TaxPain (ajustes). Los demás servicios a particulares de la UE llevan
//!   IVA español (art. 69.Uno.2.º LIVA) y no cuentan.
//! - **Factura**: con el borrador `oss` ([`OssDraft`]) las líneas van no sujetas (N2) y se suma
//!   el IVA del país del cliente ([`taxpain_core::invoice::oss`]); en el 303 su base va a la
//!   casilla informativa [123] y en el 390 a la [126]; el IVA de destino va a la 4779 del país.
//! - **Libro y 369**: las facturas OSS del trimestre por Estado miembro, clase y tipo; las
//!   rectificativas de facturas de trimestres anteriores, como correcciones de aquel periodo.
//!   No hay diseño de registro del 369 en el proyecto: el borrador es una hoja (XLSX) para
//!   pasarla al formulario de la Sede. Nada se presenta.
//! - **Alta (035)**: si hay que tributar en destino (o se opta) y no consta la fecha de alta en
//!   los ajustes, se avisa.
//!
//! La tabla de tipos generales es un dato con fecha y fuente: antes de usar el tipo de un país,
//! el usuario lo confirma ([`confirmar_tipo`]) y queda en la auditoría.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::eu::is_other_member_state;
use taxpain_core::invoice::oss::{self as core_oss, Umbral};
use taxpain_core::invoice::{
    IdType, Invoice, InvoiceKind, NotSubject, OssSupply, OssVat, PartyId, Rectification,
    RectificationMethod, Totals, VatTreatment,
};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::{InvoiceDraft, now_str};

/// Clave de los ajustes.
const AJUSTES: &str = "oss_ajustes";

/// Lo que el usuario elige en la factura a un particular de otro Estado miembro.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OssDraft {
    /// Tipo de IVA del Estado miembro del cliente.
    pub rate: Rate,
    pub supply: OssSupply,
}

/// Clase de una venta a un particular de la UE para el umbral.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Clase {
    /// Servicio TBE (art. 70.Uno.4.º y 8.º LIVA): cuenta.
    Servicios,
    /// Venta a distancia de bienes (art. 68.Tres.a LIVA): cuenta.
    Bienes,
    /// Otra operación (p. ej. un servicio de consultoría): IVA español, no cuenta.
    Otro,
}

impl Clase {
    fn code(self) -> &'static str {
        match self {
            Clase::Servicios => "servicios",
            Clase::Bienes => "bienes",
            Clase::Otro => "otro",
        }
    }

    fn parse(s: &str) -> Option<Clase> {
        match s {
            "servicios" => Some(Clase::Servicios),
            "bienes" => Some(Clase::Bienes),
            "otro" => Some(Clase::Otro),
            _ => None,
        }
    }

    fn of(s: OssSupply) -> Clase {
        match s {
            OssSupply::Servicios => Clase::Servicios,
            OssSupply::Bienes => Clase::Bienes,
        }
    }
}

/// Ajustes de la ventanilla única.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ajustes {
    /// Fecha de efectos del alta en el régimen de la Unión (modelo 035), si ya se presentó.
    #[serde(default)]
    pub alta_035: Option<Date>,
    /// Opción por tributar en destino aunque no se supere el umbral (art. 73 LIVA, último
    /// párrafo; vincula al menos dos años naturales).
    #[serde(default)]
    pub opcion_destino: bool,
    /// Ventas a distancia y servicios TBE a particulares de la UE que no están facturados en
    /// TaxPain (p. ej. de una tienda en línea), por año, sin IVA.
    #[serde(default)]
    pub otras_ventas: BTreeMap<i32, Money>,
}

pub fn ajustes(v: &Vault) -> Result<Ajustes> {
    Ok(v.setting::<Ajustes>(AJUSTES)?.unwrap_or_default())
}

pub fn guardar_ajustes(v: &Vault, a: &Ajustes, now: OffsetDateTime) -> Result<()> {
    if a.otras_ventas.values().any(|m| m.is_negative()) {
        return Err(AppError::Other(
            "las ventas de fuera de TaxPain no pueden ser negativas".into(),
        ));
    }
    v.set_setting(AJUSTES, a)?;
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        "oss.ajustes",
        "oss",
        &serde_json::json!({
            "alta_035": a.alta_035.map(|d| d.to_string()),
            "opcion_destino": a.opcion_destino,
        }),
    )?;
    Ok(())
}

/// País del destinatario si es un particular (sin NIF-IVA) de otro Estado miembro.
fn pais_particular_ue(inv: &Invoice) -> Option<String> {
    match inv.recipient.as_ref().map(|r| &r.id) {
        Some(PartyId::Foreign {
            country, id_type, ..
        }) if *id_type != IdType::NifIva && is_other_member_state(country, true) => {
            Some(core_oss::codigo(country))
        }
        _ => None,
    }
}

/// Comprueba el borrador de una factura con IVA de destino: un particular de otro Estado
/// miembro, en euros, con todas las líneas no sujetas (N2), sin retención ni recargo, y un
/// tipo razonable. Las rectificativas, por diferencias y por las causas 1 o 4.
pub(crate) fn comprobar_borrador(d: &InvoiceDraft, o: &OssDraft) -> Result<()> {
    let err = |m: &str| Err(AppError::Other(m.into()));
    let country = match d.recipient.as_ref().map(|r| &r.id) {
        Some(PartyId::Foreign {
            country, id_type, ..
        }) if *id_type != IdType::NifIva => country.clone(),
        _ => {
            return err(
                "el IVA del país del cliente (ventanilla única) sólo se aplica a un particular de \
                 otro país de la UE identificado sin NIF-IVA",
            );
        }
    };
    if !is_other_member_state(&country, o.supply == OssSupply::Bienes) {
        return Err(AppError::Other(format!(
            "«{country}» no es otro Estado miembro de la UE para {} (Irlanda del Norte sólo \
             cuenta para los bienes)",
            o.supply.label()
        )));
    }
    match d.kind {
        InvoiceKind::Completa => {}
        InvoiceKind::Rectificativa(Rectification {
            code: 1 | 4,
            method: RectificationMethod::Diferencias,
        }) => {}
        _ => {
            return err(
                "con IVA del país del cliente, TaxPain sólo emite facturas completas y sus \
                 rectificativas por diferencias (causas 1 o 4)",
            );
        }
    }
    if d.currency.is_some() {
        return err(
            "la factura con IVA del país del cliente (ventanilla única) tiene que ser en euros",
        );
    }
    if !(1..=3500).contains(&o.rate.bp()) {
        return err(
            "el tipo de IVA del país del cliente tiene que estar entre el 0,01 % y el 35 %",
        );
    }
    for (i, l) in d.lines.iter().enumerate() {
        if l.vat != VatTreatment::NotSubject(NotSubject::N2) {
            return Err(AppError::Other(format!(
                "línea {}: con IVA del país del cliente las líneas no llevan IVA español (no \
                 sujetas por reglas de localización)",
                i + 1
            )));
        }
        if l.withholding.is_some() || l.surcharge.is_some() {
            return Err(AppError::Other(format!(
                "línea {}: sin retención ni recargo de equivalencia con un cliente de fuera",
                i + 1
            )));
        }
    }
    Ok(())
}

/// Una factura emitida a un particular de otro Estado miembro.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VentaUe {
    pub invoice_id: i64,
    pub number: String,
    /// Fecha de devengo.
    pub date: Date,
    pub cliente: String,
    pub pais: String,
    pub pais_nombre: String,
    pub base: Money,
    /// IVA español repercutido.
    pub iva_espanol: Money,
    /// IVA de destino (si se emitió por la ventanilla única).
    pub oss: Option<OssVat>,
    /// Clase (de la factura OSS o la que puso el usuario); `None`: sin clasificar.
    pub clase: Option<Clase>,
    /// La clase la da la propia factura (no se puede cambiar).
    pub clase_fija: bool,
    /// Cuenta para el umbral del art. 73.
    pub cuenta: bool,
    pub rectificativa: bool,
    /// Factura rectificada (número y fecha), si lo es.
    pub rectifica: Option<(String, Date)>,
}

#[derive(Deserialize)]
struct Guardada {
    invoice: Invoice,
    totals: Totals,
}

/// Facturas emitidas (no anuladas) a particulares de otros Estados miembros con devengo en
/// `year`.
pub fn ventas(v: &Vault, year: i32) -> Result<Vec<VentaUe>> {
    let conn = v.connection();
    let mut clases = BTreeMap::new();
    {
        let mut st = conn.prepare("SELECT invoice_id, clase FROM oss_clase_factura")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (id, c) = row?;
            if let Some(c) = Clase::parse(&c) {
                clases.insert(id, c);
            }
        }
    }
    let mut st = conn.prepare(
        "SELECT id, data FROM invoice_issued WHERE status = 'emitida'
           AND CAST(substr(issue_date, 1, 4) AS INTEGER) BETWEEN ?1 AND ?2 ORDER BY id",
    )?;
    let rows = st.query_map([year - 1, year + 1], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, data) = row?;
        let Ok(g) = serde_json::from_str::<Guardada>(&data) else {
            continue;
        };
        let Some(pais) = pais_particular_ue(&g.invoice) else {
            continue;
        };
        let date = crate::models::vat_accrual_date(&g.invoice, &g.totals.breakdown);
        if date.year() != year {
            continue;
        }
        let (clase, clase_fija) = match &g.totals.oss {
            Some(o) => (Some(Clase::of(o.supply)), true),
            None => (clases.get(&id).copied(), false),
        };
        // Los servicios a Irlanda del Norte son del Reino Unido (fuera de la UE).
        let cuenta = match clase {
            Some(Clase::Bienes) => true,
            Some(Clase::Servicios) => is_other_member_state(&pais, false),
            _ => false,
        };
        out.push(VentaUe {
            invoice_id: id,
            number: g.invoice.number.clone(),
            date,
            cliente: g
                .invoice
                .recipient
                .as_ref()
                .map(|r| r.name.clone())
                .unwrap_or_default(),
            pais_nombre: core_oss::nombre_pais(&pais),
            pais,
            base: g.totals.base,
            iva_espanol: g.totals.vat,
            oss: g.totals.oss.clone(),
            clase,
            clase_fija,
            cuenta,
            rectificativa: matches!(g.invoice.kind, InvoiceKind::Rectificativa(_)),
            rectifica: g.invoice.rectified.first().cloned(),
        });
    }
    out.sort_by_key(|a| (a.date, a.invoice_id));
    Ok(out)
}

/// Clasifica una factura a un particular de la UE para el umbral (`None` la deja sin
/// clasificar). La de una factura emitida por la ventanilla única no se cambia.
pub fn clasificar(
    v: &Vault,
    invoice_id: i64,
    clase: Option<Clase>,
    now: OffsetDateTime,
) -> Result<()> {
    let conn = v.connection();
    let data: Option<String> = conn
        .query_row(
            "SELECT data FROM invoice_issued WHERE id = ?1",
            [invoice_id],
            |r| r.get(0),
        )
        .optional()?;
    let data = data.ok_or(AppError::NotFound)?;
    let g: Guardada = serde_json::from_str(&data).map_err(|e| AppError::Other(e.to_string()))?;
    if pais_particular_ue(&g.invoice).is_none() {
        return Err(AppError::Other(
            "la factura no es de un particular de otro país de la UE".into(),
        ));
    }
    if g.totals.oss.is_some() {
        return Err(AppError::Other(
            "la factura se emitió con el IVA del país del cliente: su clase es la de la factura"
                .into(),
        ));
    }
    match clase {
        Some(c) => conn.execute(
            "INSERT INTO oss_clase_factura(invoice_id, clase, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(invoice_id) DO UPDATE SET clase = excluded.clase,
               updated_at = excluded.updated_at",
            params![invoice_id, c.code(), now_str(now)],
        )?,
        None => conn.execute(
            "DELETE FROM oss_clase_factura WHERE invoice_id = ?1",
            [invoice_id],
        )?,
    };
    audit::append(
        conn,
        &now_str(now),
        Actor::User,
        "oss.clasificar",
        "invoice_issued",
        &serde_json::json!({
            "invoice_id": invoice_id,
            "number": g.invoice.number,
            "clase": clase.map(Clase::code),
        }),
    )?;
    Ok(())
}

/// Tipo de IVA de un Estado miembro: el de la tabla y el que ha confirmado el usuario.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TipoPais {
    pub pais: String,
    pub nombre: String,
    /// Tipo general de la tabla de TaxPain.
    pub tabla: Rate,
    /// Tipo general confirmado por el usuario (el que se propone en la factura).
    pub confirmado: Option<Rate>,
    pub confirmado_at: Option<String>,
}

/// Tabla de tipos con su fecha y fuente.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Tipos {
    pub fecha: &'static str,
    pub fuente: &'static str,
    pub paises: Vec<TipoPais>,
}

pub fn tipos(v: &Vault) -> Result<Tipos> {
    let conn = v.connection();
    let mut confirmados = BTreeMap::new();
    let mut st = conn.prepare("SELECT pais, tipo_bp, confirmado_at FROM oss_tipo_pais")?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i32>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (p, bp, at) = row?;
        confirmados.insert(p, (Rate::from_bp(bp), at));
    }
    Ok(Tipos {
        fecha: core_oss::TIPOS_FECHA,
        fuente: core_oss::TIPOS_FUENTE,
        paises: core_oss::TIPOS_GENERALES
            .iter()
            .map(|(c, n, bp)| {
                let conf = confirmados.get(*c);
                TipoPais {
                    pais: (*c).to_owned(),
                    nombre: (*n).to_owned(),
                    tabla: Rate::from_bp(*bp),
                    confirmado: conf.map(|x| x.0),
                    confirmado_at: conf.map(|x| x.1.clone()),
                }
            })
            .collect(),
    })
}

/// Tipo general confirmado de `pais`.
pub fn tipo_confirmado(v: &Vault, pais: &str) -> Result<Option<Rate>> {
    Ok(v.connection()
        .query_row(
            "SELECT tipo_bp FROM oss_tipo_pais WHERE pais = ?1",
            [core_oss::codigo(pais)],
            |r| r.get::<_, i32>(0),
        )
        .optional()?
        .map(Rate::from_bp))
}

/// Antes de emitir con IVA de destino, el tipo general del país tiene que estar confirmado por
/// el usuario: la tabla de TaxPain es un dato con fecha que puede haber cambiado.
pub(crate) fn exigir_tipo_confirmado(v: &Vault, d: &InvoiceDraft) -> Result<()> {
    let (Some(_), Some(r)) = (&d.oss, &d.recipient) else {
        return Ok(());
    };
    let PartyId::Foreign { country, .. } = &r.id else {
        return Ok(());
    };
    if tipo_confirmado(v, country)?.is_none() {
        return Err(AppError::Other(format!(
            "confirma antes el tipo general de IVA de {} (Contabilidad → Impuestos → Ventanilla \
             única, o en la propia factura): la tabla de TaxPain es de {} y los tipos cambian",
            core_oss::nombre_pais(country),
            core_oss::TIPOS_FECHA
        )));
    }
    Ok(())
}

/// El usuario confirma (tras cotejarlo con la fuente oficial) el tipo general de `pais`.
pub fn confirmar_tipo(
    v: &Vault,
    pais: &str,
    rate: Rate,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let pais = core_oss::codigo(pais);
    if core_oss::tipo_general(&pais).is_none() {
        return Err(AppError::Other(format!(
            "«{pais}» no es otro Estado miembro de la UE"
        )));
    }
    if !(1..=3500).contains(&rate.bp()) {
        return Err(AppError::Other(
            "el tipo tiene que estar entre el 0,01 % y el 35 %".into(),
        ));
    }
    let conn = v.connection();
    conn.execute(
        "INSERT INTO oss_tipo_pais(pais, tipo_bp, confirmado_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(pais) DO UPDATE SET tipo_bp = excluded.tipo_bp,
           confirmado_at = excluded.confirmado_at",
        params![pais, rate.bp(), now_str(now)],
    )?;
    audit::append(
        conn,
        &now_str(now),
        Actor::User,
        "oss.confirmar_tipo",
        "oss",
        &serde_json::json!({ "pais": pais, "tipo_bp": rate.bp() }),
    )?;
    Ok(())
}

/// Situación de la ventanilla única en un año.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Estado {
    pub umbral: Umbral,
    pub limite: Money,
    pub ajustes: Ajustes,
    /// Hay que repercutir el IVA del país del cliente (umbral superado o por opción).
    pub tributa_en_destino: bool,
    /// Desde cuándo (1 de enero si se superó el año anterior o se optó).
    pub destino_desde: Option<Date>,
    /// Facturas a particulares de la UE sin clasificar: cuántas y su base.
    pub sin_clasificar: usize,
    pub sin_clasificar_base: Money,
    /// Facturas que cuentan para el umbral, emitidas con IVA español cuando ya tocaba el de
    /// destino.
    pub con_iva_espanol: Vec<String>,
    /// Falta el alta en el régimen (035).
    pub falta_035: bool,
    pub avisos: Vec<String>,
}

fn cuentan(vs: &[VentaUe]) -> Vec<(Date, Money)> {
    vs.iter()
        .filter(|x| x.cuenta)
        .map(|x| (x.date, x.base))
        .collect()
}

/// Umbral, situación y avisos de `year` con las facturas emitidas hasta hoy.
pub fn estado(v: &Vault, year: i32) -> Result<Estado> {
    let a = ajustes(v)?;
    let anteriores = ventas(v, year - 1)?;
    let actuales = ventas(v, year)?;
    let fuera = |y: i32| a.otras_ventas.get(&y).copied().unwrap_or(Money::ZERO);
    let anterior: Money = cuentan(&anteriores).iter().map(|x| x.1).sum::<Money>() + fuera(year - 1);
    let mut del_anio = cuentan(&actuales);
    // Lo vendido fuera de TaxPain se toma como del 1 de enero: si con ello se supera el umbral,
    // la fecha exacta la sabe el usuario.
    if !fuera(year).is_zero()
        && let Ok(d) = Date::from_calendar_date(year, time::Month::January, 1)
    {
        del_anio.push((d, fuera(year)));
    }
    let umbral = core_oss::umbral(year, anterior, &del_anio);
    let enero = Date::from_calendar_date(year, time::Month::January, 1).ok();
    let destino_desde = if a.opcion_destino {
        enero
    } else {
        umbral.destino_desde
    };
    let tributa_en_destino = destino_desde.is_some();
    let sin: Vec<&VentaUe> = actuales.iter().filter(|x| x.clase.is_none()).collect();
    let con_iva_espanol: Vec<String> = match destino_desde {
        Some(desde) => actuales
            .iter()
            .filter(|x| x.cuenta && x.oss.is_none() && !x.rectificativa && x.date >= desde)
            .map(|x| x.number.clone())
            .collect(),
        None => Vec::new(),
    };
    let falta_035 = tributa_en_destino && a.alta_035.is_none();
    let mut avisos = Vec::new();
    if !sin.is_empty() {
        avisos.push(format!(
            "Hay {} a particulares de otros países de la UE sin clasificar ({}): di si son \
             servicios electrónicos o de telecomunicaciones, ventas a distancia de bienes u otra \
             cosa, para calcular bien el umbral.",
            plural(sin.len()),
            sin.iter().map(|x| x.base).sum::<Money>()
        ));
    }
    if let Some(d) = destino_desde {
        if umbral.superado_anterior {
            avisos.push(format!(
                "En {} pasaste de 10.000 € ({}): en {year} todas estas ventas tributan en el país \
                 del cliente (art. 73 LIVA).",
                year - 1,
                umbral.anterior
            ));
        } else if a.opcion_destino {
            avisos.push(format!(
                "Has optado por tributar en el país del cliente desde el 1/1/{year} (art. 73 \
                 LIVA, último párrafo: la opción vale al menos dos años)."
            ));
        } else {
            avisos.push(format!(
                "Pasaste de 10.000 € el {d}: esa venta y las siguientes tributan en el país del \
                 cliente (art. 73 LIVA)."
            ));
        }
        if !con_iva_espanol.is_empty() {
            avisos.push(format!(
                "Llevan IVA español y deberían llevar el del país del cliente: {}. Rectifícalas \
                 (por diferencias) y emite la nueva con el IVA de destino.",
                con_iva_espanol.join(", ")
            ));
        }
        if falta_035 {
            // TODO(VERIFY): plazo del 035 cuando se supera el umbral: hasta el día 10 del mes
            // siguiente a la primera operación (art. 57 quinquies del Reglamento de Ejecución
            // (UE) 282/2011, en relación con el art. 163 unvicies LIVA y la Orden HAC/610/2021);
            // si se opta, antes del trimestre en que empiece a aplicarse.
            avisos.push(
                "No consta el alta en el régimen de la Unión: presenta el modelo 035 en la Sede \
                 (hasta el día 10 del mes siguiente a la primera venta que tributa en destino) y \
                 apunta la fecha en estos ajustes. Después, el 369 cada trimestre."
                    .into(),
            );
        }
    } else if umbral.en_curso >= core_oss::UMBRAL.apply_rate(Rate::percent(80)) {
        avisos.push(format!(
            "Llevas {} de los 10.000 € del umbral en {year}: la venta con la que lo pases ya \
             tributará en el país del cliente.",
            umbral.en_curso
        ));
    }
    Ok(Estado {
        limite: core_oss::UMBRAL,
        ajustes: a,
        tributa_en_destino,
        destino_desde,
        sin_clasificar: sin.len(),
        sin_clasificar_base: sin.iter().map(|x| x.base).sum(),
        con_iva_espanol,
        falta_035,
        avisos,
        umbral,
    })
}

fn plural(n: usize) -> String {
    if n == 1 {
        "1 factura".into()
    } else {
        format!("{n} facturas")
    }
}

/// Una operación del libro de la ventanilla única.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OperacionOss {
    pub invoice_id: i64,
    pub number: String,
    pub date: Date,
    pub cliente: String,
    pub pais: String,
    pub pais_nombre: String,
    pub supply: OssSupply,
    pub rate: Rate,
    pub base: Money,
    pub cuota: Money,
    /// Rectificativa de una factura de otro trimestre: (año, trimestre) que corrige.
    pub corrige: Option<(i32, u8)>,
}

fn en_trimestre(d: Date, year: i32, quarter: u8) -> bool {
    d.year() == year && core_oss::trimestre(d) == quarter
}

/// Libro de las operaciones de la ventanilla única del trimestre, aparte del 303. El registro
/// se conserva 10 años (art. 369 de la Directiva 2006/112/CE y art. 63 quater del Reglamento de
/// Ejecución (UE) 282/2011; `TODO(VERIFY)`: contenido exigido del registro).
pub fn libro(v: &Vault, year: i32, quarter: u8) -> Result<Vec<OperacionOss>> {
    let mut out = Vec::new();
    for x in ventas(v, year)? {
        let Some(o) = &x.oss else { continue };
        if !en_trimestre(x.date, year, quarter) {
            continue;
        }
        let corrige = x
            .rectifica
            .as_ref()
            .filter(|_| x.rectificativa)
            .map(|(_, d)| (d.year(), core_oss::trimestre(*d)))
            .filter(|p| *p != (year, quarter));
        out.push(OperacionOss {
            invoice_id: x.invoice_id,
            number: x.number.clone(),
            date: x.date,
            cliente: x.cliente.clone(),
            pais: o.country.clone(),
            pais_nombre: core_oss::nombre_pais(&o.country),
            supply: o.supply,
            rate: o.rate,
            base: o.base,
            cuota: o.vat,
            corrige,
        });
    }
    Ok(out)
}

/// Una fila del 369: Estado miembro de consumo, clase de operación y tipo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Linea369 {
    pub pais: String,
    pub pais_nombre: String,
    pub supply: OssSupply,
    pub rate: Rate,
    pub base: Money,
    pub cuota: Money,
}

/// Corrección de un periodo anterior (por Estado miembro).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Correccion369 {
    pub year: i32,
    pub quarter: u8,
    pub pais: String,
    pub pais_nombre: String,
    pub cuota: Money,
}

/// Línea del asiento propuesto al pagar el 369.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LineaAsiento {
    pub cuenta: String,
    pub concepto: String,
    pub debe: Money,
    pub haber: Money,
}

/// Borrador del 369 trimestral (régimen de la Unión).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Borrador369 {
    pub year: i32,
    pub quarter: u8,
    pub vencimiento: Option<Date>,
    pub lineas: Vec<Linea369>,
    pub correcciones: Vec<Correccion369>,
    /// Total a ingresar (cuotas del trimestre más correcciones).
    pub total: Money,
    /// Por Estado miembro (cuotas y correcciones), lo que se ingresa a cada uno.
    pub por_pais: Vec<(String, Money)>,
    pub asiento: Vec<LineaAsiento>,
    pub avisos: Vec<String>,
}

pub fn borrador_369(v: &Vault, year: i32, quarter: u8) -> Result<Borrador369> {
    if !(1..=4).contains(&quarter) {
        return Err(AppError::Other("trimestre no válido (1-4)".into()));
    }
    let ops = libro(v, year, quarter)?;
    let mut lineas: BTreeMap<(String, OssSupply, i32), (Money, Money)> = BTreeMap::new();
    let mut corr: BTreeMap<(i32, u8, String), Money> = BTreeMap::new();
    for o in &ops {
        match o.corrige {
            Some((y, q)) => *corr.entry((y, q, o.pais.clone())).or_default() += o.cuota,
            None => {
                let e = lineas
                    .entry((o.pais.clone(), o.supply, o.rate.bp()))
                    .or_default();
                e.0 += o.base;
                e.1 += o.cuota;
            }
        }
    }
    let lineas: Vec<Linea369> = lineas
        .into_iter()
        .map(|((pais, supply, bp), (base, cuota))| Linea369 {
            pais_nombre: core_oss::nombre_pais(&pais),
            pais,
            supply,
            rate: Rate::from_bp(bp),
            base,
            cuota,
        })
        .collect();
    let correcciones: Vec<Correccion369> = corr
        .into_iter()
        .map(|((year, quarter, pais), cuota)| Correccion369 {
            year,
            quarter,
            pais_nombre: core_oss::nombre_pais(&pais),
            pais,
            cuota,
        })
        .collect();
    let mut por_pais: BTreeMap<String, Money> = BTreeMap::new();
    for l in &lineas {
        *por_pais.entry(l.pais.clone()).or_default() += l.cuota;
    }
    for c in &correcciones {
        *por_pais.entry(c.pais.clone()).or_default() += c.cuota;
    }
    let total: Money = por_pais.values().copied().sum();
    let a = crate::accounting::posting_accounts(v.connection())?;
    let mut asiento: Vec<LineaAsiento> = por_pais
        .iter()
        .filter(|(_, m)| !m.is_zero())
        .map(|(p, m)| {
            let (debe, haber) = if m.is_negative() {
                (Money::ZERO, -*m)
            } else {
                (*m, Money::ZERO)
            };
            LineaAsiento {
                cuenta: core_oss::cuenta_oss(p, a.subaccount_width)
                    .as_str()
                    .to_owned(),
                concepto: format!(
                    "IVA {} ({p}) del 369 {quarter}T {year}",
                    core_oss::nombre_pais(p)
                ),
                debe,
                haber,
            }
        })
        .collect();
    if !total.is_zero() {
        let (debe, haber) = if total.is_negative() {
            (-total, Money::ZERO)
        } else {
            (Money::ZERO, total)
        };
        asiento.push(LineaAsiento {
            cuenta: a.bank.as_str().to_owned(),
            concepto: format!("Pago del 369 {quarter}T {year}"),
            debe,
            haber,
        });
    }
    let mut avisos = vec![
        "No hay diseño de registro del 369 en TaxPain: pasa estas cifras al formulario de la \
         Sede (régimen de la Unión). TODO(VERIFY): casillas y orden del formulario."
            .to_owned(),
    ];
    if !correcciones.is_empty() {
        avisos.push(
            "Las rectificativas de facturas de trimestres anteriores van como correcciones de \
             aquel periodo (por Estado miembro), no en las cuotas de este."
                .into(),
        );
    }
    if total.is_negative() {
        avisos.push(
            "El total sale negativo: el 369 no devuelve; el exceso lo devuelve cada Estado \
             miembro. TODO(VERIFY): cómo se consigna."
                .into(),
        );
    }
    let estado = estado(v, year)?;
    if estado.falta_035 {
        avisos.push(
            "No consta el alta en el régimen (035) en los ajustes de la ventanilla única.".into(),
        );
    }
    Ok(Borrador369 {
        year,
        quarter,
        vencimiento: core_oss::vencimiento_369(year, quarter),
        lineas,
        correcciones,
        total,
        por_pais: por_pais.into_iter().collect(),
        asiento,
        avisos,
    })
}

/// Hoja (XLSX) del borrador del 369 y del libro del trimestre: (nombre, bytes).
pub fn hoja_369(v: &Vault, year: i32, quarter: u8) -> Result<(String, Vec<u8>)> {
    use crate::celda_segura::{texto, texto_con_formato};
    use rust_xlsxwriter::{Format, Workbook};
    let b = borrador_369(v, year, quarter)?;
    let ops = libro(v, year, quarter)?;
    let x = |e: rust_xlsxwriter::XlsxError| AppError::Other(e.to_string());
    // Importe en euros para la celda (sólo para mostrarlo en la hoja).
    #[allow(clippy::float_arithmetic, clippy::cast_precision_loss)]
    let eur = |m: Money| m.cents() as f64 / 100.0;
    let bold = Format::new().set_bold();
    let money = Format::new().set_num_format("#,##0.00");
    let mut wb = Workbook::new();
    {
        let ws = wb.add_worksheet().set_name("369").map_err(x)?;
        texto_con_formato(
            ws,
            0,
            0,
            format!("Borrador del modelo 369 (régimen de la Unión) · {quarter}T {year}"),
            &bold,
        )
        .map_err(x)?;
        if let Some(d) = b.vencimiento {
            texto(ws, 1, 0, format!("Plazo: hasta el {d}")).map_err(x)?;
        }
        let mut r = 3;
        for (c, h) in [
            "Estado miembro",
            "Código",
            "Operación",
            "Tipo",
            "Base",
            "Cuota",
        ]
        .iter()
        .enumerate()
        {
            texto_con_formato(ws, r, c as u16, *h, &bold).map_err(x)?;
        }
        r += 1;
        for l in &b.lineas {
            texto(ws, r, 0, &l.pais_nombre).map_err(x)?;
            texto(ws, r, 1, &l.pais).map_err(x)?;
            texto(ws, r, 2, l.supply.label()).map_err(x)?;
            texto(ws, r, 3, l.rate.to_string()).map_err(x)?;
            ws.write_number_with_format(r, 4, eur(l.base), &money)
                .map_err(x)?;
            ws.write_number_with_format(r, 5, eur(l.cuota), &money)
                .map_err(x)?;
            r += 1;
        }
        if !b.correcciones.is_empty() {
            r += 1;
            texto_con_formato(ws, r, 0, "Correcciones de periodos anteriores", &bold).map_err(x)?;
            r += 1;
            for c in &b.correcciones {
                texto(ws, r, 0, &c.pais_nombre).map_err(x)?;
                texto(ws, r, 1, &c.pais).map_err(x)?;
                texto(ws, r, 2, format!("{}T {}", c.quarter, c.year)).map_err(x)?;
                ws.write_number_with_format(r, 5, eur(c.cuota), &money)
                    .map_err(x)?;
                r += 1;
            }
        }
        r += 1;
        texto_con_formato(ws, r, 0, "Total a ingresar", &bold).map_err(x)?;
        ws.write_number_with_format(r, 5, eur(b.total), &money)
            .map_err(x)?;
        r += 2;
        for a in &b.avisos {
            texto(ws, r, 0, a).map_err(x)?;
            r += 1;
        }
    }
    {
        let ws = wb.add_worksheet().set_name("Libro OSS").map_err(x)?;
        for (c, h) in [
            "Factura",
            "Fecha",
            "Cliente",
            "Estado miembro",
            "Operación",
            "Tipo",
            "Base",
            "Cuota",
            "Corrige",
        ]
        .iter()
        .enumerate()
        {
            texto_con_formato(ws, 0, c as u16, *h, &bold).map_err(x)?;
        }
        for (i, o) in ops.iter().enumerate() {
            let r = u32::try_from(i + 1).unwrap_or(u32::MAX);
            texto(ws, r, 0, &o.number).map_err(x)?;
            texto(ws, r, 1, o.date.to_string()).map_err(x)?;
            texto(ws, r, 2, &o.cliente).map_err(x)?;
            texto(ws, r, 3, &o.pais).map_err(x)?;
            texto(ws, r, 4, o.supply.label()).map_err(x)?;
            texto(ws, r, 5, o.rate.to_string()).map_err(x)?;
            ws.write_number_with_format(r, 6, eur(o.base), &money)
                .map_err(x)?;
            ws.write_number_with_format(r, 7, eur(o.cuota), &money)
                .map_err(x)?;
            if let Some((y, q)) = o.corrige {
                texto(ws, r, 8, format!("{q}T {y}")).map_err(x)?;
            }
        }
    }
    let bytes = wb.save_to_buffer().map_err(x)?;
    Ok((format!("369_{year}_{quarter}T.xlsx"), bytes))
}
