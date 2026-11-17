//! Deshacer una conciliación bancaria (CONTABILIDAD-03): listado de conciliados, deshacer y
//! volver a conciliar, plazos sueltos, remesas, traspasos, reglas y bloqueos.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::{close_year, trial_balance};
use taxpain_app::bank::{
    import_n43, movements, reconcile_account, reconcile_invoice, reconcile_remittance,
};
use taxpain_app::cartera::{self, PartyTerms};
use taxpain_app::conciliados::{reconciled, unreconcile};
use taxpain_app::invoicing::{self, InvoiceDraft, issue};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::{bank_rules, models, remesas};
use taxpain_core::invoice::{InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::OffsetDateTime;
use time::macros::{date, datetime};

const N43: &[u8] = include_bytes!("../../../fixtures/accounting/n43-ejemplo.txt");
const NOW: OffsetDateTime = datetime!(2026-10-20 10:00 UTC);

fn setup() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
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

/// Factura emitida de `euros` + 21 % a B98765431.
fn factura(v: &Vault, euros: i64) -> i64 {
    issue(
        v,
        &InvoiceDraft {
            kind: InvoiceKind::Completa,
            series: None,
            issue_date: date!(2026 - 09 - 20),
            operation_date: None,
            recipient: Some(Party {
                id: PartyId::Nif("B98765431".into()),
                name: "Cliente Demo SL".into(),
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
        },
        NOW,
        true,
    )
    .unwrap()
    .id
}

fn bal(v: &Vault, acc: &str) -> Money {
    trial_balance(v, 2026, None)
        .unwrap()
        .iter()
        .find(|r| r.account.as_str() == acc)
        .map(|r| r.debit - r.credit)
        .unwrap_or(Money::ZERO)
}

/// Movimiento pendiente del extracto de ejemplo por importe.
fn mov(v: &Vault, cents: i64) -> i64 {
    movements(v, true)
        .unwrap()
        .into_iter()
        .find(|m| m.amount.cents() == cents)
        .unwrap()
        .id
}

fn estado(v: &Vault, invoice: i64, seq: i64) -> String {
    cartera::list(v, date!(2026 - 10 - 20))
        .unwrap()
        .into_iter()
        .find(|i| i.kind == "emitida" && i.invoice_id == invoice && i.seq == seq)
        .unwrap()
        .status
}

/// Movimiento nuevo en la cuenta del banco `bank_account` (id).
fn insertar(v: &Vault, bank_account: i64, fecha: &str, cents: i64, texto: &str) -> i64 {
    let c = v.connection();
    c.execute(
        "INSERT INTO bank_tx(bank_account, op_date, value_date, amount, description, import_hash,
                             created_at)
         VALUES (?1, ?2, ?2, ?3, ?4, ?5, '2026-10-20')",
        taxpain_vault::rusqlite::params![
            bank_account,
            fecha,
            cents,
            texto,
            format!("{fecha}{cents}{texto}")
        ],
    )
    .unwrap();
    c.last_insert_rowid()
}

#[test]
fn conciliar_mal_deshacer_y_volver_a_conciliar() {
    let v = setup();
    let inv = factura(&v, 1000);
    import_n43(&v, N43, NOW).unwrap();
    let cobro = mov(&v, 121_000);
    // Por error se lleva a ingresos varios (759) en vez de a la factura.
    reconcile_account(&v, cobro, "759", "", NOW).unwrap();
    assert_eq!(bal(&v, "759"), Money::from_euros(-1210, 0));
    assert_eq!(bank_rules::list(&v).unwrap().len(), 1, "aprende la regla");

    let l = reconciled(&v, None, None, None).unwrap();
    assert_eq!(l.items.len(), 1);
    assert_eq!(l.accounts.len(), 1);
    let r = &l.items[0];
    assert_eq!((r.kind.as_str(), r.counterpart.as_str()), ("cuenta", "759"));
    assert_eq!(r.account, l.accounts[0].account);
    assert!(r.rule.is_some());
    assert_eq!(r.blocked, None);

    assert!(
        unreconcile(&v, cobro, NOW, false).is_err(),
        "pide confirmación"
    );
    unreconcile(&v, cobro, NOW, true).unwrap();
    assert!(
        unreconcile(&v, cobro, NOW, true).is_err(),
        "ya no está conciliado"
    );
    assert_eq!(bal(&v, "759"), Money::ZERO);
    assert_eq!(bal(&v, "5720001"), Money::ZERO);
    assert!(
        bank_rules::list(&v).unwrap().is_empty(),
        "la regla se olvida"
    );
    assert!(reconciled(&v, None, None, None).unwrap().items.is_empty());
    assert_eq!(movements(&v, true).unwrap().len(), 6);

    // Ahora sí, con la factura; y deshacerlo la devuelve a pendiente en la cartera.
    reconcile_invoice(&v, cobro, "emitida", inv, None, NOW).unwrap();
    assert_eq!(estado(&v, inv, 1), "banco");
    let r = &reconciled(&v, None, None, None).unwrap().items[0];
    assert_eq!(r.kind, "emitida");
    assert!(r.counterpart.starts_with("Factura A"), "{}", r.counterpart);
    assert_eq!(r.party, "Cliente Demo SL");
    unreconcile(&v, cobro, NOW, true).unwrap();
    assert_eq!(estado(&v, inv, 1), "pendiente");
    assert_eq!(bal(&v, "4300001"), Money::from_euros(1210, 0));
    reconcile_invoice(&v, cobro, "emitida", inv, None, NOW).unwrap();
    assert_eq!(bal(&v, "4300001"), Money::ZERO);

    let audit = taxpain_vault::audit::entries(v.connection()).unwrap();
    assert_eq!(
        audit
            .iter()
            .filter(|a| a.action == "bank.unreconcile")
            .count(),
        2
    );
    assert!(v.verify_audit().is_ok());
}

#[test]
fn filtros_por_cuenta_y_fechas() {
    let v = setup();
    import_n43(&v, N43, NOW).unwrap();
    reconcile_account(&v, mov(&v, -1250), "626", "Comisión", NOW).unwrap();
    reconcile_account(&v, mov(&v, -7865), "629", "Teléfono", NOW).unwrap();
    let todos = reconciled(&v, None, None, None).unwrap().items;
    assert_eq!(todos.len(), 2);
    assert_eq!(todos[0].tx.op_date, "2026-10-12", "el más nuevo primero");
    let desde = reconciled(&v, None, Some(date!(2026 - 10 - 05)), None).unwrap();
    assert_eq!(desde.items.len(), 1);
    let hasta = reconciled(&v, None, None, Some(date!(2026 - 10 - 02))).unwrap();
    assert_eq!(hasta.items[0].counterpart, "629");
    assert_eq!(
        reconciled(&v, Some("5720001"), None, None)
            .unwrap()
            .items
            .len(),
        2
    );
    assert!(
        reconciled(&v, Some("5720099"), None, None)
            .unwrap()
            .items
            .is_empty()
    );
}

#[test]
fn deshacer_un_plazo_deja_los_demas() {
    let v = setup();
    let inv = factura(&v, 1000);
    import_n43(&v, N43, NOW).unwrap();
    cartera::set_schedule(
        &v,
        "emitida",
        inv,
        Some(&[
            (date!(2026 - 10 - 01), Money::from_euros(605, 0)),
            (date!(2026 - 11 - 01), Money::from_euros(605, 0)),
        ]),
        NOW,
    )
    .unwrap();
    let a = insertar(
        &v,
        1,
        "2026-10-03",
        60_500,
        "TRANSFERENCIA CLIENTE DEMO PLAZO 1",
    );
    let b = insertar(
        &v,
        1,
        "2026-10-04",
        60_500,
        "TRANSFERENCIA CLIENTE DEMO PLAZO 2",
    );
    reconcile_invoice(&v, a, "emitida", inv, Some(1), NOW).unwrap();
    reconcile_invoice(&v, b, "emitida", inv, Some(2), NOW).unwrap();
    let l = reconciled(&v, None, None, None).unwrap().items;
    assert!(l.iter().any(|r| r.counterpart.ends_with("(plazo 2)")));
    unreconcile(&v, a, NOW, true).unwrap();
    assert_eq!(estado(&v, inv, 1), "pendiente");
    assert_eq!(estado(&v, inv, 2), "banco");
    assert_eq!(bal(&v, "4300001"), Money::from_euros(605, 0));
    // El plazo 1 se puede volver a casar con su movimiento.
    reconcile_invoice(&v, a, "emitida", inv, Some(1), NOW).unwrap();
    assert_eq!(bal(&v, "4300001"), Money::ZERO);
}

#[test]
fn remesa_apuntada_por_el_banco() {
    let v = setup();
    let inv = factura(&v, 1000);
    cartera::set_party_terms(
        &v,
        "4300001",
        Some(&PartyTerms {
            days: vec![30],
            pay_day: None,
            method: "recibo".into(),
            treasury: None,
            iban: Some("ES9121000418450200051332".into()),
            mandate_ref: Some("MAND-0001".into()),
            mandate_date: Some(date!(2026 - 01 - 15)),
            key_190: None,
        }),
        NOW,
    )
    .unwrap();
    let now = datetime!(2026-09-25 10:00 UTC);
    let iban = "ES9121000418450200051332";
    let r = remesas::create(&v, "cobros", &[(inv, 1)], iban, date!(2026 - 10 - 01), now).unwrap();
    import_n43(&v, N43, now).unwrap();
    let m = mov(&v, 121_000);
    reconcile_remittance(&v, m, r.id, now).unwrap();
    assert_eq!(bal(&v, "4312"), Money::ZERO);
    let item = &reconciled(&v, None, None, None).unwrap().items[0];
    assert_eq!(item.kind, "remesa");
    assert_eq!(item.counterpart, "Remesa REM-2026-0001");
    // Sin repetir la remesa: sólo el concepto del banco.
    assert!(!item.party.contains("REM-2026-0001"), "{}", item.party);
    assert!(
        item.party.contains("TRANSFERENCIA DE CLIENTE"),
        "{}",
        item.party
    );

    // Deshecha la conciliación, la remesa vuelve a estar a la espera del banco (4312) y sus
    // recibos siguen cobrados por ella; ya se puede deshacer la remesa.
    unreconcile(&v, m, now, true).unwrap();
    assert_eq!(bal(&v, "4312"), Money::from_euros(1210, 0));
    assert_eq!(bal(&v, "5720001"), Money::ZERO);
    assert_eq!(estado(&v, inv, 1), "registrada");
    reconcile_remittance(&v, m, r.id, now).unwrap();

    // Con un recibo devuelto después del abono, no: la remesa se deshace entera o no.
    remesas::return_item(
        &v,
        r.id,
        inv,
        1,
        date!(2026 - 10 - 05),
        Some("5720001"),
        now,
    )
    .unwrap();
    let item = &reconciled(&v, None, None, None).unwrap().items[0];
    assert!(item.blocked.as_deref().unwrap().contains("devueltos"));
    let e = unreconcile(&v, m, now, true).unwrap_err().to_string();
    assert!(e.contains("REM-2026-0001"), "{e}");
}

#[test]
fn traspaso_entre_cuentas_propias() {
    let v = setup();
    let c = v.connection();
    for (n, acc) in [("00490001", "5720001"), ("00490002", "5720002")] {
        c.execute(
            "INSERT INTO bank_account(number, account, name) VALUES (?1, ?2, ?1)",
            [n, acc],
        )
        .unwrap();
    }
    let sale = insertar(&v, 1, "2026-10-07", -50_000, "TRASPASO A CUENTA 2");
    let entra = insertar(&v, 2, "2026-10-07", 50_000, "TRASPASO DESDE CUENTA 1");
    reconcile_account(&v, sale, "5720002", "", NOW).unwrap();
    let l = reconciled(&v, None, None, None).unwrap();
    assert_eq!(l.items.len(), 2);
    assert!(l.items.iter().all(|r| r.kind == "traspaso"));
    assert_eq!(l.accounts.len(), 2);
    assert_eq!(
        reconciled(&v, Some("5720002"), None, None)
            .unwrap()
            .items
            .len(),
        1
    );
    // Deshacer cualquiera de las dos caras deshace el traspaso entero.
    unreconcile(&v, entra, NOW, true).unwrap();
    assert_eq!(movements(&v, true).unwrap().len(), 2);
    assert_eq!(bal(&v, "5720001"), Money::ZERO);
    assert_eq!(bal(&v, "5720002"), Money::ZERO);
}

#[test]
fn bloqueos_por_ejercicio_cerrado_y_modelos_presentados() {
    let v = setup();
    let inv = factura(&v, 1000);
    import_n43(&v, N43, NOW).unwrap();
    let cobro = mov(&v, 121_000);
    let comision = mov(&v, -1250);
    reconcile_invoice(&v, cobro, "emitida", inv, None, NOW).unwrap();
    reconcile_account(&v, comision, "626", "Comisión", NOW).unwrap();

    // El 303 del 4T está presentado (importado de la gestoría): la comisión (626) entra en él;
    // el cobro de la factura no (IVA devengado al emitirla).
    v.connection()
        .execute(
            "INSERT INTO filed_model(model, year, period, filename, file_sha256, source, boxes,
                                     imported_at)
             VALUES ('303', 2026, '4T', '303.txt', 'x', 'manual', '{}', '2027-01-20')",
            [],
        )
        .unwrap();
    let e = unreconcile(&v, comision, NOW, true)
        .unwrap_err()
        .to_string();
    assert!(e.contains("303") && e.contains("4T"), "{e}");
    let l = reconciled(&v, None, None, None).unwrap().items;
    let fila = |id: i64| l.iter().find(|r| r.tx.id == id).unwrap().blocked.clone();
    assert!(fila(comision).is_some());
    assert_eq!(fila(cobro), None);

    // Con criterio de caja el cobro sí cuenta en el 303 del trimestre en que se cobra.
    let mut s = models::settings(&v).unwrap();
    s.cash_basis = true;
    models::save_settings(&v, &s, NOW).unwrap();
    let e = unreconcile(&v, cobro, NOW, true).unwrap_err().to_string();
    assert!(e.contains("303"), "{e}");
    s.cash_basis = false;
    models::save_settings(&v, &s, NOW).unwrap();

    // Ejercicio cerrado: no se deshace nada hasta reabrirlo.
    close_year(&v, 2026, NOW, true).unwrap();
    let e = unreconcile(&v, cobro, NOW, true).unwrap_err().to_string();
    assert!(e.contains("cerrado"), "{e}");
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
