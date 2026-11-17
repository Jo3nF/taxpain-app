//! Bases, topes y tipos de cotización del Régimen General por año, como datos con su norma.
//!
//! Norma:
//! - 2025: Orden PJC/178/2025, de 25 de febrero (BOE-A-2025-3780), que desarrolla las normas
//!   de cotización de 2025 (art. 19 TRLGSS; bases mínimas por grupo ligadas al SMI de 2025).
//! - 2026: Orden PJC/297/2026 (BOE-A-2026-7296), con el RDL 3/2026 (BOE-A-2026-2548).
//! - Mecanismo de equidad intergeneracional (MEI): DT 43.ª TRLGSS (0,80 % en 2025 y 0,90 % en
//!   2026, repartido entre empresa y trabajador en la proporción de las contingencias comunes).
//! - Cotización adicional de solidaridad (retribución por encima de la base máxima): art. 19 bis
//!   y DT 44.ª TRLGSS (RDL 2/2023).
//!
//! Cotejado con el BOE (tabla en `docs/revisiones/2026-11-verificacion-boe.md`, la del RETA): las
//! bases máximas (4.909,50 € y 5.101,20 €), la base mínima del grupo 7 de 2026 (1.424,40 €) y el
//! MEI de cada año. El resto lo dice cada tabla en `pendiente`, y la pantalla lo enseña: son los
//! valores que TaxPain conoce de la orden de cotización, sin haberlos cotejado con su texto en
//! esta versión (`TODO(VERIFY)`).
//!
//! La tarifa de accidentes de trabajo y enfermedades profesionales (AT/EP) depende de la
//! actividad (CNAE) o de la ocupación del trabajador (DA 4.ª de la Ley 42/2006, en la redacción
//! vigente): no está aquí, la teclea el usuario (IT e IMS) con la de su CNAE.

use serde::Serialize;

use crate::money::{Money, Rate};

/// Base mínima de un grupo de cotización: mensual (grupos 1 a 7) o diaria (8 a 11).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "periodo", content = "importe", rename_all = "lowercase")]
pub enum BaseMinima {
    Mensual(Money),
    Diaria(Money),
}

/// Tipo de cotización a cargo de la empresa y del trabajador.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Reparto {
    pub empresa: Rate,
    pub trabajador: Rate,
}

impl Reparto {
    pub fn total(self) -> Rate {
        Rate::from_bp(self.empresa.bp() + self.trabajador.bp())
    }
}

const fn rep(empresa: i32, trabajador: i32) -> Reparto {
    Reparto {
        empresa: Rate::from_bp(empresa),
        trabajador: Rate::from_bp(trabajador),
    }
}

/// Tipos del Régimen General de un año.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Tipos {
    /// Contingencias comunes.
    pub comunes: Reparto,
    /// Desempleo con contrato indefinido (incluidos fijos discontinuos y formativos).
    pub desempleo_indefinido: Reparto,
    /// Desempleo con contrato de duración determinada.
    pub desempleo_temporal: Reparto,
    /// Fondo de Garantía Salarial (sólo empresa).
    pub fogasa: Reparto,
    /// Formación profesional.
    pub formacion: Reparto,
    /// Mecanismo de equidad intergeneracional.
    pub mei: Reparto,
    /// Horas extraordinarias por fuerza mayor.
    pub horas_fuerza_mayor: Reparto,
    /// Resto de horas extraordinarias.
    pub horas_resto: Reparto,
    /// Cotización adicional de solidaridad: tipo de cada tramo de la retribución que excede la
    /// base máxima (hasta el 10 %, del 10 al 50 % y por encima del 50 %).
    pub solidaridad: [Rate; 3],
}

/// Tabla de cotización de un año.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct TablaCotizacion {
    pub year: i32,
    /// Base máxima mensual (tope de todos los grupos).
    pub base_maxima: Money,
    /// Bases mínimas de los grupos 1 a 11 (índice 0 = grupo 1).
    pub bases_minimas: [BaseMinima; 11],
    pub tipos: Tipos,
    pub norma: &'static str,
    /// Lo que no se ha cotejado con el BOE (vacío si está todo verificado).
    pub pendiente: &'static [&'static str],
}

impl TablaCotizacion {
    /// Base mínima del grupo (1 a 11).
    pub fn base_minima(&self, grupo: u8) -> Option<BaseMinima> {
        let i = usize::from(grupo.checked_sub(1)?);
        self.bases_minimas.get(i).copied()
    }
}

const fn eur(e: i64, c: i64) -> Money {
    Money::from_cents(e * 100 + c)
}

use BaseMinima::{Diaria, Mensual};

/// Tipos comunes a 2025 y 2026 salvo el MEI y la solidaridad (Orden PJC/178/2025, arts. 4 y
/// 32 a 34; Orden PJC/297/2026, mismos artículos). `TODO(VERIFY)`: cotejar cada tipo con el
/// texto de la orden de cada año.
const fn tipos(mei: Reparto, solidaridad: [Rate; 3]) -> Tipos {
    Tipos {
        comunes: rep(2360, 470),
        desempleo_indefinido: rep(550, 155),
        desempleo_temporal: rep(670, 160),
        fogasa: rep(20, 0),
        formacion: rep(60, 10),
        mei,
        horas_fuerza_mayor: rep(1200, 200),
        horas_resto: rep(2360, 470),
        solidaridad,
    }
}

pub const TABLA_2025: TablaCotizacion = TablaCotizacion {
    year: 2025,
    base_maxima: eur(4909, 50),
    bases_minimas: [
        Mensual(eur(1929, 0)),
        Mensual(eur(1599, 60)),
        Mensual(eur(1391, 70)),
        Mensual(eur(1381, 20)),
        Mensual(eur(1381, 20)),
        Mensual(eur(1381, 20)),
        Mensual(eur(1381, 20)),
        Diaria(eur(46, 4)),
        Diaria(eur(46, 4)),
        Diaria(eur(46, 4)),
        Diaria(eur(46, 4)),
    ],
    // MEI 0,80 %: 0,67 empresa y 0,13 trabajador. Solidaridad de 2025: 0,92 %, 1,00 % y 1,17 %.
    tipos: tipos(
        rep(67, 13),
        [Rate::from_bp(92), Rate::from_bp(100), Rate::from_bp(117)],
    ),
    norma: "Orden PJC/178/2025 (BOE-A-2025-3780); DT 43.ª y art. 19 bis TRLGSS",
    pendiente: &[
        "Bases mínimas de los grupos 1 a 11 de 2025 (art. 3 de la Orden PJC/178/2025)",
        "Tipos de contingencias comunes, desempleo, FOGASA, formación profesional y horas \
         extraordinarias de 2025 (arts. 4 y 32 a 34 de la orden)",
        "Reparto del MEI de 2025 entre empresa (0,67 %) y trabajador (0,13 %)",
        "Tipos de la cotización adicional de solidaridad de 2025 (DT 44.ª TRLGSS)",
    ],
};

pub const TABLA_2026: TablaCotizacion = TablaCotizacion {
    year: 2026,
    base_maxima: eur(5101, 20),
    bases_minimas: [
        // Grupos 1 a 3: las de 2025 subidas como el SMI (TODO(VERIFY) con el art. 3 de la Orden
        // PJC/297/2026: no cotejadas).
        Mensual(eur(1988, 70)),
        Mensual(eur(1649, 40)),
        Mensual(eur(1434, 90)),
        Mensual(eur(1424, 40)),
        Mensual(eur(1424, 40)),
        Mensual(eur(1424, 40)),
        Mensual(eur(1424, 40)),
        Diaria(eur(47, 48)),
        Diaria(eur(47, 48)),
        Diaria(eur(47, 48)),
        Diaria(eur(47, 48)),
    ],
    // MEI 0,90 %: 0,75 empresa y 0,15 trabajador.
    tipos: tipos(
        rep(75, 15),
        [Rate::from_bp(115), Rate::from_bp(125), Rate::from_bp(146)],
    ),
    norma: "Orden PJC/297/2026 (BOE-A-2026-7296) y RDL 3/2026; DT 43.ª y art. 19 bis TRLGSS",
    pendiente: &[
        "Bases mínimas de los grupos 1, 2 y 3 de 2026 (art. 3 de la Orden PJC/297/2026): \
         estimadas con la subida del SMI",
        "Base mínima diaria de los grupos 8 a 11 de 2026 (la mensual del grupo 7 entre 30)",
        "Tipos de contingencias comunes, desempleo, FOGASA, formación profesional y horas \
         extraordinarias de 2026 (los de 2025)",
        "Reparto del MEI de 2026 entre empresa (0,75 %) y trabajador (0,15 %)",
        "Tipos de la cotización adicional de solidaridad de 2026 (DT 44.ª TRLGSS)",
    ],
};

/// Tabla del año, si TaxPain la tiene. Sin norma de un año (p. ej. 2027 sin orden de
/// cotización), `None`: la pantalla lo dice y no calcula.
pub fn tabla(year: i32) -> Option<&'static TablaCotizacion> {
    match year {
        2025 => Some(&TABLA_2025),
        2026 => Some(&TABLA_2026),
        _ => None,
    }
}

/// Años con tabla.
pub const ANOS: [i32; 2] = [2025, 2026];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tablas_coherentes() {
        for y in ANOS {
            let t = tabla(y).expect("tabla");
            assert_eq!(t.year, y);
            for g in 1..=11u8 {
                let m = match t.base_minima(g).expect("grupo") {
                    Mensual(m) => m,
                    Diaria(d) => Money::from_cents(d.cents() * 30),
                };
                assert!(m < t.base_maxima, "{y} grupo {g}");
            }
            assert!(t.base_minima(0).is_none() && t.base_minima(12).is_none());
            assert!(!t.pendiente.is_empty());
        }
        // Lo cotejado con el BOE.
        assert_eq!(TABLA_2025.base_maxima, eur(4909, 50));
        assert_eq!(TABLA_2026.base_maxima, eur(5101, 20));
        assert_eq!(TABLA_2026.base_minima(7), Some(Mensual(eur(1424, 40))));
        assert_eq!(TABLA_2025.tipos.mei.total(), Rate::from_bp(80));
        assert_eq!(TABLA_2026.tipos.mei.total(), Rate::from_bp(90));
        assert!(tabla(2027).is_none());
    }
}
