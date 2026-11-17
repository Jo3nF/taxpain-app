//! Copias de seguridad desde la app: copia a mano, restauración en otro Mac con la emisión
//! bloqueada hasta comprobar la numeración, y copias automáticas con rotación. Una copia puede
//! venir de otra persona: la empresa restaurada no hereda su contraseña ni sus rutas.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::backup::{self, BackupSettings};
use taxpain_app::{invoicing, verifactu_lab as lab};
use taxpain_core::invoice::{Party, PartyId};
use taxpain_vault::backup as vault_backup;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::Duration;
use time::macros::{date, datetime};

const PASS: &str = "una contraseña larga";

#[test]
fn copia_restauracion_y_automaticas() {
    let tmp = tempfile::tempdir().unwrap();
    let keys = Arc::new(MemoryKeyProvider::default());
    let v = Vault::open(&tmp.path().join("empresa"), keys, Arc::new(AlwaysConfirm)).unwrap();
    let now = datetime!(2026-10-26 10:00 UTC);
    // Las facturas del 27 se emiten el 27: una fecha de expedición futura ya no se admite.
    let now27 = datetime!(2026-10-27 10:00 UTC);
    let issuer = Party {
        id: PartyId::Nif("B12345674".into()),
        name: "EJEMPLO PRUEBAS SL".into(),
        address: addr(),
        person: None,
    };
    invoicing::save_settings(&v, &lab::settings_for(issuer), now).unwrap();
    lab::mark(&v, now).unwrap();
    let numbers = lab::generate(&v, date!(2026 - 10 - 26), now).unwrap();

    // Sin copias todavía: el panel lo recuerda.
    let st = backup::status(&v, "Ejemplo SL", None, now).unwrap();
    assert!(st.remind && !st.has_passphrase && st.last.is_none());
    assert_eq!(st.suggested_name, "Ejemplo-SL-2026-10-26.taxpain");

    backup::set_passphrase(&v, PASS, now).unwrap();
    assert!(backup::verify_passphrase(&v, PASS).unwrap());
    let dest = tmp.path().join("copias").join(&st.suggested_name);
    assert!(
        backup::create(&v, &tmp.path().join("copia.zip"), false, "0.1.0", now).is_err(),
        "sólo .taxpain"
    );
    let report = backup::create(&v, &dest, false, "0.1.0", now).unwrap();
    let st = backup::status(&v, "Ejemplo SL", None, now + Duration::days(3)).unwrap();
    assert_eq!(st.days_since, Some(3));
    assert!(!st.remind);
    assert_eq!(st.last.unwrap().path, report.path);

    // Lo que se ve antes de restaurar.
    let summary = backup::inspect(&dest, PASS).unwrap();
    assert_eq!(summary.company, "EJEMPLO PRUEBAS SL");
    assert_eq!(summary.nif.as_deref(), Some("B12345674"));
    assert_eq!(summary.invoices_issued, 4);
    assert!(!summary.newer_version);
    assert!(backup::inspect(&dest, "contraseña mala").is_err());

    // Otro Mac.
    let other = Arc::new(MemoryKeyProvider::default());
    let (restored, header) = vault_backup::restore(
        &dest,
        PASS,
        &tmp.path().join("otro-mac"),
        other,
        Arc::new(AlwaysConfirm),
        now,
    )
    .unwrap();
    backup::mark_restored(&restored, &header, now).unwrap();
    // No se emite hasta comprobar que la última factura de la copia es la última de verdad.
    let e = lab::generate(&restored, date!(2026 - 10 - 27), now27)
        .unwrap_err()
        .to_string();
    assert!(e.contains("copia de seguridad"), "{e}");
    // Tampoco se anula, se subsana ni se envía: cualquier registro nuevo bifurcaría la cadena.
    let e = lab::cancel_one(&restored, now).unwrap_err().to_string();
    assert!(e.contains("copia de seguridad"), "{e}");
    let record: i64 = restored
        .connection()
        .query_row("SELECT max(id) FROM verifactu_record", [], |r| r.get(0))
        .unwrap();
    let e = invoicing::resubmit(&restored, record, now, true)
        .unwrap_err()
        .to_string();
    assert!(e.contains("copia de seguridad"), "{e}");
    let e = invoicing::pending_submission(&restored, 10, invoicing::Entorno::Pruebas)
        .unwrap_err()
        .to_string();
    assert!(e.contains("copia de seguridad"), "{e}");
    let check = backup::restore_check(&restored).unwrap().unwrap();
    assert!(
        check
            .last_by_series
            .iter()
            .any(|(_, n, _)| n == numbers.last().unwrap()),
        "{check:?}"
    );
    assert!(check.last_verifactu.is_some());
    backup::acknowledge_restore(&restored, now).unwrap();
    assert!(lab::generate(&restored, date!(2026 - 10 - 27), now27).is_ok());
    // La carpeta y las copias automáticas del Mac anterior no se heredan. Tampoco la contraseña
    // de la copia: la app pide una nueva al abrir la empresa.
    let st = backup::status(&restored, "Ejemplo SL", None, now).unwrap();
    assert!(!st.has_passphrase && st.setup_required && !st.recovery_code);
    assert!(!st.settings.auto && st.last.is_none());
    drop(restored);

    // Copias automáticas: semanales, se conservan 2.
    let folder = tmp.path().join("nube/Copias de seguridad");
    assert!(
        backup::save_settings(
            &v,
            &BackupSettings {
                folder: None,
                auto: true,
                every_days: 7,
                keep: 2
            }
        )
        .is_err(),
        "sin carpeta no hay automáticas"
    );
    backup::save_settings(
        &v,
        &BackupSettings {
            folder: Some(folder.clone()),
            auto: true,
            every_days: 7,
            keep: 2,
        },
    )
    .unwrap();
    // Una copia hecha a mano en la misma carpeta no se borra nunca.
    let manual = folder.join("Ejemplo-SL-2026-10-01.taxpain");
    backup::create(&v, &manual, false, "0.1.0", now).unwrap();
    // Una automática de otra empresa con el mismo nombre (otra marca) tampoco; una de esta
    // empresa con el nombre de antes de la marca, sí entra en la rotación.
    let other = folder.join("Ejemplo-SL-2026-10-02-auto-zzzzzzzz.taxpain");
    std::fs::copy(&manual, &other).unwrap();
    let legacy = folder.join("Ejemplo-SL-2026-10-03-auto.taxpain");
    std::fs::copy(&manual, &legacy).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    // La última es de hoy: no toca.
    assert!(
        backup::run_auto(&v, "Ejemplo SL", "0.1.0", now)
            .unwrap()
            .is_none()
    );
    let mut t = now;
    for _ in 0..3 {
        t += Duration::days(7);
        let r = backup::run_auto(&v, "Ejemplo SL", "0.1.0", t).unwrap();
        assert!(r.is_some());
        // Que la fecha de modificación sea distinta entre copias.
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let mut names: Vec<String> = std::fs::read_dir(&folder)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let tag: String = v.setting("backup_tag").unwrap().unwrap();
    assert_eq!(
        names,
        vec![
            "Ejemplo-SL-2026-10-01.taxpain".to_owned(),
            "Ejemplo-SL-2026-10-02-auto-zzzzzzzz.taxpain".to_owned(),
            format!("Ejemplo-SL-2026-11-09-auto-{tag}.taxpain"),
            format!("Ejemplo-SL-2026-11-16-auto-{tag}.taxpain"),
        ]
    );
    let st = backup::status(&v, "Ejemplo SL", None, t).unwrap();
    assert!(st.last.unwrap().auto);
    assert!(st.last_error.is_none());
}

/// Alta de la empresa (DATOS-SEGURIDAD-01): una empresa nueva sin contraseña de copias la pide
/// antes de empezar; al definirla queda el código de recuperación junto a los datos. Una empresa
/// que ya tiene datos no se bloquea: la elige desde Ajustes.
#[test]
fn contrasena_en_el_alta_y_codigo_de_recuperacion() {
    let tmp = tempfile::tempdir().unwrap();
    let keys = Arc::new(MemoryKeyProvider::default());
    let now = datetime!(2026-10-26 10:00 UTC);

    let nueva = Vault::open(
        &tmp.path().join("nueva"),
        keys.clone(),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let st = backup::status(&nueva, "Nueva SL", None, now).unwrap();
    assert!(st.setup_required && !st.has_passphrase && !st.recovery_code);
    assert!(backup::set_passphrase(&nueva, "aaaaaaaaaaaa", now).is_err());
    backup::set_passphrase(&nueva, PASS, now).unwrap();
    let st = backup::status(&nueva, "Nueva SL", None, now).unwrap();
    assert!(!st.setup_required && st.has_passphrase && st.recovery_code);

    let antigua = Vault::open(&tmp.path().join("antigua"), keys, Arc::new(AlwaysConfirm)).unwrap();
    let issuer = Party {
        id: PartyId::Nif("B12345674".into()),
        name: "EMPRESA DEMO SL".into(),
        address: addr(),
        person: None,
    };
    invoicing::save_settings(&antigua, &lab::settings_for(issuer), now).unwrap();
    lab::mark(&antigua, now).unwrap();
    lab::generate(&antigua, date!(2026 - 10 - 26), now).unwrap();
    let st = backup::status(&antigua, "Demo SL", None, now).unwrap();
    assert!(!st.setup_required && !st.has_passphrase && !st.recovery_code);

    // La empresa de pruebas VERI*FACTU, aunque esté vacía, no la pide.
    let pruebas = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    lab::mark(&pruebas, now).unwrap();
    assert!(
        !backup::status(&pruebas, "P", None, now)
            .unwrap()
            .setup_required
    );

    // En memoria no hay carpeta ni código de recuperación.
    let mem = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    backup::set_passphrase(&mem, PASS, now).unwrap();
    assert!(!backup::status(&mem, "X", None, now).unwrap().recovery_code);
}

fn issuer() -> Party {
    Party {
        id: PartyId::Nif("B12345674".into()),
        name: "EJEMPLO PRUEBAS SL".into(),
        address: addr(),
        person: None,
    }
}

/// Una copia puede venir de otra persona. La empresa restaurada no hereda la contraseña de la
/// copia (queda sin contraseña y la app pide una nueva) ni los ajustes que son rutas de carpetas
/// (las vigiladas, que un hilo de fondo lee cada dos minutos; la carpeta de la empresa; el
/// destino de las facturas): ésos los ha elegido quien hizo la copia.
#[test]
fn la_empresa_restaurada_no_hereda_la_contrasena_ni_las_rutas_de_la_copia() {
    use taxpain_app::settings::{Destination, DestinationKind, InvoicingSettings};
    use taxpain_app::{carpeta, vigiladas};

    let tmp = tempfile::tempdir().unwrap();
    let keys = Arc::new(MemoryKeyProvider::default());
    let v = Vault::open(&tmp.path().join("empresa"), keys, Arc::new(AlwaysConfirm)).unwrap();
    let now = datetime!(2026-10-26 10:00 UTC);
    let facturas = tmp.path().join("facturas-de-otro");
    let vigilada = tmp.path().join("vigilada-de-otro");
    for d in [&facturas, &vigilada] {
        std::fs::create_dir_all(d).unwrap();
    }
    let settings = InvoicingSettings {
        destination: Some(Destination {
            kind: DestinationKind::Local,
            path: facturas.clone(),
        }),
        ..lab::settings_for(issuer())
    };
    invoicing::save_settings(&v, &settings, now).unwrap();
    vigiladas::set_folders(&v, &[vigilada.to_string_lossy().into_owned()]).unwrap();
    v.set_setting(vigiladas::OLD_KEY, &Some("/Users/otro/Facturas".to_owned()))
        .unwrap();
    v.set_setting(
        "carpetas_vigiladas_vistos",
        &serde_json::json!({ "/Users/otro/Facturas/a.pdf": [10, 20] }),
    )
    .unwrap();
    v.set_setting(
        "carpetas_vigiladas_pendientes",
        &serde_json::json!([{ "folder": "/Users/otro/Facturas", "path": "b.pdf",
            "kind": "Extracto bancario", "year": 2026, "seen": "2026-10-26" }]),
    )
    .unwrap();
    // Cualquier otro ajuste de las carpetas vigiladas (los hay que guardan rutas de ficheros).
    for key in ["carpetas_vigiladas_fallidos", "carpetas_vigiladas_en_curso"] {
        v.set_setting(
            key,
            &serde_json::json!({ "path": "/Users/otro/Facturas/c.pdf" }),
        )
        .unwrap();
    }
    carpeta::set_folder(&v, "/Users/otro/Empresa").unwrap();
    backup::set_passphrase(&v, PASS, now).unwrap();
    let dest = tmp.path().join("copia.taxpain");
    backup::create(&v, &dest, false, "0.1.0", now).unwrap();

    let (restored, header) = vault_backup::restore(
        &dest,
        PASS,
        &tmp.path().join("otro-mac"),
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
        now,
    )
    .unwrap();
    backup::mark_restored(&restored, &header, now).unwrap();

    // Ninguna ruta de la copia.
    assert!(vigiladas::folders(&restored).unwrap().is_empty());
    assert!(vigiladas::pending(&restored).unwrap().is_empty());
    assert_eq!(carpeta::folder(&restored).unwrap(), None);
    for key in [
        "carpetas_vigiladas",
        "received_watched_folder",
        "carpetas_vigiladas_vistos",
        "carpetas_vigiladas_pendientes",
        "carpetas_vigiladas_fallidos",
        "carpetas_vigiladas_en_curso",
        "carpeta_empresa",
    ] {
        assert!(
            v.setting::<serde_json::Value>(key).unwrap().is_some(),
            "{key} está en la empresa de origen"
        );
        assert!(
            restored
                .setting::<serde_json::Value>(key)
                .unwrap()
                .is_none(),
            "{key}"
        );
    }
    // Las facturas no se guardan en la carpeta que decía la copia; el resto de los ajustes de
    // facturación (emisor, series, VERI*FACTU…) llega igual.
    assert_eq!(
        invoicing::settings(&restored).unwrap().unwrap(),
        InvoicingSettings {
            destination: None,
            ..settings.clone()
        }
    );

    // Sin contraseña de copias ni código de recuperación: la app la pide al abrirla.
    let st = backup::status(&restored, "Ejemplo SL", None, now).unwrap();
    assert!(!st.has_passphrase && !st.recovery_code && st.setup_required);
    assert!(st.restored.is_some());
    assert!(!backup::verify_passphrase(&restored, PASS).unwrap());
    backup::set_passphrase(&restored, "otra frase mía larga", now).unwrap();
    let st = backup::status(&restored, "Ejemplo SL", None, now).unwrap();
    assert!(st.has_passphrase && st.recovery_code && !st.setup_required);

    // La empresa de origen sigue como estaba.
    assert_eq!(vigiladas::folders(&v).unwrap().len(), 1);
    assert_eq!(
        invoicing::settings(&v).unwrap().unwrap().destination,
        settings.destination
    );
}

/// Lo que se enseña de una copia antes de restaurarla sale sólo de sus tablas: si la copia trae
/// una vista con el nombre de una tabla, no se lee (ejecutaría la consulta de quien la hizo).
#[test]
fn la_vista_previa_de_una_copia_solo_lee_tablas() {
    let tmp = tempfile::tempdir().unwrap();
    let keys = Arc::new(MemoryKeyProvider::default());
    let v = Vault::open(&tmp.path().join("empresa"), keys, Arc::new(AlwaysConfirm)).unwrap();
    let now = datetime!(2026-10-26 10:00 UTC);
    v.set_setting(
        "company",
        &serde_json::json!({ "name": "EJEMPLO SL", "nif": "B12345674" }),
    )
    .unwrap();
    backup::set_passphrase(&v, PASS, now).unwrap();
    let buena = tmp.path().join("buena.taxpain");
    vault_backup::export(&v, &buena, "0.1.0", now).unwrap();
    let summary = backup::inspect(&buena, PASS).unwrap();
    assert_eq!(summary.company, "EJEMPLO SL");
    assert_eq!(summary.nif.as_deref(), Some("B12345674"));

    v.connection()
        .execute_batch(
            r#"ALTER TABLE settings RENAME TO settings_de_verdad;
               CREATE VIEW settings AS
                 SELECT key, '{"name":"COLADA SL","nif":"B12345674"}' AS value
                   FROM settings_de_verdad;
               ALTER TABLE journal_entry RENAME TO journal_entry_de_verdad;
               CREATE VIEW journal_entry AS SELECT 1 AS id UNION ALL SELECT 2;"#,
        )
        .unwrap();
    let mala = tmp.path().join("mala.taxpain");
    vault_backup::export(&v, &mala, "0.1.0", now).unwrap();
    let summary = backup::inspect(&mala, PASS).unwrap();
    assert_eq!(summary.company, "Empresa sin nombre");
    assert_eq!(summary.nif, None);
    assert_eq!(summary.journal_entries, 0);
    // Y no se restaura.
    assert!(
        vault_backup::restore(
            &mala,
            PASS,
            &tmp.path().join("otro-mac"),
            Arc::new(MemoryKeyProvider::default()),
            Arc::new(AlwaysConfirm),
            now,
        )
        .is_err()
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
