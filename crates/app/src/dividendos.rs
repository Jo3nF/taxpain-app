//! Acuerdos de reparto de dividendos, con las comprobaciones legales antes de permitirlos y el
//! acta que los documenta (G23).
//!
//! Tres clases de acuerdo ([`AgreementKind`]):
//! - **A cuenta** del resultado del ejercicio en curso (art. 277 LSC): lo acuerdan la junta o
//!   los administradores con un estado contable que ponga de manifiesto que hay liquidez
//!   suficiente, y no puede exceder de los resultados obtenidos desde el fin del último
//!   ejercicio, deducidas las pérdidas de ejercicios anteriores, las reservas obligatorias y el
//!   impuesto estimado sobre esos resultados. Asiento: 557 «Dividendo activo a cuenta» al debe y
//!   526 «Dividendo activo a pagar» al haber. Al aprobar las cuentas, la aplicación del resultado
//!   salda la 557 (`cierre::post_allocation`).
//! - **Con cargo a reservas** de libre disposición (art. 273.2 LSC): 113 «Reservas voluntarias»
//!   (o 120 «Remanente») al debe y 526 al haber.
//! - **Con cargo al resultado** del ejercicio cerrado: es el dividendo de la aplicación del
//!   resultado que se contabiliza en Cierre (que ya comprueba reserva legal, patrimonio neto e
//!   I+D, `cierre::check_allocation`); aquí sólo se registra el acuerdo y su acta.
//!
//! Antes de contabilizar, [`check`] da un semáforo: reserva legal (art. 274 LSC), patrimonio neto
//! no inferior al capital tras el reparto (art. 273.2), gastos de investigación y desarrollo
//! frente a reservas disponibles (art. 273.3), reservas de libre disposición, el límite y la
//! liquidez del art. 277 para el dividendo a cuenta, la ficha de socios y, si la sociedad puede
//! ser entidad patrimonial (art. 5.2 LIS), un aviso. Con algo en rojo, [`record`] no deja
//! contabilizar.
//!
//! [`record`] contabiliza el acuerdo y crea en el mismo paso el acta en borrador desde la
//! plantilla del libro de actas (enlazada con el acuerdo). El pago (con su retención y el 123 del
//! trimestre) sigue siendo `socios::pay`, que liga cada pago a su acuerdo.

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::account::AccountCode;
use taxpain_core::accounting::entry::{EntryLine, JournalEntry, Origin};
use taxpain_core::money::{Money, Rate};
use taxpain_models::m184::FULL_PCT;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, OffsetDateTime};

use crate::accounting;
use crate::cierre::ProposedLine;
use crate::error::{AppError, Result};
use crate::invoicing::now_str;
use crate::libros_societarios::actas::{self, LiquidityStatement, MinuteKind, Template};
use crate::libros_societarios::credit_balance_at;
use crate::socios::{self, PayoutLine};

fn other(e: impl ToString) -> AppError {
    AppError::Other(e.to_string())
}

fn code(s: &str) -> Result<AccountCode> {
    AccountCode::new(s).map_err(other)
}

fn parse_date(s: &str) -> Result<Date> {
    Date::parse(s, time::macros::format_description!("[year]-[month]-[day]")).map_err(other)
}

/// Clase de acuerdo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgreementKind {
    /// Dividendo a cuenta del resultado del ejercicio en curso (art. 277 LSC).
    ACuenta,
    /// Con cargo a reservas de libre disposición (art. 273.2 LSC).
    Reservas,
    /// Con cargo al resultado del ejercicio cerrado (la aplicación del resultado).
    Resultado,
}

impl AgreementKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::ACuenta => "a_cuenta",
            Self::Reservas => "reservas",
            Self::Resultado => "resultado",
        }
    }

    fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "a_cuenta" => Self::ACuenta,
            "reservas" => Self::Reservas,
            "resultado" => Self::Resultado,
            _ => return Err(other(format!("clase de acuerdo desconocida: {s}"))),
        })
    }
}

/// Datos del acuerdo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgreementInput {
    pub kind: AgreementKind,
    /// Fecha del acuerdo (de la junta o de la decisión del órgano de administración).
    pub date: Date,
    /// Dividendo bruto total (en el de con cargo al resultado sale de la aplicación).
    #[serde(default)]
    pub amount: Money,
    /// Fecha de pago fijada en el acuerdo (exigibilidad: decide el 123 de la retención).
    #[serde(default)]
    pub payment_date: Option<Date>,
    /// A cuenta: fecha del estado contable de liquidez (la del acuerdo si no se da).
    #[serde(default)]
    pub statement_date: Option<Date>,
    /// A cuenta: tipo del IS para estimar el impuesto (25 % si no se da).
    #[serde(default)]
    pub tax_rate: Option<Rate>,
    /// Con cargo a reservas: la cuenta (113 si no se da, o 120 remanente).
    #[serde(default)]
    pub reserve_account: Option<String>,
    /// Con cargo al resultado: el ejercicio cuyo resultado se aplica.
    #[serde(default)]
    pub year: Option<i32>,
    /// Órgano que acuerda (si no, el que propone la plantilla del acta: socio único o junta).
    #[serde(default)]
    pub organ: Option<MinuteKind>,
}

/// Color de una comprobación.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Light {
    Verde,
    Ambar,
    /// No se cumple: bloquea el acuerdo.
    Rojo,
}

/// Una comprobación legal del semáforo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    /// `reserva_legal`, `patrimonio_neto`, `investigacion`, `reservas_libres`, `limite_277`,
    /// `liquidez_277`, `socios`, `entidad_patrimonial`, `aplicacion`.
    pub code: String,
    pub title: String,
    pub light: Light,
    pub detail: String,
    pub norm: String,
}

fn check_of(code: &str, title: &str, light: Light, detail: String, norm: &str) -> Check {
    Check {
        code: code.into(),
        title: title.into(),
        light,
        detail,
        norm: norm.into(),
    }
}

/// Semáforo y asiento de un acuerdo, sin contabilizar nada.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AgreementCheck {
    pub kind: AgreementKind,
    pub date: Date,
    pub amount: Money,
    /// Cuenta cargada: 557, 113, 120 o 129.
    pub account: String,
    pub checks: Vec<Check>,
    /// Alguna comprobación en rojo: no se deja contabilizar.
    pub blocked: bool,
    /// Estado contable del art. 277 LSC (a cuenta).
    pub liquidity: Option<LiquidityStatement>,
    /// Asiento del acuerdo (vacío en el de con cargo al resultado, que es el de la aplicación).
    pub entry: Vec<ProposedLine>,
    /// Reparto por socio con la retención.
    pub lines: Vec<PayoutLine>,
    pub withheld: Money,
    /// Órgano del acta que se crea.
    pub organ: MinuteKind,
    pub warnings: Vec<String>,
}

fn credit(conn: &Connection, prefix: &str, date: Date) -> Result<Money> {
    Ok(Money::from_cents(credit_balance_at(conn, prefix, date)?))
}

fn debit(conn: &Connection, prefix: &str, date: Date) -> Result<Money> {
    Ok(-credit(conn, prefix, date)?)
}

/// Patrimonio neto a `date` a efectos del art. 273.2 LSC (art. 28.2 de la Resolución del ICAC de
/// 5-3-2019): grupos 10, 11 y 12 con el resultado del ejercicio en curso (6 y 7 sin regularizar)
/// menos el dividendo a cuenta (557). El grupo 13 no cuenta, como en `cierre::allocation`.
fn equity_at(conn: &Connection, date: Date) -> Result<Money> {
    let mut e = Money::ZERO;
    for p in ["10", "11", "12", "6", "7"] {
        e += credit(conn, p, date)?;
    }
    Ok(e - debit(conn, "557", date)?.max(Money::ZERO))
}

/// Gastos de investigación y desarrollo en el activo pendientes de amortizar (200 y 201 menos su
/// amortización acumulada, 2800 y 2801), como `cierre::check_allocation`.
fn research_at(conn: &Connection, date: Date) -> Result<Money> {
    let mut rd = Money::ZERO;
    for p in ["200", "201", "2800", "2801"] {
        rd += debit(conn, p, date)?;
    }
    Ok(rd.max(Money::ZERO))
}

/// Reservas disponibles a `date`: voluntarias (113) y remanente (120).
fn free_reserves_at(conn: &Connection, date: Date) -> Result<Money> {
    Ok(credit(conn, "113", date)? + credit(conn, "120", date)?)
}

fn proposed(conn: &Connection, account: &str, debit: Money, credit: Money) -> Result<ProposedLine> {
    Ok(ProposedLine {
        name: accounting::account_name(conn, account)?.unwrap_or_default(),
        account: account.into(),
        debit,
        credit,
    })
}

/// Asiento de la aplicación del resultado de `year` y lo que lleva a la 526 (dividendo por
/// pagar).
fn allocation_entry(conn: &Connection, year: i32) -> Result<Option<(i64, Date, Money)>> {
    let row: Option<(i64, String)> = conn
        .query_row(
            "SELECT id, date FROM journal_entry
              WHERE origin_kind = 'AplicacionResultado' AND origin_id = ?1",
            [year],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((id, date)) = row else {
        return Ok(None);
    };
    let to_pay: i64 = conn.query_row(
        "SELECT COALESCE(SUM(credit - debit), 0) FROM journal_line
          WHERE entry_id = ?1 AND account LIKE '526%'",
        [id],
        |r| r.get(0),
    )?;
    Ok(Some((id, parse_date(&date)?, Money::from_cents(to_pay))))
}

/// Comprobaciones comunes de la ficha de socios.
fn holders_check(holders: &[socios::Shareholder]) -> Check {
    if holders.is_empty() {
        return check_of(
            "socios",
            "Socios",
            Light::Rojo,
            "No hay socios activos en la ficha: dalos de alta con su participación para repartir \
             el dividendo y calcular la retención."
                .into(),
            "Art. 275 LSC",
        );
    }
    let sum: u64 = holders.iter().map(|h| u64::from(h.pct)).sum();
    if sum == u64::from(FULL_PCT) {
        check_of(
            "socios",
            "Socios",
            Light::Verde,
            format!(
                "{} socios con el 100 % del capital: el dividendo se reparte en proporción a su \
                 participación.",
                holders.len()
            ),
            "Art. 275 LSC",
        )
    } else {
        check_of(
            "socios",
            "Socios",
            Light::Ambar,
            format!(
                "Las participaciones de los socios activos suman {},{:04} %: se reparte en \
                 proporción a ellas (revisa la ficha si no hay autocartera).",
                sum / 10_000,
                sum % 10_000
            ),
            "Arts. 148 y 275 LSC",
        )
    }
}

/// Comprobaciones de reserva legal, patrimonio neto e I+D a `date` para repartir `amount`, que
/// sale de reservas por `from_reserves` (en el a cuenta, del resultado en curso).
fn equity_checks(
    conn: &Connection,
    date: Date,
    amount: Money,
    from_reserves: Money,
    kind: AgreementKind,
    legal_deduction: Option<Money>,
) -> Result<Vec<Check>> {
    let mut out = Vec::new();
    let capital = credit(conn, "100", date)?;
    let legal = credit(conn, "112", date)?;
    let target = capital.apply_rate(Rate::percent(20));
    out.push(match (kind, legal_deduction) {
        (AgreementKind::ACuenta, Some(d)) if d.cents() > 0 => check_of(
            "reserva_legal",
            "Reserva legal",
            Light::Verde,
            format!(
                "La reserva legal ({legal}) aún no llega al 20 % del capital ({target}): del \
                 límite del dividendo a cuenta se descuenta la dotación obligatoria de este \
                 ejercicio, {d} (10 % del resultado)."
            ),
            "Arts. 274 y 277.b LSC",
        ),
        _ if legal >= target => check_of(
            "reserva_legal",
            "Reserva legal",
            Light::Verde,
            format!("Dotada: {legal} (el 20 % del capital es {target})."),
            "Art. 274 LSC",
        ),
        _ => check_of(
            "reserva_legal",
            "Reserva legal",
            Light::Ambar,
            format!(
                "La reserva legal ({legal}) no llega al 20 % del capital ({target}). No se puede \
                 repartir y, hasta que llegue, cada aplicación del resultado lleva a ella al \
                 menos el 10 % del beneficio. Las reservas voluntarias sí se pueden repartir si \
                 se cumple lo demás; si los estatutos exigen otras reservas, cúbrelas antes."
            ),
            "Arts. 273.2 y 274 LSC",
        ),
    });
    let equity = equity_at(conn, date)?;
    let after = equity - amount;
    out.push(if after < capital {
        check_of(
            "patrimonio_neto",
            "Patrimonio neto frente al capital",
            Light::Rojo,
            format!(
                "Tras el reparto el patrimonio neto sería {after} y el capital es {capital}: no \
                 se pueden repartir dividendos si el patrimonio neto es o queda por debajo del \
                 capital. Como mucho, {}.",
                (equity - capital).max(Money::ZERO)
            ),
            "Art. 273.2 LSC; art. 28 de la Resolución del ICAC de 5-3-2019",
        )
    } else {
        check_of(
            "patrimonio_neto",
            "Patrimonio neto frente al capital",
            Light::Verde,
            format!(
                "Patrimonio neto antes del reparto {equity} (con el resultado del ejercicio en \
                 curso y sin la 557); tras él, {after}, no inferior al capital ({capital})."
            ),
            "Art. 273.2 LSC",
        )
    });
    let rd = research_at(conn, date)?;
    let free_after = free_reserves_at(conn, date)? - from_reserves;
    out.push(if rd.is_zero() {
        check_of(
            "investigacion",
            "Gastos de investigación y desarrollo",
            Light::Verde,
            "No hay gastos de investigación ni de desarrollo en el activo pendientes de \
             amortizar."
                .into(),
            "Art. 273.3 LSC",
        )
    } else if rd > free_after {
        check_of(
            "investigacion",
            "Gastos de investigación y desarrollo",
            Light::Rojo,
            format!(
                "Hay {rd} de investigación y desarrollo en el activo pendientes de amortizar y \
                 las reservas disponibles (voluntarias y remanente) serían {free_after}: no se \
                 pueden repartir dividendos hasta que las reservas los cubran."
            ),
            "Art. 273.3 LSC",
        )
    } else {
        check_of(
            "investigacion",
            "Gastos de investigación y desarrollo",
            Light::Verde,
            format!(
                "{rd} de investigación y desarrollo pendientes de amortizar, cubiertos por las \
                 reservas disponibles ({free_after})."
            ),
            "Art. 273.3 LSC",
        )
    });
    Ok(out)
}

/// Aviso si la sociedad puede ser entidad patrimonial (más de la mitad del activo en valores o
/// no afecto con lo marcado en Entidad patrimonial).
fn patrimonial_check(v: &Vault, year: i32, date: Date) -> Option<Check> {
    let (pct, safe) = crate::patrimonial::senal(v, year, date)?;
    // TODO(VERIFY): efectos del reparto en una entidad patrimonial. La LIS no prohíbe repartir;
    // lo que se avisa son las consecuencias conocidas (art. 5.2 para el cómputo de la tesorería,
    // art. 21.1 para la socia sociedad, art. 29.1 para el tipo de la estimación).
    Some(if pct > 50 {
        check_of(
            "entidad_patrimonial",
            "Entidad patrimonial",
            Light::Ambar,
            format!(
                "Con lo marcado en Entidad patrimonial, el {pct} % del activo son valores o \
                 elementos no afectos ({safe} % sin contar la tesorería sin justificar): puede \
                 ser entidad patrimonial. Repartir no está prohibido, pero la tesorería que sale \
                 computaba como no afecta y el reparto cambia el porcentaje del ejercicio; el \
                 impuesto estimado va al tipo general del 25 %; y si una socia sociedad quiere \
                 la exención del art. 21 LIS y más del 70 % de los ingresos de esta sociedad son \
                 dividendos o plusvalías de participaciones, necesita además el 5 % indirecto en \
                 esas participadas. Consúltalo antes de repartir."
            ),
            "Arts. 5.2, 21.1 y 29.1 LIS",
        )
    } else {
        check_of(
            "entidad_patrimonial",
            "Entidad patrimonial",
            Light::Verde,
            format!(
                "Valores y elementos no afectos: el {pct} % del activo con lo marcado (menos de \
                 la mitad)."
            ),
            "Art. 5.2 LIS",
        )
    })
}

fn template_of(input: &AgreementInput, statement: Date, year: Option<i32>) -> Template {
    match input.kind {
        AgreementKind::ACuenta => Template::DividendoCuenta {
            amount: input.amount,
            payment_date: input.payment_date,
            statement_date: statement,
            tax_rate: input.tax_rate,
        },
        AgreementKind::Reservas => Template::Dividendo {
            amount: input.amount,
            payment_date: input.payment_date,
        },
        AgreementKind::Resultado => Template::Cuentas {
            year: year.unwrap_or(input.date.year() - 1),
        },
    }
}

/// Semáforo, estado de liquidez, reparto y asiento de un acuerdo de dividendos. No guarda nada.
pub fn check(v: &Vault, input: &AgreementInput) -> Result<AgreementCheck> {
    crate::libros_societarios::company(v)?;
    if !crate::companies::info(v)?
        .nif
        .as_deref()
        .and_then(|n| n.trim().chars().next())
        .is_some_and(|c| matches!(c.to_ascii_uppercase(), 'A' | 'B'))
    {
        return Err(other(
            "los dividendos con las comprobaciones de la Ley de Sociedades de Capital son de las \
             SA y las SL",
        ));
    }
    let conn = v.connection();
    let holders: Vec<_> = socios::shareholders(v)?
        .into_iter()
        .filter(|s| s.active)
        .collect();
    let mut checks = Vec::new();
    let mut warnings = Vec::new();
    let mut liquidity = None;
    let mut entry = Vec::new();
    let fy = accounting::fiscal_year_at(v, input.date)?;
    let (amount, account) = match input.kind {
        AgreementKind::ACuenta => {
            if input.amount.cents() <= 0 {
                return Err(other("pon el importe del dividendo a cuenta"));
            }
            let statement = input.statement_date.unwrap_or(input.date);
            if statement > input.date {
                return Err(other(
                    "el estado contable es de la fecha del acuerdo o anterior",
                ));
            }
            if !fy.contains(statement) {
                return Err(other(format!(
                    "el estado contable ha de ser del mismo ejercicio que el acuerdo (del {} al \
                     {}): el dividendo es a cuenta del resultado de ese ejercicio",
                    fy.start, fy.end
                )));
            }
            let l = actas::liquidity_statement_until(
                v,
                statement,
                input.date,
                input.amount,
                input.tax_rate,
            )?;
            checks.extend(equity_checks(
                conn,
                input.date,
                input.amount,
                Money::ZERO,
                AgreementKind::ACuenta,
                Some(l.legal_reserve),
            )?);
            checks.push(if l.result.cents() <= 0 {
                check_of(
                    "limite_277",
                    "Límite del dividendo a cuenta",
                    Light::Rojo,
                    format!(
                        "Desde el cierre del último ejercicio (del {} al {}) no hay beneficios \
                         ({}): no se puede repartir a cuenta.",
                        l.period_start, l.date, l.result
                    ),
                    "Art. 277.b LSC",
                )
            } else if input.amount > l.limit {
                check_of(
                    "limite_277",
                    "Límite del dividendo a cuenta",
                    Light::Rojo,
                    format!(
                        "El dividendo ({}) supera la cantidad máxima distribuible a cuenta, {}: \
                         resultado desde el último cierre {}, menos pérdidas anteriores {}, \
                         reserva legal {}, impuesto estimado {} ({}) y lo ya repartido a cuenta \
                         {}.",
                        input.amount,
                        l.limit,
                        l.result,
                        l.prior_losses,
                        l.legal_reserve,
                        l.estimated_tax,
                        l.tax_rate,
                        l.already_distributed
                    ),
                    "Art. 277.b LSC",
                )
            } else {
                check_of(
                    "limite_277",
                    "Límite del dividendo a cuenta",
                    Light::Verde,
                    format!(
                        "Cantidad máxima distribuible a cuenta: {} (resultado desde el último \
                         cierre {}, menos pérdidas anteriores, reserva legal, impuesto estimado \
                         al {} y lo ya repartido a cuenta).",
                        l.limit, l.result, l.tax_rate
                    ),
                    "Art. 277.b LSC",
                )
            });
            checks.push(if input.amount > l.treasury {
                check_of(
                    "liquidez_277",
                    "Liquidez",
                    Light::Rojo,
                    format!(
                        "La tesorería al {} ({}) no cubre el dividendo ({}): el estado contable \
                         ha de poner de manifiesto que hay liquidez suficiente.",
                        l.date, l.treasury, input.amount
                    ),
                    "Art. 277.a LSC",
                )
            } else if l.treasury_after < l.short_term_debts {
                check_of(
                    "liquidez_277",
                    "Liquidez",
                    Light::Ambar,
                    format!(
                        "Tras el pago quedarían {} de tesorería frente a {} de deudas a corto \
                         plazo: completa en el estado la previsión de cobros y pagos para \
                         justificar que hay liquidez suficiente.",
                        l.treasury_after, l.short_term_debts
                    ),
                    "Art. 277.a LSC",
                )
            } else {
                check_of(
                    "liquidez_277",
                    "Liquidez",
                    Light::Verde,
                    format!(
                        "Tesorería {} al {}; tras el pago, {}, que cubre las deudas a corto \
                         plazo ({}).",
                        l.treasury, l.date, l.treasury_after, l.short_term_debts
                    ),
                    "Art. 277.a LSC",
                )
            });
            liquidity = Some(l);
            entry.push(proposed(conn, "557", input.amount, Money::ZERO)?);
            entry.push(proposed(conn, "526", Money::ZERO, input.amount)?);
            (input.amount, "557".to_owned())
        }
        AgreementKind::Reservas => {
            if input.amount.cents() <= 0 {
                return Err(other("pon el importe del dividendo"));
            }
            let account = input
                .reserve_account
                .as_deref()
                .map(str::trim)
                .filter(|a| !a.is_empty())
                .unwrap_or("113")
                .to_owned();
            if !(account.starts_with("113") || account.starts_with("120")) {
                return Err(other(
                    "el dividendo con cargo a reservas sale de las reservas voluntarias (113) o \
                     del remanente (120)",
                ));
            }
            code(&account)?;
            let available = credit(conn, &account, input.date)?;
            checks.push(if input.amount > available {
                check_of(
                    "reservas_libres",
                    "Reservas de libre disposición",
                    Light::Rojo,
                    format!(
                        "La {account} tiene {available} a esa fecha: el dividendo ({}) no puede \
                         superarla. La reserva legal, la de capitalización y las indisponibles \
                         no se reparten.",
                        input.amount
                    ),
                    "Arts. 273.2 y 274 LSC; art. 25 LIS",
                )
            } else {
                check_of(
                    "reservas_libres",
                    "Reservas de libre disposición",
                    Light::Verde,
                    format!("La {account} tiene {available} a esa fecha."),
                    "Art. 273.2 LSC",
                )
            });
            checks.extend(equity_checks(
                conn,
                input.date,
                input.amount,
                input.amount,
                AgreementKind::Reservas,
                None,
            )?);
            entry.push(proposed(conn, &account, input.amount, Money::ZERO)?);
            entry.push(proposed(conn, "526", Money::ZERO, input.amount)?);
            (input.amount, account)
        }
        AgreementKind::Resultado => {
            let year = input.year.unwrap_or(fy.year - 1);
            let Some((_, alloc_date, to_pay)) = allocation_entry(conn, year)? else {
                return Err(other(format!(
                    "contabiliza antes la aplicación del resultado de {year} en Cierre: comprueba \
                     la reserva legal, el patrimonio neto y la I+D, y lleva el dividendo a la 526"
                )));
            };
            if to_pay.cents() <= 0 {
                return Err(other(format!(
                    "la aplicación del resultado de {year} no lleva dividendo por pagar (526)"
                )));
            }
            let registered: i64 = conn.query_row(
                "SELECT count(*) FROM dividend_agreement
                  WHERE kind = 'resultado' AND allocation_year = ?1",
                [year],
                |r| r.get(0),
            )?;
            if registered > 0 {
                return Err(other(format!(
                    "el acuerdo del dividendo del resultado de {year} ya está registrado"
                )));
            }
            if alloc_date != input.date {
                warnings.push(format!(
                    "La aplicación del resultado está contabilizada el {alloc_date}: el acta de \
                     la junta suele llevar esa misma fecha."
                ));
            }
            let p = crate::cierre::allocation(v, year)?;
            checks.push(check_of(
                "aplicacion",
                "Aplicación del resultado",
                Light::Verde,
                format!(
                    "Contabilizada el {alloc_date} con {to_pay} de dividendo por pagar. Al \
                     contabilizarla se comprobó la reserva legal (mínimo {}), el patrimonio neto \
                     (dividendo máximo {}) y la I+D.",
                    p.legal_minimum, p.max_dividends
                ),
                "Arts. 273 y 274 LSC",
            ));
            (to_pay, "129".to_owned())
        }
    };
    checks.push(holders_check(&holders));
    if let Some(c) = patrimonial_check(v, fy.year, input.date) {
        checks.push(c);
    }
    if let Some(p) = input.payment_date
        && p < input.date
    {
        return Err(other("la fecha de pago es la del acuerdo o posterior"));
    }
    let lines = socios::split(amount, &holders);
    let withheld = lines.iter().map(|l| l.withheld).sum();
    let organ = input.organ.unwrap_or_else(|| default_organ(&holders));
    Ok(AgreementCheck {
        kind: input.kind,
        date: input.date,
        amount,
        account,
        blocked: checks.iter().any(|c| c.light == Light::Rojo),
        checks,
        liquidity,
        entry,
        lines,
        withheld,
        organ,
        warnings,
    })
}

/// Órgano por defecto del acta: el socio único o la junta universal (el de la plantilla).
fn default_organ(holders: &[socios::Shareholder]) -> MinuteKind {
    match holders {
        [s] if s.pct == FULL_PCT => MinuteKind::SocioUnico,
        _ => MinuteKind::JuntaUniversal,
    }
}

/// Lo que crea [`record`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Recorded {
    pub id: i64,
    pub entry_id: Option<i64>,
    pub minute_id: i64,
}

/// Contabiliza el acuerdo (con confirmación) y crea su acta en borrador. Con alguna
/// comprobación en rojo, no.
pub fn record(
    v: &Vault,
    input: &AgreementInput,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<Recorded> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let c = check(v, input)?;
    if c.blocked {
        let red: Vec<String> = c
            .checks
            .iter()
            .filter(|x| x.light == Light::Rojo)
            .map(|x| format!("{}: {}", x.title, x.detail))
            .collect();
        return Err(other(format!(
            "no se puede acordar el dividendo. {}",
            red.join(" ")
        )));
    }
    let statement = input.statement_date.unwrap_or(input.date);
    let year = match input.kind {
        AgreementKind::Resultado => Some(input.year.unwrap_or(input.date.year() - 1)),
        _ => None,
    };
    let draft = actas::template_by(
        v,
        &template_of(input, statement, year),
        input.date,
        Some(c.organ),
    )?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO dividend_agreement(kind, date, amount, payment_date, statement_date,
           tax_rate_bp, account, allocation_year, checks, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            input.kind.as_str(),
            input.date.to_string(),
            c.amount.cents(),
            input.payment_date.map(|d| d.to_string()),
            (input.kind == AgreementKind::ACuenta).then(|| statement.to_string()),
            c.liquidity.as_ref().map(|l| l.tax_rate.bp()),
            c.account,
            year,
            serde_json::to_string(&c.checks).map_err(other)?,
            now_str(now)
        ],
    )?;
    let id = tx.last_insert_rowid();
    let entry_id = match input.kind {
        AgreementKind::Resultado => {
            allocation_entry(&tx, year.unwrap_or_default())?.map(|(e, _, _)| e)
        }
        kind => {
            let concept = if kind == AgreementKind::ACuenta {
                "Acuerdo de dividendo a cuenta"
            } else {
                "Acuerdo de dividendo con cargo a reservas"
            };
            let e = JournalEntry {
                date: input.date,
                concept: concept.into(),
                origin: Origin::AcuerdoDividendo(id),
                lines: vec![
                    EntryLine::debit(code(&c.account)?, c.amount),
                    EntryLine::credit(code("526")?, c.amount),
                ],
            };
            Some(accounting::record_entry(&tx, &e, now).map_err(|err| {
                other(format!(
                    "no se pudo contabilizar el acuerdo ({err}): ¿está cerrado el ejercicio?"
                ))
            })?)
        }
    };
    let minute_id = actas::write_minute(&tx, draft.minute.fiscal_year, &draft.minute, now)?;
    tx.execute(
        "UPDATE dividend_agreement SET entry_id = ?1, minute_id = ?2 WHERE id = ?3",
        params![entry_id, minute_id, id],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "dividend.agree",
        &id.to_string(),
        &serde_json::json!({
            "kind": input.kind.as_str(),
            "amount": c.amount.cents(),
            "entry": entry_id,
            "minute": minute_id,
        }),
    )?;
    tx.commit()?;
    Ok(Recorded {
        id,
        entry_id,
        minute_id,
    })
}

/// Un acuerdo registrado, con lo pagado y el estado de su acta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Agreement {
    pub id: i64,
    pub kind: AgreementKind,
    pub date: Date,
    pub amount: Money,
    pub payment_date: Option<Date>,
    pub account: String,
    pub allocation_year: Option<i32>,
    pub entry_id: Option<i64>,
    pub minute_id: Option<i64>,
    /// Número del acta en el libro (aprobada).
    pub minute_number: Option<u32>,
    pub minute_approved: bool,
    /// Pagado (bruto) con cargo a este acuerdo.
    pub paid: Money,
    pub pending: Money,
}

const AGREEMENT_SELECT: &str = "SELECT a.id, a.kind, a.date, a.amount, a.payment_date, a.account,
       a.allocation_year, a.entry_id, a.minute_id, m.number, COALESCE(m.status = 'aprobada', 0),
       (SELECT COALESCE(SUM(p.gross), 0) FROM dividend_payment p WHERE p.agreement_id = a.id)
  FROM dividend_agreement a LEFT JOIN minute m ON m.id = a.minute_id";

fn agreements_where(conn: &Connection, filter: &str, arg: Option<i64>) -> Result<Vec<Agreement>> {
    let mut st = conn.prepare(&format!(
        "{AGREEMENT_SELECT} {filter} ORDER BY a.date, a.id"
    ))?;
    type Row = (
        i64,
        String,
        String,
        i64,
        Option<String>,
        String,
        Option<i32>,
        Option<i64>,
        Option<i64>,
        Option<u32>,
        bool,
        i64,
    );
    let rows = st.query_map([arg], |r| -> taxpain_vault::rusqlite::Result<Row> {
        Ok((
            r.get(0)?,
            r.get(1)?,
            r.get(2)?,
            r.get(3)?,
            r.get(4)?,
            r.get(5)?,
            r.get(6)?,
            r.get(7)?,
            r.get(8)?,
            r.get(9)?,
            r.get(10)?,
            r.get(11)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, kind, date, amount, pay, account, year, entry, minute, number, approved, paid) =
            row?;
        let (amount, paid) = (Money::from_cents(amount), Money::from_cents(paid));
        out.push(Agreement {
            id,
            kind: AgreementKind::parse(&kind)?,
            date: parse_date(&date)?,
            amount,
            payment_date: pay.as_deref().map(parse_date).transpose()?,
            account,
            allocation_year: year,
            entry_id: entry,
            minute_id: minute,
            minute_number: number,
            minute_approved: approved,
            paid,
            pending: (amount - paid).max(Money::ZERO),
        });
    }
    Ok(out)
}

/// Todos los acuerdos, por fecha.
pub fn agreements(v: &Vault) -> Result<Vec<Agreement>> {
    agreements_where(v.connection(), "WHERE ?1 IS NULL OR a.id = ?1", None)
}

pub fn agreement(conn: &Connection, id: i64) -> Result<Agreement> {
    agreements_where(conn, "WHERE a.id = ?1", Some(id))?
        .pop()
        .ok_or(AppError::NotFound)
}

/// El acuerdo que paga un pago de `date` sin acuerdo elegido: el más antiguo con algo pendiente
/// acordado hasta esa fecha (la 526 se paga por orden).
pub fn oldest_pending(conn: &Connection, date: Date) -> Result<Option<Agreement>> {
    Ok(
        agreements_where(conn, "WHERE ?1 IS NULL OR a.id = ?1", None)?
            .into_iter()
            .find(|a| a.date <= date && a.pending.cents() > 0),
    )
}

/// Deshace un acuerdo: borra su asiento (si el ejercicio está abierto) y su acta si sigue en
/// borrador sin número; un acta aprobada se queda en el libro (sin enlace). No se deshace con
/// pagos hechos ni, el de a cuenta, con el resultado ya aplicado (salda su 557).
pub fn undo(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let a = agreement(conn, id)?;
    let payments: i64 = conn.query_row(
        "SELECT count(*) FROM dividend_payment WHERE agreement_id = ?1",
        [id],
        |r| r.get(0),
    )?;
    if payments > 0 {
        return Err(other(
            "ya hay pagos de este acuerdo: deshaz antes los pagos (Pagos de dividendos)",
        ));
    }
    if a.kind == AgreementKind::ACuenta {
        let fy = accounting::fiscal_year_at(v, a.date)?;
        if allocation_entry(conn, fy.year)?.is_some() {
            return Err(other(format!(
                "la aplicación del resultado de {} ya saldó la 557 de este dividendo a cuenta: \
                 deshaz antes la aplicación (Cierre)",
                fy.year
            )));
        }
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM dividend_agreement WHERE id = ?1", [id])?;
    if a.kind != AgreementKind::Resultado
        && let Some(e) = a.entry_id
    {
        // Los disparadores impiden borrar en un ejercicio cerrado: el error lo explica.
        tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [e])
            .map_err(|_| other("el ejercicio de ese acuerdo está cerrado: reábrelo"))?;
        tx.execute("DELETE FROM journal_entry WHERE id = ?1", [e])?;
    }
    let mut minute_deleted = false;
    if let Some(m) = a.minute_id {
        let draft: Option<bool> = tx
            .query_row(
                "SELECT status = 'borrador' AND number IS NULL FROM minute WHERE id = ?1",
                [m],
                |r| r.get(0),
            )
            .optional()?;
        if draft == Some(true) {
            tx.execute("DELETE FROM minute WHERE id = ?1", [m])?;
            minute_deleted = true;
        }
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "dividend.agree_undo",
        &id.to_string(),
        &serde_json::json!({
            "entry": a.entry_id,
            "minute": a.minute_id,
            "minute_deleted": minute_deleted,
        }),
    )?;
    tx.commit()?;
    Ok(())
}
