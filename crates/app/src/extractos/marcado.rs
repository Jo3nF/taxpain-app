//! Tablas en HTML y en el XML de Excel 2003 (SpreadsheetML), que muchos bancos descargan con
//! la extensión `.xls`.
//!
//! Los dos lectores son sencillos y sin dependencias: un recorrido de las etiquetas que no
//! distingue mayúsculas, acepta los atributos con comillas simples, dobles o sin ellas y
//! entiende las entidades (`&nbsp;`, `&aacute;`, `&#243;`, `&#xF3;`…). Del HTML se sacan las
//! tablas (`<table>`, `<tr>`, `<td>`/`<th>`) con sus celdas combinadas (`colspan`, `rowspan`)
//! ya desplegadas, para que cada valor quede en su columna; del SpreadsheetML, las hojas
//! (`Worksheet`, `Row`, `Cell`, `Data`) con sus saltos (`ss:Index`) y celdas combinadas
//! (`ss:MergeAcross`).

use crate::import::Hoja;

/// Un trozo del marcado: una etiqueta que abre (o que se cierra sola), una que cierra o texto.
#[derive(Debug)]
enum Pieza<'a> {
    Abre {
        /// En minúsculas y sin el prefijo del espacio de nombres («ss:Data» → «data»).
        nombre: String,
        /// Los atributos, con el nombre igual que el de la etiqueta.
        atributos: Vec<(String, String)>,
        sola: bool,
    },
    Cierra(String),
    Texto(&'a str),
}

/// Recorre las etiquetas y el texto de un HTML o un XML. En HTML se salta el contenido de
/// `<script>` y `<style>`; en los dos, los comentarios, `<!DOCTYPE>` y `<?xml …?>`.
#[derive(Debug)]
struct Piezas<'a> {
    texto: &'a str,
    /// El mismo texto en minúsculas ASCII (las posiciones no cambian), para buscar cierres.
    bajo: String,
    pos: usize,
    html: bool,
}

impl<'a> Piezas<'a> {
    fn new(texto: &'a str, html: bool) -> Self {
        Self {
            texto,
            bajo: texto.to_ascii_lowercase(),
            pos: 0,
            html,
        }
    }

    /// Posición siguiente a `fin` buscado desde `desde`, o el final del texto.
    fn tras(&self, desde: usize, fin: &str) -> usize {
        self.bajo
            .get(desde..)
            .and_then(|r| r.find(fin))
            .map_or(self.texto.len(), |i| desde + i + fin.len())
    }
}

/// Nombre sin el prefijo del espacio de nombres y en minúsculas.
fn local(nombre: &str) -> String {
    nombre
        .rsplit(':')
        .next()
        .unwrap_or(nombre)
        .to_ascii_lowercase()
}

/// Atributos de una etiqueta (lo que hay entre el nombre y el `>`).
fn atributos(s: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut chars = s.char_indices().peekable();
    while let Some(&(i, c)) = chars.peek() {
        if c.is_whitespace() || c == '/' {
            chars.next();
            continue;
        }
        // El nombre, hasta un espacio, `=` o el final.
        let mut fin = i;
        while let Some(&(j, c)) = chars.peek() {
            if c.is_whitespace() || c == '=' || c == '/' {
                break;
            }
            fin = j + c.len_utf8();
            chars.next();
        }
        let nombre = local(&s[i..fin]);
        while chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
            chars.next();
        }
        let mut valor = String::new();
        if chars.peek().is_some_and(|(_, c)| *c == '=') {
            chars.next();
            while chars.peek().is_some_and(|(_, c)| c.is_whitespace()) {
                chars.next();
            }
            match chars.peek().map(|(_, c)| *c) {
                Some(q @ ('"' | '\'')) => {
                    chars.next();
                    for (_, c) in chars.by_ref() {
                        if c == q {
                            break;
                        }
                        valor.push(c);
                    }
                }
                _ => {
                    while let Some(&(_, c)) = chars.peek() {
                        if c.is_whitespace() {
                            break;
                        }
                        valor.push(c);
                        chars.next();
                    }
                }
            }
        }
        if nombre.is_empty() {
            // Un `=` suelto: se descarta para no quedarse dando vueltas.
            chars.next();
            continue;
        }
        out.push((nombre, entidades(&valor)));
    }
    out
}

impl<'a> Iterator for Piezas<'a> {
    type Item = Pieza<'a>;

    fn next(&mut self) -> Option<Pieza<'a>> {
        let t = self.texto;
        let b = t.as_bytes();
        loop {
            if self.pos >= b.len() {
                return None;
            }
            if b[self.pos] != b'<' {
                let fin = t[self.pos..].find('<').map_or(b.len(), |i| self.pos + i);
                let s = &t[self.pos..fin];
                self.pos = fin;
                return Some(Pieza::Texto(s));
            }
            let resto = &self.bajo[self.pos..];
            if resto.starts_with("<!--") {
                self.pos = self.tras(self.pos + 4, "-->");
                continue;
            }
            if resto.starts_with("<![cdata[") {
                let ini = self.pos + 9;
                let fin = self.bajo[ini..].find("]]>").map_or(b.len(), |i| ini + i);
                self.pos = (fin + 3).min(b.len());
                return Some(Pieza::Texto(&t[ini..fin]));
            }
            if resto.starts_with("<!") || resto.starts_with("<?") {
                self.pos = self.tras(self.pos + 2, ">");
                continue;
            }
            let cierra = resto.starts_with("</");
            let ini = self.pos + if cierra { 2 } else { 1 };
            let nombre_fin = t[ini..]
                .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, ':' | '-' | '_')))
                .map_or(b.len(), |i| ini + i);
            if nombre_fin == ini || !b[ini].is_ascii_alphabetic() {
                // Un `<` que no abre ninguna etiqueta («a < b»): es texto.
                let s = &t[self.pos..self.pos + 1];
                self.pos += 1;
                return Some(Pieza::Texto(s));
            }
            // El `>` del final, sin contar los que vayan dentro de comillas.
            let mut fin = nombre_fin;
            let mut comilla: Option<u8> = None;
            while fin < b.len() {
                match (comilla, b[fin]) {
                    (None, b'>') => break,
                    (None, q @ (b'"' | b'\'')) => comilla = Some(q),
                    (Some(q), c) if c == q => comilla = None,
                    _ => {}
                }
                fin += 1;
            }
            let nombre = local(&t[ini..nombre_fin]);
            let dentro = &t[nombre_fin..fin.min(b.len())];
            self.pos = (fin + 1).min(b.len());
            if cierra {
                return Some(Pieza::Cierra(nombre));
            }
            let sola = dentro.trim_end().ends_with('/');
            if self.html && !sola && (nombre == "script" || nombre == "style") {
                let cierre = format!("</{nombre}");
                let p = self.tras(self.pos, &cierre);
                self.pos = self.tras(p, ">");
                continue;
            }
            return Some(Pieza::Abre {
                atributos: atributos(dentro),
                nombre,
                sola,
            });
        }
    }
}

/// Entidades con nombre de HTML: las de Latin-1 y las tipográficas más habituales.
const ENTIDADES: [(&str, char); 130] = [
    ("nbsp", ' '),
    ("amp", '&'),
    ("lt", '<'),
    ("gt", '>'),
    ("quot", '"'),
    ("apos", '\''),
    ("iexcl", '¡'),
    ("cent", '¢'),
    ("pound", '£'),
    ("curren", '¤'),
    ("yen", '¥'),
    ("brvbar", '¦'),
    ("sect", '§'),
    ("uml", '¨'),
    ("copy", '©'),
    ("ordf", 'ª'),
    ("laquo", '«'),
    ("not", '¬'),
    ("shy", '\u{ad}'),
    ("reg", '®'),
    ("macr", '¯'),
    ("deg", '°'),
    ("plusmn", '±'),
    ("sup2", '²'),
    ("sup3", '³'),
    ("acute", '´'),
    ("micro", 'µ'),
    ("para", '¶'),
    ("middot", '·'),
    ("cedil", '¸'),
    ("sup1", '¹'),
    ("ordm", 'º'),
    ("raquo", '»'),
    ("frac14", '¼'),
    ("frac12", '½'),
    ("frac34", '¾'),
    ("iquest", '¿'),
    ("Agrave", 'À'),
    ("Aacute", 'Á'),
    ("Acirc", 'Â'),
    ("Atilde", 'Ã'),
    ("Auml", 'Ä'),
    ("Aring", 'Å'),
    ("AElig", 'Æ'),
    ("Ccedil", 'Ç'),
    ("Egrave", 'È'),
    ("Eacute", 'É'),
    ("Ecirc", 'Ê'),
    ("Euml", 'Ë'),
    ("Igrave", 'Ì'),
    ("Iacute", 'Í'),
    ("Icirc", 'Î'),
    ("Iuml", 'Ï'),
    ("ETH", 'Ð'),
    ("Ntilde", 'Ñ'),
    ("Ograve", 'Ò'),
    ("Oacute", 'Ó'),
    ("Ocirc", 'Ô'),
    ("Otilde", 'Õ'),
    ("Ouml", 'Ö'),
    ("times", '×'),
    ("Oslash", 'Ø'),
    ("Ugrave", 'Ù'),
    ("Uacute", 'Ú'),
    ("Ucirc", 'Û'),
    ("Uuml", 'Ü'),
    ("Yacute", 'Ý'),
    ("THORN", 'Þ'),
    ("szlig", 'ß'),
    ("agrave", 'à'),
    ("aacute", 'á'),
    ("acirc", 'â'),
    ("atilde", 'ã'),
    ("auml", 'ä'),
    ("aring", 'å'),
    ("aelig", 'æ'),
    ("ccedil", 'ç'),
    ("egrave", 'è'),
    ("eacute", 'é'),
    ("ecirc", 'ê'),
    ("euml", 'ë'),
    ("igrave", 'ì'),
    ("iacute", 'í'),
    ("icirc", 'î'),
    ("iuml", 'ï'),
    ("eth", 'ð'),
    ("ntilde", 'ñ'),
    ("ograve", 'ò'),
    ("oacute", 'ó'),
    ("ocirc", 'ô'),
    ("otilde", 'õ'),
    ("ouml", 'ö'),
    ("divide", '÷'),
    ("oslash", 'ø'),
    ("ugrave", 'ù'),
    ("uacute", 'ú'),
    ("ucirc", 'û'),
    ("uuml", 'ü'),
    ("yacute", 'ý'),
    ("thorn", 'þ'),
    ("yuml", 'ÿ'),
    ("euro", '€'),
    ("OElig", 'Œ'),
    ("oelig", 'œ'),
    ("Scaron", 'Š'),
    ("scaron", 'š'),
    ("Yuml", 'Ÿ'),
    ("fnof", 'ƒ'),
    ("circ", 'ˆ'),
    ("tilde", '˜'),
    ("ensp", ' '),
    ("emsp", ' '),
    ("thinsp", ' '),
    ("ndash", '–'),
    ("mdash", '—'),
    ("lsquo", '‘'),
    ("rsquo", '’'),
    ("sbquo", '‚'),
    ("ldquo", '“'),
    ("rdquo", '”'),
    ("bdquo", '„'),
    ("dagger", '†'),
    ("Dagger", '‡'),
    ("bull", '•'),
    ("hellip", '…'),
    ("permil", '‰'),
    ("lsaquo", '‹'),
    ("rsaquo", '›'),
    ("trade", '™'),
    ("minus", '−'),
];

/// Caracteres de control C1 (U+0080–U+009F) leídos como Latin-1 de un fichero en Windows-1252:
/// en el texto de una tabla no tienen sentido, así que se pasan al carácter que eran («€»…).
pub(crate) fn windows_1252(c: char) -> char {
    const TABLA: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    let n = u32::from(c);
    if (0x80..0xA0).contains(&n) {
        usize::try_from(n - 0x80)
            .ok()
            .and_then(|i| TABLA.get(i))
            .copied()
            .unwrap_or(c)
    } else {
        c
    }
}

/// Texto con las entidades de HTML y XML ya convertidas. Las que no se conocen se dejan.
fn entidades(s: &str) -> String {
    if !s.contains('&') {
        return s.chars().map(windows_1252).collect();
    }
    let mut out = String::with_capacity(s.len());
    let mut resto = s;
    while let Some(i) = resto.find('&') {
        out.extend(resto[..i].chars().map(windows_1252));
        let tras = &resto[i + 1..];
        // El «;» sólo en los once bytes siguientes: buscarlo hasta el final del texto hacía el
        // lector cuadrático con una celda larga llena de «&» sueltos (minutos con 2 MB).
        let fin = tras.bytes().take(11).position(|b| b == b';').filter(|f| {
            tras[..*f]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '#')
        });
        let Some(fin) = fin else {
            out.push('&');
            resto = tras;
            continue;
        };
        let nombre = &tras[..fin];
        let c = if let Some(num) = nombre.strip_prefix('#') {
            let n = match num.strip_prefix(['x', 'X']) {
                Some(h) => u32::from_str_radix(h, 16).ok(),
                None => num.parse::<u32>().ok(),
            };
            n.and_then(char::from_u32).map(windows_1252)
        } else {
            ENTIDADES
                .iter()
                .find(|(n, _)| *n == nombre)
                .or_else(|| {
                    ENTIDADES
                        .iter()
                        .find(|(n, _)| n.eq_ignore_ascii_case(nombre))
                })
                .map(|(_, c)| *c)
        };
        match c {
            Some(c) => {
                out.push(c);
                resto = &tras[fin + 1..];
            }
            None => {
                out.push('&');
                resto = tras;
            }
        }
    }
    out.extend(resto.chars().map(windows_1252));
    out
}

/// ¿Es un número sin formato («-1234.5», «45678»)?
fn es_numero(v: &str) -> bool {
    let t = v.strip_prefix('-').unwrap_or(v);
    let (e, d) = t.split_once('.').unwrap_or((t, ""));
    !e.is_empty() && e.bytes().all(|b| b.is_ascii_digit()) && d.bytes().all(|b| b.is_ascii_digit())
}

/// Número de un atributo (`colspan="2"`), entre 1 y `max`.
fn numero(atributos: &[(String, String)], nombre: &str, max: usize) -> usize {
    atributos
        .iter()
        .find(|(n, _)| n == nombre)
        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
        .filter(|n| *n >= 1)
        .map_or(1, |n| n.min(max))
}

/// Una celda abierta de una tabla HTML.
#[derive(Debug, Default)]
struct Celda {
    texto: String,
    colspan: usize,
    rowspan: usize,
    /// El número sin formato que Excel pone en `x:num` («-1234.5» de una celda que enseña
    /// «1.234,50» en rojo): es más fiable que el texto.
    numero: Option<String>,
}

/// Una celda de una fila de arriba que, por su `rowspan`, también ocupa filas de abajo.
#[derive(Debug)]
struct Ocupada {
    columna: usize,
    filas: usize,
    texto: String,
}

/// Una tabla HTML a medio leer.
#[derive(Debug, Default)]
struct Tabla {
    orden: usize,
    filas: Vec<Vec<String>>,
    fila: Option<Vec<String>>,
    celda: Option<Celda>,
    /// Celdas de arriba que bajan a la fila actual.
    ocupadas: Vec<Ocupada>,
    /// Las que abre la fila actual, para las siguientes.
    nuevas: Vec<Ocupada>,
}

impl Tabla {
    /// Pone en la fila las celdas de arriba que ocupan la columna siguiente; con `hasta`,
    /// rellena hasta esa columna (con celdas vacías donde no baja ninguna).
    fn rellenar(&mut self, hasta: Option<usize>) {
        let Some(fila) = self.fila.as_mut() else {
            return;
        };
        loop {
            let col = fila.len();
            if hasta.is_some_and(|h| col >= h) {
                return;
            }
            match self.ocupadas.iter().find(|o| o.columna == col) {
                Some(o) => fila.push(o.texto.clone()),
                None if hasta.is_some() => fila.push(String::new()),
                None => return,
            }
        }
    }

    fn cerrar_celda(&mut self) {
        let Some(c) = self.celda.take() else {
            return;
        };
        if self.fila.is_none() {
            self.fila = Some(Vec::new());
        }
        self.rellenar(None);
        let Some(fila) = self.fila.as_mut() else {
            return;
        };
        let texto = c.numero.unwrap_or_else(|| limpiar(&c.texto));
        let col = fila.len();
        if c.rowspan > 1 {
            for k in 0..c.colspan {
                self.nuevas.push(Ocupada {
                    columna: col + k,
                    filas: c.rowspan - 1,
                    texto: if k == 0 { texto.clone() } else { String::new() },
                });
            }
        }
        fila.push(texto);
        fila.extend(std::iter::repeat_n(String::new(), c.colspan - 1));
    }

    fn cerrar_fila(&mut self) {
        self.cerrar_celda();
        if self.fila.is_none() {
            return;
        }
        // Las celdas de arriba que quedan detrás de la última de esta fila.
        if let Some(u) = self.ocupadas.iter().map(|o| o.columna + 1).max() {
            self.rellenar(Some(u));
        }
        if let Some(f) = self.fila.take() {
            self.filas.push(f);
        }
        for o in &mut self.ocupadas {
            o.filas -= 1;
        }
        self.ocupadas.retain(|o| o.filas > 0);
        self.ocupadas.append(&mut self.nuevas);
    }
}

/// Texto de una celda: sin espacios delante ni detrás (en cada línea).
fn limpiar(s: &str) -> String {
    s.split('\n')
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

/// Añade a una celda el texto de la página: los saltos de línea y tabuladores del código
/// fuente son un espacio, como en el navegador (los de `<br>` se ponen aparte).
fn añadir(celda: &mut Celda, texto: &str) {
    let mut previo_blanco = celda.texto.is_empty() || celda.texto.ends_with([' ', '\n']);
    let mut tras_salto = false;
    for c in entidades(texto).chars() {
        let c = if c == '\u{a0}' { ' ' } else { c };
        if matches!(c, '\r' | '\n' | '\t') {
            if !previo_blanco {
                celda.texto.push(' ');
                previo_blanco = true;
            }
            tras_salto = true;
            continue;
        }
        if c == ' ' && tras_salto {
            continue;
        }
        tras_salto = false;
        previo_blanco = c == ' ';
        celda.texto.push(c);
    }
}

/// Las tablas de una página HTML como hojas («tabla 1», «tabla 2»…, en el orden en que
/// empiezan), si las tiene.
pub(crate) fn hojas_html(texto: &str) -> Option<Vec<Hoja>> {
    if !texto.to_ascii_lowercase().contains("<table") {
        return None;
    }
    let mut pila: Vec<Tabla> = Vec::new();
    let mut hechas: Vec<Tabla> = Vec::new();
    let mut n = 0;
    let cerrar_tabla = |pila: &mut Vec<Tabla>, hechas: &mut Vec<Tabla>| {
        if let Some(mut t) = pila.pop() {
            t.cerrar_fila();
            hechas.push(t);
        }
    };
    for p in Piezas::new(texto, true) {
        match p {
            Pieza::Texto(s) => {
                if let Some(c) = pila.last_mut().and_then(|t| t.celda.as_mut()) {
                    añadir(c, s);
                }
            }
            Pieza::Abre {
                nombre, atributos, ..
            } => match nombre.as_str() {
                "table" => {
                    n += 1;
                    pila.push(Tabla {
                        orden: n,
                        ..Tabla::default()
                    });
                }
                "tr" => {
                    if let Some(t) = pila.last_mut() {
                        t.cerrar_fila();
                        t.fila = Some(Vec::new());
                    }
                }
                "td" | "th" => {
                    if let Some(t) = pila.last_mut() {
                        t.cerrar_celda();
                        if t.fila.is_none() {
                            t.fila = Some(Vec::new());
                        }
                        t.celda = Some(Celda {
                            texto: String::new(),
                            colspan: numero(&atributos, "colspan", 200),
                            rowspan: numero(&atributos, "rowspan", 1000),
                            numero: atributos
                                .iter()
                                .find(|(n, _)| n == "num")
                                .map(|(_, v)| v.trim().to_owned())
                                .filter(|v| es_numero(v)),
                        });
                    }
                }
                "br" | "p" | "div" | "li" => {
                    if let Some(c) = pila.last_mut().and_then(|t| t.celda.as_mut())
                        && !c.texto.trim().is_empty()
                    {
                        let sin_blancos = c.texto.trim_end_matches(' ').len();
                        c.texto.truncate(sin_blancos);
                        if nombre == "br" || !c.texto.ends_with('\n') {
                            c.texto.push('\n');
                        }
                    }
                }
                _ => {}
            },
            Pieza::Cierra(nombre) => match nombre.as_str() {
                "td" | "th" => {
                    if let Some(t) = pila.last_mut() {
                        t.cerrar_celda();
                    }
                }
                "tr" => {
                    if let Some(t) = pila.last_mut() {
                        t.cerrar_fila();
                    }
                }
                "table" => cerrar_tabla(&mut pila, &mut hechas),
                _ => {}
            },
        }
    }
    while !pila.is_empty() {
        cerrar_tabla(&mut pila, &mut hechas);
    }
    hechas.sort_by_key(|t| t.orden);
    let hojas: Vec<Hoja> = hechas
        .into_iter()
        .filter(|t| t.filas.iter().any(|f| f.iter().any(|c| !c.is_empty())))
        .map(|t| Hoja {
            nombre: format!("tabla {}", t.orden),
            filas: t.filas,
        })
        .collect();
    (!hojas.is_empty()).then_some(hojas)
}

/// ¿Es un libro en XML de Excel 2003?
fn es_spreadsheetml(texto: &str) -> bool {
    let cabeza: String = texto
        .chars()
        .take(8000)
        .collect::<String>()
        .to_ascii_lowercase();
    cabeza.contains("urn:schemas-microsoft-com:office:spreadsheet")
        && (cabeza.contains("<workbook") || cabeza.contains(":workbook"))
}

/// Las hojas de un libro en XML de Excel 2003, si lo es.
pub(crate) fn hojas_spreadsheetml(texto: &str) -> Option<Vec<Hoja>> {
    if !es_spreadsheetml(texto) {
        return None;
    }
    let mut hojas: Vec<Hoja> = Vec::new();
    let mut hoja: Option<Hoja> = None;
    let mut fila: Option<Vec<String>> = None;
    // Texto de la celda abierta, su tipo y cuántas columnas más ocupa.
    let mut celda: Option<(String, String, usize)> = None;
    let mut en_dato = false;
    let mut en_comentario = 0usize;
    for p in Piezas::new(texto, false) {
        match p {
            Pieza::Abre {
                nombre,
                atributos,
                sola,
            } => {
                let attr = |n: &str| {
                    atributos
                        .iter()
                        .find(|(k, _)| k == n)
                        .map(|(_, v)| v.as_str())
                };
                let indice = |n: &str| attr(n).and_then(|v| v.trim().parse::<usize>().ok());
                match nombre.as_str() {
                    "worksheet" => {
                        if let Some(h) = hoja.take() {
                            hojas.push(h);
                        }
                        hoja = Some(Hoja {
                            nombre: attr("name").unwrap_or_default().to_owned(),
                            filas: Vec::new(),
                        });
                    }
                    "row" => {
                        let Some(h) = hoja.as_mut() else { continue };
                        if let Some(i) = indice("index") {
                            while h.filas.len() + 1 < i.min(1_000_000) {
                                h.filas.push(Vec::new());
                            }
                        }
                        if sola {
                            h.filas.push(Vec::new());
                        } else {
                            fila = Some(Vec::new());
                        }
                    }
                    "cell" => {
                        let Some(f) = fila.as_mut() else { continue };
                        if let Some(i) = indice("index") {
                            while f.len() + 1 < i.min(16_384) {
                                f.push(String::new());
                            }
                        }
                        let mas = indice("mergeacross").unwrap_or(0).min(200);
                        if sola {
                            f.push(String::new());
                            f.extend(std::iter::repeat_n(String::new(), mas));
                        } else {
                            celda = Some((String::new(), String::new(), mas));
                        }
                    }
                    "data" if en_comentario == 0 && !sola => {
                        en_dato = true;
                        if let Some(c) = celda.as_mut() {
                            c.1 = attr("type").unwrap_or_default().to_ascii_lowercase();
                        }
                    }
                    "comment" if !sola => en_comentario += 1,
                    _ => {}
                }
            }
            Pieza::Texto(s) => {
                if en_dato
                    && en_comentario == 0
                    && let Some(c) = celda.as_mut()
                {
                    c.0.push_str(&entidades(s));
                }
            }
            Pieza::Cierra(nombre) => match nombre.as_str() {
                "data" => en_dato = false,
                "comment" => en_comentario = en_comentario.saturating_sub(1),
                "cell" => {
                    if let (Some((texto, tipo, mas)), Some(f)) = (celda.take(), fila.as_mut()) {
                        let texto = texto.trim().to_owned();
                        // Las fechas vienen como «2025-01-31T00:00:00.000»: basta el día.
                        let valor = match texto.split_once('T') {
                            Some((dia, _)) if tipo == "datetime" => dia.to_owned(),
                            _ => texto,
                        };
                        f.push(valor);
                        f.extend(std::iter::repeat_n(String::new(), mas));
                    }
                }
                "row" => {
                    if let (Some(f), Some(h)) = (fila.take(), hoja.as_mut()) {
                        h.filas.push(f);
                    }
                }
                "worksheet" => {
                    if let Some(h) = hoja.take() {
                        hojas.push(h);
                    }
                }
                _ => {}
            },
        }
    }
    if let Some(h) = hoja.take() {
        hojas.push(h);
    }
    (!hojas.is_empty()).then_some(hojas)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filas(h: &Hoja) -> Vec<Vec<&str>> {
        h.filas
            .iter()
            .map(|f| f.iter().map(String::as_str).collect())
            .collect()
    }

    #[test]
    fn tabla_html_con_celdas_combinadas_y_entidades() {
        // Como las de algunos bancos: sin <html>, etiquetas en mayúsculas y minúsculas,
        // comillas simples y dobles, y acentos en Latin-1 tal cual.
        let t = "<TABLE border='1'><TR><TD colspan='3'><b>N\u{fa}mero de cuenta:</b> 0000/00.0000001</TD></TR>\
                 <TR><TH> FECHA </TH><TH> CONCEPTO </TH><TH> IMPORTE </TH></TR>\
                 <tr><td>07/03/2016</td><td>COMISI&Oacute;N &amp; GASTOS&nbsp;</td><td>-5,00</td></tr>\
                 <tr><td colspan=\"2\"><b>Saldo total</b></td><td>10,00</td></tr></TABLE>";
        let h = hojas_html(t).unwrap();
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].nombre, "tabla 1");
        assert_eq!(
            filas(&h[0]),
            [
                vec!["Número de cuenta: 0000/00.0000001", "", ""],
                vec!["FECHA", "CONCEPTO", "IMPORTE"],
                vec!["07/03/2016", "COMISIÓN & GASTOS", "-5,00"],
                vec!["Saldo total", "", "10,00"],
            ]
        );
    }

    #[test]
    fn rowspan_saltos_y_lo_que_no_es_tabla() {
        let t = "<!DOCTYPE html><html><head><style>td { color: red }</style>\
                 <script>var x = '<table>';</script></head><body>\
                 <!-- <table><tr><td>comentario</td></tr></table> -->\
                 <table><thead><tr><th rowspan=2>Fecha</th><th>A</th><th>B</th></tr></thead>\
                 <tbody><tr><td>uno<br>dos</td><td>x</td></tr>\
                 <tr><td>&#8364; &#x20AC; &euro;</td><td>y</td><td>z</td></tr></tbody></table>\
                 <table><tr><td>  segunda\n   tabla  </td></tr></table></body></html>";
        let h = hojas_html(t).unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(
            filas(&h[0]),
            [
                vec!["Fecha", "A", "B"],
                vec!["Fecha", "uno\ndos", "x"],
                vec!["€ € €", "y", "z"],
            ]
        );
        assert_eq!(filas(&h[1]), [vec!["segunda tabla"]]);
        let h = hojas_html(
            "<table><tr><td x:num=\"-1234.5\">1.234,50</td><td x:num>7</td></tr></table>",
        )
        .unwrap();
        assert_eq!(filas(&h[0]), [vec!["-1234.5", "7"]]);
        assert_eq!(hojas_html("<html><body>sin tablas</body></html>"), None);
        assert_eq!(hojas_html("<table></table>"), None);
    }

    #[test]
    fn libro_en_xml_de_excel_2003() {
        let t = r#"<?xml version="1.0"?>
<?mso-application progid="Excel.Sheet"?>
<Workbook xmlns="urn:schemas-microsoft-com:office:spreadsheet"
 xmlns:ss="urn:schemas-microsoft-com:office:spreadsheet">
 <Styles><Style ss:ID="s1"/></Styles>
 <Worksheet ss:Name="Movimientos">
  <Table>
   <Row><Cell ss:MergeAcross="2"><Data ss:Type="String">Cuenta ES91 2100 0418 4502 0005 1332</Data></Cell></Row>
   <Row ss:Index="3"><Cell><Data ss:Type="String">Fecha</Data></Cell><Cell ss:Index="3"><Data ss:Type="String">Importe</Data></Cell></Row>
   <Row><Cell><Data ss:Type="DateTime">2025-01-31T00:00:00.000</Data></Cell>
    <Cell><Data ss:Type="String">Caf&#233; &amp; t&#xE9;</Data><Comment><Data>nota</Data></Comment></Cell>
    <Cell ss:StyleID="s1"><Data ss:Type="Number">-12.5</Data></Cell></Row>
  </Table>
 </Worksheet>
 <ss:Worksheet ss:Name="Vacía"><ss:Table/></ss:Worksheet>
</Workbook>"#;
        let h = hojas_spreadsheetml(t).unwrap();
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].nombre, "Movimientos");
        assert_eq!(
            filas(&h[0]),
            [
                vec!["Cuenta ES91 2100 0418 4502 0005 1332", "", ""],
                vec![],
                vec!["Fecha", "", "Importe"],
                vec!["2025-01-31", "Café & té", "-12.5"],
            ]
        );
        assert_eq!(h[1].nombre, "Vacía");
        assert_eq!(
            hojas_spreadsheetml("<table><tr><td>x</td></tr></table>"),
            None
        );
    }

    #[test]
    fn entidades_y_windows_1252() {
        assert_eq!(
            entidades("a&amp;b &lt;c&gt; &Ntilde;&uuml; &#39;x&#39;"),
            "a&b <c> Ñü 'x'"
        );
        assert_eq!(
            entidades("R&D & co &desconocida; &#x;"),
            "R&D & co &desconocida; &#x;"
        );
        assert_eq!(entidades("\u{80}10"), "€10");
    }

    #[test]
    fn muchos_ampersand_sueltos_se_leen_en_tiempo_lineal() {
        let s = format!("{} &amp; fin", "&".repeat(1_000_000));
        let t = std::time::Instant::now();
        let r = entidades(&s);
        assert_eq!(r.len(), 1_000_000 + 6);
        assert!(r.ends_with("&&& & fin"));
        assert!(
            t.elapsed()
                < std::time::Duration::from_secs(
                    5 * std::env::var("TAXPAIN_TEST_HOLGURA")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(1)
                ),
            "{:?}",
            t.elapsed()
        );
    }
}
