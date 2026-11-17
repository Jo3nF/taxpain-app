//! Documentos de la empresa (escrituras, tarjeta NIF, alta en el ROI/NIF-IVA, 036/037,
//! certificados, nota simple, estatutos, IAE, contratos…): se reconocen por su texto, se guardan
//! cifrados en el almacén y se leen sus datos (NIF, razón social, domicilio, fecha de
//! constitución, capital, CNAE, epígrafe del IAE, administrador, notario, inscripción…).
//!
//! Con esos datos se **proponen** cambios en la ficha de la empresa, la facturación, los
//! modelos y el perfil fiscal; no se aplica nada sin que el usuario lo acepte.
//!
//! La lectura es por reglas sobre el texto del PDF. `TODO(VERIFY)` con documentos reales: los
//! escaneados no tienen texto (sin OCR aún; se pueden completar con el asistente si llevan capa
//! de texto, o a mano).

use std::collections::BTreeMap;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use taxpain_core::Nif;
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{OptionalExtension, params};
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::invoicing::now_str;

/// Tipo de documento de la empresa.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DocKind {
    EscrituraConstitucion,
    EscrituraPoderes,
    EscrituraModificacion,
    Escritura,
    Estatutos,
    TarjetaNif,
    Censal036,
    AltaRoi,
    CertificadoAeat,
    CertificadoSeguridadSocial,
    AltaSeguridadSocial,
    RegistroMercantil,
    CuentasAnuales,
    Iae,
    ActaJunta,
    ContratoAlquiler,
    Seguro,
    Licencia,
    // Documentos de la Sede de la AEAT (ver [`crate::oficiales`]).
    CertificadoSituacionCensal,
    CertificadoIae,
    CertificadoResidencia,
    CertificadoTributario,
    RequerimientoAeat,
    NotificacionAeat,
    RegistroAeat,
    ResolucionAeat,
    LiquidacionAeat,
    CartaPagoAeat,
    DocumentoAeat,
    /// Certificado de la agencia tributaria de una comunidad autónoma, de una hacienda foral o
    /// de un ayuntamiento: no es de la AEAT.
    CertificadoOtraAdministracion,
}

impl DocKind {
    pub fn title(self) -> &'static str {
        match self {
            DocKind::EscrituraConstitucion => "Escritura de constitución",
            DocKind::EscrituraPoderes => "Escritura de poderes",
            DocKind::EscrituraModificacion => "Escritura de modificación",
            DocKind::Escritura => "Escritura notarial",
            DocKind::Estatutos => "Estatutos sociales",
            DocKind::TarjetaNif => "Tarjeta o asignación del NIF",
            DocKind::Censal036 => "Declaración censal (036/037)",
            DocKind::AltaRoi => "Alta en el ROI (NIF-IVA, operaciones intracomunitarias)",
            DocKind::CertificadoAeat => "Certificado de estar al corriente con la AEAT",
            DocKind::CertificadoSeguridadSocial => {
                "Certificado de estar al corriente con la Seguridad Social"
            }
            DocKind::AltaSeguridadSocial => "Alta en la Seguridad Social (empresa o RETA)",
            DocKind::RegistroMercantil => "Registro Mercantil (nota simple o certificación)",
            DocKind::CuentasAnuales => "Cuentas anuales depositadas",
            DocKind::Iae => "Impuesto sobre Actividades Económicas (alta o 840)",
            DocKind::ActaJunta => "Acta de la junta",
            DocKind::ContratoAlquiler => "Contrato de arrendamiento",
            DocKind::Seguro => "Póliza de seguro",
            DocKind::Licencia => "Licencia de actividad",
            DocKind::CertificadoSituacionCensal => "Certificado de situación censal",
            DocKind::CertificadoIae => "Certificado de alta en el IAE",
            DocKind::CertificadoResidencia => "Certificado de residencia fiscal",
            DocKind::CertificadoTributario => "Certificado tributario de la AEAT",
            DocKind::RequerimientoAeat => "Requerimiento de la AEAT",
            DocKind::NotificacionAeat => "Notificación o comunicación de la AEAT",
            DocKind::RegistroAeat => "Recibo de presentación en la Sede de la AEAT",
            DocKind::ResolucionAeat => "Resolución o acuerdo de la AEAT",
            DocKind::LiquidacionAeat => "Liquidación o propuesta de liquidación de la AEAT",
            DocKind::CartaPagoAeat => "Carta de pago de la AEAT",
            DocKind::DocumentoAeat => "Documento de la AEAT",
            DocKind::CertificadoOtraAdministracion => {
                "Certificado de otra administración tributaria (no de la AEAT)"
            }
        }
    }

    /// Documento de la Sede de la AEAT (certificado, requerimiento, notificación, recibo de
    /// registro, resolución, liquidación o carta de pago): no es nunca una factura, y sus datos
    /// no se proponen para la ficha de la empresa.
    pub fn de_la_aeat(self) -> bool {
        matches!(
            self,
            DocKind::CertificadoAeat
                | DocKind::CertificadoSituacionCensal
                | DocKind::CertificadoIae
                | DocKind::CertificadoResidencia
                | DocKind::CertificadoTributario
                | DocKind::RequerimientoAeat
                | DocKind::NotificacionAeat
                | DocKind::RegistroAeat
                | DocKind::ResolucionAeat
                | DocKind::LiquidacionAeat
                | DocKind::CartaPagoAeat
                | DocKind::DocumentoAeat
        )
    }

    fn code(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default()
    }

    fn from_code(s: &str) -> Option<Self> {
        serde_json::from_value(serde_json::Value::String(s.to_owned())).ok()
    }
}

/// Datos leídos de un documento (los que aparezcan).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Facts {
    pub nif: Option<String>,
    pub name: Option<String>,
    pub street: Option<String>,
    pub post_code: Option<String>,
    pub town: Option<String>,
    pub constitution_date: Option<Date>,
    /// Capital social en céntimos.
    pub capital: Option<i64>,
    pub cnae: Option<String>,
    pub iae: Option<String>,
    /// NIF-IVA (`ES` + NIF).
    pub vat_number: Option<String>,
    pub administrator: Option<String>,
    pub notary: Option<String>,
    pub protocol: Option<String>,
    /// Inscripción en el Registro Mercantil («Tomo 1234, Folio 56, Hoja V-7890»).
    pub registry: Option<String>,
    /// Fecha del documento (otorgamiento, emisión, alta…).
    pub date: Option<Date>,
}

impl Facts {
    pub fn is_empty(&self) -> bool {
        *self == Facts::default()
    }

    /// Completa los datos que falten con los de `other`.
    pub fn fill_from(&mut self, other: &Facts) {
        macro_rules! fill {
            ($($f:ident),*) => {$(
                if self.$f.is_none() {
                    self.$f.clone_from(&other.$f);
                }
            )*};
        }
        fill!(
            nif,
            name,
            street,
            post_code,
            town,
            constitution_date,
            capital,
            cnae,
            iae,
            vat_number,
            administrator,
            notary,
            protocol,
            registry,
            date
        );
    }
}

/// Minúsculas sin tildes, con los blancos compactados.
pub(crate) fn fold(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut space = false;
    for c in s.chars() {
        let c = match c {
            'á' | 'à' | 'Á' | 'À' => 'a',
            'é' | 'è' | 'É' | 'È' => 'e',
            'í' | 'ì' | 'Í' | 'Ì' => 'i',
            'ó' | 'ò' | 'Ó' | 'Ò' => 'o',
            'ú' | 'ù' | 'ü' | 'Ú' | 'Ù' | 'Ü' => 'u',
            'ñ' | 'Ñ' => 'n',
            'ç' | 'Ç' => 'c',
            // Las ligaduras de algunos PDF («certiﬁca»).
            '\u{fb01}' => {
                out.push_str("fi");
                space = false;
                continue;
            }
            '\u{fb02}' => {
                out.push_str("fl");
                space = false;
                continue;
            }
            c => c.to_lowercase().next().unwrap_or(c),
        };
        if c.is_whitespace() {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

/// Qué documento es, por las frases que lleva. Primero, lo que no es nunca un documento oficial
/// (un informe o un correo, ver [`crate::oficiales::informe_o_correo`]); después, los
/// documentos de la Sede de la AEAT, por sus marcas y su título (antes que las palabras sueltas:
/// un certificado de situación censal habla del ROI y no es el alta en el ROI); y luego, el
/// resto por las frases que llevan. Una escritura necesita las marcas del notario («ante mí» o
/// «doy fe»: un escrito o un informe citan su protocolo) y los documentos del Registro
/// Mercantil, las del Registro (la entrada, el asiento, el diario o los datos de la
/// inscripción).
pub fn classify(text: &str) -> Option<DocKind> {
    use crate::oficiales::{del_registro, notarial};
    if crate::oficiales::informe_o_correo(text) {
        return None;
    }
    if let Some(kind) = crate::oficiales::tipo(text) {
        return Some(kind);
    }
    let t = fold(text);
    let has = |s: &str| t.contains(s);
    let any = |l: &[&str]| l.iter().any(|s| t.contains(s));
    // Lo que expide el Registro Mercantil lo dice arriba (una certificación o nota simple
    // repite la escritura que inscribe, pero no es la escritura).
    let cabeza: String = t.chars().take(600).collect();
    if (cabeza.contains("registro mercantil") || cabeza.contains("registrador"))
        && [
            "certificacion",
            "nota simple",
            "nota informativa",
            "informacion mercantil",
        ]
        .iter()
        .any(|p| cabeza.contains(p))
        && del_registro(&t)
    {
        // La certificación del depósito de las cuentas anuales es el justificante de las
        // cuentas depositadas, no una nota simple ni una certificación de la hoja.
        return Some(
            if cabeza.contains("deposito de cuentas") || cabeza.contains("deposito de las cuentas")
            {
                DocKind::CuentasAnuales
            } else {
                DocKind::RegistroMercantil
            },
        );
    }
    if has("escritura") && notarial(&t) {
        return Some(
            // Una escritura de modificación cita siempre la de constitución («constituida en
            // escritura autorizada por…»): lo que modifica manda.
            if any(&[
                "elevacion a publico de acuerdos",
                "ampliacion de capital",
                "reduccion de capital",
                "modificacion de estatutos",
                "ampliacion del objeto social",
                "modificacion del objeto social",
                "cambio de domicilio",
                "traslado de domicilio",
                "cambio de denominacion",
            ]) {
                DocKind::EscrituraModificacion
            } else if any(&[
                "constitucion de sociedad",
                "constitucion de la sociedad",
                "escritura de constitucion",
                "constituyen una sociedad",
                "constituye una sociedad",
            ]) {
                DocKind::EscrituraConstitucion
            } else if any(&[
                "escritura de poder",
                "otorga poder",
                "apoderamiento",
                "confiere poder",
            ]) {
                DocKind::EscrituraPoderes
            } else if any(&[
                "ampliacion de capital",
                "reduccion de capital",
                "cambio de domicilio",
                "traslado de domicilio",
                "modificacion de estatutos",
                "cambio de denominacion",
                "nombramiento de administrador",
                "cese de administrador",
                "elevacion a publico",
            ]) {
                DocKind::EscrituraModificacion
            } else {
                DocKind::Escritura
            },
        );
    }
    if any(&["estatutos sociales", "estatutos de la sociedad"]) && has("articulo") {
        return Some(DocKind::Estatutos);
    }
    if any(&[
        "registro de operadores intracomunitarios",
        "operadores intracomunitarios",
        "numero de identificacion a efectos del iva",
        "nif-iva",
        "vies",
    ]) {
        return Some(DocKind::AltaRoi);
    }
    if any(&[
        "modelo 036",
        "modelo 037",
        "declaracion censal",
        "censo de empresarios",
    ]) {
        return Some(DocKind::Censal036);
    }
    if any(&["numero de identificacion fiscal"])
        && any(&[
            "tarjeta",
            "asignacion",
            "definitivo",
            "provisional",
            "identificacion fiscal de personas juridicas",
        ])
    {
        return Some(DocKind::TarjetaNif);
    }
    if has("certificado") && has("corriente") {
        if has("seguridad social") {
            return Some(DocKind::CertificadoSeguridadSocial);
        }
        if any(&["obligaciones tributarias", "agencia tributaria"]) {
            // El de la agencia tributaria de una comunidad o un ayuntamiento no es de la AEAT.
            return Some(if crate::oficiales::de_otra_administracion(&t) {
                DocKind::CertificadoOtraAdministracion
            } else {
                DocKind::CertificadoAeat
            });
        }
    }
    if any(&[
        "codigo de cuenta de cotizacion",
        "inscripcion de empresario",
        "regimen especial de trabajadores",
        "resolucion sobre reconocimiento de alta",
        "tesoreria general de la seguridad social",
    ]) {
        return Some(DocKind::AltaSeguridadSocial);
    }
    if any(&["deposito de cuentas", "cuentas anuales"]) && del_registro(&t) {
        return Some(DocKind::CuentasAnuales);
    }
    if has("registro mercantil")
        && any(&[
            "nota simple",
            "informacion mercantil",
            "certificacion",
            "inscrita en el registro",
            "hoja",
        ])
        && del_registro(&t)
    {
        return Some(DocKind::RegistroMercantil);
    }
    if any(&[
        "impuesto sobre actividades economicas",
        "modelo 840",
        "matricula del iae",
    ]) {
        return Some(DocKind::Iae);
    }
    // El acta de la junta general o, en una sociedad unipersonal, la de las decisiones del
    // socio único: ejerce las competencias de la junta y sus decisiones se consignan en acta
    // (art. 15 LSC).
    if (has("acta") && (has("junta general") || has("socio unico")))
        || any(&["decision del socio unico", "decisiones del socio unico"])
    {
        return Some(DocKind::ActaJunta);
    }
    if any(&["contrato de arrendamiento", "contrato de alquiler"]) {
        return Some(DocKind::ContratoAlquiler);
    }
    if has("poliza") && has("seguro") {
        return Some(DocKind::Seguro);
    }
    if any(&[
        "licencia de actividad",
        "licencia de apertura",
        "declaracion responsable de actividad",
    ]) {
        return Some(DocKind::Licencia);
    }
    None
}

/// Palabras de un nombre de fichero (sin la extensión): letras y cifras por separado
/// («Modelo036_firmado.pdf» → «modelo», «036», «firmado»).
fn name_words(filename: &str) -> Vec<String> {
    let stem = filename.rsplit_once('.').map_or(filename, |(s, _)| s);
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut digits = false;
    for c in fold(stem).chars() {
        if c.is_alphanumeric() {
            let d = c.is_ascii_digit();
            if !cur.is_empty() && d != digits {
                words.push(std::mem::take(&mut cur));
            }
            digits = d;
            cur.push(c);
        } else if !cur.is_empty() {
            words.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

/// Tipo por el nombre del fichero (fotos y escaneados, sin texto). Se comparan palabras enteras
/// («036» sí, «Factura 0360» no; «NIF» sí, «Manifiesto» no) y el nombre de una factura (o un
/// ticket, un recibo…) no es nunca un documento de la empresa.
pub fn classify_name(filename: &str) -> Option<DocKind> {
    let words = name_words(filename);
    let joined = format!(" {} ", words.join(" "));
    let has = |w: &str| joined.contains(&format!(" {w} "));
    let any = |l: &[&str]| l.iter().any(|w| has(w));
    let starts = |p: &str| words.iter().any(|w| w.starts_with(p));
    if any(&[
        "factura", "facturas", "fra", "fras", "fac", "invoice", "ticket", "recibo", "abono",
    ]) {
        return None;
    }
    Some(if starts("escritura") || starts("constitucion") {
        DocKind::Escritura
    } else if starts("estatuto") {
        DocKind::Estatutos
    } else if any(&["roi", "vies", "nif iva", "vat"]) {
        DocKind::AltaRoi
    } else if any(&["036", "037", "censal"]) {
        DocKind::Censal036
    } else if any(&["nif", "cif"]) {
        DocKind::TarjetaNif
    } else if starts("poder") {
        DocKind::EscrituraPoderes
    } else if any(&["nota simple", "registro mercantil"]) {
        DocKind::RegistroMercantil
    } else if any(&["iae", "840"]) {
        DocKind::Iae
    } else if starts("alquiler") || starts("arrendamiento") {
        DocKind::ContratoAlquiler
    } else if starts("seguro") || starts("poliza") {
        DocKind::Seguro
    } else if starts("certificado") && (any(&["aeat", "hacienda"]) || starts("tributari")) {
        DocKind::CertificadoAeat
    } else if starts("certificado") && (any(&["seguridad social", "tgss"])) {
        DocKind::CertificadoSeguridadSocial
    } else {
        return None;
    })
}

struct Rx {
    company: Regex,
    label_name: Regex,
    address: Regex,
    town: Regex,
    capital: Regex,
    cnae: Regex,
    iae: Regex,
    vat: Regex,
    admin: Regex,
    notary: Regex,
    notary_before: Regex,
    protocol: Regex,
    tomo: Regex,
    folio: Regex,
    hoja: Regex,
    date_words: Regex,
    date_num: Regex,
    date_letters: Regex,
    nif_label: Regex,
    nif_any: Regex,
}

fn rx() -> &'static Rx {
    static R: OnceLock<Rx> = OnceLock::new();
    R.get_or_init(|| {
        let r = |s: &str| Regex::new(s).expect("regla fija válida");
        Rx {
            // «EJEMPLO SERVICIOS, S.L.», «Ejemplo Sociedad Limitada».
            company: r(r"(?:^|[\s«“:])([A-ZÁÉÍÓÚÑÇ0-9][A-ZÁÉÍÓÚÑÇ0-9&.'\- ]{1,70}?),?\s+(S\.\s?L\.\s?U\.|S\.\s?L\.\s?P\.|S\.\s?L\.|S\.\s?A\.\s?U\.|S\.\s?A\.|SLU|SLP|SL|SA|SOCIEDAD LIMITADA(?: UNIPERSONAL| PROFESIONAL)?|SOCIEDAD AN[ÓO]NIMA(?: UNIPERSONAL)?)(?:[\s,.;:»”)]|$)"),
            label_name: r(r"(?i)(?:denominaci[oó]n(?: social)?|raz[oó]n social|apellidos y nombre o raz[oó]n social)\s*[:.]?\s*([^\n]{3,90})"),
            address: r(r"(?i)domicilio(?: social| fiscal)?\s*(?:en|:|es)?\s*:?\s*((?:c/|calle|avda\.?|avenida|plaza|pza\.?|paseo|carretera|ronda|camino|carrer|avinguda|pla[çc]a|passeig|cam[ií]|traves[ií]a|urbanizaci[oó]n|pol[ií]gono)[^\n;]{2,80}?)(?:,?\s*(?:c\.?\s?p\.?\s*)?(\d{5}))?(?:[\n;]|,\s*(?:con|y|inscrita|provista|NIF|CIF)|\.\s|$)"),
            town: r(r"\b(\d{5})\s*[-–,]?\s*([A-ZÁÉÍÓÚÑÇ][\p{L}'·\- ]{2,40}?)(?:\s*\(|[,.\n]|$)"),
            capital: r(r"(?i)capital(?: social)?\s*(?:de|es de|:|asciende a|se fija en)?\s*:?\s*([\d.]+(?:,\d{1,2})?)\s*(?:euros|€|eur)"),
            cnae: r(r"(?i)\bC\.?N\.?A\.?E\.?(?:[- ]?(?:2009|2025))?\s*[:.]?\s*(?:c[oó]digo\s*)?(\d{2}\.?\d{1,2})\b"),
            iae: r(r"(?i)ep[ií]grafe(?:\s+(?:del\s+)?I\.?A\.?E\.?)?\s*[:.nº°]*\s*(\d{3}(?:\.\d{1,2})?)"),
            vat: r(r"\bES\s?([A-Z0-9]\d{7}[A-Z0-9])\b"),
            // Sólo «administrador…» y el tratamiento sin distinguir mayúsculas: el nombre empieza
            // por mayúscula («de la sociedad a D. JUAN…» no es un nombre).
            admin: r(r"(?i:administrador(?:a)?(?:\s+(?:[uú]nic[oa]|solidari[oa]s?|mancomunad[oa]s?))?)\s*[:,]?\s*(?i:de\s+la\s+(?:sociedad|compa[ñn][ií]a|entidad)\s*,?\s*)?(?i:a\s+)?(?i:(?:don|doña|d\.ª|dña\.|d\.)\s*)?([A-ZÁÉÍÓÚÑ][\p{L}'\-]+(?:\s+(?:de\s+(?:la\s+)?)?[A-ZÁÉÍÓÚÑ][\p{L}'\-]+){1,4})"),
            notary: r(r"(?i)notari[oa](?:\s+(?:del\s+ilustre\s+)?colegio\s+de\s+[\p{L} ]+?)?(?:\s+de\s+[\p{L}]+)?\s*[:,]?\s*(?:don|doña|d\.ª|dña\.|d\.)\s*([A-ZÁÉÍÓÚÑ][\p{L}'\-]+(?:\s+(?:de\s+(?:la\s+)?)?[A-ZÁÉÍÓÚÑ][\p{L}'\-]+){1,4})"),
            // «Ante mí, Don X Y Z, Notario del Ilustre Colegio…»
            notary_before: r(r"(?i)ante\s+m[ií],?\s*(?:don|doña|d\.ª|dña\.|d\.)?\s*([A-ZÁÉÍÓÚÑ][\p{L}'\-]+(?:\s+(?:de\s+(?:la\s+)?)?[A-ZÁÉÍÓÚÑ][\p{L}'\-]+){1,4}),?\s+notari"),
            protocol: r(r"(?i)(?:n[uú]mero de protocolo|protocolo\s*(?:n[uú]m(?:ero)?\.?|n\.?\s?º|nº)?)\s*[:.]?\s*(\d{1,6})"),
            tomo: r(r"(?i)\btomo\s*[:.]?\s*(\d{1,6})"),
            folio: r(r"(?i)\bfolio\s*[:.]?\s*(\d{1,5})"),
            hoja: r(r"(?i)\bhoja\s*(?:n[uú]m(?:ero)?\.?|nº)?\s*[:.]?\s*([A-Z]{1,2}\s?-?\s?\d{1,7})"),
            date_words: r(r"(?i)\b(\d{1,2})\s+de\s+(enero|febrero|marzo|abril|mayo|junio|julio|agosto|septiembre|setiembre|octubre|noviembre|diciembre)\s+de\s+(\d{4})\b"),
            date_num: r(r"\b(\d{1,2})[/.-](\d{1,2})[/.-](\d{4})\b"),
            // Fecha en letras de las escrituras: «a quince de enero de dos mil veinticinco».
            date_letters: r(r"(?i)\ba\s+((?:treinta\s+y\s+uno|[\p{L}]+))\s+de\s+(enero|febrero|marzo|abril|mayo|junio|julio|agosto|septiembre|setiembre|octubre|noviembre|diciembre)\s+de\s+(dos\s+mil(?:\s+[\p{L}]+(?:\s+y\s+[\p{L}]+)?)?)"),
            nif_label: r(r"(?i)(?:N\.?I\.?F\.?|C\.?I\.?F\.?|n[uú]mero de identificaci[oó]n fiscal)\s*(?:n[uú]m\.?|nº)?\s*[:.]?\s*([A-Z0-9][\-. ]?\d{2}[\-. ]?\d{3}[\-. ]?\d{3}[\-. ]?[A-Z0-9]?)"),
            nif_any: r(r"\b([ABCDEFGHJNPQRSUVW]\d{7}[0-9A-J])\b"),
        }
    })
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

/// Número del 1 al 99 escrito en letras («quince», «veinticinco», «treinta y uno»).
fn number_in_words(s: &str) -> Option<u32> {
    const UNITS: [&str; 30] = [
        "cero",
        "uno",
        "dos",
        "tres",
        "cuatro",
        "cinco",
        "seis",
        "siete",
        "ocho",
        "nueve",
        "diez",
        "once",
        "doce",
        "trece",
        "catorce",
        "quince",
        "dieciseis",
        "diecisiete",
        "dieciocho",
        "diecinueve",
        "veinte",
        "veintiuno",
        "veintidos",
        "veintitres",
        "veinticuatro",
        "veinticinco",
        "veintiseis",
        "veintisiete",
        "veintiocho",
        "veintinueve",
    ];
    const TENS: [&str; 7] = [
        "treinta",
        "cuarenta",
        "cincuenta",
        "sesenta",
        "setenta",
        "ochenta",
        "noventa",
    ];
    let s = fold(s);
    let s = s.trim();
    let s = if s == "un" || s == "primero" {
        "uno"
    } else {
        s
    };
    let unit = |w: &str| UNITS[..10].iter().position(|u| *u == w);
    if let Some(n) = UNITS.iter().position(|u| *u == s) {
        return u32::try_from(n).ok();
    }
    let (t, u) = match s.split_once(" y ") {
        Some((t, u)) => (t, unit(if u == "un" { "uno" } else { u })?),
        None => (s, 0),
    };
    let t = TENS.iter().position(|x| *x == t)?;
    u32::try_from(30 + 10 * t + u).ok()
}

fn first_date(text: &str) -> Option<Date> {
    let r = rx();
    let words = r.date_words.captures_iter(text).find_map(|c| {
        let m = fold(&c[2]);
        let month = MONTHS
            .iter()
            .position(|x| *x == m)
            .or_else(|| (m == "setiembre").then_some(8))?;
        Date::from_calendar_date(
            c[3].parse().ok()?,
            time::Month::try_from(u8::try_from(month + 1).ok()?).ok()?,
            c[1].parse().ok()?,
        )
        .ok()
    });
    let letters = || {
        r.date_letters.captures_iter(text).find_map(|c| {
            let m = fold(&c[2]);
            let month = MONTHS
                .iter()
                .position(|x| *x == m)
                .or_else(|| (m == "setiembre").then_some(8))?;
            let year = 2000
                + fold(&c[3]).strip_prefix("dos mil").map_or(Some(0), |r| {
                    if r.trim().is_empty() {
                        Some(0)
                    } else {
                        number_in_words(r)
                    }
                })?;
            Date::from_calendar_date(
                i32::try_from(year).ok()?,
                time::Month::try_from(u8::try_from(month + 1).ok()?).ok()?,
                u8::try_from(number_in_words(&c[1])?).ok()?,
            )
            .ok()
        })
    };
    words.or_else(letters).or_else(|| {
        r.date_num.captures_iter(text).find_map(|c| {
            Date::from_calendar_date(
                c[3].parse().ok()?,
                time::Month::try_from(c[2].parse::<u8>().ok()?).ok()?,
                c[1].parse().ok()?,
            )
            .ok()
        })
    })
}

fn clean_nif(s: &str) -> Option<String> {
    let c: String = s
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_uppercase();
    Nif::parse(&c).ok().map(|n| n.as_str().to_owned())
}

fn amount_cents(s: &str) -> Option<i64> {
    let t = s.replace('.', "");
    let (i, d) = t.split_once(',').unwrap_or((&t, ""));
    let euros: i64 = i.parse().ok()?;
    let cents: i64 = format!("{d:0<2}").get(..2)?.parse().ok()?;
    euros.checked_mul(100)?.checked_add(cents)
}

/// Lee los datos que aparezcan en el texto de un documento de la empresa.
pub fn extract(text: &str, kind: DocKind) -> Facts {
    let r = rx();
    let mut f = Facts::default();
    // NIF: el que va detrás de «NIF»/«CIF» y es de persona jurídica; si no, el primero de
    // persona jurídica que aparezca (en las escrituras también salen los DNI de los socios).
    let labeled: Vec<String> = r
        .nif_label
        .captures_iter(text)
        .filter_map(|c| clean_nif(&c[1]))
        .collect();
    f.nif = labeled
        .iter()
        .find(|n| {
            !n.as_bytes()[0].is_ascii_digit() && !matches!(n.as_bytes()[0], b'X' | b'Y' | b'Z')
        })
        .cloned()
        .or_else(|| r.nif_any.captures_iter(text).find_map(|c| clean_nif(&c[1])))
        .or_else(|| labeled.first().cloned());
    f.vat_number = r
        .vat
        .captures_iter(text)
        .find_map(|c| clean_nif(&c[1]))
        .map(|n| format!("ES{n}"));
    if f.vat_number.is_none() && kind == DocKind::AltaRoi {
        f.vat_number = f.nif.as_ref().map(|n| format!("ES{n}"));
    }
    // Razón social: la de la etiqueta si termina en forma social; si no, la primera
    // «… S.L.» del texto.
    let label = r
        .label_name
        .captures_iter(text)
        .map(|c| c[1].trim().to_owned())
        .find(|v| r.company.is_match(&format!(" {v} ")));
    let fmt = |c: regex::Captures<'_>| {
        let base = c[1].trim().trim_end_matches(',').trim().to_owned();
        let form = c[2].to_uppercase();
        let form = if form.starts_with("SOCIEDAD") {
            form
        } else {
            form.replace(' ', "")
        };
        format!("{base}, {form}")
    };
    let padded = label.as_deref().map(|v| format!(" {v} "));
    f.name = padded
        .as_deref()
        .and_then(|p| r.company.captures(p))
        .or_else(|| r.company.captures(text))
        .map(fmt)
        .filter(|n| n.split_whitespace().count() >= 2);
    if let Some(c) = r.address.captures(text) {
        let mut street = c[1].trim().to_owned();
        f.post_code = c.get(2).map(|m| m.as_str().to_owned());
        // «Calle de la Paz 15, 46003 Valencia»: la calle acaba antes del código postal.
        if let Some(m) = r.town.find(&street) {
            f.post_code
                .get_or_insert_with(|| m.as_str()[..5].to_owned());
            street.truncate(m.start());
        }
        f.street = Some(street.trim().trim_end_matches([',', '.']).trim().to_owned());
    }
    if let Some(c) = r.town.captures(text) {
        if f.post_code.is_none() {
            f.post_code = Some(c[1].to_owned());
        }
        if f.post_code.as_deref() == Some(&c[1]) {
            f.town = Some(c[2].trim().to_owned());
        }
    }
    f.capital = r
        .capital
        .captures(text)
        .and_then(|c| amount_cents(&c[1]))
        .filter(|c| *c > 0);
    f.cnae = r.cnae.captures(text).map(|c| c[1].replace('.', ""));
    f.iae = r.iae.captures(text).map(|c| c[1].to_owned());
    f.administrator = r.admin.captures(text).map(|c| c[1].trim().to_owned());
    f.notary = r
        .notary_before
        .captures(text)
        .or_else(|| r.notary.captures(text))
        .map(|c| c[1].trim().to_owned());
    f.protocol = r.protocol.captures(text).map(|c| c[1].to_owned());
    let reg: Vec<String> = [
        r.tomo.captures(text).map(|c| format!("Tomo {}", &c[1])),
        r.folio.captures(text).map(|c| format!("Folio {}", &c[1])),
        r.hoja
            .captures(text)
            .map(|c| format!("Hoja {}", c[1].replace(' ', ""))),
    ]
    .into_iter()
    .flatten()
    .collect();
    if reg.len() >= 2 {
        f.registry = Some(reg.join(", "));
    }
    f.date = first_date(text);
    if kind == DocKind::EscrituraConstitucion {
        f.constitution_date = f.date;
    }
    f
}

fn iso_date(s: &str) -> Option<Date> {
    Date::parse(
        s.get(..10)?,
        time::macros::format_description!("[year]-[month]-[day]"),
    )
    .ok()
}

/// Documento reconocido (aún sin guardar).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Recognized {
    pub kind: DocKind,
    pub title: String,
    pub facts: Facts,
    /// Leído sólo por el nombre (foto o escaneado).
    pub by_name: bool,
}

/// Datos de un documento de la Sede de la AEAT: sólo el NIF, el nombre y la fecha (las
/// direcciones que lleva son las de la oficina de la AEAT, no las de la empresa). La fecha es la
/// de su firma o su etiqueta ([`crate::oficiales::leer`]): una fecha suelta del texto (un plazo,
/// la de una ley) no es la del documento.
fn facts_aeat(text: &str, kind: DocKind) -> Facts {
    let f = extract(text, kind);
    Facts {
        nif: f.nif,
        name: f.name,
        vat_number: f.vat_number.filter(|_| kind == DocKind::AltaRoi),
        date: crate::oficiales::leer(text).fecha,
        ..Facts::default()
    }
}

/// Reconoce un documento por su texto o, si no tiene, por el nombre del fichero. Un fichero de
/// texto sólo es un documento oficial si lleva sus marcas (el código seguro de verificación, las
/// del notario o las del Registro): las notas y los correos no lo son.
pub fn recognize(filename: &str, bytes: &[u8]) -> Option<Recognized> {
    let txt = filename.to_lowercase().ends_with(".txt");
    let text = if bytes.starts_with(b"%PDF") {
        crate::pdf_text::text(bytes).unwrap_or_default()
    } else if txt {
        String::from_utf8_lossy(bytes).into_owned()
    } else {
        String::new()
    };
    if txt && !crate::oficiales::con_marca(&fold(&text)) {
        return None;
    }
    if text.trim().len() > 40
        && let Some(kind) = classify(&text)
    {
        let facts = if kind.de_la_aeat() {
            facts_aeat(&text, kind)
        } else {
            extract(&text, kind)
        };
        return Some(Recognized {
            kind,
            title: kind.title().to_owned(),
            facts,
            by_name: false,
        });
    }
    let kind = classify_name(filename)?;
    Some(Recognized {
        kind,
        title: kind.title().to_owned(),
        facts: Facts::default(),
        by_name: true,
    })
}

/// Documento guardado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Stored {
    pub id: i64,
    pub kind: DocKind,
    pub title: String,
    pub filename: String,
    pub facts: Facts,
    pub date: Option<Date>,
    pub imported_at: String,
}

fn mime(filename: &str, bytes: &[u8]) -> &'static str {
    let l = filename.to_lowercase();
    if bytes.starts_with(b"%PDF") {
        "application/pdf"
    } else if l.ends_with(".png") {
        "image/png"
    } else if l.ends_with(".jpg") || l.ends_with(".jpeg") {
        "image/jpeg"
    } else if l.ends_with(".heic") {
        "image/heic"
    } else {
        "application/octet-stream"
    }
}

/// Guarda el documento (si ya estaba, devuelve el que había).
pub fn store(
    v: &Vault,
    filename: &str,
    bytes: &[u8],
    r: &Recognized,
    now: OffsetDateTime,
) -> Result<i64> {
    let sha = hex::encode(Sha256::digest(bytes));
    let conn = v.connection();
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM company_document WHERE file_sha256 = ?1",
            [&sha],
            |row| row.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO document(uuid, kind, filename, mime, sha256, size, content, created_at)
         VALUES (?1, 'empresa', ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            format!("empresa-{}", &sha[..32]),
            filename,
            mime(filename, bytes),
            sha,
            i64::try_from(bytes.len()).unwrap_or(0),
            bytes,
            now_str(now)
        ],
    )?;
    let doc = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO company_document(kind, title, filename, file_sha256, document_id, facts, doc_date, imported_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            r.kind.code(),
            r.title,
            filename,
            sha,
            doc,
            serde_json::to_string(&r.facts).map_err(|e| AppError::Other(e.to_string()))?,
            r.facts.date.map(|d| d.to_string()),
            now_str(now)
        ],
    )?;
    let id = tx.last_insert_rowid();
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "company_document.import",
        &r.title,
        &serde_json::json!({ "kind": r.kind.code() }),
    )?;
    tx.commit()?;
    Ok(id)
}

pub fn list(v: &Vault) -> Result<Vec<Stored>> {
    let mut stmt = v.connection().prepare(
        "SELECT id, kind, title, filename, facts, doc_date, imported_at FROM company_document
         ORDER BY COALESCE(doc_date, imported_at) DESC, id DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, String>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, kind, title, filename, facts, date, imported_at) = row?;
        let Some(kind) = DocKind::from_code(&kind) else {
            continue;
        };
        out.push(Stored {
            id,
            kind,
            title,
            filename,
            facts: serde_json::from_str(&facts).unwrap_or_default(),
            date: date.and_then(|d| iso_date(&d)),
            imported_at,
        });
    }
    Ok(out)
}

/// (nombre, MIME, bytes) de un documento, para abrirlo.
pub fn content(v: &Vault, id: i64) -> Result<(String, String, Vec<u8>)> {
    v.connection()
        .query_row(
            "SELECT d.filename, d.mime, d.content FROM company_document c
             JOIN document d ON d.id = c.document_id WHERE c.id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)
}

/// Carpeta temporal privada (sólo el usuario: 0700) para abrir documentos con la app del sistema.
fn temp_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("taxpain-documentos")
}

/// Borra las copias temporales de documentos más antiguas que `older_than` (todas con cero; se
/// llama al arrancar la app y cada vez que se abre uno).
pub fn clean_temp_copies(older_than: std::time::Duration) {
    clean_dir(&temp_dir(), older_than);
}

/// Borra los ficheros de `dir` más antiguos que `older_than` (todos con cero). La usan también
/// las carpetas temporales privadas de la app (el `.eml` de un correo…).
pub fn clean_dir(dir: &std::path::Path, older_than: std::time::Duration) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let now = std::time::SystemTime::now();
    for e in rd.flatten() {
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_none_or(|age| age >= older_than);
        if old {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Copia descifrada de un documento para abrirlo con la app del sistema (Vista Previa): en una
/// carpeta temporal privada, con un nombre al azar delante (no se pisa ni se adivina) y que se
/// borra pasada una hora o al volver a arrancar. Devuelve la ruta.
pub fn temp_copy(filename: &str, bytes: &[u8]) -> Result<std::path::PathBuf> {
    let io = |e: std::io::Error| AppError::Other(format!("copia temporal: {e}"));
    clean_temp_copies(std::time::Duration::from_secs(3600));
    let dir = temp_dir();
    std::fs::create_dir_all(&dir).map_err(io)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(io)?;
    }
    let mut r = [0u8; 8];
    getrandom::fill(&mut r).map_err(|e| AppError::Other(e.to_string()))?;
    let safe: String = filename
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let path = dir.join(format!("{}-{safe}", hex::encode(r)));
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    use std::io::Write;
    opts.open(&path)
        .and_then(|mut f| f.write_all(bytes))
        .map_err(io)?;
    Ok(path)
}

/// Texto del documento (para completarlo con el asistente).
pub fn text(v: &Vault, id: i64) -> Result<String> {
    let (_, _, bytes) = content(v, id)?;
    let t = if bytes.starts_with(b"%PDF") {
        crate::pdf_text::text(&bytes).unwrap_or_default()
    } else {
        String::new()
    };
    let t: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.len() < 40 {
        return Err(AppError::Other(
            "el documento no tiene texto (foto o escaneado): sus datos se teclean a mano".into(),
        ));
    }
    Ok(t.chars().take(15_000).collect())
}

/// Completa los datos leídos de un documento (por ejemplo, con los del asistente): sólo los
/// que faltaban.
pub fn merge_facts(v: &Vault, id: i64, more: &Facts) -> Result<Facts> {
    let mut facts = list(v)?
        .into_iter()
        .find(|d| d.id == id)
        .ok_or(AppError::NotFound)?
        .facts;
    facts.fill_from(more);
    v.connection().execute(
        "UPDATE company_document SET facts = ?1, doc_date = COALESCE(doc_date, ?2) WHERE id = ?3",
        params![
            serde_json::to_string(&facts).map_err(|e| AppError::Other(e.to_string()))?,
            facts.date.map(|d| d.to_string()),
            id
        ],
    )?;
    Ok(facts)
}

pub fn delete(v: &Vault, id: i64) -> Result<()> {
    let n = v
        .connection()
        .execute("DELETE FROM company_document WHERE id = ?1", [id])?;
    if n == 0 {
        return Err(AppError::NotFound);
    }
    Ok(())
}

/// Instrucciones para que el asistente lea un documento de la empresa.
pub const AI_PROMPT: &str = "\
Lee este documento de MI EMPRESA (escritura, tarjeta NIF, alta censal, certificado…) y \
responde SOLO con un objeto JSON, sin texto alrededor, con estas claves (null si no aparece): \
\"nif\" (de la sociedad, tal cual aparece aunque sea un marcador como [NIF_1]), \"name\" (razón \
social con su forma, p. ej. «X, S.L.»), \"street\", \"post_code\", \"town\", \
\"constitution_date\" (AAAA-MM-DD), \"capital\" (euros con punto decimal), \"cnae\", \"iae\" \
(epígrafe), \"vat_number\", \"administrator\", \"notary\", \"protocol\", \"registry\" (tomo, \
folio, hoja), \"date\" (fecha del documento, AAAA-MM-DD). No inventes datos.\n\nDocumento:\n";

/// Lee la respuesta del asistente.
pub fn parse_ai(reply: &str) -> Facts {
    let Some(j) = reply
        .find('{')
        .zip(reply.rfind('}'))
        .and_then(|(a, b)| reply.get(a..=b))
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
    else {
        return Facts::default();
    };
    let s = |k: &str| {
        j.get(k)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty() && *v != "null")
            .map(str::to_owned)
    };
    let date = |k: &str| s(k).and_then(|d| iso_date(&d));
    Facts {
        nif: s("nif").and_then(|n| clean_nif(&n)),
        name: s("name"),
        street: s("street"),
        post_code: s("post_code").filter(|p| p.len() == 5 && p.bytes().all(|b| b.is_ascii_digit())),
        town: s("town"),
        constitution_date: date("constitution_date"),
        capital: j
            .get("capital")
            .and_then(|v| {
                v.as_f64()
                    .map(|x| format!("{x:.2}"))
                    .or_else(|| v.as_str().map(str::to_owned))
            })
            .and_then(|t| amount_cents(&t.replace('.', ","))),
        cnae: s("cnae").map(|c| c.replace('.', "")),
        iae: s("iae"),
        vat_number: s("vat_number"),
        administrator: s("administrator"),
        notary: s("notary"),
        protocol: s("protocol"),
        registry: s("registry"),
        date: date("date"),
    }
}

// ---------- Propuestas para la ficha de la empresa ----------

/// Un dato de la empresa que los documentos dicen distinto (o que falta).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Suggestion {
    /// `nombre`, `nif`, `domicilio`, `cnae`, `intracomunitarias`.
    pub field: String,
    pub label: String,
    pub current: Option<String>,
    pub proposed: String,
    /// Documento del que sale.
    pub source: String,
}

/// Datos de todos los documentos, del más fiable y reciente al menos.
fn merged_facts(v: &Vault) -> Result<(Facts, BTreeMap<&'static str, String>)> {
    // Los documentos de la Sede de la AEAT no proponen datos de la empresa (el certificado de
    // estar al corriente, como antes, sí).
    let docs: Vec<Stored> = list(v)?
        .into_iter()
        .filter(|d| d.kind == DocKind::CertificadoAeat || !d.kind.de_la_aeat())
        .collect();
    let rank = |k: DocKind| match k {
        DocKind::TarjetaNif | DocKind::Censal036 => 0,
        DocKind::EscrituraModificacion | DocKind::RegistroMercantil => 1,
        DocKind::EscrituraConstitucion | DocKind::Estatutos => 2,
        DocKind::AltaRoi => 3,
        _ => 4,
    };
    let mut docs: Vec<&Stored> = docs.iter().collect();
    docs.sort_by_key(|d| (rank(d.kind), std::cmp::Reverse(d.date)));
    let mut f = Facts::default();
    let mut src: BTreeMap<&'static str, String> = BTreeMap::new();
    for d in docs {
        let before = f.clone();
        f.fill_from(&d.facts);
        let mut note = |k: &'static str, changed: bool| {
            if changed {
                src.entry(k).or_insert_with(|| d.title.clone());
            }
        };
        note("nif", before.nif.is_none() && f.nif.is_some());
        note("name", before.name.is_none() && f.name.is_some());
        note("street", before.street.is_none() && f.street.is_some());
        note("cnae", before.cnae.is_none() && f.cnae.is_some());
        note(
            "vat",
            before.vat_number.is_none() && f.vat_number.is_some() || d.kind == DocKind::AltaRoi,
        );
    }
    if list(v)?.iter().any(|d| d.kind == DocKind::AltaRoi) && f.vat_number.is_none() {
        f.vat_number = f.nif.as_ref().map(|n| format!("ES{n}"));
    }
    Ok((f, src))
}

const PROFILE_KEY: &str = "taxpayer_profile";

/// Lo que los documentos proponen cambiar en la empresa.
pub fn suggestions(v: &Vault) -> Result<Vec<Suggestion>> {
    let (f, src) = merged_facts(v)?;
    let info = crate::companies::info(v)?;
    let inv = crate::invoicing::settings(v)?;
    let ms = crate::models::settings(v)?;
    let mut out = Vec::new();
    let mut push = |field: &str,
                    label: &str,
                    current: Option<String>,
                    proposed: Option<&String>,
                    key: &str| {
        if let Some(p) = proposed
            && current.as_deref().map(fold) != Some(fold(p))
        {
            out.push(Suggestion {
                field: field.into(),
                label: label.into(),
                current,
                proposed: p.clone(),
                source: src.get(key).cloned().unwrap_or_default(),
            });
        }
    };
    push(
        "nombre",
        "Razón social",
        Some(info.name.clone()).filter(|n| !n.is_empty()),
        f.name.as_ref(),
        "name",
    );
    push("nif", "NIF", info.nif.clone(), f.nif.as_ref(), "nif");
    let address = f.street.as_ref().map(|s| {
        format!(
            "{s}{}{}",
            f.post_code
                .as_ref()
                .map(|p| format!(", {p}"))
                .unwrap_or_default(),
            f.town.as_ref().map(|t| format!(" {t}")).unwrap_or_default()
        )
    });
    let current_address = inv.as_ref().and_then(|s| {
        s.issuer
            .address
            .as_ref()
            .map(|a| format!("{}, {} {}", a.street, a.post_code, a.town))
    });
    if inv.is_some() {
        push(
            "domicilio",
            "Domicilio en las facturas",
            current_address,
            address.as_ref(),
            "street",
        );
    }
    push(
        "cnae",
        "CNAE de la actividad",
        Some(ms.cnae.clone()).filter(|c| !c.is_empty()),
        f.cnae.as_ref(),
        "cnae",
    );
    if let Some(vat) = &f.vat_number {
        let profile: Option<taxpain_core::calendar::rules::TaxpayerProfile> =
            v.setting(PROFILE_KEY)?;
        if profile
            .as_ref()
            .is_some_and(|p| p.intracomunitarias.is_none())
        {
            out.push(Suggestion {
                field: "intracomunitarias".into(),
                label: "Operaciones intracomunitarias (modelo 349 en el calendario)".into(),
                current: Some("No".into()),
                proposed: format!("Sí (dada de alta en el ROI con {vat})"),
                source: src.get("vat").cloned().unwrap_or_default(),
            });
        }
    }
    Ok(out)
}

/// Aplica una propuesta (con la conformidad del usuario).
pub fn apply_suggestion(v: &Vault, field: &str, now: OffsetDateTime) -> Result<()> {
    let (f, _) = merged_facts(v)?;
    let missing = || AppError::Other("los documentos ya no proponen ese dato".into());
    match field {
        "nombre" | "nif" => {
            let mut info = crate::companies::info(v)?;
            if field == "nombre" {
                info.name = f.name.clone().ok_or_else(missing)?;
            } else {
                info.nif = Some(f.nif.clone().ok_or_else(missing)?);
            }
            crate::companies::set_info(v, &info, now)?;
            // La facturación usa los mismos datos del emisor.
            if let Some(mut s) = crate::invoicing::settings(v)? {
                if field == "nombre" {
                    s.issuer.name = info.name.clone();
                } else if let Some(n) = &info.nif {
                    s.issuer.id = taxpain_core::invoice::PartyId::Nif(n.clone());
                }
                crate::invoicing::save_settings(v, &s, now)?;
            }
        }
        "domicilio" => {
            let mut s = crate::invoicing::settings(v)?
                .ok_or_else(|| AppError::Other("configura antes la facturación".into()))?;
            let street = f.street.clone().ok_or_else(missing)?;
            let mut a = s
                .issuer
                .address
                .clone()
                .unwrap_or(taxpain_core::invoice::Address {
                    street: String::new(),
                    post_code: String::new(),
                    town: String::new(),
                    province: String::new(),
                    country: "ES".into(),
                });
            a.street = street;
            if let Some(p) = &f.post_code {
                a.post_code.clone_from(p);
            }
            if let Some(t) = &f.town {
                a.town.clone_from(t);
            }
            s.issuer.address = Some(a);
            crate::invoicing::save_settings(v, &s, now)?;
        }
        "cnae" => {
            let mut ms = crate::models::settings(v)?;
            ms.cnae = f.cnae.clone().ok_or_else(missing)?;
            crate::models::save_settings(v, &ms, now)?;
        }
        "intracomunitarias" => {
            let mut p: taxpain_core::calendar::rules::TaxpayerProfile = v
                .setting(PROFILE_KEY)?
                .ok_or_else(|| AppError::Other("configura antes el perfil fiscal".into()))?;
            p.intracomunitarias = Some(taxpain_core::calendar::rules::Periodicity::Trimestral);
            v.set_setting(PROFILE_KEY, &p)?;
        }
        other => return Err(AppError::Other(format!("dato desconocido: {other}"))),
    }
    audit::append(
        v.connection(),
        &now_str(now),
        Actor::User,
        "company_document.apply",
        field,
        &serde_json::json!({}),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limpia_lo_que_se_dejo_a_otras_apps() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a1b2-A-1.eml");
        std::fs::write(&file, b"x").unwrap();
        clean_dir(dir.path(), std::time::Duration::from_secs(3600));
        assert!(file.exists(), "reciente: se queda");
        clean_dir(dir.path(), std::time::Duration::ZERO);
        assert!(!file.exists(), "con cero, todo");
        // Una carpeta que no existe no es un error.
        clean_dir(&dir.path().join("no-existe"), std::time::Duration::ZERO);
    }

    const ESCRITURA: &str = "NÚMERO MIL DOSCIENTOS TREINTA Y CUATRO. ESCRITURA DE CONSTITUCIÓN DE \
        SOCIEDAD DE RESPONSABILIDAD LIMITADA. En Valencia, a 12 de marzo de 2019. Ante mí, Don \
        Ramón Pérez Notario, Notario del Ilustre Colegio de Valencia. COMPARECE: Don Antonio García \
        López, con DNI 12345678Z. OTORGA: Que constituye una sociedad de responsabilidad limitada \
        denominada EJEMPLO SERVICIOS, S.L., con domicilio social en Calle de la Paz 15, 46003 \
        Valencia. Capital social de 3.000,00 euros. Objeto: CNAE 6201. Se nombra Administrador \
        Único a Don Antonio García López. Número de protocolo 1234. Inscrita en el Registro \
        Mercantil de Madrid, Tomo 1.234, Folio 56, Hoja M-12345. N.I.F.: B12345674.";

    #[test]
    fn escritura_de_constitucion() {
        assert_eq!(classify(ESCRITURA), Some(DocKind::EscrituraConstitucion));
        let f = extract(ESCRITURA, DocKind::EscrituraConstitucion);
        assert_eq!(f.nif.as_deref(), Some("B12345674"));
        assert_eq!(f.name.as_deref(), Some("EJEMPLO SERVICIOS, S.L."));
        assert_eq!(f.street.as_deref(), Some("Calle de la Paz 15"));
        assert_eq!(f.post_code.as_deref(), Some("46003"));
        assert_eq!(f.town.as_deref(), Some("Valencia"));
        assert_eq!(f.capital, Some(300_000));
        assert_eq!(f.cnae.as_deref(), Some("6201"));
        assert_eq!(f.administrator.as_deref(), Some("Antonio García López"));
        assert_eq!(f.notary.as_deref(), Some("Ramón Pérez Notario"));
        assert_eq!(f.protocol.as_deref(), Some("1234"));
        assert_eq!(
            f.constitution_date,
            Some(time::macros::date!(2019 - 03 - 12))
        );
        assert!(
            f.registry
                .as_deref()
                .is_some_and(|r| r.contains("Hoja M-12345")),
            "{f:?}"
        );
    }

    #[test]
    fn escritura_con_fecha_en_letras_y_administrador_de_la_sociedad() {
        let t = "ESCRITURA DE CONSTITUCIÓN DE SOCIEDAD DE RESPONSABILIDAD LIMITADA. NÚMERO MIL \
                 DOSCIENTOS. En Barcelona, a quince de enero de dos mil veinticinco. Ante mí, \
                 notario del Ilustre Colegio de Cataluña, comparece: D. JUAN ESPAÑOL PRUEBA, con \
                 NIF 12345678Z. Constituye la sociedad EMPRESA DEMO SL, con CIF B12345674. Se \
                 nombra ADMINISTRADOR ÚNICO de la sociedad a D. JUAN ESPAÑOL PRUEBA, con NIF \
                 12345678Z, por plazo indefinido.";
        let f = extract(t, DocKind::EscrituraConstitucion);
        assert_eq!(f.administrator.as_deref(), Some("JUAN ESPAÑOL PRUEBA"));
        assert_eq!(
            f.constitution_date,
            Some(time::macros::date!(2025 - 01 - 15))
        );
        assert_eq!(number_in_words("treinta y uno"), Some(31));
        assert_eq!(number_in_words("veintidós"), Some(22));
        assert_eq!(number_in_words("noventa y nueve"), Some(99));
        assert_eq!(number_in_words("mil"), None);
    }

    #[test]
    fn la_copia_para_abrir_un_documento_es_privada_y_no_se_pisa() {
        let a = temp_copy("escritura.pdf", b"uno").unwrap_or_else(|e| panic!("{e}"));
        let b = temp_copy("escritura.pdf", b"dos").unwrap_or_else(|e| panic!("{e}"));
        assert_ne!(a, b);
        assert_eq!(std::fs::read(&a).ok().as_deref(), Some(&b"uno"[..]));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &std::path::Path| {
                std::fs::metadata(p)
                    .map(|m| m.permissions().mode() & 0o777)
                    .ok()
            };
            assert_eq!(mode(&temp_dir()), Some(0o700));
            assert_eq!(mode(&a), Some(0o600));
        }
        clean_temp_copies(std::time::Duration::ZERO);
        assert!(!a.exists() && !b.exists());
    }

    #[test]
    fn tarjeta_nif_roi_y_otros() {
        let nif = "AGENCIA TRIBUTARIA. Tarjeta de identificación fiscal. NÚMERO DE IDENTIFICACIÓN \
                   FISCAL: B12345674. Denominación: EJEMPLO SERVICIOS SL. Domicilio fiscal: \
                   Calle de la Paz 15, 46003 VALENCIA. Fecha de asignación definitiva 02/04/2019";
        assert_eq!(classify(nif), Some(DocKind::TarjetaNif));
        let f = extract(nif, DocKind::TarjetaNif);
        assert_eq!(f.nif.as_deref(), Some("B12345674"));
        assert_eq!(f.name.as_deref(), Some("EJEMPLO SERVICIOS, SL"));

        let roi = "Resolución de alta en el Registro de Operadores Intracomunitarios. Número de \
                   identificación a efectos del IVA: ESB12345674";
        assert_eq!(classify(roi), Some(DocKind::AltaRoi));
        assert_eq!(
            extract(roi, DocKind::AltaRoi).vat_number.as_deref(),
            Some("ESB12345674")
        );
        assert_eq!(
            classify(
                "Certificado de estar al corriente de las obligaciones tributarias. Agencia Tributaria"
            ),
            Some(DocKind::CertificadoAeat)
        );
        assert_eq!(
            classify("MODELO 036 Declaración censal de alta, modificación y baja"),
            Some(DocKind::Censal036)
        );
        assert_eq!(classify("Factura 2025-001 Total 121,00 €"), None);
        // En una sociedad unipersonal, el acta es de las decisiones del socio único.
        assert_eq!(
            classify(
                "ACTA DE LA DECISIÓN DEL SOCIO ÚNICO DE EMPRESA DEMO SL. El socio único aprueba \
                 las cuentas anuales del ejercicio 2025"
            ),
            Some(DocKind::ActaJunta)
        );
        assert_eq!(
            classify(
                "Acta de la junta general ordinaria: aprobación de las cuentas y aplicación del \
                 resultado"
            ),
            Some(DocKind::ActaJunta)
        );
        assert_eq!(
            classify_name("Escrituras constitución.pdf"),
            Some(DocKind::Escritura)
        );
        assert_eq!(
            classify_name("foto tarjeta NIF.jpg"),
            Some(DocKind::TarjetaNif)
        );
        assert_eq!(classify_name("IMG_2031.jpg"), None);
        // Palabras enteras: ni «0360» es un 036 ni «Manifiesto» un NIF; una factura no es un
        // documento de la empresa aunque su nombre diga «VAT» o «NIF».
        assert_eq!(classify_name("Factura 0360.pdf"), None);
        assert_eq!(classify_name("IMG_0360.jpg"), None);
        assert_eq!(classify_name("Manifiesto.pdf"), None);
        assert_eq!(classify_name("Provisional.pdf"), None);
        assert_eq!(classify_name("factura VAT Google.pdf"), None);
        assert_eq!(
            classify_name("Modelo036_firmado.pdf"),
            Some(DocKind::Censal036)
        );
        assert_eq!(classify_name("alta-ROI.pdf"), Some(DocKind::AltaRoi));
        assert_eq!(classify_name("NIF-IVA.pdf"), Some(DocKind::AltaRoi));
    }

    #[test]
    fn respuesta_del_asistente() {
        let f = parse_ai(
            r#"{"nif":"B-12345674","name":"Ejemplo, S.L.","capital":3000.5,"constitution_date":"2019-03-12","post_code":"4600"}"#,
        );
        assert_eq!(f.nif.as_deref(), Some("B12345674"));
        assert_eq!(f.capital, Some(300_050));
        assert_eq!(f.post_code, None);
        assert_eq!(
            f.constitution_date,
            Some(time::macros::date!(2019 - 03 - 12))
        );
    }

    #[test]
    fn lo_del_registro_y_las_actas_no_son_escrituras_ni_facturas() {
        // Una nota informativa y una certificación del Registro citan escrituras y notarios.
        let nota = "Información Mercantil interactiva de los Registros Mercantiles de España\n\
            Registro Mercantil de Ciudad Demo\nExpedida el día: 02/02/2024\nDatos registrales: \
            Hoja V-1234 Tomo 567 Folio 89\nEscritura de constitución autorizada por el notario \
            Don Pedro Demo, protocolo 123.";
        assert_eq!(classify(nota), Some(DocKind::RegistroMercantil));
        let certificacion = "Certificación Registral expedida por:\nDON PEDRO DEMO\nRegistrador \
            de MERCANTIL DE CIUDAD DEMO\nCertificación de vigencia y cargos\nEL REGISTRADOR \
            MERCANTIL QUE SUSCRIBE, con referencia a la instancia presentada bajo el asiento 12 \
            del Diario 3 en este Registro Mercantil de Ciudad Demo, CERTIFICA: Que la escritura \
            otorgada ante el notario Don Juan Demo, protocolo 45, consta inscrita.";
        assert_eq!(classify(certificacion), Some(DocKind::RegistroMercantil));
        // Un escrito que cita la escritura (con su protocolo) no es una escritura.
        let escrito = "A LA AGENCIA ESTATAL DE ADMINISTRACIÓN TRIBUTARIA\nDon Pedro Demo, \
            representante de EMPRESA DEMO SL, comparece y EXPONE\nQue por escritura de 3 de \
            febrero de 2023 ante el notario Don Juan Demo, con número de protocolo 1.234, se \
            amplió el capital.\nSOLICITO que se tenga por comunicado.";
        assert_eq!(classify(escrito), None);
        let sin_marcas = "Resumen: la escritura de ampliación de capital, protocolo 1.234 del \
            notario Don Juan Demo, se firmó en febrero de 2023.";
        assert_eq!(classify(sin_marcas), None);
        // El acta de las decisiones del socio único es un acta, no una factura.
        let acta = "ACTA DE LA DECISION DEL SOCIO UNICO DE LA SOCIEDAD EMPRESA DEMO SLU\nEn \
            Ciudad Demo, a 25 de junio de 2025, el socio único adopta las siguientes DECISIONES: \
            aprobar las cuentas anuales del ejercicio 2024, con un resultado de 1.234,56 euros.";
        assert_eq!(classify(acta), Some(DocKind::ActaJunta));
    }

    #[test]
    fn la_certificacion_del_deposito_son_las_cuentas_depositadas() {
        let deposito = "REGISTRO MERCANTIL DE CIUDAD DEMO\nCERTIFICACIÓN DE DEPÓSITO DE CUENTAS\n\
            Entidad: EMPRESA DEMO SL\nCierre ejercicio: 31/12/2025\nNúmero entrada: 2/2026/1234\n\
            La Registradora Mercantil que suscribe, previo examen y calificación del depósito de \
            cuentas, CERTIFICA que ha procedido a su depósito bajo el número de archivo 3/2026/99.";
        assert_eq!(classify(deposito), Some(DocKind::CuentasAnuales));
    }

    #[test]
    fn la_escritura_de_modificacion_cita_la_de_constitucion() {
        let e = "JUSTIFICANTE DE EXPEDICIÓN DE COPIA\nEl presente justificante acredita que el \
            Notario ha expedido copia autorizada electrónica.\nNÚMERO MIL DOSCIENTOS\nESCRITURA DE \
            ELEVACIÓN A PÚBLICO DE ACUERDOS SOCIALES DE LA SOCIEDAD EMPRESA DEMO SL.\nAnte mí, \
            Don Juan Demo, Notario.\nCOMPARECE Don Pedro Demo, como administrador único de \
            EMPRESA DEMO SL, constituida en escritura de constitución autorizada por el Notario \
            Don Luis Demo el 3 de marzo de 2019. Acuerdos: ampliación del objeto social y \
            ampliación de capital. Doy fe.";
        assert_eq!(classify(e), Some(DocKind::EscrituraModificacion));
        let c = "ESCRITURA DE CONSTITUCIÓN DE SOCIEDAD DE RESPONSABILIDAD LIMITADA\nAnte mí, \
            Don Juan Demo, Notario. Constituyen una sociedad limitada y nombran administrador \
            único. Doy fe.";
        assert_eq!(classify(c), Some(DocKind::EscrituraConstitucion));
    }
}
