//! Revisión de una carpeta sin lecturas de IA: las facturas en PDF se leen por reglas. La emitida
//! (con el NIF propio en la cabecera) se reconoce como emitida, y de las recibidas se llevan
//! base, IVA y retención a la revisión (el 111 casa con la retención del asesor). Datos
//! inventados, NIF de prueba.

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::sync::Arc;

use serde_json::json;
use taxpain_app::carpeta::{self, Found};
use taxpain_app::companies::{self, CompanyInfo};
use taxpain_app::revision::{self, Revision};
use taxpain_core::money::Money;
use taxpain_invoicing::pdf::{Para, text_document};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

const PROPIO: &str = "B12345674";

fn vault() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let info = CompanyInfo {
        name: "EMPRESA DEMO SL".into(),
        nif: Some(PROPIO.into()),
    };
    companies::set_info(&v, &info, datetime!(2026-01-01 9:00 UTC)).unwrap();
    v
}

/// Un PDF con una línea por párrafo.
fn pdf(lineas: &[&str]) -> Vec<u8> {
    let paras = lineas.iter().map(|l| Para::Text((*l).into())).collect();
    text_document("Factura", &[paras]).unwrap()
}

fn escribir(root: &Path, ruta: &str, bytes: &[u8]) {
    let p = root.join(ruta);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

/// El 347 de 2026 que genera TaxPain, con lo que declara de CLIENTE UNO SL (clave B).
fn m347(ventas_cliente: i64) -> Vec<u8> {
    use taxpain_models::informative::{self, Key347, Op347};
    let op = Op347 {
        nif: "B98765431".into(),
        name: "CLIENTE UNO SL".into(),
        province: "28".into(),
        key: Key347::B,
        quarter: 3,
        amount: Money::from_euros(ventas_cliente, 0),
        business_rental: false,
        cash: Money::ZERO,
    };
    let r = informative::compute_347(2026, &[op]);
    let who = taxpain_models::Declarant {
        nif: PROPIO.into(),
        name: "EMPRESA DEMO SL".into(),
        given_name: None,
    };
    let f = informative::file_347(&r, &who).unwrap();
    taxpain_models::dr::to_latin1(&f).unwrap()
}

fn carpeta(root: &Path, retencion_111: &str) {
    escribir(
        root,
        "facturas/emitidas/A2026-0001.pdf",
        &pdf(&[
            "EMPRESA DEMO SL",
            "CIF B12345674",
            "Factura nº: A2026-0001",
            "Fecha: 10/07/2026",
            "Cliente: CLIENTE UNO SL NIF B98765431",
            "Base imponible 5.000,00 €",
            "IVA 21 % 1.050,00 €",
            "TOTAL FACTURA 6.050,00 €",
        ]),
    );
    escribir(
        root,
        "facturas/recibidas/AS-55 asesor.pdf",
        &pdf(&[
            "ASESOR PRUEBA",
            "NIF 12345678Z",
            "Factura nº: AS-55",
            "Fecha: 01/09/2026",
            "Cliente: EMPRESA DEMO SL NIF B12345674",
            "Base imponible 1.000,00 €",
            "IVA 21 % 210,00 €",
            "Retención IRPF 15 % -150,00 €",
            "TOTAL FACTURA 1.060,00 €",
        ]),
    );
    // El 111 del 3T, leído con su lectura (las facturas no tienen).
    let lecturas = json!({
        "version": 1,
        "generado_por": "Claude Code",
        "lecturas": [{
            "fichero": "modelos/111-3T.pdf",
            "lectura": {
                "tipo": "modelo_aeat",
                "descripcion": "Modelo 111 3T 2026",
                "modelo": {
                    "modelo": "111", "ejercicio": 2026, "periodo": "3T",
                    "casillas": { "07": "1", "08": "1000.00", "09": retencion_111 }
                }
            }
        }]
    });
    escribir(root, "modelos/111-3T.pdf", &pdf(&["Modelo 111 3T 2026"]));
    std::fs::write(
        root.join("taxpain-lecturas.json"),
        serde_json::to_vec_pretty(&lecturas).unwrap(),
    )
    .unwrap();
}

fn revisar(retencion_111: &str) -> Revision {
    let dir = tempfile::tempdir().unwrap();
    carpeta(dir.path(), retencion_111);
    revision::revisar(&vault(), dir.path(), date!(2026 - 10 - 25)).unwrap()
}

fn hay(r: &Revision, codigo: &str) -> bool {
    r.hallazgos.iter().any(|h| h.codigo == codigo)
}

#[test]
fn la_factura_emitida_en_pdf_se_reconoce_como_emitida() {
    let dir = tempfile::tempdir().unwrap();
    carpeta(dir.path(), "150.00");
    let inv = carpeta::scan(&vault(), dir.path()).unwrap();
    let item = |ruta: &str| inv.items.iter().find(|i| i.path.ends_with(ruta)).unwrap();
    assert!(
        matches!(
            &item("A2026-0001.pdf").found,
            Found::FacturaEmitida { number: Some(n), .. } if n == "A2026-0001"
        ),
        "{:?}",
        item("A2026-0001.pdf").found
    );
    assert!(
        matches!(
            &item("AS-55 asesor.pdf").found,
            Found::FacturaRecibida { nif: Some(n), .. } if n == "12345678Z"
        ),
        "{:?}",
        item("AS-55 asesor.pdf").found
    );
}

#[test]
fn el_111_casa_con_la_retencion_leida_del_pdf() {
    let r = revisar("150.00");
    assert!(!hay(&r, "modelos.111_sin_facturas"), "{:?}", r.hallazgos);
    assert!(!hay(&r, "modelos.111_retenciones"), "{:?}", r.hallazgos);
    // Con otra cifra en el 111, la diferencia se ve frente a los 150 € de la factura.
    let r = revisar("200.00");
    let h = r
        .hallazgos
        .iter()
        .find(|h| h.codigo == "modelos.111_retenciones")
        .unwrap_or_else(|| panic!("{:?}", r.hallazgos));
    assert!(h.detalle.contains("150,00 €"), "{}", h.detalle);
}

#[test]
fn el_347_en_fichero_se_cruza_por_nif() {
    let dir = tempfile::tempdir().unwrap();
    carpeta(dir.path(), "150.00");
    escribir(dir.path(), "modelos/B12345674_2026_347.txt", &m347(6050));
    let inv = carpeta::scan(&vault(), dir.path()).unwrap();
    let it = inv
        .items
        .iter()
        .find(|i| i.path.ends_with("347.txt"))
        .unwrap();
    assert!(
        matches!(&it.found, Found::Modelo { model, boxes: 1, .. } if model == "347"),
        "{:?}",
        it.found
    );
    // Se propone guardarlo para compararlo con el 347 que calcula TaxPain (G37).
    assert!(it.importable);
    let r = revision::revisar(&vault(), dir.path(), date!(2026 - 10 - 25)).unwrap();
    assert!(!hay(&r, "cruces.sin_registros"), "{:?}", r.hallazgos);
    assert!(!hay(&r, "cruces.347_de_menos"), "{:?}", r.hallazgos);
    let t = r
        .tablas
        .iter()
        .find(|t| t.titulo == "347 por NIF frente a las facturas")
        .unwrap();
    assert!(
        t.filas.iter().any(|f| f.contains(&"B98765431".to_owned())),
        "{:?}",
        t.filas
    );

    // Con un importe cambiado (5.000 € en vez de 6.050 €), la diferencia sale.
    escribir(dir.path(), "modelos/B12345674_2026_347.txt", &m347(5000));
    let r = revision::revisar(&vault(), dir.path(), date!(2026 - 10 - 25)).unwrap();
    let h = r
        .hallazgos
        .iter()
        .find(|h| h.codigo == "cruces.347_de_menos")
        .unwrap_or_else(|| panic!("{:?}", r.hallazgos));
    assert!(h.detalle.contains("B98765431"), "{}", h.detalle);
}

#[test]
fn el_349_en_fichero_casa_con_el_303_y_se_cruza_por_nif() {
    use taxpain_models::m349::{self, Key, Operation};
    let dir = tempfile::tempdir().unwrap();
    let r = m349::compute(
        2026,
        "3T",
        &[Operation {
            country: "DE".into(),
            vat_id: "123456788".into(),
            name: "KUNDE GMBH".into(),
            key: Key::S,
            base: Money::from_euros(3000, 0),
        }],
    );
    let who = taxpain_models::Declarant {
        nif: PROPIO.into(),
        name: "EMPRESA DEMO SL".into(),
        given_name: None,
    };
    let f = m349::file(&r, &who, 1).unwrap();
    escribir(
        dir.path(),
        "modelos/B12345674_2026_3T_349.txt",
        &taxpain_models::dr::to_latin1(&f).unwrap(),
    );
    // El 303 del 3T con 3.000 € de entregas a la UE ([59]), leído con su lectura.
    escribir(
        dir.path(),
        "modelos/303-3T.pdf",
        &pdf(&["Modelo 303 3T 2026"]),
    );
    let lecturas = json!({
        "version": 1,
        "generado_por": "Claude Code",
        "lecturas": [{
            "fichero": "modelos/303-3T.pdf",
            "lectura": {
                "tipo": "modelo_aeat",
                "descripcion": "Modelo 303 3T 2026",
                "modelo": {
                    "modelo": "303", "ejercicio": 2026, "periodo": "3T",
                    "casillas": { "59": "3000.00" }
                }
            }
        }]
    });
    std::fs::write(
        dir.path().join("taxpain-lecturas.json"),
        serde_json::to_vec_pretty(&lecturas).unwrap(),
    )
    .unwrap();
    let r = revision::revisar(&vault(), dir.path(), date!(2026 - 10 - 25)).unwrap();
    assert!(!hay(&r, "modelos.349_303"), "{:?}", r.hallazgos);
    assert!(!hay(&r, "cruces.sin_registros"), "{:?}", r.hallazgos);
    let t = r
        .tablas
        .iter()
        .find(|t| t.titulo == "349 por NIF frente a las facturas")
        .unwrap();
    assert!(
        t.filas
            .iter()
            .any(|f| f.contains(&"DE123456788".to_owned())),
        "{:?}",
        t.filas
    );
}
