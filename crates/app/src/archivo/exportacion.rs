//! Exportación completa de la empresa a formatos abiertos (G38), para no depender de TaxPain:
//! un ZIP con cada tabla en CSV y JSON y los documentos tal cual (ver
//! [`taxpain_vault::volcado`]), un `LEEME.txt` y un índice. Sin secretos (claves de los
//! certificados, de las copias ni de los servicios conectados). El ZIP no va cifrado: la
//! interfaz lo avisa y pide la confirmación de presencia antes. Queda en la auditoría.
//!
//! [`ZipDestino`] sirve también para el rescate de una empresa que no abre
//! ([`taxpain_vault::volcado::rescatar`]).

use std::io::Write;
use std::path::Path;

use serde::Serialize;
use sha2::{Digest, Sha256};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::volcado::{self, Destino, Volcado};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::error::{AppError, Result};

/// [`Destino`] que escribe en un ZIP en memoria, bajo la carpeta `raiz`.
#[derive(Debug)]
pub struct ZipDestino {
    zip: zip::ZipWriter<std::io::Cursor<Vec<u8>>>,
    raiz: String,
    pub ficheros: usize,
}

impl ZipDestino {
    pub fn new(raiz: &str) -> Self {
        ZipDestino {
            zip: zip::ZipWriter::new(std::io::Cursor::new(Vec::new())),
            raiz: raiz.to_owned(),
            ficheros: 0,
        }
    }

    /// Cierra el ZIP y devuelve sus bytes.
    pub fn terminar(self) -> Result<Vec<u8>> {
        Ok(self
            .zip
            .finish()
            .map_err(|e| AppError::Other(format!("ZIP: {e}")))?
            .into_inner())
    }
}

impl Destino for ZipDestino {
    fn fichero(&mut self, ruta: &str, bytes: &[u8]) -> taxpain_vault::Result<()> {
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .large_file(true);
        let err = |e: String| taxpain_vault::VaultError::InvalidInput(format!("ZIP: {e}"));
        self.zip
            .start_file(format!("{}/{ruta}", self.raiz), opts)
            .map_err(|e| err(e.to_string()))?;
        self.zip.write_all(bytes)?;
        self.ficheros += 1;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Exportado {
    pub fichero: String,
    pub sha256: String,
    pub bytes: u64,
    pub tablas: usize,
    pub filas: usize,
    pub adjuntos: usize,
    /// Tablas que no se han podido leer enteras.
    pub con_error: Vec<String>,
}

/// Texto del `LEEME.txt`: qué hay y cómo leerlo sin TaxPain.
pub fn leeme(empresa: &str, v: &Volcado, at: &str, rescate: bool) -> String {
    let mut t = format!(
        "{} DE {empresa}\nGenerado por TaxPain el {at} (esquema {}).\n\n",
        if rescate {
            "RESCATE DE LOS DATOS"
        } else {
            "EXPORTACIÓN COMPLETA"
        },
        v.esquema
    );
    t.push_str(
        "tablas/<tabla>.csv   Cada tabla de la base de datos, en CSV (UTF-8, comas, con \
         cabecera). Los importes van en céntimos (enteros) salvo que la columna diga otra cosa; \
         las fechas, AAAA-MM-DD.\ntablas/<tabla>.json  La misma tabla en JSON, con los valores \
         exactos.\nadjuntos/<tabla>/    Los documentos guardados (PDF de las facturas, \
         recibidas, justificantes, documentos de la empresa…), tal cual; la celda de la tabla \
         lleva su ruta.\n\nTablas principales: invoice_issued (facturas emitidas), \
         invoice_received (recibidas), journal_entry y journal_line (diario), document \
         (documentos), tax_return (modelos presentados), audit_log (registro de auditoría).\n\
         No salen los secretos: claves de los certificados, de las copias ni de los servicios \
         conectados.\n\nEste ZIP no va cifrado: guárdalo en un sitio seguro (un disco cifrado) \
         y bórralo cuando ya no lo necesites.\n\n",
    );
    for tb in &v.tablas {
        t.push_str(&format!(
            "  {:<40} {:>8} filas{}{}\n",
            tb.tabla,
            tb.filas,
            if tb.adjuntos > 0 {
                format!(", {} documentos", tb.adjuntos)
            } else {
                String::new()
            },
            tb.error
                .as_ref()
                .map(|e| format!("  (INCOMPLETA: {e})"))
                .unwrap_or_default()
        ));
    }
    t
}

/// Exporta la empresa entera a `dest` (ZIP) y su huella a `dest.sha256`. `confirmed`: el
/// usuario lo ha confirmado.
pub fn completa(v: &Vault, dest: &Path, now: OffsetDateTime, confirmed: bool) -> Result<Exportado> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let info = crate::companies::info(v)?;
    let at = now.format(&Rfc3339).unwrap_or_default();
    let mut z = ZipDestino::new("TaxPain - exportación completa");
    let vol = volcado::volcar(v.connection(), &mut z)?;
    z.fichero("LEEME.txt", leeme(&info.name, &vol, &at, false).as_bytes())?;
    let indice = serde_json::to_vec_pretty(&vol).map_err(|e| AppError::Other(e.to_string()))?;
    z.fichero("indice.json", &indice)?;
    let bytes = z.terminar()?;
    let sha = hex::encode(Sha256::digest(&bytes));
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    std::fs::write(dest, &bytes).map_err(taxpain_vault::VaultError::Io)?;
    let mut huella = dest.as_os_str().to_owned();
    huella.push(".sha256");
    std::fs::write(&huella, format!("{sha}  {name}\n")).map_err(taxpain_vault::VaultError::Io)?;
    let adjuntos = vol.tablas.iter().map(|t| t.adjuntos).sum();
    audit::append(
        v.connection(),
        &at,
        Actor::User,
        "archivo.exportacion_completa",
        &name,
        &serde_json::json!({ "sha256": sha, "bytes": bytes.len(), "tablas": vol.tablas.len(),
                             "filas": vol.filas(), "adjuntos": adjuntos }),
    )?;
    Ok(Exportado {
        fichero: name,
        sha256: sha,
        bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        tablas: vol.tablas.len(),
        filas: vol.filas(),
        adjuntos,
        con_error: vol
            .tablas
            .iter()
            .filter(|t| t.error.is_some())
            .map(|t| t.tabla.clone())
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider};
    use time::macros::datetime;

    #[test]
    fn la_exportacion_completa_lleva_tablas_leeme_y_huella() {
        let v = Vault::open_in_memory(
            Arc::new(MemoryKeyProvider::default()),
            Arc::new(AlwaysConfirm),
        )
        .unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("export.zip");
        let now = datetime!(2026-11-17 10:00 UTC);
        assert!(completa(&v, &dest, now, false).is_err());
        let r = completa(&v, &dest, now, true).unwrap();
        assert!(r.tablas > 50);
        assert!(r.con_error.is_empty());
        let bytes = std::fs::read(&dest).unwrap();
        assert_eq!(r.sha256, hex::encode(Sha256::digest(&bytes)));
        let z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let nombres: Vec<String> = z.file_names().map(|n| n.unwrap().into_owned()).collect();
        assert!(
            nombres
                .iter()
                .any(|n| n == "TaxPain - exportación completa/LEEME.txt")
        );
        assert!(
            nombres
                .iter()
                .any(|n| n == "TaxPain - exportación completa/tablas/invoice_issued.csv")
        );
        assert!(!nombres.iter().any(|n| n.contains("app_secret")));
        assert!(
            taxpain_vault::audit::entries(v.connection())
                .unwrap()
                .iter()
                .any(|e| e.action == "archivo.exportacion_completa")
        );
    }
}
