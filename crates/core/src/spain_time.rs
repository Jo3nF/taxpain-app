//! Hora oficial peninsular (CET/CEST) sin base de datos de zonas horarias.
//!
//! Regla de la UE (Directiva 2000/84/CE): horario de verano desde las 01:00 UTC del último
//! domingo de marzo hasta las 01:00 UTC del último domingo de octubre. Canarias va una hora por
//! detrás; para fechas de documentos se usa la peninsular.

use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time, UtcOffset, Weekday};

fn last_sunday(year: i32, month: Month) -> Option<Date> {
    let mut d = Date::from_calendar_date(year, month, month.length(year)).ok()?;
    while d.weekday() != Weekday::Sunday {
        d = d.previous_day()?;
    }
    Some(d)
}

/// Desfase de la hora peninsular en ese instante: +1 (invierno) o +2 (verano).
pub fn peninsular_offset(t: OffsetDateTime) -> UtcOffset {
    let utc = t.to_offset(UtcOffset::UTC);
    let at_1 = |d: Date| {
        PrimitiveDateTime::new(d, Time::from_hms(1, 0, 0).unwrap_or(Time::MIDNIGHT)).assume_utc()
    };
    let summer = match (
        last_sunday(utc.year(), Month::March),
        last_sunday(utc.year(), Month::October),
    ) {
        (Some(start), Some(end)) => utc >= at_1(start) && utc < at_1(end),
        _ => false,
    };
    UtcOffset::from_hms(if summer { 2 } else { 1 }, 0, 0).unwrap_or(UtcOffset::UTC)
}

/// Fecha en la España peninsular de un instante.
pub fn peninsular_date(t: OffsetDateTime) -> Date {
    t.to_offset(peninsular_offset(t)).date()
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::datetime;

    #[test]
    fn cambio_de_hora_y_medianoche() {
        // Invierno: 23:30 UTC del 31/12 ya es 1 de enero en España.
        assert_eq!(
            peninsular_date(datetime!(2026-12-31 23:30 UTC)),
            time::macros::date!(2027 - 01 - 01)
        );
        // Verano: 22:30 UTC del 14/10 ya es 15 de octubre (+2).
        assert_eq!(
            peninsular_date(datetime!(2026-10-14 22:30 UTC)),
            time::macros::date!(2026 - 10 - 15)
        );
        // 2026: el horario de verano empieza el 29/03 y termina el 25/10 a las 01:00 UTC.
        assert_eq!(
            peninsular_offset(datetime!(2026-03-29 00:59 UTC)).whole_hours(),
            1
        );
        assert_eq!(
            peninsular_offset(datetime!(2026-03-29 01:00 UTC)).whole_hours(),
            2
        );
        assert_eq!(
            peninsular_offset(datetime!(2026-10-25 00:59 UTC)).whole_hours(),
            2
        );
        assert_eq!(
            peninsular_offset(datetime!(2026-10-25 01:00 UTC)).whole_hours(),
            1
        );
    }
}
