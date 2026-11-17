//! Exportación iCalendar (RFC 5545): un evento de día completo por vencimiento, con alarmas.

use std::fmt::Write;

use time::{Date, Duration, OffsetDateTime};

use super::reminders::{DEFAULT_OFFSETS, format_es};
use super::rules::Deadline;

fn ical_date(d: Date) -> String {
    format!("{:04}{:02}{:02}", d.year(), d.month() as u8, d.day())
}

/// Texto de una propiedad TEXT (SUMMARY, DESCRIPTION, LOCATION…) según RFC 5545 §3.3.11:
/// `\\`, `;` y `,` se escapan; cualquier salto de línea (`\r\n`, `\r` o `\n`) pasa a `\\n`, y
/// los demás caracteres de control (que el RFC no admite en TEXT, salvo el tabulador) se quitan,
/// para que un nombre o un concepto no puedan cortar la línea y meter propiedades nuevas.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push_str("\\n");
            }
            '\n' => out.push_str("\\n"),
            '\t' => out.push('\t'),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Pliega líneas de más de 75 octetos (RFC 5545 §3.1) sin partir caracteres UTF-8.
fn fold(line: &str, out: &mut String) {
    let mut width = 0;
    for c in line.chars() {
        let len = c.len_utf8();
        if width + len > 75 {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(c);
        width += len;
    }
    out.push_str("\r\n");
}

/// Parte del UID que identifica la empresa: sólo letras, cifras, `-` y `_` (RFC 5545 §3.3.11).
fn uid_scope(company_id: &str) -> String {
    company_id
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Calendario con los vencimientos. `stamp` es el DTSTAMP (inyectado para tests reproducibles).
/// `company_id` entra en el UID de cada evento: la app es multiempresa y, si dos empresas se
/// importan al mismo calendario, un UID igual haría que la segunda pisara a la primera (RFC 5545
/// §3.8.4.7: el UID es único por evento). `company_name`, si se da (varias empresas), encabeza el
/// resumen.
pub fn to_ical(
    deadlines: &[Deadline],
    stamp: OffsetDateTime,
    company_id: &str,
    company_name: Option<&str>,
) -> String {
    let scope = uid_scope(company_id);
    let uid_prefix = if scope.is_empty() {
        String::new()
    } else {
        format!("{scope}-")
    };
    let stamp = format!(
        "{:04}{:02}{:02}T{:02}{:02}{:02}Z",
        stamp.year(),
        stamp.month() as u8,
        stamp.day(),
        stamp.hour(),
        stamp.minute(),
        stamp.second()
    );
    let mut lines = vec![
        "BEGIN:VCALENDAR".to_owned(),
        "VERSION:2.0".to_owned(),
        "PRODID:-//TaxPain//Calendario fiscal//ES".to_owned(),
        "CALSCALE:GREGORIAN".to_owned(),
        "METHOD:PUBLISH".to_owned(),
        "X-WR-CALNAME:TaxPain – calendario fiscal".to_owned(),
    ];
    for d in deadlines {
        let mut summary = format!(
            "{} {} {} – {}",
            d.model.title(),
            d.period.code(),
            d.fiscal_year,
            d.model.description()
        );
        if let Some(name) = company_name.map(str::trim).filter(|n| !n.is_empty()) {
            summary = format!("{name} · {summary}");
        }
        let mut desc = format!(
            "Plazo: del {} al {}.",
            format_es(d.starts_on),
            format_es(d.due_on)
        );
        if let Some(dom) = d.domiciliation_end {
            let _ = write!(desc, " Domiciliación hasta el {}.", format_es(dom));
        }
        if d.tentative {
            desc.push_str(" Fechas estimadas: pendientes de la norma anual.");
        }
        let _ = write!(desc, "\nNorma: {}.", d.rule_ref);
        lines.extend([
            "BEGIN:VEVENT".to_owned(),
            format!(
                "UID:{uid_prefix}{}-{}-{}@taxpain.es",
                d.model.code(),
                d.fiscal_year,
                d.period.code()
            ),
            format!("DTSTAMP:{stamp}"),
            format!("DTSTART;VALUE=DATE:{}", ical_date(d.due_on)),
            format!(
                "DTEND;VALUE=DATE:{}",
                ical_date(d.due_on + Duration::days(1))
            ),
            format!("SUMMARY:{}", escape(&summary)),
            format!("DESCRIPTION:{}", escape(&desc)),
            "TRANSP:TRANSPARENT".to_owned(),
        ]);
        for &off in d.model.reminder_offsets().unwrap_or(&DEFAULT_OFFSETS) {
            // Avisos a las 9:00 del día correspondiente (inicio del día + 9 h − N días).
            let trigger = if off == 0 {
                "PT9H".to_owned()
            } else {
                format!("-P{}DT15H", off - 1)
            };
            lines.extend([
                "BEGIN:VALARM".to_owned(),
                "ACTION:DISPLAY".to_owned(),
                format!("TRIGGER:{trigger}"),
                format!("DESCRIPTION:{}", escape(&summary)),
                "END:VALARM".to_owned(),
            ]);
        }
        lines.push("END:VEVENT".to_owned());
    }
    lines.push("END:VCALENDAR".to_owned());
    let mut out = String::new();
    for l in &lines {
        fold(l, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar::{HolidayCalendar, TaxpayerProfile, deadlines_in_year};
    use time::macros::datetime;

    #[test]
    fn ical_valido_y_estable() {
        let ds = deadlines_in_year(
            &TaxpayerProfile::autonomo(),
            2026,
            &HolidayCalendar::default(),
        );
        let ics = to_ical(&ds, datetime!(2026-10-13 10:00 UTC), "principal", None);
        assert!(ics.starts_with("BEGIN:VCALENDAR\r\n"));
        assert!(ics.ends_with("END:VCALENDAR\r\n"));
        assert_eq!(ics.matches("BEGIN:VEVENT").count(), ds.len());
        assert_eq!(ics.matches("BEGIN:VALARM").count(), ds.len() * 4);
        assert!(ics.contains("UID:principal-303-2026-1T@taxpain.es\r\n"));
        assert!(ics.contains("DTSTART;VALUE=DATE:20260420\r\n"));
        assert!(ics.contains("TRIGGER:-P14DT15H\r\n"));
        // Ninguna línea física supera 75 octetos y todas acaban en CRLF.
        for l in ics.split("\r\n").filter(|l| !l.is_empty()) {
            assert!(l.len() <= 75, "línea larga: {l}");
            assert!(!l.contains('\n'));
        }
        assert!(
            ics.contains("\\,") || !ics.contains(','),
            "comas sin escapar"
        );
        assert_eq!(
            ics,
            to_ical(&ds, datetime!(2026-10-13 10:00 UTC), "principal", None)
        );
    }

    #[test]
    fn dos_empresas_no_comparten_uid() {
        let ds = deadlines_in_year(
            &TaxpayerProfile::autonomo(),
            2026,
            &HolidayCalendar::default(),
        );
        let stamp = datetime!(2026-10-13 10:00 UTC);
        let a = to_ical(&ds, stamp, "principal", Some("EMPRESA DEMO SL"));
        let b = to_ical(&ds, stamp, "empresa b/2", Some("Otra SL"));
        assert!(a.contains("UID:principal-303-2026-1T@taxpain.es\r\n"));
        assert!(b.contains("UID:empresa-b-2-303-2026-1T@taxpain.es\r\n"));
        let uids = |s: &str| -> Vec<String> {
            s.lines()
                .filter(|l| l.starts_with("UID:"))
                .map(str::to_owned)
                .collect()
        };
        assert!(uids(&a).iter().all(|u| !uids(&b).contains(u)));
        // Con varias empresas, el nombre encabeza el resumen (comas y demás escapadas aparte).
        assert!(a.contains("SUMMARY:EMPRESA DEMO SL · Modelo 303 1T 2026"));
        // Sin empresa (id vacío) el UID es el de siempre.
        let c = to_ical(&ds, stamp, "", None);
        assert!(c.contains("UID:303-2026-1T@taxpain.es\r\n"));
    }

    #[test]
    fn fold_respeta_utf8() {
        let mut out = String::new();
        fold(&"ñ".repeat(60), &mut out);
        for l in out.split("\r\n").filter(|l| !l.is_empty()) {
            assert!(l.len() <= 75);
        }
        assert_eq!(out.replace("\r\n ", "").trim_end(), "ñ".repeat(60));
    }

    #[test]
    fn escape_rfc5545_y_sin_inyeccion_de_propiedades() {
        assert_eq!(escape("a\\b;c,d"), "a\\\\b\\;c\\,d");
        assert_eq!(
            escape("uno\r\ndos\rtres\ncuatro"),
            "uno\\ndos\\ntres\\ncuatro"
        );
        assert_eq!(escape("a\u{7}b\u{0}c\td"), "abc\td");
        let ds = deadlines_in_year(
            &TaxpayerProfile::autonomo(),
            2026,
            &HolidayCalendar::default(),
        );
        let nombre = "Demo SL\r\nATTENDEE:mailto:x@example.com\rLOCATION:x; y, z \\ ñ".repeat(3);
        let ics = to_ical(
            &ds,
            datetime!(2026-10-13 10:00 UTC),
            "principal",
            Some(&nombre),
        );
        // Ninguna línea lógica (tras desplegar) empieza por una propiedad inyectada.
        let desplegado = ics.replace("\r\n ", "");
        for l in desplegado.split("\r\n").filter(|l| !l.is_empty()) {
            assert!(
                !l.starts_with("ATTENDEE") && !l.starts_with("LOCATION"),
                "{l}"
            );
            assert!(!l.contains('\r') && !l.contains('\n'));
        }
        for l in ics.split("\r\n").filter(|l| !l.is_empty()) {
            assert!(l.len() <= 75, "línea larga: {l}");
        }
        assert!(desplegado.contains(
            "SUMMARY:Demo SL\\nATTENDEE:mailto:x@example.com\\nLOCATION:x\\; y\\, z \\\\ ñ"
        ));
    }
}
