//! Cuentas del banco en divisa (contravalor de cada movimiento, cobros y pagos con diferencias
//! de cambio, traspasos con una cuenta en euros) y valoración al cierre de las partidas
//! monetarias en divisa con su reversión (PGC, NRV 11.ª). Tipos de cambio inventados.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::bank::{movements, reconcile_account, reconcile_invoice, suggestions};
use taxpain_app::divisa::{
    EntradaValoracion, post_valoracion, saldos, set_contravalor, undo_valoracion, valoracion,
};
use taxpain_app::fx::{BceRate, record};
use taxpain_app::invoicing::{self, InvoiceDraft, issue};
use taxpain_app::received::{
    Nature, ReceivedFx, Reviewed, Source, VatRegime, ingest_file, review_fx,
};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::fx::{ExchangeRate, ForeignCurrency, RateSource};
use taxpain_core::invoice::{
    Address, IdType, InvoiceKind, InvoiceLine, NotSubject, Party, PartyId, RoundingMode,
    VatTreatment,
};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::rusqlite::params;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime};

const NOW: OffsetDateTime = datetime!(2026-11-16 10:00 UTC);

fn setup() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Estudio Demo SL".into(),
            address: Some(Address {
                street: "Calle Mayor 1".into(),
                post_code: "46001".into(),
                town: "València".into(),
                province: "Valencia".into(),
                country: "ES".into(),
            }),
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
            facturae: true,
            ubl: true,
        },
        destination: None,
        rounding: RoundingMode::PerRate,
        verifactu: VerifactuSettings {
            enabled: true,
            installation_id: "PRUEBA-1".into(),
            production: false,
        },
        footer: None,
        activity: None,
        simplified_up_to_3000: false,
    };
    invoicing::save_settings(&v, &s, datetime!(2026-01-01 9:00 UTC)).unwrap();
    v
}

fn usd(rate: &str, rate_date: Date) -> ForeignCurrency {
    ForeignCurrency {
        code: "USD".into(),
        rate: ExchangeRate::parse(rate).unwrap(),
        rate_date,
        source: RateSource::Manual,
    }
}

/// Tipo del BCE inventado, como si se hubiera descargado con «Tipo del BCE».
fn bce(v: &Vault, d: Date, rate: &str) {
    record(
        v,
        "prueba",
        &BceRate {
            currency: "USD".into(),
            requested: d,
            date: d,
            rate: ExchangeRate::parse(rate).unwrap(),
            cached: false,
        },
        date!(2027 - 03 - 01),
        NOW,
    )
    .unwrap();
}

/// Factura de 1.000,00 USD a un cliente de EE. UU. (no sujeta) al tipo 1,25: 800,00 €.
fn invoice_usd(v: &Vault, d: Date) -> i64 {
    let draft = InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: d,
        operation_date: None,
        recipient: Some(Party {
            id: PartyId::Foreign {
                country: "US".into(),
                id_type: IdType::DocumentoOficial,
                id: "12-3456789".into(),
            },
            name: "Client Inc.".into(),
            address: Some(Address {
                street: "1 Main Street".into(),
                post_code: "10001".into(),
                town: "New York".into(),
                province: "NY".into(),
                country: "US".into(),
            }),
            person: None,
        }),
        lines: vec![InvoiceLine {
            description: "Consultoría".into(),
            quantity_milli: 1000,
            unit_price: Money::from_euros(1000, 0),
            discount: Rate::ZERO,
            vat: VatTreatment::NotSubject(NotSubject::N2),
            surcharge: None,
            withholding: None,
            advance: false,
        }],
        rectified: vec![],
        rectified_amounts: None,
        description: "Consultoría".into(),
        notes: None,
        currency: Some(usd("1.25", d)),
        oss: None,
    };
    let out = issue(v, &draft, NOW, true).unwrap();
    assert_eq!(out.totals.payable, Money::from_euros(800, 0));
    out.id
}

/// Recibida de 500,00 USD (servicio de EE. UU., inversión del sujeto pasivo) al 1,25: 400,00 €.
fn received_usd(v: &Vault, d: Date, name: &str) -> i64 {
    let r = Reviewed {
        supplier_nif: "98-7654321".into(),
        supplier_name: "SaaS Tools Inc.".into(),
        number: format!("INV-{name}"),
        issue_date: d,
        base: Money::ZERO,
        vat: Money::ZERO,
        withholding: Money::ZERO,
        total: Money::ZERO,
        deductible_pct: 100,
        capital_good: false,
        nature: Nature::General,
        asset_class: None,
        vat_regime: VatRegime::Isp,
        supplier_country: Some("US".into()),
        self_assessed_rate: None,
        intra_goods: false,
        cash_basis: false,
        import_dua: false,
        prorata_use: Default::default(),
    };
    let fx = ReceivedFx {
        currency: usd("1.25", d),
        base: Money::from_euros(500, 0),
        vat: Money::ZERO,
        withholding: Money::ZERO,
        total: Money::from_euros(500, 0),
        vat_eur: None,
    };
    let doc = ingest_file(
        v,
        &format!("{name}.xml"),
        name.as_bytes(),
        Source::Manual,
        NOW,
    )
    .unwrap();
    review_fx(v, doc.id, &r, Some(&fx), NOW).unwrap();
    doc.id
}

fn account(v: &Vault, number: &str, sub: &str, name: &str, currency: &str) -> i64 {
    v.connection()
        .execute(
            "INSERT INTO bank_account(number, account, name, currency, kind)
             VALUES (?1, ?2, ?3, ?4, 'cuenta')",
            params![number, sub, name, currency],
        )
        .unwrap();
    v.connection().last_insert_rowid()
}

fn mov(v: &Vault, acc: i64, d: Date, cents: i64, balance: Option<i64>, desc: &str) -> i64 {
    v.connection()
        .execute(
            "INSERT INTO bank_tx(bank_account, op_date, value_date, amount, description,
               import_hash, created_at, balance)
             VALUES (?1, ?2, ?2, ?3, ?4, ?5, '2026-11-16T10:00:00Z', ?6)",
            params![
                acc,
                d.to_string(),
                cents,
                desc,
                format!("{acc}-{d}-{cents}-{desc}"),
                balance
            ],
        )
        .unwrap();
    v.connection().last_insert_rowid()
}

fn lines(v: &Vault, entry: i64) -> Vec<(String, i64, i64)> {
    let mut st = v
        .connection()
        .prepare("SELECT account, debit, credit FROM journal_line WHERE entry_id = ?1 ORDER BY id")
        .unwrap();
    st.query_map([entry], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn has(l: &[(String, i64, i64)], prefix: &str, debit: i64, credit: i64) -> bool {
    l.iter()
        .any(|(a, d, c)| a.starts_with(prefix) && *d == debit && *c == credit)
}

/// Saldo (debe − haber) de las cuentas que empiezan por `prefix` en todo el diario.
fn balance(v: &Vault, prefix: &str) -> i64 {
    v.connection()
        .query_row(
            "SELECT COALESCE(SUM(debit - credit), 0) FROM journal_line WHERE account LIKE ?1 || '%'",
            [prefix],
            |r| r.get(0),
        )
        .unwrap()
}

#[test]
fn cobro_en_la_cuenta_en_dolares_con_diferencia_positiva() {
    let v = setup();
    let inv = invoice_usd(&v, date!(2026 - 03 - 02));
    let acc = account(&v, "US00REVOLUT", "5730001", "Cuenta en dólares", "USD");
    let d = date!(2026 - 04 - 01);
    let tx = mov(&v, acc, d, 100_000, Some(100_000), "PAYMENT CLIENT INC");
    // Se propone por su importe en dólares, aunque aún no haya tipo.
    let sug = suggestions(&v, tx).unwrap();
    assert!(sug.iter().any(|s| s.id == inv), "{sug:?}");
    // Sin el tipo del día no se concilia (el asiento va en euros).
    let err = reconcile_invoice(&v, tx, "emitida", inv, None, NOW)
        .unwrap_err()
        .to_string();
    assert!(err.contains("falta el contravalor"), "{err}");
    // 1 € = 1,20 USD el 1/4: 1.000 USD son 833,33 €; la factura se contabilizó por 800 €.
    bce(&v, d, "1.20");
    let m = movements(&v, true).unwrap();
    let m = m.iter().find(|m| m.id == tx).unwrap();
    assert_eq!(m.amount_eur, Some(Money::from_cents(83_333)));
    assert_eq!(m.fx_source.as_deref(), Some("bce"));
    let e = reconcile_invoice(&v, tx, "emitida", inv, None, NOW).unwrap();
    let l = lines(&v, e);
    assert!(has(&l, "5730001", 83_333, 0), "{l:?}");
    assert!(has(&l, "430", 0, 80_000), "{l:?}");
    assert!(has(&l, "768", 0, 3_333), "{l:?}");
    // El contravalor queda guardado en el movimiento.
    let (eur, src): (i64, String) = v
        .connection()
        .query_row(
            "SELECT amount_eur, fx_source FROM bank_tx WHERE id = ?1",
            [tx],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((eur, src.as_str()), (83_333, "bce"));
    // Saldo de la cuenta: 1.000 USD y 833,33 € en la contabilidad.
    let s = saldos(&v, date!(2026 - 04 - 30)).unwrap();
    let s = s.iter().find(|s| s.account == "5730001").unwrap();
    assert_eq!(s.saldo, Some(Money::from_euros(1000, 0)));
    assert_eq!(s.contable, Money::from_cents(83_333));
}

#[test]
fn pago_desde_la_cuenta_en_dolares_con_diferencia_negativa() {
    let v = setup();
    let r = received_usd(&v, date!(2026 - 03 - 02), "a");
    let r2 = received_usd(&v, date!(2026 - 03 - 03), "b");
    let acc = account(&v, "US00REVOLUT", "5730001", "Cuenta en dólares", "USD");
    let d = date!(2026 - 04 - 01);
    bce(&v, d, "1.20");
    // 500 USD a 1,20 = 416,67 €; contabilizada por 400 €: 16,67 € en contra (668).
    let tx = mov(&v, acc, d, -50_000, None, "SAAS TOOLS");
    let e = reconcile_invoice(&v, tx, "recibida", r, None, NOW).unwrap();
    let l = lines(&v, e);
    assert!(
        has(&l, "41", 40_000, 0) || has(&l, "40", 40_000, 0),
        "{l:?}"
    );
    assert!(has(&l, "5730001", 0, 41_667), "{l:?}");
    assert!(has(&l, "668", 1_667, 0), "{l:?}");
    // Con los euros del extracto tecleados (415,00 €), por ellos.
    let tx2 = mov(&v, acc, d, -50_000, None, "SAAS TOOLS 2");
    assert!(set_contravalor(&v, tx2, Some(Money::from_euros(415, 0)), None, NOW).is_err());
    let c = set_contravalor(&v, tx2, Some(-Money::from_euros(415, 0)), None, NOW)
        .unwrap()
        .unwrap();
    assert_eq!(c.rate.decimal(), "1.204819");
    let e = reconcile_invoice(&v, tx2, "recibida", r2, None, NOW).unwrap();
    let l = lines(&v, e);
    assert!(has(&l, "5730001", 0, 41_500), "{l:?}");
    assert!(has(&l, "668", 1_500, 0), "{l:?}");
    // Ya conciliado, no se cambia.
    assert!(set_contravalor(&v, tx2, None, None, NOW).is_err());
}

#[test]
fn traspaso_de_euros_a_la_cuenta_en_dolares_por_los_euros_reales() {
    let v = setup();
    let eur = account(
        &v,
        "ES9121000418450200051332",
        "5720001",
        "Cuenta en euros",
        "EUR",
    );
    let usd_acc = account(&v, "US00REVOLUT", "5730001", "Cuenta en dólares", "USD");
    let out = mov(
        &v,
        eur,
        date!(2026 - 05 - 04),
        -100_000,
        None,
        "TRASPASO A REVOLUT",
    );
    let inn = mov(
        &v,
        usd_acc,
        date!(2026 - 05 - 05),
        118_000,
        None,
        "TOP-UP EUR",
    );
    // Sin tipo del BCE: la única otra cara del traspaso, con los euros de la cuenta en euros.
    let e = reconcile_account(&v, out, "5730001", "Traspaso a la cuenta en dólares", NOW).unwrap();
    let l = lines(&v, e);
    assert!(has(&l, "5730001", 100_000, 0), "{l:?}");
    assert!(has(&l, "5720001", 0, 100_000), "{l:?}");
    let (entry, eur_amount, src, rate): (i64, i64, String, i64) = v
        .connection()
        .query_row(
            "SELECT entry_id, amount_eur, fx_source, fx_rate FROM bank_tx WHERE id = ?1",
            [inn],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!((entry, eur_amount, src.as_str()), (e, 100_000, "traspaso"));
    assert_eq!(rate, 1_180_000);
    // Deshacer devuelve las dos caras a pendientes y olvida esos euros.
    taxpain_app::conciliados::unreconcile(&v, out, NOW, true).unwrap();
    let left: Option<i64> = v
        .connection()
        .query_row("SELECT amount_eur FROM bank_tx WHERE id = ?1", [inn], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(left, None);
    // Al revés: desde la cuenta en dólares contra la subcuenta de la de euros.
    let e = reconcile_account(&v, inn, "5720001", "", NOW).unwrap();
    let l = lines(&v, e);
    assert!(has(&l, "5730001", 100_000, 0), "{l:?}");
    assert!(has(&l, "5720001", 0, 100_000), "{l:?}");
    let pending = movements(&v, true).unwrap();
    assert!(pending.is_empty(), "{pending:?}");
}

#[test]
fn valoracion_al_cierre_de_banco_cliente_y_proveedor_y_su_reversion() {
    let v = setup();
    let inv = invoice_usd(&v, date!(2026 - 03 - 02));
    let rec = received_usd(&v, date!(2026 - 03 - 02), "a");
    let acc = account(&v, "US00REVOLUT", "5730001", "Cuenta en dólares", "USD");
    // 2.000 USD que entran el 1/6 a 1,25: 1.600 € (capital aportado, p. ej.).
    let d = date!(2026 - 06 - 01);
    bce(&v, d, "1.25");
    let tx = mov(&v, acc, d, 200_000, Some(200_000), "TRANSFER");
    reconcile_account(&v, tx, "5530001", "Aportación del socio", NOW).unwrap();

    // Sin el tipo de cierre: se enseña lo que hay, sin asiento.
    let p = valoracion(&v, 2026, None).unwrap();
    assert_eq!(p.items.len(), 3, "{:?}", p.items);
    assert!(p.lines.is_empty());
    assert!(p.warnings.iter().any(|w| w.contains("Falta el tipo")));
    assert!(post_valoracion(&v, 2026, EntradaValoracion::default(), NOW, true).is_err());

    // 1 € = 1,00 USD el 31/12: el dólar sube.
    bce(&v, date!(2026 - 12 - 31), "1");
    let p = valoracion(&v, 2026, None).unwrap();
    let bank = p.items.iter().find(|i| i.kind == "banco").unwrap();
    assert_eq!(bank.amount, Money::from_euros(2000, 0));
    assert_eq!(bank.booked, Money::from_euros(1600, 0));
    assert_eq!(bank.difference, Some(Money::from_euros(400, 0)));
    let customer = p.items.iter().find(|i| i.kind == "cliente").unwrap();
    assert_eq!(customer.difference, Some(Money::from_euros(200, 0)));
    let supplier = p.items.iter().find(|i| i.kind == "proveedor").unwrap();
    assert_eq!(supplier.difference, Some(-Money::from_euros(100, 0)));
    assert_eq!(p.gains, Money::from_euros(600, 0));
    assert_eq!(p.losses, Money::from_euros(100, 0));
    let checklist = taxpain_app::cierre::checklist(&v, 2026).unwrap();
    let item = checklist.items.iter().find(|i| i.id == "divisa").unwrap();
    assert!(!item.ok);

    assert!(post_valoracion(&v, 2026, EntradaValoracion::default(), NOW, false).is_err());
    let id = post_valoracion(&v, 2026, EntradaValoracion::default(), NOW, true).unwrap();
    let l = lines(&v, id);
    assert!(has(&l, "5730001", 40_000, 0), "{l:?}");
    assert!(has(&l, "430", 20_000, 0), "{l:?}");
    assert!(
        has(&l, "41", 0, 10_000) || has(&l, "40", 0, 10_000),
        "{l:?}"
    );
    assert!(has(&l, "668", 10_000, 0), "{l:?}");
    assert!(has(&l, "768", 0, 60_000), "{l:?}");
    let (date, year): (String, i64) = v
        .connection()
        .query_row(
            "SELECT date, fiscal_year FROM journal_entry WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((date.as_str(), year), ("2026-12-31", 2026));
    // La reversión, el 1 de enero, con las líneas al revés.
    let p = valoracion(&v, 2026, None).unwrap();
    assert!(!p.stale);
    let rev = p.reversal_id.unwrap();
    let r = lines(&v, rev);
    assert!(has(&r, "5730001", 0, 40_000), "{r:?}");
    assert!(has(&r, "768", 60_000, 0), "{r:?}");
    let rdate: String = v
        .connection()
        .query_row("SELECT date FROM journal_entry WHERE id = ?1", [rev], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(rdate, "2027-01-01");
    let checklist = taxpain_app::cierre::checklist(&v, 2026).unwrap();
    assert!(
        checklist
            .items
            .iter()
            .find(|i| i.id == "divisa")
            .unwrap()
            .ok
    );
    // Dos veces, no.
    assert!(post_valoracion(&v, 2026, EntradaValoracion::default(), NOW, true).is_err());

    // Deshacer quita las dos; se rehace con un tipo tecleado (1 € = 1,10 USD).
    undo_valoracion(&v, 2026, NOW, true).unwrap();
    let p = valoracion(&v, 2026, None).unwrap();
    assert!(p.entry_id.is_none() && p.reversal_id.is_none());
    let mut input = EntradaValoracion::default();
    input
        .tipos
        .insert("USD".into(), ExchangeRate::parse("1,10").unwrap());
    let id = post_valoracion(&v, 2026, input, NOW, true).unwrap();
    // Banco: 2.000 / 1,10 = 1.818,18 € (+218,18); cliente 909,09 (+109,09); proveedor
    // 454,55 (−54,55).
    let l = lines(&v, id);
    assert!(has(&l, "5730001", 21_818, 0), "{l:?}");
    assert!(has(&l, "430", 10_909, 0), "{l:?}");
    assert!(has(&l, "668", 5_455, 0), "{l:?}");
    assert!(has(&l, "768", 0, 32_727), "{l:?}");
    // Lo tecleado se recuerda: la propuesta sigue cuadrando con lo contabilizado.
    assert!(!valoracion(&v, 2026, None).unwrap().stale);

    // En 2027 se cobra a 1,05: 952,38 €. Tras la reversión, el cliente vuelve a sus 800 € y el
    // cobro lleva a 768 la diferencia con ellos.
    let d = date!(2027 - 02 - 01);
    bce(&v, d, "1.05");
    let tx = mov(&v, acc, d, 100_000, Some(300_000), "PAYMENT CLIENT INC");
    let e = reconcile_invoice(&v, tx, "emitida", inv, None, NOW).unwrap();
    let l = lines(&v, e);
    assert!(has(&l, "430", 0, 80_000), "{l:?}");
    assert!(has(&l, "768", 0, 15_238), "{l:?}");
    // El cliente queda a cero; lo que se lleva a resultados en los dos años es lo cobrado
    // menos lo facturado: 109,09 en 2026 y 152,38 − 109,09 = 43,29 en 2027.
    assert_eq!(balance(&v, "430"), 0);
    let _ = rec;
}

#[test]
fn sin_partidas_en_divisa_no_hay_paso() {
    let v = setup();
    let p = valoracion(&v, 2026, None).unwrap();
    assert!(p.items.is_empty());
    let checklist = taxpain_app::cierre::checklist(&v, 2026).unwrap();
    assert!(!checklist.items.iter().any(|i| i.id == "divisa"));
}
