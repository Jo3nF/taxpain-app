//! Actualizaciones de la app (Fase 8). El manifiesto (`latest.json`) lo publica quien mantiene el proyecto en
//! taxpain.es; los paquetes viven en la release de GitHub de cada versión y van firmados con la
//! clave del updater (minisign); la app sólo instala lo que verifica con la clave pública
//! compilada en ella (`TAXPAIN_UPDATER_PUBKEY` al compilar la versión). Sin esa clave
//! (compilaciones de desarrollo) las actualizaciones están desactivadas. Ver docs/RELEASE.md §3.

use serde::Serialize;
use tauri_plugin_updater::UpdaterExt;

use crate::{CmdResult, CommandError};

/// Dónde mira la app si hay versión nueva. El workflow Release sube `latest.json` como asset de
/// la release de GitHub y quien mantiene el proyecto lo copia a mano a `web/updates/` (docs/RELEASE.md §3.4). Si algún
/// día se quiere prescindir de la web, puede apuntar directamente al asset de la última release:
/// `https://github.com/<propietario>/taxpain-app/releases/latest/download/latest.json` (cambiar también
/// `plugins.updater.endpoints` en tauri.conf.json y la tabla de docs/PRIVACIDAD_APP.md).
pub const MANIFEST_URL: &str = "https://taxpain.es/updates/latest.json";

fn pubkey() -> Option<&'static str> {
    option_env!("TAXPAIN_UPDATER_PUBKEY").filter(|k| !k.trim().is_empty())
}

#[derive(Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: Option<String>,
}

fn updater(app: &tauri::AppHandle) -> CmdResult<tauri_plugin_updater::Updater> {
    let key = pubkey().ok_or_else(|| {
        CommandError::from("Esta compilación no tiene activadas las actualizaciones automáticas.")
    })?;
    let url = MANIFEST_URL
        .parse()
        .map_err(|e: url::ParseError| CommandError::from(e.to_string()))?;
    Ok(app
        .updater_builder()
        .pubkey(key)
        .endpoints(vec![url])?
        // El manifiesto se descarga sin firmar. Contra que instale una versión antigua hay dos
        // defensas, y hacen falta las dos:
        // - este comparador: no se instala nada que el manifiesto anuncie como igual o anterior
        //   a la instalada (el plugin ya lo hace salvo con `allowDowngrades`; fijado aquí, no
        //   depende de la configuración);
        // - `requireSignedVersion` en tauri.conf.json: la versión anunciada tiene que ser la
        //   que va firmada en el comentario de confianza del `.sig` (la escribe
        //   `tauri signer sign --app-version` en release.yml).
        //   Sin eso, un manifiesto falso podría anunciar «99.0.0» con el paquete y la firma de
        //   una versión antigua, auténtica y con fallos ya corregidos.
        .version_comparator(|instalada, ofrecida| es_mas_nueva(&instalada, &ofrecida.version))
        .build()?)
}

/// Sólo se instala una versión estrictamente mayor que la instalada (ni la misma ni una
/// anterior). Por precedencia de semver: los metadatos de compilación (`+…`) no cuentan, así
/// que `0.10.0+1` no es más nueva que `0.10.0`.
fn es_mas_nueva(instalada: &semver::Version, ofrecida: &semver::Version) -> bool {
    ofrecida.cmp_precedence(instalada).is_gt()
}

/// Comprueba si hay una versión nueva (sólo lectura).
#[tauri::command]
pub async fn update_check(app: tauri::AppHandle) -> CmdResult<Option<UpdateInfo>> {
    Ok(updater(&app)?.check().await?.map(|u| UpdateInfo {
        version: u.version.clone(),
        notes: u.body.clone(),
    }))
}

/// Descarga, verifica la firma e instala la versión nueva; después reinicia la app.
#[tauri::command]
pub async fn update_install(app: tauri::AppHandle, confirmed: bool) -> CmdResult<()> {
    if !confirmed {
        return Err(CommandError::from("hace falta confirmar la actualización"));
    }
    let update = updater(&app)?
        .check()
        .await?
        .ok_or_else(|| CommandError::from("No hay ninguna versión nueva."))?;
    update.download_and_install(|_, _| {}, || {}).await?;
    app.restart();
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    /// La versión que anuncia un manifiesto, leída como la lee el plugin.
    fn anunciada(version: &str) -> semver::Version {
        let r: tauri_plugin_updater::RemoteRelease = serde_json::from_value(serde_json::json!({
            "version": version,
            "platforms": {
                "darwin-universal": {
                    "signature": "x",
                    "url": "https://example.invalid/TaxPain.app.tar.gz"
                }
            }
        }))
        .unwrap();
        r.version
    }

    #[test]
    fn solo_se_instala_una_version_mayor() {
        let v = |s: &str| semver::Version::parse(s).unwrap();
        assert!(es_mas_nueva(&v("0.9.0"), &anunciada("0.10.0")));
        assert!(es_mas_nueva(&v("0.10.0"), &anunciada("v0.10.1")));
        assert!(es_mas_nueva(&v("0.10.0-beta.1"), &anunciada("0.10.0")));
        assert!(!es_mas_nueva(&v("0.10.0"), &anunciada("0.10.0")));
        assert!(!es_mas_nueva(&v("0.10.0"), &anunciada("0.9.5")));
        assert!(!es_mas_nueva(&v("0.10.0"), &anunciada("0.10.0-beta.1")));
        // Los metadatos de compilación no hacen nueva una versión (semver los ordena, pero no
        // cuentan para la precedencia).
        assert!(v("0.10.0+1") > v("0.10.0"));
        assert!(!es_mas_nueva(&v("0.10.0"), &anunciada("0.10.0+1")));
    }

    /// Nadie ha activado `allowDowngrades` (además, el comparador de arriba manda sobre él) y
    /// la versión anunciada ha de coincidir con la firmada.
    #[test]
    fn la_configuracion_no_permite_volver_atras() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let updater = &conf["plugins"]["updater"];
        assert!(updater.is_object());
        assert_ne!(updater["allowDowngrades"], serde_json::Value::Bool(true));
        assert_eq!(
            updater["requireSignedVersion"],
            serde_json::Value::Bool(true)
        );
        // Y el plugin lo lee así (un nombre mal escrito se ignoraría sin avisar).
        let plugin: tauri_plugin_updater::Config = serde_json::from_value(updater.clone()).unwrap();
        assert!(plugin.require_signed_version);
        assert!(!plugin.allow_downgrades);
    }
}
