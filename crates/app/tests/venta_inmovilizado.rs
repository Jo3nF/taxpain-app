//! Venta del inmovilizado con su factura (CONTABILIDAD-08): el ingreso cuenta una sola vez (como
//! resultado de la baja, nunca también como ventas), en los dos órdenes (factura antes o después
//! de la baja), con la amortización del ejercicio ya registrada, en el intangible (670/770) y en
//! las inversiones inmobiliarias (672/772); la factura vinculada no se anula y sus rectificativas
//! pasan al resultado. Cada venta acaba con beneficio o con pérdida, no con los dos; la factura no
//! se vincula si la 543 ya no tiene el precio; la de un ejercicio anterior a la baja se vincula en
//! el suyo; los asientos que se borran solos no se llevan sus documentos. Importes calculados a
//! mano con el prorrateo por días del programa (cuota anual × días en uso / 365, redondeada al
//! céntimo).

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::{
    accrued_depreciation, annual_accounts, attach_document, close_year, delete_manual_entry,
    dispose_asset, dispose_asset_with_invoice, entry_documents, fixed_assets, journal,
    manual_entry, post_depreciation, set_entry_notes, trial_balance,
};
use taxpain_app::apertura::add_asset_with_prior;
use taxpain_app::cierre;
use taxpain_app::invoicing::{self, InvoiceDraft, IssuedInvoice, cancel, issue};
use taxpain_app::models::{self, ModelSettings};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::venta_inmovilizado::{
    asset_sales, link_sale_invoice, sale_invoice_candidates, unlink_sale_invoice,
};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::depreciation::{FixedAsset, Method};
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{
    Exemption, InvoiceKind, InvoiceLine, Party, PartyId, Rectification, RectificationMethod,
    RoundingMode, VatTreatment,
};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime};

const NOW: OffsetDateTime = datetime!(2026-10-31 10:00 UTC);

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn cents(c: i64) -> Money {
    Money::from_cents(c)
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
    };
    invoicing::save_settings(&v, &s, datetime!(2026-01-01 9:00 UTC)).unwrap();
    v
}

fn taxed() -> VatTreatment {
    VatTreatment::Taxed {
        rate: Rate::percent(21),
    }
}

fn line(description: &str, euros: i64, vat: VatTreatment) -> InvoiceLine {
    InvoiceLine {
        description: description.into(),
        quantity_milli: 1000,
        unit_price: eur(euros),
        discount: Rate::ZERO,
        vat,
        surcharge: None,
        withholding: None,
        advance: false,
    }
}

fn draft(lines: Vec<InvoiceLine>, date: Date) -> InvoiceDraft {
    InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: date,
        operation_date: None,
        recipient: Some(Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Cliente Uno SL".into(),
            address: addr(),
            person: None,
        }),
        lines,
        rectified: vec![],
        rectified_amounts: None,
        description: "Venta de inmovilizado".into(),
        notes: None,
        currency: None,
        oss: None,
    }
}

/// Factura de venta de `euros` más el 21 % de IVA.
fn invoice(v: &Vault, euros: i64, date: Date) -> IssuedInvoice {
    issue(
        v,
        &draft(vec![line("Máquina usada", euros, taxed())], date),
        NOW,
        true,
    )
    .unwrap()
}

/// Rectificativa por diferencias de `euros` (negativo: rebaja) de la factura `of`.
fn rectify(v: &Vault, euros: i64, date: Date, of: &IssuedInvoice, of_date: Date) -> IssuedInvoice {
    let mut d = draft(vec![line("Rebaja del precio", euros, taxed())], date);
    d.kind = InvoiceKind::Rectificativa(Rectification {
        code: 1,
        method: RectificationMethod::Diferencias,
    });
    d.rectified = vec![(of.number.clone(), of_date)];
    issue(v, &d, NOW, true).unwrap()
}

/// Elemento comprado antes de TaxPain y amortizado hasta el 31/12/2025, con su apertura del
/// 1/1/2026 (coste al debe y amortización acumulada al haber, contra capital).
fn asset(
    v: &Vault,
    name: &str,
    (account, depreciation, expense): (&str, &str, &str),
    acquired: Date,
    (cost, rate, prior): (i64, i32, i64),
) -> i64 {
    manual_entry(
        v,
        &JournalEntry {
            date: date!(2026 - 01 - 01),
            concept: format!("Apertura {name}"),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc(account), eur(cost)),
                EntryLine::credit(acc(depreciation), eur(prior)),
                EntryLine::credit(acc("100"), eur(cost - prior)),
            ],
        },
        NOW,
    )
    .unwrap();
    let a = FixedAsset {
        id: 0,
        name: name.into(),
        account: acc(account),
        depreciation_account: acc(depreciation),
        expense_account: acc(expense),
        acquired_on: acquired,
        cost: eur(cost),
        residual: Money::ZERO,
        rate: Rate::percent(rate),
        disposed_on: None,
        method: Method::Lineal,
        tax_rate: None,
    };
    add_asset_with_prior(v, &a, eur(prior), Some(date!(2025 - 12 - 31)), NOW).unwrap()
}

/// «Envasadora» (213): 12.000 € del 1/1/2024 al 10 % (1.200 al año), 2.400 amortizados al
/// empezar 2026. A 30/6/2026 la cuota es 1.200 × 181/365 = 595,07, la acumulada 2.995,07 y el
/// valor neto 9.004,93.
fn envasadora(v: &Vault) -> i64 {
    asset(
        v,
        "Envasadora",
        ("213", "2813", "681"),
        date!(2024 - 01 - 01),
        (12_000, 10, 2_400),
    )
}

/// Saldo deudor (positivo) o acreedor (negativo) de la cuenta en el ejercicio.
fn saldo(v: &Vault, year: i32, account: &str) -> Money {
    trial_balance(v, year, None)
        .unwrap()
        .iter()
        .filter(|b| b.account.as_str() == account)
        .map(|b| b.balance())
        .sum()
}

fn cuadra(v: &Vault, year: i32) {
    let b = trial_balance(v, year, None).unwrap();
    let debe: Money = b.iter().map(|x| x.debit).sum();
    let haber: Money = b.iter().map(|x| x.credit).sum();
    assert_eq!(debe, haber, "sumas y saldos de {year}");
}

/// Saldos de la venta de la envasadora por 10.000 € + IVA con su factura (S1 y S2): sólo queda
/// el cobro en la 430, el IVA en la 477, la amortización hasta la baja y el beneficio.
fn saldos_de_la_venta(v: &Vault) {
    for a in ["213", "2813", "543", "705", "671"] {
        assert_eq!(saldo(v, 2026, a), Money::ZERO, "{a}");
    }
    assert_eq!(saldo(v, 2026, "4300001"), eur(12_100));
    assert_eq!(saldo(v, 2026, "477"), -eur(2_100));
    assert_eq!(saldo(v, 2026, "771"), -cents(99_507), "12.000 − 2.995,07");
    assert_eq!(saldo(v, 2026, "681"), cents(59_507));
    cuadra(v, 2026);
    // Una sola línea de ingreso: la venta no está en la cifra de negocios.
    let pyg = annual_accounts(v, 2026).unwrap().income;
    assert_eq!(pyg.get("1"), Money::ZERO);
    assert_eq!(pyg.get("8"), -cents(59_507));
    assert_eq!(pyg.get("11"), cents(99_507));
    let balance = annual_accounts(v, 2026).unwrap().balance;
    assert_eq!(balance.get("B.IV"), Money::ZERO, "543 a cero");
}

/// S1: la factura primero y la baja eligiéndola.
#[test]
fn factura_primero_y_baja_con_ella() {
    let v = setup();
    let id = envasadora(&v);
    let f = invoice(&v, 10_000, date!(2026 - 06 - 30));
    let c = sale_invoice_candidates(&v, id, "").unwrap();
    assert_eq!(c.len(), 1);
    assert_eq!(
        (c[0].invoice_id, c[0].base, c[0].open, c[0].suggested),
        (f.id, eur(10_000), eur(10_000), Some(eur(10_000)))
    );
    assert_eq!(c[0].recipient_name.as_deref(), Some("Cliente Uno SL"));

    // Sin precio no hay venta con factura.
    assert!(
        dispose_asset_with_invoice(&v, id, date!(2026 - 06 - 30), Money::ZERO, Some(f.id), NOW)
            .is_err()
    );
    // Ni por más de lo que la factura lleva a ventas.
    assert!(
        dispose_asset_with_invoice(&v, id, date!(2026 - 06 - 30), eur(10_001), Some(f.id), NOW)
            .is_err()
    );
    let d = dispose_asset_with_invoice(&v, id, date!(2026 - 06 - 30), eur(10_000), Some(f.id), NOW)
        .unwrap();
    assert_eq!(d.result, cents(99_507));
    assert_eq!(d.depreciation_removed, Money::ZERO);
    assert!(d.depreciation_entry.is_some());
    assert!(d.warnings.is_empty(), "{:?}", d.warnings);
    saldos_de_la_venta(&v);

    // El asiento que pasa la factura: 705 a 543, con la fecha de la factura.
    let sale = journal(&v, 2026)
        .unwrap()
        .into_iter()
        .find(|e| Some(e.id) == d.sale_entry)
        .unwrap();
    assert_eq!(sale.entry.date, date!(2026 - 06 - 30));
    assert_eq!(sale.entry.origin, Origin::BajaInmovilizado(id));
    assert_eq!(
        sale.entry.concept,
        format!("Venta de Envasadora: factura {}", f.number)
    );
    assert_eq!(
        sale.entry.lines,
        vec![
            EntryLine::debit(acc("705"), eur(10_000)),
            EntryLine::credit(acc("543"), eur(10_000)),
        ]
    );
    let s = &asset_sales(&v).unwrap()[0];
    assert_eq!(
        (s.price, s.invoiced, s.unbilled, s.result),
        (eur(10_000), eur(10_000), Money::ZERO, cents(99_507))
    );
    assert_eq!(s.invoices[0].number, f.number);
    assert!(s.pending.is_empty());
    // Ya no se propone para otra venta: está vinculada entera.
    let otra = envasadora(&v);
    assert!(sale_invoice_candidates(&v, otra, "").unwrap().is_empty());
}

/// S1 con pérdida: 8.000 € + IVA por un valor neto de 9.004,93.
#[test]
fn venta_con_perdida() {
    let v = setup();
    let id = envasadora(&v);
    let f = invoice(&v, 8_000, date!(2026 - 06 - 30));
    let d = dispose_asset_with_invoice(&v, id, date!(2026 - 06 - 30), eur(8_000), Some(f.id), NOW)
        .unwrap();
    assert_eq!(d.result, -cents(100_493));
    assert_eq!(saldo(&v, 2026, "671"), cents(100_493));
    assert_eq!(saldo(&v, 2026, "4300001"), eur(9_680));
    assert_eq!(saldo(&v, 2026, "477"), -eur(1_680));
    for a in ["213", "2813", "543", "705", "771"] {
        assert_eq!(saldo(&v, 2026, a), Money::ZERO, "{a}");
    }
    cuadra(&v, 2026);
}

/// S2: la baja primero (el precio queda en la 543) y la factura después, vinculada desde
/// Inmovilizado. El cierre avisa mientras falta.
#[test]
fn baja_primero_y_factura_despues() {
    let v = setup();
    let id = envasadora(&v);
    let d = dispose_asset(&v, id, date!(2026 - 06 - 30), eur(10_000), NOW).unwrap();
    assert_eq!(d.result, cents(99_507));
    assert_eq!(d.sale_entry, None);
    assert_eq!(saldo(&v, 2026, "543"), eur(10_000));
    let s = &asset_sales(&v).unwrap()[0];
    assert_eq!((s.unbilled, s.invoiced), (eur(10_000), Money::ZERO));
    let item = |v: &Vault| {
        cierre::checklist(v, 2026)
            .unwrap()
            .items
            .into_iter()
            .find(|i| i.id == "ventas_inmovilizado")
            .unwrap()
    };
    let i = item(&v);
    assert!(!i.ok);
    assert!(
        i.detail
            .contains("«Envasadora» sin factura vinculada (10.000,00 € en la 543)"),
        "{}",
        i.detail
    );

    let f = invoice(&v, 10_000, date!(2026 - 07 - 05));
    let c = sale_invoice_candidates(&v, id, "").unwrap();
    assert_eq!((c[0].invoice_id, c[0].suggested), (f.id, Some(eur(10_000))));
    let l = link_sale_invoice(&v, id, f.id, None, NOW).unwrap();
    assert_eq!((l.amount, l.result_change), (eur(10_000), Money::ZERO));
    let e = journal(&v, 2026)
        .unwrap()
        .into_iter()
        .find(|e| e.id == l.entry_id)
        .unwrap();
    assert_eq!(
        e.entry.date,
        date!(2026 - 07 - 05),
        "con la fecha de la factura"
    );
    saldos_de_la_venta(&v);
    assert!(item(&v).ok);
    assert!(
        link_sale_invoice(&v, id, f.id, None, NOW).is_err(),
        "ya vinculada"
    );
}

/// S2b: la base de la factura no es el precio de la baja: se salda toda la 543 y la diferencia
/// va al resultado de la venta. Con beneficio, una base menor lo rebaja (Debe 771), no da una
/// pérdida aparte: la venta acaba con un solo resultado.
#[test]
fn la_factura_fija_el_precio() {
    for (base, gain, loss, result) in [
        (10_500, -cents(149_507), Money::ZERO, cents(149_507)),
        (9_800, -cents(79_507), Money::ZERO, cents(79_507)),
    ] {
        let v = setup();
        let id = envasadora(&v);
        dispose_asset(&v, id, date!(2026 - 06 - 30), eur(10_000), NOW).unwrap();
        let f = invoice(&v, base, date!(2026 - 07 - 05));
        let l = link_sale_invoice(&v, id, f.id, Some(eur(base)), NOW).unwrap();
        assert_eq!(l.result_change, eur(base - 10_000));
        assert_eq!(saldo(&v, 2026, "543"), Money::ZERO);
        assert_eq!(saldo(&v, 2026, "705"), Money::ZERO);
        assert_eq!(saldo(&v, 2026, "771"), gain);
        assert_eq!(saldo(&v, 2026, "671"), loss);
        assert_eq!(asset_sales(&v).unwrap()[0].result, result);
        assert_eq!(annual_accounts(&v, 2026).unwrap().income.get("11"), result);
        cuadra(&v, 2026);
    }
}

/// S5: con la amortización de 2026 ya registrada hasta el cierre (1.200), la baja del 30/6 la
/// rehace hasta ese día (595,07; se quitan 604,93 = 1.200 × 184/365) en vez de rechazarla, y
/// después ya no se amortiza.
#[test]
fn baja_con_la_amortizacion_del_ejercicio_ya_registrada() {
    let v = setup();
    let id = envasadora(&v);
    let posted = post_depreciation(&v, 2026, NOW).unwrap();
    assert_eq!(posted.len(), 1);
    assert_eq!(saldo(&v, 2026, "681"), eur(1_200));
    let f = invoice(&v, 10_000, date!(2026 - 06 - 30));
    let d = dispose_asset_with_invoice(&v, id, date!(2026 - 06 - 30), eur(10_000), Some(f.id), NOW)
        .unwrap();
    assert_eq!(d.depreciation_removed, cents(60_493));
    assert_eq!(
        d.depreciation_entry,
        Some(posted[0]),
        "el mismo asiento, rehecho"
    );
    assert_eq!(d.result, cents(99_507));
    let amortizaciones: Vec<_> = journal(&v, 2026)
        .unwrap()
        .into_iter()
        .filter(|e| matches!(e.entry.origin, Origin::Amortizacion(_)))
        .collect();
    assert_eq!(amortizaciones.len(), 1);
    assert_eq!(amortizaciones[0].entry.date, date!(2026 - 06 - 30));
    assert_eq!(amortizaciones[0].entry.total_debit(), cents(59_507));
    saldos_de_la_venta(&v);
    assert_eq!(
        fixed_assets(&v).unwrap()[0].disposed_on,
        Some(date!(2026 - 06 - 30))
    );
    assert!(post_depreciation(&v, 2026, NOW).unwrap().is_empty());
    assert!(post_depreciation(&v, 2027, NOW).unwrap().is_empty());
    // Los trimestres: hasta el 31/3 se devengan 1.200 × 90/365 = 295,89 (aún sin asiento); desde
    // la baja, nada más que lo del diario.
    assert_eq!(
        accrued_depreciation(&v, 2026, date!(2026 - 03 - 31)).unwrap(),
        cents(29_589)
    );
    assert_eq!(
        accrued_depreciation(&v, 2026, date!(2026 - 09 - 30)).unwrap(),
        Money::ZERO
    );
}

/// Un autónomo en estimación directa: la venta no es ingreso de la actividad en el 130 (es una
/// ganancia patrimonial, art. 28.2 LIRPF) y la amortización hasta la baja sí es gasto.
#[test]
fn el_130_no_cuenta_la_venta_como_ingreso() {
    let v = setup();
    let mut years = std::collections::BTreeMap::new();
    years.insert(
        2026,
        models::YearSettings {
            irpf_regime: Some(taxpain_models::m100::IrpfRegime::Normal),
            ..models::YearSettings::default()
        },
    );
    models::save_settings(
        &v,
        &ModelSettings {
            years,
            ..ModelSettings::default()
        },
        NOW,
    )
    .unwrap();
    let id = envasadora(&v);
    post_depreciation(&v, 2026, NOW).unwrap();
    let f = invoice(&v, 10_000, date!(2026 - 06 - 30));
    dispose_asset_with_invoice(&v, id, date!(2026 - 06 - 30), eur(10_000), Some(f.id), NOW)
        .unwrap();
    let casilla = |q: u8, b: u16| {
        models::draft(&v, "130", 2026, q)
            .unwrap()
            .boxes
            .iter()
            .find(|(k, _)| *k == b)
            .map_or(Money::ZERO, |(_, m)| *m)
    };
    assert_eq!(casilla(2, 1), Money::ZERO, "ingresos");
    assert_eq!(casilla(3, 1), Money::ZERO);
    assert_eq!(
        casilla(2, 2),
        cents(59_507),
        "gastos: la amortización hasta la baja"
    );
    assert_eq!(casilla(3, 2), cents(59_507));
}

/// Baja con fecha de un ejercicio anterior a amortizaciones ya registradas: la del ejercicio
/// siguiente sobra y se quita.
#[test]
fn la_amortizacion_posterior_a_la_baja_se_quita() {
    let v = setup();
    let id = envasadora(&v);
    post_depreciation(&v, 2026, NOW).unwrap();
    post_depreciation(&v, 2027, NOW).unwrap();
    assert_eq!(saldo(&v, 2027, "681"), eur(1_200));
    let d = dispose_asset(&v, id, date!(2026 - 06 - 30), eur(10_000), NOW).unwrap();
    assert!(
        d.warnings.iter().any(|w| w.contains("2027")),
        "{:?}",
        d.warnings
    );
    assert_eq!(saldo(&v, 2027, "681"), Money::ZERO);
    assert_eq!(saldo(&v, 2026, "681"), cents(59_507));
    assert_eq!(saldo(&v, 2026, "2813"), Money::ZERO);
    assert_eq!(saldo(&v, 2026, "771"), -cents(99_507));
}

/// S8: inversiones inmobiliarias (22x) a la 772/672. Local de 200.000 € (construcción) del
/// 1/1/2016 al 2 % (4.000 al año), 40.000 amortizados al empezar 2026; baja el 30/9/2026:
/// 4.000 × 273/365 = 2.991,78, acumulada 42.991,78, valor neto 157.008,22. La factura (exenta,
/// art. 20.Uno.22.º LIVA) lleva también el suelo, que no está en el inventario: esa parte sigue
/// en ventas y se avisa.
#[test]
fn inversiones_inmobiliarias_a_la_772() {
    let v = setup();
    let id = asset(
        &v,
        "Local",
        ("221", "2821", "682"),
        date!(2016 - 01 - 01),
        (200_000, 2, 40_000),
    );
    let exempt = || VatTreatment::Exempt(Exemption::E1);
    let f = issue(
        &v,
        &draft(
            vec![
                line("Local: construcción", 180_000, exempt()),
                line("Local: suelo", 70_000, exempt()),
            ],
            date!(2026 - 09 - 30),
        ),
        NOW,
        true,
    )
    .unwrap();
    let c = sale_invoice_candidates(&v, id, "").unwrap();
    assert_eq!(c[0].suggested, Some(eur(250_000)), "sin baja, lo que queda");
    let d =
        dispose_asset_with_invoice(&v, id, date!(2026 - 09 - 30), eur(180_000), Some(f.id), NOW)
            .unwrap();
    assert_eq!(d.result, cents(2_299_178));
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("El resto de") && w.contains("70.000,00 €")),
        "{:?}",
        d.warnings
    );
    assert_eq!(saldo(&v, 2026, "772"), -cents(2_299_178));
    assert_eq!(saldo(&v, 2026, "682"), cents(299_178));
    for a in ["221", "2821", "543", "771", "671", "672"] {
        assert_eq!(saldo(&v, 2026, a), Money::ZERO, "{a}");
    }
    assert_eq!(saldo(&v, 2026, "705"), -eur(70_000));
    assert_eq!(saldo(&v, 2026, "4300001"), eur(250_000));
    cuadra(&v, 2026);
}

/// S7: intangible (20x) a la 670/770. Aplicación de 6.000 € del 1/1/2025 al 25 % (1.500 al año),
/// baja el 31/3/2026: 1.500 × 90/365 = 369,86; valor neto 4.130,14; vendida por 3.000 + IVA:
/// pérdida de 1.130,14.
#[test]
fn intangible_a_la_670() {
    let v = setup();
    let id = asset(
        &v,
        "Aplicación",
        ("206", "2806", "680"),
        date!(2025 - 01 - 01),
        (6_000, 25, 1_500),
    );
    let f = invoice(&v, 3_000, date!(2026 - 03 - 31));
    let d = dispose_asset_with_invoice(&v, id, date!(2026 - 03 - 31), eur(3_000), Some(f.id), NOW)
        .unwrap();
    assert_eq!(d.result, -cents(113_014));
    assert_eq!(saldo(&v, 2026, "670"), cents(113_014));
    assert_eq!(saldo(&v, 2026, "680"), cents(36_986));
    for a in ["206", "2806", "543", "705", "671", "771"] {
        assert_eq!(saldo(&v, 2026, a), Money::ZERO, "{a}");
    }
    assert_eq!(saldo(&v, 2026, "4300001"), eur(3_630));
    assert_eq!(saldo(&v, 2026, "477"), -eur(630));
    cuadra(&v, 2026);
}

/// La factura vinculada no se puede anular: primero se desvincula (y el precio vuelve a la 543).
#[test]
fn la_factura_vinculada_no_se_anula() {
    let v = setup();
    let id = envasadora(&v);
    let f = invoice(&v, 10_000, date!(2026 - 10 - 15));
    dispose_asset_with_invoice(&v, id, date!(2026 - 10 - 15), eur(10_000), Some(f.id), NOW)
        .unwrap();
    let e = cancel(&v, f.id, NOW, true).unwrap_err().to_string();
    assert!(e.contains("es la venta de «Envasadora»"), "{e}");
    assert!(unlink_sale_invoice(&v, id, f.id, NOW, false).is_err());
    unlink_sale_invoice(&v, id, f.id, NOW, true).unwrap();
    assert_eq!(saldo(&v, 2026, "543"), eur(10_000));
    assert_eq!(saldo(&v, 2026, "705"), -eur(10_000));
    cancel(&v, f.id, NOW, true).unwrap();
    assert_eq!(saldo(&v, 2026, "705"), Money::ZERO);
    assert_eq!(saldo(&v, 2026, "4300001"), Money::ZERO);
    assert_eq!(
        saldo(&v, 2026, "543"),
        eur(10_000),
        "la venta sigue sin factura"
    );
    let e = link_sale_invoice(&v, id, f.id, None, NOW)
        .unwrap_err()
        .to_string();
    assert!(e.contains("anulada"), "{e}");
    cuadra(&v, 2026);
}

/// S9: la rectificativa de una factura que vende un solo elemento entero pasa sola al resultado
/// (una rebaja de 500 quita 500 del beneficio de 995,07: Debe 771); no se anula ni se desvincula
/// la factura de venta antes que ella.
#[test]
fn la_rectificativa_pasa_sola_al_resultado() {
    let v = setup();
    let id = envasadora(&v);
    let f = invoice(&v, 10_000, date!(2026 - 06 - 30));
    dispose_asset_with_invoice(&v, id, date!(2026 - 06 - 30), eur(10_000), Some(f.id), NOW)
        .unwrap();
    let r = rectify(&v, -500, date!(2026 - 07 - 15), &f, date!(2026 - 06 - 30));
    let aviso = r.export_error.clone().unwrap_or_default();
    assert!(
        aviso.contains("pasa al resultado de la venta de «Envasadora»"),
        "{aviso}"
    );
    assert_eq!(saldo(&v, 2026, "705"), Money::ZERO);
    assert_eq!(saldo(&v, 2026, "671"), Money::ZERO);
    assert_eq!(saldo(&v, 2026, "771"), -cents(49_507), "9.500 − 9.004,93");
    assert_eq!(saldo(&v, 2026, "4300001"), eur(11_495));
    assert_eq!(saldo(&v, 2026, "477"), -eur(1_995));
    let s = &asset_sales(&v).unwrap()[0];
    assert_eq!((s.invoiced, s.result), (eur(9_500), cents(49_507)));
    assert_eq!(s.invoices.len(), 2);
    assert_eq!(s.invoices[1].rectifies.as_deref(), Some(f.number.as_str()));
    assert_eq!(
        annual_accounts(&v, 2026).unwrap().income.get("11"),
        cents(49_507)
    );
    cuadra(&v, 2026);

    let e = unlink_sale_invoice(&v, id, f.id, NOW, true)
        .unwrap_err()
        .to_string();
    assert!(e.contains(&r.number), "{e}");
    let e = cancel(&v, r.id, NOW, true).unwrap_err().to_string();
    assert!(e.contains("«Envasadora»"), "{e}");
    // Desvinculada, queda pendiente con su importe propuesto y se vuelve a pasar.
    unlink_sale_invoice(&v, id, r.id, NOW, true).unwrap();
    assert_eq!(saldo(&v, 2026, "705"), eur(500));
    let p = &asset_sales(&v).unwrap()[0].pending;
    assert_eq!(
        (p[0].invoice_id, p[0].suggested, p[0].cancels_sale),
        (r.id, Some(-eur(500)), false)
    );
    let l = link_sale_invoice(&v, id, r.id, None, NOW).unwrap();
    assert_eq!((l.amount, l.result_change), (-eur(500), -eur(500)));
    assert_eq!(saldo(&v, 2026, "705"), Money::ZERO);
    assert_eq!(saldo(&v, 2026, "771"), -cents(49_507));
    assert_eq!(saldo(&v, 2026, "671"), Money::ZERO);
}

/// S10: una rectificativa que anula toda la venta no se pasa sola (TaxPain no deshace la baja):
/// se avisa y queda pendiente.
#[test]
fn la_rectificativa_que_anula_la_venta_solo_avisa() {
    let v = setup();
    let id = envasadora(&v);
    let f = invoice(&v, 10_000, date!(2026 - 06 - 30));
    dispose_asset_with_invoice(&v, id, date!(2026 - 06 - 30), eur(10_000), Some(f.id), NOW)
        .unwrap();
    let r = rectify(
        &v,
        -10_000,
        date!(2026 - 07 - 15),
        &f,
        date!(2026 - 06 - 30),
    );
    let aviso = r.export_error.clone().unwrap_or_default();
    assert!(aviso.contains("anula la venta"), "{aviso}");
    let p = &asset_sales(&v).unwrap()[0].pending;
    assert_eq!((p[0].invoice_id, p[0].cancels_sale), (r.id, true));
    assert_eq!(saldo(&v, 2026, "671"), Money::ZERO);
    let i = cierre::checklist(&v, 2026)
        .unwrap()
        .items
        .into_iter()
        .find(|i| i.id == "ventas_inmovilizado")
        .unwrap();
    assert!(!i.ok);
    assert!(i.detail.contains(&r.number), "{}", i.detail);
}

/// Qué facturas se proponen y qué no se deja vincular.
#[test]
fn facturas_que_se_pueden_vincular() {
    let v = setup();
    let id = envasadora(&v);
    // Una de antes del alta (1/1/2024), una de un ejercicio anterior a la baja, una de servicios,
    // la de la venta, una anulada y una rectificativa (de la de servicios).
    let before = invoice(&v, 1_000, date!(2023 - 12 - 01));
    let old = invoice(&v, 1_000, date!(2025 - 12 - 20));
    let services = invoice(&v, 2_000, date!(2026 - 03 - 01));
    let f = invoice(&v, 10_000, date!(2026 - 06 - 30));
    let cancelled = invoice(&v, 500, date!(2026 - 10 - 20));
    cancel(&v, cancelled.id, NOW, true).unwrap();
    rectify(
        &v,
        -200,
        date!(2026 - 10 - 21),
        &services,
        date!(2026 - 03 - 01),
    );
    assert!(
        link_sale_invoice(&v, id, f.id, None, NOW).is_err(),
        "aún no está dado de baja"
    );
    dispose_asset(&v, id, date!(2026 - 06 - 30), eur(10_000), NOW).unwrap();
    let c = sale_invoice_candidates(&v, id, "").unwrap();
    let ids: Vec<i64> = c.iter().map(|c| c.invoice_id).collect();
    // A 0, 121 y 192 días de la baja; la de antes del alta, no.
    assert_eq!(
        ids,
        vec![f.id, services.id, old.id],
        "las más cercanas a la baja primero"
    );
    assert_eq!(
        c[1].suggested,
        Some(eur(2_000)),
        "lo que queda, sin pasar del precio"
    );
    let e = link_sale_invoice(&v, id, before.id, None, NOW)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("es del 01/12/2023, de antes del alta de «Envasadora» (01/01/2024)"),
        "{e}"
    );
    for wrong in [eur(10_001), Money::ZERO, -eur(1)] {
        assert!(link_sale_invoice(&v, id, f.id, Some(wrong), NOW).is_err());
    }
    link_sale_invoice(&v, id, f.id, None, NOW).unwrap();
    // Con su factura de venta, sólo se proponen rectificativas de ésta (no hay).
    assert!(sale_invoice_candidates(&v, id, "").unwrap().is_empty());
    let e = link_sale_invoice(&v, id, services.id, None, NOW)
        .unwrap_err()
        .to_string();
    assert!(e.contains("ya tiene su factura de venta"), "{e}");
}

/// Una rectificativa emitida antes de vincular su factura pasa a la venta al vincularla (si la
/// factura vende sólo ese elemento y entero).
#[test]
fn las_rectificativas_ya_emitidas_pasan_al_vincular() {
    let v = setup();
    let id = envasadora(&v);
    let f = invoice(&v, 10_000, date!(2026 - 06 - 30));
    let r = rectify(&v, -500, date!(2026 - 07 - 01), &f, date!(2026 - 06 - 30));
    assert_eq!(r.export_error, None, "aún no es de ninguna venta");
    let d = dispose_asset_with_invoice(&v, id, date!(2026 - 06 - 30), eur(10_000), Some(f.id), NOW)
        .unwrap();
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains(&r.number) && w.contains("también pasa al resultado")),
        "{:?}",
        d.warnings
    );
    assert_eq!(saldo(&v, 2026, "705"), Money::ZERO);
    assert_eq!(saldo(&v, 2026, "671"), Money::ZERO);
    assert_eq!(saldo(&v, 2026, "771"), -cents(49_507));
    assert_eq!(asset_sales(&v).unwrap()[0].invoiced, eur(9_500));
    cuadra(&v, 2026);
}

/// Una factura que vende dos elementos: cada uno vincula su parte y la rectificativa no se
/// reparte sola (se dice qué parte es de cada uno). La «Etiquetadora» (6.000 € al 10 %, 1.200
/// amortizados) a 30/6/2026: 600 × 181/365 = 297,53, valor neto 4.502,47 y beneficio 497,53; las
/// rebajas quitan beneficio a cada una (995,07 − 400 y 497,53 − 200).
#[test]
fn una_factura_para_dos_elementos() {
    let v = setup();
    let a = envasadora(&v);
    let b = asset(
        &v,
        "Etiquetadora",
        ("213", "2813", "681"),
        date!(2024 - 01 - 01),
        (6_000, 10, 1_200),
    );
    let f = issue(
        &v,
        &draft(
            vec![
                line("Envasadora usada", 10_000, taxed()),
                line("Etiquetadora usada", 5_000, taxed()),
            ],
            date!(2026 - 06 - 30),
        ),
        NOW,
        true,
    )
    .unwrap();
    let d = dispose_asset_with_invoice(&v, a, date!(2026 - 06 - 30), eur(10_000), Some(f.id), NOW)
        .unwrap();
    assert!(
        d.warnings.iter().any(|w| w.contains("5.000,00 €")),
        "{:?}",
        d.warnings
    );
    // Para la segunda, se propone lo que queda.
    dispose_asset(&v, b, date!(2026 - 06 - 30), eur(5_000), NOW).unwrap();
    let c = sale_invoice_candidates(&v, b, "").unwrap();
    assert_eq!(
        (c[0].invoice_id, c[0].open, c[0].suggested),
        (f.id, eur(5_000), Some(eur(5_000)))
    );
    link_sale_invoice(&v, b, f.id, None, NOW).unwrap();
    assert_eq!(saldo(&v, 2026, "705"), Money::ZERO);
    assert_eq!(saldo(&v, 2026, "543"), Money::ZERO);

    let r = rectify(&v, -600, date!(2026 - 07 - 15), &f, date!(2026 - 06 - 30));
    let aviso = r.export_error.clone().unwrap_or_default();
    assert!(aviso.contains("varios elementos"), "{aviso}");
    let p = &asset_sales(&v).unwrap()[0].pending;
    assert_eq!(
        (p[0].invoice_id, p[0].suggested, p[0].cancels_sale),
        (r.id, None, false)
    );
    assert!(
        link_sale_invoice(&v, a, r.id, None, NOW).is_err(),
        "hay que decir la parte"
    );
    assert!(
        link_sale_invoice(&v, a, r.id, Some(eur(400)), NOW).is_err(),
        "con su signo"
    );
    assert!(
        link_sale_invoice(&v, a, r.id, Some(-eur(700)), NOW).is_err(),
        "no más de 600"
    );
    link_sale_invoice(&v, a, r.id, Some(-eur(400)), NOW).unwrap();
    link_sale_invoice(&v, b, r.id, Some(-eur(200)), NOW).unwrap();
    assert_eq!(saldo(&v, 2026, "705"), Money::ZERO);
    assert_eq!(saldo(&v, 2026, "671"), Money::ZERO);
    assert_eq!(saldo(&v, 2026, "771"), -cents(89_260), "595,07 + 297,53");
    let sales = asset_sales(&v).unwrap();
    assert_eq!(
        (sales[0].result, sales[1].result),
        (cents(59_507), cents(29_753))
    );
    cuadra(&v, 2026);
}

/// Una rectificativa de una factura que vende dos elementos y que rebaja más que el precio de uno
/// no «anula la venta» de ése: no se sabe qué parte es de cada uno hasta que se dice.
#[test]
fn la_rectificativa_de_dos_elementos_no_anula_ninguna_venta() {
    let v = setup();
    let a = envasadora(&v);
    let b = asset(
        &v,
        "Etiquetadora",
        ("213", "2813", "681"),
        date!(2024 - 01 - 01),
        (6_000, 10, 1_200),
    );
    let f = issue(
        &v,
        &draft(
            vec![
                line("Envasadora usada", 10_000, taxed()),
                line("Etiquetadora usada", 5_000, taxed()),
            ],
            date!(2026 - 06 - 30),
        ),
        NOW,
        true,
    )
    .unwrap();
    dispose_asset_with_invoice(&v, a, date!(2026 - 06 - 30), eur(10_000), Some(f.id), NOW).unwrap();
    dispose_asset_with_invoice(&v, b, date!(2026 - 06 - 30), eur(5_000), Some(f.id), NOW).unwrap();
    // Rebaja de 6.000 de la envasadora sola: más que el precio de la etiquetadora.
    let r = rectify(&v, -6_000, date!(2026 - 07 - 15), &f, date!(2026 - 06 - 30));
    for s in asset_sales(&v).unwrap() {
        assert_eq!(
            (s.pending[0].invoice_id, s.pending[0].cancels_sale),
            (r.id, false),
            "{}",
            s.asset_id
        );
    }
    let c = sale_invoice_candidates(&v, b, "").unwrap();
    assert_eq!((c[0].invoice_id, c[0].cancels_sale), (r.id, false));
}

/// Ficha de la venta en el cierre de 2026.
fn item_del_cierre(v: &Vault) -> cierre::CheckItem {
    cierre::checklist(v, 2026)
        .unwrap()
        .items
        .into_iter()
        .find(|i| i.id == "ventas_inmovilizado")
        .unwrap()
}

/// Asiento manual de dos líneas: Debe `debe`, Haber `haber`.
fn asiento(v: &Vault, date: Date, concept: &str, debe: &str, haber: &str, euros: i64) -> i64 {
    manual_entry(
        v,
        &JournalEntry {
            date,
            concept: concept.into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc(debe), eur(euros)),
                EntryLine::credit(acc(haber), eur(euros)),
            ],
        },
        NOW,
    )
    .unwrap()
}

/// Una venta acaba con beneficio o con pérdida, nunca con los dos: cada cambio del precio va
/// primero contra el resultado que ya tiene en el ejercicio.
#[test]
fn una_venta_acaba_con_beneficio_o_con_perdida() {
    // Tirada (precio 0: pérdida de todo el valor neto, 9.004,93) y vendida después por 500 + IVA:
    // la pérdida baja a 8.504,93 (Haber 671), sin un beneficio aparte.
    let v = setup();
    let id = envasadora(&v);
    let d = dispose_asset(&v, id, date!(2026 - 06 - 30), Money::ZERO, NOW).unwrap();
    assert_eq!(d.result, -cents(900_493));
    let f = invoice(&v, 500, date!(2026 - 07 - 10));
    let l = link_sale_invoice(&v, id, f.id, None, NOW).unwrap();
    assert_eq!((l.amount, l.result_change), (eur(500), eur(500)));
    assert_eq!(saldo(&v, 2026, "671"), cents(850_493));
    for a in ["771", "705", "543", "213", "2813"] {
        assert_eq!(saldo(&v, 2026, a), Money::ZERO, "{a}");
    }
    assert_eq!(asset_sales(&v).unwrap()[0].result, -cents(850_493));
    cuadra(&v, 2026);

    // Vendida con pérdida (8.000 por 9.004,93: −1.004,93) y facturada por 9.500: la subida quita
    // la pérdida y el resto (495,07) es beneficio.
    let v = setup();
    let id = envasadora(&v);
    dispose_asset(&v, id, date!(2026 - 06 - 30), eur(8_000), NOW).unwrap();
    let f = invoice(&v, 9_500, date!(2026 - 07 - 10));
    let l = link_sale_invoice(&v, id, f.id, Some(eur(9_500)), NOW).unwrap();
    assert_eq!(l.result_change, eur(1_500));
    assert_eq!(saldo(&v, 2026, "671"), Money::ZERO);
    assert_eq!(saldo(&v, 2026, "771"), -cents(49_507));
    assert_eq!(saldo(&v, 2026, "543"), Money::ZERO);
    assert_eq!(saldo(&v, 2026, "705"), Money::ZERO);
    cuadra(&v, 2026);

    // Con beneficio (995,07), una rectificativa de −1.500 se lo come y deja 504,93 de pérdida.
    let v = setup();
    let id = envasadora(&v);
    let f = invoice(&v, 10_000, date!(2026 - 06 - 30));
    dispose_asset_with_invoice(&v, id, date!(2026 - 06 - 30), eur(10_000), Some(f.id), NOW)
        .unwrap();
    rectify(&v, -1_500, date!(2026 - 07 - 15), &f, date!(2026 - 06 - 30));
    assert_eq!(saldo(&v, 2026, "771"), Money::ZERO);
    assert_eq!(saldo(&v, 2026, "671"), cents(50_493));
    assert_eq!(saldo(&v, 2026, "705"), Money::ZERO);
    assert_eq!(
        annual_accounts(&v, 2026).unwrap().income.get("11"),
        -cents(50_493)
    );
    cuadra(&v, 2026);
}

/// Dos rectificativas: la segunda cuenta con lo que dejó la primera (995,07 − 500 = 495,07 de
/// beneficio; −600 se lo come y deja 104,93 de pérdida), así que se desvinculan en orden inverso.
#[test]
fn las_rectificativas_se_desvinculan_en_orden_inverso() {
    let v = setup();
    let id = envasadora(&v);
    let f = invoice(&v, 10_000, date!(2026 - 06 - 30));
    dispose_asset_with_invoice(&v, id, date!(2026 - 06 - 30), eur(10_000), Some(f.id), NOW)
        .unwrap();
    let r1 = rectify(&v, -500, date!(2026 - 07 - 15), &f, date!(2026 - 06 - 30));
    let r2 = rectify(&v, -600, date!(2026 - 07 - 20), &f, date!(2026 - 06 - 30));
    assert_eq!(saldo(&v, 2026, "771"), Money::ZERO);
    assert_eq!(saldo(&v, 2026, "671"), cents(10_493));
    let e = unlink_sale_invoice(&v, id, r1.id, NOW, true)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains(&format!("desvincula antes {}", r2.number)),
        "{e}"
    );
    unlink_sale_invoice(&v, id, r2.id, NOW, true).unwrap();
    assert_eq!(saldo(&v, 2026, "771"), -cents(49_507));
    assert_eq!(saldo(&v, 2026, "671"), Money::ZERO);
    unlink_sale_invoice(&v, id, r1.id, NOW, true).unwrap();
    assert_eq!(saldo(&v, 2026, "771"), -cents(99_507));
    assert_eq!(saldo(&v, 2026, "671"), Money::ZERO);
    assert_eq!(
        saldo(&v, 2026, "705"),
        eur(1_100),
        "las rebajas vuelven a ventas"
    );
    cuadra(&v, 2026);
}

/// Baja sin factura y cobro conciliado contra la 543 (12.100, con el IVA): la 543 queda en
/// −2.100 y ya no tiene el precio. La factura no se vincula encima (la 543 iría a −12.100 y el
/// cobro contaría también en la 430), la línea no dice que el precio siga en la 543 y el cierre
/// avisa de la 543 en negativo. Deshecho el cobro, se vincula.
#[test]
fn la_543_ya_cobrada_no_se_vincula_otra_vez() {
    let v = setup();
    let id = envasadora(&v);
    dispose_asset(&v, id, date!(2026 - 06 - 30), eur(10_000), NOW).unwrap();
    let cobro = asiento(
        &v,
        date!(2026 - 07 - 02),
        "Cobro de la envasadora",
        "572",
        "543",
        12_100,
    );
    assert_eq!(saldo(&v, 2026, "543"), -eur(2_100));
    assert_eq!(asset_sales(&v).unwrap()[0].unbilled, Money::ZERO);
    let i = item_del_cierre(&v);
    assert!(!i.ok);
    assert!(
        i.detail
            .contains("La 543 tiene saldo acreedor (-2.100,00 €)"),
        "{}",
        i.detail
    );
    assert!(!i.detail.contains("sin factura vinculada"), "{}", i.detail);

    let f = invoice(&v, 10_000, date!(2026 - 07 - 05));
    let e = link_sale_invoice(&v, id, f.id, None, NOW)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("la 543 ya no tiene el precio de «Envasadora» (10.000,00 €; le quedan 0,00 €)"),
        "{e}"
    );
    assert!(e.contains("deshaz esa conciliación"), "{e}");
    assert_eq!(saldo(&v, 2026, "705"), -eur(10_000), "nada cambia");
    assert_eq!(saldo(&v, 2026, "543"), -eur(2_100));

    delete_manual_entry(&v, cobro, NOW, true).unwrap();
    link_sale_invoice(&v, id, f.id, None, NOW).unwrap();
    saldos_de_la_venta(&v);
    assert!(item_del_cierre(&v).ok);
}

/// Una venta cuya factura no se puede vincular (de un ejercicio cerrado, o del libro de la
/// gestoría) o cuyo ingreso doble ya se corrigió a mano (Debe 705, Haber 543, como antes de
/// CONTABILIDAD-08): el cierre dice que se salde la 543 y, saldada, ya no la da por pendiente; y
/// otra factura no se vincula encima del ajuste.
#[test]
fn la_venta_ajustada_a_mano_no_queda_pendiente() {
    let v = setup();
    let id = envasadora(&v);
    // Facturada en 2025, que ya está cerrado.
    let f = invoice(&v, 10_000, date!(2025 - 12 - 28));
    close_year(&v, 2025, NOW, true).unwrap();
    dispose_asset(&v, id, date!(2026 - 01 - 04), eur(10_000), NOW).unwrap();
    assert!(sale_invoice_candidates(&v, id, "").unwrap().is_empty());
    assert!(
        sale_invoice_candidates(&v, id, &f.number)
            .unwrap()
            .is_empty()
    );
    let e = link_sale_invoice(&v, id, f.id, None, NOW)
        .unwrap_err()
        .to_string();
    assert!(e.contains("que está cerrado: reábrelo desde Cierre"), "{e}");
    let i = item_del_cierre(&v);
    assert!(!i.ok);
    assert!(
        i.detail.contains(
            "«Envasadora» sin factura vinculada (10.000,00 € en la 543): vincula su factura en \
             Contabilidad → Inmovilizado o, si no la emitiste en TaxPain, salda la 543 con su \
             cobro o con un asiento"
        ),
        "{}",
        i.detail
    );
    asiento(
        &v,
        date!(2026 - 01 - 04),
        "Venta de la envasadora con la factura de 2025",
        "705",
        "543",
        10_000,
    );
    let i = item_del_cierre(&v);
    assert!(i.ok, "{}", i.detail);
    assert_eq!(asset_sales(&v).unwrap()[0].unbilled, Money::ZERO);
    let g = invoice(&v, 10_000, date!(2026 - 01 - 10));
    let e = link_sale_invoice(&v, id, g.id, None, NOW)
        .unwrap_err()
        .to_string();
    assert!(e.contains("no la vincules"), "{e}");
}

/// La factura de un ejercicio anterior al de la baja (facturada el 28/12/2025 y entregada el
/// 4/1/2026) se vincula y su asiento va en 2025, con su fecha: las ventas de 2025 quedan sin ella y
/// la 543 con 10.000 acreedores (lo facturado por adelantado), que la baja salda en 2026 tras el
/// cierre. En 2026: 1.200 × 4/365 = 13,15 de amortización, acumulada 2.413,15, valor neto
/// 9.586,85 y beneficio 413,15.
#[test]
fn la_factura_de_un_ejercicio_anterior_se_vincula_en_el_suyo() {
    let v = setup();
    let id = envasadora(&v);
    let f = invoice(&v, 10_000, date!(2025 - 12 - 28));
    let d = dispose_asset(&v, id, date!(2026 - 01 - 04), eur(10_000), NOW).unwrap();
    assert_eq!(d.result, cents(41_315));
    let c = sale_invoice_candidates(&v, id, "").unwrap();
    assert_eq!((c[0].invoice_id, c[0].suggested), (f.id, Some(eur(10_000))));
    let l = link_sale_invoice(&v, id, f.id, None, NOW).unwrap();
    assert!(
        l.warnings
            .iter()
            .any(|w| w.contains("de un ejercicio anterior al de la baja")),
        "{:?}",
        l.warnings
    );
    let e = journal(&v, 2025)
        .unwrap()
        .into_iter()
        .find(|e| e.id == l.entry_id)
        .unwrap();
    assert_eq!(
        e.entry.date,
        date!(2025 - 12 - 28),
        "con la fecha de la factura"
    );
    assert_eq!(saldo(&v, 2025, "705"), Money::ZERO);
    assert_eq!(saldo(&v, 2025, "543"), -eur(10_000));
    assert_eq!(
        annual_accounts(&v, 2025).unwrap().income.get("1"),
        Money::ZERO
    );
    cuadra(&v, 2025);
    assert!(item_del_cierre(&v).ok);

    close_year(&v, 2025, NOW, true).unwrap();
    for a in ["213", "2813", "543", "705", "671"] {
        assert_eq!(saldo(&v, 2026, a), Money::ZERO, "{a}");
    }
    assert_eq!(saldo(&v, 2026, "771"), -cents(41_315));
    assert_eq!(saldo(&v, 2026, "681"), cents(1_315));
    assert_eq!(saldo(&v, 2026, "4300001"), eur(12_100));
    cuadra(&v, 2026);
    let pyg = annual_accounts(&v, 2026).unwrap().income;
    assert_eq!(pyg.get("1"), Money::ZERO);
    assert_eq!(pyg.get("11"), cents(41_315));
    assert!(item_del_cierre(&v).ok);
}

/// Un cobro conciliado contra la 543 antes de registrar la baja: no se da de baja con la factura
/// (el cobro contaría en la 543 y en la 430) y no queda nada a medias.
#[test]
fn la_543_ya_cobrada_no_se_da_de_baja_con_la_factura() {
    let v = setup();
    let id = envasadora(&v);
    let f = invoice(&v, 10_000, date!(2026 - 06 - 30));
    asiento(
        &v,
        date!(2026 - 06 - 30),
        "Cobro de la envasadora",
        "572",
        "543",
        12_100,
    );
    let e = dispose_asset_with_invoice(&v, id, date!(2026 - 06 - 30), eur(10_000), Some(f.id), NOW)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("la 543 ya no tiene el precio de «Envasadora» (10.000,00 €; le quedan 0,00 €)"),
        "{e}"
    );
    assert_eq!(fixed_assets(&v).unwrap()[0].disposed_on, None);
    assert_eq!(saldo(&v, 2026, "543"), -eur(12_100));
    assert_eq!(saldo(&v, 2026, "681"), Money::ZERO, "ni la amortización");
    assert!(asset_sales(&v).unwrap().is_empty());
}

/// Con varias ventas sin factura, lo que la 543 aún tiene se reparte de la más reciente a la más
/// antigua: lo cobrado o corregido suele ser lo más antiguo.
#[test]
fn la_543_se_reparte_de_la_venta_mas_reciente_a_la_mas_antigua() {
    let v = setup();
    let a = envasadora(&v);
    let b = asset(
        &v,
        "Etiquetadora",
        ("213", "2813", "681"),
        date!(2024 - 01 - 01),
        (6_000, 10, 1_200),
    );
    dispose_asset(&v, a, date!(2026 - 03 - 31), eur(10_000), NOW).unwrap();
    dispose_asset(&v, b, date!(2026 - 06 - 30), eur(5_000), NOW).unwrap();
    // La envasadora ya se corrigió a mano.
    asiento(
        &v,
        date!(2026 - 04 - 02),
        "Corrección de la venta de la envasadora",
        "705",
        "543",
        10_000,
    );
    let sales = asset_sales(&v).unwrap();
    assert_eq!(
        (sales[0].unbilled, sales[1].unbilled),
        (Money::ZERO, eur(5_000))
    );
    let i = item_del_cierre(&v);
    assert!(
        i.detail
            .contains("«Etiquetadora» sin factura vinculada (5.000,00 € en la 543)"),
        "{}",
        i.detail
    );
    assert!(!i.detail.contains("«Envasadora»"), "{}", i.detail);
}

/// Los asientos que se borran solos no se llevan sus documentos ni sus observaciones: el que pasa
/// la factura a la venta, al desvincularla, los deja en el de la factura; la amortización de
/// después de la baja, en el de la baja.
#[test]
fn los_documentos_de_los_asientos_borrados_no_se_pierden() {
    let v = setup();
    let id = envasadora(&v);
    let f = invoice(&v, 10_000, date!(2026 - 06 - 30));
    let d = dispose_asset_with_invoice(&v, id, date!(2026 - 06 - 30), eur(10_000), Some(f.id), NOW)
        .unwrap();
    let sale = d.sale_entry.unwrap();
    attach_document(
        &v,
        sale,
        "contrato-compraventa.pdf",
        b"%PDF-1.4 contrato",
        NOW,
    )
    .unwrap();
    set_entry_notes(&v, sale, "Entregada en la nave", NOW).unwrap();
    unlink_sale_invoice(&v, id, f.id, NOW, true).unwrap();
    let factura = journal(&v, 2026)
        .unwrap()
        .into_iter()
        .find(|e| e.entry.origin == Origin::FacturaEmitida(f.id))
        .unwrap();
    let names: Vec<String> = entry_documents(&v, factura.id)
        .unwrap()
        .into_iter()
        .map(|d| d.filename)
        .collect();
    assert_eq!(names, vec!["contrato-compraventa.pdf".to_owned()]);
    let notes = factura.notes.unwrap_or_default();
    assert!(
        notes.contains(&format!(
            "De «Venta de Envasadora: factura {}» (30/06/2026): Entregada en la nave",
            f.number
        )),
        "{notes}"
    );

    let v = setup();
    let id = envasadora(&v);
    post_depreciation(&v, 2026, NOW).unwrap();
    let e2027 = post_depreciation(&v, 2027, NOW).unwrap()[0];
    attach_document(
        &v,
        e2027,
        "cuadro-amortizacion.pdf",
        b"%PDF-1.4 cuadro",
        NOW,
    )
    .unwrap();
    let d = dispose_asset(&v, id, date!(2026 - 06 - 30), eur(10_000), NOW).unwrap();
    assert!(
        d.warnings.iter().any(|w| w
            == "Los documentos y las observaciones de la amortización quitada pasan al asiento \
                de la baja."),
        "{:?}",
        d.warnings
    );
    let names: Vec<String> = entry_documents(&v, d.disposal_entry)
        .unwrap()
        .into_iter()
        .map(|d| d.filename)
        .collect();
    assert_eq!(names, vec!["cuadro-amortizacion.pdf".to_owned()]);
}

/// Factura de `euros` más IVA a otro cliente.
fn invoice_to(v: &Vault, euros: i64, date: Date, name: &str, nif: &str) -> IssuedInvoice {
    let mut d = draft(vec![line("Servicio", euros, taxed())], date);
    d.recipient = Some(Party {
        id: PartyId::Nif(nif.into()),
        name: name.into(),
        address: addr(),
        person: None,
    });
    issue(v, &d, NOW, true).unwrap()
}

/// Una empresa con muchas facturas: la de la venta (6/1) es más antigua que las 200 más recientes,
/// pero sale la primera porque es la más cercana a la baja. Las simplificadas sólo salen
/// buscándolas, y se busca por número, cliente o NIF sin mayúsculas ni tildes.
#[test]
fn la_factura_de_la_venta_sale_aunque_haya_muchas() {
    let v = setup();
    let id = envasadora(&v);
    let mut sale = None;
    let mut avila = None;
    for d in 0..210 {
        let date = date!(2026 - 01 - 01) + time::Duration::days(d);
        match d {
            5 => sale = Some(invoice(&v, 10_000, date)),
            7 => {
                avila = Some(invoice_to(
                    &v,
                    100,
                    date,
                    "Construcciones Ávila SL",
                    "A22222228",
                ));
            }
            _ => {
                invoice_to(&v, 100, date, "Cliente Dos SA", "B98765431");
            }
        }
    }
    let sale = sale.unwrap();
    let avila = avila.unwrap();
    let mut s = draft(
        vec![line("Venta de mostrador", 50, taxed())],
        date!(2026 - 01 - 06),
    );
    s.kind = InvoiceKind::Simplificada;
    s.recipient = None;
    let simple = issue(&v, &s, NOW, true).unwrap();
    dispose_asset(&v, id, date!(2026 - 01 - 06), eur(10_000), NOW).unwrap();

    let c = sale_invoice_candidates(&v, id, "").unwrap();
    assert_eq!(c.len(), 200);
    assert_eq!(
        c[0].invoice_id, sale.id,
        "la del día de la baja, la primera"
    );
    assert!(
        c.iter().all(|c| c.invoice_id != simple.id),
        "sin simplificadas"
    );
    let ids = |q: &str| -> Vec<i64> {
        sale_invoice_candidates(&v, id, q)
            .unwrap()
            .into_iter()
            .map(|c| c.invoice_id)
            .collect()
    };
    assert_eq!(ids("CLIENTE uno"), vec![sale.id]);
    assert_eq!(ids(&sale.number.to_lowercase()), vec![sale.id]);
    assert_eq!(ids("avila"), vec![avila.id]);
    assert_eq!(ids("a22222228"), vec![avila.id]);
    assert_eq!(ids(&simple.number), vec![simple.id]);
    assert_eq!(ids("B98765431").len(), 200);
    assert!(ids("nadie").is_empty());
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
