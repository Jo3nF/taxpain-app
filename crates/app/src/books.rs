//! Libros registro de IVA/IRPF en XLSX con el formato común de la AEAT (importables en Pre303,
//! modelo 130 y Renta Web). Fichero unificado tipo «T»: EXPEDIDAS_INGRESOS y RECIBIDAS_GASTOS.
//! `TODO(VERIFY)`: la especificación no fija la fila de inicio de datos; se reproduce la
//! plantilla oficial (cabeceras en las filas 1–10) y los asientos empiezan en la fila 11.

use rust_xlsxwriter::{Format, Workbook, Worksheet, XlsxError};
use taxpain_core::Nif;
use taxpain_core::invoice::{Invoice, PartyId, Totals};
use taxpain_core::money::{Money, Rate};
use taxpain_invoicing::books::{
    Activity, BookContext, Cell, ISSUED_SHEET, RECEIVED_SHEET, ReceivedRecord, ReceivedRegime,
    expense_concept, issued_header, issued_rows, received_header, received_rows,
};
use taxpain_vault::Vault;
use time::Date;

use crate::error::{AppError, Result};
use crate::invoicing;

const FIRST_DATA_ROW: u32 = 10; // fila 11 (0-based)

pub(crate) fn xlsx(e: XlsxError) -> AppError {
    AppError::Other(format!("XLSX: {e}"))
}

#[allow(clippy::float_arithmetic, clippy::cast_precision_loss)] // sólo para escribir la celda numérica
pub(crate) fn write_cell(
    ws: &mut Worksheet,
    row: u32,
    col: u16,
    cell: &Cell,
    two_dec: &Format,
) -> Result<()> {
    match cell {
        Cell::Empty => {}
        Cell::Text(s) => {
            crate::celda_segura::texto(ws, row, col, s).map_err(xlsx)?;
        }
        Cell::Amount(m) => {
            ws.write_number_with_format(row, col, m.cents() as f64 / 100.0, two_dec)
                .map_err(xlsx)?;
        }
        Cell::Rate(r) => {
            ws.write_number_with_format(row, col, f64::from(r.bp()) / 100.0, two_dec)
                .map_err(xlsx)?;
        }
        Cell::Int(i) => {
            ws.write_number(row, col, *i as f64).map_err(xlsx)?;
        }
        Cell::Date(d) => {
            crate::celda_segura::texto(
                ws,
                row,
                col,
                format!("{:02}/{:02}/{}", d.day(), d.month() as u8, d.year()),
            )
            .map_err(xlsx)?;
        }
    }
    Ok(())
}

/// Titular de los libros (cabecera de cada hoja).
struct Holder<'a> {
    year: i32,
    nif: &'a str,
    name: &'a str,
}

fn sheet(
    wb: &mut Workbook,
    (name, title): (&str, &str),
    holder: &Holder<'_>,
    header: &[Vec<(usize, &'static str)>],
    rows: &[Vec<Cell>],
) -> Result<()> {
    let bold = Format::new().set_bold();
    let two_dec = Format::new().set_num_format("0.00");
    let ws = wb.add_worksheet();
    ws.set_name(name).map_err(xlsx)?;
    crate::celda_segura::texto_con_formato(ws, 0, 9, title, &bold).map_err(xlsx)?;
    crate::celda_segura::texto(ws, 1, 8, format!("Ejercicio: {}", holder.year)).map_err(xlsx)?;
    crate::celda_segura::texto(ws, 2, 8, format!("NIF: {}", holder.nif)).map_err(xlsx)?;
    crate::celda_segura::texto(ws, 4, 8, format!("NOMBRE O RAZÓN SOCIAL: {}", holder.name))
        .map_err(xlsx)?;
    for (i, line) in header.iter().enumerate() {
        let row = 6 + u32::try_from(i).unwrap_or(0);
        for (col, text) in line {
            crate::celda_segura::texto_con_formato(
                ws,
                row,
                u16::try_from(*col).unwrap_or(0),
                *text,
                &bold,
            )
            .map_err(xlsx)?;
        }
    }
    for (i, r) in rows.iter().enumerate() {
        let row = FIRST_DATA_ROW
            + u32::try_from(i).map_err(|_| AppError::Other("demasiadas filas".into()))?;
        for (col, cell) in r.iter().enumerate() {
            write_cell(ws, row, u16::try_from(col).unwrap_or(0), cell, &two_dec)?;
        }
    }
    Ok(())
}

/// Tipo de IVA estándar más cercano a cuota/base (las recibidas guardan sólo importes).
fn infer_rate(base: Money, vat: Money) -> Rate {
    if base.is_zero() {
        return Rate::ZERO;
    }
    let candidates = [0, 400, 1000, 2100];
    let best = candidates
        .iter()
        .min_by_key(|&&bp| (base.apply_rate(Rate::from_bp(bp)) - vat).abs().cents())
        .copied()
        .unwrap_or(2100);
    Rate::from_bp(best)
}

fn infer_withholding_rate(base: Money, wh: Money) -> Option<Rate> {
    if wh.is_zero() {
        return None;
    }
    [700, 1500, 1900, 100, 200]
        .iter()
        .map(|&bp| Rate::from_bp(bp))
        .min_by_key(|r| (base.apply_rate(*r) - wh).abs().cents())
}

/// Libros del ejercicio `year`: (nombre de fichero, contenido XLSX).
pub fn lsi_workbook(v: &Vault, year: i32) -> Result<(String, Vec<u8>)> {
    let s = invoicing::settings(v)?.ok_or(AppError::NotConfigured)?;
    let PartyId::Nif(nif) = &s.issuer.id else {
        return Err(AppError::Other("el titular debe tener NIF".into()));
    };
    let persona_fisica = Nif::parse(nif)
        .map(|n| n.is_persona_fisica())
        .unwrap_or(false);
    let ctx = BookContext {
        activity: s.activity.clone().unwrap_or(Activity {
            code: "A".into(),
            kind: if persona_fisica {
                "05".into()
            } else {
                "03".into()
            },
            iae: String::new(),
        }),
        persona_fisica,
    };
    let conn = v.connection();

    // Emitidas (las anuladas no se registran), por el ejercicio de devengo del IVA: el de la
    // fecha de operación si es anterior a la expedición (validación 11 del LSI; como el 303), así
    // que una expedida en enero por una operación de diciembre va al libro del año anterior.
    let mut issued = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT data FROM invoice_issued WHERE status = 'emitida'
               AND CAST(substr(issue_date, 1, 4) AS INTEGER) BETWEEN ?1 AND ?2
             ORDER BY issue_date, id",
        )?;
        let rows = stmt.query_map([year, year + 1], |r| r.get::<_, String>(0))?;
        for data in rows {
            #[derive(serde::Deserialize)]
            struct Stored {
                invoice: Invoice,
                totals: Totals,
            }
            let st: Stored =
                serde_json::from_str(&data?).map_err(|e| AppError::Other(e.to_string()))?;
            let accrual = crate::models::vat_accrual_date(&st.invoice, &st.totals.breakdown);
            if accrual.year() != year {
                continue;
            }
            // Rectificativa por sustitución: en el libro, lo que cambia (su importe menos el de
            // las facturas que sustituye), como en el 303; si no, Pre303 y Renta Web la sumarían
            // entera además de la sustituida.
            let (breakdown, _) =
                crate::rectificativas_emitidas::net_breakdown(conn, &st.invoice, &st.totals)?;
            let totals = Totals {
                breakdown,
                ..st.totals.clone()
            };
            issued.extend(issued_rows(&st.invoice, &totals, accrual, &ctx));
        }
    }

    // Recibidas revisadas.
    let mut received = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT id, COALESCE(supplier_nif, ''), COALESCE(supplier_name, ''), number, issue_date, substr(reviewed_at, 1, 10), base, vat, withholding, deductible_pct,
                    capital_good, vat_regime, self_assessed_rate, supplier_country
             FROM invoice_received WHERE status = 'revisada' AND substr(reviewed_at, 1, 4) = ?1 ORDER BY reviewed_at, id",
        )?;
        let rows = stmt.query_map([year.to_string()], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, String>(5)?,
                r.get::<_, i64>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?,
                r.get::<_, i64>(9)?,
                r.get::<_, bool>(10)?,
                r.get::<_, String>(11)?,
                r.get::<_, Option<i64>>(12)?,
                r.get::<_, Option<String>>(13)?,
            ))
        })?;
        let date = |s: &str| {
            Date::parse(s, time::macros::format_description!("[year]-[month]-[day]"))
                .map_err(|e| AppError::Other(e.to_string()))
        };
        for row in rows {
            let (
                id,
                nif,
                name,
                number,
                issue,
                received_on,
                base,
                vat,
                wh,
                pct,
                capital_good,
                regime,
                rate,
                country,
            ) = row?;
            let (base, wh) = (Money::from_cents(base), Money::from_cents(wh));
            let regime = match regime.as_str() {
                "intracomunitaria" => ReceivedRegime::Intracomunitaria,
                "isp" => ReceivedRegime::Isp,
                _ => ReceivedRegime::Interior,
            };
            // Con inversión del sujeto pasivo, la cuota del libro es la autorrepercutida.
            let vat = match regime {
                ReceivedRegime::Interior => Money::from_cents(vat),
                _ => base.apply_rate(Rate::from_bp(
                    i32::try_from(rate.unwrap_or(2100)).unwrap_or(2100),
                )),
            };
            let deductible = vat.apply_rate(Rate::percent(i32::try_from(pct).unwrap_or(100)));
            // Expedidor extranjero: tipo 02 (NIF-IVA) si es de la UE; 06 (otro documento) si no.
            let supplier = match country {
                Some(c) => PartyId::Foreign {
                    id: nif
                        .strip_prefix(taxpain_core::eu::vies_prefix(&c).as_str())
                        .unwrap_or(&nif)
                        .to_owned(),
                    id_type: if taxpain_core::eu::is_other_member_state(&c, true) {
                        taxpain_core::invoice::IdType::NifIva
                    } else {
                        taxpain_core::invoice::IdType::Otro
                    },
                    country: c,
                },
                None => PartyId::Nif(nif),
            };
            let rec = ReceivedRecord {
                internal_number: format!("R{id}"),
                supplier,
                supplier_name: name,
                number,
                issue_date: date(&issue)?,
                received_date: date(&received_on)?,
                base,
                vat_rate: infer_rate(base, vat),
                vat,
                deductible_vat: deductible,
                withholding_rate: infer_withholding_rate(base, wh),
                withholding: wh,
                expense_concept: expense_concept(
                    crate::accounting::posting_accounts(conn)?.expense.as_str(),
                )
                .into(),
                capital_good,
                regime,
            };
            received.extend(received_rows(&rec, &ctx));
        }
    }

    let mut wb = Workbook::new();
    let holder = s.issuer.name.clone();
    let h = Holder {
        year,
        nif,
        name: &holder,
    };
    sheet(
        &mut wb,
        (
            ISSUED_SHEET,
            "LIBRO REGISTRO FACTURAS EXPEDIDAS Y LIBRO REGISTRO DE VENTAS E INGRESOS",
        ),
        &h,
        &issued_header(),
        &issued,
    )?;
    sheet(
        &mut wb,
        (
            RECEIVED_SHEET,
            "LIBRO REGISTRO FACTURAS RECIBIDAS Y LIBRO REGISTRO DE COMPRAS Y GASTOS",
        ),
        &h,
        &received_header(),
        &received,
    )?;
    let bytes = wb.save_to_buffer().map_err(xlsx)?;
    // Nombre: ejercicio + NIF + tipo de libro (T: unificado completo) + nombre (especificación §2).
    let clean: String = holder
        .chars()
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    Ok((format!("{year}{nif}T{clean}.xlsx"), bytes))
}
