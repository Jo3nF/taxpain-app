//! Vigencia de los datos anuales y aviso de cambio de año.
//!
//! Lo que cambia cada año y TaxPain lleva como dato: festivos (BOE de la Dirección General de
//! Trabajo y boletines autonómicos y locales), calendario del contribuyente de la AEAT, diseños de
//! registro de cada modelo y parámetros (tipos del IS de la Ley 7/2024, interés legal y de
//! demora, tabla del RETA). Para un año dice qué está cargado con su norma, qué se ha prorrogado
//! del anterior (`TODO(VERIFY)` hasta que se publique) y qué falta. En diciembre (para el año
//! siguiente) y en enero (para el que empieza) el Panel lo enseña como lista de lo que hay que
//! revisar.

use serde::Serialize;
use taxpain_core::calendar::{Ccaa, TaxpayerProfile, holidays, official};
use taxpain_core::parametros::{self, Origen};
use taxpain_core::reta;
use taxpain_models::{dr_versions, m200_catalogo};
use taxpain_vault::Vault;
use time::{Date, Month};

use crate::error::Result;

/// Estado de una comprobación.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Estado {
    /// Cargado con su norma o fuente oficial del año.
    Listo,
    /// Se usa el valor o el diseño del año anterior hasta que se publique el del año.
    Supuesto,
    /// Falta un dato que TaxPain no puede inventar (festivos locales, de la comunidad…).
    Pendiente,
    /// Lo tiene que mirar el usuario (su perfil fiscal del año nuevo).
    Revisar,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Comprobacion {
    pub clave: &'static str,
    pub titulo: String,
    pub estado: Estado,
    pub detalle: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Vigencia {
    pub year: i32,
    pub comprobaciones: Vec<Comprobacion>,
}

impl Vigencia {
    /// Cuántas comprobaciones no están listas.
    pub fn por_revisar(&self) -> usize {
        self.comprobaciones
            .iter()
            .filter(|c| c.estado != Estado::Listo)
            .count()
    }
}

/// Modelos periódicos: en el año `year` se presentan los periodos de `year` (y el 4T del
/// anterior, ya comprobado).
const PERIODICOS: [&str; 7] = ["303", "130", "111", "115", "123", "202", "349"];
/// Anuales: en `year` se presenta el ejercicio `year - 1`.
const ANUALES: [&str; 7] = ["390", "347", "190", "180", "193", "184", "200"];

fn c(clave: &'static str, titulo: String, estado: Estado, detalle: String) -> Comprobacion {
    Comprobacion {
        clave,
        titulo,
        estado,
        detalle,
    }
}

fn origen_texto(o: Origen) -> (Estado, String) {
    match o {
        Origen::Norma { norma } => (Estado::Listo, norma.to_owned()),
        Origen::Prorroga { de, motivo } => (
            Estado::Supuesto,
            format!("Se mantiene el de {de}: {motivo}"),
        ),
    }
}

fn festivos(perfil: Option<&TaxpayerProfile>, year: i32) -> Vec<Comprobacion> {
    let nacionales = match holidays::national(year) {
        Some(d) => c(
            "festivos_nacionales",
            format!("Festivos nacionales de {year}"),
            Estado::Listo,
            format!(
                "{} días, de la resolución de fiestas laborales del BOE.",
                d.len()
            ),
        ),
        None => c(
            "festivos_nacionales",
            format!("Festivos nacionales de {year}"),
            Estado::Pendiente,
            "Pendiente de publicar: la Dirección General de Trabajo publica la resolución en el \
             BOE en otoño. Hasta entonces los plazos sólo se trasladan por sábados y domingos."
                .to_owned(),
        ),
    };
    let ccaa: Option<Ccaa> = perfil.and_then(|p| p.ccaa);
    let autonomicos = match ccaa {
        None => c(
            "festivos_autonomicos",
            format!("Festivos de tu comunidad en {year}"),
            Estado::Revisar,
            "Elige tu comunidad autónoma en el perfil fiscal (Calendario): sus festivos también \
             trasladan los plazos (Ley 39/2015, art. 30.6)."
                .to_owned(),
        ),
        Some(cc) => {
            let n = holidays::regional(year, cc).len();
            if n > 0 {
                c(
                    "festivos_autonomicos",
                    format!("Festivos de {} en {year}", cc.name()),
                    Estado::Listo,
                    format!("{n} días, del anexo de la misma resolución del BOE."),
                )
            } else {
                c(
                    "festivos_autonomicos",
                    format!("Festivos de {} en {year}", cc.name()),
                    Estado::Pendiente,
                    "Pendiente de publicar (o sin cargar en TaxPain): no se suponen.".to_owned(),
                )
            }
        }
    };
    let locales = perfil.map_or(0, |p| {
        p.festivos_locales
            .iter()
            .filter(|d| d.year() == year)
            .count()
    });
    let locales = if locales > 0 {
        c(
            "festivos_locales",
            format!("Festivos locales de {year}"),
            Estado::Listo,
            format!("{locales} en tu perfil fiscal."),
        )
    } else {
        c(
            "festivos_locales",
            format!("Festivos locales de {year}"),
            Estado::Pendiente,
            "Pendiente de publicar o de añadir: los dos de tu municipio salen en el boletín de la \
             comunidad o de la provincia. Añádelos en el perfil fiscal cuando se publiquen; \
             TaxPain no los inventa."
                .to_owned(),
        )
    };
    vec![nacionales, autonomicos, locales]
}

fn calendario_aeat(year: i32) -> Comprobacion {
    if official::calendario_cargado(year) {
        c(
            "calendario_aeat",
            format!("Calendario del contribuyente {year} (AEAT)"),
            Estado::Listo,
            "Cargado: fin de la domiciliación y campaña de Renta con las fechas oficiales."
                .to_owned(),
        )
    } else {
        c(
            "calendario_aeat",
            format!("Calendario del contribuyente {year} (AEAT)"),
            Estado::Supuesto,
            "Aún no está en TaxPain (la AEAT lo publica en diciembre): los plazos salen de la \
             norma de cada modelo con su traslado por inhábil, pero el último día para domiciliar \
             y la campaña de Renta son estimados."
                .to_owned(),
        )
    }
}

fn disenos(year: i32) -> Comprobacion {
    let mut faltan = Vec::new();
    for (models, ej) in [(&PERIODICOS[..], year), (&ANUALES[..], year - 1)] {
        for m in models {
            if let Some(g) = dr_versions::generador(m)
                && ej > g.comprobado_hasta
            {
                faltan.push(format!("{m} de {ej} (se usa {})", g.name));
            }
        }
    }
    let titulo = format!("Diseños de registro de lo que se presenta en {year}");
    if faltan.is_empty() {
        c(
            "disenos",
            titulo,
            Estado::Listo,
            "Todos comprobados en la Sede.".to_owned(),
        )
    } else {
        c(
            "disenos",
            titulo,
            Estado::Supuesto,
            format!(
                "Sin publicar todavía, se genera con el último vigente y el borrador lo avisa: {}.",
                faltan.join("; ")
            ),
        )
    }
}

fn parametros_del_año(year: i32) -> Vec<Comprobacion> {
    let mut out = Vec::new();
    let pct = parametros::porcentaje;
    for (clave, nombre, tabla) in [
        (
            "interes_demora",
            "Interés de demora",
            parametros::INTERES_DEMORA,
        ),
        (
            "interes_legal",
            "Interés legal del dinero",
            parametros::INTERES_LEGAL,
        ),
    ] {
        let titulo = format!("{nombre} de {year}");
        out.push(match parametros::del_año(tabla, year) {
            Some(v) if v.year == year => {
                let (estado, fuente) = origen_texto(v.origen);
                c(
                    clave,
                    titulo,
                    estado,
                    format!("{}. {fuente}.", pct(v.valor)),
                )
            }
            Some(v) => c(
                clave,
                titulo,
                Estado::Supuesto,
                format!(
                    "{} (el último cargado, de {}). Falta el del año en TaxPain.",
                    pct(v.valor),
                    v.year
                ),
            ),
            None => c(clave, titulo, Estado::Pendiente, "Sin datos.".to_owned()),
        });
    }
    // Tipos del IS: la Ley 7/2024 los fija año a año hasta 2029 (DT 44.ª LIS).
    let (a, b, r) = m200_catalogo::tipos(year);
    let explicito = m200_catalogo::TIPOS.iter().any(|(y, ..)| *y == year)
        || year > m200_catalogo::TIPOS.last().map_or(i32::MAX, |t| t.0);
    out.push(c(
        "tipos_is",
        format!("Tipos del Impuesto sobre Sociedades de {year}"),
        if explicito {
            Estado::Listo
        } else {
            Estado::Supuesto
        },
        format!(
            "Microempresas {a} % hasta 50.000 € y {b} % el resto; reducida dimensión {r} %; \
             general 25 %. Ley 7/2024 (DT 44.ª LIS), que los escalona hasta 2029."
        ),
    ));
    // RETA: la tabla del año o la prorrogada.
    out.push(match reta::tabla(year) {
        Some(t) if t.year == year && t.pendiente.is_empty() => c(
            "reta",
            format!("Cotización de autónomos (RETA) de {year}"),
            Estado::Listo,
            t.norma.to_owned(),
        ),
        Some(t) => c(
            "reta",
            format!("Cotización de autónomos (RETA) de {year}"),
            Estado::Supuesto,
            format!(
                "{}. Pendiente de cotejar: {}.",
                t.norma,
                t.pendiente.first().copied().unwrap_or_default()
            ),
        ),
        None => c(
            "reta",
            format!("Cotización de autónomos (RETA) de {year}"),
            Estado::Pendiente,
            "Sin tabla.".to_owned(),
        ),
    });
    out
}

fn perfil_del_año(year: i32) -> Comprobacion {
    c(
        "perfil",
        format!("Tu perfil fiscal para {year}"),
        Estado::Revisar,
        format!(
            "Con las cifras de {}: IVA trimestral o mensual (SII), gran empresa (más de \
             6.010.121,04 € de volumen de operaciones, art. 71.3 RIVA), 349 mensual, opciones y \
             renuncias de noviembre y diciembre, y si cambian tus trabajadores o alquileres.",
            year - 1
        ),
    )
}

/// Qué datos de `year` tiene TaxPain y de dónde salen.
pub fn vigencia(perfil: Option<&TaxpayerProfile>, year: i32) -> Vigencia {
    let mut comprobaciones = festivos(perfil, year);
    comprobaciones.push(calendario_aeat(year));
    comprobaciones.push(disenos(year));
    comprobaciones.extend(parametros_del_año(year));
    comprobaciones.push(perfil_del_año(year));
    Vigencia {
        year,
        comprobaciones,
    }
}

/// El año del que avisar al cambiar de año: en diciembre, el siguiente; en enero, el que
/// empieza. El resto del año, ninguno.
pub fn año_de_aviso(hoy: Date) -> Option<i32> {
    match hoy.month() {
        Month::December => Some(hoy.year() + 1),
        Month::January => Some(hoy.year()),
        _ => None,
    }
}

/// Aviso del Panel en diciembre y enero: la vigencia del año nuevo con el perfil guardado.
pub fn aviso_cambio_de_año(v: &Vault, hoy: Date) -> Result<Option<Vigencia>> {
    let Some(year) = año_de_aviso(hoy) else {
        return Ok(None);
    };
    let perfil: Option<TaxpayerProfile> = v.setting("taxpayer_profile")?;
    Ok(Some(vigencia(perfil.as_ref(), year)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn estado(v: &Vigencia, clave: &str) -> Estado {
        v.comprobaciones
            .iter()
            .find(|c| c.clave == clave)
            .unwrap_or_else(|| panic!("{clave}"))
            .estado
    }

    #[test]
    fn solo_en_diciembre_y_enero() {
        assert_eq!(año_de_aviso(date!(2026 - 12 - 01)), Some(2027));
        assert_eq!(año_de_aviso(date!(2027 - 01 - 31)), Some(2027));
        assert_eq!(año_de_aviso(date!(2026 - 11 - 16)), None);
        assert_eq!(año_de_aviso(date!(2027 - 02 - 01)), None);
    }

    #[test]
    fn vigencia_de_2027() {
        let perfil = TaxpayerProfile {
            ccaa: Some(Ccaa::Madrid),
            ..TaxpayerProfile::sociedad_limitada()
        };
        let v = vigencia(Some(&perfil), 2027);
        assert_eq!(estado(&v, "festivos_nacionales"), Estado::Listo);
        assert_eq!(estado(&v, "festivos_autonomicos"), Estado::Listo);
        // Sin festivos locales: no se inventan.
        assert_eq!(estado(&v, "festivos_locales"), Estado::Pendiente);
        assert_eq!(estado(&v, "calendario_aeat"), Estado::Supuesto);
        assert_eq!(estado(&v, "disenos"), Estado::Supuesto);
        assert_eq!(estado(&v, "interes_demora"), Estado::Supuesto);
        assert_eq!(estado(&v, "interes_legal"), Estado::Supuesto);
        assert_eq!(estado(&v, "tipos_is"), Estado::Listo);
        assert_eq!(estado(&v, "reta"), Estado::Supuesto);
        assert_eq!(estado(&v, "perfil"), Estado::Revisar);
        let is = v
            .comprobaciones
            .iter()
            .find(|c| c.clave == "tipos_is")
            .unwrap();
        assert!(
            is.detalle.starts_with(
                "Microempresas 17 % hasta 50.000 € y 20 % el resto; reducida dimensión 22 %"
            ),
            "{}",
            is.detalle
        );
        let d = v
            .comprobaciones
            .iter()
            .find(|c| c.clave == "disenos")
            .unwrap();
        assert!(
            d.detalle.contains("303 de 2027 (se usa DR303e26v101)"),
            "{}",
            d.detalle
        );
        assert!(
            d.detalle.contains("200 de 2026 (se usa DR200e25)"),
            "{}",
            d.detalle
        );
        let dem = v
            .comprobaciones
            .iter()
            .find(|c| c.clave == "interes_demora")
            .unwrap();
        assert!(
            dem.detalle.starts_with("4,0625 %. Se mantiene el de 2026"),
            "{}",
            dem.detalle
        );
        assert!(v.por_revisar() >= 6);
        // Con un festivo local del año, listo; sin comunidad, a revisar.
        let local = TaxpayerProfile {
            festivos_locales: vec![date!(2027 - 05 - 15)],
            ..TaxpayerProfile::sociedad_limitada()
        };
        let v = vigencia(Some(&local), 2027);
        assert_eq!(estado(&v, "festivos_locales"), Estado::Listo);
        assert_eq!(estado(&v, "festivos_autonomicos"), Estado::Revisar);
        // Un año sin nada cargado: pendiente, sin inventar.
        let v = vigencia(None, 2031);
        assert_eq!(estado(&v, "festivos_nacionales"), Estado::Pendiente);
        assert_eq!(estado(&v, "interes_demora"), Estado::Supuesto);
    }
}
