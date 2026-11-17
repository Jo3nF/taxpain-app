//! Varias empresas o autónomos en el mismo Mac (p. ej. una gestoría o quien lleva su actividad y
//! una SL): cada uno tiene su almacén propio, una BD cifrada en su carpeta, con su contabilidad,
//! facturas, modelos y certificados. Nada se mezcla entre empresas.
//!
//! Carpetas, bajo la de datos de la app:
//! - `data/`: la empresa `principal` (la de siempre; no se mueve al añadir otras).
//! - `companies/<id>/`: las demás, con `id` de 16 caracteres hexadecimales.
//! - `active-company`: el id de la empresa abierta (sólo el id, sin datos personales).
//! - `principal-deleted`: marca de que la principal se borró (ver [`remove`]).
//!
//! El nombre y el NIF de cada empresa están dentro de su BD cifrada (ajuste `company`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use taxpain_core::invoice::PartyId;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use time::OffsetDateTime;

use crate::error::{AppError, Result};

pub const PRINCIPAL: &str = "principal";
pub const COMPANY_KEY: &str = "company";
const ACTIVE_FILE: &str = "active-company";
/// Marca de que la empresa principal se borró: sin ella, `data/` se volvería a crear vacía al
/// abrir la app.
const PRINCIPAL_DELETED: &str = "principal-deleted";

/// Identificación de la empresa (dentro de su propia BD).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompanyInfo {
    pub name: String,
    pub nif: Option<String>,
}

/// Empresa de la lista.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Company {
    pub id: String,
    pub name: String,
    pub nif: Option<String>,
    pub active: bool,
    /// La empresa no se puede abrir: por qué (la interfaz ofrece la pantalla de recuperación).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_abre: Option<String>,
}

fn io(e: std::io::Error) -> AppError {
    AppError::Vault(taxpain_vault::VaultError::Io(e))
}

fn valid_id(id: &str) -> bool {
    id == PRINCIPAL || (id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Carpeta del almacén de la empresa `id`.
pub fn dir(root: &Path, id: &str) -> Result<PathBuf> {
    if !valid_id(id) {
        return Err(AppError::Other("empresa no válida".into()));
    }
    Ok(if id == PRINCIPAL {
        root.join("data")
    } else {
        root.join("companies").join(id)
    })
}

/// Prefijo de la carpeta donde se restaura una copia antes de publicarla como empresa: la lista
/// no la ve (no es un id válido), así que una restauración interrumpida no deja una empresa a
/// medias.
const STAGING_PREFIX: &str = ".restoring-";

/// Carpeta temporal donde se restaura la empresa `id` (dentro de `companies/`, para que el
/// `rename` final sea atómico).
pub fn staging_dir(root: &Path, id: &str) -> Result<PathBuf> {
    if id == PRINCIPAL || !valid_id(id) {
        return Err(AppError::Other("empresa no válida".into()));
    }
    Ok(root.join("companies").join(format!("{STAGING_PREFIX}{id}")))
}

/// Publica la empresa restaurada en `staging_dir`: a partir de aquí aparece en la lista.
pub fn publish_staged(root: &Path, id: &str) -> Result<PathBuf> {
    let from = staging_dir(root, id)?;
    let to = dir(root, id)?;
    if to.exists() {
        return Err(AppError::Other("ya existe una empresa con ese id".into()));
    }
    std::fs::rename(&from, &to).map_err(io)?;
    Ok(to)
}

/// Borra las carpetas de restauraciones que no terminaron (la app se cerró a medias).
pub fn clean_staging(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root.join("companies")) else {
        return;
    };
    for e in entries.filter_map(|e| e.ok()) {
        let name = e.file_name();
        let Some(id) = name.to_str().and_then(|n| n.strip_prefix(STAGING_PREFIX)) else {
            continue;
        };
        if valid_id(id) && id != PRINCIPAL && e.path().is_dir() {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// Ids de las empresas existentes: `principal` primero y luego las demás en orden de id.
/// La principal no está si se borró (y no se ha vuelto a poner su carpeta), salvo que no quede
/// ninguna otra: entonces vuelve, vacía si hace falta (la app necesita una empresa).
pub fn ids(root: &Path) -> Result<Vec<String>> {
    let companies = root.join("companies");
    let mut others: Vec<String> = match std::fs::read_dir(&companies) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().join("taxpain.db").is_file())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|id| id != PRINCIPAL && valid_id(id))
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(io(e)),
    };
    others.sort();
    let principal = others.is_empty()
        || root.join("data").join("taxpain.db").is_file()
        || !root.join(PRINCIPAL_DELETED).exists();
    let mut out = Vec::with_capacity(others.len() + 1);
    if principal {
        out.push(PRINCIPAL.to_owned());
    }
    out.extend(others);
    Ok(out)
}

/// ¿Se borró la empresa principal (y no ha vuelto)?
pub fn principal_deleted(root: &Path) -> bool {
    ids(root).is_ok_and(|all| !all.iter().any(|i| i == PRINCIPAL))
}

/// Empresa abierta la última vez (si no hay o ya no existe, la primera de la lista: la
/// principal, si no se ha borrado).
pub fn active(root: &Path) -> String {
    let all = ids(root).unwrap_or_default();
    std::fs::read_to_string(root.join(ACTIVE_FILE))
        .ok()
        .map(|s| s.trim().to_owned())
        .filter(|id| valid_id(id) && all.contains(id))
        .or_else(|| all.into_iter().next())
        .unwrap_or_else(|| PRINCIPAL.to_owned())
}

pub fn set_active(root: &Path, id: &str) -> Result<()> {
    if !valid_id(id) {
        return Err(AppError::Other("empresa no válida".into()));
    }
    std::fs::create_dir_all(root).map_err(io)?;
    let tmp = root.join(format!("{ACTIVE_FILE}.tmp"));
    std::fs::write(&tmp, id).map_err(io)?;
    std::fs::rename(&tmp, root.join(ACTIVE_FILE)).map_err(io)
}

/// Id nuevo para otra empresa (su carpeta la crea el almacén al abrirse).
pub fn new_id() -> Result<String> {
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).map_err(|e| AppError::Other(e.to_string()))?;
    Ok(hex::encode(b))
}

/// Comprueba y normaliza los datos de una empresa.
pub fn normalize(info: &CompanyInfo) -> Result<CompanyInfo> {
    let name = info.name.trim();
    if name.is_empty() {
        return Err(AppError::Other("pon el nombre de la empresa".into()));
    }
    let nif = match info.nif.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
        Some(n) => Some(
            taxpain_core::nif::Nif::parse(n)
                .map_err(|e| AppError::Other(format!("NIF: {e}")))?
                .as_str()
                .to_owned(),
        ),
        None => None,
    };
    Ok(CompanyInfo {
        name: name.to_owned(),
        nif,
    })
}

/// Nombre y NIF de la empresa del almacén: los guardados; si no hay, los del emisor de las
/// facturas; si tampoco, «Mi empresa».
pub fn info(v: &Vault) -> Result<CompanyInfo> {
    if let Some(i) = v.setting::<CompanyInfo>(COMPANY_KEY)? {
        return Ok(i);
    }
    if let Some(s) = crate::invoicing::settings(v)? {
        return Ok(CompanyInfo {
            name: s.issuer.name.clone(),
            nif: match &s.issuer.id {
                PartyId::Nif(n) => Some(n.clone()),
                _ => None,
            },
        });
    }
    Ok(CompanyInfo {
        name: "Mi empresa".into(),
        nif: None,
    })
}

/// Al guardar los datos de facturación: si la empresa sigue llamándose «Mi empresa» (el nombre
/// provisional), toma la razón social y el NIF del emisor, para que el selector la nombre bien.
pub fn adopt_issuer(
    v: &Vault,
    issuer: &taxpain_core::invoice::Party,
    now: OffsetDateTime,
) -> Result<()> {
    let Some(stored) = v.setting::<CompanyInfo>(COMPANY_KEY)? else {
        // Sin nombre guardado ya se usa el del emisor (`info`).
        return Ok(());
    };
    if issuer.name.trim().is_empty() || !stored.name.trim().eq_ignore_ascii_case("Mi empresa") {
        return Ok(());
    }
    let nif = match &issuer.id {
        PartyId::Nif(n) => Some(n.clone()),
        _ => None,
    };
    set_info(
        v,
        &CompanyInfo {
            name: issuer.name.clone(),
            nif: stored.nif.or(nif),
        },
        now,
    )?;
    Ok(())
}

/// ¿Lo escrito confirma el borrado de la empresa? Su nombre o su NIF (sin contar mayúsculas ni
/// espacios de más).
pub fn confirms(info: &CompanyInfo, typed: &str) -> bool {
    let norm = |s: &str| {
        s.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let typed = norm(typed);
    !typed.is_empty()
        && (typed == norm(&info.name)
            || info
                .nif
                .as_deref()
                .is_some_and(|n| norm(n).replace(' ', "") == typed.replace(' ', "")))
}

/// Mueve a la Papelera (con `trash`) la carpeta de la empresa `id`, que deja de estar en la lista.
/// No se puede borrar la única empresa ni la abierta (antes se abre otra). Si `trash` falla no
/// cambia nada.
///
/// La carpeta de la principal (`data/`) no se vuelve a crear al abrir la app: queda la marca
/// `principal-deleted`. Si alguien devuelve su carpeta desde la Papelera, vuelve a la lista.
pub fn remove(
    root: &Path,
    id: &str,
    trash: &dyn Fn(&Path) -> std::result::Result<(), String>,
) -> Result<()> {
    let all = ids(root)?;
    if !all.iter().any(|i| i == id) {
        return Err(AppError::Other("esa empresa no existe".into()));
    }
    if all.len() < 2 {
        return Err(AppError::Other(
            "es la única empresa: no se puede borrar".into(),
        ));
    }
    if active(root) == id {
        return Err(AppError::Other(
            "es la empresa abierta: abre antes otra".into(),
        ));
    }
    let d = dir(root, id)?;
    let marker = root.join(PRINCIPAL_DELETED);
    if id == PRINCIPAL {
        std::fs::write(&marker, b"").map_err(io)?;
        if !d.exists() {
            return Ok(());
        }
    }
    if let Err(e) = trash(&d) {
        if id == PRINCIPAL {
            let _ = std::fs::remove_file(&marker);
        }
        return Err(AppError::Other(format!(
            "no se pudo mover la empresa a la Papelera ({e}); no se ha borrado nada"
        )));
    }
    Ok(())
}

/// Deja en la auditoría de la empresa `v` que se borró otra.
pub fn record_deleted(
    v: &Vault,
    id: &str,
    deleted: &CompanyInfo,
    now: OffsetDateTime,
) -> Result<()> {
    audit::append(
        v.connection(),
        &crate::invoicing::now_str(now),
        Actor::User,
        "company.delete",
        &deleted.name,
        &serde_json::json!({ "id": id, "nif": deleted.nif }),
    )?;
    Ok(())
}

pub fn set_info(v: &Vault, info: &CompanyInfo, now: OffsetDateTime) -> Result<CompanyInfo> {
    let info = normalize(info)?;
    v.set_setting(COMPANY_KEY, &info)?;
    audit::append(
        v.connection(),
        &crate::invoicing::now_str(now),
        Actor::User,
        "company.update",
        &info.name,
        &serde_json::json!({ "nif": info.nif }),
    )?;
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mi_empresa_toma_el_nombre_del_emisor() {
        use std::sync::Arc;
        let v = Vault::open_in_memory(
            Arc::new(taxpain_vault::MemoryKeyProvider::default()),
            Arc::new(taxpain_vault::AlwaysConfirm),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let now = OffsetDateTime::now_utc();
        let issuer = taxpain_core::invoice::Party {
            id: PartyId::Nif("B12345674".into()),
            name: "EMPRESA DEMO SL".into(),
            address: None,
            person: None,
        };
        let mi = CompanyInfo {
            name: "Mi empresa".into(),
            nif: None,
        };
        set_info(&v, &mi, now).unwrap_or_else(|e| panic!("{e}"));
        adopt_issuer(&v, &issuer, now).unwrap_or_else(|e| panic!("{e}"));
        let i = info(&v).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(i.name, "EMPRESA DEMO SL");
        assert_eq!(i.nif.as_deref(), Some("B12345674"));
        // Un nombre elegido por el usuario no se toca.
        let otro = CompanyInfo {
            name: "Mi tienda".into(),
            nif: None,
        };
        set_info(&v, &otro, now).unwrap_or_else(|e| panic!("{e}"));
        adopt_issuer(&v, &issuer, now).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(info(&v).unwrap_or_else(|e| panic!("{e}")).name, "Mi tienda");
    }

    #[test]
    fn carpetas_y_empresa_activa() {
        let root = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let root = root.path();
        assert_eq!(ids(root).ok(), Some(vec![PRINCIPAL.to_owned()]));
        assert_eq!(active(root), PRINCIPAL);
        assert!(dir(root, "../otra").is_err());
        assert!(set_active(root, "../x").is_err());

        let id = new_id().unwrap_or_default();
        assert_eq!(id.len(), 16);
        let d = dir(root, &id).unwrap_or_default();
        assert_eq!(d, root.join("companies").join(&id));
        // Mientras no tenga BD no cuenta (p. ej. un alta que falló a medias).
        std::fs::create_dir_all(&d).unwrap_or_default();
        assert_eq!(ids(root).map(|v| v.len()).ok(), Some(1));
        std::fs::write(d.join("taxpain.db"), b"").unwrap_or_default();
        assert_eq!(ids(root).ok(), Some(vec![PRINCIPAL.to_owned(), id.clone()]));

        set_active(root, &id).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(active(root), id);
        // Si la empresa activa desaparece, se vuelve a la principal.
        std::fs::remove_dir_all(&d).unwrap_or_default();
        assert_eq!(active(root), PRINCIPAL);
    }

    /// «Papelera» de las pruebas: mueve la carpeta a `bin` (nunca a la Papelera de verdad).
    fn fake_trash(bin: &Path) -> impl Fn(&Path) -> std::result::Result<(), String> + '_ {
        move |p: &Path| {
            std::fs::create_dir_all(bin).map_err(|e| e.to_string())?;
            let name = p.file_name().ok_or("sin nombre")?;
            std::fs::rename(p, bin.join(name)).map_err(|e| e.to_string())
        }
    }

    fn fake_company(root: &Path, id: &str) {
        let d = dir(root, id).unwrap_or_else(|e| panic!("{e}"));
        std::fs::create_dir_all(&d).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(d.join("taxpain.db"), b"x").unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn borrar_mueve_la_carpeta_a_la_papelera() {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let root = tmp.path().join("app");
        let bin = tmp.path().join("papelera");
        let trash = fake_trash(&bin);
        fake_company(&root, PRINCIPAL);
        // La única empresa no se puede borrar.
        assert!(remove(&root, PRINCIPAL, &trash).is_err());
        let a = "0a1b2c3d4e5f6071";
        let b = "1111222233334444";
        fake_company(&root, a);
        fake_company(&root, b);
        assert!(remove(&root, "ffffffffffffffff", &trash).is_err());
        // Ni la abierta.
        set_active(&root, a).unwrap_or_else(|e| panic!("{e}"));
        assert!(remove(&root, a, &trash).is_err());
        remove(&root, b, &trash).unwrap_or_else(|e| panic!("{e}"));
        assert!(bin.join(b).join("taxpain.db").is_file());
        assert_eq!(
            ids(&root).ok(),
            Some(vec![PRINCIPAL.to_owned(), a.to_owned()])
        );
        // Si la Papelera falla no se toca nada.
        let broken = |_: &Path| -> std::result::Result<(), String> { Err("sin permiso".into()) };
        let e = remove(&root, PRINCIPAL, &broken)
            .err()
            .map(|e| e.to_string());
        assert!(e.is_some_and(|e| e.contains("sin permiso")));
        assert!(!principal_deleted(&root));
        assert!(!root.join(PRINCIPAL_DELETED).exists());
        // La principal: se va su carpeta y no vuelve aunque `data/` se cree otra vez vacía.
        remove(&root, PRINCIPAL, &trash).unwrap_or_else(|e| panic!("{e}"));
        assert!(bin.join("data").join("taxpain.db").is_file());
        assert_eq!(ids(&root).ok(), Some(vec![a.to_owned()]));
        assert!(principal_deleted(&root));
        std::fs::create_dir_all(root.join("data")).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(ids(&root).ok(), Some(vec![a.to_owned()]));
        // Sin `active-company` se abre la primera que queda, no una principal vacía.
        std::fs::remove_file(root.join(ACTIVE_FILE)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(active(&root), a);
        // Ya sólo queda una: no se puede borrar.
        assert!(remove(&root, a, &trash).is_err());
        // Si se devuelve la carpeta desde la Papelera, la principal vuelve a la lista.
        std::fs::remove_dir_all(root.join("data")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::rename(bin.join("data"), root.join("data")).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            ids(&root).ok(),
            Some(vec![PRINCIPAL.to_owned(), a.to_owned()])
        );
    }

    #[test]
    fn sin_otras_empresas_la_principal_vuelve() {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let root = tmp.path();
        std::fs::write(root.join(PRINCIPAL_DELETED), b"").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(ids(root).ok(), Some(vec![PRINCIPAL.to_owned()]));
        assert_eq!(active(root), PRINCIPAL);
    }

    #[test]
    fn confirmar_con_el_nombre_o_el_nif() {
        let i = CompanyInfo {
            name: "EMPRESA DEMO SL".into(),
            nif: Some("B12345674".into()),
        };
        assert!(confirms(&i, "EMPRESA DEMO SL"));
        assert!(confirms(&i, " empresa  demo sl "));
        assert!(confirms(&i, "b12345674"));
        assert!(!confirms(&i, "EMPRESA DEMO"));
        assert!(!confirms(&i, ""));
        let sin_nif = CompanyInfo {
            name: "Mi empresa".into(),
            nif: None,
        };
        assert!(confirms(&sin_nif, "Mi empresa"));
        assert!(!confirms(&sin_nif, " "));
    }

    #[test]
    fn normaliza_nombre_y_nif() {
        let ok = normalize(&CompanyInfo {
            name: "  Empresa Demo SL ".into(),
            nif: Some(" b12345674 ".into()),
        })
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(ok.name, "Empresa Demo SL");
        assert_eq!(ok.nif.as_deref(), Some("B12345674"));
        assert!(
            normalize(&CompanyInfo {
                name: " ".into(),
                nif: None
            })
            .is_err()
        );
        assert!(
            normalize(&CompanyInfo {
                name: "X".into(),
                nif: Some("B12345675".into())
            })
            .is_err()
        );
    }
}
