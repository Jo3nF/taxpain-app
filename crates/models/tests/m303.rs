//! Modelo 303: cálculo de un trimestre sintético (cuentas hechas a mano) y fichero según el DR.

use taxpain_core::invoice::{Exemption, NotSubject, TaxBreakdown, VatTreatment};
use taxpain_core::{Money, Rate};
use taxpain_models::Declarant;
use taxpain_models::dr::{PageReader, Spec, to_latin1};
use taxpain_models::m303::{
    self, DeclarationType, IssuedVat, Options303, ReceivedKind, ReceivedVat,
};
use time::macros::date;

fn eur(e: i64, c: u8) -> Money {
    Money::from_euros(e, c)
}

fn taxed(bp: i32, base: Money, vat: Money) -> TaxBreakdown {
    TaxBreakdown {
        vat: VatTreatment::Taxed {
            rate: Rate::from_bp(bp),
        },
        surcharge_rate: None,
        base,
        vat_amount: vat,
        surcharge_amount: Money::ZERO,
    }
}

fn other(vat: VatTreatment, base: Money) -> TaxBreakdown {
    TaxBreakdown {
        vat,
        surcharge_rate: None,
        base,
        vat_amount: Money::ZERO,
        surcharge_amount: Money::ZERO,
    }
}

fn issued(number: &str, rectification: bool, breakdown: Vec<TaxBreakdown>) -> IssuedVat {
    IssuedVat {
        number: number.into(),
        date: date!(2026 - 08 - 01),
        rectification,
        breakdown,
        cash_basis: false,
        eu_business: false,
        oss: false,
    }
}

fn received(kind: ReceivedKind, rect: bool, base: Money, vat: Money, pct: u8) -> ReceivedVat {
    ReceivedVat {
        number: "R".into(),
        date: date!(2026 - 08 - 02),
        kind,
        rectification: rect,
        base,
        vat,
        deductible_pct: pct,
        self_assessed: None,
        import: false,
        prorata_use: Default::default(),
        outside_prorata: false,
    }
}

fn trimestre() -> (Vec<IssuedVat>, Vec<ReceivedVat>) {
    let mut re = taxed(1000, eur(500, 0), eur(50, 0));
    re.surcharge_rate = Some(Rate::from_bp(140));
    re.surcharge_amount = eur(7, 0);
    let issued = vec![
        issued("A1", false, vec![taxed(2100, eur(1800, 0), eur(378, 0))]),
        issued("A2", false, vec![re]),
        issued(
            "A3",
            false,
            vec![other(VatTreatment::Exempt(Exemption::E5), eur(1000, 0))],
        ),
        issued(
            "A4",
            false,
            vec![other(
                VatTreatment::NotSubject(NotSubject::N2),
                eur(2000, 0),
            )],
        ),
        issued("R1", true, vec![taxed(2100, -eur(100, 0), -eur(21, 0))]),
    ];
    let received = vec![
        received(
            ReceivedKind::Corriente,
            false,
            eur(250, 0),
            eur(52, 50),
            100,
        ),
        received(
            ReceivedKind::BienInversion,
            false,
            eur(1200, 0),
            eur(252, 0),
            100,
        ),
        // Vehículo afecto al 50 %: se declara la mitad de base y cuota.
        received(ReceivedKind::Corriente, false, eur(400, 0), eur(84, 0), 50),
        received(
            ReceivedKind::Corriente,
            true,
            -eur(50, 0),
            -eur(10, 50),
            100,
        ),
        // IVA no deducible: no aparece.
        received(ReceivedKind::Corriente, false, eur(30, 0), eur(6, 30), 0),
    ];
    (issued, received)
}

fn opts() -> Options303 {
    Options303 {
        pending_compensation: eur(30, 0),
        ..Options303::default()
    }
}

#[test]
fn calculo_casilla_a_casilla() {
    let (i, r) = trimestre();
    let res = m303::compute(2026, 3, &i, &r, &opts());
    let expect = [
        (7, eur(1800, 0)),
        (9, eur(378, 0)),
        (4, eur(500, 0)),
        (6, eur(50, 0)),
        (19, eur(500, 0)),
        (21, eur(7, 0)),
        (14, -eur(100, 0)),
        (15, -eur(21, 0)),
        (27, eur(414, 0)),
        (28, eur(450, 0)),
        (29, eur(94, 50)),
        (30, eur(1200, 0)),
        (31, eur(252, 0)),
        (40, -eur(50, 0)),
        (41, -eur(10, 50)),
        (45, eur(336, 0)),
        (46, eur(78, 0)),
        (59, eur(1000, 0)),
        (120, eur(2000, 0)),
        (64, eur(78, 0)),
        (65, Money::from_cents(10_000)),
        (66, eur(78, 0)),
        (110, eur(30, 0)),
        (78, eur(30, 0)),
        (69, eur(48, 0)),
        (71, eur(48, 0)),
    ];
    for (b, v) in expect {
        assert_eq!(res.get(b), v, "casilla [{b}]");
    }
    assert_eq!(res.boxes.len(), expect.len(), "{:?}", res.boxes);
    assert_eq!(res.declaration, DeclarationType::Ingreso);
    assert!(res.warnings.is_empty(), "{:?}", res.warnings);
}

#[test]
fn resultado_negativo_compensa_o_devuelve_en_el_4t() {
    let r = vec![received(
        ReceivedKind::Corriente,
        false,
        eur(1000, 0),
        eur(210, 0),
        100,
    )];
    let o = Options303 {
        pending_compensation: eur(50, 0),
        ..Options303::default()
    };
    let t3 = m303::compute(2026, 3, &[], &r, &o);
    assert_eq!(t3.result(), -eur(210, 0));
    assert_eq!(t3.declaration, DeclarationType::Compensacion);
    // Con resultado negativo no se aplica la compensación pendiente.
    assert_eq!(t3.get(78), Money::ZERO);
    assert_eq!(t3.get(87), eur(50, 0));

    let o4 = Options303 {
        request_refund: true,
        iban: Some("ES9121000418450200051332".into()),
        ..o
    };
    let t4 = m303::compute(2026, 4, &[], &r, &o4);
    assert_eq!(t4.declaration, DeclarationType::Devolucion);
    assert!(t4.warnings.is_empty());
    // Se devuelve todo el saldo a favor: lo del 4T y lo pendiente de periodos anteriores.
    assert_eq!(t4.get(78), eur(50, 0));
    assert_eq!(t4.get(87), Money::ZERO);
    assert_eq!(t4.result(), -eur(260, 0));

    // 4T positivo pero menor que lo pendiente: también se devuelve la diferencia.
    let i = vec![issued(
        "01",
        false,
        vec![taxed(2100, eur(100, 0), eur(21, 0))],
    )];
    let o5 = Options303 {
        pending_compensation: eur(100, 0),
        ..o4.clone()
    };
    let t5 = m303::compute(2026, 4, &i, &[], &o5);
    assert_eq!(t5.get(78), eur(100, 0));
    assert_eq!(t5.result(), -eur(79, 0));
    assert_eq!(t5.declaration, DeclarationType::Devolucion);
    // Sin pedir la devolución, en el 4T se compensa hasta dejarlo a cero.
    let t6 = m303::compute(
        2026,
        4,
        &i,
        &[],
        &Options303 {
            request_refund: false,
            ..o5
        },
    );
    assert_eq!(t6.get(78), eur(21, 0));
    assert_eq!(t6.get(87), eur(79, 0));
}

#[test]
fn sin_actividad() {
    let res = m303::compute(2026, 1, &[], &[], &Options303::default());
    assert_eq!(res.declaration, DeclarationType::SinActividad);
    assert!(res.no_activity);
    assert_eq!(res.result(), Money::ZERO);
}

/// Servicios no sujetos (N2): a un empresario de la UE con NIF-IVA en [59] (se cruza con la
/// clave S del 349); a un cliente de fuera de la UE en [120]. Las entregas intracomunitarias
/// exentas (E5) siguen en [59].
#[test]
fn servicios_a_la_ue_en_59_y_fuera_en_120() {
    let mut ue = issued(
        "UE",
        false,
        vec![other(VatTreatment::NotSubject(NotSubject::N2), eur(300, 0))],
    );
    ue.eu_business = true;
    let fuera = issued(
        "US",
        false,
        vec![other(VatTreatment::NotSubject(NotSubject::N2), eur(200, 0))],
    );
    let e5 = issued(
        "E5",
        false,
        vec![other(VatTreatment::Exempt(Exemption::E5), eur(1000, 0))],
    );
    let res = m303::compute(2026, 3, &[ue, fuera, e5], &[], &Options303::default());
    assert_eq!(res.get(59), eur(1300, 0));
    assert_eq!(res.get(120), eur(200, 0));
}

#[test]
fn tipo_desconocido_avisa() {
    let i = vec![issued("X", false, vec![taxed(500, eur(100, 0), eur(5, 0))])];
    let res = m303::compute(2026, 1, &i, &[], &Options303::default());
    assert_eq!(res.warnings.len(), 1);
    assert_eq!(res.get(27), Money::ZERO);
    // La cuota no está en [27]: se marca para que el borrador se bloquee.
    assert_eq!(res.unmapped, ["X: IVA del 5 % (5,00 €)"]);
}

#[test]
fn tipos_sin_casilla_que_pierden_cuota() {
    let mut re = taxed(2100, eur(100, 0), eur(21, 0));
    re.surcharge_rate = Some(Rate::from_bp(400));
    re.surcharge_amount = eur(4, 0);
    let i = vec![
        // Recargo con un tipo inexistente: el IVA va a [07]/[09], el recargo se perdería.
        issued("A", false, vec![re]),
        // Un 0 % sin cuota no pierde IVA; una rectificativa va a [14]/[15] con cualquier tipo.
        issued("B", false, vec![taxed(0, eur(100, 0), Money::ZERO)]),
        issued("C", true, vec![taxed(2000, -eur(100, 0), -eur(20, 0))]),
    ];
    assert_eq!(
        m303::unmapped_rates(&i),
        ["A: recargo de equivalencia del 4 % (4,00 €)"]
    );
}

/// Registro de la página `suffix` dentro del fichero (en caracteres, no en bytes).
fn page(file: &str, spec: &Spec, suffix: &str) -> String {
    let tag = match suffix {
        "DID" => "<T303DID00>".to_string(),
        s => format!("<T303{s}000>"),
    };
    let start = file
        .find(&tag)
        .unwrap_or_else(|| panic!("falta la página {suffix}"));
    let len = spec.page(suffix).map(|p| p.len()).unwrap_or(0);
    file[start..].chars().take(len).collect()
}

#[test]
fn fichero_segun_el_dr() {
    let (i, r) = trimestre();
    let o = opts();
    let res = m303::compute(2026, 3, &i, &r, &o);
    let who = Declarant {
        nif: "B12345674".into(),
        name: "Estudio Peñón, S.L.".into(),
        given_name: None,
    };
    let f = m303::file(&res, &who, &o).unwrap_or_else(|e| panic!("{e}"));
    let spec = m303::spec();

    assert!(f.starts_with("<T303020263T0000><AUX>"));
    assert!(f.ends_with("</T303020263T0000>"));
    assert_eq!(f.chars().count(), 328 + 1581 + 1017 + 18);
    assert!(!f.contains("<T303DID00>"), "sin IBAN no hay página DID");
    to_latin1(&f).unwrap_or_else(|e| panic!("{e}"));

    let p1 = PageReader::new(
        spec.page("01").unwrap_or_else(|e| panic!("{e}")),
        &page(&f, spec, "01"),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(p1.raw(6).ok().as_deref(), Some("I"));
    assert_eq!(p1.raw(7).ok().as_deref(), Some("B12345674"));
    assert!(p1.raw(8).is_ok_and(|s| s.trim_end() == "ESTUDIO PEÑON S L"));
    assert_eq!(p1.raw(9).ok().as_deref(), Some("2026"));
    assert_eq!(p1.raw(10).ok().as_deref(), Some("3T"));
    assert_eq!(p1.raw(13).ok().as_deref(), Some("3"));
    assert_eq!(p1.raw(17).ok().as_deref(), Some(" "));
    // Posiciones del DR: [07] base 21 % en 326, [08] tipo constante, [09] cuota en 348.
    // Se comprueba sobre los bytes ISO-8859-1 del fichero: la «Ñ» ocupa una posición.
    let line = page(&f, spec, "01");
    let bytes = to_latin1(&line).unwrap_or_default();
    assert_eq!(bytes.len(), 1581);
    assert_eq!(&bytes[325..342], b"00000000000180000");
    assert_eq!(&bytes[342..347], b"02100");
    assert_eq!(&bytes[347..364], b"00000000000037800");
    // [15] negativa con «N» en la primera posición.
    assert_eq!(p1.amount_box(15).ok().flatten(), Some(-eur(21, 0)));
    assert_eq!(bytes[449], b'N');
    for b in [7, 9, 27, 29, 31, 41, 45, 46] {
        assert_eq!(p1.amount_box(b).ok().flatten(), Some(res.get(b)), "[{b}]");
    }
    assert!(line.ends_with("</T30301000>"));

    let p3 = PageReader::new(
        spec.page("03").unwrap_or_else(|e| panic!("{e}")),
        &page(&f, spec, "03"),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    for b in [59, 120, 64, 65, 66, 110, 78, 87, 69, 71] {
        assert_eq!(p3.amount_box(b).ok().flatten(), Some(res.get(b)), "[{b}]");
    }
    assert_eq!(p3.raw(30).ok().as_deref(), Some(" "));
}

#[test]
fn exonerado_del_390_lleva_la_pagina_04_en_el_4t_con_el_volumen_del_390() {
    use taxpain_models::m303::{Exonerado390, Volumen390};
    use taxpain_models::m390;
    let (i, r) = trimestre();
    let o = Options303 {
        exempt_from_390: true,
        ..Options303::default()
    };
    // Todo el año en el 4T.
    let quarters: Vec<_> = (1..=4)
        .map(|q| {
            if q == 4 {
                m303::compute(2026, q, &i, &r, &o)
            } else {
                m303::compute(2026, q, &[], &[], &o)
            }
        })
        .collect();
    let anual = m390::compute(2026, &i, &r, &quarters, Money::ZERO);
    let who = Declarant {
        nif: "B12345674".into(),
        name: "Estudio SL".into(),
        given_name: None,
    };
    let spec = m303::spec();

    // Fuera del último periodo no hay página 04 aunque esté exonerado.
    let f3 = m303::file(&quarters[2], &who, &o).unwrap_or_else(|e| panic!("{e}"));
    assert!(!f3.contains("<T30304000>"));

    // 4T exonerado sin los datos: la página sale, a ceros.
    let f4 = m303::file(&quarters[3], &who, &o).unwrap_or_else(|e| panic!("{e}"));
    assert!(f4.contains("<T30304000>"));
    let p1 = PageReader::new(
        spec.page("01").unwrap_or_else(|e| panic!("{e}")),
        &page(&f4, spec, "01"),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(p1.raw(23).ok().as_deref(), Some("1"), "exonerado");
    assert_eq!(p1.raw(24).ok().as_deref(), Some("1"), "con volumen");

    let o = Options303 {
        exonerado_390: Some(Exonerado390 {
            activity_code: "A03".into(),
            iae: "8311".into(),
            files_347: true,
            volume: Volumen390::of(&anual),
        }),
        ..o
    };
    let f = m303::file(&quarters[3], &who, &o).unwrap_or_else(|e| panic!("{e}"));
    assert!(f.starts_with("<T303020264T0000><AUX>"));
    let p4 = PageReader::new(
        spec.page("04").unwrap_or_else(|e| panic!("{e}")),
        &page(&f, spec, "04"),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(p4.raw(6).ok().as_deref(), Some("A03"));
    assert_eq!(p4.raw(7).ok().as_deref(), Some("8311"));
    assert_eq!(p4.raw(18).ok().as_deref(), Some("X"));
    // Volumen: 1.800 + 500 − 100 al régimen general, 1.000 intracomunitarias, 2.000 no sujetas.
    assert_eq!(p4.amount_box(80).ok().flatten(), Some(eur(2200, 0)));
    assert_eq!(p4.amount_box(93).ok().flatten(), Some(eur(1000, 0)));
    assert_eq!(p4.amount_box(84).ok().flatten(), Some(eur(2000, 0)));
    for b in [81, 94, 83, 125, 126, 127, 128, 86, 95, 96, 97, 98, 79, 99] {
        assert_eq!(p4.amount_box(b).ok().flatten(), Some(Money::ZERO), "[{b}]");
    }
    // [88] = [108] del 390 del mismo escenario.
    assert_eq!(p4.amount_box(88).ok().flatten(), Some(anual.get(108)));
    assert_eq!(anual.get(108), eur(5200, 0));
    assert!(page(&f, spec, "04").ends_with("</T30304000>"));
    // La página va entre la 03 y la 05/DID.
    let (i3, i4) = (
        f.find("<T30303000>").unwrap_or(0),
        f.find("<T30304000>").unwrap_or(0),
    );
    assert!(i3 < i4);
    to_latin1(&f).unwrap_or_else(|e| panic!("{e}"));
}

/// 4T sin facturas de un exonerado del 390 que sí operó en el año: el campo 24 (volumen anual
/// distinto de cero, art. 121 LIVA) es «1»; sólo con volumen anual cero es «2».
#[test]
fn exonerado_sin_facturas_en_el_4t_con_volumen_en_el_ano() {
    use taxpain_models::m303::{Exonerado390, Volumen390};
    let o = Options303 {
        exempt_from_390: true,
        exonerado_390: Some(Exonerado390 {
            activity_code: "A03".into(),
            iae: "8311".into(),
            files_347: false,
            volume: Volumen390 {
                general: eur(40_000, 0),
                ..Volumen390::default()
            },
        }),
        ..Options303::default()
    };
    let q4 = m303::compute(2026, 4, &[], &[], &o);
    assert!(q4.no_activity);
    let who = Declarant {
        nif: "B12345674".into(),
        name: "Estudio SL".into(),
        given_name: None,
    };
    let spec = m303::spec();
    let field24 = |o: &Options303| {
        let f = m303::file(&q4, &who, o).unwrap_or_else(|e| panic!("{e}"));
        let p1 = PageReader::new(
            spec.page("01").unwrap_or_else(|e| panic!("{e}")),
            &page(&f, spec, "01"),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        p1.raw(24).unwrap_or_else(|e| panic!("{e}"))
    };
    assert_eq!(field24(&o), "1");
    let cero = Options303 {
        exonerado_390: Some(Exonerado390::default()),
        ..o
    };
    assert_eq!(field24(&cero), "2");
}

#[test]
fn domiciliacion_con_iban_en_la_pagina_did() {
    let (i, r) = trimestre();
    // Con IBAN pero sin marcar «domiciliar», no se carga la cuenta.
    let sin = Options303 {
        iban: Some("ES9121000418450200051332".into()),
        ..opts()
    };
    assert_eq!(
        m303::compute(2026, 4, &i, &r, &sin).declaration,
        DeclarationType::Ingreso
    );
    let o = Options303 {
        iban: Some("ES9121000418450200051332".into()),
        domiciliar: true,
        ..opts()
    };
    let res = m303::compute(2026, 3, &i, &r, &o);
    assert_eq!(res.declaration, DeclarationType::Domiciliacion);
    let who = Declarant {
        nif: "B12345674".into(),
        name: "X".into(),
        given_name: None,
    };
    let f = m303::file(&res, &who, &o).unwrap_or_else(|e| panic!("{e}"));
    let spec = m303::spec();
    let did = PageReader::new(
        spec.page("DID").unwrap_or_else(|e| panic!("{e}")),
        &page(&f, spec, "DID"),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        did.raw(6)
            .is_ok_and(|s| s.trim_end() == "ES9121000418450200051332")
    );
    assert_eq!(f.chars().count(), 328 + 1581 + 1017 + 823 + 18);
}

#[test]
fn el_dr_embebido_es_el_oficial() {
    let spec = m303::spec();
    assert_eq!(spec.source, "DR303e26v101.xlsx");
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/aeat/dr/DR303e26v101.xlsx"
    ))
    .unwrap_or_default();
    assert!(!bytes.is_empty());
    // Totales de posiciones que indica cada hoja del DR.
    for (p, total) in [("01", 1581), ("03", 1017), ("DID", 823)] {
        assert_eq!(
            spec.page(p).map(|s| s.len()).ok(),
            Some(total),
            "página {p}"
        );
    }
}

mod m390 {
    use super::*;
    use taxpain_models::m390::{self, Options390};

    #[test]
    fn resumen_anual_cuadra_con_los_303() {
        let (i, r) = trimestre();
        // Todo el trimestre en el 3T; los demás trimestres sin operaciones.
        let o = Options303::default();
        let quarters: Vec<_> = (1..=4)
            .map(|q| {
                if q == 3 {
                    m303::compute(2026, q, &i, &r, &o)
                } else {
                    m303::compute(2026, q, &[], &[], &o)
                }
            })
            .collect();
        let res = m390::compute(2026, &i, &r, &quarters, Money::ZERO);
        assert!(res.warnings.is_empty(), "{:?}", res.warnings);
        // Devengado: 21 % 1.800/378; 10 % 500/50; rectificativa −100/−21; RE 1,4 % 500/7.
        assert_eq!(res.get(5), eur(1800, 0));
        assert_eq!(res.get(6), eur(378, 0));
        assert_eq!(res.get(3), eur(500, 0));
        assert_eq!(res.get(29), -eur(100, 0));
        assert_eq!(res.get(34), eur(407, 0));
        assert_eq!(res.get(600), eur(7, 0));
        assert_eq!(res.get(47), eur(414, 0));
        // Deducible: corrientes al 21 % (250 + 200 del vehículo); inversión 1.200 al 21 %.
        assert_eq!(res.get(605), eur(450, 0));
        assert_eq!(res.get(606), eur(94, 50));
        assert_eq!(res.get(613), eur(1200, 0));
        assert_eq!(res.get(62), -eur(10, 50));
        assert_eq!(res.get(64), eur(336, 0));
        assert_eq!(res.get(65), quarters[2].get(46));
        assert_eq!(res.get(95), eur(78, 0));
        // Volumen: 1.800 + 500 − 100 + 1.000 (intracomunitarias) + 2.000 (no sujetas).
        assert_eq!(res.get(108), eur(5200, 0));

        let who = Declarant {
            nif: "B12345674".into(),
            name: "Estudio SL".into(),
            given_name: None,
        };
        let f = m390::file(&res, &who, &Options390::default()).unwrap_or_else(|e| panic!("{e}"));
        assert!(f.starts_with("<T390020260A0000><AUX>"));
        assert!(f.ends_with("</T390020260A0000>"));
        let s = m390::spec();
        let expected: usize = [
            "Pág. 1",
            "Pág. 2",
            "Pág. 2 bis",
            "Pág. 3",
            "Pág. 4",
            "Pág. 6",
        ]
        .iter()
        .map(|n| {
            s.pages
                .iter()
                .find(|p| p.sheet == *n)
                .map_or(0, |p| p.len())
        })
        .sum();
        assert_eq!(f.chars().count(), 328 + expected + 18);
        for tag in [
            "<T39001000>",
            "<T39002000>",
            "<T39003000>",
            "<T39004000>",
            "<T39006000>",
        ] {
            assert!(f.contains(tag), "{tag}");
        }
    }

    #[test]
    fn servicios_a_empresarios_de_la_ue_en_103_y_el_resto_en_110() {
        let mut ue = issued(
            "UE",
            false,
            vec![other(VatTreatment::NotSubject(NotSubject::N2), eur(300, 0))],
        );
        ue.eu_business = true;
        let fuera = issued(
            "US",
            false,
            vec![other(VatTreatment::NotSubject(NotSubject::N2), eur(200, 0))],
        );
        let i = vec![ue, fuera];
        let o = Options303::default();
        let quarters: Vec<_> = (1..=4)
            .map(|q| {
                if q == 3 {
                    m303::compute(2026, q, &i, &[], &o)
                } else {
                    m303::compute(2026, q, &[], &[], &o)
                }
            })
            .collect();
        let res = m390::compute(2026, &i, &[], &quarters, Money::ZERO);
        assert!(res.warnings.is_empty(), "{:?}", res.warnings);
        // Como la [59] del 303 y la clave S del 349.
        assert_eq!(quarters[2].get(59), eur(300, 0));
        assert_eq!(res.get(103), eur(300, 0));
        assert_eq!(res.get(110), eur(200, 0));
        assert_eq!(res.get(108), eur(500, 0));
    }

    #[test]
    fn cuotas_a_compensar_generadas_en_el_ejercicio_en_662() {
        // 1T −210 (a compensar); 2T +105 y 4T +21, que compensan parte; 3T sin actividad.
        // Además, 50 € arrastrados del año anterior, que se aplican primero ([85]).
        let r = vec![received(
            ReceivedKind::Corriente,
            false,
            eur(1000, 0),
            eur(210, 0),
            100,
        )];
        let i2 = vec![issued(
            "02",
            false,
            vec![taxed(2100, eur(500, 0), eur(105, 0))],
        )];
        let i4 = vec![issued(
            "04",
            false,
            vec![taxed(2100, eur(100, 0), eur(21, 0))],
        )];
        let pending = |p: Money| Options303 {
            pending_compensation: p,
            ..Options303::default()
        };
        let t1 = m303::compute(2026, 1, &[], &r, &pending(eur(50, 0)));
        assert_eq!(t1.declaration, DeclarationType::Compensacion);
        assert_eq!(t1.get(87), eur(50, 0));
        let t2 = m303::compute(2026, 2, &i2, &[], &pending(eur(260, 0)));
        assert_eq!(t2.get(78), eur(105, 0));
        assert_eq!(t2.get(87), eur(155, 0));
        let t3 = m303::compute(2026, 3, &[], &[], &pending(eur(155, 0)));
        let t4 = m303::compute(2026, 4, &i4, &[], &pending(eur(155, 0)));
        assert_eq!(t4.get(78), eur(21, 0));
        assert_eq!(t4.get(87), eur(134, 0));
        assert_eq!(t4.result(), Money::ZERO);

        let all: Vec<_> = i2.iter().chain(&i4).cloned().collect();
        let res = m390::compute(
            2026,
            &all,
            &r,
            &[t1.clone(), t2.clone(), t3.clone(), t4],
            eur(50, 0),
        );
        assert!(res.warnings.is_empty(), "{:?}", res.warnings);
        assert_eq!(res.get(65), -eur(84, 0));
        assert_eq!(res.get(85), eur(50, 0));
        assert_eq!(res.get(86), -eur(134, 0));
        assert_eq!(res.get(95), Money::ZERO);
        assert_eq!(res.get(97), Money::ZERO);
        assert_eq!(res.get(98), Money::ZERO);
        // De los 210 generados en el 1T se han aplicado 76 (126 menos los 50 del arrastre).
        assert_eq!(res.get(662), eur(134, 0));
        // Cuadre del apartado 9: [86] = [95] − [97] − [98] − [662].
        assert_eq!(
            res.get(86),
            res.get(95) - res.get(97) - res.get(98) - res.get(662)
        );

        // Si el 4T pide la devolución de todo, no queda nada pendiente: [662] = 0.
        let t4d = m303::compute(
            2026,
            4,
            &i4,
            &[],
            &Options303 {
                request_refund: true,
                iban: Some("ES9121000418450200051332".into()),
                ..pending(eur(155, 0))
            },
        );
        let res = m390::compute(2026, &all, &r, &[t1, t2, t3, t4d], eur(50, 0));
        assert_eq!(res.get(98), eur(134, 0));
        assert_eq!(res.get(662), Money::ZERO);
    }
}

#[test]
fn recargo_del_medio_por_ciento_lleva_su_tipo() {
    let mut t = taxed(500, eur(250, 0), eur(12, 50));
    t.surcharge_rate = Some(Rate::from_bp(50));
    t.surcharge_amount = eur(1, 25);
    let res = m303::compute(2026, 3, &[issued("01", false, vec![t])], &[], &opts());
    assert_eq!(res.get(168), eur(250, 0));
    assert_eq!(res.get(170), eur(1, 25));
    let who = Declarant {
        nif: "B12345674".into(),
        name: "X".into(),
        given_name: None,
    };
    let f = m303::file(&res, &who, &opts()).unwrap_or_else(|e| panic!("{e}"));
    let spec = m303::spec();
    let p1 = PageReader::new(
        spec.page("01").unwrap_or_else(|e| panic!("{e}")),
        &page(&f, spec, "01"),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(p1.raw(54).ok().as_deref(), Some("00050"));
}

#[test]
fn recibidas_sin_iva_no_van_a_la_28_y_avisan() {
    let r = vec![received(
        ReceivedKind::Corriente,
        false,
        eur(300, 0),
        Money::ZERO,
        100,
    )];
    let res = m303::compute(2026, 3, &[], &r, &Options303::default());
    assert_eq!(res.get(28), Money::ZERO);
    assert_eq!(res.warnings.len(), 1, "{:?}", res.warnings);
    assert!(res.warnings[0].contains("autorrepercutir"));
}

#[test]
fn autorrepercusion_intracomunitaria_y_otras_isp() {
    use taxpain_models::m303::{SelfAssessed, SelfAssessedKind};
    let mut ue = received(
        ReceivedKind::Corriente,
        false,
        eur(100, 0),
        Money::ZERO,
        100,
    );
    ue.self_assessed = Some(SelfAssessed {
        kind: SelfAssessedKind::Intracomunitaria,
        quota: eur(21, 0),
        goods: false,
        part: Default::default(),
    });
    let mut fuera = received(ReceivedKind::Corriente, false, eur(200, 0), Money::ZERO, 50);
    fuera.self_assessed = Some(SelfAssessed {
        kind: SelfAssessedKind::Otra,
        quota: eur(42, 0),
        goods: false,
        part: Default::default(),
    });
    let res = m303::compute(2026, 3, &[], &[ue, fuera], &Options303::default());
    assert_eq!((res.get(10), res.get(11)), (eur(100, 0), eur(21, 0)));
    assert_eq!((res.get(12), res.get(13)), (eur(200, 0), eur(42, 0)));
    assert_eq!((res.get(36), res.get(37)), (eur(100, 0), eur(21, 0)));
    // Deducible al 50 %.
    assert_eq!((res.get(28), res.get(29)), (eur(100, 0), eur(21, 0)));
    // Devengado 63, deducible 42: resultado 21 (la parte no deducible).
    assert_eq!(res.result(), eur(21, 0));
    assert!(res.warnings.is_empty(), "{:?}", res.warnings);
}

#[test]
fn abono_de_un_proveedor_de_la_ue_va_a_rectificaciones() {
    use taxpain_models::m303::{SelfAssessed, SelfAssessedKind};
    let mut abono = received(
        ReceivedKind::Corriente,
        true,
        -eur(100, 0),
        Money::ZERO,
        100,
    );
    abono.self_assessed = Some(SelfAssessed {
        kind: SelfAssessedKind::Intracomunitaria,
        quota: -eur(21, 0),
        goods: false,
        part: Default::default(),
    });
    let res = m303::compute(2026, 3, &[], &[abono], &Options303::default());
    assert_eq!((res.get(14), res.get(15)), (-eur(100, 0), -eur(21, 0)));
    assert_eq!((res.get(40), res.get(41)), (-eur(100, 0), -eur(21, 0)));
    assert_eq!(res.get(10), Money::ZERO);
    assert_eq!(res.result(), Money::ZERO);
}

/// Ventanilla única (OSS): la venta a un consumidor de otro Estado miembro que tributa allí va
/// en la casilla informativa [123], no en [120] ni en el IVA devengado.
#[test]
fn ventas_oss_en_123() {
    let mut oss = issued(
        "OSS",
        false,
        vec![other(VatTreatment::NotSubject(NotSubject::N2), eur(500, 0))],
    );
    oss.oss = true;
    let fuera = issued(
        "US",
        false,
        vec![other(VatTreatment::NotSubject(NotSubject::N2), eur(200, 0))],
    );
    let res = m303::compute(2026, 3, &[oss, fuera], &[], &Options303::default());
    assert_eq!(res.get(123), eur(500, 0));
    assert_eq!(res.get(120), eur(200, 0));
    assert_eq!(res.get(27), Money::ZERO);
}
