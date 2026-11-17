//! Lectura de un `SUENLACE.DAT` (formatos 4 y 5) para importar en TaxPain la contabilidad de
//! a3 o de otro programa que exporte este fichero. Posiciones en `suenlace.rs` y
//! `docs/research/A3_FORMATOS.md`.
//!
//! - Registros `0` (apuntes sin IVA): forman asiento desde el `I` hasta el `U`.
//! - Facturas `1`/`2` + líneas `9`: el asiento sale como lo monta a3 (cliente o proveedor por el
//!   importe de la cabecera; base, cuota y recargo a su lado natural, o al contrario con `A`;
//!   retención al lado opuesto). Sin cuenta de IVA o de retención se usan 477/472 y 473/4751.
//! - Registros `C`: nombres (y NIF) de las cuentas.
//! - Los demás (`3`, `4`, `5`, vencimientos, analítica…) se cuentan y se ignoran.

use std::collections::BTreeMap;

use time::Date;

use super::account::AccountCode;
use super::entry::{EntryLine, JournalEntry, Origin};
use crate::money::Money;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    #[error("línea {line}: {msg}")]
    Line { line: usize, msg: String },
    #[error("el fichero no tiene registros de SUENLACE")]
    Empty,
}

/// Cuenta dada de alta en el fichero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadAccount {
    pub code: AccountCode,
    pub name: String,
    pub nif: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadFile {
    pub company: u32,
    pub accounts: Vec<ReadAccount>,
    /// Asientos con origen `Manual` (quien importa pone el suyo).
    pub entries: Vec<JournalEntry>,
    /// Cuántos de ellos son facturas con IVA.
    pub invoices: usize,
    /// Registros que no se importan (tipo → cuántos).
    pub ignored: BTreeMap<char, usize>,
}

/// Caracteres de Windows-1252 (o UTF-8 si el fichero lo es).
fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => bytes
            .iter()
            .map(|&b| match b {
                0x80 => '€',
                0x8A => 'Š',
                0x9A => 'š',
                0x91 | 0x92 => '\'',
                0x93 | 0x94 => '"',
                0x96 | 0x97 => '-',
                b => char::from(b),
            })
            .collect(),
    }
}

struct Rec<'a> {
    n: usize,
    chars: &'a [char],
}

impl Rec<'_> {
    fn s(&self, pos: usize, len: usize) -> String {
        self.chars
            .iter()
            .skip(pos - 1)
            .take(len)
            .collect::<String>()
            .trim()
            .to_owned()
    }
    fn err(&self, msg: impl Into<String>) -> ReadError {
        ReadError::Line {
            line: self.n,
            msg: msg.into(),
        }
    }
    fn kind(&self) -> char {
        self.chars.get(14).copied().unwrap_or(' ')
    }
    fn date(&self) -> Result<Date, ReadError> {
        let s = self.s(7, 8);
        let num = |a: usize, b: usize| s.get(a..b).and_then(|x| x.parse::<u16>().ok());
        let (Some(y), Some(m), Some(d)) = (num(0, 4), num(4, 6), num(6, 8)) else {
            return Err(self.err(format!("fecha no válida «{s}»")));
        };
        time::Month::try_from(u8::try_from(m).unwrap_or(0))
            .ok()
            .and_then(|m| {
                Date::from_calendar_date(i32::from(y), m, u8::try_from(d).unwrap_or(0)).ok()
            })
            .ok_or_else(|| self.err(format!("fecha no válida «{s}»")))
    }
    fn amount(&self, pos: usize) -> Result<Money, ReadError> {
        let s = self.s(pos, 14);
        if s.is_empty() {
            return Ok(Money::ZERO);
        }
        let neg = s.starts_with('-');
        let body = s.trim_start_matches(['+', '-']);
        let (i, d) = body.split_once('.').unwrap_or((body, "0"));
        let d = format!("{d:0<2}");
        let (Ok(i), Ok(d)) = (i.trim().parse::<i64>(), d[..2].parse::<i64>()) else {
            return Err(self.err(format!("importe no válido «{s}»")));
        };
        let c = i * 100 + d;
        Ok(Money::from_cents(if neg { -c } else { c }))
    }
    fn account(&self, pos: usize) -> Result<AccountCode, ReadError> {
        let s = self.s(pos, 12);
        AccountCode::new(&s).map_err(|_| self.err(format!("cuenta no válida «{s}»")))
    }
    fn opt_account(&self, pos: usize) -> Result<Option<AccountCode>, ReadError> {
        if self.s(pos, 12).is_empty() {
            Ok(None)
        } else {
            self.account(pos).map(Some)
        }
    }
}

fn acc(code: &str) -> AccountCode {
    AccountCode::new(code).unwrap_or_else(|_| unreachable!("cuenta fija válida"))
}

struct OpenInvoice {
    date: Date,
    concept: String,
    /// Libro de ventas (1) o de compras/bienes de inversión.
    sales: bool,
    /// Cabecera 2: los importes van al revés.
    credit_note: bool,
    lines: Vec<EntryLine>,
    start: usize,
}

/// Lee el fichero. Cada asiento ha de cuadrar; si no, error con la línea.
pub fn read(bytes: &[u8]) -> Result<ReadFile, ReadError> {
    let text = decode(bytes);
    let mut out = ReadFile {
        company: 0,
        accounts: Vec::new(),
        entries: Vec::new(),
        invoices: 0,
        ignored: BTreeMap::new(),
    };
    let mut open: Option<(Date, String, Vec<EntryLine>, usize)> = None;
    let mut invoice: Option<OpenInvoice> = None;
    let mut records = 0;

    let finish = |e: JournalEntry, line: usize, out: &mut ReadFile| -> Result<(), ReadError> {
        e.validate().map_err(|err| ReadError::Line {
            line,
            msg: format!("el asiento «{}» no cuadra: {err}", e.concept),
        })?;
        out.entries.push(e);
        Ok(())
    };

    for (i, raw) in text.split('\n').enumerate() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let chars: Vec<char> = line.chars().collect();
        let r = Rec {
            n: i + 1,
            chars: &chars,
        };
        if !matches!(chars.first(), Some('3' | '4' | '5')) || chars.len() < 100 {
            return Err(r.err("no es un registro de SUENLACE (formato 4 o 5)"));
        }
        records += 1;
        if out.company == 0 {
            out.company = r.s(2, 5).parse().unwrap_or(0);
        }
        let kind = r.kind();
        // Un registro que no sea 9 (ni 4/5, que van tras la factura) cierra la factura abierta.
        if !matches!(kind, '9' | '4' | '5' | 'D')
            && let Some(inv) = invoice.take()
        {
            return Err(ReadError::Line {
                line: inv.start,
                msg: "factura sin línea 9 final (U)".into(),
            });
        }
        match kind {
            '0' => {
                let date = r.date()?;
                let account = r.account(16)?;
                let debit = r.s(58, 1) == "D";
                if !debit && r.s(58, 1) != "H" {
                    return Err(r.err("el apunte no es ni D ni H"));
                }
                let amount = r.amount(100)?;
                let concept = r.s(70, 30);
                let pos = r.s(69, 1);
                let document = r.s(59, 10);
                if pos == "I" {
                    if open.is_some() {
                        return Err(r.err("asiento sin cerrar (falta la U) antes de este"));
                    }
                    let c = if document.is_empty() {
                        concept.clone()
                    } else {
                        format!("{concept} ({document})")
                    };
                    open = Some((date, c, Vec::new(), r.n));
                }
                let Some((_, _, lines, _)) = open.as_mut() else {
                    return Err(r.err("apunte fuera de un asiento (falta la I)"));
                };
                if let Some(l) = EntryLine::signed(account, amount, debit) {
                    lines.push(EntryLine {
                        description: Some(concept).filter(|c| !c.is_empty()),
                        ..l
                    });
                }
                if pos == "U" {
                    let (date, concept, lines, start) =
                        open.take().unwrap_or_else(|| unreachable!());
                    finish(
                        JournalEntry {
                            date,
                            concept,
                            origin: Origin::Manual,
                            lines,
                        },
                        start,
                        &mut out,
                    )?;
                }
            }
            '1' | '2' => {
                if open.is_some() {
                    return Err(r.err("factura dentro de un asiento sin cerrar"));
                }
                let sales = r.s(58, 1) == "1";
                let credit_note = kind == '2';
                let total = r.amount(100)?;
                let number = {
                    let long = if chars.len() >= 312 {
                        r.s(253, 60)
                    } else {
                        String::new()
                    };
                    if long.is_empty() { r.s(59, 10) } else { long }
                };
                let concept = {
                    let c = r.s(70, 30);
                    if c.is_empty() {
                        format!("Factura {number}")
                    } else {
                        c
                    }
                };
                let mut lines = Vec::new();
                let party_debit = sales != credit_note;
                lines.extend(EntryLine::signed(r.account(16)?, total, party_debit));
                invoice = Some(OpenInvoice {
                    date: r.date()?,
                    concept,
                    sales,
                    credit_note,
                    lines,
                    start: r.n,
                });
            }
            '9' => {
                let Some(inv) = invoice.as_mut() else {
                    return Err(r.err("línea de IVA sin cabecera de factura"));
                };
                // Lado natural de la base: haber en ventas, debe en compras (al revés en abonos).
                let natural_debit = !inv.sales != inv.credit_note;
                let base_debit = if r.s(58, 1) == "A" {
                    !natural_debit
                } else {
                    natural_debit
                };
                let (vat_default, wh_default) = if inv.sales {
                    ("477", "473")
                } else {
                    ("472", "4751")
                };
                let vat_acc = r.opt_account(192)?.unwrap_or_else(|| acc(vat_default));
                let sur_acc = r.opt_account(204)?.unwrap_or_else(|| vat_acc.clone());
                let wh_acc = r.opt_account(216)?.unwrap_or_else(|| acc(wh_default));
                inv.lines.extend(EntryLine::signed(
                    r.account(16)?,
                    r.amount(102)?,
                    base_debit,
                ));
                inv.lines
                    .extend(EntryLine::signed(vat_acc, r.amount(121)?, base_debit));
                inv.lines
                    .extend(EntryLine::signed(sur_acc, r.amount(140)?, base_debit));
                inv.lines
                    .extend(EntryLine::signed(wh_acc, r.amount(159)?, !base_debit));
                if r.s(69, 1) == "U" {
                    let inv = invoice.take().unwrap_or_else(|| unreachable!());
                    finish(
                        JournalEntry {
                            date: inv.date,
                            concept: inv.concept,
                            origin: Origin::Manual,
                            lines: inv.lines,
                        }
                        .compact(),
                        inv.start,
                        &mut out,
                    )?;
                    out.invoices += 1;
                }
            }
            'C' => {
                let nif = r.s(78, 14);
                out.accounts.push(ReadAccount {
                    code: r.account(16)?,
                    name: r.s(28, 30),
                    nif: Some(nif).filter(|n| !n.is_empty()),
                });
            }
            other => *out.ignored.entry(other).or_insert(0) += 1,
        }
    }
    if let Some((_, _, _, start)) = open {
        return Err(ReadError::Line {
            line: start,
            msg: "el último asiento no termina (falta la U)".into(),
        });
    }
    if let Some(inv) = invoice {
        return Err(ReadError::Line {
            line: inv.start,
            msg: "la última factura no termina (falta la U)".into(),
        });
    }
    if records == 0 {
        return Err(ReadError::Empty);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use time::macros::date;

    use super::*;
    use crate::accounting::suenlace::{
        AccountRecord, Apunte, Format, InvoiceBook, InvoiceHeader, LinePos, VatLine, Writer,
    };
    use crate::money::Rate;

    fn sample(format: Format) -> Vec<u8> {
        let mut w = Writer::with_format(1, format).unwrap_or_else(|e| panic!("{e}"));
        let d = date!(2026 - 02 - 10);
        w.account(&AccountRecord {
            date: d,
            account: "43000001",
            name: "Peña SL",
            nif: Some("B12345674"),
            postal_code: None,
            town: None,
            province: None,
        });
        for (i, (acc, debit)) in [("62900000", true), ("57200001", false)].iter().enumerate() {
            w.apunte(&Apunte {
                date: d,
                account: acc,
                account_name: "x",
                debit: *debit,
                document: "7",
                pos: LinePos::of(i, 2),
                concept: "Gestoría",
                amount: Money::from_euros(100, 0),
            })
            .unwrap_or_else(|e| panic!("{e}"));
        }
        w.invoice(&InvoiceHeader {
            date: d,
            rectifying: false,
            account: "43000001",
            name: "Peña SL",
            book: InvoiceBook::Sales,
            number: "A2026-0001-LARGA",
            concept: "Servicios",
            total: Money::from_euros(1060, 0),
            nif: None,
            postal_code: None,
            operation_date: None,
            invoice_date: d,
        })
        .unwrap_or_else(|e| panic!("{e}"));
        w.vat_line(&VatLine {
            date: d,
            account: "70500000",
            account_name: "Servicios",
            natural: true,
            number: "A2026-0001",
            last: true,
            concept: "Servicios",
            subtype: 1,
            base: Money::from_euros(1000, 0),
            vat_rate: Rate::percent(21),
            vat: Money::from_euros(210, 0),
            surcharge_rate: Rate::ZERO,
            surcharge: Money::ZERO,
            withholding_rate: Rate::percent(15),
            withholding: Money::from_euros(150, 0),
            form: Some(1),
            subject: true,
            cash_basis: false,
            vat_account: Some("47700000"),
            surcharge_account: None,
            withholding_account: None,
        })
        .unwrap_or_else(|e| panic!("{e}"));
        w.rectified(d, d, "X", None);
        w.finish()
    }

    fn bal(e: &JournalEntry, acc: &str) -> i64 {
        e.lines
            .iter()
            .filter(|l| l.account.as_str() == acc)
            .map(|l| l.debit.cents() - l.credit.cents())
            .sum()
    }

    #[test]
    fn lee_lo_que_escribe_en_los_dos_formatos() {
        for f in [Format::F4, Format::F5] {
            let r = read(&sample(f)).unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(r.company, 1);
            assert_eq!(r.accounts.len(), 1);
            assert_eq!(r.accounts[0].name, "Peña SL");
            assert_eq!(r.accounts[0].nif.as_deref(), Some("B12345674"));
            assert_eq!(r.entries.len(), 2);
            assert_eq!(r.invoices, 1);
            assert_eq!(r.ignored.get(&'4'), Some(&1), "el registro 4 se ignora");
            let a = &r.entries[0];
            assert_eq!(a.concept, "Gestoría (7)");
            assert_eq!(bal(a, "62900000"), 10_000);
            let f = &r.entries[1];
            assert_eq!(bal(f, "43000001"), 106_000);
            assert_eq!(bal(f, "70500000"), -100_000);
            assert_eq!(bal(f, "47700000"), -21_000);
            assert_eq!(bal(f, "473"), 15_000);
        }
    }

    #[test]
    fn errores_con_su_linea() {
        let mut bytes = sample(Format::F5);
        // La última línea de IVA pasa de U a M: la factura no termina.
        let pos = 512 * 4 + 68;
        bytes[pos] = b'M';
        assert!(matches!(read(&bytes), Err(ReadError::Line { line: 4, .. })));
        assert_eq!(
            read(b"hola\r\n"),
            Err(ReadError::Line {
                line: 1,
                msg: "no es un registro de SUENLACE (formato 4 o 5)".into()
            })
        );
        assert_eq!(read(b"\r\n"), Err(ReadError::Empty));
    }
}
