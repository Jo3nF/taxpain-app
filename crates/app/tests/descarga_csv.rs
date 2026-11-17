//! «Descargar de la AEAT por CSV»: dónde se guarda, cómo se llama, que no se pisa ni se duplica
//! nada y qué se dice de cada código. Datos inventados.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::carpeta;
use taxpain_app::descarga_csv::{self, Fetched, SUBFOLDER, Saved, State};
use taxpain_invoicing::pdf::{Para, text_document};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};

const CSV: &str = "ABCD1234EFGH5678";
const OTRO: &str = "Z9Y8X7W6V5U4T3S2";

fn vault() -> Vault {
    Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap()
}

fn pdf(lines: &[&str]) -> Vec<u8> {
    let doc = lines.iter().map(|l| Para::Text((*l).into())).collect();
    text_document("Documento", &[doc]).unwrap()
}

fn requerimiento() -> Vec<u8> {
    pdf(&[
        "Agencia Tributaria",
        "Requerimiento de información",
        "EMPRESA DEMO SL - B12345674",
    ])
}

#[test]
fn sin_carpeta_elegida_lo_dice() {
    let v = vault();
    let e = descarga_csv::root(&v).unwrap_err().to_string();
    assert!(e.contains("Elige primero la carpeta"), "{e}");
    // Una carpeta recordada que ya no existe.
    carpeta::set_folder(&v, "/no/existe/taxpain-prueba").unwrap();
    let e = descarga_csv::root(&v).unwrap_err().to_string();
    assert!(e.contains("ya no está"), "{e}");
    let dir = tempfile::tempdir().unwrap();
    carpeta::set_folder(&v, &dir.path().to_string_lossy()).unwrap();
    assert_eq!(descarga_csv::root(&v).unwrap(), dir.path());
}

#[test]
fn guarda_en_documentos_aeat_con_un_nombre_legible() {
    let dir = tempfile::tempdir().unwrap();
    let req = requerimiento();
    let saved = descarga_csv::save(dir.path(), CSV, &req).unwrap();
    let expected = format!("{SUBFOLDER}/Requerimiento de información - {CSV}.pdf");
    assert_eq!(saved, Saved::New(expected.clone()));
    assert_eq!(std::fs::read(dir.path().join(&expected)).unwrap(), req);
    // Sin título reconocible: sólo el CSV.
    let saved = descarga_csv::save(dir.path(), OTRO, &pdf(&["EMPRESA DEMO SL"])).unwrap();
    assert_eq!(saved, Saved::New(format!("{SUBFOLDER}/{OTRO}.pdf")));
    // Y la lectura de la carpeta los ve.
    let v = vault();
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    assert!(
        inv.items.iter().any(|i| i.path == expected),
        "{:?}",
        inv.items.iter().map(|i| &i.path).collect::<Vec<_>>()
    );
}

#[test]
fn no_sobrescribe_ni_duplica() {
    let dir = tempfile::tempdir().unwrap();
    let req = requerimiento();
    let first = descarga_csv::save(dir.path(), CSV, &req).unwrap();
    let Saved::New(path) = first else {
        panic!("{first:?}")
    };
    // El mismo CSV otra vez (aunque el PDF cambie): ya estaba, y el fichero no se toca.
    let other = pdf(&["Justificante de presentación"]);
    assert_eq!(
        descarga_csv::save(dir.path(), CSV, &other).unwrap(),
        Saved::Already(path.clone())
    );
    assert_eq!(std::fs::read(dir.path().join(&path)).unwrap(), req);
    // El mismo contenido con otro nombre (bajado a mano): ya estaba.
    std::fs::write(dir.path().join(SUBFOLDER).join("bajado a mano.pdf"), &other).unwrap();
    assert_eq!(
        descarga_csv::save(dir.path(), OTRO, &other).unwrap(),
        Saved::Already(format!("{SUBFOLDER}/bajado a mano.pdf"))
    );
    // Nada se ha añadido ni pisado: el descargado y el bajado a mano.
    let n = std::fs::read_dir(dir.path().join(SUBFOLDER))
        .unwrap()
        .count();
    assert_eq!(n, 2);
}

#[test]
fn lo_que_no_es_un_pdf_no_se_guarda() {
    let dir = tempfile::tempdir().unwrap();
    let e = descarga_csv::save(dir.path(), CSV, b"<html>Mantenimiento</html>")
        .unwrap_err()
        .to_string();
    assert!(e.contains("no es un PDF"), "{e}");
    assert!(!dir.path().join(SUBFOLDER).exists());
    // Un CSV mal formado tampoco (no sale de la subcarpeta).
    assert!(descarga_csv::save(dir.path(), "../../../etc/x", &requerimiento()).is_err());
}

#[test]
fn una_subcarpeta_que_no_es_carpeta_no_se_usa() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(SUBFOLDER), b"x").unwrap();
    let e = descarga_csv::save(dir.path(), CSV, &requerimiento())
        .unwrap_err()
        .to_string();
    assert!(e.contains("no es una carpeta"), "{e}");
}

#[cfg(unix)]
#[test]
fn un_enlace_llamado_documentos_aeat_no_se_sigue() {
    let dir = tempfile::tempdir().unwrap();
    let otra = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(otra.path(), dir.path().join(SUBFOLDER)).unwrap();
    let e = descarga_csv::save(dir.path(), CSV, &requerimiento())
        .unwrap_err()
        .to_string();
    assert!(e.contains("no es una carpeta"), "{e}");
    let e = descarga_csv::prepare(dir.path()).unwrap_err().to_string();
    assert!(e.contains("no es una carpeta"), "{e}");
    // Nada se ha escrito a través del enlace.
    assert_eq!(std::fs::read_dir(otra.path()).unwrap().count(), 0);
    assert_eq!(descarga_csv::already(dir.path(), CSV), None);
}

#[test]
fn si_el_nombre_esta_ocupado_usa_otro_sin_pisar() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join(SUBFOLDER);
    let name = format!("Requerimiento de información - {CSV}.pdf");
    // Una carpeta con el nombre que tocaría.
    std::fs::create_dir_all(sub.join(&name)).unwrap();
    let saved = descarga_csv::save(dir.path(), CSV, &requerimiento()).unwrap();
    assert_eq!(
        saved,
        Saved::New(format!(
            "{SUBFOLDER}/Requerimiento de información - {CSV} (2).pdf"
        ))
    );
    assert!(sub.join(&name).is_dir());
}

#[test]
fn un_pdf_cortado_no_cuenta_como_ya_descargado() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join(SUBFOLDER);
    std::fs::create_dir(&sub).unwrap();
    // Lo que queda si la app se cierra a mitad de escribir (de una versión anterior).
    let req = requerimiento();
    let cut = format!("Requerimiento de información - {CSV}.pdf");
    std::fs::write(sub.join(&cut), &req[..req.len() / 2]).unwrap();
    assert_eq!(descarga_csv::already(dir.path(), CSV), None);
    let saved = descarga_csv::save(dir.path(), CSV, &req).unwrap();
    let Saved::New(path) = saved else {
        panic!("{saved:?}")
    };
    assert!(path.ends_with(" (2).pdf"), "{path}");
    assert_eq!(std::fs::read(dir.path().join(&path)).unwrap(), req);
    // El cortado no se ha tocado y, a partir de ahora, el bueno «ya estaba».
    assert_eq!(
        std::fs::read(sub.join(&cut)).unwrap(),
        &req[..req.len() / 2]
    );
    assert_eq!(descarga_csv::already(dir.path(), CSV), Some(path));
}

#[test]
fn no_deja_ficheros_temporales() {
    let dir = tempfile::tempdir().unwrap();
    descarga_csv::prepare(dir.path()).unwrap();
    descarga_csv::save(dir.path(), CSV, &requerimiento()).unwrap();
    let names: Vec<String> = std::fs::read_dir(dir.path().join(SUBFOLDER))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(!names[0].starts_with('.'), "{names:?}");
}

#[test]
fn lo_que_ya_esta_se_ve_antes_de_pedirlo() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(descarga_csv::already(dir.path(), CSV), None);
    let Saved::New(path) = descarga_csv::save(dir.path(), CSV, &requerimiento()).unwrap() else {
        panic!()
    };
    assert_eq!(descarga_csv::already(dir.path(), CSV), Some(path));
    assert_eq!(descarga_csv::already(dir.path(), OTRO), None);
}

#[cfg(unix)]
#[test]
fn sin_permiso_para_escribir_un_solo_error_claro() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join(SUBFOLDER);
    std::fs::create_dir(&sub).unwrap();
    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o555)).unwrap();
    let probe = sub.join(".probe");
    // Como administrador (root) se puede escribir igual: entonces no hay nada que probar.
    if std::fs::write(&probe, b"x").is_ok() {
        let _ = std::fs::remove_file(&probe);
        std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    let e = descarga_csv::prepare(dir.path()).unwrap_err().to_string();
    std::fs::set_permissions(&sub, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(e.contains("permiso") && e.contains(SUBFOLDER), "{e}");
    assert!(!e.contains("os error"), "{e}");
}

#[test]
fn que_paso_con_cada_codigo() {
    let dir = tempfile::tempdir().unwrap();
    let item = descarga_csv::store(dir.path(), CSV, Fetched::Pdf(requerimiento()));
    assert!(matches!(&item.state, State::Guardado { path } if path.starts_with(SUBFOLDER)));
    let again = descarga_csv::store(dir.path(), CSV, Fetched::Pdf(requerimiento()));
    assert!(matches!(again.state, State::YaEstaba { .. }));
    assert_eq!(
        descarga_csv::store(dir.path(), OTRO, Fetched::NotFound).state,
        State::NoEncontrado
    );
    assert_eq!(
        descarga_csv::store(
            dir.path(),
            OTRO,
            Fetched::Annulled(Some(CSV.to_lowercase()))
        )
        .state,
        State::Anulado {
            replacement: Some(CSV.into())
        }
    );
    // Un sustituto raro o el mismo código no se ofrecen.
    for bad in ["x", OTRO] {
        assert_eq!(
            descarga_csv::store(dir.path(), OTRO, Fetched::Annulled(Some(bad.into()))).state,
            State::Anulado { replacement: None }
        );
    }
    assert!(matches!(
        descarga_csv::store(dir.path(), OTRO, Fetched::Pdf(b"no".to_vec())).state,
        State::Error { message } if message.contains("no es un PDF")
    ));
    assert!(matches!(
        descarga_csv::store(dir.path(), OTRO, Fetched::NotAvailable("x".into())).state,
        State::NoDisponible { .. }
    ));
    // Lo que ve la pantalla.
    let json = serde_json::to_value(&item).unwrap();
    assert_eq!(json["csv"], CSV);
    assert_eq!(json["state"], "guardado");
    assert!(json["path"].as_str().unwrap().ends_with(".pdf"));
}
