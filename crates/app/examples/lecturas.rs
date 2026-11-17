//! Herramienta para Claude Code (con la suscripción del usuario, sin API): ayuda a leer la
//! carpeta de la empresa y a comprobar `taxpain-lecturas.json` antes de importarlo en la app.
//!
//! ```text
//! cargo run -q -p taxpain-app --example lecturas -- hoja <fichero.xlsx|.xls|.ods|.csv> [filas]
//! cargo run -q -p taxpain-app --example lecturas -- comprobar <carpeta>
//! cargo run -q -p taxpain-app --example lecturas -- texto <fichero.pdf>
//! ```
//! `hoja` imprime la primera hoja con filas numeradas y columnas por letra (A=…, B=…) y, si el
//! libro tiene más, sus nombres (las columnas que se digan valen para todas las hojas con los
//! mismos títulos).
//! `comprobar` valida el fichero de lecturas y simula la importación (sin tocar ningún dato).
//! Dice también qué ficheros son copias de otro (el mismo contenido): sus lecturas se juntan en
//! una al importarlas.
//! `texto` imprime el texto del PDF tal como lo ve la app (el mismo lector que la importación).

#![allow(clippy::print_stdout, clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use taxpain_app::lectura_ia::{self, Reading, TIPOS};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("hoja") if args.len() >= 2 => hoja(&args[1], args.get(2).and_then(|n| n.parse().ok())),
        Some("comprobar") if args.len() >= 2 => comprobar(Path::new(&args[1])),
        Some("texto") if args.len() >= 2 => texto(&args[1]),
        _ => {
            eprintln!(
                "uso: lecturas hoja <fichero> [filas] | lecturas comprobar <carpeta> | \
                 lecturas texto <fichero.pdf>"
            );
            std::process::exit(2);
        }
    }
}

fn hoja(path: &str, rows: Option<usize>) {
    let bytes = std::fs::read(path).unwrap_or_else(|e| fail(&format!("{path}: {e}")));
    let name = Path::new(path)
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let hojas = lectura_ia::sheet_names(&name, &bytes);
    if hojas.len() > 1 {
        println!(
            "El libro tiene {} hojas: {} (abajo, la primera).",
            hojas.len(),
            hojas.join(", ")
        );
    }
    let r = taxpain_app_rows(&name, &bytes);
    print!("{}", lectura_ia::sheet_text_rows(&r, rows.unwrap_or(60)));
}

fn texto(path: &str) {
    let bytes = std::fs::read(path).unwrap_or_else(|e| fail(&format!("{path}: {e}")));
    match taxpain_app::pdf_text::text(&bytes) {
        Some(t) => print!("{t}"),
        None => fail("la app no saca texto de este PDF"),
    }
}

fn taxpain_app_rows(name: &str, bytes: &[u8]) -> Vec<Vec<String>> {
    // Las mismas filas que verá la importación.
    lectura_ia::sheet_rows(name, bytes).unwrap_or_else(|e| fail(&e))
}

fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}");
    std::process::exit(1);
}

fn comprobar(root: &Path) {
    let path = root.join(lectura_ia::PACKAGE_FILE);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| fail(&format!("{}: {e}", path.display())));
    let v: serde_json::Value =
        serde_json::from_str(&text).unwrap_or_else(|e| fail(&format!("JSON no válido: {e}")));
    let list = v
        .get("lecturas")
        .and_then(|l| l.as_array())
        .unwrap_or_else(|| fail("falta la lista «lecturas»"));
    let vault = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let mut ok = 0;
    let mut problems = 0;
    // Las lecturas de cada contenido (las copias de un fichero en otras carpetas).
    let mut by_content: BTreeMap<String, Vec<(usize, String, Reading)>> = BTreeMap::new();
    for (i, e) in list.iter().enumerate() {
        let fichero = e.get("fichero").and_then(|f| f.as_str()).unwrap_or("");
        // Lo mismo que acepta la app: `null` vale por «no aparece».
        let reading: Option<Reading> = e.get("lectura").and_then(lectura_ia::reading_from_value);
        let report = |msg: String| println!("✗ [{i}] {fichero}: {msg}");
        let Some(r) = reading else {
            report("«lectura» no tiene el formato esperado".into());
            problems += 1;
            continue;
        };
        if !TIPOS.contains(&r.tipo.as_str()) {
            report(format!("tipo desconocido «{}»", r.tipo));
            problems += 1;
            continue;
        }
        let Ok((name, bytes)) = taxpain_app::carpeta::read_file(root, fichero) else {
            report("el fichero no está en la carpeta".into());
            problems += 1;
            continue;
        };
        by_content
            .entry(hex::encode(Sha256::digest(&bytes)))
            .or_default()
            .push((i, fichero.to_owned(), r.clone()));
        match lectura_ia::simulate(&vault, &name, &bytes, &r) {
            Ok(msg) => {
                ok += 1;
                println!("✓ {fichero}: {} — {msg}", lectura_ia::label(&r));
            }
            Err(msg) => {
                problems += 1;
                report(msg);
            }
        }
    }
    let mut copies = 0;
    for group in by_content.values().filter(|g| g.len() > 1) {
        copies += group.len() - 1;
        let files: Vec<String> = group.iter().map(|(i, f, _)| format!("[{i}] {f}")).collect();
        let readings: Vec<Reading> = group.iter().map(|(_, _, r)| r.clone()).collect();
        if readings.iter().all(|r| *r == readings[0]) {
            println!(
                "ℹ mismo contenido, con la misma lectura: {}",
                files.join(", ")
            );
        } else {
            let m = lectura_ia::merge_readings(&readings);
            println!(
                "ℹ mismo contenido, con lecturas distintas: {}. Al importar se juntan en una: \
                 manda la que dice qué es y trae más datos ({}: {}), y se le añade lo que le \
                 falte de las demás del mismo tipo o «otro» (los importes, enteros de una sola)",
                files.join(", "),
                lectura_ia::label(&m),
                m.descripcion
            );
        }
    }
    match copies {
        0 => {}
        1 => println!("1 lectura es de una copia de otro fichero: cuenta una por contenido."),
        n => println!("{n} lecturas son de copias de otro fichero: cuenta una por contenido."),
    }
    println!("\n{ok} lecturas bien, {problems} con problemas.");
    if problems > 0 {
        std::process::exit(1);
    }
}
