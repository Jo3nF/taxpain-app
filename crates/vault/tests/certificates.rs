//! Importación y uso de certificados con una PKI sintética (taxpain-testkit).

#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use taxpain_testkit::{
    Duration, OffsetDateTime, P12Format, Subject, TEST_CIF, TEST_DNI, TEST_PASSWORD, TestPki,
    random_key_pkcs8,
};
use taxpain_vault::{
    AlwaysConfirm, CertificateKind, FileKeyProvider, IssuerKind, KeyId, KeyProvider,
    MemoryKeyProvider, NeverConfirm, UserPresence, Vault, VaultError, audit,
};

fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

fn memory_vault(presence: Arc<dyn UserPresence>) -> Vault {
    Vault::open_in_memory(Arc::new(MemoryKeyProvider::default()), presence).unwrap()
}

/// Doble de Touch ID que registra los motivos mostrados.
#[derive(Debug, Default)]
struct RecordingPresence {
    reasons: Mutex<Vec<String>>,
}

impl UserPresence for RecordingPresence {
    fn confirm(&self, reason: &str) -> taxpain_secrets::Result<()> {
        self.reasons.lock().unwrap().push(reason.to_owned());
        Ok(())
    }
}

#[test]
fn importa_persona_fisica_fnmt_moderno_y_legacy() {
    let pki = TestPki::fnmt();
    let cert = pki.valid_client(&Subject::persona_fisica());
    for format in [P12Format::Modern, P12Format::Legacy3Des] {
        let vault = memory_vault(Arc::new(AlwaysConfirm));
        let info = vault
            .import_certificate(&cert.to_p12(TEST_PASSWORD, format), TEST_PASSWORD, now())
            .unwrap();
        assert_eq!(info.kind, CertificateKind::PersonaFisica);
        assert_eq!(info.issuer_kind, IssuerKind::Fnmt);
        assert_eq!(info.holder_nif, TEST_DNI);
        assert_eq!(
            info.holder_name.as_deref(),
            Some("CARMEN ESPAÑOLA ESPAÑOLA")
        );
        assert_eq!(info.entity_nif, None);
        assert_eq!(info.key_algorithm, "EC 256");
        assert_eq!(info.fingerprint_sha256.len(), 64);
        assert_eq!(vault.certificates().unwrap(), vec![info]);
    }
}

#[test]
fn importa_representante_de_persona_juridica() {
    let pki = TestPki::fnmt();
    let p12 = pki
        .valid_client(&Subject::representante())
        .to_p12(TEST_PASSWORD, P12Format::Modern);
    let vault = memory_vault(Arc::new(AlwaysConfirm));
    let info = vault
        .import_certificate(&p12, TEST_PASSWORD, now())
        .unwrap();
    assert_eq!(info.kind, CertificateKind::RepresentantePj);
    assert_eq!(info.holder_nif, TEST_DNI);
    assert_eq!(info.entity_nif.as_deref(), Some(TEST_CIF));
    assert_eq!(info.entity_name.as_deref(), Some("EMPRESA DE PRUEBA SL"));
}

#[test]
fn emisor_no_fnmt() {
    let pki = TestPki::new("OTRA CA DE PRUEBAS", "CA PRUEBAS");
    let p12 = pki
        .valid_client(&Subject::persona_fisica())
        .to_p12(TEST_PASSWORD, P12Format::Modern);
    let info = memory_vault(Arc::new(AlwaysConfirm))
        .import_certificate(&p12, TEST_PASSWORD, now())
        .unwrap();
    assert_eq!(info.issuer_kind, IssuerKind::Otro);
}

#[test]
fn rechaza_ficheros_no_validos() {
    let pki = TestPki::fnmt();
    let vault = memory_vault(Arc::new(AlwaysConfirm));
    let cert = pki.valid_client(&Subject::persona_fisica());

    let p12 = cert.to_p12(TEST_PASSWORD, P12Format::Modern);
    assert!(matches!(
        vault.import_certificate(&p12, "otra", now()),
        Err(VaultError::InvalidPassphrase)
    ));
    let legacy = cert.to_p12(TEST_PASSWORD, P12Format::Legacy3Des);
    assert!(matches!(
        vault.import_certificate(&legacy, "otra", now()),
        Err(VaultError::InvalidPassphrase)
    ));

    let t = now();
    let expired = pki.client(
        &Subject::persona_fisica(),
        t - Duration::days(800),
        t - Duration::days(1),
    );
    assert!(matches!(
        vault.import_certificate(
            &expired.to_p12(TEST_PASSWORD, P12Format::Modern),
            TEST_PASSWORD,
            t
        ),
        Err(VaultError::ExpiredCertificate(_))
    ));
    let future = pki.client(
        &Subject::persona_fisica(),
        t + Duration::days(2),
        t + Duration::days(800),
    );
    assert!(matches!(
        vault.import_certificate(
            &future.to_p12(TEST_PASSWORD, P12Format::Modern),
            TEST_PASSWORD,
            t
        ),
        Err(VaultError::NotYetValidCertificate(_))
    ));

    let mismatched = cert.to_p12_with_key(&random_key_pkcs8(), TEST_PASSWORD, P12Format::Modern);
    match vault.import_certificate(&mismatched, TEST_PASSWORD, t) {
        Err(VaultError::InvalidCertificate(m)) => assert!(m.contains("no corresponde"), "{m}"),
        other => panic!("esperaba clave no coincidente, no {other:?}"),
    }

    let sin_nif = pki.valid_client(&Subject::SinNif {
        common_name: "SERVIDOR X".into(),
    });
    match vault.import_certificate(
        &sin_nif.to_p12(TEST_PASSWORD, P12Format::Modern),
        TEST_PASSWORD,
        t,
    ) {
        Err(VaultError::InvalidCertificate(m)) => assert!(m.contains("NIF"), "{m}"),
        other => panic!("esperaba error de NIF, no {other:?}"),
    }

    for garbage in [&b""[..], b"no soy un p12", &[0u8; 300_000][..]] {
        assert!(matches!(
            vault.import_certificate(garbage, TEST_PASSWORD, t),
            Err(VaultError::InvalidCertificate(_))
        ));
    }
    assert!(vault.certificates().unwrap().is_empty());
}

#[test]
fn importar_dos_veces_no_duplica() {
    let pki = TestPki::fnmt();
    let p12 = pki
        .valid_client(&Subject::persona_fisica())
        .to_p12(TEST_PASSWORD, P12Format::Modern);
    let vault = memory_vault(Arc::new(AlwaysConfirm));
    let a = vault
        .import_certificate(&p12, TEST_PASSWORD, now())
        .unwrap();
    let b = vault
        .import_certificate(&p12, TEST_PASSWORD, now())
        .unwrap();
    assert_eq!(a, b);
    assert_eq!(vault.certificates().unwrap().len(), 1);
}

#[test]
fn usar_el_certificado_exige_confirmacion_y_queda_auditado() {
    let pki = TestPki::fnmt();
    let p12 = pki
        .valid_client(&Subject::representante())
        .to_p12(TEST_PASSWORD, P12Format::Modern);

    let denied = memory_vault(Arc::new(NeverConfirm));
    let info = denied
        .import_certificate(&p12, TEST_PASSWORD, now())
        .unwrap();
    assert!(matches!(
        denied.client_identity(&info.fingerprint_sha256, "Presentar el modelo 303", now()),
        Err(VaultError::NotConfirmed)
    ));
    let actions: Vec<_> = audit::entries(denied.connection())
        .unwrap()
        .into_iter()
        .map(|e| e.action)
        .collect();
    assert_eq!(actions, ["certificate.import"]);

    let presence = Arc::new(RecordingPresence::default());
    let vault = memory_vault(presence.clone());
    let info = vault
        .import_certificate(&p12, TEST_PASSWORD, now())
        .unwrap();
    let identity = vault
        .client_identity(&info.fingerprint_sha256, "Presentar el modelo 303", now())
        .unwrap();
    assert_eq!(identity.fingerprint(), info.fingerprint_sha256);
    assert!(
        !format!("{identity:?}").contains("key: ["),
        "Debug no debe volcar la clave"
    );
    let reasons = presence.reasons.lock().unwrap().clone();
    assert_eq!(reasons.len(), 1);
    assert!(reasons[0].contains("Presentar el modelo 303"));
    assert!(reasons[0].contains("EMPRESA DE PRUEBA SL"));
    assert_eq!(vault.verify_audit().unwrap(), 2);
}

#[test]
fn borrar_pide_touch_id_sin_el_almacen_y_lo_anota() {
    let pki = TestPki::fnmt();
    let p12 = pki
        .valid_client(&Subject::representante())
        .to_p12(TEST_PASSWORD, P12Format::Modern);
    let presence = Arc::new(RecordingPresence::default());
    let vault = memory_vault(presence.clone());
    let fp = vault
        .import_certificate(&p12, TEST_PASSWORD, now())
        .unwrap()
        .fingerprint_sha256;
    // Paso 1 (con el almacén): nada se pide todavía.
    let request = vault.prepare_delete(&fp).unwrap();
    assert!(presence.reasons.lock().unwrap().is_empty());
    assert!(matches!(
        vault.prepare_delete("0000"),
        Err(VaultError::NotFound)
    ));
    // Paso 2 (sin el almacén): Touch ID con el motivo.
    let confirmed = request.confirm().unwrap();
    let reasons = presence.reasons.lock().unwrap().clone();
    assert_eq!(reasons.len(), 1);
    assert!(
        reasons[0].starts_with("Eliminar el certificado de"),
        "{reasons:?}"
    );
    assert!(reasons[0].contains("EMPRESA DE PRUEBA SL"));
    // Paso 3 (con el almacén): fuera y anotado.
    vault.finish_delete(confirmed, now()).unwrap();
    assert!(vault.certificates().unwrap().is_empty());
    let actions: Vec<_> = audit::entries(vault.connection())
        .unwrap()
        .into_iter()
        .map(|e| e.action)
        .collect();
    assert_eq!(actions, ["certificate.import", "certificate.delete"]);

    // Sin confirmar, el certificado se queda.
    let denied = memory_vault(Arc::new(NeverConfirm));
    let fp = denied
        .import_certificate(&p12, TEST_PASSWORD, now())
        .unwrap()
        .fingerprint_sha256;
    assert!(matches!(
        denied.prepare_delete(&fp).unwrap().confirm(),
        Err(VaultError::NotConfirmed)
    ));
    assert_eq!(denied.certificates().unwrap().len(), 1);
}

#[test]
fn importar_no_guarda_el_certificado_si_no_se_puede_anotar() {
    let pki = TestPki::fnmt();
    let p12 = pki
        .valid_client(&Subject::persona_fisica())
        .to_p12(TEST_PASSWORD, P12Format::Modern);
    let vault = memory_vault(Arc::new(AlwaysConfirm));
    // La auditoría no se puede escribir (disco lleno, E/S…): se simula escondiendo la tabla.
    vault
        .connection()
        .execute_batch("ALTER TABLE audit_log RENAME TO audit_log_fuera")
        .unwrap();
    assert!(
        vault
            .import_certificate(&p12, TEST_PASSWORD, now())
            .is_err()
    );
    assert!(
        vault.certificates().unwrap().is_empty(),
        "sin anotación no hay certificado"
    );
    vault
        .connection()
        .execute_batch("ALTER TABLE audit_log_fuera RENAME TO audit_log")
        .unwrap();
    // El reintento lo importa de verdad y queda anotado.
    vault
        .import_certificate(&p12, TEST_PASSWORD, now())
        .unwrap();
    let actions: Vec<_> = audit::entries(vault.connection())
        .unwrap()
        .into_iter()
        .map(|e| e.action)
        .collect();
    assert_eq!(actions, ["certificate.import"]);
}

#[test]
fn certificado_caducado_tras_importar_no_se_usa() {
    let pki = TestPki::fnmt();
    let t = now();
    let cert = pki.client(
        &Subject::persona_fisica(),
        t - Duration::days(10),
        t + Duration::days(1),
    );
    let vault = memory_vault(Arc::new(AlwaysConfirm));
    let info = vault
        .import_certificate(
            &cert.to_p12(TEST_PASSWORD, P12Format::Modern),
            TEST_PASSWORD,
            t,
        )
        .unwrap();
    assert!(matches!(
        vault.client_identity(&info.fingerprint_sha256, "x", t + Duration::days(2)),
        Err(VaultError::ExpiredCertificate(_))
    ));
}

#[test]
fn persiste_en_disco_y_sin_la_clave_no_abre() {
    let dir = tempfile::tempdir().unwrap();
    let keys: Arc<dyn KeyProvider> =
        Arc::new(FileKeyProvider::new(dir.path().join("keys")).unwrap());
    let data = dir.path().join("data");
    let pki = TestPki::fnmt();
    let p12 = pki
        .valid_client(&Subject::persona_fisica())
        .to_p12(TEST_PASSWORD, P12Format::Modern);
    let fp = {
        let vault = Vault::open(&data, keys.clone(), Arc::new(AlwaysConfirm)).unwrap();
        assert!(!vault.keys_are_secure());
        vault
            .import_certificate(&p12, TEST_PASSWORD, now())
            .unwrap()
            .fingerprint_sha256
    };
    {
        let vault = Vault::open(&data, keys.clone(), Arc::new(AlwaysConfirm)).unwrap();
        assert_eq!(vault.certificates().unwrap()[0].fingerprint_sha256, fp);
        vault.client_identity(&fp, "prueba", now()).unwrap();
        vault.delete_certificate(&fp, now()).unwrap();
        assert!(vault.certificates().unwrap().is_empty());
        assert_eq!(vault.verify_audit().unwrap(), 3);
    }
    // Sin el elemento del Llavero (y sin código de recuperación) no se crea otra clave: se dice
    // que falta, no que el almacén esté dañado.
    keys.delete(KeyId::DbMaster).unwrap();
    assert!(matches!(
        Vault::open(&data, keys.clone(), Arc::new(AlwaysConfirm)),
        Err(VaultError::KeyMissing { recoverable: false })
    ));
    // Con otra clave que no es la suya: dañado o clave equivocada.
    keys.get_or_create(KeyId::DbMaster, "").unwrap();
    assert!(matches!(
        Vault::open(&data, keys, Arc::new(AlwaysConfirm)),
        Err(VaultError::Corrupted)
    ));
}

/// Datos que vienen de otro Mac: la clave de sellado se quedó en su Llavero. El certificado sale
/// en la lista, pero al usarlo no se crea otra clave en silencio (no abriría lo sellado con la
/// antigua): se dice qué pasa y cómo se arregla.
#[test]
fn sin_la_clave_de_sellado_se_dice_que_hay_que_reimportar() {
    let keys = Arc::new(MemoryKeyProvider::default());
    let vault = Vault::open_in_memory(keys.clone(), Arc::new(AlwaysConfirm)).unwrap();
    let pki = TestPki::fnmt();
    let p12 = pki
        .valid_client(&Subject::persona_fisica())
        .to_p12(TEST_PASSWORD, P12Format::Modern);
    let fp = vault
        .import_certificate(&p12, TEST_PASSWORD, now())
        .unwrap()
        .fingerprint_sha256;
    keys.delete(KeyId::CertSeal).unwrap();
    let e = vault.client_identity(&fp, "x", now()).unwrap_err();
    assert!(matches!(e, VaultError::CertificateSealedElsewhere), "{e:?}");
    assert!(e.to_string().contains("vuelve a importarlo"), "{e}");
    assert!(
        keys.get(KeyId::CertSeal, "").unwrap().is_none(),
        "usar un certificado no crea la clave de sellado"
    );
    assert_eq!(vault.certificates().unwrap().len(), 1, "sigue en la lista");
}

/// Reimportar el mismo `.p12` cuando lo guardado no se puede abrir con la clave de este Mac lo
/// vuelve a sellar (antes respondía «bien» sin cambiar nada) y lo anota.
#[test]
fn reimportar_vuelve_a_sellar_lo_que_no_se_puede_abrir() {
    let keys = Arc::new(MemoryKeyProvider::default());
    let vault = Vault::open_in_memory(keys.clone(), Arc::new(AlwaysConfirm)).unwrap();
    let pki = TestPki::fnmt();
    let p12 = pki
        .valid_client(&Subject::persona_fisica())
        .to_p12(TEST_PASSWORD, P12Format::Modern);
    let info = vault
        .import_certificate(&p12, TEST_PASSWORD, now())
        .unwrap();
    let fp = info.fingerprint_sha256.clone();
    let actions = |v: &Vault| -> Vec<String> {
        audit::entries(v.connection())
            .unwrap()
            .into_iter()
            .map(|e| e.action)
            .collect()
    };
    // Si se puede abrir, reimportar no cambia nada (ni lo anota).
    assert_eq!(
        vault
            .import_certificate(&p12, TEST_PASSWORD, now())
            .unwrap(),
        info
    );
    assert_eq!(actions(&vault), ["certificate.import"]);

    // «Otro Mac»: el Llavero tiene otra clave de sellado (o ninguna).
    keys.delete(KeyId::CertSeal).unwrap();
    keys.get_or_create(KeyId::CertSeal, "").unwrap();
    assert!(matches!(
        vault.client_identity(&fp, "x", now()),
        Err(VaultError::CertificateSealedElsewhere)
    ));
    // Con la contraseña mala no se toca nada.
    assert!(
        vault
            .import_certificate(&p12, "otra contraseña", now())
            .is_err()
    );
    assert!(vault.client_identity(&fp, "x", now()).is_err());

    assert_eq!(
        vault
            .import_certificate(&p12, TEST_PASSWORD, now())
            .unwrap(),
        info
    );
    assert_eq!(vault.certificates().unwrap(), vec![info.clone()]);
    assert_eq!(
        actions(&vault),
        ["certificate.import", "certificate.reseal"]
    );
    vault.client_identity(&fp, "x", now()).unwrap();

    // Sin clave de sellado en el Llavero: reimportar la crea (es donde se crea) y sella.
    keys.delete(KeyId::CertSeal).unwrap();
    vault
        .import_certificate(&p12, TEST_PASSWORD, now())
        .unwrap();
    assert!(keys.get(KeyId::CertSeal, "").unwrap().is_some());
    vault.client_identity(&fp, "x", now()).unwrap();
    assert_eq!(
        actions(&vault),
        [
            "certificate.import",
            "certificate.reseal",
            "certificate.use",
            "certificate.reseal",
            "certificate.use"
        ]
    );
    assert_eq!(vault.certificates().unwrap().len(), 1);
}

#[test]
fn ajustes_y_avisos_enviados() {
    let vault = memory_vault(Arc::new(AlwaysConfirm));
    assert_eq!(vault.setting::<Vec<u32>>("x").unwrap(), None);
    vault.set_setting("x", &vec![1u32, 2]).unwrap();
    vault.set_setting("x", &vec![3u32]).unwrap();
    assert_eq!(vault.setting::<Vec<u32>>("x").unwrap(), Some(vec![3]));
    assert!(
        vault
            .mark_reminder_sent("303|2026|1T|2026-04-13", now())
            .unwrap()
    );
    assert!(
        !vault
            .mark_reminder_sent("303|2026|1T|2026-04-13", now())
            .unwrap()
    );
}
