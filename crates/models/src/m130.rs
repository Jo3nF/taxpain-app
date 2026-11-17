//! Modelo 130: pago fraccionado del IRPF de actividades económicas en estimación directa.
//!
//! Base legal: art. 99.7 Ley 35/2006 (LIRPF); arts. 109 a 112 RD 439/2007 (RIRPF); Orden
//! HAP/258/2015 (modelo). Instrucciones oficiales de cumplimentación (Sede AEAT, «Modelo 130.
//! Instrucciones»). Diseño de registro: `DR130e15v12.xls`.
//!
//! Importes del apartado I acumulados del 1 de enero al último día del trimestre.
//! - [02] en estimación directa simplificada incluye las provisiones y los gastos de difícil
//!   justificación: 5 % del rendimiento neto previo, máx. 2.000 € al año (art. 30.2.4.ª LIRPF).
//! - [04] 20 % de [03] si es positivo (art. 110.1.a RIRPF); 0 si no.
//! - [05] suma de las [07] positivas de los trimestres anteriores menos sus [16].
//! - [07] = [04] − [05] − [06] (con signo).
//! - [12] = [07] + [11]; si es negativa, 0.
//! - [13] minoración del art. 110.3.c RIRPF según el rendimiento neto del año anterior.
//! - [15] sólo si [14] > 0: [19] negativas anteriores aún no deducidas, hasta [14].
//! - [17] = [14] − [15] − [16]; [19] = [17] − [18].
//!
//! Fuera de alcance (avisos): actividades agrícolas y ganaderas (apartado II), deducción por
//! vivienda habitual ([16], DT 18.ª LIRPF), complementarias, tipo del 8 % (Ceuta y Melilla).

use serde::{Deserialize, Serialize};
use taxpain_core::{Money, Rate};

use crate::dr::{DrError, PageWriter, Spec, envelope};
use crate::{DEVELOPER_NIF, Declarant, SOFTWARE_VERSION};

pub const DR_JSON: &str = include_str!("../../../fixtures/aeat/dr/130.json");

/// Casillas de un 130 ya presentado del mismo ejercicio.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Previous130 {
    pub box07: Money,
    pub box15: Money,
    pub box16: Money,
    pub box19: Money,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Input130 {
    /// [01] ingresos computables acumulados en el año.
    pub income: Money,
    /// [02] gastos deducibles acumulados (incluidas amortizaciones).
    pub expenses: Money,
    /// [06] retenciones e ingresos a cuenta soportados, acumulados.
    pub withholdings: Money,
    /// Trimestres anteriores del mismo ejercicio, en orden.
    pub previous: Vec<Previous130>,
    /// Rendimiento neto de actividades del año anterior (para [13]); `None` si no aplica.
    pub previous_year_net: Option<Money>,
    pub iban: Option<String>,
    /// Casillas fijadas a mano (de [`ADJUSTABLE`]); el resto se recalcula con ellas.
    #[serde(default)]
    pub adjustments: crate::m303::Boxes,
    /// Estimación directa simplificada: [02] incluye las provisiones y los gastos de difícil
    /// justificación (5 % del rendimiento neto previo, máx. 2.000 € al año; art. 30.2.4.ª LIRPF).
    #[serde(default)]
    pub simplified: bool,
}

/// Casillas del 130 que se pueden fijar a mano: datos de partida, no totales.
pub const ADJUSTABLE: [u16; 6] = [1, 2, 5, 6, 13, 15];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeclarationType130 {
    /// I: ingreso.
    Ingreso,
    /// U: domiciliación.
    Domiciliacion,
    /// N: negativa (resultado cero).
    Negativa,
    /// B: resultado a deducir en los siguientes pagos fraccionados.
    ADeducir,
}

impl DeclarationType130 {
    pub fn code(self) -> &'static str {
        match self {
            DeclarationType130::Ingreso => "I",
            DeclarationType130::Domiciliacion => "U",
            DeclarationType130::Negativa => "N",
            DeclarationType130::ADeducir => "B",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Result130 {
    pub year: i32,
    pub quarter: u8,
    pub boxes: crate::m303::Boxes,
    pub declaration: DeclarationType130,
    pub warnings: Vec<String>,
}

impl Result130 {
    pub fn get(&self, b: u16) -> Money {
        self.boxes.get(&b).copied().unwrap_or(Money::ZERO)
    }
}

/// Minoración del art. 110.3.c RIRPF por rendimiento neto del año anterior.
pub fn reduction(previous_year_net: Money) -> Money {
    let euros = |e| Money::from_euros(e, 0);
    match previous_year_net {
        n if n <= euros(9_000) => euros(100),
        n if n <= euros(10_000) => euros(75),
        n if n <= euros(11_000) => euros(50),
        n if n <= euros(12_000) => euros(25),
        _ => Money::ZERO,
    }
}

pub fn compute(year: i32, quarter: u8, i: &Input130) -> Result130 {
    let mut b = crate::m303::Boxes::new();
    let mut warnings = Vec::new();
    let mut fixed = |k: u16, v: Money| match i.adjustments.get(&k) {
        Some(m) if ADJUSTABLE.contains(&k) => {
            warnings.push(format!("Casilla [{k:02}] fijada a mano: {m}"));
            *m
        }
        _ => v,
    };
    let income = fixed(1, i.income);
    // Simplificada: el 5 % (máx. 2.000 €) sobre el rendimiento neto acumulado previo, dentro de
    // [02] (instrucciones del 130, casilla 02).
    let hard = if i.simplified {
        crate::m100::hard_to_justify(year, income - i.expenses)
    } else {
        Money::ZERO
    };
    let manual_02 = i.adjustments.contains_key(&2);
    let expenses = fixed(2, i.expenses + hard);
    let withholdings = fixed(6, i.withholdings);
    let r03 = income - expenses;
    let r04 = if r03.cents() > 0 {
        r03.apply_rate(Rate::percent(20))
    } else {
        Money::ZERO
    };
    let r05 = i
        .previous
        .iter()
        .map(|p| p.box07.max(Money::ZERO) - p.box16)
        .sum::<Money>();
    let r05 = fixed(5, r05);
    let r07 = r04 - r05 - withholdings;
    let r12 = r07.max(Money::ZERO);
    let r13 = fixed(13, i.previous_year_net.map_or(Money::ZERO, reduction));
    let r14 = r12 - r13;
    let pending_negative = i
        .previous
        .iter()
        .map(|p| (-p.box19).max(Money::ZERO))
        .sum::<Money>()
        - i.previous.iter().map(|p| p.box15).sum::<Money>();
    let r15 = fixed(
        15,
        if r14.cents() > 0 {
            pending_negative.max(Money::ZERO).min(r14)
        } else {
            Money::ZERO
        },
    );
    let r17 = r14 - r15;
    let r19 = r17;

    for (k, v) in [
        (1, income),
        (2, expenses),
        (3, r03),
        (4, r04),
        (5, r05),
        (6, withholdings),
        (7, r07),
        (12, r12),
        (13, r13),
        (14, r14),
        (15, r15),
        (17, r17),
        (19, r19),
    ] {
        b.insert(k, v);
    }

    if !hard.is_zero() && !manual_02 {
        warnings.push(format!(
            "[02] incluye {hard} de provisiones y gastos de difícil justificación (estimación \
             directa simplificada: {} del rendimiento neto de {}, como máximo 2.000 € al año)",
            crate::m100::hard_to_justify_rate(year),
            income - i.expenses
        ));
    }
    if r05.cents() < 0 {
        warnings.push("[05] negativa: revisa los modelos 130 anteriores".into());
    }
    let declaration = match r19.cents() {
        c if c > 0 && i.iban.is_some() => DeclarationType130::Domiciliacion,
        c if c > 0 => DeclarationType130::Ingreso,
        0 => DeclarationType130::Negativa,
        // En el 4T ya no quedan pagos del año en los que deducirlo: «N» (negativa).
        _ if quarter == 4 => DeclarationType130::Negativa,
        _ => DeclarationType130::ADeducir,
    };
    Result130 {
        year,
        quarter,
        boxes: b,
        declaration,
        warnings,
    }
}

pub fn spec() -> &'static Spec {
    static SPEC: std::sync::OnceLock<Spec> = std::sync::OnceLock::new();
    SPEC.get_or_init(|| Spec::parse(DR_JSON))
}

/// Fichero del 130.
pub fn file(r: &Result130, who: &Declarant, iban: Option<&str>) -> Result<String, DrError> {
    let s = spec();
    let period = format!("{}T", r.quarter);
    let p = s.page("01")?;
    let mut w = PageWriter::new(p);
    w.text(6, r.declaration.code())?
        .text(7, &who.nif)?
        .text(8, &who.name)?
        .text(9, who.given_name.as_deref().unwrap_or(""))?
        .text(10, &r.year.to_string())?
        .text(11, &period)?;
    for (box_, v) in &r.boxes {
        w.amount_box(*box_, *v)?;
    }
    if let (Some(iban), DeclarationType130::Domiciliacion) = (iban, r.declaration) {
        w.text(33, iban)?;
    }
    Ok(envelope(
        "130",
        r.year,
        &period,
        SOFTWARE_VERSION,
        DEVELOPER_NIF,
        &[w.finish()?],
    ))
}
