//! Importar asientos: Excel, CSV y SUENLACE de a3, con vista previa, lote y deshacer.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use rust_xlsxwriter::Workbook;
use taxpain_app::a3::{A3Options, suenlace};
use taxpain_app::accounting::{close_year, journal, manual_entry, trial_balance};
use taxpain_app::import::{batches, import, preview, undo};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::suenlace::Format;
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

fn bal(v: &Vault, year: i32, account: &str) -> Money {
    trial_balance(v, year, None)
        .unwrap()
        .iter()
        .find(|r| r.account.as_str() == account)
        .map(|r| r.debit - r.credit)
        .unwrap_or(Money::ZERO)
}

fn excel() -> Vec<u8> {
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    // Un título encima de la tabla, como en muchos listados.
    ws.write_string(0, 0, "Diario de Ejemplo SL").unwrap();
    for (c, h) in [
        "Fecha",
        "Nº asiento",
        "Concepto",
        "Cuenta",
        "Descripción cuenta",
        "Debe",
        "Haber",
    ]
    .iter()
    .enumerate()
    {
        ws.write_string(2, u16::try_from(c).unwrap(), *h).unwrap();
    }
    let rows: [(&str, f64, &str, &str, &str, f64, f64); 4] = [
        (
            "02/01/2026",
            1.0,
            "Capital",
            "5720001",
            "Banco Ejemplo",
            3000.0,
            0.0,
        ),
        (
            "02/01/2026",
            1.0,
            "Capital",
            "100",
            "Capital social",
            0.0,
            3000.0,
        ),
        (
            "15/01/2026",
            2.0,
            "Alquiler & gastos",
            "621",
            "Arrendamientos",
            500.5,
            0.0,
        ),
        (
            "15/01/2026",
            2.0,
            "Alquiler & gastos",
            "5720001",
            "",
            0.0,
            500.5,
        ),
    ];
    for (i, (d, n, c, a, name, de, ha)) in rows.iter().enumerate() {
        let r = u32::try_from(i).unwrap() + 3;
        ws.write_string(r, 0, *d).unwrap();
        ws.write_number(r, 1, *n).unwrap();
        ws.write_string(r, 2, *c).unwrap();
        ws.write_string(r, 3, *a).unwrap();
        ws.write_string(r, 4, *name).unwrap();
        ws.write_number(r, 5, *de).unwrap();
        ws.write_number(r, 6, *ha).unwrap();
    }
    wb.save_to_buffer().unwrap()
}

#[test]
fn excel_vista_previa_lote_y_deshacer() {
    let v = vault();
    let now = datetime!(2026-10-20 10:00 UTC);
    let x = excel();
    let p = preview(&v, "diario.xlsx", &x).unwrap();
    assert_eq!((p.source.as_str(), p.entries, p.accounts), ("tabla", 2, 3));
    assert_eq!(p.first_date, Some(date!(2026 - 01 - 02)));
    assert_eq!(p.total, Money::from_cents(350_050));
    assert_eq!(p.sample[1].concept, "Alquiler & gastos");
    assert!(p.already.is_none());
    assert!(
        journal(&v, 2026).unwrap().is_empty(),
        "la vista previa no toca nada"
    );

    let (batch, _) = import(&v, "diario.xlsx", &x, now).unwrap();
    assert_eq!(bal(&v, 2026, "5720001"), Money::from_cents(249_950));
    assert!(
        journal(&v, 2026)
            .unwrap()
            .iter()
            .all(|e| e.entry.origin == Origin::Importado(batch))
    );
    // El mismo fichero, dos veces no.
    assert!(preview(&v, "otro.xlsx", &x).unwrap().already.is_some());
    assert!(import(&v, "otro.xlsx", &x, now).is_err());
    assert_eq!(batches(&v).unwrap().len(), 1);

    assert!(undo(&v, batch, now, false).is_err());
    assert_eq!(undo(&v, batch, now, true).unwrap(), 2);
    assert!(journal(&v, 2026).unwrap().is_empty());
    assert!(batches(&v).unwrap().is_empty());
    // Deshecho, se puede volver a importar.
    import(&v, "diario.xlsx", &x, now).unwrap();
    assert!(v.verify_audit().is_ok());
}

#[test]
fn csv_sin_numero_de_asiento_y_errores() {
    let v = vault();
    let now = datetime!(2026-10-20 10:00 UTC);
    // Sin columna de número: cada asiento acaba cuando cuadra.
    let csv = "Fecha;Concepto;Cuenta;Debe;Haber\n\
               01/03/2026;Luz;628;121,00;\n\
               01/03/2026;Luz;5720001;;121,00\n\
               02/03/2026;Cobro;5720001;1.000,00;\n\
               02/03/2026;Cobro;4300001;;1.000,00\n";
    let p = preview(&v, "a.csv", csv.as_bytes()).unwrap();
    assert_eq!(p.entries, 2);
    let bad = "Fecha;Concepto;Cuenta;Debe;Haber\n01/03/2026;Luz;628;121,00;\n01/03/2026;Luz;5720001;;120,00\n";
    let e = preview(&v, "b.csv", bad.as_bytes())
        .unwrap_err()
        .to_string();
    assert!(e.contains("no cuadra"), "{e}");
    let e = preview(&v, "c.csv", b"a;b;c\n1;2;3\n")
        .unwrap_err()
        .to_string();
    assert!(e.contains("Fecha, Cuenta, Debe y Haber"), "{e}");

    // A un ejercicio cerrado no se le añade nada.
    manual_entry(
        &v,
        &JournalEntry {
            date: date!(2025 - 05 - 01),
            concept: "x".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("5720001"), Money::from_euros(1, 0)),
                EntryLine::credit(acc("100"), Money::from_euros(1, 0)),
            ],
        },
        now,
    )
    .unwrap();
    close_year(&v, 2025, now, true).unwrap();
    let old =
        "Fecha;Concepto;Cuenta;Debe;Haber\n01/03/2025;Luz;628;1;\n01/03/2025;Luz;5720001;;1\n";
    let e = import(&v, "d.csv", old.as_bytes(), now)
        .unwrap_err()
        .to_string();
    assert!(e.contains("2025") && e.contains("cerrado"), "{e}");
    assert!(batches(&v).unwrap().is_empty(), "no queda el lote a medias");
}

#[test]
fn suenlace_de_taxpain_a_otra_empresa() {
    let a = vault();
    let now = datetime!(2026-10-20 10:00 UTC);
    manual_entry(
        &a,
        &JournalEntry {
            date: date!(2026 - 02 - 01),
            concept: "Préstamo".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("5720001"), Money::from_euros(5000, 0)),
                EntryLine::credit(acc("1700001"), Money::from_euros(5000, 0)),
            ],
        },
        now,
    )
    .unwrap();
    let file = suenlace(
        &a,
        2026,
        &A3Options {
            company: 1,
            digits: 8,
            format: Format::F5,
            invoices: true,
        },
    )
    .unwrap()
    .file;
    let b = vault();
    let p = preview(&b, "SUENLACE.DAT", &file).unwrap();
    assert_eq!((p.source.as_str(), p.entries), ("suenlace", 1));
    import(&b, "SUENLACE.DAT", &file, now).unwrap();
    // A 8 dígitos, como en a3.
    assert_eq!(bal(&b, 2026, "57200001"), Money::from_euros(5000, 0));
    assert_eq!(bal(&b, 2026, "17000001"), -Money::from_euros(5000, 0));
}

#[test]
fn el_diario_reexportado_con_una_correccion_avisa_del_lote_anterior() {
    use taxpain_app::carpeta::{self, Found};

    let v = vault();
    let now = datetime!(2026-10-20 10:00 UTC);
    let d1 = "Fecha;Asiento;Cuenta;Concepto;Debe;Haber\n\
              02/01/2025;1;572;Aportación;3.000,00;\n\
              02/01/2025;1;100;Aportación;;3.000,00\n";
    let d2 = format!("{d1}31/03/2025;2;628;Luz;50,00;\n31/03/2025;2;572;Luz;;50,00\n");
    let (lote, _) = import(&v, "diario.csv", d1.as_bytes(), now).unwrap();
    // La gestoría reenvía el diario corregido (otro fichero: la huella no lo reconoce).
    let p = preview(&v, "diario v2.csv", d2.as_bytes()).unwrap();
    assert!(p.already.is_none());
    assert!(
        p.warnings
            .iter()
            .any(|w| w.contains(&format!("lote {lote}")) && w.contains("deshaz")),
        "{:?}",
        p.warnings
    );
    // En la carpeta de la empresa no se marca para importar.
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("diario v2.csv"), &d2).unwrap();
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    let d = &inv.items[0];
    assert!(
        matches!(
            d.found,
            Found::Diario {
                imported: 1,
                overlap: 0,
                ..
            }
        ),
        "{d:?}"
    );
    assert!(!d.importable && d.selectable, "{d:?}");
    // Deshecho el lote anterior, el corregido entra limpio.
    undo(&v, lote, now, true).unwrap();
    let p = preview(&v, "diario v2.csv", d2.as_bytes()).unwrap();
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    import(&v, "diario v2.csv", d2.as_bytes(), now).unwrap();
    assert_eq!(journal(&v, 2025).unwrap().len(), 2);
}

#[test]
fn deshacer_un_lote_con_un_asiento_enlazado_lo_explica_en_llano() {
    let v = vault();
    let now = datetime!(2026-10-20 10:00 UTC);
    let csv = "Fecha;Asiento;Cuenta;Concepto;Debe;Haber\n\
               20/04/2025;1;4750;Pago IVA 1T;500,00;\n\
               20/04/2025;1;572;Pago IVA 1T;;500,00\n";
    let (lote, _) = import(&v, "diario.csv", csv.as_bytes(), now).unwrap();
    let entry: i64 = v
        .connection()
        .query_row("SELECT id FROM journal_entry", [], |r| r.get(0))
        .unwrap();
    // El pago del 303 del 1T queda registrado sobre ese asiento importado.
    v.connection()
        .execute(
            "INSERT INTO tax_payment(model, year, period, date, account, treasury, amount, entry_id, created_at)
             VALUES ('303', 2025, '1T', '2025-04-20', '4750', '572', 50000, ?1, '2025-04-20T10:00:00Z')",
            [entry],
        )
        .unwrap();
    let err = undo(&v, lote, now, true).unwrap_err().to_string();
    assert!(
        err.contains(&format!("el asiento {entry}")) && err.contains("pago de un modelo"),
        "{err}"
    );
    assert!(!err.contains("FOREIGN KEY"), "{err}");
    // Nada se ha borrado a medias.
    assert_eq!(journal(&v, 2025).unwrap().len(), 1);
    assert_eq!(batches(&v).unwrap().len(), 1);
    // Sin el enlace, se deshace.
    v.connection()
        .execute("DELETE FROM tax_payment", [])
        .unwrap();
    assert_eq!(undo(&v, lote, now, true).unwrap(), 1);
}
