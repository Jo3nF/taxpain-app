//! Remesas SEPA: recibos a clientes y transferencias a proveedores, banco y devoluciones.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::trial_balance;
use taxpain_app::bank::{import_n43, movements, reconcile_remittance, suggestions};
use taxpain_app::cartera::{PartyTerms, list, set_party_terms, unsettle};
use taxpain_app::invoicing::{self, InvoiceDraft, issue};
use taxpain_app::received::{AssetClass, Nature, Reviewed, Source, ingest_file, review};
use taxpain_app::remesas::{self, candidates, create, return_item, undo};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::accounting::Origin;
use taxpain_core::invoice::{InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

const N43: &[u8] = include_bytes!("../../../fixtures/accounting/n43-ejemplo.txt");

fn setup() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Ejemplo Diseño SL".into(),
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

fn draft(nif: &str, name: &str, euros: i64) -> InvoiceDraft {
    InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: date!(2026 - 09 - 01),
        operation_date: None,
        recipient: Some(Party {
            id: PartyId::Nif(nif.into()),
            name: name.into(),
            address: addr(),
            person: None,
        }),
        lines: vec![InvoiceLine {
            description: "Servicios".into(),
            quantity_milli: 1000,
            unit_price: Money::from_euros(euros, 0),
            discount: Rate::ZERO,
            vat: VatTreatment::Taxed {
                rate: Rate::percent(21),
            },
            surcharge: None,
            withholding: None,
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

fn recibo(iban: Option<&str>, mandate: bool) -> PartyTerms {
    PartyTerms {
        days: vec![30],
        pay_day: None,
        method: "recibo".into(),
        treasury: None,
        iban: iban.map(Into::into),
        mandate_ref: mandate.then(|| "MAND-0001".into()),
        mandate_date: mandate.then_some(date!(2026 - 01 - 15)),
        key_190: None,
    }
}

#[test]
fn remesa_de_recibos_banco_y_devolucion() {
    let v = setup();
    let now = datetime!(2026-09-25 10:00 UTC);
    let a = issue(&v, &draft("12345678Z", "Cliente Uno SL", 1000), now, true).unwrap();
    let b = issue(&v, &draft("A22222228", "Cliente Dos SA", 50), now, true).unwrap();
    // Sin condiciones no son recibos: no salen.
    assert!(candidates(&v, "cobros", now.date()).unwrap().is_empty());
    assert!(set_party_terms(&v, "4300001", Some(&recibo(Some("ES00 1234"), true)), now).is_err());
    set_party_terms(
        &v,
        "4300001",
        Some(&recibo(Some("es79 2100 0813 6101 2345 6789"), true)),
        now,
    )
    .unwrap();
    set_party_terms(
        &v,
        "4300002",
        Some(&recibo(Some("ES9121000418450200051332"), false)),
        now,
    )
    .unwrap();
    let c = candidates(&v, "cobros", now.date()).unwrap();
    assert_eq!(c.len(), 2);
    assert_eq!(c[0].iban.as_deref(), Some("ES7921000813610123456789"));
    assert!(c.iter().any(|x| x.item.invoice_id == b.id
        && x.problem.as_deref().is_some_and(|p| p.contains("mandato"))));

    let iban = "ES9121000418450200051332";
    assert!(
        create(&v, "cobros", &[(b.id, 1)], iban, date!(2026 - 10 - 01), now).is_err(),
        "sin mandato no"
    );
    assert!(
        create(&v, "cobros", &[(a.id, 1)], iban, date!(2026 - 09 - 01), now).is_err(),
        "fecha pasada"
    );
    let r = create(&v, "cobros", &[(a.id, 1)], iban, date!(2026 - 10 - 01), now).unwrap();
    assert_eq!(r.reference, "REM-2026-0001");
    assert_eq!(r.total, Money::from_euros(1210, 0));
    let (_, xml) = remesas::file(&v, r.id).unwrap();
    assert!(xml.contains("pain.008.001.02"));
    assert!(xml.contains("<MndtId>MAND-0001</MndtId>"));
    assert!(xml.contains("<IBAN>ES7921000813610123456789</IBAN>"));
    assert!(xml.contains("<Nm>Ejemplo Diseno SL</Nm>"));
    assert!(xml.contains("000B12345674</Id>"));
    // Ya no está pendiente: va en la remesa, y no se puede deshacer el cobro suelto.
    let item = list(&v, now.date())
        .unwrap()
        .into_iter()
        .find(|i| i.invoice_id == a.id)
        .unwrap();
    assert_eq!(item.status, "registrada");
    assert_eq!(item.remittance.as_deref(), Some("REM-2026-0001"));
    assert!(unsettle(&v, "emitida", a.id, 1, now, true).is_err());
    assert!(create(&v, "cobros", &[(a.id, 1)], iban, date!(2026 - 10 - 01), now).is_err());

    let bal = |acc: &str| {
        trial_balance(&v, 2026, None)
            .unwrap()
            .iter()
            .find(|r| r.account.as_str() == acc)
            .map(|r| r.debit - r.credit)
            .unwrap_or(Money::ZERO)
    };
    assert_eq!(bal("4312"), Money::from_euros(1210, 0));
    assert_eq!(bal("4300001"), Money::ZERO);

    // El banco abona 1.210 € el 1/10: se propone la remesa la primera.
    import_n43(&v, N43, now).unwrap();
    let m = movements(&v, true)
        .unwrap()
        .into_iter()
        .find(|m| m.amount == Money::from_euros(1210, 0))
        .unwrap();
    let s = suggestions(&v, m.id).unwrap();
    assert_eq!((s[0].kind.as_str(), s[0].id), ("remesa", r.id));
    reconcile_remittance(&v, m.id, r.id, now).unwrap();
    assert_eq!(bal("4312"), Money::ZERO);
    assert_eq!(bal("5720001"), Money::from_euros(1210, 0));
    assert!(undo(&v, r.id, now, true).is_err(), "ya está en el banco");

    // Devuelven el recibo el 5/10: vuelve a pendiente y el cliente vuelve a deber.
    assert!(return_item(&v, r.id, a.id, 1, date!(2026 - 10 - 05), None, now).is_err());
    let devolucion = return_item(
        &v,
        r.id,
        a.id,
        1,
        date!(2026 - 10 - 05),
        Some("5720001"),
        now,
    )
    .unwrap()
    .unwrap();
    assert_eq!(bal("5720001"), Money::ZERO);
    // El asiento de la devolución enlaza con la remesa y no se borra desde el diario.
    let e = taxpain_app::accounting::journal(&v, 2026)
        .unwrap()
        .into_iter()
        .find(|e| e.id == devolucion)
        .unwrap();
    assert_eq!(e.entry.origin, Origin::DevolucionRemesa(r.id));
    assert!(taxpain_app::accounting::delete_manual_entry(&v, devolucion, now, true).is_err());
    assert_eq!(bal("4300001"), Money::from_euros(1210, 0));
    let item = list(&v, now.date())
        .unwrap()
        .into_iter()
        .find(|i| i.invoice_id == a.id)
        .unwrap();
    assert_eq!((item.status.as_str(), item.remittance), ("pendiente", None));
    assert!(
        return_item(
            &v,
            r.id,
            a.id,
            1,
            date!(2026 - 10 - 05),
            Some("5720001"),
            now
        )
        .is_err()
    );
    // Puede ir en otra remesa.
    let r2 = create(&v, "cobros", &[(a.id, 1)], iban, date!(2026 - 10 - 10), now).unwrap();
    assert_eq!(r2.reference, "REM-2026-0002");
    assert!(v.verify_audit().is_ok());
}

#[test]
fn orden_de_transferencias_y_deshacer() {
    let v = setup();
    let now = datetime!(2026-09-25 10:00 UTC);
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
            supplier_nif: "A22222228".into(),
            supplier_name: "Proveedor SA".into(),
            number: "P-7".into(),
            issue_date: date!(2026 - 09 - 01),
            base: Money::from_euros(200, 0),
            vat: Money::from_euros(42, 0),
            withholding: Money::ZERO,
            total: Money::from_euros(242, 0),
            deductible_pct: 100,
            capital_good: false,
            nature: Nature::General,
            asset_class: None::<AssetClass>,
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
    // Sin forma de pago fijada cuenta como transferencia, pero falta el IBAN.
    let c = candidates(&v, "pagos", now.date()).unwrap();
    assert_eq!(c.len(), 1);
    assert!(c[0].problem.as_deref().unwrap().contains("IBAN"));
    let supplier = taxpain_app::cartera::parties(&v).unwrap()[0]
        .account
        .clone();
    set_party_terms(
        &v,
        &supplier,
        Some(&PartyTerms {
            days: vec![30],
            pay_day: None,
            method: "transferencia".into(),
            treasury: None,
            iban: Some("ES7921000813610123456789".into()),
            mandate_ref: None,
            mandate_date: None,
            key_190: None,
        }),
        now,
    )
    .unwrap();
    let r = create(
        &v,
        "pagos",
        &[(rec.id, 1)],
        "ES9121000418450200051332",
        date!(2026 - 09 - 30),
        now,
    )
    .unwrap();
    assert_eq!(r.reference, "PAG-2026-0001");
    let (_, xml) = remesas::file(&v, r.id).unwrap();
    assert!(xml.contains("pain.001.001.03"));
    assert!(xml.contains("<InstdAmt Ccy=\"EUR\">242.00</InstdAmt>"));
    let tb = trial_balance(&v, 2026, None).unwrap();
    let b555 = tb.iter().find(|r| r.account.as_str() == "555").unwrap();
    assert_eq!(b555.credit - b555.debit, Money::from_euros(242, 0));

    assert!(undo(&v, r.id, now, false).is_err());
    undo(&v, r.id, now, true).unwrap();
    assert!(remesas::list(&v).unwrap().is_empty());
    assert_eq!(
        candidates(&v, "pagos", now.date()).unwrap()[0].problem,
        None
    );
    let tb = trial_balance(&v, 2026, None).unwrap();
    assert!(
        tb.iter()
            .all(|r| r.account.as_str() != "555" || r.debit == r.credit)
    );

    // Deshecha una remesa que no era la última, la siguiente no repite la referencia (que es
    // única y el MsgId del fichero).
    let rec2 = ingest_file(
        &v,
        "g.xml",
        b"<?xml version=\"1.0\"?><y/>",
        Source::Manual,
        now,
    )
    .unwrap();
    review(
        &v,
        rec2.id,
        &Reviewed {
            supplier_nif: "B98765431".into(),
            supplier_name: "Otro Proveedor SL".into(),
            number: "Q-1".into(),
            issue_date: date!(2026 - 09 - 02),
            base: Money::from_euros(100, 0),
            vat: Money::from_euros(21, 0),
            withholding: Money::ZERO,
            total: Money::from_euros(121, 0),
            deductible_pct: 100,
            capital_good: false,
            nature: Nature::General,
            asset_class: None::<AssetClass>,
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
    let supplier2 = taxpain_app::cartera::parties(&v)
        .unwrap()
        .into_iter()
        .find(|p| p.name == "Otro Proveedor SL")
        .unwrap()
        .account;
    set_party_terms(
        &v,
        &supplier2,
        Some(&PartyTerms {
            days: vec![30],
            pay_day: None,
            method: "transferencia".into(),
            treasury: None,
            iban: Some("ES7921000813610123456789".into()),
            mandate_ref: None,
            mandate_date: None,
            key_190: None,
        }),
        now,
    )
    .unwrap();
    let iban = "ES9121000418450200051332";
    let r1 = create(
        &v,
        "pagos",
        &[(rec.id, 1)],
        iban,
        date!(2026 - 09 - 30),
        now,
    )
    .unwrap();
    let r2 = create(
        &v,
        "pagos",
        &[(rec2.id, 1)],
        iban,
        date!(2026 - 09 - 30),
        now,
    )
    .unwrap();
    assert_eq!(
        (r1.reference.as_str(), r2.reference.as_str()),
        ("PAG-2026-0001", "PAG-2026-0002")
    );
    undo(&v, r1.id, now, true).unwrap();
    let r3 = create(
        &v,
        "pagos",
        &[(rec.id, 1)],
        iban,
        date!(2026 - 09 - 30),
        now,
    )
    .unwrap();
    assert_eq!(r3.reference, "PAG-2026-0003");
}

/// Remesa de un recibo de 1.210 € abonada por el banco (movimiento del N43 del 1/10).
fn remesa_abonada(v: &Vault) -> (i64, i64) {
    let now = datetime!(2026-09-25 10:00 UTC);
    let a = issue(v, &draft("12345678Z", "Cliente Uno SL", 1000), now, true).unwrap();
    set_party_terms(
        v,
        "4300001",
        Some(&recibo(Some("ES7921000813610123456789"), true)),
        now,
    )
    .unwrap();
    let iban = "ES9121000418450200051332";
    let r = create(v, "cobros", &[(a.id, 1)], iban, date!(2026 - 10 - 01), now).unwrap();
    import_n43(v, N43, now).unwrap();
    let m = movements(v, true)
        .unwrap()
        .into_iter()
        .find(|m| m.amount == Money::from_euros(1210, 0))
        .unwrap();
    reconcile_remittance(v, m.id, r.id, now).unwrap();
    (r.id, a.id)
}

/// Cargo del banco por el recibo devuelto (como si viniera en el extracto).
fn cargo_devolucion(v: &Vault, fecha: &str) -> i64 {
    let conn = v.connection();
    conn.execute(
        "INSERT INTO bank_tx(bank_account, op_date, value_date, amount, description, import_hash, created_at)
         SELECT id, ?1, ?1, -121000, 'DEVOLUCION RECIBO CLIENTE UNO', 'dev' || ?1, '2026-10-06'
         FROM bank_account WHERE account = '5720001'",
        [fecha],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn conciliado(v: &Vault, t: i64) -> (Option<i64>, Option<String>) {
    v.connection()
        .query_row(
            "SELECT entry_id, matched_kind FROM bank_tx WHERE id = ?1",
            [t],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
}

#[test]
fn el_cargo_del_recibo_devuelto_queda_conciliado_con_la_devolucion() {
    let v = setup();
    let now = datetime!(2026-10-06 10:00 UTC);
    let (r, a) = remesa_abonada(&v);
    // El cargo ya está en el extracto: al anotar la devolución queda conciliado con su asiento.
    let t = cargo_devolucion(&v, "2026-10-05");
    let entry = return_item(&v, r, a, 1, date!(2026 - 10 - 05), Some("5720001"), now)
        .unwrap()
        .unwrap();
    assert_eq!(conciliado(&v, t), (Some(entry), Some("cuenta".into())));
    assert!(movements(&v, true).unwrap().iter().all(|m| m.id != t));
    let c = taxpain_app::conciliados::reconciled(&v, None, None, None).unwrap();
    let item = c.items.iter().find(|i| i.tx.id == t).unwrap();
    assert_eq!(item.kind, "devolucion");
    assert!(
        item.counterpart.starts_with("Devolución recibo")
            && item.counterpart.ends_with("· remesa REM-2026-0001"),
        "{}",
        item.counterpart
    );
    assert_eq!(item.party, "");
    // Deshacer esa conciliación sólo la desenlaza: el asiento de la devolución se queda.
    taxpain_app::conciliados::unreconcile(&v, t, now, true).unwrap();
    assert_eq!(conciliado(&v, t), (None, None));
    let n: i64 = v
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM journal_entry WHERE id = ?1",
            [entry],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
    // Pendiente otra vez, el banco propone la devolución y se casa con el mismo asiento.
    let s = suggestions(&v, t).unwrap();
    assert_eq!((s[0].kind.as_str(), s[0].id), ("devolucion", entry));
    assert!(taxpain_app::bank::reconcile_return(&v, t, entry + 1000, now).is_err());
    taxpain_app::bank::reconcile_return(&v, t, entry, now).unwrap();
    assert_eq!(conciliado(&v, t), (Some(entry), Some("cuenta".into())));
    // El banco cuadra: abono de la remesa y cargo de la devolución, una sola vez cada uno.
    let bal: Money = trial_balance(&v, 2026, None)
        .unwrap()
        .iter()
        .filter(|r| r.account.as_str() == "5720001")
        .map(|r| r.debit - r.credit)
        .sum();
    assert_eq!(bal, Money::ZERO);
    assert!(v.verify_audit().is_ok());
}

#[test]
fn el_cargo_que_llega_despues_propone_la_devolucion() {
    let v = setup();
    let now = datetime!(2026-10-06 10:00 UTC);
    let (r, a) = remesa_abonada(&v);
    let entry = return_item(&v, r, a, 1, date!(2026 - 10 - 05), Some("5720001"), now)
        .unwrap()
        .unwrap();
    // Un cargo lejano en fecha no se casa; el del extracto, sí.
    let lejos = cargo_devolucion(&v, "2026-11-30");
    assert!(
        suggestions(&v, lejos)
            .unwrap()
            .iter()
            .all(|s| s.kind != "devolucion")
    );
    let t = cargo_devolucion(&v, "2026-10-07");
    let s = suggestions(&v, t).unwrap();
    assert_eq!((s[0].kind.as_str(), s[0].id), ("devolucion", entry));
    assert_eq!(s[0].number, "REM-2026-0001");
    taxpain_app::bank::reconcile_return(&v, t, entry, now).unwrap();
    // Ya tiene su movimiento: no se propone para otro ni se casa dos veces.
    assert!(
        suggestions(&v, lejos)
            .unwrap()
            .iter()
            .all(|s| s.kind != "devolucion")
    );
    assert!(taxpain_app::bank::reconcile_return(&v, lejos, entry, now).is_err());
}

#[test]
fn deshacer_un_recibo_devuelto_por_error() {
    let v = setup();
    let now = datetime!(2026-10-06 10:00 UTC);
    let (r, a) = remesa_abonada(&v);
    let bal = |acc: &str| {
        trial_balance(&v, 2026, None)
            .unwrap()
            .iter()
            .find(|x| x.account.as_str() == acc)
            .map(|x| x.debit - x.credit)
            .unwrap_or(Money::ZERO)
    };
    let estado = || {
        list(&v, now.date())
            .unwrap()
            .into_iter()
            .find(|i| i.invoice_id == a)
            .map(|i| (i.status, i.remittance))
            .unwrap()
    };
    let t = cargo_devolucion(&v, "2026-10-05");
    let entry = return_item(&v, r, a, 1, date!(2026 - 10 - 05), Some("5720001"), now)
        .unwrap()
        .unwrap();
    assert_eq!(bal("4300001"), Money::from_euros(1210, 0));
    assert!(remesas::undo_return(&v, r, a, 1, now, false).is_err());
    remesas::undo_return(&v, r, a, 1, now, true).unwrap();
    // Como antes de la devolución: cobrado por la remesa, el cliente no debe nada y el cargo
    // del banco vuelve a pendiente.
    assert_eq!(
        estado(),
        ("registrada".to_owned(), Some("REM-2026-0001".to_owned()))
    );
    assert_eq!(bal("4300001"), Money::ZERO);
    assert_eq!(bal("5720001"), Money::from_euros(1210, 0));
    assert_eq!(conciliado(&v, t), (None, None));
    let n: i64 = v
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM journal_entry WHERE id = ?1",
            [entry],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
    let rem = &remesas::list(&v).unwrap()[0];
    assert!(rem.items[0].returned_on.is_none());
    assert_eq!(rem.total, Money::from_euros(1210, 0));
    assert!(
        remesas::undo_return(&v, r, a, 1, now, true).is_err(),
        "ya no está devuelto"
    );
    // Se puede volver a devolver; y si el recibo va en otra remesa, ya no se deshace.
    return_item(&v, r, a, 1, date!(2026 - 10 - 05), Some("5720001"), now).unwrap();
    assert_eq!(conciliado(&v, t).1.as_deref(), Some("cuenta"));
    create(
        &v,
        "cobros",
        &[(a, 1)],
        "ES9121000418450200051332",
        date!(2026 - 10 - 20),
        now,
    )
    .unwrap();
    assert!(remesas::undo_return(&v, r, a, 1, now, true).is_err());
    assert!(v.verify_audit().is_ok());
}

#[test]
fn deshacer_la_devolucion_de_una_remesa_aun_sin_banco() {
    let v = setup();
    let now = datetime!(2026-09-25 10:00 UTC);
    let a = issue(&v, &draft("12345678Z", "Cliente Uno SL", 1000), now, true).unwrap();
    set_party_terms(
        &v,
        "4300001",
        Some(&recibo(Some("ES7921000813610123456789"), true)),
        now,
    )
    .unwrap();
    let iban = "ES9121000418450200051332";
    let r = create(&v, "cobros", &[(a.id, 1)], iban, date!(2026 - 10 - 01), now).unwrap();
    assert_eq!(
        return_item(&v, r.id, a.id, 1, date!(2026 - 09 - 30), None, now).unwrap(),
        None
    );
    remesas::undo_return(&v, r.id, a.id, 1, now, true).unwrap();
    let item = list(&v, now.date())
        .unwrap()
        .into_iter()
        .find(|i| i.invoice_id == a.id)
        .unwrap();
    assert_eq!(item.status, "registrada");
    let b = trial_balance(&v, 2026, None).unwrap();
    let saldo = |acc: &str| {
        b.iter()
            .find(|x| x.account.as_str() == acc)
            .map(|x| x.debit - x.credit)
            .unwrap_or(Money::ZERO)
    };
    assert_eq!(saldo("4312"), Money::from_euros(1210, 0));
    assert_eq!(saldo("4300001"), Money::ZERO);
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
