//! Modelo 202 por la modalidad de la base del periodo (art. 40.3 LIS), elección de la modalidad
//! por ejercicio y simulador para decidir la opción.
//!
//! - [`modalidad`]: la que se aplica a un período impositivo: la elegida en «Datos de los
//!   modelos» o, si la cifra de negocios de los 12 meses anteriores supera 6 millones, la de la
//!   base, que entonces es obligatoria.
//! - [`draft_base`]: borrador y fichero del 202 por la base: resultado contable desde el inicio
//!   del período impositivo hasta el día anterior al plazo (31/3, 30/9 y 30/11 en un ejercicio
//!   natural), con la corrección por el IS contabilizado, las casillas que el usuario fije a mano
//!   ([07], [08], [14], [27], [28], [30], [44]), las bases negativas pendientes con su límite,
//!   las retenciones de la 473 hasta esa fecha y los pagos anteriores del mismo período.
//! - [`simulate`]: lo que se pagaría en el año por cada modalidad con lo que hay (contabilidad
//!   de los periodos ya cerrados, presupuesto o los mismos meses del año anterior) y el plazo de
//!   la opción en el censo.
//!
//! Plazo y efectos de la opción (art. 40.3 LIS, párrafos tercero y cuarto, texto consolidado
//! BOE-A-2014-12328 leído el 2026-11-16): en la declaración censal, en febrero si el período
//! coincide con el año natural; si no, en los dos primeros meses del período o hasta el fin del
//! plazo del primer pago, si es menor; vincula ese período y los siguientes hasta la renuncia,
//! en los mismos plazos.
//!
//! `TODO(VERIFY)`: la base del periodo no recoge otros ajustes extracontables (amortización
//! fiscal, multas, exención del art. 21) salvo los que se fijan a mano.

use serde::Serialize;
use taxpain_core::accounting::Origin;
use taxpain_core::accounting::fiscal_year::FiscalYear;
use taxpain_core::calendar::rules::add_months;
use taxpain_core::{Money, Nif};
pub use taxpain_models::m202::Modalidad;
use taxpain_models::m202::{self, InputBase202, Result202};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use time::{Date, Duration, Month, OffsetDateTime};

use super::{
    Draft, adjustments, corporate_tax_base, declarant, filed_boxes, income_expenses, is_profit_tax,
    settings, withholdings_473_until,
};
use crate::accounting;
use crate::error::{AppError, Result};

/// Casillas de la modalidad B que se pueden fijar a mano.
pub const ADJUSTABLE: [u16; 7] = [7, 8, 14, 27, 28, 30, 44];

/// Primer día del plazo del pago `period` (1 de abril, octubre o diciembre).
fn first_day(year: i32, period: u8) -> Result<Date> {
    let month =
        [Month::April, Month::October, Month::December][usize::from(period.clamp(1, 3) - 1)];
    Date::from_calendar_date(year, month, 1).map_err(|e| AppError::Other(e.to_string()))
}

/// Período impositivo en curso al empezar el plazo del pago y último día que computa (el
/// anterior al plazo: 31/3, 30/9 o 30/11 en un ejercicio natural).
pub fn period_window(v: &Vault, year: i32, period: u8) -> Result<(FiscalYear, Date)> {
    let day1 = first_day(year, period)?;
    let fy = accounting::fiscal_year_at(v, day1)?;
    Ok((fy, day1 - Duration::days(1)))
}

/// Modalidad que se aplica a un período impositivo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Modalidad202 {
    /// Ejercicio (período impositivo) al que se refiere.
    pub ejercicio: i32,
    /// La guardada para ese ejercicio (`None`: no se ha elegido; vale la de la cuota).
    pub elegida: Option<Modalidad>,
    /// La que se aplica.
    pub efectiva: Modalidad,
    /// Cifra de negocios de más de 6 M€: la base es obligatoria.
    pub obligatoria: bool,
    /// Cifra de negocios de los 12 meses anteriores al inicio del período, si se sabe.
    pub cifra_negocios: Option<Money>,
    /// Por qué es la que es (aviso para la interfaz y el borrador).
    pub aviso: Option<String>,
}

/// Cifra de negocios de los 12 meses anteriores al inicio del período impositivo (el último
/// cerrado: su 200 o la contabilidad).
fn turnover_before(v: &Vault, fy: &FiscalYear) -> Option<Money> {
    crate::payroll::director_turnover(v, fy.start)
        .ok()
        .and_then(|t| t.turnover)
}

/// Modalidad del 202 del período impositivo `ejercicio`.
pub fn modalidad(v: &Vault, ejercicio: i32) -> Result<Modalidad202> {
    let s = settings(v)?;
    let (ys, _) = s.year_settings(ejercicio);
    let fy = accounting::fiscal_year(v, ejercicio)?;
    let turnover = turnover_before(v, &fy);
    let by_figure = turnover.is_some_and(|t| t > m202::SIX_M);
    let obligatoria = s.turnover_over_6m || by_figure;
    let elegida = ys.m202_modalidad;
    let efectiva = if obligatoria {
        Modalidad::Base
    } else {
        elegida.unwrap_or_default()
    };
    let aviso = if obligatoria {
        let figure = match turnover {
            Some(t) if by_figure => format!("la cifra de negocios del ejercicio anterior fue {t}"),
            _ => "en «Datos de los modelos» está marcada la cifra de negocios de más de 6 \
                  millones"
                .to_owned(),
        };
        Some(format!(
            "El 202 de {ejercicio} va obligatoriamente por la base del periodo (art. 40.3 LIS): \
             {figure}, más de 6 millones de euros en los 12 meses anteriores al inicio del \
             período impositivo.{}",
            if elegida == Some(Modalidad::Cuota) {
                " La modalidad de la cuota que tenías elegida no vale."
            } else {
                ""
            }
        ))
    } else {
        None
    };
    Ok(Modalidad202 {
        ejercicio,
        elegida,
        efectiva,
        obligatoria,
        cifra_negocios: turnover,
        aviso,
    })
}

/// Modalidad del 202 del pago `period` de `year` (la del período impositivo en curso).
pub fn modalidad_del_pago(v: &Vault, year: i32, period: u8) -> Result<Modalidad202> {
    let (fy, _) = period_window(v, year, period)?;
    modalidad(v, fy.year)
}

/// Guarda la modalidad elegida para el ejercicio (`None`: sin elegir, vale la de la cuota).
pub fn set_modalidad(
    v: &Vault,
    ejercicio: i32,
    m: Option<Modalidad>,
    now: OffsetDateTime,
) -> Result<()> {
    let mut s = settings(v)?;
    let (ys, _) = s.year_settings(ejercicio);
    s.years.insert(
        ejercicio,
        super::YearSettings {
            m202_modalidad: m,
            ..ys
        },
    );
    v.set_setting(super::SETTINGS_KEY, &s)?;
    audit::append(
        v.connection(),
        &crate::invoicing::now_str(now),
        Actor::User,
        "models.202.modalidad",
        &ejercicio.to_string(),
        &serde_json::json!({ "modalidad": m }),
    )?;
    Ok(())
}

/// Gasto (positivo) o ingreso (negativo) por IS contabilizado en el ejercicio hasta `end`.
fn tax_expense_until(v: &Vault, label: i32, end: Date) -> Result<Money> {
    let mut total = Money::ZERO;
    for e in accounting::journal(v, label)? {
        if e.entry.date > end || matches!(e.entry.origin, Origin::Regularizacion | Origin::Cierre) {
            continue;
        }
        for l in e
            .entry
            .lines
            .iter()
            .filter(|l| is_profit_tax(l.account.as_str()))
        {
            total += l.net();
        }
    }
    Ok(total)
}

/// Pagos anteriores del mismo período impositivo por la base ([30]): los presentados ([34] o,
/// si se presentó por la cuota, [03]) o su borrador.
fn previous_payments(v: &Vault, year: i32, period: u8, fy: &FiscalYear) -> Result<Money> {
    let mut total = Money::ZERO;
    for (y, p) in earlier_payments(year, period) {
        if !first_day(y, p).is_ok_and(|d| fy.contains(d)) {
            continue;
        }
        let amount = match filed_boxes(v, "202", y, &format!("{p}P"))? {
            Some((_, boxes)) => m202::amount_due(&boxes),
            None => super::draft_202(v, y, p).map_or(Money::ZERO, |d| d.result),
        };
        total += amount.max(Money::ZERO);
    }
    Ok(total)
}

/// Pagos del 202 anteriores a `period` de `year` que pueden caer en el mismo período impositivo
/// (en uno de julio a junio, el 1P de un año va tras el 2P y el 3P del anterior).
fn earlier_payments(year: i32, period: u8) -> Vec<(i32, u8)> {
    let mut out: Vec<(i32, u8)> = (2..=3).map(|p| (year - 1, p)).collect();
    out.extend((1..period).map(|p| (year, p)));
    out
}

/// Datos de la modalidad B del pago `period` de `year` desde la contabilidad, con las casillas
/// fijadas a mano.
pub fn input_base(v: &Vault, year: i32, period: u8) -> Result<(InputBase202, Vec<String>)> {
    let s = settings(v)?;
    let (fy, end) = period_window(v, year, period)?;
    let (ys, _) = s.year_settings(fy.year);
    let mut notes = Vec::new();
    let (income, expenses) = income_expenses(v, fy.year, end)?;
    let tax = tax_expense_until(v, fy.year, end)?;
    let fixed = adjustments(v, "202", year, period)?;
    let get = |b: u16, auto: Money| fixed.get(&b).copied().unwrap_or(auto);
    let withholdings = withholdings_473_until(v, fy.year, Some(end))?;
    let previous = previous_payments(v, year, period, &fy)?;
    let turnover = turnover_before(v, &fy);
    // Dividendos de participadas cobrados en el periodo: el 95 % puede estar exento (art. 21
    // LIS), pero TaxPain no sabe si se cumplen los requisitos: se avisa.
    let dividends = dividends_760_until(v, fy.year, end)?;
    if dividends.cents() > 0 && !fixed.contains_key(&8) {
        notes.push(format!(
            "En el periodo hay {dividends} de dividendos de participadas (760): si tienes al \
             menos el 5 % desde hace un año, el 95 % está exento (art. 21 LIS); fija ese importe \
             en la casilla [08] (correcciones en disminución)"
        ));
    }
    let (dep_inc, dep_dec) = accounting::depreciation_differences(v, fy.year).unwrap_or_default();
    if !(dep_inc.is_zero() && dep_dec.is_zero()) && !fixed.contains_key(&7) {
        notes.push(
            "Hay diferencias entre la amortización contable y la fiscal en el ejercicio: la parte \
             de este periodo no se calcula sola; fíjala en [07] o [08] si es importante"
                .into(),
        );
    }
    if !fixed.is_empty() {
        notes.push(format!(
            "Casillas fijadas a mano en este periodo: {}",
            fixed
                .keys()
                .map(|b| format!("[{b:02}]"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let input = InputBase202 {
        period_start: fy.start,
        tax_year: fy.year,
        cnae: s.cnae.clone(),
        tax_rate: s.corporate_tax_rate.clone(),
        regime: ys.corporate_regime.unwrap_or(s.corporate_regime),
        turnover,
        turnover_over_6m: s.turnover_over_6m,
        turnover_under_1m: s.turnover_under_1m,
        accounting_result: income - expenses - tax,
        tax_expense: tax.max(Money::ZERO),
        tax_income: (-tax).max(Money::ZERO),
        other_increases: get(7, Money::ZERO),
        other_decreases: get(8, Money::ZERO),
        capitalization_remainder: get(44, Money::ZERO),
        pending_losses: ys.pending_losses,
        losses_override: fixed.get(&14).copied(),
        bonuses: get(27, Money::ZERO),
        withholdings: get(28, withholdings),
        previous_payments: get(30, previous),
        iban: s.iban.clone().filter(|_| s.domiciliar),
    };
    Ok((input, notes))
}

/// Saldo acreedor de la 760 del ejercicio hasta `end`.
fn dividends_760_until(v: &Vault, label: i32, end: Date) -> Result<Money> {
    let mut total = Money::ZERO;
    for e in accounting::journal(v, label)? {
        if e.entry.date > end || matches!(e.entry.origin, Origin::Regularizacion | Origin::Cierre) {
            continue;
        }
        for l in e
            .entry
            .lines
            .iter()
            .filter(|l| l.account.as_str().starts_with("760"))
        {
            total -= l.net();
        }
    }
    Ok(total)
}

/// Borrador del 202 por la base (art. 40.3 LIS).
pub fn draft_base(v: &Vault, year: i32, period: u8, m: &Modalidad202) -> Result<Draft> {
    let who = declarant(v)?;
    let (input, notes) = input_base(v, year, period)?;
    let mut r: Result202 = m202::compute_base(year, period, &input);
    if let Some(a) = &m.aviso {
        r.warnings.insert(0, a.clone());
    }
    r.warnings.extend(notes);
    let (fy, end) = period_window(v, year, period)?;
    r.warnings.push(format!(
        "Base del periodo: contabilidad del {} al {} (resultado contable sin la regularización)",
        fmt(fy.start),
        fmt(end)
    ));
    Ok(Draft {
        model: "202".into(),
        year,
        quarter: period,
        boxes: r.boxes.iter().map(|(k, v)| (*k, *v)).collect(),
        payees: vec![],
        details: vec![],
        declaration: r.declaration.clone(),
        result: r.result(),
        warnings: r.warnings.clone(),
        file_name: format!("{}_{year}_{period}P_202.txt", who.nif),
        file: m202::file_base(&r, &who, &input)?,
        terceros: vec![],
    })
}

fn fmt(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

// ---------- Simulador ----------

/// De dónde sale la base de un periodo en el simulador.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Fuente {
    /// El periodo ya ha acabado: la contabilidad del ejercicio.
    Contabilidad,
    /// Presupuesto del ejercicio (ingresos menos gastos de esos meses).
    Presupuesto,
    /// Los mismos meses del ejercicio anterior.
    AnoAnterior,
    /// No hay datos: base cero.
    SinDatos,
}

/// Un pago del simulador.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct PeriodoSimulado {
    /// 1, 2 o 3.
    pub period: u8,
    /// Último día que computa en la base.
    pub hasta: Date,
    /// Por la cuota (art. 40.2): `None` si falta la cuota del IS que sirve de base.
    pub cuota: Option<Money>,
    /// Base imponible del periodo ([13]) que usa la modalidad de la base.
    pub base_imponible: Money,
    /// Por la base (art. 40.3).
    pub base: Money,
    pub fuente: Fuente,
}

/// Comparación de las dos modalidades en un ejercicio.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Simulacion202 {
    pub ejercicio: i32,
    pub modalidad: Modalidad202,
    pub periodos: Vec<PeriodoSimulado>,
    /// Total por la cuota (`None` si falta algún dato).
    pub total_cuota: Option<Money>,
    pub total_base: Money,
    /// Plazo para optar por la base (o renunciar) en el censo, con efectos en este ejercicio.
    pub opcion_desde: Date,
    pub opcion_hasta: Date,
    /// Conclusión en lenguaje llano.
    pub conclusion: String,
    pub notas: Vec<String>,
}

/// Plazo de la opción (y de la renuncia) por la modalidad de la base para el período impositivo
/// `fy` (art. 40.3 LIS, párrafo cuarto): febrero si coincide con el año natural; si no, los dos
/// primeros meses o, si acaba antes, hasta el fin del plazo del primer pago fraccionado.
pub fn option_window(fy: &FiscalYear) -> (Date, Date) {
    let natural = fy.start.month() == Month::January && fy.start.day() == 1;
    if natural {
        let y = fy.start.year();
        let from = Date::from_calendar_date(y, Month::February, 1).unwrap_or(fy.start);
        let to = Date::from_calendar_date(y, Month::February, Month::February.length(y))
            .unwrap_or(fy.start);
        return (from, to);
    }
    let two = add_months(fy.start, 2) - Duration::days(1);
    // Fin del plazo del primer 202 del período (día 20 de abril, octubre o diciembre).
    let first = (fy.start.year()..=fy.start.year() + 1)
        .flat_map(|y| [Month::April, Month::October, Month::December].map(|m| (y, m)))
        .filter_map(|(y, m)| Date::from_calendar_date(y, m, 20).ok())
        .find(|d| *d >= fy.start && fy.contains(*d));
    (fy.start, first.map_or(two, |f| two.min(f)))
}

/// Resultado (ingresos menos gastos) presupuestado de toda la empresa de enero a `month`.
fn budget_until(v: &Vault, label: i32, month: u8) -> Result<Option<Money>> {
    let lines: Vec<_> = crate::presupuestos::list(v, label)?
        .into_iter()
        .filter(|l| l.center.is_empty())
        .collect();
    if lines.is_empty() {
        return Ok(None);
    }
    let mut total = Money::ZERO;
    for l in lines {
        let sum: Money = l.months.iter().take(usize::from(month)).copied().sum();
        if l.account.starts_with('7') {
            total += sum;
        } else {
            total -= sum;
        }
    }
    Ok(Some(total))
}

/// El mismo día un año antes (29/2 → 28/2).
fn year_before(d: Date) -> Date {
    add_months(d, -12)
}

/// Simulación del ejercicio `ejercicio` (período impositivo) a fecha `today`.
pub fn simulate(v: &Vault, ejercicio: i32, today: Date) -> Result<Simulacion202> {
    let who = declarant(v)?;
    if Nif::parse(&who.nif).is_ok_and(|n| n.is_persona_fisica()) {
        return Err(AppError::Other("el modelo 202 es para sociedades".into()));
    }
    let fy = accounting::fiscal_year(v, ejercicio)?;
    let m = modalidad(v, ejercicio)?;
    let s = settings(v)?;
    let mut notas = Vec::new();
    // Pagos del período impositivo, en orden.
    let pagos: Vec<(i32, u8)> = (fy.start.year()..=fy.end.year())
        .flat_map(|y| (1..=3u8).map(move |p| (y, p)))
        .filter(|(y, p)| first_day(*y, *p).is_ok_and(|d| fy.contains(d)))
        .collect();
    let natural = fy.start.month() == Month::January && fy.start.day() == 1;
    let prev_fy = accounting::fiscal_year(v, ejercicio - 1).ok();
    let mut periodos = Vec::new();
    let mut paid_base = Money::ZERO;
    let mut cuota_total = Some(Money::ZERO);
    for (y, p) in pagos {
        let end = first_day(y, p)? - Duration::days(1);
        // Por la cuota: 18 % de la cuota del último IS vencido.
        let cuota = corporate_tax_base(v, y, p)?.0.map(|b| {
            m202::compute(
                y,
                p,
                &m202::Input202 {
                    base: b,
                    period_start: fy.start,
                    cnae: String::new(),
                    tax_rate: String::new(),
                    turnover_over_6m: false,
                    turnover_under_1m: false,
                    iban: None,
                },
            )
            .result()
        });
        cuota_total = cuota_total.zip(cuota).map(|(a, b)| a + b);
        // Por la base.
        let (mut input, _) = input_base(v, y, p)?;
        let fuente = if end < today {
            Fuente::Contabilidad
        } else {
            let budget = if natural {
                budget_until(v, ejercicio, u8::from(end.month()))?
            } else {
                None
            };
            let prev = match &prev_fy {
                Some(pf) => {
                    let pend = year_before(end).min(pf.end);
                    let (i, e) = income_expenses(v, pf.year, pend)?;
                    (!(i.is_zero() && e.is_zero()))
                        .then_some((i - e, withholdings_473_until(v, pf.year, Some(pend))?))
                }
                None => None,
            };
            let (result, withheld, f) = match (budget, prev) {
                (Some(b), prev) => (b, prev.map_or(Money::ZERO, |x| x.1), Fuente::Presupuesto),
                (None, Some((r, w))) => (r, w, Fuente::AnoAnterior),
                (None, None) => (Money::ZERO, Money::ZERO, Fuente::SinDatos),
            };
            // Previsión antes del impuesto: sin IS contabilizado; los ajustes fijados a mano de
            // un periodo futuro (si los hay) se mantienen.
            input.accounting_result = result;
            input.tax_expense = Money::ZERO;
            input.tax_income = Money::ZERO;
            input.withholdings = withheld;
            f
        };
        input.previous_payments = paid_base;
        let r = m202::compute_base(y, p, &input);
        let base = r.result();
        paid_base += base;
        periodos.push(PeriodoSimulado {
            period: p,
            hasta: end,
            cuota,
            base_imponible: r.boxes.get(&13).copied().unwrap_or(Money::ZERO),
            base,
            fuente,
        });
    }
    let total_base: Money = periodos.iter().map(|p| p.base).sum();
    let (opcion_desde, opcion_hasta) = option_window(&fy);
    let plazo = format!("del {} al {}", fmt(opcion_desde), fmt(opcion_hasta));
    let conclusion = if m.obligatoria {
        format!(
            "Con más de 6 millones de cifra de negocios, en {ejercicio} el 202 va \
             obligatoriamente por la base del periodo: no hay opción."
        )
    } else {
        match cuota_total {
            None => "Falta la cuota del Impuesto sobre Sociedades de algún ejercicio anterior \
                     para calcular la modalidad de la cuota: indícala en «Datos de los modelos» \
                     para comparar."
                .to_owned(),
            Some(c) if total_base < c => format!(
                "Por la base pagarías {} menos en el año ({total_base} frente a {c}). Para \
                 aplicarla hay que optar en el censo (modelo 036) {plazo}; la opción vale para \
                 este ejercicio y los siguientes hasta que renuncies, también en ese plazo.",
                c - total_base
            ),
            Some(c) if total_base > c => format!(
                "Por la cuota pagas {} menos en el año ({c} frente a {total_base}): no conviene \
                 optar por la base.",
                total_base - c
            ),
            Some(c) => format!(
                "Las dos modalidades salen iguales ({c}): no hace falta cambiar nada en el censo."
            ),
        }
    };
    if !m.obligatoria && today > opcion_hasta {
        notas.push(format!(
            "El plazo para optar con efectos en {ejercicio} ya pasó ({plazo}): la modalidad de \
             ese ejercicio es la que conste en el censo; la simulación sirve para el siguiente."
        ));
    }
    if periodos.iter().any(|p| p.fuente != Fuente::Contabilidad) {
        notas.push(
            "Los periodos que aún no han acabado se estiman con el presupuesto o, si no lo hay, \
             con los mismos meses del año anterior (antes del impuesto). Es una previsión."
                .into(),
        );
    }
    notas.push(
        "Los pagos fraccionados son a cuenta: lo pagado de más o de menos se regulariza en el \
         200 del ejercicio. La diferencia entre modalidades es de tesorería, no de impuesto."
            .into(),
    );
    if s.corporate_tax_rate.trim().is_empty() {
        notas.push(
            "Indica el tipo de gravamen y el régimen del IS en «Datos de los modelos»: el \
             porcentaje de la base sale de ellos"
                .into(),
        );
    }
    Ok(Simulacion202 {
        ejercicio,
        modalidad: m,
        periodos,
        total_cuota: cuota_total,
        total_base,
        opcion_desde,
        opcion_hasta,
        conclusion,
        notas,
    })
}
