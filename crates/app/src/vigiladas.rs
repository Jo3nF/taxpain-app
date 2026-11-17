//! Carpetas vigiladas: cada empresa vigila las suyas (p. ej. la de Drive donde otra herramienta
//! deja las facturas del correo), con todas sus subcarpetas.
//!
//! Cada pasada (la app la hace cada 2 minutos) mira sólo los ficheros nuevos o cambiados (por
//! tamaño y fecha de modificación) y los clasifica como «Importar la carpeta":
//! - una factura recibida que no está en un libro ni es de un periodo ya declarado entra sola en
//!   Recibidas, pendiente de revisar (sin duplicados: el mismo fichero se reconoce);
//! - lo demás que se podría importar (extractos, modelos, libros, diarios, documentos…) no se
//!   importa solo: queda como aviso en Importar, porque cambia la contabilidad y el usuario lo
//!   tiene que ver antes;
//! - lo que no se reconoce sólo se marca como visto.

use std::collections::BTreeMap;
use std::ops::Deref;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use taxpain_vault::Vault;
use time::OffsetDateTime;

use crate::carpeta::{self, Found};
use crate::error::{AppError, Result};
use crate::received::{self, Source};

/// Clave antigua: una sola carpeta (sólo facturas, sin subcarpetas).
pub const OLD_KEY: &str = "received_watched_folder";
const KEY: &str = "carpetas_vigiladas";
const SEEN_KEY: &str = "carpetas_vigiladas_vistos";
const PENDING_KEY: &str = "carpetas_vigiladas_pendientes";
/// Ficheros que no se pudieron leer (con su tamaño, su fecha y el error): no se vuelven a leer
/// mientras no cambien.
const FAILED_KEY: &str = "carpetas_vigiladas_fallidos";
/// El fichero que se está leyendo (ver [`InProgress`]).
const IN_PROGRESS_KEY: &str = "carpetas_vigiladas_en_curso";
/// Cada cuánto se guarda lo visto durante una pasada larga (si la app se cierra a mitad de una
/// carpeta grande, no se vuelve a empezar de cero), y al final.
const SAVE_EVERY: Duration = Duration::from_secs(30);
/// Los ficheros modificados hace menos se miran en la pasada siguiente.
const MIN_AGE: Duration = Duration::from_secs(60);
/// Tamaño máximo de lo que se lee: lo que Recibidas admite (y por debajo del de «Importar la
/// carpeta»), para no leer y clasificar ficheros que luego se rechazan.
const MAX_SIZE: u64 = received::MAX_FILE_SIZE as u64;

/// Documento nuevo de una carpeta vigilada que no se importa solo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    /// Carpeta vigilada (ruta absoluta).
    pub folder: String,
    /// Ruta relativa a la carpeta.
    pub path: String,
    /// Qué es («Extracto bancario», «Modelo 303»…).
    pub kind: String,
    pub year: Option<i32>,
    /// Cuándo se vio (AAAA-MM-DD).
    pub seen: String,
    /// Si no se ha podido leer, por qué (el aviso es «TaxPain no pudo leer este fichero»).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Resultado de una pasada.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Pass {
    /// Facturas recibidas nuevas en Recibidas.
    pub invoices: usize,
    /// Avisos nuevos en Importar.
    pub pending: usize,
    /// Ficheros nuevos que no se han podido leer (quedan como aviso en Importar).
    pub unreadable: usize,
    /// Carpetas o ficheros que no se pudieron mirar, con su ruta. Para el registro, mejor
    /// [`Pass::error_count`]: las rutas y los nombres de fichero no deben acabar en él.
    pub errors: Vec<String>,
}

impl Pass {
    /// Cuántos errores hubo, sin el detalle (que lleva rutas y nombres de fichero).
    pub fn error_count(&self) -> usize {
        self.errors.len()
    }
}

/// Carpetas vigiladas de la empresa (con la antigua, si es la única que hay).
pub fn folders(v: &Vault) -> Result<Vec<String>> {
    if let Some(list) = v.setting::<Vec<String>>(KEY)? {
        return Ok(list);
    }
    Ok(v.setting::<Option<String>>(OLD_KEY)?
        .flatten()
        .into_iter()
        .collect())
}

/// Cambia la lista. Sólo carpetas que existen, sin repetir; los avisos y lo visto de las carpetas
/// que se quitan se olvidan.
pub fn set_folders(v: &Vault, list: &[String]) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for f in list {
        let f = f.trim().trim_end_matches('/').to_owned();
        if f.is_empty() || out.contains(&f) {
            continue;
        }
        if !Path::new(&f).is_dir() {
            return Err(AppError::Other(format!("{f} no es una carpeta")));
        }
        out.push(f);
    }
    v.set_setting(KEY, &out)?;
    v.set_setting::<Option<String>>(OLD_KEY, &None)?;
    let inside = |p: &str| {
        out.iter()
            .any(|f| p == f || p.starts_with(&format!("{f}/")))
    };
    let mut seen = seen(v)?;
    seen.retain(|p, _| inside(p));
    v.set_setting(SEEN_KEY, &seen)?;
    let mut failed = failed(v)?;
    failed.retain(|p, _| inside(p));
    v.set_setting(FAILED_KEY, &failed)?;
    let mut pend = pending(v)?;
    pend.retain(|p| out.contains(&p.folder));
    v.set_setting(PENDING_KEY, &pend)?;
    Ok(out)
}

/// Avisos pendientes en Importar.
pub fn pending(v: &Vault) -> Result<Vec<Pending>> {
    Ok(v.setting(PENDING_KEY)?.unwrap_or_default())
}

/// Quita los avisos de `folder` (o todos): el usuario ya los ha visto en Importar.
pub fn dismiss(v: &Vault, folder: Option<&str>) -> Result<()> {
    let mut pend = pending(v)?;
    match folder {
        Some(f) => pend.retain(|p| p.folder != f.trim_end_matches('/')),
        None => pend.clear(),
    }
    v.set_setting(PENDING_KEY, &pend)?;
    Ok(())
}

/// Quita los avisos de los ficheros que están dentro de `root`: se acaba de mirar esa carpeta
/// en Importar, donde se ven y se importan.
pub fn dismiss_under(v: &Vault, root: &str) -> Result<()> {
    let root = root.trim_end_matches('/');
    let mut pend = pending(v)?;
    let before = pend.len();
    pend.retain(|p| {
        let full = format!("{}/{}", p.folder, p.path);
        !(full == root || full.starts_with(&format!("{root}/")))
    });
    if pend.len() != before {
        v.set_setting(PENDING_KEY, &pend)?;
    }
    Ok(())
}

fn seen(v: &Vault) -> Result<BTreeMap<String, (u64, i64)>> {
    Ok(v.setting(SEEN_KEY)?.unwrap_or_default())
}

fn failed(v: &Vault) -> Result<BTreeMap<String, Failed>> {
    Ok(v.setting(FAILED_KEY)?.unwrap_or_default())
}

/// Un fichero que no se pudo leer: no se vuelve a leer mientras no cambie (tamaño y fecha).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Failed {
    size: u64,
    mtime: i64,
    error: String,
    /// Veces que la app se cerró mientras lo leía. Con una se intenta otra vez (pudo ser que el
    /// usuario cerrara la app); con dos queda en cuarentena. Con 0, el lector dio un error.
    #[serde(default)]
    crashes: u32,
}

impl Failed {
    fn stamp(&self) -> (u64, i64) {
        (self.size, self.mtime)
    }

    /// ¿Se deja sin leer mientras no cambie?
    fn settled(&self) -> bool {
        self.crashes != 1
    }
}

/// El fichero que se está leyendo, apuntado antes de leerlo: si la app muere al leerlo
/// (memoria, cuelgue), la pasada siguiente lo sabe.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct InProgress {
    folder: String,
    path: String,
    size: u64,
    mtime: i64,
}

/// Una pasada por todas las carpetas vigiladas de la empresa.
pub fn scan(v: &Vault, now: OffsetDateTime) -> Result<Pass> {
    scan_with(|| Ok(v), now)
}

/// Como [`scan`], cogiendo el almacén con `vault` sólo para lo que lo necesita (en la app, el
/// cerrojo de la empresa abierta): leer y clasificar cada fichero se hace sin él.
pub fn scan_with<G: Deref<Target = Vault>>(
    vault: impl Fn() -> Result<G>,
    now: OffsetDateTime,
) -> Result<Pass> {
    Scanner {
        vault: &vault,
        now,
        max_files: carpeta::MAX_WALKED_FILES,
        min_age: MIN_AGE,
        save_every: SAVE_EVERY,
        read: &|root: &Path, path: &Path, bytes: &[u8]| {
            carpeta::classify_one(root, path, bytes, &vault)
        },
    }
    .run()
}

/// Lector de un fichero: qué es (como lo vería «Importar la carpeta»).
type ReadFn<'a> = dyn Fn(&Path, &Path, &[u8]) -> Result<Found> + 'a;

/// Una pasada, con lo que se puede cambiar en las pruebas.
struct Scanner<'a, V> {
    vault: &'a V,
    now: OffsetDateTime,
    max_files: usize,
    /// Los ficheros cambiados hace menos se dejan para la pasada siguiente (Drive puede estar
    /// escribiéndolos).
    min_age: Duration,
    /// Cada cuánto se guarda lo visto (y al final).
    save_every: Duration,
    read: &'a ReadFn<'a>,
}

/// Lo nuevo de una pasada, que se añade a lo guardado al guardar.
#[derive(Default)]
struct News {
    seen: BTreeMap<String, (u64, i64)>,
    failed: BTreeMap<String, Failed>,
    pending: Vec<Pending>,
    /// Avisos que ya había o que ya se han dado en esta pasada (carpeta, ruta): no se cuentan
    /// otra vez.
    known: std::collections::BTreeSet<(String, String)>,
}

impl News {
    fn is_empty(&self) -> bool {
        self.seen.is_empty() && self.failed.is_empty() && self.pending.is_empty()
    }

    /// Añade un aviso si no lo había ya. ¿Es nuevo?
    fn add(&mut self, item: Pending) -> bool {
        if !self.known.insert((item.folder.clone(), item.path.clone())) {
            return false;
        }
        self.pending.push(item);
        true
    }
}

/// Qué ha pasado con un fichero.
enum Outcome {
    /// Leído (y, si era una factura, en Recibidas).
    Done,
    /// No se ha podido leer ahora (se está descargando, almacén ocupado…): en la pasada siguiente.
    Later,
    /// No se puede leer: se avisa y no se vuelve a leer mientras no cambie.
    Failed(String),
}

impl<G: Deref<Target = Vault>, V: Fn() -> Result<G>> Scanner<'_, V> {
    fn run(&self) -> Result<Pass> {
        let list = folders(&*(self.vault)()?)?;
        let mut pass = Pass::default();
        if list.is_empty() {
            return Ok(pass);
        }
        let mut news = News::default();
        match self.run_folders(&list, &mut news, &mut pass) {
            Ok(()) => Ok(pass),
            Err(e) => {
                // Lo leído hasta aquí se guarda si se puede, y ya no hay nada a medio leer.
                let _ = self.save(&mut news, true);
                Err(e)
            }
        }
    }

    fn run_folders(&self, list: &[String], news: &mut News, pass: &mut Pass) -> Result<()> {
        self.recover(list, news, pass)?;
        let (mut seen, mut failed) = {
            let v = (self.vault)()?;
            news.known = pending(&v)?
                .into_iter()
                .map(|p| (p.folder, p.path))
                .collect();
            (seen(&v)?, failed(&v)?)
        };
        let today = self.now.date().to_string();
        let mut last_save = Instant::now();
        for folder in list {
            // El usuario puede haber quitado la carpeta mientras dura la pasada.
            if !folders(&*(self.vault)()?)?.contains(folder) {
                continue;
            }
            let root = Path::new(folder);
            if !root.is_dir() {
                pass.errors
                    .push(format!("{folder}: no se encuentra la carpeta"));
                continue;
            }
            let walked = carpeta::files_under_max(root, self.max_files);
            if walked.over_limit > 0 {
                pass.errors.push(format!(
                    "{folder}: carpeta incompleta: {} ficheros fuera del límite de {}",
                    walked.over_limit, self.max_files
                ));
            }
            if walked.too_deep > 0 {
                pass.errors.push(format!(
                    "{folder}: carpeta incompleta: {} subcarpetas demasiado profundas",
                    walked.too_deep
                ));
            }
            for path in walked.files {
                let key = path.to_string_lossy().into_owned();
                // Sin seguir enlaces, como al recorrer la carpeta.
                let Ok(meta) = std::fs::symlink_metadata(&path) else {
                    continue;
                };
                if !meta.file_type().is_file() {
                    continue;
                }
                let stamp = (meta.len(), mtime_of(&meta));
                if seen.get(&key) == Some(&stamp)
                    || failed
                        .get(&key)
                        .is_some_and(|f| f.stamp() == stamp && f.settled())
                {
                    continue;
                }
                // Un fichero que se acaba de escribir (Drive lo puede estar bajando) se mira en
                // la pasada siguiente: si no, entraría a medias y otra vez al terminar.
                if meta
                    .modified()
                    .ok()
                    .and_then(|m| std::time::SystemTime::now().duration_since(m).ok())
                    .is_some_and(|age| age < self.min_age)
                {
                    continue;
                }
                let rel = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned();
                let outcome = if meta.len() > MAX_SIZE {
                    Outcome::Failed(format!(
                        "ocupa {} MB y el máximo es {} MB",
                        meta.len().div_ceil(1024 * 1024),
                        MAX_SIZE / (1024 * 1024)
                    ))
                } else {
                    self.set_in_progress(Some(&InProgress {
                        folder: folder.clone(),
                        path: key.clone(),
                        size: stamp.0,
                        mtime: stamp.1,
                    }))?;
                    self.one(root, folder, &path, &rel, &today, news, pass)
                };
                match outcome {
                    Outcome::Done => {
                        failed.remove(&key);
                        news.failed.remove(&key);
                        seen.insert(key.clone(), stamp);
                        news.seen.insert(key, stamp);
                    }
                    Outcome::Later => {}
                    Outcome::Failed(error) => {
                        pass.errors.push(format!("{rel}: {error}"));
                        let f = Failed {
                            size: stamp.0,
                            mtime: stamp.1,
                            error: error.clone(),
                            crashes: 0,
                        };
                        failed.insert(key.clone(), f.clone());
                        news.failed.insert(key, f);
                        news.add(unreadable(folder, &rel, &error, &today));
                        pass.unreadable += 1;
                    }
                }
                if last_save.elapsed() >= self.save_every {
                    self.save(news, false)?;
                    last_save = Instant::now();
                }
            }
        }
        self.save(news, true)
    }

    /// Lee y clasifica un fichero (y, si es una factura recibida, la da de alta). Un pánico del
    /// lector es un error de ese fichero, no de la pasada.
    #[allow(clippy::too_many_arguments)]
    fn one(
        &self,
        root: &Path,
        folder: &str,
        path: &Path,
        rel: &str,
        today: &str,
        news: &mut News,
        pass: &mut Pass,
    ) -> Outcome {
        let bytes = match read_regular(path, MAX_SIZE) {
            Ok(b) => b,
            Err(ReadError::TooBig) => {
                return Outcome::Failed(format!(
                    "pasa de {} MB, el máximo",
                    MAX_SIZE / (1024 * 1024)
                ));
            }
            // Se está descargando, ha cambiado mientras se abría…: en la pasada siguiente.
            Err(ReadError::Io) => return Outcome::Later,
        };
        let found = match catch_unwind(AssertUnwindSafe(|| (self.read)(root, path, &bytes))) {
            Ok(Ok(found)) => found,
            Ok(Err(e)) if transient(&e) => return Outcome::Later,
            Ok(Err(e)) => return Outcome::Failed(e.to_string()),
            Err(p) => return Outcome::Failed(panic_message(p.as_ref())),
        };
        if auto_import(&found) {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let ingest = catch_unwind(AssertUnwindSafe(|| {
                received::ingest_file_with(self.vault, &name, &bytes, Source::Carpeta, self.now)
            }));
            match ingest {
                Ok(Ok(o)) if !o.duplicate => pass.invoices += 1,
                Ok(Ok(_)) => {}
                Ok(Err(e)) if transient(&e) => return Outcome::Later,
                Ok(Err(e)) => return Outcome::Failed(e.to_string()),
                Err(p) => return Outcome::Failed(panic_message(p.as_ref())),
            }
        } else if found.importable() || found.selectable() {
            let item = Pending {
                folder: folder.to_owned(),
                path: rel.to_owned(),
                kind: label(&found),
                year: year(&found),
                seen: today.to_owned(),
                error: None,
            };
            if news.add(item) {
                pass.pending += 1;
            }
        }
        Outcome::Done
    }

    /// Si la pasada anterior no terminó de leer un fichero (la app murió leyéndolo), se apunta:
    /// la primera vez se reintenta; la segunda queda en cuarentena, con aviso.
    fn recover(&self, list: &[String], news: &mut News, pass: &mut Pass) -> Result<()> {
        let v = (self.vault)()?;
        let Some(ip) = v.setting::<Option<InProgress>>(IN_PROGRESS_KEY)?.flatten() else {
            return Ok(());
        };
        let before = failed(&v)?
            .get(&ip.path)
            .filter(|f| f.stamp() == (ip.size, ip.mtime))
            .map_or(0, |f| f.crashes);
        let crashes = before.saturating_add(1);
        let error = "la app se cerró mientras lo leía (puede estar dañado o ser demasiado \
                     grande)"
            .to_owned();
        news.failed.insert(
            ip.path.clone(),
            Failed {
                size: ip.size,
                mtime: ip.mtime,
                error: error.clone(),
                crashes,
            },
        );
        if crashes >= 2 && list.contains(&ip.folder) {
            let rel = Path::new(&ip.path)
                .strip_prefix(&ip.folder)
                .map(|r| r.to_string_lossy().into_owned())
                .unwrap_or_else(|_| ip.path.clone());
            pass.errors.push(format!("{rel}: {error}"));
            pass.unreadable += 1;
            news.pending.push(unreadable(
                &ip.folder,
                &rel,
                &error,
                &self.now.date().to_string(),
            ));
        }
        drop(v);
        self.save(news, true)
    }

    fn set_in_progress(&self, ip: Option<&InProgress>) -> Result<()> {
        (self.vault)()?.set_setting(IN_PROGRESS_KEY, &ip)?;
        Ok(())
    }

    /// Añade lo nuevo a lo guardado, releyéndolo con el almacén cogido (lo que el usuario haya
    /// cambiado mientras tanto, como quitar un aviso o una carpeta, se respeta), y apunta que no
    /// hay ningún fichero a medio leer (se guarda entre fichero y fichero). Sin nada nuevo, sólo
    /// lo hace con `force`.
    fn save(&self, news: &mut News, force: bool) -> Result<()> {
        if news.is_empty() && !force {
            return Ok(());
        }
        let v = (self.vault)()?;
        if !news.is_empty() {
            let list = folders(&v)?;
            let inside = |p: &str| {
                list.iter()
                    .any(|f| p == f || p.starts_with(&format!("{f}/")))
            };
            let mut seen = seen(&v)?;
            let mut failed = failed(&v)?;
            let mut pend = pending(&v)?;
            for (k, st) in std::mem::take(&mut news.seen) {
                if inside(&k) {
                    // Se ha podido leer: fuera el aviso de que no se podía.
                    if failed.remove(&k).is_some() {
                        pend.retain(|p| {
                            p.error.is_none() || format!("{}/{}", p.folder, p.path) != k
                        });
                    }
                    seen.insert(k, st);
                }
            }
            for (k, f) in std::mem::take(&mut news.failed) {
                if inside(&k) {
                    failed.insert(k, f);
                }
            }
            for item in std::mem::take(&mut news.pending) {
                if list.contains(&item.folder)
                    && !pend
                        .iter()
                        .any(|p| p.folder == item.folder && p.path == item.path)
                {
                    pend.push(item);
                }
            }
            v.set_setting(SEEN_KEY, &seen)?;
            v.set_setting(FAILED_KEY, &failed)?;
            v.set_setting(PENDING_KEY, &pend)?;
        }
        v.set_setting::<Option<InProgress>>(IN_PROGRESS_KEY, &None)?;
        Ok(())
    }
}

/// Aviso en Importar de un fichero que no se ha podido leer.
fn unreadable(folder: &str, rel: &str, error: &str, today: &str) -> Pending {
    Pending {
        folder: folder.to_owned(),
        path: rel.to_owned(),
        kind: format!("TaxPain no pudo leer este fichero: {error}"),
        year: None,
        seen: today.to_owned(),
        error: Some(error.to_owned()),
    }
}

/// Errores que no son del fichero (el almacén cerrado u ocupado): se reintenta.
fn transient(e: &AppError) -> bool {
    matches!(e, AppError::Vault(_))
}

fn panic_message(p: &(dyn std::any::Any + Send)) -> String {
    let detail = p
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| p.downcast_ref::<String>().cloned());
    match detail {
        Some(d) => format!("error interno al leerlo ({d})"),
        None => "error interno al leerlo".to_owned(),
    }
}

fn mtime_of(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok())
        .unwrap_or(0)
}

enum ReadError {
    /// Pasa del tope.
    TooBig,
    /// No se puede abrir o leer, no es un fichero normal, o ha cambiado entre mirarlo y abrirlo.
    Io,
}

/// Lee un fichero normal sin seguir enlaces y sin pasar de `max` bytes: el tipo y el tamaño se
/// miran en el descriptor abierto (no en la ruta, que se puede cambiar por un enlace entre
/// recorrer la carpeta y leerla).
fn read_regular(path: &Path, max: u64) -> std::result::Result<Vec<u8>, ReadError> {
    use std::io::Read;

    let before = std::fs::symlink_metadata(path).map_err(|_| ReadError::Io)?;
    if !before.file_type().is_file() {
        return Err(ReadError::Io);
    }
    let f = std::fs::File::open(path).map_err(|_| ReadError::Io)?;
    let meta = f.metadata().map_err(|_| ReadError::Io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // Si entre mirarlo y abrirlo lo han cambiado por un enlace, el abierto es otro.
        if meta.dev() != before.dev() || meta.ino() != before.ino() {
            return Err(ReadError::Io);
        }
    }
    if !meta.is_file() {
        return Err(ReadError::Io);
    }
    if meta.len() > max {
        return Err(ReadError::TooBig);
    }
    let mut out = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(0));
    f.take(max.saturating_add(1))
        .read_to_end(&mut out)
        .map_err(|_| ReadError::Io)?;
    if out.len() as u64 > max {
        return Err(ReadError::TooBig);
    }
    Ok(out)
}

/// ¿Entra sola en Recibidas? Sólo las facturas recibidas sueltas: la que ya está en un libro se
/// adjunta a su línea al importar la carpeta (si entrara aparte se deduciría dos veces) y la de
/// un periodo ya declarado no se propone.
fn auto_import(f: &Found) -> bool {
    match f {
        Found::FacturaRecibida {
            in_book,
            historical,
            ..
        } => !in_book && !historical,
        Found::Ia {
            tipo, importable, ..
        } => tipo == "factura_recibida" && *importable,
        _ => false,
    }
}

fn year(f: &Found) -> Option<i32> {
    match f {
        Found::Diario { last, .. }
        | Found::Libro { last, .. }
        | Found::Banco { last, .. }
        | Found::Cripto { last, .. }
        | Found::Valores { last, .. } => last.map(|d| d.year()),
        Found::FacturaRecibida { date, .. } | Found::FacturaEmitida { date, .. } => {
            date.map(|d| d.year())
        }
        Found::Modelo { year, .. } => Some(*year),
        Found::Ia { year, .. } => *year,
        Found::Documento { .. } | Found::Otro { .. } => None,
    }
}

fn label(f: &Found) -> String {
    match f {
        Found::Ia { titulo, .. } => titulo.clone(),
        Found::Documento { title, .. } => title.clone(),
        Found::Diario { .. } => "Diario contable".into(),
        Found::Libro { .. } => "Libro registro de facturas".into(),
        Found::Banco { .. } => "Extracto bancario".into(),
        Found::FacturaRecibida { .. } => "Factura recibida".into(),
        Found::FacturaEmitida { .. } => "Factura emitida".into(),
        Found::Modelo { .. } => "Modelo de la AEAT".into(),
        Found::Cripto { .. } => "Criptomonedas".into(),
        Found::Valores { .. } => "Inversiones en valores".into(),
        Found::Otro { .. } => "Documento".into(),
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::Arc;

    use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider};
    use time::macros::datetime;

    use super::*;

    const AHORA: OffsetDateTime = datetime!(2026-11-12 10:00 UTC);

    fn vault() -> Vault {
        Vault::open_in_memory(
            Arc::new(MemoryKeyProvider::default()),
            Arc::new(AlwaysConfirm),
        )
        .unwrap()
    }

    /// Escribe un fichero con fecha de hace una hora (los recién escritos se dejan para la
    /// pasada siguiente).
    fn escribir(root: &Path, rel: &str, bytes: &[u8]) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, bytes).unwrap();
        envejecer(&p);
    }

    fn envejecer(p: &Path) {
        let f = std::fs::File::options().write(true).open(p).unwrap();
        f.set_modified(std::time::SystemTime::now() - Duration::from_secs(3600))
            .unwrap();
    }

    fn banco() -> Found {
        Found::Banco {
            resumen: "Banco Ejemplo".into(),
            last: None,
            cuadra: None,
        }
    }

    /// Una pasada con el lector `read` (en vez del de «Importar la carpeta»).
    fn pasada(v: &Vault, read: &ReadFn<'_>) -> Pass {
        pasada_con(v, read, 50_000)
    }

    fn pasada_con(v: &Vault, read: &ReadFn<'_>, max_files: usize) -> Pass {
        Scanner {
            vault: &|| Ok(v),
            now: AHORA,
            max_files,
            min_age: MIN_AGE,
            save_every: SAVE_EVERY,
            read,
        }
        .run()
        .unwrap()
    }

    fn carpeta(v: &Vault) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        set_folders(v, &[d.path().to_string_lossy().into_owned()]).unwrap();
        d
    }

    fn avisos(v: &Vault) -> Vec<(String, Option<String>)> {
        pending(v)
            .unwrap()
            .into_iter()
            .map(|p| (p.path, p.error))
            .collect()
    }

    #[test]
    fn un_panico_del_lector_es_un_error_de_ese_fichero_y_no_se_reintenta() {
        let v = vault();
        let d = carpeta(&v);
        escribir(d.path(), "malo.xlsx", b"malo");
        escribir(d.path(), "bueno.n43", b"bueno");
        let lecturas = Cell::new(0);
        let read = |_: &Path, p: &Path, _: &[u8]| -> Result<Found> {
            lecturas.set(lecturas.get() + 1);
            if p.ends_with("malo.xlsx") {
                panic!("lector roto");
            }
            Ok(banco())
        };
        let pass = pasada(&v, &read);
        assert_eq!(lecturas.get(), 2);
        assert_eq!((pass.pending, pass.unreadable), (1, 1), "{pass:?}");
        assert_eq!(pass.error_count(), 1);
        assert!(pass.errors[0].contains("malo.xlsx"), "{pass:?}");
        let a = avisos(&v);
        assert!(a.contains(&("bueno.n43".into(), None)), "{a:?}");
        let malo = pending(&v)
            .unwrap()
            .into_iter()
            .find(|p| p.path == "malo.xlsx")
            .unwrap();
        assert!(
            malo.kind.starts_with("TaxPain no pudo leer este fichero"),
            "{malo:?}"
        );
        assert!(malo.error.unwrap().contains("lector roto"));

        // La pasada siguiente no vuelve a leer ninguno de los dos.
        let pass = pasada(&v, &read);
        assert_eq!(lecturas.get(), 2);
        assert_eq!(pass, Pass::default());

        // Si cambia, sí; y si ahora se lee, se quita el aviso de que no se podía.
        escribir(d.path(), "malo.xlsx", b"ya no tan malo");
        let read2 = |_: &Path, _: &Path, _: &[u8]| -> Result<Found> {
            lecturas.set(lecturas.get() + 1);
            Ok(Found::Otro {
                reason: "nada".into(),
            })
        };
        let pass = pasada(&v, &read2);
        assert_eq!(lecturas.get(), 3);
        assert!(pass.errors.is_empty());
        assert_eq!(avisos(&v), [("bueno.n43".to_owned(), None)]);
    }

    #[test]
    fn un_error_del_lector_se_guarda_y_sólo_se_reintenta_si_el_fichero_cambia() {
        let v = vault();
        let d = carpeta(&v);
        escribir(d.path(), "raro.pdf", b"raro");
        let lecturas = Cell::new(0);
        let read = |_: &Path, _: &Path, _: &[u8]| -> Result<Found> {
            lecturas.set(lecturas.get() + 1);
            Err(AppError::Other("no se entiende".into()))
        };
        let pass = pasada(&v, &read);
        assert_eq!((lecturas.get(), pass.unreadable), (1, 1));
        let pass = pasada(&v, &read);
        assert_eq!((lecturas.get(), pass.unreadable), (1, 0));
        // Quitar el aviso no hace que vuelva.
        dismiss(&v, None).unwrap();
        pasada(&v, &read);
        assert!(avisos(&v).is_empty());
        escribir(d.path(), "raro.pdf", b"raro, otra version");
        let pass = pasada(&v, &read);
        assert_eq!((lecturas.get(), pass.unreadable), (2, 1));
        // Un error del almacén no es del fichero: se reintenta en la pasada siguiente.
        escribir(d.path(), "otro.pdf", b"otro");
        let ocupado = |_: &Path, _: &Path, _: &[u8]| -> Result<Found> {
            lecturas.set(lecturas.get() + 1);
            Err(AppError::Vault(taxpain_vault::VaultError::InvalidInput(
                "ocupado".into(),
            )))
        };
        let pass = pasada(&v, &ocupado);
        assert_eq!((lecturas.get(), pass.unreadable), (3, 0));
        pasada(&v, &ocupado);
        assert_eq!(lecturas.get(), 4);
    }

    /// Lo guardado en este momento (lo que quedaría si la app muriera ahora).
    fn foto(v: &Vault) -> Vec<(&'static str, Option<serde_json::Value>)> {
        [SEEN_KEY, FAILED_KEY, PENDING_KEY, IN_PROGRESS_KEY]
            .into_iter()
            .map(|k| (k, v.setting::<serde_json::Value>(k).unwrap()))
            .collect()
    }

    fn volver_a(v: &Vault, foto: &[(&'static str, Option<serde_json::Value>)]) {
        for (k, val) in foto {
            v.set_setting(k, &val.clone().unwrap_or(serde_json::Value::Null))
                .unwrap();
        }
    }

    #[test]
    fn si_la_app_muere_leyendo_un_fichero_se_reintenta_una_vez_y_luego_queda_en_cuarentena() {
        let v = vault();
        let d = carpeta(&v);
        escribir(d.path(), "bomba.xlsx", b"bomba");
        escribir(d.path(), "banco.n43", b"banco");
        let lecturas = Cell::new(0);
        let al_morir = std::cell::RefCell::new(None);
        // Un lector con el que la app «muere» al leer la bomba: lo que queda guardado es lo que
        // había en ese momento.
        let muere = |_: &Path, p: &Path, _: &[u8]| -> Result<Found> {
            if p.ends_with("bomba.xlsx") {
                lecturas.set(lecturas.get() + 1);
                *al_morir.borrow_mut() = Some(foto(&v));
            }
            Ok(banco())
        };
        let morir = |pass: Pass| {
            volver_a(&v, al_morir.borrow().as_ref().unwrap());
            pass
        };
        morir(pasada(&v, &muere));
        assert_eq!(lecturas.get(), 1);
        let marca = v
            .setting::<Option<InProgress>>(IN_PROGRESS_KEY)
            .unwrap()
            .flatten();
        assert!(marca.unwrap().path.ends_with("bomba.xlsx"));

        // La primera vez puede que el usuario cerrara la app: se vuelve a intentar…
        morir(pasada(&v, &muere));
        assert_eq!(lecturas.get(), 2);
        // …y si vuelve a morir con el mismo fichero, queda en cuarentena, con aviso.
        let pass = pasada(&v, &muere);
        assert_eq!(lecturas.get(), 2, "en cuarentena no se lee");
        assert_eq!(pass.unreadable, 1, "{pass:?}");
        let bomba = pending(&v)
            .unwrap()
            .into_iter()
            .find(|p| p.path == "bomba.xlsx")
            .unwrap();
        assert!(
            bomba
                .kind
                .starts_with("TaxPain no pudo leer este fichero: la app se cerró"),
            "{bomba:?}"
        );
        assert_eq!(
            v.setting::<Option<InProgress>>(IN_PROGRESS_KEY).unwrap(),
            Some(None)
        );
        // Ni en las pasadas siguientes, hasta que cambie.
        pasada(&v, &muere);
        assert_eq!(lecturas.get(), 2);
        escribir(d.path(), "bomba.xlsx", b"otra cosa");
        pasada(&v, &muere);
        assert_eq!(lecturas.get(), 3);
    }

    #[test]
    fn si_la_app_se_cierra_una_vez_leyendo_un_fichero_luego_se_lee_normal() {
        let v = vault();
        let d = carpeta(&v);
        escribir(d.path(), "banco.n43", b"banco");
        let p = d.path().join("banco.n43");
        let meta = std::fs::symlink_metadata(&p).unwrap();
        v.set_setting(
            IN_PROGRESS_KEY,
            &Some(InProgress {
                folder: d.path().to_string_lossy().into_owned(),
                path: p.to_string_lossy().into_owned(),
                size: meta.len(),
                mtime: mtime_of(&meta),
            }),
        )
        .unwrap();
        let read = |_: &Path, _: &Path, _: &[u8]| -> Result<Found> { Ok(banco()) };
        let pass = pasada(&v, &read);
        assert_eq!((pass.pending, pass.unreadable), (1, 0), "{pass:?}");
        assert!(failed(&v).unwrap().is_empty());
    }

    #[test]
    fn lo_que_pasa_del_tope_de_recibidas_se_avisa_sin_leerlo() {
        let v = vault();
        let d = carpeta(&v);
        let p = d.path().join("escaneo.pdf");
        let f = std::fs::File::create(&p).unwrap();
        f.set_len(MAX_SIZE + 1).unwrap();
        drop(f);
        envejecer(&p);
        let read = |_: &Path, _: &Path, _: &[u8]| -> Result<Found> {
            panic!("no se tiene que leer");
        };
        let pass = pasada(&v, &read);
        assert_eq!(pass.unreadable, 1, "{pass:?}");
        let a = pending(&v).unwrap();
        assert!(a[0].kind.contains("el máximo es 25 MB"), "{a:?}");
        assert_eq!(pasada(&v, &read), Pass::default());
    }

    #[test]
    fn una_carpeta_con_mas_ficheros_que_el_tope_se_avisa() {
        let v = vault();
        let d = carpeta(&v);
        for i in 0..5 {
            escribir(d.path(), &format!("f{i}.n43"), b"banco");
        }
        let read = |_: &Path, _: &Path, _: &[u8]| -> Result<Found> { Ok(banco()) };
        let pass = pasada_con(&v, &read, 3);
        assert_eq!(pass.pending, 3);
        assert_eq!(pass.error_count(), 1);
        assert!(
            pass.errors[0].ends_with("carpeta incompleta: 2 ficheros fuera del límite de 3"),
            "{pass:?}"
        );
    }

    #[test]
    fn un_fichero_recien_escrito_se_deja_para_la_pasada_siguiente() {
        let v = vault();
        let d = carpeta(&v);
        let p = d.path().join("a-medias.n43");
        std::fs::write(&p, b"banco a medi").unwrap();
        let read = |_: &Path, _: &Path, _: &[u8]| -> Result<Found> { Ok(banco()) };
        assert_eq!(pasada(&v, &read).pending, 0);
        assert!(seen(&v).unwrap().is_empty());
        std::fs::write(&p, b"banco a medias, ya entero").unwrap();
        envejecer(&p);
        assert_eq!(pasada(&v, &read).pending, 1);
    }

    #[test]
    fn la_pasada_respeta_lo_que_el_usuario_cambia_mientras_dura() {
        let v = vault();
        let d = carpeta(&v);
        let otra = tempfile::tempdir().unwrap();
        let (a, b) = (
            d.path().to_string_lossy().into_owned(),
            otra.path().to_string_lossy().into_owned(),
        );
        set_folders(&v, &[a.clone(), b.clone()]).unwrap();
        escribir(d.path(), "viejo.n43", b"viejo");
        let read = |_: &Path, _: &Path, _: &[u8]| -> Result<Found> { Ok(banco()) };
        pasada(&v, &read);
        assert_eq!(avisos(&v).len(), 1);

        // A mitad de la pasada el usuario quita el aviso que había y la segunda carpeta.
        escribir(d.path(), "nuevo.n43", b"nuevo");
        escribir(otra.path(), "de-la-otra.n43", b"otra");
        let cambia = |_: &Path, _: &Path, _: &[u8]| -> Result<Found> {
            dismiss(&v, None).unwrap();
            set_folders(&v, std::slice::from_ref(&a)).unwrap();
            Ok(banco())
        };
        pasada(&v, &cambia);
        // El aviso quitado no vuelve, la carpeta quitada no se mira ni se apunta.
        assert_eq!(avisos(&v), [("nuevo.n43".to_owned(), None)]);
        assert_eq!(folders(&v).unwrap(), std::slice::from_ref(&a));
        assert!(seen(&v).unwrap().keys().all(|k| k.starts_with(&a)));
    }

    #[test]
    fn leer_sin_seguir_enlaces_y_con_tope() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("f.pdf");
        std::fs::write(&p, b"0123456789").unwrap();
        assert_eq!(read_regular(&p, 10).ok().unwrap(), b"0123456789");
        assert!(matches!(read_regular(&p, 9), Err(ReadError::TooBig)));
        #[cfg(unix)]
        {
            let enlace = d.path().join("enlace.pdf");
            std::os::unix::fs::symlink(&p, &enlace).unwrap();
            assert!(matches!(read_regular(&enlace, 100), Err(ReadError::Io)));
        }
        assert!(matches!(read_regular(d.path(), 100), Err(ReadError::Io)));
    }

    #[test]
    fn lo_guardado_antes_se_sigue_leyendo() {
        let v = vault();
        // Un aviso guardado por una versión anterior (sin `error`).
        v.set_setting(
            PENDING_KEY,
            &serde_json::json!([{
                "folder": "/x", "path": "a.n43", "kind": "Extracto bancario",
                "year": 2026, "seen": "2026-11-01"
            }]),
        )
        .unwrap();
        assert_eq!(pending(&v).unwrap()[0].error, None);
    }
}
