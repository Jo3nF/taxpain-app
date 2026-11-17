//! Festivos oficiales (Resoluciones anuales de la Dirección General de Trabajo, BOE) cargados
//! de `fixtures/calendar/festivos-nacionales.json` (2026: BOE-A-2025-21667; 2027:
//! BOE-A-2026-21254).
//!
//! Los festivos autonómicos y locales del domicilio **también** prorrogan los plazos ante la
//! AEAT, incluida la presentación electrónica (Ley 39/2015 art. 30.6; página "Vencimientos en
//! días inhábiles" del calendario del contribuyente 2026). Ver `docs/CALENDAR_RULES.md`.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use time::Date;

use super::business::HolidayCalendar;

const DATA: &str = include_str!("../../../../fixtures/calendar/festivos-nacionales.json");

#[derive(Deserialize)]
struct File {
    years: BTreeMap<String, Year>,
}

#[derive(Deserialize)]
struct Year {
    #[serde(default)]
    national: Vec<Day>,
    #[serde(default)]
    by_region: BTreeMap<String, Vec<Day>>,
}

#[derive(Deserialize)]
struct Day {
    date: Date,
}

fn data() -> &'static File {
    static F: OnceLock<File> = OnceLock::new();
    F.get_or_init(|| serde_json::from_str(DATA).expect("festivos-nacionales.json válido"))
}

/// Comunidades y ciudades autónomas (territorio común y foral).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Ccaa {
    Andalucia,
    Aragon,
    Asturias,
    IllesBalears,
    Canarias,
    Cantabria,
    CastillaLaMancha,
    CastillaYLeon,
    Cataluna,
    Extremadura,
    Galicia,
    Madrid,
    Murcia,
    Navarra,
    PaisVasco,
    LaRioja,
    ComunitatValenciana,
    Ceuta,
    Melilla,
}

impl Ccaa {
    pub const ALL: [Ccaa; 19] = [
        Ccaa::Andalucia,
        Ccaa::Aragon,
        Ccaa::Asturias,
        Ccaa::IllesBalears,
        Ccaa::Canarias,
        Ccaa::Cantabria,
        Ccaa::CastillaLaMancha,
        Ccaa::CastillaYLeon,
        Ccaa::Cataluna,
        Ccaa::Extremadura,
        Ccaa::Galicia,
        Ccaa::Madrid,
        Ccaa::Murcia,
        Ccaa::Navarra,
        Ccaa::PaisVasco,
        Ccaa::LaRioja,
        Ccaa::ComunitatValenciana,
        Ccaa::Ceuta,
        Ccaa::Melilla,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Ccaa::Andalucia => "Andalucía",
            Ccaa::Aragon => "Aragón",
            Ccaa::Asturias => "Asturias",
            Ccaa::IllesBalears => "Illes Balears",
            Ccaa::Canarias => "Canarias",
            Ccaa::Cantabria => "Cantabria",
            Ccaa::CastillaLaMancha => "Castilla-La Mancha",
            Ccaa::CastillaYLeon => "Castilla y León",
            Ccaa::Cataluna => "Cataluña",
            Ccaa::Extremadura => "Extremadura",
            Ccaa::Galicia => "Galicia",
            Ccaa::Madrid => "Comunidad de Madrid",
            Ccaa::Murcia => "Región de Murcia",
            Ccaa::Navarra => "Comunidad Foral de Navarra",
            Ccaa::PaisVasco => "País Vasco",
            Ccaa::LaRioja => "La Rioja",
            Ccaa::ComunitatValenciana => "Comunitat Valenciana",
            Ccaa::Ceuta => "Ceuta",
            Ccaa::Melilla => "Melilla",
        }
    }

    /// Claves con que aparece en el anexo del BOE (varían ligeramente de un año a otro).
    fn boe_keys(self) -> &'static [&'static str] {
        match self {
            Ccaa::Madrid => &["Com. Madrid", "Com. de Madrid"],
            Ccaa::Murcia => &["Región Murcia", "Región de Murcia"],
            Ccaa::Ceuta => &["Ciudad de Ceuta"],
            Ccaa::Melilla => &["Ciudad de Melilla"],
            Ccaa::Navarra => &["C. Foral Navarra", "C. Foral de Navarra"],
            other => match other {
                Ccaa::Andalucia => &["Andalucía"],
                Ccaa::Aragon => &["Aragón"],
                Ccaa::Asturias => &["Asturias"],
                Ccaa::IllesBalears => &["Illes Balears"],
                Ccaa::Canarias => &["Canarias"],
                Ccaa::Cantabria => &["Cantabria"],
                Ccaa::CastillaLaMancha => &["Castilla-La Mancha"],
                Ccaa::CastillaYLeon => &["Castilla y León"],
                Ccaa::Cataluna => &["Cataluña"],
                Ccaa::Extremadura => &["Extremadura"],
                Ccaa::Galicia => &["Galicia"],
                Ccaa::PaisVasco => &["País Vasco"],
                Ccaa::LaRioja => &["La Rioja"],
                Ccaa::ComunitatValenciana => &["Comunitat Valenciana"],
                _ => &[],
            },
        }
    }
}

/// Festivos nacionales de `year`, o `None` si el BOE de ese año no está cargado.
pub fn national(year: i32) -> Option<Vec<Date>> {
    data()
        .years
        .get(&year.to_string())
        .map(|y| y.national.iter().map(|d| d.date).collect())
}

/// Festivos autonómicos de `year` en `ccaa` (además de los nacionales).
pub fn regional(year: i32, ccaa: Ccaa) -> Vec<Date> {
    let Some(y) = data().years.get(&year.to_string()) else {
        return Vec::new();
    };
    ccaa.boe_keys()
        .iter()
        .find_map(|k| y.by_region.get(*k))
        .map(|days| days.iter().map(|d| d.date).collect())
        .unwrap_or_default()
}

/// Calendario para calcular plazos que vencen en `year`: festivos nacionales, autonómicos y
/// locales de `year` y `year + 1` (un plazo de finales de año puede trasladarse a enero).
/// Devuelve también si los festivos oficiales de `year` estaban cargados.
pub fn calendar_for(year: i32, ccaa: Option<Ccaa>, local: &[Date]) -> (HolidayCalendar, bool) {
    let mut days = Vec::new();
    let mut loaded = false;
    for y in [year, year + 1] {
        if let Some(n) = national(y) {
            loaded |= y == year;
            days.extend(n);
        }
        if let Some(c) = ccaa {
            days.extend(regional(y, c));
        }
    }
    days.extend_from_slice(local);
    (HolidayCalendar::new(days), loaded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn festivos_2026_y_2027_cargados() {
        let n26 = national(2026).unwrap();
        assert_eq!(n26.len(), 8);
        assert!(n26.contains(&date!(2026 - 04 - 03)), "Viernes Santo");
        assert!(n26.contains(&date!(2026 - 12 - 25)));
        let n27 = national(2027).unwrap();
        assert!(n27.contains(&date!(2027 - 03 - 26)) && n27.contains(&date!(2027 - 11 - 01)));
        assert_eq!(national(2030), None);
    }

    #[test]
    fn todas_las_ccaa_tienen_datos() {
        for year in [2026, 2027] {
            for c in Ccaa::ALL {
                assert!(!regional(year, c).is_empty(), "{year} {}", c.name());
            }
        }
        assert!(regional(2026, Ccaa::IllesBalears).contains(&date!(2026 - 03 - 02)));
        assert!(regional(2026, Ccaa::Melilla).contains(&date!(2026 - 03 - 20)));
    }

    #[test]
    fn calendario_combinado() {
        let (cal, loaded) = calendar_for(2026, Some(Ccaa::IllesBalears), &[date!(2026 - 06 - 24)]);
        assert!(loaded);
        assert!(cal.is_holiday(date!(2026 - 01 - 06)));
        assert!(cal.is_holiday(date!(2026 - 03 - 02)));
        assert!(cal.is_holiday(date!(2026 - 06 - 24)));
        assert!(
            cal.is_holiday(date!(2027 - 01 - 01)),
            "incluye el año siguiente"
        );
        assert!(!calendar_for(2031, None, &[]).1);
    }
}
