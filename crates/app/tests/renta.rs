//! Modalidad de estimación directa en el 130 (RETENCIONES-02) y Renta del autónomo (modelo 100,
//! parte de la actividad) desde la contabilidad. Datos inventados.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;

use taxpain_app::accounting::record_entry;
use taxpain_app::models::{self, YearSettings};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::{invoicing, presentados, renta, tax_payments};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{Party, PartyId, PersonName, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_models::m100::{IrpfRegime, Reduction32};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::Date;
use time::macros::{date, datetime};

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
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
            name: "Carmen Española Española".into(),
            address: addr(),
            person: Some(PersonName {
                name: "Carmen".into(),
                first_surname: "Española".into(),
                second_surname: Some("Española".into()),
            }),
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

fn entry(v: &Vault, d: Date, origin: Origin, lines: Vec<EntryLine>) {
    record_entry(
        v.connection(),
        &JournalEntry {
            date: d,
            concept: "Asiento de prueba".into(),
            origin,
            lines,
        },
        datetime!(2026-12-31 9:00 UTC),
    )
    .unwrap();
}

/// 1T 2026: ingresos 30.000 y gastos 10.000.
fn first_quarter(v: &Vault) {
    entry(
        v,
        date!(2026 - 02 - 10),
        Origin::Manual,
        vec![
            EntryLine::debit(acc("572"), eur(30_000)),
            EntryLine::credit(acc("705"), eur(30_000)),
        ],
    );
    entry(
        v,
        date!(2026 - 03 - 10),
        Origin::Manual,
        vec![
            EntryLine::debit(acc("629"), eur(10_000)),
            EntryLine::credit(acc("572"), eur(10_000)),
        ],
    );
}

fn get(d: &models::Draft, b: u16) -> Money {
    d.boxes
        .iter()
        .find(|(k, _)| *k == b)
        .map_or(Money::ZERO, |(_, v)| *v)
}

fn with_regime(v: &Vault, regime: Option<IrpfRegime>) {
    let mut s = models::settings(v).unwrap();
    s.years.insert(
        2026,
        YearSettings {
            irpf_regime: regime,
            previous_year_net: Some(eur(50_000)),
            ..YearSettings::default()
        },
    );
    models::save_settings(v, &s, datetime!(2026-04-01 9:00 UTC)).unwrap();
}

#[test]
fn el_130_en_simplificada_por_defecto_suma_el_cinco_por_ciento() {
    let v = setup();
    first_quarter(&v);
    with_regime(&v, None);
    let d = models::draft(&v, "130", 2026, 1).unwrap();
    // [02] = 10.000 + 5 % × 20.000; [04] = 20 % × 19.000.
    assert_eq!(get(&d, 2), eur(11_000));
    assert_eq!(get(&d, 4), eur(3_800));
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("simplificada (la habitual)")),
        "{:?}",
        d.warnings
    );
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("difícil justificación"))
    );
    // Normal, indicada en «Datos de los modelos»: sin el 5 % y sin aviso de modalidad.
    with_regime(&v, Some(IrpfRegime::Normal));
    let d = models::draft(&v, "130", 2026, 1).unwrap();
    assert_eq!(get(&d, 2), eur(10_000));
    assert_eq!(get(&d, 4), eur(4_000));
    assert!(!d.warnings.iter().any(|w| w.contains("simplificada")));
}

#[test]
fn con_130_presentados_antes_no_cambia_nada_en_silencio() {
    let v = setup();
    first_quarter(&v);
    with_regime(&v, None);
    // Lo que apunta la migración 0046 para quien ya tenía 130 de 2026 presentados.
    v.set_setting("models.irpf_regime_pending", &vec![2026])
        .unwrap();
    let d = models::draft(&v, "130", 2026, 1).unwrap();
    assert_eq!(get(&d, 2), eur(10_000));
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("Falta la modalidad") && w.contains("Datos de los modelos")),
        "{:?}",
        d.warnings
    );
    // Al elegir simplificada, el 130 recoge el 5 % y el aviso desaparece.
    with_regime(&v, Some(IrpfRegime::Simplificada));
    let d = models::draft(&v, "130", 2026, 1).unwrap();
    assert_eq!(get(&d, 2), eur(11_000));
    assert!(!d.warnings.iter().any(|w| w.contains("Falta la modalidad")));
    // Otros ejercicios sin 130 presentados: simplificada.
    let (r, _) = models::irpf_regime(&v, &models::settings(&v).unwrap(), 2027).unwrap();
    assert_eq!(r, IrpfRegime::Simplificada);
}

#[test]
fn renta_rendimiento_de_la_actividad_y_casillas() {
    let v = setup();
    first_quarter(&v);
    // Cuota de autónomo (642 sin nómina) 3.000; una nómina con 642 de la empresa 300; intereses
    // del banco 100 (no son de la actividad).
    entry(
        &v,
        date!(2026 - 05 - 05),
        Origin::Manual,
        vec![
            EntryLine::debit(acc("642"), eur(3_000)),
            EntryLine::credit(acc("572"), eur(3_000)),
        ],
    );
    entry(
        &v,
        date!(2026 - 06 - 30),
        Origin::Nomina,
        vec![
            EntryLine::debit(acc("640"), eur(1_000)),
            EntryLine::debit(acc("642"), eur(300)),
            EntryLine::credit(acc("476"), eur(300)),
            EntryLine::credit(acc("465"), eur(1_000)),
        ],
    );
    entry(
        &v,
        date!(2026 - 07 - 01),
        Origin::Manual,
        vec![
            EntryLine::debit(acc("572"), eur(100)),
            EntryLine::credit(acc("769"), eur(100)),
        ],
    );
    // 130 del 1T presentado por el gestor con 2.000 € a ingresar.
    let mut boxes = BTreeMap::new();
    boxes.insert(19, eur(2_000));
    presentados::save_manual(
        &v,
        "130",
        2026,
        "1T",
        &boxes,
        datetime!(2026-04-15 9:00 UTC),
    )
    .unwrap();
    let mut s = models::settings(&v).unwrap();
    s.years.insert(
        2026,
        YearSettings {
            irpf_regime: Some(IrpfRegime::Simplificada),
            activity_reduction: Reduction32::Dependiente,
            other_income_over_6500: true,
            ..YearSettings::default()
        },
    );
    models::save_settings(&v, &s, datetime!(2026-04-01 9:00 UTC)).unwrap();

    let a = renta::activity(&v, 2026).unwrap();
    let r = &a.result;
    assert_eq!(r.income, eur(30_000));
    // Gastos: 10.000 + 3.000 + 1.000 + 300 = 14.300; previo 15.700; 5 % = 785.
    assert_eq!(r.net_before, eur(15_700));
    assert_eq!(r.hard_to_justify, eur(785));
    assert_eq!(r.net, eur(14_915));
    // Art. 32.2.1.º con otras rentas de más de 6.500 €: sólo los 2.000 €.
    assert_eq!(r.reduction_32_2, eur(2_000));
    assert_eq!(r.net_reduced, eur(12_915));
    assert_eq!(r.installments, eur(2_000));
    let code = |c: &str| {
        r.boxes
            .iter()
            .find(|b| b.code.as_deref() == Some(c))
            .unwrap()
            .amount
    };
    assert_eq!(code("0180"), eur(30_000));
    assert_eq!(code("0186"), eur(3_000));
    assert_eq!(code("0222"), eur(785));
    assert_eq!(code("0224"), eur(14_915));
    assert_eq!(code("0235"), eur(12_915));
    let label = |l: &str| r.boxes.iter().find(|b| b.label == l).unwrap().amount;
    assert_eq!(label("Seguridad Social a cargo de la empresa"), eur(300));
    assert_eq!(label("Sueldos y salarios"), eur(1_000));
    assert_eq!(a.missing_130, vec!["2T", "3T", "4T"]);
    assert!(a.notes.iter().any(|n| n.contains("ingresos financieros")));
    assert!(a.check.is_empty());

    // Con los datos fiscales de la AEAT: cotejo de pagos del 130 y cuotas de autónomo.
    let pad = |s: &str, n: usize| format!("{s:<n$}");
    let file = format!(
        "0DDFF2026\n1{}20270402\n2PF00001130I1T46{}{:013}\n2CT00001{}0521{:017}\n99\n",
        " ".repeat(50),
        pad("1300000000001", 13),
        200_000,
        pad("281234567890", 12),
        320_000
    );
    let data = taxpain_models::datos_fiscales::parse(file.as_bytes()).unwrap();
    models::store_tax_data(&v, file.as_bytes(), &data, datetime!(2027-04-10 9:00 UTC)).unwrap();
    let a = renta::activity(&v, 2026).unwrap();
    let line = |t: &str| a.check.iter().find(|l| l.topic == t).unwrap();
    assert!(line("pago_130").matches);
    assert!(!line("cotizaciones").matches);
    assert_eq!(a.check_date.as_deref(), Some("20270402"));
}

#[test]
fn renta_sólo_para_personas_físicas() {
    let v = setup();
    let mut s = invoicing::settings(&v).unwrap().unwrap();
    s.issuer.id = PartyId::Nif("B12345674".into());
    s.issuer.person = None;
    invoicing::save_settings(&v, &s, datetime!(2026-01-02 9:00 UTC)).unwrap();
    assert!(renta::activity(&v, 2026).is_err());
}

#[test]
fn renta_resta_el_pago_del_130_registrado_sin_contarlo_dos_veces() {
    let v = setup();
    first_quarter(&v);
    with_regime(&v, Some(IrpfRegime::Simplificada));
    let draft = models::draft(&v, "130", 2026, 1).unwrap().result;
    assert!(draft.cents() > 0);
    let p = tax_payments::pay(
        &v,
        "130",
        2026,
        1,
        date!(2026 - 04 - 20),
        "572",
        datetime!(2026-04-20 9:00 UTC),
    )
    .unwrap();
    assert_eq!(p.amount, draft);
    let a = renta::activity(&v, 2026).unwrap();
    assert_eq!(a.result.installments, draft);
    assert_eq!(a.missing_130, vec!["2T", "3T", "4T"]);
    // Si además consta presentado, vale lo presentado (no se suma el pago otra vez).
    let mut boxes = BTreeMap::new();
    boxes.insert(19, eur(2_000));
    presentados::save_manual(
        &v,
        "130",
        2026,
        "1T",
        &boxes,
        datetime!(2026-04-21 9:00 UTC),
    )
    .unwrap();
    let a = renta::activity(&v, 2026).unwrap();
    assert_eq!(a.result.installments, eur(2_000));
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
