//! Revisión de la carpeta: los extractos del banco (meses sin extracto, saldos que no encadenan
//! de un extracto al siguiente y extractos que no cuadran). Datos inventados.

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::sync::Arc;

use taxpain_app::companies::{self, CompanyInfo};
use taxpain_app::revision::{self, Revision};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

fn vault() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let info = CompanyInfo {
        name: "Empresa Demo SL".into(),
        nif: Some("B98765431".into()),
    };
    companies::set_info(&v, &info, datetime!(2026-01-01 9:00 UTC)).unwrap();
    v
}

/// Un extracto MT940 de la cuenta de ejemplo con un saldo inicial, un movimiento y un saldo
/// final.
fn mt940(inicial: (&str, &str), movimiento: &str, final_: (&str, &str)) -> Vec<u8> {
    format!(
        "{{1:F01DEMOESMMAXXX0000000000}}{{4:\n:20:DEMO\n:25:ES9121000418450200051332\n\
         :28C:1/1\n:60F:C{}EUR{}\n:61:{movimiento}\n:86:MOVIMIENTO DE PRUEBA\n:62F:C{}EUR{}\n-}}",
        inicial.0, inicial.1, final_.0, final_.1
    )
    .into_bytes()
}

fn escribir(root: &Path, ruta: &str, bytes: &[u8]) {
    let p = root.join(ruta);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

fn revisar() -> Revision {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    // Enero cuadra y acaba con 900,00; febrero sale de 850,00 (faltan 50,00 entre los dos);
    // marzo no está (y el saldo pasa de 900,00 a 1.200,00); abril no cuadra (1.200 − 10 no son
    // 800).
    escribir(
        root,
        "2025/T1/banco-enero.sta",
        &mt940(
            ("250101", "1000,00"),
            "250105D100,00NTRFNONREF",
            ("250131", "900,00"),
        ),
    );
    escribir(
        root,
        "2025/T1/banco-febrero.sta",
        &mt940(
            ("250201", "850,00"),
            "250210C50,00NTRFNONREF",
            ("250228", "900,00"),
        ),
    );
    escribir(
        root,
        "2025/T2/banco-abril.sta",
        &mt940(
            ("250401", "1200,00"),
            "250405D10,00NTRFNONREF",
            ("250430", "800,00"),
        ),
    );
    revision::revisar(&vault(), root, date!(2026 - 10 - 25)).unwrap()
}

#[test]
fn extractos_del_banco() {
    let r = revisar();
    let codigos: Vec<&str> = r.hallazgos.iter().map(|h| h.codigo.as_str()).collect();

    let tabla = r
        .tablas
        .iter()
        .find(|t| t.titulo == "Extractos del banco")
        .unwrap();
    assert_eq!(tabla.filas.len(), 3, "{:?}", tabla.filas);
    assert!(
        tabla.filas[0][0].contains("…1332 EUR"),
        "{:?}",
        tabla.filas[0]
    );

    let falta = r
        .hallazgos
        .iter()
        .find(|h| h.codigo == "bancos.meses_sin_extracto")
        .unwrap_or_else(|| panic!("{codigos:?}"));
    assert!(
        falta
            .detalle
            .contains("marzo de 2025 (saldo de 900,00 € a 1.200,00 €)"),
        "{}",
        falta.detalle
    );
    assert!(r.pedir.iter().any(|p| p.que.contains("marzo de 2025")));

    let encadena = r
        .hallazgos
        .iter()
        .find(|h| h.codigo == "bancos.saldos_no_encadenan")
        .unwrap_or_else(|| panic!("{codigos:?}"));
    assert!(encadena.detalle.contains("-50,00"), "{}", encadena.detalle);
    assert_eq!(encadena.ficheros.len(), 2);

    let no_cuadra = r
        .hallazgos
        .iter()
        .find(|h| h.codigo == "bancos.no_cuadran")
        .unwrap_or_else(|| panic!("{codigos:?}"));
    assert_eq!(no_cuadra.ficheros.len(), 1);
    assert!(no_cuadra.ficheros[0].contains("banco-abril"));
}
