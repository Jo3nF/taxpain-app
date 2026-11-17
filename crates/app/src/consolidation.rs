//! Consolidación (agregación) de las cuentas de varias empresas de este Mac: balance y cuenta
//! de pérdidas y ganancias sumados, con la eliminación de lo que se deben entre ellas y de sus
//! ventas y compras mutuas.
//!
//! Cómo se reconoce lo de dentro del grupo: las subcuentas de clientes y proveedores cuyo NIF
//! es el de otra empresa del grupo. Se eliminan sus saldos (créditos y deudas recíprocos) y,
//! de los asientos que las usan, las líneas de ingresos (7) y gastos (6). Si lo que una dice
//! que le deben no coincide con lo que la otra dice que debe, se avisa.
//!
//! `TODO(VERIFY)`: no es la consolidación de las NOFCAC (RD 1159/2010): sin eliminación
//! inversión-patrimonio neto, socios externos, fondo de comercio ni ajustes de homogeneización.
//! Sirve como agregado de gestión y como punto de partida.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;
use taxpain_core::accounting::ledger::{AccountBalance, trial_balance};
use taxpain_core::accounting::statements::{ModelKind, balance_sheet_for, income_statement_for};
use taxpain_core::accounting::{AccountCode, Origin};
use taxpain_core::money::Money;
use taxpain_vault::Vault;

use crate::accounting::{self, AnnualAccounts};
use crate::error::Result;

/// Lo que hace falta de cada empresa para consolidar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanyData {
    pub name: String,
    pub nif: String,
    pub balances: Vec<AccountBalance>,
    /// Saldo (deudor +, acreedor −) de cada subcuenta de otra empresa del grupo: (cuenta, NIF
    /// de la otra, rol).
    pub intercompany: Vec<(AccountCode, String, String, Money)>,
    /// Líneas de ingresos y gastos de asientos con otra empresa del grupo: (cuenta, saldo).
    pub intercompany_pl: Vec<(AccountCode, Money)>,
    pub fiscal_year_end: time::Date,
}

/// Datos de una empresa para el ejercicio `year`; `group` son los NIF de las demás.
pub fn company_data(
    v: &Vault,
    name: &str,
    nif: &str,
    year: i32,
    group: &[String],
) -> Result<CompanyData> {
    let fy = accounting::fiscal_year(v, year)?;
    let entries: Vec<_> = accounting::journal(v, year)?
        .into_iter()
        .map(|s| s.entry)
        .filter(|e| !matches!(e.origin, Origin::Regularizacion | Origin::Cierre))
        .collect();
    let balances = trial_balance(&entries);
    let others: BTreeSet<&str> = group
        .iter()
        .map(String::as_str)
        .filter(|n| *n != nif)
        .collect();
    let mut accounts: BTreeMap<String, (String, String)> = BTreeMap::new();
    {
        let mut st = v
            .connection()
            .prepare("SELECT account, tax_id, role FROM third_party_account")?;
        let rows = st.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        for row in rows {
            let (account, tax_id, role) = row?;
            if others.contains(tax_id.to_uppercase().as_str()) {
                accounts.insert(account, (tax_id.to_uppercase(), role));
            }
        }
    }
    let intercompany = balances
        .iter()
        .filter_map(|b| {
            accounts
                .get(b.account.as_str())
                .map(|(nif, role)| (b.account.clone(), nif.clone(), role.clone(), b.balance()))
        })
        .filter(|x| !x.3.is_zero())
        .collect();
    let mut pl: BTreeMap<AccountCode, Money> = BTreeMap::new();
    for e in &entries {
        if !e
            .lines
            .iter()
            .any(|l| accounts.contains_key(l.account.as_str()))
        {
            continue;
        }
        for l in &e.lines {
            if matches!(l.account.group(), 6 | 7) {
                *pl.entry(l.account.clone()).or_default() += l.debit - l.credit;
            }
        }
    }
    Ok(CompanyData {
        name: name.to_owned(),
        nif: nif.to_uppercase(),
        balances,
        intercompany,
        intercompany_pl: pl.into_iter().filter(|(_, m)| !m.is_zero()).collect(),
        fiscal_year_end: fy.end,
    })
}

/// Una eliminación aplicada: empresa, cuenta, contraparte y saldo eliminado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Elimination {
    pub company: String,
    pub account: String,
    pub counterpart: String,
    pub amount: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Consolidated {
    pub companies: Vec<String>,
    pub accounts: AnnualAccounts,
    /// Sumas y saldos agregados, ya con las eliminaciones.
    pub balances: Vec<AccountBalance>,
    pub eliminations: Vec<Elimination>,
    pub warnings: Vec<String>,
}

fn add(map: &mut BTreeMap<AccountCode, (Money, Money)>, account: &AccountCode, d: Money, c: Money) {
    let e = map.entry(account.clone()).or_default();
    e.0 += d;
    e.1 += c;
}

/// Suma las empresas y, si `eliminate`, quita lo de dentro del grupo.
pub fn consolidate(data: &[CompanyData], kind: ModelKind, eliminate: bool) -> Consolidated {
    let mut warnings = Vec::new();
    let mut map: BTreeMap<AccountCode, (Money, Money)> = BTreeMap::new();
    for c in data {
        for b in &c.balances {
            add(&mut map, &b.account, b.debit, b.credit);
        }
    }
    let ends: BTreeSet<time::Date> = data.iter().map(|c| c.fiscal_year_end).collect();
    if ends.len() > 1 {
        warnings.push(
            "Las empresas no cierran el ejercicio el mismo día: se suman ejercicios distintos"
                .into(),
        );
    }
    let mut eliminations = Vec::new();
    if eliminate {
        let name_of = |nif: &str| {
            data.iter()
                .find(|c| c.nif == nif)
                .map_or(nif.to_owned(), |c| c.name.clone())
        };
        // Créditos y deudas recíprocos: se quita el saldo de cada subcuenta.
        let mut net_by_pair: BTreeMap<(String, String), Money> = BTreeMap::new();
        for c in data {
            for (account, nif, _, saldo) in &c.intercompany {
                if saldo.cents() > 0 {
                    add(&mut map, account, Money::ZERO, *saldo);
                } else {
                    add(&mut map, account, -*saldo, Money::ZERO);
                }
                eliminations.push(Elimination {
                    company: c.name.clone(),
                    account: account.as_str().to_owned(),
                    counterpart: name_of(nif),
                    amount: *saldo,
                });
                let key = if c.nif < *nif {
                    (c.nif.clone(), nif.clone())
                } else {
                    (nif.clone(), c.nif.clone())
                };
                *net_by_pair.entry(key).or_default() += *saldo;
            }
        }
        for ((a, b), net) in net_by_pair {
            if !net.is_zero() {
                warnings.push(format!(
                    "Lo que se deben {} y {} no cuadra entre las dos contabilidades: \
                     diferencia de {net}",
                    name_of(&a),
                    name_of(&b)
                ));
            }
        }
        // Ventas y compras dentro del grupo.
        let mut pl_total = Money::ZERO;
        for c in data {
            for (account, saldo) in &c.intercompany_pl {
                if saldo.cents() > 0 {
                    add(&mut map, account, Money::ZERO, *saldo);
                } else {
                    add(&mut map, account, -*saldo, Money::ZERO);
                }
                pl_total += *saldo;
                eliminations.push(Elimination {
                    company: c.name.clone(),
                    account: account.as_str().to_owned(),
                    counterpart: "grupo".into(),
                    amount: *saldo,
                });
            }
        }
        if !pl_total.is_zero() {
            warnings.push(format!(
                "Las ventas y compras dentro del grupo no coinciden: el resultado consolidado \
                 cambia en {}",
                -pl_total
            ));
        }
        warnings.push(
            "Agregado de gestión: no elimina la inversión en las filiales contra su patrimonio \
             neto ni calcula socios externos (consolidación de las NOFCAC)."
                .into(),
        );
    }
    let balances: Vec<AccountBalance> = map
        .into_iter()
        .map(|(account, (debit, credit))| AccountBalance {
            account,
            debit,
            credit,
        })
        .filter(|b| !b.debit.is_zero() || !b.credit.is_zero())
        .collect();
    let income = income_statement_for(kind, &balances);
    let balance = balance_sheet_for(kind, &balances, income.get(kind.result_id()));
    let balanced = balance.get("TA") == balance.get("TP");
    Consolidated {
        companies: data.iter().map(|c| c.name.clone()).collect(),
        accounts: AnnualAccounts {
            balance,
            income,
            balanced,
        },
        balances,
        eliminations,
        warnings,
    }
}
