//! Modelo 202 por la base del periodo (art. 40.3 LIS): cálculo con cifras inventadas y fichero
//! de la modalidad «B» según el DR.

use taxpain_core::Money;
use taxpain_models::Declarant;
use taxpain_models::dr::PageReader;
use taxpain_models::m200::TaxRegime;
use taxpain_models::m202::{self, Input202, InputBase202, Modalidad};
use time::macros::date;

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn sociedad() -> Declarant {
    Declarant {
        nif: "B12345674".into(),
        name: "Estudio Demo SL".into(),
        given_name: None,
    }
}

fn base(result: i64) -> InputBase202 {
    InputBase202 {
        period_start: date!(2026 - 01 - 01),
        tax_year: 2026,
        cnae: "6201".into(),
        tax_rate: "25".into(),
        regime: TaxRegime::General,
        turnover: Some(eur(2_000_000)),
        turnover_over_6m: false,
        turnover_under_1m: false,
        accounting_result: eur(result),
        tax_expense: Money::ZERO,
        tax_income: Money::ZERO,
        other_increases: Money::ZERO,
        other_decreases: Money::ZERO,
        capitalization_remainder: Money::ZERO,
        pending_losses: Money::ZERO,
        losses_override: None,
        bonuses: Money::ZERO,
        withholdings: Money::ZERO,
        previous_payments: Money::ZERO,
        iban: None,
    }
}

fn get(r: &m202::Result202, b: u16) -> Money {
    r.boxes.get(&b).copied().unwrap_or(Money::ZERO)
}

#[test]
fn porcentajes_de_cinco_septimos_y_diecinueve_veinteavos() {
    use m202::base_percentages as p;
    // 5/7 × 25 = 17,86 → 17; 5/7 × 23 = 16,43 → 16; 5/7 × 24 = 17,14 → 17; 5/7 × 15 = 10,71 → 10.
    assert_eq!(p(TaxRegime::General, 2026, false), (17, None));
    assert_eq!(p(TaxRegime::ReducidaDimension, 2026, false), (16, None));
    assert_eq!(p(TaxRegime::ReducidaDimension, 2025, false), (17, None));
    assert_eq!(p(TaxRegime::NuevaCreacion, 2026, false), (10, None));
    // Microempresa 2026 (19 % y 21 %): 13 % y 15 %.
    assert_eq!(p(TaxRegime::Micro, 2026, false), (13, Some(15)));
    // 10 M€ o más: 19/20 × 25 = 23,75 → 24.
    assert_eq!(p(TaxRegime::General, 2026, true), (24, None));
}

#[test]
fn caso_general_con_bases_negativas_retenciones_y_pagos_anteriores() {
    // Nueve meses (2P): resultado 120.000, IS contabilizado 6.000, 4.000 de multas en [07],
    // 20.000 de bases negativas pendientes, 1.000 de retenciones y 2.000 pagados en el 1P.
    let i = InputBase202 {
        accounting_result: eur(114_000),
        tax_expense: eur(6_000),
        other_increases: eur(4_000),
        pending_losses: eur(20_000),
        withholdings: eur(1_000),
        previous_payments: eur(2_000),
        ..base(0)
    };
    let r = m202::compute_base(2026, 2, &i);
    assert_eq!(r.modalidad, Modalidad::Base);
    for (b, v) in [
        (4, eur(114_000)),
        (5, eur(6_000)),
        (7, eur(4_000)),
        (38, eur(10_000)),
        (13, eur(124_000)),
        (14, eur(20_000)),
        (16, eur(104_000)),
        (17, eur(17)),
        (18, eur(17_680)),
        (28, eur(1_000)),
        (29, eur(100)),
        (30, eur(2_000)),
        (32, eur(14_680)),
        (34, eur(14_680)),
    ] {
        assert_eq!(get(&r, b), v, "[{b}]");
    }
    assert_eq!(r.result(), eur(14_680));
    assert_eq!(r.declaration, "I");
    // Sin pago mínimo por debajo de 10 M€.
    assert!(!r.boxes.contains_key(&33));
}

#[test]
fn limite_de_bases_negativas() {
    // Base 2.000.000 y 3.000.000 pendientes: como mucho el 70 % (1.400.000, más que el millón).
    let i = InputBase202 {
        pending_losses: eur(3_000_000),
        ..base(2_000_000)
    };
    let r = m202::compute_base(2026, 3, &i);
    assert_eq!(get(&r, 14), eur(1_400_000));
    assert_eq!(get(&r, 16), eur(600_000));
    assert_eq!(get(&r, 18), eur(102_000));
    assert!(r.warnings.iter().any(|w| w.contains("art. 26.1")));
    // Con una base pequeña, el millón: se compensa todo lo que cabe.
    let i = InputBase202 {
        pending_losses: eur(3_000_000),
        ..base(50_000)
    };
    let r = m202::compute_base(2026, 1, &i);
    assert_eq!(get(&r, 14), eur(50_000));
    assert_eq!(r.result(), Money::ZERO);
    assert_eq!(r.declaration, "N");
}

#[test]
fn microempresa_con_dos_porcentajes() {
    // Base 80.000: 50.000 al 13 % (6.500) y 30.000 al 15 % (4.500).
    let i = InputBase202 {
        regime: TaxRegime::Micro,
        turnover: Some(eur(600_000)),
        turnover_under_1m: true,
        ..base(80_000)
    };
    let r = m202::compute_base(2026, 2, &i);
    for (b, v) in [
        (19, eur(80_000)),
        (20, eur(50_000)),
        (21, eur(13)),
        (22, eur(6_500)),
        (23, eur(30_000)),
        (24, eur(15)),
        (25, eur(4_500)),
        (26, eur(11_000)),
        (34, eur(11_000)),
    ] {
        assert_eq!(get(&r, b), v, "[{b}]");
    }
    assert!(!r.boxes.contains_key(&16));
}

#[test]
fn gran_empresa_con_pago_minimo() {
    // 12 M€ de cifra de negocios: resultado 100.000 con 60.000 de dividendos exentos en [08].
    // Base 40.000 al 24 % = 9.600; pago mínimo: 23 % de 100.000 = 23.000. Con 5.000 pagados
    // antes: [32] = 4.600 y [33] = 18.000, que es lo que se ingresa.
    let i = InputBase202 {
        turnover: Some(eur(12_000_000)),
        turnover_over_6m: true,
        other_decreases: eur(60_000),
        previous_payments: eur(5_000),
        ..base(100_000)
    };
    let r = m202::compute_base(2026, 2, &i);
    assert_eq!(get(&r, 13), eur(40_000));
    assert_eq!(get(&r, 17), eur(24));
    assert_eq!(get(&r, 18), eur(9_600));
    assert_eq!(get(&r, 32), eur(4_600));
    assert_eq!(get(&r, 33), eur(18_000));
    assert_eq!(r.result(), eur(18_000));
    assert!(r.warnings.iter().any(|w| w.contains("DA 14.ª")));
}

#[test]
fn retenciones_mayores_que_el_pago_sale_cero() {
    let i = InputBase202 {
        withholdings: eur(5_000),
        ..base(10_000)
    };
    let r = m202::compute_base(2026, 1, &i);
    assert_eq!(get(&r, 18), eur(1_700));
    assert_eq!(r.result(), Money::ZERO);
    assert_eq!(r.declaration, "N");
}

#[test]
fn perdidas_en_el_periodo() {
    let r = m202::compute_base(2026, 1, &base(-30_000));
    assert_eq!(get(&r, 13), eur(-30_000));
    assert_eq!(get(&r, 16), Money::ZERO);
    assert_eq!(r.result(), Money::ZERO);
}

#[test]
fn fichero_de_la_modalidad_b() {
    let i = InputBase202 {
        withholdings: eur(1_000),
        iban: Some("ES9121000418450200051332".into()),
        ..base(100_000)
    };
    let r = m202::compute_base(2026, 3, &i);
    assert_eq!(r.declaration, "U");
    let f = m202::file_base(&r, &sociedad(), &i).unwrap_or_else(|e| panic!("{e}"));
    assert!(f.starts_with("<T202020263P0000><AUX>"));
    let spec = m202::spec();
    let page = |marker: &str, suffix: &str| {
        let p = spec.page(suffix).unwrap_or_else(|e| panic!("{e}"));
        let at = f.find(marker).unwrap_or_else(|| panic!("falta {marker}"));
        let rec: String = f[at..].chars().take(p.len()).collect();
        assert!(rec.ends_with(&marker.replace("<T", "</T")), "{suffix}");
        (p, rec)
    };
    let (p1, rec1) = page("<T20201000>", "(1)");
    let r1 = PageReader::new(p1, &rec1).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(r1.raw(6).ok().as_deref(), Some("U"));
    assert_eq!(r1.raw(7).ok().as_deref(), Some("B12345674"));
    assert_eq!(r1.raw(12).ok().as_deref(), Some("3P"));
    assert_eq!(r1.raw(13).ok().as_deref(), Some("01012026"));
    assert_eq!(r1.raw(26).ok().as_deref(), Some("0"));
    assert_eq!(r1.raw(27).ok().as_deref(), Some("B"));
    assert_eq!(r1.amount_box(4).ok().flatten(), Some(eur(100_000)));
    assert_eq!(r1.amount_box(13).ok().flatten(), Some(eur(100_000)));
    // La modalidad A queda vacía.
    assert_eq!(r1.amount_box(3).ok().flatten(), Some(Money::ZERO));
    let (p2, rec2) = page("<T20202000>", "(2)");
    let r2 = PageReader::new(p2, &rec2).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(r2.amount_box(16).ok().flatten(), Some(eur(100_000)));
    // Porcentaje: 3 enteros y 2 decimales.
    assert_eq!(r2.raw(7).ok().as_deref(), Some("01700"));
    assert_eq!(r2.amount_box(18).ok().flatten(), Some(eur(17_000)));
    assert_eq!(r2.amount_box(28).ok().flatten(), Some(eur(1_000)));
    assert_eq!(r2.raw(33).ok().as_deref(), Some("10000"));
    assert_eq!(r2.amount_box(34).ok().flatten(), Some(eur(16_000)));
    assert_eq!(
        r2.raw(54).ok().as_deref().map(str::trim_end),
        Some("ES9121000418450200051332")
    );
}

#[test]
fn fichero_de_gran_empresa_marca_el_tramo() {
    let i = InputBase202 {
        turnover: Some(eur(25_000_000)),
        ..base(100_000)
    };
    let r = m202::compute_base(2026, 1, &i);
    let f = m202::file_base(&r, &sociedad(), &i).unwrap_or_else(|e| panic!("{e}"));
    let p1 = m202::spec().page("(1)").unwrap_or_else(|e| panic!("{e}"));
    let at = f.find("<T20201000>").unwrap_or_default();
    let rec: String = f[at..].chars().take(p1.len()).collect();
    let r1 = PageReader::new(p1, &rec).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(r1.raw(20).ok().as_deref(), Some("X"));
    assert_eq!(r1.raw(26).ok().as_deref(), Some("2"));
}

#[test]
fn la_modalidad_a_sigue_igual() {
    let i = Input202 {
        base: eur(10_000),
        period_start: date!(2026 - 01 - 01),
        cnae: "6201".into(),
        tax_rate: "25".into(),
        turnover_over_6m: false,
        turnover_under_1m: false,
        iban: None,
    };
    let r = m202::compute(2026, 2, &i);
    assert_eq!(r.modalidad, Modalidad::Cuota);
    assert_eq!(r.result(), eur(1_800));
    let f = m202::file(&r, &sociedad(), &i).unwrap_or_else(|e| panic!("{e}"));
    let p1 = m202::spec().page("(1)").unwrap_or_else(|e| panic!("{e}"));
    let at = f.find("<T20201000>").unwrap_or_default();
    let rec: String = f[at..].chars().take(p1.len()).collect();
    let r1 = PageReader::new(p1, &rec).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(r1.raw(27).ok().as_deref(), Some("A"));
    assert_eq!(r1.amount_box(3).ok().flatten(), Some(eur(1_800)));
    // El fichero de la base no se escribe con un cálculo por la cuota.
    assert!(m202::file_base(&r, &sociedad(), &base(0)).is_err());
}
