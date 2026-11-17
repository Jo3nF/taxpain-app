//! Ajustes de facturación (onboarding): datos del emisor, formato, salidas, carpeta destino y
//! VERI*FACTU. Se guardan cifrados en `settings` (clave `invoicing`).
//!
//! Carpetas destino (D10): iCloud Drive, Google Drive y Dropbox se usan a través de la carpeta
//! que sincroniza su app de escritorio en el Mac; no hay OAuth ni servidor propio.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use taxpain_core::invoice::{Party, RoundingMode};
use taxpain_core::money::Rate;

pub const SETTINGS_KEY: &str = "invoicing";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    #[default]
    Es,
    En,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DestinationKind {
    Local,
    ICloudDrive,
    GoogleDrive,
    Dropbox,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Destination {
    pub kind: DestinationKind,
    /// Carpeta raíz donde se guardan `AAAA/numero.pdf`.
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outputs {
    pub pdf: bool,
    /// Facturae 3.2.2 (`.xml`, sin firma).
    pub facturae: bool,
    /// Factura electrónica UBL (EN 16931) para la facturación electrónica B2B (`.ubl.xml`).
    #[serde(default)]
    pub ubl: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifactuSettings {
    /// Genera el registro de facturación encadenado de cada factura. El QR y la leyenda
    /// (Orden HAC/1177/2024 arts. 20 y 21) sólo se imprimen cuando el envío a la AEAT es real
    /// (`production`) o en la empresa del banco de pruebas: ver `invoicing::issue`.
    pub enabled: bool,
    /// Identificador único de esta instalación (`NumeroInstalacion`).
    pub installation_id: String,
    /// `false` = el QR apunta al entorno de pruebas de la AEAT. Producción sólo cuando el responsable lo
    /// apruebe y la app esté firmada (D6).
    pub production: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvoicingSettings {
    pub issuer: Party,
    pub default_series: String,
    pub rectifying_series: String,
    /// Serie de las facturas simplificadas, separada de la de las completas (RD 1619/2012
    /// art. 7.1.a último párrafo). Las instalaciones anteriores a este campo toman «S».
    #[serde(default = "default_simplified_series")]
    pub simplified_series: String,
    /// Serie de las rectificativas de facturas simplificadas (R5): son simplificadas, así que
    /// no pueden ir en la serie de rectificativas de completas (art. 7.1.a último párrafo del
    /// RD 1619/2012: series separadas de completas y simplificadas en el mismo año) ni conviene
    /// mezclarlas con las simplificadas ordinarias (art. 7.1.a 4.º). Las instalaciones
    /// anteriores a este campo toman «RS».
    #[serde(default = "default_simplified_rectifying_series")]
    pub simplified_rectifying_series: String,
    pub language: Language,
    pub default_vat: Rate,
    pub default_withholding: Option<Rate>,
    pub outputs: Outputs,
    pub destination: Option<Destination>,
    pub rounding: RoundingMode,
    pub verifactu: VerifactuSettings,
    /// Texto al pie (forma de pago, IBAN…).
    pub footer: Option<String>,
    /// Actividad para los libros registro (código A/B/C/D, tipo y epígrafe IAE).
    #[serde(default)]
    pub activity: Option<taxpain_invoicing::books::Activity>,
    /// La empresa hace operaciones del art. 4.2 RD 1619/2012 (ventas al por menor, hostelería y
    /// restauración, transporte de personas, peluquería…): la factura simplificada llega a
    /// 3.000 €. Si no (por defecto), el límite es el general de 400 € (art. 4.1.a).
    #[serde(default)]
    pub simplified_up_to_3000: bool,
}

impl InvoicingSettings {
    /// Límite de la factura simplificada, IVA incluido (art. 4 RD 1619/2012).
    pub fn simplified_limit(&self) -> taxpain_core::money::Money {
        if self.simplified_up_to_3000 {
            taxpain_core::invoice::SIMPLIFIED_LIMIT_ART_4_2
        } else {
            taxpain_core::invoice::SIMPLIFIED_LIMIT
        }
    }
}

/// Serie de simplificadas por defecto (también para los ajustes guardados sin ella).
pub fn default_simplified_series() -> String {
    "S".into()
}

/// Serie de rectificativas de simplificadas por defecto (también para los ajustes guardados sin
/// ella).
pub fn default_simplified_rectifying_series() -> String {
    "RS".into()
}

/// Identificador aleatorio para `NumeroInstalacion`.
pub fn new_installation_id() -> String {
    let mut b = [0u8; 16];
    // Si el sistema no da aleatoriedad, el id cae a ceros: sigue siendo válido y se avisa.
    if getrandom::fill(&mut b).is_err() {
        tracing::warn!("sin aleatoriedad del sistema para el id de instalación");
    }
    format!("TP-{}", hex::encode(b))
}

/// Carpetas de sincronización presentes en `home` (macOS), para ofrecerlas en el onboarding.
pub fn detect_destinations(home: &Path) -> Vec<Destination> {
    let mut out = Vec::new();
    let icloud = home.join("Library/Mobile Documents/com~apple~CloudDocs");
    if icloud.is_dir() {
        out.push(Destination {
            kind: DestinationKind::ICloudDrive,
            path: icloud.join("TaxPain"),
        });
    }
    let cloud_storage = home.join("Library/CloudStorage");
    if let Ok(entries) = std::fs::read_dir(&cloud_storage) {
        let mut names: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        names.sort();
        for p in names {
            let name = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if name.starts_with("GoogleDrive-") {
                let root = ["My Drive", "Mi unidad"]
                    .iter()
                    .map(|d| p.join(d))
                    .find(|d| d.is_dir());
                if let Some(root) = root {
                    out.push(Destination {
                        kind: DestinationKind::GoogleDrive,
                        path: root.join("TaxPain"),
                    });
                }
            } else if name.starts_with("Dropbox") {
                out.push(Destination {
                    kind: DestinationKind::Dropbox,
                    path: p.join("TaxPain"),
                });
            }
        }
    }
    let legacy_dropbox = home.join("Dropbox");
    if legacy_dropbox.is_dir() && !out.iter().any(|d| d.kind == DestinationKind::Dropbox) {
        out.push(Destination {
            kind: DestinationKind::Dropbox,
            path: legacy_dropbox.join("TaxPain"),
        });
    }
    out.push(Destination {
        kind: DestinationKind::Local,
        path: home.join("Documents/TaxPain"),
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detecta_carpetas_sincronizadas() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        std::fs::create_dir_all(h.join("Library/Mobile Documents/com~apple~CloudDocs")).unwrap();
        std::fs::create_dir_all(
            h.join("Library/CloudStorage/GoogleDrive-usuario@example.com/Mi unidad"),
        )
        .unwrap();
        std::fs::create_dir_all(h.join("Library/CloudStorage/Dropbox")).unwrap();
        let d = detect_destinations(h);
        let kinds: Vec<_> = d.iter().map(|x| x.kind).collect();
        assert_eq!(
            kinds,
            [
                DestinationKind::ICloudDrive,
                DestinationKind::Dropbox,
                DestinationKind::GoogleDrive,
                DestinationKind::Local
            ]
        );
        assert!(d[2].path.ends_with("Mi unidad/TaxPain"));
        assert!(d[3].path.ends_with("Documents/TaxPain"));
    }

    #[test]
    fn sin_nubes_solo_local() {
        let home = tempfile::tempdir().unwrap();
        assert_eq!(detect_destinations(home.path()).len(), 1);
        assert!(new_installation_id().starts_with("TP-"));
        assert_eq!(new_installation_id().len(), 35);
    }
}
