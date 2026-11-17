//! Parámetros fiscales que cambian cada año y que no dependen de un modelo concreto: interés
//! legal del dinero e interés de demora tributario. Cada valor dice de dónde sale: una norma ya
//! publicada o la prórroga del año anterior (presupuestos prorrogados, art. 134.4 CE), que se
//! enseña en el aviso de cambio de año del Panel. Cotejado el 2026-11-16: 2015 a 2023, con la
//! disposición adicional de cada ley de presupuestos en el BOE; 2024 a 2026, con la tabla del
//! Banco de España (3,25 %, sin ley de presupuestos de esos años); 2027 no tiene norma todavía
//! y sigue marcado `TODO(VERIFY)`.
//!
//! Regla del proyecto: toda tabla de parámetros por año llega con valor explícito hasta
//! [`ULTIMO_EJERCICIO_PREVISTO`] (hoy, 2027): el que fija una ley publicada o la prórroga
//! marcada como tal. Un test lo comprueba aquí y en las demás tablas (`reta::TABLAS`,
//! `m200_catalogo::TIPOS`).

use serde::Serialize;

/// Último ejercicio para el que cada tabla debe tener una fila propia.
pub const ULTIMO_EJERCICIO_PREVISTO: i32 = 2027;

/// De dónde sale el valor de un año.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum Origen {
    /// Lo fija una norma publicada.
    Norma { norma: &'static str },
    /// Sin norma del año: se mantiene el valor del ejercicio `de`. `TODO(VERIFY)` cuando se
    /// publique la ley de presupuestos (o la norma que corresponda).
    Prorroga { de: i32, motivo: &'static str },
}

impl Origen {
    pub fn es_prorroga(self) -> bool {
        matches!(self, Origen::Prorroga { .. })
    }
}

/// Valor de un parámetro en un ejercicio.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ValorAnual<T: 'static> {
    pub year: i32,
    pub valor: T,
    pub origen: Origen,
}

const fn norma<T>(year: i32, valor: T, norma: &'static str) -> ValorAnual<T> {
    ValorAnual {
        year,
        valor,
        origen: Origen::Norma { norma },
    }
}

const fn prorroga<T>(year: i32, valor: T, de: i32, motivo: &'static str) -> ValorAnual<T> {
    ValorAnual {
        year,
        valor,
        origen: Origen::Prorroga { de, motivo },
    }
}

/// La fila del año, o la del último año anterior que haya (para los años posteriores al último
/// cargado; el aviso de cambio de año dice que falta). `None` si el año es anterior a la tabla.
pub fn del_año<T>(tabla: &'static [ValorAnual<T>], year: i32) -> Option<&'static ValorAnual<T>> {
    tabla.iter().rev().find(|v| v.year <= year)
}

const LPGE_PRORROGADA: &str = "Presupuestos Generales del Estado de 2023 prorrogados (art. 134.4 CE): sigue la DA \
     42.ª de la Ley 31/2022; el Banco de España publica el 3,25 % para 2024, 2025 y 2026";
const SIN_LPGE: &str = "A 16-11-2026 no hay ley de presupuestos de 2027 en el BOE: se mantiene el del año \
                        anterior (art. 134.4 CE). TODO(VERIFY)";

/// Interés de demora tributario (art. 26.6 LGT: el interés legal más un 25 %, salvo que la ley
/// de presupuestos diga otro), en millonésimas (40.625 = 4,0625 %).
pub const INTERES_DEMORA: &[ValorAnual<i64>] = &[
    norma(
        2015,
        43_750,
        "Ley 36/2014 (LPGE 2015), DA 32.ª (BOE-A-2014-13612)",
    ),
    norma(
        2016,
        37_500,
        "Ley 48/2015 (LPGE 2016), DA 34.ª (BOE-A-2015-11644)",
    ),
    norma(
        2017,
        37_500,
        "Ley 3/2017 (LPGE 2017), DA 44.ª (BOE-A-2017-7387)",
    ),
    norma(
        2018,
        37_500,
        "Ley 6/2018 (LPGE 2018), DA 57.ª (BOE-A-2018-9268)",
    ),
    prorroga(
        2019,
        37_500,
        2018,
        "Presupuestos de 2018 prorrogados (art. 134.4 CE)",
    ),
    prorroga(
        2020,
        37_500,
        2018,
        "Presupuestos de 2018 prorrogados (art. 134.4 CE)",
    ),
    norma(
        2021,
        37_500,
        "Ley 11/2020 (LPGE 2021), DA 49.ª (BOE-A-2020-17339)",
    ),
    norma(
        2022,
        37_500,
        "Ley 22/2021 (LPGE 2022), DA 46.ª (BOE-A-2021-21653)",
    ),
    norma(
        2023,
        40_625,
        "Ley 31/2022 (LPGE 2023), DA 42.ª (BOE-A-2022-22128)",
    ),
    prorroga(2024, 40_625, 2023, LPGE_PRORROGADA),
    prorroga(2025, 40_625, 2023, LPGE_PRORROGADA),
    prorroga(2026, 40_625, 2023, LPGE_PRORROGADA),
    prorroga(2027, 40_625, 2026, SIN_LPGE),
];

/// Interés legal del dinero (Ley 24/1984 y la ley de presupuestos de cada año), en millonésimas
/// (32.500 = 3,25 %).
pub const INTERES_LEGAL: &[ValorAnual<i64>] = &[
    norma(
        2015,
        35_000,
        "Ley 36/2014 (LPGE 2015), DA 32.ª (BOE-A-2014-13612)",
    ),
    norma(
        2016,
        30_000,
        "Ley 48/2015 (LPGE 2016), DA 34.ª (BOE-A-2015-11644)",
    ),
    norma(
        2017,
        30_000,
        "Ley 3/2017 (LPGE 2017), DA 44.ª (BOE-A-2017-7387)",
    ),
    norma(
        2018,
        30_000,
        "Ley 6/2018 (LPGE 2018), DA 57.ª (BOE-A-2018-9268)",
    ),
    prorroga(
        2019,
        30_000,
        2018,
        "Presupuestos de 2018 prorrogados (art. 134.4 CE)",
    ),
    prorroga(
        2020,
        30_000,
        2018,
        "Presupuestos de 2018 prorrogados (art. 134.4 CE)",
    ),
    norma(
        2021,
        30_000,
        "Ley 11/2020 (LPGE 2021), DA 49.ª (BOE-A-2020-17339)",
    ),
    norma(
        2022,
        30_000,
        "Ley 22/2021 (LPGE 2022), DA 46.ª (BOE-A-2021-21653)",
    ),
    norma(
        2023,
        32_500,
        "Ley 31/2022 (LPGE 2023), DA 42.ª (BOE-A-2022-22128)",
    ),
    prorroga(2024, 32_500, 2023, LPGE_PRORROGADA),
    prorroga(2025, 32_500, 2023, LPGE_PRORROGADA),
    prorroga(2026, 32_500, 2023, LPGE_PRORROGADA),
    prorroga(2027, 32_500, 2026, SIN_LPGE),
];

/// Interés de demora del año en millonésimas (el primero de la tabla para años anteriores).
pub fn interes_demora(year: i32) -> i64 {
    del_año(INTERES_DEMORA, year)
        .or_else(|| INTERES_DEMORA.first())
        .map_or(0, |v| v.valor)
}

/// Interés legal del dinero del año en millonésimas.
pub fn interes_legal(year: i32) -> i64 {
    del_año(INTERES_LEGAL, year)
        .or_else(|| INTERES_LEGAL.first())
        .map_or(0, |v| v.valor)
}

/// Millonésimas como porcentaje para leer: 40.625 → «4,0625 %», 32.500 → «3,25 %».
pub fn porcentaje(millonesimas: i64) -> String {
    let entero = millonesimas / 10_000;
    let resto = millonesimas % 10_000;
    if resto == 0 {
        return format!("{entero} %");
    }
    let dec = format!("{resto:04}");
    format!("{entero},{} %", dec.trim_end_matches('0'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cubre_hasta_el_previsto<T>(nombre: &str, t: &'static [ValorAnual<T>]) {
        let años: Vec<i32> = t.iter().map(|v| v.year).collect();
        assert!(
            años.windows(2).all(|w| w[1] == w[0] + 1),
            "{nombre}: años seguidos"
        );
        assert_eq!(
            años.last().copied(),
            Some(ULTIMO_EJERCICIO_PREVISTO),
            "{nombre}: fila propia para {ULTIMO_EJERCICIO_PREVISTO}"
        );
        for v in t {
            if let Origen::Prorroga { de, .. } = v.origen {
                assert!(de < v.year, "{nombre} {}", v.year);
            }
        }
    }

    #[test]
    fn las_tablas_llegan_a_2027_con_su_origen() {
        cubre_hasta_el_previsto("demora", INTERES_DEMORA);
        cubre_hasta_el_previsto("legal", INTERES_LEGAL);
        // 2027: prórroga marcada, no una ley inventada.
        assert!(del_año(INTERES_DEMORA, 2027).unwrap().origen.es_prorroga());
        assert!(del_año(INTERES_LEGAL, 2027).unwrap().origen.es_prorroga());
        assert_eq!(interes_demora(2027), 40_625);
        assert_eq!(interes_legal(2027), 32_500);
        // El de demora es el legal más un 25 % (art. 26.6 LGT) en los años de la tabla.
        for (d, l) in INTERES_DEMORA.iter().zip(INTERES_LEGAL) {
            assert_eq!(d.year, l.year);
            assert_eq!(d.valor * 4, l.valor * 5, "{}", d.year);
        }
        assert_eq!(
            interes_demora(2010),
            43_750,
            "antes de la tabla, la primera"
        );
    }

    #[test]
    fn porcentajes() {
        assert_eq!(porcentaje(40_625), "4,0625 %");
        assert_eq!(porcentaje(32_500), "3,25 %");
        assert_eq!(porcentaje(30_000), "3 %");
        assert_eq!(porcentaje(37_500), "3,75 %");
    }
}
