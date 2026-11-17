//! Fechas que la AEAT fija cada año y que no se pueden deducir con una regla general: fin de
//! la domiciliación bancaria (ver "Discrepancias" en `docs/CALENDAR_RULES.md`) e inicio de la
//! campaña de Renta. Fuente: calendario del contribuyente, cargado de
//! `fixtures/calendar/aeat-<año>.json`.
//!
//! La AEAT publica el calendario del contribuyente de cada año en diciembre del anterior. Hasta
//! que se carga el del año (una línea más en [`FICHEROS`]), los plazos salen de las reglas
//! generales (`rules.rs`), la domiciliación es una estimación y la campaña de Renta va marcada
//! como estimada; [`calendario_cargado`] lo dice para que la pantalla avise.

use std::sync::OnceLock;

use serde::Deserialize;
use time::Date;

/// Calendarios del contribuyente cargados: (año natural del calendario, JSON).
const FICHEROS: &[(i32, &str)] = &[(
    2026,
    include_str!("../../../../fixtures/calendar/aeat-2026.json"),
)];

#[derive(Clone, Debug, Deserialize)]
pub struct OfficialEntry {
    pub model: String,
    pub period: String,
    pub fiscal_year: i32,
    pub start: Option<Date>,
    pub end: Date,
    pub end_domiciliacion: Option<Date>,
    pub source: String,
}

#[derive(Deserialize)]
struct File {
    entries: Vec<OfficialEntry>,
}

/// Todas las entradas oficiales cargadas.
pub fn entries() -> &'static [OfficialEntry] {
    static E: OnceLock<Vec<OfficialEntry>> = OnceLock::new();
    E.get_or_init(|| {
        FICHEROS
            .iter()
            .flat_map(|(year, json)| {
                serde_json::from_str::<File>(json)
                    .unwrap_or_else(|e| panic!("aeat-{year}.json válido: {e}"))
                    .entries
            })
            .collect()
    })
}

/// ¿Está cargado el calendario del contribuyente de la AEAT del año natural `year`?
pub fn calendario_cargado(year: i32) -> bool {
    FICHEROS.iter().any(|(y, _)| *y == year)
}

pub fn find(model: &str, fiscal_year: i32, period: &str) -> Option<&'static OfficialEntry> {
    entries()
        .iter()
        .find(|e| e.model == model && e.fiscal_year == fiscal_year && e.period == period)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendario_de_2026_cargado_y_el_de_2027_aun_no() {
        assert!(calendario_cargado(2026));
        // TODO(VERIFY): cuando la AEAT publique el calendario del contribuyente de 2027
        // (diciembre de 2026), se añade `aeat-2027.json` a `FICHEROS` y se cambia esta línea.
        assert!(!calendario_cargado(2027));
        assert!(find("303", 2026, "1T").is_some());
        assert!(entries().iter().all(|e| e.end.year() == 2026));
    }
}
