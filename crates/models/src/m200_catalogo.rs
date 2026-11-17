//! Catálogo de casillas del modelo 200 que TaxPain sabe rellenar fuera de la liquidación básica:
//! correcciones al resultado contable (páginas 12 y 13), bonificaciones y deducciones por doble
//! imposición (página 14), otras deducciones y retenciones desglosadas (página 14 bis).
//!
//! Números y textos copiados del diseño de registro de 2025 (`DR200e25.xls`, en
//! `fixtures/aeat/dr/200.json`); el test [`tests::cada_casilla_esta_en_su_pagina_del_dr`]
//! comprueba que cada una está en la página que se dice. La norma es la que cita el propio DR en
//! la descripción de la casilla.

use serde::Serialize;

/// Corrección al resultado contable con su casilla oficial.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Correccion {
    pub casilla: u16,
    /// Aumento (suma al resultado) o disminución.
    pub aumento: bool,
    /// Página del 200 («12» o «13»).
    pub pagina: &'static str,
    pub concepto: &'static str,
    pub norma: &'static str,
    /// Incentivo de las entidades de reducida dimensión (cap. XI del título VII LIS): no lo
    /// puede aplicar una entidad patrimonial (art. 101.1 LIS).
    pub solo_erd: bool,
}

const fn c(
    casilla: u16,
    aumento: bool,
    pagina: &'static str,
    concepto: &'static str,
    norma: &'static str,
) -> Correccion {
    Correccion {
        casilla,
        aumento,
        pagina,
        concepto,
        norma,
        solo_erd: false,
    }
}

const fn erd(
    casilla: u16,
    aumento: bool,
    concepto: &'static str,
    norma: &'static str,
) -> Correccion {
    Correccion {
        casilla,
        aumento,
        pagina: "13",
        concepto,
        norma,
        solo_erd: true,
    }
}

/// Correcciones al resultado contable que se pueden indicar (además de la del propio IS,
/// [00301]/[00302], que sale de la contabilidad).
pub const CORRECCIONES: &[Correccion] = &[
    c(
        355,
        true,
        "12",
        "Cambio de criterios contables",
        "art. 11.3.2.º LIS",
    ),
    c(
        356,
        false,
        "12",
        "Cambio de criterios contables",
        "art. 11.3.2.º LIS",
    ),
    c(357, true, "12", "Operaciones a plazos", "art. 11.4 LIS"),
    c(358, false, "12", "Operaciones a plazos", "art. 11.4 LIS"),
    c(
        361,
        true,
        "12",
        "Otras diferencias de imputación temporal de ingresos y gastos",
        "art. 11 LIS",
    ),
    c(
        362,
        false,
        "12",
        "Otras diferencias de imputación temporal de ingresos y gastos",
        "art. 11 LIS",
    ),
    c(
        303,
        true,
        "12",
        "Amortización contable superior a la fiscal",
        "art. 12.1 LIS",
    ),
    c(
        304,
        false,
        "12",
        "Amortización fiscal superior a la contable",
        "art. 12.1 LIS",
    ),
    c(
        1005,
        true,
        "12",
        "Amortización del inmovilizado intangible y fondo de comercio",
        "art. 12.2 LIS",
    ),
    c(
        1006,
        false,
        "12",
        "Amortización del inmovilizado intangible y fondo de comercio",
        "art. 12.2 LIS",
    ),
    c(
        1003,
        true,
        "12",
        "Libertad de amortización de inmovilizado material nuevo (reversión)",
        "art. 12.3.e LIS",
    ),
    c(
        1004,
        false,
        "12",
        "Libertad de amortización de inmovilizado material nuevo",
        "art. 12.3.e LIS",
    ),
    c(
        309,
        true,
        "12",
        "Otros supuestos de libertad de amortización (reversión)",
        "art. 12.3.a y d LIS, DA 16.ª y 17.ª",
    ),
    c(
        310,
        false,
        "12",
        "Otros supuestos de libertad de amortización",
        "art. 12.3.a y d LIS, DA 16.ª y 17.ª",
    ),
    c(
        321,
        true,
        "12",
        "Deterioro de créditos no deducible",
        "art. 13.1 LIS",
    ),
    c(
        322,
        false,
        "12",
        "Deterioro de créditos que pasa a ser deducible o revierte",
        "art. 13.1 LIS",
    ),
    c(
        331,
        true,
        "12",
        "Deterioro del inmovilizado material, inversiones inmobiliarias e intangible",
        "art. 13.2.a LIS",
    ),
    c(
        332,
        false,
        "12",
        "Reversión o baja del inmovilizado con deterioro no deducido",
        "art. 13.2.a LIS",
    ),
    c(
        325,
        true,
        "12",
        "Deterioro de participaciones en el capital o fondos propios",
        "art. 13.2.b LIS",
    ),
    c(
        326,
        false,
        "12",
        "Reversión del deterioro de participaciones no deducido",
        "art. 13.2.b LIS",
    ),
    c(
        327,
        true,
        "12",
        "Deterioro de valores representativos de deuda",
        "art. 13.2.c LIS",
    ),
    c(
        328,
        false,
        "12",
        "Deterioro de valores representativos de deuda (reversión)",
        "art. 13.2.c LIS",
    ),
    c(337, true, "12", "Provisiones no deducibles", "art. 14 LIS"),
    c(
        338,
        false,
        "12",
        "Aplicación o exceso de provisiones no deducidas",
        "art. 14 LIS",
    ),
    c(
        368,
        false,
        "12",
        "Subvenciones públicas no integrables",
        "art. 14.8 LIS",
    ),
    c(
        1002,
        true,
        "12",
        "Retribución de fondos propios",
        "art. 15.a LIS",
    ),
    c(
        1815,
        true,
        "12",
        "Multas, sanciones y recargos",
        "art. 15.c LIS",
    ),
    c(
        339,
        true,
        "12",
        "Donativos y liberalidades (atenciones a clientes por encima del 1 % de la cifra de negocios)",
        "art. 15.e LIS",
    ),
    c(
        1816,
        true,
        "12",
        "Gastos de actuaciones contrarias al ordenamiento jurídico",
        "art. 15.f LIS",
    ),
    c(
        508,
        true,
        "12",
        "Gastos financieros de deudas con entidades del grupo",
        "art. 15.h LIS",
    ),
    c(
        1817,
        true,
        "12",
        "Indemnizaciones por extinción de la relación laboral o mercantil (exceso)",
        "art. 15.i LIS",
    ),
    c(
        1807,
        true,
        "12",
        "Deterioro de participaciones",
        "art. 15.k LIS",
    ),
    c(
        1811,
        false,
        "12",
        "Deterioro de participaciones (reversión)",
        "art. 15.k LIS",
    ),
    c(
        363,
        true,
        "12",
        "Limitación de gastos financieros",
        "art. 16 LIS",
    ),
    c(
        364,
        false,
        "12",
        "Gastos financieros no deducidos en ejercicios anteriores",
        "art. 16 LIS",
    ),
    c(
        347,
        true,
        "12",
        "Transmisiones lucrativas y societarias a valor de mercado",
        "art. 17.4 LIS",
    ),
    c(
        348,
        false,
        "12",
        "Transmisiones lucrativas y societarias a valor de mercado",
        "art. 17.4 LIS",
    ),
    c(
        1011,
        true,
        "12",
        "Operaciones vinculadas a valor de mercado",
        "art. 18 LIS",
    ),
    c(
        1012,
        false,
        "12",
        "Operaciones vinculadas a valor de mercado",
        "art. 18 LIS",
    ),
    c(
        370,
        false,
        "13",
        "Exención de dividendos de entidades residentes",
        "art. 21.1 y 21.10 LIS",
    ),
    c(
        2181,
        false,
        "13",
        "Exención de dividendos de entidades no residentes",
        "art. 21.1 y 21.10 LIS",
    ),
    c(
        1764,
        false,
        "13",
        "Exención de dividendos de entidades residentes (filial nueva de una empresa pequeña)",
        "art. 21.11 LIS",
    ),
    c(
        1765,
        false,
        "13",
        "Exención de dividendos de entidades no residentes (filial nueva de una empresa pequeña)",
        "art. 21.11 LIS",
    ),
    c(
        2182,
        true,
        "13",
        "Renta de la transmisión de participaciones de entidades residentes",
        "art. 21.3 y 21.10 LIS",
    ),
    c(
        2183,
        false,
        "13",
        "Exención de la renta de la transmisión de participaciones de entidades residentes",
        "art. 21.3 y 21.10 LIS",
    ),
    c(
        2184,
        true,
        "13",
        "Renta de la transmisión de participaciones de entidades no residentes",
        "art. 21.3 y 21.10 LIS",
    ),
    c(
        2185,
        false,
        "13",
        "Exención de la renta de la transmisión de participaciones de entidades no residentes",
        "art. 21.3 y 21.10 LIS",
    ),
    c(
        256,
        true,
        "13",
        "Exención de rentas en el extranjero",
        "art. 22 LIS",
    ),
    c(
        278,
        false,
        "13",
        "Exención de rentas en el extranjero",
        "art. 22 LIS",
    ),
    c(
        340,
        true,
        "13",
        "Impuesto extranjero no deducible por afectar a rentas con deducción por doble imposición",
        "art. 31.2 LIS",
    ),
    c(
        1589,
        false,
        "13",
        "Impuesto extranjero no deducible por afectar a rentas con deducción por doble imposición",
        "art. 31.2 LIS",
    ),
    erd(
        311,
        true,
        "Libertad de amortización de las empresas de reducida dimensión (reversión)",
        "art. 102 LIS",
    ),
    erd(
        312,
        false,
        "Libertad de amortización de las empresas de reducida dimensión",
        "art. 102 LIS",
    ),
    erd(
        313,
        true,
        "Amortización acelerada de las empresas de reducida dimensión (reversión)",
        "art. 103 LIS",
    ),
    erd(
        314,
        false,
        "Amortización acelerada de las empresas de reducida dimensión",
        "art. 103 LIS",
    ),
    erd(
        323,
        true,
        "Deterioro global de créditos de las empresas de reducida dimensión (reversión)",
        "art. 104 LIS",
    ),
    erd(
        324,
        false,
        "Deterioro global de créditos de las empresas de reducida dimensión",
        "art. 104 LIS",
    ),
    c(
        317,
        true,
        "13",
        "Arrendamiento financiero: régimen especial",
        "art. 106 LIS",
    ),
    c(
        318,
        false,
        "13",
        "Arrendamiento financiero: régimen especial",
        "art. 106 LIS",
    ),
    c(
        250,
        true,
        "13",
        "Aportaciones a entidades sin fines lucrativos",
        "Ley 49/2002",
    ),
    c(
        251,
        false,
        "13",
        "Aportaciones a entidades sin fines lucrativos",
        "Ley 49/2002",
    ),
    c(
        413,
        true,
        "13",
        "Otras correcciones al resultado",
        "art. 10.3 LIS",
    ),
    c(
        414,
        false,
        "13",
        "Otras correcciones al resultado",
        "art. 10.3 LIS",
    ),
];

/// La corrección de la casilla `casilla`, si está en el catálogo.
pub fn correccion(casilla: u16) -> Option<&'static Correccion> {
    CORRECCIONES.iter().find(|x| x.casilla == casilla)
}

/// Casilla de la cuota (bonificación, deducción por doble imposición u otra deducción) o de
/// retenciones desglosadas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct CasillaCuota {
    pub casilla: u16,
    pub grupo: GrupoCuota,
    pub concepto: &'static str,
    pub norma: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GrupoCuota {
    /// Minoran la cuota íntegra antes que las deducciones por doble imposición ([00582]).
    Bonificacion,
    DobleImposicion,
    /// Minoran la cuota íntegra ajustada positiva hasta la cuota líquida ([00592]).
    Deduccion,
    /// Desglose de [01766].
    Retencion,
}

const fn q(
    casilla: u16,
    grupo: GrupoCuota,
    concepto: &'static str,
    norma: &'static str,
) -> CasillaCuota {
    CasillaCuota {
        casilla,
        grupo,
        concepto,
        norma,
    }
}

pub const CUOTA: &[CasillaCuota] = &[
    q(
        567,
        GrupoCuota::Bonificacion,
        "Bonificación por rentas obtenidas en Ceuta y Melilla",
        "art. 33 LIS",
    ),
    q(
        568,
        GrupoCuota::Bonificacion,
        "Bonificación por prestación de servicios públicos locales",
        "art. 34 LIS",
    ),
    q(569, GrupoCuota::Bonificacion, "Otras bonificaciones", "LIS"),
    q(
        571,
        GrupoCuota::DobleImposicion,
        "Doble imposición internacional de periodos anteriores aplicada",
        "arts. 31.6 y 32 LIS",
    ),
    q(
        573,
        GrupoCuota::DobleImposicion,
        "Doble imposición internacional generada y aplicada en el ejercicio",
        "arts. 31 y 32 LIS",
    ),
    q(
        583,
        GrupoCuota::Deduccion,
        "Apoyo fiscal a la inversión y otras deducciones",
        "LIS",
    ),
    q(
        588,
        GrupoCuota::Deduccion,
        "Deducciones para incentivar determinadas actividades (I+D+i y otras)",
        "cap. IV del tít. VI LIS (desglose en las páginas 17 y 18)",
    ),
    q(
        82,
        GrupoCuota::Deduccion,
        "Deducciones de I+D+i excluidas del límite",
        "art. 39.2 LIS (desglose en las páginas 19 y 19 bis)",
    ),
    q(
        565,
        GrupoCuota::Deduccion,
        "Deducción por donaciones a entidades sin fines de lucro",
        "art. 20 Ley 49/2002 (desglose en la página 18 ter)",
    ),
    q(
        1785,
        GrupoCuota::Retencion,
        "Retenciones por rendimientos del capital mobiliario",
        "art. 41 LIS; art. 60 RIS",
    ),
    q(
        1787,
        GrupoCuota::Retencion,
        "Retenciones por arrendamientos de inmuebles urbanos",
        "art. 41 LIS; art. 60 RIS",
    ),
    q(
        1793,
        GrupoCuota::Retencion,
        "Retenciones por otros conceptos (actividades profesionales y otras)",
        "art. 41 LIS; art. 60 RIS",
    ),
    q(
        1795,
        GrupoCuota::Retencion,
        "Retenciones e ingresos a cuenta de participaciones en IIC",
        "art. 41 LIS; art. 60 RIS",
    ),
    q(
        1798,
        GrupoCuota::Retencion,
        "Retenciones por otros conceptos no incluidos en las anteriores",
        "art. 41 LIS; art. 60 RIS",
    ),
];

pub fn casilla_cuota(casilla: u16) -> Option<&'static CasillaCuota> {
    CUOTA.iter().find(|x| x.casilla == casilla)
}

/// Tipos de gravamen por ejercicio de inicio del periodo impositivo (art. 29.1 LIS y DT 44.ª,
/// redacción de la Ley 7/2024, de 20 de diciembre; `docs/research/MODELO_200.md`):
/// (ejercicio desde, micro primer tramo hasta 50.000 €, micro resto, resto de reducida
/// dimensión). Entidades con cifra de negocios < 1 M€ («micro»): dos tramos; el resto de las de
/// reducida dimensión (cifra < 10 M€, art. 101 LIS), tipo único. Cotejado el 2026-11-16 con el
/// texto consolidado de la LIS (BOE-A-2014-12328, a 02-09-2026): 2025, DT 44.ª.1 (21 % y 22 %;
/// 24 %); 2026, DT 44.ª.2 (19 % y 21 %; 23 %); 2027, art. 29.1 (17 % y 20 %) y DT 44.ª.3 (22 %);
/// 2028, DT 44.ª.4 (21 %); desde 2029, art. 29.1 (17 % y 20 %; 20 %). El 23 % de 2023 y 2024 es
/// del art. 68 de la Ley 31/2022 (BOE-A-2022-22128). Nueva creación: 15 % en el primer periodo
/// con base positiva y en el siguiente (art. 29.1). Ninguno de estos tipos se aplica a la
/// entidad patrimonial (art. 29.1, último párrafo).
pub const TIPOS: &[(i32, i32, i32, i32)] = &[
    (i32::MIN, 25, 25, 25),
    (2023, 23, 23, 25),
    (2025, 21, 22, 24),
    (2026, 19, 21, 23),
    (2027, 17, 20, 22),
    (2028, 17, 20, 21),
    (2029, 17, 20, 20),
];

/// (micro primer tramo, micro resto, resto de reducida dimensión) del ejercicio `year`.
pub fn tipos(year: i32) -> (i32, i32, i32) {
    TIPOS
        .iter()
        .rev()
        .find(|(desde, ..)| year >= *desde)
        .map_or((25, 25, 25), |(_, a, b, r)| (*a, *b, *r))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cada_casilla_esta_en_su_pagina_del_dr() {
        let spec = crate::dr::Spec::parse(crate::m200::DR_JSON);
        for x in CORRECCIONES {
            let p = spec
                .page(&format!("DP2000{}", x.pagina))
                .unwrap_or_else(|e| panic!("{e}"));
            let f = p
                .field_for_box(x.casilla)
                .unwrap_or_else(|e| panic!("[{}]: {e}", x.casilla));
            // El DR escribe «Diminuciones» en la [01006].
            let d = f
                .desc
                .to_lowercase()
                .replace("diminuciones", "disminuciones");
            assert!(
                d.contains(if x.aumento {
                    "aumentos"
                } else {
                    "disminuciones"
                }),
                "[{}] {}",
                x.casilla,
                f.desc
            );
        }
        for x in CUOTA {
            let pagina = match x.grupo {
                GrupoCuota::Bonificacion | GrupoCuota::DobleImposicion => "DP200014",
                GrupoCuota::Deduccion | GrupoCuota::Retencion => "DP200014B",
            };
            let p = spec.page(pagina).unwrap_or_else(|e| panic!("{e}"));
            assert!(p.field_for_box(x.casilla).is_ok(), "[{}]", x.casilla);
        }
        // Sin casillas repetidas.
        let mut v: Vec<u16> = CORRECCIONES.iter().map(|x| x.casilla).collect();
        v.extend(CUOTA.iter().map(|x| x.casilla));
        let n = v.len();
        v.sort_unstable();
        v.dedup();
        assert_eq!(v.len(), n);
    }

    #[test]
    fn tipos_por_ejercicio() {
        assert_eq!(tipos(2022), (25, 25, 25));
        assert_eq!(tipos(2024), (23, 23, 25));
        assert_eq!(tipos(2026), (19, 21, 23));
        assert_eq!(tipos(2030), (17, 20, 20));
    }
}
