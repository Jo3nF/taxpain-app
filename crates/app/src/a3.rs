//! Exportación de la contabilidad a a3ASESOR | eco / | con (`SUENLACE.DAT`), para la gestoría
//! que trabaja con a3. Formato en `taxpain_core::accounting::suenlace` (por defecto el 5, el
//! vigente; el 4 como opción).
//!
//! - Clientes y proveedores van como cuentas con su NIF (registro `C`).
//! - Las facturas van como facturas con IVA (cabecera `1`/`2`, una línea `9` por tipo y el
//!   registro `4` de las rectificativas), para que a3 rellene sus libros de IVA y el 347/349.
//!   Antes se comprueba que el asiento que montará a3 con esos registros es el mismo que el de
//!   TaxPain; si no (IVA no deducible, inversión del sujeto pasivo, varias cuentas…), la factura
//!   pasa como asiento sin IVA (registros `0`) y se avisa.
//! - El resto de asientos, como apuntes sin IVA. La regularización y el cierre no se exportan:
//!   los hace a3.
//!
//! `TODO(VERIFY)`: importe de la cabecera con retención (aquí, lo que se cobra o paga: base +
//! IVA + recargo − retención, que es el saldo del cliente o proveedor; ver
//! `docs/research/A3_FORMATOS.md` §1.3). Probarlo importando en un a3 real.

use std::collections::BTreeMap;

use serde::Serialize;
use taxpain_core::accounting::suenlace::{
    AccountRecord, Apunte, Format, InvoiceBook, InvoiceHeader, LinePos, VatLine, Writer,
    expand_account,
};
use taxpain_core::accounting::{EntryLine, Origin};
use taxpain_core::invoice::{Exemption, InvoiceKind, PartyId, VatTreatment};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::Vault;
use taxpain_vault::rusqlite::Connection;
use time::Date;

use crate::accounting;
use crate::error::{AppError, Result};

fn yes() -> bool {
    true
}

/// Opciones de la exportación (las de la empresa en a3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct A3Options {
    /// Código de la empresa en a3 (1–99999).
    pub company: u32,
    /// Dígitos de las cuentas en a3 (nivel 6–12; lo habitual, 8).
    pub digits: usize,
    /// Formato del fichero (5 por omisión; 4 para versiones antiguas de a3).
    #[serde(default)]
    pub format: Format,
    /// Facturas como facturas con IVA (si no, todo como asientos).
    #[serde(default = "yes")]
    pub invoices: bool,
}

const OPTIONS_KEY: &str = "a3_options";

/// Opciones usadas la última vez (o código 1, 8 dígitos, formato 5 y facturas con IVA).
pub fn options(v: &Vault) -> Result<A3Options> {
    Ok(v.setting(OPTIONS_KEY)?.unwrap_or(A3Options {
        company: 1,
        digits: 8,
        format: Format::F5,
        invoices: true,
    }))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct A3Export {
    pub file: Vec<u8>,
    /// Asientos exportados (facturas incluidas).
    pub entries: usize,
    /// De ellos, facturas con IVA.
    pub invoices: usize,
    pub accounts: usize,
    pub warnings: Vec<String>,
}

fn err(e: impl std::fmt::Display) -> AppError {
    AppError::Other(format!("a3: {e}"))
}

/// Saldo deudor (debe − haber) por cuenta.
fn deltas(lines: &[EntryLine]) -> BTreeMap<String, i64> {
    let mut m = BTreeMap::new();
    for l in lines {
        *m.entry(l.account.as_str().to_owned()).or_insert(0) += l.debit.cents() - l.credit.cents();
    }
    m.retain(|_, v| *v != 0);
    m
}

/// Una línea `9`, con importes ya en el signo de a3.
struct PlannedVat {
    account: String,
    subtype: u8,
    base: Money,
    vat_rate: Rate,
    vat: Money,
    surcharge_rate: Rate,
    surcharge: Money,
    withholding_rate: Rate,
    withholding: Money,
    form: Option<u8>,
    vat_account: Option<String>,
    withholding_account: Option<String>,
}

/// Factura lista para escribirse como registros con IVA.
struct Planned {
    rectifying: bool,
    book: InvoiceBook,
    account: String,
    name: String,
    nif: Option<String>,
    number: String,
    invoice_date: Date,
    operation_date: Option<Date>,
    total: Money,
    lines: Vec<PlannedVat>,
    /// Factura rectificada (fecha, número) y tipo SII del registro 4.
    rectified: Option<(Date, String, char)>,
}

/// Única cuenta de la lista que empieza por alguno de los prefijos (o ninguna).
fn only(
    accounts: &BTreeMap<String, i64>,
    prefixes: &[&str],
) -> std::result::Result<Option<String>, String> {
    let found: Vec<&String> = accounts
        .keys()
        .filter(|a| prefixes.iter().any(|p| a.starts_with(p)))
        .collect();
    match found.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some((*one).clone())),
        _ => Err(format!("varias cuentas {}", prefixes.join("/"))),
    }
}

fn check(
    expected: BTreeMap<String, i64>,
    actual: &BTreeMap<String, i64>,
) -> std::result::Result<(), String> {
    let mut e = expected;
    e.retain(|_, v| *v != 0);
    if &e == actual {
        Ok(())
    } else {
        Err("su asiento no coincide con el que montaría a3".into())
    }
}

fn plan_issued(
    conn: &Connection,
    id: i64,
    lines: &[EntryLine],
) -> std::result::Result<Planned, String> {
    let (inv, totals) = crate::invoicing::stored_invoice(conn, id).map_err(|e| e.to_string())?;
    let d = deltas(lines);
    let party = only(&d, &["43"])?.ok_or("sin cuenta de cliente")?;
    let revenue = only(&d, &["7"])?.ok_or("sin cuenta de ingresos")?;
    let vat_acc = only(&d, &["477"])?;
    let wh_acc = only(&d, &["473"])?;
    if totals.withholdings.len() > 1 {
        return Err("varios tipos de retención".into());
    }
    let (wh_rate, _, _) =
        totals
            .withholdings
            .first()
            .copied()
            .unwrap_or((Rate::ZERO, Money::ZERO, Money::ZERO));
    let rectifying = matches!(inv.kind, InvoiceKind::Rectificativa(_));
    // En una rectificativa (cabecera 2), a3 quiere en positivo lo que reduce la original.
    let sign = |m: Money| if rectifying { -m } else { m };
    let spanish = matches!(inv.recipient.as_ref().map(|p| &p.id), Some(PartyId::Nif(_)));
    let mut planned = Vec::new();
    for (i, b) in totals.breakdown.iter().enumerate() {
        let (subtype, rate, form) = match b.vat {
            VatTreatment::Taxed { rate } => (1, rate, spanish.then_some(1)),
            VatTreatment::Exempt(Exemption::E1) => (2, Rate::ZERO, spanish.then_some(1)),
            VatTreatment::Exempt(Exemption::E5) => (3, Rate::ZERO, Some(2)),
            VatTreatment::Exempt(Exemption::E2) => (6, Rate::ZERO, None),
            other => return Err(format!("operación {other:?} sin equivalencia segura en a3")),
        };
        let first = i == 0;
        planned.push(PlannedVat {
            account: revenue.clone(),
            subtype,
            base: sign(b.base),
            vat_rate: rate,
            vat: sign(b.vat_amount),
            surcharge_rate: b.surcharge_rate.unwrap_or(Rate::ZERO),
            surcharge: sign(b.surcharge_amount),
            withholding_rate: if first { wh_rate } else { Rate::ZERO },
            withholding: if first {
                sign(totals.withholding)
            } else {
                Money::ZERO
            },
            form,
            vat_account: vat_acc.clone(),
            withholding_account: wh_acc.clone(),
        });
    }
    if planned.is_empty() {
        return Err("sin bases".into());
    }
    let mut expected = BTreeMap::new();
    expected.insert(party.clone(), totals.payable.cents());
    *expected.entry(revenue).or_insert(0) -= totals.base.cents();
    if let Some(a) = &vat_acc {
        *expected.entry(a.clone()).or_insert(0) -= (totals.vat + totals.surcharge).cents();
    } else if !(totals.vat + totals.surcharge).is_zero() {
        return Err("sin cuenta de IVA repercutido".into());
    }
    if let Some(a) = &wh_acc {
        *expected.entry(a.clone()).or_insert(0) += totals.withholding.cents();
    } else if !totals.withholding.is_zero() {
        return Err("sin cuenta de retenciones".into());
    }
    check(expected, &d)?;
    let rectified = match inv.kind {
        InvoiceKind::Rectificativa(r) => {
            let (number, date) = inv
                .rectified
                .first()
                .cloned()
                .ok_or("rectificativa sin la factura rectificada")?;
            let sii = match r.code {
                1 => 'T',
                2 => 'C',
                3 => 'D',
                5 => 'J',
                _ => 'R',
            };
            Some((date, number, sii))
        }
        _ => None,
    };
    let (name, nif) = match &inv.recipient {
        Some(p) => (
            p.name.clone(),
            Some(match &p.id {
                PartyId::Nif(n) => n.clone(),
                PartyId::Foreign { country, id, .. } => format!("{country}{id}"),
            }),
        ),
        None => ("Clientes varios".into(), None),
    };
    Ok(Planned {
        rectifying,
        book: InvoiceBook::Sales,
        account: party,
        name,
        nif,
        number: inv.number,
        invoice_date: inv.issue_date,
        operation_date: inv.operation_date,
        total: sign(totals.payable),
        lines: planned,
        rectified,
    })
}

/// Tipo que da exactamente `amount` sobre `base` (entre los habituales).
fn rate_for(base: Money, amount: Money, candidates: &[i32]) -> Option<Rate> {
    if amount.is_zero() {
        return Some(Rate::ZERO);
    }
    candidates
        .iter()
        .map(|p| Rate::percent(*p))
        .find(|r| base.apply_rate(*r) == amount)
}

fn plan_received(
    conn: &Connection,
    id: i64,
    lines: &[EntryLine],
) -> std::result::Result<Planned, String> {
    let row: (String, String, String, String, i64, i64, i64, String) = conn
        .query_row(
            "SELECT COALESCE(supplier_nif, ''), COALESCE(supplier_name, ''), COALESCE(number, ''),
                    issue_date, base, vat, withholding, vat_regime
             FROM invoice_received WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?;
    let (nif, name, number, issue, base, vat, wh, regime) = row;
    if regime != "interior" {
        return Err("adquisición intracomunitaria o con inversión del sujeto pasivo".into());
    }
    let (base, vat, wh) = (
        Money::from_cents(base),
        Money::from_cents(vat),
        Money::from_cents(wh),
    );
    if base.cents() <= 0 {
        return Err("factura recibida rectificativa o sin base".into());
    }
    let d = deltas(lines);
    let party = only(&d, &["40", "41"])?.ok_or("sin cuenta de proveedor")?;
    let vat_acc = only(&d, &["472"])?;
    let wh_acc = only(&d, &["4751"])?;
    let others: Vec<&String> = d
        .keys()
        .filter(|a| {
            Some(*a) != Some(&party) && Some(*a) != vat_acc.as_ref() && Some(*a) != wh_acc.as_ref()
        })
        .collect();
    let [expense] = others.as_slice() else {
        return Err("varias cuentas de gasto o IVA no deducible".into());
    };
    let expense = (*expense).clone();
    let vat_rate = rate_for(base, vat, &[21, 10, 4, 5, 2]).ok_or("tipo de IVA no reconocido")?;
    let wh_rate =
        rate_for(base, wh, &[15, 7, 19, 1, 2, 20, 24]).ok_or("tipo de retención no reconocido")?;
    let payable = base + vat - wh;
    let mut expected = BTreeMap::new();
    expected.insert(party.clone(), -payable.cents());
    expected.insert(expense.clone(), base.cents());
    if let Some(a) = &vat_acc {
        *expected.entry(a.clone()).or_insert(0) += vat.cents();
    }
    if let Some(a) = &wh_acc {
        *expected.entry(a.clone()).or_insert(0) -= wh.cents();
    }
    check(expected, &d)?;
    let date = Date::parse(
        &issue,
        time::macros::format_description!("[year]-[month]-[day]"),
    )
    .map_err(|e| e.to_string())?;
    let spanish = taxpain_core::Nif::parse(&nif).is_ok();
    Ok(Planned {
        rectifying: false,
        book: if expense.starts_with('2') {
            InvoiceBook::CapitalGoods
        } else {
            InvoiceBook::Purchases
        },
        account: party,
        name,
        nif: (!nif.is_empty()).then_some(nif),
        number,
        invoice_date: date,
        operation_date: None,
        total: payable,
        lines: vec![PlannedVat {
            account: expense,
            subtype: 1,
            base,
            vat_rate,
            vat,
            surcharge_rate: Rate::ZERO,
            surcharge: Money::ZERO,
            withholding_rate: wh_rate,
            withholding: wh,
            form: spanish.then_some(1),
            vat_account: vat_acc,
            withholding_account: wh_acc,
        }],
        rectified: None,
    })
}

/// `SUENLACE.DAT` con el diario del ejercicio `year`.
pub fn suenlace(v: &Vault, year: i32, o: &A3Options) -> Result<A3Export> {
    let conn = v.connection();
    // Terceros: cuenta → (nombre, NIF).
    let mut third: BTreeMap<String, (String, String)> = BTreeMap::new();
    {
        let mut stmt =
            conn.prepare("SELECT account, name, tax_id FROM third_party_account ORDER BY account")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (a, n, t) = row?;
            third.insert(a, (n, t));
        }
    }
    let expand = |code: &str| expand_account(code, o.digits).map_err(err);
    let name_of = |code: &str| -> Result<String> {
        Ok(accounting::account_name(conn, code)?.unwrap_or_else(|| format!("Cuenta {code}")))
    };

    let mut w = Writer::with_format(o.company, o.format).map_err(err)?;
    let mut warnings = Vec::new();
    let start = accounting::fiscal_year_conn(conn, year)?.start;

    // Primero las cuentas de terceros, para que a3 las cree con su NIF.
    let mut accounts = 0;
    for (code, (name, nif)) in &third {
        w.account(&AccountRecord {
            date: start,
            account: &expand(code)?,
            name,
            nif: Some(nif),
            postal_code: None,
            town: None,
            province: None,
        });
        accounts += 1;
    }

    let mut entries = 0;
    let mut invoices = 0;
    let mut skipped = 0;
    let mut as_entries: Vec<String> = Vec::new();
    for e in accounting::journal(v, year)? {
        if matches!(e.entry.origin, Origin::Regularizacion | Origin::Cierre) {
            skipped += 1;
            continue;
        }
        let lines = &e.entry.lines;
        if lines.len() < 2 {
            warnings.push(format!(
                "asiento {} con una sola línea: no se exporta",
                e.number
            ));
            continue;
        }
        let plan = match (o.invoices, &e.entry.origin) {
            (true, Origin::FacturaEmitida(id)) => Some(plan_issued(conn, *id, lines)),
            (true, Origin::FacturaRecibida(id)) => Some(plan_received(conn, *id, lines)),
            _ => None,
        };
        match plan {
            Some(Ok(p)) => {
                let date = e.entry.date;
                w.invoice(&InvoiceHeader {
                    date,
                    rectifying: p.rectifying,
                    account: &expand(&p.account)?,
                    name: &p.name,
                    book: p.book,
                    number: &p.number,
                    concept: &e.entry.concept,
                    total: p.total,
                    nif: p.nif.as_deref(),
                    postal_code: None,
                    operation_date: p.operation_date,
                    invoice_date: p.invoice_date,
                })
                .map_err(err)?;
                let n = p.lines.len();
                for (i, l) in p.lines.iter().enumerate() {
                    let vat_account = l.vat_account.as_deref().map(expand).transpose()?;
                    let wh_account = l.withholding_account.as_deref().map(expand).transpose()?;
                    w.vat_line(&VatLine {
                        date,
                        account: &expand(&l.account)?,
                        account_name: &name_of(&l.account)?,
                        natural: true,
                        number: &p.number,
                        last: i + 1 == n,
                        concept: &e.entry.concept,
                        subtype: l.subtype,
                        base: l.base,
                        vat_rate: l.vat_rate,
                        vat: l.vat,
                        surcharge_rate: l.surcharge_rate,
                        surcharge: l.surcharge,
                        withholding_rate: l.withholding_rate,
                        withholding: l.withholding,
                        form: l.form,
                        subject: true,
                        cash_basis: false,
                        vat_account: vat_account.as_deref(),
                        surcharge_account: vat_account.as_deref(),
                        withholding_account: wh_account.as_deref(),
                    })
                    .map_err(err)?;
                }
                if let Some((odate, onumber, sii)) = &p.rectified {
                    w.rectified(date, *odate, onumber, Some(*sii));
                }
                invoices += 1;
                entries += 1;
                continue;
            }
            Some(Err(why)) => as_entries.push(format!("asiento {} ({why})", e.number)),
            None => {}
        }
        let document = e.number.to_string();
        for (i, l) in lines.iter().enumerate() {
            let code = l.account.as_str();
            let debit = !l.debit.is_zero();
            let amount = if debit { l.debit } else { l.credit };
            w.apunte(&Apunte {
                date: e.entry.date,
                account: &expand(code)?,
                account_name: &name_of(code)?,
                debit,
                document: &document,
                pos: LinePos::of(i, lines.len()),
                concept: l.description.as_deref().unwrap_or(&e.entry.concept),
                amount,
            })
            .map_err(err)?;
        }
        entries += 1;
    }
    if skipped > 0 {
        warnings.push(format!(
            "{skipped} asiento(s) de regularización o cierre no se exportan: el cierre lo hace a3"
        ));
    }
    if !as_entries.is_empty() {
        warnings.push(format!(
            "{} factura(s) van como asiento sin IVA (revisa sus libros de IVA en a3): {}",
            as_entries.len(),
            as_entries.join("; ")
        ));
    }
    if !o.invoices && entries > 0 {
        warnings.push(
            "las facturas pasan como asientos: los libros de IVA de a3 no se rellenan".into(),
        );
    }
    if entries > 0 {
        warnings.push(
            "importa el fichero una sola vez: a3 añade los asientos y, si lo importas de nuevo, quedan duplicados"
                .into(),
        );
    }
    v.set_setting(OPTIONS_KEY, o)?;
    Ok(A3Export {
        file: w.finish(),
        entries,
        invoices,
        accounts,
        warnings,
    })
}
