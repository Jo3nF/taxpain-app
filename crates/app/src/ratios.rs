//! Ratios financieros (`docs/research/A3.md` §2.17) a partir del balance y la cuenta de
//! pérdidas y ganancias (modelo PYMES), con el ejercicio anterior para comparar. Los valores van
//! en centésimas (1,53 veces → 153; 15,3 % → 1530; 45 días → 4500) para no usar coma flotante.
//!
//! Los rangos orientativos son los habituales en el análisis de pymes españolas; dependen del
//! sector, así que sólo sirven de señal (`bien`, `atencion`), nunca de juicio.

use serde::Serialize;
use taxpain_core::money::Money;
use taxpain_vault::Vault;

use crate::accounting::{self, AnnualAccounts};
use crate::error::Result;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    /// Euros (el valor va en céntimos).
    Euros,
    Veces,
    Porcentaje,
    Dias,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Ratio {
    pub id: &'static str,
    pub group: &'static str,
    pub name: &'static str,
    pub formula: &'static str,
    pub explanation: &'static str,
    pub unit: Unit,
    /// En centésimas (en euros, céntimos); `None` si no se puede calcular (divisor cero).
    pub value: Option<i64>,
    pub previous: Option<i64>,
    /// Rango orientativo (mismas unidades), si lo hay.
    pub range: Option<(i64, i64)>,
    /// `bien`, `atencion` o `sin_datos`.
    pub status: &'static str,
}

/// `num / den` en centésimas, redondeado (`None` si `den` es cero).
fn div(num: Money, den: Money, scale: i64) -> Option<i64> {
    if den.is_zero() {
        return None;
    }
    let n = i128::from(num.cents()) * i128::from(scale) * 100;
    let d = i128::from(den.cents());
    let q = if (n >= 0) == (d > 0) {
        (n + d.abs() / 2 * d.signum()) / d
    } else {
        (n - d.abs() / 2 * d.signum()) / d
    };
    i64::try_from(q).ok()
}

struct Data {
    b: Money,
    existencias: Money,
    efectivo: Money,
    pc: Money,
    pb: Money,
    pn: Money,
    ta: Money,
    clientes: Money,
    proveedores: Money,
    ventas: Money,
    aprovisionamientos: Money,
    otros_gastos: Money,
    amortizacion: Money,
    explotacion: Money,
    gastos_financieros: Money,
    resultado: Money,
}

/// Saldo acreedor (positivo) de proveedores y acreedores comerciales (400 a 406 y 41) al cierre
/// del ejercicio, antes de regularizar y cerrar. La partida «Proveedores» del balance PYMES
/// (PC.IV.1) sólo recoge 400-406 y la app contabiliza las recibidas en la 410 por defecto, que
/// cae en «Otros acreedores» (PC.IV.2): el periodo medio de pago se calcula con las dos. La 407
/// (anticipos a proveedores) es del activo y no cuenta.
fn trade_creditors(v: &Vault, year: i32) -> Result<Money> {
    let cents: i64 = v.connection().query_row(
        "SELECT COALESCE(SUM(l.credit - l.debit), 0) FROM journal_line l
           JOIN journal_entry e ON e.id = l.entry_id
          WHERE e.fiscal_year = ?1 AND e.origin_kind NOT IN ('Regularizacion', 'Cierre')
            AND (l.account LIKE '40%' OR l.account LIKE '41%') AND l.account NOT LIKE '407%'",
        [year],
        |r| r.get(0),
    )?;
    Ok(Money::from_cents(cents))
}

fn data(ca: &AnnualAccounts, proveedores: Money) -> Data {
    let b = |id: &str| ca.balance.get(id);
    let p = |id: &str| ca.income.get(id);
    Data {
        b: b("B"),
        existencias: b("B.I"),
        efectivo: b("B.VI"),
        pc: b("PC"),
        pb: b("PB"),
        pn: b("PA"),
        ta: b("TA"),
        clientes: b("B.II.1"),
        proveedores,
        ventas: p("1"),
        aprovisionamientos: -p("4"),
        otros_gastos: -p("7"),
        amortizacion: -p("8"),
        explotacion: p("A"),
        gastos_financieros: -p("13"),
        resultado: p("D"),
    }
}

type Calc = fn(&Data) -> Option<i64>;

struct Def {
    id: &'static str,
    group: &'static str,
    name: &'static str,
    formula: &'static str,
    explanation: &'static str,
    unit: Unit,
    range: Option<(i64, i64)>,
    calc: Calc,
}

const DEFS: &[Def] = &[
    Def {
        id: "fondo_maniobra",
        group: "Liquidez",
        name: "Fondo de maniobra",
        formula: "Activo corriente − Pasivo corriente",
        explanation: "Lo que queda del activo a corto plazo tras pagar las deudas a corto. Negativo: las deudas que vencen en el año superan lo que se puede cobrar o vender en él.",
        unit: Unit::Euros,
        range: Some((0, i64::MAX)),
        calc: |d| Some((d.b - d.pc).cents()),
    },
    Def {
        id: "liquidez",
        group: "Liquidez",
        name: "Liquidez general",
        formula: "Activo corriente / Pasivo corriente",
        explanation: "Cuántas veces cubre el activo corriente las deudas a corto plazo.",
        unit: Unit::Veces,
        range: Some((150, 200)),
        calc: |d| div(d.b, d.pc, 1),
    },
    Def {
        id: "acida",
        group: "Liquidez",
        name: "Prueba ácida",
        formula: "(Activo corriente − Existencias) / Pasivo corriente",
        explanation: "Igual que la liquidez, sin contar las existencias, que tardan más en convertirse en dinero.",
        unit: Unit::Veces,
        range: Some((80, 120)),
        calc: |d| div(d.b - d.existencias, d.pc, 1),
    },
    Def {
        id: "tesoreria",
        group: "Liquidez",
        name: "Disponibilidad",
        formula: "Efectivo / Pasivo corriente",
        explanation: "Parte de las deudas a corto que se podría pagar hoy con lo que hay en caja y bancos.",
        unit: Unit::Veces,
        range: Some((10, 30)),
        calc: |d| div(d.efectivo, d.pc, 1),
    },
    Def {
        id: "endeudamiento",
        group: "Solvencia",
        name: "Endeudamiento",
        formula: "(Pasivo no corriente + Pasivo corriente) / Total patrimonio neto y pasivo",
        explanation: "Qué parte de lo que financia la empresa son deudas (el resto son fondos propios).",
        unit: Unit::Porcentaje,
        range: Some((4000, 6000)),
        calc: |d| div(d.pb + d.pc, d.ta, 100),
    },
    Def {
        id: "solvencia",
        group: "Solvencia",
        name: "Solvencia (garantía)",
        formula: "Activo total / (Pasivo no corriente + Pasivo corriente)",
        explanation: "Cuántas veces cubren los bienes y derechos de la empresa todas sus deudas.",
        unit: Unit::Veces,
        range: Some((150, i64::MAX)),
        calc: |d| div(d.ta, d.pb + d.pc, 1),
    },
    Def {
        id: "cobertura_intereses",
        group: "Solvencia",
        name: "Cobertura de intereses",
        formula: "Resultado de explotación / Gastos financieros",
        explanation: "Cuántas veces paga el negocio los intereses de sus deudas.",
        unit: Unit::Veces,
        range: Some((300, i64::MAX)),
        calc: |d| div(d.explotacion, d.gastos_financieros, 1),
    },
    Def {
        id: "roa",
        group: "Rentabilidad",
        name: "Rentabilidad económica (ROA)",
        formula: "Resultado de explotación / Activo total",
        explanation: "Lo que rinden los activos de la empresa, antes de intereses e impuestos.",
        unit: Unit::Porcentaje,
        range: Some((500, i64::MAX)),
        calc: |d| div(d.explotacion, d.ta, 100),
    },
    Def {
        id: "roe",
        group: "Rentabilidad",
        name: "Rentabilidad financiera (ROE)",
        formula: "Resultado del ejercicio / Patrimonio neto",
        explanation: "Lo que rinde el dinero de los socios.",
        unit: Unit::Porcentaje,
        range: Some((800, i64::MAX)),
        calc: |d| div(d.resultado, d.pn, 100),
    },
    Def {
        id: "margen_explotacion",
        group: "Rentabilidad",
        name: "Margen de explotación",
        formula: "Resultado de explotación / Importe neto de la cifra de negocios",
        explanation: "De cada 100 € vendidos, cuántos quedan tras los gastos del negocio.",
        unit: Unit::Porcentaje,
        range: None,
        calc: |d| div(d.explotacion, d.ventas, 100),
    },
    Def {
        id: "margen_neto",
        group: "Rentabilidad",
        name: "Margen neto",
        formula: "Resultado del ejercicio / Importe neto de la cifra de negocios",
        explanation: "De cada 100 € vendidos, cuántos quedan como beneficio final.",
        unit: Unit::Porcentaje,
        range: None,
        calc: |d| div(d.resultado, d.ventas, 100),
    },
    Def {
        id: "ebitda",
        group: "Rentabilidad",
        name: "EBITDA",
        formula: "Resultado de explotación + Amortizaciones",
        explanation: "Lo que genera el negocio antes de amortizaciones, intereses e impuestos.",
        unit: Unit::Euros,
        range: Some((0, i64::MAX)),
        calc: |d| Some((d.explotacion + d.amortizacion).cents()),
    },
    Def {
        id: "rotacion",
        group: "Actividad",
        name: "Rotación del activo",
        formula: "Importe neto de la cifra de negocios / Activo total",
        explanation: "Cuántas veces se vende al año el valor de los activos.",
        unit: Unit::Veces,
        range: None,
        calc: |d| div(d.ventas, d.ta, 1),
    },
    Def {
        id: "pmc",
        group: "Actividad",
        name: "Periodo medio de cobro",
        formula: "Clientes / Importe neto de la cifra de negocios × 365",
        explanation: "Días que tardan de media los clientes en pagar (aproximado: los saldos de clientes llevan IVA y las ventas no).",
        unit: Unit::Dias,
        range: Some((0, 6000)),
        calc: |d| div(d.clientes, d.ventas, 365),
    },
    Def {
        id: "pmp",
        group: "Actividad",
        name: "Periodo medio de pago",
        formula: "Proveedores y acreedores (40 y 41) / (Aprovisionamientos + Otros gastos de explotación) × 365",
        explanation: "Días que tarda de media la empresa en pagar a sus proveedores y acreedores (máximo legal entre empresas: 60).",
        unit: Unit::Dias,
        range: Some((0, 6000)),
        calc: |d| div(d.proveedores, d.aprovisionamientos + d.otros_gastos, 365),
    },
];

fn status(value: Option<i64>, range: Option<(i64, i64)>) -> &'static str {
    match (value, range) {
        (None, _) => "sin_datos",
        (Some(_), None) => "bien",
        (Some(v), Some((lo, hi))) if v >= lo && v <= hi => "bien",
        (Some(_), Some(_)) => "atencion",
    }
}

/// Ratios del ejercicio `year` y del anterior.
pub fn ratios(v: &Vault, year: i32) -> Result<Vec<Ratio>> {
    let now = data(
        &accounting::annual_accounts(v, year)?,
        trade_creditors(v, year)?,
    );
    let before = if accounting::journal(v, year - 1)?.is_empty() {
        None
    } else {
        Some(data(
            &accounting::annual_accounts(v, year - 1)?,
            trade_creditors(v, year - 1)?,
        ))
    };
    Ok(DEFS
        .iter()
        .map(|d| {
            let value = (d.calc)(&now);
            Ratio {
                id: d.id,
                group: d.group,
                name: d.name,
                formula: d.formula,
                explanation: d.explanation,
                unit: d.unit,
                value,
                previous: before.as_ref().and_then(|b| (d.calc)(b)),
                range: d.range,
                status: status(value, d.range),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn division_en_centesimas() {
        let m = |e: i64| Money::from_euros(e, 0);
        assert_eq!(div(m(3), m(2), 1), Some(150));
        assert_eq!(div(m(1), m(3), 100), Some(3333));
        assert_eq!(div(m(2), m(3), 1), Some(67));
        assert_eq!(div(m(-2), m(3), 1), Some(-67));
        assert_eq!(div(m(30), m(365), 365), Some(3000));
        assert_eq!(div(m(1), Money::ZERO, 1), None);
        assert_eq!(status(Some(160), Some((150, 200))), "bien");
        assert_eq!(status(Some(100), Some((150, 200))), "atencion");
        assert_eq!(status(None, None), "sin_datos");
    }
}
