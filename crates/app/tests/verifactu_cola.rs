//! Cola de remisión VERI*FACTU contra un entorno de pruebas de la AEAT simulado (sin red): el
//! simulador lee el cuerpo que se enviaría y contesta con el XML de `RespuestaSuministro`, que
//! se interpreta con el mismo `parse_response` que la app.

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::Arc;

use taxpain_app::invoicing::{self, Entorno, InvoiceDraft, issue, list_issued};
use taxpain_app::settings::{
    Destination, DestinationKind, InvoicingSettings, Language, Outputs, VerifactuSettings,
    new_installation_id,
};
use taxpain_app::verifactu_cola::{self, Siguiente};
use taxpain_core::invoice::{
    Address, InvoiceKind, InvoiceLine, Party, PartyId, PersonName, RoundingMode, VatTreatment,
};
use taxpain_core::money::{Money, Rate};
use taxpain_invoicing::verifactu::{self, Response};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Duration, OffsetDateTime};

fn addr() -> Option<Address> {
    Some(Address {
        street: "Calle Mayor 1".into(),
        post_code: "46001".into(),
        town: "València".into(),
        province: "Valencia".into(),
        country: "ES".into(),
    })
}

fn settings(dest: PathBuf) -> InvoicingSettings {
    InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Estudio Demo SL".into(),
            address: addr(),
            person: None,
        },
        default_series: "A".into(),
        rectifying_series: "R".into(),
        simplified_series: "S".into(),
        simplified_rectifying_series: "RS".into(),
        language: Language::Es,
        default_vat: Rate::percent(21),
        default_withholding: None,
        outputs: Outputs {
            pdf: true,
            facturae: false,
            ubl: false,
        },
        destination: Some(Destination {
            kind: DestinationKind::Local,
            path: dest,
        }),
        rounding: RoundingMode::PerRate,
        verifactu: VerifactuSettings {
            enabled: true,
            installation_id: new_installation_id(),
            production: false,
        },
        footer: None,
        activity: None,
        simplified_up_to_3000: false,
    }
}

fn draft(amount_euros: i64) -> InvoiceDraft {
    InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: date!(2026 - 10 - 13),
        operation_date: None,
        recipient: Some(Party {
            id: PartyId::Nif("12345678Z".into()),
            name: "Carmen Española Española".into(),
            address: addr(),
            person: Some(PersonName {
                name: "Carmen".into(),
                first_surname: "Española".into(),
                second_surname: Some("Española".into()),
            }),
        }),
        lines: vec![InvoiceLine {
            description: "Consultoría".into(),
            quantity_milli: 1000,
            unit_price: Money::from_euros(amount_euros, 0),
            discount: Rate::ZERO,
            vat: VatTreatment::Taxed {
                rate: Rate::percent(21),
            },
            surcharge: None,
            withholding: None,
            advance: false,
        }],
        rectified: vec![],
        rectified_amounts: None,
        description: "Servicios de consultoría".into(),
        notes: None,
        currency: None,
        oss: None,
    }
}

fn vault() -> Vault {
    Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap()
}

const T0: OffsetDateTime = datetime!(2026-10-13 10:00 +02:00);

fn setup() -> (Vault, tempfile::TempDir) {
    let v = vault();
    let dest = tempfile::tempdir().unwrap();
    invoicing::save_settings(&v, &settings(dest.path().into()), T0).unwrap();
    (v, dest)
}

/// Cómo contesta el simulador a cada registro del envío.
#[derive(Clone, Copy)]
enum Linea {
    Correcto,
    /// Rechazado por duplicado: la AEAT ya lo tiene como `Correcta`.
    Duplicado,
    /// Sin línea en la respuesta.
    Omitida,
}

/// Entorno de pruebas simulado: saca del cuerpo los números de factura y el tipo de cada
/// registro y responde con `TiempoEsperaEnvio = espera`.
fn aeat(body: &str, espera: u32, linea: Linea) -> Response {
    assert!(body.starts_with("<sfLR:RegFactuSistemaFacturacion"));
    let ns_r = "https://www2.agenciatributaria.gob.es/static_files/common/internet/dep/aplicaciones/es/aeat/tike/cont/ws/RespuestaSuministro.xsd";
    let ns_sf = verifactu::NS_SF;
    let mut lineas = String::new();
    for reg in body.split("<sfLR:RegistroFactura>").skip(1) {
        let (op, tag) = if reg.contains("<sf:RegistroAnulacion>") {
            ("Anulacion", "NumSerieFacturaAnulada")
        } else {
            ("Alta", "NumSerieFactura")
        };
        let num = reg
            .split(&format!("<sf:{tag}>"))
            .nth(1)
            .and_then(|r| r.split('<').next())
            .unwrap();
        let estado = match linea {
            Linea::Correcto => "<tikR:EstadoRegistro>Correcto</tikR:EstadoRegistro>".to_owned(),
            Linea::Duplicado => "<tikR:EstadoRegistro>Incorrecto</tikR:EstadoRegistro>\
                <tikR:CodigoErrorRegistro>3000</tikR:CodigoErrorRegistro>\
                <tikR:DescripcionErrorRegistro>Registro duplicado</tikR:DescripcionErrorRegistro>\
                <tikR:RegistroDuplicado><tik:IdPeticionRegistroDuplicado>1</tik:IdPeticionRegistroDuplicado>\
                <tik:EstadoRegistroDuplicado>Correcta</tik:EstadoRegistroDuplicado></tikR:RegistroDuplicado>"
                .to_owned(),
            Linea::Omitida => continue,
        };
        lineas.push_str(&format!(
            "<tikR:RespuestaLinea><tikR:IDFactura><tik:IDEmisorFactura>B12345674</tik:IDEmisorFactura>\
             <tik:NumSerieFactura>{num}</tik:NumSerieFactura>\
             <tik:FechaExpedicionFactura>13-10-2026</tik:FechaExpedicionFactura></tikR:IDFactura>\
             <tikR:Operacion><tik:TipoOperacion>{op}</tik:TipoOperacion></tikR:Operacion>{estado}\
             </tikR:RespuestaLinea>"
        ));
    }
    let xml = format!(
        "<env:Envelope xmlns:env=\"http://schemas.xmlsoap.org/soap/envelope/\"><env:Body>\
         <tikR:RespuestaRegFactuSistemaFacturacion xmlns:tikR=\"{ns_r}\" xmlns:tik=\"{ns_sf}\">\
         <tikR:Cabecera><tik:ObligadoEmision><tik:NombreRazon>Estudio Demo SL</tik:NombreRazon>\
         <tik:NIF>B12345674</tik:NIF></tik:ObligadoEmision></tikR:Cabecera>\
         <tikR:TiempoEsperaEnvio>{espera}</tikR:TiempoEsperaEnvio>\
         <tikR:EstadoEnvio>Correcto</tikR:EstadoEnvio>{lineas}\
         </tikR:RespuestaRegFactuSistemaFacturacion></env:Body></env:Envelope>"
    );
    verifactu::parse_response(&xml).unwrap()
}

fn lote(v: &Vault, now: OffsetDateTime) -> verifactu_cola::Lote {
    match verifactu_cola::siguiente_lote(v, now).unwrap() {
        Siguiente::Enviar(l) => l,
        other => panic!("se esperaba un lote: {other:?}"),
    }
}

fn statuses(v: &Vault) -> Vec<String> {
    v.connection()
        .prepare("SELECT send_status FROM verifactu_record ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

#[test]
fn respeta_el_tiempo_de_espera_de_la_aeat() {
    let (v, _d) = setup();
    assert_eq!(
        verifactu_cola::siguiente_lote(&v, T0).unwrap(),
        Siguiente::Nada
    );
    issue(&v, &draft(100), T0, true).unwrap();
    issue(&v, &draft(200), T0, true).unwrap();

    let l = lote(&v, T0);
    assert_eq!(l.ids.len(), 2);
    assert!(!l.incidencia);
    assert_eq!(l.entorno, Entorno::Pruebas);
    assert!(!l.body.contains("Incidencia"));
    verifactu_cola::registrar_respuesta(&v, &l, &aeat(&l.body, 120, Linea::Correcto), T0).unwrap();
    assert_eq!(statuses(&v), ["pruebas", "pruebas"]);

    // Una factura nueva no sale hasta que pasen los 120 s.
    issue(&v, &draft(300), T0, true).unwrap();
    let t = T0 + Duration::seconds(30);
    assert_eq!(
        verifactu_cola::siguiente_lote(&v, t).unwrap(),
        Siguiente::Esperar(90)
    );
    let e = verifactu_cola::estado(&v, t).unwrap();
    assert_eq!(
        (e.pendientes, e.espera_segundos, e.tiempo_espera),
        (1, 90, 120)
    );
    assert_eq!(e.aceptados_pruebas, 2);
    assert_eq!(e.entorno, "pruebas");
    assert!(e.proximo_envio.is_some());
    assert!(e.aviso.is_none());
    let l = lote(&v, T0 + Duration::seconds(120));
    assert_eq!(l.ids.len(), 1);
}

#[test]
fn tras_un_corte_reintenta_con_espera_y_marca_la_incidencia() {
    let (v, _d) = setup();
    let a = issue(&v, &draft(100), T0, true).unwrap();
    let l = lote(&v, T0);
    verifactu_cola::registrar_fallo(&v, &l, "sin conexión con la AEAT", false, T0).unwrap();

    let e = verifactu_cola::estado(&v, T0).unwrap();
    assert!(e.incidencia_desde.is_some());
    assert_eq!((e.fallos_seguidos, e.espera_segundos), (1, 60));
    assert!(
        e.aviso
            .as_deref()
            .unwrap()
            .contains("1 registro sin enviar"),
        "{e:?}"
    );
    assert_eq!(
        verifactu_cola::siguiente_lote(&v, T0 + Duration::seconds(10)).unwrap(),
        Siguiente::Esperar(50)
    );

    // El reintento lleva Incidencia = S; si vuelve a fallar, espera el doble.
    let t1 = T0 + Duration::seconds(60);
    let l = lote(&v, t1);
    assert!(l.incidencia);
    assert!(l.body.contains("<sf:Incidencia>S</sf:Incidencia>"));
    verifactu_cola::registrar_fallo(&v, &l, "SOAP Fault (env:Server): caído", false, t1).unwrap();
    assert_eq!(
        verifactu_cola::siguiente_lote(&v, t1).unwrap(),
        Siguiente::Esperar(120)
    );

    // Llega: los registros quedan aceptados con la marca de incidencia y la cola, limpia.
    let t2 = t1 + Duration::seconds(120);
    let l = lote(&v, t2);
    assert!(l.incidencia);
    verifactu_cola::registrar_respuesta(&v, &l, &aeat(&l.body, 60, Linea::Correcto), t2).unwrap();
    let e = verifactu_cola::estado(&v, t2).unwrap();
    assert_eq!(
        (e.incidencia_desde, e.fallos_seguidos, e.aviso),
        (None, 0, None)
    );
    let (st, inc, intentos): (String, i64, i64) = v
        .connection()
        .query_row(
            "SELECT send_status, incidencia, intentos FROM verifactu_record WHERE invoice_id = ?1",
            [a.id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((st.as_str(), inc, intentos), ("pruebas", 1, 3));
}

#[test]
fn un_rechazo_del_mensaje_no_se_reintenta_solo() {
    let (v, _d) = setup();
    issue(&v, &draft(100), T0, true).unwrap();
    let l = lote(&v, T0);
    verifactu_cola::registrar_fallo(
        &v,
        &l,
        "SOAP Fault (env:Client): Codigo[4102].El XML no cumple el esquema",
        true,
        T0,
    )
    .unwrap();
    let later = T0 + Duration::hours(5);
    assert!(matches!(
        verifactu_cola::siguiente_lote(&v, later).unwrap(),
        Siguiente::Bloqueada(m) if m.contains("4102")
    ));
    let e = verifactu_cola::estado(&v, later).unwrap();
    assert!(e.bloqueada);
    assert!(e.aviso.unwrap().contains("Reintentar"));
    verifactu_cola::reabrir(&v, later).unwrap();
    let l = lote(&v, later);
    assert!(
        !l.incidencia,
        "un rechazo del mensaje no es una incidencia técnica"
    );
}

#[test]
fn un_duplicado_cuenta_como_remitido() {
    let (v, _d) = setup();
    let a = issue(&v, &draft(100), T0, true).unwrap();
    let l = lote(&v, T0);
    verifactu_cola::registrar_respuesta(&v, &l, &aeat(&l.body, 60, Linea::Duplicado), T0).unwrap();
    assert_eq!(statuses(&v), ["pruebas"]);
    assert!(invoicing::verifactu_issues(&v).unwrap().is_empty());
    let (dup, code): (Option<String>, Option<String>) = v
        .connection()
        .query_row(
            "SELECT duplicado, error_code FROM verifactu_record WHERE invoice_id = ?1",
            [a.id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((dup.as_deref(), code), (Some("Correcta"), None));
}

#[test]
fn sin_linea_se_reenvia_una_vez_y_luego_queda_sin_respuesta() {
    let (v, _d) = setup();
    let a = issue(&v, &draft(100), T0, true).unwrap();
    let l = lote(&v, T0);
    verifactu_cola::registrar_respuesta(&v, &l, &aeat(&l.body, 60, Linea::Omitida), T0).unwrap();
    assert_eq!(statuses(&v), ["pendiente"]);

    let t1 = T0 + Duration::seconds(60);
    let l = lote(&v, t1);
    assert_eq!(l.ids.len(), 1, "se reenvía una vez");
    verifactu_cola::registrar_respuesta(&v, &l, &aeat(&l.body, 60, Linea::Omitida), t1).unwrap();

    // A la segunda sale de la cola y se ve en la factura.
    let t2 = t1 + Duration::seconds(60);
    assert_eq!(
        verifactu_cola::siguiente_lote(&v, t2).unwrap(),
        Siguiente::Nada
    );
    let e = verifactu_cola::estado(&v, t2).unwrap();
    assert_eq!((e.pendientes, e.sin_respuesta), (0, 1));
    assert!(e.aviso.unwrap().contains("no ha contestado"));
    let f = list_issued(&v, 2026)
        .unwrap()
        .into_iter()
        .find(|i| i.id == a.id)
        .unwrap();
    assert_eq!(f.verifactu_status.as_deref(), Some("sin_respuesta"));
    assert!(f.verifactu_error.unwrap().contains("no devolvió respuesta"));
    // Sigue contando como no remitido (estado `pendiente` en la base de datos).
    assert_eq!(statuses(&v), ["pendiente"]);
}

/// El interruptor de producción está apagado: aunque la empresa tenga `production`, todo va a
/// pruebas y las facturas reales no llevan QR ni leyenda.
#[test]
fn produccion_sigue_cerrada_aunque_la_empresa_la_pida() {
    let v = vault();
    let dest = tempfile::tempdir().unwrap();
    let mut s = settings(dest.path().into());
    s.verifactu.production = true;
    invoicing::save_settings(&v, &s, T0).unwrap();
    const { assert!(!taxpain_core::productor::PRODUCCION_HABILITADA) };
    assert!(!verifactu_cola::remite_a_produccion(&s));
    assert_eq!(verifactu_cola::entorno(&s), Entorno::Pruebas);
    assert!(
        verifactu_cola::motivo_sin_produccion(&s)
            .unwrap()
            .contains("entorno de pruebas")
    );
    let a = issue(&v, &draft(100), T0, true).unwrap();
    assert_eq!(a.qr_url, None);
    let l = lote(&v, T0);
    assert_eq!(l.entorno, Entorno::Pruebas);
    let e = verifactu_cola::estado(&v, T0).unwrap();
    assert_eq!(e.entorno, "pruebas");
    assert!(e.sin_produccion.is_some());
}

#[test]
fn sin_verifactu_avisa_desde_cuando_no_es_conforme() {
    let dest = tempfile::tempdir().unwrap();
    let mut s = settings(dest.path().into());
    assert_eq!(
        verifactu_cola::aviso_sin_verifactu(&s, date!(2027 - 03 - 01)),
        None
    );
    s.verifactu.enabled = false;
    // Sociedad: 1/1/2027.
    assert_eq!(verifactu_cola::obligado_desde(&s), date!(2027 - 01 - 01));
    let antes = verifactu_cola::aviso_sin_verifactu(&s, date!(2026 - 11 - 17)).unwrap();
    assert!(
        antes.contains("01/01/2027") && antes.contains("antes de esa fecha"),
        "{antes}"
    );
    let despues = verifactu_cola::aviso_sin_verifactu(&s, date!(2027 - 01 - 02)).unwrap();
    assert!(despues.contains("sin registro de facturación"), "{despues}");
    // Autónomo: 1/7/2027.
    s.issuer.id = PartyId::Nif("12345678Z".into());
    assert_eq!(verifactu_cola::obligado_desde(&s), date!(2027 - 07 - 01));
}
