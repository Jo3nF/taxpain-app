//! Consulta de la legislación consolidada del BOE (API de datos abiertos) para el asistente:
//! buscar normas vigentes por palabras del título y leer un artículo. Sólo habla con
//! `www.boe.es` y sólo con texto público: nunca se le pasan datos del usuario (el asistente
//! rechaza consultas con marcadores o datos personales antes de llegar aquí).
//!
//! Idea de `packages/agent/src/research.ts` de reforma-digital (MIT); código propio.

use regex::Regex;
use std::sync::OnceLock;

use crate::error::AssistantError;
use crate::provider::Client;

const BASE: &str = "https://www.boe.es/datosabiertos/api/legislacion-consolidada";
/// Máximo de texto de un artículo que se devuelve al modelo.
const MAX_ARTICLE: usize = 6000;

/// Norma encontrada.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Law {
    pub id: String,
    pub title: String,
}

fn valid_id(id: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^BOE-A-\d{4}-\d{1,6}$").expect("patrón fijo"))
        .is_match(id)
}

/// Palabras de búsqueda: sólo letras, cifras y espacios (sin operadores de la API).
fn words(query: &str) -> String {
    query
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .take(8)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Bloque del índice de un artículo: «91» → `a91`, «8 bis» → `a8-2`, «20 ter» → `a20-3`.
pub fn article_block(article: &str) -> Option<String> {
    let a = article.to_lowercase();
    let a = a
        .trim()
        .trim_start_matches("artículo")
        .trim_start_matches("art.")
        .trim();
    let digits: String = a.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || digits.len() > 4 {
        return None;
    }
    let rest = a[digits.len()..].trim();
    let suffix = match rest {
        "" => "",
        "bis" => "-2",
        "ter" => "-3",
        "quater" | "quáter" => "-4",
        "quinquies" => "-5",
        "sexies" => "-6",
        _ => return None,
    };
    Some(format!("a{digits}{suffix}"))
}

/// Texto plano de la última versión de un bloque XML del BOE.
pub fn block_text(xml: &str) -> Option<String> {
    let start = xml.rfind("<version")?;
    let end = xml[start..].find("</version>").map(|e| start + e)?;
    let body = &xml[start..end];
    static TAG: OnceLock<Regex> = OnceLock::new();
    let tag = TAG.get_or_init(|| Regex::new(r"</p>|<[^>]+>").expect("patrón fijo"));
    let text = tag.replace_all(
        body,
        |c: &regex::Captures<'_>| {
            if &c[0] == "</p>" { "\n" } else { "" }
        },
    );
    let text = text
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&");
    let text: String = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

impl Client {
    /// Normas consolidadas vigentes cuyo título contiene las palabras de `query`.
    pub async fn boe_search(&self, query: &str) -> Result<Vec<Law>, AssistantError> {
        let q = words(query);
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let filter = serde_json::json!({
            "query": { "query_string": { "query": format!("titulo:({q}) and vigencia_agotada:N") } }
        });
        let mut url = url::Url::parse(BASE).map_err(|_| AssistantError::InvalidUrl)?;
        url.query_pairs_mut()
            .append_pair("query", &filter.to_string())
            .append_pair("limit", "8");
        let v: serde_json::Value = self.get_boe(url, "application/json").await?.parse_json()?;
        Ok(v.get("data")
            .and_then(|d| d.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|n| {
                        Some(Law {
                            id: n.get("identificador")?.as_str()?.to_owned(),
                            title: n.get("titulo")?.as_str()?.to_owned(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    /// Texto vigente del artículo `article` («91», «8 bis») de la norma `id`.
    pub async fn boe_article(&self, id: &str, article: &str) -> Result<String, AssistantError> {
        if !valid_id(id) {
            return Err(AssistantError::Provider(format!(
                "identificador del BOE no válido: {id}"
            )));
        }
        let block = article_block(article)
            .ok_or_else(|| AssistantError::Provider(format!("artículo no válido: {article}")))?;
        let url = url::Url::parse(&format!("{BASE}/id/{id}/texto/bloque/{block}"))
            .map_err(|_| AssistantError::InvalidUrl)?;
        let xml = self.get_boe(url, "application/xml").await?.0;
        let mut text = block_text(&xml)
            .ok_or_else(|| AssistantError::Provider("el BOE no devolvió ese artículo".into()))?;
        if text.chars().count() > MAX_ARTICLE {
            text = text.chars().take(MAX_ARTICLE).collect::<String>() + "\n[…]";
        }
        Ok(text)
    }

    async fn get_boe(&self, url: url::Url, accept: &str) -> Result<Body, AssistantError> {
        if url.scheme() != "https" || url.host_str() != Some("www.boe.es") {
            return Err(AssistantError::HostNotAllowed(
                url.host_str().unwrap_or("").to_owned(),
            ));
        }
        let res = self
            .http()
            .get(url)
            .header("Accept", accept)
            .send()
            .await
            .map_err(|e| AssistantError::Network(e.without_url().to_string()))?;
        let status = res.status();
        if !status.is_success() {
            return Err(AssistantError::Provider(format!("BOE: HTTP {status}")));
        }
        let body = crate::provider::read_limited(res, crate::provider::limits::BOE)
            .await
            .map_err(|e| match e {
                crate::provider::Limited::TooLarge => AssistantError::TooLarge,
                crate::provider::Limited::Http(e) => {
                    AssistantError::Network(e.without_url().to_string())
                }
            })?;
        Ok(Body(String::from_utf8_lossy(&body).into_owned()))
    }
}

struct Body(String);

impl Body {
    fn parse_json(self) -> Result<serde_json::Value, AssistantError> {
        serde_json::from_str(&self.0)
            .map_err(|_| AssistantError::Provider("respuesta del BOE no válida".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn articulos_y_textos() {
        assert_eq!(article_block("91").as_deref(), Some("a91"));
        assert_eq!(article_block("Artículo 8 bis").as_deref(), Some("a8-2"));
        assert_eq!(article_block("20 ter").as_deref(), Some("a20-3"));
        assert_eq!(article_block("uno"), None);
        assert!(valid_id("BOE-A-1992-28740"));
        assert!(!valid_id("BOE-A-1992-28740/../x"));
        assert_eq!(
            words("tipos «reducidos» del IVA; drop*"),
            "tipos reducidos del IVA drop"
        );
        let xml = r#"<data><bloque><version a="1"><p>Viejo</p></version>
            <version a="2"><p class="articulo">Artículo 91. Tipos.</p>
            <p class="parrafo">Uno. Se aplicará el tipo del 10 por 100 &amp; más.</p></version></bloque></data>"#;
        assert_eq!(
            block_text(xml).as_deref(),
            Some("Artículo 91. Tipos.\nUno. Se aplicará el tipo del 10 por 100 & más.")
        );
    }
}

#[cfg(test)]
mod live {
    /// Contra el BOE real (red): `cargo test -p taxpain-assistant boe_real -- --ignored`.
    #[tokio::test]
    #[ignore = "usa la red"]
    async fn boe_real() {
        let c = crate::Client::new().unwrap_or_else(|e| panic!("{e}"));
        let laws = c
            .boe_search("Impuesto sobre el Valor Añadido")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(laws.iter().any(|l| l.id == "BOE-A-1992-28740"), "{laws:?}");
        let a = c
            .boe_article("BOE-A-1992-28740", "91")
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(a.starts_with("Artículo 91."), "{a}");
    }
}
