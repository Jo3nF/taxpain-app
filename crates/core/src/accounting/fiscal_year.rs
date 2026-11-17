//! Ejercicios contables, naturales o no (p. ej. del 1 de julio al 30 de junio).
//!
//! Un ejercicio se identifica por el **año en que empieza**, como el modelo 200 («períodos
//! impositivos iniciados en 2026») y el calendario fiscal. Las fechas reales van siempre con él:
//! donde la norma pide una fecha (cierre, apertura, prorrateos, plazos) se usan las fechas, no
//! el número. Si la sociedad cambia la fecha de cierre, el ejercicio de transición (más corto)
//! se guarda con sus propias fechas y el número libre siguiente.

use serde::{Deserialize, Serialize};
use time::{Date, Duration, Month};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FiscalYear {
    /// Número del ejercicio (año en que empieza, salvo un ejercicio de transición).
    pub year: i32,
    pub start: Date,
    /// Último día (fecha de cierre).
    pub end: Date,
}

impl FiscalYear {
    /// Ejercicio natural (1 de enero a 31 de diciembre).
    pub fn natural(year: i32) -> Self {
        Self::numbered(year, (Month::December, 31))
    }

    /// Ejercicio número `year` con cierre anual en `close` (mes, día): el que empieza en `year`.
    pub fn numbered(year: i32, close: (Month, u8)) -> Self {
        let prev = close_on(year - 1, close) + Duration::days(1);
        let start = if prev.year() == year {
            prev
        } else {
            close_on(year, close) + Duration::days(1)
        };
        Self::containing(start, close)
    }

    /// Ejercicio con cierre anual en `close` que contiene `date`.
    pub fn containing(date: Date, close: (Month, u8)) -> Self {
        let c = close_on(date.year(), close);
        let (start, end) = if date <= c {
            (close_on(date.year() - 1, close) + Duration::days(1), c)
        } else {
            (c + Duration::days(1), close_on(date.year() + 1, close))
        };
        FiscalYear {
            year: start.year(),
            start,
            end,
        }
    }

    pub fn contains(&self, d: Date) -> bool {
        self.start <= d && d <= self.end
    }

    pub fn is_natural(&self) -> bool {
        self.start.month() == Month::January
            && self.start.day() == 1
            && self.end.month() == Month::December
            && self.end.day() == 31
            && self.start.year() == self.end.year()
    }

    /// Días del ejercicio.
    pub fn days(&self) -> i64 {
        (self.end - self.start).whole_days() + 1
    }

    /// Días de un año completo que acaba en `end` (365 o 366): base de los prorrateos.
    pub fn full_year_days(&self) -> i64 {
        let back = Date::from_calendar_date(
            self.end.year() - 1,
            self.end.month(),
            self.end
                .day()
                .min(self.end.month().length(self.end.year() - 1)),
        )
        .unwrap_or(self.end);
        (self.end - back).whole_days()
    }

    /// «2026» o «2026/2027» (si cruza el año natural).
    pub fn label(&self) -> String {
        if self.start.year() == self.end.year() {
            self.year.to_string()
        } else {
            format!("{}/{}", self.start.year(), self.end.year())
        }
    }
}

/// Fecha de cierre `close` (mes, día) del año `year`; el día se ajusta al último del mes
/// (cierre a 29 de febrero en años no bisiestos → 28).
pub fn close_on(year: i32, close: (Month, u8)) -> Date {
    let (m, d) = close;
    let d = d.clamp(1, m.length(year));
    Date::from_calendar_date(year, m, d).unwrap_or(Date::MIN)
}

/// Comprueba una fecha de cierre (mes, día) introducida por el usuario.
pub fn valid_close(close: (Month, u8)) -> bool {
    let (m, d) = close;
    // Febrero admite 29 (años bisiestos).
    d >= 1 && d <= m.length(2024)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    const JUNIO: (Month, u8) = (Month::June, 30);

    #[test]
    fn natural() {
        let f = FiscalYear::natural(2026);
        assert_eq!(
            (f.start, f.end),
            (date!(2026 - 01 - 01), date!(2026 - 12 - 31))
        );
        assert!(f.is_natural());
        assert_eq!(f.label(), "2026");
        assert_eq!(f.full_year_days(), 365);
        assert_eq!(FiscalYear::natural(2028).full_year_days(), 366);
        assert_eq!(
            FiscalYear::containing(date!(2026 - 12 - 31), (Month::December, 31)),
            f
        );
    }

    #[test]
    fn julio_a_junio() {
        let f = FiscalYear::containing(date!(2027 - 03 - 15), JUNIO);
        assert_eq!(f.year, 2026);
        assert_eq!(
            (f.start, f.end),
            (date!(2026 - 07 - 01), date!(2027 - 06 - 30))
        );
        assert_eq!(FiscalYear::numbered(2026, JUNIO), f);
        assert_eq!(FiscalYear::containing(date!(2027 - 06 - 30), JUNIO), f);
        assert_eq!(
            FiscalYear::containing(date!(2027 - 07 - 01), JUNIO).year,
            2027
        );
        assert_eq!(f.label(), "2026/2027");
        assert!(!f.is_natural());
        assert_eq!(f.days(), 365);
    }

    #[test]
    fn febrero_y_cierres_raros() {
        let feb = (Month::February, 29);
        let f = FiscalYear::containing(date!(2027 - 06 - 01), feb);
        assert_eq!(
            (f.start, f.end),
            (date!(2027 - 03 - 01), date!(2028 - 02 - 29))
        );
        assert_eq!(f.full_year_days(), 366);
        let g = FiscalYear::numbered(2028, feb);
        assert_eq!(
            (g.start, g.end),
            (date!(2028 - 03 - 01), date!(2029 - 02 - 28))
        );
        // Cierre a 31 de marzo: el ejercicio 2026 empieza el 1 de abril de 2026.
        let m = FiscalYear::numbered(2026, (Month::March, 31));
        assert_eq!(
            (m.start, m.end),
            (date!(2026 - 04 - 01), date!(2027 - 03 - 31))
        );
        assert!(valid_close(feb));
        assert!(!valid_close((Month::April, 31)));
        assert!(!valid_close((Month::May, 0)));
    }
}
