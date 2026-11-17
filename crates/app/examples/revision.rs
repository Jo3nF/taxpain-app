//! Revisión de la carpeta de una empresa desde la terminal, para probarla con carpetas reales sin
//! abrir la app. Usa un almacén en memoria: lee `taxpain-lecturas.json` si está en la carpeta.
//!
//! ```text
//! cargo run -q -p taxpain-app --example revision -- <carpeta> [--nif NIF] [--nombre NOMBRE]
//!     [--sl] [--json] [--pdf fichero.pdf] [--xlsx fichero.xlsx]
//! ```
//! `--nif` y `--nombre` son los de la empresa (para saber qué facturas son suyas), `--sl` usa el
//! perfil fiscal de una sociedad limitada (para los modelos que faltan) y `--json` saca la
//! revisión en JSON en vez de en Markdown.

#![allow(clippy::print_stdout, clippy::unwrap_used)]

use std::path::Path;
use std::sync::Arc;

use taxpain_app::companies::{self, CompanyInfo};
use taxpain_app::revision;
use taxpain_core::calendar::TaxpayerProfile;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::OffsetDateTime;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |f: &str| {
        args.iter()
            .position(|a| a == f)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let has = |f: &str| args.iter().any(|a| a == f);
    let Some(root) = args.first() else {
        eprintln!(
            "uso: revision <carpeta> [--nif NIF] [--nombre NOMBRE] [--sl] [--json] [--pdf f] [--xlsx f]"
        );
        std::process::exit(2);
    };
    let now = OffsetDateTime::now_utc();
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    if let Some(nif) = flag("--nif") {
        let info = CompanyInfo {
            name: flag("--nombre").unwrap_or_default(),
            nif: Some(nif),
        };
        companies::set_info(&v, &info, now).unwrap();
    }
    if has("--sl") {
        v.set_setting("taxpayer_profile", &TaxpayerProfile::sociedad_limitada())
            .unwrap();
    }
    let r = revision::revisar(&v, Path::new(root), now.date()).unwrap();
    if has("--json") {
        println!("{}", serde_json::to_string_pretty(&r).unwrap());
    } else {
        print!("{}", revision::informe::markdown(&r));
    }
    if let Some(f) = flag("--pdf") {
        std::fs::write(f, revision::informe::pdf(&r).unwrap()).unwrap();
    }
    if let Some(f) = flag("--xlsx") {
        std::fs::write(f, revision::informe::excel(&r).unwrap()).unwrap();
    }
}
