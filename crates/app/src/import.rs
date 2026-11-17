//! Importar asientos de otro programa: `SUENLACE.DAT` de a3 (formatos 4 y 5) o una tabla de
//! asientos en Excel (`.xlsx`, `.xls`), OpenDocument (`.ods`) o CSV, con columnas como las de a3innuva o ContaSOL (fecha,
//! número de asiento, concepto, cuenta, debe, haber…).
//!
//! Primero se lee el fichero y se enseña un resumen ([`preview`]); al confirmar se importan los
//! asientos de una vez en un lote ([`import`]) que se puede deshacer entero ([`undo`]). El mismo
//! fichero no se importa dos veces.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::LazyLock;

use regex::Regex;

use serde::Serialize;
use sha2::{Digest, Sha256};
use taxpain_core::accounting::suenlace_in;
use taxpain_core::accounting::{AccountCode, EntryLine, JournalEntry, Origin};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::{Date, Duration, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;

pub(crate) mod limites;

use limites::MAX_CELDAS_LIBRO;

/// Fichero leído y listo para importar.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Parsed {
    source: &'static str,
    entries: Vec<JournalEntry>,
    /// Nombres de cuenta del fichero.
    accounts: Vec<(AccountCode, String)>,
    invoices: usize,
    warnings: Vec<String>,
}

/// Resumen de lo que se va a importar (o de lo importado).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Preview {
    /// `suenlace` o `tabla`.
    pub source: String,
    pub entries: usize,
    pub invoices: usize,
    pub accounts: usize,
    pub first_date: Option<Date>,
    pub last_date: Option<Date>,
    /// Suma del debe de todos los asientos.
    pub total: Money,
    /// Los primeros asientos, para revisarlos.
    pub sample: Vec<JournalEntry>,
    pub warnings: Vec<String>,
    /// Ya se importó este fichero (lote y fecha).
    pub already: Option<String>,
}

fn other(e: impl std::fmt::Display) -> AppError {
    AppError::Other(e.to_string())
}

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

// ---------- Tablas (CSV y XLSX) ----------

/// Texto en UTF-8 o, si no lo es, en Windows-1252/Latin-1.
pub(crate) fn decode(bytes: &[u8]) -> String {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

/// Separador de un CSV: el que aparece el mismo número de veces en más líneas de las primeras
/// (fuera de comillas). Mirar sólo la primera falla si es un título («Libro registro, 2025»).
fn separator(text: &str) -> char {
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(30)
        .collect();
    let score = |sep: char| {
        let mut freq: std::collections::BTreeMap<usize, usize> = Default::default();
        for l in &lines {
            let mut quoted = false;
            let n = l
                .chars()
                .filter(|c| {
                    if *c == '"' {
                        quoted = !quoted;
                    }
                    *c == sep && !quoted
                })
                .count();
            if n > 0 {
                *freq.entry(n).or_default() += 1;
            }
        }
        // (líneas con el número más repetido, ese número)
        freq.into_iter()
            .map(|(n, lines)| (lines, n))
            .max()
            .unwrap_or((0, 0))
    };
    // Con empate gana el primero: el punto y coma de los CSV españoles.
    let mut best = (';', score(';'));
    for sep in ['\t', ','] {
        let s = score(sep);
        if s > best.1 {
            best = (sep, s);
        }
    }
    best.0
}

/// ¿Es un importe con puntos de miles y sin decimales («1.500», «12.345.678»)?
fn dot_thousands(s: &str) -> bool {
    let t = s.trim().trim_start_matches('-');
    let mut groups = t.split('.');
    let first = groups.next().unwrap_or("");
    let rest: Vec<&str> = groups.collect();
    !rest.is_empty()
        && (1..=3).contains(&first.len())
        && first.bytes().all(|b| b.is_ascii_digit())
        && rest
            .iter()
            .all(|g| g.len() == 3 && g.bytes().all(|b| b.is_ascii_digit()))
}

/// En un CSV, «1.500» es mil quinientos si en su columna los importes van con coma decimal
/// («10,50») o ninguno lleva punto decimal («12.5»): se le quitan los puntos para que no se lea
/// como 1,50 €. Las columnas con punto decimal se dejan como están.
fn normalize_thousands(rows: &mut [Vec<String>]) {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0);
    for col in 0..width {
        let cells = || rows.iter().filter_map(|r| r.get(col)).map(|c| c.trim());
        let numeric = |c: &str| {
            !c.is_empty()
                && c.trim_start_matches('-')
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b == b'.' || b == b',')
        };
        let comma_decimal = cells().filter(|c| numeric(c)).any(|c| {
            c.rsplit_once(',')
                .is_some_and(|(_, d)| (1..=2).contains(&d.len()))
        });
        let dot_decimal = cells().filter(|c| numeric(c) && !c.contains(',')).any(|c| {
            c.rsplit_once('.')
                .is_some_and(|(_, d)| (1..=2).contains(&d.len()))
        });
        if dot_decimal && !comma_decimal {
            continue;
        }
        for r in rows.iter_mut() {
            if let Some(c) = r.get_mut(col)
                && !c.contains(',')
                && dot_thousands(c)
            {
                *c = c.replace('.', "");
            }
        }
    }
}

pub(crate) fn csv(bytes: &[u8]) -> Vec<Vec<String>> {
    let text = decode(bytes);
    let sep = separator(&text);
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                cell.push('"');
                chars.next();
            }
            '"' => quoted = !quoted,
            c if c == sep && !quoted => row.push(std::mem::take(&mut cell)),
            '\n' if !quoted => {
                row.push(std::mem::take(&mut cell).trim_end_matches('\r').to_owned());
                rows.push(std::mem::take(&mut row));
            }
            c => cell.push(c),
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    normalize_thousands(&mut rows);
    rows
}

/// Una hoja de un libro de cálculo (o el CSV entero, sin nombre).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hoja {
    pub nombre: String,
    pub filas: Vec<Vec<String>>,
}

/// Primera hoja de un libro de Excel (`.xlsx`, `.xls`) u OpenDocument (`.ods`) como filas de
/// texto (los números con punto decimal; las fechas de Excel como su número de serie).
/// También las tablas HTML y el XML de Excel 2003 que algunos programas guardan como `.xls`.
pub(crate) fn spreadsheet(bytes: &[u8]) -> Result<Vec<Vec<String>>> {
    let mut wb = match abrir_libro(bytes) {
        Ok(wb) => wb,
        Err(Libro::Grande(e)) => return Err(e),
        Err(Libro::NoAbre(e)) => {
            return hojas_de_marcado(bytes)
                .and_then(|h| h.into_iter().next())
                .map(|h| h.filas)
                .ok_or_else(|| other(format!("no se puede leer la hoja de cálculo: {e}")));
        }
    };
    let nombre = calamine::Reader::sheet_names(&wb)
        .into_iter()
        .next()
        .ok_or_else(|| other("la hoja de cálculo no tiene hojas"))?;
    let range = rango_hoja(&mut wb, &nombre)?
        .ok_or_else(|| other("hoja de cálculo: no se puede leer la primera hoja"))?;
    let filas = range_rows(&range, false);
    if con_nulos(&filas)
        && let Some(Ok(r)) = xls_unicode(bytes).and_then(|mut x| {
            use calamine::Reader;
            x.worksheet_range_at(0)
        })
    {
        limites::comprobar_rango(&nombre, &r)?;
        return Ok(range_rows(&r, false));
    }
    Ok(filas)
}

/// Por qué no se ha abierto un libro con [`abrir_libro`].
#[derive(Debug)]
pub(crate) enum Libro {
    /// Pasa de los topes (`limites::MAX_CELDAS_HOJA` y compañía): no hay que intentar leerlo
    /// de otra forma.
    Grande(AppError),
    /// calamine no lo reconoce o está dañado (puede ser una tabla HTML con extensión `.xls`).
    NoAbre(String),
}

/// Abre un libro de Excel (`.xlsx`, `.xlsm`, `.xlsb`, `.xls`) u OpenDocument (`.ods`) con
/// calamine, después de comprobar que no pasa de los topes (ver `limites`): un fichero de
/// pocos KB puede hacer que calamine reserve gigas o se quede en un bucle. Todo lo que abra
/// hojas de cálculo con calamine tiene que pasar por aquí y leer las hojas con [`rango_hoja`].
pub(crate) fn abrir_libro(
    bytes: &[u8],
) -> std::result::Result<calamine::Sheets<Cursor<&[u8]>>, Libro> {
    limites::comprobar(bytes).map_err(Libro::Grande)?;
    calamine::open_workbook_auto_from_rs(Cursor::new(bytes))
        .map_err(|e| Libro::NoAbre(e.to_string()))
}

/// Las celdas de una hoja de un libro abierto con [`abrir_libro`], como `worksheet_range` pero
/// con los topes. `Ok(None)` si calamine no puede leer esa hoja; error si pasa de los topes.
pub(crate) fn rango_hoja<RS: std::io::Read + std::io::Seek>(
    wb: &mut calamine::Sheets<RS>,
    nombre: &str,
) -> Result<Option<calamine::Range<calamine::Data>>> {
    limites::rango(wb, nombre)
}

/// Todas las hojas con datos de un libro de Excel u OpenDocument, en su orden (un diario o un
/// libro registro por meses, las hojas extra de un libro…). También las tablas HTML y el XML
/// de Excel 2003 que algunos programas guardan como `.xls`.
pub(crate) fn spreadsheet_sheets(bytes: &[u8]) -> Result<Vec<Hoja>> {
    spreadsheet_sheets_with(bytes, false)
}

/// Como [`spreadsheet_sheets`]; con `fechas`, las de Excel van como `dd/mm/aaaa`.
fn spreadsheet_sheets_with(bytes: &[u8], fechas: bool) -> Result<Vec<Hoja>> {
    let mut out = match abrir_libro(bytes) {
        Ok(mut wb) => hojas_de(&mut wb, fechas, rango_hoja)?,
        Err(Libro::Grande(e)) => return Err(e),
        Err(Libro::NoAbre(e)) => {
            return hojas_de_marcado(bytes)
                .ok_or_else(|| other(format!("no se puede leer la hoja de cálculo: {e}")));
        }
    };
    if out.iter().any(|h| con_nulos(&h.filas))
        && let Some(mut x) = xls_unicode(bytes)
    {
        // El mismo `.xls`, ya comprobado: calamine tiene las celdas en memoria.
        out = hojas_de(&mut x, fechas, |x, n| {
            use calamine::Reader;
            match x.worksheet_range(n) {
                Ok(r) => limites::comprobar_rango(n, &r).map(|()| Some(r)),
                Err(_) => Ok(None),
            }
        })?;
    }
    if out.is_empty() {
        return Err(other("la hoja de cálculo no tiene hojas con datos"));
    }
    Ok(out)
}

/// Las hojas con datos de un libro ya abierto, leídas con `rango` (con los topes). Las hojas que
/// calamine no puede leer se saltan; pasar de los topes es un error (en una hoja o entre todas).
fn hojas_de<RS, R>(
    wb: &mut R,
    fechas: bool,
    rango: impl Fn(&mut R, &str) -> Result<Option<calamine::Range<calamine::Data>>>,
) -> Result<Vec<Hoja>>
where
    RS: std::io::Read + std::io::Seek,
    R: calamine::Reader<RS>,
{
    let mut out = Vec::new();
    let mut celdas = 0u64;
    for nombre in wb.sheet_names() {
        let Some(range) = rango(wb, &nombre)? else {
            continue;
        };
        celdas = celdas.saturating_add(limites::celdas_desde_a1(&range));
        if celdas > MAX_CELDAS_LIBRO {
            return Err(other(format!(
                "la hoja de cálculo es demasiado grande para leerla (más de {MAX_CELDAS_LIBRO} \
                 celdas entre todas las hojas)"
            )));
        }
        let filas = range_rows(&range, fechas);
        if filas.iter().any(|f| f.iter().any(|c| !c.trim().is_empty())) {
            out.push(Hoja { nombre, filas });
        }
    }
    Ok(out)
}

/// ¿Hay textos con U+0000 dentro? Es la señal de un `.xls` mal decodificado (ver
/// [`xls_unicode`]).
fn con_nulos(filas: &[Vec<String>]) -> bool {
    filas.iter().flatten().any(|c| c.contains('\0'))
}

/// Un `.xls` (BIFF8) abierto como UTF-16. Algunos programas que escriben `.xls` (JExcelAPI, el
/// de las descargas de ING, por ejemplo) ponen la página de códigos 1252 aunque guardan los
/// textos en UTF-16, y calamine les hace caso: cada letra sale seguida de un U+0000. Forzar la
/// página 1200 es lo correcto en BIFF8, donde cada texto dice si va en 8 o en 16 bits; los
/// BIFF5/7 sí dependen de la página de códigos, pero esos nunca dan ceros, y por eso esto sólo
/// se intenta cuando aparecen.
fn xls_unicode(bytes: &[u8]) -> Option<calamine::Xls<Cursor<&[u8]>>> {
    let mut opciones = calamine::XlsOptions::default();
    opciones.force_codepage = Some(1200);
    calamine::Xls::new_with_options(Cursor::new(bytes), opciones).ok()
}

/// Las tablas de una página HTML o de un libro en XML de Excel 2003, que muchos bancos y
/// programas descargan con la extensión `.xls`.
fn hojas_de_marcado(bytes: &[u8]) -> Option<Vec<Hoja>> {
    use crate::extractos::marcado;

    let texto = crate::extractos::texto_de(bytes);
    let t = texto.trim_start_matches('\u{feff}').trim_start();
    if !t.starts_with('<') {
        return None;
    }
    marcado::hojas_spreadsheetml(t).or_else(|| marcado::hojas_html(t))
}

/// Una fecha de Excel como `dd/mm/aaaa` (con `hh:mm` si lleva hora; sólo la hora si no lleva
/// día).
fn fecha_excel(d: &calamine::ExcelDateTime) -> String {
    let (y, m, dia, h, min, _, _) = d.to_ymd_hms_milli();
    let hora = format!("{h:02}:{min:02}");
    if d.as_f64() < 1.0 {
        hora
    } else if h == 0 && min == 0 {
        format!("{dia:02}/{m:02}/{y}")
    } else {
        format!("{dia:02}/{m:02}/{y} {hora}")
    }
}

/// Número de una celda como texto. Con un punto y tres decimales («12.345») se añade un cero
/// («12.3450»): [`parse_amount`] tomaría si no ese punto como de miles, y en una celda numérica
/// es siempre el decimal.
fn texto_float(f: f64) -> String {
    let t = f.to_string();
    let ambiguo = t
        .trim_start_matches('-')
        .split_once('.')
        .is_some_and(|(e, d)| d.len() == 3 && !e.contains('.'));
    if ambiguo { t + "0" } else { t }
}

/// Filas de texto de una hoja, con las filas y columnas vacías de delante. Las fechas de Excel
/// van como su número de serie (lo entienden los importadores) o, con `fechas`, como
/// `dd/mm/aaaa`. La hoja tiene que venir de [`rango_hoja`] (o comprobada con
/// `limites::comprobar_rango`): aquí se materializa cada fila, también las vacías de delante.
fn range_rows(range: &calamine::Range<calamine::Data>, fechas: bool) -> Vec<Vec<String>> {
    use calamine::Data;

    let (r0, c0) = range.start().unwrap_or((0, 0));
    let mut rows: Vec<Vec<String>> = vec![Vec::new(); usize::try_from(r0).unwrap_or(0)];
    for row in range.rows() {
        let mut out = vec![String::new(); usize::try_from(c0).unwrap_or(0)];
        out.extend(row.iter().map(|c| match c {
            Data::Empty => String::new(),
            Data::String(s) => s.clone(),
            Data::Int(i) => i.to_string(),
            Data::Float(f) => texto_float(*f),
            Data::Bool(b) => b.to_string(),
            Data::DateTime(d) if fechas && d.is_datetime() => fecha_excel(d),
            Data::DateTime(d) => d.as_f64().to_string(),
            Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
            Data::Error(_) => String::new(),
        }));
        rows.push(out);
    }
    rows
}

pub(crate) fn norm(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            'á' | 'Á' => Some('a'),
            'é' | 'É' => Some('e'),
            'í' | 'Í' => Some('i'),
            'ó' | 'Ó' => Some('o'),
            'ú' | 'Ú' | 'ü' => Some('u'),
            'ñ' | 'Ñ' => Some('n'),
            'º' | 'ª' => None,
            c if c.is_alphanumeric() => Some(c.to_ascii_lowercase()),
            _ => None,
        })
        .collect()
}

/// Importe en formato español (`1.234,56`), inglés (`1,234.56`, `1234.56`) o vacío (cero).
/// Con la regla de [`crate::extractos::numeros::importe`]: con los dos separadores, el último
/// es el decimal; un separador único seguido de exactamente tres cifras es de miles (`1.500`).
pub(crate) fn parse_amount(s: &str) -> Option<Money> {
    let t: String = s.trim().replace(['€', ' ', '\u{a0}'], "");
    if t.is_empty() || t == "-" {
        return Some(Money::ZERO);
    }
    crate::extractos::numeros::importe(s)
}

/// Fecha `dd/mm/aaaa`, `dd-mm-aa`, `aaaa-mm-dd` o número de serie de Excel.
/// Filas de una hoja de cálculo (`.xlsx`, `.xls`, `.ods`) o de un CSV, según el fichero.
pub(crate) fn rows(filename: &str, bytes: &[u8]) -> Result<Vec<Vec<String>>> {
    if is_spreadsheet(filename, bytes) {
        spreadsheet(bytes)
    } else {
        Ok(csv(bytes))
    }
}

/// Como [`sheets`], con las fechas de Excel como `dd/mm/aaaa`: para leer una hoja como texto
/// (en una conversación con la IA), no para importarla.
pub(crate) fn sheets_as_text(filename: &str, bytes: &[u8]) -> Result<Vec<Hoja>> {
    if is_spreadsheet(filename, bytes) {
        spreadsheet_sheets_with(bytes, true)
    } else {
        sheets(filename, bytes)
    }
}

/// Todas las hojas de una hoja de cálculo, o el CSV como una sola hoja sin nombre.
pub(crate) fn sheets(filename: &str, bytes: &[u8]) -> Result<Vec<Hoja>> {
    if is_spreadsheet(filename, bytes) {
        spreadsheet_sheets(bytes)
    } else {
        Ok(vec![Hoja {
            nombre: String::new(),
            filas: csv(bytes),
        }])
    }
}

/// Año del periodo que pone la cabecera de un listado («Período: de 01/01/2024 a 31/12/2024»,
/// «Período De 01 Ene a 31 Dic 2025», «Ejercicio 2025»), para las fechas que vienen sin año.
pub(crate) fn año_de_cabecera(filas: &[Vec<String>]) -> Option<i32> {
    static AÑO: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\b(?:19|20)\d{2}\b").ok());
    let re = AÑO.as_ref()?;
    filas.iter().take(12).find_map(|fila| {
        let texto = fila.join(" ");
        let n = norm(&texto);
        if !(n.contains("periodo") || n.contains("ejercicio")) {
            return None;
        }
        re.find_iter(&texto).last()?.as_str().parse().ok()
    })
}

/// [`parse_date`] y, si la fecha no lleva año («dd/mm»), con el `año` de la cabecera.
pub(crate) fn parse_date_in(s: &str, año: Option<i32>) -> Option<Date> {
    parse_date(s).or_else(|| {
        let (d, m) = s.trim().split_once(['/', '-'])?;
        if !(1..=2).contains(&d.len()) || !(1..=2).contains(&m.len()) {
            return None;
        }
        let mes = time::Month::try_from(m.parse::<u8>().ok()?).ok()?;
        Date::from_calendar_date(año?, mes, d.parse().ok()?).ok()
    })
}

/// ¿Es un libro de Excel u OpenDocument (por la extensión o la firma)?
pub(crate) fn is_spreadsheet(filename: &str, bytes: &[u8]) -> bool {
    let lower = filename.to_lowercase();
    [".xlsx", ".xlsm", ".xls", ".ods"]
        .iter()
        .any(|e| lower.ends_with(e))
        || bytes.starts_with(b"PK\x03\x04")
        || bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0])
}

pub(crate) fn parse_date(s: &str) -> Option<Date> {
    let s = s.trim();
    // Número de serie de Excel (sistema 1900: el 25569 es el 1/1/1970), con o sin hora.
    let int = s.split('.').next().unwrap_or(s);
    if let Ok(serial) = int.parse::<i64>()
        && (20_000..200_000).contains(&serial)
        && s.bytes().all(|b| b.is_ascii_digit() || b == b'.')
    {
        return Some(time::macros::date!(1970 - 01 - 01) + Duration::days(serial - 25_569));
    }
    let parts: Vec<&str> = s.split(['/', '-', '.']).map(str::trim).collect();
    let [a, b, c] = parts.as_slice() else {
        return None;
    };
    let (y, m, d) = if a.len() == 4 {
        (
            a.parse::<i32>().ok()?,
            b.parse::<u8>().ok()?,
            c.get(..2).unwrap_or(c).parse::<u8>().ok()?,
        )
    } else {
        let y: i32 = c.get(..4).unwrap_or(c).trim().parse().ok()?;
        let y = if y < 100 { 2000 + y } else { y };
        (y, b.parse().ok()?, a.parse().ok()?)
    };
    Date::from_calendar_date(y, time::Month::try_from(m).ok()?, d).ok()
}

#[derive(Default)]
struct Columns {
    date: Option<usize>,
    number: Option<usize>,
    document: Option<usize>,
    concept: Option<usize>,
    account: Option<usize>,
    account_name: Option<usize>,
    debit: Option<usize>,
    credit: Option<usize>,
}

fn columns(row: &[String]) -> Columns {
    let mut c = Columns::default();
    for (i, h) in row.iter().enumerate() {
        let n = norm(h);
        let slot = match n.as_str() {
            "fecha" | "fechaasiento" | "fechaapunte" => &mut c.date,
            "asiento" | "numeroasiento" | "nasiento" | "numasiento" | "numero" | "nasto" => {
                &mut c.number
            }
            "documento" | "doc" | "numerodocumento" => &mut c.document,
            "concepto" | "descripcion" | "conceptoapunte" => &mut c.concept,
            "cuenta" | "subcuenta" | "codigocuenta" | "codigo" => &mut c.account,
            "descripcioncuenta"
            | "descripciondelacuenta"
            | "descripciondecuenta"
            | "nombrecuenta"
            | "titulo"
            | "titulocuenta" => &mut c.account_name,
            "debe" | "importedebe" | "cargo" => &mut c.debit,
            "haber" | "importehaber" | "abono" => &mut c.credit,
            _ => continue,
        };
        if slot.is_none() {
            *slot = Some(i);
        }
    }
    c
}

fn parse_table(rows: &[Vec<String>]) -> Result<Parsed> {
    let (head, cols) = rows
        .iter()
        .enumerate()
        .take(20)
        .map(|(i, r)| (i, columns(r)))
        .find(|(_, c)| {
            c.date.is_some() && c.account.is_some() && c.debit.is_some() && c.credit.is_some()
        })
        .ok_or_else(|| other(SIN_TITULOS))?;
    let año = año_de_cabecera(&rows[..head]);
    let get = |r: &[String], c: Option<usize>| -> String {
        c.and_then(|i| r.get(i))
            .map(|s| s.trim().to_owned())
            .unwrap_or_default()
    };
    let mut entries: Vec<JournalEntry> = Vec::new();
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    let mut current: Option<(String, JournalEntry, usize)> = None;
    let mut warnings = Vec::new();
    let close = |cur: Option<(String, JournalEntry, usize)>,
                 entries: &mut Vec<JournalEntry>|
     -> Result<()> {
        if let Some((_, e, line)) = cur {
            e.validate().map_err(|err| {
                other(format!(
                    "fila {line}: el asiento «{}» no cuadra: {err}",
                    e.concept
                ))
            })?;
            entries.push(e);
        }
        Ok(())
    };
    for (i, r) in rows.iter().enumerate().skip(head + 1) {
        let line = i + 1;
        if r.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        let account_s = get(r, cols.account);
        if account_s.is_empty() {
            continue; // filas de totales o subtítulos
        }
        let account = AccountCode::new(&account_s)
            .map_err(|_| other(format!("fila {line}: cuenta no válida «{account_s}»")))?;
        let date_s = get(r, cols.date);
        let debit = parse_amount(&get(r, cols.debit))
            .ok_or_else(|| other(format!("fila {line}: importe del debe no válido")))?;
        let credit = parse_amount(&get(r, cols.credit))
            .ok_or_else(|| other(format!("fila {line}: importe del haber no válido")))?;
        let concept = get(r, cols.concept);
        let number = get(r, cols.number);
        let name = get(r, cols.account_name);
        if !name.is_empty() {
            names.entry(account.as_str().to_owned()).or_insert(name);
        }
        // Nuevo asiento: cambia el número (o, sin columna de número, el anterior ya cuadra). Una
        // línea sin número sigue el asiento de la anterior: en los listados de a3 o Sage el
        // número (y la fecha) sólo van en la primera línea de cada asiento.
        let key = if cols.number.is_some() {
            number.clone()
        } else {
            String::new()
        };
        let starts = match &current {
            None => true,
            Some((k, e, _)) => {
                if cols.number.is_some() {
                    !key.is_empty() && *k != key
                } else {
                    !e.lines.is_empty() && e.total_debit() == e.total_credit()
                }
            }
        };
        if starts {
            close(current.take(), &mut entries)?;
            let date = if date_s.is_empty() {
                return Err(other(format!("fila {line}: falta la fecha")));
            } else {
                parse_date_in(&date_s, año)
                    .ok_or_else(|| other(format!("fila {line}: fecha no válida «{date_s}»")))?
            };
            let document = get(r, cols.document);
            let mut c = if concept.is_empty() {
                format!("Asiento {number}")
            } else {
                concept.clone()
            };
            if !document.is_empty() {
                c = format!("{c} ({document})");
            }
            current = Some((
                key,
                JournalEntry {
                    date,
                    concept: c.trim().to_owned(),
                    origin: Origin::Manual,
                    lines: Vec::new(),
                },
                line,
            ));
        }
        let Some((_, e, _)) = current.as_mut() else {
            continue;
        };
        if !date_s.is_empty()
            && parse_date_in(&date_s, año).is_some_and(|d| d != e.date)
            && cols.number.is_some()
        {
            warnings.push(format!(
                "fila {line}: el asiento {number} tiene líneas con fechas distintas; se usa la primera"
            ));
        }
        let desc = Some(concept).filter(|c| !c.is_empty() && *c != e.concept);
        // Un importe negativo en el debe es un haber, y al revés.
        let net = debit - credit;
        if let Some(l) = EntryLine::signed(account, net, true) {
            e.lines.push(EntryLine {
                description: desc,
                ..l
            });
        }
    }
    close(current, &mut entries)?;
    if entries.is_empty() {
        return Err(other(SIN_ASIENTOS));
    }
    Ok(Parsed {
        source: "tabla",
        entries,
        accounts: names
            .into_iter()
            .filter_map(|(c, n)| AccountCode::new(&c).ok().map(|c| (c, n)))
            .collect(),
        invoices: 0,
        warnings,
    })
}

const SIN_TITULOS: &str =
    "no encuentro la fila de títulos: hacen falta las columnas Fecha, Cuenta, Debe y Haber";
const SIN_ASIENTOS: &str = "no hay asientos en la tabla";

/// Asientos de todas las hojas del libro (un diario por meses, por ejemplo), seguidos. Las
/// hojas que no son de asientos se saltan; un error en una hoja dice en cuál.
fn parse_sheets(hojas: &[Hoja]) -> Result<Parsed> {
    if let [una] = hojas {
        return parse_table(&una.filas);
    }
    let mut out: Option<Parsed> = None;
    let mut saltadas = Vec::new();
    for h in hojas {
        match parse_table(&h.filas) {
            Ok(mut p) => {
                p.warnings = p
                    .warnings
                    .into_iter()
                    .map(|w| format!("hoja «{}»: {w}", h.nombre))
                    .collect();
                match out.as_mut() {
                    None => out = Some(p),
                    Some(o) => {
                        o.entries.extend(p.entries);
                        o.accounts.extend(p.accounts);
                        o.invoices += p.invoices;
                        o.warnings.extend(p.warnings);
                    }
                }
            }
            Err(e) => {
                let texto = e.to_string();
                if texto.contains(SIN_TITULOS) || texto.contains(SIN_ASIENTOS) {
                    saltadas.push(h.nombre.clone());
                } else {
                    return Err(other(format!("hoja «{}»: {texto}", h.nombre)));
                }
            }
        }
    }
    let mut p = out.ok_or_else(|| other(SIN_TITULOS))?;
    if !saltadas.is_empty() {
        p.warnings.push(format!(
            "hojas sin asientos, que no se importan: {}",
            saltadas.join(", ")
        ));
    }
    Ok(p)
}

fn parse(filename: &str, bytes: &[u8]) -> Result<Parsed> {
    let lower = filename.to_lowercase();
    if is_spreadsheet(filename, bytes) {
        return parse_sheets(&spreadsheet_sheets(bytes)?);
    }
    if lower.ends_with(".dat")
        || matches!(bytes.first(), Some(b'4' | b'5')) && !lower.ends_with(".csv")
    {
        let r = suenlace_in::read(bytes).map_err(|e| other(format!("SUENLACE: {e}")))?;
        let mut warnings = Vec::new();
        if !r.ignored.is_empty() {
            warnings.push(format!(
                "registros que no se importan: {}",
                r.ignored
                    .iter()
                    .map(|(k, n)| format!("{n} de tipo {k}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        if r.invoices > 0 {
            warnings.push(format!(
                "{} factura(s) entran como asientos: sus datos de IVA no pasan a los libros de facturas de TaxPain",
                r.invoices
            ));
        }
        return Ok(Parsed {
            source: "suenlace",
            entries: r.entries,
            accounts: r
                .accounts
                .into_iter()
                .filter(|a| !a.name.is_empty())
                .map(|a| (a.code, a.name))
                .collect(),
            invoices: r.invoices,
            warnings,
        });
    }
    parse_table(&csv(bytes))
}

/// Asientos, nombres de cuenta y avisos de un diario (SUENLACE, Excel o CSV) sin importarlo: el
/// asistente de saldos iniciales saca de él la apertura del gestor ([`crate::apertura`]).
pub(crate) type Journal = (Vec<JournalEntry>, Vec<(AccountCode, String)>, Vec<String>);

pub(crate) fn read_journal(filename: &str, bytes: &[u8]) -> Result<Journal> {
    let p = parse(filename, bytes)?;
    Ok((p.entries, p.accounts, p.warnings))
}

fn already(v: &Vault, bytes: &[u8]) -> Result<Option<String>> {
    Ok(v.connection()
        .query_row(
            "SELECT filename, created_at FROM import_batch WHERE sha256 = ?1",
            [sha(bytes)],
            |r| {
                Ok(format!(
                    "{} (importado el {})",
                    r.get::<_, String>(0)?,
                    &r.get::<_, String>(1)?[..10]
                ))
            },
        )
        .optional()?)
}

fn summary(p: &Parsed, already: Option<String>) -> Preview {
    let dates = p.entries.iter().map(|e| e.date);
    Preview {
        source: p.source.into(),
        entries: p.entries.len(),
        invoices: p.invoices,
        accounts: p.accounts.len(),
        first_date: dates.clone().min(),
        last_date: dates.max(),
        total: p
            .entries
            .iter()
            .fold(Money::ZERO, |a, e| a + e.total_debit()),
        sample: p.entries.iter().take(5).cloned().collect(),
        warnings: p.warnings.clone(),
        already,
    }
}

/// Asientos propios de TaxPain (no importados de otro programa) entre dos fechas: si se importa
/// un diario de esas fechas, se contarían dos veces.
pub fn own_entries_between(v: &Vault, first: Date, last: Date) -> Result<usize> {
    let n: i64 = v.connection().query_row(
        "SELECT COUNT(*) FROM journal_entry WHERE date BETWEEN ?1 AND ?2
           AND origin_kind <> 'Importado'",
        params![first.to_string(), last.to_string()],
        |r| r.get(0),
    )?;
    Ok(usize::try_from(n).unwrap_or(0))
}

/// Lotes ya importados con asientos entre dos fechas: (lote, fichero, asientos). El gestor
/// suele reenviar el diario corregido al cierre: el mismo fichero cambiado no se reconoce por
/// su huella y, sin este aviso, todo el ejercicio entraría dos veces (art. 28 Código de
/// Comercio: un asiento por operación).
pub fn imported_entries_between(
    v: &Vault,
    first: Date,
    last: Date,
) -> Result<Vec<(i64, String, usize)>> {
    let mut stmt = v.connection().prepare(
        "SELECT b.id, b.filename, COUNT(*) FROM journal_entry e
           JOIN import_batch b ON b.id = e.origin_id
          WHERE e.origin_kind = 'Importado' AND e.date BETWEEN ?1 AND ?2
          GROUP BY b.id ORDER BY b.id",
    )?;
    let rows = stmt.query_map(params![first.to_string(), last.to_string()], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            usize::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
        ))
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Aviso si TaxPain ya tiene asientos (propios o de otro lote importado) en las fechas del
/// diario.
fn overlap_warning(v: &Vault, p: &mut Preview) -> Result<()> {
    if let (Some(a), Some(b)) = (p.first_date, p.last_date) {
        let n = own_entries_between(v, a, b)?;
        if n > 0 {
            p.warnings.push(format!(
                "TaxPain ya tiene {n} asiento(s) propios entre el {a} y el {b}: si importas este \
                 diario de esas fechas, esas operaciones se contarían dos veces"
            ));
        }
        for (batch, file, n) in imported_entries_between(v, a, b)? {
            p.warnings.push(format!(
                "TaxPain ya tiene {n} asiento(s) importados de «{file}» (lote {batch}) entre el \
                 {a} y el {b}: si este diario es el mismo corregido, deshaz antes ese lote en \
                 Importar asientos; si no, esas operaciones se contarían dos veces"
            ));
        }
    }
    Ok(())
}

/// Lee el fichero y dice qué se importaría, sin tocar nada.
pub fn preview(v: &Vault, filename: &str, bytes: &[u8]) -> Result<Preview> {
    let p = parse(filename, bytes)?;
    let mut s = summary(&p, already(v, bytes)?);
    overlap_warning(v, &mut s)?;
    Ok(s)
}

/// Importa los asientos del fichero en un lote. Devuelve el resumen y el número de lote.
pub fn import(
    v: &Vault,
    filename: &str,
    bytes: &[u8],
    now: OffsetDateTime,
) -> Result<(i64, Preview)> {
    if let Some(a) = already(v, bytes)? {
        return Err(other(format!("este fichero ya se importó: {a}")));
    }
    let p = parse(filename, bytes)?;
    import_parsed(v, filename, bytes, &p, now)
}

/// Tabla de asientos con las columnas ya identificadas (por ejemplo, por el asistente): vista
/// previa sin importar.
pub(crate) fn preview_rows(v: &Vault, bytes: &[u8], rows: &[Vec<String>]) -> Result<Preview> {
    let p = parse_table(rows)?;
    let mut s = summary(&p, already(v, bytes)?);
    overlap_warning(v, &mut s)?;
    Ok(s)
}

/// Importa una tabla de asientos con las columnas ya identificadas.
pub(crate) fn import_rows(
    v: &Vault,
    filename: &str,
    bytes: &[u8],
    rows: &[Vec<String>],
    now: OffsetDateTime,
) -> Result<(i64, Preview)> {
    if let Some(a) = already(v, bytes)? {
        return Err(other(format!("este fichero ya se importó: {a}")));
    }
    let p = parse_table(rows)?;
    import_parsed(v, filename, bytes, &p, now)
}

fn import_parsed(
    v: &Vault,
    filename: &str,
    bytes: &[u8],
    p: &Parsed,
    now: OffsetDateTime,
) -> Result<(i64, Preview)> {
    if let Some(a) = already(v, bytes)? {
        return Err(other(format!("este fichero ya se importó: {a}")));
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO import_batch(source, filename, sha256, entries, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![p.source, filename, sha(bytes), i64::try_from(p.entries.len()).unwrap_or(0), now_str(now)],
    )?;
    let batch = tx.last_insert_rowid();
    for (code, name) in &p.accounts {
        tx.execute(
            "INSERT OR IGNORE INTO account(code, name, created_at) VALUES (?1, ?2, ?3)",
            params![code.as_str(), name, now_str(now)],
        )?;
    }
    for e in &p.entries {
        let mut e = e.clone();
        e.origin = Origin::Importado(batch);
        crate::accounting::record_entry(&tx, &e, now).map_err(|err| match err {
            AppError::Vault(_) if err.to_string().contains("cerrado") => other(format!(
                "el ejercicio del {} está cerrado: no se le pueden añadir asientos",
                e.date
            )),
            err => err,
        })?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "import.asientos",
        &batch.to_string(),
        &serde_json::json!({ "source": p.source, "file": filename, "entries": p.entries.len() }),
    )?;
    tx.commit()?;
    Ok((batch, summary(p, None)))
}

/// Lote importado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Batch {
    pub id: i64,
    pub source: String,
    pub filename: String,
    pub entries: i64,
    pub created_at: String,
}

pub fn batches(v: &Vault) -> Result<Vec<Batch>> {
    let mut stmt = v.connection().prepare(
        "SELECT id, source, filename, entries, created_at FROM import_batch ORDER BY id DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Batch {
            id: r.get(0)?,
            source: r.get(1)?,
            filename: r.get(2)?,
            entries: r.get(3)?,
            created_at: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Tablas que apuntan a un asiento (`REFERENCES journal_entry(id)` en las migraciones) y lo que
/// significa para quien quiere deshacer el lote.
pub(crate) const ENTRY_REFERENCES: [(&str, &str, &str); 13] = [
    (
        "bank_tx",
        "entry_id",
        "está conciliado con un movimiento del banco",
    ),
    (
        "settlement",
        "entry_id",
        "es el cobro o pago de una factura",
    ),
    ("tax_payment", "entry_id", "es el pago de un modelo"),
    ("payslip", "entry_id", "es el de una nómina"),
    (
        "fixed_asset",
        "disposal_entry",
        "es la baja de un inmovilizado",
    ),
    (
        "asset_sale_invoice",
        "entry_id",
        "pasa una factura a la venta de un inmovilizado",
    ),
    ("leasing", "start_entry", "es el alta de un arrendamiento"),
    (
        "leasing_installment",
        "entry_id",
        "es la cuota de un arrendamiento",
    ),
    ("dividend_payment", "entry_id", "es el pago de un dividendo"),
    ("prestamo", "alta_entry", "es el alta de un préstamo"),
    ("prestamo_cuota", "entry_id", "es la cuota de un préstamo"),
    (
        "prestamo_devengo",
        "entry_id",
        "es el devengo de intereses de un préstamo",
    ),
    ("travel_expense", "entry_id", "es el de un gasto de viaje"),
];

/// Deshace un lote: borra sus asientos (si sus ejercicios siguen abiertos y nada apunta a
/// ellos).
pub fn undo(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<usize> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let filename: String = tx
        .query_row(
            "SELECT filename FROM import_batch WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    // Asientos del lote a los que apunta otra cosa: con las claves foráneas activas el borrado
    // fallaría con un «FOREIGN KEY constraint failed»; mejor decir qué asiento y por qué.
    for (table, column, why) in ENTRY_REFERENCES {
        let blocked: Option<(i64, String, String)> = tx
            .query_row(
                &format!(
                    "SELECT e.id, e.date, e.concept FROM journal_entry e
                      WHERE e.origin_kind = 'Importado' AND e.origin_id = ?1
                        AND EXISTS(SELECT 1 FROM {table} x WHERE x.{column} = e.id)
                      ORDER BY e.id LIMIT 1"
                ),
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((entry, date, concept)) = blocked {
            return Err(other(format!(
                "no se puede deshacer: el asiento {entry} ({date}, {concept}) {why}; deshaz \
                 antes eso"
            )));
        }
    }
    tx.execute(
        "DELETE FROM line_tick WHERE line_id IN (SELECT l.id FROM journal_line l JOIN journal_entry e ON e.id = l.entry_id
           WHERE e.origin_kind = 'Importado' AND e.origin_id = ?1)",
        [id],
    )?;
    tx.execute(
        "DELETE FROM journal_line WHERE entry_id IN
           (SELECT id FROM journal_entry WHERE origin_kind = 'Importado' AND origin_id = ?1)",
        [id],
    )
    .map_err(|e| other(format!("no se puede deshacer: {e}")))?;
    let n = tx
        .execute(
            "DELETE FROM journal_entry WHERE origin_kind = 'Importado' AND origin_id = ?1",
            [id],
        )
        .map_err(|e| other(format!("no se puede deshacer: {e}")))?;
    tx.execute("DELETE FROM import_batch WHERE id = ?1", [id])?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "import.deshacer",
        &id.to_string(),
        &serde_json::json!({ "file": filename, "entries": n }),
    )?;
    tx.commit()?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn importes_y_fechas() {
        assert_eq!(parse_amount("1.234,56"), Some(Money::from_cents(123_456)));
        assert_eq!(parse_amount("1234.5"), Some(Money::from_cents(123_450)));
        assert_eq!(parse_amount("-80"), Some(Money::from_cents(-8000)));
        assert_eq!(
            parse_amount("12.300000000000001"),
            Some(Money::from_cents(1230))
        );
        assert_eq!(
            parse_amount("1.234.567"),
            Some(Money::from_cents(123_456_700))
        );
        assert_eq!(parse_amount(""), Some(Money::ZERO));
        assert_eq!(parse_amount("abc"), None);
        assert_eq!(
            parse_amount("20,000.00"),
            Some(Money::from_cents(2_000_000))
        );
        assert_eq!(parse_amount("1,234.56"), Some(Money::from_cents(123_456)));
        assert_eq!(parse_amount("1.500"), Some(Money::from_cents(150_000)));
        assert_eq!(parse_amount("1,5"), Some(Money::from_cents(150)));
        // En una celda numérica el punto es el decimal.
        assert_eq!(
            parse_amount(&texto_float(12.345)),
            Some(Money::from_cents(1235))
        );
        assert_eq!(texto_float(1500.0), "1500");
        let d = time::macros::date!(2026 - 03 - 05);
        assert_eq!(parse_date("05/03/2026"), Some(d));
        assert_eq!(parse_date("5-3-26"), Some(d));
        assert_eq!(parse_date("2026-03-05"), Some(d));
        assert_eq!(parse_date("46086"), Some(d));
        assert_eq!(parse_date("46086.5"), Some(d));
        assert_eq!(parse_date("05.03.2026"), Some(d));
        assert_eq!(parse_date("31/02/2026"), None);
        // Sin año: el de la cabecera del listado.
        assert_eq!(parse_date_in("05/03", Some(2026)), Some(d));
        assert_eq!(parse_date_in("5-3", Some(2026)), Some(d));
        assert_eq!(parse_date_in("05/03", None), None);
        assert_eq!(parse_date_in("05/03/2026", None), Some(d));
        let cab = |t: &str| vec![vec!["Diario.".to_owned()], vec![t.to_owned()]];
        assert_eq!(
            año_de_cabecera(&cab("Período De 01 Ene a 31 Dic 2025")),
            Some(2025)
        );
        assert_eq!(
            año_de_cabecera(&cab("Período: de 01/01/2024 a 31/12/2024")),
            Some(2024)
        );
        assert_eq!(año_de_cabecera(&cab("Empresa: EJEMPLO 2025 SL")), None);
    }

    #[test]
    fn csv_con_comillas_y_punto_y_coma() {
        let rows =
            csv(b"Fecha;Concepto;Cuenta;Debe;Haber\r\n01/02/2026;\"Pago; luz\";628;10,00;\r\n");
        assert_eq!(rows[1], vec!["01/02/2026", "Pago; luz", "628", "10,00", ""]);
    }

    #[test]
    fn csv_con_titulo_delante_y_puntos_de_miles() {
        // El título tiene una coma, pero el separador es el punto y coma de las filas.
        let rows = csv("Libro registro de facturas, 2025\n\
             Fecha;Número;Base;Cuota\n\
             01/02/2025;1;1.500;315,00\n\
             02/02/2025;2;250,50;52,61\n"
            .as_bytes());
        assert_eq!(rows[2], vec!["01/02/2025", "1", "1500", "315,00"]);
        assert_eq!(parse_amount(&rows[2][2]), Some(Money::from_cents(150_000)));
        // Sin comas en la columna y todos con grupos de tres cifras: miles.
        let rows = csv(b"Fecha;Importe\n01/02/2025;1.500\n02/02/2025;12.345.678\n03/02/2025;300\n");
        assert_eq!(rows[1][1], "1500");
        assert_eq!(rows[2][1], "12345678");
        // Con punto decimal en la columna, «1.500» se deja (es 1,5).
        let rows = csv(b"Fecha,Importe\n01/02/2025,1.500\n02/02/2025,12.50\n");
        assert_eq!(rows[1][1], "1.500");
        // Las fechas con puntos no se tocan.
        let rows = csv(b"Fecha;Importe\n01.02.2025;1.500\n");
        assert_eq!(rows[1], vec!["01.02.2025", "1500"]);
    }
}
