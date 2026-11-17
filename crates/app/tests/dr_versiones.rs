//! Cada fichero de autoliquidación se lee con el diseño de registro de su ejercicio y periodo
//! (`taxpain_models::dr_versions`), y no con el vigente: con datos inventados, un fichero escrito
//! con el diseño de cada versión se lee bien y uno de 2025 leído como si fuera de 2026 no se da
//! por bueno. También la comparación con la gestoría (Importar → Comparar), que lee con
//! `presentados::read_file`.

#![allow(clippy::unwrap_used)]

use taxpain_app::lectura_justificante;
use taxpain_app::presentados::{self, FileError};
use taxpain_core::money::Money;
use taxpain_models::dr::Spec;
use taxpain_models::dr_versions::{self, DESIGNS};

/// El periodo con que se prueba cada diseño (el primero en que vale).
fn period(model: &str, index: u8) -> String {
    match model {
        "390" => "0A".into(),
        "202" => format!("{index}P"),
        _ => format!("{index}T"),
    }
}

/// Casillas inventadas para cada diseño: en el 303, unas que cuadran ([03] y [27] devengado,
/// [29] y [45] deducible, [46] resultado); en el resto, las dos primeras casillas de importe de
/// cada página.
fn boxes(model: &str, spec: &Spec) -> Vec<(u16, Money)> {
    if model == "303" {
        let e = |c: i64| Money::from_cents(c);
        return vec![
            (3, e(21_000)),
            (27, e(21_000)),
            (29, e(6_300)),
            (45, e(6_300)),
            (46, e(14_700)),
        ];
    }
    let mut out = Vec::new();
    let mut n = 0i64;
    for page in &spec.pages {
        let fields: Vec<u16> = page
            .fields
            .iter()
            .filter(|f| f.is_amount() && f.constant.is_none())
            .filter_map(|f| match (f.box_, model) {
                (Some(b), _) => Some(b),
                (None, "111") if (12..=41).contains(&f.n) => Some(f.n - 11),
                _ => None,
            })
            .take(2)
            .collect();
        for b in fields {
            n += 1;
            if !out.iter().any(|(x, _)| *x == b) {
                out.push((b, Money::from_cents(100 * n + 7)));
            }
        }
    }
    out
}

/// El fichero con el diseño de ese ejercicio y periodo (como el de una gestoría).
fn file(model: &str, year: i32, period: &str, spec: &Spec) -> (String, Vec<(u16, Money)>) {
    let b = boxes(model, spec);
    let f = dr_versions::file_with_boxes(model, year, period, &b).unwrap();
    (f, b)
}

#[test]
fn cada_version_se_lee_con_su_diseno() {
    for d in DESIGNS.iter().filter(|d| d.model != "200") {
        let (year, index) = d.from;
        let period = period(d.model, index);
        let (text, written) = file(d.model, year, &period, d.spec());
        assert!(!written.is_empty(), "{}: sin casillas que probar", d.name);
        let r = presentados::read_file_checked(&text)
            .unwrap_or_else(|e| panic!("{} ({year} {period}): {e}", d.name));
        assert_eq!((r.model.as_str(), r.year), (d.model, year), "{}", d.name);
        for (b, m) in &written {
            assert_eq!(r.boxes.get(b), Some(m), "{}: casilla [{b}]", d.name);
        }
    }
}

/// El 303 de 2025 tiene páginas de la misma longitud que el de 2026, con los campos en otro
/// orden: un fichero de 2026 que dice ser de 2025 no se lee con las posiciones de 2026.
#[test]
fn un_303_de_2025_no_se_lee_con_el_diseno_de_2026() {
    let d26 = dr_versions::design("303", 2026, "1T").unwrap();
    let d25 = dr_versions::design("303", 2025, "1T").unwrap();
    assert_ne!(d25.name, d26.name);
    let (text26, written) = file("303", 2026, "1T", d26.spec());
    let as_2025 = text26.replacen("<T303020261T0000>", "<T303020251T0000>", 1);
    match presentados::read_file_checked(&as_2025) {
        // O no cuadra la longitud de alguna página, o las casillas no cuadran entre sí…
        Err(FileError::UnknownDesign { .. } | FileError::Incoherent { .. }) => {}
        // …o, si cuadra, lee otras posiciones: no da lo escrito con el diseño de 2026.
        Ok(r) => assert!(
            written.iter().any(|(b, m)| r.boxes.get(b) != Some(m)),
            "se ha leído con el diseño de 2026"
        ),
        Err(e) => panic!("{e}"),
    }
}

#[test]
fn ejercicio_sin_diseno_conocido() {
    let d = dr_versions::design("303", 2026, "1T").unwrap();
    let (text, _) = file("303", 2026, "1T", d.spec());
    let old = text.replacen("<T303020261T0000>", "<T303020161T0000>", 1);
    assert_eq!(
        presentados::read_file_checked(&old).unwrap_err(),
        FileError::UnknownDesign {
            model: "303".into(),
            year: 2016,
            period: "1T".into()
        }
    );
    assert!(presentados::read_file(&old).is_none());
    // Una página cortada (otra longitud): tampoco se lee por posiciones.
    let cut = text.replacen("</T30301000>", " </T30301000>", 1);
    assert!(matches!(
        presentados::read_file_checked(&cut),
        Err(FileError::UnknownDesign { .. })
    ));
}

/// El 200 cambia cada año: un fichero escrito con el diseño de cada ejercicio (de 2017 a 2025) se
/// lee con el suyo, y uno de 2018 que dice ser de 2025 no casa con el diseño de 2025.
#[test]
fn cada_200_se_lee_con_el_diseno_de_su_ejercicio() {
    let e = |c: i64| Money::from_cents(c);
    // Balance que cuadra ([00180] = [00252]), cifra de negocios y resultado inventados.
    let written = [
        (255u16, e(1_234_567)),
        (500, e(234_568)),
        (180, e(3_456_789)),
        (252, e(3_456_789)),
    ];
    for d in DESIGNS.iter().filter(|d| d.model == "200") {
        let (year, _) = d.from;
        let text = dr_versions::file_with_boxes("200", year, "0A", &written).unwrap();
        let b = lectura_justificante::from_file_200(&text, year)
            .unwrap_or_else(|| panic!("{}: no se lee", d.name));
        for (k, m) in &written {
            assert_eq!(b.get(u32::from(*k)), *m, "{}: casilla [{k:05}]", d.name);
        }
        assert!(
            !lectura_justificante::unknown_design(&text, "200", year),
            "{}",
            d.name
        );
    }
    // Sin diseño para 2026, y un fichero de 2018 no casa con el diseño de 2025.
    let text18 = dr_versions::file_with_boxes("200", 2018, "0A", &written).unwrap();
    let as_2025 = text18.replacen("<T200020180A0000>", "<T200020250A0000>", 1);
    assert!(lectura_justificante::from_file_200(&as_2025, 2025).is_none());
    assert!(lectura_justificante::unknown_design(&as_2025, "200", 2025));
    assert!(dr_versions::design("200", 2026, "0A").is_none());
}
