//! Avisos antes de cada vencimiento: 15, 7, 2 y 0 días naturales antes del último día real (o
//! los propios del trámite, `Model::reminder_offsets`).
//! Si el aviso cae en inhábil se adelanta al hábil anterior, para que llegue en día de trabajo.

use serde::Serialize;
use time::{Date, Duration};

use super::business::HolidayCalendar;
use super::rules::Deadline;

pub const DEFAULT_OFFSETS: [i64; 4] = [15, 7, 2, 0];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Reminder {
    pub date: Date,
    /// Días naturales que faltan hasta `deadline.due_on`.
    pub days_before: i64,
    /// Días hábiles que faltan (lo que de verdad queda para preparar la declaración).
    pub business_days_before: u32,
    pub deadline: Deadline,
}

fn roll_back(holidays: &HolidayCalendar, mut d: Date) -> Date {
    while !holidays.is_business_day(d) {
        d -= Duration::days(1);
    }
    d
}

/// Avisos de `deadlines` con fecha en `[from, to]`, ordenados por fecha.
pub fn reminders(
    deadlines: &[Deadline],
    offsets: &[i64],
    holidays: &HolidayCalendar,
    from: Date,
    to: Date,
) -> Vec<Reminder> {
    let mut out = Vec::new();
    for dl in deadlines {
        let mut seen = Vec::new();
        for &off in dl.model.reminder_offsets().unwrap_or(offsets) {
            let mut date = dl.due_on - Duration::days(off);
            if off > 0 {
                date = roll_back(holidays, date).max(dl.starts_on.min(date));
            }
            if seen.contains(&date) || date < from || date > to {
                continue;
            }
            seen.push(date);
            out.push(Reminder {
                date,
                days_before: (dl.due_on - date).whole_days(),
                business_days_before: holidays.business_days_between(date, dl.due_on),
                deadline: dl.clone(),
            });
        }
    }
    out.sort_by_key(|r| (r.date, r.deadline.due_on));
    out
}

/// Texto de la notificación, en lenguaje llano.
pub fn notification_text(r: &Reminder) -> (String, String) {
    let d = &r.deadline;
    let title = format!(
        "{} · {} {}",
        d.model.title(),
        d.period.code(),
        d.fiscal_year
    );
    let when = match r.days_before {
        0 => "vence hoy".to_owned(),
        1 => "vence mañana".to_owned(),
        n => format!("vence en {n} días ({} hábiles)", r.business_days_before),
    };
    let body = format!(
        "{}: {when}, el {}.",
        d.model.description(),
        format_es(d.due_on)
    );
    (title, body)
}

pub fn format_es(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), d.month() as u8, d.year())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::rules::{TaxpayerProfile, deadlines_in_year};
    use time::macros::date;

    #[test]
    fn avisos_del_303_1t_2026() {
        let cal = HolidayCalendar::default();
        let ds: Vec<_> = deadlines_in_year(&TaxpayerProfile::autonomo(), 2026, &cal)
            .into_iter()
            .filter(|d| d.model.code() == "303" && d.period.code() == "1T" && d.fiscal_year == 2026)
            .collect();
        let rs = reminders(
            &ds,
            &DEFAULT_OFFSETS,
            &cal,
            date!(2026 - 01 - 01),
            date!(2026 - 12 - 31),
        );
        let dates: Vec<_> = rs.iter().map(|r| r.date).collect();
        // Vence el lunes 20/04/2026. −15 = domingo 5 → viernes 3; −7 = lunes 13; −2 = sábado 18
        // → viernes 17; 0 = lunes 20.
        assert_eq!(
            dates,
            [
                date!(2026 - 04 - 03),
                date!(2026 - 04 - 13),
                date!(2026 - 04 - 17),
                date!(2026 - 04 - 20)
            ]
        );
        assert_eq!(rs[1].business_days_before, 5);
        let (title, body) = notification_text(&rs[1]);
        assert_eq!(title, "Modelo 303 · 1T 2026");
        assert_eq!(
            body,
            "IVA: declaración del periodo: vence en 7 días (5 hábiles), el 20/04/2026."
        );
        assert_eq!(
            notification_text(&rs[3]).1,
            "IVA: declaración del periodo: vence hoy, el 20/04/2026."
        );
    }
}
