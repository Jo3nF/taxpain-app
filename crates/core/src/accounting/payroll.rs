//! Nómina (asiento manual asistido): 640 Sueldos y salarios y 642 Seguridad Social a cargo de
//! la empresa al debe; 4751 (IRPF retenido), 476 (Organismos de la Seguridad Social,
//! acreedores: cuota obrera + patronal) y 465 (Remuneraciones pendientes de pago: líquido) al
//! haber. Ver `docs/ACCOUNTING_RULES.md`. El cálculo de la nómina (bases, tipos) no se hace
//! aquí: los importes vienen del recibo de salarios.

use serde::{Deserialize, Serialize};
use time::Date;

use super::account::acc;
use super::entry::{EntryLine, JournalEntry, Origin};
use crate::money::Money;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payslip {
    pub date: Date,
    pub employee: String,
    pub gross: Money,
    pub irpf: Money,
    pub ss_employee: Money,
    pub ss_company: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PayrollError {
    #[error("importes no válidos: el líquido no puede ser negativo")]
    Invalid,
}

impl Payslip {
    /// Líquido a percibir.
    pub fn net(&self) -> Money {
        self.gross - self.irpf - self.ss_employee
    }

    pub fn entry(&self) -> Result<JournalEntry, PayrollError> {
        let amounts = [self.gross, self.irpf, self.ss_employee, self.ss_company];
        if amounts.iter().any(|a| a.is_negative())
            || self.gross.is_zero()
            || self.net().is_negative()
        {
            return Err(PayrollError::Invalid);
        }
        let lines = [
            EntryLine::signed(acc("640"), self.gross, true),
            EntryLine::signed(acc("642"), self.ss_company, true),
            EntryLine::signed(acc("4751"), self.irpf, false),
            EntryLine::signed(acc("476"), self.ss_employee + self.ss_company, false),
            EntryLine::signed(acc("465"), self.net(), false),
        ];
        Ok(JournalEntry {
            date: self.date,
            concept: format!("Nómina {}", self.employee),
            origin: Origin::Nomina,
            lines: lines.into_iter().flatten().collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn nomina_basica() {
        let p = Payslip {
            date: date!(2026 - 10 - 31),
            employee: "Trabajador 1".into(),
            gross: Money::from_euros(2000, 0),
            irpf: Money::from_euros(240, 0),
            ss_employee: Money::from_euros(130, 0),
            ss_company: Money::from_euros(640, 0),
        };
        assert_eq!(p.net(), Money::from_euros(1630, 0));
        let e = p.entry().unwrap();
        e.validate().unwrap();
        let get = |c: &str| {
            e.lines
                .iter()
                .find(|l| l.account.as_str() == c)
                .unwrap()
                .clone()
        };
        assert_eq!(get("476").credit, Money::from_euros(770, 0));
        assert_eq!(get("465").credit, Money::from_euros(1630, 0));
        let mut bad = p.clone();
        bad.irpf = Money::from_euros(3000, 0);
        assert_eq!(bad.entry(), Err(PayrollError::Invalid));
    }
}
