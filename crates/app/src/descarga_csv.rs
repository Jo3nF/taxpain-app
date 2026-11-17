//! «Descargar de la AEAT por CSV»: el usuario pega los códigos seguros de verificación (CSV) que
//! ve en la Sede (asientos registrales, «Mis expedientes», justificantes…) y la app baja cada
//! documento en PDF a la carpeta de la empresa, en la subcarpeta [`SUBFOLDER`], para que se lea
//! como el resto de la carpeta.
//!
//! Aquí está lo que no necesita la red: sacar los CSV del texto pegado, dar nombre al PDF,
//! guardarlo sin pisar nada y decir qué pasó con cada código. La descarga (servicio web de
//! cotejo con el certificado y, si no, la Sede pública) está en la app (`presentado.rs`), con
//! `taxpain_aeat::cotejo`.
//!
//! Los CSV dan acceso al documento: no se escriben nunca en el registro (`tracing`).

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use taxpain_vault::Vault;

use crate::error::{AppError, Result};

/// Subcarpeta de la carpeta de la empresa donde se guardan los documentos descargados.
pub const SUBFOLDER: &str = "Documentos AEAT";

/// Códigos que se descargan de una vez como máximo (la AEAT limita los accesos al cotejo).
pub const MAX_CSV: usize = 50;

/// Largo máximo del título en el nombre del fichero (en letras).
const MAX_TITLE: usize = 60;

/// Tamaño máximo de un PDF de la subcarpeta que se lee para ver si ya estaba.
const MAX_COMPARE: u64 = 30 * 1024 * 1024;

/// Un CSV de la AEAT: 16 letras mayúsculas o cifras (la misma regla que
/// `taxpain_aeat::cotejo::valid_csv`; este crate no depende de `taxpain-aeat`).
pub fn valid_csv(csv: &str) -> bool {
    csv.len() == 16
        && csv
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

/// Los CSV de un texto pegado: uno por línea, separados por espacios o comas, o una tabla de la
/// Sede copiada con más columnas. En mayúsculas, sin repetidos y en el orden en que aparecen.
///
/// Para no confundir otras palabras de 16 letras con un código, una palabra cuenta si tiene
/// letras y cifras; una sólo de letras, si está sola en su línea y en mayúsculas (pegada a
/// propósito). Las que son sólo cifras (una tarjeta, una cuenta) nunca. Las huellas (SHA-1,
/// SHA-256), los IBAN o los NIF no tienen 16 caracteres seguidos y no cuentan.
///
/// Más de [`MAX_CSV`] códigos distintos es un error con un mensaje claro.
pub fn extract(text: &str) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let alone = line.trim();
        for token in line.split(|c: char| !c.is_ascii_alphanumeric()) {
            if token.len() != 16 {
                continue;
            }
            let upper = token.to_ascii_uppercase();
            if !valid_csv(&upper) {
                continue;
            }
            let letters = upper.bytes().any(|b| b.is_ascii_uppercase());
            let digits = upper.bytes().any(|b| b.is_ascii_digit());
            let ok = letters && (digits || (alone == token && token == upper));
            if ok && !out.contains(&upper) {
                out.push(upper);
            }
        }
    }
    if out.len() > MAX_CSV {
        return Err(AppError::Other(format!(
            "Has pegado {} códigos: como mucho {MAX_CSV} de cada vez. Pega los primeros {MAX_CSV} \
             y, cuando acabe, el resto.",
            out.len()
        )));
    }
    Ok(out)
}

/// La carpeta de la empresa recordada (la de Importar), que tiene que existir.
pub fn root(v: &Vault) -> Result<PathBuf> {
    let Some(folder) = crate::carpeta::folder(v)? else {
        return Err(AppError::Other(
            "Elige primero la carpeta de la empresa en Importar.".into(),
        ));
    };
    let root = PathBuf::from(folder);
    if !root.is_dir() {
        return Err(AppError::Other(
            "La carpeta de la empresa ya no está donde estaba: elígela otra vez en Importar."
                .into(),
        ));
    }
    Ok(root)
}

/// Palabras con las que empieza el título de un documento de la AEAT.
const TITLE_STARTS: [&str; 27] = [
    "justificante",
    "requerimiento",
    "notificación",
    "notificacion",
    "comunicación",
    "comunicacion",
    "acuerdo",
    "resolución",
    "resolucion",
    "propuesta",
    "liquidación",
    "liquidacion",
    "certificado",
    "certificación",
    "certificacion",
    "diligencia",
    "providencia",
    "acuse de recibo",
    "recibo",
    "carta de pago",
    "documento de ingreso",
    "solicitud",
    "declaración",
    "declaracion",
    "autoliquidación",
    "autoliquidacion",
    "modelo ",
];

/// Un título corto del documento sacado de su texto: la primera línea (entre las primeras) que
/// empieza como empiezan los documentos de la AEAT («Justificante de presentación»,
/// «Requerimiento de información», «Acuerdo de…»). `None` si no hay ninguna.
pub fn title(text: &str) -> Option<String> {
    let lines: Vec<String> = text
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .take(60)
        .collect();
    let starts = |l: &str| {
        let lower = l.to_lowercase();
        TITLE_STARTS.iter().any(|k| lower.starts_with(k))
    };
    let caps = |s: &str| s.chars().any(char::is_alphabetic) && !s.chars().any(char::is_lowercase);
    let found = lines.iter().position(|l| {
        l.chars().count() >= 8
            && starts(l)
            // Una línea que es casi todo cifras (un número de justificante) no es un título.
            && l.chars().filter(char::is_ascii_digit).count() * 2 < l.chars().count()
    });
    let raw = match found {
        Some(i) => {
            let mut t = lines[i].clone();
            // Un título en mayúsculas partido en dos líneas («JUSTIFICANTE DE PRESENTACIÓN SIN»
            // y «CUOTA A INGRESAR»): la segunda, corta, sin cifras y también en mayúsculas.
            if let Some(next) = lines.get(i + 1)
                && caps(&t)
                && caps(next)
                && next.chars().count() <= 40
                && !next.chars().any(|c| c.is_ascii_digit())
                && !starts(next)
            {
                t = format!("{t} {next}");
            }
            t
        }
        // Un escrito del contribuyente a la AEAT («A LA AGENCIA ESTATAL…», «EXPONE»): no tiene
        // título propio.
        None if lines.iter().take(15).any(|l| {
            let lower = l.to_lowercase();
            lower.starts_with("a la agencia estatal") || lower == "expone"
        }) =>
        {
            "Escrito a la AEAT".to_owned()
        }
        None => return None,
    };
    Some(sanitize(&raw)).filter(|t| t.chars().count() >= 4)
}

/// Un título apto para nombre de fichero en el Mac: sin `/`, `:` ni signos raros, espacios
/// seguidos en uno, sin puntos al principio ni al final y como mucho [`MAX_TITLE`] letras (se
/// corta por una palabra). Si viene todo en mayúsculas, en minúsculas con la primera en
/// mayúscula.
pub fn sanitize(title: &str) -> String {
    let clean: String = title
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || " -_,()ºª".contains(c) {
                c
            } else {
                ' '
            }
        })
        .collect();
    let mut s = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    if !s.chars().any(char::is_lowercase) {
        let lower = s.to_lowercase();
        let mut chars = lower.chars();
        s = chars
            .next()
            .map(|f| f.to_uppercase().chain(chars).collect())
            .unwrap_or_default();
    }
    if s.chars().count() > MAX_TITLE {
        let cut: String = s.chars().take(MAX_TITLE).collect();
        s = match cut.rfind(' ') {
            Some(i) if i > MAX_TITLE / 2 => cut[..i].to_owned(),
            _ => cut,
        };
    }
    s.trim_matches(|c: char| c == ' ' || c == '-' || c == '_' || c == ',')
        .to_owned()
}

/// Nombre del fichero: «<título> - <CSV>.pdf» si del texto del PDF sale un título y, si no,
/// «<CSV>.pdf».
pub fn file_name(csv: &str, pdf: &[u8]) -> String {
    match crate::pdf_text::text(pdf).as_deref().and_then(title) {
        Some(t) => format!("{t} - {csv}.pdf"),
        None => format!("{csv}.pdf"),
    }
}

/// Dónde quedó un documento guardado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Saved {
    /// Nuevo, con su ruta relativa a la carpeta de la empresa.
    New(String),
    /// Ya estaba (un fichero con ese CSV en el nombre o con el mismo contenido): su ruta.
    Already(String),
}

fn rel(name: &str) -> String {
    format!("{SUBFOLDER}/{name}")
}

/// Un error del disco al escribir en la subcarpeta, en palabras llanas (sin el texto del
/// sistema en inglés).
fn io_error(e: &std::io::Error) -> AppError {
    use std::io::ErrorKind;
    let why = match e.kind() {
        ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem => {
            "no hay permiso para escribir en ella (o el disco es de sólo lectura)"
        }
        ErrorKind::StorageFull | ErrorKind::QuotaExceeded => "el disco está lleno",
        ErrorKind::NotFound => "la carpeta de la empresa ya no está donde estaba",
        _ => "el disco no deja escribir",
    };
    AppError::Other(format!(
        "No se puede guardar en la carpeta «{SUBFOLDER}»: {why}. Revisa la carpeta de la \
         empresa y vuelve a intentarlo."
    ))
}

/// La subcarpeta [`SUBFOLDER`] de `root`, creada si no está. Un enlace o un fichero con ese
/// nombre no se usa (no se escribe a través de él).
fn subfolder(root: &Path) -> Result<PathBuf> {
    if !root.is_dir() {
        return Err(AppError::Other(
            "La carpeta de la empresa ya no está donde estaba: elígela otra vez en Importar."
                .into(),
        ));
    }
    let dir = root.join(SUBFOLDER);
    match std::fs::symlink_metadata(&dir) {
        Ok(m) if m.is_dir() => {}
        Ok(_) => {
            return Err(AppError::Other(format!(
                "En la carpeta de la empresa hay algo llamado «{SUBFOLDER}» que no es una \
                 carpeta: cámbiale el nombre y vuelve a intentarlo."
            )));
        }
        Err(_) => std::fs::create_dir(&dir).map_err(|e| io_error(&e))?,
    }
    Ok(dir)
}

/// Prepara la subcarpeta antes de descargar nada y comprueba que se puede escribir en ella
/// (con un fichero oculto que se borra al momento): si no se puede, un solo error claro en vez
/// de uno por código, y sin pedir nada a la AEAT.
pub fn prepare(root: &Path) -> Result<PathBuf> {
    let dir = subfolder(root)?;
    let probe = temp_path(&dir, 0);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .and_then(|mut f| f.write_all(b"x"))
        .map_err(|e| io_error(&e))?;
    let _ = std::fs::remove_file(&probe);
    Ok(dir)
}

/// Un nombre oculto (empieza por «.», la lectura de la carpeta no lo mira) para escribir el
/// PDF antes de darle su nombre: si la app se cierra a medias, no queda un PDF cortado con el
/// CSV en el nombre.
fn temp_path(dir: &Path, i: usize) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or_default();
    dir.join(format!(
        ".taxpain-descarga-{}-{nanos}-{i}.tmp",
        std::process::id()
    ))
}

/// ¿Es un PDF entero? Empieza por `%PDF` y tiene `%%EOF` al final (un fichero cortado, no).
fn complete_pdf(path: &Path) -> bool {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; 4];
    if f.read_exact(&mut head).is_err() || &head != b"%PDF" {
        return false;
    }
    let Ok(len) = f.seek(SeekFrom::End(0)) else {
        return false;
    };
    let from = len.saturating_sub(1024);
    let mut tail = Vec::new();
    f.seek(SeekFrom::Start(from)).is_ok()
        && f.read_to_end(&mut tail).is_ok()
        && tail.windows(5).any(|w| w == b"%%EOF")
}

/// Ficheros normales (no ocultos, ni enlaces, ni carpetas) de la subcarpeta, por nombre.
fn files(dir: &Path) -> Vec<(String, PathBuf, u64)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<_> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let meta = std::fs::symlink_metadata(e.path()).ok()?;
            (meta.is_file() && !name.starts_with('.')).then(|| (name, e.path(), meta.len()))
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Si en la subcarpeta ya hay un PDF entero con `csv` en el nombre, su ruta relativa a la
/// carpeta de la empresa. Se mira **antes** de pedirlo a la AEAT (no se gastan accesos al
/// cotejo en lo que ya está). Un fichero cortado (de una descarga interrumpida) no cuenta.
pub fn already(root: &Path, csv: &str) -> Option<String> {
    let dir = root.join(SUBFOLDER);
    if !std::fs::symlink_metadata(&dir).is_ok_and(|m| m.is_dir()) {
        return None;
    }
    files(&dir)
        .into_iter()
        .find(|(name, path, _)| {
            let upper = name.to_ascii_uppercase();
            upper.contains(csv) && upper.ends_with(".PDF") && complete_pdf(path)
        })
        .map(|(name, ..)| rel(&name))
}

/// Guarda el PDF de `csv` en la subcarpeta [`SUBFOLDER`] de `root` (la crea si no está). Nunca
/// sobrescribe: si ya hay un PDF con el mismo CSV en el nombre o con el mismo contenido, no lo
/// duplica y dice cuál es. Se escribe primero en un fichero oculto y, ya entero, se le da el
/// nombre (sin pisar otro).
pub fn save(root: &Path, csv: &str, pdf: &[u8]) -> Result<Saved> {
    if !valid_csv(csv) {
        return Err(AppError::Other("el código CSV no es válido".into()));
    }
    if !pdf.starts_with(b"%PDF") {
        return Err(AppError::Other(
            "lo que ha dado la AEAT no es un PDF: no se ha guardado".into(),
        ));
    }
    let dir = subfolder(root)?;
    if let Some(path) = already(root, csv) {
        return Ok(Saved::Already(path));
    }
    let hash = Sha256::digest(pdf);
    for (name, path, len) in files(&dir) {
        if len == pdf.len() as u64
            && len <= MAX_COMPARE
            && std::fs::read(&path).is_ok_and(|b| Sha256::digest(&b) == hash)
        {
            return Ok(Saved::Already(rel(&name)));
        }
    }
    let temp = write_temp(&dir, pdf)?;
    let result = place(&dir, &temp, &file_name(csv, pdf));
    let _ = std::fs::remove_file(&temp);
    result
}

/// Escribe `pdf` entero en un fichero oculto nuevo de `dir`.
fn write_temp(dir: &Path, pdf: &[u8]) -> Result<PathBuf> {
    for i in 1..=20 {
        let path = temp_path(dir, i);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                if let Err(e) = f.write_all(pdf).and_then(|()| f.sync_all()) {
                    drop(f);
                    let _ = std::fs::remove_file(&path);
                    return Err(io_error(&e));
                }
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(io_error(&e)),
        }
    }
    Err(AppError::Other(
        "no se ha encontrado un nombre libre en la subcarpeta".into(),
    ))
}

/// Da al fichero oculto `temp` el nombre `name` (o «<nombre> (2).pdf»… si está ocupado) sin
/// pisar nada: un enlace duro, que falla si el nombre existe. En discos sin enlaces duros, se
/// renombra si el nombre está libre.
fn place(dir: &Path, temp: &Path, name: &str) -> Result<Saved> {
    let stem = name.trim_end_matches(".pdf").to_owned();
    for i in 1..=20 {
        let candidate = if i == 1 {
            name.to_owned()
        } else {
            format!("{stem} ({i}).pdf")
        };
        let path = dir.join(&candidate);
        match std::fs::hard_link(temp, &path) {
            Ok(()) => return Ok(Saved::New(rel(&candidate))),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) if std::fs::symlink_metadata(&path).is_err() => {
                std::fs::rename(temp, &path).map_err(|e| io_error(&e))?;
                return Ok(Saved::New(rel(&candidate)));
            }
            Err(e) => return Err(io_error(&e)),
        }
    }
    Err(AppError::Other(
        "no se ha encontrado un nombre libre en la subcarpeta".into(),
    ))
}

/// Lo que dio la AEAT para un CSV.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fetched {
    Pdf(Vec<u8>),
    /// La AEAT no tiene ningún documento con ese código.
    NotFound,
    /// Anulado; si la AEAT lo dice, el CSV del documento que lo sustituye.
    Annulled(Option<String>),
    /// Existe, pero la AEAT no deja descargarlo (con su explicación).
    NotAvailable(String),
    /// No se pudo descargar (red, la Sede no responde, límite de accesos…).
    Failed(String),
}

/// Qué pasó con un CSV.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum State {
    /// Guardado: ruta relativa a la carpeta de la empresa.
    Guardado {
        path: String,
    },
    /// Ya estaba en la subcarpeta: su ruta.
    YaEstaba {
        path: String,
    },
    NoEncontrado,
    /// Anulado; `replacement`, el CSV del que lo sustituye (para ofrecer descargarlo).
    Anulado {
        replacement: Option<String>,
    },
    NoDisponible {
        message: String,
    },
    Error {
        message: String,
    },
}

/// Resultado de un CSV, para la pantalla.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Item {
    pub csv: String,
    #[serde(flatten)]
    pub state: State,
}

/// Guarda lo que dio la AEAT para `csv` y dice qué pasó.
pub fn store(root: &Path, csv: &str, fetched: Fetched) -> Item {
    let state = match fetched {
        Fetched::Pdf(b) => match save(root, csv, &b) {
            Ok(Saved::New(path)) => State::Guardado { path },
            Ok(Saved::Already(path)) => State::YaEstaba { path },
            Err(e) => State::Error {
                message: e.to_string(),
            },
        },
        Fetched::NotFound => State::NoEncontrado,
        Fetched::Annulled(r) => State::Anulado {
            replacement: r
                .map(|r| r.trim().to_ascii_uppercase())
                .filter(|r| valid_csv(r) && r != csv),
        },
        Fetched::NotAvailable(message) => State::NoDisponible { message },
        Fetched::Failed(message) => State::Error { message },
    };
    Item {
        csv: csv.to_owned(),
        state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "ABCD1234EFGH5678";
    const B: &str = "Z9Y8X7W6V5U4T3S2";

    #[test]
    fn saca_los_csv_de_lo_pegado() {
        // Uno por línea, con espacios y comas, en minúsculas y repetidos.
        let t = format!("{A}\n  {B} , {}\n{A}", A.to_lowercase());
        assert_eq!(extract(&t).unwrap(), vec![A.to_owned(), B.to_owned()]);
        // Una tabla de la Sede copiada con más columnas (tabuladores).
        let tabla = format!(
            "Número\tFecha\tAsunto\tCSV\n2025GRC1234567\t12/03/2025\tRequerimiento\t{B}\n\
             2025GRC1234568\t13/03/2025\tNotificación\t{A}\n"
        );
        assert_eq!(extract(&tabla).unwrap(), vec![B, A]);
        assert!(extract("").unwrap().is_empty());
    }

    #[test]
    fn no_confunde_otras_palabras_con_un_csv() {
        let t = "ADMINISTRACIONES establecimientos B12345674 \
                 1234567812345678 \
                 da39a3ee5e6b4b0d3255bfef95601890afd80709 \
                 ES91 2100 0418 4502 0005 1332 ES9121000418450200051332 \
                 ABCD1234EFGH567";
        assert!(extract(t).unwrap().is_empty(), "{:?}", extract(t));
        // Sólo letras, sola en su línea y en mayúsculas: un CSV pegado a propósito.
        assert_eq!(
            extract("QWERTYUIOPASDFGH\n").unwrap(),
            vec!["QWERTYUIOPASDFGH"]
        );
        assert!(extract("qwertyuiopasdfgh").unwrap().is_empty());
        // En una tabla, una palabra de 16 letras en mayúsculas no está sola: no cuenta.
        assert_eq!(
            extract(&format!("ADMINISTRACIONES\tRequerimiento\t{A}\r\n")).unwrap(),
            vec![A]
        );
    }

    #[test]
    fn lineas_con_retorno_de_carro() {
        // Pegado desde Windows o desde una tabla web: «\r\n».
        let t = format!("{A}\r\n{B}\r\nQWERTYUIOPASDFGH\r\n");
        assert_eq!(extract(&t).unwrap(), vec![A, B, "QWERTYUIOPASDFGH"]);
    }

    #[test]
    fn como_mucho_cincuenta_de_una_vez() {
        let t: String = (0..51).map(|i| format!("ABCDEFGH1234{i:04}\n")).collect();
        let e = extract(&t).unwrap_err().to_string();
        assert!(e.contains("51") && e.contains("50"), "{e}");
        let t: String = (0..50).map(|i| format!("ABCDEFGH1234{i:04}\n")).collect();
        assert_eq!(extract(&t).unwrap().len(), 50);
    }

    #[test]
    fn titulo_del_texto_del_pdf() {
        let t = "Agencia Tributaria\n\nJUSTIFICANTE  DE PRESENTACIÓN\nModelo 303";
        assert_eq!(title(t).as_deref(), Some("Justificante de presentación"));
        let t = "AEAT\nRequerimiento de información: art. 93/LGT\n";
        assert_eq!(
            title(t).as_deref(),
            Some("Requerimiento de información art 93 LGT")
        );
        assert_eq!(title("Agencia Tributaria\nEMPRESA DEMO SL\n"), None);
        assert_eq!(title("Justificante 3030000000001234"), None);
        // En mayúsculas y sin tildes, como salen muchos textos de la AEAT.
        for (t, want) in [
            (
                "AEAT\nDECLARACION INFORMATIVA ANUAL\n",
                "Declaracion informativa anual",
            ),
            (
                "AEAT\nCERTIFICACION DE RETENCIONES\n",
                "Certificacion de retenciones",
            ),
            (
                "AEAT\nAUTOLIQUIDACION COMPLEMENTARIA\n",
                "Autoliquidacion complementaria",
            ),
        ] {
            assert_eq!(title(t).as_deref(), Some(want));
        }
        // Un título en mayúsculas partido en dos líneas se junta; si la segunda tiene cifras o
        // minúsculas, no.
        let t = "JUSTIFICANTE DE PRESENTACIÓN SIN\nCUOTA A INGRESAR\nPresentación telemática";
        assert_eq!(
            title(t).as_deref(),
            Some("Justificante de presentación sin cuota a ingresar")
        );
        let t = "JUSTIFICANTE DE PRESENTACIÓN\nMODELO 600\n";
        assert_eq!(title(t).as_deref(), Some("Justificante de presentación"));
        // Un escrito a la AEAT, sin título propio.
        let t = "A LA AGENCIA ESTATAL DE ADMINISTRACIÓN TRIBUTARIA\nDon Fulano…\nEXPONE\n";
        assert_eq!(title(t).as_deref(), Some("Escrito a la AEAT"));
    }

    #[test]
    fn errores_del_disco_en_palabras_llanas() {
        use std::io::{Error, ErrorKind};
        for (kind, want) in [
            (ErrorKind::PermissionDenied, "permiso"),
            (ErrorKind::ReadOnlyFilesystem, "sólo lectura"),
            (ErrorKind::StorageFull, "lleno"),
            (ErrorKind::NotFound, "ya no está"),
            (ErrorKind::Other, "no deja escribir"),
        ] {
            let m = io_error(&Error::new(kind, "Permission denied (os error 13)")).to_string();
            assert!(m.contains(want) && m.contains(SUBFOLDER), "{m}");
            assert!(!m.contains("os error"), "{m}");
        }
    }

    #[test]
    fn nombres_seguros_para_el_mac() {
        assert_eq!(sanitize("../a:b/c\\d*?\"<>|"), "a b c d");
        assert_eq!(sanitize("  .Acuerdo.  "), "Acuerdo");
        let long = "Notificación de la propuesta de liquidación provisional del impuesto sobre \
                    el valor añadido del ejercicio";
        let s = sanitize(long);
        assert!(s.chars().count() <= MAX_TITLE, "{s}");
        assert!(!s.ends_with(' '));
        assert!(long.starts_with(&s));
    }
}
