//! Motor de calendario fiscal (Fase 2): obligaciones por perfil, plazos con traslado a día
//! hábil, avisos y exportación iCal. Ver `docs/CALENDAR_RULES.md`.

pub mod business;
pub mod holidays;
pub mod ical;
pub mod official;
pub mod reminders;
pub mod rules;

pub use business::HolidayCalendar;
pub use holidays::{Ccaa, calendar_for};
pub use reminders::{DEFAULT_OFFSETS, Reminder, notification_text, reminders};
pub use rules::{
    Deadline, IrpfMethod, Model, Period, Periodicity, Persona, TaxpayerProfile, VatProfile,
    deadlines_in_year,
};
