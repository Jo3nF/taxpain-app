//! Detector local de nombres de persona y direcciones, para lo que no está entre los valores
//! conocidos: el nombre de un particular en el concepto de una factura, el domicilio de un
//! cliente en el texto de un PDF…
//!
//! Une dos cosas, y todo corre en el Mac (el texto no sale de él para analizarlo):
//! - **Rampart Edge** (CC0, National Design Studio; `models/rampart-edge/`), un modelo de
//!   1,45 MB que marca nombres y direcciones. Va dentro del binario con su huella SHA-256
//!   comprobada en los tests, y la red está escrita a mano en `bert.rs` (sin ONNX Runtime).
//! - **Reglas** para lo que el modelo no ve: el nombre detrás de «D.ª», «Titular:» o «a nombre
//!   de», y las direcciones con tipo de vía y número.
//!
//! Adapta `packages/agent/src/{rampart-edge,detector}.ts` de reforma-digital (MIT, ver
//! `THIRD_PARTY_NOTICES.md`). No es una garantía: reforma-digital midió un 49,6 % de
//! nombres completos encontrados por el modelo. Es una capa más sobre los valores conocidos.

// La red neuronal trabaja con números reales; aquí no hay importes.
#![allow(clippy::float_arithmetic)]

mod bert;
mod onnx;
mod wordpiece;

use std::sync::OnceLock;

use regex::Regex;

const MODEL: &[u8] = include_bytes!("../../models/rampart-edge/model_q4.onnx");
const VOCAB: &str = include_str!("../../models/rampart-edge/vocab.txt");
/// Etiquetas de `config.json` (`span_labels`), en su orden.
const LABELS: [&str; bert::LABELS] = [
    "O",
    "B-NAME",
    "I-NAME",
    "B-ADDRESS",
    "I-ADDRESS",
    "B-EMAIL",
    "I-EMAIL",
    "B-PHONE",
    "I-PHONE",
    "B-GOV_ID",
    "I-GOV_ID",
    "B-FINANCIAL",
    "I-FINANCIAL",
    "B-DOB_KEYED",
    "I-DOB_KEYED",
    "B-URL_HANDLE",
    "I-URL_HANDLE",
    "B-OTHER_ID",
    "I-OTHER_ID",
];
/// Solape entre ventanas (`config.json`: `stride`).
const STRIDE: usize = 96;

/// Qué es lo encontrado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Name,
    Address,
}

impl Kind {
    pub fn marker(self) -> &'static str {
        match self {
            Kind::Name => "NOMBRE",
            Kind::Address => "DIRECCION",
        }
    }
}

/// Un dato encontrado: posiciones en bytes del texto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

pub struct Detector {
    model: bert::Model,
    vocab: wordpiece::Vocab,
}

impl std::fmt::Debug for Detector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Detector").finish_non_exhaustive()
    }
}

/// El detector, cargado una sola vez. Si no se puede cargar, el asistente no envía nada.
pub fn detector() -> Result<&'static Detector, String> {
    static D: OnceLock<Result<Detector, String>> = OnceLock::new();
    D.get_or_init(|| {
        let vocab = wordpiece::Vocab::parse(VOCAB)?;
        let model = bert::Model::load(MODEL, vocab.len)?;
        Ok(Detector { model, vocab })
    })
    .as_ref()
    .map_err(Clone::clone)
}

/// Lo que marca el modelo, antes de filtrar.
#[derive(Clone, Debug)]
struct Raw {
    start: usize,
    end: usize,
    label: &'static str,
    prob: f32,
}

impl Detector {
    /// Nombres y direcciones del texto, sin solapes y por orden. Falla (y entonces el texto no
    /// debe salir) si el modelo no ha podido analizarlo entero.
    pub fn find(&self, text: &str) -> Result<Vec<Span>, String> {
        let mut found: Vec<Span> = self
            .model_spans(text)?
            .into_iter()
            .filter_map(|r| {
                let kind = match r.label {
                    "NAME" => Kind::Name,
                    "ADDRESS" => Kind::Address,
                    // Documentos, cuentas, teléfonos y correos los cubren mejor las reglas con
                    // dígito de control de `pii.rs`; el modelo marcaría también números de
                    // factura o casillas.
                    _ => return None,
                };
                let r = if kind == Kind::Name {
                    capitalized(&r, text)?
                } else {
                    r
                };
                reliable(&r, kind, text).then_some(Span {
                    start: r.start,
                    end: r.end,
                    kind,
                })
            })
            .collect();
        found.extend(by_rules(text));
        Ok(merge(found))
    }

    fn model_spans(&self, text: &str) -> Result<Vec<Raw>, String> {
        let pieces = wordpiece::tokenize(text, &self.vocab);
        let content = bert::MAX_TOKENS - 2;
        let step = content.saturating_sub(STRIDE).max(1);
        // Cada token se queda con la etiqueta de la ventana en la que está más lejos del borde.
        let mut chosen: Vec<(usize, f32, isize)> = vec![(0, 0.0, -1); pieces.len()];
        let mut starts = vec![0];
        while starts.last().is_some_and(|&f| f + content < pieces.len()) {
            starts.push(starts[starts.len() - 1] + step);
        }
        let windows: Vec<Vec<usize>> = starts
            .iter()
            .map(|&from| {
                let window = &pieces[from..(from + content).min(pieces.len())];
                let mut ids = Vec::with_capacity(window.len() + 2);
                ids.push(self.vocab.cls);
                ids.extend(window.iter().map(|p| p.id));
                ids.push(self.vocab.sep);
                ids
            })
            .collect();
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        // Las ventanas a la vez (cada hilo coge la siguiente libre), y los núcleos que sobren
        // repartidos dentro de cada una.
        let workers = cores.min(windows.len());
        let inner = (cores / windows.len()).max(1);
        let next = std::sync::atomic::AtomicUsize::new(0);
        let mut failed = false;
        let mut logits: Vec<Vec<[f32; bert::LABELS]>> = vec![Vec::new(); windows.len()];
        std::thread::scope(|s| {
            let handles: Vec<_> = (0..workers)
                .map(|_| {
                    s.spawn(|| {
                        let mut done = Vec::new();
                        loop {
                            let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(ids) = windows.get(i) else { break };
                            done.push((i, self.model.logits(ids, inner)));
                        }
                        done
                    })
                })
                .collect();
            for h in handles {
                match h.join() {
                    Ok(done) => {
                        for (i, rows) in done {
                            logits[i] = rows;
                        }
                    }
                    Err(_) => failed = true,
                }
            }
        });
        if failed {
            return Err("el detector de nombres ha fallado".into());
        }
        for (&from, rows) in starts.iter().zip(&logits) {
            let len = rows.len().saturating_sub(2);
            for k in 0..len {
                let row = &rows[k + 1];
                let (label, max) =
                    row.iter()
                        .enumerate()
                        .fold(
                            (0, f32::NEG_INFINITY),
                            |acc, (i, &v)| {
                                if v > acc.1 { (i, v) } else { acc }
                            },
                        );
                let total: f32 = row.iter().map(|v| (v - max).exp()).sum();
                let margin = k.min(len - 1 - k) as isize;
                let c = &mut chosen[from + k];
                if margin > c.2 {
                    *c = (label, 1.0 / total, margin);
                }
            }
        }
        // Tokens seguidos con el mismo tipo (B- y sus I-) son un dato, ampliado a palabras enteras.
        let mut spans: Vec<(Raw, Vec<f32>)> = Vec::new();
        for (k, p) in pieces.iter().enumerate() {
            let label = LABELS[chosen[k].0];
            if label == "O" {
                continue;
            }
            let kind = &label[2..];
            let prev_o = k == 0 || LABELS[chosen[k - 1].0] == "O";
            let follows = spans.last().is_some_and(|(s, _)| {
                s.label == kind && (label.starts_with("I-") || s.end >= p.word.0) && !prev_o
            });
            match spans.last_mut() {
                Some((s, probs)) if follows => {
                    s.end = p.word.1;
                    probs.push(chosen[k].1);
                }
                _ => spans.push((
                    Raw {
                        start: p.word.0,
                        end: p.word.1,
                        label: kind,
                        prob: 0.0,
                    },
                    vec![chosen[k].1],
                )),
            }
        }
        Ok(spans
            .into_iter()
            .map(|(mut s, probs)| {
                s.prob = probs.iter().sum::<f32>() / probs.len() as f32;
                s
            })
            .collect())
    }
}

fn is_word_char(c: Option<char>) -> bool {
    c.is_some_and(char::is_alphanumeric)
}

/// Un nombre propio va en mayúscula: el modelo a veces se lleva las palabras de al lado. Se
/// queda de la primera a la última palabra en mayúscula; sin ninguna, no es un nombre.
fn capitalized(r: &Raw, text: &str) -> Option<Raw> {
    let value = &text[r.start..r.end];
    let mut first = None;
    let mut last = None;
    let mut offset = 0;
    for w in value.split_whitespace() {
        let at = offset + value[offset..].find(w).unwrap_or(0);
        offset = at + w.len();
        if w.chars().next().is_some_and(char::is_uppercase) {
            first.get_or_insert(at);
            last = Some(at + w.len());
        }
    }
    Some(Raw {
        start: r.start + first?,
        end: r.start + last?,
        ..r.clone()
    })
}

/// Descarta lo que el modelo marca y no suele ser un dato: un trozo de palabra, o un «nombre»
/// de una sola palabra con poca confianza («Getafe», «Alta»).
fn reliable(r: &Raw, kind: Kind, text: &str) -> bool {
    if is_word_char(text[..r.start].chars().next_back())
        || is_word_char(text[r.end..].chars().next())
    {
        return false;
    }
    if kind != Kind::Name {
        return true;
    }
    let value = &text[r.start..r.end];
    let single = !value.trim().contains(char::is_whitespace);
    !(single && r.prob < 0.6) && value.chars().any(char::is_uppercase)
}

struct Rules {
    address: Regex,
    presents: Regex,
    title: Regex,
    proper: Regex,
    /// Candidatos a «nombre de pila + apellidos»: de 2 a 5 palabras en mayúscula.
    capitalized: Regex,
}

/// Nombres de pila frecuentes en España (en minúscula y sin tildes). Faltan a propósito los que
/// también son palabras o lugares corrientes («Pilar», «Rosa», «Mercedes», «Domingo»,
/// «Santiago», «Victoria», «Salvador», «Paz»…): darían falsos positivos.
const GIVEN: &[&str] = &[
    "adrian",
    "agustin",
    "aitana",
    "aitor",
    "alberto",
    "alejandra",
    "alejandro",
    "alfonso",
    "alfredo",
    "alicia",
    "almudena",
    "alvaro",
    "amaia",
    "ana",
    "andrea",
    "andres",
    "angela",
    "antonia",
    "antonio",
    "ariadna",
    "arturo",
    "beatriz",
    "begona",
    "benito",
    "bernardo",
    "borja",
    "candela",
    "carla",
    "carlos",
    "carmen",
    "carolina",
    "catalina",
    "cayetana",
    "celia",
    "cesar",
    "claudia",
    "cristian",
    "cristina",
    "daniel",
    "daniela",
    "david",
    "diana",
    "diego",
    "eduardo",
    "elena",
    "elisa",
    "emilio",
    "enrique",
    "ernesto",
    "esteban",
    "esther",
    "eugenia",
    "eva",
    "fatima",
    "federico",
    "felipe",
    "fernando",
    "francisca",
    "francisco",
    "gabriel",
    "gema",
    "gemma",
    "gerard",
    "gerardo",
    "gonzalo",
    "gregorio",
    "guillermo",
    "gustavo",
    "hector",
    "hugo",
    "ignacio",
    "ines",
    "irene",
    "isabel",
    "ismael",
    "ivan",
    "jaime",
    "javier",
    "jesica",
    "joan",
    "joaquin",
    "jordi",
    "jorge",
    "jose",
    "josefa",
    "josep",
    "juan",
    "juana",
    "julia",
    "julian",
    "julio",
    "laia",
    "laura",
    "leire",
    "leonor",
    "lidia",
    "lorena",
    "lorenzo",
    "lucas",
    "lucia",
    "luis",
    "luisa",
    "manuel",
    "manuela",
    "marc",
    "marcos",
    "margarita",
    "maria",
    "mariano",
    "marina",
    "mario",
    "marta",
    "martina",
    "maximo",
    "miguel",
    "miriam",
    "monica",
    "montserrat",
    "nerea",
    "nicolas",
    "noelia",
    "nuria",
    "oscar",
    "pablo",
    "patricia",
    "paula",
    "pedro",
    "pere",
    "rafael",
    "ramon",
    "raquel",
    "raul",
    "rebeca",
    "ricardo",
    "roberto",
    "rocio",
    "rodrigo",
    "ruben",
    "samuel",
    "sandra",
    "sara",
    "sergio",
    "silvia",
    "sofia",
    "sonia",
    "susana",
    "teresa",
    "tomas",
    "valentina",
    "vanesa",
    "vanessa",
    "veronica",
    "vicente",
    "vicenta",
    "virginia",
    "xavier",
    "yolanda",
];

/// Palabras en mayúscula que, detrás de un nombre de pila, indican que no es una persona.
const NOT_SURNAME: &[&str] = &[
    "sl",
    "slu",
    "sa",
    "sau",
    "scp",
    "cb",
    "sociedad",
    "asociacion",
    "fundacion",
    "grupo",
    "servicios",
    "consultores",
    "asesores",
    "hermanos",
    "e",
    "hijos",
    "calle",
    "avenida",
    "plaza",
    "hospital",
    "colegio",
    "instituto",
    "hotel",
    "restaurante",
    "bar",
    "farmacia",
];

/// Minúsculas sin tildes.
fn plain(word: &str) -> String {
    use unicode_normalization::UnicodeNormalization;
    use unicode_normalization::char::is_combining_mark;
    word.to_lowercase()
        .nfd()
        .filter(|&c| !is_combining_mark(c))
        .collect()
}

fn rules() -> &'static Rules {
    static R: OnceLock<Rules> = OnceLock::new();
    R.get_or_init(|| {
        let r = |s: &str| Regex::new(s).expect("regla fija válida");
        // Tipo de vía: tras una abreviatura con punto o barra el nombre puede ir pegado.
        let via = r"(?:(?:c/|avda\.|av\.|pza\.|p[º°]|ctra\.|urb\.|avgda\.|pl\.)\s*|(?:calle|avda|avenida|plaza|pza|paseo|carretera|ctra|ronda|traves[ií]a|glorieta|callej[oó]n|urbanizaci[oó]n|pasaje|rambla|carrer|avinguda|pla[cç]a|passeig|cam[ií]|partida)\s+)";
        let word = r"[\p{L}\p{N}][\p{L}\p{N}.'’-]*";
        // El nombre de la vía y el número o «s/n», con piso, escalera o puerta y, si sigue, el
        // código postal y la población.
        let rest = format!(
            r"{word}(?:\s+{word}){{0,5}}?,?\s*(?:n[º°.]\s*)?(?:\d{{1,4}}(?:\s?[a-z]\b)?|s/n)(?:,?\s*(?:\d{{1,2}}\s?\.?[º°ª]\s?[a-z]{{0,4}}\.?|bajo|[áa]tico|esc(?:alera)?\.?\s*\w+|puerta\s*\w+|piso\s*\w+|pta\.?\s*\w+))*(?:,?\s*\d{{5}}(?:\s+\p{{Lu}}[\p{{L}}'’-]+)?)?"
        );
        let parentesco = "hij[oa]|madre|padre|marido|mujer|espos[oa]|pareja|herman[oa]|abuel[oa]|niet[oa]|t[ií][oa]|sobrin[oa]|prim[oa]|suegr[oa]|cuñad[oa]|yerno|nuera|novi[oa]";
        Rules {
            address: r(&format!(
                r"(?i)(?:^|[^\p{{L}}\p{{N}}])(?:(?:domicilio|direcci[oó]n)(?:\s+(?:postal|fiscal|social))?\s*:\s*(?P<a>(?:{via})?{rest})|(?P<b>{via}{rest}))"
            )),
            // Lo que presenta un nombre: «me llamo», «a nombre de», «mi hija», «Titular:»,
            // «Cliente:»…
            presents: r(&format!(
                r"(?i)(?:^|[^\p{{L}}\p{{N}}])(?:(?:me\s+llamo|mi\s+nombre\s+es|a\s+nombre\s+de|(?:mi|tu|su|nuestr[oa])\s+(?:{parentesco}))\s+|(?:nombre(?:\s+y\s+apellidos)?|apellidos|titular|cliente|destinatari[oa]|administrador[a]?|representante|trabajador[a]?|emplead[oa])\s*:[^\S\n]*)"
            )),
            // «D.ª», «Dña.», «D.», «don», «doña»: «d.» en minúscula es un apartado y «R.D.», un
            // real decreto, por eso no puede ir detrás de un punto ni de una letra.
            title: r(r"(?:^|[^\p{L}\p{N}.])(?:D\.?ª\s*|(?:Dña|D)\.\s*|[Dd](?:on|oña)\s+)+"),
            // Palabras en mayúscula, con «de», «del» o «de la» entre ellas: «María de la Fuente».
            proper: r(
                r"^\p{Lu}[\p{L}'’-]+(?:[^\S\n]+(?:del?[^\S\n]+(?:l[ao]s?[^\S\n]+)?)?\p{Lu}[\p{L}'’-]+){0,4}",
            ),
            capitalized: r(
                r"\p{Lu}[\p{L}'’-]+(?:[^\S\n]+(?:(?:del?|i|y)[^\S\n]+(?:l[ao]s?[^\S\n]+)?)?\p{Lu}[\p{L}'’-]+){1,4}",
            ),
        }
    })
}

/// «Lucía Villalobos Cuesta», «JOSÉ LUIS PÉREZ»: empieza por un nombre de pila frecuente y le
/// siguen apellidos (palabras en mayúscula que no son de una empresa).
fn given_names(text: &str, out: &mut Vec<Span>) {
    for m in rules().capitalized.find_iter(text) {
        if is_word_char(text[..m.start()].chars().next_back()) {
            continue;
        }
        let words: Vec<String> = m.as_str().split_whitespace().map(plain).collect();
        let starts = GIVEN.contains(&words[0].as_str());
        let surnames = words[1..]
            .iter()
            .filter(|w| !matches!(w.as_str(), "de" | "del" | "la" | "las" | "los" | "i" | "y"))
            .filter(|w| !GIVEN.contains(&w.as_str()))
            .count();
        let company = words.iter().any(|w| NOT_SURNAME.contains(&w.as_str()));
        if starts && surnames > 0 && !company {
            out.push(Span {
                start: m.start(),
                end: m.end(),
                kind: Kind::Name,
            });
        }
    }
}

/// Lo que encuentran las reglas, sin el modelo.
fn by_rules(text: &str) -> Vec<Span> {
    let r = rules();
    let mut out = Vec::new();
    given_names(text, &mut out);
    for c in r.address.captures_iter(text) {
        if let Some(m) = c.name("a").or_else(|| c.name("b")) {
            out.push(Span {
                start: m.start(),
                end: m.end(),
                kind: Kind::Address,
            });
        }
    }
    let starts = r
        .presents
        .find_iter(text)
        .chain(r.title.find_iter(text))
        .map(|m| m.end());
    for at in starts {
        let Some(m) = r.proper.find(&text[at..]) else {
            continue;
        };
        let mut end = at + m.end();
        // Una palabra seguida de «:» es otra etiqueta («Nombre: Apellidos:»), no un nombre.
        if text[end..].trim_start_matches([' ', '\t']).starts_with(':') {
            match text[at..end].rfind(char::is_whitespace) {
                Some(i) => end = at + i,
                None => continue,
            }
        }
        out.push(Span {
            start: at,
            end: at + text[at..end].trim_end().len(),
            kind: Kind::Name,
        });
    }
    out
}

/// Une los trozos que se solapan (el modelo marca «Calle» y la regla «Calle Mayor 1, 2º B»).
fn merge(mut spans: Vec<Span>) -> Vec<Span> {
    spans.sort_by_key(|s| (s.start, std::cmp::Reverse(s.end)));
    let mut out: Vec<Span> = Vec::new();
    for s in spans {
        match out.last_mut() {
            Some(prev) if s.start < prev.end => prev.end = prev.end.max(s.end),
            _ => out.push(s),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha256_hex(data: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        Sha256::digest(data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    #[test]
    fn modelo_con_la_huella_publicada() {
        // Huellas de `config.json` → `files` en la revisión 69a99a3 de Hugging Face.
        assert_eq!(
            sha256_hex(MODEL),
            "73fce00c229d8318d7e1f8decccc5ad2dbf7aa45a872364b54d632c38a9eb16c"
        );
        assert_eq!(
            sha256_hex(VOCAB.as_bytes()),
            "ee0684683c7fb15744d0136e671cd7def76ae4d4ddafb72ced864e94c8d32c9a"
        );
    }

    #[derive(serde::Deserialize)]
    struct Golden {
        text: String,
        ids: Vec<usize>,
        logits: Vec<Vec<f32>>,
        ort_labels: Vec<usize>,
    }

    /// Los mismos tokens que el tokenizador oficial de Hugging Face, las mismas puntuaciones que
    /// el grafo calculado en coma flotante (numpy) y la misma etiqueta en cada token que ONNX
    /// Runtime (que redondea a 8 bits al multiplicar y da puntuaciones algo distintas).
    #[test]
    fn igual_que_onnxruntime() {
        let d = detector().unwrap_or_else(|e| panic!("{e}"));
        let golden: Vec<Golden> =
            serde_json::from_str(include_str!("golden.json")).unwrap_or_else(|e| panic!("{e}"));
        for g in golden {
            let mut ids = vec![d.vocab.cls];
            ids.extend(wordpiece::tokenize(&g.text, &d.vocab).iter().map(|p| p.id));
            ids.push(d.vocab.sep);
            assert_eq!(ids, g.ids, "tokens de «{}»", g.text);
            let out = d.model.logits(&ids, 2);
            for ((row, want), &label) in out.iter().zip(&g.logits).zip(&g.ort_labels) {
                for (a, b) in row.iter().zip(want) {
                    assert!((a - b).abs() < 0.01, "«{}»: {a} frente a {b}", g.text);
                }
                let best = (0..bert::LABELS).fold(0, |m, i| if row[i] > row[m] { i } else { m });
                assert_eq!(best, label, "«{}»", g.text);
            }
        }
    }

    fn found(text: &str) -> Vec<(&str, Kind)> {
        let d = detector().unwrap_or_else(|e| panic!("{e}"));
        d.find(text)
            .unwrap_or_else(|e| panic!("{e}"))
            .into_iter()
            .map(|s| (&text[s.start..s.end], s.kind))
            .collect()
    }

    #[test]
    fn nombres_y_direcciones() {
        let t = "Factura emitida por María José García López, con domicilio en Calle Mayor 12, 3º B, 46001 Valencia.";
        let f = found(t);
        assert!(
            f.contains(&("María José García López", Kind::Name)),
            "{f:?}"
        );
        assert!(
            f.iter()
                .any(|(v, k)| *k == Kind::Address && v.starts_with("Calle Mayor 12")),
            "{f:?}"
        );
        let f = found(
            "Concepto: reparación del coche de Antonio Fernández. Base imponible 1.200,00 €, IVA 21 % 252,00 €.",
        );
        assert!(f.contains(&("Antonio Fernández", Kind::Name)), "{f:?}");
    }

    #[test]
    fn reglas_para_lo_que_el_modelo_no_ve() {
        let f = found("Cliente: Jordi Puig i Ferrer, Avinguda Diagonal 640, 08017 Barcelona.");
        assert!(
            f.iter()
                .any(|(v, k)| *k == Kind::Name && v.starts_with("Jordi Puig")),
            "{f:?}"
        );
        assert!(
            f.iter()
                .any(|(v, k)| *k == Kind::Address && v.starts_with("Avinguda Diagonal 640")),
            "{f:?}"
        );
        let f = found("Recibí de D.ª Lucía Ortega Vidal la cantidad de 300 €.");
        assert!(f.iter().any(|(v, _)| v.contains("Lucía Ortega")), "{f:?}");
        let f = found("Pedido recogido en tienda por Lucía Villalobos Cuesta. Total 121,00 EUR");
        assert!(
            f.contains(&("Lucía Villalobos Cuesta", Kind::Name)),
            "{f:?}"
        );
        let f = found("ENTREGADO A JOSÉ LUIS PÉREZ MORA EL 3/10");
        assert!(f.iter().any(|(v, _)| v.contains("PÉREZ MORA")), "{f:?}");
        // Un nombre de pila en el nombre de una empresa no es una persona.
        assert!(
            found("Factura de Antonio Hermanos SL por 50 €.")
                .iter()
                .all(|(v, _)| !v.contains("Hermanos"))
        );
        // «Nombre: Apellidos:» de un formulario vacío no es un nombre.
        assert!(found("Nombre: Apellidos: ").is_empty());
    }

    #[test]
    fn no_marca_textos_fiscales() {
        for t in [
            "¿Cómo me hago autónomo?",
            "El Impuesto sobre Sociedades se presenta en julio; la Agencia Tributaria publica el modelo 200.",
            "Modelo 303, casilla [71]: 1.234,56 €. Real Decreto 1619/2012, artículo 15.",
            "Según el apartado d. del artículo 7 y el R.D. 439/2007, la base es 2.000,00 €.",
        ] {
            assert_eq!(found(t), vec![], "{t}");
        }
    }

    #[test]
    fn texto_largo_en_varias_ventanas() {
        let relleno = "La base imponible del trimestre es de 1.000,00 euros. ".repeat(60);
        let t = format!("{relleno}Pagado por Antonio Fernández Ruiz. {relleno}");
        let f = found(&t);
        assert!(
            f.iter().any(|(v, _)| v.contains("Antonio Fernández")),
            "{f:?}"
        );
    }
}
