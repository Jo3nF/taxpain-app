//! Previsión de tesorería con una sociedad sintética: cartera, impuestos, nóminas, leasing y
//! asientos periódicos.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::manual_entry;
use taxpain_app::forecast::{Category, Forecast, ForecastItem, WarningKind, forecast};
use taxpain_app::invoicing::{self, InvoiceDraft, issue};
use taxpain_app::models::{self, ModelSettings};
use taxpain_app::payroll::{Employee, PayslipInput, register, save_employee};
use taxpain_app::received::{Reviewed, Source, ingest_file, review};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::templates::{Template, TemplateLine, save as save_template};
use taxpain_app::{leasing, tax_payments};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::leasing::Leasing;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::calendar::TaxpayerProfile;
use taxpain_core::invoice::{InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime};

const NOW: OffsetDateTime = datetime!(2026-10-24 10:00 UTC);
/// Sábado.
const TODAY: Date = date!(2026 - 10 - 24);

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

/// Sociedad sintética (NIF de prueba) con IVA trimestral.
fn setup(with_profile: bool) -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Agencia Ejemplo SL".into(),
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
    if with_profile {
        v.set_setting("taxpayer_profile", &TaxpayerProfile::sociedad_limitada())
            .unwrap();
    }
    v
}

fn capital(v: &Vault, euros: i64) {
    manual_entry(
        v,
        &JournalEntry {
            date: date!(2026 - 01 - 02),
            concept: "Aportación de los socios".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("5720001"), eur(euros)),
                EntryLine::credit(acc("100"), eur(euros)),
            ],
        },
        NOW,
    )
    .unwrap();
}

fn invoice(v: &Vault, euros: i64, date: Date) {
    let d = InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: date,
        operation_date: None,
        recipient: Some(Party {
            id: PartyId::Nif("A22222228".into()),
            name: "Cliente Ejemplo SA".into(),
            address: addr(),
            person: None,
        }),
        lines: vec![InvoiceLine {
            description: "Diseño web".into(),
            quantity_milli: 1000,
            unit_price: eur(euros),
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
    issue(v, &d, NOW, true).unwrap();
}

/// Factura recibida revisada (base + 21 %). Se construye desde JSON para no depender de los
/// campos opcionales de la revisión.
fn received(v: &Vault, number: &str, base: i64, date: Date) {
    let rec = ingest_file(
        v,
        &format!("{number}.xml"),
        format!("<?xml version=\"1.0\"?><x n=\"{number}\"/>").as_bytes(),
        Source::Manual,
        NOW,
    )
    .unwrap();
    let vat = base * 21;
    let r: Reviewed = serde_json::from_value(serde_json::json!({
        "supplier_nif": "B87654323",
        "supplier_name": "Proveedor Ejemplo SL",
        "number": number,
        "issue_date": date.to_string(),
        "base": base * 100,
        "vat": vat,
        "withholding": 0,
        "total": base * 100 + vat,
        "deductible_pct": 100,
    }))
    .unwrap();
    review(v, rec.id, &r, NOW).unwrap();
}

fn line(account: &str, debit: bool, euros: Option<i64>) -> TemplateLine {
    TemplateLine {
        account: account.into(),
        description: "$".into(),
        debit,
        amount: euros.map(eur),
    }
}

fn template(v: &Vault, name: &str, lines: Vec<TemplateLine>, next: Date) {
    save_template(
        v,
        &Template {
            id: 0,
            name: name.into(),
            concept: name.into(),
            lines,
            every_months: Some(1),
            next_date: Some(next),
            counter: 0,
        },
        NOW,
    )
    .unwrap();
}

fn find<'a>(f: &'a Forecast, concept: &str) -> Vec<&'a ForecastItem> {
    f.items
        .iter()
        .filter(|i| i.concept.starts_with(concept))
        .collect()
}

fn has_warning(f: &Forecast, kind: WarningKind, text: &str) -> bool {
    f.warnings
        .iter()
        .any(|w| w.kind == kind && w.text.contains(text))
}

#[test]
fn agencia_con_cartera_impuestos_nominas_y_fijos() {
    let v = setup(true);
    capital(&v, 60_000);
    // Cobro vencido (31/8) y otro el 14/11; pago a un proveedor el 9/11.
    invoice(&v, 1000, date!(2026 - 08 - 01));
    received(&v, "P-7", 500, date!(2026 - 10 - 10));
    invoice(&v, 2000, date!(2026 - 10 - 15));
    // Nómina de octubre: líquido 1.573 (465) y seguros sociales 727 (476).
    let emp = save_employee(
        &v,
        &Employee {
            nif: "12345678Z".into(),
            name: "Española Española Ana".into(),
            province: "46".into(),
            birth_year: 1990,
            family: 3,
            contract: 1,
            active: true,
            ..Default::default()
        },
        NOW,
    )
    .unwrap();
    register(
        &v,
        &PayslipInput {
            employee_id: emp,
            date: date!(2026 - 10 - 23),
            gross: eur(2000),
            irpf: eur(300),
            ss_employee: eur(127),
            ss_company: eur(600),
            small_entity: None,
        },
        NOW,
    )
    .unwrap();
    // 202 con la cuota del último IS; el 2P (octubre) ya está pagado.
    models::save_settings(
        &v,
        &ModelSettings {
            corporate_tax_base: Some(eur(5000)),
            ..Default::default()
        },
        NOW,
    )
    .unwrap();
    let p202 = tax_payments::amount_due(&v, "202", 2026, 2).unwrap().0;
    assert!(p202.cents() > 0);
    tax_payments::pay(&v, "202", 2026, 2, date!(2026 - 10 - 20), "5720001", NOW).unwrap();
    // Alquiler mensual (con su retención) por banco; el pago al proveedor por plantilla ya va en
    // la cartera, y otro asiento pide el importe.
    template(
        &v,
        "Alquiler oficina",
        vec![
            line("621", true, Some(1000)),
            line("4751", false, Some(190)),
            line("5720001", false, Some(810)),
        ],
        date!(2026 - 11 - 01),
    );
    template(
        &v,
        "Pago proveedor habitual",
        vec![
            line("4100001", true, Some(100)),
            line("5720001", false, Some(100)),
        ],
        date!(2026 - 11 - 05),
    );
    template(
        &v,
        "Suministros",
        vec![line("628", true, None), line("5720001", false, None)],
        date!(2026 - 11 - 10),
    );
    // Leasing de un equipo desde junio: cuotas de julio a octubre sin contabilizar.
    leasing::create(
        &v,
        &Leasing {
            id: 0,
            name: "Equipos".into(),
            start: date!(2026 - 06 - 15),
            principal: eur(12_000),
            months: 36,
            rate: Rate::from_bp(500),
            purchase_option: eur(300),
            vat: Rate::percent(21),
            asset_account: acc("217"),
            debt_account: acc("5240001"),
            lessor: None,
        },
        Rate::percent(25),
        NOW,
    )
    .unwrap();

    let f = forecast(&v, TODAY).unwrap();
    eprintln!("{:#?}", f.warnings);
    eprintln!("{:#?}", f.notes);

    // Saldo de hoy: 60.000 − el 202 pagado.
    assert_eq!(f.opening, eur(60_000) - p202);
    assert_eq!(f.today, TODAY);
    assert_eq!(f.weekly.buckets.len(), 13);
    assert_eq!(f.monthly.buckets.len(), 12);
    assert_eq!(f.monthly.end, date!(2027 - 09 - 30));

    // Cartera: el cobro vencido cuenta hoy.
    let overdue = find(&f, "Cobro A");
    assert_eq!(overdue.len(), 2);
    assert!(overdue[0].overdue);
    assert_eq!(
        (overdue[0].date, overdue[0].due_date, overdue[0].amount),
        (TODAY, date!(2026 - 08 - 31), eur(1210))
    );
    assert_eq!(overdue[1].date, date!(2026 - 11 - 14));
    assert_eq!(overdue[1].amount, eur(2420));
    let pago = find(&f, "Pago P-7");
    assert_eq!(pago.len(), 1);
    assert_eq!(
        (pago[0].date, pago[0].amount),
        (date!(2026 - 11 - 09), -eur(605))
    );
    assert!(has_warning(
        &f,
        WarningKind::Overdue,
        "Cobros vencidos: 1 por 1.210,00 €"
    ));

    // Impuestos: el 303 del 3T venció el 20/10 sin presentar (210 € del IVA de agosto) y cuenta
    // hoy; el del 4T (en curso) el 1/2/2027 (el 30/1 es sábado): 420 − 105.
    let t3 = find(&f, "Modelo 303 3T 2026");
    assert_eq!(t3.len(), 1);
    assert!(t3[0].overdue && t3[0].estimated);
    assert_eq!((t3[0].date, t3[0].amount), (TODAY, -eur(210)));
    assert!(has_warning(&f, WarningKind::Overdue, "303 3T 2026 venció"));
    let t4 = find(&f, "Modelo 303 4T 2026 (periodo en curso)");
    assert_eq!(t4.len(), 1);
    assert_eq!(
        (t4[0].date, t4[0].amount),
        (date!(2027 - 02 - 01), -eur(315))
    );
    assert_eq!(t4[0].category, Category::Taxes);
    // 111 del 4T: la retención de la nómina de octubre.
    let r111 = find(&f, "Modelo 111 4T 2026");
    assert_eq!(r111.len(), 1);
    assert_eq!(
        (r111[0].date, r111[0].amount),
        (date!(2027 - 01 - 20), -eur(300))
    );
    // 202: el 2P está pagado; el 3P (21/12, el 20 es domingo) y el 1P de 2027 sí cuentan.
    assert!(find(&f, "Modelo 202 2P 2026").is_empty());
    let p3 = find(&f, "Modelo 202 3P 2026");
    assert_eq!(p3.len(), 1);
    assert_eq!(p3[0].date, date!(2026 - 12 - 21));
    assert_eq!(
        p3[0].amount,
        -models::draft(&v, "202", 2026, 3).unwrap().result
    );
    assert_eq!(find(&f, "Modelo 202 1P 2027").len(), 1);
    // Sin alquileres con retención ni operaciones intracomunitarias: ni 115 ni informativos.
    assert!(find(&f, "Modelo 115").is_empty());
    assert!(find(&f, "Modelo 390").is_empty());
    assert!(find(&f, "Modelo 349").is_empty());

    // Nóminas: lo de octubre pendiente de pago hoy y los seguros sociales el 30/11; después se
    // repite cada mes hasta septiembre de 2027.
    let pend = find(&f, "Nóminas contabilizadas");
    assert_eq!((pend[0].date, pend[0].amount), (TODAY, -eur(1573)));
    let ss = find(&f, "Seguros sociales contabilizados");
    assert_eq!(
        (ss[0].date, ss[0].amount),
        (date!(2026 - 11 - 30), -eur(727))
    );
    let net: Vec<_> = find(&f, "Nóminas de ");
    assert_eq!(net.len(), 11);
    assert!(net.iter().all(|i| i.estimated && i.amount == -eur(1573)));
    assert_eq!(net[0].date, date!(2026 - 11 - 30));
    assert_eq!(net[10].date, date!(2027 - 09 - 30));
    // Los de noviembre se pagan el 31/12; los de septiembre de 2027, fuera del horizonte.
    let ss_est = find(&f, "Seguros sociales de ");
    assert_eq!(ss_est.len(), 10);
    assert_eq!(ss_est[0].date, date!(2026 - 12 - 31));

    // Asientos periódicos: el alquiler (810 € por banco) cada día 1; el pago al proveedor no
    // (ya está en la cartera) y el que pide el importe, tampoco.
    let rent = find(&f, "Alquiler oficina");
    assert_eq!(rent.len(), 11);
    assert!(rent.iter().all(|i| i.amount == -eur(810)));
    assert!(find(&f, "Pago proveedor habitual").is_empty());
    assert!(find(&f, "Suministros").is_empty());
    assert!(
        f.notes
            .iter()
            .any(|n| n.contains("«Suministros» pide el importe"))
    );

    // Leasing: cuotas desde hoy, con IVA; las de julio a octubre (pasadas) no.
    let view = &leasing::list(&v).unwrap()[0];
    let expected: Vec<_> = view
        .installments
        .iter()
        .filter(|q| q.q.date >= TODAY && q.q.date <= f.monthly.end)
        .collect();
    let lea = find(&f, "Leasing Equipos");
    assert_eq!(lea.len(), expected.len());
    assert_eq!(lea[0].amount, -(expected[0].q.amount + expected[0].q.vat));
    assert!(f.notes.iter().any(|n| n.starts_with("4 cuotas de leasing")));

    // Primera semana (24–25/10): entra el cobro vencido; salen el 303 vencido y las nóminas.
    let w0 = &f.weekly.buckets[0];
    assert_eq!((w0.start, w0.end), (TODAY, date!(2026 - 10 - 25)));
    assert_eq!(w0.inflows.get(&Category::Collections), Some(&eur(1210)));
    assert_eq!(w0.outflows.get(&Category::Taxes), Some(&eur(210)));
    assert_eq!(w0.outflows.get(&Category::Payroll), Some(&eur(1573)));
    assert_eq!(w0.closing, f.opening + eur(1210) - eur(210) - eur(1573));
    // Los periodos se encadenan y el último cierra con todo lo del horizonte.
    for h in [&f.weekly, &f.monthly] {
        for pair in h.buckets.windows(2) {
            assert_eq!(pair[0].closing, pair[1].opening);
            assert_eq!(pair[0].end + time::Duration::days(1), pair[1].start);
        }
        let total: Money = f
            .items
            .iter()
            .filter(|i| i.date <= h.end)
            .map(|i| i.amount)
            .sum();
        assert_eq!(h.buckets.last().unwrap().closing, f.opening + total);
        assert_eq!(h.points.last().unwrap().balance, f.opening + total);
        assert!(h.lowest <= h.buckets.iter().map(|b| b.closing).min().unwrap());
    }
    assert!(f.items.windows(2).all(|w| w[0].date <= w[1].date));
    assert!(!has_warning(&f, WarningKind::Negative, ""));

    // Dinero que no es tuyo: IVA 630 − 105 y la retención de la nómina.
    assert_eq!(f.not_yours.vat, eur(525));
    assert_eq!(f.not_yours.withholdings, eur(300));
    assert_eq!(f.not_yours.total, eur(825));

    // Al pagar el 303 del 3T deja de contar (y sale de la tesorería y del IVA pendiente).
    tax_payments::pay(&v, "303", 2026, 3, TODAY, "5720001", NOW).unwrap();
    let g = forecast(&v, TODAY).unwrap();
    assert!(find(&g, "Modelo 303 3T").is_empty());
    assert!(!has_warning(&g, WarningKind::Overdue, "303 3T"));
    assert_eq!(g.opening, f.opening - eur(210));
    assert_eq!(g.not_yours.vat, eur(315));

    // Con más de 6 millones de cifra de negocios el 202 va por el art. 40.3 LIS (base del
    // periodo): se prevé con ese cálculo, no con el 18 % de la cuota.
    let mut m = models::settings(&v).unwrap();
    m.turnover_over_6m = true;
    models::save_settings(&v, &m, NOW).unwrap();
    let big = forecast(&v, TODAY).unwrap();
    let d = models::draft(&v, "202", 2026, 3).unwrap();
    assert!(d.boxes.iter().any(|(k, _)| *k == 34), "{:?}", d.boxes);
    let previsto: Money = find(&big, "Modelo 202 3P 2026")
        .iter()
        .map(|l| l.amount)
        .sum();
    assert_eq!(previsto, -d.result.max(Money::ZERO));
}

#[test]
fn saldo_negativo_y_sin_perfil() {
    let v = setup(false);
    capital(&v, 300);
    // Una compra de 1.000 + IVA que vence el 9/11: la semana del 9 se queda en negativo.
    received(&v, "P-1", 1000, date!(2026 - 10 - 10));
    let f = forecast(&v, TODAY).unwrap();
    assert_eq!(f.opening, eur(300));
    assert_eq!(f.weekly.lowest, eur(300) - eur(1210));
    assert_eq!(f.weekly.lowest_date, date!(2026 - 11 - 09));
    assert_eq!(f.weekly.first_negative, Some(date!(2026 - 11 - 09)));
    assert_eq!(f.weekly.buckets[3].closing, -eur(910));
    let first = &f.warnings[0];
    assert_eq!(first.kind, WarningKind::Negative);
    assert_eq!(
        first.text,
        "Saldo negativo desde la semana del 9 de noviembre; mínimo de -910,00 € el 09/11/2026."
    );
    assert!(f.notes.iter().any(|n| n.starts_with("Sin perfil fiscal")));
    // Sin datos de nóminas, leasing ni plantillas: no se inventa nada.
    assert!(
        f.items
            .iter()
            .all(|i| matches!(i.category, Category::Payments | Category::Taxes))
    );
    // El IVA soportado supera al repercutido: no hay dinero de Hacienda en el banco.
    assert_eq!(f.not_yours.vat, -eur(210));
    assert_eq!(f.not_yours.total, Money::ZERO);
}

#[test]
fn sin_datos() {
    let v = setup(true);
    let f = forecast(&v, TODAY).unwrap();
    assert_eq!(f.opening, Money::ZERO);
    assert!(f.items.is_empty(), "{:#?}", f.items);
    assert!(f.warnings.is_empty(), "{:#?}", f.warnings);
    assert_eq!(f.weekly.points.len(), 1);
    assert_eq!(f.monthly.lowest, Money::ZERO);
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
