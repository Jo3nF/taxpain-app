//! Exportación del informe de inversiones en valores: libro XLSX (posiciones, ventas, rentas y
//! retenciones por país, operaciones, 720 y asientos propuestos) y PDF de texto.

use rust_xlsxwriter::{Format, Workbook};
use taxpain_core::money::Money;
use taxpain_invoicing::books::Cell;
use taxpain_invoicing::pdf::{self, Para};
use taxpain_vault::Vault;

use super::{Report, report};
use crate::books::{write_cell, xlsx};
use crate::error::{AppError, Result};

struct Sheet {
    name: &'static str,
    title: String,
    header: Vec<&'static str>,
    rows: Vec<Vec<Cell>>,
}

fn t(s: impl Into<String>) -> Cell {
    Cell::Text(s.into())
}

fn opt(m: Option<Money>) -> Cell {
    m.map_or(Cell::Empty, Cell::Amount)
}

fn sheets(v: &Vault, r: &Report) -> Result<Vec<Sheet>> {
    let valores = super::valores(v)?;
    let cuentas = super::cuentas(v)?;
    let ticker = |id: Option<i64>| {
        id.and_then(|id| valores.iter().find(|x| x.id == id))
            .map(|x| x.ticker.clone())
            .unwrap_or_default()
    };
    let cuenta = |id: i64| {
        cuentas
            .iter()
            .find(|c| c.id == id)
            .map(|c| c.name.clone())
            .unwrap_or_default()
    };
    let posiciones = r
        .posiciones
        .iter()
        .map(|p| {
            vec![
                t(&p.ticker),
                t(&p.isin),
                t(p.kind.label()),
                t(&p.account),
                t(if p.abroad { "sí" } else { "no" }),
                t(p.cantidad.to_string()),
                Cell::Amount(p.coste),
                t(p.precio
                    .map(|x| format!("{x} {}", p.divisa))
                    .unwrap_or_default()),
                opt(p.valor),
                opt(p.latente),
            ]
        })
        .collect();
    let ventas = r
        .realizados
        .iter()
        .map(|x| {
            vec![
                Cell::Date(x.fecha),
                t(if x.derivado {
                    format!("{} (derivado)", x.ticker)
                } else {
                    x.ticker.clone()
                }),
                t(&x.account),
                t(x.cantidad.to_string()),
                Cell::Amount(x.transmision),
                Cell::Amount(x.coste),
                Cell::Amount(x.resultado),
            ]
        })
        .collect();
    let rentas = r
        .doble_imposicion
        .paises
        .iter()
        .map(|p| {
            vec![
                t(&p.pais),
                Cell::Amount(p.rentas),
                Cell::Amount(p.retenido),
                t(p.convenio_pct
                    .map(|c| {
                        format!(
                            "{c} %{}",
                            if p.convenio_comprobado {
                                ""
                            } else {
                                " (a comprobar)"
                            }
                        )
                    })
                    .unwrap_or_default()),
                Cell::Amount(p.cuota_espanola),
                Cell::Amount(p.deducible),
                Cell::Amount(p.exceso),
            ]
        })
        .collect();
    let ops = super::ops(v, None)?
        .into_iter()
        .filter(|o| o.date >= r.inicio && o.date <= r.fecha)
        .map(|o| {
            vec![
                Cell::Date(o.date),
                t(&o.time),
                t(o.kind.label()),
                t(cuenta(o.account_id)),
                t(ticker(o.security_id)),
                t(if o.qty.is_zero() {
                    String::new()
                } else {
                    o.qty.to_string()
                }),
                t(o.price.map(|p| p.to_string()).unwrap_or_default()),
                t(&o.currency),
                Cell::Amount(o.amount),
                Cell::Amount(o.eur),
                Cell::Amount(o.fee_eur),
                t(&o.country),
                t(&o.note),
            ]
        })
        .collect();
    let mut m720 = Vec::new();
    for c in &r.m720.cuentas {
        for (tk, isin, q, val) in &c.valores {
            m720.push(vec![
                t(&c.account),
                t(&c.country),
                t(tk),
                t(isin),
                t(q.to_string()),
                Cell::Amount(*val),
            ]);
        }
    }
    m720.push(vec![
        t("Total en el extranjero"),
        Cell::Empty,
        Cell::Empty,
        Cell::Empty,
        Cell::Empty,
        Cell::Amount(r.m720.total_extranjero),
    ]);
    let mut asientos = Vec::new();
    for a in &r.asientos {
        for l in &a.lineas {
            asientos.push(vec![
                Cell::Date(a.fecha),
                t(&a.concepto),
                t(&l.cuenta),
                Cell::Amount(l.debe),
                Cell::Amount(l.haber),
            ]);
        }
    }
    Ok(vec![
        Sheet {
            name: "Posiciones",
            title: format!("Posiciones a {} ({})", r.fecha, r.metodo.label()),
            header: vec![
                "Valor",
                "ISIN",
                "Clase",
                "Cuenta",
                "Extranjero",
                "Títulos",
                "Coste",
                "Precio de cierre",
                "Valor",
                "Plusvalía latente",
            ],
            rows: posiciones,
        },
        Sheet {
            name: "Ventas",
            title: format!("Resultado de las ventas de {}", r.year),
            header: vec![
                "Fecha",
                "Valor",
                "Cuenta",
                "Títulos",
                "Valor de transmisión",
                "Coste",
                "Resultado",
            ],
            rows: ventas,
        },
        Sheet {
            name: "Retenciones",
            title: format!("Dividendos y retenciones en origen de {} por país", r.year),
            header: vec![
                "País",
                "Dividendos íntegros",
                "Retenido",
                "Convenio",
                "Cuota española",
                "Deducible",
                "Exceso (a reclamar)",
            ],
            rows: rentas,
        },
        Sheet {
            name: "Operaciones",
            title: format!("Operaciones de {}", r.year),
            header: vec![
                "Fecha",
                "Hora",
                "Tipo",
                "Cuenta",
                "Valor",
                "Títulos",
                "Precio",
                "Divisa",
                "Importe",
                "Euros",
                "Comisión (€)",
                "País",
                "Nota",
            ],
            rows: ops,
        },
        Sheet {
            name: "720",
            title: format!("Valores en el extranjero a {} (modelo 720)", r.m720.fecha),
            header: vec![
                "Cuenta",
                "País",
                "Valor",
                "ISIN",
                "Títulos",
                "Valor en euros",
            ],
            rows: m720,
        },
        Sheet {
            name: "Asientos",
            title: format!("Asientos propuestos de {}", r.year),
            header: vec!["Fecha", "Concepto", "Cuenta", "Debe", "Haber"],
            rows: asientos,
        },
    ])
}

/// Libro XLSX del ejercicio. Devuelve (nombre del fichero, bytes).
pub fn workbook(v: &Vault, year: i32) -> Result<(String, Vec<u8>)> {
    let r = report(v, year)?;
    let bold = Format::new().set_bold();
    let two_dec = Format::new().set_num_format("#,##0.00");
    let mut wb = Workbook::new();
    for s in sheets(v, &r)? {
        let ws = wb.add_worksheet();
        ws.set_name(s.name).map_err(xlsx)?;
        crate::celda_segura::texto_con_formato(ws, 0, 0, &s.title, &bold).map_err(xlsx)?;
        for (c, h) in s.header.iter().enumerate() {
            let c = u16::try_from(c).unwrap_or(0);
            crate::celda_segura::texto_con_formato(ws, 2, c, *h, &bold).map_err(xlsx)?;
            ws.set_column_width(c, 18.0).map_err(xlsx)?;
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
    Ok((format!("Inversiones en valores {year}.xlsx"), bytes))
}

/// PDF de texto con el informe del ejercicio.
pub fn pdf_bytes(v: &Vault, year: i32) -> Result<(String, Vec<u8>)> {
    let r = report(v, year)?;
    let res = &r.resultado;
    let mut p = vec![
        Para::Title(format!("Inversiones en valores {year}")),
        Para::Text(format!(
            "Del {} al {}. Ventas con {}.",
            r.inicio,
            r.fecha,
            r.metodo.label()
        )),
        Para::Gap,
        Para::Bold(format!("Posiciones a {}", r.fecha)),
    ];
    for x in &r.posiciones {
        p.push(Para::Row(
            format!(
                "{} {} en {}{} · coste {}",
                x.cantidad,
                x.ticker,
                x.account,
                if x.abroad { " (extranjero)" } else { "" },
                x.coste
            ),
            x.valor
                .map(|v| format!("valor {v}"))
                .unwrap_or_else(|| "sin precio".into()),
        ));
    }
    p.push(Para::Row(
        "Coste total / valor total".into(),
        format!("{} / {}", r.coste_total, r.valor_total),
    ));
    p.push(Para::Gap);
    p.push(Para::Bold(format!("Resultado de {year}")));
    for (k, m) in [
        ("Ganancias en ventas", res.ganancias),
        ("Pérdidas en ventas", res.perdidas),
        ("Neto de las ventas", res.neto),
        ("Derivados (opciones)", res.derivados),
        ("Dividendos íntegros", res.dividendos),
        ("Retenciones en origen", res.retenciones_extranjero),
        ("Retenciones españolas", res.retenciones_espana),
        ("Intereses", res.intereses),
        ("Comisiones a resultados", res.comisiones),
    ] {
        p.push(Para::Row(k.into(), m.to_string()));
    }
    for x in &r.realizados {
        p.push(Para::Row(
            format!(
                "{} {} de {} {} ({}): {} − coste {}",
                x.fecha,
                if x.derivado { "derivado" } else { "venta" },
                x.cantidad,
                x.ticker,
                x.account,
                x.transmision,
                x.coste
            ),
            x.resultado.to_string(),
        ));
    }
    p.push(Para::Gap);
    p.push(Para::Bold("Doble imposición internacional".into()));
    for x in &r.doble_imposicion.paises {
        p.push(Para::Row(
            format!(
                "{}: dividendos {} · retenido {}",
                x.pais, x.rentas, x.retenido
            ),
            format!("deducible {}", x.deducible),
        ));
    }
    p.push(Para::Text(r.doble_imposicion.nota.clone()));
    p.push(Para::Gap);
    p.push(Para::Bold("Modelo 720".into()));
    p.push(Para::Text(r.m720.nota.clone()));
    if !r.avisos.is_empty() {
        p.push(Para::Gap);
        p.push(Para::Bold("Avisos".into()));
        for a in &r.avisos {
            p.push(Para::Text(a.clone()));
        }
    }
    p.push(Para::Gap);
    p.push(Para::Bold("Criterios".into()));
    for n in &r.notas {
        p.push(Para::Text(n.clone()));
    }
    let bytes = pdf::text_document(&format!("Inversiones en valores {year}"), &[p])
        .map_err(AppError::Other)?;
    Ok((format!("Inversiones en valores {year}.pdf"), bytes))
}
