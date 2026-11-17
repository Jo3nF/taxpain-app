//! Documentos oficiales de la Sede de la AEAT: certificados (estar al corriente, situación
//! censal, IAE, residencia fiscal), requerimientos, notificaciones y comunicaciones (con los
//! certificados de puesta a disposición y de acceso de la DEHú), recibos de presentación en el
//! registro, resoluciones, liquidaciones y cartas de pago.
//!
//! Se reconocen por sus marcas: el pie con el código seguro de verificación de la AEAT (o el
//! certificado de la Dirección Electrónica Habilitada única) y el título que llevan arriba. No
//! son nunca facturas. Y lo contrario: un informe (la revisión que exporta TaxPain, una revisión
//! fiscal hecha con el asistente) o un correo hablan de escrituras, depósitos y requerimientos,
//! pero no son documentos oficiales.
//!
//! [`leer`] saca de su texto lo que usa la revisión de la carpeta: fechas, referencias, plazos,
//! ejercicios, obligaciones del censo… `TODO(VERIFY)`: comprobado con documentos reales de la
//! Sede; otros formatos (cartas de pago, liquidaciones provisionales) sólo con
//! textos de prueba.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;
use taxpain_core::money::Money;
use time::{Date, Month};

use crate::documentos::{DocKind, fold};

/// ¿Lleva el pie de un documento de la AEAT? Su autenticidad con el código seguro de
/// verificación («Autenticidad verificable mediante Código Seguro Verificación…», «La
/// autenticidad de este documento puede ser comprobada mediante el Código Seguro de
/// Verificación…») y la Agencia Estatal (su nombre o la dirección de su Sede). Que un texto
/// cite un código seguro de verificación y a la «Agencia Tributaria» no basta: una factura de
/// la gestoría lo hace al cobrar la presentación de un modelo, y las agencias tributarias de las
/// comunidades y los ayuntamientos también firman con un código (texto plegado con [`fold`]).
pub(crate) fn de_la_aeat(t: &str) -> bool {
    let estatal = t.contains("agenciatributaria.gob.es")
        || t.contains("agenciatributaria.es")
        || t.contains("agencia estatal de administracion tributaria");
    if !estatal || de_otra_administracion(t) {
        return false;
    }
    [
        "codigo seguro de verificacion",
        "codigo seguro verificacion",
    ]
    .iter()
    .flat_map(|p| t.match_indices(p))
    .any(|(i, _)| {
        let antes = &t[t.floor_char_boundary(i.saturating_sub(250))..i];
        antes.contains("autenticidad")
            || antes.ends_with("verificable mediante ")
            || antes.ends_with("comprobada mediante el ")
    })
}

/// Agencias tributarias de las comunidades autónomas, las haciendas forales y las de los
/// ayuntamientos (texto plegado con [`fold`]).
const OTRAS_ADMINISTRACIONES: [&str; 20] = [
    "agencia tributaria valenciana",
    "agencia tributaria de catalunya",
    "agencia tributaria de cataluna",
    "agencia tributaria canaria",
    "agencia tributaria de las illes balears",
    "agencia tributaria de les illes balears",
    "agencia tributaria de andalucia",
    "agencia tributaria de galicia",
    "axencia tributaria de galicia",
    "agencia tributaria de la region de murcia",
    "agencia tributaria de extremadura",
    "agencia tributaria del principado de asturias",
    "ente tributario del principado de asturias",
    "agencia tributaria de cantabria",
    "agencia tributaria de castilla",
    "agencia tributaria de madrid",
    "agencia tributaria madrid",
    "hacienda tributaria de navarra",
    "hacienda foral",
    "diputacion foral",
];

/// ¿Es de la agencia tributaria de una comunidad autónoma, de una hacienda foral o de un
/// ayuntamiento? Lo dice arriba, en su cabecera, sin nombrar a la Agencia Estatal (texto
/// plegado con [`fold`]).
pub(crate) fn de_otra_administracion(t: &str) -> bool {
    let cabeza = &t[..t.floor_char_boundary(800)];
    OTRAS_ADMINISTRACIONES.iter().any(|a| cabeza.contains(a))
        && !cabeza.contains("agencia estatal de administracion tributaria")
}

/// Certificado de la Dirección Electrónica Habilitada única (DEHú) de una notificación de la
/// AEAT: no lleva el código de la AEAT, sino el de la DEHú.
fn de_la_dehu(t: &str) -> bool {
    t.contains("direccion electronica habilitada unica")
        && t.contains("certifica")
        && t.contains("agencia estatal de administracion tributaria")
}

/// Marcas notariales: «ante mí» o «doy fe». Un número de protocolo no basta: un escrito o un
/// informe citan el de una escritura.
pub(crate) fn notarial(t: &str) -> bool {
    t.contains("ante mi") || t.contains("doy fe")
}

/// Estructura de un documento del Registro Mercantil: su nombre con la entrada, el asiento, el
/// diario o los datos de la inscripción (texto plegado con [`fold`]).
pub(crate) fn del_registro(t: &str) -> bool {
    (t.contains("registro mercantil de") || t.contains("registrador"))
        && [
            "asiento", "diario", "entrada", "inscrit", "tomo", "folio", "hoja",
        ]
        .iter()
        .any(|p| t.contains(p))
}

/// ¿Tiene marcas de documento oficial? (de la AEAT, de la DEHú, de un notario o del Registro
/// Mercantil). Texto plegado con [`fold`].
pub(crate) fn con_marca(t: &str) -> bool {
    de_la_aeat(t) || de_la_dehu(t) || notarial(t) || del_registro(t)
}

/// ¿Es un informe, un correo o un escrito? La revisión que exporta TaxPain y los informes que
/// se hacen con el asistente lo dicen (y no son de un notario, del Registro ni de la AEAT); los
/// correos empiezan por el asunto o por un saludo; un escrito a la Administración tiene su
/// «expone» y quien «comparece» o «solicita». Hablan de escrituras, depósitos y requerimientos,
/// pero no son documentos oficiales.
pub(crate) fn informe_o_correo(texto: &str) -> bool {
    let t = fold(texto);
    // El aviso del informe que exporta TaxPain (una factura del dominio de TaxPain no lo lleva).
    if t.contains("hecha por taxpain") {
        return true;
    }
    let todas: Vec<String> = texto
        .lines()
        .map(|l| fold(l).trim().to_owned())
        .filter(|l| !l.is_empty())
        .take(60)
        .collect();
    let expone = todas.iter().any(|l| {
        l.trim_end_matches([':', '.', ' ']) == "expone"
            || l.trim_end_matches([':', '.', ' ']) == "exponen"
    });
    if expone && !notarial(&t) && (t.contains("comparece") || t.contains("solicit")) {
        return true;
    }
    let lineas: Vec<String> = todas.into_iter().take(8).collect();
    let Some(primera) = lineas.first() else {
        return false;
    };
    if ["revision fiscal", "revision de la carpeta"]
        .iter()
        .any(|p| primera.starts_with(p))
    {
        return true;
    }
    // La cabecera de un correo: su asunto, o quién lo manda o a quién con su dirección. Un
    // «Para:» o un «To:» solos son el cliente de una factura.
    let direcciones = ["de:", "from:", "para:", "to:", "cc:"];
    if lineas.iter().any(|l| {
        l.starts_with("asunto:")
            || l.starts_with("subject:")
            || (l.contains('@') && direcciones.iter().any(|c| l.starts_with(c)))
    }) {
        return true;
    }
    ["hola", "buenos dias", "buenas tardes"]
        .iter()
        .any(|p| primera.starts_with(p))
}

/// ¿Lleva alguna de las frases en una línea corta de arriba (un título)? Las de los títulos de
/// las liquidaciones salen también en las notas legales de los formularios («… no haya
/// practicado liquidación provisional…»).
fn en_titulo(texto: &str, frases: &[&str]) -> bool {
    texto
        .lines()
        .map(|l| fold(l).trim().to_owned())
        .filter(|l| !l.is_empty())
        .take(40)
        .filter(|l| l.chars().count() <= 100)
        .any(|l| frases.iter().any(|p| l.contains(p)))
}

/// Tipo de un documento oficial de la AEAT por su texto (el título, que va arriba, decide).
/// `None` si no lleva las marcas de la AEAT ni de la DEHú.
pub(crate) fn tipo(texto: &str) -> Option<DocKind> {
    let t = fold(texto);
    if !de_la_aeat(&t) && !de_la_dehu(&t) {
        return None;
    }
    let cabeza: String = t.chars().take(1500).collect();
    let c = cabeza.as_str();
    let any = |l: &[&str]| l.iter().any(|p| c.contains(p));
    let dehu = c.contains("direccion electronica habilitada unica") && c.contains("certifica");
    Some(
        if c.contains("informacion de la presentacion de la declaracion") {
            // El justificante de una declaración (los que se leen se toman antes por modelos).
            if any(&["modelo 036", "modelo 037"]) {
                DocKind::Censal036
            } else {
                DocKind::DocumentoAeat
            }
        } else if any(&[
            "recibo de presentacion en la notificacion",
            "comparecencia electronica",
        ]) || dehu
            || any(&[
                "puesta a disposicion de la notificacion",
                "certificado de notificacion",
            ])
        {
            DocKind::NotificacionAeat
        } else if any(&["recibo de presentacion", "resguardo de la solicitud"]) {
            DocKind::RegistroAeat
        } else if c.contains("certificado de situacion censal") {
            DocKind::CertificadoSituacionCensal
        } else if c.contains("censo de actividades economicas") {
            DocKind::CertificadoIae
        } else if c.contains("residencia fiscal") && c.contains("certific") {
            DocKind::CertificadoResidencia
        } else if c.contains("al corriente")
            && c.contains("obligaciones tributarias")
            && c.contains("certific")
        {
            DocKind::CertificadoAeat
        } else if en_titulo(
            texto,
            &[
                "liquidacion de recargo",
                "propuesta de liquidacion",
                "liquidacion provisional",
                "clave de liquidacion",
            ],
        ) {
            DocKind::LiquidacionAeat
        } else if any(&[
            "notificacion de requerimiento",
            "requerimiento de informacion",
            "emision de requerimiento",
            "se le requiere",
            "se requiere la siguiente",
        ]) {
            DocKind::RequerimientoAeat
        } else if en_titulo(texto, &["carta de pago", "documento de ingreso"]) {
            DocKind::CartaPagoAeat
        } else if c.contains("alta en el registro de operadores intracomunitarios") {
            DocKind::AltaRoi
        } else if any(&[
            "nuevo plazo",
            "ampliacion de plazo",
            "acuerdo",
            "resolucion",
        ]) {
            DocKind::ResolucionAeat
        } else if any(&["modelo 036", "modelo 037", "declaracion censal"]) {
            DocKind::Censal036
        } else if c.contains("numero de identificacion fiscal") && any(&["tarjeta", "asignacion"]) {
            DocKind::TarjetaNif
        } else if any(&["certificado", "certifica"]) {
            DocKind::CertificadoTributario
        } else if any(&[
            "notificacion",
            "comunicacion",
            "le informamos",
            "le recordamos",
            "estimado",
        ]) {
            DocKind::NotificacionAeat
        } else {
            DocKind::DocumentoAeat
        },
    )
}

/// Lo que se lee de un documento oficial de la AEAT para la revisión.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Oficial {
    /// Código seguro de verificación del propio documento (el de su pie).
    pub csv: Option<String>,
    /// Fecha del documento: la de la firma, la del certificado o la del asiento.
    pub fecha: Option<Date>,
    /// Número de referencia del documento (de un certificado, de un acuerdo) o del asiento de
    /// un recibo de registro.
    pub referencia: Option<String>,
    /// Expediente al que pertenece (el de un requerimiento, o aquél al que contesta un recibo).
    pub expediente: Option<String>,
    /// Número de certificado de la notificación (NCC): el de la notificación, o aquélla a la que
    /// se refiere un recibo o un certificado de la DEHú.
    pub notificacion: Option<String>,
    /// Código seguro de verificación del documento al que se refiere (un recibo de contestación
    /// o un certificado de puesta a disposición).
    pub csv_referido: Option<String>,
    /// Órgano que lo emite: «la Inspección», «Gestión Tributaria», «Recaudación».
    pub organo: Option<String>,
    /// Certificado con carácter positivo (`true`) o negativo (`false`).
    pub positivo: Option<bool>,
    /// Certificado: meses de validez que dice («una validez de doce meses»).
    pub validez_meses: Option<u32>,
    /// Certificado que vale mientras no cambien sus datos (el de situación censal).
    pub mientras_no_cambien: bool,
    /// Plazo para contestar, en días hábiles.
    pub plazo_dias: Option<u32>,
    /// Último día de un plazo ampliado («se le concede un nuevo plazo que finalizará el…»).
    pub nuevo_plazo: Option<Date>,
    /// Ejercicios que cita un requerimiento.
    pub ejercicios: BTreeSet<i32>,
    /// Impuestos que cita un requerimiento.
    pub conceptos: Vec<String>,
    /// Certificado de situación censal: sus obligaciones periódicas, como las escribe.
    pub obligaciones: Vec<String>,
    /// Certificado de situación censal: primera fecha de alta en el IVA o en una actividad.
    pub alta_iva: Option<Date>,
    /// Notificación: puesta a disposición y acceso (o aceptación en la DEHú).
    pub puesta: Option<Date>,
    pub accedida: Option<Date>,
    /// Recibo de registro: trámite y asunto.
    pub tramite: Option<String>,
    pub asunto: Option<String>,
    /// Liquidación: modelo, ejercicio y periodo de la autoliquidación, e importe.
    pub modelo: Option<(String, i32, String)>,
    pub importe: Option<Money>,
    /// Certificado del IAE: ejercicio al que se refiere.
    pub ejercicio: Option<i32>,
}

impl Oficial {
    /// Modelos de las obligaciones periódicas del certificado de situación censal: 111, 115,
    /// 123, 130, 200, 216, 303 y 349. `TODO(VERIFY)`: comprobado con certificados reales para
    /// algunos; los demás, por cómo los llama la AEAT en el censo.
    pub fn modelos_obligados(&self) -> BTreeSet<&'static str> {
        let mut out = BTreeSet::new();
        for o in &self.obligaciones {
            let t = fold(o);
            let m = if t.contains("arrend") || t.contains("inmuebles urbanos") {
                "115"
            } else if t.contains("trabajo") || t.contains("profesional") {
                "111"
            } else if t.contains("cap. mob") || t.contains("capital mobiliario") {
                "123"
            } else if t.contains("no residentes") {
                "216"
            } else if t.contains("recapitulativa") || t.contains("intracomunitari") {
                "349"
            } else if t.contains("pago fraccionado") && !t.contains("sociedades") {
                "130"
            } else if t.contains("sociedades") {
                "200"
            } else if t.contains("valor a") {
                "303"
            } else {
                continue;
            };
            out.insert(m);
        }
        out
    }
}

struct Rx {
    csv_pie: Regex,
    csv_etiqueta: Regex,
    fecha_firmado: Regex,
    fecha_firma: Regex,
    fecha_lugar: Regex,
    fecha_etiqueta: Regex,
    fecha_num: Regex,
    referencia: Regex,
    expediente: Regex,
    asiento: Regex,
    certificado: Regex,
    plazo: Regex,
    nuevo_plazo: Regex,
    años: Regex,
    puesta: Regex,
    accedida: Regex,
    tramite: Regex,
    asunto: Regex,
    modelo: Regex,
    reducido: Regex,
    recargo: Regex,
    ejercicio: Regex,
    validez: Regex,
}

fn rx() -> &'static Rx {
    static R: LazyLock<Rx> = LazyLock::new(|| {
        let r = |s: &str| Regex::new(s).expect("regla fija válida");
        const MES: &str = r"(enero|febrero|marzo|abril|mayo|junio|julio|agosto|septiembre|setiembre|octubre|noviembre|diciembre)";
        Rx {
            csv_pie: r(
                r"(?i)(?:verificable|comprobada)[^.]{0,80}?c[oó]digo\s+seguro\s+(?:de\s+)?verificaci[oó]n\s*(?:\(csv\))?\s*:?\s*([A-Z0-9]{16})\b",
            ),
            csv_etiqueta: r(
                r"(?i)(?:^|\s)(?:csv|c[oó]digo\s+seguro\s+de\s+verificaci[oó]n)\s*:\s*([A-Z0-9]{16})\b",
            ),
            // La de la firma: «Documento firmado electrónicamente (…) por …, 20 de septiembre de
            // 2021» o «…, con fecha 10 de octubre de 2025».
            fecha_firmado: r(&format!(
                r"(?i)firmado\s+electr[oó]nicamente.{{0,250}}?(\d{{1,2}})\s+de\s+{MES}\s+de\s+(\d{{4}})"
            )),
            fecha_firma: r(&format!(
                r"(?i)con\s+fecha\s+(\d{{1,2}})\s+de\s+{MES}\s+de\s+(\d{{4}})"
            )),
            // El lugar y la fecha del pie: «CIUDAD DEMO, 03 de marzo de 2025», «València, a 10
            // de marzo de 2026».
            fecha_lugar: r(&format!(
                r"\b\p{{Lu}}[\p{{L}}' ·.-]{{2,40}},\s+(?:a\s+)?(\d{{1,2}})\s+de\s+(?i:{MES})\s+de\s+(\d{{4}})"
            )),
            fecha_etiqueta: r(
                r"(?i)(?:^|\s)(?:fecha\s*:|fecha\s+de\s+emisi[oó]n\s*:?)\s*(\d{1,2})[-/](\d{1,2})[-/](\d{4})",
            ),
            fecha_num: r(r"\b(\d{1,2})[-/](\d{1,2})[-/](\d{4})\b"),
            referencia: r(
                r"(?i)(?:n[ºo°.]*\s*(?:de\s+)?referencia|(?:^|\s)referencia)\s*:\s*([A-Z0-9]{6,})",
            ),
            expediente: r(
                r"(?i)(?:n[ºo°.]*\s*de\s+expediente\s*:?|n[uú]mero\s+de\s+expediente\s*:?|expediente\s*/\s*referencia\s*:|referencia\s+requerimiento\s*:)\s*(?:[A-Z]{2,6}\s*-\s*)?([A-Z0-9]{8,})",
            ),
            asiento: r(r"(?i)n[uú]mero\s+de\s+asiento\s+registral\s*:\s*([A-Z0-9]{6,})"),
            certificado: r(
                r"(?i)(?:n[ºo°.]*\s*certificado|ncc\s+asociado|identificador)\s*:\s*(\d{10,})",
            ),
            plazo: r(
                r"(?i)plazo\s+(?:m[aá]ximo\s+)?de\s+(\d{1,2}|cinco|diez|quince|veinte)\s+d[ií]as\s+h[aá]biles",
            ),
            nuevo_plazo: r(
                r"(?i)nuevo\s+plazo\s+que\s+finalizar[aá]\s+el\s+(\d{1,2})[-/](\d{1,2})[-/](\d{4})",
            ),
            // «los años 2019 a 2021», «ejercicios 2021, 2022 y 2023», «del ejercicio 2020 al
            // 2022».
            años: r(
                r"(?i)\b(?:a[ñn]os?|ejercicios?)\s+((?:19|20)\d{2}(?:\s*(?:,|\by\b|\ba\b|\bal\b|-|–)\s*(?:19|20)\d{2})*)",
            ),
            puesta: r(
                r"(?i)(?:puesto\s+a\s+disposici[oó]n[^.]{0,80}?con\s+fecha|fecha\s+de\s+puesta\s+a\s+disposici[oó]n\s*:)\s*(\d{1,2})[-/](\d{1,2})[-/](\d{4})",
            ),
            accedida: r(
                r"(?i)(?:ha\s+accedido[^.]{0,250}?con\s+fecha|fecha\s+aceptaci[oó]n\s*:)\s*(\d{1,2})[-/](\d{1,2})[-/](\d{4})",
            ),
            tramite: r(
                r"(?i)(?:^|\s)tr[aá]mite\s*:\s*(.{3,160}?)\s+(?:procedimiento|expediente|asunto|csv)\s*:",
            ),
            asunto: r(
                r"(?i)(?:^|\s)asunto\s*:\s*(.{3,160}?)\s+(?:datos|nif|referencia|relaci[oó]n)\b",
            ),
            modelo: r(
                r"(?i)modelo\s+(\d{3})\b.{0,120}?ejercicio\s+(\d{4}),?\s+per[ií]odo\s+([0-9][A-Z0-9])",
            ),
            reducido: r(r"(?i)recargo\s+reducido\s*:?\s*(\d{1,3}(?:\.\d{3})*,\d{2})"),
            recargo: r(
                r"(?i)(?:importe\s+del\s+recargo|importe\s+a\s+ingresar|total\s+a\s+ingresar)\s*:?\s*(\d{1,3}(?:\.\d{3})*,\d{2})",
            ),
            ejercicio: r(r"(?i)correspondiente\s+al\s+ejercicio\s+(\d{4})"),
            validez: r(r"(?i)validez\s+de\s+(\d{1,2}|tres|seis|doce)\s+meses"),
        }
    });
    &R
}

fn mes(nombre: &str) -> Option<Month> {
    Some(match nombre.to_lowercase().as_str() {
        "enero" => Month::January,
        "febrero" => Month::February,
        "marzo" => Month::March,
        "abril" => Month::April,
        "mayo" => Month::May,
        "junio" => Month::June,
        "julio" => Month::July,
        "agosto" => Month::August,
        "septiembre" | "setiembre" => Month::September,
        "octubre" => Month::October,
        "noviembre" => Month::November,
        "diciembre" => Month::December,
        _ => return None,
    })
}

fn dia(d: &str, m: Month, y: &str) -> Option<Date> {
    Date::from_calendar_date(y.parse().ok()?, m, d.parse().ok()?).ok()
}

/// Fecha de tres grupos numéricos (día, mes, año).
fn de_numeros(c: &regex::Captures<'_>, desde: usize) -> Option<Date> {
    let m = Month::try_from(c.get(desde + 1)?.as_str().parse::<u8>().ok()?).ok()?;
    dia(c.get(desde)?.as_str(), m, c.get(desde + 2)?.as_str())
}

/// Fecha de un día, el mes en letras y un año.
fn de_letras(c: &regex::Captures<'_>) -> Option<Date> {
    dia(
        c.get(1)?.as_str(),
        mes(c.get(2)?.as_str())?,
        c.get(3)?.as_str(),
    )
}

/// Espacios seguidos (y saltos de línea) en uno.
fn compacto(s: &str) -> String {
    s.replace('\u{fb01}', "fi")
        .replace('\u{fb02}', "fl")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// El código seguro de verificación es de 16 letras y cifras, con alguna de cada.
fn csv_valido(s: &str) -> Option<String> {
    (s.chars().any(|c| c.is_ascii_digit()) && s.chars().any(|c| c.is_ascii_alphabetic()))
        .then(|| s.to_uppercase())
}

/// El primer valor de `re` con alguna cifra («el número de expediente indicado» no es uno).
fn con_cifras(re: &Regex, t: &str) -> Option<String> {
    re.captures_iter(t)
        .map(|c| c[1].to_uppercase())
        .find(|v| v.chars().any(|c| c.is_ascii_digit()))
}

fn numero_en_letras(s: &str) -> Option<u32> {
    match s.to_lowercase().as_str() {
        "tres" => Some(3),
        "cinco" => Some(5),
        "seis" => Some(6),
        "doce" => Some(12),
        "diez" => Some(10),
        "quince" => Some(15),
        "veinte" => Some(20),
        n => n.parse().ok(),
    }
}

/// Las líneas de las obligaciones periódicas de un certificado de situación censal.
fn obligaciones(texto: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut dentro = false;
    for linea in texto.lines() {
        let l = fold(linea);
        let l = l.trim();
        if l.starts_with("obligaciones periodicas") {
            dentro = true;
            continue;
        }
        if !dentro || l.is_empty() || l.starts_with("modelo periodicidad") || l == "modelo" {
            continue;
        }
        if l.starts_with("y para que conste")
            || l.starts_with("documento firmado")
            || l.starts_with("la presente")
        {
            break;
        }
        out.push(compacto(linea));
    }
    out
}

/// Primera fecha de alta en el IVA o en una actividad de un certificado de situación censal:
/// las fechas entre la situación tributaria y las obligaciones periódicas (la del régimen de
/// IVA y las de alta de cada actividad).
fn alta_iva(texto: &str) -> Option<Date> {
    let t = fold(texto);
    let desde = t.find("impuesto sobre el valor anadido")?;
    let hasta = t[desde..]
        .find("obligaciones periodicas")
        .map_or(t.len(), |i| desde + i);
    rx().fecha_num
        .captures_iter(&t[desde..hasta])
        .filter_map(|c| de_numeros(&c, 1))
        .min()
}

/// Los años de una lista: «2019 a 2021» (de uno a otro, hasta diez), «2021, 2022 y 2023».
fn lista_de_años(s: &str) -> BTreeSet<i32> {
    static AÑO: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"(?:19|20)\d{2}").ok());
    let mut out = BTreeSet::new();
    let Some(año) = AÑO.as_ref() else {
        return out;
    };
    let mut antes: Option<(i32, usize)> = None;
    for m in año.find_iter(s) {
        let Ok(y) = m.as_str().parse::<i32>() else {
            continue;
        };
        let rango = antes.and_then(|(a, fin)| {
            let sep = s[fin..m.start()].trim().to_lowercase();
            (["a", "al", "-", "–"].contains(&sep.as_str()) && y >= a && y - a <= 10).then_some(a)
        });
        match rango {
            Some(a) => out.extend(a..=y),
            None => {
                out.insert(y);
            }
        }
        antes = Some((y, m.end()));
    }
    out
}

/// Lee un documento oficial de la AEAT (su texto, tal como sale del PDF).
pub fn leer(texto: &str) -> Oficial {
    let r = rx();
    let t = compacto(texto);
    let plano = fold(texto);
    let mut o = Oficial {
        csv: r.csv_pie.captures_iter(&t).find_map(|c| csv_valido(&c[1])),
        csv_referido: r
            .csv_etiqueta
            .captures_iter(&t)
            .find_map(|c| csv_valido(&c[1])),
        referencia: r
            .asiento
            .captures(&t)
            .map(|c| c[1].to_uppercase())
            .or_else(|| con_cifras(&r.referencia, &t)),
        expediente: con_cifras(&r.expediente, &t),
        notificacion: r.certificado.captures(&t).map(|c| c[1].to_owned()),
        puesta: r.puesta.captures(&t).and_then(|c| de_numeros(&c, 1)),
        accedida: r.accedida.captures(&t).and_then(|c| de_numeros(&c, 1)),
        plazo_dias: r.plazo.captures(&t).and_then(|c| numero_en_letras(&c[1])),
        nuevo_plazo: r.nuevo_plazo.captures(&t).and_then(|c| de_numeros(&c, 1)),
        tramite: r.tramite.captures(&t).map(|c| c[1].trim().to_owned()),
        asunto: r.asunto.captures(&t).map(|c| c[1].trim().to_owned()),
        importe: r
            .reducido
            .captures(&t)
            .or_else(|| r.recargo.captures(&t))
            .and_then(|c| crate::received::parse_amount(&c[1])),
        modelo: r
            .modelo
            .captures(&t)
            .and_then(|c| Some((c[1].to_owned(), c[2].parse().ok()?, c[3].to_uppercase()))),
        ejercicio: r.ejercicio.captures(&t).and_then(|c| c[1].parse().ok()),
        obligaciones: obligaciones(texto),
        alta_iva: alta_iva(texto),
        ..Oficial::default()
    };
    // La fecha del documento es la de su firma, la del lugar y fecha del pie o la de su
    // etiqueta. Una fecha suelta del texto no lo es (un plazo, la de una ley, la de otra
    // declaración): sin ninguna de ésas, no se sabe.
    o.fecha = r
        .fecha_firmado
        .captures(&t)
        .and_then(|c| de_letras(&c))
        .or_else(|| r.fecha_firma.captures(&t).and_then(|c| de_letras(&c)))
        .or_else(|| r.fecha_lugar.captures(&t).and_then(|c| de_letras(&c)))
        .or_else(|| {
            r.fecha_etiqueta
                .captures(&t)
                .and_then(|c| de_numeros(&c, 1))
        })
        .or(o.accedida)
        .or(o.puesta);
    o.organo =
        if plano.contains("inspeccion regional") || plano.contains("inspeccion de los tributos") {
            Some("la Inspección".to_owned())
        } else if plano.contains("recaudacion") && !plano.contains("gestion tributaria") {
            Some("Recaudación".to_owned())
        } else if plano.contains("gestion tributaria") {
            Some("Gestión Tributaria".to_owned())
        } else {
            None
        };
    let residencia = plano.contains("residencia fiscal");
    o.positivo = if plano.contains("caracter de positivo") {
        Some(true)
    } else if plano.contains("caracter de negativo")
        || (residencia && plano.contains("no es residente"))
    {
        Some(false)
    } else if residencia && plano.contains("es residente en") {
        Some(true)
    } else {
        None
    };
    o.validez_meses = r.validez.captures(&t).and_then(|c| numero_en_letras(&c[1]));
    o.mientras_no_cambien = plano.contains("mientras no se modifiquen");
    // Los ejercicios que cita («en el año 2019 a 2021», «respecto a los años 2019 a 2022»), sin
    // los posteriores al documento.
    let hasta = o.fecha.map_or(i32::MAX, Date::year);
    for c in r.años.captures_iter(&t) {
        o.ejercicios
            .extend(lista_de_años(&c[1]).into_iter().filter(|y| *y <= hasta));
    }
    for (p, nombre) in [
        ("impuesto sobre sociedades", "Impuesto sobre Sociedades"),
        ("impuesto sobre el valor anadido", "IVA"),
        (" iva ", "IVA"),
        ("irpf", "IRPF"),
        ("impuesto sobre la renta de las personas fisicas", "IRPF"),
        ("retenciones", "retenciones"),
    ] {
        if plano.contains(p) && !o.conceptos.iter().any(|c| c == nombre) {
            o.conceptos.push(nombre.to_owned());
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn informes_y_correos_no_son_oficiales() {
        assert!(informe_o_correo(
            "Revisión de la carpeta de Empresa Demo SL\nRevisión automática hecha por TaxPain"
        ));
        assert!(informe_o_correo(
            "Revisión fiscal de Empresa Demo SL, 2020 a 2025\nEscritura ante notario…"
        ));
        assert!(informe_o_correo(
            "Asunto: Cuentas de 2025\n\nHola:\nEl depósito de cuentas en el Registro…"
        ));
        assert!(informe_o_correo(
            "Hola, Ana:\nTe paso la escritura del notario."
        ));
        assert!(!informe_o_correo(
            "CERTIFICADO DE SITUACIÓN CENSAL\nNº DE REFERENCIA: 20251234567"
        ));
        // Un escrito a la AEAT: quien comparece expone y solicita.
        assert!(informe_o_correo(
            "A LA AGENCIA ESTATAL DE ADMINISTRACIÓN TRIBUTARIA\nDon Pedro Demo, como \
             representante de EMPRESA DEMO SL, comparece y, como mejor proceda,\nEXPONE\n\
             Primero. Que mediante escritura de fecha 3 de febrero de 2023, con número de \
             protocolo 1.234…\nSOLICITO\nQue se tenga por presentado este escrito."
        ));
        // Una escritura también «expone», pero ante notario.
        assert!(!informe_o_correo(
            "ESCRITURA DE CONSTITUCIÓN\nAnte mí, Don Pedro Demo, Notario, comparece Don Ana \
             Demo.\nEXPONE\nQue desea constituir una sociedad. Doy fe."
        ));
        // Un correo con quién lo manda y a quién, con sus direcciones.
        assert!(informe_o_correo(
            "De: Ana Demo <ana@demo.es>\nPara: Pedro Demo <pedro@demo.es>\nTe paso la escritura."
        ));
        // El cliente de una factura («Para:», «To:») no la convierte en un correo, ni el dominio
        // de TaxPain en una factura del registrador.
        assert!(!informe_o_correo(
            "PUBLICIDAD DEMO SL\nCIF: B11111119\nFACTURA: D-00001\nPARA:\nEMPRESA DEMO SL"
        ));
        assert!(!informe_o_correo(
            "Proveedor Demo Ltd\n8 Demo st.\nTo:\nEMPRESA DEMO SL\nCalle Demo 1\nInvoice"
        ));
        assert!(!informe_o_correo(
            "REGISTRADOR DEMO SL\nFactura DD-2026-1\nRenovación del dominio taxpain.es (1 año)"
        ));
    }

    #[test]
    fn la_marca_de_la_aeat_es_su_pie_y_no_una_cita() {
        let pie = "Autenticidad verificable mediante Código Seguro Verificación ABCD1234EFGH5678 \
                   en sede.agenciatributaria.gob.es";
        assert!(de_la_aeat(&fold(pie)));
        // Una factura de la gestoría que cita el código de un justificante.
        assert!(!de_la_aeat(&fold(
            "ASESORES DEMO SL\nFactura nº: 2025-0042\nPresentación del modelo 303 del 1T 2025 \
             ante la Agencia Tributaria (código seguro de verificación del justificante: \
             ABCD1234EFGH5678)"
        )));
        // Un certificado de la agencia tributaria de una comunidad, con su código.
        let valenciana = fold(
            "AGÈNCIA TRIBUTÀRIA VALENCIANA\nCERTIFICADO DE ESTAR AL CORRIENTE\nCIUDAD DEMO, 10 de \
             marzo de 2026\nLa autenticidad de este documento puede comprobarse con el Código \
             Seguro de Verificación: ATVA1234DEMO5678 en https://sede.atv.demo",
        );
        assert!(!de_la_aeat(&valenciana));
        assert!(de_otra_administracion(&valenciana));
        assert_eq!(tipo("AGÈNCIA TRIBUTÀRIA VALENCIANA\nRequerimiento"), None);
    }

    #[test]
    fn una_nota_legal_no_es_una_liquidacion() {
        // El formulario de un modelo de la AEAT habla de la «liquidación provisional» en su nota
        // legal: no es una liquidación.
        let modelo = "200\n21-07-2025 a las 10:00:00\n2024200000000001A\nPresentación por \
             Internet\n2002000000001\nLa autenticidad de este documento puede ser comprobada \
             mediante el Código Seguro de Verificación ABCD1234EFGH5678 en \
             https://sede.agenciatributaria.gob.es\nNota: el declarante podrá solicitar de la \
             Administración la rectificación de la presente declaración siempre que la \
             Administración tributaria no haya practicado liquidación definitiva o liquidación \
             provisional por el mismo motivo.";
        assert_ne!(tipo(modelo), Some(DocKind::LiquidacionAeat));
        let recargo = "GESTION TRIBUTARIA\nLIQUIDACIÓN DE RECARGO POR PRESENTACIÓN FUERA DE \
             PLAZO\nCon fecha 12-05-2021 ha presentado la autoliquidación.\nAutenticidad \
             verificable mediante Código Seguro Verificación LIQU1234DEMO5678 en \
             sede.agenciatributaria.gob.es";
        assert_eq!(tipo(recargo), Some(DocKind::LiquidacionAeat));
    }

    #[test]
    fn marcas_de_documento_oficial() {
        assert!(con_marca(&fold(
            "Autenticidad verificable mediante Código Seguro Verificación ABCD1234EFGH5678 en \
             sede.agenciatributaria.gob.es"
        )));
        assert!(con_marca(&fold("Ante mí, Don Pedro Demo, Notario")));
        // Un número de protocolo no basta: un escrito cita el de una escritura.
        assert!(!con_marca(&fold(
            "Escritura de fecha 3 de febrero de 2023 con número de protocolo 1.234"
        )));
        assert!(con_marca(&fold(
            "REGISTRO MERCANTIL DE VALENCIA. Entrada 1/2025/123, diario 2025, asiento 456"
        )));
        assert!(!con_marca(&fold(
            "La escritura del notario y el depósito en el Registro Mercantil"
        )));
    }

    #[test]
    fn fechas_referencias_y_plazos() {
        let o = leer(
            "INSPECCIÓN REGIONAL\nNº de expediente: INSP - I123452025000123 ( cítese en la \
             respuesta )\nREQUERIMIENTO DE INFORMACIÓN\nJustificación de los ingresos en el año \
             2021 a 2023 y en el Impuesto sobre Sociedades. Deberá suministrar la información en \
             el plazo máximo de DIEZ días hábiles.\nCIUDAD DEMO, 03 de marzo de 2025\n\
             9000000000 Nº Certificado: 2512345678901\nAutenticidad verificable mediante Código \
             Seguro Verificación ABCD1234EFGH5678 en sede.agenciatributaria.gob.es",
        );
        assert_eq!(o.expediente.as_deref(), Some("I123452025000123"));
        assert_eq!(o.notificacion.as_deref(), Some("2512345678901"));
        assert_eq!(o.csv.as_deref(), Some("ABCD1234EFGH5678"));
        assert_eq!(o.fecha, Some(date!(2025 - 03 - 03)));
        assert_eq!(o.plazo_dias, Some(10));
        assert_eq!(o.organo.as_deref(), Some("la Inspección"));
        assert_eq!(
            o.ejercicios.iter().copied().collect::<Vec<_>>(),
            [2021, 2022, 2023]
        );
        assert!(o.conceptos.iter().any(|c| c == "Impuesto sobre Sociedades"));
    }

    #[test]
    fn los_ejercicios_de_una_lista_y_la_fecha_del_pie() {
        let o = leer(
            "INSPECCIÓN REGIONAL\nREQUERIMIENTO DE INFORMACIÓN\nJustificación de los ingresos de \
             los ejercicios 2021, 2022 y 2023 en el Impuesto sobre Sociedades.\nVALÈNCIA, 10 de \
             marzo de 2026\nAutenticidad verificable mediante Código Seguro Verificación \
             ABCD1234EFGH5678 en sede.agenciatributaria.gob.es",
        );
        assert_eq!(
            o.ejercicios.iter().copied().collect::<Vec<_>>(),
            [2021, 2022, 2023]
        );
        assert_eq!(o.fecha, Some(date!(2026 - 03 - 10)));
        let o =
            leer("Documentación de los años 2019 a 2021 y 2023.\nCiudad Demo, 3 de marzo de 2025");
        assert_eq!(
            o.ejercicios.iter().copied().collect::<Vec<_>>(),
            [2019, 2020, 2021, 2023]
        );
        assert_eq!(o.fecha, Some(date!(2025 - 03 - 03)));
    }

    #[test]
    fn la_fecha_es_la_de_la_firma_y_no_una_suelta() {
        // Las leyes que cita llevan fechas: la del documento es la de su firma.
        let o = leer(
            "GESTION TRIBUTARIA\nLIQUIDACIÓN DE RECARGO\nCon fecha 12-05-2021 ha presentado \
             autoliquidación. NORMAS APLICABLES Ley General Tributaria (Ley 58/2003, de 17 de \
             diciembre, B.O.E. de 18 de diciembre de 2003).\nDocumento firmado electrónicamente \
             (Ley 40/2015, artículo 43), por PEDRO DEMO, el Administrador, 20 de septiembre de \
             2021. Autenticidad verificable mediante Código Seguro Verificación \
             LIQU1234DEMO5678 en sede.agenciatributaria.gob.es",
        );
        assert_eq!(o.fecha, Some(date!(2021 - 09 - 20)));
        // Una comunicación sin fecha propia: la de un plazo no es la suya.
        let o = leer(
            "Estimado/a Sr./Sra.: El próximo 1 de enero de 2026 finaliza el plazo para adaptar \
             los programas de facturación.\nAutenticidad verificable mediante Código Seguro \
             Verificación COMU1234DEMO5678 en sede.agenciatributaria.gob.es",
        );
        assert_eq!(o.fecha, None);
    }

    #[test]
    fn validez_y_resultado_de_los_certificados() {
        let o = leer(
            "CERTIFICADO\nEl presente certificado se expide a petición del interesado, tiene \
             carácter de POSITIVO y una validez de doce meses contados desde la fecha de su \
             expedición.\nDocumento firmado electrónicamente (Ley 40/2015) por la Agencia \
             Estatal de Administración Tributaria, con fecha 10 de octubre de 2025.",
        );
        assert_eq!(o.positivo, Some(true));
        assert_eq!(o.validez_meses, Some(12));
        assert_eq!(o.fecha, Some(date!(2025 - 10 - 10)));
        let o = leer(
            "CERTIFICADO DE SITUACIÓN CENSAL\nLa presente certificación será válida y surtirá \
             efectos mientras no se modifiquen las circunstancias determinantes de su contenido.",
        );
        assert!(o.mientras_no_cambien);
        assert_eq!(o.validez_meses, None);
        let o = leer(
            "RESIDENCIA FISCAL EN ESPAÑA\nLas autoridades fiscales de ESPAÑA certifican que \
             EMPRESA DEMO SL con NIF B12345674, es residente en ESPAÑA.",
        );
        assert_eq!(o.positivo, Some(true));
    }
}
