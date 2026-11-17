//! Estado de la app: el almacén con los proveedores de la plataforma.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

use taxpain_app::companies::{self, Company, CompanyInfo};
use taxpain_secrets::{KeyProvider, UserPresence};
use taxpain_vault::{Vault, VaultError};

use crate::{CmdResult, CommandError};

pub struct AppState {
    /// Carpeta de datos de la app (con `data/`, `companies/` y `active-company`).
    root: PathBuf,
    keys: Arc<dyn KeyProvider>,
    presence: Arc<dyn UserPresence>,
    /// Empresa abierta y su almacén (bajo el mismo cerrojo: cambiar de empresa es atómico).
    active: Mutex<Active>,
    /// Para abrir aparte el almacén de otra empresa (lectura) o borrar una (escritura): mientras
    /// se borra, nadie tiene abierta esa empresa. Se coge siempre antes que `active`.
    files: RwLock<()>,
    /// Mueve una carpeta a la Papelera (en las pruebas, a otra carpeta).
    trash: Trash,
}

/// Razón (fija: no la elige la interfaz) que enseña Touch ID al cambiar la contraseña de las
/// copias.
const CHANGE_PASSPHRASE_REASON: &str = "Cambiar la contraseña de las copias";

/// Cómo se mueve a la Papelera la carpeta de una empresa borrada.
pub type Trash = Arc<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;

/// Almacén de otra empresa abierto aparte (ver [`AppState::open_company`]): mientras exista, esa
/// empresa no se puede borrar.
pub(crate) struct OtherVault<'a> {
    vault: Vault,
    _files: RwLockReadGuard<'a, ()>,
}

impl std::ops::Deref for OtherVault<'_> {
    type Target = Vault;
    fn deref(&self) -> &Vault {
        &self.vault
    }
}

struct Active {
    id: String,
    vault: Vault,
}

/// El almacén de una empresa, sea la abierta o no (ver [`AppState::company_vault`]).
pub(crate) enum CompanyVault<'a> {
    Active(VaultGuard<'a>),
    Other(Box<OtherVault<'a>>),
}

impl std::ops::Deref for CompanyVault<'_> {
    type Target = Vault;
    fn deref(&self) -> &Vault {
        match self {
            CompanyVault::Active(g) => g,
            CompanyVault::Other(o) => o,
        }
    }
}

impl AppState {
    pub fn open(dir: &Path) -> Result<Self, VaultError> {
        let (keys, presence) = platform_providers(dir)?;
        Self::open_with(dir, keys, presence, platform_trash(dir))
    }

    /// [`Self::open`] con otros proveedores de claves y otra Papelera (pruebas).
    pub fn open_with(
        dir: &Path,
        keys: Arc<dyn KeyProvider>,
        presence: Arc<dyn UserPresence>,
        trash: Trash,
    ) -> Result<Self, VaultError> {
        migrate_old_dir(dir);
        companies::clean_staging(dir);
        let open = |id: &str| -> Result<Vault, VaultError> {
            let d = companies::dir(dir, id).map_err(|e| VaultError::InvalidInput(e.to_string()))?;
            Vault::open(&d, keys.clone(), presence.clone())
        };
        let id = companies::active(dir);
        // Si la empresa que estaba abierta no se puede abrir, se arranca con la primera de la
        // lista que abra (la principal, si no se ha borrado); la que no abre sale en el selector
        // marcada, con su pantalla de recuperación. Si no abre ninguna, el error de la activa.
        let others: Vec<String> = companies::ids(dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|i| *i != id)
            .collect();
        let (id, vault) = match open(&id) {
            Ok(v) => (id, v),
            Err(e) => {
                tracing::warn!(error = %e, "no se pudo abrir la empresa activa; se prueba otra");
                let mut found = None;
                for other in others {
                    match open(&other) {
                        Ok(v) => {
                            found = Some((other, v));
                            break;
                        }
                        Err(e2) => {
                            tracing::warn!(empresa = %other, error = %e2, "tampoco abre");
                        }
                    }
                }
                match found {
                    Some((other, v)) => {
                        // Al volver a arrancar se abre ésta (la otra se repara desde el selector).
                        let _ = companies::set_active(dir, &other);
                        (other, v)
                    }
                    None => return Err(e),
                }
            }
        };
        Ok(AppState {
            root: dir.to_owned(),
            keys,
            presence,
            active: Mutex::new(Active { id, vault }),
            files: RwLock::new(()),
            trash,
        })
    }

    /// Recupera la clave de los datos sin el Llavero, con la contraseña de las copias y el
    /// código de recuperación de cualquiera de las empresas `ids` (la clave es la misma para
    /// todas: vale la contraseña de cualquiera), la vuelve a guardar en el Llavero y abre la app
    /// como siempre. Si ninguna coincide, [`VaultError::BackupPassphrase`].
    ///
    /// Por eso mismo (un código de recuperación abre todas las empresas del Mac) una empresa
    /// restaurada de una copia no hereda la contraseña de la copia y una empresa borrada no se
    /// lleva el suyo a la Papelera: ver [`Self::restore_company`] y [`Self::delete_company`].
    pub fn recover(dir: &Path, ids: &[String], passphrase: &str) -> Result<Self, VaultError> {
        let (keys, presence) = platform_providers(dir)?;
        let mut wrong_pass = false;
        let mut first_err = None;
        for id in ids {
            let r = companies::dir(dir, id)
                .map_err(|e| VaultError::InvalidInput(e.to_string()))
                .and_then(|d| {
                    Vault::open_recovered(&d, keys.clone(), presence.clone(), passphrase)
                });
            match r {
                Ok(v) => {
                    drop(v);
                    return Self::open(dir);
                }
                Err(VaultError::BackupPassphrase) => wrong_pass = true,
                Err(e) => {
                    tracing::warn!(empresa = %id, error = %e, "no se pudo recuperar con esta empresa");
                    first_err.get_or_insert(e);
                }
            }
        }
        // Una contraseña que no coincide pesa más que un código de recuperación dañado: lo
        // útil para el usuario es saber que pruebe otra.
        Err(match first_err {
            Some(e) if !wrong_pass => e,
            _ => VaultError::BackupPassphrase,
        })
    }

    /// Cerrojo del almacén. Si un comando entró en pánico con él cogido (el mutex queda
    /// «envenenado»), se recupera en vez de dejar la app inservible hasta reiniciarla: se
    /// deshace la transacción que pudiera quedar abierta y se sigue.
    fn lock(&self) -> Result<MutexGuard<'_, Active>, VaultError> {
        match self.active.lock() {
            Ok(g) => Ok(g),
            Err(poisoned) => {
                let g = poisoned.into_inner();
                let conn = g.vault.connection();
                if !conn.is_autocommit()
                    && let Err(e) = conn.execute_batch("ROLLBACK")
                {
                    tracing::warn!(error = %e, "no se pudo deshacer la transacción a medias");
                }
                self.active.clear_poison();
                tracing::warn!("un comando falló a medias: se recupera el almacén y se sigue");
                Ok(g)
            }
        }
    }

    pub fn vault(&self) -> Result<VaultGuard<'_>, VaultError> {
        self.lock().map(VaultGuard)
    }

    /// Id de la empresa abierta.
    pub fn active_id(&self) -> Result<String, VaultError> {
        Ok(self.lock()?.id.clone())
    }

    /// [`Self::vault`] sólo si la empresa abierta sigue siendo `id`: para lo que suelta el
    /// cerrojo a medias (leer documentos con el Mac) y no debe acabar guardando en otra empresa
    /// si el usuario cambia entretanto.
    pub(crate) fn vault_of(&self, id: &str) -> CmdResult<VaultGuard<'_>> {
        let g = self.lock()?;
        if g.id != id {
            return Err(CommandError::from(
                "se ha cambiado de empresa a medias: vuelve a intentarlo en la que toca",
            ));
        }
        Ok(VaultGuard(g))
    }

    /// El almacén de la empresa abierta y su id, con el mismo cerrojo (sin carrera entre los
    /// dos): para lo que después suelta el cerrojo y sigue con [`Self::vault_of`].
    pub(crate) fn vault_with_id(&self) -> Result<(String, VaultGuard<'_>), VaultError> {
        let g = self.lock()?;
        Ok((g.id.clone(), VaultGuard(g)))
    }

    /// Como [`AppState::identity`], pero sólo si la empresa abierta sigue siendo `id` (el
    /// certificado se desella del almacén de esa empresa, no del de otra).
    pub(crate) fn identity_of(
        &self,
        id: &str,
        request: taxpain_vault::IdentityRequest,
    ) -> CmdResult<taxpain_vault::ClientIdentity> {
        let confirmed = request.confirm()?;
        Ok(self
            .vault_of(id)?
            .finish_identity(confirmed, time::OffsetDateTime::now_utc())?)
    }

    /// Cierre que da [`Self::vault_of`] (para las funciones de `taxpain_app` que cogen el
    /// almacén sólo para lo que lo necesitan).
    pub(crate) fn vault_fn<'a>(
        &'a self,
        id: &'a str,
    ) -> impl Fn() -> taxpain_app::Result<VaultGuard<'a>> + 'a {
        move || {
            self.vault_of(id)
                .map_err(|e| taxpain_app::AppError::Other(e.message))
        }
    }

    /// Abre aparte el almacén de otra empresa (no la activa: para ésa, [`Self::vault`]).
    /// Sólo una que existe: con un id de una lista vieja (una empresa ya borrada) no se crea otra
    /// vacía. No llamar dentro de [`Self::for_each_company`] (el cerrojo no es reentrante).
    pub(crate) fn open_company(&self, id: &str) -> CmdResult<OtherVault<'_>> {
        let files = self.files_read();
        if !self.exists(id)? {
            return Err(CommandError::from("esa empresa no existe"));
        }
        Ok(OtherVault {
            vault: self.open_dir(id)?,
            _files: files,
        })
    }

    /// Abre el almacén de la empresa `id` sin coger `files` (quien llama ya lo tiene).
    fn open_dir(&self, id: &str) -> CmdResult<Vault> {
        let d = companies::dir(&self.root, id)?;
        Ok(Vault::open(&d, self.keys.clone(), self.presence.clone())?)
    }

    fn files_read(&self) -> RwLockReadGuard<'_, ()> {
        self.files.read().unwrap_or_else(|p| p.into_inner())
    }

    fn files_write(&self) -> RwLockWriteGuard<'_, ()> {
        self.files.write().unwrap_or_else(|p| p.into_inner())
    }

    /// ¿Existe la empresa `id`? (abrir el almacén de una que no existe la crearía vacía).
    fn exists(&self, id: &str) -> CmdResult<bool> {
        Ok(companies::ids(&self.root)?.iter().any(|i| i == id))
    }

    /// ¿Hay más de una empresa?
    pub fn several_companies(&self) -> bool {
        companies::ids(&self.root).is_ok_and(|ids| ids.len() > 1)
    }

    /// Empresas de este Mac, con la activa marcada. Las que no abren salen también, con
    /// `no_abre` (y el nombre de la última copia apuntada, si lo hay).
    pub(crate) fn companies(&self) -> CmdResult<Vec<Company>> {
        let _files = self.files_read();
        let active = self.active_id()?;
        let mut out = Vec::new();
        for id in companies::ids(&self.root)? {
            let opened = if id == active {
                self.vault()
                    .map_err(CommandError::from)
                    .and_then(|v| Ok(companies::info(&v)?))
            } else {
                self.open_dir(&id).and_then(|v| Ok(companies::info(&v)?))
            };
            match opened {
                Ok(i) => out.push(Company {
                    id: id.clone(),
                    name: i.name,
                    nif: i.nif,
                    active: id == active,
                    no_abre: None,
                }),
                Err(e) => {
                    let known = companies::dir(&self.root, &id)
                        .map(|d| taxpain_app::recuperacion::copias_conocidas(&d).empresa)
                        .ok()
                        .flatten();
                    out.push(Company {
                        name: known.unwrap_or_else(|| format!("Empresa {id}")),
                        id,
                        nif: None,
                        active: false,
                        no_abre: Some(e.message),
                    });
                }
            }
        }
        Ok(out)
    }

    /// Diagnóstico de las empresas que no abren (no la activa, que está abierta).
    pub(crate) fn broken_companies(
        &self,
    ) -> CmdResult<Vec<taxpain_app::recuperacion::Diagnostico>> {
        let _files = self.files_read();
        let active = self.active_id()?;
        let mut out = Vec::new();
        for id in companies::ids(&self.root)? {
            if id == active {
                continue;
            }
            let d = companies::dir(&self.root, &id)?;
            if let Err(e) = Vault::open(&d, self.keys.clone(), self.presence.clone()) {
                out.push(diagnose(&d, &id, &e, self.keys.as_ref()));
            }
        }
        Ok(out)
    }

    /// Rescata lo que se pueda de la empresa `id` (que no abre) en un ZIP en `dest`, sin tocarla.
    pub(crate) fn rescue_company(&self, id: &str, dest: &Path) -> CmdResult<RescueReport> {
        let _files = self.files_read();
        if !self.exists(id)? {
            return Err(CommandError::from("esa empresa no existe"));
        }
        rescue(&self.root, id, self.keys.as_ref(), dest)
    }

    /// Ejecuta `f` con el almacén de cada empresa (la activa con el suyo; las demás se abren y
    /// se cierran). Una empresa que no abre se salta (sale en el selector como «no abre»): las
    /// vistas de grupo, los avisos y las copias siguen con las demás. Un fallo de `f` en una
    /// empresa no impide seguir con las demás: se devuelve el primero al final.
    pub(crate) fn for_each_company(
        &self,
        mut f: impl FnMut(&str, bool, &Vault) -> CmdResult<()>,
    ) -> CmdResult<()> {
        let _files = self.files_read();
        let active = self.active_id()?;
        let mut first_err = None;
        for id in companies::ids(&self.root)? {
            let r = if id == active {
                let v = self.vault()?;
                f(&id, true, &v)
            } else {
                match self.open_dir(&id) {
                    Ok(v) => f(&id, false, &v),
                    Err(e) => {
                        tracing::warn!(empresa = %id, error = %e.message, "una empresa no abre: se salta");
                        continue;
                    }
                }
            };
            if let Err(e) = r {
                tracing::warn!(empresa = %id, error = %e.message, "fallo en una empresa");
                first_err.get_or_insert(e);
            }
        }
        first_err.map_or(Ok(()), Err)
    }

    /// Da de alta otra empresa (almacén nuevo, vacío) sin cambiar la activa. Con `passphrase`
    /// queda definida la contraseña de las copias (y el código de recuperación).
    pub(crate) fn create_company(
        &self,
        info: &CompanyInfo,
        passphrase: Option<&str>,
    ) -> CmdResult<Company> {
        let info = companies::normalize(info)?;
        // Antes de crear nada: una contraseña que no sirve no deja una empresa a medias.
        if let Some(p) = passphrase {
            taxpain_vault::backup::check_passphrase(p)?;
        }
        let id = companies::new_id()?;
        // Almacén nuevo: se crea al abrirlo (`open_company` sólo abre las que ya existen).
        let _files = self.files_read();
        let v = self.open_dir(&id)?;
        let now = time::OffsetDateTime::now_utc();
        let info = companies::set_info(&v, &info, now)?;
        if let Some(p) = passphrase {
            taxpain_app::backup::set_passphrase(&v, p, now)?;
        }
        Ok(Company {
            id,
            name: info.name,
            nif: info.nif,
            active: false,
            no_abre: None,
        })
    }

    /// Restaura una copia de seguridad como empresa nueva (no cambia a ella). La emisión queda
    /// bloqueada en ella hasta que el usuario compruebe la numeración. `passphrase` es la de la
    /// copia y sólo sirve para abrirla: la empresa restaurada queda sin contraseña de copias (y
    /// sin código de recuperación) y la interfaz le pide una nueva al abrirla
    /// (`BackupStatus::setup_required`).
    pub(crate) fn restore_company(&self, src: &Path, passphrase: &str) -> CmdResult<Company> {
        restore_into(
            &self.root,
            self.keys.clone(),
            self.presence.clone(),
            src,
            passphrase,
        )
    }

    /// Cambia de empresa: abre su almacén y sustituye al activo (que se cierra).
    pub(crate) fn switch_company(&self, id: &str) -> CmdResult<()> {
        let _files = self.files_read();
        self.switch_locked(id)
    }

    /// [`Self::switch_company`] con `files` ya cogido.
    fn switch_locked(&self, id: &str) -> CmdResult<()> {
        if !self.exists(id)? {
            return Err(CommandError::from("esa empresa no existe"));
        }
        if self.active_id()? == id {
            return Ok(());
        }
        let vault = self.open_dir(id)?;
        let old = {
            let mut a = self.lock()?;
            let old = std::mem::replace(&mut a.vault, vault);
            a.id = id.to_owned();
            old
        };
        // Al cerrarse, el almacén anterior actualiza su ancla de auditoría.
        drop(old);
        companies::set_active(&self.root, id)?;
        Ok(())
    }

    /// Cambia el nombre y el NIF de cualquier empresa (la abierta o no).
    pub(crate) fn update_company(&self, id: &str, info: &CompanyInfo) -> CmdResult<CompanyInfo> {
        let _files = self.files_read();
        if !self.exists(id)? {
            return Err(CommandError::from("esa empresa no existe"));
        }
        let now = time::OffsetDateTime::now_utc();
        // Mismo cerrojo para mirar cuál es la abierta y guardar: sin carrera con un cambio.
        let active = self.lock()?;
        if active.id == id {
            let g = VaultGuard(active);
            return Ok(companies::set_info(&g, info, now)?);
        }
        drop(active);
        Ok(companies::set_info(&self.open_dir(id)?, info, now)?)
    }

    /// El almacén de la empresa `id`, sea o no la abierta (si no lo es, se abre aparte, como en
    /// [`Self::update_company`]): para guardar lo que llega tarde (la respuesta del asistente) en
    /// la empresa en la que se pidió, aunque el usuario haya cambiado de empresa mientras tanto.
    pub(crate) fn company_vault(&self, id: &str) -> CmdResult<CompanyVault<'_>> {
        let files = self.files_read();
        if !self.exists(id)? {
            return Err(CommandError::from("esa empresa no existe"));
        }
        // Mismo cerrojo para mirar cuál es la abierta y usarla: sin carrera con un cambio.
        let active = self.lock()?;
        if active.id == id {
            return Ok(CompanyVault::Active(VaultGuard(active)));
        }
        drop(active);
        Ok(CompanyVault::Other(Box::new(OtherVault {
            vault: self.open_dir(id)?,
            _files: files,
        })))
    }

    /// Borra la empresa `id`: su carpeta va a la Papelera (se puede recuperar mientras no se
    /// vacíe). `confirm` es su nombre o su NIF, escrito por el usuario. Si es la abierta, antes
    /// se abre otra (la primera que queda) y devuelve `true`: la interfaz se ha de recargar. Si
    /// algo falla, no se borra nada y sigue abierta la misma empresa.
    ///
    /// A la Papelera no va su código de recuperación (`recovery.sealed`): se borra antes. La
    /// clave que sella es la de todas las empresas del Mac, así que ese fichero, con la
    /// contraseña de las copias de la empresa borrada, seguiría abriendo las que quedan. Si la
    /// carpeta se saca de la Papelera en este mismo Mac, la empresa se abre con el Llavero y el
    /// código se vuelve a escribir solo; en otro Mac ya no se podría abrir (para llevarse una
    /// empresa a otro Mac está la copia de seguridad).
    pub(crate) fn delete_company(&self, id: &str, confirm: &str) -> CmdResult<bool> {
        // Nadie más abre ninguna empresa mientras tanto (ni las tareas de fondo).
        let _files = self.files_write();
        let all = companies::ids(&self.root)?;
        if !all.iter().any(|i| i == id) {
            return Err(CommandError::from("esa empresa no existe"));
        }
        let Some(next) = all.iter().find(|i| *i != id) else {
            return Err(CommandError::from(
                "es la única empresa: no se puede borrar (añade antes otra)",
            ));
        };
        let was_active = self.active_id()? == id;
        let info = if was_active {
            companies::info(&*self.vault()?)?
        } else {
            companies::info(&self.open_dir(id)?)?
        };
        if !companies::confirms(&info, confirm) {
            return Err(CommandError::from(
                "escribe el nombre o el NIF de la empresa para confirmar",
            ));
        }
        if was_active {
            // Al cambiar se cierra su almacén: no queda nada abierto en su carpeta.
            self.switch_locked(next)?;
        }
        // Antes de la Papelera, fuera el código de recuperación. Si no se puede borrar, la
        // empresa no se borra; y si luego falla la Papelera, al volver a abrir la empresa se
        // repone solo (`Vault::open`).
        let removed = companies::dir(&self.root, id)
            .map_err(CommandError::from)
            .and_then(|d| {
                taxpain_vault::recovery::remove(&d).map_err(|e| {
                    CommandError::from(format!(
                        "no se pudo borrar el código de recuperación de la empresa: {e}"
                    ))
                })
            })
            .and_then(|()| Ok(companies::remove(&self.root, id, &*self.trash)?));
        if let Err(e) = removed {
            if was_active && let Err(back) = self.switch_locked(id) {
                tracing::warn!(error = %back.message, "no se pudo volver a la empresa");
            }
            return Err(e);
        }
        let now = time::OffsetDateTime::now_utc();
        // Queda anotado en la empresa que se queda abierta.
        let noted = self
            .vault()
            .map_err(CommandError::from)
            .and_then(|v| Ok(companies::record_deleted(&v, id, &info, now)?));
        if let Err(e) = noted {
            tracing::warn!(error = %e.message, "no se pudo anotar el borrado en la auditoría");
        }
        tracing::info!(empresa = %id, "empresa borrada (a la Papelera)");
        Ok(was_active)
    }

    /// Pide la confirmación (Touch ID) **sin** tener el almacén bloqueado, y después desella el
    /// certificado. Mientras el usuario responde, el resto de la app sigue funcionando.
    /// No llamar con un `vault()` en vigor (el mutex no es reentrante).
    pub fn identity(
        &self,
        request: taxpain_vault::IdentityRequest,
    ) -> Result<taxpain_vault::ClientIdentity, VaultError> {
        let confirmed = request.confirm()?;
        self.vault()?
            .finish_identity(confirmed, time::OffsetDateTime::now_utc())
    }

    /// Define o cambia la contraseña de las copias de la empresa abierta. La primera vez no
    /// pide nada más; si ya hay una, cambiarla exige antes la confirmación de presencia (Touch
    /// ID), sin tener el almacén bloqueado mientras el usuario responde: quien se sienta un
    /// momento delante de la app abierta no puede cambiar la contraseña y, con ella, reescribir
    /// el código de recuperación (que abre todas las empresas del Mac). No llamar con un
    /// `vault()` en vigor.
    pub(crate) fn set_backup_passphrase(&self, passphrase: &str) -> CmdResult<()> {
        let (id, had) = {
            let (id, v) = self.vault_with_id()?;
            let had = taxpain_vault::backup::has_passphrase(&v)?;
            (id, had)
        };
        if had {
            self.presence
                .confirm(CHANGE_PASSPHRASE_REASON)
                .map_err(VaultError::from)?;
        }
        let v = self.vault_of(&id)?;
        // Entre mirar y guardar se ha soltado el cerrojo: si mientras tanto otra ventana ha
        // definido una, esto ya es un cambio y no se ha confirmado.
        if !had && taxpain_vault::backup::has_passphrase(&v)? {
            return Err(CommandError::from(
                "la contraseña de las copias se acaba de definir: vuelve a intentarlo para \
                 cambiarla",
            ));
        }
        Ok(taxpain_app::backup::set_passphrase(
            &v,
            passphrase,
            time::OffsetDateTime::now_utc(),
        )?)
    }

    /// Pide la confirmación de presencia (Touch ID o la contraseña del Mac) antes de un acto
    /// con efectos (emitir o anular una factura, cerrar un ejercicio…). `reason` la construye
    /// Rust con datos del almacén, nunca la interfaz: es lo que el usuario ve en el aviso del
    /// sistema. Si no confirma, [`VaultError::NotConfirmed`] («operación no confirmada por el
    /// usuario»). No llamar con un `vault()` en vigor: el resto de la app sigue mientras tanto.
    pub(crate) fn confirm_presence(&self, reason: &str) -> CmdResult<()> {
        self.presence.confirm(reason).map_err(VaultError::from)?;
        Ok(())
    }

    /// El usuario ha visto el aviso de que la auditoría de la empresa abierta ha retrocedido (o
    /// le falta el ancla): deja de estar pendiente y queda anotado. `false` si no había ninguno.
    /// Sólo desde una acción del usuario en la interfaz.
    pub fn acknowledge_audit_rollback(&self) -> Result<bool, VaultError> {
        self.vault()?
            .acknowledge_audit_rollback(time::OffsetDateTime::now_utc())
    }

    /// Como [`AppState::identity`], para borrar un certificado: Touch ID sin el almacén
    /// bloqueado y después el borrado. No llamar con un `vault()` en vigor.
    pub fn delete_certificate(
        &self,
        request: taxpain_vault::DeleteRequest,
    ) -> Result<(), VaultError> {
        let confirmed = request.confirm()?;
        self.vault()?
            .finish_delete(confirmed, time::OffsetDateTime::now_utc())
    }
}

/// Restaura la copia `src` como empresa nueva en `root` (ver [`AppState::restore_company`]):
/// también sin la app arrancada, desde la pantalla de recuperación.
pub(crate) fn restore_into(
    root: &Path,
    keys: Arc<dyn KeyProvider>,
    presence: Arc<dyn UserPresence>,
    src: &Path,
    passphrase: &str,
) -> CmdResult<Company> {
    let id = companies::new_id()?;
    // Se restaura en una carpeta aparte que la lista no ve, se marca como «viene de una
    // copia» (la marca viaja con los datos) y sólo entonces se publica con un `rename`: si la
    // app se cierra a medias no queda una empresa incompleta ni una sin el bloqueo de emitir.
    let staging = companies::staging_dir(root, &id)?;
    let now = time::OffsetDateTime::now_utc();
    let staged = (|| -> CmdResult<CompanyInfo> {
        let (v, header) =
            taxpain_vault::backup::restore(src, passphrase, &staging, keys, presence, now)
                .map_err(taxpain_app::backup::restore_error)?;
        taxpain_app::backup::mark_restored(&v, &header, now)?;
        let info = companies::info(&v)?;
        // Al cerrarse, el almacén deja su ancla de auditoría en la carpeta.
        drop(v);
        Ok(info)
    })();
    let info = match staged.and_then(|info| {
        companies::publish_staged(root, &id)?;
        Ok(info)
    }) {
        Ok(info) => info,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(e);
        }
    };
    Ok(Company {
        id,
        name: info.name,
        nif: info.nif,
        active: false,
        no_abre: None,
    })
}

/// Diagnóstico de la empresa `id` (carpeta `dir`) que no abre con el error `e`.
pub(crate) fn diagnose(
    dir: &Path,
    id: &str,
    e: &VaultError,
    keys: &dyn KeyProvider,
) -> taxpain_app::recuperacion::Diagnostico {
    let has_key = keys
        .get(
            taxpain_secrets::KeyId::DbMaster,
            "Comprobar la clave de los datos",
        )
        .ok()
        .flatten()
        .is_some();
    taxpain_app::recuperacion::diagnosticar(id, dir, e, has_key)
}

/// Diagnóstico de todas las empresas de `root` que no abren, sin la app arrancada (pantalla de
/// recuperación al arrancar).
pub(crate) fn diagnose_all(root: &Path) -> Vec<taxpain_app::recuperacion::Diagnostico> {
    let Ok((keys, presence)) = providers(root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for id in companies::ids(root).unwrap_or_default() {
        let Ok(d) = companies::dir(root, &id) else {
            continue;
        };
        if let Err(e) = Vault::open(&d, keys.clone(), presence.clone()) {
            out.push(diagnose(&d, &id, &e, keys.as_ref()));
        }
    }
    out
}

/// Lo que ha salido de un rescate.
#[derive(serde::Serialize)]
pub(crate) struct RescueReport {
    pub fichero: String,
    pub originales: Vec<String>,
    pub tablas: usize,
    pub filas: usize,
    pub sin_volcado: Option<String>,
}

/// Rescata la empresa `id` de `root` en el ZIP `dest`: los ficheros tal cual y, si se puede,
/// sus tablas en CSV y JSON. No escribe nada en la carpeta de la empresa.
pub(crate) fn rescue(
    root: &Path,
    id: &str,
    keys: &dyn KeyProvider,
    dest: &Path,
) -> CmdResult<RescueReport> {
    use taxpain_vault::volcado::Destino;
    let d = companies::dir(root, id)?;
    let mut z = taxpain_app::archivo::exportacion::ZipDestino::new("TaxPain - rescate");
    let r = taxpain_vault::volcado::rescatar(&d, keys, &mut z)?;
    let empresa = taxpain_app::recuperacion::copias_conocidas(&d)
        .empresa
        .unwrap_or_else(|| format!("la empresa {id}"));
    let at = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    let mut leeme = match &r.volcado {
        Some(v) => taxpain_app::archivo::exportacion::leeme(&empresa, v, &at, true),
        None => format!(
            "RESCATE DE LOS DATOS DE {empresa}\nGenerado por TaxPain el {at}.\n\nNo se ha \
             podido leer la base de datos: {}.\n",
            r.sin_volcado.clone().unwrap_or_default()
        ),
    };
    leeme.push_str(
        "\noriginales/  Los ficheros de la empresa tal cual (cifrados): con ellos y la clave del \
         Llavero, o con la contraseña de las copias y recovery.sealed, se pueden volver a abrir \
         en TaxPain.\n",
    );
    z.fichero("LEEME.txt", leeme.as_bytes())?;
    let bytes = z.terminar()?;
    std::fs::write(dest, &bytes)?;
    Ok(RescueReport {
        fichero: dest
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        originales: r.originales,
        tablas: r.volcado.as_ref().map_or(0, |v| v.tablas.len()),
        filas: r
            .volcado
            .as_ref()
            .map_or(0, taxpain_vault::volcado::Volcado::filas),
        sin_volcado: r.sin_volcado,
    })
}

/// Empresas que se pueden recuperar con la contraseña de las copias cuando el Llavero no tiene
/// la clave (las que tienen código de recuperación), en este orden: la activa, la principal y
/// las demás. La clave es la misma para todas las empresas del Mac: basta con una.
pub fn recoverable_companies(root: &Path) -> Vec<String> {
    let mut order = vec![companies::active(root), companies::PRINCIPAL.to_owned()];
    order.extend(companies::ids(root).unwrap_or_default());
    let mut out: Vec<String> = Vec::new();
    for id in order {
        if !out.contains(&id)
            && companies::dir(root, &id).is_ok_and(|d| {
                d.join("taxpain.db").is_file() && taxpain_vault::recovery::exists(&d)
            })
        {
            out.push(id);
        }
    }
    out
}

/// Acceso exclusivo al almacén de la empresa activa. Al soltarlo (fin de cada comando) se
/// actualiza el ancla de la auditoría con lo que se haya registrado.
pub struct VaultGuard<'a>(MutexGuard<'a, Active>);

impl std::ops::Deref for VaultGuard<'_> {
    type Target = Vault;
    fn deref(&self) -> &Vault {
        &self.0.vault
    }
}

impl Drop for VaultGuard<'_> {
    fn drop(&mut self) {
        if let Err(e) = self.0.vault.sync_anchor() {
            tracing::warn!(error = %e, "no se pudo actualizar el ancla de la auditoría");
        }
    }
}

/// Papelera del Mac.
#[cfg(target_os = "macos")]
fn platform_trash(_dir: &Path) -> Trash {
    Arc::new(taxpain_macos::move_to_trash)
}

/// Otras plataformas (sólo desarrollo): una carpeta `papelera/` junto a los datos.
#[cfg(not(target_os = "macos"))]
fn platform_trash(dir: &Path) -> Trash {
    let bin = dir.join("papelera");
    Arc::new(move |p: &Path| {
        let name = p.file_name().ok_or("ruta no válida")?.to_string_lossy();
        let stamp = time::OffsetDateTime::now_utc().unix_timestamp_nanos();
        std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
        std::fs::rename(p, bin.join(format!("{name}-{stamp}"))).map_err(|e| e.to_string())
    })
}

/// Proveedor de claves y confirmación de presencia de la plataforma.
type Providers = (Arc<dyn KeyProvider>, Arc<dyn UserPresence>);

/// [`platform_providers`] para la pantalla de recuperación (sin la app arrancada).
pub(crate) fn providers(dir: &Path) -> Result<Providers, VaultError> {
    platform_providers(dir)
}

#[cfg(target_os = "macos")]
fn platform_providers(_dir: &Path) -> Result<Providers, VaultError> {
    use taxpain_macos::{KeychainKeyProvider, KeychainMode, TouchIdPresence};
    // El Keychain de protección de datos (D9) exige la app firmada con el entitlement
    // `keychain-access-groups`: sólo se usa si la compilación lo pide expresamente
    // (`TAXPAIN_KEYCHAIN=data-protection`, docs/RELEASE.md). Si no (desarrollo o DMG sin
    // firmar), el Keychain clásico, que también guarda la clave cifrada por el sistema.
    let mode = match option_env!("TAXPAIN_KEYCHAIN") {
        Some("data-protection") => KeychainMode::DataProtection,
        _ => KeychainMode::Legacy,
    };
    Ok((
        Arc::new(KeychainKeyProvider::new(mode)),
        Arc::new(TouchIdPresence),
    ))
}

/// Otras plataformas: sólo desarrollo. Claves en fichero (inseguro, la UI lo avisa) y sin
/// confirmación biométrica. Windows tendrá su proveedor (DPAPI / Windows Hello) más adelante.
/// Una compilación release para otra plataforma no se puede construir: no tendría ni claves
/// protegidas ni confirmación de presencia antes de firmar o presentar.
#[cfg(all(not(target_os = "macos"), not(debug_assertions)))]
compile_error!(
    "TaxPain sólo se distribuye para macOS: fuera de macOS no hay Keychain ni Touch ID \
     (compila en modo desarrollo para probar)"
);

#[cfg(not(target_os = "macos"))]
fn platform_providers(dir: &Path) -> Result<Providers, VaultError> {
    use taxpain_secrets::{AlwaysConfirm, FileKeyProvider};
    Ok((
        Arc::new(FileKeyProvider::new(dir.join("dev-keys"))?),
        Arc::new(AlwaysConfirm),
    ))
}

/// Hasta la 0.1.0 el identificador era `es.taxpain.app` (desaconsejado en macOS: termina como la
/// extensión de los paquetes). Si existe la carpeta antigua y no la nueva, se mueve tal cual (la
/// clave de la BD está en el Keychain con un servicio que no cambia).
fn migrate_old_dir(dir: &Path) {
    let Some(old) = dir.parent().map(|p| p.join("es.taxpain.app")) else {
        return;
    };
    if old != dir
        && old.join("data").exists()
        && !dir.join("data").exists()
        && !companies::principal_deleted(dir)
    {
        if let Some(parent) = dir.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::rename(&old, dir) {
            Ok(()) => tracing::info!("datos movidos al nuevo identificador de la app"),
            Err(e) => tracing::warn!(error = %e, "no se pudieron mover los datos antiguos"),
        }
    }
}

/// Fuera del Mac las claves van en `dev-keys/` (ficheros): se puede simular que el Llavero
/// pierde la clave borrando el fichero.
#[cfg(all(test, not(target_os = "macos")))]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use taxpain_secrets::KeyId;

    #[test]
    fn sin_la_clave_se_recupera_con_la_contrasena_de_las_copias() {
        let root = std::env::temp_dir().join(format!("taxpain-recover-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        const PASS: &str = "caballo grapa batería";
        let key_file = root
            .join("dev-keys")
            .join(format!("{}.key", KeyId::DbMaster.name()));
        {
            let s = AppState::open(&root).unwrap();
            let v = s.vault().unwrap();
            taxpain_app::backup::set_passphrase(&v, PASS, time::OffsetDateTime::now_utc()).unwrap();
        }
        // Con la contraseña queda el código de recuperación junto a los datos.
        assert_eq!(recoverable_companies(&root), vec![companies::PRINCIPAL]);
        std::fs::remove_file(&key_file).unwrap();
        assert!(matches!(
            AppState::open(&root),
            Err(VaultError::KeyMissing { recoverable: true })
        ));
        assert!(!key_file.exists(), "no se crea otra clave");
        assert!(matches!(
            AppState::recover(&root, &[companies::PRINCIPAL.to_owned()], "otra contraseña"),
            Err(VaultError::BackupPassphrase)
        ));
        let s = AppState::recover(&root, &[companies::PRINCIPAL.to_owned()], PASS).unwrap();
        assert!(taxpain_vault::backup::has_passphrase(&s.vault().unwrap()).unwrap());
        drop(s);
        assert!(key_file.exists(), "la clave vuelve al llavero");
        assert!(AppState::open(&root).is_ok());
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// F1/F2: con varias empresas vale la contraseña de las copias de cualquiera, aunque la
    /// activa no tenga código de recuperación.
    #[test]
    fn con_varias_empresas_vale_la_contrasena_de_cualquiera() {
        let root = std::env::temp_dir().join(format!("taxpain-recover-n-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let key_file = root
            .join("dev-keys")
            .join(format!("{}.key", KeyId::DbMaster.name()));
        let info = |name: &str| CompanyInfo {
            name: name.to_owned(),
            nif: None,
        };
        let (b, c) = {
            let s = AppState::open(&root).unwrap();
            let b = s
                .create_company(&info("EMPRESA DEMO SL"), Some("caballo grapa batería"))
                .unwrap();
            let c = s
                .create_company(&info("OTRA DEMO SL"), Some("tortuga lenta 2026 azul"))
                .unwrap();
            // La activa (la principal) no tiene contraseña de las copias.
            (b.id, c.id)
        };
        let ids = recoverable_companies(&root);
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&b) && ids.contains(&c));
        std::fs::remove_file(&key_file).unwrap();
        assert!(matches!(
            AppState::recover(&root, &ids, "otra contraseña"),
            Err(VaultError::BackupPassphrase)
        ));
        let s = AppState::recover(&root, &ids, "tortuga lenta 2026 azul").unwrap();
        assert_eq!(s.active_id().unwrap(), companies::PRINCIPAL);
        assert_eq!(s.companies().unwrap().len(), 3);
        drop(s);
        assert!(key_file.exists(), "la clave vuelve al llavero");
        std::fs::remove_dir_all(&root).unwrap();
    }
}

/// Modificar y borrar empresas, con las claves en memoria y una «Papelera» que es otra carpeta
/// (nunca el Llavero ni la Papelera de verdad).
#[cfg(test)]
mod company_tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use taxpain_secrets::{AlwaysConfirm, MemoryKeyProvider};

    struct Env {
        tmp: PathBuf,
        keys: Arc<dyn KeyProvider>,
    }

    impl Env {
        fn new(name: &str) -> Self {
            let tmp = std::env::temp_dir()
                .join(format!("taxpain-empresas-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&tmp);
            Env {
                tmp,
                keys: Arc::new(MemoryKeyProvider::default()),
            }
        }
        fn root(&self) -> PathBuf {
            self.tmp.join("app")
        }
        fn bin(&self) -> PathBuf {
            self.tmp.join("papelera")
        }
        fn open_with_trash(&self, trash: Trash) -> AppState {
            AppState::open_with(
                &self.root(),
                self.keys.clone(),
                Arc::new(AlwaysConfirm),
                trash,
            )
            .unwrap()
        }
        /// Papelera de pruebas: mueve la carpeta a `papelera/`.
        fn trash(&self) -> Trash {
            let bin = self.bin();
            Arc::new(move |p: &Path| {
                std::fs::create_dir_all(&bin).map_err(|e| e.to_string())?;
                let name = p.file_name().ok_or("sin nombre")?;
                std::fs::rename(p, bin.join(name)).map_err(|e| e.to_string())
            })
        }
        fn open(&self) -> AppState {
            self.open_with_trash(self.trash())
        }
        fn open_with_presence(&self, presence: Arc<dyn UserPresence>) -> AppState {
            AppState::open_with(&self.root(), self.keys.clone(), presence, self.trash()).unwrap()
        }
    }

    const PASS: &str = "caballo grapa batería";

    /// Touch ID de pruebas: apunta las razones que enseña y se puede negar.
    #[derive(Debug, Default)]
    struct Presence {
        deny: std::sync::atomic::AtomicBool,
        reasons: Mutex<Vec<String>>,
    }

    impl Presence {
        fn deny(&self, deny: bool) {
            self.deny.store(deny, std::sync::atomic::Ordering::SeqCst);
        }
        fn reasons(&self) -> Vec<String> {
            self.reasons.lock().unwrap().clone()
        }
    }

    impl UserPresence for Presence {
        fn confirm(&self, reason: &str) -> taxpain_secrets::Result<()> {
            self.reasons.lock().unwrap().push(reason.to_owned());
            if self.deny.load(std::sync::atomic::Ordering::SeqCst) {
                Err(taxpain_secrets::Error::NotConfirmed)
            } else {
                Ok(())
            }
        }
    }

    /// Definir la contraseña de las copias la primera vez no pide nada; cambiarla (y con ella
    /// el código de recuperación) exige Touch ID, con una razón fija.
    #[test]
    fn cambiar_la_contrasena_de_las_copias_pide_presencia() {
        let env = Env::new("presencia");
        let presence = Arc::new(Presence::default());
        let s = env.open_with_presence(presence.clone());
        let verify = |pass: &str| {
            taxpain_vault::backup::verify_passphrase(&s.vault().unwrap(), pass).unwrap()
        };
        // La primera vez, aunque Touch ID fuera a decir que no: no se pregunta.
        presence.deny(true);
        s.set_backup_passphrase(PASS).unwrap();
        assert!(presence.reasons().is_empty());
        assert!(verify(PASS));

        // Cambiarla sin confirmar: no cambia nada.
        let e = s
            .set_backup_passphrase("tortuga lenta 2026 azul")
            .unwrap_err();
        assert!(e.message.contains("no confirmada"), "{}", e.message);
        assert_eq!(presence.reasons(), ["Cambiar la contraseña de las copias"]);
        assert!(verify(PASS) && !verify("tortuga lenta 2026 azul"));
        let dir = companies::dir(&env.root(), companies::PRINCIPAL).unwrap();
        assert!(taxpain_vault::recovery::unseal(&dir, PASS).is_ok());

        // Confirmando, cambia (y el código de recuperación pasa a la nueva).
        presence.deny(false);
        s.set_backup_passphrase("tortuga lenta 2026 azul").unwrap();
        assert_eq!(presence.reasons().len(), 2);
        assert!(verify("tortuga lenta 2026 azul") && !verify(PASS));
        assert!(taxpain_vault::recovery::unseal(&dir, PASS).is_err());
        assert!(taxpain_vault::recovery::unseal(&dir, "tortuga lenta 2026 azul").is_ok());
        // Una contraseña que no sirve se rechaza igual, antes o después de confirmar.
        assert!(s.set_backup_passphrase("corta").is_err());
        assert!(verify("tortuga lenta 2026 azul"));
    }

    /// La carpeta de una empresa borrada va a la Papelera sin su código de recuperación: la
    /// clave que sella es la de todas las empresas del Mac.
    #[test]
    fn borrar_una_empresa_no_manda_su_codigo_de_recuperacion_a_la_papelera() {
        let env = Env::new("borrar-codigo");
        let s = env.open();
        s.set_backup_passphrase("tortuga lenta 2026 azul").unwrap();
        let b = s
            .create_company(&info("EMPRESA DEMO SL", Some("B12345674")), Some(PASS))
            .unwrap();
        let dir = companies::dir(&env.root(), &b.id).unwrap();
        assert!(taxpain_vault::recovery::exists(&dir));
        // Un temporal del código que hubiera quedado a medias tampoco viaja.
        std::fs::write(dir.join(".recovery.sealed.tmp"), b"x").unwrap();

        assert!(!s.delete_company(&b.id, "B12345674").unwrap());
        let trashed = env.bin().join(&b.id);
        assert!(trashed.join("taxpain.db").is_file());
        assert!(!trashed.join("recovery.sealed").exists());
        assert!(!trashed.join(".recovery.sealed.tmp").exists());
        // La que queda conserva el suyo.
        let principal = companies::dir(&env.root(), companies::PRINCIPAL).unwrap();
        assert!(taxpain_vault::recovery::exists(&principal));

        // También si la borrada era la abierta.
        let c = s
            .create_company(&info("OTRA DEMO SL", None), Some(PASS))
            .unwrap();
        s.switch_company(&c.id).unwrap();
        assert!(s.delete_company(&c.id, "OTRA DEMO SL").unwrap());
        let trashed = env.bin().join(&c.id);
        assert!(trashed.join("taxpain.db").is_file());
        assert!(!trashed.join("recovery.sealed").exists());
    }

    /// Si la Papelera falla, la empresa sigue ahí y su código de recuperación se repone al
    /// volver a abrirla.
    #[test]
    fn si_la_papelera_falla_el_codigo_de_recuperacion_se_repone() {
        let env = Env::new("falla-codigo");
        let s = env.open_with_trash(Arc::new(|_: &Path| Err("sin permiso".to_owned())));
        let b = s
            .create_company(&info("EMPRESA DEMO SL", None), Some(PASS))
            .unwrap();
        let dir = companies::dir(&env.root(), &b.id).unwrap();
        assert!(s.delete_company(&b.id, "EMPRESA DEMO SL").is_err());
        assert_eq!(s.companies().unwrap().len(), 2);
        assert!(taxpain_vault::recovery::exists(&dir));
        assert!(taxpain_vault::recovery::unseal(&dir, PASS).is_ok());
    }

    /// Restaurar una copia no deja en la empresa nueva la contraseña de la copia (que es de
    /// quien la hizo) ni un código de recuperación sellado con ella.
    #[test]
    fn una_empresa_restaurada_no_hereda_la_contrasena_de_la_copia() {
        let env = Env::new("restaurar");
        let s = env.open();
        s.update_company(companies::PRINCIPAL, &info("EJEMPLO SL", Some("B12345674")))
            .unwrap();
        s.set_backup_passphrase(PASS).unwrap();
        let copy = env.tmp.join("copia.taxpain");
        taxpain_app::backup::create(
            &s.vault().unwrap(),
            &copy,
            false,
            "0.1.0",
            time::OffsetDateTime::now_utc(),
        )
        .unwrap();
        assert!(s.restore_company(&copy, "otra contraseña").is_err());
        assert_eq!(s.companies().unwrap().len(), 1);

        let restored = s.restore_company(&copy, PASS).unwrap();
        assert_eq!(restored.name, "EJEMPLO SL");
        let dir = companies::dir(&env.root(), &restored.id).unwrap();
        assert!(!taxpain_vault::recovery::exists(&dir));
        {
            let v = s.open_company(&restored.id).unwrap();
            assert!(!taxpain_vault::backup::has_passphrase(&v).unwrap());
            let st = taxpain_app::backup::status(
                &v,
                "EJEMPLO SL",
                None,
                time::OffsetDateTime::now_utc(),
            )
            .unwrap();
            assert!(st.setup_required && st.restored.is_some());
            assert!(v.audit_rollback_pending().unwrap().is_none());
        }
        // Abrirla (también desde las tareas de fondo) no le escribe un código de recuperación.
        assert!(!taxpain_vault::recovery::exists(&dir));
        // Al abrirla el usuario elige la suya: sin Touch ID (es la primera) y con su código.
        s.switch_company(&restored.id).unwrap();
        s.set_backup_passphrase("tortuga lenta 2026 azul").unwrap();
        assert!(taxpain_vault::recovery::unseal(&dir, "tortuga lenta 2026 azul").is_ok());
        assert!(taxpain_vault::recovery::unseal(&dir, PASS).is_err());
    }

    impl Drop for Env {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.tmp);
        }
    }

    fn info(name: &str, nif: Option<&str>) -> CompanyInfo {
        CompanyInfo {
            name: name.to_owned(),
            nif: nif.map(str::to_owned),
        }
    }

    /// Una empresa que no abre sale en la lista marcada, las vistas de grupo siguen con las
    /// demás, se diagnostica y se rescata sin tocarla.
    #[test]
    fn una_empresa_que_no_abre_no_rompe_las_demas() {
        let env = Env::new("no-abre");
        let s = env.open();
        let b = s
            .create_company(&info("EMPRESA DEMO SL", None), None)
            .unwrap();
        let db = companies::dir(&env.root(), &b.id)
            .unwrap()
            .join("taxpain.db");
        let _ = std::fs::remove_file(db.with_extension("db-wal"));
        std::fs::write(&db, b"esto no es una base de datos").unwrap();
        let list = s.companies().unwrap();
        assert_eq!(list.len(), 2);
        let rota = list.iter().find(|c| c.id == b.id).unwrap();
        assert!(rota.no_abre.is_some());
        let mut vistas = 0;
        s.for_each_company(|_, _, _| {
            vistas += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(vistas, 1, "se salta la que no abre");
        let diags = s.broken_companies().unwrap();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].id, b.id);
        assert_eq!(diags[0].tipo, taxpain_app::recuperacion::Tipo::Danada);
        let dest = env.tmp.join("rescate.zip");
        let r = s.rescue_company(&b.id, &dest).unwrap();
        assert!(r.originales.contains(&"taxpain.db".to_owned()));
        assert!(r.sin_volcado.is_some());
        assert!(dest.is_file());
        assert_eq!(std::fs::read(&db).unwrap(), b"esto no es una base de datos");
    }

    #[test]
    fn modifica_una_empresa_que_no_esta_abierta() {
        let env = Env::new("modificar");
        let s = env.open();
        let b = s
            .create_company(&info("EMPRESA DEMO SL", None), None)
            .unwrap();
        let saved = s
            .update_company(&b.id, &info(" OTRA DEMO SL ", Some("b98765431")))
            .unwrap();
        assert_eq!(saved, info("OTRA DEMO SL", Some("B98765431")));
        assert!(
            s.update_company(&b.id, &info("X", Some("B12345675")))
                .is_err(),
            "NIF que no es válido"
        );
        assert!(
            s.update_company("ffffffffffffffff", &info("X", None))
                .is_err()
        );
        assert!(!env.root().join("companies/ffffffffffffffff").exists());
        let list = s.companies().unwrap();
        let other = list.iter().find(|c| c.id == b.id).unwrap();
        assert_eq!(other.name, "OTRA DEMO SL");
        assert_eq!(other.nif.as_deref(), Some("B98765431"));
        // Sigue abierta la misma y también se puede modificar.
        assert_eq!(s.active_id().unwrap(), companies::PRINCIPAL);
        s.update_company(companies::PRINCIPAL, &info("Mi tienda", None))
            .unwrap();
        assert_eq!(
            companies::info(&s.vault().unwrap()).unwrap().name,
            "Mi tienda"
        );
    }

    #[test]
    fn company_vault_abre_la_otra_si_ya_no_es_la_activa() {
        let env = Env::new("guardar-tarde");
        let s = env.open();
        let b = s
            .create_company(&info("EMPRESA DEMO SL", None), None)
            .unwrap();
        // La abierta: su mismo almacén.
        {
            let v = s.company_vault(companies::PRINCIPAL).unwrap();
            assert!(matches!(v, CompanyVault::Active(_)));
            v.set_setting("prueba", &1).unwrap();
        }
        // Si se cambia de empresa, la de antes se abre aparte y lo que se guarda queda en ella.
        s.switch_company(&b.id).unwrap();
        {
            let v = s.company_vault(companies::PRINCIPAL).unwrap();
            assert!(matches!(v, CompanyVault::Other(_)));
            assert_eq!(v.setting::<i32>("prueba").unwrap(), Some(1));
            v.set_setting("prueba", &2).unwrap();
        }
        assert_eq!(s.active_id().unwrap(), b.id);
        assert_eq!(s.vault().unwrap().setting::<i32>("prueba").unwrap(), None);
        s.switch_company(companies::PRINCIPAL).unwrap();
        assert_eq!(
            s.vault().unwrap().setting::<i32>("prueba").unwrap(),
            Some(2)
        );
        // Una que no existe no se crea vacía.
        assert!(s.company_vault("ffffffffffffffff").is_err());
        assert!(!env.root().join("companies/ffffffffffffffff").exists());
    }

    #[test]
    fn borra_una_empresa_que_no_esta_abierta() {
        let env = Env::new("borrar");
        let s = env.open();
        // La única no se puede borrar.
        assert!(
            s.delete_company(companies::PRINCIPAL, "Mi empresa")
                .is_err()
        );
        let b = s
            .create_company(&info("EMPRESA DEMO SL", Some("B12345674")), None)
            .unwrap();
        // Sin escribir su nombre o NIF no se borra.
        assert!(s.delete_company(&b.id, "EMPRESA").is_err());
        assert_eq!(s.companies().unwrap().len(), 2);
        assert!(!s.delete_company(&b.id, "b12345674").unwrap());
        assert!(env.bin().join(&b.id).join("taxpain.db").is_file());
        let list = s.companies().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, companies::PRINCIPAL);
        assert_eq!(s.active_id().unwrap(), companies::PRINCIPAL);
        // Con el id de una lista vieja no se vuelve a crear vacía.
        assert!(s.open_company(&b.id).is_err());
        assert!(!env.root().join("companies").join(&b.id).exists());
    }

    #[test]
    fn borrar_la_abierta_abre_antes_otra() {
        let env = Env::new("abierta");
        let s = env.open();
        let b = s
            .create_company(&info("EMPRESA DEMO SL", None), None)
            .unwrap();
        s.switch_company(&b.id).unwrap();
        assert!(s.delete_company(&b.id, "empresa demo sl").unwrap());
        assert_eq!(s.active_id().unwrap(), companies::PRINCIPAL);
        assert_eq!(companies::active(&env.root()), companies::PRINCIPAL);
        assert!(env.bin().join(&b.id).is_dir());
        assert_eq!(s.companies().unwrap().len(), 1);
    }

    #[test]
    fn si_la_papelera_falla_no_cambia_nada() {
        let env = Env::new("falla");
        let s = env.open_with_trash(Arc::new(|_: &Path| Err("sin permiso".to_owned())));
        let b = s
            .create_company(&info("EMPRESA DEMO SL", None), None)
            .unwrap();
        s.switch_company(&b.id).unwrap();
        let e = s.delete_company(&b.id, "EMPRESA DEMO SL").unwrap_err();
        assert!(e.message.contains("Papelera"), "{}", e.message);
        assert_eq!(s.active_id().unwrap(), b.id);
        assert_eq!(s.companies().unwrap().len(), 2);
    }

    #[test]
    fn borrar_la_principal_no_la_vuelve_a_crear() {
        let env = Env::new("principal");
        let b = {
            let s = env.open();
            let b = s
                .create_company(&info("EMPRESA DEMO SL", Some("B12345674")), None)
                .unwrap();
            assert!(
                s.delete_company(companies::PRINCIPAL, "Mi empresa")
                    .unwrap()
            );
            assert_eq!(s.active_id().unwrap(), b.id);
            assert!(env.bin().join("data").join("taxpain.db").is_file());
            b
        };
        // Al volver a abrir la app no aparece una «Mi empresa» vacía.
        let s = env.open();
        assert_eq!(s.active_id().unwrap(), b.id);
        let list = s.companies().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "EMPRESA DEMO SL");
        assert!(!env.root().join("data").exists());
        assert!(s.open_company(companies::PRINCIPAL).is_err());
        assert!(!env.root().join("data").exists());
        // Y la que queda no se puede borrar.
        assert!(s.delete_company(&b.id, "EMPRESA DEMO SL").is_err());
    }
}
