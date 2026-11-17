//! Copias de seguridad de la empresa (formato y cifrado en `taxpain_vault::backup`): ajustes,
//! copias a mano y automáticas a una carpeta (iCloud Drive, Google Drive, Dropbox…), rotación,
//! recordatorio en el panel y lo que hay que comprobar tras restaurar una copia.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::backup::{self as vault_backup, BackupHeader, BackupReport};
use taxpain_vault::rusqlite::{Connection, OptionalExtension};
use taxpain_vault::{Vault, VaultError};
use time::{Duration, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;

pub const SETTINGS_KEY: &str = "backup";
const LAST_KEY: &str = "backup_last";
const ERROR_KEY: &str = "backup_error";
const RESTORED_KEY: &str = "restored_backup";
/// Marca de las copias automáticas de esta empresa en este Mac.
const TAG_KEY: &str = "backup_tag";
/// Sin copia en este plazo, el panel lo recuerda.
pub const REMIND_DAYS: i64 = 30;
/// Subcarpeta para las copias dentro de la carpeta de TaxPain en la nube.
const FOLDER_NAME: &str = "Copias de seguridad";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupSettings {
    /// Carpeta de las copias automáticas (y la propuesta para las manuales).
    pub folder: Option<PathBuf>,
    /// Copia automática cada `every_days` días mientras la app está abierta.
    pub auto: bool,
    pub every_days: u32,
    /// Copias automáticas que se conservan (las más antiguas se borran).
    pub keep: u32,
}

impl Default for BackupSettings {
    fn default() -> Self {
        BackupSettings {
            folder: None,
            auto: false,
            every_days: 7,
            keep: 10,
        }
    }
}

/// Última copia hecha.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastBackup {
    pub at: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub auto: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupError {
    pub at: String,
    pub message: String,
}

/// La empresa viene de restaurar una copia.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Restored {
    pub at: String,
    /// Fecha de la copia restaurada.
    pub copy_created_at: String,
    /// Cuándo confirmó el usuario que la numeración y VERI*FACTU están al día.
    pub acknowledged_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BackupStatus {
    pub has_passphrase: bool,
    pub settings: BackupSettings,
    pub last: Option<LastBackup>,
    pub last_error: Option<BackupError>,
    /// Días desde la última copia.
    pub days_since: Option<i64>,
    /// Hace más de [`REMIND_DAYS`] días (o nunca) que no se hace una copia.
    pub remind: bool,
    /// Carpeta propuesta (la de las facturas en la nube, si hay).
    pub suggested_folder: Option<PathBuf>,
    /// Nombre propuesto para una copia hecha ahora.
    pub suggested_name: String,
    pub restored: Option<Restored>,
    pub min_passphrase: usize,
    /// Junto a los datos está el código de recuperación (la clave de los datos sellada con la
    /// contraseña de las copias): carpeta + contraseña bastan para abrirlos en otro Mac.
    pub recovery_code: bool,
    /// Empresa sin contraseña de copias que la ha de elegir antes de empezar: la recién dada de
    /// alta (sin facturas, recibidas ni asientos; DATOS-SEGURIDAD-01) y la recién restaurada de
    /// una copia, que no hereda la contraseña de la copia. La interfaz la pide al abrirla.
    pub setup_required: bool,
}

pub fn settings(v: &Vault) -> Result<BackupSettings> {
    Ok(v.setting(SETTINGS_KEY)?.unwrap_or_default())
}

pub fn save_settings(v: &Vault, s: &BackupSettings) -> Result<BackupSettings> {
    let mut s = s.clone();
    if s.auto && s.folder.is_none() {
        return Err(AppError::Other(
            "elige la carpeta de las copias automáticas".into(),
        ));
    }
    if let Some(f) = &s.folder
        && !f.is_absolute()
    {
        return Err(AppError::Other(
            "la carpeta de las copias no es válida".into(),
        ));
    }
    s.every_days = s.every_days.clamp(1, 90);
    s.keep = s.keep.clamp(1, 100);
    v.set_setting(SETTINGS_KEY, &s)?;
    Ok(s)
}

fn parse_at(at: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(at, &time::format_description::well_known::Rfc3339).ok()
}

/// Nombre de fichero sin caracteres problemáticos a partir del nombre de la empresa.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(40).collect();
    let out = out.trim_end_matches('-').to_owned();
    if out.is_empty() {
        "TaxPain".into()
    } else {
        out
    }
}

/// `Empresa-AAAA-MM-DD.taxpain`; las automáticas acaban en `-auto-<marca>` (ver
/// [`auto_tag`]).
pub fn file_name(company: &str, now: OffsetDateTime, auto_tag: Option<&str>) -> String {
    let d = taxpain_core::spain_time::peninsular_date(now);
    format!(
        "{}-{:04}-{:02}-{:02}{}.{}",
        slug(company),
        d.year(),
        u8::from(d.month()),
        d.day(),
        auto_tag.map(|t| format!("-auto-{t}")).unwrap_or_default(),
        vault_backup::EXTENSION
    )
}

fn is_tag(t: &str) -> bool {
    t.len() == 8 && t.bytes().all(|b| b.is_ascii_lowercase())
}

/// Marca (8 letras al azar) de las copias automáticas de esta empresa: la rotación sólo borra
/// las que la llevan, aunque otra empresa se llame igual (p. ej. una copia restaurada en el
/// mismo Mac) o se cambie el nombre. Una empresa restaurada tiene su propia marca.
fn auto_tag(v: &Vault) -> Result<String> {
    if let Some(t) = v.setting::<String>(TAG_KEY)?.filter(|t| is_tag(t)) {
        return Ok(t);
    }
    let mut b = [0u8; 8];
    getrandom::fill(&mut b).map_err(|e| AppError::Other(e.to_string()))?;
    let t: String = b.iter().map(|x| char::from(b'a' + x % 26)).collect();
    v.set_setting(TAG_KEY, &t)?;
    Ok(t)
}

/// Carpeta propuesta: junto a las facturas en la nube; si no, la primera carpeta de
/// sincronización del Mac; si no, Documentos/TaxPain.
fn suggested_folder(v: &Vault, home: Option<&Path>) -> Result<Option<PathBuf>> {
    if let Some(dest) = crate::invoicing::settings(v)?.and_then(|s| s.destination) {
        return Ok(Some(dest.path.join(FOLDER_NAME)));
    }
    Ok(home.and_then(|h| {
        crate::settings::detect_destinations(h)
            .into_iter()
            .next()
            .map(|d| d.path.join(FOLDER_NAME))
    }))
}

pub fn status(
    v: &Vault,
    company: &str,
    home: Option<&Path>,
    now: OffsetDateTime,
) -> Result<BackupStatus> {
    let last: Option<LastBackup> = v.setting(LAST_KEY)?;
    let days_since = last
        .as_ref()
        .and_then(|l| parse_at(&l.at))
        .map(|at| (now - at).whole_days().max(0));
    let has_passphrase = vault_backup::has_passphrase(v)?;
    let restored: Option<Restored> = v.setting(RESTORED_KEY)?;
    Ok(BackupStatus {
        has_passphrase,
        recovery_code: v.dir().is_some_and(taxpain_vault::recovery::exists),
        setup_required: !has_passphrase && (restored.is_some() || is_new_company(v)),
        settings: settings(v)?,
        remind: days_since.is_none_or(|d| d > REMIND_DAYS),
        days_since,
        last,
        last_error: v.setting(ERROR_KEY)?,
        suggested_folder: suggested_folder(v, home)?,
        suggested_name: file_name(company, now, None),
        restored,
        min_passphrase: vault_backup::MIN_PASSPHRASE,
    })
}

pub fn set_passphrase(v: &Vault, passphrase: &str, now: OffsetDateTime) -> Result<()> {
    Ok(vault_backup::set_passphrase(v, passphrase, now)?)
}

pub fn verify_passphrase(v: &Vault, passphrase: &str) -> Result<bool> {
    Ok(vault_backup::verify_passphrase(v, passphrase)?)
}

/// Hace una copia en `dest` y la apunta como la última.
pub fn create(
    v: &Vault,
    dest: &Path,
    auto: bool,
    app_version: &str,
    now: OffsetDateTime,
) -> Result<BackupReport> {
    if dest.extension().and_then(|e| e.to_str()) != Some(vault_backup::EXTENSION) {
        return Err(AppError::Other(format!(
            "el fichero de la copia ha de acabar en .{}",
            vault_backup::EXTENSION
        )));
    }
    match vault_backup::export(v, dest, app_version, now) {
        Ok(r) => {
            v.set_setting(
                LAST_KEY,
                &LastBackup {
                    at: r.created_at.clone(),
                    path: r.path.clone(),
                    bytes: r.bytes,
                    auto,
                },
            )?;
            v.connection()
                .execute("DELETE FROM settings WHERE key = ?1", [ERROR_KEY])?;
            // Para ofrecer esta copia aunque un día la BD no abra (ver `recuperacion`).
            if let Some(dir) = v.dir() {
                let empresa = crate::companies::info(v)
                    .map(|i| i.name)
                    .unwrap_or_default();
                crate::recuperacion::recordar_copia(
                    dir,
                    &empresa,
                    &r.path,
                    &r.created_at,
                    settings(v)?.folder.as_deref(),
                );
            }
            Ok(r)
        }
        Err(e) => {
            let message = e.to_string();
            v.set_setting(
                ERROR_KEY,
                &BackupError {
                    at: now_str(now),
                    message: message.clone(),
                },
            )?;
            Err(AppError::Other(message))
        }
    }
}

/// ¿Toca copia automática?
pub fn auto_due(v: &Vault, now: OffsetDateTime) -> Result<bool> {
    let s = settings(v)?;
    if !s.auto || s.folder.is_none() || !vault_backup::has_passphrase(v)? {
        return Ok(false);
    }
    let last: Option<LastBackup> = v.setting(LAST_KEY)?;
    let error: Option<BackupError> = v.setting(ERROR_KEY)?;
    // Tras un fallo se reintenta al cabo de una hora, no en cada vuelta.
    if error
        .and_then(|e| parse_at(&e.at))
        .is_some_and(|at| now - at < Duration::hours(1))
    {
        return Ok(false);
    }
    Ok(last.and_then(|l| parse_at(&l.at)).is_none_or(|at| {
        now - at >= Duration::days(i64::from(s.every_days)) - Duration::minutes(30)
    }))
}

/// Copia automática si toca: en la carpeta elegida, sin pisar otra del mismo día, y borra las
/// automáticas que sobran (sólo las `-auto` de esta empresa; nunca las hechas a mano).
pub fn run_auto(
    v: &Vault,
    company: &str,
    app_version: &str,
    now: OffsetDateTime,
) -> Result<Option<BackupReport>> {
    if !auto_due(v, now)? {
        return Ok(None);
    }
    let s = settings(v)?;
    let Some(folder) = s.folder else {
        return Ok(None);
    };
    let tag = auto_tag(v)?;
    let base = file_name(company, now, Some(&tag));
    let mut dest = folder.join(&base);
    let mut n = 2;
    while dest.exists() {
        dest = folder.join(base.replace(&format!("-auto-{tag}."), &format!("-auto-{tag}-{n}.")));
        n += 1;
    }
    let r = create(v, &dest, true, app_version, now)?;
    rotate(&folder, &tag, &slug(company), s.keep);
    Ok(Some(r))
}

/// Borra las copias automáticas más antiguas de esta empresa que pasan de `keep`.
fn rotate(folder: &Path, tag: &str, slug: &str, keep: u32) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let mut autos: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| is_own_auto(&e.file_name().to_string_lossy(), tag, slug))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    autos.sort_by_key(|a| std::cmp::Reverse(a.0));
    for (_, path) in autos
        .into_iter()
        .skip(usize::try_from(keep).unwrap_or(usize::MAX))
    {
        if let Err(e) = std::fs::remove_file(&path) {
            tracing::warn!(error = %e, "no se pudo borrar una copia automática antigua");
        }
    }
}

/// `head` acaba en `-AAAA-MM-DD`: devuelve lo de antes.
fn before_date(head: &str) -> Option<&str> {
    let (prefix, date) = head.split_at_checked(head.len().checked_sub(11)?)?;
    let d = date.as_bytes();
    let digits =
        |r: std::ops::Range<usize>| d.get(r).is_some_and(|x| x.iter().all(u8::is_ascii_digit));
    (d.first() == Some(&b'-')
        && digits(1..5)
        && d.get(5) == Some(&b'-')
        && digits(6..8)
        && d.get(8) == Some(&b'-')
        && digits(9..11))
    .then_some(prefix)
}

/// ¿Es `name` una copia automática de esta empresa? Las de ahora llevan su marca
/// (`…-AAAA-MM-DD-auto-<marca>[-N].taxpain`); las de antes de la marca, el nombre de la empresa
/// (`Empresa-AAAA-MM-DD-auto[-N].taxpain`). Nunca las hechas a mano.
fn is_own_auto(name: &str, tag: &str, slug: &str) -> bool {
    let Some(stem) = name.strip_suffix(&format!(".{}", vault_backup::EXTENSION)) else {
        return false;
    };
    let stem = match stem.rsplit_once('-') {
        Some((head, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => stem,
    };
    if let Some(head) = stem.strip_suffix(&format!("-auto-{tag}")) {
        return before_date(head).is_some_and(|p| !p.is_empty());
    }
    stem.strip_suffix("-auto").and_then(before_date) == Some(slug)
}

/// Lo que hay dentro de una copia, para enseñarlo antes de restaurarla.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BackupSummary {
    pub created_at: String,
    pub app_version: String,
    pub company: String,
    pub nif: Option<String>,
    pub invoices_issued: i64,
    pub invoices_received: i64,
    pub journal_entries: i64,
    /// Fecha de la última factura emitida.
    pub last_invoice_date: Option<String>,
    /// La versión de la copia es más nueva que esta app.
    pub newer_version: bool,
    /// Empresa de este Mac con el mismo NIF (la rellena quien conoce las empresas): restaurar
    /// daría dos empresas que emiten en las mismas series.
    pub same_nif_company: Option<String>,
}

/// NIF comparable: sin espacios, guiones ni el prefijo `ES`, en mayúsculas.
pub fn same_nif(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        let s: String = s
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>()
            .to_uppercase();
        s.strip_prefix("ES").map(str::to_owned).unwrap_or(s)
    };
    let (a, b) = (norm(a), norm(b));
    !a.is_empty() && a == b
}

/// Sin facturas emitidas ni recibidas ni asientos: la empresa se acaba de dar de alta. Las que
/// ya tienen datos (de antes de pedir la contraseña en el alta) la eligen desde Ajustes, y la
/// empresa de pruebas VERI*FACTU no la necesita.
fn is_new_company(v: &Vault) -> bool {
    if crate::verifactu_lab::info(v).map_or(true, |lab| lab.is_some()) {
        return false;
    }
    let c = v.connection();
    ["invoice_issued", "invoice_received", "journal_entry"]
        .iter()
        .all(|t| count(c, &format!("SELECT count(*) FROM {t}")) == 0)
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap_or(0)
}

/// Abre la copia con su contraseña y resume su contenido.
pub fn inspect(src: &Path, passphrase: &str) -> Result<BackupSummary> {
    let header = vault_backup::read_header(src)?;
    let newer = usize::try_from(header.schema_version)
        .map_or(true, |s| s > taxpain_vault::db::schema_version());
    let opened = vault_backup::open(src, passphrase)?;
    let conn = &opened.conn;
    // La copia puede venir de otra persona: sólo se leen tablas (una vista con el nombre de una
    // tabla ejecutaría la consulta que hubiera escrito quien hizo la copia).
    let table = |name: &str| opened.has_table(name).unwrap_or(false);
    let setting = |key: &str| -> Option<String> {
        if !table("settings") {
            return None;
        }
        conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()
        .ok()
        .flatten()
    };
    let rows = |name: &str| {
        if table(name) {
            count(conn, &format!("SELECT count(*) FROM {name}"))
        } else {
            0
        }
    };
    let company = setting("company");
    let issuer = setting("invoicing");
    let (name, nif) = company_from(company.as_deref(), issuer.as_deref());
    Ok(BackupSummary {
        created_at: header.created_at,
        app_version: header.app_version,
        company: name,
        nif,
        invoices_issued: rows("invoice_issued"),
        invoices_received: rows("invoice_received"),
        journal_entries: rows("journal_entry"),
        last_invoice_date: table("invoice_issued")
            .then(|| {
                conn.query_row("SELECT max(issue_date) FROM invoice_issued", [], |r| {
                    r.get(0)
                })
                .ok()
                .flatten()
            })
            .flatten(),
        newer_version: newer,
        same_nif_company: None,
    })
}

/// Nombre y NIF a partir de los ajustes `company` o, si no hay, del emisor de las facturas.
fn company_from(company: Option<&str>, issuer: Option<&str>) -> (String, Option<String>) {
    let parse = |s: Option<&str>| s.and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
    if let Some(c) = parse(company) {
        return (
            c["name"].as_str().unwrap_or("Empresa").to_owned(),
            c["nif"].as_str().map(str::to_owned),
        );
    }
    if let Some(i) = parse(issuer) {
        let name = i["issuer"]["name"].as_str().unwrap_or("Empresa").to_owned();
        let nif = i["issuer"]["id"]["Nif"].as_str().map(str::to_owned);
        return (name, nif);
    }
    ("Empresa sin nombre".into(), None)
}

/// Ajustes que son rutas de carpetas del Mac donde se hizo la copia. No valen en éste y, si la
/// copia viene de otra persona, son rutas que ha elegido ella: las carpetas vigiladas las lee
/// un hilo de fondo cada dos minutos (también en las empresas que no están abiertas) y mete en
/// Recibidas las facturas que encuentra, y en la carpeta de la empresa se escriben las descargas.
/// Se borran al restaurar; el usuario las vuelve a elegir.
const PATH_SETTINGS: [&str; 2] = [
    // `crate::vigiladas`: la clave antigua, de una sola carpeta.
    crate::vigiladas::OLD_KEY,
    // `crate::carpeta`: la carpeta de la empresa elegida la última vez.
    "carpeta_empresa",
];

/// Prefijo de todos los ajustes de `crate::vigiladas` (la lista de carpetas, lo visto, los
/// avisos, lo que no se pudo leer, lo que se estaba leyendo…): se borran por el prefijo, para
/// que uno nuevo no se quede fuera.
const WATCHED_PREFIX: &str = "carpetas_vigiladas";

/// Tras restaurar: marca la empresa para que no se emita hasta comprobar la numeración y le
/// quita los ajustes que son del Mac (o de la persona) de donde viene la copia.
pub fn mark_restored(v: &Vault, header: &BackupHeader, now: OffsetDateTime) -> Result<()> {
    v.set_setting(
        RESTORED_KEY,
        &Restored {
            at: now_str(now),
            copy_created_at: header.created_at.clone(),
            acknowledged_at: None,
        },
    )?;
    // Lo del Mac anterior no vale en éste: ni la carpeta ni las copias automáticas.
    v.set_setting(SETTINGS_KEY, &BackupSettings::default())?;
    let conn = v.connection();
    conn.execute(
        "DELETE FROM settings WHERE key IN (?1, ?2, ?3)",
        [LAST_KEY, ERROR_KEY, TAG_KEY],
    )?;
    for key in PATH_SETTINGS {
        conn.execute("DELETE FROM settings WHERE key = ?1", [key])?;
    }
    conn.execute(
        "DELETE FROM settings WHERE substr(key, 1, length(?1)) = ?1",
        [WATCHED_PREFIX],
    )?;
    clear_invoice_destination(v)?;
    Ok(())
}

/// Quita de los ajustes de facturación la carpeta donde se guardan las facturas emitidas (el
/// resto de los ajustes no se toca). Se cambia sobre el JSON guardado, sin pasar por
/// `InvoicingSettings`: unos ajustes de una versión anterior, o que no se entiendan, no han de
/// impedir restaurar.
fn clear_invoice_destination(v: &Vault) -> Result<()> {
    let Some(mut s) = v.setting::<serde_json::Value>(crate::settings::SETTINGS_KEY)? else {
        return Ok(());
    };
    let Some(map) = s.as_object_mut() else {
        return Ok(());
    };
    if map
        .get("destination")
        .is_none_or(serde_json::Value::is_null)
    {
        return Ok(());
    }
    map.insert("destination".into(), serde_json::Value::Null);
    v.set_setting(crate::settings::SETTINGS_KEY, &s)?;
    Ok(())
}

/// Lo que hay que comprobar tras restaurar antes de volver a emitir.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct RestoreCheck {
    pub restored: Restored,
    /// Última factura de cada serie en la copia: (serie, número, fecha).
    pub last_by_series: Vec<(String, String, String)>,
    /// Último registro VERI*FACTU: número, fecha y huella.
    pub last_verifactu: Option<(String, String, String)>,
}

pub fn restore_check(v: &Vault) -> Result<Option<RestoreCheck>> {
    let Some(restored) = v.setting::<Restored>(RESTORED_KEY)? else {
        return Ok(None);
    };
    let conn = v.connection();
    let mut stmt = conn.prepare(
        "SELECT s.code, i.number, i.issue_date FROM invoice_issued i
           JOIN invoice_series s ON s.id = i.series_id
          WHERE i.id IN (SELECT max(id) FROM invoice_issued GROUP BY series_id)
          ORDER BY s.code",
    )?;
    let last_by_series = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let last_verifactu = conn
        .query_row(
            "SELECT num_serie, fecha, huella FROM verifactu_record ORDER BY id DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    Ok(Some(RestoreCheck {
        restored,
        last_by_series,
        last_verifactu,
    }))
}

/// El usuario confirma que la última factura y el último registro VERI*FACTU de la copia son
/// los últimos de verdad (no se emitió nada después de la copia en el Mac anterior).
pub fn acknowledge_restore(v: &Vault, now: OffsetDateTime) -> Result<()> {
    let Some(mut r) = v.setting::<Restored>(RESTORED_KEY)? else {
        return Ok(());
    };
    if r.acknowledged_at.is_some() {
        return Ok(());
    }
    r.acknowledged_at = Some(now_str(now));
    v.set_setting(RESTORED_KEY, &r)?;
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        "backup.restore_checked",
        "facturas",
        &serde_json::json!({ "copy_created_at": r.copy_created_at }),
    )?;
    Ok(())
}

/// Error si la empresa viene de una copia y aún no se ha comprobado la numeración: emitir, anular,
/// subsanar o enviar con una copia antigua repetiría números, bifurcaría la cadena de VERI*FACTU o
/// reenviaría registros que el otro Mac ya envió. Todo lo que escribe o envía la cadena pasa por
/// aquí.
pub fn ensure_can_issue(v: &Vault) -> Result<()> {
    match v.setting::<Restored>(RESTORED_KEY)? {
        Some(r) if r.acknowledged_at.is_none() => Err(AppError::Other(
            "esta empresa viene de una copia de seguridad: antes de emitir, anular o enviar \
             facturas, comprueba en el panel que su última factura es la última que emitiste"
                .into(),
        )),
        _ => Ok(()),
    }
}

/// Errores del almacén al restaurar, con un mensaje para el usuario.
pub fn restore_error(e: VaultError) -> AppError {
    match e {
        VaultError::Corrupted => {
            AppError::Other("la copia está dañada o no se puede abrir con esta contraseña".into())
        }
        e => AppError::Vault(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn nombres_de_fichero() {
        assert_eq!(slug("Taller Ejemplo, S.L."), "Taller-Ejemplo-S-L");
        assert_eq!(slug("  "), "TaxPain");
        assert_eq!(
            file_name(
                "Ejemplo SL",
                datetime!(2026-10-24 23:30 UTC),
                Some("abcdefgh")
            ),
            "Ejemplo-SL-2026-10-25-auto-abcdefgh.taxpain",
            "fecha de Madrid"
        );
        let own = |n: &str| is_own_auto(n, "abcdefgh", "Ejemplo-SL");
        assert!(own("Ejemplo-SL-2026-10-25-auto-abcdefgh.taxpain"));
        assert!(own("Ejemplo-SL-2026-10-25-auto-abcdefgh-2.taxpain"));
        assert!(
            own("Nombre-Nuevo-2026-10-25-auto-abcdefgh.taxpain"),
            "tras cambiar el nombre"
        );
        assert!(
            own("Ejemplo-SL-2026-10-25-auto.taxpain"),
            "las de antes de la marca"
        );
        assert!(own("Ejemplo-SL-2026-10-25-auto-3.taxpain"));
        assert!(
            !own("Ejemplo-SL-2026-10-25-auto-zzzzzzzz.taxpain"),
            "otra empresa"
        );
        assert!(!own("Otra-2026-10-25-auto.taxpain"));
        assert!(!own("Ejemplo-SL-2026-10-25.taxpain"), "a mano");
        assert!(!own("copia-auto-abcdefgh.taxpain"));
        assert!(!own("Ñandú-2026-10-25-auto.taxpain"));
    }

    #[test]
    fn empresa_de_la_copia() {
        assert_eq!(
            company_from(Some(r#"{"name":"A SL","nif":"B1"}"#), None),
            ("A SL".into(), Some("B1".into()))
        );
        assert_eq!(
            company_from(
                None,
                Some(r#"{"issuer":{"name":"B SL","id":{"Nif":"B2"}}}"#)
            ),
            ("B SL".into(), Some("B2".into()))
        );
    }
}
