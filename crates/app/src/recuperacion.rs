//! Empresa que no abre (G46): qué ha pasado, en llano, y qué se puede hacer sin tocar nada.
//!
//! Cuando el almacén de una empresa no se puede abrir (BD dañada, clave que no está en el
//! Llavero, migración que falla, versión más nueva de la app, auditoría alterada), la interfaz
//! enseña una pantalla de recuperación con este [`Diagnostico`]: qué ha pasado, abrir la última
//! copia de seguridad (se restaura como empresa nueva: la dañada se queda como está) y exportar
//! lo que se pueda ([`taxpain_vault::volcado::rescatar`], que sólo lee y copia).
//!
//! Para poder ofrecer la última copia aunque la BD no abra, cada copia de seguridad deja junto a
//! los datos un `copias.json` con la ruta de la última y la carpeta de las automáticas (sólo
//! rutas: nada secreto) — ver [`recordar_copia`].

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use taxpain_vault::VaultError;

/// Fichero junto a los datos con la última copia conocida.
pub const FICHERO_COPIAS: &str = "copias.json";

/// Lo que se sabe de las copias de una empresa sin abrir su BD.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopiasConocidas {
    pub empresa: Option<String>,
    pub ultima: Option<PathBuf>,
    pub ultima_en: Option<String>,
    pub carpeta: Option<PathBuf>,
}

/// Apunta la última copia de la empresa de `dir` (se llama al hacer cada copia). Si no se puede
/// escribir, se sigue: es una ayuda, no un dato.
pub fn recordar_copia(dir: &Path, empresa: &str, ultima: &Path, en: &str, carpeta: Option<&Path>) {
    let c = CopiasConocidas {
        empresa: Some(empresa.to_owned()),
        ultima: Some(ultima.to_owned()),
        ultima_en: Some(en.to_owned()),
        carpeta: carpeta.map(Path::to_owned),
    };
    let r = serde_json::to_vec_pretty(&c)
        .map_err(std::io::Error::other)
        .and_then(|b| {
            let tmp = dir.join("copias.json.tmp");
            std::fs::write(&tmp, b)?;
            std::fs::rename(&tmp, dir.join(FICHERO_COPIAS))
        });
    if let Err(e) = r {
        tracing::warn!(error = %e, "no se pudo apuntar la última copia junto a los datos");
    }
}

pub fn copias_conocidas(dir: &Path) -> CopiasConocidas {
    std::fs::read(dir.join(FICHERO_COPIAS))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Una copia `.taxpain` encontrada.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Copia {
    pub ruta: PathBuf,
    pub nombre: String,
    pub bytes: u64,
    /// Fecha de modificación del fichero (RFC 3339).
    pub modificada: Option<String>,
}

/// Copias que se pueden ofrecer: la última apuntada y las `.taxpain` de la carpeta de las
/// automáticas, de la más reciente a la más antigua (como mucho 10).
pub fn copias(dir: &Path) -> Vec<Copia> {
    let c = copias_conocidas(dir);
    let mut rutas: Vec<PathBuf> = c.ultima.into_iter().collect();
    if let Some(carpeta) = c.carpeta
        && let Ok(rd) = std::fs::read_dir(carpeta)
    {
        rutas.extend(rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| {
            p.extension()
                .is_some_and(|e| e == taxpain_vault::backup::EXTENSION)
        }));
    }
    let mut out: Vec<(std::time::SystemTime, Copia)> = Vec::new();
    for r in rutas {
        if out.iter().any(|(_, c)| c.ruta == r) {
            continue;
        }
        let Ok(m) = std::fs::metadata(&r) else {
            continue;
        };
        if !m.is_file() {
            continue;
        }
        let t = m.modified().unwrap_or(std::time::UNIX_EPOCH);
        let modificada = time::OffsetDateTime::from(t)
            .format(&time::format_description::well_known::Rfc3339)
            .ok();
        out.push((
            t,
            Copia {
                nombre: r
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                ruta: r,
                bytes: m.len(),
                modificada,
            },
        ));
    }
    out.sort_by_key(|(t, _)| std::cmp::Reverse(*t));
    out.into_iter().take(10).map(|(_, c)| c).collect()
}

/// Qué le pasa a una empresa que no abre.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tipo {
    /// El Llavero no tiene la clave (Mac nuevo, cuenta nueva, elemento borrado).
    SinClave,
    /// El Llavero no ha dado la clave (acceso denegado).
    LlaveroDenegado,
    /// El fichero está dañado o la clave no es la suya.
    Danada,
    /// Los datos son de una versión más nueva de TaxPain.
    VersionNueva,
    /// La actualización de la estructura de los datos (migración) ha fallado.
    Migracion,
    /// El registro de auditoría no cuadra (se ha tocado la BD por fuera).
    AuditoriaAlterada,
    /// No está el fichero de datos.
    NoEsta,
    /// Otra cosa (disco lleno, permisos…).
    Otro,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Diagnostico {
    pub id: String,
    /// Nombre de la empresa, si se sabe (de la última copia apuntada).
    pub empresa: Option<String>,
    pub tipo: Tipo,
    pub titulo: String,
    /// Qué ha pasado y qué hacer, en llano.
    pub explicacion: String,
    /// El error técnico, para pedir ayuda.
    pub detalle: String,
    /// Copias de seguridad que se pueden abrir.
    pub copias: Vec<Copia>,
    /// Se puede rescatar algo más que los ficheros cifrados (el Llavero tiene la clave).
    pub rescate_legible: bool,
}

const NO_SE_TOCA: &str = "TaxPain no ha cambiado ni borrado nada: los ficheros de esta empresa \
                          siguen como estaban.";

/// Clasifica el error de apertura de la empresa `id` (carpeta `dir`).
pub fn diagnosticar(id: &str, dir: &Path, e: &VaultError, clave_en_llavero: bool) -> Diagnostico {
    let detalle = e.to_string();
    let (tipo, titulo, que) = match e {
        VaultError::KeyMissing { recoverable } => (
            Tipo::SinClave,
            "Falta la clave de los datos en el Llavero",
            if *recoverable {
                "El Llavero de este Mac no tiene la clave que cifra estos datos (pasa al cambiar \
                 de Mac o de cuenta de usuario). Junto a los datos está el código de \
                 recuperación: con la contraseña de las copias se recupera la clave."
            } else {
                "El Llavero de este Mac no tiene la clave que cifra estos datos y no hay código \
                 de recuperación. Abre una copia de seguridad (.taxpain) con su contraseña."
            },
        ),
        VaultError::KeyUnavailable(_) | VaultError::NotConfirmed => (
            Tipo::LlaveroDenegado,
            "El Llavero no ha dado la clave",
            "El Mac no ha dejado leer la clave del Llavero. Vuelve a abrir TaxPain y, cuando el \
             Mac pregunte, pulsa «Permitir siempre».",
        ),
        VaultError::Corrupted => (
            Tipo::Danada,
            "Los datos están dañados",
            "El fichero de la base de datos está dañado (un fallo del disco, una copia a medias \
             o una sincronización en la nube que lo ha pisado) o la clave del Llavero no es la \
             suya. Abre la última copia de seguridad: se restaura como empresa nueva y ésta se \
             queda como está, por si hace falta.",
        ),
        VaultError::AuditTampered(_) => (
            Tipo::AuditoriaAlterada,
            "El registro de auditoría no cuadra",
            "El registro de auditoría de esta empresa no cuadra con su huella: los datos se han \
             cambiado por fuera de TaxPain o están dañados. Por seguridad no se abren. Abre la \
             última copia de seguridad o exporta lo que se pueda y pide ayuda.",
        ),
        VaultError::NotFound => (
            Tipo::NoEsta,
            "No está el fichero de datos",
            "En la carpeta de esta empresa no está el fichero de datos (taxpain.db). Si la \
             moviste, devuélvela a su sitio; si no, abre una copia de seguridad.",
        ),
        VaultError::InvalidInput(m) if m.contains("más nueva") => (
            Tipo::VersionNueva,
            "Los datos son de una versión más nueva de TaxPain",
            "Estos datos los ha guardado una versión de TaxPain más nueva que ésta. Actualiza \
             la app (Ajustes → Actualizaciones o la web) y vuelve a abrirla.",
        ),
        VaultError::Db(_) => (
            Tipo::Migracion,
            "No se han podido preparar los datos para esta versión",
            "Al actualizar la estructura de los datos para esta versión de TaxPain ha fallado \
             algo y se ha deshecho. Vuelve a la versión anterior de la app, abre la última copia \
             de seguridad o exporta lo que se pueda y pide ayuda con el detalle de abajo.",
        ),
        _ => (
            Tipo::Otro,
            "No se ha podido abrir la empresa",
            "Comprueba que el disco tiene espacio y que la carpeta de datos de TaxPain se puede \
             escribir, y vuelve a abrir la app. Si sigue igual, abre la última copia o exporta \
             lo que se pueda.",
        ),
    };
    Diagnostico {
        id: id.to_owned(),
        empresa: copias_conocidas(dir).empresa,
        tipo,
        titulo: titulo.to_owned(),
        explicacion: format!("{que}\n\n{NO_SE_TOCA}"),
        detalle,
        copias: copias(dir),
        rescate_legible: clave_en_llavero && dir.join("taxpain.db").is_file(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cada_fallo_tiene_su_explicacion_y_dice_que_no_se_ha_tocado_nada() {
        let tmp = tempfile::tempdir().unwrap();
        let casos = [
            (VaultError::KeyMissing { recoverable: true }, Tipo::SinClave),
            (
                VaultError::KeyUnavailable("no".into()),
                Tipo::LlaveroDenegado,
            ),
            (VaultError::Corrupted, Tipo::Danada),
            (VaultError::AuditTampered(7), Tipo::AuditoriaAlterada),
            (
                VaultError::InvalidInput(
                    "la base de datos es de una versión más nueva de TaxPain".into(),
                ),
                Tipo::VersionNueva,
            ),
            (
                VaultError::Db(taxpain_vault::rusqlite::Error::InvalidQuery),
                Tipo::Migracion,
            ),
            (VaultError::NotFound, Tipo::NoEsta),
        ];
        for (e, t) in casos {
            let d = diagnosticar("principal", tmp.path(), &e, false);
            assert_eq!(d.tipo, t, "{e}");
            assert!(d.explicacion.contains("no ha cambiado ni borrado nada"));
            assert!(!d.rescate_legible);
        }
    }

    #[test]
    fn la_ultima_copia_apuntada_se_ofrece_aunque_la_bd_no_abra() {
        let tmp = tempfile::tempdir().unwrap();
        let carpeta = tmp.path().join("copias");
        std::fs::create_dir_all(&carpeta).unwrap();
        let a = carpeta.join("Estudio-2026-11-01-auto-x.taxpain");
        let b = tmp.path().join("a-mano.taxpain");
        std::fs::write(&a, b"copia").unwrap();
        std::fs::write(&b, b"copia").unwrap();
        std::fs::write(carpeta.join("otra.txt"), b"no").unwrap();
        let datos = tmp.path().join("datos");
        std::fs::create_dir_all(&datos).unwrap();
        recordar_copia(
            &datos,
            "Estudio Demo SL",
            &b,
            "2026-11-10T10:00:00Z",
            Some(&carpeta),
        );
        let d = diagnosticar("principal", &datos, &VaultError::Corrupted, true);
        assert_eq!(d.empresa.as_deref(), Some("Estudio Demo SL"));
        let rutas: Vec<_> = d.copias.iter().map(|c| c.ruta.clone()).collect();
        assert_eq!(rutas.len(), 2);
        assert!(rutas.contains(&a) && rutas.contains(&b));
        assert!(!d.rescate_legible, "sin taxpain.db no hay nada que leer");
    }
}
