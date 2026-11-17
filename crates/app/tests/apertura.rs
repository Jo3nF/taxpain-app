//! Saldos iniciales: balance del gestor, pendientes en la cartera, inmovilizado con su
//! amortización anterior (CONTABILIDAD-05), Hacienda y bancos, y que no se duplique la apertura.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::{
    dispose_asset, fixed_assets, journal, post_depreciation, trial_balance,
};
use taxpain_app::apertura::{
    self, Asset, Bank, Draft, Line, Pending, Tax, add_asset_with_prior, apply, read_balance,
    read_pending, summary, undo,
};
use taxpain_app::bank::{import_n43, movements, reconcile_invoice, suggestions};
use taxpain_app::cartera::{self, settle, unsettle};
use taxpain_core::accounting::Origin;
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::depreciation::{FixedAsset, Method};
use taxpain_core::money::{Money, Rate};
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

fn line(account: &str, name: &str, debit: i64, credit: i64) -> Line {
    Line {
        account: account.into(),
        name: name.into(),
        debit: eur(debit),
        credit: eur(credit),
    }
}

/// Empresa inventada que empieza en TaxPain el 1/1/2026 con el balance del gestor.
fn sample() -> Draft {
    Draft {
        date: Some(date!(2026 - 01 - 01)),
        step: 6,
        source: None,
        lines: vec![
            line("1000000", "Capital social", 0, 3_000),
            line("4300001", "CLIENTE UNO SL", 1_210, 0),
            line("4000001", "PROVEEDOR DOS SL", 0, 500),
            line("5720001", "Banco", 2_290, 0),
            line("2170000", "Equipos informáticos", 1_200, 0),
            line("2817000", "Amortización equipos", 0, 900),
        ],
        customers: vec![Pending {
            account: Some("4300001".into()),
            tax_id: Some("B12345674".into()),
            name: "CLIENTE UNO SL".into(),
            number: "F-2025-0010".into(),
            issue_date: date!(2025 - 12 - 10),
            due_date: date!(2026 - 01 - 10),
            amount: eur(1_210),
        }],
        suppliers: vec![Pending {
            account: Some("4000001".into()),
            tax_id: Some("B98765431".into()),
            name: "PROVEEDOR DOS SL".into(),
            number: "P-77".into(),
            issue_date: date!(2025 - 12 - 15),
            due_date: date!(2026 - 01 - 15),
            amount: eur(500),
        }],
        assets: vec![Asset {
            name: "Portátil".into(),
            account: "2170000".into(),
            depreciation_account: "2817000".into(),
            expense_account: "681".into(),
            acquired_on: date!(2023 - 01 - 01),
            cost: eur(1_200),
            rate: Rate::percent(25),
            method: Method::Lineal,
            accumulated: eur(900),
        }],
        banks: vec![Bank {
            account: "5720001".into(),
            name: "Banco".into(),
            balance: eur(2_290),
        }],
        // La 4750 no está en el balance: la añade el paso de Hacienda y así cuadra.
        tax: Tax {
            vat_due: eur(300),
            ..Tax::default()
        },
    }
}

#[test]
fn lee_un_balance_de_sumas_y_saldos_sin_contar_dos_veces_los_grupos() {
    let csv = "Balance de sumas y saldos;;;\n\
               Cuenta;Descripción;Saldo deudor;Saldo acreedor\n\
               1;FINANCIACIÓN BÁSICA;;3.000,00\n\
               10;CAPITAL;;3.000,00\n\
               100;Capital social;;3.000,00\n\
               1000000;Capital social;;3.000,00\n\
               430;Clientes;1.210,00;\n\
               4300001;CLIENTE UNO SL;1.210,00;\n\
               5720000;Banco;1.790,00;\n\
               Total;;3.000,00;3.000,00\n";
    let b = read_balance("sumas.csv", csv.as_bytes()).unwrap();
    assert_eq!(b.kind, "sumas_saldos");
    assert_eq!(
        b.lines
            .iter()
            .map(|l| (l.account.as_str(), l.debit, l.credit))
            .collect::<Vec<_>>(),
        vec![
            ("1000000", Money::ZERO, eur(3_000)),
            ("4300001", eur(1_210), Money::ZERO),
            ("5720000", eur(1_790), Money::ZERO),
        ]
    );
    assert_eq!(b.lines[1].name, "CLIENTE UNO SL");

    // Con una sola columna de saldo (deudor positivo).
    let csv = "Cuenta;Nombre;Saldo\n1000000;Capital;-500,00\n5720000;Banco;500,00\n";
    let b = read_balance("saldos.csv", csv.as_bytes()).unwrap();
    assert_eq!(b.lines[0].credit, eur(500));
    assert_eq!(b.lines[1].debit, eur(500));
}

#[test]
fn del_diario_del_gestor_toma_el_asiento_de_apertura() {
    let csv = "Fecha;Asiento;Concepto;Cuenta;Debe;Haber\n\
               01/01/2026;1;Asiento de apertura;5720000;2000,00;\n\
               01/01/2026;1;Asiento de apertura;1000000;;2000,00\n\
               15/01/2026;2;Venta;4300001;121,00;\n\
               15/01/2026;2;Venta;7000000;;100,00\n\
               15/01/2026;2;Venta;4770000;;21,00\n";
    let b = read_balance("diario.csv", csv.as_bytes()).unwrap();
    assert_eq!(b.kind, "apertura");
    assert_eq!(b.date, Some(date!(2026 - 01 - 01)));
    assert_eq!(b.lines.len(), 2);

    // Sin apertura pero con cierre: el inverso del cierre, al día siguiente.
    let csv = "Fecha;Asiento;Concepto;Cuenta;Debe;Haber\n\
               31/12/2025;90;Asiento de cierre;1000000;2000,00;\n\
               31/12/2025;90;Asiento de cierre;5720000;;2000,00\n";
    let b = read_balance("diario.csv", csv.as_bytes()).unwrap();
    assert_eq!(b.kind, "cierre");
    assert_eq!(b.date, Some(date!(2026 - 01 - 01)));
    assert_eq!(b.lines[0].account, "1000000");
    assert_eq!(b.lines[0].credit, eur(2_000));
}

#[test]
fn lee_los_pendientes_de_un_listado_de_cartera_o_de_un_libro() {
    let v = vault();
    let csv = "Cliente;NIF;Factura;Fecha;Vencimiento;Importe\n\
               CLIENTE UNO SL;B12345674;F-10;10/12/2025;10/01/2026;1.210,00\n\
               Total;;;;;1.210,00\n";
    let r = read_pending(&v, "emitida", "cartera.csv", csv.as_bytes()).unwrap();
    assert!(!r.from_book);
    assert_eq!(r.items.len(), 1);
    assert_eq!(r.items[0].due_date, date!(2026 - 01 - 10));
    assert_eq!(r.items[0].amount, eur(1_210));
    assert!(
        r.warnings.is_empty(),
        "la fila Total no avisa: {:?}",
        r.warnings
    );

    let libro = "Fecha;Número;NIF;Cliente;Base imponible;Tipo IVA;Cuota IVA;Total\n\
                 10/12/2025;F-10;B12345674;CLIENTE UNO SL;1000,00;21;210,00;1210,00\n";
    let r = read_pending(&v, "emitida", "emitidas.csv", libro.as_bytes()).unwrap();
    assert!(r.from_book);
    assert_eq!(r.items[0].amount, eur(1_210));
    assert_eq!(
        r.items[0].due_date,
        date!(2026 - 01 - 09),
        "30 días por defecto"
    );
}

#[test]
fn contabiliza_la_apertura_y_los_pendientes_entran_en_la_cartera_y_el_banco() {
    let v = vault();
    let now = datetime!(2026-10-26 10:00 UTC);
    let d = sample();
    apertura::save_draft(&v, &d).unwrap();
    assert_eq!(
        apertura::draft(&v).unwrap(),
        d,
        "se guarda y se sigue después"
    );

    let s = summary(&v, &d).unwrap();
    assert!(s.errors.is_empty(), "{:?}", s.errors);
    assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    assert_eq!(s.difference, Money::ZERO);
    assert_eq!(s.debit, eur(4_700));
    assert!(s.duplicate.is_none());
    assert!(
        s.lines
            .iter()
            .any(|l| l.account == "4750" && l.from == "hacienda")
    );

    let a = apply(&v, &d, false, now).unwrap();
    assert_eq!((a.pending, a.assets, a.fiscal_year), (2, 1, 2026));
    assert_eq!(apertura::draft(&v).unwrap(), Draft::default());
    let entries = journal(&v, 2026).unwrap();
    let opening = entries
        .iter()
        .find(|e| e.entry.origin == Origin::Apertura)
        .unwrap();
    assert_eq!(opening.entry.total_debit(), eur(4_700));
    assert_eq!(opening.entry.date, date!(2026 - 01 - 01));

    // Los pendientes, en la cartera con su vencimiento y su subcuenta.
    let today = date!(2026 - 10 - 26);
    let items = cartera::list(&v, today).unwrap();
    let cobro = items.iter().find(|i| i.kind == "emitida").unwrap();
    assert!(cobro.invoice_id < 0);
    assert_eq!(cobro.number, "F-2025-0010");
    assert_eq!(cobro.party_account.as_deref(), Some("4300001"));
    assert_eq!(cobro.due_date, date!(2026 - 01 - 10));
    assert_eq!(cobro.status, "pendiente");
    assert!(cartera::ledger_mismatches(&v, today).unwrap().is_empty());

    // El ingreso de 1.210 € del extracto se propone y se concilia con el pendiente.
    import_n43(
        &v,
        include_bytes!("../../../fixtures/accounting/n43-ejemplo.txt"),
        now,
    )
    .unwrap();
    let mov = movements(&v, true)
        .unwrap()
        .into_iter()
        .find(|m| m.amount == eur(1_210))
        .unwrap();
    let sug = suggestions(&v, mov.id).unwrap();
    assert!(sug.iter().any(|s| s.id == cobro.invoice_id), "{sug:?}");
    reconcile_invoice(&v, mov.id, "emitida", cobro.invoice_id, Some(1), now).unwrap();
    let tb = trial_balance(&v, 2026, None).unwrap();
    let saldo = |c: &str| {
        tb.iter()
            .find(|b| b.account.as_str() == c)
            .map(|b| b.debit - b.credit)
    };
    assert_eq!(saldo("4300001"), Some(Money::ZERO));
    // Y sale en los conciliados (con su número y su cliente), sin romper la lista.
    let lista = taxpain_app::conciliados::reconciled(&v, None, None, None).unwrap();
    let item = lista
        .items
        .iter()
        .find(|i| i.tx.id == mov.id)
        .expect("el cobro conciliado");
    assert!(item.counterpart.starts_with("Factura "), "{item:?}");
    assert!(item.blocked.is_none(), "{item:?}");

    // El pago al proveedor, en caja.
    let pago = cartera::list(&v, today)
        .unwrap()
        .into_iter()
        .find(|i| i.kind == "recibida")
        .unwrap();
    settle(&v, "recibida", pago.invoice_id, 1, today, "570", now).unwrap();
    assert_eq!(saldo("4000001"), Some(-eur(500)));
    let tb = trial_balance(&v, 2026, None).unwrap();
    assert_eq!(
        tb.iter()
            .find(|b| b.account.as_str() == "4000001")
            .map(|b| b.debit - b.credit),
        Some(Money::ZERO)
    );

    // Deshacer no vale con cobros hechos.
    assert!(undo(&v, a.id, now, true).is_err());
    unsettle(&v, "recibida", pago.invoice_id, 1, now, true).unwrap();
    assert!(
        undo(&v, a.id, now, true).is_err(),
        "sigue el cobro del banco"
    );
}

#[test]
fn no_se_duplica_la_apertura_sin_confirmarlo_y_se_puede_deshacer() {
    let v = vault();
    let now = datetime!(2026-10-26 10:00 UTC);
    let d = sample();
    let a = apply(&v, &d, false, now).unwrap();
    let s = summary(&v, &d).unwrap();
    let dup = s.duplicate.unwrap();
    assert!(dup.contains("hecho con este asistente"), "{dup}");
    let err = apply(&v, &d, false, now).unwrap_err().to_string();
    assert!(err.contains("ya hay un asiento de apertura"), "{err}");

    // Deshacer: se borra todo y vuelve el borrador para corregirlo.
    assert!(undo(&v, a.id, now, false).is_err(), "pide confirmación");
    undo(&v, a.id, now, true).unwrap();
    assert!(apertura::applied(&v).unwrap().is_empty());
    assert_eq!(apertura::draft(&v).unwrap(), d);
    assert!(journal(&v, 2026).unwrap().is_empty());
    assert!(fixed_assets(&v).unwrap().is_empty());
    assert!(cartera::list(&v, date!(2026 - 10 - 26)).unwrap().is_empty());
    // Y se puede volver a contabilizar sin aviso.
    assert!(summary(&v, &d).unwrap().duplicate.is_none());
    apply(&v, &d, false, now).unwrap();
}

#[test]
fn comprueba_que_cuadra_y_las_subcuentas() {
    let v = vault();
    let mut d = sample();
    d.tax = Tax::default();
    let s = summary(&v, &d).unwrap();
    assert_eq!(s.difference, eur(300));
    assert!(
        s.errors.iter().any(|e| e.contains("no cuadra")),
        "{:?}",
        s.errors
    );
    assert!(apply(&v, &d, false, datetime!(2026-10-26 10:00 UTC)).is_err());

    // El balance trae las subcuentas de clientes: hay que decir cuál es la de cada pendiente.
    let mut d = sample();
    d.customers[0].account = None;
    let s = summary(&v, &d).unwrap();
    assert!(
        s.errors.iter().any(|e| e.contains("di cuál es la de")),
        "{:?}",
        s.errors
    );

    // Lo pendiente no coincide con el saldo del balance: aviso.
    let mut d = sample();
    d.customers[0].amount = eur(1_000);
    let s = summary(&v, &d).unwrap();
    assert!(
        s.warnings.iter().any(|w| w.contains("4300001")),
        "{:?}",
        s.warnings
    );

    // Sin balance, los pasos crean las líneas (y la subcuenta del cliente nuevo).
    let d = Draft {
        date: Some(date!(2026 - 01 - 01)),
        lines: vec![line("1000000", "Capital", 0, 1_710)],
        customers: vec![Pending {
            account: None,
            ..sample().customers[0].clone()
        }],
        banks: vec![Bank {
            account: "5720001".into(),
            name: "Banco".into(),
            balance: eur(500),
        }],
        ..Draft::default()
    };
    let s = summary(&v, &d).unwrap();
    assert!(s.errors.is_empty(), "{:?}", s.errors);
    assert!(s.lines.iter().any(|l| l.new_account && l.account == "430"));
    apply(&v, &d, false, datetime!(2026-10-26 10:00 UTC)).unwrap();
    let item = cartera::list(&v, date!(2026 - 10 - 26)).unwrap();
    assert_eq!(item[0].party_account.as_deref(), Some("4300001"));
}

fn laptop() -> FixedAsset {
    FixedAsset {
        id: 0,
        name: "Portátil".into(),
        account: acc("217"),
        depreciation_account: acc("2817"),
        expense_account: acc("681"),
        acquired_on: date!(2023 - 01 - 01),
        cost: eur(1_200),
        residual: Money::ZERO,
        rate: Rate::percent(25),
        disposed_on: None,
        method: Method::Lineal,
        tax_rate: None,
    }
}

fn charge(v: &Vault, year: i32) -> Money {
    post_depreciation(v, year, datetime!(2026-10-26 10:00 UTC)).unwrap();
    journal(v, year)
        .unwrap()
        .iter()
        .filter(|e| matches!(e.entry.origin, Origin::Amortizacion(_)))
        .map(|e| e.entry.total_debit())
        .sum()
}

/// CONTABILIDAD-05: un bien de 2023 ya amortizado en tres cuartas partes sólo amortiza lo que
/// le queda (300 €), no otra vez desde cero.
#[test]
fn la_amortizacion_sigue_desde_lo_amortizado_antes_de_taxpain() {
    let v = vault();
    let now = datetime!(2026-10-26 10:00 UTC);
    add_asset_with_prior(&v, &laptop(), eur(900), Some(date!(2025 - 12 - 31)), now).unwrap();
    assert_eq!(charge(&v, 2026), eur(300));
    assert_eq!(charge(&v, 2027), Money::ZERO);
    let p = apertura::priors(&v).unwrap();
    assert_eq!(
        (p[0].amount, p[0].until),
        (eur(900), Some(date!(2025 - 12 - 31)))
    );
    assert!(
        add_asset_with_prior(&v, &laptop(), eur(1_300), Some(date!(2025 - 12 - 31)), now).is_err(),
        "no más que el coste"
    );
}

/// Apertura a mitad de año: lo amortizado llega al 30/6 y TaxPain amortiza sólo del 1/7.
#[test]
fn con_apertura_a_mitad_de_ano_amortiza_solo_desde_el_dia_siguiente() {
    let v = vault();
    let now = datetime!(2026-10-26 10:00 UTC);
    let id =
        add_asset_with_prior(&v, &laptop(), eur(750), Some(date!(2026 - 06 - 30)), now).unwrap();
    // 300 € × 184 días / 365.
    assert_eq!(charge(&v, 2026), Money::from_cents(15_123));
    assert_eq!(charge(&v, 2027), Money::from_cents(29_877));
    assert_eq!(charge(&v, 2028), Money::ZERO);
    // La baja da de baja también lo amortizado antes de TaxPain.
    let r = dispose_asset(&v, id, date!(2029 - 03 - 01), Money::ZERO, now).unwrap();
    assert_eq!(r.result, Money::ZERO, "ya estaba amortizado del todo");
}

/// La 4700 de una apertura del 1 de enero es IVA a compensar: pasa a la casilla [110] del 303
/// del 1T (sin pisar lo que ya hubiera en los ajustes) y al deshacer la apertura se quita.
#[test]
fn la_4700_de_la_apertura_pasa_a_compensar_en_el_303_y_se_deshace() {
    use taxpain_app::models::{self, ModelSettings};
    let v = vault();
    let now = datetime!(2026-10-26 10:00 UTC);
    let mut d = sample();
    d.tax.vat_refund = eur(350);
    d.lines[0].credit = eur(3_350);
    let s = summary(&v, &d).unwrap();
    assert!(s.errors.is_empty(), "{:?}", s.errors);
    assert!(
        s.also
            .iter()
            .any(|a| a.contains("[110]") && a.contains("350")),
        "{:?}",
        s.also
    );
    let a = apply(&v, &d, false, now).unwrap();
    taxpain_app::invoicing::save_settings(&v, &issuer(), now).unwrap();
    let ms = models::settings(&v).unwrap();
    assert_eq!(ms.vat_compensation_carried, eur(350));
    assert_eq!(ms.vat_compensation_year, Some(2026));
    let q1 = models::draft(&v, "303", 2026, 1).unwrap();
    let c110 = q1
        .boxes
        .iter()
        .find(|(b, _)| *b == 110)
        .map(|(_, m)| *m)
        .unwrap_or_default();
    assert_eq!(c110, eur(350), "{:?}", q1.boxes);

    // Deshacer la deja como estaba.
    undo(&v, a.id, now, true).unwrap();
    assert_eq!(models::settings(&v).unwrap(), ModelSettings::default());

    // Con otra cifra ya anotada: se avisa y no se pisa.
    let mine = ModelSettings {
        vat_compensation_carried: eur(100),
        vat_compensation_year: Some(2026),
        ..ModelSettings::default()
    };
    models::save_settings(&v, &mine, now).unwrap();
    let s = summary(&v, &d).unwrap();
    assert!(s.also.is_empty(), "{:?}", s.also);
    assert!(
        s.warnings.iter().any(|w| w.contains("ya hay 100,00")),
        "{:?}",
        s.warnings
    );
    let a = apply(&v, &d, false, now).unwrap();
    assert_eq!(models::settings(&v).unwrap(), mine);
    undo(&v, a.id, now, true).unwrap();
    assert_eq!(models::settings(&v).unwrap(), mine);
}

fn issuer() -> taxpain_app::settings::InvoicingSettings {
    use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
    use taxpain_core::invoice::{Party, PartyId, RoundingMode};
    InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("B12345674".into()),
            name: "EMPRESA DEMO SL".into(),
            address: addr(),
            person: None,
        },
        default_series: "A".into(),
        rectifying_series: "R".into(),
        simplified_series: "S".into(),
        simplified_rectifying_series: "RS".into(),
        language: Language::Es,
        default_vat: Rate::percent(21),
        default_withholding: None,
        outputs: Outputs {
            pdf: false,
            facturae: false,
            ubl: false,
        },
        destination: None,
        rounding: RoundingMode::PerRate,
        verifactu: VerifactuSettings {
            enabled: false,
            installation_id: "x".into(),
            production: false,
        },
        footer: None,
        activity: None,
        simplified_up_to_3000: false,
    }
}

/// La hoja «Cartera» del Excel del ejercicio lleva también los pendientes de la apertura
/// (facturas de antes), pendientes o cobrados y pagados en el ejercicio.
#[test]
fn la_cartera_del_excel_lleva_los_pendientes_de_la_apertura() {
    use calamine::{Data, Reader, open_workbook_auto_from_rs};
    use taxpain_app::listings::workbook;
    use taxpain_core::accounting::statements::ModelKind;
    let v = vault();
    let now = datetime!(2026-10-26 10:00 UTC);
    let today = date!(2026 - 10 - 26);
    taxpain_app::invoicing::save_settings(&v, &issuer(), now).unwrap();
    apply(&v, &sample(), false, now).unwrap();
    let pago = cartera::list(&v, today)
        .unwrap()
        .into_iter()
        .find(|i| i.kind == "recibida")
        .unwrap();
    settle(&v, "recibida", pago.invoice_id, 1, today, "570", now).unwrap();
    let (_, bytes) = workbook(&v, 2026, ModelKind::Pymes, today).unwrap();
    let mut wb = open_workbook_auto_from_rs(std::io::Cursor::new(bytes)).unwrap();
    let c = wb.worksheet_range("Cartera").unwrap();
    let cells: Vec<String> = c
        .cells()
        .filter_map(|(_, _, d)| match d {
            Data::String(s) => Some(s.clone()),
            _ => None,
        })
        .collect();
    for n in ["F-2025-0010", "P-77", "Pendiente", "26/10/2026"] {
        assert!(cells.iter().any(|s| s == n), "falta {n}: {cells:?}");
    }
}

/// Con amortizaciones ya contabilizadas la apertura no se deshace y el mensaje dice qué hacer
/// (no hay forma de borrar esas amortizaciones).
#[test]
fn con_amortizaciones_no_se_deshace_y_dice_por_que() {
    let v = vault();
    let now = datetime!(2026-10-26 10:00 UTC);
    let a = apply(&v, &sample(), false, now).unwrap();
    assert!(!post_depreciation(&v, 2026, now).unwrap().is_empty());
    let err = undo(&v, a.id, now, true).unwrap_err().to_string();
    assert!(err.contains("no se pueden borrar"), "{err}");
    assert!(err.contains("«Nuevo asiento»"), "{err}");
    assert_eq!(apertura::applied(&v).unwrap().len(), 1);
}

/// Domicilio inventado: la factura completa lo exige (art. 6.1.e RD 1619/2012).
fn addr() -> Option<taxpain_core::invoice::Address> {
    Some(taxpain_core::invoice::Address {
        street: "Calle Inventada 1".into(),
        post_code: "46001".into(),
        town: "València".into(),
        province: "Valencia".into(),
        country: "ES".into(),
    })
}
