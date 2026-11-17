//! Activity Flex de Interactive Brokers (XML o CSV): lo que devuelve el Flex Web Service o se
//! descarga a mano desde Client Portal → Performance & Reports → Flex Queries.
//!
//! Estructura del XML (guía de referencia de las Activity Flex Queries de IBKR, consultada el
//! 2026-11-02): `<FlexQueryResponse><FlexStatements><FlexStatement accountId fromDate toDate>`
//! con, entre otras, las secciones `AccountInformation` (la divisa base), `Trades/Trade`,
//! `CashTransactions/CashTransaction`, `OpenPositions/OpenPosition`,
//! `SecuritiesInfo/SecurityInfo`, `ConversionRates/ConversionRate` y
//! `CorporateActions/CorporateAction`. Los datos van en atributos; las fechas, por defecto
//! `yyyyMMdd` y `yyyyMMdd;HHmmss`.
//! - Trade: `quantity` (negativa en las ventas), `tradePrice`, `tradeMoney`, `proceeds`,
//!   `ibCommission` (negativa) e `ibCommissionCurrency`, `taxes`, `fxRateToBase` (de la divisa
//!   a la base), `buySell`, `transactionID`, `tradeID`, `levelOfDetail` (`EXECUTION`, `ORDER`,
//!   `CLOSED_LOT`…) y `assetCategory` (`STK` con `subCategory="ETF"` en los ETF, `FUND`, `BOND`,
//!   `CASH` en las conversiones de divisa…). Las cancelaciones llevan `transactionType=
//!   "TradeCancel"` o `buySell="BUY (Ca.)"` y el `origTradeID` de la que anulan.
//! - CashTransaction: `type` (`Dividends`, `Payment In Lieu Of Dividends`, `Withholding Tax`,
//!   `Broker Interest Received`/`Paid`, `Bond Interest Received`/`Paid`, `Other Fees`,
//!   `Commission Adjustments`, `Deposits & Withdrawals`…), `amount` con signo, `currency`,
//!   `fxRateToBase`, `dateTime`, `transactionID` e `issuerCountryCode`.
//! - OpenPosition: `position`, `markPrice` (cierre de `reportDate`), `levelOfDetail`
//!   (`SUMMARY` o `LOT`).
//! - ConversionRate: `reportDate`, `fromCurrency`, `toCurrency` (la base) y `rate`.
//!
//! En el CSV las columnas se llaman distinto («ClientAccountID», «CurrencyPrimary»,
//! «Buy/Sell»…): se pasan a los nombres del XML.

use std::collections::{BTreeMap, BTreeSet};

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use sha2::{Digest, Sha256};
use taxpain_core::cripto::{Precio, Qty};
use taxpain_core::money::Money;
use time::{Date, Month};

use super::importar::{CambioLeido, Extracto, OpLeida, PosicionLeida, PrecioLeido, ValorLeido};
use super::{Broker, Clase, TipoOp, a_euros, isin_valido};

type Attrs = BTreeMap<String, String>;

/// Un `FlexStatement` (una cuenta) con sus filas.
#[derive(Debug, Default)]
struct Estado {
    attrs: Attrs,
    filas: Vec<(String, Attrs)>,
}

const ELEMENTOS: [&str; 8] = [
    "AccountInformation",
    "Trade",
    "CashTransaction",
    "OpenPosition",
    "SecurityInfo",
    "ConversionRate",
    "CorporateAction",
    "Transfer",
];

pub fn es_flex_xml(texto: &str) -> bool {
    texto.contains("<FlexQueryResponse") || texto.contains("<FlexStatements")
}

pub fn es_flex_csv(texto: &str) -> bool {
    texto
        .lines()
        .take(5)
        .any(|l| l.to_lowercase().contains("clientaccountid"))
}

fn atributos(e: &BytesStart<'_>) -> Attrs {
    e.attributes()
        .flatten()
        .filter_map(|a| {
            let k = a.key.local_name().as_ref().to_owned();
            let v = a
                .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                .ok()?
                .into_owned();
            Some((k, v))
        })
        .collect()
}

/// Error de una respuesta del Flex Web Service guardada como fichero.
fn error_flex(texto: &str) -> Option<String> {
    if !texto.contains("<FlexStatementResponse") {
        return None;
    }
    let entre = |a: &str, b: &str| {
        let i = texto.find(a)? + a.len();
        let j = texto[i..].find(b)? + i;
        Some(texto[i..j].trim().to_owned())
    };
    Some(format!(
        "es una respuesta de error del Flex Web Service ({} {})",
        entre("<ErrorCode>", "</ErrorCode>").unwrap_or_default(),
        entre("<ErrorMessage>", "</ErrorMessage>").unwrap_or_default()
    ))
}

/// Lee el XML de una Activity Flex Query: un extracto por cuenta.
pub fn leer_xml(texto: &str) -> Result<Vec<Extracto>, String> {
    if let Some(e) = error_flex(texto) {
        return Err(e);
    }
    let mut reader = Reader::from_str(texto);
    let mut estados: Vec<Estado> = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e) | Event::Empty(e)) => {
                let nombre = e.local_name().as_ref().to_owned();
                if nombre == "FlexStatement" {
                    estados.push(Estado {
                        attrs: atributos(&e),
                        filas: Vec::new(),
                    });
                } else if ELEMENTOS.contains(&nombre.as_str()) {
                    if estados.is_empty() {
                        estados.push(Estado::default());
                    }
                    if let Some(s) = estados.last_mut() {
                        s.filas.push((nombre, atributos(&e)));
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("el XML de IBKR no se puede leer: {e}")),
            _ => {}
        }
    }
    if estados.is_empty() {
        return Err("el XML no trae ningún extracto (FlexStatement) de IBKR".into());
    }
    estados.iter().map(convertir).collect()
}

/// Divide una línea CSV (comillas dobles, comillas repetidas dentro).
fn campos(linea: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut actual = String::new();
    let mut comillas = false;
    let mut it = linea.chars().peekable();
    while let Some(c) = it.next() {
        match c {
            '"' if comillas && it.peek() == Some(&'"') => {
                actual.push('"');
                it.next();
            }
            '"' => comillas = !comillas,
            ',' if !comillas => out.push(std::mem::take(&mut actual)),
            _ => actual.push(c),
        }
    }
    out.push(actual);
    out.into_iter().map(|s| s.trim().to_owned()).collect()
}

/// Columna del CSV → atributo del XML.
fn columna(c: &str) -> String {
    let k: String = c
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_lowercase();
    match k.as_str() {
        "clientaccountid" | "accountid" => "accountId",
        "currencyprimary" | "currency" => "currency",
        "assetclass" | "assetcategory" => "assetCategory",
        "subcategory" => "subCategory",
        "symbol" => "symbol",
        "description" => "description",
        "conid" => "conid",
        "isin" => "isin",
        "issuercountrycode" => "issuerCountryCode",
        "fxratetobase" => "fxRateToBase",
        "tradeid" => "tradeID",
        "transactionid" => "transactionID",
        "origtradeid" => "origTradeID",
        "tradedate" => "tradeDate",
        "datetime" => "dateTime",
        "date" => "date",
        "reportdate" => "reportDate",
        "transactiontype" => "transactionType",
        "buysell" => "buySell",
        "quantity" => "quantity",
        "tradeprice" | "price" => "tradePrice",
        "trademoney" => "tradeMoney",
        "proceeds" => "proceeds",
        "taxes" => "taxes",
        "ibcommission" | "commission" => "ibCommission",
        "ibcommissioncurrency" | "commissioncurrency" => "ibCommissionCurrency",
        "levelofdetail" => "levelOfDetail",
        "type" => "type",
        "amount" => "amount",
        "position" => "position",
        "markprice" => "markPrice",
        "positionvalue" => "positionValue",
        "fromcurrency" => "fromCurrency",
        "tocurrency" => "toCurrency",
        "rate" => "rate",
        _ => return c.to_owned(),
    }
    .to_owned()
}

/// Lee el CSV de una Activity Flex Query (varias secciones, cada una con su fila de títulos).
pub fn leer_csv(texto: &str) -> Result<Vec<Extracto>, String> {
    let mut por_cuenta: BTreeMap<String, Estado> = BTreeMap::new();
    let mut titulos: Option<Vec<String>> = None;
    for linea in texto.lines() {
        if linea.trim().is_empty() {
            continue;
        }
        let mut f = campos(linea);
        // Con «Include Section Code and Line Descriptor»: HEADER/DATA y el código de sección.
        match f.first().map(String::as_str) {
            Some("BOF" | "EOF" | "BOS" | "EOS") => continue,
            Some("HEADER" | "DATA") if f.len() > 2 => {
                f.drain(..2);
            }
            _ => {}
        }
        if f.iter().any(|c| columna(c) == "accountId") {
            titulos = Some(f.iter().map(|c| columna(c)).collect());
            continue;
        }
        let Some(t) = &titulos else { continue };
        let attrs: Attrs = t
            .iter()
            .cloned()
            .zip(f)
            .filter(|(_, v)| !v.is_empty())
            .collect();
        let tiene = |k: &str| t.iter().any(|c| c == k);
        let elemento = if tiene("buySell") || tiene("tradePrice") {
            "Trade"
        } else if tiene("type") && tiene("amount") {
            "CashTransaction"
        } else if tiene("position") && tiene("markPrice") {
            "OpenPosition"
        } else if tiene("fromCurrency") && tiene("rate") {
            "ConversionRate"
        } else {
            continue;
        };
        let cuenta = attrs.get("accountId").cloned().unwrap_or_default();
        let e = por_cuenta.entry(cuenta.clone()).or_insert_with(|| Estado {
            attrs: [("accountId".to_owned(), cuenta)].into_iter().collect(),
            filas: Vec::new(),
        });
        e.filas.push((elemento.to_owned(), attrs));
    }
    if por_cuenta.is_empty() {
        return Err("el CSV no trae filas de una Activity Flex Query de IBKR".into());
    }
    por_cuenta.values().map(convertir).collect()
}

fn g<'a>(a: &'a Attrs, k: &str) -> &'a str {
    a.get(k).map(String::as_str).unwrap_or("").trim()
}

/// Número del Flex (punto decimal; sin separador de miles salvo comas) escalado por
/// 10^`decimales`, redondeado.
fn decimal(s: &str, decimales: u32) -> Option<i128> {
    let t: String = s.trim().chars().filter(|c| *c != ',').collect();
    if t.is_empty() || t.eq_ignore_ascii_case("n/a") || t == "--" {
        return None;
    }
    let (neg, t) = match t.strip_prefix('-') {
        Some(r) => (true, r.to_owned()),
        None => (false, t.trim_start_matches('+').to_owned()),
    };
    let (ent, dec) = t.split_once('.').unwrap_or((&t, ""));
    if !ent.bytes().all(|b| b.is_ascii_digit()) || !dec.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if ent.is_empty() && dec.is_empty() {
        return None;
    }
    let escala = 10i128.checked_pow(decimales)?;
    let ent: i128 = if ent.is_empty() { 0 } else { ent.parse().ok()? };
    let n = decimales as usize;
    let mut d: String = dec.chars().take(n).collect();
    while d.len() < n {
        d.push('0');
    }
    let mut v = ent.checked_mul(escala)?;
    if !d.is_empty() {
        v = v.checked_add(d.parse::<i128>().ok()?)?;
    }
    if dec.len() > n && dec.as_bytes()[n] >= b'5' {
        v += 1;
    }
    Some(if neg { -v } else { v })
}

fn dinero(s: &str) -> Option<Money> {
    decimal(s, 2).and_then(|c| i64::try_from(c).ok().map(Money::from_cents))
}

fn precio(s: &str) -> Option<Precio> {
    decimal(s, 8).map(Precio::from_raw)
}

fn titulos(s: &str) -> Option<Qty> {
    decimal(s, 18).map(Qty::from_raw)
}

/// Fecha del Flex: `yyyyMMdd`, `yyyy-MM-dd` o `yyyy/MM/dd`, con o sin la hora detrás.
fn fecha(s: &str) -> Option<Date> {
    let d = s.trim().split([';', ' ', ',', 'T']).next()?;
    let (y, m, dd) = if d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit()) {
        (&d[..4], &d[4..6], &d[6..8])
    } else if d.len() == 10 && (d.as_bytes()[4] == b'-' || d.as_bytes()[4] == b'/') {
        (&d[..4], &d[5..7], &d[8..10])
    } else {
        return None;
    };
    Date::from_calendar_date(
        y.parse().ok()?,
        Month::try_from(m.parse::<u8>().ok()?).ok()?,
        dd.parse().ok()?,
    )
    .ok()
}

/// Hora de un `dateTime` (`yyyyMMdd;HHmmss`, `yyyy-MM-dd;HH:mm:ss`…) como `HH:MM:SS`.
fn hora(s: &str) -> String {
    let t = s.trim();
    let resto = t
        .split_once([';', ' ', ',', 'T'])
        .map(|(_, r)| r.trim())
        .unwrap_or("");
    let d: String = resto.chars().filter(char::is_ascii_digit).collect();
    if d.len() >= 6 {
        format!("{}:{}:{}", &d[..2], &d[2..4], &d[4..6])
    } else {
        "00:00:00".into()
    }
}

fn huella(cuenta: &str, tipo: &str, a: &Attrs) -> String {
    let id = [g(a, "transactionID"), g(a, "tradeID"), g(a, "actionID")]
        .into_iter()
        .find(|x| !x.is_empty() && *x != "0");
    match id {
        Some(id) => format!("ibkr:{cuenta}:{tipo}:{id}"),
        None => {
            let mut h = Sha256::new();
            for (k, v) in a {
                h.update(k.as_bytes());
                h.update(b"=");
                h.update(v.as_bytes());
                h.update(b";");
            }
            format!("ibkr:{cuenta}:{tipo}:h{}", &hex::encode(h.finalize())[..24])
        }
    }
}

/// Euros por unidad de `divisa`. Con base EUR, el `fxRateToBase` de la fila; si no, el de las
/// ConversionRates del día; con otra base, el de la fila entre el del euro a la base.
struct Cambios {
    base: String,
    /// (fecha, divisa) → unidades de la base por 1 de la divisa.
    tabla: BTreeMap<(Date, String), Precio>,
}

impl Cambios {
    fn a_base(&self, fecha: Date, divisa: &str) -> Option<Precio> {
        if divisa == self.base {
            return Some(Precio::from_euros(1));
        }
        self.tabla
            .range(..=(fecha, divisa.to_owned()))
            .rev()
            .find(|((d, c), _)| c == divisa && *d >= fecha - time::Duration::days(7))
            .map(|(_, p)| *p)
    }

    fn eur_por_unidad(&self, fecha: Date, divisa: &str, fila: Option<Precio>) -> Option<Precio> {
        if divisa == "EUR" {
            return Some(Precio::from_euros(1));
        }
        let a_base = fila
            .filter(|p| !p.is_zero())
            .or_else(|| self.a_base(fecha, divisa))?;
        if self.base == "EUR" {
            return Some(a_base);
        }
        // Euro → base: unidades de la base por 1 €.
        let eur = self.a_base(fecha, "EUR")?;
        if eur.is_zero() {
            return None;
        }
        Some(Precio::from_raw(a_base.raw() * 100_000_000 / eur.raw()))
    }
}

fn clase(cat: &str, sub: &str) -> Option<Clase> {
    match (cat, sub) {
        ("STK", s) if s.eq_ignore_ascii_case("ETF") => Some(Clase::Etf),
        ("ETF", _) => Some(Clase::Etf),
        ("STK", _) => Some(Clase::Accion),
        ("FUND", _) => Some(Clase::Fondo),
        ("BOND" | "BILL", _) => Some(Clase::Bono),
        _ => None,
    }
}

fn pais(a: &Attrs) -> String {
    let p = g(a, "issuerCountryCode");
    if p.len() == 2 {
        return p.to_uppercase();
    }
    let isin = g(a, "isin");
    if isin_valido(isin) {
        isin[..2].to_owned()
    } else {
        String::new()
    }
}

/// Proporción de un desdoblamiento o agrupación en la descripción de IBKR («SPLIT 1 FOR 10»):
/// (títulos nuevos, títulos viejos).
fn proporcion_split(desc: &str) -> Option<(i128, i128)> {
    let d = desc.to_uppercase();
    let resto = &d[d.find("SPLIT ")? + 6..];
    let mut p = resto.split_whitespace();
    let n: i128 = p.next()?.parse().ok()?;
    (p.next()? == "FOR").then_some(())?;
    let m: i128 = p
        .next()?
        .trim_end_matches(|c: char| !c.is_ascii_digit())
        .parse()
        .ok()?;
    (n > 0 && m > 0).then_some((n, m))
}

/// Fecha de una fila de agrupación (RS).
fn fecha_rs(a: &Attrs) -> Option<Date> {
    fecha(g(a, "reportDate")).or_else(|| fecha(g(a, "dateTime")))
}

/// Movimiento de efectivo por las fracciones de una agrupación («cash in lieu of fractional
/// shares»). No lo es el «Payment In Lieu Of Dividends» (el sustituto de un dividendo).
/// TODO(VERIFY): el `type` y la `description` exactos que da IBKR no se han contrastado con un
/// Flex real.
fn es_efectivo_por_fraccion(a: &Attrs) -> bool {
    let d = format!("{} {}", g(a, "type"), g(a, "description")).to_uppercase();
    d.contains("IN LIEU") && !d.contains("DIVIDEND")
}

fn clave(a: &Attrs) -> Option<String> {
    let isin = g(a, "isin").to_uppercase();
    if isin_valido(&isin) {
        return Some(isin);
    }
    let s = g(a, "symbol").to_uppercase();
    (!s.is_empty()).then_some(s)
}

fn convertir(st: &Estado) -> Result<Extracto, String> {
    let cuenta = {
        let c = g(&st.attrs, "accountId");
        if c.is_empty() {
            st.filas
                .iter()
                .map(|(_, a)| g(a, "accountId"))
                .find(|c| !c.is_empty())
                .unwrap_or("")
                .to_owned()
        } else {
            c.to_owned()
        }
    };
    if cuenta.is_empty() {
        return Err("el extracto de IBKR no dice la cuenta (accountId)".into());
    }
    let mut avisos: Vec<String> = Vec::new();
    // Divisa base: la de AccountInformation o, si no viene, la de las filas con cambio 1.
    let base = st
        .filas
        .iter()
        .find(|(n, _)| n == "AccountInformation")
        .map(|(_, a)| g(a, "currency").to_uppercase())
        .filter(|c| c.len() == 3)
        .or_else(|| {
            st.filas
                .iter()
                .filter(|(n, _)| n == "Trade" || n == "CashTransaction")
                .find(|(_, a)| precio(g(a, "fxRateToBase")) == Some(Precio::from_euros(1)))
                .map(|(_, a)| g(a, "currency").to_uppercase())
        })
        .unwrap_or_else(|| {
            avisos.push(
                "El extracto no dice la divisa base de la cuenta (marca «Account Information» \
                 en la Flex Query): se supone EUR."
                    .into(),
            );
            "EUR".into()
        });
    let mut e = Extracto::nuevo(Broker::Ibkr, &cuenta, &base);
    e.desde = fecha(g(&st.attrs, "fromDate"));
    e.hasta = fecha(g(&st.attrs, "toDate"));

    let mut cambios = Cambios {
        base: base.clone(),
        tabla: BTreeMap::new(),
    };
    for (_, a) in st.filas.iter().filter(|(n, _)| n == "ConversionRate") {
        let (Some(d), Some(r)) = (
            fecha(g(a, "reportDate")).or_else(|| fecha(g(a, "dateTime"))),
            precio(g(a, "rate")),
        ) else {
            continue;
        };
        let de = g(a, "fromCurrency").to_uppercase();
        let a_ = g(a, "toCurrency").to_uppercase();
        if de.len() != 3 || r.is_zero() || (!a_.is_empty() && a_ != base) {
            continue;
        }
        cambios.tabla.insert((d, de.clone()), r);
        if base == "EUR" && de != "EUR" {
            e.cambios.push(CambioLeido {
                fecha: d,
                divisa: de,
                eur_por_unidad: r,
            });
        }
    }

    // Valores: de SecuritiesInfo y de las filas que los nombran.
    let mut valores: BTreeMap<String, ValorLeido> = BTreeMap::new();
    let mut info: BTreeMap<String, Attrs> = BTreeMap::new();
    for (_, a) in st.filas.iter().filter(|(n, _)| n == "SecurityInfo") {
        if let Some(k) = clave(a) {
            info.insert(k, a.clone());
        }
    }
    let mut anotar = |a: &Attrs, clase_: Clase| -> Option<String> {
        let k = clave(a)?;
        let i = info.get(&k).unwrap_or(a);
        valores.entry(k.clone()).or_insert_with(|| ValorLeido {
            clave: k.clone(),
            isin: if isin_valido(&k) {
                k.clone()
            } else {
                String::new()
            },
            ticker: {
                let s = g(i, "symbol").to_uppercase();
                if s.is_empty() { k.clone() } else { s }
            },
            nombre: g(i, "description").to_owned(),
            divisa: {
                let c = g(i, "currency").to_uppercase();
                if c.len() == 3 { c } else { base.clone() }
            },
            clase: clase(g(i, "assetCategory"), g(i, "subCategory")).unwrap_or(clase_),
            pais: pais(i),
        });
        Some(k)
    };

    let mut sin_cambio = 0usize;
    let mut derivados = 0usize;
    let mut divisas = 0usize;
    let mut a_euros_fila = |fecha_: Date, divisa: &str, a: &Attrs, importe: Money| {
        let fx = cambios.eur_por_unidad(fecha_, divisa, precio(g(a, "fxRateToBase")));
        match fx {
            Some(fx) if divisa == "EUR" => (None, importe, Some(fx)),
            Some(fx) => (Some(fx), a_euros(importe, fx), Some(fx)),
            None => {
                sin_cambio += 1;
                (None, Money::ZERO, None)
            }
        }
    };

    // Operaciones de compraventa (sin las anuladas ni los resúmenes).
    let trades: Vec<&Attrs> = st
        .filas
        .iter()
        .filter(|(n, _)| n == "Trade")
        .map(|(_, a)| a)
        .collect();
    let hay_ejecuciones = trades
        .iter()
        .any(|a| matches!(g(a, "levelOfDetail"), "" | "EXECUTION"));
    let anuladas: BTreeSet<String> = trades
        .iter()
        .filter(|a| es_anulacion(a))
        .map(|a| g(a, "origTradeID").to_owned())
        .filter(|x| !x.is_empty())
        .collect();
    for a in &trades {
        let nivel = g(a, "levelOfDetail");
        let vale = if hay_ejecuciones {
            matches!(nivel, "" | "EXECUTION")
        } else {
            nivel == "ORDER"
        };
        if !vale || es_anulacion(a) || anuladas.contains(g(a, "tradeID")) {
            continue;
        }
        let cat = g(a, "assetCategory");
        if cat == "CASH" {
            divisas += 1;
            continue;
        }
        let Some(cl) = clase(cat, g(a, "subCategory")) else {
            derivados += 1;
            continue;
        };
        let Some(k) = anotar(a, cl) else { continue };
        let Some(f) = fecha(g(a, "tradeDate")).or_else(|| fecha(g(a, "dateTime"))) else {
            avisos.push(format!(
                "Operación de {k} sin fecha legible («{}»): usa el formato de fecha por \
                 defecto (yyyyMMdd)",
                g(a, "tradeDate")
            ));
            continue;
        };
        let q = titulos(g(a, "quantity")).unwrap_or_default();
        let lado = g(a, "buySell").to_uppercase();
        let compra = if lado.starts_with("BUY") {
            true
        } else if lado.starts_with("SELL") {
            false
        } else {
            !q.is_negative()
        };
        let pr = precio(g(a, "tradePrice"));
        let divisa = {
            let c = g(a, "currency").to_uppercase();
            if c.len() == 3 { c } else { base.clone() }
        };
        let bruto = dinero(g(a, "tradeMoney"))
            .or_else(|| pr.and_then(|p| q.abs().valor(p)))
            .or_else(|| dinero(g(a, "proceeds")))
            .unwrap_or_default()
            .abs();
        let com_div = g(a, "ibCommissionCurrency").to_uppercase();
        let com = dinero(g(a, "ibCommission")).unwrap_or_default().abs();
        let tasas = dinero(g(a, "taxes")).unwrap_or_default().abs();
        let (cambio, euros, fx) = a_euros_fila(f, &divisa, a, bruto);
        let (comision, comision_eur) = if com_div.is_empty() || com_div == divisa {
            let c = com + tasas;
            (c, fx.map(|x| a_euros(c, x)).unwrap_or_default())
        } else if com_div == "EUR" {
            (
                tasas,
                com + fx.map(|x| a_euros(tasas, x)).unwrap_or_default(),
            )
        } else {
            (
                com + tasas,
                fx.map(|x| a_euros(com + tasas, x)).unwrap_or_default(),
            )
        };
        e.ops.push(OpLeida {
            fecha: f,
            hora: hora(g(a, "dateTime")),
            tipo: if compra {
                TipoOp::Compra
            } else {
                TipoOp::Venta
            },
            valor: Some(k),
            cantidad: q.abs(),
            precio: pr,
            divisa,
            importe: bruto,
            comision,
            cambio,
            euros,
            comision_eur,
            pais: String::new(),
            nota: g(a, "description").to_owned(),
            fuente: "ibkr".into(),
            huella: huella(&cuenta, "t", a),
            comision_huella: Vec::new(),
        });
    }

    // Hasta aquí, las compraventas (para buscar la venta de la fracción de una agrupación).
    let mut fin_trades = e.ops.len();
    // Valor y fecha de cada agrupación (RS): su efectivo por las fracciones no es un movimiento
    // aparte, va con la agrupación.
    let rs: BTreeSet<(String, Date)> = st
        .filas
        .iter()
        .filter(|(n, a)| n == "CorporateAction" && g(a, "type").eq_ignore_ascii_case("RS"))
        .filter_map(|(_, a)| Some((clave(a)?, fecha_rs(a)?)))
        .collect();
    let mut efectivo_rs: BTreeMap<(String, Date), Vec<&Attrs>> = BTreeMap::new();

    // Movimientos de efectivo.
    let mut tipos_raros: BTreeMap<String, usize> = BTreeMap::new();
    for (_, a) in st.filas.iter().filter(|(n, _)| n == "CashTransaction") {
        if g(a, "levelOfDetail").eq_ignore_ascii_case("SUMMARY") {
            continue;
        }
        if es_efectivo_por_fraccion(a)
            && let Some(k) = clave(a)
            && let Some(f) = fecha(g(a, "dateTime")).or_else(|| fecha(g(a, "reportDate")))
            && rs.contains(&(k.clone(), f))
        {
            efectivo_rs.entry((k, f)).or_default().push(a);
            continue;
        }
        let tipo_ib = g(a, "type").to_owned();
        let t = tipo_ib.to_lowercase();
        let Some(importe) = dinero(g(a, "amount")) else {
            continue;
        };
        let (tipo, importe) = if t.contains("withholding") || t.contains("871(m)") {
            (TipoOp::Retencion, -importe)
        } else if t.contains("dividend") {
            (TipoOp::Dividendo, importe)
        } else if t.contains("interest") {
            (TipoOp::Interes, importe)
        } else if t.contains("fee") || t.contains("commission") {
            (TipoOp::Comision, -importe)
        } else if t.contains("deposit") || t.contains("withdraw") {
            if importe.is_negative() {
                (TipoOp::Retirada, importe.abs())
            } else {
                (TipoOp::Deposito, importe)
            }
        } else {
            *tipos_raros.entry(tipo_ib.clone()).or_default() += 1;
            continue;
        };
        if importe.is_zero() {
            continue;
        }
        let Some(f) = fecha(g(a, "dateTime"))
            .or_else(|| fecha(g(a, "reportDate")))
            .or_else(|| fecha(g(a, "settleDate")))
        else {
            continue;
        };
        let valor = if matches!(tipo, TipoOp::Dividendo | TipoOp::Retencion) {
            let cl = clase(g(a, "assetCategory"), g(a, "subCategory")).unwrap_or(Clase::Accion);
            anotar(a, cl)
        } else {
            None
        };
        let divisa = {
            let c = g(a, "currency").to_uppercase();
            if c.len() == 3 { c } else { base.clone() }
        };
        let (cambio, euros, _) = a_euros_fila(f, &divisa, a, importe);
        e.ops.push(OpLeida {
            fecha: f,
            hora: hora(g(a, "dateTime")),
            tipo,
            valor,
            cantidad: Qty::ZERO,
            precio: None,
            divisa,
            importe,
            comision: Money::ZERO,
            cambio,
            euros,
            comision_eur: Money::ZERO,
            pais: if matches!(tipo, TipoOp::Dividendo | TipoOp::Retencion) {
                pais(a)
            } else {
                String::new()
            },
            nota: format!("{tipo_ib}: {}", g(a, "description")),
            fuente: "ibkr".into(),
            huella: huella(&cuenta, "c", a),
            comision_huella: Vec::new(),
        });
    }

    // Desdoblamientos.
    let mut agrupaciones: BTreeMap<String, Vec<&Attrs>> = BTreeMap::new();
    for (_, a) in st.filas.iter().filter(|(n, _)| n == "CorporateAction") {
        let t = g(a, "type").to_uppercase();
        let q = titulos(g(a, "quantity")).unwrap_or_default();
        if t == "RS" {
            // Agrupación (contrasplit): salen los títulos viejos y entran los nuevos en dos
            // filas de la misma operación; se tratan juntas más abajo.
            let id = match g(a, "actionID") {
                "" => format!("{}|{}", g(a, "reportDate"), g(a, "description")),
                x => x.to_owned(),
            };
            agrupaciones.entry(id).or_default().push(a);
        } else if matches!(t.as_str(), "FS" | "FI" | "SD") && !q.is_zero() {
            let Some(k) = anotar(a, Clase::Accion) else {
                continue;
            };
            let Some(f) = fecha(g(a, "reportDate")).or_else(|| fecha(g(a, "dateTime"))) else {
                continue;
            };
            e.ops.push(OpLeida {
                fecha: f,
                hora: hora(g(a, "dateTime")),
                tipo: TipoOp::Split,
                valor: Some(k),
                cantidad: q,
                precio: None,
                divisa: base.clone(),
                importe: Money::ZERO,
                comision: Money::ZERO,
                cambio: None,
                euros: Money::ZERO,
                comision_eur: Money::ZERO,
                pais: String::new(),
                nota: g(a, "description").to_owned(),
                fuente: "ibkr".into(),
                huella: huella(&cuenta, "ca", a),
                comision_huella: Vec::new(),
            });
        } else {
            avisos.push(format!(
                "Operación societaria «{}» ({t}) del {}: no se importa; revísala y anótala a mano \
                 si cambia los títulos o el coste",
                g(a, "description"),
                g(a, "reportDate")
            ));
        }
    }
    // Agrupaciones: el coste total pasa a los títulos nuevos (el motor lo reparte) y la fracción
    // pagada en efectivo («cash in lieu») es una venta de esa fracción.
    //
    // TODO(VERIFY): sin contrastar con un Flex real con una agrupación y su efectivo. Se ha
    // escrito para que la fracción no se cuente dos veces, la traiga IBKR como la traiga:
    // - en el `proceeds` de la fila RS: se crea la venta de la fracción con ese importe;
    // - como un Trade de venta de menos de un título del mismo valor y día: esa es la venta (no
    //   se crea otra) y se coloca justo después de la agrupación, para que venda títulos nuevos;
    // - como un CashTransaction «cash in lieu» del mismo valor y día: no se importa aparte; su
    //   importe es el de la venta si la fila RS no trae `proceeds`.
    for filas in agrupaciones.values() {
        let a0 = filas[0];
        let claves: BTreeSet<Option<String>> = filas.iter().map(|a| clave(a)).collect();
        let Some(f) = fecha_rs(a0) else {
            continue;
        };
        if claves.len() != 1 || claves.contains(&None) {
            // TODO(VERIFY): con ISIN nuevo habría que pasar el coste del valor viejo al nuevo.
            avisos.push(format!(
                "Agrupación «{}» del {}: el valor cambia de ISIN y no se importa; anota a mano \
                 la salida de los títulos viejos y la entrada de los nuevos con el mismo coste",
                g(a0, "description"),
                g(a0, "reportDate")
            ));
            continue;
        }
        let Some(k) = anotar(a0, Clase::Accion) else {
            continue;
        };
        let qs: Vec<Qty> = filas
            .iter()
            .map(|a| titulos(g(a, "quantity")).unwrap_or_default())
            .collect();
        let salen = qs
            .iter()
            .filter(|q| q.is_negative())
            .fold(Qty::ZERO, |t, q| t.checked_add(q.abs()).unwrap_or(t));
        let entran = qs
            .iter()
            .filter(|q| !q.is_negative())
            .fold(Qty::ZERO, |t, q| t.checked_add(*q).unwrap_or(t));
        // Fracción según la proporción «SPLIT n FOR m» de la descripción.
        let por_proporcion = proporcion_split(g(a0, "description"))
            .and_then(|(n, m)| Some(Qty::from_raw(salen.raw().checked_mul(n)? / m)))
            .and_then(|nuevas| nuevas.checked_sub(entran))
            .filter(|f| !f.is_zero() && !f.is_negative());
        // La hora de la agrupación: la venta de la fracción va después.
        let hora_rs = filas
            .iter()
            .map(|a| hora(g(a, "dateTime")))
            .max()
            .unwrap_or_default();
        // ¿Un Trade de venta de la fracción (menos de un título, mismo valor y día)? Con una
        // proporción que no da decimales exactos (1 por 3), la fracción calculada lleva 18
        // decimales y la del Trade unos pocos: casa si difieren en menos de 0,001 títulos.
        let uno = Qty::from_units(1);
        let tolerancia = Qty::from_raw(Qty::from_units(1).raw() / 1_000);
        let i_trade = (0..fin_trades).find(|&i| {
            let o = &e.ops[i];
            o.tipo == TipoOp::Venta
                && o.fecha == f
                && o.valor.as_deref() == Some(k.as_str())
                && !o.cantidad.is_zero()
                && o.cantidad < uno
                && por_proporcion.is_none_or(|p| {
                    p.checked_sub(o.cantidad)
                        .is_some_and(|d| d.abs() < tolerancia)
                })
        });
        let venta_trade = i_trade.map(|i| {
            fin_trades -= 1;
            let mut o = e.ops.remove(i);
            if o.hora < hora_rs {
                o.hora.clone_from(&hora_rs);
            }
            o
        });
        let efectivo_ct: Vec<&Attrs> = efectivo_rs.remove(&(k.clone(), f)).unwrap_or_default();
        let efectivo_filas: Money = filas
            .iter()
            .filter_map(|a| dinero(g(a, "proceeds")))
            .filter(|m| !m.is_negative())
            .sum();
        // El efectivo, una sola vez: el de la fila RS o, si no lo trae, el del CashTransaction.
        let (efectivo, fila_efectivo) = if efectivo_filas.is_zero() {
            (
                efectivo_ct
                    .iter()
                    .filter_map(|a| dinero(g(a, "amount")))
                    .filter(|m| !m.is_negative())
                    .sum::<Money>(),
                efectivo_ct.first().copied().unwrap_or(a0),
            )
        } else {
            (efectivo_filas, a0)
        };
        let fraccion = if let Some(o) = &venta_trade {
            Some(o.cantidad)
        } else if efectivo.is_zero() {
            None
        } else {
            if por_proporcion.is_none() {
                avisos.push(format!(
                    "Agrupación «{}» del {}: hay {efectivo} en efectivo por las fracciones pero \
                     no se sabe cuántos títulos son; anota a mano la venta de la fracción",
                    g(a0, "description"),
                    g(a0, "reportDate")
                ));
            }
            por_proporcion
        };
        let divisa = {
            let c = g(fila_efectivo, "currency").to_uppercase();
            if c.len() == 3 { c } else { base.clone() }
        };
        let mut fraccion_puesta = false;
        for (a, q) in filas.iter().zip(&qs) {
            let mut q = *q;
            if q.is_zero() {
                continue;
            }
            // Los títulos nuevos, con la fracción (que se vende justo después).
            if let (Some(fr), false, false) = (fraccion, q.is_negative(), fraccion_puesta) {
                q = q.checked_add(fr).unwrap_or(q);
                fraccion_puesta = true;
            }
            e.ops.push(OpLeida {
                fecha: f,
                hora: hora(g(a, "dateTime")),
                tipo: TipoOp::Split,
                valor: Some(k.clone()),
                cantidad: q,
                precio: None,
                divisa: base.clone(),
                importe: Money::ZERO,
                comision: Money::ZERO,
                cambio: None,
                euros: Money::ZERO,
                comision_eur: Money::ZERO,
                pais: String::new(),
                nota: g(a, "description").to_owned(),
                fuente: "ibkr".into(),
                huella: huella(&cuenta, "ca", a),
                comision_huella: Vec::new(),
            });
        }
        if let Some(o) = venta_trade {
            // La venta del Trade, con sus importes, detrás de la agrupación.
            e.ops.push(o);
        } else if let Some(fr) = fraccion.filter(|_| fraccion_puesta) {
            let (cambio, euros, _) = a_euros_fila(f, &divisa, fila_efectivo, efectivo);
            e.ops.push(OpLeida {
                fecha: f,
                hora: hora_rs.clone(),
                tipo: TipoOp::Venta,
                valor: Some(k.clone()),
                cantidad: fr,
                precio: None,
                divisa: divisa.clone(),
                importe: efectivo,
                comision: Money::ZERO,
                cambio,
                euros,
                comision_eur: Money::ZERO,
                pais: String::new(),
                nota: format!("Fracción en efectivo: {}", g(a0, "description")),
                fuente: "ibkr".into(),
                huella: huella(&cuenta, "cil", a0),
                comision_huella: Vec::new(),
            });
        }
    }
    // Efectivo «cash in lieu» de una agrupación que no se ha podido tratar (sin fecha o sin
    // valor): que no se pierda sin aviso.
    for filas in efectivo_rs.values() {
        for a in filas {
            avisos.push(format!(
                "Efectivo por fracciones «{}» del {}: no se ha casado con su agrupación; anota a \
                 mano la venta de la fracción",
                g(a, "description"),
                g(a, "dateTime")
            ));
        }
    }
    let traspasos = st.filas.iter().filter(|(n, _)| n == "Transfer").count();
    if traspasos > 0 {
        avisos.push(format!(
            "{traspasos} traspasos de valores (ACATS, entre cuentas…): no se importan; si vienen \
             de otro bróker tuyo, anota la compra con su coste original"
        ));
    }

    // Posiciones abiertas: precio de cierre y títulos a la fecha del informe.
    let posiciones: Vec<&Attrs> = st
        .filas
        .iter()
        .filter(|(n, _)| n == "OpenPosition")
        .map(|(_, a)| a)
        .collect();
    let hay_resumen = posiciones
        .iter()
        .any(|a| !g(a, "levelOfDetail").eq_ignore_ascii_case("LOT"));
    let mut vistos: BTreeSet<(Date, String)> = BTreeSet::new();
    for a in posiciones {
        if hay_resumen && g(a, "levelOfDetail").eq_ignore_ascii_case("LOT") {
            continue;
        }
        let Some(cl) = clase(g(a, "assetCategory"), g(a, "subCategory")) else {
            continue;
        };
        let Some(k) = anotar(a, cl) else { continue };
        let Some(f) = fecha(g(a, "reportDate")).or(e.hasta) else {
            continue;
        };
        if let Some(q) = titulos(g(a, "position")) {
            e.posiciones.push(PosicionLeida {
                fecha: f,
                valor: k.clone(),
                cantidad: q,
            });
        }
        if let Some(p) = precio(g(a, "markPrice")).filter(|p| *p > Precio::ZERO)
            && vistos.insert((f, k.clone()))
        {
            let divisa = {
                let c = g(a, "currency").to_uppercase();
                if c.len() == 3 { c } else { base.clone() }
            };
            e.precios.push(PrecioLeido {
                fecha: f,
                valor: k,
                precio: p,
                divisa,
            });
        }
    }

    if sin_cambio > 0 {
        avisos.push(format!(
            "{sin_cambio} movimientos sin tipo de cambio a euros (falta «fxRateToBase» o la \
             sección «Currency Conversion Rate»): quedan sin importe en euros"
        ));
    }
    if derivados > 0 {
        avisos.push(format!(
            "{derivados} operaciones con derivados (opciones, futuros, CFD…) no se importan"
        ));
    }
    if divisas > 0 {
        avisos.push(format!(
            "{divisas} cambios de divisa del efectivo no se importan (no son valores)"
        ));
    }
    for (t, n) in tipos_raros {
        avisos.push(format!(
            "{n} movimientos de efectivo «{t}» no se reconocen y no se importan"
        ));
    }
    e.valores = valores.into_values().collect();
    e.avisos = avisos;
    Ok(e)
}

fn es_anulacion(a: &Attrs) -> bool {
    g(a, "transactionType").eq_ignore_ascii_case("TradeCancel") || g(a, "buySell").contains("(Ca.)")
}

#[cfg(test)]
pub(crate) mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use time::macros::date;

    /// Activity Flex sintético (datos inventados) con la forma del de IBKR: compra en USD con
    /// comisión, venta en EUR, una compra anulada, un resumen por orden que no cuenta, un cambio
    /// de divisa, dividendo con retención, intereses, una comisión de datos, un ingreso, un
    /// desdoblamiento y las posiciones abiertas.
    pub(crate) const FLEX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<FlexQueryResponse queryName="TaxPain" type="AF">
<FlexStatements count="1">
<FlexStatement accountId="U0000001" fromDate="20260101" toDate="20261231" period="Year" whenGenerated="20270102;090000">
<AccountInformation accountId="U0000001" currency="EUR" name="EMPRESA DEMO SL" />
<Trades>
<Trade accountId="U0000001" currency="USD" fxRateToBase="0.9" assetCategory="STK" subCategory="COMMON" symbol="ACME" description="ACME CORP" conid="1001" isin="US0378331005" issuerCountryCode="US" tradeID="11" transactionID="21" tradeDate="20260310" dateTime="20260310;153000" quantity="10" tradePrice="100" tradeMoney="1000" proceeds="-1000" taxes="0" ibCommission="-1" ibCommissionCurrency="USD" buySell="BUY" levelOfDetail="EXECUTION" transactionType="ExchTrade" />
<Trade accountId="U0000001" currency="USD" fxRateToBase="0.9" assetCategory="STK" symbol="ACME" isin="US0378331005" tradeID="11" transactionID="21" tradeDate="20260310" quantity="10" tradePrice="100" buySell="BUY" levelOfDetail="ORDER" />
<Trade accountId="U0000001" currency="EUR" fxRateToBase="1" assetCategory="STK" subCategory="ETF" symbol="ETFX" description="ETF EURO DEMO" isin="IE00B4L5Y983" tradeID="12" transactionID="22" tradeDate="20260401" dateTime="20260401;100000" quantity="20" tradePrice="50" tradeMoney="1000" ibCommission="-1.25" ibCommissionCurrency="EUR" buySell="BUY" levelOfDetail="EXECUTION" />
<Trade accountId="U0000001" currency="EUR" fxRateToBase="1" assetCategory="STK" subCategory="ETF" symbol="ETFX" isin="IE00B4L5Y983" tradeID="13" transactionID="23" tradeDate="20260402" quantity="5" tradePrice="51" buySell="BUY" levelOfDetail="EXECUTION" />
<Trade accountId="U0000001" currency="EUR" fxRateToBase="1" assetCategory="STK" subCategory="ETF" symbol="ETFX" isin="IE00B4L5Y983" tradeID="14" transactionID="24" origTradeID="13" tradeDate="20260402" quantity="-5" tradePrice="51" buySell="BUY (Ca.)" transactionType="TradeCancel" levelOfDetail="EXECUTION" />
<Trade accountId="U0000001" currency="EUR" fxRateToBase="1" assetCategory="STK" subCategory="ETF" symbol="ETFX" isin="IE00B4L5Y983" tradeID="15" transactionID="25" tradeDate="20260915" dateTime="20260915;110000" quantity="-8" tradePrice="55" tradeMoney="-440" proceeds="440" ibCommission="-1.25" ibCommissionCurrency="EUR" buySell="SELL" levelOfDetail="EXECUTION" />
<Trade accountId="U0000001" currency="USD" fxRateToBase="0.9" assetCategory="CASH" symbol="EUR.USD" tradeID="16" transactionID="26" tradeDate="20260309" quantity="1000" tradePrice="1.1" buySell="BUY" levelOfDetail="EXECUTION" />
</Trades>
<CashTransactions>
<CashTransaction accountId="U0000001" currency="USD" fxRateToBase="0.92" assetCategory="STK" symbol="ACME" isin="US0378331005" issuerCountryCode="US" dateTime="20260615" amount="20" type="Dividends" transactionID="31" description="ACME CASH DIVIDEND USD 2 PER SHARE" levelOfDetail="DETAIL" />
<CashTransaction accountId="U0000001" currency="USD" fxRateToBase="0.92" assetCategory="STK" symbol="ACME" isin="US0378331005" issuerCountryCode="US" dateTime="20260615" amount="-3" type="Withholding Tax" transactionID="32" description="ACME US TAX" levelOfDetail="DETAIL" />
<CashTransaction accountId="U0000001" currency="EUR" fxRateToBase="1" dateTime="20260705" amount="4.5" type="Broker Interest Received" transactionID="33" description="EUR CREDIT INT" />
<CashTransaction accountId="U0000001" currency="EUR" fxRateToBase="1" dateTime="20260705" amount="-1.5" type="Other Fees" transactionID="34" description="MARKET DATA" />
<CashTransaction accountId="U0000001" currency="EUR" fxRateToBase="1" dateTime="20260105" amount="5000" type="Deposits &amp; Withdrawals" transactionID="35" description="CASH RECEIPTS" />
<CashTransaction accountId="U0000001" currency="EUR" fxRateToBase="1" dateTime="20260105" amount="5000" type="Deposits &amp; Withdrawals" levelOfDetail="SUMMARY" />
</CashTransactions>
<CorporateActions>
<CorporateAction accountId="U0000001" currency="USD" assetCategory="STK" symbol="ACME" isin="US0378331005" reportDate="20260801" dateTime="20260801;202500" quantity="10" type="FS" transactionID="41" description="ACME SPLIT 2 FOR 1" />
</CorporateActions>
<OpenPositions>
<OpenPosition accountId="U0000001" currency="USD" fxRateToBase="0.95" assetCategory="STK" symbol="ACME" isin="US0378331005" reportDate="20261231" position="20" markPrice="60" positionValue="1200" levelOfDetail="SUMMARY" />
<OpenPosition accountId="U0000001" currency="EUR" fxRateToBase="1" assetCategory="STK" subCategory="ETF" symbol="ETFX" isin="IE00B4L5Y983" reportDate="20261231" position="12" markPrice="56" positionValue="672" levelOfDetail="SUMMARY" />
<OpenPosition accountId="U0000001" currency="EUR" assetCategory="STK" symbol="ETFX" isin="IE00B4L5Y983" reportDate="20261231" position="12" markPrice="56" levelOfDetail="LOT" />
</OpenPositions>
<ConversionRates>
<ConversionRate reportDate="20261231" fromCurrency="USD" toCurrency="EUR" rate="0.95" />
</ConversionRates>
</FlexStatement>
</FlexStatements>
</FlexQueryResponse>"#;

    #[test]
    fn lee_el_flex_xml() {
        assert!(es_flex_xml(FLEX));
        let ex = leer_xml(FLEX).unwrap();
        assert_eq!(ex.len(), 1);
        let e = &ex[0];
        assert_eq!(e.cuenta, "U0000001");
        assert_eq!(e.divisa, "EUR");
        assert_eq!(e.desde, Some(date!(2026 - 01 - 01)));
        let n = |t: TipoOp| e.ops.iter().filter(|o| o.tipo == t).count();
        // La compra anulada, el resumen por orden y el cambio de divisa no cuentan.
        assert_eq!(n(TipoOp::Compra), 2);
        assert_eq!(n(TipoOp::Venta), 1);
        assert_eq!(n(TipoOp::Dividendo), 1);
        assert_eq!(n(TipoOp::Retencion), 1);
        assert_eq!(n(TipoOp::Interes), 1);
        assert_eq!(n(TipoOp::Comision), 1);
        assert_eq!(n(TipoOp::Deposito), 1);
        assert_eq!(n(TipoOp::Split), 1);
        // Compra en USD: 1.000 $ y 1 $ de comisión al 0,9.
        let c = e
            .ops
            .iter()
            .find(|o| o.tipo == TipoOp::Compra && o.divisa == "USD")
            .unwrap();
        assert_eq!(c.valor.as_deref(), Some("US0378331005"));
        assert_eq!(c.importe, Money::from_euros(1_000, 0));
        assert_eq!(c.euros, Money::from_euros(900, 0));
        assert_eq!(c.comision_eur, Money::from_cents(90));
        assert_eq!(c.hora, "15:30:00");
        assert_eq!(c.huella, "ibkr:U0000001:t:21");
        // Retención en positivo y con su país.
        let r = e.ops.iter().find(|o| o.tipo == TipoOp::Retencion).unwrap();
        assert_eq!(r.euros, Money::from_cents(276));
        assert_eq!(r.pais, "US");
        let com = e.ops.iter().find(|o| o.tipo == TipoOp::Comision).unwrap();
        assert_eq!(com.euros, Money::from_cents(150));
        // Precios, posiciones (sin los lotes) y cambio de fin de año.
        assert_eq!(e.precios.len(), 2);
        assert_eq!(e.posiciones.len(), 2);
        assert_eq!(e.cambios.len(), 1);
        let etf = e.valores.iter().find(|v| v.ticker == "ETFX").unwrap();
        assert_eq!(etf.clase, Clase::Etf);
        assert_eq!(etf.pais, "IE");
        assert!(
            e.avisos.iter().any(|a| a.contains("cambios de divisa")),
            "{:?}",
            e.avisos
        );
    }

    #[test]
    fn lee_el_flex_csv_y_el_error_del_servicio() {
        let csv = "\"ClientAccountID\",\"CurrencyPrimary\",\"AssetClass\",\"Symbol\",\"ISIN\",\"TradeDate\",\"Date/Time\",\"Buy/Sell\",\"Quantity\",\"TradePrice\",\"IBCommission\",\"IBCommissionCurrency\",\"FXRateToBase\",\"TransactionID\"\n\
\"U0000002\",\"EUR\",\"STK\",\"ETFX\",\"IE00B4L5Y983\",\"20260401\",\"20260401;100000\",\"BUY\",\"20\",\"50\",\"-1.25\",\"EUR\",\"1\",\"77\"\n\
\n\
\"ClientAccountID\",\"CurrencyPrimary\",\"Type\",\"Amount\",\"Date/Time\",\"FXRateToBase\",\"TransactionID\",\"Description\"\n\
\"U0000002\",\"EUR\",\"Broker Interest Received\",\"2.5\",\"20260705\",\"1\",\"78\",\"EUR CREDIT INT\"\n";
        assert!(es_flex_csv(csv));
        let ex = leer_csv(csv).unwrap();
        assert_eq!(ex[0].cuenta, "U0000002");
        assert_eq!(ex[0].ops.len(), 2);
        assert_eq!(ex[0].ops[0].euros, Money::from_euros(1_000, 0));
        assert_eq!(ex[0].ops[0].comision_eur, Money::from_cents(125));
        let err = leer_xml(
            "<FlexStatementResponse timestamp=\"x\"><Status>Fail</Status><ErrorCode>1012</ErrorCode><ErrorMessage>Token has expired.</ErrorMessage></FlexStatementResponse>",
        )
        .unwrap_err();
        assert!(err.contains("1012"), "{err}");
    }
}
