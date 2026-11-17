//! Tokenizador WordPiece de BERT (minúsculas y sin tildes) que conserva, para cada pieza, su
//! posición en el texto original: el detector necesita saber qué parte del texto ocultar.
//!
//! Adapta `tokenizar` de `packages/agent/src/rampart-edge.ts` de reforma-digital (MIT, ver
//! `THIRD_PARTY_NOTICES.md`); aquí las posiciones son en bytes de UTF-8.

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// BERT da por desconocida una palabra más larga que esto.
const MAX_WORD: usize = 100;

#[derive(Debug)]
pub(super) struct Vocab {
    ids: HashMap<String, usize>,
    pub unk: usize,
    pub cls: usize,
    pub sep: usize,
    pub len: usize,
}

impl Vocab {
    pub(super) fn parse(text: &str) -> Result<Self, String> {
        let ids: HashMap<String, usize> = text
            .lines()
            .enumerate()
            .map(|(i, t)| (t.trim().to_owned(), i))
            .collect();
        let get = |t: &str| {
            ids.get(t)
                .copied()
                .ok_or_else(|| format!("el vocabulario no tiene {t}"))
        };
        Ok(Vocab {
            unk: get("[UNK]")?,
            cls: get("[CLS]")?,
            sep: get("[SEP]")?,
            len: text.lines().count(),
            ids,
        })
    }
}

/// Una pieza: su identificador, dónde está y dónde está la palabra a la que pertenece.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Piece {
    pub id: usize,
    pub start: usize,
    pub end: usize,
    pub word: (usize, usize),
}

struct Classes {
    /// Lo que BERT trata como palabra suelta: la puntuación y cada ideograma chino (los signos
    /// ASCII se miran aparte). Los demás símbolos (`€`, emojis) van pegados a su palabra.
    loose: Regex,
    space: Regex,
    /// Controles y caracteres de formato: se quitan sin separar («a\u{200B}b» es «ab»).
    hidden: Regex,
}

fn classes() -> &'static Classes {
    static C: OnceLock<Classes> = OnceLock::new();
    C.get_or_init(|| {
        let r = |s: &str| Regex::new(s).expect("clase fija válida");
        Classes {
            loose: r(r"^[\p{P}\p{Unified_Ideograph}]$"),
            space: r(r"^[\t\n\r\p{Zs}]$"),
            hidden: r(r"^[\p{Cc}\p{Cf}]$"),
        }
    })
}

struct Word {
    text: Vec<char>,
    /// Posición original (inicio y fin en bytes) de cada carácter normalizado.
    pos: Vec<(usize, usize)>,
    start: usize,
    end: usize,
}

pub(super) fn tokenize(text: &str, vocab: &Vocab) -> Vec<Piece> {
    let c = classes();
    let mut words: Vec<Word> = Vec::new();
    let mut open = false;
    let mut buf = [0u8; 4];
    for (i, ch) in text.char_indices() {
        let next = i + ch.len_utf8();
        let s: &str = ch.encode_utf8(&mut buf);
        if c.space.is_match(s) {
            open = false;
            continue;
        }
        if c.hidden.is_match(s) {
            continue;
        }
        let norm: Vec<char> = ch
            .to_lowercase()
            .nfd()
            .filter(|&m| !is_combining_mark(m))
            .collect();
        if ch.is_ascii_punctuation() || c.loose.is_match(s) {
            words.push(Word {
                pos: vec![(i, next); norm.len()],
                text: norm,
                start: i,
                end: next,
            });
            open = false;
        } else if !norm.is_empty() {
            if !open {
                words.push(Word {
                    text: Vec::new(),
                    pos: Vec::new(),
                    start: i,
                    end: i,
                });
                open = true;
            }
            if let Some(w) = words.last_mut() {
                for n in norm {
                    w.text.push(n);
                    w.pos.push((i, next));
                }
                w.end = next;
            }
        }
    }
    let mut pieces = Vec::new();
    for w in &words {
        let word = (w.start, w.end);
        let mut sub = Vec::new();
        let mut from = 0;
        while w.text.len() <= MAX_WORD && from < w.text.len() {
            // La pieza más larga que esté en el vocabulario; las que siguen a otra llevan «##».
            let mut found = None;
            for to in (from + 1..=w.text.len()).rev() {
                let piece: String = w.text[from..to].iter().collect();
                let key = if from > 0 {
                    format!("##{piece}")
                } else {
                    piece
                };
                if let Some(&id) = vocab.ids.get(&key) {
                    found = Some((id, to));
                    break;
                }
            }
            let Some((id, to)) = found else { break };
            sub.push(Piece {
                id,
                start: w.pos[from].0,
                end: w.pos[to - 1].1,
                word,
            });
            from = to;
        }
        // Si no se puede trocear entera, la palabra es desconocida.
        if from == w.text.len() && !sub.is_empty() {
            pieces.extend(sub);
        } else {
            pieces.push(Piece {
                id: vocab.unk,
                start: w.start,
                end: w.end,
                word,
            });
        }
    }
    pieces
}
