//! Balance y cuenta de pérdidas y ganancias con los modelos oficiales: PYMES (RD 1515/2007) y
//! normal y abreviado (RD 1514/2007), tercera parte. Mapeo cuentas → partidas y códigos del
//! depósito (Orden JUS/616/2022) en `fixtures/accounting/cuentas-anuales-*.json`.
//!
//! Signos: en el activo cuenta el saldo deudor; en patrimonio neto, pasivo y PyG, el acreedor.
//! Las cuentas entre paréntesis del modelo ya restan con esa regla (p. ej. 280, de saldo
//! acreedor, minora el inmovilizado). Cada cuenta va a la partida con el prefijo más largo que
//! coincida; las que no encajan en ninguna se devuelven en `unmapped`.

use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::ledger::AccountBalance;
use crate::money::Money;

const PYMES: &str = include_str!("../../../../fixtures/accounting/cuentas-anuales-pymes.json");
const ABREVIADO: &str =
    include_str!("../../../../fixtures/accounting/cuentas-anuales-abreviado.json");
const NORMAL: &str = include_str!("../../../../fixtures/accounting/cuentas-anuales-normal.json");

/// Modelo de cuentas anuales.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelKind {
    #[default]
    Pymes,
    Abreviado,
    Normal,
}

impl ModelKind {
    /// Partida del resultado del ejercicio en la cuenta de pérdidas y ganancias.
    pub fn result_id(self) -> &'static str {
        match self {
            ModelKind::Normal => "A.5",
            _ => "D",
        }
    }

    /// Partidas de resultado que son sumas de otras (en orden de cálculo).
    fn formulas(self) -> &'static [(&'static str, &'static [&'static str])] {
        // Las partidas 1 a 11 y «Otros resultados» (678, 778; RD 602/2016).
        const ONE_TO_ELEVEN: &[&str] = &[
            "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "OR",
        ];
        const FINANCIAL: &[&str] = &["12", "13", "14", "15", "16"];
        match self {
            ModelKind::Normal => &[
                ("A.1", ONE_TO_ELEVEN),
                ("A.2", FINANCIAL),
                ("A.3", &["A.1", "A.2"]),
                ("A.4", &["A.3", "17"]),
                ("A.5", &["A.4", "18"]),
            ],
            _ => &[
                ("A", ONE_TO_ELEVEN),
                ("B", FINANCIAL),
                ("C", &["A", "B"]),
                ("D", &["C", "17"]),
            ],
        }
    }
}

#[derive(Deserialize)]
struct Model {
    balance: Vec<Partida>,
    pyg: Vec<Partida>,
}

#[derive(Deserialize)]
struct Partida {
    id: String,
    label: String,
    #[serde(default)]
    codigo: Option<String>,
    accounts: Option<String>,
    #[serde(default)]
    side: Option<String>,
}

fn model_of(kind: ModelKind) -> &'static Model {
    static P: OnceLock<Model> = OnceLock::new();
    static A: OnceLock<Model> = OnceLock::new();
    static N: OnceLock<Model> = OnceLock::new();
    let (cell, data) = match kind {
        ModelKind::Pymes => (&P, PYMES),
        ModelKind::Abreviado => (&A, ABREVIADO),
        ModelKind::Normal => (&N, NORMAL),
    };
    cell.get_or_init(|| serde_json::from_str(data).expect("modelo de cuentas anuales válido"))
}

#[cfg(test)]
fn model() -> &'static Model {
    model_of(ModelKind::Pymes)
}

/// Prefijos de cuenta de una partida ("20, (280), 71*" → ["20", "280", "71"]).
fn prefixes(spec: &str) -> Vec<String> {
    spec.split(',')
        .map(|p| {
            p.trim()
                .trim_matches(|c| c == '(' || c == ')' || c == '*')
                .trim()
                .to_owned()
        })
        .filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatementLine {
    pub id: String,
    pub label: String,
    pub amount: Money,
    /// Partida con cuentas (hoja) o agregado.
    pub leaf: bool,
    /// Código de la partida en el depósito de cuentas (Orden JUS/616/2022).
    #[serde(default)]
    pub code: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Statement {
    pub lines: Vec<StatementLine>,
    /// Cuentas con saldo que no encajan en ninguna partida.
    pub unmapped: Vec<AccountBalance>,
}

impl Statement {
    pub fn get(&self, id: &str) -> Money {
        self.lines
            .iter()
            .find(|l| l.id == id)
            .map_or(Money::ZERO, |l| l.amount)
    }
}

/// Partida (índice) a la que va cada cuenta: el prefijo más largo entre `candidates`.
fn assign(account: &str, candidates: &[(usize, Vec<String>)]) -> Option<usize> {
    candidates
        .iter()
        .flat_map(|(i, ps)| {
            ps.iter()
                .filter(|p| account.starts_with(p.as_str()))
                .map(move |p| (p.len(), *i))
        })
        .max_by_key(|(len, _)| *len)
        .map(|(_, i)| i)
}

/// `child` cuelga de `parent`: «A.I» de «A», «PA-1» de «PA», «12.a1» de «12.a».
fn is_descendant(child: &str, parent: &str) -> bool {
    if child.len() <= parent.len() || !child.starts_with(parent) {
        return false;
    }
    let next = child.as_bytes()[parent.len()];
    matches!(next, b'.' | b'-')
        || (next.is_ascii_digit() && parent.as_bytes().last().is_some_and(u8::is_ascii_lowercase))
}

/// Cuenta de pérdidas y ganancias (modelo PYMES) a partir de saldos de cuentas 6 y 7.
pub fn income_statement(balances: &[AccountBalance]) -> Statement {
    income_statement_for(ModelKind::Pymes, balances)
}

/// Cuenta de pérdidas y ganancias del modelo `kind`.
pub fn income_statement_for(kind: ModelKind, balances: &[AccountBalance]) -> Statement {
    let m = model_of(kind);
    let leaves: Vec<(usize, Vec<String>)> = m
        .pyg
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.accounts.as_deref().map(|a| (i, prefixes(a))))
        .collect();
    let mut amounts = vec![Money::ZERO; m.pyg.len()];
    let mut unmapped = Vec::new();
    for b in balances
        .iter()
        .filter(|b| b.account.is_income_statement() && !b.balance().is_zero())
    {
        match assign(b.account.as_str(), &leaves) {
            Some(i) => amounts[i] -= b.balance(), // saldo acreedor positivo
            None => unmapped.push(b.clone()),
        }
    }
    // Agregados: suma de sus hojas.
    for (i, p) in m.pyg.iter().enumerate() {
        if p.accounts.is_none() {
            amounts[i] = m
                .pyg
                .iter()
                .enumerate()
                .filter(|(j, q)| q.accounts.is_some() && is_descendant(&q.id, &p.id) && *j != i)
                .map(|(j, _)| amounts[j])
                .sum();
        }
    }
    // Resultados: sumas de partidas (en orden).
    let pos = |id: &str| m.pyg.iter().position(|p| p.id == id);
    let formulas = kind.formulas();
    for (id, parts) in formulas {
        let total: Money = parts
            .iter()
            .filter_map(|p| pos(p))
            .map(|j| amounts[j])
            .sum();
        if let Some(i) = pos(id) {
            amounts[i] = total;
        }
    }
    let is_formula = |id: &str| formulas.iter().any(|(f, _)| *f == id);
    let lines = m
        .pyg
        .iter()
        .enumerate()
        .map(|(i, p)| StatementLine {
            id: p.id.clone(),
            label: p.label.clone(),
            // Los títulos (operaciones continuadas/interrumpidas del modelo normal) no suman.
            amount: if p.accounts.is_none() && !is_formula(&p.id) && p.codigo.is_none() {
                Money::ZERO
            } else {
                amounts[i]
            },
            leaf: p.accounts.is_some(),
            code: p.codigo.clone(),
        })
        .collect();
    Statement { lines, unmapped }
}

/// Balance (modelo PYMES). `year_result` es el resultado aún no regularizado (se suma a la
/// partida de la cuenta 129); si ya se regularizó, pásese cero.
pub fn balance_sheet(balances: &[AccountBalance], year_result: Money) -> Statement {
    balance_sheet_for(ModelKind::Pymes, balances, year_result)
}

/// Balance del modelo `kind`.
pub fn balance_sheet_for(
    kind: ModelKind,
    balances: &[AccountBalance],
    year_result: Money,
) -> Statement {
    let m = model_of(kind);
    let leaves: Vec<(usize, Vec<String>)> = m
        .balance
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.accounts.as_deref().map(|a| (i, prefixes(a))))
        .collect();
    let mut amounts = vec![Money::ZERO; m.balance.len()];
    let mut unmapped = Vec::new();
    // Prefijo más largo y, a igual longitud, la partida de su lado: el modelo lista algunas
    // cuentas en el activo y en el pasivo según su saldo (551 y 5525 en B.IV y PC.II.3 en el
    // PYMES; PGC PYMES, tercera parte), así que una cuenta corriente con socios deudora va al
    // activo y acreedora al pasivo.
    let pick = |account: &str, debit: bool| -> Option<usize> {
        leaves
            .iter()
            .flat_map(|(i, ps)| {
                ps.iter()
                    .filter(|p| account.starts_with(p.as_str()))
                    .map(move |p| (p.len(), *i))
            })
            .max_by_key(|(len, i)| {
                let asset = m.balance[*i].side.as_deref() == Some("activo");
                (*len, asset == debit)
            })
            .map(|(_, i)| i)
    };
    for b in balances
        .iter()
        .filter(|b| !b.account.is_income_statement() && !b.balance().is_zero())
    {
        match pick(b.account.as_str(), !b.balance().is_negative()) {
            Some(i) => {
                let asset = m.balance[i].side.as_deref() == Some("activo");
                amounts[i] += if asset { b.balance() } else { -b.balance() };
            }
            None => unmapped.push(b.clone()),
        }
    }
    if let Some(i) = m
        .balance
        .iter()
        .position(|p| p.accounts.as_deref() == Some("129"))
    {
        amounts[i] += year_result;
    }
    let leaf_total = |pred: &dyn Fn(&Partida) -> bool| -> Money {
        m.balance
            .iter()
            .enumerate()
            .filter(|(_, p)| p.accounts.is_some() && pred(p))
            .map(|(i, _)| amounts[i])
            .sum()
    };
    let lines = m
        .balance
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let amount = match (p.accounts.is_some(), p.id.as_str()) {
                (true, _) => amounts[i],
                (false, "TA") => leaf_total(&|x| x.side.as_deref() == Some("activo")),
                (false, "TP") => leaf_total(&|x| x.side.as_deref() != Some("activo")),
                (false, parent) => leaf_total(&|x| is_descendant(&x.id, parent)),
            };
            StatementLine {
                id: p.id.clone(),
                label: p.label.clone(),
                amount,
                leaf: p.accounts.is_some(),
                code: p.codigo.clone(),
            }
        })
        .collect();
    Statement { lines, unmapped }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounting::account::acc;
    use crate::accounting::entry::{EntryLine, JournalEntry, Origin};
    use crate::accounting::ledger::trial_balance;
    use time::macros::date;

    fn e(lines: Vec<EntryLine>) -> JournalEntry {
        JournalEntry {
            date: date!(2026 - 06 - 01),
            concept: "x".into(),
            origin: Origin::Manual,
            lines,
        }
    }

    #[test]
    fn modelo_cargado() {
        assert_eq!(model().balance.len(), 52);
        // Las 21 del modelo y «Otros resultados».
        assert_eq!(model().pyg.len(), 22);
        assert_eq!(
            prefixes("20, (280), (290), 71*"),
            ["20", "280", "290", "71"]
        );
        assert!(
            is_descendant("A.I", "A") && is_descendant("PA-1.I", "PA") && !is_descendant("PA", "P")
        );
    }

    #[test]
    fn los_tres_modelos_cuadran_y_dan_el_mismo_resultado() {
        let m = Money::from_euros;
        let entries = [
            e(vec![
                EntryLine::debit(acc("5720001"), m(10000, 0)),
                EntryLine::credit(acc("100"), m(3000, 0)),
                EntryLine::credit(acc("1700001"), m(7000, 0)),
            ]),
            e(vec![
                EntryLine::debit(acc("217"), m(1200, 0)),
                EntryLine::credit(acc("5720001"), m(1200, 0)),
            ]),
            e(vec![
                EntryLine::debit(acc("681"), m(300, 0)),
                EntryLine::credit(acc("2817"), m(300, 0)),
            ]),
            e(vec![
                EntryLine::debit(acc("4300001"), m(1210, 0)),
                EntryLine::credit(acc("705"), m(1000, 0)),
                EntryLine::credit(acc("477"), m(210, 0)),
            ]),
            e(vec![
                EntryLine::debit(acc("600"), m(150, 0)),
                EntryLine::debit(acc("640"), m(100, 0)),
                EntryLine::debit(acc("6623"), m(20, 0)),
                EntryLine::debit(acc("472"), m(31, 50)),
                EntryLine::credit(acc("4000001"), m(301, 50)),
            ]),
        ];
        let tb = trial_balance(&entries);
        for kind in [ModelKind::Pymes, ModelKind::Abreviado, ModelKind::Normal] {
            let pyg = income_statement_for(kind, &tb);
            assert!(pyg.unmapped.is_empty(), "{kind:?}: {:?}", pyg.unmapped);
            assert_eq!(pyg.get(kind.result_id()), m(430, 0), "{kind:?}");
            let bal = balance_sheet_for(kind, &tb, pyg.get(kind.result_id()));
            assert!(bal.unmapped.is_empty(), "{kind:?}: {:?}", bal.unmapped);
            assert_eq!(bal.get("TA"), bal.get("TP"), "{kind:?}");
            assert_eq!(bal.get("TA"), m(10941, 50), "{kind:?}");
            assert_eq!(bal.get("PA-1.VII"), m(430, 0), "{kind:?}");
            // Cada partida con importe lleva su código del depósito.
            assert!(
                pyg.lines
                    .iter()
                    .chain(&bal.lines)
                    .filter(|l| !l.amount.is_zero())
                    .all(|l| l.code.is_some()),
                "{kind:?}"
            );
        }
        // Normal: desglose de ventas y de gastos de personal y financieros.
        let pyg = income_statement_for(ModelKind::Normal, &tb);
        assert_eq!(pyg.get("1"), m(1000, 0));
        assert_eq!(pyg.get("1.b"), m(1000, 0));
        assert_eq!(pyg.get("6.a"), -m(100, 0));
        assert_eq!(pyg.get("13.b"), -m(20, 0));
        assert_eq!(pyg.get("A.1"), m(450, 0));
        assert_eq!(pyg.get("A.2"), -m(20, 0));
        assert_eq!(pyg.get("A"), Money::ZERO, "los títulos no suman");
    }

    #[test]
    fn sl_sencilla_cuadra() {
        let m = Money::from_euros;
        let entries = [
            // Capital 3.000 al banco.
            e(vec![
                EntryLine::debit(acc("5720001"), m(3000, 0)),
                EntryLine::credit(acc("100"), m(3000, 0)),
            ]),
            // Portátil 1.200 pagado con banco, amortización 300.
            e(vec![
                EntryLine::debit(acc("217"), m(1200, 0)),
                EntryLine::credit(acc("5720001"), m(1200, 0)),
            ]),
            e(vec![
                EntryLine::debit(acc("681"), m(300, 0)),
                EntryLine::credit(acc("2817"), m(300, 0)),
            ]),
            // Venta 1.000 + 210 IVA a cliente.
            e(vec![
                EntryLine::debit(acc("4300001"), m(1210, 0)),
                EntryLine::credit(acc("705"), m(1000, 0)),
                EntryLine::credit(acc("477"), m(210, 0)),
            ]),
            // Gasto 200 + 42 IVA a acreedor.
            e(vec![
                EntryLine::debit(acc("629"), m(200, 0)),
                EntryLine::debit(acc("472"), m(42, 0)),
                EntryLine::credit(acc("4100001"), m(242, 0)),
            ]),
        ];
        let tb = trial_balance(&entries);
        let pyg = income_statement(&tb);
        assert_eq!(pyg.get("1"), m(1000, 0));
        assert_eq!(pyg.get("7"), -m(200, 0));
        assert_eq!(pyg.get("8"), -m(300, 0));
        assert_eq!(pyg.get("A"), m(500, 0));
        assert_eq!(pyg.get("D"), m(500, 0));
        assert!(pyg.unmapped.is_empty());

        let bal = balance_sheet(&tb, pyg.get("D"));
        assert!(bal.unmapped.is_empty(), "{:?}", bal.unmapped);
        assert_eq!(bal.get("A.II"), m(900, 0));
        assert_eq!(bal.get("B.II.1"), m(1210, 0));
        assert_eq!(bal.get("B.VI"), m(1800, 0));
        assert_eq!(bal.get("PA-1.VII"), m(500, 0));
        assert_eq!(bal.get("TA"), m(3952, 0));
        assert_eq!(
            bal.get("TA"),
            bal.get("TP"),
            "activo = patrimonio neto + pasivo"
        );
        assert_eq!(bal.get("A") + bal.get("B"), bal.get("TA"));
        assert_eq!(bal.get("PA") + bal.get("PB") + bal.get("PC"), bal.get("TP"));
    }

    #[test]
    fn cuentas_sin_partida_se_informan() {
        let m = Money::from_euros;
        let tb = trial_balance(&[e(vec![
            EntryLine::debit(acc("473"), m(150, 0)),
            EntryLine::credit(acc("5720001"), m(150, 0)),
        ])]);
        let bal = balance_sheet(&tb, Money::ZERO);
        let unmapped: Vec<&str> = bal.unmapped.iter().map(|b| b.account.as_str()).collect();
        // La 473 va con las demás de Hacienda en «Otros deudores» (no figura en el modelo
        // transcrito, pero sin ella el balance no cuadra antes de contabilizar el impuesto).
        assert!(unmapped.is_empty(), "{unmapped:?}");
        assert_eq!(bal.get("B.II.3"), m(150, 0));
    }

    /// La 551 (cuenta corriente con socios) está en B.IV y en PC.II.3: va al activo con saldo
    /// deudor y al pasivo con saldo acreedor, nunca como pasivo negativo.
    #[test]
    fn cuentas_de_los_dos_lados_segun_su_saldo() {
        let m = Money::from_euros;
        // El socio debe 100 € a la sociedad (salieron del banco).
        let tb = trial_balance(&[e(vec![
            EntryLine::debit(acc("5510001"), m(100, 0)),
            EntryLine::credit(acc("5720001"), m(100, 0)),
        ])]);
        let bal = balance_sheet(&tb, Money::ZERO);
        assert_eq!(bal.get("B.IV"), m(100, 0));
        assert_eq!(bal.get("PC.II.3"), Money::ZERO);
        assert_eq!(bal.get("TA"), bal.get("TP"));
        // La sociedad debe 100 € al socio (los puso en el banco).
        let tb = trial_balance(&[e(vec![
            EntryLine::debit(acc("5720001"), m(100, 0)),
            EntryLine::credit(acc("5510001"), m(100, 0)),
        ])]);
        let bal = balance_sheet(&tb, Money::ZERO);
        assert_eq!(bal.get("B.IV"), Money::ZERO);
        assert_eq!(bal.get("PC.II.3"), m(100, 0));
        assert_eq!(bal.get("TA"), m(100, 0));
        assert_eq!(bal.get("TA"), bal.get("TP"));
    }

    proptest::proptest! {
        /// Con cualquier conjunto de asientos cuadrados sobre cuentas del modelo, el balance
        /// cuadra y el resultado de la PyG es el de la contabilidad.
        #[test]
        fn balance_siempre_cuadra(moves in proptest::collection::vec((0usize..14, 0usize..14, 1i64..5_000_000), 1..60)) {
            let accounts = ["100", "5720001", "217", "2817", "4300001", "4100001", "477", "472", "4751", "520", "629", "705", "681", "640"];
            let entries: Vec<_> = moves.iter().filter(|(a, b, _)| a != b).map(|&(a, b, c)| e(vec![
                EntryLine::debit(acc(accounts[a]), Money::from_cents(c)),
                EntryLine::credit(acc(accounts[b]), Money::from_cents(c)),
            ])).collect();
            let tb = trial_balance(&entries);
            let pyg = income_statement(&tb);
            proptest::prop_assert!(pyg.unmapped.is_empty());
            proptest::prop_assert_eq!(pyg.get("D"), crate::accounting::closing::result_of_year(&entries));
            let bal = balance_sheet(&tb, pyg.get("D"));
            proptest::prop_assert!(bal.unmapped.is_empty(), "{:?}", bal.unmapped);
            proptest::prop_assert_eq!(bal.get("TA"), bal.get("TP"));
        }
    }

    /// Una multa (678) va a «Otros resultados», dentro del resultado de explotación, y un 630
    /// sin subcuenta al impuesto sobre beneficios: el resultado y el balance cuadran.
    #[test]
    fn otros_resultados_y_630() {
        let m = Money::from_euros;
        let tb = trial_balance(&[
            e(vec![
                EntryLine::debit(acc("5720001"), m(3_000, 0)),
                EntryLine::credit(acc("100"), m(3_000, 0)),
            ]),
            e(vec![
                EntryLine::debit(acc("4300001"), m(10_000, 0)),
                EntryLine::credit(acc("705"), m(10_000, 0)),
            ]),
            e(vec![
                EntryLine::debit(acc("678"), m(200, 0)),
                EntryLine::credit(acc("5720001"), m(200, 0)),
            ]),
            e(vec![
                EntryLine::debit(acc("630"), m(100, 0)),
                EntryLine::credit(acc("4752"), m(100, 0)),
            ]),
        ]);
        let pyg = income_statement(&tb);
        assert!(pyg.unmapped.is_empty(), "{:?}", pyg.unmapped);
        assert_eq!(pyg.get("OR"), m(-200, 0));
        assert_eq!(pyg.get("A"), m(9_800, 0));
        assert_eq!(pyg.get("17"), m(-100, 0));
        assert_eq!(pyg.get("D"), m(9_700, 0));
        let bal = balance_sheet(&tb, pyg.get("D"));
        assert!(bal.unmapped.is_empty(), "{:?}", bal.unmapped);
        assert_eq!(bal.get("TA"), bal.get("TP"));
    }
}
