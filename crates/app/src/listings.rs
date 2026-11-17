//! Listados de la contabilidad en una hoja de cálculo (XLSX): diario, mayor, sumas y saldos,
//! balance, pérdidas y ganancias y cartera de cobros y pagos del ejercicio, una hoja cada uno.

use rust_xlsxwriter::{Format, Workbook};
use taxpain_core::accounting::statements::ModelKind;
use taxpain_core::money::Money;
use taxpain_invoicing::books::Cell;
use taxpain_vault::Vault;
use time::Date;

use crate::books::{write_cell, xlsx};
use crate::error::Result;
use crate::{accounting, cartera, invoicing};

struct Sheet {
    name: &'static str,
    title: String,
    header: Vec<&'static str>,
    widths: Vec<f64>,
    rows: Vec<Vec<Cell>>,
}

fn amount(m: Money) -> Cell {
    Cell::Amount(m)
}

fn text(s: impl Into<String>) -> Cell {
    Cell::Text(s.into())
}

/// Libro de contabilidad del ejercicio `year` en XLSX. Devuelve (nombre del fichero, bytes).
pub fn workbook(v: &Vault, year: i32, model: ModelKind, today: Date) -> Result<(String, Vec<u8>)> {
    let conn = v.connection();
    let fy = accounting::fiscal_year(v, year)?;
    let (nif, name) = match invoicing::settings(v)? {
        Some(s) => (
            match &s.issuer.id {
                taxpain_core::invoice::PartyId::Nif(n) => n.clone(),
                taxpain_core::invoice::PartyId::Foreign { id, .. } => id.clone(),
            },
            s.issuer.name,
        ),
        None => (String::new(), String::new()),
    };
    let names: std::collections::BTreeMap<String, String> = accounting::chart(v, year)?
        .into_iter()
        .map(|a| (a.code, a.name))
        .collect();
    let name_of = |code: &str| -> String {
        names
            .get(code)
            .cloned()
            .or_else(|| accounting::account_name(conn, code).ok().flatten())
            .unwrap_or_default()
    };

    // Diario.
    let journal = accounting::journal(v, year)?;
    let mut diario = Vec::new();
    for e in &journal {
        for l in &e.entry.lines {
            diario.push(vec![
                Cell::Int(e.number),
                Cell::Date(e.entry.date),
                text(l.account.as_str()),
                text(name_of(l.account.as_str())),
                text(
                    l.description
                        .clone()
                        .unwrap_or_else(|| e.entry.concept.clone()),
                ),
                amount(l.debit),
                amount(l.credit),
            ]);
        }
    }

    // Mayor: todas las cuentas con movimiento, con saldo acumulado.
    let mut lines: Vec<(&str, i64, Date, &str, Money, Money)> = journal
        .iter()
        .flat_map(|e| {
            e.entry.lines.iter().map(move |l| {
                (
                    l.account.as_str(),
                    e.number,
                    e.entry.date,
                    e.entry.concept.as_str(),
                    l.debit,
                    l.credit,
                )
            })
        })
        .collect();
    lines.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
    let mut mayor = Vec::new();
    let mut current = "";
    let mut balance = Money::ZERO;
    for (acc, number, date, concept, debit, credit) in lines {
        if acc != current {
            current = acc;
            balance = Money::ZERO;
        }
        balance += debit - credit;
        mayor.push(vec![
            text(acc),
            text(name_of(acc)),
            Cell::Int(number),
            Cell::Date(date),
            text(concept),
            amount(debit),
            amount(credit),
            amount(balance),
        ]);
    }

    // Sumas y saldos (subcuentas).
    let sumas: Vec<Vec<Cell>> = accounting::trial_balance(v, year, None)?
        .into_iter()
        .map(|b| {
            let bal = b.balance();
            vec![
                text(b.account.as_str()),
                text(name_of(b.account.as_str())),
                amount(b.debit),
                amount(b.credit),
                amount(bal.max(Money::ZERO)),
                amount((-bal).max(Money::ZERO)),
            ]
        })
        .collect();

    // Balance y PyG del modelo elegido.
    let ca = accounting::annual_accounts_model(v, year, model)?;
    let statement = |s: &taxpain_core::accounting::statements::Statement| -> Vec<Vec<Cell>> {
        s.lines
            .iter()
            .map(|l| {
                vec![
                    text(l.code.clone().unwrap_or_default()),
                    text(if l.leaf {
                        format!("    {}", l.label)
                    } else {
                        l.label.clone()
                    }),
                    amount(l.amount),
                ]
            })
            .collect()
    };

    // Cartera: los vencimientos de las facturas del ejercicio y, como en Cobros y pagos, los de
    // antes (p. ej. los pendientes de la apertura) que seguían pendientes al empezarlo.
    let cartera_rows: Vec<Vec<Cell>> = cartera::list(v, today)?
        .into_iter()
        .filter(|d| {
            fy.contains(d.issue_date)
                || (d.issue_date < fy.start && d.settled_on.is_none_or(|s| s >= fy.start))
        })
        .map(|d| {
            vec![
                text(if d.kind == "emitida" { "Cobro" } else { "Pago" }),
                text(d.number),
                text(d.party),
                Cell::Date(d.issue_date),
                Cell::Date(d.due_date),
                amount(d.amount),
                text(match d.status.as_str() {
                    "pendiente" => "Pendiente".to_owned(),
                    _ => d
                        .settled_on
                        .map(|s| format!("{:02}/{:02}/{}", s.day(), u8::from(s.month()), s.year()))
                        .unwrap_or_else(|| "Cobrado/pagado".into()),
                }),
            ]
        })
        .collect();

    let period = format!(
        "Ejercicio {} ({:02}/{:02}/{} a {:02}/{:02}/{})",
        fy.label(),
        fy.start.day(),
        u8::from(fy.start.month()),
        fy.start.year(),
        fy.end.day(),
        u8::from(fy.end.month()),
        fy.end.year()
    );
    let sheets = vec![
        Sheet {
            name: "Diario",
            title: "Libro diario".into(),
            header: vec![
                "Asiento", "Fecha", "Cuenta", "Nombre", "Concepto", "Debe", "Haber",
            ],
            widths: vec![9.0, 11.0, 12.0, 32.0, 44.0, 13.0, 13.0],
            rows: diario,
        },
        Sheet {
            name: "Mayor",
            title: "Libro mayor".into(),
            header: vec![
                "Cuenta", "Nombre", "Asiento", "Fecha", "Concepto", "Debe", "Haber", "Saldo",
            ],
            widths: vec![12.0, 32.0, 9.0, 11.0, 44.0, 13.0, 13.0, 13.0],
            rows: mayor,
        },
        Sheet {
            name: "Sumas y saldos",
            title: "Balance de sumas y saldos".into(),
            header: vec![
                "Cuenta",
                "Nombre",
                "Sumas debe",
                "Sumas haber",
                "Saldo deudor",
                "Saldo acreedor",
            ],
            widths: vec![12.0, 40.0, 14.0, 14.0, 14.0, 14.0],
            rows: sumas,
        },
        Sheet {
            name: "Balance",
            title: "Balance".into(),
            header: vec!["Código", "Partida", "Importe"],
            widths: vec![10.0, 70.0, 15.0],
            rows: statement(&ca.balance),
        },
        Sheet {
            name: "PyG",
            title: "Cuenta de pérdidas y ganancias".into(),
            header: vec!["Código", "Partida", "Importe"],
            widths: vec![10.0, 70.0, 15.0],
            rows: statement(&ca.income),
        },
        Sheet {
            name: "Cartera",
            title: "Cobros y pagos".into(),
            header: vec![
                "Tipo",
                "Factura",
                "Tercero",
                "Fecha",
                "Vencimiento",
                "Importe",
                "Estado",
            ],
            widths: vec![8.0, 16.0, 36.0, 11.0, 12.0, 13.0, 16.0],
            rows: cartera_rows,
        },
    ];

    let bold = Format::new().set_bold();
    let two_dec = Format::new().set_num_format("#,##0.00");
    let mut wb = Workbook::new();
    for s in &sheets {
        let ws = wb.add_worksheet();
        ws.set_name(s.name).map_err(xlsx)?;
        crate::celda_segura::texto_con_formato(ws, 0, 0, &s.title, &bold).map_err(xlsx)?;
        crate::celda_segura::texto(ws, 1, 0, format!("{name} · NIF {nif} · {period}"))
            .map_err(xlsx)?;
        for (c, h) in s.header.iter().enumerate() {
            let c = u16::try_from(c).unwrap_or(0);
            crate::celda_segura::texto_con_formato(ws, 3, c, *h, &bold).map_err(xlsx)?;
        }
        for (c, w) in s.widths.iter().enumerate() {
            ws.set_column_width(u16::try_from(c).unwrap_or(0), *w)
                .map_err(xlsx)?;
        }
        for (i, row) in s.rows.iter().enumerate() {
            let r = 4 + u32::try_from(i).unwrap_or(u32::MAX - 4);
            for (c, cell) in row.iter().enumerate() {
                write_cell(ws, r, u16::try_from(c).unwrap_or(0), cell, &two_dec)?;
            }
        }
        ws.set_freeze_panes(4, 0).map_err(xlsx)?;
    }
    let bytes = wb.save_to_buffer().map_err(xlsx)?;
    Ok((
        format!("Contabilidad {} {nif}.xlsx", fy.label().replace('/', "-")),
        bytes,
    ))
}
