//! Días hábiles para plazos tributarios.
//!
//! Regla: si el último día de un plazo es inhábil, se prorroga al primer día hábil siguiente
//! (Ley 39/2015, art. 30.5, aplicable supletoriamente a los procedimientos tributarios por su
//! disp. adicional 1.ª.2.a y el art. 7.2 de la LGT). Son inhábiles los sábados, los domingos y
//! los festivos (Ley 39/2015, art. 30.2), también los autonómicos y locales del domicilio del
//! contribuyente (art. 30.6; AEAT, «Vencimientos en días inhábiles»): `holidays.rs` los carga y
//! `HolidayCalendar::with_extra` permite añadir otros. Ver `docs/CALENDAR_RULES.md`.

use std::collections::BTreeSet;

use time::{Date, Duration, Weekday};

/// Conjunto de días festivos.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HolidayCalendar {
    days: BTreeSet<Date>,
}

impl HolidayCalendar {
    pub fn new(days: impl IntoIterator<Item = Date>) -> Self {
        HolidayCalendar {
            days: days.into_iter().collect(),
        }
    }

    /// Añade festivos (p. ej. autonómicos o locales).
    pub fn with_extra(mut self, days: impl IntoIterator<Item = Date>) -> Self {
        self.days.extend(days);
        self
    }

    pub fn is_holiday(&self, d: Date) -> bool {
        self.days.contains(&d)
    }

    /// Hábil = de lunes a viernes y no festivo.
    pub fn is_business_day(&self, d: Date) -> bool {
        !matches!(d.weekday(), Weekday::Saturday | Weekday::Sunday) && !self.is_holiday(d)
    }

    /// `d` si es hábil; si no, el primer día hábil posterior.
    pub fn roll_forward(&self, mut d: Date) -> Date {
        while !self.is_business_day(d) {
            d += Duration::days(1);
        }
        d
    }

    /// `d` si es hábil; si no, el último día hábil anterior.
    pub fn roll_back(&self, mut d: Date) -> Date {
        while !self.is_business_day(d) {
            d -= Duration::days(1);
        }
        d
    }

    /// Días hábiles en `(from, to]` (0 si `to <= from`).
    pub fn business_days_between(&self, from: Date, to: Date) -> u32 {
        let mut n = 0;
        let mut d = from;
        while d < to {
            d += Duration::days(1);
            if self.is_business_day(d) {
                n += 1;
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn roll_forward_skips_weekends_and_holidays() {
        // 2026-08-15 es sábado; el lunes 17 es hábil.
        let cal = HolidayCalendar::new([date!(2026 - 08 - 15)]);
        assert_eq!(
            cal.roll_forward(date!(2026 - 08 - 15)),
            date!(2026 - 08 - 17)
        );
        // Viernes festivo → lunes.
        let cal = HolidayCalendar::new([date!(2026 - 05 - 01)]);
        assert_eq!(
            cal.roll_forward(date!(2026 - 05 - 01)),
            date!(2026 - 05 - 04)
        );
        // Día hábil no se mueve.
        assert_eq!(
            cal.roll_forward(date!(2026 - 04 - 20)),
            date!(2026 - 04 - 20)
        );
    }

    #[test]
    fn business_days_between_counts_open_interval() {
        let cal = HolidayCalendar::default();
        // Del viernes 10/04/2026 al lunes 13: sólo el lunes.
        assert_eq!(
            cal.business_days_between(date!(2026 - 04 - 10), date!(2026 - 04 - 13)),
            1
        );
        assert_eq!(
            cal.business_days_between(date!(2026 - 04 - 13), date!(2026 - 04 - 13)),
            0
        );
        assert_eq!(
            cal.business_days_between(date!(2026 - 04 - 13), date!(2026 - 04 - 20)),
            5
        );
    }
}
