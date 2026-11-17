//! Panel financiero: cifras del ejercicio sacadas de la contabilidad y de los borradores.
//!
//! Todo se calcula en local a partir del libro diario (grupos 6 y 7 del PGC, 57 tesorería,
//! 43 clientes, 40/41 proveedores y acreedores) y de los borradores de los modelos del trimestre.
//! Son orientativas: los asientos de regularización, cierre y apertura no cuentan como ingresos
//! ni gastos. Si el ejercicio no tiene ningún otro asiento, el panel lo dice (`sin_contabilidad`)
//! y, en vez de ceros, da lo presentado a la AEAT ese año (las sumas de la evolución).

use std::collections::BTreeMap;

use serde::Serialize;
use taxpain_core::Money;
use taxpain_vault::Vault;
use taxpain_vault::rusqlite::params;
use time::Date;

use crate::error::Result;
use crate::evolucion::{self, CifraPresentada};
use crate::{invoicing, models};

/// Ingresos y gastos de un mes (bases, sin IVA).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct MonthFigures {
    pub month: u8,
    pub income: Money,
    pub expenses: Money,
}

/// Estimación de un modelo del trimestre en curso.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TaxEstimate {
    pub model: String,
    pub quarter: u8,
    /// Periodo como lo pide la AEAT («4T», «3P»).
    pub period: String,
    /// Positivo: a ingresar; negativo: a compensar o devolver.
    pub result: Money,
    /// Si no se puede estimar (`result` no vale): por qué.
    pub note: Option<String>,
}

/// Tareas pendientes que conviene no olvidar.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Pending {
    pub received_to_review: i64,
    pub bank_unreconciled: i64,
    pub notices_pending: i64,
    pub verifactu_unsent: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Dashboard {
    pub year: i32,
    pub months: Vec<MonthFigures>,
    pub income: Money,
    pub expenses: Money,
    /// Resultado antes de impuestos (ingresos − gastos).
    pub profit: Money,
    /// Saldo de tesorería (57) según la contabilidad.
    pub cash: Money,
    /// Pendiente de cobro (43) y de pago (40, 41).
    pub receivables: Money,
    pub payables: Money,
    pub estimates: Vec<TaxEstimate>,
    pub pending: Pending,
    /// TaxPain no tiene la contabilidad del ejercicio: ningún asiento (sin contar apertura,
    /// regularización y cierre). Las cifras de arriba son cero porque no hay datos, no porque no
    /// haya habido actividad.
    pub sin_contabilidad: bool,
    /// Sin contabilidad: lo presentado a la AEAT ese año que se ha leído (vacío si no hay).
    pub presentado: Vec<CifraPresentada>,
}

/// Asientos que no son actividad del ejercicio.
const NOT_ACTIVITY: &str = "('Regularizacion', 'Cierre', 'Apertura')";

fn balance(v: &Vault, year: i32, prefixes: &[&str], skip_closing: bool) -> Result<Money> {
    // Saldo deudor (debe − haber) de las cuentas que empiezan por algún prefijo.
    let like: Vec<String> = prefixes
        .iter()
        .map(|p| format!("l.account LIKE '{p}%'"))
        .collect();
    let closing = if skip_closing {
        " AND e.origin_kind <> 'Cierre'"
    } else {
        ""
    };
    let sql = format!(
        "SELECT COALESCE(SUM(l.debit - l.credit), 0) FROM journal_line l
         JOIN journal_entry e ON e.id = l.entry_id
         WHERE e.fiscal_year = ?1 AND ({}){closing}",
        like.join(" OR ")
    );
    let cents: i64 = v.connection().query_row(&sql, [year], |r| r.get(0))?;
    Ok(Money::from_cents(cents))
}

fn count(v: &Vault, sql: &str) -> Result<i64> {
    Ok(v.connection().query_row(sql, [], |r| r.get(0))?)
}

fn count_year(v: &Vault, sql: &str, year: i32) -> Result<i64> {
    Ok(v.connection().query_row(sql, [year], |r| r.get(0))?)
}

/// Trimestre natural (1–4) del mes.
fn quarter_of(month: u8) -> u8 {
    month.saturating_sub(1) / 3 + 1
}

/// Panel del ejercicio `year`. `current_month` (1–12) indica el trimestre que se estima.
pub fn dashboard(v: &Vault, year: i32, current_month: u8) -> Result<Dashboard> {
    let mut months: Vec<MonthFigures> = (1..=12)
        .map(|m| MonthFigures {
            month: m,
            ..MonthFigures::default()
        })
        .collect();
    {
        let mut stmt = v.connection().prepare(&format!(
            "SELECT CAST(substr(e.date, 6, 2) AS INTEGER) AS m,
                    COALESCE(SUM(CASE WHEN l.account LIKE '7%' THEN l.credit - l.debit ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN l.account LIKE '6%' THEN l.debit - l.credit ELSE 0 END), 0)
             FROM journal_line l JOIN journal_entry e ON e.id = l.entry_id
             WHERE e.fiscal_year = ?1 AND e.origin_kind NOT IN {NOT_ACTIVITY}
             GROUP BY m"
        ))?;
        let rows = stmt.query_map(params![year], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?;
        for row in rows {
            let (m, income, expenses) = row?;
            if let Some(slot) = usize::try_from(m - 1).ok().and_then(|i| months.get_mut(i)) {
                slot.income = Money::from_cents(income);
                slot.expenses = Money::from_cents(expenses);
            }
        }
    }
    let income: Money = months.iter().map(|m| m.income).sum();
    let expenses: Money = months.iter().map(|m| m.expenses).sum();

    // Saldos a la fecha: sin el asiento de cierre (que los deja a cero).
    let cash = balance(v, year, &["57"], true)?;
    let receivables = balance(v, year, &["43"], true)?;
    let payables = -balance(v, year, &["40", "41"], true)?;

    // Estimaciones del trimestre: sólo si hay datos para calcularlas (si falta configuración,
    // el panel se muestra igual sin ellas).
    let quarter = quarter_of(current_month.clamp(1, 12));
    let mut estimates = Vec::new();
    let is_person = invoicing::settings(v)?.is_some_and(|s| s.issuer.person.is_some());
    // Persona física: 303 y 130; sociedad: 303 y el 202 que toca. Las retenciones (111, 115)
    // sólo si hay algo que ingresar.
    let mut wanted: Vec<(&str, u8)> = vec![("303", quarter)];
    if is_person {
        wanted.push(("130", quarter));
    } else {
        // 202: 1P en abril (enero–abril), 2P en octubre (mayo–octubre) y 3P en diciembre.
        let p = match current_month {
            ..=4 => 1,
            5..=10 => 2,
            _ => 3,
        };
        wanted.push(("202", p));
    }
    wanted.push(("111", quarter));
    wanted.push(("115", quarter));
    // El 202 sale por la modalidad del ejercicio (cuota o base, art. 40.2 o 40.3 LIS).
    for (model, q) in wanted {
        if let Ok(d) = models::draft(v, model, year, q) {
            if matches!(model, "111" | "115" | "202") && d.result == Money::ZERO {
                continue;
            }
            estimates.push(TaxEstimate {
                model: model.to_owned(),
                quarter: q,
                period: models::period_code(model, q),
                result: d.result,
                note: None,
            });
        }
    }

    let asientos = count_year(
        v,
        &format!(
            "SELECT count(*) FROM journal_entry
             WHERE fiscal_year = ?1 AND origin_kind NOT IN {NOT_ACTIVITY}"
        ),
        year,
    )?;
    let sin_contabilidad = asientos == 0;
    let presentado = if sin_contabilidad {
        evolucion::presentado_del_año(v, year)?
    } else {
        Vec::new()
    };

    let pending = Pending {
        received_to_review: count(
            v,
            "SELECT count(*) FROM invoice_received WHERE status = 'pendiente'",
        )?,
        // Las compras con tarjeta de débito sólo informan, y lo que ya contabilizó un diario
        // importado no se vuelve a conciliar.
        bank_unreconciled: count(
            v,
            &format!(
                "SELECT count(*) FROM bank_tx t JOIN bank_account b ON b.id = t.bank_account
                 WHERE t.entry_id IS NULL AND b.kind <> 'tarjeta_debito' AND NOT {}",
                crate::bank::BOOKED_ELSEWHERE
            ),
        )?,
        notices_pending: count(
            v,
            "SELECT count(*) FROM aeat_notice WHERE kind = 'N' AND state = 'P'",
        )?,
        // Hoy sólo hay envío al entorno de pruebas: en una empresa real (no la de pruebas) y sin
        // producción no se avisa de registros por enviar que no se pueden enviar
        // (FACTURACION-01).
        // Con producción, lo aceptado sólo en el entorno de pruebas también está por enviar.
        verifactu_unsent: if crate::invoicing::settings(v)?.is_some_and(|s| s.verifactu.production)
        {
            count(
                v,
                "SELECT count(*) FROM verifactu_record WHERE send_status IN ('pendiente', 'pruebas')",
            )?
        } else if crate::verifactu_lab::info(v)?.is_some() {
            count(
                v,
                "SELECT count(*) FROM verifactu_record WHERE send_status = 'pendiente'",
            )?
        } else {
            0
        },
    };

    Ok(Dashboard {
        year,
        months,
        income,
        expenses,
        profit: income - expenses,
        cash,
        receivables,
        payables,
        estimates,
        pending,
        sin_contabilidad,
        presentado,
    })
}

/// Lo pendiente de cobrar o de facturar que el panel lleva a otras secciones.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Receivables {
    /// Facturas emitidas con algún vencimiento pasado sin cobrar, y lo vencido.
    pub overdue_invoices: i64,
    pub overdue_amount: Money,
    /// Presupuestos aceptados y sin facturar (de cualquier año) y su total.
    pub accepted_quotes: i64,
    pub accepted_total: Money,
}

/// [`Receivables`] a fecha `today`.
///
/// Lo vencido es por factura (no por plazo) y sin abonos: una rectificativa en negativo no es
/// una factura vencida sin cobrar, y lo que abona se descuenta de su original (una factura
/// anulada con un abono por el total ya no se debe).
pub fn receivables(v: &Vault, today: Date) -> Result<Receivables> {
    let mut overdue: BTreeMap<i64, Money> = BTreeMap::new();
    let mut credits: BTreeMap<i64, Money> = BTreeMap::new();
    let mut rectifies: BTreeMap<i64, i64> = BTreeMap::new();
    {
        let mut st = v.connection().prepare(
            "SELECT id, rectifies_id FROM invoice_issued
             WHERE rectifies_id IS NOT NULL AND status = 'emitida'",
        )?;
        for row in st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))? {
            let (id, original) = row?;
            rectifies.insert(id, original);
        }
    }
    for d in crate::cartera::list(v, today)? {
        if d.kind != "emitida" || d.status != "pendiente" {
            continue;
        }
        if d.amount.is_negative() {
            // Abono pendiente: descuenta de su original (venza cuando venza).
            if let Some(original) = rectifies.get(&d.invoice_id) {
                *credits.entry(*original).or_default() += d.amount;
            }
        } else if d.overdue_days > 0 {
            *overdue.entry(d.invoice_id).or_default() += d.amount;
        }
    }
    let mut out = Receivables::default();
    for (id, amount) in overdue {
        let net = amount + credits.get(&id).copied().unwrap_or_default();
        if net.cents() > 0 {
            out.overdue_invoices += 1;
            out.overdue_amount += net;
        }
    }
    let (n, total) = crate::quotes::accepted_pending(v)?;
    out.accepted_quotes = n;
    out.accepted_total = total;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::quarter_of;

    #[test]
    fn trimestres() {
        assert_eq!(
            (1..=12).map(quarter_of).collect::<Vec<_>>(),
            [1, 1, 1, 2, 2, 2, 3, 3, 3, 4, 4, 4]
        );
    }
}
