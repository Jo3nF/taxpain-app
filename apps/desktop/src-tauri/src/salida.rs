//! Dónde puede escribir un comando lo que pide la interfaz.
//!
//! Los comandos de «guardar» reciben la ruta (o la carpeta) que el usuario ha elegido con el
//! diálogo del sistema, pero la manda la interfaz: un script que llegara a ejecutarse en la
//! webview podría pedir `~/.zshenv` o un `LaunchAgent`. Antes de escribir se comprueba aquí que
//! la ruta es de las que puede dar el diálogo para un documento: absoluta, sin `..` ni carpetas
//! ocultas, fuera de la carpeta de datos de la app, de `~/Library`, del sistema y de los
//! paquetes `.app`, con la extensión de ese comando y sin seguir un enlace simbólico.

use std::path::{Component, Path, PathBuf};

use tauri::{Manager, Runtime};

use crate::{CmdResult, CommandError};

/// Carpetas del sistema: ahí nunca se guarda un documento.
const SISTEMA: [&str; 13] = [
    "/System",
    "/Library",
    "/Applications",
    "/usr",
    "/bin",
    "/sbin",
    "/etc",
    "/var",
    "/private",
    "/opt",
    "/dev",
    "/cores",
    "/Network",
];

/// Donde puede haber documentos del usuario fuera de su carpeta personal.
const OTRAS_PERMITIDAS: [&str; 2] = ["/Volumes", "/Users/Shared"];

/// Dentro de `~/Library`, sólo iCloud Drive y las carpetas de Dropbox, Google Drive, OneDrive…
const NUBE: [&str; 2] = [
    "Library/Mobile Documents/com~apple~CloudDocs",
    "Library/CloudStorage",
];

/// Paquetes de macOS: por dentro son carpetas, pero escribir ahí cambia una app o un módulo.
const PAQUETES: [&str; 6] = [
    ".app",
    ".appex",
    ".bundle",
    ".framework",
    ".plugin",
    ".kext",
];

const NO_VALIDA: &str = "la ruta para guardar no es válida: elígela otra vez con el diálogo";
const LUGAR: &str = "ahí no se guardan documentos: elige una carpeta tuya (Documentos, \
                     Escritorio, una carpeta de iCloud Drive o un disco externo)";

/// Lo que hace falta saber del Mac para decidir.
#[derive(Debug, Default)]
pub(crate) struct Reglas {
    pub home: Option<PathBuf>,
    /// Carpetas de datos de la app (datos, configuración, caché, registros).
    pub datos: Vec<PathBuf>,
    pub temporal: Option<PathBuf>,
}

impl Reglas {
    pub fn de<R: Runtime>(app: &tauri::AppHandle<R>) -> Reglas {
        let p = app.path();
        let datos = [
            p.app_data_dir(),
            p.app_local_data_dir(),
            p.app_config_dir(),
            p.app_cache_dir(),
            p.app_log_dir(),
        ];
        Reglas {
            home: p.home_dir().ok(),
            datos: datos.into_iter().flatten().collect(),
            temporal: Some(std::env::temp_dir()),
        }
        .canonicas()
    }

    /// Las mismas carpetas, ya resueltas (las rutas que se comprueban también lo están).
    fn canonicas(self) -> Reglas {
        let c = |p: PathBuf| std::fs::canonicalize(&p).unwrap_or(p);
        Reglas {
            // Una carpeta personal que fuera la raíz lo permitiría todo.
            home: self.home.map(c).filter(|h| h.components().count() > 1),
            datos: self.datos.into_iter().map(c).collect(),
            temporal: self.temporal.map(c),
        }
    }
}

/// Ruta de fichero elegida con el diálogo de guardar, comprobada. `extensiones` en minúsculas.
pub(crate) fn salida_permitida<R: Runtime>(
    app: &tauri::AppHandle<R>,
    ruta: &str,
    extensiones: &[&str],
) -> CmdResult<PathBuf> {
    comprobar_fichero(Path::new(ruta), extensiones, &Reglas::de(app)).map_err(CommandError::from)
}

/// Carpeta elegida con el diálogo, comprobada (resuelta: sin enlaces simbólicos).
pub(crate) fn carpeta_permitida<R: Runtime>(
    app: &tauri::AppHandle<R>,
    dir: &str,
) -> CmdResult<PathBuf> {
    comprobar_carpeta(Path::new(dir), &Reglas::de(app)).map_err(CommandError::from)
}

/// Fichero `nombre` (formado con datos: se sanea) dentro de la carpeta `dir` elegida con el
/// diálogo, comprobado como [`salida_permitida`].
pub(crate) fn en_carpeta<R: Runtime>(
    app: &tauri::AppHandle<R>,
    dir: &str,
    nombre: &str,
    extensiones: &[&str],
) -> CmdResult<PathBuf> {
    let reglas = Reglas::de(app);
    let dir = comprobar_carpeta(Path::new(dir), &reglas)?;
    let nombre = nombre_seguro(nombre, extensiones)?;
    Ok(comprobar_fichero(&dir.join(nombre), extensiones, &reglas)?)
}

/// Escribe `bytes` en `ruta` (ya comprobada) sin seguir un enlace simbólico que apareciera en
/// el destino entre la comprobación y la escritura.
pub(crate) fn escribir(ruta: &Path, bytes: impl AsRef<[u8]>) -> CmdResult<()> {
    use std::io::Write;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW);
    }
    let mut f = o.open(ruta).map_err(|e| {
        #[cfg(unix)]
        if e.raw_os_error() == Some(libc::ELOOP) {
            return CommandError::from("no se guarda encima de un enlace simbólico");
        }
        CommandError::from(e)
    })?;
    f.write_all(bytes.as_ref())?;
    Ok(())
}

/// Documentos que se abren con su app al pulsar «Abrir» (visores, hojas de cálculo, correo).
/// Lo demás que haya llegado como justificante (un `.command`, un `.app`, un `.html`…) no se
/// lanza: se enseña en el Finder.
pub(crate) const SE_ABREN: [&str; 19] = [
    "pdf", "xml", "xsig", "png", "jpg", "jpeg", "heic", "heif", "gif", "tif", "tiff", "webp",
    "bmp", "xlsx", "xls", "ods", "csv", "txt", "eml",
];

/// Lo que se importa como factura recibida (lo mismo que ofrece el diálogo).
pub(crate) const RECIBIDAS: [&str; 13] = [
    "pdf", "xml", "xsig", "jpg", "jpeg", "png", "heic", "heif", "tif", "tiff", "webp", "gif", "bmp",
];

/// `true` si `name` tiene una extensión de `lista` (sin distinguir mayúsculas).
pub(crate) fn extension_en(name: &str, lista: &[&str]) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|e| lista.contains(&e.to_string_lossy().to_ascii_lowercase().as_str()))
}

/// Abre la copia temporal de un documento guardado con la app del sistema si es de los que se
/// abren ([`SE_ABREN`]); si no, la enseña en el Finder sin abrirla.
pub(crate) fn abrir_copia<R: Runtime>(app: &tauri::AppHandle<R>, ruta: &Path) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let nombre = ruta
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    if extension_en(&nombre, &SE_ABREN) {
        app.opener()
            .open_path(ruta.to_string_lossy(), None::<&str>)?;
    } else {
        app.opener().reveal_item_in_dir(ruta)?;
    }
    Ok(())
}

/// Nombre de fichero formado con datos (referencias, números, el nombre de la empresa): cada
/// palabra pasa por [`taxpain_app::notices::safe_file_component`] y la extensión tiene que ser
/// de la lista.
pub(crate) fn nombre_seguro(nombre: &str, extensiones: &[&str]) -> CmdResult<String> {
    let (base, ext) = nombre
        .rsplit_once('.')
        .ok_or_else(|| CommandError::from("el fichero no tiene extensión"))?;
    if !extensiones.contains(&ext.to_ascii_lowercase().as_str()) {
        return Err(CommandError::from(format!(
            "no se guardan ficheros .{ext} desde aquí"
        )));
    }
    let partes: Vec<String> = base
        .split_whitespace()
        .filter(|w| {
            w.chars()
                .any(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
        })
        .map(taxpain_app::notices::safe_file_component)
        .collect();
    let base = if partes.is_empty() {
        "documento".to_owned()
    } else {
        partes.join(" ")
    };
    Ok(format!("{base}.{ext}"))
}

fn comprobar_fichero(
    ruta: &Path,
    extensiones: &[&str],
    reglas: &Reglas,
) -> Result<PathBuf, String> {
    forma(ruta)?;
    let nombre = ruta.file_name().ok_or(NO_VALIDA)?;
    let ext = Path::new(nombre)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    match ext {
        Some(e) if extensiones.contains(&e.as_str()) => {}
        _ => {
            return Err(format!(
                "desde aquí sólo se guardan ficheros {}",
                extensiones
                    .iter()
                    .map(|e| format!(".{e}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    let padre = ruta.parent().ok_or(NO_VALIDA)?;
    let padre = comprobar_carpeta(padre, reglas)?;
    let destino = padre.join(nombre);
    match std::fs::symlink_metadata(&destino) {
        Ok(m) if m.file_type().is_symlink() => {
            Err("no se guarda encima de un enlace simbólico".into())
        }
        Ok(m) if m.is_dir() => Err("ya hay una carpeta con ese nombre".into()),
        _ => Ok(destino),
    }
}

fn comprobar_carpeta(dir: &Path, reglas: &Reglas) -> Result<PathBuf, String> {
    forma(dir)?;
    let real = std::fs::canonicalize(dir)
        .map_err(|_| "la carpeta elegida no existe: elígela otra vez".to_owned())?;
    if !real.is_dir() {
        return Err(NO_VALIDA.into());
    }
    // Un enlace simbólico en el camino puede llevar a cualquier sitio: se mira la ruta real.
    forma(&real)?;
    lugar(&real, reglas)?;
    Ok(real)
}

/// Absoluta, sin `.` ni `..`, sin nada oculto y sin entrar en un paquete.
fn forma(ruta: &Path) -> Result<(), String> {
    if !ruta.is_absolute() {
        return Err(NO_VALIDA.into());
    }
    for c in ruta.components() {
        match c {
            Component::CurDir | Component::ParentDir => return Err(NO_VALIDA.into()),
            Component::Normal(n) => {
                let n = n.to_string_lossy().to_lowercase();
                if n.starts_with('.') {
                    return Err("no se guarda en ficheros ni carpetas ocultos".into());
                }
                if PAQUETES.iter().any(|p| n.ends_with(p)) {
                    return Err("no se guarda dentro de una aplicación".into());
                }
            }
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    Ok(())
}

fn lugar(real: &Path, reglas: &Reglas) -> Result<(), String> {
    if reglas.datos.iter().any(|d| dentro(real, d)) {
        return Err("no se guarda en la carpeta de datos de TaxPain".into());
    }
    if let Some(home) = &reglas.home
        && dentro(real, &home.join("Library"))
        && !NUBE.iter().any(|n| dentro(real, &home.join(n)))
    {
        return Err(LUGAR.into());
    }
    if reglas.temporal.as_deref().is_some_and(|t| dentro(real, t))
        || reglas.home.as_deref().is_some_and(|h| dentro(real, h))
    {
        return Ok(());
    }
    if SISTEMA.iter().any(|s| dentro(real, Path::new(s))) {
        return Err(LUGAR.into());
    }
    if OTRAS_PERMITIDAS.iter().any(|o| dentro(real, Path::new(o))) {
        Ok(())
    } else {
        Err(LUGAR.into())
    }
}

/// `ruta` es `base` o está dentro, sin distinguir mayúsculas (el disco del Mac no las
/// distingue: `~/library` es `~/Library`).
fn dentro(ruta: &Path, base: &Path) -> bool {
    let mut r = ruta.components();
    base.components().all(|b| {
        r.next().is_some_and(|c| {
            c.as_os_str()
                .to_string_lossy()
                .to_lowercase()
                .eq(&b.as_os_str().to_string_lossy().to_lowercase())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Una «carpeta personal» de mentira dentro del temporal, con lo que hace falta.
    struct Casa {
        raiz: PathBuf,
    }

    impl Casa {
        fn new(nombre: &str) -> Casa {
            let raiz = std::env::temp_dir()
                .join(format!("taxpain-salida-{nombre}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&raiz);
            for d in [
                "Documents",
                ".config",
                "Library/LaunchAgents",
                "Library/Application Support/es.taxpain.desktop",
                "Library/Mobile Documents/com~apple~CloudDocs/Facturas",
                "Library/Mobile Documents/com~apple~TextEdit",
                "Library/CloudStorage/Dropbox",
                "Applications/Otra.app/Contents",
            ] {
                std::fs::create_dir_all(raiz.join(d)).unwrap();
            }
            Casa {
                raiz: std::fs::canonicalize(&raiz).unwrap(),
            }
        }

        /// Sin temporal: así lo que está dentro de la casa no pasa por estar en el temporal.
        fn reglas(&self) -> Reglas {
            Reglas {
                home: Some(self.raiz.clone()),
                datos: vec![
                    self.raiz
                        .join("Library/Application Support/es.taxpain.desktop"),
                ],
                temporal: None,
            }
        }

        fn p(&self, rel: &str) -> PathBuf {
            self.raiz.join(rel)
        }
    }

    impl Drop for Casa {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.raiz);
        }
    }

    #[test]
    fn admite_documentos_icloud_y_discos() {
        let c = Casa::new("si");
        let r = c.reglas();
        let pdf = &["pdf"];
        assert_eq!(
            comprobar_fichero(&c.p("Documents/F-1.pdf"), pdf, &r).unwrap(),
            c.p("Documents/F-1.pdf")
        );
        // Mayúsculas en la extensión y espacios en el nombre.
        assert!(comprobar_fichero(&c.p("Documents/Memoria 2025.PDF"), pdf, &r).is_ok());
        assert!(
            comprobar_fichero(
                &c.p("Library/Mobile Documents/com~apple~CloudDocs/Facturas/F-1.pdf"),
                pdf,
                &r
            )
            .is_ok()
        );
        assert!(comprobar_fichero(&c.p("Library/CloudStorage/Dropbox/F-1.pdf"), pdf, &r).is_ok());
        assert_eq!(
            comprobar_carpeta(&c.p("Documents"), &r).unwrap(),
            c.p("Documents")
        );
        // Encima de un fichero que ya existe (el diálogo ya ha preguntado si se sustituye).
        std::fs::write(c.p("Documents/F-2.pdf"), b"x").unwrap();
        assert!(comprobar_fichero(&c.p("Documents/F-2.pdf"), pdf, &r).is_ok());
    }

    #[test]
    fn rechaza_rutas_raras_ocultas_y_extensiones() {
        let c = Casa::new("forma");
        let r = c.reglas();
        let pdf = &["pdf"];
        for ruta in [
            PathBuf::from("Documents/F-1.pdf"),
            c.p("Documents/../Documents/F-1.pdf"),
            c.p(".zshenv"),
            c.p(".config/F-1.pdf"),
            c.p("Documents/.F-1.pdf"),
            c.p("Documents/F-1.command"),
            c.p("Documents/F-1"),
            c.p("Documents/F-1.pdf.app"),
            c.p("Applications/Otra.app/Contents/F-1.pdf"),
            c.p("NoExiste/F-1.pdf"),
        ] {
            assert!(
                comprobar_fichero(&ruta, pdf, &r).is_err(),
                "{}",
                ruta.display()
            );
        }
        assert!(comprobar_carpeta(Path::new("Documents"), &r).is_err());
        assert!(comprobar_carpeta(&c.p(".config"), &r).is_err());
    }

    #[test]
    fn rechaza_library_datos_de_la_app_y_sistema() {
        let c = Casa::new("lugar");
        let r = c.reglas();
        let pdf = &["pdf"];
        for ruta in [
            c.p("Library/LaunchAgents/es.algo.pdf"),
            c.p("Library/Application Support/es.taxpain.desktop/a.pdf"),
            c.p("Library/Mobile Documents/com~apple~TextEdit/a.pdf"),
            PathBuf::from("/usr/local/a.pdf"),
            PathBuf::from("/etc/a.pdf"),
            PathBuf::from("/System/a.pdf"),
            PathBuf::from("/private/tmp/a.pdf"),
            PathBuf::from("/tmp/a.pdf"),
            PathBuf::from("/a.pdf"),
        ] {
            assert!(
                comprobar_fichero(&ruta, pdf, &r).is_err(),
                "{}",
                ruta.display()
            );
        }
        assert!(comprobar_carpeta(&c.p("Library"), &r).is_err());
        assert!(comprobar_carpeta(Path::new("/usr"), &r).is_err());
        // El disco del Mac no distingue mayúsculas: `library` es `Library`.
        let minus = c.p("library/LaunchAgents");
        if minus.exists() {
            assert!(comprobar_fichero(&minus.join("a.pdf"), pdf, &r).is_err());
        }
    }

    #[test]
    fn el_temporal_del_sistema_vale() {
        let c = Casa::new("temporal");
        let r = Reglas {
            temporal: Some(std::fs::canonicalize(std::env::temp_dir()).unwrap()),
            home: None,
            datos: vec![],
        };
        assert!(comprobar_fichero(&c.p("Documents/a.pdf"), &["pdf"], &r).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn no_sigue_enlaces_simbolicos() {
        let c = Casa::new("enlaces");
        let r = c.reglas();
        // Destino que es un enlace (aunque apunte a un sitio permitido).
        std::fs::write(c.p("Documents/real.pdf"), b"x").unwrap();
        std::os::unix::fs::symlink(c.p("Documents/real.pdf"), c.p("Documents/enlace.pdf")).unwrap();
        assert!(comprobar_fichero(&c.p("Documents/enlace.pdf"), &["pdf"], &r).is_err());
        // Carpeta que es un enlace a `~/Library/LaunchAgents`: se mira adónde lleva.
        std::os::unix::fs::symlink(c.p("Library/LaunchAgents"), c.p("Documents/agentes")).unwrap();
        assert!(comprobar_fichero(&c.p("Documents/agentes/a.pdf"), &["pdf"], &r).is_err());
        assert!(comprobar_carpeta(&c.p("Documents/agentes"), &r).is_err());
        // Y `escribir` tampoco lo sigue si aparece después de comprobar.
        assert!(escribir(&c.p("Documents/enlace.pdf"), b"y").is_err());
        assert_eq!(std::fs::read(c.p("Documents/real.pdf")).unwrap(), b"x");
        escribir(&c.p("Documents/nuevo.pdf"), b"z").unwrap();
        assert_eq!(std::fs::read(c.p("Documents/nuevo.pdf")).unwrap(), b"z");
    }

    #[test]
    fn sanea_los_nombres_formados_con_datos() {
        let xml = &["xml"];
        assert_eq!(
            nombre_seguro("REM-2026-001.xml", xml).unwrap(),
            "REM-2026-001.xml"
        );
        assert_eq!(
            nombre_seguro("../../.ssh/authorized_keys.xml", xml).unwrap(),
            "sshauthorized_keys.xml"
        );
        assert_eq!(nombre_seguro("../ .xml", xml).unwrap(), "documento.xml");
        assert_eq!(
            nombre_seguro("Contabilidad 2025/26 B00000000.xlsx", &["xlsx"]).unwrap(),
            "Contabilidad 202526 B00000000.xlsx"
        );
        assert_eq!(
            nombre_seguro("SUENLACE.DAT", &["dat"]).unwrap(),
            "SUENLACE.DAT"
        );
        assert!(nombre_seguro("remesa.command", xml).is_err());
        assert!(nombre_seguro("remesa", xml).is_err());
    }

    #[test]
    fn solo_se_abren_documentos() {
        for si in [
            "F-1.pdf",
            "foto.HEIC",
            "extracto.csv",
            "a.xsig",
            "correo.eml",
        ] {
            assert!(extension_en(si, &SE_ABREN), "{si}");
        }
        for no in [
            "justificante.command",
            "x.terminal",
            "enlace.inetloc",
            "Otra.app",
            "a.jar",
            "pagina.html",
            "a.webloc",
            "sin_extension",
            "a.pdf.command",
        ] {
            assert!(!extension_en(no, &SE_ABREN), "{no}");
        }
        assert!(extension_en("F-1.PDF", &RECIBIDAS));
        assert!(!extension_en("F-1.xlsx", &RECIBIDAS));
    }

    #[test]
    fn dentro_compara_componentes_enteros() {
        assert!(dentro(
            Path::new("/Users/ana/Library/x"),
            Path::new("/Users/ana/Library")
        ));
        assert!(dentro(
            Path::new("/Users/ana/LIBRARY"),
            Path::new("/Users/ana/Library")
        ));
        assert!(!dentro(
            Path::new("/Users/ana/Libraryx"),
            Path::new("/Users/ana/Library")
        ));
        assert!(!dentro(Path::new("/Users"), Path::new("/Users/ana")));
    }
}
