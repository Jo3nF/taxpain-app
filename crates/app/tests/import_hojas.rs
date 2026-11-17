//! Hojas de cálculo con varias hojas, como las que exportan los programas de las gestorías: el
//! diario por meses, el diario resumido con la cuenta en dos columnas y los libros registro con
//! sus hojas de adquisiciones, inversión del sujeto pasivo y no deducibles, con las fechas sin
//! año. Datos inventados.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use rust_xlsxwriter::Workbook;
use serde_json::json;
use taxpain_app::accounting::trial_balance;
use taxpain_app::import::{import, preview};
use taxpain_app::lectura_ia;
use taxpain_app::libros::{self, BookKind};
use taxpain_core::money::Money;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

fn vault() -> Vault {
    Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap()
}

fn saldo(v: &Vault, year: i32, cuenta: &str) -> Money {
    trial_balance(v, year, None)
        .unwrap()
        .iter()
        .find(|r| r.account.as_str() == cuenta)
        .map(|r| r.debit - r.credit)
        .unwrap_or(Money::ZERO)
}

/// Libro de Excel con una hoja por cada `(nombre, filas)`; las celdas van como texto, igual que
/// las deja un listado exportado.
fn libro(hojas: &[(&str, Vec<Vec<&str>>)]) -> Vec<u8> {
    let mut wb = Workbook::new();
    for (nombre, filas) in hojas {
        let ws = wb.add_worksheet();
        ws.set_name(*nombre).unwrap();
        for (r, fila) in filas.iter().enumerate() {
            for (c, celda) in fila.iter().enumerate() {
                if !celda.is_empty() {
                    ws.write_string(u32::try_from(r).unwrap(), u16::try_from(c).unwrap(), *celda)
                        .unwrap();
                }
            }
        }
    }
    wb.save_to_buffer().unwrap()
}

/// Cabecera de un listado de a3 o Sage: título, empresa y periodo, y los títulos en la fila 7.
fn cabecera<'a>(titulo: &'a str, periodo: &'a str, titulos: &[&'a str]) -> Vec<Vec<&'a str>> {
    vec![
        vec![titulo],
        vec![],
        vec!["Empresa: EJEMPLO SL"],
        vec![periodo],
        vec!["Fecha: 04/03/2026"],
        vec![],
        titulos.to_vec(),
    ]
}

const DIARIO: [&str; 9] = [
    "Fecha",
    "Asiento",
    "Apunte",
    "Concepto",
    "Documento",
    "Cuenta",
    "Descripción de la cuenta",
    "Importe debe",
    "Importe haber",
];

/// Diario con una hoja por mes (como el «contabilidad» de a3): entran todas, no sólo enero.
#[test]
fn diario_con_una_hoja_por_mes() {
    let periodo = "Período: de 01/01/2025 a 31/12/2025";
    let mut enero = cabecera("Diario.", periodo, &DIARIO);
    enero.extend([
        vec![
            "02/01/2025",
            "1",
            "1",
            "Capital",
            "",
            "100",
            "Capital social",
            "0",
            "3000",
        ],
        vec!["", "", "2", "Capital", "", "5720001", "Banco", "3000", "0"],
        vec![
            "15/01/2025",
            "2",
            "3",
            "Alquiler",
            "F-1",
            "621",
            "Arrendamientos",
            "500",
            "0",
        ],
        vec![
            "", "", "4", "Alquiler", "F-1", "5720001", "Banco", "0", "500",
        ],
    ]);
    let mut febrero = cabecera("Diario.", periodo, &DIARIO);
    febrero.extend([
        vec![
            "10/02/2025",
            "3",
            "5",
            "Teléfono",
            "T-9",
            "629",
            "Otros servicios",
            "60",
            "0",
        ],
        vec![
            "",
            "",
            "6",
            "Teléfono",
            "T-9",
            "5720001",
            "Banco",
            "0",
            "60",
        ],
    ]);
    // Una hoja que no es del diario se salta.
    let resumen = vec![vec!["Resumen del año"], vec!["Total", "3560"]];
    let bytes = libro(&[("Enero", enero), ("Febrero", febrero), ("Resumen", resumen)]);
    let v = vault();
    let p = preview(&v, "2025 - contabilidad.xlsx", &bytes).unwrap();
    assert_eq!(p.entries, 3);
    assert_eq!(p.last_date, Some(date!(2025 - 02 - 10)));
    assert!(
        p.warnings.iter().any(|w| w.contains("Resumen")),
        "{:?}",
        p.warnings
    );
    import(
        &v,
        "2025 - contabilidad.xlsx",
        &bytes,
        datetime!(2026-03-01 9:00 UTC),
    )
    .unwrap();
    assert_eq!(saldo(&v, 2025, "5720001"), Money::from_euros(2440, 0));
    assert_eq!(saldo(&v, 2025, "629"), Money::from_euros(60, 0));
}

/// Diario resumido con la cuenta en dos columnas (debe a la izquierda, haber a la derecha) y
/// una hoja por mes, leído con las columnas que señaló el asistente.
#[test]
fn diario_resumido_con_la_cuenta_en_dos_columnas() {
    let titulos = vec![
        "Fecha",
        "Importe Debe",
        "Cuenta Debe",
        "Descripción Cuenta",
        "Cuenta Haber",
        "Importe Haber",
    ];
    let enero = vec![
        vec!["EMPRESA:", "EJEMPLO SL"],
        titulos.clone(),
        vec!["01/01/2025", "", "", "CAPITAL SOCIAL", "1000", "3000"],
        vec!["01/01/2025", "3000", "5720", "BANCOS", "", ""],
        vec!["31/01/2025", "500", "621", "ARRENDAMIENTOS", "", ""],
        vec!["31/01/2025", "105", "472", "IVA SOPORTADO", "", ""],
        vec!["31/01/2025", "", "", "BANCOS", "5720", "605"],
    ];
    let febrero = vec![
        vec!["EMPRESA:", "EJEMPLO SL"],
        titulos,
        vec!["28/02/2025", "300", "629", "OTROS SERVICIOS", "", ""],
        vec!["28/02/2025", "", "", "BANCOS", "5720", "300"],
    ];
    let bytes = libro(&[("Enero", enero), ("Febrero", febrero)]);
    let lectura = lectura_ia::reading_from_value(&json!({
        "tipo": "diario",
        "descripcion": "Diario resumido de 2025",
        "hoja": {
            "fila_titulos": 2,
            "primera_fila": 3,
            "columnas": {
                "fecha": "A", "debe": "B", "cuenta_debe": "C", "nombre_cuenta": "D",
                "cuenta_haber": "E", "haber": "F"
            }
        }
    }))
    .unwrap();
    let v = vault();
    let msg = lectura_ia::apply(
        &v,
        "DIARIO_2025.xlsx",
        &bytes,
        &lectura,
        None,
        datetime!(2026-03-01 9:00 UTC),
    )
    .unwrap();
    assert!(msg.starts_with("3 asientos"), "{msg}");
    assert_eq!(saldo(&v, 2025, "5720"), Money::from_euros(2095, 0));
    assert_eq!(saldo(&v, 2025, "1000"), -Money::from_euros(3000, 0));
    assert_eq!(saldo(&v, 2025, "472"), Money::from_euros(105, 0));
}

const LIBRO: [&str; 12] = [
    "NºOrden",
    "N.Referencia",
    "Núm.Fact.",
    "Fecha",
    "Concepto",
    "N.I.F.",
    "Expedidor",
    "Base Imponible",
    "%IVA",
    "Cuota",
    "Retención",
    "Total Fra.",
];

/// Libro de recibidas de a3 con sus hojas: las fechas vienen sin año (el del periodo), la hoja
/// de inversión del sujeto pasivo trae el total sin la cuota y la de no deducibles entra con un
/// 0 % deducible.
#[test]
fn libro_de_recibidas_con_sus_hojas() {
    let periodo = "Período De 01 Ene a 31 Dic 2025";
    let mut facturas = cabecera(
        "Operaciones Interiores - Facturas Recibidas",
        periodo,
        &LIBRO,
    );
    facturas.push(vec![
        "1",
        "1",
        "A-1",
        "15/03",
        "Su Fra. A-1",
        "B12345674",
        "PAPELERIA EJEMPLO SL",
        "100",
        "21,0",
        "21",
        "0",
        "121",
    ]);
    let mut isp = cabecera("Inversiones Sujeto Pasivo", periodo, &LIBRO);
    isp.push(vec![
        "1",
        "2",
        "INV-7",
        "20/04",
        "Su Fra. INV-7",
        "",
        "SOFTWARE EJEMPLO INC",
        "50",
        "21,0",
        "10,5",
        "0",
        "50",
    ]);
    let mut no_deducibles = cabecera("Operaciones I.V.A. no Deducible", periodo, &LIBRO);
    no_deducibles.push(vec![
        "1",
        "3",
        "R-9",
        "05/06",
        "Su Fra. R-9",
        "B11111119",
        "RESTAURANTE EJEMPLO SL",
        "200",
        "10,0",
        "20",
        "0",
        "220",
    ]);
    let bytes = libro(&[
        ("Facturas", facturas),
        ("Sujeto pasivo", isp),
        ("No deducibles", no_deducibles),
    ]);
    let b = libros::parse("2025 - Libro registro recibidas.xlsx", &bytes)
        .unwrap()
        .unwrap();
    assert_eq!(b.kind, BookKind::Recibidas);
    assert_eq!(b.invoices.len(), 3, "{:?}", b.warnings);
    let por = |n: &str| b.invoices.iter().find(|i| i.number == n).unwrap();
    assert_eq!(por("A-1").issue_date, date!(2025 - 03 - 15));
    assert_eq!(por("A-1").deductible_pct, 100);
    assert_eq!(por("INV-7").vat_in_total, Some(false));
    assert_eq!(por("R-9").issue_date, date!(2025 - 06 - 05));
    assert_eq!(por("R-9").deductible_pct, 0);

    // Leído con el asistente (columnas por letra), lo mismo.
    let lectura = lectura_ia::reading_from_value(&json!({
        "tipo": "libro_recibidas",
        "descripcion": "Libro de recibidas de 2025",
        "hoja": {
            "fila_titulos": 7,
            "primera_fila": 8,
            "columnas": {
                "numero": "C", "fecha": "D", "nif": "F", "nombre": "G", "base": "H",
                "tipo_iva": "I", "cuota_iva": "J", "retencion": "K", "total": "L"
            }
        }
    }))
    .unwrap();
    let v = vault();
    let msg = lectura_ia::apply(
        &v,
        "2025 - Libro registro recibidas.xlsx",
        &bytes,
        &lectura,
        None,
        datetime!(2026-03-01 9:00 UTC),
    )
    .unwrap();
    assert!(msg.starts_with("3 facturas"), "{msg}");
    let pct: i64 = v
        .connection()
        .query_row(
            "SELECT deductible_pct FROM invoice_received WHERE number = 'R-9'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(pct, 0);
}

/// En el libro de emitidas, las hojas de adquisiciones e inversión del sujeto pasivo son la
/// autorrepercusión de compras: no entran como ventas.
#[test]
fn libro_de_emitidas_sin_la_autorrepercusion() {
    let periodo = "Período De 01 Ene a 31 Dic 2025";
    let titulos = [
        "NºOrden",
        "Núm.Fact.",
        "Fecha",
        "N.I.F.",
        "Destinatario",
        "Base Imponible",
        "%IVA",
        "Cuota",
        "Total Fra.",
    ];
    let mut facturas = cabecera(
        "Operaciones Interiores - Facturas Emitidas",
        periodo,
        &titulos,
    );
    facturas.push(vec![
        "1",
        "2025-0001",
        "10/02",
        "B22222228",
        "CLIENTE EJEMPLO SL",
        "1000",
        "21,0",
        "210",
        "1210",
    ]);
    let mut adquisiciones = cabecera("Adquisiciones Intracomunitarias", periodo, &titulos);
    adquisiciones.push(vec![
        "1",
        "FR-55",
        "18/08",
        "FR32123456789",
        "PROVEEDOR EUROPEO",
        "600",
        "21,0",
        "126",
        "600",
    ]);
    let bytes = libro(&[("Facturas", facturas), ("Adquisiciones", adquisiciones)]);
    let b = libros::parse("2025 - Libro registro emitidas.xlsx", &bytes)
        .unwrap()
        .unwrap();
    assert_eq!(b.kind, BookKind::Emitidas);
    assert_eq!(b.invoices.len(), 1);
    assert_eq!(b.invoices[0].issue_date, date!(2025 - 02 - 10));
    assert!(
        b.warnings.iter().any(|w| w.contains("Adquisiciones")),
        "{:?}",
        b.warnings
    );
}
