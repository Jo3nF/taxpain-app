//! Las peticiones del servicio de notificaciones cumplen los XSD oficiales y los ejemplos
//! sintéticos de respuesta también.

use std::path::PathBuf;

use taxpain_aeat::notifications::{Operation, acceso_request, consulta_request};
use taxpain_testkit::xsd;
use time::macros::date;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aeat/notificaciones")
}

/// Valida y admite sólo una discrepancia conocida de la documentación oficial: `TipoVersion`
/// exige longitud 5 en `TiposGN.xsd`, pero la especificación v1.7 y todos sus ejemplos usan
/// «1.1»/«1.3» (3 caracteres). Se envía lo que dice la especificación. `TODO(VERIFY)` con la AEAT.
fn valid(xsd_name: &str, xml: &str) -> Result<(), Vec<String>> {
    match xsd::validate(&dir().join(xsd_name), xml) {
        Ok(()) => Ok(()),
        Err(errors) => {
            let rest: Vec<String> = errors
                .into_iter()
                .filter(|e| !(e.contains("}Version'") && e.contains("allowed length of '5'")))
                .collect();
            if rest.is_empty() { Ok(()) } else { Err(rest) }
        }
    }
}

/// Extrae el elemento de `Body` como documento suelto.
fn body(envelope: &str) -> String {
    let start = envelope
        .find("<env:Body>")
        .map_or(0, |i| i + "<env:Body>".len());
    let end = envelope.find("</env:Body>").unwrap_or(envelope.len());
    let inner = &envelope[start..end];
    // Declara en el elemento raíz el prefijo que el sobre declaraba.
    let (prefix, xsd_name) = if inner.starts_with("<pc:") {
        ("pc", "PeticionConsulta.xsd")
    } else if inner.starts_with("<pa:") {
        ("pa", "PeticionAcceso.xsd")
    } else if inner.contains("<rc:") {
        ("rc", "RespuestaConsulta.xsd")
    } else {
        ("ra", "RespuestaAcceso.xsd")
    };
    let ns = format!(
        " xmlns:{prefix}=\"https://www2.agenciatributaria.gob.es/static_files/common/dep/aduanas/es/aeat/gnno/jdit/ws/{xsd_name}\""
    );
    let trimmed = inner.trim();
    let close = trimmed.find('>').unwrap_or(0);
    format!(
        "<?xml version=\"1.0\"?>{}{ns}{}",
        &trimmed[..close],
        &trimmed[close..]
    )
}

#[test]
fn peticiones_validas() {
    let c = consulta_request(
        "12345678Z",
        date!(2026 - 07 - 01),
        date!(2026 - 09 - 28),
        "0",
    );
    assert_eq!(valid("PeticionConsulta.xsd", &body(&c)), Ok(()));
    let a = acceso_request("2319486500018", Operation::Comparecer);
    assert_eq!(valid("PeticionAcceso.xsd", &body(&a)), Ok(()));
    // Un NIF mal formado no pasa el esquema.
    let malo = consulta_request("123", date!(2026 - 07 - 01), date!(2026 - 09 - 28), "0");
    assert!(valid("PeticionConsulta.xsd", &body(&malo)).is_err());
}

#[test]
fn ejemplos_de_respuesta_validos() {
    for (f, x) in [
        ("respuesta-consulta-ejemplo.xml", "RespuestaConsulta.xsd"),
        ("respuesta-acceso-ejemplo.xml", "RespuestaAcceso.xsd"),
    ] {
        let xml = std::fs::read_to_string(dir().join(f)).unwrap_or_default();
        assert_eq!(valid(x, &body(&xml)), Ok(()), "{f}");
    }
}
