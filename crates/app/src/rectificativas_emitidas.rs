//! Rectificativas emitidas: motivo (R1–R5), tipo (por diferencias o por sustitución),
//! rectificativas que agrupan varias facturas y control de la numeración (RD 1619/2012 arts. 6,
//! 13 y 15; art. 80 y 89 LIVA; Orden HAC/1177/2024, `TipoFactura` y `TipoRectificativa`).
//!
//! - **Motivo**: R1 error fundado en derecho y art. 80.Uno, Dos y Seis LIVA (devoluciones,
//!   descuentos posteriores, operación anulada o resuelta, alteración del precio); R2 concurso
//!   del cliente (art. 80.Tres); R3 crédito incobrable (art. 80.Cuatro, sólo desde
//!   [`crate::incobrables`], que comprueba plazos y reclamación); R4 resto; R5 rectificativa de
//!   una simplificada (va sola: la pide la factura rectificada).
//! - **Por diferencias** (`TipoRectificativa` I): la rectificativa recoge sólo lo que cambia (en
//!   negativo para reducir). Todo (303, 349, 347, 130, libros y contabilidad) la suma tal cual.
//! - **Por sustitución** (S): recoge los importes correctos completos y, en VERI*FACTU,
//!   `ImporteRectificacion` con la base y la cuota de las facturas que sustituye. Lo que cambia
//!   es su importe menos el de las sustituidas: así lo cuentan el 303 y el 390 ([`net_breakdown`]),
//!   el 349, el 347, el 130, el libro registro, la cartera y el asiento (que sólo contabiliza la
//!   diferencia). En `invoice_issued`, `replaced_*` guarda lo sustituido para los totales por SQL.
//! - **Agrupadas**: una rectificativa puede rectificar varias facturas del mismo cliente
//!   (art. 15.2 RD 1619/2012 admite una sola rectificativa para varias facturas, siempre que se
//!   identifiquen todas); quedan en `invoice_rectified`.
//! - **Numeración**: cada clase en su serie (la de rectificativas es obligatoria, art. 6.1.a
//!   2.º), correlativa y sin huecos: [`numeracion`] lo comprueba por serie y año.
//!
//! `TODO(VERIFY)`: la frontera entre R1 y R4 no la fija ninguna norma de rango legal; se sigue
//! la descripción de las claves de la Orden HAC/1177/2024 y del SII («Error fundado en derecho y
//! Art. 80 Uno Dos y Seis LIVA», «Resto»).

use serde::{Deserialize, Serialize};
use taxpain_core::invoice::totals::line_base;
use taxpain_core::invoice::{
    Invoice, InvoiceKind, PartyId, RectificationMethod, TaxBreakdown, Totals,
};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};

use crate::error::{AppError, Result};
use crate::invoicing::InvoiceDraft;

/// Una factura rectificada tal como está en TaxPain.
#[derive(Clone, Debug)]
pub(crate) struct Original {
    pub id: i64,
    pub status: String,
    pub invoice: Invoice,
    pub totals: Totals,
    pub recipient_tax_id: Option<String>,
}

#[derive(Deserialize)]
struct Stored {
    invoice: Invoice,
    totals: Totals,
}

fn other(s: impl Into<String>) -> AppError {
    AppError::Other(s.into())
}

/// La emitida con número `number` (el número es único), con cualquier estado.
pub(crate) fn original(conn: &Connection, number: &str) -> Result<Option<Original>> {
    let row: Option<(i64, String, String, Option<String>)> = conn
        .query_row(
            "SELECT id, status, data, recipient_tax_id FROM invoice_issued WHERE number = ?1",
            [number],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((id, status, data, recipient_tax_id)) = row else {
        return Ok(None);
    };
    let st: Stored = serde_json::from_str(&data).map_err(|e| other(e.to_string()))?;
    Ok(Some(Original {
        id,
        status,
        invoice: st.invoice,
        totals: st.totals,
        recipient_tax_id,
    }))
}

/// Lo que suman las facturas que sustituye una rectificativa por sustitución (en euros): lo que
/// va en `ImporteRectificacion` y en `invoice_issued.replaced_*`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sustituido {
    pub base: Money,
    pub vat: Money,
    pub surcharge: Money,
    pub payable: Money,
}

/// Resultado de comprobar una rectificativa antes de emitirla.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Comprobada {
    /// Ids de las facturas rectificadas que están en TaxPain.
    pub originals: Vec<i64>,
    /// Por sustitución: lo sustituido.
    pub sustituido: Option<Sustituido>,
    /// Por diferencias, código 2 o 3: la factura sólo modifica la base (asiento sólo del IVA).
    pub modifica_base: bool,
}

fn tax_id(p: &PartyId) -> String {
    match p {
        PartyId::Nif(n) => n.clone(),
        PartyId::Foreign { id, .. } => id.clone(),
    }
}

fn is_substitution(kind: InvoiceKind) -> bool {
    matches!(kind, InvoiceKind::Rectificativa(r) if r.method == RectificationMethod::Sustitucion)
}

/// Comprueba una rectificativa (con sus totales ya calculados, en euros): las facturas que
/// rectifica existen y no están anuladas, la clave R5 va sólo con simplificadas, las agrupadas
/// son del mismo cliente, por sustitución sólo de facturas de TaxPain y no ya sustituidas, y R2
/// y R3 sólo por diferencias y para reducir. Las R3, además, piden el expediente del crédito
/// incobrable con los requisitos cumplidos ([`crate::incobrables::requisitos_r3`]).
pub(crate) fn comprobar(
    conn: &Connection,
    d: &InvoiceDraft,
    totals: &Totals,
) -> Result<Comprobada> {
    let InvoiceKind::Rectificativa(r) = d.kind else {
        return Ok(Comprobada::default());
    };
    let substitution = r.method == RectificationMethod::Sustitucion;
    let mut out = Comprobada::default();
    let mut seen = Vec::new();
    let mut sum = Sustituido::default();
    for (num, date) in &d.rectified {
        if seen.contains(num) {
            return Err(other(format!(
                "la factura {num} está dos veces en la rectificativa"
            )));
        }
        seen.push(num.clone());
        if *date > d.issue_date {
            return Err(other(format!(
                "la factura rectificada {num} ({date}) no puede ser posterior a la rectificativa"
            )));
        }
        let Some(o) = original(conn, num)? else {
            if substitution {
                return Err(other(format!(
                    "{num} no está en TaxPain: por sustitución sólo se rectifican facturas \
                     emitidas o importadas aquí (hace falta su importe); emítela por diferencias"
                )));
            }
            if r.code == 5 {
                return Err(other(format!(
                    "{num} no está en TaxPain: la clave R5 es sólo para rectificar una factura \
                     simplificada (F2) emitida aquí"
                )));
            }
            continue;
        };
        if o.status == "anulada" {
            return Err(other(format!(
                "{num} está anulada: una factura anulada no se rectifica"
            )));
        }
        if o.invoice.issue_date != *date {
            return Err(other(format!(
                "la factura {num} es del {}, no del {date}",
                o.invoice.issue_date
            )));
        }
        let simplified = matches!(
            o.invoice.kind,
            InvoiceKind::Simplificada
                | InvoiceKind::Rectificativa(taxpain_core::invoice::Rectification { code: 5, .. })
        );
        if simplified && r.code != 5 {
            return Err(other(format!(
                "{num} es una factura simplificada: su rectificativa lleva la clave R5"
            )));
        }
        if !simplified && r.code == 5 {
            return Err(other(format!(
                "{num} no es una simplificada: la clave R5 es sólo para rectificar facturas \
                 simplificadas"
            )));
        }
        // Agrupadas: todas del mismo cliente, y el de la rectificativa (salvo las R5, que no lo
        // llevan).
        if let Some(rec) = &d.recipient {
            let want = tax_id(&rec.id).to_uppercase();
            if let Some(have) = &o.recipient_tax_id
                && have.to_uppercase() != want
            {
                return Err(other(format!(
                    "{num} es de otro cliente ({have}): una rectificativa sólo agrupa facturas \
                     del mismo destinatario"
                )));
            }
        }
        if substitution {
            let ya: Option<String> = conn
                .query_row(
                    "SELECT i.number FROM invoice_rectified x
                     JOIN invoice_issued i ON i.id = x.rectifying_id
                     WHERE x.rectified_id = ?1 AND i.status = 'emitida' AND i.rect_method = 'S'",
                    [o.id],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(n) = ya {
                return Err(other(format!(
                    "{num} ya está sustituida por {n}: si hay que volver a corregirla, rectifica \
                     {n}"
                )));
            }
            sum.base += o.totals.base;
            sum.vat += o.totals.vat;
            sum.surcharge += o.totals.surcharge;
            sum.payable += o.totals.payable;
        }
        out.originals.push(o.id);
    }
    if matches!(r.code, 2 | 3) {
        if substitution {
            return Err(other(
                "las rectificativas por concurso (R2) o por crédito incobrable (R3) se emiten por \
                 diferencias: reducen la base en lo que no se ha cobrado",
            ));
        }
        if !totals.base.is_negative() || totals.vat.is_negative() != totals.base.is_negative() {
            return Err(other(
                "una rectificativa R2 o R3 reduce la base y la cuota: importes en negativo",
            ));
        }
        if out.originals.len() != 1 || d.rectified.len() != 1 {
            return Err(other(
                "una rectificativa R2 o R3 rectifica una sola factura emitida en TaxPain",
            ));
        }
        out.modifica_base = true;
    }
    if r.code == 3 {
        crate::incobrables::requisitos_r3(conn, out.originals[0], d.issue_date, totals)?;
    }
    if substitution {
        if let Some((b, c)) = d.rectified_amounts
            && (b != sum.base || c != sum.vat)
        {
            return Err(other(format!(
                "los importes rectificados que indicas ({b} y {c}) no son los de las facturas \
                 sustituidas ({} y {})",
                sum.base, sum.vat
            )));
        }
        out.sustituido = Some(sum);
    }
    Ok(out)
}

/// Guarda, con la rectificativa ya grabada, su tipo, lo sustituido y la lista de facturas que
/// rectifica.
pub(crate) fn guardar(
    conn: &Connection,
    invoice_id: i64,
    kind: InvoiceKind,
    c: &Comprobada,
) -> Result<()> {
    let InvoiceKind::Rectificativa(r) = kind else {
        return Ok(());
    };
    let s = c.sustituido;
    conn.execute(
        "UPDATE invoice_issued SET rect_method = ?1, replaced_base = ?2, replaced_vat = ?3,
           replaced_surcharge = ?4, replaced_payable = ?5 WHERE id = ?6",
        params![
            if r.method == RectificationMethod::Sustitucion {
                "S"
            } else {
                "I"
            },
            s.map(|s| s.base.cents()),
            s.map(|s| s.vat.cents()),
            s.map(|s| s.surcharge.cents()),
            s.map(|s| s.payable.cents()),
            invoice_id
        ],
    )?;
    for o in &c.originals {
        conn.execute(
            "INSERT OR IGNORE INTO invoice_rectified(rectifying_id, rectified_id) VALUES (?1, ?2)",
            params![invoice_id, o],
        )?;
    }
    Ok(())
}

/// Columnas `replaced_*` de una emitida.
type Replaced = (Option<i64>, Option<i64>, Option<i64>, Option<i64>);

/// Lo sustituido por una rectificativa ya emitida (para subsanar su registro VERI*FACTU).
pub(crate) fn sustituido_de(conn: &Connection, invoice_id: i64) -> Result<Option<Sustituido>> {
    let row: Option<Replaced> = conn
        .query_row(
            "SELECT replaced_base, replaced_vat, replaced_surcharge, replaced_payable
             FROM invoice_issued WHERE id = ?1",
            [invoice_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    Ok(row.and_then(|(b, v, s, p)| {
        Some(Sustituido {
            base: Money::from_cents(b?),
            vat: Money::from_cents(v?),
            surcharge: Money::from_cents(s.unwrap_or(0)),
            payable: Money::from_cents(p.unwrap_or(0)),
        })
    }))
}

/// `a − b` agrupando por tratamiento de IVA y recargo.
pub(crate) fn breakdown_diff(a: &[TaxBreakdown], b: &[TaxBreakdown]) -> Vec<TaxBreakdown> {
    let mut acc: Vec<TaxBreakdown> = a.to_vec();
    for t in b {
        match acc
            .iter_mut()
            .find(|x| x.vat == t.vat && x.surcharge_rate == t.surcharge_rate)
        {
            Some(x) => {
                x.base -= t.base;
                x.vat_amount -= t.vat_amount;
                x.surcharge_amount -= t.surcharge_amount;
            }
            None => acc.push(TaxBreakdown {
                base: -t.base,
                vat_amount: -t.vat_amount,
                surcharge_amount: -t.surcharge_amount,
                ..t.clone()
            }),
        }
    }
    acc.retain(|t| !(t.base.is_zero() && t.vat_amount.is_zero() && t.surcharge_amount.is_zero()));
    acc
}

/// Facturas que sustituye la rectificativa `invoice` (las que se encuentran; las demás, en el
/// segundo valor).
fn substituted(conn: &Connection, invoice: &Invoice) -> Result<(Vec<Original>, Vec<String>)> {
    let mut found = Vec::new();
    let mut missing = Vec::new();
    for (num, _) in &invoice.rectified {
        match original(conn, num)? {
            Some(o) => found.push(o),
            None => missing.push(num.clone()),
        }
    }
    Ok((found, missing))
}

/// Desglose de lo que **cambia** una emitida: el suyo y, si es una rectificativa por
/// sustitución, menos el de las facturas que sustituye (con aviso de las que no encuentra).
pub(crate) fn net_breakdown(
    conn: &Connection,
    invoice: &Invoice,
    totals: &Totals,
) -> Result<(Vec<TaxBreakdown>, Vec<String>)> {
    if !is_substitution(invoice.kind) {
        return Ok((totals.breakdown.clone(), vec![]));
    }
    let (found, missing) = substituted(conn, invoice)?;
    let old: Vec<TaxBreakdown> = found
        .iter()
        .flat_map(|o| o.totals.breakdown.iter().cloned())
        .collect();
    let warnings = missing
        .into_iter()
        .map(|n| {
            format!(
                "{}: no encuentro la factura sustituida {n}; se cuenta completa",
                invoice.number
            )
        })
        .collect();
    Ok((breakdown_diff(&totals.breakdown, &old), warnings))
}

/// Totales de lo que **cambia** una rectificativa por sustitución (los suyos menos los de las
/// facturas que sustituye) y la base de anticipo de las sustituidas, para su asiento. Las demás
/// facturas, tal cual y sin anticipo que restar.
pub(crate) fn net_totals(
    conn: &Connection,
    invoice: &Invoice,
    totals: &Totals,
) -> Result<(Totals, Money)> {
    if !is_substitution(invoice.kind) {
        return Ok((totals.clone(), Money::ZERO));
    }
    let (found, _) = substituted(conn, invoice)?;
    let mut t = totals.clone();
    let mut advance = Money::ZERO;
    let mut old = Vec::new();
    for o in &found {
        t.base -= o.totals.base;
        t.vat -= o.totals.vat;
        t.surcharge -= o.totals.surcharge;
        t.withholding -= o.totals.withholding;
        t.tax_total -= o.totals.tax_total;
        t.invoice_total -= o.totals.invoice_total;
        t.payable -= o.totals.payable;
        old.extend(o.totals.breakdown.iter().cloned());
        advance += o
            .invoice
            .lines
            .iter()
            .filter(|l| l.advance)
            .map(line_base)
            .sum::<Money>();
    }
    t.breakdown = breakdown_diff(&totals.breakdown, &old);
    t.withholdings = vec![];
    Ok((t, advance))
}

/// Una serie en un año (o sin reinicio anual: `year` 0) con su control de numeración.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct SerieNumeracion {
    pub serie: String,
    /// `ordinaria`, `rectificativas`, `simplificadas` o `rectificativas de simplificadas`.
    pub clase: String,
    pub year: i32,
    pub emitidas: i64,
    pub anuladas: i64,
    pub primera: Option<String>,
    pub ultima: Option<String>,
    /// Números de orden que faltan entre el 1 y el último (o el contador).
    pub huecos: Vec<i64>,
    /// Facturas con fecha anterior a la de la anterior de la serie.
    pub fuera_de_orden: Vec<String>,
    /// Facturas cuya clase no es la de la serie (p. ej. una rectificativa en la ordinaria).
    pub otra_clase: Vec<String>,
}

impl SerieNumeracion {
    pub fn correcta(&self) -> bool {
        self.huecos.is_empty() && self.fuera_de_orden.is_empty() && self.otra_clase.is_empty()
    }
}

/// Control de la numeración de las facturas emitidas con TaxPain en `year` (las de series con
/// reinicio anual de ese año y las de series continuas expedidas ese año): huecos, fechas fuera
/// de orden y facturas en una serie que no es la suya. Las anuladas conservan su número (no son
/// un hueco). Las importadas de los libros del gestor se revisan en la carpeta de la empresa.
pub fn numeracion(v: &Vault, year: i32) -> Result<Vec<SerieNumeracion>> {
    let conn = v.connection();
    let mut stmt = conn.prepare(
        "SELECT s.id, s.code, s.data FROM invoice_series s
         WHERE EXISTS(SELECT 1 FROM invoice_issued i WHERE i.series_id = s.id AND i.origin = 'app')
         ORDER BY s.code",
    )?;
    let series: Vec<(i64, String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let mut out = Vec::new();
    for (id, code, data) in series {
        let s: taxpain_core::invoice::Series =
            serde_json::from_str(&data).map_err(|e| other(e.to_string()))?;
        let clase = crate::invoicing::series_kind_label(&s);
        let y = if s.yearly_reset { year } else { 0 };
        let mut stmt = conn.prepare(
            "SELECT seq, number, issue_date, kind, status FROM invoice_issued
             WHERE series_id = ?1 AND year = ?2 AND origin = 'app' ORDER BY seq",
        )?;
        let rows: Vec<(i64, String, String, String, String)> = stmt
            .query_map(params![id, y], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
            })?
            .collect::<std::result::Result<_, _>>()?;
        // Serie continua: sólo si tiene alguna del año.
        if !s.yearly_reset && !rows.iter().any(|r| r.2.starts_with(&year.to_string())) {
            continue;
        }
        if rows.is_empty() {
            continue;
        }
        let counter: Option<i64> = conn
            .query_row(
                "SELECT last_seq FROM invoice_counter WHERE series_id = ?1 AND year = ?2",
                params![id, y],
                |r| r.get(0),
            )
            .optional()?;
        let max = rows
            .iter()
            .map(|r| r.0)
            .max()
            .unwrap_or(0)
            .max(counter.unwrap_or(0));
        let mut huecos = Vec::new();
        let mut next = 1;
        for (seq, ..) in &rows {
            while next < *seq {
                huecos.push(next);
                next += 1;
            }
            next = seq + 1;
        }
        while next <= max {
            huecos.push(next);
            next += 1;
        }
        let mut fuera = Vec::new();
        let mut last = String::new();
        for (_, number, date, ..) in &rows {
            if *date < last {
                fuera.push(number.clone());
            }
            last = last.max(date.clone());
        }
        let otra: Vec<String> = rows
            .iter()
            .filter(|(_, _, _, kind, _)| {
                let rect = kind.starts_with('R');
                let simple = kind == "F2" || kind == "R5";
                rect != s.rectificativa || simple != s.simplificada
            })
            .map(|r| r.1.clone())
            .collect();
        out.push(SerieNumeracion {
            serie: code,
            clase: clase.into(),
            year: y,
            emitidas: rows.iter().filter(|r| r.4 == "emitida").count() as i64,
            anuladas: rows.iter().filter(|r| r.4 == "anulada").count() as i64,
            primera: rows.first().map(|r| r.1.clone()),
            ultima: rows.last().map(|r| r.1.clone()),
            huecos,
            fuera_de_orden: fuera,
            otra_clase: otra,
        });
    }
    Ok(out)
}
