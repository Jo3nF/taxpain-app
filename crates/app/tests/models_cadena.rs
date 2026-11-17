//! Encadenar los modelos con lo presentado: la compensación del 303 de un año al siguiente, los
//! trimestres que presentó el gestor y las nóminas de su diario. Datos inventados.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;

use taxpain_app::models::{self, ModelSettings};
use taxpain_app::received::{Nature, Reviewed, Source, ingest_file, review};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::{invoicing, presentados};
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime, Time};

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
            id: PartyId::Nif("A22222228".into()),
            name: "Mi Empresa SL".into(),
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
    invoicing::save_settings(&v, &s, datetime!(2024-01-01 9:00 UTC)).unwrap();
    v
}

fn at(d: Date) -> OffsetDateTime {
    d.with_time(Time::from_hms(10, 0, 0).unwrap()).assume_utc()
}

/// Compra revisada el día `d` (su IVA se deduce en ese trimestre).
fn receive(v: &Vault, number: &str, base: i64, vat: i64, d: Date) {
    let rec = ingest_file(
        v,
        &format!("{number}.xml"),
        format!("<?xml version=\"1.0\"?><x n=\"{number}\"/>").as_bytes(),
        Source::Manual,
        at(d),
    )
    .unwrap();
    review(
        v,
        rec.id,
        &Reviewed {
            supplier_nif: "B12345674".into(),
            supplier_name: "Proveedor".into(),
            number: number.into(),
            issue_date: d,
            base: eur(base),
            vat: eur(vat),
            withholding: eur(0),
            total: eur(base + vat),
            deductible_pct: 100,
            capital_good: false,
            nature: Nature::General,
            asset_class: None,
            vat_regime: Default::default(),
            supplier_country: None,
            self_assessed_rate: None,
            intra_goods: false,
            cash_basis: false,
            import_dua: false,
            prorata_use: Default::default(),
        },
        at(d),
    )
    .unwrap();
}

fn get(d: &models::Draft, b: u16) -> Money {
    d.boxes
        .iter()
        .find(|(k, _)| *k == b)
        .map_or(Money::ZERO, |(_, v)| *v)
}

#[test]
fn lo_que_queda_por_compensar_pasa_al_ano_siguiente() {
    let v = setup();
    receive(&v, "G-1", 1000, 210, date!(2026 - 11 - 10));
    let q4 = models::draft(&v, "303", 2026, 4).unwrap();
    assert_eq!((q4.result, q4.declaration.as_str()), (eur(-210), "C"));
    let q1 = models::draft(&v, "303", 2027, 1).unwrap();
    assert_eq!(get(&q1, 110), eur(210), "{:?}", q1.boxes);
    assert_eq!(get(&q1, 87), eur(210));

    // Lo anotado en los ajustes vale al empezar la cadena, no cada año otra vez.
    let s = ModelSettings {
        vat_compensation_carried: eur(500),
        ..models::settings(&v).unwrap()
    };
    models::save_settings(&v, &s, at(date!(2026 - 01 - 02))).unwrap();
    assert_eq!(
        get(&models::draft(&v, "303", 2026, 1).unwrap(), 110),
        eur(500)
    );
    assert_eq!(
        get(&models::draft(&v, "303", 2027, 1).unwrap(), 110),
        eur(710)
    );
    // El 390 de 2027 parte de lo pendiente al empezar 2027.
    let m390 = models::draft(&v, "390", 2027, 0).unwrap();
    assert!(
        m390.warnings.iter().all(|w| !w.contains("no coincide")),
        "{:?}",
        m390.warnings
    );

    // Con el año anotado, sólo ese año empieza con ese importe.
    let s = ModelSettings {
        vat_compensation_year: Some(2027),
        ..models::settings(&v).unwrap()
    };
    models::save_settings(&v, &s, at(date!(2026 - 01 - 02))).unwrap();
    assert_eq!(
        get(&models::draft(&v, "303", 2026, 1).unwrap(), 110),
        Money::ZERO
    );
    assert_eq!(
        get(&models::draft(&v, "303", 2027, 1).unwrap(), 110),
        eur(500)
    );
}

#[test]
fn los_trimestres_del_gestor_encadenan_con_los_de_taxpain() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    // El gestor presentó el 1T (a ingresar) y el 3T (a compensar 300) de 2025; TaxPain hace el
    // 4T con una compra.
    let filed = |q: &str, pairs: &[(u16, i64)]| {
        let b: BTreeMap<u16, Money> = pairs.iter().map(|(k, e)| (*k, eur(*e))).collect();
        presentados::save_manual(&v, "303", 2025, q, &b, now).unwrap();
    };
    filed("1T", &[(27, 500), (46, 500), (71, 500)]);
    filed("3T", &[(29, 300), (45, 300), (46, -300), (71, -300)]);
    receive(&v, "G-4", 1000, 210, date!(2025 - 11 - 10));
    let q4 = models::draft(&v, "303", 2025, 4).unwrap();
    assert_eq!(get(&q4, 110), eur(300), "{:?}", q4.boxes);
    assert_eq!(q4.result, eur(-210));
    // El 390: lo ingresado del 1T del gestor en la [95] y el 4T a compensar en la [97].
    let m390 = models::draft(&v, "390", 2025, 0).unwrap();
    assert_eq!(get(&m390, 95), eur(500), "{:?}", m390.boxes);
    assert_eq!(get(&m390, 97), eur(210));
}

#[test]
fn el_4t_del_gestor_en_fichero_dice_si_se_compenso_o_se_pidio_devolver() {
    let now = datetime!(2026-10-24 10:00 UTC);
    for (refund, carried) in [(false, eur(210)), (true, Money::ZERO)] {
        // El 303 del 4T de 2024 «del gestor»: el fichero que saldría de sus datos.
        let gestor = setup();
        receive(&gestor, "G-1", 1000, 210, date!(2024 - 11 - 10));
        let s = ModelSettings {
            request_refund: refund,
            iban: Some("ES9121000418450200051332".into()),
            ..models::settings(&gestor).unwrap()
        };
        models::save_settings(&gestor, &s, now).unwrap();
        // Con el diseño de registro de 2024 (como el de la gestoría) y su tipo de declaración
        // (campo 6 de la página 01, posición 13 en todos los diseños del 303).
        let d = models::draft(&gestor, "303", 2024, 4).unwrap();
        let mut file =
            taxpain_models::dr_versions::file_with_boxes("303", 2024, "4T", &d.boxes).unwrap();
        let at = file.find("<T30301000>").unwrap() + 12;
        file.replace_range(at..at + 1, &d.declaration);

        let v = setup();
        let r = presentados::read_file(&file).unwrap();
        presentados::store(&v, "303_4T.txt", file.as_bytes(), &r, now).unwrap();
        let q1 = models::draft(&v, "303", 2025, 1).unwrap();
        assert_eq!(
            get(&q1, 110),
            carried,
            "devolución: {refund}; {:?}",
            q1.boxes
        );
    }
}

#[test]
fn las_nominas_del_diario_del_gestor_van_al_111() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    // Diario del gestor: nómina de enero de dos empleados (465 por empleado) y una factura de
    // un profesional con retención (sin 640: no es una nómina).
    let diario = "Fecha;Asiento;Cuenta;Concepto;Debe;Haber\n\
                  31/01/2025;1;6400000;Nóminas enero;2.000,00;\n\
                  31/01/2025;1;4760000;Nóminas enero;;130,00\n\
                  31/01/2025;1;4751000;Nóminas enero;;300,00\n\
                  31/01/2025;1;4650001;Nóminas enero;;1.000,00\n\
                  31/01/2025;1;4650002;Nóminas enero;;570,00\n\
                  15/02/2025;2;6230000;Asesoría;300,00;\n\
                  15/02/2025;2;4720000;Asesoría;63,00;\n\
                  15/02/2025;2;4751000;Asesoría;;45,00\n\
                  15/02/2025;2;4100001;Asesoría;;318,00\n";
    taxpain_app::import::import(&v, "diario.csv", diario.as_bytes(), now).unwrap();
    let m111 = models::draft(&v, "111", 2025, 1).unwrap();
    assert_eq!(get(&m111, 2), eur(2000), "{:?}", m111.boxes);
    assert_eq!(get(&m111, 3), eur(300));
    assert_eq!(
        m111.payees.iter().find(|(b, _)| *b == 1).map(|(_, n)| *n),
        Some(2)
    );
    assert!(
        m111.warnings.iter().any(|w| w.contains("diario importado")),
        "{:?}",
        m111.warnings
    );
    // En el 190 no pueden ir en la clave A sin los datos de cada empleado: aviso.
    let m190 = models::draft(&v, "190", 2025, 0).unwrap();
    assert!(
        m190.warnings.iter().any(|w| w.contains("diario importado")),
        "{:?}",
        m190.warnings
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
