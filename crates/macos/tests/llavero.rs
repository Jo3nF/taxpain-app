//! Prueba contra el Llavero real del Mac (punto 71 de `docs/MAC_TESTING.md`): el ciclo completo
//! del código de recuperación con el proveedor de claves de verdad, sin la interfaz.
//!
//! Usa un servicio propio (`es.taxpain.prueba.<pid>`), así que no toca el elemento
//! «TaxPain (db-master)» del usuario, y borra lo suyo al terminar. Está marcada `#[ignore]`
//! porque crea elementos en el Llavero de quien la ejecuta:
//!
//! ```sh
//! cargo test -p taxpain-macos --test llavero -- --ignored
//! ```
#![cfg(target_os = "macos")]
#![allow(clippy::unwrap_used)]

use std::process::Command;
use std::sync::Arc;

use taxpain_macos::{KeychainKeyProvider, KeychainMode};
use taxpain_vault::backup::{self, KdfParams};
use taxpain_vault::{AlwaysConfirm, KeyId, KeyProvider, Vault, VaultError, audit, recovery};

const PASS: &str = "caballo grapa batería";
const FAST: KdfParams = KdfParams {
    log_n: 12,
    r: 8,
    p: 1,
};

/// Lo que haría el usuario en «Acceso a Llaveros»: borrar el elemento con la herramienta del
/// sistema, no con el proveedor de la app.
fn borrar_desde_el_sistema(service: &str) {
    let out = Command::new("security")
        .args(["delete-generic-password", "-s", service, "-a", "db-master"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "security delete-generic-password: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// ¿Existe el elemento en el Llavero según el sistema?
fn existe_en_el_sistema(service: &str) -> bool {
    Command::new("security")
        .args(["find-generic-password", "-s", service, "-a", "db-master"])
        .output()
        .unwrap()
        .status
        .success()
}

#[test]
#[ignore = "crea y borra elementos en el Llavero real del Mac"]
fn recuperacion_con_el_llavero_real() {
    let service = format!("es.taxpain.prueba.{}", std::process::id());
    let keys = Arc::new(KeychainKeyProvider::with_service(
        KeychainMode::Legacy,
        &service,
    ));
    keys.delete(KeyId::DbMaster).unwrap();
    assert!(!existe_en_el_sistema(&service));

    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("data");

    // 1. Alta: la clave nace en el Llavero y, con la contraseña de copias, sale el código.
    let v = Vault::open(&dir, keys.clone(), Arc::new(AlwaysConfirm)).unwrap();
    assert!(keys.is_secure());
    assert!(
        existe_en_el_sistema(&service),
        "el elemento aparece en el Llavero"
    );
    v.set_setting("company", &serde_json::json!({ "name": "EMPRESA DEMO SL" }))
        .unwrap();
    assert!(!recovery::exists(&dir));
    backup::set_passphrase_with(&v, PASS, FAST, time::OffsetDateTime::now_utc()).unwrap();
    assert!(recovery::exists(&dir), "recovery.sealed junto a taxpain.db");
    let db_key = keys.get(KeyId::DbMaster, "").unwrap().unwrap();
    drop(v);

    // 2. El usuario borra «TaxPain (db-master)» en Acceso a Llaveros.
    borrar_desde_el_sistema(&service);
    assert!(!existe_en_el_sistema(&service));
    assert!(matches!(
        Vault::open(&dir, keys.clone(), Arc::new(AlwaysConfirm)),
        Err(VaultError::KeyMissing { recoverable: true })
    ));
    assert!(
        keys.get(KeyId::DbMaster, "").unwrap().is_none(),
        "abrir sin clave no crea otra"
    );
    // Contraseña mala: no abre y no deja nada en el Llavero.
    assert!(matches!(
        Vault::open_recovered(&dir, keys.clone(), Arc::new(AlwaysConfirm), "otra"),
        Err(VaultError::BackupPassphrase)
    ));
    assert!(!existe_en_el_sistema(&service));
    // Contraseña buena: vuelven los datos y el elemento del Llavero, con la misma clave.
    let v = Vault::open_recovered(&dir, keys.clone(), Arc::new(AlwaysConfirm), PASS).unwrap();
    assert_eq!(
        v.setting::<serde_json::Value>("company").unwrap().unwrap()["name"],
        "EMPRESA DEMO SL"
    );
    assert_eq!(
        audit::entries(v.connection())
            .unwrap()
            .pop()
            .unwrap()
            .action,
        "vault.recovered"
    );
    assert!(existe_en_el_sistema(&service));
    assert_eq!(
        keys.get(KeyId::DbMaster, "").unwrap().unwrap().expose(),
        db_key.expose()
    );
    drop(v);
    // La siguiente vez abre sin pedir nada.
    let v = Vault::open(&dir, keys.clone(), Arc::new(AlwaysConfirm)).unwrap();
    assert!(backup::verify_passphrase(&v, PASS).unwrap());
    drop(v);

    // 3. Sin elemento y sin código de recuperación: «falta la clave», no «almacén dañado».
    borrar_desde_el_sistema(&service);
    std::fs::remove_file(recovery::path(&dir)).unwrap();
    let err = Vault::open(&dir, keys.clone(), Arc::new(AlwaysConfirm)).unwrap_err();
    assert!(
        matches!(err, VaultError::KeyMissing { recoverable: false }),
        "{err}"
    );
    assert!(err.to_string().contains("Llavero"), "{err}");
    assert!(!err.to_string().contains("dañado"), "{err}");

    // 4. Otro Mac con su propia `db-master`: la BD se recifra con ésa y el código se reescribe.
    //    (Se vuelve a dejar el código de recuperación con la clave original para simularlo.)
    let kek = {
        let keys2 = Arc::new(KeychainKeyProvider::with_service(
            KeychainMode::Legacy,
            &service,
        ));
        keys2.put(KeyId::DbMaster, &db_key).unwrap();
        let v = Vault::open(&dir, keys2.clone(), Arc::new(AlwaysConfirm)).unwrap();
        assert!(
            recovery::exists(&dir),
            "al abrir con contraseña se repone el código"
        );
        drop(v);
        keys2.delete(KeyId::DbMaster).unwrap();
        recovery::unseal(&dir, PASS).unwrap()
    };
    assert_eq!(kek.expose(), db_key.expose());
    let other = keys.get_or_create(KeyId::DbMaster, "").unwrap();
    assert_ne!(other.expose(), db_key.expose());
    assert!(matches!(
        Vault::open(&dir, keys.clone(), Arc::new(AlwaysConfirm)),
        Err(VaultError::Corrupted)
    ));
    let v = Vault::open_recovered(&dir, keys.clone(), Arc::new(AlwaysConfirm), PASS).unwrap();
    assert_eq!(
        v.setting::<serde_json::Value>("company").unwrap().unwrap()["name"],
        "EMPRESA DEMO SL"
    );
    drop(v);
    assert_eq!(
        recovery::unseal(&dir, PASS).unwrap().expose(),
        other.expose()
    );
    assert_eq!(
        keys.get(KeyId::DbMaster, "").unwrap().unwrap().expose(),
        other.expose()
    );

    // Limpieza: no queda nada en el Llavero.
    keys.delete(KeyId::DbMaster).unwrap();
    assert!(!existe_en_el_sistema(&service));
}
