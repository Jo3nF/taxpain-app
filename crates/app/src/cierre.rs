//! Cierre del ejercicio guiado: lo que falta antes de cerrar, el asiento del Impuesto sobre
//! Sociedades (desde el borrador del 200) y, ya cerrado, la aplicación del resultado que aprueba
//! la junta (reserva legal, compensación de pérdidas, reservas voluntarias, dividendos).
//!
//! - IS (PGC, NRV 13.ª): 6300 al debe por la cuota del ejercicio [00592]; 473 al haber por las
//!   retenciones y pagos fraccionados; la diferencia [00611] a la 4752 (a pagar) o a la 4709 (a
//!   devolver). Fecha: el último día del ejercicio. Sin impuesto diferido (lo habitual en una SL
//!   pequeña sin diferencias temporarias).
//! - Aplicación (arts. 273 y 274 LSC; art. 28 de la Resolución del ICAC de 5-3-2019, BOE-A-2019-3422):
//!   si las pérdidas de ejercicios anteriores dejan el patrimonio neto por debajo del capital, el
//!   beneficio compensa primero esas pérdidas (lo que falta hasta el capital) y sólo después va a
//!   la reserva legal; un 10 % del beneficio a la reserva legal hasta que llegue al 20 % del
//!   capital; dividendos sólo si el patrimonio neto no queda por debajo del capital (sin contar
//!   las subvenciones ni los ajustes por cambios de valor, grupo 13) y si las reservas libres
//!   cubren lo activado en I+D; la reserva de capitalización (art. 25.1.b LIS) por la reducción
//!   aplicada en el 200. Se contabiliza en el ejercicio siguiente, con la fecha de la junta (en
//!   los seis meses siguientes al cierre, art. 164 LSC). Los dividendos llevan una retención del
//!   19 % al pagarlos (modelo 123). Sólo para SA y SL (la LSC); el IS, para las entidades que lo
//!   pagan (no las comunidades de bienes ni las de propietarios, en atribución de rentas).
//!
//! TaxPain propone; los administradores formulan y la junta aprueba.

use serde::{Deserialize, Serialize};
use taxpain_core::accounting::account::AccountCode;
use taxpain_core::accounting::entry::{EntryLine, JournalEntry, Origin};
use taxpain_core::money::{Money, Rate};
use taxpain_core::nif::Nif;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, Duration, OffsetDateTime};

use crate::accounting::{self, record_entry};
use crate::error::{AppError, Result};
use crate::invoicing::now_str;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CheckItem {
    pub id: String,
    pub label: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Deadline {
    pub label: String,
    pub date: Date,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Checklist {
    pub year: i32,
    pub start: Date,
    pub end: Date,
    pub closed: bool,
    /// Entidad que paga el Impuesto sobre Sociedades.
    pub company: bool,
    /// Sociedad de capital (SA o SL): aplicación del resultado según la LSC y dividendos.
    pub lsc: bool,
    pub items: Vec<CheckItem>,
    /// Plazos tras el cierre (legalización, cuentas, depósito, 200).
    pub deadlines: Vec<Deadline>,
}

/// Último día de los `months` meses siguientes al cierre `end` (un ejercicio que acaba el 30 de
/// junio: los seis meses siguientes acaban el 31 de diciembre, no el 30).
pub fn after_close(end: Date, months: u32) -> Date {
    crate::recurring::months_after(end + Duration::days(1), months) - Duration::days(1)
}

/// Fin del plazo del modelo 200 de un ejercicio que acaba en `end`: los 25 días naturales
/// siguientes a los seis meses posteriores al cierre (art. 124.1 LIS).
pub fn corporate_tax_deadline(end: Date) -> Date {
    after_close(end, 6) + Duration::days(25)
}

/// Fin del plazo para depositar las cuentas de un ejercicio que acaba en `end`: la junta
/// ordinaria se celebra dentro de los seis primeros meses (art. 164.1 LSC) y el depósito, dentro
/// del mes siguiente a la aprobación (art. 279.1 LSC): un mes contado de fecha a fecha desde el
/// último día de la junta (cierre a 31/12 → 30 de julio), como en el calendario fiscal.
pub fn deposit_deadline(end: Date) -> Date {
    crate::recurring::months_after(after_close(end, 6), 1)
}

fn code(s: &str) -> Result<AccountCode> {
    AccountCode::new(s).map_err(|e| AppError::Other(e.to_string()))
}

fn count(conn: &Connection, sql: &str, p: impl taxpain_vault::rusqlite::Params) -> Result<i64> {
    Ok(conn.query_row(sql, p, |r| r.get(0))?)
}

/// Primera letra del NIF de la empresa si es una entidad (no una persona física).
fn entity_letter(v: &Vault) -> Result<Option<char>> {
    let nif = crate::companies::info(v)?.nif;
    Ok(nif
        .as_deref()
        .and_then(|n| Nif::parse(n).ok())
        .filter(|n| !n.is_persona_fisica())
        .and_then(|n| n.as_str().chars().next()))
}

/// ¿Paga el Impuesto sobre Sociedades? Las comunidades de bienes (E) y de propietarios (H) y los
/// organismos públicos (P, Q, S) no (atribución de rentas o exentos). Las sociedades civiles (J)
/// sólo si tienen objeto mercantil (se avisa).
fn pays_corporate_tax(letter: Option<char>) -> bool {
    letter.is_some_and(|c| !matches!(c, 'E' | 'H' | 'P' | 'Q' | 'S'))
}

/// ¿Es una sociedad de capital (SA o SL), a la que se aplican los arts. 273 y 274 LSC?
fn capital_company(letter: Option<char>) -> bool {
    matches!(letter, Some('A' | 'B'))
}

/// Saldo deudor (positivo) o acreedor (negativo) de las cuentas que empiezan por `prefix` al
/// final del ejercicio `year`, antes del asiento de cierre.
fn balance(conn: &Connection, year: i32, prefix: &str) -> Result<Money> {
    let cents: i64 = conn.query_row(
        "SELECT COALESCE(SUM(l.debit - l.credit), 0) FROM journal_line l
           JOIN journal_entry e ON e.id = l.entry_id
          WHERE e.fiscal_year = ?1 AND e.origin_kind != 'Cierre' AND l.account LIKE ?2 || '%'",
        params![year, prefix],
        |r| r.get(0),
    )?;
    Ok(Money::from_cents(cents))
}

fn entry_of(conn: &Connection, kind: &str, year: i64) -> Result<Option<i64>> {
    Ok(conn
        .query_row(
            "SELECT id FROM journal_entry WHERE origin_kind = ?1 AND origin_id = ?2",
            params![kind, year],
            |r| r.get(0),
        )
        .optional()?)
}

/// Lo que conviene tener hecho antes de cerrar `year`, y los plazos de después.
pub fn checklist(v: &Vault, year: i32) -> Result<Checklist> {
    let conn = v.connection();
    let fy = accounting::fiscal_year(v, year)?;
    let (start, end) = (fy.start.to_string(), fy.end.to_string());
    let letter = entity_letter(v)?;
    let company = pays_corporate_tax(letter);
    let lsc = capital_company(letter);
    let closed = count(
        conn,
        "SELECT count(*) FROM fiscal_year WHERE year = ?1 AND closed = 1",
        [year],
    )? > 0;
    let mut items = Vec::new();
    let mut push = |id: &str, label: &str, ok: bool, detail: String| {
        items.push(CheckItem {
            id: id.into(),
            label: label.into(),
            ok,
            detail,
        });
    };

    let pending = count(
        conn,
        "SELECT count(*) FROM invoice_received WHERE status = 'pendiente'
           AND (issue_date IS NULL OR issue_date BETWEEN ?1 AND ?2)",
        params![start, end],
    )?;
    push(
        "recibidas",
        "Facturas recibidas revisadas",
        pending == 0,
        if pending == 0 {
            "Todas revisadas.".into()
        } else {
            format!("Quedan {pending} por revisar (Recibidas): sin revisar no cuentan.")
        },
    );
    let bank = count(
        conn,
        // Las compras con tarjeta de débito sólo informan (ya salen en la cuenta), y lo que ya
        // contabilizó un diario importado no se vuelve a conciliar.
        &format!(
            "SELECT count(*) FROM bank_tx t JOIN bank_account b ON b.id = t.bank_account
             WHERE t.entry_id IS NULL AND t.op_date BETWEEN ?1 AND ?2
               AND b.kind <> 'tarjeta_debito' AND NOT {}",
            crate::bank::BOOKED_ELSEWHERE
        ),
        params![start, end],
    )?;
    push(
        "banco",
        "Banco conciliado",
        bank == 0,
        if bank == 0 {
            "Todos los movimientos del ejercicio tienen asiento.".into()
        } else {
            format!("{bank} movimientos sin conciliar (Contabilidad → Banco).")
        },
    );
    let depreciation = accounting::accrued_depreciation(v, year, fy.end)?;
    push(
        "amortizaciones",
        "Amortizaciones del ejercicio",
        depreciation == Money::ZERO,
        if depreciation == Money::ZERO {
            "Contabilizadas.".into()
        } else {
            format!("Faltan {depreciation} (Contabilidad → Inmovilizado → «Amortizar»).")
        },
    );
    // Ventas del inmovilizado sin su factura vinculada: el precio sigue en la 543 y la factura
    // cuenta como ventas, así que el ingreso estaría dos veces (CONTABILIDAD-08).
    let (sales, unfinished) = crate::venta_inmovilizado::unfinished_sales(conn, fy.start, fy.end)?;
    if sales > 0 {
        push(
            "ventas_inmovilizado",
            "Ventas del inmovilizado con su factura",
            unfinished.is_empty(),
            if unfinished.is_empty() {
                "Todas tienen su factura de venta vinculada o la 543 ya saldada.".into()
            } else {
                format!("{}.", unfinished.join(". "))
            },
        );
    }
    // Lo aceptado sólo por el entorno de pruebas (`pruebas`) no está remitido a la AEAT
    // (art. 16 RD 1007/2023): cuenta como sin enviar.
    let unsent_status = |status: &str| {
        count(
            conn,
            "SELECT count(*) FROM verifactu_record r JOIN invoice_issued i ON i.id = r.invoice_id
              WHERE r.send_status = ?3 AND i.issue_date BETWEEN ?1 AND ?2",
            params![start, end, status],
        )
        .unwrap_or(0)
    };
    let (unsent, in_testing) = (unsent_status("pendiente"), unsent_status("pruebas"));
    push(
        "verifactu",
        "Registros VERI*FACTU enviados",
        unsent + in_testing == 0,
        match (unsent, in_testing) {
            (0, 0) => "No queda ninguno pendiente.".into(),
            (n, 0) => format!("{n} sin enviar (Facturas)."),
            (0, t) => format!(
                "{t} enviados sólo al entorno de pruebas de la AEAT: no están remitidos (Facturas)."
            ),
            (n, t) => format!(
                "{n} sin enviar y {t} enviados sólo al entorno de pruebas de la AEAT, que no \
                 están remitidos (Facturas)."
            ),
        },
    );
    let last_vat = count(
        conn,
        "SELECT count(*) FROM journal_entry WHERE fiscal_year = ?1 AND origin_kind = 'LiquidacionIva'
           AND date >= ?2",
        params![year, (fy.end - Duration::days(31)).to_string()],
    )?;
    push(
        "iva",
        "IVA del último periodo liquidado",
        last_vat > 0,
        if last_vat > 0 {
            "Hay asiento de liquidación del último periodo.".into()
        } else {
            "Contabiliza la liquidación del último 303 (Contabilidad → IVA y libros).".into()
        },
    );
    // Saldos fantasma de las liquidaciones antiguas (4700 deudora y 4750 acreedora a la vez,
    // ver `vat_balances`): se avisa y se propone el ajuste, nunca se contabiliza solo.
    let phantom = vat_balances(v, year)?;
    push(
        "iva_saldos",
        "Saldos de la 4700 y la 4750",
        phantom.is_none(),
        match &phantom {
            None => {
                let opening = opening_unpaid_vat(conn, year)?;
                if opening.is_zero() {
                    "Sin saldos simultáneos: las compensaciones están aplicadas.".into()
                } else {
                    format!(
                        "Sin saldos que ajustar. La 4750 de la apertura ({opening}) es el 303 \
                         del último periodo de {}, que no tiene pago registrado: regístralo en \
                         Modelos cuando lo pagues.",
                        year - 1
                    )
                }
            }
            Some(p) => format!(
                "La 4700 (IVA a compensar) tiene {} y la 4750 (IVA a ingresar) {} a la vez. \
                 Si la 4700 es IVA que un 303 ya compensó, contabiliza el ajuste de {} (abajo); \
                 si no, mira en el mayor de dónde sale.",
                p.debtor, p.creditor, p.amount
            ),
        },
    );
    // Saldos anómalos y clientes vencidos de más de seis meses sin deterioro (ajustes de
    // cierre, ver `ajustes_cierre`).
    let anomalos: Vec<String> = crate::ajustes_cierre::comprobaciones(v, year)?
        .into_iter()
        .filter(|c| !c.ok)
        .map(|c| c.label)
        .collect();
    push(
        "saldos",
        "Saldos sin anomalías",
        anomalos.is_empty(),
        if anomalos.is_empty() {
            "Proveedores, clientes, bancos, 555, 551 e IVA sin saldos raros.".into()
        } else {
            format!("Revisa (abajo, «Comprobaciones»): {}.", anomalos.join("; "))
        },
    );
    if !closed {
        let sin_deterioro = crate::ajustes_cierre::resumen(v, year, false)?
            .deteriorables
            .len();
        push(
            "ajustes",
            "Ajustes de cierre",
            sin_deterioro == 0,
            if sin_deterioro == 0 {
                "Revisa abajo las periodificaciones, las facturas pendientes de recibir o de \
                 emitir y las provisiones."
                    .into()
            } else {
                format!(
                    "{sin_deterioro} cobros vencidos hace más de seis meses sin deterioro \
                     (abajo, «Deterioro de clientes»); revisa también las periodificaciones y \
                     las facturas pendientes."
                )
            },
        );
    }
    if company {
        let posted = entry_of(conn, "ImpuestoSociedades", i64::from(year))?.is_some();
        // Gasto por IS contabilizado a mano (sin contar la regularización, que lo salda).
        let manual = count(
            conn,
            "SELECT COALESCE(SUM(l.debit - l.credit), 0) FROM journal_line l
               JOIN journal_entry e ON e.id = l.entry_id
              WHERE e.fiscal_year = ?1 AND e.origin_kind NOT IN ('Cierre', 'Regularizacion')
                AND l.account LIKE '630%'",
            [year],
        )? != 0;
        let ct = corporate_tax(v, year).ok();
        // El asiento de TaxPain ha de seguir cuadrando con el 200 (si éste cambia, se rehace).
        let stale = posted && ct.as_ref().is_some_and(|t| t.stale);
        // Sin cuota ni pagos a cuenta (p. ej. pérdidas) no hay asiento que hacer.
        let nothing = !posted && !manual && ct.as_ref().is_some_and(|t| t.lines.len() < 2);
        let ok = (posted && !stale) || (!posted && manual) || nothing;
        push(
            "is",
            "Impuesto sobre Sociedades contabilizado",
            ok,
            if stale {
                "El borrador del 200 ha cambiado desde que se contabilizó: deshaz el asiento del \
                 IS y vuelve a contabilizarlo."
                    .into()
            } else if nothing {
                "Sin cuota ni pagos a cuenta: no hay asiento del IS que hacer.".into()
            } else if ok {
                "Hay gasto por IS (630) en el ejercicio.".into()
            } else {
                "Contabilízalo antes de cerrar (abajo, desde el borrador del 200).".into()
            },
        );
    }
    // Saldos en moneda extranjera al tipo de cierre (NRV 11.ª), sólo si los hay.
    if let Some((ok, detail)) = crate::divisa::estado_cierre(v, year)? {
        push(
            "divisa",
            "Saldos en moneda extranjera valorados",
            ok,
            detail,
        );
    }
    // Cartera de valores a valor razonable o con su deterioro (NRV 9.ª), sólo si la hay.
    if let Some((ok, detail)) = crate::valores::contabilidad::estado_cierre(v, year)? {
        push(
            "valores",
            "Cartera de valores valorada al cierre",
            ok,
            detail,
        );
    }
    push(
        "cierre",
        "Ejercicio cerrado",
        closed,
        if closed {
            "Regularizado, cerrado y abierto el siguiente.".into()
        } else {
            "Cuando todo lo anterior esté, cierra el ejercicio.".into()
        },
    );
    if lsc && closed {
        let done = entry_of(conn, "AplicacionResultado", i64::from(year))?.is_some();
        push(
            "aplicacion",
            "Aplicación del resultado",
            done,
            if done {
                "Contabilizada.".into()
            } else {
                "Cuando la junta apruebe las cuentas, contabiliza la aplicación (abajo).".into()
            },
        );
    }

    let after =
        |months: u32, extra_days: i64| after_close(fy.end, months) + Duration::days(extra_days);
    let mut deadlines = vec![Deadline {
        label: "Legalizar los libros en el Registro Mercantil (4 meses)".into(),
        date: after(4, 0),
    }];
    if company {
        deadlines.push(Deadline {
            label: "Junta que aprueba las cuentas y la aplicación del resultado (6 meses)".into(),
            date: after(6, 0),
        });
        deadlines.push(Deadline {
            label: "Modelo 200 (25 días naturales tras los 6 meses)".into(),
            date: after(6, 25),
        });
        deadlines.push(Deadline {
            label: "Depósito de las cuentas (1 mes tras la junta; como tarde)".into(),
            date: deposit_deadline(fy.end),
        });
    }
    Ok(Checklist {
        year,
        start: fy.start,
        end: fy.end,
        closed,
        company,
        lsc,
        items,
        deadlines,
    })
}

/// Saldos simultáneos de la 4700 (deudora) y la 4750 (acreedora) en un ejercicio con
/// liquidaciones del IVA, y el ajuste que los salda.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VatBalances {
    pub year: i32,
    /// Fecha del ajuste propuesto: la de la última liquidación del IVA del ejercicio.
    pub date: Date,
    /// Saldo deudor de la 4700 (sin el asiento de cierre ni el exceso que deja la última
    /// liquidación, que se compensa en el periodo siguiente).
    pub debtor: Money,
    /// Saldo acreedor de la 4750 que no corresponde a liquidaciones pendientes de pago.
    pub creditor: Money,
    /// Importe del ajuste: el menor de los dos.
    pub amount: Money,
    pub lines: Vec<ProposedLine>,
}

/// Saldo deudor neto de las cuentas que empiezan por `prefix` en el ejercicio (con la apertura,
/// sin el cierre), como el balance de cierre.
fn year_balance(conn: &Connection, year: i32, prefix: &str) -> Result<Money> {
    let cents: i64 = conn.query_row(
        "SELECT COALESCE(SUM(l.debit - l.credit), 0) FROM journal_line l
           JOIN journal_entry e ON e.id = l.entry_id
          WHERE e.fiscal_year = ?1 AND e.origin_kind <> 'Cierre' AND l.account LIKE ?2 || '%'",
        params![year, prefix],
        |r| r.get(0),
    )?;
    Ok(Money::from_cents(cents))
}

/// Detecta los saldos fantasma que dejaban las liquidaciones del IVA anteriores a la corrección
/// de CONTABILIDAD-02: la 4700 con saldo deudor (exceso de periodos anteriores) y la 4750 con
/// saldo acreedor a la vez, cuando el 303 de un periodo compensó ese exceso y el pago se cargó
/// sólo por lo ingresado. La 4750 de una liquidación aún no pagada no cuenta (es deuda real).
/// Devuelve el ajuste propuesto (4750 al debe contra la 4700 al haber por el menor de los dos
/// saldos; PGC PYMES, cuenta 4700 b1) o `None` si no hay nada que ajustar. Sólo en ejercicios
/// abiertos con alguna liquidación contabilizada.
pub fn vat_balances(v: &Vault, year: i32) -> Result<Option<VatBalances>> {
    let conn = v.connection();
    let fy = accounting::fiscal_year(v, year)?;
    let closed = count(
        conn,
        "SELECT count(*) FROM fiscal_year WHERE year = ?1 AND closed = 1",
        [year],
    )? > 0;
    if closed {
        return Ok(None);
    }
    let debtor = year_balance(conn, year, "4700")?;
    let creditor = -year_balance(conn, year, "4750")?;
    if debtor.cents() <= 0 || creditor.cents() <= 0 {
        return Ok(None);
    }
    // Liquidaciones del ejercicio sin pago registrado: su 4750 es lo que se va a pagar.
    let mut st = conn.prepare(
        "SELECT e.origin_id, COALESCE(SUM(l.credit - l.debit), 0) FROM journal_entry e
           JOIN journal_line l ON l.entry_id = e.id AND l.account LIKE '4750%'
          WHERE e.fiscal_year = ?1 AND e.origin_kind = 'LiquidacionIva'
          GROUP BY e.origin_id",
    )?;
    let settled: Vec<(i64, i64)> = st
        .query_map([year], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    if settled.is_empty() {
        return Ok(None);
    }
    let mut unpaid = Money::ZERO;
    for (key, cents) in settled {
        let (y, q) = (key / 100, key % 100);
        let paid = count(
            conn,
            "SELECT count(*) FROM tax_payment WHERE model = '303' AND year = ?1 AND period = ?2",
            params![y, format!("{q}T")],
        )? > 0;
        if !paid {
            unpaid += Money::from_cents(cents);
        }
    }
    // La 4750 de la apertura es el 303 del último periodo del año anterior: si no tiene pago
    // registrado, es deuda de verdad (falta registrar el pago), no un saldo fantasma.
    let opening = opening_unpaid_vat(conn, year)?;
    let creditor = creditor - unpaid - opening;
    // La 4700 que deja la última liquidación contabilizada aún no ha podido compensarse: se
    // compensa en el periodo siguiente (o el año que viene, si es el último del ejercicio).
    let last_excess: i64 = conn.query_row(
        "SELECT COALESCE(SUM(l.debit - l.credit), 0) FROM journal_entry e
           JOIN journal_line l ON l.entry_id = e.id AND l.account LIKE '4700%'
          WHERE e.fiscal_year = ?1 AND e.origin_kind = 'LiquidacionIva'
            AND e.origin_id = (SELECT max(origin_id) FROM journal_entry
                                WHERE fiscal_year = ?1 AND origin_kind = 'LiquidacionIva')",
        [year],
        |r| r.get(0),
    )?;
    let debtor = debtor - Money::from_cents(last_excess.max(0));
    // Fecha del ajuste: la del último asiento del ejercicio que mueve la 4700 o la 4750.
    let last: String = conn.query_row(
        "SELECT max(e.date) FROM journal_entry e JOIN journal_line l ON l.entry_id = e.id
          WHERE e.fiscal_year = ?1 AND e.origin_kind NOT IN ('Regularizacion', 'Cierre')
            AND (l.account LIKE '4700%' OR l.account LIKE '4750%')",
        [year],
        |r| r.get(0),
    )?;
    let amount = debtor.min(creditor);
    if amount.cents() <= 0 {
        return Ok(None);
    }
    Ok(Some(VatBalances {
        year,
        date: Date::parse(
            &last,
            time::macros::format_description!("[year]-[month]-[day]"),
        )
        .unwrap_or(fy.end),
        debtor,
        creditor,
        amount,
        lines: vec![
            line(conn, "4750", amount, Money::ZERO)?,
            line(conn, "4700", Money::ZERO, amount)?,
        ],
    }))
}

/// Saldo acreedor de la 4750 en la apertura del ejercicio cuando el 303 del último periodo del
/// año anterior (4T o diciembre) no tiene pago registrado.
fn opening_unpaid_vat(conn: &Connection, year: i32) -> Result<Money> {
    let cents: i64 = conn.query_row(
        "SELECT COALESCE(SUM(l.credit - l.debit), 0) FROM journal_line l
           JOIN journal_entry e ON e.id = l.entry_id
          WHERE e.fiscal_year = ?1 AND e.origin_kind = 'Apertura' AND l.account LIKE '4750%'",
        [year],
        |r| r.get(0),
    )?;
    if cents <= 0 {
        return Ok(Money::ZERO);
    }
    let paid = count(
        conn,
        "SELECT count(*) FROM tax_payment
          WHERE model = '303' AND year = ?1 AND period IN ('4T', '12')",
        [year - 1],
    )? > 0;
    Ok(if paid {
        Money::ZERO
    } else {
        Money::from_cents(cents)
    })
}

/// Contabiliza el ajuste que propone [`vat_balances`] como asiento manual (se puede borrar desde
/// el diario), tras confirmación. Devuelve el id del asiento.
pub fn post_vat_balances(
    v: &Vault,
    year: i32,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<i64> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let p = vat_balances(v, year)?.ok_or_else(|| {
        AppError::Other("la 4700 y la 4750 no tienen saldos simultáneos que ajustar".into())
    })?;
    let lines = p
        .lines
        .iter()
        .map(|l| {
            Ok(EntryLine {
                account: code(&l.account)?,
                debit: l.debit,
                credit: l.credit,
                description: None,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let e = JournalEntry {
        date: p.date,
        concept: format!(
            "Ajuste IVA {year}: compensaciones de periodos anteriores aplicadas (4750 contra 4700)"
        ),
        origin: Origin::Manual,
        lines,
    };
    let id = accounting::manual_entry(v, &e, now)?;
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        "closing.vat_balances",
        &year.to_string(),
        &serde_json::json!({ "entry": id, "amount": p.amount.cents() }),
    )?;
    Ok(id)
}

/// Línea de un asiento propuesto (para enseñarlo antes de contabilizarlo).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProposedLine {
    pub account: String,
    pub name: String,
    pub debit: Money,
    pub credit: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct CorporateTax {
    pub year: i32,
    pub date: Date,
    /// Fin del plazo del 200 (para pagar o pedir la devolución).
    pub due: Date,
    /// Cuota del ejercicio [00592], retenciones [01766], pagos fraccionados y resultado [00611].
    pub tax: Money,
    pub withholdings: Money,
    pub installments: Money,
    pub result: Money,
    pub lines: Vec<ProposedLine>,
    /// Asiento ya contabilizado (para deshacerlo).
    pub entry_id: Option<i64>,
    /// El asiento contabilizado ya no coincide con el borrador del 200.
    pub stale: bool,
    pub warnings: Vec<String>,
}

fn box_of(d: &crate::models::Draft, b: u16) -> Money {
    d.boxes
        .iter()
        .find(|(k, _)| *k == b)
        .map_or(Money::ZERO, |(_, m)| *m)
}

fn line(conn: &Connection, account: &str, debit: Money, credit: Money) -> Result<ProposedLine> {
    Ok(ProposedLine {
        name: accounting::account_name(conn, account)?.unwrap_or_default(),
        account: account.into(),
        debit,
        credit,
    })
}

/// Asiento del IS que saldría del borrador del 200 de `year`.
pub fn corporate_tax(v: &Vault, year: i32) -> Result<CorporateTax> {
    let letter = entity_letter(v)?;
    if !pays_corporate_tax(letter) {
        return Err(AppError::Other(
            "el Impuesto sobre Sociedades es de las sociedades (una comunidad de bienes o de \
             propietarios está en atribución de rentas)"
                .into(),
        ));
    }
    let conn = v.connection();
    let fy = accounting::fiscal_year(v, year)?;
    let entry_id = entry_of(conn, "ImpuestoSociedades", i64::from(year))?;
    let draft = crate::models::draft(v, "200", year, 0)?;
    let tax = box_of(&draft, 592);
    let withholdings = box_of(&draft, 1766);
    let installments = box_of(&draft, 601) + box_of(&draft, 603) + box_of(&draft, 605);
    let result = box_of(&draft, 611);
    let mut lines = Vec::new();
    if tax != Money::ZERO {
        lines.push(line(conn, "6300", tax, Money::ZERO)?);
    }
    let paid = withholdings + installments;
    if paid != Money::ZERO {
        lines.push(line(conn, "473", Money::ZERO, paid)?);
    }
    if result.cents() > 0 {
        lines.push(line(conn, "4752", Money::ZERO, result)?);
    } else if result.cents() < 0 {
        lines.push(line(conn, "4709", -result, Money::ZERO)?);
    }
    let mut warnings = draft.warnings.clone();
    if letter == Some('J') {
        warnings.push(
            "Una sociedad civil sólo paga el Impuesto sobre Sociedades si tiene objeto mercantil; \
             si no, está en atribución de rentas (modelo 184) y no hay asiento del IS."
                .into(),
        );
    }
    let stale = match entry_id {
        Some(id) => posted_lines(conn, id)? != comparable(&lines),
        None => false,
    };
    if stale {
        warnings.push(
            "El borrador del 200 ha cambiado desde que se contabilizó el IS: deshaz el asiento y \
             vuelve a contabilizarlo."
                .into(),
        );
    }
    let in_473 = balance(conn, year, "473")?;
    if entry_id.is_none() && in_473 != paid {
        warnings.push(format!(
            "La 473 tiene un saldo de {in_473} y el 200 cuenta {paid} en retenciones y pagos \
             fraccionados: revisa los pagos del 202 y las retenciones antes de contabilizar."
        ));
    }
    Ok(CorporateTax {
        year,
        date: fy.end,
        due: corporate_tax_deadline(fy.end),
        tax,
        withholdings,
        installments,
        result,
        lines,
        entry_id,
        stale,
        warnings,
    })
}

/// (cuenta, debe, haber) ordenado, para comparar asientos.
fn comparable(lines: &[ProposedLine]) -> Vec<(String, i64, i64)> {
    let mut out: Vec<_> = lines
        .iter()
        .map(|l| (l.account.clone(), l.debit.cents(), l.credit.cents()))
        .collect();
    out.sort();
    out
}

fn posted_lines(conn: &Connection, entry: i64) -> Result<Vec<(String, i64, i64)>> {
    let mut st = conn.prepare(
        "SELECT account, debit, credit FROM journal_line WHERE entry_id = ?1 ORDER BY account",
    )?;
    let rows = st.query_map([entry], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
    let mut out = rows.collect::<std::result::Result<Vec<(String, i64, i64)>, _>>()?;
    out.sort();
    Ok(out)
}

/// Contabiliza el IS de `year` con la fecha de cierre (con confirmación).
pub fn post_corporate_tax(
    v: &Vault,
    year: i32,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<i64> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let p = corporate_tax(v, year)?;
    if p.entry_id.is_some() {
        return Err(AppError::Other(
            "el IS de ese ejercicio ya está contabilizado: deshazlo para recalcularlo".into(),
        ));
    }
    if p.lines.len() < 2 {
        return Err(AppError::Other(
            "no hay impuesto que contabilizar (cuota y pagos a cuenta a cero)".into(),
        ));
    }
    let lines = p
        .lines
        .iter()
        .map(|l| {
            Ok(EntryLine {
                account: code(&l.account)?,
                debit: l.debit,
                credit: l.credit,
                description: None,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let e = JournalEntry {
        date: p.date,
        concept: format!("Impuesto sobre Sociedades {year}"),
        origin: Origin::ImpuestoSociedades(i64::from(year)),
        lines,
    };
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let id = record_entry(&tx, &e, now)?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "closing.corporate_tax",
        &year.to_string(),
        &serde_json::json!({ "tax": p.tax.cents(), "result": p.result.cents() }),
    )?;
    tx.commit()?;
    Ok(id)
}

/// Quita el asiento de `kind` de `year` (IS o aplicación) si el ejercicio está abierto.
fn undo(v: &Vault, kind: &str, year: i32, action: &str, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let id = entry_of(conn, kind, i64::from(year))?.ok_or(AppError::NotFound)?;
    let tx = conn.unchecked_transaction()?;
    // Los disparadores impiden borrar en un ejercicio cerrado: el error lo explica.
    tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [id])
        .map_err(|_| {
            AppError::Other("el ejercicio de ese asiento está cerrado: reábrelo".into())
        })?;
    tx.execute("DELETE FROM journal_entry WHERE id = ?1", [id])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        action,
        &year.to_string(),
        &serde_json::json!({ "entry": id }),
    )?;
    tx.commit()?;
    Ok(())
}

pub fn undo_corporate_tax(
    v: &Vault,
    year: i32,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    undo(
        v,
        "ImpuestoSociedades",
        year,
        "closing.corporate_tax_undo",
        now,
    )
}

/// Cuenta de la reserva de capitalización (art. 25.1.b LIS: «con absoluta separación y título
/// apropiado»). El PGC no le da número: una subcuenta libre de la 114 (reservas especiales).
pub const CAPITALIZATION_ACCOUNT: &str = "1149";

/// Propuesta de aplicación del resultado de `year`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AllocationProposal {
    pub year: i32,
    /// Resultado del ejercicio (positivo: beneficio): el de su regularización.
    pub result: Money,
    pub capital: Money,
    /// Reserva legal antes de aplicar.
    pub legal_reserve: Money,
    /// Mínimo que ha de ir a la reserva legal (art. 274 LSC).
    pub legal_minimum: Money,
    /// Pérdidas de ejercicios anteriores sin compensar (121).
    pub losses: Money,
    /// Patrimonio neto a efectos de reparto, con el resultado (fondos propios menos el
    /// dividendo a cuenta; sin subvenciones ni ajustes por cambios de valor).
    pub equity: Money,
    /// Pérdidas que hay que compensar antes de la reserva legal (art. 28.3 RICAC).
    pub required_compensation: Money,
    /// Dividendos máximos (art. 273.2 LSC).
    pub max_dividends: Money,
    /// Mínimo a la reserva de capitalización (la reducción [01032] del 200).
    pub capitalization_minimum: Money,
    /// Dividendo a cuenta ya repartido (557): el dividendo ha de ser al menos esto y lo salda.
    pub interim_dividend: Money,
    /// Reparto propuesto: reserva legal, compensación de pérdidas, voluntarias, dividendos,
    /// remanente y reserva de capitalización.
    pub suggested: Allocation,
    /// Primer día en que puede ir (el siguiente al cierre) y plazo de la junta.
    pub earliest: Date,
    pub meeting_deadline: Date,
    pub entry_id: Option<i64>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Allocation {
    pub legal: Money,
    pub losses: Money,
    pub voluntary: Money,
    pub dividends: Money,
    pub remanente: Money,
    /// Reserva de capitalización (art. 25 LIS).
    #[serde(default)]
    pub capitalization: Money,
}

impl Allocation {
    fn total(self) -> Money {
        self.legal
            + self.losses
            + self.voluntary
            + self.dividends
            + self.remanente
            + self.capitalization
    }
}

/// Resultado de `year`: lo que su regularización lleva a la 129 (sin los resultados de años
/// anteriores que sigan en la 129 sin aplicar).
fn year_result(conn: &Connection, year: i32) -> Result<Money> {
    let cents: i64 = conn.query_row(
        "SELECT COALESCE(SUM(l.credit - l.debit), 0) FROM journal_line l
           JOIN journal_entry e ON e.id = l.entry_id
          WHERE e.fiscal_year = ?1 AND e.origin_kind = 'Regularizacion'
            AND l.account LIKE '129%'",
        [year],
        |r| r.get(0),
    )?;
    Ok(Money::from_cents(cents))
}

pub fn allocation(v: &Vault, year: i32) -> Result<AllocationProposal> {
    if !capital_company(entity_letter(v)?) {
        return Err(AppError::Other(
            "la aplicación del resultado según la Ley de Sociedades de Capital es de las SA y SL"
                .into(),
        ));
    }
    let conn = v.connection();
    let fy = accounting::fiscal_year(v, year)?;
    let closed = count(
        conn,
        "SELECT count(*) FROM fiscal_year WHERE year = ?1 AND closed = 1",
        [year],
    )? > 0;
    if !closed {
        return Err(AppError::Other(format!(
            "cierra antes el ejercicio {year}: el resultado se aplica con las cuentas cerradas"
        )));
    }
    let mut warnings = Vec::new();
    let result = year_result(conn, year)?;
    // Lo que queda en la 129 de ejercicios anteriores (no se aplicó): no entra en este reparto.
    let carried = -balance(conn, year, "129")? - result;
    if carried != Money::ZERO {
        warnings.push(format!(
            "La 129 trae {carried} de ejercicios anteriores sin aplicar: este reparto es sólo el \
             resultado de {year}. Contabiliza la aplicación pendiente en su ejercicio (reábrelo si \
             hace falta)."
        ));
    }
    let capital = -balance(conn, year, "100")?;
    let legal_reserve = -balance(conn, year, "112")?;
    let losses = balance(conn, year, "121")?.max(Money::ZERO);
    // Patrimonio neto a efectos del art. 273.2 LSC (art. 28.2 RICAC): grupos 10, 11 y 12 (con el
    // resultado) menos el dividendo a cuenta (557); el grupo 13 no cuenta.
    let interim_dividend = balance(conn, year, "557")?.max(Money::ZERO);
    let equity =
        -(balance(conn, year, "10")? + balance(conn, year, "11")? + balance(conn, year, "12")?)
            - interim_dividend;
    let equity_before = equity - result;
    let target = capital.apply_rate(Rate::percent(20));
    let positive = result.cents() > 0;
    // Art. 28.3 RICAC: con pérdidas que dejan el patrimonio neto por debajo del capital, el
    // beneficio las compensa (hasta llegar al capital) antes de la reserva legal.
    let required_compensation = if positive && equity_before < capital {
        result.min(losses).min(capital - equity_before)
    } else {
        Money::ZERO
    };
    let legal_minimum = if positive {
        result
            .apply_rate(Rate::percent(10))
            .min((target - legal_reserve).max(Money::ZERO))
            .min(result - required_compensation)
    } else {
        Money::ZERO
    };
    // Dividendos en total (los a cuenta ya restan del patrimonio neto: se suman de nuevo).
    let max_dividends = if positive {
        (equity + interim_dividend - capital)
            .max(Money::ZERO)
            .min(result)
    } else {
        Money::ZERO
    };
    if interim_dividend.cents() > 0 && interim_dividend > max_dividends {
        warnings.push(format!(
            "Se repartieron {interim_dividend} a cuenta (557) y el beneficio sólo permite \
             {max_dividends} de dividendos: el exceso se reclasifica a reservas o se reclama a los \
             socios (arts. 277 y 278 LSC, art. 28.5 de la Resolución del ICAC)."
        ));
    }
    let capitalization_minimum = if positive {
        crate::models::draft(v, "200", year, 0)
            .ok()
            .map_or(Money::ZERO, |d| box_of(&d, 1032))
            .max(Money::ZERO)
            .min(result - required_compensation - legal_minimum)
    } else {
        Money::ZERO
    };
    let suggested = if positive {
        let rest = result - required_compensation - legal_minimum - capitalization_minimum;
        let comp = rest.min(losses - required_compensation);
        let dividends = interim_dividend.min(rest - comp);
        Allocation {
            legal: legal_minimum,
            losses: required_compensation + comp,
            voluntary: rest - comp - dividends,
            dividends,
            remanente: Money::ZERO,
            capitalization: capitalization_minimum,
        }
    } else {
        Allocation::default()
    };
    Ok(AllocationProposal {
        year,
        result,
        capital,
        legal_reserve,
        legal_minimum,
        losses,
        equity,
        required_compensation,
        max_dividends,
        capitalization_minimum,
        interim_dividend,
        suggested,
        earliest: fy.end + Duration::days(1),
        meeting_deadline: after_close(fy.end, 6),
        entry_id: entry_of(conn, "AplicacionResultado", i64::from(year))?,
        warnings,
    })
}

/// Comprueba un reparto del beneficio frente a la propuesta (límites legales).
fn check_allocation(conn: &Connection, p: &AllocationProposal, a: &Allocation) -> Result<()> {
    if a.total() != p.result {
        return Err(AppError::Other(format!(
            "el reparto ({}) ha de sumar el beneficio ({})",
            a.total(),
            p.result
        )));
    }
    if [
        a.legal,
        a.losses,
        a.voluntary,
        a.dividends,
        a.remanente,
        a.capitalization,
    ]
    .iter()
    .any(|m| m.cents() < 0)
    {
        return Err(AppError::Other(
            "los importes del reparto no pueden ser negativos".into(),
        ));
    }
    if a.losses > p.losses {
        return Err(AppError::Other(format!(
            "sólo hay {} de pérdidas por compensar",
            p.losses
        )));
    }
    if a.losses < p.required_compensation {
        return Err(AppError::Other(format!(
            "las pérdidas de ejercicios anteriores dejan el patrimonio neto por debajo del \
             capital: el beneficio ha de compensar al menos {} antes de la reserva legal (art. \
             28.3 de la Resolución del ICAC de 5-3-2019)",
            p.required_compensation
        )));
    }
    if a.legal < p.legal_minimum {
        return Err(AppError::Other(format!(
            "a la reserva legal va como mínimo {} (10 % del beneficio hasta el 20 % del \
             capital, art. 274 LSC)",
            p.legal_minimum
        )));
    }
    if a.dividends > p.max_dividends {
        return Err(AppError::Other(format!(
            "con ese dividendo el patrimonio neto quedaría por debajo del capital: como mucho {} \
             (art. 273.2 LSC)",
            p.max_dividends
        )));
    }
    if a.dividends < p.interim_dividend.min(p.max_dividends) {
        return Err(AppError::Other(format!(
            "ya se repartieron {} a cuenta (557): el dividendo ha de ser al menos eso",
            p.interim_dividend
        )));
    }
    if a.dividends.cents() > 0 {
        // Art. 273.3 LSC: reservas libres al menos iguales a la I+D activada.
        let rd = (balance(conn, p.year, "200")?
            + balance(conn, p.year, "201")?
            + balance(conn, p.year, "2800")?
            + balance(conn, p.year, "2801")?)
        .max(Money::ZERO);
        let free = -(balance(conn, p.year, "113")? + balance(conn, p.year, "120")?)
            + a.voluntary
            + a.remanente;
        if rd > free {
            return Err(AppError::Other(format!(
                "hay {rd} de investigación y desarrollo en el activo y las reservas libres son \
                 {free}: no se pueden repartir dividendos hasta que las cubran (art. 273.3 LSC)"
            )));
        }
    }
    if a.capitalization < p.capitalization_minimum {
        return Err(AppError::Other(format!(
            "el 200 aplica la reducción por reserva de capitalización: hay que dotar al menos {} \
             (art. 25.1.b LIS), o quitar la reducción del 200",
            p.capitalization_minimum
        )));
    }
    Ok(())
}

/// Contabiliza la aplicación del resultado de `year` el día de la junta (`date`, en el
/// ejercicio siguiente). Con pérdidas, van a la 121 (sin reparto).
pub fn post_allocation(
    v: &Vault,
    year: i32,
    date: Date,
    a: &Allocation,
    now: OffsetDateTime,
    confirmed: bool,
) -> Result<i64> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let p = allocation(v, year)?;
    if p.entry_id.is_some() {
        return Err(AppError::Other(
            "la aplicación de ese ejercicio ya está contabilizada".into(),
        ));
    }
    if date < p.earliest {
        return Err(AppError::Other(
            "la junta es después del cierre: pon su fecha".into(),
        ));
    }
    let conn = v.connection();
    let r129 = code("129")?;
    let lines = if p.result.cents() > 0 {
        check_allocation(conn, &p, a)?;
        let mut lines = vec![EntryLine::debit(r129.clone(), p.result)];
        // Lo repartido a cuenta salda la 557; el resto del dividendo queda por pagar (526).
        let interim = p.interim_dividend.min(a.dividends);
        for (acc, m) in [
            ("112", a.legal),
            ("121", a.losses),
            (CAPITALIZATION_ACCOUNT, a.capitalization),
            ("113", a.voluntary),
            ("557", interim),
            ("526", a.dividends - interim),
            ("120", a.remanente),
        ] {
            if m.cents() > 0 {
                lines.push(EntryLine::credit(code(acc)?, m));
            }
        }
        lines
    } else if p.result.cents() < 0 {
        // Pérdidas: a resultados negativos de ejercicios anteriores.
        vec![
            EntryLine::debit(code("121")?, -p.result),
            EntryLine::credit(r129, -p.result),
        ]
    } else {
        return Err(AppError::Other(
            "el resultado es cero: no hay nada que aplicar".into(),
        ));
    };
    let e = JournalEntry {
        date,
        concept: format!("Aplicación del resultado {year}"),
        origin: Origin::AplicacionResultado(i64::from(year)),
        lines,
    };
    let tx = conn.unchecked_transaction()?;
    if a.capitalization.cents() > 0 {
        tx.execute(
            "INSERT INTO account(code, name, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(code) DO NOTHING",
            params![
                CAPITALIZATION_ACCOUNT,
                "Reserva de capitalización (art. 25 LIS)",
                now_str(now)
            ],
        )?;
    }
    let id = record_entry(&tx, &e, now)?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "closing.allocation",
        &year.to_string(),
        &serde_json::json!({
            "result": p.result.cents(),
            "legal": a.legal.cents(),
            "dividends": a.dividends.cents(),
            "capitalization": a.capitalization.cents(),
        }),
    )?;
    tx.commit()?;
    Ok(id)
}

pub fn undo_allocation(v: &Vault, year: i32, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    // Los dividendos ya pagados con cargo a este reparto (dividend_payment.allocation_year) salen
    // de su 526: antes hay que deshacer el pago. Los pagos de otros repartos no bloquean. Un
    // asiento de dividendos sin reparto anotado (pagos anteriores a esa columna, o hechos a
    // mano) cuenta si es posterior a la fecha de la aplicación, sea cual sea su ejercicio: la
    // exigibilidad puede caer en el ejercicio siguiente al de la junta.
    let conn = v.connection();
    let alloc =
        entry_of(conn, "AplicacionResultado", i64::from(year))?.ok_or(AppError::NotFound)?;
    let paid = count(
        conn,
        "SELECT count(*) FROM journal_entry e WHERE e.origin_kind = 'Dividendo'
           AND (EXISTS (SELECT 1 FROM dividend_payment p
                         WHERE p.entry_id = e.id AND p.allocation_year = ?2)
                OR (e.date >= (SELECT date FROM journal_entry WHERE id = ?1)
                    AND NOT EXISTS (SELECT 1 FROM dividend_payment p
                                     WHERE p.entry_id = e.id AND p.allocation_year IS NOT NULL)))",
        [alloc, i64::from(year)],
    )?;
    if paid > 0 {
        return Err(AppError::Other(
            "ya hay dividendos pagados con cargo a este reparto: deshaz antes su pago (Socios y \
             dividendos)"
                .into(),
        ));
    }
    let agreed = count(
        conn,
        "SELECT count(*) FROM dividend_agreement WHERE kind = 'resultado' AND allocation_year = ?1",
        [i64::from(year)],
    )?;
    if agreed > 0 {
        return Err(AppError::Other(
            "el dividendo de este reparto tiene registrado su acuerdo con el acta: deshaz antes \
             el acuerdo (Socios y dividendos)"
                .into(),
        ));
    }
    undo(
        v,
        "AplicacionResultado",
        year,
        "closing.allocation_undo",
        now,
    )
}
