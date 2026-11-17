//! Bases, cuotas y total de una factura, en céntimos.
//!
//! Base de línea = cantidad × precio − descuento, redondeada al céntimo (half away from zero).
//! Cuotas: por defecto se agrupan las bases por tipo y se aplica el tipo a la suma
//! (`RoundingMode::PerRate`); alternativamente, línea a línea (`PerLine`).
//! `TODO(VERIFY)`: criterio de redondeo admitido por la AEAT (ver `docs/INVOICING_RULES.md`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::model::{InvoiceLine, VatTreatment};
use crate::money::{Money, Rate};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoundingMode {
    #[default]
    PerRate,
    PerLine,
}

/// Una fila del desglose: tratamiento + recargo, con su base y cuotas.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxBreakdown {
    pub vat: VatTreatment,
    pub surcharge_rate: Option<Rate>,
    pub base: Money,
    pub vat_amount: Money,
    pub surcharge_amount: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    pub breakdown: Vec<TaxBreakdown>,
    /// Retenciones por tipo: (tipo, base, importe).
    pub withholdings: Vec<(Rate, Money, Money)>,
    pub base: Money,
    pub vat: Money,
    pub surcharge: Money,
    pub withholding: Money,
    /// `CuotaTotal` de VERI*FACTU: IVA + recargo de equivalencia.
    pub tax_total: Money,
    /// `ImporteTotal` de VERI*FACTU: base + IVA + recargo (sin restar retenciones).
    pub invoice_total: Money,
    /// A cobrar: total − retenciones.
    pub payable: Money,
    /// IVA de otro Estado miembro (ventanilla única, OSS): ya sumado a `invoice_total` y
    /// `payable`, no a `vat` ni a `tax_total` (ver [`super::oss`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oss: Option<super::oss::OssVat>,
}

/// Base de una línea: `quantity_milli/1000 × unit_price × (1 − descuento)`, al céntimo.
pub fn line_base(l: &InvoiceLine) -> Money {
    let gross = Money::from_cents(round_div(
        i128::from(l.quantity_milli) * i128::from(l.unit_price.cents()),
        1000,
    ));
    gross - gross.apply_rate(l.discount)
}

fn round_div(num: i128, den: i128) -> i64 {
    let q = num / den;
    let r = num % den;
    let q = if 2 * r.abs() >= den {
        q + num.signum()
    } else {
        q
    };
    i64::try_from(q).expect("importe fuera de rango")
}

fn vat_rate(t: VatTreatment) -> Option<Rate> {
    match t {
        VatTreatment::Taxed { rate } => Some(rate),
        _ => None,
    }
}

pub fn compute_totals(lines: &[InvoiceLine], mode: RoundingMode) -> Totals {
    // (tratamiento, recargo) → (base, cuota IVA por línea, cuota RE por línea)
    let mut groups: BTreeMap<(VatTreatment, Option<Rate>), (Money, Money, Money)> = BTreeMap::new();
    let mut wh: BTreeMap<Rate, (Money, Money)> = BTreeMap::new();
    for l in lines {
        let base = line_base(l);
        let rate = vat_rate(l.vat);
        let e = groups.entry((l.vat, l.surcharge)).or_default();
        e.0 += base;
        e.1 += rate.map_or(Money::ZERO, |r| base.apply_rate(r));
        e.2 += l.surcharge.map_or(Money::ZERO, |r| base.apply_rate(r));
        if let Some(r) = l.withholding {
            let w = wh.entry(r).or_default();
            w.0 += base;
            w.1 += base.apply_rate(r);
        }
    }
    let breakdown: Vec<TaxBreakdown> = groups
        .into_iter()
        .map(|((vat, surcharge_rate), (base, line_vat, line_re))| {
            let (vat_amount, surcharge_amount) = match mode {
                RoundingMode::PerLine => (line_vat, line_re),
                RoundingMode::PerRate => (
                    vat_rate(vat).map_or(Money::ZERO, |r| base.apply_rate(r)),
                    surcharge_rate.map_or(Money::ZERO, |r| base.apply_rate(r)),
                ),
            };
            TaxBreakdown {
                vat,
                surcharge_rate,
                base,
                vat_amount,
                surcharge_amount,
            }
        })
        .collect();
    let withholdings: Vec<(Rate, Money, Money)> = wh
        .into_iter()
        .map(|(r, (base, per_line))| {
            let amount = match mode {
                RoundingMode::PerLine => per_line,
                RoundingMode::PerRate => base.apply_rate(r),
            };
            (r, base, amount)
        })
        .collect();
    let base: Money = breakdown.iter().map(|b| b.base).sum();
    let vat: Money = breakdown.iter().map(|b| b.vat_amount).sum();
    let surcharge: Money = breakdown.iter().map(|b| b.surcharge_amount).sum();
    let withholding: Money = withholdings.iter().map(|w| w.2).sum();
    let tax_total = vat + surcharge;
    let invoice_total = base + tax_total;
    Totals {
        breakdown,
        withholdings,
        base,
        vat,
        surcharge,
        withholding,
        tax_total,
        invoice_total,
        payable: invoice_total - withholding,
        oss: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invoice::model::Exemption;
    use proptest::prelude::*;

    fn line(qty: i64, price: i64, vat: VatTreatment) -> InvoiceLine {
        InvoiceLine {
            description: "x".into(),
            quantity_milli: qty * 1000,
            unit_price: Money::from_cents(price),
            discount: Rate::ZERO,
            vat,
            surcharge: None,
            withholding: None,
            advance: false,
        }
    }

    const IVA21: VatTreatment = VatTreatment::Taxed {
        rate: Rate::percent(21),
    };

    #[test]
    fn autonomo_profesional_con_retencion() {
        // 1.000 € de honorarios, IVA 21 %, IRPF 15 %: total 1.210, a cobrar 1.060.
        let mut l = line(1, 100_000, IVA21);
        l.withholding = Some(Rate::percent(15));
        let t = compute_totals(&[l], RoundingMode::PerRate);
        assert_eq!(t.base, Money::from_euros(1000, 0));
        assert_eq!(t.vat, Money::from_euros(210, 0));
        assert_eq!(t.withholding, Money::from_euros(150, 0));
        assert_eq!(t.invoice_total, Money::from_euros(1210, 0));
        assert_eq!(t.payable, Money::from_euros(1060, 0));
        assert_eq!(t.tax_total, Money::from_euros(210, 0));
    }

    #[test]
    fn varios_tipos_recargo_exenta_y_descuento() {
        let mut a = line(3, 1999, IVA21); // 59,97
        a.discount = Rate::percent(10); // −6,00 (5,997) → 53,97
        let mut b = line(
            2,
            1050,
            VatTreatment::Taxed {
                rate: Rate::percent(10),
            },
        ); // 21,00
        b.surcharge = Some(Rate::from_bp(140)); // RE 1,4 % → 0,29 (0,294)
        let c = line(1, 5000, VatTreatment::Exempt(Exemption::E1));
        let t = compute_totals(&[a, b, c], RoundingMode::PerRate);
        assert_eq!(t.breakdown.len(), 3);
        assert_eq!(t.base, Money::from_cents(5397 + 2100 + 5000));
        assert_eq!(t.vat, Money::from_cents(1133 + 210)); // 53,97×21 % = 11,3337 → 11,33
        assert_eq!(t.surcharge, Money::from_cents(29));
        assert_eq!(t.invoice_total, Money::from_cents(12497 + 1343 + 29));
    }

    #[test]
    fn redondeo_por_tipo_y_por_linea_difieren() {
        // 3 líneas de 0,05 € al 21 %: por línea 0,01×3 = 0,03; por tipo 0,15×21 % = 0,03.
        // 3 líneas de 0,02 €: por línea 0,00×3 = 0; por tipo 0,06×21 % = 0,0126 → 0,01.
        let ls: Vec<_> = (0..3).map(|_| line(1, 2, IVA21)).collect();
        assert_eq!(compute_totals(&ls, RoundingMode::PerLine).vat, Money::ZERO);
        assert_eq!(
            compute_totals(&ls, RoundingMode::PerRate).vat,
            Money::from_cents(1)
        );
    }

    #[test]
    fn inversion_sujeto_pasivo_sin_cuota() {
        let t = compute_totals(
            &[line(1, 10_000, VatTreatment::ReverseCharge)],
            RoundingMode::PerRate,
        );
        assert_eq!(
            (t.vat, t.invoice_total),
            (Money::ZERO, Money::from_euros(100, 0))
        );
    }

    #[test]
    fn cantidades_fraccionarias() {
        let mut l = line(0, 1234, IVA21);
        l.quantity_milli = 2_500; // 2,5 × 12,34 = 30,85
        assert_eq!(line_base(&l), Money::from_cents(3085));
        l.quantity_milli = 333; // 0,333 × 12,34 = 4,10922 → 4,11
        assert_eq!(line_base(&l), Money::from_cents(411));
    }

    proptest! {
        /// Cuadre: total = Σbases + ΣIVA + ΣRE y a cobrar = total − retención, en ambos modos.
        #[test]
        fn totales_cuadran(
            items in prop::collection::vec((1i64..50_000, -100_000i64..1_000_000, 0usize..4, any::<bool>()), 1..20),
            per_line in any::<bool>(),
        ) {
            let rates = [0, 400, 1000, 2100];
            let lines: Vec<_> = items.iter().map(|&(q, p, r, w)| InvoiceLine {
                description: "x".into(),
                quantity_milli: q,
                unit_price: Money::from_cents(p),
                discount: Rate::ZERO,
                vat: VatTreatment::Taxed { rate: Rate::from_bp(rates[r]) },
                surcharge: None,
                withholding: w.then_some(Rate::percent(15)),
                advance: false,
            }).collect();
            let mode = if per_line { RoundingMode::PerLine } else { RoundingMode::PerRate };
            let t = compute_totals(&lines, mode);
            let sum_base: Money = lines.iter().map(line_base).sum();
            prop_assert_eq!(t.base, sum_base);
            prop_assert_eq!(t.invoice_total, t.base + t.vat + t.surcharge);
            prop_assert_eq!(t.payable, t.invoice_total - t.withholding);
            // Diferencia entre modos acotada a medio céntimo por línea.
            let other = compute_totals(&lines, if per_line { RoundingMode::PerRate } else { RoundingMode::PerLine });
            prop_assert!((t.vat - other.vat).abs().cents() <= lines.len() as i64);
        }
    }
}
