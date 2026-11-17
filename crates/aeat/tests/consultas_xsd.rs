//! El ejemplo sintético de respuesta de ConsultaExt cumple el esquema oficial `consultas.xsd`.

use std::path::PathBuf;

use taxpain_testkit::xsd;

#[test]
fn ejemplo_valido_contra_el_xsd_oficial() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aeat/presentacion");
    let xml = std::fs::read_to_string(dir.join("consulta-ejemplo.xml")).unwrap_or_default();
    assert!(xsd::validate(&dir.join("consultas.xsd"), &xml).is_ok());
    let malo = xml.replace("ABCDEFGHIJKLMNOP", "CORTO");
    assert!(xsd::validate(&dir.join("consultas.xsd"), &malo).is_err());
}
