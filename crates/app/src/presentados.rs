//! Modelos presentados por la gestoría, para compararlos casilla a casilla con los que calcula
//! TaxPain con los mismos datos.
//!
//! Se leen de:
//! - el **fichero** en formato de la AEAT (el `.txt` de «presentación mediante fichero»): exacto,
//!   con los diseños de registro de `fixtures/aeat/dr`;
//! - el **PDF** de la declaración o del justificante: se buscan pares «casilla importe» con las
//!   casillas que existen en ese modelo. Es aproximado: la pantalla pide revisarlo;
//! - lo que se **teclee** a mano.

use std::collections::BTreeMap;

use serde::Serialize;
use sha2::{Digest, Sha256};
use taxpain_core::money::Money;
use taxpain_models::dr::{self, Spec};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;

/// Autoliquidaciones con casillas que se leen de su fichero o de su PDF y se comparan casilla a
/// casilla.
pub const MODELS: [&str; 8] = ["303", "390", "111", "115", "123", "130", "202", "349"];

/// Declaraciones informativas que se comparan con lo que calcula TaxPain: sus totales (en la
/// [02] el importe y en la [03] las retenciones, como en la revisión de la carpeta) y, si se
/// tiene su fichero, perceptor a perceptor o tercero a tercero.
pub const INFORMATIVAS: [&str; 4] = ["347", "190", "180", "193"];

/// ¿Se compara con lo que calcula TaxPain? Las autoliquidaciones de [`MODELS`], el 200 y las
/// [`INFORMATIVAS`]. No se comparan el 216, el 296, el 720 ni el 721: TaxPain no los calcula ni
/// tiene sus diseños de registro (`TODO(VERIFY)` si se añaden), y el 184, que sí calcula, no
/// lo presenta una sociedad.
pub fn comparable(model: &str) -> bool {
    MODELS.contains(&model) || MANUAL_ONLY.contains(&model) || INFORMATIVAS.contains(&model)
}

/// Modelos que sólo se meten a mano (sus casillas, sin leer el fichero): el 200, para
/// compararlo y para la base de los 202 siguientes (art. 40.2 LIS).
pub const MANUAL_ONLY: [&str; 1] = ["200"];

/// Modelos que se guardan en «Modelos presentados» desde un justificante con sus casillas
/// leídas: los que se comparan y el 200 (que no se lee de un fichero, pero sí de su
/// justificante).
pub fn storable(model: &str) -> bool {
    comparable(model)
}

/// Modelos que se aceptan de una lectura del asistente o de Claude Code: los que TaxPain lee,
/// el 200 y los demás que presenta una gestoría y que revisa la carpeta.
pub fn known_model(model: &str) -> bool {
    MODELS.contains(&model)
        || MANUAL_ONLY.contains(&model)
        || crate::revision::modelos::MODELOS.contains(&model)
}

/// Casillas de importe del 200 que calcula TaxPain (`taxpain_models::m200`).
const BOXES_200: [u16; 21] = [
    500, 301, 302, 501, 1815, 417, 418, 550, 1032, 547, 552, 562, 582, 592, 1766, 599, 601, 603,
    605, 611, 1586,
];

fn spec(model: &str) -> Option<&'static Spec> {
    Some(match model {
        "303" => taxpain_models::m303::spec(),
        "390" => taxpain_models::m390::spec(),
        "111" => taxpain_models::withholding::spec_111(),
        "115" => taxpain_models::withholding::spec_115(),
        "123" => taxpain_models::m123::spec(),
        "130" => taxpain_models::m130::spec(),
        "202" => taxpain_models::m202::spec(),
        "349" => taxpain_models::m349::spec(),
        _ => return None,
    })
}

/// Casilla de un campo cuando el DR no la trae: en el 111 el campo `11 + n` es la casilla [n].
fn box_of(model: &str, f: &dr::Field) -> Option<u16> {
    match model {
        "111" if (12..=41).contains(&f.n) => Some(f.n - 11),
        _ => None,
    }
}

/// Casillas que existen en el modelo (para no tomar por casilla cualquier número del PDF).
pub(crate) fn known_boxes(model: &str) -> Vec<u16> {
    if model == "200" {
        return BOXES_200.to_vec();
    }
    if INFORMATIVAS.contains(&model) {
        return vec![2, 3];
    }
    let Some(s) = spec(model) else {
        return Vec::new();
    };
    let mut v: Vec<u16> = s
        .pages
        .iter()
        .flat_map(|p| p.fields.iter())
        .filter(|f| f.is_amount())
        .filter_map(|f| f.box_.or_else(|| box_of(model, f)))
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

/// Casillas de importe de `model` en el diseño de registro de ese ejercicio y periodo, junto con
/// las del vigente: un formulario de 2017 tiene casillas que ya no existen (la [61] o la [67] del
/// 303 de 2017 no están en el de 2026).
pub(crate) fn known_boxes_for(model: &str, year: i32, period: &str) -> Vec<u16> {
    let mut v = known_boxes(model);
    if model != "200"
        && !INFORMATIVAS.contains(&model)
        && let Some(s) = taxpain_models::dr_versions::spec_for(model, year, period)
    {
        v.extend(
            s.pages
                .iter()
                .flat_map(|p| p.fields.iter())
                .filter(|f| f.is_amount())
                .filter_map(|f| f.box_.or_else(|| box_of(model, f))),
        );
        v.sort_unstable();
        v.dedup();
    }
    v
}

/// Un modelo leído (aún sin guardar).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Read {
    pub model: String,
    pub year: i32,
    /// `1T`…`4T`, `0A`, `1P`…`3P` o el mes.
    pub period: String,
    /// `fichero`, `pdf` o `justificante` (el PDF del justificante de presentación de la AEAT,
    /// leído por posiciones).
    pub source: String,
    pub boxes: BTreeMap<u16, Money>,
    /// El documento no decía el ejercicio: se ha deducido (hay que comprobarlo).
    pub year_guessed: bool,
    /// Del justificante: fecha de presentación.
    pub presented_at: Option<Date>,
    /// Del justificante: su número (trece cifras) y el de la declaración a la que complementa
    /// o sustituye.
    pub receipt: Option<String>,
    pub previous_receipt: Option<String>,
    /// Del justificante: «normal», «complementaria» o «sustitutiva».
    pub kind: Option<String>,
    /// Del justificante: NIF del declarante.
    pub nif: Option<String>,
    /// Se han leído las casillas. Un justificante cuyas casillas no se han podido leer (o no
    /// cuadran) se reconoce igual, pero `boxes` va vacío y no se sabe cuánto valen: no son cero.
    pub boxes_read: bool,
}

/// Por qué no se ha podido leer un fichero de la AEAT.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileError {
    /// No es el fichero de una autoliquidación que TaxPain lea.
    NotAFile,
    /// TaxPain no tiene el diseño de registro de ese ejercicio y periodo, o el fichero no cuadra
    /// con él (páginas de otra longitud): hay que actualizar TaxPain (los diseños nuevos se
    /// buscan con `tools/aeat/dr_actualizar.py`). Mientras, las casillas salen del PDF.
    UnknownDesign {
        model: String,
        year: i32,
        period: String,
    },
    /// Leído con el diseño de su ejercicio, las casillas no cuadran entre sí (en el 303, [27],
    /// [45] y [46] con sus sumas): el fichero se generó con el diseño de otro año. No se usa.
    Incoherent {
        model: String,
        year: i32,
        period: String,
    },
}

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FileError::NotAFile => write!(f, "no es un fichero de autoliquidación"),
            FileError::UnknownDesign {
                model,
                year,
                period,
            } => write!(
                f,
                "TaxPain no tiene el diseño de registro del modelo {model} de {year} {period}: \
                 hay que actualizar TaxPain; mientras, revisa el PDF"
            ),
            FileError::Incoherent {
                model,
                year,
                period,
            } => write!(
                f,
                "las casillas del modelo {model} de {year} {period} no cuadran entre sí: el \
                 fichero parece hecho con el diseño de registro de otro año; revisa el PDF"
            ),
        }
    }
}

/// Fichero en formato de la AEAT.
pub fn read_file(text: &str) -> Option<Read> {
    read_file_checked(text).ok()
}

/// Fichero en formato de la AEAT, con el diseño de registro de su ejercicio y periodo
/// (`taxpain_models::dr_versions`): el de 2026 lee mal un 303 de 2025. Si TaxPain no tiene ese
/// diseño o el fichero no cuadra con él, no se lee por posiciones.
pub fn read_file_checked(text: &str) -> std::result::Result<Read, FileError> {
    let (model, year, period) = dr::read_header(text).ok_or(FileError::NotAFile)?;
    if spec(&model).is_none() {
        return Err(FileError::NotAFile);
    }
    let unknown = || FileError::UnknownDesign {
        model: model.clone(),
        year,
        period: period.clone(),
    };
    let s = taxpain_models::dr_versions::spec_for(&model, year, &period).ok_or_else(unknown)?;
    if !dr::pages_fit(s, text) {
        return Err(unknown());
    }
    let boxes = dr::read_boxes(s, text, |f| box_of(&model, f));
    let check = crate::lectura_justificante::FilingBoxes {
        casillas: boxes.iter().map(|(k, v)| (u32::from(*k), *v)).collect(),
        ..Default::default()
    };
    if !crate::lectura_justificante::plausible(&model, &check) {
        return Err(FileError::Incoherent {
            model,
            year,
            period,
        });
    }
    Ok(Read {
        model,
        year,
        period,
        source: "fichero".into(),
        boxes,
        boxes_read: true,
        ..Read::default()
    })
}

/// Importe español al final de un trozo («1.234,56», «-12,00»).
pub(crate) fn amount(s: &str) -> Option<Money> {
    let t = s.trim();
    let neg = t.starts_with('-');
    let t = t.trim_start_matches('-');
    let (i, d) = t.split_once(',')?;
    if d.len() != 2 || !d.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let digits: String = i.chars().filter(char::is_ascii_digit).collect();
    if digits.is_empty() || i.chars().any(|c| !c.is_ascii_digit() && c != '.') {
        return None;
    }
    let c: i64 = format!("{digits}{d}").parse().ok()?;
    Some(Money::from_cents(if neg { -c } else { c }))
}

/// Texto del PDF de un modelo: modelo, ejercicio, periodo y pares «casilla importe».
///
/// El ejercicio sale de la etiqueta «Ejercicio» (aunque pdf-extract ponga un salto de línea o
/// separe las cifras). Si no la hay, se deduce (y se marca `year_guessed` para que se compruebe):
/// con la fecha más reciente del documento (la de presentación), que es del año siguiente si es
/// un 4T, un anual o diciembre presentado en enero o febrero; y si no hay fechas, con el año más
/// antiguo que aparezca suelto. Nunca el más reciente: los 4T y los anuales se presentan en enero
/// del año siguiente y esa fecha también sale en el PDF.
pub fn read_pdf_text(text: &str) -> Option<Read> {
    // Espacios simples: pdf-extract pone saltos de línea entre la etiqueta y el valor.
    let lower = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if !lower.contains("agencia tributaria") && !lower.contains("aeat") {
        return None;
    }
    let model = MODELS.iter().find(|m| {
        ["modelo ", "modelo: ", "mod. ", "mod.", "modelo nº "]
            .iter()
            .any(|p| lower.contains(&format!("{p}{m}")))
    })?;
    let period = period_in(&lower).unwrap_or_else(|| {
        if *model == "390" {
            "0A".into()
        } else {
            String::new()
        }
    });
    let (year, year_guessed) = match labeled_year(&lower) {
        Some(y) => (y, false),
        None => (guess_year(&lower, &period)?, true),
    };
    let known = known_boxes(model);
    let mut boxes = BTreeMap::new();
    let tokens: Vec<&str> = text.split_whitespace().collect();
    for w in tokens.windows(2) {
        let b = w[0].trim_matches(|c: char| c == '[' || c == ']' || c == '(' || c == ')');
        if !(2..=3).contains(&b.len()) || !b.bytes().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(n) = b.parse::<u16>() else { continue };
        if !known.contains(&n) {
            continue;
        }
        if let Some(m) = amount(w[1]) {
            boxes.entry(n).or_insert(m);
        }
    }
    if boxes.is_empty() {
        return None;
    }
    Some(Read {
        model: (*model).to_owned(),
        year,
        period,
        source: "pdf".into(),
        boxes,
        year_guessed,
        boxes_read: true,
        ..Read::default()
    })
}

struct PdfRx {
    year: regex::Regex,
    period: regex::Regex,
    quarter_words: regex::Regex,
    date: regex::Regex,
}

fn pdf_rx() -> &'static PdfRx {
    static R: std::sync::OnceLock<PdfRx> = std::sync::OnceLock::new();
    R.get_or_init(|| {
        let r = |s: &str| regex::Regex::new(s).expect("regla fija válida");
        PdfRx {
            // «Ejercicio 2024», «Ejercicio: 2024», «Ejercicio fiscal 2 0 2 4».
            year: r(r"\bejercicio(?: fiscal)?\s*[:.]?\s*((?:\d ?){4})\b"),
            period: r(r"\bper[ií]odo\s*[:.]?\s*(1t|2t|3t|4t|0a|1p|2p|3p|0[1-9]|1[0-2])\b"),
            quarter_words: r(r"\b(primer|1º|1er|segundo|2º|tercer|3º|3er|cuarto|4º) trimestre\b"),
            date: r(r"\b(\d{1,2})[/.-](\d{1,2})[/.-](20\d\d)\b"),
        }
    })
}

fn labeled_year(lower: &str) -> Option<i32> {
    pdf_rx()
        .year
        .captures_iter(lower)
        .filter_map(|c| c[1].replace(' ', "").parse::<i32>().ok())
        .find(|y| (2000..=2099).contains(y))
}

fn period_in(lower: &str) -> Option<String> {
    let rx = pdf_rx();
    if let Some(c) = rx.period.captures(lower) {
        return Some(c[1].to_uppercase());
    }
    let c = rx.quarter_words.captures(lower)?;
    Some(
        match &c[1] {
            "primer" | "1º" | "1er" => "1T",
            "segundo" | "2º" => "2T",
            "tercer" | "3º" | "3er" => "3T",
            _ => "4T",
        }
        .to_owned(),
    )
}

/// Ejercicio deducido cuando el PDF no lo dice (ver [`read_pdf_text`]).
fn guess_year(lower: &str, period: &str) -> Option<i32> {
    let rx = pdf_rx();
    let latest = rx
        .date
        .captures_iter(lower)
        .filter_map(|c| {
            let y: i32 = c[3].parse().ok()?;
            let m: u8 = c[2].parse().ok()?;
            let d: u8 = c[1].parse().ok()?;
            ((1..=12).contains(&m) && (1..=31).contains(&d)).then_some((y, m, d))
        })
        .max();
    if let Some((y, m, _)) = latest {
        let filed_next_year = matches!(period, "4T" | "0A" | "12") && m <= 2;
        return Some(if filed_next_year { y - 1 } else { y });
    }
    // Un año suelto: no dentro de una fecha ni de una referencia como «HAC/819/2024».
    lower
        .split_whitespace()
        .filter(|w| !w.contains(['/', '-']))
        .filter_map(|w| {
            let t = w.trim_matches(|c: char| "().,;:".contains(c));
            (t.len() == 4 && t.starts_with("20")).then(|| t.parse::<i32>().ok())?
        })
        .min()
}

/// Lee un fichero (formato AEAT o PDF). Un PDF se lee primero como justificante de
/// presentación de la AEAT ([`crate::lectura_justificante::leer_justificante`]), de cualquier
/// modelo; si no lo es, por su texto.
pub fn read(bytes: &[u8]) -> Option<Read> {
    if bytes.starts_with(b"%PDF") {
        if let Some(j) = crate::lectura_justificante::leer_justificante(bytes) {
            return Some(de_justificante(&j));
        }
        let text = crate::pdf_text::text(bytes)?;
        return read_pdf_text(&text);
    }
    read_file(&latin1(bytes))
}

/// Un justificante como modelo leído. Las casillas, sólo de los modelos que TaxPain conoce
/// ([`known_model`]); las informativas llevan su importe total en la [02] y sus retenciones en
/// la [03], como en la revisión de la carpeta.
pub(crate) fn de_justificante(j: &crate::lectura_justificante::Justificante) -> Read {
    let leidas = j.casillas.as_ref().filter(|_| known_model(&j.modelo));
    let mut boxes: BTreeMap<u16, Money> = leidas
        .map(|b| {
            b.casillas
                .iter()
                .filter_map(|(k, v)| Some((u16::try_from(*k).ok()?, *v)))
                .collect()
        })
        .unwrap_or_default();
    if let Some(b) = leidas {
        if let Some(i) = b.importe {
            boxes.entry(2).or_insert(i);
        }
        if let Some(r) = b.retenciones {
            boxes.entry(3).or_insert(r);
        }
    }
    Read {
        model: j.modelo.clone(),
        year: j.ejercicio,
        period: j.periodo.clone(),
        source: "justificante".into(),
        boxes,
        year_guessed: false,
        presented_at: j.presentado,
        receipt: Some(j.numero.clone()),
        previous_receipt: j.anterior.clone(),
        kind: Some(j.tipo.code().to_owned()),
        nif: j.nif.clone(),
        boxes_read: leidas.is_some(),
    }
}

/// Fichero de un 347 o un 349 (registros de 500 posiciones, sin casillas): sus terceros u
/// operadores, para cruzarlos por NIF en la revisión de la carpeta. No se guarda para comparar.
pub fn read_informative(bytes: &[u8]) -> Option<taxpain_models::informative::ReadFile> {
    if bytes.starts_with(b"%PDF") {
        return None;
    }
    taxpain_models::informative::read_file(&latin1(bytes))
}

/// Fichero de una informativa que se compara (347, 190, 180, 193) como modelo presentado: sus
/// totales en la [02] (importe) y la [03] (retenciones). Sus perceptores o terceros se leen
/// otra vez del documento guardado al comparar.
pub fn read_informative_filed(bytes: &[u8]) -> Option<Read> {
    let f = read_informative(bytes)?;
    if !INFORMATIVAS.contains(&f.model.as_str()) {
        return None;
    }
    let mut boxes = BTreeMap::from([(2, f.total)]);
    if let Some(w) = f.withheld {
        boxes.insert(3, w);
    }
    Some(Read {
        model: f.model,
        year: f.year,
        period: f.period,
        source: "fichero".into(),
        boxes,
        nif: Some(f.declarant_nif).filter(|n| !n.is_empty()),
        boxes_read: true,
        ..Read::default()
    })
}

/// Texto de un fichero de la AEAT (UTF-8 o, si no lo es, ISO-8859-1).
fn latin1(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_owned(),
        Err(_) => bytes.iter().map(|&b| char::from(b)).collect(),
    }
}

/// Tipo de declaración (I, U, C, D, N) de un 303 en el formato de la AEAT (campo 6 de la
/// página 01).
fn declaration_in_file(model: &str, text: &str) -> Option<String> {
    if model != "303" {
        return None;
    }
    let page = spec(model)?.page("01").ok()?;
    let marker: String = page
        .fields
        .iter()
        .take_while(|f| f.constant.is_some())
        .filter_map(|f| f.constant.clone())
        .collect();
    let at = text.find(&marker)?;
    let record: String = text[at..].chars().take(page.len()).collect();
    let d = dr::PageReader::new(page, &record)
        .ok()?
        .raw(6)
        .ok()?
        .trim()
        .to_uppercase();
    (d.len() == 1).then_some(d)
}

/// Casillas presentadas y, si se sabe, el tipo de declaración.
pub(crate) type FiledBoxes = (Option<String>, BTreeMap<u16, Money>);

/// Lo que presentó el gestor de un modelo y periodo (importado de la carpeta o tecleado):
/// casillas y, si se sabe (fichero de la AEAT), el tipo de declaración. Si hay varios, manda el
/// fichero de la AEAT, luego lo tecleado y luego el PDF.
pub(crate) fn filed_by_gestor(
    v: &Vault,
    model: &str,
    year: i32,
    period: &str,
) -> Result<Option<FiledBoxes>> {
    let row: Option<(String, String, Option<Vec<u8>>)> = v
        .connection()
        .query_row(
            "SELECT f.source, f.boxes, d.content FROM filed_model f
             LEFT JOIN document d ON d.id = f.document_id
             WHERE f.model = ?1 AND f.year = ?2 AND f.period = ?3
               -- Una declaración que la AEAT tiene por sustituida (complementaria o
               -- sustitutiva posterior) no es «lo presentado».
               AND NOT EXISTS (
                 SELECT 1 FROM aeat_filing a JOIN aeat_filing b
                   ON b.previous_receipt = a.receipt AND b.model = a.model AND b.year = a.year
                 WHERE a.filed_model_id = f.id)
             -- Primero la declaración vigente según la AEAT (la que ninguna otra sustituye), sea
             -- cual sea su fuente: una complementaria leída del PDF gana a la original importada
             -- de la carpeta del gestor. Después, fichero > manual > PDF.
             ORDER BY CASE WHEN EXISTS (SELECT 1 FROM aeat_filing a WHERE a.filed_model_id = f.id)
                           THEN 0 ELSE 1 END,
                      CASE f.source WHEN 'fichero' THEN 0 WHEN 'manual' THEN 1 ELSE 2 END,
                      f.id DESC
             LIMIT 1",
            params![model, year, period],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    Ok(row.map(|(source, boxes, content)| {
        let declaration = content
            .filter(|_| source == "fichero")
            .and_then(|c| declaration_in_file(model, &latin1(&c)));
        (declaration, boxes_from(&boxes))
    }))
}

/// Modelo presentado guardado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Filed {
    pub id: i64,
    pub model: String,
    pub year: i32,
    pub period: String,
    pub filename: String,
    pub source: String,
    pub boxes: BTreeMap<u16, Money>,
}

fn boxes_json(b: &BTreeMap<u16, Money>) -> String {
    let m: BTreeMap<String, i64> = b.iter().map(|(k, v)| (k.to_string(), v.cents())).collect();
    serde_json::to_string(&m).unwrap_or_else(|_| "{}".into())
}

fn boxes_from(json: &str) -> BTreeMap<u16, Money> {
    serde_json::from_str::<BTreeMap<String, i64>>(json)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(k, v)| Some((k.parse().ok()?, Money::from_cents(v))))
        .collect()
}

/// Guarda un modelo leído. Si el mismo fichero ya estaba, devuelve el que había.
pub fn store(
    v: &Vault,
    filename: &str,
    bytes: &[u8],
    r: &Read,
    now: OffsetDateTime,
) -> Result<i64> {
    Ok(store_read(v, filename, bytes, r, false, now)?.id())
}

/// Qué pasó al guardar un modelo leído.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stored {
    New(i64),
    /// El mismo fichero ya estaba leído del PDF y se ha sustituido por esta lectura.
    Replaced(i64),
    /// El mismo fichero ya estaba y se deja como estaba.
    Kept(i64),
    /// Otro fichero con la misma declaración (el mismo número de justificante): una copia. No
    /// se guarda otra vez; si las casillas leídas difieren, se dice cuál se queda.
    Copy(i64, Copia),
}

/// Qué pasó con una copia de una declaración ya guardada ([`Stored::Copy`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Copia {
    /// Las mismas casillas: nada que hacer.
    Igual,
    /// Casillas distintas: se queda la que había (el justificante oficial o la lectura más
    /// completa).
    SeQuedaLaGuardada { these: usize, kept: usize },
    /// Casillas distintas: ésta sustituye a la que había (es el justificante oficial o trae
    /// más casillas).
    Sustituye { these: usize, before: usize },
}

impl Copia {
    /// Aviso para el resultado de la importación, si las casillas difieren.
    pub fn aviso(self) -> Option<String> {
        match self {
            Copia::Igual => None,
            Copia::SeQuedaLaGuardada { these, kept } => Some(format!(
                "ojo: esta copia trae casillas distintas ({these} leídas frente a {kept}); se \
                 queda la guardada (el justificante oficial o la lectura más completa): \
                 revisa el PDF"
            )),
            Copia::Sustituye { these, before } => Some(format!(
                "ojo: la copia guardada antes traía casillas distintas ({before} leídas frente \
                 a {these}); se queda ésta (el justificante oficial o la lectura más completa): \
                 revisa el PDF"
            )),
        }
    }
}

impl Stored {
    pub fn id(self) -> i64 {
        match self {
            Stored::New(id) | Stored::Replaced(id) | Stored::Kept(id) | Stored::Copy(id, _) => id,
        }
    }

    pub fn is_new(self) -> bool {
        matches!(self, Stored::New(_))
    }
}

/// Guarda un modelo leído. Si el mismo fichero ya estaba y `replace`, una lectura del PDF (la
/// del asistente, mejor que la de las reglas) sustituye a la que había sacada del mismo PDF; el
/// fichero de la AEAT y lo tecleado no se tocan.
pub fn store_read(
    v: &Vault,
    filename: &str,
    bytes: &[u8],
    r: &Read,
    replace: bool,
    now: OffsetDateTime,
) -> Result<Stored> {
    let sha = hex::encode(Sha256::digest(bytes));
    let conn = v.connection();
    // Las casillas de un justificante salen de su PDF: se guardan como las de un PDF.
    let fuente = if r.source == "justificante" {
        "pdf"
    } else {
        r.source.as_str()
    };
    if let Some((id, source)) = conn
        .query_row(
            "SELECT id, source FROM filed_model WHERE file_sha256 = ?1",
            [&sha],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?
    {
        if !(replace && source == "pdf" && fuente == "pdf") {
            return Ok(Stored::Kept(id));
        }
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE filed_model SET model = ?1, year = ?2, period = ?3, boxes = ?4 WHERE id = ?5",
            params![r.model, r.year, r.period, boxes_json(&r.boxes), id],
        )?;
        audit::append(
            &tx,
            &now_str(now),
            Actor::User,
            "filed_model.replace",
            &format!("{} {} {}", r.model, r.year, r.period),
            &serde_json::json!({ "boxes": r.boxes.len() }),
        )?;
        tx.commit()?;
        return Ok(Stored::Replaced(id));
    }
    // Una copia del mismo justificante con otro contenido (otra descarga, impreso, con la
    // declaración detrás): la misma declaración, no otra.
    let mut sustituida = None;
    if let Some((id, otro, otra)) = same_declaration(v, r)? {
        if otra.boxes == r.boxes {
            return Ok(Stored::Copy(id, Copia::Igual));
        }
        if !wins_over(filename, r, &otro, &otra) {
            return Ok(Stored::Copy(
                id,
                Copia::SeQuedaLaGuardada {
                    these: r.boxes.len(),
                    kept: otra.boxes.len(),
                },
            ));
        }
        sustituida = Some((id, otra.boxes.len()));
    }
    let tx = conn.unchecked_transaction()?;
    let uuid = format!("presentado-{}", &sha[..32]);
    // Si el modelo se quitó (Importar → Comparar), su documento sigue: se reutiliza.
    tx.execute(
        "INSERT INTO document(uuid, kind, filename, mime, sha256, size, content, created_at)
         VALUES (?1, 'modelo_presentado', ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(uuid) DO NOTHING",
        params![
            uuid,
            filename,
            if bytes.starts_with(b"%PDF") {
                "application/pdf"
            } else {
                "text/plain"
            },
            sha,
            i64::try_from(bytes.len()).unwrap_or(0),
            bytes,
            now_str(now)
        ],
    )?;
    let doc: i64 = tx.query_row("SELECT id FROM document WHERE uuid = ?1", [&uuid], |r| {
        r.get(0)
    })?;
    tx.execute(
        "INSERT INTO filed_model(model, year, period, filename, file_sha256, source, boxes, document_id, imported_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            r.model,
            r.year,
            r.period,
            filename,
            sha,
            fuente,
            boxes_json(&r.boxes),
            doc,
            now_str(now)
        ],
    )?;
    let id = tx.last_insert_rowid();
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "filed_model.import",
        &format!("{} {} {}", r.model, r.year, r.period),
        &serde_json::json!({ "source": fuente, "boxes": r.boxes.len() }),
    )?;
    if let Some((old, _)) = sustituida {
        tx.execute("DELETE FROM filed_model WHERE id = ?1", [old])?;
    }
    tx.commit()?;
    Ok(match sustituida {
        Some((_, before)) => Stored::Copy(
            id,
            Copia::Sustituye {
                these: r.boxes.len(),
                before,
            },
        ),
        None => Stored::New(id),
    })
}

/// La declaración guardada con el mismo número de justificante que `r` (del mismo modelo y
/// periodo), leída otra vez de su documento.
fn same_declaration(v: &Vault, r: &Read) -> Result<Option<(i64, String, Read)>> {
    let Some(receipt) = r.receipt.as_deref() else {
        return Ok(None);
    };
    let mut st = v.connection().prepare(
        "SELECT f.id, f.filename, d.content FROM filed_model f JOIN document d ON d.id = f.document_id
         WHERE f.model = ?1 AND f.year = ?2 AND f.period = ?3 AND f.source = 'pdf'
         ORDER BY f.id",
    )?;
    let rows = st
        .query_map(params![r.model, r.year, r.period], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Vec<u8>>(2)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows.into_iter().find_map(|(id, name, content)| {
        read(&content)
            .filter(|o| o.receipt.as_deref() == Some(receipt))
            .map(|o| (id, name, o))
    }))
}

/// Entre dos ficheros con el mismo justificante y casillas distintas, ¿gana el nuevo? El
/// justificante oficial (el fichero que lleva su número en el nombre, como lo descarga la Sede)
/// y, si no se distingue, la lectura con más casillas; a igualdad, el que ya estaba.
fn wins_over(new_name: &str, new: &Read, old_name: &str, old: &Read) -> bool {
    let official = |r: &Read, name: &str| {
        r.source == "justificante" && r.receipt.as_deref().is_some_and(|n| name.contains(n))
    };
    match (official(new, new_name), official(old, old_name)) {
        (true, false) => true,
        (false, true) => false,
        _ => new.boxes.len() > old.boxes.len(),
    }
}

/// Casillas tecleadas a mano (las de la declaración del gestor). Sustituye a las que hubiera
/// tecleadas para el mismo modelo y periodo.
pub fn save_manual(
    v: &Vault,
    model: &str,
    year: i32,
    period: &str,
    boxes: &BTreeMap<u16, Money>,
    now: OffsetDateTime,
) -> Result<i64> {
    if !comparable(model) {
        return Err(AppError::Other(format!(
            "el modelo {model} no se puede comparar aún"
        )));
    }
    let key = format!("manual|{model}|{year}|{period}");
    let sha = hex::encode(Sha256::digest(key.as_bytes()));
    let conn = v.connection();
    conn.execute("DELETE FROM filed_model WHERE file_sha256 = ?1", [&sha])?;
    conn.execute(
        "INSERT INTO filed_model(model, year, period, filename, file_sha256, source, boxes, imported_at)
         VALUES (?1, ?2, ?3, 'tecleado a mano', ?4, 'manual', ?5, ?6)",
        params![model, year, period, sha, boxes_json(boxes), now_str(now)],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list(v: &Vault, year: Option<i32>) -> Result<Vec<Filed>> {
    let mut stmt = v.connection().prepare(
        "SELECT id, model, year, period, filename, source, boxes FROM filed_model
         WHERE ?1 IS NULL OR year = ?1 ORDER BY year, model, period, id",
    )?;
    let rows = stmt.query_map([year], |r| {
        Ok(Filed {
            id: r.get(0)?,
            model: r.get(1)?,
            year: r.get(2)?,
            period: r.get(3)?,
            filename: r.get(4)?,
            source: r.get(5)?,
            boxes: boxes_from(&r.get::<_, String>(6)?),
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

pub fn delete(v: &Vault, id: i64) -> Result<()> {
    let n = v
        .connection()
        .execute("DELETE FROM filed_model WHERE id = ?1", [id])?;
    if n == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Una casilla en la comparación.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Row {
    #[serde(rename = "box")]
    pub box_: u16,
    pub ours: Money,
    pub theirs: Option<Money>,
    /// `ours − theirs` (o lo nuestro si la gestoría no la tiene).
    pub diff: Money,
    /// Qué es la casilla (del diseño de registro), si se sabe.
    pub label: String,
    /// Si no cuadra: de dónde suele venir la diferencia, en lenguaje llano. Orientativo.
    pub explanation: Option<String>,
}

/// Un perceptor (190, 180, 193) o un tercero (347) en la comparación de una informativa.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LineRow {
    pub nif: String,
    pub name: String,
    /// Clave de percepción u operación («G01», «A», «B»…; vacía en el 180).
    pub key: String,
    pub ours: Option<Money>,
    pub theirs: Option<Money>,
    pub ours_withheld: Option<Money>,
    pub theirs_withheld: Option<Money>,
    /// Si no cuadra: qué pasa, en lenguaje llano.
    pub explanation: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Comparison {
    pub filed: Filed,
    pub rows: Vec<Row>,
    /// Casillas (y, en las informativas, perceptores o terceros) que no cuadran (más de 1
    /// céntimo de diferencia).
    pub mismatches: usize,
    pub warnings: Vec<String>,
    /// Informativas: perceptor a perceptor o tercero a tercero. Vacío si no se tiene el
    /// fichero de la declaración (del PDF sólo salen los totales).
    pub lines: Vec<LineRow>,
}

/// Trimestre o periodo de `draft` que corresponde al periodo del modelo presentado.
/// Los meses («01»…«12») de los modelos que pueden ser mensuales (303, 111, 115, 123, 349) son
/// los periodos 101…112 de [`taxpain_models::periodo`].
pub(crate) fn quarter_of(model: &str, period: &str) -> Result<u8> {
    if model == "390" || model == "200" || INFORMATIVAS.contains(&model) {
        return Ok(0);
    }
    let p = period.to_uppercase();
    let first = p.chars().next().and_then(|c| c.to_digit(10));
    match (first, p.chars().nth(1)) {
        (Some(n @ 1..=4), Some('T')) if model != "202" => Ok(u8::try_from(n).unwrap_or(1)),
        (Some(n @ 1..=3), Some('P')) if model == "202" => Ok(u8::try_from(n).unwrap_or(1)),
        _ if crate::models::MENSUALES.contains(&model) => taxpain_models::periodo::de_codigo(&p)
            .filter(|q| taxpain_models::periodo::mes(*q).is_some())
            .ok_or_else(|| {
                AppError::Other(format!(
                    "el periodo «{period}» del modelo {model} no se puede comparar"
                ))
            }),
        _ => Err(AppError::Other(format!(
            "el periodo «{period}» del modelo {model} no se puede comparar"
        ))),
    }
}

/// Casillas de las complementarias y rectificativas: (resultado, a deducir lo ingresado antes,
/// devoluciones ya acordadas). El borrador de TaxPain calcula el periodo entero, sin ellas.
fn amendment_boxes(model: &str) -> Option<(u16, u16, Option<u16>)> {
    match model {
        // [71] = [69] − [70] + [109].
        "303" => Some((71, 70, Some(109))),
        // [30] = [28] − [29].
        "111" => Some((30, 29, None)),
        // [05] = [03] − [04].
        "115" => Some((5, 4, None)),
        // [14] = [12] − [13].
        "123" => Some((14, 13, None)),
        // [19] = [17] − [18].
        "130" => Some((19, 18, None)),
        _ => None,
    }
}

/// Si `theirs` es una complementaria (lleva lo ingresado antes o devoluciones anteriores),
/// rehace su resultado como el del periodo entero y quita esas casillas, para compararlo con el
/// borrador. Devuelve si lo era.
pub(crate) fn whole_period(model: &str, theirs: &mut BTreeMap<u16, Money>) -> bool {
    let Some((result, deduct, refunds)) = amendment_boxes(model) else {
        return false;
    };
    let d = theirs.get(&deduct).copied().unwrap_or(Money::ZERO);
    let r = refunds
        .and_then(|b| theirs.get(&b).copied())
        .unwrap_or(Money::ZERO);
    if d.is_zero() && r.is_zero() {
        return false;
    }
    let total = theirs.get(&result).copied().unwrap_or(Money::ZERO) + d - r;
    theirs.insert(result, total);
    theirs.remove(&deduct);
    if let Some(b) = refunds {
        theirs.remove(&b);
    }
    true
}

/// Compara un modelo presentado con el que calcula TaxPain para el mismo periodo: casilla a
/// casilla, con lo que suele explicar cada diferencia, y en las informativas también perceptor a
/// perceptor (o tercero a tercero) si se tiene su fichero.
pub fn compare(v: &Vault, id: i64) -> Result<Comparison> {
    let filed = list(v, None)?
        .into_iter()
        .find(|f| f.id == id)
        .ok_or(AppError::NotFound)?;
    if !comparable(&filed.model) {
        return Err(AppError::Other(format!(
            "TaxPain no calcula el modelo {}: no se puede comparar",
            filed.model
        )));
    }
    let q = quarter_of(&filed.model, &filed.period)?;
    let draft = crate::models::draft(v, &filed.model, filed.year, q)?;
    // Sólo importes (no porcentajes como la [65] del 303), en los dos lados: una lectura del
    // asistente trae todas las casillas, también las de tipos. El 349 no tiene casillas: se
    // compara su importe total, la [02] («Importe de las operaciones intracomunitarias»). Las
    // informativas, su importe [02] y sus retenciones [03].
    let informativa = INFORMATIVAS.contains(&filed.model.as_str());
    let (amounts, draft_boxes) = if filed.model == "349" {
        (vec![2], vec![(2, draft.result)])
    } else if informativa {
        let (_, total, withheld) = ours_lines(v, &filed.model, filed.year)?;
        (vec![2, 3], vec![(2, total), (3, withheld)])
    } else {
        (known_boxes(&filed.model), draft.boxes.clone())
    };
    let ours: BTreeMap<u16, Money> = draft_boxes
        .iter()
        .copied()
        .filter(|(b, _)| amounts.contains(b))
        .collect();
    let mut theirs: BTreeMap<u16, Money> = filed
        .boxes
        .iter()
        .map(|(b, m)| (*b, *m))
        .filter(|(b, _)| amounts.contains(b))
        .collect();
    let amendment = whole_period(&filed.model, &mut theirs);
    let mut keys: Vec<u16> = ours.keys().chain(theirs.keys()).copied().collect();
    keys.sort_unstable();
    keys.dedup();
    let rows: Vec<Row> = keys
        .into_iter()
        .map(|b| {
            let o = ours.get(&b).copied().unwrap_or(Money::ZERO);
            let t = theirs.get(&b).copied();
            let diff = o - t.unwrap_or(Money::ZERO);
            Row {
                box_: b,
                ours: o,
                theirs: t,
                diff,
                label: box_label(&filed.model, filed.year, &filed.period, b),
                explanation: (diff.cents().abs() > 1)
                    .then(|| explain(&filed.model, b, o, t, &filed.source)),
            }
        })
        .filter(|r| !(r.ours.is_zero() && r.theirs.is_none_or(|t| t.is_zero())))
        .collect();
    let lines = if informativa {
        compare_lines(v, &filed)?
    } else {
        Vec::new()
    };
    let mismatches = rows.iter().filter(|r| r.diff.cents().abs() > 1).count()
        + lines.iter().filter(|l| l.explanation.is_some()).count();
    let mut warnings = draft.warnings;
    if amendment {
        warnings.insert(
            0,
            "Es una complementaria o rectificativa: el resultado presentado descuenta lo \
             ingresado antes en este periodo; se compara el total del periodo"
                .into(),
        );
    }
    if filed.source == "pdf" {
        warnings.insert(
            0,
            "Casillas leídas del PDF: comprueba con el documento que estén todas y bien".into(),
        );
    }
    if informativa && lines.is_empty() && filed.source != "fichero" {
        warnings.push(
            "Sólo se comparan los totales: para comparar perceptor a perceptor hace falta el \
             fichero de la declaración (el .txt de la AEAT)"
                .into(),
        );
    }
    Ok(Comparison {
        filed,
        rows,
        mismatches,
        warnings,
        lines,
    })
}

/// Qué es una casilla: su descripción en el diseño de registro, sin el camino de apartados
/// («Liquidación (3) - Régimen General - IVA Devengado - …»): sus dos últimos tramos.
fn box_label(model: &str, year: i32, period: &str, b: u16) -> String {
    match model {
        "347" | "190" | "180" | "193" | "349" if b == 2 => {
            return match model {
                "190" => "Percepciones íntegras",
                "180" | "193" => "Base de las retenciones",
                _ => "Importe de las operaciones",
            }
            .into();
        }
        "190" | "180" | "193" if b == 3 => return "Retenciones e ingresos a cuenta".into(),
        "200" => return label_200(b).into(),
        _ => {}
    }
    let specs = [
        taxpain_models::dr_versions::spec_for(model, year, period),
        spec(model),
    ];
    let desc = specs.into_iter().flatten().find_map(|s| {
        s.pages
            .iter()
            .flat_map(|p| p.fields.iter())
            .find(|f| f.is_amount() && f.box_.or_else(|| box_of(model, f)) == Some(b))
            .map(|f| f.desc.clone())
    });
    let Some(d) = desc else {
        return String::new();
    };
    let parts: Vec<&str> = d.split(" - ").map(str::trim).collect();
    let tail = parts[parts.len().saturating_sub(2)..].join(" · ");
    // Sin la casilla entre corchetes del final (ya se enseña aparte).
    match tail.rfind('[') {
        Some(i) if tail.ends_with(']') => tail[..i].trim_end().to_owned(),
        _ => tail,
    }
}

/// Las casillas del 200 que calcula TaxPain (`taxpain_models::m200`).
fn label_200(b: u16) -> &'static str {
    match b {
        500 => "Resultado de la cuenta de pérdidas y ganancias",
        301 | 302 => "Impuesto sobre Sociedades (corrección)",
        501 => "Resultado antes del Impuesto",
        1815 => "Multas y sanciones (no deducibles)",
        417 | 418 => "Otras correcciones al resultado",
        550 => "Base imponible antes de la reserva de capitalización y las bases negativas",
        1032 => "Reserva de capitalización",
        547 => "Compensación de bases negativas de periodos anteriores",
        552 => "Base imponible",
        562 => "Cuota íntegra",
        582 => "Cuota líquida positiva",
        592 => "Cuota del ejercicio",
        1766 | 599 => "Retenciones e ingresos a cuenta",
        601 => "Pagos fraccionados",
        603 | 605 => "Pagos fraccionados (otros)",
        611 => "Cuota diferencial",
        1586 => "Resultado de la declaración",
        _ => "",
    }
}

/// De dónde suele venir la diferencia en una casilla. Orientativo: TaxPain lo dice por el tipo
/// de casilla, no porque sepa qué factura falta.
pub(crate) fn explain(
    model: &str,
    b: u16,
    ours: Money,
    theirs: Option<Money>,
    source: &str,
) -> String {
    let diff = ours - theirs.unwrap_or(Money::ZERO);
    let quien = if diff.cents() > 0 {
        format!("TaxPain calcula {} más", diff.abs())
    } else {
        format!("TaxPain calcula {} menos", diff.abs())
    };
    let causa: &str = match (model, b) {
        _ if theirs.is_none() => {
            "la gestoría no rellenó esta casilla (o no se ha leído del documento): mira si \
             le falta esa operación o si la puso en otra casilla"
        }
        _ if ours.is_zero() => {
            "TaxPain no tiene nada aquí: faltan en TaxPain las facturas, nóminas o asientos \
             de esa operación, o la gestoría la puso a mano"
        }
        ("303", 110 | 78 | 87) => {
            "es la compensación de periodos anteriores: depende de los 303 anteriores; si \
             TaxPain no los tiene presentados, encadena los que calcula con sus datos"
        }
        ("303", 1..=27 | 150..=170) => {
            "IVA repercutido: compara las facturas emitidas del periodo (fecha de devengo, \
             rectificativas, tipo de IVA, autorrepercusiones de compras a la UE o con \
             inversión del sujeto pasivo)"
        }
        ("303", 28..=45) => {
            "IVA soportado deducible: compara las facturas recibidas (las que se deducen en \
             otro periodo, las que no tienen IVA deducible, bienes de inversión, prorrata)"
        }
        ("303", 59 | 60 | 120 | 122) => {
            "operaciones sin IVA español (entregas a la UE, exportaciones, no sujetas o con \
             inversión del sujeto pasivo): revisa cómo está marcado el IVA de esas facturas"
        }
        ("303", 46 | 64 | 66 | 69 | 71) => {
            "es un resultado: la diferencia viene de las casillas de arriba"
        }
        ("390", _) => {
            "el resumen anual suma los 303 del año: compara primero los trimestres; si \
             cuadran, la diferencia es de una rectificación o de un ajuste del 4T"
        }
        ("111", 1..=6) => {
            "rendimientos del trabajo: compara las nóminas del periodo (fecha de pago, \
             retribuciones en especie, administradores)"
        }
        ("111", 7..=9) => {
            "actividades profesionales: facturas recibidas con retención, por la fecha de \
             pago (no la de la factura)"
        }
        ("111", _) | ("115", _) | ("123", _) => {
            "las retenciones van por la fecha de pago o de exigibilidad: mira los pagos del \
             periodo y si alguno está en otro"
        }
        ("130", _) => {
            "ingresos y gastos acumulados desde el 1 de enero: la diferencia puede venir de \
             un trimestre anterior"
        }
        ("202", _) => {
            "pago fraccionado: depende de la base (último 200 o resultado de los meses del \
             año) y del tipo; revisa los datos del 202 en los ajustes de los modelos"
        }
        ("200", _) => {
            "Impuesto sobre Sociedades: TaxPain no calcula todas las correcciones, \
             deducciones ni bases negativas por año; revisa los ajustes extracontables"
        }
        ("349", _) => {
            "operaciones con la UE: compara las facturas con NIF-IVA de otro Estado miembro \
             (entregas, servicios y adquisiciones) y su fecha de devengo"
        }
        ("347", _) => {
            "operaciones con terceros de más de 3.005,06 € al año, IVA incluido: mira el \
             detalle por tercero"
        }
        ("190" | "180" | "193", _) => "mira el detalle por perceptor",
        _ => "revisa las operaciones de esa casilla",
    };
    let fuente = if source == "pdf" {
        " (casilla leída del PDF: comprueba que se leyó bien)"
    } else {
        ""
    };
    format!("{quien}: {causa}{fuente}.")
}

/// Un perceptor o tercero: (NIF, nombre, clave, importe, retención).
type Linea = (String, String, String, Money, Option<Money>);

/// Lo que calcula TaxPain de una informativa: sus líneas, el importe total y las retenciones.
fn ours_lines(v: &Vault, model: &str, year: i32) -> Result<(Vec<Linea>, Money, Money)> {
    Ok(match model {
        "347" => {
            let r = crate::models::result_347(v, year)?;
            let lines = r
                .lines
                .iter()
                .map(|l| {
                    (
                        l.nif.clone(),
                        l.name.clone(),
                        l.key.code().to_owned(),
                        l.total,
                        None,
                    )
                })
                .collect();
            (lines, r.total, Money::ZERO)
        }
        "190" => {
            let r = crate::models::result_190(v, year)?;
            let lines = r
                .lines
                .iter()
                .map(|l| {
                    let (k, sub) = l.effective_key().codes();
                    let key = if sub == "00" {
                        k.to_owned()
                    } else {
                        format!("{k}{sub}")
                    };
                    (
                        l.nif.clone(),
                        l.name.clone(),
                        key,
                        l.gross,
                        Some(l.withheld),
                    )
                })
                .collect();
            (lines, r.total_gross, r.total_withheld)
        }
        "180" => {
            let r = crate::models::result_180(v, year)?;
            let lines = r
                .lines
                .iter()
                .map(|l| {
                    (
                        l.nif.clone(),
                        l.name.clone(),
                        String::new(),
                        l.base,
                        Some(l.withheld),
                    )
                })
                .collect();
            (lines, r.total_base, r.total_withheld)
        }
        _ => {
            let r = crate::models::result_193(v, year)?;
            let lines = r
                .lines
                .iter()
                .map(|l| {
                    (
                        l.nif.clone(),
                        l.name.clone(),
                        l.key.to_string(),
                        l.gross,
                        Some(l.withheld),
                    )
                })
                .collect();
            (lines, r.total_base, r.total_withheld)
        }
    })
}

/// Las líneas que presentó la gestoría, leídas del fichero guardado (del PDF no salen).
fn theirs_lines(v: &Vault, id: i64, model: &str) -> Result<Option<Vec<Linea>>> {
    let content: Option<Vec<u8>> = v
        .connection()
        .query_row(
            "SELECT d.content FROM filed_model f JOIN document d ON d.id = f.document_id
             WHERE f.id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let Some(f) = content.and_then(|c| read_informative(&c)) else {
        return Ok(None);
    };
    if f.model != model {
        return Ok(None);
    }
    Ok(Some(
        f.lines
            .into_iter()
            .map(|l| {
                let key = if model == "180" { String::new() } else { l.key };
                (l.nif, l.name, key, l.amount, l.withheld)
            })
            .collect(),
    ))
}

/// NIF para emparejar: sin espacios ni guiones, en mayúsculas y sin el «ES» delante.
fn nif_key(n: &str) -> String {
    crate::revision::expediente::nif_clave(n)
}

/// Perceptor a perceptor (o tercero a tercero), por NIF y clave.
fn compare_lines(v: &Vault, filed: &Filed) -> Result<Vec<LineRow>> {
    let Some(theirs) = theirs_lines(v, filed.id, &filed.model)? else {
        return Ok(Vec::new());
    };
    let (ours, _, _) = ours_lines(v, &filed.model, filed.year)?;
    type Acc = (
        String,
        Option<Money>,
        Option<Money>,
        Option<Money>,
        Option<Money>,
    );
    let mut by: BTreeMap<(String, String), Acc> = BTreeMap::new();
    let add = |m: &mut Option<Money>, x: Money| *m = Some(m.unwrap_or(Money::ZERO) + x);
    for (nif, name, key, amount, withheld) in ours {
        let e = by
            .entry((nif_key(&nif), key.to_uppercase()))
            .or_insert_with(|| (name.clone(), None, None, None, None));
        add(&mut e.1, amount);
        if let Some(w) = withheld {
            add(&mut e.3, w);
        }
    }
    for (nif, name, key, amount, withheld) in theirs {
        let e = by
            .entry((nif_key(&nif), key.to_uppercase()))
            .or_insert_with(|| (name.clone(), None, None, None, None));
        if e.0.is_empty() {
            e.0 = name;
        }
        add(&mut e.2, amount);
        if let Some(w) = withheld {
            add(&mut e.4, w);
        }
    }
    let differs = |a: Option<Money>, b: Option<Money>| {
        (a.unwrap_or(Money::ZERO) - b.unwrap_or(Money::ZERO))
            .cents()
            .abs()
            > 1
    };
    let mut out: Vec<LineRow> = by
        .into_iter()
        .map(|((nif, key), (name, o, t, ow, tw))| {
            let explanation = match (o, t) {
                (None, Some(_)) => Some(
                    "La gestoría lo declara y TaxPain no: faltan en TaxPain sus facturas, \
                     nóminas o pagos de ese año, o tiene otro NIF o clave"
                        .to_owned(),
                ),
                (Some(_), None) => Some(
                    "TaxPain lo declararía y la gestoría no: puede que se le olvidara o que lo \
                     declare con otro NIF o clave"
                        .to_owned(),
                ),
                _ if differs(o, t) => Some(format!(
                    "Importes distintos ({} en TaxPain, {} en lo presentado): mira las \
                     facturas o pagos de ese perceptor en el año (fecha de pago, \
                     rectificativas, IVA incluido en el 347)",
                    o.unwrap_or(Money::ZERO),
                    t.unwrap_or(Money::ZERO)
                )),
                _ if differs(ow, tw) => Some(format!(
                    "Retenciones distintas ({} en TaxPain, {} en lo presentado): mira el tipo \
                     de retención de sus facturas o nóminas",
                    ow.unwrap_or(Money::ZERO),
                    tw.unwrap_or(Money::ZERO)
                )),
                _ => None,
            };
            LineRow {
                nif,
                name,
                key,
                ours: o,
                theirs: t,
                ours_withheld: ow,
                theirs_withheld: tw,
                explanation,
            }
        })
        .collect();
    // Primero lo que no cuadra.
    out.sort_by_key(|l| l.explanation.is_none());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complementaria_como_periodo_entero() {
        let e = |n: i64| Money::from_euros(n, 0);
        // 303: [71] = [69] − [70] + [109].
        let mut t: BTreeMap<u16, Money> = [(69, e(1200)), (70, e(1000)), (71, e(200))].into();
        assert!(whole_period("303", &mut t));
        assert_eq!(t, [(69, e(1200)), (71, e(1200))].into());
        let mut t: BTreeMap<u16, Money> = [(71, e(-50)), (109, e(30))].into();
        assert!(whole_period("303", &mut t));
        assert_eq!(t, [(71, e(-80))].into());
        // 111: [30] = [28] − [29].
        let mut t: BTreeMap<u16, Money> = [(28, e(500)), (29, e(300)), (30, e(200))].into();
        assert!(whole_period("111", &mut t));
        assert_eq!(t.get(&30), Some(&e(500)));
        // Una declaración normal no se toca.
        let mut t: BTreeMap<u16, Money> = [(71, e(200))].into();
        assert!(!whole_period("303", &mut t));
        assert!(!whole_period("390", &mut t));
    }

    #[test]
    fn importe_espanol() {
        assert_eq!(amount("1.234,56"), Some(Money::from_cents(123_456)));
        assert_eq!(amount("-12,00"), Some(Money::from_cents(-1200)));
        assert_eq!(amount("12.00"), None);
        assert_eq!(amount("2025"), None);
    }

    #[test]
    fn pdf_con_pares_casilla_importe() {
        let t = "Agencia Tributaria\nModelo 303 IVA Autoliquidación\nEjercicio 2025 Periodo 1T\n\
                 Régimen general 01 1.000,00 02 21,00 03 210,00\n27 210,00 [28] 300,00 29 63,00 \
                 Resultado 71 147,00 Página 1 de 3";
        let r = read_pdf_text(t).unwrap_or_else(|| panic!("no leído"));
        assert_eq!(
            (r.model.as_str(), r.year, r.period.as_str()),
            ("303", 2025, "1T")
        );
        assert_eq!(r.boxes.get(&1), Some(&Money::from_cents(100_000)));
        assert_eq!(r.boxes.get(&28), Some(&Money::from_cents(30_000)));
        assert_eq!(r.boxes.get(&71), Some(&Money::from_cents(14_700)));
        // «de 3» no es una casilla con importe.
        assert!(!r.boxes.contains_key(&3) || r.boxes[&3] == Money::from_cents(21_000));
    }

    #[test]
    fn el_ejercicio_del_pdf_no_es_el_ano_de_presentacion() {
        // pdf-extract parte la etiqueta y el valor en dos líneas.
        let t = "Agencia Tributaria\nModelo 303\nEjercicio\n2024\nPeríodo\n4T\n\
                 Fecha de presentación 29/01/2025\n27 210,00 71 147,00";
        let r = read_pdf_text(t).unwrap_or_else(|| panic!("no leído"));
        assert_eq!(
            (r.year, r.period.as_str(), r.year_guessed),
            (2024, "4T", false)
        );
        // Cifras sueltas en casillas.
        let t = "Agencia Tributaria Modelo 303 Ejercicio 2 0 2 4 Periodo 3T 27 210,00";
        assert_eq!(read_pdf_text(t).map(|r| r.year), Some(2024));
        // Sin «Ejercicio»: un 4T presentado en enero es del año anterior.
        let t = "AEAT Modelo 111 cuarto trimestre presentado el 20/01/2025 01 2 02 1.000,00";
        let r = read_pdf_text(t).unwrap_or_else(|| panic!("no leído"));
        assert_eq!(
            (r.year, r.period.as_str(), r.year_guessed),
            (2024, "4T", true)
        );
        // Sin fechas: el año suelto más antiguo (no el de la orden «HAC/819/2025»).
        let t = "AEAT Modelo 303 2025 2024 Orden HAC/819/2023 Periodo 2T 27 210,00";
        assert_eq!(read_pdf_text(t).map(|r| r.year), Some(2024));
    }

    #[test]
    fn periodo_del_draft() {
        assert_eq!(quarter_of("303", "3T").ok(), Some(3));
        assert_eq!(quarter_of("202", "2P").ok(), Some(2));
        assert_eq!(quarter_of("390", "0A").ok(), Some(0));
        // Los meses del 303 (gran empresa, REDEME) son los periodos 101…112.
        assert_eq!(quarter_of("303", "01").ok(), Some(101));
        assert_eq!(quarter_of("111", "12").ok(), Some(112));
        assert!(quarter_of("130", "01").is_err());
        assert!(quarter_of("303", "13").is_err());
        assert_eq!(quarter_of("190", "0A").ok(), Some(0));
    }
}
