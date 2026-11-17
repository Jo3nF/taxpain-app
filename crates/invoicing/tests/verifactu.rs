//! VERI*FACTU contra los vectores y esquemas oficiales (`fixtures/aeat/verifactu/`).

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use taxpain_core::invoice::{
    Address, Exemption, Invoice, InvoiceKind, InvoiceLine, Party, PartyId, Rectification,
    RectificationMethod, RoundingMode, VatTreatment, compute_totals,
};
use taxpain_core::money::{Money, Rate};
use taxpain_invoicing::verifactu::{
    self, AltaOptions, QrEnv, SistemaInformatico, VerifactuError, huella_alta, huella_anulacion,
    parse_response, qr_url, qr_url_raw, soap_envelope, submission, submission_with,
};
use taxpain_testkit::xsd;
use time::macros::{date, datetime};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/aeat/verifactu")
        .join(name)
}

fn json(name: &str) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(fixture(name)).unwrap()).unwrap()
}

#[test]
fn vectores_oficiales_de_huella() {
    let v = json("hash-vectors.json");
    let vectors = v["vectors"].as_array().unwrap();
    assert_eq!(vectors.len(), 3);
    for t in vectors {
        let f = &t["fields"];
        let s = |k: &str| f[k].as_str().unwrap();
        let got = match t["type"].as_str().unwrap() {
            "alta" => huella_alta(
                s("IDEmisorFactura"),
                s("NumSerieFactura"),
                s("FechaExpedicionFactura"),
                s("TipoFactura"),
                s("CuotaTotal"),
                s("ImporteTotal"),
                s("Huella"),
                s("FechaHoraHusoGenRegistro"),
            ),
            _ => huella_anulacion(
                s("IDEmisorFacturaAnulada"),
                s("NumSerieFacturaAnulada"),
                s("FechaExpedicionFacturaAnulada"),
                s("Huella"),
                s("FechaHoraHusoGenRegistro"),
            ),
        };
        assert_eq!(got, t["expected"].as_str().unwrap(), "{}", t["id"]);
    }
    // Los espacios al principio y al final de cada valor se eliminan (pág. 6).
    assert_eq!(
        huella_alta(
            " 89890001K ",
            " 12345678/G33 ",
            "01-01-2024",
            "F1",
            "12.35",
            "123.45",
            "",
            "2024-01-01T19:20:30+01:00"
        ),
        "3C464DAF61ACB827C65FDA19F352A4E3BDC2C640E9E9FC4CC058073F38F12F60"
    );
}

#[test]
fn urls_oficiales_del_qr() {
    let v = json("qr-vectors.json");
    for t in v["vectors"].as_array().unwrap() {
        let Some(expected) = t["expected_url"].as_str() else {
            continue;
        };
        if expected.contains("NoVerifactu") {
            continue;
        }
        let p = &t["params"];
        let env = if t["env"] == "prod" {
            QrEnv::Produccion
        } else {
            QrEnv::Pruebas
        };
        let got = qr_url_raw(
            env,
            p["nif"].as_str().unwrap(),
            p["numserie"].as_str().unwrap(),
            p["fecha"].as_str().unwrap(),
            p["importe"].as_str().unwrap(),
        );
        assert_eq!(got, expected, "{}", t["id"]);
    }
    assert_eq!(
        qr_url(
            QrEnv::Pruebas,
            "89890001K",
            "A 2026/1",
            date!(2026 - 10 - 13),
            Money::from_cents(24140)
        ),
        "https://prewww2.aeat.es/wlpl/TIKE-CONT/ValidarQR?nif=89890001K&numserie=A+2026%2F1&fecha=13-10-2026&importe=241.40"
    );
}

fn invoice(number: &str) -> Invoice {
    let mut l = InvoiceLine {
        description: "Servicio".into(),
        quantity_milli: 1000,
        unit_price: Money::from_euros(100, 0),
        discount: Rate::ZERO,
        vat: VatTreatment::Taxed {
            rate: Rate::percent(21),
        },
        surcharge: Some(Rate::from_bp(520)),
        withholding: Some(Rate::percent(15)),
        advance: false,
    };
    let mut ex = l.clone();
    ex.vat = VatTreatment::Exempt(Exemption::E1);
    ex.surcharge = None;
    let mut isp = ex.clone();
    isp.vat = VatTreatment::ReverseCharge;
    l.description = "Servicio <especial> & más".into();
    Invoice {
        kind: InvoiceKind::Completa,
        number: number.into(),
        issue_date: date!(2026 - 10 - 13),
        operation_date: Some(date!(2026 - 10 - 01)),
        issuer: Party {
            id: PartyId::Nif("12345678Z".into()),
            name: "Carmen Española".into(),
            address: None,
            person: None,
        },
        recipient: Some(Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Empresa de Prueba SL".into(),
            address: Some(Address {
                street: "x".into(),
                post_code: "46001".into(),
                town: "València".into(),
                province: "Valencia".into(),
                country: "ES".into(),
            }),
            person: None,
        }),
        lines: vec![l, ex, isp],
        rectified: vec![],
        description: "Servicios profesionales".into(),
        notes: None,
    }
}

#[test]
fn macrodato_en_facturas_de_cien_millones_o_mas() {
    let sif = SistemaInformatico::taxpain("instalacion-de-prueba-0001");
    let at = datetime!(2026-10-13 10:15:00 +02:00);
    let mut f = invoice("A2026-0001");
    f.lines.truncate(1);
    f.lines[0].surcharge = None;
    f.lines[0].withholding = None;
    // 100 M€ de base + 21 % de IVA: ImporteTotal 121.000.000,00 → Macrodato S.
    f.lines[0].unit_price = Money::from_euros(100_000_000, 0);
    let t = compute_totals(&f.lines, RoundingMode::PerRate);
    let r = verifactu::alta(&f, &t, &sif, None, at, &AltaOptions::default()).unwrap();
    assert!(
        r.xml
            .contains("</sf:DescripcionOperacion><sf:Macrodato>S</sf:Macrodato><sf:Destinatarios>"),
        "{}",
        r.xml
    );
    let body = submission("Carmen Española", "12345678Z", &[&r]).unwrap();
    let doc = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>{body}");
    let (_dir, xsd_file) = schema("SuministroLR.xsd");
    if let Err(e) = xsd::validate(&xsd_file, &doc) {
        panic!("registro no válido:\n{}\n\n{doc}", e.join("\n"));
    }
    // Por debajo del umbral no se escribe.
    let f = invoice("A2026-0002");
    let t = compute_totals(&f.lines, RoundingMode::PerRate);
    let r = verifactu::alta(&f, &t, &sif, None, at, &AltaOptions::default()).unwrap();
    assert!(!r.xml.contains("Macrodato"));
}

fn schema(name: &str) -> (xsd::tempfile_dir::Dir, PathBuf) {
    let dir = xsd::local_schema_dir(
        &fixture(""),
        &[(
            "http://www.w3.org/TR/xmldsig-core/xmldsig-core-schema.xsd",
            "xmldsig-core-schema.xsd",
        )],
    );
    let p = dir.path().join(name);
    (dir, p)
}

#[test]
fn cadena_alta_alta_rectificativa_anulacion_valida_contra_el_xsd() {
    let sif = SistemaInformatico::taxpain("instalacion-de-prueba-0001");
    let at = datetime!(2026-10-13 10:15:00 +02:00);
    let f1 = invoice("A2026-0001");
    let t1 = compute_totals(&f1.lines, RoundingMode::PerRate);
    let r1 = verifactu::alta(&f1, &t1, &sif, None, at, &AltaOptions::default()).unwrap();
    assert!(r1.xml.contains("<sf:PrimerRegistro>S</sf:PrimerRegistro>"));
    // La huella del registro es la del algoritmo oficial con sus propios campos.
    assert_eq!(
        r1.huella,
        huella_alta(
            "12345678Z",
            "A2026-0001",
            "13-10-2026",
            "F1",
            &verifactu::amount(t1.tax_total),
            &verifactu::amount(t1.invoice_total),
            "",
            "2026-10-13T10:15:00+02:00"
        )
    );

    let f2 = invoice("A2026-0002");
    let r2 = verifactu::alta(&f2, &t1, &sif, Some(&r1.link), at, &AltaOptions::default()).unwrap();
    assert!(r2.xml.contains(&format!(
        "<sf:Huella>{}</sf:Huella></sf:RegistroAnterior>",
        r1.huella
    )));

    let mut f3 = invoice("R2026-0001");
    f3.kind = InvoiceKind::Rectificativa(Rectification {
        code: 1,
        method: RectificationMethod::Sustitucion,
    });
    f3.rectified = vec![("A2026-0001".into(), date!(2026 - 10 - 13))];
    assert_eq!(
        verifactu::alta(&f3, &t1, &sif, Some(&r2.link), at, &AltaOptions::default()),
        Err(VerifactuError::MissingRectifiedAmount)
    );
    let opts = AltaOptions {
        importe_rectificacion: Some((t1.base, t1.tax_total)),
        ..AltaOptions::default()
    };
    let r3 = verifactu::alta(&f3, &t1, &sif, Some(&r2.link), at, &opts).unwrap();
    assert!(
        r3.xml
            .contains("<sf:TipoRectificativa>S</sf:TipoRectificativa>")
    );
    assert!(!r3.xml.contains("CuotaRecargoRectificado"));
    // Con recargo de equivalencia en lo sustituido, su cuota va aparte.
    let con_recargo = verifactu::alta(
        &f3,
        &t1,
        &sif,
        Some(&r2.link),
        at,
        &AltaOptions {
            recargo_rectificado: Some(Money::from_cents(520)),
            ..opts.clone()
        },
    )
    .unwrap();
    assert!(con_recargo.xml.contains(
        "</sf:CuotaRectificada><sf:CuotaRecargoRectificado>5.20</sf:CuotaRecargoRectificado>\
         </sf:ImporteRectificacion>"
    ));
    let r4 = verifactu::anulacion(
        "12345678Z",
        "A2026-0002",
        date!(2026 - 10 - 13),
        &sif,
        Some(&r3.link),
        at,
        false,
    )
    .unwrap();
    // Reenvíos tras un rechazo: alta (S, X) y anulación (RechazoPrevio S).
    let r5 = verifactu::alta(
        &invoice("A2026-0001"),
        &t1,
        &sif,
        Some(&r4.link),
        at,
        &AltaOptions {
            subsanacion: true,
            rechazo_previo: Some(verifactu::RechazoPrevio::X),
            ..AltaOptions::default()
        },
    )
    .unwrap();
    assert!(
        r5.xml
            .contains("<sf:Subsanacion>S</sf:Subsanacion><sf:RechazoPrevio>X</sf:RechazoPrevio>")
    );
    let r6 = verifactu::anulacion(
        "12345678Z",
        "A2026-0002",
        date!(2026 - 10 - 13),
        &sif,
        Some(&r5.link),
        at,
        true,
    )
    .unwrap();
    assert!(r6.xml.contains("<sf:RechazoPrevio>S</sf:RechazoPrevio>"));

    let body = submission(
        "Carmen Española",
        "12345678Z",
        &[&r1, &r2, &r3, &r4, &r5, &r6],
    )
    .unwrap();
    let doc = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>{body}");
    let (_dir, xsd_file) = schema("SuministroLR.xsd");
    if let Err(e) = xsd::validate(&xsd_file, &doc) {
        panic!("registro no válido:\n{}\n\n{doc}", e.join("\n"));
    }
    let env = soap_envelope(&body);
    assert!(
        env.starts_with("<?xml") && env.contains("<soapenv:Body><sfLR:RegFactuSistemaFacturacion")
    );
}

#[test]
fn respuesta_de_la_aeat() {
    let ns_r = "https://www2.agenciatributaria.gob.es/static_files/common/internet/dep/aplicaciones/es/aeat/tike/cont/ws/RespuestaSuministro.xsd";
    let ns_sf = verifactu::NS_SF;
    let body = format!(
        "<tikR:RespuestaRegFactuSistemaFacturacion xmlns:tikR=\"{ns_r}\" xmlns:tik=\"{ns_sf}\">\
         <tikR:CSV>A-ABCDEFGHIJKLMN</tikR:CSV>\
         <tikR:DatosPresentacion><tik:NIFPresentador>12345678Z</tik:NIFPresentador>\
         <tik:TimestampPresentacion>2026-10-13T10:15:05+02:00</tik:TimestampPresentacion>\
         </tikR:DatosPresentacion>\
         <tikR:Cabecera><tik:ObligadoEmision><tik:NombreRazon>Carmen</tik:NombreRazon>\
         <tik:NIF>12345678Z</tik:NIF></tik:ObligadoEmision></tikR:Cabecera>\
         <tikR:TiempoEsperaEnvio>60</tikR:TiempoEsperaEnvio>\
         <tikR:EstadoEnvio>ParcialmenteCorrecto</tikR:EstadoEnvio>\
         <tikR:RespuestaLinea><tikR:IDFactura><tik:IDEmisorFactura>12345678Z</tik:IDEmisorFactura>\
         <tik:NumSerieFactura>A2026-0001</tik:NumSerieFactura>\
         <tik:FechaExpedicionFactura>13-10-2026</tik:FechaExpedicionFactura></tikR:IDFactura>\
         <tikR:Operacion><tik:TipoOperacion>Alta</tik:TipoOperacion></tikR:Operacion>\
         <tikR:EstadoRegistro>Correcto</tikR:EstadoRegistro></tikR:RespuestaLinea>\
         <tikR:RespuestaLinea><tikR:IDFactura><tik:IDEmisorFactura>12345678Z</tik:IDEmisorFactura>\
         <tik:NumSerieFactura>A2026-0002</tik:NumSerieFactura>\
         <tik:FechaExpedicionFactura>13-10-2026</tik:FechaExpedicionFactura></tikR:IDFactura>\
         <tikR:Operacion><tik:TipoOperacion>Alta</tik:TipoOperacion></tikR:Operacion>\
         <tikR:EstadoRegistro>Incorrecto</tikR:EstadoRegistro>\
         <tikR:CodigoErrorRegistro>1100</tikR:CodigoErrorRegistro>\
         <tikR:DescripcionErrorRegistro>Valor o tipo incorrecto &amp; otro</tikR:DescripcionErrorRegistro>\
         </tikR:RespuestaLinea></tikR:RespuestaRegFactuSistemaFacturacion>"
    );
    // El ejemplo sintético debe ser una respuesta válida según el esquema oficial.
    let (_dir, xsd_file) = schema("RespuestaSuministro.xsd");
    if let Err(e) = xsd::validate(&xsd_file, &format!("<?xml version=\"1.0\"?>{body}")) {
        panic!("respuesta de ejemplo no válida:\n{}", e.join("\n"));
    }
    let r = parse_response(&format!(
        "<env:Envelope xmlns:env=\"http://schemas.xmlsoap.org/soap/envelope/\"><env:Body>{body}</env:Body></env:Envelope>"
    ))
    .unwrap();
    assert_eq!(r.csv.as_deref(), Some("A-ABCDEFGHIJKLMN"));
    assert_eq!(r.estado_envio, "ParcialmenteCorrecto");
    assert_eq!(r.tiempo_espera_envio, 60);
    assert_eq!(r.lineas.len(), 2);
    assert_eq!(
        (r.lineas[0].num_serie.as_str(), r.lineas[0].estado.as_str()),
        ("A2026-0001", "Correcto")
    );
    assert_eq!(r.lineas[1].codigo_error.as_deref(), Some("1100"));
    assert_eq!(
        r.lineas[1].descripcion_error.as_deref(),
        Some("Valor o tipo incorrecto & otro")
    );

    let fault = "<env:Envelope xmlns:env=\"http://schemas.xmlsoap.org/soap/envelope/\"><env:Body><env:Fault>\
                 <faultcode>env:Client</faultcode><faultstring>Codigo[4102].El XML no cumple el esquema</faultstring>\
                 </env:Fault></env:Body></env:Envelope>";
    assert!(
        matches!(parse_response(fault), Err(VerifactuError::InvalidResponse(m)) if m.contains("4102"))
    );
}

#[test]
fn rechaza_series_no_admitidas() {
    let sif = SistemaInformatico::taxpain("x");
    let f = invoice("A<1>");
    let t = compute_totals(&f.lines, RoundingMode::PerRate);
    assert_eq!(
        verifactu::alta(
            &f,
            &t,
            &sif,
            None,
            datetime!(2026-10-13 10:00 UTC),
            &AltaOptions::default()
        ),
        Err(VerifactuError::InvalidSeriesNumber)
    );
}

#[test]
fn consulta_valida_contra_el_xsd_y_su_respuesta() {
    let prev = verifactu::ConsultaRegistro {
        num_serie: "PRU2026-0003".into(),
        fecha: "13-10-2026".into(),
        tipo_factura: "F1".into(),
        importe_total: None,
        huella: None,
        estado: "Correcto".into(),
        codigo_error: None,
        descripcion_error: None,
    };
    for body in [
        verifactu::consulta("Carmen Española", "12345678Z", 2026, 10, None, None),
        verifactu::consulta(
            "Carmen Española",
            "12345678Z",
            2026,
            10,
            Some("PRU2026-0001"),
            Some(&prev),
        ),
    ] {
        let doc = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>{body}");
        let (_dir, xsd_file) = schema("ConsultaLR.xsd");
        if let Err(e) = xsd::validate(&xsd_file, &doc) {
            panic!("consulta no válida:\n{}\n\n{doc}", e.join("\n"));
        }
    }

    let ns_r = "https://www2.agenciatributaria.gob.es/static_files/common/internet/dep/aplicaciones/es/aeat/tike/cont/ws/RespuestaConsultaLR.xsd";
    let ns_sf = verifactu::NS_SF;
    let xml = format!(
        "<env:Envelope xmlns:env=\"http://schemas.xmlsoap.org/soap/envelope/\"><env:Body>\
         <tikR:RespuestaConsultaFactuSistemaFacturacion xmlns:tikR=\"{ns_r}\" xmlns:tik=\"{ns_sf}\">\
         <tikR:Cabecera><tik:IDVersion>1.0</tik:IDVersion><tik:ObligadoEmision><tik:NombreRazon>C</tik:NombreRazon>\
         <tik:NIF>12345678Z</tik:NIF></tik:ObligadoEmision></tikR:Cabecera>\
         <tikR:PeriodoImputacion><tikR:Ejercicio>2026</tikR:Ejercicio><tikR:Periodo>10</tikR:Periodo></tikR:PeriodoImputacion>\
         <tikR:IndicadorPaginacion>N</tikR:IndicadorPaginacion><tikR:ResultadoConsulta>ConDatos</tikR:ResultadoConsulta>\
         <tikR:RegistroRespuestaConsultaFactuSistemaFacturacion>\
         <tikR:IDFactura><tik:IDEmisorFactura>12345678Z</tik:IDEmisorFactura>\
         <tik:NumSerieFactura>PRU2026-0004</tik:NumSerieFactura><tik:FechaExpedicionFactura>13-10-2026</tik:FechaExpedicionFactura></tikR:IDFactura>\
         <tikR:DatosRegistroFacturacion><tikR:TipoFactura>R1</tikR:TipoFactura>\
         <tikR:FacturasRectificadas><tik:IDFacturaRectificada><tik:IDEmisorFactura>12345678Z</tik:IDEmisorFactura>\
         <tik:NumSerieFactura>PRU2026-0001</tik:NumSerieFactura><tik:FechaExpedicionFactura>12-10-2026</tik:FechaExpedicionFactura>\
         </tik:IDFacturaRectificada></tikR:FacturasRectificadas>\
         <tikR:ImporteTotal>-121.00</tikR:ImporteTotal><tikR:Huella>ABC</tikR:Huella></tikR:DatosRegistroFacturacion>\
         <tikR:EstadoRegistro><tikR:TimestampUltimaModificacion>2026-10-13T10:15:05+02:00</tikR:TimestampUltimaModificacion>\
         <tikR:EstadoRegistro>AceptadoConErrores</tikR:EstadoRegistro><tikR:CodigoErrorRegistro>2000</tikR:CodigoErrorRegistro>\
         <tikR:DescripcionErrorRegistro>Huella incorrecta</tikR:DescripcionErrorRegistro></tikR:EstadoRegistro>\
         </tikR:RegistroRespuestaConsultaFactuSistemaFacturacion>\
         </tikR:RespuestaConsultaFactuSistemaFacturacion></env:Body></env:Envelope>"
    );
    let r = verifactu::parse_consulta(&xml).unwrap();
    assert_eq!(r.resultado, "ConDatos");
    assert!(!r.mas);
    assert_eq!(r.registros.len(), 1);
    let g = &r.registros[0];
    assert_eq!(
        (g.num_serie.as_str(), g.fecha.as_str()),
        ("PRU2026-0004", "13-10-2026")
    );
    assert_eq!(g.tipo_factura, "R1");
    assert_eq!(g.importe_total.as_deref(), Some("-121.00"));
    assert_eq!(g.estado, "AceptadoConErrores");
    assert_eq!(g.codigo_error.as_deref(), Some("2000"));
    assert!(verifactu::parse_consulta("<x><faultstring>mal</faultstring></x>").is_err());
}

/// Tras un corte, el reenvío lleva `RemisionVoluntaria/Incidencia = S` en la cabecera (Orden
/// HAC/1177/2024 art. 16.4) y sigue cumpliendo el esquema.
#[test]
fn cabecera_con_incidencia_cumple_el_esquema() {
    let sif = SistemaInformatico::taxpain("instalacion-de-prueba-0001");
    let at = datetime!(2026-10-13 10:15:00 +02:00);
    let f = invoice("A2026-0001");
    let t = compute_totals(&f.lines, RoundingMode::PerRate);
    let r = verifactu::alta(&f, &t, &sif, None, at, &AltaOptions::default()).unwrap();
    let body = submission_with("Carmen Española", "12345678Z", &[&r], true).unwrap();
    assert!(body.contains(
        "</sf:ObligadoEmision><sf:RemisionVoluntaria><sf:Incidencia>S</sf:Incidencia></sf:RemisionVoluntaria></sfLR:Cabecera>"
    ));
    let (_dir, xsd_file) = schema("SuministroLR.xsd");
    let doc = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>{body}");
    if let Err(e) = xsd::validate(&xsd_file, &doc) {
        panic!("envío con incidencia no válido:\n{}\n\n{doc}", e.join("\n"));
    }
    // Sin incidencia, la cabecera no lleva el bloque.
    let normal = submission("Carmen Española", "12345678Z", &[&r]).unwrap();
    assert!(!normal.contains("RemisionVoluntaria"));
}

/// Un registro que la AEAT ya tenía vuelve como rechazado con `RegistroDuplicado` y el estado
/// con que lo tiene: se lee para darlo por remitido.
#[test]
fn lee_el_registro_duplicado_y_el_tipo_de_fault() {
    let ns_r = "https://www2.agenciatributaria.gob.es/static_files/common/internet/dep/aplicaciones/es/aeat/tike/cont/ws/RespuestaSuministro.xsd";
    let ns_sf = verifactu::NS_SF;
    let body = format!(
        "<tikR:RespuestaRegFactuSistemaFacturacion xmlns:tikR=\"{ns_r}\" xmlns:tik=\"{ns_sf}\">\
         <tikR:Cabecera><tik:ObligadoEmision><tik:NombreRazon>Carmen</tik:NombreRazon>\
         <tik:NIF>12345678Z</tik:NIF></tik:ObligadoEmision></tikR:Cabecera>\
         <tikR:TiempoEsperaEnvio>120</tikR:TiempoEsperaEnvio>\
         <tikR:EstadoEnvio>Incorrecto</tikR:EstadoEnvio>\
         <tikR:RespuestaLinea><tikR:IDFactura><tik:IDEmisorFactura>12345678Z</tik:IDEmisorFactura>\
         <tik:NumSerieFactura>A2026-0001</tik:NumSerieFactura>\
         <tik:FechaExpedicionFactura>13-10-2026</tik:FechaExpedicionFactura></tikR:IDFactura>\
         <tikR:Operacion><tik:TipoOperacion>Alta</tik:TipoOperacion></tikR:Operacion>\
         <tikR:EstadoRegistro>Incorrecto</tikR:EstadoRegistro>\
         <tikR:CodigoErrorRegistro>3000</tikR:CodigoErrorRegistro>\
         <tikR:DescripcionErrorRegistro>Registro de facturacion duplicado</tikR:DescripcionErrorRegistro>\
         <tikR:RegistroDuplicado><tik:IdPeticionRegistroDuplicado>202610131015</tik:IdPeticionRegistroDuplicado>\
         <tik:EstadoRegistroDuplicado>Correcta</tik:EstadoRegistroDuplicado></tikR:RegistroDuplicado>\
         </tikR:RespuestaLinea></tikR:RespuestaRegFactuSistemaFacturacion>"
    );
    let (_dir, xsd_file) = schema("RespuestaSuministro.xsd");
    if let Err(e) = xsd::validate(&xsd_file, &format!("<?xml version=\"1.0\"?>{body}")) {
        panic!("respuesta de ejemplo no válida:\n{}", e.join("\n"));
    }
    let r = parse_response(&format!(
        "<env:Envelope xmlns:env=\"http://schemas.xmlsoap.org/soap/envelope/\"><env:Body>{body}</env:Body></env:Envelope>"
    ))
    .unwrap();
    assert_eq!(r.tiempo_espera_envio, 120);
    assert_eq!(r.lineas[0].duplicado.as_deref(), Some("Correcta"));
    assert_eq!(r.lineas[0].codigo_error.as_deref(), Some("3000"));

    let fault = |code: &str| {
        format!(
            "<env:Envelope xmlns:env=\"http://schemas.xmlsoap.org/soap/envelope/\"><env:Body><env:Fault>\
             <faultcode>{code}</faultcode><faultstring>Codigo[4102].El XML no cumple el esquema</faultstring>\
             </env:Fault></env:Body></env:Envelope>"
        )
    };
    let Err(VerifactuError::InvalidResponse(m)) = parse_response(&fault("env:Client")) else {
        panic!("se esperaba un fault");
    };
    assert!(verifactu::fault_is_client(&m), "{m}");
    let Err(VerifactuError::InvalidResponse(m)) = parse_response(&fault("soapenv:Server")) else {
        panic!("se esperaba un fault");
    };
    assert!(!verifactu::fault_is_client(&m), "{m}");
    assert!(!verifactu::fault_is_client("sin red"));
}
