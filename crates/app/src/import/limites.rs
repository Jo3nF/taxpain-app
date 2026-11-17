//! Topes al abrir hojas de cálculo que llegan de fuera (una carpeta vigilada, un adjunto…).
//!
//! calamine se fía de lo que dice el fichero: reserva las celdas del rectángulo que va de la
//! primera a la última (una hoja con A1 y XFD1048576 son 2^34 celdas), reserva lo que dicen las
//! cabeceras (el registro `Dimensions` de un `.xls`, el `uniqueCount` de un `.xlsx`), repite las
//! celdas y filas que un `.ods` dice repetidas, y copia el texto compartido en cada celda que lo
//! usa. Un fichero de pocos KB puede así agotar la memoria, y algunas estructuras rotas (un
//! `.xls` con una cadena de sectores en bucle, un `.ods` sin cerrar) lo dejan en un bucle sin fin.
//!
//! Antes de dárselo a calamine, [`comprobar`] repasa la estructura con las mismas reglas que
//! calamine y rechaza lo que pasaría de los topes; los `.xlsx` y `.xlsb` se leen celda a celda
//! con [`rango`], con los topes, en vez de con `worksheet_range`.

use std::io::{BufReader, Cursor, Read, Seek};

use calamine::{Cell, Data, DataRef, Range, Reader, Sheets};
use quick_xml::Reader as XmlReader;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::QName;

use crate::error::{AppError, Result};

/// Como mucho tantas celdas en una hoja (filas × columnas del rectángulo que ocupa, contando
/// desde A1 para lo que se devuelve): una tabla de 100.000 filas y 20 columnas.
pub(crate) const MAX_CELDAS_HOJA: u64 = 2_000_000;
/// Como mucho tantas celdas en todo el libro (calamine tiene todas las hojas de un `.xls` o un
/// `.ods` a la vez en memoria).
pub(crate) const MAX_CELDAS_LIBRO: u64 = 6_000_000;
/// Como mucho tanto texto (en bytes) en las celdas del libro, contando cada copia.
pub(crate) const MAX_TEXTO: u64 = 128 * 1024 * 1024;
/// Como mucho tanto, descomprimido, en un `.xlsx`, `.xlsb` u `.ods` (sumando sus partes).
pub(crate) const MAX_DESCOMPRIMIDO: u64 = 100 * 1024 * 1024;
/// Como mucho tantas partes en el zip de un `.xlsx`, `.xlsb` u `.ods`.
pub(crate) const MAX_ENTRADAS: usize = 10_000;
/// Como mucho tanto por registro de un `.xlsb`: la longitud de un registro puede declarar hasta
/// 256 MB y calamine reserva esa memoria antes de leerlo. Un registro normal (una celda, un texto
/// de 32.767 letras con formato) ocupa como mucho unos cientos de KB.
const MAX_REGISTRO_XLSB: u64 = 16 * 1024 * 1024;
/// Como mucho tantos textos compartidos declarados en un `.xlsx` (calamine los reserva).
const MAX_COMPARTIDOS: u64 = 4_000_000;
/// Filas y columnas de una hoja de Excel (calamine recorta a esto las repeticiones de un `.ods`).
const MAX_FILAS: u64 = 1_048_576;
const MAX_COLUMNAS: u64 = 16_384;

fn demasiado(que: impl std::fmt::Display) -> AppError {
    AppError::Other(format!(
        "la hoja de cálculo es demasiado grande para leerla ({que})"
    ))
}

fn ilegible(que: impl std::fmt::Display) -> AppError {
    AppError::Other(format!(
        "no se puede leer la hoja de cálculo: está dañada ({que})"
    ))
}

fn miles(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('.');
        }
        out.push(c);
    }
    out
}

fn celdas_de_mas(que: &str, n: u64, max: u64) -> AppError {
    demasiado(format!(
        "{que}: {} celdas, y el máximo es {}",
        miles(n),
        miles(max)
    ))
}

/// Comprueba, sin dárselo a calamine, que un libro de Excel u OpenDocument se puede abrir sin
/// pasar de los topes. Lo que no es ni `.xls` ni zip no lo puede abrir calamine: pasa (ya fallará
/// al abrirlo).
pub(crate) fn comprobar(bytes: &[u8]) -> Result<()> {
    if bytes.starts_with(&CFB_FIRMA) {
        xls::comprobar(bytes)?;
    }
    // calamine busca el zip por el final del fichero, aunque no empiece por `PK`.
    if let Ok(z) = zip::ZipArchive::new(Cursor::new(bytes)) {
        comprobar_zip(z)?;
    }
    Ok(())
}

/// Las celdas de una hoja como las da `worksheet_range`, con los topes: en `.xlsx` y `.xlsb`
/// se leen celda a celda (sin fiarse de la dimensión declarada); en `.xls` y `.ods` ya las
/// tiene calamine, comprobadas por [`comprobar`]. `Ok(None)`: calamine no puede leer la hoja.
pub(crate) fn rango<RS: Read + Seek>(
    wb: &mut Sheets<RS>,
    nombre: &str,
) -> Result<Option<Range<Data>>> {
    match wb {
        Sheets::Xlsx(x) => match x.worksheet_cells_reader(nombre) {
            Ok(mut r) => recoger(nombre, || r.next_cell()),
            Err(_) => Ok(None),
        },
        Sheets::Xlsb(x) => match x.worksheet_cells_reader(nombre) {
            Ok(mut r) => recoger(nombre, || r.next_cell()),
            Err(_) => Ok(None),
        },
        Sheets::Xls(_) | Sheets::Ods(_) => match wb.worksheet_range(nombre) {
            Ok(r) => {
                comprobar_rango(nombre, &r)?;
                Ok(Some(r))
            }
            Err(_) => Ok(None),
        },
    }
}

/// Celdas que ocupan las filas de texto de una hoja (desde A1: las filas y columnas vacías de
/// delante también se devuelven). Ver `range_rows`.
pub(crate) fn celdas_desde_a1(range: &Range<Data>) -> u64 {
    let (r0, c0) = range.start().unwrap_or((0, 0));
    let (alto, ancho) = range.get_size();
    let alto = u64::try_from(alto).unwrap_or(u64::MAX);
    let ancho = u64::try_from(ancho).unwrap_or(u64::MAX);
    alto.saturating_mul(ancho.saturating_add(u64::from(c0)))
        .saturating_add(u64::from(r0))
}

/// Rechaza una hoja que, como filas de texto desde A1, pasaría de [`MAX_CELDAS_HOJA`].
pub(crate) fn comprobar_rango(nombre: &str, range: &Range<Data>) -> Result<()> {
    let n = celdas_desde_a1(range);
    if n > MAX_CELDAS_HOJA {
        return Err(celdas_de_mas(
            &format!("hoja «{nombre}»"),
            n,
            MAX_CELDAS_HOJA,
        ));
    }
    Ok(())
}

/// Recoge las celdas con valor de un lector de celdas, parando en cuanto el rectángulo que
/// ocupan (o el texto que llevan) pasa de los topes.
fn recoger<'a, E>(
    nombre: &str,
    mut siguiente: impl FnMut() -> std::result::Result<Option<Cell<DataRef<'a>>>, E>,
) -> Result<Option<Range<Data>>> {
    let mut celdas: Vec<Cell<Data>> = Vec::new();
    let mut caja: Option<(u32, u32, u32, u32)> = None;
    let mut texto = 0u64;
    loop {
        let c = match siguiente() {
            Ok(Some(c)) => c,
            Ok(None) => break,
            Err(_) => return Ok(None),
        };
        let v = c.get_value();
        if matches!(v, DataRef::Empty) {
            continue;
        }
        let (f, col) = c.get_position();
        let (f0, c0, f1, c1) = caja.unwrap_or((f, col, f, col));
        let nueva = (f0.min(f), c0.min(col), f1.max(f), c1.max(col));
        caja = Some(nueva);
        // Las filas de texto se devuelven desde A1: cuenta lo de delante.
        let alto = u64::from(nueva.2 - nueva.0) + 1;
        let ancho = u64::from(nueva.3) + 1;
        let n = alto
            .saturating_mul(ancho)
            .saturating_add(u64::from(nueva.0));
        if n > MAX_CELDAS_HOJA {
            return Err(celdas_de_mas(
                &format!("hoja «{nombre}»"),
                n,
                MAX_CELDAS_HOJA,
            ));
        }
        texto += match v {
            DataRef::String(s) | DataRef::DateTimeIso(s) | DataRef::DurationIso(s) => {
                s.len() as u64
            }
            DataRef::SharedString(s) => s.len() as u64,
            _ => 0,
        };
        if texto > MAX_TEXTO {
            return Err(demasiado(format!("hoja «{nombre}»: demasiado texto")));
        }
        celdas.push(Cell::new((f, col), Data::from(v.clone())));
    }
    Ok(Some(Range::from_sparse(celdas)))
}

// ---------- zip: .xlsx, .xlsb, .ods ----------

/// Partes del zip: cuántas, cuánto ocupan descomprimidas (lo declarado, que el lector de zip
/// hace cumplir, y lo leído, con tope) y, de las que calamine lee de una vez, lo que declaran.
fn comprobar_zip<R: Read + Seek>(mut z: zip::ZipArchive<R>) -> Result<()> {
    if z.len() > MAX_ENTRADAS {
        return Err(demasiado(format!(
            "{} partes en el zip",
            miles(z.len() as u64)
        )));
    }
    let mut declarado = 0u64;
    for i in 0..z.len() {
        if let Ok(f) = z.by_index_raw(i) {
            declarado = declarado.saturating_add(f.size());
        }
    }
    if declarado > MAX_DESCOMPRIMIDO {
        return Err(demasiado(format!(
            "{} MB descomprimido, y el máximo es {} MB",
            declarado / (1024 * 1024),
            MAX_DESCOMPRIMIDO / (1024 * 1024)
        )));
    }
    let es_xlsb = z
        .file_names()
        .filter_map(std::result::Result::ok)
        .any(|n| n.replace('\\', "/").eq_ignore_ascii_case("xl/workbook.bin"));
    let mut leido = 0u64;
    for i in 0..z.len() {
        // Lo que no se puede descomprimir aquí tampoco lo puede leer calamine (usa el mismo
        // lector de zip).
        let Ok(f) = z.by_index(i) else {
            continue;
        };
        let tam = f.size();
        let nombre = f.name().map(|n| n.to_lowercase()).unwrap_or_default();
        let mut parte = f.take(tam.saturating_add(1));
        let n = if nombre == "content.xml" {
            let mut contado = Contado::new(&mut parte);
            let r = ods::comprobar(&mut contado);
            let n = contado.n + std::io::copy(&mut contado.r, &mut std::io::sink()).unwrap_or(0);
            r?;
            n
        } else if nombre.ends_with("sharedstrings.xml") {
            let mut contado = Contado::new(&mut parte);
            let r = compartidos_declarados(&mut contado);
            let n = contado.n + std::io::copy(&mut contado.r, &mut std::io::sink()).unwrap_or(0);
            r?;
            n
        } else if es_xlsb && let Some(como) = xlsb::recorrido(&nombre) {
            let mut contado = Contado::new(&mut parte);
            let r = xlsb::comprobar_parte(&mut contado, como);
            let n = contado.n + std::io::copy(&mut contado.r, &mut std::io::sink()).unwrap_or(0);
            r?;
            n
        } else {
            std::io::copy(&mut parte, &mut std::io::sink()).unwrap_or(0)
        };
        if n > tam {
            return Err(ilegible("una parte del zip ocupa más de lo que declara"));
        }
        leido = leido.saturating_add(n);
        if leido > MAX_DESCOMPRIMIDO {
            return Err(demasiado("demasiado descomprimido"));
        }
    }
    Ok(())
}

// ---------- .xlsb ----------

mod xlsb {
    use std::io::Read;

    use super::{MAX_REGISTRO_XLSB, Result, demasiado, miles};

    /// Cómo se mira una parte de un `.xlsb`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum Recorrido {
        /// Las que calamine 0.37.0 lee registro a registro: el libro, los estilos, los textos
        /// compartidos y las hojas (sólo abre hojas bajo `xl/worksheets/`, `xl/chartsheets/` o
        /// `xl/dialogsheets/`). Siempre se recorren.
        Conocida,
        /// Cualquier otra parte `.bin` (comentarios, vínculos externos, cachés de tablas
        /// dinámicas…), por si otra versión de calamine llega a leerla: se recorre salvo que sea
        /// un fichero compuesto (CFB: objetos incrustados, ActiveX), que no son registros.
        SalvoCfb,
    }

    /// Firma de un fichero compuesto de OLE ([MS-CFB] 2.2).
    const CFB: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

    /// Si hay que recorrer los registros de esa parte de un `.xlsb` (`nombre` va en
    /// minúsculas). No se recorren las relaciones, el proyecto de VBA (CFB) ni la configuración
    /// de impresora (una estructura `DEVMODE`, no registros).
    pub(super) fn recorrido(nombre: &str) -> Option<Recorrido> {
        let nombre = nombre.replace('\\', "/");
        if matches!(
            nombre.as_str(),
            "xl/workbook.bin" | "xl/styles.bin" | "xl/sharedstrings.bin"
        ) || (["xl/worksheets/", "xl/chartsheets/", "xl/dialogsheets/"]
            .iter()
            .any(|d| nombre.starts_with(d))
            && !nombre.contains("/_rels/"))
        {
            return Some(Recorrido::Conocida);
        }
        let fichero = nombre.rsplit('/').next().unwrap_or_default();
        let fuera = nombre.contains("/_rels/")
            || nombre.contains("printersettings")
            || fichero.starts_with("vbaproject");
        (nombre.ends_with(".bin") && !fuera).then_some(Recorrido::SalvoCfb)
    }

    /// Como [`comprobar`], saltándose la parte si es un CFB y se puede.
    pub(super) fn comprobar_parte<R: Read>(mut r: R, como: Recorrido) -> Result<()> {
        if como == Recorrido::Conocida {
            return comprobar(r);
        }
        let mut cabecera = Vec::with_capacity(CFB.len());
        (&mut r)
            .take(CFB.len() as u64)
            .read_to_end(&mut cabecera)
            .unwrap_or(0);
        if cabecera == CFB {
            return Ok(());
        }
        comprobar(cabecera.as_slice().chain(r))
    }

    fn byte<R: Read>(r: &mut R) -> Option<u8> {
        let mut b = [0u8];
        r.read_exact(&mut b).ok().map(|()| b[0])
    }

    /// Recorre los registros (tipo y longitud como los lee calamine, [MS-XLSB] 2.1.4) y rechaza
    /// el que declare más de [`MAX_REGISTRO_XLSB`]. Lo que se acaba antes de tiempo pasa: calamine
    /// fallará al leerlo, pero sin reservar de más.
    pub(super) fn comprobar<R: Read>(mut r: R) -> Result<()> {
        loop {
            // Tipo: uno o dos bytes.
            let Some(b) = byte(&mut r) else {
                return Ok(());
            };
            if b & 0x80 != 0 && byte(&mut r).is_none() {
                return Ok(());
            }
            // Longitud: hasta cuatro bytes de 7 bits.
            let Some(mut b) = byte(&mut r) else {
                return Ok(());
            };
            let mut largo = u64::from(b & 0x7F);
            for i in 1..4 {
                if b & 0x80 == 0 {
                    break;
                }
                let Some(sig) = byte(&mut r) else {
                    return Ok(());
                };
                b = sig;
                largo += u64::from(b & 0x7F) << (7 * i);
            }
            if largo > MAX_REGISTRO_XLSB {
                return Err(demasiado(format!(
                    "un registro del .xlsb declara {} bytes, y el máximo es {}",
                    miles(largo),
                    miles(MAX_REGISTRO_XLSB)
                )));
            }
            let saltado =
                std::io::copy(&mut (&mut r).take(largo), &mut std::io::sink()).unwrap_or(0);
            if saltado < largo {
                return Ok(());
            }
        }
    }
}

/// Lector que cuenta los bytes que da.
struct Contado<R> {
    r: R,
    n: u64,
}

impl<R> Contado<R> {
    fn new(r: R) -> Self {
        Self { r, n: 0 }
    }
}

impl<R: Read> Read for Contado<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.r.read(buf)?;
        self.n += n as u64;
        Ok(n)
    }
}

/// El `uniqueCount` de `sharedStrings.xml`, que calamine reserva al abrir el `.xlsx`.
fn compartidos_declarados<R: Read>(r: R) -> Result<()> {
    let mut xml = lector(r);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match xml.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.local_name().as_ref() == "sst" => {
                if let Some(v) = atributo(&e, "uniqueCount")? {
                    // Como lo lee calamine: cifras (sin signo ni espacios); si no lo entiende,
                    // no reserva.
                    let cifras = v.trim_start_matches('+').trim_start_matches('0');
                    let largo = cifras.bytes().take_while(u8::is_ascii_digit).count();
                    if largo > 12 || cifras[..largo].parse::<u64>().unwrap_or(0) > MAX_COMPARTIDOS {
                        return Err(demasiado("demasiados textos compartidos"));
                    }
                }
                return Ok(());
            }
            Ok(Event::Eof) | Err(_) => return Ok(()),
            _ => {}
        }
    }
}

/// Lector de XML configurado como los de calamine.
fn lector<R: Read>(r: R) -> XmlReader<BufReader<R>> {
    let mut x = XmlReader::from_reader(BufReader::new(r));
    let c = x.config_mut();
    c.check_end_names = false;
    c.trim_text(false);
    c.check_comments = false;
    c.expand_empty_elements = true;
    x
}

/// Atributos de un elemento como los lee calamine (`clave="valor"`, sin resolver entidades):
/// un atributo mal formado es un error, como allí.
fn atributos<'a>(e: &'a BytesStart<'_>) -> impl Iterator<Item = Result<(&'a str, &'a str)>> + 'a {
    let raw = e.attributes_raw();
    let b = raw.as_bytes();
    let mut pos = 0usize;
    std::iter::from_fn(move || {
        while pos < b.len() && b[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if pos >= b.len() {
            return None;
        }
        let k0 = pos;
        while pos < b.len() && b[pos] != b'=' {
            pos += 1;
        }
        if pos >= b.len() {
            pos = b.len();
            return Some(Err(ilegible("atributo sin valor")));
        }
        let clave = raw[k0..pos].trim_ascii_end();
        pos += 1;
        while pos < b.len() && b[pos].is_ascii_whitespace() {
            pos += 1;
        }
        let comilla = match b.get(pos) {
            Some(q @ (b'"' | b'\'')) => *q,
            _ => {
                pos = b.len();
                return Some(Err(ilegible("atributo sin comillas")));
            }
        };
        pos += 1;
        let v0 = pos;
        while pos < b.len() && b[pos] != comilla {
            pos += 1;
        }
        let valor = &raw[v0..pos];
        if pos < b.len() {
            pos += 1;
        }
        Some(Ok((clave, valor)))
    })
}

fn atributo<'a>(e: &'a BytesStart<'_>, clave: &str) -> Result<Option<&'a str>> {
    for a in atributos(e) {
        let (k, v) = a?;
        if k == clave {
            return Ok(Some(v));
        }
    }
    Ok(None)
}

/// Un valor de atributo con las entidades resueltas (como `decode_attr` de calamine).
fn sin_entidades(v: &str) -> Result<String> {
    quick_xml::escape::unescape(v)
        .map(|s| s.into_owned())
        .map_err(|_| ilegible("entidad desconocida en un atributo"))
}

/// Recuentos de un libro, para los topes.
#[derive(Default)]
struct Cuenta {
    celdas: u64,
    texto: u64,
}

impl Cuenta {
    fn celdas(&mut self, n: u64) -> Result<()> {
        self.celdas = self.celdas.saturating_add(n);
        if self.celdas > MAX_CELDAS_LIBRO {
            return Err(celdas_de_mas("el libro", self.celdas, MAX_CELDAS_LIBRO));
        }
        Ok(())
    }

    fn texto(&mut self, n: u64) -> Result<()> {
        self.texto = self.texto.saturating_add(n);
        if self.texto > MAX_TEXTO {
            return Err(demasiado("demasiado texto en las celdas"));
        }
        Ok(())
    }
}

/// `content.xml` de un `.ods`, recorrido como lo recorre calamine (que lo lee entero al abrir el
/// libro), contando las celdas y el texto que calamine guardaría.
mod ods {
    use super::*;

    /// Una fila leída: celdas que guarda calamine, dónde está la primera y la última con valor
    /// (en los datos y en las fórmulas) y el texto que llevan.
    struct Fila {
        repite: u64,
        ancho: u64,
        datos: Option<(u64, u64)>,
        formulas: Option<(u64, u64)>,
        texto: u64,
    }

    pub(super) fn comprobar<R: Read>(r: R) -> Result<()> {
        let mut xml = lector(r);
        let mut cuenta = Cuenta::default();
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match xml.read_event_into(&mut buf) {
                Ok(Event::Start(e)) if e.name() == QName("table:table") => {
                    let nombre = atributo(&e, "table:name")?.map(str::to_owned);
                    if let Some(n) = nombre {
                        tabla(&mut xml, &n, &mut cuenta)?;
                    }
                }
                Ok(Event::Start(e)) if e.name() == QName("table:named-expressions") => {
                    nombres(&mut xml)?;
                }
                Ok(Event::Eof) => return Ok(()),
                Err(_) => return Err(ilegible("XML mal formado")),
                _ => {}
            }
        }
    }

    fn tabla<R: Read>(
        xml: &mut XmlReader<BufReader<R>>,
        nombre: &str,
        cuenta: &mut Cuenta,
    ) -> Result<()> {
        let mut filas: Vec<Fila> = Vec::new();
        let mut total = 0u64;
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match xml.read_event_into(&mut buf) {
                Ok(Event::Start(e)) if e.name() == QName("table:table-row") => {
                    let repite = match atributo(&e, "table:number-rows-repeated")? {
                        Some(v) => sin_entidades(v)?
                            .parse::<u64>()
                            .map_err(|_| ilegible("repetición de filas"))?,
                        None => 1,
                    };
                    let repite = repite.min(MAX_FILAS.saturating_sub(total));
                    total += repite;
                    let mut f = fila(xml, cuenta)?;
                    f.repite = repite;
                    filas.push(f);
                }
                Ok(Event::End(e)) if e.name() == QName("table:table") => break,
                // calamine no sale nunca de una tabla sin cerrar: se queda en bucle.
                Ok(Event::Eof) => return Err(ilegible("hoja sin cerrar")),
                Err(_) => return Err(ilegible("XML mal formado")),
                _ => {}
            }
        }
        // Lo que guarda calamine al quedarse con el rectángulo que ocupan los datos (y otro para
        // las fórmulas): las filas repetidas se copian, y las vacías de en medio también.
        for formulas in [false, true] {
            let caja = |f: &Fila| if formulas { f.formulas } else { f.datos };
            let Some(primera) = filas.iter().position(|f| caja(f).is_some()) else {
                continue;
            };
            let ultima = filas
                .iter()
                .rposition(|f| caja(f).is_some())
                .unwrap_or(primera);
            let c0 = filas
                .iter()
                .filter_map(&caja)
                .map(|c| c.0)
                .min()
                .unwrap_or(0);
            let c1 = filas
                .iter()
                .filter_map(&caja)
                .map(|c| c.1)
                .max()
                .unwrap_or(0);
            let ancho = c1 + 1 - c0;
            let mut celdas = 0u64;
            let mut texto = 0u64;
            for f in &filas[primera..=ultima] {
                celdas = celdas.saturating_add(f.repite.saturating_mul(ancho));
                if caja(f).is_some() {
                    texto = texto.saturating_add(f.repite.saturating_mul(f.texto));
                }
            }
            if !formulas && celdas > MAX_CELDAS_HOJA {
                return Err(celdas_de_mas(
                    &format!("hoja «{nombre}»"),
                    celdas,
                    MAX_CELDAS_HOJA,
                ));
            }
            cuenta.celdas(celdas)?;
            cuenta.texto(texto)?;
        }
        Ok(())
    }

    fn fila<R: Read>(xml: &mut XmlReader<BufReader<R>>, cuenta: &mut Cuenta) -> Result<Fila> {
        let mut f = Fila {
            repite: 1,
            ancho: 0,
            datos: None,
            formulas: None,
            texto: 0,
        };
        let mut vacias_pendientes = 0u64;
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match xml.read_event_into(&mut buf) {
                Ok(Event::Start(e))
                    if e.name() == QName("table:table-cell")
                        || e.name() == QName("table:covered-table-cell") =>
                {
                    let repite = match atributo(&e, "table:number-columns-repeated")? {
                        Some(v) => v
                            .parse::<u64>()
                            .map_err(|_| ilegible("repetición de columnas"))?,
                        None => 1,
                    };
                    let nombre = e.name().as_ref().to_owned();
                    let (dato, formula, texto, cerrada) = celda(xml, &e)?;
                    let quedan = MAX_COLUMNAS.saturating_sub(f.ancho);
                    let vacias = vacias_pendientes.min(quedan);
                    f.ancho += vacias;
                    cuenta.celdas(vacias.saturating_mul(2))?;
                    vacias_pendientes = 0;
                    let quedan = MAX_COLUMNAS.saturating_sub(f.ancho);
                    let repite = repite.min(quedan);
                    if !dato && !formula {
                        vacias_pendientes = repite;
                    } else if repite > 0 {
                        let (a, b) = (f.ancho, f.ancho + repite - 1);
                        if dato {
                            let d = f.datos.get_or_insert((a, b));
                            d.1 = b;
                        }
                        if formula {
                            let d = f.formulas.get_or_insert((a, b));
                            d.1 = b;
                        }
                        f.ancho += repite;
                        // Cada celda repetida es una copia (valor y fórmula).
                        let t = texto.saturating_mul(repite);
                        f.texto = f.texto.saturating_add(t);
                        cuenta.celdas(repite.saturating_mul(2))?;
                        cuenta.texto(t)?;
                    }
                    if !cerrada {
                        let mut b2 = Vec::new();
                        xml.read_to_end_into(QName(&nombre), &mut b2)
                            .map_err(|_| ilegible("celda sin cerrar"))?;
                    }
                }
                Ok(Event::End(e)) if e.name() == QName("table:table-row") => return Ok(f),
                Err(_) => return Err(ilegible("XML mal formado")),
                // calamine no admite nada más dentro de una fila: no abriría el libro.
                Ok(_) => return Err(ilegible("fila con contenido inesperado")),
            }
        }
    }

    /// Una celda como la lee calamine: ¿lleva valor?, ¿lleva fórmula?, bytes de texto (valor y
    /// fórmula) y si ya se ha leído hasta su cierre.
    fn celda<R: Read>(
        xml: &mut XmlReader<BufReader<R>>,
        e: &BytesStart<'_>,
    ) -> Result<(bool, bool, u64, bool)> {
        let mut es_texto = false;
        let mut con_valor = false;
        let mut texto = 0u64;
        let mut formula = 0u64;
        for a in atributos(e) {
            let (k, v) = a?;
            match k {
                "office:value" | "office:boolean-value" if !con_valor => con_valor = true,
                "office:string-value" | "office:date-value" | "office:time-value" if !con_valor => {
                    con_valor = true;
                    texto = sin_entidades(v)?.len() as u64;
                }
                "office:value-type" if !con_valor => es_texto = v == "string",
                "table:formula" => formula = sin_entidades(v)?.len() as u64,
                _ => {}
            }
        }
        if con_valor || !es_texto {
            return Ok((con_valor, formula > 0, texto + formula, false));
        }
        // El texto va dentro de la celda: hasta su cierre.
        let mut primer_parrafo = true;
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match xml.read_event_into(&mut buf) {
                Ok(Event::Text(t)) => texto += t.xml10_content().len() as u64,
                Ok(Event::GeneralRef(_)) => texto += 4,
                Ok(Event::End(e))
                    if e.name() == QName("table:table-cell")
                        || e.name() == QName("table:covered-table-cell") =>
                {
                    return Ok((true, formula > 0, texto + formula, true));
                }
                Ok(Event::Start(e)) if e.name() == QName("office:annotation") => {
                    let mut b2 = Vec::new();
                    loop {
                        b2.clear();
                        match xml.read_event_into(&mut b2) {
                            Ok(Event::End(e)) if e.name() == QName("office:annotation") => break,
                            // calamine no sale nunca de una nota sin cerrar.
                            Ok(Event::Eof) => return Err(ilegible("nota sin cerrar")),
                            Err(_) => return Err(ilegible("XML mal formado")),
                            _ => {}
                        }
                    }
                }
                Ok(Event::Start(e)) if e.name() == QName("text:p") => {
                    if !primer_parrafo {
                        texto += 1;
                    }
                    primer_parrafo = false;
                }
                // `<text:s text:c="N"/>`: N espacios, que calamine escribe uno a uno.
                Ok(Event::Start(e)) if e.name() == QName("text:s") => {
                    let n = match atributo(&e, "text:c")? {
                        Some(v) => sin_entidades(v)?
                            .parse::<u64>()
                            .map_err(|_| ilegible("espacios"))?,
                        None => 1,
                    };
                    texto = texto.saturating_add(n);
                    if texto > MAX_TEXTO {
                        return Err(demasiado("demasiado texto en una celda"));
                    }
                }
                Err(_) => return Err(ilegible("XML mal formado")),
                Ok(Event::Eof) => return Err(ilegible("celda sin cerrar")),
                _ => {}
            }
        }
    }

    /// `table:named-expressions`, sólo para seguir por donde sigue calamine.
    fn nombres<R: Read>(xml: &mut XmlReader<BufReader<R>>) -> Result<()> {
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match xml.read_event_into(&mut buf) {
                Ok(Event::Start(e))
                    if e.name() == QName("table:named-range")
                        || e.name() == QName("table:named-expression") => {}
                Ok(Event::End(e))
                    if e.name() == QName("table:named-range")
                        || e.name() == QName("table:named-expression") => {}
                Ok(Event::End(e)) if e.name() == QName("table:named-expressions") => {
                    return Ok(());
                }
                _ => return Err(ilegible("nombres definidos")),
            }
        }
    }
}

// ---------- .xls (BIFF dentro de un fichero compuesto de OLE) ----------

const CFB_FIRMA: [u8; 8] = *b"\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1";

/// El `.xls` se recorre con las mismas reglas que calamine (fichero compuesto, flujo
/// `Workbook`, registros BIFF de cada hoja), mirando lo que calamine reservaría y las cadenas de
/// sectores que seguiría (si dan la vuelta, calamine se queda en un bucle sin fin).
mod xls {
    use super::*;

    const ENDOFCHAIN: u32 = 0xFFFF_FFFE;
    const DIFSECT: u32 = 0xFFFF_FFFC;
    const RESERVADOS: u32 = 0xFFFF_FFFA;

    fn u16_en(b: &[u8], i: usize) -> Result<u16> {
        b.get(i..i + 2)
            .and_then(|s| s.try_into().ok())
            .map(u16::from_le_bytes)
            .ok_or_else(|| ilegible("registro corto"))
    }

    fn u32_en(b: &[u8], i: usize) -> Result<u32> {
        b.get(i..i + 4)
            .and_then(|s| s.try_into().ok())
            .map(u32::from_le_bytes)
            .ok_or_else(|| ilegible("registro corto"))
    }

    struct Compuesto<'a> {
        bytes: &'a [u8],
        tam: usize,
        maxid: u32,
    }

    impl<'a> Compuesto<'a> {
        /// Un sector (el último puede estar cortado), como lo lee calamine.
        fn sector(&self, id: u32) -> Result<&'a [u8]> {
            if id > self.maxid {
                return Err(ilegible("sector fuera del fichero"));
            }
            let ini = (id as usize + 1)
                .checked_mul(self.tam)
                .filter(|i| *i < self.bytes.len())
                .ok_or_else(|| ilegible("sector fuera del fichero"))?;
            Ok(&self.bytes[ini..(ini + self.tam).min(self.bytes.len())])
        }

        /// Una cadena de sectores hasta `len` bytes (o hasta su fin, con `None`), sin dar la
        /// vuelta: como mucho tantos pasos como sectores tiene el fichero.
        fn cadena(
            &self,
            mut id: u32,
            fat: &[u32],
            len: Option<usize>,
            sector: impl Fn(u32) -> Result<&'a [u8]>,
        ) -> Result<Vec<u8>> {
            if len.is_some_and(|l| l > self.bytes.len()) {
                return Err(ilegible("flujo más largo que el fichero"));
            }
            let mut out = Vec::new();
            let mut pasos = 0u64;
            while id != ENDOFCHAIN {
                pasos += 1;
                if pasos > u64::from(self.maxid) + 2 {
                    return Err(ilegible("cadena de sectores en bucle"));
                }
                let s = sector(id)?;
                let quedan = len.map_or(s.len(), |l| l - out.len());
                out.extend_from_slice(&s[..s.len().min(quedan)]);
                if len.is_some_and(|l| out.len() >= l) {
                    break;
                }
                id = *fat
                    .get(id as usize)
                    .ok_or_else(|| ilegible("sector fuera de la tabla"))?;
            }
            Ok(out)
        }
    }

    fn a_u32(b: &[u8]) -> impl Iterator<Item = u32> + '_ {
        b.as_chunks::<4>().0.iter().map(|c| u32::from_le_bytes(*c))
    }

    /// El flujo `Workbook` (o `Book`), o `None` si no lo hay (no es un `.xls`: calamine no lo
    /// abrirá).
    fn flujo(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
        if bytes.len() < 512 {
            return Ok(None);
        }
        let tam = match u16_en(bytes, 30)? {
            9 => 512usize,
            12 => 4096,
            _ => return Ok(None),
        };
        if u16_en(bytes, 32)? != 6 || bytes.len() < tam {
            return Ok(None);
        }
        let version = u16_en(bytes, 26)?;
        let cfb = Compuesto {
            bytes,
            tam,
            maxid: u32::try_from(bytes.len() / tam).unwrap_or(u32::MAX),
        };
        let inicio_dir = u32_en(bytes, 48)?;
        let inicio_minifat = u32_en(bytes, 60)?;
        let sectores_minifat = u32_en(bytes, 64)? as usize;
        let mut id = u32_en(bytes, 68)?;
        let mut difat: Vec<u32> = a_u32(&bytes[76..512]).collect();
        let mut pasos = 0u64;
        while id < RESERVADOS {
            pasos += 1;
            if pasos > u64::from(cfb.maxid) + 2 {
                return Err(ilegible("cadena de sectores en bucle"));
            }
            difat.extend(a_u32(cfb.sector(id)?));
            id = difat.pop().unwrap_or(ENDOFCHAIN);
        }
        let sectores_fat: Vec<u32> = difat.into_iter().filter(|id| *id < DIFSECT).collect();
        if sectores_fat.len() as u64 > u64::from(cfb.maxid) + 1 {
            return Err(ilegible("tabla de sectores más larga que el fichero"));
        }
        let mut fat = Vec::new();
        for id in sectores_fat {
            fat.extend(a_u32(cfb.sector(id)?));
        }
        let dirs = cfb.cadena(inicio_dir, &fat, None, |id| cfb.sector(id))?;
        if dirs.is_empty() || !dirs.len().is_multiple_of(128) {
            return Err(ilegible("directorio"));
        }
        let entradas: Vec<(String, u32, usize)> = dirs
            .as_chunks::<128>()
            .0
            .iter()
            .map(|d| {
                let utf16: Vec<u16> = d[..64]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_le_bytes(*c))
                    .collect();
                let mut nombre = String::from_utf16_lossy(&utf16);
                if let Some(i) = nombre.find('\0') {
                    nombre.truncate(i);
                }
                let inicio = u32::from_le_bytes([d[116], d[117], d[118], d[119]]);
                let len = if tam == 512 {
                    u64::from(u32::from_le_bytes([d[120], d[121], d[122], d[123]]))
                } else {
                    u64::from_le_bytes([
                        d[120], d[121], d[122], d[123], d[124], d[125], d[126], d[127],
                    ])
                };
                (nombre, inicio, usize::try_from(len).unwrap_or(usize::MAX))
            })
            .collect();
        if version != 3 && entradas[0].1 == ENDOFCHAIN {
            return Ok(None);
        }
        let (mini, minifat) = if sectores_minifat > 0 {
            let mini = cfb.cadena(entradas[0].1, &fat, Some(entradas[0].2), |id| {
                cfb.sector(id)
            })?;
            let largo = sectores_minifat
                .checked_mul(tam)
                .ok_or_else(|| ilegible("minitabla"))?;
            let minifat: Vec<u32> =
                a_u32(&cfb.cadena(inicio_minifat, &fat, Some(largo), |id| cfb.sector(id))?)
                    .collect();
            (mini, minifat)
        } else {
            (Vec::new(), Vec::new())
        };
        let Some((_, inicio, len)) = ["Workbook", "Book", "WORKBOOK", "BOOK"]
            .iter()
            .find_map(|n| entradas.iter().find(|e| e.0 == *n))
        else {
            return Ok(None);
        };
        let s = if *len < 4096 {
            let max_mini = cfb.maxid.saturating_mul(8);
            cfb.cadena(*inicio, &minifat, Some(*len), |id| {
                if id > max_mini {
                    return Err(ilegible("minisector fuera del fichero"));
                }
                let ini = id as usize * 64;
                // calamine completa un minisector cortado con lo que haya en el fichero: no se
                // puede saber qué leería.
                if ini + 64 > mini.len() {
                    return Err(ilegible("minisector fuera del flujo"));
                }
                Ok(&mini[ini..(ini + 64).min(mini.len())])
            })?
        } else {
            cfb.cadena(*inicio, &fat, Some(*len), |id| cfb.sector(id))?
        };
        Ok(Some(s))
    }

    /// Un registro BIFF: tipo, datos y los `Continue` que le siguen.
    struct Registro<'a> {
        tipo: u16,
        datos: &'a [u8],
        siguen: Vec<&'a [u8]>,
    }

    /// Los registros desde el principio de `s`, como los separa calamine.
    fn registros(mut s: &[u8]) -> impl Iterator<Item = Result<Registro<'_>>> {
        std::iter::from_fn(move || {
            if s.is_empty() {
                return None;
            }
            if s.len() < 4 {
                s = &[];
                return Some(Err(ilegible("registro cortado")));
            }
            let tipo = u16::from_le_bytes([s[0], s[1]]);
            let len = usize::from(u16::from_le_bytes([s[2], s[3]]));
            if s.len() < len + 4 {
                s = &[];
                return Some(Err(ilegible("registro cortado")));
            }
            let datos = &s[4..4 + len];
            s = &s[4 + len..];
            let mut siguen = Vec::new();
            while s.len() > 4 && u16::from_le_bytes([s[0], s[1]]) == 0x003C {
                let len = usize::from(u16::from_le_bytes([s[2], s[3]]));
                if s.len() < len + 4 {
                    s = &[];
                    return Some(Err(ilegible("registro cortado")));
                }
                siguen.push(&s[4..4 + len]);
                s = &s[4 + len..];
            }
            Some(Ok(Registro {
                tipo,
                datos,
                siguen,
            }))
        })
    }

    /// Largo (en caracteres) de cada texto de la tabla de textos compartidos (SST), leída como
    /// la lee calamine.
    fn textos(r: &Registro<'_>) -> Result<Vec<u64>> {
        let corto = || ilegible("tabla de textos");
        let mut datos = r.datos.get(8..).ok_or_else(corto)?;
        let mut siguen = r.siguen.iter();
        let mut out = Vec::new();
        loop {
            if datos.is_empty() {
                match siguen.next() {
                    Some(d) => datos = d,
                    None => return Ok(out),
                }
                if datos.is_empty() {
                    out.push(0);
                    continue;
                }
            }
            if datos.len() < 3 {
                return Err(corto());
            }
            let cch = usize::from(u16::from_le_bytes([datos[0], datos[1]]));
            let flags = datos[2];
            datos = &datos[3..];
            let mut runs = 0usize;
            if flags & 0x8 != 0 {
                runs = usize::from(u16_en(datos, 0)?);
                datos = &datos[2..];
            }
            let mut ext = 0usize;
            if flags & 0x4 != 0 {
                ext = u32_en(datos, 0)? as usize;
                datos = &datos[4..];
            }
            let mut ancho = if flags & 0x1 != 0 { 2 } else { 1 };
            let mut quedan = cch;
            while quedan > 0 {
                let l = (datos.len() / ancho).min(quedan);
                datos = &datos[l * ancho..];
                quedan -= l;
                if quedan > 0 {
                    datos = siguen.next().ok_or_else(corto)?;
                    let f = *datos.first().ok_or_else(corto)?;
                    ancho = if f & 0x1 != 0 { 2 } else { 1 };
                    datos = &datos[1..];
                }
            }
            out.push(cch as u64);
            let mut saltar = runs * 4 + ext;
            while saltar > 0 {
                if datos.is_empty() {
                    datos = siguen.next().ok_or_else(corto)?;
                }
                let l = saltar.min(datos.len());
                datos = &datos[l..];
                saltar -= l;
            }
        }
    }

    pub(super) fn comprobar(bytes: &[u8]) -> Result<()> {
        let Some(s) = flujo(bytes)? else {
            return Ok(());
        };
        let mut hojas = Vec::new();
        let mut textos_sst: Vec<u64> = Vec::new();
        for r in registros(&s) {
            let r = r?;
            match r.tipo {
                // Con contraseña: calamine no lo abre.
                0x002F if u16_en(r.datos, 0)? != 0 => return Ok(()),
                0x0085 => {
                    if r.datos.len() < 5 {
                        return Err(ilegible("hoja"));
                    }
                    hojas.push(u32_en(r.datos, 0)? as usize);
                }
                0x00FC => textos_sst = textos(&r)?,
                0x000A => break,
                _ => {}
            }
        }
        let mut cuenta = Cuenta::default();
        // Varias hojas pueden apuntar al mismo sitio: como mucho se recorre el flujo dos veces.
        let mut presupuesto = (s.len() as u64).saturating_mul(2) + 65_536;
        for (i, pos) in hojas.into_iter().enumerate() {
            let h = s
                .get(pos..)
                .ok_or_else(|| ilegible("hoja fuera del libro"))?;
            hoja(h, i + 1, &textos_sst, &mut cuenta, &mut presupuesto)?;
        }
        Ok(())
    }

    /// Las celdas de una hoja: lo que reserva su registro `Dimensions` y el rectángulo que
    /// ocupan las celdas (calamine reserva los dos).
    fn hoja(
        s: &[u8],
        n: usize,
        textos_sst: &[u64],
        cuenta: &mut Cuenta,
        presupuesto: &mut u64,
    ) -> Result<()> {
        let que = format!("hoja {n}");
        let mut caja: Option<(u32, u32, u32, u32)> = None;
        let mut celdas = 0u64;
        let mut pos_formula = (0u32, 0u32);
        let mut visto_bof = false;
        let mut anidado = 0usize;
        let poner = |caja: &mut Option<(u32, u32, u32, u32)>, f: u32, c0: u32, c1: u32| {
            let (a, b, c, d) = caja.unwrap_or((f, c0, f, c1));
            *caja = Some((a.min(f), b.min(c0), c.max(f), d.max(c1)));
        };
        for r in registros(s) {
            let r = r?;
            let d = r.datos;
            let largo = 4 + d.len() + r.siguen.iter().map(|c| 4 + c.len()).sum::<usize>();
            *presupuesto = presupuesto
                .checked_sub(largo as u64)
                .ok_or_else(|| ilegible("hojas repetidas"))?;
            if anidado > 0 {
                match r.tipo {
                    0x0809 => anidado += 1,
                    0x000A => anidado -= 1,
                    _ => {}
                }
                continue;
            }
            match r.tipo {
                0x0809 => {
                    if visto_bof {
                        anidado += 1;
                    } else {
                        visto_bof = true;
                    }
                }
                0x0200 => {
                    let (rf, rl, mut cf, cl) = match d.len() {
                        10 => (
                            u32::from(u16_en(d, 0)?),
                            u32::from(u16_en(d, 2)?),
                            u32::from(u16_en(d, 4)?),
                            u32::from(u16_en(d, 6)?),
                        ),
                        14 | 16 => (
                            u32_en(d, 0)?,
                            u32_en(d, 4)?,
                            u32::from(u16_en(d, 8)?),
                            u32::from(u16_en(d, 10)?),
                        ),
                        _ => return Err(ilegible("dimensiones")),
                    };
                    if 0xFF < cf || cl < cf {
                        cf = 0;
                    }
                    let (ini, fin) = if rl >= 1 && cl >= 1 {
                        ((rf, cf), (rl - 1, cl - 1))
                    } else {
                        ((rf, cf), (rf, cf))
                    };
                    if fin.0 < ini.0 || fin.1 < ini.1 {
                        return Err(ilegible("dimensiones"));
                    }
                    let n = (u64::from(fin.0 - ini.0) + 1) * (u64::from(fin.1 - ini.1) + 1);
                    if n > MAX_CELDAS_HOJA {
                        return Err(celdas_de_mas(&que, n, MAX_CELDAS_HOJA));
                    }
                }
                0x0203 | 0x0204 | 0x00D6 | 0x0205 | 0x027E => {
                    let (f, c) = (u32::from(u16_en(d, 0)?), u32::from(u16_en(d, 2)?));
                    poner(&mut caja, f, c, c);
                    celdas += 1;
                }
                0x00FD => {
                    let (f, c) = (u32::from(u16_en(d, 0)?), u32::from(u16_en(d, 2)?));
                    let i = u32_en(d, 6)? as usize;
                    if let Some(t) = textos_sst.get(i) {
                        poner(&mut caja, f, c, c);
                        celdas += 1;
                        // Cada celda lleva su copia del texto (hasta 3 bytes por carácter, y
                        // el doble si la página de códigos no es la de Unicode).
                        cuenta.texto(t.saturating_mul(6))?;
                    }
                }
                0x00BD => {
                    if d.len() < 6 {
                        return Err(ilegible("celdas"));
                    }
                    let f = u32::from(u16_en(d, 0)?);
                    let c0 = u16_en(d, 2)?;
                    let c1 = u16_en(d, d.len() - 2)?;
                    if c1 < c0 {
                        return Err(ilegible("celdas"));
                    }
                    poner(&mut caja, f, u32::from(c0), u32::from(c1));
                    celdas += u64::from(c1 - c0) + 1;
                }
                0x0006 => {
                    pos_formula = (u32::from(u16_en(d, 0)?), u32::from(u16_en(d, 2)?));
                    poner(&mut caja, pos_formula.0, pos_formula.1, pos_formula.1);
                    celdas += 1;
                }
                0x0207 => {
                    poner(&mut caja, pos_formula.0, pos_formula.1, pos_formula.1);
                    celdas += 1;
                }
                0x000A => break,
                _ => {}
            }
            if celdas > MAX_CELDAS_HOJA {
                return Err(celdas_de_mas(&que, celdas, MAX_CELDAS_HOJA));
            }
        }
        if let Some((f0, c0, f1, c1)) = caja {
            let n = (u64::from(f1 - f0) + 1) * (u64::from(c1 - c0) + 1);
            if n > MAX_CELDAS_HOJA {
                return Err(celdas_de_mas(&que, n, MAX_CELDAS_HOJA));
            }
            cuenta.celdas(n.saturating_add(celdas))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::time::{Duration, Instant};

    use super::*;
    use crate::import::{spreadsheet, spreadsheet_sheets};

    /// Corre `f` en otro hilo y falla si no termina a tiempo (los ficheros que dejan a calamine
    /// en bucle no deben llegar a él).
    fn a_tiempo<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(Duration::from_secs(30))
            .expect("no ha terminado a tiempo")
    }

    fn es_demasiado(r: &Result<impl std::fmt::Debug>) -> bool {
        matches!(r, Err(AppError::Other(m)) if m.contains("demasiado grande"))
    }

    fn es_ilegible(r: &Result<impl std::fmt::Debug>) -> bool {
        matches!(r, Err(AppError::Other(m)) if m.contains("dañada"))
    }

    // ---------- .xlsx ----------

    fn xlsx(celdas: &[(u32, u16, &str)]) -> Vec<u8> {
        let mut wb = rust_xlsxwriter::Workbook::new();
        let ws = wb.add_worksheet();
        for (f, c, t) in celdas {
            ws.write_string(*f, *c, *t).unwrap();
        }
        wb.save_to_buffer().unwrap()
    }

    #[test]
    fn un_xlsx_con_a1_y_xfd1048576_se_rechaza_rapido() {
        // Dos celdas en las esquinas: calamine reservaría 2^34 celdas.
        let bytes = xlsx(&[(0, 0, "a"), (1_048_575, 16_383, "b")]);
        assert!(
            bytes.len() < 20_000,
            "el fichero es pequeño: {}",
            bytes.len()
        );
        let t = Instant::now();
        let r = a_tiempo(move || (spreadsheet(&bytes), spreadsheet_sheets(&bytes)));
        assert!(es_demasiado(&r.0), "{:?}", r.0);
        assert!(es_demasiado(&r.1), "{:?}", r.1);
        assert!(t.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn un_xlsx_normal_se_sigue_leyendo() {
        let bytes = xlsx(&[
            (0, 0, "Fecha"),
            (0, 1, "Importe"),
            (1, 0, "01/02/2026"),
            (1, 1, "12,5"),
        ]);
        let filas = spreadsheet(&bytes).unwrap();
        assert_eq!(
            filas,
            [vec!["Fecha", "Importe"], vec!["01/02/2026", "12,5"]]
        );
        // Una celda lejos pero con la hoja por debajo del tope, también.
        let bytes = xlsx(&[(0, 0, "a"), (99_999, 9, "b")]);
        let filas = spreadsheet(&bytes).unwrap();
        assert_eq!(filas.len(), 100_000);
    }

    #[test]
    fn un_texto_compartido_copiado_en_muchas_celdas_se_rechaza() {
        // 5.000 celdas con el mismo texto de 32.767 letras: 160 MB al copiarlo en cada una.
        let largo = "x".repeat(32_767);
        let celdas: Vec<(u32, u16, &str)> = (0..5_000).map(|f| (f, 0, largo.as_str())).collect();
        let bytes = xlsx(&celdas);
        assert!(es_demasiado(&spreadsheet(&bytes)));
    }

    /// Un zip con estas partes (sin comprimir lo que se pida).
    fn zip_con(partes: &[(&str, &[u8])]) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (nombre, datos) in partes {
            let metodo = if *nombre == "mimetype" {
                zip::CompressionMethod::Stored
            } else {
                zip::CompressionMethod::Deflated
            };
            let o = zip::write::SimpleFileOptions::default().compression_method(metodo);
            z.start_file(*nombre, o).unwrap();
            z.write_all(datos).unwrap();
        }
        z.finish().unwrap().into_inner()
    }

    #[test]
    fn un_zip_que_se_descomprime_en_demasiado_se_rechaza() {
        let ceros = vec![0u8; 101 * 1024 * 1024];
        let bytes = zip_con(&[("xl/media/relleno.bin", &ceros)]);
        assert!(bytes.len() < 1024 * 1024);
        assert!(es_demasiado(&comprobar(&bytes)));
    }

    #[test]
    fn un_xlsb_con_un_registro_que_declara_256_mb_se_rechaza() {
        // BrtBeginBook (0x0083: 0x83 0x01) con longitud 0xFF 0xFF 0xFF 0x7F = 2^28 - 1 bytes,
        // en un fichero de unos pocos bytes.
        let registro = [0x83, 0x01, 0xFF, 0xFF, 0xFF, 0x7F, 0, 0, 0];
        let bytes = zip_con(&[("xl/workbook.bin", &registro)]);
        assert!(bytes.len() < 1024);
        let r = comprobar(&bytes);
        assert!(es_demasiado(&r), "{r:?}");
        // También en una hoja (con mayúsculas como las escribe Excel).
        let bien = [0x83, 0x01, 0x00];
        let bytes = zip_con(&[
            ("xl/workbook.bin", &bien),
            ("xl/worksheets/Sheet1.bin", &registro),
        ]);
        assert!(es_demasiado(&comprobar(&bytes)));
        let bytes = a_tiempo(move || spreadsheet(&bytes));
        assert!(es_demasiado(&bytes), "{bytes:?}");
    }

    #[test]
    fn un_xlsb_con_registros_normales_pasa_y_lo_que_no_es_xlsb_no_se_recorre() {
        // Registros pequeños, uno vacío y uno cortado al final.
        let registros = [
            0x83, 0x01, 0x00, 0x9C, 0x01, 0x02, 0xAA, 0xBB, 0x84, 0x01, 0x05, 1,
        ];
        let bytes = zip_con(&[("xl/workbook.bin", &registros)]);
        assert!(comprobar(&bytes).is_ok());
        // printerSettings y similares no son registros: no se miran.
        let raro = [0x83, 0x01, 0xFF, 0xFF, 0xFF, 0x7F];
        let bytes = zip_con(&[
            ("xl/workbook.bin", &registros),
            ("xl/printerSettings/printerSettings1.bin", &raro),
        ]);
        assert!(comprobar(&bytes).is_ok());
        // Ni el proyecto de VBA ni un objeto incrustado (CFB).
        let mut cfb = vec![0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
        cfb.extend_from_slice(&raro);
        let bytes = zip_con(&[
            ("xl/workbook.bin", &registros),
            ("xl/vbaProject.bin", &raro),
            ("xl/embeddings/oleObject1.bin", &cfb),
            ("xl/worksheets/_rels/sheet1.bin.rels", &raro),
        ]);
        assert!(comprobar(&bytes).is_ok());
        // Sin `xl/workbook.bin` no es un .xlsb.
        let bytes = zip_con(&[("xl/worksheets/sheet1.bin", &raro)]);
        assert!(comprobar(&bytes).is_ok());
    }

    #[test]
    fn en_un_xlsb_tambien_se_recorren_las_demas_partes_bin() {
        // Partes que calamine 0.37.0 no lee, por si otra versión las lee registro a registro.
        let bien = [0x83, 0x01, 0x00];
        let raro = [0x83, 0x01, 0xFF, 0xFF, 0xFF, 0x7F];
        for parte in [
            "xl/comments1.bin",
            "xl/externalLinks/externalLink1.bin",
            "xl/pivotCache/pivotCacheRecords1.bin",
        ] {
            let bytes = zip_con(&[("xl/workbook.bin", &bien), (parte, &raro)]);
            assert!(es_demasiado(&comprobar(&bytes)), "{parte}");
            let bytes = zip_con(&[("xl/workbook.bin", &bien), (parte, &bien)]);
            assert!(comprobar(&bytes).is_ok(), "{parte}");
        }
        assert_eq!(
            xlsb::recorrido("xl/worksheets/sheet1.bin"),
            Some(xlsb::Recorrido::Conocida)
        );
        assert_eq!(xlsb::recorrido("xl/media/image1.png"), None);
    }

    #[test]
    fn demasiados_textos_compartidos_declarados_se_rechazan() {
        let sst = br#"<?xml version="1.0"?><sst xmlns="x" count="1" uniqueCount="99999999999999"><si><t>a</t></si></sst>"#;
        let bytes = zip_con(&[("xl/sharedStrings.xml", sst)]);
        assert!(es_demasiado(&comprobar(&bytes)));
        let sst = br#"<?xml version="1.0"?><sst xmlns="x" count="1" uniqueCount="1"><si><t>a</t></si></sst>"#;
        assert!(comprobar(&zip_con(&[("xl/sharedStrings.xml", sst)])).is_ok());
    }

    // ---------- .ods ----------

    fn ods(tablas: &str) -> Vec<u8> {
        let content = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:spreadsheet>{tablas}</office:spreadsheet></office:body></office:document-content>"#
        );
        let manifest = br#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.spreadsheet"/></manifest:manifest>"#;
        zip_con(&[
            (
                "mimetype",
                b"application/vnd.oasis.opendocument.spreadsheet",
            ),
            ("META-INF/manifest.xml", manifest),
            ("content.xml", content.as_bytes()),
        ])
    }

    const CELDA: &str = r#"<table:table-cell office:value-type="string"><text:p>Banco</text:p></table:table-cell><table:table-cell office:value-type="float" office:value="12.5"/>"#;

    #[test]
    fn un_ods_normal_se_sigue_leyendo() {
        let bytes = ods(&format!(
            r#"<table:table table:name="Hoja1"><table:table-row>{CELDA}<table:table-cell table:number-columns-repeated="1020"/></table:table-row><table:table-row table:number-rows-repeated="1048575"><table:table-cell table:number-columns-repeated="1024"/></table:table-row></table:table>"#
        ));
        comprobar(&bytes).unwrap();
        assert_eq!(spreadsheet(&bytes).unwrap(), [vec!["Banco", "12.5"]]);
    }

    #[test]
    fn un_ods_con_filas_y_celdas_repetidas_se_rechaza() {
        // 6.000 filas de 16.384 celdas: 98 millones de celdas, por debajo del tope de calamine.
        let bytes = ods(
            r#"<table:table table:name="H"><table:table-row table:number-rows-repeated="6000"><table:table-cell table:number-columns-repeated="16384" office:value-type="float" office:value="1"/></table:table-row></table:table>"#,
        );
        assert!(bytes.len() < 2_000);
        let r = a_tiempo(move || spreadsheet(&bytes));
        assert!(es_demasiado(&r), "{r:?}");
    }

    #[test]
    fn un_ods_con_millones_de_espacios_se_rechaza() {
        let bytes = ods(
            r#"<table:table table:name="H"><table:table-row><table:table-cell office:value-type="string"><text:p>a<text:s text:c="999999999999"/></text:p></table:table-cell></table:table-row></table:table>"#,
        );
        let r = a_tiempo(move || spreadsheet(&bytes));
        assert!(es_demasiado(&r), "{r:?}");
    }

    #[test]
    fn un_ods_con_una_hoja_sin_cerrar_no_deja_a_calamine_en_bucle() {
        let bytes = ods(r#"<table:table table:name="H"><table:table-row>"#);
        let r = a_tiempo(move || spreadsheet(&bytes));
        assert!(es_ilegible(&r), "{r:?}");
    }

    // ---------- .xls ----------

    fn registro(tipo: u16, datos: &[u8]) -> Vec<u8> {
        let mut out = tipo.to_le_bytes().to_vec();
        out.extend(u16::try_from(datos.len()).unwrap().to_le_bytes());
        out.extend(datos);
        out
    }

    fn numero(f: u16, c: u16, v: f64) -> Vec<u8> {
        let mut d = f.to_le_bytes().to_vec();
        d.extend(c.to_le_bytes());
        d.extend(0u16.to_le_bytes());
        d.extend(v.to_le_bytes());
        registro(0x0203, &d)
    }

    fn dimensiones(rf: u32, rl: u32, cf: u16, cl: u16) -> Vec<u8> {
        let mut d = rf.to_le_bytes().to_vec();
        d.extend(rl.to_le_bytes());
        d.extend(cf.to_le_bytes());
        d.extend(cl.to_le_bytes());
        d.extend(0u16.to_le_bytes());
        registro(0x0200, &d)
    }

    /// El flujo `Workbook` de un `.xls` (BIFF8) con una hoja hecha con estos registros.
    fn libro(hoja: &[Vec<u8>]) -> Vec<u8> {
        let bof = |dt: u16| {
            let mut d = 0x0600u16.to_le_bytes().to_vec();
            d.extend(dt.to_le_bytes());
            d.extend([0u8; 12]);
            registro(0x0809, &d)
        };
        let nombre = b"Hoja1";
        let hoja_meta = |pos: u32| {
            let mut d = pos.to_le_bytes().to_vec();
            d.extend([0u8, 0u8, u8::try_from(nombre.len()).unwrap(), 0u8]);
            d.extend(nombre);
            registro(0x0085, &d)
        };
        let mut globales = bof(0x0005);
        globales.extend(registro(0x0042, &1200u16.to_le_bytes()));
        let largo_meta = hoja_meta(0).len();
        // La hoja va después de un relleno, para que el flujo pase de 4.096 bytes (sin el
        // minisector).
        let pos = globales.len() + largo_meta + 4 + 5000;
        globales.extend(hoja_meta(u32::try_from(pos).unwrap()));
        globales.extend(registro(0x000A, &[]));
        globales.resize(pos, 0);
        globales.extend(bof(0x0010));
        for r in hoja {
            globales.extend(r);
        }
        globales.extend(registro(0x000A, &[]));
        globales
    }

    const FREESECT: u32 = 0xFFFF_FFFF;
    const FIN: u32 = 0xFFFF_FFFE;

    /// Un fichero compuesto de OLE (sectores de 512) con el flujo `Workbook`; `bucle` hace que la
    /// cadena del directorio dé la vuelta.
    fn compuesto(flujo: &[u8], bucle: bool) -> Vec<u8> {
        let n = flujo.len().div_ceil(512);
        let mut h = vec![0u8; 512];
        h[..8].copy_from_slice(&CFB_FIRMA);
        h[24..26].copy_from_slice(&0x003Eu16.to_le_bytes());
        h[26..28].copy_from_slice(&3u16.to_le_bytes());
        h[28..30].copy_from_slice(&0xFFFEu16.to_le_bytes());
        h[30..32].copy_from_slice(&9u16.to_le_bytes());
        h[32..34].copy_from_slice(&6u16.to_le_bytes());
        h[44..48].copy_from_slice(&1u32.to_le_bytes());
        h[48..52].copy_from_slice(&1u32.to_le_bytes());
        h[56..60].copy_from_slice(&4096u32.to_le_bytes());
        h[60..64].copy_from_slice(&FIN.to_le_bytes());
        h[68..72].copy_from_slice(&FIN.to_le_bytes());
        for i in 0..109 {
            let v = if i == 0 { 0 } else { FREESECT };
            h[76 + i * 4..80 + i * 4].copy_from_slice(&v.to_le_bytes());
        }
        // FAT: sector 0 la FAT, 1 el directorio, 2.. el flujo.
        let mut fat = vec![0xFFFF_FFFDu32, if bucle { 1 } else { FIN }];
        for i in 0..n {
            fat.push(if i + 1 == n {
                FIN
            } else {
                u32::try_from(i + 3).unwrap()
            });
        }
        fat.resize(128, FREESECT);
        let mut out = h;
        for v in fat {
            out.extend(v.to_le_bytes());
        }
        let entrada = |nombre: &str, tipo: u8, hijo: u32, inicio: u32, len: u32| {
            let mut e = vec![0u8; 128];
            let utf16: Vec<u16> = nombre.encode_utf16().collect();
            for (i, c) in utf16.iter().enumerate() {
                e[i * 2..i * 2 + 2].copy_from_slice(&c.to_le_bytes());
            }
            e[64..66].copy_from_slice(&u16::try_from((utf16.len() + 1) * 2).unwrap().to_le_bytes());
            e[66] = tipo;
            e[67] = 1;
            e[68..72].copy_from_slice(&FREESECT.to_le_bytes());
            e[72..76].copy_from_slice(&FREESECT.to_le_bytes());
            e[76..80].copy_from_slice(&hijo.to_le_bytes());
            e[116..120].copy_from_slice(&inicio.to_le_bytes());
            e[120..124].copy_from_slice(&len.to_le_bytes());
            e
        };
        out.extend(entrada("Root Entry", 5, 1, FIN, 0));
        out.extend(entrada(
            "Workbook",
            2,
            FREESECT,
            2,
            u32::try_from(flujo.len()).unwrap(),
        ));
        out.extend(entrada("", 0, FREESECT, 0, 0));
        out.extend(entrada("", 0, FREESECT, 0, 0));
        let mut datos = flujo.to_vec();
        datos.resize(n * 512, 0);
        out.extend(datos);
        out
    }

    #[test]
    fn un_xls_normal_se_sigue_leyendo() {
        let bytes = compuesto(
            &libro(&[
                dimensiones(0, 2, 0, 2),
                numero(0, 0, 1.0),
                numero(1, 1, 2.5),
            ]),
            false,
        );
        comprobar(&bytes).unwrap();
        assert_eq!(
            spreadsheet(&bytes).unwrap(),
            [vec!["1", ""], vec!["", "2.5"]]
        );
    }

    #[test]
    fn un_xls_con_celdas_en_las_esquinas_se_rechaza() {
        let bytes = compuesto(
            &libro(&[numero(0, 0, 1.0), numero(65_535, 255, 2.0)]),
            false,
        );
        let r = a_tiempo(move || spreadsheet(&bytes));
        assert!(es_demasiado(&r), "{r:?}");
    }

    #[test]
    fn un_xls_con_dimensiones_enormes_se_rechaza() {
        let bytes = compuesto(
            &libro(&[dimensiones(0, 1_000_000, 0, 255), numero(0, 0, 1.0)]),
            false,
        );
        assert!(es_demasiado(&comprobar(&bytes)));
        // Al revés (la última fila antes que la primera): calamine restaría sin signo.
        let bytes = compuesto(
            &libro(&[dimensiones(10, 2, 0, 2), numero(0, 0, 1.0)]),
            false,
        );
        assert!(es_ilegible(&comprobar(&bytes)));
    }

    #[test]
    fn un_xls_con_la_cadena_de_sectores_en_bucle_no_deja_a_calamine_en_bucle() {
        let bytes = compuesto(&libro(&[numero(0, 0, 1.0)]), true);
        let r = a_tiempo(move || spreadsheet(&bytes));
        assert!(es_ilegible(&r), "{r:?}");
    }

    /// Las hojas de cálculo de verdad del repositorio (diseños de registro de la AEAT, libros de
    /// ejemplo) pasan la comprobación y se leen.
    #[test]
    fn las_hojas_de_los_fixtures_pasan() {
        let raiz = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let mut pendientes = vec![raiz];
        let mut vistas = 0;
        while let Some(d) = pendientes.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                let p = e.path();
                if p.is_dir() {
                    pendientes.push(p);
                    continue;
                }
                let ext = p
                    .extension()
                    .map(|x| x.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                if ["xls", "xlsx", "xlsm", "ods"].contains(&ext.as_str()) {
                    let bytes = std::fs::read(&p).unwrap();
                    comprobar(&bytes).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
                    spreadsheet_sheets(&bytes).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
                    vistas += 1;
                }
            }
        }
        assert!(vistas >= 5, "{vistas}");
    }
}
