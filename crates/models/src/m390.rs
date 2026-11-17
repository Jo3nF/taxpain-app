//! Modelo 390: declaración-resumen anual del IVA (régimen general).
//!
//! Base legal: art. 164.Uno.6.º LIVA; art. 71.4 RIVA; Orden EHA/3111/2009 y modificaciones (la
//! última, Orden HAC/27/2026, para el modelo del ejercicio 2026).
//!
//! Diseños de registro (`fixtures/aeat/dr/SOURCES.md`): el fichero de 2026 y siguientes sigue
//! `dr390e2026.xlsx` (v1.00, «Ejercicio 2026 y siguientes») y el de 2025, `dr390e2025.xlsx`
//! (v1.02). Los ejercicios anteriores a 2025 también salen con el de 2025, como siempre, y el
//! borrador avisa de que el suyo es otro. El de 2026 añade la marca de pago a cuenta de
//! carburantes en la página 1 (campo 22: los siguientes se corren uno), el pago a cuenta [112]
//! en la página 6 (dos veces) y el CNAE de 4 cifras en la 7. `TODO(VERIFY)`: las instrucciones
//! del 390 de 2026 aún no están publicadas (CNAE de 4 cifras); la AEAT revisó en diciembre los
//! diseños de 2024 y 2025: comprobar con `tools/aeat/dr_actualizar.py --comprobar`.
//!
//! Se calcula con las mismas operaciones que los 303 del año (devengado por tipo, recargo de
//! equivalencia, deducible corriente e inversión por tipo) y con los resultados de los cuatro
//! 303 para el apartado de resultados ([85], [95], [97], [98]).
//!
//! Con prorrata (general o especial) las deducciones van con el porcentaje provisional, la
//! regularización con el definitivo en [522], la de bienes de inversión en [63] y el apartado 12
//! con el porcentaje del año. Con criterio de caja, el devengado del régimen va en [643]–[648] y
//! la información por la regla general en [653]–[657].
//!
//! Volumen de operaciones [108] (art. 121 LIVA): las ventas de inmovilizado ya suman en su
//! casilla de operaciones ([99], [105]…) y se restan en [106] (inmuebles) y [107] (bienes de
//! inversión, art. 108 LIVA); qué ventas son lo decide quien llama ([`Annual390`]).
//!
//! Fuera de alcance: regímenes especiales distintos del criterio de caja, sectores
//! diferenciados (apartado 13), [232] (IVA soportado no deducible) y el pago a cuenta de
//! carburantes del modelo 319 ([112] y su marca van a cero y «No»).

use serde::{Deserialize, Serialize};
use taxpain_core::invoice::{Exemption, NotSubject, VatTreatment};
use taxpain_core::{Money, Rate};

use crate::dr::{DrError, PageWriter, Spec, envelope};
use crate::m303::{
    Boxes, DeclarationType, IssuedVat, Prorata, ProrataUse, ReceivedKind, ReceivedVat, Result303,
    SelfAssessedKind, SelfAssessedPart, Volumen390,
};
use crate::{DEVELOPER_NIF, Declarant, SOFTWARE_VERSION};

/// Diseño vigente: el de 2026 y siguientes.
pub const DR_JSON: &str = include_str!("../../../fixtures/aeat/dr/390.json");
/// Diseño de 2025, con el que se generan 2025 y los ejercicios anteriores.
pub const DR_JSON_2025: &str = include_str!("../../../fixtures/aeat/dr/anteriores/dr390e2025.json");
/// Primer ejercicio que se genera con el diseño vigente.
const FIRST_YEAR: i32 = 2026;

/// El diseño con que se genera el 390 de `year`.
pub fn dr_json(year: i32) -> &'static str {
    if year >= FIRST_YEAR {
        DR_JSON
    } else {
        DR_JSON_2025
    }
}

/// El diseño vigente, ya interpretado.
pub fn spec() -> &'static Spec {
    static SPEC: std::sync::OnceLock<Spec> = std::sync::OnceLock::new();
    SPEC.get_or_init(|| Spec::parse(DR_JSON))
}

/// El diseño con que se genera el 390 de `year`, ya interpretado.
pub fn spec_for(year: i32) -> &'static Spec {
    static SPEC_2025: std::sync::OnceLock<Spec> = std::sync::OnceLock::new();
    if year >= FIRST_YEAR {
        spec()
    } else {
        SPEC_2025.get_or_init(|| Spec::parse(DR_JSON_2025))
    }
}

/// Datos de identificación y estadísticos.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Options390 {
    /// Clave de actividad (3, p. ej. «A03») y epígrafe del IAE (4) de la actividad principal.
    pub activity_code: String,
    pub iae: String,
    /// Ha presentado (o presentará) el 347 del ejercicio.
    pub files_347: bool,
    /// Descripción de la actividad (apartado 12, prorratas).
    #[serde(default)]
    pub activity: String,
}

/// Datos del año que no salen de las facturas: prorrata, criterio de caja y ventas de
/// inmovilizado.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Annual390 {
    /// Prorrata con el definitivo del año, la regularización [522] y la de inversiones [63].
    pub prorata: Option<Prorata>,
    /// Acogido al criterio de caja.
    pub cash_basis: bool,
    /// Destinatario de operaciones con criterio de caja.
    pub cash_recipient: bool,
    /// Emitidas y recibidas del año con criterio de caja, por la regla general de devengo.
    pub cash_issued_base: Money,
    pub cash_issued_vat: Money,
    pub cash_received_base: Money,
    pub cash_received_vat: Money,
    /// Ventas del año de inmuebles del inmovilizado [106] («entregas de bienes inmuebles… no
    /// habituales») y de otros bienes de inversión [107] (art. 108 LIVA). Ya están en su casilla
    /// de operaciones y se restan del volumen [108] (art. 121.Tres LIVA). Pueden ser negativas:
    /// la rectificativa del año de una venta de un año anterior también está en su casilla.
    #[serde(default)]
    pub real_estate: Money,
    #[serde(default)]
    pub capital_goods: Money,
    /// Regularizaciones por entregas de bienes de inversión del año (art. 110 LIVA), en [63]
    /// aunque no haya prorrata (art. 110.Dos).
    #[serde(default)]
    pub capital_goods_sale_regularization: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Result390 {
    pub year: i32,
    pub boxes: Boxes,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub prorata: Option<Prorata>,
    #[serde(default)]
    pub cash_basis: bool,
    #[serde(default)]
    pub cash_recipient: bool,
}

impl Volumen390 {
    /// Volumen de operaciones de un 390 ya calculado, para la página 04 del 303 del último
    /// periodo de un exonerado: `total()` coincide con [108].
    pub fn of(r: &Result390) -> Volumen390 {
        Volumen390 {
            general: r.get(99),
            cash_basis: r.get(653),
            intra: r.get(103),
            exports: r.get(104),
            exempt: r.get(105),
            not_subject: r.get(110),
            oss: r.get(126),
            reverse_charge: r.get(125),
            real_estate: r.get(106),
            capital_goods: r.get(107),
        }
    }
}

impl Result390 {
    pub fn get(&self, b: u16) -> Money {
        self.boxes.get(&b).copied().unwrap_or(Money::ZERO)
    }
}

fn add(b: &mut Boxes, k: u16, v: Money) {
    if !v.is_zero() {
        *b.entry(k).or_insert(Money::ZERO) += v;
    }
}

fn devengado(rate: Rate) -> Option<(u16, u16)> {
    match rate.bp() {
        400 => Some((1, 2)),
        1000 => Some((3, 4)),
        2100 => Some((5, 6)),
        _ => None,
    }
}

/// Devengado con el régimen especial del criterio de caja, por tipo.
fn devengado_caja(rate: Rate) -> Option<(u16, u16)> {
    match rate.bp() {
        0 => Some((708, 709)),
        200 => Some((675, 676)),
        400 => Some((643, 644)),
        500 => Some((710, 711)),
        750 => Some((677, 678)),
        1000 => Some((645, 646)),
        2100 => Some((647, 648)),
        _ => None,
    }
}

fn recargo(rate: Rate) -> Option<(u16, u16)> {
    match rate.bp() {
        50 => Some((35, 36)),
        140 => Some((599, 600)),
        175 => Some((41, 42)),
        520 => Some((601, 602)),
        _ => None,
    }
}

/// Tipo más cercano a cuota/base (las recibidas guardan sólo importes).
fn infer_rate(base: Money, vat: Money) -> Rate {
    [400, 1000, 2100]
        .into_iter()
        .map(Rate::from_bp)
        .min_by_key(|r| (base.apply_rate(*r) - vat).abs().cents())
        .unwrap_or(Rate::from_bp(2100))
}

fn deducible(kind: ReceivedKind, rate: Rate) -> Option<(u16, u16)> {
    match (kind, rate.bp()) {
        (ReceivedKind::Corriente, 400) => Some((190, 191)),
        (ReceivedKind::Corriente, 1000) => Some((603, 604)),
        (ReceivedKind::Corriente, 2100) => Some((605, 606)),
        (ReceivedKind::BienInversion, 400) => Some((196, 197)),
        (ReceivedKind::BienInversion, 1000) => Some((611, 612)),
        (ReceivedKind::BienInversion, 2100) => Some((613, 614)),
        _ => None,
    }
}

/// `quarters`: los 303 del año (1T a 4T) ya calculados.
pub fn compute(
    year: i32,
    issued: &[IssuedVat],
    received: &[ReceivedVat],
    quarters: &[Result303],
    carried_compensation: Money,
) -> Result390 {
    compute_with(
        year,
        issued,
        received,
        quarters,
        carried_compensation,
        &Annual390::default(),
    )
}

/// Como [`compute`], con prorrata y criterio de caja.
pub fn compute_with(
    year: i32,
    issued: &[IssuedVat],
    received: &[ReceivedVat],
    quarters: &[Result303],
    carried_compensation: Money,
    extra: &Annual390,
) -> Result390 {
    let mut b = Boxes::new();
    let mut warnings = Vec::new();
    let sum = |b: &Boxes, ks: &[u16]| ks.iter().filter_map(|k| b.get(k)).copied().sum::<Money>();

    for inv in issued {
        for t in &inv.breakdown {
            match t.vat {
                VatTreatment::Taxed { rate } => {
                    let boxes = if inv.rectification {
                        Some((29, 30))
                    } else if inv.cash_basis {
                        devengado_caja(rate)
                    } else {
                        devengado(rate)
                    };
                    match boxes {
                        Some((kb, kc)) => {
                            add(&mut b, kb, t.base);
                            add(&mut b, kc, t.vat_amount);
                            // Las del criterio de caja cuentan en el volumen por [653].
                            if !inv.cash_basis {
                                add(&mut b, 99, t.base);
                            }
                        }
                        None => warnings.push(format!(
                            "{}: tipo del {rate} sin casilla en este cálculo",
                            inv.number
                        )),
                    }
                    if let Some(sr) = t.surcharge_rate {
                        let boxes = if inv.rectification {
                            Some((43, 44))
                        } else {
                            recargo(sr)
                        };
                        if let Some((kb, kc)) = boxes {
                            add(&mut b, kb, t.base);
                            add(&mut b, kc, t.surcharge_amount);
                        }
                    }
                }
                VatTreatment::ReverseCharge => add(&mut b, 125, t.base),
                VatTreatment::Exempt(Exemption::E5) => add(&mut b, 103, t.base),
                VatTreatment::Exempt(Exemption::E2 | Exemption::E3 | Exemption::E4) => {
                    add(&mut b, 104, t.base);
                }
                VatTreatment::Exempt(Exemption::E1 | Exemption::E6) => add(&mut b, 105, t.base),
                // Servicios a empresarios de otro Estado miembro (art. 69.Uno.1º LIVA): son
                // «prestaciones de servicios intracomunitarias» y van en [103], con las [59] de
                // los 303 y la clave S del 349; [110] excluye expresamente las incluidas en [103]
                // (instrucciones del 390, casillas 103 y 110).
                VatTreatment::NotSubject(NotSubject::N2) if inv.eu_business => {
                    add(&mut b, 103, t.base);
                }
                // Ventanilla única: «OSS. Operaciones no sujetas por reglas de localización
                // acogidas a la OSS» [126] (dr390e2026, página 6); la [110] las excluye.
                VatTreatment::NotSubject(NotSubject::N2) if inv.oss => add(&mut b, 126, t.base),
                VatTreatment::NotSubject(NotSubject::N2) => add(&mut b, 110, t.base),
                VatTreatment::NotSubject(NotSubject::N1) => {}
            }
        }
    }
    // Inversión del sujeto pasivo: devengo por tipo (adquisiciones intracomunitarias de
    // bienes [21]–[26], de servicios [545]–[552]; otras ISP [27]/[28]) y deducción más abajo.
    for r in received {
        let Some(sa) = r.self_assessed else { continue };
        if sa.part == SelfAssessedPart::Deduccion {
            continue; // Devengada en un ejercicio anterior.
        }
        let rate = infer_rate(r.base, sa.quota);
        let boxes = match (sa.kind, sa.goods, rate.bp()) {
            // Abonos: modificación de bases y cuotas.
            _ if r.rectification => Some((29, 30)),
            (SelfAssessedKind::Otra, _, _) => Some((27, 28)),
            (SelfAssessedKind::Intracomunitaria, true, 400) => Some((21, 22)),
            (SelfAssessedKind::Intracomunitaria, true, 1000) => Some((23, 24)),
            (SelfAssessedKind::Intracomunitaria, true, 2100) => Some((25, 26)),
            (SelfAssessedKind::Intracomunitaria, false, 400) => Some((545, 546)),
            (SelfAssessedKind::Intracomunitaria, false, 1000) => Some((547, 548)),
            (SelfAssessedKind::Intracomunitaria, false, 2100) => Some((551, 552)),
            _ => None,
        };
        match boxes {
            Some((kb, kc)) => {
                add(&mut b, kb, r.base);
                add(&mut b, kc, sa.quota);
            }
            None => warnings.push(format!(
                "{}: tipo de la autorrepercusión sin casilla en este cálculo",
                r.number
            )),
        }
    }
    let base_iva = sum(
        &b,
        &[
            1, 3, 5, 708, 675, 643, 710, 677, 645, 647, 21, 23, 25, 545, 547, 551, 27, 29,
        ],
    );
    let cuota_iva = sum(
        &b,
        &[
            2, 4, 6, 709, 676, 644, 711, 678, 646, 648, 22, 24, 26, 546, 548, 552, 28, 30,
        ],
    );
    b.insert(33, base_iva);
    b.insert(34, cuota_iva);
    let total_devengado = cuota_iva + sum(&b, &[36, 600, 602, 42, 44]);
    b.insert(47, total_devengado);

    // Deducciones sujetas a la prorrata (como en el 303): aparte hasta aplicar el porcentaje.
    let mut common = Boxes::new();
    for r in received {
        let pct = Rate::percent(i32::from(r.deductible_pct.min(100)));
        let (prorated, excluded) = match &extra.prorata {
            None => (false, false),
            Some(_) if r.outside_prorata => (false, false),
            Some(p) if !p.special => (true, false),
            Some(_) => (
                r.prorata_use == ProrataUse::Comun,
                r.prorata_use == ProrataUse::Ninguna,
            ),
        };
        if excluded {
            continue;
        }
        // La base va siempre sin prorratear; la cuota, con la prorrata si le toca.
        let quota_box = |b: &mut Boxes, common: &mut Boxes, k: u16, v: Money| {
            add(if prorated { common } else { b }, k, v);
        };
        if let Some(sa) = r.self_assessed {
            if sa.part == SelfAssessedPart::Devengo {
                continue; // Se deduce en el ejercicio en que se registró.
            }
            let (base, quota) = (r.base.apply_rate(pct), sa.quota.apply_rate(pct));
            let rate = infer_rate(r.base, sa.quota);
            let boxes = match (sa.kind, sa.goods, r.kind, rate.bp()) {
                // Abonos: rectificación de deducciones.
                _ if r.rectification => Some((639, 62)),
                // Otras ISP: se deducen como operaciones interiores.
                (SelfAssessedKind::Otra, _, k, _) => deducible(k, rate),
                (SelfAssessedKind::Intracomunitaria, true, ReceivedKind::Corriente, 400) => {
                    Some((214, 215))
                }
                (SelfAssessedKind::Intracomunitaria, true, ReceivedKind::Corriente, 1000) => {
                    Some((627, 628))
                }
                (SelfAssessedKind::Intracomunitaria, true, ReceivedKind::Corriente, 2100) => {
                    Some((629, 630))
                }
                (SelfAssessedKind::Intracomunitaria, _, ReceivedKind::BienInversion, 400) => {
                    Some((220, 221))
                }
                (SelfAssessedKind::Intracomunitaria, _, ReceivedKind::BienInversion, 1000) => {
                    Some((631, 632))
                }
                (SelfAssessedKind::Intracomunitaria, _, ReceivedKind::BienInversion, 2100) => {
                    Some((633, 634))
                }
                (SelfAssessedKind::Intracomunitaria, false, ReceivedKind::Corriente, 400) => {
                    Some((587, 588))
                }
                (SelfAssessedKind::Intracomunitaria, false, ReceivedKind::Corriente, 1000) => {
                    Some((635, 636))
                }
                (SelfAssessedKind::Intracomunitaria, false, ReceivedKind::Corriente, 2100) => {
                    Some((637, 638))
                }
                _ => None,
            };
            if let Some((kb, kc)) = boxes {
                add(&mut b, kb, base);
                quota_box(&mut b, &mut common, kc, quota);
            }
            continue;
        }
        let vat = r.vat.apply_rate(pct);
        let base = r.base.apply_rate(pct);
        if vat.is_zero() {
            continue;
        }
        if r.rectification {
            add(&mut b, 639, base);
            quota_box(&mut b, &mut common, 62, vat);
            continue;
        }
        let rate = infer_rate(r.base, r.vat);
        let boxes = if r.import {
            match (r.kind, rate.bp()) {
                (ReceivedKind::Corriente, 400) => Some((202, 203)),
                (ReceivedKind::Corriente, 1000) => Some((619, 620)),
                (ReceivedKind::Corriente, 2100) => Some((621, 622)),
                (ReceivedKind::BienInversion, 400) => Some((208, 209)),
                (ReceivedKind::BienInversion, 1000) => Some((623, 624)),
                (ReceivedKind::BienInversion, 2100) => Some((625, 626)),
                _ => None,
            }
        } else {
            deducible(r.kind, rate)
        };
        if let Some((kb, kc)) = boxes {
            add(&mut b, kb, base);
            quota_box(&mut b, &mut common, kc, vat);
        }
    }
    if let Some(p) = &extra.prorata {
        let pct = Rate::percent(i32::from(p.provisional.min(100)));
        for (k, v) in &common {
            add(&mut b, *k, v.apply_rate(pct));
        }
        // Regularizaciones del último periodo: inversiones [63] y prorrata definitiva [522].
        add(&mut b, 63, p.investment_regularization);
        add(&mut b, 522, p.regularization);
    }
    add(&mut b, 63, extra.capital_goods_sale_regularization);
    let corr = (sum(&b, &[190, 603, 605]), sum(&b, &[191, 604, 606]));
    let inv = (sum(&b, &[196, 611, 613]), sum(&b, &[197, 612, 614]));
    add(&mut b, 48, corr.0);
    add(&mut b, 49, corr.1);
    add(&mut b, 50, inv.0);
    add(&mut b, 51, inv.1);
    let imp_corr = (sum(&b, &[202, 619, 621]), sum(&b, &[203, 620, 622]));
    let imp_inv = (sum(&b, &[208, 623, 625]), sum(&b, &[209, 624, 626]));
    add(&mut b, 52, imp_corr.0);
    add(&mut b, 53, imp_corr.1);
    add(&mut b, 54, imp_inv.0);
    add(&mut b, 55, imp_inv.1);
    let intra_corr = (sum(&b, &[214, 627, 629]), sum(&b, &[215, 628, 630]));
    let intra_inv = (sum(&b, &[220, 631, 633]), sum(&b, &[221, 632, 634]));
    let intra_serv = (sum(&b, &[587, 635, 637]), sum(&b, &[588, 636, 638]));
    add(&mut b, 56, intra_corr.0);
    add(&mut b, 57, intra_corr.1);
    add(&mut b, 58, intra_inv.0);
    add(&mut b, 59, intra_inv.1);
    add(&mut b, 597, intra_serv.0);
    add(&mut b, 598, intra_serv.1);
    let deducciones = corr.1
        + inv.1
        + imp_corr.1
        + imp_inv.1
        + intra_corr.1
        + intra_inv.1
        + intra_serv.1
        + sum(&b, &[62, 63, 522]);
    b.insert(64, deducciones);
    let r65 = total_devengado - deducciones;
    b.insert(65, r65);
    b.insert(84, r65);

    // Compensación de cuotas del ejercicio anterior aplicada en el año (hasta lo arrastrado).
    let applied: Money = quarters.iter().map(|q| q.get(78)).sum();
    let r85 = applied.min(carried_compensation.max(Money::ZERO));
    add(&mut b, 85, r85);
    b.insert(86, r65 - r85);

    let ingresos: Money = quarters
        .iter()
        .filter(|q| {
            matches!(
                q.declaration,
                DeclarationType::Ingreso | DeclarationType::Domiciliacion
            )
        })
        .map(Result303::result)
        .sum();
    add(&mut b, 95, ingresos);
    if let Some(last) = quarters.iter().find(|q| q.quarter == 4) {
        match last.declaration {
            DeclarationType::Compensacion => add(&mut b, 97, -last.result()),
            DeclarationType::Devolucion => add(&mut b, 98, -last.result()),
            _ => {}
        }
        // [662]: cuotas a compensar generadas en periodos del ejercicio distintos del último y
        // no aplicadas en el resto del año (instrucciones del 390, casilla 662). En la cadena de
        // los 303, [87] del 4T es todo lo pendiente al cerrar el año; se le quita la parte del
        // arrastre del año anterior que tampoco se ha aplicado (arrastre − [85], que se aplica
        // primero). Así [86] = [95] − [97] − [98] − [662].
        let unapplied_carry = carried_compensation.max(Money::ZERO) - r85;
        add(
            &mut b,
            662,
            (last.get(87) - unapplied_carry).max(Money::ZERO),
        );
    } else {
        warnings.push("Falta el 303 del 4T para el apartado de resultados".into());
    }

    // Criterio de caja: operaciones del año por la regla general de devengo.
    if extra.cash_basis {
        add(&mut b, 653, extra.cash_issued_base);
        add(&mut b, 654, extra.cash_issued_base);
        add(&mut b, 655, extra.cash_issued_vat);
    }
    if extra.cash_basis || extra.cash_recipient {
        add(&mut b, 656, extra.cash_received_base);
        add(&mut b, 657, extra.cash_received_vat);
    }

    // Ventas de inmovilizado: ya suman en su casilla de operaciones y se restan del volumen.
    add(&mut b, 106, extra.real_estate);
    add(&mut b, 107, extra.capital_goods);
    let volumen = sum(&b, &[99, 653, 103, 104, 105, 110, 126, 125]) - sum(&b, &[106, 107]);
    b.insert(108, volumen);

    // Cuadre con los 303: la suma de los resultados trimestrales [46] debe ser [65].
    let quarterly: Money = quarters.iter().map(|q| q.get(46)).sum();
    // Con prorrata el porcentaje se aplica a otras agrupaciones (por tipo en el 390, por
    // casilla en cada 303): se admite la diferencia de redondeo.
    let tolerance = if extra.prorata.is_some() { 10 } else { 0 };
    if quarters.len() == 4 && (quarterly - r65).abs().cents() > tolerance {
        warnings.push(format!(
            "La suma de los 303 ({quarterly}) no coincide con el resultado anual ({r65}): revisa los periodos"
        ));
    }

    b.retain(|k, v| !v.is_zero() || matches!(k, 33 | 34 | 47 | 64 | 65 | 84 | 86 | 108));
    Result390 {
        year,
        boxes: b,
        warnings,
        prorata: extra.prorata.clone(),
        cash_basis: extra.cash_basis,
        cash_recipient: extra.cash_recipient,
    }
}

/// Campos de la página 1 que cambian de número según el diseño (los del 1 al 21 son iguales).
#[derive(Debug)]
struct Pagina1 {
    /// Sujeto pasivo con derecho a deducir el pago a cuenta de carburantes (desde 2026).
    carburantes: Option<u16>,
    /// Sustitutiva y sustitutiva por rectificación de cuotas.
    sustitutivas: [u16; 2],
    /// Datos estadísticos de la actividad principal: B (clave) y C (epígrafe del IAE).
    actividad: u16,
    epigrafe: u16,
    /// Dato estadístico D: declaración anual de operaciones con terceras personas (347).
    m347: u16,
}

impl Pagina1 {
    /// Los de `dr390e2026`: la marca de carburantes entra en el 22 y corre uno los siguientes.
    const DE_2026: Pagina1 = Pagina1 {
        carburantes: Some(22),
        sustitutivas: [23, 24],
        actividad: 27,
        epigrafe: 28,
        m347: 44,
    };
    /// Los de `dr390e2025`.
    const DE_2025: Pagina1 = Pagina1 {
        carburantes: None,
        sustitutivas: [22, 23],
        actividad: 26,
        epigrafe: 27,
        m347: 43,
    };

    fn of(year: i32) -> &'static Pagina1 {
        if year >= FIRST_YEAR {
            &Self::DE_2026
        } else {
            &Self::DE_2025
        }
    }
}

/// Fichero del 390 con el diseño con que se genera su ejercicio ([`spec_for`]): páginas 1, 2,
/// 2 bis, 3, 4 y 6, y la 7 con prorrata o criterio de caja.
pub fn file(r: &Result390, who: &Declarant, o: &Options390) -> Result<String, DrError> {
    let s = spec_for(r.year);
    let c = Pagina1::of(r.year);
    let mut pages = Vec::new();

    let mut w = PageWriter::new(s.page("Pág. 1")?);
    w.text(7, &who.nif)?
        .text(8, &who.name)?
        .text(9, who.given_name.as_deref().unwrap_or(""))?
        .text(10, &r.year.to_string())?;
    for n in [12, 13, 15, 16, c.sustitutivas[0], c.sustitutivas[1]] {
        w.text(n, "0")?;
    }
    for n in [17, 19] {
        w.text(n, "2")?;
    }
    // Criterio de caja: acogido [20] y destinatario de operaciones del régimen [21].
    let yes_no = |b: bool| if b { "1" } else { "2" };
    w.text(20, yes_no(r.cash_basis))?
        .text(21, yes_no(r.cash_recipient))?;
    // Pago a cuenta de carburantes (modelo 319): sólo para el último depositante de un depósito
    // fiscal; el diseño admite «1» o «2», sin blanco.
    if let Some(n) = c.carburantes {
        w.text(n, yes_no(false))?;
    }
    w.text(c.actividad, &o.activity_code)?
        .text(c.epigrafe, &o.iae)?
        .text(c.m347, if o.files_347 { "1" } else { "0" })?;
    pages.push(w.finish()?);

    for sheet in ["Pág. 2", "Pág. 2 bis", "Pág. 3", "Pág. 4", "Pág. 6"] {
        let p = s
            .pages
            .iter()
            .find(|p| p.sheet == sheet)
            .ok_or_else(|| DrError::NoPage("390".into(), sheet.into()))?;
        let mut w = PageWriter::new(p);
        for (box_, v) in &r.boxes {
            // En la página 6, [658], [84] y [659] (y desde 2026 [112]) aparecen dos veces
            // (resultado y territorio): sólo la primera (field_for_box) se rellena.
            if p.field_for_box(*box_).is_ok() {
                w.amount_box(*box_, *v)?;
            }
        }
        pages.push(w.finish()?);
    }
    let prorata = r
        .prorata
        .as_ref()
        .and_then(|p| p.definitive.map(|d| (p, d)));
    if prorata.is_some() || [654, 655, 656, 657].iter().any(|k| r.boxes.contains_key(k)) {
        let p7 = s.page("Pág. 7")?;
        let mut w = PageWriter::new(p7);
        for k in [654u16, 655, 656, 657] {
            if let Some(v) = r.boxes.get(&k) {
                w.amount_box(k, *v)?;
            }
        }
        // Apartado 12, primera actividad: CNAE-2025 con las cifras del diseño (las tres primeras
        // hasta 2025, como dicen las instrucciones; cuatro desde 2026, como la página 05 del
        // 303).
        if let Some((p, def)) = prorata {
            let cnae: String = p
                .cnae
                .chars()
                .filter(char::is_ascii_digit)
                .take(p7.field(18)?.len)
                .collect();
            w.text(17, &o.activity.chars().take(40).collect::<String>())?
                .text(18, &cnae)?
                .amount(19, p.operations.max(Money::ZERO))?
                .amount(20, p.deductible_operations.max(Money::ZERO))?
                .text(21, if p.special { "E" } else { "G" })?
                .amount(22, Money::from_cents(i64::from(def.min(100)) * 100))?;
        }
        pages.push(w.finish()?);
    }
    Ok(envelope(
        "390",
        r.year,
        "0A",
        SOFTWARE_VERSION,
        DEVELOPER_NIF,
        &pages,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Los campos de la página 1 que escribe [`file`] son los que dice cada diseño con que se
    /// genera.
    #[test]
    fn campos_de_la_pagina_1_de_cada_diseno() {
        for (year, source) in [
            (2017, "dr390e2025.xlsx"),
            (2025, "dr390e2025.xlsx"),
            (2026, "dr390e2026.xlsx"),
            (2030, "dr390e2026.xlsx"),
        ] {
            let s = spec_for(year);
            assert_eq!(s.source, source, "{year}");
            let p1 = s.page("Pág. 1").unwrap_or_else(|e| panic!("{e}"));
            let desc = |n: u16| p1.field(n).map(|f| f.desc.clone()).unwrap_or_default();
            let c = Pagina1::of(year);
            assert_eq!(c.carburantes.is_some(), year >= 2026, "{year}");
            if let Some(n) = c.carburantes {
                assert!(desc(n).contains("pago a cuenta de entregas de gasolinas"));
            }
            assert_eq!(desc(c.sustitutivas[0]), "2. Devengo - Sustitutiva?");
            assert_eq!(
                desc(c.sustitutivas[1]),
                "2. Devengo - Sustitutiva por rectificación de cuotas?"
            );
            assert!(desc(c.actividad).contains("B - Código de actividad - Principal"));
            assert!(desc(c.epigrafe).contains("C -Epígrafe I.A.E. - Principal"));
            assert!(desc(c.m347).contains("D - Declaración anual operac. con terceras personas"));
            // Los fijos (1 a 21) no se mueven.
            assert_eq!(desc(10), "2. Devengo - Ejercicio.");
            assert!(desc(21).contains("destinatario del régimen especial del criterio de caja"));
        }
        assert_eq!(dr_json(2025), DR_JSON_2025);
        assert_eq!(dr_json(2026), DR_JSON);
    }
}
