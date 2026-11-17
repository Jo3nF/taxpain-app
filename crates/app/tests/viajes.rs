//! Gastos de viaje: parte exenta (art. 9 RIRPF) contabilizada en la 629 y declarada en la clave
//! L.01 del 190; exceso y dietas de un administrador sin relación laboral, avisados como
//! retribución en el 111 y el 190.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::payroll::{Employee, save_employee};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::viajes::{self, Trip};
use taxpain_app::{AppError, accounting, invoicing, models};
use taxpain_core::accounting::Origin;
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_models::informative::DirectorSs;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
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
            name: "Estudio Viajero SL".into(),
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

fn trip(nif: &str, name: &str, labor: bool, counter: &str) -> Trip {
    Trip {
        date: date!(2026 - 02 - 12),
        employee_id: None,
        nif: nif.into(),
        name: name.into(),
        labor,
        origin: "València".into(),
        destination: "Barcelona".into(),
        reason: "Taller con un cliente".into(),
        abroad: false,
        overnight: false,
        days: 1,
        km: 350,
        km_amount: Money::from_cents(9_100),
        meals: Money::from_euros(30, 0),
        lodging: Money::ZERO,
        tolls_parking: Money::from_euros(22, 0),
        transport: Money::ZERO,
        counter_account: counter.into(),
    }
}

#[test]
fn viajes_190_l01_y_avisos() {
    let v = setup();
    let now = datetime!(2026-03-01 10:00 UTC);
    let emp = save_employee(
        &v,
        &Employee {
            nif: "12345678Z".into(),
            name: "Empleada Viajera Ana".into(),
            province: "46".into(),
            birth_year: 1990,
            family: 3,
            contract: 1,
            active: true,
            ..Default::default()
        },
        now,
    )
    .unwrap();
    let admin = save_employee(
        &v,
        &Employee {
            nif: "00000000T".into(),
            name: "Administrador Viajero Luis".into(),
            province: "08".into(),
            administrator: true,
            ss_regime: DirectorSs::Autonomo,
            ..Default::default()
        },
        now,
    )
    .unwrap();

    // Comprobaciones: motivo y contrapartida.
    let mut bad = trip("12345678Z", "Empleada Viajera Ana", true, "465");
    bad.reason = String::new();
    assert!(viajes::register(&v, &bad, now).is_err());
    let mut bad = trip("12345678Z", "Empleada Viajera Ana", true, "700");
    bad.counter_account = "700".into();
    assert!(viajes::register(&v, &bad, now).is_err());

    // Empleada: 350 km × 0,26 = 91 € (exento), dieta de 30 € sin pernoctar (26,67 exentos y 3,33
    // de exceso) y 22 € de peajes.
    let mut t = trip("12345678Z", "Empleada Viajera Ana", true, "465");
    t.employee_id = Some(emp);
    let c = viajes::preview(&v, &t).unwrap();
    assert_eq!(c.exempt, Money::from_cents(9_100 + 2_667 + 2_200));
    assert_eq!(c.taxable, Money::from_cents(333));
    viajes::register(&v, &t, now).unwrap();

    // Administrador sin relación laboral: dietas y kilometraje son retribución; los peajes,
    // gasto de la sociedad.
    let mut a = trip("00000000T", "Administrador Viajero Luis", false, "551");
    a.employee_id = Some(admin);
    let c = viajes::preview(&v, &a).unwrap();
    assert_eq!(c.exempt, Money::ZERO);
    assert_eq!(c.not_income, Money::from_euros(22, 0));
    assert_eq!(c.taxable, Money::from_cents(9_100 + 3_000));
    let a_id = viajes::register(&v, &a, now).unwrap();
    // Marcado como relación laboral, se avisa de que es administrador.
    let mut a2 = a.clone();
    a2.labor = true;
    assert!(
        viajes::preview(&v, &a2)
            .unwrap()
            .warnings
            .iter()
            .any(|w| w.contains("TEAC"))
    );

    let entries: Vec<_> = accounting::journal(&v, 2026)
        .unwrap()
        .into_iter()
        .filter(|e| matches!(e.entry.origin, Origin::GastosViaje(_)))
        .collect();
    assert_eq!(entries.len(), 2);
    let posted: Money = entries
        .iter()
        .flat_map(|e| e.entry.lines.iter())
        .filter(|l| l.account.as_str() == "629")
        .map(|l| l.debit)
        .sum();
    assert_eq!(posted, Money::from_cents(13_967 + 2_200));
    assert_eq!(viajes::trips_of_year(&v, 2026).unwrap().len(), 2);

    // 190: clave L.01 sólo con lo exento de la empleada; avisos de lo que es retribución.
    let d = models::draft(&v, "190", 2026, 4).unwrap();
    assert!(
        d.details
            .iter()
            .any(|(t, m)| t.contains("clave L.01") && *m == Money::from_cents(13_967)),
        "{:?}",
        d.details
    );
    assert!(!d.details.iter().any(|(t, _)| t.contains("00000000T")));
    assert!(d.file.contains("L01"));
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("Administrador Viajero Luis"))
    );
    // 111 del 1T: aviso de los importes que han de ir en nómina.
    let d111 = models::draft(&v, "111", 2026, 1).unwrap();
    assert!(
        d111.warnings.iter().any(|w| w.contains("Gastos de viaje")),
        "{:?}",
        d111.warnings
    );

    // Borrar con confirmación.
    assert!(matches!(
        viajes::delete(&v, a_id, now, false),
        Err(AppError::NotConfirmed)
    ));
    viajes::delete(&v, a_id, now, true).unwrap();
    assert_eq!(viajes::trips_of_year(&v, 2026).unwrap().len(), 1);
}

/// Más de nueve meses seguidos desplazado en el mismo municipio: las dietas de manutención y
/// estancia dejan de estar exentas (art. 9.A.3.b RIRPF); se avisa.
#[test]
fn desplazamiento_de_mas_de_nueve_meses() {
    let v = setup();
    let now = datetime!(2026-12-01 10:00 UTC);
    let stay = |d: time::Date, days: u16| {
        let mut t = trip("12345678Z", "Empleada Viajera Ana", true, "465");
        t.date = d;
        t.destination = "Bilbao".into();
        t.overnight = true;
        t.days = days;
        t.km = 0;
        t.km_amount = Money::ZERO;
        t.tolls_parking = Money::ZERO;
        t.meals = Money::from_cents(5_334 * i64::from(days));
        t
    };
    let warned = |t: &Trip| {
        viajes::preview(&v, t)
            .unwrap()
            .warnings
            .iter()
            .any(|w| w.contains("nueve meses"))
    };
    // Una liquidación al mes, de enero a septiembre (fines de semana entre una y otra).
    for m in 1..=9u8 {
        let d = time::Date::from_calendar_date(2026, time::Month::try_from(m).unwrap(), 1).unwrap();
        let t = stay(d, 28);
        assert!(!warned(&t), "mes {m}");
        viajes::register(&v, &t, now).unwrap();
    }
    // En octubre ya pasa de nueve meses.
    let october = stay(date!(2026 - 10 - 01), 28);
    assert!(warned(&october));
    // Otro destino, u otra persona, no.
    let mut elsewhere = october.clone();
    elsewhere.destination = "Sevilla".into();
    assert!(!warned(&elsewhere));
    let mut other = october.clone();
    other.nif = "00000000T".into();
    assert!(!warned(&other));
    // Tras un mes sin ir, vuelve a empezar.
    let december = stay(date!(2026 - 12 - 01), 20);
    assert!(!warned(&december));
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
