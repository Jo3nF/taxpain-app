//! Lectura de la carpeta de la empresa con el asistente («Full IA»).
//!
//! El asistente dice **qué es** cada fichero y **dónde está** cada dato; los importes de los
//! libros y del diario se siguen leyendo con los importadores de siempre:
//! - en una hoja de cálculo señala la fila de títulos, la primera de datos y qué columna es cada
//!   campo; TaxPain pone títulos conocidos a esas columnas y la importa con [`crate::libros`] o
//!   [`crate::import`] (los importes salen de las celdas, no de la IA);
//! - en un PDF o una imagen (factura, modelo, escritura, tarjeta NIF…) extrae los datos, que
//!   quedan para revisar: las facturas recibidas, pendientes en Recibidas; las emitidas, en el
//!   libro de emitidas con aviso; los modelos, para comparar; los documentos, con sus datos.
//!
//! Con «Full IA» los PDF y las imágenes se mandan tal cual (no se pueden anonimizar) con el
//! permiso expreso del usuario; el texto de las hojas sí sale anonimizado. Las lecturas se
//! guardan por huella del fichero para no pagar dos veces la misma.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use taxpain_core::money::Money;
use taxpain_vault::Vault;
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;

/// Ajustes de la lectura con IA (por empresa).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// «Full IA»: el asistente puede leer todo, también PDF e imágenes tal cual.
    pub full: bool,
    /// Modelo para leer documentos (vacío = el del asistente).
    #[serde(default)]
    pub model: Option<String>,
    /// Cuándo se dio el permiso.
    #[serde(default)]
    pub accepted_at: Option<String>,
}

const SETTINGS_KEY: &str = "lectura_ia";

pub fn settings(v: &Vault) -> Result<Settings> {
    Ok(v.setting(SETTINGS_KEY)?.unwrap_or_default())
}

pub fn save_settings(v: &Vault, s: &Settings, now: OffsetDateTime) -> Result<Settings> {
    let mut s = s.clone();
    if s.full && s.accepted_at.is_none() {
        s.accepted_at = Some(now_str(now));
    }
    if !s.full {
        s.accepted_at = None;
    }
    v.set_setting(SETTINGS_KEY, &s)?;
    taxpain_vault::audit::append(
        v.connection(),
        &now_str(now),
        taxpain_vault::audit::Actor::User,
        "lectura_ia.settings",
        if s.full { "full" } else { "local" },
        &serde_json::json!({}),
    )?;
    Ok(s)
}

/// Qué ve el asistente de un fichero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Excerpt {
    /// Hoja de cálculo como texto (se anonimiza).
    Sheet(String),
    /// PDF o imagen tal cual (sólo con «Full IA»).
    File { media_type: String },
}

const SHEET_ROWS: usize = 40;
const SHEET_COLS: usize = 26;

fn col_letter(i: usize) -> String {
    let mut n = i + 1;
    let mut s = String::new();
    while n > 0 {
        let r = (n - 1) % 26;
        s.insert(0, char::from(b'A' + u8::try_from(r).unwrap_or(0)));
        n = (n - 1) / 26;
    }
    s
}

fn col_index(letter: &str) -> Option<usize> {
    let l = letter.trim().to_ascii_uppercase();
    if l.is_empty() || !l.bytes().all(|b| b.is_ascii_uppercase()) {
        return l.parse::<usize>().ok().and_then(|n| n.checked_sub(1));
    }
    let mut n = 0usize;
    for b in l.bytes() {
        n = n * 26 + usize::from(b - b'A' + 1);
    }
    n.checked_sub(1)
}

/// Las primeras filas de una hoja, numeradas y con las columnas por letra.
pub fn sheet_text(rows: &[Vec<String>]) -> String {
    sheet_text_rows(rows, SHEET_ROWS)
}

/// Como [`sheet_text`], con `max` filas.
pub fn sheet_text_rows(rows: &[Vec<String>], max: usize) -> String {
    sheet_text_cut(rows, max, 40).0
}

/// Lo que se queda fuera al pasar una hoja a texto con [`sheet_text_cut`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SheetCut {
    /// Las filas que van y las que tiene la hoja (hasta la última con algo), si no van todas.
    pub rows: Option<(usize, usize)>,
    /// Hay algo más allá de la columna Z.
    pub columns: bool,
    /// Alguna celda se ha recortado (los caracteres que caben en una).
    pub cells: Option<usize>,
}

impl SheetCut {
    /// Lo que se ha quedado fuera, en frases para el texto de la hoja.
    pub(crate) fn notes(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some((van, total)) = self.rows {
            out.push(format!(
                "(Van {van} de sus {total} filas: las demás no se han leído.)"
            ));
        }
        if self.columns {
            out.push(format!(
                "(Sólo van las columnas de la A a la {}.)",
                col_letter(SHEET_COLS - 1)
            ));
        }
        if let Some(n) = self.cells {
            out.push(format!(
                "(Las celdas de más de {n} caracteres van recortadas.)"
            ));
        }
        out
    }
}

/// Como [`sheet_text_rows`], con `max_cell` caracteres por celda, y lo que se queda fuera.
pub(crate) fn sheet_text_cut(
    rows: &[Vec<String>],
    max: usize,
    max_cell: usize,
) -> (String, SheetCut) {
    let mut out = String::new();
    let mut cut = SheetCut::default();
    for (i, r) in rows.iter().take(max).enumerate() {
        let cells: Vec<String> = r
            .iter()
            .take(SHEET_COLS)
            .enumerate()
            .filter(|(_, c)| !c.trim().is_empty())
            .map(|(j, c)| {
                let c = c.trim();
                if c.chars().count() > max_cell {
                    cut.cells = Some(max_cell);
                }
                let c: String = c.chars().take(max_cell).collect();
                format!("{}={c}", col_letter(j))
            })
            .collect();
        if !cells.is_empty() {
            out.push_str(&format!("fila {}: {}\n", i + 1, cells.join(" | ")));
        }
    }
    let lleno = |r: &Vec<String>| r.iter().any(|c| !c.trim().is_empty());
    let total = rows.iter().rposition(lleno).map_or(0, |i| i + 1);
    if total > max {
        cut.rows = Some((max, total));
    }
    cut.columns = rows
        .iter()
        .take(max)
        .any(|r| r.iter().skip(SHEET_COLS).any(|c| !c.trim().is_empty()));
    (out, cut)
}

/// El fichero tal como se le manda al asistente: un PDF guardado dentro de un sobre MIME (desde
/// el correo o la web del banco, como los de BBVA) va sin el sobre.
pub fn attachment_bytes(filename: &str, bytes: &[u8]) -> Vec<u8> {
    pdf_inside(filename, bytes).unwrap_or_else(|| bytes.to_vec())
}

/// El PDF que va dentro de un sobre MIME, si el fichero es eso.
fn pdf_inside(filename: &str, bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.starts_with(b"%PDF") {
        return None;
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]).to_ascii_lowercase();
    (filename.to_lowercase().ends_with(".pdf") || head.contains("content-type:"))
        .then(|| crate::extractos::mime::pdf_dentro(bytes))
        .flatten()
}

pub fn media_type(filename: &str, bytes: &[u8]) -> Option<&'static str> {
    let l = filename.to_lowercase();
    if bytes.starts_with(b"%PDF") || pdf_inside(filename, bytes).is_some() {
        Some("application/pdf")
    } else if l.ends_with(".png") {
        Some("image/png")
    } else if l.ends_with(".jpg") || l.ends_with(".jpeg") {
        Some("image/jpeg")
    } else if l.ends_with(".webp") {
        Some("image/webp")
    } else if l.ends_with(".gif") {
        Some("image/gif")
    } else {
        None
    }
}

/// Qué se le manda al asistente de este fichero (`None` si no se puede leer con él).
pub fn excerpt(filename: &str, bytes: &[u8]) -> Option<Excerpt> {
    let l = filename.to_lowercase();
    if [".xlsx", ".xlsm", ".xls", ".ods", ".csv"]
        .iter()
        .any(|e| l.ends_with(e))
    {
        let hojas = crate::import::sheets(filename, bytes).ok()?;
        let primera = hojas.first()?;
        let mut t = String::new();
        if hojas.len() > 1 {
            t.push_str(&format!(
                "El libro tiene {} hojas: {}. Esta es la primera; las columnas que digas valen \
                 para todas las hojas con los mismos títulos.\n",
                hojas.len(),
                hojas
                    .iter()
                    .map(|h| h.nombre.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        t.push_str(&sheet_text(&primera.filas));
        return (!t.trim().is_empty()).then_some(Excerpt::Sheet(t));
    }
    media_type(filename, bytes).map(|m| Excerpt::File {
        media_type: m.to_owned(),
    })
}

/// Instrucciones del asistente para leer un fichero de la carpeta.
pub const PROMPT: &str = "\
Eres el lector de documentos de TaxPain, una app de contabilidad e impuestos en España. Te paso \
un fichero de la carpeta de MI EMPRESA (lo que me da la gestoría). Di qué es y extrae sus datos. \
Responde SOLO con un objeto JSON, sin texto alrededor, con estas claves (omite las que no \
apliquen; null si un dato no aparece; no inventes nada):\n\
- \"tipo\": uno de diario, libro_emitidas, libro_recibidas, sumas_saldos, extracto_bancario, \
factura_recibida, factura_emitida, modelo_aeat, escritura_constitucion, escritura_poderes, \
escritura_modificacion, estatutos, tarjeta_nif, censal_036, alta_roi, certificado_aeat, \
certificado_seguridad_social, alta_seguridad_social, registro_mercantil, cuentas_anuales, iae, \
acta_junta, contrato_alquiler, seguro, licencia, nomina, otro.\n\
- \"descripcion\": una frase con lo que es (periodo, emisor…).\n\
- \"hoja\" (sólo hojas de cálculo de diario, libros de facturas o extractos bancarios): \
{\"fila_titulos\": número de la fila con los títulos (0 si no hay), \"primera_fila\": primera \
fila de datos, \"columnas\": {campo: letra}} con estos campos si existen. Diario: fecha, \
asiento, documento, concepto, cuenta (o cuenta_debe y cuenta_haber si la cuenta va en dos \
columnas), nombre_cuenta, debe, haber. Libros: fecha, fecha_registro, serie, numero, nif, \
nombre, pais, base, tipo_iva, cuota_iva, recargo, retencion, total. Extractos: fecha, \
fecha_valor, concepto (y concepto_2, beneficiario u observaciones si el texto va en más \
columnas), importe (con signo) o cargo y abono, saldo, divisa. Si el libro tiene varias hojas \
con los mismos títulos (una por mes, o las de adquisiciones y no deducibles), las columnas \
valen para todas.\n\
- \"extracto\" (extractos bancarios en PDF o en foto): {\"cuenta\": {\"iban\", \"banco\", \
\"titular\", \"divisa\" (ISO 4217), \"clase\" (cuenta, tarjeta_credito, tarjeta_debito o \
monedero), \"desde\", \"hasta\" (AAAA-MM-DD), \"saldo_inicial\", \"saldo_final\"}, \
\"movimientos\": [[fecha, fecha_valor, concepto, importe, saldo], …]} con TODOS los movimientos \
en el orden del documento: fechas AAAA-MM-DD, importes con punto decimal y signo (negativo si \
sale dinero de la cuenta), saldo después del movimiento (null si no lo pone). Si hay varias \
cuentas o divisas, \"extractos\": [ {\"cuenta\": …, \"movimientos\": …}, … ].\n\
- \"factura\" (facturas): {\"nif_emisor\", \"nombre_emisor\", \"pais_emisor\" (código ISO 3166-1 \
de dos letras, como US; sólo si el emisor no es de España), \"nif_destinatario\", \
\"nombre_destinatario\", \"numero\", \"fecha\" (AAAA-MM-DD), \"base\", \"iva\", \"retencion\" (positiva), \"total\" (importes con punto \
decimal), \"moneda\" (código ISO 4217, como USD; sólo si no es EUR), \"tipos\": [{\"base\", \"tipo\", \"cuota\"}]}. \
En \"nombre_destinatario\" va a quien está hecha la factura aunque no tenga NIF (su nombre o, si \
sólo sale eso, su correo).\n\
- \"modelo\" (modelos de la AEAT): {\"modelo\": \"303\", \"ejercicio\": 2025, \"periodo\": \"1T\" \
(o 0A, 1P, 01…), \"casillas\": {\"01\": \"1234.56\", …}} con las casillas que tengan importe, \
\"presentado\" (fecha de presentación del justificante, AAAA-MM-DD), en el 200 \"inicio\" y \
\"fin\" del periodo impositivo (AAAA-MM-DD) si no es el año natural, y, en los resúmenes anuales \
(190, 180, 193, 347, 349), \"registros\": [{\"nif\", \"nombre\", \"clave\", \"importe\", \
\"retencion\"}] con una línea por perceptor o tercero; y \"tipo\" (\"complementaria\" o \
\"sustitutiva\") si la declaración lo es.\n\
- \"registro\" (justificantes del Registro Mercantil: depósito de cuentas o legalización de \
libros, y cuentas anuales): {\"tramite\": \"deposito_cuentas\" o \"legalizacion_libros\", \
\"ejercicio\", \"presentado\" (fecha de presentación en el Registro, AAAA-MM-DD), \
\"resultado\" (resultado del ejercicio de la cuenta de pérdidas y ganancias), \
\"cifra_negocios\"}.\n\
- \"acta\" (actas de la junta o decisiones del socio único que aprueban las cuentas o mueven \
reservas): {\"aprueba_cuentas\" (true si aprueba las cuentas y la aplicación del resultado; false \
si es otra decisión, como un dividendo con cargo a reservas o una dotación posterior), \
\"ejercicio\" (el de las cuentas que se aprueban o al que se refiere), \"fecha\" (AAAA-MM-DD), \
\"resultado\" (el que se reparte; negativo si son pérdidas; null si no se reparte un resultado), \
\"reserva_legal\", \"reservas_voluntarias\", \"reserva_nivelacion\", \"reserva_capitalizacion\", \
\"dividendos\", \"compensacion_perdidas\", \"remanente\"} con lo que va a cada partida del \
ejercicio del acta (no el del año anterior; null si no va nada; negativo si sale de ella).\n\
- \"nomina\" (nóminas): {\"nif_trabajador\", \"nombre_trabajador\", \"periodo\" (AAAA-MM), \
\"bruto\" (total devengado), \"retencion\" (la del IRPF, positiva), \"tipo_retencion\" (el \
porcentaje del IRPF)}.\n\
- \"empresa\" (documentos de la empresa): {\"nif\", \"name\", \"street\", \"post_code\", \"town\", \
\"constitution_date\", \"capital\", \"cnae\", \"iae\", \"vat_number\", \"administrator\", \"notary\", \
\"protocol\", \"registry\", \"date\"}.\n\
Los datos personales pueden venir como marcadores ([NIF_1], [DATO_2]…): cópialos tal cual.";

/// Tipos de fichero que entiende la importación.
pub const TIPOS: [&str; 27] = [
    "diario",
    "libro_emitidas",
    "libro_recibidas",
    "sumas_saldos",
    "extracto_bancario",
    "factura_recibida",
    "factura_emitida",
    "modelo_aeat",
    "escritura_constitucion",
    "escritura_poderes",
    "escritura_modificacion",
    "estatutos",
    "tarjeta_nif",
    "censal_036",
    "alta_roi",
    "certificado_aeat",
    "certificado_seguridad_social",
    "alta_seguridad_social",
    "registro_mercantil",
    "cuentas_anuales",
    "iae",
    "acta_junta",
    "contrato_alquiler",
    "seguro",
    "licencia",
    "nomina",
    "otro",
];

/// Lectura del asistente.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    pub tipo: String,
    #[serde(default)]
    pub descripcion: String,
    #[serde(default)]
    pub hoja: Option<SheetPlan>,
    #[serde(default)]
    pub factura: Option<serde_json::Value>,
    #[serde(default)]
    pub modelo: Option<serde_json::Value>,
    #[serde(default)]
    pub empresa: Option<serde_json::Value>,
    /// Extractos bancarios que no reconoce ningún lector: la cuenta y sus movimientos (ver
    /// `extractos::ia`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extracto: Option<serde_json::Value>,
    /// Justificante del Registro Mercantil (depósito de cuentas, legalización de libros) o
    /// cuentas anuales: trámite, ejercicio, fecha de presentación y cifras (ver la revisión).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registro: Option<serde_json::Value>,
    /// Acta de la junta (o decisión del socio único) que aprueba las cuentas: el ejercicio, la
    /// fecha y la aplicación del resultado por partidas (ver la revisión).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acta: Option<serde_json::Value>,
    /// Nómina: el trabajador (NIF y nombre), el mes, el bruto y la retención del IRPF (ver la
    /// revisión).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nomina: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SheetPlan {
    #[serde(default)]
    pub fila_titulos: usize,
    #[serde(default)]
    pub primera_fila: usize,
    #[serde(default)]
    pub columnas: BTreeMap<String, String>,
}

/// JSON de la respuesta (lo primero entre llaves).
pub fn parse_reply(reply: &str) -> Option<Reading> {
    let json = reply
        .find('{')
        .zip(reply.rfind('}'))
        .and_then(|(a, b)| reply.get(a..=b))?;
    reading_from_value(&serde_json::from_str(json).ok()?)
}

/// Lectura a partir de su JSON, con manga ancha: `null` en cualquier dato opcional vale lo
/// mismo que no ponerlo (las instrucciones de Claude Code piden `null` para lo que no aparece),
/// los números de fila pueden venir como texto y los bloques que no son objetos se ignoran.
/// `None` sólo si no es un objeto o no dice qué tipo de fichero es.
pub fn reading_from_value(v: &serde_json::Value) -> Option<Reading> {
    use serde_json::Value;
    let o = v.as_object()?;
    let tipo: String = o
        .get("tipo")
        .and_then(Value::as_str)?
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c == ' ' || c == '-' { '_' } else { c })
        .collect();
    if tipo.is_empty() {
        return None;
    }
    let block = |k: &str| o.get(k).filter(|b| b.is_object()).cloned();
    Some(Reading {
        tipo,
        descripcion: o.get("descripcion").and_then(scalar).unwrap_or_default(),
        hoja: o.get("hoja").and_then(Value::as_object).map(|h| SheetPlan {
            fila_titulos: row_number(h.get("fila_titulos")),
            primera_fila: row_number(h.get("primera_fila")),
            columnas: h
                .get("columnas")
                .and_then(Value::as_object)
                .map(|c| {
                    c.iter()
                        .filter_map(|(k, l)| Some((k.trim().to_lowercase(), scalar(l)?)))
                        .collect()
                })
                .unwrap_or_default(),
        }),
        factura: block("factura"),
        modelo: block("modelo"),
        empresa: block("empresa"),
        registro: block("registro"),
        acta: block("acta"),
        nomina: block("nomina"),
        // Con varias cuentas puede venir como lista («extractos»).
        extracto: block("extracto").or_else(|| {
            o.get("extractos")
                .filter(|l| l.is_array())
                .map(|l| serde_json::json!({ "extractos": l }))
        }),
    })
}

/// Texto o número como texto (sin `null` ni vacíos).
fn scalar(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.trim().to_owned()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
    .filter(|s| !s.is_empty() && s != "null")
}

/// Número de fila: entero, texto con cifras («3», «3.0») o nada (0).
fn row_number(v: Option<&serde_json::Value>) -> usize {
    v.and_then(scalar)
        .and_then(|s| s.split(['.', ',']).next()?.trim().parse().ok())
        .unwrap_or(0)
}

// ---------- Lecturas guardadas ----------

fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub fn store(v: &Vault, bytes: &[u8], r: &Reading, full: bool, now: OffsetDateTime) -> Result<()> {
    store_sha(v, &sha(bytes), r, full, now)
}

/// Como [`store`], con la huella del fichero ya calculada.
fn store_sha(
    v: &Vault,
    file_sha: &str,
    r: &Reading,
    full: bool,
    now: OffsetDateTime,
) -> Result<()> {
    v.connection().execute(
        "INSERT INTO ai_reading(file_sha256, reading, full, created_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(file_sha256) DO UPDATE SET reading = excluded.reading, full = excluded.full,
           created_at = excluded.created_at",
        params![
            file_sha,
            serde_json::to_string(r).map_err(|e| AppError::Other(e.to_string()))?,
            full,
            now_str(now)
        ],
    )?;
    Ok(())
}

pub fn stored(v: &Vault, bytes: &[u8]) -> Result<Option<Reading>> {
    let json: Option<String> = v
        .connection()
        .query_row(
            "SELECT reading FROM ai_reading WHERE file_sha256 = ?1",
            [sha(bytes)],
            |r| r.get(0),
        )
        .optional()?;
    Ok(json
        .and_then(|j| serde_json::from_str::<serde_json::Value>(&j).ok())
        .and_then(|j| reading_from_value(&j)))
}

// ---------- Aplicar una lectura ----------

/// Títulos que entienden los importadores, por campo.
fn canonical(field: &str) -> Option<&'static str> {
    Some(match field {
        // Libros registro (los entiende `libros::columns`).
        "fecha_registro" => "Fecha registro",
        "serie" => "Serie",
        "numero" => "Número factura",
        "nif" => "NIF",
        "nombre" => "Nombre",
        "pais" => "País",
        "base" => "Base imponible",
        "tipo_iva" => "Tipo IVA",
        "cuota_iva" => "Cuota IVA",
        "recargo" => "Recargo",
        "retencion" => "Retención",
        "total" => "Total",
        // Diario (los entiende `import::columns`).
        "asiento" => "Asiento",
        "documento" => "Documento",
        "concepto" => "Concepto",
        "cuenta" => "Cuenta",
        "nombre_cuenta" => "Nombre cuenta",
        "debe" => "Debe",
        "haber" => "Haber",
        _ => return None,
    })
}

/// Filas de la hoja con títulos conocidos en las columnas que señaló el asistente. Las fechas
/// sin año («dd/mm») se completan con el de la cabecera del listado y, en un diario con la
/// cuenta en dos columnas (`cuenta_debe` y `cuenta_haber`), cada línea lleva la de su lado.
pub fn remap(
    rows: &[Vec<String>],
    plan: &SheetPlan,
    journal: bool,
    title: Option<&str>,
) -> Vec<Vec<String>> {
    remap_con_ancho(rows, plan, journal, title, 0)
}

/// [`remap`] con un ancho mínimo, para que varias hojas recolocadas casen columna a columna.
fn remap_con_ancho(
    rows: &[Vec<String>],
    plan: &SheetPlan,
    journal: bool,
    title: Option<&str>,
    ancho: usize,
) -> Vec<Vec<String>> {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0).max(ancho);
    let mut header = vec![String::new(); width];
    for (field, letter) in &plan.columnas {
        let Some(i) = col_index(letter) else { continue };
        if i >= width {
            continue;
        }
        let name = if field == "fecha" {
            Some(if journal {
                "Fecha"
            } else {
                "Fecha expedición"
            })
        } else {
            canonical(field)
        };
        if let Some(n) = name {
            header[i] = n.to_owned();
        }
    }
    let first = plan.primera_fila.max(plan.fila_titulos + 1).max(1) - 1;
    let col = |f: &str| plan.columnas.get(f).and_then(|l| col_index(l));
    // Fechas sin año: el del periodo de la cabecera.
    let año = crate::import::año_de_cabecera(rows.get(..first).unwrap_or(rows));
    let fechas: Vec<usize> = ["fecha", "fecha_registro"]
        .iter()
        .filter_map(|f| col(f))
        .collect();
    // Cuenta en dos columnas: una sola, la del lado que tiene importe.
    let lados = if journal && !plan.columnas.contains_key("cuenta") {
        col("cuenta_debe").zip(col("cuenta_haber")).zip(col("debe"))
    } else {
        None
    };
    if lados.is_some() {
        header.push("Cuenta".to_owned());
    }
    let mut out = Vec::new();
    if let Some(t) = title {
        out.push(vec![t.to_owned()]);
    }
    out.push(header);
    for r in rows.iter().skip(first) {
        let mut r = r.clone();
        for &i in &fechas {
            if let Some(c) = r.get_mut(i)
                && crate::import::parse_date(c).is_none()
                && let Some(d) = crate::import::parse_date_in(c, año)
            {
                *c = format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year());
            }
        }
        if let Some(((debe_c, haber_c), debe)) = lados {
            let celda = |i: usize| r.get(i).map(|x| x.trim().to_owned()).unwrap_or_default();
            let en_debe = crate::import::parse_amount(&celda(debe)).is_some_and(|m| !m.is_zero());
            let cuenta = celda(if en_debe { debe_c } else { haber_c });
            r.resize(width, String::new());
            r.push(cuenta);
        }
        out.push(r);
    }
    out
}

/// Qué hay en las hojas que se recolocan, para saber cuáles se juntan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contenido {
    Diario,
    Emitidas,
    Recibidas,
}

/// Como [`remap`], con todas las hojas del libro que tienen la fila de títulos de la primera
/// (un diario por meses, las hojas de un libro registro…), seguidas, y los avisos de las que no
/// entran. En recibidas entran las de adquisiciones intracomunitarias e inversión del sujeto
/// pasivo (las marca su total sin la cuota) y las de IVA no deducible, con un 0 % deducible; en
/// emitidas, sólo las normales: esas otras son la autorrepercusión de compras, que ya sale de
/// las recibidas.
pub(crate) fn remap_hojas(
    hojas: &[crate::import::Hoja],
    plan: &SheetPlan,
    contenido: Contenido,
    title: Option<&str>,
) -> (Vec<Vec<String>>, Vec<String>) {
    use crate::libros::{ClaseHoja, clase_de_hoja};
    let titulos = |h: &crate::import::Hoja| {
        plan.fila_titulos
            .checked_sub(1)
            .and_then(|i| h.filas.get(i))
            .map(|f| {
                f.iter()
                    .map(|c| crate::import::norm(c))
                    .filter(|c| !c.is_empty())
                    .collect::<Vec<_>>()
            })
    };
    let Some(primera) = hojas.first() else {
        return (Vec::new(), Vec::new());
    };
    let base = titulos(primera);
    let mut avisos = Vec::new();
    let elegidas: Vec<&crate::import::Hoja> = hojas
        .iter()
        .enumerate()
        .filter(|(n, h)| {
            if *n == 0 {
                return true;
            }
            if plan.fila_titulos > 0 && titulos(h) != base {
                avisos.push(format!(
                    "la hoja «{}» tiene otros títulos: no entra",
                    h.nombre
                ));
                return false;
            }
            if contenido == Contenido::Emitidas && clase_de_hoja(&h.nombre) != ClaseHoja::Normal {
                avisos.push(format!(
                    "la hoja «{}» es la autorrepercusión de compras: no entra como emitidas",
                    h.nombre
                ));
                return false;
            }
            true
        })
        .map(|(_, h)| h)
        .collect();
    let ancho = elegidas
        .iter()
        .flat_map(|h| h.filas.iter().map(Vec::len))
        .max()
        .unwrap_or(0);
    let cabecera = usize::from(title.is_some()) + 1;
    let mut out: Vec<Vec<String>> = Vec::new();
    for h in elegidas {
        let mut m = remap_con_ancho(&h.filas, plan, contenido == Contenido::Diario, title, ancho);
        if contenido == Contenido::Recibidas {
            // Porcentaje deducible: 0 en las hojas de IVA no deducible.
            let ancho_fila = m.get(cabecera - 1).map_or(ancho, Vec::len);
            let pct = if clase_de_hoja(&h.nombre) == ClaseHoja::NoDeducible {
                "0"
            } else {
                "100"
            };
            for (i, fila) in m.iter_mut().enumerate().skip(cabecera - 1) {
                fila.resize(ancho_fila, String::new());
                fila.push(
                    if i == cabecera - 1 {
                        "% deducible"
                    } else {
                        pct
                    }
                    .to_owned(),
                );
            }
        }
        if out.is_empty() {
            out = m;
        } else {
            out.extend(m.into_iter().skip(cabecera));
        }
    }
    (out, avisos)
}

fn text(j: &serde_json::Value, k: &str) -> Option<String> {
    j.get(k)
        .and_then(|v| match v {
            serde_json::Value::String(s) => Some(s.trim().to_owned()),
            serde_json::Value::Number(n) => Some(n.to_string()),
            _ => None,
        })
        .filter(|s| !s.is_empty() && s != "null")
}

/// Importe escrito por el asistente (el último separador es el decimal: «1,234.56» no es 1,23).
fn money(j: &serde_json::Value, k: &str) -> Option<Money> {
    text(j, k).and_then(|s| crate::received::parse_amount(&s))
}

fn date(j: &serde_json::Value, k: &str) -> Option<Date> {
    text(j, k).and_then(|s| crate::import::parse_date(&s))
}

/// Extracción de una factura (para la revisión de Recibidas), con confianza media.
pub fn invoice_extraction(f: &serde_json::Value) -> taxpain_invoicing::ingest::Extraction {
    use taxpain_invoicing::ingest::{Confidence, Extraction, Field};
    fn field<T>(value: T) -> Option<Field<T>> {
        Some(Field {
            value,
            confidence: Confidence::Media,
        })
    }
    Extraction {
        source: "asistente".into(),
        supplier_nif: text(f, "nif_emisor").and_then(field),
        supplier_name: text(f, "nombre_emisor").and_then(field),
        recipient_nif: text(f, "nif_destinatario").and_then(field),
        number: text(f, "numero").and_then(field),
        issue_date: date(f, "fecha").and_then(field),
        base: money(f, "base").and_then(field),
        vat: money(f, "iva").and_then(field),
        withholding: money(f, "retencion").map(Money::abs).and_then(field),
        total: money(f, "total").and_then(field),
        taxes: vec![],
        fx: None,
        warnings: vec!["leída con el asistente: revisa los datos".into()],
    }
}

/// Una factura emitida leída con el asistente, como libro de una factura, tratada como las de un
/// libro del gestor: NIF del cliente normalizado (y su país), tipos redondeados a los españoles y
/// comprobados con la cuota, y la retención con el signo de la base. `Err` si falta el número,
/// la fecha o la base, o si un tipo no es ninguno de los españoles (no se importa a ciegas).
fn issued_book(
    v: &Vault,
    f: &serde_json::Value,
) -> std::result::Result<crate::libros::Book, String> {
    use crate::libros::{Book, BookInvoice, BookKind, BookLine};
    let number = text(f, "numero").ok_or("falta el número")?;
    let issue_date = date(f, "fecha").ok_or("falta la fecha")?;
    let mut warnings = vec!["factura emitida leída con el asistente: revísala".to_owned()];
    let mut line = |base: Money, rate: Option<String>, cuota: Option<String>| {
        let (rate_bp, vat) = crate::libros::line_rate_vat(
            base,
            rate.as_deref(),
            cuota.as_deref(),
            false,
            &number,
            &mut warnings,
        );
        BookLine {
            base,
            rate_bp,
            vat,
            surcharge: Money::ZERO,
        }
    };
    let mut lines: Vec<BookLine> = f
        .get("tipos")
        .and_then(|t| t.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|t| Some(line(money(t, "base")?, text(t, "tipo"), text(t, "cuota"))))
                .collect()
        })
        .unwrap_or_default();
    if lines.is_empty() {
        let base = money(f, "base").ok_or("falta la base")?;
        lines.push(line(base, None, text(f, "iva")));
    }
    if let Some(l) = lines.iter().find(|l| !crate::libros::known_rate(l.rate_bp)) {
        return Err(format!(
            "el tipo de IVA leído ({} %) no es ninguno de los españoles: revisa la lectura",
            Money::from_cents(i64::from(l.rate_bp))
        ));
    }
    let base: Money = lines.iter().fold(Money::ZERO, |a, l| a + l.base);
    let vat: Money = lines.iter().fold(Money::ZERO, |a, l| a + l.vat);
    let withholding = money(f, "retencion").map(Money::abs).unwrap_or(Money::ZERO);
    let withholding = if base.is_negative() {
        -withholding
    } else {
        withholding
    };
    let computed = base + vat - withholding;
    let total = match money(f, "total") {
        Some(t) if t == computed || t == base + vat => computed,
        Some(t) => {
            warnings.push(format!(
                "{number}: el total leído ({t}) no cuadra con base + IVA − retención ({computed})"
            ));
            computed
        }
        None => computed,
    };
    let (nif, country) = text(f, "nif_destinatario")
        .map(|n| crate::libros::normalize_nif(&n, None))
        .unwrap_or((None, None));
    // El nombre del cliente: el que lea el asistente o el de otras facturas suyas.
    let name = text(f, "nombre_destinatario")
        .or_else(|| {
            let n = nif.as_deref()?;
            v.connection()
                .query_row(
                    "SELECT name FROM (
                       SELECT recipient_name AS name, id FROM invoice_issued
                        WHERE recipient_tax_id = ?1 AND recipient_name IS NOT NULL
                       ORDER BY id DESC LIMIT 1)
                     UNION ALL SELECT name FROM contact WHERE tax_id = ?1 LIMIT 1",
                    [n],
                    |r| r.get::<_, String>(0),
                )
                .optional()
                .ok()
                .flatten()
        })
        .unwrap_or_default();
    Ok(Book {
        kind: BookKind::Emitidas,
        invoices: vec![BookInvoice {
            number,
            issue_date,
            registered: issue_date,
            nif,
            country,
            name,
            lines,
            withholding,
            total,
            vat_in_total: None,
            deductible_pct: 100,
            self_assessed: false,
        }],
        warnings,
    })
}

/// Si una factura emitida leída con el asistente no se debe importar sin mirarla: ya está en
/// el libro de emitidas (mismo número normalizado y año) o es de un periodo que declaró el
/// gestor (y entonces debería estar en su libro).
pub(crate) fn issued_status(v: &Vault, f: &serde_json::Value) -> Option<String> {
    let number = text(f, "numero")?;
    let d = date(f, "fecha")?;
    let conn = v.connection();
    if let Ok(Some(n)) = crate::libros::find_issued(conn, &number, d) {
        return Some(format!("ya está en el libro de emitidas ({n})"));
    }
    crate::libros::issued_period_covered(conn, d)
        .unwrap_or(false)
        .then(|| {
            "es de un periodo que ya declaró el gestor y no está en su libro de emitidas: no se \
             propone importarla"
                .to_owned()
        })
}

/// Libro de recibidas de una lectura (con las columnas que señaló el asistente).
pub(crate) fn received_book(
    filename: &str,
    bytes: &[u8],
    r: &Reading,
) -> Option<crate::libros::Book> {
    if r.tipo != "libro_recibidas" {
        return None;
    }
    book_of(filename, bytes, r)
}

/// Libro de facturas (de emitidas o de recibidas) de una lectura, con las columnas que señaló el
/// asistente.
pub(crate) fn book_of(filename: &str, bytes: &[u8], r: &Reading) -> Option<crate::libros::Book> {
    let (title, contenido) = match r.tipo.as_str() {
        "libro_emitidas" => ("Libro registro de facturas expedidas", Contenido::Emitidas),
        "libro_recibidas" => ("Libro registro de facturas recibidas", Contenido::Recibidas),
        _ => return None,
    };
    let hojas = crate::import::sheets(filename, bytes).ok()?;
    let (mapped, _) = remap_hojas(&hojas, r.hoja.as_ref()?, contenido, Some(title));
    crate::libros::parse_rows(filename, &mapped)
}

/// « Avisos: …» para añadir a un mensaje, si los hay.
fn with_warnings(warnings: &[String]) -> String {
    if warnings.is_empty() {
        String::new()
    } else {
        format!(". Avisos: {}", warnings.join("; "))
    }
}

pub(crate) fn doc_kind(tipo: &str) -> Option<crate::documentos::DocKind> {
    use crate::documentos::DocKind as K;
    Some(match tipo {
        "escritura_constitucion" => K::EscrituraConstitucion,
        "escritura_poderes" => K::EscrituraPoderes,
        "escritura_modificacion" => K::EscrituraModificacion,
        "estatutos" => K::Estatutos,
        "tarjeta_nif" => K::TarjetaNif,
        "censal_036" => K::Censal036,
        "alta_roi" => K::AltaRoi,
        "certificado_aeat" => K::CertificadoAeat,
        "certificado_seguridad_social" => K::CertificadoSeguridadSocial,
        "alta_seguridad_social" => K::AltaSeguridadSocial,
        "registro_mercantil" => K::RegistroMercantil,
        "cuentas_anuales" => K::CuentasAnuales,
        "iae" => K::Iae,
        "acta_junta" => K::ActaJunta,
        "contrato_alquiler" => K::ContratoAlquiler,
        "seguro" => K::Seguro,
        "licencia" => K::Licencia,
        _ => return None,
    })
}

/// Tipo de documento de una lectura, corregido cuando el fichero lo contradice de forma
/// evidente: el justificante del depósito de las cuentas anuales que la lectura toma por una
/// certificación cualquiera del Registro Mercantil (por su trámite, por el nombre del fichero
/// o por lo que dicen las reglas de su texto).
pub(crate) fn doc_kind_of(
    filename: &str,
    bytes: &[u8],
    r: &Reading,
) -> Option<crate::documentos::DocKind> {
    use crate::documentos::DocKind as K;
    let k = doc_kind(&r.tipo)?;
    if k != K::RegistroMercantil {
        return Some(k);
    }
    let tramite = r
        .registro
        .as_ref()
        .and_then(|x| x.get("tramite"))
        .and_then(|x| x.as_str())
        == Some("deposito_cuentas");
    let nombre = crate::documentos::fold(filename);
    let por_nombre =
        (nombre.contains("deposito") || nombre.contains("depositad")) && nombre.contains("cuentas");
    let por_texto = || {
        crate::documentos::recognize(filename, bytes)
            .is_some_and(|d| !d.by_name && d.kind == K::CuentasAnuales)
    };
    Some(if tramite || por_nombre || por_texto() {
        K::CuentasAnuales
    } else {
        k
    })
}

/// Título de lo que es, para la pantalla.
pub fn label(r: &Reading) -> String {
    let t = match r.tipo.as_str() {
        "diario" => "Diario",
        "libro_emitidas" => "Libro de facturas emitidas",
        "libro_recibidas" => "Libro de facturas recibidas",
        "sumas_saldos" => "Balance de sumas y saldos",
        "extracto_bancario" => "Extracto bancario",
        "factura_recibida" => "Factura recibida",
        "factura_emitida" => "Factura emitida",
        "modelo_aeat" => "Modelo de la AEAT",
        "nomina" => "Nómina",
        "otro" => "Otro documento",
        t => return doc_kind(t).map_or_else(|| t.replace('_', " "), |k| k.title().to_owned()),
    };
    t.to_owned()
}

/// ¿Se importa algo con esta lectura?
pub fn importable(r: &Reading) -> bool {
    match r.tipo.as_str() {
        "diario" | "libro_emitidas" | "libro_recibidas" => r.hoja.is_some(),
        // Los extractos se leen con sus lectores; la lectura sólo cuenta si no lo reconoce
        // ninguno, y entonces tiene que traer las columnas o los movimientos.
        "extracto_bancario" => r.hoja.is_some() || r.extracto.is_some(),
        "factura_recibida" | "factura_emitida" => r.factura.is_some(),
        "modelo_aeat" => r.modelo.is_some(),
        t => doc_kind(t).is_some(),
    }
}

/// Aplica una lectura (con los importadores de siempre). Devuelve qué se hizo.
pub fn apply(
    v: &Vault,
    filename: &str,
    bytes: &[u8],
    r: &Reading,
    own_nif: Option<&str>,
    now: OffsetDateTime,
) -> Result<String> {
    apply_with_received(v, filename, bytes, r, own_nif, &[], now)
}

/// Como [`apply`]; en un libro de emitidas, las líneas que son facturas de `received` (los
/// libros de recibidas de la carpeta) son la autorrepercusión de compras y no entran como ventas.
pub(crate) fn apply_with_received(
    v: &Vault,
    filename: &str,
    bytes: &[u8],
    r: &Reading,
    own_nif: Option<&str>,
    received: &[crate::libros::BookInvoice],
    now: OffsetDateTime,
) -> Result<String> {
    match r.tipo.as_str() {
        "extracto_bancario" => {
            let r = crate::extractos::importar::importar_fichero(
                v,
                filename,
                bytes,
                Some(r),
                false,
                now,
            )?;
            Ok(r.frase())
        }
        "diario" | "libro_emitidas" | "libro_recibidas" => {
            let plan = r
                .hoja
                .as_ref()
                .ok_or_else(|| AppError::Other("el asistente no señaló las columnas".into()))?;
            let hojas = crate::import::sheets(filename, bytes)?;
            if r.tipo == "diario" {
                let (mapped, avisos) = remap_hojas(&hojas, plan, Contenido::Diario, None);
                let (batch, p) = crate::import::import_rows(v, filename, bytes, &mapped, now)?;
                return Ok(format!(
                    "{} asientos (lote {batch}), columnas según el asistente{}",
                    p.entries,
                    with_warnings(&avisos)
                ));
            }
            let emitidas = r.tipo == "libro_emitidas";
            let title = if emitidas {
                "Libro registro de facturas expedidas"
            } else {
                "Libro registro de facturas recibidas"
            };
            let contenido = if emitidas {
                Contenido::Emitidas
            } else {
                Contenido::Recibidas
            };
            let (mapped, avisos) = remap_hojas(&hojas, plan, contenido, Some(title));
            let mut book = crate::libros::parse_rows(filename, &mapped).ok_or_else(|| {
                AppError::Other("con las columnas que dijo el asistente no salen facturas".into())
            })?;
            crate::libros::mark_self_assessed(&mut book, received);
            let origin = crate::libros::BatchOrigin {
                filename,
                bytes,
                source: "libro",
            };
            let mut out = if emitidas {
                crate::libros::import_issued_as(v, &book, &origin, now)?
            } else {
                crate::libros::import_received_as(v, &book, &origin, now)?
            };
            out.warnings.extend(avisos);
            Ok(format!(
                "{} facturas al libro de {} ({} ya estaban), columnas según el asistente{}",
                out.added,
                if emitidas { "emitidas" } else { "recibidas" },
                out.skipped,
                with_warnings(&out.warnings)
            ))
        }
        "factura_recibida" => {
            let f = r
                .factura
                .as_ref()
                .ok_or_else(|| AppError::Other("sin datos".into()))?;
            let e = invoice_extraction(f);
            // Si ya está en el libro (el del gestor, con otro formato de número), se le adjunta:
            // como pendiente se deduciría otra vez al revisarla.
            let found = crate::libros::ReceivedRef::from_extraction(&e);
            if let Some(id) = crate::libros::find_received(v.connection(), &found)? {
                return crate::libros::attach_document(v, id, filename, bytes, now);
            }
            let historical = found
                .date
                .is_some_and(|d| crate::libros::period_covered(v.connection(), d).unwrap_or(false));
            let out = crate::received::ingest_file(
                v,
                filename,
                bytes,
                crate::received::Source::Carpeta,
                now,
            )?;
            if out.duplicate {
                return Ok("ya estaba en Recibidas".into());
            }
            crate::received::merge_extraction(v, out.id, &e)?;
            if historical {
                crate::libros::warn_pending(v, out.id, crate::libros::NOT_IN_BOOK)?;
            }
            Ok(format!(
                "a Recibidas para revisar, con los datos del asistente{}",
                if historical {
                    "; ojo: es de un periodo que ya declaró el gestor y no está en su libro"
                } else {
                    ""
                }
            ))
        }
        "factura_emitida" => {
            let f = r
                .factura
                .as_ref()
                .ok_or_else(|| AppError::Other("sin datos".into()))?;
            if let Some(why) = not_own_issued(f, own_nif) {
                return Ok(why);
            }
            let book = issued_book(v, f).map_err(AppError::Other)?;
            if let Some(inv) = book.invoices.first()
                && let Some(n) =
                    crate::libros::find_issued(v.connection(), &inv.number, inv.issue_date)?
            {
                return Ok(format!("ya estaba en el libro de emitidas ({n})"));
            }
            let origin = crate::libros::BatchOrigin {
                filename,
                bytes,
                source: "lectura",
            };
            let out = crate::libros::import_issued_as(v, &book, &origin, now)?;
            Ok(if out.added > 0 {
                format!(
                    "al libro de emitidas (leída con el asistente: revísala; se puede deshacer \
                     desde las importaciones de facturas){}",
                    with_warnings(&out.warnings)
                )
            } else {
                "ya estaba en el libro de emitidas".into()
            })
        }
        "modelo_aeat" => {
            let m = r
                .modelo
                .as_ref()
                .ok_or_else(|| AppError::Other("sin datos".into()))?;
            let model = text(m, "modelo").ok_or_else(|| AppError::Other("sin modelo".into()))?;
            let year: i32 = text(m, "ejercicio")
                .and_then(|y| y.parse().ok())
                .ok_or_else(|| AppError::Other("sin ejercicio".into()))?;
            let period = text(m, "periodo").unwrap_or_default().to_uppercase();
            check_model(&model, year, &period).map_err(AppError::Other)?;
            let boxes: BTreeMap<u16, Money> = m
                .get("casillas")
                .and_then(|c| c.as_object())
                .map(|o| {
                    o.iter()
                        .filter_map(|(k, val)| {
                            let n: u16 = k
                                .trim_start_matches('[')
                                .trim_end_matches(']')
                                .parse()
                                .ok()?;
                            let s = match val {
                                serde_json::Value::String(s) => s.clone(),
                                serde_json::Value::Number(x) => x.to_string(),
                                _ => return None,
                            };
                            Some((n, crate::received::parse_amount(&s)?))
                        })
                        .collect()
                })
                .unwrap_or_default();
            let read = crate::presentados::Read {
                model: model.clone(),
                year,
                period: period.clone(),
                source: "pdf".into(),
                boxes,
                boxes_read: true,
                ..crate::presentados::Read::default()
            };
            // La lectura del asistente sustituye a la que se hubiera sacado del mismo PDF con
            // las reglas (no al fichero de la AEAT ni a lo tecleado).
            let stored = crate::presentados::store_read(v, filename, bytes, &read, true, now)?;
            Ok(match stored {
                crate::presentados::Stored::New(_) => format!(
                    "modelo {model} {year} {period} guardado para comparar (leído con el asistente)"
                ),
                crate::presentados::Stored::Replaced(_) => format!(
                    "modelo {model} {year} {period}: la lectura del asistente sustituye a la que \
                     se sacó antes del mismo PDF"
                ),
                crate::presentados::Stored::Kept(_) | crate::presentados::Stored::Copy(..) => {
                    format!(
                        "modelo {model} {year} {period}: ya estaba guardado (del fichero de la AEAT o \
                     tecleado); no se cambia"
                    )
                }
            })
        }
        _ => match doc_kind_of(filename, bytes, r) {
            Some(kind) => {
                let facts = r
                    .empresa
                    .as_ref()
                    .map(|e| crate::documentos::parse_ai(&e.to_string()))
                    .unwrap_or_default();
                let rec = crate::documentos::Recognized {
                    kind,
                    title: kind.title().to_owned(),
                    facts,
                    by_name: false,
                };
                crate::documentos::store(v, filename, bytes, &rec, now)?;
                Ok(format!(
                    "{} guardado en Documentos de la empresa",
                    kind.title()
                ))
            }
            None => Ok(format!("{}: no se importa", label(r))),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn el_deposito_de_cuentas_no_es_una_certificacion_cualquiera() {
        use crate::documentos::DocKind as K;
        let r = |registro: Option<serde_json::Value>| Reading {
            tipo: "registro_mercantil".into(),
            registro,
            ..Reading::default()
        };
        let pdf = b"%PDF-1.4 sin texto";
        // Por el trámite que dice la propia lectura.
        let tramite = r(Some(serde_json::json!({"tramite": "deposito_cuentas"})));
        assert_eq!(
            doc_kind_of("certificacion.pdf", pdf, &tramite),
            Some(K::CuentasAnuales)
        );
        // Por el nombre del fichero.
        assert_eq!(
            doc_kind_of("Cuentas_Anuales_2025_Demo_depósito.pdf", pdf, &r(None)),
            Some(K::CuentasAnuales)
        );
        // Una nota simple sigue siendo una nota simple.
        assert_eq!(
            doc_kind_of("nota simple.pdf", pdf, &r(None)),
            Some(K::RegistroMercantil)
        );
    }

    #[test]
    fn el_periodo_tiene_que_ser_del_modelo() {
        assert!(check_model("303", 2025, "1T").is_ok());
        assert!(check_model("303", 2025, "07").is_ok());
        assert!(check_model("390", 2025, "0A").is_ok());
        assert!(check_model("202", 2025, "2P").is_ok());
        assert!(check_model("130", 2025, "4T").is_ok());
        assert!(check_model("349", 2025, "11").is_ok());
        for (m, p) in [
            ("390", "1T"),
            ("303", "0A"),
            ("303", "1P"),
            ("200", "4T"),
            ("202", "1T"),
            ("130", "05"),
            ("190", "4T"),
        ] {
            let e = check_model(m, 2025, p).unwrap_err();
            assert!(e.contains("no tiene periodo"), "{m} {p}: {e}");
        }
        assert!(check_model("303", 2025, "5T").is_err());
        assert!(check_model("abc", 2025, "1T").is_err());
        assert!(check_model("303", 20255, "1T").is_err());
    }

    #[test]
    fn pdf_dentro_de_un_sobre_mime() {
        let pdf = b"%PDF-1.4\n1 0 obj\n<<>>\nendobj\n%%EOF";
        let mut sobre = b"\r\n--uuid:demo\r\nContent-Type: multipart/mixed\r\n\
Content-Transfer-Encoding: binary\r\n\r\n"
            .to_vec();
        sobre.extend_from_slice(pdf);
        sobre.extend_from_slice(b"\r\n--uuid:demo--");
        assert_eq!(
            media_type("Movimientos.pdf", &sobre),
            Some("application/pdf")
        );
        assert_eq!(attachment_bytes("Movimientos.pdf", &sobre), pdf.to_vec());
        assert_eq!(attachment_bytes("factura.pdf", pdf), pdf.to_vec());
        assert_eq!(media_type("notas.txt", b"hola"), None);
    }

    #[test]
    fn letras_de_columna() {
        assert_eq!(col_letter(0), "A");
        assert_eq!(col_letter(25), "Z");
        assert_eq!(col_letter(26), "AA");
        assert_eq!(col_index("A"), Some(0));
        assert_eq!(col_index("aa"), Some(26));
        assert_eq!(col_index("3"), Some(2));
    }

    #[test]
    fn hoja_sin_titulos_se_importa_con_las_columnas_del_asistente() {
        // Libro de emitidas sin fila de títulos: fecha en B, número en A, NIF en C…
        let rows: Vec<Vec<String>> = [
            [
                "F-1",
                "15/01/2025",
                "B12345674",
                "Cliente SL",
                "1.000,00",
                "21",
                "210,00",
                "1.210,00",
            ],
            [
                "F-2",
                "20/01/2025",
                "B12345674",
                "Cliente SL",
                "100,00",
                "21",
                "21,00",
                "121,00",
            ],
        ]
        .iter()
        .map(|r| r.iter().map(|c| (*c).to_owned()).collect())
        .collect();
        let reply = r#"Aquí está: {"tipo":"libro_emitidas","descripcion":"Ventas 2025","hoja":{"fila_titulos":0,"primera_fila":1,"columnas":{"numero":"A","fecha":"B","nif":"C","nombre":"D","base":"E","tipo_iva":"F","cuota_iva":"G","total":"H"}}}"#;
        let r = parse_reply(reply).unwrap_or_else(|| panic!("no leído"));
        assert!(importable(&r));
        let plan = r.hoja.clone().unwrap_or_default();
        let mapped = remap(
            &rows,
            &plan,
            false,
            Some("Libro registro de facturas expedidas"),
        );
        let book = crate::libros::parse_rows("ventas.xlsx", &mapped)
            .unwrap_or_else(|| panic!("{mapped:?}"));
        assert_eq!(book.invoices.len(), 2);
        assert_eq!(book.invoices[0].base(), Money::from_cents(100_000));
        assert_eq!(book.invoices[1].vat(), Money::from_cents(2100));
    }

    #[test]
    fn diario_con_columnas_raras() {
        let rows: Vec<Vec<String>> = [
            vec!["Apuntes de la empresa", "", "", "", ""],
            vec!["F.", "Nº", "Cta.", "Texto", "Cargo €", "Abono €"],
            vec!["02/01/2025", "1", "572", "Aportación", "3000", ""],
            vec!["02/01/2025", "1", "100", "Aportación", "", "3000"],
        ]
        .iter()
        .map(|r| r.iter().map(|c| (*c).to_owned()).collect())
        .collect();
        let plan = SheetPlan {
            fila_titulos: 2,
            primera_fila: 3,
            columnas: [
                ("fecha", "A"),
                ("asiento", "B"),
                ("cuenta", "C"),
                ("concepto", "D"),
                ("debe", "E"),
                ("haber", "F"),
            ]
            .into_iter()
            .map(|(a, b)| (a.to_owned(), b.to_owned()))
            .collect(),
        };
        let mapped = remap(&rows, &plan, true, None);
        assert_eq!(mapped[0][0], "Fecha");
        assert_eq!(mapped[0][4], "Debe");
        assert_eq!(mapped.len(), 3);
    }

    #[test]
    fn factura_y_modelo_del_asistente() {
        let r = parse_reply(
            r#"{"tipo":"factura_recibida","factura":{"nif_emisor":"B12345674","numero":"A-7","fecha":"2025-03-01","base":"100.00","iva":21,"retencion":"-15","total":"106"}}"#,
        )
        .unwrap_or_else(|| panic!("no leído"));
        let e = invoice_extraction(r.factura.as_ref().unwrap_or(&serde_json::Value::Null));
        assert_eq!(e.base.map(|f| f.value), Some(Money::from_cents(10_000)));
        assert_eq!(e.vat.map(|f| f.value), Some(Money::from_cents(2100)));
        assert_eq!(
            e.withholding.map(|f| f.value),
            Some(Money::from_cents(1500))
        );
        assert_eq!(label(&r), "Factura recibida");
        assert!(parse_reply("no sé").is_none());
    }

    /// El acta que aprueba las cuentas trae la aplicación del resultado en su bloque, y se guarda
    /// con la lectura (la revisión lo lee de ahí).
    #[test]
    fn acta_con_la_aplicacion_del_resultado() {
        let r = parse_reply(
            r#"{"tipo":"acta_junta","descripcion":"Acta de 30/06/2026","acta":{"ejercicio":2025,"reserva_legal":"100.00","reserva_nivelacion":null}}"#,
        )
        .unwrap_or_else(|| panic!("no leído"));
        let acta = r.acta.as_ref().unwrap_or_else(|| panic!("sin acta"));
        assert_eq!(acta["ejercicio"], 2025);
        let json = serde_json::to_value(&r).unwrap_or_default();
        assert_eq!(reading_from_value(&json), Some(r));
    }
}

/// Filas de una hoja de cálculo (las de la primera hoja, las mismas que usa la importación).
pub fn sheet_rows(filename: &str, bytes: &[u8]) -> std::result::Result<Vec<Vec<String>>, String> {
    crate::import::rows(filename, bytes).map_err(|e| e.to_string())
}

/// Nombres de las hojas de una hoja de cálculo (vacío en un CSV).
pub fn sheet_names(filename: &str, bytes: &[u8]) -> Vec<String> {
    crate::import::sheets(filename, bytes)
        .map(|hs| {
            hs.into_iter()
                .map(|h| h.nombre)
                .filter(|n| !n.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Por qué una lectura de factura emitida no se importa como venta propia: sin NIF del emisor
/// o con el de otra empresa, una factura ajena entraría en el libro de emitidas (303 [07],
/// 390, 347).
fn not_own_issued(f: &serde_json::Value, own_nif: Option<&str>) -> Option<String> {
    let nif = |s: &str| crate::libros::normalize_nif(s, None).0;
    match (text(f, "nif_emisor"), own_nif) {
        (None, _) => Some(
            "la lectura no dice el NIF del emisor: no se importa como emitida (si es tuya, \
             añádelo a la lectura)"
                .into(),
        ),
        (Some(n), Some(own)) if nif(&n) != nif(own) => {
            Some("el emisor no es tu empresa: no se importa como emitida".into())
        }
        _ => None,
    }
}

/// Periodos de los modelos que se guardan: trimestres, meses, anual y pagos fraccionados.
fn valid_period(p: &str) -> bool {
    matches!(p, "1T" | "2T" | "3T" | "4T" | "0A" | "1P" | "2P" | "3P")
        || (p.len() == 2 && p.parse::<u8>().is_ok_and(|m| (1..=12).contains(&m)))
}

/// Periodos que admite cada modelo: los resúmenes anuales y el 200 sólo «0A»; el 202, sus tres
/// pagos (1P a 3P); el 303, el 349 y las retenciones (111, 115, 123), trimestrales o
/// mensuales (grandes empresas, REDEME y SII; en el 349, también por el volumen de
/// operaciones); el 130 y el 131, trimestrales. Un 390 con periodo «1T» o un 303 «0A» no
/// existen en la Sede y dejarían un modelo fantasma.
fn period_fits(model: &str, period: &str) -> bool {
    let trimestre = matches!(period, "1T" | "2T" | "3T" | "4T");
    let mes = period.len() == 2 && period.parse::<u8>().is_ok_and(|m| (1..=12).contains(&m));
    match model {
        "390" | "190" | "180" | "193" | "347" | "200" => period == "0A",
        "202" => matches!(period, "1P" | "2P" | "3P"),
        "303" | "349" | "111" | "115" | "123" => trimestre || mes,
        "130" | "131" => trimestre,
        _ => valid_period(period),
    }
}

/// Modelo, ejercicio y periodo de una lectura, antes de guardarlos: un modelo «abc», un
/// ejercicio 20255 o un periodo «ABC» dejarían un modelo fantasma que no se puede comparar y
/// que marca periodos como declarados.
fn check_model(model: &str, year: i32, period: &str) -> std::result::Result<(), String> {
    if !crate::presentados::known_model(model) {
        return Err(format!(
            "modelo «{model}» desconocido: se guardan los que presenta una gestoría (303, 390, \
             111, 115, 123, 130, 131, 190, 180, 193, 200, 202, 347 y 349)"
        ));
    }
    if !(2000..=2100).contains(&year) {
        return Err(format!("ejercicio {year}: no es un año"));
    }
    if !valid_period(period) {
        return Err(format!(
            "periodo «{period}» desconocido: 1T a 4T, 01 a 12, 0A (anual) o 1P a 3P (202)"
        ));
    }
    if !period_fits(model, period) {
        let admite = match model {
            "390" | "190" | "180" | "193" | "347" | "200" => "anual (0A)",
            "202" => "1P, 2P o 3P",
            "130" | "131" => "trimestral (1T a 4T)",
            _ => "trimestral (1T a 4T) o mensual (01 a 12)",
        };
        return Err(format!(
            "el modelo {model} no tiene periodo «{period}»: es {admite}"
        ));
    }
    Ok(())
}

fn need<'a>(
    o: &'a Option<serde_json::Value>,
    what: &str,
) -> std::result::Result<&'a serde_json::Value, String> {
    o.as_ref()
        .ok_or_else(|| format!("falta el bloque «{what}»"))
}

/// Comprueba una lectura sin importar nada: qué saldría al importarla.
pub fn simulate(
    v: &Vault,
    filename: &str,
    bytes: &[u8],
    r: &Reading,
) -> std::result::Result<String, String> {
    match r.tipo.as_str() {
        "extracto_bancario" => {
            let l = crate::extractos::leer_con_lectura(filename, bytes, Some(r))
                .map_err(|e| e.to_string())?;
            let mal: Vec<String> = l
                .iter()
                .filter_map(|x| match &x.cuadre {
                    crate::extractos::Cuadre::NoCuadra { detalle, .. } => Some(detalle.clone()),
                    _ => None,
                })
                .collect();
            if !mal.is_empty() {
                return Err(format!("no cuadra con sus saldos: {}", mal.join("; ")));
            }
            Ok(crate::extractos::resumen(&l))
        }
        "diario" | "libro_emitidas" | "libro_recibidas" => {
            let plan = r
                .hoja
                .as_ref()
                .ok_or("falta el bloque «hoja» con las columnas")?;
            let hojas = crate::import::sheets(filename, bytes).map_err(|e| e.to_string())?;
            if r.tipo == "diario" {
                let (filas, avisos) = remap_hojas(&hojas, plan, Contenido::Diario, None);
                let p = crate::import::preview_rows(v, bytes, &filas).map_err(|e| e.to_string())?;
                return Ok(format!(
                    "{} asientos{}{}",
                    p.entries,
                    match (p.first_date, p.last_date) {
                        (Some(a), Some(b)) => format!(" del {a} al {b}"),
                        _ => String::new(),
                    },
                    with_warnings(&avisos)
                ));
            }
            let (title, contenido) = if r.tipo == "libro_emitidas" {
                ("Libro registro de facturas expedidas", Contenido::Emitidas)
            } else {
                ("Libro registro de facturas recibidas", Contenido::Recibidas)
            };
            let (filas, avisos) = remap_hojas(&hojas, plan, contenido, Some(title));
            let b = crate::libros::parse_rows(filename, &filas)
                .ok_or("con esas columnas no sale ninguna factura")?;
            let no_deducibles = b.invoices.iter().filter(|i| i.deductible_pct == 0).count();
            let avisos: Vec<String> = avisos.into_iter().chain(b.warnings).collect();
            Ok(format!(
                "{} facturas{}{}",
                b.invoices.len(),
                if no_deducibles > 0 {
                    format!(" ({no_deducibles} con IVA no deducible)")
                } else {
                    String::new()
                },
                with_warnings(&avisos)
            ))
        }
        "factura_recibida" | "factura_emitida" => {
            let f = need(&r.factura, "factura")?;
            let e = invoice_extraction(f);
            let missing: Vec<&str> = [
                ("número", e.number.is_none()),
                ("fecha", e.issue_date.is_none()),
                ("total", e.total.is_none()),
            ]
            .into_iter()
            .filter_map(|(n, m)| m.then_some(n))
            .collect();
            if !missing.is_empty() {
                return Err(format!("a la factura le falta: {}", missing.join(", ")));
            }
            // Una emitida se importa al libro: tiene que ser nuestra y sus tipos, españoles.
            if r.tipo == "factura_emitida" {
                let own = crate::carpeta::own_nif(v);
                if let Some(why) = not_own_issued(f, own.as_deref()) {
                    return Err(why);
                }
                issued_book(v, f)?;
            }
            // Base + IVA − retención tiene que dar el total (± 2 céntimos): si no, algún importe
            // se ha leído mal (p. ej. «1,234.56» tomado por 1,23).
            let ret = e.withholding.as_ref().map_or(Money::ZERO, |w| w.value);
            let descuadre = match (&e.base, &e.vat, &e.total) {
                (Some(b), Some(i), Some(t)) => {
                    let calc = b.value + i.value - ret;
                    ((calc - t.value).cents().abs() > 2).then(|| {
                        format!(
                            "; ojo: los importes no cuadran (base {} + IVA {} − retención {ret} = \
                             {calc}, pero el total es {}): comprueba cómo están escritos",
                            b.value, i.value, t.value
                        )
                    })
                }
                _ => None,
            };
            Ok(format!("factura completa{}", descuadre.unwrap_or_default()))
        }
        "modelo_aeat" => {
            let m = need(&r.modelo, "modelo")?;
            let n = m
                .get("casillas")
                .and_then(|c| c.as_object())
                .map_or(0, |c| c.len());
            let (Some(model), Some(year)) = (text(m, "modelo"), text(m, "ejercicio")) else {
                return Err("al modelo le falta el número o el ejercicio".into());
            };
            let year: i32 = year
                .parse()
                .map_err(|_| format!("ejercicio «{year}» que no es un año"))?;
            let period = text(m, "periodo").unwrap_or_default().to_uppercase();
            check_model(&model, year, &period)?;
            Ok(format!("{n} casillas"))
        }
        _ => Ok("se guarda con sus datos".into()),
    }
}

// ---------- Lecturas hechas con Claude Code (sin API) ----------

/// Fichero que deja Claude Code en la carpeta de la empresa con sus lecturas
/// (`.claude/commands/leer-carpeta.md`). Mismo formato que las del asistente.
pub const PACKAGE_FILE: &str = "taxpain-lecturas.json";

/// Resultado de cargar las lecturas de Claude Code.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct PackageLoad {
    pub loaded: usize,
    /// Ficheros que ya no están en la carpeta.
    pub missing: Vec<String>,
    /// Lecturas sin tipo o mal formadas (se saltan; las demás se cargan).
    pub invalid: usize,
    /// Lecturas que no se cargan porque el asistente de la app leyó ese fichero después.
    pub kept: usize,
    /// Ficheros con el mismo contenido que otro (una copia en otra carpeta) y otra lectura: las
    /// lecturas de un mismo contenido se juntan en una ([`merge_readings`]).
    pub merged: usize,
    /// El fichero entero no se puede leer (p. ej. JSON a medio escribir): no se carga nada de
    /// él, pero la carpeta se lee igual.
    pub error: Option<String>,
}

/// Lee `taxpain-lecturas.json`: la lista de lecturas, cuándo se escribió y, si no es JSON, por
/// qué. Cada entrada se valida aparte después.
fn package_entries(
    path: &std::path::Path,
    now: OffsetDateTime,
) -> Option<(
    std::result::Result<Vec<serde_json::Value>, String>,
    OffsetDateTime,
)> {
    let bytes = std::fs::read(path).ok()?;
    // Cuándo se escribió: las lecturas del paquete son de entonces.
    let made = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(OffsetDateTime::from)
        .unwrap_or(now);
    let text = String::from_utf8_lossy(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes));
    let list = match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(serde_json::Value::Array(a)) => Ok(a),
        Ok(p) => Ok(p
            .get("lecturas")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default()),
        Err(e) => Err(format!(
            "{PACKAGE_FILE} no se puede leer (¿Claude Code aún lo está escribiendo?): {e}"
        )),
    };
    Some((list, made))
}

/// Ruta y lectura de una entrada del paquete (`None` si está mal formada).
pub fn package_entry(e: &serde_json::Value) -> Option<(String, Reading)> {
    let fichero = e
        .get("fichero")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|f| !f.is_empty())?;
    Some((fichero.to_owned(), reading_from_value(e.get("lectura")?)?))
}

/// Carga `taxpain-lecturas.json` de la carpeta (si existe): cada lectura se guarda por la huella
/// del fichero al que se refiere, como si la hubiera hecho el asistente. No llama a ninguna API.
/// Cada lectura va aparte: una mal escrita se cuenta en `invalid` y no impide cargar las demás
/// ni leer la carpeta. Las lecturas de ficheros con el mismo contenido (copias en carpetas
/// distintas) se juntan en una ([`merge_readings`]): si no, la última pisaría a las demás aunque
/// tuviera menos datos.
pub fn load_package(
    v: &Vault,
    root: &std::path::Path,
    now: OffsetDateTime,
) -> Result<Option<PackageLoad>> {
    let Some((list, made)) = package_entries(&root.join(PACKAGE_FILE), now) else {
        return Ok(None);
    };
    let mut out = PackageLoad::default();
    let list = match list {
        Ok(l) => l,
        Err(e) => {
            out.error = Some(e);
            return Ok(Some(out));
        }
    };
    // Las lecturas de cada contenido, en el orden del paquete.
    let mut groups: Vec<(String, Vec<Reading>)> = Vec::new();
    for e in &list {
        let Some((fichero, reading)) = package_entry(e) else {
            out.invalid += 1;
            continue;
        };
        match crate::carpeta::read_file(root, &fichero) {
            Ok((_, bytes)) => {
                let h = sha(&bytes);
                match groups.iter_mut().find(|(g, _)| *g == h) {
                    Some((_, rs)) => rs.push(reading),
                    None => groups.push((h, vec![reading])),
                }
            }
            Err(_) => out.missing.push(fichero),
        }
    }
    for (h, readings) in groups {
        let first = &readings[0];
        let reading = if readings.iter().all(|r| r == first) {
            first.clone()
        } else {
            out.merged += readings.len() - 1;
            merge_readings(&readings)
        };
        match store_from_package(v, &h, &reading, made) {
            Ok(true) => out.loaded += readings.len(),
            Ok(false) => out.kept += readings.len(),
            Err(err) => {
                tracing::warn!(error = %err, "no se pudo guardar una lectura de Claude Code");
                out.invalid += readings.len();
            }
        }
    }
    Ok(Some(out))
}

/// Datos que trae una lectura (los que no están vacíos), para elegir la más completa.
fn completeness(r: &Reading) -> usize {
    fn n(v: &serde_json::Value) -> usize {
        match v {
            serde_json::Value::Null => 0,
            serde_json::Value::String(s) if s.trim().is_empty() => 0,
            serde_json::Value::Object(o) => o.values().map(n).sum(),
            serde_json::Value::Array(a) => a.iter().map(n).sum(),
            _ => 1,
        }
    }
    serde_json::to_value(r).map_or(0, |v| n(&v))
}

/// ¿No dice nada este dato (no está, `null` o texto vacío)?
fn empty(v: Option<&serde_json::Value>) -> bool {
    match v {
        None | Some(serde_json::Value::Null) => true,
        Some(serde_json::Value::String(s)) => s.trim().is_empty(),
        Some(serde_json::Value::Array(a)) => a.is_empty(),
        Some(serde_json::Value::Object(o)) => o.is_empty(),
        Some(_) => false,
    }
}

/// Datos que van juntos y se toman enteros de una sola lectura: las columnas de una hoja, los
/// movimientos de un extracto, las casillas y los registros de un modelo, las líneas por tipo de
/// una factura. Mezclados con los de otra lectura darían algo que no dice ninguna.
const WHOLE: [&str; 6] = [
    "hoja",
    "extracto",
    "extractos",
    "casillas",
    "registros",
    "tipos",
];

/// Importes de una factura: si la lectura ya trae alguno, no se toma ninguno de otra (base, IVA y
/// retención de una con el total de otra no cuadrarían).
const INVOICE_AMOUNTS: [&str; 6] = ["base", "iva", "retencion", "total", "moneda", "tipos"];

/// Completa `a` con lo que trae `b` y a `a` le falta, campo a campo (`ctx`: el campo en el que
/// están): lo que `a` ya dice no se cambia, y lo que va junto ([`WHOLE`], [`INVOICE_AMOUNTS`])
/// sólo se toma entero.
fn fill(a: &mut serde_json::Value, b: &serde_json::Value, ctx: &str) {
    let (Some(a), Some(b)) = (a.as_object_mut(), b.as_object()) else {
        return;
    };
    let with_amounts = ctx == "factura" && INVOICE_AMOUNTS.iter().any(|k| !empty(a.get(*k)));
    for (k, vb) in b {
        if with_amounts && INVOICE_AMOUNTS.contains(&k.as_str()) {
            continue;
        }
        let whole = WHOLE.contains(&k.as_str());
        match a.get_mut(k) {
            Some(va) if !whole && va.is_object() && vb.is_object() => fill(va, vb, k),
            va if empty(va.as_deref()) && !empty(Some(vb)) => {
                a.insert(k.clone(), vb.clone());
            }
            _ => {}
        }
    }
}

/// Junta las lecturas de un mismo contenido (el mismo fichero en dos carpetas, leído dos
/// veces). Manda una que dice qué es el fichero (no «otro»): la más completa y, con empate, la
/// última, como cuando cada una pisaba a la anterior. Las demás de su tipo y las «otro» sólo
/// completan lo que le falta ([`fill`]); las de otro tipo no se mezclan.
pub fn merge_readings(readings: &[Reading]) -> Reading {
    let mut order: Vec<(usize, &Reading)> = readings.iter().enumerate().collect();
    order.sort_by_key(|(i, r)| {
        (
            r.tipo == "otro",
            std::cmp::Reverse(completeness(r)),
            std::cmp::Reverse(*i),
        )
    });
    let Some(best) = order.first().map(|(_, r)| *r) else {
        return Reading::default();
    };
    let Ok(mut merged) = serde_json::to_value(best) else {
        return best.clone();
    };
    for (_, r) in order
        .iter()
        .skip(1)
        .filter(|(_, r)| r.tipo == best.tipo || r.tipo == "otro")
    {
        if let Ok(other) = serde_json::to_value(r) {
            fill(&mut merged, &other, "");
        }
    }
    reading_from_value(&merged).unwrap_or_else(|| best.clone())
}

/// Guarda una lectura del paquete (del fichero de huella `file_sha`) salvo que el asistente de
/// la app haya leído el fichero después de escribirse el paquete (en `ai_reading`, `full` = 1
/// son las de la app con «Full IA»; las del paquete van con 0). Devuelve si la del paquete es la
/// que queda.
fn store_from_package(
    v: &Vault,
    file_sha: &str,
    r: &Reading,
    made: OffsetDateTime,
) -> Result<bool> {
    let json = serde_json::to_string(r).map_err(|e| AppError::Other(e.to_string()))?;
    let current: Option<(String, bool, String)> = v
        .connection()
        .query_row(
            "SELECT reading, full, created_at FROM ai_reading WHERE file_sha256 = ?1",
            [file_sha],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    if let Some((old, full, at)) = current {
        if old == json {
            return Ok(true);
        }
        let at = OffsetDateTime::parse(&at, &time::format_description::well_known::Rfc3339).ok();
        if full && at.is_some_and(|at| at >= made) {
            return Ok(false);
        }
    }
    store_sha(v, file_sha, r, false, made)?;
    Ok(true)
}
