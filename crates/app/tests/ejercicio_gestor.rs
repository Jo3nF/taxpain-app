//! Importar un ejercicio entero del gestor: lectura del «Diario.» de a3 por meses, cuadre,
//! importación en lote, cotejo con el balance de sumas y saldos y con los modelos, cierre con
//! TaxPain, deshacer, diario resumido e inventario de activos. Todo con una empresa inventada.

#![allow(clippy::unwrap_used, clippy::float_arithmetic)]

use std::collections::BTreeMap;
use std::sync::Arc;

use rust_xlsxwriter::Workbook;
use taxpain_app::accounting::{close_year, fiscal_year_info, journal, manual_entry, trial_balance};
use taxpain_app::apertura::{add_asset_with_prior, priors};
use taxpain_app::ejercicio_gestor::{
    Options, compare_balance, compare_models, import, lots, preview, read, read_inventory, undo,
};
use taxpain_app::presentados::save_manual;
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::depreciation::FixedAsset;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
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

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

/// Celda de una hoja de prueba.
#[derive(Clone)]
enum C {
    S(&'static str),
    N(f64),
    E,
}

fn write(ws: &mut rust_xlsxwriter::Worksheet, rows: &[Vec<C>]) {
    for (r, row) in rows.iter().enumerate() {
        for (c, cell) in row.iter().enumerate() {
            let (r, c) = (u32::try_from(r).unwrap(), u16::try_from(c).unwrap());
            match cell {
                C::S(s) => ws.write_string(r, c, *s).unwrap(),
                C::N(n) => ws.write_number(r, c, *n).unwrap(),
                C::E => continue,
            };
        }
    }
}

fn xlsx(sheets: &[(&str, Vec<Vec<C>>)]) -> Vec<u8> {
    let mut wb = Workbook::new();
    for (name, rows) in sheets {
        let ws = wb.add_worksheet();
        ws.set_name(*name).unwrap();
        write(ws, rows);
    }
    wb.save_to_buffer().unwrap()
}

/// Un apunte del diario: (fecha, nº asiento, concepto, documento, cuenta, nombre, debe, haber).
/// Sólo la primera línea de cada asiento lleva fecha y número, como en el listado de a3.
type Apunte = (
    &'static str,
    u32,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    f64,
    f64,
);

/// El diario de 2025 de EMPRESA DEMO SL (inventada): apertura, facturas, nóminas, cobros y
/// pagos, liquidación y pago del IVA, pago del 111, amortización y regularización (12
/// asientos). El cierre va aparte.
fn apuntes() -> Vec<Apunte> {
    vec![
        (
            "01/01/2025",
            1,
            "Asiento de apertura",
            "Apertura",
            "57200001",
            "BANCO DEMO",
            10_000.0,
            0.0,
        ),
        (
            "",
            0,
            "Asiento de apertura",
            "",
            "21700001",
            "EQUIPOS INFORMATICOS",
            3_000.0,
            0.0,
        ),
        (
            "",
            0,
            "Asiento de apertura",
            "",
            "28170001",
            "AMORT. ACUM. EQUIPOS",
            0.0,
            750.0,
        ),
        (
            "",
            0,
            "Asiento de apertura",
            "",
            "10000000",
            "CAPITAL SOCIAL",
            0.0,
            3_000.0,
        ),
        (
            "",
            0,
            "Asiento de apertura",
            "",
            "11300000",
            "RESERVAS VOLUNTARIAS",
            0.0,
            9_250.0,
        ),
        (
            "15/01/2025",
            2,
            "Factura emitida",
            "2025/1",
            "43000001",
            "CLIENTE UNO SL",
            1_210.0,
            0.0,
        ),
        (
            "",
            0,
            "Factura emitida",
            "",
            "70500000",
            "PRESTACIONES DE SERVICIOS",
            0.0,
            1_000.0,
        ),
        (
            "",
            0,
            "Factura emitida",
            "",
            "47700000",
            "HP IVA REPERCUTIDO",
            0.0,
            210.0,
        ),
        (
            "20/01/2025",
            3,
            "Factura recibida",
            "A/438",
            "62900000",
            "OTROS SERVICIOS",
            500.0,
            0.0,
        ),
        (
            "",
            0,
            "Factura recibida",
            "",
            "47200000",
            "HP IVA SOPORTADO",
            105.0,
            0.0,
        ),
        (
            "",
            0,
            "Factura recibida",
            "",
            "41000002",
            "ACREEDOR DOS SL",
            0.0,
            605.0,
        ),
        (
            "31/01/2025",
            4,
            "Nomina enero",
            "",
            "64000000",
            "SUELDOS Y SALARIOS",
            2_000.0,
            0.0,
        ),
        (
            "",
            0,
            "Nomina enero",
            "",
            "64200000",
            "SEG. SOCIAL EMPRESA",
            600.0,
            0.0,
        ),
        (
            "",
            0,
            "Nomina enero",
            "",
            "47510000",
            "HP ACREEDORA RETENCIONES",
            0.0,
            300.0,
        ),
        (
            "",
            0,
            "Nomina enero",
            "",
            "47600000",
            "ORGANISMOS SS ACREEDORES",
            0.0,
            730.0,
        ),
        (
            "",
            0,
            "Nomina enero",
            "",
            "46500000",
            "REMUNERACIONES PENDIENTES",
            0.0,
            1_570.0,
        ),
        (
            "05/02/2025",
            5,
            "Cobro factura 2025/1",
            "",
            "57200001",
            "BANCO DEMO",
            1_210.0,
            0.0,
        ),
        (
            "",
            0,
            "Cobro factura 2025/1",
            "",
            "43000001",
            "CLIENTE UNO SL",
            0.0,
            1_210.0,
        ),
        (
            "10/02/2025",
            6,
            "Pago factura A/438",
            "",
            "41000002",
            "ACREEDOR DOS SL",
            605.0,
            0.0,
        ),
        (
            "",
            0,
            "Pago factura A/438",
            "",
            "57200001",
            "BANCO DEMO",
            0.0,
            605.0,
        ),
        (
            "28/02/2025",
            7,
            "Pago nomina enero",
            "",
            "46500000",
            "REMUNERACIONES PENDIENTES",
            1_570.0,
            0.0,
        ),
        (
            "",
            0,
            "Pago nomina enero",
            "",
            "57200001",
            "BANCO DEMO",
            0.0,
            1_570.0,
        ),
        (
            "31/03/2025",
            8,
            "Liquidacion IVA 1T",
            "",
            "47700000",
            "HP IVA REPERCUTIDO",
            210.0,
            0.0,
        ),
        (
            "",
            0,
            "Liquidacion IVA 1T",
            "",
            "47200000",
            "HP IVA SOPORTADO",
            0.0,
            105.0,
        ),
        (
            "",
            0,
            "Liquidacion IVA 1T",
            "",
            "47500000",
            "HP ACREEDORA IVA",
            0.0,
            105.0,
        ),
        (
            "20/04/2025",
            9,
            "Pago modelo 303 1T",
            "303-1T",
            "47500000",
            "HP ACREEDORA IVA",
            105.0,
            0.0,
        ),
        (
            "",
            0,
            "Pago modelo 303 1T",
            "",
            "57200001",
            "BANCO DEMO",
            0.0,
            105.0,
        ),
        (
            "20/04/2025",
            10,
            "Pago modelo 111 1T",
            "",
            "47510000",
            "HP ACREEDORA RETENCIONES",
            300.0,
            0.0,
        ),
        (
            "",
            0,
            "Pago modelo 111 1T",
            "",
            "57200001",
            "BANCO DEMO",
            0.0,
            300.0,
        ),
        (
            "31/12/2025",
            11,
            "Amortizacion del ejercicio",
            "",
            "68100000",
            "AMORT. INMOV. MATERIAL",
            750.0,
            0.0,
        ),
        (
            "",
            0,
            "Amortizacion del ejercicio",
            "",
            "28170001",
            "AMORT. ACUM. EQUIPOS",
            0.0,
            750.0,
        ),
        (
            "31/12/2025",
            12,
            "Regularizacion",
            "",
            "70500000",
            "PRESTACIONES DE SERVICIOS",
            1_000.0,
            0.0,
        ),
        (
            "",
            0,
            "Regularizacion",
            "",
            "12900000",
            "RESULTADO DEL EJERCICIO",
            2_850.0,
            0.0,
        ),
        (
            "",
            0,
            "Regularizacion",
            "",
            "62900000",
            "OTROS SERVICIOS",
            0.0,
            500.0,
        ),
        (
            "",
            0,
            "Regularizacion",
            "",
            "64000000",
            "SUELDOS Y SALARIOS",
            0.0,
            2_000.0,
        ),
        (
            "",
            0,
            "Regularizacion",
            "",
            "64200000",
            "SEG. SOCIAL EMPRESA",
            0.0,
            600.0,
        ),
        (
            "",
            0,
            "Regularizacion",
            "",
            "68100000",
            "AMORT. INMOV. MATERIAL",
            0.0,
            750.0,
        ),
    ]
}

fn cierre() -> Vec<Apunte> {
    vec![
        (
            "31/12/2025",
            13,
            "Cierre ejercicio",
            "",
            "28170001",
            "AMORT. ACUM. EQUIPOS",
            1_500.0,
            0.0,
        ),
        (
            "",
            0,
            "Cierre ejercicio",
            "",
            "10000000",
            "CAPITAL SOCIAL",
            3_000.0,
            0.0,
        ),
        (
            "",
            0,
            "Cierre ejercicio",
            "",
            "11300000",
            "RESERVAS VOLUNTARIAS",
            9_250.0,
            0.0,
        ),
        (
            "",
            0,
            "Cierre ejercicio",
            "",
            "47600000",
            "ORGANISMOS SS ACREEDORES",
            730.0,
            0.0,
        ),
        (
            "",
            0,
            "Cierre ejercicio",
            "",
            "57200001",
            "BANCO DEMO",
            0.0,
            8_630.0,
        ),
        (
            "",
            0,
            "Cierre ejercicio",
            "",
            "21700001",
            "EQUIPOS INFORMATICOS",
            0.0,
            3_000.0,
        ),
        (
            "",
            0,
            "Cierre ejercicio",
            "",
            "12900000",
            "RESULTADO DEL EJERCICIO",
            0.0,
            2_850.0,
        ),
    ]
}

/// Número de serie de Excel de una fecha `dd/mm/aaaa`.
fn serial(dmy: &str) -> f64 {
    let d = time::Date::parse(
        dmy,
        time::macros::format_description!("[day]/[month]/[year]"),
    )
    .unwrap();
    (d - date!(1899 - 12 - 30)).whole_days() as f64
}

/// Una hoja con el formato «Diario.» de a3: cuatro filas de cabecera, la de columnas, los
/// apuntes y la fila «Total en <mes>». Con `serial_dates` las fechas van como número de Excel.
fn hoja_diario(mes: &'static str, apuntes: &[Apunte], serial_dates: bool) -> Vec<Vec<C>> {
    let mut rows = vec![
        vec![C::S("Diario.")],
        vec![C::S("Empresa: EMPRESA DEMO SL (B12345674)")],
        vec![C::S("Período: de 01/01/2025 a 31/12/2025")],
        vec![C::S("Fecha: 26/10/2026")],
        vec![
            C::S("Fecha"),
            C::S("Asiento"),
            C::S("Apunte"),
            C::S("Concepto"),
            C::S("Documento"),
            C::S("Cuenta"),
            C::S("Descripción de la cuenta"),
            C::S("Importe debe"),
            C::S("Importe haber"),
        ],
    ];
    let (mut td, mut th) = (0.0, 0.0);
    let mut apunte = 0.0;
    for (fecha, n, concepto, doc, cuenta, nombre, debe, haber) in apuntes {
        apunte += 1.0;
        td += debe;
        th += haber;
        rows.push(vec![
            if fecha.is_empty() {
                C::E
            } else if serial_dates {
                C::N(serial(fecha))
            } else {
                C::S(fecha)
            },
            if *n == 0 { C::E } else { C::N(f64::from(*n)) },
            C::N(apunte),
            C::S(concepto),
            C::S(doc),
            C::S(cuenta),
            C::S(nombre),
            if *debe == 0.0 { C::E } else { C::N(*debe) },
            if *haber == 0.0 { C::E } else { C::N(*haber) },
        ]);
    }
    rows.push(vec![
        C::S(mes),
        C::E,
        C::E,
        C::E,
        C::E,
        C::E,
        C::E,
        C::N(td),
        C::N(th),
    ]);
    rows
}

/// El diario por meses (una hoja por mes, como lo saca a3), con o sin el cierre.
fn diario(con_cierre: bool) -> Vec<u8> {
    let a = apuntes();
    let mut dic: Vec<Apunte> = a[29..].to_vec();
    if con_cierre {
        dic.extend(cierre());
    }
    xlsx(&[
        ("Enero", hoja_diario("Total en enero", &a[..16], false)),
        ("Febrero", hoja_diario("Total en febrero", &a[16..22], true)),
        ("Marzo", hoja_diario("Total en marzo", &a[22..25], false)),
        ("Abril", hoja_diario("Total en abril", &a[25..29], false)),
        ("Diciembre", hoja_diario("Total en diciembre", &dic, false)),
    ])
}

fn now() -> time::OffsetDateTime {
    datetime!(2026-10-26 10:00 UTC)
}

fn bal(v: &Vault, year: i32, account: &str) -> Money {
    trial_balance(v, year, None)
        .unwrap()
        .iter()
        .filter(|r| r.account.as_str().starts_with(account))
        .map(|r| r.debit - r.credit)
        .sum()
}

#[test]
fn lectura_y_agrupacion_del_diario_por_meses() {
    let p = read("Diario 2025.xlsx", &diario(false)).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(p.source, "diario");
    assert_eq!(p.entries.len(), 12);
    assert_eq!(p.lines(), 37);
    assert_eq!(p.first_date(), Some(date!(2025 - 01 - 01)));
    assert_eq!(p.last_date(), Some(date!(2025 - 12 - 31)));
    // Apertura y regularización reconocidas; el cierre falta y se avisa.
    assert_eq!(p.opening().map(|e| e.lines.len()), Some(5));
    assert!(p.regularization().is_some());
    assert!(p.closing().is_none());
    assert_eq!(p.current().count(), 10);
    assert!(p.warnings.iter().any(|w| w.contains("cierre")));
    assert_eq!(p.result(), eur(-2_850));
    // Las fechas como número de Excel (hoja de febrero) se leen igual.
    assert!(p.entries.iter().any(|e| e.date == date!(2025 - 02 - 10)));
    // Nombres de cuenta del gestor.
    assert!(
        p.accounts
            .iter()
            .any(|(c, n)| c == "43000001" && n == "CLIENTE UNO SL")
    );
    // Con el cierre: reconocido y sin aviso.
    let p = read("Diario 2025.xlsx", &diario(true)).unwrap();
    assert_eq!(p.entries.len(), 13);
    assert!(p.closing().is_some());
    assert!(
        !p.warnings
            .iter()
            .any(|w| w.contains("no viene el asiento de cierre"))
    );
}

#[test]
fn asiento_descuadrado_se_rechaza_con_la_fila() {
    let mut a = apuntes();
    a[7].7 = 200.0; // la 477 de la factura emitida: 200 en vez de 210
    let x = xlsx(&[("Enero", hoja_diario("Total en enero", &a[..16], false))]);
    let e = read("diario.xlsx", &x)
        .err()
        .map(|e| e.to_string())
        .unwrap();
    assert!(e.contains("no cuadra") && e.contains("fila"), "{e}");
}

#[test]
fn importacion_crea_cuentas_y_asientos_y_coteja_con_el_balance() {
    let v = vault();
    let p = read("Diario 2025.xlsx", &diario(false)).unwrap();
    let pv = preview(&v, &p).unwrap();
    assert!(pv.errors.is_empty(), "{:?}", pv.errors);
    assert_eq!((pv.fiscal_year, pv.entries, pv.lines), (Some(2025), 12, 37));
    assert!(pv.has_opening && pv.has_regularization && !pv.has_closing);
    assert!(
        pv.new_accounts
            .iter()
            .any(|a| a.code == "43000001" && a.name == "CLIENTE UNO SL")
    );
    assert!(
        journal(&v, 2025).unwrap().is_empty(),
        "la vista previa no toca nada"
    );

    let lot = import(&v, 2025, &p, &Options::default(), now()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((lot.entries, lot.lines, lot.fiscal_year), (10, 37, 2025));
    assert!(lot.has_opening && lot.has_regularization && !lot.has_closing && !lot.closed_year);
    let j = journal(&v, 2025).unwrap();
    assert_eq!(j.len(), 12);
    assert_eq!(j[0].entry.origin, Origin::Apertura);
    assert_eq!(j[11].entry.origin, Origin::Regularizacion);
    assert!(
        j[1..11]
            .iter()
            .all(|e| e.entry.origin == Origin::Gestor(lot.id))
    );
    // El concepto conserva el documento del gestor.
    assert!(j[1].entry.concept.contains("2025/1"));
    // Las cuentas nuevas con el nombre del gestor, sin tocar.
    let name: String = v
        .connection()
        .query_row(
            "SELECT name FROM account WHERE code = '41000002'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(name, "ACREEDOR DOS SL");
    assert_eq!(bal(&v, 2025, "572"), eur(8_630));
    assert_eq!(bal(&v, 2025, "129"), eur(2_850));
    assert!(
        !fiscal_year_info(&v, Some(2025), date!(2026 - 10 - 26))
            .unwrap()
            .closed
    );
    // El mismo fichero no se importa dos veces.
    let again = preview(&v, &p).unwrap();
    assert!(again.errors.iter().any(|e| e.contains("ya se importó")));

    // Cotejo con el balance de sumas y saldos del gestor (formato a3, con filas de grupo).
    let c = compare_balance(
        &v,
        2025,
        "Sumas y saldos 2025.xlsx",
        &sumas_y_saldos(8_630.0),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(c.mismatches, 0, "{:?}", c.rows);
    assert_eq!(c.basis, "tras_regularizacion");
    assert!(
        c.rows
            .iter()
            .any(|r| r.account == "57200001" && r.ours == eur(8_630))
    );
    // Con un saldo cambiado en el balance, una diferencia.
    let c = compare_balance(&v, 2025, "sumas.xlsx", &sumas_y_saldos(8_600.0)).unwrap();
    assert_eq!(c.mismatches, 1);
    let r = c.rows.iter().find(|r| r.account == "57200001").unwrap();
    assert_eq!((r.theirs, r.diff), (Some(eur(8_600)), eur(30)));

    // Cotejo con los modelos presentados: 303 del 1T y 200.
    let mut b = BTreeMap::new();
    b.insert(27u16, eur(210));
    b.insert(45u16, eur(105));
    save_manual(&v, "303", 2025, "1T", &b, now()).unwrap();
    let mut b = BTreeMap::new();
    b.insert(500u16, eur(-2_850));
    save_manual(&v, "200", 2025, "0A", &b, now()).unwrap();
    let m = compare_models(&v, 2025).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(m.mismatches, 0, "{:?}", m.rows);
    let r27 = m
        .rows
        .iter()
        .find(|r| r.period == "1T" && r.box_ == 27)
        .unwrap();
    assert_eq!((r27.ours, r27.theirs), (eur(210), Some(eur(210))));
    let r500 = m.rows.iter().find(|r| r.model == "200").unwrap();
    assert_eq!(r500.ours, eur(-2_850));
    // Los trimestres sin IVA ni modelo no salen.
    assert!(!m.rows.iter().any(|r| r.period == "3T"));
    // Sin 130 guardado no se coteja (a una sociedad no le toca).
    assert!(!m.rows.iter().any(|r| r.model == "130"));

    // Cotejo con un 130 inventado del 1T: ingresos y gastos acumulados del diario (grupos 7 y 6
    // hasta el 31 de marzo) frente a [01], [02] y [03].
    let (mut income, mut expenses) = (Money::ZERO, Money::ZERO);
    for e in journal(&v, 2025).unwrap() {
        if e.entry.date > date!(2025 - 03 - 31)
            || matches!(e.entry.origin, Origin::Regularizacion | Origin::Cierre)
        {
            continue;
        }
        for l in &e.entry.lines {
            match l.account.group() {
                7 => income -= l.net(),
                6 => expenses += l.net(),
                _ => {}
            }
        }
    }
    assert!(income.cents() > 0 && expenses.cents() > 0);
    let mut b = BTreeMap::new();
    b.insert(1u16, income);
    b.insert(2u16, expenses + eur(100));
    b.insert(3u16, income - expenses - eur(100));
    save_manual(&v, "130", 2025, "1T", &b, now()).unwrap();
    let m = compare_models(&v, 2025).unwrap_or_else(|e| panic!("{e}"));
    let r130: Vec<_> = m.rows.iter().filter(|r| r.model == "130").collect();
    assert_eq!(r130.len(), 3, "{r130:?}");
    assert!(r130.iter().all(|r| r.period == "1T"));
    let row = |b: u16| r130.iter().find(|r| r.box_ == b).unwrap();
    assert_eq!(
        (row(1).ours, row(1).theirs, row(1).diff),
        (income, Some(income), Money::ZERO)
    );
    assert_eq!((row(2).ours, row(2).diff), (expenses, -eur(100)));
    assert_eq!((row(3).ours, row(3).diff), (income - expenses, eur(100)));
    assert_eq!(m.mismatches, 2);
    assert!(
        m.warnings
            .iter()
            .any(|w| w.contains("difícil justificación")),
        "{:?}",
        m.warnings
    );
}

/// Balance de sumas y saldos de a3 («Sumas y saldos.»), tras la regularización, con filas de
/// grupo y cuenta que no deben contarse dos veces. `banco` es el saldo deudor de la 57200001.
fn sumas_y_saldos(banco: f64) -> Vec<u8> {
    let mut rows = vec![
        vec![C::S("Sumas y saldos.")],
        vec![C::S("Empresa: EMPRESA DEMO SL")],
        vec![C::S("Período: de 01/01/2025 a 31/12/2025")],
        vec![C::E, C::E, C::E, C::S("Período"), C::E, C::S("Acumulados")],
        vec![
            C::S("Cuenta"),
            C::S("Descripción"),
            C::S("Saldo inicial"),
            C::S("Debe"),
            C::S("Haber"),
            C::S("Debe acum."),
            C::S("Haber acum."),
            C::S("Saldo deudor"),
            C::S("Saldo acreedor"),
        ],
    ];
    let lines: Vec<(&str, &str, f64, f64, f64, f64)> = vec![
        ("1", "FINANCIACION BASICA", 2_850.0, 12_250.0, 0.0, 9_400.0),
        ("10000000", "CAPITAL SOCIAL", 0.0, 3_000.0, 0.0, 3_000.0),
        (
            "11300000",
            "RESERVAS VOLUNTARIAS",
            0.0,
            9_250.0,
            0.0,
            9_250.0,
        ),
        (
            "12900000",
            "RESULTADO DEL EJERCICIO",
            2_850.0,
            0.0,
            2_850.0,
            0.0,
        ),
        (
            "21700001",
            "EQUIPOS INFORMATICOS",
            3_000.0,
            0.0,
            3_000.0,
            0.0,
        ),
        (
            "28170001",
            "AMORT. ACUM. EQUIPOS",
            0.0,
            1_500.0,
            0.0,
            1_500.0,
        ),
        ("4", "ACREEDORES Y DEUDORES", 4_105.0, 4_835.0, 0.0, 730.0),
        ("41000002", "ACREEDOR DOS SL", 605.0, 605.0, 0.0, 0.0),
        ("43000001", "CLIENTE UNO SL", 1_210.0, 1_210.0, 0.0, 0.0),
        (
            "46500000",
            "REMUNERACIONES PENDIENTES",
            1_570.0,
            1_570.0,
            0.0,
            0.0,
        ),
        (
            "47",
            "ADMINISTRACIONES PUBLICAS",
            720.0,
            1_450.0,
            0.0,
            730.0,
        ),
        ("47200000", "HP IVA SOPORTADO", 105.0, 105.0, 0.0, 0.0),
        ("47500000", "HP ACREEDORA IVA", 105.0, 105.0, 0.0, 0.0),
        (
            "47510000",
            "HP ACREEDORA RETENCIONES",
            300.0,
            300.0,
            0.0,
            0.0,
        ),
        (
            "47600000",
            "ORGANISMOS SS ACREEDORES",
            0.0,
            730.0,
            0.0,
            730.0,
        ),
        ("47700000", "HP IVA REPERCUTIDO", 210.0, 210.0, 0.0, 0.0),
        ("57200001", "BANCO DEMO", 11_210.0, 2_580.0, banco, 0.0),
        ("62900000", "OTROS SERVICIOS", 500.0, 500.0, 0.0, 0.0),
        ("64000000", "SUELDOS Y SALARIOS", 2_000.0, 2_000.0, 0.0, 0.0),
        ("64200000", "SEG. SOCIAL EMPRESA", 600.0, 600.0, 0.0, 0.0),
        ("68100000", "AMORT. INMOV. MATERIAL", 750.0, 750.0, 0.0, 0.0),
        (
            "70500000",
            "PRESTACIONES DE SERVICIOS",
            1_000.0,
            1_000.0,
            0.0,
            0.0,
        ),
    ];
    for (c, n, d, h, sd, sa) in lines {
        rows.push(vec![
            C::S(c),
            C::S(n),
            C::E,
            C::N(d),
            C::N(h),
            C::N(d),
            C::N(h),
            C::N(sd),
            C::N(sa),
        ]);
    }
    rows.push(vec![
        C::S("Total"),
        C::E,
        C::E,
        C::N(26_780.0),
        C::N(26_780.0),
    ]);
    xlsx(&[("Sumas y saldos", rows)])
}

#[test]
fn cierre_con_taxpain_abre_el_siguiente_con_los_mismos_saldos() {
    let v = vault();
    let p = read("Diario 2025.xlsx", &diario(false)).unwrap();
    import(&v, 2025, &p, &Options::default(), now()).unwrap();
    let s = close_year(&v, 2025, now(), true).unwrap_or_else(|e| panic!("{e}"));
    // La regularización ya venía del gestor: TaxPain sólo cierra y abre.
    assert!(s.regularization_entry.is_none());
    assert!(s.closing_entry.is_some() && s.opening_entry.is_some());
    assert_eq!(s.result, Money::ZERO);
    let j26 = journal(&v, 2026).unwrap();
    assert_eq!(j26.len(), 1);
    let o = &j26[0].entry;
    assert_eq!(
        (o.origin.clone(), o.date),
        (Origin::Apertura, date!(2026 - 01 - 01))
    );
    let net = |code: &str| -> Money {
        o.lines
            .iter()
            .filter(|l| l.account.as_str() == code)
            .map(EntryLine::net)
            .sum()
    };
    assert_eq!(net("57200001"), eur(8_630));
    assert_eq!(net("21700001"), eur(3_000));
    assert_eq!(net("12900000"), eur(2_850));
    assert_eq!(net("28170001"), eur(-1_500));
    assert_eq!(net("10000000"), eur(-3_000));
    assert_eq!(net("11300000"), eur(-9_250));
    assert_eq!(net("47600000"), eur(-730));
    assert_eq!(o.lines.len(), 7);
    assert!(
        fiscal_year_info(&v, Some(2025), date!(2026 - 10 - 26))
            .unwrap()
            .closed
    );
    // Cerrado en TaxPain: el lote ya no se deshace sin reabrir.
    let id = lots(&v).unwrap()[0].id;
    let e = undo(&v, id, now(), true)
        .err()
        .map(|e| e.to_string())
        .unwrap();
    assert!(e.contains("reábrelo"), "{e}");
    // Reabrir quita sólo el cierre y la apertura de TaxPain (la regularización es del gestor) y
    // después el lote se deshace.
    let quitados = taxpain_app::accounting::reopen_year(&v, 2025, now(), true)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(quitados, 2);
    assert!(journal(&v, 2026).unwrap().is_empty());
    undo(&v, id, now(), true).unwrap_or_else(|e| panic!("{e}"));
}

#[test]
fn con_el_cierre_del_gestor_queda_cerrado_y_abre_el_siguiente() {
    let v = vault();
    let p = read("Diario 2025.xlsx", &diario(true)).unwrap();
    let pv = preview(&v, &p).unwrap();
    assert!(pv.has_closing && pv.next_opening, "{pv:?}");
    let lot = import(&v, 2025, &p, &Options::default(), now()).unwrap_or_else(|e| panic!("{e}"));
    assert!(lot.has_closing && lot.closed_year && lot.next_opening);
    assert!(
        fiscal_year_info(&v, Some(2025), date!(2026 - 10 - 26))
            .unwrap()
            .closed
    );
    let j26 = journal(&v, 2026).unwrap();
    assert_eq!(j26.len(), 1);
    assert_eq!(j26[0].entry.origin, Origin::Apertura);
    assert_eq!(bal(&v, 2026, "572"), eur(8_630));
    assert_eq!(bal(&v, 2025, "572"), Money::ZERO, "el cierre salda todo");
    // Deshacer: se va todo, también la apertura de 2026, y 2025 vuelve a estar abierto y vacío.
    let n = undo(&v, lot.id, now(), true).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(n, 14);
    assert!(journal(&v, 2025).unwrap().is_empty());
    assert!(journal(&v, 2026).unwrap().is_empty());
    assert!(
        !fiscal_year_info(&v, Some(2025), date!(2026 - 10 - 26))
            .unwrap()
            .closed
    );
    assert!(lots(&v).unwrap().is_empty());
}

/// El diario de 2026 del gestor, sin asiento de apertura (empieza el 2 de enero).
const DIARIO_2026: &str = "Fecha;Asiento;Cuenta;Concepto;Debe;Haber\n\
                           02/01/2026;1;572;Aportación;3.000,00;\n\
                           02/01/2026;1;100;Aportación;;3.000,00\n";

/// El diario de 2026 del gestor con su propia apertura (la del 2 de enero, como hacen algunos
/// programas) y una aportación.
const DIARIO_2026_CON_APERTURA: &str = "Fecha;Asiento;Cuenta;Concepto;Debe;Haber\n\
                                        02/01/2026;1;57200001;Asiento de apertura;8.630,00;\n\
                                        02/01/2026;1;10000000;Asiento de apertura;;8.630,00\n\
                                        03/01/2026;2;572;Aportación;3.000,00;\n\
                                        03/01/2026;2;100;Aportación;;3.000,00\n";

/// La apertura que sale del cierre sólo se desmarca si el ejercicio siguiente tiene su propio
/// diario (junto al elegido o ya en TaxPain) y ese diario trae su apertura: con las dos, los
/// saldos iniciales irían dos veces. Si el diario del siguiente no la trae, se marca, y después
/// ese diario se carga sobre ella.
#[test]
fn la_apertura_del_siguiente_se_desmarca_si_su_diario_trae_la_suya() {
    use taxpain_app::ejercicio_gestor::{import_file, preview_file_at};
    let no = Options {
        next_opening: false,
    };
    let dir = tempfile::tempdir().unwrap();
    let elegido = dir.path().join("2025/Diario 2025.xlsx");
    std::fs::create_dir_all(elegido.parent().unwrap()).unwrap();
    std::fs::create_dir_all(dir.path().join("2026")).unwrap();
    let bytes = diario(true);
    std::fs::write(&elegido, &bytes).unwrap();
    let siguiente = dir.path().join("2026/Diario 2026.csv");
    // Sin el de 2026 al lado: se propone, como siempre.
    let v = vault();
    let pv = preview_file_at(&v, &elegido, &bytes).unwrap();
    assert!(pv.next_opening && pv.next_ledger.is_none(), "{pv:?}");
    assert!(
        pv.warnings
            .iter()
            .any(|w| w.contains("se creará la apertura de 2026")),
        "{:?}",
        pv.warnings
    );

    // A) El de 2026 en la carpeta de al lado, sin apertura: se propone y, con ella, 2026 se
    // carga después con sus saldos iniciales.
    std::fs::write(&siguiente, DIARIO_2026).unwrap();
    std::fs::write(dir.path().join("2026/Facturas 2026.csv"), "no es un diario").unwrap();
    let pv = preview_file_at(&v, &elegido, &bytes).unwrap();
    assert!(pv.next_opening && pv.next_ledger.is_none(), "{pv:?}");
    assert!(
        pv.warnings
            .iter()
            .any(|w| w.contains("se creará la apertura de 2026")
                && w.contains("Diario 2026.csv")
                && w.contains("no trae apertura")),
        "{:?}",
        pv.warnings
    );
    import(
        &v,
        2025,
        &read("Diario 2025.xlsx", &bytes).unwrap(),
        &Options::default(),
        now(),
    )
    .unwrap();
    let pv26 = preview(
        &v,
        &read("Diario 2026.csv", DIARIO_2026.as_bytes()).unwrap(),
    )
    .unwrap();
    assert!(pv26.errors.is_empty(), "{:?}", pv26.errors);
    assert!(
        pv26.warnings
            .iter()
            .any(|w| w.contains("la apertura que creó el cierre")),
        "{:?}",
        pv26.warnings
    );
    import_file(
        &v,
        2026,
        "Diario 2026.csv",
        DIARIO_2026.as_bytes(),
        &no,
        now(),
    )
    .unwrap();
    assert_eq!(bal(&v, 2026, "572"), eur(11_630));

    // El de 2026 al lado, con su apertura: no se propone; sin ella, se cargan los dos, y con
    // ella el de 2026 ya no (irían dos aperturas).
    std::fs::write(&siguiente, DIARIO_2026_CON_APERTURA).unwrap();
    let v = vault();
    let pv = preview_file_at(&v, &elegido, &bytes).unwrap();
    assert!(
        pv.next_opening
            && pv
                .next_ledger
                .as_deref()
                .is_some_and(|l| l.contains("Diario 2026.csv")),
        "{pv:?}"
    );
    assert!(
        pv.warnings
            .iter()
            .any(|w| w.contains("La apertura de 2026 no se marca")),
        "{:?}",
        pv.warnings
    );
    import(
        &v,
        2025,
        &read("Diario 2025.xlsx", &bytes).unwrap(),
        &no,
        now(),
    )
    .unwrap();
    import_file(
        &v,
        2026,
        "Diario 2026.csv",
        DIARIO_2026_CON_APERTURA.as_bytes(),
        &no,
        now(),
    )
    .unwrap();
    assert_eq!(bal(&v, 2026, "572"), eur(11_630));
    let otro = vault();
    import(
        &otro,
        2025,
        &read("Diario 2025.xlsx", &bytes).unwrap(),
        &Options::default(),
        now(),
    )
    .unwrap();
    let e = import_file(
        &otro,
        2026,
        "Diario 2026.csv",
        DIARIO_2026_CON_APERTURA.as_bytes(),
        &no,
        now(),
    )
    .unwrap_err()
    .to_string();
    assert!(
        e.contains("ya tiene 1 asiento") && e.contains("trae la suya"),
        "{e}"
    );

    // B) 2026 ya cargado en TaxPain sin su apertura: se propone y 2026 queda con sus saldos.
    let v = vault();
    import_file(
        &v,
        2026,
        "Diario 2026.csv",
        DIARIO_2026.as_bytes(),
        &no,
        now(),
    )
    .unwrap();
    let p25 = read("Diario 2025.xlsx", &bytes).unwrap();
    let pv = preview(&v, &p25).unwrap();
    assert!(pv.next_opening && pv.next_ledger.is_none(), "{pv:?}");
    import(&v, 2025, &p25, &Options::default(), now()).unwrap();
    assert_eq!(bal(&v, 2026, "572"), eur(11_630));
    // Ya cargado con su apertura: no se propone.
    let v = vault();
    import_file(
        &v,
        2026,
        "Diario 2026.csv",
        DIARIO_2026_CON_APERTURA.as_bytes(),
        &no,
        now(),
    )
    .unwrap();
    let pv = preview(&v, &p25).unwrap();
    assert!(
        pv.next_opening
            && pv
                .next_ledger
                .as_deref()
                .is_some_and(|l| l.contains("ya cargado en TaxPain")),
        "{pv:?}"
    );
}

/// Un lote de «Importar asientos» de 2026 sólo es el diario de ese ejercicio con su apertura si
/// trae un asiento de apertura: una nómina suelta no lo es, así que la apertura se sigue
/// proponiendo.
#[test]
fn una_nomina_importada_del_siguiente_no_desmarca_su_apertura() {
    let v = vault();
    let nomina = "Fecha;Asiento;Cuenta;Concepto;Debe;Haber\n\
                  31/01/2026;1;640;Nómina de enero;1.500,00;\n\
                  31/01/2026;1;465;Nómina de enero;;1.500,00\n";
    taxpain_app::import::import(&v, "nominas enero 2026.csv", nomina.as_bytes(), now()).unwrap();
    let p25 = read("Diario 2025.xlsx", &diario(true)).unwrap();
    let pv = preview(&v, &p25).unwrap();
    assert!(pv.next_opening && pv.next_ledger.is_none(), "{pv:?}");
    // Un lote con la apertura de 2026 (sólo cuentas de balance, el 1 de enero), sí.
    let apertura = "Fecha;Asiento;Cuenta;Concepto;Debe;Haber\n\
                    01/01/2026;1;572;Saldos iniciales;8.630,00;\n\
                    01/01/2026;1;100;Saldos iniciales;;8.630,00\n";
    taxpain_app::import::import(&v, "saldos 2026.csv", apertura.as_bytes(), now()).unwrap();
    let pv = preview(&v, &p25).unwrap();
    assert!(
        pv.next_ledger
            .as_deref()
            .is_some_and(|l| l.contains("saldos 2026.csv")),
        "{pv:?}"
    );
}

#[test]
fn deshacer_deja_el_ejercicio_vacio() {
    let v = vault();
    let p = read("Diario 2025.xlsx", &diario(false)).unwrap();
    let lot = import(&v, 2025, &p, &Options::default(), now()).unwrap();
    assert!(undo(&v, lot.id, now(), false).is_err(), "sin confirmar no");
    assert_eq!(undo(&v, lot.id, now(), true).unwrap(), 12);
    assert!(journal(&v, 2025).unwrap().is_empty());
    assert!(lots(&v).unwrap().is_empty());
    // Se puede volver a importar.
    let lot = import(&v, 2025, &p, &Options::default(), now()).unwrap();
    assert_eq!(lot.entries, 10);
}

#[test]
fn ejercicio_con_movimientos_previos_se_rechaza() {
    let v = vault();
    manual_entry(
        &v,
        &JournalEntry {
            date: date!(2025 - 03 - 01),
            concept: "Algo de TaxPain".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("572"), eur(10)),
                EntryLine::credit(acc("100"), eur(10)),
            ],
        },
        now(),
    )
    .unwrap();
    let p = read("Diario 2025.xlsx", &diario(false)).unwrap();
    let pv = preview(&v, &p).unwrap();
    assert!(
        pv.errors.iter().any(|e| e.contains("ya tiene 1 asiento")),
        "{:?}",
        pv.errors
    );
    let e = import(&v, 2025, &p, &Options::default(), now())
        .err()
        .map(|e| e.to_string())
        .unwrap();
    assert!(e.contains("ya tiene"), "{e}");
    assert_eq!(journal(&v, 2025).unwrap().len(), 1, "no se importa nada");
    // Y el ejercicio equivocado también.
    let v = vault();
    let e = import(&v, 2024, &p, &Options::default(), now())
        .err()
        .map(|e| e.to_string())
        .unwrap();
    assert!(e.contains("2025") && e.contains("2024"), "{e}");
}

#[test]
fn diario_resumido_mensual() {
    let v = vault();
    let csv = "Diario resumido\n\
               Empresa: EMPRESA DEMO SL\n\
               Período: de 01/01/2025 a 31/12/2025\n\
               Fecha;Importe Debe;Cuenta Debe;Descripción Cuenta;Cuenta Haber;Importe Haber\n\
               31/01/2025;3.000,00;5720;BANCOS;;\n\
               ;;;CAPITAL SOCIAL;1000;3.000,00\n\
               31/01/2025;1.210,00;4300;CLIENTES;;\n\
               ;;;VENTAS;7000;1.000,00\n\
               ;;;HP IVA REPERCUTIDO;4770;210,00\n\
               28/02/2025;1.210,00;5720;BANCOS;;\n\
               ;;;CLIENTES;4300;1.210,00\n\
               Total;;;;;\n";
    let p = read("resumido 2025.csv", csv.as_bytes()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(p.source, "resumido");
    assert_eq!(p.entries.len(), 2, "un asiento por fecha");
    assert_eq!(
        p.entries[0].lines.len(),
        5,
        "las cinco cuentas del mes en un asiento"
    );
    assert!(p.opening().is_none());
    let lot = import(&v, 2025, &p, &Options::default(), now()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!((lot.source.as_str(), lot.entries), ("resumido", 2));
    assert_eq!(bal(&v, 2025, "5720"), eur(4_210));
    assert_eq!(bal(&v, 2025, "4300"), Money::ZERO);
    assert_eq!(bal(&v, 2025, "7000"), eur(-1_000));
}

#[test]
fn inventario_de_activos_con_amortizacion_previa() {
    let rows = vec![
        vec![C::S("Inventario de Activos")],
        vec![C::S("Empresa: EMPRESA DEMO SL")],
        vec![C::S("Fecha: 31/12/2025")],
        vec![C::E],
        vec![
            C::S("Código de Activo"),
            C::S("Descripción"),
            C::S("Ampliación Activo"),
            C::S("Valor Adquisición"),
            C::S("Valor contable"),
            C::S("Fecha Ini Amort."),
        ],
        vec![
            C::S("A0001"),
            C::S("Portátil"),
            C::E,
            C::N(1_200.0),
            C::N(450.0),
            C::S("01/07/2023"),
        ],
        vec![
            C::S("A0002"),
            C::S("Impresora"),
            C::S("ampliación"),
            C::N(300.0),
            C::N(300.0),
            C::S("01/12/2025"),
        ],
        vec![
            C::S("Total Cta. Inmov."),
            C::S("21700001"),
            C::E,
            C::N(1_500.0),
            C::N(750.0),
        ],
        vec![
            C::S("A0003"),
            C::S("Mesa"),
            C::E,
            C::N(500.0),
            C::N(400.0),
            C::S("01/01/2024"),
        ],
        vec![
            C::S("Total Cta. Inmov."),
            C::S("21600001"),
            C::E,
            C::N(500.0),
            C::N(400.0),
        ],
    ];
    let r = read_inventory("Inventario.xlsx", &xlsx(&[("Inventario", rows)]))
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(r.assets.len(), 3);
    let p = &r.assets[0];
    assert_eq!(p.name, "Portátil (A0001)");
    assert_eq!((p.cost, p.accumulated), (eur(1_200), eur(750)));
    assert_eq!(p.acquired_on, date!(2023 - 07 - 01));
    assert_eq!(
        (
            p.account.as_str(),
            p.depreciation_account.as_str(),
            p.expense_account.as_str()
        ),
        ("21700001", "2817", "681")
    );
    assert_eq!(p.rate.bp(), 2500, "coeficiente propuesto por la cuenta");
    assert_eq!(r.assets[1].accumulated, Money::ZERO);
    assert!(r.assets[1].name.contains("ampliación"));
    let m = &r.assets[2];
    assert_eq!(
        (m.account.as_str(), m.rate.bp(), m.accumulated),
        ("21600001", 1000, eur(100))
    );

    // Dado de alta con lo ya amortizado hasta el cierre del gestor: la amortización de TaxPain
    // sigue desde el 1 de enero.
    let v = vault();
    let id = add_asset_with_prior(
        &v,
        &FixedAsset {
            id: 0,
            name: p.name.clone(),
            account: acc(&p.account),
            depreciation_account: acc(&p.depreciation_account),
            expense_account: acc(&p.expense_account),
            acquired_on: p.acquired_on,
            cost: p.cost,
            residual: Money::ZERO,
            rate: p.rate,
            disposed_on: None,
            method: p.method,
            tax_rate: None,
        },
        p.accumulated,
        Some(date!(2025 - 12 - 31)),
        now(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let pr = priors(&v).unwrap();
    assert_eq!(pr.len(), 1);
    assert_eq!(
        (pr[0].id, pr[0].amount, pr[0].until),
        (id, eur(750), Some(date!(2025 - 12 - 31)))
    );
}

/// El inventario del gestor, en Inmovilizado sin pasar por los saldos iniciales: cada bien entra
/// con lo ya amortizado hasta el cierre del ejercicio del listado, sin ningún asiento (ni otra
/// apertura); los terrenos no entran, y lo que ya está no se repite (tampoco si se dio de alta a
/// mano en la 217).
#[test]
fn el_inventario_del_gestor_se_da_de_alta_en_inmovilizado_sin_otra_apertura() {
    use taxpain_app::accounting::{add_fixed_asset, fixed_assets, post_depreciation};
    use taxpain_app::ejercicio_gestor::{import_inventory, preview_inventory};
    let fila = |codigo, nombre, coste: f64, contable: f64, fecha| {
        vec![
            C::S(codigo),
            C::S(nombre),
            C::E,
            C::N(coste),
            C::N(contable),
            C::S(fecha),
        ]
    };
    let total = |cuenta| vec![C::S("Total Cta. Inmov."), C::S(cuenta)];
    let rows = vec![
        vec![C::S("Inventario de Activos del Ejercicio 2025")],
        vec![C::S("Empresa: EMPRESA DEMO SL")],
        vec![C::S("Fecha: 15/01/2026")],
        vec![C::E],
        vec![
            C::S("Código de Activo"),
            C::S("Descripción"),
            C::S("Ampliación Activo"),
            C::S("Valor Adquisición"),
            C::S("Valor contable"),
            C::S("Fecha Ini Amort."),
        ],
        fila("1", "Solar del almacén", 20_000.0, 20_000.0, "10/03/20"),
        total("21000001"),
        fila("2", "Portátil", 1_200.0, 450.0, "01/07/2023"),
        fila("3", "Pantalla", 400.0, 0.0, "01/01/2021"),
        total("21700001"),
        fila("4", "Silla", 300.0, 270.0, "01/01/2025"),
        total("21600001"),
    ];
    let bytes = xlsx(&[("Inventario", rows)]);
    let v = vault();
    // La silla ya se dio de alta a mano (en la 216, con otro nombre).
    add_fixed_asset(
        &v,
        &FixedAsset {
            id: 0,
            name: "Silla de oficina".into(),
            account: acc("216"),
            depreciation_account: acc("2816"),
            expense_account: acc("681"),
            acquired_on: date!(2025 - 01 - 01),
            cost: eur(300),
            residual: Money::ZERO,
            rate: taxpain_core::money::Rate::from_bp(1000),
            disposed_on: None,
            method: Default::default(),
            tax_rate: None,
        },
        now(),
    )
    .unwrap();
    let pv = preview_inventory(&v, "Inventario.xlsx", &bytes).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(pv.until, Some(date!(2025 - 12 - 31)));
    assert_eq!(pv.items.len(), 4);
    assert!(
        pv.items[0]
            .skip
            .as_deref()
            .is_some_and(|s| s.contains("terreno")),
        "{pv:?}"
    );
    assert!(pv.items[1].existing.is_none() && pv.items[1].skip.is_none());
    assert!(pv.items[3].existing.is_some(), "{pv:?}");
    let assets: Vec<_> = pv.items.iter().map(|i| i.asset.clone()).collect();
    let r = import_inventory(&v, &assets, date!(2025 - 12 - 31), now()).unwrap();
    assert_eq!((r.added, r.existing, r.skipped), (2, 1, 1), "{r:?}");
    // Sin asientos: ni apertura ni nada.
    assert!(journal(&v, 2025).unwrap().is_empty());
    assert!(journal(&v, 2026).unwrap().is_empty());
    // Con lo ya amortizado hasta el 31/12/2025.
    let pr = priors(&v).unwrap();
    let mut amortizado: Vec<_> = pr.iter().map(|p| (p.amount, p.until)).collect();
    amortizado.sort();
    assert_eq!(
        amortizado,
        [
            (eur(400), Some(date!(2025 - 12 - 31))),
            (eur(750), Some(date!(2025 - 12 - 31)))
        ]
    );
    // En 2026 sólo se amortiza lo que queda del portátil (la pantalla ya lo está del todo).
    let n = post_depreciation(&v, 2026, now()).unwrap();
    assert_eq!(n.len(), 2, "el portátil y la silla");
    assert_eq!(bal(&v, 2026, "2817"), eur(-300), "un 25 % de 1.200 €");
    // Otra vez: nada nuevo.
    let again = import_inventory(&v, &assets, date!(2025 - 12 - 31), now()).unwrap();
    assert_eq!((again.added, again.existing), (0, 3));
    assert_eq!(fixed_assets(&v).unwrap().len(), 3);
}

/// Fila de un bien del «Inventario de Activos» y fila de total de su cuenta.
fn bien_inv(
    codigo: &'static str,
    nombre: &'static str,
    coste: f64,
    contable: f64,
    fecha: &'static str,
) -> Vec<C> {
    vec![
        C::S(codigo),
        C::S(nombre),
        C::E,
        C::N(coste),
        C::N(contable),
        C::S(fecha),
    ]
}

fn total_inv(cuenta: &'static str) -> Vec<C> {
    vec![C::S("Total Cta. Inmov."), C::S(cuenta)]
}

/// Un «Inventario de Activos» con esta cabecera y estas filas.
fn inventario(cabecera: &'static str, filas: Vec<Vec<C>>) -> Vec<u8> {
    let mut rows = vec![
        vec![C::S(cabecera)],
        vec![C::S("Empresa: EMPRESA DEMO SL")],
        vec![C::S("Fecha: 15/01/2026")],
        vec![C::E],
        vec![
            C::S("Código de Activo"),
            C::S("Descripción"),
            C::S("Ampliación Activo"),
            C::S("Valor Adquisición"),
            C::S("Valor contable"),
            C::S("Fecha Ini Amort."),
        ],
    ];
    rows.extend(filas);
    xlsx(&[("Inventario", rows)])
}

fn alta_a_mano(v: &Vault, nombre: &str, cuenta: &str, fecha: time::Date, coste: i64) {
    taxpain_app::accounting::add_fixed_asset(
        v,
        &FixedAsset {
            id: 0,
            name: nombre.into(),
            account: acc(cuenta),
            depreciation_account: acc("2817"),
            expense_account: acc("681"),
            acquired_on: fecha,
            cost: eur(coste),
            residual: Money::ZERO,
            rate: taxpain_core::money::Rate::from_bp(2500),
            disposed_on: None,
            method: Default::default(),
            tax_rate: None,
        },
        now(),
    )
    .unwrap();
}

/// Dos bienes iguales del listado (dos portátiles con el mismo coste y la misma fecha) son dos
/// bienes: entran los dos. Si uno ya se dio de alta a mano, sólo entra el otro; y una segunda
/// vez, ninguno.
#[test]
fn dos_bienes_iguales_del_inventario_entran_los_dos() {
    use taxpain_app::accounting::fixed_assets;
    use taxpain_app::ejercicio_gestor::{import_inventory, preview_inventory};
    let bytes = inventario(
        "Inventario de Activos del Ejercicio 2025",
        vec![
            bien_inv("1", "Portátil", 1_200.0, 600.0, "01/01/2024"),
            bien_inv("2", "Portátil", 1_200.0, 600.0, "01/01/2024"),
            total_inv("21700001"),
        ],
    );
    let hasta = date!(2025 - 12 - 31);
    let v = vault();
    let pv = preview_inventory(&v, "Inventario.xlsx", &bytes).unwrap();
    assert!(pv.items.iter().all(|i| i.existing.is_none()), "{pv:?}");
    let assets: Vec<_> = pv.items.iter().map(|i| i.asset.clone()).collect();
    let r = import_inventory(&v, &assets, hasta, now()).unwrap();
    assert_eq!((r.added, r.existing), (2, 0), "{r:?}");
    assert_eq!(fixed_assets(&v).unwrap().len(), 2);
    let r = import_inventory(&v, &assets, hasta, now()).unwrap();
    assert_eq!((r.added, r.existing), (0, 2), "{r:?}");

    // Con uno dado de alta a mano, el otro sigue siendo nuevo.
    let v = vault();
    alta_a_mano(
        &v,
        "Portátil de oficina",
        "217",
        date!(2024 - 01 - 01),
        1_200,
    );
    let pv = preview_inventory(&v, "Inventario.xlsx", &bytes).unwrap();
    let ya: Vec<bool> = pv.items.iter().map(|i| i.existing.is_some()).collect();
    assert_eq!(ya, [true, false], "{pv:?}");
    let r = import_inventory(&v, &assets, hasta, now()).unwrap();
    assert_eq!((r.added, r.existing), (1, 1), "{r:?}");
    assert_eq!(fixed_assets(&v).unwrap().len(), 2);
}

/// Con el ejercicio de julio a junio, lo ya amortizado del listado llega al cierre del ejercicio
/// del listado: el final del periodo que dice la cabecera o, con «2024/2025», el del ejercicio que
/// empieza en 2024. Si la cabecera sólo dice un año, se toma el ejercicio que empieza ese año y se
/// avisa de que se compruebe.
#[test]
fn el_inventario_de_un_ejercicio_de_julio_a_junio_llega_a_su_cierre() {
    use taxpain_app::ejercicio_gestor::preview_inventory;
    let v = vault();
    let mut p = taxpain_core::calendar::TaxpayerProfile::sociedad_limitada();
    p.cierre_ejercicio = (time::Month::June, 30);
    v.set_setting("taxpayer_profile", &p).unwrap();
    let filas = || {
        vec![
            bien_inv("1", "Portátil", 1_200.0, 900.0, "01/07/2024"),
            total_inv("21700001"),
        ]
    };
    for cabecera in [
        "Período: de 01/07/2024 a 30/06/2025",
        "Inventario de Activos del Ejercicio 2024/2025",
    ] {
        let pv = preview_inventory(&v, "Inventario.xlsx", &inventario(cabecera, filas())).unwrap();
        assert_eq!(pv.until, Some(date!(2025 - 06 - 30)), "{cabecera}");
    }
    let pv = preview_inventory(
        &v,
        "Inventario.xlsx",
        &inventario("Inventario de Activos del Ejercicio 2025", filas()),
    )
    .unwrap();
    assert_eq!(pv.until, Some(date!(2026 - 06 - 30)));
    assert!(
        pv.warnings
            .iter()
            .any(|w| w.contains("comprueba") && w.contains("Amortizado hasta el")),
        "{:?}",
        pv.warnings
    );
}

/// Un bien a medio amortizar: se propone el coeficiente que da lo ya amortizado (lo amortizado ÷
/// coste ÷ años desde que empezó), que es el plan del gestor, y se avisa si no es el de su cuenta.
/// Lo amortizado del todo o de hace poco sigue con el de su cuenta.
#[test]
fn el_coeficiente_del_inventario_sale_de_lo_ya_amortizado() {
    use taxpain_app::accounting::post_depreciation;
    use taxpain_app::ejercicio_gestor::{import_inventory, preview_inventory};
    let bytes = inventario(
        "Inventario de Activos del Ejercicio 2025",
        vec![
            // Al 10 %: 30,14 € en 2023 (275 días), 40 € en 2024 y 40 € en 2025.
            bien_inv("A12", "Impresora", 400.0, 289.86, "01/04/2023"),
            // Al 25 %, desde el 1 de septiembre: 66,85 €.
            bien_inv("A15", "Pantalla grande", 800.0, 733.15, "01/09/2025"),
            bien_inv("A10", "Servidor", 2_000.0, 0.0, "01/01/2018"),
            total_inv("21700001"),
        ],
    );
    let v = vault();
    let pv = preview_inventory(&v, "Inventario.xlsx", &bytes).unwrap();
    let rates: Vec<i32> = pv.items.iter().map(|i| i.asset.rate.bp()).collect();
    assert_eq!(rates, [1000, 2500, 2500], "{pv:?}");
    assert!(
        pv.warnings
            .iter()
            .any(|w| w.contains("Impresora") && w.contains("10 %") && w.contains("25 %")),
        "{:?}",
        pv.warnings
    );
    assert!(
        !pv.warnings.iter().any(|w| w.contains("Pantalla grande")),
        "{:?}",
        pv.warnings
    );
    let assets: Vec<_> = pv.items.iter().map(|i| i.asset.clone()).collect();
    import_inventory(&v, &assets, date!(2025 - 12 - 31), now()).unwrap();
    post_depreciation(&v, 2026, now()).unwrap();
    assert_eq!(bal(&v, 2026, "2817"), eur(-240), "40 € y 200 €");
}

/// El terreno no se da de alta ni pide coeficiente: el aviso de los bienes sin coeficiente no lo
/// cuenta (tampoco al leer el inventario para el asistente de saldos iniciales).
#[test]
fn el_terreno_del_inventario_no_pide_coeficiente() {
    use taxpain_app::ejercicio_gestor::preview_inventory;
    let bytes = inventario(
        "Inventario de Activos del Ejercicio 2025",
        vec![
            bien_inv("A01", "Solar", 30_000.0, 30_000.0, "10/03/20"),
            total_inv("21000001"),
            bien_inv("A02", "Portátil", 1_200.0, 900.0, "01/01/2025"),
            total_inv("21700001"),
        ],
    );
    let v = vault();
    let pv = preview_inventory(&v, "Inventario.xlsx", &bytes).unwrap();
    assert!(pv.items[0].skip.is_some(), "{pv:?}");
    assert!(
        !pv.warnings.iter().any(|w| w.contains("sin coeficiente")),
        "{:?}",
        pv.warnings
    );
    let r = read_inventory("Inventario.xlsx", &bytes).unwrap();
    assert!(
        !r.warnings.iter().any(|w| w.contains("sin coeficiente")),
        "{:?}",
        r.warnings
    );
}

/// Si el diario del gestor ya está en TaxPain con una subcuenta de amortización acumulada por bien
/// (la 28170003 del 21700003) o con la genérica (28170000), el bien se da de alta con ella, y su
/// gasto con la subcuenta de la 681 del gestor: la amortización de TaxPain sigue en las mismas
/// cuentas que lo ya amortizado.
#[test]
fn el_inventario_usa_las_subcuentas_del_gestor() {
    use taxpain_app::accounting::{post_depreciation, set_account};
    use taxpain_app::ejercicio_gestor::{import_inventory, preview_inventory};
    let v = vault();
    for (code, name) in [
        ("28170003", "A.A. IMPRESORA"),
        ("28170000", "AMORT. ACUM. EQUIPOS"),
        ("68100000", "AMORT. INMOV. MATERIAL"),
    ] {
        set_account(&v, code, name, now()).unwrap();
    }
    let bytes = inventario(
        "Inventario de Activos del Ejercicio 2025",
        vec![
            bien_inv("A12", "Impresora", 400.0, 289.86, "01/04/2023"),
            total_inv("21700003"),
            // Al 10 %: 7,53 € en 2024 (275 días) y 10 € en 2025.
            bien_inv("A16", "Teclado", 100.0, 82.47, "01/04/2024"),
            total_inv("21700009"),
        ],
    );
    let pv = preview_inventory(&v, "Inventario.xlsx", &bytes).unwrap();
    let cuentas: Vec<(&str, &str)> = pv
        .items
        .iter()
        .map(|i| {
            (
                i.asset.depreciation_account.as_str(),
                i.asset.expense_account.as_str(),
            )
        })
        .collect();
    assert_eq!(
        cuentas,
        [("28170003", "68100000"), ("28170000", "68100000")],
        "{pv:?}"
    );
    let assets: Vec<_> = pv.items.iter().map(|i| i.asset.clone()).collect();
    import_inventory(&v, &assets, date!(2025 - 12 - 31), now()).unwrap();
    post_depreciation(&v, 2026, now()).unwrap();
    assert_eq!(bal(&v, 2026, "28170003"), eur(-40));
    assert_eq!(bal(&v, 2026, "28170000"), eur(-10));
    assert_eq!(bal(&v, 2026, "68100000"), eur(50));
}
