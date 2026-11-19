//! Texto de un PDF con `pdf-extract`, a salvo de sus pánicos.
//!
//! `pdf-extract` tiene muchos `unwrap` internos: un PDF raro (dañado, cifrado, con fuentes
//! extrañas) puede hacerlo entrar en pánico. Todo el texto de PDF de la app pasa por aquí: el
//! pánico se recoge y el PDF se trata como si no tuviera texto. Así un solo fichero no tumba la
//! lectura de una carpeta entera ni deja el almacén bloqueado.
//!
//! Como la lectura de la carpeta mira el mismo PDF con varios lectores (modelo, factura,
//! documento), los últimos textos se guardan por huella para no volver a leerlo.
//!
//! Los PDF llegan de fuera (una carpeta vigilada, un adjunto) y `pdf-extract` 0.12.1 lee con
//! `lopdf` 0.42, que descomprime cada flujo entero y sin tope (`FlateDecode`, `LZWDecode` y
//! `ASCII85Decode`, en cualquier orden; los demás filtros no los descomprime): los object
//! streams ya en `load_mem` (o en `decrypt` si el PDF va cifrado) y los contenidos de página al
//! sacar el texto. Por eso todo PDF se abre con [`abrir`], que pone estos topes:
//!
//! - no se abre un PDF de más de [`MAX_PDF`] bytes;
//! - antes de `load_mem` se repasa el fichero sin interpretar sus diccionarios (que se pueden
//!   escribir de mil maneras): desde cada palabra `stream` se prueba lo que sigue como zlib,
//!   deflate sin cabecera y LZW, también tras capas de `ASCII85Decode`, y lo que salga se
//!   vuelve a probar igual (filtros encadenados). Lo que crece cada descompresión se cuenta
//!   contra un presupuesto total de [`MAX_DESCOMPRIMIDO`] bytes, y lo que se lee contra otro
//!   proporcional al fichero, así que el repaso es lineal. Lo que no es de ese filtro falla en
//!   pocos bytes. Si se pasa, el PDF no se abre. Si va cifrado, se repite después de
//!   `load_mem` y antes de `decrypt`, con una copia descifrada de cada flujo;
//! - tampoco se abre si algún `/Columns`, `/Colors` o `/BitsPerComponent` es desmesurado: con
//!   un predictor PNG `lopdf` reservaría filas de ese tamaño aunque el flujo sea corto;
//! - tampoco si algún `[` o `<<` se anida más de 256 niveles fuera de cadenas y flujos (el
//!   analizador de `lopdf` es recursivo; él solo ya para en 100, esto es una segunda barrera);
//! - y, ya abierto (y descifrado), se acota el trabajo que hará `pdf-extract`, que rehace mucho
//!   por cada referencia u operador: `get_page_content` descomprime y concatena cada referencia
//!   de `/Contents` (con sus repeticiones); cada `Do` descomprime el formulario y lo ejecuta
//!   entero, sin tope de anidamiento ni de ciclos (uno que se llama a sí mismo desborda la pila,
//!   y eso aborta la app: `catch_unwind` no lo para); cada `cs`/`CS` descomprime su perfil ICC
//!   (o la función de un `Separation`); cada `gs` copia su `/SMask`; cada `q` copia el estado con
//!   esos datos; cada fuente se crea una vez por nombre y página, descomprimiendo y analizando
//!   sus flujos (`/ToUnicode`, `/Encoding`, `/FontFile*`) y expandiendo los `beginbfrange` del
//!   `/ToUnicode` código a código; y `/Resources` y `/MediaBox` se buscan subiendo por
//!   `/Parent` sin tope. Se mide una vez cada flujo y se resume una vez cada contenido (con el
//!   propio analizador de `lopdf`, tras comprobar que analizarlo no pide demasiada memoria);
//!   luego se multiplica por las veces que se ejecuta. No se abre si el trabajo total pasa de
//!   [`MAX_TRABAJO`], si una página tendría en memoria más de [`MAX_MEMORIA`], si hay demasiadas
//!   operaciones costosas, un ciclo de formularios, más de 16 uno dentro de otro, un `/Parent`
//!   en bucle o un CMap o una fuente Type1 anidados de más (`pdf-extract` los lee con `pom`, sin
//!   tope);
//! - además de cobrar cada caso, unos topes generales acotan clases enteras de lo que
//!   `pdf-extract` copia o guarda por operador: ningún operador de un contenido lleva más de
//!   [`MAX_OPERANDOS`] operandos, contando lo que hay dentro de sus arrays y diccionarios (salvo
//!   el array de `TJ` y la imagen en línea de `BI`, que no guarda), porque los de
//!   `sc`/`scn`/`SC`/`SCN` van al estado gráfico; no hay más de [`MAX_PILA`] `q` abiertos a la
//!   vez, sumando los de cada formulario a los que había al llamarlo; y el estado se cobra en
//!   memoria como `q` abiertos por el tamaño del estado más grande (y en trabajo, por cada `q`).
//!   Las rutas y el contenido marcado, que se van guardando, se cobran por trozo; lo demás que
//!   guarda es de tamaño fijo, una referencia o ya se cobra (fuentes, `/SMask`, ICC), y el texto
//!   va con tope;
//! - se deja de sacar texto en cuanto pasa de [`MAX_TEXTO`].
//!
//! Queda fuera lo que `lopdf` copia sin descomprimir: varios objetos con `/Length` que se
//! solapan copian cada uno su trozo del fichero (como mucho [`MAX_PDF`] cada uno). Lo cerraría
//! subir a `lopdf` ≥ 0.44, que exige otra versión de `pdf-extract`. Las cuentas de memoria son
//! estimaciones por lo alto (lo que ocupa cada objeto analizado, cada copia del estado, cada
//! código), no una medida; y no se han repasado a fondo la carga de `lopdf` (tablas `xref`,
//! object streams) ni lo que hacen `cff-parser` y `type1-encoding-parser` con los datos de una
//! fuente (se cuenta su tamaño). La solución de fondo es leer los ficheros de fuera en un
//! proceso hijo con límite de memoria y tiempo.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Mutex;

use pdf_extract::{Dictionary, Document, Object, ObjectId, Stream};
use sha2::{Digest, Sha256};

/// Cuántos textos recientes se guardan (los del fichero que se está mirando y, al importar, los
/// de los ficheros elegidos, que se miran dos veces: para ordenarlos y al importarlos).
const CACHE: usize = 32;

/// PDF más grande que se abre: una factura, un modelo o un extracto de banco ocupan mucho
/// menos; un escaneado grande no tiene texto que sacar.
pub const MAX_PDF: usize = 50 * 1024 * 1024;
/// Texto más largo que se saca de un PDF (unos miles de páginas de texto).
pub const MAX_TEXTO: usize = 16 * 1024 * 1024;
/// Lo más que se deja crecer lo que se descomprime de los flujos de un PDF (sobre lo que se lee
/// para descomprimirlo), sumados todos.
pub const MAX_DESCOMPRIMIDO: u64 = 256 * 1024 * 1024;

static RECENT: Mutex<VecDeque<(String, Option<String>)>> = Mutex::new(VecDeque::new());

/// Texto del PDF, o `None` si no se puede leer (o `pdf-extract` falla o entra en pánico).
pub fn text(bytes: &[u8]) -> Option<String> {
    let key = hex::encode(Sha256::digest(bytes));
    if let Ok(c) = RECENT.lock()
        && let Some((_, hit)) = c.iter().find(|(k, _)| *k == key)
    {
        return hit.clone();
    }
    let out = extract(bytes);
    if let Ok(mut c) = RECENT.lock() {
        if c.len() >= CACHE {
            c.pop_front();
        }
        c.push_back((key, out.clone()));
    }
    out
}

/// Salida de texto de `pdf-extract` que se corta al pasar de [`MAX_TEXTO`].
struct Topada<'a> {
    dentro: pdf_extract::PlainTextOutput<&'a mut String>,
    /// Lo escrito, por lo alto: cada letra y lo que se le pueda poner delante (un espacio o un
    /// salto de línea).
    escrito: usize,
    max: usize,
}

impl pdf_extract::OutputDev for Topada<'_> {
    fn begin_page(
        &mut self,
        page_num: u32,
        media_box: &pdf_extract::MediaBox,
        art_box: Option<(f64, f64, f64, f64)>,
    ) -> Result<(), pdf_extract::OutputError> {
        self.dentro.begin_page(page_num, media_box, art_box)
    }
    fn end_page(&mut self) -> Result<(), pdf_extract::OutputError> {
        self.dentro.end_page()
    }
    fn output_character(
        &mut self,
        trm: &pdf_extract::Transform,
        width: f64,
        spacing: f64,
        font_size: f64,
        char: &str,
    ) -> Result<(), pdf_extract::OutputError> {
        self.escrito = self.escrito.saturating_add(char.len() + 2);
        if self.escrito > self.max {
            return Err(pdf_extract::OutputError::FormatError(std::fmt::Error));
        }
        self.dentro
            .output_character(trm, width, spacing, font_size, char)
    }
    fn begin_word(&mut self) -> Result<(), pdf_extract::OutputError> {
        self.dentro.begin_word()
    }
    fn end_word(&mut self) -> Result<(), pdf_extract::OutputError> {
        self.dentro.end_word()
    }
    fn end_line(&mut self) -> Result<(), pdf_extract::OutputError> {
        self.escrito = self.escrito.saturating_add(1);
        self.dentro.end_line()
    }
}

/// Topes con los que se abre un PDF (en las pruebas, más bajos).
#[derive(Clone, Copy, Debug)]
struct Topes {
    max_pdf: usize,
    max_descomprimido: u64,
    max_trabajo: u64,
    max_memoria: u64,
    max_operaciones: u64,
}

impl Topes {
    const DE_VERDAD: Self = Self {
        max_pdf: MAX_PDF,
        max_descomprimido: MAX_DESCOMPRIMIDO,
        max_trabajo: MAX_TRABAJO,
        max_memoria: MAX_MEMORIA,
        max_operaciones: MAX_OPERACIONES,
    };
}

/// Abre un PDF de fuera con los topes del módulo, y lo descifra con la contraseña vacía si va
/// cifrado. `None` si es demasiado grande, si descomprimiría demasiado o si no se puede abrir.
/// Puede entrar en pánico (es `lopdf`): quien la llame tiene que recogerlo.
pub(crate) fn abrir(bytes: &[u8]) -> Option<Document> {
    abrir_con(bytes, Topes::DE_VERDAD)
}

fn abrir_con(bytes: &[u8], topes: Topes) -> Option<Document> {
    if bytes.len() > topes.max_pdf {
        tracing::warn!(
            "PDF de {} bytes: demasiado grande para abrirlo",
            bytes.len()
        );
        return None;
    }
    if anidamiento_razonable(bytes).is_err() {
        tracing::warn!("PDF con un anidamiento desmesurado: no se abre");
        return None;
    }
    let mut rep = Repaso::new(bytes.len(), topes.max_descomprimido);
    if predictores_razonables(bytes).is_err() || cabe_lo_comprimido(bytes, &mut rep).is_err() {
        tracing::warn!("PDF que descomprime demasiado: no se abre");
        return None;
    }
    let mut doc = Document::load_mem(bytes).ok()?;
    if doc.is_encrypted() {
        if cabe_lo_cifrado(&doc, &mut rep).is_err() {
            tracing::warn!("PDF cifrado que descomprime demasiado: no se abre");
            return None;
        }
        doc.decrypt("").ok()?;
    }
    if cabe_el_trabajo(&doc, topes).is_err() {
        tracing::warn!("PDF que haría trabajar demasiado a pdf-extract: no se abre");
        return None;
    }
    Some(doc)
}

/// Lo descomprimido de un PDF pasaría del presupuesto, o el repaso tendría que leer demasiado.
#[derive(Debug)]
struct Demasiado;

/// Trozo más grande que se pide de una vez a una fuente.
const TROZO: usize = 64 * 1024;
/// Primer trozo que se pide a una fuente (luego se dobla): lo que no es un flujo comprimido falla
/// en pocos bytes, y así cada intento fallido lee poco.
const PRIMER_TROZO: usize = 16;
/// Búfer de entrada de cada capa de `ASCII85Decode`.
const TROZO_A85: usize = 1024;
/// Capas de `ASCII85Decode` seguidas que se prueban como mucho. Cada capa alarga el texto un
/// 25 %, así que en 50 MB no caben más de unas 80; si hubiera más, el PDF no se abre.
const MAX_CAPAS_A85: usize = 128;
/// Lo más que admite `lopdf` en `/Columns`, `/Colors` y `/BitsPerComponent` de un predictor
/// PNG sin reservar demasiado (reserva dos filas de `Colors × BitsPerComponent / 8 × Columns`
/// bytes, sin mirar lo largo que es el flujo).
const MAX_COLUMNAS: i64 = 100_000;
const MAX_COLORES: i64 = 32;
const MAX_BITS: i64 = 32;

/// Lo que queda por gastar en un repaso.
struct Gasto {
    /// Bytes que aún puede crecer lo descomprimido (Flate y LZW) sobre lo leído, sumados todos
    /// los intentos.
    queda: u64,
    /// Bytes que aún se pueden leer, sumadas todas las fuentes de todos los intentos (también
    /// los fallidos y las capas de `ASCII85Decode`): con esto el repaso entero cuesta
    /// O(tamaño del fichero + presupuesto), aunque se pruebe desde cada `stream`.
    lectura: u64,
}

impl Gasto {
    fn leer(&mut self, n: usize) -> Result<(), Demasiado> {
        self.lectura = self.lectura.checked_sub(n as u64).ok_or(Demasiado)?;
        Ok(())
    }
    /// Cobra lo que lo descomprimido (`salida` bytes hasta ahora) pasa de lo leído para sacarlo
    /// (`entrada`), menos lo ya `cobrado`. Lo que no crece no se cobra: no hace bomba (`lopdf`
    /// tiene a la vez la entrada y la salida, y la entrada ya estaba), y así un flujo con muchos
    /// ceros, que LZW lee como LZW válido una y otra vez, no gasta el presupuesto.
    fn crecer(&mut self, cobrado: &mut u64, salida: usize, entrada: u64) -> Result<(), Demasiado> {
        let crece = (salida as u64).saturating_sub(entrada);
        if crece > *cobrado {
            self.queda = self.queda.checked_sub(crece - *cobrado).ok_or(Demasiado)?;
            *cobrado = crece;
        }
        Ok(())
    }
}

/// Algo de lo que se leen bytes, contándolos en el gasto. Devuelve 0 al acabar.
trait Fuente {
    fn leer(&mut self, buf: &mut [u8], g: &mut Gasto) -> Result<usize, Demasiado>;
}

/// Unos bytes tal cual (el fichero desde un `stream`, o lo que salió de otro intento).
struct Trozo<'a>(&'a [u8]);

impl Fuente for Trozo<'_> {
    fn leer(&mut self, buf: &mut [u8], g: &mut Gasto) -> Result<usize, Demasiado> {
        let n = buf.len().min(self.0.len());
        g.leer(n)?;
        buf[..n].copy_from_slice(&self.0[..n]);
        self.0 = &self.0[n..];
        Ok(n)
    }
}

/// `ASCII85Decode` sobre otra fuente, poco a poco y como en `lopdf` 0.42: se saltan los
/// espacios, `z` al principio de un grupo son cuatro ceros y se para en el primer carácter que
/// no vale (también en `~>`), completando el último grupo. Donde `lopdf` da error (una `z` a
/// media grupo, un grupo que se desborda) aquí se sigue o se para con lo que haya: sale lo
/// mismo o más, nunca menos.
struct A85<'a> {
    dentro: &'a mut dyn Fuente,
    ent: [u8; TROZO_A85],
    lo: usize,
    hi: usize,
    tam: usize,
    sin_entrada: bool,
    grupo: u32,
    cuenta: usize,
    pend: [u8; 4],
    pend_lo: usize,
    pend_hi: usize,
    acabado: bool,
}

impl<'a> A85<'a> {
    fn new(dentro: &'a mut dyn Fuente) -> Self {
        Self {
            dentro,
            ent: [0; TROZO_A85],
            lo: 0,
            hi: 0,
            tam: PRIMER_TROZO,
            sin_entrada: false,
            grupo: 0,
            cuenta: 0,
            pend: [0; 4],
            pend_lo: 0,
            pend_hi: 0,
            acabado: false,
        }
    }

    fn poner(&mut self, b: &[u8]) {
        self.pend[..b.len()].copy_from_slice(b);
        self.pend_lo = 0;
        self.pend_hi = b.len();
    }

    /// El último grupo, incompleto, como en `lopdf` (se rellena con `u`).
    fn terminar(&mut self) {
        if self.cuenta > 0 {
            let mut g = self.grupo;
            for _ in self.cuenta..5 {
                g = g.wrapping_mul(85).wrapping_add(84);
            }
            let n = self.cuenta - 1;
            self.poner(&g.to_be_bytes()[..n]);
        }
        self.cuenta = 0;
        self.acabado = true;
    }
}

impl Fuente for A85<'_> {
    fn leer(&mut self, buf: &mut [u8], g: &mut Gasto) -> Result<usize, Demasiado> {
        let mut n = 0;
        while n < buf.len() {
            if self.pend_lo < self.pend_hi {
                let k = (buf.len() - n).min(self.pend_hi - self.pend_lo);
                buf[n..n + k].copy_from_slice(&self.pend[self.pend_lo..self.pend_lo + k]);
                self.pend_lo += k;
                n += k;
                continue;
            }
            if self.acabado {
                break;
            }
            if self.lo == self.hi {
                if self.sin_entrada {
                    self.terminar();
                    continue;
                }
                self.hi = self.dentro.leer(&mut self.ent[..self.tam], g)?;
                self.lo = 0;
                self.tam = (self.tam * 2).min(TROZO_A85);
                self.sin_entrada = self.hi == 0;
                continue;
            }
            let c = self.ent[self.lo];
            self.lo += 1;
            match c {
                b'z' if self.cuenta == 0 => self.poner(&[0; 4]),
                b'z' => self.terminar(),
                c if c.is_ascii_whitespace() => {}
                b'!'..=b'u' => {
                    self.grupo = self
                        .grupo
                        .wrapping_mul(85)
                        .wrapping_add(u32::from(c - b'!'));
                    self.cuenta += 1;
                    if self.cuenta == 5 {
                        let b = self.grupo.to_be_bytes();
                        self.poner(&b);
                        self.grupo = 0;
                        self.cuenta = 0;
                    }
                }
                _ => self.terminar(),
            }
        }
        g.leer(n)?;
        Ok(n)
    }
}

/// Llama a `f` con `datos` tras `capas` capas de `ASCII85Decode`.
fn con_capas<R>(
    datos: &[u8],
    capas: usize,
    f: &mut dyn FnMut(&mut dyn Fuente) -> Result<R, Demasiado>,
) -> Result<R, Demasiado> {
    fn sobre<R>(
        fuente: &mut dyn Fuente,
        capas: usize,
        f: &mut dyn FnMut(&mut dyn Fuente) -> Result<R, Demasiado>,
    ) -> Result<R, Demasiado> {
        if capas == 0 {
            return f(fuente);
        }
        let mut a = A85::new(fuente);
        sobre(&mut a, capas - 1, f)
    }
    sobre(&mut Trozo(datos), capas, f)
}

/// Descompresores y búferes de un repaso, que se reutilizan de un intento a otro (crearlos en
/// cada `stream` costaría más que el propio intento).
struct Descompresores {
    zlib: flate2::Decompress,
    deflate: flate2::Decompress,
    /// `LZWDecode` con `/EarlyChange 1` (lo normal) y con `/EarlyChange 0`: como no se mira el
    /// diccionario, se prueban los dos.
    lzw: weezl::decode::Decoder,
    lzw_tardio: weezl::decode::Decoder,
    ent: Vec<u8>,
    sal: Vec<u8>,
}

/// Lo que haría `lopdf` con el primer filtro de la cadena, medido.
#[derive(Clone, Copy)]
enum Intento {
    /// `FlateDecode` con cabecera zlib.
    Zlib,
    /// La vuelta atrás de `Stream::decompress_zlib` de `lopdf` 0.42 cuando zlib no saca nada:
    /// deflate sin cabecera, sin los dos primeros bytes.
    Deflate,
    /// `LZWDecode` con `/EarlyChange 1` (lo normal).
    Lzw,
    /// `LZWDecode` con `/EarlyChange 0`.
    LzwTardio,
}

impl Descompresores {
    fn new() -> Self {
        use weezl::{BitOrder, decode::Decoder};
        Self {
            zlib: flate2::Decompress::new(true),
            deflate: flate2::Decompress::new(false),
            lzw: Decoder::with_tiff_size_switch(BitOrder::Msb, 8),
            lzw_tardio: Decoder::new(BitOrder::Msb, 8),
            ent: vec![0; TROZO],
            sal: vec![0; TROZO],
        }
    }

    /// Lo que sale de `f` con `intento`, contado en el gasto y guardado (para probar también
    /// los filtros siguientes de la cadena).
    fn probar(
        &mut self,
        intento: Intento,
        f: &mut dyn Fuente,
        g: &mut Gasto,
    ) -> Result<Vec<u8>, Demasiado> {
        let Self {
            zlib,
            deflate,
            lzw,
            lzw_tardio,
            ent,
            sal,
        } = self;
        match intento {
            Intento::Zlib => {
                zlib.reset(true);
                inflar(zlib, f, g, ent, sal)
            }
            Intento::Deflate => {
                let mut dos = [0u8; 2];
                let mut k = 0;
                while k < 2 {
                    let n = f.leer(&mut dos[k..], g)?;
                    if n == 0 {
                        return Ok(Vec::new());
                    }
                    k += n;
                }
                deflate.reset(false);
                inflar(deflate, f, g, ent, sal)
            }
            Intento::Lzw => {
                lzw.reset();
                deslzw(lzw, f, g, ent, sal)
            }
            Intento::LzwTardio => {
                lzw_tardio.reset();
                deslzw(lzw_tardio, f, g, ent, sal)
            }
        }
    }
}

/// Infla `f` con `d` hasta el final del flujo, el primer error o el final de la fuente (lo que
/// salga hasta ahí también cuenta, como en `lopdf`).
fn inflar(
    d: &mut flate2::Decompress,
    f: &mut dyn Fuente,
    g: &mut Gasto,
    ent: &mut [u8],
    sal: &mut [u8],
) -> Result<Vec<u8>, Demasiado> {
    use flate2::{FlushDecompress, Status};
    let mut out = Vec::new();
    let mut cobrado = 0;
    let (mut lo, mut hi, mut tam, mut fin) = (0, 0, PRIMER_TROZO, false);
    loop {
        if lo == hi && !fin {
            hi = f.leer(&mut ent[..tam], g)?;
            lo = 0;
            tam = (tam * 2).min(ent.len());
            fin = hi == 0;
        }
        let (i0, o0) = (d.total_in(), d.total_out());
        let modo = if fin {
            FlushDecompress::Finish
        } else {
            FlushDecompress::None
        };
        let r = d.decompress(&ent[lo..hi], sal, modo);
        let usado = (d.total_in() - i0) as usize;
        let hecho = (d.total_out() - o0) as usize;
        lo += usado;
        if hecho > 0 {
            out.extend_from_slice(&sal[..hecho]);
            g.crecer(&mut cobrado, out.len(), d.total_in())?;
        }
        match r {
            Ok(Status::StreamEnd) | Err(_) => break,
            Ok(_) if usado == 0 && hecho == 0 && (fin || lo < hi) => break,
            Ok(_) => {}
        }
    }
    Ok(out)
}

/// Lo mismo con `LZWDecode`, como `Stream::decompress_lzw` de `lopdf` 0.42 (`weezl`, MSB, 8
/// bits de inicio).
fn deslzw(
    d: &mut weezl::decode::Decoder,
    f: &mut dyn Fuente,
    g: &mut Gasto,
    ent: &mut [u8],
    sal: &mut [u8],
) -> Result<Vec<u8>, Demasiado> {
    let mut out = Vec::new();
    let mut cobrado = 0;
    let mut leido: u64 = 0;
    let (mut lo, mut hi, mut tam, mut fin) = (0, 0, PRIMER_TROZO, false);
    loop {
        if lo == hi && !fin {
            hi = f.leer(&mut ent[..tam], g)?;
            lo = 0;
            tam = (tam * 2).min(ent.len());
            fin = hi == 0;
        }
        let r = d.decode_bytes(&ent[lo..hi], sal);
        lo += r.consumed_in;
        leido += r.consumed_in as u64;
        if r.consumed_out > 0 {
            out.extend_from_slice(&sal[..r.consumed_out]);
            g.crecer(&mut cobrado, out.len(), leido)?;
        }
        match r.status {
            Ok(weezl::LzwStatus::Done) | Err(_) => break,
            Ok(_) if r.consumed_in == 0 && r.consumed_out == 0 && (fin || lo < hi) => break,
            Ok(_) => {}
        }
    }
    Ok(out)
}

/// Lectura que se permite en un repaso, por cada byte del fichero y del presupuesto. Un PDF
/// normal lee cada flujo una vez y poco más en los intentos que fallan.
const LECTURA_POR_BYTE: u64 = 4;
/// Y además, para los PDF pequeños.
const LECTURA_FIJA: u64 = 16 * 1024 * 1024;

/// Un repaso de lo que descomprimiría `lopdf`, con su gasto.
struct Repaso {
    gasto: Gasto,
    d: Descompresores,
    /// Lo que ha salido de un intento y falta probar con los filtros siguientes.
    pendientes: Vec<(Vec<u8>, Lzw)>,
}

impl Repaso {
    fn new(largo: usize, max_descomprimido: u64) -> Self {
        Self {
            gasto: Gasto {
                queda: max_descomprimido,
                lectura: LECTURA_POR_BYTE
                    .saturating_mul((largo as u64).saturating_add(max_descomprimido))
                    .saturating_add(LECTURA_FIJA),
            },
            d: Descompresores::new(),
            pendientes: Vec::new(),
        }
    }

    /// Prueba `datos` como el principio de un flujo con cualquier cadena de los filtros que
    /// descomprime `lopdf` 0.42 (`FlateDecode`, `LZWDecode` y `ASCII85Decode`, en cualquier
    /// orden y repetidos), sin mirar su diccionario: lo que sale de cada intento se cuenta y se
    /// vuelve a probar. Lo que no es de ese filtro falla en pocos bytes y apenas cuenta.
    fn flujo(&mut self, datos: &[u8]) -> Result<(), Demasiado> {
        self.capas(datos, Lzw::Las2)?;
        while let Some((v, lzw)) = self.pendientes.pop() {
            self.capas(&v, lzw)?;
        }
        Ok(())
    }

    /// Flate y LZW sobre `datos` tras 0, 1, 2… capas de `ASCII85Decode`, mientras salga algo.
    /// `ASCII85Decode` sólo no se cuenta: saca como mucho cuatro bytes por cada uno que lee (y
    /// sólo con `z`), así que no hace bomba; lo que importa es lo que infle el filtro siguiente.
    fn capas(&mut self, datos: &[u8], lzw: Lzw) -> Result<(), Demasiado> {
        let Self {
            gasto: g,
            d,
            pendientes,
        } = self;
        for capas in 0..=MAX_CAPAS_A85 + 1 {
            if capas > 0 {
                let algo = con_capas(datos, capas, &mut |f| Ok(f.leer(&mut [0u8; 1], g)? > 0))?;
                if !algo {
                    return Ok(());
                }
                if capas > MAX_CAPAS_A85 {
                    return Err(Demasiado);
                }
            }
            let mut probar = |i| con_capas(datos, capas, &mut |f| d.probar(i, f, g));
            let z = probar(Intento::Zlib)?;
            let z = if z.is_empty() {
                probar(Intento::Deflate)?
            } else {
                z
            };
            let temprano = if lzw == Lzw::Tardio {
                Vec::new()
            } else {
                probar(Intento::Lzw)?
            };
            let tardio = if lzw == Lzw::Temprano {
                Vec::new()
            } else {
                probar(Intento::LzwTardio)?
            };
            let siguen = if temprano == tardio {
                [(z, lzw), (temprano, Lzw::Las2), (Vec::new(), lzw)]
            } else {
                [(z, lzw), (temprano, Lzw::Temprano), (tardio, Lzw::Tardio)]
            };
            pendientes.extend(siguen.into_iter().filter(|(v, _)| !v.is_empty()));
        }
        Ok(())
    }
}

/// Qué variantes de `LZWDecode` quedan por probar en una cadena de filtros. `lopdf` usa el
/// mismo `/DecodeParms` (y por tanto el mismo `/EarlyChange`) para todos los filtros de la
/// cadena: tras un LZW, sólo se sigue probando el mismo. Sin esto, en datos con muchos ceros
/// (que LZW lee como LZW válido) cada nivel doblaría el trabajo.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Lzw {
    Las2,
    Temprano,
    Tardio,
}

/// Repasa el fichero tal cual, antes de que `lopdf` lo abra, sin interpretar nada: desde cada
/// palabra `stream` (salvo `endstream`) se prueba lo que sigue como datos de un flujo, por si
/// lo fuera. `lopdf` 0.42 empieza los datos tras `stream`, los espacios o tabuladores y un fin
/// de línea (CRLF, LF o CR); si no lo hay, se prueba igual justo detrás. Los datos se miden
/// hasta el final del fichero: el descompresor se para solo donde acaba el flujo, y así ni un
/// `/Length` ni un `endstream` falsos recortan lo que se mide. Se recorren todos, también los
/// que nadie use y los object streams, que `lopdf` descomprime al abrir.
fn cabe_lo_comprimido(bytes: &[u8], rep: &mut Repaso) -> Result<(), Demasiado> {
    let mut desde = 0;
    while let Some(p) = buscar(bytes, b"stream", desde) {
        desde = p + b"stream".len();
        if p >= 3 && &bytes[p - 3..p] == b"end" {
            continue;
        }
        let inicio = tras_fin_de_linea(bytes, desde);
        rep.flujo(&bytes[inicio..])?;
    }
    Ok(())
}

/// Lo mismo, para un PDF cifrado ya abierto y aún sin descifrar: `lopdf` no descomprime sus
/// object streams hasta `decrypt`, y en el fichero los flujos van cifrados (lo que se ha
/// probado antes no sirve). Se prueba una copia descifrada de cada flujo, también sin mirar sus
/// filtros.
fn cabe_lo_cifrado(doc: &Document, rep: &mut Repaso) -> Result<(), Demasiado> {
    let Ok(estado) = pdf_extract::EncryptionState::decode(doc, "") else {
        // Sin la contraseña vacía tampoco se podrá descifrar después.
        return Ok(());
    };
    let dict_cifrado = doc
        .trailer
        .get(b"Encrypt")
        .and_then(Object::as_reference)
        .ok();
    for (&id, obj) in &doc.objects {
        if Some(id) == dict_cifrado || !matches!(obj, Object::Stream(_)) {
            continue;
        }
        let mut copia = obj.clone();
        if pdf_extract::encryption::decrypt_object(&estado, id, &mut copia).is_err() {
            continue;
        }
        if let Object::Stream(s) = &copia {
            rep.flujo(&s.content)?;
        }
    }
    Ok(())
}

/// Ningún `/Columns`, `/Colors` ni `/BitsPerComponent` del fichero pasa de lo razonable. Con un
/// predictor PNG, `lopdf` 0.42 reserva (y llena de ceros) dos filas de `Colors ×
/// BitsPerComponent / 8 × Columns` bytes antes de mirar los datos: con unos pocos bytes de
/// flujo se podrían pedir gigas. Tampoco aquí se interpreta el diccionario: se mira cada
/// nombre del fichero (con los `#xx` resueltos, como `lopdf`) y el entero que lo siga (tras
/// espacios y comentarios). Un nombre así dentro de una cadena, o en otro sitio, sólo puede
/// hacer que no se abra un PDF raro.
fn predictores_razonables(bytes: &[u8]) -> Result<(), Demasiado> {
    let mut i = 0;
    while let Some(p) = bytes
        .get(i..)
        .and_then(|r| r.iter().position(|&c| c == b'/'))
    {
        let mut j = i + p + 1;
        let mut nombre = [0u8; 16];
        let mut n = 0;
        let mut largo = false;
        while j < bytes.len() && es_regular(bytes[j]) {
            let mut c = bytes[j];
            j += 1;
            if c == b'#'
                && let Some(h) = bytes.get(j..j + 2)
                && let Ok(h) = std::str::from_utf8(h)
                && let Ok(b) = u8::from_str_radix(h, 16)
            {
                c = b;
                j += 2;
            }
            if n < nombre.len() {
                nombre[n] = c;
                n += 1;
            } else {
                largo = true;
            }
        }
        i = j;
        if largo {
            continue;
        }
        let tope = match &nombre[..n] {
            b"Columns" => MAX_COLUMNAS,
            b"Colors" => MAX_COLORES,
            b"BitsPerComponent" => MAX_BITS,
            _ => continue,
        };
        let mut k = j;
        loop {
            match bytes.get(k) {
                Some(&c) if es_espacio(c) => k += 1,
                Some(b'%') => {
                    while bytes.get(k).is_some_and(|c| !b"\r\n".contains(c)) {
                        k += 1;
                    }
                }
                _ => break,
            }
        }
        if matches!(bytes.get(k), Some(b'+' | b'-')) {
            k += 1;
        }
        let mut v: i64 = 0;
        while let Some(&c) = bytes.get(k).filter(|c| c.is_ascii_digit()) {
            v = v.saturating_mul(10).saturating_add(i64::from(c - b'0'));
            k += 1;
        }
        if bytes[j..k].first() != Some(&b'-') && v > tope {
            return Err(Demasiado);
        }
        i = k.max(j);
    }
    Ok(())
}

/// Espacio para `lopdf` (también el NUL y el salto de página).
fn es_espacio(c: u8) -> bool {
    b" \t\n\r\0\x0C".contains(&c)
}

/// Letra de un nombre para `lopdf`: ni espacio ni delimitador.
fn es_regular(c: u8) -> bool {
    !es_espacio(c) && !b"()<>[]{}/%".contains(&c)
}

fn buscar(bytes: &[u8], aguja: &[u8], desde: usize) -> Option<usize> {
    bytes
        .get(desde..)?
        .windows(aguja.len())
        .position(|w| w == aguja)
        .map(|p| p + desde)
}

/// Dónde empiezan los datos de un flujo: tras `stream`, los espacios o tabuladores y el fin de
/// línea, como `space0` y `eol` de `lopdf` 0.42.
fn tras_fin_de_linea(bytes: &[u8], mut i: usize) -> usize {
    while matches!(bytes.get(i), Some(b' ' | b'\t')) {
        i += 1;
    }
    match (bytes.get(i), bytes.get(i + 1)) {
        (Some(b'\r'), Some(b'\n')) => i + 2,
        (Some(b'\r' | b'\n'), _) => i + 1,
        _ => i,
    }
}

// ---------------------------------------------------------------------------------------------
// Segunda comprobación: el trabajo que hará `pdf-extract` con el documento ya abierto.
// ---------------------------------------------------------------------------------------------

/// Lo más que se deja trabajar a `pdf-extract` con un PDF ya abierto: bytes que descomprime o
/// copia y fichas que analiza, sumadas todas las veces que lo hace (cada página, cada `Do`, cada
/// `cs`…). Un PDF normal se queda en unos pocos MB.
pub const MAX_TRABAJO: u64 = 512 * 1024 * 1024;
/// Lo más que puede tener `pdf-extract` en memoria a la vez al leer una página (su contenido
/// entero, ya analizado, los formularios que lleva dentro y sus fuentes), por lo alto.
pub const MAX_MEMORIA: u64 = 512 * 1024 * 1024;
/// Lo más de operaciones costosas que se le dejan hacer a `pdf-extract` (páginas, referencias de
/// `/Contents`, `Do`, `cs`, `CS`, `gs` y fuentes creadas, contadas cada vez que se hacen), y
/// también de pasos de esta comprobación.
const MAX_OPERACIONES: u64 = 4_000_000;
/// Formularios (`Do`) dentro de formularios. `pdf-extract` no tiene tope: un formulario que se
/// llama a sí mismo desborda la pila (y eso aborta la app, no se puede recoger).
const MAX_HONDURA_FORMAS: usize = 16;
/// Lo más largo que puede ser el camino de `/Parent` de una página: `pdf-extract` busca
/// `/Resources` y `/MediaBox` subiendo por él sin tope y sin mirar si da la vuelta.
const MAX_PADRES: usize = 128;
/// El `Document::DEREF_LIMIT` de `lopdf` 0.42 (que no es público).
const MAX_REFERENCIAS: usize = 128;
/// Anidamiento de `[` y `<<` que se admite en el fichero, fuera de cadenas y de flujos. `lopdf`
/// 0.42 ya para en 100 niveles (y en 100 paréntesis dentro de una cadena): esto es una segunda
/// barrera por si cambia.
const MAX_ANIDAMIENTO: usize = 256;
/// Anidamiento que se admite en un CMap o en una fuente Type1, que `pdf-extract` lee con `pom`
/// (`adobe-cmap-parser`, `type1-encoding-parser`) sin ningún tope: arrays, diccionarios,
/// procedimientos y paréntesis dentro de una cadena. Los de verdad no pasan de 3.
const MAX_ANIDAMIENTO_POM: usize = 32;
/// Nombres de fuente distintos que se siguen uno a uno por página; los demás se suman.
const MAX_NOMBRES: usize = 64;
/// Lo que ocupa cada ficha de un contenido analizado por `lopdf`, por lo alto: el `Object`, el
/// hueco que puede dejar el `Vec` al crecer y lo que se pide aparte.
const PESO_FICHA: u64 = 2 * size_of::<Object>() as u64 + 32;
/// Lo mismo para `pom` (`adobe_cmap_parser::Value`; el `Value` de `type1-encoding-parser` es
/// igual con un caso más).
const PESO_FICHA_POM: u64 = 2 * size_of::<adobe_cmap_parser::Value>() as u64 + 64;
/// Lo que ocupa cada código de un `/ToUnicode` en las dos tablas que hace `pdf-extract`.
const PESO_ENTRADA: u64 = 160;
/// Lo que ocupa cada copia del estado gráfico que guarda `q` (sin los datos de color).
const PESO_ESTADO: u64 = 1024;
/// Y lo que se le cobra en trabajo.
const TIEMPO_ESTADO: u64 = 64;
/// Lo más de operandos que se admite en un operador de un contenido (página o formulario),
/// contando también lo que hay dentro de los arrays y diccionarios que lleva. Ningún operador
/// de ISO 32000 lleva más de 6, salvo `sc`/`scn`/`SC`/`SCN`, con un número por componente (32
/// como mucho en un `DeviceN`, Annex C) y el nombre de un patrón; `d` lleva el array de rayas y
/// la fase, y `BDC`/`DP` una etiqueta y un diccionario de propiedades corto. `pdf-extract` copia
/// los operandos de `sc`/`scn`/`SC`/`SCN` al estado gráfico y cada `q` copia el estado, así que
/// sin este tope el coste de `q` crecería con el cuadrado del contenido. Quedan fuera el array de
/// `TJ` (se recorre sin guardarlo y en una línea larga lleva cientos de elementos) y el
/// diccionario de una imagen en línea (`BI`, que `pdf-extract` no mira): de esos solo cuenta que
/// sean un operando.
const MAX_OPERANDOS: u64 = 64;
/// Lo más de `q` sin su `Q` a la vez (la pila de estados de `pdf-extract`), en el orden en que se
/// ejecutan: un formulario llamado con `Do` empieza su propia pila, pero la de quien lo llama
/// sigue en memoria, así que se suman. ISO 32000-1 (Annex C) da 28 como límite habitual; se deja
/// mucho margen para los generadores que abren `q` sin cerrarlos.
const MAX_PILA: u64 = 1024;
/// Lo que ocupa cada trozo de ruta (`m`, `l`, `c`, `v`, `y`, `h`, `re`) que `pdf-extract` va
/// guardando (no la vacían `s`, `b`, `B`, `f*` ni `W`), con el hueco del `Vec` al crecer.
const PESO_RUTA: u64 = 2 * size_of::<pdf_extract::PathOp>() as u64;
/// Lo que ocupa cada `BMC`/`BDC` en la pila de contenido marcado (una referencia).
const PESO_MARCA: u64 = 2 * size_of::<usize>() as u64;

/// Qué sintaxis se lee en [`leer_sintaxis`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Sintaxis {
    /// Un contenido de página o de formulario, como lo lee `lopdf`.
    Contenido,
    /// Un CMap (`/ToUnicode`, `/Encoding`), como lo lee `adobe-cmap-parser`.
    CMap,
    /// Una fuente Type1 (`/FontFile`), como la lee `type1-encoding-parser`: también hay
    /// procedimientos entre llaves.
    Type1,
}

/// Lo que se sabe de unos datos sin interpretarlos.
#[derive(Default)]
struct Lectura {
    /// Objetos que pueden salir de analizarlos, por lo alto.
    fichas: u64,
    /// Anidamiento más hondo (arrays, diccionarios, llaves y paréntesis dentro de cadenas).
    hondura: usize,
}

/// Repasa unos datos de una sintaxis de PDF (o PostScript) de forma lineal: cuenta por lo alto
/// los objetos que saldrán de analizarlos y lo hondo que llega el anidamiento, fuera de cadenas
/// y comentarios. Se para donde no puede empezar nada (un byte que no es de la sintaxis, un
/// cierre que no toca): ahí paran también `lopdf` y `pom`, que no se saltan nada (un flujo de
/// imagen se para enseguida). Donde podría no pararse sigue: cuenta de más, nunca de menos. Las
/// imágenes en línea (`BI … ID … EI`) no se siguen: desde ahí se cuenta cada byte que no es un
/// espacio como un objeto.
fn leer_sintaxis(d: &[u8], sintaxis: Sintaxis) -> Lectura {
    let llaves = sintaxis == Sintaxis::Type1;
    let mut l = Lectura::default();
    let mut pila: Vec<u8> = Vec::new();
    let mut i = 0;
    while let Some(&c) = d.get(i) {
        if es_espacio(c) {
            i += 1;
            continue;
        }
        match c {
            b'%' => {
                while d.get(i).is_some_and(|c| !b"\r\n".contains(c)) {
                    i += 1;
                }
            }
            b'(' => {
                l.fichas += 1;
                let mut p = 0usize;
                while let Some(&c) = d.get(i) {
                    i += 1;
                    match c {
                        b'\\' => i += 1,
                        b'(' => {
                            p += 1;
                            l.hondura = l.hondura.max(pila.len() + p);
                        }
                        b')' => {
                            p -= 1;
                            if p == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            }
            b'<' if d.get(i + 1) == Some(&b'<') => {
                l.fichas += 1;
                pila.push(b'<');
                l.hondura = l.hondura.max(pila.len());
                i += 2;
            }
            b'<' => {
                l.fichas += 1;
                i = d[i..]
                    .iter()
                    .position(|&c| c == b'>')
                    .map_or(d.len(), |p| i + p + 1);
            }
            b'>' if d.get(i + 1) == Some(&b'>') && pila.last() == Some(&b'<') => {
                pila.pop();
                i += 2;
            }
            b'[' | b'{' if c == b'[' || llaves => {
                l.fichas += 1;
                pila.push(c);
                l.hondura = l.hondura.max(pila.len());
                i += 1;
            }
            b']' | b'}' if pila.last() == Some(&if c == b']' { b'[' } else { b'{' }) => {
                pila.pop();
                i += 1;
            }
            b'/' => {
                l.fichas += 1;
                i += 1;
                while d.get(i).is_some_and(|&c| es_regular(c)) {
                    i += 1;
                }
            }
            c if c.is_ascii_alphanumeric() || b"+-.*'\"".contains(&c) => {
                let desde = i;
                while d.get(i).is_some_and(|&c| es_regular(c)) {
                    i += 1;
                }
                let racha = &d[desde..i];
                if sintaxis == Sintaxis::Contenido && pila.is_empty() && racha.starts_with(b"BI") {
                    let resto = d[desde..].iter().filter(|&&c| !es_espacio(c)).count();
                    l.fichas += resto as u64;
                    return l;
                }
                l.fichas += objetos_en_racha(racha);
            }
            _ => break,
        }
    }
    l
}

/// Objetos que pueden salir de una racha de letras normales (sin espacios ni delimitadores),
/// por lo alto: un número o un operador acaban donde cambia la clase de letra o en un signo o un
/// punto (`1.5.5`, `1-1`, `12Tf`), y `true`, `false` y `null` pueden ir pegados (cuatro letras o
/// más cada uno).
fn objetos_en_racha(r: &[u8]) -> u64 {
    let letra = |c: u8| c.is_ascii_alphabetic() || b"*'\"".contains(&c);
    let cortes = r
        .windows(2)
        .filter(|w| b"+-.".contains(&w[1]) || letra(w[0]) != letra(w[1]))
        .count();
    let letras = r.iter().filter(|&&c| letra(c)).count();
    (1 + cortes + letras / 4) as u64
}

/// Ningún `[` ni `<<` del fichero, fuera de cadenas, comentarios y flujos, se anida más de
/// [`MAX_ANIDAMIENTO`] niveles. Los datos de un flujo se saltan desde `stream` hasta el
/// siguiente `endstream`.
fn anidamiento_razonable(bytes: &[u8]) -> Result<(), Demasiado> {
    let mut hondura = 0usize;
    let mut i = 0;
    while let Some(&c) = bytes.get(i) {
        match c {
            b'%' => {
                while bytes.get(i).is_some_and(|c| !b"\r\n".contains(c)) {
                    i += 1;
                }
            }
            b'(' => {
                let mut p = 0usize;
                while let Some(&c) = bytes.get(i) {
                    i += 1;
                    match c {
                        b'\\' => i += 1,
                        b'(' => p += 1,
                        b')' => {
                            p -= 1;
                            if p == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                }
            }
            b'<' | b'[' => {
                if c == b'<' && bytes.get(i + 1) != Some(&b'<') {
                    i = bytes[i..]
                        .iter()
                        .position(|&c| c == b'>')
                        .map_or(bytes.len(), |p| i + p + 1);
                    continue;
                }
                hondura += 1;
                if hondura > MAX_ANIDAMIENTO {
                    return Err(Demasiado);
                }
                i += if c == b'<' { 2 } else { 1 };
            }
            b'>' | b']' => {
                hondura = hondura.saturating_sub(1);
                i += if c == b'>' && bytes.get(i + 1) == Some(&b'>') {
                    2
                } else {
                    1
                };
            }
            b'/' => {
                i += 1;
                while bytes.get(i).is_some_and(|&c| es_regular(c)) {
                    i += 1;
                }
            }
            c if es_regular(c) => {
                let desde = i;
                while bytes.get(i).is_some_and(|&c| es_regular(c)) {
                    i += 1;
                }
                if &bytes[desde..i] == b"stream" {
                    match buscar(bytes, b"endstream", i) {
                        Some(p) => i = p + b"endstream".len(),
                        None => break,
                    }
                }
            }
            _ => i += 1,
        }
    }
    Ok(())
}

/// Lo que se lleva a `pdf-extract` ejecutar un contenido (una página o un formulario) una vez,
/// por lo alto.
#[derive(Clone, Default)]
struct Coste {
    /// Bytes que descomprime o copia y fichas que analiza (sin las fuentes).
    trabajo: u64,
    /// Lo más que tiene en memoria a la vez (sin las fuentes ni el propio contenido, que cuenta
    /// quien lo ejecuta).
    memoria: u64,
    /// Operaciones costosas (sin las fuentes).
    operaciones: u64,
    /// Lo más de `q` sin su `Q` a la vez, con los de los formularios que llama.
    pila: u64,
    /// Las fuentes que pueden crearse: `pdf-extract` las guarda por su nombre hasta cambiar de
    /// página, así que se cuentan una vez por nombre y página.
    fuentes: Fuentes,
}

/// Lo que cuesta crear una fuente: en trabajo y en memoria.
#[derive(Clone, Copy, Default)]
struct CosteFuente {
    trabajo: u64,
    memoria: u64,
}

/// Fuentes que pueden crearse en una página, por nombre (lo más caro de lo que puede tener ese
/// nombre); pasados [`MAX_NOMBRES`] nombres, el resto se suma.
#[derive(Clone, Default)]
struct Fuentes {
    por_nombre: BTreeMap<Vec<u8>, CosteFuente>,
    resto: CosteFuente,
    cuantas_resto: u64,
}

impl Fuentes {
    fn poner(&mut self, nombre: &[u8], c: CosteFuente) {
        if let Some(v) = self.por_nombre.get_mut(nombre) {
            v.trabajo = v.trabajo.max(c.trabajo);
            v.memoria = v.memoria.max(c.memoria);
        } else if self.por_nombre.len() < MAX_NOMBRES {
            self.por_nombre.insert(nombre.to_vec(), c);
        } else {
            self.resto.trabajo = self.resto.trabajo.saturating_add(c.trabajo);
            self.resto.memoria = self.resto.memoria.saturating_add(c.memoria);
            self.cuantas_resto += 1;
        }
    }

    fn juntar(&mut self, otras: &Fuentes) {
        for (n, c) in &otras.por_nombre {
            self.poner(n, *c);
        }
        self.resto.trabajo = self.resto.trabajo.saturating_add(otras.resto.trabajo);
        self.resto.memoria = self.resto.memoria.saturating_add(otras.resto.memoria);
        self.cuantas_resto = self.cuantas_resto.saturating_add(otras.cuantas_resto);
    }

    fn total(&self) -> (CosteFuente, u64) {
        let mut t = self.resto;
        for c in self.por_nombre.values() {
            t.trabajo = t.trabajo.saturating_add(c.trabajo);
            t.memoria = t.memoria.saturating_add(c.memoria);
        }
        (t, self.por_nombre.len() as u64 + self.cuantas_resto)
    }
}

/// Lo que importa de un contenido para lo que cuesta: cuántas veces sale cada operador que
/// hace trabajo y con qué nombre (su primer operando, como en `pdf-extract`).
#[derive(Default)]
struct Resumen {
    fichas: u64,
    q: u64,
    /// Lo más de `q` sin su `Q` a la vez.
    pila: u64,
    /// Lo más de operandos de un `sc`/`scn` más lo más de un `SC`/`SCN`: los números de color
    /// que lleva cada copia del estado.
    colores: u64,
    /// Trozos de ruta y `BMC`/`BDC`, que `pdf-extract` va guardando.
    rutas: u64,
    marcas: u64,
    dos: BTreeMap<Vec<u8>, u64>,
    /// Por cada `Do`, lo más de `q` abiertos cuando se llama.
    pila_en_do: BTreeMap<Vec<u8>, u64>,
    tfs: BTreeSet<Vec<u8>>,
    css: BTreeMap<Vec<u8>, u64>,
    gss: BTreeMap<Vec<u8>, u64>,
}

/// La dirección de algo del documento, para recordar lo ya medido (el documento no cambia
/// mientras se mide).
fn dir<T>(x: &T) -> usize {
    std::ptr::from_ref(x).addr()
}

/// Lo que devuelve `get_contents` de `pdf-extract` (y lo que pone `get_page_content` de
/// `lopdf`): lo descomprimido, o los datos tal cual si no se puede.
fn datos_de(s: &Stream) -> std::borrow::Cow<'_, [u8]> {
    match s.decompressed_content() {
        Ok(d) if s.filters().is_ok() => std::borrow::Cow::Owned(d),
        _ => std::borrow::Cow::Borrowed(&s.content),
    }
}

/// La cota del trabajo de `pdf-extract` sobre un documento.
struct Cota<'a> {
    doc: &'a Document,
    topes: Topes,
    /// Los recursos de una página que no tiene (como el `empty_resources` de `pdf-extract`).
    vacio: &'a Dictionary,
    tamanos: HashMap<usize, u64>,
    resumenes: HashMap<usize, Rc<Resumen>>,
    formas: HashMap<(usize, usize), Rc<Coste>>,
    paginas: HashMap<ObjectId, Rc<Coste>>,
    fuentes: HashMap<usize, CosteFuente>,
    mascaras: HashMap<usize, u64>,
    /// Los formularios que se están midiendo, uno dentro de otro.
    pila: Vec<usize>,
    /// Pasos de esta comprobación y bytes que ha descomprimido o analizado.
    pasos: u64,
    leido: u64,
}

impl<'a> Cota<'a> {
    fn new(doc: &'a Document, topes: Topes, vacio: &'a Dictionary) -> Self {
        Self {
            doc,
            topes,
            vacio,
            tamanos: HashMap::new(),
            resumenes: HashMap::new(),
            formas: HashMap::new(),
            paginas: HashMap::new(),
            fuentes: HashMap::new(),
            mascaras: HashMap::new(),
            pila: Vec::new(),
            pasos: 0,
            leido: 0,
        }
    }

    fn paso(&mut self, n: u64) -> Result<(), Demasiado> {
        self.pasos = self.pasos.saturating_add(n);
        if self.pasos > self.topes.max_operaciones {
            return Err(Demasiado);
        }
        Ok(())
    }

    /// Lo que descomprime o analiza esta comprobación: cada cosa que mide la usa `pdf-extract`
    /// al menos una vez, así que no puede pasar del presupuesto de trabajo.
    fn leer(&mut self, n: u64) -> Result<(), Demasiado> {
        self.leido = self.leido.saturating_add(n);
        if self.leido > self.topes.max_trabajo {
            return Err(Demasiado);
        }
        Ok(())
    }

    fn cabe(&self, c: &Coste) -> Result<(), Demasiado> {
        if c.trabajo > self.topes.max_trabajo
            || c.memoria > self.topes.max_memoria
            || c.operaciones > self.topes.max_operaciones
            || c.pila > MAX_PILA
        {
            return Err(Demasiado);
        }
        Ok(())
    }

    /// Como `maybe_deref` de `pdf-extract` (`get_object` de `lopdf` sigue las referencias).
    fn deref(&self, o: &'a Object) -> Option<&'a Object> {
        self.doc.dereference(o).ok().map(|(_, o)| o)
    }

    fn en(&self, d: &'a Dictionary, clave: &[u8]) -> Option<&'a Object> {
        d.get(clave).ok().and_then(|o| self.deref(o))
    }

    fn dict_en(&self, d: &'a Dictionary, clave: &[u8]) -> Option<&'a Dictionary> {
        self.en(d, clave).and_then(|o| o.as_dict().ok())
    }

    fn flujo_en(&self, d: &'a Dictionary, clave: &[u8]) -> Option<&'a Stream> {
        self.en(d, clave).and_then(|o| o.as_stream().ok())
    }

    fn largo_de_array(&self, o: Option<&'a Object>) -> u64 {
        o.and_then(|o| self.deref(o))
            .and_then(|o| o.as_array().ok())
            .map_or(0, |a| a.len() as u64)
    }

    /// Lo que mide un flujo descomprimido, una vez por flujo.
    fn tamano(&mut self, s: &'a Stream) -> Result<u64, Demasiado> {
        let k = dir(s);
        if let Some(&t) = self.tamanos.get(&k) {
            return Ok(t);
        }
        self.paso(1)?;
        let t = datos_de(s).len() as u64;
        self.leer(t)?;
        self.tamanos.insert(k, t);
        Ok(t)
    }

    /// El resumen de un contenido, tras comprobar que analizarlo no pide demasiada memoria.
    fn resumir(&mut self, d: &[u8]) -> Result<Resumen, Demasiado> {
        let l = leer_sintaxis(d, Sintaxis::Contenido);
        // Segunda barrera contra el desbordamiento de pila del analizador de contenidos, que
        // hoy sólo para el `MAX_NESTING_DEPTH` de lopdf 0.42: `anidamiento_razonable` no mira
        // dentro de los flujos.
        if l.hondura > MAX_ANIDAMIENTO {
            return Err(Demasiado);
        }
        let memoria = (d.len() as u64).saturating_add(l.fichas.saturating_mul(PESO_FICHA));
        if memoria > self.topes.max_memoria {
            return Err(Demasiado);
        }
        self.leer(l.fichas)?;
        let mut r = Resumen {
            fichas: l.fichas,
            ..Resumen::default()
        };
        // Lo que `pdf-extract` no puede analizar le hace entrar en pánico: ahí para todo.
        let Ok(c) = pdf_extract::content::Content::decode(d) else {
            return Ok(r);
        };
        self.paso(c.operations.len() as u64 / 64)?;
        let (mut pila, mut relleno, mut trazo) = (0u64, 0u64, 0u64);
        for op in &c.operations {
            let nombre = || {
                op.operands
                    .first()
                    .and_then(|o| o.as_name().ok())
                    .map(<[u8]>::to_vec)
            };
            let cuantos = match op.operator.as_str() {
                "TJ" | "BI" => op.operands.len() as u64,
                _ => operandos(&op.operands, MAX_OPERANDOS),
            };
            if cuantos > MAX_OPERANDOS {
                return Err(Demasiado);
            }
            match op.operator.as_str() {
                "q" => {
                    r.q += 1;
                    pila += 1;
                    r.pila = r.pila.max(pila);
                    if pila > MAX_PILA {
                        return Err(Demasiado);
                    }
                }
                "Q" => pila = pila.saturating_sub(1),
                "sc" | "scn" => relleno = relleno.max(cuantos),
                "SC" | "SCN" => trazo = trazo.max(cuantos),
                "m" | "l" | "c" | "v" | "y" | "h" | "re" => r.rutas += 1,
                "BMC" | "BDC" => r.marcas += 1,
                "Do" | "cs" | "CS" | "gs" => {
                    let Some(n) = nombre() else { continue };
                    if op.operator == "Do" {
                        let p = r.pila_en_do.entry(n.clone()).or_default();
                        *p = (*p).max(pila);
                    }
                    let m = match op.operator.as_str() {
                        "Do" => &mut r.dos,
                        "gs" => &mut r.gss,
                        _ => &mut r.css,
                    };
                    *m.entry(n).or_default() += 1;
                }
                "Tf" => {
                    if let Some(n) = nombre() {
                        r.tfs.insert(n);
                    }
                }
                _ => {}
            }
        }
        r.colores = relleno.saturating_add(trazo);
        Ok(r)
    }

    /// El tamaño y el resumen de un formulario (o de lo que se llame con `Do`, que
    /// `pdf-extract` trata igual aunque sea una imagen), una vez por flujo.
    fn resumen_forma(&mut self, s: &'a Stream) -> Result<(u64, Rc<Resumen>), Demasiado> {
        let k = dir(s);
        if let Some(r) = self.resumenes.get(&k) {
            return Ok((self.tamanos.get(&k).copied().unwrap_or(0), r.clone()));
        }
        self.paso(1)?;
        let d = datos_de(s);
        let t = d.len() as u64;
        self.leer(t)?;
        self.tamanos.insert(k, t);
        let r = Rc::new(self.resumir(&d)?);
        self.resumenes.insert(k, r.clone());
        Ok((t, r))
    }

    /// Lo que cuesta ejecutar un contenido ya resumido con unos recursos.
    fn coste_contenido(&mut self, r: &Resumen, res: &'a Dictionary) -> Result<Coste, Demasiado> {
        self.paso((r.dos.len() + r.tfs.len() + r.css.len() + r.gss.len() + 1) as u64)?;
        let mut c = Coste {
            trabajo: r.fichas,
            memoria: r
                .fichas
                .saturating_mul(PESO_FICHA)
                .saturating_add(r.rutas.saturating_mul(PESO_RUTA))
                .saturating_add(r.marcas.saturating_mul(PESO_MARCA)),
            pila: r.pila,
            ..Coste::default()
        };
        // `cs` y `CS`: cada vez se rehace el espacio de color, y un ICC (o la función de un
        // `Separation`) se descomprime entero.
        let mut color = 0u64;
        for (n, &k) in &r.css {
            let t = self.espacio(res, n)?;
            color = color.max(t);
            c.trabajo = c.trabajo.saturating_add(k.saturating_mul(t));
            c.operaciones = c.operaciones.saturating_add(k);
        }
        // `gs`: cada vez se copia la `/SMask` entera.
        let mut mascara = 0u64;
        for (n, &k) in &r.gss {
            let t = self.mascara(res, n)?;
            mascara = mascara.max(t);
            c.trabajo = c.trabajo.saturating_add(k.saturating_mul(t));
            c.operaciones = c.operaciones.saturating_add(k);
        }
        // `q` copia el estado gráfico, con sus espacios de color (relleno y trazo), sus números
        // de color y su máscara: cada `q` cuesta una copia, y en memoria hay a la vez tantas
        // como `q` abiertos, cada una como mucho del tamaño del estado más grande.
        let pesado = color
            .saturating_mul(2)
            .saturating_add(mascara)
            .saturating_add(r.colores.saturating_mul(8));
        let q_trabajo = r.q.saturating_mul(TIEMPO_ESTADO.saturating_add(pesado));
        let q_memoria = r.pila.saturating_mul(PESO_ESTADO.saturating_add(pesado));
        c.trabajo = c.trabajo.saturating_add(q_trabajo);
        c.memoria = c.memoria.saturating_add(q_memoria).saturating_add(pesado);
        for n in &r.tfs {
            let f = self.fuente(res, n)?;
            c.fuentes.poner(n, f);
        }
        self.cabe(&c)?;
        // `Do`: cada vez se descomprime el formulario y se ejecuta entero.
        let mut hijo_max = 0u64;
        for (n, &k) in &r.dos {
            c.operaciones = c.operaciones.saturating_add(k);
            let Some((s, res_hijo)) = self.forma(res, n) else {
                continue;
            };
            let (t, h) = self.coste_forma(s, res_hijo)?;
            // Sus `q` se suman a los que había abiertos al llamarlo.
            let abiertos = r.pila_en_do.get(n).copied().unwrap_or(0);
            c.pila = c.pila.max(abiertos.saturating_add(h.pila));
            let una = t.saturating_add(h.trabajo);
            c.trabajo = c.trabajo.saturating_add(k.saturating_mul(una));
            c.operaciones = c
                .operaciones
                .saturating_add(k.saturating_mul(h.operaciones));
            hijo_max = hijo_max.max(t.saturating_add(h.memoria));
            c.fuentes.juntar(&h.fuentes);
            self.cabe(&c)?;
        }
        c.memoria = c.memoria.saturating_add(hijo_max);
        self.cabe(&c)?;
        Ok(c)
    }

    /// El formulario que llama `Do /nombre` y los recursos con los que se ejecuta, como en
    /// `pdf-extract`.
    fn forma(&self, res: &'a Dictionary, nombre: &[u8]) -> Option<(&'a Stream, &'a Dictionary)> {
        let xo = self.dict_en(res, b"XObject")?;
        let s = self.flujo_en(xo, nombre)?;
        Some((s, self.dict_en(&s.dict, b"Resources").unwrap_or(res)))
    }

    /// Tamaño y coste de un formulario con unos recursos, una vez por pareja. Un formulario que
    /// ya se está midiendo (un ciclo) o más de [`MAX_HONDURA_FORMAS`] uno dentro de otro no se
    /// admiten.
    fn coste_forma(
        &mut self,
        s: &'a Stream,
        res: &'a Dictionary,
    ) -> Result<(u64, Rc<Coste>), Demasiado> {
        let k = dir(s);
        if self.pila.contains(&k) || self.pila.len() >= MAX_HONDURA_FORMAS {
            return Err(Demasiado);
        }
        let (t, r) = self.resumen_forma(s)?;
        if let Some(c) = self.formas.get(&(k, dir(res))) {
            return Ok((t, c.clone()));
        }
        self.pila.push(k);
        let c = Rc::new(self.coste_contenido(&r, res)?);
        self.pila.pop();
        self.formas.insert((k, dir(res)), c.clone());
        Ok((t, c))
    }

    /// Lo que mueve `make_colorspace` de `pdf-extract` con `cs /nombre`: los datos del perfil
    /// ICC (también el alternativo de un `Separation`), la función de un `Separation` y los
    /// arrays de sus diccionarios.
    fn espacio(&mut self, res: &'a Dictionary, nombre: &[u8]) -> Result<u64, Demasiado> {
        if matches!(
            nombre,
            b"DeviceGray" | b"DeviceRGB" | b"DeviceCMYK" | b"Pattern"
        ) {
            return Ok(0);
        }
        let Some(cs) = self
            .dict_en(res, b"ColorSpace")
            .and_then(|d| self.en(d, nombre))
            .and_then(|o| o.as_array().ok())
        else {
            return Ok(0);
        };
        let mut t = 0u64;
        let tipo = cs.first().and_then(|o| o.as_name().ok());
        let mut sumar = |c: &mut Self, o: Option<&'a Object>| -> Result<(), Demasiado> {
            match o.and_then(|o| c.deref(o)) {
                Some(Object::Stream(s)) => {
                    t = t
                        .saturating_add(c.tamano(s)?)
                        .saturating_add(8 * c.arrays(&s.dict)?);
                }
                Some(Object::Dictionary(d)) => t = t.saturating_add(8 * c.arrays(d)?),
                _ => {}
            }
            Ok(())
        };
        if tipo == Some(b"Separation") {
            if let Some(alt) = cs
                .get(2)
                .and_then(|o| self.deref(o))
                .and_then(|o| o.as_array().ok())
            {
                sumar(self, alt.get(1))?;
            }
            sumar(self, cs.get(3))?;
        } else {
            sumar(self, cs.get(1))?;
        }
        Ok(t)
    }

    /// Elementos de los arrays de un diccionario (los `Vec<f64>` que saca `pdf-extract`).
    fn arrays(&mut self, d: &'a Dictionary) -> Result<u64, Demasiado> {
        self.paso(d.len() as u64)?;
        Ok(d.iter().map(|(_, v)| self.largo_de_array(Some(v))).sum())
    }

    /// Lo que copia `apply_state` de `pdf-extract` con `gs /nombre`: la `/SMask` entera.
    fn mascara(&mut self, res: &'a Dictionary, nombre: &[u8]) -> Result<u64, Demasiado> {
        let Some(st) = self
            .dict_en(res, b"ExtGState")
            .and_then(|d| self.dict_en(d, nombre))
        else {
            return Ok(0);
        };
        let entradas = st.len() as u64;
        let Some(sm) = self.dict_en(st, b"SMask") else {
            return Ok(entradas);
        };
        let k = dir(sm);
        if let Some(&t) = self.mascaras.get(&k) {
            return Ok(t.saturating_add(entradas));
        }
        let mut pasos = 0;
        let t = peso_objeto_dict(sm, &mut pasos);
        self.paso(pasos)?;
        self.mascaras.insert(k, t);
        Ok(t.saturating_add(entradas))
    }

    /// Lo que cuesta crear la fuente de `Tf /nombre` con `make_font` de `pdf-extract`: sus
    /// flujos descomprimidos (`/ToUnicode`, `/Encoding`, `/FontFile`, `/FontFile2` y
    /// `/FontFile3`), lo que se analiza de ellos y los códigos que salen del `/ToUnicode`.
    fn fuente(&mut self, res: &'a Dictionary, nombre: &[u8]) -> Result<CosteFuente, Demasiado> {
        let Some(f) = self
            .dict_en(res, b"Font")
            .and_then(|d| self.dict_en(d, nombre))
        else {
            return Ok(CosteFuente::default());
        };
        let k = dir(f);
        if let Some(&c) = self.fuentes.get(&k) {
            return Ok(c);
        }
        self.paso(1)?;
        let mut c = CosteFuente::default();
        let sumar = |c: &mut CosteFuente, t: u64, m: u64| {
            c.trabajo = c.trabajo.saturating_add(t);
            c.memoria = c.memoria.saturating_add(m);
        };
        // Los anchos: una tabla por elemento de `/Widths` y de los arrays de `/W`.
        let mut anchos = self.largo_de_array(f.get(b"Widths").ok());
        if let Some(w) = self
            .en(f, b"DescendantFonts")
            .and_then(|o| o.as_array().ok())
            .and_then(|a| a.first())
            .and_then(|o| self.deref(o))
            .and_then(|o| o.as_dict().ok())
            .and_then(|cid| self.en(cid, b"W"))
            .and_then(|o| o.as_array().ok())
        {
            self.paso(w.len() as u64)?;
            anchos += w
                .iter()
                .map(|o| self.largo_de_array(Some(o)).max(1))
                .sum::<u64>();
        }
        sumar(&mut c, anchos, anchos.saturating_mul(64));
        if let Some(s) = self.flujo_en(f, b"ToUnicode") {
            let (t, m) = self.pom(s, Sintaxis::CMap, true)?;
            sumar(&mut c, t, m);
        }
        if let Some(s) = self.flujo_en(f, b"Encoding") {
            let (t, m) = self.pom(s, Sintaxis::CMap, false)?;
            sumar(&mut c, t, m);
        }
        if let Some(fd) = self.dict_en(f, b"FontDescriptor") {
            if let Some(s) = self.flujo_en(fd, b"FontFile") {
                let (t, m) = self.pom(s, Sintaxis::Type1, false)?;
                sumar(&mut c, t, m);
            }
            for clave in [&b"FontFile2"[..], b"FontFile3"] {
                if let Some(s) = self.flujo_en(fd, clave) {
                    // La de tipo 3 la analiza `cff-parser`, que no hace más que tablas.
                    let t = self.tamano(s)?;
                    sumar(&mut c, t.saturating_mul(2), t.saturating_mul(2));
                }
            }
        }
        self.fuentes.insert(k, c);
        Ok(c)
    }

    /// Un flujo que `pdf-extract` analiza con `pom`: que no se anide demasiado (no tiene tope y
    /// desbordaría la pila), lo que cuesta y, en un `/ToUnicode`, los códigos que salen de sus
    /// `beginbfrange` (cada rango se expande código a código).
    fn pom(
        &mut self,
        s: &'a Stream,
        sintaxis: Sintaxis,
        rangos: bool,
    ) -> Result<(u64, u64), Demasiado> {
        self.paso(1)?;
        let d = datos_de(s);
        let largo = d.len() as u64;
        self.leer(largo)?;
        let l = leer_sintaxis(&d, sintaxis);
        if l.hondura > MAX_ANIDAMIENTO_POM {
            return Err(Demasiado);
        }
        self.leer(l.fichas)?;
        let memoria = largo.saturating_add(l.fichas.saturating_mul(PESO_FICHA_POM));
        if memoria > self.topes.max_memoria {
            return Err(Demasiado);
        }
        let mut t = largo.saturating_add(l.fichas);
        let mut m = memoria;
        if rangos {
            let n = codigos_del_cmap(&d);
            t = t.saturating_add(n);
            m = m.saturating_add(n.saturating_mul(PESO_ENTRADA));
        }
        Ok((t, m))
    }

    /// Las veces que se sube por `/Parent` desde una página, sin ciclos y sin pasar de
    /// [`MAX_PADRES`].
    fn padres(&mut self, id: ObjectId, mut d: &'a Dictionary) -> Result<(), Demasiado> {
        let mut vistos = vec![id];
        loop {
            let Ok(p) = d.get(b"Parent").and_then(Object::as_reference) else {
                return Ok(());
            };
            if vistos.contains(&p) || vistos.len() > MAX_PADRES {
                return Err(Demasiado);
            }
            vistos.push(p);
            self.paso(1)?;
            let Ok(pd) = self.doc.get_dictionary(p) else {
                return Ok(());
            };
            d = pd;
        }
    }

    /// Como `get_inherited` de `pdf-extract` para `/Resources` (tras [`Self::padres`]).
    fn recursos(&self, mut d: &'a Dictionary) -> &'a Dictionary {
        for _ in 0..=MAX_PADRES {
            if let Some(r) = self.dict_en(d, b"Resources") {
                return r;
            }
            match d
                .get(b"Parent")
                .and_then(Object::as_reference)
                .and_then(|p| self.doc.get_dictionary(p))
            {
                Ok(p) => d = p,
                Err(_) => break,
            }
        }
        self.vacio
    }

    /// Lo que concatena `get_page_content` de `lopdf`: cada referencia de `/Contents` (con sus
    /// repeticiones) más un salto de línea, y cuántas referencias recorre.
    fn contenidos(&mut self, pagina: &'a Dictionary) -> Result<(u64, u64), Demasiado> {
        let mut ids = Vec::new();
        let Ok(mut c) = pagina.get(b"Contents") else {
            return Ok((0, 0));
        };
        let mut saltos = 0;
        let lista: &[Object] = loop {
            match c {
                Object::Reference(id) => match self.doc.objects.get(id) {
                    None | Some(Object::Stream(_)) => {
                        ids.push(*id);
                        break &[];
                    }
                    Some(o) => {
                        saltos += 1;
                        if saltos < MAX_REFERENCIAS {
                            c = o;
                            continue;
                        }
                        break &[];
                    }
                },
                Object::Array(a) => break a,
                _ => break &[],
            }
        };
        let (mut bytes, mut refs) = (0u64, 0u64);
        for id in ids
            .into_iter()
            .chain(lista.iter().filter_map(|o| o.as_reference().ok()))
        {
            refs += 1;
            self.paso(1)?;
            if let Ok(s) = self.doc.get_object(id).and_then(Object::as_stream) {
                bytes = bytes.saturating_add(self.tamano(s)?).saturating_add(1);
                if bytes > self.topes.max_memoria || bytes > self.topes.max_trabajo {
                    return Err(Demasiado);
                }
            }
        }
        Ok((bytes, refs))
    }

    /// Lo que cuesta `output_doc_inner` de `pdf-extract` con una página, una vez por página.
    fn pagina(&mut self, id: ObjectId) -> Result<Rc<Coste>, Demasiado> {
        if let Some(c) = self.paginas.get(&id) {
            return Ok(c.clone());
        }
        self.paso(1)?;
        let Ok(d) = self.doc.get_object(id).and_then(Object::as_dict) else {
            // `pdf-extract` entra en pánico y no sigue.
            return Ok(Rc::new(Coste::default()));
        };
        self.padres(id, d)?;
        let res = self.recursos(d);
        let (bytes, refs) = self.contenidos(d)?;
        self.leer(bytes)?;
        let r = {
            let contenido = self.doc.get_page_content(id).unwrap_or_default();
            self.resumir(&contenido)?
        };
        let c = self.coste_contenido(&r, res)?;
        let (f, cuantas) = c.fuentes.total();
        let p = Coste {
            trabajo: bytes.saturating_add(c.trabajo).saturating_add(f.trabajo),
            memoria: bytes.saturating_add(c.memoria).saturating_add(f.memoria),
            operaciones: (1 + refs)
                .saturating_add(c.operaciones)
                .saturating_add(cuantas),
            pila: c.pila,
            fuentes: Fuentes::default(),
        };
        self.cabe(&p)?;
        let p = Rc::new(p);
        self.paginas.insert(id, p.clone());
        Ok(p)
    }
}

/// Los operandos de un operador, contando lo que hay dentro de sus arrays y diccionarios (sin
/// seguir referencias). Para en cuanto pasa de `max`.
fn operandos(os: &[Object], max: u64) -> u64 {
    fn contar(o: &Object, n: &mut u64, max: u64) {
        *n += 1;
        match o {
            Object::Array(a) => {
                for x in a {
                    if *n > max {
                        return;
                    }
                    contar(x, n, max);
                }
            }
            Object::Dictionary(d) => {
                for (_, x) in d {
                    if *n > max {
                        return;
                    }
                    contar(x, n, max);
                }
            }
            _ => {}
        }
    }
    let mut n = 0;
    for o in os {
        if n > max {
            break;
        }
        contar(o, &mut n, max);
    }
    n
}

/// Lo que pesa una copia de un diccionario (y lo que lleva dentro, sin seguir referencias).
fn peso_objeto_dict(d: &Dictionary, pasos: &mut u64) -> u64 {
    d.iter().fold(size_of::<Dictionary>() as u64, |t, (k, v)| {
        t.saturating_add(k.len() as u64 + 16)
            .saturating_add(peso_objeto(v, pasos))
    })
}

fn peso_objeto(o: &Object, pasos: &mut u64) -> u64 {
    *pasos += 1;
    let base = size_of::<Object>() as u64;
    base.saturating_add(match o {
        Object::Name(v) | Object::String(v, _) => v.len() as u64,
        Object::Array(a) => a
            .iter()
            .fold(0u64, |t, x| t.saturating_add(peso_objeto(x, pasos))),
        Object::Dictionary(d) => peso_objeto_dict(d, pasos),
        Object::Stream(s) => {
            peso_objeto_dict(&s.dict, pasos).saturating_add(s.content.len() as u64)
        }
        _ => 0,
    })
}

/// Códigos que `get_unicode_map` de `adobe-cmap-parser` mete en su tabla con un `/ToUnicode`,
/// leído con el mismo analizador y recorrido igual (sin meterlos): un `beginbfrange` de
/// `<00000000> <FFFFFFFF>` son cuatro mil millones. Donde aquel da error o entra en pánico,
/// aquí se para.
fn codigos_del_cmap(d: &[u8]) -> u64 {
    use adobe_cmap_parser::Value;
    let Ok(v) = adobe_cmap_parser::parse(d) else {
        return 0;
    };
    let codigo = |s: &[u8]| s.iter().fold(0u32, |c, &b| (c << 8) | u32::from(b));
    let mut n = 0u64;
    let mut i = 0;
    while let Some(x) = v.get(i) {
        let Value::Operator(o) = x else {
            i += 1;
            continue;
        };
        let rango = match o.as_str() {
            "beginbfchar" => false,
            "beginbfrange" => true,
            _ => {
                i += 1;
                continue;
            }
        };
        let Some(Value::Integer(cuantos)) = i.checked_sub(1).and_then(|j| v.get(j)) else {
            return n;
        };
        i += 1;
        for _ in 0..*cuantos {
            let (Some(Value::LiteralString(a)), Some(Value::LiteralString(b))) =
                (v.get(i), v.get(i + 1))
            else {
                return n;
            };
            if !rango {
                n = n.saturating_add(1);
                i += 2;
                continue;
            }
            let (bajo, alto) = (codigo(a), codigo(b));
            let ancho = if alto >= bajo {
                u64::from(alto - bajo) + 1
            } else {
                0
            };
            match v.get(i + 2) {
                Some(Value::LiteralString(s)) if matches!(s.len(), 2 | 4) => {
                    n = n.saturating_add(ancho);
                }
                Some(Value::Array(a))
                    if alto.wrapping_sub(bajo).wrapping_add(1) as usize == a.len() =>
                {
                    n = n.saturating_add(ancho.min(a.len() as u64));
                }
                _ => return n,
            }
            i += 3;
        }
        i += 1;
    }
    n
}

/// Comprueba que `pdf-extract` no trabajará demasiado con el documento ya abierto (y
/// descifrado): por cada página de `get_pages` (con sus repeticiones), lo que concatena de
/// `/Contents` y, en su contenido y en el de cada formulario, lo que hace cada `Do`, `cs`,
/// `CS`, `gs`, `q` y `Tf`. Los tamaños se miden una vez por flujo y los costes de un formulario
/// una vez por formulario y recursos; luego se multiplican por las veces que se ejecutan.
fn cabe_el_trabajo(doc: &Document, topes: Topes) -> Result<(), Demasiado> {
    let vacio = Dictionary::new();
    let mut cota = Cota::new(doc, topes, &vacio);
    let (mut trabajo, mut operaciones) = (0u64, 0u64);
    for id in doc.get_pages().into_values() {
        let c = cota.pagina(id)?;
        trabajo = trabajo.saturating_add(c.trabajo);
        operaciones = operaciones.saturating_add(c.operaciones);
        if trabajo > topes.max_trabajo || operaciones > topes.max_operaciones {
            return Err(Demasiado);
        }
    }
    Ok(())
}

/// Como `pdf_extract::extract_text_from_mem`, con los topes.
fn extract_text_topado(bytes: &[u8], topes: Topes, max_texto: usize) -> Option<String> {
    let doc = abrir_con(bytes, topes)?;
    let mut s = String::new();
    let r = {
        let mut salida = Topada {
            dentro: pdf_extract::PlainTextOutput::new(&mut s),
            escrito: 0,
            max: max_texto,
        };
        pdf_extract::output_doc(&doc, &mut salida)
    };
    match r {
        Ok(()) => Some(s),
        Err(_) => {
            if s.len() >= max_texto / 2 {
                tracing::warn!("PDF con demasiado texto: no se lee");
            }
            None
        }
    }
}

fn extract(bytes: &[u8]) -> Option<String> {
    extract_con(bytes, Topes::DE_VERDAD, MAX_TEXTO)
}

fn extract_con(bytes: &[u8], topes: Topes, max_texto: usize) -> Option<String> {
    match catch_unwind(AssertUnwindSafe(|| {
        extract_text_topado(bytes, topes, max_texto)
    })) {
        Ok(t) => t,
        Err(_) => {
            tracing::warn!("pdf-extract entró en pánico con un PDF: se trata como sin texto");
            None
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// PDF mínimo y válido cuya página no tiene `MediaBox`: pdf-extract 0.12 entra en pánico
    /// (`expect("MediaBox")`).
    pub(crate) fn panicking_pdf() -> Vec<u8> {
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R >>",
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj {o} endobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        for o in offsets {
            out.extend(format!("{o:010} 00000 n \n").bytes());
        }
        out.extend(
            format!(
                "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .bytes(),
        );
        out
    }

    #[test]
    fn un_pdf_que_hace_entrar_en_panico_a_pdf_extract_se_trata_como_sin_texto() {
        let pdf = panicking_pdf();
        // Sin la protección, pdf-extract entra en pánico con este PDF.
        let raw = std::panic::catch_unwind(|| pdf_extract::extract_text_from_mem(&pdf));
        assert!(raw.is_err(), "se esperaba un pánico de pdf-extract");
        assert_eq!(text(&pdf), None);
        // La segunda vez sale de la caché (mismo resultado).
        assert_eq!(text(&pdf), None);
    }

    /// PDF con una página cuyo contenido escribe `veces` veces un texto de `largo` letras.
    fn pdf_con_texto(largo: usize, veces: usize) -> Vec<u8> {
        let linea = format!("({}) Tj T* ", "A".repeat(largo));
        let contenido = format!("BT /F1 12 Tf 14 TL 72 720 Td {} ET", linea.repeat(veces));
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
             /Resources << /Font << /F1 5 0 R >> >> >>"
                .to_owned(),
            format!(
                "<< /Length {} >>\nstream\n{contenido}\nendstream",
                contenido.len()
            ),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj {o} endobj\n", i + 1).bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        for o in offsets {
            out.extend(format!("{o:010} 00000 n \n").bytes());
        }
        out.extend(
            format!(
                "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .bytes(),
        );
        out
    }

    #[test]
    fn un_pdf_normal_se_sigue_leyendo() {
        let pdf = pdf_con_texto(20, 3);
        let t = extract(&pdf).unwrap();
        assert!(t.contains(&"A".repeat(20)), "{t:?}");
    }

    #[test]
    fn un_pdf_con_demasiado_texto_se_corta() {
        // 100 líneas de 100 letras con un tope de 5.000 bytes (el de verdad es de 16 MB).
        let pdf = pdf_con_texto(100, 100);
        assert!(pdf_extract::extract_text_from_mem(&pdf).unwrap().len() > 10_000);
        assert_eq!(extract_con(&pdf, Topes::DE_VERDAD, 5_000), None);
        assert!(extract_con(&pdf, Topes::DE_VERDAD, 100_000).is_some());
    }

    #[test]
    fn un_pdf_demasiado_grande_no_se_abre() {
        let pdf = pdf_con_texto(20, 3);
        let topes = |max_pdf| Topes {
            max_pdf,
            ..Topes::DE_VERDAD
        };
        assert!(extract_con(&pdf, topes(pdf.len()), MAX_TEXTO).is_some());
        assert_eq!(extract_con(&pdf, topes(pdf.len() - 1), MAX_TEXTO), None);
    }

    fn zlib(datos: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        z.write_all(datos).unwrap();
        z.finish().unwrap()
    }

    /// PDF de una página cuyo contenido (`contenido`, ya codificado) va con el diccionario
    /// `dict_flujo` (sin `/Length`).
    fn pdf_con_flujo(dict_flujo: &str, contenido: &[u8]) -> Vec<u8> {
        pdf_con_flujo_y(dict_flujo, "\n", contenido)
    }

    /// Lo mismo, con `entre` entre el diccionario del flujo y la palabra `stream`.
    fn pdf_con_flujo_y(dict_flujo: &str, entre: &str, contenido: &[u8]) -> Vec<u8> {
        let mut objects: Vec<Vec<u8>> = vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
              /Resources << /Font << /F1 5 0 R >> >> >>"
                .to_vec(),
        ];
        let mut flujo = format!(
            "<< /Length {} {dict_flujo} >>{entre}stream\n",
            contenido.len()
        )
        .into_bytes();
        flujo.extend_from_slice(contenido);
        flujo.extend_from_slice(b"\nendstream");
        objects.push(flujo);
        objects.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec());
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj ", i + 1).bytes());
            out.extend_from_slice(o);
            out.extend_from_slice(b" endobj\n");
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
        for o in offsets {
            out.extend(format!("{o:010} 00000 n \n").bytes());
        }
        out.extend(
            format!(
                "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objects.len() + 1
            )
            .bytes(),
        );
        out
    }

    /// Contenido de página con `veces` líneas de texto y, detrás, `relleno` espacios (que no
    /// pintan nada pero se descomprimen).
    fn contenido(veces: usize, relleno: usize) -> Vec<u8> {
        let mut c = format!(
            "BT /F1 12 Tf 14 TL 72 720 Td {} ET",
            "(Factura de prueba) Tj T* ".repeat(veces)
        )
        .into_bytes();
        c.resize(c.len() + relleno, b' ');
        c
    }

    /// Topes con un presupuesto de descompresión de 1 MB (el de verdad es de 256 MB).
    const UN_MEGA: Topes = Topes {
        max_descomprimido: 1024 * 1024,
        ..Topes::DE_VERDAD
    };

    #[test]
    fn un_pdf_comprimido_normal_se_sigue_leyendo() {
        let pdf = pdf_con_flujo("/Filter /FlateDecode", &zlib(&contenido(3, 1000)));
        let t = extract_con(&pdf, UN_MEGA, MAX_TEXTO).unwrap();
        assert!(t.contains("Factura de prueba"), "{t:?}");
        // También con los dos filtros encadenados y con /DecodeParms (sin predictor).
        let a85 = ascii85_de(&zlib(&contenido(3, 1000)));
        let pdf = pdf_con_flujo(
            "/Filter [/ASCII85Decode /FlateDecode] /DecodeParms [null << /Predictor 1 >>]",
            &a85,
        );
        let t = extract_con(&pdf, UN_MEGA, MAX_TEXTO).unwrap();
        assert!(t.contains("Factura de prueba"), "{t:?}");
    }

    #[test]
    fn un_pdf_que_descomprime_demasiado_no_se_abre() {
        // Unos pocos KB que se inflan a 2 MB, con un presupuesto de 1 MB.
        let bomba = zlib(&contenido(3, 2 * 1024 * 1024));
        assert!(bomba.len() < 10_000);
        let pdf = pdf_con_flujo("/Filter /FlateDecode", &bomba);
        // pdf-extract sí lo leería entero.
        assert!(pdf_extract::extract_text_from_mem(&pdf).is_ok());
        assert_eq!(extract_con(&pdf, UN_MEGA, MAX_TEXTO), None);
        assert!(abrir_con(&pdf, UN_MEGA).is_none());
        // Con el presupuesto de verdad, se lee.
        assert!(extract(&pdf).unwrap().contains("Factura de prueba"));

        // Igual tras un ASCII85Decode, con el nombre escrito con #xx o en un flujo que no
        // usa nadie (por ejemplo, un object stream).
        for (dict, datos) in [
            ("/Filter [/ASCII85Decode /FlateDecode]", ascii85_de(&bomba)),
            ("/Filter /Flate#44ecode", bomba.clone()),
            (
                "/Type /ObjStm /N 0 /First 0 /Filter [/FlateDecode]",
                bomba.clone(),
            ),
        ] {
            let pdf = pdf_con_flujo(dict, &datos);
            assert!(abrir_con(&pdf, UN_MEGA).is_none(), "{dict}");
        }
        // Un `/Length` corto no recorta lo que se mide.
        let mut pdf = pdf_con_flujo("/Filter /FlateDecode", &bomba);
        let largo = format!("/Length {} ", bomba.len());
        let p = buscar(&pdf, largo.as_bytes(), 0).unwrap();
        pdf.splice(
            p..p + largo.len(),
            format!("/Length 1{}", " ".repeat(largo.len() - 9)).bytes(),
        );
        assert!(abrir_con(&pdf, UN_MEGA).is_none());
    }

    #[test]
    fn el_presupuesto_es_de_todos_los_flujos_juntos() {
        let medio = zlib(&vec![b' '; 600 * 1024]);
        let mut pdf = pdf_con_flujo("/Filter /FlateDecode", &medio);
        let repaso = |pdf: &[u8]| {
            cabe_lo_comprimido(pdf, &mut Repaso::new(pdf.len(), UN_MEGA.max_descomprimido))
        };
        assert!(repaso(&pdf).is_ok());
        // Otro flujo igual, suelto, al final: entre los dos pasan de 1 MB.
        pdf.extend_from_slice(b"9 0 obj << /Filter /FlateDecode >>\nstream\n");
        pdf.extend_from_slice(&medio);
        pdf.extend_from_slice(b"\nendstream endobj\n");
        assert!(repaso(&pdf).is_err());
    }

    /// La bomba de las pruebas: unos pocos KB que se inflan a 2 MB (con un presupuesto de 1 MB).
    fn bomba() -> Vec<u8> {
        let b = zlib(&contenido(3, 2 * 1024 * 1024));
        assert!(b.len() < 10_000);
        b
    }

    /// Los saltos del repaso anterior, que leía el diccionario: ahora no se mira, así que da
    /// igual cómo se escriba.
    #[test]
    fn la_bomba_no_se_abre_se_escriba_como_se_escriba_el_diccionario() {
        let bomba = bomba();
        for (dict, entre) in [
            ("/Fil#74er /FlateDecode", "\n"),
            ("/X << /Filter /None >> /Filter /FlateDecode", "\n"),
            ("/Filter /None /Filter /FlateDecode", "\n"),
            ("/T (/Filter /None) /Filter /FlateDecode", "\n"),
            ("/Filter /FlateDecode", "%\n"),
            ("/Filter /FlateDecode", "\0"),
        ] {
            let pdf = pdf_con_flujo_y(dict, entre, &bomba);
            // `lopdf` sí lo descomprime (con el presupuesto de verdad se lee el texto)…
            let t = extract(&pdf);
            assert!(
                t.as_deref()
                    .is_some_and(|t| t.contains("Factura de prueba")),
                "{dict:?} {entre:?}: {t:?}"
            );
            // …y con 1 MB no se abre.
            assert!(abrir_con(&pdf, UN_MEGA).is_none(), "{dict:?} {entre:?}");
        }
        // Tras `stream`, con CRLF, CR o espacios antes del fin de línea, o sin fin de línea.
        for fin in ["\r\n", "\r", " \t\n", ""] {
            let mut pdf = b"%PDF-1.4\n1 0 obj << >>\nstream".to_vec();
            pdf.extend_from_slice(fin.as_bytes());
            pdf.extend_from_slice(&bomba);
            let mut rep = Repaso::new(pdf.len(), UN_MEGA.max_descomprimido);
            assert!(cabe_lo_comprimido(&pdf, &mut rep).is_err(), "{fin:?}");
        }
    }

    /// Cadenas de filtros: también se mide lo que infla un filtro tras otro.
    #[test]
    fn la_bomba_no_se_abre_tras_otros_filtros() {
        let bomba = bomba();
        let doble = zlib(&bomba);
        let a85 = ascii85_de(&bomba);
        for (dict, datos) in [
            ("/Filter [/FlateDecode /FlateDecode]", doble.clone()),
            ("/Filter [/ASCII85Decode /FlateDecode]", a85.clone()),
            (
                "/Filter [/ASCII85Decode /ASCII85Decode /FlateDecode]",
                ascii85_de(&a85),
            ),
            (
                "/Filter [/FlateDecode /ASCII85Decode /FlateDecode]",
                zlib(&a85),
            ),
            (
                "/Filter [/ASCII85Decode /FlateDecode /FlateDecode]",
                ascii85_de(&doble),
            ),
        ] {
            let pdf = pdf_con_flujo(dict, &datos);
            assert!(
                extract(&pdf).unwrap().contains("Factura de prueba"),
                "{dict}"
            );
            assert!(abrir_con(&pdf, UN_MEGA).is_none(), "{dict}");
        }
    }

    fn lzw(datos: &[u8], tardio: bool) -> Vec<u8> {
        use weezl::{BitOrder, encode::Encoder};
        let mut e = if tardio {
            Encoder::new(BitOrder::Msb, 8)
        } else {
            Encoder::with_tiff_size_switch(BitOrder::Msb, 8)
        };
        e.encode(datos).unwrap()
    }

    #[test]
    fn la_bomba_lzw_no_se_abre() {
        let grande = contenido(3, 2 * 1024 * 1024);
        for (dict, datos) in [
            ("/Filter /LZWDecode", lzw(&grande, false)),
            ("/Filter /LZW#44ecode", lzw(&grande, false)),
            (
                "/Filter /LZWDecode /DecodeParms << /EarlyChange 0 >>",
                lzw(&grande, true),
            ),
            ("/Filter [/LZWDecode /FlateDecode]", lzw(&bomba(), false)),
            (
                "/Filter [/FlateDecode /LZWDecode]",
                zlib(&lzw(&grande, false)),
            ),
        ] {
            assert!(datos.len() < 200_000, "{dict}: {}", datos.len());
            let pdf = pdf_con_flujo(dict, &datos);
            assert!(
                extract(&pdf).unwrap().contains("Factura de prueba"),
                "{dict}"
            );
            assert!(abrir_con(&pdf, UN_MEGA).is_none(), "{dict}");
        }
        // Uno normal sí se abre.
        let pdf = pdf_con_flujo("/Filter /LZWDecode", &lzw(&contenido(3, 1000), false));
        let t = extract_con(&pdf, UN_MEGA, MAX_TEXTO).unwrap();
        assert!(t.contains("Factura de prueba"), "{t:?}");
    }

    /// Lo que no crece no gasta: los ceros, que LZW lee como LZW válido (y lo que sale, otra
    /// vez), no se cuentan una y otra vez.
    #[test]
    fn un_flujo_con_muchos_ceros_no_gasta_de_mas() {
        let ceros = vec![0u8; 4 * 1024 * 1024];
        let pdf = pdf_con_flujo("/Filter /FlateDecode", &zlib(&ceros));
        let max = 5 * 1024 * 1024;
        let mut rep = Repaso::new(pdf.len(), max);
        assert!(cabe_lo_comprimido(&pdf, &mut rep).is_ok());
        let gastado = max - rep.gasto.queda;
        assert!(gastado < ceros.len() as u64 + 64 * 1024, "{gastado}");
    }

    #[test]
    fn un_predictor_desmesurado_no_se_abre() {
        let datos = zlib(&contenido(3, 1000));
        for parms in [
            "<< /Predictor 12 /Columns 2000000000 >>",
            "<< /Predictor 12 /Col#75mns %x\n 2000000000 >>",
            "<< /Predictor 12 /Colors 1000000 >>",
            "<< /Predictor 12 /BitsPerComponent +1000000 >>",
        ] {
            let pdf = pdf_con_flujo(
                &format!("/Filter /FlateDecode /DecodeParms {parms}"),
                &datos,
            );
            assert!(abrir_con(&pdf, Topes::DE_VERDAD).is_none(), "{parms}");
        }
        // Los de verdad (una tabla xref en flujo, una imagen) sí.
        for parms in [
            "<< /Predictor 12 /Columns 5 >>",
            "<< /Predictor 15 /Colors 3 /BitsPerComponent 8 /Columns 4000 >>",
            "<< /Columns -7 /Colors 7 0 R >>",
        ] {
            let pdf = pdf_con_flujo(
                &format!("/Filter /FlateDecode /DecodeParms {parms}"),
                &datos,
            );
            assert!(predictores_razonables(&pdf).is_ok(), "{parms}");
        }
    }

    #[test]
    fn el_ascii85_y_los_inicios_de_flujo_son_los_de_lopdf() {
        let leer = |datos: &[u8], capas| {
            let mut g = Gasto {
                queda: 0,
                lectura: u64::MAX,
            };
            con_capas(datos, capas, &mut |f| {
                let mut out = Vec::new();
                let mut buf = [0u8; 3];
                loop {
                    let n = f.leer(&mut buf, &mut g)?;
                    if n == 0 {
                        return Ok(out);
                    }
                    out.extend_from_slice(&buf[..n]);
                }
            })
            .unwrap()
        };
        assert_eq!(leer(&ascii85_de(b"Hola, mundo"), 1), b"Hola, mundo");
        assert_eq!(leer(&ascii85_de(&ascii85_de(b"Hola")), 2), b"Hola");
        assert_eq!(leer(b"z !!!!! \n~>resto", 1), [0u8; 8]);
        assert_eq!(leer(b"Hola", 0), b"Hola");
        for (pdf, fin) in [
            (&b"stream\r\nX"[..], 8),
            (b"stream\nX", 7),
            (b"stream\rX", 7),
            (b"stream \t\nX", 9),
            (b"streamX", 6),
        ] {
            assert_eq!(tras_fin_de_linea(pdf, 6), fin, "{pdf:?}");
        }
    }

    /// El repaso es lineal: un fichero lleno de `>> stream` (que además es ASCII85 válido)
    /// lee poco por cada uno y acaba enseguida.
    #[test]
    fn el_repaso_es_lineal() {
        let pdf = b">> stream\n".repeat(500_000);
        let mut rep = Repaso::new(pdf.len(), MAX_DESCOMPRIMIDO);
        let antes = rep.gasto.lectura;
        let t = std::time::Instant::now();
        assert!(cabe_lo_comprimido(&pdf, &mut rep).is_ok());
        let leido = antes - rep.gasto.lectura;
        // Unos 75 bytes leídos por byte del fichero (unos 750 por `stream`), 7 s en debug.
        assert!(leido < 150 * pdf.len() as u64, "{leido}");
        assert!(
            // TAXPAIN_TEST_HOLGURA: multiplica el tope en máquinas lentas (el Mac Intel de
            // GitHub). Lo que garantiza que es lineal es `leido`; el reloj sólo pilla lo grosero.
            t.elapsed()
                < std::time::Duration::from_secs(
                    30 * std::env::var("TAXPAIN_TEST_HOLGURA")
                        .ok()
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(1)
                ),
            "{:?}",
            t.elapsed()
        );
        assert!(predictores_razonables(&b"/Columns ".repeat(500_000)).is_ok());
    }

    /// Ningún PDF de `fixtures/` (facturas, modelos, extractos de verdad o inventados) se
    /// rechaza con los topes de verdad, ni antes de abrirlo ni después.
    #[test]
    fn los_pdf_de_fixtures_pasan_el_repaso() {
        let raiz = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let mut pendientes = vec![raiz];
        let mut mirados = 0;
        let mut malos = Vec::new();
        while let Some(dir) = pendientes.pop() {
            let Ok(entradas) = std::fs::read_dir(&dir) else {
                continue;
            };
            for e in entradas {
                let p = e.unwrap().path();
                if p.is_dir() {
                    pendientes.push(p);
                    continue;
                }
                if !p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf")) {
                    continue;
                }
                mirados += 1;
                let bytes = std::fs::read(&p).unwrap();
                let mut rep = Repaso::new(bytes.len(), MAX_DESCOMPRIMIDO);
                let mut bien = bytes.len() <= MAX_PDF
                    && predictores_razonables(&bytes).is_ok()
                    && cabe_lo_comprimido(&bytes, &mut rep).is_ok();
                if bien && let Ok(Ok(mut doc)) = catch_unwind(|| Document::load_mem(&bytes)) {
                    if doc.is_encrypted() {
                        bien = cabe_lo_cifrado(&doc, &mut rep).is_ok() && doc.decrypt("").is_ok();
                    }
                    // Y lo que hará `pdf-extract` con él.
                    let t = std::time::Instant::now();
                    bien = bien && cabe_el_trabajo(&doc, Topes::DE_VERDAD).is_ok();
                    // Y lo lejos que quedan de los topes generales (sin los formularios).
                    let (mut ops, mut pila) = (0, 0u64);
                    for id in doc.get_pages().into_values() {
                        let c = doc.get_page_content(id).unwrap_or_default();
                        let Ok(c) = pdf_extract::content::Content::decode(&c) else {
                            continue;
                        };
                        let mut abiertos = 0u64;
                        for op in &c.operations {
                            if !matches!(op.operator.as_str(), "TJ" | "BI") {
                                ops = ops.max(operandos(&op.operands, u64::MAX));
                            }
                            match op.operator.as_str() {
                                "q" => abiertos += 1,
                                "Q" => abiertos = abiertos.saturating_sub(1),
                                _ => {}
                            }
                            pila = pila.max(abiertos);
                        }
                    }
                    eprintln!(
                        "{}: {:?}, {ops} operandos, {pila} q abiertos",
                        p.display(),
                        t.elapsed()
                    );
                }
                if !bien {
                    malos.push(p.display().to_string());
                }
            }
        }
        assert!(mirados > 0, "no hay PDF en fixtures/");
        assert!(malos.is_empty(), "se rechazan:\n{}", malos.join("\n"));
    }

    /// PDF con estos objetos, numerados desde el 1 (el 1 es el catálogo).
    fn pdf_de(objetos: &[Vec<u8>]) -> Vec<u8> {
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objetos.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj ", i + 1).bytes());
            out.extend_from_slice(o);
            out.extend_from_slice(b" endobj\n");
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objetos.len() + 1).bytes());
        for o in offsets {
            out.extend(format!("{o:010} 00000 n \n").bytes());
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

    /// Un objeto de flujo con este diccionario (sin `/Length`) y estos datos.
    fn flujo(dict: &str, datos: &[u8]) -> Vec<u8> {
        let mut f = format!("<< /Length {} {dict} >>\nstream\n", datos.len()).into_bytes();
        f.extend_from_slice(datos);
        f.extend_from_slice(b"\nendstream");
        f
    }

    fn obj(s: &str) -> Vec<u8> {
        s.as_bytes().to_vec()
    }

    /// Catálogo y árbol con una página (el objeto 3), con estos `/Contents` y `/Resources`.
    fn una_pagina(contents: &str, recursos: &str) -> Vec<Vec<u8>> {
        vec![
            obj("<< /Type /Catalog /Pages 2 0 R >>"),
            obj("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            obj(&format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {contents} \
                 /Resources << /Font << /F1 9 0 R >> {recursos} >> >>"
            )),
        ]
    }

    /// Rellena hasta el objeto `n - 1` con `null`, para poner el siguiente en el número `n`.
    fn hasta(objetos: &mut Vec<Vec<u8>>, n: usize) {
        while objetos.len() + 1 < n {
            objetos.push(obj("null"));
        }
    }

    const HELVETICA: &str = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>";

    /// Se abre con los topes de verdad y se lee el texto de prueba.
    fn se_lee(pdf: &[u8]) -> bool {
        abrir(pdf).is_some()
            && extract(pdf)
                .as_deref()
                .is_some_and(|t| t.contains("Factura de prueba"))
    }

    /// El salto de la tercera revisión: `/Contents` que repite mil veces el mismo flujo, que
    /// `get_page_content` descomprime y concatena cada vez. El repaso de antes de abrirlo lo
    /// mide una vez; ahora se mide lo que se concatena.
    #[test]
    fn un_contents_que_repite_un_flujo_no_se_abre() {
        let grande = zlib(&contenido(3, 2 * 1024 * 1024));
        let pdf_con = |veces: usize| {
            let refs = "4 0 R ".repeat(veces);
            let mut o = una_pagina(&format!("[{refs}]"), "");
            o.push(flujo("/Filter /FlateDecode", &grande));
            hasta(&mut o, 9);
            o.push(obj(HELVETICA));
            pdf_de(&o)
        };
        let pdf = pdf_con(1000);
        assert!(pdf.len() < 20_000, "{}", pdf.len());
        // El repaso de los flujos lo deja pasar (2 MB de 256)…
        let mut rep = Repaso::new(pdf.len(), MAX_DESCOMPRIMIDO);
        assert!(predictores_razonables(&pdf).is_ok());
        assert!(cabe_lo_comprimido(&pdf, &mut rep).is_ok());
        // …`lopdf` concatenaría 2 GB…
        let doc = Document::load_mem(&pdf).unwrap();
        let pagina = doc.get_pages()[&1];
        assert_eq!(doc.get_page_contents(pagina).len(), 1000);
        // …y no se abre.
        assert!(cabe_el_trabajo(&doc, Topes::DE_VERDAD).is_err());
        assert!(abrir(&pdf).is_none());
        assert_eq!(extract(&pdf), None);
        // Con tres, sí.
        assert!(se_lee(&pdf_con(3)));
    }

    /// Un formulario grande al que se llama miles de veces: cada `Do` lo descomprime y lo
    /// ejecuta entero.
    #[test]
    fn muchos_do_a_un_formulario_grande_no_se_abren() {
        let form = zlib(&contenido(1, 1024 * 1024));
        let pdf_con = |veces: usize| {
            let pagina = format!(
                "{}{}",
                "q /X Do Q\n".repeat(veces),
                "BT /F1 12 Tf 72 700 Td (Factura de prueba) Tj ET"
            );
            let mut o = una_pagina("4 0 R", "/XObject << /X 5 0 R >>");
            o.push(flujo("", pagina.as_bytes()));
            o.push(flujo(
                "/Type /XObject /Subtype /Form /BBox [0 0 612 792] /Filter /FlateDecode \
                 /Resources << /Font << /F1 9 0 R >> >>",
                &form,
            ));
            hasta(&mut o, 9);
            o.push(obj(HELVETICA));
            pdf_de(&o)
        };
        let pdf = pdf_con(1000);
        assert!(pdf.len() < 50_000, "{}", pdf.len());
        assert!(abrir(&pdf).is_none());
        assert!(se_lee(&pdf_con(10)));
    }

    /// Formularios que se llaman a sí mismos: `pdf-extract` desbordaría la pila (y eso aborta la
    /// app). No se abren, y estas pruebas no llegan a ejecutarlos.
    #[test]
    fn un_formulario_que_se_llama_a_si_mismo_no_se_abre() {
        let form = |recursos: &str, cuerpo: &str| {
            flujo(
                &format!("/Type /XObject /Subtype /Form /BBox [0 0 1 1] {recursos}"),
                cuerpo.as_bytes(),
            )
        };
        let con = |recursos_pagina: &str, formas: Vec<Vec<u8>>| {
            let mut o = una_pagina("4 0 R", recursos_pagina);
            o.push(flujo("", b"/X Do BT /F1 12 Tf (Factura de prueba) Tj ET"));
            o.extend(formas);
            hasta(&mut o, 9);
            o.push(obj(HELVETICA));
            pdf_de(&o)
        };
        let casos = [
            // El 5 se llama a sí mismo con sus recursos.
            con(
                "/XObject << /X 5 0 R >>",
                vec![form("/Resources << /XObject << /X 5 0 R >> >>", "/X Do")],
            ),
            // El 5 no tiene recursos: hereda los de la página, que lo llaman a él.
            con("/XObject << /X 5 0 R >>", vec![form("", "/X Do")]),
            // Un ciclo de dos: 5 → 6 → 5.
            con(
                "/XObject << /X 5 0 R >>",
                vec![
                    form("/Resources << /XObject << /Y 6 0 R >> >>", "/Y Do"),
                    form("/Resources << /XObject << /X 5 0 R >> >>", "/X Do"),
                ],
            ),
        ];
        for pdf in &casos {
            let doc = Document::load_mem(pdf).unwrap();
            assert!(cabe_el_trabajo(&doc, Topes::DE_VERDAD).is_err());
            assert!(abrir(pdf).is_none());
            assert_eq!(extract(pdf), None);
        }
        // Una cadena de formularios sin ciclo: hasta 16 se abre; con 20, no.
        let cadena = |n: usize| {
            let mut formas = Vec::new();
            for k in 0..n {
                let (recursos, cuerpo) = if k + 1 < n {
                    (
                        format!("/Resources << /XObject << /X {} 0 R >> >>", k + 6),
                        "/X Do",
                    )
                } else {
                    (String::new(), "")
                };
                formas.push(form(&recursos, cuerpo));
            }
            // 1 catálogo, 2 árbol, 3 página, 4 su contenido, 5… los formularios y la fuente.
            let mut o = una_pagina("4 0 R", "/XObject << /X 5 0 R >>");
            o[2] = obj(&format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
                 /Resources << /Font << /F1 {} 0 R >> /XObject << /X 5 0 R >> >> >>",
                5 + n
            ));
            o.push(flujo("", b"/X Do BT /F1 12 Tf (Factura de prueba) Tj ET"));
            o.extend(formas);
            o.push(obj(HELVETICA));
            pdf_de(&o)
        };
        assert!(se_lee(&cadena(MAX_HONDURA_FORMAS)), "16 formularios");
        assert!(abrir(&cadena(20)).is_none(), "20 formularios");
    }

    /// Un `/Parent` que da la vuelta: `pdf-extract` subiría por él sin parar buscando
    /// `/MediaBox`.
    #[test]
    fn un_parent_en_bucle_no_se_abre() {
        let mut o = vec![
            obj("<< /Type /Catalog /Pages 2 0 R >>"),
            obj("<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
            obj("<< /Type /Page /Parent 4 0 R /Contents 5 0 R >>"),
            obj("<< /Type /Pages /Parent 3 0 R >>"),
        ];
        o.push(flujo("", b"BT ET"));
        let pdf = pdf_de(&o);
        let doc = Document::load_mem(&pdf).unwrap();
        assert_eq!(doc.get_pages().len(), 1);
        assert!(abrir(&pdf).is_none());
        assert_eq!(extract(&pdf), None);
    }

    #[test]
    fn un_anidamiento_desmesurado_no_se_abre() {
        let con = |dentro: &str| {
            let mut o = una_pagina("4 0 R", &format!("/X {dentro}"));
            o.push(flujo("", contenido(1, 0).as_slice()));
            hasta(&mut o, 9);
            o.push(obj(HELVETICA));
            pdf_de(&o)
        };
        for hondo in [
            format!("{}{}", "[".repeat(300), "]".repeat(300)),
            format!("{}{}", "<< /K ".repeat(300), ">>".repeat(300)),
        ] {
            let pdf = con(&hondo);
            assert!(anidamiento_razonable(&pdf).is_err());
            assert!(abrir(&pdf).is_none());
        }
        // Lo mismo en una cadena, en un comentario o en un flujo no cuenta.
        for no in [
            format!("({})", "[".repeat(300)),
            format!("1 %{}\n", "[".repeat(300)),
        ] {
            assert!(anidamiento_razonable(&con(&no)).is_ok(), "{no:.20}");
        }
        let mut o = una_pagina("4 0 R", "");
        o.push(flujo(
            "",
            format!("{} {}", "[".repeat(300), contenido(1, 0).escape_ascii()).as_bytes(),
        ));
        assert!(anidamiento_razonable(&pdf_de(&o)).is_ok());
        // Lo normal se abre.
        assert!(se_lee(&con(&format!(
            "{}{}",
            "[".repeat(20),
            "]".repeat(20)
        ))));
    }

    #[test]
    fn un_contenido_anidado_de_mas_no_se_abre() {
        let con = |n: usize| {
            let mut o = una_pagina("4 0 R", "");
            o.push(flujo(
                "",
                format!(
                    "{}{} {}",
                    "[".repeat(n),
                    "]".repeat(n),
                    contenido(1, 0).escape_ascii()
                )
                .as_bytes(),
            ));
            hasta(&mut o, 9);
            o.push(obj(HELVETICA));
            pdf_de(&o)
        };
        assert!(abrir(&con(300)).is_none());
        assert!(se_lee(&con(20)));
    }

    /// `lopdf` 0.42 para él solo en 100 niveles (`MAX_NESTING_DEPTH`) y en 100 paréntesis
    /// dentro de una cadena (`MAX_BRACKET`), y llegar a ellos cabe de sobra en la pila de un
    /// hilo de 1 MB (los de la app tienen 2 MB o más).
    #[test]
    fn lopdf_aguanta_su_tope_de_anidamiento_en_poca_pila() {
        std::thread::Builder::new()
            .stack_size(1024 * 1024)
            .spawn(|| {
                for n in [99, 150, 100_000] {
                    let hondo = format!(
                        "{}({}{}){}",
                        "[ << /K ".repeat(n / 2),
                        "(".repeat(n),
                        ")".repeat(n),
                        " >> ]".repeat(n / 2)
                    );
                    let mut o = una_pagina("4 0 R", &format!("/X {hondo}"));
                    o.push(flujo("", format!("BT {hondo} Tj ET").as_bytes()));
                    hasta(&mut o, 9);
                    o.push(obj(HELVETICA));
                    let pdf = pdf_de(&o);
                    let doc = Document::load_mem(&pdf);
                    if let Ok(doc) = doc {
                        let pagina = doc.get_pages().get(&1).copied().unwrap_or((3, 0));
                        let _ = doc.get_page_content(pagina).map(|c| {
                            pdf_extract::content::Content::decode(&c).map(|c| c.operations.len())
                        });
                    }
                }
            })
            .unwrap()
            .join()
            .unwrap();
    }

    /// Un CMap anidado: `adobe-cmap-parser` lo leería sin tope y desbordaría la pila.
    #[test]
    fn un_cmap_anidado_no_se_abre() {
        let con = |cmap: &str| {
            let mut o = una_pagina("4 0 R", "");
            o.push(flujo("", contenido(1, 0).as_slice()));
            o.push(flujo("/Filter /FlateDecode", &zlib(cmap.as_bytes())));
            hasta(&mut o, 9);
            o.push(obj(
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 5 0 R >>",
            ));
            pdf_de(&o)
        };
        let bien = "1 beginbfchar <0041> <0041> endbfchar";
        assert!(se_lee(&con(bien)));
        for n in [100, 100_000] {
            let mal = format!("{}{}", "[".repeat(n), "]".repeat(n));
            assert!(abrir(&con(&mal)).is_none(), "{n} corchetes");
            let mal = format!("{}{}", "(".repeat(n), ")".repeat(n));
            assert!(abrir(&con(&mal)).is_none(), "{n} paréntesis");
        }
    }

    /// Un `beginbfrange` enorme: `adobe-cmap-parser` mete cada código en una tabla.
    #[test]
    fn un_tounicode_con_rangos_enormes_no_se_abre() {
        let con = |cmap: &str| {
            let mut o = una_pagina("4 0 R", "");
            o.push(flujo("", contenido(1, 0).as_slice()));
            o.push(flujo("", cmap.as_bytes()));
            hasta(&mut o, 9);
            o.push(obj(
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 5 0 R >>",
            ));
            pdf_de(&o)
        };
        assert!(se_lee(&con(
            "1 beginbfrange <0020> <007E> <0020> endbfrange"
        )));
        for mal in [
            "1 beginbfrange <00000000> <FFFFFFFF> <0000> endbfrange",
            "1 beginbfrange <00000000> <00FFFFFF> <00000000> endbfrange",
        ] {
            let pdf = con(mal);
            assert!(abrir(&pdf).is_none(), "{mal}");
            assert_eq!(extract(&pdf), None);
        }
        assert_eq!(
            codigos_del_cmap(b"2 beginbfrange <00> <0F> <0041> <10> <11> [<0041> <0042>] endbfrange 1 beginbfchar <01> <0041> endbfchar"),
            19
        );
    }

    /// Miles de `Tf` a una fuente grande: `pdf-extract` la crea una vez por página (la guarda
    /// por nombre), así que se abre; la misma fuente en cientos de páginas, no.
    #[test]
    fn la_fuente_se_cuenta_una_vez_por_pagina() {
        let cmap = format!(
            "1 beginbfchar <0041> <0041> endbfchar{}",
            " ".repeat(1024 * 1024)
        );
        let con = |paginas: usize| {
            let kids: String = (0..paginas).map(|k| format!("{} 0 R ", k + 10)).collect();
            let mut o = vec![
                obj("<< /Type /Catalog /Pages 2 0 R >>"),
                obj(&format!(
                    "<< /Type /Pages /Kids [{kids}] /Count {paginas} >>"
                )),
                obj("null"),
            ];
            let tfs = "/F1 12 Tf ".repeat(5000);
            o.push(flujo(
                "",
                format!("BT {tfs} 72 700 Td (Factura de prueba) Tj ET").as_bytes(),
            ));
            o.push(flujo("/Filter /FlateDecode", &zlib(cmap.as_bytes())));
            hasta(&mut o, 9);
            o.push(obj(
                "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 5 0 R >>",
            ));
            for _ in 0..paginas {
                o.push(obj(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
                     /Resources << /Font << /F1 9 0 R >> >> >>",
                ));
            }
            pdf_de(&o)
        };
        assert!(abrir(&con(2)).is_some());
        assert!(abrir(&con(600)).is_none());
    }

    /// Miles de `cs` a un perfil ICC grande (cada vez se descomprime) o de `q` tras él (cada vez
    /// se copia).
    #[test]
    fn muchos_cs_o_q_con_un_icc_grande_no_se_abren() {
        let icc = zlib(&vec![7u8; 1024 * 1024]);
        let con = |cuerpo: &str| {
            let mut o = una_pagina("4 0 R", "/ColorSpace << /C0 [/ICCBased 5 0 R] >>");
            o.push(flujo(
                "",
                format!("{cuerpo} BT /F1 12 Tf 72 700 Td (Factura de prueba) Tj ET").as_bytes(),
            ));
            o.push(flujo("/N 3 /Filter /FlateDecode", &icc));
            hasta(&mut o, 9);
            o.push(obj(HELVETICA));
            pdf_de(&o)
        };
        assert!(se_lee(&con(&"/C0 cs ".repeat(10))));
        assert!(abrir(&con(&"/C0 cs ".repeat(1000))).is_none());
        assert!(abrir(&con(&format!("/C0 cs /C0 CS {}", "q ".repeat(1000)))).is_none());
        // Los espacios de siempre no cuestan.
        assert!(se_lee(&con(&"/DeviceRGB cs q Q ".repeat(10_000))));
    }

    /// Página con este contenido delante del texto de prueba, y un formulario `/X` con este
    /// otro contenido.
    fn con_forma(pagina: &str, forma: &str) -> Vec<u8> {
        let mut o = una_pagina("4 0 R", "/XObject << /X 5 0 R >>");
        o.push(flujo(
            "",
            format!("{pagina} BT /F1 12 Tf 72 700 Td (Factura de prueba) Tj ET").as_bytes(),
        ));
        o.push(flujo(
            "/Type /XObject /Subtype /Form /BBox [0 0 612 792]",
            forma.as_bytes(),
        ));
        hasta(&mut o, 9);
        o.push(obj(HELVETICA));
        pdf_de(&o)
    }

    /// El salto de la quinta revisión: `pdf-extract` copia todos los operandos de `sc` al
    /// estado gráfico y cada `q` lo copia, así que un `sc` con 50.000 operandos y 50.000 `q`
    /// son gigas en un PDF de pocos KB. Ahora ningún operador lleva más de [`MAX_OPERANDOS`].
    #[test]
    fn un_operador_con_demasiados_operandos_no_se_abre() {
        let sc = |n: usize| format!("/DeviceRGB cs {}sc ", "0.5 ".repeat(n));
        // El del auditor, con el contenido comprimido.
        let auditor = format!("{}{}", sc(50_000), "q ".repeat(50_000));
        let mut o = una_pagina("4 0 R", "");
        o.push(flujo(
            "/Filter /FlateDecode",
            &zlib(format!("{auditor} BT /F1 12 Tf 72 700 Td (Factura de prueba) Tj ET").as_bytes()),
        ));
        hasta(&mut o, 9);
        o.push(obj(HELVETICA));
        let pdf = pdf_de(&o);
        assert!(pdf.len() < 10_000, "{}", pdf.len());
        assert!(abrir(&pdf).is_none());
        // Basta el `sc`, sin `q`; y también en un formulario, o con `SC`/`scn`/`SCN`.
        assert!(abrir(&con_forma(&sc(50_000), "")).is_none());
        assert!(abrir(&con_forma("/X Do", &sc(65))).is_none());
        for op in ["SC", "scn", "SCN"] {
            let c = format!("{}{op}", "1 ".repeat(65));
            assert!(abrir(&con_forma(&c, "")).is_none(), "{op}");
        }
        // Cuenta lo que va dentro de arrays y diccionarios (un `d`, un `BDC`) y en cualquier
        // operador, también en uno que `pdf-extract` no conoce.
        let rayas = format!("[{}] 0 d", "3 ".repeat(100));
        assert!(abrir(&con_forma(&rayas, "")).is_none());
        let props: String = (0..100).map(|k| format!("/K{k} {k} ")).collect();
        assert!(abrir(&con_forma(&format!("/Span <<{props}>> BDC EMC"), "")).is_none());
        assert!(abrir(&con_forma(&format!("{}zz", "1 ".repeat(65)), "")).is_none());
        // Lo de verdad sí: un `DeviceN` de 32 componentes con muchos `q`, un `d`, un `BDC` con
        // sus propiedades y un `TJ` largo (su array no cuenta).
        let normal = format!(
            "{}{} [3 2] 0 d /Span << /Lang (es-ES) /MCID 0 /ActualText (x) >> BDC EMC              BT /F1 12 Tf 72 600 Td [{}] TJ ET",
            sc(32),
            "q 1 0 0 1 0 0 cm Q ".repeat(10_000),
            "(a) -3 ".repeat(5_000),
        );
        assert!(se_lee(&con_forma(&normal, "")));
    }

    /// `q` sin su `Q`: cada uno guarda una copia del estado gráfico hasta el final del
    /// contenido. No se admiten más de [`MAX_PILA`] abiertos a la vez, contando los de los
    /// formularios con los que había abiertos al llamarlos.
    #[test]
    fn una_pila_de_q_desmesurada_no_se_abre() {
        let q = |n: usize| "q ".repeat(n);
        assert!(abrir(&con_forma(&q(10_000), "")).is_none());
        // Lo que se cierra no se acumula.
        assert!(se_lee(&con_forma(&"q Q ".repeat(10_000), "")));
        assert!(se_lee(&con_forma(&q(1000), "")));
        // Los del formulario se suman a los abiertos al llamarlo, y solo a esos.
        assert!(abrir(&con_forma(&format!("{} /X Do", q(600)), &q(600))).is_none());
        assert!(se_lee(&con_forma(&format!("{} /X Do", q(600)), &q(300))));
        let cerrados = format!("{}{} /X Do", q(600), "Q ".repeat(600));
        assert!(se_lee(&con_forma(&cerrados, &q(600))));
        // Un `Q` de más no cierra nada.
        assert!(abrir(&con_forma(&format!("Q Q {}", q(1025)), "")).is_none());
    }

    /// Un contenido con muchas fichas pide mucha memoria al analizarlo (cada `Object` de `lopdf`
    /// ocupa más de 100 bytes), aunque descomprima poco.
    #[test]
    fn un_contenido_con_demasiadas_fichas_no_se_abre() {
        // 2 bytes por ficha: 64 MB son 32 millones de fichas, más de 3 GB analizados.
        let mucho = zlib(&b"1 ".repeat(32 * 1024 * 1024));
        let mut o = una_pagina("4 0 R", "");
        o.push(flujo("/Filter /FlateDecode", &mucho));
        hasta(&mut o, 9);
        o.push(obj(HELVETICA));
        let pdf = pdf_de(&o);
        assert!(abrir(&pdf).is_none());
        // Una imagen grande no se analiza: `lopdf` para en el primer byte que no vale.
        let imagen = zlib(&vec![0xFFu8; 8 * 1024 * 1024]);
        let mut o = una_pagina("4 0 R", "/XObject << /I 5 0 R >>");
        o.push(flujo(
            "",
            b"q /I Do Q BT /F1 12 Tf 72 700 Td (Factura de prueba) Tj ET",
        ));
        o.push(flujo(
            "/Type /XObject /Subtype /Image /Width 2048 /Height 1024 /ColorSpace /DeviceRGB \
             /BitsPerComponent 8 /Filter /FlateDecode",
            &imagen,
        ));
        hasta(&mut o, 9);
        o.push(obj(HELVETICA));
        let pdf = pdf_de(&o);
        let doc = Document::load_mem(&pdf).unwrap();
        assert!(cabe_el_trabajo(&doc, Topes::DE_VERDAD).is_ok());
    }

    #[test]
    fn la_lectura_de_la_sintaxis_cuenta_de_mas_y_se_para_donde_lopdf() {
        let l = |d: &[u8]| leer_sintaxis(d, Sintaxis::Contenido);
        let c = b"q 1 0 0 1 72.5 -720 cm BT /F1 12 Tf (Hola (y) adi\\)os) Tj [(a) -3 (b)] TJ ET Q";
        let ops = pdf_extract::content::Content::decode(c).unwrap();
        let objetos: usize = ops.operations.iter().map(|o| o.operands.len() + 1).sum();
        assert!(
            l(c).fichas as usize >= objetos + 3,
            "{} {objetos}",
            l(c).fichas
        );
        // Pegados.
        for (d, n) in [
            (&b"1.5.5"[..], 3),
            (b"truefalsenull", 3),
            (b"1-1-1", 3),
            (b"12Tf", 2),
        ] {
            assert!(objetos_en_racha(d) >= n, "{d:?}");
        }
        // Se para en lo que no puede empezar nada (los datos de una imagen)…
        assert_eq!(l(b"q Q \xFF\xD8 1 2 3 4 5 6 7 8").fichas, 2);
        // …salvo tras `BI`, donde cuenta todo.
        assert_eq!(l(b"BI /W 1 ID \xFF EI 1 2").fichas, 12);
        // La hondura: arrays, diccionarios y paréntesis de una cadena.
        assert_eq!(l(b"[[<< /A (a(b(c))) >>]]").hondura, 6);
        assert_eq!(leer_sintaxis(b"{{{ }}}", Sintaxis::Type1).hondura, 3);
        assert_eq!(leer_sintaxis(b"{{{ }}}", Sintaxis::CMap).hondura, 0);
    }

    /// Los PDF que hace la propia app (facturas, informes) se siguen abriendo.
    #[test]
    fn los_pdf_de_la_app_se_abren() {
        use taxpain_core::invoice::{
            Invoice, InvoiceKind, Party, PartyId, RoundingMode, compute_totals,
        };
        use taxpain_invoicing::pdf::{self, Para, PdfOptions};
        let factura = Invoice {
            kind: InvoiceKind::Completa,
            number: "A-1".into(),
            issue_date: time::macros::date!(2026 - 10 - 24),
            operation_date: None,
            issuer: Party {
                id: PartyId::Nif("B12345674".into()),
                name: "Emisor de prueba".into(),
                address: None,
                person: None,
            },
            recipient: Some(Party {
                id: PartyId::Nif("12345678Z".into()),
                name: "Cliente de prueba".into(),
                address: None,
                person: None,
            }),
            lines: vec![],
            rectified: vec![],
            description: "Servicios de prueba".into(),
            notes: None,
        };
        let totales = compute_totals(&factura.lines, RoundingMode::default());
        let opciones = PdfOptions {
            language: pdf::Language::Es,
            verifactu: None,
            footer: Some("Pago por transferencia".into()),
            currency: None,
        };
        let pdf = pdf::render(&factura, &totales, &opciones).unwrap();
        assert!(abrir(&pdf).is_some());
        assert!(text(&pdf).unwrap().contains("Cliente de prueba"));
        let filas: Vec<Para> = (0..400)
            .map(|k| Para::Row(format!("Fila {k}"), format!("{k},00")))
            .collect();
        let pdf = pdf::text_document("Informe de prueba", &[filas]).unwrap();
        assert!(abrir(&pdf).is_some());
        assert!(text(&pdf).unwrap().contains("Fila 399"));
    }

    /// La comprobación es lineal: miles de páginas con su contenido y sus fuentes se miden
    /// enseguida (y una vez cada cosa).
    #[test]
    fn la_cota_es_lineal() {
        let paginas = 1000;
        let kids: String = (0..paginas)
            .map(|k| format!("{} 0 R ", 2 * k + 10))
            .collect();
        let mut o = vec![
            obj("<< /Type /Catalog /Pages 2 0 R >>"),
            obj(&format!(
                "<< /Type /Pages /Kids [{kids}] /Count {paginas} >>"
            )),
        ];
        hasta(&mut o, 9);
        o.push(obj(HELVETICA));
        let texto = "(Factura de prueba) Tj T* ".repeat(200);
        for _ in 0..paginas {
            let n = o.len() + 2;
            o.push(obj(&format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {n} 0 R \
                 /Resources << /Font << /F1 9 0 R >> >> >>"
            )));
            o.push(flujo(
                "",
                format!("BT /F1 12 Tf 14 TL 72 720 Td {texto} ET").as_bytes(),
            ));
        }
        let pdf = pdf_de(&o);
        let doc = Document::load_mem(&pdf).unwrap();
        let t = std::time::Instant::now();
        assert!(cabe_el_trabajo(&doc, Topes::DE_VERDAD).is_ok());
        let cota = t.elapsed();
        let t = std::time::Instant::now();
        let mut s = String::new();
        pdf_extract::output_doc(&doc, &mut pdf_extract::PlainTextOutput::new(&mut s)).unwrap();
        let leer = t.elapsed();
        // La comprobación analiza cada contenido una vez, como `pdf-extract`.
        assert!(
            cota < leer * 3 + std::time::Duration::from_secs(1),
            "{cota:?} {leer:?}"
        );
    }

    /// `ASCII85Encode`, con `~>` al final.
    fn ascii85_de(datos: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for trozo in datos.chunks(4) {
            let mut g = [0u8; 4];
            g[..trozo.len()].copy_from_slice(trozo);
            let mut n = u32::from_be_bytes(g);
            let mut c = [0u8; 5];
            for k in (0..5).rev() {
                c[k] = (n % 85) as u8 + b'!';
                n /= 85;
            }
            out.extend_from_slice(&c[..trozo.len() + 1]);
        }
        out.extend_from_slice(b"~>");
        out
    }
}
