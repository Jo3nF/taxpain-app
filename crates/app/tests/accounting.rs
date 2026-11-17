//! Contabilidad integrada: facturas → asientos → libros → cierre.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::{
    account_name, chart, close_year, delete_manual_entry, journal, ledger_of, manual_entry,
    record_entry, set_account, set_tick, trial_balance, update_manual_entry,
};
use taxpain_app::invoicing::{self, InvoiceDraft, cancel, issue};
use taxpain_app::received::{Nature, Reviewed, Source, ingest_file, review};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

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

fn setup() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("12345678Z".into()),
            name: "Carmen".into(),
            address: addr(),
            person: None,
        },
        default_series: "A".into(),
        rectifying_series: "R".into(),
        simplified_series: "S".into(),
        simplified_rectifying_series: "RS".into(),
        language: Language::Es,
        default_vat: Rate::percent(21),
        default_withholding: Some(Rate::percent(15)),
        outputs: Outputs {
            pdf: true,
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
    };
    invoicing::save_settings(&v, &s, datetime!(2026-01-01 9:00 UTC)).unwrap();
    v
}

fn draft(nif: &str, name: &str, euros: i64, date: time::Date) -> InvoiceDraft {
    InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: date,
        operation_date: None,
        recipient: Some(Party {
            id: PartyId::Nif(nif.into()),
            name: name.into(),
            address: addr(),
            person: None,
        }),
        lines: vec![InvoiceLine {
            description: "Consultoría".into(),
            quantity_milli: 1000,
            unit_price: Money::from_euros(euros, 0),
            discount: Rate::ZERO,
            vat: VatTreatment::Taxed {
                rate: Rate::percent(21),
            },
            surcharge: None,
            withholding: Some(Rate::percent(15)),
            advance: false,
        }],
        rectified: vec![],
        rectified_amounts: None,
        description: "Servicios".into(),
        notes: None,
        currency: None,
        oss: None,
    }
}

#[test]
fn facturas_libros_y_cierre() {
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);

    // Aportación inicial (asiento manual).
    manual_entry(
        &v,
        &JournalEntry {
            date: date!(2026 - 01 - 02),
            concept: "Aportación inicial".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("5720001"), Money::from_euros(3000, 0)),
                EntryLine::credit(acc("102"), Money::from_euros(3000, 0)),
            ],
        },
        now,
    )
    .unwrap();

    let a = issue(
        &v,
        &draft("B12345674", "Cliente Uno SL", 1000, date!(2026 - 03 - 10)),
        now,
        true,
    )
    .unwrap();
    issue(
        &v,
        &draft("A22222228", "Cliente Dos SA", 500, date!(2026 - 04 - 15)),
        now,
        true,
    )
    .unwrap();
    let c = issue(
        &v,
        &draft("B12345674", "Cliente Uno SL", 200, date!(2026 - 05 - 20)),
        now,
        true,
    )
    .unwrap();
    // Anulada dentro de su trimestre, antes de declararlo (con el 303 presentado o vencido
    // habría que rectificarla).
    cancel(&v, c.id, datetime!(2026-05-25 10:00 UTC), true).unwrap();
    // El contraasiento enlaza con la factura y no se borra desde el diario.
    let reversal = journal(&v, 2026)
        .unwrap()
        .into_iter()
        .find(|e| e.entry.origin == Origin::AnulacionFactura(c.id))
        .expect("contraasiento de la anulación");
    assert!(delete_manual_entry(&v, reversal.id, now, true).is_err());

    // Recibida revisada: 300 + 63 IVA.
    let rec = ingest_file(
        &v,
        "f.xml",
        b"<?xml version=\"1.0\"?><x/>",
        Source::Manual,
        now,
    )
    .unwrap();
    review(
        &v,
        rec.id,
        &Reviewed {
            supplier_nif: "B12345674".into(),
            supplier_name: "Proveedor SL".into(),
            number: "P-1".into(),
            issue_date: date!(2026 - 06 - 01),
            base: Money::from_euros(300, 0),
            vat: Money::from_euros(63, 0),
            withholding: Money::ZERO,
            total: Money::from_euros(363, 0),
            deductible_pct: 100,
            capital_good: false,
            nature: Nature::General,
            asset_class: None,
            vat_regime: Default::default(),
            supplier_country: None,
            self_assessed_rate: None,
            intra_goods: false,
            cash_basis: false,
            import_dua: false,
            prorata_use: Default::default(),
        },
        now,
    )
    .unwrap();

    let diario = journal(&v, 2026).unwrap();
    // aportación + 3 emitidas + anulación + recibida
    assert_eq!(diario.len(), 6);
    assert!(diario.iter().all(|s| s.entry.validate().is_ok()));
    assert_eq!(
        diario.iter().map(|s| s.number).collect::<Vec<_>>(),
        [1, 2, 3, 4, 5, 6]
    );

    let tb = trial_balance(&v, 2026, None).unwrap();
    let d: Money = tb.iter().map(|b| b.debit).sum();
    let cr: Money = tb.iter().map(|b| b.credit).sum();
    assert_eq!(d, cr, "sumas y saldos cuadra");

    // Subcuentas por tercero: el mismo cliente reutiliza la suya.
    let accounts: Vec<&str> = tb.iter().map(|b| b.account.as_str()).collect();
    assert!(accounts.contains(&"4300001") && accounts.contains(&"4300002"));
    assert!(accounts.contains(&"4100001"), "proveedor en 410");
    // IVA repercutido neto: 210 + 105 (la anulada se compensa) ; soportado 63.
    let bal = |c: &str| {
        tb.iter()
            .find(|b| b.account.as_str() == c)
            .unwrap()
            .balance()
    };
    assert_eq!(bal("477"), -Money::from_euros(315, 0));
    assert_eq!(bal("472"), Money::from_euros(63, 0));
    assert_eq!(bal("473"), Money::from_euros(225, 0));
    assert_eq!(bal("705"), -Money::from_euros(1500, 0));

    let mayor = ledger_of(&v, 2026, "4300001").unwrap();
    assert_eq!(mayor.last().unwrap().balance, Money::from_euros(1060, 0));

    // Una factura no se contabiliza dos veces.
    let dup = JournalEntry {
        date: date!(2026 - 03 - 10),
        concept: "dup".into(),
        origin: Origin::FacturaEmitida(a.id),
        lines: vec![
            EntryLine::debit(acc("430"), Money::from_cents(1)),
            EntryLine::credit(acc("705"), Money::from_cents(1)),
        ],
    };
    assert!(record_entry(v.connection(), &dup, now).is_err());

    // Cierre: resultado 1.500 − 300 = 1.200 de beneficio.
    assert!(close_year(&v, 2026, now, false).is_err());
    let s = close_year(&v, 2026, now, true).unwrap();
    assert_eq!(s.result, Money::from_euros(1200, 0));
    assert!(
        trial_balance(&v, 2026, None)
            .unwrap()
            .iter()
            .all(|b| b.balance().is_zero())
    );
    let open = trial_balance(&v, 2027, None).unwrap();
    assert_eq!(
        open.iter()
            .find(|b| b.account.as_str() == "129")
            .unwrap()
            .balance(),
        -Money::from_euros(1200, 0)
    );
    assert_eq!(
        open.iter()
            .find(|b| b.account.as_str() == "5720001")
            .unwrap()
            .balance(),
        Money::from_euros(3000, 0)
    );

    // Ejercicio cerrado: no admite asientos ni cambios.
    assert!(
        manual_entry(
            &v,
            &JournalEntry {
                date: date!(2026 - 12 - 15),
                concept: "tarde".into(),
                origin: Origin::Manual,
                lines: vec![
                    EntryLine::debit(acc("629"), Money::from_cents(100)),
                    EntryLine::credit(acc("5720001"), Money::from_cents(100))
                ],
            },
            now
        )
        .is_err()
    );
    assert!(
        v.connection()
            .execute("UPDATE journal_line SET debit = debit + 1 WHERE id = 1", [])
            .is_err()
    );
    assert!(
        close_year(&v, 2026, now, true).is_err(),
        "no se cierra dos veces"
    );
    assert!(
        issue(
            &v,
            &draft("B12345674", "Cliente Uno SL", 10, date!(2026 - 12 - 30)),
            now,
            true
        )
        .is_err(),
        "no se factura en un ejercicio cerrado"
    );
}

#[test]
fn amortizaciones_anuales_sin_duplicar() {
    use taxpain_app::accounting::{add_fixed_asset, post_depreciation};
    use taxpain_core::accounting::depreciation::FixedAsset;
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    let id = add_fixed_asset(
        &v,
        &FixedAsset {
            id: 0,
            name: "Portátil".into(),
            account: acc("217"),
            depreciation_account: acc("2817"),
            expense_account: acc("681"),
            acquired_on: date!(2026 - 07 - 01),
            cost: Money::from_euros(1200, 0),
            residual: Money::ZERO,
            rate: Rate::percent(25),
            disposed_on: None,
            method: Default::default(),
            tax_rate: None,
        },
        now,
    )
    .unwrap();
    assert!(id > 0);
    assert_eq!(post_depreciation(&v, 2026, now).unwrap().len(), 1);
    assert!(
        post_depreciation(&v, 2026, now).unwrap().is_empty(),
        "idempotente"
    );
    assert_eq!(post_depreciation(&v, 2027, now).unwrap().len(), 1);
    let b26 = trial_balance(&v, 2026, None).unwrap();
    assert_eq!(
        b26.iter()
            .find(|b| b.account.as_str() == "681")
            .unwrap()
            .balance(),
        Money::from_cents(15_123)
    );
    let b27 = trial_balance(&v, 2027, None).unwrap();
    assert_eq!(
        b27.iter()
            .find(|b| b.account.as_str() == "2817")
            .unwrap()
            .balance(),
        -Money::from_euros(300, 0)
    );
}

#[test]
fn liquidacion_trimestral_del_iva() {
    use taxpain_app::accounting::settle_vat_quarter;
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    issue(
        &v,
        &draft("B12345674", "Cliente", 1000, date!(2026 - 02 - 10)),
        now,
        true,
    )
    .unwrap();
    issue(
        &v,
        &draft("B12345674", "Cliente", 500, date!(2026 - 04 - 10)),
        now,
        true,
    )
    .unwrap();
    let id = settle_vat_quarter(&v, 2026, 1, now).unwrap();
    assert!(id.is_some());
    assert_eq!(
        settle_vat_quarter(&v, 2026, 1, now).unwrap(),
        id,
        "no se liquida dos veces: devuelve el asiento existente"
    );
    let tb = trial_balance(&v, 2026, None).unwrap();
    let bal = |c: &str| {
        tb.iter()
            .find(|b| b.account.as_str() == c)
            .map_or(Money::ZERO, |b| b.balance())
    };
    assert_eq!(
        bal("4750"),
        -Money::from_euros(210, 0),
        "1T: 210 a ingresar"
    );
    assert_eq!(
        bal("477"),
        -Money::from_euros(105, 0),
        "el 2T sigue pendiente"
    );
    assert!(settle_vat_quarter(&v, 2026, 5, now).is_err());
}

/// Una factura del trimestre registrada después de liquidarlo: «Rehacer» borra el asiento y lo
/// vuelve a hacer con el 303 de ahora; con el pago registrado ya no se puede.
#[test]
fn rehacer_la_liquidacion_del_iva() {
    use taxpain_app::accounting::{redo_vat_quarter, settle_vat_quarter};
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    issue(
        &v,
        &draft("B12345674", "Cliente", 1000, date!(2026 - 02 - 10)),
        now,
        true,
    )
    .unwrap();
    let old = settle_vat_quarter(&v, 2026, 1, now).unwrap().unwrap();
    let bal = |c: &str| {
        trial_balance(&v, 2026, None)
            .unwrap()
            .iter()
            .find(|b| b.account.as_str() == c)
            .map_or(Money::ZERO, |b| b.balance())
    };
    issue(
        &v,
        &draft("B12345674", "Cliente", 500, date!(2026 - 03 - 15)),
        now,
        true,
    )
    .unwrap();
    assert_eq!(
        bal("4750"),
        -Money::from_euros(210, 0),
        "la liquidación vieja"
    );
    assert!(
        redo_vat_quarter(&v, 2026, 1, now, false).is_err(),
        "pide confirmación"
    );
    let new = redo_vat_quarter(&v, 2026, 1, now, true).unwrap().unwrap();
    assert_ne!(new, old);
    assert_eq!(bal("4750"), -Money::from_euros(315, 0));
    assert_eq!(bal("477"), Money::ZERO);
    assert!(
        redo_vat_quarter(&v, 2026, 2, now, true).is_err(),
        "el 2T no está liquidado"
    );
    taxpain_app::tax_payments::pay(&v, "303", 2026, 1, date!(2026 - 04 - 20), "572", now).unwrap();
    let err = redo_vat_quarter(&v, 2026, 1, now, true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("pago"), "{err}");
}

/// CONTABILIDAD-02: el 1T sale a compensar (21 − 63 = −42) y el 2T (210) compensa esos 42 e
/// ingresa 168. La liquidación del 2T abona la 4700 por lo compensado y la 4750 sólo por lo
/// que se paga: tras el pago las dos quedan a cero.
#[test]
fn liquidaciones_encadenadas_con_compensacion() {
    use taxpain_app::accounting::settle_vat_quarter;
    use taxpain_app::received::register_typed;
    let v = setup();
    let q1 = datetime!(2026-03-20 10:00 UTC);
    let mut venta = draft("B12345674", "Cliente", 100, date!(2026 - 02 - 10));
    venta.lines[0].withholding = None;
    issue(&v, &venta, q1, true).unwrap();
    register_typed(
        &v,
        &Reviewed {
            supplier_nif: "B98765431".into(),
            supplier_name: "EMPRESA DEMO SL".into(),
            number: "C-1".into(),
            issue_date: date!(2026 - 03 - 01),
            base: Money::from_euros(300, 0),
            vat: Money::from_euros(63, 0),
            withholding: Money::ZERO,
            total: Money::from_euros(363, 0),
            deductible_pct: 100,
            capital_good: false,
            nature: Nature::General,
            asset_class: None,
            vat_regime: Default::default(),
            supplier_country: None,
            self_assessed_rate: None,
            intra_goods: false,
            cash_basis: false,
            import_dua: false,
            prorata_use: Default::default(),
        },
        q1,
    )
    .unwrap();
    let q2 = datetime!(2026-07-10 10:00 UTC);
    let mut venta = draft("B12345674", "Cliente", 1000, date!(2026 - 05 - 10));
    venta.lines[0].withholding = None;
    issue(&v, &venta, q2, true).unwrap();
    settle_vat_quarter(&v, 2026, 1, q2).unwrap().unwrap();
    let bal = |c: &str| {
        trial_balance(&v, 2026, None)
            .unwrap()
            .iter()
            .find(|b| b.account.as_str() == c)
            .map_or(Money::ZERO, |b| b.balance())
    };
    assert_eq!(bal("4700"), Money::from_euros(42, 0), "1T a compensar");
    let d2 = taxpain_app::models::draft(&v, "303", 2026, 2).unwrap();
    assert_eq!(d2.result, Money::from_euros(168, 0));
    taxpain_app::tax_payments::pay(&v, "303", 2026, 2, date!(2026 - 07 - 15), "572", q2).unwrap();
    for c in ["4700", "4750", "477", "472"] {
        assert_eq!(bal(c), Money::ZERO, "{c}");
    }
    let e = journal(&v, 2026)
        .unwrap()
        .into_iter()
        .find(|e| e.entry.origin == Origin::LiquidacionIva(202602))
        .unwrap();
    assert!(
        e.entry
            .lines
            .iter()
            .all(|l| !l.account.as_str().starts_with("63")),
        "sin diferencias con el 303"
    );
}

#[test]
fn libros_registro_xlsx() {
    use std::io::Read;
    use taxpain_app::books::lsi_workbook;
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    issue(
        &v,
        &draft("B12345674", "Cliente Uno SL", 1000, date!(2026 - 03 - 10)),
        now,
        true,
    )
    .unwrap();
    let c = issue(
        &v,
        &draft("A22222228", "Cliente Dos SA", 500, date!(2026 - 04 - 15)),
        now,
        true,
    )
    .unwrap();
    // Anulada dentro de su trimestre, antes de declararlo.
    cancel(&v, c.id, datetime!(2026-04-20 10:00 UTC), true).unwrap();
    let rec = ingest_file(
        &v,
        "f.xml",
        b"<?xml version=\"1.0\"?><y/>",
        Source::Manual,
        now,
    )
    .unwrap();
    review(
        &v,
        rec.id,
        &Reviewed {
            supplier_nif: "B12345674".into(),
            supplier_name: "Proveedor SL".into(),
            number: "P-1".into(),
            issue_date: date!(2026 - 06 - 01),
            base: Money::from_euros(300, 0),
            vat: Money::from_euros(63, 0),
            withholding: Money::ZERO,
            total: Money::from_euros(363, 0),
            deductible_pct: 100,
            capital_good: false,
            nature: Nature::General,
            asset_class: None,
            vat_regime: Default::default(),
            supplier_country: None,
            self_assessed_rate: None,
            intra_goods: false,
            cash_basis: false,
            import_dua: false,
            prorata_use: Default::default(),
        },
        now,
    )
    .unwrap();
    let (name, bytes) = lsi_workbook(&v, 2026).unwrap();
    assert_eq!(name, "202612345678ZTCarmen.xlsx");
    if let Ok(dir) = std::env::var("TAXPAIN_DUMP_DIR") {
        std::fs::write(std::path::Path::new(&dir).join(&name), &bytes).unwrap();
    }
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut wb = String::new();
    z.by_name("xl/workbook.xml")
        .unwrap()
        .read_to_string(&mut wb)
        .unwrap();
    assert!(wb.contains("EXPEDIDAS_INGRESOS") && wb.contains("RECIBIDAS_GASTOS"));
    let mut shared = String::new();
    z.by_name("xl/sharedStrings.xml")
        .unwrap()
        .read_to_string(&mut shared)
        .unwrap();
    assert!(
        shared.contains("A2026-0001") && !shared.contains("A2026-0002"),
        "la anulada no aparece"
    );
    assert!(shared.contains("P-1") && shared.contains("10/03/2026"));
}

/// Validación 11 del LSI: Ejercicio y Periodo son los de la fecha de operación (si es anterior a
/// la de expedición), como en el 303; en el cambio de año la factura va al libro del devengo.
#[test]
fn el_libro_de_expedidas_sigue_la_fecha_de_operacion() {
    use std::io::Read;
    use taxpain_app::books::lsi_workbook;
    let v = setup();
    let now = datetime!(2027-01-10 10:00 UTC);
    let mut d = draft("B12345674", "Cliente Uno SL", 1000, date!(2026 - 04 - 03));
    d.operation_date = Some(date!(2026 - 03 - 30));
    issue(&v, &d, now, true).unwrap();
    let mut d = draft("B12345674", "Cliente Uno SL", 500, date!(2027 - 01 - 05));
    d.operation_date = Some(date!(2026 - 12 - 28));
    issue(&v, &d, now, true).unwrap();
    let shared = |year: i32| {
        let (_, bytes) = lsi_workbook(&v, year).unwrap();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut s = String::new();
        z.by_name("xl/sharedStrings.xml")
            .unwrap()
            .read_to_string(&mut s)
            .unwrap();
        s
    };
    let s26 = shared(2026);
    // Operación del 30/3 expedida el 3/4: 1T, con las dos fechas en sus columnas.
    assert!(s26.contains(">1T<") && !s26.contains(">2T<"), "{s26}");
    assert!(s26.contains("03/04/2026") && s26.contains("30/03/2026"));
    // La expedida el 5/1/2027 por una operación del 28/12: libro de 2026, 4T, y no en el de 2027.
    assert!(s26.contains(">4T<") && s26.contains("28/12/2026"));
    assert!(!shared(2027).contains("28/12/2026"));
}

#[test]
fn cuentas_anuales_antes_y_despues_del_cierre() {
    use taxpain_app::accounting::annual_accounts;
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    manual_entry(
        &v,
        &JournalEntry {
            date: date!(2026 - 01 - 02),
            concept: "Capital".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("5720001"), Money::from_euros(3000, 0)),
                EntryLine::credit(acc("100"), Money::from_euros(3000, 0)),
            ],
        },
        now,
    )
    .unwrap();
    let mut d = draft("B12345674", "Cliente", 1000, date!(2026 - 03 - 10));
    d.lines[0].withholding = None;
    issue(&v, &d, now, true).unwrap();
    let a = annual_accounts(&v, 2026).unwrap();
    assert!(a.balanced, "{:?}", a.balance.unmapped);
    assert_eq!(a.income.get("D"), Money::from_euros(1000, 0));
    assert_eq!(a.balance.get("PA-1.VII"), Money::from_euros(1000, 0));
    close_year(&v, 2026, now, true).unwrap();
    let after = annual_accounts(&v, 2026).unwrap();
    assert_eq!(
        after, a,
        "el cierre no cambia las cuentas anuales del ejercicio"
    );
}

#[test]
fn diario_en_orden_cronologico() {
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    let entry = |d, c: &str| JournalEntry {
        date: d,
        concept: c.into(),
        origin: Origin::Manual,
        lines: vec![
            EntryLine::debit(acc("629"), Money::from_euros(10, 0)),
            EntryLine::credit(acc("5720001"), Money::from_euros(10, 0)),
        ],
    };
    manual_entry(&v, &entry(date!(2026 - 05 - 10), "mayo"), now).unwrap();
    // Registrado después, pero con fecha anterior: va antes en el diario.
    manual_entry(&v, &entry(date!(2026 - 02 - 01), "febrero"), now).unwrap();
    let j = journal(&v, 2026).unwrap();
    let order: Vec<(i64, &str)> = j
        .iter()
        .map(|s| (s.number, s.entry.concept.as_str()))
        .collect();
    assert_eq!(order, [(1, "febrero"), (2, "mayo")]);
}

#[test]
fn plan_de_cuentas_con_nombres() {
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    issue(
        &v,
        &draft("B12345674", "Cliente Uno SL", 1000, date!(2026 - 03 - 10)),
        now,
        true,
    )
    .unwrap();
    // Cuenta propia: un segundo banco.
    set_account(&v, "5720002", "Banco Sabadell", now).unwrap();
    assert!(set_account(&v, "57", "x", now).is_err());
    assert!(set_account(&v, "572a", "x", now).is_err());
    assert!(set_account(&v, "5720003", "  ", now).is_err());

    let c = chart(&v, 2026).unwrap();
    let get = |code: &str| c.iter().find(|a| a.code == code).unwrap();
    assert_eq!(get("705").name, "Prestaciones de servicios");
    assert_eq!(get("705").credit, Money::from_euros(1000, 0));
    let client = get("4300001");
    assert_eq!(client.name, "Cliente Uno SL");
    assert_eq!(client.source, "tercero");
    assert_eq!(client.nif.as_deref(), Some("B12345674"));
    assert_eq!(get("5720002").source, "propia");
    assert!(c.windows(2).all(|w| w[0].code < w[1].code));

    let conn = v.connection();
    assert_eq!(
        account_name(conn, "5720002").unwrap().as_deref(),
        Some("Banco Sabadell")
    );
    assert_eq!(
        account_name(conn, "5720001").unwrap().as_deref(),
        Some("Bancos e instituciones de crédito c/c vista, euros")
    );
    // Cambiar el nombre no duplica.
    set_account(&v, "5720002", "Banco Sabadell (empresa)", now).unwrap();
    assert_eq!(
        chart(&v, 2026)
            .unwrap()
            .iter()
            .filter(|a| a.code == "5720002")
            .count(),
        1
    );
}

#[test]
fn modificar_y_borrar_asientos_manuales() {
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    let entry = |euros: i64, date: time::Date| JournalEntry {
        date,
        concept: "Comisión".into(),
        origin: Origin::Manual,
        lines: vec![
            EntryLine::debit(acc("626"), Money::from_euros(euros, 0)),
            EntryLine::credit(acc("5720001"), Money::from_euros(euros, 0)),
        ],
    };
    let id = manual_entry(&v, &entry(10, date!(2026 - 05 - 01)), now).unwrap();
    update_manual_entry(&v, id, &entry(12, date!(2026 - 05 - 02)), now).unwrap();
    let j = journal(&v, 2026).unwrap();
    assert_eq!(j.len(), 1);
    assert_eq!(j[0].entry.date, date!(2026 - 05 - 02));
    assert_eq!(j[0].entry.lines[0].debit, Money::from_euros(12, 0));
    // No se cambia de ejercicio ni se descuadra.
    assert!(update_manual_entry(&v, id, &entry(12, date!(2027 - 01 - 02)), now).is_err());
    let mut bad = entry(12, date!(2026 - 05 - 02));
    bad.lines.pop();
    assert!(update_manual_entry(&v, id, &bad, now).is_err());

    // Los asientos de facturas no se tocan desde aquí.
    issue(
        &v,
        &draft("B12345674", "Cliente Uno SL", 1000, date!(2026 - 03 - 10)),
        now,
        true,
    )
    .unwrap();
    let invoice_entry = journal(&v, 2026)
        .unwrap()
        .into_iter()
        .find(|e| e.entry.origin != Origin::Manual)
        .unwrap();
    assert!(delete_manual_entry(&v, invoice_entry.id, now, true).is_err());
    assert!(
        update_manual_entry(&v, invoice_entry.id, &entry(1, date!(2026 - 03 - 10)), now).is_err()
    );

    assert!(delete_manual_entry(&v, id, now, false).is_err());
    delete_manual_entry(&v, id, now, true).unwrap();
    assert_eq!(journal(&v, 2026).unwrap().len(), 1);
    let log = taxpain_vault::audit::entries(v.connection()).unwrap();
    assert!(log.iter().any(|e| e.action == "journal.update"));
    assert!(
        log.iter()
            .any(|e| e.action == "journal.delete" && e.details.contains("626"))
    );
    assert!(v.verify_audit().is_ok());
}

#[test]
fn punteo_del_mayor() {
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    for (d, e) in [(date!(2026 - 02 - 01), 10), (date!(2026 - 03 - 01), 20)] {
        manual_entry(
            &v,
            &JournalEntry {
                date: d,
                concept: "Comisión".into(),
                origin: Origin::Manual,
                lines: vec![
                    EntryLine::debit(acc("626"), Money::from_euros(e, 0)),
                    EntryLine::credit(acc("5720001"), Money::from_euros(e, 0)),
                ],
            },
            now,
        )
        .unwrap();
    }
    let m = ledger_of(&v, 2026, "572").unwrap();
    assert_eq!(m.len(), 2);
    assert!(m.iter().all(|l| !l.ticked));
    set_tick(&v, m[0].line_id, true, now).unwrap();
    set_tick(&v, m[0].line_id, true, now).unwrap(); // idempotente
    let m2 = ledger_of(&v, 2026, "572").unwrap();
    assert!(m2[0].ticked && !m2[1].ticked);
    // La línea del 626 del mismo asiento no queda punteada.
    assert!(
        ledger_of(&v, 2026, "626")
            .unwrap()
            .iter()
            .all(|l| !l.ticked)
    );
    set_tick(&v, m[0].line_id, false, now).unwrap();
    assert!(
        ledger_of(&v, 2026, "572")
            .unwrap()
            .iter()
            .all(|l| !l.ticked)
    );
    assert!(set_tick(&v, 999_999, true, now).is_err());
    // Se puntea también con el ejercicio cerrado.
    close_year(&v, 2026, now, true).unwrap();
    set_tick(&v, m[1].line_id, true, now).unwrap();
}

#[test]
fn listados_en_excel() {
    use calamine::{Data, Reader, open_workbook_auto_from_rs};
    use taxpain_app::listings::workbook;
    use taxpain_core::accounting::statements::ModelKind;
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    manual_entry(
        &v,
        &JournalEntry {
            date: date!(2026 - 03 - 01),
            concept: "Aportación".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("572"), Money::from_euros(3000, 0)),
                EntryLine::credit(acc("100"), Money::from_euros(3000, 0)),
            ],
        },
        now,
    )
    .unwrap();
    let (name, bytes) = workbook(&v, 2026, ModelKind::Pymes, date!(2026 - 10 - 13)).unwrap();
    assert_eq!(name, "Contabilidad 2026 12345678Z.xlsx");
    let mut wb = open_workbook_auto_from_rs(std::io::Cursor::new(bytes)).unwrap();
    assert_eq!(
        wb.sheet_names(),
        [
            "Diario",
            "Mayor",
            "Sumas y saldos",
            "Balance",
            "PyG",
            "Cartera"
        ]
    );
    let d = wb.worksheet_range("Diario").unwrap();
    assert_eq!(d.get_value((3, 0)), Some(&Data::String("Asiento".into())));
    assert_eq!(d.get_value((4, 2)), Some(&Data::String("572".into())));
    assert_eq!(d.get_value((4, 5)), Some(&Data::Float(3000.0)));
    let m = wb.worksheet_range("Mayor").unwrap();
    assert_eq!(
        m.get_value((5, 7)),
        Some(&Data::Float(3000.0)),
        "saldo de la 572"
    );
}

#[test]
fn cuentas_por_defecto_y_por_tercero() {
    use taxpain_app::accounting::{posting_accounts, save_posting_accounts, set_party_counterpart};
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    let revenue_of = |number: usize| -> String {
        let j = journal(&v, 2026).unwrap();
        j[number]
            .entry
            .lines
            .iter()
            .find(|l| l.account.as_str().starts_with('7'))
            .unwrap()
            .account
            .as_str()
            .to_owned()
    };

    // Cuentas por defecto: grupo correcto y, con terceros ya creados, sin cambiar dígitos.
    let mut a = posting_accounts(v.connection()).unwrap();
    a.revenue = acc("600");
    assert!(save_posting_accounts(&v, &a).is_err());
    a.revenue = acc("700");
    a.subaccount_width = 8;
    save_posting_accounts(&v, &a).unwrap();
    issue(
        &v,
        &draft("B12345674", "Cliente SL", 100, date!(2026 - 03 - 01)),
        now,
        true,
    )
    .unwrap();
    assert_eq!(revenue_of(0), "700");
    let customer = &chart(&v, 2026)
        .unwrap()
        .into_iter()
        .find(|c| c.source == "tercero")
        .unwrap()
        .code;
    assert_eq!(customer, "43000001");
    a.subaccount_width = 7;
    assert!(save_posting_accounts(&v, &a).is_err());

    // Cuenta propia del cliente.
    assert!(set_party_counterpart(&v, customer, Some("629"), now).is_err());
    assert!(set_party_counterpart(&v, "5720001", Some("705"), now).is_err());
    set_party_counterpart(&v, customer, Some("7050001"), now).unwrap();
    issue(
        &v,
        &draft("B12345674", "Cliente SL", 100, date!(2026 - 03 - 02)),
        now,
        true,
    )
    .unwrap();
    assert_eq!(revenue_of(1), "7050001");
    set_party_counterpart(&v, customer, None, now).unwrap();
    issue(
        &v,
        &draft("B12345674", "Cliente SL", 100, date!(2026 - 03 - 03)),
        now,
        true,
    )
    .unwrap();
    assert_eq!(revenue_of(2), "700");
}

#[test]
fn notas_y_adjuntos_de_los_asientos() {
    use taxpain_app::accounting::{
        attach_document, detach_document, entry_document, entry_documents, set_entry_notes,
    };
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    let entry = |concept: &str| JournalEntry {
        date: date!(2026 - 03 - 01),
        concept: concept.into(),
        origin: Origin::Manual,
        lines: vec![
            EntryLine::debit(acc("629"), Money::from_euros(50, 0)),
            EntryLine::credit(acc("570"), Money::from_euros(50, 0)),
        ],
    };
    let id = manual_entry(&v, &entry("Taxi"), now).unwrap();
    set_entry_notes(&v, id, "  Visita al cliente de Alicante ", now).unwrap();
    let doc = attach_document(&v, id, "/tmp/x/ticket.pdf", b"%PDF-1.4 ticket", now).unwrap();
    assert!(attach_document(&v, id, "ticket.pdf", b"%PDF-1.4 ticket", now).is_err());
    assert!(attach_document(&v, id, "vacio.pdf", b"", now).is_err());
    let j = journal(&v, 2026).unwrap();
    assert_eq!(j[0].notes.as_deref(), Some("Visita al cliente de Alicante"));
    assert_eq!(j[0].attachments, 1);
    let docs = entry_documents(&v, id).unwrap();
    assert_eq!(
        (docs[0].filename.as_str(), docs[0].size),
        ("ticket.pdf", 15)
    );
    let (name, mime, bytes) = entry_document(&v, doc).unwrap();
    assert_eq!(
        (name.as_str(), mime.as_str()),
        ("ticket.pdf", "application/pdf")
    );
    assert_eq!(bytes, b"%PDF-1.4 ticket");
    detach_document(&v, id, doc, now).unwrap();
    assert!(entry_document(&v, doc).is_err());

    // Al borrar el asiento se van sus adjuntos.
    let doc2 = attach_document(&v, id, "otro.png", b"\x89PNG....", now).unwrap();
    delete_manual_entry(&v, id, now, true).unwrap();
    let left: i64 = v
        .connection()
        .query_row("SELECT COUNT(*) FROM document WHERE id = ?1", [doc2], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(left, 0);

    // Ejercicio cerrado: ni notas ni adjuntos.
    let id = manual_entry(&v, &entry("Otro"), now).unwrap();
    close_year(&v, 2026, now, true).unwrap();
    assert!(set_entry_notes(&v, id, "tarde", now).is_err());
    assert!(attach_document(&v, id, "t.pdf", b"%PDF", now).is_err());
}

#[test]
fn baja_y_venta_del_inmovilizado() {
    use taxpain_app::accounting::{add_fixed_asset, dispose_asset, post_depreciation};
    use taxpain_core::accounting::depreciation::FixedAsset;
    let v = setup();
    let now = datetime!(2027-07-01 10:00 UTC);
    let asset = |name: &str| FixedAsset {
        id: 0,
        name: name.into(),
        account: acc("217"),
        depreciation_account: acc("2817"),
        expense_account: acc("681"),
        acquired_on: date!(2026 - 07 - 01),
        cost: Money::from_euros(1200, 0),
        residual: Money::ZERO,
        rate: Rate::percent(25),
        disposed_on: None,
        method: Default::default(),
        tax_rate: None,
    };
    let sold = add_fixed_asset(&v, &asset("Portátil"), now).unwrap();
    let scrapped = add_fixed_asset(&v, &asset("Impresora"), now).unwrap();
    post_depreciation(&v, 2026, now).unwrap();
    assert!(dispose_asset(&v, sold, date!(2026 - 01 - 01), Money::ZERO, now).is_err());

    // Venta el 30/6/2027 por 1.000: amortización de 2027 hasta ese día (148,77; 151,23 en 2026,
    // 300 en total), valor neto 900 y beneficio de 100 en la 771.
    let d = dispose_asset(
        &v,
        sold,
        date!(2027 - 06 - 30),
        Money::from_euros(1000, 0),
        now,
    )
    .unwrap();
    assert_eq!(d.result, Money::from_euros(100, 0));
    assert!(d.depreciation_entry.is_some());
    assert!(dispose_asset(&v, sold, date!(2027 - 06 - 30), Money::ZERO, now).is_err());
    let b = trial_balance(&v, 2027, None).unwrap();
    let saldo = |a: &str| {
        b.iter()
            .find(|x| x.account.as_str() == a)
            .map_or(Money::ZERO, |x| x.balance())
    };
    assert_eq!(saldo("681"), Money::from_cents(14_877));
    assert_eq!(saldo("543"), Money::from_euros(1000, 0));
    assert_eq!(saldo("771"), -Money::from_euros(100, 0));
    // Baja sin precio de la impresora: pérdida por el valor neto (900) en la 671.
    let d = dispose_asset(&v, scrapped, date!(2027 - 06 - 30), Money::ZERO, now).unwrap();
    assert_eq!(d.result, -Money::from_euros(900, 0));
    // Los amortizados ya no se amortizan después.
    assert!(post_depreciation(&v, 2027, now).unwrap().is_empty());
    let b = trial_balance(&v, 2027, None).unwrap();
    let saldo = |a: &str| {
        b.iter()
            .find(|x| x.account.as_str() == a)
            .map_or(Money::ZERO, |x| x.balance())
    };
    assert_eq!(saldo("671"), Money::from_euros(900, 0));
    assert_eq!(saldo("681"), Money::from_cents(14_877 * 2));
    // El coste sale de la 217 (la compra entró en 2026 con su factura).
    assert_eq!(saldo("217"), -Money::from_euros(2400, 0));
}

#[test]
fn reabrir_un_ejercicio_cerrado() {
    use taxpain_app::accounting::reopen_year;
    let v = setup();
    let now = datetime!(2027-01-20 10:00 UTC);
    let e = |d: time::Date, euros: i64| JournalEntry {
        date: d,
        concept: "Venta".into(),
        origin: Origin::Manual,
        lines: vec![
            EntryLine::debit(acc("572"), Money::from_euros(euros, 0)),
            EntryLine::credit(acc("705"), Money::from_euros(euros, 0)),
        ],
    };
    manual_entry(&v, &e(date!(2026 - 05 - 01), 1000), now).unwrap();
    assert!(reopen_year(&v, 2026, now, true).is_err(), "no está cerrado");
    close_year(&v, 2026, now, true).unwrap();
    manual_entry(&v, &e(date!(2027 - 01 - 10), 50), now).unwrap();
    assert!(manual_entry(&v, &e(date!(2026 - 12 - 30), 200), now).is_err());

    assert!(reopen_year(&v, 2026, now, false).is_err());
    assert_eq!(reopen_year(&v, 2026, now, true).unwrap(), 3);
    // Se corrige y se vuelve a cerrar: la apertura de 2027 recoge el resultado nuevo.
    manual_entry(&v, &e(date!(2026 - 12 - 30), 200), now).unwrap();
    let s = close_year(&v, 2026, now, true).unwrap();
    assert_eq!(s.result, Money::from_euros(1200, 0));
    let j27 = journal(&v, 2027).unwrap();
    assert_eq!(
        j27.iter()
            .filter(|s| s.entry.origin == Origin::Apertura)
            .count(),
        1
    );
    assert_eq!(j27.len(), 2);

    // Con 2027 también cerrado, 2026 no se reabre.
    close_year(&v, 2027, now, true).unwrap();
    assert!(reopen_year(&v, 2026, now, true).is_err());
    // Regularización y cierre de 2027 y apertura de 2028.
    assert_eq!(reopen_year(&v, 2027, now, true).unwrap(), 3);
}

#[test]
fn borrar_en_bloque_y_modificar_importados() {
    use taxpain_app::accounting::delete_entries;
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    let e = |origin: Origin, euros: i64| JournalEntry {
        date: date!(2026 - 03 - 01),
        concept: "Gasto".into(),
        origin,
        lines: vec![
            EntryLine::debit(acc("629"), Money::from_euros(euros, 0)),
            EntryLine::credit(acc("570"), Money::from_euros(euros, 0)),
        ],
    };
    let a = manual_entry(&v, &e(Origin::Manual, 10), now).unwrap();
    let b = manual_entry(&v, &e(Origin::Manual, 20), now).unwrap();
    let imported = record_entry(v.connection(), &e(Origin::Importado(1), 30), now).unwrap();
    let invoice_like = record_entry(v.connection(), &e(Origin::Banco(5), 40), now).unwrap();
    // Un importado se modifica y conserva su origen.
    update_manual_entry(&v, imported, &e(Origin::Manual, 35), now).unwrap();
    let j = journal(&v, 2026).unwrap();
    let i = j.iter().find(|s| s.id == imported).unwrap();
    assert_eq!(i.entry.origin, Origin::Importado(1));
    assert_eq!(i.entry.lines[0].debit, Money::from_euros(35, 0));
    // En bloque: todo o nada; los del banco no.
    assert!(delete_entries(&v, &[a, invoice_like], now, true).is_err());
    assert_eq!(journal(&v, 2026).unwrap().len(), 4);
    assert!(delete_entries(&v, &[a, b], now, false).is_err());
    assert_eq!(delete_entries(&v, &[a, b, imported], now, true).unwrap(), 3);
    assert_eq!(journal(&v, 2026).unwrap().len(), 1);
}

#[test]
fn copiar_el_plan_de_otra_empresa() {
    use taxpain_app::accounting::{copy_chart, posting_accounts, save_posting_accounts};
    let from = setup();
    let to = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    set_account(&from, "6290001", "Gastos de oficina", now).unwrap();
    set_account(&from, "5720002", "Banco Sabadell", now).unwrap();
    set_account(&to, "5720002", "Mi banco", now).unwrap();
    let mut a = posting_accounts(from.connection()).unwrap();
    a.revenue = acc("700");
    a.subaccount_width = 8;
    save_posting_accounts(&from, &a).unwrap();
    let c = copy_chart(&from, &to, now).unwrap();
    assert_eq!(c.accounts, 1, "la que ya tenía nombre no se pisa");
    assert!(c.posting_accounts);
    assert_eq!(
        account_name(to.connection(), "5720002").unwrap().as_deref(),
        Some("Mi banco")
    );
    assert_eq!(
        account_name(to.connection(), "6290001").unwrap().as_deref(),
        Some("Gastos de oficina")
    );
    assert_eq!(
        posting_accounts(to.connection()).unwrap().subaccount_width,
        8
    );
}

#[test]
fn traspaso_de_saldo_entre_cuentas() {
    use taxpain_app::accounting::transfer_balance;
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    manual_entry(
        &v,
        &JournalEntry {
            date: date!(2026 - 03 - 01),
            concept: "Gasto mal imputado".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("6290009"), Money::from_euros(80, 0)),
                EntryLine::credit(acc("570"), Money::from_euros(80, 0)),
            ],
        },
        now,
    )
    .unwrap();
    assert!(transfer_balance(&v, "629", "629", date!(2026 - 03 - 31), now).is_err());
    assert!(
        transfer_balance(&v, "6280001", "629", date!(2026 - 03 - 31), now)
            .unwrap()
            .is_none()
    );
    transfer_balance(&v, "6290009", "6290001", date!(2026 - 03 - 31), now)
        .unwrap()
        .unwrap();
    let b = trial_balance(&v, 2026, None).unwrap();
    let saldo = |a: &str| {
        b.iter()
            .find(|x| x.account.as_str() == a)
            .map_or(Money::ZERO, |x| x.balance())
    };
    assert_eq!(saldo("6290009"), Money::ZERO);
    assert_eq!(saldo("6290001"), Money::from_euros(80, 0));
}

#[test]
fn leasing_alta_y_cuotas() {
    use taxpain_app::accounting::fixed_assets;
    use taxpain_app::leasing::{create, list, post_due, set_lessor};
    use taxpain_core::accounting::leasing::{Leasing, Lessor};
    let v = setup();
    let now = datetime!(2026-05-02 10:00 UTC);
    let l = Leasing {
        id: 0,
        name: "Furgoneta".into(),
        start: date!(2026 - 01 - 15),
        principal: Money::from_euros(30_000, 0),
        months: 48,
        rate: Rate::from_bp(500),
        purchase_option: Money::from_euros(600, 0),
        vat: Rate::percent(21),
        asset_account: acc("218"),
        debt_account: acc("5240001"),
        lessor: None,
    };
    assert!(
        create(
            &v,
            &Leasing {
                months: 0,
                ..l.clone()
            },
            Rate::percent(16),
            now
        )
        .is_err()
    );
    let id = create(&v, &l, Rate::percent(16), now).unwrap();
    assert_eq!(
        fixed_assets(&v).unwrap()[0].cost,
        Money::from_euros(30_000, 0)
    );
    // Sin arrendador no hay a quién registrar las facturas de las cuotas.
    assert!(post_due(&v, id, date!(2026 - 04 - 30), now).is_err());
    let lessor = Lessor {
        nif: "B98765431".into(),
        name: "EMPRESA DEMO SL".into(),
        deductible_pct: 100,
    };
    assert!(
        set_lessor(
            &v,
            id,
            &Lessor {
                nif: "X".into(),
                ..lessor.clone()
            },
            now
        )
        .is_err()
    );
    set_lessor(&v, id, &lessor, now).unwrap();
    assert_eq!(
        post_due(&v, id, date!(2026 - 04 - 30), now).unwrap().posted,
        3
    );
    assert_eq!(
        post_due(&v, id, date!(2026 - 04 - 30), now).unwrap().posted,
        0
    );
    // Una cuota futura no se registra antes de tiempo.
    assert_eq!(
        post_due(&v, id, date!(2026 - 12 - 31), now).unwrap().posted,
        0
    );
    let view = &list(&v).unwrap()[0];
    assert_eq!(view.installments.iter().filter(|q| q.posted).count(), 3);
    let paid_capital: Money = view.installments[..3].iter().map(|q| q.q.capital).sum();
    let interest: Money = view.installments[..3].iter().map(|q| q.q.interest).sum();
    let b = trial_balance(&v, 2026, None).unwrap();
    let saldo = |a: &str| {
        b.iter()
            .find(|x| x.account.as_str() == a)
            .map_or(Money::ZERO, |x| x.balance())
    };
    assert_eq!(
        saldo("5240001"),
        -(Money::from_euros(30_000, 0) - paid_capital)
    );
    assert_eq!(saldo("662"), interest);
    assert_eq!(saldo("218"), Money::from_euros(30_000, 0));
    // Cada cuota es una factura recibida del arrendador: su IVA está en la 472 y en el 303 del
    // trimestre en que se registra, y la cuota con IVA queda pendiente de pago al arrendador.
    let vat: Money = view.installments[..3].iter().map(|q| q.q.vat).sum();
    let total: Money = view.installments[..3]
        .iter()
        .map(|q| q.q.amount + q.q.vat)
        .sum();
    assert_eq!(saldo("472"), vat);
    let d = taxpain_app::models::draft(&v, "303", 2026, 2).unwrap();
    let get = |b: u16| {
        d.boxes
            .iter()
            .find(|(k, _)| *k == b)
            .map_or(Money::ZERO, |(_, m)| *m)
    };
    assert_eq!(get(29), vat);
    assert_eq!(get(28), paid_capital + interest);
    let received = taxpain_app::received::list_received(&v, Some("revisada")).unwrap();
    assert_eq!(received.len(), 3);
    let supplier: Money = b
        .iter()
        .filter(|x| x.account.as_str().starts_with("40") || x.account.as_str().starts_with("41"))
        .map(|x| x.balance())
        .sum();
    assert_eq!(supplier, -total, "las cuotas están por pagar, una sola vez");
    let pending = taxpain_app::cartera::list(&v, date!(2026 - 05 - 02)).unwrap();
    assert_eq!(
        pending.iter().filter(|i| i.kind == "recibida").count(),
        3,
        "las tres cuotas están en la cartera"
    );
    // Cada una vence el día de su cuota (no a 30 días): el cargo propone la que toca.
    for q in &view.installments[..3] {
        let due: Vec<_> = pending
            .iter()
            .filter(|i| i.kind == "recibida" && i.number.ends_with(&format!("cuota {}/48", q.q.n)))
            .map(|i| (i.due_date, i.amount))
            .collect();
        assert_eq!(due, vec![(q.q.date, q.q.amount + q.q.vat)]);
    }
    // El cargo de la primera cuota en el banco se concilia con su factura.
    let first = &view.installments[0].q;
    let conn = v.connection();
    conn.execute(
        "INSERT INTO bank_account(number, account, name) VALUES ('00490001', '5720001', 'Banco')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO bank_tx(bank_account, op_date, value_date, amount, description, import_hash, created_at)
         VALUES (1, '2026-02-16', '2026-02-16', ?1, 'RECIBO LEASING', 'h1', '2026-05-02')",
        [-(first.amount + first.vat).cents()],
    )
    .unwrap();
    let sug = taxpain_app::bank::suggestions(&v, 1).unwrap();
    let s1 = sug
        .iter()
        .find(|s| s.kind == "recibida" && s.number.ends_with("cuota 1/48"))
        .expect("el cargo propone la factura de la primera cuota");
    taxpain_app::bank::reconcile_invoice(&v, 1, "recibida", s1.id, None, now).unwrap();
    let b = trial_balance(&v, 2026, None).unwrap();
    let saldo = |a: &str| {
        b.iter()
            .find(|x| x.account.as_str() == a)
            .map_or(Money::ZERO, |x| x.balance())
    };
    assert_eq!(saldo("5720001"), -(first.amount + first.vat));
    assert_eq!(
        saldo("5240001"),
        -(Money::from_euros(30_000, 0) - paid_capital),
        "pagar la cuota no vuelve a tocar la deuda del leasing"
    );
    // Al cierre de 2026, el capital que vence después de 2027 pasa a la 174; repetir no hace nada.
    use taxpain_app::leasing::reclassify;
    assert_eq!(reclassify(&v, 2026, now).unwrap(), 1);
    assert_eq!(reclassify(&v, 2026, now).unwrap(), 0);
    let lt: Money = view
        .installments
        .iter()
        .filter(|q| q.q.date > date!(2027 - 12 - 31))
        .map(|q| q.q.capital)
        .sum();
    let b = trial_balance(&v, 2026, None).unwrap();
    let saldo = |a: &str| {
        b.iter()
            .find(|x| x.account.as_str() == a)
            .map_or(Money::ZERO, |x| x.balance())
    };
    assert_eq!(saldo("1740001"), -lt);
    assert_eq!(
        saldo("5240001"),
        -(Money::from_euros(30_000, 0) - paid_capital - lt)
    );
}

fn furgoneta(start: time::Date) -> taxpain_core::accounting::leasing::Leasing {
    taxpain_core::accounting::leasing::Leasing {
        id: 0,
        name: "Furgoneta".into(),
        start,
        principal: Money::from_euros(30_000, 0),
        months: 48,
        rate: Rate::from_bp(500),
        purchase_option: Money::from_euros(600, 0),
        vat: Rate::percent(21),
        asset_account: acc("218"),
        debt_account: acc("5240001"),
        lessor: None,
    }
}

fn saldo(v: &Vault, year: i32, account: &str) -> Money {
    trial_balance(v, year, None)
        .unwrap()
        .iter()
        .find(|b| b.account.as_str() == account)
        .map_or(Money::ZERO, |b| b.balance())
}

/// Con el ejercicio anterior cerrado, la apertura trae la 174 y la reclasificación del nuevo
/// ejercicio parte de ese saldo (no de la suma de todos los ejercicios).
#[test]
fn leasing_174_tras_cerrar_un_ejercicio() {
    use taxpain_app::leasing::{create, list, reclassify};
    use taxpain_core::accounting::leasing::long_term_at;
    let v = setup();
    let now = datetime!(2025-02-01 10:00 UTC);
    create(
        &v,
        &furgoneta(date!(2025 - 01 - 15)),
        Rate::percent(16),
        now,
    )
    .unwrap();
    let l = list(&v).unwrap()[0].leasing.clone();
    assert_eq!(reclassify(&v, 2025, now).unwrap(), 1);
    let lt25 = long_term_at(&l, date!(2025 - 12 - 31));
    assert_eq!(saldo(&v, 2025, "1740001"), -lt25);
    close_year(&v, 2025, now, true).unwrap();
    assert_eq!(
        saldo(&v, 2026, "1740001"),
        -lt25,
        "la apertura de 2026 trae la 174"
    );
    let lt26 = long_term_at(&l, date!(2026 - 12 - 31));
    assert!(lt26 < lt25);
    assert_eq!(reclassify(&v, 2026, now).unwrap(), 1);
    assert_eq!(saldo(&v, 2026, "1740001"), -lt26);
    assert_eq!(
        saldo(&v, 2026, "5240001"),
        -(Money::from_euros(30_000, 0) - lt26),
        "sin cuotas pagadas, el resto del principal queda a corto"
    );
    assert_eq!(reclassify(&v, 2026, now).unwrap(), 0);
}

/// Dos contratos en la misma 5240001: la 174 queda con la suma de sus largos plazos.
#[test]
fn leasing_dos_contratos_en_la_misma_cuenta() {
    use taxpain_app::leasing::{create, list, reclassify};
    use taxpain_core::accounting::leasing::{Leasing, long_term_at};
    let v = setup();
    let now = datetime!(2026-05-02 10:00 UTC);
    create(
        &v,
        &furgoneta(date!(2026 - 01 - 15)),
        Rate::percent(16),
        now,
    )
    .unwrap();
    create(
        &v,
        &Leasing {
            name: "Equipo".into(),
            principal: Money::from_euros(12_000, 0),
            months: 36,
            purchase_option: Money::from_euros(100, 0),
            asset_account: acc("217"),
            ..furgoneta(date!(2026 - 03 - 01))
        },
        Rate::percent(25),
        now,
    )
    .unwrap();
    let views = list(&v).unwrap();
    let total: Money = views
        .iter()
        .map(|x| long_term_at(&x.leasing, date!(2026 - 12 - 31)))
        .sum();
    assert_eq!(
        reclassify(&v, 2026, now).unwrap(),
        1,
        "un asiento por cuenta"
    );
    assert_eq!(saldo(&v, 2026, "1740001"), -total);
    assert_eq!(
        saldo(&v, 2026, "5240001"),
        -(Money::from_euros(42_000, 0) - total)
    );
    assert_eq!(reclassify(&v, 2026, now).unwrap(), 0);
}

/// Los ejercicios se cierran en orden: cerrar 2026 con 2025 abierto dejaría 2026 sin apertura y
/// después la apertura de 2026 no podría entrar (ejercicio cerrado).
#[test]
fn cerrar_un_ejercicio_exige_cerrar_antes_el_anterior() {
    let v = setup();
    let now = datetime!(2027-01-15 10:00 UTC);
    let aportacion = |d: time::Date, euros: i64| JournalEntry {
        date: d,
        concept: "Aportación de capital".into(),
        origin: Origin::Manual,
        lines: vec![
            EntryLine::debit(acc("5720001"), Money::from_euros(euros, 0)),
            EntryLine::credit(acc("100"), Money::from_euros(euros, 0)),
        ],
    };
    manual_entry(&v, &aportacion(date!(2025 - 03 - 01), 3_000), now).unwrap();
    manual_entry(&v, &aportacion(date!(2026 - 03 - 01), 1_000), now).unwrap();
    let err = close_year(&v, 2026, now, true).unwrap_err().to_string();
    assert!(err.contains("2025"), "{err}");
    close_year(&v, 2025, now, true).unwrap();
    close_year(&v, 2026, now, true).unwrap();
    assert_eq!(
        saldo(&v, 2027, "5720001"),
        Money::from_euros(4_000, 0),
        "la apertura de 2027 trae los saldos de los dos ejercicios"
    );
}

/// Una fecha mal tecleada (2099, 0226) no crea un ejercicio que después no se puede quitar.
#[test]
fn fecha_mal_tecleada_no_crea_ejercicio() {
    let v = setup();
    let now = datetime!(2026-10-20 10:00 UTC);
    let e = |d: time::Date| JournalEntry {
        date: d,
        concept: "Aportación de capital".into(),
        origin: Origin::Manual,
        lines: vec![
            EntryLine::debit(acc("5720001"), Money::from_euros(100, 0)),
            EntryLine::credit(acc("100"), Money::from_euros(100, 0)),
        ],
    };
    manual_entry(&v, &e(date!(2026 - 10 - 01)), now).unwrap();
    let years = |v: &Vault| -> Vec<i32> {
        let conn = v.connection();
        let mut st = conn
            .prepare("SELECT year FROM fiscal_year ORDER BY year")
            .unwrap();
        st.query_map([], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    for bad in [date!(2099 - 03 - 01), date!(0226 - 03 - 01)] {
        let err = manual_entry(&v, &e(bad), now).unwrap_err().to_string();
        assert!(err.contains("revisa la fecha"), "{err}");
    }
    assert_eq!(years(&v), [2026]);
    // Hasta un año por delante sí (previsiones, asientos del ejercicio siguiente).
    manual_entry(&v, &e(date!(2027 - 06 - 30)), now).unwrap();
    assert_eq!(years(&v), [2026, 2027]);
}

#[test]
fn copiar_asientos_a_otra_empresa() {
    use taxpain_app::accounting::{entries_to_copy, journal, manual_entry, paste_entries};
    let a = setup();
    let b = setup();
    let now = datetime!(2026-05-02 10:00 UTC);
    let e = JournalEntry {
        date: date!(2026 - 03 - 01),
        concept: "Alquiler marzo".into(),
        origin: Origin::Manual,
        lines: vec![
            EntryLine::debit(acc("621"), Money::from_euros(500, 0)),
            EntryLine::credit(acc("5720001"), Money::from_euros(500, 0)),
        ],
    };
    let id = manual_entry(&a, &e, now).unwrap();
    let (list, third) = entries_to_copy(&a, 2026, &[id]).unwrap();
    assert!(third.is_empty());
    assert!(entries_to_copy(&a, 2026, &[id, 9999]).is_err());
    assert_eq!(paste_entries(&b, &list, "Empresa A", now).unwrap(), 1);
    let jb = journal(&b, 2026).unwrap();
    assert_eq!(jb.len(), 1);
    assert_eq!(jb[0].entry.concept, "Alquiler marzo");
}

/// Contrato con arrendador y cuotas contabilizadas con el sistema antiguo (asiento directo con
/// el IVA a la 472 contra la 572, sin factura), como las que dejaba la app antes.
fn leasing_antiguo(v: &Vault, now: time::OffsetDateTime, cuotas: usize) -> i64 {
    use taxpain_app::leasing::{create, list, set_lessor};
    use taxpain_core::accounting::leasing::{Lessor, installment_entry};
    let id = create(v, &furgoneta(date!(2026 - 01 - 15)), Rate::percent(16), now).unwrap();
    set_lessor(
        v,
        id,
        &Lessor {
            nif: "B98765431".into(),
            name: "EMPRESA DEMO SL".into(),
            deductible_pct: 100,
        },
        now,
    )
    .unwrap();
    let view = list(v).unwrap().remove(0);
    for q in &view.installments[..cuotas] {
        let e = installment_entry(&view.leasing, &q.q, &acc("662"), &acc("472"), &acc("572"));
        let conn = v.connection();
        let entry = taxpain_app::accounting::record_entry(conn, &e, now).unwrap();
        conn.execute(
            "INSERT INTO leasing_installment(leasing_id, n, entry_id) VALUES (?1, ?2, ?3)",
            (id, q.q.n, entry),
        )
        .unwrap();
    }
    id
}

/// Las cuotas del sistema antiguo se detectan y, con confirmación, se regularizan: su factura
/// del arrendador entra en el libro y en el 303 del periodo en curso, queda pagada en la cartera
/// y el asiento antiguo desaparece (el gasto, el IVA y la deuda no se duplican). Las de un
/// trimestre con el 303 presentado no se tocan.
#[test]
fn leasing_regulariza_las_cuotas_antiguas() {
    use taxpain_app::error::AppError;
    use taxpain_app::leasing::{list, regularize};
    let v = setup();
    let now = datetime!(2026-05-02 10:00 UTC);
    let id = leasing_antiguo(&v, now, 3);
    let view = list(&v).unwrap().remove(0);
    let legacy: Vec<_> = view.installments.iter().filter(|q| q.legacy).collect();
    assert_eq!(legacy.len(), 3);
    assert!(legacy.iter().all(|q| q.blocked.is_none()));
    let before = trial_balance(&v, 2026, None).unwrap();
    let saldo_antes = |a: &str| {
        before
            .iter()
            .find(|x| x.account.as_str() == a)
            .map_or(Money::ZERO, |x| x.balance())
    };
    // Sin confirmar no hace nada.
    assert!(matches!(
        regularize(&v, id, now, false),
        Err(AppError::NotConfirmed)
    ));
    // El 303 del 1T está presentado: las cuotas de febrero y marzo no se pueden regularizar.
    let mut boxes = std::collections::BTreeMap::new();
    boxes.insert(27u16, Money::ZERO);
    taxpain_app::presentados::save_manual(&v, "303", 2026, "1T", &boxes, now).unwrap();
    let view = list(&v).unwrap().remove(0);
    let blocked: Vec<_> = view
        .installments
        .iter()
        .filter_map(|q| q.blocked.as_deref())
        .collect();
    assert_eq!(blocked.len(), 2);
    assert!(blocked[0].contains("303 del 1T de 2026"), "{blocked:?}");

    let r = regularize(&v, id, now, true).unwrap();
    assert_eq!(r.done, 1);
    assert_eq!(r.blocked.len(), 2);
    assert!(r.blocked[0].starts_with("cuota 1/48"), "{:?}", r.blocked);
    let view = list(&v).unwrap().remove(0);
    let third = &view.installments[2];
    assert!(third.posted && !third.legacy);
    assert_eq!(view.installments.iter().filter(|q| q.legacy).count(), 2);
    // Repetir no hace nada más.
    assert_eq!(regularize(&v, id, now, true).unwrap().done, 0);

    // La factura está en el libro y su IVA en el 303 del 2T (el de su registro).
    let received = taxpain_app::received::list_received(&v, Some("revisada")).unwrap();
    assert_eq!(received.len(), 1);
    let d = taxpain_app::models::draft(&v, "303", 2026, 2).unwrap();
    let vat29 = d
        .boxes
        .iter()
        .find(|(k, _)| *k == 29)
        .map_or(Money::ZERO, |(_, m)| *m);
    assert_eq!(vat29, third.q.vat);
    // Pagada en la cartera el día de la cuota, desde la misma 572.
    let cartera = taxpain_app::cartera::list(&v, date!(2026 - 05 - 02)).unwrap();
    let item = cartera.iter().find(|i| i.kind == "recibida").unwrap();
    assert_eq!(item.status, "registrada");
    assert_eq!(item.settled_on, Some(third.q.date));
    assert_eq!(item.due_date, third.q.date, "vence el día de la cuota");
    // Los saldos no cambian: ni gasto, ni IVA, ni deuda, ni banco duplicados; el arrendador, a 0.
    let after = trial_balance(&v, 2026, None).unwrap();
    let saldo = |a: &str| {
        after
            .iter()
            .find(|x| x.account.as_str() == a)
            .map_or(Money::ZERO, |x| x.balance())
    };
    for a in ["472", "662", "5240001", "572", "218"] {
        assert_eq!(saldo(a), saldo_antes(a), "saldo de la {a}");
    }
    let supplier: Money = after
        .iter()
        .filter(|x| x.account.as_str().starts_with("40") || x.account.as_str().starts_with("41"))
        .map(|x| x.balance())
        .sum();
    assert_eq!(supplier, Money::ZERO);
    let old: i64 = v
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM journal_entry WHERE origin_kind = 'Leasing' AND concept LIKE '%cuota 3/48'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(old, 0, "el asiento antiguo de la cuota 3 se ha borrado");
    let audited: i64 = v
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE action = 'leasing.regularize'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(audited, 1);

    // La factura real de una cuota antigua no se registra en Recibidas: se regulariza.
    let q1 = &view.installments[0].q;
    let err = taxpain_app::received::register_typed(
        &v,
        &Reviewed {
            supplier_nif: "B98765431".into(),
            supplier_name: "EMPRESA DEMO SL".into(),
            number: "L-2026-0001".into(),
            issue_date: q1.date,
            base: q1.amount,
            vat: q1.vat,
            withholding: Money::ZERO,
            total: q1.amount + q1.vat,
            deductible_pct: 100,
            capital_good: false,
            nature: Nature::General,
            asset_class: None,
            vat_regime: Default::default(),
            supplier_country: None,
            self_assessed_rate: None,
            intra_goods: false,
            cash_basis: false,
            import_dua: false,
            prorata_use: Default::default(),
        },
        now,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("Regularizar"), "{err}");
}

/// La factura real del arrendador de una cuota ya registrada desde Leasing se bloquea (a mano o
/// importada del libro del gestor); otra factura suya (otro importe) se registra.
#[test]
fn leasing_bloquea_la_factura_del_arrendador_duplicada() {
    use taxpain_app::leasing::{create, list, post_due};
    use taxpain_app::received::register_typed;
    use taxpain_core::accounting::leasing::{Leasing, Lessor};
    let v = setup();
    let now = datetime!(2026-05-02 10:00 UTC);
    let l = Leasing {
        lessor: Some(Lessor {
            nif: "b98765431".into(),
            name: "EMPRESA DEMO SL".into(),
            deductible_pct: 100,
        }),
        ..furgoneta(date!(2026 - 01 - 15))
    };
    let id = create(&v, &l, Rate::percent(16), now).unwrap();
    assert_eq!(
        post_due(&v, id, date!(2026 - 04 - 30), now).unwrap().posted,
        3
    );
    let q2 = list(&v).unwrap()[0].installments[1].q.clone();
    let real = |number: &str, date: time::Date, base: Money, vat: Money| Reviewed {
        supplier_nif: "B98765431".into(),
        supplier_name: "EMPRESA DEMO SL".into(),
        number: number.into(),
        issue_date: date,
        base,
        vat,
        withholding: Money::ZERO,
        total: base + vat,
        deductible_pct: 100,
        capital_good: false,
        nature: Nature::General,
        asset_class: None,
        vat_regime: Default::default(),
        supplier_country: None,
        self_assessed_rate: None,
        intra_goods: false,
        cash_basis: false,
        import_dua: false,
        prorata_use: Default::default(),
    };
    // La de la cuota 2, con fecha y céntimos del arrendador: bloqueada.
    let err = register_typed(
        &v,
        &real(
            "L-2026-0002",
            q2.date + time::Duration::days(2),
            q2.amount + Money::from_cents(1),
            q2.vat,
        ),
        now,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("cuota 2/48"), "{err}");
    assert!(
        err.contains("ya está registrada desde Contabilidad → Leasing"),
        "{err}"
    );
    // Tecleada a mano no hay nada que descartar; de un documento, sí.
    assert!(err.ends_with("No hace falta registrarla aquí."), "{err}");
    let rec = ingest_file(
        &v,
        "cuota2.xml",
        b"<?xml version=\"1.0\"?><cuota/>",
        Source::Manual,
        now,
    )
    .unwrap();
    let err = review(
        &v,
        rec.id,
        &real("L-2026-0002", q2.date, q2.amount, q2.vat),
        now,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("Descarta esta"), "{err}");
    // Una factura del arrendador por otra cosa (gastos de gestión) sí entra.
    register_typed(
        &v,
        &real(
            "G-2026-0001",
            q2.date,
            Money::from_euros(50, 0),
            Money::from_euros(10, 50),
        ),
        now,
    )
    .unwrap();
    // Del libro de recibidas del gestor, la cuota no se importa (y lo dice).
    let money = |m: Money| m.to_string().trim_end_matches(" €").to_owned();
    let fecha = format!(
        "{:02}/{:02}/{}",
        q2.date.day(),
        u8::from(q2.date.month()),
        q2.date.year()
    );
    let csv = format!(
        "Fecha;Nº Factura;NIF Proveedor;Proveedor;Base;Cuota soportada;Retención;Total\n\
         {fecha};L-2026-0002;B98765431;EMPRESA DEMO SL;{};{};0,00;{}\n",
        money(q2.amount),
        money(q2.vat),
        money(q2.amount + q2.vat)
    );
    let book = taxpain_app::libros::parse("recibidas.csv", csv.as_bytes())
        .unwrap()
        .unwrap();
    let r = taxpain_app::libros::import_received(&v, &book, "recibidas.csv", csv.as_bytes(), now)
        .unwrap();
    assert_eq!((r.added, r.skipped), (0, 1));
    assert!(
        r.warnings.iter().any(|w| w.contains("no se importa")),
        "{:?}",
        r.warnings
    );
}

/// Un contrato dado de alta por error se borra con confirmación: contrato, cuotas previstas,
/// asiento de alta, paso a la 174 y elemento del inmovilizado. No se borra si tiene cuotas
/// contabilizadas, una factura real del arrendador en Recibidas o su alta en un ejercicio
/// cerrado; en cada caso dice qué deshacer antes.
#[test]
fn leasing_borra_un_contrato_dado_de_alta_por_error() {
    use taxpain_app::accounting::fixed_assets;
    use taxpain_app::error::AppError;
    use taxpain_app::leasing::{create, delete, list, post_due, reclassify};
    use taxpain_app::received::register_typed;
    use taxpain_core::accounting::leasing::{Leasing, Lessor};
    let v = setup();
    let now = datetime!(2026-05-02 10:00 UTC);
    let lessor = Lessor {
        nif: "B98765431".into(),
        name: "EMPRESA DEMO SL".into(),
        deductible_pct: 100,
    };
    let l = Leasing {
        lessor: Some(lessor.clone()),
        ..furgoneta(date!(2026 - 01 - 15))
    };
    let id = create(&v, &l, Rate::percent(16), now).unwrap();
    assert_eq!(reclassify(&v, 2026, now).unwrap(), 1);
    assert_eq!(journal(&v, 2026).unwrap().len(), 2);
    assert!(matches!(
        delete(&v, id, now, false),
        Err(AppError::NotConfirmed)
    ));
    delete(&v, id, now, true).unwrap();
    assert!(list(&v).unwrap().is_empty());
    assert!(fixed_assets(&v).unwrap().is_empty());
    assert!(journal(&v, 2026).unwrap().is_empty(), "alta y 174 borrados");
    assert_eq!(trial_balance(&v, 2026, None).unwrap().len(), 0);
    let audits: i64 = v
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE action = 'leasing.delete'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(audits, 1);
    assert!(matches!(delete(&v, id, now, true), Err(AppError::NotFound)));

    // Con cuotas contabilizadas no se borra, y dice qué deshacer.
    let id = create(&v, &l, Rate::percent(16), now).unwrap();
    assert_eq!(
        post_due(&v, id, date!(2026 - 02 - 28), now).unwrap().posted,
        1
    );
    let err = delete(&v, id, now, true).unwrap_err().to_string();
    assert!(err.contains("1 de 48 cuotas contabilizadas"), "{err}");
    assert!(err.contains("rectificativa"), "{err}");
    assert_eq!(list(&v).unwrap().len(), 1);

    // Con la factura real del arrendador por una cuota en Recibidas, tampoco.
    let l2 = Leasing {
        name: "Carretilla".into(),
        principal: Money::from_euros(12_000, 0),
        ..l.clone()
    };
    let id2 = create(&v, &l2, Rate::percent(16), now).unwrap();
    let q1 = list(&v)
        .unwrap()
        .into_iter()
        .find(|x| x.leasing.id == id2)
        .unwrap()
        .installments[0]
        .q
        .clone();
    register_typed(
        &v,
        &Reviewed {
            supplier_nif: lessor.nif.clone(),
            supplier_name: lessor.name.clone(),
            number: "L-2026-0101".into(),
            issue_date: q1.date,
            base: q1.amount,
            vat: q1.vat,
            withholding: Money::ZERO,
            total: q1.amount + q1.vat,
            deductible_pct: 100,
            capital_good: false,
            nature: Nature::General,
            asset_class: None,
            vat_regime: Default::default(),
            supplier_country: None,
            self_assessed_rate: None,
            intra_goods: false,
            cash_basis: false,
            import_dua: false,
            prorata_use: Default::default(),
        },
        now,
    )
    .unwrap();
    let err = delete(&v, id2, now, true).unwrap_err().to_string();
    assert!(err.contains("«L-2026-0101»"), "{err}");
    assert!(err.contains("cuota 1/48"), "{err}");

    // El alta en un ejercicio cerrado no se toca.
    let v = setup();
    let now = datetime!(2026-02-01 10:00 UTC);
    let id = create(
        &v,
        &furgoneta(date!(2025 - 06 - 15)),
        Rate::percent(16),
        now,
    )
    .unwrap();
    close_year(&v, 2025, now, true).unwrap();
    let err = delete(&v, id, now, true).unwrap_err().to_string();
    assert!(err.contains("ejercicio 2025, que está cerrado"), "{err}");
    assert_eq!(list(&v).unwrap().len(), 1);
}

/// Las cuotas registradas antes del arreglo de octubre de 2026 vencían a +30 días (las
/// condiciones de pago del arrendador). Se detectan y «Corregir vencimientos» pone cada una en
/// su fecha de cuota, sin tocar las ya pagadas o conciliadas.
#[test]
fn leasing_corrige_los_vencimientos_antiguos() {
    use taxpain_app::leasing::{create, fix_due_dates, list, post_due};
    use taxpain_core::accounting::leasing::{Leasing, Lessor};
    let v = setup();
    let now = datetime!(2026-05-02 10:00 UTC);
    let l = Leasing {
        lessor: Some(Lessor {
            nif: "B98765431".into(),
            name: "EMPRESA DEMO SL".into(),
            deductible_pct: 100,
        }),
        ..furgoneta(date!(2026 - 01 - 15))
    };
    let id = create(&v, &l, Rate::percent(16), now).unwrap();
    assert_eq!(
        post_due(&v, id, date!(2026 - 04 - 30), now).unwrap().posted,
        3
    );
    let view = list(&v).unwrap().remove(0);
    assert!(view.installments.iter().all(|q| !q.due_mismatch));
    assert_eq!(fix_due_dates(&v, id, now).unwrap(), 0);
    // Como las dejaba la app antes: vencimiento a 30 días de la factura.
    let conn = v.connection();
    conn.execute(
        "UPDATE invoice_installment SET due_date = date(due_date, '+30 days')
         WHERE kind = 'recibida'",
        [],
    )
    .unwrap();
    let view = list(&v).unwrap().remove(0);
    let wrong: Vec<u16> = view
        .installments
        .iter()
        .filter(|q| q.due_mismatch)
        .map(|q| q.q.n)
        .collect();
    assert_eq!(wrong, vec![1, 2, 3]);
    // La primera ya está pagada desde la cartera: no se toca.
    let rid: i64 = conn
        .query_row(
            "SELECT id FROM invoice_received WHERE number = 'Leasing 1 cuota 1/48'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let q1 = view.installments[0].q.clone();
    taxpain_app::cartera::settle(&v, "recibida", rid, 1, q1.date, "5720001", now).unwrap();
    let view = list(&v).unwrap().remove(0);
    let wrong: Vec<u16> = view
        .installments
        .iter()
        .filter(|q| q.due_mismatch)
        .map(|q| q.q.n)
        .collect();
    assert_eq!(wrong, vec![2, 3]);
    assert_eq!(fix_due_dates(&v, id, now).unwrap(), 2);
    let view = list(&v).unwrap().remove(0);
    assert!(view.installments.iter().all(|q| !q.due_mismatch));
    let due = |n: u16| -> String {
        conn.query_row(
            "SELECT due_date FROM invoice_installment WHERE kind = 'recibida'
             AND invoice_id = (SELECT id FROM invoice_received WHERE number = ?1)",
            [format!("Leasing 1 cuota {n}/48")],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(due(2), view.installments[1].q.date.to_string());
    assert_eq!(due(3), view.installments[2].q.date.to_string());
    assert_eq!(
        due(1),
        (q1.date + time::Duration::days(30)).to_string(),
        "la pagada conserva su vencimiento"
    );
    let audits: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE action = 'leasing.due_dates'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(audits, 1);
}

/// El duplicado inverso: la factura real del arrendador registrada en Recibidas antes de
/// «Registrar cuotas». Al registrar, la cuota se enlaza con ella (por importe y fecha a ±5 días,
/// o por el número) en vez de crear otra recibida; su gasto pasa a capital (524) e intereses
/// (662) y vence el día de la cuota.
#[test]
fn leasing_enlaza_la_factura_real_registrada_antes() {
    use taxpain_app::leasing::{create, list, post_due};
    use taxpain_app::received::register_typed;
    use taxpain_core::accounting::leasing::{Leasing, Lessor};
    let v = setup();
    let now = datetime!(2026-05-02 10:00 UTC);
    let l = Leasing {
        lessor: Some(Lessor {
            nif: "B98765431".into(),
            name: "EMPRESA DEMO SL".into(),
            deductible_pct: 100,
        }),
        ..furgoneta(date!(2026 - 01 - 15))
    };
    let id = create(&v, &l, Rate::percent(16), now).unwrap();
    let qs: Vec<_> = list(&v).unwrap().remove(0).installments[..3]
        .iter()
        .map(|q| q.q.clone())
        .collect();
    let real = |number: &str, date: time::Date, base: Money, vat: Money| Reviewed {
        supplier_nif: "B98765431".into(),
        supplier_name: "EMPRESA DEMO SL".into(),
        number: number.into(),
        issue_date: date,
        base,
        vat,
        withholding: Money::ZERO,
        total: base + vat,
        deductible_pct: 100,
        capital_good: false,
        nature: Nature::General,
        asset_class: None,
        vat_regime: Default::default(),
        supplier_country: None,
        self_assessed_rate: None,
        intra_goods: false,
        cash_basis: false,
        import_dua: false,
        prorata_use: Default::default(),
    };
    // La de la cuota 1 con la fecha y los céntimos del arrendador; la de la 2 con el número
    // con que la registraría el leasing; una tercera suya por otra cosa (gastos).
    register_typed(
        &v,
        &real(
            "L-2026-0001",
            qs[0].date + time::Duration::days(3),
            qs[0].amount + Money::from_cents(1),
            qs[0].vat,
        ),
        now,
    )
    .unwrap();
    register_typed(
        &v,
        &real(
            &format!("Leasing {id} cuota 2/48"),
            qs[1].date + time::Duration::days(20),
            qs[1].amount,
            qs[1].vat,
        ),
        now,
    )
    .unwrap();
    register_typed(
        &v,
        &real(
            "G-2026-0001",
            qs[2].date,
            Money::from_euros(50, 0),
            Money::from_euros(10, 50),
        ),
        now,
    )
    .unwrap();
    let out = post_due(&v, id, date!(2026 - 04 - 30), now).unwrap();
    assert_eq!(out.posted, 1, "sólo la cuota 3 es factura nueva");
    assert_eq!(out.linked.len(), 2, "{:?}", out.linked);
    assert!(out.linked[0].starts_with("cuota 1/48: enlazada con la factura «L-2026-0001»"));
    assert!(
        out.linked[0].contains("capital a la 5240001"),
        "{}",
        out.linked[0]
    );
    assert!(
        out.linked[0].contains("vence el día de la cuota"),
        "{}",
        out.linked[0]
    );
    assert!(
        out.linked[1].starts_with("cuota 2/48: enlazada"),
        "{}",
        out.linked[1]
    );
    let view = list(&v).unwrap().remove(0);
    assert!(
        view.installments[..3]
            .iter()
            .all(|q| q.posted && !q.legacy && !q.due_mismatch)
    );
    assert!(!view.installments[3].posted);
    // Sin recibidas de más: las dos reales, la de gastos y la nueva de la cuota 3.
    let received = taxpain_app::received::list_received(&v, Some("revisada")).unwrap();
    assert_eq!(received.len(), 4);
    // El asiento de la factura real lleva ahora el capital a la 524 y el resto a la 662, cuadrado.
    let b = trial_balance(&v, 2026, None).unwrap();
    let saldo = |a: &str| {
        b.iter()
            .find(|x| x.account.as_str() == a)
            .map_or(Money::ZERO, |x| x.balance())
    };
    let capital: Money = qs.iter().map(|q| q.capital).sum();
    assert_eq!(saldo("5240001"), -(Money::from_euros(30_000, 0) - capital));
    let interest: Money = qs.iter().map(|q| q.interest).sum();
    assert_eq!(saldo("662"), interest + Money::from_cents(1));
    assert!(
        b.iter()
            .all(|x| !x.account.as_str().starts_with("62")
                || x.balance() == Money::from_euros(50, 0)),
        "el gasto de las cuotas ya no está en el grupo 62"
    );
    // Y vuelve a registrar las cuotas: nada que hacer.
    let again = post_due(&v, id, date!(2026 - 04 - 30), now).unwrap();
    assert_eq!((again.posted, again.linked.len()), (0, 0));
}

/// Fecha del asiento con el que se anula la factura `id`.
fn reversal_date(v: &Vault, id: i64) -> Option<String> {
    use taxpain_vault::rusqlite::OptionalExtension;
    v.connection()
        .query_row(
            "SELECT date FROM journal_entry WHERE origin_kind = 'AnulacionFactura' AND origin_id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()
        .unwrap()
}

/// La anulación de una factura de un trimestre anterior se contabiliza en la fecha de la
/// factura (el 303 la saca de su periodo de origen); si ese IVA ya está liquidado en la
/// contabilidad o el ejercicio está cerrado, no se anula: se rectifica.
#[test]
fn la_anulacion_va_en_el_periodo_de_la_factura() {
    use taxpain_app::accounting::settle_vat_quarter;
    let v = setup();
    let sept = datetime!(2026-09-30 10:00 UTC);
    let oct = datetime!(2026-10-05 10:00 UTC);
    // Factura del 3T anulada en octubre, antes del plazo del 303 y sin liquidar: el
    // contraasiento va al 30/09 y el 3T queda sin IVA de esa factura.
    let a = issue(
        &v,
        &draft("B12345674", "Cliente", 1000, date!(2026 - 09 - 30)),
        sept,
        true,
    )
    .unwrap();
    cancel(&v, a.id, oct, true).unwrap();
    assert_eq!(reversal_date(&v, a.id).as_deref(), Some("2026-09-30"));
    let tb = trial_balance(&v, 2026, None).unwrap();
    let bal = |c: &str| {
        tb.iter()
            .find(|b| b.account.as_str() == c)
            .map_or(Money::ZERO, |b| b.balance())
    };
    assert_eq!(bal("477"), Money::ZERO);

    // Con la liquidación del 3T contabilizada: hay que emitir una rectificativa.
    let b = issue(
        &v,
        &draft("B12345674", "Cliente", 500, date!(2026 - 09 - 30)),
        sept,
        true,
    )
    .unwrap();
    settle_vat_quarter(&v, 2026, 3, oct).unwrap().unwrap();
    let err = cancel(&v, b.id, oct, true).unwrap_err().to_string();
    assert!(
        err.contains("liquidación del IVA del 3T de 2026") && err.contains("rectificativa"),
        "{err}"
    );
    assert_eq!(reversal_date(&v, b.id), None);
}

#[test]
fn no_se_anula_una_factura_de_un_ejercicio_cerrado() {
    let v = setup();
    let dic = datetime!(2025-12-20 10:00 UTC);
    let ene = datetime!(2026-01-10 10:00 UTC);
    let a = issue(
        &v,
        &draft("B12345674", "Cliente", 1000, date!(2025 - 12 - 20)),
        dic,
        true,
    )
    .unwrap();
    close_year(&v, 2025, ene, true).unwrap();
    // El 303 del 4T de 2025 aún está en plazo, pero el ejercicio está cerrado.
    let err = cancel(&v, a.id, ene, true).unwrap_err().to_string();
    assert!(
        err.contains("ejercicio 2025 está cerrado") && err.contains("rectificativa"),
        "{err}"
    );
    assert_eq!(reversal_date(&v, a.id), None);
}
