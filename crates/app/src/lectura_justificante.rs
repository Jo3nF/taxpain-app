//! Casillas de una declaración presentada en la AEAT a partir de lo que da el cotejo por CSV
//! («Comprobar lo presentado»), cuando ya se sabe de qué modelo, ejercicio y periodo es:
//! - el **fichero** del visor de la declaración (formato de los diseños de registro): exacto.
//!   Las autoliquidaciones con `presentados::read_file`; las informativas (190, 193, 180, 347,
//!   349), con los totales de su registro de declarante;
//! - el **PDF** del justificante, para los años en que el visor no responde (declaraciones
//!   antiguas). En el formulario de la AEAT cada importe va en la misma línea que su número de
//!   casilla, justo a su derecha («28 1.000,00 29 210,00»), pero el texto plano del PDF los
//!   separa: se leen las posiciones y cada importe se asigna a la casilla más cercana a su
//!   izquierda en la misma línea. El 200, con casillas de cinco cifras pegadas al importe, se
//!   lee del texto (`revision::expediente::modelo_200_de_texto`).
//!
//! Lo del PDF es aproximado (como toda lectura de PDF): sólo se aceptan casillas de importe que
//! existen en el modelo, y las que no cuadran entre sí (`plausible`) se quedan sin leer.
//!
//! [`leer_justificante`] lee además la portada del justificante («Información de la
//! presentación de la declaración») sin saber de antemano qué es: modelo, ejercicio, periodo,
//! fecha de presentación, número de justificante, CSV y si complementa o sustituye a otra. Es lo
//! que usa la revisión de la carpeta con los justificantes que guarda la empresa.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{LazyLock, Mutex};

use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use taxpain_core::money::Money;
use time::{Date, Month};

use crate::extractos::pdf::generico::maqueta;
use crate::presentados;

/// Casillas leídas de una declaración (lo que se guarda en `aeat_filing.boxes`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilingBoxes {
    /// Casilla (sin ceros a la izquierda: [00500] es 500) → importe.
    #[serde(default)]
    pub casillas: BTreeMap<u32, Money>,
    /// Informativas: número de registros de tipo 2 (perceptores, declarados u operadores).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registros: Option<u32>,
    /// Informativas: importe total (percepciones en el 190, base en el 193 y el 180, operaciones
    /// en el 347 y el 349).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importe: Option<Money>,
    /// Informativas: retenciones e ingresos a cuenta (190, 193, 180).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retenciones: Option<Money>,
    /// 347 y 349: una línea por tercero u operador (para cruzarlos por NIF).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lineas: Vec<Linea>,
    /// Casillas que no se han podido leer bien del PDF (las de la liquidación del 200 cuando no
    /// cuadran entre sí): no están en `casillas` y no se sabe cuánto valen, que no es cero.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub dudosas: BTreeSet<u32>,
}

/// Un tercero (347) u operador intracomunitario (349) declarado.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Linea {
    pub nif: String,
    pub nombre: String,
    /// Clave de la operación («A», «B»; «E», «A», «S», «I»…).
    pub clave: String,
    pub importe: Money,
}

impl FilingBoxes {
    /// Importe de una casilla (cero si no está).
    pub fn get(&self, casilla: u32) -> Money {
        self.casillas.get(&casilla).copied().unwrap_or(Money::ZERO)
    }

    /// ¿Hay algo leído?
    pub fn is_empty(&self) -> bool {
        self.casillas.is_empty() && self.registros.is_none() && self.importe.is_none()
    }
}

/// De dónde salen las casillas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    /// Fichero de una autoliquidación.
    File,
    /// Fichero de una declaración informativa (totales del declarante).
    Informative,
    /// Texto o posiciones del PDF del justificante.
    Pdf,
}

impl Source {
    pub fn code(self) -> &'static str {
        match self {
            Source::File => "fichero",
            Source::Informative => "informativa",
            Source::Pdf => "pdf",
        }
    }
}

/// Casillas del fichero de la declaración del visor, si es de `model` (y su ejercicio y
/// periodo). Las autoliquidaciones que TaxPain sabe leer y las informativas.
pub fn from_file(
    file: &str,
    model: &str,
    year: i32,
    period: &str,
) -> Option<(FilingBoxes, Source)> {
    if let Some(r) = presentados::read_file(file)
        && r.model == model
        && r.year == year
        && r.period.eq_ignore_ascii_case(period)
    {
        let casillas = r.boxes.iter().map(|(k, v)| (u32::from(*k), *v)).collect();
        return Some((
            FilingBoxes {
                casillas,
                ..FilingBoxes::default()
            },
            Source::File,
        ));
    }
    let s = taxpain_models::informative::read_summary(file)?;
    if s.model != model || s.year != year {
        return None;
    }
    let lineas = if matches!(model, "347" | "349") {
        taxpain_models::informative::read_file(file)
            .map(|f| {
                f.lines
                    .into_iter()
                    .map(|l| Linea {
                        nif: l.nif,
                        nombre: l.name,
                        clave: l.key,
                        importe: l.amount,
                    })
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    Some((
        FilingBoxes {
            registros: Some(s.records),
            importe: Some(s.total),
            retenciones: s.withheld,
            lineas,
            ..FilingBoxes::default()
        },
        Source::Informative,
    ))
}

/// Casillas del fichero de un 200 del visor, con el diseño de registro de su ejercicio (el 200
/// cambia cada año: TaxPain tiene los de 2017 a 2025, `dr_versions`). Comprobado con declaraciones
/// reales: cada página mide lo que dice su diseño y el balance cuadra. `None` si
/// no es un 200 de ese ejercicio, si TaxPain no tiene su diseño o si el fichero no casa con él.
/// `TODO(VERIFY)`: el diseño de 2026 cuando se publique (`tools/aeat/dr_actualizar.py`).
pub fn from_file_200(file: &str, year: i32) -> Option<FilingBoxes> {
    let (m, y, _) = taxpain_models::dr::read_header(file)?;
    if m != "200" || y != year {
        return None;
    }
    let casillas: BTreeMap<u32, Money> = taxpain_models::m200::read_boxes_of_year(file, year)?
        .into_iter()
        .map(|(k, v)| (u32::from(k), v))
        .collect();
    let b = FilingBoxes {
        casillas,
        ..FilingBoxes::default()
    };
    (!b.casillas.is_empty() && plausible("200", &b)).then_some(b)
}

/// ¿Es el fichero de un ejercicio cuyo diseño de registro no tiene TaxPain? (Hay que actualizar
/// TaxPain; mientras, las casillas salen del PDF.)
pub fn unknown_design(file: &str, model: &str, year: i32) -> bool {
    if model == "200" {
        return taxpain_models::m200::read_boxes_of_year(file, year).is_none();
    }
    matches!(
        presentados::read_file_checked(file),
        Err(presentados::FileError::UnknownDesign { .. }
            | presentados::FileError::Incoherent { .. })
    )
}

/// ¿Es coherente lo leído? El fichero se lee con el diseño de registro vigente, y el de algunos
/// años anteriores tenía otras posiciones: el 303 de 2025 leído con el diseño de 2026 da casillas
/// cambiadas. En el 303, el resultado del régimen general [46] es la cuota devengada [27] menos
/// la deducible [45] (art. 71 RIVA, «Resultado régimen general (27 - 45)» del formulario).
/// Además, [27] es la suma de las cuotas devengadas y [45] la de las deducibles (las fórmulas
/// impresas en el formulario). En el 200, el balance: total activo [00180] = total patrimonio
/// neto y pasivo [00252].
pub fn plausible(model: &str, b: &FilingBoxes) -> bool {
    let sum = |boxes: &[u32]| boxes.iter().map(|k| b.get(*k)).sum::<Money>();
    match model {
        "303" => {
            b.get(46) == b.get(27) - b.get(45)
                && b.get(27) == sum(&[152, 167, 3, 155, 6, 9, 11, 13, 15, 158, 170, 18, 21, 24, 26])
                && b.get(45) == sum(&[29, 31, 33, 35, 37, 39, 41, 42, 43, 44])
        }
        "200" => b.get(180) == b.get(252),
        _ => true,
    }
}

/// ¿Es coherente lo leído del PDF? Del PDF se puede perder o confundir una casilla, así que
/// además de [`plausible`] se comprueban más fórmulas que imprime el formulario:
/// - 390: «Resultado régimen general (47 - 64)» [65], «Suma de resultados (65 + 83 + 658)»
///   [84] y «Resultado de la liquidación (84 + 659 - 85)» [86] (las mismas en los formularios
///   de 2017 a 2025). Si no, un tipo del recargo de equivalencia leído como la [65] pasaría.
/// - Informativas: tienen que traer su importe total.
///
/// La liquidación del 200 se comprueba aparte (`liquidacion_200`): si no cuadra, sólo sus
/// casillas se quedan sin leer.
pub fn plausible_pdf(model: &str, b: &FilingBoxes) -> bool {
    let sum = |boxes: &[u32]| boxes.iter().map(|k| b.get(*k)).sum::<Money>();
    plausible(model, b)
        && match model {
            "390" => {
                b.get(65) == b.get(47) - b.get(64)
                    && b.get(84) == sum(&[65, 83, 658])
                    && b.get(86) == b.get(84) + b.get(659) - b.get(85)
            }
            "180" | "190" | "193" | "347" | "349" => b.importe.is_some() || !b.casillas.is_empty(),
            _ => true,
        }
}

/// Casillas de la liquidación del 200 que se comprueban juntas: cuota del ejercicio [00599],
/// pagos fraccionados [00601], [00603] y [00605], cuota diferencial [00611], incremento por
/// pérdida de beneficios fiscales [00615], intereses de demora [00617], resultado de la
/// autoliquidación [01586] y líquido a ingresar o devolver [00621].
const LIQUIDACION_200: [u32; 9] = [599, 601, 603, 605, 611, 615, 617, 1586, 621];

/// En un 200 leído del PDF, la liquidación tiene que cuadrar: «Cuota diferencial» [00611] =
/// [00599] − [00601] − [00603] − [00605] y, si no se leyó la [00611], el resultado [01586] = esa
/// cuota diferencial + [00615] + [00617]. En los PDF de algunos años se pierde un pago
/// fraccionado o la cuota (las letras de la página se montan unas sobre otras): si no cuadra,
/// esas casillas pasan a `dudosas` y no se usan (una que falta no es cero).
fn liquidacion_200(b: &mut FilingBoxes) {
    let pagos = b.get(599) - b.get(601) - b.get(603) - b.get(605);
    let cuadra = match (b.casillas.get(&611), b.casillas.get(&1586)) {
        (Some(d), _) => *d == pagos,
        (None, Some(r)) => *r - b.get(615) - b.get(617) == pagos,
        // Sin resultado: cuadra si no hay nada (un 200 sin cuota ni pagos).
        (None, None) => pagos.is_zero() && !b.casillas.contains_key(&621),
    };
    if !cuadra {
        for c in LIQUIDACION_200 {
            b.casillas.remove(&c);
            b.dudosas.insert(c);
        }
    }
}

/// Casillas del PDF del justificante de `model`, `year` y `period` (las casillas que se aceptan
/// son las del diseño de registro de ese ejercicio).
pub fn from_pdf(model: &str, year: i32, period: &str, pdf: &[u8]) -> Option<FilingBoxes> {
    if model == "200" {
        let letras = maqueta::letras(pdf)?;
        let casillas = casillas_200(&maqueta::lineas(&letras));
        // Un 200 de verdad tiene decenas de casillas; con pocas, es otra cosa.
        if casillas.len() < 15 {
            return None;
        }
        let mut b = FilingBoxes {
            casillas,
            ..FilingBoxes::default()
        };
        liquidacion_200(&mut b);
        return Some(b);
    }
    if matches!(model, "180" | "190" | "193" | "347" | "349") {
        let letras = maqueta::letras(pdf)?;
        let lineas = maqueta::lineas(&letras);
        let mut b = informativa_de_lineas(model, &lineas)?;
        if model == "349" {
            b.lineas = operadores_349(&lineas, &b);
        } else if model == "347" {
            b.lineas = declarados_347(&lineas, &b);
        }
        return Some(b);
    }
    let known: Vec<u32> = presentados::known_boxes_for(model, year, period)
        .into_iter()
        .map(u32::from)
        .collect();
    if known.is_empty() {
        return None;
    }
    let letras = maqueta::letras(pdf)?;
    let lineas = maqueta::lineas(&letras);
    let casillas = from_lines(&lineas, &known);
    (!casillas.is_empty() || a_cero(&lineas)).then(|| FilingBoxes {
        casillas,
        ..FilingBoxes::default()
    })
}

/// ¿Es el justificante de una declaración negativa, sin actividad o a cero? («NEGATIVA SIN
/// ACTIVIDAD / RESULTADO CERO» en la primera página.) No tiene casillas con importe, pero está
/// leída.
fn a_cero(lineas: &[maqueta::Linea]) -> bool {
    lineas.iter().any(|l| {
        let t = l.texto().to_uppercase();
        t.contains("SIN ACTIVIDAD") || t.contains("RESULTADO CERO")
    })
}

/// Totales de la portada de una declaración informativa en PDF (las de 2017-2018, que el visor
/// de la AEAT no tiene). Cada total va al final de su línea tras su casilla:
/// «Número total de percepciones … 01 2», «Importe total de las percepciones … 02 12.000,50».
/// - 190, 193 y 180: [01] número de registros, [02] importe (percepciones o base), [03]
///   retenciones.
/// - 347: [01] número de declarados, [02] importe anual de las operaciones.
/// - 349: [01] número de operadores, [02] importe de las operaciones.
///
/// Vale la primera línea con cada casilla. En algunos años los cuatro totales van en la misma
/// línea («01 1 02 1.000,00 03 190,00 04 190,00»): se miran todos los pares de la línea.
/// `None` si no aparece el importe [02].
pub(crate) fn informativa_de_lineas(model: &str, lines: &[maqueta::Linea]) -> Option<FilingBoxes> {
    let mut vals: BTreeMap<u8, String> = BTreeMap::new();
    for l in lines {
        let t = l.texto();
        let words: Vec<&str> = t.split_whitespace().collect();
        for par in words.windows(2) {
            let (label, value) = (par[0], par[1]);
            let casilla = match label {
                "01" => 1u8,
                "02" => 2,
                "03" => 3,
                "04" => 4,
                _ => continue,
            };
            // El [01] es un número de registros y los demás, importes (el [03] y el [04] del
            // 349 son otro número y otro importe: el número no se toma como importe).
            let vale = if casilla == 1 {
                !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit() || b == b'.')
            } else {
                presentados::amount(value).is_some()
            };
            if vale {
                vals.entry(casilla).or_insert_with(|| value.to_owned());
            }
        }
    }
    let count = |c: u8| -> Option<u32> {
        vals.get(&c)
            .and_then(|v| v.replace('.', "").parse::<u32>().ok())
    };
    let money = |c: u8| -> Option<Money> { vals.get(&c).and_then(|v| presentados::amount(v)) };
    let importe = money(2)?;
    let retenciones = matches!(model, "180" | "190" | "193")
        .then(|| money(3))
        .flatten();
    Some(FilingBoxes {
        registros: count(1),
        importe: Some(importe),
        retenciones,
        ..FilingBoxes::default()
    })
}

/// Puntos de la línea de guía del formulario.
const GUIA: [char; 3] = ['.', '…', '·'];

/// Número de casilla de `cifras` cifras (o entre dos y tres si es `None`), sin nada más. Los
/// puntos de la línea de guía pueden ir pegados delante o detrás («…………61», «00355……») y, si
/// los hay, también entre las cifras («……2.9» es la [29]: los puntos se montan sobre el número).
fn etiqueta(w: &str, cifras: Option<usize>) -> Option<u32> {
    let limpio = w.trim_matches(GUIA);
    let con_guia = limpio.len() != w.len();
    let n: String = if con_guia {
        limpio.chars().filter(|c| !GUIA.contains(c)).collect()
    } else {
        limpio.to_owned()
    };
    let largo = cifras.map_or((2..=3).contains(&n.len()), |c| n.len() == c);
    (largo && n.bytes().all(|b| b.is_ascii_digit()))
        .then(|| n.parse().ok())
        .flatten()
}

/// ¿Es un número de casilla de dos o tres cifras?
fn box_label(w: &str) -> Option<u32> {
    etiqueta(w, None)
}

/// Cada importe con la casilla más cercana a su izquierda. Lo normal es que vayan en la misma
/// línea, pero en los formularios antiguos (2017-2018) el importe sale a veces un poco más abajo
/// que su número de casilla y el texto lo pone en la línea siguiente («28 … 29» y debajo
/// «1.234,56 259,26»): se buscan casillas en una franja vertical de 1,6 veces el tamaño de la
/// letra (6 puntos como mínimo) y se elige la que está más cerca, contando la distancia vertical
/// cuatro veces más que la horizontal, para que gane la de la misma línea. Sólo valen las
/// casillas de importe del modelo (`known`): un porcentaje junto a la casilla del tipo
/// («05 10,00») no se toma. La primera vez que sale una casilla es la que vale.
pub(crate) fn from_lines(lines: &[maqueta::Linea], known: &[u32]) -> BTreeMap<u32, Money> {
    asignar(lines, box_label, |b| known.contains(&b))
}

/// Casillas del PDF de un 200: números de casilla de cinco cifras («00255») y el importe a su
/// derecha. En el 200 un mismo número sale en varias páginas con otro sentido (la [00562] es la
/// cuota íntegra en la liquidación y la distribución de dividendos en el estado de cambios del
/// patrimonio neto; la [00325], el resultado antes de impuestos y un ajuste por deterioro): se
/// leen primero las páginas de la liquidación (las que el formulario numera 12 a 15) y luego las
/// demás, y la primera vez que sale una casilla es la que vale (como `m200::read_boxes`).
pub(crate) fn casillas_200(lines: &[maqueta::Linea]) -> BTreeMap<u32, Money> {
    let mut numero: BTreeMap<u32, u32> = BTreeMap::new();
    for l in lines {
        let t = l.texto();
        if let Some(i) = t.find("Página ") {
            let n: String = t[i + "Página ".len()..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if let Ok(n) = n.parse::<u32>() {
                numero.entry(l.pagina).or_insert(n);
            }
        }
    }
    let liquidacion =
        |l: &maqueta::Linea| numero.get(&l.pagina).is_some_and(|n| (12..=15).contains(n));
    let ordenadas: Vec<maqueta::Linea> = lines
        .iter()
        .filter(|l| liquidacion(l))
        .chain(lines.iter().filter(|l| !liquidacion(l)))
        .cloned()
        .collect();
    asignar(&ordenadas, |w| etiqueta(w, Some(5)), |_| true)
}

/// Un trozo de una palabra del formulario: un número de casilla o un importe, con su sitio.
struct Pieza {
    pagina: u32,
    y: i32,
    tam: i32,
    x0: i32,
    x1: i32,
    casilla: Option<u32>,
    importe: Option<Money>,
}

/// Importe pegado al número de la casilla siguiente («4.100,0008»: 4.100,00 y la [08];
/// «1.234,5600604»: 1.234,56 y la [00604]).
static PEGADO: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^(-?\d{1,3}(?:\.\d{3})*,\d{2})(\d{2,5})$").ok());

/// Las piezas de una palabra: una casilla, un importe o las dos cosas si van pegadas (el
/// importe y la casilla siguiente, o una casilla del 200 con su importe metido entre los puntos
/// de la línea de guía: «01815……3..1..5,.2..0.» es la [01815] con 315,20).
fn piezas(
    l: &maqueta::Linea,
    p: &maqueta::Palabra,
    label: &impl Fn(&str) -> Option<u32>,
) -> Vec<Pieza> {
    let base = |x0: i32, x1: i32, casilla: Option<u32>, importe: Option<Money>| Pieza {
        pagina: l.pagina,
        y: l.y,
        tam: l.tam,
        x0,
        x1,
        casilla,
        importe,
    };
    let t = p.texto.as_str();
    if let Some(m) = presentados::amount(t) {
        return vec![base(p.x0, p.x1, None, Some(m))];
    }
    if let Some(b) = label(t) {
        return vec![base(p.x0, p.x1, Some(b), None)];
    }
    // Dónde se parte la palabra, por el número de letras.
    let corte = |letras: usize| {
        let total = i32::try_from(t.chars().count()).unwrap_or(1).max(1);
        let n = i32::try_from(letras).unwrap_or(0);
        p.x0 + (p.x1 - p.x0) * n / total
    };
    if let Some(c) = PEGADO.as_ref().and_then(|re| re.captures(t)) {
        let (importe, resto) = (&c[1], &c[2]);
        // Sólo si lo que sigue es un número de casilla (las del 200 empiezan por 0). Puede ser
        // la del tipo («3.000,0008»: la [07] y la [08], el tipo del 21 %), que no es de importe.
        if let (Some(m), Some(b)) = (presentados::amount(importe), label(resto))
            && (resto.len() != 5 || resto.starts_with('0'))
        {
            let x = corte(importe.chars().count());
            return vec![base(p.x0, x, None, Some(m)), base(x, p.x1, Some(b), None)];
        }
    }
    // Casilla de cinco cifras seguida de la guía con el importe dentro.
    if t.len() > 5
        && t.is_char_boundary(5)
        && t[..5].bytes().all(|b| b.is_ascii_digit())
        && t[5..].starts_with(GUIA)
        && let Some(b) = label(&t[..5])
    {
        let valor: String = t[5..].chars().filter(|c| !GUIA.contains(c)).collect();
        if let Some(m) = presentados::amount(&valor) {
            let x = corte(5);
            return vec![base(p.x0, x, Some(b), None), base(x, p.x1, None, Some(m))];
        }
        return vec![base(p.x0, p.x1, Some(b), None)];
    }
    Vec::new()
}

/// Importes que caen en la columna «Tipo %» de una tabla del formulario (los tipos impresos,
/// como el 0,62 del recargo de equivalencia, no son importes de la casilla de al lado). La
/// columna empieza en la cabecera «Tipo %» y sigue, también en las páginas siguientes, hasta la
/// cabecera de otra tabla («Base…» o «Cuota…» sin «Tipo»).
fn en_columna_de_tipo(lines: &[maqueta::Linea]) -> Vec<(u32, i32, i32, i32)> {
    let mut fuera = Vec::new();
    let mut columna: Option<(i32, i32)> = None;
    for l in lines {
        let textos: Vec<String> = l.trozos.iter().map(|t| plano(&t.texto)).collect();
        if let Some(t) = l
            .trozos
            .iter()
            .zip(&textos)
            .find(|(_, p)| p.starts_with("tipo %") || p.starts_with("tipo%"))
            .map(|(t, _)| t)
        {
            columna = Some((t.x0, t.x1));
            continue;
        }
        if textos
            .iter()
            .any(|p| p == "base" || p.starts_with("base imponible") || p.starts_with("cuota"))
        {
            columna = None;
            continue;
        }
        let Some((c0, c1)) = columna else { continue };
        for p in l.trozos.iter().flat_map(|t| t.palabras.iter()) {
            let centro = i32::midpoint(p.x0, p.x1);
            if presentados::amount(&p.texto).is_some() && c0 - 50 <= centro && centro <= c1 + 50 {
                fuera.push((l.pagina, l.y, p.x0, p.x1));
            }
        }
    }
    fuera
}

/// Cada importe con su casilla (ver [`from_lines`]): `label` reconoce un número de casilla y `ok`
/// dice si es de importe en el modelo.
fn asignar(
    lines: &[maqueta::Linea],
    label: impl Fn(&str) -> Option<u32>,
    ok: impl Fn(u32) -> bool,
) -> BTreeMap<u32, Money> {
    let tipos = en_columna_de_tipo(lines);
    let todas: Vec<Pieza> = lines
        .iter()
        .flat_map(|l| {
            l.trozos
                .iter()
                .flat_map(|t| t.palabras.iter())
                .flat_map(|p| piezas(l, p, &label))
                .collect::<Vec<_>>()
        })
        .collect();
    let labels: Vec<(&Pieza, u32)> = todas
        .iter()
        .filter_map(|w| w.casilla.map(|b| (w, b)))
        .collect();
    let mut out = BTreeMap::new();
    for a in &todas {
        let Some(m) = a.importe else {
            continue;
        };
        if tipos
            .iter()
            .any(|(pg, y, x0, x1)| *pg == a.pagina && *y == a.y && *x0 <= a.x0 && a.x1 <= *x1)
        {
            continue;
        }
        let franja = (a.tam * 16 / 10).max(60);
        let found = labels
            .iter()
            .filter(|(l, _)| l.pagina == a.pagina && l.x1 <= a.x0 + 10)
            .filter(|(l, _)| (l.y - a.y).abs() <= franja)
            .min_by_key(|(l, _)| (a.x0 - l.x1) + 4 * (l.y - a.y).abs())
            .map(|(_, b)| *b);
        if let Some(b) = found
            && ok(b)
            && !m.is_zero()
        {
            out.entry(b).or_insert(m);
        }
    }
    out
}

/// Los operadores del 349 en PDF: en cada «Operador N» del formulario, la cabecera «Código
/// país · NIF comunitario · Apellidos y nombre… · Clave · Base imponible» y debajo sus datos.
/// Sólo valen si son tantos como dice el resumen [01] y suman su importe [02] (si no, se ha
/// leído mal alguno y no se usa ninguno). Las rectificaciones de periodos anteriores no se leen.
pub(crate) fn operadores_349(lines: &[maqueta::Linea], resumen: &FilingBoxes) -> Vec<Linea> {
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let col = |que: &str| {
            l.trozos
                .iter()
                .find(|t| plano(&t.texto).starts_with(que))
                .map(|t| t.x0)
        };
        let (Some(pais), Some(nif), Some(nombre), Some(clave), Some(base)) = (
            col("codigo pais"),
            col("nif comunitario"),
            col("apellidos y nombre"),
            col("clave"),
            col("base imponible"),
        ) else {
            continue;
        };
        if plano(&l.texto()).contains("rectific") {
            continue;
        }
        // Los datos: la línea de debajo, antes de la cabecera del operador sustituto (que no
        // tiene «Clave»).
        let Some(datos) = lines[i + 1..]
            .iter()
            .take_while(|d| d.pagina == l.pagina && d.y - l.y <= 250)
            .find(|d| !plano(&d.texto()).starts_with("a cumplimentar"))
        else {
            continue;
        };
        let palabras: Vec<&maqueta::Palabra> = datos
            .trozos
            .iter()
            .flat_map(|t| t.palabras.iter())
            .collect();
        let entre = |desde: i32, hasta: i32| -> String {
            palabras
                .iter()
                .filter(|p| p.x0 >= desde - 100 && p.x0 < hasta - 50)
                .map(|p| p.texto.as_str())
                .collect::<Vec<_>>()
                .join(" ")
        };
        let codigo = entre(pais, nif);
        let numero = entre(nif, nombre);
        let razon = entre(nombre, clave);
        let k = entre(clave, base);
        let importe = palabras
            .iter()
            .rev()
            .find(|p| p.x0 >= clave)
            .and_then(|p| presentados::amount(&p.texto));
        let (Some(importe), true, true, true) = (
            importe,
            codigo.len() == 2 && codigo.bytes().all(|b| b.is_ascii_uppercase()),
            !numero.is_empty() && !numero.contains(' '),
            k.len() == 1 && k.bytes().all(|b| b.is_ascii_uppercase()),
        ) else {
            continue;
        };
        out.push(Linea {
            nif: format!("{codigo}{numero}"),
            nombre: razon,
            clave: k,
            importe,
        });
    }
    let suma: Money = out.iter().map(|l| l.importe).sum();
    let cuadra = !out.is_empty()
        && resumen.importe == Some(suma)
        && resumen
            .registros
            .is_none_or(|n| usize::try_from(n).is_ok_and(|n| n == out.len()));
    if cuadra { out } else { Vec::new() }
}

/// Los declarados del 347 en PDF: en cada «Declarado N» del formulario, el NIF (bajo «NIF
/// declarado») y el nombre, la clave de la operación (bajo «Clave operación») y el importe anual
/// (bajo «Importe anual de las operaciones»). Como en el 349, sólo valen si son tantos como dice
/// el resumen [01] y suman su importe [02].
pub(crate) fn declarados_347(lines: &[maqueta::Linea], resumen: &FilingBoxes) -> Vec<Linea> {
    fn palabras(l: &maqueta::Linea) -> Vec<&maqueta::Palabra> {
        l.trozos.iter().flat_map(|t| t.palabras.iter()).collect()
    }
    let col = |l: &maqueta::Linea, que: &str| {
        l.trozos
            .iter()
            .position(|t| plano(&t.texto).starts_with(que))
    };
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let (Some(n), Some(a)) = (col(l, "nif declarado"), col(l, "apellidos y nombre")) else {
            continue;
        };
        let nif_x = l.trozos[n].x0;
        let nombre_x = l.trozos[a].x0;
        let siguiente_x = l.trozos.get(n + 1).map_or(nombre_x, |t| t.x0).min(nombre_x);
        // El bloque del declarado: hasta el siguiente «Declarado N» (o el final de la página).
        let bloque: Vec<&maqueta::Linea> = lines[i + 1..]
            .iter()
            .take_while(|d| d.pagina == l.pagina && !plano(&d.texto()).starts_with("declarado "))
            .collect();
        let Some(datos) = bloque.first() else {
            continue;
        };
        let ps = palabras(datos);
        let nif: String = ps
            .iter()
            .filter(|p| p.x0 >= nif_x - 100 && p.x0 < siguiente_x - 50)
            .map(|p| p.texto.as_str())
            .collect();
        let nombre = ps
            .iter()
            .filter(|p| p.x0 >= nombre_x - 100)
            .map(|p| p.texto.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        // La clave, en la línea de debajo de su cabecera.
        let clave = bloque.iter().enumerate().find_map(|(j, c)| {
            let k = col(c, "clave operacion")?;
            let x = c.trozos[k].x0;
            palabras(bloque.get(j + 1)?)
                .into_iter()
                .find(|p| {
                    p.x0 >= x - 100
                        && p.x0 < x + 600
                        && p.texto.len() == 1
                        && p.texto.bytes().all(|b| (b'A'..=b'G').contains(&b))
                })
                .map(|p| p.texto.clone())
        });
        // El importe anual: la primera cifra debajo de su cabecera, antes de la columna
        // siguiente («Importe anual percibido por transmisiones de inmuebles…»).
        let importe = bloque.iter().enumerate().find_map(|(j, c)| {
            let k = c
                .trozos
                .iter()
                .position(|t| plano(&t.texto).starts_with("importe anual de las operaciones"))?;
            let x0 = c.trozos[k].x0;
            let hasta = c.trozos.get(k + 1).map_or(i32::MAX, |t| t.x0 - 50);
            bloque[j + 1..].iter().take(3).find_map(|d| {
                palabras(d)
                    .into_iter()
                    .filter(|p| p.x0 >= x0 - 100 && p.x0 < hasta)
                    .find_map(|p| presentados::amount(&p.texto))
            })
        });
        let (Some(clave), Some(importe), true) = (
            clave,
            importe,
            !nif.is_empty() && nif.bytes().all(|b| b.is_ascii_alphanumeric()),
        ) else {
            continue;
        };
        out.push(Linea {
            nif,
            nombre,
            clave,
            importe,
        });
    }
    let suma: Money = out.iter().map(|l| l.importe).sum();
    let cuadra = !out.is_empty()
        && resumen.importe == Some(suma)
        && resumen
            .registros
            .is_none_or(|n| usize::try_from(n).is_ok_and(|n| n == out.len()));
    if cuadra { out } else { Vec::new() }
}

// ── El justificante de presentación ──────────────────────────────────────────────────────

/// Tipo de declaración según su justificante.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tipo {
    #[default]
    Normal,
    /// Complementa a otra anterior del mismo periodo. En las autoliquidaciones lleva el periodo
    /// entero y descuenta lo ingresado con la anterior; en las informativas, sólo lo que faltaba.
    Complementaria,
    /// Anula y sustituye a otra anterior del mismo periodo.
    Sustitutiva,
}

impl Tipo {
    pub fn code(self) -> &'static str {
        match self {
            Tipo::Normal => "normal",
            Tipo::Complementaria => "complementaria",
            Tipo::Sustitutiva => "sustitutiva",
        }
    }
}

/// Lo que dice el justificante de presentación de una declaración en la AEAT: la portada
/// («Información de la presentación de la declaración») y el formulario que va detrás.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Justificante {
    pub modelo: String,
    pub ejercicio: i32,
    /// «1T»…«4T», «01»…«12», «1P»…«3P» o «0A» (los modelos anuales, siempre).
    pub periodo: String,
    /// NIF del declarante (el primero que sale en el formulario).
    pub nif: Option<String>,
    /// Código seguro de verificación.
    pub csv: Option<String>,
    /// Número de justificante: trece cifras, las tres primeras son el modelo.
    pub numero: String,
    /// Número de registro asignado (expediente o referencia): empieza por el ejercicio.
    pub expediente: Option<String>,
    pub presentado: Option<Date>,
    /// Quién la presentó: «Titular», «Colaborador», «Representante»…
    pub en_calidad_de: Option<String>,
    /// NIF de quien la presentó (la empresa o su gestoría).
    pub presentador: Option<String>,
    pub tipo: Tipo,
    /// Número de justificante de la declaración a la que complementa o sustituye.
    pub anterior: Option<String>,
    /// La portada dice «Solicitud de devolución»: el resultado (negativo) se pidió a devolver.
    pub devolucion: bool,
    /// Casillas leídas y coherentes. `None` si no se han podido leer: no se sabe cuánto valen
    /// (no son cero).
    pub casillas: Option<FilingBoxes>,
}

/// Cabecera de la portada del justificante (en minúsculas, sin tildes).
const CABECERA: &str = "informacion de la presentacion de la declaracion";

/// Minúsculas, sin tildes y con los espacios seguidos en uno, para buscar etiquetas.
fn plano(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for w in s.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        for c in w.chars().flat_map(char::to_lowercase) {
            out.push(match c {
                'á' | 'à' | 'ä' => 'a',
                'é' | 'è' | 'ë' => 'e',
                'í' | 'ì' | 'ï' => 'i',
                'ó' | 'ò' | 'ö' => 'o',
                'ú' | 'ù' | 'ü' => 'u',
                c => c,
            });
        }
    }
    out
}

/// Espacios seguidos en uno (sin cambiar mayúsculas ni tildes).
fn espacios(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Modelos que son siempre anuales: su periodo es «0A» aunque el formulario no lo diga.
fn anual(modelo: &str) -> bool {
    matches!(
        modelo,
        "100"
            | "180"
            | "182"
            | "184"
            | "187"
            | "188"
            | "190"
            | "193"
            | "196"
            | "198"
            | "200"
            | "220"
            | "232"
            | "270"
            | "296"
            | "347"
            | "390"
            | "714"
            | "720"
            | "721"
    )
}

/// ¿Vale el periodo para el modelo? El 202 va por pagos (1P, 2P, 3P); los demás, por trimestres,
/// por meses o anual.
fn periodo_valido(modelo: &str, p: &str) -> bool {
    match p.as_bytes() {
        [b'1'..=b'3', b'P'] => modelo == "202",
        [b'1'..=b'4', b'T'] | [b'0', b'A'] | [b'0', b'1'..=b'9'] | [b'1', b'0'..=b'2'] => {
            modelo != "202"
        }
        _ => false,
    }
}

struct Rx {
    fecha: Regex,
    fecha_suelta: Regex,
    expediente: Regex,
    csv: Regex,
    csv_pie: Regex,
    numero: Regex,
    presentador: Regex,
    calidad: Regex,
    calidad_suelta: Regex,
    modelo: Regex,
}

fn rx() -> Option<&'static Rx> {
    static R: LazyLock<Option<Rx>> = LazyLock::new(|| {
        let r = |s: &str| Regex::new(s).ok();
        Some(Rx {
            fecha: r(r"(?i)presentaci[oó]n realizada el:? ?(\d{1,2})[-/.](\d{1,2})[-/.](\d{4})\b")?,
            fecha_suelta: r(r"\b(\d{2})-(\d{2})-(\d{4}) a las \d{2}[:.]\d{2}[:.]\d{2}\b")?,
            expediente: r(
                r"(?i)expediente/referencia \(n[ºo°.]* ?registro asignado\):? ?([0-9A-Z]{8,30})\b",
            )?,
            csv: r(r"(?i)c[oó]digo seguro de verificaci[oó]n:? ?([0-9A-Z]{16})\b")?,
            csv_pie: r(r"(?i)de verificaci[oó]n ([0-9A-Z]{16}) en https?://")?,
            numero: r(r"(?i)n[uú]mero de justificante:? ?(\d{13})\b")?,
            presentador: r(r"(?i)nif presentador:? ?([0-9A-Z]{9})\b")?,
            calidad: r(
                r"(?i)en calidad de:? ?(titular|colaborador|representante|apoderado|sucesor)\b",
            )?,
            calidad_suelta: r(r"(?i)^(titular|colaborador|representante|apoderado|sucesor)$")?,
            modelo: r(r"(?i)\bmodelo ?(\d{3})\b")?,
        })
    });
    R.as_ref()
}

/// Lee un justificante de presentación de la AEAT en PDF (sin saber de antemano de qué modelo
/// ni de qué periodo es). `None` si no es un justificante o si sus datos no casan entre sí: el
/// número de justificante empieza por el modelo y el expediente por el ejercicio. Las casillas,
/// con [`from_pdf`] y sólo si son coherentes ([`plausible_pdf`]); si no, quedan sin leer.
pub fn leer_justificante(pdf: &[u8]) -> Option<Justificante> {
    if !pdf.starts_with(b"%PDF") {
        return None;
    }
    // La revisión de la carpeta mira el mismo PDF varias veces (al clasificarlo y al revisar):
    // las últimas lecturas se guardan por huella.
    static RECIENTES: Mutex<VecDeque<(String, Option<Justificante>)>> = Mutex::new(VecDeque::new());
    let clave = hex::encode(Sha256::digest(pdf));
    if let Ok(c) = RECIENTES.lock()
        && let Some((_, j)) = c.iter().find(|(k, _)| *k == clave)
    {
        return j.clone();
    }
    let j = leer_sin_cache(pdf);
    if let Ok(mut c) = RECIENTES.lock() {
        if c.len() >= 64 {
            c.pop_front();
        }
        c.push_back((clave, j.clone()));
    }
    j
}

fn leer_sin_cache(pdf: &[u8]) -> Option<Justificante> {
    let texto = crate::pdf_text::text(pdf).unwrap_or_default();
    // Lo rápido primero: sin la cabecera de la portada no es un justificante.
    if !plano(&texto).contains(CABECERA) {
        return None;
    }
    let letras = maqueta::letras(pdf)?;
    let lineas = maqueta::lineas(&letras);
    let mut j = cabecera(&lineas, &texto)?;
    j.casillas =
        from_pdf(&j.modelo, j.ejercicio, &j.periodo, pdf).filter(|b| plausible_pdf(&j.modelo, b));
    Some(j)
}

/// Texto de una página de la maqueta (las líneas una detrás de otra).
fn texto_de_pagina(lineas: &[maqueta::Linea], pagina: u32) -> String {
    lineas
        .iter()
        .filter(|l| l.pagina == pagina)
        .map(maqueta::Linea::texto)
        .collect::<Vec<_>>()
        .join(" ")
}

/// La portada en el texto del PDF: de la cabecera al pie de la primera página («La autenticidad
/// de este documento…»), con los espacios seguidos en uno.
fn portada_del_texto(texto: &str) -> String {
    let t = espacios(texto);
    let p = t.to_lowercase();
    let desde = p.find("informaci").unwrap_or(0);
    let hasta = p[desde..]
        .find("la autenticidad de este documento")
        .map_or(t.len().min(desde + 3000), |i| desde + i);
    t.get(desde..hasta).unwrap_or_default().to_owned()
}

/// Lo que dice el justificante sin las casillas. La portada se lee de la maqueta (etiqueta y
/// valor en la misma línea) y, si no sale, del texto del PDF: en las portadas de algunos años
/// las letras de la etiqueta y del valor se montan unas sobre otras en la maqueta, y en otras
/// el texto da primero todas las etiquetas y luego todos los valores. Por eso cada dato se
/// busca por su etiqueta y, si no, por su forma («dd-mm-aaaa a las hh:mm:ss», trece cifras que
/// empiezan por el modelo, el expediente que empieza por el ejercicio y el modelo…). El
/// ejercicio, el periodo, el NIF y si es complementaria o sustitutiva salen del formulario (las
/// páginas siguientes), por posiciones.
pub(crate) fn cabecera(lineas: &[maqueta::Linea], texto: &str) -> Option<Justificante> {
    let rx = rx()?;
    let mut paginas: Vec<u32> = lineas.iter().map(|l| l.pagina).collect();
    paginas.dedup();
    let portada = paginas
        .iter()
        .copied()
        .find(|p| plano(&texto_de_pagina(lineas, *p)).contains(CABECERA))
        .or_else(|| {
            plano(texto)
                .contains(CABECERA)
                .then(|| paginas.first().copied())?
        })?;
    let del_texto = portada_del_texto(texto);
    let palabras: Vec<&str> = del_texto.split_whitespace().collect();
    // Dónde buscar los datos por su etiqueta: las líneas de la portada una a una y, después,
    // la portada del texto.
    let mut fuentes: Vec<String> = lineas
        .iter()
        .filter(|l| l.pagina == portada)
        .map(|l| espacios(&l.texto()))
        .collect();
    let de_la_maqueta = fuentes.len();
    fuentes.push(del_texto.clone());
    let buscar = |re: &Regex| -> Option<Vec<String>> {
        fuentes.iter().find_map(|f| {
            re.captures(f).map(|c| {
                c.iter()
                    .skip(1)
                    .map(|m| m.map(|m| m.as_str().to_owned()).unwrap_or_default())
                    .collect()
            })
        })
    };
    let primero = |re: &Regex| buscar(re).map(|mut c| c.swap_remove(0).to_uppercase());
    // El modelo: «Modelo 303» (o «MODELO303») en la portada o, si el texto pone las etiquetas
    // antes que los valores, el primer número de tres cifras suelto.
    let de_portada = fuentes[..de_la_maqueta]
        .iter()
        .chain(std::iter::once(&del_texto))
        .find_map(|f| rx.modelo.captures(f).map(|c| c[1].to_owned()))
        .or_else(|| {
            palabras
                .iter()
                .find(|w| w.len() == 3 && w.bytes().all(|b| b.is_ascii_digit()))
                .map(|w| (*w).to_owned())
        });
    let numero = primero(&rx.numero).or_else(|| {
        palabras
            .iter()
            .find(|w| {
                w.len() == 13
                    && w.bytes().all(|b| b.is_ascii_digit())
                    && de_portada.as_deref().is_none_or(|m| w.starts_with(m))
            })
            .map(|w| (*w).to_owned())
    })?;
    let modelo = numero.get(..3)?.to_owned();
    // El modelo de la portada tiene que ser el del número de justificante.
    if de_portada.is_some_and(|m| m != modelo) {
        return None;
    }
    let presentado = buscar(&rx.fecha)
        .or_else(|| buscar(&rx.fecha_suelta))
        .and_then(|c| {
            let m = Month::try_from(c[1].parse::<u8>().ok()?).ok()?;
            Date::from_calendar_date(c[2].parse().ok()?, m, c[0].parse().ok()?).ok()
        });
    // El expediente empieza por el ejercicio y el modelo («2025303…»).
    let expediente = primero(&rx.expediente).or_else(|| {
        palabras
            .iter()
            .find(|w| {
                (12..=30).contains(&w.len())
                    && w.get(4..7) == Some(modelo.as_str())
                    && es_año(w.get(..4).unwrap_or_default()).is_some()
                    && w.bytes().all(|b| b.is_ascii_alphanumeric())
            })
            .map(|w| w.to_uppercase())
    });
    // El CSV: por su etiqueta, por el pie de la página («… de Verificación XXXX en https://…»)
    // o por su forma (dieciséis letras y cifras).
    let csv = primero(&rx.csv)
        .or_else(|| primero(&rx.csv_pie))
        .or_else(|| {
            rx.csv_pie
                .captures(&espacios(texto))
                .map(|c| c[1].to_uppercase())
        })
        .or_else(|| {
            palabras
                .iter()
                .find(|w| {
                    w.len() == 16
                        && w.bytes()
                            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
                        && w.bytes().any(|b| b.is_ascii_digit())
                        && w.bytes().any(|b| b.is_ascii_uppercase())
                })
                .map(|w| (*w).to_owned())
        });
    let presentador = primero(&rx.presentador).or_else(|| {
        palabras
            .iter()
            .map(|w| w.trim_matches(|c: char| !c.is_ascii_alphanumeric()))
            .find(|w| matches!(crate::revision::expediente::nif_español(w), Some(Ok(_))))
            .map(str::to_uppercase)
    });
    let en_calidad_de = buscar(&rx.calidad)
        .map(|mut c| c.swap_remove(0))
        .or_else(|| {
            palabras
                .iter()
                .find(|w| rx.calidad_suelta.is_match(w))
                .map(|w| (*w).to_owned())
        })
        .map(|p| {
            let p = p.to_lowercase();
            let mut cs = p.chars();
            cs.next()
                .map(|a| a.to_uppercase().chain(cs).collect::<String>())
                .unwrap_or_default()
        });
    let formulario: Vec<&maqueta::Linea> = lineas.iter().filter(|l| l.pagina > portada).collect();
    let de_expediente = expediente.as_deref().and_then(|e| es_año(e.get(..4)?));
    let ejercicio = match (valor(&formulario, "ejercicio", es_año), de_expediente) {
        (Some(f), Some(e)) if f != e => return None,
        (Some(y), _) | (None, Some(y)) => y,
        (None, None) => return None,
    };
    let periodo = if anual(&modelo) {
        "0A".to_owned()
    } else {
        valor(&formulario, "periodo", |w| {
            let p = w.to_uppercase();
            periodo_valido(&modelo, &p).then_some(p)
        })?
    };
    let (tipo, anterior) = tipo_y_anterior(&formulario, &modelo, &numero);
    let devolucion = fuentes
        .iter()
        .any(|f| plano(f).contains("solicitud de devolucion"));
    Some(Justificante {
        nif: nif_del_formulario(&formulario),
        modelo,
        ejercicio,
        periodo,
        csv,
        numero,
        expediente,
        presentado,
        en_calidad_de,
        presentador,
        tipo,
        anterior,
        devolucion,
        casillas: None,
    })
}

/// Un ejercicio («2025»).
fn es_año(w: &str) -> Option<i32> {
    (w.len() == 4)
        .then(|| w.parse::<i32>().ok())
        .flatten()
        .filter(|y| (2000..=2100).contains(y))
}

/// Valor de una etiqueta del formulario («Ejercicio … 2025», «Período 1T»): la palabra más
/// cercana a su derecha o un poco más abajo que `ok` acepta (sin los puntos de la guía). La
/// primera etiqueta que tiene valor es la que vale (el 349 tiene un «Ejercicio» de título sin
/// valor antes del de verdad).
fn valor<T>(
    formulario: &[&maqueta::Linea],
    etiqueta: &str,
    ok: impl Fn(&str) -> Option<T>,
) -> Option<T> {
    struct W<'a> {
        pagina: u32,
        y: i32,
        x0: i32,
        x1: i32,
        texto: &'a str,
    }
    let palabras: Vec<W> = formulario
        .iter()
        .flat_map(|l| {
            l.trozos
                .iter()
                .flat_map(|t| t.palabras.iter())
                .map(move |p| W {
                    pagina: l.pagina,
                    y: l.y,
                    x0: p.x0,
                    x1: p.x1,
                    texto: p.texto.as_str(),
                })
        })
        .collect();
    let limpio = |w: &str| {
        w.trim_matches(|c: char| GUIA.contains(&c) || matches!(c, ':' | '(' | ')' | '*'))
            .to_owned()
    };
    for (i, e) in palabras.iter().enumerate() {
        if plano(&limpio(e.texto)) != etiqueta {
            continue;
        }
        let mut cerca: Vec<&W> = palabras
            .iter()
            .enumerate()
            .filter(|(j, w)| *j != i && w.pagina == e.pagina && w.x0 >= e.x1 - 20)
            .filter(|(_, w)| (-40..=120).contains(&(w.y - e.y)))
            .map(|(_, w)| w)
            .collect();
        cerca.sort_by_key(|w| (w.x0 - e.x1).max(0) + 4 * (w.y - e.y).abs());
        if let Some(v) = cerca.iter().find_map(|w| ok(&limpio(w.texto))) {
            return Some(v);
        }
    }
    None
}

/// NIF del declarante, de la primera página del formulario: el de la identificación, junto a
/// su etiqueta «NIF» (detrás, en la misma línea, o debajo, a su altura); si no se encuentra, el
/// primero válido. Un sello de la gestoría encima del formulario («ASESORES DEMO SL · NIF … ·
/// Copia para el cliente») no es la identificación.
fn nif_del_formulario(formulario: &[&maqueta::Linea]) -> Option<String> {
    let primera = formulario.first()?.pagina;
    let lineas: Vec<&maqueta::Linea> = formulario
        .iter()
        .copied()
        .filter(|l| l.pagina == primera)
        .collect();
    let valido = |p: &maqueta::Palabra| {
        let n = p
            .texto
            .trim_matches(|c: char| !c.is_ascii_alphanumeric())
            .to_uppercase();
        matches!(crate::revision::expediente::nif_español(&n), Some(Ok(_))).then_some(n)
    };
    let etiqueta = |t: &str| matches!(t.trim().trim_end_matches(':'), "NIF" | "N.I.F." | "N.I.F");
    // La etiqueta sola en su trozo, con el NIF detrás o debajo.
    for (i, l) in lineas.iter().enumerate() {
        for t in l.trozos.iter().filter(|t| etiqueta(&t.texto)) {
            let detras = l
                .trozos
                .iter()
                .filter(|o| o.x0 > t.x0)
                .flat_map(|o| o.palabras.iter())
                .find_map(valido);
            let debajo = || {
                lineas
                    .iter()
                    .skip(i + 1)
                    .take(2)
                    .flat_map(|d| d.trozos.iter())
                    .filter(|o| o.x1 >= t.x0 - 20 && o.x0 <= t.x1 + 120)
                    .flat_map(|o| o.palabras.iter())
                    .find_map(valido)
            };
            if let Some(n) = detras.or_else(debajo) {
                return Some(n);
            }
        }
    }
    // Un trozo que empieza por la etiqueta y sigue con el NIF («NIF B12345674»).
    let empieza = lineas.iter().flat_map(|l| l.trozos.iter()).find_map(|t| {
        let [e, n, ..] = t.palabras.as_slice() else {
            return None;
        };
        if etiqueta(&e.texto) { valido(n) } else { None }
    });
    empieza.or_else(|| {
        lineas
            .iter()
            .flat_map(|l| l.trozos.iter().flat_map(|t| t.palabras.iter()))
            .find_map(valido)
    })
}

/// ¿Es complementaria o sustitutiva? Lo dice una «X» en la línea de su casilla («Declaración
/// sustitutiva …… X», «X Autoliquidación complementaria») y el número de la declaración
/// anterior: otro de trece cifras que empieza por el mismo modelo. Las frases largas que
/// explican cuándo marcarla (con la «X» entre comillas) no cuentan. Si está el número anterior
/// pero no se ve la «X», se toma como complementaria: en una informativa así no se usan sus
/// totales, que pueden ser sólo lo que faltaba.
fn tipo_y_anterior(
    formulario: &[&maqueta::Linea],
    modelo: &str,
    numero: &str,
) -> (Tipo, Option<String>) {
    let mut tipo = Tipo::Normal;
    for l in formulario {
        let marcada = l
            .trozos
            .iter()
            .flat_map(|t| t.palabras.iter())
            .any(|p| p.texto.trim_matches(GUIA) == "X");
        if !marcada {
            continue;
        }
        for t in &l.trozos {
            let p = plano(&t.texto);
            let sin_guia = p.chars().filter(|c| !GUIA.contains(c)).count();
            if sin_guia > 90 {
                continue;
            }
            if p.contains("sustitutiva") && !p.contains("complementaria") {
                tipo = Tipo::Sustitutiva;
            } else if (p.contains("complementaria") || p.contains("rectificativa"))
                && !p.contains("sustitutiva")
                && tipo == Tipo::Normal
            {
                tipo = Tipo::Complementaria;
            }
        }
    }
    let anterior = formulario
        .iter()
        .flat_map(|l| l.trozos.iter().flat_map(|t| t.palabras.iter()))
        .flat_map(|p| {
            p.texto
                .split(|c: char| !c.is_ascii_digit())
                .filter(|s| s.len() == 13)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .find(|n| n.starts_with(modelo) && n != numero);
    if anterior.is_some() && tipo == Tipo::Normal {
        tipo = Tipo::Complementaria;
    }
    (tipo, anterior)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eur(c: i64) -> Money {
        Money::from_cents(c)
    }

    /// Líneas como las del formulario del 303 de la AEAT (posiciones en décimas de punto).
    #[test]
    fn importes_con_su_casilla_en_la_misma_linea() {
        let lines = vec![
            maqueta::Linea::de_trozos(
                2,
                4428,
                &[
                    (439, "IVA devengado"),
                    (2956, "150"),
                    (3956, "151"),
                    (4596, "152"),
                ],
            ),
            // El tipo va junto a la casilla 05 (no es de importe): no se toma.
            maqueta::Linea::de_trozos(
                2,
                4906,
                &[(2973, "04"), (3973, "05"), (4259, "10,00"), (4613, "06")],
            ),
            maqueta::Linea::de_trozos(
                2,
                5133,
                &[
                    (434, "Adquisiciones intracomunitarias"),
                    (2961, "10"),
                    (3692, "12,34"),
                    (4609, "11"),
                    (5392, "2,59"),
                ],
            ),
            // La fórmula de la [27] lleva muchos números: vale el más cercano, la 27.
            maqueta::Linea::de_trozos(
                2,
                6245,
                &[
                    (597, "Total cuota devengada (152 + 167 + 03 + 26)"),
                    (4603, "27"),
                    (5338, "18,88"),
                ],
            ),
            maqueta::Linea::de_trozos(
                2,
                6592,
                &[
                    (430, "Por cuotas soportadas"),
                    (3607, "28"),
                    (4207, "1.000,00"),
                    (4607, "29"),
                    (5286, "210,00"),
                ],
            ),
            maqueta::Linea::de_trozos(
                2,
                8118,
                &[
                    (1097, "Resultado régimen general (27 - 45)"),
                    (4604, "46"),
                    (5256, "-191,12"),
                ],
            ),
            // Un importe sin casilla a la izquierda (otra línea del formulario): no se toma.
            maqueta::Linea::de_trozos(2, 5514, &[(4309, "1,75")]),
        ];
        let known = [3, 6, 10, 11, 27, 28, 29, 46];
        let b = from_lines(&lines, &known);
        assert_eq!(
            b,
            [
                (10, eur(1234)),
                (11, eur(259)),
                (27, eur(1888)),
                (28, eur(100_000)),
                (29, eur(21_000)),
                (46, eur(-19_112)),
            ]
            .into_iter()
            .collect()
        );
    }

    /// En los formularios de 2017-2018 el importe sale un poco más abajo que su casilla: el texto
    /// lo pone en la línea siguiente. Importes inventados.
    #[test]
    fn importe_en_la_linea_de_debajo() {
        let lines = vec![
            maqueta::Linea::de_trozos(
                3,
                4000,
                &[
                    (
                        430,
                        "Por cuotas soportadas en operaciones interiores corrientes",
                    ),
                    (3607, "28"),
                    (4607, "29"),
                ],
            ),
            maqueta::Linea::de_trozos(3, 4090, &[(3900, "1.234,56"), (5100, "259,26")]),
            // Casilla pegada a la línea de puntos del formulario.
            maqueta::Linea::de_trozos(
                3,
                5000,
                &[
                    (430, "Cuotas a compensar de periodos anteriores ....67"),
                    (5233, "321,00"),
                ],
            ),
            // Otra línea, demasiado lejos: su importe no es de la [29].
            maqueta::Linea::de_trozos(3, 4400, &[(5100, "7,77")]),
        ];
        let b = from_lines(&lines, &[28, 29, 67]);
        assert_eq!(b.get(&28), Some(&eur(123_456)));
        assert_eq!(b.get(&29), Some(&eur(25_926)));
        assert_eq!(b.get(&67), Some(&eur(32_100)));
        assert_eq!(b.len(), 3);
    }

    /// El 200 en PDF (inventado): casillas de cinco cifras, y la liquidación (páginas 12 a 15
    /// del formulario) manda sobre las demás cuando una casilla se repite con otro sentido.
    #[test]
    fn el_200_en_pdf_lee_primero_la_liquidacion() {
        let mut lines = vec![
            maqueta::Linea::de_trozos(5, 100, &[(5000, "Página 4")]),
            maqueta::Linea::de_trozos(
                5,
                900,
                &[
                    (300, "TOTAL ACTIVO ......"),
                    (4606, "00180"),
                    (5266, "45.678,90"),
                ],
            ),
            maqueta::Linea::de_trozos(10, 100, &[(5000, "Página 10")]),
            // Estado de cambios del patrimonio neto: la [00562] es la distribución de dividendos.
            maqueta::Linea::de_trozos(10, 900, &[(2980, "00562"), (3500, "5.000,00")]),
            maqueta::Linea::de_trozos(12, 100, &[(5000, "Página 12")]),
            maqueta::Linea::de_trozos(
                12,
                900,
                &[
                    (300, "Cuota íntegra ...."),
                    (4600, "00562"),
                    (5260, "2.222,22"),
                ],
            ),
        ];
        let labels: Vec<String> = (0..15).map(|i| format!("{:05}", 300 + i)).collect();
        for (i, l) in labels.iter().enumerate() {
            let y = 1000 + 200 * i32::try_from(i).unwrap();
            lines.push(maqueta::Linea::de_trozos(
                6,
                y,
                &[(4600, l), (5260, "1,00")],
            ));
        }
        let b = casillas_200(&lines);
        assert_eq!(b.get(&562), Some(&eur(222_222)));
        assert_eq!(b.get(&180), Some(&eur(4_567_890)));
        assert_eq!(b.get(&300), Some(&eur(100)));
    }

    /// Portada de una informativa en PDF (inventada): el total va al final de su línea.
    #[test]
    fn portada_de_una_informativa_en_pdf() {
        let l = |p: u32, y: i32, texto: &str, casilla: &str, valor: &str| {
            maqueta::Linea::de_trozos(p, y, &[(400, texto), (5000, casilla), (6000, valor)])
        };
        let lines = vec![
            l(
                2,
                1000,
                "Número total de percepciones relacionadas ......",
                "01",
                "3",
            ),
            l(
                2,
                1100,
                "Importe total de las percepciones relacionadas ......",
                "02",
                "12.000,50",
            ),
            l(
                2,
                1200,
                "Importe total de las retenciones relacionadas ......",
                "03",
                "1.800,25",
            ),
            // Una tabla posterior con otra casilla 02: vale la primera.
            l(3, 1000, "Perceptor", "02", "1,00"),
        ];
        let b = informativa_de_lineas("190", &lines).unwrap();
        assert_eq!(b.registros, Some(3));
        assert_eq!(b.importe, Some(eur(1_200_050)));
        assert_eq!(b.retenciones, Some(eur(180_025)));
        // En el 349 no hay retenciones.
        assert_eq!(
            informativa_de_lineas("349", &lines).unwrap().retenciones,
            None
        );
        // Sin importe total, no hay lectura.
        assert!(informativa_de_lineas("190", &lines[..1]).is_none());
    }

    #[test]
    fn declaracion_a_cero() {
        let l = |t: &str| maqueta::Linea::de_trozos(1, 4560, &[(760, t)]);
        assert!(a_cero(&[l("NEGATIVA SIN ACTIVIDAD / RESULTADO CERO")]));
        assert!(!a_cero(&[l("A COMPENSAR")]));
    }

    #[test]
    fn informativa_del_visor() {
        // Registro de declarante de un 190 inventado: 2 perceptores, 12.000,50 € y 1.800,25 €.
        let mut r = String::from("11902025B12345674");
        r.push_str(&format!("{:<40}", "EMPRESA DEMO SL"));
        r.push('T');
        r.push_str(&"0".repeat(9));
        r.push_str(&" ".repeat(40));
        r.push_str(&"0".repeat(13));
        r.push(' ');
        r.push(' ');
        r.push_str(&"0".repeat(13));
        r.push_str("000000002");
        r.push(' ');
        r.push_str("000000001200050");
        r.push_str("000000000180025");
        while r.chars().count() < 500 {
            r.push(' ');
        }
        let (b, s) = from_file(&r, "190", 2025, "0A").unwrap();
        assert_eq!(s, Source::Informative);
        assert_eq!(b.registros, Some(2));
        assert_eq!(b.importe, Some(eur(1_200_050)));
        assert_eq!(b.retenciones, Some(eur(180_025)));
        // De otro modelo o de otro año: no.
        assert!(from_file(&r, "190", 2024, "0A").is_none());
        assert!(from_file(&r, "193", 2025, "0A").is_none());
    }

    #[test]
    fn el_303_tiene_que_cuadrar() {
        let b = |pairs: &[(u32, i64)]| FilingBoxes {
            casillas: pairs.iter().map(|(k, v)| (*k, eur(*v))).collect(),
            ..FilingBoxes::default()
        };
        assert!(plausible(
            "303",
            &b(&[
                (11, 259),
                (13, 1629),
                (27, 1888),
                (29, 21_000),
                (37, 259),
                (45, 21_259),
                (46, -19_371)
            ])
        ));
        // El resultado cuadra, pero la [27] no es la suma de sus cuotas.
        assert!(!plausible(
            "303",
            &b(&[(27, 1888), (45, 21_259), (46, -19_371)])
        ));
        // Sin actividad: todo a cero.
        assert!(plausible("303", &b(&[])));
        // Leído con el diseño de otro año: las casillas no cuadran.
        assert!(!plausible("303", &b(&[(7, 1234), (45, 259), (10, 5555)])));
        assert!(plausible("111", &b(&[(2, 100)])));
    }

    #[test]
    fn casillas_guardadas_en_json() {
        let b = FilingBoxes {
            casillas: [(500, eur(1234)), (255, eur(-5))].into_iter().collect(),
            ..FilingBoxes::default()
        };
        let j = serde_json::to_string(&b).unwrap();
        let back: FilingBoxes = serde_json::from_str(&j).unwrap();
        assert_eq!(back, b);
        assert_eq!(back.get(500), eur(1234));
        assert_eq!(back.get(1), Money::ZERO);
        assert!(FilingBoxes::default().is_empty());
    }

    fn l(pagina: u32, y: i32, trozos: &[(i32, &str)]) -> maqueta::Linea {
        maqueta::Linea::de_trozos(pagina, y, trozos)
    }

    /// Portada de un justificante (como las de 2022 en adelante: cada etiqueta con su valor en
    /// la misma línea) y la primera página del formulario. Datos inventados.
    fn portada(modelo: &str, numero: &str, expediente: &str, fecha: &str) -> Vec<maqueta::Linea> {
        vec![
            l(
                1,
                709,
                &[(1593, "INFORMACIÓN DE LA PRESENTACIÓN DE LA DECLARACIÓN")],
            ),
            l(1, 1109, &[(3043, &format!("Modelo {modelo}"))]),
            l(
                1,
                1589,
                &[(
                    620,
                    &format!("Presentación realizada el: {fecha} a las 12:00:00"),
                )],
            ),
            l(
                1,
                1789,
                &[(
                    620,
                    &format!("Expediente/Referencia (nº registro asignado): {expediente}"),
                )],
            ),
            l(
                1,
                1989,
                &[(620, "Código Seguro de Verificación: ABCD1234EFGH5678")],
            ),
            l(
                1,
                2189,
                &[(620, &format!("Número de justificante: {numero}"))],
            ),
            l(1, 2889, &[(620, "NIF Presentador: B11111119")]),
            l(1, 3289, &[(620, "En calidad de: Colaborador")]),
        ]
    }

    #[test]
    fn portada_y_formulario_de_un_justificante() {
        let mut lineas = portada("303", "3030000000017", "202530334500001A", "21-01-2026");
        lineas.extend([
            // El ejercicio y el periodo, debajo de sus etiquetas (como en 2017).
            l(
                2,
                1265,
                &[
                    (3503, "Devengo (2)"),
                    (4156, "Ejercicio"),
                    (5031, "Período"),
                ],
            ),
            l(2, 1290, &[(4664, "2025"), (5506, "4T")]),
            l(2, 1599, &[(445, "B12345674"), (1501, "EMPRESA DEMO SL")]),
        ]);
        let j = cabecera(&lineas, "").unwrap();
        assert_eq!(
            (j.modelo.as_str(), j.ejercicio, j.periodo.as_str()),
            ("303", 2025, "4T")
        );
        // Un 4T presentado en enero es del año anterior: lo dice el formulario, no la fecha.
        assert_eq!(j.presentado, Some(time::macros::date!(2026 - 01 - 21)));
        assert_eq!(j.numero, "3030000000017");
        assert_eq!(j.csv.as_deref(), Some("ABCD1234EFGH5678"));
        assert_eq!(j.expediente.as_deref(), Some("202530334500001A"));
        assert_eq!(j.presentador.as_deref(), Some("B11111119"));
        assert_eq!(j.en_calidad_de.as_deref(), Some("Colaborador"));
        assert_eq!(j.nif.as_deref(), Some("B12345674"));
        assert_eq!((j.tipo, j.anterior), (Tipo::Normal, None));
        // Si el expediente es de otro ejercicio o el modelo de la portada no es el del número de
        // justificante, no se fía.
        let mut otro = portada("303", "3030000000017", "202430334500001A", "21-01-2026");
        otro.extend_from_slice(&lineas[8..]);
        assert!(cabecera(&otro, "").is_none());
        let mut otro = portada("111", "3030000000017", "202530334500001A", "21-01-2026");
        otro.extend_from_slice(&lineas[8..]);
        assert!(cabecera(&otro, "").is_none());
        // Sin la portada no es un justificante (un borrador, por ejemplo).
        assert!(cabecera(&lineas[8..], "").is_none());
    }

    /// La portada dice si el resultado se pidió a devolver («Solicitud de devolución»).
    #[test]
    fn la_portada_dice_si_se_pidio_la_devolucion() {
        let formulario = [
            l(2, 1265, &[(4156, "Ejercicio"), (5031, "Período")]),
            l(2, 1290, &[(4664, "2025"), (5506, "4T")]),
        ];
        let mut lineas = portada("303", "3030000000017", "202530334500001A", "21-01-2026");
        lineas.extend(formulario.clone());
        assert!(!cabecera(&lineas, "").unwrap().devolucion);
        let mut lineas = portada("303", "3030000000017", "202530334500001A", "21-01-2026");
        lineas.push(l(1, 2600, &[(1500, "SOLICITUD DE DEVOLUCIÓN")]));
        lineas.extend(formulario);
        assert!(cabecera(&lineas, "").unwrap().devolucion);
    }

    /// En las portadas de 2017 a 2021 el texto del PDF da primero todas las etiquetas y luego
    /// todos los valores, y en la maqueta las letras se montan: los datos se reconocen por su
    /// forma. Datos inventados.
    #[test]
    fn portada_con_las_etiquetas_antes_que_los_valores() {
        let lineas = vec![
            l(1, 814, &[(1683, "INFORMACIÓN DE LA PRESENTACIÓN DE LA")]),
            l(1, 982, &[(2700, "DECLARACIÓN")]),
            l(1, 1159, &[(3976, "MODELO111")]),
            l(
                1,
                1818,
                &[(
                    773,
                    "Expediente/Referencia (nº registro asig2n0a2d4o1)1:34500005C",
                )],
            ),
            l(
                2,
                1183,
                &[
                    (3729, "Ejercicio ..."),
                    (4400, "2024"),
                    (4876, "Período ....."),
                    (5800, "4T"),
                ],
            ),
            l(2, 2128, &[(668, "B12345674"), (1636, "EMPRESA DEMO SL")]),
        ];
        let texto = "INFORMACIÓN DE LA PRESENTACIÓN DE LA\nDECLARACIÓN MODELO\nRegistro\n\
                     Presentación realizada el\nExpediente/Referencia (nº registro asignado):\n\
                     Código Seguro de Verificación:\nPresentador\nNIF Presentador:\nEn calidad \
                     de:\nApellidos y Nombre / Razón social:\nVía de entrada:\nNúmero de \
                     justificante:\n 111\n\n20-01-2025 a las 10:05:00\n\n202411134500005C\n\n\
                     ABCD1234EFGH5678\n\nB11111119\nASESORES DEMO SL\n\nColaborador\n\n\
                     Presentación por Internet\n1110000000013\n\nLa autenticidad de este \
                     documento puede ser comprobada mediante el Código Seguro\n de Verificación \
                     ABCD1234EFGH5678 en https://sede.agenciatributaria.gob.es 1";
        let j = cabecera(&lineas, texto).unwrap();
        assert_eq!(
            (j.modelo.as_str(), j.ejercicio, j.periodo.as_str()),
            ("111", 2024, "4T")
        );
        assert_eq!(j.numero, "1110000000013");
        assert_eq!(j.presentado, Some(time::macros::date!(2025 - 01 - 20)));
        assert_eq!(j.expediente.as_deref(), Some("202411134500005C"));
        assert_eq!(j.csv.as_deref(), Some("ABCD1234EFGH5678"));
        assert_eq!(j.presentador.as_deref(), Some("B11111119"));
        assert_eq!(j.en_calidad_de.as_deref(), Some("Colaborador"));
    }

    #[test]
    fn complementarias_sustitutivas_y_anuales() {
        // Un 111 complementario: la «X» delante de su casilla y el número del anterior.
        let mut lineas = portada("111", "1110000000013", "202211134500005C", "13-04-2022");
        lineas.extend([
            l(
                2,
                1183,
                &[
                    (3729, "Ejercicio ..."),
                    (4400, "2022"),
                    (4876, "Período ....."),
                    (5800, "1T"),
                ],
            ),
            l(2, 2128, &[(668, "B12345674")]),
            // La explicación lleva la «X» entre comillas: no cuenta.
            l(
                2,
                7052,
                &[(
                    3370,
                    "Si esta autoliquidación es complementaria de otra, marque con una \"X\"",
                )],
            ),
            l(
                2,
                7508,
                &[(3913, "X"), (4087, "Autoliquidación complementaria")],
            ),
            l(
                2,
                8016,
                &[(3487, "Nº de justificante:"), (4281, "1110000000021")],
            ),
        ]);
        let j = cabecera(&lineas, "").unwrap();
        assert_eq!(j.tipo, Tipo::Complementaria);
        assert_eq!(j.anterior.as_deref(), Some("1110000000021"));
        // Un 390 sustitutivo: anual aunque no diga el periodo.
        let mut lineas = portada("390", "3900000000019", "202439034500007E", "29-01-2025");
        lineas.extend([
            l(
                2,
                1191,
                &[
                    (3204, "Ejercicio"),
                    (3700, "2024"),
                    (4579, "Declaración sustitutiva ....... X"),
                ],
            ),
            l(
                2,
                1547,
                &[
                    (3153, "Número identificativo declaración anterior"),
                    (5300, "3900000000027"),
                ],
            ),
            l(2, 1850, &[(486, "B12345674")]),
        ]);
        let j = cabecera(&lineas, "").unwrap();
        assert_eq!((j.ejercicio, j.periodo.as_str()), (2024, "0A"));
        assert_eq!(j.tipo, Tipo::Sustitutiva);
        assert_eq!(j.anterior.as_deref(), Some("3900000000027"));
        // El 202, por pagos, con el valor pegado a la guía.
        let mut lineas = portada("202", "2020000000015", "202520234500009F", "20-10-2025");
        lineas.extend([
            l(
                2,
                1100,
                &[(
                    3607,
                    "Ejercicio .......................................2025",
                )],
            ),
            l(
                2,
                1300,
                &[(
                    3607,
                    "Período (*)................................................. 2P",
                )],
            ),
            l(2, 1500, &[(400, "B12345674")]),
        ]);
        let j = cabecera(&lineas, "").unwrap();
        assert_eq!(
            (j.ejercicio, j.periodo.as_str(), j.tipo),
            (2025, "2P", Tipo::Normal)
        );
    }

    #[test]
    fn importes_pegados_puntos_de_guia_y_columna_de_tipo() {
        let lines = vec![
            // El importe de la [08] pegado al número de la [09].
            l(
                2,
                1000,
                &[(2000, "08"), (2400, "4.100,0009"), (3600, "615,00")],
            ),
            // La base de la [01] pegada al número de la [02], la del tipo (que no es de importe).
            l(
                2,
                1200,
                &[
                    (2000, "01"),
                    (2400, "3.000,0002"),
                    (3000, "4,00"),
                    (3500, "03"),
                    (4000, "120,00"),
                ],
            ),
            // La [29] con los puntos de la guía metidos entre sus cifras.
            l(
                2,
                1500,
                &[
                    (
                        300,
                        "Resultados de anteriores autoliquidaciones ...............2.9",
                    ),
                    (4700, "12,00"),
                ],
            ),
            // La columna «Tipo %»: el 0,62 impreso del recargo no es de la [665].
            l(
                3,
                900,
                &[
                    (2675, "Base imponible"),
                    (3768, "Tipo %"),
                    (4539, "Cuota devengada"),
                ],
            ),
            l(3, 1458, &[(2217, "665"), (3838, "0,62"), (4117, "666")]),
            l(
                3,
                1600,
                &[
                    (2217, "25"),
                    (3300, "800,00"),
                    (3875, "21"),
                    (4135, "26"),
                    (5299, "168,00"),
                ],
            ),
            // Otra tabla: ya no hay columna de tipo.
            l(3, 2000, &[(4018, "Base"), (4995, "Cuota")]),
            l(
                3,
                2200,
                &[
                    (3607, "28"),
                    (3850, "1.000,00"),
                    (4607, "31"),
                    (5286, "210,00"),
                ],
            ),
        ];
        let b = from_lines(&lines, &[1, 3, 8, 9, 25, 26, 28, 29, 31, 665, 666]);
        assert_eq!(b.get(&8), Some(&eur(410_000)));
        assert_eq!(b.get(&9), Some(&eur(61_500)));
        assert_eq!(b.get(&1), Some(&eur(300_000)));
        assert_eq!(b.get(&3), Some(&eur(12_000)));
        assert_eq!(b.get(&2), None);
        assert_eq!(b.get(&29), Some(&eur(1_200)));
        assert_eq!(b.get(&665), None);
        assert_eq!(b.get(&25), Some(&eur(80_000)));
        assert_eq!(b.get(&26), Some(&eur(16_800)));
        assert_eq!(b.get(&28), Some(&eur(100_000)));
        assert_eq!(b.get(&31), Some(&eur(21_000)));
        // En el 200, la casilla con su importe entre los puntos de la guía.
        let lines = vec![l(
            13,
            5001,
            &[
                (311, "Multas, sanciones y otros (art. 15 c) LIS) ......"),
                (3900, "01815.........................3..1...5...,.2...0."),
            ],
        )];
        assert_eq!(casillas_200(&lines).get(&1815), Some(&eur(31_520)));
    }

    #[test]
    fn informativa_con_los_totales_en_una_linea() {
        let lines = vec![l(
            2,
            6677,
            &[
                (253, "01"),
                (1228, "1"),
                (1549, "02"),
                (2171, "1.000,00"),
                (2849, "03"),
                (3755, "190,00"),
                (4371, "04"),
                (5284, "190,00"),
            ],
        )];
        let b = informativa_de_lineas("193", &lines).unwrap();
        assert_eq!(b.registros, Some(1));
        assert_eq!(b.importe, Some(eur(100_000)));
        assert_eq!(b.retenciones, Some(eur(19_000)));
    }

    #[test]
    fn operadores_del_349() {
        let cabecera_op = |y: i32| {
            l(
                3,
                y,
                &[
                    (360, "Código país"),
                    (720, "NIF comunitario"),
                    (1584, "Apellidos y nombre, denominación o razón social"),
                    (4623, "Clave"),
                    (4831, "Base imponible"),
                ],
            )
        };
        let lines = vec![
            l(3, 1050, &[(379, "Operador 1")]),
            cabecera_op(1177),
            l(
                3,
                1308,
                &[
                    (402, "IE"),
                    (734, "1234567X"),
                    (1606, "PROVEEDOR DEMO LIMITED"),
                    (4661, "I"),
                    (5372, "23,40"),
                ],
            ),
            l(
                3,
                1424,
                &[(
                    361,
                    "A cumplimentar exclusivamente en caso de clave de operación C",
                )],
            ),
            l(3, 1868, &[(379, "Operador 2")]),
            cabecera_op(1994),
            l(
                3,
                2134,
                &[
                    (362, "DE"),
                    (729, "123456789"),
                    (1601, "CLIENTE DEMO GMBH"),
                    (4647, "S"),
                    (5000, "1.000,00"),
                ],
            ),
            // Un operador vacío del formulario: no cuenta.
            l(3, 2694, &[(375, "Operador 3")]),
            cabecera_op(2820),
        ];
        let resumen = |n: u32, total: i64| FilingBoxes {
            registros: Some(n),
            importe: Some(eur(total)),
            ..FilingBoxes::default()
        };
        let ops = operadores_349(&lines, &resumen(2, 102_340));
        assert_eq!(ops.len(), 2);
        assert_eq!(
            (ops[0].nif.as_str(), ops[0].clave.as_str(), ops[0].importe),
            ("IE1234567X", "I", eur(2_340))
        );
        assert_eq!(ops[1].nif, "DE123456789");
        assert_eq!(ops[1].nombre, "CLIENTE DEMO GMBH");
        // Si no suman el total del resumen, se ha leído mal alguno: ninguno.
        assert!(operadores_349(&lines, &resumen(2, 102_341)).is_empty());
        assert!(operadores_349(&lines, &resumen(3, 102_340)).is_empty());
    }

    #[test]
    fn declarados_del_347() {
        let declarado = |y: i32, nif: &str, nombre: &str, clave: &str, importe: &str| {
            vec![
                l(3, y, &[(365, &format!("Declarado {}", y / 1000))]),
                l(
                    3,
                    y + 120,
                    &[
                        (386, "NIF declarado"),
                        (1124, "NIF-IVA declarado"),
                        (2035, "NIF representante"),
                        (2777, "Apellidos y nombre, razón social o denominación"),
                    ],
                ),
                l(3, y + 260, &[(385, nif), (2781, nombre)]),
                l(
                    3,
                    y + 400,
                    &[
                        (391, "Provincia (Código)"),
                        (1179, "País (Código)"),
                        (1962, "Clave operación"),
                    ],
                ),
                l(3, y + 530, &[(407, "46"), (2179, clave)]),
                l(
                    3,
                    y + 860,
                    &[
                        (379, "Importe anual de las operaciones"),
                        (
                            2037,
                            "Importe anual percibido por transmisiones de inmuebles",
                        ),
                    ],
                ),
                l(3, y + 1000, &[(1478, importe)]),
                l(3, y + 1230, &[(414, "1 T"), (1478, importe), (2076, "1 T")]),
            ]
        };
        let mut lines = declarado(1000, "B12345674", "PROVEEDOR DEMO SL", "A", "4.000,00");
        lines.extend(declarado(
            3000,
            "B22222228",
            "CLIENTE DEMO SL",
            "B",
            "12.500,50",
        ));
        let resumen = |n: u32, total: i64| FilingBoxes {
            registros: Some(n),
            importe: Some(eur(total)),
            ..FilingBoxes::default()
        };
        let d = declarados_347(&lines, &resumen(2, 1_650_050));
        assert_eq!(d.len(), 2);
        assert_eq!(
            (d[0].nif.as_str(), d[0].clave.as_str(), d[0].importe),
            ("B12345674", "A", eur(400_000))
        );
        assert_eq!(d[1].nombre, "CLIENTE DEMO SL");
        assert_eq!((d[1].clave.as_str(), d[1].importe), ("B", eur(1_250_050)));
        assert!(declarados_347(&lines, &resumen(2, 1_650_051)).is_empty());
    }

    #[test]
    fn el_390_y_la_liquidacion_del_200_tienen_que_cuadrar() {
        let b = |pairs: &[(u32, i64)]| FilingBoxes {
            casillas: pairs.iter().map(|(k, v)| (*k, eur(*v))).collect(),
            ..FilingBoxes::default()
        };
        // [65] = [47] − [64], [84] = [65] + [83] + [658], [86] = [84] + [659] − [85].
        let bien = b(&[
            (47, 1_000_000),
            (64, 100_000),
            (65, 900_000),
            (84, 900_000),
            (86, 900_000),
        ]);
        assert!(plausible_pdf("390", &bien));
        // El tipo del recargo (0,62) leído como la [65].
        let mal = b(&[
            (47, 1_000_000),
            (64, 100_000),
            (65, 62),
            (84, 900_000),
            (86, 900_000),
        ]);
        assert!(!plausible_pdf("390", &mal));
        // El fichero de la AEAT se comprueba sólo con su diseño de registro.
        assert!(plausible("390", &mal));
        // En el 200, cuota diferencial [00611] = [00599] − [00601] − [00603] − [00605].
        let mut d = b(&[
            (599, 400_000),
            (601, 650_000),
            (611, -250_000),
            (1586, -250_000),
            (552, 1_000_000),
        ]);
        liquidacion_200(&mut d);
        assert!(d.dudosas.is_empty());
        assert_eq!(d.get(601), eur(650_000));
        // Sin la [00611], con el resultado [01586].
        let mut d = b(&[(603, 50_000), (605, 50_000), (1586, -100_000)]);
        liquidacion_200(&mut d);
        assert!(d.dudosas.is_empty());
        // Se ha perdido un pago fraccionado: la liquidación no se usa, el resto sí.
        let mut d = b(&[(603, 50_000), (1586, -100_000), (552, 1_000_000)]);
        liquidacion_200(&mut d);
        assert!(d.dudosas.contains(&603) && d.dudosas.contains(&1586));
        assert_eq!(d.casillas.get(&603), None);
        assert_eq!(d.get(552), eur(1_000_000));
        let j = serde_json::to_string(&d).unwrap();
        assert_eq!(serde_json::from_str::<FilingBoxes>(&j).unwrap(), d);
    }
}
