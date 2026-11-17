//! Las filas de la tabla de movimientos: los movimientos, las filas de saldo que van entre
//! ellos (que no son movimientos), las que siguen el concepto de la de arriba, y luego el signo
//! de los importes y el orden.

use taxpain_core::money::Money;
use time::Date;

use super::super::cuadre::signos_por_saldo;
use super::super::iban;
use super::super::numeros::{importe, inferir_orden};
use super::cabecera::Columnas;
use super::datos::divisa;
use super::diccionario::{Etiqueta, es_total, etiqueta_de_saldo};
use super::texto::{clave, fecha_cerca, limpio};

/// Un movimiento tal como viene en la tabla.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Fila {
    pub fecha: Date,
    pub fecha_valor: Option<Date>,
    pub concepto: String,
    pub referencia: String,
    /// La divisa de la fila, si la tabla tiene esa columna.
    pub divisa: Option<String>,
    /// La cuenta de la fila (IBAN o número), si la tabla la repite en cada fila.
    pub cuenta: Option<String>,
    pub importe: Money,
    pub comision: Option<Money>,
    pub saldo: Option<Money>,
    /// Los euros de la operación, si la tabla los da (ver [`euros_de`]); sólo cuentan en una
    /// cuenta en otra divisa.
    pub importe_eur: Option<Money>,
}

/// Lo que sale de leer las filas de una tabla.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Lectura {
    /// En el orden del fichero.
    pub filas: Vec<Fila>,
    /// La fila siguiente a la última de datos: lo de debajo ya son datos de la cuenta.
    pub fin: usize,
    /// Las filas de saldo inicial y final de dentro de la tabla («SALDO ANTERIOR»).
    pub saldo_inicial: Option<Money>,
    pub saldo_final: Option<Money>,
    /// Filas con fecha y sin importe.
    pub sin_importe: usize,
    /// Filas con importe cuya fecha no se entiende («29/02/2025», un OCR mal leído): se pierden.
    pub fecha_ilegible: usize,
    /// Movimientos pendientes, rechazados o anulados, que no se leen.
    pub descartados: usize,
    /// ¿Los importes traen su signo (alguno es negativo, o van en columnas de cargo y abono, o
    /// hay columna de signo)?
    pub con_signo: bool,
    /// La divisa que dicen los símbolos de los importes («12,00 €», «$5.00»).
    pub simbolo: Option<String>,
}

/// Cómo van los decimales en una columna de importes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Decimales {
    Coma,
    Punto,
    NoSe,
}

/// Los decimales de una columna: los que llevan casi todos sus importes.
fn decimales<'a>(celdas: impl Iterator<Item = &'a str>) -> Decimales {
    let (mut coma, mut punto) = (0usize, 0usize);
    for c in celdas {
        let t = c.trim().trim_end_matches(|ch: char| !ch.is_ascii_digit());
        let (p, q) = (t.rfind('.'), t.rfind(','));
        let ultimo = match (p, q) {
            (Some(p), Some(q)) => Some(if p > q { ('.', p) } else { (',', q) }),
            (Some(p), None) => Some(('.', p)),
            (None, Some(q)) => Some((',', q)),
            (None, None) => None,
        };
        if let Some((sep, i)) = ultimo
            && ((1..=2).contains(&(t.len() - i - 1)) || (p.is_some() && q.is_some()))
        {
            if sep == ',' {
                coma += 1;
            } else {
                punto += 1;
            }
        }
    }
    match coma.cmp(&punto) {
        std::cmp::Ordering::Greater => Decimales::Coma,
        std::cmp::Ordering::Less => Decimales::Punto,
        std::cmp::Ordering::Equal => Decimales::NoSe,
    }
}

/// ¿Es `t` un número con un solo separador `sep` y tres cifras detrás («1.234», «-0,125»)?
fn tres_decimales(t: &str, sep: char) -> bool {
    let u = t.trim_start_matches(['-', '+', '−']);
    let Some((a, b)) = u.split_once(sep) else {
        return false;
    };
    (1..=3).contains(&a.len())
        && b.len() == 3
        && a.bytes().all(|x| x.is_ascii_digit())
        && b.bytes().all(|x| x.is_ascii_digit())
}

/// El importe de una celda, con los decimales de su columna: «1.234» es mil doscientos treinta
/// y cuatro si la columna va con coma decimal, y uno con veintitrés si va con punto (el número
/// de una hoja de cálculo).
fn dinero(s: &str, d: Decimales) -> Option<Money> {
    let t = s.trim();
    if t.is_empty() || matches!(t, "-" | "—" | "–") {
        return None;
    }
    let ambiguo = match d {
        Decimales::Punto => tres_decimales(t, '.'),
        Decimales::Coma => tres_decimales(t, ','),
        Decimales::NoSe => false,
    };
    if ambiguo {
        // Con un cero más ya no parece de miles.
        return importe(&format!("{t}0"));
    }
    importe(t)
}

fn celda(f: &[String], c: usize) -> &str {
    f.get(c).map_or("", |s| s.trim())
}

/// ¿Es el valor de una columna de signo un cargo (`Some(true)`) o un abono? `con_a` dice si la
/// columna usa «A» de abono (entonces «C» es cargo; si no, «C» es crédito).
fn signo(v: &str, con_a: bool) -> Option<bool> {
    match v.trim() {
        "-" => return Some(true),
        "+" => return Some(false),
        _ => {}
    }
    match clave(v).as_str() {
        "d" | "dr" | "db" | "deb" | "debe" | "cargo" | "debito" | "debit" | "af" | "soll" | "s"
        | "out" | "outgoing" => Some(true),
        "h" | "cr" | "cred" | "haber" | "abono" | "credito" | "credit" | "bij" | "haben" | "a"
        | "in" | "incoming" => Some(false),
        "c" => Some(con_a),
        _ => None,
    }
}

/// ¿Es un estado de un movimiento que no ha llegado a la cuenta?
fn descartado(v: &str) -> bool {
    let k = clave(v);
    matches!(
        k.split(' ').next().unwrap_or(""),
        "pending"
            | "pendiente"
            | "declined"
            | "rechazada"
            | "rechazado"
            | "denegada"
            | "denegado"
            | "denied"
            | "reverted"
            | "revertida"
            | "revertido"
            | "failed"
            | "fallida"
            | "fallido"
            | "cancelled"
            | "canceled"
            | "cancelada"
            | "cancelado"
            | "anulada"
            | "anulado"
            | "refused"
            | "expired"
            | "memo"
    )
}

/// La cuenta de una celda: el IBAN, el de un código cuenta cliente o el número tal cual (si
/// tiene cifras suficientes).
fn cuenta_de(v: &str) -> Option<String> {
    let v = v.trim();
    if let Some(i) = iban::en_texto(v).into_iter().next() {
        return Some(i);
    }
    if let Some(i) = iban::desde_ccc(v) {
        return Some(i);
    }
    let c = iban::compactar(v);
    (c.bytes().filter(u8::is_ascii_digit).count() >= 6).then_some(c)
}

/// El concepto de una fila: sus columnas de concepto, sin espacios de más y unidas con « · »
/// (sin las vacías, las enmascaradas «*****» ni los «-»).
fn concepto_de(f: &[String], col: &Columnas) -> String {
    let mut partes: Vec<String> = Vec::new();
    for c in &col.conceptos {
        let t = limpio(celda(f, *c));
        if t.is_empty() || t.chars().all(|ch| matches!(ch, '*' | '-' | '.')) {
            continue;
        }
        if partes.last() != Some(&t) {
            partes.push(t);
        }
    }
    partes.join(" · ")
}

/// Los decimales de las columnas de dinero de la tabla.
#[derive(Clone, Copy, Debug)]
struct Formatos {
    importe: Decimales,
    cargo: Decimales,
    abono: Decimales,
    saldo: Decimales,
    comision: Decimales,
    /// La de los euros de la operación (contravalor, importe de origen o de llegada).
    cambio: Decimales,
    /// Si la columna de signo usa «A» de abono.
    con_a: bool,
}

/// El importe de una fila: el de su columna, o abono menos cargo, con el signo de la columna de
/// signo si la hay.
fn importe_de(f: &[String], col: &Columnas, fm: &Formatos) -> Option<Money> {
    let base = match col.importe {
        Some(c) => dinero(celda(f, c), fm.importe)?,
        None => {
            let cargo = col
                .cargo
                .and_then(|c| dinero(celda(f, c), fm.cargo))
                .map(|m| -m.abs());
            let abono = col
                .abono
                .and_then(|c| dinero(celda(f, c), fm.abono))
                .map(Money::abs);
            if cargo.is_none() && abono.is_none() {
                return None;
            }
            cargo.unwrap_or_default() + abono.unwrap_or_default()
        }
    };
    if let Some(c) = col.signo
        && let Some(neg) = signo(celda(f, c), fm.con_a)
    {
        return Some(if neg { -base.abs() } else { base.abs() });
    }
    Some(base)
}

/// Los euros de una fila, con el signo de su importe `imp`, si las columnas del cambio los dan:
/// el contravalor del banco; el importe de origen si la divisa de origen es el euro (Revolut
/// Business); lo que salió al cambiar a euros, o el importe entre el tipo si se cambió desde
/// euros (Wise: «Exchange From/To», «Exchange To Amount», «Exchange Rate»).
fn euros_de(f: &[String], col: &Columnas, d: Decimales, imp: Money) -> Option<Money> {
    let c = &col.cambio;
    let con_signo = |m: Money| {
        let m = m.abs();
        if imp.is_negative() { -m } else { m }
    };
    let es_eur = |k: Option<usize>| k.and_then(|k| divisa(celda(f, k))).as_deref() == Some("EUR");
    let cifra = |k: Option<usize>| {
        k.and_then(|k| dinero(celda(f, k), d))
            .filter(|m| !m.is_zero())
    };
    if let Some(m) = cifra(c.contravalor) {
        return Some(con_signo(m));
    }
    if es_eur(c.divisa_origen)
        && let Some(m) = cifra(c.importe_origen)
    {
        return Some(con_signo(m));
    }
    if es_eur(c.cambio_a)
        && let Some(m) = cifra(c.importe_cambio_a)
    {
        return Some(con_signo(m));
    }
    if es_eur(c.cambio_de)
        && let Some(t) = c.tipo_cambio.map(|k| celda(f, k))
    {
        // Como mucho 6 decimales, los del tipo del BCE.
        let t = match t.find(['.', ',']) {
            Some(p) if t.len() > p + 7 => &t[..p + 7],
            _ => t,
        };
        if let Some(r) = taxpain_core::fx::ExchangeRate::parse(t) {
            return Some(con_signo(r.to_eur(imp)));
        }
    }
    None
}

/// Lee las filas `desde..hasta` de una tabla con estas columnas. `cerca` es la fecha a la que
/// acercar las que vienen sin año (el final del periodo, si se sabe). En una hoja de cálculo
/// (`de_libro`), un importe con un solo punto («1.234») es un número con decimales, no de
/// miles: así lo escribe la hoja.
pub(super) fn leer(
    filas: &[Vec<String>],
    desde: usize,
    hasta: usize,
    col: &Columnas,
    cerca: Option<Date>,
    de_libro: bool,
) -> Lectura {
    let hasta = hasta.min(filas.len());
    let rango = filas.get(desde..hasta).unwrap_or(&[]);
    let muestras: Vec<&str> = rango
        .iter()
        .flat_map(|f| {
            [Some(col.fecha), col.fecha_valor]
                .into_iter()
                .flatten()
                .map(|c| celda(f, c))
        })
        .collect();
    let orden = inferir_orden(muestras);
    let dec = |c: Option<usize>| match c.map(|c| decimales(rango.iter().map(|f| celda(f, c)))) {
        Some(Decimales::NoSe) | None if de_libro => Decimales::Punto,
        Some(d) => d,
        None => Decimales::NoSe,
    };
    let fm = Formatos {
        importe: dec(col.importe),
        cargo: dec(col.cargo),
        abono: dec(col.abono),
        saldo: dec(col.saldo),
        comision: dec(col.comision),
        cambio: dec(col
            .cambio
            .contravalor
            .or(col.cambio.importe_origen)
            .or(col.cambio.importe_cambio_a)),
        con_a: col
            .signo
            .is_some_and(|c| rango.iter().any(|f| clave(celda(f, c)) == "a")),
    };
    let dinero_cols: Vec<usize> = [col.importe, col.cargo, col.abono, col.saldo]
        .into_iter()
        .flatten()
        .collect();
    let mut l = Lectura {
        fin: desde,
        ..Lectura::default()
    };
    let mut anclas: Vec<(usize, Etiqueta, Money)> = Vec::new();
    let mut seguida = false;
    let mut negativos = false;
    for (k, f) in rango.iter().enumerate() {
        let n = desde + k;
        if f.iter().all(|c| c.trim().is_empty()) {
            seguida = false;
            continue;
        }
        let concepto = concepto_de(f, col);
        let saldo = col.saldo.and_then(|c| dinero(celda(f, c), fm.saldo));
        let imp = importe_de(f, col, &fm);
        if l.simbolo.is_none() {
            l.simbolo = dinero_cols.iter().find_map(|c| {
                let t = celda(f, *c);
                if t.contains('€') {
                    Some("EUR".to_owned())
                } else if t.contains('£') {
                    Some("GBP".to_owned())
                } else if t.contains('$') {
                    Some("USD".to_owned())
                } else {
                    None
                }
            });
        }
        let fecha = fecha_cerca(celda(f, col.fecha), orden, cerca).or_else(|| {
            col.fecha_otra
                .filter(|_| celda(f, col.fecha).is_empty())
                .and_then(|c| fecha_cerca(celda(f, c), orden, cerca))
        });
        let Some(fecha) = fecha else {
            // Sin fecha: una fila de saldo («SALDO INICIAL», con la cifra en el importe o en el
            // saldo), la continuación del concepto de la de arriba o nada.
            let etiqueta = if concepto.is_empty() {
                f.iter()
                    .map(|c| c.trim())
                    .find(|c| !c.is_empty())
                    .unwrap_or("")
            } else {
                concepto.as_str()
            };
            if let Some(e) = etiqueta_de_saldo(etiqueta)
                && let Some(v) = saldo
                    .or(imp)
                    .or_else(|| f.iter().find_map(|c| importe(c.trim())))
            {
                anclas.push((n, e, v));
                seguida = false;
                continue;
            }
            let solo_concepto = f
                .iter()
                .enumerate()
                .all(|(c, v)| v.trim().is_empty() || col.conceptos.contains(&c));
            if seguida
                && imp.is_none()
                && saldo.is_none()
                && !concepto.is_empty()
                && solo_concepto
                && let Some(u) = l.filas.last_mut()
            {
                u.concepto.push(' ');
                u.concepto.push_str(&concepto);
                l.fin = n + 1;
                continue;
            }
            // Una fecha que no se entiende junto a un importe es un movimiento que se pierde:
            // se cuenta para avisar.
            if imp.is_some() && !celda(f, col.fecha).is_empty() && !es_total(etiqueta) {
                l.fecha_ilegible += 1;
            }
            seguida = false;
            continue;
        };
        seguida = false;
        if let Some(e) = etiqueta_de_saldo(&concepto) {
            if let Some(v) = saldo.or(imp) {
                anclas.push((n, e, v));
            }
            continue;
        }
        if es_total(&concepto) {
            continue;
        }
        if col.estados.iter().any(|c| descartado(celda(f, *c))) {
            l.descartados += 1;
            continue;
        }
        let Some(imp) = imp else {
            l.sin_importe += 1;
            continue;
        };
        negativos |= imp.is_negative();
        let fecha_valor = col
            .fecha_valor
            .and_then(|c| fecha_cerca(celda(f, c), orden, Some(fecha)));
        let referencia = col
            .referencias
            .iter()
            .map(|c| limpio(celda(f, *c)))
            .filter(|s| !s.is_empty() && s != "-")
            .collect::<Vec<_>>()
            .join(" ");
        let divisa_fila = col.divisa.and_then(|c| divisa(celda(f, c)));
        // Una comisión en otra divisa (473 JPY al cambiar euros a yenes en Revolut) no va en
        // el importe: el movimiento ya la trae descontada.
        let comision_ajena = col
            .divisa_comision
            .and_then(|c| divisa(celda(f, c)))
            .is_some_and(|d| {
                divisa_fila
                    .as_deref()
                    .or(col.divisa_titulo.as_deref())
                    .is_some_and(|m| m != d)
            });
        l.filas.push(Fila {
            fecha,
            fecha_valor,
            concepto,
            referencia,
            divisa: divisa_fila,
            cuenta: col.cuenta.and_then(|c| cuenta_de(celda(f, c))),
            importe: imp,
            comision: col
                .comision
                .filter(|_| !comision_ajena)
                .and_then(|c| dinero(celda(f, c), fm.comision)),
            saldo,
            importe_eur: if col.cambio.alguna() {
                euros_de(f, col, fm.cambio, imp)
            } else {
                None
            },
        });
        l.fin = n + 1;
        seguida = true;
    }
    // Las filas de saldo de debajo de los movimientos se leen con los datos de la cuenta.
    for (n, e, v) in anclas {
        if n >= l.fin {
            continue;
        }
        match e {
            Etiqueta::SaldoInicial if l.saldo_inicial.is_none() => l.saldo_inicial = Some(v),
            Etiqueta::SaldoFinal => l.saldo_final = Some(v),
            _ => {}
        }
    }
    l.con_signo = col.cargo.is_some() || col.abono.is_some() || col.signo.is_some() || negativos;
    l
}

/// Cuántas filas encadenan su saldo con la anterior: en el orden del fichero y en el contrario
/// (del más nuevo al más antiguo).
fn cadena(imp: &[Money], saldos: &[Option<Money>]) -> (usize, usize) {
    let (mut adelante, mut atras) = (0, 0);
    for i in 1..imp.len().min(saldos.len()) {
        if let (Some(a), Some(b)) = (saldos[i - 1], saldos[i]) {
            if a + imp[i] == b {
                adelante += 1;
            }
            if b + imp[i - 1] == a {
                atras += 1;
            }
        }
    }
    (adelante, atras)
}

fn encadenan(imp: &[Money], saldos: &[Option<Money>]) -> usize {
    let (a, b) = cadena(imp, saldos);
    a.max(b)
}

/// Pone el signo que falta a los importes: resta la comisión que va en otra columna, deduce del
/// saldo el de los importes que vienen sin él y da la vuelta a los que vienen al revés (como
/// los ve el banco). `inicial` es el saldo de antes del primer movimiento, si se sabe.
pub(super) fn signos(
    l: &mut Lectura,
    col: &Columnas,
    inicial: Option<Money>,
    avisos: &mut Vec<String>,
) {
    let saldos: Vec<Option<Money>> = l.filas.iter().map(|f| f.saldo).collect();
    let hay_saldos = saldos.iter().filter(|s| s.is_some()).count() >= 2;
    if !col.neto
        && l.filas
            .iter()
            .any(|f| f.comision.is_some_and(|c| !c.is_zero()))
    {
        let a: Vec<Money> = l.filas.iter().map(|f| f.importe).collect();
        let b: Vec<Money> = l
            .filas
            .iter()
            .map(|f| f.importe - f.comision.unwrap_or_default().abs())
            .collect();
        if !hay_saldos || encadenan(&b, &saldos) > encadenan(&a, &saldos) {
            for (f, x) in l.filas.iter_mut().zip(b) {
                f.importe = x;
            }
            avisos.push(
                "A cada movimiento se le ha restado su comisión, que el extracto da en otra \
                 columna."
                    .to_owned(),
            );
        }
    }
    if !l.con_signo && hay_saldos {
        let imp: Vec<Money> = l.filas.iter().map(|f| f.importe.abs()).collect();
        if let Some(s) = signos_por_saldo(&imp, &saldos, inicial)
            && s.importes.iter().flatten().any(|m| m.is_negative())
        {
            let mut sin = 0;
            for (f, x) in l.filas.iter_mut().zip(&s.importes) {
                match x {
                    Some(m) => f.importe = *m,
                    None if !f.importe.is_zero() => sin += 1,
                    None => {}
                }
            }
            if sin > 0 {
                avisos.push(format!(
                    "El extracto da los importes sin signo y el de {sin} no se ha podido sacar \
                     del saldo: se han dejado como abonos; revísalos."
                ));
            }
        }
    }
    if hay_saldos {
        let imp: Vec<Money> = l.filas.iter().map(|f| f.importe).collect();
        let vuelta: Vec<Money> = imp.iter().map(|m| -*m).collect();
        if encadenan(&vuelta, &saldos) > encadenan(&imp, &saldos) {
            for f in &mut l.filas {
                f.importe = -f.importe;
            }
            avisos.push(
                "Los cargos y los abonos venían con el signo al revés (como los ve el banco): \
                 se les ha dado la vuelta."
                    .to_owned(),
            );
        }
    }
}

/// Pone los movimientos de más antiguo a más nuevo. Si el fichero va del más nuevo al más
/// antiguo, se le da la vuelta entera (y así también queda bien el orden dentro de cada día);
/// si todos son del mismo día, lo dice el saldo.
pub(super) fn ordenar(filas: &mut [Fila]) {
    let (Some(a), Some(b)) = (filas.first(), filas.last()) else {
        return;
    };
    let al_reves = if a.fecha == b.fecha {
        let imp: Vec<Money> = filas.iter().map(|f| f.importe).collect();
        let saldos: Vec<Option<Money>> = filas.iter().map(|f| f.saldo).collect();
        let (adelante, atras) = cadena(&imp, &saldos);
        atras > adelante
    } else {
        a.fecha > b.fecha
    };
    if al_reves {
        filas.reverse();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn filas(v: &[&[&str]]) -> Vec<Vec<String>> {
        v.iter()
            .map(|f| f.iter().map(|s| (*s).to_owned()).collect())
            .collect()
    }

    fn cols(conceptos: Vec<usize>) -> Columnas {
        Columnas {
            fecha: 0,
            conceptos,
            importe: Some(2),
            saldo: Some(3),
            alto: 1,
            ..Columnas::default()
        }
    }

    #[test]
    fn decimales_de_la_columna() {
        let d = |v: &[&str]| decimales(v.iter().copied());
        assert_eq!(d(&["1.234,56", "12,5", "1.500"]), Decimales::Coma);
        assert_eq!(d(&["-10", "1234.56", "5.2", "1.234"]), Decimales::Punto);
        assert_eq!(
            dinero("1.234", Decimales::Punto),
            Some(Money::from_cents(123))
        );
        assert_eq!(
            dinero("1.234", Decimales::Coma),
            Some(Money::from_cents(123_400))
        );
        assert_eq!(dinero("-", Decimales::NoSe), None);
    }

    #[test]
    fn saldo_inicial_continuaciones_y_totales() {
        let f = filas(&[
            &["", "SALDO INICIAL", "100,00", ""],
            &["01/02/2025", "RECIBO LUZ", "-10,00", "90,00"],
            &["", "SEGUNDA LINEA", "", ""],
            &["02/02/2025", "TRANSFERENCIA", "5,00", "95,00"],
            &["02/02/2025", "TOTAL", "-5,00", ""],
            &["03/02/2025", "PENDIENTE DE IMPORTE", "", ""],
            &[],
            &["", "Saldo total", "", "95,00"],
        ]);
        let l = leer(&f, 0, f.len(), &cols(vec![1]), None, false);
        assert_eq!(l.filas.len(), 2);
        assert_eq!(l.filas[0].concepto, "RECIBO LUZ SEGUNDA LINEA");
        assert_eq!(l.saldo_inicial, Some(Money::from_cents(10_000)));
        // El saldo total va debajo de la tabla: lo leen los datos de la cuenta.
        assert_eq!(l.saldo_final, None);
        assert_eq!(l.fin, 4);
        assert_eq!(l.sin_importe, 1);
        assert_eq!(l.fecha_ilegible, 0);
        assert!(l.con_signo);
    }

    #[test]
    fn una_fecha_que_no_se_entiende_se_cuenta() {
        let f = filas(&[
            &["28/02/2025", "A", "-10,00", "90,00"],
            &["29/02/2025", "B", "-20,00", "70,00"],
            &["O1/03/2025", "C", "-5,00", "65,00"],
            &["01/03/2025", "D", "5,00", "70,00"],
            &["Total", "", "-30,00", ""],
        ]);
        let l = leer(&f, 0, f.len(), &cols(vec![1]), None, false);
        assert_eq!(l.filas.len(), 2);
        assert_eq!(l.fecha_ilegible, 2);
    }

    #[test]
    fn signos_del_saldo_y_orden() {
        // Del más nuevo al más antiguo y sin signo.
        let f = filas(&[
            &["03/02/2025", "B", "5,00", "95,00"],
            &["02/02/2025", "A", "10,00", "90,00"],
            &["01/02/2025", "X", "100,00", "100,00"],
        ]);
        let c = cols(vec![1]);
        let mut l = leer(&f, 0, f.len(), &c, None, false);
        assert!(!l.con_signo);
        let mut avisos = Vec::new();
        signos(&mut l, &c, None, &mut avisos);
        let imp: Vec<i64> = l.filas.iter().map(|f| f.importe.cents()).collect();
        assert_eq!(imp, [500, -1000, 10_000]);
        ordenar(&mut l.filas);
        assert_eq!(l.filas[0].fecha, date!(2025 - 02 - 01));
        assert_eq!(avisos.len(), 1);
    }
}
