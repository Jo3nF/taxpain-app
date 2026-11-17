//! Garantía de arquitectura: el asistente no puede leer el certificado, contraseñas ni claves
//! porque no depende de los crates que los tienen. Si alguien añade la dependencia, este test
//! falla.

#[test]
fn no_depende_del_almacen_ni_de_los_secretos() {
    let manifest = include_str!("../Cargo.toml");
    let deps = manifest
        .split("[dependencies]")
        .nth(1)
        .and_then(|rest| rest.split("\n[").next())
        .unwrap_or_default();
    for forbidden in [
        "taxpain-vault",
        "taxpain-secrets",
        "taxpain-macos",
        "taxpain-aeat",
    ] {
        assert!(
            !deps.contains(forbidden),
            "el asistente no debe depender de {forbidden}"
        );
    }
}
