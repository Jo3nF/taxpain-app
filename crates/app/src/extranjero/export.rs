//! Hoja de registros del 720 o del 721 en Excel: un resumen por bloque con la decisión y una
//! fila por registro con los datos del formulario. No es el fichero de la AEAT (TaxPain no
//! tiene su diseño de registro): sirve para pasar los datos al formulario de la Sede.

use rust_xlsxwriter::{Format, Workbook};
use taxpain_invoicing::books::Cell;
use taxpain_vault::Vault;
use time::Date;

use super::{Alta, Borrador, borrador};
use crate::books::{write_cell, xlsx};
use crate::error::Result;

fn t(s: impl Into<String>) -> Cell {
    Cell::Text(s.into())
}

fn fecha(d: Option<Date>) -> Cell {
    d.map_or(Cell::Empty, Cell::Date)
}

fn pct(bp: i64) -> String {
    format!("{},{:02} %", bp / 100, bp % 100)
}

fn alta(a: Alta) -> &'static str {
    match a {
        Alta::A => "A",
        Alta::M => "M",
        Alta::C => "C",
    }
}

struct Sheet {
    name: &'static str,
    title: String,
    header: Vec<&'static str>,
    rows: Vec<Vec<Cell>>,
}

fn sheets(b: &Borrador) -> Vec<Sheet> {
    let mut resumen = vec![
        vec![t("Declarante"), t(format!("{} {}", b.nif, b.nombre))],
        vec![t("Fecha de los saldos"), Cell::Date(b.fecha)],
        vec![t("Plazo"), Cell::Date(b.plazo)],
        vec![
            t("¿Hay que presentar?"),
            t(if b.exenta_por_contabilidad {
                "Persona jurídica: no, si los bienes están contabilizados de forma \
                 individualizada"
            } else if b.obligado {
                "Sí"
            } else {
                "No"
            }),
        ],
        vec![t("Fichero"), t(&b.fichero.nota)],
        vec![t("Norma"), t(b.norma)],
        vec![],
    ];
    for bl in &b.bloques {
        resumen.push(vec![t(bl.titulo)]);
        resumen.push(vec![t("Valor a 31/12"), Cell::Amount(bl.total)]);
        if let Some(m) = bl.total_medio {
            resumen.push(vec![t("Saldo medio del 4.º trimestre"), Cell::Amount(m)]);
        }
        if let Some(a) = &bl.anterior {
            resumen.push(vec![
                t(format!("Declarado en {} ({})", a.year, a.origen)),
                Cell::Amount(a.valor),
            ]);
        }
        if let Some(i) = bl.incremento {
            resumen.push(vec![t("Incremento"), Cell::Amount(i)]);
        }
        resumen.push(vec![
            t("Decisión"),
            t(if bl.obligado {
                "Se presenta"
            } else {
                "No se presenta"
            }),
        ]);
        resumen.push(vec![t("Motivo"), t(&bl.motivo)]);
        resumen.push(vec![]);
    }
    for r in &b.requisitos {
        resumen.push(vec![t("Requisito de la exoneración"), t(r)]);
    }
    for a in &b.avisos {
        resumen.push(vec![t("Aviso"), t(a)]);
    }
    for n in &b.notas {
        resumen.push(vec![t("Nota"), t(n)]);
    }
    let registros = b
        .bloques
        .iter()
        .flat_map(|bl| bl.registros.iter())
        .map(|r| {
            vec![
                t(r.clave),
                t(alta(r.alta)),
                t(&r.descripcion),
                t(&r.identificacion),
                t(&r.entidad),
                t(&r.nif_entidad),
                t(&r.bic),
                t(&r.domicilio),
                t(&r.pais),
                t(format!("{} · {}", r.condicion.clave(), r.condicion.label())),
                t(pct(r.porcentaje_bp)),
                fecha(r.fecha_apertura),
                t(if r.fecha_estimada {
                    "primer movimiento"
                } else {
                    ""
                }),
                fecha(r.fecha_extincion),
                t(r.unidades.clone().unwrap_or_default()),
                Cell::Amount(r.saldo),
                r.saldo_medio.map_or(Cell::Empty, Cell::Amount),
                t(&r.moneda),
                t(r.faltan.join(", ")),
            ]
        })
        .collect();
    vec![
        Sheet {
            name: "Resumen",
            title: format!("Modelo {} de {} (borrador)", b.modelo, b.year),
            header: vec!["", ""],
            rows: resumen,
        },
        Sheet {
            name: "Registros",
            title: format!(
                "Registros del {} a {} (claves y orden de los campos por verificar con la orden \
                 del modelo)",
                b.modelo, b.fecha
            ),
            header: vec![
                "Clave",
                "Origen (A/M/C)",
                "Bien",
                "Identificación",
                "Entidad o custodio",
                "NIF de la entidad",
                "BIC",
                "Domicilio",
                "País",
                "Condición",
                "Participación",
                "Apertura o adquisición",
                "Fecha estimada",
                "Extinción",
                "Unidades",
                "Saldo o valor a 31/12",
                "Saldo medio 4.º trimestre",
                "Moneda",
                "Falta",
            ],
            rows: registros,
        },
    ]
}

/// Libro XLSX del borrador. Devuelve (nombre del fichero, bytes).
pub fn workbook(v: &Vault, modelo: &str, year: i32, today: Date) -> Result<(String, Vec<u8>)> {
    let b = borrador(v, modelo, year, today)?;
    let bold = Format::new().set_bold();
    let two_dec = Format::new().set_num_format("#,##0.00");
    let mut wb = Workbook::new();
    for s in sheets(&b) {
        let ws = wb.add_worksheet();
        ws.set_name(s.name).map_err(xlsx)?;
        crate::celda_segura::texto_con_formato(ws, 0, 0, &s.title, &bold).map_err(xlsx)?;
        for (c, h) in s.header.iter().enumerate() {
            let c = u16::try_from(c).unwrap_or(0);
            crate::celda_segura::texto_con_formato(ws, 2, c, *h, &bold).map_err(xlsx)?;
            ws.set_column_width(c, 20.0).map_err(xlsx)?;
        }
        for (i, row) in s.rows.iter().enumerate() {
            let rr = 3 + u32::try_from(i).unwrap_or(u32::MAX - 3);
            for (c, cell) in row.iter().enumerate() {
                write_cell(ws, rr, u16::try_from(c).unwrap_or(0), cell, &two_dec)?;
            }
        }
        ws.set_freeze_panes(3, 0).map_err(xlsx)?;
    }
    let bytes = wb.save_to_buffer().map_err(xlsx)?;
    Ok((format!("Modelo {modelo} {year} - registros.xlsx"), bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcentaje_con_dos_decimales() {
        assert_eq!(pct(10_000), "100,00 %");
        assert_eq!(pct(5_050), "50,50 %");
    }
}
