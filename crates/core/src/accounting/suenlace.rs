//! Fichero `SUENLACE.DAT` de a3ASESOR | eco y | con («Enlace contable de entrada»): lo que
//! importa a3 con «Utilidades / Importar/Exportar / Enlace Contable». Sirve para pasar la
//! contabilidad de TaxPain a una gestoría que trabaja con a3.
//!
//! Diseño: PDF públicos de Wolters Kluwer, transcritos en `docs/research/CONTABILIDAD_MERCADO.md`
//! §2.1 (formato 4) y `docs/research/A3_FORMATOS.md` (formato 5):
//! - formato `5` (el vigente, «adaptado al SII»): registros de 512 bytes (510 de datos + CR LF),
//!   moneda en la 509, `N` en la 510 y número de factura completo (60) en la 253;
//! - formato `4`: registros de 256 bytes (254 + CR LF), moneda en la 253 y `N` en la 254.
//!
//! Las posiciones 1–252 de los registros 0, 1, 2, 9 y C son iguales en los dos. Importes
//! `±0000000000.00`; moneda `E`.
//!
//! `TODO(VERIFY)`: juego de caracteres. a3 es una aplicación de Windows: se escribe en
//! Windows-1252 (los caracteres que no caben se sustituyen por `?`).

use time::Date;

use crate::money::{Money, Rate};

/// Versión del fichero (posición 1 de cada registro).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Format {
    /// 256 bytes por registro (PDF de 2016).
    F4,
    /// 512 bytes por registro, adaptado al SII (PDF de 2018 a 2024).
    #[default]
    F5,
}

impl Format {
    /// Bytes de datos de cada registro (sin el CR LF final).
    fn data_len(self) -> usize {
        match self {
            Format::F4 => 254,
            Format::F5 => 510,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SuenlaceError {
    #[error("código de empresa de a3 fuera de rango (1–99999): {0}")]
    Company(u32),
    #[error("la cuenta {account} tiene más dígitos que el nivel de a3 ({digits})")]
    AccountTooLong { account: String, digits: usize },
    #[error("nivel de cuentas de a3 fuera de rango (6–12): {0}")]
    Digits(usize),
    #[error("importe demasiado grande para a3: {0}")]
    Amount(Money),
}

/// Carácter en Windows-1252 (un byte), o `?`.
fn cp1252(c: char) -> u8 {
    match c {
        '€' => 0x80,
        c if (c as u32) < 0x80 || (0xA0..=0xFF).contains(&(c as u32)) => {
            u8::try_from(c as u32).unwrap_or(b'?')
        }
        _ => b'?',
    }
}

/// Importe con signo, 10 enteros, punto y 2 decimales (14 caracteres).
fn amount(m: Money) -> Result<String, SuenlaceError> {
    let c = m.cents();
    if c.unsigned_abs() >= 1_000_000_000_000 {
        return Err(SuenlaceError::Amount(m));
    }
    let sign = if c < 0 { '-' } else { '+' };
    let a = c.unsigned_abs();
    Ok(format!("{sign}{:010}.{:02}", a / 100, a % 100))
}

/// Porcentaje `xx.xx` (5 caracteres).
fn percent(r: Rate) -> String {
    let bp = r.bp().unsigned_abs();
    format!("{:02}.{:02}", (bp / 100) % 100, bp % 100)
}

fn yyyymmdd(d: Date) -> String {
    format!("{:04}{:02}{:02}", d.year(), d.month() as u8, d.day())
}

/// Cuenta al nivel de a3 (`digits` dígitos), como hace a3 con el punto («572.1»):
/// - hasta 4 dígitos es una cuenta del PGC y se completa con ceros a la derecha
///   (`705` → `70500000`);
/// - con más, es una subcuenta (cuenta base + número, `AccountCode::subaccount`) y los ceros van
///   entre la base y el número (`4300001` → `43000001`). La base es de 3 dígitos, o de 4 si el
///   cuarto no es cero (`4751005` → `47510005`).
pub fn expand_account(code: &str, digits: usize) -> Result<String, SuenlaceError> {
    if !(6..=12).contains(&digits) {
        return Err(SuenlaceError::Digits(digits));
    }
    if code.len() > digits || !code.is_ascii() {
        return Err(SuenlaceError::AccountTooLong {
            account: code.to_owned(),
            digits,
        });
    }
    let pad = "0".repeat(digits - code.len());
    if code.len() <= 4 {
        return Ok(format!("{code}{pad}"));
    }
    let base = if code.as_bytes()[3] == b'0' { 3 } else { 4 };
    Ok(format!("{}{pad}{}", &code[..base], &code[base..]))
}

/// Posición del apunte dentro del asiento.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinePos {
    First,
    Middle,
    Last,
}

impl LinePos {
    fn code(self) -> char {
        match self {
            LinePos::First => 'I',
            LinePos::Middle => 'M',
            LinePos::Last => 'U',
        }
    }

    /// Posición de la línea `i` de `n`. Un asiento siempre tiene al menos dos líneas.
    pub fn of(i: usize, n: usize) -> LinePos {
        if i == 0 {
            LinePos::First
        } else if i + 1 == n {
            LinePos::Last
        } else {
            LinePos::Middle
        }
    }
}

/// Apunte sin IVA (registro tipo `0`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Apunte<'a> {
    pub date: Date,
    /// Cuenta ya al nivel de a3 ([`expand_account`]).
    pub account: &'a str,
    /// Nombre de la cuenta (a3 lo usa sólo si la cuenta no existe).
    pub account_name: &'a str,
    pub debit: bool,
    pub document: &'a str,
    pub pos: LinePos,
    pub concept: &'a str,
    pub amount: Money,
}

/// Alta o modificación de cuenta, cliente o proveedor (registro tipo `C`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountRecord<'a> {
    /// Fecha de alta (decide el ejercicio).
    pub date: Date,
    pub account: &'a str,
    pub name: &'a str,
    pub nif: Option<&'a str>,
    pub postal_code: Option<&'a str>,
    pub town: Option<&'a str>,
    pub province: Option<&'a str>,
}

/// Libro de la factura (posición 58 de los registros 1 y 2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvoiceBook {
    Sales,
    Purchases,
    CapitalGoods,
}

/// Cabecera de factura con IVA (registro `1`, o `2` si es rectificativa).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvoiceHeader<'a> {
    /// Fecha del asiento.
    pub date: Date,
    pub rectifying: bool,
    /// Cuenta del cliente o proveedor.
    pub account: &'a str,
    pub name: &'a str,
    pub book: InvoiceBook,
    pub number: &'a str,
    pub concept: &'a str,
    /// Importe de la factura.
    pub total: Money,
    /// NIF y nombre para el libro registro y el 347 (si la cuenta es genérica o para fijarlos).
    pub nif: Option<&'a str>,
    pub postal_code: Option<&'a str>,
    pub operation_date: Option<Date>,
    pub invoice_date: Date,
}

/// Línea de IVA (registro `9`) tras una cabecera 1 o 2.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VatLine<'a> {
    pub date: Date,
    /// Cuenta de ventas o de compras/gastos.
    pub account: &'a str,
    pub account_name: &'a str,
    /// `true`: la base va a su columna natural (`C`); `false`: a la contraria (`A`).
    pub natural: bool,
    pub number: &'a str,
    pub last: bool,
    pub concept: &'a str,
    /// Subtipo de a3 (01 interior, 03 intracomunitaria, 04 ISP…).
    pub subtype: u8,
    pub base: Money,
    pub vat_rate: Rate,
    pub vat: Money,
    pub surcharge_rate: Rate,
    pub surcharge: Money,
    pub withholding_rate: Rate,
    pub withholding: Money,
    /// Impreso al que va (01 = 347, 02 = 349…), si alguno.
    pub form: Option<u8>,
    pub subject: bool,
    pub cash_basis: bool,
    pub vat_account: Option<&'a str>,
    pub surcharge_account: Option<&'a str>,
    pub withholding_account: Option<&'a str>,
}

/// Escritor del fichero, para una empresa de a3.
#[derive(Debug)]
pub struct Writer {
    company: u32,
    format: Format,
    out: Vec<u8>,
}

impl Writer {
    /// Escritor en el formato vigente (5).
    pub fn new(company: u32) -> Result<Writer, SuenlaceError> {
        Writer::with_format(company, Format::F5)
    }

    pub fn with_format(company: u32, format: Format) -> Result<Writer, SuenlaceError> {
        if !(1..=99_999).contains(&company) {
            return Err(SuenlaceError::Company(company));
        }
        Ok(Writer {
            company,
            format,
            out: Vec::new(),
        })
    }

    /// Registro con cabecera común y los campos `(posición 1-based, longitud, texto)`.
    fn record(&mut self, date: Date, kind: char, fields: &[(usize, usize, &str)]) {
        let len = self.format.data_len();
        let mut rec = vec![b' '; len];
        let mut put = |pos: usize, len: usize, text: &str| {
            for (i, c) in text.chars().take(len).enumerate() {
                if let Some(b) = rec.get_mut(pos - 1 + i) {
                    *b = cp1252(c);
                }
            }
        };
        put(
            1,
            1,
            match self.format {
                Format::F4 => "4",
                Format::F5 => "5",
            },
        );
        put(2, 5, &format!("{:05}", self.company));
        put(7, 8, &yyyymmdd(date));
        put(15, 1, &kind.to_string());
        for (pos, len, text) in fields {
            put(*pos, *len, text);
        }
        // Los registros 3 y 4 no llevan moneda (en el formato 4 su 253 es texto o reserva).
        if !matches!(kind, '3' | '4') {
            put(len - 1, 1, "E");
        }
        put(len, 1, "N");
        self.out.extend_from_slice(&rec);
        self.out.extend_from_slice(b"\r\n");
    }

    pub fn apunte(&mut self, a: &Apunte<'_>) -> Result<(), SuenlaceError> {
        let amt = amount(a.amount)?;
        self.record(
            a.date,
            '0',
            &[
                (16, 12, a.account),
                (28, 30, a.account_name),
                (58, 1, if a.debit { "D" } else { "H" }),
                (59, 10, a.document),
                (69, 1, &a.pos.code().to_string()),
                (70, 30, a.concept),
                (100, 14, &amt),
            ],
        );
        Ok(())
    }

    pub fn account(&mut self, c: &AccountRecord<'_>) {
        self.record(
            c.date,
            'C',
            &[
                (16, 12, c.account),
                (28, 30, c.name),
                (58, 1, "N"),
                (78, 14, c.nif.unwrap_or("")),
                (135, 20, c.town.unwrap_or("")),
                (155, 5, c.postal_code.unwrap_or("")),
                (160, 15, c.province.unwrap_or("")),
            ],
        );
    }

    pub fn invoice(&mut self, h: &InvoiceHeader<'_>) -> Result<(), SuenlaceError> {
        let total = amount(h.total)?;
        let book = match h.book {
            InvoiceBook::Sales => "1",
            InvoiceBook::Purchases => "2",
            InvoiceBook::CapitalGoods => "3",
        };
        let op = h.operation_date.map(yyyymmdd).unwrap_or_default();
        let inv = yyyymmdd(h.invoice_date);
        let nif = h.nif.unwrap_or("");
        let has_nif = !nif.is_empty();
        let nif_name = if has_nif { h.name } else { "" };
        let nif_cp = if has_nif {
            h.postal_code.unwrap_or("")
        } else {
            ""
        };
        self.record(
            h.date,
            if h.rectifying { '2' } else { '1' },
            &[
                (16, 12, h.account),
                (28, 30, h.name),
                (58, 1, book),
                (59, 10, h.number),
                (69, 1, "I"),
                (70, 30, h.concept),
                (100, 14, &total),
                (176, 14, nif),
                (190, 40, nif_name),
                (230, 5, nif_cp),
                (237, 8, &op),
                (245, 8, &inv),
                // Número completo para el SII (sólo formato 5; en el 4 se pisa con la moneda).
                (
                    253,
                    if self.format == Format::F5 { 60 } else { 0 },
                    h.number,
                ),
            ],
        );
        Ok(())
    }

    /// Datos de la factura rectificada (registro `4`), tras el último `9` de la cabecera `2`.
    /// `sii_type`: tipo de rectificativa del SII (formato 5): `T` art. 80.1, 80.2 y error
    /// fundado, `C` concurso, `D` incobrable, `R` resto, `J` ticket.
    pub fn rectified(
        &mut self,
        date: Date,
        original_date: Date,
        original_number: &str,
        sii_type: Option<char>,
    ) {
        let sii = sii_type.map(String::from).unwrap_or_default();
        match self.format {
            Format::F4 => self.record(
                date,
                '4',
                &[(59, 8, &yyyymmdd(original_date)), (67, 10, original_number)],
            ),
            Format::F5 => self.record(
                date,
                '4',
                &[
                    (59, 8, &yyyymmdd(original_date)),
                    (67, 60, original_number),
                    (157, 1, &sii),
                ],
            ),
        }
    }

    pub fn vat_line(&mut self, l: &VatLine<'_>) -> Result<(), SuenlaceError> {
        let (base, vat, sur, wh) = (
            amount(l.base)?,
            amount(l.vat)?,
            amount(l.surcharge)?,
            amount(l.withholding)?,
        );
        let subtype = format!("{:02}", l.subtype % 100);
        let form = l
            .form
            .map(|f| format!("{:02}", f % 100))
            .unwrap_or_default();
        self.record(
            l.date,
            '9',
            &[
                (16, 12, l.account),
                (28, 30, l.account_name),
                (58, 1, if l.natural { "C" } else { "A" }),
                (59, 10, l.number),
                (69, 1, if l.last { "U" } else { "M" }),
                (70, 30, l.concept),
                (100, 2, &subtype),
                (102, 14, &base),
                (116, 5, &percent(l.vat_rate)),
                (121, 14, &vat),
                (135, 5, &percent(l.surcharge_rate)),
                (140, 14, &sur),
                (154, 5, &percent(l.withholding_rate)),
                (159, 14, &wh),
                (173, 2, &form),
                (175, 1, if l.subject { "S" } else { "N" }),
                (176, 1, "N"),
                (177, 1, if l.cash_basis { "S" } else { "N" }),
                (192, 12, l.vat_account.unwrap_or("")),
                (204, 12, l.surcharge_account.unwrap_or("")),
                (216, 12, l.withholding_account.unwrap_or("")),
            ],
        );
        Ok(())
    }

    pub fn finish(self) -> Vec<u8> {
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn field(rec: &[u8], pos: usize, len: usize) -> String {
        String::from_utf8_lossy(&rec[pos - 1..pos - 1 + len]).into_owned()
    }

    #[test]
    fn importes_porcentajes_y_cuentas() {
        assert_eq!(
            amount(Money::from_cents(100_000)).ok().as_deref(),
            Some("+0000001000.00")
        );
        assert_eq!(
            amount(Money::from_cents(-5)).ok().as_deref(),
            Some("-0000000000.05")
        );
        assert!(amount(Money::from_cents(1_000_000_000_000)).is_err());
        assert_eq!(percent(Rate::percent(21)), "21.00");
        assert_eq!(percent(Rate::from_bp(520)), "05.20");
        let x = |c: &str, d: usize| expand_account(c, d).ok();
        assert_eq!(x("705", 8).as_deref(), Some("70500000"));
        assert_eq!(x("4300001", 8).as_deref(), Some("43000001"));
        assert_eq!(x("5720001", 8).as_deref(), Some("57200001"));
        assert_eq!(x("4751005", 8).as_deref(), Some("47510005"));
        assert_eq!(x("4751", 10).as_deref(), Some("4751000000"));
        assert_eq!(x("43000001", 8).as_deref(), Some("43000001"));
        assert!(x("4300000001", 8).is_none());
        assert!(x("705", 5).is_none());
    }

    #[test]
    fn registros_de_256_bytes_con_cabecera_comun() {
        let mut w = Writer::with_format(7, Format::F4).unwrap_or_else(|e| panic!("{e}"));
        w.apunte(&Apunte {
            date: date!(2026 - 03 - 31),
            account: "62900000",
            account_name: "Otros servicios",
            debit: true,
            document: "F-1",
            pos: LinePos::First,
            concept: "Gestoría marzo",
            amount: Money::from_euros(100, 0),
        })
        .unwrap_or_else(|e| panic!("{e}"));
        w.account(&AccountRecord {
            date: date!(2026 - 01 - 01),
            account: "41000001",
            name: "Peña Asesores SL",
            nif: Some("B12345674"),
            postal_code: Some("46001"),
            town: Some("València"),
            province: None,
        });
        let out = w.finish();
        assert_eq!(out.len(), 512);
        let (a, c) = out.split_at(256);
        assert_eq!(&a[254..], b"\r\n");
        assert_eq!(field(a, 1, 15), "400007202603310");
        assert_eq!(field(a, 16, 12), "62900000    ");
        assert_eq!(field(a, 58, 1), "D");
        assert_eq!(field(a, 69, 1), "I");
        assert_eq!(field(a, 100, 14), "+0000000100.00");
        assert_eq!(field(a, 253, 2), "EN");
        // «ñ» y «à» en Windows-1252: un byte cada una.
        assert_eq!(c[14], b'C');
        assert_eq!(c[27 + 2], 0xF1);
        assert_eq!(field(c, 78, 9), "B12345674");
        assert_eq!(field(c, 155, 5), "46001");
        assert_eq!(c[134 + 3], 0xE8);
    }

    #[test]
    fn factura_emitida_con_iva_y_retencion() {
        let mut w = Writer::new(1).unwrap_or_else(|e| panic!("{e}"));
        let d = date!(2026 - 02 - 10);
        w.invoice(&InvoiceHeader {
            date: d,
            rectifying: false,
            account: "43000001",
            name: "Cliente SL",
            book: InvoiceBook::Sales,
            number: "A2026-0001",
            concept: "Servicios",
            total: Money::from_euros(1060, 0),
            nif: Some("B12345674"),
            postal_code: Some("46001"),
            operation_date: None,
            invoice_date: d,
        })
        .unwrap_or_else(|e| panic!("{e}"));
        w.vat_line(&VatLine {
            date: d,
            account: "70500000",
            account_name: "Prestaciones de servicios",
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
            withholding_account: Some("47300000"),
        })
        .unwrap_or_else(|e| panic!("{e}"));
        w.rectified(d, date!(2026 - 01 - 15), "A2026-0000-LARGO", Some('R'));
        let out = w.finish();
        assert_eq!(out.len(), 3 * 512);
        let (h, rest) = out.split_at(512);
        let (l, r4) = rest.split_at(512);
        // Formato 5: tipo 5, número completo en la 253, moneda en la 509 y `N` en la 510.
        assert_eq!(field(h, 1, 1), "5");
        assert_eq!(field(h, 253, 10), "A2026-0001");
        assert_eq!(field(h, 509, 2), "EN");
        assert_eq!(&h[510..], b"\r\n");
        assert_eq!(field(r4, 15, 1), "4");
        assert_eq!(field(r4, 59, 24), "20260115A2026-0000-LARGO");
        assert_eq!(field(r4, 157, 1), "R");
        assert_eq!(field(r4, 509, 2), " N", "el registro 4 no lleva moneda");
        assert_eq!(field(h, 15, 1), "1");
        assert_eq!(field(h, 58, 12), "1A2026-0001I");
        assert_eq!(field(h, 176, 9), "B12345674");
        assert_eq!(field(h, 190, 10), "Cliente SL");
        assert_eq!(field(h, 245, 8), "20260210");
        assert_eq!(field(l, 15, 1), "9");
        assert_eq!(field(l, 58, 1), "C");
        assert_eq!(field(l, 69, 1), "U");
        assert_eq!(field(l, 100, 2), "01");
        assert_eq!(field(l, 102, 14), "+0000001000.00");
        assert_eq!(field(l, 116, 5), "21.00");
        assert_eq!(field(l, 154, 19), "15.00+0000000150.00");
        assert_eq!(field(l, 173, 5), "01SNN");
        assert_eq!(field(l, 192, 8), "47700000");
        assert_eq!(field(l, 216, 8), "47300000");
    }
}
