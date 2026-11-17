//! Notificaciones de la AEAT en el almacén: alta al consultar, contenido al acceder,
//! inmutabilidad y auditoría de la comparecencia.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::notices::{self, NoticeContent, NoticeMeta};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::datetime;

fn meta(id: &str, kind: &str, state: &str) -> NoticeMeta {
    NoticeMeta {
        id: id.into(),
        kind: kind.into(),
        state: state.into(),
        role: "T".into(),
        subject: "Notificación administrativa".into(),
        available_at: "2026-09-01T09:00:00".into(),
        holder_nif: "12345678Z".into(),
    }
}

#[test]
fn consulta_acceso_y_comparecencia() {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let now = datetime!(2026-10-14 9:00 UTC);
    let list = [
        meta("2319486500018", "C", "A"),
        meta("2319979900012", "N", "P"),
    ];
    assert_eq!(notices::record_listed(&v, true, &list, now).unwrap(), 2);
    assert_eq!(notices::record_listed(&v, true, &list, now).unwrap(), 0);
    let saved = notices::list(&v).unwrap();
    assert!(
        saved[0].is_pending_notification(),
        "las pendientes, primero"
    );

    let content = NoticeContent {
        metadata: vec![
            ("Concepto".into(), "Requerimiento".into()),
            ("CSVDocumentoPDF".into(), "AAAAAAAAAAAAAAAA".into()),
        ],
        document_pdf: b"%PDF-1.4 doc".to_vec(),
        certification_pdf: b"%PDF-1.4 cert".to_vec(),
    };
    assert!(
        notices::record_content(
            &v,
            "2319979900012",
            &NoticeContent {
                document_pdf: b"<html>".to_vec(),
                ..content.clone()
            },
            true,
            now
        )
        .is_err()
    );
    notices::record_content(&v, "2319979900012", &content, true, now).unwrap();
    let n = notices::list(&v)
        .unwrap()
        .into_iter()
        .find(|n| n.id == "2319979900012")
        .unwrap();
    assert_eq!(n.state, "A");
    assert!(n.has_content);
    assert_eq!(n.metadata[0].1, "Requerimiento");
    assert_eq!(
        notices::pdf(&v, "2319979900012", true).unwrap().as_deref(),
        Some(&b"%PDF-1.4 cert"[..])
    );
    // El contenido no se sustituye ni se borra.
    notices::record_content(
        &v,
        "2319979900012",
        &NoticeContent {
            document_pdf: b"%PDF-otro".to_vec(),
            ..content.clone()
        },
        false,
        now,
    )
    .unwrap();
    assert_eq!(
        notices::pdf(&v, "2319979900012", false).unwrap().as_deref(),
        Some(&b"%PDF-1.4 doc"[..])
    );
    assert!(
        v.connection()
            .execute("DELETE FROM aeat_notice", [])
            .is_err()
    );
    let log = taxpain_vault::audit::entries(v.connection()).unwrap();
    assert!(
        log.iter()
            .any(|e| e.action == "notices.appear" && e.subject == "2319979900012")
    );
    assert!(notices::record_content(&v, "0000000000000", &content, false, now).is_err());
}

#[test]
fn comparecer_solo_pendientes_y_con_constancia_previa() {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let now = datetime!(2026-10-14 9:00 UTC);
    let list = [
        meta("2319486500018", "C", "A"),
        meta("2319979900012", "N", "P"),
    ];
    notices::record_listed(&v, true, &list, now).unwrap();
    assert!(notices::begin_appear(&v, "0000000000000", now).is_err());
    assert!(notices::begin_appear(&v, "2319486500018", now).is_err());
    notices::begin_appear(&v, "2319979900012", now).unwrap();
    // Contenido inservible tras comparecer: queda como accedida y auditada igualmente.
    let bad = NoticeContent {
        metadata: vec![],
        document_pdf: b"<html>".to_vec(),
        certification_pdf: vec![],
    };
    assert!(notices::record_content(&v, "2319979900012", &bad, true, now).is_err());
    let n = notices::list(&v)
        .unwrap()
        .into_iter()
        .find(|n| n.id == "2319979900012")
        .unwrap();
    assert_eq!(n.state, "A");
    let actions: Vec<String> = taxpain_vault::audit::entries(v.connection())
        .unwrap()
        .into_iter()
        .map(|e| e.action)
        .collect();
    assert!(actions.contains(&"notices.appear.request".to_owned()));
    assert!(actions.contains(&"notices.appear".to_owned()));
    // Ya no se puede volver a comparecer.
    assert!(notices::begin_appear(&v, "2319979900012", now).is_err());

    assert_eq!(
        notices::pdf_file_name("../../etc/x", false),
        "notificacion_etcx.pdf"
    );
    assert_eq!(notices::safe_file_component("/"), "sin_id");
}

#[test]
fn cuenta_atras_de_las_pendientes_y_dias_de_cortesia() {
    use time::macros::date;
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let now = datetime!(2026-08-03 9:00 UTC);
    // Puesta a disposición el 03/08 (agosto no es inhábil aquí): se da por notificada el 13/08.
    let pend = NoticeMeta {
        available_at: "2026-08-03T10:34:53".into(),
        ..meta("2319979900012", "N", "P")
    };
    let list = [pend, meta("2319486500018", "C", "P"), meta("1", "N", "A")];
    notices::record_listed(&v, true, &list, now).unwrap();
    let saved = notices::list(&v).unwrap();
    let n = saved.iter().find(|n| n.id == "2319979900012").unwrap();
    assert_eq!(n.deemed_notified_on, Some(date!(2026 - 08 - 13)));
    // Ni las comunicaciones ni las ya abiertas tienen cuenta atrás.
    assert!(
        saved
            .iter()
            .filter(|n| n.id != "2319979900012")
            .all(|n| n.deemed_notified_on.is_none())
    );
    // Avisos a 5, 2 y 0 días del 12/08 (la víspera).
    assert!(
        notices::reminders_due(&v, date!(2026 - 08 - 06))
            .unwrap()
            .is_empty()
    );
    let r = notices::reminders_due(&v, date!(2026 - 08 - 07)).unwrap();
    assert_eq!(r.len(), 1);
    assert!(
        r[0].body.contains("ábrela antes del 12/08/2026"),
        "{}",
        r[0].body
    );
    assert!(r[0].body.contains("El 13/08/2026 se dará por notificada"));
    let r = notices::reminders_due(&v, date!(2026 - 08 - 12)).unwrap();
    assert!(r[0].body.contains("hoy es el último día"));
    assert_ne!(
        r[0].key,
        notices::reminders_due(&v, date!(2026 - 08 - 10)).unwrap()[0].key
    );

    // Días de cortesía: ordenados, sin repetir y como mucho 30 por año.
    assert!(notices::courtesy_days(&v).unwrap().is_empty());
    let days = notices::set_courtesy_days(
        &v,
        &[
            date!(2026 - 08 - 20),
            date!(2026 - 08 - 10),
            date!(2026 - 08 - 10),
        ],
    )
    .unwrap();
    assert_eq!(days, [date!(2026 - 08 - 10), date!(2026 - 08 - 20)]);
    assert_eq!(notices::courtesy_days(&v).unwrap(), days);
    let many: Vec<_> = (0..31)
        .map(|i| date!(2026 - 07 - 01) + time::Duration::days(i))
        .collect();
    assert!(notices::set_courtesy_days(&v, &many).is_err());
    assert_eq!(notices::courtesy_days(&v).unwrap(), days);
}
