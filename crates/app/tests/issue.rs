//! Emisión de facturas de extremo a extremo con el almacén cifrado.

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;
use std::sync::Arc;

use taxpain_app::invoicing::{
    InvoiceDraft, apply_submission_response, cancel, invoice_document, issue, list_issued,
    pending_submission, preview,
};
use taxpain_app::settings::{
    Destination, DestinationKind, InvoicingSettings, Language, Outputs, VerifactuSettings,
    new_installation_id,
};
use taxpain_app::{AppError, invoicing};
use taxpain_core::invoice::{
    Address, InvoiceKind, InvoiceLine, Party, PartyId, PersonName, Rectification,
    RectificationMethod, RoundingMode, VatTreatment,
};
use taxpain_core::money::{Money, Rate};
use taxpain_invoicing::verifactu::{self, Response, ResponseLine};
use taxpain_testkit::xsd;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

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
            id: PartyId::Nif("12345678Z".into()),
            name: "Carmen Española Española".into(),
            address: addr(),
            person: Some(PersonName {
                name: "Carmen".into(),
                first_surname: "Española".into(),
                second_surname: None,
            }),
        },
        default_series: "A".into(),
        rectifying_series: "R".into(),
        simplified_series: "S".into(),
        simplified_rectifying_series: "RS".into(),
        language: Language::Es,
        default_vat: Rate::percent(21),
        default_withholding: Some(Rate::percent(15)),
        outputs: Outputs {
            pdf: true,
            facturae: true,
            ubl: true,
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
        footer: Some("IBAN ES00 0000".into()),
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
            id: PartyId::Nif("B12345674".into()),
            name: "Empresa de Prueba SL".into(),
            address: addr(),
            person: None,
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
            withholding: Some(Rate::percent(15)),
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

#[test]
fn flujo_completo_de_emision() {
    let v = vault();
    let dest = tempfile::tempdir().unwrap();
    let now = datetime!(2026-10-13 10:00 +02:00);

    assert!(matches!(
        issue(&v, &draft(100), now, true),
        Err(AppError::NotConfigured)
    ));
    invoicing::save_settings(&v, &settings(dest.path().into()), now).unwrap();

    // Borrador.
    let (t, pdf) = preview(&v, &draft(100), now).unwrap();
    assert_eq!(t.payable, Money::from_euros(106, 0));
    assert!(pdf.starts_with(b"%PDF"));

    // Sin confirmación no se emite.
    assert!(matches!(
        issue(&v, &draft(100), now, false),
        Err(AppError::NotConfirmed)
    ));

    let a = issue(&v, &draft(100), now, true).unwrap();
    let b = issue(&v, &draft(200), now, true).unwrap();
    assert_eq!(
        (a.number.as_str(), b.number.as_str()),
        ("A2026-0001", "A2026-0002")
    );
    // Registro encadenado sí; QR y leyenda no: sin envío a producción la factura no es
    // verificable en la Sede (Orden HAC/1177/2024 art. 20.1.b) y el QR iría a prewww2.aeat.es.
    assert!(a.verifactu_huella.is_some());
    assert_eq!(
        a.qr_url, None,
        "sin producción ni banco de pruebas no hay QR"
    );
    assert_eq!(a.export_error, None);
    let exported = a.exported_to.clone().unwrap();
    assert!(exported.ends_with("2026/A2026-0001.pdf") && exported.is_file());
    assert!(
        exported.with_extension("xml").is_file(),
        "Facturae exportado"
    );
    assert!(
        exported.with_extension("ubl.xml").is_file(),
        "factura UBL exportada"
    );
    let (name, _, ubl) = invoicing::invoice_ubl(&v, a.id).unwrap();
    assert_eq!(name, "A2026-0001.ubl.xml");
    assert!(
        String::from_utf8(ubl)
            .unwrap()
            .contains("urn:aeat:names:specification:ubl:schema:profile:B2B")
    );

    // Rectificativa por diferencias en su serie.
    let mut r = draft(-50);
    r.kind = InvoiceKind::Rectificativa(Rectification {
        code: 1,
        method: RectificationMethod::Diferencias,
    });
    r.rectified = vec![("A2026-0002".into(), date!(2026 - 10 - 13))];
    let rect = issue(&v, &r, now, true).unwrap();
    assert_eq!(rect.number, "R2026-0001");

    // Anulación encadenada.
    cancel(&v, a.id, now, true).unwrap();
    let list = list_issued(&v, 2026).unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(
        list.iter().find(|i| i.id == a.id).unwrap().status,
        "anulada"
    );
    assert!(
        list.iter()
            .all(|i| i.verifactu_status.as_deref() == Some("pendiente"))
    );
    // Los ficheros guardados se pueden descargar desde la lista.
    assert!(list.iter().all(|i| i.has_ubl));

    // La cadena enlaza cada registro con el anterior.
    let conn = v.connection();
    let mut stmt = conn
        .prepare("SELECT huella, prev_huella, record_type FROM verifactu_record ORDER BY id")
        .unwrap();
    let chain: Vec<(String, Option<String>, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(chain.len(), 4);
    assert_eq!(chain[0].1, None);
    for w in chain.windows(2) {
        assert_eq!(w[1].1.as_deref(), Some(w[0].0.as_str()));
    }
    assert_eq!(chain[3].2, "anulacion");
    // Inmutable y sin borrado.
    assert!(
        conn.execute("UPDATE verifactu_record SET huella = 'X' WHERE id = 1", [])
            .is_err()
    );
    assert!(conn.execute("DELETE FROM verifactu_record", []).is_err());

    // Documentos recuperables desde la BD cifrada.
    let (name, mime, bytes) = invoice_document(&v, b.id, false).unwrap();
    assert_eq!(
        (name.as_str(), mime.as_str()),
        ("A2026-0002.pdf", "application/pdf")
    );
    assert!(bytes.starts_with(b"%PDF"));
    assert!(
        invoice_document(&v, b.id, true)
            .unwrap()
            .2
            .starts_with(b"<?xml")
    );

    // Lote pendiente: válido contra el esquema oficial.
    // La anulación de A2026-0001 espera al siguiente envío: un registro por factura y lote.
    let (ids, body) = pending_submission(&v, 1000, invoicing::Entorno::Pruebas)
        .unwrap()
        .unwrap();
    assert_eq!(ids, [1, 2, 3]);
    let dir = xsd::local_schema_dir(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/aeat/verifactu"),
        &[(
            "http://www.w3.org/TR/xmldsig-core/xmldsig-core-schema.xsd",
            "xmldsig-core-schema.xsd",
        )],
    );
    if let Err(e) = xsd::validate(
        &dir.path().join("SuministroLR.xsd"),
        &format!("<?xml version=\"1.0\"?>{body}"),
    ) {
        panic!("lote no válido:\n{}", e.join("\n"));
    }

    // Respuesta: dos correctos, uno con errores; el resto sigue pendiente.
    let line = |n: &str, e: &str| ResponseLine {
        num_serie: n.into(),
        operacion: "Alta".into(),
        estado: e.into(),
        codigo_error: (e != "Correcto").then(|| "1100".into()),
        descripcion_error: None,
        duplicado: None,
    };
    let resp = Response {
        csv: Some("CSV-PRUEBA".into()),
        estado_envio: "ParcialmenteCorrecto".into(),
        tiempo_espera_envio: 60,
        lineas: vec![
            line("A2026-0002", "Correcto"),
            line("R2026-0001", "AceptadoConErrores"),
        ],
    };
    apply_submission_response(&v, &ids[1..3], &resp, invoicing::Entorno::Pruebas, now).unwrap();
    let st: Vec<String> = conn
        .prepare("SELECT send_status FROM verifactu_record ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        st,
        ["pendiente", "pruebas", "aceptado_con_errores", "pendiente"]
    );
    // Alta y anulación de la misma factura: cada una toma su línea por el tipo de operación.
    let resp2 = Response {
        csv: Some("CSV-2".into()),
        estado_envio: "ParcialmenteCorrecto".into(),
        tiempo_espera_envio: 60,
        lineas: vec![
            ResponseLine {
                operacion: "Anulacion".into(),
                ..line("A2026-0001", "Incorrecto")
            },
            line("A2026-0001", "Correcto"),
        ],
    };
    apply_submission_response(&v, &[1, 4], &resp2, invoicing::Entorno::Pruebas, now).unwrap();
    let st: Vec<String> = conn
        .prepare("SELECT send_status FROM verifactu_record ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(
        st,
        ["pruebas", "pruebas", "aceptado_con_errores", "incorrecto"]
    );
    assert_eq!(
        v.verify_audit().unwrap(),
        7,
        "ajustes + 3 emisiones + anulación + 2 envíos"
    );

    // Subsanación: R2026-0001 (aceptado con errores) y la anulación rechazada.
    let issues = invoicing::verifactu_issues(&v).unwrap();
    assert_eq!(
        issues.iter().map(|i| i.id).collect::<Vec<_>>(),
        [3, 4],
        "{issues:?}"
    );
    assert!(
        invoicing::resubmit(&v, 3, now, false).is_err(),
        "sin confirmar, no"
    );
    let a = invoicing::resubmit(&v, 3, now, true).unwrap();
    let b = invoicing::resubmit(&v, 4, now, true).unwrap();
    assert!(
        invoicing::resubmit(&v, 4, now, true).is_err(),
        "sólo una vez"
    );
    assert!(
        invoicing::resubmit(&v, 2, now, true).is_err(),
        "los correctos no"
    );
    assert!(invoicing::verifactu_issues(&v).unwrap().is_empty());
    let xml = |id: i64| -> String {
        conn.query_row(
            "SELECT xml FROM verifactu_record WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert!(xml(a).contains("<sf:Subsanacion>S</sf:Subsanacion><sf:TipoFactura>"));
    assert!(xml(b).contains("<sf:RechazoPrevio>S</sf:RechazoPrevio>"));
    // Ambos quedan pendientes; van en lotes distintos si son de la misma factura, y son válidos.
    let (ids, body) = pending_submission(&v, 1000, invoicing::Entorno::Pruebas)
        .unwrap()
        .unwrap();
    assert_eq!(ids, [a, b]);
    if let Err(e) = xsd::validate(
        &dir.path().join("SuministroLR.xsd"),
        &format!("<?xml version=\"1.0\"?>{body}"),
    ) {
        panic!("reenvío no válido:\n{}", e.join("\n"));
    }
    // Si la AEAT rechaza también la nueva alta (que no existe allí): se reenvía con X.
    let resp3 = Response {
        csv: None,
        estado_envio: "Incorrecto".into(),
        tiempo_espera_envio: 60,
        lineas: vec![line("R2026-0001", "Incorrecto")],
    };
    apply_submission_response(&v, &[a], &resp3, invoicing::Entorno::Pruebas, now).unwrap();
    let c = invoicing::resubmit(&v, a, now, true).unwrap();
    assert!(
        xml(c).contains("<sf:Subsanacion>S</sf:Subsanacion><sf:RechazoPrevio>S</sf:RechazoPrevio>"),
        "la subsanación de un registro que existe rechazada: S/S"
    );
    // Un alta normal rechazada (no existe en la AEAT): reenvío con X.
    let nueva = issue(&v, &draft(300), now, true).unwrap();
    let rec_id: i64 = conn
        .query_row(
            "SELECT id FROM verifactu_record WHERE invoice_id = ?1",
            [nueva.id],
            |r| r.get(0),
        )
        .unwrap();
    let resp4 = Response {
        csv: None,
        estado_envio: "Incorrecto".into(),
        tiempo_espera_envio: 60,
        lineas: vec![line(&nueva.number, "Incorrecto")],
    };
    apply_submission_response(&v, &[rec_id], &resp4, invoicing::Entorno::Pruebas, now).unwrap();
    let d = invoicing::resubmit(&v, rec_id, now, true).unwrap();
    assert!(
        xml(d).contains("<sf:Subsanacion>S</sf:Subsanacion><sf:RechazoPrevio>X</sf:RechazoPrevio>")
    );
    let _ = verifactu::LEGEND;
}

/// El PDF lleva el bloque VERI*FACTU (QR y leyenda) sólo si contiene «QR tributario».
fn pdf_has_qr(v: &Vault, id: i64) -> bool {
    let (_, _, bytes) = invoice_document(v, id, false).unwrap();
    let text = pdf_extract::extract_text_from_mem(&bytes).unwrap();
    text.contains("QR tributario") || text.contains(verifactu::LEGEND)
}

#[test]
fn sin_produccion_la_factura_real_no_lleva_qr_ni_leyenda() {
    let v = vault();
    let dest = tempfile::tempdir().unwrap();
    let now = datetime!(2026-10-13 10:00 +02:00);
    invoicing::save_settings(&v, &settings(dest.path().into()), now).unwrap();
    let a = issue(&v, &draft(100), now, true).unwrap();
    assert!(
        a.verifactu_huella.is_some(),
        "el registro se genera y encadena"
    );
    assert_eq!(a.qr_url, None);
    assert!(!pdf_has_qr(&v, a.id), "ni QR ni leyenda en el PDF");
    let stored_qr: Option<String> = v
        .connection()
        .query_row(
            "SELECT qr_url FROM verifactu_record WHERE invoice_id = ?1",
            [a.id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored_qr, None);
}

#[test]
fn la_empresa_del_banco_de_pruebas_lleva_el_qr_de_pruebas() {
    let v = vault();
    let dest = tempfile::tempdir().unwrap();
    let now = datetime!(2026-10-13 10:00 +02:00);
    invoicing::save_settings(&v, &settings(dest.path().into()), now).unwrap();
    taxpain_app::verifactu_lab::mark(&v, now).unwrap();
    let a = issue(&v, &draft(100), now, true).unwrap();
    assert!(
        a.qr_url
            .as_ref()
            .is_some_and(|u| u.starts_with("https://prewww2.aeat.es/")),
        "{:?}",
        a.qr_url
    );
    assert!(pdf_has_qr(&v, a.id));
}

#[test]
fn con_verifactu_desactivado_no_hay_registro_ni_qr() {
    let v = vault();
    let dest = tempfile::tempdir().unwrap();
    let now = datetime!(2026-10-13 10:00 +02:00);
    let mut s = settings(dest.path().into());
    s.verifactu.enabled = false;
    invoicing::save_settings(&v, &s, now).unwrap();
    let a = issue(&v, &draft(100), now, true).unwrap();
    assert_eq!((a.verifactu_huella, a.qr_url), (None, None));
    assert!(!pdf_has_qr(&v, a.id));
}

#[test]
fn las_simplificadas_van_en_su_serie_y_no_se_mezclan_con_las_completas() {
    let v = vault();
    let dest = tempfile::tempdir().unwrap();
    let now = datetime!(2026-10-13 10:00 +02:00);
    invoicing::save_settings(&v, &settings(dest.path().into()), now).unwrap();

    let mut simple = draft(10);
    simple.kind = InvoiceKind::Simplificada;
    simple.recipient = None;
    // RD 1619/2012 art. 7.1.a último párrafo: series separadas de completas y simplificadas.
    let a = issue(&v, &draft(100), now, true).unwrap();
    let s1 = issue(&v, &simple, now, true).unwrap();
    let b = issue(&v, &draft(100), now, true).unwrap();
    let s2 = issue(&v, &simple, now, true).unwrap();
    assert_eq!(
        [&a.number, &s1.number, &b.number, &s2.number],
        ["A2026-0001", "S2026-0001", "A2026-0002", "S2026-0002"]
    );

    // Una simplificada en la serie ordinaria y una completa en la «S»: rechazadas.
    let mut wrong = simple.clone();
    wrong.series = Some("A".into());
    let err = issue(&v, &wrong, now, true).unwrap_err().to_string();
    assert!(err.contains("serie A es de facturas completas"), "{err}");
    let mut wrong = draft(100);
    wrong.series = Some("S".into());
    let err = issue(&v, &wrong, now, true).unwrap_err().to_string();
    assert!(
        err.contains("serie S es de facturas simplificadas"),
        "{err}"
    );
    // Una rectificativa de una simplificada (R5) es una simplificada: va en su propia serie
    // «RS», separada de las rectificativas de completas (RD 1619/2012 art. 7.1.a último
    // párrafo) y de las simplificadas ordinarias (art. 7.1.a 4.º).
    let mut r = draft(-5);
    r.kind = InvoiceKind::Rectificativa(Rectification {
        code: 5,
        method: RectificationMethod::Diferencias,
    });
    r.recipient = None;
    r.rectified = vec![("S2026-0001".into(), date!(2026 - 10 - 13))];
    assert_eq!(issue(&v, &r, now, true).unwrap().number, "RS2026-0001");
    let mut wrong = r.clone();
    wrong.series = Some("R".into());
    let err = issue(&v, &wrong, now, true).unwrap_err().to_string();
    assert!(
        err.contains(
            "serie R es de rectificativas y esta factura es de rectificativas de \
                      simplificadas"
        ),
        "{err}"
    );
    // Y una rectificativa de una completa (R1) no puede ir en la «RS».
    let mut r1 = draft(-5);
    r1.kind = InvoiceKind::Rectificativa(Rectification {
        code: 1,
        method: RectificationMethod::Diferencias,
    });
    r1.rectified = vec![("A2026-0001".into(), date!(2026 - 10 - 13))];
    r1.series = Some("RS".into());
    let err = issue(&v, &r1, now, true).unwrap_err().to_string();
    assert!(
        err.contains("serie RS es de rectificativas de simplificadas"),
        "{err}"
    );
}

#[test]
fn ajustes_anteriores_sin_serie_de_simplificadas_toman_la_s() {
    let v = vault();
    let dest = tempfile::tempdir().unwrap();
    let now = datetime!(2026-10-13 10:00 +02:00);
    invoicing::save_settings(&v, &settings(dest.path().into()), now).unwrap();
    // Una instalación anterior: los ajustes guardados no tienen `simplified_series` y la serie
    // «S» no existe en la base de datos.
    let mut json: serde_json::Value = v
        .setting(taxpain_app::settings::SETTINGS_KEY)
        .unwrap()
        .unwrap();
    json.as_object_mut().unwrap().remove("simplified_series");
    json.as_object_mut()
        .unwrap()
        .remove("simplified_rectifying_series");
    v.set_setting(taxpain_app::settings::SETTINGS_KEY, &json)
        .unwrap();
    v.connection()
        .execute("DELETE FROM invoice_series WHERE code IN ('S', 'RS')", [])
        .unwrap();
    let loaded = invoicing::settings(&v).unwrap().unwrap();
    assert_eq!(loaded.simplified_series, "S");
    assert_eq!(loaded.simplified_rectifying_series, "RS");
    let mut simple = draft(10);
    simple.kind = InvoiceKind::Simplificada;
    simple.recipient = None;
    assert_eq!(issue(&v, &simple, now, true).unwrap().number, "S2026-0001");
}

#[test]
fn las_tres_series_deben_ser_distintas() {
    let v = vault();
    let dest = tempfile::tempdir().unwrap();
    let now = datetime!(2026-10-13 10:00 +02:00);
    let mut s = settings(dest.path().into());
    s.simplified_series = "A".into();
    assert!(invoicing::save_settings(&v, &s, now).is_err());
    s.simplified_series = " ".into();
    assert!(invoicing::save_settings(&v, &s, now).is_err());
}

#[test]
fn simplificada_sin_destinatario_no_genera_facturae() {
    let v = vault();
    let dest = tempfile::tempdir().unwrap();
    let now = datetime!(2026-10-13 10:00 +02:00);
    let mut s = settings(dest.path().into());
    s.verifactu.enabled = false;
    invoicing::save_settings(&v, &s, now).unwrap();
    let mut d = draft(10);
    d.kind = InvoiceKind::Simplificada;
    d.recipient = None;
    let out = issue(&v, &d, now, true).unwrap();
    assert_eq!(out.verifactu_huella, None);
    assert!(invoice_document(&v, out.id, true).is_err());
    assert!(out.exported_to.unwrap().is_file());
}

#[test]
fn facturae_imposible_no_impide_emitir() {
    let v = vault();
    let dest = tempfile::tempdir().unwrap();
    let now = datetime!(2026-10-13 10:00 +02:00);
    // Emisor con un país que la Facturae no conoce: no se puede generar, la factura sí se emite.
    // (Sin domicilio ya no se emite: art. 6.1.e RD 1619/2012.)
    let mut s = settings(dest.path().into());
    s.issuer.address.as_mut().unwrap().country = "XX".into();
    invoicing::save_settings(&v, &s, now).unwrap();
    let out = issue(&v, &draft(100), now, true).unwrap();
    assert_eq!(out.number, "A2026-0001");
    assert!(
        out.export_error
            .as_deref()
            .is_some_and(|e| e.contains("Facturae")),
        "{:?}",
        out.export_error
    );
    assert!(out.verifactu_huella.is_some());
}

#[test]
fn series_correlativas_y_rectificativas_aparte() {
    let v = vault();
    let dest = tempfile::tempdir().unwrap();
    let now = datetime!(2026-10-13 10:00 +02:00);
    invoicing::save_settings(&v, &settings(dest.path().into()), now).unwrap();
    issue(&v, &draft(100), now, true).unwrap();
    // Fecha anterior a la última de la serie: no.
    let mut atras = draft(100);
    atras.issue_date = date!(2026 - 10 - 01);
    let e = issue(&v, &atras, now, true).unwrap_err().to_string();
    assert!(e.contains("correlativa"), "{e}");
    // Una ordinaria en la serie de rectificativas: no.
    let mut en_r = draft(100);
    en_r.series = Some("R".into());
    let e = issue(&v, &en_r, now, true).unwrap_err().to_string();
    assert!(e.contains("serie R es de rectificativas"), "{e}");
}

#[test]
fn sin_fecha_futura_ni_operacion_posterior_a_la_factura() {
    let v = vault();
    let dest = tempfile::tempdir().unwrap();
    let now = datetime!(2026-10-13 10:00 +02:00);
    invoicing::save_settings(&v, &settings(dest.path().into()), now).unwrap();
    // Expedición futura: VERI*FACTU la rechaza («no podrá ser superior a la fecha actual»).
    let mut futura = draft(100);
    futura.issue_date = date!(2026 - 10 - 14);
    let e = issue(&v, &futura, now, true).unwrap_err().to_string();
    assert!(e.contains("posterior a hoy"), "{e}");
    // La vista previa avisa de lo mismo, antes de llegar a confirmar.
    let e = invoicing::preview(&v, &futura, now)
        .unwrap_err()
        .to_string();
    assert!(e.contains("posterior a hoy"), "{e}");
    // Operación posterior a la expedición: sólo con claves de régimen 14 o 15, que no se usan.
    let mut op = draft(100);
    op.operation_date = Some(date!(2026 - 10 - 20));
    let e = issue(&v, &op, now, true).unwrap_err().to_string();
    assert!(e.contains("fecha de la operación"), "{e}");
    let e = invoicing::preview(&v, &op, now).unwrap_err().to_string();
    assert!(e.contains("fecha de la operación"), "{e}");
    // Con la fecha de hoy, la vista previa sale.
    invoicing::preview(&v, &draft(100), now).unwrap();
    // «Hoy» es el peninsular: a las 23:30 UTC del 12/10 ya es 13/10 en España.
    let mut ok = draft(100);
    ok.operation_date = Some(date!(2026 - 10 - 01));
    issue(&v, &ok, datetime!(2026-10-12 23:30 UTC), true).unwrap();
}

#[test]
fn nif_iva_del_cliente_con_la_estructura_de_su_estado() {
    use taxpain_core::invoice::IdType;
    let v = vault();
    let dest = tempfile::tempdir().unwrap();
    let now = datetime!(2026-10-13 10:00 +02:00);
    invoicing::save_settings(&v, &settings(dest.path().into()), now).unwrap();
    let eu = |id: &str| {
        let mut d = draft(100);
        d.recipient = Some(Party {
            id: PartyId::Foreign {
                country: "FR".into(),
                id_type: IdType::NifIva,
                id: id.into(),
            },
            name: "Client SARL".into(),
            address: addr(),
            person: None,
        });
        d.lines[0].vat = VatTreatment::NotSubject(taxpain_core::invoice::NotSubject::N2);
        d.lines[0].withholding = None;
        d
    };
    // «FR1» pasaba al registro VERI*FACTU, al 349 y a la UBL: la AEAT lo rechaza después.
    let e = issue(&v, &eu("FR1"), now, true).unwrap_err().to_string();
    assert!(e.contains("NIF-IVA") && e.contains("11 caracteres"), "{e}");
    assert!(preview(&v, &eu("FR1"), now).is_err());
    // Con espacios y en el formato del país: se normaliza y se emite.
    let r = issue(&v, &eu("FR 123 456 789 01"), now, true).unwrap();
    assert_eq!(r.number, "A2026-0001");
}

/// No se anula una factura que ya tiene una rectificativa o un cobro: quedarían colgando (el 303
/// sumaría la rectificativa de una venta que ya no existe; el cobro, contra una 430 sin deuda).
#[test]
fn no_se_anula_lo_que_ya_tiene_rectificativa_o_cobro() {
    let tmp = tempfile::tempdir().unwrap();
    let v = vault();
    let now = datetime!(2026-10-13 10:00 UTC);
    invoicing::save_settings(&v, &settings(tmp.path().to_path_buf()), now).unwrap();
    let a = issue(&v, &draft(1000), now, true).unwrap();
    let b = issue(&v, &draft(500), now, true).unwrap();
    let mut r = draft(-500);
    r.kind = InvoiceKind::Rectificativa(Rectification {
        code: 1,
        method: RectificationMethod::Diferencias,
    });
    r.rectified = vec![(a.number.clone(), date!(2026 - 10 - 13))];
    let rect = issue(&v, &r, now, true).unwrap();
    let e = cancel(&v, a.id, now, true).unwrap_err().to_string();
    assert!(e.contains(&format!("la rectifica {}", rect.number)), "{e}");
    // Anulada la rectificativa, ya se puede.
    cancel(&v, rect.id, now, true).unwrap();
    cancel(&v, a.id, now, true).unwrap();
    // Con un cobro, no.
    taxpain_app::cartera::settle(&v, "emitida", b.id, 1, date!(2026 - 10 - 13), "570", now)
        .unwrap();
    let e = cancel(&v, b.id, now, true).unwrap_err().to_string();
    assert!(e.contains("cobros"), "{e}");
}

/// Lo que acepta el entorno de pruebas queda como `pruebas`: no cuenta como remitido en el
/// cierre y se vuelve a enviar a producción. Cada lote se corta en el primer registro repetido
/// de una factura, para no adelantar los posteriores.
#[test]
fn lo_aceptado_en_pruebas_no_esta_remitido_y_el_lote_va_en_orden() {
    let v = vault();
    let dest = tempfile::tempdir().unwrap();
    let now = datetime!(2026-10-13 10:00 +02:00);
    invoicing::save_settings(&v, &settings(dest.path().into()), now).unwrap();
    let a1 = issue(&v, &draft(100), now, true).unwrap();
    cancel(&v, a1.id, now, true).unwrap();
    let a2 = issue(&v, &draft(200), now, true).unwrap();
    let ids_of = |n: &str| -> Vec<i64> {
        v.connection()
            .prepare("SELECT id FROM verifactu_record WHERE num_serie = ?1 ORDER BY id")
            .unwrap()
            .query_map([n], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    };
    let (alta1, anul1, alta2) = (
        ids_of(&a1.number)[0],
        ids_of(&a1.number)[1],
        ids_of(&a2.number)[0],
    );

    // Alta A-1, anulación A-1, alta A-2: el primer lote es sólo el alta de A-1 (antes iban el
    // alta de A-1 y la de A-2, y la anulación quedaba detrás de un registro posterior).
    let (ids, _) = pending_submission(&v, 1000, invoicing::Entorno::Pruebas)
        .unwrap()
        .unwrap();
    assert_eq!(ids, [alta1]);
    let ok = |n: &str, op: &str| ResponseLine {
        num_serie: n.into(),
        operacion: op.into(),
        estado: "Correcto".into(),
        codigo_error: None,
        descripcion_error: None,
        duplicado: None,
    };
    let resp = |lineas| Response {
        csv: Some("CSV-PRUEBAS".into()),
        estado_envio: "Correcto".into(),
        tiempo_espera_envio: 60,
        lineas,
    };
    apply_submission_response(
        &v,
        &ids,
        &resp(vec![ok(&a1.number, "Alta")]),
        invoicing::Entorno::Pruebas,
        now,
    )
    .unwrap();
    let (ids, _) = pending_submission(&v, 1000, invoicing::Entorno::Pruebas)
        .unwrap()
        .unwrap();
    assert_eq!(ids, [anul1, alta2]);
    apply_submission_response(
        &v,
        &ids,
        &resp(vec![ok(&a1.number, "Anulacion"), ok(&a2.number, "Alta")]),
        invoicing::Entorno::Pruebas,
        now,
    )
    .unwrap();

    // Aceptados en pruebas: no son `correcto` ni tienen nada que subsanar.
    let st: Vec<String> = v
        .connection()
        .prepare("SELECT send_status FROM verifactu_record ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(st, ["pruebas", "pruebas", "pruebas"]);
    assert!(invoicing::verifactu_issues(&v).unwrap().is_empty());
    // Al entorno de pruebas no se reenvían; a producción sí, en el mismo orden y por lotes.
    assert!(
        pending_submission(&v, 1000, invoicing::Entorno::Pruebas)
            .unwrap()
            .is_none()
    );
    let (ids, _) = pending_submission(&v, 1000, invoicing::Entorno::Produccion)
        .unwrap()
        .unwrap();
    assert_eq!(ids, [alta1]);
    apply_submission_response(
        &v,
        &ids,
        &resp(vec![ok(&a1.number, "Alta")]),
        invoicing::Entorno::Produccion,
        now,
    )
    .unwrap();
    let (ids, _) = pending_submission(&v, 1000, invoicing::Entorno::Produccion)
        .unwrap()
        .unwrap();
    assert_eq!(ids, [anul1, alta2]);

    // El cierre no los da por remitidos.
    let c = taxpain_app::cierre::checklist(&v, 2026).unwrap();
    let item = c.items.iter().find(|i| i.id == "verifactu").unwrap();
    assert!(!item.ok, "{item:?}");
    assert!(
        item.detail
            .contains("2 enviados sólo al entorno de pruebas"),
        "{item:?}"
    );
}
