//! Revolut Business (Revolut Bank UAB, sucursal en España): el «Extracto de cuenta» o
//! «Extracto de transacción» en PDF, en sus tres maquetas:
//! - la de 2024, con una sección por cada subcuenta en una divisa (euros, libras, dólares…),
//!   todas con el mismo IBAN, y sin saldo de apertura;
//! - la de 2025-2026, con un «Resumen del saldo» (apertura, entradas, salidas y cierre) y, al
//!   final, unos «Tipos de transacción» que no hay que leer como movimientos;
//! - la búsqueda con filtros (2026), sin columna de saldo ni IBAN, con el estado y la cuenta
//!   pegados a la descripción.
//!
//! El lector de PDF junta las columnas «Dinero saliente» y «Dinero entrante»: cada movimiento
//! trae su importe sin signo y el saldo (`€12.00 €1 234.56`, con el espacio como separador de
//! miles). El signo sale de la diferencia con el saldo anterior, recorriendo los movimientos del
//! más antiguo al más nuevo (el banco los da al revés); en la búsqueda, que no tiene saldo, sale
//! del tipo de la operación (CAR pago con tarjeta, FEE comisión, MOS dinero enviado, MOR dinero
//! recibido, MOA dinero añadido…). Los pagos con tarjeta en otra divisa añaden una línea con el
//! tipo de cambio (la comisión del cambio ya va en el importe) y el importe en la otra divisa.
//! El pie legal de cada página sale al principio de la página y el título, detrás de los
//! movimientos de la primera.
//!
//! También se aceptan las etiquetas del extracto en inglés («Account name», «Opening balance»,
//! «Date (UTC)»…), aunque no se han visto en extractos reales.

use std::sync::LazyLock;

use regex::Regex;
use taxpain_core::money::Money;
use time::Date;

use super::comun::{cantidad, fecha, importe, limpiar, sin_punto, unir};
use crate::extractos::{Extracto, Movimiento, iban, numeros};

static PIE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"^©.*Revolut.*\d+/\d+$").ok());
static TITULO: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"^(?:Extracto de (?:cuenta|transacción)|(?:Account|Transaction) statement)$").ok()
});
static GENERADO: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"^(?:(?:Documento generado|Generado) el|Generated on(?: the)?) (.+)$").ok()
});
static CUENTAS: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^\*\s*Cuentas: ([^·,]+?) · ([A-Z]{3})\b").ok());
static NOMBRE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^(?:Nombre de la cuenta|Account name) (.+)$").ok());
static DIVISA: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^(?:Divisa|Currency) ([A-Z]{3})$").ok());
static IBAN: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"^IBAN (.+)$").ok());
static PERIODO: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"^(?:Transacciones de|Transactions from) (.+?) (?:a|to) (.+)$").ok()
});
static RESUMEN: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"^(Saldo de apertura|Opening balance|Dinero entrante|Money in|Dinero saliente|Money out|Saldo de cierre|Closing balance) (.+)$",
    )
    .ok()
});
static FILA: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^(\d{1,2}) (\p{L}+)\.? (\d{4}) ([A-Z]{3})(?: (.*))?$").ok());
static DIA_MES: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^(\d{1,2}) (\p{L}+)\.?$").ok());
static TIPO: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"^([A-Z]{3})(?: (.*))?$").ok());
static DINERO: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(?:([-+]) )?([€$£])(\d{1,3}(?: \d{3})*\.\d{2})").ok());
static ESTADO: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"\s*(?:Completado|Pendiente|Revertido|Rechazado|Cancelado|Completed|Pending|Reverted|Declined)\s+([^·]*?)\s*·\s*$",
    )
    .ok()
});

/// El texto sin los pies, títulos y notas de cada página, y lo que se ha sacado de ellos.
#[derive(Default)]
struct Limpio<'a> {
    lineas: Vec<&'a str>,
    generado: Option<Date>,
    /// La cuenta y la divisa de una búsqueda con filtros («* Cuentas: Main · EUR»).
    cuentas: Option<(String, String)>,
    filtrado: bool,
}

/// Un movimiento tal como sale: su fecha, su tipo y sus trozos de texto.
struct Fila {
    fecha: Date,
    tipo: String,
    partes: Vec<String>,
}

/// Una subcuenta en una divisa.
#[derive(Default)]
struct Seccion {
    etiqueta: String,
    divisa: Option<String>,
    iban: Option<String>,
    titular: String,
    desde: Option<Date>,
    hasta: Option<Date>,
    apertura: Option<Money>,
    cierre: Option<Money>,
    entrante: Option<Money>,
    saliente: Option<Money>,
    /// ¿La tabla tiene columna de saldo?
    con_saldo: bool,
    filas: Vec<Fila>,
}

/// ¿Es la cabecera de la tabla de movimientos (o su segunda línea, en la búsqueda)?
fn es_cabecera(t: &str) -> bool {
    [
        "Fecha (UTC)",
        "(UTC) Descripción",
        "Date (UTC)",
        "(UTC) Description",
    ]
    .iter()
    .any(|c| t.starts_with(c))
}

/// Extractos de Revolut, uno por divisa, si el texto es de uno.
pub(super) fn leer(texto: &str) -> Option<Vec<Extracto>> {
    if !texto.contains("Revolut Bank UAB") || !texto.lines().any(|l| es_cabecera(l.trim())) {
        return None;
    }
    let limpio = sin_pies(texto)?;
    let secciones = secciones(&limpio)?;
    let out: Vec<Extracto> = secciones
        .into_iter()
        .map(|s| extracto(s, &limpio))
        .collect();
    Some(out)
}

/// Quita los pies legales, los títulos, la fecha de generación, la nota de las subcuentas en
/// libras y las marcas de la búsqueda con filtros.
fn sin_pies(texto: &str) -> Option<Limpio<'_>> {
    let todas: Vec<&str> = texto.lines().collect();
    let mut l = Limpio::default();
    let mut i = 0;
    while let Some(linea) = todas.get(i) {
        let t = linea.trim();
        i += 1;
        if t == "Informar de una tarjeta perdida o robada" || t.starts_with("Report lost") {
            if let Some(j) = todas[i..]
                .iter()
                .take(30)
                .position(|x| PIE.as_ref().is_some_and(|re| re.is_match(x.trim())))
            {
                i += j + 1;
            }
            continue;
        }
        if t.starts_with("Para las transacciones entrantes y salientes") {
            if let Some(j) = todas[i - 1..]
                .iter()
                .take(8)
                .position(|x| x.trim_end().ends_with("al cliente."))
            {
                i += j;
            }
            continue;
        }
        if TITULO.as_ref()?.is_match(t)
            || t.starts_with("Tus fondos se encuentran")
            || t.starts_with("*Búsqueda y filtros")
        {
            l.filtrado |= t.starts_with("*Búsqueda");
            continue;
        }
        if let Some(c) = GENERADO.as_ref()?.captures(t) {
            l.generado = fecha(&c[1]);
            continue;
        }
        if let Some(c) = CUENTAS.as_ref()?.captures(t) {
            l.cuentas = Some((c[1].trim().to_owned(), c[2].to_owned()));
            continue;
        }
        l.lineas.push(linea);
    }
    Some(l)
}

/// Fecha de un movimiento («2 mar 2026», «25 sept 2026»).
fn fecha_fila(dia: &str, mes: &str, año: &str) -> Option<Date> {
    numeros::mes(mes)?;
    fecha(&format!("{dia} {mes} {año}"))
}

/// Si en `i` empieza un movimiento, lo devuelve y deja `i` en la línea siguiente. La búsqueda
/// con filtros parte a veces la fecha en tres líneas («25 sept», «2026», «CAR …»).
fn inicio(lineas: &[&str], i: &mut usize) -> Option<Fila> {
    let t = lineas.get(*i)?.trim();
    if let Some(c) = FILA.as_ref()?.captures(t)
        && let Some(f) = fecha_fila(&c[1], &c[2], &c[3])
    {
        *i += 1;
        return Some(Fila {
            fecha: f,
            tipo: c[4].to_owned(),
            partes: c
                .get(5)
                .map(|x| x.as_str().to_owned())
                .into_iter()
                .collect(),
        });
    }
    let c = DIA_MES.as_ref()?.captures(t)?;
    let mut siguientes = (*i + 1..lineas.len()).filter(|&j| !lineas[j].trim().is_empty());
    let (j, k) = (siguientes.next()?, siguientes.next()?);
    let año = lineas[j].trim();
    if año.len() != 4 || !año.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let f = fecha_fila(&c[1], &c[2], año)?;
    let tc = TIPO.as_ref()?.captures(lineas[k].trim())?;
    *i = k + 1;
    Some(Fila {
        fecha: f,
        tipo: tc[1].to_owned(),
        partes: tc
            .get(2)
            .map(|x| x.as_str().to_owned())
            .into_iter()
            .collect(),
    })
}

fn secciones(l: &Limpio<'_>) -> Option<Vec<Seccion>> {
    let mut secciones: Vec<Seccion> = Vec::new();
    let nueva = |etiqueta: &str, divisa: Option<String>| Seccion {
        etiqueta: etiqueta.to_owned(),
        divisa,
        ..Seccion::default()
    };
    // La búsqueda con filtros no tiene secciones: una sola, la de «* Cuentas».
    if let Some((etiqueta, divisa)) = &l.cuentas
        && !l
            .lineas
            .iter()
            .any(|x| x.trim().starts_with("Nombre de la cuenta "))
    {
        secciones.push(nueva(etiqueta, Some(divisa.clone())));
    }
    let mut en_tabla = false;
    let mut en_tipos = false;
    let mut esperando_titular = true;
    let mut i = 0;
    while let Some(linea) = l.lineas.get(i) {
        let t = linea.trim();
        if t.is_empty() {
            i += 1;
            continue;
        }
        if let Some(c) = NOMBRE.as_ref()?.captures(t) {
            secciones.push(nueva(c[1].trim(), None));
            en_tabla = false;
            en_tipos = false;
            esperando_titular = false;
            i += 1;
            continue;
        }
        if secciones.is_empty() {
            secciones.push(nueva("Main", None));
        }
        let Some(s) = secciones.last_mut() else {
            break;
        };
        // El resumen por tipos del final: sus importes no son movimientos.
        if t == "Tipos de transacción" || t == "Transaction types" {
            en_tipos = true;
        }
        if en_tipos {
            i += 1;
            continue;
        }
        if t == "Sin transacciones durante este periodo"
            || t == "No transactions during this period"
        {
            en_tabla = false;
            i += 1;
            continue;
        }
        if es_cabecera(t) {
            en_tabla = true;
            s.con_saldo = t.ends_with("Saldo") || t.ends_with("Balance");
            i += 1;
            continue;
        }
        if en_tabla {
            if let Some(f) = inicio(&l.lineas, &mut i) {
                s.filas.push(f);
                continue;
            }
            if let Some(f) = s.filas.last_mut() {
                f.partes.push(t.to_owned());
            }
            i += 1;
            continue;
        }
        i += 1;
        if let Some(c) = DIVISA.as_ref()?.captures(t) {
            s.divisa = Some(c[1].to_owned());
        } else if let Some(c) = IBAN.as_ref()?.captures(t) {
            let num = iban::compactar(&c[1]);
            if s.iban.is_none() && iban::valido(&num) {
                s.iban = Some(num);
            }
        } else if t.starts_with("BIC intermediario") || t.starts_with("Intermediary BIC") {
            esperando_titular = true;
        } else if let Some(c) = PERIODO.as_ref()?.captures(t) {
            s.desde = fecha(&c[1]);
            s.hasta = fecha(&c[2]);
        } else if let Some(c) = RESUMEN.as_ref()?.captures(t) {
            let m = DINERO
                .as_ref()?
                .captures(&c[2])
                .and_then(|d| importe(&d[3]));
            match &c[1] {
                "Saldo de apertura" | "Opening balance" => s.apertura = m,
                "Dinero entrante" | "Money in" => s.entrante = m,
                "Dinero saliente" | "Money out" => s.saliente = m,
                _ => s.cierre = m,
            }
        } else if esperando_titular
            && !linea.starts_with(' ')
            && t != "Fecha"
            && t != "Date"
            && !t.starts_with("Tipo ")
            && !t.starts_with("Type ")
            && !t.starts_with("BIC ")
        {
            // La empresa; el usuario que generó el extracto sale antes, con un espacio delante.
            s.titular = sin_punto(t);
            esperando_titular = false;
        }
    }
    Some(secciones.into_iter().flat_map(por_cuenta).collect())
}

/// La cuenta y la divisa de una fila de la búsqueda con filtros («Completado Main · » y, en la
/// línea siguiente, «EUR»).
fn cuenta_de_fila(f: &Fila) -> Option<(String, String)> {
    let re = ESTADO.as_ref()?;
    let i = f.partes.iter().position(|p| re.is_match(p))?;
    let etiqueta = re
        .captures(&f.partes[i])?
        .get(1)?
        .as_str()
        .trim()
        .to_owned();
    let divisa = f.partes.get(i + 1)?.trim();
    (divisa.len() == 3 && divisa.bytes().all(|b| b.is_ascii_uppercase()))
        .then(|| (etiqueta, divisa.to_owned()))
}

/// Parte la sección de una búsqueda con filtros que junta varias cuentas o divisas: cada fila
/// dice la suya.
fn por_cuenta(s: Seccion) -> Vec<Seccion> {
    if s.con_saldo || s.filas.iter().all(|f| cuenta_de_fila(f).is_none()) {
        return vec![s];
    }
    let base = (s.etiqueta.clone(), s.divisa.clone().unwrap_or_default());
    let mut grupos: Vec<((String, String), Vec<Fila>)> = Vec::new();
    for f in s.filas {
        let clave = cuenta_de_fila(&f).unwrap_or_else(|| base.clone());
        match grupos.iter_mut().find(|(k, _)| *k == clave) {
            Some((_, v)) => v.push(f),
            None => grupos.push((clave, vec![f])),
        }
    }
    grupos
        .into_iter()
        .map(|((etiqueta, divisa), filas)| Seccion {
            etiqueta,
            divisa: Some(divisa),
            iban: s.iban.clone(),
            titular: s.titular.clone(),
            desde: s.desde,
            hasta: s.hasta,
            filas,
            ..Seccion::default()
        })
        .collect()
}

/// Símbolo de la divisa con el que Revolut escribe los importes.
fn simbolo(divisa: &str) -> Option<&'static str> {
    Some(match divisa {
        "EUR" => "€",
        "USD" => "$",
        "GBP" => "£",
        _ => return None,
    })
}

fn divisa_de(simbolo: &str) -> &'static str {
    match simbolo {
        "€" => "EUR",
        "$" => "USD",
        _ => "GBP",
    }
}

/// Signo del importe por el tipo de operación: −1 si sale dinero, +1 si entra.
fn signo_del_tipo(tipo: &str) -> Option<i64> {
    match tipo {
        "CAR" | "FEE" | "MOS" | "ATM" | "EXO" => Some(-1),
        "MOA" | "MOR" | "EXI" => Some(1),
        _ => None,
    }
}

/// Un movimiento leído, todavía sin signo.
struct Leida {
    fecha: Date,
    tipo: String,
    concepto: String,
    importe: Money,
    saldo: Option<Money>,
}

/// Importe, saldo y concepto de una fila. `Err` con el aviso si no se ha podido leer.
fn leer_fila(f: &Fila, divisa: &str, con_saldo: bool) -> Result<Leida, String> {
    let (Some(re), Some(estado), Some(sim)) = (DINERO.as_ref(), ESTADO.as_ref(), simbolo(divisa))
    else {
        return Err(format!(
            "No se saben leer los importes en {divisa}: no se ha leído el movimiento del {}.",
            f.fecha
        ));
    };
    let todo = f.partes.join(" ");
    let mut propios: Vec<Money> = Vec::new();
    let mut otros: Vec<String> = Vec::new();
    for c in re.captures_iter(&todo) {
        let (Some(m), Some(s)) = (c.get(0), c.get(2)) else {
            continue;
        };
        let antes = &todo[..m.start()];
        let antes_ = antes.trim_end();
        if antes.ends_with(char::is_alphabetic)
            || antes_.ends_with("Comisión:")
            || antes_.ends_with("Fee:")
        {
            continue;
        }
        let Some(x) = importe(&c[3]) else {
            continue;
        };
        if s.as_str() == sim {
            propios.push(x);
        } else {
            otros.push(cantidad(x, divisa_de(s.as_str())));
        }
    }
    let (importe_, saldo) = match (con_saldo, propios.as_slice()) {
        (true, [.., i, s]) => (*i, Some(*s)),
        (false, [.., i]) => (*i, None),
        _ => {
            return Err(format!(
                "No se ha encontrado el importe del movimiento del {} («{}»): no se ha leído.",
                f.fecha,
                limpiar(&todo)
            ));
        }
    };
    let partes: Vec<String> = f
        .partes
        .iter()
        .filter(|p| {
            let p = p.trim_start();
            !p.starts_with("Tipo de cambio:") && !p.starts_with("Exchange rate:")
        })
        .map(|p| {
            let sin_dinero = re.replace_all(p, "");
            limpiar(&estado.replace(&sin_dinero, ""))
        })
        .filter(|p| !p.is_empty() && p != divisa)
        .collect();
    let mut concepto = unir(partes.iter().map(String::as_str));
    if !otros.is_empty() {
        concepto = format!("{concepto} ({})", otros.join(", "));
    }
    Ok(Leida {
        fecha: f.fecha,
        tipo: f.tipo.clone(),
        concepto,
        importe: importe_,
        saldo,
    })
}

fn extracto(s: Seccion, l: &Limpio<'_>) -> Extracto {
    let mut e = Extracto::nuevo("PDF");
    e.banco = "Revolut".to_owned();
    e.divisa = s.divisa.clone().unwrap_or_else(|| "EUR".to_owned());
    match &s.iban {
        Some(i) => e.con_iban(i),
        None => {
            e.cuenta = format!("revolut:{}", s.etiqueta);
            e.avisos.push(format!(
                "El extracto no da el IBAN de la cuenta «{}»: hay que decir a qué cuenta \
                 corresponde.",
                s.etiqueta
            ));
        }
    }
    e.titular = s.titular.clone();
    e.desde = s.desde;
    e.hasta = match (s.hasta, l.generado) {
        (Some(h), Some(g)) => Some(h.min(g)),
        (h, g) => h.or(g),
    };
    e.saldo_inicial = s.apertura;
    e.saldo_final = s.cierre;
    // Del más nuevo al más antiguo: se les da la vuelta.
    let mut leidas: Vec<Leida> = Vec::new();
    for f in s.filas.iter().rev() {
        match leer_fila(f, &e.divisa, s.con_saldo) {
            Ok(x) => leidas.push(x),
            Err(aviso) => e.avisos.push(aviso),
        }
    }
    let mut previo = s.apertura.or_else(|| {
        let p = leidas.first()?;
        let signo = signo_del_tipo(&p.tipo)?;
        Some(p.saldo? - Money::from_cents(signo * p.importe.cents()))
    });
    for x in leidas {
        let por_tipo = signo_del_tipo(&x.tipo);
        let con_signo = match (previo, x.saldo) {
            (Some(p), Some(saldo)) if saldo - p == x.importe => Some(x.importe),
            (Some(p), Some(saldo)) if saldo - p == -x.importe => Some(-x.importe),
            _ => None,
        };
        let imp = match (con_signo, por_tipo) {
            (Some(i), _) => i,
            (None, Some(sg)) => {
                if s.con_saldo {
                    e.avisos.push(format!(
                        "El movimiento del {} («{}») no encadena con el saldo anterior: el signo \
                         sale de su tipo ({}).",
                        x.fecha, x.concepto, x.tipo
                    ));
                }
                Money::from_cents(sg * x.importe.cents())
            }
            (None, None) => {
                e.avisos.push(format!(
                    "No se sabe si el movimiento del {} («{}», tipo {}) es un cargo o un abono: \
                     se ha tomado como cargo.",
                    x.fecha, x.concepto, x.tipo
                ));
                -x.importe
            }
        };
        previo = x.saldo.or(previo.map(|p| p + imp));
        e.movimientos.push(Movimiento {
            importe_eur: None,
            fecha: x.fecha,
            fecha_valor: None,
            concepto: x.concepto,
            importe: imp,
            saldo: x.saldo,
            referencia: String::new(),
        });
    }
    if !s.con_saldo && !e.movimientos.is_empty() {
        e.avisos.push(
            "El listado no trae saldos: el signo de cada movimiento sale de su tipo y no se puede \
             comprobar con el saldo."
                .to_owned(),
        );
    }
    if l.filtrado {
        e.avisos.push(
            "Es una búsqueda con filtros: puede que no estén todos los movimientos de la cuenta."
                .to_owned(),
        );
    }
    let entradas: Money = e
        .movimientos
        .iter()
        .filter(|m| !m.importe.is_negative())
        .map(|m| m.importe)
        .sum();
    let salidas: Money = e
        .movimientos
        .iter()
        .filter(|m| m.importe.is_negative())
        .map(|m| -m.importe)
        .sum();
    for (dice, suma, que) in [
        (s.entrante, entradas, "entrante"),
        (s.saliente, salidas, "saliente"),
    ] {
        if let Some(d) = dice
            && d != suma
        {
            e.avisos.push(format!(
                "El resumen dice {} de dinero {que} y los movimientos suman {}.",
                cantidad(d, &e.divisa),
                cantidad(suma, &e.divisa)
            ));
        }
    }
    e
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extractos::{Clase, Cuadre};
    use time::macros::date;

    /// Maqueta de 2025-2026 («Extracto de cuenta»): dos páginas, con resumen del saldo, pagos
    /// en dólares y los «Tipos de transacción» del final.
    const V2: &[&str] = &[
        "",
        "",
        "Informar de una tarjeta perdida o robada",
        "+34 900 94 32 45",
        "Obtén ayuda directamente en la aplicación",
        "Escanea el código QR",
        " Texto legal de ejemplo de la sucursal, inscrita con el número 0000, ",
        "en el tomo 1, folio 1, hoja M-000000. Dirección de ejemplo 20, 28001 Madrid. ",
        "Texto de relleno de la entidad, con número de empresa 000000000 y ",
        "código de autorización XX000000, con oficina en av. Ejemplo, 21B, Vilna. ",
        "Si tienes alguna pregunta, texto de relleno para el chat de la app. ",
        "Texto de relleno sobre la garantía de los depósitos que cumplan los ",
        "sitio web oficial:  .www.example.lt/en",
        "",
        "©   2026 Revolut Bank UAB, Sucursal en España 1/2",
        "",
        "Nombre de la cuenta Main",
        "",
        "Divisa EUR",
        "",
        "Tipo Local",
        "",
        "IBAN ES91 2100 0418 4502 0005 1332",
        "",
        "BIC REVOESM2",
        "",
        "Tipo Internacional",
        "",
        "IBAN ES91 2100 0418 4502 0005 1332",
        "",
        "BIC REVOESM2",
        "",
        "BIC intermediario DEMODEFX",
        "",
        "EMPRESA DEMO SL",
        "",
        "CALLE FALSA, 1 - BJ",
        "VALENCIA",
        "46000",
        "Spain",
        "",
        "Resumen del saldo",
        "",
        "Saldo de apertura €1 200.00",
        "",
        "Dinero entrante €2 199.00",
        "",
        "Dinero saliente - €3 053.08",
        "",
        "Saldo de cierre €345.92",
        "",
        "Tus fondos se encuentran almacenados y protegidos por un banco autorizado.",
        "",
        "       Transacciones de 1 de enero de 2026 a 28 de febrero de 2026",
        "",
        "Fecha (UTC) Descripción Dinero saliente Dinero entrante Saldo",
        "",
        "15 feb 2026 MOR Recompensa €50.00 €345.92",
        "",
        "3 feb 2026 FEE Comisión de Revolut Business • Tarifa del ",
        "",
        "plan Basic",
        " €10.00 €295.92",
        "",
        "2 feb 2026 CAR Restaurante Demo €45.60 €305.92",
        "",
        "30 ene 2026 MOS A PROVEEDOR DEMO SL • Factura 2026-001 €1 250.00 €351.52",
        "",
        "25 ene 2026 CAR Servicio Demo Online",
        "",
        "Tipo de cambio: 1\u{a0}EUR = 1.100000\u{a0}USD €22.73",
        "",
        "$25.00 €1 601.52",
        "",
        "20 ene 2026 MOR Reembolso de Tienda Demo Valencia €149.00 €1 624.25",
        "",
        "14 ene 2026 CAR Tienda Demo Valencia €1 649.00 €1 475.25",
        "",
        "Extracto de cuenta",
        "",
        " Documento generado el 28 de febrero de 2026",
        "",
        "Informar de una tarjeta perdida o robada",
        "+34 900 94 32 45",
        "Obtén ayuda directamente en la aplicación",
        "Escanea el código QR",
        " Texto legal de ejemplo de la sucursal, inscrita con el número 0000, ",
        "en el tomo 1, folio 1, hoja M-000000. Dirección de ejemplo 20, 28001 Madrid. ",
        "Texto de relleno de la entidad, con número de empresa 000000000 y ",
        "código de autorización XX000000, con oficina en av. Ejemplo, 21B, Vilna. ",
        "Si tienes alguna pregunta, texto de relleno para el chat de la app. ",
        "Texto de relleno sobre la garantía de los depósitos que cumplan los ",
        "sitio web oficial:  .www.example.lt/en",
        "",
        "©   2026 Revolut Bank UAB, Sucursal en España 2/2",
        "",
        "9 ene 2026 MOA Dinero añadido por EMPRESA DEMO SL ",
        "",
        "• Traspaso entre cuentas",
        " €2 000.00 €3 124.25",
        "",
        "6 ene 2026 CAR Software Demo Inc",
        "",
        "Tipo de cambio: 1\u{a0}EUR = 1.200000\u{a0}USD, Comisión: €0.50 €58.25",
        "",
        "$69.30 €1 124.25",
        "",
        "3 ene 2026 FEE Comisión de Revolut Business • Tarifa del ",
        "",
        "plan Basic",
        " €10.00 €1 182.50",
        "",
        "2 ene 2026 CAR Proveedor Hosting Demo €7.50 €1 192.50",
        "",
        "Tipos de transacción",
        "",
        " Pagos con tarjeta (CAR)",
        "- €1 782.58  Dinero enviado (MOS)",
        "- €1 250.00  Dinero recibido (MOR)",
        "+ €199.00  Dinero añadido (MOA)",
        "+ €2 000.00",
        "",
        " Retiradas en cajeros automáticos (ATM)",
        "€0.00  Operación de cambio saliente (EXO)",
        "€0.00  Operación de cambio entrante (EXI)",
        "€0.00  Comisiones de Revolut (FEE)",
        "- €20.50",
    ];

    /// Maqueta de 2024: la sección en euros y las de libras y dólares, vacías, con el mismo IBAN.
    const V1: &[&str] = &[
        "",
        "",
        "Informar de una tarjeta perdida o robada",
        "+34 900 94 32 45",
        "Obtén ayuda directamente en la aplicación",
        "Escanea el código QR",
        " Texto legal de ejemplo de la sucursal, inscrita con el número 0000, ",
        "en el tomo 1, folio 1, hoja M-000000. Dirección de ejemplo 20, 28001 Madrid. ",
        "Texto de relleno de la entidad, con número de empresa 000000000 y ",
        "código de autorización XX000000, con oficina en av. Ejemplo, 21B, Vilna. ",
        "Si tienes alguna pregunta, texto de relleno para el chat de la app. ",
        "Texto de relleno sobre la garantía de los depósitos que cumplan los ",
        "sitio web oficial:  .www.example.lt/en",
        "",
        "©   2024 Revolut Bank UAB, Sucursal en España 1/3",
        "",
        "Nombre de la cuenta Main",
        "",
        "Divisa EUR",
        "",
        "Tipo Local",
        "",
        "IBAN ES91 2100 0418 4502 0005 1332",
        "",
        "BIC REVOESM2",
        "",
        "Tipo Internacional",
        "",
        "IBAN ES91 2100 0418 4502 0005 1332",
        "",
        "BIC REVOESM2",
        "",
        "BIC intermediario DEMODEFX",
        "",
        " Usuario Demo",
        "EMPRESA DEMO SL",
        "",
        "CALLE FALSA, 1 - BJ",
        "VALENCIA",
        "46000",
        "Spain",
        "        Transacciones de 01 julio 2024 a 30 septiembre 2024",
        "",
        "Fecha (UTC) Descripción Dinero saliente Dinero entrante Saldo",
        "",
        "3 ago 2024 FEE Comisión de Revolut Business • Comisión del ",
        "",
        "plan Grow",
        " €25.00 €75.00",
        "",
        "1 ago 2024 MOR De Empresa Demo FM - EUR €100.00 €100.00",
        "",
        "5 jul 2024 MOS A Empresa Demo FM - EUR €12 000.00 €0.00",
        "",
        "4 jul 2024 MOA Dinero añadido por EMPRESA DEMO SOCIEDAD ",
        "",
        "LIMITADA • TRASPASO",
        " €12 000.00 €12 000.00",
        "",
        "Extracto de transacción",
        "",
        " Generado el 30 septiembre 2024",
        "",
        "Informar de una tarjeta perdida o robada",
        "+34 900 94 32 45",
        "Obtén ayuda directamente en la aplicación",
        "Escanea el código QR",
        " Texto legal de ejemplo de la sucursal, inscrita con el número 0000, ",
        "en el tomo 1, folio 1, hoja M-000000. Dirección de ejemplo 20, 28001 Madrid. ",
        "Texto de relleno de la entidad, con número de empresa 000000000 y ",
        "código de autorización XX000000, con oficina en av. Ejemplo, 21B, Vilna. ",
        "Si tienes alguna pregunta, texto de relleno para el chat de la app. ",
        "Texto de relleno sobre la garantía de los depósitos que cumplan los ",
        "sitio web oficial:  .www.example.lt/en",
        "",
        "©   2024 Revolut Bank UAB, Sucursal en España 2/3",
        "",
        "Nombre de la cuenta Main",
        "",
        "Divisa GBP",
        "",
        "Tipo Local",
        "",
        "Número de cuenta 00012345",
        "",
        "Sort code 00-11-22",
        "",
        "Tipo Internacional",
        "",
        "IBAN ES91 2100 0418 4502 0005 1332",
        "",
        "BIC REVOESM2",
        "",
        "BIC intermediario DEMOGB2L",
        "",
        " Usuario Demo",
        "EMPRESA DEMO SL",
        "",
        "CALLE FALSA, 1 - BJ",
        "VALENCIA",
        "46000",
        "Spain",
        "        Transacciones de 01 julio 2024 a 30 septiembre 2024",
        "",
        "Fecha (UTC) Descripción Dinero saliente Dinero entrante Saldo",
        "",
        "Sin transacciones durante este periodo",
        "",
        "Para las transacciones entrantes y salientes de tus subcuentas ajenas al EEE, ",
        "texto de relleno de la nota sobre el dinero electrónico y su efecto en la ",
        "empresa. Si quieres más información, ponte en contacto con atención al cliente.",
        " Extracto de transacción",
        "",
        " Generado el 30 septiembre 2024",
        "",
        "Informar de una tarjeta perdida o robada",
        "+34 900 94 32 45",
        "Obtén ayuda directamente en la aplicación",
        "Escanea el código QR",
        " Texto legal de ejemplo de la sucursal, inscrita con el número 0000, ",
        "en el tomo 1, folio 1, hoja M-000000. Dirección de ejemplo 20, 28001 Madrid. ",
        "Texto de relleno de la entidad, con número de empresa 000000000 y ",
        "código de autorización XX000000, con oficina en av. Ejemplo, 21B, Vilna. ",
        "Si tienes alguna pregunta, texto de relleno para el chat de la app. ",
        "Texto de relleno sobre la garantía de los depósitos que cumplan los ",
        "sitio web oficial:  .www.example.lt/en",
        "",
        "©   2024 Revolut Bank UAB, Sucursal en España 3/3",
        "",
        "Nombre de la cuenta Main",
        "",
        "Divisa USD",
        "",
        "Tipo Internacional",
        "",
        "IBAN ES91 2100 0418 4502 0005 1332",
        "",
        "BIC REVOESM2",
        "",
        "BIC intermediario DEMOGB2L",
        "",
        " Usuario Demo",
        "EMPRESA DEMO SL",
        "",
        "CALLE FALSA, 1 - BJ",
        "VALENCIA",
        "46000",
        "Spain",
        "        Transacciones de 01 julio 2024 a 30 septiembre 2024",
        "",
        "Fecha (UTC) Descripción Dinero saliente Dinero entrante Saldo",
        "",
        "Sin transacciones durante este periodo",
        "",
        "Extracto de transacción",
        "",
        " Generado el 30 septiembre 2024",
    ];

    /// Búsqueda con filtros de 2026: sin saldo ni IBAN, con fechas partidas en tres líneas.
    const V3: &[&str] = &[
        "",
        "",
        "* Cuentas: Main · EUR",
        "",
        "Informar de una tarjeta perdida o robada",
        "+34 900 94 32 45",
        "Obtén ayuda directamente en la aplicación",
        "Escanea el código QR",
        " Texto legal de ejemplo de la sucursal, inscrita con el número 0000, ",
        "en el tomo 1, folio 1, hoja M-000000. Dirección de ejemplo 20, 28001 Madrid. ",
        "Texto de relleno de la entidad, con número de empresa 000000000 y ",
        "código de autorización XX000000, con oficina en av. Ejemplo, 21B, Vilna. ",
        "Si tienes alguna pregunta, texto de relleno para el chat de la app. ",
        "Texto de relleno sobre la garantía de los depósitos que cumplan los ",
        "requisitos, de la institución pública \"Ejemplo\" en su documento y en su ",
        "sitio web oficial:  .www.example.lt/en",
        "",
        "©   2026 Revolut Bank UAB Sucursal En España 1/2",
        "",
        "EMPRESA DEMO SL",
        "",
        "CALLE FALSA, 1 - BJ",
        "VALENCIA",
        "46000",
        "Spain",
        "        Transacciones de 1 de enero de 2026 a 30 de septiembre de 2026",
        "",
        "*Búsqueda y filtros aplicados",
        "",
        "Fecha ",
        "(UTC) Descripción Estado Cuenta Dinero ",
        "saliente Dinero ",
        "entrante",
        "",
        "29 sept 2026 CAR Hosting Demo Gmbh Completado Main · ",
        "",
        "EUR",
        " €15.00",
        "",
        "26 sept 2026 FEE Comisión de Revolut Business • Tarifa del plan Basic Completado Main · ",
        "",
        "EUR",
        " €10.00",
        "",
        "22 sept ",
        "",
        "2026",
        " CAR Servicio Demo Llc",
        "",
        "Tipo de cambio: 1\u{a0}EUR = 1.125000\u{a0}USD Completado Main · ",
        "",
        "EUR",
        " €88.00",
        "",
        "$99.00",
        "",
        "24 ago 2026 MOA Dinero añadido por EMPRESA DEMO SL • Transferencia entre ",
        "",
        "cuentas",
        " Completado Main · ",
        "",
        "EUR",
        " €500.00",
        "",
        "Extracto de transacción",
        " Documento generado el 30 de septiembre de 2026",
        "",
        "* Cuentas: Main · EUR",
        "",
        "Informar de una tarjeta perdida o robada",
        "+34 900 94 32 45",
        "Obtén ayuda directamente en la aplicación",
        "Escanea el código QR",
        " Texto legal de ejemplo de la sucursal, inscrita con el número 0000, ",
        "en el tomo 1, folio 1, hoja M-000000. Dirección de ejemplo 20, 28001 Madrid. ",
        "Texto de relleno de la entidad, con número de empresa 000000000 y ",
        "código de autorización XX000000, con oficina en av. Ejemplo, 21B, Vilna. ",
        "Si tienes alguna pregunta, texto de relleno para el chat de la app. ",
        "Texto de relleno sobre la garantía de los depósitos que cumplan los ",
        "requisitos, de la institución pública \"Ejemplo\" en su documento y en su ",
        "sitio web oficial:  .www.example.lt/en",
        "",
        "©   2026 Revolut Bank UAB Sucursal En España 2/2",
        "",
        "18 ago 2026 CAR Editor Video Demo Completado Main · ",
        "",
        "EUR",
        " €25.00",
        "",
        "12 ago 2026 CAR Asistente Demo",
        "",
        "Tipo de cambio: 1\u{a0}EUR = 1.210000\u{a0}USD, Comisión: €0.20 Completado Main · ",
        "",
        "EUR",
        " €20.20",
        "",
        "$24.20",
        "",
        "4 ago 2026 CAR Oficina Demo Completado Main · ",
        "",
        "EUR",
        " €12.00",
    ];

    fn texto(lineas: &[&str]) -> String {
        lineas.join("\n")
    }

    type Fila<'a> = (Date, i64, Option<i64>, &'a str);

    fn filas(e: &Extracto) -> Vec<Fila<'_>> {
        e.movimientos
            .iter()
            .map(|m| {
                (
                    m.fecha,
                    m.importe.cents(),
                    m.saldo.map(Money::cents),
                    m.concepto.as_str(),
                )
            })
            .collect()
    }

    fn comun(e: &Extracto) {
        assert_eq!(e.formato, "PDF");
        assert_eq!(e.banco, "Revolut");
        assert_eq!(e.clase, Clase::Cuenta);
        assert_eq!(e.titular, "EMPRESA DEMO SL");
        assert!(
            e.movimientos
                .iter()
                .all(|m| m.fecha_valor.is_none() && m.referencia.is_empty())
        );
    }

    #[test]
    fn extracto_de_cuenta_con_resumen() {
        let mut v = leer(&texto(V2)).unwrap();
        assert_eq!(v.len(), 1);
        let mut e = v.remove(0);
        comun(&e);
        assert_eq!(e.iban.as_deref(), Some("ES9121000418450200051332"));
        assert_eq!(e.cuenta, "210004180200051332");
        assert_eq!(e.divisa, "EUR");
        assert_eq!(e.desde, Some(date!(2026 - 01 - 01)));
        assert_eq!(e.hasta, Some(date!(2026 - 02 - 28)));
        assert_eq!(e.saldo_inicial, Some(Money::from_cents(120_000)));
        assert_eq!(e.saldo_final, Some(Money::from_cents(34_592)));
        assert!(e.avisos.is_empty(), "{:?}", e.avisos);
        let cuota = "Comisión de Revolut Business • Tarifa del plan Basic";
        assert_eq!(
            filas(&e),
            [
                (
                    date!(2026 - 01 - 02),
                    -750,
                    Some(119_250),
                    "Proveedor Hosting Demo"
                ),
                (date!(2026 - 01 - 03), -1_000, Some(118_250), cuota),
                (
                    date!(2026 - 01 - 06),
                    -5_825,
                    Some(112_425),
                    "Software Demo Inc (69,30 USD)"
                ),
                (
                    date!(2026 - 01 - 09),
                    200_000,
                    Some(312_425),
                    "Dinero añadido por EMPRESA DEMO SL • Traspaso entre cuentas"
                ),
                (
                    date!(2026 - 01 - 14),
                    -164_900,
                    Some(147_525),
                    "Tienda Demo Valencia"
                ),
                (
                    date!(2026 - 01 - 20),
                    14_900,
                    Some(162_425),
                    "Reembolso de Tienda Demo Valencia"
                ),
                (
                    date!(2026 - 01 - 25),
                    -2_273,
                    Some(160_152),
                    "Servicio Demo Online (25,00 USD)"
                ),
                (
                    date!(2026 - 01 - 30),
                    -125_000,
                    Some(35_152),
                    "A PROVEEDOR DEMO SL • Factura 2026-001"
                ),
                (
                    date!(2026 - 02 - 02),
                    -4_560,
                    Some(30_592),
                    "Restaurante Demo"
                ),
                (date!(2026 - 02 - 03), -1_000, Some(29_592), cuota),
                (date!(2026 - 02 - 15), 5_000, Some(34_592), "Recompensa"),
            ]
        );
        assert!(e.cerrar().cuadra());
    }

    #[test]
    fn extracto_con_una_seccion_por_divisa() {
        let v = leer(&texto(V1)).unwrap();
        assert_eq!(v.len(), 3);
        for (e, divisa) in v.iter().zip(["EUR", "GBP", "USD"]) {
            comun(e);
            assert_eq!(e.divisa, divisa);
            // El mismo IBAN en las tres: la cuenta es la misma clave con otra divisa.
            assert_eq!(e.cuenta, "210004180200051332");
            assert_eq!(e.desde, Some(date!(2024 - 07 - 01)));
            assert_eq!(e.hasta, Some(date!(2024 - 09 - 30)));
            assert!(e.avisos.is_empty(), "{:?}", e.avisos);
        }
        assert!(v[1].movimientos.is_empty() && v[2].movimientos.is_empty());
        let mut e = v[0].clone();
        assert_eq!((e.saldo_inicial, e.saldo_final), (None, None));
        assert_eq!(
            filas(&e),
            [
                (
                    date!(2024 - 07 - 04),
                    1_200_000,
                    Some(1_200_000),
                    "Dinero añadido por EMPRESA DEMO SOCIEDAD LIMITADA • TRASPASO"
                ),
                (
                    date!(2024 - 07 - 05),
                    -1_200_000,
                    Some(0),
                    "A Empresa Demo FM - EUR"
                ),
                (
                    date!(2024 - 08 - 01),
                    10_000,
                    Some(10_000),
                    "De Empresa Demo FM - EUR"
                ),
                (
                    date!(2024 - 08 - 03),
                    -2_500,
                    Some(7_500),
                    "Comisión de Revolut Business • Comisión del plan Grow"
                ),
            ]
        );
        assert!(e.cerrar().cuadra());
        assert_eq!(e.saldo_inicial, Some(Money::ZERO));
        assert_eq!(e.saldo_final, Some(Money::from_cents(7_500)));
    }

    #[test]
    fn busqueda_con_filtros_sin_saldos() {
        let mut v = leer(&texto(V3)).unwrap();
        assert_eq!(v.len(), 1);
        let mut e = v.remove(0);
        comun(&e);
        assert_eq!(e.iban, None);
        assert_eq!(e.cuenta, "revolut:Main");
        assert_eq!(e.divisa, "EUR");
        assert_eq!(e.desde, Some(date!(2026 - 01 - 01)));
        assert_eq!(e.hasta, Some(date!(2026 - 09 - 30)));
        assert_eq!((e.saldo_inicial, e.saldo_final), (None, None));
        // Sin IBAN, sin saldos y con filtros.
        assert_eq!(e.avisos.len(), 3, "{:?}", e.avisos);
        assert_eq!(
            filas(&e),
            [
                (date!(2026 - 08 - 04), -1_200, None, "Oficina Demo"),
                (
                    date!(2026 - 08 - 12),
                    -2_020,
                    None,
                    "Asistente Demo (24,20 USD)"
                ),
                (date!(2026 - 08 - 18), -2_500, None, "Editor Video Demo"),
                (
                    date!(2026 - 08 - 24),
                    50_000,
                    None,
                    "Dinero añadido por EMPRESA DEMO SL • Transferencia entre cuentas"
                ),
                (
                    date!(2026 - 09 - 22),
                    -8_800,
                    None,
                    "Servicio Demo Llc (99,00 USD)"
                ),
                (
                    date!(2026 - 09 - 26),
                    -1_000,
                    None,
                    "Comisión de Revolut Business • Tarifa del plan Basic"
                ),
                (date!(2026 - 09 - 29), -1_500, None, "Hosting Demo Gmbh"),
            ]
        );
        assert_eq!(e.cerrar(), Cuadre::SinSaldos);
    }

    #[test]
    fn busqueda_con_varias_cuentas() {
        let t = texto(V3).replace(
            "Editor Video Demo Completado Main · \n\nEUR\n €25.00",
            "Editor Video Demo Completado Ahorro · \n\nUSD\n $25.00",
        );
        let v = leer(&t).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(
            (v[0].cuenta.as_str(), v[0].divisa.as_str()),
            ("revolut:Main", "EUR")
        );
        assert_eq!(v[0].movimientos.len(), 6);
        assert_eq!(
            (v[1].cuenta.as_str(), v[1].divisa.as_str()),
            ("revolut:Ahorro", "USD")
        );
        assert_eq!(
            filas(&v[1]),
            [(date!(2026 - 08 - 18), -2_500, None, "Editor Video Demo")]
        );
        assert_eq!(v[1].titular, "EMPRESA DEMO SL");
        assert_eq!(v[1].desde, Some(date!(2026 - 01 - 01)));
    }

    #[test]
    fn avisa_si_un_movimiento_no_encadena_con_el_saldo() {
        let t = texto(V2).replace("Saldo de apertura €1 200.00", "Saldo de apertura €1 100.00");
        let mut e = leer(&t).unwrap().remove(0);
        assert_eq!(e.movimientos[0].importe.cents(), -750);
        assert!(
            e.avisos.iter().any(|a| a.contains("no encadena")),
            "{:?}",
            e.avisos
        );
        assert!(!e.cerrar().cuadra());
    }

    #[test]
    fn otra_seccion_despues_de_los_tipos_de_transaccion() {
        let mut lineas = V2.to_vec();
        lineas.extend([
            "",
            "Nombre de la cuenta Main",
            "",
            "Divisa USD",
            "",
            "Tipo Internacional",
            "",
            "IBAN ES91 2100 0418 4502 0005 1332",
            "",
            "BIC intermediario DEMOGB2L",
            "",
            "EMPRESA DEMO SL",
            "",
            "Resumen del saldo",
            "",
            "Saldo de apertura $0.00",
            "",
            "Dinero entrante $1 100.00",
            "",
            "Dinero saliente - $0.00",
            "",
            "Saldo de cierre $1 100.00",
            "",
            "       Transacciones de 1 de enero de 2026 a 28 de febrero de 2026",
            "",
            "Fecha (UTC) Descripción Dinero saliente Dinero entrante Saldo",
            "",
            "5 feb 2026 MOR De Cliente Demo Usa $1 100.00 $1 100.00",
        ]);
        let v = leer(&texto(&lineas)).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].movimientos.len(), 11);
        let mut e = v[1].clone();
        assert_eq!(
            (e.cuenta.as_str(), e.divisa.as_str()),
            ("210004180200051332", "USD")
        );
        assert_eq!(
            filas(&e),
            [(
                date!(2026 - 02 - 05),
                110_000,
                Some(110_000),
                "De Cliente Demo Usa"
            )]
        );
        assert!(e.avisos.is_empty(), "{:?}", e.avisos);
        assert!(e.cerrar().cuadra());
    }

    #[test]
    fn etiquetas_en_ingles() {
        let mut t = texto(V2);
        for (es, en) in [
            (
                "Informar de una tarjeta perdida o robada",
                "Report lost or stolen card",
            ),
            ("Nombre de la cuenta", "Account name"),
            ("Divisa EUR", "Currency EUR"),
            ("Tipo Local", "Type Local"),
            ("Tipo Internacional", "Type International"),
            ("BIC intermediario", "Intermediary BIC"),
            ("Saldo de apertura", "Opening balance"),
            ("Dinero entrante €", "Money in €"),
            ("Dinero saliente - €", "Money out - €"),
            ("Saldo de cierre", "Closing balance"),
            (
                "Transacciones de 1 de enero de 2026 a 28 de febrero de 2026",
                "Transactions from 1 January 2026 to 28 February 2026",
            ),
            (
                "Fecha (UTC) Descripción Dinero saliente Dinero entrante Saldo",
                "Date (UTC) Description Money out Money in Balance",
            ),
            ("Tipo de cambio:", "Exchange rate:"),
            ("Comisión: €", "Fee: €"),
            (" ene 2026", " Jan 2026"),
            ("Extracto de cuenta", "Account statement"),
            (
                "Documento generado el 28 de febrero de 2026",
                "Generated on 28 February 2026",
            ),
            ("Tipos de transacción", "Transaction types"),
        ] {
            assert!(t.contains(es), "{es}");
            t = t.replace(es, en);
        }
        let mut e = leer(&t).unwrap().remove(0);
        assert_eq!(e.cuenta, "210004180200051332");
        assert_eq!(e.titular, "EMPRESA DEMO SL");
        assert_eq!(
            (e.desde, e.hasta),
            (Some(date!(2026 - 01 - 01)), Some(date!(2026 - 02 - 28)))
        );
        assert_eq!(e.movimientos.len(), 11);
        assert_eq!(e.movimientos[2].importe.cents(), -5_825);
        assert!(e.avisos.is_empty(), "{:?}", e.avisos);
        assert!(e.cerrar().cuadra());
    }

    #[test]
    fn sin_la_huella_de_revolut_no_es_suyo() {
        assert!(leer(&texto(V2).replace("Revolut Bank UAB", "Otro banco")).is_none());
        assert!(leer(&texto(V2).replace("Fecha (UTC)", "Fecha")).is_none());
    }
}
