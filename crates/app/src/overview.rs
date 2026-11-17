//! Resumen de una empresa para la vista de todas las empresas (gestorías o quien lleva varias):
//! próximos plazos y los vencidos sin presentar. Cuenta como presentado lo presentado desde
//! TaxPain y lo que la AEAT tiene según la última «Comprobar lo presentado» (también lo de la
//! gestoría).

use serde::Serialize;
use taxpain_core::calendar::{self, Period, TaxpayerProfile};
use taxpain_vault::Vault;
use taxpain_vault::rusqlite::params;
use time::{Date, Duration};

use crate::error::Result;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DeadlineStatus {
    pub model: String,
    /// Para la pantalla: «Modelo 303 · 3.er trimestre 2026», «Cuota de autónomos · octubre 2026».
    pub label: String,
    pub description: String,
    pub period: String,
    pub fiscal_year: i32,
    pub due_on: Date,
    /// Presentado en producción desde TaxPain o, según la última comprobación con la AEAT,
    /// presentado por cualquier vía (`None`: no es un modelo de la AEAT).
    pub filed: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CompanyStatus {
    /// Sin perfil fiscal no hay plazos que calcular.
    pub has_profile: bool,
    /// Vencidos en los últimos 90 días y no presentados (ni desde TaxPain ni, según la última
    /// comprobación, en la AEAT).
    pub overdue: Vec<DeadlineStatus>,
    /// Los que vencen en los próximos 45 días.
    pub upcoming: Vec<DeadlineStatus>,
    /// Última comprobación de lo presentado con la AEAT (`None`: nunca): sin ella, lo
    /// presentado por otra vía (la gestoría) sale como vencido.
    pub aeat_checked_at: Option<String>,
}

const ISO_DATE: &[time::format_description::BorrowedFormatItem<'_>] =
    time::macros::format_description!("[year]-[month]-[day]");

const MONTHS: [&str; 12] = [
    "enero",
    "febrero",
    "marzo",
    "abril",
    "mayo",
    "junio",
    "julio",
    "agosto",
    "septiembre",
    "octubre",
    "noviembre",
    "diciembre",
];

/// El periodo en palabras: «1.er trimestre», «octubre», «anual», «2.º pago».
fn period_label(p: Period, year: i32) -> String {
    match p {
        Period::Quarter(q) => {
            let ord = if q == 1 || q == 3 { "er" } else { "º" };
            format!("{q}.{ord} trimestre {year}")
        }
        Period::Month(m) => {
            let name = MONTHS
                .get(usize::from(m).wrapping_sub(1))
                .copied()
                .unwrap_or("mes");
            format!("{name} {year}")
        }
        Period::Annual => format!("ejercicio {year}"),
        Period::Installment(n) => format!("{n}.º pago {year}"),
    }
}

/// Día en que se empezó a usar TaxPain con esta empresa (la primera anotación de la auditoría).
fn first_use(v: &Vault) -> Result<Option<Date>> {
    let at: Option<String> =
        v.connection()
            .query_row("SELECT min(at) FROM audit_log", [], |r| r.get(0))?;
    Ok(at.and_then(|a| Date::parse(a.get(..10)?, ISO_DATE).ok()))
}

fn filed(v: &Vault, model: &str, year: i32, period: &str) -> Result<Option<bool>> {
    if !model.chars().all(|c| c.is_ascii_digit()) {
        return Ok(None);
    }
    let n: i64 = v.connection().query_row(
        "SELECT COUNT(*) FROM tax_return WHERE model = ?1 AND year = ?2 AND period = ?3
           AND environment = 'produccion'",
        params![model, year, period],
        |r| r.get(0),
    )?;
    Ok(Some(
        n > 0 || crate::presentado_aeat::filed_at_aeat(v, model, year, period)?,
    ))
}

pub fn company_status(v: &Vault, today: Date) -> Result<CompanyStatus> {
    let profile: Option<TaxpayerProfile> = v.setting("taxpayer_profile")?;
    let Some(profile) = profile else {
        return Ok(CompanyStatus {
            has_profile: false,
            overdue: Vec::new(),
            upcoming: Vec::new(),
            aeat_checked_at: crate::presentado_aeat::last_check(v)?,
        });
    };
    let mut all = Vec::new();
    for y in [today.year() - 1, today.year(), today.year() + 1] {
        let (h, _) = calendar::calendar_for(y, profile.ccaa, &profile.festivos_locales);
        all.extend(calendar::deadlines_in_year(&profile, y, &h));
    }
    all.sort_by_key(|d| d.due_on);
    // Lo que venció antes de empezar a usar TaxPain con la empresa no se marca como vencido
    // sin presentar: se presentó por otro lado (una empresa dada de alta hoy no sale con todo
    // el trimestre anterior en rojo). Si el reloj no cuadra (fecha futura), no se aplica.
    let since = first_use(v)?.filter(|d| *d <= today);
    let mut overdue = Vec::new();
    let mut upcoming = Vec::new();
    for d in all {
        let window_past = d.due_on < today
            && d.due_on >= today - Duration::days(90)
            && since.is_none_or(|s| d.due_on >= s);
        let window_next = d.due_on >= today && d.due_on <= today + Duration::days(45);
        if !window_past && !window_next {
            continue;
        }
        let period = d.period.code();
        let status = DeadlineStatus {
            model: d.model.code().to_owned(),
            label: format!(
                "{} · {}",
                d.model.title(),
                period_label(d.period, d.fiscal_year)
            ),
            description: d.model.description().to_owned(),
            filed: filed(v, d.model.code(), d.fiscal_year, &period)?,
            period,
            fiscal_year: d.fiscal_year,
            due_on: d.due_on,
        };
        if window_past {
            if status.filed == Some(false) {
                overdue.push(status);
            }
        } else {
            upcoming.push(status);
        }
    }
    Ok(CompanyStatus {
        has_profile: true,
        overdue,
        upcoming,
        aeat_checked_at: crate::presentado_aeat::last_check(v)?,
    })
}
