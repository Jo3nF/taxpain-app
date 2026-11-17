//! Prevalidación de autoliquidaciones con el servicio **ServValiDos** de la AEAT.
//!
//! Fuente: «Especificaciones de los servicios de Presentación Directa, Consulta de Declaraciones
//! Presentadas y Validación de Impresión», v30, §3 y §4.2 (`docs/AEAT_INTEGRATION.md` §2).
//! - POST JSON a `https://prewww2.aeat.es/wlpl/PFTW-PICW/ServValiDos` con
//!   `{MODELO, EJERCICIO, PERIODO, IDI, F01}`; `F01` es el fichero DR completo sin CRLF ni
//!   tabuladores (la petición va en UTF-8).
//! - Respuesta: `RESPUESTA` con `AVISOS` y el `PDF` del borrador en base64, o `respuesta.errores`.
//! - Admite 111, 115, 130, 202, 303, 390… (§5); las informativas se validan con TGVI.
//!
//! La prevalidación **no presenta nada** ni tiene efectos jurídicos, pero envía los datos a la
//! AEAT: la app la ofrece sólo a petición del usuario.
//!
//! `TODO(VERIFY)`: si exige certificado (ServVali no; para ServValiDos no se indica) y la forma
//! exacta de `AVISOS` y `errores` (cadenas u objetos): se aceptan ambas.

use base64::Engine;
use serde_json::{Value, json};

use crate::http::{AeatClient, AeatError};

pub const SERV_VALIDOS: &str = "https://prewww2.aeat.es/wlpl/PFTW-PICW/ServValiDos";

/// Modelos que admite ServValiDos entre los que genera TaxPain (§5; las informativas 349, 190,
/// 180… van por TGVI). La interfaz tiene su copia en `apps/desktop/src/api.ts` (PREVALIDATABLE).
pub const VALIDATABLE: &[&str] = &["111", "115", "123", "130", "200", "202", "303", "390"];

/// Cuerpo de la petición. `period`: «1T»…«4T», «0A», «1P»…
pub fn request_body(model: &str, year: i32, period: &str, file: &str) -> Value {
    let f01: String = file
        .chars()
        .filter(|c| !matches!(c, '\r' | '\n' | '\t'))
        .collect();
    json!({
        "MODELO": model,
        "EJERCICIO": year.to_string(),
        "PERIODO": period,
        "IDI": "ES",
        "F01": f01,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Validation {
    /// Sin errores: avisos (si los hay) y el borrador oficial en PDF.
    Valid {
        warnings: Vec<String>,
        pdf: Option<Vec<u8>>,
    },
    /// Errores que impedirían presentar.
    Invalid { errors: Vec<String> },
}

/// Busca una clave sin distinguir mayúsculas.
fn get<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.as_object()?
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}

/// Lista de mensajes: cadenas, objetos (se unen sus valores) o un único valor.
fn messages(v: Option<&Value>) -> Vec<String> {
    fn one(v: &Value) -> String {
        match v {
            Value::String(s) => s.clone(),
            Value::Object(o) => o
                .values()
                .map(one)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · "),
            Value::Null => String::new(),
            other => other.to_string(),
        }
    }
    match v {
        None | Some(Value::Null) => vec![],
        Some(Value::Array(a)) => a.iter().map(one).filter(|s| !s.is_empty()).collect(),
        Some(other) => vec![one(other)]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect(),
    }
}

pub fn parse_response(v: &Value) -> Result<Validation, AeatError> {
    let r = get(v, "respuesta")
        .ok_or_else(|| AeatError::InvalidResponse("falta «respuesta»".into()))?;
    let errors = messages(get(r, "errores"));
    if !errors.is_empty() {
        return Ok(Validation::Invalid { errors });
    }
    // El PDF llega como una lista de trozos en base64 (§4.2: `"PDF":["JVBERi…"]`); se acepta
    // también una cadena suelta.
    let b64: String = match get(r, "pdf") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).collect(),
        _ => String::new(),
    };
    let b64: String = b64.chars().filter(|c| !c.is_whitespace()).collect();
    let pdf = if b64.is_empty() {
        None
    } else {
        Some(
            base64::engine::general_purpose::STANDARD
                .decode(&b64)
                .map_err(|e| AeatError::InvalidResponse(format!("PDF en base64: {e}")))?,
        )
    };
    Ok(Validation::Valid {
        warnings: messages(get(r, "avisos")),
        pdf,
    })
}

/// Prevalida el fichero en `url` (normalmente [`SERV_VALIDOS`]).
pub async fn prevalidate(
    client: &AeatClient,
    url: &str,
    model: &str,
    year: i32,
    period: &str,
    file: &str,
) -> Result<Validation, AeatError> {
    let body = request_body(model, year, period, file);
    let res = client.post_json(url, &body).await?;
    parse_response(&res)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Las listas cubren todo lo que TaxPain genera y la AEAT admite (especificación v30, §4 y
    /// §5, `fixtures/aeat/presentacion/`), y la interfaz (`api.ts`) tiene exactamente las mismas.
    #[test]
    fn listas_de_servicios_cubren_lo_que_genera_taxpain() {
        // Modelos con fichero en TaxPain (`taxpain_app::presentados::MODELS` más el 200).
        const TAXPAIN: &[&str] = &[
            "303", "390", "111", "115", "123", "130", "202", "349", "200",
        ];
        // §4 «Listado de modelos soportados» del servicio de presentación.
        const OFICIAL_PRESENTACION: &[&str] = &[
            "005", "030", "036", "100", "111", "115", "117", "123", "124", "126", "128", "130",
            "131", "151", "200", "202", "210", "211", "213", "216", "222", "237", "242", "303",
            "308", "309", "319", "322", "353", "390",
        ];
        // §5 «Listado de modelos soportados» del servicio de validación e impresión.
        const OFICIAL_VALIDACION: &[&str] = &[
            "100", "111", "115", "117", "123", "124", "126", "128", "130", "131", "151", "200",
            "202", "210", "211", "213", "216", "222", "237", "242", "303", "308", "309", "319",
            "322", "353", "390",
        ];
        for m in TAXPAIN {
            assert_eq!(
                PRESENTABLE.contains(m),
                OFICIAL_PRESENTACION.contains(m),
                "presentación directa del {m}"
            );
            assert_eq!(
                VALIDATABLE.contains(m),
                OFICIAL_VALIDACION.contains(m),
                "prevalidación del {m}"
            );
        }
        let api = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../apps/desktop/src/api.ts"
        ))
        .expect("apps/desktop/src/api.ts");
        let ts_list = |name: &str| -> Vec<String> {
            let prefix = format!("export const {name} = [");
            let line = api
                .lines()
                .find(|l| l.starts_with(&prefix))
                .unwrap_or_else(|| panic!("falta {name} en api.ts"));
            line.split('\'')
                .skip(1)
                .step_by(2)
                .map(str::to_owned)
                .collect()
        };
        assert_eq!(ts_list("PREVALIDATABLE"), VALIDATABLE);
        assert_eq!(ts_list("PRESENTABLE"), PRESENTABLE);
    }

    #[test]
    fn cuerpo_sin_saltos_de_linea() {
        let b = request_body("349", 2026, "3T", "1AAA\r\n2BBB\t");
        assert_eq!(b["F01"], "1AAA2BBB");
        assert_eq!(b["EJERCICIO"], "2026");
        assert_eq!(b["IDI"], "ES");
        assert_eq!(b["MODELO"], "349");
    }

    #[test]
    fn respuesta_valida_con_avisos_y_pdf() {
        let v = json!({"RESPUESTA": {"AVISOS": ["Revise la casilla 28", {"codigo": "W1", "texto": "Otro"}], "PDF": "JVBERi0="}});
        assert_eq!(
            parse_response(&v).ok(),
            Some(Validation::Valid {
                warnings: vec!["Revise la casilla 28".into(), "W1 · Otro".into()],
                pdf: Some(b"%PDF-".to_vec()),
            })
        );
    }

    #[test]
    fn pdf_en_lista_como_en_la_especificacion() {
        // Ejemplo de la especificación v30, §4.2 (con un PDF real mínimo en dos trozos).
        let v = json!({"RESPUESTA": {"AVISOS": ["20932 - VERIFIQUE EL CONTENIDO DE LAS CLAVES 00634 (PÁG. 42)"], "PDF": ["JVBE", "Ri0x"]}});
        let Ok(Validation::Valid { warnings, pdf }) = parse_response(&v) else {
            panic!("debería ser válida");
        };
        assert_eq!(warnings.len(), 1);
        assert_eq!(pdf.as_deref(), Some(&b"%PDF-1"[..]));
    }

    #[test]
    fn respuesta_con_errores() {
        let v = json!({"respuesta": {"errores": ["NIF no censado", "Casilla 71 incorrecta"]}});
        assert_eq!(
            parse_response(&v).ok(),
            Some(Validation::Invalid {
                errors: vec!["NIF no censado".into(), "Casilla 71 incorrecta".into()]
            })
        );
        assert!(parse_response(&json!({"otra": 1})).is_err());
        assert!(parse_response(&json!({"respuesta": {"pdf": "%%%"}})).is_err());
    }
}

// ---------- Presentación directa (PresBasicaDos) ----------
//
// Especificación v30 §1: POST JSON {MODELO, EJERCICIO, PERIODO, NRC, IDI, F01,
// FIR="FirmaBasica", FIRNIF, FIRNOMBRE} con autenticación mTLS («firma no criptográfica»).
// FIRNIF debe ser el NIF del titular del certificado. La respuesta correcta trae el CSV y la URL
// del PDF justificante (que es el justificante legal); la de error, `respuesta.errores`.
// «No es posible anular declaraciones ya grabadas»: la app sólo llama aquí tras la conformidad
// expresa del usuario con los datos mostrados (§1.4, «Obtención de datos y autorización»).

pub const PRES_PRODUCCION: &str =
    "https://www1.agenciatributaria.gob.es/wlpl/PFTW-PICW/PresBasicaDos";
pub const PRES_PRUEBAS: &str = "https://prewww1.aeat.es/wlpl/PFTW-PICW/PresBasicaDos";

/// Modelos que admite la presentación directa entre los que genera TaxPain (§4). La interfaz
/// tiene su copia en `apps/desktop/src/api.ts` (PRESENTABLE).
pub const PRESENTABLE: &[&str] = &["111", "115", "123", "130", "200", "202", "303", "390"];

/// Destino de la presentación. Sólo se construye con los métodos: no hay producción por
/// accidente.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Destination {
    url: String,
    production: bool,
}

impl Destination {
    /// Entorno de pruebas para externos (días laborables de 8:00 a 15:00; el justificante, si
    /// existe, lleva marca de agua).
    pub fn testing() -> Self {
        Destination {
            url: PRES_PRUEBAS.into(),
            production: false,
        }
    }

    /// Producción: rechazado en builds de desarrollo y sin la confirmación del usuario.
    pub fn production(confirmed_by_user: bool) -> Result<Self, AeatError> {
        if cfg!(debug_assertions) {
            return Err(AeatError::ProductionBlocked("build de desarrollo"));
        }
        if !confirmed_by_user {
            return Err(AeatError::ProductionBlocked(
                "falta la confirmación del usuario",
            ));
        }
        Ok(Destination {
            url: PRES_PRODUCCION.into(),
            production: true,
        })
    }

    /// Servidor simulado (tests). Sólo con la feature `mock`: la app no lo lleva.
    #[cfg(any(test, feature = "mock"))]
    pub fn mock(url: &str) -> Self {
        Destination {
            url: url.into(),
            production: false,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn is_production(&self) -> bool {
        self.production
    }
}

/// Datos de una presentación.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Filing<'a> {
    pub model: &'a str,
    pub year: i32,
    pub period: &'a str,
    /// NRC del pago, sólo si la forma de pago es «I» (ingreso).
    pub nrc: Option<&'a str>,
    pub file: &'a str,
    /// NIF y nombre del titular del certificado (el presentador).
    pub presenter_nif: &'a str,
    pub presenter_name: &'a str,
}

pub fn filing_body(f: &Filing<'_>) -> Value {
    let mut b = request_body(f.model, f.year, f.period, f.file);
    if let Some(o) = b.as_object_mut() {
        o.insert("NRC".into(), json!(f.nrc.unwrap_or("")));
        o.insert("FIR".into(), json!("FirmaBasica"));
        o.insert("FIRNIF".into(), json!(f.presenter_nif));
        o.insert("FIRNOMBRE".into(), json!(f.presenter_name));
    }
    b
}

/// Justificante de una presentación aceptada (atributos de §1.3).
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Receipt {
    /// Código Seguro de Verificación (16).
    pub csv: String,
    pub date: String,
    pub time: String,
    pub file_number: String,
    /// Número de justificante (13).
    pub receipt_number: String,
    pub declarant_nif: String,
    pub payment_method: String,
    pub amount: String,
    pub nrc: String,
    pub pdf_url: String,
    pub warnings: Vec<String>,
    pub notices: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FilingOutcome {
    Accepted(Box<Receipt>),
    Rejected { errors: Vec<String> },
}

/// Busca una clave ignorando mayúsculas y espacios sobrantes (el ejemplo oficial trae
/// `"CodigoSeguroVerificacion "` con un espacio final).
fn field(v: &Value, key: &str) -> String {
    v.as_object()
        .and_then(|o| {
            o.iter()
                .find(|(k, _)| k.trim().eq_ignore_ascii_case(key))
                .map(|(_, v)| v)
        })
        .map(|v| match v {
            Value::String(s) => s.trim().to_owned(),
            Value::Null => String::new(),
            other => other.to_string(),
        })
        .unwrap_or_default()
}

pub fn parse_filing(v: &Value) -> Result<FilingOutcome, AeatError> {
    let r = get(v, "respuesta")
        .ok_or_else(|| AeatError::InvalidResponse("falta «respuesta»".into()))?;
    let errors = messages(get(r, "errores"));
    if !errors.is_empty() {
        return Ok(FilingOutcome::Rejected { errors });
    }
    let c = get(r, "correcta")
        .ok_or_else(|| AeatError::InvalidResponse("ni «correcta» ni «errores»".into()))?;
    let receipt = Receipt {
        csv: field(c, "CodigoSeguroVerificacion"),
        date: field(c, "Fecha"),
        time: field(c, "Hora"),
        file_number: field(c, "Expediente"),
        receipt_number: field(c, "Justificante"),
        declarant_nif: field(c, "NIFDeclarante"),
        payment_method: field(c, "FormaPago"),
        amount: field(c, "ImporteAIngresar"),
        nrc: field(c, "NRCPago"),
        pdf_url: field(c, "urlPdf"),
        warnings: messages(get(c, "advertencias")),
        notices: messages(get(c, "avisos")),
    };
    // «La respuesta correcta siempre contendrá el CSV y la URL del PDF» (§1.3).
    if receipt.csv.is_empty() || receipt.pdf_url.is_empty() {
        return Err(AeatError::InvalidResponse(
            "respuesta correcta sin CSV o sin URL del justificante".into(),
        ));
    }
    Ok(FilingOutcome::Accepted(Box::new(receipt)))
}

/// Presenta la declaración. **Sólo** debe llamarse tras la conformidad expresa del usuario.
pub async fn file_return(
    client: &AeatClient,
    dest: &Destination,
    f: &Filing<'_>,
) -> Result<FilingOutcome, AeatError> {
    let res = client.post_json(dest.url(), &filing_body(f)).await?;
    parse_filing(&res)
}

#[cfg(test)]
mod filing_tests {
    use super::*;

    fn filing() -> Filing<'static> {
        Filing {
            model: "303",
            year: 2026,
            period: "3T",
            nrc: None,
            file: "<T303…>\r\n",
            presenter_nif: "12345678Z",
            presenter_name: "ESPAÑOLA ESPAÑOLA CARMEN",
        }
    }

    #[test]
    fn cuerpo_con_firma_basica() {
        let b = filing_body(&filing());
        assert_eq!(b["FIR"], "FirmaBasica");
        assert_eq!(b["FIRNIF"], "12345678Z");
        assert_eq!(b["NRC"], "");
        assert_eq!(b["F01"], "<T303…>");
    }

    #[test]
    fn respuesta_correcta_del_ejemplo_oficial() {
        let v = json!({"respuesta": {"correcta": {
            "CodigoSeguroVerificacion ": "AAAAAAAAAAAAAAAA",
            "Fecha": "2020-09-03", "Hora": "14:04", "Expediente": "2020111111111111",
            "NIFDeclarante": "88888888", "Modelo": "111", "Ejercicio": "2020", "Periodo": "01",
            "Justificante": "9999999999999", "Idioma": "ES",
            "urlPdf": "https://www2.agenciatributaria.gob.es/wlpl/x",
            "avisos": ["20932 - Verifique el contenido de las claves 00634 (pág. 42)"],
            "advertencias": ["16412 - No puede aplicarse la clave [00583]"]
        }}});
        let Ok(FilingOutcome::Accepted(r)) = parse_filing(&v) else {
            panic!("debería aceptarse");
        };
        assert_eq!(r.csv, "AAAAAAAAAAAAAAAA");
        assert_eq!(r.receipt_number, "9999999999999");
        assert_eq!(r.notices.len(), 1);
        assert_eq!(r.warnings.len(), 1);
    }

    #[test]
    fn errores_y_respuestas_incompletas() {
        let v = json!({"respuesta": {"errores": ["E00 - Error. Código de error: E00000000000C"]}});
        assert!(matches!(
            parse_filing(&v),
            Ok(FilingOutcome::Rejected { .. })
        ));
        let sin_csv = json!({"respuesta": {"correcta": {"urlPdf": "x"}}});
        assert!(parse_filing(&sin_csv).is_err());
    }

    #[test]
    fn produccion_bloqueada_en_desarrollo() {
        assert!(Destination::testing().url().contains("prewww1.aeat.es"));
        if cfg!(debug_assertions) {
            assert!(Destination::production(true).is_err());
        }
        assert!(Destination::production(false).is_err());
    }
}

// ---------- Consulta de declaraciones presentadas (ConsultaExt) ----------
//
// Especificación v30 §2: POST form-urlencoded con NIF, ANR, MOD, EJF, PER, FED, FEH, HOD, HOH
// (en ese orden; sólo los valores codificados) a ConsultaExt, con certificado. Respuesta XML
// según `consultas.xsd`: hasta 100 `respuestaCorrecta` o un `error`. Sólo lectura: no tiene
// efectos. Sólo hay URL de producción.

pub const CONSULTA_PRODUCCION: &str =
    "https://www1.agenciatributaria.gob.es/wlpl/SCEJ-MANT/ConsultaExt";

/// Una declaración presentada según la AEAT.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Presented {
    pub year: String,
    pub model: String,
    pub period: String,
    pub nif: String,
    pub csv: String,
    pub file_number: String,
    pub receipt_number: String,
    /// Justificante de la declaración anterior (complementarias/rectificativas).
    pub previous_receipt: Option<String>,
    /// `xs:dateTime` tal como llega.
    pub presented_at: String,
}

pub fn query_form(nif: &str, name: &str, model: &str, year: i32, period: Option<&str>) -> String {
    let mut s = url::form_urlencoded::Serializer::new(String::new());
    s.append_pair("NIF", nif)
        .append_pair("ANR", name)
        .append_pair("MOD", model)
        .append_pair("EJF", &year.to_string())
        .append_pair("PER", period.unwrap_or(""))
        .append_pair("FED", "")
        .append_pair("FEH", "")
        .append_pair("HOD", "")
        .append_pair("HOH", "");
    s.finish()
}

/// Interpreta la respuesta XML. `Err` con la descripción si la AEAT devuelve `error`.
pub fn parse_presented(xml: &str) -> Result<Result<Vec<Presented>, String>, AeatError> {
    use quick_xml::Reader;
    use quick_xml::events::Event;

    let mut reader = Reader::from_str(xml);
    let mut out = Vec::new();
    let mut current: Option<Presented> = None;
    let mut text = String::new();
    let mut error: Option<String> = None;
    // Sin la raíz `servicioConsultasDirectas` no es una respuesta del servicio (página de error,
    // SOAP fault…): nunca se interpreta como «no hay nada presentado».
    let mut root = false;
    loop {
        match reader
            .read_event()
            .map_err(|e| AeatError::InvalidResponse(format!("XML: {e}")))?
        {
            Event::Start(e) => {
                if e.local_name().as_ref() == "servicioConsultasDirectas" {
                    root = true;
                }
                if e.local_name().as_ref() == "respuestaCorrecta" {
                    current = Some(Presented::default());
                }
                text.clear();
            }
            Event::Text(t) => text.push_str(&t.xml10_content()),
            Event::GeneralRef(r) => {
                text.push_str(match r.as_ref() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => "",
                });
            }
            Event::End(e) => {
                let name = e.local_name().as_ref().to_owned();
                let value = text.trim().to_owned();
                if let Some(p) = current.as_mut() {
                    match name.as_str() {
                        "ejercicio" => p.year = value,
                        "modelo" => p.model = value,
                        "periodo" => p.period = value,
                        "nif" => p.nif = value,
                        "csv" => p.csv = value,
                        "expediente" => p.file_number = value,
                        "justificante" => p.receipt_number = value,
                        "just_anterior" => p.previous_receipt = Some(value),
                        "fechaYHoraPresentacion" => p.presented_at = value,
                        "respuestaCorrecta" => out.extend(current.take()),
                        _ => {}
                    }
                } else if name == "descripcionError" {
                    error = Some(value);
                }
                text.clear();
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !root {
        return Err(AeatError::InvalidResponse(
            "la respuesta no es del servicio de consultas".into(),
        ));
    }
    Ok(match error {
        Some(e) => Err(e),
        None => Ok(out),
    })
}

/// Texto con el que ConsultaExt dice que no hay declaraciones de ese modelo y año (comprobado
/// el 27/10/2026 con una consulta real: modelo 130, ejercicio 2025, sin declaraciones). Llega
/// como `<error><descripcionError>`; el XSD no da ningún código que lo distinga de un error.
pub const NO_DECLARATIONS: &str = "Usted no ha presentado la declaración solicitada";

/// Minúsculas, sin tildes, sin el punto final y con los espacios recortados y juntos.
fn normalized(s: &str) -> String {
    let s: String = s
        .trim()
        .trim_end_matches('.')
        .chars()
        .map(|c| match c {
            'á' | 'Á' | 'à' | 'À' => 'a',
            'é' | 'É' | 'è' | 'È' => 'e',
            'í' | 'Í' | 'ì' | 'Ì' => 'i',
            'ó' | 'Ó' | 'ò' | 'Ò' => 'o',
            'ú' | 'Ú' | 'ù' | 'Ù' | 'ü' | 'Ü' => 'u',
            c => c.to_ascii_lowercase(),
        })
        .collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// ¿Dice el error de ConsultaExt que no hay declaraciones de ese modelo y año? Sólo con el texto
/// exacto de la AEAT ([`NO_DECLARATIONS`], sin distinguir mayúsculas ni tildes): cualquier otro
/// es un error de verdad y no se dice que falte nada.
pub fn no_declarations(msg: &str) -> bool {
    normalized(msg) == normalized(NO_DECLARATIONS)
}

/// Texto real de ConsultaExt para un modelo que el servicio no permite consultar (visto con el
/// 721 el 28/10/2026: «El modelo y el ejercicio seleccionados no están disponibles para esta
/// consulta», seguido a veces del modelo y el año sin separar).
pub const NOT_AVAILABLE: &str =
    "El modelo y el ejercicio seleccionados no están disponibles para esta consulta";

/// ¿Dice el error que el servicio no permite consultar ese modelo y ejercicio? No es un fallo ni
/// quiere decir que no se presentara: hay que mirarlo en la Sede.
pub fn not_available(msg: &str) -> bool {
    normalized(msg).starts_with(&normalized(NOT_AVAILABLE))
}

/// Consulta las declaraciones presentadas de un modelo y ejercicio (producción, sólo lectura).
pub async fn query_presented(
    client: &AeatClient,
    url: &str,
    nif: &str,
    name: &str,
    model: &str,
    year: i32,
) -> Result<Result<Vec<Presented>, String>, AeatError> {
    let body = client
        .post_form(url, &query_form(nif, name, model, year, None))
        .await?;
    parse_presented(&body)
}

#[cfg(test)]
mod query_tests {
    use super::*;

    #[test]
    fn sin_declaraciones_y_errores_de_verdad() {
        for t in [
            "Usted no ha presentado la declaración solicitada",
            "  usted no ha presentado la declaracion solicitada.  ",
            "USTED NO HA PRESENTADO LA DECLARACIÓN  SOLICITADA",
        ] {
            assert!(no_declarations(t), "{t}");
        }
        for t in [
            "No existen declaraciones para los datos indicados",
            "No existe el modelo 232 en este servicio",
            "Petición sin datos obligatorios",
            "No hay servicio en este momento, inténtelo más tarde",
            "No se ha encontrado el NIF",
            "Certificado no autorizado para el NIF",
            "Usted no ha presentado la declaración solicitada del NIF de otro",
            "",
        ] {
            assert!(!no_declarations(t), "{t}");
        }
    }

    #[test]
    fn modelo_que_el_servicio_no_consulta() {
        assert!(not_available(
            "El modelo y el ejercicio seleccionados no están disponibles para esta consulta721, 2026"
        ));
        assert!(not_available(
            "el modelo y el ejercicio seleccionados no estan disponibles para esta consulta"
        ));
        assert!(!not_available(
            "Usted no ha presentado la declaración solicitada"
        ));
        assert!(!not_available("El modelo no existe"));
    }

    #[test]
    fn formulario_en_el_orden_de_la_especificacion() {
        let f = query_form(
            "12345678Z",
            "ESPAÑOLA ESPAÑOLA CARMEN",
            "303",
            2026,
            Some("3T"),
        );
        assert_eq!(
            f,
            "NIF=12345678Z&ANR=ESPA%C3%91OLA+ESPA%C3%91OLA+CARMEN&MOD=303&EJF=2026&PER=3T&FED=&FEH=&HOD=&HOH="
        );
    }

    #[test]
    fn respuesta_con_dos_declaraciones_y_con_error() {
        let ok = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/aeat/presentacion/consulta-ejemplo.xml"
        ))
        .unwrap_or_default();
        let list = parse_presented(&ok)
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].csv, "ABCDEFGHIJKLMNOP");
        assert_eq!(list[0].previous_receipt, None);
        assert_eq!(list[1].previous_receipt.as_deref(), Some("3030000000001"));
        let err = "<servicioConsultasDirectas><error><descripcionError>Sin datos &amp; más</descripcionError></error></servicioConsultasDirectas>";
        assert_eq!(
            parse_presented(err).ok(),
            Some(Err("Sin datos & más".into()))
        );
        // Cualquier otra cosa no es «nada presentado».
        for other in ["", "<html><body>Error</body></html>", "<Fault/>"] {
            assert!(parse_presented(other).is_err(), "{other}");
        }
    }
}
