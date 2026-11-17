//! Norma 43 (Cuaderno 43 AEB/CECA/UNACC, «Información normalizada de cuenta corriente»,
//! junio 2012). Registros de 80 posiciones; especificación en `fixtures/accounting/n43.md`.
//!
//! Signos desde el punto de vista del titular: abonos (clave 2, Haber) positivos, cargos
//! (clave 1, Debe) negativos; saldo acreedor (clave 2) positivo.

use serde::{Deserialize, Serialize};
use time::{Date, Month};

use crate::money::Money;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Movement {
    pub op_date: Date,
    pub value_date: Date,
    pub common_concept: String,
    pub own_concept: String,
    /// + abono (ingreso), − cargo.
    pub amount: Money,
    pub document: String,
    pub reference1: String,
    pub reference2: String,
    /// Textos de los registros 23 (concepto complementario).
    pub details: Vec<String>,
}

impl Movement {
    /// Descripción legible: complementarios o referencias.
    pub fn description(&self) -> String {
        let d = self.details.join(" ");
        let d = d.split_whitespace().collect::<Vec<_>>().join(" ");
        if d.is_empty() {
            format!(
                "{} {}",
                common_concept_name(&self.common_concept),
                self.reference2.trim()
            )
            .trim()
            .to_owned()
        } else {
            d
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct N43Account {
    pub bank: String,
    pub office: String,
    pub account: String,
    pub start: Date,
    pub end: Date,
    pub initial_balance: Money,
    pub final_balance: Money,
    pub currency: String,
    pub holder: String,
    pub movements: Vec<Movement>,
}

impl N43Account {
    /// Entidad + oficina + cuenta (identifica la cuenta en el fichero).
    pub fn number(&self) -> String {
        format!("{}{}{}", self.bank, self.office, self.account)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum N43Error {
    #[error("línea {0}: registro desconocido o mal formado")]
    BadRecord(usize),
    #[error("línea {0}: fecha no válida")]
    BadDate(usize),
    #[error("línea {0}: importe no válido")]
    BadAmount(usize),
    #[error("línea {0}: movimiento fuera de una cuenta")]
    Orphan(usize),
    #[error("cuenta {0}: los totales del registro 33 no cuadran con los movimientos")]
    TotalsMismatch(String),
    #[error("el número de registros del registro 88 no coincide")]
    RecordCountMismatch,
    #[error("el fichero no contiene cuentas")]
    Empty,
}

/// Nombre del concepto común (Anexo 2).
pub fn common_concept_name(code: &str) -> &'static str {
    match code {
        "01" => "Talones - reintegros",
        "02" => "Abonarés - entregas - ingresos",
        "03" => "Domiciliados - recibos",
        "04" => "Transferencias - traspasos",
        "05" => "Amortizaciones de préstamos",
        "06" => "Remesas de efectos",
        "07" => "Suscripciones - canjes",
        "08" => "Dividendos - cupones",
        "09" => "Operaciones de bolsa",
        "10" => "Cheques gasolina",
        "11" => "Cajero automático",
        "12" => "Tarjetas",
        "13" => "Operaciones extranjero",
        "14" => "Devoluciones e impagados",
        "15" => "Nóminas - seguros sociales",
        "16" => "Timbres - corretaje",
        "17" => "Intereses - comisiones - gastos",
        "98" => "Anulaciones",
        _ => "Varios",
    }
}

/// Texto del fichero: UTF-8 si lo es; si no, ISO-8859-1 (habitual en los bancos).
pub fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

fn field(line: &[char], from: usize, to: usize) -> String {
    line.get(from - 1..to.min(line.len()))
        .map(|c| c.iter().collect())
        .unwrap_or_default()
}

fn date(line: &[char], from: usize, n: usize) -> Result<Date, N43Error> {
    let s = field(line, from, from + 5);
    let num = |a: usize| s.get(a..a + 2).and_then(|x| x.parse::<u8>().ok());
    let (yy, mm, dd) = (num(0), num(2), num(4));
    match (yy, mm, dd) {
        (Some(y), Some(m), Some(d)) => Month::try_from(m)
            .ok()
            .and_then(|m| Date::from_calendar_date(2000 + i32::from(y), m, d).ok())
            .ok_or(N43Error::BadDate(n)),
        _ => Err(N43Error::BadDate(n)),
    }
}

fn amount(line: &[char], from: usize, to: usize, n: usize) -> Result<Money, N43Error> {
    let s = field(line, from, to);
    if s.len() != to - from + 1 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(N43Error::BadAmount(n));
    }
    s.parse::<i64>()
        .map(Money::from_cents)
        .map_err(|_| N43Error::BadAmount(n))
}

fn count(line: &[char], from: usize, to: usize, n: usize) -> Result<i64, N43Error> {
    field(line, from, to)
        .trim()
        .parse()
        .map_err(|_| N43Error::BadRecord(n))
}

/// Lee un fichero Norma 43 y comprueba sus totales.
pub fn parse_n43(text: &str) -> Result<Vec<N43Account>, N43Error> {
    let mut accounts: Vec<N43Account> = Vec::new();
    let mut current: Option<N43Account> = None;
    let mut records = 0i64;
    let mut saw_88 = false;
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let raw = raw.trim_end_matches('\r');
        if raw.trim().is_empty() {
            continue;
        }
        let mut line: Vec<char> = raw.chars().collect();
        line.resize(80.max(line.len()), ' ');
        match field(&line, 1, 2).as_str() {
            "11" => {
                records += 1;
                let sign = if field(&line, 33, 33) == "1" { -1 } else { 1 };
                current = Some(N43Account {
                    bank: field(&line, 3, 6),
                    office: field(&line, 7, 10),
                    account: field(&line, 11, 20),
                    start: date(&line, 21, n)?,
                    end: date(&line, 27, n)?,
                    initial_balance: Money::from_cents(sign * amount(&line, 34, 47, n)?.cents()),
                    final_balance: Money::ZERO,
                    currency: field(&line, 48, 50),
                    holder: field(&line, 52, 77).trim().to_owned(),
                    movements: Vec::new(),
                });
            }
            "22" => {
                records += 1;
                let acc = current.as_mut().ok_or(N43Error::Orphan(n))?;
                let sign = match field(&line, 28, 28).as_str() {
                    "1" => -1,
                    "2" => 1,
                    _ => return Err(N43Error::BadRecord(n)),
                };
                acc.movements.push(Movement {
                    op_date: date(&line, 11, n)?,
                    value_date: date(&line, 17, n)?,
                    common_concept: field(&line, 23, 24),
                    own_concept: field(&line, 25, 27),
                    amount: Money::from_cents(sign * amount(&line, 29, 42, n)?.cents()),
                    document: field(&line, 43, 52).trim().to_owned(),
                    reference1: field(&line, 53, 64).trim().to_owned(),
                    reference2: field(&line, 65, 80).trim().to_owned(),
                    details: Vec::new(),
                });
            }
            "23" => {
                records += 1;
                let acc = current.as_mut().ok_or(N43Error::Orphan(n))?;
                let m = acc.movements.last_mut().ok_or(N43Error::Orphan(n))?;
                for part in [field(&line, 5, 42), field(&line, 43, 80)] {
                    if !part.trim().is_empty() {
                        m.details.push(part.trim().to_owned());
                    }
                }
            }
            "24" => records += 1, // Equivalencia en divisa: informativa.
            "33" => {
                records += 1;
                let mut acc = current.take().ok_or(N43Error::Orphan(n))?;
                let debits: Money = acc
                    .movements
                    .iter()
                    .filter(|m| m.amount.is_negative())
                    .map(|m| -m.amount)
                    .sum();
                let credits: Money = acc
                    .movements
                    .iter()
                    .filter(|m| !m.amount.is_negative())
                    .map(|m| m.amount)
                    .sum();
                let n_debit = acc
                    .movements
                    .iter()
                    .filter(|m| m.amount.is_negative())
                    .count();
                let n_credit = acc.movements.len() - n_debit;
                let sign = if field(&line, 59, 59) == "1" { -1 } else { 1 };
                let final_balance = Money::from_cents(sign * amount(&line, 60, 73, n)?.cents());
                let ok = count(&line, 21, 25, n)? == i64::try_from(n_debit).unwrap_or(-1)
                    && amount(&line, 26, 39, n)? == debits
                    && count(&line, 40, 44, n)? == i64::try_from(n_credit).unwrap_or(-1)
                    && amount(&line, 45, 58, n)? == credits
                    && acc.initial_balance + credits - debits == final_balance;
                if !ok {
                    return Err(N43Error::TotalsMismatch(acc.number()));
                }
                acc.final_balance = final_balance;
                accounts.push(acc);
            }
            "88" => {
                saw_88 = true;
                if count(&line, 21, 26, n)? != records {
                    return Err(N43Error::RecordCountMismatch);
                }
            }
            _ => return Err(N43Error::BadRecord(n)),
        }
    }
    let _ = saw_88; // El registro 88 es opcional para leer cuentas completas.
    if current.is_some() || accounts.is_empty() {
        return Err(N43Error::Empty);
    }
    Ok(accounts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    const SAMPLE: &str = include_str!("../../../fixtures/accounting/n43-ejemplo.txt");

    #[test]
    fn fichero_de_ejemplo() {
        let a = parse_n43(SAMPLE).unwrap();
        assert_eq!(a.len(), 1);
        let acc = &a[0];
        assert_eq!(acc.bank, "9999");
        assert_eq!(acc.initial_balance, Money::from_cents(123_456));
        assert_eq!(acc.final_balance, Money::from_cents(97_322));
        assert_eq!(acc.movements.len(), 6);
        assert_eq!(acc.start, date!(2026 - 10 - 01));
        let first = &acc.movements[0];
        assert_eq!(first.amount, Money::from_euros(1210, 0));
        assert!(first.description().contains("TRANSFERENCIA DE CLIENTE"));
        let sum: Money = acc.movements.iter().map(|m| m.amount).sum();
        assert_eq!(acc.initial_balance + sum, acc.final_balance);
    }

    #[test]
    fn detecta_ficheros_alterados() {
        // Cambiar un importe rompe los totales del registro 33.
        let bad = SAMPLE.replacen("000000001210000", "000000001211000", 1);
        assert!(matches!(parse_n43(&bad), Err(N43Error::TotalsMismatch(_))));
        assert!(matches!(
            parse_n43("99 basura"),
            Err(N43Error::BadRecord(1))
        ));
        assert!(matches!(parse_n43(""), Err(N43Error::Empty)));
        let no88: String = SAMPLE
            .lines()
            .filter(|l| !l.starts_with("88"))
            .map(|l| format!("{l}\n"))
            .collect();
        assert!(
            parse_n43(&no88).is_ok(),
            "el 88 es de cierre de fichero; sin él se aceptan las cuentas completas"
        );
        let wrong88 = SAMPLE.replace("88999999999999999999000014", "88999999999999999999000099");
        if wrong88 != SAMPLE {
            assert!(matches!(
                parse_n43(&wrong88),
                Err(N43Error::RecordCountMismatch)
            ));
        }
    }

    #[test]
    fn latin1() {
        assert_eq!(decode(&[b'N', 0xD3, b'M']), "NÓM");
    }
}
