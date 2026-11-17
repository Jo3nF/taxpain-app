//! Contratos de leasing: alta (bien en el inmovilizado contra la deuda), cuadro de cuotas y
//! contabilización de las cuotas vencidas. Ver `taxpain_core::accounting::leasing`.

use serde::Serialize;
use taxpain_core::accounting::AccountCode;
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::depreciation::FixedAsset;
use taxpain_core::accounting::leasing::{Installment, Leasing, Lessor, schedule, start_entry};
use taxpain_core::money::{Money, Rate};
use taxpain_core::nif::Nif;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};

use crate::accounting;
use crate::error::{AppError, Result};
use crate::received::{self, Nature, Reviewed, VatRegime};

fn other(e: impl ToString) -> AppError {
    AppError::Other(e.to_string())
}

fn now_str(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LeasingView {
    pub leasing: Leasing,
    pub installments: Vec<InstallmentView>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InstallmentView {
    #[serde(flatten)]
    pub q: Installment,
    pub posted: bool,
    /// Contabilizada con el sistema antiguo (asiento directo contra la 572, sin factura del
    /// arrendador): su IVA no está en el 303 ni en el libro registro. Ver [`regularize`].
    pub legacy: bool,
    /// Por qué una cuota antigua no se puede regularizar desde aquí (`None` si se puede).
    pub blocked: Option<String>,
    /// La factura de la cuota, aún sin pagar, vence en otra fecha que la de la cuota (las
    /// registradas antes del arreglo de octubre de 2026 vencían a +30 días, con las condiciones
    /// de pago del arrendador). Ver [`fix_due_dates`].
    pub due_mismatch: bool,
}

/// Resultado de [`post_due`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Posted {
    /// Cuotas registradas como factura nueva del arrendador.
    pub posted: usize,
    /// Cuotas enlazadas con la factura real del arrendador que ya estaba en Recibidas (en vez
    /// de registrar otra), con lo que se ha hecho con cada una.
    pub linked: Vec<String>,
}

/// Da de alta un leasing: asiento del bien contra la deuda y elemento del inmovilizado con el
/// coeficiente `depreciation` para amortizarlo. Devuelve su id.
pub fn create(v: &Vault, l: &Leasing, depreciation: Rate, now: OffsetDateTime) -> Result<i64> {
    if l.principal.cents() <= 0 || l.months == 0 || l.months > 240 {
        return Err(other("revisa el importe y el número de cuotas (1 a 240)"));
    }
    if l.purchase_option.is_negative() || l.purchase_option >= l.principal {
        return Err(other(
            "la opción de compra ha de ser menor que el valor al contado",
        ));
    }
    if !l.asset_account.as_str().starts_with('2') || !l.debt_account.as_str().starts_with("52") {
        return Err(other(
            "el bien va en el grupo 2 (218, 213…) y la deuda en la 524",
        ));
    }
    if let Some(lessor) = &l.lessor {
        check_lessor(lessor)?;
    }
    let suffix = &l.asset_account.as_str()[1..];
    let asset = FixedAsset {
        id: 0,
        name: format!("{} (leasing)", l.name),
        account: l.asset_account.clone(),
        depreciation_account: AccountCode::new(&format!("28{suffix}")).map_err(other)?,
        expense_account: acc(if l.asset_account.as_str().starts_with("20") {
            "680"
        } else {
            "681"
        }),
        acquired_on: l.start,
        cost: l.principal,
        residual: Money::ZERO,
        rate: depreciation,
        disposed_on: None,
        method: Default::default(),
        tax_rate: None,
    };
    let asset_id = accounting::add_fixed_asset(v, &asset, now)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO leasing(data, asset_id, created_at) VALUES ('{}', ?1, ?2)",
        params![asset_id, now_str(now)],
    )?;
    let id = tx.last_insert_rowid();
    let l = Leasing { id, ..l.clone() };
    let entry = accounting::record_entry(&tx, &start_entry(&l), now)?;
    tx.execute(
        "UPDATE leasing SET data = ?1, start_entry = ?2 WHERE id = ?3",
        params![serde_json::to_string(&l).map_err(other)?, entry, id],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "leasing.create",
        &id.to_string(),
        &serde_json::json!({ "principal": l.principal.cents(), "months": l.months }),
    )?;
    tx.commit()?;
    Ok(id)
}

pub fn list(v: &Vault) -> Result<Vec<LeasingView>> {
    let conn = v.connection();
    let mut st = conn.prepare("SELECT id, data FROM leasing ORDER BY id")?;
    let rows: Vec<(i64, String)> = st
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let mut out = Vec::new();
    for (id, data) in rows {
        let l: Leasing = serde_json::from_str(&data).map_err(other)?;
        // Cuota registrada → (asiento si es del sistema antiguo —origen «Leasing»—, factura
        // recibida si es de los nuevos).
        let mut posted = std::collections::BTreeMap::new();
        let mut q = conn.prepare(
            "SELECT li.n, CASE WHEN e.origin_kind = 'Leasing' THEN e.id END,
                    CASE WHEN e.origin_kind = 'FacturaRecibida' THEN e.origin_id END
             FROM leasing_installment li LEFT JOIN journal_entry e ON e.id = li.entry_id
             WHERE li.leasing_id = ?1",
        )?;
        for row in q.query_map([id], |r| {
            Ok((
                r.get::<_, u16>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<i64>>(2)?,
            ))
        })? {
            let (n, legacy, invoice) = row?;
            posted.insert(n, (legacy, invoice));
        }
        let mut installments = Vec::new();
        for q in schedule(&l) {
            let (legacy, invoice) = posted.get(&q.n).copied().unwrap_or((None, None));
            let blocked = match legacy {
                Some(entry) => legacy_blocker(conn, &l, &q, entry)?,
                None => None,
            };
            let due_mismatch = match invoice {
                Some(rid) => unpaid_due_elsewhere(conn, rid, q.date)?,
                None => false,
            };
            installments.push(InstallmentView {
                posted: posted.contains_key(&q.n),
                legacy: legacy.is_some(),
                blocked,
                due_mismatch,
                q,
            });
        }
        out.push(LeasingView {
            installments,
            leasing: l,
        });
    }
    Ok(out)
}

/// Comprueba los datos del arrendador.
fn check_lessor(l: &Lessor) -> Result<()> {
    Nif::parse(&l.nif).map_err(|_| other("el NIF del arrendador no es válido"))?;
    if l.name.trim().is_empty() {
        return Err(other("falta el nombre del arrendador"));
    }
    if l.deductible_pct > 100 {
        return Err(other("el porcentaje deducible del IVA va de 0 a 100"));
    }
    Ok(())
}

/// Pone el arrendador de un contrato (los dados de alta antes de registrar sus cuotas como
/// facturas no lo tienen).
pub fn set_lessor(v: &Vault, id: i64, lessor: &Lessor, now: OffsetDateTime) -> Result<()> {
    check_lessor(lessor)?;
    let mut l = list(v)?
        .into_iter()
        .find(|x| x.leasing.id == id)
        .ok_or(AppError::NotFound)?
        .leasing;
    l.lessor = Some(Lessor {
        nif: lessor.nif.trim().to_owned(),
        name: lessor.name.trim().to_owned(),
        ..lessor.clone()
    });
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE leasing SET data = ?1 WHERE id = ?2",
        params![serde_json::to_string(&l).map_err(other)?, id],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "leasing.lessor",
        &id.to_string(),
        &serde_json::json!({ "nif": lessor.nif.trim() }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Número con el que se registra la factura de una cuota (único por contrato y cuota).
fn invoice_number(l: &Leasing, n: u16) -> String {
    format!("Leasing {} cuota {n}/{}", l.id, l.months)
}

/// Factura del arrendador por la cuota `q` y el reparto de su base: la parte de capital salda
/// la deuda del leasing (no es gasto ni deuda nueva) y los intereses van a la 662.
fn installment_invoice(
    l: &Leasing,
    lessor: &Lessor,
    q: &Installment,
) -> (Reviewed, [(AccountCode, Money); 2]) {
    let r = Reviewed {
        supplier_nif: lessor.nif.clone(),
        supplier_name: lessor.name.clone(),
        number: invoice_number(l, q.n),
        issue_date: q.date,
        base: q.amount,
        vat: q.vat,
        withholding: Money::ZERO,
        total: q.amount + q.vat,
        deductible_pct: lessor.deductible_pct,
        capital_good: false,
        nature: Nature::General,
        asset_class: None,
        vat_regime: VatRegime::Interior,
        supplier_country: None,
        self_assessed_rate: None,
        intra_goods: false,
        cash_basis: false,
        import_dua: false,
        prorata_use: Default::default(),
    };
    let split = [
        (l.debt_account.clone(), q.capital),
        (acc("662"), q.interest),
    ];
    (r, split)
}

/// Registra cada cuota vencida hasta `until` (y no después de hoy) que falte como factura
/// recibida del arrendador: capital a la deuda (524), intereses a la 662 e IVA a la 472 contra
/// el arrendador (400/410). Así su IVA entra en el 303 y en el libro registro (arts. 92.Uno y
/// 97 LIVA), queda pendiente de pago en la cartera y se paga conciliando el cargo del banco (o
/// desde la cartera), como cualquier otra factura. La cuota es un servicio mientras no se
/// ejerce la opción de compra (art. 8.Dos.5.º LIVA): no va como bien de inversión.
///
/// Si la factura real del arrendador de una cuota ya estaba en Recibidas (registrada antes de
/// pulsar aquí: mismo proveedor y mismo número, o mismo importe con fecha a ±5 días), la cuota
/// se enlaza con ella en vez de registrar otra (ver [`link_invoice`]).
pub fn post_due(v: &Vault, id: i64, until: Date, now: OffsetDateTime) -> Result<Posted> {
    let view = list(v)?
        .into_iter()
        .find(|x| x.leasing.id == id)
        .ok_or(AppError::NotFound)?;
    let l = &view.leasing;
    let Some(lessor) = &l.lessor else {
        return Err(other(
            "indica antes el arrendador del leasing (NIF y nombre): cada cuota se registra como \
             una factura suya",
        ));
    };
    let until = until.min(taxpain_core::spain_time::peninsular_date(now));
    let mut out = Posted::default();
    for q in view
        .installments
        .iter()
        .filter(|q| !q.posted && q.q.date <= until)
    {
        let q = &q.q;
        let conn = v.connection();
        if let Some((rid, number)) = existing_invoice(conn, l, lessor, q)? {
            out.linked.push(link_invoice(v, l, q, rid, &number)?);
            continue;
        }
        let (r, split) = installment_invoice(l, lessor, q);
        let received = received::register_typed_split(v, &r, None, Some(&split), now)?;
        due_on(conn, received, q.date)?;
        let entry: i64 = conn.query_row(
            "SELECT id FROM journal_entry WHERE origin_kind = 'FacturaRecibida' AND origin_id = ?1",
            [received],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT INTO leasing_installment(leasing_id, n, entry_id) VALUES (?1, ?2, ?3)",
            params![id, q.n, entry],
        )?;
        out.posted += 1;
    }
    if out.posted > 0 || !out.linked.is_empty() {
        audit::append(
            v.connection(),
            &now_str(now),
            Actor::User,
            "leasing.installments",
            &id.to_string(),
            &serde_json::json!({
                "posted": out.posted,
                "linked": out.linked.len(),
                "until": until.to_string(),
            }),
        )?;
    }
    Ok(out)
}

/// Margen de fecha para reconocer, al registrar las cuotas, la factura real del arrendador que
/// ya está en Recibidas (más estrecho que [`CLASH_DAYS`]: aquí se enlaza sin preguntar).
const LINK_DAYS: i64 = 5;

/// La factura revisada del arrendador que ya está en Recibidas y es la cuota `q`: la que lleva
/// el número con que la registraría el leasing, o la de su importe (base o total, ±1 €) con
/// fecha a ±`LINK_DAYS` días de la cuota. No cuenta si ya es la factura de otra cuota.
fn existing_invoice(
    conn: &Connection,
    l: &Leasing,
    lessor: &Lessor,
    q: &Installment,
) -> Result<Option<(i64, String)>> {
    let mut st = conn.prepare(
        "SELECT r.id, r.number, r.issue_date, r.base, r.total FROM invoice_received r
         WHERE r.status = 'revisada' AND r.supplier_nif = ?1 AND r.issue_date IS NOT NULL
           AND NOT EXISTS(SELECT 1 FROM leasing_installment li JOIN journal_entry e
                           ON e.id = li.entry_id
                          WHERE e.origin_kind = 'FacturaRecibida' AND e.origin_id = r.id)
         ORDER BY r.id",
    )?;
    let rows: Vec<(i64, String, String, i64, i64)> = st
        .query_map([norm_nif(&lessor.nif)], |r| {
            Ok((
                r.get(0)?,
                r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                r.get(2)?,
                r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                r.get::<_, Option<i64>>(4)?.unwrap_or(0),
            ))
        })?
        .collect::<std::result::Result<_, _>>()?;
    let same = |a: Money, b: Money| (a - b).abs() <= CLASH_AMOUNT;
    for (rid, number, issue, base, total) in rows {
        if number.trim() == invoice_number(l, q.n) {
            return Ok(Some((rid, number)));
        }
        let Ok(issue) = Date::parse(
            &issue,
            time::macros::format_description!("[year]-[month]-[day]"),
        ) else {
            continue;
        };
        let (base, total) = (Money::from_cents(base), Money::from_cents(total));
        let near = (q.date - issue).whole_days().abs() <= LINK_DAYS;
        // La cuota más cercana a esa fecha ha de ser ésta (no la anterior ni la siguiente).
        let nearest = matching_installment(l, issue, base, total).is_some_and(|m| m.n == q.n);
        if near && nearest && (same(base, q.amount) || same(total, q.amount + q.vat)) {
            return Ok(Some((rid, number)));
        }
    }
    Ok(None)
}

/// Enlaza la cuota `q` con la factura real `rid` del arrendador que ya estaba en Recibidas: la
/// cuota queda registrada sin otra factura (su IVA ya está en el libro y en el 303). Si el
/// ejercicio de su asiento está abierto, el gasto que se contabilizó al revisarla (grupo 6) pasa
/// a ser capital a la deuda del leasing e intereses a la 662, como en las cuotas registradas
/// desde aquí; si está cerrado, el asiento no se toca y se dice. Si aún no está pagada, vence el
/// día de la cuota. Devuelve lo hecho, para decírselo al usuario.
fn link_invoice(v: &Vault, l: &Leasing, q: &Installment, rid: i64, number: &str) -> Result<String> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let (entry, year, closed): (i64, i32, bool) = tx.query_row(
        "SELECT e.id, e.fiscal_year, COALESCE(f.closed, 0) FROM journal_entry e
         LEFT JOIN fiscal_year f ON f.year = e.fiscal_year
         WHERE e.origin_kind = 'FacturaRecibida' AND e.origin_id = ?1",
        [rid],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    tx.execute(
        "INSERT INTO leasing_installment(leasing_id, n, entry_id) VALUES (?1, ?2, ?3)",
        params![l.id, q.n, entry],
    )?;
    let what = format!(
        "cuota {}/{}: enlazada con la factura «{number}» que ya estaba en Recibidas",
        q.n, l.months
    );
    let mut notes = Vec::new();
    if closed {
        notes.push(format!(
            "su asiento es del ejercicio {year}, cerrado: el gasto se queda como se contabilizó \
             (capital e intereses sin separar)"
        ));
    } else {
        // Las líneas de gasto del asiento (grupo 6, al debe) suman la base (y el IVA no
        // deducible, si lo hay): se reparten entre la deuda y los intereses.
        let mut st = tx.prepare(
            "SELECT id, debit FROM journal_line WHERE entry_id = ?1 AND debit > 0
             AND account LIKE '6%'",
        )?;
        let lines: Vec<(i64, i64)> = st
            .query_map([entry], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<_, _>>()?;
        drop(st);
        let expense = Money::from_cents(lines.iter().map(|(_, d)| d).sum());
        if lines.is_empty() || expense < q.capital {
            notes.push(
                "su asiento no lleva el gasto de la cuota en el grupo 6: revísalo en el Diario \
                 (capital a la deuda del leasing e intereses a la 662)"
                    .into(),
            );
        } else {
            for (id, _) in &lines {
                tx.execute("DELETE FROM line_tick WHERE line_id = ?1", [id])?;
                tx.execute("DELETE FROM journal_line WHERE id = ?1", [id])?;
            }
            for (account, amount) in [
                (l.debt_account.as_str(), q.capital),
                ("662", expense - q.capital),
            ] {
                if amount.is_zero() {
                    continue;
                }
                tx.execute(
                    "INSERT INTO journal_line(entry_id, account, debit, credit) VALUES (?1, ?2, ?3, 0)",
                    params![entry, account, amount.cents()],
                )?;
            }
            notes.push(format!(
                "su asiento lleva ahora el capital a la {} y los intereses a la 662",
                l.debt_account.as_str()
            ));
        }
    }
    let items = crate::cartera::installments(&tx, "recibida", rid)?;
    if items.iter().all(|i| i.status == "pendiente") {
        due_on(&tx, rid, q.date)?;
        notes.push("vence el día de la cuota".into());
    }
    tx.commit()?;
    Ok(format!("{what} ({})", notes.join("; ")))
}

/// ¿La factura `rid`, aún sin pagar ni conciliar, vence en otra fecha que `date`? Las cuotas
/// registradas antes de octubre de 2026 vencían con las condiciones de pago del arrendador
/// (+30 días), y la cartera y el banco no proponían la cuota que tocaba.
fn unpaid_due_elsewhere(conn: &Connection, rid: i64, date: Date) -> Result<bool> {
    let items = crate::cartera::installments(conn, "recibida", rid)?;
    Ok(items.iter().all(|i| i.status == "pendiente") && items.iter().any(|i| i.due_date != date))
}

/// Pone el vencimiento de cada cuota del contrato, de las aún sin pagar ni conciliar, en su
/// fecha de cuota (la del cuadro). Las pagadas o conciliadas no se tocan. Devuelve cuántas.
pub fn fix_due_dates(v: &Vault, id: i64, now: OffsetDateTime) -> Result<usize> {
    let view = list(v)?
        .into_iter()
        .find(|x| x.leasing.id == id)
        .ok_or(AppError::NotFound)?;
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let mut fixed = Vec::new();
    for iv in view.installments.iter().filter(|q| q.due_mismatch) {
        let rid: i64 = tx.query_row(
            "SELECT e.origin_id FROM leasing_installment li JOIN journal_entry e ON e.id = li.entry_id
             WHERE li.leasing_id = ?1 AND li.n = ?2 AND e.origin_kind = 'FacturaRecibida'",
            params![id, iv.q.n],
            |r| r.get(0),
        )?;
        due_on(&tx, rid, iv.q.date)?;
        fixed.push(iv.q.n);
    }
    if !fixed.is_empty() {
        audit::append(
            &tx,
            &now_str(now),
            Actor::User,
            "leasing.due_dates",
            &id.to_string(),
            &serde_json::json!({ "cuotas": fixed }),
        )?;
    }
    tx.commit()?;
    Ok(fixed.len())
}

/// Borra un contrato dado de alta por error (con confirmación): el contrato, sus cuotas
/// previstas, el asiento de alta (bien contra la 524), los de paso a la 174 que hubiera y el
/// elemento del inmovilizado. Sólo si no tiene ninguna cuota contabilizada, ninguna factura del
/// arrendador por una cuota en Recibidas ni amortización contabilizada, y si sus asientos son
/// de ejercicios abiertos (los cerrados no se tocan: reabrir antes). Queda en la auditoría.
pub fn delete(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<()> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let view = list(v)?
        .into_iter()
        .find(|x| x.leasing.id == id)
        .ok_or(AppError::NotFound)?;
    let l = &view.leasing;
    let posted = view.installments.iter().filter(|q| q.posted).count();
    if posted > 0 {
        let legacy = view.installments.iter().filter(|q| q.legacy).count();
        let mut how = Vec::new();
        if posted > legacy {
            how.push(format!(
                "las {} registradas como facturas de {} no se deshacen desde TaxPain: si el \
                 contrato no existe, esas facturas tampoco deberían (pide al arrendador la \
                 rectificativa o consúltalo con tu gestor)",
                posted - legacy,
                l.lessor
                    .as_ref()
                    .map_or("el arrendador", |x| x.name.as_str())
            ));
        }
        if legacy > 0 {
            how.push(format!(
                "los {legacy} asientos del sistema antiguo se borran desde el Diario"
            ));
        }
        return Err(other(format!(
            "el leasing «{}» tiene {posted} de {} cuotas contabilizadas: un contrato con cuotas \
             no se borra. Deshaz antes las cuotas: {}.",
            l.name,
            l.months,
            how.join("; ")
        )));
    }
    let conn = v.connection();
    if let Some(lessor) = &l.lessor {
        let mut st = conn.prepare(
            "SELECT number, issue_date, base, total FROM invoice_received
             WHERE status = 'revisada' AND supplier_nif = ?1 AND issue_date IS NOT NULL",
        )?;
        let rows: Vec<(String, String, i64, i64)> = st
            .query_map([norm_nif(&lessor.nif)], |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    r.get(1)?,
                    r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                ))
            })?
            .collect::<std::result::Result<_, _>>()?;
        for (number, issue, base, total) in rows {
            let Ok(issue) = Date::parse(
                &issue,
                time::macros::format_description!("[year]-[month]-[day]"),
            ) else {
                continue;
            };
            if let Some(q) =
                matching_installment(l, issue, Money::from_cents(base), Money::from_cents(total))
            {
                return Err(other(format!(
                    "el leasing «{}» tiene en Recibidas la factura «{number}» de {} del {}, que es \
                     su cuota {}/{}: un contrato con una factura registrada no se borra. Si el \
                     contrato no existe, esa factura tampoco debería (pide al arrendador la \
                     rectificativa o consúltalo con tu gestor).",
                    l.name,
                    lessor.name,
                    taxpain_core::calendar::reminders::format_es(issue),
                    q.n,
                    l.months
                )));
            }
        }
    }
    let (asset, start): (Option<i64>, Option<i64>) = conn.query_row(
        "SELECT asset_id, start_entry FROM leasing WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if let Some(asset) = asset {
        let depreciated: Option<i32> = conn
            .query_row(
                "SELECT MIN(fiscal_year) FROM journal_entry
                 WHERE origin_kind = 'Amortizacion' AND origin_id = ?1",
                [asset],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        if let Some(year) = depreciated {
            return Err(other(format!(
                "el bien del leasing «{}» tiene amortización contabilizada (desde {year}): \
                 deshaz antes esa amortización en Inmovilizado.",
                l.name
            )));
        }
        let disposed: bool = conn.query_row(
            "SELECT disposal_entry IS NOT NULL FROM fixed_asset WHERE id = ?1",
            [asset],
            |r| r.get(0),
        )?;
        if disposed {
            return Err(other(format!(
                "el bien del leasing «{}» está dado de baja en el inmovilizado: ese contrato no \
                 fue un error de alta.",
                l.name
            )));
        }
    }
    // Asientos del contrato: el de alta y los de paso a la 174 (origen «Leasing», sin cuotas).
    let mut entries: Vec<(i64, String)> = Vec::new();
    if let Some(e) = start {
        entries.push((e, "de alta".into()));
    }
    let mut st = conn.prepare(
        "SELECT id, fiscal_year FROM journal_entry
         WHERE origin_kind = 'Leasing' AND origin_id = ?1 ORDER BY id",
    )?;
    for row in st.query_map([id], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i32>(1)?)))? {
        let (e, year) = row?;
        if Some(e) != start {
            entries.push((e, format!("de paso a la 174 del cierre de {year}")));
        }
    }
    drop(st);
    for (entry, what) in &entries {
        let (year, closed): (i32, bool) = conn.query_row(
            "SELECT e.fiscal_year, COALESCE(f.closed, 0) FROM journal_entry e
             LEFT JOIN fiscal_year f ON f.year = e.fiscal_year WHERE e.id = ?1",
            [entry],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if closed {
            return Err(other(format!(
                "el asiento {what} del leasing «{}» es del ejercicio {year}, que está cerrado: \
                 reábrelo antes (Cierre).",
                l.name
            )));
        }
        for (table, column, why) in crate::import::ENTRY_REFERENCES {
            if table == "leasing" {
                continue;
            }
            let used: bool = conn.query_row(
                &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {column} = ?1)"),
                [entry],
                |r| r.get(0),
            )?;
            if used {
                return Err(other(format!(
                    "el asiento {what} del leasing «{}» {why}: deshaz antes eso.",
                    l.name
                )));
            }
        }
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM leasing_installment WHERE leasing_id = ?1",
        [id],
    )?;
    tx.execute("DELETE FROM leasing WHERE id = ?1", [id])?;
    for (entry, _) in &entries {
        tx.execute(
            "DELETE FROM line_tick WHERE line_id IN (SELECT id FROM journal_line WHERE entry_id = ?1)",
            [entry],
        )?;
        tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [entry])?;
        tx.execute("DELETE FROM journal_entry WHERE id = ?1", [entry])?;
    }
    if let Some(asset) = asset {
        tx.execute(
            "DELETE FROM iva_bien_inversion WHERE asset_id = ?1",
            [asset],
        )?;
        tx.execute("DELETE FROM fixed_asset WHERE id = ?1", [asset])?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "leasing.delete",
        &id.to_string(),
        &serde_json::json!({
            "name": l.name,
            "principal": l.principal.cents(),
            "months": l.months,
            "entries": entries.iter().map(|(e, _)| *e).collect::<Vec<_>>(),
            "asset": asset,
        }),
    )?;
    tx.commit()?;
    Ok(())
}

// ---------- Cuotas del sistema antiguo y facturas del arrendador duplicadas ----------

/// NIF normalizado para comparar (el del arrendador se guarda como se tecleó).
fn norm_nif(n: &str) -> String {
    Nif::parse(n).map_or_else(|_| n.trim().to_ascii_uppercase(), |x| x.as_str().to_owned())
}

/// Margen para reconocer la factura real de una cuota: la fecha (la de la factura suele ser la
/// del vencimiento; las cuotas son mensuales) y el importe (redondeos del arrendador).
const CLASH_DAYS: i64 = 10;
const CLASH_AMOUNT: Money = Money::from_euros(1, 0);

/// La cuota del cuadro de `l` que corresponde a una factura del arrendador del día `issue` con
/// esa base o ese total: la de vencimiento más cercano, si está a `CLASH_DAYS` días o menos y
/// el importe coincide (±1 €). Una factura del arrendador por otra cosa (otro importe u otra
/// fecha: gastos, la opción de compra…) no es una cuota.
fn matching_installment(
    l: &Leasing,
    issue: Date,
    base: Money,
    total: Money,
) -> Option<Installment> {
    let q = schedule(l)
        .into_iter()
        .min_by_key(|q| (q.date - issue).whole_days().abs())?;
    let near = (q.date - issue).whole_days().abs() <= CLASH_DAYS;
    let same = |a: Money, b: Money| (a - b).abs() <= CLASH_AMOUNT;
    (near && (same(base, q.amount) || same(total, q.amount + q.vat))).then_some(q)
}

/// Contratos de leasing guardados (para quien sólo tiene la conexión).
fn contracts(conn: &Connection) -> Result<Vec<Leasing>> {
    let mut st = conn.prepare("SELECT data FROM leasing ORDER BY id")?;
    let rows: Vec<String> = st
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    rows.iter()
        .map(|d| serde_json::from_str(d).map_err(other))
        .collect()
}

/// Si una factura recibida del proveedor `supplier` (fecha `issue`, base y total) es una cuota
/// de un leasing suyo que ya está registrada desde Leasing, el motivo para no registrarla otra
/// vez: deduciría su IVA dos veces y duplicaría el gasto y la deuda. La factura que registra el
/// propio leasing (`number` = la de la cuota) no choca consigo misma. Lo usan la revisión de
/// recibidas (a mano, por foto o con IA) y la importación del libro de recibidas. Con `true`
/// si la cuota ya es una factura registrada desde Leasing (quien llama dice qué hacer con la
/// suya: descartarla, no teclearla…); con `false`, si es del sistema antiguo.
pub(crate) fn installment_clash(
    conn: &Connection,
    supplier: &str,
    number: &str,
    issue: Date,
    base: Money,
    total: Money,
) -> Result<Option<(String, bool)>> {
    let supplier = norm_nif(supplier);
    for l in contracts(conn)? {
        let Some(lessor) = &l.lessor else { continue };
        if norm_nif(&lessor.nif) != supplier {
            continue;
        }
        let Some(q) = matching_installment(&l, issue, base, total) else {
            continue;
        };
        if number.trim() == invoice_number(&l, q.n) {
            continue;
        }
        let origin: Option<Option<String>> = conn
            .query_row(
                "SELECT e.origin_kind FROM leasing_installment li
                 LEFT JOIN journal_entry e ON e.id = li.entry_id
                 WHERE li.leasing_id = ?1 AND li.n = ?2",
                params![l.id, q.n],
                |r| r.get(0),
            )
            .optional()?;
        let Some(origin) = origin else { continue };
        let what = format!(
            "Esta factura es la cuota {}/{} del leasing «{}» (vence el {}, {} con IVA)",
            q.n,
            l.months,
            l.name,
            taxpain_core::calendar::reminders::format_es(q.date),
            q.amount + q.vat
        );
        let legacy = origin.as_deref() == Some("Leasing");
        return Ok(Some(if legacy {
            (
                format!(
                    "{what}, que ya está contabilizada desde Contabilidad → Leasing (con el sistema \
                 antiguo): no la registres aquí, porque el gasto y el IVA se contarían dos veces. \
                 Para que su IVA entre en el 303, pulsa «Regularizar» en Leasing."
                ),
                false,
            )
        } else {
            (
                format!(
                    "{what}, que ya está registrada desde Contabilidad → Leasing como la factura \
                 «{}» de {}: registrarla otra vez deduciría su IVA dos veces y duplicaría el \
                 gasto y la deuda con el arrendador.",
                    invoice_number(&l, q.n),
                    lessor.name
                ),
                true,
            )
        }));
    }
    Ok(None)
}

/// Por qué la cuota antigua `q` (asiento `entry`, del sistema anterior) no se puede regularizar
/// desde aquí (`None` si se puede). Regularizar borra su asiento: no se toca un ejercicio
/// cerrado, ni un trimestre con el 303 presentado o con el IVA ya liquidado en el diario (la
/// liquidación se llevó su 472), ni un asiento al que apunta otra cosa (un movimiento del
/// banco…). Tampoco si su factura real ya está en Recibidas (estaría contabilizada dos veces).
/// El IVA de las cuotas bloqueadas se deduce en un 303 posterior, sin rectificar el del
/// trimestre: art. 99.Tres LIVA (leído en el BOE el 2026-10-27): «El derecho a la deducción solo
/// podrá ejercitarse en la declaración-liquidación relativa al periodo de liquidación en que su
/// titular haya soportado las cuotas deducibles o en las de los sucesivos, siempre que no hubiera
/// transcurrido el plazo de cuatro años, contados a partir del nacimiento del mencionado
/// derecho». No hace falta rectificar el 303 ni tocar la liquidación ya hecha: el usuario
/// registra la factura del arrendador en el trimestre en que la deduce y la 472 sale de ese
/// asiento. TODO(VERIFY): qué contrapartida lleva si la cuota ya se pagó y se contabilizó sin
/// IVA (lo lógico es contra la cuenta donde quedó ese IVA, gasto o 524; no hay consulta leída).
fn legacy_blocker(
    conn: &Connection,
    l: &Leasing,
    q: &Installment,
    entry: i64,
) -> Result<Option<String>> {
    let (year, closed, date): (i32, bool, String) = conn.query_row(
        "SELECT e.fiscal_year, COALESCE(f.closed, 0), e.date FROM journal_entry e
         LEFT JOIN fiscal_year f ON f.year = e.fiscal_year WHERE e.id = ?1",
        [entry],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    if closed {
        return Ok(Some(format!("es del ejercicio {year}, que está cerrado")));
    }
    let date = Date::parse(
        &date,
        time::macros::format_description!("[year]-[month]-[day]"),
    )
    .map_err(other)?;
    let month = u8::from(date.month());
    let quarter = (month - 1) / 3 + 1;
    let y = date.year();
    let presented: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM tax_return WHERE model = '303' AND year = ?1
                         AND period IN (?2, ?3) AND environment = 'produccion')
             OR EXISTS(SELECT 1 FROM filed_model WHERE model = '303' AND year = ?1
                         AND period IN (?2, ?3))",
        params![y, format!("{quarter}T"), format!("{month:02}")],
        |r| r.get(0),
    )?;
    if presented {
        return Ok(Some(format!(
            "el 303 del {quarter}T de {y} ya está presentado"
        )));
    }
    let settled: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM journal_entry
                       WHERE origin_kind = 'LiquidacionIva' AND origin_id = ?1)",
        [i64::from(y) * 100 + i64::from(quarter)],
        |r| r.get(0),
    )?;
    if settled {
        return Ok(Some(format!(
            "el IVA del {quarter}T de {y} ya está liquidado en el diario"
        )));
    }
    for (table, column, what) in crate::import::ENTRY_REFERENCES {
        if table == "leasing_installment" {
            continue;
        }
        let used: bool = conn.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE {column} = ?1)"),
            [entry],
            |r| r.get(0),
        )?;
        if used {
            return Ok(Some(format!("su asiento {what}")));
        }
    }
    if let Some(lessor) = &l.lessor {
        let mut st = conn.prepare(
            "SELECT number, issue_date, base, total FROM invoice_received
             WHERE status = 'revisada' AND supplier_nif = ?1 AND issue_date IS NOT NULL",
        )?;
        let rows: Vec<(String, String, i64, i64)> = st
            .query_map([norm_nif(&lessor.nif)], |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    r.get(1)?,
                    r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    r.get::<_, Option<i64>>(3)?.unwrap_or(0),
                ))
            })?
            .collect::<std::result::Result<_, _>>()?;
        for (number, issue, base, total) in rows {
            let Ok(issue) = Date::parse(
                &issue,
                time::macros::format_description!("[year]-[month]-[day]"),
            ) else {
                continue;
            };
            let found =
                matching_installment(l, issue, Money::from_cents(base), Money::from_cents(total));
            if found.is_some_and(|f| f.n == q.n) {
                return Ok(Some(format!(
                    "su factura real ya está en Recibidas («{number}»): la cuota está \
                     contabilizada dos veces"
                )));
            }
        }
    }
    Ok(None)
}

/// Resultado de [`regularize`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Regularized {
    /// Cuotas regularizadas.
    pub done: usize,
    /// Las que no se pueden regularizar desde aquí, con el motivo.
    pub blocked: Vec<String>,
}

/// Regulariza las cuotas de un contrato contabilizadas con el sistema antiguo (asiento directo
/// con el IVA a la 472 contra la 572, sin factura): para cada una registra la factura del
/// arrendador como las nuevas (entra en el libro registro, en el 303 del periodo en que se
/// registra —art. 99.Tres LIVA— y en el 347), la da por pagada en la cartera el día de la cuota
/// contra la misma cuenta de tesorería y borra el asiento antiguo, de modo que el gasto, el IVA
/// y la deuda no se duplican. Las que tienen un bloqueo (ver [`legacy_blocker`]) se dejan como
/// están y se devuelven con el motivo. Necesita la confirmación del usuario.
pub fn regularize(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<Regularized> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let view = list(v)?
        .into_iter()
        .find(|x| x.leasing.id == id)
        .ok_or(AppError::NotFound)?;
    let l = &view.leasing;
    let Some(lessor) = &l.lessor else {
        return Err(other(
            "indica antes el arrendador del leasing (NIF y nombre): cada cuota se registra como \
             una factura suya",
        ));
    };
    let mut done = Vec::new();
    let mut blocked = Vec::new();
    for iv in view.installments.iter().filter(|q| q.legacy) {
        if let Some(b) = &iv.blocked {
            blocked.push(format!("cuota {}/{}: {b}", iv.q.n, l.months));
            continue;
        }
        regularize_one(v, l, lessor, &iv.q, now)?;
        done.push(iv.q.n);
    }
    if !done.is_empty() {
        audit::append(
            v.connection(),
            &now_str(now),
            Actor::User,
            "leasing.regularize",
            &id.to_string(),
            &serde_json::json!({ "cuotas": done, "bloqueadas": blocked.len() }),
        )?;
    }
    Ok(Regularized {
        done: done.len(),
        blocked,
    })
}

/// Regulariza una cuota antigua (ver [`regularize`]).
fn regularize_one(
    v: &Vault,
    l: &Leasing,
    lessor: &Lessor,
    q: &Installment,
    now: OffsetDateTime,
) -> Result<()> {
    let conn = v.connection();
    let old: i64 = conn.query_row(
        "SELECT entry_id FROM leasing_installment WHERE leasing_id = ?1 AND n = ?2",
        params![l.id, q.n],
        |r| r.get(0),
    )?;
    // La cuenta desde la que se pagó (la 572 que se eligió entonces).
    let treasury: String = conn.query_row(
        "SELECT account FROM journal_line WHERE entry_id = ?1 AND credit > 0
         ORDER BY credit DESC, id LIMIT 1",
        [old],
        |r| r.get(0),
    )?;
    let treasury = AccountCode::new(&treasury).map_err(other)?;
    let (r, split) = installment_invoice(l, lessor, q);
    let rid = received::register_typed_split(v, &r, None, Some(&split), now)?;
    if let Err(e) = replace_entry(v, l.id, q, old, rid, &treasury, now) {
        // Sin el cambio del asiento, la factura nueva duplicaría la cuota: se quita.
        remove_invoice(v, rid)?;
        return Err(e);
    }
    Ok(())
}

/// Pasa la cuota `q` del asiento antiguo `old` al de su factura `rid`: los adjuntos se mueven,
/// el asiento antiguo se borra y la factura queda pagada el día de la cuota contra `treasury`.
fn replace_entry(
    v: &Vault,
    leasing: i64,
    q: &Installment,
    old: i64,
    rid: i64,
    treasury: &AccountCode,
    now: OffsetDateTime,
) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let new: i64 = tx.query_row(
        "SELECT id FROM journal_entry WHERE origin_kind = 'FacturaRecibida' AND origin_id = ?1",
        [rid],
        |r| r.get(0),
    )?;
    tx.execute(
        "UPDATE entry_document SET entry_id = ?1 WHERE entry_id = ?2",
        params![new, old],
    )?;
    tx.execute(
        "UPDATE leasing_installment SET entry_id = ?1 WHERE leasing_id = ?2 AND n = ?3",
        params![new, leasing, q.n],
    )?;
    tx.execute("DELETE FROM journal_line WHERE entry_id = ?1", [old])?;
    tx.execute("DELETE FROM journal_entry WHERE id = ?1", [old])?;
    due_on(&tx, rid, q.date)?;
    for item in crate::cartera::installments(&tx, "recibida", rid)? {
        if item.status == "pendiente" {
            crate::cartera::settle_in(
                &tx,
                "recibida",
                rid,
                item.seq,
                q.date,
                treasury,
                Some("cuota de leasing ya pagada"),
                None,
                now,
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}

/// La factura de una cuota se paga entera el día de la cuota (el cargo del arrendador), no con
/// las condiciones de pago del proveedor: así la cartera y el banco proponen la cuota que toca.
fn due_on(conn: &Connection, rid: i64, date: Date) -> Result<()> {
    let total = crate::cartera::installments(conn, "recibida", rid)?
        .first()
        .map(|i| i.invoice_amount)
        .ok_or(AppError::NotFound)?;
    conn.execute(
        "DELETE FROM invoice_installment WHERE kind = 'recibida' AND invoice_id = ?1",
        [rid],
    )?;
    conn.execute(
        "INSERT INTO invoice_installment(kind, invoice_id, seq, due_date, amount)
         VALUES ('recibida', ?1, 1, ?2, ?3)",
        params![rid, date.to_string(), total.cents()],
    )?;
    Ok(())
}

/// Quita una factura recibida recién registrada (su asiento, sus vencimientos y su documento).
fn remove_invoice(v: &Vault, rid: i64) -> Result<()> {
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let doc: Option<i64> = tx
        .query_row(
            "SELECT document_id FROM invoice_received WHERE id = ?1",
            [rid],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    tx.execute(
        "DELETE FROM journal_line WHERE entry_id IN (SELECT id FROM journal_entry
           WHERE origin_kind = 'FacturaRecibida' AND origin_id = ?1)",
        [rid],
    )?;
    tx.execute(
        "DELETE FROM journal_entry WHERE origin_kind = 'FacturaRecibida' AND origin_id = ?1",
        [rid],
    )?;
    tx.execute(
        "DELETE FROM invoice_installment WHERE kind = 'recibida' AND invoice_id = ?1",
        [rid],
    )?;
    tx.execute(
        "UPDATE iva_bien_inversion SET received_id = NULL WHERE received_id = ?1",
        [rid],
    )?;
    tx.execute("DELETE FROM invoice_received WHERE id = ?1", [rid])?;
    if let Some(doc) = doc {
        tx.execute("DELETE FROM document WHERE id = ?1", [doc])?;
    }
    tx.commit()?;
    Ok(())
}

/// Cuenta 174 de un leasing: la de la deuda con el grupo cambiado (5240001 → 1740001).
fn long_term_account(l: &Leasing) -> Result<AccountCode> {
    let debt = l.debt_account.as_str();
    AccountCode::new(&format!("174{}", debt.get(3..).unwrap_or(""))).map_err(other)
}

/// Al cierre del ejercicio `year`, deja en la 174 de cada leasing el capital que vence a más de
/// doce meses (y el resto en la 524), con un asiento por la diferencia. Se puede repetir: si ya
/// está al día no hace nada. Devuelve cuántos asientos ha hecho.
///
/// Los contratos que comparten la cuenta de la deuda (la interfaz los da de alta todos en la
/// 5240001) se reclasifican juntos: el objetivo de su 174 es la suma de los largos plazos de
/// todos, y sale un asiento por cuenta.
pub fn reclassify(v: &Vault, year: i32, now: OffsetDateTime) -> Result<usize> {
    use std::collections::BTreeMap;
    use taxpain_core::accounting::leasing::long_term_at;
    use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
    let fy = accounting::fiscal_year(v, year)?;
    let views = list(v)?;
    let mut groups: BTreeMap<AccountCode, (Vec<&Leasing>, Money)> = BTreeMap::new();
    for view in &views {
        let l = &view.leasing;
        if l.start > fy.end {
            continue;
        }
        let g = groups
            .entry(l.debt_account.clone())
            .or_insert_with(|| (Vec::new(), Money::ZERO));
        g.0.push(l);
        g.1 += long_term_at(l, fy.end);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let mut n = 0;
    for (debt, (contracts, target)) in groups {
        let Some(first) = contracts.first() else {
            continue;
        };
        let lt = long_term_account(first)?;
        // Saldo acreedor de la 174 en el ejercicio (con su apertura y sin el asiento de cierre),
        // como el balance de cierre. Sumar todos los ejercicios contaría dos veces el saldo en
        // cuanto hay uno cerrado: la reclasificación del anterior y la apertura que la trae.
        let current: i64 = tx.query_row(
            "SELECT COALESCE(SUM(l.credit - l.debit), 0) FROM journal_line l
             JOIN journal_entry e ON e.id = l.entry_id
             WHERE l.account = ?1 AND e.fiscal_year = ?2 AND e.origin_kind <> 'Cierre'",
            params![lt.as_str(), year],
            |r| r.get(0),
        )?;
        let diff = target - Money::from_cents(current);
        if diff.is_zero() {
            continue;
        }
        let (to_long, amount) = (diff.cents() > 0, diff.abs());
        let names: Vec<&str> = contracts.iter().map(|l| l.name.as_str()).collect();
        let e = JournalEntry {
            date: fy.end,
            concept: format!(
                "Leasing {}: deuda a más de un año {} la 174",
                names.join(", "),
                if to_long { "a" } else { "desde" }
            ),
            origin: Origin::Leasing(first.id),
            lines: vec![
                EntryLine::signed(debt.clone(), amount, to_long),
                EntryLine::signed(lt.clone(), amount, !to_long),
            ]
            .into_iter()
            .flatten()
            .collect(),
        };
        accounting::record_entry(&tx, &e, now)?;
        n += 1;
    }
    if n > 0 {
        audit::append(
            &tx,
            &now_str(now),
            Actor::User,
            "leasing.reclassify",
            &year.to_string(),
            &serde_json::json!({ "entries": n }),
        )?;
    }
    tx.commit()?;
    Ok(n)
}
