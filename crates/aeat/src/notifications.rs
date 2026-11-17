//! Servicio web de comunicaciones y notificaciones de la AEAT (GNNO-JDIT).
//!
//! Fuente: «Servicio Web comunicaciones y notificaciones AEAT», v1.7 (30/06/2025), y sus WSDL y
//! XSD (`fixtures/aeat/notificaciones/`, con SHA-256). SOAP 1.1 document/literal con mTLS y el
//! certificado del propio titular; «no se requiere ningún tipo de registro previo».
//!
//! - **Consulta** (`ConsultaV1SOAP`): envíos de un NIF por tipo (C comunicaciones, N
//!   notificaciones, T todos), estado (A accedido, P pendiente, T todos) y un rango de fechas de
//!   **tres meses como máximo**; versión 1.3 con paginación (`ClavePaginacion`, 0 = no hay más).
//! - **Acceso** (`AccesoV1SOAP`): `D` descarga una comunicación o una notificación ya comparecida;
//!   `C` **comparece** a una notificación pendiente. Comparecer equivale a abrirla: la
//!   notificación se entiende practicada y **empiezan a correr los plazos** (art. 43 Ley 39/2015).
//!   Por eso sólo se hace con la confirmación expresa del usuario.

use base64::Engine;
use quick_xml::Reader;
use quick_xml::events::Event;

use crate::http::{AeatClient, AeatError};

const NS_BASE: &str =
    "https://www2.agenciatributaria.gob.es/static_files/common/dep/aduanas/es/aeat/gnno/jdit/ws";

pub const CONSULTA_PRODUCCION: &str =
    "https://www1.agenciatributaria.gob.es/wlpl/GNNO-JDIT/sede/descargas/ConsultaV1SOAP";
pub const CONSULTA_PRUEBAS: &str =
    "https://prewww1.aeat.es/wlpl/GNNO-JDIT/sede/descargas/ConsultaV1SOAP";
pub const ACCESO_PRODUCCION: &str =
    "https://www1.agenciatributaria.gob.es/wlpl/GNNO-JDIT/sede/descargas/AccesoV1SOAP";
pub const ACCESO_PRUEBAS: &str =
    "https://prewww1.aeat.es/wlpl/GNNO-JDIT/sede/descargas/AccesoV1SOAP";

/// Par de URLs (consulta y acceso) de un entorno.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoints {
    pub consulta: String,
    pub acceso: String,
}

impl Endpoints {
    pub fn production() -> Self {
        Endpoints {
            consulta: CONSULTA_PRODUCCION.into(),
            acceso: ACCESO_PRODUCCION.into(),
        }
    }

    /// Entorno de pruebas: los envíos de prueba se piden a wsnotificaciones@correo.aeat.es.
    pub fn testing() -> Self {
        Endpoints {
            consulta: CONSULTA_PRUEBAS.into(),
            acceso: ACCESO_PRUEBAS.into(),
        }
    }

    /// Servidor simulado (tests). Sólo con la feature `mock`: la app no lo lleva.
    #[cfg(any(test, feature = "mock"))]
    pub fn mock(base: &str) -> Self {
        Endpoints {
            consulta: format!("{base}/ConsultaV1SOAP"),
            acceso: format!("{base}/AccesoV1SOAP"),
        }
    }
}

/// Un envío según la consulta (metadatos públicos: no abre nada).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Envio {
    /// Identificador de 13 dígitos.
    pub id: String,
    /// T titular, A autorizado.
    pub role: String,
    /// A accedido, P pendiente.
    pub state: String,
    /// C comunicación, N notificación.
    pub kind: String,
    /// `dateTime` de la puesta a disposición.
    pub available_at: String,
    pub subject: String,
    pub holder_nif: String,
    pub recipient_nif: String,
}

impl Envio {
    pub fn is_pending_notification(&self) -> bool {
        self.kind == "N" && self.state == "P"
    }
}

/// Contenido de un envío accedido.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Contenido {
    pub id: String,
    /// Metadatos privados (nombre → valor), p. ej. `Concepto`, `Modelo`, `Importe`,
    /// `CSVDocumentoPDF`, `FechaAcceso`.
    pub metadata: Vec<(String, String)>,
    pub document_pdf: Vec<u8>,
    /// Certificación de la comparecencia (diligencia), en PDF.
    pub certification_pdf: Vec<u8>,
}

impl Contenido {
    pub fn meta(&self, key: &str) -> &str {
        self.metadata
            .iter()
            .find(|(k, _)| k == key)
            .map_or("", |(_, v)| v.as_str())
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NotifError {
    #[error("la AEAT respondió {code}: {message}")]
    Code { code: String, message: String },
    #[error("el rango de fechas no puede superar tres meses")]
    RangeTooLong,
    #[error("comparecer abre los plazos de la notificación: hace falta tu confirmación")]
    NotConfirmed,
    #[error("comparecer en producción no está permitido en una compilación de desarrollo")]
    ProductionInDebug,
    #[error("identificador de envío no válido")]
    InvalidId,
    #[error("demasiados resultados: acorta el periodo de la consulta")]
    TooManyPages,
}

/// Los identificadores de envío de la AEAT (`NumeroCertificado`) son numéricos. Se validan
/// antes de enviarlos y antes de usarlos en nombres de fichero.
pub fn valid_id(id: &str) -> bool {
    (1..=30).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}

fn envelope(prefix: &str, xsd: &str, body: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><env:Envelope xmlns:env=\"http://schemas.xmlsoap.org/soap/envelope/\" xmlns:{prefix}=\"{NS_BASE}/{xsd}\"><env:Header/><env:Body>{body}</env:Body></env:Envelope>"
    )
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Fecha `dd-mm-aaaa` (TipoFecha).
pub fn date_param(d: time::Date) -> String {
    format!("{:02}-{:02}-{:04}", d.day(), u8::from(d.month()), d.year())
}

/// Petición de consulta v1.3 (todos los tipos y estados).
pub fn consulta_request(nif: &str, from: time::Date, to: time::Date, page: &str) -> String {
    envelope(
        "pc",
        "PeticionConsulta.xsd",
        &format!(
            "<pc:PeticionConsulta><pc:Version>1.3</pc:Version><pc:Nif>{}</pc:Nif><pc:TipoEnvio>T</pc:TipoEnvio><pc:Estado>T</pc:Estado><pc:FechaInicio>{}</pc:FechaInicio><pc:FechaFin>{}</pc:FechaFin><pc:ClavePaginacion>{}</pc:ClavePaginacion></pc:PeticionConsulta>",
            esc(&nif.to_uppercase()),
            date_param(from),
            date_param(to),
            esc(page)
        ),
    )
}

/// Operación de acceso.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    /// C: comparecer (abre la notificación y los plazos).
    Comparecer,
    /// D: descargar una comunicación o una notificación ya comparecida.
    Descargar,
}

pub fn acceso_request(id: &str, op: Operation) -> String {
    let op = match op {
        Operation::Comparecer => "C",
        Operation::Descargar => "D",
    };
    envelope(
        "pa",
        "PeticionAcceso.xsd",
        &format!(
            "<pa:PeticionAcceso><pa:Version>1.1</pa:Version><pa:NumeroCertificado>{}</pa:NumeroCertificado><pa:Operacion>{op}</pa:Operacion></pa:PeticionAcceso>",
            esc(id)
        ),
    )
}

/// Recorre el XML y llama a `f(ruta, texto)` al cerrar cada elemento con texto.
fn walk(xml: &str, mut f: impl FnMut(&[String], &str)) -> Result<(), AeatError> {
    let mut reader = Reader::from_str(xml);
    let mut path: Vec<String> = Vec::new();
    let mut text = String::new();
    loop {
        match reader
            .read_event()
            .map_err(|e| AeatError::InvalidResponse(format!("XML: {e}")))?
        {
            Event::Start(e) => {
                path.push(e.local_name().as_ref().to_owned());
                text.clear();
            }
            Event::Text(t) => text.push_str(&t.xml10_content()),
            Event::GeneralRef(r) => text.push_str(match r.as_ref() {
                "amp" => "&",
                "lt" => "<",
                "gt" => ">",
                "quot" => "\"",
                "apos" => "'",
                _ => "",
            }),
            Event::End(_) => {
                f(&path, text.trim());
                text.clear();
                path.pop();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

fn check_code(code: &str, message: &str) -> Result<(), NotifError> {
    if code == "00" {
        Ok(())
    } else {
        Err(NotifError::Code {
            code: code.into(),
            message: message.into(),
        })
    }
}

/// Respuesta de consulta: envíos y clave de la página siguiente (`None` si no hay más).
#[allow(clippy::type_complexity)]
pub fn parse_consulta(
    xml: &str,
) -> Result<Result<(Vec<Envio>, Option<String>), NotifError>, AeatError> {
    let mut envios = Vec::new();
    let mut cur = Envio::default();
    let (mut code, mut msg, mut page) = (String::new(), String::new(), None);
    walk(xml, |path, text| {
        let Some(name) = path.last().map(String::as_str) else {
            return;
        };
        let in_envio = path.iter().any(|p| p == "Envio");
        match (in_envio, name) {
            (true, "NumeroCertificado") => cur.id = text.into(),
            (true, "Rol") => cur.role = text.into(),
            (true, "Estado") => cur.state = text.into(),
            (true, "TipoEnvio") => cur.kind = text.into(),
            (true, "FechaPuestaDisposicion") => cur.available_at = text.replace(['\n', ' '], ""),
            (true, "Asunto") => cur.subject = text.into(),
            (true, "NifTitular") => cur.holder_nif = text.into(),
            (true, "NifDestinatario") => cur.recipient_nif = text.into(),
            (true, "Envio") => envios.push(std::mem::take(&mut cur)),
            (false, "CodRespuesta") => code = text.into(),
            (false, "MsgRespuesta") => msg = text.into(),
            (false, "ClavePaginacion") => page = Some(text.to_owned()),
            _ => {}
        }
    })?;
    if let Err(e) = check_code(&code, &msg) {
        return Ok(Err(e));
    }
    Ok(Ok((envios, page.filter(|p| !p.is_empty() && p != "0"))))
}

pub fn parse_acceso(xml: &str) -> Result<Result<Contenido, NotifError>, AeatError> {
    let mut c = Contenido::default();
    let (mut code, mut msg) = (String::new(), String::new());
    let (mut doc, mut cert) = (String::new(), String::new());
    walk(xml, |path, text| {
        let Some(name) = path.last().map(String::as_str) else {
            return;
        };
        if path.iter().any(|p| p == "MetadatosPrivados") && name != "MetadatosPrivados" {
            c.metadata.push((name.to_owned(), text.to_owned()));
            return;
        }
        match name {
            "NumeroCertificado" => c.id = text.into(),
            "DocumentoPDF" => doc = text.into(),
            "CertificacionPDF" => cert = text.into(),
            "CodRespuesta" => code = text.into(),
            "MsgRespuesta" => msg = text.into(),
            _ => {}
        }
    })?;
    if let Err(e) = check_code(&code, &msg) {
        return Ok(Err(e));
    }
    let b64 = |s: &str| {
        let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        base64::engine::general_purpose::STANDARD
            .decode(clean)
            .map_err(|e| AeatError::InvalidResponse(format!("PDF en base64: {e}")))
    };
    c.document_pdf = b64(&doc)?;
    c.certification_pdf = b64(&cert)?;
    Ok(Ok(c))
}

/// Tramos de como máximo tres meses (90 días) que cubren `[from, to]`.
pub fn ranges(from: time::Date, to: time::Date) -> Vec<(time::Date, time::Date)> {
    let mut out = Vec::new();
    let mut start = from;
    while start <= to {
        let end = (start + time::Duration::days(89)).min(to);
        out.push((start, end));
        start = end + time::Duration::days(1);
    }
    out
}

async fn soap(client: &AeatClient, url: &str, body: &str) -> Result<String, AeatError> {
    let (status, text) = client.post_soap(url, body).await?;
    soap_body(status, text)
}

/// Cuerpo de una respuesta SOAP correcta. Un Fault llega con HTTP 500 (SOAP 1.1 §6.2) y la AEAT
/// explica en `faultstring` qué ha fallado (NIF o versión incorrectos, esquema…): se devuelve
/// ese motivo en vez de un «respuesta HTTP 500» mudo.
fn soap_body(status: u16, text: String) -> Result<String, AeatError> {
    if (200..300).contains(&status) {
        return Ok(text);
    }
    if status == 500
        && let Some(fault) = fault_string(&text)
    {
        return Err(AeatError::Service(format!(
            "la AEAT no atendió la petición: {fault}"
        )));
    }
    Err(AeatError::Status(status))
}

/// Texto de `<faultstring>` (con o sin prefijo de espacio de nombres), sin entidades XML.
fn fault_string(xml: &str) -> Option<String> {
    let mut from = 0;
    loop {
        let i = from + xml[from..].find("faultstring")?;
        let lt = xml[..i].rfind('<')?;
        // Etiqueta de apertura: entre «<» y el nombre no hay «/» (eso sería la de cierre).
        if !xml[lt + 1..i].contains('/') {
            let open_end = i + xml[i..].find('>')?;
            let close = open_end + 1 + xml[open_end + 1..].find("</")?;
            let s = xml[open_end + 1..close].trim();
            return (!s.is_empty()).then(|| {
                s.replace("&lt;", "<")
                    .replace("&gt;", ">")
                    .replace("&quot;", "\"")
                    .replace("&apos;", "'")
                    .replace("&amp;", "&")
            });
        }
        from = i + 1;
    }
}

/// Lista los envíos de `nif` entre `from` y `to` (troceado en tramos de tres meses y
/// recorriendo todas las páginas). No abre nada.
pub async fn list(
    client: &AeatClient,
    ep: &Endpoints,
    nif: &str,
    from: time::Date,
    to: time::Date,
) -> Result<Result<Vec<Envio>, NotifError>, AeatError> {
    let mut all = Vec::new();
    for (a, b) in ranges(from, to) {
        let mut page = "0".to_owned();
        // Límite de seguridad: 100 páginas por tramo; si se alcanza, error (no se trunca).
        let mut done = false;
        for _ in 0..100 {
            let xml = soap(client, &ep.consulta, &consulta_request(nif, a, b, &page)).await?;
            match parse_consulta(&xml)? {
                Err(e) => return Ok(Err(e)),
                Ok((envios, next)) => {
                    all.extend(envios);
                    match next {
                        Some(n) => page = n,
                        None => {
                            done = true;
                            break;
                        }
                    }
                }
            }
        }
        if !done {
            return Ok(Err(NotifError::TooManyPages));
        }
    }
    Ok(Ok(all))
}

/// Descarga una comunicación o una notificación ya comparecida (operación D).
pub async fn download(
    client: &AeatClient,
    ep: &Endpoints,
    id: &str,
) -> Result<Result<Contenido, NotifError>, AeatError> {
    if !valid_id(id) {
        return Ok(Err(NotifError::InvalidId));
    }
    let xml = soap(
        client,
        &ep.acceso,
        &acceso_request(id, Operation::Descargar),
    )
    .await?;
    parse_acceso(&xml)
}

/// Condiciones previas a comparecer (sin red).
pub fn check_appear(ep: &Endpoints, id: &str, confirmed: bool) -> Result<(), NotifError> {
    if !confirmed {
        return Err(NotifError::NotConfirmed);
    }
    if !valid_id(id) {
        return Err(NotifError::InvalidId);
    }
    // Comparecer tiene efectos jurídicos: nunca en producción desde una compilación de desarrollo.
    if cfg!(debug_assertions) && ep.acceso == ACCESO_PRODUCCION {
        return Err(NotifError::ProductionInDebug);
    }
    Ok(())
}

/// **Comparece** a una notificación pendiente (operación C): la notificación queda practicada y
/// empiezan los plazos. `confirmed` debe venir de una confirmación expresa del usuario.
pub async fn appear(
    client: &AeatClient,
    ep: &Endpoints,
    id: &str,
    confirmed: bool,
) -> Result<Result<Contenido, NotifError>, AeatError> {
    if let Err(e) = check_appear(ep, id, confirmed) {
        return Ok(Err(e));
    }
    let xml = soap(
        client,
        &ep.acceso,
        &acceso_request(id, Operation::Comparecer),
    )
    .await?;
    parse_acceso(&xml)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn un_500_con_fault_devuelve_el_motivo_de_la_aeat() {
        let fault = "<env:Envelope xmlns:env=\"http://schemas.xmlsoap.org/soap/envelope/\"><env:Body>\
             <env:Fault><faultcode>env:Client</faultcode>\
             <faultstring>Codigo[4102].El XML no cumple el esquema &lt;Consulta&gt;</faultstring>\
             </env:Fault></env:Body></env:Envelope>";
        match soap_body(500, fault.to_owned()) {
            Err(AeatError::Service(m)) => {
                assert!(
                    m.contains("Codigo[4102]") && m.contains("<Consulta>"),
                    "{m}"
                );
            }
            other => panic!("esperaba el faultstring, no {other:?}"),
        }
        // Con prefijo de espacio de nombres y atributos también.
        let qualified = "<soap:Envelope><soap:Body><soap:Fault>\
             <soap:faultstring xml:lang=\"es\"> NIF incorrecto </soap:faultstring>\
             </soap:Fault></soap:Body></soap:Envelope>";
        assert_eq!(fault_string(qualified).as_deref(), Some("NIF incorrecto"));
        // Un 500 sin Fault o cualquier otro estado siguen siendo errores de estado.
        assert!(matches!(
            soap_body(500, "<html>caído</html>".into()),
            Err(AeatError::Status(500))
        ));
        assert!(matches!(
            soap_body(503, fault.to_owned()),
            Err(AeatError::Status(503))
        ));
        assert_eq!(soap_body(200, "<ok/>".into()).unwrap(), "<ok/>");
    }

    #[test]
    fn comparecer_exige_confirmacion_id_y_no_produccion_en_desarrollo() {
        let t = Endpoints::testing();
        assert_eq!(check_appear(&t, "2319486500018", true), Ok(()));
        assert_eq!(
            check_appear(&t, "2319486500018", false),
            Err(NotifError::NotConfirmed)
        );
        for bad in ["", "../x", "12a", "1/2"] {
            assert_eq!(check_appear(&t, bad, true), Err(NotifError::InvalidId));
        }
        let prod = check_appear(&Endpoints::production(), "2319486500018", true);
        if cfg!(debug_assertions) {
            assert_eq!(prod, Err(NotifError::ProductionInDebug));
        } else {
            assert_eq!(prod, Ok(()));
        }
    }

    #[test]
    fn tramos_de_tres_meses() {
        let r = ranges(date!(2026 - 01 - 01), date!(2026 - 10 - 14));
        assert_eq!(r.first().map(|x| x.0), Some(date!(2026 - 01 - 01)));
        assert_eq!(r.last().map(|x| x.1), Some(date!(2026 - 10 - 14)));
        assert!(r.iter().all(|(a, b)| (*b - *a).whole_days() <= 89));
        assert!(
            r.windows(2)
                .all(|w| w[1].0 == w[0].1 + time::Duration::days(1))
        );
    }

    #[test]
    fn peticiones() {
        let c = consulta_request(
            "12345678z",
            date!(2026 - 07 - 01),
            date!(2026 - 09 - 28),
            "0",
        );
        assert!(c.contains("<pc:Nif>12345678Z</pc:Nif>"));
        assert!(c.contains("<pc:FechaInicio>01-07-2026</pc:FechaInicio>"));
        assert!(c.contains("<pc:Version>1.3</pc:Version>"));
        let a = acceso_request("2319486500018", Operation::Descargar);
        assert!(a.contains("<pa:Operacion>D</pa:Operacion>"));
    }

    fn fixture(name: &str) -> String {
        std::fs::read_to_string(format!(
            "{}/../../fixtures/aeat/notificaciones/{name}",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap_or_default()
    }

    #[test]
    fn consulta_del_ejemplo_oficial() {
        let Ok(Ok((envios, next))) = parse_consulta(&fixture("respuesta-consulta-ejemplo.xml"))
        else {
            panic!("respuesta no válida");
        };
        assert_eq!(envios.len(), 2);
        assert_eq!(envios[0].id, "2319486500018");
        assert_eq!(envios[0].available_at, "2023-01-02T10:34:53");
        assert_eq!(envios[1].state, "P");
        assert!(envios[1].is_pending_notification());
        assert_eq!(next.as_deref(), Some("1"));
        let err = "<RespuestaConsulta><CodRespuesta>07</CodRespuesta><MsgRespuesta>El rango máximo de consulta es de tres meses</MsgRespuesta></RespuestaConsulta>";
        assert!(matches!(
            parse_consulta(err),
            Ok(Err(NotifError::Code { code, .. })) if code == "07"
        ));
    }

    #[test]
    fn acceso_con_pdf() {
        let Ok(Ok(c)) = parse_acceso(&fixture("respuesta-acceso-ejemplo.xml")) else {
            panic!("respuesta no válida");
        };
        assert_eq!(c.id, "2319486500018");
        assert_eq!(c.meta("Modelo"), "303");
        assert_eq!(c.meta("Concepto"), "Requerimiento de información");
        assert!(c.document_pdf.starts_with(b"%PDF"));
        assert!(c.certification_pdf.starts_with(b"%PDF"));
    }
}
