//! Herramientas del asistente: una **lista cerrada de consultas de sólo lectura**. Si el modelo
//! pide otra cosa, la consulta se corta (no se le contesta «no existe»: se para). Ninguna
//! herramienta escribe, presenta, envía ni toca el certificado; las de datos del usuario las
//! implementa la app ([`LocalTools`]) y las del BOE este crate.
//!
//! - La entrada de una herramienta local se **restaura** (marcadores → datos) sólo en el Mac,
//!   para poder buscar; la salida se **anonimiza** antes de volver al modelo.
//! - Las del BOE no reciben datos del usuario: se rechaza una consulta con marcadores, que el
//!   anonimizador de la conversación cambiaría (con sus valores conocidos y lo ya tapado) o con
//!   cinco cifras seguidas o más fuera de un identificador de norma («BOE-A-2014-12328»).
//! - Las citas de la respuesta («…») se comprueban contra los artículos leídos; las que no
//!   aparecen se señalan ([`Answered::unverified`]).
//!
//! Ideas de `packages/agent/src/tools/{index,names}.ts` de Spain Assist y de
//! `packages/ai/src/grounding.ts` de reforma-digital (MIT); código propio.

use serde_json::{Value, json};

use crate::error::AssistantError;
use crate::pii::Scrubber;
use crate::provider::{Client, Provider};
use crate::{Outgoing, Role};

/// Herramienta: nombre, descripción y esquema JSON de la entrada.
#[derive(Clone, Copy, Debug)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: &'static str,
}

/// Consultas a los datos del usuario (las implementa la app, de sólo lectura).
pub const LOCAL_TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "plazos_proximos",
        description: "Próximos plazos de presentación del usuario (modelo, periodo y fechas).",
        schema: r#"{"type":"object","properties":{}}"#,
    },
    ToolSpec {
        name: "borrador_modelo",
        description: "Borrador de un modelo de la AEAT calculado con los datos del usuario: casillas, resultado y avisos. No presenta nada.",
        schema: r#"{"type":"object","properties":{"modelo":{"type":"string","description":"303, 130, 111, 115, 349, 390, 202, 347, 200…"},"ejercicio":{"type":"integer"},"trimestre":{"type":"integer","description":"1-4; 0 en los anuales"}},"required":["modelo","ejercicio"]}"#,
    },
    ToolSpec {
        name: "buscar_facturas",
        description: "Facturas emitidas o recibidas del usuario (como mucho 20) que contienen un texto en el número o el nombre, en un ejercicio.",
        schema: r#"{"type":"object","properties":{"tipo":{"type":"string","enum":["emitidas","recibidas"]},"ejercicio":{"type":"integer"},"texto":{"type":"string"}},"required":["tipo","ejercicio"]}"#,
    },
    ToolSpec {
        name: "saldo_cuenta",
        description: "Saldo de una cuenta o grupo del plan contable (p. ej. 572, 4300001, 62) en un ejercicio.",
        schema: r#"{"type":"object","properties":{"cuenta":{"type":"string"},"ejercicio":{"type":"integer"}},"required":["cuenta","ejercicio"]}"#,
    },
];

/// Consultas al BOE (legislación consolidada vigente, datos públicos).
pub const BOE_TOOLS: &[ToolSpec] = &[
    ToolSpec {
        name: "buscar_ley",
        description: "Busca normas vigentes del BOE por palabras del título (p. ej. «Impuesto sobre Sociedades»). Devuelve identificadores BOE-A-… y títulos. No incluyas datos del usuario.",
        schema: r#"{"type":"object","properties":{"consulta":{"type":"string"}},"required":["consulta"]}"#,
    },
    ToolSpec {
        name: "leer_articulo",
        description: "Texto vigente de un artículo de una norma del BOE (identificador BOE-A-… y artículo, p. ej. «91» u «8 bis»). Cita literalmente entre comillas «…» sólo lo que leas aquí.",
        schema: r#"{"type":"object","properties":{"id":{"type":"string"},"articulo":{"type":"string"}},"required":["id","articulo"]}"#,
    },
];

/// Todas las herramientas que se ofrecen al modelo.
pub fn all_tools() -> impl Iterator<Item = &'static ToolSpec> {
    LOCAL_TOOLS.iter().chain(BOE_TOOLS)
}

/// Herramientas de datos del usuario, de sólo lectura (implementadas por la app).
pub trait LocalTools: Send + Sync {
    /// Resultado en texto (o un error que se le devuelve al modelo como texto).
    fn call(&self, name: &str, input: &Value) -> Result<String, String>;
}

/// Llamada a una herramienta pedida por el modelo.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

/// Un turno de la conversación con herramientas.
#[derive(Clone, Debug, PartialEq)]
pub enum Turn {
    Text { role: Role, text: String },
    Calls { text: String, calls: Vec<ToolCall> },
    Results(Vec<(String, String)>),
}

impl Provider {
    /// Cuerpo de la petición con herramientas y turnos.
    pub fn tools_body(&self, system: &str, turns: &[Turn], max_tokens: u32) -> Value {
        match self {
            Provider::Anthropic { model } => {
                let tools: Vec<Value> = all_tools()
                    .map(|t| {
                        json!({ "name": t.name, "description": t.description,
                                "input_schema": serde_json::from_str::<Value>(t.schema).unwrap_or(json!({})) })
                    })
                    .collect();
                let messages: Vec<Value> = turns
                    .iter()
                    .map(|t| match t {
                        Turn::Text { role, text } => json!({ "role": role, "content": text }),
                        Turn::Calls { text, calls } => {
                            let mut blocks = Vec::new();
                            if !text.is_empty() {
                                blocks.push(json!({ "type": "text", "text": text }));
                            }
                            blocks.extend(calls.iter().map(|c| {
                                json!({ "type": "tool_use", "id": c.id, "name": c.name, "input": c.input })
                            }));
                            json!({ "role": "assistant", "content": blocks })
                        }
                        Turn::Results(rs) => json!({
                            "role": "user",
                            "content": rs.iter().map(|(id, content)| json!({
                                "type": "tool_result", "tool_use_id": id, "content": content
                            })).collect::<Vec<_>>()
                        }),
                    })
                    .collect();
                json!({ "model": model, "max_tokens": max_tokens, "system": system,
                        "tools": tools, "messages": messages })
            }
            Provider::OpenAiCompatible { model, .. } => {
                let tools: Vec<Value> = all_tools()
                    .map(|t| {
                        json!({ "type": "function", "function": {
                            "name": t.name, "description": t.description,
                            "parameters": serde_json::from_str::<Value>(t.schema).unwrap_or(json!({})) } })
                    })
                    .collect();
                let mut messages = vec![json!({ "role": "system", "content": system })];
                for t in turns {
                    match t {
                        Turn::Text { role, text } => {
                            messages.push(json!({ "role": role, "content": text }));
                        }
                        Turn::Calls { text, calls } => messages.push(json!({
                            "role": "assistant",
                            "content": if text.is_empty() { Value::Null } else { json!(text) },
                            "tool_calls": calls.iter().map(|c| json!({
                                "id": c.id, "type": "function",
                                "function": { "name": c.name, "arguments": c.input.to_string() }
                            })).collect::<Vec<_>>()
                        })),
                        Turn::Results(rs) => {
                            for (id, content) in rs {
                                messages.push(json!({ "role": "tool", "tool_call_id": id, "content": content }));
                            }
                        }
                    }
                }
                json!({ "model": model, "max_tokens": max_tokens, "messages": messages, "tools": tools })
            }
        }
    }

    /// ¿La respuesta se ha cortado por llegar al máximo de longitud (`max_tokens`)?
    pub fn was_cut(&self, v: &Value) -> bool {
        match self {
            Provider::Anthropic { .. } => {
                v.get("stop_reason").and_then(Value::as_str) == Some("max_tokens")
            }
            Provider::OpenAiCompatible { .. } => {
                v.pointer("/choices/0/finish_reason")
                    .and_then(Value::as_str)
                    == Some("length")
            }
        }
    }

    /// Texto y llamadas a herramientas de una respuesta.
    pub fn parse_turn(&self, v: &Value) -> Result<(String, Vec<ToolCall>), AssistantError> {
        if let Some(e) = v.get("error") {
            let msg = e
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("error del proveedor");
            return Err(AssistantError::Provider(msg.to_owned()));
        }
        match self {
            Provider::Anthropic { .. } => {
                let blocks = v
                    .get("content")
                    .and_then(|c| c.as_array())
                    .ok_or_else(|| AssistantError::Provider("respuesta con otro formato".into()))?;
                let mut text = String::new();
                let mut calls = Vec::new();
                for b in blocks {
                    match b.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            text.push_str(b.get("text").and_then(|t| t.as_str()).unwrap_or(""))
                        }
                        Some("tool_use") => calls.push(ToolCall {
                            id: b
                                .get("id")
                                .and_then(|x| x.as_str())
                                .unwrap_or("")
                                .to_owned(),
                            name: b
                                .get("name")
                                .and_then(|x| x.as_str())
                                .unwrap_or("")
                                .to_owned(),
                            input: b.get("input").cloned().unwrap_or(json!({})),
                        }),
                        _ => {}
                    }
                }
                Ok((text, calls))
            }
            Provider::OpenAiCompatible { .. } => {
                let msg = v
                    .pointer("/choices/0/message")
                    .ok_or_else(|| AssistantError::Provider("respuesta con otro formato".into()))?;
                let text = msg
                    .get("content")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_owned();
                let calls = msg
                    .get("tool_calls")
                    .and_then(|c| c.as_array())
                    .map(|a| {
                        a.iter()
                            .map(|c| ToolCall {
                                id: c
                                    .get("id")
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("")
                                    .to_owned(),
                                name: c
                                    .pointer("/function/name")
                                    .and_then(|x| x.as_str())
                                    .unwrap_or("")
                                    .to_owned(),
                                input: c
                                    .pointer("/function/arguments")
                                    .and_then(|x| x.as_str())
                                    .and_then(|s| serde_json::from_str(s).ok())
                                    .unwrap_or(json!({})),
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                Ok((text, calls))
            }
        }
    }
}

/// Respuesta con herramientas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answered {
    pub text: String,
    pub replaced: usize,
    /// Herramientas usadas, en orden («buscar_ley», «saldo_cuenta»…).
    pub used: Vec<String>,
    /// Normas del BOE leídas (identificador y artículo).
    pub sources: Vec<String>,
    /// Citas literales de la respuesta que no aparecen en lo leído.
    pub unverified: Vec<String>,
    /// Lo que se tapó con marcadores en la pregunta, en lo que devolvieron las herramientas y,
    /// por tanto, en la respuesta (ya restaurada): la app lo guarda para taparlo también en las
    /// preguntas siguientes de la conversación, aunque ya no salga en sus datos.
    pub masked: Vec<String>,
}

/// Citas entre «» o “” (de 25 caracteres o más) de un texto.
pub fn quotes(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (open, close) in [('«', '»'), ('“', '”')] {
        let mut rest = text;
        while let Some(a) = rest.find(open) {
            let after = &rest[a + open.len_utf8()..];
            let Some(b) = after.find(close) else { break };
            let q = after[..b].trim();
            if q.chars().count() >= 25 {
                out.push(q.to_owned());
            }
            rest = &after[b + close.len_utf8()..];
        }
    }
    out
}

fn normalize(s: &str) -> String {
    s.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches(|c: char| c == '.' || c == ',' || c == ';')
        .to_owned()
}

/// Citas que no aparecen en ninguno de los textos leídos.
pub fn unverified_quotes(answer: &str, read: &[String]) -> Vec<String> {
    let corpus: Vec<String> = read.iter().map(|r| normalize(r)).collect();
    quotes(answer)
        .into_iter()
        .filter(|q| {
            let n = normalize(q.trim_end_matches('…').trim_end_matches("[…]"));
            !corpus.iter().any(|c| c.contains(&n))
        })
        .collect()
}

/// Rondas máximas de herramientas por pregunta.
pub const MAX_ROUNDS: usize = 6;

/// Lo que se añade a una respuesta que se ha cortado por su longitud.
pub const CUT_NOTE: &str =
    "\n\n(La respuesta se ha cortado por su longitud: pide que siga o que lo resuma.)";

/// El texto final de una respuesta (ya restaurado): con el aviso si se ha cortado y un error si
/// no ha llegado nada.
fn final_text(mut text: String, cut: bool) -> Result<String, AssistantError> {
    if text.trim().is_empty() {
        return Err(AssistantError::Provider(if cut {
            "la respuesta se ha cortado antes de terminar: vuelve a preguntar o pide algo más \
             concreto"
                .into()
        } else {
            "respuesta vacía".into()
        }));
    }
    if cut {
        text.push_str(CUT_NOTE);
    }
    Ok(text)
}

async fn run_boe(
    client: &Client,
    name: &str,
    input: &Value,
    read: &mut Vec<String>,
    sources: &mut Vec<String>,
) -> String {
    let s = |k: &str| {
        input
            .get(k)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_owned()
    };
    match name {
        "buscar_ley" => match client.boe_search(&s("consulta")).await {
            Ok(laws) if laws.is_empty() => {
                "No hay normas vigentes con esas palabras en el título.".into()
            }
            Ok(laws) => laws
                .iter()
                .map(|l| format!("{} · {}", l.id, l.title))
                .collect::<Vec<_>>()
                .join("\n"),
            Err(e) => format!("Error: {e}"),
        },
        _ => {
            let (id, art) = (s("id"), s("articulo"));
            match client.boe_article(&id, &art).await {
                Ok(t) => {
                    read.push(t.clone());
                    sources.push(format!("{id}, artículo {art}"));
                    t
                }
                Err(e) => format!("Error: {e}"),
            }
        }
    }
}

/// Lo que devuelve una herramienta, anonimizado como el resto del envío: lo que ya se ha tapado
/// en la conversación lleva el mismo marcador aunque el detector no lo vea aquí (el modelo busca
/// justo por ese marcador, así que vuelve con el dato), y lo nuevo se tapa con el anonimizador.
fn scrub_result(scrubber: &mut Scrubber, text: &str) -> String {
    let seen = scrubber.scrub_seen(text);
    let scrubbed = scrubber.scrub(&seen);
    scrubber.scrub_seen(&scrubbed)
}

/// Identificadores de diarios oficiales («BOE-A-2014-12328», «DOUE-L-2006-82164»): las únicas
/// cifras largas que puede llevar una consulta al BOE.
fn legal_ids() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(r"(?i)\b[a-z]{2,5}-[a-z]-\d{4}-\d{1,6}\b").expect("patrón fijo")
    })
}

/// Cinco cifras seguidas o más (un número de factura, un importe, parte de un NIF o de una
/// cuenta) fuera de un identificador de diario oficial. Un año, «37/1992», «1619/2012» o
/// «artículo 91» no las tienen.
fn long_number(text: &str) -> bool {
    let rest = legal_ids().replace_all(text, " ");
    let mut run = 0;
    for c in rest.chars() {
        run = if c.is_ascii_digit() { run + 1 } else { 0 };
        if run >= 5 {
            return true;
        }
    }
    false
}

/// Si la entrada de una herramienta del BOE se puede mandar: sin marcadores, sin nada que el
/// anonimizador de la conversación (con sus valores conocidos y lo ya tapado) cambiaría y sin
/// cifras largas que no sean un identificador de norma.
fn boe_input_allowed(conversation: &Scrubber, input: &Value) -> bool {
    if input.to_string().contains('[') {
        return false;
    }
    let mut texts = Vec::new();
    json_texts(input, &mut texts);
    // Sin el detector de nombres: tomaría por nombres los títulos de las normas. Sobre una
    // copia, para no añadir a la conversación marcadores de algo que no se envía.
    let mut scrubber = conversation.clone().without_names();
    texts
        .iter()
        .all(|t| !long_number(t) && conversation.scrub_seen(t) == *t && scrubber.scrub(t) == *t)
}

/// Los textos (claves, cadenas y números) de un JSON, en cualquier nivel.
fn json_texts(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => out.push(s.clone()),
        Value::Number(n) => out.push(n.to_string()),
        Value::Array(a) => a.iter().for_each(|x| json_texts(x, out)),
        Value::Object(o) => o.iter().for_each(|(k, x)| {
            out.push(k.clone());
            json_texts(x, out);
        }),
        Value::Bool(_) | Value::Null => {}
    }
}

/// Pregunta con herramientas: el modelo puede consultar datos (sólo lectura) y el BOE.
pub async fn ask_with_tools(
    client: &Client,
    provider: &Provider,
    api_key: Option<&str>,
    out: &Outgoing,
    local: &dyn LocalTools,
) -> Result<Answered, AssistantError> {
    let mut scrubber: Scrubber = out.scrubber.clone();
    let mut turns: Vec<Turn> = out
        .messages
        .iter()
        .map(|m| Turn::Text {
            role: m.role,
            text: m.content.clone(),
        })
        .collect();
    let mut used = Vec::new();
    let mut sources = Vec::new();
    let mut read: Vec<String> = Vec::new();
    for _ in 0..MAX_ROUNDS {
        let v = client
            .post_json(
                provider,
                api_key,
                &provider.tools_body(&out.system, &turns, 2048),
            )
            .await?;
        let (text, calls) = provider.parse_turn(&v)?;
        if calls.is_empty() {
            let restored = final_text(scrubber.restore(&text), provider.was_cut(&v))?;
            return Ok(Answered {
                unverified: unverified_quotes(&restored, &read),
                text: restored,
                replaced: scrubber.replaced(),
                masked: scrubber.originals(),
                used,
                sources,
            });
        }
        let mut results = Vec::new();
        for c in &calls {
            // Lista cerrada: otra herramienta corta la consulta.
            if !all_tools().any(|t| t.name == c.name) {
                return Err(AssistantError::UnknownTool(c.name.clone()));
            }
            used.push(c.name.clone());
            let result = if BOE_TOOLS.iter().any(|t| t.name == c.name) {
                // Al BOE no van datos del usuario: ni marcadores ni nada que el anonimizador tape.
                if !boe_input_allowed(&scrubber, &c.input) {
                    "Error: la búsqueda en el BOE no puede llevar datos personales ni marcadores."
                        .into()
                } else {
                    run_boe(client, &c.name, &c.input, &mut read, &mut sources).await
                }
            } else {
                // Datos locales: la entrada se restaura sólo aquí, en el Mac.
                let input: Value = serde_json::from_str(&scrubber.restore(&c.input.to_string()))
                    .unwrap_or_else(|_| c.input.clone());
                match local.call(&c.name, &input) {
                    Ok(r) => r,
                    Err(e) => format!("Error: {e}"),
                }
            };
            // Lo que devuelve una herramienta puede traer texto ajeno (el concepto de una
            // factura recibida sale de su PDF): anonimizado y entre las marcas de documento
            // externo, para que el modelo no lo tome por instrucciones.
            results.push((
                c.id.clone(),
                crate::pii::external_document(
                    &format!("resultado de {}", c.name),
                    &scrub_result(&mut scrubber, &result),
                ),
            ));
        }
        turns.push(Turn::Calls { text, calls });
        turns.push(Turn::Results(results));
    }
    Err(AssistantError::Provider(
        "demasiadas consultas para una sola pregunta: reformúlala".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn al_boe_no_van_datos_de_la_conversacion_ni_cifras_largas() {
        // Lo que se ha tapado en la conversación (valores conocidos de la empresa).
        let mut conv =
            Scrubber::with_known(&["Ferretería Inventada Ruiz", "Calle Falsa 123"]).without_names();
        let tapado = conv.scrub("Factura de Ferretería Inventada Ruiz, Calle Falsa 123.");
        assert!(tapado.contains("[DATO_"));
        let ok = |input: Value| boe_input_allowed(&conv, &input);

        // Consultas de normas: sí, también con identificadores, años y números de ley.
        for q in [
            json!({"consulta": "Impuesto sobre Sociedades"}),
            json!({"consulta": "Ley 37/1992 del IVA"}),
            json!({"consulta": "Real Decreto 1619/2012 facturación"}),
            json!({"consulta": "Reglamento (UE) 2016/679"}),
            json!({"consulta": "IVA 2025 artículo 91"}),
            json!({"id": "BOE-A-2014-12328", "articulo": "91"}),
            json!({"id": "boe-a-1992-28740", "articulo": "20 bis"}),
            json!({"consulta": "DOUE-L-2006-82164"}),
        ] {
            assert!(ok(q.clone()), "{q}");
        }
        // Con datos: no.
        for q in [
            // Marcadores.
            json!({"consulta": "IVA de [DATO_1]"}),
            // Lo tapado en la conversación, escrito de otra forma.
            json!({"consulta": "ferreteria inventada ruiz sociedades"}),
            json!({"consulta": "IVA FERRETERÍA INVENTADA RUIZ"}),
            // Un NIF válido aunque no salga en la conversación.
            json!({"consulta": "IVA 12345678Z"}),
            // Cinco cifras seguidas o más: factura, importe, NIF o cuenta a medias.
            json!({"consulta": "factura 20250147"}),
            json!({"consulta": "IVA de 12100,00 euros"}),
            json!({"consulta": "deducción 1234567"}),
            json!({"consulta": "Ley 37/1992", "otro": 123456}),
            json!({"id": "BOE-A-2014-12328", "articulo": "12345"}),
            json!({"id": "BOE-A-2014-1232812", "articulo": "1"}),
        ] {
            assert!(!ok(q.clone()), "{q}");
        }
        // La comprobación no añade marcadores a la conversación.
        assert_eq!(conv.replaced(), 2);
        assert!(long_number("abc 12345"));
        assert!(!long_number("1234 5678"));
    }

    #[test]
    fn citas_comprobadas() {
        let read = vec![
            "Artículo 91. Uno. Se aplicará el tipo del 10 por 100 a las operaciones siguientes"
                .to_string(),
        ];
        let a = "Según el art. 91 LIVA, «se aplicará el tipo del 10 por 100 a las operaciones» y \
                 «el tipo superreducido del 4 por 100 se aplica al pan común».";
        assert_eq!(
            unverified_quotes(a, &read),
            vec!["el tipo superreducido del 4 por 100 se aplica al pan común".to_string()]
        );
        assert!(quotes("«corta»").is_empty());
    }

    #[test]
    fn respuesta_cortada_se_avisa() {
        let anth = Provider::Anthropic { model: "m".into() };
        let oai = Provider::OpenAiCompatible {
            base_url: "http://localhost:1/v1".into(),
            model: "m".into(),
        };
        assert!(anth.was_cut(&json!({"content":[],"stop_reason":"max_tokens"})));
        assert!(!anth.was_cut(&json!({"content":[],"stop_reason":"end_turn"})));
        assert!(oai.was_cut(&json!({"choices":[{"message":{},"finish_reason":"length"}]})));
        assert!(!oai.was_cut(&json!({"choices":[{"message":{},"finish_reason":"stop"}]})));
        // Cada proveedor mira su campo, no el del otro.
        assert!(!anth.was_cut(&json!({"choices":[{"finish_reason":"length"}]})));
        assert!(!oai.was_cut(&json!({"stop_reason":"max_tokens"})));

        // Con texto: se avisa al final; sin texto: error que dice qué hacer.
        assert_eq!(
            final_text("Primero, dota la reserva".into(), true),
            Ok(format!("Primero, dota la reserva{CUT_NOTE}"))
        );
        assert_eq!(final_text("Hecho.".into(), false), Ok("Hecho.".into()));
        let Err(AssistantError::Provider(e)) = final_text("  ".into(), true) else {
            panic!("una respuesta cortada y vacía es un error");
        };
        assert!(e.contains("se ha cortado antes de terminar"), "{e}");
        assert_eq!(
            final_text(String::new(), false),
            Err(AssistantError::Provider("respuesta vacía".into()))
        );
    }

    #[test]
    fn formatos_de_herramientas() {
        let anth = Provider::Anthropic { model: "m".into() };
        let turns = vec![
            Turn::Text {
                role: Role::User,
                text: "¿Saldo?".into(),
            },
            Turn::Calls {
                text: String::new(),
                calls: vec![ToolCall {
                    id: "t1".into(),
                    name: "saldo_cuenta".into(),
                    input: json!({"cuenta":"572","ejercicio":2026}),
                }],
            },
            Turn::Results(vec![("t1".into(), "1.000,00 €".into())]),
        ];
        let b = anth.tools_body("s", &turns, 100);
        assert_eq!(
            b["tools"].as_array().map(Vec::len),
            Some(LOCAL_TOOLS.len() + BOE_TOOLS.len())
        );
        assert_eq!(b["messages"][1]["content"][0]["type"], "tool_use");
        assert_eq!(b["messages"][2]["content"][0]["tool_use_id"], "t1");
        let (text, calls) = anth
            .parse_turn(&json!({"content":[{"type":"text","text":"Miro."},{"type":"tool_use","id":"x","name":"buscar_ley","input":{"consulta":"IVA"}}]}))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            (text.as_str(), calls[0].name.as_str()),
            ("Miro.", "buscar_ley")
        );

        let oai = Provider::OpenAiCompatible {
            base_url: "http://localhost:1/v1".into(),
            model: "m".into(),
        };
        let b = oai.tools_body("s", &turns, 100);
        assert_eq!(
            b["messages"][2]["tool_calls"][0]["function"]["name"],
            "saldo_cuenta"
        );
        assert_eq!(b["messages"][3]["role"], "tool");
        let (_, calls) = oai
            .parse_turn(&json!({"choices":[{"message":{"content":null,"tool_calls":[{"id":"c","type":"function","function":{"name":"plazos_proximos","arguments":"{}"}}]}}]}))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(calls[0].name, "plazos_proximos");
    }
}
