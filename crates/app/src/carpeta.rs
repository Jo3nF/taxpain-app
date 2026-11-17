//! «Importar la carpeta de la empresa»: se elige la carpeta donde están las facturas, los libros
//! y los Excel de la gestoría, TaxPain la recorre, dice qué es cada fichero y, con la conformidad
//! del usuario, lo pasa al importador que toca:
//!
//! | Fichero | Qué se hace |
//! |---|---|
//! | Diario (SUENLACE de a3, Excel o CSV de asientos) | [`crate::import`]: asientos en un lote que se puede deshacer |
//! | Libro registro de facturas emitidas o recibidas | [`crate::libros`]: libros de facturas sin asiento, en un lote que se puede deshacer |
//! | Extracto del banco (Norma 43, camt.053) | [`crate::bank`] |
//! | Factura recibida (PDF, Facturae) | si ya está en el libro, se le adjunta; si no, queda para revisar |
//! | Modelo presentado (fichero de la AEAT o PDF) | [`crate::presentados`]: para compararlo |
//! | Criptomonedas (CoinTracking, ledgers de Kraken, hoja propia, saldos por fecha), sueltas o en un zip | [`crate::cripto::paquete`]: operaciones y precios de cierre |
//! | Bróker de valores (informe de cuenta de XTB en PDF o Excel, Activity Flex de Interactive Brokers en XML o CSV) | [`crate::valores::importar`]: operaciones, precios y cambios, sin duplicar |
//!
//! Nada se borra ni se presenta. Leer la carpeta no cambia nada; aplicar es otro paso.
//!
//! Una factura recibida de un periodo que ya declaró el gestor (hay un modelo suyo o su libro
//! de ese trimestre) y que no está en su libro no se propone importar: su IVA casi seguro ya está
//! deducido y, revisada ahora, se deduciría otra vez en el trimestre en que se revise.
//!
//! Leer una carpeta grande lleva tiempo (PDF, reconocimiento de texto): lo lento se hace sin el
//! almacén y éste sólo se coge, fichero a fichero, para lo que lo necesita ([`scan_with`],
//! [`apply_with`]); mientras tanto la app sigue respondiendo.

use std::ops::Deref;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::libros::{BookKind, ReceivedRef};

/// Como mucho se leen tantos ficheros: una tienda en línea con años de tickets pasa de los
/// 10.000 (y la revisión avisa si se pasa: `revision::ficheros::incompleta`).
const MAX_FILES: usize = 50_000;
const MAX_SIZE: u64 = 30 * 1024 * 1024;
/// Extensiones que se miran: las de los documentos, hojas de cálculo y extractos bancarios
/// (Norma 43, OFX, QIF, MT940, tablas HTML), los zip (de ellos sólo se leen los historiales de
/// criptomonedas) y las de las fotos que acepta Recibidas ([`crate::received::IMAGE_EXTENSIONS`]).
const EXTENSIONS: [&str; 25] = [
    "pdf", "xml", "xsig", "xlsx", "xlsm", "xls", "ods", "csv", "tsv", "txt", "dat", "n43", "aeb",
    "q43", "c43", "ofx", "qfx", "qif", "sta", "940", "mt940", "mt942", "htm", "html", "zip",
];

fn is_image_ext(ext: &str) -> bool {
    crate::received::IMAGE_EXTENSIONS.contains(&ext)
}

/// Extensión (en minúsculas) de un fichero que se mira al leer la carpeta.
pub(crate) fn admitted_ext(ext: &str) -> bool {
    EXTENSIONS.contains(&ext) || is_image_ext(ext)
}

/// Qué es un fichero de la carpeta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Found {
    /// Leído con el asistente («Full IA»): lo que dijo que es y un resumen.
    Ia {
        tipo: String,
        titulo: String,
        descripcion: String,
        detalle: String,
        importable: bool,
        /// Se puede marcar aunque no se proponga (un diario de fechas que ya tienen asientos en
        /// TaxPain): importarlo hace algo, con aviso.
        selectable: bool,
        year: Option<i32>,
    },
    /// Documento de la empresa (escritura, tarjeta NIF, alta en el ROI, 036, certificado…).
    Documento {
        doc_kind: crate::documentos::DocKind,
        title: String,
        facts: Box<crate::documentos::Facts>,
        /// Reconocido sólo por el nombre (foto o escaneado, sin texto).
        by_name: bool,
    },
    /// Asientos (SUENLACE, Excel o CSV).
    Diario {
        entries: usize,
        first: Option<Date>,
        last: Option<Date>,
        already: bool,
        /// Asientos propios de TaxPain (no importados) en esas fechas: importarlo los contaría
        /// dos veces, así que no se propone.
        overlap: usize,
        /// Asientos de otros lotes importados en esas fechas (el diario del gestor reenviado
        /// con una corrección): tampoco se propone; hay que deshacer antes el lote anterior.
        imported: usize,
    },
    /// Libro registro de facturas.
    Libro {
        book: BookKind,
        /// Líneas del libro (también las de autorrepercusión).
        invoices: usize,
        first: Option<Date>,
        last: Option<Date>,
        /// Base e IVA de lo que entra: sin las líneas de autorrepercusión (`self_assessed`).
        base: Money,
        vat: Money,
        /// Facturas propias de TaxPain (expedidas en la app o revisadas a mano) en esas fechas:
        /// las del libro con el mismo número no se duplican, pero las demás sí, así que no se
        /// propone.
        overlap: usize,
        /// En un libro de emitidas, las líneas que son la autorrepercusión de compras y no
        /// ventas ([`crate::libros::BookInvoice::self_assessed`]): no entran al importarlo y, si lo
        /// son todas, no se propone.
        self_assessed: usize,
        /// Lo que dice la lectura con IA del fichero cuando no es un libro de emitidas (las reglas
        /// lo leen como tal): no se propone.
        #[serde(skip_serializing_if = "Option::is_none")]
        reading: Option<String>,
    },
    /// Extracto bancario de cualquier banco y formato (Norma 43, camt, OFX, MT940, hojas de
    /// cálculo, CSV, PDF…).
    Banco {
        /// Lo leído: banco, cuenta, divisa, periodo y movimientos de cada extracto.
        resumen: String,
        /// Fecha del último movimiento.
        last: Option<Date>,
        /// ¿Cuadra con sus saldos? `None` si no trae saldos con los que comprobarlo (los
        /// listados de tarjeta, por ejemplo).
        cuadra: Option<bool>,
    },
    FacturaRecibida {
        supplier: Option<String>,
        /// NIF del proveedor (si se lee).
        nif: Option<String>,
        number: Option<String>,
        date: Option<Date>,
        total: Option<Money>,
        /// Ya está en el libro de recibidas (o en un libro de la carpeta): se le adjunta.
        in_book: bool,
        /// Es de un periodo que ya declaró el gestor y no está en su libro: no se propone.
        historical: bool,
    },
    /// Factura emitida por la empresa (las de antes de TaxPain entran por su libro).
    FacturaEmitida {
        number: Option<String>,
        date: Option<Date>,
        total: Option<Money>,
    },
    Modelo {
        model: String,
        year: i32,
        period: String,
        /// `fichero`, `pdf`, `registros` (347 o 349 en fichero) o `justificante` (el PDF del
        /// justificante de presentación de la AEAT).
        source: String,
        boxes: usize,
        /// Se han leído sus casillas. Un justificante sin casillas leídas cuenta como presentado
        /// en la revisión, pero no se guarda para comparar.
        boxes_read: bool,
    },
    /// Operaciones o precios de criptomonedas ([`crate::cripto::paquete`]): una hoja o un CSV
    /// que entiende el importador de Criptomonedas, o un zip con historiales de exchanges.
    Cripto {
        /// Lo leído, fichero a fichero.
        resumen: String,
        ops: usize,
        /// Precios de cierre (de una hoja de saldos por fecha).
        prices: usize,
        /// Operaciones que ya estaban importadas.
        duplicates: usize,
        first: Option<Date>,
        last: Option<Date>,
        /// Ficheros de dentro del zip que no se han podido leer.
        unread: usize,
        /// Sin punto de partida en Criptomonedas: ventas o traspasos de más unidades de las que
        /// había (faltan operaciones).
        huecos: usize,
    },
    /// Extracto de un bróker de valores ([`crate::valores`]): el informe de cuenta de XTB en
    /// PDF o Excel, o el Activity Flex de Interactive Brokers (XML o CSV).
    Valores {
        /// «XTB (PDF)», «XTB (Excel)», «Interactive Brokers (Flex XML)»…
        formato: String,
        /// Número de cuenta en el bróker.
        cuenta: String,
        resumen: String,
        ops: usize,
        prices: usize,
        /// Operaciones que ya estaban importadas.
        duplicates: usize,
        first: Option<Date>,
        last: Option<Date>,
    },
    /// No se sabe qué es (o no se puede leer).
    Otro { reason: String },
}

impl Found {
    /// Orden al aplicar: primero lo que da contexto (documentos, modelos, libros, diario) y luego
    /// el resto. Lo leído con IA va con lo de su tipo: una factura suelta, después de los libros,
    /// para que la que ya está en el libro se le adjunte en vez de quedar pendiente en Recibidas
    /// (y deducirse dos veces al revisarla). El libro de recibidas va antes que el de emitidas: así
    /// se ven en él las compras que el de emitidas repite como autorrepercusión.
    fn order(&self) -> u8 {
        match self {
            Found::Documento { .. } | Found::Modelo { .. } => 0,
            Found::Libro {
                book: BookKind::Recibidas,
                ..
            } => 1,
            Found::Libro {
                book: BookKind::Emitidas,
                ..
            } => 2,
            Found::Diario { .. } => 3,
            Found::Banco { .. } | Found::Cripto { .. } | Found::Valores { .. } => 4,
            Found::FacturaRecibida { .. } => 5,
            Found::FacturaEmitida { .. } => 6,
            Found::Otro { .. } => 7,
            Found::Ia { tipo, .. } => match tipo.as_str() {
                "libro_recibidas" => 1,
                "libro_emitidas" => 2,
                "diario" => 3,
                "extracto_bancario" => 4,
                "factura_recibida" => 5,
                "factura_emitida" => 6,
                "modelo_aeat" => 0,
                t if crate::lectura_ia::doc_kind(t).is_some() => 0,
                _ => 7,
            },
        }
    }

    /// Año al que se refiere (para filtrar por ejercicio).
    fn year(&self) -> Option<i32> {
        match self {
            Found::Diario { last, .. }
            | Found::Libro { last, .. }
            | Found::Banco { last, .. }
            | Found::Cripto { last, .. }
            | Found::Valores { last, .. } => last.map(|d| d.year()),
            Found::FacturaRecibida { date, .. } | Found::FacturaEmitida { date, .. } => {
                date.map(|d| d.year())
            }
            Found::Modelo { year, .. } => Some(*year),
            Found::Ia { year, .. } => *year,
            Found::Documento { .. } | Found::Otro { .. } => None,
        }
    }

    /// ¿Se importa algo al aplicar? (las emitidas y lo desconocido sólo se enseñan).
    pub(crate) fn importable(&self) -> bool {
        match self {
            Found::Diario {
                already,
                overlap,
                imported,
                ..
            } => !already && *overlap == 0 && *imported == 0,
            Found::FacturaRecibida { historical, .. } => !historical,
            Found::Libro {
                overlap,
                invoices,
                self_assessed,
                reading,
                ..
            } => *overlap == 0 && self_assessed < invoices && reading.is_none(),
            Found::Documento { .. } => true,
            // Un 349 en fichero sólo se usa en la revisión de la carpeta; el 347, el 190, el 180
            // y el 193 se guardan para compararlos con lo que calcula TaxPain, y de un
            // justificante sólo se guarda lo que se puede comparar (Importar → Comparar).
            Found::Modelo {
                source,
                model,
                boxes_read,
                ..
            } => match source.as_str() {
                "registros" => crate::presentados::INFORMATIVAS.contains(&model.as_str()),
                "justificante" => *boxes_read && crate::presentados::storable(model),
                _ => true,
            },
            // Un extracto que no cuadra con sus saldos no se propone (se puede marcar).
            Found::Banco { cuadra, .. } => *cuadra != Some(false),
            // Lo que ya estaba importado (las mismas operaciones) no se propone otra vez.
            Found::Cripto {
                ops,
                prices,
                duplicates,
                ..
            } => ops > duplicates || *prices > 0,
            Found::Valores {
                ops,
                prices,
                duplicates,
                ..
            } => ops > duplicates || *prices > 0,
            Found::Ia { importable, .. } => *importable,
            Found::FacturaEmitida { .. } | Found::Otro { .. } => false,
        }
    }

    /// ¿Se puede importar si el usuario lo marca? Lo que se propone y, además, lo que no se
    /// propone por precaución pero importado hace algo (con aviso).
    pub(crate) fn selectable(&self) -> bool {
        self.importable()
            || matches!(
                self,
                Found::FacturaRecibida { .. }
                    | Found::Diario { already: false, .. }
                    | Found::Libro { .. }
                    | Found::Banco { .. }
                    | Found::Cripto { .. }
                    | Found::Valores { .. }
            )
            || matches!(
                self,
                Found::Ia {
                    selectable: true,
                    ..
                }
            )
            || matches!(self, Found::Ia { tipo, .. }
                if tipo == "factura_recibida" || tipo == "factura_emitida")
    }
}

/// Un fichero de la carpeta y lo que se ha visto en él.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Item {
    /// Ruta relativa a la carpeta.
    pub path: String,
    pub size: u64,
    pub year: Option<i32>,
    pub found: Found,
    /// Se propone importarlo (marcado de entrada).
    pub importable: bool,
    /// Se puede importar si el usuario lo marca, aunque no se proponga (una factura de un
    /// periodo ya declarado, un diario de fechas con asientos propios…): importarlo hace algo,
    /// con aviso.
    pub selectable: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Inventory {
    pub root: String,
    /// Lecturas de Claude Code cargadas de `taxpain-lecturas.json` (si lo hay).
    pub claude_code: Option<crate::lectura_ia::PackageLoad>,
    pub items: Vec<Item>,
    /// Ficheros que no se miran (otro tipo, ocultos, enlaces o demasiado grandes).
    pub skipped: usize,
    /// Ficheros admitidos que se han dejado fuera por pasar de [`MAX_FILES`]: la carpeta está
    /// incompleta y hay que decirlo (dividirla por años).
    pub over_limit: usize,
    /// Subcarpetas a más de [`MAX_DEPTH`] niveles, que no se recorren.
    pub too_deep: usize,
}

/// Niveles de subcarpetas que se recorren.
const MAX_DEPTH: u8 = 8;

/// Lo que se deja fuera al recorrer la carpeta, por qué.
#[derive(Default)]
struct Skipped {
    /// Otro tipo, ocultos, enlaces, vacíos o demasiado grandes.
    other: usize,
    /// Admitidos pero por encima de [`MAX_FILES`].
    over_limit: usize,
    /// Subcarpetas demasiado profundas.
    too_deep: usize,
}

/// Ficheros de la carpeta. Los enlaces simbólicos no se siguen: podrían traer ficheros de fuera.
fn walk(dir: &Path, out: &mut Vec<PathBuf>, skipped: &mut Skipped, depth: u8) {
    walk_max(dir, out, skipped, depth, MAX_FILES);
}

/// Como [`walk`], con el tope de ficheros `max`.
fn walk_max(dir: &Path, out: &mut Vec<PathBuf>, skipped: &mut Skipped, depth: u8, max: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.')
            || name.starts_with("~$")
            || name == crate::lectura_ia::PACKAGE_FILE
        {
            continue;
        }
        let path = e.path();
        // Sin seguir enlaces: el tamaño y el tipo son los del propio fichero.
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            skipped.other += 1;
            continue;
        }
        if meta.is_dir() {
            if depth < MAX_DEPTH {
                walk_max(&path, out, skipped, depth + 1, max);
            } else {
                skipped.too_deep += 1;
            }
            continue;
        }
        let ext = path
            .extension()
            .map(|x| x.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !meta.is_file()
            || !(EXTENSIONS.contains(&ext.as_str()) || is_image_ext(&ext))
            || meta.len() == 0
            || meta.len() > MAX_SIZE
        {
            skipped.other += 1;
            continue;
        }
        if out.len() >= max {
            skipped.over_limit += 1;
            continue;
        }
        out.push(path);
    }
}

pub(crate) fn own_nif(v: &Vault) -> Option<String> {
    crate::invoicing::settings(v)
        .ok()
        .flatten()
        .and_then(|s| match s.issuer.id {
            taxpain_core::invoice::PartyId::Nif(n) => Some(n),
            taxpain_core::invoice::PartyId::Foreign { .. } => None,
        })
        .or_else(|| crate::companies::info(v).ok().and_then(|i| i.nif))
        .map(|n| n.to_uppercase())
}

/// ¿Es un extracto Norma 43 (AEB/CSB 43)? Mira el registro de cabecera «11»: tras el código
/// van entidad, oficina, cuenta y las fechas inicial y final (posiciones 3 a 32), todo cifras.
/// No se recortan los espacios del final: el titular (posiciones 52 a 77) puede ser corto y
/// venir relleno de espacios.
pub(crate) fn is_norma43(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(400)]).into_owned();
    let first = text.lines().next().unwrap_or("").trim_end_matches('\r');
    first.starts_with("11")
        && first.len() >= 32
        && first.as_bytes()[2..32].iter().all(u8::is_ascii_digit)
}

// ---------- Facturas que ya están en los libros ----------

/// Dónde está una factura recibida respecto a los libros.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BookStatus {
    /// En el libro de recibidas (de TaxPain o de un libro de la carpeta): se le adjunta.
    InBook,
    /// De un periodo que ya declaró el gestor y no está en su libro.
    Historical,
    /// Nueva: a Recibidas para revisar.
    New,
}

const IN_FOLDER_BOOK: &str =
    "está en el libro de recibidas de la carpeta: al importar se adjunta a su factura";
const HISTORICAL: &str = crate::libros::NOT_IN_BOOK;

/// Dónde está una factura recibida, mirando el almacén (no los libros de la carpeta, que van
/// después en [`scan_with`]).
fn received_status(v: &Vault, r: &ReceivedRef) -> BookStatus {
    let conn = v.connection();
    if crate::libros::find_received(conn, r)
        .ok()
        .flatten()
        .is_some()
    {
        return BookStatus::InBook;
    }
    if r.date
        .is_some_and(|d| crate::libros::period_covered(conn, d).unwrap_or(false))
    {
        return BookStatus::Historical;
    }
    BookStatus::New
}

/// Factura recibida con su estado respecto a los libros.
fn received_found(
    e: &taxpain_invoicing::ingest::Extraction,
    status: BookStatus,
    mut warnings: Vec<String>,
) -> (Found, Vec<String>) {
    if status == BookStatus::Historical {
        warnings.push(HISTORICAL.into());
    }
    (
        Found::FacturaRecibida {
            supplier: e
                .supplier_name
                .as_ref()
                .map(|f| f.value.clone())
                .or_else(|| e.supplier_nif.as_ref().map(|f| f.value.to_uppercase())),
            nif: e.supplier_nif.as_ref().map(|f| f.value.to_uppercase()),
            number: e.number.as_ref().map(|f| f.value.clone()),
            date: e.issue_date.as_ref().map(|f| f.value),
            total: e.total.as_ref().map(|f| f.value),
            in_book: status == BookStatus::InBook,
            historical: status == BookStatus::Historical,
        },
        warnings,
    )
}

/// Lo que se ha visto en un fichero y, si es una factura recibida, sus datos (para buscarla
/// también en los libros de la carpeta).
struct Classified {
    found: Found,
    warnings: Vec<String>,
    received: Option<ReceivedRef>,
}

impl From<(Found, Vec<String>)> for Classified {
    fn from((found, warnings): (Found, Vec<String>)) -> Self {
        Classified {
            found,
            warnings,
            received: None,
        }
    }
}

/// Qué es un fichero: las reglas y, si el asistente lo leyó y las reglas no lo entienden del
/// todo, lo que dijo el asistente. Los formatos exactos (fichero de la AEAT, Norma 43, diario y
/// libros reconocidos) mandan siempre las reglas.
fn classify(v: &Vault, ruta: &str, name: &str, bytes: &[u8], own: Option<&str>) -> Classified {
    let mut c = classify_inner(v, name, bytes, own);
    historical_by_route(v, ruta, &mut c);
    c
}

/// Trimestres (año y trimestre, 0 a 3) del periodo que dice la ruta de un fichero: la carpeta
/// del año («2017») y, justo debajo, la del trimestre («T3», «3T», «Q3», «3er trimestre») o la
/// del mes («09»). Sin trimestre ni mes, los cuatro del año. Vacío si la ruta no lleva año.
fn route_quarters(ruta: &str) -> Vec<(i32, u8)> {
    let parts: Vec<&str> = ruta.split(['/', '\\']).collect();
    // Sin el nombre del fichero: «2017-factura.pdf» no es una carpeta del año.
    let dirs = &parts[..parts.len().saturating_sub(1)];
    let Some(i) = dirs.iter().rposition(|d| {
        d.trim().len() == 4
            && d.trim()
                .parse::<i32>()
                .is_ok_and(|y| (2000..=2099).contains(&y))
    }) else {
        return Vec::new();
    };
    let year: i32 = dirs[i].trim().parse().unwrap_or_default();
    let quarter = dirs.get(i + 1).and_then(|d| {
        let t: String = d
            .to_uppercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        let digit = |s: &str| s.parse::<u8>().ok().filter(|n| (1..=4).contains(n));
        if let Some(n) = t
            .strip_prefix('T')
            .or_else(|| t.strip_prefix('Q'))
            .and_then(digit)
        {
            return Some(n - 1);
        }
        for suffix in ["T", "Q", "TRIMESTRE", "ERTRIMESTRE", "OTRIMESTRE"] {
            if let Some(n) = t.strip_suffix(suffix).and_then(digit) {
                return Some(n - 1);
            }
        }
        // Un mes con dos cifras («09»).
        (t.len() == 2)
            .then(|| t.parse::<u8>().ok())
            .flatten()
            .filter(|m| (1..=12).contains(m))
            .map(|m| (m - 1) / 3)
    });
    match quarter {
        Some(q) => vec![(year, q)],
        None => (0..4).map(|q| (year, q)).collect(),
    }
}

/// ¿Ha declarado ya el gestor todo ese trimestre (303 del trimestre o de sus tres meses, o el
/// 390 del año), según el almacén?
fn quarter_covered(v: &Vault, (year, q): (i32, u8)) -> bool {
    (1..=3).all(|k| {
        time::Month::try_from(q * 3 + k)
            .ok()
            .and_then(|m| Date::from_calendar_date(year, m, 15).ok())
            .is_some_and(|d| crate::libros::period_covered(v.connection(), d).unwrap_or(false))
    })
}

/// Una factura recibida sin fecha (la lectura no la trae: una factura extranjera, un recibo de
/// una web) se sitúa por la carpeta en que está: si el gestor ya declaró ese periodo y no está
/// en su libro, no se propone, como las de fecha conocida.
fn historical_by_route(v: &Vault, ruta: &str, c: &mut Classified) {
    if c.received.as_ref().is_none_or(|r| r.date.is_some()) {
        return;
    }
    let quarters = route_quarters(ruta);
    if !quarters.is_empty() && quarters.iter().all(|q| quarter_covered(v, *q)) {
        mark_found(&mut c.found, &mut c.warnings, BookStatus::Historical);
    }
}

fn classify_inner(v: &Vault, name: &str, bytes: &[u8], own: Option<&str>) -> Classified {
    let c = classify_rules(v, name, bytes, own);
    // Un justificante de la AEAT con sus casillas leídas también: su portada se comprueba con
    // el número de justificante y el expediente.
    let exact = matches!(
        &c.found,
        Found::Modelo { source, boxes_read, .. }
            if source == "fichero" || source == "registros"
                || (source == "justificante" && *boxes_read)
    ) || matches!(
        c.found,
        Found::Diario { .. } | Found::Libro { .. } | Found::Cripto { .. } | Found::Valores { .. }
    )
        // Un zip: de él sólo se leen los historiales de cripto, y si no los hay se dice por qué
        // (la lectura con IA describe el zip, pero no lo importa).
        || name.to_lowercase().ends_with(".zip")
        // Un extracto que cuadra con sus saldos (o que no los trae, como los listados de
        // tarjeta) es eso seguro; si no cuadra, puede que el asistente lo haya leído mejor.
        || matches!(c.found, Found::Banco { cuadra, .. } if cuadra != Some(false))
        // Un documento de la Sede de la AEAT, por su código seguro de verificación y su título:
        // el asistente no distingue un certificado de situación censal de uno de estar al
        // corriente, ni un requerimiento de una notificación cualquiera.
        || matches!(&c.found, Found::Documento { doc_kind, by_name: false, .. }
            if doc_kind.de_la_aeat());
    if exact {
        return not_sales(v, bytes, c);
    }
    match crate::lectura_ia::stored(v, bytes) {
        Ok(Some(r)) => ia_found(v, name, bytes, &r),
        _ => c,
    }
}

/// Un libro de emitidas que las reglas leen como tal pero del que la lectura con IA dice que es
/// otra cosa (la autorrepercusión de compras, por ejemplo): no se propone, para no importar como
/// ventas lo que no lo es.
fn not_sales(v: &Vault, bytes: &[u8], mut c: Classified) -> Classified {
    if let Found::Libro {
        book: BookKind::Emitidas,
        reading,
        ..
    } = &mut c.found
        && let Ok(Some(r)) = crate::lectura_ia::stored(v, bytes)
        && r.tipo != "libro_emitidas"
    {
        let what = match r.descripcion.trim() {
            "" => crate::lectura_ia::label(&r),
            d => format!("{}: «{d}»", crate::lectura_ia::label(&r)),
        };
        c.warnings.push(format!(
            "la lectura con IA dice que no es un libro de ventas ({what}); por eso no se marca \
             para importar"
        ));
        *reading = Some(what);
    }
    c
}

/// Vista previa del diario de una lectura con IA, con las columnas que señaló.
fn ia_journal(
    v: &Vault,
    name: &str,
    bytes: &[u8],
    r: &crate::lectura_ia::Reading,
) -> Option<crate::import::Preview> {
    use crate::lectura_ia as ia;
    let plan = r.hoja.as_ref()?;
    let hojas = crate::import::sheets(name, bytes).ok()?;
    let (mapped, _) = ia::remap_hojas(&hojas, plan, ia::Contenido::Diario, None);
    crate::import::preview_rows(v, bytes, &mapped).ok()
}

fn ia_found(v: &Vault, name: &str, bytes: &[u8], r: &crate::lectura_ia::Reading) -> Classified {
    use crate::lectura_ia as ia;
    let get = |j: &Option<serde_json::Value>, k: &str| {
        j.as_ref()
            .and_then(|x| x.get(k))
            .map(|x| x.to_string().trim_matches('"').to_owned())
            .filter(|x| !x.is_empty() && x != "null")
    };
    let mut year = None;
    let mut importable = ia::importable(r);
    // Lo que no se propone tampoco se puede marcar, salvo lo que se diga aquí.
    let mut selectable = None;
    let mut received = None;
    let mut warnings = Vec::new();
    let detalle = match r.tipo.as_str() {
        // Las mismas comprobaciones que un diario que leen las reglas: si ya se importó, o si ya
        // hay asientos de esas fechas (propios, de otro lote o del ejercicio del gestor), no se
        // propone; si ya se importó, tampoco se puede marcar.
        "diario" => match ia_journal(v, name, bytes, r) {
            Some(p) => {
                year = p.last_date.map(|d| d.year());
                let entries = p.entries;
                let already = p.already.is_some();
                let (overlap, imported, w) = journal_overlap(v, p);
                warnings = w;
                selectable = Some(importable && !already);
                importable &= !already && overlap == 0 && imported == 0;
                format!(
                    "{entries} asientos{}",
                    if already {
                        " · ya importado"
                    } else if overlap > 0 || imported > 0 {
                        " · TaxPain ya tiene asientos de esas fechas"
                    } else {
                        ""
                    }
                )
            }
            None => "no salen asientos con esas columnas".into(),
        },
        // En emitidas, las líneas que son la autorrepercusión de compras no son ventas: si lo
        // son todas, no se propone (se puede marcar: al importarlo, ésas no entran).
        "libro_emitidas" | "libro_recibidas" => match ia::book_of(name, bytes, r) {
            Some(mut b) => {
                year = b.invoices.iter().map(|i| i.issue_date.year()).max();
                let n = b.invoices.len();
                let sa =
                    crate::libros::mark_self_assessed_in_vault(v.connection(), &mut b).unwrap_or(0);
                if sa > 0 {
                    warnings.push(self_assessed_note(sa, n));
                }
                if sa >= n {
                    selectable = Some(importable);
                    importable = false;
                }
                format!("{n} facturas")
            }
            None => "no salen facturas con esas columnas".into(),
        },
        "factura_recibida" | "factura_emitida" => {
            let f = &r.factura;
            year = get(f, "fecha").and_then(|d| d.get(..4).and_then(|y| y.parse().ok()));
            let mut parts: Vec<String> = [
                get(f, "nombre_emisor"),
                get(f, "numero"),
                get(f, "fecha"),
                get(f, "total").map(|t| format!("total {t}")),
            ]
            .into_iter()
            .flatten()
            .collect();
            if let Some(f) = f {
                if r.tipo == "factura_recibida" {
                    let rr = ReceivedRef::from_extraction(&ia::invoice_extraction(f));
                    match received_status(v, &rr) {
                        BookStatus::InBook => {
                            parts.push("ya está en el libro de recibidas: se le adjunta".into());
                        }
                        BookStatus::Historical => {
                            importable = false;
                            parts.push(HISTORICAL.into());
                        }
                        BookStatus::New => {}
                    }
                    received = Some(rr);
                } else if let Some(msg) = ia::issued_status(v, f) {
                    importable = false;
                    parts.push(msg);
                }
            }
            parts.join(" · ")
        }
        "extracto_bancario" => match crate::extractos::leer_con_lectura(name, bytes, Some(r)) {
            Ok(l) => {
                use crate::extractos::Cuadre;
                year = l
                    .iter()
                    .filter_map(|x| x.extracto.hasta)
                    .max()
                    .map(|d| d.year());
                let no_cuadra = l
                    .iter()
                    .any(|x| matches!(x.cuadre, Cuadre::NoCuadra { .. }));
                importable = !no_cuadra && (r.hoja.is_some() || r.extracto.is_some());
                let mut t = crate::extractos::resumen(&l);
                if no_cuadra {
                    t.push_str(" · no cuadra con sus saldos");
                }
                t
            }
            Err(e) => {
                importable = false;
                e.to_string()
            }
        },
        "modelo_aeat" => {
            let m = &r.modelo;
            year = get(m, "ejercicio").and_then(|y| y.parse().ok());
            format!(
                "modelo {} {} {}",
                get(m, "modelo").unwrap_or_default(),
                get(m, "ejercicio").unwrap_or_default(),
                get(m, "periodo").unwrap_or_default()
            )
        }
        _ => [get(&r.empresa, "name"), get(&r.empresa, "nif")]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · "),
    };
    Classified {
        found: Found::Ia {
            tipo: r.tipo.clone(),
            titulo: ia::doc_kind_of(name, bytes, r)
                .map_or_else(|| ia::label(r), |k| k.title().to_owned()),
            descripcion: r.descripcion.clone(),
            detalle,
            importable,
            selectable: selectable.unwrap_or(importable),
            year,
        },
        warnings,
        received,
    }
}

/// Asientos que TaxPain ya tiene en las fechas de un diario: (propios, de otros lotes
/// importados, avisos). Los propios incluyen los de un ejercicio del gestor ya cargado
/// («Un diario entero»), que además se dice cuál es.
fn journal_overlap(v: &Vault, p: crate::import::Preview) -> (usize, usize, Vec<String>) {
    // Los avisos de los asientos que ya hay ya vienen de la vista previa.
    let mut warnings = p.warnings;
    let (Some(a), Some(b)) = (p.first_date, p.last_date) else {
        return (0, 0, warnings);
    };
    let overlap = crate::import::own_entries_between(v, a, b).unwrap_or(0);
    let imported = crate::import::imported_entries_between(v, a, b)
        .map(|l| l.iter().map(|(_, _, n)| n).sum())
        .unwrap_or(0);
    for (year, file) in crate::ejercicio_gestor::lots_between(v, a, b).unwrap_or_default() {
        warnings.push(format!(
            "el ejercicio {year} ya está cargado asiento a asiento con el diario del gestor \
             («{file}», en Saldos iniciales → «Un diario entero»): este diario lo duplicaría"
        ));
    }
    if overlap > 0 || imported > 0 {
        warnings.push("por eso no se marca para importar".into());
    }
    (overlap, imported, warnings)
}

/// Diario de asientos: avisa si TaxPain ya tiene asientos (propios, de otro lote importado o de
/// un ejercicio del gestor) en esas fechas.
fn journal(v: &Vault, p: crate::import::Preview) -> (Found, Vec<String>) {
    let (entries, first, last, already) =
        (p.entries, p.first_date, p.last_date, p.already.is_some());
    let (overlap, imported, warnings) = journal_overlap(v, p);
    (
        Found::Diario {
            entries,
            first,
            last,
            already,
            overlap,
            imported,
        },
        warnings,
    )
}

/// « ojo: …» para el resultado de importar un diario de fechas que ya tenían asientos.
fn overlap_note(overlap: usize, imported: usize) -> String {
    let mut s = String::new();
    if overlap > 0 {
        s.push_str(&format!(
            "; ojo: TaxPain ya tenía {overlap} asiento(s) propios en esas fechas"
        ));
    }
    if imported > 0 {
        s.push_str(&format!(
            "; ojo: TaxPain ya tenía {imported} asiento(s) de otro lote importado en esas fechas \
             (deshaz el lote anterior si era el mismo diario)"
        ));
    }
    s
}

/// Aviso de un libro de emitidas con líneas que son la autorrepercusión de compras.
fn self_assessed_note(self_assessed: usize, invoices: usize) -> String {
    const POR_QUE: &str = "(inversión del sujeto pasivo o adquisiciones intracomunitarias, arts. \
                           84.Uno.2º y 85 LIVA)";
    match (self_assessed >= invoices, self_assessed) {
        (true, 1) => format!(
            "es la autorrepercusión de una compra {POR_QUE}: su línea es una factura de un \
             proveedor que va en el libro de recibidas, no una venta; por eso no se marca para \
             importar"
        ),
        (true, n) => format!(
            "es la autorrepercusión de compras {POR_QUE}: sus {n} líneas son facturas de \
             proveedores que van en el libro de recibidas, no ventas; por eso no se marca para \
             importar"
        ),
        (false, 1) => format!(
            "1 de sus {invoices} líneas es la autorrepercusión de una compra {POR_QUE}: al \
             importarlo no entra como venta"
        ),
        (false, n) => format!(
            "{n} de sus {invoices} líneas son la autorrepercusión de compras {POR_QUE}: al \
             importarlo no entran como ventas"
        ),
    }
}

/// Base e IVA de lo que entra de un libro: sin las líneas de autorrepercusión de compras.
fn sales_totals(b: &crate::libros::Book) -> (Money, Money) {
    b.invoices
        .iter()
        .filter(|i| !i.self_assessed)
        .fold((Money::ZERO, Money::ZERO), |(base, vat), i| {
            (base + i.base(), vat + i.vat())
        })
}

/// Libro registro: avisa si TaxPain ya tiene facturas propias en esas fechas y, en emitidas, de
/// las líneas que son la autorrepercusión de compras (con las recibidas del libro de TaxPain;
/// [`link_folder_books`] mira después las de los libros de la carpeta).
fn book(v: &Vault, b: &crate::libros::Book) -> (Found, Vec<String>) {
    let mut b = b.clone();
    let self_assessed =
        crate::libros::mark_self_assessed_in_vault(v.connection(), &mut b).unwrap_or(0);
    let first = b.invoices.iter().map(|i| i.issue_date).min();
    let last = b.invoices.iter().map(|i| i.issue_date).max();
    let (base, vat) = sales_totals(&b);
    let overlap = match (first, last) {
        (Some(a), Some(z)) => {
            crate::libros::own_invoices_between(v.connection(), b.kind, a, z).unwrap_or(0)
        }
        _ => 0,
    };
    let mut warnings = b.warnings.clone();
    if let (Some(a), Some(z)) = (first, last)
        && overlap > 0
    {
        warnings.push(format!(
            "TaxPain ya tiene {overlap} factura(s) {} propias entre el {a} y el {z}: las del \
             libro con el mismo número no se duplican, pero las demás se contarían dos veces en \
             el 303; por eso no se marca para importar",
            if b.kind == BookKind::Emitidas {
                "emitidas"
            } else {
                "recibidas revisadas"
            }
        ));
    }
    if self_assessed > 0 {
        warnings.push(self_assessed_note(self_assessed, b.invoices.len()));
    }
    (
        Found::Libro {
            book: b.kind,
            invoices: b.invoices.len(),
            first,
            last,
            base,
            vat,
            overlap,
            self_assessed,
            reading: None,
        },
        warnings,
    )
}

/// Qué es un fichero, sólo con las reglas.
fn classify_rules(v: &Vault, name: &str, bytes: &[u8], own: Option<&str>) -> Classified {
    let lower = name.to_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    // Modelos de la AEAT (fichero o PDF).
    if let Some(r) = crate::presentados::read(bytes) {
        // El justificante de un modelo de otra empresa (el de una participada que está en la
        // carpeta, por ejemplo) no es de ésta.
        if let (Some(nif), Some(own)) = (r.nif.as_deref(), own)
            && crate::revision::expediente::nif_clave(nif)
                != crate::revision::expediente::nif_clave(own)
        {
            return (
                Found::Otro {
                    reason: format!(
                        "justificante del modelo {} {} {} de otra empresa (NIF {nif})",
                        r.model, r.year, r.period
                    ),
                },
                vec![],
            )
                .into();
        }
        let mut warnings = Vec::new();
        if r.year_guessed {
            warnings.push(format!(
                "no dice el ejercicio: se ha deducido {} por las fechas del documento; \
                 compruébalo",
                r.year
            ));
        }
        if r.source == "justificante" && !r.boxes_read {
            warnings.push(
                "no se han podido leer sus casillas: cuenta como presentado, pero no se compara \
                 ni entra en los cruces"
                    .into(),
            );
        }
        return (
            Found::Modelo {
                boxes: r.boxes.len(),
                model: r.model,
                year: r.year,
                period: r.period,
                source: r.source,
                boxes_read: r.boxes_read,
            },
            warnings,
        )
            .into();
    }
    // 347, 349, 190, 180 y 193 en fichero: se leen sus terceros o perceptores para la revisión
    // (y, salvo el 349, se guardan para compararlos con TaxPain).
    if let Some(r) = crate::presentados::read_informative(bytes) {
        return (
            Found::Modelo {
                boxes: r.lines.len(),
                model: r.model,
                year: r.year,
                period: r.period,
                source: "registros".into(),
                boxes_read: true,
            },
            vec![],
        )
            .into();
    }
    match ext {
        "zip" => match crate::cripto::paquete::leer(v, name, bytes) {
            Ok(p) => cripto(&p).into(),
            Err(reason) => (Found::Otro { reason }, vec![]).into(),
        },
        "csv" if crate::valores::flex::es_flex_csv(&String::from_utf8_lossy(bytes)) => {
            valores(v, name, bytes)
        }
        // El informe de cuenta de XTB en Excel (uno por cuenta), antes que los libros.
        "xlsx" if crate::valores::xtb_xlsx::es_informe(bytes) => valores(v, name, bytes),
        "xlsx" | "xlsm" | "xls" | "ods" | "csv" => {
            // Los historiales de cripto de formato exacto (CoinTracking, Kraken) van antes: no
            // son libros ni diarios. Una hoja genérica (fecha, tipo, activo, cantidad…) o de
            // saldos por fecha, sólo si no es otra cosa y trae alguna criptomoneda conocida.
            let cripto_leido = crate::cripto::paquete::leer(v, name, bytes).ok();
            if let Some(p) = &cripto_leido
                && p.exacto()
            {
                return cripto(p).into();
            }
            match crate::libros::parse(name, bytes) {
                Ok(Some(b)) => return book(v, &b).into(),
                Ok(None) => {}
                Err(e) => {
                    // Muchos bancos descargan como `.xls` una tabla HTML o el XML de Excel 2003,
                    // que no son hojas de cálculo de verdad.
                    if let Some(c) = banco(name, bytes) {
                        return c;
                    }
                    return (
                        Found::Otro {
                            reason: e.to_string(),
                        },
                        vec![],
                    )
                        .into();
                }
            }
            match crate::import::preview(v, name, bytes) {
                Ok(p) if p.entries > 0 => journal(v, p).into(),
                _ => match &cripto_leido {
                    Some(p) => cripto(p).into(),
                    None => banco(name, bytes).unwrap_or_else(|| {
                        (
                            Found::Otro {
                                reason: "hoja de cálculo que no es un diario, un libro de \
                                         facturas, un extracto bancario ni un historial de \
                                         criptomonedas"
                                    .into(),
                            },
                            vec![],
                        )
                            .into()
                    }),
                },
            }
        }
        "xml" | "xsig" => {
            let text = String::from_utf8_lossy(bytes);
            // Activity Flex de Interactive Brokers.
            if crate::valores::flex::es_flex_xml(&text) {
                return valores(v, name, bytes);
            }
            // camt.053 (extracto), camt.052 (informe del día) o camt.054 (avisos).
            if text.contains("BkToCstmr") {
                return banco_o_motivo(name, bytes, "extracto camt que no se puede leer");
            }
            invoice(v, name, bytes, own)
        }
        "n43" | "aeb" | "q43" | "c43" => {
            banco_o_motivo(name, bytes, "extracto Norma 43 que no se puede leer")
        }
        "ofx" | "qfx" | "qif" | "sta" | "940" | "mt940" | "mt942" | "htm" | "html" | "tsv" => {
            banco_o_motivo(name, bytes, "no es un extracto bancario que se pueda leer")
        }
        "txt" | "dat" => {
            if is_norma43(bytes) {
                return banco_o_motivo(name, bytes, "extracto Norma 43 que no se puede leer");
            }
            if ext == "txt"
                && let Some(d) = crate::documentos::recognize(name, bytes).filter(|d| !d.by_name)
            {
                return document(d).into();
            }
            match crate::import::preview(v, name, bytes) {
                Ok(p) if p.entries > 0 => journal(v, p).into(),
                _ => banco_o_motivo(
                    name,
                    bytes,
                    "fichero de texto que no es un diario, un extracto ni un modelo",
                ),
            }
        }
        "pdf" => {
            // Los extractos en PDF, antes que las facturas (con una comprobación rápida del texto
            // para no pasar cada factura por los lectores de extractos).
            let pdf = crate::extractos::mime::pdf_dentro(bytes);
            let pdf = pdf.as_deref().unwrap_or(bytes);
            let texto = crate::pdf_text::text(pdf)
                .filter(|t| t.trim().len() > 40)
                .or_else(|| ocr_text(name, pdf));
            // El informe de cuenta de XTB (posiciones y operaciones en efectivo).
            if texto
                .as_deref()
                .is_some_and(crate::valores::xtb::es_informe)
            {
                return valores(v, name, pdf);
            }
            if texto
                .as_deref()
                .is_some_and(crate::extractos::parece_extracto)
                && let Some(c) = banco(name, bytes)
            {
                // Si cuadra con sus saldos, es un extracto seguro. Si no trae saldos (o no
                // cuadra), puede ser una factura con líneas fechadas (luz, teléfono…): gana la
                // factura si tiene número e importe.
                if matches!(
                    c.found,
                    Found::Banco {
                        cuadra: Some(true),
                        ..
                    }
                ) {
                    return c;
                }
                let f = invoice(v, name, bytes, own);
                return match &f.found {
                    Found::FacturaRecibida {
                        number: Some(_),
                        total: Some(_),
                        ..
                    }
                    | Found::FacturaEmitida {
                        number: Some(_),
                        total: Some(_),
                        ..
                    }
                    | Found::Modelo { .. } => f,
                    _ => c,
                };
            }
            invoice(v, name, bytes, own)
        }
        // Una foto o captura de un extracto.
        e if is_image_ext(e)
            && ocr_text(name, bytes).is_some_and(|t| crate::extractos::parece_extracto(&t))
            && let Some(c) = banco(name, bytes) =>
        {
            c
        }
        e if is_image_ext(e) => match ocr_text(name, bytes).and_then(|t| from_ocr(v, &t, own)) {
            Some(found) => found,
            None => match crate::documentos::recognize(name, bytes) {
                Some(d) => document(d).into(),
                None => (
                    Found::Otro {
                        reason: if OCR.get().is_some() {
                            "imagen sin texto reconocible".into()
                        } else {
                            "imagen: sin reconocimiento de texto en este equipo".into()
                        },
                    },
                    vec![],
                )
                    .into(),
            },
        },
        _ => (
            Found::Otro {
                reason: "tipo de fichero no soportado".into(),
            },
            vec![],
        )
            .into(),
    }
}

/// Operaciones o precios de criptomonedas leídos.
fn cripto(p: &crate::cripto::paquete::Paquete) -> (Found, Vec<String>) {
    let (first, last) = p.fechas();
    let mut warnings = p.avisos();
    if p.ops() > 0 && p.ops() == p.duplicadas() && p.precios() == 0 {
        warnings.push("ya estaban importadas en Criptomonedas: no se marca para importar".into());
    } else if p.ops() > 0 {
        warnings.push(
            "al importarlas no se contabilizan: los asientos se generan aparte, en \
             Criptomonedas; no los generes para los ejercicios que ya contabilizó el gestor"
                .into(),
        );
    }
    (
        Found::Cripto {
            resumen: p.resumen(),
            ops: p.ops(),
            prices: p.precios(),
            duplicates: p.duplicadas(),
            first,
            last,
            unread: p.no_leidos.len(),
            huecos: p.huecos.len(),
        },
        warnings,
    )
}

/// Extracto de un bróker de valores: lo que entraría en Inversiones en valores.
fn valores(v: &Vault, name: &str, bytes: &[u8]) -> Classified {
    let l = match crate::valores::importar::leer(v, &[(name.to_owned(), bytes.to_vec())]) {
        Ok(l) => l,
        Err(e) => {
            return (
                Found::Otro {
                    reason: e.to_string(),
                },
                vec![],
            )
                .into();
        }
    };
    let Some(f) = l.ficheros.first() else {
        return (
            Found::Otro {
                reason: "extracto de bróker vacío".into(),
            },
            vec![],
        )
            .into();
    };
    if let Some(e) = &f.error {
        return (Found::Otro { reason: e.clone() }, vec![]).into();
    }
    let ops = l.nuevas + l.duplicadas;
    let mut warnings = l.avisos.clone();
    if ops > 0 && l.nuevas == 0 && l.precios == 0 {
        warnings.push(
            "ya estaban importadas en Inversiones en valores: no se marca para importar".into(),
        );
    } else if ops > 0 {
        warnings.push(
            "al importarlas no se contabilizan: los asientos se proponen en Inversiones en \
             valores; no los lleves al diario en los ejercicios que ya contabilizó el gestor"
                .into(),
        );
    }
    (
        Found::Valores {
            formato: f.formato.clone(),
            cuenta: f.cuenta.clone(),
            resumen: l
                .ficheros
                .iter()
                .map(|x| x.resumen.clone())
                .collect::<Vec<_>>()
                .join("; "),
            ops,
            prices: l.precios,
            duplicates: l.duplicadas,
            first: l.ficheros.iter().filter_map(|x| x.desde).min(),
            last: l.ficheros.iter().filter_map(|x| x.hasta).max(),
        },
        warnings,
    )
        .into()
}

/// Extracto bancario, si lo es (de cualquier banco y formato).
fn banco(name: &str, bytes: &[u8]) -> Option<Classified> {
    use crate::extractos::Cuadre;
    let leidos = crate::extractos::leer(name, bytes).ok()?;
    let last = leidos.iter().filter_map(|l| l.extracto.hasta).max();
    let cuadra = if leidos.iter().all(|l| l.cuadre == Cuadre::SinSaldos) {
        None
    } else {
        Some(
            leidos
                .iter()
                .all(|l| !matches!(l.cuadre, Cuadre::NoCuadra { .. })),
        )
    };
    let mut warnings: Vec<String> = leidos
        .iter()
        .flat_map(|l| l.extracto.avisos.iter().cloned())
        .collect();
    for l in &leidos {
        if let Cuadre::NoCuadra { detalle, .. } = &l.cuadre {
            warnings.push(format!(
                "no cuadra con sus saldos ({detalle}): no se marca para importar"
            ));
        }
    }
    Some(
        (
            Found::Banco {
                resumen: crate::extractos::resumen(&leidos),
                last,
                cuadra,
            },
            warnings,
        )
            .into(),
    )
}

/// [`banco`] o, si no se puede leer, «otro» con el motivo.
fn banco_o_motivo(name: &str, bytes: &[u8], motivo: &str) -> Classified {
    banco(name, bytes).unwrap_or_else(|| {
        (
            Found::Otro {
                reason: motivo.to_owned(),
            },
            vec![],
        )
            .into()
    })
}

fn document(d: crate::documentos::Recognized) -> (Found, Vec<String>) {
    let warnings = if d.by_name {
        vec!["reconocido por el nombre del fichero (sin texto): sus datos se teclean a mano".into()]
    } else if d.facts.is_empty() {
        vec!["no se han podido leer sus datos: puedes completarlos con el asistente".into()]
    } else {
        vec![]
    };
    (
        Found::Documento {
            doc_kind: d.kind,
            title: d.title,
            facts: Box::new(d.facts),
            by_name: d.by_name,
        },
        warnings,
    )
}

/// PDF o Facturae: documento de la empresa, o factura recibida o emitida (por el NIF del
/// emisor). Una factura (con número e importe, que dice «factura») va antes que un documento:
/// la del notario habla de la escritura, pero es una factura. Y un documento reconocido sólo por
/// el nombre del fichero no gana a una factura de la que se ha leído el número o el importe.
fn invoice(v: &Vault, name: &str, bytes: &[u8], own: Option<&str>) -> Classified {
    // Un documento de la Sede de la AEAT (su código seguro de verificación y su título) no es
    // nunca una factura, aunque hable de importes, referencias y NIF.
    if let Some(d) =
        crate::documentos::recognize(name, bytes).filter(|d| !d.by_name && d.kind.de_la_aeat())
    {
        return document(d).into();
    }
    let e = crate::received::extract(name, bytes, own);
    let nif = e.supplier_nif.as_ref().map(|f| f.value.to_uppercase());
    let number = e.number.as_ref().map(|f| f.value.clone());
    let date = e.issue_date.as_ref().map(|f| f.value);
    let total = e.total.as_ref().map(|f| f.value);
    let looks_invoice = number.is_some()
        && total.is_some()
        && (name.to_lowercase().contains("factura")
            || crate::pdf_text::text(bytes).is_some_and(|t| t.to_lowercase().contains("factura")));
    // Un informe (la revisión que exporta TaxPain, una revisión fiscal hecha con el asistente),
    // un correo o un escrito a la Administración hablan de escrituras y depósitos, pero no son
    // documentos oficiales.
    if !looks_invoice
        && crate::pdf_text::text(bytes).is_some_and(|t| crate::oficiales::informe_o_correo(&t))
    {
        return (
            Found::Otro {
                reason: "informe, correo o escrito: no es una factura ni un documento oficial"
                    .into(),
            },
            vec![],
        )
            .into();
    }
    if !looks_invoice
        && let Some(d) = crate::documentos::recognize(name, bytes)
        && (!d.by_name || (number.is_none() && total.is_none()))
    {
        return document(d).into();
    }
    if number.is_none()
        && total.is_none()
        && nif.is_none()
        && e.warnings.iter().any(|w| w.contains("escaneado"))
        && let Some(found) = ocr_text(name, bytes).and_then(|t| from_ocr(v, &t, own))
    {
        return found;
    }
    // Sin número y sin importe no es una factura, aunque lleve el NIF de la empresa (o el de
    // quien presentó un escrito por ella). Salvo que diga que lo es (el nombre del fichero o su
    // texto) y se sepa quién la emite: un proveedor con su NIF, o la empresa con el NIF de su
    // cliente. Entonces es una factura a la que le faltan datos (para leerla con IA).
    let dice_factura = || {
        let es = |t: &str| {
            let t = t.to_lowercase();
            t.contains("factura") || t.contains("invoice")
        };
        es(name) || crate::pdf_text::text(bytes).is_some_and(|t| es(&t))
    };
    let emisor_claro = match (&nif, own) {
        (Some(n), Some(o)) if n == o => {
            crate::pdf_text::text(bytes).is_some_and(|t| cliente_con_nif(&t, o))
        }
        (Some(_), _) => true,
        (None, _) => false,
    };
    if number.is_none() && total.is_none() && !(emisor_claro && dice_factura()) {
        return (
            Found::Otro {
                reason: if e.warnings.iter().any(|w| w.contains("escaneado")) {
                    "PDF escaneado (sin texto): de momento hay que teclearlo".into()
                } else if nif.is_some() {
                    "sin número ni importe: no se toma por una factura (si lo es, léela con IA)"
                        .into()
                } else {
                    "no parece una factura ni un modelo".into()
                },
            },
            vec![],
        )
            .into();
    }
    if let (Some(n), Some(o)) = (&nif, own)
        && n == o
    {
        return (
            Found::FacturaEmitida {
                number,
                date,
                total,
            },
            vec![],
        )
            .into();
    }
    let r = ReceivedRef::from_extraction(&e);
    let (found, warnings) = received_found(&e, received_status(v, &r), e.warnings.clone());
    Classified {
        found,
        warnings,
        received: Some(r),
    }
}

/// ¿Dice el texto quién es el cliente, con su NIF? Una línea con «Cliente», «Destinatario»,
/// «Facturar a», «Para:»… y un NIF válido que no es el de la empresa, en ella o en la de
/// debajo. Un código que pasa por NIF (el de un artículo) no basta.
fn cliente_con_nif(texto: &str, own: &str) -> bool {
    const CLIENTE: [&str; 7] = [
        "cliente",
        "destinatario",
        "facturar a",
        "facturado a",
        "para:",
        "bill to",
        "to:",
    ];
    let propio = crate::revision::expediente::nif_clave(own);
    let ajeno = |l: &str| {
        l.split(|c: char| !c.is_ascii_alphanumeric())
            .map(str::to_uppercase)
            .any(|t| {
                let t = t
                    .strip_prefix("ES")
                    .filter(|r| r.len() == 9)
                    .map_or(t.as_str(), |r| r);
                crate::revision::expediente::nif_clave(t) != propio
                    && matches!(crate::revision::expediente::nif_español(t), Some(Ok(_)))
            })
    };
    let lineas: Vec<String> = texto.lines().map(str::to_lowercase).collect();
    lineas.iter().enumerate().any(|(i, l)| {
        CLIENTE.iter().any(|c| l.contains(c))
            && (ajeno(l) || lineas.get(i + 1).is_some_and(|d| ajeno(d)))
    })
}

/// Lo que da [`files_under_max`]: los ficheros y lo que se ha dejado fuera por los topes.
#[derive(Debug, Default)]
pub(crate) struct Walked {
    pub files: Vec<PathBuf>,
    /// Admitidos que no se miran por pasar del tope de ficheros.
    pub over_limit: usize,
    /// Subcarpetas a más de [`MAX_DEPTH`] niveles, que no se recorren.
    pub too_deep: usize,
}

/// Como mucho tantos ficheros se miran de una carpeta vigilada (los de «Importar la carpeta»).
pub(crate) const MAX_WALKED_FILES: usize = MAX_FILES;

/// Ficheros admitidos de `root` y sus subcarpetas, como al importar la carpeta (sin ocultos ni
/// enlaces, hasta [`MAX_DEPTH`] niveles y `max` ficheros), diciendo cuántos ficheros y
/// subcarpetas se dejan fuera por los topes: la carpeta está incompleta y hay que avisar. Para
/// las carpetas vigiladas ([`crate::vigiladas`]).
pub(crate) fn files_under_max(root: &Path, max: usize) -> Walked {
    let mut files = Vec::new();
    let mut skipped = Skipped::default();
    walk_max(root, &mut files, &mut skipped, 0, max);
    Walked {
        files,
        over_limit: skipped.over_limit,
        too_deep: skipped.too_deep,
    }
}

/// Qué es un fichero suelto de `root`, como lo vería «Importar la carpeta» (sin cruzarlo con los
/// libros de la carpeta). Lo lento (texto, reconocimiento de texto) se hace antes, sin `vault`.
pub(crate) fn classify_one<G: Deref<Target = Vault>>(
    root: &Path,
    path: &Path,
    bytes: &[u8],
    vault: impl Fn() -> Result<G>,
) -> Result<Found> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    prewarm(&name, bytes);
    let v = vault()?;
    let own = own_nif(&v);
    Ok(classify(&v, &relative(root, path), &name, bytes, own.as_deref()).found)
}

fn relative(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .into_owned()
}

/// Lo lento de mirar un fichero (el texto del PDF, el reconocimiento de texto), sin el almacén:
/// queda en caché para cuando se clasifique con él.
fn prewarm(name: &str, bytes: &[u8]) {
    let lower = name.to_lowercase();
    if bytes.starts_with(b"%PDF") {
        if crate::pdf_text::text(bytes).is_none_or(|t| t.trim().len() <= 20) {
            let _ = ocr_text(name, bytes);
        }
    } else if lower.rsplit_once('.').is_some_and(|(_, e)| is_image_ext(e)) {
        let _ = ocr_text(name, bytes);
    }
}

/// Recorre la carpeta y dice qué es cada fichero. No cambia nada.
pub fn scan(v: &Vault, root: &Path) -> Result<Inventory> {
    scan_with(root, || Ok(v))
}

/// Como [`scan`], cogiendo el almacén con `vault` (en la app, su cerrojo) sólo para lo que lo
/// necesita: lo lento de cada fichero se hace antes, sin él.
pub fn scan_with<G: Deref<Target = Vault>>(
    root: &Path,
    vault: impl Fn() -> Result<G>,
) -> Result<Inventory> {
    if !root.is_dir() {
        return Err(AppError::Other(format!(
            "{} no es una carpeta",
            root.display()
        )));
    }
    let claude_code = crate::lectura_ia::load_package(&*vault()?, root, OffsetDateTime::now_utc())?;
    let mut files = Vec::new();
    let mut skipped = Skipped::default();
    walk(root, &mut files, &mut skipped, 0);
    let own = own_nif(&*vault()?);
    let mut items = Vec::new();
    let mut refs: Vec<Option<ReceivedRef>> = Vec::new();
    for path in files {
        let Ok(bytes) = std::fs::read(&path) else {
            skipped.other += 1;
            continue;
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        prewarm(&name, &bytes);
        let c = classify(
            &*vault()?,
            &relative(root, &path),
            &name,
            &bytes,
            own.as_deref(),
        );
        items.push(Item {
            path: relative(root, &path),
            size: u64::try_from(bytes.len()).unwrap_or(0),
            year: c.found.year(),
            importable: c.found.importable(),
            selectable: c.found.selectable(),
            found: c.found,
            warnings: c.warnings,
        });
        refs.push(c.received);
    }
    link_folder_books(root, &vault, &mut items, &refs)?;
    items.sort_by(|a, b| {
        a.found
            .order()
            .cmp(&b.found.order())
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(Inventory {
        root: root.to_string_lossy().into_owned(),
        claude_code,
        items,
        skipped: skipped.other,
        over_limit: skipped.over_limit,
        too_deep: skipped.too_deep,
    })
}

/// Las facturas recibidas que están en un libro de recibidas de la misma carpeta (aún sin
/// importar) se marcan como «en el libro»: al aplicar, el libro va antes y se les adjuntan. Las
/// de un trimestre que cubre ese libro y que no están en él, como de periodo ya declarado.
fn link_folder_books<G: Deref<Target = Vault>>(
    root: &Path,
    vault: &impl Fn() -> Result<G>,
    items: &mut [Item],
    refs: &[Option<ReceivedRef>],
) -> Result<()> {
    let mut book: Vec<crate::libros::BookInvoice> = Vec::new();
    for item in items.iter() {
        let parsed = match &item.found {
            Found::Libro {
                book: BookKind::Recibidas,
                ..
            } => read_file(root, &item.path)
                .ok()
                .and_then(|(name, bytes)| crate::libros::parse(&name, &bytes).ok().flatten()),
            Found::Ia { tipo, .. } if tipo == "libro_recibidas" => {
                let Ok((name, bytes)) = read_file(root, &item.path) else {
                    continue;
                };
                crate::lectura_ia::stored(&*vault()?, &bytes)
                    .ok()
                    .flatten()
                    .and_then(|r| crate::lectura_ia::received_book(&name, &bytes, &r))
            }
            _ => None,
        };
        book.extend(parsed.into_iter().flat_map(|b| b.invoices));
    }
    if !book.is_empty() {
        for item in items.iter_mut() {
            link_self_assessed(root, vault, item, &book)?;
        }
    }
    let quarter = |d: Date| (d.year(), (u8::from(d.month()) - 1) / 3);
    let mut covered: std::collections::BTreeSet<(i32, u8)> =
        book.iter().map(|i| quarter(i.registered)).collect();
    // Los modelos del gestor de la carpeta (303 del trimestre, 390 del año) también dicen que
    // ese periodo ya está declarado.
    for item in items.iter() {
        if let Found::Modelo {
            model,
            year,
            period,
            ..
        } = &item.found
        {
            let q = period
                .strip_suffix('T')
                .and_then(|n| n.parse::<u8>().ok())
                .filter(|n| (1..=4).contains(n));
            match (model.as_str(), q) {
                ("303", Some(n)) => {
                    covered.insert((*year, n - 1));
                }
                ("390", _) => covered.extend((0..4).map(|n| (*year, n))),
                _ => {}
            }
        }
    }
    if book.is_empty() && covered.is_empty() {
        return Ok(());
    }
    for (item, r) in items.iter_mut().zip(refs) {
        let Some(r) = r else { continue };
        let in_book = book.iter().any(|i| {
            r.same_as(
                i.nif.as_deref(),
                &i.name,
                &i.number,
                Some(i.issue_date),
                &[i.total, i.base() + i.vat()],
            )
        });
        // Sin fecha, el periodo de la carpeta en que está (ver [`route_quarters`]).
        let quarters = match r.date {
            Some(d) => vec![quarter(d)],
            None => route_quarters(&item.path),
        };
        let status = if in_book {
            BookStatus::InBook
        } else if !quarters.is_empty() && quarters.iter().all(|q| covered.contains(q)) {
            BookStatus::Historical
        } else {
            continue;
        };
        mark(item, status);
    }
    Ok(())
}

/// Las líneas de un libro de emitidas de la carpeta que son compras de un libro de recibidas de
/// la misma carpeta (aún sin importar): la autorrepercusión, no ventas.
fn link_self_assessed<G: Deref<Target = Vault>>(
    root: &Path,
    vault: &impl Fn() -> Result<G>,
    item: &mut Item,
    received: &[crate::libros::BookInvoice],
) -> Result<()> {
    let parsed = match &item.found {
        Found::Libro {
            book: BookKind::Emitidas,
            ..
        } => read_file(root, &item.path)
            .ok()
            .and_then(|(name, bytes)| crate::libros::parse(&name, &bytes).ok().flatten()),
        Found::Ia { tipo, .. } if tipo == "libro_emitidas" => {
            let Ok((name, bytes)) = read_file(root, &item.path) else {
                return Ok(());
            };
            crate::lectura_ia::stored(&*vault()?, &bytes)
                .ok()
                .flatten()
                .and_then(|r| crate::lectura_ia::book_of(&name, &bytes, &r))
        }
        _ => None,
    };
    let Some(mut b) = parsed else {
        return Ok(());
    };
    let n = b.invoices.len();
    let before =
        crate::libros::mark_self_assessed_in_vault(vault()?.connection(), &mut b).unwrap_or(0);
    let after = crate::libros::mark_self_assessed(&mut b, received);
    if after == before {
        return Ok(());
    }
    match &mut item.found {
        Found::Libro {
            self_assessed,
            base,
            vat,
            ..
        } => {
            *self_assessed = after;
            (*base, *vat) = sales_totals(&b);
        }
        Found::Ia {
            importable,
            selectable,
            ..
        } if after >= n => {
            *selectable = true;
            *importable = false;
        }
        _ => {}
    }
    if before > 0 {
        let old = self_assessed_note(before, n);
        item.warnings.retain(|w| *w != old);
    }
    item.warnings.push(self_assessed_note(after, n));
    item.importable = item.found.importable();
    item.selectable = item.found.selectable();
    Ok(())
}

/// Cambia el estado de una factura recibida de la lectura (por un libro de la carpeta).
fn mark(item: &mut Item, status: BookStatus) {
    if mark_found(&mut item.found, &mut item.warnings, status) {
        item.importable = item.found.importable();
        item.selectable = item.found.selectable();
    }
}

/// Como [`mark`], sobre lo visto y sus avisos. Devuelve si ha cambiado algo.
fn mark_found(found: &mut Found, warnings: &mut Vec<String>, status: BookStatus) -> bool {
    let note = match status {
        BookStatus::InBook => IN_FOLDER_BOOK,
        BookStatus::Historical => HISTORICAL,
        BookStatus::New => return false,
    };
    match found {
        Found::FacturaRecibida {
            in_book,
            historical,
            ..
        } => {
            if *in_book {
                return false;
            }
            *in_book = status == BookStatus::InBook;
            *historical = status == BookStatus::Historical;
            warnings.retain(|w| w != HISTORICAL);
        }
        Found::Ia {
            importable,
            detalle,
            ..
        } => {
            if detalle.contains("ya está en el libro") {
                return false;
            }
            *importable = status == BookStatus::InBook;
            *detalle = detalle.replace(&format!(" · {HISTORICAL}"), "");
            if !detalle.contains(note) {
                *detalle = format!("{detalle} · {note}");
            }
        }
        _ => return false,
    }
    if !warnings.iter().any(|w| w == note) {
        warnings.push(note.into());
    }
    true
}

/// Resultado de importar un fichero.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Outcome {
    pub path: String,
    pub ok: bool,
    pub message: String,
}

/// Datos de una factura recibida de un fichero para buscarla en el libro: los del PDF o el XML
/// y, si es una imagen o un escaneado, los del reconocimiento de texto.
fn received_ref(name: &str, bytes: &[u8], own: Option<&str>) -> ReceivedRef {
    let e = crate::received::extract(name, bytes, own);
    if e.total.is_none()
        && e.number.is_none()
        && let Some(t) = ocr_text(name, bytes)
    {
        return ReceivedRef::from_extraction(&taxpain_invoicing::ingest::from_text(&t, own));
    }
    ReceivedRef::from_extraction(&e)
}

/// Las facturas de los libros de recibidas de la carpeta (los que leen las reglas y los leídos con
/// IA), estén o no en TaxPain: con ellas se reconoce en un libro de emitidas la autorrepercusión
/// de compras al importarlo, también si el de recibidas no se importa a la vez. Sólo se miran las
/// hojas de cálculo y los CSV.
fn folder_received<G: Deref<Target = Vault>>(
    root: &Path,
    vault: &impl Fn() -> Result<G>,
) -> Result<Vec<crate::libros::BookInvoice>> {
    let mut files = Vec::new();
    walk(root, &mut files, &mut Skipped::default(), 0);
    let mut out = Vec::new();
    for path in files {
        let ext = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !["xlsx", "xlsm", "xls", "ods", "csv", "txt"].contains(&ext.as_str()) {
            continue;
        }
        let Ok((name, bytes)) = read_file(root, &relative(root, &path)) else {
            continue;
        };
        let book = match crate::libros::parse(&name, &bytes).ok().flatten() {
            Some(b) => Some(b),
            None => crate::lectura_ia::stored(&*vault()?, &bytes)
                .ok()
                .flatten()
                .and_then(|r| crate::lectura_ia::received_book(&name, &bytes, &r)),
        };
        if let Some(b) = book.filter(|b| b.kind == BookKind::Recibidas) {
            out.extend(b.invoices);
        }
    }
    Ok(out)
}

fn apply_one(
    v: &Vault,
    root: &Path,
    path: &str,
    own: Option<&str>,
    received: &[crate::libros::BookInvoice],
    sustituidas: &std::collections::BTreeSet<String>,
    now: OffsetDateTime,
) -> Result<String> {
    let (name, bytes) = read_file(root, path)?;
    // Se vuelve a mirar: la carpeta puede haber cambiado desde que se leyó, y los libros que se
    // acaban de importar cambian lo que es una factura (ya en el libro o no).
    let found = classify(v, path, &name, &bytes, own).found;
    match found {
        Found::Diario { already: true, .. } => Ok("ya estaba importado".into()),
        Found::Diario {
            overlap, imported, ..
        } => {
            let (batch, p) = crate::import::import(v, &name, &bytes, now)?;
            Ok(format!(
                "{} asientos (lote {batch}){}",
                p.entries,
                overlap_note(overlap, imported)
            ))
        }
        Found::Libro {
            book,
            overlap,
            reading,
            ..
        } => {
            let mut b = crate::libros::parse(&name, &bytes)?
                .ok_or_else(|| AppError::Other("el libro ya no se puede leer".into()))?;
            // Las líneas del de emitidas que son compras de un libro de recibidas de la carpeta:
            // la autorrepercusión, aunque ese libro no se importe.
            crate::libros::mark_self_assessed(&mut b, received);
            let origin = crate::libros::BatchOrigin {
                filename: &name,
                bytes: &bytes,
                source: "libro",
            };
            let r = match book {
                BookKind::Emitidas => crate::libros::import_issued_as(v, &b, &origin, now)?,
                BookKind::Recibidas => crate::libros::import_received_as(v, &b, &origin, now)?,
            };
            Ok(format!(
                "{} facturas al libro de {}{}{}{}{}",
                r.added,
                if book == BookKind::Emitidas {
                    "emitidas"
                } else {
                    "recibidas"
                },
                if r.skipped > 0 {
                    format!(" ({} ya estaban)", r.skipped)
                } else {
                    String::new()
                },
                if overlap > 0 {
                    format!("; ojo: TaxPain ya tenía {overlap} factura(s) propias en esas fechas")
                } else {
                    String::new()
                },
                if reading.is_some() {
                    "; ojo: la lectura con IA dice que no es un libro de ventas"
                } else {
                    ""
                },
                if r.warnings.is_empty() {
                    String::new()
                } else {
                    format!(". Avisos: {}", r.warnings.join("; "))
                }
            ))
        }
        Found::Banco { cuadra, .. } => {
            // Uno que no cuadra no se propone: si llega aquí es que el usuario lo ha marcado
            // sabiéndolo, y entra igualmente.
            let forzar = cuadra == Some(false);
            let r =
                crate::extractos::importar::importar_fichero(v, &name, &bytes, None, forzar, now)?;
            Ok(r.frase())
        }
        Found::FacturaRecibida {
            supplier,
            historical,
            ..
        } => {
            let r = received_ref(&name, &bytes, own);
            if let Some(id) = crate::libros::find_received(v.connection(), &r)? {
                return crate::libros::attach_document(v, id, &name, &bytes, now);
            }
            let out = crate::received::ingest_file(
                v,
                &name,
                &bytes,
                crate::received::Source::Carpeta,
                now,
            )?;
            // Escaneada o foto: los datos leídos en el Mac.
            if !out.duplicate
                && out.extraction.total.is_none()
                && let Some(t) = ocr_text(&name, &bytes)
            {
                let mut ex = taxpain_invoicing::ingest::from_text(&t, own);
                ex.warnings
                    .push("leída con el reconocimiento de texto del Mac: revisa los datos".into());
                crate::received::merge_extraction(v, out.id, &ex)?;
            }
            if !out.duplicate && historical {
                crate::libros::warn_pending(v, out.id, HISTORICAL)?;
            }
            Ok(if out.duplicate {
                "ya estaba en Recibidas".into()
            } else {
                format!(
                    "a Recibidas para revisar{}{}",
                    supplier.map(|s| format!(" ({s})")).unwrap_or_default(),
                    if historical {
                        "; ojo: es de un periodo que ya declaró el gestor"
                    } else {
                        ""
                    }
                )
            })
        }
        Found::Modelo { .. } => {
            if let Some(r) = crate::presentados::read_informative(&bytes) {
                // La complementaria de una informativa sólo trae lo que faltaba: no se compara.
                let complementaria =
                    crate::revision::expediente::TipoDeclaracion::de_fichero(&bytes)
                        == Some(crate::revision::expediente::TipoDeclaracion::Complementaria);
                let Some(filed) =
                    crate::presentados::read_informative_filed(&bytes).filter(|_| !complementaria)
                else {
                    return Ok(format!(
                        "modelo {} {} {}: se usa en la revisión de la carpeta, no hace falta \
                         guardarlo",
                        r.model, r.year, r.period
                    ));
                };
                let stored = crate::presentados::store_read(v, &name, &bytes, &filed, false, now)?;
                return Ok(format!(
                    "modelo {} {} {} {} ({} {})",
                    filed.model,
                    filed.year,
                    filed.period,
                    if stored.is_new() {
                        "guardado para comparar"
                    } else {
                        "ya estaba guardado"
                    },
                    r.lines.len(),
                    if filed.model == "347" {
                        "terceros"
                    } else {
                        "perceptores"
                    }
                ));
            }
            let r = crate::presentados::read(&bytes)
                .ok_or_else(|| AppError::Other("el modelo ya no se puede leer".into()))?;
            if r.source == "justificante" {
                if complementaria_de_informativa(&r) {
                    return Ok(format!(
                        "modelo {} {} {}: complementaria de una declaración informativa (sólo \
                         trae lo que faltaba en la anterior), no se guarda para comparar; se usa \
                         en la revisión de la carpeta",
                        r.model, r.year, r.period
                    ));
                }
                if r.receipt.as_ref().is_some_and(|n| sustituidas.contains(n)) {
                    return Ok(format!(
                        "modelo {} {} {}: lo sustituye otra declaración de la carpeta \
                         (complementaria o sustitutiva), no se guarda",
                        r.model, r.year, r.period
                    ));
                }
                if !r.boxes_read || !crate::presentados::storable(&r.model) {
                    return Ok(format!(
                        "modelo {} {} {}: justificante de la AEAT; se usa en la revisión de la \
                         carpeta, no hace falta guardarlo",
                        r.model, r.year, r.period
                    ));
                }
            }
            let stored = crate::presentados::store_read(v, &name, &bytes, &r, false, now)?;
            let aviso = match stored {
                crate::presentados::Stored::Copy(_, c) => c.aviso(),
                _ => None,
            };
            Ok(format!(
                "modelo {} {} {} {} ({} casillas){}",
                r.model,
                r.year,
                r.period,
                match stored {
                    crate::presentados::Stored::New(_) => "guardado para comparar",
                    crate::presentados::Stored::Copy(
                        _,
                        crate::presentados::Copia::Sustituye { .. },
                    ) => "guardado para comparar en lugar de otra copia",
                    _ => "ya estaba guardado",
                },
                r.boxes.len(),
                aviso.map(|a| format!("; {a}")).unwrap_or_default()
            ))
        }
        Found::Ia { tipo, .. } => {
            let r = crate::lectura_ia::stored(v, &bytes)?
                .ok_or_else(|| AppError::Other("falta la lectura del asistente".into()))?;
            // Un diario de fechas que ya tenían asientos sólo llega aquí si el usuario lo ha
            // marcado sabiéndolo: entra, con el aviso.
            let note = if tipo == "diario" {
                ia_journal(v, &name, &bytes, &r)
                    .map(|p| {
                        let (overlap, imported, _) = journal_overlap(v, p);
                        overlap_note(overlap, imported)
                    })
                    .unwrap_or_default()
            } else {
                String::new()
            };
            Ok(format!(
                "{}{note}",
                crate::lectura_ia::apply_with_received(v, &name, &bytes, &r, own, received, now)?
            ))
        }
        Found::Documento { title, .. } => {
            // El reconocimiento de texto del Mac (Vision, hasta diez páginas dibujadas) sólo si
            // el PDF no se reconoce por su propio texto: esto corre con el almacén cogido.
            let by_text = || {
                ocr_text(&name, &bytes).and_then(|t| {
                    crate::documentos::classify(&t).map(|kind| crate::documentos::Recognized {
                        kind,
                        title: kind.title().to_owned(),
                        facts: crate::documentos::extract(&t, kind),
                        by_name: false,
                    })
                })
            };
            let d = crate::documentos::recognize(&name, &bytes)
                .filter(|d| !d.by_name)
                .or_else(by_text)
                .or_else(|| crate::documentos::recognize(&name, &bytes))
                .ok_or_else(|| AppError::Other("el documento ya no se reconoce".into()))?;
            crate::documentos::store(v, &name, &bytes, &d, now)?;
            Ok(format!("{title} guardado en Documentos de la empresa"))
        }
        Found::Cripto { .. } => {
            let p = crate::cripto::paquete::leer(v, &name, &bytes).map_err(AppError::Other)?;
            let (nuevas, dup, precios) = crate::cripto::paquete::aplicar(v, &p, now)?;
            let mut partes = Vec::new();
            if nuevas > 0 || precios == 0 {
                partes.push(format!(
                    "{nuevas} {} de cripto",
                    if nuevas == 1 {
                        "operación"
                    } else {
                        "operaciones"
                    }
                ));
            }
            if dup > 0 {
                partes.push(format!("{dup} ya estaban"));
            }
            if precios > 0 {
                partes.push(format!("{precios} precios de cierre"));
            }
            let mut msg = format!("{} a Criptomonedas", partes.join(", "));
            // Operaciones incompletas sin punto de partida: que lo ponga.
            if nuevas > 0
                && let Some(a) = crate::cripto::inicio::aviso_sin_inicio(v)?
            {
                msg.push_str(&format!("; {a}"));
            }
            // Los historiales de exchange que no se leen, uno a uno con su motivo; lo que no es
            // de cripto (extractos, PDF…) y las copias, contados.
            let (otros, motivos): (Vec<_>, Vec<_>) = p.no_leidos.iter().partition(|(_, m)| {
                m.starts_with("dentro de un zip")
                    || m.starts_with("no es un historial")
                    || m.starts_with("es el mismo fichero")
            });
            if !motivos.is_empty() {
                msg.push_str(&format!(
                    "; no se ha podido leer: {}",
                    motivos
                        .iter()
                        .map(|(r, m)| format!("{r} ({m})"))
                        .collect::<Vec<_>>()
                        .join("; ")
                ));
            }
            if !otros.is_empty() {
                msg.push_str(&format!(
                    "; {} del zip no {} de cripto o {} copia de otro",
                    if otros.len() == 1 {
                        "1 fichero".to_owned()
                    } else {
                        format!("{} ficheros", otros.len())
                    },
                    if otros.len() == 1 { "es" } else { "son" },
                    if otros.len() == 1 { "es" } else { "son" },
                ));
            }
            Ok(msg)
        }
        Found::Valores { .. } => {
            let pdf = crate::extractos::mime::pdf_dentro(&bytes);
            let datos = pdf.as_deref().unwrap_or(&bytes);
            let a = crate::valores::importar::importar_fichero(v, &name, datos, now)?;
            let mut msg = format!(
                "{} {} a Inversiones en valores",
                a.nuevas,
                if a.nuevas == 1 {
                    "operación"
                } else {
                    "operaciones"
                }
            );
            if a.duplicadas > 0 {
                msg.push_str(&format!(", {} ya estaban", a.duplicadas));
            }
            if a.sustituidas > 0 {
                msg.push_str(&format!(
                    ", {} leídas antes de las posiciones sustituidas",
                    a.sustituidas
                ));
            }
            if a.precios > 0 {
                msg.push_str(&format!(", {} precios de cierre", a.precios));
            }
            if a.cuentas_nuevas > 0 {
                msg.push_str(&format!(", {} cuenta de bróker nueva", a.cuentas_nuevas));
            }
            if let Some(aviso) = a.avisos.first() {
                msg.push_str(&format!("; ojo: {aviso}"));
            }
            Ok(msg)
        }
        Found::FacturaEmitida { .. } => {
            Ok("factura emitida: entra con el libro de emitidas del gestor".into())
        }
        Found::Otro { reason } => Ok(format!("no se importa: {reason}")),
    }
}

/// Importa los ficheros elegidos (rutas relativas de [`scan`]). Cada fichero va aparte: si uno
/// falla, los demás siguen.
pub fn apply(
    v: &Vault,
    root: &Path,
    paths: &[String],
    now: OffsetDateTime,
) -> Result<Vec<Outcome>> {
    apply_with(root, paths, || Ok(v), now)
}

/// ¿Es el justificante de la complementaria de una declaración informativa (347, 349, 190…)?
/// Sólo trae los registros que faltaban en la anterior: sus totales no son los del periodo.
fn complementaria_de_informativa(r: &crate::presentados::Read) -> bool {
    r.kind.as_deref() == Some("complementaria")
        && crate::presentado_aeat::INFORMATIVE.contains(&r.model.as_str())
}

/// Como [`apply`], cogiendo el almacén con `vault` fichero a fichero. Sólo se miran los ficheros
/// elegidos (no toda la carpeta otra vez); se importan en orden: primero lo que da contexto
/// (modelos, libros, diario) y después las facturas sueltas.
pub fn apply_with<G: Deref<Target = Vault>>(
    root: &Path,
    paths: &[String],
    vault: impl Fn() -> Result<G>,
    now: OffsetDateTime,
) -> Result<Vec<Outcome>> {
    if !root.is_dir() {
        return Err(AppError::Other(format!(
            "{} no es una carpeta",
            root.display()
        )));
    }
    let own = own_nif(&*vault()?);
    let mut chosen: Vec<(u8, &String)> = Vec::new();
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut issued_book = false;
    // Justificantes a los que complementa o sustituye otro de los que se importan: no se
    // guardan, para que lo presentado sea la declaración vigente. La complementaria de una
    // informativa sólo trae lo que faltaba: no sustituye a la anterior.
    let mut sustituidas = std::collections::BTreeSet::new();
    for p in paths {
        if !seen.insert(p) {
            continue;
        }
        match read_file(root, p) {
            Ok((name, bytes)) => {
                prewarm(&name, &bytes);
                let found = classify(&*vault()?, p, &name, &bytes, own.as_deref()).found;
                issued_book |= matches!(
                    &found,
                    Found::Libro {
                        book: BookKind::Emitidas,
                        ..
                    }
                ) || matches!(&found, Found::Ia { tipo, .. } if tipo == "libro_emitidas");
                if matches!(&found, Found::Modelo { source, .. } if source == "justificante")
                    && let Some(r) = crate::presentados::read(&bytes)
                    && !complementaria_de_informativa(&r)
                    && let Some(anterior) = r.previous_receipt
                {
                    sustituidas.insert(anterior);
                }
                chosen.push((found.order(), p));
            }
            Err(e) => out.push(Outcome {
                path: p.clone(),
                ok: false,
                message: e.to_string(),
            }),
        }
    }
    chosen.sort_by_key(|(order, _)| *order);
    let received = if issued_book {
        folder_received(root, &vault)?
    } else {
        Vec::new()
    };
    for (_, path) in chosen {
        let result = vault()
            .and_then(|v| apply_one(&v, root, path, own.as_deref(), &received, &sustituidas, now));
        out.push(match result {
            Ok(message) => Outcome {
                path: path.clone(),
                ok: true,
                message,
            },
            Err(e) => Outcome {
                path: path.clone(),
                ok: false,
                message: e.to_string(),
            },
        });
    }
    Ok(out)
}

/// Contenido de un fichero de la carpeta (comprueba que la ruta no salga de ella, tampoco por
/// un enlace).
pub fn read_file(root: &Path, rel: &str) -> Result<(String, Vec<u8>)> {
    let io = |e: std::io::Error| AppError::Other(format!("{rel}: {e}"));
    let canon = root.join(rel).canonicalize().map_err(io)?;
    if !canon.starts_with(root.canonicalize().map_err(io)?) {
        return Err(AppError::Other("ruta fuera de la carpeta".into()));
    }
    let meta = std::fs::metadata(&canon).map_err(io)?;
    if !meta.is_file() || meta.len() > MAX_SIZE {
        return Err(AppError::Other(format!(
            "{rel}: no es un fichero o es demasiado grande"
        )));
    }
    let name = canon
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok((name, std::fs::read(&canon).map_err(io)?))
}

// ---------- Reconocimiento de texto local (fotos y escaneados) ----------

/// Lector de texto de imágenes y PDF escaneados (en el Mac, Vision). Lo instala la app al
/// arrancar; en otras plataformas no hay y esos ficheros se reconocen sólo por el nombre.
pub type OcrFn = dyn Fn(&str, &[u8]) -> Option<String> + Send + Sync;

static OCR: std::sync::OnceLock<Box<OcrFn>> = std::sync::OnceLock::new();
static OCR_CACHE: std::sync::Mutex<Option<std::collections::HashMap<String, Option<String>>>> =
    std::sync::Mutex::new(None);

pub fn set_ocr(f: Box<OcrFn>) {
    let _ = OCR.set(f);
}

/// Texto leído en el Mac (con caché por huella: leer la carpeta y luego importar no lo repite).
pub fn ocr_text(name: &str, bytes: &[u8]) -> Option<String> {
    let f = OCR.get()?;
    let key = hex::encode(Sha256::digest(bytes));
    if let Ok(mut c) = OCR_CACHE.lock()
        && let Some(hit) = c.get_or_insert_with(Default::default).get(&key)
    {
        return hit.clone();
    }
    let text = f(name, bytes).filter(|t| t.trim().len() > 20);
    if let Ok(mut c) = OCR_CACHE.lock() {
        let map = c.get_or_insert_with(Default::default);
        if map.len() > 2000 {
            map.clear();
        }
        map.insert(key, text.clone());
    }
    text
}

/// Lo que se reconoce en un texto leído en el Mac: documento de la empresa o factura.
fn from_ocr(v: &Vault, text: &str, own: Option<&str>) -> Option<Classified> {
    if let Some(kind) = crate::documentos::classify(text) {
        let facts = crate::documentos::extract(text, kind);
        return Some(
            document(crate::documentos::Recognized {
                kind,
                title: kind.title().to_owned(),
                facts,
                by_name: false,
            })
            .into(),
        );
    }
    let e = taxpain_invoicing::ingest::from_text(text, own);
    let nif = e.supplier_nif.as_ref().map(|f| f.value.to_uppercase());
    let number = e.number.as_ref().map(|f| f.value.clone());
    let date = e.issue_date.as_ref().map(|f| f.value);
    let total = e.total.as_ref().map(|f| f.value);
    if number.is_none() && total.is_none() {
        return None;
    }
    let mut warnings = e.warnings.clone();
    warnings.push("leída con el reconocimiento de texto del Mac: revisa los datos".into());
    if let (Some(n), Some(o)) = (&nif, own)
        && n == o
    {
        return Some(
            (
                Found::FacturaEmitida {
                    number,
                    date,
                    total,
                },
                warnings,
            )
                .into(),
        );
    }
    let r = ReceivedRef::from_extraction(&e);
    let (found, warnings) = received_found(&e, received_status(v, &r), warnings);
    Some(Classified {
        found,
        warnings,
        received: Some(r),
    })
}

const FOLDER_KEY: &str = "carpeta_empresa";

/// Carpeta de la empresa elegida la última vez.
pub fn folder(v: &Vault) -> Result<Option<String>> {
    Ok(v.setting(FOLDER_KEY)?)
}

pub fn set_folder(v: &Vault, path: &str) -> Result<()> {
    v.set_setting(FOLDER_KEY, &path.to_owned())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_periodo_de_la_ruta() {
        assert_eq!(
            route_quarters("FACTURAS/2017/T3/GASTOS/a.pdf"),
            vec![(2017, 2)]
        );
        assert_eq!(route_quarters("2019/3T/a.pdf"), vec![(2019, 2)]);
        assert_eq!(route_quarters("x/2020/Q1/a.pdf"), vec![(2020, 0)]);
        assert_eq!(route_quarters("2021/4º trimestre/a.pdf"), vec![(2021, 3)]);
        assert_eq!(route_quarters("2022/11/a.pdf"), vec![(2022, 3)]);
        assert_eq!(
            route_quarters("2023/GASTOS/a.pdf"),
            (0..4).map(|q| (2023, q)).collect::<Vec<_>>()
        );
        // El año en el nombre del fichero no cuenta, ni un número que no es un año.
        assert!(route_quarters("FACTURAS/2024.pdf").is_empty());
        assert!(route_quarters("1999/T1/a.pdf").is_empty());
        assert!(route_quarters("a.pdf").is_empty());
    }

    /// La carpeta se corta en el tope de ficheros admitidos y se cuenta lo que queda fuera
    /// (`over_limit`), para avisar en vez de callar; lo que no se admite (vacíos, otros
    /// formatos) no gasta cupo. Con un tope pequeño, para no crear 50.000 ficheros.
    #[test]
    fn el_recorrido_se_corta_en_el_tope_y_cuenta_lo_que_queda_fuera() {
        const TOPE: usize = 40;
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("2025");
        std::fs::create_dir(&sub).unwrap();
        for i in 0..(TOPE + 7) {
            // Un byte: un fichero vacío no se admite (no cuenta para el tope).
            std::fs::write(sub.join(format!("f{i:05}.txt")), b"x").unwrap();
        }
        std::fs::write(sub.join("vacio.txt"), b"").unwrap();
        std::fs::write(sub.join("otro.bin"), b"x").unwrap();
        let mut out = Vec::new();
        let mut skipped = Skipped::default();
        walk_max(dir.path(), &mut out, &mut skipped, 0, TOPE);
        assert_eq!(out.len(), TOPE);
        assert_eq!(skipped.over_limit, 7);
        assert_eq!(skipped.other, 2, "vacío y formato no admitido");
        assert_eq!(skipped.too_deep, 0);
        // En orden de nombre: los que quedan fuera son los últimos.
        assert!(out[0].ends_with("f00000.txt"));
        assert!(out[TOPE - 1].ends_with(format!("f{:05}.txt", TOPE - 1)));
    }
}
