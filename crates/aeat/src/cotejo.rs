//! Cotejo de documentos de la AEAT por su código seguro de verificación (CSV): el justificante
//! en PDF de una declaración presentada y, de las autoliquidaciones, el fichero de la
//! declaración (con sus casillas exactas).
//!
//! Fuentes (`docs/AEAT_INTEGRATION.md` §5 bis):
//! - **Servicio web oficial `CotejoInternetV1`** («E2 - Servicio web de cotejo de documentos
//!   electrónicos», especificación v1.00 de 11/01/2022, WSDL y XSD en
//!   `…/static_files/common/internet/dep/aduanas/es/aeat/kata/apli/ws/`). SOAP 1.1, documento
//!   literal, **con certificado personal** (mTLS) en `www1`. Entrada `peticionDocumento` con el
//!   `CSV` (16 caracteres) y `ENI` opcional; salida `cotejoResponse` con `documento/binario`
//!   (el PDF en base64) y `mensajeSalida` (`codigo` 1 correcto, 2 sin CSV, 3 no encontrado,
//!   4 anulado con `csvSustituto`, 5 no cotejable, 6-8 errores, 100 reintentar). La AEAT limita
//!   los accesos fallidos por día: no se reintenta en bucle.
//! - **Página pública de cotejo** de la Sede (`www2`, sin certificado ni sesión, comprobado el
//!   27/10/2026): `CotejoDocSv?CSV=` devuelve el PDF. Es la alternativa si el servicio web falla.
//! - **Visor de la declaración** (`ADHT-DECL/DeclaVisor?fDetalle=1&fCsv=&fNif=`, botón
//!   «Declaración electrónica: Ver XML» de la misma página): XML `DocumentoElectronicoV2` con la
//!   petición de presentación, cuyo campo `fichero` es la declaración en el formato de los
//!   diseños de registro. `TODO(VERIFY)`: no está documentado como servicio; si cambia, la app
//!   lee las casillas del PDF.
//!
//! Lo que sale del Mac: el CSV (y en el visor, el NIF del declarante). El CSV da acceso al
//! documento: la app lo guarda sólo en el almacén cifrado y nunca lo escribe en el registro.

use std::time::Duration;

use base64::Engine;
use url::Url;

use crate::http::{AeatClient, AeatError, without_url};

/// Servicio web oficial, producción, con certificado personal o de representante.
pub const SOAP_PRODUCCION: &str =
    "https://www1.agenciatributaria.gob.es/wlpl/KATA-APLI/CotejoInternetV1SOAP";
/// Servicio web oficial, entorno de pruebas (otra CA de servidor).
pub const SOAP_PRUEBAS: &str = "https://prewww1.aeat.es/wlpl/KATA-APLI/CotejoInternetV1SOAP";

/// Host de la Sede pública (sin certificado).
pub const WEB_HOST: &str = "www2.agenciatributaria.gob.es";
const WEB_PDF: &str = "https://www2.agenciatributaria.gob.es/wlpl/KATA-APLI/cotejo/CotejoDocSv";
const WEB_XML: &str = "https://www2.agenciatributaria.gob.es/wlpl/ADHT-DECL/DeclaVisor";

const NS_REQUEST: &str = "https://www3.agenciatributaria.gob.es/static_files/common/internet/dep/aduanas/es/aeat/kata/apli/ws/cotejo_request_int_V1.xsd";

/// Un CSV de la AEAT: 16 letras mayúsculas o cifras.
pub fn valid_csv(csv: &str) -> bool {
    csv.len() == 16
        && csv
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}

fn check_csv(csv: &str) -> Result<(), AeatError> {
    if valid_csv(csv) {
        Ok(())
    } else {
        Err(AeatError::Service(
            "el código seguro de verificación no es válido (16 letras o cifras)".into(),
        ))
    }
}

/// Sobre SOAP de la petición (§3.2 y ejemplo §5.2), sin pedir el formato ENI.
pub fn envelope(csv: &str) -> Result<String, AeatError> {
    check_csv(csv)?;
    Ok(format!(
        "<soapenv:Envelope xmlns:soapenv=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         xmlns:cot=\"{NS_REQUEST}\"><soapenv:Header/><soapenv:Body><cot:peticionDocumento>\
         <cot:CSV>{csv}</cot:CSV></cot:peticionDocumento></soapenv:Body></soapenv:Envelope>"
    ))
}

/// Texto de una referencia a entidad del XML (`&amp;`…).
fn entity(r: &quick_xml::events::BytesRef<'_>) -> &'static str {
    match r.as_ref() {
        "amp" => "&",
        "lt" => "<",
        "gt" => ">",
        "quot" => "\"",
        "apos" => "'",
        _ => "",
    }
}

/// Resultado del cotejo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// El documento (en la práctica, un PDF).
    Document(Vec<u8>),
    /// Anulado; si se sabe, el CSV del que lo sustituye.
    Annulled { replacement: Option<String> },
    /// La AEAT no tiene ningún documento con ese CSV.
    NotFound,
    /// Existe pero no se puede obtener (documento no cotejable).
    NotAvailable,
    /// Otro error del servicio (código y descripción), p. ej. «reinténtelo más tarde».
    Error { code: String, message: String },
}

/// Lee la respuesta del servicio web (`cotejoResponse`, §3.3 y ejemplos §5).
pub fn parse_response(xml: &str) -> Result<Outcome, AeatError> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    let mut text = String::new();
    let (mut binary, mut code, mut message, mut replacement, mut fault) =
        (None, None, String::new(), None, None);
    let mut root = false;
    loop {
        match reader
            .read_event()
            .map_err(|e| AeatError::InvalidResponse(format!("XML: {e}")))?
        {
            Event::Start(e) => {
                if e.local_name().as_ref() == "cotejoResponse" {
                    root = true;
                }
                text.clear();
            }
            Event::Text(t) => text.push_str(&t.xml10_content()),
            Event::GeneralRef(r) => text.push_str(entity(&r)),
            Event::CData(t) => text.push_str(&t.into_inner()),
            Event::End(e) => {
                let value = text.trim().to_owned();
                match e.local_name().as_ref() {
                    "binario" | "ValorBinario" if !value.is_empty() => binary = Some(value),
                    "codigo" => code = Some(value),
                    "descripcion" => message = value,
                    "csvSustituto" if !value.is_empty() => replacement = Some(value),
                    "faultstring" => fault = Some(value),
                    _ => {}
                }
                text.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if let Some(f) = fault {
        return Err(AeatError::Service(format!(
            "la AEAT no atendió el cotejo: {f}"
        )));
    }
    if !root {
        return Err(AeatError::InvalidResponse(
            "la respuesta no es del servicio de cotejo".into(),
        ));
    }
    let code = code.ok_or_else(|| AeatError::InvalidResponse("falta mensajeSalida".into()))?;
    Ok(match code.as_str() {
        "1" => {
            let b64: String = binary
                .ok_or_else(|| AeatError::InvalidResponse("cotejo correcto sin documento".into()))?
                .chars()
                .filter(|c| !c.is_whitespace())
                .collect();
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(b64.as_bytes())
                .map_err(|e| AeatError::InvalidResponse(format!("base64: {e}")))?;
            Outcome::Document(bytes)
        }
        "3" => Outcome::NotFound,
        "4" => Outcome::Annulled { replacement },
        "5" => Outcome::NotAvailable,
        _ => Outcome::Error { code, message },
    })
}

/// Pide el documento al servicio web oficial con el certificado de `client`.
pub async fn fetch(client: &AeatClient, url: &str, csv: &str) -> Result<Outcome, AeatError> {
    let body = envelope(csv)?;
    let (status, xml) = client.post_soap(url, &body).await?;
    // La AEAT devuelve los SOAP Fault con 500: se leen para dar su mensaje.
    match parse_response(&xml) {
        Ok(o) => Ok(o),
        Err(e) if status >= 400 => Err(match e {
            AeatError::Service(_) => e,
            _ => AeatError::Status(status),
        }),
        Err(e) => Err(e),
    }
}

/// URL de la página pública que devuelve el PDF.
pub fn pdf_url(csv: &str) -> Result<Url, AeatError> {
    check_csv(csv)?;
    let mut u = Url::parse(WEB_PDF).map_err(|_| AeatError::InvalidUrl)?;
    u.query_pairs_mut().append_pair("CSV", csv);
    Ok(u)
}

/// URL del visor de la declaración en XML.
pub fn xml_url(csv: &str, nif: &str) -> Result<Url, AeatError> {
    check_csv(csv)?;
    if nif.is_empty() || !nif.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(AeatError::Service("NIF no válido".into()));
    }
    let mut u = Url::parse(WEB_XML).map_err(|_| AeatError::InvalidUrl)?;
    u.query_pairs_mut()
        .append_pair("fDetalle", "1")
        .append_pair("fCsv", csv)
        .append_pair("fNif", &nif.to_ascii_uppercase());
    Ok(u)
}

/// Fichero de la declaración (formato de los diseños de registro) dentro del XML del visor:
/// el campo `fichero` del JSON que va en el elemento `Declaracion`. `None` si el documento no
/// lo trae (declaraciones informativas, otros documentos).
pub fn declaration_file(xml: &str) -> Result<Option<String>, AeatError> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    let mut inside = false;
    let mut root = false;
    let mut content = String::new();
    loop {
        match reader
            .read_event()
            .map_err(|e| AeatError::InvalidResponse(format!("XML: {e}")))?
        {
            Event::Start(e) => match e.local_name().as_ref() {
                "DocumentoElectronicoV2" => root = true,
                "Declaracion" => {
                    inside = true;
                    content.clear();
                }
                _ => {}
            },
            Event::CData(t) if inside => content.push_str(&t.into_inner()),
            Event::Text(t) if inside => content.push_str(&t.xml10_content()),
            Event::GeneralRef(r) if inside => content.push_str(entity(&r)),
            Event::End(e) if e.local_name().as_ref() == "Declaracion" => break,
            Event::Eof => break,
            _ => {}
        }
    }
    if !root {
        return Err(AeatError::InvalidResponse(
            "la respuesta no es una declaración de la AEAT".into(),
        ));
    }
    let content = content.trim();
    if content.is_empty() {
        return Ok(None);
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(content) {
        return Ok(v
            .get("fichero")
            .and_then(serde_json::Value::as_str)
            .filter(|f| looks_like_file(f))
            .map(str::to_owned));
    }
    Ok(looks_like_file(content).then(|| content.to_owned()))
}

/// ¿Es el fichero de una declaración? Las autoliquidaciones empiezan por `<T` y el modelo; las
/// informativas (registros de 500 posiciones), por el tipo de registro `1` y el modelo.
fn looks_like_file(f: &str) -> bool {
    let f = f.trim_start();
    // Por bytes: cortar el `&str` por posiciones podría caer en medio de una letra («1ÑÑ»).
    f.starts_with("<T")
        || (f.starts_with('1')
            && f.as_bytes()
                .get(1..4)
                .is_some_and(|d| d.iter().all(u8::is_ascii_digit)))
}

/// Lo que da el visor de la declaración de la Sede para un CSV.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Visor {
    /// El fichero de la declaración (en el formato de los diseños de registro).
    File(String),
    /// El visor da el documento, pero sin un fichero que leer.
    NoFile,
    /// El visor no tiene ese documento: responde su página de «Error interno», como con algunas
    /// declaraciones antiguas (comprobado con declaraciones reales). Es definitivo para ese CSV.
    /// `TODO(VERIFY)`: el visor no está documentado; si un «Error interno» pasajero se pareciera
    /// a éste, se tomaría por definitivo (la app permite volver a pedirlo).
    NotAvailable,
    /// Una respuesta que no es ni la declaración ni esa página de error (una página de
    /// mantenimiento, de límite de accesos, un XML de otra cosa…): no es definitiva, cuenta como
    /// fallo y se vuelve a pedir en la próxima comprobación.
    Unexpected,
}

/// Pausa mínima entre dos peticiones a la Sede pública.
pub const PUBLIC_PAUSE: Duration = Duration::from_millis(300);

/// Peticiones a la Sede pública por cliente (una comprobación) como máximo: la AEAT limita los
/// accesos al cotejo y no se insiste sin fin si algo va mal.
pub const PUBLIC_BUDGET: usize = 600;

/// Cliente de la Sede pública, **sin certificado**: servidor verificado con el almacén de
/// confianza del sistema, sólo `https` a `www2`, sin redirecciones ni proxies, tiempo corto,
/// una pausa entre peticiones y un tope de peticiones.
#[derive(Debug)]
pub struct PublicClient {
    http: reqwest::Client,
    last: std::sync::Mutex<Option<std::time::Instant>>,
    used: std::sync::atomic::AtomicUsize,
    budget: usize,
}

impl PublicClient {
    pub fn new() -> Result<Self, AeatError> {
        Self::with_budget(PUBLIC_BUDGET)
    }

    /// Con un tope de `budget` peticiones.
    pub fn with_budget(budget: usize) -> Result<Self, AeatError> {
        Ok(PublicClient {
            http: crate::http::public_client(Duration::from_secs(30))?,
            last: std::sync::Mutex::new(None),
            used: std::sync::atomic::AtomicUsize::new(0),
            budget,
        })
    }

    /// Reserva una petición del tope y espera la pausa desde la anterior.
    async fn turn(&self) -> Result<(), AeatError> {
        use std::sync::atomic::Ordering;
        if self.used.fetch_add(1, Ordering::SeqCst) >= self.budget {
            return Err(AeatError::Service(
                "se ha llegado al máximo de peticiones a la Sede en esta comprobación".into(),
            ));
        }
        let wait = {
            let mut last = self
                .last
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let now = std::time::Instant::now();
            let wait = last
                .map(|t| PUBLIC_PAUSE.saturating_sub(now.duration_since(t)))
                .unwrap_or_default();
            *last = Some(now + wait);
            wait
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        Ok(())
    }

    async fn get(&self, url: Url) -> Result<(String, Vec<u8>), AeatError> {
        if url.scheme() != "https" || url.host_str() != Some(WEB_HOST) {
            return Err(AeatError::HostNotAllowed(
                url.host_str().unwrap_or("").into(),
            ));
        }
        self.turn().await?;
        tracing::info!(host = WEB_HOST, path = url.path(), "GET cotejo AEAT");
        let res = self
            .http
            .get(url)
            .send()
            .await
            .map_err(|e| AeatError::Network(without_url(&e)))?;
        let status = res.status();
        tracing::info!(status = status.as_u16(), "respuesta cotejo AEAT");
        if !status.is_success() {
            return Err(AeatError::Status(status.as_u16()));
        }
        let ctype = res
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .to_ascii_lowercase();
        let body = crate::http::read_limited(res, crate::http::limits::DOCUMENT).await?;
        Ok((ctype, body))
    }

    /// El justificante en PDF de la página pública de cotejo.
    pub async fn pdf(&self, csv: &str) -> Result<Vec<u8>, AeatError> {
        self.pdf_answer(csv).await?.ok_or_else(|| {
            AeatError::Service(
                "la AEAT no devolvió el PDF de ese código (puede que no exista)".into(),
            )
        })
    }

    /// Como [`PublicClient::pdf`], pero distingue una respuesta de la Sede **sin PDF**
    /// (`Ok(None)`: un código que no existe o está anulado, o una página de aviso) de los
    /// errores de red, HTTP o del tope de peticiones (`Err`).
    pub async fn pdf_answer(&self, csv: &str) -> Result<Option<Vec<u8>>, AeatError> {
        let (_, body) = self.get(pdf_url(csv)?).await?;
        Ok(body.starts_with(b"%PDF").then_some(body))
    }

    /// El fichero de la declaración en el visor de la Sede. Su página de «Error interno» es
    /// [`Visor::NotAvailable`] (definitivo: el visor no tiene las declaraciones antiguas); otra
    /// respuesta que no es la declaración es [`Visor::Unexpected`]. Los errores (`Err`) son los
    /// de red, los HTTP y el tope de peticiones: se puede reintentar.
    pub async fn declaration(&self, csv: &str, nif: &str) -> Result<Visor, AeatError> {
        let (ctype, body) = self.get(xml_url(csv, nif)?).await?;
        Ok(visor_answer(&ctype, &body))
    }
}

/// Lo que dice una respuesta del visor (tipo de contenido y cuerpo).
pub fn visor_answer(ctype: &str, body: &[u8]) -> Visor {
    if !ctype.to_ascii_lowercase().contains("xml") {
        // Sólo la página de error del visor es definitiva.
        return if String::from_utf8_lossy(body).contains("Error interno") {
            Visor::NotAvailable
        } else {
            Visor::Unexpected
        };
    }
    let Ok(xml) = std::str::from_utf8(body) else {
        return Visor::Unexpected;
    };
    match declaration_file(xml) {
        Ok(Some(f)) => Visor::File(f),
        Ok(None) => Visor::NoFile,
        Err(_) => Visor::Unexpected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn la_sede_publica_tiene_tope_de_peticiones() {
        let c = PublicClient::with_budget(1).unwrap();
        // La primera petición cabe en el tope; la segunda, no (no sale nada a la red).
        assert!(c.turn().await.is_ok());
        let e = c.turn().await.unwrap_err();
        assert!(e.to_string().contains("máximo"), "{e}");
    }

    const CSV: &str = "ABCDEFGH12345678";

    fn wrap(inner: &str) -> String {
        format!(
            "<env:Envelope xmlns:env=\"http://schemas.xmlsoap.org/soap/envelope/\" \
             xmlns:cot=\"urn:x\"><env:Body><cot:cotejoResponse>{inner}</cot:cotejoResponse>\
             </env:Body></env:Envelope>"
        )
    }

    #[test]
    fn csv_valido_y_sobre_de_la_peticion() {
        assert!(valid_csv(CSV));
        for bad in [
            "",
            "abcdefgh12345678",
            "ABCDEFGH1234567",
            "ABCDEFGH1234567!",
        ] {
            assert!(!valid_csv(bad), "{bad}");
            assert!(envelope(bad).is_err());
        }
        let e = envelope(CSV).unwrap();
        assert!(e.contains("<cot:CSV>ABCDEFGH12345678</cot:CSV>"));
        assert!(e.contains(NS_REQUEST));
        assert!(!e.contains("ENI"));
    }

    #[test]
    fn respuesta_correcta_con_el_pdf() {
        let b64 = base64::engine::general_purpose::STANDARD.encode(b"%PDF-1.4 prueba");
        // El base64 puede venir partido en líneas.
        let (a, b) = b64.split_at(8);
        let xml = wrap(&format!(
            "<cot:documento><cot:huella>00</cot:huella><cot:algoritmo>SHA-1</cot:algoritmo>\
             <cot:binario>{a}\n{b}</cot:binario></cot:documento><cot:mensajeSalida>\
             <cot:codigo>1</cot:codigo><cot:descripcion>Correcto</cot:descripcion>\
             </cot:mensajeSalida>"
        ));
        assert_eq!(
            parse_response(&xml).unwrap(),
            Outcome::Document(b"%PDF-1.4 prueba".to_vec())
        );
    }

    #[test]
    fn anulado_no_encontrado_no_cotejable_y_errores() {
        let msg = |code: &str, extra: &str| {
            wrap(&format!(
                "<cot:mensajeSalida><cot:codigo>{code}</cot:codigo>\
                 <cot:descripcion>x</cot:descripcion>{extra}</cot:mensajeSalida>"
            ))
        };
        assert_eq!(
            parse_response(&msg(
                "4",
                "<cot:csvSustituto>ZZZZZZZZ99999999</cot:csvSustituto>"
            ))
            .unwrap(),
            Outcome::Annulled {
                replacement: Some("ZZZZZZZZ99999999".into())
            }
        );
        assert_eq!(parse_response(&msg("3", "")).unwrap(), Outcome::NotFound);
        assert_eq!(
            parse_response(&msg("5", "")).unwrap(),
            Outcome::NotAvailable
        );
        assert!(matches!(
            parse_response(&msg("100", "")).unwrap(),
            Outcome::Error { code, .. } if code == "100"
        ));
        // Correcto sin binario: respuesta rota.
        assert!(parse_response(&msg("1", "")).is_err());
        // SOAP Fault y páginas que no son del servicio.
        let fault = "<env:Envelope xmlns:env=\"http://schemas.xmlsoap.org/soap/envelope/\">\
            <env:Body><env:Fault><faultcode>env:Server</faultcode><faultstring>Límite de \
            accesos</faultstring></env:Fault></env:Body></env:Envelope>";
        assert!(
            matches!(parse_response(fault), Err(AeatError::Service(m)) if m.contains("Límite"))
        );
        assert!(parse_response("<html><body>Mantenimiento</body></html>").is_err());
    }

    #[test]
    fn urls_de_la_sede_publica() {
        assert_eq!(
            pdf_url(CSV).unwrap().as_str(),
            "https://www2.agenciatributaria.gob.es/wlpl/KATA-APLI/cotejo/CotejoDocSv?CSV=ABCDEFGH12345678"
        );
        assert_eq!(
            xml_url(CSV, "b12345674").unwrap().as_str(),
            "https://www2.agenciatributaria.gob.es/wlpl/ADHT-DECL/DeclaVisor?fDetalle=1&fCsv=ABCDEFGH12345678&fNif=B12345674"
        );
        assert!(pdf_url("../x").is_err());
        assert!(xml_url(CSV, "B1234&x=1").is_err());
        assert!(xml_url(CSV, "").is_err());
    }

    #[test]
    fn fichero_de_la_declaracion_en_el_xml_del_visor() {
        let xml = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/aeat/cotejo/declaracion-ejemplo.xml"
        ))
        .unwrap();
        let f = declaration_file(&xml).unwrap().unwrap();
        assert!(f.starts_with("<T3030"), "{f}");
        assert!(f.contains("EMPRESA DEMO SL"));
        // Sin `Declaracion` o con otra cosa dentro: nada.
        let empty =
            "<DocumentoElectronicoV2 xmlns=\"urn:x\"><DatosDocumento/></DocumentoElectronicoV2>";
        assert_eq!(declaration_file(empty).unwrap(), None);
        let other = "<DocumentoElectronicoV2><DatosDocumento><Declaracion><![CDATA[{\"otro\":1}]]>\
            </Declaracion></DatosDocumento></DocumentoElectronicoV2>";
        assert_eq!(declaration_file(other).unwrap(), None);
        assert!(declaration_file("<html/>").is_err());
        // Una informativa: registros de 500 posiciones que empiezan por «1» y el modelo.
        let informativa = "<DocumentoElectronicoV2><Declaracion><![CDATA[{\"fichero\":\"11902025B12345674EMPRESA DEMO SL\"}]]>\
            </Declaracion></DocumentoElectronicoV2>";
        assert_eq!(
            declaration_file(informativa).unwrap().as_deref(),
            Some("11902025B12345674EMPRESA DEMO SL")
        );
    }

    #[test]
    fn un_fichero_raro_no_rompe_nada() {
        // Cortar por bytes en medio de una letra haría saltar la app.
        assert!(!looks_like_file("1ññ"));
        assert!(!looks_like_file("1é"));
        assert!(looks_like_file("1190"));
        let xml =
            "<DocumentoElectronicoV2><Declaracion>1º error</Declaracion></DocumentoElectronicoV2>";
        assert_eq!(declaration_file(xml).unwrap(), None);
    }

    #[test]
    fn respuestas_del_visor() {
        // Las declaraciones antiguas: una página HTML de «Error interno». Definitivo.
        assert_eq!(
            visor_answer("text/html; charset=UTF-8", b"<html>Error interno</html>"),
            Visor::NotAvailable
        );
        // Una página de mantenimiento o de límite de accesos con estado 200 no es definitiva.
        assert_eq!(
            visor_answer(
                "text/html",
                b"<html>Servicio no disponible temporalmente</html>"
            ),
            Visor::Unexpected
        );
        assert_eq!(visor_answer("text/xml", b"<html/>"), Visor::Unexpected);
        assert_eq!(visor_answer("text/xml", &[0xff, 0xfe]), Visor::Unexpected);
        assert_eq!(
            visor_answer(
                "text/xml;charset=utf-8",
                b"<DocumentoElectronicoV2><DatosDocumento/></DocumentoElectronicoV2>"
            ),
            Visor::NoFile
        );
        let xml = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/aeat/cotejo/declaracion-ejemplo.xml"
        ))
        .unwrap();
        assert!(
            matches!(visor_answer("text/xml", &xml), Visor::File(f) if f.starts_with("<T3030"))
        );
    }
}
