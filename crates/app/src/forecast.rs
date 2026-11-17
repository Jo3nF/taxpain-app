//! Previsión de tesorería: saldo previsto de bancos y caja en las próximas 13 semanas y en los
//! próximos 12 meses.
//!
//! Parte del saldo de hoy de las cuentas 57 (caja 570, bancos 572…) según el diario y le suma lo
//! que se sabe que va a entrar o salir, cada cosa en su fecha:
//! - **Cartera** ([`cartera::list`]): cobros de las facturas emitidas y pagos de las recibidas
//!   pendientes, por vencimiento. Lo ya vencido cuenta hoy (primera semana) y se avisa.
//! - **Impuestos**: los modelos con ingreso (303, 111, 115, 130 o 202, y 200) el último día de su
//!   plazo según el calendario fiscal (`taxpain_core::calendar`), con el importe presentado o el
//!   del borrador ([`tax_payments::amount_due`]). Sólo del periodo en curso y de los ya
//!   terminados que faltan por presentar o pagar; los informativos (349, 390, 347…) no mueven
//!   dinero. Un borrador que no se puede calcular se salta con un aviso.
//! - **Asientos periódicos** ([`templates`]) que tocan la tesorería, en sus próximas fechas.
//! - **Nóminas y Seguridad Social**: repite las del último mes contabilizado (465 y 476).
//! - **Leasing**: cuotas pendientes del cuadro ([`leasing::list`]) con su IVA.
//! - **Préstamos** ([`prestamos::list`]): las cuotas sin registrar desde hoy, netas de la
//!   retención (cobros de los concedidos, pagos de los recibidos), y el alta pendiente con fecha
//!   futura. Lo registrado ya está en el saldo de los bancos y no se cuenta otra vez; los asientos
//!   periódicos que tocan las cuentas de un préstamo tampoco.
//!
//! Nada se inventa: cada categoría sale de datos guardados y, si faltan, no se cuenta. Si una
//! categoría no se puede leer, se avisa y se sigue con las demás: la previsión nunca falla entera
//! por una parte. También calcula el «dinero que no es tuyo»: el IVA y las retenciones cobrados
//! o practicados que aún hay que ingresar a Hacienda.

use std::collections::{BTreeMap, HashSet};

use serde::Serialize;
use taxpain_core::calendar::{self, Period, TaxpayerProfile};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::rusqlite::OptionalExtension;
use time::{Date, Duration, Month};

use crate::error::Result;
use crate::{
    accounting, cartera, invoicing, leasing, models, presentados, prestamos, tax_payments,
    templates,
};

/// Semanas del horizonte corto y meses del largo.
pub const WEEKS: usize = 13;
pub const MONTHS: usize = 12;
/// Plazos ya vencidos que aún se miran, por si quedó un modelo sin presentar.
const TAX_LOOKBACK_DAYS: i64 = 60;
/// Las nóminas se repiten si el último mes con nóminas es de hace dos meses como mucho.
const PAYROLL_MAX_GAP_MONTHS: i32 = 2;

/// De dónde sale cada entrada o salida.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Cobros de clientes (facturas emitidas pendientes).
    Collections,
    /// Pagos a proveedores y acreedores (facturas recibidas pendientes).
    Payments,
    /// Modelos a ingresar.
    Taxes,
    /// Líquido de las nóminas.
    Payroll,
    /// Seguros sociales (cuota del trabajador y de la empresa).
    SocialSecurity,
    /// Cuotas de leasing (con IVA).
    Leasing,
    /// Cuotas de los préstamos concedidos (cobros) y recibidos (pagos), y su alta.
    Loans,
    /// Asientos periódicos que tocan la tesorería.
    Recurring,
}

/// Un cobro o pago previsto.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ForecastItem {
    /// Fecha en la previsión: el vencimiento o, si ya pasó, hoy.
    pub date: Date,
    /// Vencimiento o plazo original.
    pub due_date: Date,
    pub concept: String,
    /// Positivo: entra; negativo: sale.
    pub amount: Money,
    pub category: Category,
    /// Dato del que sale: «cartera», «borrador del modelo», «asiento periódico»…
    pub source: String,
    /// Vencido y sin cobrar o pagar: se cuenta hoy.
    pub overdue: bool,
    /// Importe estimado (borrador de un periodo, nóminas repetidas…), no definitivo.
    pub estimated: bool,
}

/// Un periodo (semana o mes) de la previsión.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Bucket {
    pub start: Date,
    pub end: Date,
    pub opening: Money,
    /// Entradas por categoría (positivas).
    pub inflows: BTreeMap<Category, Money>,
    /// Salidas por categoría (en positivo).
    pub outflows: BTreeMap<Category, Money>,
    pub total_in: Money,
    pub total_out: Money,
    pub closing: Money,
}

/// Saldo al final de un día con movimientos.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct BalancePoint {
    pub date: Date,
    pub balance: Money,
}

/// Previsión en un horizonte (13 semanas o 12 meses).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Horizon {
    pub buckets: Vec<Bucket>,
    /// Último día del horizonte.
    pub end: Date,
    /// Saldo día a día: el de hoy antes de nada y el del final de cada día con movimientos.
    pub points: Vec<BalancePoint>,
    /// Saldo más bajo al final de un día (desde el final de hoy, y dentro de los periodos, no sólo
    /// a su cierre) y su fecha.
    pub lowest: Money,
    pub lowest_date: Date,
    /// Primer día con saldo negativo, si lo hay.
    pub first_negative: Option<Date>,
}

/// Dinero de Hacienda que está en el banco: el IVA repercutido menos el soportado deducible
/// (477 − 472) más el IVA liquidado sin pagar (4750), y las retenciones practicadas sin ingresar
/// (4751), a hoy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct NotYours {
    /// Negativo: el IVA soportado supera al repercutido (saldo a compensar, no es deuda).
    pub vat: Money,
    pub withholdings: Money,
    /// IVA (si es a ingresar) más retenciones.
    pub total: Money,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WarningKind {
    /// El saldo previsto se queda en negativo.
    Negative,
    /// Cobros, pagos o modelos vencidos.
    Overdue,
    /// Un modelo que no se ha podido estimar.
    Tax,
    /// Datos que no se han podido leer.
    Data,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Warning {
    pub kind: WarningKind,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Forecast {
    pub today: Date,
    /// Saldo de tesorería (57) hoy según la contabilidad.
    pub opening: Money,
    pub weekly: Horizon,
    pub monthly: Horizon,
    pub not_yours: NotYours,
    /// Cobros y pagos previstos hasta el final del horizonte largo, por fecha.
    pub items: Vec<ForecastItem>,
    pub warnings: Vec<Warning>,
    /// Cómo se ha calculado y qué no se cuenta.
    pub notes: Vec<String>,
}

// ---------------- Fechas ----------------

const MONTH_NAMES: [&str; 12] = [
    "enero",
    "febrero",
    "marzo",
    "abril",
    "mayo",
    "junio",
    "julio",
    "agosto",
    "septiembre",
    "octubre",
    "noviembre",
    "diciembre",
];

fn month_name(m: Month) -> &'static str {
    MONTH_NAMES[usize::from(u8::from(m)) - 1]
}

/// «9 de noviembre».
fn long_date(d: Date) -> String {
    format!("{} de {}", d.day(), month_name(d.month()))
}

/// Meses desde el año 0 (para sumar y restar meses sin líos).
fn month_index(d: Date) -> i32 {
    d.year() * 12 + i32::from(u8::from(d.month())) - 1
}

fn month_of_index(i: i32) -> (i32, Month) {
    let m = u8::try_from(i.rem_euclid(12) + 1).unwrap_or(1);
    (
        i.div_euclid(12),
        Month::try_from(m).unwrap_or(Month::January),
    )
}

fn first_of_month(i: i32) -> Date {
    let (y, m) = month_of_index(i);
    Date::from_calendar_date(y, m, 1).unwrap_or(Date::MIN)
}

fn last_of_month(i: i32) -> Date {
    let (y, m) = month_of_index(i);
    Date::from_calendar_date(y, m, m.length(y)).unwrap_or(Date::MIN)
}

/// «octubre de 2026».
fn month_label(i: i32) -> String {
    let (y, m) = month_of_index(i);
    format!("{} de {y}", month_name(m))
}

/// Domingo de la semana (de lunes a domingo) de `d`.
fn end_of_week(d: Date) -> Date {
    d + Duration::days(i64::from(6 - d.weekday().number_days_from_monday()))
}

/// Periodos semanales: de hoy al domingo y luego semanas enteras de lunes a domingo.
fn week_ranges(today: Date) -> Vec<(Date, Date)> {
    let mut out = Vec::with_capacity(WEEKS);
    let mut start = today;
    for _ in 0..WEEKS {
        let end = end_of_week(start);
        out.push((start, end));
        start = end + Duration::days(1);
    }
    out
}

/// Periodos mensuales: de hoy a fin de mes y luego meses enteros.
fn month_ranges(today: Date) -> Vec<(Date, Date)> {
    let first = month_index(today);
    (0..MONTHS)
        .map(|k| {
            let i = first + i32::try_from(k).unwrap_or(0);
            let start = if k == 0 { today } else { first_of_month(i) };
            (start, last_of_month(i))
        })
        .collect()
}

// ---------------- Saldos ----------------

/// Saldo deudor (debe − haber) a `today` de las cuentas que empiezan por algún prefijo.
///
/// Acumula el diario hasta hoy sin los asientos de cierre ni los de apertura (que se anulan entre
/// sí de un ejercicio al siguiente), salvo la apertura del primer ejercicio con asientos, que trae
/// los saldos de antes de TaxPain. Así sale bien aunque el ejercicio anterior siga sin cerrar
/// (el panel suma sólo el ejercicio en curso; con los ejercicios encadenados, coinciden).
fn balance_at(v: &Vault, today: Date, prefixes: &[&str]) -> Result<Money> {
    // Los prefijos son constantes de este módulo (sólo dígitos).
    let like: Vec<String> = prefixes
        .iter()
        .map(|p| format!("l.account LIKE '{p}%'"))
        .collect();
    let sql = format!(
        "SELECT COALESCE(SUM(l.debit - l.credit), 0) FROM journal_line l
         JOIN journal_entry e ON e.id = l.entry_id
         WHERE e.date <= ?1 AND e.origin_kind <> 'Cierre'
           AND (e.origin_kind <> 'Apertura'
                OR e.fiscal_year = (SELECT MIN(fiscal_year) FROM journal_entry))
           AND ({})",
        like.join(" OR ")
    );
    let cents: i64 = v
        .connection()
        .query_row(&sql, [today.to_string()], |r| r.get(0))?;
    Ok(Money::from_cents(cents))
}

fn not_yours(v: &Vault, today: Date) -> Result<NotYours> {
    // Saldos acreedores en positivo.
    let vat = -balance_at(v, today, &["477", "472", "4750"])?;
    let withholdings = -balance_at(v, today, &["4751"])?;
    Ok(NotYours {
        vat,
        withholdings,
        total: vat.max(Money::ZERO) + withholdings.max(Money::ZERO),
    })
}

// ---------------- Categorías ----------------

/// Lo que se va reuniendo: movimientos, avisos y notas.
struct Acc {
    today: Date,
    /// Último día del horizonte largo: lo posterior no se cuenta.
    end: Date,
    items: Vec<ForecastItem>,
    warnings: Vec<Warning>,
    notes: Vec<String>,
}

impl Acc {
    fn warn(&mut self, kind: WarningKind, text: String) {
        self.warnings.push(Warning { kind, text });
    }

    /// Añade un movimiento con vencimiento `due` (si ya pasó, cuenta hoy y va como vencido).
    fn push(
        &mut self,
        due: Date,
        concept: String,
        amount: Money,
        category: Category,
        source: &str,
        estimated: bool,
    ) {
        if due > self.end || amount.is_zero() {
            return;
        }
        let overdue = due < self.today;
        self.items.push(ForecastItem {
            date: due.max(self.today),
            due_date: due,
            concept,
            amount,
            category,
            source: source.to_owned(),
            overdue,
            estimated,
        });
    }
}

/// Cobros y pagos pendientes de la cartera. Usa la lista de la cartera tal cual (así hereda sus
/// reglas: facturas anuladas, sin revisar o importadas de los libros de la gestoría, remesas…).
fn cartera_items(v: &Vault, acc: &mut Acc) -> Result<()> {
    let mut overdue: [(usize, Money); 2] = [(0, Money::ZERO); 2];
    for d in cartera::list(v, acc.today)?
        .into_iter()
        .filter(|d| d.status == "pendiente")
    {
        let incoming = d.kind == "emitida";
        let number = if d.installments > 1 {
            format!("{} ({}/{})", d.number, d.seq, d.installments)
        } else {
            d.number.clone()
        };
        let (concept, amount, category) = if incoming {
            (
                format!("Cobro {number} · {}", d.party),
                d.amount,
                Category::Collections,
            )
        } else {
            (
                format!("Pago {number} · {}", d.party),
                -d.amount,
                Category::Payments,
            )
        };
        if d.due_date < acc.today {
            let slot = &mut overdue[usize::from(!incoming)];
            slot.0 += 1;
            slot.1 += d.amount;
        }
        acc.push(d.due_date, concept, amount, category, "cartera", false);
    }
    for (i, (n, total)) in overdue.into_iter().enumerate() {
        if n == 0 {
            continue;
        }
        let text = if i == 0 {
            format!("Cobros vencidos: {n} por {total}. Se cuentan como si se cobraran esta semana.")
        } else {
            format!("Pagos vencidos: {n} por {total}. Se cuentan como si se pagaran esta semana.")
        };
        acc.warn(WarningKind::Overdue, text);
    }
    Ok(())
}

/// Perfil fiscal guardado o, si no hay, el general (SL o autónomo con IVA trimestral).
fn profile(v: &Vault, acc: &mut Acc) -> Result<TaxpayerProfile> {
    if let Ok(Some(p)) = v.setting::<TaxpayerProfile>("taxpayer_profile") {
        return Ok(p);
    }
    let person = invoicing::settings(v)?.is_some_and(|s| s.issuer.person.is_some());
    acc.notes.push(format!(
        "Sin perfil fiscal: los plazos son los de {} con IVA trimestral. Configúralo en el \
         calendario fiscal.",
        if person {
            "un autónomo en estimación directa"
        } else {
            "una sociedad limitada"
        }
    ));
    Ok(if person {
        TaxpayerProfile::autonomo()
    } else {
        TaxpayerProfile::sociedad_limitada()
    })
}

/// Modelos presentados (en producción desde TaxPain o por la gestoría): (modelo, año, periodo).
fn filed_set(v: &Vault) -> HashSet<(String, i32, String)> {
    let mut out = HashSet::new();
    if let Ok(list) = models::filed_returns(v, None) {
        out.extend(
            list.into_iter()
                .filter(|f| f.environment == "produccion")
                .map(|f| (f.model, f.year, f.period)),
        );
    }
    if let Ok(list) = presentados::list(v, None) {
        out.extend(list.into_iter().map(|f| (f.model, f.year, f.period)));
    }
    out
}

/// Modelos a ingresar con plazo en el horizonte.
fn tax_items(v: &Vault, acc: &mut Acc) -> Result<()> {
    let profile = profile(v, acc)?;
    let from = acc.today - Duration::days(TAX_LOOKBACK_DAYS);
    let mut deadlines = Vec::new();
    for y in from.year()..=acc.end.year() {
        let (h, _) = calendar::calendar_for(y, profile.ccaa, &profile.festivos_locales);
        deadlines.extend(
            calendar::deadlines_in_year(&profile, y, &h)
                .into_iter()
                .filter(|d| d.due_on >= from && d.due_on <= acc.end)
                .filter(|d| tax_payments::payable_account(d.model.code()).is_some()),
        );
    }
    let filed = filed_set(v);
    let mut monthly = false;
    let mut missing_is_base = false;
    for d in deadlines {
        let model = d.model.code();
        let fy = d.fiscal_year;
        let (quarter, started, ended) = match d.period {
            Period::Quarter(q) => {
                let first = fy * 12 + i32::from(q.saturating_sub(1)) * 3;
                (
                    q,
                    first_of_month(first) <= acc.today,
                    last_of_month(first + 2) < acc.today,
                )
            }
            // El 202 se calcula con la cuota del último Impuesto sobre Sociedades, no con lo
            // que se va devengando: se puede contar aunque el periodo no haya empezado.
            Period::Installment(n) => (n, true, true),
            Period::Annual => match accounting::fiscal_year(v, fy) {
                Ok(f) => (0, f.start <= acc.today, f.end < acc.today),
                Err(e) => {
                    acc.warn(
                        WarningKind::Tax,
                        format!("No se ha podido leer el ejercicio {fy} para el {model}: {e}"),
                    );
                    continue;
                }
            },
            Period::Month(_) => {
                monthly = true;
                continue;
            }
        };
        if !started {
            continue;
        }
        let period = models::period_code(model, quarter);
        let label = format!("{model} {period} {fy}");
        match tax_payments::payment(v, model, fy, quarter) {
            Ok(Some(_)) => continue,
            Ok(None) => {}
            Err(e) => {
                acc.warn(
                    WarningKind::Tax,
                    format!("No se ha podido comprobar el pago del {label}: {e}"),
                );
                continue;
            }
        }
        let is_filed = filed.contains(&(model.to_owned(), fy, period.clone()));
        // Plazo vencido y presentado: se da por pagado (domiciliado o con NRC); el cargo ya
        // estará o estará pronto en el banco.
        if d.due_on < acc.today && is_filed {
            continue;
        }
        // Por la cuota (art. 40.2 LIS) hace falta la del último IS; por la base (art. 40.3), no.
        if model == "202"
            && !is_filed
            && models::pago202::modalidad_del_pago(v, fy, quarter)
                .is_ok_and(|m| m.efectiva == taxpain_models::m202::Modalidad::Cuota)
            && models::corporate_tax_base(v, fy, quarter)
                .map(|(b, _)| b.is_none())
                .unwrap_or(true)
        {
            missing_is_base = true;
            continue;
        }
        let (amount, from_filing) = match tax_payments::amount_due(v, model, fy, quarter) {
            Ok(x) => x,
            Err(e) => {
                acc.warn(
                    WarningKind::Tax,
                    format!("No se ha podido estimar el {label}: {e}"),
                );
                continue;
            }
        };
        // A compensar o a devolver: no sale dinero (y la devolución no tiene fecha segura).
        if amount.cents() <= 0 {
            continue;
        }
        if d.due_on < acc.today {
            acc.warn(
                WarningKind::Overdue,
                format!(
                    "El modelo {label} venció el {} y no consta presentado: se cuentan sus {amount} \
                     esta semana. Si ya lo presentó la gestoría, importa su modelo presentado y dejará de \
                     contar.",
                    calendar::reminders::format_es(d.due_on)
                ),
            );
        }
        let concept = format!(
            "Modelo {label}{}",
            if ended { "" } else { " (periodo en curso)" }
        );
        let source = if from_filing {
            "modelo presentado"
        } else {
            "borrador del modelo"
        };
        acc.push(
            d.due_on,
            concept,
            -amount,
            Category::Taxes,
            source,
            !from_filing,
        );
    }
    acc.notes.push(
        "Impuestos: el periodo en curso (con lo contabilizado hasta hoy) y los terminados sin \
         presentar o pagar; los periodos que aún no han empezado no se incluyen."
            .into(),
    );
    if monthly {
        acc.notes
            .push("Los modelos mensuales (IVA o retenciones) aún no se estiman.".into());
    }
    if missing_is_base {
        acc.notes.push(
            "202: falta la cuota del último Impuesto sobre Sociedades en «Datos de los modelos»; \
             no se incluye."
                .into(),
        );
    }
    Ok(())
}

/// Último mes con nóminas en el diario (abonos a 465 o 476) y sus importes: (mes, líquido, SS).
fn last_payroll_month(v: &Vault, today: Date) -> Result<Option<(i32, Money, Money)>> {
    let row: Option<(String, i64, i64)> = v
        .connection()
        .query_row(
            "SELECT substr(e.date, 1, 7) AS ym,
                    COALESCE(SUM(CASE WHEN l.account LIKE '465%' THEN l.credit ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN l.account LIKE '476%' THEN l.credit ELSE 0 END), 0)
             FROM journal_line l JOIN journal_entry e ON e.id = l.entry_id
             WHERE e.date <= ?1 AND e.origin_kind NOT IN ('Apertura', 'Cierre')
               AND l.credit > 0 AND (l.account LIKE '465%' OR l.account LIKE '476%')
             GROUP BY ym ORDER BY ym DESC LIMIT 1",
            [today.to_string()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((ym, net, ss)) = row else {
        return Ok(None);
    };
    let parsed = ym
        .split_once('-')
        .and_then(|(y, m)| Some((y.parse::<i32>().ok()?, m.parse::<i32>().ok()?)));
    Ok(parsed.map(|(y, m)| {
        (
            y * 12 + m - 1,
            Money::from_cents(net),
            Money::from_cents(ss),
        )
    }))
}

/// Nóminas y seguros sociales: lo contabilizado sin pagar y, para los meses siguientes, lo mismo
/// que el último mes. El líquido se paga el último día del mes y los seguros sociales de un mes,
/// el último día del mes siguiente (domiciliación de la TGSS). Devuelve si hay nóminas.
fn payroll_items(v: &Vault, acc: &mut Acc) -> Result<bool> {
    let Some((last, net, ss)) = last_payroll_month(v, acc.today)? else {
        return Ok(false);
    };
    let now = month_index(acc.today);
    if now - last > PAYROLL_MAX_GAP_MONTHS {
        acc.notes.push(format!(
            "Las últimas nóminas contabilizadas son de {}: no se repiten.",
            month_label(last)
        ));
        return Ok(false);
    }
    let pending_net = -balance_at(v, acc.today, &["465"])?;
    if pending_net.cents() > 0 {
        acc.push(
            acc.today,
            "Nóminas contabilizadas pendientes de pago (465)".into(),
            -pending_net,
            Category::Payroll,
            "contabilidad",
            false,
        );
    }
    let pending_ss = -balance_at(v, acc.today, &["476"])?;
    if pending_ss.cents() > 0 {
        acc.push(
            last_of_month(last + 1).max(acc.today),
            "Seguros sociales contabilizados pendientes (476)".into(),
            -pending_ss,
            Category::SocialSecurity,
            "contabilidad",
            false,
        );
    }
    let source = format!("nóminas de {}", month_label(last));
    let mut month = last + 1;
    while last_of_month(month) <= acc.end {
        let label = month_label(month);
        if last_of_month(month) >= acc.today {
            acc.push(
                last_of_month(month),
                format!("Nóminas de {label} (estimadas)"),
                -net,
                Category::Payroll,
                &source,
                true,
            );
        }
        if last_of_month(month + 1) >= acc.today {
            acc.push(
                last_of_month(month + 1),
                format!("Seguros sociales de {label} (estimados)"),
                -ss,
                Category::SocialSecurity,
                &source,
                true,
            );
        }
        month += 1;
    }
    acc.notes.push(format!(
        "Nóminas: se repiten las de {} ({net} de líquido y {ss} de seguros sociales al mes).",
        month_label(last)
    ));
    Ok(net.cents() > 0 || ss.cents() > 0)
}

/// Cuotas de leasing sin contabilizar. Devuelve las cuentas de la deuda de cada contrato.
fn leasing_items(v: &Vault, acc: &mut Acc) -> Result<Vec<String>> {
    let mut debts = Vec::new();
    let mut past = 0usize;
    for view in leasing::list(v)? {
        let l = &view.leasing;
        debts.push(l.debt_account.as_str().to_owned());
        for q in view.installments.iter().filter(|q| !q.posted) {
            // Una cuota pasada sin contabilizar seguramente ya se cargó en el banco.
            if q.q.date < acc.today {
                past += 1;
                continue;
            }
            acc.push(
                q.q.date,
                format!("Leasing {}: cuota {}/{}", l.name, q.q.n, l.months),
                -(q.q.amount + q.q.vat),
                Category::Leasing,
                "leasing",
                false,
            );
        }
    }
    if past > 0 {
        acc.notes.push(format!(
            "{past} cuotas de leasing con fecha pasada sin contabilizar: no se incluyen."
        ));
    }
    Ok(debts)
}

/// Cuotas de los préstamos sin registrar (y el alta pendiente) desde hoy. Devuelve las cuentas
/// de capital de cada préstamo.
fn loan_items(v: &Vault, acc: &mut Acc) -> Result<Vec<String>> {
    use taxpain_core::accounting::prestamo::Sentido;
    let mut accounts = Vec::new();
    let mut past = 0usize;
    for view in prestamos::list(v, acc.today)? {
        let p = &view.prestamo;
        accounts.push(p.cuentas.largo.as_str().to_owned());
        accounts.push(p.cuentas.corto.as_str().to_owned());
        let cobro = p.sentido == Sentido::Concedido;
        if view.alta == prestamos::EstadoAlta::Pendiente {
            if p.inicio < acc.today {
                past += 1;
            } else {
                acc.push(
                    p.inicio,
                    format!(
                        "Préstamo {}: {}",
                        p.nombre,
                        if cobro { "entrega" } else { "recepción" }
                    ),
                    if cobro { -p.principal } else { p.principal },
                    Category::Loans,
                    "préstamo",
                    false,
                );
            }
        }
        let n = view.cuotas.len();
        for q in view
            .cuotas
            .iter()
            .filter(|q| q.estado == prestamos::EstadoCuota::Pendiente)
        {
            // Una cuota pasada sin registrar seguramente ya pasó por el banco.
            if q.q.fecha < acc.today {
                past += 1;
                continue;
            }
            acc.push(
                q.q.fecha,
                format!("Préstamo {}: cuota {}/{n}", p.nombre, q.q.n),
                if cobro { q.neto } else { -q.neto },
                Category::Loans,
                "préstamo",
                false,
            );
        }
    }
    if past > 0 {
        acc.notes.push(format!(
            "{past} cuotas o altas de préstamos con fecha pasada sin registrar: no se incluyen. \
             Si siguen pendientes, regístralas en Contabilidad → Préstamos."
        ));
    }
    Ok(accounts)
}

/// La plantilla mueve dinero que ya se cuenta en otra categoría: cobros y pagos de facturas
/// (40, 41, 43: cartera), pagos de modelos (cargo en 4750, 4751, 4752 o 473), nóminas (465,
/// 476) si se repiten, la deuda de un leasing y el capital de un préstamo.
fn counted_elsewhere(t: &templates::Template, payroll: bool, leasing_debts: &[String]) -> bool {
    t.lines.iter().any(|l| {
        let a = l.account.trim_end_matches('?');
        ["40", "41", "43"].iter().any(|p| a.starts_with(p))
            || (l.debit
                && ["4750", "4751", "4752", "473"]
                    .iter()
                    .any(|p| a.starts_with(p)))
            || (payroll && (a.starts_with("465") || a.starts_with("476")))
            || leasing_debts.iter().any(|d| d == a)
    })
}

/// Asientos periódicos que tocan la tesorería (57), en sus fechas desde hoy.
fn template_items(v: &Vault, acc: &mut Acc, payroll: bool, leasing_debts: &[String]) -> Result<()> {
    let mut past = 0usize;
    for t in templates::list(v)? {
        let (Some(every), Some(mut next)) = (t.every_months, t.next_date) else {
            continue;
        };
        if every == 0 || counted_elsewhere(&t, payroll, leasing_debts) {
            continue;
        }
        let treasury: Vec<&templates::TemplateLine> = t
            .lines
            .iter()
            .filter(|l| l.account.starts_with("57"))
            .collect();
        if treasury.is_empty() {
            continue;
        }
        if treasury.iter().any(|l| l.amount.is_none()) {
            acc.notes.push(format!(
                "El asiento periódico «{}» pide el importe al usarlo: no se incluye.",
                t.name
            ));
            continue;
        }
        let net: Money = treasury
            .iter()
            .map(|l| {
                let a = l.amount.unwrap_or_default();
                if l.debit { a } else { -a }
            })
            .sum();
        while next <= acc.end {
            // Fechas pasadas sin registrar: puede que ya se pagaran por el banco; no se cuentan.
            if next < acc.today {
                past += 1;
            } else {
                acc.push(
                    next,
                    t.name.clone(),
                    net,
                    Category::Recurring,
                    "asiento periódico",
                    false,
                );
            }
            let after = templates::add_months(next, every);
            if after <= next {
                break;
            }
            next = after;
        }
    }
    if past > 0 {
        acc.notes.push(format!(
            "{past} asientos periódicos con fecha pasada sin registrar: no se incluyen. Si siguen \
             pendientes de pago, regístralos."
        ));
    }
    Ok(())
}

// ---------------- Periodos ----------------

fn bucket(start: Date, end: Date, opening: Money, items: &[ForecastItem]) -> Bucket {
    let mut inflows = BTreeMap::new();
    let mut outflows = BTreeMap::new();
    for i in items.iter().filter(|i| i.date >= start && i.date <= end) {
        if i.amount.is_negative() {
            *outflows.entry(i.category).or_insert(Money::ZERO) += -i.amount;
        } else {
            *inflows.entry(i.category).or_insert(Money::ZERO) += i.amount;
        }
    }
    let total_in: Money = inflows.values().copied().sum();
    let total_out: Money = outflows.values().copied().sum();
    Bucket {
        start,
        end,
        opening,
        inflows,
        outflows,
        total_in,
        total_out,
        closing: opening + total_in - total_out,
    }
}

/// `items` ordenados por fecha.
fn horizon(
    today: Date,
    opening: Money,
    ranges: &[(Date, Date)],
    items: &[ForecastItem],
) -> Horizon {
    let mut buckets = Vec::with_capacity(ranges.len());
    let mut running = opening;
    for (start, end) in ranges {
        let b = bucket(*start, *end, running, items);
        running = b.closing;
        buckets.push(b);
    }
    let end = ranges.last().map_or(today, |r| r.1);
    let mut points = vec![BalancePoint {
        date: today,
        balance: opening,
    }];
    let mut balance = opening;
    for i in items.iter().filter(|i| i.date <= end) {
        balance += i.amount;
        let n = points.len();
        match points.last_mut() {
            // Varios movimientos el mismo día: cuenta el saldo al final del día. El primer punto
            // es el saldo de hoy antes de nada, así que hoy tiene dos.
            Some(p) if p.date == i.date && n > 1 => p.balance = balance,
            _ => points.push(BalancePoint {
                date: i.date,
                balance,
            }),
        }
    }
    // Desde el saldo al final de hoy (con lo vencido, que cuenta hoy): el de partida sólo si
    // hoy no hay movimientos.
    let end_of_today = points
        .iter()
        .skip(1)
        .find(|p| p.date == today)
        .map_or(opening, |p| p.balance);
    let (mut lowest, mut lowest_date) = (end_of_today, today);
    let mut first_negative = end_of_today.is_negative().then_some(today);
    for p in points.iter().skip(1).filter(|p| p.date > today) {
        if p.balance < lowest {
            lowest = p.balance;
            lowest_date = p.date;
        }
        if first_negative.is_none() && p.balance.is_negative() {
            first_negative = Some(p.date);
        }
    }
    Horizon {
        buckets,
        end,
        points,
        lowest,
        lowest_date,
        first_negative,
    }
}

/// Previsión de tesorería a partir de `today`.
pub fn forecast(v: &Vault, today: Date) -> Result<Forecast> {
    let weeks = week_ranges(today);
    let months = month_ranges(today);
    let end = months.last().map_or(today, |r| r.1);
    let mut acc = Acc {
        today,
        end,
        items: Vec::new(),
        warnings: Vec::new(),
        notes: Vec::new(),
    };
    let opening = balance_at(v, today, &["57"])?;
    // Las cuentas en divisa cuentan por su contravalor contable: se dice cuánto son en divisa.
    if let Ok(notes) = crate::divisa::notas_tesoreria(v, today) {
        acc.notes.extend(notes);
    }

    // Cada categoría es opcional: si no se puede leer, se avisa y se sigue.
    if let Err(e) = cartera_items(v, &mut acc) {
        acc.warn(
            WarningKind::Data,
            format!("No se han podido leer los cobros y pagos pendientes: {e}"),
        );
    }
    if let Err(e) = tax_items(v, &mut acc) {
        acc.warn(
            WarningKind::Data,
            format!("No se han podido calcular los impuestos: {e}"),
        );
    }
    let payroll = payroll_items(v, &mut acc).unwrap_or_else(|e| {
        acc.warn(
            WarningKind::Data,
            format!("No se han podido leer las nóminas: {e}"),
        );
        false
    });
    let mut debts = leasing_items(v, &mut acc).unwrap_or_else(|e| {
        acc.warn(
            WarningKind::Data,
            format!("No se han podido leer los leasing: {e}"),
        );
        Vec::new()
    });
    match loan_items(v, &mut acc) {
        Ok(mut a) => debts.append(&mut a),
        Err(e) => acc.warn(
            WarningKind::Data,
            format!("No se han podido leer los préstamos: {e}"),
        ),
    }
    if let Err(e) = template_items(v, &mut acc, payroll, &debts) {
        acc.warn(
            WarningKind::Data,
            format!("No se han podido leer los asientos periódicos: {e}"),
        );
    }
    let not_yours = not_yours(v, today).unwrap_or_else(|e| {
        acc.warn(
            WarningKind::Data,
            format!("No se ha podido calcular el IVA y las retenciones pendientes: {e}"),
        );
        NotYours {
            vat: Money::ZERO,
            withholdings: Money::ZERO,
            total: Money::ZERO,
        }
    });

    let mut items = std::mem::take(&mut acc.items);
    items.sort_by(|a, b| {
        (a.date, a.category, &a.concept, a.amount).cmp(&(b.date, b.category, &b.concept, b.amount))
    });
    let weekly = horizon(today, opening, &weeks, &items);
    let monthly = horizon(today, opening, &months, &items);

    // El aviso de saldo negativo va el primero.
    let mut warnings = Vec::new();
    if let Some(day) = monthly.first_negative {
        let text = match weekly
            .buckets
            .iter()
            .find(|b| b.start <= day && day <= b.end)
        {
            Some(b) => format!(
                "Saldo negativo desde la semana del {}; mínimo de {} el {}.",
                long_date(b.start),
                weekly.lowest,
                calendar::reminders::format_es(weekly.lowest_date)
            ),
            None => format!(
                "Saldo negativo desde {}; mínimo de {} el {}.",
                month_label(month_index(day)),
                monthly.lowest,
                calendar::reminders::format_es(monthly.lowest_date)
            ),
        };
        warnings.push(Warning {
            kind: WarningKind::Negative,
            text,
        });
    }
    warnings.append(&mut acc.warnings);

    Ok(Forecast {
        today,
        opening,
        weekly,
        monthly,
        not_yours,
        items,
        warnings,
        notes: acc.notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn semanas_de_lunes_a_domingo() {
        // Sábado 24/10/2026: la primera semana es el fin de semana.
        let w = week_ranges(date!(2026 - 10 - 24));
        assert_eq!(w.len(), WEEKS);
        assert_eq!(w[0], (date!(2026 - 10 - 24), date!(2026 - 10 - 25)));
        assert_eq!(w[1], (date!(2026 - 10 - 26), date!(2026 - 11 - 01)));
        assert_eq!(w[12].1, date!(2027 - 01 - 17));
        // Domingo: la primera semana es un solo día.
        assert_eq!(
            week_ranges(date!(2026 - 10 - 25))[0],
            (date!(2026 - 10 - 25), date!(2026 - 10 - 25))
        );
    }

    #[test]
    fn meses_hasta_un_ano() {
        let m = month_ranges(date!(2026 - 10 - 24));
        assert_eq!(m.len(), MONTHS);
        assert_eq!(m[0], (date!(2026 - 10 - 24), date!(2026 - 10 - 31)));
        assert_eq!(m[4], (date!(2027 - 02 - 01), date!(2027 - 02 - 28)));
        assert_eq!(m[11], (date!(2027 - 09 - 01), date!(2027 - 09 - 30)));
        assert_eq!(long_date(date!(2026 - 11 - 09)), "9 de noviembre");
        assert_eq!(
            month_label(month_index(date!(2027 - 03 - 15))),
            "marzo de 2027"
        );
    }

    fn item(date: Date, euros: i64) -> ForecastItem {
        ForecastItem {
            date,
            due_date: date,
            concept: String::new(),
            amount: Money::from_cents(euros * 100),
            category: Category::Payments,
            source: String::new(),
            overdue: false,
            estimated: false,
        }
    }

    #[test]
    fn minimo_dentro_del_periodo() {
        let today = date!(2026 - 10 - 24);
        // Un pago grande el 2/11 y un cobro mayor el 6/11: la semana cierra en positivo, pero
        // el 2/11 se queda en negativo.
        let items = [
            item(date!(2026 - 11 - 02), -1500),
            item(date!(2026 - 11 - 06), 2000),
        ];
        let h = horizon(
            today,
            Money::from_euros(1000, 0),
            &week_ranges(today),
            &items,
        );
        assert_eq!(h.buckets[2].closing, Money::from_euros(1500, 0));
        assert_eq!(h.lowest, -Money::from_euros(500, 0));
        assert_eq!(h.lowest_date, date!(2026 - 11 - 02));
        assert_eq!(h.first_negative, Some(date!(2026 - 11 - 02)));
        assert_eq!(h.points.len(), 3);
    }

    #[test]
    fn minimo_desde_el_final_de_hoy() {
        // Lo vencido cuenta hoy: el mínimo es el saldo al final de hoy, no el de antes.
        let today = date!(2026 - 10 - 24);
        let items = [
            item(today, 500),
            item(today, -200),
            item(date!(2026 - 11 - 02), 100),
        ];
        let h = horizon(
            today,
            Money::from_euros(1000, 0),
            &week_ranges(today),
            &items,
        );
        assert_eq!(h.points.len(), 3);
        assert_eq!(h.points[1].balance, Money::from_euros(1300, 0));
        assert_eq!(
            (h.lowest, h.lowest_date),
            (Money::from_euros(1300, 0), today)
        );
        assert_eq!(h.first_negative, None);
    }
}
