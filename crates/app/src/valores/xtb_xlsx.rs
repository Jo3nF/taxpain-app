//! Informe de cuenta de XTB en Excel (xStation 5 → «Historial de la cuenta» → Exportar →
//! Excel): un libro por cuenta (una cuenta por divisa), con el nombre `EUR_<cuenta>_<desde>_
//! <hasta>.xlsx`, y tres hojas, cada una con la cabecera de metadatos (`Account number`,
//! `Date from (UTC)`, `Date to (UTC)`) y luego su tabla:
//! - `Closed Positions`: Instrument, Ticker, Category (STOCK, ETF, ETC, OPTION…), Type, Volume,
//!   Open Price, Open Time (UTC), Close Price, Close Time (UTC), Product, Profit/Loss, Gross
//!   Profit, Purchase Value, Sale Value, …, Position ID, Comment; al final, la fila del total.
//! - `Cash Operations`: Type («Stock purchase», «Stock sell», «SEC fee», «Free funds interest»,
//!   «Free funds interest tax», «Option BTO raw premium», dividendos, retenciones, ingresos y
//!   retiradas…), Instrument, Ticker, Category, Time, Amount (en la divisa de la cuenta), ID,
//!   Comment (`OPEN BUY ACME.US 10/10.5 @ 100.00`: 10 títulos de una orden de 10,5 al
//!   precio, con o sin el símbolo), Product, Position ID.
//! - `Open Positions`: un resumen y, por valor, una fila con el total y otra por posición
//!   (Volume, Value en la divisa de la cuenta, Current price, Open price, Open time).
//!
//! Diferencias con el PDF ([`super::xtb`]): las horas van en UTC (aquí se pasan a la hora
//! peninsular, la del PDF); la venta («Stock sell») trae lo cobrado, sin la fila «close trade»
//! con el beneficio; y los ID de las operaciones en efectivo son los mismos que en el PDF, así
//! que lo que ya entró de un PDF (o de otro Excel) no se duplica.
//!
//! Lo que no está en «Cash Operations» porque es de antes de `Date from` (la compra de una
//! posición cerrada o abierta en el periodo) se toma de las posiciones, como en el PDF sin
//! operaciones en efectivo (`xtb-posiciones`), con una huella que no depende del informe: si ya
//! está (del informe anterior) no entra, y si llega después el informe anterior, lo sustituye.
//!
//! Opciones: la prima pagada («Option BTO raw premium») es la compra de un valor de clase
//! [`Clase::Derivado`] y su cierre o vencimiento (la fila de `Closed Positions`) su venta, por
//! lo cobrado (cero si vence sin valor): el resultado sale en el informe en la línea de
//! derivados. Las opciones lanzadas (vendidas) no se importan: se avisa con su resultado.
//!
//! `TODO(VERIFY)`: los nombres de las hojas y columnas con xStation en español (se reconocen
//! también unos supuestos, sin haberlos visto en un fichero real) y el multiplicador de 100 de
//! las opciones sobre acciones de EE. UU. cuando no hay prima con la que deducirlo.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use calamine::{Data, Reader};
use regex::Regex;
use taxpain_core::cripto::{Precio, Qty};
use taxpain_core::money::Money;
use time::{Date, Month, OffsetDateTime, PrimitiveDateTime, Time};

use super::importar::{Extracto, OpLeida, PosicionLeida, PrecioLeido, ValorLeido};
use super::xtb::{decimal, dinero, divisa_mercado, pais_mercado, precio, titulos};
use super::{Broker, Clase, TipoOp};

/// Multiplicador de las opciones sobre acciones de EE. UU. (100 acciones por contrato) cuando
/// no hay prima de la que sacarlo.
const MULTIPLICADOR_OPCION: i64 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hoja {
    Cerradas,
    Efectivo,
    Abiertas,
}

/// Minúsculas, sin acentos, sin «(UTC)» y con un solo espacio.
fn normal(s: &str) -> String {
    let t: String = s
        .to_lowercase()
        .chars()
        .map(|c| match c {
            'á' | 'à' => 'a',
            'é' | 'è' => 'e',
            'í' => 'i',
            'ó' | 'ò' => 'o',
            'ú' | 'ü' => 'u',
            c => c,
        })
        .collect();
    t.replace("(utc)", " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn hoja_de(nombre: &str) -> Option<Hoja> {
    let n = normal(nombre);
    if n.contains("closed position") || n.contains("posiciones cerradas") {
        Some(Hoja::Cerradas)
    } else if n.contains("cash operation")
        || n.contains("operaciones de caja")
        || n.contains("operaciones en efectivo")
    {
        Some(Hoja::Efectivo)
    } else if n.contains("open position") || n.contains("posiciones abiertas") {
        Some(Hoja::Abiertas)
    } else {
        None
    }
}

/// Columnas que se usan, con los nombres en inglés y (supuestos) en español.
const COLUMNAS: &[(&str, &[&str])] = &[
    ("tipo", &["type", "tipo"]),
    (
        "instrumento",
        &[
            "instrument",
            "instrumento",
            "instrument/position",
            "instrumento/posicion",
        ],
    ),
    ("ticker", &["ticker", "symbol", "simbolo"]),
    ("categoria", &["category", "categoria"]),
    ("hora", &["time", "hora", "fecha", "fecha y hora"]),
    ("importe", &["amount", "importe"]),
    ("id", &["id"]),
    ("comentario", &["comment", "comentario"]),
    (
        "posicion",
        &["position id", "id de posicion", "id posicion", "posicion"],
    ),
    ("volumen", &["volume", "volumen"]),
    (
        "precio_apertura",
        &["open price", "precio de apertura", "precio apertura"],
    ),
    (
        "hora_apertura",
        &["open time", "hora de apertura", "fecha de apertura"],
    ),
    ("precio_cierre", &["close price", "precio de cierre"]),
    (
        "hora_cierre",
        &["close time", "hora de cierre", "fecha de cierre"],
    ),
    (
        "resultado",
        &[
            "profit/loss",
            "beneficio/perdida",
            "ganancia/perdida",
            "beneficios/perdidas",
        ],
    ),
    ("valor_compra", &["purchase value", "valor de compra"]),
    ("valor_venta", &["sale value", "valor de venta"]),
    ("valor", &["value", "valor"]),
    (
        "precio_actual",
        &["current price", "precio actual", "precio de mercado"],
    ),
];

fn columna(cabecera: &str) -> Option<&'static str> {
    let n = normal(cabecera);
    COLUMNAS
        .iter()
        .find(|(_, alias)| alias.contains(&n.as_str()))
        .map(|(k, _)| *k)
}

/// Una celda como texto (los números sin coma flotante a la vista: con 8 decimales como mucho).
fn texto(c: &Data) -> String {
    match c {
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => s.trim().to_owned(),
        Data::Float(f) => num(*f),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => b.to_string(),
        Data::DateTime(d) => num(d.as_f64()),
        Data::Error(_) | Data::Empty => String::new(),
    }
}

fn num(f: f64) -> String {
    let s = format!("{f:.8}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_owned() }
}

/// Instante de una celda: fecha de Excel, número de serie o texto (`2026-10-30 13:11:14.430`,
/// `30/10/2026 13:11:14`…), en UTC.
fn instante(c: &Data) -> Option<OffsetDateTime> {
    match c {
        Data::DateTime(d) => {
            let (y, m, dia, h, min, s, _) = d.to_ymd_hms_milli();
            let fecha =
                Date::from_calendar_date(i32::from(y), Month::try_from(m).ok()?, dia).ok()?;
            Some(PrimitiveDateTime::new(fecha, Time::from_hms(h, min, s).ok()?).assume_utc())
        }
        Data::Float(f) if *f > 1.0 => serie(&num(*f)),
        Data::Int(i) if *i > 1 => serie(&i.to_string()),
        Data::String(s) | Data::DateTimeIso(s) => instante_texto(s),
        _ => None,
    }
}

/// Número de serie de Excel (días desde el 30-12-1899 y la fracción del día), sin coma flotante.
fn serie(s: &str) -> Option<OffsetDateTime> {
    const UNO: i128 = 100_000_000;
    let base = Date::from_calendar_date(1899, Month::December, 30).ok()?;
    let v = decimal(s, 8)?;
    let dias = i64::try_from(v / UNO).ok()?;
    let segundos = i64::try_from((v % UNO * 86_400 * 2 + UNO) / (2 * UNO)).ok()?;
    let fecha = base.checked_add(time::Duration::days(dias))?;
    Some(
        PrimitiveDateTime::new(fecha, Time::MIDNIGHT).assume_utc()
            + time::Duration::seconds(segundos),
    )
}

static FECHA_ISO: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{4})-(\d{2})-(\d{2})(?:[ T](\d{2}):(\d{2})(?::(\d{2}))?)?")
        .expect("patrón fijo válido")
});
static FECHA_ES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(\d{2})[./\-](\d{2})[./\-](\d{4})(?:\s+(\d{2}):(\d{2})(?::(\d{2}))?)?")
        .expect("patrón fijo válido")
});
/// `OPEN BUY ACME.US 10/10.5 @ 100.00`, `CLOSE BUY 6 @ 110.00`.
static ORDEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(OPEN|CLOSE)\s+(BUY|SELL)\s+(?:(\S*[A-Za-z]\S*)\s+)?([\d.,]+)(?:\s*/\s*([\d.,]+))?\s*@\s*([\d.,]+)",
    )
    .expect("patrón fijo válido")
});
static OPCION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)option_instrument:\s*(\S+)").expect("patrón fijo válido"));

fn instante_texto(s: &str) -> Option<OffsetDateTime> {
    let s = s.trim();
    let (y, m, d, h, mi, se) = if let Some(c) = FECHA_ISO.captures(s) {
        (
            c[1].parse().ok()?,
            c[2].parse::<u8>().ok()?,
            c[3].parse().ok()?,
            c.get(4),
            c.get(5),
            c.get(6),
        )
    } else {
        let c = FECHA_ES.captures(s)?;
        (
            c[3].parse().ok()?,
            c[2].parse::<u8>().ok()?,
            c[1].parse().ok()?,
            c.get(4),
            c.get(5),
            c.get(6),
        )
    };
    let n = |x: Option<regex::Match<'_>>| x.and_then(|x| x.as_str().parse::<u8>().ok());
    let fecha = Date::from_calendar_date(y, Month::try_from(m).ok()?, d).ok()?;
    let hora = Time::from_hms(n(h).unwrap_or(0), n(mi).unwrap_or(0), n(se).unwrap_or(0)).ok()?;
    Some(PrimitiveDateTime::new(fecha, hora).assume_utc())
}

/// Fecha y hora peninsulares de un instante en UTC.
fn local(t: OffsetDateTime) -> (Date, String) {
    let l = t.to_offset(taxpain_core::spain_time::peninsular_offset(t));
    (
        l.date(),
        format!("{:02}:{:02}:{:02}", l.hour(), l.minute(), l.second()),
    )
}

/// Una hoja: los metadatos de arriba (clave normalizada → celda) y las filas de la tabla.
struct Tabla {
    meta: BTreeMap<String, Data>,
    /// Filas de antes de la cabecera (el resumen de «Open Positions»).
    previas: Vec<Vec<Data>>,
    cols: BTreeMap<&'static str, usize>,
    filas: Vec<Vec<Data>>,
}

impl Tabla {
    fn celda<'a>(&self, fila: &'a [Data], col: &str) -> Option<&'a Data> {
        self.cols.get(col).and_then(|i| fila.get(*i))
    }

    fn txt(&self, fila: &[Data], col: &str) -> String {
        self.celda(fila, col).map(texto).unwrap_or_default()
    }

    fn t(&self, fila: &[Data], col: &str) -> Option<OffsetDateTime> {
        self.celda(fila, col).and_then(instante)
    }
}

fn requeridas(h: Hoja) -> &'static [&'static str] {
    match h {
        Hoja::Efectivo => &["tipo", "hora", "importe", "id"],
        Hoja::Cerradas => &[
            "ticker",
            "volumen",
            "precio_apertura",
            "hora_apertura",
            "precio_cierre",
            "hora_cierre",
        ],
        Hoja::Abiertas => &["ticker", "volumen", "tipo"],
    }
}

fn tabla(range: &calamine::Range<Data>, h: Hoja) -> Option<Tabla> {
    let mut meta = BTreeMap::new();
    let mut previas = Vec::new();
    let mut filas = range.rows();
    let req = requeridas(h);
    for fila in filas.by_ref() {
        let mut cols: BTreeMap<&'static str, usize> = BTreeMap::new();
        for (i, c) in fila.iter().enumerate() {
            if let Data::String(s) = c
                && let Some(k) = columna(s)
            {
                cols.entry(k).or_insert(i);
            }
        }
        if req.iter().all(|k| cols.contains_key(k)) {
            let filas: Vec<Vec<Data>> = filas
                .map(<[Data]>::to_vec)
                .filter(|f| f.iter().any(|c| !texto(c).is_empty()))
                .collect();
            return Some(Tabla {
                meta,
                previas,
                cols,
                filas,
            });
        }
        if let (Some(Data::String(k)), Some(v)) = (fila.first(), fila.get(1))
            && !texto(v).is_empty()
        {
            meta.entry(normal(k)).or_insert_with(|| v.clone());
        }
        previas.push(fila.to_vec());
    }
    None
}

/// El libro y sus tablas (`Ok(None)` si no es el informe de XTB). Con los topes de
/// [`crate::import::abrir_libro`]: un libro que pasa de ellos es un error, no «otro fichero».
fn abrir(bytes: &[u8]) -> Result<Option<BTreeMap<Hoja, Tabla>>, String> {
    if !bytes.starts_with(b"PK") {
        return Ok(None);
    }
    let mut wb = match crate::import::abrir_libro(bytes) {
        Ok(wb) => wb,
        Err(crate::import::Libro::Grande(e)) => return Err(e.to_string()),
        Err(crate::import::Libro::NoAbre(_)) => return Ok(None),
    };
    let mut out = BTreeMap::new();
    for nombre in wb.sheet_names() {
        let Some(h) = hoja_de(&nombre) else { continue };
        let Some(range) = crate::import::rango_hoja(&mut wb, &nombre).map_err(|e| e.to_string())?
        else {
            continue;
        };
        if let Some(t) = tabla(&range, h) {
            out.insert(h, t);
        }
    }
    let cuenta = out
        .values()
        .any(|t| meta(t, &["account number", "numero de cuenta", "cuenta"]).is_some());
    Ok(
        (cuenta && (out.contains_key(&Hoja::Efectivo) || out.contains_key(&Hoja::Cerradas)))
            .then_some(out),
    )
}

impl PartialOrd for Hoja {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for Hoja {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        (*self as u8).cmp(&(*o as u8))
    }
}

fn meta<'a>(t: &'a Tabla, claves: &[&str]) -> Option<&'a Data> {
    t.meta
        .iter()
        .find(|(k, _)| claves.iter().any(|c| k.as_str() == *c || k.starts_with(c)))
        .map(|(_, v)| v)
}

/// ¿Es el informe de cuenta de XTB en Excel?
pub fn es_informe(bytes: &[u8]) -> bool {
    matches!(abrir(bytes), Ok(Some(_)))
}

/// Divisa de la cuenta por el nombre del fichero (`USD_99000001_…`).
fn divisa_por_nombre(nombre: &str) -> Option<String> {
    let base = nombre.rsplit(['/', '\\']).next().unwrap_or(nombre);
    let (d, resto) = base.split_once('_')?;
    (d.len() == 3
        && d.chars().all(|c| c.is_ascii_alphabetic())
        && resto.starts_with(|c: char| c.is_ascii_digit()))
    .then(|| d.to_uppercase())
}

#[derive(Clone, Debug)]
struct Mov {
    id: String,
    tipo: String,
    ticker: String,
    nombre: String,
    categoria: String,
    t: OffsetDateTime,
    importe: Money,
    comentario: String,
    posicion: String,
}

#[derive(Clone, Debug)]
struct Cerrada {
    ticker: String,
    nombre: String,
    categoria: String,
    compra: bool,
    volumen: Qty,
    precio_apertura: Precio,
    apertura: OffsetDateTime,
    precio_cierre: Precio,
    cierre: OffsetDateTime,
    resultado: Option<Money>,
    valor_compra: Option<Money>,
    valor_venta: Option<Money>,
    posicion: String,
}

#[derive(Clone, Debug)]
struct Abierta {
    ticker: String,
    nombre: String,
    categoria: String,
    compra: bool,
    volumen: Qty,
    valor: Option<Money>,
    precio_actual: Option<Precio>,
    precio_apertura: Option<Precio>,
    apertura: Option<OffsetDateTime>,
    posicion: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Clasificado {
    Compra,
    Venta,
    /// «close trade»: el beneficio de una venta (como en el PDF).
    Cierre,
    Opcion,
    ImpuestoIntereses,
    /// SEC fee y otras comisiones de una venta o compra.
    Comision,
    Op(TipoOp),
    Cfd,
    Desconocido,
}

fn clasificar(tipo: &str, categoria: &str) -> Clasificado {
    let t = normal(tipo);
    let tiene = |xs: &[&str]| xs.iter().any(|x| t.contains(x));
    if tiene(&["option", "opcion"]) || normal(categoria) == "option" {
        Clasificado::Opcion
    } else if tiene(&["cfd", "swap", "rollover"]) {
        Clasificado::Cfd
    } else if tiene(&["close trade", "cierre"]) {
        Clasificado::Cierre
    } else if tiene(&["purchase", "compra"]) {
        Clasificado::Compra
    } else if tiene(&["sell", "sale", "venta"]) {
        Clasificado::Venta
    } else if tiene(&["interest", "interes"]) && tiene(&["tax", "impuesto"]) {
        Clasificado::ImpuestoIntereses
    } else if tiene(&["withholding", "retenc"]) {
        Clasificado::Op(TipoOp::Retencion)
    } else if tiene(&["divid"]) {
        Clasificado::Op(TipoOp::Dividendo)
    } else if tiene(&["interest", "interes"]) {
        Clasificado::Op(TipoOp::Interes)
    } else if tiene(&["deposit", "deposito", "ingreso"]) {
        Clasificado::Op(TipoOp::Deposito)
    } else if tiene(&["withdraw", "retirada", "retiro"]) {
        Clasificado::Op(TipoOp::Retirada)
    } else if tiene(&["fee", "commission", "comision"]) {
        Clasificado::Comision
    } else if tiene(&["tax", "impuesto"]) {
        Clasificado::Op(TipoOp::Impuesto)
    } else {
        Clasificado::Desconocido
    }
}

fn clase(categoria: &str) -> Clase {
    match normal(categoria).as_str() {
        "etf" => Clase::Etf,
        "stock" | "accion" | "acciones" => Clase::Accion,
        "option" | "opcion" => Clase::Derivado,
        "cfd" => Clase::Cfd,
        "bond" | "bono" => Clase::Bono,
        "" => Clase::Accion,
        // ETC (materias primas) y lo demás.
        _ => Clase::Otro,
    }
}

fn es_opcion(categoria: &str) -> bool {
    clase(categoria) == Clase::Derivado
}

fn qty_txt(s: &str) -> Option<Qty> {
    decimal(s, 8).and_then(|v| Qty::parse(&Precio::from_raw(v).to_text()))
}

fn movimientos(t: &Tabla) -> Vec<Mov> {
    t.filas
        .iter()
        .filter_map(|f| {
            let id = t.txt(f, "id");
            if id.is_empty() || !id.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            Some(Mov {
                id,
                tipo: t.txt(f, "tipo"),
                ticker: t.txt(f, "ticker").to_uppercase(),
                nombre: t.txt(f, "instrumento"),
                categoria: t.txt(f, "categoria"),
                t: t.t(f, "hora")?,
                importe: dinero(&t.txt(f, "importe"))?,
                comentario: t.txt(f, "comentario"),
                posicion: t.txt(f, "posicion"),
            })
        })
        .collect()
}

fn cerradas(t: &Tabla) -> Vec<Cerrada> {
    t.filas
        .iter()
        .filter_map(|f| {
            let ticker = t.txt(f, "ticker").to_uppercase();
            if ticker.is_empty() {
                return None;
            }
            let tipo = normal(&t.txt(f, "tipo"));
            Some(Cerrada {
                ticker,
                nombre: t.txt(f, "instrumento"),
                categoria: t.txt(f, "categoria"),
                compra: !matches!(tipo.as_str(), "sell" | "venta"),
                volumen: qty_txt(&t.txt(f, "volumen"))?,
                precio_apertura: precio(&t.txt(f, "precio_apertura"))?,
                apertura: t.t(f, "hora_apertura")?,
                precio_cierre: precio(&t.txt(f, "precio_cierre")).unwrap_or(Precio::ZERO),
                cierre: t.t(f, "hora_cierre")?,
                resultado: dinero(&t.txt(f, "resultado")),
                valor_compra: dinero(&t.txt(f, "valor_compra")),
                valor_venta: dinero(&t.txt(f, "valor_venta")),
                posicion: t.txt(f, "posicion"),
            })
        })
        .collect()
}

fn abiertas(t: &Tabla) -> Vec<Abierta> {
    let mut categoria: BTreeMap<String, (String, String)> = BTreeMap::new();
    let mut out = Vec::new();
    for f in &t.filas {
        let ticker = t.txt(f, "ticker").to_uppercase();
        if ticker.is_empty() {
            continue;
        }
        let tipo = normal(&t.txt(f, "tipo"));
        // La fila del total del valor trae el nombre y la categoría; la de cada posición, el ID.
        if !matches!(tipo.as_str(), "buy" | "sell" | "compra" | "venta") {
            categoria.insert(ticker, (t.txt(f, "instrumento"), t.txt(f, "categoria")));
            continue;
        }
        let Some(volumen) = qty_txt(&t.txt(f, "volumen")) else {
            continue;
        };
        let (nombre, cat) = categoria.get(&ticker).cloned().unwrap_or_default();
        let cat = if t.txt(f, "categoria").is_empty() {
            cat
        } else {
            t.txt(f, "categoria")
        };
        out.push(Abierta {
            nombre,
            categoria: cat,
            compra: matches!(tipo.as_str(), "buy" | "compra"),
            volumen,
            valor: dinero(&t.txt(f, "valor")),
            precio_actual: precio(&t.txt(f, "precio_actual")),
            precio_apertura: precio(&t.txt(f, "precio_apertura")),
            apertura: t.t(f, "hora_apertura"),
            posicion: t.txt(f, "instrumento"),
            ticker,
        });
    }
    out
}

/// `importe / títulos` como precio por título.
fn por_titulo(importe: Money, q: Qty) -> Option<Precio> {
    q.precio_de(importe.abs())
}

fn cerca(a: OffsetDateTime, b: OffsetDateTime, segundos: i64) -> bool {
    (a - b).whole_seconds().abs() <= segundos
}

/// Lee el informe de cuenta de XTB en Excel. `nombre`: el del fichero (da la divisa).
pub fn leer(nombre: &str, bytes: &[u8]) -> Result<Extracto, String> {
    let tablas = abrir(bytes)?.ok_or_else(|| {
        "no es un informe de XTB en Excel que se pueda leer (expórtalo de nuevo desde «Historial \
         de la cuenta» de xStation 5)"
            .to_owned()
    })?;
    let cuenta = tablas
        .values()
        .find_map(|t| meta(t, &["account number", "numero de cuenta", "cuenta"]))
        .map(texto)
        .filter(|c| !c.is_empty())
        .ok_or("no se encuentra el número de cuenta de XTB en el informe")?;
    let mut avisos = Vec::new();
    let divisa = divisa_por_nombre(nombre)
        .or_else(|| {
            tablas.get(&Hoja::Abiertas).and_then(|t| {
                t.previas.iter().flatten().find_map(|c| match c {
                    Data::String(s)
                        if s.len() == 3 && s.chars().all(|c| c.is_ascii_uppercase()) =>
                    {
                        Some(s.clone())
                    }
                    _ => None,
                })
            })
        })
        .unwrap_or_else(|| {
            avisos.push(
                "No se sabe la divisa de la cuenta (ni por el nombre del fichero ni por el \
                 informe): se toma el euro. Si no es así, renombra el fichero como lo exporta \
                 xStation (`USD_<cuenta>_…`) e impórtalo de nuevo."
                    .to_owned(),
            );
            "EUR".into()
        });
    let fecha_meta = |claves: &[&str]| {
        tablas
            .values()
            .find_map(|t| meta(t, claves))
            .and_then(instante)
    };
    let desde_t = fecha_meta(&["date from", "fecha desde", "desde"]);
    let hasta_t = fecha_meta(&["date to", "fecha hasta", "hasta"]);
    let generado_t = tablas
        .get(&Hoja::Abiertas)
        .and_then(|t| meta(t, &["data as of", "datos a", "generado"]))
        .and_then(instante)
        .or(hasta_t);

    let movs = tablas
        .get(&Hoja::Efectivo)
        .map(movimientos)
        .unwrap_or_default();
    let cerr = tablas
        .get(&Hoja::Cerradas)
        .map(cerradas)
        .unwrap_or_default();
    let abie = tablas
        .get(&Hoja::Abiertas)
        .map(abiertas)
        .unwrap_or_default();
    let desde_t = desde_t.or_else(|| movs.iter().map(|m| m.t).min());
    let hasta_t = hasta_t.or_else(|| movs.iter().map(|m| m.t).max());

    let mut e = Extracto::nuevo(Broker::Xtb, &cuenta, &divisa);
    let mut valores: BTreeMap<String, ValorLeido> = BTreeMap::new();
    let mut valor = |ticker: &str, nombre: &str, categoria: &str, clase_: Clase| {
        let v = valores
            .entry(ticker.to_owned())
            .or_insert_with(|| ValorLeido {
                clave: ticker.to_owned(),
                isin: String::new(),
                ticker: ticker.to_owned(),
                nombre: String::new(),
                divisa: if clase_ == Clase::Derivado {
                    divisa.clone()
                } else {
                    divisa_mercado(ticker).unwrap_or(&divisa).to_owned()
                },
                clase: clase_,
                pais: String::new(),
            });
        if v.nombre.is_empty() && !nombre.is_empty() {
            v.nombre = nombre.to_owned();
        }
        if !categoria.is_empty() {
            v.clase = clase_;
        }
    };
    let huella = |id: &str| format!("xtb:{cuenta}:{id}");
    let mut cfd = 0usize;
    let mut desconocidos: BTreeMap<String, usize> = BTreeMap::new();
    // Ventas por posición (índice en `e.ops`), para las comisiones y el cuadre.
    let mut ventas: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut compras_pos: BTreeSet<(String, String)> = BTreeSet::new();
    let base = |m: &Mov| {
        let (fecha, hora) = local(m.t);
        OpLeida {
            fecha,
            hora,
            tipo: TipoOp::Comision,
            valor: (!m.ticker.is_empty()).then(|| m.ticker.clone()),
            cantidad: Qty::ZERO,
            precio: None,
            divisa: divisa.clone(),
            importe: m.importe,
            comision: Money::ZERO,
            cambio: None,
            euros: m.importe,
            comision_eur: Money::ZERO,
            pais: String::new(),
            nota: m.comentario.clone(),
            fuente: "xtb".into(),
            huella: huella(&m.id),
            comision_huella: Vec::new(),
        }
    };

    // 1) Compras, ventas, intereses, dividendos, ingresos y retiradas.
    for m in &movs {
        match clasificar(&m.tipo, &m.categoria) {
            k @ (Clasificado::Compra | Clasificado::Venta) => {
                let compra = k == Clasificado::Compra;
                let Some(c) = ORDEN.captures(&m.comentario) else {
                    avisos.push(format!(
                        "«{}» del {} ({}): no se entiende el comentario «{}»; no se importa",
                        m.tipo,
                        local(m.t).0,
                        m.id,
                        m.comentario
                    ));
                    continue;
                };
                if c[2].eq_ignore_ascii_case("SELL") {
                    cfd += 1;
                    continue;
                }
                let ticker = if m.ticker.is_empty() {
                    c.get(3)
                        .map(|x| x.as_str().to_uppercase())
                        .unwrap_or_default()
                } else {
                    m.ticker.clone()
                };
                let (Some(q), Some(pr)) = (titulos(&c[4]), precio(&c[6])) else {
                    continue;
                };
                if ticker.is_empty() {
                    continue;
                }
                valor(&ticker, &m.nombre, &m.categoria, clase(&m.categoria));
                let mut nota = m.comentario.clone();
                if !m.posicion.is_empty() {
                    nota = format!("{nota} · posición {}", m.posicion);
                }
                if compra {
                    compras_pos.insert((m.posicion.clone(), q.to_text()));
                } else {
                    ventas
                        .entry(m.posicion.clone())
                        .or_default()
                        .push(e.ops.len());
                }
                e.ops.push(OpLeida {
                    tipo: if compra {
                        TipoOp::Compra
                    } else {
                        TipoOp::Venta
                    },
                    valor: Some(ticker),
                    cantidad: q,
                    precio: Some(pr),
                    importe: m.importe.abs(),
                    euros: m.importe.abs(),
                    nota,
                    ..base(m)
                });
            }
            Clasificado::ImpuestoIntereses | Clasificado::Op(_) if m.importe.is_zero() => {}
            Clasificado::ImpuestoIntereses => e.ops.push(OpLeida {
                tipo: TipoOp::Retencion,
                importe: -m.importe,
                euros: -m.importe,
                // Como en el PDF: la retención de XTB (sucursal en España) sobre los intereses
                // del efectivo, española (473). `TODO(VERIFY)` con su certificado.
                pais: "ES".into(),
                ..base(m)
            }),
            Clasificado::Op(t) => {
                let importe = match t {
                    TipoOp::Retencion | TipoOp::Comision | TipoOp::Impuesto => -m.importe,
                    TipoOp::Deposito | TipoOp::Retirada => m.importe.abs(),
                    _ => m.importe,
                };
                if !m.ticker.is_empty() {
                    valor(&m.ticker, &m.nombre, &m.categoria, clase(&m.categoria));
                }
                e.ops.push(OpLeida {
                    tipo: t,
                    importe,
                    euros: importe,
                    pais: if m.ticker.is_empty() {
                        String::new()
                    } else {
                        pais_mercado(&m.ticker)
                    },
                    ..base(m)
                });
            }
            Clasificado::Cfd => cfd += 1,
            Clasificado::Desconocido => *desconocidos.entry(m.tipo.clone()).or_default() += 1,
            Clasificado::Cierre | Clasificado::Comision | Clasificado::Opcion => {}
        }
    }

    // 2) «close trade» (si el informe los trae, la venta es lo que costó: se suma el
    // beneficio), comisiones de las ventas (SEC fee) y opciones.
    let mut opciones_cerradas: BTreeMap<usize, (String, Money)> = BTreeMap::new();
    let mut opciones_vendidas: BTreeSet<usize> = BTreeSet::new();
    let mut opciones_abiertas: BTreeMap<usize, String> = BTreeMap::new();
    for m in &movs {
        match clasificar(&m.tipo, &m.categoria) {
            Clasificado::Cierre => {
                let venta = ventas.get(&m.posicion).and_then(|v| {
                    v.iter().copied().find(|i| {
                        let o = &e.ops[*i];
                        cerr.iter().any(|c| {
                            c.posicion == m.posicion
                                && c.volumen == o.cantidad
                                && c.valor_compra == Some(o.importe)
                                && c.valor_venta.is_some_and(|v| v != o.importe)
                        })
                    })
                });
                match venta {
                    Some(i) => {
                        let o = &mut e.ops[i];
                        o.importe += m.importe;
                        o.euros = o.importe;
                        o.nota = format!("{} · {}", o.nota, m.comentario);
                    }
                    None => avisos.push(format!(
                        "«{}» del {} ({}, {}): no se sabe a qué venta va; no se suma",
                        m.comentario,
                        local(m.t).0,
                        m.id,
                        m.importe
                    )),
                }
            }
            Clasificado::Comision if m.importe.is_zero() => {}
            Clasificado::Comision => {
                let fee = m.importe.abs();
                // A la venta (o compra) de la misma posición de este informe; si no está, sola.
                let destino = ventas.get(&m.posicion).and_then(|v| {
                    v.iter()
                        .copied()
                        .filter(|i| local(m.t).0 >= e.ops[*i].fecha)
                        .max_by_key(|i| e.ops[*i].cantidad)
                });
                match destino {
                    Some(i) if !m.posicion.is_empty() => {
                        let o = &mut e.ops[i];
                        o.comision += fee;
                        o.comision_eur = o.comision;
                        o.nota = format!("{} · {} {} ({})", o.nota, m.tipo, fee, huella(&m.id));
                        o.comision_huella.push(huella(&m.id));
                    }
                    _ => {
                        if !m.ticker.is_empty() {
                            valor(&m.ticker, &m.nombre, &m.categoria, clase(&m.categoria));
                        }
                        e.ops.push(OpLeida {
                            tipo: TipoOp::Comision,
                            importe: fee,
                            euros: fee,
                            nota: format!(
                                "{}{}",
                                m.comentario,
                                if m.posicion.is_empty() {
                                    String::new()
                                } else {
                                    format!(" · posición {}", m.posicion)
                                }
                            ),
                            ..base(m)
                        });
                    }
                }
            }
            Clasificado::Opcion if m.importe.is_zero() => {}
            Clasificado::Opcion => {
                let t = normal(&m.tipo);
                let codigo = OPCION
                    .captures(&m.comentario)
                    .map(|c| c[1].to_uppercase())
                    .unwrap_or_else(|| format!("{} OPCION", m.ticker));
                let corta = t.contains("sto") || t.contains("btc") || t.contains("sell to open");
                if corta {
                    avisos.push(format!(
                        "Opción lanzada (vendida) {codigo} del {}: prima {} {divisa}. No se \
                         importa: anota su resultado a mano.",
                        local(m.t).0,
                        m.importe
                    ));
                    continue;
                }
                let compra = t.contains("bto") || (!t.contains("stc") && m.importe.is_negative());
                let nombre_op = format!("{} (opción {codigo})", m.nombre);
                if compra {
                    let cerrada = cerr.iter().enumerate().find(|(i, c)| {
                        es_opcion(&c.categoria)
                            && c.compra
                            && c.ticker == m.ticker
                            && !opciones_cerradas.contains_key(i)
                            && cerca(c.apertura, m.t, 120)
                    });
                    let abierta = abie.iter().enumerate().find(|(i, a)| {
                        es_opcion(&a.categoria)
                            && a.ticker == m.ticker
                            && !opciones_abiertas.contains_key(i)
                            && a.apertura.is_some_and(|x| cerca(x, m.t, 120))
                    });
                    let q = match (cerrada, abierta) {
                        (Some((i, c)), _) => {
                            opciones_cerradas.insert(i, (codigo.clone(), m.importe.abs()));
                            c.volumen
                        }
                        (None, Some((i, a))) => {
                            opciones_abiertas.insert(i, codigo.clone());
                            a.volumen
                        }
                        _ => {
                            avisos.push(format!(
                                "Opción {codigo} del {}: no se encuentran sus contratos en las \
                                 posiciones; se toma 1.",
                                local(m.t).0
                            ));
                            Qty::from_units(1)
                        }
                    };
                    valor(&codigo, &nombre_op, "OPTION", Clase::Derivado);
                    e.ops.push(OpLeida {
                        tipo: TipoOp::Compra,
                        valor: Some(codigo),
                        cantidad: q,
                        precio: por_titulo(m.importe, q),
                        importe: m.importe.abs(),
                        euros: m.importe.abs(),
                        nota: format!("{} · prima de la opción", m.comentario),
                        ..base(m)
                    });
                } else {
                    let cerrada = cerr.iter().enumerate().find(|(i, c)| {
                        es_opcion(&c.categoria)
                            && c.compra
                            && c.ticker == m.ticker
                            && !opciones_vendidas.contains(i)
                            && cerca(c.cierre, m.t, 120)
                    });
                    let q = cerrada.map_or(Qty::from_units(1), |(_, c)| c.volumen);
                    if let Some((i, _)) = cerrada {
                        opciones_vendidas.insert(i);
                    }
                    valor(&codigo, &nombre_op, "OPTION", Clase::Derivado);
                    e.ops.push(OpLeida {
                        tipo: TipoOp::Venta,
                        valor: Some(codigo),
                        cantidad: q,
                        precio: por_titulo(m.importe, q),
                        importe: m.importe.abs(),
                        euros: m.importe.abs(),
                        nota: format!("{} · venta de la opción", m.comentario),
                        ..base(m)
                    });
                }
            }
            _ => {}
        }
    }

    // 3) Posiciones cerradas: cuadre de las ventas, compras de antes del periodo y
    // vencimiento de las opciones.
    let antes = |t: OffsetDateTime| desde_t.is_some_and(|d| t < d);
    let pos_huella = |c: &str, fecha: Date, q: Qty, p: Precio, lado: &str| {
        format!(
            "xtb:{cuenta}:pos:{c}:{fecha}:{}:{}:{lado}",
            q.to_text(),
            p.to_text()
        )
    };
    let mut cortas = Vec::new();
    for (i, c) in cerr.iter().enumerate() {
        if es_opcion(&c.categoria) {
            if !c.compra {
                cortas.push(c);
                continue;
            }
            if opciones_vendidas.contains(&i) || antes(c.cierre) {
                continue;
            }
            let (codigo, coste) = match opciones_cerradas.get(&i) {
                Some((k, coste)) => (k.clone(), Some(*coste)),
                None => (format!("{} OPCION {}", c.ticker, c.posicion), None),
            };
            let mult = Money::from_euros(MULTIPLICADOR_OPCION, 0);
            let por_contrato = |p: Precio| {
                c.volumen
                    .valor(p)
                    .map(|m| Money::from_cents(m.cents() * mult.cents() / 100))
                    .unwrap_or_default()
            };
            // Sin la prima en este informe (de antes del periodo): la compra, de la posición.
            let coste = match coste {
                Some(x) => x,
                None => {
                    let x = c
                        .valor_compra
                        .or_else(|| {
                            c.resultado
                                .filter(|_| c.precio_cierre.is_zero())
                                .map(|r| -r)
                        })
                        .unwrap_or_else(|| por_contrato(c.precio_apertura));
                    let (fecha, hora) = local(c.apertura);
                    valor(
                        &codigo,
                        &format!("{} (opción)", c.nombre),
                        "OPTION",
                        Clase::Derivado,
                    );
                    e.ops.push(OpLeida {
                        fecha,
                        hora,
                        tipo: TipoOp::Compra,
                        valor: Some(codigo.clone()),
                        cantidad: c.volumen,
                        precio: por_titulo(x, c.volumen),
                        divisa: divisa.clone(),
                        importe: x,
                        comision: Money::ZERO,
                        cambio: None,
                        euros: x,
                        comision_eur: Money::ZERO,
                        pais: String::new(),
                        nota: format!("Prima de la opción, de la posición {}", c.posicion),
                        fuente: "xtb-posiciones".into(),
                        huella: pos_huella(&c.posicion, fecha, c.volumen, c.precio_apertura, "c"),
                        comision_huella: Vec::new(),
                    });
                    x
                }
            };
            let cobrado = match c.resultado {
                Some(r) => (coste + r).max(Money::ZERO),
                None => por_contrato(c.precio_cierre),
            };
            let (fecha, hora) = local(c.cierre);
            e.ops.push(OpLeida {
                fecha,
                hora,
                tipo: TipoOp::Venta,
                valor: Some(codigo),
                cantidad: c.volumen,
                precio: Some(por_titulo(cobrado, c.volumen).unwrap_or(Precio::ZERO)),
                divisa: divisa.clone(),
                importe: cobrado,
                comision: Money::ZERO,
                cambio: None,
                euros: cobrado,
                comision_eur: Money::ZERO,
                pais: String::new(),
                nota: format!(
                    "{} de la opción (posición {}): resultado según XTB {}",
                    if cobrado.is_zero() {
                        "Vencimiento sin valor"
                    } else {
                        "Cierre"
                    },
                    c.posicion,
                    c.resultado.map(|r| r.to_string()).unwrap_or_default()
                ),
                fuente: "xtb".into(),
                huella: pos_huella(&c.posicion, fecha, c.volumen, c.precio_cierre, "v"),
                comision_huella: Vec::new(),
            });
            continue;
        }
        if !c.compra {
            cfd += 1;
            continue;
        }
        valor(&c.ticker, &c.nombre, &c.categoria, clase(&c.categoria));
        // La venta, en las operaciones en efectivo.
        if !antes(c.cierre) {
            let ok = ventas.get(&c.posicion).is_some_and(|v| {
                v.iter().any(|i| {
                    let o = &e.ops[*i];
                    o.cantidad == c.volumen
                        && o.precio == Some(c.precio_cierre)
                        && c.valor_venta.is_none_or(|x| x == o.importe)
                })
            });
            if !ok {
                avisos.push(format!(
                    "La posición cerrada {} de {} ({} títulos el {}) no cuadra con las \
                     operaciones en efectivo",
                    c.posicion,
                    c.ticker,
                    c.volumen,
                    local(c.cierre).0
                ));
            }
        }
        // La compra, de antes del periodo: de la posición.
        if antes(c.apertura) && !compras_pos.contains(&(c.posicion.clone(), c.volumen.to_text())) {
            let importe = c
                .valor_compra
                .or_else(|| c.volumen.valor(c.precio_apertura))
                .unwrap_or_default();
            e.ops.push(compra_de_posicion(
                &cuenta,
                &divisa,
                &c.ticker,
                &c.posicion,
                c.apertura,
                c.volumen,
                c.precio_apertura,
                importe,
                desde_t,
            ));
        }
    }
    for c in cortas {
        avisos.push(format!(
            "Opción lanzada (vendida) de {} cerrada el {}: resultado según XTB {} {divisa}. No \
             se importa: anota su resultado a mano.",
            c.ticker,
            local(c.cierre).0,
            c.resultado.map(|r| r.to_string()).unwrap_or_default()
        ));
    }

    // 4) Posiciones abiertas: títulos y precio de mercado a la fecha del informe; y la compra
    // de las abiertas antes del periodo.
    if let Some(generado) = generado_t {
        let fecha = local(generado).0;
        let mut con_precio: BTreeSet<String> = BTreeSet::new();
        for (i, a) in abie.iter().enumerate() {
            if es_opcion(&a.categoria) {
                if let Some(k) = opciones_abiertas.get(&i) {
                    e.posiciones.push(PosicionLeida {
                        fecha,
                        valor: k.clone(),
                        cantidad: a.volumen,
                    });
                }
                continue;
            }
            if !a.compra {
                cfd += 1;
                continue;
            }
            valor(&a.ticker, &a.nombre, &a.categoria, clase(&a.categoria));
            e.posiciones.push(PosicionLeida {
                fecha,
                valor: a.ticker.clone(),
                cantidad: a.volumen,
            });
            // El precio va en la divisa de la cuenta si `Value` es títulos × precio; si no, en
            // la de su mercado.
            let en_cuenta = |p: Precio| {
                a.valor.zip(a.volumen.valor(p)).is_some_and(|(v, b)| {
                    (v - b).abs() <= Money::from_cents(b.cents().abs() / 100 + 2)
                })
            };
            if let Some(p) = a.precio_actual.filter(|p| *p > Precio::ZERO)
                && con_precio.insert(a.ticker.clone())
            {
                e.precios.push(PrecioLeido {
                    fecha,
                    valor: a.ticker.clone(),
                    precio: p,
                    divisa: if en_cuenta(p) {
                        divisa.clone()
                    } else {
                        divisa_mercado(&a.ticker).unwrap_or(&divisa).to_owned()
                    },
                });
            }
            if let (Some(t), Some(p)) = (a.apertura, a.precio_apertura)
                && antes(t)
                && !compras_pos.contains(&(a.posicion.clone(), a.volumen.to_text()))
            {
                // En la divisa de la cuenta: con la relación entre `Value` y el precio actual.
                let bruto = a.volumen.valor(p).unwrap_or_default();
                let importe = match (a.valor, a.precio_actual.and_then(|x| a.volumen.valor(x))) {
                    (Some(v), Some(b))
                        if !b.is_zero() && !en_cuenta(a.precio_actual.unwrap_or(p)) =>
                    {
                        Money::from_cents(
                            i64::try_from(
                                i128::from(bruto.cents()) * i128::from(v.cents())
                                    / i128::from(b.cents()),
                            )
                            .unwrap_or(0),
                        )
                    }
                    _ => bruto,
                };
                e.ops.push(compra_de_posicion(
                    &cuenta,
                    &divisa,
                    &a.ticker,
                    &a.posicion,
                    t,
                    a.volumen,
                    p,
                    importe,
                    desde_t,
                ));
            }
        }
        // Con la hoja de posiciones abiertas, un valor sin posición tiene 0 títulos.
        if tablas.contains_key(&Hoja::Abiertas) {
            for (k, v) in &valores {
                if v.clase != Clase::Derivado && !e.posiciones.iter().any(|p| &p.valor == k) {
                    e.posiciones.push(PosicionLeida {
                        fecha,
                        valor: k.clone(),
                        cantidad: Qty::ZERO,
                    });
                }
            }
        }
    }

    e.desde = desde_t.map(|t| local(t).0);
    e.hasta = hasta_t.map(|t| local(t).0);
    e.efectivo = e.desde.zip(e.hasta);
    e.valores = valores.into_values().collect();
    if divisa != "EUR" {
        for o in &mut e.ops {
            o.euros = Money::ZERO;
            o.comision_eur = Money::ZERO;
        }
        avisos.push(format!(
            "La cuenta de XTB va en {divisa}: los importes se pasan a euros con el tipo de \
             referencia del BCE del día de cada operación (si falta, «Pasar a euros con el BCE» \
             en Inversiones en valores)."
        ));
    }
    if cfd > 0 {
        avisos.push(format!(
            "{cfd} filas de CFD o posiciones cortas no se importan (son derivados, no valores)"
        ));
    }
    for (t, n) in desconocidos {
        avisos.push(format!(
            "{n} operaciones en efectivo de tipo «{t}» no se reconocen y no se importan"
        ));
    }
    e.avisos = avisos;
    if e.ops.is_empty() && e.precios.is_empty() {
        return Err("el informe de XTB no trae operaciones ni posiciones que importar".into());
    }
    Ok(e)
}

/// Compra de una posición abierta antes del periodo del informe (no está en «Cash
/// Operations»): con la huella de la posición, igual en cualquier informe que la traiga.
#[allow(clippy::too_many_arguments)]
fn compra_de_posicion(
    cuenta: &str,
    divisa: &str,
    ticker: &str,
    posicion: &str,
    apertura: OffsetDateTime,
    q: Qty,
    p: Precio,
    importe: Money,
    desde: Option<OffsetDateTime>,
) -> OpLeida {
    let (fecha, hora) = local(apertura);
    OpLeida {
        fecha,
        hora,
        tipo: TipoOp::Compra,
        valor: Some(ticker.to_owned()),
        cantidad: q,
        precio: Some(p),
        divisa: divisa.to_owned(),
        importe,
        comision: Money::ZERO,
        cambio: None,
        euros: importe,
        comision_eur: Money::ZERO,
        pais: String::new(),
        nota: format!(
            "Compra de la posición {posicion}, de las posiciones del informe (es de antes del {})",
            desde.map(|d| local(d).0.to_string()).unwrap_or_default()
        ),
        fuente: "xtb-posiciones".into(),
        huella: format!(
            "xtb:{cuenta}:pos:{posicion}:{fecha}:{}:{}:c",
            q.to_text(),
            p.to_text()
        ),
        comision_huella: Vec::new(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use rust_xlsxwriter::{ExcelDateTime, Format, Workbook};
    use time::macros::date;

    /// Una celda del libro de prueba.
    #[derive(Clone, Copy, PartialEq)]
    pub(crate) enum C {
        S(&'static str),
        N(f64),
        /// Fecha y hora UTC `aaaa-mm-dd hh:mm:ss`, como fecha de Excel.
        T(&'static str),
        E,
    }

    fn fila(ws: &mut rust_xlsxwriter::Worksheet, r: u32, cs: &[C]) {
        let f = Format::new().set_num_format("yyyy-mm-dd hh:mm:ss");
        for (i, c) in cs.iter().enumerate() {
            let col = u16::try_from(i).unwrap();
            match c {
                C::S(s) => {
                    ws.write_string(r, col, *s).unwrap();
                }
                C::N(n) => {
                    ws.write_number(r, col, *n).unwrap();
                }
                C::T(t) => {
                    let d = ExcelDateTime::parse_from_str(t).unwrap();
                    ws.write_datetime_with_format(r, col, &d, &f).unwrap();
                }
                C::E => {}
            }
        }
    }

    /// Libro con las tres hojas del informe de xStation (cabecera de metadatos y tabla).
    pub(crate) fn libro(
        cuenta: &'static str,
        desde: &'static str,
        hasta: &'static str,
        cerradas: &[Vec<C>],
        efectivo: &[Vec<C>],
        abiertas: &[Vec<C>],
    ) -> Vec<u8> {
        use C::{E, N, S, T};
        let mut wb = Workbook::new();
        let cab = |ws: &mut rust_xlsxwriter::Worksheet, titulo: &'static str| {
            fila(ws, 0, &[S("Account number"), S(cuenta)]);
            fila(ws, 1, &[S(titulo), S("")]);
            fila(ws, 2, &[S("Date from (UTC)"), T(desde)]);
            fila(ws, 3, &[S("Date to (UTC)"), T(hasta)]);
        };
        let ws = wb.add_worksheet().set_name("Closed Positions").unwrap();
        cab(ws, "Closed Positions");
        let t: Vec<C> = [
            "Instrument",
            "Ticker",
            "Category",
            "Type",
            "Volume",
            "Open Price",
            "Open Time (UTC)",
            "Close Price",
            "Close Time (UTC)",
            "Product",
            "Profit/Loss",
            "Gross Profit",
            "Purchase Value",
            "Sale Value",
            "Stop Loss",
            "Take Profit",
            "Commission",
            "Margin",
            "Swap",
            "Rollover",
            "Open Conversion Rate",
            "Close Conversion Rate",
            "Close Origin",
            "Position ID",
            "Comment",
        ]
        .into_iter()
        .map(S)
        .collect();
        fila(ws, 4, &t);
        let mut r = 5;
        for c in cerradas {
            fila(ws, r, c);
            r += 1;
        }
        fila(
            ws,
            r,
            &[S("Profit/loss"), E, E, E, E, E, E, E, E, E, N(0.0)],
        );
        let ws = wb.add_worksheet().set_name("Cash Operations").unwrap();
        cab(ws, "Cash Operations");
        let t: Vec<C> = [
            "Type",
            "Instrument",
            "Ticker",
            "Category",
            "Time",
            "Amount",
            "ID",
            "Comment",
            "Product",
            "Position ID",
        ]
        .into_iter()
        .map(S)
        .collect();
        fila(ws, 4, &t);
        let mut r = 5;
        for c in efectivo {
            fila(ws, r, c);
            r += 1;
        }
        fila(ws, r, &[S("Total"), E, E, E, E, N(0.0)]);
        let ws = wb.add_worksheet().set_name("Open Positions").unwrap();
        fila(ws, 0, &[S("Account number"), S(cuenta)]);
        fila(ws, 1, &[S("Open Positions"), S("")]);
        fila(ws, 2, &[S("Data as of report generated"), T(hasta)]);
        fila(
            ws,
            3,
            &[S("Product"), S("Metric"), S("Amount"), S("Currency")],
        );
        fila(
            ws,
            4,
            &[S("My Trades"), S("Open position value"), N(0.0), S("USD")],
        );
        fila(
            ws,
            6,
            &[
                S("Note"),
                S("Summary values and open positions are shown as of the report generation time"),
            ],
        );
        let t: Vec<C> = [
            "Product",
            "Instrument/Position",
            "Ticker",
            "Category",
            "Type",
            "Volume",
            "Value",
            "Current price",
            "Open price",
            "Open time (UTC)",
            "Stop Loss",
            "Take Profit",
            "Net Profit %",
            "Net Profit",
            "Gross Profit",
            "Margin",
            "Open Commission",
            "Swap",
            "Rollover",
        ]
        .into_iter()
        .map(S)
        .collect();
        fila(ws, 7, &t);
        for (r, c) in (8..).zip(abiertas) {
            fila(ws, r, c);
        }
        wb.save_to_buffer().unwrap()
    }

    /// Cuenta en dólares (datos inventados): una compra y su venta con la SEC fee del día
    /// siguiente, una venta de algo comprado antes del periodo (sólo en «Closed Positions»), una
    /// opción comprada que vence sin valor, intereses con su retención, un ingreso y una
    /// posición abierta.
    pub(crate) fn usd() -> Vec<u8> {
        usd_con(true)
    }

    /// La misma cuenta, con o sin la fila de la SEC fee (la del día siguiente a la venta).
    pub(crate) fn usd_con(sec_fee: bool) -> Vec<u8> {
        use C::{E, N, S, T};
        let my = S("My Trades");
        let filtro = |f: &Vec<C>| sec_fee || !f.contains(&S("1003"));
        libro(
            "99000001",
            "2025-12-31 23:00:00",
            "2026-10-31 12:00:00",
            &[
                vec![
                    S("Acme"),
                    S("ACME.US"),
                    S("STOCK"),
                    S("BUY"),
                    N(10.0),
                    N(100.0),
                    T("2026-03-02 15:00:00"),
                    N(120.0),
                    T("2026-04-01 14:00:00"),
                    my,
                    N(200.0),
                    N(200.0),
                    N(1000.0),
                    N(1200.0),
                    S(""),
                    S(""),
                    N(0.0),
                    S(""),
                    S(""),
                    S(""),
                    S(""),
                    S(""),
                    S("xStation5"),
                    N(5001.0),
                    S(""),
                ],
                vec![
                    S("Oldco"),
                    S("OLDCO.US"),
                    S("STOCK"),
                    S("BUY"),
                    N(5.0),
                    N(90.0),
                    T("2025-10-01 14:00:00"),
                    N(110.0),
                    T("2026-01-20 15:00:00"),
                    my,
                    N(100.0),
                    N(100.0),
                    N(450.0),
                    N(550.0),
                    S(""),
                    S(""),
                    N(0.0),
                    S(""),
                    S(""),
                    S(""),
                    S(""),
                    S(""),
                    S("xStation5"),
                    N(4001.0),
                    S(""),
                ],
                vec![
                    S("Demo ETF"),
                    S("DEMO.US"),
                    S("OPTION"),
                    S("BUY"),
                    N(2.0),
                    N(0.3),
                    T("2026-09-01 13:59:58"),
                    N(0.0),
                    T("2026-09-18 20:00:00"),
                    my,
                    N(-60.0),
                    N(-60.0),
                    S(""),
                    S(""),
                    S(""),
                    S(""),
                    N(0.0),
                    S(""),
                    S(""),
                    S(""),
                    N(0.0),
                    N(0.0),
                    S("Manager"),
                    N(7001.0),
                    S("Exercise"),
                ],
            ],
            &[
                vec![
                    S("SEC fee"),
                    S("Acme"),
                    S("ACME.US"),
                    S("STOCK"),
                    T("2026-04-02 13:00:00"),
                    N(-0.05),
                    S("1003"),
                    S("Sec Fee adj ACME.US 20260401"),
                    my,
                    S("5001"),
                ],
                vec![
                    S("Stock sell"),
                    S("Acme"),
                    S("ACME.US"),
                    S("STOCK"),
                    T("2026-04-01 14:00:00"),
                    N(1200.0),
                    S("1002"),
                    S("CLOSE BUY ACME.US 10 @ 120.00"),
                    my,
                    S("5001"),
                ],
                vec![
                    S("Stock purchase"),
                    S("Acme"),
                    S("ACME.US"),
                    S("STOCK"),
                    T("2026-03-02 15:00:00"),
                    N(-1000.0),
                    S("1001"),
                    S("OPEN BUY ACME.US 10 @ 100.00"),
                    my,
                    S("5001"),
                ],
                vec![
                    S("Stock sell"),
                    S("Oldco"),
                    S("OLDCO.US"),
                    S("STOCK"),
                    T("2026-01-20 15:00:00"),
                    N(550.0),
                    S("1004"),
                    S("CLOSE BUY 5 @ 110.00"),
                    my,
                    S("4001"),
                ],
                vec![
                    S("Option BTO raw premium"),
                    S("Demo ETF"),
                    S("DEMO.US"),
                    S("OPTION"),
                    T("2026-09-01 14:00:00"),
                    N(-60.0),
                    S("1005"),
                    S("option_instrument: DEMO.US_260918_P_40"),
                    my,
                    S(""),
                ],
                vec![
                    S("Free funds interest"),
                    S(""),
                    S(""),
                    S(""),
                    T("2026-02-03 10:00:00"),
                    N(10.0),
                    S("1006"),
                    S("Free-funds Interest 2026-01"),
                    my,
                    S(""),
                ],
                vec![
                    S("Free funds interest tax"),
                    S(""),
                    S(""),
                    S(""),
                    T("2026-02-03 09:59:00"),
                    N(-1.9),
                    S("1007"),
                    S("Free-funds Interest Tax 2026-01"),
                    my,
                    S(""),
                ],
                vec![
                    S("Free funds interest tax"),
                    S(""),
                    S(""),
                    S(""),
                    T("2026-03-03 09:59:00"),
                    N(0.0),
                    S("1010"),
                    S("Free-funds Interest Tax 2026-02"),
                    my,
                    S(""),
                ],
                vec![
                    S("Stock purchase"),
                    S("Beta Corp"),
                    S("BETA.US"),
                    S("STOCK"),
                    T("2026-10-01 14:00:00"),
                    N(-500.0),
                    S("1008"),
                    S("OPEN BUY BETA.US 5 @ 100.00"),
                    my,
                    S("6001"),
                ],
                vec![
                    S("deposit"),
                    S(""),
                    S(""),
                    S(""),
                    T("2026-01-05 10:00:00"),
                    N(2000.0),
                    S("1009"),
                    S("BANK deposit"),
                    my,
                    S(""),
                ],
            ]
            .into_iter()
            .filter(filtro)
            .collect::<Vec<_>>(),
            &[
                vec![
                    my,
                    S("Beta Corp"),
                    S("BETA.US"),
                    S("STOCK"),
                    S(""),
                    N(5.0),
                    N(480.0),
                    S(""),
                    N(100.0),
                    S(""),
                ],
                vec![
                    my,
                    S("6001"),
                    S("BETA.US"),
                    S(""),
                    S("BUY"),
                    N(5.0),
                    N(480.0),
                    N(96.0),
                    N(100.0),
                    T("2026-10-01 14:00:00"),
                ],
                vec![E],
            ],
        )
    }

    #[test]
    fn lee_la_cuenta_en_dolares() {
        let bytes = usd();
        assert!(es_informe(&bytes));
        assert!(!es_informe(b"PK no es un libro"));
        let e = leer("USD_99000001_2026-01-01_2026-10-31.xlsx", &bytes).unwrap();
        assert_eq!((e.cuenta.as_str(), e.divisa.as_str()), ("99000001", "USD"));
        assert_eq!(
            e.efectivo,
            Some((date!(2026 - 01 - 01), date!(2026 - 10 - 31)))
        );
        let n = |t: TipoOp| e.ops.iter().filter(|o| o.tipo == t).count();
        // ACME, BETA, OLDCO (de la posición) y la prima de la opción.
        assert_eq!(n(TipoOp::Compra), 4);
        // ACME, OLDCO y el vencimiento de la opción.
        assert_eq!(n(TipoOp::Venta), 3);
        assert_eq!(n(TipoOp::Interes), 1);
        assert_eq!(n(TipoOp::Retencion), 1);
        assert_eq!(n(TipoOp::Deposito), 1);
        // La SEC fee va en la venta de su posición, con su huella en la nota.
        assert_eq!(n(TipoOp::Comision), 0);
        let v = e
            .ops
            .iter()
            .find(|o| o.tipo == TipoOp::Venta && o.valor.as_deref() == Some("ACME.US"))
            .unwrap();
        assert_eq!(v.importe, Money::from_euros(1_200, 0));
        assert_eq!(v.comision, Money::from_cents(5));
        assert_eq!(v.comision_huella, vec!["xtb:99000001:1003".to_owned()]);
        assert_eq!(v.huella, "xtb:99000001:1002");
        // Hora peninsular (verano: UTC+2).
        assert_eq!(
            (v.fecha, v.hora.as_str()),
            (date!(2026 - 04 - 01), "16:00:00")
        );
        assert!(v.euros.is_zero(), "cuenta en dólares: sin euros inventados");
        // La compra de antes del periodo, de la posición, con su huella estable.
        let old = e
            .ops
            .iter()
            .find(|o| o.tipo == TipoOp::Compra && o.valor.as_deref() == Some("OLDCO.US"))
            .unwrap();
        assert_eq!(old.fuente, "xtb-posiciones");
        assert_eq!(old.fecha, date!(2025 - 10 - 01));
        assert_eq!(old.importe, Money::from_euros(450, 0));
        assert_eq!(old.huella, "xtb:99000001:pos:4001:2025-10-01:5:90:c");
        // La opción: prima de 60 por 2 contratos y su vencimiento a cero.
        let op = e
            .ops
            .iter()
            .filter(|o| o.valor.as_deref() == Some("DEMO.US_260918_P_40"))
            .collect::<Vec<_>>();
        assert_eq!(op.len(), 2);
        assert_eq!(op[0].cantidad, Qty::from_units(2));
        assert_eq!(op[0].importe, Money::from_euros(60, 0));
        assert_eq!(op[1].tipo, TipoOp::Venta);
        assert!(op[1].importe.is_zero());
        let x = e
            .valores
            .iter()
            .find(|v| v.clave == "DEMO.US_260918_P_40")
            .unwrap();
        assert_eq!(x.clase, Clase::Derivado);
        // El subyacente de la opción no es un valor de la cartera.
        assert!(!e.valores.iter().any(|v| v.clave == "DEMO.US"));
        // Retención de los intereses (española) y la fila a cero fuera.
        let r = e.ops.iter().find(|o| o.tipo == TipoOp::Retencion).unwrap();
        assert_eq!((r.importe, r.pais.as_str()), (Money::from_cents(190), "ES"));
        // Posición abierta y su precio en dólares a la fecha del informe.
        assert_eq!(e.precios.len(), 1);
        assert_eq!(e.precios[0].precio, Precio::parse("96").unwrap());
        assert_eq!(e.precios[0].divisa, "USD");
        assert_eq!(e.precios[0].fecha, date!(2026 - 10 - 31));
        assert!(
            e.posiciones
                .iter()
                .any(|p| p.valor == "BETA.US" && p.cantidad == Qty::from_units(5))
        );
        assert!(
            e.posiciones
                .iter()
                .any(|p| p.valor == "ACME.US" && p.cantidad.is_zero())
        );
        assert!(
            e.avisos.iter().any(|a| a.contains("va en USD")),
            "{:?}",
            e.avisos
        );
        assert!(
            !e.avisos.iter().any(|a| a.contains("no cuadra")),
            "{:?}",
            e.avisos
        );
    }

    #[test]
    fn opcion_lanzada_y_cabeceras_en_espanol() {
        use C::{N, S, T};
        // Cabeceras supuestas en español: se reconocen igual.
        let mut wb = Workbook::new();
        let ws = wb.add_worksheet().set_name("Operaciones de caja").unwrap();
        fila(ws, 0, &[S("Número de cuenta"), S("99000003")]);
        fila(ws, 1, &[S("Fecha desde (UTC)"), T("2025-12-31 23:00:00")]);
        fila(
            ws,
            2,
            &[
                S("Tipo"),
                S("Instrumento"),
                S("Ticker"),
                S("Categoría"),
                S("Hora"),
                S("Importe"),
                S("ID"),
                S("Comentario"),
            ],
        );
        fila(
            ws,
            3,
            &[
                S("Free funds interest"),
                S(""),
                S(""),
                S(""),
                T("2026-02-03 10:00:00"),
                N(3.5),
                S("2001"),
                S("Free-funds Interest 2026-01"),
            ],
        );
        fila(
            ws,
            4,
            &[
                S("Option STO raw premium"),
                S("Demo"),
                S("DEMO.US"),
                S("OPTION"),
                T("2026-02-04 15:00:00"),
                N(40.0),
                S("2002"),
                S("option_instrument: DEMO.US_260220_C_50"),
            ],
        );
        let bytes = wb.save_to_buffer().unwrap();
        assert!(es_informe(&bytes));
        let e = leer("EUR_99000003_2026-01-01_2026-10-31.xlsx", &bytes).unwrap();
        assert_eq!(e.divisa, "EUR");
        assert_eq!(e.ops.len(), 1);
        assert_eq!(e.ops[0].euros, Money::from_cents(350));
        assert!(
            e.avisos
                .iter()
                .any(|a| a.contains("Opción lanzada") && a.contains("40,00")),
            "{:?}",
            e.avisos
        );
    }

    #[test]
    fn instantes_y_divisa_por_el_nombre() {
        assert_eq!(
            divisa_por_nombre("/x/USD_52_2025-12-31.xlsx").as_deref(),
            Some("USD")
        );
        assert_eq!(divisa_por_nombre("informe.xlsx"), None);
        let t = instante(&Data::String("2026-01-07 13:27:07.445".into())).unwrap();
        assert_eq!(local(t), (date!(2026 - 01 - 07), "14:27:07".into()));
        let t = instante(&Data::String("31/12/2025 23:00:00".into())).unwrap();
        assert_eq!(local(t).0, date!(2026 - 01 - 01));
        let t = instante(&Data::Float(45_658.5)).unwrap();
        assert_eq!(local(t), (date!(2025 - 01 - 01), "13:00:00".into()));
        assert_eq!(num(1234.5), "1234.5");
        assert_eq!(num(5_000_000_009.0), "5000000009");
    }
}
