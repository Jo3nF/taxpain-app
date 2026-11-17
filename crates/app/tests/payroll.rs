//! Nóminas: ficha del empleado, recibos contabilizados, 111 y claves A y E del 190.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::{self, journal};
use taxpain_app::invoicing;
use taxpain_app::models;
use taxpain_app::payroll::{Employee, PayslipInput, delete, employees, register, save_employee};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::accounting::Origin;
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
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
            name: "Taller Ejemplo SL".into(),
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

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

#[test]
fn nominas_111_y_190_clave_a() {
    let v = setup();
    let now = datetime!(2026-12-01 10:00 UTC);
    let mut e = Employee {
        nif: "12345678z".into(),
        name: "Española Española Ana".into(),
        province: "46".into(),
        birth_year: 1990,
        family: 3,
        contract: 1,
        children_rest: 1,
        children_whole: true,
        active: true,
        ..Default::default()
    };
    assert!(
        save_employee(
            &v,
            &Employee {
                nif: "1234".into(),
                ..e.clone()
            },
            now
        )
        .is_err()
    );
    let id = save_employee(&v, &e, now).unwrap();
    assert!(save_employee(&v, &e, now).is_err(), "NIF repetido");
    e.id = id;
    e.birth_year = 1991;
    save_employee(&v, &e, now).unwrap();
    assert_eq!(employees(&v).unwrap()[0].nif, "12345678Z");

    let slip = |d: time::Date| PayslipInput {
        employee_id: id,
        date: d,
        gross: eur(2000),
        irpf: eur(240),
        ss_employee: eur(130),
        ss_company: eur(640),
        small_entity: None,
    };
    for d in [
        date!(2026 - 01 - 31),
        date!(2026 - 02 - 28),
        date!(2026 - 04 - 30),
    ] {
        register(&v, &slip(d), now).unwrap();
    }
    let extra = register(&v, &slip(date!(2026 - 05 - 31)), now).unwrap();
    delete(&v, extra, now, true).unwrap();
    let entries: Vec<_> = journal(&v, 2026)
        .unwrap()
        .into_iter()
        .filter(|e| e.entry.origin == Origin::Nomina)
        .collect();
    assert_eq!(entries.len(), 3);
    assert_eq!(
        entries[0].entry.concept,
        "Nómina Española Española Ana 01/2026"
    );

    // 111 1T: dos nóminas de un perceptor.
    let d = models::draft(&v, "111", 2026, 1).unwrap();
    let get = |d: &models::Draft, b: u16| {
        d.boxes
            .iter()
            .find(|(k, _)| *k == b)
            .map_or(Money::ZERO, |(_, m)| *m)
    };
    assert_eq!(get(&d, 2), eur(4000));
    assert_eq!(get(&d, 3), eur(480));
    assert!(d.payees.contains(&(1, 1)), "{:?}", d.payees);

    // 190: clave A con 6.000 de bruto, 720 de IRPF y 390 de cotizaciones como gasto.
    let d190 = models::draft(&v, "190", 2026, 4).unwrap();
    assert_eq!(d190.result, eur(720));
    assert!(
        d190.details
            .iter()
            .any(|(t, m)| t.contains("clave A") && *m == eur(720))
    );
    assert!(
        !d190.warnings.iter().any(|w| w.contains("a mano")),
        "{:?}",
        d190.warnings
    );
    assert!(d190.file.contains("12345678Z"));
    assert!(d190.file.contains("A00"));
}

fn sales(v: &Vault, d: time::Date, euros: i64) {
    use taxpain_core::accounting::account::acc;
    use taxpain_core::accounting::{EntryLine, JournalEntry};
    taxpain_app::accounting::manual_entry(
        v,
        &JournalEntry {
            date: d,
            concept: "Ventas del ejercicio".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("5720001"), eur(euros)),
                EntryLine::credit(acc("705"), eur(euros)),
            ],
        },
        datetime!(2026-01-10 10:00 UTC),
    )
    .unwrap();
}

#[test]
fn nomina_de_un_ejercicio_cerrado_pide_reabrirlo() {
    let v = setup();
    let now = datetime!(2027-01-15 10:00 UTC);
    let e = Employee {
        nif: "12345678z".into(),
        name: "Española Española Ana".into(),
        province: "46".into(),
        birth_year: 1990,
        family: 3,
        contract: 1,
        active: true,
        ..Default::default()
    };
    let id = save_employee(&v, &e, now).unwrap();
    let slip = |d: time::Date| PayslipInput {
        employee_id: id,
        date: d,
        gross: eur(2000),
        irpf: eur(240),
        ss_employee: eur(130),
        ss_company: eur(640),
        small_entity: None,
    };
    let first = register(&v, &slip(date!(2026 - 06 - 30)), now).unwrap();
    accounting::close_year(&v, 2026, now, true).unwrap();
    // Ni registrar ni borrar en el ejercicio cerrado: aviso de la app, no el error de SQLite.
    let err = register(&v, &slip(date!(2026 - 07 - 31)), now)
        .unwrap_err()
        .to_string();
    assert!(err.contains("cerrado: reábrelo"), "{err}");
    let err = delete(&v, first, now, true).unwrap_err().to_string();
    assert!(err.contains("cerrado: reábrelo"), "{err}");
    let nominas = journal(&v, 2026)
        .unwrap()
        .into_iter()
        .filter(|e| e.entry.origin == Origin::Nomina)
        .count();
    assert_eq!(nominas, 1, "el recibo cerrado sigue ahí");
}

#[test]
fn administrador_clave_e_y_111() {
    use taxpain_app::payroll::director_turnover;
    use taxpain_models::informative::DirectorSs;
    let v = setup();
    let now = datetime!(2026-12-01 10:00 UTC);
    let admin = Employee {
        nif: "12345678Z".into(),
        name: "Administradora Ejemplo Ana".into(),
        province: "46".into(),
        administrator: true,
        ss_regime: DirectorSs::Autonomo,
        ..Default::default()
    };
    let id = save_employee(&v, &admin, now).unwrap();
    let saved = employees(&v).unwrap();
    assert!(saved[0].administrator);
    assert_eq!(saved[0].ss_regime, DirectorSs::Autonomo);
    let slip = |d: time::Date, gross: i64, irpf: i64| PayslipInput {
        employee_id: id,
        date: d,
        gross: eur(gross),
        irpf: eur(irpf),
        ss_employee: Money::ZERO,
        ss_company: Money::ZERO,
        small_entity: None,
    };

    // Sin la contabilidad de 2025 hay que decir si la cifra de negocios fue menor de 100.000 €.
    let t = director_turnover(&v, date!(2026 - 03 - 31)).unwrap();
    assert_eq!((t.year, t.turnover, t.small), (2025, None, None));
    assert!(register(&v, &slip(date!(2026 - 03 - 31), 3000, 570), now).is_err());
    // En el RETA, sin cotizaciones en el recibo.
    let mut with_ss = slip(date!(2026 - 03 - 31), 3000, 570);
    with_ss.small_entity = Some(true);
    with_ss.ss_employee = eur(100);
    assert!(register(&v, &with_ss, now).is_err());

    // 2025 con 80.000 € de ventas: entidad de menos de 100.000 € → 19 % y subclave E.03.
    sales(&v, date!(2025 - 01 - 31), 80_000);
    let t = director_turnover(&v, date!(2026 - 03 - 31)).unwrap();
    assert_eq!(t.turnover, Some(eur(80_000)));
    assert_eq!(t.small, Some(true));
    assert_eq!(
        taxpain_app::payroll::director_expected_irpf(eur(3000), true),
        eur(570)
    );
    register(&v, &slip(date!(2026 - 03 - 31), 3000, 570), now).unwrap();
    register(&v, &slip(date!(2026 - 06 - 30), 3000, 570), now).unwrap();
    let slips = taxpain_app::payroll::payslips_of_year(&v, 2026).unwrap();
    assert!(slips.iter().all(|s| s.director_subkey == Some(3)));

    // 111: rendimientos del trabajo.
    let get = |d: &models::Draft, b: u16| {
        d.boxes
            .iter()
            .find(|(k, _)| *k == b)
            .map_or(Money::ZERO, |(_, m)| *m)
    };
    let d = models::draft(&v, "111", 2026, 1).unwrap();
    assert_eq!(get(&d, 2), eur(3000));
    assert_eq!(get(&d, 3), eur(570));
    assert!(d.payees.contains(&(1, 1)));

    // Un empleado asimilado en una entidad de más de 100.000 €: E.01 con sus cotizaciones.
    let asim = Employee {
        nif: "00000000T".into(),
        name: "Consejero Ejemplo Luis".into(),
        province: "28".into(),
        administrator: true,
        ss_regime: DirectorSs::Asimilado,
        ..Default::default()
    };
    let id2 = save_employee(&v, &asim, now).unwrap();
    register(
        &v,
        &PayslipInput {
            employee_id: id2,
            date: date!(2026 - 06 - 30),
            gross: eur(2000),
            irpf: eur(700),
            ss_employee: eur(120),
            ss_company: eur(500),
            small_entity: Some(false),
        },
        now,
    )
    .unwrap();

    let d190 = models::draft(&v, "190", 2026, 4).unwrap();
    assert_eq!(d190.result, eur(1840));
    assert!(
        d190.details
            .iter()
            .any(|(t, m)| t.contains("clave E.03") && *m == eur(1140)),
        "{:?}",
        d190.details
    );
    assert!(
        d190.details
            .iter()
            .any(|(t, m)| t.contains("clave E.01") && *m == eur(700))
    );
    assert!(d190.file.contains("E03"));
    assert!(d190.file.contains("E01"));
}

fn entry(v: &Vault, d: time::Date, concept: &str, lines: &[(&str, i64, i64)]) {
    use taxpain_core::accounting::account::acc;
    use taxpain_core::accounting::{EntryLine, JournalEntry};
    let lines = lines
        .iter()
        .map(|(a, debit, credit)| {
            if *debit > 0 {
                EntryLine::debit(acc(a), eur(*debit))
            } else {
                EntryLine::credit(acc(a), eur(*credit))
            }
        })
        .collect();
    taxpain_app::accounting::manual_entry(
        v,
        &JournalEntry {
            date: d,
            concept: concept.into(),
            origin: Origin::Manual,
            lines,
        },
        datetime!(2026-01-10 10:00 UTC),
    )
    .unwrap();
}

fn autumn_sales(v: &Vault) {
    for d in [
        date!(2025 - 09 - 30),
        date!(2025 - 10 - 31),
        date!(2025 - 11 - 30),
        date!(2025 - 12 - 31),
    ] {
        entry(v, d, "Ventas", &[("430", 10_000, 0), ("705", 0, 10_000)]);
    }
}

/// Primer ejercicio de una sociedad constituida en septiembre: sus 40.000 € de ventas se elevan
/// al año (≈ 119.672 €, art. 101.2 LIRPF) → 35 %.
#[test]
fn cifra_de_negocios_del_primer_ejercicio_se_eleva_al_anio() {
    use taxpain_app::payroll::director_turnover;
    let v = setup();
    entry(
        &v,
        date!(2025 - 09 - 01),
        "Aportación del capital",
        &[("572", 3_000, 0), ("100", 0, 3_000)],
    );
    autumn_sales(&v);
    let t = director_turnover(&v, date!(2026 - 01 - 31)).unwrap();
    assert_eq!(t.turnover, Some(Money::from_cents(11_967_213)));
    assert_eq!(t.small, Some(false));
}

/// Sin asiento de capital ni escritura, un primer ejercicio que empieza en septiembre no se sabe
/// si es una sociedad nueva o TaxPain empezado a mitad de año: lo decide el usuario.
#[test]
fn cifra_de_negocios_incompleta_la_decide_el_usuario() {
    use taxpain_app::payroll::director_turnover;
    let v = setup();
    autumn_sales(&v);
    let t = director_turnover(&v, date!(2026 - 01 - 31)).unwrap();
    assert_eq!((t.turnover, t.small), (Some(eur(40_000)), None));
    // Con la escritura de constitución en Documentos (constituida el 1/9), se eleva al año.
    let facts = taxpain_app::documentos::Facts {
        constitution_date: Some(date!(2025 - 09 - 01)),
        ..Default::default()
    };
    taxpain_app::documentos::store(
        &v,
        "escritura.pdf",
        b"%PDF-escritura",
        &taxpain_app::documentos::Recognized {
            kind: taxpain_app::documentos::DocKind::EscrituraConstitucion,
            title: "Escritura de constitución".into(),
            facts,
            by_name: false,
        },
        datetime!(2026-01-10 10:00 UTC),
    )
    .unwrap();
    let t = director_turnover(&v, date!(2026 - 01 - 31)).unwrap();
    assert_eq!(t.small, Some(false));
}

/// Un ejercicio importado con su regularización (6/7 contra la 129) como asiento normal: la
/// cifra de negocios es la de las ventas, no cero.
#[test]
fn cifra_de_negocios_sin_la_regularizacion_importada() {
    use taxpain_app::payroll::director_turnover;
    let v = setup();
    entry(
        &v,
        date!(2024 - 12 - 31),
        "Ventas 2024",
        &[("430", 90_000, 0), ("705", 0, 90_000)],
    );
    entry(
        &v,
        date!(2025 - 03 - 31),
        "Ventas",
        &[("430", 150_000, 0), ("705", 0, 150_000)],
    );
    entry(
        &v,
        date!(2025 - 12 - 31),
        "Regularización",
        &[("705", 150_000, 0), ("129", 0, 150_000)],
    );
    let t = director_turnover(&v, date!(2026 - 01 - 31)).unwrap();
    assert_eq!((t.turnover, t.small), (Some(eur(150_000)), Some(false)));
}

/// Guarda el 200 de `year` presentado en la AEAT con la casilla [00255] leída (en céntimos).
fn aeat_200(v: &Vault, year: i32, cifra_cents: i64) {
    use taxpain_app::presentado_aeat::{self, Answer, Presented};
    let csv = format!("C200{year}0AXXXXXXX");
    let a = Answer {
        models: vec!["200".into()],
        list: vec![Presented {
            model: "200".into(),
            year,
            period: "0A".into(),
            csv: csv.clone(),
            file_number: String::new(),
            receipt: format!("200{year}000001"),
            previous_receipt: None,
            presented_at: format!("{}-07-24T10:00:00", year + 1),
        }],
        ..Answer::default()
    };
    presentado_aeat::record(v, "B12345674", year, &a, datetime!(2026-10-27 10:00 UTC)).unwrap();
    v.connection()
        .execute(
            "UPDATE aeat_filing SET boxes = ?1, boxes_source = 'fichero' WHERE csv = ?2",
            [
                format!(r#"{{"casillas":{{"255":{cifra_cents},"500":100000,"180":9000000,"252":9000000}}}}"#),
                csv,
            ],
        )
        .unwrap();
}

/// La contabilidad de 2025 sin ventas (TaxPain sin acabar) no dice que la cifra fue cero: sin
/// 200, no se sabe; con el 200 presentado, su [00255].
#[test]
fn cifra_de_negocios_del_200_presentado() {
    use taxpain_app::payroll::{TurnoverSource, director_turnover};
    let v = setup();
    entry(
        &v,
        date!(2025 - 03 - 31),
        "Alquiler",
        &[("621", 1_000, 0), ("5720001", 0, 1_000)],
    );
    let t = director_turnover(&v, date!(2026 - 01 - 31)).unwrap();
    assert_eq!((t.turnover, t.small), (None, None));
    assert_eq!(t.source, TurnoverSource::Contabilidad);

    aeat_200(&v, 2025, 4_800_000);
    let t = director_turnover(&v, date!(2026 - 01 - 31)).unwrap();
    assert_eq!(t.year, 2025);
    assert_eq!((t.turnover, t.small), (Some(eur(48_000)), Some(true)));
    assert_eq!(t.source, TurnoverSource::Aeat);
}

/// El 200 manda sobre la contabilidad: 150.000 € de ventas en TaxPain, 90.000 € en el 200.
#[test]
fn el_200_presentado_manda_sobre_la_contabilidad() {
    use taxpain_app::payroll::{TurnoverSource, director_turnover};
    let v = setup();
    sales(&v, date!(2025 - 01 - 31), 150_000);
    let t = director_turnover(&v, date!(2026 - 01 - 31)).unwrap();
    assert_eq!(
        (t.small, t.source),
        (Some(false), TurnoverSource::Contabilidad)
    );
    aeat_200(&v, 2025, 9_000_000);
    let t = director_turnover(&v, date!(2026 - 01 - 31)).unwrap();
    assert_eq!((t.turnover, t.small), (Some(eur(90_000)), Some(true)));
    assert_eq!(t.source, TurnoverSource::Aeat);
    // Un 200 de otro ejercicio no cuenta para 2025.
    let t = director_turnover(&v, date!(2027 - 01 - 31)).unwrap();
    assert_eq!((t.year, t.turnover), (2026, None));
}

/// Sin el del AEAT, un 200 guardado en TaxPain con la [00255]; en el primer ejercicio (escritura
/// del 1/9), elevada al año.
#[test]
fn cifra_de_negocios_del_200_guardado_y_elevada() {
    use taxpain_app::payroll::{TurnoverSource, director_turnover};
    let v = setup();
    let boxes = [(255u16, eur(40_000)), (500, eur(1_000))]
        .into_iter()
        .collect();
    taxpain_app::presentados::save_manual(
        &v,
        "200",
        2025,
        "0A",
        &boxes,
        datetime!(2026-07-20 10:00 UTC),
    )
    .unwrap();
    let t = director_turnover(&v, date!(2026 - 01 - 31)).unwrap();
    assert_eq!((t.turnover, t.small), (Some(eur(40_000)), Some(true)));
    assert_eq!(t.source, TurnoverSource::Guardado);
    let facts = taxpain_app::documentos::Facts {
        constitution_date: Some(date!(2025 - 09 - 01)),
        ..Default::default()
    };
    taxpain_app::documentos::store(
        &v,
        "escritura.pdf",
        b"%PDF-escritura",
        &taxpain_app::documentos::Recognized {
            kind: taxpain_app::documentos::DocKind::EscrituraConstitucion,
            title: "Escritura de constitución".into(),
            facts,
            by_name: false,
        },
        datetime!(2026-01-10 10:00 UTC),
    )
    .unwrap();
    let t = director_turnover(&v, date!(2026 - 01 - 31)).unwrap();
    assert_eq!(
        (t.turnover, t.small),
        (Some(Money::from_cents(11_967_213)), Some(false))
    );
}

/// Un 200 leído sin la [00255] (las casillas a cero no se guardan, y puede faltar la página) no
/// es una cifra de negocios de cero: sin ventas en la contabilidad, no se sabe.
#[test]
fn el_200_sin_la_casilla_255_no_es_cero() {
    use taxpain_app::payroll::director_turnover;
    let v = setup();
    entry(
        &v,
        date!(2025 - 03 - 31),
        "Alquiler",
        &[("621", 1_000, 0), ("5720001", 0, 1_000)],
    );
    aeat_200(&v, 2025, 4_800_000);
    v.connection()
        .execute(
            r#"UPDATE aeat_filing SET boxes = '{"casillas":{"500":100000,"180":9000000,"252":9000000}}'"#,
            [],
        )
        .unwrap();
    let t = director_turnover(&v, date!(2026 - 01 - 31)).unwrap();
    assert_eq!((t.turnover, t.small), (None, None));
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
