//! Legalización telemática de libros en el Registro Mercantil (art. 27 del Código de Comercio,
//! Instrucción DGRN de 12/2/2015): un ZIP con los libros, `NOMBRES.TXT` y `DATOS.TXT`, que se
//! presenta con certificado en registradores.org dentro de los cuatro meses siguientes al cierre.
//! TaxPain sólo prepara el paquete; la presentación la hace la persona usuaria.
//!
//! Libros (XLSX, formato admitido):
//! - `DIARIO_nnn` (nnn = número del libro): el diario del ejercicio con apertura, regularización y cierre (art. 28.2);
//!   completo o resumido por meses (una línea por cuenta y mes, como el «diario resumido» de a3).
//! - `INV_CUEN_nnn`: balance inicial, sumas y saldos de cada trimestre, inventario de cierre,
//!   balance y pérdidas y ganancias (art. 28.1).
//!
//! - `ACTASCON_nnn`, `SOCIOS_nnn` y `SOCUNICO_nnn` (PDF con folios, si se piden): libro de actas
//!   con las aprobadas del ejercicio, libro registro de socios y libro registro de contratos del
//!   socio único (`libros_societarios`). El de socios sólo es obligatorio el primer año y los
//!   ejercicios con cambios de titularidad o gravámenes (Instrucción DGRN de 12/2/2015).
//!
//! Formato de `DATOS.TXT`: ver `docs/research/LEGALIZACION.md` y los `TODO(VERIFY)` de
//! [`datos_txt`].

use std::collections::BTreeMap;
use std::io::Write as _;

use rust_xlsxwriter::{Format, Workbook, Worksheet, XlsxError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use taxpain_core::accounting::{AccountCode, EntryLine, JournalEntry, Origin, trial_balance};
use taxpain_core::invoice::PartyId;
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::rusqlite::OptionalExtension;
use time::{Date, Duration, Month, OffsetDateTime};

use crate::accounting::{self, StoredEntry};
use crate::error::{AppError, Result};

fn xlsx(e: XlsxError) -> AppError {
    AppError::Other(format!("XLSX: {e}"))
}

/// Datos de la solicitud (lo que no sale de la contabilidad).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub year: i32,
    /// Número de cada libro (sigue la numeración de los legalizados en años anteriores).
    pub diary_number: u16,
    pub inventory_number: u16,
    /// Diario resumido por meses (una línea por cuenta y mes).
    #[serde(default)]
    pub summarized: bool,
    /// Número del libro de actas, del registro de socios y del de contratos del socio único,
    /// si se añaden al paquete.
    #[serde(default)]
    pub minutes_number: Option<u16>,
    #[serde(default)]
    pub partners_number: Option<u16>,
    #[serde(default)]
    pub contracts_number: Option<u16>,
    /// Domicilio de la empresa (si no está en los datos de facturación).
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default)]
    pub town: Option<String>,
    #[serde(default)]
    pub postal_code: Option<String>,
    /// Código INE de la provincia (2 dígitos).
    #[serde(default)]
    pub province_code: Option<String>,
    /// Código del Registro Mercantil (5 dígitos) y datos registrales.
    #[serde(default)]
    pub registry_code: Option<String>,
    #[serde(default)]
    pub tomo: Option<String>,
    #[serde(default)]
    pub folio: Option<String>,
    #[serde(default)]
    pub hoja: Option<String>,
    /// Presentante, si no es la propia empresa.
    #[serde(default)]
    pub presenter_name: Option<String>,
    #[serde(default)]
    pub presenter_nif: Option<String>,
    #[serde(default)]
    pub presenter_email: Option<String>,
    #[serde(default)]
    pub presenter_phone: Option<String>,
}

/// Libro del paquete.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Book {
    pub file: String,
    pub description: String,
    pub number: u16,
    pub opened: Date,
    pub closed: Date,
    /// SHA-256 del fichero en Base64.
    pub huella: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Package {
    /// Nombre propuesto para el ZIP.
    pub filename: String,
    #[serde(skip)]
    pub zip: Vec<u8>,
    pub books: Vec<Book>,
    pub warnings: Vec<String>,
    /// Último día de plazo (cuatro meses tras el cierre del ejercicio).
    pub deadline: Date,
}

/// Base64 estándar (RFC 4648) con relleno.
pub fn base64(bytes: &[u8]) -> String {
    taxpain_core::base64::encode(bytes)
}

fn ddmmaaaa(d: Date) -> String {
    format!("{:02}{:02}{:04}", d.day(), u8::from(d.month()), d.year())
}

fn dmy(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

struct Sheet<'a> {
    ws: &'a mut Worksheet,
    row: u32,
    money: Format,
    bold: Format,
}

impl Sheet<'_> {
    fn text(&mut self, col: u16, s: &str) -> Result<()> {
        crate::celda_segura::texto(self.ws, self.row, col, s).map_err(xlsx)?;
        Ok(())
    }
    fn bold(&mut self, col: u16, s: &str) -> Result<()> {
        crate::celda_segura::texto_con_formato(self.ws, self.row, col, s, &self.bold)
            .map_err(xlsx)?;
        Ok(())
    }
    #[allow(clippy::float_arithmetic, clippy::cast_precision_loss)] // sólo para la celda
    fn money(&mut self, col: u16, m: Money) -> Result<()> {
        if !m.is_zero() {
            self.ws
                .write_number_with_format(self.row, col, m.cents() as f64 / 100.0, &self.money)
                .map_err(xlsx)?;
        }
        Ok(())
    }
    fn int(&mut self, col: u16, n: i64) -> Result<()> {
        #[allow(clippy::cast_precision_loss)]
        self.ws
            .write_number(self.row, col, n as f64)
            .map_err(xlsx)?;
        Ok(())
    }
    fn next(&mut self) {
        self.row += 1;
    }
    /// Cabecera común: empresa, NIF, libro y ejercicio.
    fn header(&mut self, who: &(String, String), title: &str, widths: &[f64]) -> Result<()> {
        for (i, w) in widths.iter().enumerate() {
            self.ws
                .set_column_width(u16::try_from(i).unwrap_or(0), *w)
                .map_err(xlsx)?;
        }
        self.bold(0, &who.0)?;
        self.next();
        self.text(0, &format!("NIF {}", who.1))?;
        self.next();
        self.bold(0, title)?;
        self.row += 2;
        Ok(())
    }
}

fn new_sheet<'a>(wb: &'a mut Workbook, name: &str) -> Result<Sheet<'a>> {
    let money = Format::new().set_num_format("#,##0.00");
    let bold = Format::new().set_bold();
    let ws = wb.add_worksheet();
    ws.set_name(name).map_err(xlsx)?;
    Ok(Sheet {
        ws,
        row: 0,
        money,
        bold,
    })
}

fn names(v: &Vault, codes: impl Iterator<Item = String>) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for c in codes {
        let n = accounting::account_name(v.connection(), &c)?.unwrap_or_default();
        out.insert(c, n);
    }
    Ok(out)
}

/// Diario resumido: cada mes, un asiento con el total al debe y al haber de cada cuenta. La
/// apertura, la regularización y el cierre van aparte, como están.
fn summarize(entries: &[StoredEntry]) -> Vec<(String, JournalEntry)> {
    let mut out = Vec::new();
    let mut months: BTreeMap<(i32, u8), BTreeMap<String, (Money, Money)>> = BTreeMap::new();
    for e in entries {
        if matches!(
            e.entry.origin,
            Origin::Apertura | Origin::Regularizacion | Origin::Cierre
        ) {
            continue;
        }
        let m = months
            .entry((e.entry.date.year(), u8::from(e.entry.date.month())))
            .or_default();
        for l in &e.entry.lines {
            let slot = m.entry(l.account.as_str().to_owned()).or_default();
            slot.0 += l.debit;
            slot.1 += l.credit;
        }
    }
    let special = |o: &Origin| -> Vec<(String, JournalEntry)> {
        entries
            .iter()
            .filter(|e| &e.entry.origin == o)
            .map(|e| (String::new(), e.entry.clone()))
            .collect()
    };
    out.extend(special(&Origin::Apertura));
    for ((y, m), accs) in months {
        let month = Month::try_from(m).unwrap_or(Month::January);
        let last = Date::from_calendar_date(y, month, month.length(y))
            .unwrap_or_else(|_| unreachable!("fin de mes válido"));
        let mut lines = Vec::new();
        for (acc, (d, c)) in accs {
            let Ok(code) = AccountCode::new(&acc) else {
                continue;
            };
            if !d.is_zero() {
                lines.push(EntryLine::debit(code.clone(), d));
            }
            if !c.is_zero() {
                lines.push(EntryLine::credit(code, c));
            }
        }
        out.push((
            String::new(),
            JournalEntry {
                date: last,
                concept: format!("Resumen de {} de {y}", MONTHS[usize::from(m - 1)]),
                origin: Origin::Manual,
                lines,
            },
        ));
    }
    out.extend(special(&Origin::Regularizacion));
    out.extend(special(&Origin::Cierre));
    out
}

const MONTHS: [&str; 12] = [
    "enero",
    "febrero",
    "marzo",
    "abril",
    "mayo",
    "junio",
    "julio",
    "agosto",
    "septiembre",
    "octubre",
    "noviembre",
    "diciembre",
];

fn diary(
    v: &Vault,
    who: &(String, String),
    year: i32,
    number: u16,
    entries: &[StoredEntry],
    summarized: bool,
) -> Result<Vec<u8>> {
    let rows: Vec<(String, JournalEntry)> = if summarized {
        summarize(entries)
    } else {
        entries
            .iter()
            .map(|e| (String::new(), e.entry.clone()))
            .collect()
    };
    let all_accounts = rows
        .iter()
        .flat_map(|(_, e)| e.lines.iter().map(|l| l.account.as_str().to_owned()));
    let names = names(v, all_accounts)?;
    let mut wb = Workbook::new();
    let mut s = new_sheet(&mut wb, "Diario")?;
    s.header(
        who,
        &format!(
            "Libro Diario{} · Ejercicio {year} · Libro nº {number}",
            if summarized {
                " (resumido por meses)"
            } else {
                ""
            }
        ),
        &[9.0, 11.0, 12.0, 36.0, 44.0, 14.0, 14.0],
    )?;
    for (i, h) in [
        "Asiento",
        "Fecha",
        "Cuenta",
        "Título de la cuenta",
        "Concepto",
        "Debe",
        "Haber",
    ]
    .iter()
    .enumerate()
    {
        s.bold(u16::try_from(i).unwrap_or(0), h)?;
    }
    s.next();
    let (mut td, mut tc) = (Money::ZERO, Money::ZERO);
    for (n, (_, e)) in rows.iter().enumerate() {
        for (j, l) in e.lines.iter().enumerate() {
            if j == 0 {
                s.int(0, i64::try_from(n + 1).unwrap_or(0))?;
                s.text(1, &dmy(e.date))?;
            }
            s.text(2, l.account.as_str())?;
            s.text(3, names.get(l.account.as_str()).map_or("", String::as_str))?;
            s.text(4, l.description.as_deref().unwrap_or(&e.concept))?;
            s.money(5, l.debit)?;
            s.money(6, l.credit)?;
            td += l.debit;
            tc += l.credit;
            s.next();
        }
    }
    s.next();
    s.bold(4, "Totales")?;
    s.money(5, td)?;
    s.money(6, tc)?;
    wb.save_to_buffer().map_err(xlsx)
}

fn balances_sheet(
    wb: &mut Workbook,
    v: &Vault,
    who: &(String, String),
    name: &str,
    title: &str,
    entries: &[JournalEntry],
    filter: impl Fn(&str) -> bool,
) -> Result<()> {
    let tb: Vec<_> = trial_balance(entries)
        .into_iter()
        .filter(|b| filter(b.account.as_str()) && (!b.debit.is_zero() || !b.credit.is_zero()))
        .collect();
    let names = names(v, tb.iter().map(|b| b.account.as_str().to_owned()))?;
    let mut s = new_sheet(wb, name)?;
    s.header(who, title, &[12.0, 40.0, 15.0, 15.0, 15.0, 15.0])?;
    for (i, h) in [
        "Cuenta",
        "Título",
        "Suma debe",
        "Suma haber",
        "Saldo deudor",
        "Saldo acreedor",
    ]
    .iter()
    .enumerate()
    {
        s.bold(u16::try_from(i).unwrap_or(0), h)?;
    }
    s.next();
    let mut t = [Money::ZERO; 4];
    for b in &tb {
        let bal = b.balance();
        let (sd, sa) = if bal.is_negative() {
            (Money::ZERO, -bal)
        } else {
            (bal, Money::ZERO)
        };
        s.text(0, b.account.as_str())?;
        s.text(1, names.get(b.account.as_str()).map_or("", String::as_str))?;
        s.money(2, b.debit)?;
        s.money(3, b.credit)?;
        s.money(4, sd)?;
        s.money(5, sa)?;
        for (k, m) in [b.debit, b.credit, sd, sa].into_iter().enumerate() {
            t[k] += m;
        }
        s.next();
    }
    if tb.is_empty() {
        s.text(0, "Sin movimientos.")?;
        s.next();
    }
    s.next();
    s.bold(1, "Totales")?;
    for (k, m) in t.into_iter().enumerate() {
        s.money(u16::try_from(k + 2).unwrap_or(2), m)?;
    }
    Ok(())
}

fn inventory(
    v: &Vault,
    who: &(String, String),
    year: i32,
    number: u16,
    entries: &[StoredEntry],
    period: (Date, Date),
) -> Result<Vec<u8>> {
    let all: Vec<JournalEntry> = entries.iter().map(|e| e.entry.clone()).collect();
    let mut wb = Workbook::new();
    let opening: Vec<JournalEntry> = all
        .iter()
        .filter(|e| e.origin == Origin::Apertura)
        .cloned()
        .collect();
    let label =
        format!("Libro de Inventarios y Cuentas anuales · Ejercicio {year} · Libro nº {number}");
    balances_sheet(
        &mut wb,
        v,
        who,
        "Balance inicial",
        &format!("{label} · Balance inicial"),
        &opening,
        |_| true,
    )?;
    let operations: Vec<JournalEntry> = all
        .iter()
        .filter(|e| !matches!(e.origin, Origin::Regularizacion | Origin::Cierre))
        .cloned()
        .collect();
    // Balances de comprobación al menos trimestrales (art. 28.1 CCom), por trimestres del
    // ejercicio: que puede no coincidir con el año natural (de julio a junio) o durar menos de
    // doce meses (transición al cambiar la fecha de cierre). El último acaba con el ejercicio.
    let (start, last) = period;
    let quarter_end = |q: u8| {
        (crate::recurring::months_after(start, 3 * u32::from(q)) - Duration::days(1)).min(last)
    };
    for q in (1u8..=4).take_while(|q| *q == 1 || quarter_end(q - 1) < last) {
        let end = quarter_end(q);
        let upto: Vec<JournalEntry> = operations
            .iter()
            .filter(|e| e.date <= end)
            .cloned()
            .collect();
        balances_sheet(
            &mut wb,
            v,
            who,
            &format!("Sumas y saldos {q}T"),
            &format!("{label} · Sumas y saldos a {}", dmy(end)),
            &upto,
            |_| true,
        )?;
    }
    // Inventario de cierre: saldos patrimoniales (grupos 1 a 5) tras regularizar.
    let before_close: Vec<JournalEntry> = all
        .iter()
        .filter(|e| e.origin != Origin::Cierre)
        .cloned()
        .collect();
    balances_sheet(
        &mut wb,
        v,
        who,
        "Inventario de cierre",
        &format!("{label} · Inventario de cierre"),
        &before_close,
        |a| matches!(a.as_bytes().first(), Some(b'1'..=b'5')),
    )?;
    let ca = accounting::annual_accounts(v, year)?;
    for (name, st) in [
        ("Balance", &ca.balance),
        ("Pérdidas y ganancias", &ca.income),
    ] {
        let mut s = new_sheet(&mut wb, name)?;
        s.header(
            who,
            &format!("{label} · {name} (modelo PYMES)"),
            &[70.0, 16.0],
        )?;
        for l in &st.lines {
            if l.leaf {
                s.text(0, &l.label)?;
            } else {
                s.bold(0, &l.label)?;
            }
            s.money(1, l.amount)?;
            s.next();
        }
    }
    wb.save_to_buffer().map_err(xlsx)
}

/// Texto ASCII (sin tildes ni eñes) cortado a `len` caracteres.
fn ascii(s: &str, len: usize) -> String {
    s.trim()
        .chars()
        .map(|c| match c {
            'á' | 'à' | 'ä' | 'â' => 'a',
            'Á' | 'À' | 'Ä' | 'Â' => 'A',
            'é' | 'è' | 'ë' | 'ê' => 'e',
            'É' | 'È' | 'Ë' | 'Ê' => 'E',
            'í' | 'ì' | 'ï' | 'î' => 'i',
            'Í' | 'Ì' | 'Ï' | 'Î' => 'I',
            'ó' | 'ò' | 'ö' | 'ô' => 'o',
            'Ó' | 'Ò' | 'Ö' | 'Ô' => 'O',
            'ú' | 'ù' | 'ü' | 'û' => 'u',
            'Ú' | 'Ù' | 'Ü' | 'Û' => 'U',
            'ñ' => 'n',
            'Ñ' => 'N',
            'ç' => 'c',
            'Ç' => 'C',
            c if c.is_ascii() && !c.is_ascii_control() => c,
            _ => ' ',
        })
        .take(len)
        .collect()
}

/// `DATOS.TXT`: un registro por dato, «código» seguido del valor sin separador ni relleno, en
/// ASCII y con CR LF (Anexo I de la Instrucción DGRN de 12/2/2015; sintaxis como Legalia 2, ver
/// `docs/research/LEGALIZACION.md`). Los datos de cabecera vacíos no se escriben; los seis de
/// cada libro, siempre. `TODO(VERIFY)`: datos vacíos de cabecera y juego de caracteres.
pub fn datos_txt(fields: &[(String, usize, String)]) -> String {
    let mut out = String::new();
    for (code, len, value) in fields {
        let v = ascii(value, *len);
        let book_field = code.len() == 5;
        if v.is_empty() && !book_field {
            continue;
        }
        out.push_str(code);
        out.push_str(&v);
        out.push_str("\r\n");
    }
    out
}

/// Prepara el paquete de legalización del ejercicio (que ha de estar cerrado).
pub fn build(v: &Vault, r: &Request, now: OffsetDateTime) -> Result<Package> {
    let year = r.year;
    let closed: Option<i64> = v
        .connection()
        .query_row(
            "SELECT closed FROM fiscal_year WHERE year = ?1",
            [year],
            |row| row.get(0),
        )
        .optional()?;
    if closed != Some(1) {
        return Err(AppError::Other(format!(
            "cierra antes el ejercicio {year}: los libros se legalizan con la regularización y el cierre"
        )));
    }
    if r.diary_number == 0 || r.inventory_number == 0 {
        return Err(AppError::Other("el número de libro empieza en 1".into()));
    }
    let inv = crate::invoicing::settings(v)?.ok_or(AppError::NotConfigured)?;
    let nif = match &inv.issuer.id {
        PartyId::Nif(n) => n.clone(),
        PartyId::Foreign { id, .. } => id.clone(),
    };
    let who = (inv.issuer.name.clone(), nif.clone());
    let entries = accounting::journal(v, year)?;
    let fy = accounting::fiscal_year(v, year)?;
    let (start, end) = (fy.start, fy.end);
    // Ley 14/2013 art. 18: cuatro meses desde el cierre, de fecha a fecha (CC art. 5.1).
    let deadline = taxpain_core::calendar::rules::add_months(end, 4);

    let mut warnings = Vec::new();
    let mut files: Vec<(String, String, u16, Vec<u8>)> = vec![
        (
            format!("DIARIO_{:03}.xlsx", r.diary_number),
            format!("Libro Diario {year}"),
            r.diary_number,
            diary(v, &who, year, r.diary_number, &entries, r.summarized)?,
        ),
        (
            format!("INV_CUEN_{:03}.xlsx", r.inventory_number),
            format!("Libro de Inventarios y Cuentas anuales {year}"),
            r.inventory_number,
            inventory(v, &who, year, r.inventory_number, &entries, (start, end))?,
        ),
    ];
    corporate_books(v, r, (start, end), &mut files, &mut warnings)?;
    let books: Vec<Book> = files
        .iter()
        .map(|(f, d, n, bytes)| Book {
            file: f.clone(),
            description: d.clone(),
            number: *n,
            opened: start,
            closed: end,
            huella: base64(&Sha256::digest(bytes)),
        })
        .collect();

    let addr = inv.issuer.address.as_ref();
    let pick = |given: &Option<String>, fallback: Option<&String>| {
        given
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .or_else(|| fallback.cloned())
            .unwrap_or_default()
    };
    let address = pick(&r.address, addr.map(|a| &a.street));
    let town = pick(&r.town, addr.map(|a| &a.town));
    let postal = pick(&r.postal_code, addr.map(|a| &a.post_code));
    let province = pick(&r.province_code, None)
        .chars()
        .take(2)
        .collect::<String>();
    let province = if province.is_empty() {
        postal.chars().take(2).collect()
    } else {
        province
    };
    if address.is_empty() || town.is_empty() || postal.is_empty() {
        warnings.push("falta el domicilio de la empresa (calle, ciudad o código postal)".into());
    }
    if r.registry_code
        .as_deref()
        .is_none_or(|c| c.trim().is_empty())
    {
        warnings.push(
            "falta el código del Registro Mercantil: lo pide la aplicación del Registro al presentar"
                .into(),
        );
    }

    let address_for_presenter = (
        address.clone(),
        town.clone(),
        postal.clone(),
        addr.map(|a| a.province.clone()).unwrap_or_default(),
    );
    let mut f: Vec<(String, usize, String)> = Vec::new();
    let mut put = |code: &str, len: usize, value: String| f.push((code.into(), len, value));
    put("101", 8, ddmmaaaa(now.date()));
    put("102", 100, inv.issuer.name.clone());
    put("105", 9, nif.clone());
    put("106", 32, address);
    put("107", 32, town);
    put("108", 5, postal);
    put("109", 2, province);
    put("111", 10, r.presenter_phone.clone().unwrap_or_default());
    put("112", 5, r.registry_code.clone().unwrap_or_default());
    put("201", 6, r.tomo.clone().unwrap_or_default());
    put("204", 6, r.folio.clone().unwrap_or_default());
    let registered = [&r.tomo, &r.folio, &r.hoja]
        .iter()
        .any(|x| x.as_deref().is_some_and(|s| !s.trim().is_empty()));
    put(
        "205",
        32,
        if registered {
            "REGISTRO MERCANTIL".into()
        } else {
            String::new()
        },
    );
    put("206", 6, r.hoja.clone().unwrap_or_default());
    put(
        "301",
        100,
        r.presenter_name
            .clone()
            .unwrap_or_else(|| inv.issuer.name.clone()),
    );
    put(
        "304",
        9,
        r.presenter_nif.clone().unwrap_or_else(|| nif.clone()),
    );
    if r.presenter_name
        .as_deref()
        .is_none_or(|n| n.trim().is_empty())
    {
        // Presenta la propia empresa: su domicilio.
        put("305", 32, address_for_presenter.0);
        put("306", 32, address_for_presenter.1);
        put("307", 5, address_for_presenter.2);
        put("308", 32, address_for_presenter.3);
    }
    put("311", 60, r.presenter_email.clone().unwrap_or_default());
    put("401", 2, "NO".into());
    put("501", 3, books.len().to_string());
    for (i, b) in books.iter().enumerate() {
        let x = format!("{:03}", i + 1);
        put(&format!("{x}01"), 100, b.description.clone());
        put(&format!("{x}02"), 3, b.number.to_string());
        put(&format!("{x}03"), 8, ddmmaaaa(b.opened));
        put(&format!("{x}04"), 8, ddmmaaaa(b.closed));
        put(&format!("{x}05"), 8, String::new());
        put(&format!("{x}06"), 64, b.huella.clone());
    }
    let datos = datos_txt(&f);
    let nombres: String = books.iter().map(|b| format!("{}\r\n", b.file)).collect();

    // ZIP 2.0 con Deflate estándar, todo en la raíz, sin cifrar.
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(false);
    let mut add = |name: &str, bytes: &[u8]| -> Result<()> {
        zip.start_file(name, opts)
            .map_err(|e| AppError::Other(format!("ZIP: {e}")))?;
        zip.write_all(bytes)
            .map_err(|e| AppError::Other(format!("ZIP: {e}")))?;
        Ok(())
    };
    for (name, _, _, bytes) in &files {
        add(name, bytes)?;
    }
    add("NOMBRES.TXT", nombres.as_bytes())?;
    add("DATOS.TXT", datos.as_bytes())?;
    let zip = zip
        .finish()
        .map_err(|e| AppError::Other(format!("ZIP: {e}")))?
        .into_inner();
    if now.date() > deadline {
        warnings.push(format!(
            "el plazo terminó el {} (cuatro meses tras el cierre): el Registro puede legalizarlos igualmente, pero fuera de plazo",
            dmy(deadline)
        ));
    }
    Ok(Package {
        filename: format!("LEGALIZACION_{year}_{nif}.ZIP"),
        zip,
        books,
        warnings,
        deadline,
    })
}

/// Añade los libros societarios pedidos (`ACTASCON`, `SOCIOS`, `SOCUNICO`) en PDF con folios.
fn corporate_books(
    v: &Vault,
    r: &Request,
    (start, end): (Date, Date),
    files: &mut Vec<(String, String, u16, Vec<u8>)>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    use crate::libros_societarios::{self as ls, BookKind};
    let year = r.year;
    for (kind, number) in [
        (BookKind::Actas, r.minutes_number),
        (BookKind::Socios, r.partners_number),
        (BookKind::Contratos, r.contracts_number),
    ] {
        let Some(number) = number else { continue };
        if number == 0 {
            return Err(AppError::Other("el número de libro empieza en 1".into()));
        }
        match kind {
            BookKind::Actas => {
                let (approved, drafts) = ls::actas::counts(v, year)?;
                if approved == 0 {
                    return Err(AppError::Other(format!(
                        "no hay actas aprobadas del ejercicio {year}: apruébalas en Contabilidad → \
                         Libros societarios o quita el libro de actas"
                    )));
                }
                if drafts > 0 {
                    warnings.push(format!(
                        "hay {drafts} acta(s) de {year} en borrador: no van en el libro de actas"
                    ));
                }
            }
            BookKind::Socios => {
                if ls::registro::movements(v)?.is_empty() {
                    return Err(AppError::Other(
                        "el libro registro de socios está vacío: anota al menos la constitución"
                            .into(),
                    ));
                }
                if ls::registro::movements_between(v, start, end)? == 0 {
                    warnings.push(format!(
                        "en {year} no hubo cambios de titularidad ni gravámenes: el libro de \
                         socios sólo hay que legalizarlo el primer año y los ejercicios con cambios"
                    ));
                }
            }
            BookKind::Contratos => {
                if ls::contratos::contracts_between(v, start, end)? == 0 {
                    return Err(AppError::Other(format!(
                        "no hay contratos con el socio único en {year}: quita ese libro del \
                         paquete"
                    )));
                }
                if ls::contratos::sole_partner(v)?.is_none() {
                    warnings.push(
                        "según la ficha de socios la sociedad no es unipersonal: el libro de \
                         contratos es de las sociedades unipersonales (art. 16 LSC)"
                            .into(),
                    );
                }
            }
        }
        let bytes = ls::book(v, kind, Some(year), Some(number), true)?;
        files.push((
            format!("{}_{number:03}.pdf", kind.legalia_prefix()),
            format!("{} {year}", kind.label()),
            number,
            bytes,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&Sha256::digest(b"abc")).len(), 44);
        // Ejemplo de docs/research/LEGALIZACION.md: SHA-256 de un fichero vacío.
        assert_eq!(
            base64(&Sha256::digest(b"")),
            "47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU="
        );
    }

    #[test]
    fn datos_codigo_y_valor() {
        let f = |c: &str, l: usize, v: &str| (c.to_owned(), l, v.to_owned());
        let d = datos_txt(&[
            f("102", 100, "Peña Diseño SL"),
            f("103", 32, ""),
            f("501", 3, "2"),
            f("00105", 8, ""),
        ]);
        assert_eq!(d, "102Pena Diseno SL\r\n5012\r\n00105\r\n");
    }
}
