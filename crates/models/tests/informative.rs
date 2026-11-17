//! 347, 190, 180 y 193: agregación, umbrales y registros de 500 posiciones.

use taxpain_core::Money;
use taxpain_models::Declarant;
use taxpain_models::dr::{PageReader, to_latin1};
use taxpain_models::informative::{
    self, Dividend193, Key190, Key347, Op347, Payment190, Property, Rent180, THRESHOLD_347,
};

fn eur(e: i64, c: u8) -> Money {
    Money::from_euros(e, c)
}

fn who() -> Declarant {
    Declarant {
        nif: "B12345674".into(),
        name: "Estudio Peñón SL".into(),
        given_name: None,
    }
}

fn records(f: &str) -> Vec<&str> {
    let recs: Vec<&str> = f.split("\r\n").collect();
    for r in &recs {
        assert_eq!(to_latin1(r).unwrap_or_default().len(), 500, "{r}");
    }
    recs
}

fn op(nif: &str, key: Key347, quarter: u8, euros: i64) -> Op347 {
    Op347 {
        nif: nif.into(),
        name: format!("Tercero {nif}"),
        province: "46".into(),
        key,
        quarter,
        amount: eur(euros, 0),
        business_rental: false,
        cash: Money::ZERO,
    }
}

/// Lo percibido en metálico sólo se declara si pasa de 6.000 € por tercero y año (art. 34.1.h
/// RGAT), en las posiciones 101-115 con el ejercicio en 132-135.
#[test]
fn m347_metalico_de_mas_de_6000() {
    let con_metalico = |nif: &str, q: u8, euros: i64, cash: i64| Op347 {
        cash: eur(cash, 0),
        ..op(nif, Key347::B, q, euros)
    };
    let ops = [
        con_metalico("B12345674", 1, 5000, 4000),
        con_metalico("B12345674", 2, 5000, 2500),
        con_metalico("A22222228", 1, 7000, 6000),
    ];
    let r = informative::compute_347(2026, &ops);
    let by = |nif: &str| r.lines.iter().find(|l| l.nif == nif).unwrap();
    assert_eq!(by("B12345674").cash, eur(6500, 0));
    assert_eq!(
        by("A22222228").cash,
        Money::ZERO,
        "justo 6.000: no se declara"
    );
    let f = informative::file_347(&r, &who()).unwrap_or_else(|e| panic!("{e}"));
    let recs = records(&f);
    // NIF del tercero en las posiciones 18-26 (el del declarante va en 9-17).
    let tercero = |nif: &str| {
        recs.iter()
            .find(|x| x.starts_with("2347") && &x[17..26] == nif)
            .unwrap()
    };
    let b = tercero("B12345674");
    assert_eq!(&b[100..115], "000000000650000", "{b}");
    assert_eq!(&b[131..135], "2026");
    let a = tercero("A22222228");
    assert_eq!(&a[100..115], "000000000000000");
    assert_eq!(&a[131..135], "0000");
}

/// Las claves de proveedor se leen de su código («G03», «H04») y van a las posiciones 78-80.
#[test]
fn m190_clave_del_proveedor_por_codigo() {
    assert_eq!(Key190::from_code("g03"), Some(Key190::G(3)));
    assert_eq!(Key190::from_code("H04"), Some(Key190::H(4)));
    assert_eq!(Key190::from_code("G05"), None);
    assert_eq!(Key190::from_code("A00"), None);
    assert_eq!(Key190::from_code(""), None);
    assert_eq!(Key190::default(), Key190::G(1));
    assert_eq!(Key190::G(3).label(), "clave G.03");
    for c in Key190::SUPPLIER_CODES {
        assert!(Key190::from_code(c).is_some(), "{c}");
    }
    let p = Payment190 {
        nif: "12345678Z".into(),
        name: "Abogada Ruiz".into(),
        province: "46".into(),
        gross: eur(1000, 0),
        withheld: eur(70, 0),
        employee: None,
        key: Key190::G(3),
        deductible: Money::ZERO,
    };
    let r = informative::compute_190(2026, &[p]);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    let f = informative::file_190(&r, &who()).unwrap_or_else(|e| panic!("{e}"));
    let rec = records(&f)[1];
    assert_eq!(&rec[77..80], "G03", "{rec}");
}

#[test]
fn m347_umbral_por_tercero_y_clave() {
    let ops = [
        // Cliente con 2.000 + 1.500 → 3.500 > 3.005,06: entra, con desglose trimestral.
        op("B12345674", Key347::B, 1, 2000),
        op("B12345674", Key347::B, 3, 1500),
        // El mismo NIF como proveedor (clave A) por debajo del umbral: no entra.
        op("B12345674", Key347::A, 2, 3000),
        // Justo en el umbral: no entra (hay que superarlo).
        Op347 {
            amount: THRESHOLD_347,
            ..op("A22222228", Key347::A, 4, 0)
        },
        op("A22222228", Key347::B, 4, 4000),
    ];
    let r = informative::compute_347(2026, &ops);
    assert_eq!(r.lines.len(), 2);
    assert_eq!(r.lines[0].nif, "A22222228");
    assert_eq!(
        r.lines[1].quarters,
        [eur(2000, 0), Money::ZERO, eur(1500, 0), Money::ZERO]
    );
    assert_eq!(r.total, eur(7500, 0));

    let f = informative::file_347(&r, &who()).unwrap_or_else(|e| panic!("{e}"));
    let recs = records(&f);
    assert_eq!(recs.len(), 3);
    assert!(recs[0].starts_with("13472026B12345674ESTUDIO PEÑON SL"));
    let s = informative::spec("347");
    let d = PageReader::new(s.page("347-1").unwrap_or_else(|e| panic!("{e}")), recs[0])
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(d.raw(13).ok().as_deref(), Some("000000002"));
    assert_eq!(d.raw(15).ok().as_deref(), Some("0000000007500"));
    let t = PageReader::new(s.page("347-2D").unwrap_or_else(|e| panic!("{e}")), recs[2])
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(t.raw(5).ok().as_deref(), Some("B12345674"));
    assert_eq!(t.raw(8).ok().as_deref(), Some("D"));
    assert_eq!(t.raw(9).ok().as_deref(), Some("46"));
    assert_eq!(t.raw(12).ok().as_deref(), Some("B"));
    assert_eq!(t.raw(13).ok().as_deref(), Some(" "));
    assert_eq!(t.raw(14).ok().as_deref(), Some("0000000003500"));
    assert_eq!(t.raw(25).ok().as_deref(), Some("0000000002000"));
    assert_eq!(t.raw(37).ok().as_deref(), Some("0000000001500"));
}

#[test]
fn m347_importe_negativo_lleva_n() {
    let r = informative::compute_347(2026, &[op("B12345674", Key347::B, 1, -4000)]);
    let f = informative::file_347(&r, &who()).unwrap_or_else(|e| panic!("{e}"));
    let recs = records(&f);
    assert_eq!(&recs[1][82..83], "N");
}

#[test]
fn m190_profesionales_clave_g() {
    let p = |nif: &str, g: i64, r: i64| Payment190 {
        nif: nif.into(),
        name: "Abogada Ruiz".into(),
        province: "46".into(),
        gross: eur(g, 0),
        withheld: eur(r, 0),
        employee: None,
        key: Key190::G(1),
        deductible: Money::ZERO,
    };
    let r = informative::compute_190(
        2026,
        &[p("12345678Z", 1000, 150), p("12345678Z", 2000, 300)],
    );
    assert_eq!(r.lines.len(), 1);
    assert_eq!(r.total_withheld, eur(450, 0));
    let f = informative::file_190(&r, &who()).unwrap_or_else(|e| panic!("{e}"));
    let recs = records(&f);
    let s = informative::spec("190");
    let t = PageReader::new(s.page("190-2").unwrap_or_else(|e| panic!("{e}")), recs[1])
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(t.raw(9).ok().as_deref(), Some("G"));
    assert_eq!(t.raw(10).ok().as_deref(), Some("01"));
    assert_eq!(t.raw(12).ok().as_deref(), Some("00000003000"));
    assert_eq!(t.raw(14).ok().as_deref(), Some("00000000450"));
}

#[test]
fn m190_empleados_clave_a() {
    use taxpain_models::informative::Employee190;
    let e = Employee190 {
        birth_year: 1990,
        family: 2,
        spouse_nif: Some("00000000t".into()),
        disability: 0,
        contract: 1,
        mobility: false,
        deductible_expenses: eur(130, 0),
        children_under3: 1,
        children_rest: 2,
        children_whole: false,
    };
    let p = |g: i64, r: i64| Payment190 {
        nif: "12345678Z".into(),
        name: "Trabajadora Uno".into(),
        province: "46".into(),
        gross: eur(g, 0),
        withheld: eur(r, 0),
        employee: Some(e.clone()),
        key: Key190::A,
        deductible: Money::ZERO,
    };
    // Dos nóminas del mismo empleado y un pago profesional suyo: dos registros (A y G).
    let mut g = p(500, 75);
    g.employee = None;
    g.key = Key190::G(1);
    let r = informative::compute_190(2026, &[p(2000, 240), p(2000, 240), g]);
    assert_eq!(r.lines.len(), 2);
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    let f = informative::file_190(&r, &who()).unwrap_or_else(|e| panic!("{e}"));
    let recs = records(&f);
    let s = informative::spec("190");
    let a = recs
        .iter()
        .skip(1)
        .map(|rec| PageReader::new(s.page("190-2").unwrap_or_else(|e| panic!("{e}")), rec))
        .map(|t| t.unwrap_or_else(|e| panic!("{e}")))
        .find(|t| t.raw(9).ok().as_deref() == Some("A"))
        .unwrap_or_else(|| panic!("sin clave A"));
    assert_eq!(a.raw(10).ok().as_deref(), Some("00"));
    assert_eq!(a.raw(12).ok().as_deref(), Some("00000004000"));
    assert_eq!(a.raw(14).ok().as_deref(), Some("00000000480"));
    assert_eq!(a.raw(25).ok().as_deref(), Some("1990"));
    assert_eq!(a.raw(26).ok().as_deref(), Some("2"));
    assert_eq!(a.raw(27).ok().as_deref(), Some("00000000T"));
    assert_eq!(a.raw(29).ok().as_deref(), Some("1"));
    assert_eq!(a.raw(34).ok().as_deref(), Some("00000000260"));
    assert_eq!(a.raw(40).ok().as_deref(), Some("1"));
    assert_eq!(a.raw(41).ok().as_deref(), Some("0"));
    assert_eq!(a.raw(42).ok().as_deref(), Some("02"));
    // Tres hijos por mitad: pos. 251-253 = «222».
    assert_eq!(a.raw(60).ok().as_deref(), Some("2"));
    assert_eq!(a.raw(61).ok().as_deref(), Some("2"));
    assert_eq!(a.raw(62).ok().as_deref(), Some("2"));

    // Un hijo por entero: «100».
    let mut uno = p(2000, 240);
    uno.employee = Some(Employee190 {
        children_under3: 0,
        children_rest: 1,
        children_whole: true,
        ..e.clone()
    });
    let r = informative::compute_190(2026, &[uno]);
    let f = informative::file_190(&r, &who()).unwrap_or_else(|e| panic!("{e}"));
    let recs = records(&f);
    let t = PageReader::new(s.page("190-2").unwrap_or_else(|e| panic!("{e}")), recs[1])
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(t.raw(43).ok().as_deref(), Some("01"));
    assert_eq!(t.raw(60).ok().as_deref(), Some("1"));
    assert_eq!(t.raw(61).ok().as_deref(), Some("0"));
    assert_eq!(t.raw(62).ok().as_deref(), Some("0"));

    let mut bad = p(100, 0);
    bad.employee = Some(Employee190::default());
    assert_eq!(informative::compute_190(2026, &[bad]).warnings.len(), 3);
}

#[test]
fn m180_con_y_sin_inmueble() {
    let rent = |prop: Option<Property>| Rent180 {
        nif: "B12345674".into(),
        name: "Inmobiliaria SL".into(),
        province: "46".into(),
        base: eur(1000, 0),
        withheld: eur(190, 0),
        property: prop,
    };
    let sin = informative::compute_180(2026, &[rent(None), rent(None)]);
    assert_eq!(sin.lines.len(), 1);
    assert_eq!(sin.total_base, eur(2000, 0));
    assert_eq!(sin.warnings.len(), 1);

    let prop = Property {
        situation: 1,
        cadastral_ref: "9872023VH5797S0001WX".into(),
        street: "Calle de Colón".into(),
        number: "12".into(),
        municipality: "València".into(),
        province_code: "46".into(),
        postal_code: "46004".into(),
    };
    let r = informative::compute_180(2026, &[rent(Some(prop))]);
    assert!(r.warnings.is_empty());
    let f = informative::file_180(&r, &who()).unwrap_or_else(|e| panic!("{e}"));
    let recs = records(&f);
    let s = informative::spec("180");
    let t = PageReader::new(s.page("180-2").unwrap_or_else(|e| panic!("{e}")), recs[1])
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(t.raw(13).ok().as_deref(), Some("19"));
    assert_eq!(t.raw(14).ok().as_deref(), Some("00"));
    assert_eq!(t.raw(15).ok().as_deref(), Some("0000000019000"));
    // «Ejercicio devengo» a ceros: no son atrasos ni reintegros.
    assert_eq!(t.raw(16).ok().as_deref(), Some("0000"));
    assert_eq!(t.raw(18).ok().as_deref(), Some("9872023VH5797S0001WX"));
    assert!(t.raw(20).is_ok_and(|v| v.trim_end() == "CALLE DE COLON"));
}

#[test]
fn los_cuatro_disenos_suman_500() {
    for m in ["347", "190", "180", "193"] {
        for p in &informative::spec(m).pages {
            assert_eq!(p.len(), 500, "{m} {}", p.sheet);
            // Campos contiguos, sin huecos ni solapes (transcritos a mano).
            let mut next = 1;
            for f in &p.fields {
                assert_eq!(f.pos, next, "{m} {} campo {}", p.sheet, f.n);
                next = f.pos + f.len;
            }
        }
    }
}

fn reader(model: &str, sheet: &str, rec: &str) -> PageReader<'static> {
    let s = informative::spec(model);
    PageReader::new(s.page(sheet).unwrap_or_else(|e| panic!("{e}")), rec)
        .unwrap_or_else(|e| panic!("{e}"))
}

#[test]
fn m190_administrador_clave_e_y_dietas_l01() {
    use taxpain_models::informative::{DirectorSs, director_rate, director_subkey};
    assert_eq!(director_subkey(DirectorSs::Asimilado, false), 1);
    assert_eq!(director_subkey(DirectorSs::Asimilado, true), 2);
    assert_eq!(director_subkey(DirectorSs::Autonomo, true), 3);
    assert_eq!(director_subkey(DirectorSs::Autonomo, false), 4);
    assert_eq!(director_subkey(DirectorSs::Ninguno, true), 4);
    assert_eq!(director_rate(true).bp(), 1900);
    assert_eq!(director_rate(false).bp(), 3500);

    let base = |key: Key190, g: i64, r: i64| Payment190 {
        nif: "12345678Z".into(),
        name: "Administradora Uno".into(),
        province: "46".into(),
        gross: eur(g, 0),
        withheld: eur(r, 0),
        employee: None,
        key,
        deductible: Money::ZERO,
    };
    let mut asimilado = base(Key190::E(2), 1000, 190);
    asimilado.deductible = eur(65, 0);
    let r = informative::compute_190(
        2026,
        &[
            base(Key190::E(3), 2000, 380),
            base(Key190::E(3), 2000, 380),
            asimilado,
            base(Key190::L01, 300, 0),
        ],
    );
    assert_eq!(r.lines.len(), 3, "E.02, E.03 y L.01 por separado");
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(r.total_gross, eur(5300, 0));
    assert_eq!(r.total_withheld, eur(950, 0));
    let f = informative::file_190(&r, &who()).unwrap_or_else(|e| panic!("{e}"));
    let recs = records(&f);
    let by_key = |k: &str, sub: &str| {
        recs.iter()
            .skip(1)
            .map(|rec| reader("190", "190-2", rec))
            .find(|t| t.raw(9).ok().as_deref() == Some(k) && t.raw(10).ok().as_deref() == Some(sub))
            .unwrap_or_else(|| panic!("sin {k}.{sub}"))
    };
    let e3 = by_key("E", "03");
    assert_eq!(e3.raw(12).ok().as_deref(), Some("00000004000"));
    assert_eq!(e3.raw(14).ok().as_deref(), Some("00000000760"));
    // Retención ingresada en la Hacienda Estatal; sin gastos deducibles ni datos de la clave A.
    assert_eq!(e3.raw(77).ok().as_deref(), Some("00000000760"));
    assert_eq!(e3.raw(34).ok().as_deref(), Some("00000000000"));
    assert_eq!(e3.raw(25).ok().as_deref(), Some("0000"));
    let e2 = by_key("E", "02");
    assert_eq!(e2.raw(34).ok().as_deref(), Some("00000000065"));
    let l = by_key("L", "01");
    assert_eq!(l.raw(12).ok().as_deref(), Some("00000000300"));
    assert_eq!(l.raw(14).ok().as_deref(), Some("00000000000"));
    assert_eq!(l.raw(77).ok().as_deref(), Some("00000000000"));

    let mut bad = base(Key190::E(4), 100, 35);
    bad.deductible = eur(5, 0);
    let mut l_ret = base(Key190::L01, 100, 0);
    l_ret.withheld = eur(1, 0);
    assert_eq!(
        informative::compute_190(2026, &[bad, base(Key190::E(7), 1, 0), l_ret])
            .warnings
            .len(),
        3
    );
}

#[test]
fn m193_dividendos_con_y_sin_retencion() {
    use time::macros::date;
    let d = |nif: &str, gross: i64, withheld: i64, bp: i32| Dividend193 {
        nif: nif.into(),
        name: format!("Socio {nif}"),
        province: "46".into(),
        accrual: date!(2026 - 07 - 15),
        gross: eur(gross, 0),
        withheld: eur(withheld, 0),
        rate: taxpain_core::Rate::from_bp(bp),
    };
    let r = informative::compute_193(
        2026,
        &[
            d("12345678z", 6000, 1140, 1900),
            d("12345678Z", 0, 0, 1900),
            d("B87654321", 4000, 0, 0),
        ],
    );
    assert_eq!(
        r.lines.len(),
        3,
        "con retención (02) y sin ella (08) en registros distintos"
    );
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(r.total_base, eur(6000, 0));
    assert_eq!(r.total_withheld, eur(1140, 0));
    let f = informative::file_193(&r, &who()).unwrap_or_else(|e| panic!("{e}"));
    let recs = records(&f);
    assert_eq!(recs.len(), 4);
    let t1 = reader("193", "193-1", recs[0]);
    assert!(recs[0].starts_with("1193202"));
    assert_eq!(t1.raw(9).ok().as_deref(), Some("1932026000001"));
    assert_eq!(t1.raw(13).ok().as_deref(), Some("000000003"));
    assert_eq!(t1.raw(14).ok().as_deref(), Some("0000000006000"));
    assert_eq!(t1.raw(16).ok().as_deref(), Some("0000000001140"));
    assert_eq!(t1.raw(18).ok().as_deref(), Some("0000000000000"));
    assert_eq!(t1.raw(23).ok().as_deref(), Some("S"));
    let lines: Vec<_> = recs
        .iter()
        .skip(1)
        .map(|rec| reader("193", "193-2", rec))
        .collect();
    let socio = lines
        .iter()
        .find(|t| {
            t.raw(5).ok().as_deref() == Some("12345678Z") && t.raw(13).ok().as_deref() == Some("02")
        })
        .unwrap_or_else(|| panic!("sin socio 02"));
    assert_eq!(socio.raw(9).ok().as_deref(), Some("46"));
    assert_eq!(socio.raw(10).ok().as_deref(), Some("0"));
    assert_eq!(socio.raw(12).ok().as_deref(), Some("A"));
    assert_eq!(socio.raw(14).ok().as_deref(), Some("0"));
    assert_eq!(socio.raw(19).ok().as_deref(), Some("1"));
    assert_eq!(socio.raw(20).ok().as_deref(), Some("00000006000"));
    assert_eq!(socio.raw(25).ok().as_deref(), Some("00000006000"));
    assert_eq!(socio.raw(27).ok().as_deref(), Some("19"));
    assert_eq!(socio.raw(28).ok().as_deref(), Some("00"));
    assert_eq!(socio.raw(29).ok().as_deref(), Some("00000001140"));
    assert_eq!(socio.raw(35).ok().as_deref(), Some("S"));
    assert_eq!(socio.raw(55).ok().as_deref(), Some("15"));
    assert_eq!(socio.raw(56).ok().as_deref(), Some("07"));
    assert_eq!(socio.raw(57).ok().as_deref(), Some("2026"));
    assert_eq!(socio.raw(58).ok().as_deref(), Some("D"));
    let sl = lines
        .iter()
        .find(|t| t.raw(5).ok().as_deref() == Some("B87654321"))
        .unwrap_or_else(|| panic!("sin la sociedad"));
    assert_eq!(sl.raw(13).ok().as_deref(), Some("08"));
    assert_eq!(sl.raw(20).ok().as_deref(), Some("00000004000"));
    assert_eq!(sl.raw(25).ok().as_deref(), Some("00000000000"));
    assert_eq!(sl.raw(27).ok().as_deref(), Some("00"));
    // Números de orden secuenciales.
    let orders: Vec<String> = lines.iter().filter_map(|t| t.raw(53).ok()).collect();
    assert_eq!(orders, vec!["0000001", "0000002", "0000003"]);

    let mut sin_prov = d("12345678Z", 100, 19, 1900);
    sin_prov.province = String::new();
    sin_prov.accrual = date!(2025 - 12 - 31);
    assert_eq!(
        informative::compute_193(2026, &[sin_prov]).warnings.len(),
        2
    );
}

#[test]
fn m193_intereses_de_prestamos_con_clave_b() {
    use informative::Interest193;
    let i = |nif: &str, gross: i64, withheld: i64| Interest193 {
        nif: nif.into(),
        name: "CARMEN ESPANOLA ESPANOLA".into(),
        province: "28".into(),
        gross: eur(gross, 0),
        withheld: eur(withheld, 0),
        rate: taxpain_core::Rate::percent(19),
    };
    let r = informative::compute_193_with_interest(
        2026,
        &[],
        &[
            i("12345678Z", 300, 57),
            i("12345678z", 200, 38),
            // Sin retención (banco o error): no va en el 193.
            i("A22222228", 1000, 0),
        ],
    );
    assert_eq!(r.lines.len(), 1, "un registro por prestamista");
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(r.total_base, eur(500, 0));
    assert_eq!(r.total_withheld, eur(95, 0));
    let f = informative::file_193(&r, &who()).unwrap_or_else(|e| panic!("{e}"));
    let recs = records(&f);
    assert_eq!(recs.len(), 2);
    let t = reader("193", "193-2", recs[1]);
    assert_eq!(t.raw(5).ok().as_deref(), Some("12345678Z"));
    assert_eq!(t.raw(12).ok().as_deref(), Some("B"));
    assert_eq!(t.raw(13).ok().as_deref(), Some("03"));
    assert_eq!(t.raw(20).ok().as_deref(), Some("00000000500"));
    assert_eq!(t.raw(25).ok().as_deref(), Some("00000000500"));
    assert_eq!(t.raw(29).ok().as_deref(), Some("00000000095"));
    // Sin fecha de devengo (sólo clave A).
    assert_eq!(t.raw(55).ok().as_deref(), Some("00"));
    assert_eq!(t.raw(57).ok().as_deref(), Some("0000"));
}

#[test]
fn m347_se_lee_de_su_fichero() {
    let ops = [
        op("B98765431", Key347::B, 1, 6000),
        op("A22222228", Key347::A, 2, 4840),
    ];
    let r = informative::compute_347(2026, &ops);
    let f = informative::file_347(&r, &who()).unwrap_or_else(|e| panic!("{e}"));
    let l = informative::read_file(&f).unwrap_or_else(|| panic!("no se lee"));
    assert_eq!(
        (l.model.as_str(), l.year, l.period.as_str()),
        ("347", 2026, "0A")
    );
    assert_eq!(l.declarant_nif, "B12345674");
    assert_eq!(l.total, eur(10840, 0));
    let lines: Vec<(&str, &str, Money)> = l
        .lines
        .iter()
        .map(|x| (x.nif.as_str(), x.key.as_str(), x.amount))
        .collect();
    assert_eq!(
        lines,
        [
            ("A22222228", "A", eur(4840, 0)),
            ("B98765431", "B", eur(6000, 0)),
        ]
    );
    assert_eq!(l.lines[0].name, "TERCERO A22222228");
    // Sin fin de línea entre registros, y con un importe cambiado a mano.
    let pegado = f
        .replace("\r\n", "")
        .replacen("000000000484000", "000000000500000", 1);
    let l = informative::read_file(&pegado).unwrap_or_else(|| panic!("no se lee"));
    assert_eq!(l.lines[0].amount, eur(5000, 0));
    // Una autoliquidación no es un 347.
    assert!(informative::read_file("<T30302026").is_none());
}

#[test]
fn m349_se_lee_de_su_fichero() {
    use taxpain_models::m349::{self, Key, Operation};
    let op = |id: &str, key, euros| Operation {
        country: "DE".into(),
        vat_id: id.into(),
        name: "Kunde GmbH".into(),
        key,
        base: eur(euros, 0),
    };
    let r = m349::compute(
        2026,
        "3T",
        &[op("123456788", Key::S, 3000), op("123456788", Key::E, 1200)],
    );
    let f = m349::file(&r, &who(), 1).unwrap_or_else(|e| panic!("{e}"));
    let l = informative::read_file(&f).unwrap_or_else(|| panic!("no se lee"));
    assert_eq!(
        (l.model.as_str(), l.year, l.period.as_str()),
        ("349", 2026, "3T")
    );
    assert_eq!(l.total, eur(4200, 0));
    let lines: Vec<(&str, &str, Money)> = l
        .lines
        .iter()
        .map(|x| (x.nif.as_str(), x.key.as_str(), x.amount))
        .collect();
    assert_eq!(
        lines,
        [
            ("DE123456788", "E", eur(1200, 0)),
            ("DE123456788", "S", eur(3000, 0)),
        ]
    );
}

/// Los totales del registro de declarante se leen de vuelta del fichero (lo que hace la
/// comprobación con la AEAT con el fichero de cada informativa presentada).
#[test]
fn totales_del_declarante_de_vuelta() {
    use time::macros::date;
    let p = |nif: &str, g: i64, r: i64| Payment190 {
        nif: nif.into(),
        name: "Abogada Ruiz".into(),
        province: "46".into(),
        gross: eur(g, 0),
        withheld: eur(r, 0),
        employee: None,
        key: Key190::G(1),
        deductible: Money::ZERO,
    };
    let r190 = informative::compute_190(
        2026,
        &[p("12345678Z", 1000, 150), p("87654321X", 2000, 300)],
    );
    let f = informative::file_190(&r190, &who()).unwrap();
    let s = informative::read_summary(&f).unwrap();
    assert_eq!(
        (
            s.model.as_str(),
            s.year,
            s.period.as_str(),
            s.declarant_nif.as_str()
        ),
        ("190", 2026, "0A", "B12345674")
    );
    assert_eq!(s.records, 2);
    assert_eq!(s.total, eur(3000, 0));
    assert_eq!(s.withheld, Some(eur(450, 0)));

    let d = |nif: &str, gross: i64, withheld: i64| Dividend193 {
        nif: nif.into(),
        name: format!("Socio {nif}"),
        province: "46".into(),
        accrual: date!(2026 - 07 - 15),
        gross: eur(gross, 0),
        withheld: eur(withheld, 0),
        rate: taxpain_core::Rate::from_bp(1900),
    };
    let r193 = informative::compute_193(2026, &[d("12345678Z", 6000, 1140)]);
    let s = informative::read_summary(&informative::file_193(&r193, &who()).unwrap()).unwrap();
    assert_eq!((s.model.as_str(), s.records), ("193", 1));
    assert_eq!(s.total, eur(6000, 0));
    assert_eq!(s.withheld, Some(eur(1140, 0)));

    let rent = Rent180 {
        nif: "B12345674".into(),
        name: "Inmobiliaria SL".into(),
        province: "46".into(),
        base: eur(1000, 0),
        withheld: eur(190, 0),
        property: None,
    };
    let r180 = informative::compute_180(2026, &[rent]);
    let s = informative::read_summary(&informative::file_180(&r180, &who()).unwrap()).unwrap();
    assert_eq!((s.model.as_str(), s.records), ("180", 1));
    assert_eq!(s.total, eur(1000, 0));
    assert_eq!(s.withheld, Some(eur(190, 0)));

    let r347 = informative::compute_347(2026, &[op("A22222228", Key347::B, 1, 7000)]);
    let s = informative::read_summary(&informative::file_347(&r347, &who()).unwrap()).unwrap();
    assert_eq!((s.model.as_str(), s.records, s.withheld), ("347", 1, None));
    assert_eq!(s.total, eur(7000, 0));
    // Otro fichero: nada.
    assert!(informative::read_summary("<T3030202...>").is_none());
}

/// Los perceptores del 190, el 180 y el 193 se leen de vuelta del fichero (para compararlos por
/// NIF y clave con lo que calcula TaxPain).
#[test]
fn perceptores_de_vuelta() {
    use time::macros::date;
    let p = |nif: &str, key: Key190, g: i64, r: i64| Payment190 {
        nif: nif.into(),
        name: format!("Perceptor {nif}"),
        province: "46".into(),
        gross: eur(g, 0),
        withheld: eur(r, 0),
        employee: None,
        key,
        deductible: Money::ZERO,
    };
    let r190 = informative::compute_190(
        2026,
        &[
            p("12345678Z", Key190::G(1), 1000, 150),
            p("87654321X", Key190::E(1), 2000, 380),
        ],
    );
    let f = informative::read_file(&informative::file_190(&r190, &who()).unwrap()).unwrap();
    assert_eq!((f.model.as_str(), f.period.as_str()), ("190", "0A"));
    assert_eq!(f.withheld, Some(eur(530, 0)));
    let mut l: Vec<(String, String, Money, Option<Money>)> = f
        .lines
        .into_iter()
        .map(|l| (l.nif, l.key, l.amount, l.withheld))
        .collect();
    l.sort();
    assert_eq!(
        l,
        vec![
            (
                "12345678Z".into(),
                "G01".into(),
                eur(1000, 0),
                Some(eur(150, 0))
            ),
            (
                "87654321X".into(),
                "E01".into(),
                eur(2000, 0),
                Some(eur(380, 0))
            ),
        ]
    );

    let rent = Rent180 {
        nif: "A22222228".into(),
        name: "Inmobiliaria Demo SA".into(),
        province: "46".into(),
        base: eur(1000, 0),
        withheld: eur(190, 0),
        property: None,
    };
    let r180 = informative::compute_180(2026, &[rent]);
    let f = informative::read_file(&informative::file_180(&r180, &who()).unwrap()).unwrap();
    assert_eq!(f.lines.len(), 1);
    assert_eq!(
        (
            f.lines[0].nif.as_str(),
            f.lines[0].amount,
            f.lines[0].withheld
        ),
        ("A22222228", eur(1000, 0), Some(eur(190, 0)))
    );

    let d = Dividend193 {
        nif: "12345678Z".into(),
        name: "Carmen Española Española".into(),
        province: "46".into(),
        accrual: date!(2026 - 07 - 15),
        gross: eur(6000, 0),
        withheld: eur(1140, 0),
        rate: taxpain_core::Rate::from_bp(1900),
    };
    let r193 = informative::compute_193(2026, &[d]);
    let f = informative::read_file(&informative::file_193(&r193, &who()).unwrap()).unwrap();
    assert_eq!(
        (
            f.lines[0].key.as_str(),
            f.lines[0].amount,
            f.lines[0].withheld
        ),
        ("A", eur(6000, 0), Some(eur(1140, 0)))
    );
}

/// El 303, el 111 y el 115 de un mes llevan el periodo «01»…«12» (sus diseños de registro lo
/// admiten), y el último periodo del año mensual es diciembre.
#[test]
fn periodo_mensual_en_los_ficheros() {
    use taxpain_models::periodo;
    use taxpain_models::withholding::{self, Payment};
    let rents = [Payment {
        payee: "A22222228".into(),
        base: eur(1000, 0),
        withheld: eur(190, 0),
    }];
    let r = withholding::compute_115(2026, periodo::de_mes(7), &rents, false);
    let f = withholding::file(&r, &who(), None).unwrap();
    assert!(f.starts_with("<T1150202607"), "{}", &f[..20]);
    let p1 = taxpain_models::withholding::spec_115().page("01").unwrap();
    let start = f.find("<T11501000>").unwrap();
    let rec: String = f[start..].chars().take(p1.len()).collect();
    let rd = PageReader::new(p1, &rec).unwrap();
    assert_eq!(rd.raw(11).unwrap(), "07");

    let r = taxpain_models::m303::compute(
        2026,
        periodo::de_mes(12),
        &[],
        &[],
        &taxpain_models::m303::Options303 {
            redeme: true,
            ..Default::default()
        },
    );
    let f = taxpain_models::m303::file(
        &r,
        &who(),
        &taxpain_models::m303::Options303 {
            redeme: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(f.starts_with("<T3030202612"), "{}", &f[..20]);
}
