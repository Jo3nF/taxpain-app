//! Modelo 390: el fichero de cada ejercicio con su diseño de registro (el de 2025 hasta 2025 y
//! el de 2026 desde 2026) y el volumen de operaciones sin las ventas de inmovilizado.
//! Escenarios sintéticos con las cuentas hechas a mano.

use taxpain_core::invoice::{Exemption, NotSubject, TaxBreakdown, VatTreatment};
use taxpain_core::{Money, Rate};
use taxpain_models::Declarant;
use taxpain_models::dr::{PageReader, Spec, pages_fit};
use taxpain_models::m303::{
    self, Exonerado390, IssuedVat, Options303, Prorata, ReceivedKind, ReceivedVat, Result303,
    SelfAssessed, SelfAssessedKind, Volumen390,
};
use taxpain_models::m390::{self, Annual390, Options390, Result390};
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

fn issued(number: &str, breakdown: Vec<TaxBreakdown>) -> IssuedVat {
    IssuedVat {
        number: number.into(),
        date: date!(2025 - 08 - 01),
        rectification: false,
        breakdown,
        cash_basis: false,
        eu_business: false,
        oss: false,
    }
}

fn received(kind: ReceivedKind, base: Money, vat: Money, pct: u8) -> ReceivedVat {
    ReceivedVat {
        number: "R".into(),
        date: date!(2025 - 08 - 02),
        kind,
        rectification: false,
        base,
        vat,
        deductible_pct: pct,
        self_assessed: None,
        import: false,
        prorata_use: Default::default(),
        outside_prorata: false,
    }
}

/// Emitidas del año: 21 %, 10 % con recargo del 1,4 %, 4 %, entregas intracomunitarias,
/// exportación, exenta del art. 20, inversión del sujeto pasivo, servicios a empresarios de la
/// UE y de fuera, y una rectificativa. Con `caja`, además una del criterio de caja.
fn emitidas(caja: bool) -> Vec<IssuedVat> {
    let mut re = taxed(1000, eur(500, 0), eur(50, 0));
    re.surcharge_rate = Some(Rate::from_bp(140));
    re.surcharge_amount = eur(7, 0);
    let mut ue = issued(
        "A9",
        vec![other(VatTreatment::NotSubject(NotSubject::N2), eur(300, 0))],
    );
    ue.eu_business = true;
    let mut rect = issued("R1", vec![taxed(2100, -eur(100, 0), -eur(21, 0))]);
    rect.rectification = true;
    let mut out = vec![
        issued("A1", vec![taxed(2100, eur(1800, 0), eur(378, 0))]),
        issued("A2", vec![re]),
        issued("A3", vec![taxed(400, eur(250, 0), eur(10, 0))]),
        issued(
            "A4",
            vec![other(VatTreatment::Exempt(Exemption::E5), eur(1000, 0))],
        ),
        issued(
            "A5",
            vec![other(VatTreatment::Exempt(Exemption::E2), eur(600, 0))],
        ),
        issued(
            "A6",
            vec![other(VatTreatment::Exempt(Exemption::E1), eur(400, 0))],
        ),
        issued("A7", vec![other(VatTreatment::ReverseCharge, eur(700, 0))]),
        issued(
            "A8",
            vec![other(
                VatTreatment::NotSubject(NotSubject::N2),
                eur(2000, 0),
            )],
        ),
        ue,
        rect,
    ];
    if caja {
        let mut c = issued("C1", vec![taxed(2100, eur(400, 0), eur(84, 0))]);
        c.cash_basis = true;
        out.push(c);
    }
    out
}

/// Recibidas: corrientes al 21 % y al 10 %, un bien de inversión, un vehículo al 50 %, un
/// abono, una sin derecho a deducir, adquisiciones intracomunitarias de bienes y de servicios
/// y una importación.
fn recibidas() -> Vec<ReceivedVat> {
    let ue = |base: i64, quota: i64, goods: bool| ReceivedVat {
        self_assessed: Some(SelfAssessed {
            kind: SelfAssessedKind::Intracomunitaria,
            quota: eur(quota, 0),
            goods,
            part: Default::default(),
        }),
        ..received(ReceivedKind::Corriente, eur(base, 0), Money::ZERO, 100)
    };
    let mut abono = received(ReceivedKind::Corriente, -eur(50, 0), -eur(10, 50), 100);
    abono.rectification = true;
    let mut dua = received(ReceivedKind::Corriente, eur(300, 0), eur(63, 0), 100);
    dua.import = true;
    vec![
        received(ReceivedKind::Corriente, eur(250, 0), eur(52, 50), 100),
        received(ReceivedKind::Corriente, eur(150, 0), eur(15, 0), 100),
        received(ReceivedKind::BienInversion, eur(1200, 0), eur(252, 0), 100),
        received(ReceivedKind::Corriente, eur(400, 0), eur(84, 0), 50),
        abono,
        received(ReceivedKind::Corriente, eur(30, 0), eur(6, 30), 0),
        ue(100, 21, true),
        ue(200, 42, false),
        dua,
    ]
}

/// Los cuatro 303 del año: todo en el 3T.
fn trimestres(year: i32, i: &[IssuedVat], r: &[ReceivedVat]) -> Vec<Result303> {
    let o = Options303::default();
    (1..=4)
        .map(|q| {
            if q == 3 {
                m303::compute(year, q, i, r, &o)
            } else {
                m303::compute(year, q, &[], &[], &o)
            }
        })
        .collect()
}

/// Prorrata general (provisional 80 %, definitiva 75 %) y criterio de caja.
fn prorrata_y_caja() -> Annual390 {
    Annual390 {
        prorata: Some(Prorata {
            provisional: 80,
            special: false,
            investment_regularization: -eur(5, 0),
            definitive: Some(75),
            operations: eur(4800, 0),
            deductible_operations: eur(3600, 0),
            regularization: -eur(12, 0),
            cnae: "62.01".into(),
        }),
        cash_basis: true,
        cash_recipient: true,
        cash_issued_base: eur(400, 0),
        cash_issued_vat: eur(84, 0),
        cash_received_base: eur(100, 0),
        cash_received_vat: eur(21, 0),
        ..Annual390::default()
    }
}

fn declarante() -> Declarant {
    Declarant {
        nif: "B12345674".into(),
        name: "Estudio Ejemplo SL".into(),
        given_name: None,
    }
}

fn opciones() -> Options390 {
    Options390 {
        activity_code: "A03".into(),
        iae: "0742".into(),
        files_347: true,
        activity: "Consultoría informática".into(),
    }
}

/// El 390 de `year` sin nada especial y con prorrata y criterio de caja.
fn escenarios(year: i32) -> [(&'static str, Result390); 2] {
    let r = recibidas();
    let i = emitidas(false);
    let base = m390::compute(year, &i, &r, &trimestres(year, &i, &r), Money::ZERO);
    let i = emitidas(true);
    let caja = m390::compute_with(
        year,
        &i,
        &r,
        &trimestres(year, &i, &r),
        eur(10, 0),
        &prorrata_y_caja(),
    );
    [("base", base), ("prorrata", caja)]
}

/// Las páginas del fichero, sin la cabecera `<AUX>` (versión y NIF del productor) ni el cierre.
fn paginas(f: &str) -> &str {
    let start = f.find("</AUX>").map_or(0, |i| i + "</AUX>".len());
    let end = f.rfind("</T390").unwrap_or(f.len());
    &f[start..end]
}

/// Registro de la página `page` («01», «06»…) del fichero, de su marca a su cierre.
fn registro(f: &str, page: &str) -> String {
    let marca = format!("<T390{page}000>");
    let cierre = format!("</T390{page}000>");
    let at = f.find(&marca).unwrap_or_else(|| panic!("falta {marca}"));
    let len = f[at..]
        .find(&cierre)
        .unwrap_or_else(|| panic!("falta {cierre}"))
        + cierre.len();
    f[at..at + len].to_string()
}

/// `len` posiciones desde la `pos` (1, como en el diseño) del registro.
fn en(rec: &str, pos: usize, len: usize) -> String {
    rec.chars().skip(pos - 1).take(len).collect()
}

/// Importe de 17 posiciones del diseño: 15 enteros y 2 decimales, «N» delante si es negativo.
fn importe(m: Money) -> String {
    let digits = m.cents().unsigned_abs().to_string();
    if m.is_negative() {
        format!("N{digits:0>16}")
    } else {
        format!("{digits:0>17}")
    }
}

fn hoja<'a>(s: &'a Spec, sheet: &str) -> &'a taxpain_models::dr::PageSpec {
    s.pages
        .iter()
        .find(|p| p.sheet == sheet)
        .unwrap_or_else(|| panic!("{sheet}"))
}

/// Primera posición en que difieren dos ficheros, con la página y el campo del diseño de 2025.
fn primera_diferencia(a: &str, b: &str) -> String {
    let Some(i) = a.chars().zip(b.chars()).position(|(x, y)| x != y) else {
        return format!("longitudes {} y {}", a.chars().count(), b.chars().count());
    };
    for p in &m390::spec_for(2025).pages {
        let marca: String = p.fields[..4]
            .iter()
            .filter_map(|f| f.constant.clone())
            .collect();
        if !marca.ends_with('>') {
            continue;
        }
        let Some(at) = a.find(&marca).map(|byte| a[..byte].chars().count()) else {
            continue;
        };
        if (at..at + p.len()).contains(&i) {
            let pos = i - at + 1;
            let f = p
                .fields
                .iter()
                .find(|f| (f.pos..f.pos + f.len).contains(&pos));
            return format!(
                "{}, posición {pos}: campo {:?}",
                p.sheet,
                f.map(|f| (f.n, &f.desc))
            );
        }
    }
    format!("posición {i}")
}

/// Hasta 2025 el fichero sale igual que antes del diseño de 2026, carácter a carácter: el de
/// 2025 lo generó el código anterior (`tests/esperado/`), y los años anteriores salen con ese
/// diseño, que sólo cambia el ejercicio de la página 1.
#[test]
fn hasta_2025_el_fichero_no_cambia() {
    let esperado = [
        ("base", include_str!("esperado/390-2025-base.txt")),
        ("prorrata", include_str!("esperado/390-2025-prorrata.txt")),
    ];
    for year in [2017, 2024, 2025] {
        for ((name, r), (_, before)) in escenarios(year).into_iter().zip(esperado) {
            let f = m390::file(&r, &declarante(), &opciones()).unwrap_or_else(|e| panic!("{e}"));
            assert!(f.starts_with(&format!("<T3900{year}0A0000><AUX>")));
            assert!(pages_fit(m390::spec_for(2025), &f), "{name} {year}");
            // El ejercicio va en las posiciones 103 a 106 de la página 1, la primera.
            let mut now: Vec<char> = paginas(&f).chars().collect();
            assert_eq!(now[102..106].iter().collect::<String>(), year.to_string());
            now.splice(102..106, "2025".chars());
            let now: String = now.into_iter().collect();
            assert!(
                now == before,
                "{name} {year}: {}",
                primera_diferencia(before, &now)
            );
        }
    }
}

/// El 390 de 2026 con prorrata y criterio de caja, campo a campo con las posiciones del diseño
/// oficial (`dr390e2026.xlsx`, v1.00), escritas aquí a mano: las que cambian (marca de
/// carburantes, campos corridos de la página 1, [112] en la 6 y CNAE de 4 cifras en la 7) y
/// todas las casillas que rellena TaxPain.
#[test]
fn el_390_de_2026_sigue_su_diseno() {
    let [_, (_, r)] = escenarios(2026);
    let f = m390::file(&r, &declarante(), &opciones()).unwrap_or_else(|e| panic!("{e}"));
    let s = taxpain_models::dr_versions::spec_for("390", 2026, "0A")
        .unwrap_or_else(|| panic!("sin el diseño de 2026"));
    assert_eq!(s.source, "dr390e2026.xlsx");
    assert!(pages_fit(s, &f));
    assert!(!pages_fit(m390::spec_for(2025), &f), "no es el de 2025");
    assert!(f.starts_with("<T390020260A0000><AUX>"));
    assert!(f.ends_with("</T390020260A0000>"));

    // Las páginas, seguidas y cada una con la longitud que da su hoja («TOTAL … POSICIONES»).
    let mut rest: Vec<char> = paginas(&f).chars().collect();
    for (page, len) in [
        ("01000", 1188),
        ("02000", 1806),
        ("02B00", 531),
        ("03000", 1840),
        ("04000", 854),
        ("06000", 862),
        ("07000", 781),
    ] {
        let rec: String = rest.drain(..len.min(rest.len())).collect();
        assert!(rec.starts_with(&format!("<T390{page}>")), "{page}");
        assert!(rec.ends_with(&format!("</T390{page}>")), "{page}: {len}");
    }
    assert!(rest.is_empty(), "sobra {}", rest.len());

    let p1 = registro(&f, "01");
    assert_eq!(p1.chars().count(), 1188);
    assert_eq!(en(&p1, 14, 9), "B12345674");
    assert_eq!(en(&p1, 23, 60).trim_end(), "ESTUDIO EJEMPLO SL");
    assert_eq!(en(&p1, 103, 4), "2026");
    // Criterio de caja: acogido (131) y destinatario (132).
    assert_eq!(en(&p1, 131, 2), "11");
    // Campo 22, nuevo: derecho a deducir el pago a cuenta de carburantes, «2» (no).
    assert_eq!(en(&p1, 133, 1), "2");
    // Sustitutiva (134) y por rectificación de cuotas (135); justificante (136, 13) en blanco.
    assert_eq!(en(&p1, 134, 2), "00");
    assert_eq!(en(&p1, 136, 13), " ".repeat(13));
    // Actividad principal: A (149, 40) en blanco, B clave (189, 3) y C epígrafe (192, 4).
    assert_eq!(en(&p1, 149, 40), " ".repeat(40));
    assert_eq!(en(&p1, 189, 3), "A03");
    assert_eq!(en(&p1, 192, 4), "0742");
    // D: operaciones con terceras personas (431).
    assert_eq!(en(&p1, 431, 1), "1");
    assert_eq!(en(&p1, 1177, 12), "</T39001000>");

    // Página 6: apartados 7 a 10. [112] (pago a cuenta de carburantes) a cero las dos veces.
    let p6 = registro(&f, "06");
    assert_eq!(p6.chars().count(), 862);
    let zero = "0".repeat(17);
    for (b, pos) in [
        (84, 30),
        (85, 64),
        (86, 98),
        (95, 259),
        (97, 310),
        (98, 327),
        (662, 344),
        (99, 395),
        (653, 412),
        (103, 429),
        (104, 446),
        (105, 463),
        (110, 480),
        (125, 497),
        (106, 650),
        (107, 667),
        (108, 684),
    ] {
        assert_eq!(en(&p6, pos, 17), importe(r.get(b)), "[{b}] en {pos}");
    }
    assert_eq!(en(&p6, 81, 17), zero, "[112] del apartado 7");
    assert_eq!(en(&p6, 225, 17), zero, "[112] del apartado 8");
    // Apartado 8 (territorio) sin rellenar: [87] a [91] y [84] repetida.
    assert_eq!(en(&p6, 115, 25), "0".repeat(25));
    assert_eq!(en(&p6, 157, 17), zero);
    assert_eq!(en(&p6, 701, 150), " ".repeat(150));
    assert_eq!(en(&p6, 851, 12), "</T39006000>");

    // Página 7: criterio de caja y primera prorrata, con el CNAE de 4 cifras.
    let p7 = registro(&f, "07");
    assert_eq!(p7.chars().count(), 781);
    for (b, pos) in [(654, 132), (655, 149), (656, 166), (657, 183)] {
        assert_eq!(en(&p7, pos, 17), importe(r.get(b)), "[{b}] en {pos}");
    }
    assert_eq!(en(&p7, 200, 40).trim_end(), "CONSULTORIA INFORMATICA");
    assert_eq!(en(&p7, 240, 4), "6201");
    assert_eq!(en(&p7, 244, 17), importe(eur(4800, 0)), "[115]");
    assert_eq!(en(&p7, 261, 17), importe(eur(3600, 0)), "[116]");
    assert_eq!(en(&p7, 278, 1), "G", "[117]");
    assert_eq!(en(&p7, 279, 5), "07500", "[118]");
    // La segunda prorrata empieza en 284, vacía.
    assert_eq!(en(&p7, 284, 44), " ".repeat(44));
    assert_eq!(en(&p7, 770, 12), "</T39007000>");

    // Cada casilla calculada, en su campo del diseño de 2026 (la primera vez que aparece).
    let mut seen = 0;
    for (b, v) in &r.boxes {
        let hit = s.pages.iter().find_map(|p| {
            let field = p.field_for_box(*b).ok()?;
            let marca: String = p.fields[..4]
                .iter()
                .filter_map(|f| f.constant.clone())
                .collect();
            let rec = f
                .find(&marca)
                .map(|i| f[i..].chars().take(p.len()).collect::<String>())?;
            Some(
                PageReader::new(p, &rec)
                    .ok()?
                    .amount(field.n)
                    .ok()
                    .flatten(),
            )
        });
        let Some(read) = hit else {
            panic!("[{b}] no está en el diseño de 2026");
        };
        assert_eq!(read, Some(*v), "[{b}]");
        seen += 1;
    }
    assert!(seen > 60, "{seen} casillas");
}

/// El diseño de 2026 no cambia ningún cálculo: [86] = [84] + [659] − [85] − [112], y TaxPain no
/// tiene [659] ni [112] (pago a cuenta de carburantes, modelo 319).
#[test]
fn el_resultado_no_cambia_con_el_pago_a_cuenta_de_2026() {
    for year in [2025, 2026] {
        let [(_, a), (_, b)] = escenarios(year);
        for r in [a, b] {
            assert_eq!(r.get(86), r.get(84) - r.get(85), "{year}");
            assert!(!r.boxes.contains_key(&112));
        }
    }
    // Los mismos importes en los dos años: el diseño no cambia ningún cálculo.
    let [(_, a25), (_, b25)] = escenarios(2025);
    let [(_, a26), (_, b26)] = escenarios(2026);
    assert_eq!(a25.boxes, a26.boxes);
    assert_eq!(b25.boxes, b26.boxes);
}

/// Ventas de inmovilizado: una máquina (bien de inversión, 3.000) y un local exento (20.000),
/// ya facturadas: suman en [99] y [105] y se restan en [107] y [106] (art. 121.Tres LIVA).
#[test]
fn las_ventas_de_inmovilizado_no_cuentan_en_el_volumen() {
    let mut i = emitidas(false);
    i.push(issued("V1", vec![taxed(2100, eur(3000, 0), eur(630, 0))]));
    i.push(issued(
        "V2",
        vec![other(VatTreatment::Exempt(Exemption::E1), eur(20000, 0))],
    ));
    let r = recibidas();
    let extra = Annual390 {
        capital_goods: eur(3000, 0),
        real_estate: eur(20000, 0),
        ..Annual390::default()
    };
    let sin = m390::compute(2026, &i, &r, &trimestres(2026, &i, &r), Money::ZERO);
    let con = m390::compute_with(2026, &i, &r, &trimestres(2026, &i, &r), Money::ZERO, &extra);
    // Régimen general: 1.800 + 500 + 250 − 100 + 3.000 = 5.450; exentas sin derecho a deducir:
    // 400 + 20.000; intracomunitarias 1.000 + 300; exportación 600; ISP 700; no sujetas 2.000.
    assert_eq!(con.get(99), eur(5450, 0));
    assert_eq!(con.get(105), eur(20400, 0));
    assert_eq!(con.get(103), eur(1300, 0));
    assert_eq!(con.get(104), eur(600, 0));
    assert_eq!(con.get(125), eur(700, 0));
    assert_eq!(con.get(110), eur(2000, 0));
    assert_eq!(sin.get(108), eur(30450, 0));
    assert_eq!((con.get(106), con.get(107)), (eur(20000, 0), eur(3000, 0)));
    assert_eq!(con.get(108), eur(30450 - 23000, 0));
    assert_eq!((sin.get(106), sin.get(107)), (Money::ZERO, Money::ZERO));
    // Lo demás no cambia.
    let mut a = con.boxes.clone();
    let mut b = sin.boxes.clone();
    for k in [106, 107, 108] {
        a.remove(&k);
        b.remove(&k);
    }
    assert_eq!(a, b);

    // En el fichero de cada diseño, en su sitio: 616/633/650 en el de 2025 y 650/667/684 en el
    // de 2026.
    for (year, pos) in [(2025, [616, 633, 650]), (2026, [650, 667, 684])] {
        let mut r = con.clone();
        r.year = year;
        let f = m390::file(&r, &declarante(), &opciones()).unwrap_or_else(|e| panic!("{e}"));
        let p6 = registro(&f, "06");
        assert_eq!(en(&p6, pos[0], 17), importe(eur(20000, 0)), "{year}");
        assert_eq!(en(&p6, pos[1], 17), importe(eur(3000, 0)), "{year}");
        assert_eq!(en(&p6, pos[2], 17), importe(eur(7450, 0)), "{year}");
    }

    // El exonerado lleva el mismo volumen en la página 04 del 303 del 4T: [79] y [99] restan.
    let v = Volumen390::of(&con);
    assert_eq!(
        (v.real_estate, v.capital_goods),
        (eur(20000, 0), eur(3000, 0))
    );
    assert_eq!(v.total(), con.get(108));
    let o = Options303 {
        exempt_from_390: true,
        exonerado_390: Some(Exonerado390 {
            activity_code: "A03".into(),
            iae: "0742".into(),
            files_347: false,
            volume: v,
        }),
        ..Options303::default()
    };
    let q4 = m303::compute(2026, 4, &[], &[], &o);
    let f = m303::file(&q4, &declarante(), &o).unwrap_or_else(|e| panic!("{e}"));
    let s = m303::spec();
    let p4 = s.page("04").unwrap_or_else(|e| panic!("{e}"));
    let at = f
        .find("<T30304000>")
        .unwrap_or_else(|| panic!("sin página 04"));
    let rec: String = f[at..].chars().take(p4.len()).collect();
    let p4 = PageReader::new(p4, &rec).unwrap_or_else(|e| panic!("{e}"));
    let caja = |b: u16| p4.amount_box(b).ok().flatten();
    assert_eq!(caja(79), Some(eur(20000, 0)));
    assert_eq!(caja(99), Some(eur(3000, 0)));
    assert_eq!(caja(83), Some(eur(20400, 0)));
    assert_eq!(caja(88), Some(eur(7450, 0)));
}

/// El diseño embebido de cada ejercicio es el oficial: el original está en `fixtures/aeat/dr/`
/// (vigente) o en `anteriores/`, y cada página mide lo que dice su hoja («TOTAL … POSICIONES»).
#[test]
fn los_disenos_embebidos_son_los_oficiales() {
    for (year, path, totals) in [
        (
            2026,
            "dr390e2026.xlsx",
            [1188, 1806, 531, 1840, 854, 1519, 862, 781, 1092],
        ),
        (
            2025,
            "anteriores/dr390e2025.xlsx",
            [1187, 1806, 531, 1840, 854, 1519, 828, 776, 1092],
        ),
    ] {
        let s = m390::spec_for(year);
        assert!(path.ends_with(&s.source), "{year}: {}", s.source);
        let bytes = std::fs::read(format!(
            "{}/../../fixtures/aeat/dr/{path}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap_or_default();
        assert!(!bytes.is_empty(), "{path}");
        let sheets = [
            "Pág. 1",
            "Pág. 2",
            "Pág. 2 bis",
            "Pág. 3",
            "Pág. 4",
            "Pág. 5",
            "Pág. 6",
            "Pág. 7",
            "Pág. 8",
        ];
        for (sheet, total) in sheets.into_iter().zip(totals) {
            assert_eq!(hoja(s, sheet).len(), total, "{year} {sheet}");
        }
    }
    assert!(std::ptr::eq(m390::spec_for(2027), m390::spec()));
    assert_eq!(m390::spec().source, "dr390e2026.xlsx");
}

/// Ventanilla única (OSS): en [126] y en el volumen de operaciones [108], no en [110].
#[test]
fn ventas_oss_en_126() {
    let mut oss = issued(
        "OSS",
        vec![other(VatTreatment::NotSubject(NotSubject::N2), eur(500, 0))],
    );
    oss.oss = true;
    let fuera = issued(
        "US",
        vec![other(VatTreatment::NotSubject(NotSubject::N2), eur(200, 0))],
    );
    let i = vec![oss, fuera];
    let r = m390::compute(2026, &i, &[], &trimestres(2026, &i, &[]), Money::ZERO);
    assert_eq!(r.get(126), eur(500, 0));
    assert_eq!(r.get(110), eur(200, 0));
    assert_eq!(r.get(108), eur(700, 0));
    assert_eq!(m303::Volumen390::of(&r).total(), eur(700, 0));
    // Y se escribe en la página 6 del diseño de 2026.
    let txt = m390::file(&r, &declarante(), &opciones()).unwrap();
    assert!(txt.contains("00000000000050000"));
}
