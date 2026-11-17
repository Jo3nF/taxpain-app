//! Maqueta de las páginas de un PDF: cada letra con su posición, y con ellas las líneas (de
//! arriba abajo) y, en cada línea, los trozos de texto separados por un hueco de columna.
//!
//! Las posiciones se pasan enseguida a enteros, en décimas de punto, con el origen arriba a la
//! izquierda de la página (la `y` crece hacia abajo).

use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Mutex;

use pdf_extract::{MediaBox, OutputDev, OutputError, Transform};
use sha2::{Digest, Sha256};

/// Una letra (o un grupo de letras de un mismo glifo, como «fi») con su posición.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Letra {
    pub pagina: u32,
    /// Borde izquierdo y derecho, en décimas de punto.
    pub x0: i32,
    pub x1: i32,
    /// Línea de base, en décimas de punto desde arriba.
    pub y: i32,
    /// Tamaño de la letra, en décimas de punto.
    pub tam: i32,
    pub texto: String,
    /// ¿Había un espacio justo delante (en el mismo texto del PDF)?
    pub espacio_antes: bool,
    /// En qué dirección va el texto (en cuartos de vuelta).
    pub giro: u8,
}

/// Un trozo de una línea: una o varias palabras seguidas, sin un hueco de columna entre ellas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Trozo {
    pub x0: i32,
    pub x1: i32,
    pub texto: String,
    /// Las palabras del trozo, cada una con su posición.
    pub palabras: Vec<Palabra>,
}

/// Una palabra con su posición.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Palabra {
    pub x0: i32,
    pub x1: i32,
    pub texto: String,
}

/// Una línea de una página.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Linea {
    pub pagina: u32,
    pub y: i32,
    /// Tamaño de letra más habitual de la línea, en décimas de punto.
    pub tam: i32,
    pub trozos: Vec<Trozo>,
}

impl Linea {
    /// Texto de la línea con los trozos separados por tres espacios.
    pub(crate) fn texto(&self) -> String {
        self.trozos
            .iter()
            .map(|t| t.texto.as_str())
            .collect::<Vec<_>>()
            .join("   ")
    }

    /// Una línea hecha a mano con sus trozos (para las pruebas y para leer el texto sin
    /// posiciones).
    #[cfg(test)]
    pub(crate) fn de_trozos(pagina: u32, y: i32, trozos: &[(i32, &str)]) -> Self {
        let tam = 90;
        let trozos = trozos
            .iter()
            .map(|(x, t)| {
                let mut palabras = Vec::new();
                let mut px = *x;
                for p in t.split(' ').filter(|p| !p.is_empty()) {
                    let ancho = ancho_estimado(p, tam);
                    palabras.push(Palabra {
                        x0: px,
                        x1: px + ancho,
                        texto: p.to_owned(),
                    });
                    px += ancho + tam / 4;
                }
                Trozo {
                    x0: *x,
                    x1: palabras.last().map_or(*x, |p| p.x1),
                    texto: (*t).to_owned(),
                    palabras,
                }
            })
            .collect();
        Self {
            pagina,
            y,
            tam,
            trozos,
        }
    }
}

/// Ancho aproximado de un texto (media letra por carácter), para las líneas hechas a mano.
#[cfg(test)]
pub(crate) fn ancho_estimado(t: &str, tam: i32) -> i32 {
    i32::try_from(t.chars().count()).unwrap_or(0) * tam / 2
}

/// Posición más lejana que se admite, en décimas de punto (unos 350 metros).
const LIMITE: f64 = 10_000_000.0;

/// Ancho de una letra en Helvetica, en milésimas del tamaño de la letra (las métricas estándar
/// de la fuente; las letras con tilde, como su letra sin tilde). Sirve para rehacer el avance
/// de las letras a las que `pdf-extract` da ancho 0 (ver [`Recolector::avance`]); es aproximado.
fn ancho_helvetica(c: char) -> f64 {
    let base = match c {
        'á' | 'à' | 'ä' | 'â' | 'ª' => 'a',
        'é' | 'è' | 'ë' | 'ê' => 'e',
        'í' | 'ì' | 'ï' | 'î' => 'i',
        'ó' | 'ò' | 'ö' | 'ô' | 'º' => 'o',
        'ú' | 'ù' | 'ü' | 'û' => 'u',
        'ñ' => 'n',
        'ç' => 'c',
        'Á' | 'À' | 'Ä' | 'Â' => 'A',
        'É' | 'È' | 'Ë' | 'Ê' => 'E',
        'Í' | 'Ì' | 'Ï' | 'Î' => 'I',
        'Ó' | 'Ò' | 'Ö' | 'Ô' => 'O',
        'Ú' | 'Ù' | 'Ü' | 'Û' => 'U',
        'Ñ' => 'N',
        'Ç' => 'C',
        '€' | '«' | '»' => '0',
        '…' => '_',
        c => c,
    };
    f64::from(match base {
        ' ' | '!' | ',' | '.' | '/' | ':' | ';' | '[' | '\\' | ']' | 'f' | 't' | 'I' => 278,
        '"' => 355,
        '\'' | '`' | 'i' | 'j' | 'l' => 222,
        '(' | ')' | '-' | 'r' => 333,
        '*' => 389,
        '+' | '<' | '=' | '>' | '~' => 584,
        '%' => 889,
        '&' | 'A' | 'B' | 'E' | 'K' | 'P' | 'S' | 'V' | 'X' | 'Y' => 667,
        '@' => 1015,
        'C' | 'D' | 'H' | 'N' | 'R' | 'U' | 'w' => 722,
        'F' | 'T' | 'Z' => 611,
        'G' | 'O' | 'Q' => 778,
        'J' | 'c' | 'k' | 's' | 'v' | 'x' | 'y' | 'z' => 500,
        'L' => 556,
        'M' | 'm' => 833,
        'W' => 944,
        '^' => 469,
        '{' | '}' => 334,
        '|' => 260,
        // Cifras, minúsculas redondas, «#», «$», «?», «_» y lo que no está en la tabla.
        _ => 556,
    })
}

/// Recoge las letras que va dibujando `pdf-extract`.
#[derive(Debug, Default)]
struct Recolector {
    pagina: u32,
    alto: f64,
    letras: Vec<Letra>,
    espacio: bool,
    /// Avance que falta en el texto que se está dibujando, en la dirección del texto y en
    /// unidades de la página. `pdf-extract` 0.12 guarda las fuentes por su nombre y no vacía
    /// esa memoria hasta cambiar de página: si un formulario dibujado dentro de la página
    /// (XObject) tiene una fuente con el mismo nombre que una de la página, la página se lee con
    /// la del formulario y muchas letras salen con ancho 0. Con ancho 0 no avanza a la siguiente
    /// letra y las cifras se apilan en el mismo sitio («665» se leía «65»). Para esas letras se
    /// rehace el avance con el ancho de Helvetica, también el de los espacios.
    avance: f64,
}

impl OutputDev for Recolector {
    fn begin_page(
        &mut self,
        page_num: u32,
        media_box: &MediaBox,
        _art_box: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), OutputError> {
        self.pagina = page_num;
        self.alto = media_box.ury;
        Ok(())
    }

    fn end_page(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    // La geometría de la página llega en coma flotante: se pasa aquí a décimas de punto
    // enteras y ya no se usa más (nunca para importes).
    #[allow(clippy::float_arithmetic, clippy::cast_possible_truncation)]
    fn output_character(
        &mut self,
        trm: &Transform,
        width: f64,
        _spacing: f64,
        font_size: f64,
        char: &str,
    ) -> Result<(), OutputError> {
        // La dirección en la que avanza el texto: normal (0), girado un cuarto de vuelta (1),
        // del revés (2) o tres cuartos (3). Lo torcido (marcas de agua) no se lee. Las
        // posiciones se pasan a las de la lectura: `x` en el sentido del texto e `y` hacia
        // abajo de la línea.
        // Como en `pdf_text`: un PDF de fuera no puede llenar la memoria de letras.
        if self.letras.len() >= MAX_LETRAS {
            return Err(OutputError::FormatError(std::fmt::Error));
        }
        let (a, b, c, d) = (trm.m11, trm.m12, trm.m21, trm.m22);
        let n = (a * a + b * b).sqrt();
        if !n.is_finite() || n <= 0.0 {
            return Ok(());
        }
        // Lo que no avanzó `pdf-extract` en las letras anteriores de este texto.
        let (px, py) = (trm.m31 + self.avance * a / n, trm.m32 + self.avance * b / n);
        let sin_ancho = width.is_nan() || width <= 0.0;
        // Ancho estimado de la letra en unidades de la página (cero si no se puede calcular).
        let estimado = {
            let e = char.chars().map(ancho_helvetica).sum::<f64>() / 1000.0 * font_size * n;
            if e.is_finite() { e.max(0.0) } else { 0.0 }
        };
        if char.trim().is_empty() {
            if sin_ancho {
                self.avance += estimado;
            }
            self.espacio = true;
            return Ok(());
        }
        let giro = if b.abs() <= a.abs() * 0.2 {
            if a > 0.0 { 0 } else { 2 }
        } else if a.abs() <= b.abs() * 0.2 {
            if b > 0.0 { 1 } else { 3 }
        } else {
            return Ok(());
        };
        let escala = (a * d - b * c).abs().sqrt();
        let tam = font_size * escala;
        if !tam.is_finite() || tam <= 0.0 || !trm.m31.is_finite() || !trm.m32.is_finite() {
            return Ok(());
        }
        let ancho = if sin_ancho {
            self.avance += estimado;
            estimado
        } else {
            width * font_size * n
        };
        let rx = (px * a + py * b) / n;
        let ry = self.alto + (px * b - py * a) / n;
        // Acotadas, para que las cuentas con posiciones absurdas (PDF rotos) no desborden.
        let entero = |v: f64, max: f64| (v * 10.0).round().clamp(-max, max) as i32;
        let x0 = entero(rx, LIMITE);
        let x1 = entero(rx + ancho, LIMITE);
        let y = entero(ry, LIMITE);
        self.letras.push(Letra {
            pagina: self.pagina,
            x0,
            x1: x1.max(x0 + 1),
            y,
            tam: entero(tam, 100_000.0).max(1),
            texto: char.to_owned(),
            espacio_antes: self.espacio,
            giro,
        });
        self.espacio = false;
        Ok(())
    }

    fn begin_word(&mut self) -> Result<(), OutputError> {
        // Cada texto del PDF empieza aparte: no se sabe si hay un espacio delante, y su primera
        // letra está donde la pone el PDF.
        self.espacio = false;
        self.avance = 0.0;
        Ok(())
    }

    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }

    fn end_line(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
}

/// Cuántas maquetas recientes se guardan por huella del PDF, como hace `pdf_text` con el
/// texto: la lectura de la carpeta mira el mismo PDF varias veces (al clasificarlo, al revisar,
/// al importar) y un PDF que no es un extracto no se guarda en la caché de lecturas, así que
/// sin esto se volvía a sacar la maqueta entera cada vez.
const CACHE: usize = 32;
/// Tope de letras guardadas en total (unas 2.000 páginas densas), para no crecer sin límite.
const MAX_LETRAS: usize = 2_000_000;

static RECIENTES: Mutex<VecDeque<(String, Option<Vec<Letra>>)>> = Mutex::new(VecDeque::new());

/// Letras de un PDF con su posición. `None` si `pdf-extract` no puede leerlo (o entra en
/// pánico, como con algunos PDF raros) o si no tiene texto. Las últimas se guardan por huella.
pub(crate) fn letras(bytes: &[u8]) -> Option<Vec<Letra>> {
    let clave = hex::encode(Sha256::digest(bytes));
    if let Ok(c) = RECIENTES.lock()
        && let Some((_, l)) = c.iter().find(|(k, _)| *k == clave)
    {
        return l.clone();
    }
    let out = letras_sin_cache(bytes);
    if let Ok(mut c) = RECIENTES.lock() {
        let cuenta = |l: &Option<Vec<Letra>>| l.as_ref().map_or(0, Vec::len);
        let nuevas = cuenta(&out);
        let mut total: usize = c.iter().map(|(_, l)| cuenta(l)).sum();
        while let Some((_, primero)) = c.front()
            && (c.len() >= CACHE || total + nuevas > MAX_LETRAS)
        {
            total -= cuenta(primero);
            c.pop_front();
        }
        if nuevas <= MAX_LETRAS {
            c.push_back((clave, out.clone()));
        }
    }
    out
}

/// Si la maqueta de ese PDF está en la caché (para las pruebas).
#[cfg(test)]
pub(crate) fn en_cache(bytes: &[u8]) -> bool {
    let clave = hex::encode(Sha256::digest(bytes));
    RECIENTES
        .lock()
        .is_ok_and(|c| c.iter().any(|(k, _)| *k == clave))
}

fn letras_sin_cache(bytes: &[u8]) -> Option<Vec<Letra>> {
    let r = catch_unwind(AssertUnwindSafe(|| -> Option<Vec<Letra>> {
        // Con los topes de tamaño y de descompresión de `pdf_text`.
        let doc = crate::pdf_text::abrir(bytes)?;
        let mut rec = Recolector::default();
        pdf_extract::output_doc(&doc, &mut rec).ok()?;
        Some(rec.letras)
    }));
    match r {
        Ok(Some(l)) if !l.is_empty() => Some(orientadas(l)),
        Ok(_) => None,
        Err(_) => {
            tracing::warn!("pdf-extract entró en pánico al sacar las posiciones de un PDF");
            None
        }
    }
}

/// En cada página, sólo las letras que van en la dirección de la mayoría (la del texto de la
/// página): las que van giradas, como los textos legales en vertical del margen, no son de
/// ninguna tabla.
fn orientadas(letras: Vec<Letra>) -> Vec<Letra> {
    let mut cuenta: Vec<(u32, [usize; 4])> = Vec::new();
    for l in &letras {
        let k = usize::from(l.giro.min(3));
        match cuenta.iter_mut().find(|(p, _)| *p == l.pagina) {
            Some((_, c)) => c[k] += 1,
            None => {
                let mut c = [0; 4];
                c[k] = 1;
                cuenta.push((l.pagina, c));
            }
        }
    }
    let giro_de = |p: u32| {
        cuenta
            .iter()
            .find(|(x, _)| *x == p)
            .and_then(|(_, c)| (0..4u8).max_by_key(|k| (c[usize::from(*k)], 4 - k)))
            .unwrap_or(0)
    };
    letras
        .into_iter()
        .filter(|l| l.giro == giro_de(l.pagina))
        .collect()
}

/// Líneas de las páginas, de la primera a la última y de arriba abajo.
pub(crate) fn lineas(letras: &[Letra]) -> Vec<Linea> {
    let mut ls: Vec<&Letra> = letras.iter().collect();
    ls.sort_by_key(|l| (l.pagina, l.y, l.x0));
    // Letras repetidas en el mismo sitio (negritas hechas pintando dos veces): una sola.
    let mut unicas: Vec<&Letra> = Vec::with_capacity(ls.len());
    for l in ls {
        let repetida = unicas.iter().rev().take(40).any(|u| {
            u.pagina == l.pagina
                && u.texto == l.texto
                && (u.x0 - l.x0).abs() <= 10
                && (u.y - l.y).abs() <= 10
        });
        if !repetida {
            unicas.push(l);
        }
    }
    let mut out = Vec::new();
    let mut grupo: Vec<&Letra> = Vec::new();
    for l in unicas {
        if let Some(primera) = grupo.first() {
            let tol = (primera.tam.max(l.tam) * 35 / 100).max(15);
            if primera.pagina != l.pagina || l.y - primera.y > tol {
                out.push(linea(&grupo));
                grupo.clear();
            }
        }
        grupo.push(l);
    }
    if !grupo.is_empty() {
        out.push(linea(&grupo));
    }
    out
}

/// Una línea con sus letras (de la misma página y a la misma altura).
fn linea(letras: &[&Letra]) -> Linea {
    let mut ls: Vec<&Letra> = letras.to_vec();
    ls.sort_by_key(|l| (l.x0, l.y));
    let tam = moda(ls.iter().map(|l| l.tam)).unwrap_or(90);
    let y = moda(ls.iter().map(|l| l.y)).unwrap_or(0);
    let pagina = ls.first().map_or(0, |l| l.pagina);
    // Palabras: se parte donde hay un espacio o un hueco de más de un sexto de la letra.
    let mut palabras: Vec<Palabra> = Vec::new();
    let mut fin_anterior: Option<i32> = None;
    for l in &ls {
        let hueco_palabra = (l.tam.max(tam) / 6).max(5);
        let nueva = match (fin_anterior, palabras.last()) {
            (Some(fin), Some(_)) => l.espacio_antes || l.x0 - fin > hueco_palabra,
            _ => true,
        };
        if nueva {
            palabras.push(Palabra {
                x0: l.x0,
                x1: l.x1,
                texto: l.texto.clone(),
            });
        } else if let Some(p) = palabras.last_mut() {
            p.texto.push_str(&l.texto);
            p.x1 = p.x1.max(l.x1);
        }
        fin_anterior = Some(fin_anterior.map_or(l.x1, |f| f.max(l.x1)));
    }
    // Trozos: se parte donde el hueco entre palabras es de columna (más de unas cuatro quintas
    // partes de la letra).
    let hueco_columna = (tam * 8 / 10).max(20);
    let mut trozos: Vec<Trozo> = Vec::new();
    for p in palabras {
        match trozos.last_mut() {
            Some(t) if p.x0 - t.x1 <= hueco_columna => {
                t.texto.push(' ');
                t.texto.push_str(&p.texto);
                t.x1 = t.x1.max(p.x1);
                t.palabras.push(p);
            }
            _ => trozos.push(Trozo {
                x0: p.x0,
                x1: p.x1,
                texto: p.texto.clone(),
                palabras: vec![p],
            }),
        }
    }
    Linea {
        pagina,
        y,
        tam,
        trozos,
    }
}

/// El valor más repetido (el menor si hay empate).
fn moda(v: impl Iterator<Item = i32>) -> Option<i32> {
    let mut v: Vec<i32> = v.collect();
    v.sort_unstable();
    let mut mejor: Option<(i32, usize)> = None;
    let mut i = 0;
    while i < v.len() {
        let j = v[i..]
            .iter()
            .position(|x| *x != v[i])
            .map_or(v.len(), |p| i + p);
        if mejor.is_none_or(|(_, n)| j - i > n) {
            mejor = Some((v[i], j - i));
        }
        i = j;
    }
    mejor.map(|(x, _)| x)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn letra(x0: i32, y: i32, t: &str) -> Letra {
        Letra {
            pagina: 1,
            x0,
            x1: x0 + 50,
            y,
            tam: 100,
            texto: t.to_owned(),
            espacio_antes: false,
            giro: 0,
        }
    }

    /// Letras de un texto escrito en `x`, a 5 puntos por letra.
    fn texto(x: i32, y: i32, t: &str) -> Vec<Letra> {
        let mut out = Vec::new();
        let mut espacio = false;
        for (i, c) in t.chars().enumerate() {
            if c == ' ' {
                espacio = true;
                continue;
            }
            let mut l = letra(x + i32::try_from(i).unwrap_or(0) * 50, y, &c.to_string());
            l.espacio_antes = espacio;
            espacio = false;
            out.push(l);
        }
        out
    }

    #[test]
    fn la_maqueta_se_guarda_por_huella() {
        // Un PDF mínimo sin texto: la maqueta es `None` y aun así se recuerda, para no volver
        // a abrir el PDF en la siguiente lectura.
        let pdf =
            b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog >>\nendobj\ntrailer\n<< /Root 1 0 R >>\n%%EOF";
        assert!(!en_cache(pdf));
        assert_eq!(letras(pdf), None);
        assert!(en_cache(pdf));
        assert_eq!(letras(pdf), None);
        assert!(!en_cache(b"otro fichero"));
    }

    #[test]
    fn lineas_y_trozos_por_la_posicion() {
        let mut ls = Vec::new();
        // Dibujadas en cualquier orden: primero la columna de la derecha.
        ls.extend(texto(4000, 1002, "1.234,56"));
        ls.extend(texto(500, 1000, "02/01/2025"));
        ls.extend(texto(1200, 1000, "RECIBO LUZ"));
        ls.extend(texto(500, 1150, "otra línea"));
        // Una negrita hecha pintando dos veces la misma letra.
        ls.push(letra(503, 1151, "o"));
        let l = lineas(&ls);
        assert_eq!(l.len(), 2);
        let textos: Vec<&str> = l[0].trozos.iter().map(|t| t.texto.as_str()).collect();
        assert_eq!(textos, ["02/01/2025", "RECIBO LUZ", "1.234,56"]);
        assert_eq!(l[0].trozos[1].palabras.len(), 2);
        assert_eq!(l[0].trozos[2].x0, 4000);
        assert_eq!(l[1].texto(), "otra línea");
        assert_eq!(l[0].texto(), "02/01/2025   RECIBO LUZ   1.234,56");
    }

    /// Un PDF hecho a mano: la página dibuja primero un formulario (XObject) y luego `texto` con
    /// la fuente /F1 (Helvetica). El formulario tiene su propia /F1, con ancho sólo para la «A»,
    /// como los subconjuntos de fuentes de los formularios de la AEAT.
    pub(crate) fn pdf_con_formulario(texto: &str) -> Vec<u8> {
        let contenido = format!("q /Fm0 Do Q BT /F1 10 Tf 100 700 Td ({texto}) Tj ET");
        let formulario = "BT /F1 10 Tf 100 750 Td (A) Tj ET";
        let objetos = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 4 0 \
             R >> /XObject << /Fm0 6 0 R >> >> /Contents 5 0 R >>"
                .to_owned(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
                .to_owned(),
            format!(
                "<< /Length {} >>\nstream\n{contenido}\nendstream",
                contenido.len() + 1
            ),
            format!(
                "<< /Type /XObject /Subtype /Form /BBox [0 0 595 842] /Resources << /Font << /F1 \
                 7 0 R >> >> /Length {} >>\nstream\n{formulario}\nendstream",
                formulario.len() + 1
            ),
            "<< /Type /Font /Subtype /Type1 /BaseFont /DEMO+Subconjunto /FirstChar 65 /LastChar \
             65 /Widths [667] /Encoding /WinAnsiEncoding >>"
                .to_owned(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut posiciones = Vec::new();
        for (i, o) in objetos.iter().enumerate() {
            posiciones.push(out.len());
            out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objetos.len() + 1).bytes());
        for p in posiciones {
            out.extend(format!("{p:010} 00000 n \n").bytes());
        }
        out.extend(
            format!(
                "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objetos.len() + 1
            )
            .bytes(),
        );
        out
    }

    /// Con la fuente del formulario, las cifras de la página salen con ancho 0: sin rehacer su
    /// avance se apilaban en el mismo sitio y «665» se quedaba en «65».
    #[test]
    fn las_letras_sin_ancho_no_se_apilan() {
        let pdf = pdf_con_formulario("Casilla 665 1.234,00");
        let l = lineas(&letras(&pdf).unwrap());
        let textos: Vec<String> = l.iter().map(Linea::texto).collect();
        assert!(textos.contains(&"A".to_owned()), "{textos:?}");
        let fila = l.iter().find(|x| x.texto() != "A").unwrap();
        let palabras: Vec<&str> = fila
            .trozos
            .iter()
            .flat_map(|t| t.palabras.iter().map(|p| p.texto.as_str()))
            .collect();
        assert_eq!(palabras, ["Casilla", "665", "1.234,00"]);
        // Las posiciones son las de Helvetica: «Casilla » mide unos 3,6 puntos por letra.
        let p = &fila.trozos[0].palabras;
        assert_eq!(p[0].x0, 1000);
        assert!((1300..1400).contains(&p[1].x0), "{}", p[1].x0);
    }

    #[test]
    fn un_pdf_raro_no_tumba_la_lectura() {
        assert_eq!(letras(b"%PDF-1.4 esto no es un PDF"), None);
        assert_eq!(letras(&crate::pdf_text::tests::panicking_pdf()), None);
    }
}
