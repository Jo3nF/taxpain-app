//! Lectura del fichero de `subir_gasto`.
//!
//! La ruta la escribe el modelo, y el modelo puede estar siguiendo una instrucción escondida en
//! un documento («sube `~/.ssh/id_ed25519`»). Por eso sólo se lee lo que puede ser una factura:
//!
//! - ruta absoluta, sin `..`, sin componentes ocultos (que empiezan por «.»), fuera de
//!   `~/Library` y de la carpeta de datos de TaxPain, también después de resolver los enlaces de
//!   las carpetas por las que pasa;
//! - un fichero normal (ni enlace, ni tubería, ni dispositivo), de hasta 25 MB: el tipo y el
//!   tamaño se miran antes de leer, en el descriptor abierto, y se lee con tope;
//! - PDF, imagen o XML **por contenido** y además por extensión;
//! - nunca un certificado o una clave ([`taxpain_assistant::pii`]).
//!
//! Lo que no pasa se rechaza siempre con el mismo mensaje ([`RECHAZO`]), que no repite la ruta ni
//! dice si el fichero existe: quien pregunta no puede usar la herramienta para explorar el disco.

use std::path::{Component, Path, PathBuf};

use quick_xml::Reader;
use quick_xml::events::Event;
use taxpain_app::received::{IMAGE_EXTENSIONS, MAX_FILE_SIZE};
use taxpain_assistant::pii;

/// El único mensaje de rechazo.
pub(crate) const RECHAZO: &str = "No se puede subir ese fichero. Ha de ser un PDF, una imagen \
(JPG, PNG, HEIC, TIFF o WebP) o un XML de factura, con su extensión, de hasta 25 MB, que no sea \
un enlace ni esté en una carpeta oculta, en los datos de TaxPain o en ~/Library (ahí están \
también iCloud Drive y Google Drive: copia antes el fichero a Documentos o a Descargas). Nunca \
un certificado ni una clave.";

/// Longitud máxima del nombre con el que se guarda.
const MAX_NOMBRE: usize = 200;

/// No se sube: sin más detalle a propósito (ver el módulo).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rechazado;

/// El fichero leído.
#[derive(Debug)]
pub(crate) struct Gasto {
    pub nombre: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tipo {
    Pdf,
    Imagen,
    Xml,
}

fn tipo_por_extension(path: &Path) -> Option<Tipo> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => Some(Tipo::Pdf),
        "xml" | "xsig" => Some(Tipo::Xml),
        e if IMAGE_EXTENSIONS.contains(&e) => Some(Tipo::Imagen),
        _ => None,
    }
}

/// Ruta absoluta hecha sólo de nombres visibles: ni `..`, ni «.», ni nombres que no sean UTF-8.
fn ruta_limpia(path: &Path) -> bool {
    let mut parts = path.components();
    if parts.next() != Some(Component::RootDir) {
        return false;
    }
    let mut names = 0;
    for c in parts {
        match c {
            Component::Normal(n) => match n.to_str() {
                Some(s) if !s.starts_with('.') => names += 1,
                _ => return false,
            },
            _ => return false,
        }
    }
    names > 0
}

/// ¿Está `path` dentro de `dir` (o es `dir`)? Sin distinguir mayúsculas: el disco del Mac no las
/// distingue, y de más no hace daño.
fn dentro(path: &Path, dir: &Path) -> bool {
    let mut p = path.components();
    dir.components().all(|d| match (p.next(), d) {
        (Some(Component::Normal(a)), Component::Normal(b)) => match (a.to_str(), b.to_str()) {
            (Some(a), Some(b)) => a.to_lowercase() == b.to_lowercase(),
            _ => a == b,
        },
        (Some(a), b) => a == b,
        (None, _) => false,
    })
}

/// ¿Es `dir`, o alguna de las carpetas que la contienen, una de las prohibidas? Se comparan por
/// identidad (dispositivo e inodo), no por nombre: vale también si se llega por otro camino.
#[cfg(unix)]
fn misma_carpeta(dir: &Path, prohibidas: &[PathBuf]) -> bool {
    use std::os::unix::fs::MetadataExt;
    let ids: Vec<(u64, u64)> = prohibidas
        .iter()
        .filter_map(|d| std::fs::metadata(d).ok())
        .map(|m| (m.dev(), m.ino()))
        .collect();
    !ids.is_empty()
        && dir
            .ancestors()
            .filter_map(|a| std::fs::metadata(a).ok())
            .any(|m| ids.contains(&(m.dev(), m.ino())))
}

#[cfg(not(unix))]
fn misma_carpeta(_dir: &Path, _prohibidas: &[PathBuf]) -> bool {
    false
}

/// Lee un fichero normal sin seguir enlaces y sin pasar de `max` bytes (el mismo patrón que las
/// carpetas vigiladas de `taxpain-app`): el tipo y el tamaño se miran en el descriptor abierto.
fn leer_normal(path: &Path, max: u64) -> Result<Vec<u8>, Rechazado> {
    use std::io::Read;

    let before = std::fs::symlink_metadata(path).map_err(|_| Rechazado)?;
    if !before.file_type().is_file() {
        return Err(Rechazado);
    }
    // El tamaño que dice la ruta ya descarta lo grande antes de abrir nada.
    if before.len() == 0 || before.len() > max {
        return Err(Rechazado);
    }
    let f = std::fs::File::open(path).map_err(|_| Rechazado)?;
    let meta = f.metadata().map_err(|_| Rechazado)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // Si entre mirarlo y abrirlo lo han cambiado por otra cosa, el abierto es otro.
        if meta.dev() != before.dev() || meta.ino() != before.ino() {
            return Err(Rechazado);
        }
    }
    if !meta.is_file() || meta.len() == 0 || meta.len() > max {
        return Err(Rechazado);
    }
    let mut out = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
    f.take(max.saturating_add(1))
        .read_to_end(&mut out)
        .map_err(|_| Rechazado)?;
    if out.is_empty() || out.len() as u64 > max {
        return Err(Rechazado);
    }
    Ok(out)
}

/// Firmas de imagen: las que reconoce `taxpain_app::received` (JPEG, PNG, HEIC) y las de los
/// otros dos formatos que admite por extensión (TIFF y WebP).
fn es_imagen(b: &[u8]) -> bool {
    b.starts_with(&[0xFF, 0xD8, 0xFF])
        || b.starts_with(b"\x89PNG\r\n\x1a\n")
        || b.get(4..12)
            .is_some_and(|t| t == b"ftypheic" || t == b"ftypheix" || t == b"ftypmif1")
        || b.starts_with(b"II*\0")
        || b.starts_with(b"MM\0*")
        || (b.starts_with(b"RIFF") && b.get(8..12) == Some(b"WEBP".as_slice()))
}

/// XML bien formado (UTF-8, un solo elemento raíz, etiquetas que cierran, sin DTD) y sin nada
/// que parezca una clave o un certificado fuera de su firma electrónica (la firma de una
/// Facturae lleva el certificado público de quien firma: eso sí es normal).
fn es_xml_de_factura(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let mut reader = Reader::from_str(text);
    let mut depth = 0usize;
    let mut roots = 0usize;
    // Profundidad a la que se abrió la firma (`ds:Signature`) en la que se está, si se está.
    let mut firma: Option<usize> = None;
    let mut fuera_de_la_firma = String::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                if depth == 0 {
                    roots += 1;
                }
                depth += 1;
                if firma.is_none() && e.local_name().into_inner() == "Signature" {
                    firma = Some(depth);
                }
            }
            Ok(Event::End(_)) => {
                if firma == Some(depth) {
                    firma = None;
                }
                let Some(d) = depth.checked_sub(1) else {
                    return false;
                };
                depth = d;
            }
            Ok(Event::Empty(_)) => {
                if depth == 0 {
                    roots += 1;
                }
            }
            Ok(Event::Text(t)) => {
                let t: &str = &t;
                if depth == 0 && !t.trim().is_empty() {
                    return false;
                }
                if firma.is_none() {
                    fuera_de_la_firma.push_str(t);
                    fuera_de_la_firma.push('\n');
                }
            }
            Ok(Event::CData(t)) => {
                if depth == 0 {
                    return false;
                }
                if firma.is_none() {
                    fuera_de_la_firma.push_str(&t);
                    fuera_de_la_firma.push('\n');
                }
            }
            Ok(Event::GeneralRef(_)) => {
                if depth == 0 {
                    return false;
                }
            }
            Ok(Event::DocType(_)) => return false,
            Ok(Event::Comment(_) | Event::Decl(_) | Event::PI(_)) => {}
            Ok(Event::Eof) => break,
            Err(_) => return false,
        }
    }
    depth == 0 && roots == 1 && pii::looks_secret(&fuera_de_la_firma).is_none()
}

fn contenido_valido(tipo: Tipo, bytes: &[u8]) -> bool {
    if pii::der_key_or_cert(bytes) {
        return false;
    }
    match tipo {
        Tipo::Pdf => bytes.starts_with(b"%PDF-"),
        Tipo::Imagen => es_imagen(bytes),
        Tipo::Xml => es_xml_de_factura(bytes),
    }
}

/// Lee el fichero de `ruta` si puede ser una factura (ver el módulo). `prohibidas`: carpetas de
/// las que no se lee nada (`~/Library`, los datos de TaxPain).
pub(crate) fn leer(ruta: &str, prohibidas: &[PathBuf]) -> Result<Gasto, Rechazado> {
    let path = Path::new(ruta);
    // Primero, lo que se ve en la propia ruta, sin tocar el disco.
    if !ruta_limpia(path) || prohibidas.iter().any(|d| dentro(path, d)) {
        return Err(Rechazado);
    }
    let tipo = tipo_por_extension(path).ok_or(Rechazado)?;
    let (Some(parent), Some(name)) = (path.parent(), path.file_name().and_then(|n| n.to_str()))
    else {
        return Err(Rechazado);
    };
    // Después, dónde está de verdad: una carpeta del camino puede ser un enlace a otra parte.
    let dir = std::fs::canonicalize(parent).map_err(|_| Rechazado)?;
    let real = dir.join(name);
    if !ruta_limpia(&real)
        || prohibidas.iter().any(|d| dentro(&real, d))
        || misma_carpeta(&dir, prohibidas)
    {
        return Err(Rechazado);
    }
    let bytes = leer_normal(&real, MAX_FILE_SIZE as u64)?;
    if !contenido_valido(tipo, &bytes) {
        return Err(Rechazado);
    }
    Ok(Gasto {
        nombre: nombre_guardado(name),
        bytes,
    })
}

/// El nombre con el que se guarda: sin caracteres invisibles y con tope, conservando la
/// extensión (por ella se abre luego con la app que toca).
fn nombre_guardado(name: &str) -> String {
    let clean = pii::strip_invisible(name);
    if clean.chars().count() <= MAX_NOMBRE {
        return clean;
    }
    let (stem, ext) = clean.rsplit_once('.').unwrap_or((clean.as_str(), ""));
    let keep = MAX_NOMBRE.saturating_sub(ext.chars().count() + 1);
    let stem: String = stem.chars().take(keep).collect();
    format!("{stem}.{ext}")
}

/// Carpetas de las que `subir_gasto` no lee: `~/Library` y la de datos de TaxPain, tal como se
/// escriben y con los enlaces resueltos (en el Mac, `/var` es `/private/var`).
pub(crate) fn prohibidas(home: Option<PathBuf>, data_root: Option<PathBuf>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for dir in home.map(|h| h.join("Library")).into_iter().chain(data_root) {
        if let Ok(real) = std::fs::canonicalize(&dir)
            && real != dir
        {
            out.push(real);
        }
        out.push(dir);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const PDF: &[u8] = b"%PDF-1.4\nFactura 7 de Proveedor Inventado SL, total 121,00 EUR\n%%EOF\n";

    /// Carpeta temporal con nombre visible (las de `tempfile` empiezan por «.» por defecto).
    fn tmp() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("taxpain-mcp-")
            .tempdir()
            .unwrap()
    }

    fn escribir(dir: &Path, name: &str, bytes: &[u8]) -> String {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p.to_string_lossy().into_owned()
    }

    #[test]
    fn lee_pdf_imagen_y_xml() {
        let t = tmp();
        let g = leer(&escribir(t.path(), "factura.pdf", PDF), &[]).unwrap();
        assert_eq!(g.nombre, "factura.pdf");
        assert_eq!(g.bytes, PDF);
        for (name, bytes) in [
            ("foto.JPG", &[0xFF, 0xD8, 0xFF, 0xE0, 0, 0][..]),
            ("captura.png", b"\x89PNG\r\n\x1a\n0000"),
            ("movil.heic", b"\0\0\0\x18ftypheic\0\0\0\0"),
            ("escaner.tiff", b"II*\0\x08\0\0\0"),
            ("web.webp", b"RIFF\x10\0\0\0WEBPVP8 "),
            (
                "facturae.xml",
                b"<?xml version=\"1.0\"?>\n<Facturae><Total>121.00</Total></Facturae>\n",
            ),
        ] {
            assert!(
                leer(&escribir(t.path(), name, bytes), &[]).is_ok(),
                "{name}"
            );
        }
    }

    #[test]
    fn el_contenido_manda_no_la_extension() {
        let t = tmp();
        for (name, bytes) in [
            // Una clave con extensión de factura.
            (
                "clave.pdf",
                &b"-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjE\n"[..],
            ),
            ("texto.pdf", b"Factura 7, total 121,00 EUR"),
            ("texto.png", b"no es una imagen"),
            ("roto.xml", b"<Facturae><Total>121.00</Facturae>"),
            ("abierto.xml", b"<Facturae><Total>121.00</Total>"),
            ("dos.xml", b"<a/><b/>"),
            ("vacio.xml", b"<?xml version=\"1.0\"?>\n"),
            ("dtd.xml", b"<!DOCTYPE a [<!ENTITY x \"y\">]><a>&x;</a>"),
            ("latin1.xml", b"<a>Espa\xF1a</a>"),
            // Un PDF con otra extensión, y una extensión que no es de factura.
            ("factura.txt", PDF),
            ("factura", PDF),
            ("id_ed25519", b"-----BEGIN OPENSSH PRIVATE KEY-----\n"),
            ("datos.plist", b"<?xml version=\"1.0\"?><plist/>"),
            ("vacio.pdf", b""),
        ] {
            assert_eq!(
                leer(&escribir(t.path(), name, bytes), &[]).unwrap_err(),
                Rechazado,
                "{name}"
            );
        }
    }

    #[test]
    fn un_xml_con_una_clave_no_pasa_pero_una_facturae_firmada_si() {
        let t = tmp();
        let clave = "<a><k>-----BEGIN PRIVATE KEY-----\nMIIEvQIBADANBgkqhkiG9w0BAQEFAASC\n\
                     -----END PRIVATE KEY-----</k></a>";
        assert!(leer(&escribir(t.path(), "clave.xml", clave.as_bytes()), &[]).is_err());
        let api = "<a><token>sk-ant-abcdefghijklmnopqrstuvwxyz0123456789</token></a>";
        assert!(leer(&escribir(t.path(), "api.xml", api.as_bytes()), &[]).is_err());
        // La firma XAdES lleva el certificado público del firmante y bloques largos en base64.
        let b64 = "TUlJ".repeat(80);
        let firmada = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<fe:Facturae xmlns:fe=\"urn:x\" \
             xmlns:ds=\"urn:y\"><Total>121.00</Total><ds:Signature><ds:SignatureValue>{b64}\
             </ds:SignatureValue><ds:X509Certificate>{b64}</ds:X509Certificate></ds:Signature>\
             </fe:Facturae>"
        );
        assert!(leer(&escribir(t.path(), "firmada.xsig", firmada.as_bytes()), &[]).is_ok());
        // El mismo bloque fuera de la firma, no.
        let fuera = format!("<a><b>{b64}</b></a>");
        assert!(leer(&escribir(t.path(), "fuera.xml", fuera.as_bytes()), &[]).is_err());
    }

    #[test]
    fn rutas_que_no_se_leen() {
        let t = tmp();
        let ok = escribir(t.path(), "factura.pdf", PDF);
        assert!(leer(&ok, &[]).is_ok());
        // Relativa, con «..», con un componente oculto.
        assert!(leer("factura.pdf", &[]).is_err());
        let sub = t.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        let vuelta = format!("{}/../factura.pdf", sub.to_string_lossy());
        assert!(leer(&vuelta, &[]).is_err());
        let oculta = t.path().join(".ssh");
        std::fs::create_dir(&oculta).unwrap();
        assert!(leer(&escribir(&oculta, "factura.pdf", PDF), &[]).is_err());
        assert!(leer(&escribir(t.path(), ".factura.pdf", PDF), &[]).is_err());
        // Una carpeta, y un fichero que no existe: el mismo rechazo.
        std::fs::create_dir(t.path().join("carpeta.pdf")).unwrap();
        let carpeta = t.path().join("carpeta.pdf");
        assert!(leer(&carpeta.to_string_lossy(), &[]).is_err());
        assert!(leer("/no/existe/factura.pdf", &[]).is_err());
    }

    #[test]
    fn carpetas_prohibidas_tambien_por_un_enlace_o_con_otras_mayusculas() {
        let t = tmp();
        let home = t.path().join("casa");
        let library = home.join("Library");
        let datos = t.path().join("datos de taxpain");
        std::fs::create_dir_all(library.join("Preferences")).unwrap();
        std::fs::create_dir_all(datos.join("companies")).unwrap();
        let no = prohibidas(Some(home.clone()), Some(datos.clone()));
        let en_library = escribir(&library.join("Preferences"), "factura.pdf", PDF);
        assert!(leer(&en_library, &[]).is_ok());
        assert!(leer(&en_library, &no).is_err());
        assert!(leer(&en_library.replace("Library", "LIBRARY"), &no).is_err());
        assert!(leer(&escribir(&datos.join("companies"), "factura.pdf", PDF), &no).is_err());
        // Sin que el fichero exista, tampoco (no se llega a mirar el disco).
        let fantasma = library.join("no-existe.pdf");
        assert!(leer(&fantasma.to_string_lossy(), &no).is_err());
        // Fuera de las prohibidas, sí.
        let fuera = home.join("Documents");
        std::fs::create_dir_all(&fuera).unwrap();
        assert!(leer(&escribir(&fuera, "factura.pdf", PDF), &no).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn ni_enlaces_ni_tuberias_ni_dispositivos() {
        use std::os::unix::fs::symlink;
        let t = tmp();
        let real = escribir(t.path(), "factura.pdf", PDF);
        // Un enlace al fichero, aunque el destino sea una factura.
        let link = t.path().join("enlace.pdf");
        symlink(&real, &link).unwrap();
        assert!(leer(&link.to_string_lossy(), &[]).is_err());
        // Un enlace a un dispositivo sin fin: no se abre (no agota la memoria).
        let zero = t.path().join("cero.pdf");
        symlink("/dev/zero", &zero).unwrap();
        assert!(leer(&zero.to_string_lossy(), &[]).is_err());
        // Una carpeta enlazada a una prohibida: se mira dónde está de verdad.
        let secreta = t.path().join("secreta");
        std::fs::create_dir(&secreta).unwrap();
        escribir(&secreta, "factura.pdf", PDF);
        let atajo = t.path().join("atajo");
        symlink(&secreta, &atajo).unwrap();
        let por_atajo = atajo.join("factura.pdf");
        assert!(leer(&por_atajo.to_string_lossy(), &[]).is_ok());
        assert!(leer(&por_atajo.to_string_lossy(), std::slice::from_ref(&secreta)).is_err());
        // Una carpeta oculta alcanzada por un enlace con nombre visible.
        let oculta = t.path().join(".oculta");
        std::fs::create_dir(&oculta).unwrap();
        escribir(&oculta, "factura.pdf", PDF);
        let visible = t.path().join("visible");
        symlink(&oculta, &visible).unwrap();
        assert!(leer(&visible.join("factura.pdf").to_string_lossy(), &[]).is_err());
        // Una tubería con nombre: no se abre (no se queda colgado esperando a quien escriba).
        let fifo = t.path().join("tubo.pdf");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .is_ok_and(|s| s.success());
        if made {
            assert!(leer(&fifo.to_string_lossy(), &[]).is_err());
        }
    }

    #[test]
    fn el_tope_se_mira_antes_de_leer() {
        let t = tmp();
        let p = t.path().join("grande.pdf");
        let f = std::fs::File::create(&p).unwrap();
        // Fichero disperso: 25 MB y un byte sin ocupar disco.
        f.set_len(MAX_FILE_SIZE as u64 + 1).unwrap();
        assert!(leer(&p.to_string_lossy(), &[]).is_err());
        assert!(leer_normal(&p, MAX_FILE_SIZE as u64).is_err());
        // Con el tope justo, se lee.
        f.set_len(64).unwrap();
        assert_eq!(leer_normal(&p, 64).unwrap().len(), 64);
        assert!(leer_normal(&p, 63).is_err());
    }

    #[test]
    fn el_nombre_se_guarda_sin_invisibles_y_con_tope() {
        let t = tmp();
        let g = leer(&escribir(t.path(), "fac\u{200B}tura\u{202E}.pdf", PDF), &[]).unwrap();
        assert_eq!(g.nombre, "factura.pdf");
        let largo = format!("{}.pdf", "a".repeat(230));
        let g = leer(&escribir(t.path(), &largo, PDF), &[]).unwrap();
        assert_eq!(g.nombre.chars().count(), MAX_NOMBRE);
        assert!(g.nombre.ends_with("aaa.pdf"));
    }
}
