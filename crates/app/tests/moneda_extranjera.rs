//! Facturas en moneda extranjera (art. 79.Once LIVA; art. 12.1 RD 1619/2012): se emiten en la
//! moneda y todo lo que cuenta va en euros; cobros, pagos y banco con diferencias de cambio
//! (768/668); recibidas en dólares; plantillas periódicas y presupuestos.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::bank::{import_n43, movements, reconcile_invoice, suggestions};
use taxpain_app::cartera::{list, settle_amount};
use taxpain_app::invoicing::{self, InvoiceDraft, issue, list_issued, preview_full};
use taxpain_app::received::{
    Nature, ReceivedFx, Reviewed, Source, VatRegime, ingest_file, list_received, review_fx,
};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::{quotes, recurring};
use taxpain_core::fx::{ExchangeRate, ForeignCurrency, RateSource};
use taxpain_core::invoice::{
    Address, IdType, InvoiceKind, InvoiceLine, NotSubject, Party, PartyId, RoundingMode,
    VatTreatment,
};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::Date;
use time::macros::{date, datetime};

fn setup() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Agencia Sintética SL".into(),
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
        source: RateSource::Bce,
    }
}

fn us_client() -> Party {
    Party {
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
    }
}

fn line(desc: &str, qty_milli: i64, price: Money) -> InvoiceLine {
    InvoiceLine {
        description: desc.into(),
        quantity_milli: qty_milli,
        unit_price: price,
        discount: Rate::ZERO,
        vat: VatTreatment::NotSubject(NotSubject::N2),
        surcharge: None,
        withholding: None,
        advance: false,
    }
}

/// 10 h × 120,00 + 2,5 × 99,99 = 1.449,98 USD a un cliente de EE. UU. (no sujeta, N2).
fn draft(d: Date, currency: Option<ForeignCurrency>) -> InvoiceDraft {
    InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: d,
        operation_date: None,
        recipient: Some(us_client()),
        lines: vec![
            line("Desarrollo web", 10_000, Money::from_euros(120, 0)),
            line("Hosting anual", 2_500, Money::from_cents(9_999)),
        ],
        rectified: vec![],
        rectified_amounts: None,
        description: "Web".into(),
        notes: None,
        currency,
        oss: None,
    }
}

fn entry_lines(v: &Vault, origin: &str, id: i64) -> Vec<(String, i64, i64)> {
    let mut stmt = v
        .connection()
        .prepare(
            "SELECT l.account, l.debit, l.credit FROM journal_line l
             JOIN journal_entry e ON e.id = l.entry_id
             WHERE e.origin_kind = ?1 AND e.origin_id = ?2 ORDER BY l.id",
        )
        .unwrap();
    stmt.query_map(taxpain_vault::rusqlite::params![origin, id], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

/// Cuenta (por prefijo), debe y haber de un asiento.
fn has(lines: &[(String, i64, i64)], prefix: &str, debit: i64, credit: i64) -> bool {
    lines
        .iter()
        .any(|(a, d, c)| a.starts_with(prefix) && *d == debit && *c == credit)
}

#[test]
fn factura_en_dolares_se_contabiliza_y_declara_en_euros() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    // Sábado: vale el tipo del viernes 23 (1 € = 1,1193 USD).
    let d = draft(
        date!(2026 - 10 - 24),
        Some(usd("1.1193", date!(2026 - 10 - 23))),
    );
    let p = preview_full(&v, &d, None, now).unwrap();
    assert_eq!(p.totals.invoice_total, Money::from_cents(144_998));
    assert_eq!(
        p.eur_totals.as_ref().unwrap().invoice_total,
        Money::from_cents(129_543)
    );
    let out = issue(&v, &d, now, true).unwrap();
    // 1.449,98 / 1,1193 = 1.295,43 €
    assert_eq!(out.totals.invoice_total, Money::from_cents(129_543));
    let (base, total, payable): (i64, i64, i64) = v
        .connection()
        .query_row(
            "SELECT base, total, payable FROM invoice_issued WHERE id = ?1",
            [out.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((base, total, payable), (129_543, 129_543, 129_543));
    let l = list_issued(&v, 2026).unwrap();
    assert_eq!(l[0].currency.as_deref(), Some("USD"));
    assert_eq!(l[0].currency_payable, Some(Money::from_cents(144_998)));
    // Con su tipo, para rectificarla al mismo.
    let rate = l[0].currency_rate.as_ref().unwrap();
    assert_eq!(rate.rate.decimal(), "1.1193");
    assert_eq!(rate.rate_date, date!(2026 - 10 - 23));
    assert_eq!(l[0].payable, Money::from_cents(129_543));

    // Asiento en euros.
    let e = entry_lines(&v, "FacturaEmitida", out.id);
    assert!(has(&e, "430", 129_543, 0), "{e:?}");
    assert!(has(&e, "705", 0, 129_543), "{e:?}");

    // VERI*FACTU en euros (sin moneda en el esquema).
    let xml: String = v
        .connection()
        .query_row(
            "SELECT xml FROM verifactu_record WHERE invoice_id = ?1",
            [out.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(xml.contains(">1295.43<"), "{xml}");
    assert!(!xml.contains("1449.98"), "{xml}");

    // Facturae y UBL en dólares con el contravalor en euros.
    let (_, _, fe) = invoicing::invoice_document(&v, out.id, true).unwrap();
    let fe = String::from_utf8(fe).unwrap();
    assert!(fe.contains("<InvoiceCurrencyCode>USD</InvoiceCurrencyCode>"));
    assert!(fe.contains("<ExchangeRate>1.1193</ExchangeRate>"));
    assert!(fe.contains(
        "<TotalInvoicesAmount><TotalAmount>1449.98</TotalAmount><EquivalentInEuros>1295.43</EquivalentInEuros>"
    ));
    let (_, _, ubl) = invoicing::invoice_ubl(&v, out.id).unwrap();
    let ubl = String::from_utf8(ubl).unwrap();
    assert!(ubl.contains("<cbc:DocumentCurrencyCode>USD</cbc:DocumentCurrencyCode>"));
    assert!(ubl.contains("<cbc:PayableAmount currencyID=\"USD\">1449.98</cbc:PayableAmount>"));

    // «Duplicar»: las líneas en dólares y la moneda, no las líneas en euros.
    let dup = recurring::draft_of(&v, out.id).unwrap();
    assert_eq!(dup.lines[0].unit_price, Money::from_euros(120, 0));
    assert_eq!(dup.lines[0].quantity_milli, 10_000);
    assert_eq!(dup.currency.as_ref().unwrap().code, "USD");
}

#[test]
fn el_tipo_tiene_que_ser_el_del_devengo() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let today = date!(2026 - 10 - 24);
    let err = |c: ForeignCurrency| {
        issue(&v, &draft(today, Some(c)), now, true)
            .unwrap_err()
            .to_string()
    };
    // Un tipo de dos semanas antes no es el vigente.
    assert!(err(usd("1.12", date!(2026 - 10 - 10))).contains("79.Once"));
    // Ni uno posterior a la operación.
    assert!(err(usd("1.12", date!(2026 - 10 - 25))).contains("posterior"));
    // Ni una moneda sin dos decimales.
    let mut jpy = usd("175.51", date!(2026 - 10 - 23));
    jpy.code = "JPY".into();
    assert!(err(jpy).contains("no admitida"));
    // Con fecha de la operación, cuenta la de la operación.
    let mut d = draft(today, Some(usd("1.1205", date!(2026 - 10 - 22))));
    d.operation_date = Some(date!(2026 - 10 - 22));
    issue(&v, &d, now, true).unwrap();
}

#[test]
fn cobros_y_pagos_con_diferencias_de_cambio() {
    let v = setup();
    let now = datetime!(2026-11-10 10:00 UTC);
    let d = date!(2026 - 10 - 23);
    let a = issue(&v, &draft(d, Some(usd("1.1193", d))), now, true).unwrap();
    let b = issue(&v, &draft(d, Some(usd("1.1193", d))), now, true).unwrap();
    let c = list(&v, date!(2026 - 11 - 10)).unwrap();
    let item = c.iter().find(|i| i.invoice_id == a.id).unwrap();
    assert_eq!(item.currency.as_deref(), Some("USD"));
    assert_eq!(item.amount, Money::from_cents(129_543));

    // El banco abonó 1.300,00 €: 4,57 € a favor (768).
    let e = settle_amount(
        &v,
        "emitida",
        a.id,
        1,
        date!(2026 - 11 - 10),
        "572",
        Some(Money::from_euros(1300, 0)),
        now,
    )
    .unwrap();
    let lines = entry_lines_by_id(&v, e);
    assert!(has(&lines, "572", 130_000, 0), "{lines:?}");
    assert!(has(&lines, "430", 0, 129_543), "{lines:?}");
    assert!(has(&lines, "768", 0, 457), "{lines:?}");
    // 1.290,00 €: 5,43 € en contra (668).
    let e = settle_amount(
        &v,
        "emitida",
        b.id,
        1,
        date!(2026 - 11 - 10),
        "570",
        Some(Money::from_euros(1290, 0)),
        now,
    )
    .unwrap();
    let lines = entry_lines_by_id(&v, e);
    assert!(has(&lines, "668", 543, 0), "{lines:?}");
    assert!(has(&lines, "430", 0, 129_543), "{lines:?}");

    // En una factura en euros no hay diferencia de cambio que valga.
    let eur = issue(&v, &draft(d, None), now, true).unwrap();
    let err = settle_amount(
        &v,
        "emitida",
        eur.id,
        1,
        date!(2026 - 11 - 10),
        "572",
        Some(Money::from_euros(1400, 0)),
        now,
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("moneda extranjera"), "{err}");
    // Ni una «diferencia» de más de un 10 %: será un cobro parcial (o un error al teclear), no
    // una pérdida de 595,43 € en la 668.
    let c = issue(&v, &draft(d, Some(usd("1.1193", d))), now, true).unwrap();
    let pay = |cents: i64| {
        settle_amount(
            &v,
            "emitida",
            c.id,
            1,
            date!(2026 - 11 - 10),
            "572",
            Some(Money::from_cents(cents)),
            now,
        )
    };
    let err = pay(70_000).unwrap_err().to_string();
    assert!(err.contains("¿Es un cobro o pago parcial?"), "{err}");
    assert!(pay(115_000).is_err()); // 11,2 % menos
    let st = |v: &Vault| {
        list(v, date!(2026 - 11 - 10))
            .unwrap()
            .into_iter()
            .find(|i| i.invoice_id == c.id)
            .unwrap()
            .status
    };
    assert_eq!(st(&v), "pendiente");
    // Un cobro parcial: se divide el vencimiento (en los euros contabilizados) y se registra la
    // parte cobrada; el resto queda pendiente.
    taxpain_app::cartera::set_schedule(
        &v,
        "emitida",
        c.id,
        Some(&[
            (date!(2026 - 11 - 10), Money::from_euros(700, 0)),
            (date!(2026 - 12 - 10), Money::from_cents(59_543)),
        ]),
        now,
    )
    .unwrap();
    let e = settle_amount(
        &v,
        "emitida",
        c.id,
        1,
        date!(2026 - 11 - 10),
        "572",
        Some(Money::from_euros(702, 0)),
        now,
    )
    .unwrap();
    let lines = entry_lines_by_id(&v, e);
    assert!(has(&lines, "768", 0, 200), "{lines:?}");
    assert!(lines.iter().all(|l| !l.0.starts_with("668")), "{lines:?}");
    let c2 = list(&v, date!(2026 - 11 - 10)).unwrap();
    assert!(
        c2.iter()
            .any(|i| i.invoice_id == c.id && i.seq == 2 && i.status == "pendiente")
    );
    // Dentro del 10 % sí es diferencia de cambio (1.170,00 €: 125,43 € a la 668).
    let d2 = issue(&v, &draft(d, Some(usd("1.1193", d))), now, true).unwrap();
    let e = settle_amount(
        &v,
        "emitida",
        d2.id,
        1,
        date!(2026 - 11 - 10),
        "572",
        Some(Money::from_euros(1170, 0)),
        now,
    )
    .unwrap();
    assert!(has(&entry_lines_by_id(&v, e), "668", 12_543, 0));
}

fn entry_lines_by_id(v: &Vault, entry: i64) -> Vec<(String, i64, i64)> {
    let mut stmt = v
        .connection()
        .prepare("SELECT account, debit, credit FROM journal_line WHERE entry_id = ?1 ORDER BY id")
        .unwrap();
    stmt.query_map([entry], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

const N43: &[u8] = include_bytes!("../../../fixtures/accounting/n43-ejemplo.txt");

#[test]
fn conciliar_con_el_banco_otros_euros() {
    let v = setup();
    let now = datetime!(2026-10-13 10:00 UTC);
    let d = date!(2026 - 09 - 15);
    // 1.350,00 USD a 1,1193 = 1.206,11 €; el banco abonó 1.210,00 € el 1/10.
    let mut dr = draft(d, Some(usd("1.1193", d)));
    dr.lines = vec![line("Campaña", 1000, Money::from_euros(1350, 0))];
    let inv = issue(&v, &dr, now, true).unwrap();
    assert_eq!(inv.totals.payable, Money::from_cents(120_611));
    import_n43(&v, N43, now).unwrap();
    let cobro = movements(&v, true)
        .unwrap()
        .into_iter()
        .find(|m| m.amount == Money::from_euros(1210, 0))
        .unwrap();
    // Se propone aunque no coincida al céntimo (moneda extranjera, menos de un 10 %).
    let sug = suggestions(&v, cobro.id).unwrap();
    assert!(sug.iter().any(|s| s.id == inv.id), "{sug:?}");
    let e = reconcile_invoice(&v, cobro.id, "emitida", inv.id, None, now).unwrap();
    let lines = entry_lines_by_id(&v, e);
    assert!(has(&lines, "572", 121_000, 0), "{lines:?}");
    assert!(has(&lines, "430", 0, 120_611), "{lines:?}");
    assert!(has(&lines, "768", 0, 389), "{lines:?}");
    let c = list(&v, date!(2026 - 10 - 13)).unwrap();
    assert_eq!(
        c.iter().find(|i| i.invoice_id == inv.id).unwrap().status,
        "banco"
    );
}

fn reviewed(d: Date) -> Reviewed {
    Reviewed {
        supplier_nif: "98-7654321".into(),
        supplier_name: "SaaS Tools Inc.".into(),
        number: "INV-2026-001".into(),
        issue_date: d,
        // Los importes en euros los pone la conversión.
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
    }
}

#[test]
fn recibida_en_dolares_con_autorrepercusion_en_euros() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let d = date!(2026 - 10 - 23);
    let fx = |total: i64, rate_date: Date| ReceivedFx {
        currency: usd("1.1193", rate_date),
        base: Money::from_cents(10_000),
        vat: Money::ZERO,
        withholding: Money::ZERO,
        total: Money::from_cents(total),
        vat_eur: None,
    };
    let doc = ingest_file(&v, "saas.xml", b"<x/>", Source::Manual, now).unwrap();
    // Importes que no cuadran o un tipo de otro mes: no se revisa.
    assert!(review_fx(&v, doc.id, &reviewed(d), Some(&fx(12_000, d)), now).is_err());
    assert!(
        review_fx(
            &v,
            doc.id,
            &reviewed(d),
            Some(&fx(10_000, date!(2026 - 09 - 01))),
            now
        )
        .is_err()
    );
    review_fx(&v, doc.id, &reviewed(d), Some(&fx(10_000, d)), now).unwrap();
    // 100,00 USD / 1,1193 = 89,34 €; IVA autorrepercutido 21 % de 89,34 = 18,76 €.
    let (base, total, fxj): (i64, i64, String) = v
        .connection()
        .query_row(
            "SELECT base, total, fx FROM invoice_received WHERE id = ?1",
            [doc.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((base, total), (8_934, 8_934));
    assert!(fxj.contains("\"USD\""), "{fxj}");
    let e = entry_lines(&v, "FacturaRecibida", doc.id);
    assert!(has(&e, "472", 1_876, 0), "{e:?}");
    assert!(has(&e, "477", 0, 1_876), "{e:?}");
    assert!(has(&e, "410", 0, 8_934), "{e:?}");
    let r = list_received(&v, Some("revisada")).unwrap();
    assert_eq!(r[0].currency.as_deref(), Some("USD"));
    assert_eq!(r[0].currency_total, Some(Money::from_cents(10_000)));

    // Pagada con la tarjeta: 90,10 € (0,76 € en contra, 668).
    let e = settle_amount(
        &v,
        "recibida",
        doc.id,
        1,
        date!(2026 - 10 - 24),
        "572",
        Some(Money::from_cents(9_010)),
        now,
    )
    .unwrap();
    let lines = entry_lines_by_id(&v, e);
    assert!(has(&lines, "410", 8_934, 0), "{lines:?}");
    assert!(has(&lines, "572", 0, 9_010), "{lines:?}");
    assert!(has(&lines, "668", 76, 0), "{lines:?}");
}

#[test]
fn plantilla_periodica_pide_el_tipo_del_dia() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let today = date!(2026 - 10 - 24);
    let d = date!(2026 - 09 - 15);
    let first = issue(&v, &draft(d, Some(usd("1.1193", d))), now, true).unwrap();
    let t = recurring::create_from_invoice(&v, first.id, 1, now).unwrap();
    // La plantilla guarda las líneas en dólares y la moneda.
    assert_eq!(t.draft.lines[0].unit_price, Money::from_euros(120, 0));
    assert_eq!(t.draft.currency.as_ref().unwrap().rate_date, d);
    // Toca el 15/10 y se emite hoy: con el tipo de septiembre, no.
    let err = recurring::issue(&v, &t.id, today, now, true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("79.Once"), "{err}");
    let mut gbp = usd("0.83", date!(2026 - 10 - 15));
    gbp.code = "GBP".into();
    assert!(recurring::issue_at_rate(&v, &t.id, today, now, true, Some(gbp)).is_err());
    // Con el del 15/10 (la fecha de la operación), sí.
    let out = recurring::issue_at_rate(
        &v,
        &t.id,
        today,
        now,
        true,
        Some(usd("1.1208", date!(2026 - 10 - 15))),
    )
    .unwrap();
    // 1.449,98 / 1,1208 = 1.293,70 €
    assert_eq!(out.totals.payable, Money::from_cents(129_370));
    let t = recurring::list(&v, today)
        .unwrap()
        .into_iter()
        .find(|r| r.template.id == t.id)
        .unwrap()
        .template;
    assert_eq!(t.issued, 1);
    assert_eq!(t.draft.currency.unwrap().rate_date, date!(2026 - 10 - 15));
}

#[test]
fn presupuesto_en_dolares() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let today = date!(2026 - 10 - 24);
    let d = draft(today, Some(usd("1.1193", date!(2026 - 10 - 23))));
    let q = quotes::create(
        &v,
        &quotes::QuoteInput {
            kind: "presupuesto".into(),
            draft: d,
            valid_until: None,
            language: Some(Language::En),
        },
        now,
    )
    .unwrap();
    // En el panel, su equivalente en euros; en la lista, también en dólares.
    assert_eq!(q.total, Money::from_cents(129_543));
    assert_eq!(q.currency_total, Some(Money::from_cents(144_998)));
    let (_, pdf) = quotes::document(&v, q.id, today).unwrap();
    assert!(pdf.starts_with(b"%PDF-"));
    let (draft, _) = quotes::invoice_draft(&v, q.id, today).unwrap();
    assert_eq!(draft.currency.unwrap().code, "USD");
}

fn balance_of(v: &Vault, prefix: &str) -> i64 {
    v.connection()
        .query_row(
            "SELECT COALESCE(SUM(debit - credit), 0) FROM journal_line WHERE account LIKE ?1 || '%'",
            [prefix],
            |r| r.get(0),
        )
        .unwrap()
}

fn advance_line(desc: &str, price: Money) -> InvoiceLine {
    InvoiceLine {
        advance: true,
        ..line(desc, 1000, price)
    }
}

/// Un anticipo cobrado es una partida no monetaria (NRV 11.ª PGC): la final lo cancela por los
/// euros con que se contabilizó, no a su tipo; el IVA, sobre la base en euros de la final.
#[test]
fn anticipo_en_moneda_se_cancela_por_sus_euros() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    // Anticipo de 1.000 USD a 1,10 el 1/10: 909,09 € a la 438.
    let d1 = date!(2026 - 10 - 01);
    let mut a = draft(d1, Some(usd("1.10", d1)));
    a.lines = vec![advance_line("Anticipo", Money::from_euros(1000, 0))];
    issue(&v, &a, now, true).unwrap();
    assert_eq!(balance_of(&v, "438"), -90_909);
    // Final de 3.000 USD − 1.000 USD de anticipo a 1,20 el 20/10.
    let d2 = date!(2026 - 10 - 20);
    let mut f = draft(d2, Some(usd("1.20", d2)));
    f.lines = vec![
        line("Proyecto", 1000, Money::from_euros(3000, 0)),
        advance_line("Anticipo descontado", -Money::from_euros(1000, 0)),
    ];
    // Descontar más de lo anticipado en dólares no se emite (ni en el borrador).
    let mut too_much = f.clone();
    too_much.lines[1].unit_price = -Money::from_euros(1500, 0);
    let err = preview_full(&v, &too_much, None, now)
        .unwrap_err()
        .to_string();
    assert!(err.contains("1.000,00 USD"), "{err}");
    assert!(issue(&v, &too_much, now, true).is_err());
    let fin = issue(&v, &f, now, true).unwrap();
    // Base en euros de la final (la del IVA y los modelos): 2.000 USD a 1,20.
    assert_eq!(fin.totals.base, Money::from_cents(166_667));
    // La 438 queda saldada y ventas recoge 2.500,00 + 75,76 de diferencia.
    assert_eq!(balance_of(&v, "438"), 0);
    assert_eq!(balance_of(&v, "705"), -257_576);
    let e = entry_lines(&v, "FacturaEmitida", fin.id);
    assert!(has(&e, "438", 90_909, 0), "{e:?}");
    assert!(has(&e, "430", 166_667, 0), "{e:?}");
    // Ya no queda anticipo en dólares que descontar.
    let mut again = draft(date!(2026 - 10 - 21), Some(usd("1.20", d2)));
    again.lines = vec![
        line("Más", 1000, Money::from_euros(500, 0)),
        advance_line("Anticipo", -Money::from_euros(1, 0)),
    ];
    assert!(issue(&v, &again, now, true).is_err());
}

#[test]
fn anticipo_en_moneda_descontado_en_dos_veces() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let d1 = date!(2026 - 10 - 01);
    let mut a = draft(d1, Some(usd("1.10", d1)));
    a.lines = vec![advance_line("Anticipo", Money::from_euros(1000, 0))];
    issue(&v, &a, now, true).unwrap();
    // 300 USD el 10/10 (a 1,15) y los 700 USD restantes el 20/10 (a 1,20): la 438, por su
    // parte de los 909,09 € (272,73 €) y por lo que queda (636,36 €).
    for (d, rate, usd_amount) in [
        (date!(2026 - 10 - 10), "1.15", 300),
        (date!(2026 - 10 - 20), "1.20", 700),
    ] {
        let mut f = draft(d, Some(usd(rate, d)));
        f.lines = vec![
            line("Fase", 1000, Money::from_euros(2000, 0)),
            advance_line("Anticipo descontado", -Money::from_euros(usd_amount, 0)),
        ];
        let out = issue(&v, &f, now, true).unwrap();
        let e = entry_lines(&v, "FacturaEmitida", out.id);
        let expected = if usd_amount == 300 { 27_273 } else { 63_636 };
        assert!(has(&e, "438", expected, 0), "{e:?}");
    }
    assert_eq!(balance_of(&v, "438"), 0);
}

/// El IVA deducible de una recibida en otra moneda es el que figura en euros en la factura
/// (art. 12 RD 1619/2012): la base en euros por el tipo, no la cuota en la moneda convertida.
#[test]
fn recibida_en_moneda_con_el_iva_en_euros_de_la_factura() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let d = date!(2026 - 10 - 23);
    // 100,02 USD + 21 % (21,00 USD) a 1,1193: base 89,36 €; la cuota en euros es 89,36 × 21 %
    // = 18,77 €, mientras que 21,00 USD / 1,1193 = 18,76 €.
    let (base, vat) = (Money::from_cents(10_002), Money::from_cents(2_100));
    let rate = taxpain_core::fx::ExchangeRate::parse("1.1193").unwrap();
    assert_eq!(rate.to_eur(vat), Money::from_cents(1_876));
    assert_eq!(
        taxpain_app::received::euro_vat(base, vat, rate.to_eur(base)),
        Some(Money::from_cents(1_877))
    );
    let fx = |vat_eur: Option<Money>| ReceivedFx {
        currency: usd("1.1193", d),
        base,
        vat,
        withholding: Money::ZERO,
        total: base + vat,
        vat_eur,
    };
    let mut r = reviewed(d);
    r.vat_regime = VatRegime::Interior;
    r.supplier_nif = "B98765431".into();
    r.supplier_country = None;
    let doc = ingest_file(&v, "a.xml", b"<a/>", Source::Manual, now).unwrap();
    // Sin indicarlo: la base en euros por el 21 %.
    review_fx(&v, doc.id, &r, Some(&fx(None)), now).unwrap();
    let (b, iva): (i64, i64) = v
        .connection()
        .query_row(
            "SELECT base, vat FROM invoice_received WHERE id = ?1",
            [doc.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((b, iva), (8_936, 1_877));
    // Indicado: el de la factura; muy distinto del convertido, no.
    let doc2 = ingest_file(&v, "b.xml", b"<b/>", Source::Manual, now).unwrap();
    let mut r2 = r.clone();
    r2.number = "INV-2026-002".into();
    assert!(
        review_fx(
            &v,
            doc2.id,
            &r2,
            Some(&fx(Some(Money::from_cents(2_500)))),
            now
        )
        .is_err()
    );
    review_fx(
        &v,
        doc2.id,
        &r2,
        Some(&fx(Some(Money::from_cents(1_876)))),
        now,
    )
    .unwrap();
    let (iva, total): (i64, i64) = v
        .connection()
        .query_row(
            "SELECT vat, total FROM invoice_received WHERE id = ?1",
            [doc2.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!((iva, total), (1_876, 8_936 + 1_876));
}
