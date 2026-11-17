//! Copias de seguridad: se hacen con la contraseña, se comprueban, se restauran en «otro Mac»
//! (otro proveedor de claves) y no llevan los certificados. La empresa restaurada no hereda la
//! contraseña de la copia.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_testkit::{OffsetDateTime, P12Format, Subject, TEST_PASSWORD, TestPki};
use taxpain_vault::backup::{self, KdfParams};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault, VaultError, audit};

const PASS: &str = "caballo grapa batería";
/// scrypt más ligero para que los tests vayan rápido (el formato guarda los parámetros).
const FAST: KdfParams = KdfParams {
    log_n: 12,
    r: 8,
    p: 1,
};

fn now() -> OffsetDateTime {
    OffsetDateTime::now_utc()
}

fn open(dir: &std::path::Path, keys: &Arc<MemoryKeyProvider>) -> Vault {
    Vault::open(dir, keys.clone(), Arc::new(AlwaysConfirm)).unwrap()
}

#[test]
fn copia_y_restauracion_en_otro_mac() {
    let tmp = tempfile::tempdir().unwrap();
    let keys = Arc::new(MemoryKeyProvider::default());
    let v = open(&tmp.path().join("empresa"), &keys);
    v.set_setting(
        "company",
        &serde_json::json!({ "name": "EJEMPLO SL", "nif": "B12345674" }),
    )
    .unwrap();
    let p12 = TestPki::fnmt()
        .valid_client(&Subject::representante())
        .to_p12(TEST_PASSWORD, P12Format::Modern);
    v.import_certificate(&p12, TEST_PASSWORD, now()).unwrap();
    v.connection()
        .execute(
            "INSERT INTO app_secret(name, value, updated_at) VALUES ('api-key@ejemplo', 'sk-x', 'x')",
            [],
        )
        .unwrap();

    // Sin contraseña no hay copia.
    let dest = tmp.path().join("copias/EJEMPLO-2026-10-24.taxpain");
    assert!(backup::export(&v, &dest, "0.1.0", now()).is_err());
    assert!(!backup::has_passphrase(&v).unwrap());
    assert!(backup::set_passphrase_with(&v, "corta", FAST, now()).is_err());
    backup::set_passphrase_with(&v, PASS, FAST, now()).unwrap();
    assert!(backup::has_passphrase(&v).unwrap());
    assert!(backup::verify_passphrase(&v, PASS).unwrap());
    assert!(!backup::verify_passphrase(&v, "otra contraseña").unwrap());

    let report = backup::export(&v, &dest, "0.1.0", now()).unwrap();
    assert!(report.bytes > 0 && report.audit_entries > 0);
    // Ni temporales ni datos en claro junto a la copia.
    let names: Vec<String> = std::fs::read_dir(dest.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["EJEMPLO-2026-10-24.taxpain".to_owned()]);
    let raw = std::fs::read(&dest).unwrap();
    assert!(!raw.windows(10).any(|w| w == b"EJEMPLO SL"));
    // La copia queda en la auditoría y el certificado sigue en este Mac.
    let last = audit::entries(v.connection()).unwrap().pop().unwrap();
    assert_eq!(last.action, "backup.export");
    assert_eq!(v.certificates().unwrap().len(), 1);

    // La cabecera se lee sin contraseña; el contenido no.
    let header = backup::read_header(&dest).unwrap();
    assert_eq!(header.app_version, "0.1.0");
    assert_eq!(header.kdf, FAST);
    assert!(matches!(
        backup::open(&dest, "otra contraseña"),
        Err(VaultError::BackupPassphrase)
    ));
    let opened = backup::open(&dest, PASS).unwrap();
    let name: String = opened
        .conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'company'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(name.contains("EJEMPLO SL"));
    let certs: i64 = opened
        .conn
        .query_row("SELECT count(*) FROM certificate_ref", [], |r| r.get(0))
        .unwrap();
    assert_eq!(certs, 0, "los certificados no van en la copia");
    let secrets: i64 = opened
        .conn
        .query_row("SELECT count(*) FROM app_secret", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        secrets, 0,
        "la clave de API del asistente no va en la copia"
    );
    assert_eq!(
        v.connection()
            .query_row("SELECT count(*) FROM app_secret", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1,
        "y sigue en este Mac"
    );
    drop(opened);

    // Otro Mac: otro Keychain. Se restaura en una carpeta nueva.
    let other_keys = Arc::new(MemoryKeyProvider::default());
    let target = tmp.path().join("otro-mac/empresa");
    assert!(matches!(
        backup::restore(
            &dest,
            "otra contraseña",
            &target,
            other_keys.clone(),
            Arc::new(AlwaysConfirm),
            now()
        ),
        Err(VaultError::BackupPassphrase)
    ));
    assert!(!target.join("taxpain.db").exists());
    let (restored, header) = backup::restore(
        &dest,
        PASS,
        &target,
        other_keys.clone(),
        Arc::new(AlwaysConfirm),
        now(),
    )
    .unwrap();
    assert_eq!(header.sha256, report.sha256);
    let company: Option<serde_json::Value> = restored.setting("company").unwrap();
    assert_eq!(company.unwrap()["nif"], "B12345674");
    assert!(restored.certificates().unwrap().is_empty());
    assert_eq!(
        audit::entries(restored.connection())
            .unwrap()
            .pop()
            .unwrap()
            .action,
        "backup.restore"
    );
    // La contraseña de la copia es de quien la hizo: la empresa restaurada no la hereda. Queda
    // sin contraseña de copias y sin código de recuperación (que sella la clave de todas las
    // empresas del Mac) hasta que el usuario elige una.
    assert!(!backup::has_passphrase(&restored).unwrap());
    assert!(!backup::verify_passphrase(&restored, PASS).unwrap());
    assert!(!taxpain_vault::recovery::exists(&target));
    let again = target.parent().unwrap().join("de-nuevo.taxpain");
    assert!(backup::export(&restored, &again, "0.1.0", now()).is_err());
    drop(restored);
    // Se vuelve a abrir con normalidad (auditoría verificada, migraciones al día, sin avisos:
    // la empresa restaurada nace con su ancla) y sigue sin código de recuperación.
    let reopened = open(&target, &other_keys);
    assert!(reopened.audit_rollback().is_none());
    assert!(reopened.audit_rollback_pending().unwrap().is_none());
    assert!(!taxpain_vault::recovery::exists(&target));
    // Con la contraseña nueva, la que elige quien restaura, ya hay copias y código.
    backup::set_passphrase_with(&reopened, "otra frase mía larga", FAST, now()).unwrap();
    assert!(taxpain_vault::recovery::exists(&target));
    assert!(matches!(
        taxpain_vault::recovery::unseal(&target, PASS),
        Err(VaultError::BackupPassphrase)
    ));
    backup::export(&reopened, &again, "0.1.0", now()).unwrap();
    assert!(matches!(
        backup::open(&again, PASS),
        Err(VaultError::BackupPassphrase)
    ));
    assert!(backup::open(&again, "otra frase mía larga").is_ok());
    drop(reopened);

    // No se restaura encima de una empresa.
    assert!(
        backup::restore(
            &dest,
            PASS,
            &target,
            other_keys,
            Arc::new(AlwaysConfirm),
            now()
        )
        .is_err()
    );
}

#[test]
fn copia_danada_o_ajena() {
    let tmp = tempfile::tempdir().unwrap();
    let keys = Arc::new(MemoryKeyProvider::default());
    let v = open(&tmp.path().join("empresa"), &keys);
    backup::set_passphrase_with(&v, PASS, FAST, now()).unwrap();
    let dest = tmp.path().join("copia.taxpain");
    backup::export(&v, &dest, "0.1.0", now()).unwrap();

    // Un byte cambiado en la BD: «dañada», no «contraseña incorrecta».
    let mut bytes = std::fs::read(&dest).unwrap();
    let last = bytes.len() - 100;
    bytes[last] ^= 0x55;
    let bad = tmp.path().join("mala.taxpain");
    std::fs::write(&bad, &bytes).unwrap();
    let e = backup::open(&bad, PASS).unwrap_err().to_string();
    assert!(e.contains("dañada"), "{e}");

    // Un fichero cualquiera.
    let other = tmp.path().join("otro.taxpain");
    std::fs::write(&other, b"no soy una copia").unwrap();
    let e = backup::read_header(&other).unwrap_err().to_string();
    assert!(e.contains("no es una copia"), "{e}");

    // Cambiar la contraseña no impide abrir las copias anteriores con la antigua.
    backup::set_passphrase_with(&v, "otra contraseña larga", FAST, now()).unwrap();
    assert!(backup::open(&dest, PASS).is_ok());
    let newer = tmp.path().join("nueva.taxpain");
    backup::export(&v, &newer, "0.1.0", now()).unwrap();
    assert!(matches!(
        backup::open(&newer, PASS),
        Err(VaultError::BackupPassphrase)
    ));
    assert!(backup::open(&newer, "otra contraseña larga").is_ok());
}

/// Código de recuperación: con la carpeta de datos y la contraseña de las copias se abre el
/// almacén sin el elemento del Llavero (Mac nuevo, cuenta nueva, elemento borrado).
#[test]
fn codigo_de_recuperacion_abre_sin_el_llavero() {
    use taxpain_vault::{KeyId, KeyProvider, recovery};

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("empresa");
    let keys = Arc::new(MemoryKeyProvider::default());
    let v = open(&dir, &keys);
    v.set_setting("company", &serde_json::json!({ "name": "EJEMPLO SL" }))
        .unwrap();
    // Sin contraseña de copias no hay código de recuperación.
    assert!(!recovery::exists(&dir));
    backup::set_passphrase_with(&v, PASS, FAST, now()).unwrap();
    assert!(recovery::exists(&dir));
    let db_key = keys.get_or_create(KeyId::DbMaster, "").unwrap();
    assert_eq!(
        recovery::unseal(&dir, PASS).unwrap().expose(),
        db_key.expose()
    );
    assert!(matches!(
        recovery::unseal(&dir, "otra contraseña"),
        Err(VaultError::BackupPassphrase)
    ));
    // Al cambiar la contraseña se regenera: la antigua deja de servir.
    backup::set_passphrase_with(&v, "la contraseña nueva", FAST, now()).unwrap();
    assert!(recovery::unseal(&dir, PASS).is_err());
    assert_eq!(
        recovery::unseal(&dir, "la contraseña nueva")
            .unwrap()
            .expose(),
        db_key.expose()
    );
    drop(v);

    // «Otro Mac»: Llavero vacío. Abrir dice que falta la clave y que se puede recuperar.
    let fresh = Arc::new(MemoryKeyProvider::default());
    assert!(matches!(
        Vault::open(&dir, fresh.clone(), Arc::new(AlwaysConfirm)),
        Err(VaultError::KeyMissing { recoverable: true })
    ));
    assert!(
        fresh.get(KeyId::DbMaster, "").unwrap().is_none(),
        "no crea otra clave"
    );
    assert!(matches!(
        Vault::open_recovered(
            &dir,
            fresh.clone(),
            Arc::new(AlwaysConfirm),
            "otra contraseña"
        ),
        Err(VaultError::BackupPassphrase)
    ));
    assert!(fresh.get(KeyId::DbMaster, "").unwrap().is_none());
    let v = Vault::open_recovered(
        &dir,
        fresh.clone(),
        Arc::new(AlwaysConfirm),
        "la contraseña nueva",
    )
    .unwrap();
    assert_eq!(
        v.setting::<serde_json::Value>("company").unwrap().unwrap()["name"],
        "EJEMPLO SL"
    );
    let last = audit::entries(v.connection()).unwrap().pop().unwrap();
    assert_eq!(last.action, "vault.recovered");
    // La clave vuelve al Llavero: la próxima apertura es normal.
    assert_eq!(
        fresh.get(KeyId::DbMaster, "").unwrap().unwrap().expose(),
        db_key.expose()
    );
    drop(v);
    let v = open(&dir, &fresh);
    assert!(backup::verify_passphrase(&v, "la contraseña nueva").unwrap());
    drop(v);

    // Mac con otras empresas (ya tiene su propia `db-master`): la BD se recifra con ésa y el
    // código de recuperación se reescribe para la nueva clave.
    let other = Arc::new(MemoryKeyProvider::default());
    let other_key = other.get_or_create(KeyId::DbMaster, "").unwrap();
    assert!(matches!(
        Vault::open(&dir, other.clone(), Arc::new(AlwaysConfirm)),
        Err(VaultError::Corrupted)
    ));
    assert!(recovery::exists(&dir));
    let v = Vault::open_recovered(
        &dir,
        other.clone(),
        Arc::new(AlwaysConfirm),
        "la contraseña nueva",
    )
    .unwrap();
    assert_eq!(
        v.setting::<serde_json::Value>("company").unwrap().unwrap()["name"],
        "EJEMPLO SL"
    );
    assert_eq!(
        other.get_or_create(KeyId::DbMaster, "").unwrap().expose(),
        other_key.expose(),
        "la clave del Mac no cambia"
    );
    assert_eq!(
        recovery::unseal(&dir, "la contraseña nueva")
            .unwrap()
            .expose(),
        other_key.expose()
    );
    drop(v);
    let v = open(&dir, &other);
    assert!(v.verify_audit().unwrap() > 0);
    drop(v);

    // Datos sin código de recuperación pero con contraseña (versión anterior): al abrir se repone.
    std::fs::remove_file(recovery::path(&dir)).unwrap();
    let v = open(&dir, &other);
    assert!(recovery::exists(&dir));
    drop(v);
    // Sin código ni Llavero no hay recuperación posible, y se dice claramente.
    std::fs::remove_file(recovery::path(&dir)).unwrap();
    assert!(matches!(
        Vault::open(
            &dir,
            Arc::new(MemoryKeyProvider::default()),
            Arc::new(AlwaysConfirm)
        ),
        Err(VaultError::KeyMissing { recoverable: false })
    ));
    assert!(matches!(
        Vault::open_recovered(
            &dir,
            Arc::new(MemoryKeyProvider::default()),
            Arc::new(AlwaysConfirm),
            "la contraseña nueva"
        ),
        Err(VaultError::Io(_))
    ));
}
