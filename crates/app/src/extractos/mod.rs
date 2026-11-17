//! Extractos bancarios de cualquier banco y en cualquier formato.
//!
//! Cada lector convierte su formato (Norma 43, camt.05x, OFX, QIF, MT940, hojas de cálculo,
//! CSV, tablas HTML, PDF…) en un [`Extracto`]: la cuenta, su divisa, el periodo, los saldos y
//! los movimientos con su signo (+ entra dinero, − sale), en orden cronológico. Antes de
//! importarlo se comprueba con sus saldos ([`cuadre`]): un extracto que no cuadra no entra a
//! ciegas.

pub(crate) mod cuadre;
pub(crate) mod ia;
pub(crate) mod iban;
pub mod importar;
pub(crate) mod marcado;
pub(crate) mod mime;
pub(crate) mod mt940;
pub(crate) mod numeros;
pub(crate) mod ofx;
pub(crate) mod pdf;
pub(crate) mod qif;
pub(crate) mod tabla;

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;
use sha2::{Digest, Sha256};
use taxpain_core::bank::N43Account;
use taxpain_core::money::Money;
use time::Date;

pub use cuadre::Cuadre;

use crate::error::{AppError, Result};
use crate::import::Hoja;

/// Un movimiento del extracto.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Movimiento {
    /// Fecha de la operación.
    pub fecha: Date,
    /// Fecha valor, si el extracto la da.
    pub fecha_valor: Option<Date>,
    pub concepto: String,
    /// + abono (entra dinero), − cargo (sale), en céntimos de la divisa de la cuenta.
    pub importe: Money,
    /// Saldo después del movimiento, si el extracto lo da.
    pub saldo: Option<Money>,
    /// Referencia o número de documento del banco, si lo hay.
    pub referencia: String,
    /// En una cuenta en otra divisa, los euros del movimiento si el propio extracto los da (la
    /// columna del importe en la divisa de origen de Revolut Business, el cambio a euros de
    /// Wise…), con el signo del movimiento. Se guardan como su contravalor al importarlo.
    pub importe_eur: Option<Money>,
}

/// Qué es la cuenta del extracto.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Clase {
    /// Cuenta corriente o de pago (también las de Revolut, Wise…).
    Cuenta,
    /// Tarjeta de crédito: sus compras salen en la cuenta como un cargo al liquidarse.
    TarjetaCredito,
    /// Tarjeta de débito: sus compras ya salen una a una en la cuenta.
    TarjetaDebito,
    /// Monedero o pasarela (PayPal, Stripe…).
    Monedero,
}

/// Un extracto leído, de una sola cuenta y una sola divisa.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Extracto {
    /// Formato de origen, para mostrarlo («Norma 43», «OFX», «Hoja de cálculo», «PDF»…).
    pub formato: String,
    /// Nombre del banco, si se sabe.
    pub banco: String,
    /// IBAN completo (sin espacios), si el extracto lo da.
    pub iban: Option<String>,
    /// Clave estable de la cuenta: con un IBAN español, entidad + oficina + cuenta (la misma que
    /// la de su Norma 43); con otro IBAN, el IBAN; de una tarjeta, `tarjeta:<entidad>:<últimos>`;
    /// si sólo se ven unos dígitos de la cuenta, la clave provisional de
    /// [`iban::Enmascarada::clave`], que la importación intenta casar con una cuenta conocida.
    /// La cuenta se identifica por esta clave y la divisa juntas: una misma cuenta con varias
    /// divisas (Revolut, Wise…) da un extracto por divisa.
    pub cuenta: String,
    pub titular: String,
    /// Divisa ISO 4217 de la cuenta («EUR», «USD»…).
    pub divisa: String,
    pub clase: Clase,
    pub desde: Option<Date>,
    pub hasta: Option<Date>,
    pub saldo_inicial: Option<Money>,
    pub saldo_final: Option<Money>,
    /// En orden cronológico.
    pub movimientos: Vec<Movimiento>,
    /// Lo que el lector quiere que vea el usuario (columnas dudosas, filas saltadas…).
    pub avisos: Vec<String>,
}

impl Extracto {
    /// Extracto vacío de un formato, para que los lectores lo vayan rellenando.
    pub(crate) fn nuevo(formato: &str) -> Self {
        Self {
            formato: formato.to_owned(),
            banco: String::new(),
            iban: None,
            cuenta: String::new(),
            titular: String::new(),
            divisa: "EUR".to_owned(),
            clase: Clase::Cuenta,
            desde: None,
            hasta: None,
            saldo_inicial: None,
            saldo_final: None,
            movimientos: Vec::new(),
            avisos: Vec::new(),
        }
    }

    /// Pone el IBAN y, con él, la clave de la cuenta y el banco (si no lo tenía).
    pub(crate) fn con_iban(&mut self, iban_: &str) {
        let compacto = iban::compactar(iban_);
        self.cuenta = iban::clave(&compacto);
        if self.banco.is_empty()
            && let Some(b) = iban::banco(&compacto)
        {
            self.banco = b.to_owned();
        }
        self.iban = Some(compacto);
    }

    /// Pone la clave de una tarjeta: la entidad (si se sabe) y sus últimos dígitos, nunca los
    /// primeros (son los del emisor de la tarjeta, iguales para muchas).
    pub(crate) fn con_tarjeta(&mut self, entidad: Option<&str>, ultimos: &str) {
        self.cuenta = format!("tarjeta:{}:{ultimos}", entidad.unwrap_or("?"));
    }

    /// Ordena los movimientos de más antiguo a más nuevo (muchos bancos los dan al revés),
    /// completa el periodo y los saldos que se deducen de los movimientos y comprueba que
    /// cuadran.
    pub(crate) fn cerrar(&mut self) -> cuadre::Cuadre {
        cuadre::ordenar(&mut self.movimientos);
        if self.desde.is_none() {
            self.desde = self.movimientos.first().map(|m| m.fecha);
        }
        if self.hasta.is_none() {
            self.hasta = self.movimientos.last().map(|m| m.fecha);
        }
        cuadre::completar_saldos(self);
        let (cuadre, dias) = cuadre::comprobar(self);
        if let (Some(primero), true) = (dias.first(), cuadre.cuadra()) {
            let lista = if dias.len() <= 3 {
                dias.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            } else {
                format!("{primero} y {} días más", dias.len() - 1)
            };
            self.avisos.push(format!(
                "El saldo de las filas no encadena dentro del día ({lista}), pero el día entero \
                 cuadra: el banco lista esos movimientos en otro orden o con uno anulado el mismo \
                 día."
            ));
        }
        cuadre
    }
}

/// Un extracto leído y lo que ha salido al comprobarlo con sus saldos.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Leido {
    pub extracto: Extracto,
    pub cuadre: Cuadre,
}

fn otro(m: impl Into<String>) -> AppError {
    AppError::Other(m.into())
}

/// Lee un extracto bancario de cualquier formato: Norma 43, camt.05x, OFX/QFX, QIF, MT940,
/// hojas de cálculo (`.xlsx`, `.xls`, `.ods`, también las tablas HTML y el XML de Excel 2003
/// que algunos bancos guardan como `.xls`), CSV y PDF (con texto o escaneado, si hay lector de
/// texto en el equipo). Devuelve un extracto por cuenta, ya comprobado con sus saldos. Error si
/// no es un extracto o no se ha podido leer.
pub fn leer(nombre: &str, bytes: &[u8]) -> Result<Vec<Leido>> {
    // Los últimos leídos se guardan por huella y extensión (el lector decide por ella: imagen,
    // «.pdf» dentro de un correo, QIF, y la clave «hoja:<nombre>» lleva el nombre): la lectura
    // de la carpeta, la revisión y la importación miran los mismos ficheros. Los errores no se
    // guardan (un fallo pasajero del lector de texto se repetiría toda la sesión) y la memoria
    // se acota por entradas y por movimientos.
    static RECIENTES: Mutex<VecDeque<(String, Vec<Leido>)>> = Mutex::new(VecDeque::new());
    const MAX_ENTRADAS: usize = 128;
    const MAX_MOVIMIENTOS: usize = 200_000;
    let extension = nombre
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    let clave = format!("{}:{extension}", hex::encode(Sha256::digest(bytes)));
    if let Ok(c) = RECIENTES.lock()
        && let Some((_, r)) = c.iter().find(|(k, _)| *k == clave)
    {
        return Ok(r.clone());
    }
    let r = leer_sin_cache(nombre, bytes)?;
    if let Ok(mut c) = RECIENTES.lock() {
        let movimientos = |l: &[Leido]| {
            l.iter()
                .map(|x| x.extracto.movimientos.len())
                .sum::<usize>()
        };
        let nuevos = movimientos(&r);
        let mut total: usize = c.iter().map(|(_, l)| movimientos(l)).sum();
        while let Some((_, primero)) = c.front()
            && (c.len() >= MAX_ENTRADAS || total + nuevos > MAX_MOVIMIENTOS)
        {
            total -= movimientos(primero);
            c.pop_front();
        }
        if nuevos <= MAX_MOVIMIENTOS {
            c.push_back((clave, r.clone()));
        }
    }
    Ok(r)
}

/// Como [`leer`], pero si ningún lector lo reconoce (o no cuadra) y hay una lectura de IA con
/// las columnas o los movimientos, con esa lectura (también comprobada con sus saldos).
pub fn leer_con_lectura(
    nombre: &str,
    bytes: &[u8],
    lectura: Option<&crate::lectura_ia::Reading>,
) -> Result<Vec<Leido>> {
    let leido = leer(nombre, bytes);
    let bien = leido.as_ref().is_ok_and(|l| {
        l.iter()
            .all(|x| !matches!(x.cuadre, Cuadre::NoCuadra { .. }))
    });
    if bien {
        return leido;
    }
    let Some(ia_) = lectura.and_then(|r| ia::desde_lectura(r, nombre, bytes)) else {
        return leido;
    };
    let con_ia: Vec<Leido> = ia_
        .into_iter()
        .map(|mut e| {
            let cuadre = e.cerrar();
            Leido {
                extracto: e,
                cuadre,
            }
        })
        .collect();
    // La lectura de IA sólo gana si cuadra al menos tan bien como la de los lectores.
    let cuadran = |l: &[Leido]| l.iter().filter(|x| x.cuadre == Cuadre::Cuadra).count();
    match leido {
        Ok(l) if cuadran(&l) >= cuadran(&con_ia) => Ok(l),
        _ if con_ia.is_empty() => Err(otro(format!("{nombre}: la lectura no trae movimientos"))),
        _ => Ok(con_ia),
    }
}

fn leer_sin_cache(nombre: &str, bytes: &[u8]) -> Result<Vec<Leido>> {
    let extractos = extraer(nombre, bytes)?;
    let mut out: Vec<Leido> = extractos
        .into_iter()
        .filter(|e| !e.movimientos.is_empty() || e.saldo_final.is_some())
        .map(|mut e| {
            let cuadre = e.cerrar();
            Leido {
                extracto: e,
                cuadre,
            }
        })
        .collect();
    if out.is_empty() {
        return Err(otro(format!(
            "{nombre}: no se ha encontrado ningún movimiento bancario"
        )));
    }
    out.sort_by(|a, b| a.extracto.cuenta.cmp(&b.extracto.cuenta));
    Ok(out)
}

/// ¿Es un libro de Excel u OpenDocument binario (zip u OLE)?
fn es_libro(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0])
}

fn extraer(nombre: &str, bytes: &[u8]) -> Result<Vec<Extracto>> {
    if bytes.starts_with(b"%PDF") {
        return pdf::leer(nombre, bytes);
    }
    // Una foto o captura del extracto: con el lector de texto del equipo, si lo hay.
    if crate::received::IMAGE_EXTENSIONS
        .iter()
        .any(|e| nombre.to_lowercase().ends_with(&format!(".{e}")))
    {
        let texto = crate::carpeta::ocr_text(nombre, bytes)
            .ok_or_else(|| otro(format!("{nombre}: imagen sin texto reconocible")))?;
        return pdf::leer_texto(nombre, &texto);
    }
    if es_libro(bytes) {
        let hojas = crate::import::spreadsheet_sheets(bytes)?;
        return Ok(tabla::leer_hojas(nombre, &hojas));
    }
    let lower = nombre.to_lowercase();
    let cabeza = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]).to_ascii_lowercase();
    if (lower.ends_with(".pdf")
        || cabeza.contains("mime-version")
        || cabeza.contains("content-type:"))
        && let Some(pdf_) = mime::pdf_dentro(bytes)
    {
        return pdf::leer(nombre, &pdf_);
    }
    let texto = texto_de(bytes);
    let t = texto.trim_start_matches('\u{feff}').trim_start();
    if crate::carpeta::is_norma43(t.as_bytes()) {
        let cuentas =
            taxpain_core::bank::parse_n43(t).map_err(|e| otro(format!("Norma 43: {e}")))?;
        return Ok(cuentas.iter().map(|a| desde_n43(a, "Norma 43")).collect());
    }
    if t.starts_with('<') && t.contains("BkToCstmr") {
        let cuentas = taxpain_core::camt::parse_camt(t).map_err(|e| otro(format!("camt: {e}")))?;
        return Ok(cuentas.iter().map(|a| desde_n43(a, "camt.053")).collect());
    }
    if ofx::es_ofx(t) {
        return Ok(ofx::leer(t));
    }
    if mt940::es_mt940(t) {
        return Ok(mt940::leer(t));
    }
    if qif::es_qif(t) {
        return Ok(qif::leer(t, nombre));
    }
    if let Some(hojas) = marcado::hojas_spreadsheetml(t).or_else(|| marcado::hojas_html(t)) {
        return Ok(tabla::leer_hojas(nombre, &hojas));
    }
    // Lo demás, como CSV o texto separado por tabuladores.
    let hojas = [Hoja {
        nombre: String::new(),
        filas: crate::import::csv(texto.as_bytes()),
    }];
    Ok(tabla::leer_hojas(nombre, &hojas))
}

/// Texto de un fichero: UTF-16 (con su marca o por los ceros), UTF-8 o, si no, Latin-1.
pub(crate) fn texto_de(bytes: &[u8]) -> String {
    let utf16 = |be: bool, b: &[u8]| -> String {
        let unidades: Vec<u16> = b
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| {
                if be {
                    u16::from_be_bytes(*c)
                } else {
                    u16::from_le_bytes(*c)
                }
            })
            .collect();
        String::from_utf16_lossy(&unidades)
    };
    if let Some(b) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(false, b);
    }
    if let Some(b) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(true, b);
    }
    // Sin marca: muchos ceros en las posiciones impares (o pares) delatan UTF-16.
    let muestra = &bytes[..bytes.len().min(400)];
    if muestra.len() >= 8 {
        let ceros_impares = muestra
            .iter()
            .skip(1)
            .step_by(2)
            .filter(|b| **b == 0)
            .count();
        let ceros_pares = muestra.iter().step_by(2).filter(|b| **b == 0).count();
        let mitad = muestra.len() / 4;
        if ceros_impares > mitad && ceros_pares == 0 {
            return utf16(false, bytes);
        }
        if ceros_pares > mitad && ceros_impares == 0 {
            return utf16(true, bytes);
        }
    }
    crate::import::decode(bytes)
}

/// Divisa ISO 4217 en letras a partir del código numérico de la Norma 43 («978»).
fn divisa_iso(codigo: &str) -> String {
    match codigo.trim() {
        "978" | "" => "EUR",
        "840" => "USD",
        "826" => "GBP",
        "756" => "CHF",
        "392" => "JPY",
        "124" => "CAD",
        "036" => "AUD",
        "752" => "SEK",
        "578" => "NOK",
        "208" => "DKK",
        "985" => "PLN",
        "484" => "MXN",
        otro_ => return otro_.to_uppercase(),
    }
    .to_owned()
}

/// Un extracto Norma 43 o camt, ya leído por los lectores de siempre, como [`Extracto`].
pub(crate) fn desde_n43(a: &N43Account, formato: &str) -> Extracto {
    let mut e = Extracto::nuevo(formato);
    e.cuenta = a.number();
    e.banco = iban::nombre_entidad(&a.bank).unwrap_or_default().to_owned();
    e.titular = a.holder.trim().to_owned();
    e.divisa = divisa_iso(&a.currency);
    e.desde = Some(a.start);
    e.hasta = Some(a.end);
    e.saldo_inicial = Some(a.initial_balance);
    e.saldo_final = Some(a.final_balance);
    e.movimientos = a
        .movements
        .iter()
        .map(|m| Movimiento {
            importe_eur: None,
            fecha: m.op_date,
            fecha_valor: Some(m.value_date),
            concepto: m.description(),
            importe: m.amount,
            saldo: None,
            referencia: [m.document.trim(), m.reference1.trim(), m.reference2.trim()]
                .iter()
                .filter(|s| !s.is_empty())
                .copied()
                .collect::<Vec<_>>()
                .join(" "),
        })
        .collect();
    e
}

/// ¿Parece el texto de un extracto (y no de una factura u otro documento)? Una comprobación
/// rápida, para no pasar por los lectores de extractos cada PDF de la carpeta: alguna palabra
/// de extracto en algún idioma, al menos tres líneas que empiezan por una fecha y al menos tres
/// importes (en muchos PDF el importe va en otra línea que la fecha).
pub fn parece_extracto(texto: &str) -> bool {
    const PALABRAS: [&str; 16] = [
        "extracto",
        "movimientos",
        "saldo",
        "statement",
        "balance",
        "transacciones",
        "transactions",
        "kontoauszug",
        "kontostand",
        "releve",
        "solde",
        "estratto",
        "moviments",
        "historial",
        "listado de movimientos",
        "account activity",
    ];
    let texto: String = texto.chars().take(200_000).collect();
    let t = numeros::sin_tildes(&texto);
    if !PALABRAS.iter().any(|p| t.contains(p)) {
        return false;
    }
    let empieza_por_fecha = |l: &str| {
        let cabeza: String = l.trim().chars().take(12).collect();
        (6..=cabeza.len()).rev().any(|n| {
            cabeza
                .get(..n)
                .is_some_and(|c| numeros::fecha(c, Some(2000), numeros::Orden::DiaMes).is_some())
        })
    };
    let fechas = texto
        .lines()
        .filter(|l| empieza_por_fecha(l))
        .take(3)
        .count();
    let importes: usize = texto.lines().map(|l| numeros::importes_en(l).len()).sum();
    fechas >= 3 && importes >= 3
}

/// Lo que se ha leído, en una línea por extracto: banco, cuenta, divisa, periodo y movimientos.
pub fn resumen(leidos: &[Leido]) -> String {
    leidos
        .iter()
        .map(|l| {
            let e = &l.extracto;
            format!(
                "{} · {} a {} · {} movimientos",
                e.nombre(),
                dia_es(e.desde),
                dia_es(e.hasta),
                e.movimientos.len()
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// Aviso de las filas con importe cuya fecha no se entiende (un 29 de febrero que no existe, un
/// número mal leído por el lector de texto): un movimiento que se pierde sin que se note salvo
/// que los saldos lo delaten.
/// Aviso de las filas con fecha pero sin importe, que no se leen (cabeceras repetidas, líneas
/// de un movimiento partido en dos…).
pub(crate) fn aviso_sin_importe(n: usize) -> String {
    if n == 1 {
        "1 fila con fecha pero sin importe no se ha leído.".to_owned()
    } else {
        format!("{n} filas con fecha pero sin importe no se han leído.")
    }
}

pub(crate) fn aviso_fechas_ilegibles(n: usize) -> String {
    if n == 1 {
        "1 fila con importe tiene una fecha que no se entiende y no se ha leído: comprueba que no \
         falte ese movimiento."
            .to_owned()
    } else {
        format!(
            "{n} filas con importe tienen una fecha que no se entiende y no se han leído: \
             comprueba que no falten esos movimientos."
        )
    }
}

/// Fecha como `dd/mm/aaaa` («?» si no hay).
pub(crate) fn dia_es(d: Option<Date>) -> String {
    d.map(|d| format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year()))
        .unwrap_or_else(|| "?".to_owned())
}

impl Extracto {
    /// Nombre corto de la cuenta para enseñarlo: banco, clase, últimos dígitos y divisa
    /// («BBVA …1332 EUR», «BBVA tarjeta de crédito …1234 EUR»).
    pub fn nombre(&self) -> String {
        let digitos: Vec<char> = self.cuenta.chars().filter(char::is_ascii_digit).collect();
        let ultimos: String = digitos[digitos.len().saturating_sub(4)..].iter().collect();
        let tipo = match self.clase {
            Clase::Cuenta => "",
            Clase::TarjetaCredito => "tarjeta de crédito ",
            Clase::TarjetaDebito => "tarjeta de débito ",
            Clase::Monedero => "monedero ",
        };
        let banco = if self.banco.is_empty() {
            "Cuenta"
        } else {
            &self.banco
        };
        format!("{banco} {tipo}…{ultimos} {}", self.divisa)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn los_avisos_de_filas_distinguen_singular_y_plural() {
        assert_eq!(
            aviso_sin_importe(1),
            "1 fila con fecha pero sin importe no se ha leído."
        );
        assert_eq!(
            aviso_sin_importe(3),
            "3 filas con fecha pero sin importe no se han leído."
        );
        assert!(aviso_fechas_ilegibles(1).starts_with("1 fila con importe tiene"));
        assert!(aviso_fechas_ilegibles(2).starts_with("2 filas con importe tienen"));
    }
}
