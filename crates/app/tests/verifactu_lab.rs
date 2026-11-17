//! Banco de pruebas VERI*FACTU: batería de facturas, anulación, envío válido contra el XSD
//! oficial, respuesta de la AEAT y comparación con su consulta. Sin red.

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::Arc;

use taxpain_app::{invoicing, verifactu_lab as lab};
use taxpain_core::invoice::{Party, PartyId};
use taxpain_invoicing::verifactu::{self, ConsultaRegistro};
use taxpain_testkit::xsd;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/aeat/verifactu")
        .join(name)
}

#[test]
fn bateria_anulacion_envio_y_consulta() {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let now = datetime!(2026-10-26 10:00 UTC);
    // Fuera de la empresa de pruebas no se generan.
    assert!(lab::generate(&v, date!(2026 - 10 - 26), now).is_err());

    let issuer = Party {
        id: PartyId::Nif("B12345674".into()),
        name: "EJEMPLO PRUEBAS SL".into(),
        address: addr(),
        person: None,
    };
    invoicing::save_settings(&v, &lab::settings_for(issuer), now).unwrap();
    lab::mark(&v, now).unwrap();

    let checks = lab::preflight(&v, None, now, 10, time::Weekday::Monday).unwrap();
    let get = |id: &str| checks.iter().find(|c| c.id == id).unwrap().ok;
    assert!(get("empresa") && get("modo") && get("horario"));
    assert!(!get("certificado"), "sin certificado no se puede enviar");
    let sat = lab::preflight(&v, None, now, 10, time::Weekday::Saturday).unwrap();
    assert!(!sat.iter().find(|c| c.id == "horario").unwrap().ok);

    let numbers = lab::generate(&v, date!(2026 - 10 - 26), now).unwrap();
    assert_eq!(numbers.len(), 4);
    assert!(numbers[0].starts_with("PRU"));
    assert!(numbers[3].starts_with("PRUR"), "{numbers:?}");

    // Primer envío: las 4 altas, válido según el esquema de la AEAT.
    let (ids, body) = invoicing::pending_submission(&v, 1000, invoicing::Entorno::Pruebas)
        .unwrap()
        .unwrap();
    assert_eq!(ids.len(), 4);
    let dir = xsd::local_schema_dir(
        &fixture(""),
        &[(
            "http://www.w3.org/TR/xmldsig-core/xmldsig-core-schema.xsd",
            "xmldsig-core-schema.xsd",
        )],
    );
    let doc = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>{body}");
    if let Err(e) = xsd::validate(&dir.path().join("SuministroLR.xsd"), &doc) {
        panic!("envío no válido:\n{}", e.join("\n"));
    }

    // La AEAT acepta todo.
    let lines: String = numbers
        .iter()
        .map(|n| {
            format!(
                "<tikR:RespuestaLinea><tikR:IDFactura><tik:IDEmisorFactura>B12345674</tik:IDEmisorFactura>\
                 <tik:NumSerieFactura>{n}</tik:NumSerieFactura><tik:FechaExpedicionFactura>26-10-2026</tik:FechaExpedicionFactura>\
                 </tikR:IDFactura><tikR:Operacion><tik:TipoOperacion>Alta</tik:TipoOperacion></tikR:Operacion>\
                 <tikR:EstadoRegistro>Correcto</tikR:EstadoRegistro></tikR:RespuestaLinea>"
            )
        })
        .collect();
    let respond = |csv: &str, lines: &str| {
        format!(
            "<tikR:RespuestaRegFactuSistemaFacturacion xmlns:tikR=\"r\" xmlns:tik=\"s\"><tikR:CSV>{csv}</tikR:CSV>\
             <tikR:TiempoEsperaEnvio>60</tikR:TiempoEsperaEnvio><tikR:EstadoEnvio>Correcto</tikR:EstadoEnvio>{lines}\
             </tikR:RespuestaRegFactuSistemaFacturacion>"
        )
    };
    let resp = verifactu::parse_response(&respond("A-TEST", &lines)).unwrap();
    invoicing::apply_submission_response(&v, &ids, &resp, invoicing::Entorno::Pruebas, now)
        .unwrap();

    // Segundo envío: la anulación de la segunda, también válida según el XSD.
    let cancelled = lab::cancel_one(&v, now).unwrap();
    assert_eq!(cancelled, numbers[1]);
    let (ids, body) = invoicing::pending_submission(&v, 1000, invoicing::Entorno::Pruebas)
        .unwrap()
        .unwrap();
    assert_eq!(ids.len(), 1);
    let doc = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>{body}");
    if let Err(e) = xsd::validate(&dir.path().join("SuministroLR.xsd"), &doc) {
        panic!("anulación no válida:\n{}", e.join("\n"));
    }
    let line = format!(
        "<tikR:RespuestaLinea><tikR:IDFactura><tik:IDEmisorFactura>B12345674</tik:IDEmisorFactura>\
         <tik:NumSerieFactura>{}</tik:NumSerieFactura><tik:FechaExpedicionFactura>26-10-2026</tik:FechaExpedicionFactura>\
         </tikR:IDFactura><tikR:Operacion><tik:TipoOperacion>Anulacion</tik:TipoOperacion></tikR:Operacion>\
         <tikR:EstadoRegistro>Correcto</tikR:EstadoRegistro></tikR:RespuestaLinea>",
        numbers[1]
    );
    let resp = verifactu::parse_response(&respond("A-TEST2", &line)).unwrap();
    invoicing::apply_submission_response(&v, &ids, &resp, invoicing::Entorno::Pruebas, now)
        .unwrap();
    assert!(
        lab::records(&v)
            .unwrap()
            .iter()
            .all(|r| r.send_status == "pruebas")
    );

    // La consulta: la huella de la primera no coincide (lo que se vería si la cadena estuviera mal).
    let huella_1: String = v
        .connection()
        .query_row(
            "SELECT huella FROM verifactu_record WHERE num_serie = ?1",
            [&numbers[0]],
            |r| r.get(0),
        )
        .unwrap();
    let reg = |n: &str, estado: &str, huella: Option<String>| ConsultaRegistro {
        num_serie: n.into(),
        fecha: "26-10-2026".into(),
        tipo_factura: "F1".into(),
        importe_total: None,
        huella,
        estado: estado.into(),
        codigo_error: None,
        descripcion_error: None,
    };
    let regs = vec![
        reg(&numbers[0], "Correcto", Some(huella_1.to_lowercase())),
        reg(&numbers[1], "Anulado", None),
        reg(&numbers[2], "Correcto", Some("MAL".into())),
    ];
    // Consultar otro mes no da por fallidas las facturas de octubre.
    assert!(lab::compare(&v, &[], 2026, 9).unwrap().is_empty());
    let c = lab::compare(&v, &regs, 2026, 10).unwrap();
    let by = |n: &str| c.iter().find(|x| x.num_serie == n).unwrap();
    assert!(by(&numbers[0]).ok && by(&numbers[0]).huella_ok == Some(true));
    assert!(
        by(&numbers[1]).ok,
        "la anulada sale como Anulado: {:?}",
        by(&numbers[1])
    );
    assert!(!by(&numbers[2]).ok && by(&numbers[2]).huella_ok == Some(false));
    assert!(
        !by(&numbers[3]).ok && by(&numbers[3]).aeat_status.is_none(),
        "no está en la AEAT"
    );
    lab::save_query(&v, &c).unwrap();

    let md = lab::report_markdown(&v, &checks, now).unwrap();
    assert!(
        md.contains("A-TEST") && md.contains("NO coincide") && md.contains(&numbers[3]),
        "{md}"
    );
}

/// Domicilio inventado: la factura completa lo exige (art. 6.1.e RD 1619/2012).
fn addr() -> Option<taxpain_core::invoice::Address> {
    Some(taxpain_core::invoice::Address {
        street: "Calle Inventada 1".into(),
        post_code: "46001".into(),
        town: "València".into(),
        province: "Valencia".into(),
        country: "ES".into(),
    })
}
