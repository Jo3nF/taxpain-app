//! Ficheros de autoliquidación según los diseños de registro (DR) oficiales de la AEAT.
//!
//! Los DR (`fixtures/aeat/dr/*.xls[x]`) se convierten a JSON con `tools/aeat/dr2json.py`; aquí
//! se cargan esos JSON y se escribe/lee cada página como un registro de ancho fijo.
//!
//! Reglas comunes de los DR (nota 2 de cada hoja):
//! - `An`/`A`: alineados a la izquierda y rellenos de blancos; sólo letras, números y blancos.
//! - `Num`: sin signo, alineados a la derecha y rellenos de ceros.
//! - `N`: con signo; los negativos llevan una `N` en la primera posición.
//! - Importes: 15 enteros y 2 decimales sin separador (en céntimos).
//!
//! `TODO(VERIFY)`: codificación del fichero (se genera ISO-8859-1) y tratamiento de signos de
//! puntuación en los nombres (se sustituyen por blancos); confirmar con la prevalidación.

use std::collections::BTreeMap;

use serde::Deserialize;
use taxpain_core::Money;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DrError {
    #[error("el DR del modelo {0} no tiene la página {1}")]
    NoPage(String, String),
    #[error("{page}: el campo {n} no existe")]
    NoField { page: String, n: u16 },
    #[error("{page}: la casilla [{0}] no existe", .box_)]
    NoBox { page: String, box_: u16 },
    #[error("{page}: el campo {n} es una constante del DR")]
    Constant { page: String, n: u16 },
    #[error("{page}: el campo {n} no admite negativos ({value})")]
    Negative { page: String, n: u16, value: Money },
    #[error("{page}: el campo {n} no cabe en {len} posiciones ({value})")]
    Overflow {
        page: String,
        n: u16,
        len: usize,
        value: String,
    },
    #[error("{page}: registro de {got} posiciones, el DR dice {expected}")]
    Length {
        page: String,
        got: usize,
        expected: usize,
    },
    #[error("el texto no es representable en ISO-8859-1")]
    Encoding,
    /// El caso no está implementado y un fichero generado sería incorrecto.
    #[error("{0}")]
    Unsupported(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Kind {
    An,
    A,
    Num,
    N,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Field {
    pub n: u16,
    pub pos: usize,
    pub len: usize,
    #[serde(rename = "type")]
    pub kind: Kind,
    pub desc: String,
    #[serde(rename = "box")]
    pub box_: Option<u16>,
    #[serde(rename = "const")]
    pub constant: Option<String>,
}

impl Field {
    /// Campo de importe (numérico de 13 posiciones o más, sin valor fijo): los de las casillas
    /// en euros. No lo son los recuentos aunque sean largos («Número de rentas» [01]–[03] del
    /// 123, «Número perceptores» [01] del 115).
    pub fn is_amount(&self) -> bool {
        self.constant.is_none()
            && matches!(self.kind, Kind::N | Kind::Num)
            && self.len >= 13
            && !self.desc.to_lowercase().contains("número")
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct PageSpec {
    pub sheet: String,
    pub fields: Vec<Field>,
}

impl PageSpec {
    pub fn len(&self) -> usize {
        self.fields.last().map_or(0, |f| f.pos - 1 + f.len)
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn field(&self, n: u16) -> Result<&Field, DrError> {
        self.fields
            .iter()
            .find(|f| f.n == n)
            .ok_or_else(|| DrError::NoField {
                page: self.sheet.clone(),
                n,
            })
    }

    pub fn field_for_box(&self, box_: u16) -> Result<&Field, DrError> {
        self.fields
            .iter()
            .find(|f| f.box_ == Some(box_) && f.constant.is_none())
            .ok_or_else(|| DrError::NoBox {
                page: self.sheet.clone(),
                box_,
            })
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Spec {
    pub model: String,
    pub source: String,
    pub sha256: String,
    pub pages: Vec<PageSpec>,
}

impl Spec {
    pub fn parse(json: &str) -> Self {
        // Los JSON van embebidos en el binario y los valida un test: no pueden fallar.
        #[allow(clippy::expect_used)]
        serde_json::from_str(json).expect("DR JSON embebido válido")
    }

    /// Página cuya hoja termina en `suffix` (p. ej. "01", "03", "DID").
    pub fn page(&self, suffix: &str) -> Result<&PageSpec, DrError> {
        self.pages
            .iter()
            .find(|p| p.sheet.ends_with(suffix))
            .ok_or_else(|| DrError::NoPage(self.model.clone(), suffix.to_owned()))
    }
}

/// Texto alfanumérico del DR: mayúsculas, sin tildes y sólo letras (incluidas Ñ y Ç), números
/// y blancos. Los demás caracteres pasan a blanco y los blancos repetidos se compactan.
pub fn alnum(s: &str) -> String {
    let mapped: String = s
        .chars()
        .flat_map(char::to_uppercase)
        .map(|c| match c {
            'Á' | 'À' | 'Ä' | 'Â' => 'A',
            'É' | 'È' | 'Ë' | 'Ê' => 'E',
            'Í' | 'Ì' | 'Ï' | 'Î' => 'I',
            'Ó' | 'Ò' | 'Ö' | 'Ô' => 'O',
            'Ú' | 'Ù' | 'Ü' | 'Û' => 'U',
            c if c.is_ascii_alphanumeric() || c == 'Ñ' || c == 'Ç' => c,
            _ => ' ',
        })
        .collect();
    mapped.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Escritura de una página: se rellenan casillas e importes; constantes y huecos los pone el DR.
#[derive(Debug)]
pub struct PageWriter<'a> {
    spec: &'a PageSpec,
    values: BTreeMap<u16, String>,
}

impl<'a> PageWriter<'a> {
    pub fn new(spec: &'a PageSpec) -> Self {
        PageWriter {
            spec,
            values: BTreeMap::new(),
        }
    }

    fn settable(&self, n: u16) -> Result<&'a Field, DrError> {
        let f = self.spec.field(n)?;
        if f.constant.is_some() {
            return Err(DrError::Constant {
                page: self.spec.sheet.clone(),
                n,
            });
        }
        Ok(f)
    }

    /// Importe (o porcentaje con dos decimales) en el campo `n`.
    pub fn amount(&mut self, n: u16, value: Money) -> Result<&mut Self, DrError> {
        let f = self.settable(n)?;
        let page = || self.spec.sheet.clone();
        let digits = value.cents().unsigned_abs().to_string();
        let s = match (f.kind, value.cents() < 0) {
            (Kind::Num, true) => {
                return Err(DrError::Negative {
                    page: page(),
                    n,
                    value,
                });
            }
            (Kind::N, true) => format!("N{digits:0>width$}", width = f.len - 1),
            _ => format!("{digits:0>width$}", width = f.len),
        };
        if s.len() != f.len {
            return Err(DrError::Overflow {
                page: page(),
                n,
                len: f.len,
                value: value.to_string(),
            });
        }
        self.values.insert(n, s);
        Ok(self)
    }

    /// Importe en la casilla `[box_]`.
    pub fn amount_box(&mut self, box_: u16, value: Money) -> Result<&mut Self, DrError> {
        let n = self.spec.field_for_box(box_)?.n;
        self.amount(n, value)
    }

    /// Texto (alfanumérico) o código en el campo `n`; se normaliza con [`alnum`] salvo que sea
    /// un código ya válido. Los numéricos se rellenan con ceros por la izquierda.
    pub fn text(&mut self, n: u16, value: &str) -> Result<&mut Self, DrError> {
        let f = self.settable(n)?;
        let v = match f.kind {
            Kind::An | Kind::A => alnum(value),
            Kind::Num | Kind::N => value.to_owned(),
        };
        if v.chars().count() > f.len {
            return Err(DrError::Overflow {
                page: self.spec.sheet.clone(),
                n,
                len: f.len,
                value: v,
            });
        }
        let padded = match f.kind {
            Kind::An | Kind::A => format!("{v:<width$}", width = f.len),
            Kind::Num | Kind::N => format!("{v:0>width$}", width = f.len),
        };
        self.values.insert(n, padded);
        Ok(self)
    }

    /// Valor literal (ya con su longitud exacta), p. ej. un blanco en un campo numérico cuando
    /// el DR lo pide expresamente.
    pub fn raw(&mut self, n: u16, value: &str) -> Result<&mut Self, DrError> {
        let f = self.settable(n)?;
        if value.chars().count() != f.len {
            return Err(DrError::Overflow {
                page: self.spec.sheet.clone(),
                n,
                len: f.len,
                value: value.to_owned(),
            });
        }
        self.values.insert(n, value.to_owned());
        Ok(self)
    }

    /// Escritura sobre una página ya escrita: parte de los valores de `record` (para cambiar
    /// unos campos de un fichero generado, como las marcas de una rectificativa).
    pub fn from_record(spec: &'a PageSpec, record: &str) -> Result<Self, DrError> {
        let reader = PageReader::new(spec, record)?;
        let mut values = BTreeMap::new();
        for f in spec.fields.iter().filter(|f| f.constant.is_none()) {
            values.insert(f.n, reader.raw(f.n)?);
        }
        Ok(PageWriter { spec, values })
    }

    pub fn finish(&self) -> Result<String, DrError> {
        let mut out = String::with_capacity(self.spec.len());
        let mut written = 0usize;
        for f in &self.spec.fields {
            // Un hueco entre dos campos (algún diseño oficial lo tiene, como la página 03 del 303
            // de 2017 a 2020): en blanco, para que cada campo quede en su posición.
            while written + 1 < f.pos {
                out.push(' ');
                written += 1;
            }
            let v = match (&f.constant, self.values.get(&f.n)) {
                (Some(c), _) => format!("{c:<width$}", width = f.len),
                (None, Some(v)) => v.clone(),
                (None, None) => match f.kind {
                    Kind::An | Kind::A => " ".repeat(f.len),
                    Kind::Num | Kind::N => "0".repeat(f.len),
                },
            };
            written += v.chars().count();
            out.push_str(&v);
        }
        let got = out.chars().count();
        if got != self.spec.len() {
            return Err(DrError::Length {
                page: self.spec.sheet.clone(),
                got,
                expected: self.spec.len(),
            });
        }
        Ok(out)
    }
}

/// Lectura de una página ya escrita (para tests y para mostrar el borrador de un fichero).
#[derive(Debug)]
pub struct PageReader<'a> {
    spec: &'a PageSpec,
    record: Vec<char>,
}

impl<'a> PageReader<'a> {
    pub fn new(spec: &'a PageSpec, record: &str) -> Result<Self, DrError> {
        let record: Vec<char> = record.chars().collect();
        if record.len() != spec.len() {
            return Err(DrError::Length {
                page: spec.sheet.clone(),
                got: record.len(),
                expected: spec.len(),
            });
        }
        Ok(PageReader { spec, record })
    }

    pub fn raw(&self, n: u16) -> Result<String, DrError> {
        let f = self.spec.field(n)?;
        Ok(self.record[f.pos - 1..f.pos - 1 + f.len].iter().collect())
    }

    pub fn amount(&self, n: u16) -> Result<Option<Money>, DrError> {
        let s = self.raw(n)?;
        let (neg, digits) = match s.strip_prefix('N') {
            Some(d) => (true, d),
            None => (false, s.as_str()),
        };
        Ok(digits.parse::<i64>().ok().map(|c| {
            let m = Money::from_cents(c);
            if neg { -m } else { m }
        }))
    }

    pub fn amount_box(&self, box_: u16) -> Result<Option<Money>, DrError> {
        self.amount(self.spec.field_for_box(box_)?.n)
    }
}

/// Cabecera de un fichero de autoliquidación: (modelo, ejercicio, periodo).
pub fn read_header(text: &str) -> Option<(String, i32, String)> {
    let start = text.find("<T")?;
    let h: Vec<char> = text[start..].chars().take(16).collect();
    // <T + modelo (3) + 0 + ejercicio (4) + periodo (2) + 0000>
    if h.len() < 16 || h[5] != '0' || h[12..16].iter().collect::<String>() != "0000" {
        return None;
    }
    let model: String = h[2..5].iter().collect();
    let year: i32 = h[6..10].iter().collect::<String>().parse().ok()?;
    let period: String = h[10..12].iter().collect();
    model
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then_some((model, year, period))
}

/// ¿Cuadra cada página del fichero con el diseño? Cada página va de su marca de inicio
/// (`<T30301000>`) a la de cierre (`</T30301000>`), y entre las dos
/// tiene que haber exactamente la longitud del diseño. Si alguna no cuadra (o no aparece ninguna
/// página), el fichero es de otro diseño y no se puede leer por posiciones.
pub fn pages_fit(spec: &Spec, text: &str) -> bool {
    let mut seen = 0;
    for page in &spec.pages {
        let marker: String = page
            .fields
            .iter()
            .take_while(|f| f.constant.is_some())
            .filter_map(|f| f.constant.clone())
            .collect();
        if marker.len() < 8 || !marker.starts_with("<T") || !marker.ends_with('>') {
            continue;
        }
        // La cabecera del fichero junta dos marcas («<T200020250A0000><AUX>» en el 200 de 2025):
        // no es una página con cierre propio y no se comprueba.
        if marker.find('>').is_some_and(|i| i + 1 != marker.len()) {
            continue;
        }
        // La marca de cierre se saca de la de inicio y no de la constante del último campo: en
        // algún diseño oficial está mal escrita (página 7 del 390 de 2022: «</T3900700>»).
        let close = format!("</{}", &marker[1..]);
        let Some(at) = text.find(&marker) else {
            continue;
        };
        let Some(end) = text[at..].find(&close) else {
            return false;
        };
        if text[at..at + end + close.len()].chars().count() != page.len() {
            return false;
        }
        seen += 1;
    }
    seen > 0
}

/// Importes de las casillas de un fichero de autoliquidación ya presentado (el inverso de
/// [`PageWriter`]): cada página se busca por su marca de inicio (`<T30301000>`…) y se leen los
/// campos numéricos que son casillas. `box_of` da la casilla de un campo cuando el DR no la
/// trae (en el 111, el campo 12 es la casilla [01]).
pub fn read_boxes(
    spec: &Spec,
    text: &str,
    box_of: impl Fn(&Field) -> Option<u16>,
) -> BTreeMap<u16, Money> {
    let mut out = BTreeMap::new();
    for page in &spec.pages {
        // La marca: las constantes del principio de la página.
        let marker: String = page
            .fields
            .iter()
            .take_while(|f| f.constant.is_some())
            .filter_map(|f| f.constant.clone())
            .collect();
        if marker.len() < 8 || !marker.ends_with('>') {
            continue;
        }
        let Some(at) = text.find(&marker) else {
            continue;
        };
        let record: String = text[at..].chars().take(page.len()).collect();
        let Ok(reader) = PageReader::new(page, &record) else {
            continue;
        };
        for f in &page.fields {
            if !f.is_amount() {
                continue;
            }
            let Some(b) = f.box_.or_else(|| box_of(f)) else {
                continue;
            };
            if let Ok(Some(m)) = reader.amount(f.n)
                && !m.is_zero()
            {
                out.entry(b).or_insert(m);
            }
        }
    }
    out
}

/// Marca de inicio de una página (las constantes del principio: `<T30303000>`).
pub fn page_marker(page: &PageSpec) -> String {
    page.fields
        .iter()
        .take_while(|f| f.constant.is_some())
        .filter_map(|f| f.constant.clone())
        .collect()
}

/// Cambia unos campos de la página `suffix` de un fichero ya generado y devuelve el fichero
/// entero. Error si la página no está en el fichero o no tiene la longitud del diseño.
pub fn patch_page(
    spec: &Spec,
    text: &str,
    suffix: &str,
    edit: impl FnOnce(&mut PageWriter<'_>) -> Result<(), DrError>,
) -> Result<String, DrError> {
    let page = spec.page(suffix)?;
    let marker = page_marker(page);
    let at = text
        .find(&marker)
        .filter(|_| marker.len() >= 8)
        .ok_or_else(|| DrError::NoPage(spec.model.clone(), suffix.to_owned()))?;
    let record: String = text[at..].chars().take(page.len()).collect();
    let mut w = PageWriter::from_record(page, &record)?;
    edit(&mut w)?;
    let new = w.finish()?;
    Ok(format!(
        "{}{new}{}",
        &text[..at],
        &text[at + record.len()..]
    ))
}

/// Añade una página al final de un fichero, delante de la marca de cierre del fichero
/// (`</T303020261T0000>`).
pub fn append_page(text: &str, page: &str) -> String {
    match text.rfind("</T") {
        Some(at) => format!("{}{page}{}", &text[..at], &text[at..]),
        None => format!("{text}{page}"),
    }
}

/// Fichero completo: `<T{modelo}0{ejercicio}{periodo}0000>` + `<AUX>…</AUX>` + páginas + cierre.
/// `version` (4) y `developer_nif` (9) identifican el software y a su desarrollador (nota 1).
pub fn envelope(
    model: &str,
    year: i32,
    period: &str,
    version: &str,
    developer_nif: &str,
    pages: &[String],
) -> String {
    let mut s = format!("<T{model}0{year:04}{period}0000>");
    s.push_str("<AUX>");
    s.push_str(&" ".repeat(70));
    s.push_str(&format!("{:<4.4}", version));
    s.push_str(&" ".repeat(4));
    s.push_str(&format!("{:<9.9}", developer_nif));
    s.push_str(&" ".repeat(213));
    s.push_str("</AUX>");
    for p in pages {
        s.push_str(p);
    }
    s.push_str(&format!("</T{model}0{year:04}{period}0000>"));
    s
}

/// Bytes del fichero en ISO-8859-1.
pub fn to_latin1(s: &str) -> Result<Vec<u8>, DrError> {
    s.chars()
        .map(|c| u8::try_from(u32::from(c)).map_err(|_| DrError::Encoding))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alnum_normaliza() {
        assert_eq!(
            alnum("Estudio Peñón, S.L. — Café"),
            "ESTUDIO PEÑON S L CAFE"
        );
        assert_eq!(alnum("  a  b "), "A B");
    }

    #[test]
    fn latin1() {
        assert_eq!(to_latin1("ÑA").ok(), Some(vec![0xD1, b'A']));
        assert_eq!(to_latin1("€"), Err(DrError::Encoding));
    }

    #[test]
    fn cabecera_de_328_posiciones() {
        let e = envelope("303", 2026, "3T", "0100", crate::DEVELOPER_NIF, &[]);
        let head = &e[..e.find("</AUX>").map_or(0, |i| i + 6)];
        assert_eq!(head.len(), 328);
        assert!(head.starts_with("<T30302026"));
        assert_eq!(&head[92..96], "0100");
        // En la versión pública (NIF de ejemplo) el NIF va en blanco: 9 espacios.
        assert_eq!(&head[100..109], format!("{:<9}", crate::DEVELOPER_NIF));
        assert!(e.ends_with("</T303020263T0000>"));
    }

    #[test]
    fn cambia_unos_campos_de_una_pagina_ya_escrita() {
        let s = crate::withholding::spec_111();
        let mut w = PageWriter::new(s.page("01").unwrap());
        w.text(6, "I").unwrap().text(7, "B12345674").unwrap();
        let file = envelope(
            "111",
            2026,
            "1T",
            "0100",
            crate::DEVELOPER_NIF,
            &[w.finish().unwrap()],
        );
        let patched = patch_page(s, &file, "01", |w| {
            w.text(42, "X")?.text(43, "1110000000014")?;
            Ok(())
        })
        .unwrap();
        assert_eq!(patched.len(), file.len());
        let at = patched.find("<T11101000>").unwrap();
        let page = s.page("01").unwrap();
        let record: String = patched[at..].chars().take(page.len()).collect();
        let r = PageReader::new(page, &record).unwrap();
        assert_eq!(r.raw(42).unwrap(), "X");
        assert_eq!(r.raw(43).unwrap(), "1110000000014");
        // Lo demás sigue igual.
        assert_eq!(r.raw(6).unwrap(), "I");
        assert_eq!(r.raw(7).unwrap(), "B12345674");
        assert!(patch_page(s, "<T1110>", "01", |_| Ok(())).is_err());
        let extra = append_page(&file, "<PAGINA>");
        assert!(extra.ends_with("<PAGINA></T111020261T0000>"));
    }

    #[test]
    fn los_recuentos_no_son_importes() {
        let field = |s: &Spec, b: u16| {
            s.pages
                .iter()
                .flat_map(|p| p.fields.iter())
                .find(|f| f.box_ == Some(b) && f.constant.is_none())
                .cloned()
                .unwrap()
        };
        // 123: número de rentas [01]–[03] (15 posiciones) y bases e importes desde [04].
        let s = crate::m123::spec();
        assert!((1..=3).all(|b| !field(s, b).is_amount()));
        assert!((4..=14).all(|b| field(s, b).is_amount()));
        // 115: número de perceptores [01].
        let s = crate::withholding::spec_115();
        assert!(!field(s, 1).is_amount());
        assert!(field(s, 2).is_amount());
    }
}
