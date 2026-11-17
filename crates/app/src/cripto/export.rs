//! Exportación del informe de criptomonedas: libro XLSX (posiciones, operaciones, resultados,
//! 721) y PDF de texto.

use rust_xlsxwriter::{Format, Workbook};
use taxpain_core::cripto::Tipo;
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

fn opt(m: Option<taxpain_core::money::Money>) -> Cell {
    m.map_or(Cell::Empty, Cell::Amount)
}

fn sheets(v: &Vault, r: &Report) -> Result<Vec<Sheet>> {
    let plataformas = super::platforms(v)?;
    let nombre = |id: i64| {
        plataformas
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
            .unwrap_or_default()
    };
    let posiciones = r
        .posiciones
        .iter()
        .map(|p| {
            vec![
                t(&p.ticker),
                t(&p.platform),
                t(if p.abroad { "sí" } else { "no" }),
                t(p.cantidad.to_string()),
                Cell::Amount(p.coste),
                t(p.precio.map(|x| x.to_string()).unwrap_or_default()),
                opt(p.valor),
                opt(p.latente),
            ]
        })
        .collect();
    let ops = super::ops(v, Some(r.year))?
        .iter()
        .map(|o| {
            vec![
                Cell::Date(o.date),
                t(&o.time),
                t(o.kind.label()),
                t(nombre(o.platform_id)),
                t(o.to_platform_id.map(nombre).unwrap_or_default()),
                t(&o.ticker),
                t(o.qty.to_string()),
                t(o.ticker2.clone().unwrap_or_default()),
                t(o.qty2.map(|q| q.to_string()).unwrap_or_default()),
                Cell::Amount(o.eur),
                Cell::Amount(o.fee_eur),
                t(&o.note),
                t(if o.entry_id.is_some() { "sí" } else { "no" }),
            ]
        })
        .collect();
    let realizados = r
        .realizados
        .iter()
        .map(|x| {
            vec![
                Cell::Date(x.fecha),
                t(x.tipo.label()),
                t(&x.ticker),
                t(&x.platform),
                t(x.cantidad.to_string()),
                Cell::Amount(x.valor),
                Cell::Amount(x.coste),
                Cell::Amount(x.resultado),
            ]
        })
        .collect();
    let mut m721 = Vec::new();
    for p in &r.m721.plataformas {
        for (ticker, q, valor) in &p.activos {
            m721.push(vec![
                t(&p.platform),
                t(&p.country),
                t(ticker),
                t(q.to_string()),
                Cell::Amount(*valor),
            ]);
        }
    }
    m721.push(vec![
        t("Total en el extranjero"),
        Cell::Empty,
        Cell::Empty,
        Cell::Empty,
        Cell::Amount(r.m721.total_extranjero),
    ]);
    let resumen = vec![
        vec![
            t("Método"),
            t(r.resumen_pmp.metodo.label()),
            t(r.resumen_fifo.metodo.label()),
        ],
        vec![
            t("Ganancias"),
            Cell::Amount(r.resumen_pmp.ganancias),
            Cell::Amount(r.resumen_fifo.ganancias),
        ],
        vec![
            t("Pérdidas"),
            Cell::Amount(r.resumen_pmp.perdidas),
            Cell::Amount(r.resumen_fifo.perdidas),
        ],
        vec![
            t("Neto"),
            Cell::Amount(r.resumen_pmp.neto),
            Cell::Amount(r.resumen_fifo.neto),
        ],
        vec![
            t("Recompensas"),
            Cell::Amount(r.resumen_pmp.recompensas),
            Cell::Amount(r.resumen_fifo.recompensas),
        ],
        vec![
            t("Comisiones (gasto)"),
            Cell::Amount(r.resumen_pmp.comisiones),
            Cell::Amount(r.resumen_fifo.comisiones),
        ],
        vec![
            t("Se declara con"),
            t(r.metodo_declaracion.label()),
            Cell::Empty,
        ],
    ];
    let mut resumen = resumen;
    if let Some(i) = &r.inicio {
        resumen.push(vec![
            t(format!(
                "Punto de partida a {}",
                super::inicio::dmy(i.fecha)
            )),
            Cell::Amount(i.coste_total()),
            t(&i.origen),
        ]);
        if r.antes_del_inicio {
            resumen.push(vec![
                t(format!("Resultados de {}", r.year)),
                t("calculados por la gestoría"),
                Cell::Empty,
            ]);
        }
    }
    Ok(vec![
        Sheet {
            name: "Posiciones",
            title: format!("Posiciones a {} ({})", r.fecha, r.settings.metodo.label()),
            header: vec![
                "Activo",
                "Plataforma",
                "Extranjero",
                "Cantidad",
                "Coste",
                "Precio de cierre",
                "Valor",
                "Plusvalía latente",
            ],
            rows: posiciones,
        },
        Sheet {
            name: "Operaciones",
            title: format!("Operaciones de {}", r.year),
            header: vec![
                "Fecha",
                "Hora",
                "Tipo",
                "Plataforma",
                "Destino",
                "Activo",
                "Cantidad",
                "Recibido",
                "Cantidad recibida",
                "Euros",
                "Comisión",
                "Nota",
                "Asiento",
            ],
            rows: ops,
        },
        Sheet {
            name: "Resultados",
            title: format!(
                "Resultados realizados de {} ({})",
                r.year,
                r.metodo_declaracion.label()
            ),
            header: vec![
                "Fecha",
                "Tipo",
                "Activo",
                "Plataforma",
                "Cantidad",
                "Valor de transmisión",
                "Coste",
                "Resultado",
            ],
            rows: realizados,
        },
        Sheet {
            name: "Resumen",
            title: format!("Resumen de {} por los dos métodos", r.year),
            header: vec!["", "PMP", "FIFO"],
            rows: resumen,
        },
        Sheet {
            name: "721",
            title: format!("Saldos en el extranjero a {} (modelo 721)", r.m721.fecha),
            header: vec!["Plataforma", "País", "Activo", "Cantidad", "Valor en euros"],
            rows: m721,
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
    Ok((format!("Criptomonedas {year}.xlsx"), bytes))
}

/// PDF de texto con el informe del ejercicio.
pub fn pdf_bytes(v: &Vault, year: i32) -> Result<(String, Vec<u8>)> {
    let r = report(v, year)?;
    let mut p = vec![
        Para::Title(format!("Criptomonedas {year}")),
        Para::Text(format!(
            "Método: {} (se declara con {}). Naturaleza contable: {}. Posiciones a {}.",
            r.settings.metodo.label(),
            r.metodo_declaracion.label(),
            r.settings.naturaleza.label(),
            r.fecha
        )),
        Para::Gap,
        Para::Bold("Posiciones".into()),
    ];
    if let Some(i) = &r.inicio {
        p.insert(
            2,
            Para::Text(format!(
                "Punto de partida: {} con un coste pendiente de {}{}. {}",
                super::inicio::dmy(i.fecha),
                i.coste_total(),
                if i.origen.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", i.origen)
                },
                if r.antes_del_inicio {
                    format!("Los resultados de {year} los calculó la gestoría.")
                } else {
                    "Las operaciones anteriores son historial.".into()
                }
            )),
        );
    }
    for x in &r.posiciones {
        p.push(Para::Row(
            format!(
                "{} {} en {}{} · coste {}",
                x.cantidad,
                x.ticker,
                x.platform,
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
    p.push(Para::Bold(format!("Resultados realizados de {year}")));
    for x in &r.realizados {
        p.push(Para::Row(
            format!(
                "{} {} {} {} ({}): valor {} − coste {}",
                x.fecha,
                x.tipo.label(),
                x.cantidad,
                x.ticker,
                x.platform,
                x.valor,
                x.coste
            ),
            x.resultado.to_string(),
        ));
    }
    p.push(Para::Row(
        "Neto PMP / FIFO".into(),
        format!("{} / {}", r.resumen_pmp.neto, r.resumen_fifo.neto),
    ));
    p.push(Para::Row(
        "Recompensas / comisiones".into(),
        format!(
            "{} / {}",
            r.resumen_fifo.recompensas, r.resumen_fifo.comisiones
        ),
    ));
    p.push(Para::Gap);
    p.push(Para::Bold("Modelo 721".into()));
    p.push(Para::Text(r.m721.nota.clone()));
    if !r.avisos.is_empty() {
        p.push(Para::Gap);
        p.push(Para::Bold("Avisos".into()));
        for a in &r.avisos {
            p.push(Para::Text(a.clone()));
        }
    }
    p.push(Para::Gap);
    p.push(Para::Text(
        "Tipos de operación: ".to_owned()
            + &Tipo::TODOS
                .iter()
                .map(|t| t.label())
                .collect::<Vec<_>>()
                .join(", ")
            + ".",
    ));
    let bytes =
        pdf::text_document(&format!("Criptomonedas {year}"), &[p]).map_err(AppError::Other)?;
    Ok((format!("Criptomonedas {year}.pdf"), bytes))
}
