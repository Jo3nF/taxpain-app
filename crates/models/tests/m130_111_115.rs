//! Modelos 130, 111 y 115: cálculo con casos sintéticos hechos a mano y ficheros según el DR.

use taxpain_core::Money;
use taxpain_models::Declarant;
use taxpain_models::dr::PageReader;
use taxpain_models::m130::{self, DeclarationType130, Input130, Previous130};
use taxpain_models::withholding::{self, DeclarationTypeW, Input111, Payment};

fn eur(e: i64, c: u8) -> Money {
    Money::from_euros(e, c)
}

fn persona() -> Declarant {
    Declarant {
        nif: "12345678Z".into(),
        name: "Española Española".into(),
        given_name: Some("Carmen".into()),
    }
}

#[test]
fn m130_primer_trimestre() {
    // Ingresos 12.000, gastos 4.000 → rendimiento 8.000; 20 % = 1.600; retenciones 900.
    let i = Input130 {
        income: eur(12_000, 0),
        expenses: eur(4_000, 0),
        withholdings: eur(900, 0),
        previous_year_net: Some(eur(8_500, 0)),
        ..Input130::default()
    };
    let r = m130::compute(2026, 1, &i);
    for (b, v) in [
        (3, eur(8_000, 0)),
        (4, eur(1_600, 0)),
        (5, Money::ZERO),
        (7, eur(700, 0)),
        (12, eur(700, 0)),
        (13, eur(100, 0)),
        (14, eur(600, 0)),
        (15, Money::ZERO),
        (17, eur(600, 0)),
        (19, eur(600, 0)),
    ] {
        assert_eq!(r.get(b), v, "[{b}]");
    }
    assert_eq!(r.declaration, DeclarationType130::Ingreso);
}

#[test]
fn m130_acumulado_con_trimestres_anteriores() {
    // 3T: acumulado ingresos 30.000, gastos 14.000 → 16.000; 20 % = 3.200.
    // Anteriores: 1T [07] 700; 2T [07] −200 (no computa) y [19] −200 pendiente de deducir.
    let i = Input130 {
        income: eur(30_000, 0),
        expenses: eur(14_000, 0),
        withholdings: eur(2_000, 0),
        previous: vec![
            Previous130 {
                box07: eur(700, 0),
                box19: eur(700, 0),
                ..Previous130::default()
            },
            Previous130 {
                box07: -eur(200, 0),
                box19: -eur(200, 0),
                ..Previous130::default()
            },
        ],
        ..Input130::default()
    };
    let r = m130::compute(2026, 3, &i);
    assert_eq!(r.get(5), eur(700, 0));
    assert_eq!(r.get(7), eur(500, 0)); // 3.200 − 700 − 2.000
    assert_eq!(r.get(15), eur(200, 0));
    assert_eq!(r.get(19), eur(300, 0));
}

#[test]
fn m130_rendimiento_negativo_y_minoraciones() {
    let i = Input130 {
        income: eur(1_000, 0),
        expenses: eur(3_000, 0),
        ..Input130::default()
    };
    let r = m130::compute(2026, 1, &i);
    assert_eq!(r.get(3), -eur(2_000, 0));
    assert_eq!(r.get(4), Money::ZERO);
    assert_eq!(r.get(19), Money::ZERO);
    assert_eq!(r.declaration, DeclarationType130::Negativa);

    assert_eq!(m130::reduction(eur(9_000, 0)), eur(100, 0));
    assert_eq!(m130::reduction(eur(9_000, 1)), eur(75, 0));
    assert_eq!(m130::reduction(eur(11_500, 0)), eur(25, 0));
    assert_eq!(m130::reduction(eur(12_000, 1)), Money::ZERO);
}

#[test]
fn m130_fichero() {
    let i = Input130 {
        income: eur(12_000, 0),
        expenses: eur(4_000, 0),
        withholdings: eur(900, 0),
        ..Input130::default()
    };
    let r = m130::compute(2026, 1, &i);
    let f = m130::file(&r, &persona(), None).unwrap_or_else(|e| panic!("{e}"));
    assert!(f.starts_with("<T130020261T0000><AUX>"));
    assert!(f.ends_with("</T130020261T0000>"));
    assert_eq!(f.chars().count(), 328 + 600 + 18);
    let rec: String = f.chars().skip(328).take(600).collect();
    let spec = m130::spec();
    let p = PageReader::new(spec.page("01").unwrap_or_else(|e| panic!("{e}")), &rec)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(p.raw(7).ok().as_deref(), Some("12345678Z"));
    assert!(p.raw(8).is_ok_and(|s| s.trim_end() == "ESPAÑOLA ESPAÑOLA"));
    assert!(p.raw(9).is_ok_and(|s| s.trim_end() == "CARMEN"));
    assert_eq!(p.raw(11).ok().as_deref(), Some("1T"));
    // [01] en la posición 109, 17 posiciones.
    assert_eq!(p.raw(12).ok().as_deref(), Some("00000000001200000"));
    for b in [1, 2, 3, 4, 6, 7, 12, 14, 17, 19] {
        assert_eq!(p.amount_box(b).ok().flatten(), Some(r.get(b)), "[{b}]");
    }
    assert!(rec.ends_with("</T13001000>"));
}

fn pay(payee: &str, base: i64, withheld: i64) -> Payment {
    Payment {
        payee: payee.into(),
        base: eur(base, 0),
        withheld: eur(withheld, 0),
    }
}

#[test]
fn m111_y_fichero() {
    let i = Input111 {
        work: vec![pay("11111111H", 6_000, 900), pay("11111111H", 6_000, 900)],
        professionals: vec![pay("22222222J", 1_000, 150), pay("33333333P", 2_000, 300)],
    };
    let r = withholding::compute_111(2026, 2, &i, false);
    assert_eq!(r.get(2), eur(12_000, 0));
    assert_eq!(r.get(3), eur(1_800, 0));
    assert_eq!(r.get(8), eur(3_000, 0));
    assert_eq!(r.get(9), eur(450, 0));
    assert_eq!(r.result(), eur(2_250, 0));
    assert_eq!(r.payees, vec![(1, 1), (7, 2)]);
    assert_eq!(r.declaration, DeclarationTypeW::Ingreso);

    let f = withholding::file(&r, &persona(), None).unwrap_or_else(|e| panic!("{e}"));
    assert!(f.starts_with("<T111020262T0000><AUX>"));
    let rec: String = f.chars().skip(328).take(1000).collect();
    let p = PageReader::new(
        withholding::spec_111()
            .page("01")
            .unwrap_or_else(|e| panic!("{e}")),
        &rec,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(p.raw(12).ok().as_deref(), Some("00000001")); // [01] nº perceptores
    assert_eq!(p.raw(18).ok().as_deref(), Some("00000002")); // [07]
    assert_eq!(p.amount(20).ok().flatten(), Some(eur(450, 0))); // [09]
    assert_eq!(p.amount(39).ok().flatten(), Some(eur(2_250, 0))); // [28]
    assert_eq!(p.amount(41).ok().flatten(), Some(eur(2_250, 0))); // [30]
}

#[test]
fn perceptores_con_importe_neto_cero_no_cuentan() {
    // Una factura de alquiler y su rectificativa que la anula: ese arrendador no cuenta.
    let rents = vec![
        pay("B12345674", 1_000, 190),
        pay("B98765431", 500, 95),
        pay("B98765431", -500, -95),
    ];
    let r = withholding::compute_115(2026, 3, &rents, false);
    assert_eq!(r.payees, vec![(1, 1)]);
    assert_eq!(r.get(3), eur(190, 0));
    let i = Input111 {
        work: vec![],
        professionals: vec![
            pay("22222222J", 1_000, 150),
            pay("33333333P", 200, 30),
            pay("33333333P", -200, -30),
        ],
    };
    let r = withholding::compute_111(2026, 3, &i, false);
    assert_eq!(r.payees, vec![(1, 0), (7, 1)]);
}

#[test]
fn m111_sin_retenciones_es_negativa() {
    let r = withholding::compute_111(2026, 1, &Input111::default(), true);
    assert_eq!(r.declaration, DeclarationTypeW::Negativa);
}

#[test]
fn m115_y_fichero_con_domiciliacion() {
    let rents = vec![pay("B12345674", 1_000, 190), pay("B12345674", 1_000, 190)];
    let r = withholding::compute_115(2026, 3, &rents, true);
    assert_eq!(r.get(2), eur(2_000, 0));
    assert_eq!(r.result(), eur(380, 0));
    assert_eq!(r.declaration, DeclarationTypeW::Domiciliacion);
    let f = withholding::file(&r, &persona(), Some("ES9121000418450200051332"))
        .unwrap_or_else(|e| panic!("{e}"));
    let rec: String = f.chars().skip(328).take(500).collect();
    let p = PageReader::new(
        withholding::spec_115()
            .page("01")
            .unwrap_or_else(|e| panic!("{e}")),
        &rec,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(p.raw(6).ok().as_deref(), Some("U"));
    assert_eq!(p.raw(12).ok().as_deref(), Some("000000000000001"));
    assert_eq!(p.amount(16).ok().flatten(), Some(eur(380, 0))); // [05]
    assert!(
        p.raw(19)
            .is_ok_and(|s| s.starts_with("ES9121000418450200051332"))
    );
    // Leído como un 115 del gestor: el resultado es la [05] (el DR describe el campo 16 como
    // «[03] - [04]» y no debe quedarse con una [03] repetida).
    let boxes = taxpain_models::dr::read_boxes(withholding::spec_115(), &f, |_| None);
    assert_eq!(boxes.get(&3), Some(&eur(380, 0)));
    assert_eq!(boxes.get(&5), Some(&eur(380, 0)));
}

#[test]
fn casillas_unicas_en_los_dr() {
    use std::collections::BTreeSet;
    // (El 390 y el 200 repiten casillas por diseño: bloques por actividad o por página.)
    for spec in [
        taxpain_models::m303::spec(),
        m130::spec(),
        withholding::spec_111(),
        withholding::spec_115(),
        taxpain_models::m123::spec(),
        taxpain_models::m202::spec(),
    ] {
        for p in &spec.pages {
            let mut seen = BTreeSet::new();
            for f in p.fields.iter().filter(|f| f.constant.is_none()) {
                if let Some(b) = f.box_ {
                    assert!(
                        seen.insert(b),
                        "{} {}: casilla [{b}] repetida",
                        spec.model,
                        p.sheet
                    );
                }
            }
        }
    }
}

mod m349 {
    use taxpain_core::Money;
    use taxpain_models::Declarant;
    use taxpain_models::dr::{PageReader, to_latin1};
    use taxpain_models::m349::{self, Key, Operation};

    fn op(country: &str, id: &str, name: &str, key: Key, euros: i64) -> Operation {
        Operation {
            country: country.into(),
            vat_id: id.into(),
            name: name.into(),
            key,
            base: Money::from_euros(euros, 0),
        }
    }

    #[test]
    fn acumula_por_operador_y_clave() {
        let ops = [
            op("DE", "DE123456789", "Kunde GmbH", Key::E, 1000),
            op("DE", "123456789", "Kunde GmbH", Key::E, 500),
            op("DE", "123456789", "Kunde GmbH", Key::S, 300),
            op("GR", "EL 094259216", "Pelatis AE", Key::S, 200),
            op("ES", "B12345674", "Cliente SL", Key::E, 99),
            op("FR", "FR12345678901", "Client SARL", Key::S, 100),
            op("FR", "FR12345678901", "Client SARL", Key::S, -150),
        ];
        let r = m349::compute(2026, "3T", &ops);
        let keys: Vec<(String, String, Key, Money)> = r
            .lines
            .iter()
            .map(|l| (l.country.clone(), l.vat_id.clone(), l.key, l.base))
            .collect();
        assert_eq!(
            keys,
            vec![
                (
                    "DE".into(),
                    "123456789".into(),
                    Key::E,
                    Money::from_euros(1500, 0)
                ),
                (
                    "DE".into(),
                    "123456789".into(),
                    Key::S,
                    Money::from_euros(300, 0)
                ),
                (
                    "EL".into(),
                    "094259216".into(),
                    Key::S,
                    Money::from_euros(200, 0)
                ),
            ]
        );
        assert_eq!(r.total, Money::from_euros(2000, 0));
        assert_eq!(r.warnings.len(), 2, "{:?}", r.warnings);
    }

    /// Sólo operadores de otros Estados miembros: un cliente de EE. UU. o de Suiza no va en el
    /// 349 (se avisa) e Irlanda del Norte (XI) sólo vale para bienes.
    #[test]
    fn fuera_de_la_ue_no_va_en_el_349() {
        let ops = [
            op("US", "123456789", "Acme Inc", Key::S, 2000),
            op("CH", "CHE123456789", "Kunde AG", Key::E, 700),
            op("XI", "123456789", "Belfast Ltd", Key::S, 100),
            op("XI", "123456789", "Belfast Ltd", Key::E, 400),
            op("IE", "1234567T", "Irish Ltd", Key::I, 50),
        ];
        let r = m349::compute(2026, "4T", &ops);
        let keys: Vec<(&str, Key)> = r
            .lines
            .iter()
            .map(|l| (l.country.as_str(), l.key))
            .collect();
        assert_eq!(keys, [("IE", Key::I), ("XI", Key::E)]);
        assert_eq!(r.total, Money::from_euros(450, 0));
        assert_eq!(r.warnings.len(), 3, "{:?}", r.warnings);
        assert!(r.warnings[0].contains("Acme Inc") && r.warnings[0].contains("«US»"));

        let mut r = m349::compute(2026, "4T", &[]);
        m349::add_rectifications(
            &mut r,
            &[m349::Rectification {
                country: "US".into(),
                vat_id: "123456789".into(),
                name: "Acme Inc".into(),
                key: Key::S,
                year: 2026,
                period: "3T".into(),
                delta: Money::from_euros(-100, 0),
                previous: Money::from_euros(2000, 0),
            }],
        );
        assert!(r.rectifications.is_empty());
        assert_eq!(r.warnings.len(), 1, "{:?}", r.warnings);
    }

    #[test]
    fn fichero_de_registros_de_500() {
        let r = m349::compute(
            2026,
            "3T",
            &[op("DE", "123456789", "Kunde GmbH", Key::E, 1500)],
        );
        let who = Declarant {
            nif: "B12345674".into(),
            name: "Estudio Peñón SL".into(),
            given_name: None,
        };
        let f = m349::file(&r, &who, 1).unwrap_or_else(|e| panic!("{e}"));
        let recs: Vec<&str> = f.split("\r\n").collect();
        assert_eq!(recs.len(), 2);
        for rec in &recs {
            assert_eq!(to_latin1(rec).unwrap_or_default().len(), 500);
        }
        let s = m349::spec();
        let d = PageReader::new(s.page("349-1").unwrap_or_else(|e| panic!("{e}")), recs[0])
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(recs[0].starts_with("13492026B12345674ESTUDIO PEÑON SL"));
        assert_eq!(d.raw(9).ok().as_deref(), Some("3492026030001"));
        assert_eq!(d.raw(13).ok().as_deref(), Some("3T"));
        assert_eq!(d.raw(14).ok().as_deref(), Some("000000001"));
        assert_eq!(
            d.amount(15).ok().flatten(),
            Some(Money::from_euros(1500, 0))
        );
        let o = PageReader::new(s.page("349-2op").unwrap_or_else(|e| panic!("{e}")), recs[1])
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(recs[1].starts_with("23492026B12345674"));
        assert_eq!(o.raw(6).ok().as_deref(), Some("DE"));
        assert!(o.raw(7).is_ok_and(|v| v.trim_end() == "123456789"));
        assert_eq!(o.raw(9).ok().as_deref(), Some("E"));
        assert_eq!(o.raw(10).ok().as_deref(), Some("0000000150000"));
    }
}

mod m202 {
    use taxpain_core::Money;
    use taxpain_models::Declarant;
    use taxpain_models::dr::PageReader;
    use taxpain_models::m202::{self, Input202};
    use time::macros::date;

    fn input(base: i64) -> Input202 {
        Input202 {
            base: Money::from_euros(base, 0),
            period_start: date!(2026 - 01 - 01),
            cnae: "6201".into(),
            tax_rate: "23".into(),
            turnover_over_6m: false,
            turnover_under_1m: true,
            iban: None,
        }
    }

    #[test]
    fn dieciocho_por_ciento_de_la_cuota() {
        let r = m202::compute(2026, 2, &input(10_000));
        assert_eq!(r.result(), Money::from_euros(1_800, 0));
        assert_eq!(r.declaration, "I");
        assert!(r.warnings.is_empty());
        let cero = m202::compute(2026, 1, &input(0));
        assert_eq!(cero.declaration, "N");
        assert_eq!(cero.warnings.len(), 1);
    }

    #[test]
    fn fichero_con_dos_paginas() {
        let i = input(10_000);
        let r = m202::compute(2026, 2, &i);
        let who = Declarant {
            nif: "B12345674".into(),
            name: "Estudio SL".into(),
            given_name: None,
        };
        let f = m202::file(&r, &who, &i).unwrap_or_else(|e| panic!("{e}"));
        assert!(f.starts_with("<T202020262P0000><AUX>"));
        assert!(f.ends_with("</T202020262P0000>"));
        assert_eq!(f.len(), 328 + 700 + 900 + 18);
        let s = m202::spec();
        let p = PageReader::new(
            s.page("(1)").unwrap_or_else(|e| panic!("{e}")),
            &f[328..1028],
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(p.raw(12).ok().as_deref(), Some("2P"));
        assert_eq!(p.raw(13).ok().as_deref(), Some("01012026"));
        assert_eq!(p.raw(27).ok().as_deref(), Some("A"));
        assert_eq!(
            p.amount_box(1).ok().flatten(),
            Some(Money::from_euros(10_000, 0))
        );
        assert_eq!(
            p.amount_box(3).ok().flatten(),
            Some(Money::from_euros(1_800, 0))
        );
        assert!(f[1028..].starts_with("<T20202000>"));
    }

    /// Con más de 6 millones la modalidad es la del art. 40.3 LIS (no implementada): avisa y no
    /// genera un fichero con la modalidad «A».
    #[test]
    fn mas_de_seis_millones_sin_fichero() {
        let mut i = input(10_000);
        i.turnover_over_6m = true;
        let r = m202::compute(2026, 2, &i);
        assert!(r.warnings.iter().any(|w| w.contains("40.3")));
        let who = Declarant {
            nif: "B12345674".into(),
            name: "Estudio SL".into(),
            given_name: None,
        };
        let err = m202::file(&r, &who, &i).unwrap_err();
        assert!(err.to_string().contains("40.3"), "{err}");
    }
}

#[test]
fn m130_negativo_es_b_salvo_en_el_4t() {
    let i = Input130 {
        // [04] = 80 y la minoración de [13] (rendimiento anterior ≤ 9.000 €) = 100 → [19] = −20.
        income: Money::from_euros(400, 0),
        previous_year_net: Some(Money::from_euros(5000, 0)),
        ..Input130::default()
    };
    let r3 = m130::compute(2026, 3, &i);
    assert!(r3.boxes.get(&19).is_some_and(|v| v.cents() < 0));
    assert_eq!(r3.declaration, DeclarationType130::ADeducir);
    assert_eq!(
        m130::compute(2026, 4, &i).declaration,
        DeclarationType130::Negativa
    );
}

#[test]
fn m130_simplificada_suma_el_cinco_por_ciento_en_02() {
    // RETENCIONES-02: ingresos 30.000, gastos 10.000 → 5 % de 20.000 = 1.000 en [02].
    let mut i = Input130 {
        income: eur(30_000, 0),
        expenses: eur(10_000, 0),
        simplified: true,
        ..Input130::default()
    };
    let r = m130::compute(2026, 3, &i);
    assert_eq!(r.get(2), eur(11_000, 0));
    assert_eq!(r.get(3), eur(19_000, 0));
    assert_eq!(r.get(4), eur(3_800, 0));
    assert!(
        r.warnings
            .iter()
            .any(|w| w.contains("difícil justificación"))
    );
    // Con un rendimiento de 100.000, el tope de 2.000 €.
    i.income = eur(110_000, 0);
    let r = m130::compute(2026, 3, &i);
    assert_eq!(r.get(2), eur(12_000, 0));
    // Normal: sin el 5 %.
    i.simplified = false;
    let r = m130::compute(2026, 3, &i);
    assert_eq!(r.get(2), eur(10_000, 0));
    // [02] fijada a mano: manda la cifra tecleada.
    i.simplified = true;
    i.adjustments.insert(2, eur(9_000, 0));
    let r = m130::compute(2026, 3, &i);
    assert_eq!(r.get(2), eur(9_000, 0));
}
