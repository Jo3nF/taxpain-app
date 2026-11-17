//! Facturas recibidas: ingesta (arrastrar, carpeta vigilada), extracción y revisión humana.
//!
//! Las fotos (tickets, facturas en papel) y los PDF escaneados se leen con el reconocimiento de
//! texto del Mac (Vision, `carpeta::set_ocr`) y pasan por la misma extracción que un PDF con
//! texto. Sin reconocimiento (fuera del Mac) se guardan con un aviso para rellenarlos a mano.
//!
//! Facturas en moneda extranjera (una suscripción en dólares, por ejemplo): la revisión recibe
//! los importes en la moneda y el tipo ([`ReceivedFx`]); se contabilizan y declaran en euros
//! (art. 79.Once LIVA: base al tipo vigente al devengo) y la moneda queda guardada para
//! consulta y para las diferencias de cambio al pagarla.

use std::ops::Deref;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use taxpain_core::Nif;
use taxpain_core::fx::ForeignCurrency;
use taxpain_core::invoice::PartyId;
use taxpain_core::money::Money;
use taxpain_invoicing::ingest::{self, Extraction};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::format_description::well_known::Rfc3339;
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing;

/// Tamaño máximo de un documento recibido.
pub const MAX_FILE_SIZE: usize = 25 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    Manual,
    Carpeta,
    Correo,
}

impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Source::Manual => "manual",
            Source::Carpeta => "carpeta",
            Source::Correo => "correo",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct IngestOutcome {
    pub id: i64,
    /// El mismo fichero ya estaba importado.
    pub duplicate: bool,
    pub extraction: Extraction,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ReceivedSummary {
    pub id: i64,
    pub filename: String,
    pub status: String,
    pub supplier_nif: Option<String>,
    pub supplier_name: Option<String>,
    pub number: Option<String>,
    pub issue_date: Option<String>,
    pub total: Option<Money>,
    pub extraction: Extraction,
    /// En moneda extranjera: la moneda y el total en ella (`total` es en euros).
    pub currency: Option<String>,
    pub currency_total: Option<Money>,
    /// Revisada: los datos que se confirmaron (en euros), para corregirlos.
    pub reviewed: Option<Reviewed>,
    /// Revisada en moneda extranjera: la moneda, el tipo y los importes en ella.
    pub fx: Option<ReceivedFx>,
    /// Revisada: fecha de registro (la peninsular de la revisión), la que la lleva al 303.
    pub registered_on: Option<Date>,
}

/// Factura recibida en moneda extranjera: moneda, tipo e importes en la moneda.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceivedFx {
    pub currency: ForeignCurrency,
    pub base: Money,
    pub vat: Money,
    #[serde(default)]
    pub withholding: Money,
    pub total: Money,
    /// IVA en euros que figura en la factura (art. 12 RD 1619/2012: la cuota se expresa en
    /// euros). Es el que se deduce; si no se indica, la base en euros por el tipo de IVA.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vat_eur: Option<Money>,
}

/// Tipos de IVA (con y sin recargo de equivalencia) con los que se reconoce la cuota de una
/// factura recibida en otra moneda.
const VAT_RATES_BP: [i32; 8] = [2100, 1000, 500, 400, 2620, 1140, 562, 450];

/// IVA en euros de una factura en otra moneda como lo calcula quien la emite: la base en euros
/// por el tipo de IVA (art. 79.Once LIVA y art. 12 RD 1619/2012), si la cuota en la moneda es la
/// de un tipo español; si no (varios tipos, cuota redondeada por línea…), `None`.
pub fn euro_vat(base: Money, vat: Money, base_eur: Money) -> Option<Money> {
    if vat.is_zero() {
        return Some(Money::ZERO);
    }
    VAT_RATES_BP
        .iter()
        .map(|bp| taxpain_core::money::Rate::from_bp(*bp))
        .find(|r| base.apply_rate(*r) == vat)
        .map(|r| base_eur.apply_rate(r))
}

impl ReceivedFx {
    /// La revisión con los importes en euros: base y retención al tipo (art. 79.Once LIVA), el
    /// IVA en euros de la factura (o la base en euros por el tipo de IVA) y el total sumado igual
    /// que en la moneda (con o sin la retención restada).
    pub fn in_euros(&self, r: &Reviewed) -> Result<Reviewed> {
        let c = &self.currency;
        c.validate().map_err(|e| AppError::Other(e.to_string()))?;
        if r.import_dua {
            return Err(AppError::Other(
                "un DUA de importación va siempre en euros (lo liquida la aduana)".into(),
            ));
        }
        let (base, withholding) = (c.to_eur(self.base), c.to_eur(self.withholding));
        let converted = c.to_eur(self.vat);
        let vat = match self.vat_eur {
            Some(v) => {
                // La de la factura se calcula sobre la base en euros: puede separarse de la
                // convertida en unos céntimos (hasta 5, o un 1 % si es más), no más.
                let diff = (v - converted).abs().cents();
                if v.is_negative() != converted.is_negative()
                    || (diff > 5 && diff * 100 > converted.abs().cents())
                {
                    return Err(AppError::Other(format!(
                        "el IVA en euros ({v}) no se corresponde con el IVA en {} al tipo \
                         ({converted}): revisa el importe",
                        c.code
                    )));
                }
                v
            }
            None => euro_vat(self.base, self.vat, base).unwrap_or(converted),
        };
        let total = if self.total == self.base + self.vat - self.withholding {
            base + vat - withholding
        } else if self.total == self.base + self.vat {
            base + vat
        } else {
            return Err(AppError::Other(format!(
                "los importes en {} no cuadran: base + IVA − retención ≠ total",
                c.code
            )));
        };
        Ok(Reviewed {
            base,
            vat,
            withholding,
            total,
            ..r.clone()
        })
    }
}

/// Datos confirmados por el usuario en la revisión.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reviewed {
    pub supplier_nif: String,
    pub supplier_name: String,
    pub number: String,
    pub issue_date: Date,
    pub base: Money,
    pub vat: Money,
    pub withholding: Money,
    pub total: Money,
    /// Porcentaje deducible (100 salvo uso mixto).
    pub deductible_pct: u8,
    /// Bien de inversión (art. 108 LIVA): va a las casillas de inversión del 303/390.
    #[serde(default)]
    pub capital_good: bool,
    /// Naturaleza del gasto: decide si la retención va al 115/180 o al 111/190.
    #[serde(default)]
    pub nature: Nature,
    /// Clase de inmovilizado si es bien de inversión (cuenta y coeficiente de amortización).
    #[serde(default)]
    pub asset_class: Option<AssetClass>,
    /// Régimen de IVA de la compra: interior (con IVA en la factura) o con inversión del
    /// sujeto pasivo (sin IVA; se autorrepercute).
    #[serde(default)]
    pub vat_regime: VatRegime,
    /// País del proveedor (ISO alfa-2) si no es España.
    #[serde(default)]
    pub supplier_country: Option<String>,
    /// Tipo para autorrepercutir el IVA (por defecto, el general del 21 %).
    #[serde(default)]
    pub self_assessed_rate: Option<taxpain_core::money::Rate>,
    /// Adquisición intracomunitaria de **bienes** (clave A del 349); si no, de servicios (I).
    #[serde(default)]
    pub intra_goods: bool,
    /// El proveedor aplica el criterio de caja (lo dice la factura): su IVA se deduce al pagarla.
    #[serde(default)]
    pub cash_basis: bool,
    /// DUA de importación: base = valor en aduana, IVA = el liquidado en la aduana y total = lo
    /// pagado (el IVA). El proveedor es el agente de aduanas o la AEAT.
    #[serde(default)]
    pub import_dua: bool,
    /// Con prorrata especial: destino de la compra (uso común, sólo operaciones con derecho a
    /// deducir o sólo sin él).
    #[serde(default)]
    pub prorata_use: taxpain_models::m303::ProrataUse,
}

/// Régimen de IVA de una factura recibida.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VatRegime {
    /// Operación interior con IVA repercutido en la factura.
    #[default]
    Interior,
    /// Adquisición intracomunitaria de bienes o servicios (proveedor con NIF-IVA de otro país
    /// de la UE): sin IVA en la factura; se autorrepercute (arts. 13, 15, 84.Uno.2º LIVA).
    Intracomunitaria,
    /// Otra inversión del sujeto pasivo: servicios de empresas de fuera de la UE o ISP interior
    /// (art. 84.Uno.2º LIVA).
    Isp,
}

impl VatRegime {
    pub fn as_str(self) -> &'static str {
        match self {
            VatRegime::Interior => "interior",
            VatRegime::Intracomunitaria => "intracomunitaria",
            VatRegime::Isp => "isp",
        }
    }

    /// El guardado en `invoice_received.vat_regime` (lo desconocido, interior).
    pub fn parse(s: &str) -> VatRegime {
        match s {
            "intracomunitaria" => VatRegime::Intracomunitaria,
            "isp" => VatRegime::Isp,
            _ => VatRegime::Interior,
        }
    }
}

/// Estados miembros de la UE (prefijos de NIF-IVA; Grecia usa «EL»; «XI» = Irlanda del Norte
/// para bienes). Vive en `taxpain_core::eu` para que también lo usen los modelos (349, 303).
pub use taxpain_core::eu::EU_COUNTRIES;

/// Identificador del proveedor normalizado y su país (`None` = España).
fn supplier_id(r: &Reviewed) -> Result<(String, Option<String>)> {
    let country = r
        .supplier_country
        .as_deref()
        .map(|c| c.trim().to_ascii_uppercase())
        .filter(|c| !c.is_empty() && c != "ES");
    match (r.vat_regime, &country) {
        (VatRegime::Intracomunitaria, None) => Err(AppError::Other(
            "una adquisición intracomunitaria es de un proveedor de otro país de la UE".into(),
        )),
        (VatRegime::Intracomunitaria, Some(c))
            if !taxpain_core::eu::is_other_member_state(c, true) =>
        {
            Err(AppError::Other(format!("{c} no es un país de la UE")))
        }
        (_, None) => {
            let nif = Nif::parse(&r.supplier_nif)
                .map_err(|_| AppError::Other("NIF del proveedor no válido".into()))?;
            Ok((nif.as_str().to_owned(), None))
        }
        (_, Some(c)) => {
            let id: String = r
                .supplier_nif
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric())
                .collect::<String>()
                .to_ascii_uppercase();
            // Prefijo del NIF-IVA: el del país salvo Grecia («GR», prefijo «EL»).
            let p = taxpain_core::eu::vies_prefix(c);
            let id = id.strip_prefix(p.as_str()).unwrap_or(&id).to_owned();
            if id.is_empty() || id.len() > 20 {
                return Err(AppError::Other(
                    "identificación fiscal del proveedor no válida".into(),
                ));
            }
            // Se guarda con el prefijo (como el NIF-IVA: «IE1234567T»).
            Ok((format!("{p}{id}"), Some(c.clone())))
        }
    }
}

/// Clase de bien de inversión: cuenta del PGC (grupo 21), amortización acumulada (281x) y
/// coeficiente lineal máximo de la tabla del art. 12.1.a LIS (aplicable al IRPF en
/// estimación directa por el art. 28 LIRPF; `fixtures/accounting/amortizacion.json`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetClass {
    /// 217 Equipos para procesos de información: 25 %.
    Informatica,
    /// 216 Mobiliario: 10 %.
    Mobiliario,
    /// 218 Elementos de transporte (externo): 16 %.
    Transporte,
    /// 213 Maquinaria: 12 %.
    Maquinaria,
    /// 214 Utillaje (útiles y herramientas): 25 %.
    Utillaje,
    /// 215 Otras instalaciones: 10 %.
    Instalaciones,
    /// 211 Construcciones (sin el suelo): 2 %. Su IVA se regulariza durante nueve años.
    Construcciones,
}

impl AssetClass {
    /// (cuenta, amortización acumulada, coeficiente en centésimas de punto).
    pub fn accounts(self) -> (&'static str, &'static str, i32) {
        match self {
            AssetClass::Informatica => ("217", "2817", 2500),
            AssetClass::Mobiliario => ("216", "2816", 1000),
            AssetClass::Transporte => ("218", "2818", 1600),
            AssetClass::Maquinaria => ("213", "2813", 1200),
            AssetClass::Utillaje => ("214", "2814", 2500),
            AssetClass::Instalaciones => ("215", "2815", 1000),
            AssetClass::Construcciones => ("211", "2811", 200),
        }
    }
}

/// Naturaleza de una factura recibida a efectos de los modelos.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Nature {
    #[default]
    General,
    /// Arrendamiento de inmueble urbano (retención → 115/180).
    Alquiler,
    /// Servicios profesionales (retención → 111/190).
    Profesional,
}

impl Nature {
    pub fn as_str(self) -> &'static str {
        match self {
            Nature::General => "general",
            Nature::Alquiler => "alquiler",
            Nature::Profesional => "profesional",
        }
    }

    /// La guardada en `invoice_received.nature` (lo desconocido, general).
    pub fn parse(s: &str) -> Nature {
        match s {
            "alquiler" => Nature::Alquiler,
            "profesional" => Nature::Profesional,
            _ => Nature::General,
        }
    }
}

fn now_str(t: OffsetDateTime) -> String {
    t.format(&Rfc3339).unwrap_or_default()
}

fn own_nif(v: &Vault) -> Option<String> {
    invoicing::settings(v)
        .ok()
        .flatten()
        .and_then(|s| match s.issuer.id {
            PartyId::Nif(n) => Some(n),
            PartyId::Foreign { .. } => None,
        })
}

/// Aviso si en el texto de la factura no aparece nuestro NIF: un ticket (factura simplificada
/// sin los datos del destinatario, art. 7 RD 1619/2012) no da derecho a deducir su IVA (art.
/// 97.Uno.1.º LIVA). Vale para el texto de un PDF y para el leído de una foto o un escaneado.
fn warn_without_own_nif(e: &mut Extraction, text: &str, own_nif: Option<&str>) {
    // El NIF propio sin guiones ni «ES» (en el texto se buscan sólo letras y cifras).
    let mine = own_nif.map(|n| crate::contacts::key("ES", n));
    let flat: String = text
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_uppercase();
    if mine.is_some_and(|n| !n.is_empty() && !flat.contains(&n)) {
        e.warnings.push(
            "no aparece tu NIF: si es un ticket (factura simplificada sin tus datos), su IVA no \
             es deducible (art. 97 LIVA); pide la factura completa"
                .into(),
        );
    }
}

/// Extrae datos según el tipo de fichero.
pub fn extract(filename: &str, bytes: &[u8], own_nif: Option<&str>) -> Extraction {
    let lower = filename.to_lowercase();
    if lower.ends_with(".xml") || lower.ends_with(".xsig") || bytes.starts_with(b"<?xml") {
        if let Some(e) = std::str::from_utf8(bytes)
            .ok()
            .and_then(ingest::from_facturae)
        {
            return e;
        }
        return Extraction {
            source: "xml".into(),
            warnings: vec!["XML no reconocido como Facturae: revisa los datos a mano".into()],
            ..Extraction::default()
        };
    }
    if bytes.starts_with(b"%PDF")
        && let Some(text) = crate::pdf_text::text(bytes).filter(|t| t.trim().len() > 20)
    {
        let mut e = ingest::from_text(&text, own_nif);
        warn_without_own_nif(&mut e, &text, own_nif);
        return e;
    }
    let scanned = bytes.starts_with(b"%PDF");
    if !scanned && !is_image(&lower, bytes) {
        return Extraction {
            source: "otro".into(),
            warnings: vec!["formato no reconocido: rellena los datos a mano".into()],
            ..Extraction::default()
        };
    }
    // Foto o escaneado: el texto lo lee el Mac.
    match crate::carpeta::ocr_text(filename, bytes) {
        Some(text) => {
            let mut e = ingest::from_text(&text, own_nif);
            e.source = "ocr".into();
            e.warnings.push(
                "leída de una foto o un escaneado: comprueba los importes y guarda el papel (una \
                 foto no es una copia digitalizada certificada)"
                    .into(),
            );
            warn_without_own_nif(&mut e, &text, own_nif);
            e
        }
        None => {
            Extraction {
                source: if scanned { "pdf" } else { "imagen" }.into(),
                warnings: vec![if scanned {
                "PDF escaneado sin texto: en el Mac se lee solo; aquí, rellena los datos a mano"
            } else {
                "foto: en el Mac se lee el texto sola; aquí, rellena los datos a mano"
            }
            .into()],
                ..Extraction::default()
            }
        }
    }
}

/// Extensiones de las fotos que se aceptan (las del iPhone incluidas).
pub const IMAGE_EXTENSIONS: &[&str] =
    &["jpg", "jpeg", "png", "heic", "heif", "tif", "tiff", "webp"];

pub(crate) fn is_image(lower_name: &str, bytes: &[u8]) -> bool {
    let by_name = lower_name
        .rsplit_once('.')
        .is_some_and(|(_, ext)| IMAGE_EXTENSIONS.contains(&ext));
    let by_magic = bytes.starts_with(&[0xFF, 0xD8, 0xFF])
        || bytes.starts_with(b"\x89PNG")
        || bytes
            .get(4..12)
            .is_some_and(|b| b == b"ftypheic" || b == b"ftypheix" || b == b"ftypmif1");
    by_name || by_magic
}

/// Tipo MIME del fichero recibido (para guardarlo y abrirlo con la app que toca).
fn mime_of(name: &str, bytes: &[u8]) -> &'static str {
    let lower = name.to_lowercase();
    if bytes.starts_with(b"%PDF") {
        "application/pdf"
    } else if bytes.starts_with(b"<?xml") {
        "application/xml"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.starts_with(b"\x89PNG") {
        "image/png"
    } else if lower.ends_with(".heic") || lower.ends_with(".heif") {
        "image/heic"
    } else if lower.ends_with(".tif") || lower.ends_with(".tiff") {
        "image/tiff"
    } else if lower.ends_with(".webp") {
        "image/webp"
    } else {
        "application/octet-stream"
    }
}

/// Importa un documento recibido. Idempotente: el mismo fichero no se duplica.
pub fn ingest_file(
    v: &Vault,
    filename: &str,
    bytes: &[u8],
    source: Source,
    now: OffsetDateTime,
) -> Result<IngestOutcome> {
    ingest_file_with(|| Ok(v), filename, bytes, source, now)
}

/// Como [`ingest_file`], para el fichero que sube Claude por el servidor MCP: en la auditoría
/// queda como hecho por el asistente (`via: mcp`), no por el usuario, y la factura lleva un aviso
/// que se ve al revisarla (la columna `source` sólo admite manual, carpeta o correo).
pub fn ingest_file_mcp(
    v: &Vault,
    filename: &str,
    bytes: &[u8],
    now: OffsetDateTime,
) -> Result<IngestOutcome> {
    ingest_by(|| Ok(v), filename, bytes, Source::Manual, true, now)
}

/// Como [`ingest_file`], cogiendo el almacén con `vault` (en la app, su cerrojo) sólo para mirar
/// si ya estaba y para guardarlo: la lectura (texto del PDF y reconocimiento de texto del Mac,
/// hasta diez páginas por escaneado) se hace sin él, y mientras tanto la app sigue respondiendo.
pub fn ingest_file_with<G: Deref<Target = Vault>>(
    vault: impl Fn() -> Result<G>,
    filename: &str,
    bytes: &[u8],
    source: Source,
    now: OffsetDateTime,
) -> Result<IngestOutcome> {
    ingest_by(vault, filename, bytes, source, false, now)
}

/// `mcp`: lo sube Claude por el servidor MCP ([`ingest_file_mcp`]) y no el usuario.
fn ingest_by<G: Deref<Target = Vault>>(
    vault: impl Fn() -> Result<G>,
    filename: &str,
    bytes: &[u8],
    source: Source,
    mcp: bool,
    now: OffsetDateTime,
) -> Result<IngestOutcome> {
    if bytes.is_empty() || bytes.len() > MAX_FILE_SIZE {
        return Err(AppError::Other(
            "fichero vacío o demasiado grande (máx. 25 MB)".into(),
        ));
    }
    let sha = hex::encode(Sha256::digest(bytes));
    let own = {
        let v = vault()?;
        if let Some(dup) = duplicate(&v, &sha)? {
            return Ok(dup);
        }
        own_nif(&v)
    };
    let mut extraction = extract(filename, bytes, own.as_deref());
    if mcp {
        extraction
            .warnings
            .push(crate::mcp_ajustes::NOTA_RECIBIDA.into());
    }
    let v = vault()?;
    // Otra importación del mismo fichero pudo terminar mientras se leía.
    if let Some(dup) = duplicate(&v, &sha)? {
        return Ok(dup);
    }
    let conn = v.connection();
    let mime = mime_of(filename, bytes);
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO document(uuid, kind, filename, mime, sha256, size, content, created_at)
         VALUES (?1, 'recibida', ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            format!("recibida-{}", &sha[..32]),
            filename,
            mime,
            sha,
            i64::try_from(bytes.len()).unwrap_or(0),
            bytes,
            now_str(now)
        ],
    )?;
    let doc = tx.last_insert_rowid();
    let ex_json = serde_json::to_string(&extraction).map_err(|e| AppError::Other(e.to_string()))?;
    tx.execute(
        "INSERT INTO invoice_received(document_id, file_sha256, filename, source, status, extraction,
           supplier_nif, supplier_name, number, issue_date, base, vat, withholding, total, created_at)
         VALUES (?1, ?2, ?3, ?4, 'pendiente', ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            doc,
            sha,
            filename,
            source.as_str(),
            ex_json,
            extraction.supplier_nif.as_ref().map(|f| f.value.clone()),
            extraction.supplier_name.as_ref().map(|f| f.value.clone()),
            extraction.number.as_ref().map(|f| f.value.clone()),
            extraction.issue_date.as_ref().map(|f| f.value.to_string()),
            extraction.base.as_ref().map(|f| f.value.cents()),
            extraction.vat.as_ref().map(|f| f.value.cents()),
            extraction.withholding.as_ref().map(|f| f.value.cents()),
            extraction.total.as_ref().map(|f| f.value.cents()),
            now_str(now),
        ],
    )?;
    let id = tx.last_insert_rowid();
    let mut details = serde_json::json!({ "sha256": sha, "source": source.as_str() });
    if mcp {
        details["via"] = "mcp".into();
    }
    audit::append(
        &tx,
        &now_str(now),
        if mcp { Actor::Assistant } else { Actor::User },
        "received.ingest",
        filename,
        &details,
    )?;
    tx.commit()?;
    Ok(IngestOutcome {
        id,
        duplicate: false,
        extraction,
    })
}

/// El documento con esa huella, si ya se importó.
fn duplicate(v: &Vault, sha: &str) -> Result<Option<IngestOutcome>> {
    let row = v
        .connection()
        .query_row(
            "SELECT id, extraction FROM invoice_received WHERE file_sha256 = ?1",
            [sha],
            |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?;
    Ok(row.map(|(id, ex)| IngestOutcome {
        id,
        duplicate: true,
        extraction: serde_json::from_str(&ex).unwrap_or_default(),
    }))
}

/// Importa los ficheros nuevos de una carpeta vigilada (PDF y XML, no recursivo).
/// Devuelve cuántos eran nuevos.
pub fn scan_folder(v: &Vault, dir: &Path, now: OffsetDateTime) -> Result<usize> {
    scan_folder_with(dir, || Ok(v), now)
}

/// Como [`scan_folder`], cogiendo el almacén con `vault` sólo para lo que lo necesita (ver
/// [`ingest_file_with`]).
pub fn scan_folder_with<G: Deref<Target = Vault>>(
    dir: &Path,
    vault: impl Fn() -> Result<G>,
    now: OffsetDateTime,
) -> Result<usize> {
    let mut new = 0;
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| AppError::Other(e.to_string()))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        let ext = path
            .extension()
            .map(|x| x.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        // Sin seguir enlaces simbólicos (podrían traer ficheros de fuera de la carpeta, como en
        // [`crate::carpeta`]) y mirando el tamaño antes de leer.
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink()
            || !meta.is_file()
            || name.starts_with('.')
            || !(matches!(ext.as_str(), "pdf" | "xml" | "xsig")
                || IMAGE_EXTENSIONS.contains(&ext.as_str()))
            || usize::try_from(meta.len()).is_ok_and(|n| n > MAX_FILE_SIZE)
        {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        if !ingest_file_with(&vault, &name, &bytes, Source::Carpeta, now)?.duplicate {
            new += 1;
        }
    }
    Ok(new)
}

pub fn list_received(v: &Vault, status: Option<&str>) -> Result<Vec<ReceivedSummary>> {
    let conn = v.connection();
    let mut stmt = conn.prepare(&format!(
        "SELECT id, filename, status, supplier_nif, supplier_name, number, issue_date, total, extraction,
                json_extract(fx, '$.currency.code'), json_extract(fx, '$.total'), {STORED_COLUMNS}
         FROM invoice_received WHERE ?1 IS NULL OR status = ?1 ORDER BY id DESC"
    ))?;
    let rows = stmt.query_map([status], |r| {
        let ex: String = r.get(8)?;
        let status: String = r.get(2)?;
        let stored = (status == "revisada")
            .then(|| stored_from_row(r, 11))
            .transpose()?;
        let (reviewed, fx, registered_on) = match stored {
            Some(s) => (Some(s.reviewed), s.fx, s.registered_on),
            None => (None, None, None),
        };
        Ok(ReceivedSummary {
            id: r.get(0)?,
            filename: r.get(1)?,
            status,
            supplier_nif: r.get(3)?,
            supplier_name: r.get(4)?,
            number: r.get(5)?,
            issue_date: r.get(6)?,
            total: r.get::<_, Option<i64>>(7)?.map(Money::from_cents),
            extraction: serde_json::from_str(&ex).unwrap_or_default(),
            currency: r.get(9)?,
            currency_total: r.get::<_, Option<i64>>(10)?.map(Money::from_cents),
            reviewed,
            fx,
            registered_on,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Columnas con los datos de la revisión de una recibida (las lee [`stored_from_row`]).
pub(crate) const STORED_COLUMNS: &str = "supplier_nif, supplier_name, number, issue_date, base, \
     vat, withholding, total, deductible_pct, capital_good, nature, vat_regime, supplier_country, \
     self_assessed_rate, intra_goods, cash_basis, import_dua, prorata_use, property, fx, \
     reviewed_at, posted";

/// Datos guardados de una recibida revisada.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Stored {
    pub reviewed: Reviewed,
    pub fx: Option<ReceivedFx>,
    /// Fecha de registro peninsular (la de la revisión): la del 303 y el libro registro.
    pub registered_on: Option<Date>,
    /// `false`: viene del libro del gestor y no tiene asiento en TaxPain.
    pub posted: bool,
}

/// Lee [`STORED_COLUMNS`] desde la columna `at` de la fila.
pub(crate) fn stored_from_row(
    r: &taxpain_vault::rusqlite::Row<'_>,
    at: usize,
) -> taxpain_vault::rusqlite::Result<Stored> {
    let money = |i: usize| -> taxpain_vault::rusqlite::Result<Money> {
        Ok(Money::from_cents(
            r.get::<_, Option<i64>>(at + i)?.unwrap_or(0),
        ))
    };
    let issue: Option<String> = r.get(at + 3)?;
    let issue_date = issue
        .as_deref()
        .and_then(|d| {
            Date::parse(d, time::macros::format_description!("[year]-[month]-[day]")).ok()
        })
        .unwrap_or(Date::MIN);
    let capital_good: bool = r.get(at + 9)?;
    let property: bool = r.get(at + 18)?;
    let regime = VatRegime::parse(&r.get::<_, String>(at + 11)?);
    let fx: Option<String> = r.get(at + 19)?;
    let reviewed_at: Option<String> = r.get(at + 20)?;
    Ok(Stored {
        reviewed: Reviewed {
            supplier_nif: r.get::<_, Option<String>>(at)?.unwrap_or_default(),
            supplier_name: r.get::<_, Option<String>>(at + 1)?.unwrap_or_default(),
            number: r.get::<_, Option<String>>(at + 2)?.unwrap_or_default(),
            issue_date,
            base: money(4)?,
            vat: money(5)?,
            withholding: money(6)?,
            total: money(7)?,
            deductible_pct: u8::try_from(r.get::<_, i64>(at + 8)?.clamp(0, 100)).unwrap_or(100),
            capital_good,
            nature: Nature::parse(&r.get::<_, String>(at + 10)?),
            // La clase no se guarda: sólo si es una construcción (la de la regularización).
            asset_class: (capital_good && property).then_some(AssetClass::Construcciones),
            vat_regime: regime,
            supplier_country: r.get(at + 12)?,
            self_assessed_rate: r
                .get::<_, Option<i32>>(at + 13)?
                .map(taxpain_core::money::Rate::from_bp),
            intra_goods: r.get(at + 14)?,
            cash_basis: r.get(at + 15)?,
            import_dua: r.get(at + 16)?,
            prorata_use: taxpain_models::m303::ProrataUse::parse(&r.get::<_, String>(at + 17)?),
        },
        fx: fx.and_then(|f| serde_json::from_str(&f).ok()),
        registered_on: reviewed_at
            .as_deref()
            .and_then(|t| crate::models::local_date(t).ok()),
        posted: r.get(at + 21)?,
    })
}

/// Estado y datos guardados de la recibida `id`.
pub(crate) fn stored(
    conn: &taxpain_vault::rusqlite::Connection,
    id: i64,
) -> Result<(String, Stored)> {
    conn.query_row(
        &format!("SELECT status, {STORED_COLUMNS} FROM invoice_received WHERE id = ?1"),
        [id],
        |r| Ok((r.get(0)?, stored_from_row(r, 1)?)),
    )
    .optional()?
    .ok_or(AppError::NotFound)
}

/// Confirma los datos de una factura recibida tras revisarlos.
pub fn review(v: &Vault, id: i64, r: &Reviewed, now: OffsetDateTime) -> Result<()> {
    review_fx(v, id, r, None, now)
}

/// [`review`] de una factura en moneda extranjera: los importes de `r` se sustituyen por los de
/// `fx` pasados a euros con su tipo, que debe ser el vigente a la fecha de la factura.
pub fn review_fx(
    v: &Vault,
    id: i64,
    r: &Reviewed,
    fx: Option<&ReceivedFx>,
    now: OffsetDateTime,
) -> Result<()> {
    review_split(v, id, r, fx, None, now)
}

/// Reparto de la base de una recibida en varias cuentas (ver [`review_split`]).
pub(crate) type ExpenseSplit<'a> = Option<&'a [(taxpain_core::accounting::AccountCode, Money)]>;

/// Unos datos revisados ya comprobados ([`check_reviewed`]): los importes en euros y lo que se
/// guarda de ellos.
#[derive(Clone, Debug)]
pub(crate) struct Checked {
    /// Los datos, con los importes en euros.
    pub r: Reviewed,
    pub fx_json: Option<String>,
    /// Proveedor normalizado (NIF o NIF-IVA con su prefijo) y su país (`None` = España).
    pub supplier: String,
    pub country: Option<String>,
    /// Tipo para autorrepercutir (con inversión del sujeto pasivo) y la cuota resultante.
    pub rate: Option<taxpain_core::money::Rate>,
    pub self_assessed: Money,
    pub dua: bool,
    pub asset: Option<AssetClass>,
}

impl Checked {
    /// La factura para contabilizarla.
    pub(crate) fn invoice(&self, id: i64) -> taxpain_core::accounting::ReceivedInvoice {
        let r = &self.r;
        taxpain_core::accounting::ReceivedInvoice {
            id,
            date: r.issue_date,
            number: r.number.trim().to_owned(),
            supplier_name: r.supplier_name.trim().to_owned(),
            base: r.base,
            vat: r.vat,
            withholding: r.withholding,
            deductible_pct: r.deductible_pct,
            expense_account: self
                .asset
                .map(|c| taxpain_core::accounting::account::acc(c.accounts().0)),
            self_assessed_vat: self.self_assessed,
            import_dua: self.dua,
        }
    }
}

/// Comprueba los datos de la revisión de la recibida `id` (los mismos para revisarla y para
/// corregirla): moneda y tipo, proveedor, importes, número repetido, cuota de leasing ya
/// registrada y clase de bien de inversión.
pub(crate) fn check_reviewed(
    v: &Vault,
    id: i64,
    r: &Reviewed,
    fx: Option<&ReceivedFx>,
) -> Result<Checked> {
    let converted;
    let r = match fx {
        Some(f) => {
            f.currency
                .check_rate_date(r.issue_date)
                .map_err(|e| AppError::Other(e.to_string()))?;
            crate::fx::check_bce(v, Some(&f.currency))?;
            converted = f.in_euros(r)?;
            &converted
        }
        None => r,
    };
    let fx_json = fx
        .map(serde_json::to_string)
        .transpose()
        .map_err(|e| AppError::Other(e.to_string()))?;
    let (supplier, country) = supplier_id(r)?;
    // Con inversión del sujeto pasivo la factura no lleva IVA: se autorrepercute.
    let rate = match r.vat_regime {
        VatRegime::Interior => None,
        _ => {
            if !r.vat.is_zero() {
                return Err(AppError::Other(
                    "con inversión del sujeto pasivo la factura no lleva IVA: deja el IVA a 0"
                        .into(),
                ));
            }
            Some(
                r.self_assessed_rate
                    .unwrap_or(taxpain_core::money::Rate::percent(21)),
            )
        }
    };
    let self_assessed = rate.map_or(Money::ZERO, |t| r.base.apply_rate(t));
    if r.number.trim().is_empty() {
        return Err(AppError::Other("falta el número de factura".into()));
    }
    let dua = r.import_dua && r.vat_regime == VatRegime::Interior;
    if dua && r.vat != r.total {
        return Err(AppError::Other(format!(
            "en un DUA el total es el IVA pagado en la aduana ({}), no {}",
            r.vat, r.total
        )));
    }
    if !dua && r.base + r.vat - r.withholding != r.total && r.base + r.vat != r.total {
        return Err(AppError::Other(format!(
            "los importes no cuadran: base {} + IVA {} − retención {} ≠ total {}",
            r.base, r.vat, r.withholding, r.total
        )));
    }
    if r.deductible_pct > 100 {
        return Err(AppError::Other("porcentaje deducible no válido".into()));
    }
    let conn = v.connection();
    let dup: Option<i64> = conn
        .query_row(
            "SELECT id FROM invoice_received WHERE status = 'revisada' AND supplier_nif = ?1 AND number = ?2 AND id <> ?3",
            params![supplier, r.number.trim(), id],
            |row| row.get(0),
        )
        .optional()?;
    if dup.is_some() {
        return Err(AppError::Other(
            "ya hay una factura revisada con ese proveedor y número".into(),
        ));
    }
    // La misma con el número escrito de otra forma (la del libro del gestor, por ejemplo):
    // revisarla otra vez deduciría su IVA dos veces.
    if let Some(n) =
        crate::libros::reviewed_duplicate(conn, &supplier, r.number.trim(), r.issue_date, id)?
    {
        return Err(AppError::Other(format!(
            "esa factura ya está en el libro de recibidas como «{n}» (mismo proveedor y año): \
             descarta esta"
        )));
    }
    // La cuota de un leasing ya registrada desde Leasing: es la misma factura.
    if let Some((msg, registered)) = crate::leasing::installment_clash(
        conn,
        &supplier,
        r.number.trim(),
        r.issue_date,
        r.base,
        r.total,
    )? {
        // Qué hacer con ésta depende de dónde viene: tecleada a mano o un documento.
        let typed: bool = conn
            .query_row(
                "SELECT CAST(substr(d.content, 1, 40) AS TEXT) LIKE ?2 || '%'
                 FROM invoice_received r JOIN document d ON d.id = r.document_id
                 WHERE r.id = ?1",
                params![id, TYPED_MARK],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(false);
        let tail = match (registered, typed) {
            (false, _) => "",
            (true, true) => " No hace falta registrarla aquí.",
            (true, false) => {
                " Descarta esta; si quieres guardar el PDF, adjúntalo al asiento de esa cuota en \
                 el Diario."
            }
        };
        return Err(AppError::Other(format!("{msg}{tail}")));
    }
    let asset = match (r.capital_good, r.asset_class) {
        (true, None) => {
            return Err(AppError::Other(
                "indica qué tipo de bien de inversión es".into(),
            ));
        }
        (true, Some(c)) => Some(c),
        (false, _) => None,
    };
    Ok(Checked {
        r: r.clone(),
        fx_json,
        supplier,
        country,
        rate,
        self_assessed,
        dua,
        asset,
    })
}

/// Guarda los datos comprobados en la fila de la recibida `id` si está en el estado `from`, y
/// la deja revisada. `reviewed_at`: la fecha de registro, sólo al revisarla (al corregirla se
/// conserva la de entonces, la que la llevó a su 303). Devuelve las filas cambiadas (0 o 1).
pub(crate) fn update_row(
    conn: &taxpain_vault::rusqlite::Connection,
    id: i64,
    c: &Checked,
    reviewed_at: Option<String>,
    from: &str,
) -> Result<usize> {
    let r = &c.r;
    Ok(conn.execute(
        "UPDATE invoice_received SET status = 'revisada', supplier_nif = ?1, supplier_name = ?2, number = ?3,
           issue_date = ?4, base = ?5, vat = ?6, withholding = ?7, total = ?8, deductible_pct = ?9,
           reviewed_at = COALESCE(?10, reviewed_at),
           capital_good = ?12, nature = ?13, vat_regime = ?14, supplier_country = ?15,
           self_assessed_rate = ?16, intra_goods = ?17, cash_basis = ?18, import_dua = ?19,
           prorata_use = ?20, property = ?21, fx = ?22
         WHERE id = ?11 AND status = ?23",
        params![
            c.supplier,
            r.supplier_name.trim(),
            r.number.trim(),
            r.issue_date.to_string(),
            r.base.cents(),
            r.vat.cents(),
            r.withholding.cents(),
            r.total.cents(),
            r.deductible_pct,
            reviewed_at,
            id,
            r.capital_good,
            r.nature.as_str(),
            r.vat_regime.as_str(),
            c.country,
            c.rate.map(|t| t.bp()),
            r.vat_regime == VatRegime::Intracomunitaria && r.intra_goods,
            r.vat_regime == VatRegime::Interior && r.cash_basis,
            c.dua,
            r.prorata_use.code(),
            c.asset == Some(AssetClass::Construcciones),
            c.fx_json,
            from
        ],
    )?)
}

/// [`review_fx`] con la base repartida en varias cuentas (`split`, que suma la base) en vez de
/// ir entera a la de gasto: la cuota de un leasing (capital a la 524 e intereses a la 662).
pub(crate) fn review_split(
    v: &Vault,
    id: i64,
    r: &Reviewed,
    fx: Option<&ReceivedFx>,
    split: ExpenseSplit<'_>,
    now: OffsetDateTime,
) -> Result<()> {
    let checked = check_reviewed(v, id, r, fx)?;
    // Se registra hoy (la fecha del 303 y del libro registro); si ya estaba registrada, también
    // cambia lo que se registró entonces.
    let hoy = taxpain_core::spain_time::peninsular_date(now);
    let antes = stored(v.connection(), id)
        .ok()
        .and_then(|(_, s)| s.registered_on)
        .filter(|d| *d != hoy);
    for d in std::iter::once(hoy).chain(antes) {
        crate::periodos::exigir_abierto(
            v.connection(),
            d,
            &format!(
                "registrar o cambiar la recibida {} (registro del {d})",
                r.number
            ),
            now,
        )?;
    }
    let (r, supplier, self_assessed, asset) = (
        &checked.r,
        &checked.supplier,
        checked.self_assessed,
        checked.asset,
    );
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    // Fecha de registro con el desfase peninsular: el día que cuenta para el periodo (y el que
    // se ve en el libro registro) es el de España, no el de UTC.
    let at = now_str(now.to_offset(taxpain_core::spain_time::peninsular_offset(now)));
    if update_row(&tx, id, &checked, Some(at), "pendiente")? == 0 {
        // Una vez revisada ya está contabilizada (y en los modelos): se corrige con «Corregir».
        return Err(not_pending(&tx, id)?);
    }
    crate::accounting::post_received_invoice_split(
        &tx,
        &checked.invoice(id),
        supplier,
        split,
        now,
    )?;
    // Bien de inversión: se da de alta en el inmovilizado por su coste (base + IVA no
    // deducible) para amortizarlo, en vez de ir a gasto (art. 12 LIS, art. 28 LIRPF).
    if let Some(class) = asset {
        use taxpain_core::accounting::account::acc;
        let (account, dep, rate) = class.accounts();
        let iva = r.vat + self_assessed;
        let deductible = iva.apply_rate(taxpain_core::money::Rate::percent(i32::from(
            r.deductible_pct,
        )));
        let asset_id = crate::accounting::add_fixed_asset(
            v,
            &taxpain_core::accounting::depreciation::FixedAsset {
                id: 0,
                name: format!("{} · {}", r.supplier_name.trim(), r.number.trim()),
                account: acc(account),
                depreciation_account: acc(dep),
                expense_account: acc("681"), // Amortización del inmovilizado material
                acquired_on: r.issue_date,
                cost: r.base + (iva - deductible),
                residual: Money::ZERO,
                rate: taxpain_core::money::Rate::from_bp(rate),
                disposed_on: None,
                method: Default::default(),
                tax_rate: None,
            },
            now,
        )?;
        // El libro de bienes de inversión del IVA (arts. 107 a 110 LIVA) sigue al elemento
        // desde su recibida.
        crate::iva_holding::enlazar_bien(
            &tx,
            asset_id,
            id,
            class == AssetClass::Construcciones,
            now,
        )?;
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "received.review",
        &r.number,
        &serde_json::json!({
            "supplier": supplier,
            "total": r.total.cents(),
            "vat_regime": r.vat_regime.as_str(),
            "moneda": fx.map(|f| serde_json::json!({
                "code": f.currency.code,
                "rate": f.currency.rate.decimal(),
                "rate_date": f.currency.rate_date.to_string(),
                "total": f.total.cents(),
            })),
        }),
    )?;
    tx.commit()?;
    Ok(())
}

/// Error para una recibida que no está pendiente (o no existe).
fn not_pending(conn: &taxpain_vault::rusqlite::Connection, id: i64) -> Result<AppError> {
    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM invoice_received WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(match status.as_deref() {
        None => AppError::NotFound,
        Some("revisada") => AppError::Other(
            "la factura ya está revisada y contabilizada: para cambiarla, usa «Corregir…»".into(),
        ),
        Some(_) => AppError::Other("la factura está descartada".into()),
    })
}

/// Principio del documento de texto que se guarda con una factura tecleada sin documento.
const TYPED_MARK: &str = "Factura recibida tecleada en TaxPain";

/// Registra una factura recibida tecleada (desde el diario) sin documento: se guarda como
/// documento un resumen en texto y se revisa con los datos dados, como cualquier otra (IVA,
/// asiento, cartera, 347…). Si los datos no valen, no queda nada. Devuelve su id.
pub fn register_typed(v: &Vault, r: &Reviewed, now: OffsetDateTime) -> Result<i64> {
    register_typed_fx(v, r, None, now)
}

/// [`register_typed`] de una factura en moneda extranjera (ver [`review_fx`]).
pub fn register_typed_fx(
    v: &Vault,
    r: &Reviewed,
    fx: Option<&ReceivedFx>,
    now: OffsetDateTime,
) -> Result<i64> {
    register_typed_split(v, r, fx, None, now)
}

/// [`register_typed_fx`] con la base repartida en varias cuentas (ver [`review_split`]).
pub(crate) fn register_typed_split(
    v: &Vault,
    r: &Reviewed,
    fx: Option<&ReceivedFx>,
    split: ExpenseSplit<'_>,
    now: OffsetDateTime,
) -> Result<i64> {
    let hoy = taxpain_core::spain_time::peninsular_date(now);
    crate::periodos::exigir_abierto(
        v.connection(),
        hoy,
        &format!("registrar la recibida {} (registro del {hoy})", r.number),
        now,
    )?;
    let converted;
    let shown = match fx {
        Some(f) => {
            converted = f.in_euros(r)?;
            &converted
        }
        None => r,
    };
    let currency_line = fx.map_or(String::new(), |f| {
        format!(
            "Moneda: {} (total {} {}; 1 € = {} {} del {})\n",
            f.currency.code,
            f.total.to_string().trim_end_matches(" €"),
            f.currency.code,
            f.currency.rate,
            f.currency.code,
            f.currency.rate_date
        )
    });
    let r = shown;
    let text = format!(
        "{TYPED_MARK}, sin documento.\n\
         Proveedor: {} {}\nNúmero: {}\nFecha: {}\nBase: {}\nIVA: {}\nRetención: {}\n\
         Total: {}\n{currency_line}Registrada: {}\n",
        r.supplier_nif.trim(),
        r.supplier_name.trim(),
        r.number.trim(),
        r.issue_date,
        r.base,
        r.vat,
        r.withholding,
        r.total,
        now_str(now)
    );
    let safe: String = format!("{}-{}", r.supplier_nif.trim(), r.number.trim())
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let out = ingest_file(
        v,
        &format!("{safe}.txt"),
        text.as_bytes(),
        Source::Manual,
        now,
    )?;
    if out.duplicate {
        return Err(AppError::Other("esa factura ya está registrada".into()));
    }
    if let Err(e) = review_split(v, out.id, r, fx, split, now) {
        let conn = v.connection();
        let doc: Option<i64> = conn
            .query_row(
                "SELECT document_id FROM invoice_received WHERE id = ?1 AND status = 'pendiente'",
                [out.id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(doc) = doc {
            conn.execute("DELETE FROM invoice_received WHERE id = ?1", [out.id])?;
            conn.execute("DELETE FROM document WHERE id = ?1", [doc])?;
        }
        return Err(e);
    }
    Ok(out.id)
}

pub fn discard(v: &Vault, id: i64, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let n = conn.execute(
        "UPDATE invoice_received SET status = 'descartada' WHERE id = ?1 AND status = 'pendiente'",
        [id],
    )?;
    if n == 0 {
        return Err(not_pending(conn, id)?);
    }
    audit::append(
        conn,
        &now_str(now),
        Actor::User,
        "received.discard",
        &id.to_string(),
        &serde_json::json!({}),
    )?;
    Ok(())
}

/// Recupera una recibida descartada por error: vuelve a «pendiente de revisar». Es el único
/// camino de vuelta, porque reimportar el mismo fichero lo frena la huella (`duplicate`).
pub fn undiscard(v: &Vault, id: i64, now: OffsetDateTime) -> Result<()> {
    let conn = v.connection();
    let n = conn.execute(
        "UPDATE invoice_received SET status = 'pendiente' WHERE id = ?1 AND status = 'descartada'",
        [id],
    )?;
    if n == 0 {
        let status: Option<String> = conn
            .query_row(
                "SELECT status FROM invoice_received WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?;
        return Err(match status {
            None => AppError::NotFound,
            Some(_) => AppError::Other("la factura no está descartada".into()),
        });
    }
    audit::append(
        conn,
        &now_str(now),
        Actor::User,
        "received.undiscard",
        &id.to_string(),
        &serde_json::json!({}),
    )?;
    Ok(())
}

/// Documento original de una recibida: (nombre, MIME, bytes).
pub fn received_document(v: &Vault, id: i64) -> Result<(String, String, Vec<u8>)> {
    v.connection()
        .query_row(
            "SELECT d.filename, d.mime, d.content FROM invoice_received r JOIN document d ON d.id = r.document_id WHERE r.id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)
}

/// Texto del documento de una recibida (PDF con capa de texto), para extraerlo con el
/// asistente. Recortado a 12.000 caracteres.
pub fn document_text(v: &Vault, id: i64) -> Result<String> {
    let (filename, _, bytes) = received_document(v, id)?;
    let lower = filename.to_lowercase();
    // Fotos y escaneados, con el texto que lee el Mac (el de la importación, que se recuerda):
    // nunca el binario de la imagen como si fuera texto.
    let text = if bytes.starts_with(b"%PDF") {
        crate::pdf_text::text(&bytes)
            .filter(|t| t.split_whitespace().count() > 3)
            .or_else(|| crate::carpeta::ocr_text(&filename, &bytes))
            .ok_or_else(|| AppError::Other("no se pudo leer el texto del PDF".into()))?
    } else if is_image(&lower, &bytes) {
        crate::carpeta::ocr_text(&filename, &bytes).ok_or_else(|| {
            AppError::Other(
                "la foto no tiene texto que se pueda leer: rellena los datos a mano".into(),
            )
        })?
    } else {
        std::str::from_utf8(&bytes)
            .map_err(|_| {
                AppError::Other("el documento no es texto: rellena los datos a mano".into())
            })?
            .to_owned()
    };
    let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.len() < 20 {
        return Err(AppError::Other(
            "el documento no tiene texto (¿escaneado?): rellena los datos a mano".into(),
        ));
    }
    Ok(text.chars().take(12_000).collect())
}

/// Instrucciones para que el asistente extraiga los datos de una factura recibida.
pub const AI_EXTRACTION_PROMPT: &str = "\
Extrae los datos de esta factura RECIBIDA (soy el destinatario). Responde SOLO con un objeto \
JSON, sin texto alrededor, con estas claves (null si no aparece): \
\"supplier_nif\" (NIF o NIF-IVA del emisor, tal cual aparece, aunque sea un marcador como \
[NIF_1]), \"supplier_name\", \"number\" (serie y número), \"issue_date\" (AAAA-MM-DD), \
\"base\", \"vat\", \"withholding\", \"total\" (importes en euros con punto decimal, p. ej. \
1234.56; \"withholding\" es la retención de IRPF, positiva). No inventes datos.\n\nFactura:\n";

/// Importe escrito por el asistente: «1234.56», «1.234,56», «1,234.56», «-12,5», «1.234,56 €».
/// El último separador es el decimal (si le siguen 1 o 2 cifras); más de 2 decimales no se
/// aceptan. A diferencia de [`crate::import::parse_amount`] (celdas de una hoja de cálculo),
/// aquí «1.500» no es de miles: el modelo responde con punto decimal.
pub(crate) fn parse_amount(raw: &str) -> Option<Money> {
    let raw: String = raw
        .trim()
        .chars()
        .filter(|c| !matches!(c, '€' | ' ' | '\u{a0}' | '+'))
        .collect();
    let neg = raw.starts_with('-');
    let s = raw.trim_start_matches('-');
    let last = s.rfind(['.', ',']);
    let (int, dec) = match last {
        Some(i) if s.len() - i - 1 <= 2 => (&s[..i], &s[i + 1..]),
        _ => (s, ""),
    };
    let int: String = int.chars().filter(|c| *c != '.' && *c != ',').collect();
    if int.is_empty()
        || !int.bytes().all(|b| b.is_ascii_digit())
        || !dec.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let euros: i64 = int.parse().ok()?;
    let cents: i64 = format!("{dec:0<2}").parse().ok()?;
    let total = euros.checked_mul(100)?.checked_add(cents)?;
    Some(Money::from_cents(if neg { -total } else { total }))
}

/// Lee la respuesta del asistente y la convierte en una extracción para revisar (confianza
/// media: siempre la revisa el usuario).
pub fn parse_ai_extraction(reply: &str) -> Extraction {
    use taxpain_invoicing::ingest::{Confidence, Field};
    let mut e = Extraction {
        source: "asistente".into(),
        ..Extraction::default()
    };
    let json = reply
        .find('{')
        .zip(reply.rfind('}'))
        .and_then(|(a, b)| reply.get(a..=b))
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
    let Some(j) = json else {
        e.warnings
            .push("el asistente no devolvió datos legibles: rellénalos a mano".into());
        return e;
    };
    let text = |k: &str| {
        j.get(k)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };
    let money = |k: &str| -> Option<Money> {
        let raw = match j.get(k)? {
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s.trim().trim_end_matches('€').trim().to_owned(),
            _ => return None,
        };
        parse_amount(&raw)
    };
    fn medium<T>(value: T) -> Option<Field<T>> {
        Some(Field {
            value,
            confidence: Confidence::Media,
        })
    }
    if let Some(nif) = text("supplier_nif") {
        let clean: String = nif
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect::<String>()
            .to_uppercase();
        let conf = if Nif::parse(&clean).is_ok() {
            Confidence::Media
        } else {
            Confidence::Baja
        };
        e.supplier_nif = Some(Field {
            value: clean,
            confidence: conf,
        });
    }
    e.supplier_name = text("supplier_name").and_then(medium);
    e.number = text("number").and_then(medium);
    e.issue_date = text("issue_date")
        .and_then(|d| {
            Date::parse(
                &d,
                time::macros::format_description!("[year]-[month]-[day]"),
            )
            .ok()
        })
        .and_then(medium);
    e.base = money("base").and_then(medium);
    e.vat = money("vat").and_then(medium);
    e.withholding = money("withholding").map(|m| m.abs()).and_then(medium);
    e.total = money("total").and_then(medium);
    if let (Some(b), Some(t)) = (&e.base, &e.total) {
        let vat = e.vat.as_ref().map_or(Money::ZERO, |v| v.value);
        let wh = e.withholding.as_ref().map_or(Money::ZERO, |v| v.value);
        if t.value != b.value + vat && t.value != b.value + vat - wh {
            e.warnings.push(format!(
                "los importes no cuadran: base {} + IVA {} ≠ total {}",
                b.value, vat, t.value
            ));
        }
    }
    e.warnings
        .push("datos propuestos por el asistente: revísalos antes de confirmar".into());
    e
}

/// Recibidas pendientes a las que les falta algún dato principal (proveedor, número, fecha o
/// total): las que conviene completar con el asistente.
pub fn pending_incomplete(v: &Vault) -> Result<Vec<i64>> {
    let mut stmt = v.connection().prepare(
        "SELECT id FROM invoice_received WHERE status = 'pendiente'
           AND (supplier_nif IS NULL OR number IS NULL OR issue_date IS NULL OR total IS NULL)
         ORDER BY id",
    )?;
    let ids = stmt.query_map([], |r| r.get(0))?;
    Ok(ids.collect::<std::result::Result<_, _>>()?)
}

/// Completa la extracción guardada de una recibida pendiente con la del asistente: sólo los
/// datos que faltan o que se leyeron con confianza baja. Sigue pendiente: la revisa el usuario.
pub fn merge_extraction(v: &Vault, id: i64, ai: &Extraction) -> Result<()> {
    use taxpain_invoicing::ingest::Confidence;
    let conn = v.connection();
    let stored: String = conn
        .query_row(
            "SELECT extraction FROM invoice_received WHERE id = ?1 AND status = 'pendiente'",
            [id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let mut e: Extraction = serde_json::from_str(&stored).unwrap_or_default();
    fn take<T: Clone>(
        mine: &mut Option<taxpain_invoicing::ingest::Field<T>>,
        theirs: &Option<taxpain_invoicing::ingest::Field<T>>,
    ) {
        if theirs.is_some()
            && mine
                .as_ref()
                .is_none_or(|f| f.confidence == Confidence::Baja)
        {
            mine.clone_from(theirs);
        }
    }
    take(&mut e.supplier_nif, &ai.supplier_nif);
    take(&mut e.supplier_name, &ai.supplier_name);
    take(&mut e.number, &ai.number);
    take(&mut e.issue_date, &ai.issue_date);
    take(&mut e.base, &ai.base);
    take(&mut e.vat, &ai.vat);
    take(&mut e.withholding, &ai.withholding);
    take(&mut e.total, &ai.total);
    e.warnings.extend(ai.warnings.iter().cloned());
    e.warnings
        .push("completada con el asistente: revisa los datos".into());
    let json = serde_json::to_string(&e).map_err(|err| AppError::Other(err.to_string()))?;
    conn.execute(
        "UPDATE invoice_received SET extraction = ?1, supplier_nif = ?2, supplier_name = ?3,
           number = ?4, issue_date = ?5, base = ?6, vat = ?7, withholding = ?8, total = ?9
         WHERE id = ?10 AND status = 'pendiente'",
        params![
            json,
            e.supplier_nif.as_ref().map(|f| f.value.clone()),
            e.supplier_name.as_ref().map(|f| f.value.clone()),
            e.number.as_ref().map(|f| f.value.clone()),
            e.issue_date.as_ref().map(|f| f.value.to_string()),
            e.base.as_ref().map(|f| f.value.cents()),
            e.vat.as_ref().map(|f| f.value.cents()),
            e.withholding.as_ref().map(|f| f.value.cents()),
            e.total.as_ref().map(|f| f.value.cents()),
            id
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod ai_tests {
    use super::*;

    #[test]
    fn lee_la_respuesta_del_asistente() {
        let reply = "Aquí tienes:\n```json\n{\"supplier_nif\": \"B-12345674\", \"supplier_name\": \"Suministros Levante SL\", \"number\": \"FV-118\", \"issue_date\": \"2026-10-05\", \"base\": 45, \"vat\": \"9,45\", \"withholding\": null, \"total\": 54.45}\n```";
        let e = parse_ai_extraction(reply);
        assert_eq!(e.source, "asistente");
        assert_eq!(
            e.supplier_nif.as_ref().map(|f| f.value.as_str()),
            Some("B12345674")
        );
        assert_eq!(
            e.base.as_ref().map(|f| f.value),
            Some(Money::from_euros(45, 0))
        );
        assert_eq!(
            e.vat.as_ref().map(|f| f.value),
            Some(Money::from_cents(945))
        );
        assert_eq!(
            e.total.as_ref().map(|f| f.value),
            Some(Money::from_cents(5445))
        );
        assert!(e.withholding.is_none());
        assert_eq!(e.warnings.len(), 1, "{:?}", e.warnings);
        for (raw, cents) in [
            ("1.234,56", 123_456),
            ("1,234.56", 123_456),
            ("1234.5", 123_450),
            ("-12,50", -1_250),
            ("1.234", 123_400),
            (" 1.234,56 €", 123_456),
            ("+12", 1_200),
        ] {
            assert_eq!(parse_amount(raw), Some(Money::from_cents(cents)), "{raw}");
        }
        assert_eq!(parse_amount("12.345.678,9x"), None);
        // Importes que no cuadran y respuesta sin JSON.
        let bad = parse_ai_extraction("{\"base\": 100, \"vat\": 21, \"total\": 150}");
        assert!(bad.warnings.iter().any(|w| w.contains("no cuadran")));
        assert!(parse_ai_extraction("no sé").supplier_name.is_none());
    }
}
