//! Avisos de umbral con datos sintéticos: 347, 349, gran empresa, 720, 232, tipo del IS e IAE.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::record_entry;
use taxpain_app::invoicing::{self, InvoiceDraft, issue};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::umbrales::{self, Severity, ThresholdAlert};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::calendar::{Periodicity, TaxpayerProfile};
use taxpain_core::invoice::{
    Exemption, IdType, InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment,
};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime, Time};

const HOY: Date = date!(2026 - 10 - 26);

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn at(d: Date) -> OffsetDateTime {
    d.with_time(Time::from_hms(10, 0, 0).unwrap()).assume_utc()
}

fn setup(profile: &TaxpayerProfile) -> Vault {
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
    invoicing::save_settings(&v, &s, datetime!(2025-01-01 9:00 UTC)).unwrap();
    v.set_setting("taxpayer_profile", profile).unwrap();
    v
}

fn emit(v: &Vault, recipient: Party, vat: VatTreatment, euros: i64, d: Date) {
    let draft = InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: d,
        operation_date: None,
        recipient: Some(recipient),
        lines: vec![InvoiceLine {
            description: "Servicios".into(),
            quantity_milli: 1000,
            unit_price: eur(euros),
            discount: Rate::ZERO,
            vat,
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
    issue(v, &draft, at(d), true).unwrap();
}

fn cliente() -> Party {
    Party {
        id: PartyId::Nif("B98765431".into()),
        name: "Cliente Demo SL".into(),
        address: addr(),
        person: None,
    }
}

fn iva21() -> VatTreatment {
    VatTreatment::Taxed {
        rate: Rate::percent(21),
    }
}

fn with<'a>(alerts: &'a [ThresholdAlert], code: &str) -> Vec<&'a ThresholdAlert> {
    alerts.iter().filter(|a| a.code == code).collect()
}

#[test]
fn sin_perfil_no_hay_avisos() {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    assert!(umbrales::alerts(&v, HOY).unwrap().is_empty());
}

#[test]
fn el_347_avisa_si_no_esta_en_el_perfil_y_no_con_sii() {
    let p = TaxpayerProfile::autonomo();
    let v = setup(&p);
    emit(&v, cliente(), iva21(), 2000, date!(2026 - 03 - 10));
    assert!(with(&umbrales::alerts(&v, HOY).unwrap(), "347").is_empty());
    emit(&v, cliente(), iva21(), 1000, date!(2026 - 05 - 10));
    let a = umbrales::alerts(&v, HOY).unwrap();
    let a = with(&a, "347");
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].severity, Severity::Warning);
    assert!(a[0].detail.contains("Cliente Demo SL"), "{}", a[0].detail);
    assert!(a[0].action.contains("febrero de 2027"));
    // Marcado en el perfil, o con SII, no se avisa.
    v.set_setting(
        "taxpayer_profile",
        &TaxpayerProfile {
            obligado_347: true,
            ..p.clone()
        },
    )
    .unwrap();
    assert!(with(&umbrales::alerts(&v, HOY).unwrap(), "347").is_empty());
    let mut sii = p;
    sii.iva = sii.iva.map(|mut i| {
        i.sii = true;
        i
    });
    v.set_setting("taxpayer_profile", &sii).unwrap();
    assert!(with(&umbrales::alerts(&v, HOY).unwrap(), "347").is_empty());
}

#[test]
fn el_349_falta_en_el_perfil_o_pasa_a_mensual() {
    let p = TaxpayerProfile::sociedad_limitada();
    let v = setup(&p);
    let de = Party {
        id: PartyId::Foreign {
            country: "DE".into(),
            id_type: IdType::NifIva,
            id: "DE123456789".into(),
        },
        name: "Kunde GmbH".into(),
        address: addr(),
        person: None,
    };
    emit(
        &v,
        de.clone(),
        VatTreatment::Exempt(Exemption::E5),
        10_000,
        date!(2026 - 02 - 10),
    );
    let a = umbrales::alerts(&v, HOY).unwrap();
    let a = with(&a, "349");
    assert_eq!(a.len(), 1);
    assert!(a[0].action.contains("«Sí, trimestral»"), "{}", a[0].action);
    emit(
        &v,
        de,
        VatTreatment::Exempt(Exemption::E5),
        45_000,
        date!(2026 - 03 - 10),
    );
    // Trimestral: 55.000 € en el 1T 2026 → mensual.
    v.set_setting(
        "taxpayer_profile",
        &TaxpayerProfile {
            intracomunitarias: Some(Periodicity::Trimestral),
            ..p.clone()
        },
    )
    .unwrap();
    let a = umbrales::alerts(&v, HOY).unwrap();
    let a = with(&a, "349");
    assert_eq!(a[0].title, "El 349 pasa a ser mensual");
    assert!(a[0].detail.contains("1T 2026"), "{}", a[0].detail);
    v.set_setting(
        "taxpayer_profile",
        &TaxpayerProfile {
            intracomunitarias: Some(Periodicity::Mensual),
            ..p
        },
    )
    .unwrap();
    assert!(with(&umbrales::alerts(&v, HOY).unwrap(), "349").is_empty());
}

#[test]
fn gran_empresa_por_el_volumen_del_año_anterior() {
    let p = TaxpayerProfile {
        obligado_347: true,
        ..TaxpayerProfile::sociedad_limitada()
    };
    let v = setup(&p);
    emit(&v, cliente(), iva21(), 6_000_000, date!(2025 - 06 - 10));
    // 6.000.000 € no llega a 6.010.121,04 €.
    assert!(with(&umbrales::alerts(&v, HOY).unwrap(), "gran_empresa").is_empty());
    emit(&v, cliente(), iva21(), 20_000, date!(2025 - 07 - 10));
    let a = umbrales::alerts(&v, HOY).unwrap();
    let a = with(&a, "gran_empresa");
    assert_eq!(a[0].title, "En 2026 eres gran empresa");
    assert!(a[0].action.contains("SII"));
    assert!(a[0].action.contains("art. 40.3 LIS"));
}

#[test]
fn cuentas_en_el_extranjero_720() {
    let v = setup(&TaxpayerProfile::autonomo());
    v.connection()
        .execute(
            "INSERT INTO bank_account(iban, number, account, name)
             VALUES ('LT121000011101001000', 'LT121000011101001000', '5720009', 'Neobanco')",
            [],
        )
        .unwrap();
    let ingreso = |d: Date, e: i64| {
        record_entry(
            v.connection(),
            &JournalEntry {
                date: d,
                concept: "Traspaso".into(),
                origin: Origin::Manual,
                lines: vec![
                    EntryLine::debit(acc("5720009"), eur(e)),
                    EntryLine::credit(acc("5720001"), eur(e)),
                ],
            },
            at(d),
        )
        .unwrap();
    };
    ingreso(date!(2025 - 11 - 03), 40_000);
    assert!(with(&umbrales::alerts(&v, HOY).unwrap(), "720").is_empty());
    ingreso(date!(2025 - 12 - 15), 15_000);
    // Lo de 2026 no cuenta para el 31/12/2025.
    ingreso(date!(2026 - 01 - 15), 99_000);
    let a = umbrales::alerts(&v, HOY).unwrap();
    let a = with(&a, "720");
    assert_eq!(a.len(), 1);
    assert!(a[0].detail.contains("55.000,00 €"), "{}", a[0].detail);
    assert!(a[0].action.contains("31 de marzo de 2026"));
}

#[test]
fn operaciones_vinculadas_232() {
    let v = setup(&TaxpayerProfile::sociedad_limitada());
    emit(&v, cliente(), iva21(), 260_000, date!(2026 - 04 - 10));
    // Sin socios del 25 % no es vinculada.
    assert!(with(&umbrales::alerts(&v, HOY).unwrap(), "232").is_empty());
    v.connection()
        .execute(
            "INSERT INTO shareholder(nif, name, pct, legal_person, created_at)
             VALUES ('B-98765431', 'Cliente Demo SL', 300000, 1, '2025-01-01')",
            [],
        )
        .unwrap();
    let a = umbrales::alerts(&v, HOY).unwrap();
    let a = with(&a, "232");
    assert_eq!(a.len(), 1);
    assert_eq!(a[0].severity, Severity::Info);
    assert!(a[0].detail.contains("Cliente Demo SL"));
    assert!(a[0].action.contains("noviembre de 2027"));
    // Con el 232 en el perfil, nada.
    v.set_setting(
        "taxpayer_profile",
        &TaxpayerProfile {
            operaciones_vinculadas: true,
            ..TaxpayerProfile::sociedad_limitada()
        },
    )
    .unwrap();
    assert!(with(&umbrales::alerts(&v, HOY).unwrap(), "232").is_empty());
}

#[test]
fn cifra_de_negocios_tipo_del_is_e_iae() {
    let v = setup(&TaxpayerProfile::sociedad_limitada());
    let asiento = |d: Date, e: i64| {
        record_entry(
            v.connection(),
            &JournalEntry {
                date: d,
                concept: "Ventas".into(),
                origin: Origin::Manual,
                lines: vec![
                    EntryLine::debit(acc("5720001"), eur(e)),
                    EntryLine::credit(acc("700"), eur(e)),
                ],
            },
            at(d),
        )
        .unwrap();
    };
    asiento(date!(2025 - 01 - 05), 1_500_000);
    asiento(date!(2026 - 01 - 05), 1000);
    let a = umbrales::alerts(&v, HOY).unwrap();
    let is = with(&a, "is_regimen");
    assert_eq!(is.len(), 1, "{a:?}");
    assert!(is[0].detail.contains("reducida dimensión"));
    assert!(is[0].detail.contains("según la contabilidad de TaxPain"));
    let iae = with(&a, "iae");
    assert_eq!(iae[0].title, "Desde 2027 pagas el IAE");
    assert!(iae[0].action.contains("diciembre de 2026"));
    // Con el año del IAE en el perfil, ya no se avisa.
    v.set_setting(
        "taxpayer_profile",
        &TaxpayerProfile {
            iae_desde: Some(2027),
            ..TaxpayerProfile::sociedad_limitada()
        },
    )
    .unwrap();
    assert!(with(&umbrales::alerts(&v, HOY).unwrap(), "iae").is_empty());
}

/// Guarda el 200 de 2025 presentado en la AEAT con la casilla [00255] leída.
fn aeat_200_2025(v: &Vault, cifra: Money) {
    use taxpain_app::presentado_aeat::{self, Answer, Presented};
    let csv = "C20020250AXXXXXX".to_owned();
    let a = Answer {
        models: vec!["200".into()],
        list: vec![Presented {
            model: "200".into(),
            year: 2025,
            period: "0A".into(),
            csv: csv.clone(),
            file_number: String::new(),
            receipt: "2002025000001".into(),
            previous_receipt: None,
            presented_at: "2026-07-24T10:00:00".into(),
        }],
        ..Answer::default()
    };
    presentado_aeat::record(v, "B12345674", 2025, &a, at(HOY)).unwrap();
    v.connection()
        .execute(
            "UPDATE aeat_filing SET boxes = ?1, boxes_source = 'fichero' WHERE csv = ?2",
            [
                format!(
                    r#"{{"casillas":{{"255":{},"500":100000,"180":9000000,"252":9000000}}}}"#,
                    cifra.cents()
                ),
                csv,
            ],
        )
        .unwrap();
}

/// La contabilidad de 2025 sin ventas (sin acabar) no es una cifra de negocios de cero: sin el
/// 200 no hay aviso; con el 200 presentado, el aviso usa su [00255] y lo dice.
#[test]
fn cifra_de_negocios_del_200_presentado_y_no_de_la_contabilidad_sin_ventas() {
    let v = setup(&TaxpayerProfile::sociedad_limitada());
    record_entry(
        v.connection(),
        &JournalEntry {
            date: date!(2025 - 03 - 31),
            concept: "Alquiler".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("621"), eur(1_000)),
                EntryLine::credit(acc("5720001"), eur(1_000)),
            ],
        },
        at(date!(2025 - 03 - 31)),
    )
    .unwrap();
    let a = umbrales::alerts(&v, HOY).unwrap();
    assert!(with(&a, "is_regimen").is_empty(), "{a:?}");

    aeat_200_2025(&v, eur(48_000));
    let a = umbrales::alerts(&v, HOY).unwrap();
    let is = with(&a, "is_regimen");
    assert_eq!(is.len(), 1, "{a:?}");
    assert!(is[0].detail.contains("48.000,00 €"), "{}", is[0].detail);
    assert!(
        is[0].detail.contains("según el 200 presentado"),
        "{}",
        is[0].detail
    );
    assert!(is[0].detail.contains("microempresa"), "{}", is[0].detail);
    assert!(!is[0].detail.contains("fue 0,00 €"), "{}", is[0].detail);
}

#[test]
fn cifra_de_negocios_de_2_millones_del_200_es_reducida_dimension() {
    let v = setup(&TaxpayerProfile::sociedad_limitada());
    aeat_200_2025(&v, eur(2_000_000));
    let a = umbrales::alerts(&v, HOY).unwrap();
    let is = with(&a, "is_regimen");
    assert_eq!(is.len(), 1, "{a:?}");
    assert!(is[0].detail.contains("2.000.000,00 €"), "{}", is[0].detail);
    assert!(is[0].detail.contains("reducida dimensión"));
    let iae = with(&a, "iae");
    assert!(
        iae[0].detail.contains("según el 200 presentado"),
        "{}",
        iae[0].detail
    );
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
