//! Calendario fiscal en la app: perfil, vencimientos, export iCal y avisos del sistema.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_notification::NotificationExt;
use taxpain_core::calendar::{
    self, Ccaa, DEFAULT_OFFSETS, Deadline, TaxpayerProfile, calendar_for, notification_text,
};
use taxpain_vault::Vault;
use time::{Date, OffsetDateTime};

use crate::state::AppState;
use crate::{CmdResult, CommandError};

const PROFILE_KEY: &str = "taxpayer_profile";

/// Hoy en la España peninsular, que es la fecha que rige los plazos: con el Mac en otro huso
/// (o entre las 00:00 y las 02:00 españolas en UTC) la fecha local no es la de la AEAT. Es el
/// «hoy» de todos los comandos que lo necesitan (plazos, ejercicio en curso, previsión).
pub(crate) fn today() -> Date {
    today_at(OffsetDateTime::now_utc())
}

fn today_at(now: OffsetDateTime) -> Date {
    taxpain_core::spain_time::peninsular_date(now)
}

/// Vencimiento aplanado para el WebView.
#[derive(Serialize)]
pub struct DeadlineView {
    model: &'static str,
    /// «Modelo 303», «Cuota de autónomos»…
    title: String,
    description: String,
    period: String,
    fiscal_year: i32,
    starts_on: Date,
    nominal_end: Date,
    due_on: Date,
    domiciliation_end: Option<Date>,
    domiciliation_official: bool,
    tentative: bool,
    rule_ref: &'static str,
}

impl From<&Deadline> for DeadlineView {
    fn from(d: &Deadline) -> Self {
        DeadlineView {
            model: d.model.code(),
            title: d.model.title(),
            description: d.model.description().into(),
            period: d.period.code(),
            fiscal_year: d.fiscal_year,
            starts_on: d.starts_on,
            nominal_end: d.nominal_end,
            due_on: d.due_on,
            domiciliation_end: d.domiciliation_end,
            domiciliation_official: d.domiciliation_official,
            tentative: d.tentative,
            rule_ref: d.rule_ref,
        }
    }
}

/// Descripción del 202 según la modalidad del ejercicio.
fn m202_description(base: bool) -> &'static str {
    if base {
        "Impuesto sobre Sociedades: pago fraccionado por la base del periodo (art. 40.3 LIS)"
    } else {
        "Impuesto sobre Sociedades: pago fraccionado por la cuota del último IS (art. 40.2 LIS)"
    }
}

#[derive(Serialize)]
pub struct CalendarView {
    year: i32,
    today: Date,
    /// `false` si faltan los festivos oficiales del año: los traslados pueden no ser exactos.
    holidays_loaded: bool,
    /// `false` si falta el calendario del contribuyente de la AEAT del año: domiciliación y
    /// campaña de Renta estimadas.
    aeat_calendar_loaded: bool,
    /// Festivos locales del año en el perfil (0: pendientes de publicar o de añadir).
    local_holidays: usize,
    deadlines: Vec<DeadlineView>,
}

/// Perfiles de partida para el formulario.
#[tauri::command]
pub fn profile_presets() -> Vec<(&'static str, TaxpayerProfile)> {
    vec![
        ("Autónomo (estimación directa)", TaxpayerProfile::autonomo()),
        ("Sociedad limitada", TaxpayerProfile::sociedad_limitada()),
    ]
}

/// Comunidades autónomas para el selector (código serde, nombre).
#[tauri::command]
pub fn ccaa_list() -> Vec<(Ccaa, &'static str)> {
    Ccaa::ALL.iter().map(|c| (*c, c.name())).collect()
}

#[tauri::command]
pub fn get_profile(state: tauri::State<'_, AppState>) -> CmdResult<Option<TaxpayerProfile>> {
    Ok(state.vault()?.setting(PROFILE_KEY)?)
}

/// El perfil fiscal deducido de lo presentado (para «Rellenar con lo presentado»): no se guarda.
#[tauri::command]
pub fn deduced_profile(
    state: tauri::State<'_, AppState>,
) -> CmdResult<Option<taxpain_app::perfil_deducido::PerfilDeducido>> {
    Ok(taxpain_app::perfil_deducido::deducir(&*state.vault()?)?)
}

#[tauri::command]
pub fn set_profile(state: tauri::State<'_, AppState>, profile: TaxpayerProfile) -> CmdResult<()> {
    Ok(state.vault()?.set_setting(PROFILE_KEY, &profile)?)
}

#[tauri::command]
pub fn fiscal_calendar(state: tauri::State<'_, AppState>, year: i32) -> CmdResult<CalendarView> {
    let (deadlines, holidays_loaded) = deadlines_for(&state, year)?;
    let vault = state.vault()?;
    let deadlines = deadlines
        .iter()
        .map(|d| {
            let mut view = DeadlineView::from(d);
            // El 202, con la modalidad de su ejercicio (art. 40.2 o 40.3 LIS).
            if d.model == calendar::Model::M202 {
                view.description = m202_description(
                    taxpain_app::models::pago202::modalidad(&vault, d.fiscal_year)
                        .is_ok_and(|m| m.efectiva == taxpain_app::models::pago202::Modalidad::Base),
                )
                .into();
            }
            view
        })
        .collect::<Vec<_>>();
    // Los cambios censales apuntados desde Calendario → Censo (un mes desde el hecho, o la
    // ventana de la opción): su último día, hasta que se marcan como presentados.
    let mut deadlines = deadlines;
    for c in taxpain_app::censo::avisos_calendario(&vault, year)? {
        deadlines.push(DeadlineView {
            model: "036-cambio",
            title: "Modelo 036".into(),
            description: if c.nota.is_empty() {
                "Censo: comunicar el cambio en el 036 (apuntado en Calendario → Censo)".into()
            } else {
                format!("Censo: comunicar el cambio en el 036. {}", c.nota)
            },
            period: c.titulo,
            fiscal_year: c.vence.year(),
            starts_on: c.fecha_hecho.unwrap_or(c.vence).min(c.vence),
            nominal_end: c.vence,
            due_on: c.vence,
            domiciliation_end: None,
            domiciliation_official: false,
            tentative: false,
            rule_ref: "RGAT arts. 9 a 14 (declaración censal, modelo 036)",
        });
    }
    deadlines.sort_by_key(|d| d.due_on);
    let local_holidays = vault
        .setting::<TaxpayerProfile>(PROFILE_KEY)?
        .map_or(0, |p| {
            p.festivos_locales
                .iter()
                .filter(|d| d.year() == year)
                .count()
        });
    Ok(CalendarView {
        year,
        today: today(),
        holidays_loaded,
        aeat_calendar_loaded: calendar::official::calendario_cargado(year),
        local_holidays,
        deadlines,
    })
}

/// Qué datos del año tiene TaxPain (festivos, calendario de la AEAT, diseños, parámetros) y de
/// dónde salen.
#[tauri::command]
pub fn vigencia_ejercicio(
    state: tauri::State<'_, AppState>,
    year: i32,
) -> CmdResult<taxpain_app::cambio_ejercicio::Vigencia> {
    let perfil: Option<TaxpayerProfile> = state.vault()?.setting(PROFILE_KEY)?;
    Ok(taxpain_app::cambio_ejercicio::vigencia(
        perfil.as_ref(),
        year,
    ))
}

/// Aviso del Panel en diciembre y enero: lo que hay que revisar del año nuevo.
#[tauri::command]
pub fn aviso_cambio_ejercicio(
    state: tauri::State<'_, AppState>,
) -> CmdResult<Option<taxpain_app::cambio_ejercicio::Vigencia>> {
    Ok(taxpain_app::cambio_ejercicio::aviso_cambio_de_año(
        &*state.vault()?,
        today(),
    )?)
}

fn deadlines_for(state: &AppState, year: i32) -> CmdResult<(Vec<Deadline>, bool)> {
    let profile: TaxpayerProfile = state
        .vault()?
        .setting(PROFILE_KEY)?
        .ok_or_else(|| CommandError::from("configura primero tu perfil fiscal"))?;
    let (h, holidays_loaded) = calendar_for(year, profile.ccaa, &profile.festivos_locales);
    Ok((
        calendar::deadlines_in_year(&profile, year, &h),
        holidays_loaded,
    ))
}

/// Avisos de umbral con los datos de la empresa (347, 349, gran empresa, 720, 232, IS, IAE).
#[tauri::command]
pub fn threshold_alerts(
    state: tauri::State<'_, AppState>,
) -> CmdResult<Vec<taxpain_app::umbrales::ThresholdAlert>> {
    Ok(taxpain_app::umbrales::alerts(&*state.vault()?, today())?)
}

/// Escribe el calendario del año en `path` (elegido con el diálogo de guardar). El id de la
/// empresa va en los UID (dos empresas en el mismo calendario no se pisan) y, si hay varias,
/// su nombre encabeza cada evento.
#[tauri::command]
pub fn export_ical(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    year: i32,
    path: String,
) -> CmdResult<()> {
    let path = crate::salida::salida_permitida(&app, &path, &["ics"])?;
    let (deadlines, _) = deadlines_for(&state, year)?;
    let company_id = state.active_id()?;
    let company_name = if state.several_companies() {
        Some(taxpain_app::companies::info(&*state.vault()?)?.name)
    } else {
        None
    };
    let ics = calendar::ical::to_ical(
        &deadlines,
        OffsetDateTime::now_utc(),
        &company_id,
        company_name.as_deref(),
    );
    crate::salida::escribir(&path, ics)?;
    Ok(())
}

/// Revisa cada 30 minutos los avisos de hoy y muestra los que no se han enviado. Funciona con la
/// ventana cerrada (la app sigue en el Dock) y, si el usuario lo activa, desde el inicio de
/// sesión (`background.rs`).
pub fn spawn_reminder_loop(app: AppHandle) {
    std::thread::spawn(move || {
        loop {
            if let Err(e) = send_due_reminders(&app) {
                tracing::warn!(error = %e.message, "no se pudieron revisar los avisos");
            }
            std::thread::sleep(Duration::from_secs(30 * 60));
        }
    });
}

/// Avisos pendientes de todas las empresas (con su nombre delante si hay varias).
fn send_due_reminders(app: &AppHandle) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let several = state.several_companies();
    state.for_each_company(|_, _, vault| {
        let prefix = if several {
            format!("{} · ", taxpain_app::companies::info(vault)?.name)
        } else {
            String::new()
        };
        send_company_reminders(app, vault, &prefix)
    })
}

fn send_company_reminders(app: &AppHandle, vault: &Vault, prefix: &str) -> CmdResult<()> {
    let day = today();
    // Notificaciones de la AEAT sin abrir (las ya consultadas; no hace falta red).
    // Plazos de los expedientes y certificados que caducan.
    let mut sueltos = taxpain_app::notices::reminders_due(vault, day)?;
    sueltos.extend(taxpain_app::expedientes::reminders_due(vault, day)?);
    for r in sueltos {
        if vault.mark_reminder_sent(&r.key, OffsetDateTime::now_utc())? {
            app.notification()
                .builder()
                .title(format!("{prefix}{}", r.title))
                .body(r.body)
                .show()?;
        }
    }
    let Some(profile) = vault.setting::<TaxpayerProfile>(PROFILE_KEY)? else {
        return Ok(());
    };
    // Los avisos de enero pueden corresponder a vencimientos del año siguiente.
    for year in [day.year(), day.year() + 1] {
        let (h, _) = calendar_for(year, profile.ccaa, &profile.festivos_locales);
        let deadlines = calendar::deadlines_in_year(&profile, year, &h);
        for r in calendar::reminders(&deadlines, &DEFAULT_OFFSETS, &h, day, day) {
            let d = &r.deadline;
            let key = format!(
                "{}|{}|{}|{}",
                d.model.code(),
                d.fiscal_year,
                d.period.code(),
                r.date
            );
            if vault.mark_reminder_sent(&key, OffsetDateTime::now_utc())? {
                let (title, body) = notification_text(&r);
                app.notification()
                    .builder()
                    .title(format!("{prefix}{title}"))
                    .body(body)
                    .show()?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::{Month, Time};

    #[test]
    fn hoy_es_la_fecha_peninsular() {
        // 20/04/2027 a las 22:30 UTC (19:30 en Nueva York) ya es 21 de abril en Madrid (+2).
        let utc = |d: u8, h: u8| {
            OffsetDateTime::new_utc(
                Date::from_calendar_date(2027, Month::April, d).unwrap(),
                Time::from_hms(h, 30, 0).unwrap(),
            )
        };
        assert_eq!(
            today_at(utc(20, 22)),
            Date::from_calendar_date(2027, Month::April, 21).unwrap()
        );
        assert_eq!(
            today_at(utc(20, 12)),
            Date::from_calendar_date(2027, Month::April, 20).unwrap()
        );
        // El «hoy» compartido por los comandos (ejercicio en curso, previsión, resumen de
        // empresas) es el mismo que el del calendario: nunca el reloj local del Mac.
        assert_eq!(today(), today_at(OffsetDateTime::now_utc()));
    }
}
