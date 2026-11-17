//! Perfil fiscal deducido de lo presentado: una empresa sin perfil guardado, comprobada con la
//! AEAT, y la revisión de su carpeta pide lo que falta con el perfil deducido, sin pedir lo de
//! años sin ese modelo cerca. Datos inventados, NIF de prueba.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::companies::{self, CompanyInfo};
use taxpain_app::presentado_aeat::{self, Answer, COMMON, Presented};
use taxpain_app::revision;
use taxpain_core::calendar::{Periodicity, Persona};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

const NIF: &str = "B12345674";

fn vault() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let info = CompanyInfo {
        name: "Empresa Demo SL".into(),
        nif: Some(NIF.into()),
    };
    companies::set_info(&v, &info, datetime!(2026-01-01 9:00 UTC)).unwrap();
    v
}

fn p(model: &str, year: i32, period: &str) -> Presented {
    Presented {
        model: model.into(),
        year,
        period: period.into(),
        csv: format!("{:X<16}", format!("C{model}{year}{period}")),
        file_number: String::new(),
        receipt: format!("{model}{year}{period}"),
        previous_receipt: None,
        presented_at: format!("{}-01-20T10:00:00", year + 1),
    }
}

fn presentar(v: &Vault, year: i32, list: Vec<Presented>) {
    let a = Answer {
        models: COMMON.iter().map(|m| (*m).to_owned()).collect(),
        list,
        ..Answer::default()
    };
    presentado_aeat::record(v, NIF, year, &a, datetime!(2026-10-31 10:00 UTC)).unwrap();
}

/// 303, 111, 202 y 200 presentados en 2024 y 2025 (en 2025 falta el 303 del 2T); en 2022 sólo
/// el 200.
fn empresa() -> Vault {
    let v = vault();
    for y in [2024, 2025] {
        let mut l = Vec::new();
        for t in ["1T", "2T", "3T", "4T"] {
            if !(y == 2025 && t == "2T") {
                l.push(p("303", y, t));
            }
            l.push(p("111", y, t));
        }
        l.extend([
            p("390", y, "0A"),
            p("190", y, "0A"),
            p("200", y, "0A"),
            p("202", y, "1P"),
            p("202", y, "2P"),
            p("202", y, "3P"),
        ]);
        presentar(&v, y, l);
    }
    presentar(&v, 2022, vec![p("200", 2022, "0A")]);
    v
}

#[test]
fn perfil_deducido_de_303_111_202_y_200() {
    let v = empresa();
    let d = taxpain_app::perfil_deducido::deducir(&v).unwrap().unwrap();
    assert_eq!(d.perfil.persona, Persona::Juridica);
    assert_eq!(
        d.perfil.iva.map(|i| (i.periodicity, i.sii)),
        Some((Periodicity::Trimestral, false))
    );
    assert!(d.perfil.retiene_trabajo_profesionales);
    assert!(d.perfil.pagos_fraccionados_is);
    assert!(!d.perfil.retiene_alquileres);
    assert!(
        d.motivos
            .contains(&"303 trimestral (2024 a 2025)".to_owned()),
        "{:?}",
        d.motivos
    );
    // Sin nada presentado no hay perfil que deducir.
    assert!(
        taxpain_app::perfil_deducido::deducir(&vault())
            .unwrap()
            .is_none()
    );
}

#[test]
fn la_revision_sin_perfil_usa_el_deducido() {
    let v = empresa();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("nota.txt"), b"carpeta de prueba").unwrap();
    let r = revision::revisar(&v, dir.path(), date!(2026 - 11 - 01)).unwrap();
    assert!(
        !r.hallazgos.iter().any(|h| h.codigo == "modelos.sin_perfil"),
        "{:?}",
        r.hallazgos
    );
    let nota = r
        .hallazgos
        .iter()
        .find(|h| h.codigo == "modelos.perfil_deducido")
        .unwrap();
    assert!(nota.detalle.contains("303 trimestral"), "{}", nota.detalle);
    let faltan: Vec<_> = r
        .hallazgos
        .iter()
        .filter(|h| h.codigo == "modelos.faltan")
        .collect();
    let de_2025 = faltan.iter().find(|h| h.ejercicio == Some(2025)).unwrap();
    assert!(de_2025.detalle.contains("303 (2T)"), "{}", de_2025.detalle);
    // En 2022 no hay ningún 303 cerca (ni en 2021 ni en 2023): no se pide.
    assert!(
        !faltan.iter().any(|h| h.ejercicio == Some(2022)),
        "{:?}",
        faltan
    );
}
