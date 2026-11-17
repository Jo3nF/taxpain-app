//! Modelo 100 (Renta), sólo la parte de la actividad económica en estimación directa: rendimiento
//! neto, reducciones del art. 32 LIRPF y su traslado a las casillas. Es una ayuda para rellenar
//! Renta Web; TaxPain no presenta la Renta.
//!
//! Base legal: arts. 28 a 32 Ley 35/2006 (LIRPF); arts. 28 a 30 RD 439/2007 (RIRPF).
//! - Simplificada: provisiones y gastos de difícil justificación = 5 % del rendimiento neto
//!   positivo previo (sin este concepto), con el límite de 2.000 € anuales: habilitación en el
//!   art. 30.2.4.ª LIRPF y cuantía en el art. 30.2.ª RIRPF (redacción del RD 633/2015, vigente
//!   en el texto consolidado del BOE actualizado el 07/10/2026). En 2023 fue el 7 % (DA 56.ª
//!   LIRPF, Ley 31/2022).
//! - Art. 32.2.1.º y 2.º: reducción de 2.000 € y, con rendimientos netos bajos, la adicional.
//! - Art. 32.2.3.º: rendimientos netos inferiores a 12.000 €.
//! - Art. 32.3: 20 % por inicio de actividad (primer año con rendimiento positivo y el siguiente;
//!   sobre un máximo de 100.000 €).
//!
//! Casillas: anexo I de la Orden HAC/277/2026 (modelo D-100 de Renta 2025, BOE-A-2026-7041,
//! apartado D1 «Rendimientos de actividades económicas en estimación directa»), cotejado el
//! 2026-10-27: ingresos [0171]-[0180]; gastos [0181]-[0217] con la suma en [0218]; normal
//! [0219]/[0220]; simplificada [0221] diferencia, [0222] provisiones y difícil justificación,
//! [0223]; [0224] rendimiento neto; [0225] irregulares y [0236] Copa América; [0226] = [0224] −
//! [0225] − [0236]; [0231] suma de las [0226]; [0232] art. 32.2.1.º; [0233] art. 32.2.3.º;
//! [0234] art. 32.3; [0237] actividades artísticas (DA 60.ª); [0235] = [0231] − [0232] − [0233]
//! − [0234] − [0237]. Retenciones de la actividad en la [0599] y pagos fraccionados en la [0604]
//! (ayuda de Renta Web 2025, apartado 7.4.2.9). Los conceptos que TaxPain agrupa y en el modelo
//! van en varias casillas se dejan sin número (`code: None`) con la pista en la etiqueta.

use serde::{Deserialize, Serialize};
use taxpain_core::{Money, Rate};

/// Modalidad del método de estimación directa del IRPF (art. 28 RIRPF).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum IrpfRegime {
    /// Normal: obligatoria si la cifra de negocios del año anterior supera 600.000 € o si se
    /// renuncia a la simplificada.
    Normal,
    /// Simplificada: la de la mayoría de los autónomos.
    #[default]
    Simplificada,
}

/// Límite anual de provisiones y gastos de difícil justificación (art. 30.2.ª RIRPF).
pub const MAX_HARD_TO_JUSTIFY: Money = Money::from_cents(200_000);

/// Porcentaje de provisiones y gastos de difícil justificación del ejercicio.
pub fn hard_to_justify_rate(year: i32) -> Rate {
    // 7 % sólo en 2023 (DA 56.ª LIRPF, Ley 31/2022); 5 % en el resto (art. 30.2.ª RIRPF).
    // Comprobado el 2026-10-27 en el texto consolidado del BOE (RD 439/2007, actualizado el
    // 07/10/2026): el 5 % y el tope de 2.000 € siguen vigentes; el modelo de Renta 2025 remite a
    // ese artículo en la nota de la casilla [0222].
    if year == 2023 {
        Rate::percent(7)
    } else {
        Rate::percent(5)
    }
}

/// Provisiones y gastos de difícil justificación de la simplificada sobre el rendimiento neto
/// previo `net_before` (ingresos menos gastos, sin este concepto): 0 si no es positivo.
pub fn hard_to_justify(year: i32, net_before: Money) -> Money {
    if net_before.cents() <= 0 {
        return Money::ZERO;
    }
    net_before
        .apply_rate(hard_to_justify_rate(year))
        .min(MAX_HARD_TO_JUSTIFY)
}

/// Reducción del art. 32.2 LIRPF que el contribuyente dice cumplir.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reduction32 {
    #[default]
    Ninguna,
    /// Art. 32.2.1.º: autónomo económicamente dependiente o con un único cliente no vinculado,
    /// sin rendimientos del trabajo, gastos que no pasan del 30 % de los ingresos, sin
    /// retención en menos del 70 % de los ingresos… (requisitos del art. 32.2.1.º).
    Dependiente,
    /// Art. 32.2.3.º: rendimientos netos de actividades inferiores a 12.000 € y otras rentas
    /// (sin las exentas) no superiores a 6.500 €.
    RentasBajas,
}

/// Gastos de la actividad por conceptos del modelo 100.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Expenses {
    /// Consumos de explotación (compras y variación de existencias).
    pub consumption: Money,
    /// Sueldos y salarios.
    pub wages: Money,
    /// Seguridad Social a cargo de la empresa (de los trabajadores).
    pub employer_ss: Money,
    /// Cuotas de autónomo (RETA) o mutualidad alternativa del titular.
    pub owner_ss: Money,
    /// Otros gastos de personal.
    pub other_staff: Money,
    /// Arrendamientos y cánones.
    pub rentals: Money,
    /// Reparaciones y conservación.
    pub repairs: Money,
    /// Servicios de profesionales independientes.
    pub professionals: Money,
    /// Otros servicios exteriores (transportes, seguros, bancos, publicidad…).
    pub other_services: Money,
    /// Suministros (luz, agua, teléfono…).
    pub supplies: Money,
    /// Tributos fiscalmente deducibles.
    pub taxes: Money,
    /// Gastos financieros.
    pub financial: Money,
    /// Dotaciones para amortización del inmovilizado.
    pub depreciation: Money,
    /// Otros conceptos fiscalmente deducibles.
    pub other: Money,
}

impl Expenses {
    pub fn total(&self) -> Money {
        [
            self.consumption,
            self.wages,
            self.employer_ss,
            self.owner_ss,
            self.other_staff,
            self.rentals,
            self.repairs,
            self.professionals,
            self.other_services,
            self.supplies,
            self.taxes,
            self.financial,
            self.depreciation,
            self.other,
        ]
        .into_iter()
        .sum()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Input100 {
    pub regime: IrpfRegime,
    /// Ingresos de explotación (ventas y prestaciones de servicios).
    pub operating_income: Money,
    /// Otros ingresos de la actividad (subvenciones, autoconsumo, otros).
    pub other_income: Money,
    pub expenses: Expenses,
    /// Provisiones deducibles (sólo en la modalidad normal).
    pub provisions: Money,
    pub reduction_32_2: Reduction32,
    /// Otras rentas (sin las exentas) de más de 6.500 €: sin la reducción adicional.
    pub other_income_over_6500: bool,
    /// Reducción del 20 % por inicio de actividad (art. 32.3 LIRPF).
    pub activity_start: bool,
    /// Retenciones e ingresos a cuenta soportados por la actividad.
    pub withholdings: Money,
    /// Pagos fraccionados ingresados (modelo 130) del ejercicio.
    pub installments: Money,
}

/// Una casilla del modelo 100.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Box100 {
    /// Número de casilla, si lo hemos podido confirmar.
    pub code: Option<String>,
    pub label: String,
    pub amount: Money,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Result100 {
    pub year: i32,
    pub regime: IrpfRegime,
    pub income: Money,
    pub expenses: Money,
    /// Ingresos menos gastos, antes de provisiones y gastos de difícil justificación.
    pub net_before: Money,
    /// Simplificada: provisiones y gastos de difícil justificación; normal: provisiones.
    pub hard_to_justify: Money,
    /// Rendimiento neto de la actividad.
    pub net: Money,
    pub reduction_32_2: Money,
    pub reduction_start: Money,
    /// Rendimiento neto reducido total de la estimación directa.
    pub net_reduced: Money,
    pub withholdings: Money,
    pub installments: Money,
    pub boxes: Vec<Box100>,
    pub warnings: Vec<String>,
}

fn euros(e: i64, c: u8) -> Money {
    Money::from_euros(e, c)
}

/// Reducción del art. 32.2 LIRPF sobre el rendimiento neto `net` (redacción de la Ley 31/2022,
/// desde 2023).
pub fn reduction_32_2(kind: Reduction32, net: Money, other_income_over_6500: bool) -> Money {
    if net.cents() <= 0 {
        return Money::ZERO;
    }
    let r = match kind {
        Reduction32::Ninguna => Money::ZERO,
        Reduction32::Dependiente => {
            // a) 2.000 € y b) adicional con rendimientos netos inferiores a 19.747,50 € si las
            // demás rentas no pasan de 6.500 €.
            let extra = if other_income_over_6500 {
                Money::ZERO
            } else if net <= euros(14_047, 50) {
                euros(6_498, 0)
            } else if net < euros(19_747, 50) {
                euros(6_498, 0) - (net - euros(14_047, 50)).apply_rate(Rate::from_bp(11_400))
            } else {
                Money::ZERO
            };
            euros(2_000, 0) + extra.max(Money::ZERO)
        }
        Reduction32::RentasBajas => {
            if other_income_over_6500 {
                Money::ZERO
            } else if net <= euros(8_000, 0) {
                euros(1_620, 0)
            } else if net < euros(12_000, 0) {
                euros(1_620, 0) - (net - euros(8_000, 0)).apply_rate(Rate::from_bp(4_050))
            } else {
                Money::ZERO
            }
        }
    };
    // La reducción no puede dejar el rendimiento negativo.
    r.max(Money::ZERO).min(net)
}

/// Límite del rendimiento neto sobre el que se aplica la reducción por inicio de actividad.
pub const MAX_START_BASE: Money = Money::from_cents(10_000_000);

pub fn compute(year: i32, i: &Input100) -> Result100 {
    let mut warnings = Vec::new();
    let income = i.operating_income + i.other_income;
    let expenses = i.expenses.total();
    let net_before = income - expenses;
    let (hard, total_expenses) = match i.regime {
        IrpfRegime::Simplificada => {
            if !i.provisions.is_zero() {
                warnings.push(
                    "En simplificada las provisiones no se deducen aparte: ya están en el 5 % de \
                     gastos de difícil justificación"
                        .into(),
                );
            }
            let h = hard_to_justify(year, net_before);
            (h, expenses + h)
        }
        IrpfRegime::Normal => (i.provisions, expenses + i.provisions),
    };
    let net = income - total_expenses;
    // Art. 32.1 (rendimientos irregulares, 30 %): no se calcula.
    let net_reduced_activity = net;
    let red_32_2 = reduction_32_2(i.reduction_32_2, net, i.other_income_over_6500);
    if i.reduction_32_2 == Reduction32::Dependiente {
        warnings.push(
            "Reducción del art. 32.2.1.º: compruébala con los requisitos (cliente único o \
             autónomo dependiente, sin rentas del trabajo, gastos de hasta el 30 % de los \
             ingresos, retención en al menos el 70 % de los ingresos…)"
                .into(),
        );
    }
    let after_32_2 = net_reduced_activity - red_32_2;
    let red_start = if i.activity_start && after_32_2.cents() > 0 {
        after_32_2.min(MAX_START_BASE).apply_rate(Rate::percent(20))
    } else {
        Money::ZERO
    };
    if i.activity_start {
        warnings.push(
            "Reducción por inicio de actividad (art. 32.3 LIRPF): sólo el primer año con \
             rendimiento positivo y el siguiente, y no si más de la mitad de los ingresos vienen \
             de quien fue tu empleador el año anterior"
                .into(),
        );
    }
    let net_reduced = after_32_2 - red_start;

    let b = |code: Option<&str>, label: &str, amount: Money| Box100 {
        code: code.map(str::to_owned),
        label: label.to_owned(),
        amount,
    };
    let e = &i.expenses;
    let mut boxes = vec![
        b(Some("0171"), "Ingresos de explotación", i.operating_income),
        b(
            None,
            "Otros ingresos (subvenciones [0173], autoconsumo [0175], otros [0178])",
            i.other_income,
        ),
        b(Some("0180"), "Total ingresos computables", income),
        b(
            None,
            "Consumos de explotación (compras [0181], variación de existencias [0182], otros \
             [0183])",
            e.consumption,
        ),
        b(Some("0184"), "Sueldos y salarios", e.wages),
        b(
            Some("0185"),
            "Seguridad Social a cargo de la empresa",
            e.employer_ss,
        ),
        b(
            Some("0186"),
            "Seguridad Social del titular (cuotas de autónomo; una mutualidad alternativa va en \
             la [0195])",
            e.owner_ss,
        ),
        b(Some("0190"), "Otros gastos de personal", e.other_staff),
        b(Some("0192"), "Arrendamientos y cánones", e.rentals),
        b(Some("0193"), "Reparaciones y conservación", e.repairs),
        b(
            Some("0199"),
            "Servicios de profesionales independientes",
            e.professionals,
        ),
        b(Some("0202"), "Otros servicios exteriores", e.other_services),
        b(
            Some("0194"),
            "Suministros (electricidad, agua, gas, telefonía e internet)",
            e.supplies,
        ),
        b(
            Some("0206"),
            "Otros tributos fiscalmente deducibles",
            e.taxes,
        ),
        b(Some("0203"), "Gastos financieros", e.financial),
        b(
            None,
            "Dotaciones para amortización del inmovilizado (material [0208], intangible [0227])",
            e.depreciation,
        ),
        b(
            Some("0217"),
            "Otros conceptos fiscalmente deducibles",
            e.other,
        ),
        b(Some("0218"), "Suma de gastos", expenses),
    ];
    match i.regime {
        IrpfRegime::Normal => boxes.extend([
            b(
                Some("0219"),
                "Provisiones fiscalmente deducibles",
                i.provisions,
            ),
            b(Some("0220"), "Total gastos deducibles", total_expenses),
        ]),
        IrpfRegime::Simplificada => boxes.extend([
            b(Some("0221"), "Diferencia (ingresos − gastos)", net_before),
            b(
                Some("0222"),
                "Provisiones deducibles y gastos de difícil justificación",
                hard,
            ),
            b(Some("0223"), "Total gastos deducibles", total_expenses),
        ]),
    }
    boxes.extend([
        b(Some("0224"), "Rendimiento neto", net),
        b(
            Some("0226"),
            "Rendimiento neto reducido",
            net_reduced_activity,
        ),
        b(
            match i.reduction_32_2 {
                Reduction32::Dependiente => Some("0232"),
                Reduction32::RentasBajas => Some("0233"),
                Reduction32::Ninguna => None,
            },
            "Reducción por el ejercicio de determinadas actividades (art. 32.2)",
            red_32_2,
        ),
        b(
            Some("0234"),
            "Reducción por inicio de una actividad económica (art. 32.3)",
            red_start,
        ),
        b(
            Some("0235"),
            "Rendimiento neto reducido total de la estimación directa",
            net_reduced,
        ),
        b(
            Some("0599"),
            "Retenciones e ingresos a cuenta de actividades económicas (cuota diferencial)",
            i.withholdings,
        ),
        b(
            Some("0604"),
            "Pagos fraccionados de actividades económicas, modelo 130 (cuota diferencial)",
            i.installments,
        ),
    ]);
    if net.cents() < 0 {
        warnings.push(
            "Rendimiento neto negativo: se compensa con el resto de rentas de la base general \
             en Renta Web"
                .into(),
        );
    }
    Result100 {
        year,
        regime: i.regime,
        income,
        expenses: total_expenses,
        net_before,
        hard_to_justify: hard,
        net,
        reduction_32_2: red_32_2,
        reduction_start: red_start,
        net_reduced,
        withholdings: i.withholdings,
        installments: i.installments,
        boxes,
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eur(e: i64) -> Money {
        Money::from_euros(e, 0)
    }

    #[test]
    fn cinco_por_ciento_con_tope() {
        assert_eq!(hard_to_justify(2026, eur(20_000)), eur(1_000));
        assert_eq!(hard_to_justify(2026, eur(100_000)), eur(2_000));
        assert_eq!(hard_to_justify(2026, -eur(5_000)), Money::ZERO);
        assert_eq!(hard_to_justify(2023, eur(20_000)), eur(1_400));
    }

    #[test]
    fn reducciones_del_32_2() {
        use Reduction32::*;
        assert_eq!(reduction_32_2(Dependiente, eur(10_000), false), eur(8_498));
        // 2.000 + 6.498 − 1,14 × (16.047,50 − 14.047,50) = 6.218.
        assert_eq!(
            reduction_32_2(Dependiente, Money::from_euros(16_047, 50), false),
            eur(6_218)
        );
        assert_eq!(reduction_32_2(Dependiente, eur(30_000), false), eur(2_000));
        assert_eq!(reduction_32_2(Dependiente, eur(10_000), true), eur(2_000));
        assert_eq!(reduction_32_2(Dependiente, eur(1_000), false), eur(1_000));
        assert_eq!(reduction_32_2(RentasBajas, eur(7_000), false), eur(1_620));
        // 1.620 − 0,405 × 2.000 = 810.
        assert_eq!(reduction_32_2(RentasBajas, eur(10_000), false), eur(810));
        assert_eq!(reduction_32_2(RentasBajas, eur(12_000), false), Money::ZERO);
        assert_eq!(reduction_32_2(Ninguna, eur(5_000), false), Money::ZERO);
    }

    #[test]
    fn rendimiento_simplificada_y_normal() {
        let mut i = Input100 {
            operating_income: eur(30_000),
            expenses: Expenses {
                owner_ss: eur(3_000),
                other: eur(7_000),
                ..Expenses::default()
            },
            withholdings: eur(4_500),
            installments: eur(800),
            ..Input100::default()
        };
        let r = compute(2026, &i);
        assert_eq!(r.net_before, eur(20_000));
        assert_eq!(r.hard_to_justify, eur(1_000));
        assert_eq!(r.net, eur(19_000));
        assert_eq!(r.net_reduced, eur(19_000));
        let code = |r: &Result100, c: &str| {
            r.boxes
                .iter()
                .find(|b| b.code.as_deref() == Some(c))
                .map(|b| b.amount)
        };
        assert_eq!(code(&r, "0222"), Some(eur(1_000)));
        assert_eq!(code(&r, "0223"), Some(eur(11_000)));
        assert_eq!(code(&r, "0186"), Some(eur(3_000)));
        assert_eq!(code(&r, "0219"), None);
        // Casillas del anexo I de la Orden HAC/277/2026 (Renta 2025).
        assert_eq!(code(&r, "0171"), Some(eur(30_000)));
        assert_eq!(code(&r, "0180"), Some(eur(30_000)));
        assert_eq!(code(&r, "0217"), Some(eur(7_000)));
        assert_eq!(code(&r, "0218"), Some(eur(10_000)));
        assert_eq!(code(&r, "0224"), Some(eur(19_000)));
        assert_eq!(code(&r, "0226"), Some(eur(19_000)));
        assert_eq!(code(&r, "0235"), Some(eur(19_000)));
        assert_eq!(code(&r, "0599"), Some(eur(4_500)));
        assert_eq!(code(&r, "0604"), Some(eur(800)));
        assert_eq!(code(&r, "0232"), None);
        assert_eq!(code(&r, "0233"), None);
        assert_eq!(code(&r, "0234"), Some(Money::ZERO));

        i.regime = IrpfRegime::Normal;
        i.activity_start = true;
        let r = compute(2026, &i);
        assert_eq!(r.net, eur(20_000));
        assert_eq!(r.reduction_start, eur(4_000));
        assert_eq!(r.net_reduced, eur(16_000));
        assert_eq!(code(&r, "0220"), Some(eur(10_000)));
        assert_eq!(code(&r, "0222"), None);
        assert_eq!(code(&r, "0234"), Some(eur(4_000)));

        // La reducción del art. 32.2 va a la [0232] (1.º) o a la [0233] (3.º) según el caso.
        i.activity_start = false;
        i.reduction_32_2 = Reduction32::RentasBajas;
        i.operating_income = eur(17_000);
        let r = compute(2026, &i);
        assert_eq!(code(&r, "0233"), Some(r.reduction_32_2));
        assert_eq!(code(&r, "0232"), None);
        i.reduction_32_2 = Reduction32::Dependiente;
        let r = compute(2026, &i);
        assert_eq!(code(&r, "0232"), Some(r.reduction_32_2));
        assert_eq!(code(&r, "0233"), None);
    }
}
