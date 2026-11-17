//! Modelo 202: pago fraccionado del Impuesto sobre Sociedades, modalidades del art. 40.2 y del
//! art. 40.3 LIS.
//!
//! Base legal: art. 40 Ley 27/2014 (LIS); DA 14.ª LIS (pago mínimo); Orden HFP/227/2017 y
//! modificaciones. Diseño de registro: `DR202e25.xlsx` (ejercicio 2025 y siguientes), que trae
//! las casillas de las dos modalidades (campo 27 de la página 1: «A» o «B», nota 11).
//!
//! Modalidad A (art. 40.2 LIS): 18 % de la cuota íntegra del último período impositivo cuyo
//! plazo de declaración estuviera vencido al inicio de cada plazo, minorada en deducciones,
//! bonificaciones, retenciones e ingresos a cuenta. Periodos 1P (abril), 2P (octubre) y 3P
//! (diciembre).
//!
//! Modalidad B (art. 40.3 LIS): base imponible de los 3, 9 u 11 primeros meses del año (o, si el
//! periodo impositivo no es el natural, de los días transcurridos desde su inicio hasta el día
//! anterior al del plazo) por el porcentaje de 5/7 del tipo de gravamen redondeado por defecto
//! (17 % con el 25 %), menos bonificaciones, retenciones e ingresos a cuenta del periodo y los
//! pagos fraccionados anteriores del mismo periodo impositivo ([`compute_base`]). Es obligatoria
//! si la cifra de negocios de los 12 meses anteriores al inicio del periodo supera 6 millones;
//! si no, se opta por ella en el censo (036) en febrero. Con 10 millones o más, el porcentaje es
//! 19/20 del tipo redondeado por exceso (24 %) y hay un pago mínimo del 23 % del resultado
//! contable positivo del periodo (DA 14.ª LIS).
//!
//! Fuera de alcance: complementarias, regímenes especiales (cooperativas, navieras, Canarias,
//! ZEC, Ceuta y Melilla), reserva de nivelación y territorios forales.
//!
//! `TODO(VERIFY)`: que [03] = 18 % × [01] − [02] (el DR no lo dice; es lo que aplica el
//! formulario); formato del tipo de gravamen (campo libre de 15 posiciones); en la modalidad B,
//! que [32] no pueda ser negativo, el tramo de 50.000 € de las microempresas sin prorratear y
//! que la marca 19 de la página 1 corresponda a toda entidad de reducida dimensión con tipo
//! propio.
//!
//! Porcentajes cotejados el 2026-11-16 con el texto consolidado de la LIS (BOE-A-2014-12328, a
//! 02-09-2026): 18 % (art. 40.2); 5/7 del tipo redondeado por defecto (art. 40.3); con 10 M€ o
//! más, 19/20 redondeado por exceso y pago mínimo del 23 % (DA 14.ª.1, redacción del art. 71 de
//! la Ley 6/2018 y de la Ley 8/2018). Ni la STC 11/2024 ni el RDL 4/2024 tocan la DA 14.ª: la
//! sentencia anuló la DA 15.ª (límites a las bases negativas del RDL 3/2016), que la Ley 7/2024
//! repuso; lo que se anuló de los pagos fraccionados fue el RDL 2/2016 (STC 78/2020) y la Ley
//! 6/2018 ya lo había vuelto a aprobar para los periodos iniciados desde 2018.

use serde::{Deserialize, Serialize};
use taxpain_core::{Money, Rate};
use time::Date;

use crate::dr::{DrError, PageWriter, Spec, envelope};
use crate::m200::{SIXTY_M, TWENTY_M, TaxRegime, small_company_rates};
use crate::m303::Boxes;
use crate::{DEVELOPER_NIF, Declarant, SOFTWARE_VERSION};

pub const DR_JSON: &str = include_str!("../../../fixtures/aeat/dr/202.json");

pub fn spec() -> &'static Spec {
    static SPEC: std::sync::OnceLock<Spec> = std::sync::OnceLock::new();
    SPEC.get_or_init(|| Spec::parse(DR_JSON))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Input202 {
    /// [01] Cuota íntegra del último IS presentado menos deducciones, bonificaciones,
    /// retenciones e ingresos a cuenta (casilla de «cuota a ingresar» antes de pagos
    /// fraccionados del modelo 200).
    pub base: Money,
    /// Fecha de inicio del período impositivo en curso.
    pub period_start: Date,
    /// CNAE de la actividad principal (CNAE-2025 desde 2026).
    pub cnae: String,
    /// Tipo de gravamen del ejercicio en curso, tal como se escribe en el formulario.
    pub tax_rate: String,
    /// Cifra de negocios de los 12 meses anteriores superior a 6 millones (obliga a la B).
    pub turnover_over_6m: bool,
    /// Cifra de negocios del período anterior inferior a 1 millón.
    pub turnover_under_1m: bool,
    pub iban: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Result202 {
    pub year: i32,
    /// 1, 2 o 3 (1P, 2P, 3P).
    pub period: u8,
    /// Modalidad del cálculo (art. 40.2 o 40.3 LIS).
    #[serde(default)]
    pub modalidad: Modalidad,
    pub boxes: Boxes,
    /// I, U o N.
    pub declaration: String,
    pub warnings: Vec<String>,
}

impl Result202 {
    /// Importe a ingresar: [03] en la modalidad A, [34] en la B.
    pub fn result(&self) -> Money {
        amount_due(&self.boxes)
    }
}

/// Importe a ingresar de un 202 por sus casillas: [03] (modalidad A) o [34] (modalidad B). Sólo
/// una de las dos tiene importe.
pub fn amount_due(boxes: &Boxes) -> Money {
    let get = |b: u16| boxes.get(&b).copied().unwrap_or(Money::ZERO);
    if boxes.contains_key(&34) {
        get(34)
    } else {
        get(3)
    }
}

/// Modalidad del pago fraccionado.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Modalidad {
    /// Art. 40.2 LIS: 18 % de la cuota del último Impuesto sobre Sociedades vencido.
    #[default]
    Cuota,
    /// Art. 40.3 LIS: porcentaje sobre la base imponible de los 3, 9 u 11 primeros meses.
    Base,
}

impl Modalidad {
    /// Letra del campo 27 de la página 1 (nota 11 del DR).
    pub fn letter(self) -> &'static str {
        match self {
            Modalidad::Cuota => "A",
            Modalidad::Base => "B",
        }
    }
}

pub fn compute(year: i32, period: u8, i: &Input202) -> Result202 {
    let mut warnings = Vec::new();
    if i.turnover_over_6m {
        warnings.push(OVER_6M.into());
    }
    let base = i.base.max(Money::ZERO);
    let pay = base.apply_rate(Rate::percent(18));
    let mut boxes = Boxes::new();
    boxes.insert(1, base);
    boxes.insert(3, pay);
    let declaration = match (pay.cents() > 0, i.iban.is_some()) {
        (false, _) => "N",
        (true, true) => "U",
        (true, false) => "I",
    };
    if pay.is_zero() {
        warnings.push(
            "Con base cero no hay pago; sólo las grandes empresas deben presentar el 202 sin importe"
                .into(),
        );
    }
    Result202 {
        year,
        period,
        modalidad: Modalidad::Cuota,
        boxes,
        declaration: declaration.into(),
        warnings,
    }
}

/// Aviso (y error del fichero) para las empresas de más de 6 millones con la modalidad A.
pub const OVER_6M: &str = "Con una cifra de negocios superior a 6 millones el pago fraccionado se \
     calcula obligatoriamente por la modalidad del art. 40.3 LIS (base imponible del periodo), no \
     por la cuota: no se genera el fichero de la modalidad del art. 40.2.";

/// Fichero del 202 (modalidad del art. 40.2 LIS). Con más de 6 millones de cifra de negocios la
/// modalidad del art. 40.3 es obligatoria: se rechaza en vez de escribir un fichero con la
/// modalidad «A» y el 18 % de la cuota, que sería incorrecto (ver [`file_base`]).
pub fn file(r: &Result202, who: &Declarant, i: &Input202) -> Result<String, DrError> {
    if i.turnover_over_6m {
        return Err(DrError::Unsupported(OVER_6M.into()));
    }
    let head = Header {
        period_start: i.period_start,
        cnae: &i.cnae,
        tax_rate: &i.tax_rate,
        turnover_over_6m: false,
        turnover_under_1m: i.turnover_under_1m,
        reduced_rate: false,
        turnover_band: "0",
        iban: i.iban.as_deref(),
    };
    write_file(r, who, &head)
}

/// Datos de la página 1 (y del IBAN de la 2) que no son casillas.
struct Header<'a> {
    period_start: Date,
    cnae: &'a str,
    tax_rate: &'a str,
    turnover_over_6m: bool,
    turnover_under_1m: bool,
    /// Campo 19: reducida dimensión con el tipo de gravamen específico.
    reduced_rate: bool,
    /// Campo 26: tramo de la cifra de negocios de 10 M€ o más («0» a «3»).
    turnover_band: &'a str,
    iban: Option<&'a str>,
}

fn write_file(r: &Result202, who: &Declarant, h: &Header) -> Result<String, DrError> {
    let s = spec();
    let period = format!("{}P", r.period);
    let start = format!(
        "{:02}{:02}{:04}",
        h.period_start.day(),
        u8::from(h.period_start.month()),
        h.period_start.year()
    );
    let page1 = s.page("(1)")?;
    let page2 = s.page("(2)")?;

    let mut w = PageWriter::new(page1);
    w.text(6, &r.declaration)?
        .text(7, &who.nif)?
        .text(8, &who.name)?
        .text(9, who.given_name.as_deref().unwrap_or(""))?
        .text(11, &r.year.to_string())?
        .text(12, &period)?
        .text(13, &start)?
        .text(14, h.cnae)?
        .text(25, h.tax_rate)?
        .text(26, h.turnover_band)?
        .text(27, r.modalidad.letter())?;
    if h.reduced_rate {
        w.text(19, "X")?;
    }
    if h.turnover_over_6m {
        w.text(20, "X")?;
    }
    if h.turnover_under_1m {
        w.text(23, "X")?;
    }
    for (b, v) in r
        .boxes
        .iter()
        .filter(|(b, _)| page1.field_for_box(**b).is_ok())
    {
        w.amount_box(*b, *v)?;
    }
    let p1 = w.finish()?;

    let mut w = PageWriter::new(page2);
    for (b, v) in r
        .boxes
        .iter()
        .filter(|(b, _)| page1.field_for_box(**b).is_err())
    {
        w.amount_box(*b, *v)?;
    }
    if let (Some(iban), "U") = (h.iban, r.declaration.as_str()) {
        w.text(54, iban)?;
    }
    let p2 = w.finish()?;
    Ok(envelope(
        "202",
        r.year,
        &period,
        SOFTWARE_VERSION,
        DEVELOPER_NIF,
        &[p1, p2],
    ))
}

// ---------- Modalidad B (art. 40.3 LIS) ----------

/// Un millón de euros: mínimo de bases negativas compensables (art. 26.1 LIS).
const ONE_M: Money = Money::from_cents(100_000_000);
/// Diez millones: porcentaje de 19/20 y pago mínimo (art. 40.3 y DA 14.ª LIS).
pub const TEN_M: Money = Money::from_cents(1_000_000_000);
/// Seis millones: por encima, la modalidad B es obligatoria (art. 40.3 LIS).
pub const SIX_M: Money = Money::from_cents(600_000_000);
/// Primer tramo de las microempresas (art. 29.1 LIS).
const FIFTY_K: Money = Money::from_cents(5_000_000);

/// Datos de la modalidad del art. 40.3 LIS para un periodo (1P, 2P o 3P).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputBase202 {
    /// Fecha de inicio del período impositivo en curso.
    pub period_start: Date,
    /// Ejercicio (año en que empieza el período impositivo): da los tipos de reducida
    /// dimensión.
    pub tax_year: i32,
    pub cnae: String,
    pub tax_rate: String,
    pub regime: TaxRegime,
    /// Cifra de negocios de los 12 meses anteriores al inicio del período impositivo, si se
    /// sabe: con 10 M€ o más cambia el porcentaje y hay pago mínimo; con 20 y 60 M€ se limita la
    /// compensación de bases negativas (DA 15.ª LIS).
    pub turnover: Option<Money>,
    /// Más de 6 M€ en esos 12 meses (campo 20 de la página 1).
    pub turnover_over_6m: bool,
    /// Cifra de negocios del período anterior inferior a 1 millón (campo 23).
    pub turnover_under_1m: bool,
    /// [04] Resultado contable del periodo (después del IS contabilizado).
    pub accounting_result: Money,
    /// [05] Gasto por IS contabilizado en el periodo (corrección en aumento).
    pub tax_expense: Money,
    /// [06] Ingreso por IS contabilizado en el periodo (corrección en disminución).
    pub tax_income: Money,
    /// [07] y [08] Resto de correcciones al resultado contable (p. ej. el 95 % exento de los
    /// dividendos de participadas, art. 21 LIS, en [08]).
    pub other_increases: Money,
    pub other_decreases: Money,
    /// [44] Remanente de reserva de capitalización de ejercicios anteriores no aplicado.
    pub capitalization_remainder: Money,
    /// Bases imponibles negativas pendientes al empezar el período impositivo.
    pub pending_losses: Money,
    /// [14] fijado a mano (si no, se compensa lo pendiente con el límite del art. 26.1 LIS).
    pub losses_override: Option<Money>,
    /// [27] Bonificaciones del periodo.
    pub bonuses: Money,
    /// [28] Retenciones e ingresos a cuenta soportados en el periodo.
    pub withholdings: Money,
    /// [30] Pagos fraccionados anteriores del mismo período impositivo.
    pub previous_payments: Money,
    pub iban: Option<String>,
}

/// Porcentajes de la modalidad B: (primer tramo, segundo tramo si lo hay). 5/7 del tipo de
/// gravamen redondeado por defecto (art. 40.3, párrafo segundo, LIS: 17 % con el 25 %) o, con
/// cifra de negocios de 10 M€ o más, 19/20 redondeado por exceso (24 %). Las microempresas
/// tienen dos tipos y, por tanto, dos porcentajes (casillas B2 del modelo).
pub fn base_percentages(regime: TaxRegime, tax_year: i32, large: bool) -> (i32, Option<i32>) {
    let ((micro_a, micro_b), reduced) = small_company_rates(tax_year);
    let rates = match regime {
        TaxRegime::General => (25, None),
        TaxRegime::ReducidaDimension => (reduced, None),
        TaxRegime::Micro => (micro_a, Some(micro_b)),
        TaxRegime::NuevaCreacion => (15, None),
    };
    // Con 10 M€ o más no hay tipos de reducida dimensión (art. 101 LIS): se toma el general.
    if large {
        let r = match regime {
            TaxRegime::NuevaCreacion => 15,
            _ => 25,
        };
        // 19/20 redondeado por exceso.
        return ((19 * r + 19) / 20, None);
    }
    let pct = |r: i32| 5 * r / 7;
    (pct(rates.0), rates.1.map(pct))
}

fn pct_box(p: i32) -> Money {
    Money::from_cents(i64::from(p) * 100)
}

/// Pago fraccionado por la modalidad del art. 40.3 LIS. `period`: 1, 2 o 3 (3, 9 u 11 meses).
pub fn compute_base(year: i32, period: u8, i: &InputBase202) -> Result202 {
    let mut warnings = Vec::new();
    let large = i.turnover.is_some_and(|t| t >= TEN_M);
    let r04 = i.accounting_result;
    let r05 = i.tax_expense.max(Money::ZERO);
    let r06 = i.tax_income.max(Money::ZERO);
    let r07 = i.other_increases.max(Money::ZERO);
    let r08 = i.other_decreases.max(Money::ZERO);
    let r38 = r05 + r07;
    let r39 = r06 + r08;
    let r13 = r04 + r38 - r39;
    let positive = r13.max(Money::ZERO);
    let r44 = i.capitalization_remainder.max(Money::ZERO).min(positive);
    // Bases negativas: hasta 1 M€ y, por encima, el 70 % de la base previa (50 % o 25 % con 20 o
    // 60 M€ de cifra de negocios, DA 15.ª LIS), como en el 200. TODO(VERIFY): el millón del
    // periodo entero, sin prorratear por los meses del pago fraccionado.
    let loss_pct = match i.turnover {
        Some(t) if t >= SIXTY_M => 25,
        Some(t) if t >= TWENTY_M => 50,
        _ => 70,
    };
    let limit = ONE_M.max(positive.apply_rate(Rate::percent(loss_pct)));
    let room = (positive - r44).max(Money::ZERO);
    let r14 = match i.losses_override {
        Some(m) => m.max(Money::ZERO).min(room),
        None => i.pending_losses.max(Money::ZERO).min(limit).min(room),
    };
    if i.losses_override.is_none() && i.pending_losses > r14 && room > r14 {
        warnings.push(format!(
            "Quedan bases negativas sin compensar en este pago por el límite del art. 26.1 LIS              ({loss_pct} % de la base previa, con un mínimo de 1 millón)"
        ));
    }
    let r16 = (r13 - r44 - r14).max(Money::ZERO);
    let (p1, p2) = base_percentages(i.regime, i.tax_year, large);

    let mut b = Boxes::new();
    for (k, v) in [
        (4, r04),
        (5, r05),
        (6, r06),
        (7, r07),
        (8, r08),
        (38, r38),
        (39, r39),
        (13, r13),
        (44, r44),
        (14, r14),
    ] {
        b.insert(k, v);
    }
    let previo = match p2 {
        // B2: más de un porcentaje (microempresas: hasta 50.000 € y el resto).
        Some(p2) => {
            let t1 = r16.min(FIFTY_K);
            let t2 = r16 - t1;
            let r22 = t1.apply_rate(Rate::percent(p1));
            let r25 = t2.apply_rate(Rate::percent(p2));
            let r26 = r22 + r25;
            for (k, v) in [
                (19, r16),
                (20, t1),
                (21, pct_box(p1)),
                (22, r22),
                (23, t2),
                (24, pct_box(p2)),
                (25, r25),
                (26, r26),
            ] {
                b.insert(k, v);
            }
            r26
        }
        None => {
            let r18 = r16.apply_rate(Rate::percent(p1));
            b.insert(16, r16);
            b.insert(17, pct_box(p1));
            b.insert(18, r18);
            r18
        }
    };
    let r27 = i.bonuses.max(Money::ZERO);
    let r28 = i.withholdings.max(Money::ZERO);
    let r30 = i.previous_payments.max(Money::ZERO);
    let raw32 = previo - r27 - r28 - r30;
    let r32 = raw32.max(Money::ZERO);
    // Pago mínimo de las empresas de 10 M€ o más (DA 14.ª LIS): 23 % del resultado positivo de
    // la cuenta de pérdidas y ganancias del periodo, menos los pagos anteriores del mismo
    // período impositivo. La DA 14.ª.1.a excluye de ese resultado las quitas y esperas de un
    // acuerdo de acreedores (salvo la parte que se integre en la base), los aumentos de capital
    // por compensación de créditos que no se integran (art. 17.2), las rentas exentas de las
    // entidades parcialmente exentas y las bonificadas del art. 34 y de las navieras de
    // Canarias; no se aplica a las entidades de los apartados 3, 4 y 5 del art. 29, a las SOCIMI
    // ni a las de capital-riesgo, y es del 25 % para las del art. 29.6. No excluye los
    // dividendos ni las plusvalías exentas de los arts. 21 y 22. Aquí no se resta ninguna
    // exclusión ni se aplica el 25 %: se avisa.
    let r33 = if large {
        (r04.max(Money::ZERO).apply_rate(Rate::percent(23)) - r30).max(Money::ZERO)
    } else {
        Money::ZERO
    };
    let r34 = r32.max(r33);
    for (k, v) in [
        (27, r27),
        (28, r28),
        (29, pct_box(100)),
        (30, r30),
        (32, r32),
        (33, r33),
        (34, r34),
    ] {
        b.insert(k, v);
    }
    // Sin importe no se escriben, salvo las que identifican el cálculo.
    b.retain(|k, v| !v.is_zero() || matches!(k, 4 | 13 | 16 | 17 | 18 | 19 | 26 | 29 | 32 | 34));

    let months = [3, 9, 11][usize::from(period.clamp(1, 3) - 1)];
    warnings.push(format!(
        "Modalidad del art. 40.3 LIS: {} % de la base imponible de los {months} primeros meses \
         del período impositivo{}, menos retenciones y pagos anteriores",
        p1,
        p2.map(|p| format!(" ({p} % por encima de 50.000 €)"))
            .unwrap_or_default()
    ));
    if large {
        warnings.push(
            "Cifra de negocios de 10 millones o más: porcentaje de 19/20 del tipo (24 %) y pago \
             mínimo del 23 % del resultado contable positivo del periodo (art. 40.3 y DA 14.ª \
             LIS). El pago mínimo no resta los dividendos exentos; sí las quitas y esperas de \
             un acuerdo de acreedores y las demás rentas que excluye la DA 14.ª: revísalas."
                .into(),
        );
    }
    if raw32.cents() < 0 && r34.is_zero() {
        warnings.push(
            "Las retenciones y los pagos anteriores superan el pago del periodo: sale cero \
             (lo que sobre se recupera en el 200)"
                .into(),
        );
    }
    let declaration = match (r34.cents() > 0, i.iban.is_some()) {
        (false, _) => "N",
        (true, true) => "U",
        (true, false) => "I",
    };
    Result202 {
        year,
        period,
        modalidad: Modalidad::Base,
        boxes: b,
        declaration: declaration.into(),
        warnings,
    }
}

/// Campo 26 de la página 1: tramo de la cifra de negocios de los 12 meses anteriores.
pub fn turnover_band(turnover: Option<Money>) -> &'static str {
    match turnover {
        Some(t) if t >= SIXTY_M => "3",
        Some(t) if t >= TWENTY_M => "2",
        Some(t) if t >= TEN_M => "1",
        _ => "0",
    }
}

/// Fichero del 202 por la modalidad del art. 40.3 LIS («B»).
pub fn file_base(r: &Result202, who: &Declarant, i: &InputBase202) -> Result<String, DrError> {
    if r.modalidad != Modalidad::Base {
        return Err(DrError::Unsupported(
            "el fichero de la modalidad B necesita el cálculo por la base".into(),
        ));
    }
    let head = Header {
        period_start: i.period_start,
        cnae: &i.cnae,
        tax_rate: &i.tax_rate,
        turnover_over_6m: i.turnover_over_6m || i.turnover.is_some_and(|t| t > SIX_M),
        turnover_under_1m: i.turnover_under_1m,
        reduced_rate: matches!(i.regime, TaxRegime::Micro | TaxRegime::ReducidaDimension),
        turnover_band: turnover_band(i.turnover),
        iban: i.iban.as_deref(),
    };
    write_file(r, who, &head)
}
