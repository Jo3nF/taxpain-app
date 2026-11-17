//! Informe de cuenta de XTB en PDF (xStation 5 → «Historial de la cuenta» → Exportar → PDF).
//!
//! XTB no tiene API desde el 14 de marzo de 2025 (su centro de ayuda: «As of March 14, 2025,
//! the XTB API service via the hosts xapi.xtb.com and ws.xtb.com has been disabled»; consultado
//! el 2026-11-02), así que se lee el PDF. Cada página empieza con la cabecera de la cuenta
//! («Name and surname Account Currency <fecha>», el titular, el número y la divisa, y los saldos)
//! y el informe trae cuatro apartados:
//! - `CLOSED POSITION HISTORY` (con su periodo): Position, Symbol, Type, Volume, Open time, Open
//!   price, Close time, Close price, Open origin, Close origin, Purchase value, Sale value, SL,
//!   TP, Margin, Commission, Swap, Rollover, Gross P/L, Comment.
//! - `OPEN POSITION HISTORY` (a una fecha): Position, Symbol, Type, Volume, Open time, Open
//!   price, Market price, Purchase value, SL, TP, Margin, Commission, Swap, Rollover, Gross P/L.
//! - `PENDING ORDERS HISTORY`: órdenes sin ejecutar (no se usan).
//! - `CASH OPERATION HISTORY` (con su periodo): ID, Type, Time, Comment, Symbol, Amount. Es el
//!   libro completo del efectivo: «Stock purchase» (`OPEN BUY 10/10.5 @ 100.00`: 10 títulos
//!   de una orden de 10,5, al precio), «Stock sale» (`CLOSE BUY …`; el importe es lo que
//!   costó lo vendido) y su «close trade» («Profit of position #…», el beneficio), dividendos,
//!   retenciones, intereses del efectivo y su impuesto, ingresos y retiradas. Los comentarios
//!   largos (un ingreso) ocupan varias líneas y el importe va en la última.
//!
//! Las fechas van como `dd/mm/aaaa hh:mm:ss` y los importes con punto decimal, en la divisa de
//! la cuenta. `TODO(VERIFY)`: con xStation en español los títulos de los apartados y los tipos
//! de operación salen traducidos; se reconocen también por la forma de cada fila, pero los
//! nombres en español no se han podido ver en un informe real.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;
use taxpain_core::cripto::{Precio, Qty};
use taxpain_core::money::Money;
use time::{Date, Month};

use super::importar::{Extracto, OpLeida, PosicionLeida, PrecioLeido, ValorLeido};
use super::{Broker, Clase, TipoOp};

/// ¿Es el informe de cuenta de XTB?
pub fn es_informe(texto: &str) -> bool {
    let t = texto.to_uppercase();
    let apartado = [
        "CLOSED POSITION HISTORY",
        "OPEN POSITION HISTORY",
        "CASH OPERATION HISTORY",
        "HISTORIAL DE POSICIONES CERRADAS",
        "HISTORIAL DE OPERACIONES",
    ]
    .iter()
    .any(|a| t.contains(a));
    let cabecera = (t.contains("BALANCE") && t.contains("EQUITY") && t.contains("FREE MARGIN"))
        || t.contains("XSTATION")
        || (t.contains("MARGEN LIBRE") && t.contains("BALANCE"));
    apartado && cabecera
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Apartado {
    Cerradas,
    Abiertas,
    Pendientes,
    Efectivo,
}

fn apartado(linea: &str) -> Option<Apartado> {
    let t = linea.trim().to_uppercase();
    if t.contains("CLOSED POSITION") || t.contains("POSICIONES CERRADAS") {
        Some(Apartado::Cerradas)
    } else if t.contains("OPEN POSITION") || t.contains("POSICIONES ABIERTAS") {
        Some(Apartado::Abiertas)
    } else if t.contains("PENDING ORDER") || t.contains("ÓRDENES PENDIENTES") {
        Some(Apartado::Pendientes)
    } else if t.contains("CASH OPERATION") || t.contains("OPERACIONES EN EFECTIVO") {
        Some(Apartado::Efectivo)
    } else {
        None
    }
}

/// Línea de la cabecera de página o de las columnas: termina la fila en curso.
fn es_cabecera(linea: &str) -> bool {
    let t = linea.trim();
    let u = t.to_uppercase();
    u.starts_with("NAME AND SURNAME")
        || u.starts_with("NOMBRE Y APELLIDOS")
        || u.starts_with("BALANCE ")
        || u.starts_with("POSITION SYMBOL")
        || u.starts_with("ID TYPE")
        || u.starts_with("ID SYMBOL")
        || u.starts_with("VALUE ")
        || u.starts_with("TOTAL")
        || apartado(t).is_some()
        || (t.len() <= 4 && t.chars().all(|c| c.is_ascii_digit()))
}

/// Importe o precio de XTB: `1250.40`, `-0.07`, `98.7600` (y, por si acaso, coma decimal).
pub(super) fn decimal(s: &str, decimales: usize) -> Option<i128> {
    let t = s.trim().trim_start_matches('+');
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, t),
    };
    let t = if t.contains(',') && t.contains('.') {
        if t.rfind(',') > t.rfind('.') {
            t.replace('.', "").replace(',', ".")
        } else {
            t.replace(',', "")
        }
    } else {
        t.replace(',', ".")
    };
    let (ent, dec) = t.split_once('.').unwrap_or((&t, ""));
    if ent.is_empty() || !ent.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !dec.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let escala = 10i128.checked_pow(u32::try_from(decimales).ok()?)?;
    let mut v = ent.parse::<i128>().ok()?.checked_mul(escala)?;
    let mut d: String = dec.chars().take(decimales).collect();
    while d.len() < decimales {
        d.push('0');
    }
    if !d.is_empty() {
        v += d.parse::<i128>().ok()?;
    }
    // Redondeo de lo que sobra (mitad hacia arriba).
    if dec.len() > decimales && dec.as_bytes()[decimales] >= b'5' {
        v += 1;
    }
    Some(if neg { -v } else { v })
}

pub(super) fn dinero(s: &str) -> Option<Money> {
    decimal(s, 2).and_then(|c| i64::try_from(c).ok().map(Money::from_cents))
}

pub(super) fn precio(s: &str) -> Option<Precio> {
    decimal(s, 8).map(Precio::from_raw)
}

pub(super) fn titulos(s: &str) -> Option<Qty> {
    decimal(s, 18).map(Qty::from_raw)
}

/// Un número con decimales (los importes siempre los llevan; «xStation 5» no).
fn es_importe(s: &str) -> bool {
    let t = s.trim_start_matches('-');
    (t.contains('.') || t.contains(','))
        && t.chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
        && t.chars().next().is_some_and(|c| c.is_ascii_digit())
}

fn fecha(s: &str) -> Option<Date> {
    let p: Vec<&str> = s.split(['/', '.', '-']).collect();
    if p.len() != 3 {
        return None;
    }
    let (d, m, y) = (
        p[0].parse().ok()?,
        p[1].parse::<u8>().ok()?,
        p[2].parse().ok()?,
    );
    Date::from_calendar_date(y, Month::try_from(m).ok()?, d).ok()
}

/// Un símbolo de XTB: `ABCD.DE`, `XYZ.US`, `BRK.B.US`.
fn es_simbolo(s: &str) -> bool {
    let Some((base, mercado)) = s.rsplit_once('.') else {
        return false;
    };
    !base.is_empty()
        && base.len() <= 12
        && (2..=4).contains(&mercado.len())
        && mercado.chars().all(|c| c.is_ascii_uppercase())
        && base
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '&'))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Cerrada {
    posicion: String,
    simbolo: String,
    compra: bool,
    volumen: Qty,
    apertura: (Date, String),
    precio_apertura: Precio,
    cierre: (Date, String),
    precio_cierre: Precio,
    valor_compra: Option<Money>,
    valor_venta: Option<Money>,
    comision: Money,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Abierta {
    simbolo: String,
    compra: bool,
    volumen: Qty,
    precio_mercado: Precio,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Efectivo {
    id: String,
    tipo: String,
    fecha: Date,
    hora: String,
    resto: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Movimiento {
    id: String,
    id_num: i64,
    tipo: String,
    fecha: Date,
    hora: String,
    comentario: String,
    simbolo: Option<String>,
    importe: Money,
}

fn segundos(h: &str) -> i64 {
    let p: Vec<i64> = h.split(':').filter_map(|x| x.parse().ok()).collect();
    match p.as_slice() {
        [a, b, c] => a * 3600 + b * 60 + c,
        _ => 0,
    }
}

fn instante(f: Date, h: &str) -> i64 {
    i64::from(f.to_julian_day()) * 86_400 + segundos(h)
}

struct Patrones {
    cerrada: Regex,
    abierta: Regex,
    efectivo: Regex,
    periodo: Regex,
    a_fecha: Regex,
    orden: Regex,
}

static PATRONES: LazyLock<Patrones> = LazyLock::new(|| {
    const F: &str = r"(\d{2}[/.\-]\d{2}[/.\-]\d{4})\s+(\d{2}:\d{2}:\d{2})";
    const N: &str = r"(-?[\d.,]+)";
    let re = |s: &str| Regex::new(s).expect("patrón fijo válido");
    Patrones {
        cerrada: re(&format!(
            r"^(\d+)\s+(\S+)\s+(BUY|SELL|COMPRA|VENTA)\s+{N}\s+{F}\s+{N}\s+{F}\s+{N}(?:\s+(.*))?$"
        )),
        abierta: re(&format!(
            r"^(\d+)\s+(\S+)\s+(BUY|SELL|COMPRA|VENTA)\s+{N}\s+{F}\s+{N}\s+{N}(?:\s+(.*))?$"
        )),
        efectivo: re(&format!(r"^(\d{{5,}})\s+(.+?)\s+{F}(?:\s+(.*))?$")),
        periodo: re(r"(\d{2}[/.\-]\d{2}[/.\-]\d{4})\s*-\s*(\d{2}[/.\-]\d{2}[/.\-]\d{4})"),
        a_fecha: re(r"^(\d{2}[/.\-]\d{2}[/.\-]\d{4})\s+\d{2}:\d{2}:\d{2}$"),
        orden: re(
            r"(?i)(OPEN|CLOSE)\s+(BUY|SELL)\s+([\d.,]+)(?:\s*/\s*([\d.,]+))?\s*@\s*([\d.,]+)",
        ),
    }
});

/// Lo que se lee del PDF, antes de convertirlo en operaciones.
#[derive(Debug, Default)]
struct Leido {
    cuenta: String,
    divisa: String,
    generado: Option<Date>,
    cerradas: Vec<Cerrada>,
    abiertas: Vec<Abierta>,
    abiertas_a: Option<Date>,
    efectivo: Vec<Efectivo>,
    periodo_efectivo: Option<(Date, Date)>,
    hay_efectivo: bool,
    hay_cerradas: bool,
    hay_abiertas: bool,
}

fn analizar(texto: &str) -> Leido {
    let p = &*PATRONES;
    let mut l = Leido::default();
    let mut actual: Option<Apartado> = None;
    let mut fila: Option<Efectivo> = None;
    let mut tras_titular = false;
    let lineas: Vec<String> = texto
        .lines()
        .map(|x| x.replace('\u{a0}', " ").trim().to_owned())
        .collect();
    let cerrar = |fila: &mut Option<Efectivo>, l: &mut Leido| {
        if let Some(f) = fila.take() {
            l.efectivo.push(f);
        }
    };
    for linea in &lineas {
        if linea.is_empty() {
            continue;
        }
        let u = linea.to_uppercase();
        // Cabecera de la cuenta: la línea siguiente trae el titular, el número y la divisa.
        if (u.contains("ACCOUNT") && u.contains("CURRENCY"))
            || (u.contains("CUENTA") && u.contains("DIVISA"))
        {
            cerrar(&mut fila, &mut l);
            tras_titular = true;
            let t: Vec<&str> = linea.split_whitespace().collect();
            if let [.., d, h] = t.as_slice()
                && h.len() == 8
            {
                l.generado = l.generado.or(fecha(d));
            }
            continue;
        }
        if tras_titular {
            tras_titular = false;
            let t: Vec<&str> = linea.split_whitespace().collect();
            if let [.., n, d] = t.as_slice()
                && d.len() == 3
                && d.chars().all(|c| c.is_ascii_uppercase())
                && n.chars().all(|c| c.is_ascii_digit())
            {
                if l.cuenta.is_empty() {
                    l.cuenta = (*n).to_owned();
                    l.divisa = (*d).to_owned();
                }
                continue;
            }
        }
        if let Some(a) = apartado(linea) {
            cerrar(&mut fila, &mut l);
            actual = Some(a);
            match a {
                Apartado::Cerradas => l.hay_cerradas = true,
                Apartado::Abiertas => l.hay_abiertas = true,
                Apartado::Efectivo => l.hay_efectivo = true,
                Apartado::Pendientes => {}
            }
            continue;
        }
        if let Some(c) = p.periodo.captures(linea)
            && linea.len() < 40
        {
            if actual == Some(Apartado::Efectivo)
                && let (Some(a), Some(b)) = (fecha(&c[1]), fecha(&c[2]))
            {
                l.periodo_efectivo = Some((a, b));
            }
            continue;
        }
        if let Some(c) = p.a_fecha.captures(linea) {
            if actual == Some(Apartado::Abiertas) {
                l.abiertas_a = fecha(&c[1]);
            }
            continue;
        }
        if actual == Some(Apartado::Pendientes) {
            continue;
        }
        if matches!(actual, None | Some(Apartado::Cerradas))
            && let Some(c) = p.cerrada.captures(linea)
            && let Some(x) = cerrada(&c)
        {
            cerrar(&mut fila, &mut l);
            l.cerradas.push(x);
            continue;
        }
        if matches!(actual, None | Some(Apartado::Abiertas))
            && let Some(c) = p.abierta.captures(linea)
            && let Some(x) = abierta(&c)
        {
            cerrar(&mut fila, &mut l);
            l.abiertas.push(x);
            continue;
        }
        if matches!(actual, None | Some(Apartado::Efectivo))
            && let Some(c) = p.efectivo.captures(linea)
            && let (Some(f), Some(h)) = (fecha(&c[3]), c.get(4))
        {
            cerrar(&mut fila, &mut l);
            fila = Some(Efectivo {
                id: c[1].to_owned(),
                tipo: c[2].trim().to_owned(),
                fecha: f,
                hora: h.as_str().to_owned(),
                resto: c.get(5).map(|m| m.as_str().to_owned()).unwrap_or_default(),
            });
            continue;
        }
        if es_cabecera(linea) {
            cerrar(&mut fila, &mut l);
            continue;
        }
        // Continuación del comentario (y el importe) de la fila de efectivo en curso.
        if let Some(f) = fila.as_mut() {
            f.resto.push(' ');
            f.resto.push_str(linea);
        }
    }
    cerrar(&mut fila, &mut l);
    l
}

fn cerrada(c: &regex::Captures<'_>) -> Option<Cerrada> {
    let resto = c.get(11).map(|m| m.as_str()).unwrap_or_default();
    let nums: Vec<Money> = resto
        .split_whitespace()
        .filter(|t| es_importe(t))
        .filter_map(dinero)
        .collect();
    // Purchase value, Sale value, …, Commission, Swap, Rollover, Gross P/L.
    let comision = if nums.len() >= 6 {
        nums[nums.len() - 4].abs()
    } else {
        Money::ZERO
    };
    Some(Cerrada {
        posicion: c[1].to_owned(),
        simbolo: c[2].to_uppercase(),
        compra: matches!(&c[3], "BUY" | "COMPRA"),
        volumen: titulos(&c[4])?,
        apertura: (fecha(&c[5])?, c[6].to_owned()),
        precio_apertura: precio(&c[7])?,
        cierre: (fecha(&c[8])?, c[9].to_owned()),
        precio_cierre: precio(&c[10])?,
        valor_compra: nums.first().copied(),
        valor_venta: nums.get(1).copied(),
        comision,
    })
}

fn abierta(c: &regex::Captures<'_>) -> Option<Abierta> {
    Some(Abierta {
        simbolo: c[2].to_uppercase(),
        compra: matches!(&c[3], "BUY" | "COMPRA"),
        volumen: titulos(&c[4])?,
        precio_mercado: precio(&c[8])?,
    })
}

/// Separa el comentario, el símbolo y el importe del final de una fila de efectivo.
fn movimiento(e: &Efectivo) -> Option<Movimiento> {
    let tokens: Vec<&str> = e.resto.split_whitespace().collect();
    let pos = tokens.iter().rposition(|t| es_importe(t))?;
    let importe = dinero(tokens[pos])?;
    let (simbolo, fin) = match pos.checked_sub(1).map(|i| tokens[i]) {
        Some(s) if es_simbolo(s) => (Some(s.to_owned()), pos - 1),
        _ => (None, pos),
    };
    Some(Movimiento {
        id: e.id.clone(),
        id_num: e.id.parse().unwrap_or(0),
        tipo: e.tipo.clone(),
        fecha: e.fecha,
        hora: e.hora.clone(),
        comentario: tokens[..fin].join(" "),
        simbolo,
        importe,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Clasificado {
    Compra,
    Venta,
    Cierre,
    Op(TipoOp),
    /// Intereses del efectivo: su impuesto.
    ImpuestoIntereses,
    Cfd,
    Ignorar,
    Desconocido,
}

fn clasificar(tipo: &str) -> Clasificado {
    let t = tipo.to_lowercase();
    let tiene = |xs: &[&str]| xs.iter().any(|x| t.contains(x));
    if tiene(&["cfd", "swap", "rollover"]) {
        Clasificado::Cfd
    } else if tiene(&["purchase", "compra"]) {
        Clasificado::Compra
    } else if tiene(&["close trade", "cierre"]) {
        Clasificado::Cierre
    } else if tiene(&["sale", "sell", "venta"]) {
        Clasificado::Venta
    } else if tiene(&["interest", "interés", "interes"]) && tiene(&["tax", "impuesto"]) {
        Clasificado::ImpuestoIntereses
    } else if tiene(&["withholding", "retenc"]) {
        Clasificado::Op(TipoOp::Retencion)
    } else if tiene(&["divid"]) {
        Clasificado::Op(TipoOp::Dividendo)
    } else if tiene(&["interest", "interés", "interes"]) {
        Clasificado::Op(TipoOp::Interes)
    } else if tiene(&["deposit", "depósito", "ingreso"]) {
        Clasificado::Op(TipoOp::Deposito)
    } else if tiene(&["withdraw", "retirada", "retiro"]) {
        Clasificado::Op(TipoOp::Retirada)
    } else if tiene(&["commission", "comisi", "fee"]) {
        Clasificado::Op(TipoOp::Comision)
    } else if tiene(&["transfer"]) {
        Clasificado::Ignorar
    } else {
        Clasificado::Desconocido
    }
}

/// Divisa en la que cotiza un valor por el sufijo del símbolo de XTB (el precio de mercado de
/// las posiciones abiertas va en ella, no en la de la cuenta). `None` si no se sabe (Londres
/// cotiza en libras o en peniques según el valor).
pub(super) fn divisa_mercado(simbolo: &str) -> Option<&'static str> {
    let m = simbolo.rsplit('.').next().unwrap_or("");
    Some(match m {
        "US" => "USD",
        "DE" | "FR" | "NL" | "ES" | "IT" | "PT" | "BE" | "FI" | "IE" | "AT" => "EUR",
        "CH" => "CHF",
        "PL" => "PLN",
        "DK" => "DKK",
        "SE" => "SEK",
        "NO" => "NOK",
        "CZ" => "CZK",
        _ => return None,
    })
}

/// País del mercado por el sufijo del símbolo de XTB (sólo como pista para la retención).
pub(super) fn pais_mercado(simbolo: &str) -> String {
    let m = simbolo.rsplit('.').next().unwrap_or("");
    match m {
        "US" => "US",
        "UK" => "GB",
        "DE" => "DE",
        "FR" => "FR",
        "NL" => "NL",
        "ES" => "ES",
        "IT" => "IT",
        "PT" => "PT",
        "BE" => "BE",
        "CH" => "CH",
        "PL" => "PL",
        "DK" => "DK",
        "SE" => "SE",
        "NO" => "NO",
        "FI" => "FI",
        "IE" => "IE",
        "AT" => "AT",
        _ => "",
    }
    .to_owned()
}

/// Lee el texto del informe de XTB.
pub fn leer(texto: &str) -> Result<Extracto, String> {
    let l = analizar(texto);
    if l.cuenta.is_empty() {
        return Err(
            "no se encuentra el número de cuenta de XTB en la cabecera del informe".to_owned(),
        );
    }
    let divisa = if l.divisa.is_empty() {
        "EUR".to_owned()
    } else {
        l.divisa.clone()
    };
    let mut e = Extracto::nuevo(Broker::Xtb, &l.cuenta, &divisa);
    let cuenta = l.cuenta.clone();
    let mut simbolos: BTreeMap<String, ()> = BTreeMap::new();
    let p = &*PATRONES;
    let mut cfd = 0usize;
    let mut desconocidos: BTreeMap<String, usize> = BTreeMap::new();

    if l.hay_efectivo || !l.efectivo.is_empty() {
        let movs: Vec<Movimiento> = l.efectivo.iter().filter_map(movimiento).collect();
        if movs.len() < l.efectivo.len() {
            e.avisos.push(format!(
                "{} filas de «Cash operations» sin importe legible: no se importan",
                l.efectivo.len() - movs.len()
            ));
        }
        let mut cierres: Vec<(usize, &Movimiento)> = movs
            .iter()
            .enumerate()
            .filter(|(_, m)| clasificar(&m.tipo) == Clasificado::Cierre)
            .collect();
        let mut usados: Vec<bool> = vec![false; movs.len()];
        for m in &movs {
            let base = OpLeida {
                fecha: m.fecha,
                hora: m.hora.clone(),
                tipo: TipoOp::Comision,
                valor: m.simbolo.clone(),
                cantidad: Qty::ZERO,
                precio: None,
                divisa: divisa.clone(),
                importe: m.importe,
                comision: Money::ZERO,
                cambio: None,
                euros: m.importe,
                comision_eur: Money::ZERO,
                pais: String::new(),
                nota: m.comentario.clone(),
                fuente: "xtb".into(),
                huella: format!("xtb:{cuenta}:{}", m.id),
                comision_huella: Vec::new(),
            };
            match clasificar(&m.tipo) {
                Clasificado::Compra | Clasificado::Venta => {
                    let compra = clasificar(&m.tipo) == Clasificado::Compra;
                    let (Some(c), Some(s)) = (p.orden.captures(&m.comentario), &m.simbolo) else {
                        e.avisos.push(format!(
                            "«{}» del {} ({}): no se entiende el comentario «{}»; no se importa",
                            m.tipo, m.fecha, m.id, m.comentario
                        ));
                        continue;
                    };
                    if c[2].eq_ignore_ascii_case("SELL") {
                        cfd += 1;
                        continue;
                    }
                    let (Some(q), Some(pr)) = (titulos(&c[3]), precio(&c[5])) else {
                        continue;
                    };
                    let mut importe = m.importe.abs();
                    let mut nota = m.comentario.clone();
                    if !compra {
                        // Lo cobrado = lo que costó (esta fila) + el beneficio de su «close
                        // trade»: el del mismo símbolo y casi la misma hora cuyo importe cuadra
                        // con títulos × precio o, si no, el de ID anterior, o el más cercano.
                        let t0 = instante(m.fecha, &m.hora);
                        let bruto = q.valor(pr);
                        let candidatos: Vec<(usize, usize, &Movimiento)> = cierres
                            .iter()
                            .enumerate()
                            .filter(|(_, (i, x))| {
                                !usados[*i]
                                    && x.simbolo.as_deref() == Some(s.as_str())
                                    && (instante(x.fecha, &x.hora) - t0).abs() <= 10
                            })
                            .map(|(k, (i, x))| (k, *i, *x))
                            .collect();
                        let elegido = candidatos
                            .iter()
                            .find(|(_, _, x)| Some(importe + x.importe) == bruto)
                            .or_else(|| {
                                candidatos.iter().find(|(_, _, x)| x.id_num + 1 == m.id_num)
                            })
                            .or_else(|| {
                                candidatos
                                    .iter()
                                    .min_by_key(|(_, _, x)| (instante(x.fecha, &x.hora) - t0).abs())
                            })
                            .copied();
                        if let Some((k, i, x)) = elegido {
                            usados[i] = true;
                            importe += x.importe;
                            nota = format!("{nota} · {}", x.comentario);
                            cierres.remove(k);
                        }
                    }
                    simbolos.insert(s.clone(), ());
                    e.ops.push(OpLeida {
                        tipo: if compra {
                            TipoOp::Compra
                        } else {
                            TipoOp::Venta
                        },
                        cantidad: q,
                        precio: Some(pr),
                        importe,
                        euros: importe,
                        nota,
                        ..base
                    });
                }
                Clasificado::Cierre => {}
                // Una fila a cero (impuesto sobre unos céntimos de intereses) no es operación.
                Clasificado::ImpuestoIntereses | Clasificado::Op(_) if m.importe.is_zero() => {}
                Clasificado::ImpuestoIntereses => e.ops.push(OpLeida {
                    tipo: TipoOp::Retencion,
                    importe: -m.importe,
                    euros: -m.importe,
                    // TODO(VERIFY): XTB opera en España como sucursal y retiene sobre los
                    // intereses del efectivo; se trata como retención española (473, se resta
                    // en el 200). Compruébalo con el certificado de retenciones de XTB.
                    pais: "ES".into(),
                    ..base
                }),
                Clasificado::Op(t) => {
                    let pais = m.simbolo.as_deref().map(pais_mercado).unwrap_or_default();
                    let importe = match t {
                        // En el PDF las retenciones y comisiones salen en negativo.
                        TipoOp::Retencion | TipoOp::Comision | TipoOp::Impuesto => -m.importe,
                        TipoOp::Deposito | TipoOp::Retirada => m.importe.abs(),
                        _ => m.importe,
                    };
                    if let Some(s) = &m.simbolo {
                        simbolos.insert(s.clone(), ());
                    }
                    e.ops.push(OpLeida {
                        tipo: t,
                        importe,
                        euros: importe,
                        pais,
                        ..base
                    });
                }
                Clasificado::Cfd => cfd += 1,
                Clasificado::Ignorar => {}
                Clasificado::Desconocido => *desconocidos.entry(m.tipo.clone()).or_default() += 1,
            }
        }
        for (_, x) in cierres {
            if x.simbolo
                .as_deref()
                .is_some_and(|s| simbolos.contains_key(s))
            {
                e.avisos.push(format!(
                    "«{}» del {} ({}) sin su venta: su beneficio no se ha sumado",
                    x.comentario, x.fecha, x.id
                ));
            } else {
                cfd += 1;
            }
        }
        e.efectivo = l.periodo_efectivo.or_else(|| {
            let (a, b) = e.fechas();
            a.zip(b)
        });
        // Las posiciones cerradas sirven para cuadrar las ventas.
        for c in l.cerradas.iter().filter(|c| es_simbolo(&c.simbolo)) {
            let ok = e.ops.iter().any(|o| {
                o.tipo == TipoOp::Venta
                    && o.valor.as_deref() == Some(c.simbolo.as_str())
                    && o.fecha == c.cierre.0
                    && o.cantidad == c.volumen
                    && o.precio == Some(c.precio_cierre)
                    && c.valor_venta.is_none_or(|v| v == o.importe)
            });
            if !ok
                && e.efectivo
                    .is_some_and(|(a, b)| c.cierre.0 >= a && c.cierre.0 <= b)
            {
                e.avisos.push(format!(
                    "La posición cerrada {} de {} ({} títulos el {}) no cuadra con las \
                     operaciones en efectivo",
                    c.posicion, c.simbolo, c.volumen, c.cierre.0
                ));
            }
        }
    } else if !l.cerradas.is_empty() {
        // Sin operaciones en efectivo: compras y ventas de las posiciones cerradas.
        for c in &l.cerradas {
            // Las cortas y los CFD (US500, EURUSD, AAPL.US_9…) no son valores.
            if !c.compra || !es_simbolo(&c.simbolo) {
                cfd += 1;
                continue;
            }
            simbolos.insert(c.simbolo.clone(), ());
            // Con el precio de cierre: dos lotes de la misma posición cerrados en el mismo
            // segundo con los mismos títulos son dos ventas.
            let huella = format!(
                "xtb:{cuenta}:pos:{}:{}T{}:{}:{}",
                c.posicion,
                c.cierre.0,
                c.cierre.1,
                c.volumen.to_text(),
                c.precio_cierre.to_text()
            );
            let compra = c
                .valor_compra
                .or_else(|| c.volumen.valor(c.precio_apertura))
                .unwrap_or_default();
            let venta = c
                .valor_venta
                .or_else(|| c.volumen.valor(c.precio_cierre))
                .unwrap_or_default();
            let comun = OpLeida {
                fecha: c.apertura.0,
                hora: c.apertura.1.clone(),
                tipo: TipoOp::Compra,
                valor: Some(c.simbolo.clone()),
                cantidad: c.volumen,
                precio: Some(c.precio_apertura),
                divisa: divisa.clone(),
                importe: compra,
                comision: Money::ZERO,
                cambio: None,
                euros: compra,
                comision_eur: Money::ZERO,
                pais: String::new(),
                nota: format!("posición {}", c.posicion),
                fuente: "xtb-posiciones".into(),
                huella: format!("{huella}:c"),
                comision_huella: Vec::new(),
            };
            e.ops.push(OpLeida {
                fecha: c.cierre.0,
                hora: c.cierre.1.clone(),
                tipo: TipoOp::Venta,
                precio: Some(c.precio_cierre),
                importe: venta,
                euros: venta,
                comision: c.comision,
                comision_eur: c.comision,
                huella: format!("{huella}:v"),
                ..comun.clone()
            });
            e.ops.push(comun);
        }
        e.avisos.push(
            "Este PDF no trae las operaciones en efectivo («Cash operations»): las compras y \
             ventas salen de las posiciones cerradas, y no entran las compras de lo que sigue \
             abierto ni los dividendos e intereses. Importa también el informe con las \
             operaciones en efectivo (lo que ya esté no se duplica)."
                .into(),
        );
    }

    // Posiciones abiertas: el precio de mercado y los títulos a la fecha del informe.
    let a_fecha = l.abiertas_a.or(l.generado);
    if let Some(f) = a_fecha {
        let mut vistos: BTreeMap<String, ()> = BTreeMap::new();
        for a in &l.abiertas {
            if !a.compra || !es_simbolo(&a.simbolo) {
                cfd += 1;
                continue;
            }
            simbolos.insert(a.simbolo.clone(), ());
            e.posiciones.push(PosicionLeida {
                fecha: f,
                valor: a.simbolo.clone(),
                cantidad: a.volumen,
            });
            // Un precio a cero (cotización suspendida) no vale: se pone a mano.
            if a.precio_mercado > Precio::ZERO && vistos.insert(a.simbolo.clone(), ()).is_none() {
                e.precios.push(PrecioLeido {
                    fecha: f,
                    valor: a.simbolo.clone(),
                    precio: a.precio_mercado,
                    divisa: divisa_mercado(&a.simbolo).unwrap_or(&divisa).to_owned(),
                });
            }
        }
        // Con posiciones leídas, un valor sin posición abierta tiene 0 títulos a esa fecha.
        if l.hay_abiertas {
            for s in simbolos.keys() {
                if !e.posiciones.iter().any(|p| &p.valor == s) {
                    e.posiciones.push(PosicionLeida {
                        fecha: f,
                        valor: s.clone(),
                        cantidad: Qty::ZERO,
                    });
                }
            }
        }
    }
    e.desde = e
        .efectivo
        .map(|(a, _)| a)
        .or_else(|| e.ops.iter().map(|o| o.fecha).min());
    e.hasta = e
        .efectivo
        .map(|(_, b)| b)
        .or_else(|| e.ops.iter().map(|o| o.fecha).max());
    for s in simbolos.keys() {
        e.valores.push(ValorLeido {
            clave: s.clone(),
            isin: String::new(),
            ticker: s.clone(),
            nombre: String::new(),
            divisa: divisa_mercado(s).unwrap_or(&divisa).to_owned(),
            clase: Clase::Accion,
            pais: String::new(),
        });
    }
    // Un valor que no cotiza en la divisa de la cuenta: el importe no es títulos × precio.
    for o in &e.ops {
        if o.tipo == TipoOp::Compra
            && let (Some(pr), Some(s)) = (o.precio, &o.valor)
            && divisa_mercado(s).is_none_or(|d| d == divisa)
            && let Some(bruto) = o.cantidad.valor(pr)
            && (bruto - o.importe).abs() > Money::from_cents(bruto.cents().abs() / 50 + 2)
        {
            e.avisos.push(format!(
                "{s}: el importe en {divisa} no es títulos × precio; puede que cotice en otra \
                 divisa: revisa su precio de cierre"
            ));
            break;
        }
    }
    // En una cuenta en otra divisa los importes no son euros: se pasan al importar con el tipo
    // del BCE del día (si ya está descargado; si no, quedan para pasarlos a mano).
    if divisa != "EUR" {
        for o in &mut e.ops {
            o.euros = Money::ZERO;
            o.comision_eur = Money::ZERO;
        }
        e.avisos.push(format!(
            "La cuenta de XTB va en {divisa}: los importes se pasan a euros con el tipo de \
             referencia del BCE del día de cada operación."
        ));
    }
    if cfd > 0 {
        e.avisos.push(format!(
            "{cfd} filas de CFD o posiciones cortas no se importan (son derivados, no valores)"
        ));
    }
    for (t, n) in desconocidos {
        e.avisos.push(format!(
            "{n} operaciones en efectivo de tipo «{t}» no se reconocen y no se importan"
        ));
    }
    if e.ops.is_empty() && e.precios.is_empty() {
        return Err("el informe de XTB no trae operaciones ni posiciones que importar".into());
    }
    Ok(e)
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use time::macros::date;

    /// Informe sintético con la forma del de xStation 5 (datos inventados): dos páginas, una
    /// venta en dos partes a la misma hora, compras con orden fraccionada, un ingreso con el
    /// comentario en varias líneas y una posición abierta.
    pub(crate) const INFORME: &str = "\
Name and surname Account Currency 15/01/2026 10:00:00

EMPRESA DEMO SL 12345678 EUR

Balance Equity Margin Free margin Margin level

80.00 10750.00 0.00 80.00 0.00

CLOSED POSITION HISTORY

01/01/2025 - 15/01/2026

Balance :80.00 - 0.00

Position Symbol Type Volume Open time Open price Close time Close price Open origin Close origin Purchase
value Sale value SL TP Margin Commission Swap Rollover Gross P/L Comment

5000000002 ACME.DE BUY 6.0000 10/03/2025 10:00:01 100.0000 20/06/2025 15:30:00 110.0000 xStation 5 xStation 5 600.00 660.00 0.00 0.00 0.00 60.00

5000000002 ACME.DE BUY 4.0000 10/03/2025 10:00:01 100.0000 20/06/2025 15:30:00 112.0000 xStation 5 xStation 5 400.00 448.00 0.00 0.00 0.00 48.00

5000000001 ACME.DE BUY 0.5000 10/03/2025 10:00:00 100.2000 20/06/2025 15:29:59 110.0000 xStation 5 xStation 5 50.10 55.00 0.00 0.00 0.00 4.90

Total 0.00 0.00 0.00 112.90

1

Name and surname Account Currency 15/01/2026 10:00:00

EMPRESA DEMO SL 12345678 EUR

Balance Equity Margin Free margin Margin level

80.00 10750.00 0.00 80.00 0.00

OPEN POSITION HISTORY

15/01/2026 00:00:00

Position Symbol Type Volume Open time Open price Market price Purchase
value SL TP Margin Commission Swap Rollover Gross P/L Comment

5000000009 MMKT.DE BUY 100.0000 01/07/2025 11:00:00 105.0000 106.5000 10500.00 0.00 0.00 0.00 0.00 150.00

Total 0.00 0.00 0.00 150.00

2

Name and surname Account Currency 15/01/2026 10:00:00

EMPRESA DEMO SL 12345678 EUR

Balance Equity Margin Free margin Margin level

80.00 10750.00 0.00 80.00 0.00

CASH OPERATION HISTORY

01/01/2025 - 15/01/2026

ID Type Time Comment Symbol Amount

700000001 deposit 01/03/2025 09:00:00 BANK deposit, provider
transaction id=TXN-0000-1111,
merchant reference id=TXN-0000-
1111, id=42
 11000.00

700000003 Stock purchase 10/03/2025 10:00:01 OPEN BUY 10/10.5 @ 100.00 ACME.DE -1000.00

700000002 Stock purchase 10/03/2025 10:00:00 OPEN BUY 0.5/10.5 @ 100.20 ACME.DE -50.10

700000010 close trade 20/06/2025 15:30:00 Profit of position #5000000002 ACME.DE 48.00

700000011 Stock sale 20/06/2025 15:30:00 CLOSE BUY 4/10.5 @ 112.00 ACME.DE 400.00

700000012 close trade 20/06/2025 15:30:00 Profit of position #5000000002 ACME.DE 60.00

700000013 Stock sale 20/06/2025 15:30:00 CLOSE BUY 6/10.5 @ 110.00 ACME.DE 600.00

700000014 close trade 20/06/2025 15:29:59 Profit of position #5000000001 ACME.DE 4.90

700000015 Stock sale 20/06/2025 15:29:59 CLOSE BUY 0.5/10.5 @ 110.00 ACME.DE 50.10

700000020 Stock purchase 01/07/2025 11:00:00 OPEN BUY 100 @ 105.00 MMKT.DE -10500.00

700000030 DIVIDENT 15/09/2025 12:00:00 ACME.DE EUR 0.5000/ SHR ACME.DE 5.00

700000031 Withholding tax 15/09/2025 12:00:00 ACME.DE EUR WHT 26.375% ACME.DE -1.32

700000040 Free-funds Interest Tax 03/10/2025 13:00:00 Free-funds Interest Tax 2025-09 -0.05

700000041 Free-funds Interest 03/10/2025 13:10:00 Free-funds Interest 2025-09 0.25

Total 80.00 EUR";

    #[test]
    fn reconoce_y_lee_el_informe_completo() {
        assert!(es_informe(INFORME));
        assert!(!es_informe("Factura 2025-001 ACME SL"));
        let e = leer(INFORME).unwrap();
        assert_eq!(e.cuenta, "12345678");
        assert_eq!(e.divisa, "EUR");
        assert_eq!(
            e.efectivo,
            Some((date!(2025 - 01 - 01), date!(2026 - 01 - 15)))
        );
        let n = |t: TipoOp| e.ops.iter().filter(|o| o.tipo == t).count();
        assert_eq!(n(TipoOp::Compra), 3);
        assert_eq!(n(TipoOp::Venta), 3);
        assert_eq!(n(TipoOp::Deposito), 1);
        assert_eq!(n(TipoOp::Dividendo), 1);
        assert_eq!(n(TipoOp::Retencion), 2);
        assert_eq!(n(TipoOp::Interes), 1);
        // El ingreso con el comentario en cuatro líneas y el importe en la quinta.
        let dep = e.ops.iter().find(|o| o.tipo == TipoOp::Deposito).unwrap();
        assert_eq!(dep.euros, Money::from_euros(11_000, 0));
        assert!(dep.nota.contains("id=42"), "{}", dep.nota);
        // La venta de 4 a 112 cobra 400 + 48 (no 400 + 60, aunque sean de la misma hora).
        let v4 = e
            .ops
            .iter()
            .find(|o| o.tipo == TipoOp::Venta && o.cantidad == Qty::from_units(4))
            .unwrap();
        assert_eq!(v4.euros, Money::from_cents(44_800));
        let v6 = e
            .ops
            .iter()
            .find(|o| o.tipo == TipoOp::Venta && o.cantidad == Qty::from_units(6))
            .unwrap();
        assert_eq!(v6.euros, Money::from_cents(66_000));
        // La compra fraccionada: 0,5 títulos a 100,20.
        let c = e
            .ops
            .iter()
            .find(|o| o.tipo == TipoOp::Compra && o.precio == Precio::parse("100.2"))
            .unwrap();
        assert_eq!(c.cantidad, Qty::parse("0.5").unwrap());
        assert_eq!(c.huella, "xtb:12345678:700000002");
        // Retención del dividendo (positiva, país del mercado) y la de los intereses.
        let r = e
            .ops
            .iter()
            .filter(|o| o.tipo == TipoOp::Retencion)
            .map(|o| (o.euros, o.pais.as_str()))
            .collect::<Vec<_>>();
        assert!(r.contains(&(Money::from_cents(132), "DE")), "{r:?}");
        assert!(r.contains(&(Money::from_cents(5), "ES")), "{r:?}");
        // Precio de mercado y títulos de la posición abierta.
        assert_eq!(e.precios.len(), 1);
        assert_eq!(e.precios[0].precio, Precio::parse("106.5").unwrap());
        assert_eq!(e.precios[0].fecha, date!(2026 - 01 - 15));
        assert!(
            e.posiciones
                .iter()
                .any(|p| p.valor == "ACME.DE" && p.cantidad.is_zero())
        );
        assert!(e.avisos.is_empty(), "{:?}", e.avisos);
    }

    #[test]
    fn sin_operaciones_en_efectivo_lee_las_posiciones_cerradas() {
        let solo: String = INFORME
            .split("CASH OPERATION HISTORY")
            .next()
            .unwrap()
            .to_owned();
        let e = leer(&solo).unwrap();
        assert_eq!(e.efectivo, None);
        let compras: Vec<&OpLeida> = e.ops.iter().filter(|o| o.tipo == TipoOp::Compra).collect();
        assert_eq!(compras.len(), 3);
        assert!(compras.iter().all(|o| o.fuente == "xtb-posiciones"));
        let venta = e
            .ops
            .iter()
            .find(|o| o.tipo == TipoOp::Venta && o.cantidad == Qty::from_units(4))
            .unwrap();
        assert_eq!(venta.euros, Money::from_cents(44_800));
        assert!(
            e.avisos
                .iter()
                .any(|a| a.contains("no trae las operaciones"))
        );
    }

    /// Una acción de EE. UU. en una cuenta en euros: el precio de mercado está en dólares.
    /// Un CFD (US500) no es un valor. Un precio de mercado a cero (cotización suspendida) no
    /// para la importación.
    #[test]
    fn divisa_de_cotizacion_cfd_y_precio_cero() {
        let texto = INFORME.replace(
            "5000000009 MMKT.DE BUY",
            "5000000060 XYZ.US BUY 10.0000 01/07/2025 11:00:00 150.0000 160.0000 1380.00 0.00 0.00 0.00 0.00 90.00\n\n\
             5000000050 US500 BUY 0.0100 01/07/2025 11:00:00 5500.00 5600.00 550.00 0.00 0.00 0.00 0.00 1.00\n\n\
             5000000070 SUSP.DE BUY 5.0000 01/07/2025 11:00:00 10.0000 0.0000 50.00 0.00 0.00 0.00 0.00 -50.00\n\n\
             5000000009 MMKT.DE BUY",
        );
        let e = leer(&texto).unwrap();
        let xyz = e.precios.iter().find(|p| p.valor == "XYZ.US").unwrap();
        assert_eq!(xyz.divisa, "USD");
        let v = e.valores.iter().find(|v| v.clave == "XYZ.US").unwrap();
        assert_eq!(v.divisa, "USD");
        assert!(!e.valores.iter().any(|v| v.clave == "US500"));
        assert!(!e.posiciones.iter().any(|p| p.valor == "US500"));
        assert!(!e.precios.iter().any(|p| p.valor == "SUSP.DE"));
    }

    /// Una cuenta en dólares: los importes no son euros (se pasan al importar con el BCE).
    #[test]
    fn cuenta_en_dolares_sin_euros_inventados() {
        let texto = INFORME.replace(
            "EMPRESA DEMO SL 12345678 EUR",
            "EMPRESA DEMO SL 12345678 USD",
        );
        let e = leer(&texto).unwrap();
        assert_eq!(e.divisa, "USD");
        let dep = e.ops.iter().find(|o| o.tipo == TipoOp::Deposito).unwrap();
        assert_eq!(dep.importe, Money::from_euros(11_000, 0));
        assert!(dep.euros.is_zero(), "{:?}", dep.euros);
    }

    /// Una fila a cero (el impuesto sobre 0,01 € de intereses) no se convierte en operación.
    #[test]
    fn fila_a_cero_no_es_operacion() {
        let texto = INFORME.replace(
            "Total 80.00 EUR",
            "700000042 Free-funds Interest Tax 03/11/2025 13:00:00 Free-funds Interest Tax 2025-10 0.00\n\nTotal 80.00 EUR",
        );
        let e = leer(&texto).unwrap();
        assert!(e.ops.iter().all(|o| !o.importe.is_zero()), "{:?}", e.ops);
    }

    /// Dos lotes de la misma posición cerrados en el mismo segundo con los mismos títulos (a
    /// precios distintos) son dos ventas, no una repetida.
    #[test]
    fn lotes_iguales_cerrados_a_la_vez_no_se_confunden() {
        let solo: String = INFORME
            .split("CASH OPERATION HISTORY")
            .next()
            .unwrap()
            .replace("BUY 6.0000 10/03/2025", "BUY 5.0000 10/03/2025")
            .replace("BUY 4.0000 10/03/2025", "BUY 5.0000 10/03/2025");
        let e = leer(&solo).unwrap();
        let mut huellas: Vec<&str> = e.ops.iter().map(|o| o.huella.as_str()).collect();
        let n = huellas.len();
        huellas.sort_unstable();
        huellas.dedup();
        assert_eq!(huellas.len(), n);
    }

    #[test]
    fn numeros_y_simbolos() {
        assert_eq!(dinero("-12345.67"), Some(Money::from_cents(-1_234_567)));
        assert_eq!(dinero("1.234,56"), Some(Money::from_cents(123_456)));
        assert_eq!(precio("98.7600"), Precio::parse("98.76"));
        assert_eq!(titulos("0.4321"), Qty::parse("0.4321"));
        assert!(es_simbolo("ACME.DE") && es_simbolo("BRK.B.US") && !es_simbolo("xStation"));
        assert!(!es_importe("5") && es_importe("-0.11") && !es_importe("2025-09"));
    }
}
