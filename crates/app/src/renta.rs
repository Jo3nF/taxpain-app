//! Renta del autónomo (modelo 100), parte de la actividad económica en estimación directa:
//! rendimiento neto desde la contabilidad del año, reducciones del art. 32 LIRPF, pagos
//! fraccionados del 130, retenciones soportadas y cotejo con los datos fiscales descargados.
//! Es un borrador para rellenar Renta Web: no se presenta nada.
//!
//! Reparto de las cuentas del PGC entre los conceptos de gasto del modelo 100 (grupo 6, sin el
//! impuesto sobre beneficios): 60/61 consumos, 640/641 sueldos, 642 de las nóminas Seguridad
//! Social de la empresa y el resto de la 642 cuotas de autónomo del titular, 643/644/649 otros
//! gastos de personal, 621 arrendamientos, 622 reparaciones, 623 profesionales, 628 suministros,
//! 624-627 y 629 otros servicios exteriores, 63 tributos, 66 financieros, 68 amortizaciones
//! (más la devengada aún sin contabilizar), 69 provisiones y el resto «otros conceptos».
//! Ingresos (grupo 7): 70-73 de explotación; 74, 75 y 79 otros. Los financieros (76) y los
//! beneficios del inmovilizado (77) no son rendimiento de la actividad: los primeros suelen ser
//! rendimientos del capital mobiliario y los segundos ganancias patrimoniales (art. 28.2 LIRPF).

use serde::Serialize;
use taxpain_core::accounting::Origin;
use taxpain_core::{Money, Nif};
use taxpain_models::m100::{self, Expenses, Input100, IrpfRegime, Result100};
use taxpain_vault::Vault;
use time::{Date, Month};

use crate::accounting;
use crate::error::{AppError, Result};
use crate::models::{self, CheckLine};

/// Borrador de la actividad para la Renta de un ejercicio.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ActivityDraft {
    pub year: i32,
    pub regime: IrpfRegime,
    pub result: Result100,
    /// Trimestres del 130 que no constan presentados.
    pub missing_130: Vec<String>,
    /// Cotejo con los datos fiscales de la AEAT (vacío si no se han descargado).
    pub check: Vec<CheckLine>,
    /// Fecha de los datos fiscales (`AAAAMMDD`), si los hay.
    pub check_date: Option<String>,
    pub notes: Vec<String>,
    /// Ganancias y pérdidas por criptomonedas (FIFO), si hay operaciones ese año.
    pub cripto: Option<crate::cripto::CriptoRenta>,
}

#[derive(Default)]
struct Ledger {
    operating: Money,
    other_income: Money,
    financial_income: Money,
    asset_gains: Money,
    exceptional: Money,
    provisions: Money,
    e: Expenses,
}

fn classify(v: &Vault, year: i32) -> Result<Ledger> {
    let mut l = Ledger::default();
    for st in accounting::journal(v, year)? {
        let entry = &st.entry;
        // Los asientos de criptomonedas no son de la actividad: sus ganancias y pérdidas van a la
        // base del ahorro (y ya salen aparte, en su bloque).
        if matches!(
            entry.origin,
            Origin::Regularizacion | Origin::Cierre | Origin::Cripto(_) | Origin::Valores(_)
        ) {
            continue;
        }
        let payroll = matches!(entry.origin, Origin::Nomina)
            || entry.lines.iter().any(|x| {
                let a = x.account.as_str();
                a.starts_with("640") || a.starts_with("641")
            });
        for line in &entry.lines {
            let a = line.account.as_str();
            let n = line.net();
            let starts = |p: &[&str]| p.iter().any(|x| a.starts_with(x));
            match line.account.group() {
                7 => {
                    let credit = -n;
                    if starts(&["76"]) {
                        l.financial_income += credit;
                    } else if starts(&["77"]) {
                        l.asset_gains += credit;
                    } else if starts(&["70", "71", "72", "73"]) {
                        l.operating += credit;
                    } else {
                        l.other_income += credit;
                    }
                }
                6 if models::is_profit_tax(a) => {}
                6 => {
                    let e = &mut l.e;
                    if starts(&["60", "61"]) {
                        e.consumption += n;
                    } else if starts(&["640", "641"]) {
                        e.wages += n;
                    } else if starts(&["642"]) {
                        if payroll {
                            e.employer_ss += n;
                        } else {
                            e.owner_ss += n;
                        }
                    } else if starts(&["64"]) {
                        e.other_staff += n;
                    } else if starts(&["621"]) {
                        e.rentals += n;
                    } else if starts(&["622"]) {
                        e.repairs += n;
                    } else if starts(&["623"]) {
                        e.professionals += n;
                    } else if starts(&["628"]) {
                        e.supplies += n;
                    } else if starts(&["624", "625", "626", "627", "629"]) {
                        e.other_services += n;
                    } else if starts(&["63"]) {
                        e.taxes += n;
                    } else if starts(&["66"]) {
                        e.financial += n;
                    } else if starts(&["68"]) {
                        e.depreciation += n;
                    } else if starts(&["69"]) {
                        l.provisions += n;
                    } else {
                        if starts(&["678"]) {
                            l.exceptional += n;
                        }
                        e.other += n;
                    }
                }
                _ => {}
            }
        }
    }
    Ok(l)
}

/// Rendimiento de la actividad del ejercicio `year` para la Renta, con las casillas.
pub fn activity(v: &Vault, year: i32) -> Result<ActivityDraft> {
    let who = models::declarant(v)?;
    if !Nif::parse(&who.nif).is_ok_and(|n| n.is_persona_fisica()) {
        return Err(AppError::Other(
            "la Renta (modelo 100) es de personas físicas (autónomos)".into(),
        ));
    }
    let s = models::settings(v)?;
    let (ys, _) = s.year_settings(year);
    let (regime, regime_note) = models::irpf_regime(v, &s, year)?;
    let mut notes: Vec<String> = regime_note.into_iter().collect();
    let end = Date::from_calendar_date(year, Month::December, 31)
        .map_err(|e| AppError::Other(e.to_string()))?;

    let mut l = classify(v, year)?;
    // La amortización del año que aún no está en el diario (antes del cierre).
    l.e.depreciation += accounting::accrued_depreciation(v, year, end)?;
    if !l.financial_income.is_zero() {
        notes.push(format!(
            "Hay {} de ingresos financieros (cuentas 76): no entran en la actividad; los \
             intereses de cuentas y depósitos van como rendimientos del capital mobiliario.",
            l.financial_income
        ));
    }
    if !l.asset_gains.is_zero() {
        notes.push(format!(
            "Hay {} de beneficios por venta de inmovilizado (cuentas 77): no son rendimiento de \
             la actividad sino ganancia patrimonial (art. 28.2 LIRPF); decláralos en ese apartado.",
            l.asset_gains
        ));
    }
    if !l.exceptional.is_zero() {
        notes.push(format!(
            "Hay {} de gastos excepcionales (cuenta 678) en «otros conceptos»: las multas, \
             sanciones y recargos no son deducibles; quítalos si los hay.",
            l.exceptional
        ));
    }

    // Pagos fraccionados: lo ingresado en cada 130 presentado ([19] positiva) o, si no consta
    // presentado, su pago registrado (sin contar dos veces el mismo trimestre).
    let mut installments = Money::ZERO;
    let mut missing_130 = Vec::new();
    for q in 1..=4u8 {
        let period = format!("{q}T");
        match models::filed_boxes(v, "130", year, &period)? {
            Some((_, boxes)) => {
                installments += boxes
                    .get(&19)
                    .copied()
                    .unwrap_or(Money::ZERO)
                    .max(Money::ZERO);
            }
            // Sin presentación que conste, el pago registrado en TaxPain (Modelos → pagar).
            None => match crate::tax_payments::payment(v, "130", year, q)? {
                Some(p) => installments += p.amount.max(Money::ZERO),
                None => missing_130.push(period),
            },
        }
    }
    if !missing_130.is_empty() {
        notes.push(format!(
            "No constan presentados estos 130 de {year}: {}. Sólo se restan los pagos que \
             constan; si los presentó tu gestor, impórtalos en Importar → Comparar con la \
             gestoría (o mira los datos fiscales de la AEAT).",
            missing_130.join(", ")
        ));
    }
    let withholdings = models::issued_withholdings(v, year, end)?;

    let input = Input100 {
        regime,
        operating_income: l.operating,
        other_income: l.other_income,
        expenses: l.e,
        provisions: l.provisions,
        reduction_32_2: ys.activity_reduction,
        other_income_over_6500: ys.other_income_over_6500,
        activity_start: ys.activity_start,
        withholdings,
        installments,
    };
    let result = m100::compute(year, &input);

    // Cotejo con los datos fiscales descargados de la AEAT.
    let mut check = Vec::new();
    let mut check_date = None;
    if let Some(data) = models::tax_data(v, year)? {
        check_date = data.process_date.clone();
        let close = |a: Money, b: Money| (a - b).abs() < Money::from_euros(1, 0);
        let aeat_income: Money = data.activities.iter().map(|a| a.income).sum();
        if !aeat_income.is_zero() {
            check.push(CheckLine {
                topic: "ingresos".into(),
                label: "Ingresos que tus clientes declararon en su 190 (en TaxPain deben ser al \
                        menos esos)"
                    .into(),
                aeat: aeat_income,
                taxpain: result.income,
                matches: result.income + Money::from_euros(1, 0) > aeat_income,
            });
        }
        let aeat_wh: Money = data.activities.iter().map(|a| a.withholding).sum();
        check.push(CheckLine {
            topic: "retenciones".into(),
            label: "Retenciones de la actividad".into(),
            aeat: aeat_wh,
            taxpain: withholdings,
            matches: close(aeat_wh, withholdings),
        });
        let aeat_pf: Money = data
            .installments
            .iter()
            .filter(|p| p.model == "130")
            .map(|p| p.amount)
            .sum();
        check.push(CheckLine {
            topic: "pago_130".into(),
            label: "Pagos fraccionados del 130".into(),
            aeat: aeat_pf,
            taxpain: installments,
            matches: close(aeat_pf, installments),
        });
        let reta: Money = data
            .contributions
            .iter()
            .filter(|c| matches!(c.regime.as_str(), "0521" | "0540" | "0825" | "0840"))
            .map(|c| c.amount)
            .sum();
        if !reta.is_zero() {
            check.push(CheckLine {
                topic: "cotizaciones".into(),
                label: "Cuotas de autónomo (casilla 0186)".into(),
                aeat: reta,
                taxpain: input.expenses.owner_ss,
                matches: close(reta, input.expenses.owner_ss),
            });
        }
        if check.iter().any(|c| !c.matches) {
            notes.push(
                "Hay diferencias con los datos de la AEAT: en Renta Web, las retenciones y los \
                 pagos fraccionados se toman de los datos fiscales; revisa cuál es el bueno \
                 antes de confirmar."
                    .into(),
            );
        }
    } else {
        notes.push(
            "Descarga tus datos fiscales (abajo, «Cotejo con los datos fiscales») para comparar \
             retenciones, pagos del 130 y cuotas de autónomo con los de la AEAT."
                .into(),
        );
    }
    notes.push(
        "En Renta Web puedes importar el libro registro de ingresos y gastos (Contabilidad → \
         «Libros registro de IVA e IRPF») para que rellene los gastos por conceptos; compara el \
         resultado con este resumen."
            .into(),
    );
    let cripto = crate::cripto::renta(v, year)?;
    if cripto.is_some() {
        notes.push(
            "Tienes operaciones con criptomonedas este año: sus ganancias y pérdidas van en la \
             base del ahorro (apartado «monedas virtuales» de Renta Web), no en la actividad; \
             abajo tienes el cálculo con FIFO."
                .into(),
        );
    }
    Ok(ActivityDraft {
        year,
        regime,
        result,
        missing_130,
        check,
        check_date,
        notes,
        cripto,
    })
}
