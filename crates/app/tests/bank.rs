//! Extracto Norma 43: importación sin duplicados y conciliación con facturas y cuentas.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::trial_balance;
use taxpain_app::bank::{import_n43, movements, reconcile_account, reconcile_invoice, suggestions};
use taxpain_app::invoicing::{self, InvoiceDraft, issue};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
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

#[test]
fn importar_sugerir_y_conciliar() {
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    let inv = issue(
        &v,
        &InvoiceDraft {
            kind: InvoiceKind::Completa,
            series: None,
            issue_date: date!(2026 - 09 - 20),
            operation_date: None,
            recipient: Some(Party {
                id: PartyId::Nif("B12345674".into()),
                name: "Cliente Ejemplo SA".into(),
                address: addr(),
                person: None,
            }),
            lines: vec![InvoiceLine {
                description: "Servicios".into(),
                quantity_milli: 1000,
                unit_price: Money::from_euros(1000, 0),
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
        now,
        true,
    )
    .unwrap();

    let s = import_n43(&v, N43, now).unwrap();
    assert_eq!((s.accounts, s.new_movements, s.duplicates), (1, 6, 0));
    let again = import_n43(&v, N43, now).unwrap();
    assert_eq!(
        (again.new_movements, again.duplicates),
        (0, 6),
        "reimportar no duplica"
    );

    let pending = movements(&v, true).unwrap();
    assert_eq!(pending.len(), 6);
    let cobro = pending
        .iter()
        .find(|m| m.amount == Money::from_euros(1210, 0))
        .unwrap();
    let sug = suggestions(&v, cobro.id).unwrap();
    assert_eq!(sug.len(), 1);
    assert_eq!(sug[0].id, inv.id);
    reconcile_invoice(&v, cobro.id, "emitida", inv.id, None, now).unwrap();
    assert!(
        reconcile_invoice(&v, cobro.id, "emitida", inv.id, None, now).is_err(),
        "no se concilia dos veces"
    );
    assert!(
        suggestions(&v, cobro.id).unwrap().is_empty(),
        "la factura ya está cobrada"
    );

    let cargo = movements(&v, true)
        .unwrap()
        .into_iter()
        .find(|m| m.amount.is_negative())
        .unwrap();
    // La misma factura no se cobra con otro movimiento, ni un cargo puede ser un cobro.
    let otro_ingreso = movements(&v, true)
        .unwrap()
        .into_iter()
        .find(|m| !m.amount.is_negative())
        .unwrap();
    assert!(reconcile_invoice(&v, otro_ingreso.id, "emitida", inv.id, None, now).is_err());
    assert!(reconcile_invoice(&v, cargo.id, "emitida", inv.id, None, now).is_err());
    reconcile_account(&v, cargo.id, "626", "Comisión bancaria", now).unwrap();
    assert_eq!(movements(&v, true).unwrap().len(), 4);

    let tb = trial_balance(&v, 2026, None).unwrap();
    let bal = |c: &str| {
        tb.iter()
            .find(|b| b.account.as_str() == c)
            .map_or(Money::ZERO, |b| b.balance())
    };
    assert_eq!(bal("4300001"), Money::ZERO, "cliente cobrado");
    assert_eq!(bal("5720001"), Money::from_euros(1210, 0) + cargo.amount);
    assert_eq!(bal("626"), -cargo.amount);
    let d: Money = tb.iter().map(|b| b.debit).sum();
    let c: Money = tb.iter().map(|b| b.credit).sum();
    assert_eq!(d, c);
}

/// Una rectificativa en negativo (abono al cliente) se devuelve por transferencia: el cargo del
/// banco casa con su vencimiento negativo y lo cierra.
#[test]
fn abono_devuelto_por_transferencia() {
    use taxpain_core::invoice::{Rectification, RectificationMethod};
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    let draft = |euros: i64| InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: date!(2026 - 09 - 20),
        operation_date: None,
        recipient: Some(Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Cliente Ejemplo SA".into(),
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
    };
    let inv = issue(&v, &draft(100), now, true).unwrap();
    let mut r = draft(-100);
    r.kind = InvoiceKind::Rectificativa(Rectification {
        code: 1,
        method: RectificationMethod::Diferencias,
    });
    r.rectified = vec![(inv.number.clone(), date!(2026 - 09 - 20))];
    let rect = issue(&v, &r, now, true).unwrap();
    let conn = v.connection();
    conn.execute(
        "INSERT INTO bank_account(number, account, name) VALUES ('00490001', '5720001', 'Banco')",
        [],
    )
    .unwrap();
    for (i, amount) in [-12_100, 12_100].iter().enumerate() {
        conn.execute(
            "INSERT INTO bank_tx(bank_account, op_date, value_date, amount, description, import_hash, created_at)
             VALUES (1, '2026-10-01', '2026-10-01', ?1, 'TRANSFERENCIA', ?2, '2026-10-02')",
            taxpain_vault::rusqlite::params![amount, format!("h{i}")],
        )
        .unwrap();
    }
    let (cargo, ingreso) = (1, 2);
    let sug = suggestions(&v, cargo).unwrap();
    assert_eq!(
        sug.iter().map(|s| (s.id, s.amount)).collect::<Vec<_>>(),
        [(rect.id, Money::from_euros(-121, 0))],
        "el cargo propone el abono, no la factura"
    );
    let sug = suggestions(&v, ingreso).unwrap();
    assert_eq!(
        sug.iter().map(|s| s.id).collect::<Vec<_>>(),
        [inv.id],
        "el ingreso propone la factura, no el abono"
    );
    assert!(reconcile_invoice(&v, cargo, "emitida", inv.id, None, now).is_err());
    assert!(reconcile_invoice(&v, ingreso, "emitida", rect.id, None, now).is_err());
    reconcile_invoice(&v, cargo, "emitida", rect.id, None, now).unwrap();
    let item = taxpain_app::cartera::list(&v, now.date())
        .unwrap()
        .into_iter()
        .find(|i| i.invoice_id == rect.id)
        .unwrap();
    assert_eq!(item.status, "banco");
    let tb = trial_balance(&v, 2026, None).unwrap();
    let bal = |c: &str| {
        tb.iter()
            .find(|b| b.account.as_str() == c)
            .map_or(Money::ZERO, |b| b.balance())
    };
    assert_eq!(
        bal("4300001"),
        Money::from_euros(121, 0),
        "queda la factura por cobrar"
    );
    assert_eq!(bal("5720001"), Money::from_euros(-121, 0));
}

/// La subcuenta del banco sale siempre de la cuenta de tres dígitos: si la configurada ya es
/// una subcuenta (5720001), no se le añaden dígitos (57200011).
#[test]
fn cuenta_del_banco_desde_la_de_tres_digitos() {
    use taxpain_app::accounting::save_posting_accounts;
    use taxpain_core::accounting::account::acc;
    use taxpain_core::accounting::posting::PostingAccounts;
    let v = setup();
    let a = PostingAccounts {
        bank: acc("5720001"),
        ..PostingAccounts::default()
    };
    save_posting_accounts(&v, &a).unwrap();
    import_n43(&v, N43, datetime!(2026-10-13 10:00 UTC)).unwrap();
    let account: String = v
        .connection()
        .query_row("SELECT account FROM bank_account", [], |r| r.get(0))
        .unwrap();
    assert_eq!(account, "5720001");
}

#[test]
fn fichero_corrupto_no_importa_nada() {
    let v = setup();
    let bad = String::from_utf8_lossy(N43).replacen("000000001210000", "000000001211000", 1);
    assert!(import_n43(&v, bad.as_bytes(), datetime!(2026-10-13 10:00 UTC)).is_err());
    assert!(movements(&v, false).unwrap().is_empty());
}

#[test]
fn reglas_aprendidas_del_banco() {
    use taxpain_app::bank_rules::{apply, delete, list, proposals, rule_for};
    let v = setup();
    let now = datetime!(2026-10-22 10:00 UTC);
    let conn = v.connection();
    conn.execute(
        "INSERT INTO bank_account(number, account, name) VALUES ('00490001', '5720001', 'Banco')",
        [],
    )
    .unwrap();
    for (i, (desc, amount)) in [
        ("COMISION MANTENIMIENTO 0912", -1_200),
        ("COMISION MANTENIMIENTO 1012", -1_200),
        ("RECIBO IBERDROLA REF 77", -8_000),
        ("COMISION MANTENIMIENTO 1112", 1_200),
    ]
    .iter()
    .enumerate()
    {
        conn.execute(
            "INSERT INTO bank_tx(bank_account, op_date, value_date, amount, description, import_hash, created_at)
             VALUES (1, '2026-10-01', '2026-10-01', ?1, ?2, ?3, '2026-10-02')",
            taxpain_vault::rusqlite::params![amount, desc, format!("h{i}")],
        )
        .unwrap();
    }
    // Sin reglas no se propone nada; al contabilizar la primera comisión se aprende.
    assert!(proposals(&v).unwrap().is_empty());
    reconcile_account(&v, 1, "626", "", now).unwrap();
    let rules = list(&v).unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(
        (
            rules[0].pattern.as_str(),
            rules[0].sign.as_str(),
            rules[0].account.as_str()
        ),
        ("comision mantenimiento", "cargo", "626")
    );
    assert_eq!(
        rule_for(&v, 2).unwrap().map(|r| r.account),
        Some("626".into())
    );
    // Sólo la del mismo sentido (el abono de 12 € no) y no la de Iberdrola.
    let p = proposals(&v).unwrap();
    assert_eq!(p.iter().map(|p| p.tx.id).collect::<Vec<_>>(), [2]);
    assert_eq!(apply(&v, &[2], now).unwrap(), 1);
    assert!(apply(&v, &[3], now).is_err());
    let b = trial_balance(&v, 2026, None).unwrap();
    let saldo = |a: &str| {
        b.iter()
            .find(|x| x.account.as_str() == a)
            .map_or(Money::ZERO, |x| x.balance())
    };
    assert_eq!(saldo("626"), Money::from_cents(2_400));
    delete(&v, rules[0].id, now).unwrap();
    assert!(list(&v).unwrap().is_empty());
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

/// Factura emitida de 121 € (100 + 21 % de IVA) a un cliente inventado, con una cuenta de banco
/// y los movimientos `amounts` (en céntimos), y las funciones para ver su cartera y un saldo.
fn factura_121(v: &Vault, amounts: &[i64]) -> i64 {
    let now = datetime!(2026-10-13 10:00 UTC);
    let inv = issue(
        v,
        &InvoiceDraft {
            kind: InvoiceKind::Completa,
            series: None,
            issue_date: date!(2026 - 09 - 20),
            operation_date: None,
            recipient: Some(Party {
                id: PartyId::Nif("B12345674".into()),
                name: "Cliente Ejemplo SA".into(),
                address: addr(),
                person: None,
            }),
            lines: vec![InvoiceLine {
                description: "Servicios".into(),
                quantity_milli: 1000,
                unit_price: Money::from_euros(100, 0),
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
        now,
        true,
    )
    .unwrap();
    let conn = v.connection();
    conn.execute(
        "INSERT INTO bank_account(number, account, name) VALUES ('00490001', '5720001', 'Banco')",
        [],
    )
    .unwrap();
    for (i, amount) in amounts.iter().enumerate() {
        conn.execute(
            "INSERT INTO bank_tx(bank_account, op_date, value_date, amount, description, import_hash, created_at)
             VALUES (1, '2026-10-01', '2026-10-01', ?1, 'TRANSFERENCIA', ?2, '2026-10-02')",
            taxpain_vault::rusqlite::params![amount, format!("h{i}")],
        )
        .unwrap();
    }
    inv.id
}

fn cartera_de(v: &Vault, invoice_id: i64) -> Vec<(String, Money)> {
    let now = datetime!(2026-10-13 10:00 UTC);
    let mut x: Vec<(String, Money)> = taxpain_app::cartera::list(v, now.date())
        .unwrap()
        .into_iter()
        .filter(|i| i.invoice_id == invoice_id)
        .map(|i| (i.status, i.amount))
        .collect();
    x.sort();
    x
}

fn saldo_de(v: &Vault, c: &str) -> Money {
    trial_balance(v, 2026, None)
        .unwrap()
        .iter()
        .find(|b| b.account.as_str() == c)
        .map_or(Money::ZERO, |b| b.balance())
}

fn concepto_del_banco(v: &Vault, tx_id: i64) -> String {
    taxpain_app::accounting::journal(v, 2026)
        .unwrap()
        .into_iter()
        .find(|e| e.entry.origin == taxpain_core::accounting::Origin::Banco(tx_id))
        .unwrap()
        .entry
        .concept
}

/// Un cobro parcial no deja la factura cobrada: la parte cobrada pasa a ser un vencimiento
/// propio (conciliado con el banco) y el resto sigue pendiente. Con el segundo cobro (1 céntimo
/// de menos, redondeo) queda cobrada y el cliente a cero.
#[test]
fn cobro_parcial_deja_lo_pendiente() {
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    let inv = factura_121(&v, &[5_000, 7_099]);
    // 50 € de 121 €: quedan 71 € pendientes.
    reconcile_invoice(&v, 1, "emitida", inv, None, now).unwrap();
    assert_eq!(
        cartera_de(&v, inv),
        vec![
            ("banco".to_owned(), Money::from_euros(50, 0)),
            ("pendiente".to_owned(), Money::from_euros(71, 0)),
        ]
    );
    assert_eq!(saldo_de(&v, "4300001"), Money::from_euros(71, 0));
    // 70,99 €: un céntimo de redondeo; cobrada.
    reconcile_invoice(&v, 2, "emitida", inv, None, now).unwrap();
    assert!(cartera_de(&v, inv).iter().all(|(s, _)| s == "banco"));
    assert_eq!(saldo_de(&v, "4300001"), Money::ZERO);
    assert_eq!(saldo_de(&v, "669"), Money::from_cents(1));
}

/// Con un vencimiento elegido y un cobro parcial, el «(s/n)» del concepto es el del vencimiento
/// nuevo con el que casa el movimiento, contando los que hay tras dividir (antes decía «(1/2)»)
/// y ordenándolos por fecha de vencimiento.
#[test]
fn cobro_parcial_de_un_vencimiento_numera_tras_dividir() {
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    let inv = factura_121(&v, &[5_000]);
    taxpain_app::cartera::set_schedule(
        &v,
        "emitida",
        inv,
        Some(&[
            (date!(2026 - 10 - 20), Money::from_euros(60, 0)),
            (date!(2026 - 11 - 20), Money::from_euros(61, 0)),
        ]),
        now,
    )
    .unwrap();
    reconcile_invoice(&v, 1, "emitida", inv, Some(1), now).unwrap();
    assert_eq!(
        cartera_de(&v, inv),
        vec![
            ("banco".to_owned(), Money::from_euros(50, 0)),
            ("pendiente".to_owned(), Money::from_euros(10, 0)),
            ("pendiente".to_owned(), Money::from_euros(61, 0)),
        ]
    );
    let c = concepto_del_banco(&v, 1);
    // El cobrado es el primero: vence el 20/10, como lo que queda, y va antes.
    assert!(c.contains("(1/3) (parcial; quedan 10,00 €)"), "{c}");
    assert!(!c.contains("(1/2)"), "{c}");
}

/// Un cobro por más de lo pendiente salda al cliente por lo pendiente (la cartera y el 430
/// cuadran) y lo de más va a la 438, con el concepto que lo dice.
#[test]
fn sobrepago_a_anticipos_sin_descuadrar_cartera_y_cliente() {
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    let inv = factura_121(&v, &[5_000, 8_000]);
    reconcile_invoice(&v, 1, "emitida", inv, None, now).unwrap();
    // 80 € para los 71 € que quedan: 9 € de más.
    reconcile_invoice(&v, 2, "emitida", inv, None, now).unwrap();
    assert_eq!(
        cartera_de(&v, inv),
        vec![
            ("banco".to_owned(), Money::from_euros(50, 0)),
            ("banco".to_owned(), Money::from_euros(71, 0)),
        ]
    );
    assert_eq!(saldo_de(&v, "4300001"), Money::ZERO);
    assert_eq!(saldo_de(&v, "438"), Money::from_euros(-9, 0));
    assert_eq!(saldo_de(&v, "5720001"), Money::from_euros(130, 0));
    let c = concepto_del_banco(&v, 2);
    assert!(c.contains("cobrado de más 9,00 €, a la 438"), "{c}");
}
