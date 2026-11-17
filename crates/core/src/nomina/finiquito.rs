//! Finiquito básico: salario de los días del último mes, vacaciones devengadas y no
//! disfrutadas, parte proporcional de las pagas extraordinarias y, si es el fin de un contrato
//! temporal, su indemnización. Sin despidos ni otras indemnizaciones.
//!
//! Norma:
//! - Vacaciones: art. 38 ET (treinta días naturales al año como mínimo; las no disfrutadas al
//!   terminar el contrato se compensan en dinero). Se devengan en proporción al tiempo
//!   trabajado en el año natural de la baja. El valor del día es el salario mensual entre 30
//!   (`TODO(VERIFY)` con el convenio, que puede incluir otros complementos).
//! - Pagas extraordinarias: art. 31 ET; cada una en proporción a los días de su periodo de
//!   devengo (el que fija el convenio: lo teclea el usuario).
//! - Fin de contrato temporal: art. 49.1.c ET, doce días de salario por año de servicio
//!   (proporcional por fracción), salvo los formativos y los de sustitución (antes interinidad).
//!   `TODO(VERIFY)`: días según la fecha del contrato (DT 8.ª ET) y si el convenio mejora.
//!   El salario diario es el anual (doce mensualidades más las pagas) entre 365.
//! - Salario, vacaciones y pagas cotizan y tributan (las vacaciones no disfrutadas alargan el
//!   alta en la Seguridad Social: art. 153 TRLGSS). La indemnización no cotiza (art. 147.2.c
//!   TRLGSS) y está exenta del IRPF en la cuantía obligatoria del ET (art. 7.e LIRPF;
//!   `TODO(VERIFY)` con la doctrina de la DGT sobre el fin de contrato temporal).

use serde::{Deserialize, Serialize};
use time::{Date, Month};

use super::proporcion;
use crate::money::Money;

/// Una paga extraordinaria con el periodo de devengo en curso a la fecha de la baja.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PagaFiniquito {
    pub nombre: String,
    /// Importe de la paga completa.
    pub importe: Money,
    pub devengo_desde: Date,
    pub devengo_hasta: Date,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatosFiniquito {
    pub alta: Date,
    pub baja: Date,
    /// Salario mensual (salario base y complementos fijos) por mes completo.
    pub salario_mensual: Money,
    /// Días del último mes que faltan por pagar.
    pub dias_pendientes: u8,
    /// Días naturales de vacaciones al año (30 como mínimo, art. 38 ET).
    #[serde(default = "treinta")]
    pub vacaciones_anuales: u16,
    /// Días de vacaciones ya disfrutados en el año de la baja.
    #[serde(default)]
    pub vacaciones_disfrutadas: u16,
    /// Pagas no prorrateadas con su periodo de devengo (vacío si están prorrateadas).
    #[serde(default)]
    pub pagas: Vec<PagaFiniquito>,
    /// Fin de un contrato temporal con derecho a indemnización.
    #[serde(default)]
    pub fin_temporal: bool,
    /// Días de salario por año de servicio de esa indemnización.
    #[serde(default = "doce")]
    pub dias_por_ano: u16,
}

fn treinta() -> u16 {
    30
}

fn doce() -> u16 {
    12
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineaFiniquito {
    pub concepto: String,
    pub importe: Money,
    /// Cotiza a la Seguridad Social y tributa en el IRPF.
    pub cotiza: bool,
    pub detalle: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finiquito {
    pub lineas: Vec<LineaFiniquito>,
    /// Lo que cotiza y tributa (para el último recibo de salarios).
    pub sujeto: Money,
    /// Indemnización (no cotiza; exenta en la cuantía obligatoria).
    pub indemnizacion: Money,
    pub total: Money,
    /// Días de vacaciones pendientes, en centésimas de día.
    pub vacaciones_pendientes_centesimas: i64,
    pub avisos: Vec<String>,
}

fn dias(desde: Date, hasta: Date) -> i64 {
    (hasta - desde).whole_days() + 1
}

fn fmt_dias(centesimas: i64) -> String {
    let e = centesimas / 100;
    let c = (centesimas % 100).abs();
    if c == 0 {
        format!("{e}")
    } else {
        format!("{e},{c:02}")
    }
}

pub fn calcular(d: &DatosFiniquito) -> Result<Finiquito, String> {
    if d.baja < d.alta {
        return Err("la baja no puede ser anterior al alta".into());
    }
    if d.salario_mensual.is_negative() || d.salario_mensual.is_zero() {
        return Err("falta el salario mensual".into());
    }
    if d.dias_pendientes > 31 || d.vacaciones_anuales > 366 {
        return Err("días no válidos".into());
    }
    let mut lineas = Vec::new();
    let mut avisos = Vec::new();

    // Salario de los días del último mes.
    if d.dias_pendientes > 0 {
        lineas.push(LineaFiniquito {
            concepto: "Salario de los días trabajados del mes".into(),
            importe: proporcion(d.salario_mensual, i64::from(d.dias_pendientes), 30),
            cotiza: true,
            detalle: format!("{} días × salario mensual / 30", d.dias_pendientes),
        });
    }

    // Vacaciones del año natural de la baja.
    let inicio_ano =
        Date::from_calendar_date(d.baja.year(), Month::January, 1).map_err(|e| e.to_string())?;
    let desde = d.alta.max(inicio_ano);
    let trabajados = dias(desde, d.baja);
    let dias_ano = i64::from(time::util::days_in_year(d.baja.year()));
    // En centésimas de día.
    let devengadas = (i64::from(d.vacaciones_anuales) * trabajados * 100 + dias_ano / 2) / dias_ano;
    let pendientes = devengadas - i64::from(d.vacaciones_disfrutadas) * 100;
    if pendientes < 0 {
        avisos.push(format!(
            "Ha disfrutado {} días más de los que había devengado: el convenio dirá si se \
             descuentan (no se restan aquí)",
            fmt_dias(-pendientes)
        ));
    } else if pendientes > 0 {
        lineas.push(LineaFiniquito {
            concepto: "Vacaciones devengadas y no disfrutadas".into(),
            importe: proporcion(d.salario_mensual, pendientes, 3000),
            cotiza: true,
            detalle: format!(
                "{} días ({} devengados de {} al año por {trabajados} días trabajados, menos {} \
                 disfrutados) × salario mensual / 30",
                fmt_dias(pendientes),
                fmt_dias(devengadas),
                d.vacaciones_anuales,
                d.vacaciones_disfrutadas
            ),
        });
    }

    // Pagas extraordinarias.
    for p in &d.pagas {
        if p.devengo_hasta < p.devengo_desde || p.importe.is_negative() {
            return Err(format!("periodo de devengo no válido en «{}»", p.nombre));
        }
        let ini = p.devengo_desde.max(d.alta);
        let fin = p.devengo_hasta.min(d.baja);
        if fin < ini {
            continue;
        }
        let devengo = dias(ini, fin);
        let periodo = dias(p.devengo_desde, p.devengo_hasta);
        lineas.push(LineaFiniquito {
            concepto: format!("Parte proporcional de {}", p.nombre.trim()),
            importe: proporcion(p.importe, devengo, periodo),
            cotiza: true,
            detalle: format!("{devengo} de {periodo} días del periodo de devengo"),
        });
    }

    // Indemnización por fin de contrato temporal.
    let mut indemnizacion = Money::ZERO;
    if d.fin_temporal {
        let anual = Money::from_cents(d.salario_mensual.cents() * 12)
            + d.pagas.iter().map(|p| p.importe).sum::<Money>();
        let servicio = dias(d.alta, d.baja);
        // salario diario × días por año × servicio / 365, con el diario = anual / 365.
        indemnizacion = proporcion(anual, i64::from(d.dias_por_ano) * servicio, 365 * 365);
        lineas.push(LineaFiniquito {
            concepto: "Indemnización por fin de contrato temporal".into(),
            importe: indemnizacion,
            cotiza: false,
            detalle: format!(
                "{} días de salario por año de servicio ({servicio} días), con el salario diario \
                 de {} (art. 49.1.c ET; TODO(VERIFY) con el contrato y el convenio)",
                d.dias_por_ano,
                proporcion(anual, 1, 365)
            ),
        });
    }

    let total: Money = lineas.iter().map(|l| l.importe).sum();
    let sujeto: Money = lineas.iter().filter(|l| l.cotiza).map(|l| l.importe).sum();
    avisos.push(
        "El salario, las vacaciones y las pagas cotizan y tributan: van en el último recibo \
         de salarios con su retención y sus cuotas. Las vacaciones no disfrutadas alargan el \
         alta en la Seguridad Social tantos días como se pagan (art. 153 TRLGSS)."
            .into(),
    );
    Ok(Finiquito {
        lineas,
        sujeto,
        indemnizacion,
        total,
        vacaciones_pendientes_centesimas: pendientes.max(0),
        avisos,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn eur(e: i64) -> Money {
        Money::from_euros(e, 0)
    }

    #[test]
    fn finiquito_con_vacaciones_pagas_e_indemnizacion() {
        let d = DatosFiniquito {
            alta: date!(2025 - 07 - 01),
            baja: date!(2026 - 06 - 30),
            salario_mensual: eur(1500),
            dias_pendientes: 30,
            vacaciones_anuales: 30,
            vacaciones_disfrutadas: 5,
            pagas: vec![
                PagaFiniquito {
                    nombre: "la paga de verano".into(),
                    importe: eur(1500),
                    devengo_desde: date!(2025 - 07 - 01),
                    devengo_hasta: date!(2026 - 06 - 30),
                },
                PagaFiniquito {
                    nombre: "la paga de Navidad".into(),
                    importe: eur(1500),
                    devengo_desde: date!(2026 - 01 - 01),
                    devengo_hasta: date!(2026 - 12 - 31),
                },
            ],
            fin_temporal: true,
            dias_por_ano: 12,
        };
        let f = calcular(&d).unwrap();
        let get = |c: &str| {
            f.lineas
                .iter()
                .find(|l| l.concepto.starts_with(c))
                .unwrap()
                .importe
        };
        assert_eq!(get("Salario"), eur(1500));
        // 181 días de 2026 → 30 × 181 / 365 = 14,88 días; menos 5 = 9,88 × 50 € = 494,00.
        assert_eq!(f.vacaciones_pendientes_centesimas, 988);
        assert_eq!(get("Vacaciones"), eur(494));
        // Verano entera; Navidad 181/365 de 1.500 = 743,84.
        assert_eq!(get("Parte proporcional de la paga de verano"), eur(1500));
        assert_eq!(
            get("Parte proporcional de la paga de Navidad"),
            Money::from_cents(74_384)
        );
        // Anual 21.000; 12 días × 365/365 → 21.000 × 12 / 365 = 690,41.
        assert_eq!(f.indemnizacion, Money::from_cents(69_041));
        assert_eq!(f.total, f.sujeto + f.indemnizacion);
    }

    #[test]
    fn sin_indemnizacion_y_vacaciones_de_mas() {
        let d = DatosFiniquito {
            alta: date!(2026 - 03 - 01),
            baja: date!(2026 - 03 - 31),
            salario_mensual: eur(1200),
            dias_pendientes: 0,
            vacaciones_anuales: 30,
            vacaciones_disfrutadas: 10,
            pagas: vec![],
            fin_temporal: false,
            dias_por_ano: 12,
        };
        let f = calcular(&d).unwrap();
        assert!(f.lineas.is_empty());
        assert_eq!(f.total, Money::ZERO);
        assert!(f.avisos[0].contains("más de los que había devengado"));
        let mut mal = d.clone();
        mal.baja = date!(2026 - 02 - 01);
        assert!(calcular(&mal).is_err());
    }
}
