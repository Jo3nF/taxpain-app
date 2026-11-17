//! Códigos de cuenta del PGC: el primer dígito es el grupo (1–7 en el PGC de PYMES; 8–9 sólo
//! en el PGC normal). Las subcuentas se forman ampliando dígitos (p. ej. 4300001).

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AccountError {
    #[error("código de cuenta no válido: {0} (sólo dígitos, de 3 a 12)")]
    Invalid(String),
    #[error("la subcuenta {n} de la {base} no cabe en {width} dígitos")]
    Overflow { base: String, n: u32, width: usize },
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AccountCode(String);

impl AccountCode {
    pub fn new(code: &str) -> Result<Self, AccountError> {
        let c = code.trim();
        if (3..=12).contains(&c.len())
            && c.bytes().all(|b| b.is_ascii_digit())
            && !c.starts_with('0')
        {
            Ok(AccountCode(c.to_owned()))
        } else {
            Err(AccountError::Invalid(code.to_owned()))
        }
    }

    /// Código de un grupo o subgrupo (1 o 2 cifras) para los saldos agregados de
    /// [`super::ledger::aggregate`]; para cuentas, [`Self::new`].
    pub(crate) fn agregado(code: &str) -> Option<Self> {
        let c = code.trim();
        ((1..=12).contains(&c.len())
            && c.bytes().all(|b| b.is_ascii_digit())
            && !c.starts_with('0'))
        .then(|| AccountCode(c.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Grupo del PGC (1–9).
    pub fn group(&self) -> u8 {
        self.0.as_bytes()[0] - b'0'
    }

    /// `true` si esta cuenta es `prefix` o una subcuenta suya (430 ⊃ 4300001).
    pub fn starts_with(&self, prefix: &str) -> bool {
        self.0.starts_with(prefix)
    }

    /// Subcuenta: `430` + 7 dígitos → `4300001` para el tercero `n`. Falla si el número no
    /// cabe en `width` dígitos: un plan con longitudes mezcladas lo rechaza a3 al exportar y
    /// confunde los listados.
    pub fn subaccount(&self, width: usize, n: u32) -> Result<AccountCode, AccountError> {
        let digits = n.to_string();
        let Some(pad) = width.checked_sub(self.0.len() + digits.len()) else {
            return Err(AccountError::Overflow {
                base: self.0.clone(),
                n,
                width,
            });
        };
        Ok(AccountCode(format!(
            "{}{}{digits}",
            self.0,
            "0".repeat(pad)
        )))
    }

    /// Cuentas de gestión (grupos 6 y 7): se saldan contra 129 en la regularización.
    pub fn is_income_statement(&self) -> bool {
        matches!(self.group(), 6 | 7)
    }

    /// Gastos e ingresos imputados al patrimonio neto (grupos 8 y 9): no se regularizan contra
    /// 129; al cierre se traspasan a su cuenta del grupo 13 (PGC, quinta parte).
    pub fn is_equity_income_expense(&self) -> bool {
        matches!(self.group(), 8 | 9)
    }
}

impl TryFrom<String> for AccountCode {
    type Error = AccountError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        AccountCode::new(&s)
    }
}

impl From<AccountCode> for String {
    fn from(a: AccountCode) -> String {
        a.0
    }
}

impl fmt::Display for AccountCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Atajo para cuentas fijas del código.
pub fn acc(code: &str) -> AccountCode {
    AccountCode::new(code).expect("código de cuenta constante válido")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codigos() {
        assert_eq!(acc("430").subaccount(7, 1).unwrap().as_str(), "4300001");
        assert_eq!(acc("4770").subaccount(7, 12).unwrap().as_str(), "4770012");
        assert_eq!(acc("430").subaccount(7, 9999).unwrap().as_str(), "4309999");
        assert!(acc("430").subaccount(7, 10_000).is_err(), "ocho dígitos");
        assert!(acc("5720001").subaccount(7, 1).is_err());
        assert_eq!(acc("705").group(), 7);
        assert!(acc("4300001").starts_with("430"));
        assert!(acc("629").is_income_statement() && !acc("572").is_income_statement());
        assert!(AccountCode::new("43").is_err());
        assert!(AccountCode::new("43a").is_err());
        assert!(AccountCode::new("0430").is_err());
        let j: AccountCode = serde_json::from_str("\"5720001\"").unwrap();
        assert_eq!(j.as_str(), "5720001");
        assert!(serde_json::from_str::<AccountCode>("\"x\"").is_err());
    }
}
