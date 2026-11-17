//! Importes y fechas tal como vienen en los extractos de cualquier banco y país.

use std::sync::LazyLock;

use regex::Regex;
use taxpain_core::money::Money;
use time::{Date, Month};

/// Códigos de moneda que pueden acompañar a un importe.
const MONEDAS: [&str; 17] = [
    "EUR", "USD", "GBP", "CHF", "JPY", "CAD", "AUD", "SEK", "NOK", "DKK", "PLN", "CZK", "MXN",
    "EURO", "EUROS", "US", "USDOLLAR",
];

/// `t` sin el sufijo `m` (sin distinguir mayúsculas), si lo tiene y queda algo delante.
fn quitar_sufijo(t: &str, m: &str) -> Option<String> {
    let n = t.len().checked_sub(m.len())?;
    if n == 0 || !t.is_char_boundary(n) || !t[n..].eq_ignore_ascii_case(m) {
        return None;
    }
    Some(t[..n].trim_end().to_owned())
}

/// `t` sin el prefijo `m` (sin distinguir mayúsculas), si lo tiene y queda algo detrás.
fn quitar_prefijo(t: &str, m: &str) -> Option<String> {
    if t.len() <= m.len() || !t.is_char_boundary(m.len()) || !t[..m.len()].eq_ignore_ascii_case(m) {
        return None;
    }
    Some(t[m.len()..].trim_start().to_owned())
}

/// Quita los símbolos y códigos de moneda y las marcas de debe y haber de detrás. Devuelve el
/// resto y el signo que imponen las marcas (`Some(true)` = negativo).
fn quitar_marcas(s: &str) -> (String, Option<bool>) {
    let limpio = s.replace(['€', '$', '£', '¥'], " ");
    let mut t = limpio
        .split_whitespace()
        .filter(|w| !MONEDAS.iter().any(|m| w.eq_ignore_ascii_case(m)))
        .collect::<Vec<_>>()
        .join(" ");
    // Códigos pegados al número («12,00EUR», «EUR12,00»).
    for m in MONEDAS {
        if let Some(r) = quitar_sufijo(&t, m) {
            t = r;
        }
        if let Some(r) = quitar_prefijo(&t, m)
            && r.starts_with(|c: char| c.is_ascii_digit() || c == '-' || c == '+')
        {
            t = r;
        }
    }
    // Marcas de debe (cargo) y haber (abono) detrás del número.
    for (marca, neg) in [
        ("DR", true),
        ("DB", true),
        ("CR", false),
        ("D", true),
        ("H", false),
    ] {
        if let Some(r) = quitar_sufijo(&t, marca)
            && r.ends_with(|c: char| c.is_ascii_digit() || c == ')' || c == '-')
        {
            return (r, Some(neg));
        }
    }
    (t, None)
}

/// Importe en cualquiera de las formas de los extractos:
/// - `1.234,56`, `1,234.56`, `1234.56`, `1 234,56`, `1'234.56`;
/// - `-1.234,56`, `1.234,56-`, `(1.234,56)`, `+12,00`, con el menos tipográfico (`−`);
/// - `1.234,56 D` (debe: cargo), `1.234,56 H` (haber: abono), `12.00 DR`, `12.00 CR`;
/// - con o sin símbolo o código de moneda (`€`, `EUR`, `$`, `USD`…).
///
/// Con un solo separador seguido de tres cifras (`1.234`, `1,234`) se toma como de miles.
/// `None` si no es un importe (vacío, texto, varios números…).
pub(crate) fn importe(s: &str) -> Option<Money> {
    let s = s.replace(['\u{2212}', '\u{2013}'], "-");
    let (t, marca) = quitar_marcas(&s);
    let mut t: String = t
        .chars()
        .filter(|c| !matches!(c, ' ' | '\u{a0}' | '\u{202f}' | '\'' | '’'))
        .collect();
    let mut neg = false;
    if t.starts_with('(') && t.ends_with(')') && t.len() > 2 {
        neg = true;
        t = t[1..t.len() - 1].to_owned();
    }
    if let Some(r) = t.strip_prefix('-') {
        neg = !neg;
        t = r.to_owned();
    } else if let Some(r) = t.strip_prefix('+') {
        t = r.to_owned();
    }
    if let Some(r) = t.strip_suffix('-') {
        neg = !neg;
        t = r.to_owned();
    } else if let Some(r) = t.strip_suffix('+') {
        t = r.to_owned();
    }
    if t.is_empty() || !t.starts_with(|c: char| c.is_ascii_digit() || c == '.' || c == ',') {
        return None;
    }
    if !t
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == ',')
    {
        return None;
    }
    let (entero, decimales) = partir(&t)?;
    let entero: i64 = if entero.is_empty() {
        0
    } else {
        entero.parse().ok()?
    };
    // Excel guarda a veces 12.300000000000001: se redondea al céntimo, sin coma flotante.
    let mut cent = format!("{:0<2}", decimales.get(..2).unwrap_or(&decimales))
        .parse::<i64>()
        .ok()?;
    if decimales.as_bytes().get(2).is_some_and(|b| *b >= b'5') {
        cent += 1;
    }
    let c = entero.checked_mul(100)?.checked_add(cent)?;
    // Con signo y marca a la vez («-12,00 D»), basta con que uno diga que es un cargo.
    let neg = marca.is_some_and(|m| m) || neg;
    Some(Money::from_cents(if neg { -c } else { c }))
}

/// Parte entera y decimales de un número con `.` y `,` mezclados como separadores.
fn partir(t: &str) -> Option<(String, String)> {
    let puntos = t.matches('.').count();
    let comas = t.matches(',').count();
    let (decimal, miles) = match (puntos, comas) {
        (0, 0) => return Some((t.to_owned(), String::new())),
        (_, 0) | (0, _) => {
            let sep = if puntos > 0 { '.' } else { ',' };
            let n = puntos + comas;
            let (ent, dec) = t.rsplit_once(sep)?;
            // `1.234.567` o `1,234`: separadores de miles.
            if n > 1 || (dec.len() == 3 && !ent.is_empty() && ent != "0" && ent.len() <= 3) {
                if !grupos_de_miles(t, sep) {
                    return None;
                }
                return Some((t.replace(sep, ""), String::new()));
            }
            return Some((ent.to_owned(), dec.to_owned()));
        }
        _ => {
            // Los dos: el último es el decimal.
            let ultimo_punto = t.rfind('.')?;
            let ultima_coma = t.rfind(',')?;
            if ultimo_punto > ultima_coma {
                ('.', ',')
            } else {
                (',', '.')
            }
        }
    };
    let (ent, dec) = t.rsplit_once(decimal)?;
    if ent.contains(decimal) || !grupos_de_miles(ent, miles) {
        return None;
    }
    Some((ent.replace(miles, ""), dec.to_owned()))
}

/// ¿Los grupos separados por `sep` son de miles (el primero de 1 a 3 cifras y el resto de 3)?
fn grupos_de_miles(t: &str, sep: char) -> bool {
    let mut partes = t.split(sep);
    let Some(primero) = partes.next() else {
        return false;
    };
    if !t.contains(sep) {
        return true;
    }
    (1..=3).contains(&primero.len()) && partes.all(|p| p.len() == 3)
}

/// Un importe encontrado dentro de una línea de texto (de un PDF, por ejemplo).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Token {
    /// Posición en bytes en la línea.
    pub inicio: usize,
    pub fin: usize,
    pub importe: Money,
    /// ¿Lleva signo o marca de debe/haber? Si no, el signo hay que deducirlo (de la columna o
    /// del saldo).
    pub con_signo: bool,
}

/// Importes con dos decimales que hay en una línea, de izquierda a derecha. Pensado para las
/// líneas de un extracto en PDF, donde las columnas llegan separadas por espacios: por eso aquí
/// el espacio normal no se acepta como separador de miles (sí el de no separación).
pub(crate) fn importes_en(linea: &str) -> Vec<Token> {
    static RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(
            r"(?x)
            (?P<pre>[-−+(]\s?)?
            (?:[€$£]\s?)?
            (?P<num>\d{1,3}(?:[.,\u{a0}\u{202f}']\d{3})+[.,]\d{2}|\d+[.,]\d{2})
            (?P<post>\)|-|−)?
            (?:\s?(?:€|EUR|USD|GBP|\$))?
            (?P<dh>\s?(?:DR|CR|D|H)\b)?",
        )
        .ok()
    });
    let Some(re) = RE.as_ref() else {
        return Vec::new();
    };
    let bytes = linea.as_bytes();
    let mut out = Vec::new();
    for c in re.captures_iter(linea) {
        let (Some(todo), Some(num)) = (c.get(0), c.name("num")) else {
            continue;
        };
        // Que no sea un trozo de otro número ni de una fecha o un código (`12/05/2025`,
        // `ES12 3456`, `1.234,567`).
        let antes = num.start().checked_sub(1).and_then(|i| bytes.get(i));
        let despues = bytes.get(num.end());
        if antes.is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b','))
            || despues.is_some_and(|b| b.is_ascii_digit() || matches!(b, b'/' | b'%'))
        {
            continue;
        }
        let Some(importe) = importe(todo.as_str()) else {
            continue;
        };
        let con_signo =
            c.name("pre").is_some() || c.name("post").is_some() || c.name("dh").is_some();
        out.push(Token {
            inicio: todo.start(),
            fin: todo.end(),
            importe,
            con_signo,
        });
    }
    out
}

/// Orden de día y mes en las fechas numéricas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Orden {
    /// `dd/mm/aaaa` (España, casi toda Europa).
    #[default]
    DiaMes,
    /// `mm/dd/aaaa` (Estados Unidos).
    MesDia,
}

/// Deduce el orden de día y mes de un lote de fechas numéricas del mismo extracto: si alguna
/// tiene el primer número mayor que 12, es día/mes; si es el segundo, mes/día. Si no se puede
/// saber, día/mes.
pub(crate) fn inferir_orden<'a>(muestras: impl IntoIterator<Item = &'a str>) -> Orden {
    let mut mes_dia = false;
    for m in muestras {
        let partes: Vec<&str> = m.trim().split(['/', '-', '.']).map(str::trim).collect();
        let (Some(a), Some(b)) = (partes.first(), partes.get(1)) else {
            continue;
        };
        if a.len() == 4 {
            continue;
        }
        let (Ok(a), Ok(b)) = (a.parse::<u32>(), b.parse::<u32>()) else {
            continue;
        };
        if a > 12 && b <= 12 {
            return Orden::DiaMes;
        }
        if b > 12 && a <= 12 {
            mes_dia = true;
        }
    }
    if mes_dia {
        Orden::MesDia
    } else {
        Orden::DiaMes
    }
}

/// Mes por su nombre o abreviatura en español, catalán, gallego, euskera, inglés, francés,
/// alemán, italiano, portugués o neerlandés (sin tildes y en minúsculas).
pub(crate) fn mes(nombre: &str) -> Option<u8> {
    const MESES: [(&[&str], u8); 12] = [
        (
            &[
                "enero",
                "ene",
                "gener",
                "gen",
                "xaneiro",
                "xan",
                "urtarrila",
                "january",
                "jan",
                "janvier",
                "janv",
                "januar",
                "janner",
                "gennaio",
                "janeiro",
                "januari",
            ],
            1,
        ),
        (
            &[
                "febrero",
                "feb",
                "febrer",
                "febr",
                "febreiro",
                "otsaila",
                "february",
                "fevrier",
                "fevr",
                "fev",
                "februar",
                "febbraio",
                "fevereiro",
                "februari",
            ],
            2,
        ),
        (
            &[
                "marzo", "mar", "marc", "martxoa", "march", "mars", "marz", "mrz", "marco",
                "maart", "mrt",
            ],
            3,
        ),
        (
            &[
                "abril", "abr", "apirila", "april", "apr", "avril", "avr", "aprile",
            ],
            4,
        ),
        (
            &[
                "mayo", "may", "maig", "maio", "maiatza", "mai", "maggio", "mag", "mei",
            ],
            5,
        ),
        (
            &[
                "junio", "jun", "juny", "xuno", "xun", "ekaina", "june", "juin", "juni", "giugno",
                "giu", "junho",
            ],
            6,
        ),
        (
            &[
                "julio", "jul", "juliol", "xullo", "xul", "uztaila", "july", "juillet", "juil",
                "juli", "luglio", "lug", "julho",
            ],
            7,
        ),
        (
            &[
                "agosto", "ago", "agost", "abuztua", "august", "aug", "aout", "augustus",
            ],
            8,
        ),
        (
            &[
                "septiembre",
                "setiembre",
                "sep",
                "sept",
                "set",
                "setembre",
                "setembro",
                "iraila",
                "september",
                "septembre",
                "settembre",
            ],
            9,
        ),
        (
            &[
                "octubre", "oct", "outubro", "out", "urria", "october", "octobre", "oktober",
                "okt", "ottobre", "ott",
            ],
            10,
        ),
        (
            &[
                "noviembre",
                "nov",
                "novembre",
                "novembro",
                "azaroa",
                "november",
            ],
            11,
        ),
        (
            &[
                "diciembre",
                "dic",
                "desembre",
                "des",
                "decembro",
                "dec",
                "abendua",
                "december",
                "decembre",
                "dezember",
                "dez",
                "dicembre",
                "dezembro",
            ],
            12,
        ),
    ];
    let n = sin_tildes(nombre).trim_end_matches('.').to_owned();
    MESES
        .iter()
        .find(|(nombres, _)| nombres.contains(&n.as_str()))
        .map(|(_, m)| *m)
}

/// Minúsculas y sin tildes ni diéresis (también `ç` → `c` y `ñ` → `n`).
pub(crate) fn sin_tildes(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ä' | 'ã' | 'Á' | 'À' | 'Â' | 'Ä' | 'Ã' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
            'ó' | 'ò' | 'ô' | 'ö' | 'õ' | 'Ó' | 'Ò' | 'Ô' | 'Ö' | 'Õ' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            'ñ' | 'Ñ' => 'n',
            c => c.to_ascii_lowercase(),
        })
        .collect()
}

/// ¿Cumple el número la prueba de Luhn (la de los números de tarjeta)?
pub(crate) fn luhn(s: &str) -> bool {
    let mut suma = 0;
    for (i, c) in s.chars().rev().enumerate() {
        let Some(mut d) = c.to_digit(10) else {
            return false;
        };
        if i % 2 == 1 {
            d *= 2;
            if d > 9 {
                d -= 9;
            }
        }
        suma += d;
    }
    suma % 10 == 0
}

fn año_completo(a: &str) -> Option<i32> {
    let y: i32 = a.parse().ok()?;
    match a.len() {
        4 => Some(y),
        // Los extractos con año de dos cifras son de este siglo o del final del anterior.
        2 => Some(if y >= 70 { 1900 + y } else { 2000 + y }),
        _ => None,
    }
}

fn dia(y: i32, m: u8, d: u8) -> Option<Date> {
    Date::from_calendar_date(y, Month::try_from(m).ok()?, d).ok()
}

/// Fecha de un extracto en cualquiera de las formas habituales:
/// - `dd/mm/aaaa`, `d/m/aa`, `dd-mm-aaaa`, `dd.mm.aaaa` (o mes/día, según `orden`);
/// - `aaaa-mm-dd`, `aaaa/mm/dd`, `aaaammdd`, ISO con hora (`2025-01-31T10:00:00`);
/// - número de serie de Excel;
/// - con el mes en letra: `1 Jan 2025`, `01-ENE-25`, `1 de enero de 2025`, `Jan 1, 2025`,
///   `15 juil. 2025`…;
/// - sin año (`dd/mm`, `15 ene`), con el `año` que se le dé.
///
/// La hora, si viene detrás, se ignora.
pub(crate) fn fecha(s: &str, año: Option<i32>, orden: Orden) -> Option<Date> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // La hora de detrás: «2025-01-31T10:00:00», «31/01/2025 10:00», «31/01/2025 10:00:00».
    let s = match s.get(..10) {
        Some(iso)
            if s.as_bytes().get(10) == Some(&b'T')
                && iso.bytes().take(4).all(|b| b.is_ascii_digit()) =>
        {
            iso
        }
        _ => s,
    };
    let s = match s.rsplit_once(' ') {
        Some((a, b)) if b.contains(':') && b.chars().all(|c| c.is_ascii_digit() || c == ':') => a,
        _ => s,
    };
    let s = s.trim();
    // Número de serie de Excel o `aaaammdd`.
    if s.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        let ent = s.split('.').next().unwrap_or(s);
        if ent.len() == 8 && !s.contains('.') {
            let y = año_completo(&ent[..4])?;
            return dia(y, ent[4..6].parse().ok()?, ent[6..8].parse().ok()?);
        }
        if (!s.contains('.') || (s.matches('.').count() == 1 && ent.len() == 5))
            && let Ok(serie) = ent.parse::<i64>()
            && (20_000..200_000).contains(&serie)
        {
            return crate::import::parse_date(ent);
        }
    }
    let partes: Vec<&str> = s
        .split(|c: char| c == '/' || c == '-' || c == '.' || c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if partes.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit())) {
        return match partes.as_slice() {
            [a, b, c] if a.len() == 4 => dia(año_completo(a)?, b.parse().ok()?, c.parse().ok()?),
            [a, b, c] if c.len() == 4 || c.len() == 2 => {
                let (d, m) = match orden {
                    Orden::DiaMes => (a, b),
                    Orden::MesDia => (b, a),
                };
                if d.len() > 2 || m.len() > 2 {
                    return None;
                }
                dia(año_completo(c)?, m.parse().ok()?, d.parse().ok()?)
            }
            [a, b] if a.len() <= 2 && b.len() <= 2 => {
                let (d, m) = match orden {
                    Orden::DiaMes => (a, b),
                    Orden::MesDia => (b, a),
                };
                dia(año?, m.parse().ok()?, d.parse().ok()?)
            }
            _ => None,
        };
    }
    // Con el mes en letra.
    let palabras: Vec<String> = partes
        .iter()
        .map(|p| sin_tildes(p))
        .filter(|p| !matches!(p.as_str(), "de" | "del" | "d'" | "of" | "le" | "el"))
        .collect();
    let mut m = None;
    let mut numeros: Vec<&str> = Vec::new();
    for p in &palabras {
        if p.bytes().all(|b| b.is_ascii_digit()) {
            numeros.push(p);
        } else if let Some(x) = mes(p.trim_start_matches("d'")) {
            if m.is_some() {
                return None;
            }
            m = Some(x);
        } else {
            // «1st», «2nd», «3rd», «4th»: el día en inglés. Otra palabra («lunes», «Saldo»)
            // quiere decir que no es una fecha.
            let d = p.trim_end_matches(|c: char| c.is_ascii_alphabetic());
            let sufijo = &p[d.len()..];
            if d.is_empty()
                || !d.bytes().all(|b| b.is_ascii_digit())
                || !["st", "nd", "rd", "th"].contains(&sufijo)
            {
                return None;
            }
            numeros.push(d);
        }
    }
    let m = m?;
    let (d, y) = match numeros.as_slice() {
        [d] if d.len() <= 2 => (d.parse().ok()?, año?),
        [d, y] if d.len() <= 2 => (d.parse().ok()?, año_completo(y)?),
        [y, d] if y.len() == 4 && d.len() <= 2 => (d.parse().ok()?, año_completo(y)?),
        _ => return None,
    };
    dia(y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn e(s: &str) -> Option<i64> {
        importe(s).map(Money::cents)
    }

    #[test]
    fn importes_en_todas_sus_formas() {
        assert_eq!(e("1.234,56"), Some(123_456));
        assert_eq!(e("1,234.56"), Some(123_456));
        assert_eq!(e("1234.56"), Some(123_456));
        assert_eq!(e("1234,5"), Some(123_450));
        assert_eq!(e("1 234,56"), Some(123_456));
        assert_eq!(e("1\u{a0}234,56 €"), Some(123_456));
        assert_eq!(e("1'234.56"), Some(123_456));
        assert_eq!(e("-1.234,56"), Some(-123_456));
        assert_eq!(e("−12,00"), Some(-1200));
        assert_eq!(e("1.234,56-"), Some(-123_456));
        assert_eq!(e("(1.234,56)"), Some(-123_456));
        assert_eq!(e("+12,00"), Some(1200));
        assert_eq!(e("12,00 D"), Some(-1200));
        assert_eq!(e("12,00 H"), Some(1200));
        assert_eq!(e("12.00 DR"), Some(-1200));
        assert_eq!(e("12.00CR"), Some(1200));
        assert_eq!(e("EUR 12.00"), Some(1200));
        assert_eq!(e("-$1,200.50"), Some(-120_050));
        assert_eq!(e("12,00 EUR"), Some(1200));
        assert_eq!(e("USD -5.10"), Some(-510));
        assert_eq!(e("1.234"), Some(123_400));
        assert_eq!(e("1,234"), Some(123_400));
        assert_eq!(e("1.234.567"), Some(123_456_700));
        assert_eq!(e("0.125"), Some(13));
        assert_eq!(e("12.300000000000001"), Some(1230));
        assert_eq!(e("-0.5"), Some(-50));
        assert_eq!(e("7"), Some(700));
    }

    #[test]
    fn lo_que_no_es_un_importe() {
        for s in [
            "",
            "-",
            "abc",
            "12/05/2025",
            "1.2.3,4",
            "ES12",
            "12,34,56",
            "1,23.45",
        ] {
            assert_eq!(importe(s), None, "{s}");
        }
    }

    #[test]
    fn importes_dentro_de_una_linea() {
        let l = "02/01/2025 03/01/2025 RECIBO LUZ 1.234,56- 10.000,00";
        let t = importes_en(l);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].importe.cents(), -123_456);
        assert!(t[0].con_signo);
        assert_eq!(t[1].importe.cents(), 1_000_000);
        assert!(!t[1].con_signo);

        let t = importes_en("Card payment Amazon €12.50 €1,000.00");
        assert_eq!(
            t.iter().map(|t| t.importe.cents()).collect::<Vec<_>>(),
            [1250, 100_000]
        );

        // Ni fechas, ni porcentajes, ni trozos de IBAN o de referencias.
        assert!(importes_en("ES91 2100 0418 4502 0005 1332 al 21,00% ref 12.345678").is_empty());
        assert_eq!(
            importes_en("TRANSF 12,00 H")
                .first()
                .map(|t| t.importe.cents()),
            Some(1200)
        );
    }

    #[test]
    fn fechas_en_todas_sus_formas() {
        let f = |s: &str| fecha(s, Some(2025), Orden::DiaMes);
        assert_eq!(f("31/01/2025"), Some(date!(2025 - 01 - 31)));
        assert_eq!(f("1/2/25"), Some(date!(2025 - 02 - 01)));
        assert_eq!(f("31-01-2025"), Some(date!(2025 - 01 - 31)));
        assert_eq!(f("31.01.2025"), Some(date!(2025 - 01 - 31)));
        assert_eq!(f("2025-01-31"), Some(date!(2025 - 01 - 31)));
        assert_eq!(f("2025/01/31"), Some(date!(2025 - 01 - 31)));
        assert_eq!(f("20250131"), Some(date!(2025 - 01 - 31)));
        assert_eq!(f("2025-01-31T10:00:00"), Some(date!(2025 - 01 - 31)));
        assert_eq!(f("31/01/2025 10:15"), Some(date!(2025 - 01 - 31)));
        assert_eq!(f("45688"), Some(date!(2025 - 01 - 31)));
        assert_eq!(f("1 Jan 2025"), Some(date!(2025 - 01 - 01)));
        assert_eq!(f("01-ENE-25"), Some(date!(2025 - 01 - 01)));
        assert_eq!(f("1 de enero de 2025"), Some(date!(2025 - 01 - 01)));
        assert_eq!(f("Jan 1, 2025"), Some(date!(2025 - 01 - 01)));
        assert_eq!(f("15 juil. 2025"), Some(date!(2025 - 07 - 15)));
        assert_eq!(f("3 de març de 2025"), Some(date!(2025 - 03 - 03)));
        assert_eq!(f("12 Dez 2024"), Some(date!(2024 - 12 - 12)));
        assert_eq!(f("December 2nd, 2024"), Some(date!(2024 - 12 - 02)));
        assert_eq!(f("31/01"), Some(date!(2025 - 01 - 31)));
        assert_eq!(f("15 ene"), Some(date!(2025 - 01 - 15)));
        assert_eq!(
            fecha("01/31/2025", None, Orden::MesDia),
            Some(date!(2025 - 01 - 31))
        );
    }

    #[test]
    fn lo_que_no_es_una_fecha() {
        let f = |s: &str| fecha(s, None, Orden::DiaMes);
        for s in [
            "",
            "31/02/2025",
            "Saldo anterior",
            "1.234,56",
            "lunes 3 marzo",
            "15 ene",
        ] {
            assert_eq!(f(s), None, "{s}");
        }
    }

    #[test]
    fn orden_de_dia_y_mes() {
        assert_eq!(inferir_orden(["01/02/2025", "13/02/2025"]), Orden::DiaMes);
        assert_eq!(inferir_orden(["01/02/2025", "02/13/2025"]), Orden::MesDia);
        assert_eq!(inferir_orden(["01/02/2025"]), Orden::DiaMes);
        assert_eq!(inferir_orden(["2025-02-13"]), Orden::DiaMes);
    }

    #[test]
    fn meses_en_varios_idiomas() {
        assert_eq!(mes("Sept."), Some(9));
        assert_eq!(mes("AGO"), Some(8));
        assert_eq!(mes("Août"), Some(8));
        assert_eq!(mes("Mrz"), Some(3));
        assert_eq!(mes("xuño"), Some(6));
        assert_eq!(mes("saldo"), None);
    }
}
