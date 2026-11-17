//! Ancla de la auditoría fuera de la BD: una copia antigua de la BD se detecta al abrir, y el
//! aviso queda pendiente en la propia BD hasta que el usuario lo reconoce.

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::sync::Arc;

use serde_json::json;
use taxpain_vault::audit::{self, Actor, AnchorProblem, Rollback};
use taxpain_vault::{AlwaysConfirm, FileKeyProvider, KeyProvider, Vault};
use time::OffsetDateTime;

fn log(v: &Vault, action: &str) {
    audit::append(
        v.connection(),
        "2026-10-14T10:00:00Z",
        Actor::User,
        action,
        "s",
        &json!({}),
    )
    .unwrap();
}

fn keys(dir: &Path) -> Arc<dyn KeyProvider> {
    Arc::new(FileKeyProvider::new(dir.join("keys")).unwrap())
}

fn last_action(v: &Vault) -> String {
    audit::entries(v.connection())
        .unwrap()
        .pop()
        .unwrap()
        .action
}

fn anchored_seq(data: &Path) -> i64 {
    let text = std::fs::read_to_string(data.join("audit.head")).unwrap();
    audit::Anchor::decode(&text).unwrap().seq
}

/// Deja `data` con tres entradas vistas (ancla en la 3) y la BD de cuando sólo tenía una.
fn roll_back(data: &Path, open: &dyn Fn() -> Vault) {
    {
        let v = open();
        assert_eq!(v.audit_rollback(), None);
        log(&v, "a");
    }
    let backup = data.with_extension("copia.db");
    std::fs::copy(data.join("taxpain.db"), &backup).unwrap();
    {
        let v = open();
        log(&v, "b");
        log(&v, "c");
        // El ancla se actualiza también sin cerrar.
        v.sync_anchor().unwrap();
        assert_eq!(anchored_seq(data), 3);
    }
    // Se restaura la copia antigua (p. ej. desde una copia de seguridad).
    std::fs::copy(&backup, data.join("taxpain.db")).unwrap();
}

#[test]
fn una_copia_antigua_de_la_bd_se_detecta_y_queda_pendiente_hasta_reconocerla() {
    let dir = tempfile::tempdir().unwrap();
    let keys = keys(dir.path());
    let data = dir.path().join("data");
    let open = || Vault::open(&data, keys.clone(), Arc::new(AlwaysConfirm)).unwrap();
    let rollback = Rollback {
        anchored_seq: 3,
        current_seq: 1,
    };
    roll_back(&data, &open);

    // Quien abre primero puede ser una tarea de fondo o cualquier otro que no enseña nada: lo
    // detecta, lo anota en la cadena y el ancla avanza.
    {
        let v = open();
        assert_eq!(v.audit_rollback(), Some(&rollback));
        assert_eq!(last_action(&v), "audit.rollback");
        assert_eq!(v.verify_audit().unwrap(), 2);
    }
    assert_eq!(anchored_seq(&data), 2);

    // El aviso no se ha perdido: sigue pendiente en las aperturas siguientes (la de la app,
    // que es la que lo enseña), sin anotarse otra vez, hasta que el usuario lo reconoce.
    for _ in 0..2 {
        let v = open();
        assert_eq!(v.audit_rollback(), Some(&rollback));
        let p = v.audit_rollback_pending().unwrap().unwrap();
        assert_eq!(p.kind, AnchorProblem::Rollback);
        assert_eq!(p.anchored_seq, Some(3));
        assert_eq!(p.current_seq, 1);
        assert!(p.detected_at.starts_with("20"), "{}", p.detected_at);
        assert_eq!(v.verify_audit().unwrap(), 2, "no se vuelve a anotar");
    }
    {
        let v = open();
        assert!(
            v.acknowledge_audit_rollback(OffsetDateTime::now_utc())
                .unwrap()
        );
        assert_eq!(v.audit_rollback(), None);
        assert!(v.audit_rollback_pending().unwrap().is_none());
        let last = audit::entries(v.connection()).unwrap().pop().unwrap();
        assert_eq!(last.action, "audit.rollback_acknowledged");
        assert_eq!(last.actor, "user");
        assert!(
            last.details.contains("\"anchored_seq\":3"),
            "{}",
            last.details
        );
        // Ya no hay nada que reconocer.
        assert!(
            !v.acknowledge_audit_rollback(OffsetDateTime::now_utc())
                .unwrap()
        );
        assert_eq!(v.verify_audit().unwrap(), 3);
    }
    let v = open();
    assert_eq!(v.audit_rollback(), None);
    assert!(v.audit_rollback_pending().unwrap().is_none());
}

/// Hay entradas de auditoría y el ancla no se puede leer o no está: sin ella un retroceso no se
/// vería, así que también se avisa (y se anota), aunque no sea un retroceso que se pueda medir.
#[test]
fn un_ancla_ilegible_o_borrada_es_sospechosa() {
    let dir = tempfile::tempdir().unwrap();
    let keys = keys(dir.path());
    let data = dir.path().join("data");
    let open = || Vault::open(&data, keys.clone(), Arc::new(AlwaysConfirm)).unwrap();
    {
        let v = open();
        log(&v, "a");
    }
    std::fs::write(data.join("audit.head"), "basura").unwrap();
    {
        let v = open();
        assert_eq!(v.audit_rollback(), None, "no es un retroceso medible");
        let p = v.audit_rollback_pending().unwrap().unwrap();
        assert_eq!(p.kind, AnchorProblem::AnchorInvalid);
        assert_eq!((p.anchored_seq, p.current_seq), (None, 1));
        assert_eq!(p.rollback(), None);
        assert_eq!(last_action(&v), "audit.anchor_invalid");
    }
    // El ancla se reescribe con la cabeza nueva.
    assert_eq!(anchored_seq(&data), 2);
    {
        let v = open();
        assert!(
            v.audit_rollback_pending().unwrap().is_some(),
            "sigue pendiente"
        );
        assert!(
            v.acknowledge_audit_rollback(OffsetDateTime::now_utc())
                .unwrap()
        );
    }

    std::fs::remove_file(data.join("audit.head")).unwrap();
    {
        let v = open();
        let p = v.audit_rollback_pending().unwrap().unwrap();
        assert_eq!(p.kind, AnchorProblem::AnchorMissing);
        assert_eq!((p.anchored_seq, p.current_seq), (None, 3));
        assert_eq!(last_action(&v), "audit.anchor_missing");
        assert_eq!(v.verify_audit().unwrap(), 4);
    }
    assert_eq!(anchored_seq(&data), 4);
    let v = open();
    assert_eq!(
        v.audit_rollback_pending().unwrap().unwrap().kind,
        AnchorProblem::AnchorMissing
    );
    assert_eq!(v.verify_audit().unwrap(), 4, "no se vuelve a anotar");
}

/// Borrar el ancla después de retroceder la BD no rebaja el aviso de retroceso pendiente.
#[test]
fn borrar_el_ancla_no_tapa_un_retroceso_pendiente() {
    let dir = tempfile::tempdir().unwrap();
    let keys = keys(dir.path());
    let data = dir.path().join("data");
    let open = || Vault::open(&data, keys.clone(), Arc::new(AlwaysConfirm)).unwrap();
    roll_back(&data, &open);
    drop(open());
    std::fs::remove_file(data.join("audit.head")).unwrap();
    let v = open();
    assert_eq!(last_action(&v), "audit.anchor_missing");
    let p = v.audit_rollback_pending().unwrap().unwrap();
    assert_eq!(p.kind, AnchorProblem::Rollback);
    assert_eq!(
        v.audit_rollback(),
        Some(&Rollback {
            anchored_seq: 3,
            current_seq: 1
        })
    );
}

/// Un almacén recién creado no tiene entradas ni ancla: no hay nada de qué avisar, ni al
/// crearlo ni al volver a abrirlo.
#[test]
fn un_almacen_nuevo_no_avisa() {
    let dir = tempfile::tempdir().unwrap();
    let keys = keys(dir.path());
    let data = dir.path().join("data");
    let open = || Vault::open(&data, keys.clone(), Arc::new(AlwaysConfirm)).unwrap();
    for _ in 0..2 {
        let v = open();
        assert!(v.audit_rollback_pending().unwrap().is_none());
        assert_eq!(v.verify_audit().unwrap(), 0);
    }
    assert!(!data.join("audit.head").exists());
    {
        let v = open();
        log(&v, "a");
    }
    let v = open();
    assert!(v.audit_rollback_pending().unwrap().is_none());
    assert_eq!(v.verify_audit().unwrap(), 1);
}
