//! Créditos incobrables: recuperar el IVA de una factura que el cliente no paga (art. 80.Cuatro
//! LIVA; art. 24 RIVA) y enlazarlo con el deterioro contable (NRV 9.ª; art. 13.1 LIS, en
//! [`crate::ajustes_cierre`]).
//!
//! El procedimiento, por factura:
//! 1. **Plazo**: un año desde el devengo sin cobrarla, o seis meses si el volumen de operaciones
//!    del año anterior no pasó de 6.010.121,04 € (art. 80.Cuatro.A.1.ª). En las operaciones a
//!    plazos o con precio aplazado, desde el vencimiento del plazo impagado: con varios
//!    vencimientos, TaxPain cuenta desde el último pendiente.
//! 2. **Reclamación**: reclamación judicial o requerimiento notarial al deudor, o cualquier otro
//!    medio que acredite fehacientemente la reclamación (burofax con certificación de texto y
//!    acuse de recibo…), antes de rectificar (4.ª).
//! 3. **Destinatario**: empresario o profesional o, si no, base imponible de más de 50 € (3.ª).
//! 4. **Factura rectificativa R3** de la parte no cobrada, en los **seis meses** siguientes al
//!    fin del plazo del punto 1 (art. 80.Cuatro, párrafo segundo). Sólo reduce la base del IVA:
//!    la venta no se anula y el asiento es 477 a cliente por la cuota
//!    ([`crate::accounting::post_issued_invoice`]); la base se deteriora (694/490) o se da por
//!    perdida (650) en los ajustes de cierre.
//! 5. **Comunicación a la AEAT** en el plazo de **un mes** desde la R3, por la sede electrónica,
//!    con la copia de la R3 y el documento de la reclamación (art. 24.2 RIVA).
//!
//! No procede (art. 80.Cinco) con créditos con garantía real, afianzados por entidades de
//! crédito o sociedades de garantía recíproca o con seguro de crédito o caución, entre personas
//! vinculadas (art. 79.Cinco), ni si el destinatario no está establecido en el territorio de
//! aplicación del impuesto, Canarias, Ceuta o Melilla. Si el cliente está en concurso, la vía es
//! el art. 80.Tres (R2). Después de la R3, la base ya no se modifica al alza aunque se cobre
//! (salvo si el cliente no es empresario: el IVA va incluido en lo cobrado); si se desiste de la
//! reclamación judicial o se llega a un acuerdo de cobro, hay que rectificar al alza en el mes
//! siguiente.
//!
//! `TODO(VERIFY)`: los plazos (seis meses para rectificar, 50 € y «cualquier otro medio
//! fehaciente») son los de la redacción de la Ley 31/2022 (PGE 2023) del art. 80.Cuatro LIVA;
//! contrastar con el texto consolidado (BOE-A-1992-28740). El formulario de la comunicación de la
//! sede (antes modelo 952) y su código de procedimiento no se han comprobado. Las exclusiones del
//! art. 80.Cinco se declaran por el usuario (sólo la vinculación se detecta).

use serde::{Deserialize, Serialize};
use taxpain_core::invoice::{
    InvoiceKind, InvoiceLine, PartyId, Rectification, RectificationMethod, Totals, VatTreatment,
};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::{InvoiceDraft, IssuedInvoice, now_str};
use crate::recurring::months_after;

/// Meses para emitir la R3 desde que se cumple el plazo (art. 80.Cuatro, párrafo segundo).
pub const MESES_PARA_RECTIFICAR: u32 = 6;
/// Base mínima si el destinatario no es empresario ni profesional (art. 80.Cuatro.A.3.ª).
pub const BASE_MINIMA_PARTICULAR: Money = Money::from_euros(50, 0);
/// Volumen de operaciones del año anterior hasta el que vale el plazo de seis meses.
pub const VOLUMEN_PLAZO_CORTO: Money = Money::from_cents(601_012_104);

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

/// Cómo se reclamó el cobro (art. 80.Cuatro.A.4.ª).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Medio {
    /// Reclamación judicial (demanda, monitorio).
    Judicial,
    /// Requerimiento notarial.
    Notarial,
    /// Otro medio que acredite fehacientemente la reclamación (burofax con certificación de
    /// texto y acuse de recibo…).
    Fehaciente,
}

impl Medio {
    pub fn code(self) -> &'static str {
        match self {
            Medio::Judicial => "judicial",
            Medio::Notarial => "notarial",
            Medio::Fehaciente => "fehaciente",
        }
    }

    pub fn from_code(s: &str) -> Option<Medio> {
        [Medio::Judicial, Medio::Notarial, Medio::Fehaciente]
            .into_iter()
            .find(|m| m.code() == s)
    }

    fn label(self) -> &'static str {
        match self {
            Medio::Judicial => "reclamación judicial",
            Medio::Notarial => "requerimiento notarial",
            Medio::Fehaciente => "reclamación por medio fehaciente",
        }
    }
}

/// Por qué no procede la modificación (art. 80.Cinco LIVA), según el usuario.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Exclusion {
    #[default]
    Ninguna,
    /// Crédito con garantía real (en la parte garantizada).
    GarantiaReal,
    /// Afianzado por una entidad de crédito o SGR, o con seguro de crédito o caución.
    Afianzado,
    /// Cliente vinculado (art. 79.Cinco LIVA).
    Vinculada,
    /// Cliente declarado en concurso: la vía es el art. 80.Tres (R2).
    Concurso,
}

impl Exclusion {
    pub fn code(self) -> &'static str {
        match self {
            Exclusion::Ninguna => "ninguna",
            Exclusion::GarantiaReal => "garantia_real",
            Exclusion::Afianzado => "afianzado",
            Exclusion::Vinculada => "vinculada",
            Exclusion::Concurso => "concurso",
        }
    }

    pub fn from_code(s: &str) -> Exclusion {
        [
            Exclusion::GarantiaReal,
            Exclusion::Afianzado,
            Exclusion::Vinculada,
            Exclusion::Concurso,
        ]
        .into_iter()
        .find(|e| e.code() == s)
        .unwrap_or_default()
    }

    fn texto(self) -> &'static str {
        match self {
            Exclusion::Ninguna => "",
            Exclusion::GarantiaReal => {
                "El crédito tiene garantía real: no se modifica la base en la parte garantizada \
                 (art. 80.Cinco.1.ª LIVA)."
            }
            Exclusion::Afianzado => {
                "El crédito está afianzado por un banco o una SGR o cubierto por un seguro de \
                 crédito o caución: no se modifica la base (art. 80.Cinco.2.ª LIVA)."
            }
            Exclusion::Vinculada => {
                "El cliente es una persona o entidad vinculada: no se modifica la base (art. \
                 80.Cinco.3.ª LIVA)."
            }
            Exclusion::Concurso => {
                "El cliente está en concurso: la vía es la del art. 80.Tres LIVA (rectificativa \
                 R2, en Facturas), no la del 80.Cuatro."
            }
        }
    }
}

/// Fechas del procedimiento de una factura.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Calendario {
    /// Desde cuándo cuenta el plazo: el devengo o, con varios vencimientos, el último pendiente.
    pub inicio: Date,
    /// Desde cuándo se cumple el plazo de seis meses o un año.
    pub desde: Date,
    /// Último día para emitir la R3.
    pub limite_rectificar: Date,
    /// Último día para comunicarla a la AEAT (con la R3 emitida).
    pub limite_comunicar: Option<Date>,
}

/// El calendario de una factura impagada: plazo de `plazo_meses` (6 o 12) desde `inicio`, seis
/// meses más para rectificar y un mes desde la R3 (`r3`) para comunicarla.
pub fn calendario(inicio: Date, plazo_meses: u32, r3: Option<Date>) -> Calendario {
    let desde = months_after(inicio, plazo_meses);
    Calendario {
        inicio,
        desde,
        limite_rectificar: months_after(desde, MESES_PARA_RECTIFICAR),
        limite_comunicar: r3.map(|d| months_after(d, 1)),
    }
}

/// Un requisito del art. 80.Cuatro, cumplido o no, explicado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Requisito {
    pub clave: &'static str,
    pub cumplido: bool,
    pub texto: String,
}

/// Una factura impagada: en seguimiento (`id`) o candidata (sin `id`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Credito {
    pub id: Option<i64>,
    pub invoice_id: i64,
    pub number: String,
    pub cliente: String,
    pub nif: Option<String>,
    pub issue_date: Date,
    pub devengo: Date,
    pub total: Money,
    /// Pendiente de cobro de la factura (sin restar la R3).
    pub pendiente: Money,
    /// Base y cuota de la parte no cobrada (lo que reduce la R3).
    pub base_pendiente: Money,
    pub iva_pendiente: Money,
    pub plazo_meses: u32,
    /// `candidata`, `abierto`, `rectificado`, `comunicado`, `cobrado` o `desistido`.
    pub estado: String,
    pub exclusion: Exclusion,
    pub reclamacion_fecha: Option<Date>,
    pub reclamacion_medio: Option<Medio>,
    pub reclamacion_ref: Option<String>,
    pub rectificativa: Option<String>,
    pub rectificativa_fecha: Option<Date>,
    pub comunicada_el: Option<Date>,
    pub comunicacion_ref: Option<String>,
    pub notas: String,
    pub calendario: Calendario,
    pub requisitos: Vec<Requisito>,
    /// Se puede emitir la R3 hoy.
    pub puede_rectificar: bool,
    pub avisos: Vec<String>,
    /// El deterioro contable de la factura, si ya lo hay (de los ajustes de cierre).
    pub deterioro: Option<String>,
}

/// Fila de `credito_incobrable`.
struct Fila {
    id: i64,
    invoice_id: i64,
    plazo_meses: u32,
    estado: String,
    exclusion: Exclusion,
    reclamacion_fecha: Option<Date>,
    reclamacion_medio: Option<Medio>,
    reclamacion_ref: Option<String>,
    rectificativa_id: Option<i64>,
    comunicada_el: Option<Date>,
    comunicacion_ref: Option<String>,
    notas: String,
}

fn fecha_opt(s: Option<String>) -> Option<Date> {
    s.and_then(|s| {
        Date::parse(
            &s,
            time::macros::format_description!("[year]-[month]-[day]"),
        )
        .ok()
    })
}

/// `dd/mm/aaaa`.
fn fecha_es(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

const COLUMNAS: &str = "id, invoice_id, plazo_meses, estado, exclusion, reclamacion_fecha,
    reclamacion_medio, reclamacion_ref, rectificativa_id, comunicada_el, comunicacion_ref, notas";

fn fila(r: &taxpain_vault::rusqlite::Row<'_>) -> taxpain_vault::rusqlite::Result<Fila> {
    Ok(Fila {
        id: r.get(0)?,
        invoice_id: r.get(1)?,
        plazo_meses: r.get(2)?,
        estado: r.get(3)?,
        exclusion: Exclusion::from_code(&r.get::<_, String>(4)?),
        reclamacion_fecha: fecha_opt(r.get(5)?),
        reclamacion_medio: r
            .get::<_, Option<String>>(6)?
            .as_deref()
            .and_then(Medio::from_code),
        reclamacion_ref: r.get(7)?,
        rectificativa_id: r.get(8)?,
        comunicada_el: fecha_opt(r.get(9)?),
        comunicacion_ref: r.get(10)?,
        notas: r.get(11)?,
    })
}

fn load(conn: &Connection, id: i64) -> Result<Fila> {
    conn.query_row(
        &format!("SELECT {COLUMNAS} FROM credito_incobrable WHERE id = ?1"),
        [id],
        fila,
    )
    .optional()?
    .ok_or(AppError::NotFound)
}

fn of_invoice(conn: &Connection, invoice_id: i64) -> Result<Option<Fila>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLUMNAS} FROM credito_incobrable WHERE invoice_id = ?1"),
            [invoice_id],
            fila,
        )
        .optional()?)
}

/// La factura y su situación de cobro.
struct Factura {
    o: crate::rectificativas_emitidas::Original,
    origin: String,
    foreign_currency: bool,
    devengo: Date,
    pendiente: Money,
    /// Último vencimiento pendiente y cuántos vencimientos tiene.
    ultimo_vencimiento: Option<Date>,
    plazos: usize,
}

fn factura(conn: &Connection, invoice_id: i64) -> Result<Factura> {
    let (number, origin, fx): (String, String, Option<String>) = conn
        .query_row(
            "SELECT number, origin, json_extract(data, '$.fx.currency.code') FROM invoice_issued
             WHERE id = ?1",
            [invoice_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let o = crate::rectificativas_emitidas::original(conn, &number)?.ok_or(AppError::NotFound)?;
    let devengo = crate::models::vat_accrual_date(&o.invoice, &o.totals.breakdown);
    let (mut pendiente, mut ultimo, mut plazos) = (Money::ZERO, None::<Date>, 0);
    if o.status == "emitida" && origin == "app" {
        for i in crate::cartera::installments(conn, "emitida", invoice_id)? {
            plazos += 1;
            if i.status == "pendiente" && i.amount.cents() > 0 {
                pendiente += i.amount;
                ultimo = ultimo.max(Some(i.due_date));
            }
        }
    }
    Ok(Factura {
        o,
        origin,
        foreign_currency: fx.is_some(),
        devengo,
        pendiente,
        ultimo_vencimiento: ultimo,
        plazos,
    })
}

/// `a × b / c` redondeado al céntimo (mitad hacia fuera).
fn mul_div(a: Money, b: Money, c: Money) -> Money {
    let (a, b, c) = (
        i128::from(a.cents()),
        i128::from(b.cents()),
        i128::from(c.cents()),
    );
    if c == 0 {
        return Money::ZERO;
    }
    let num = a * b;
    let q = (num.abs() * 2 + c.abs()) / (c.abs() * 2);
    let q = if (num < 0) != (c < 0) { -q } else { q };
    Money::from_cents(i64::try_from(q).unwrap_or(0))
}

/// Base y cuota de la parte no cobrada, por tipo de IVA (sólo las líneas sujetas con IVA: las
/// exentas o no sujetas no tienen cuota que recuperar).
fn partes(f: &Factura) -> Vec<(VatTreatment, Option<Rate>, Money, Money)> {
    let payable = f.o.totals.payable;
    if payable.cents() <= 0 || f.pendiente.cents() <= 0 {
        return vec![];
    }
    let pend = f.pendiente.min(payable);
    f.o.totals
        .breakdown
        .iter()
        .filter(|b| matches!(b.vat, VatTreatment::Taxed { .. }) && b.vat_amount.cents() > 0)
        .map(|b| {
            (
                b.vat,
                b.surcharge_rate,
                mul_div(b.base, pend, payable),
                mul_div(b.vat_amount + b.surcharge_amount, pend, payable),
            )
        })
        .collect()
}

fn es_entidad(nif: &str) -> bool {
    nif.chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() && !matches!(c, 'X' | 'Y' | 'Z' | 'K' | 'L' | 'M'))
}

fn medio_texto(m: Option<Medio>) -> &'static str {
    m.map_or("reclamación", Medio::label)
}

/// Los requisitos del art. 80.Cuatro para emitir la R3 el día `hoy`.
fn requisitos(
    conn: &Connection,
    f: &Factura,
    plazo_meses: u32,
    exclusion: Exclusion,
    reclamacion: Option<(Date, Option<Medio>)>,
    hoy: Date,
) -> Result<(Vec<Requisito>, Calendario)> {
    let inicio = match f.ultimo_vencimiento {
        Some(v) if f.plazos > 1 => v.max(f.devengo),
        _ => f.devengo,
    };
    let cal = calendario(inicio, plazo_meses, None);
    let plazo = if plazo_meses == 6 {
        "seis meses"
    } else {
        "un año"
    };
    let desde_que = if f.plazos > 1 {
        "desde el último vencimiento impagado"
    } else {
        "desde el devengo"
    };
    let mut out = vec![Requisito {
        clave: "pendiente",
        cumplido: f.pendiente.cents() > 0 && !partes(f).is_empty(),
        texto: if f.pendiente.cents() <= 0 {
            "La factura no tiene nada pendiente de cobro.".into()
        } else if partes(f).is_empty() {
            "La factura no lleva IVA repercutido: no hay cuota que recuperar.".into()
        } else {
            format!("Quedan {} sin cobrar.", f.pendiente)
        },
    }];
    out.push(Requisito {
        clave: "plazo",
        cumplido: hoy >= cal.desde,
        texto: if hoy >= cal.desde {
            format!("Ha pasado {plazo} {desde_que} ({}).", cal.inicio)
        } else {
            format!(
                "Hasta el {} no se cumple el plazo de {plazo} {desde_que} ({}).",
                cal.desde, cal.inicio
            )
        },
    });
    out.push(Requisito {
        clave: "a_tiempo",
        cumplido: hoy <= cal.limite_rectificar,
        texto: if hoy <= cal.limite_rectificar {
            format!(
                "La rectificativa se emite como tarde el {} (seis meses desde que se cumple el \
                 plazo).",
                cal.limite_rectificar
            )
        } else {
            format!(
                "El plazo para rectificar acabó el {}: el IVA de esta factura ya no se recupera \
                 por esta vía.",
                cal.limite_rectificar
            )
        },
    });
    out.push(match reclamacion {
        Some((d, m)) if d <= hoy && m.is_some() => Requisito {
            clave: "reclamacion",
            cumplido: true,
            texto: format!("Cobro reclamado por {} el {d}.", medio_texto(m)),
        },
        _ => Requisito {
            clave: "reclamacion",
            cumplido: false,
            texto: "Falta reclamar el cobro por vía judicial, por requerimiento notarial o por \
                    otro medio fehaciente (burofax con certificación de texto y acuse de recibo)."
                .into(),
        },
    });
    let rec = f.o.invoice.recipient.as_ref();
    let base = f.o.totals.base;
    let (cumplido, texto) = match rec.map(|r| &r.id) {
        None => (
            false,
            "La factura no identifica al cliente: hace falta una factura completa.".to_owned(),
        ),
        Some(PartyId::Foreign { .. }) => (
            false,
            "El cliente no está establecido en España (territorio del IVA, Canarias, Ceuta o \
             Melilla): no se modifica la base (art. 80.Cinco LIVA)."
                .to_owned(),
        ),
        Some(PartyId::Nif(n)) if es_entidad(n) => (
            true,
            "El cliente es una empresa (actúa como empresario).".to_owned(),
        ),
        Some(PartyId::Nif(_)) if base > BASE_MINIMA_PARTICULAR => (
            true,
            format!(
                "Si el cliente es un particular, la base ({base}) pasa de {BASE_MINIMA_PARTICULAR}."
            ),
        ),
        Some(PartyId::Nif(_)) => (
            false,
            format!(
                "La base ({base}) no pasa de {BASE_MINIMA_PARTICULAR}: sólo se puede si el cliente \
                 actúa como empresario o profesional."
            ),
        ),
    };
    out.push(Requisito {
        clave: "destinatario",
        cumplido,
        texto,
    });
    let vinculada = match rec.map(|r| &r.id) {
        Some(PartyId::Nif(n)) => crate::ajustes_cierre::vinculado(conn, n)?,
        _ => false,
    };
    let exclusion = if vinculada && exclusion == Exclusion::Ninguna {
        Exclusion::Vinculada
    } else {
        exclusion
    };
    out.push(Requisito {
        clave: "exclusion",
        cumplido: exclusion == Exclusion::Ninguna,
        texto: if exclusion == Exclusion::Ninguna {
            "Sin garantías ni vinculación (art. 80.Cinco LIVA): dímelo si las hay.".into()
        } else {
            exclusion.texto().into()
        },
    });
    out.push(Requisito {
        clave: "libros",
        cumplido: true,
        texto: "Está en el libro registro de facturas expedidas de TaxPain.".into(),
    });
    Ok((out, cal))
}

fn credito(conn: &Connection, f: &Factura, fila: Option<&Fila>, hoy: Date) -> Result<Credito> {
    let plazo_meses =
        fila.map_or_else(|| plazo_por_defecto(conn, f.devengo), |x| Ok(x.plazo_meses))?;
    let exclusion = fila.map_or(Exclusion::Ninguna, |x| x.exclusion);
    let reclamacion = fila.and_then(|x| x.reclamacion_fecha.map(|d| (d, x.reclamacion_medio)));
    let (rect, rect_fecha) = match fila.and_then(|x| x.rectificativa_id) {
        Some(id) => conn
            .query_row(
                "SELECT number, issue_date FROM invoice_issued WHERE id = ?1",
                [id],
                |r| Ok((Some(r.get::<_, String>(0)?), fecha_opt(r.get(1)?))),
            )
            .optional()?
            .unwrap_or((None, None)),
        None => (None, None),
    };
    let (requisitos, mut cal) = requisitos(conn, f, plazo_meses, exclusion, reclamacion, hoy)?;
    cal.limite_comunicar = rect_fecha.map(|d| months_after(d, 1));
    let estado = fila.map_or_else(|| "candidata".to_owned(), |x| x.estado.clone());
    let puede = estado == "abierto" && requisitos.iter().all(|r| r.cumplido);
    let parts = partes(f);
    let mut avisos = Vec::new();
    if f.foreign_currency {
        avisos.push(
            "Factura en moneda extranjera: TaxPain aún no prepara su R3; hazla a mano.".into(),
        );
    }
    match estado.as_str() {
        "candidata" | "abierto" if hoy < cal.desde => {}
        "candidata" | "abierto" if hoy <= cal.limite_rectificar => {
            let dias = (cal.limite_rectificar - hoy).whole_days();
            avisos.push(format!(
                "Quedan {dias} días para emitir la rectificativa (hasta el {}).",
                cal.limite_rectificar
            ));
        }
        "rectificado" => {
            if let Some(l) = cal.limite_comunicar {
                avisos.push(if hoy <= l {
                    format!(
                        "Comunica la rectificativa a la AEAT por la sede electrónica como tarde el \
                         {l}."
                    )
                } else {
                    format!(
                        "El plazo para comunicar la rectificativa acabó el {l}: comunícala cuanto \
                         antes."
                    )
                });
            }
        }
        _ => {}
    }
    let deterioro: Option<String> = conn
        .query_row(
            "SELECT year, importe, deducible FROM ajuste_cierre WHERE tipo = 'deterioro'
               AND factura_kind = 'emitida' AND factura_id = ?1 AND reversal_year IS NULL
             ORDER BY year DESC LIMIT 1",
            [f.o.id],
            |r| {
                Ok(format!(
                    "Deterioro contable de {} en el cierre de {} ({}).",
                    Money::from_cents(r.get(1)?),
                    r.get::<_, i32>(0)?,
                    if r.get::<_, bool>(2)? {
                        "deducible en el IS"
                    } else {
                        "no deducible"
                    }
                ))
            },
        )
        .optional()?;
    Ok(Credito {
        id: fila.map(|x| x.id),
        invoice_id: f.o.id,
        number: f.o.invoice.number.clone(),
        cliente: f
            .o
            .invoice
            .recipient
            .as_ref()
            .map(|r| r.name.clone())
            .unwrap_or_default(),
        nif: f.o.recipient_tax_id.clone(),
        issue_date: f.o.invoice.issue_date,
        devengo: f.devengo,
        total: f.o.totals.payable,
        pendiente: f.pendiente,
        base_pendiente: parts.iter().map(|p| p.2).sum(),
        iva_pendiente: parts.iter().map(|p| p.3).sum(),
        plazo_meses,
        estado,
        exclusion,
        reclamacion_fecha: fila.and_then(|x| x.reclamacion_fecha),
        reclamacion_medio: fila.and_then(|x| x.reclamacion_medio),
        reclamacion_ref: fila.and_then(|x| x.reclamacion_ref.clone()),
        rectificativa: rect,
        rectificativa_fecha: rect_fecha,
        comunicada_el: fila.and_then(|x| x.comunicada_el),
        comunicacion_ref: fila.and_then(|x| x.comunicacion_ref.clone()),
        notas: fila.map(|x| x.notas.clone()).unwrap_or_default(),
        calendario: cal,
        requisitos,
        puede_rectificar: puede && !f.foreign_currency,
        avisos,
        deterioro,
    })
}

/// Volumen de operaciones aproximado de `year`: bases de las emitidas no anuladas (lo que
/// cambian las rectificativas por sustitución).
fn volumen(conn: &Connection, year: i32) -> Result<Money> {
    let cents: i64 = conn.query_row(
        "SELECT COALESCE(SUM(base - COALESCE(replaced_base, 0)), 0) FROM invoice_issued
         WHERE status = 'emitida' AND substr(issue_date, 1, 4) = ?1",
        [year.to_string()],
        |r| r.get(0),
    )?;
    Ok(Money::from_cents(cents))
}

/// Seis meses si el volumen de operaciones del año anterior al devengo no pasó de
/// 6.010.121,04 €; si no, un año.
fn plazo_por_defecto(conn: &Connection, devengo: Date) -> Result<u32> {
    Ok(
        if volumen(conn, devengo.year() - 1)? <= VOLUMEN_PLAZO_CORTO {
            6
        } else {
            12
        },
    )
}

/// Las facturas emitidas vencidas y sin cobrar (candidatas) y las que ya están en seguimiento.
pub fn list(v: &Vault, hoy: Date) -> Result<Vec<Credito>> {
    let conn = v.connection();
    let mut out = Vec::new();
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNAS} FROM credito_incobrable ORDER BY id"
    ))?;
    let filas: Vec<Fila> = stmt
        .query_map([], fila)?
        .collect::<std::result::Result<_, _>>()?;
    let seguidas: Vec<i64> = filas.iter().map(|f| f.invoice_id).collect();
    for x in &filas {
        let f = factura(conn, x.invoice_id)?;
        out.push(credito(conn, &f, Some(x), hoy)?);
    }
    let mut vistas = Vec::new();
    for i in crate::cartera::list(v, hoy)? {
        if i.kind != "emitida"
            || i.status != "pendiente"
            || i.amount.cents() <= 0
            || i.overdue_days <= 0
            || i.invoice_id <= 0
            || seguidas.contains(&i.invoice_id)
            || vistas.contains(&i.invoice_id)
        {
            continue;
        }
        vistas.push(i.invoice_id);
        let f = factura(conn, i.invoice_id)?;
        if !matches!(
            f.o.invoice.kind,
            InvoiceKind::Completa | InvoiceKind::SustitutivaDeSimplificadas
        ) || partes(&f).is_empty()
        {
            continue;
        }
        out.push(credito(conn, &f, None, hoy)?);
    }
    Ok(out)
}

fn audit_log(
    conn: &Connection,
    now: OffsetDateTime,
    action: &str,
    subject: &str,
    d: serde_json::Value,
) -> Result<()> {
    audit::append(conn, &now_str(now), Actor::User, action, subject, &d)?;
    Ok(())
}

/// Abre el seguimiento de una factura impagada con el plazo de `plazo_meses` (6 o 12; `None`,
/// el que corresponde por el volumen de operaciones).
pub fn abrir(
    v: &Vault,
    invoice_id: i64,
    plazo_meses: Option<u32>,
    exclusion: Exclusion,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<i64> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let f = factura(&tx, invoice_id)?;
    if f.o.status != "emitida" || f.origin != "app" {
        return Err(other(
            "sólo las facturas emitidas con TaxPain y sin anular (las importadas se cobran en la \
             contabilidad del gestor)",
        ));
    }
    if !matches!(
        f.o.invoice.kind,
        InvoiceKind::Completa | InvoiceKind::SustitutivaDeSimplificadas
    ) {
        return Err(other(
            "sólo de facturas completas: una rectificativa o una simplificada no se sigue como \
             crédito incobrable",
        ));
    }
    if f.pendiente.cents() <= 0 {
        return Err(other("la factura no tiene nada pendiente de cobro"));
    }
    if of_invoice(&tx, invoice_id)?.is_some() {
        return Err(other("esa factura ya está en seguimiento"));
    }
    let defecto = plazo_por_defecto(&tx, f.devengo)?;
    let plazo = plazo_meses.unwrap_or(defecto);
    if plazo != 6 && plazo != 12 {
        return Err(other("el plazo es de 6 o de 12 meses"));
    }
    if plazo == 6 && defecto == 12 {
        return Err(other(format!(
            "el volumen de operaciones de {} pasó de 6.010.121,04 €: el plazo es de un año (art. \
             80.Cuatro LIVA)",
            f.devengo.year() - 1
        )));
    }
    let at = now_str(now);
    tx.execute(
        "INSERT INTO credito_incobrable(invoice_id, plazo_meses, exclusion, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?4)",
        params![invoice_id, plazo, exclusion.code(), at],
    )?;
    let id = tx.last_insert_rowid();
    audit_log(
        &tx,
        now,
        "incobrable.abrir",
        &f.o.invoice.number,
        serde_json::json!({ "plazo_meses": plazo, "exclusion": exclusion.code() }),
    )?;
    tx.commit()?;
    Ok(id)
}

fn hoy(now: OffsetDateTime) -> Date {
    taxpain_core::spain_time::peninsular_date(now)
}

/// Cambia el plazo o la exclusión de un seguimiento abierto.
pub fn ajustar(
    v: &Vault,
    id: i64,
    plazo_meses: u32,
    exclusion: Exclusion,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let x = load(&tx, id)?;
    if x.estado != "abierto" {
        return Err(other("sólo antes de emitir la rectificativa"));
    }
    let f = factura(&tx, x.invoice_id)?;
    if plazo_meses != 6 && plazo_meses != 12 {
        return Err(other("el plazo es de 6 o de 12 meses"));
    }
    if plazo_meses == 6 && plazo_por_defecto(&tx, f.devengo)? == 12 {
        return Err(other(
            "el volumen de operaciones del año anterior pasó de 6.010.121,04 €: el plazo es de un \
             año",
        ));
    }
    tx.execute(
        "UPDATE credito_incobrable SET plazo_meses = ?1, exclusion = ?2, updated_at = ?3 WHERE id = ?4",
        params![plazo_meses, exclusion.code(), now_str(now), id],
    )?;
    audit_log(
        &tx,
        now,
        "incobrable.ajustar",
        &f.o.invoice.number,
        serde_json::json!({ "plazo_meses": plazo_meses, "exclusion": exclusion.code() }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Anota la reclamación del cobro (fecha, medio y referencia: número de procedimiento, de
/// protocolo notarial o del burofax).
pub fn reclamar(
    v: &Vault,
    id: i64,
    fecha: Date,
    medio: Medio,
    referencia: &str,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    if fecha > hoy(now) {
        return Err(other(
            "la fecha de la reclamación no puede ser posterior a hoy",
        ));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let x = load(&tx, id)?;
    if x.estado != "abierto" {
        return Err(other(
            "la reclamación se anota antes de emitir la rectificativa",
        ));
    }
    let f = factura(&tx, x.invoice_id)?;
    if fecha < f.o.invoice.issue_date {
        return Err(other("la reclamación no puede ser anterior a la factura"));
    }
    let r = referencia.trim();
    tx.execute(
        "UPDATE credito_incobrable SET reclamacion_fecha = ?1, reclamacion_medio = ?2,
           reclamacion_ref = ?3, updated_at = ?4 WHERE id = ?5",
        params![
            fecha.to_string(),
            medio.code(),
            (!r.is_empty()).then_some(r),
            now_str(now),
            id
        ],
    )?;
    audit_log(
        &tx,
        now,
        "incobrable.reclamar",
        &f.o.invoice.number,
        serde_json::json!({ "fecha": fecha.to_string(), "medio": medio.code(), "referencia": r }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Lo que se va a emitir: el borrador de la R3 de la parte no cobrada y sus requisitos.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PropuestaR3 {
    pub draft: InvoiceDraft,
    pub base: Money,
    pub iva: Money,
    pub requisitos: Vec<Requisito>,
    pub puede: bool,
}

fn borrador(f: &Factura, x: &Fila, fecha: Date) -> InvoiceDraft {
    let o = &f.o.invoice;
    let lines = partes(f)
        .into_iter()
        .map(|(vat, surcharge, base, _)| InvoiceLine {
            description: format!(
                "Modificación de la base imponible por crédito incobrable de la factura {} del {} \
                 (art. 80.Cuatro LIVA)",
                o.number,
                fecha_es(o.issue_date)
            ),
            quantity_milli: 1000,
            unit_price: -base,
            discount: Rate::ZERO,
            vat,
            surcharge,
            withholding: None,
            advance: false,
        })
        .collect();
    let reclamada = x.reclamacion_fecha.map(|d| {
        format!(
            "Cobro reclamado por {} el {}{}.",
            medio_texto(x.reclamacion_medio),
            fecha_es(d),
            x.reclamacion_ref
                .as_deref()
                .map(|r| format!(" ({r})"))
                .unwrap_or_default()
        )
    });
    InvoiceDraft {
        kind: InvoiceKind::Rectificativa(Rectification {
            code: 3,
            method: RectificationMethod::Diferencias,
        }),
        series: None,
        issue_date: fecha,
        operation_date: None,
        recipient: o.recipient.clone(),
        lines,
        rectified: vec![(o.number.clone(), o.issue_date)],
        rectified_amounts: None,
        description: format!(
            "Rectificación por crédito total o parcialmente incobrable de la factura {}",
            o.number
        ),
        notes: reclamada,
        currency: None,
        oss: None,
    }
}

/// La R3 que se emitiría el día `fecha`.
pub fn propuesta(v: &Vault, id: i64, fecha: Date) -> Result<PropuestaR3> {
    let conn = v.connection();
    let x = load(conn, id)?;
    let f = factura(conn, x.invoice_id)?;
    let (requisitos, _) = requisitos(
        conn,
        &f,
        x.plazo_meses,
        x.exclusion,
        x.reclamacion_fecha.map(|d| (d, x.reclamacion_medio)),
        fecha,
    )?;
    let parts = partes(&f);
    Ok(PropuestaR3 {
        draft: borrador(&f, &x, fecha),
        base: -parts.iter().map(|p| p.2).sum::<Money>(),
        iva: -parts.iter().map(|p| p.3).sum::<Money>(),
        puede: x.estado == "abierto"
            && !f.foreign_currency
            && requisitos.iter().all(|r| r.cumplido),
        requisitos,
    })
}

/// Lo que exige [`crate::rectificativas_emitidas::comprobar`] para emitir una R3 de la factura
/// `invoice_id` el día `fecha`: su seguimiento abierto con todos los requisitos cumplidos y que
/// no reduzca más de la parte no cobrada.
pub(crate) fn requisitos_r3(
    conn: &Connection,
    invoice_id: i64,
    fecha: Date,
    totals: &Totals,
) -> Result<()> {
    let Some(x) = of_invoice(conn, invoice_id)? else {
        return Err(other(
            "la rectificativa por crédito incobrable (R3) se emite desde Cobros y pagos › \
             Créditos incobrables, que comprueba los plazos y la reclamación",
        ));
    };
    if x.estado != "abierto" {
        return Err(other(
            "ese crédito incobrable ya está rectificado o cerrado",
        ));
    }
    let f = factura(conn, invoice_id)?;
    if f.foreign_currency {
        return Err(other(
            "la R3 de una factura en moneda extranjera aún no la prepara TaxPain",
        ));
    }
    let (reqs, _) = requisitos(
        conn,
        &f,
        x.plazo_meses,
        x.exclusion,
        x.reclamacion_fecha.map(|d| (d, x.reclamacion_medio)),
        fecha,
    )?;
    let faltan: Vec<String> = reqs
        .into_iter()
        .filter(|r| !r.cumplido)
        .map(|r| r.texto)
        .collect();
    if !faltan.is_empty() {
        return Err(other(format!(
            "todavía no se puede emitir la R3: {}",
            faltan.join(" ")
        )));
    }
    let parts = partes(&f);
    let base: Money = parts.iter().map(|p| p.2).sum();
    let iva: Money = parts.iter().map(|p| p.3).sum();
    if -totals.base > base || -(totals.vat + totals.surcharge) > iva {
        return Err(other(format!(
            "la R3 no puede reducir más de la parte no cobrada ({base} de base y {iva} de cuota)"
        )));
    }
    Ok(())
}

/// Emite la R3 (en la serie de rectificativas, con su registro VERI*FACTU si está activo) y
/// deja el seguimiento como rectificado.
pub fn emitir_r3(
    v: &Vault,
    id: i64,
    fecha: Date,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<IssuedInvoice> {
    let conn = v.connection();
    let x = load(conn, id)?;
    let f = factura(conn, x.invoice_id)?;
    let draft = borrador(&f, &x, fecha);
    let number = f.o.invoice.number.clone();
    crate::invoicing::issue_linked(v, &draft, None, now, confirmed, |tx, invoice_id| {
        tx.execute(
            "UPDATE credito_incobrable SET rectificativa_id = ?1, estado = 'rectificado',
               updated_at = ?2 WHERE id = ?3",
            params![invoice_id, now_str(now), id],
        )?;
        audit_log(
            tx,
            now,
            "incobrable.rectificar",
            &number,
            serde_json::json!({ "rectificativa_id": invoice_id }),
        )
    })
}

/// Los datos que pide la comunicación de la modificación de la base en la sede electrónica.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Comunicacion {
    pub campos: Vec<(String, String)>,
    pub adjuntos: Vec<String>,
    pub plazo: Option<Date>,
    pub donde: String,
}

/// Lo que hay que tener a mano para comunicar la R3 a la AEAT (TaxPain no la envía).
pub fn comunicacion(v: &Vault, id: i64) -> Result<Comunicacion> {
    let conn = v.connection();
    let x = load(conn, id)?;
    let f = factura(conn, x.invoice_id)?;
    let s = crate::invoicing::require_settings(v)?;
    let r3 = match x.rectificativa_id {
        Some(rid) => {
            let n: String = conn.query_row(
                "SELECT number FROM invoice_issued WHERE id = ?1",
                [rid],
                |r| r.get(0),
            )?;
            crate::rectificativas_emitidas::original(conn, &n)?
        }
        None => None,
    };
    let Some(r3) = r3 else {
        return Err(other("primero emite la rectificativa R3"));
    };
    let emisor = match &s.issuer.id {
        PartyId::Nif(n) => n.clone(),
        PartyId::Foreign { id, .. } => id.clone(),
    };
    let o = &f.o;
    let campos = vec![
        ("NIF del emisor (acreedor)".into(), emisor),
        (
            "Nombre o razón social del emisor".into(),
            s.issuer.name.clone(),
        ),
        (
            "NIF del destinatario (deudor)".into(),
            o.recipient_tax_id.clone().unwrap_or_default(),
        ),
        (
            "Nombre del destinatario".into(),
            o.invoice
                .recipient
                .as_ref()
                .map(|r| r.name.clone())
                .unwrap_or_default(),
        ),
        ("Factura original".into(), o.invoice.number.clone()),
        (
            "Fecha de la factura original".into(),
            fecha_es(o.invoice.issue_date),
        ),
        ("Fecha de devengo".into(), fecha_es(f.devengo)),
        ("Base imponible original".into(), o.totals.base.to_string()),
        (
            "Cuota original".into(),
            (o.totals.vat + o.totals.surcharge).to_string(),
        ),
        ("Factura rectificativa".into(), r3.invoice.number.clone()),
        (
            "Fecha de la rectificativa".into(),
            fecha_es(r3.invoice.issue_date),
        ),
        (
            "Base imponible modificada".into(),
            r3.totals.base.to_string(),
        ),
        (
            "Cuota modificada".into(),
            (r3.totals.vat + r3.totals.surcharge).to_string(),
        ),
        (
            "Reclamación".into(),
            match x.reclamacion_fecha {
                Some(d) => format!(
                    "{} del {}{}",
                    medio_texto(x.reclamacion_medio),
                    fecha_es(d),
                    x.reclamacion_ref
                        .as_deref()
                        .map(|r| format!(" ({r})"))
                        .unwrap_or_default()
                ),
                None => String::new(),
            },
        ),
    ];
    Ok(Comunicacion {
        campos,
        adjuntos: vec![
            format!("Copia de la factura rectificativa {}.", r3.invoice.number),
            match x.reclamacion_medio {
                Some(Medio::Judicial) => {
                    "Copia de la reclamación judicial (demanda o monitorio admitido a trámite)."
                        .into()
                }
                Some(Medio::Notarial) => "Copia del requerimiento notarial.".into(),
                _ => "Documento que acredite la reclamación fehaciente (burofax con \
                      certificación de texto y acuse de recibo)."
                    .into(),
            },
        ],
        plazo: Some(months_after(r3.invoice.issue_date, 1)),
        // TODO(VERIFY): nombre exacto del trámite en la sede (antes modelo 952).
        donde: "Sede electrónica de la AEAT › IVA › Comunicación de la modificación de la base \
                imponible (créditos incobrables y concurso de acreedores). TaxPain no la envía."
            .into(),
    })
}

/// Anota que la R3 se comunicó a la AEAT (fecha y número de registro o CSV del justificante).
pub fn comunicada(
    v: &Vault,
    id: i64,
    fecha: Date,
    referencia: &str,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    if fecha > hoy(now) {
        return Err(other(
            "la fecha de la comunicación no puede ser posterior a hoy",
        ));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let x = load(&tx, id)?;
    if x.estado != "rectificado" {
        return Err(other(
            "la comunicación se anota con la rectificativa emitida",
        ));
    }
    let r3_date: Option<String> = tx
        .query_row(
            "SELECT issue_date FROM invoice_issued WHERE id = ?1",
            [x.rectificativa_id],
            |r| r.get(0),
        )
        .optional()?;
    if fecha_opt(r3_date).is_some_and(|d| fecha < d) {
        return Err(other(
            "la comunicación no puede ser anterior a la rectificativa",
        ));
    }
    let r = referencia.trim();
    tx.execute(
        "UPDATE credito_incobrable SET estado = 'comunicado', comunicada_el = ?1,
           comunicacion_ref = ?2, updated_at = ?3 WHERE id = ?4",
        params![
            fecha.to_string(),
            (!r.is_empty()).then_some(r),
            now_str(now),
            id
        ],
    )?;
    let f = factura(&tx, x.invoice_id)?;
    audit_log(
        &tx,
        now,
        "incobrable.comunicar",
        &f.o.invoice.number,
        serde_json::json!({ "fecha": fecha.to_string(), "referencia": r }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Cierra el seguimiento: `cobrado` (se cobró antes de rectificar) o `desistido` (después de la
/// R3 se desistió de la reclamación judicial o hubo acuerdo de cobro: hay que rectificar al alza
/// en el mes siguiente). Devuelve lo que queda por hacer.
pub fn cerrar(
    v: &Vault,
    id: i64,
    estado: &str,
    fecha: Date,
    notas: &str,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<String> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let x = load(&tx, id)?;
    let siguiente = match (estado, x.estado.as_str()) {
        ("cobrado", "abierto") => {
            "Seguimiento cerrado: la factura se cobró antes de rectificar.".to_owned()
        }
        ("desistido", "rectificado" | "comunicado") => format!(
            "Como se desistió de la reclamación o hubo acuerdo de cobro, emite en Facturas una \
             rectificativa al alza de la cuota como tarde el {} (art. 80.Cuatro LIVA).",
            months_after(fecha, 1)
        ),
        ("cobrado", _) => {
            return Err(other(
                "con la R3 emitida, cobrar después no cambia la base si el cliente es empresario \
                 (art. 80.Cuatro LIVA); si es un particular, el IVA va incluido en lo cobrado: \
                 rectifica al alza en Facturas",
            ));
        }
        ("desistido", _) => {
            return Err(other("el desistimiento se anota después de emitir la R3"));
        }
        _ => return Err(other("estado no válido")),
    };
    tx.execute(
        "UPDATE credito_incobrable SET estado = ?1, cerrado_el = ?2, notas = ?3, updated_at = ?4
         WHERE id = ?5",
        params![estado, fecha.to_string(), notas.trim(), now_str(now), id],
    )?;
    let f = factura(&tx, x.invoice_id)?;
    audit_log(
        &tx,
        now,
        "incobrable.cerrar",
        &f.o.invoice.number,
        serde_json::json!({ "estado": estado, "fecha": fecha.to_string() }),
    )?;
    tx.commit()?;
    Ok(siguiente)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn calendario_de_seis_meses_y_de_un_ano() {
        let c = calendario(date!(2025 - 03 - 31), 6, None);
        assert_eq!(c.desde, date!(2025 - 09 - 30));
        assert_eq!(c.limite_rectificar, date!(2026 - 03 - 30));
        assert_eq!(c.limite_comunicar, None);
        let c = calendario(date!(2025 - 01 - 15), 12, Some(date!(2026 - 02 - 10)));
        assert_eq!(c.desde, date!(2026 - 01 - 15));
        assert_eq!(c.limite_rectificar, date!(2026 - 07 - 15));
        assert_eq!(c.limite_comunicar, Some(date!(2026 - 03 - 10)));
    }

    #[test]
    fn parte_proporcional_redondeada() {
        let m = |c| Money::from_cents(c);
        assert_eq!(mul_div(m(100_000), m(60_500), m(121_000)), m(50_000));
        assert_eq!(mul_div(m(1), m(1), m(2)), m(1));
        assert_eq!(mul_div(m(-1), m(1), m(2)), m(-1));
    }

    #[test]
    fn empresa_o_particular_por_el_nif() {
        assert!(es_entidad("B12345674"));
        assert!(es_entidad("A22222228"));
        assert!(!es_entidad("12345678Z"));
        assert!(!es_entidad("X1234567L"));
    }
}
