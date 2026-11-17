//! Los datos de la cuenta que hay en cualquier parte de las páginas (a menudo después de los
//! movimientos, en el orden en que salen del PDF): IBAN o tarjeta, titular, divisa, periodo,
//! saldos inicial y final y banco.

use std::sync::LazyLock;

use regex::Regex;
use time::Date;

use super::maqueta::Linea;
use super::piezas::{self, Cifra};
use super::tabla::{self, ClaseSaldo};
use crate::extractos::numeros::{self, Orden, luhn, sin_tildes};
use crate::extractos::{Clase, iban};

/// Lo que se sabe de la cuenta del extracto.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Datos {
    pub iban: Option<String>,
    /// Cuenta enmascarada o tarjeta: la clave ya hecha.
    pub cuenta: Option<String>,
    /// Tarjeta: la entidad (si se sabe) y sus últimos dígitos.
    pub tarjeta: Option<(Option<String>, String)>,
    pub clase: Option<Clase>,
    pub banco: Option<String>,
    pub titular: Option<String>,
    pub divisa: Option<&'static str>,
    pub desde: Option<Date>,
    pub hasta: Option<Date>,
    pub saldo_inicial: Option<Cifra>,
    pub saldo_final: Option<Cifra>,
    /// La fecha más reciente que aparece fuera de la tabla (la de generación, normalmente),
    /// para saber el año de las fechas que no lo llevan.
    pub referencia: Option<Date>,
}

/// Bancos y monederos que se reconocen por su nombre en el texto (además de los del registro
/// de entidades, por el código del IBAN). El código de entidad, si es un banco español.
const NOMBRES: [(&str, &str, Option<&str>); 34] = [
    ("banco bilbao vizcaya", "BBVA", Some("0182")),
    ("bbva", "BBVA", Some("0182")),
    ("banca march", "Banca March", Some("0061")),
    ("bancamarch", "Banca March", Some("0061")),
    ("bankinter", "Bankinter", Some("0128")),
    ("caixabank", "CaixaBank", Some("2100")),
    ("banco santander", "Banco Santander", Some("0049")),
    ("banco sabadell", "Banco Sabadell", Some("0081")),
    ("openbank", "Openbank", Some("0073")),
    ("ing bank", "ING", Some("1465")),
    ("unicaja", "Unicaja", Some("2103")),
    ("kutxabank", "Kutxabank", Some("2095")),
    ("ibercaja", "Ibercaja", Some("2085")),
    ("abanca", "Abanca", Some("2080")),
    ("cajamar", "Cajamar", Some("3058")),
    ("caixa d'enginyers", "Caixa d'Enginyers", Some("3025")),
    ("laboral kutxa", "Laboral Kutxa", Some("3035")),
    ("triodos", "Triodos Bank", Some("1491")),
    ("mediolanum", "Banco Mediolanum", Some("0186")),
    ("evo banco", "EVO Banco", Some("0239")),
    ("deutsche bank", "Deutsche Bank", Some("0019")),
    ("revolut", "Revolut", None),
    ("paypal", "PayPal", None),
    ("stripe payments", "Stripe", None),
    ("wise payments", "Wise", None),
    ("n26", "N26", None),
    ("qonto", "Qonto", None),
    ("commerzbank", "Commerzbank", None),
    ("sparkasse", "Sparkasse", None),
    ("bnp paribas", "BNP Paribas", None),
    ("societe generale", "Société Générale", None),
    ("credit agricole", "Crédit Agricole", None),
    ("hsbc", "HSBC", None),
    ("barclays", "Barclays", None),
];

/// Monederos y pasarelas (no son cuentas de un banco).
const MONEDEROS: [&str; 2] = ["PayPal", "Stripe"];

fn re(p: &str) -> Option<Regex> {
    Regex::new(p).ok()
}

/// Los datos de la cuenta de unas líneas. `fuera` dice qué líneas no son de la tabla de
/// movimientos (en ellas se buscan las huellas del banco y las fechas de referencia).
pub(crate) fn leer(lineas: &[Linea], fuera: &[bool], primera_tabla: usize) -> Datos {
    let textos: Vec<String> = lineas.iter().map(Linea::texto).collect();
    let todo = textos.join("\n");
    let mut d = Datos::default();
    // Cuenta: el primer IBAN de antes de la tabla, o el primero de todos.
    let antes: String = textos[..primera_tabla.min(textos.len())].join("\n");
    d.iban = iban::en_texto(&antes)
        .into_iter()
        .next()
        .or_else(|| {
            textos
                .iter()
                .enumerate()
                .filter(|(i, _)| fuera.get(*i).copied().unwrap_or(true))
                .find_map(|(_, t)| iban::en_texto(t).into_iter().next())
        })
        .or_else(|| ccc(&todo));
    let fuera_txt: String = textos
        .iter()
        .enumerate()
        .filter(|(i, _)| fuera.get(*i).copied().unwrap_or(true))
        .map(|(_, t)| t.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    d.banco = banco(&fuera_txt, d.iban.as_deref());
    if d.iban.is_none() {
        tarjeta_o_cuenta(&textos, fuera, &mut d);
    }
    if d.clase.is_none() && d.banco.as_deref().is_some_and(|b| MONEDEROS.contains(&b)) {
        d.clase = Some(Clase::Monedero);
        if d.iban.is_none() && d.cuenta.is_none() && d.tarjeta.is_none() {
            d.cuenta = id_de_cuenta(&textos).map(|id| {
                format!(
                    "{}:{id}",
                    d.banco.as_deref().unwrap_or("cuenta").to_lowercase()
                )
            });
        }
    }
    d.titular = titular(lineas);
    d.divisa = divisa(lineas);
    let orden = numeros::inferir_orden(textos.iter().flat_map(|t| t.split_whitespace()));
    if let Some((a, b)) = periodo(&textos, orden) {
        d.desde = Some(a);
        d.hasta = Some(b);
    }
    d.referencia = fecha_de_referencia(&textos, fuera, orden);
    for (i, l) in lineas.iter().enumerate() {
        if !fuera.get(i).copied().unwrap_or(true) {
            continue;
        }
        let Some(cl) = tabla::clase_saldo(&textos[i]) else {
            continue;
        };
        let Some(c) = cifra_de_saldo(l, d.divisa) else {
            continue;
        };
        match cl {
            ClaseSaldo::Inicial if d.saldo_inicial.is_none() => d.saldo_inicial = Some(c),
            ClaseSaldo::Final if d.saldo_final.is_none() => d.saldo_final = Some(c),
            _ => {}
        }
    }
    d
}

/// El importe de una línea de saldo: el primero detrás del texto del saldo (los hay con varias
/// columnas, como «Saldo inicial · Saldo final»: entonces no vale).
fn cifra_de_saldo(l: &Linea, divisa: Option<&str>) -> Option<Cifra> {
    let ps = piezas::de_linea(l);
    let cifras: Vec<Cifra> = ps
        .iter()
        .filter_map(|p| p.cifra.filter(|_| !p.fecha))
        .filter(|c| match (c.divisa, divisa) {
            (Some(a), Some(b)) => a == b,
            _ => true,
        })
        .collect();
    let etiquetas = ps
        .iter()
        .filter(|p| p.es_texto() && tabla::clase_saldo(&p.texto).is_some())
        .count();
    let n_saldos = l
        .trozos
        .iter()
        .filter(|t| tabla::clase_saldo(&t.texto).is_some())
        .count();
    if n_saldos > 1 || etiquetas > 1 {
        return None;
    }
    cifras.first().copied()
}

/// IBAN de un código cuenta cliente de 20 dígitos (con sus dígitos de control bien).
fn ccc(texto: &str) -> Option<String> {
    static RE: LazyLock<Option<Regex>> =
        LazyLock::new(|| re(r"\b(\d{4})[ -]?(\d{4})[ -]?(\d{2})[ -]?(\d{10})\b"));
    let r = RE.as_ref()?;
    r.captures_iter(texto).find_map(|c| {
        let (e, o, dc, n) = (c.get(1)?, c.get(2)?, c.get(3)?, c.get(4)?);
        let d1 = digito_control(&format!("00{}{}", e.as_str(), o.as_str()))?;
        let d2 = digito_control(n.as_str())?;
        (format!("{d1}{d2}") == dc.as_str())
            .then(|| {
                iban::desde_ccc(&format!(
                    "{}{}{}{}",
                    e.as_str(),
                    o.as_str(),
                    dc.as_str(),
                    n.as_str()
                ))
            })
            .flatten()
    })
}

/// Dígito de control de un código cuenta cliente (diez dígitos).
fn digito_control(s: &str) -> Option<u32> {
    const PESOS: [u32; 10] = [1, 2, 4, 8, 5, 10, 9, 7, 3, 6];
    if s.len() != 10 {
        return None;
    }
    let mut suma = 0;
    for (c, p) in s.chars().zip(PESOS) {
        suma += c.to_digit(10)? * p;
    }
    Some(match 11 - suma % 11 {
        11 => 0,
        10 => 1,
        d => d,
    })
}

/// El banco por el código del IBAN o por su nombre en el texto.
fn banco(texto: &str, iban_: Option<&str>) -> Option<String> {
    if let Some(b) = iban_.and_then(iban::banco) {
        return Some(b.to_owned());
    }
    let t = sin_tildes(texto);
    for (huella, nombre, _) in NOMBRES {
        if let Some(p) = t.find(huella) {
            let antes = t[..p].chars().next_back();
            let despues = t[p + huella.len()..].chars().next();
            let suelta = |c: Option<char>| c.is_none_or(|c| !c.is_alphanumeric());
            if suelta(antes) && suelta(despues) {
                return Some(nombre.to_owned());
            }
        }
    }
    None
}

/// Código de entidad de un banco por su nombre.
fn entidad_de(banco_: &str) -> Option<&'static str> {
    NOMBRES
        .iter()
        .find(|(_, n, _)| *n == banco_)
        .and_then(|(_, _, e)| *e)
}

/// Sin IBAN: una tarjeta (por su número, completo o enmascarado) o una cuenta enmascarada.
fn tarjeta_o_cuenta(textos: &[String], fuera: &[bool], d: &mut Datos) {
    static PAN: LazyLock<Option<Regex>> = LazyLock::new(|| re(r"\b\d{13,19}\b"));
    let es_tarjeta = |t: &str| {
        let s = sin_tildes(t);
        ["tarjeta", "card", "karte", "carte", "targeta", "carta"]
            .iter()
            .any(|w| s.contains(w))
    };
    let lineas_fuera: Vec<&String> = textos
        .iter()
        .enumerate()
        .filter(|(i, _)| fuera.get(*i).copied().unwrap_or(true))
        .map(|(_, t)| t)
        .collect();
    let mut ultimos: Option<String> = None;
    // ¿El título del documento (sus primeras líneas) habla de una tarjeta?
    let titulo = lineas_fuera.iter().take(8).any(|t| es_tarjeta(t));
    for (k, t) in lineas_fuera.iter().enumerate() {
        // El número en la misma línea que la palabra «tarjeta», en la siguiente o en un
        // documento que es de una tarjeta.
        let cerca = titulo
            || es_tarjeta(t)
            || k.checked_sub(1)
                .is_some_and(|j| es_tarjeta(lineas_fuera[j]));
        if !cerca {
            continue;
        }
        if let Some(m) = iban::enmascarada(t).filter(|m| m.prefijo.is_none()) {
            ultimos = Some(m.ultimos);
            break;
        }
        if let Some(r) = PAN.as_ref()
            && let Some(m) = r.find_iter(t).find(|m| luhn(m.as_str()))
        {
            ultimos = Some(m.as_str()[m.as_str().len() - 4..].to_owned());
            break;
        }
    }
    let todo = sin_tildes(
        &lineas_fuera
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
    );
    if let Some(u) = ultimos {
        // La entidad: la de un número de contrato que empiece por un código conocido, o la del
        // banco que se ve en el texto.
        let entidad =
            contrato(&todo).or_else(|| d.banco.as_deref().and_then(entidad_de).map(str::to_owned));
        if d.banco.is_none()
            && let Some(e) = &entidad
        {
            d.banco = iban::nombre_entidad(e).map(str::to_owned);
        }
        d.tarjeta = Some((entidad, u));
        let debito = ["debito", "debit"].iter().any(|w| todo.contains(w));
        let credito = ["credito", "credit"].iter().any(|w| todo.contains(w));
        d.clase = Some(if debito && !credito {
            Clase::TarjetaDebito
        } else {
            Clase::TarjetaCredito
        });
        return;
    }
    // Una cuenta de la que sólo se ven unos dígitos.
    for t in &lineas_fuera {
        if let Some(m) = iban::enmascarada(t)
            && (m.prefijo.is_some() || !es_tarjeta(t))
        {
            d.cuenta = Some(m.clave());
            return;
        }
    }
}

/// Código de entidad de un número de contrato de 20 o más dígitos que empiece por uno
/// conocido («2100-0418-…», «21000418…»).
fn contrato(texto: &str) -> Option<String> {
    static RE: LazyLock<Option<Regex>> =
        LazyLock::new(|| re(r"\b(\d{4})[- ]?\d{4}[- ]?\d{4}[- ]?\d{8,14}\b"));
    let r = RE.as_ref()?;
    r.captures_iter(texto).find_map(|c| {
        let e = c.get(1)?.as_str();
        iban::nombre_entidad(e).map(|_| e.to_owned())
    })
}

/// El identificador de la cuenta de un monedero («Id. de cuenta: …», «Account ID: …»).
fn id_de_cuenta(textos: &[String]) -> Option<String> {
    static RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
        re(
            r"(?i)(?:id\.? de cuenta(?: de [a-z]+)?|account id|merchant id|n[ºo°]\.? de cuenta|n[uú]mero de cuenta|account number)\s*:?\s*([A-Z0-9]{6,24})\b",
        )
    });
    let r = RE.as_ref()?;
    textos.iter().find_map(|t| {
        r.captures(t)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_owned())
    })
}

/// Palabras que suelen ir en el nombre de una empresa.
fn parece_empresa(t: &str) -> bool {
    let s = format!(" {} ", sin_tildes(t).replace([',', '.'], " "));
    [
        " sl ",
        " s l ",
        " slu ",
        " s l u ",
        " sa ",
        " s a ",
        " sociedad ",
        " limitada ",
        " gmbh ",
        " ltd ",
        " limited ",
        " inc ",
        " llc ",
        " sas ",
        " sarl ",
        " bv ",
        " ag ",
        " srl ",
        " spa ",
        " cb ",
        " scp ",
    ]
    .iter()
    .any(|w| s.contains(w))
}

/// El titular: tras una etiqueta («Titular», «Account holder», «Empresa»…) en la misma línea o
/// debajo; si no, el nombre que va con el IBAN («ES… - NOMBRE») o el primer nombre de empresa
/// de arriba de la primera página.
fn titular(lineas: &[Linea]) -> Option<String> {
    const ETIQUETAS: [&str; 16] = [
        "titular de la cuenta",
        "titular de la tarjeta",
        "titular",
        "account holder",
        "account name",
        "holder",
        "kontoinhaber",
        "inhaber",
        "titulaire",
        "intestatario",
        "nombre del titular",
        "razon social",
        "empresa",
        "cliente",
        "customer",
        "name",
    ];
    let limpia = |s: &str| -> Option<String> {
        // Sin restos de la plantilla del PDF («[object Object]») ni paréntesis alrededor.
        let mut s = s.to_owned();
        while let (Some(a), Some(b)) = (s.find('['), s.find(']')) {
            if b < a {
                break;
            }
            s.replace_range(a..=b, "");
        }
        let s = s.trim().trim_start_matches(':').trim();
        let s = match s.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
            Some(x) if !x.contains('(') => x.trim(),
            _ => s,
        };
        let ok = s.chars().filter(|c| c.is_alphabetic()).count() >= 3
            && !s.bytes().take(6).all(|b| b.is_ascii_digit())
            && !es_etiqueta(s);
        ok.then(|| s.to_owned())
    };
    for (i, l) in lineas.iter().enumerate() {
        for (k, t) in l.trozos.iter().enumerate() {
            let s = sin_tildes(&t.texto);
            let s = s.trim().trim_end_matches(':').trim();
            let Some(e) = ETIQUETAS
                .iter()
                .find(|e| s == **e || s.starts_with(&format!("{e}:")))
            else {
                continue;
            };
            // «Titular: NOMBRE» en el mismo trozo.
            if s.len() > e.len() + 1 {
                let resto: String = t.texto.chars().skip(e.chars().count() + 1).collect();
                if let Some(v) = limpia(&resto) {
                    return Some(v);
                }
            }
            // En el trozo de al lado (si no es otra etiqueta: entonces los valores van debajo).
            if let Some(v) = l.trozos.get(k + 1).and_then(|n| limpia(&n.texto)) {
                return Some(v);
            }
            // Debajo, a la misma altura de la columna.
            if let Some(sig) = lineas.get(i + 1).filter(|s| s.pagina == l.pagina)
                && let Some(v) = sig
                    .trozos
                    .iter()
                    .find(|n| (n.x0 - t.x0).abs() <= 60)
                    .and_then(|n| limpia(&n.texto))
            {
                return Some(v);
            }
        }
    }
    // «ES91 2100 0418 4502 0005 1332 - NOMBRE» (o el número enmascarado de una tarjeta).
    for l in lineas {
        let t = l.texto();
        if (!iban::en_texto(&t).is_empty() || iban::enmascarada(&t).is_some())
            && let Some((_, nombre)) = t.split_once(" - ")
            && let Some(v) = limpia(nombre.trim_end_matches('.'))
        {
            return Some(v);
        }
    }
    lineas
        .iter()
        .take_while(|l| l.pagina <= 1)
        .take(30)
        .flat_map(|l| l.trozos.iter())
        .find(|t| parece_empresa(&t.texto))
        .and_then(|t| limpia(&t.texto))
}

/// ¿Es el nombre de un dato de la cabecera del extracto («Usuario», «Fecha y hora»,
/// «Cuenta»…) y no un valor?
fn es_etiqueta(s: &str) -> bool {
    const ETIQUETAS: [&str; 30] = [
        "usuario",
        "user",
        "fecha",
        "fecha y hora",
        "hora",
        "cuenta",
        "cuenta relacion",
        "divisa",
        "moneda",
        "banco",
        "periodo",
        "importe",
        "filtros",
        "oficina",
        "nif",
        "cif",
        "iban",
        "bic",
        "swift",
        "tipo",
        "saldo",
        "contrato",
        "tarjeta",
        "producto",
        "sucursal",
        "entidad",
        "direccion",
        "domicilio",
        "telefono",
        "email",
    ];
    let t = sin_tildes(s);
    let t = t.trim().trim_end_matches(':').trim();
    ETIQUETAS.contains(&t)
}

/// La divisa de la cuenta: tras una etiqueta («Divisa», «Moneda», «Currency»…) o, si no, la
/// que más llevan los importes.
fn divisa(lineas: &[Linea]) -> Option<&'static str> {
    if let Some(d) = divisa_de_etiqueta(lineas) {
        return Some(d);
    }
    let mut cuenta: Vec<(&'static str, usize)> = Vec::new();
    for l in lineas {
        for p in piezas::de_linea(l) {
            if let Some(d) = p.cifra.and_then(|c| c.divisa) {
                match cuenta.iter_mut().find(|(x, _)| *x == d) {
                    Some((_, n)) => *n += 1,
                    None => cuenta.push((d, 1)),
                }
            }
        }
    }
    cuenta.into_iter().max_by_key(|(_, n)| *n).map(|(d, _)| d)
}

/// La divisa que dice una etiqueta («Divisa EUR», «Currency: USD») o el título de una
/// sección («Historial de transacciones - EUR», «Cuenta en USD»).
pub(crate) fn divisa_de_etiqueta(lineas: &[Linea]) -> Option<&'static str> {
    const ETIQUETAS: [&str; 8] = [
        "divisa de la cuenta",
        "account currency",
        "divisa",
        "moneda",
        "currency",
        "wahrung",
        "devise",
        "valuta",
    ];
    for l in lineas {
        for (k, t) in l.trozos.iter().enumerate() {
            let s = sin_tildes(&t.texto);
            let s = s.trim().trim_end_matches(':').trim();
            if let Some(e) = ETIQUETAS.iter().find(|e| s.starts_with(**e)) {
                let resto = s[e.len()..].trim().trim_start_matches(':').trim();
                if let Some(d) = piezas::divisa(resto) {
                    return Some(d);
                }
                if resto.is_empty()
                    && let Some(d) = l.trozos.get(k + 1).and_then(|n| piezas::divisa(&n.texto))
                {
                    return Some(d);
                }
            }
        }
        // Un título que acaba en la divisa: «… - EUR», «… (USD)», «… en GBP».
        if l.trozos.len() == 1 {
            let t = l.texto();
            let t = t.trim().trim_end_matches(')');
            if let Some((antes, ultima)) = t.rsplit_once([' ', '(']) {
                let antes = antes.trim_end();
                let titulo = antes.ends_with(" -")
                    || antes.ends_with(" en")
                    || antes.ends_with(" in")
                    || t.ends_with(&format!("({ultima}"));
                if titulo && let Some(d) = piezas::divisa(ultima).filter(|_| ultima.len() == 3) {
                    return Some(d);
                }
            }
        }
    }
    None
}

/// La cuenta y la divisa que dicen unas líneas (las de encima de una cabecera de la tabla),
/// para los extractos de varias cuentas o divisas.
pub(crate) fn contexto(lineas: &[Linea]) -> (Option<String>, Option<&'static str>) {
    let texto: String = lineas
        .iter()
        .map(Linea::texto)
        .collect::<Vec<_>>()
        .join("\n");
    (
        iban::en_texto(&texto).into_iter().next(),
        divisa_de_etiqueta(lineas),
    )
}

/// Lo que dicen las palabras de un documento (sin las filas de su tabla) sobre si es un
/// extracto: +2 si su título es el de un extracto o una lista de movimientos, en varios
/// idiomas; −2 por cada clase de otro documento que parece (factura, nómina, impuestos,
/// cuentas anuales).
pub(crate) fn palabras_de_extracto(texto: &str) -> i32 {
    const EXTRACTO: [&str; 24] = [
        "extracto",
        "movimientos",
        "statement",
        "transacciones",
        "transactions",
        "kontoauszug",
        "umsatze",
        "umsaetze",
        "releve",
        "estratto",
        "rekeningafschrift",
        "afschrift",
        "historial de",
        "account activity",
        "account history",
        "estado de cuenta",
        "detalle de movimientos",
        "transaction history",
        "posicion de cuentas",
        "moviments",
        "mugimenduak",
        "extracte",
        "movimentos",
        "kontoumsatze",
    ];
    const OTROS: [&[&str]; 5] = [
        &[
            "factura",
            "invoice",
            "rechnung",
            "facture",
            "fattura",
            "credit note",
            "nota de credito",
            "base imponible",
            "presupuesto",
            "albaran",
        ],
        &[
            "nomina",
            "devengos",
            "liquido a percibir",
            "recibo de salarios",
            "payslip",
        ],
        &[
            "agencia tributaria",
            "modelo 1",
            "modelo 2",
            "modelo 3",
            "informacion fiscal",
            "impuesto sobre la renta",
            "declaracion de la renta",
            "autoliquidacion",
        ],
        &[
            "cuentas anuales",
            "balance de situacion",
            "perdidas y ganancias",
            "memoria abreviada",
            "deposito de cuentas",
        ],
        &[
            "escritura",
            "acta de la junta",
            "certificado de titularidad",
        ],
    ];
    let t = sin_tildes(texto);
    let mut nota = 0;
    if EXTRACTO.iter().any(|w| t.contains(w)) {
        nota += 2;
    }
    for clase in OTROS {
        if clase.iter().any(|w| t.contains(w)) {
            nota -= 2;
        }
    }
    nota
}

/// Patrón de una fecha dentro de un texto (numérica o con el mes en letra).
const FECHA: &str = r"(?:\d{1,2}[/.\-]\d{1,2}[/.\-]\d{2,4}|\d{4}-\d{2}-\d{2}|\d{1,2}(?:\s+de)?\s+\p{L}{3,10}\.?(?:\s+de|,)?\s+\d{4}|\p{L}{3,10}\.?\s+\d{1,2},?\s+\d{4})";

/// El periodo del extracto: dos fechas unidas por «-», «a», «al», «hasta», «to», «bis», «au»…
fn periodo(textos: &[String], orden: Orden) -> Option<(Date, Date)> {
    static RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
        re(&format!(
            r"(?i)(?P<a>{FECHA})\s*(?:-|–|—|a|al|hasta|to|until|till|bis|au|al|e|y|and|\.\.)\s*(?P<b>{FECHA})"
        ))
    });
    let r = RE.as_ref()?;
    for t in textos {
        for c in r.captures_iter(t) {
            let (Some(a), Some(b)) = (c.name("a"), c.name("b")) else {
                continue;
            };
            let (Some(a), Some(b)) = (
                numeros::fecha(a.as_str(), None, orden),
                numeros::fecha(b.as_str(), None, orden),
            ) else {
                continue;
            };
            if a <= b {
                return Some((a, b));
            }
        }
    }
    None
}

/// La fecha más reciente con año completo que hay fuera de la tabla.
fn fecha_de_referencia(textos: &[String], fuera: &[bool], orden: Orden) -> Option<Date> {
    static RE: LazyLock<Option<Regex>> = LazyLock::new(|| re(FECHA));
    let r = RE.as_ref()?;
    textos
        .iter()
        .enumerate()
        .filter(|(i, _)| fuera.get(*i).copied().unwrap_or(true))
        .flat_map(|(_, t)| {
            r.find_iter(t)
                .map(|m| m.as_str().to_owned())
                .collect::<Vec<_>>()
        })
        .filter_map(|s| numeros::fecha(&s, None, orden))
        .max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn ls(textos: &[&[(i32, &str)]]) -> Vec<Linea> {
        textos
            .iter()
            .enumerate()
            .map(|(i, t)| Linea::de_trozos(1, i32::try_from(i).unwrap_or(0) * 150 + 100, t))
            .collect()
    }

    #[test]
    fn cuenta_titular_divisa_periodo_y_saldos() {
        let l = ls(&[
            &[(400, "EMPRESA DEMO SL")],
            &[
                (400, "Titular"),
                (1200, "EMPRESA DEMO SL"),
                (3000, "Periodo"),
                (3600, "01/01/2025-31/01/2025"),
            ],
            &[
                (400, "Cuenta"),
                (1200, "ES9121000418450200051332"),
                (3000, "Divisa"),
                (3600, "EUR"),
            ],
            &[(400, "Saldo anterior"), (4000, "1.000,00")],
            &[(400, "Saldo final"), (4000, "1.234,56")],
        ]);
        let d = leer(&l, &[true; 5], 5);
        assert_eq!(d.iban.as_deref(), Some("ES9121000418450200051332"));
        assert_eq!(d.banco.as_deref(), Some("CaixaBank"));
        assert_eq!(d.titular.as_deref(), Some("EMPRESA DEMO SL"));
        assert_eq!(d.divisa, Some("EUR"));
        assert_eq!(
            (d.desde, d.hasta),
            (Some(date!(2025 - 01 - 01)), Some(date!(2025 - 01 - 31)))
        );
        assert_eq!(d.saldo_inicial.map(|c| c.importe.cents()), Some(100_000));
        assert_eq!(d.saldo_final.map(|c| c.importe.cents()), Some(123_456));
    }

    #[test]
    fn titular_debajo_de_su_etiqueta_o_junto_al_numero() {
        // Una fila de etiquetas y debajo la de sus valores.
        let l = ls(&[
            &[(500, "Empresa"), (2400, "Usuario"), (4500, "Fecha y hora")],
            &[
                (500, "EMPRESA DEMO SL"),
                (2400, "PERSONA DEMO"),
                (4500, "1/2/24 10:00"),
            ],
        ]);
        assert_eq!(titular(&l).as_deref(), Some("EMPRESA DEMO SL"));
        // Con restos de la plantilla del PDF.
        let l = ls(&[&[(
            500,
            "ES91 2100 0418 4502 0005 1332 - [object Object] (EMPRESA DEMO SL)",
        )]]);
        assert_eq!(titular(&l).as_deref(), Some("EMPRESA DEMO SL"));
        let l = ls(&[&[(500, "**** **** **** 1111 - PERSONA DEMO")]]);
        assert_eq!(titular(&l).as_deref(), Some("PERSONA DEMO"));
    }

    #[test]
    fn periodos_en_varios_idiomas() {
        let p = |t: &str| periodo(&[t.to_owned()], Orden::DiaMes);
        assert_eq!(
            p("Transacciones de 2 de mayo de 2023 a 6 de enero de 2024"),
            Some((date!(2023 - 05 - 02), date!(2024 - 01 - 06)))
        );
        assert_eq!(
            p("Fecha de 01/02/2022 hasta 30/04/2022"),
            Some((date!(2022 - 02 - 01), date!(2022 - 04 - 30)))
        );
        assert_eq!(
            p("Statement period: 1 Jan 2025 to 31 Jan 2025"),
            Some((date!(2025 - 01 - 01), date!(2025 - 01 - 31)))
        );
        assert_eq!(
            p("Kontoauszug vom 01.02.2025 bis 28.02.2025"),
            Some((date!(2025 - 02 - 01), date!(2025 - 02 - 28)))
        );
        assert_eq!(
            p("01.03.2020 - 31.05.2020"),
            Some((date!(2020 - 03 - 01), date!(2020 - 05 - 31)))
        );
    }

    #[test]
    fn tarjeta_sin_iban() {
        let l = ls(&[
            &[(400, "Listado de movimientos de tarjeta de crédito")],
            &[(400, "Nº de tarjeta"), (1200, "4111********1111")],
            &[(400, "Nº de contrato"), (1200, "21000418450000012345")],
        ]);
        let d = leer(&l, &[true; 3], 3);
        assert_eq!(
            d.tarjeta,
            Some((Some("2100".to_owned()), "1111".to_owned()))
        );
        assert_eq!(d.clase, Some(Clase::TarjetaCredito));
        assert_eq!(d.banco.as_deref(), Some("CaixaBank"));

        let l = ls(&[
            &[(400, "Tarjetas de débito - Extracto")],
            &[(400, "**** **** **** 1111 - EMPRESA DEMO SL")],
        ]);
        let d = leer(&l, &[true; 2], 2);
        assert_eq!(d.tarjeta, Some((None, "1111".to_owned())));
        assert_eq!(d.clase, Some(Clase::TarjetaDebito));
    }

    #[test]
    fn palabras_de_un_extracto_y_de_otros_documentos() {
        assert_eq!(palabras_de_extracto("Extracto de movimientos"), 2);
        assert_eq!(palabras_de_extracto("Bank statement · Account activity"), 2);
        assert_eq!(palabras_de_extracto("Kontoauszug Nr. 3"), 2);
        assert_eq!(palabras_de_extracto("FACTURA 12 · Base imponible"), -2);
        assert_eq!(palabras_de_extracto("Invoice statement"), 0);
        assert_eq!(palabras_de_extracto("Nómina · Líquido a percibir"), -2);
        assert_eq!(palabras_de_extracto("Carta al cliente"), 0);
    }

    #[test]
    fn iban_de_un_codigo_cuenta_cliente() {
        assert_eq!(
            ccc("Cuenta 2100 0418 45 0200051332").as_deref(),
            Some("ES9121000418450200051332")
        );
        assert_eq!(ccc("Contrato 2100 0418 46 0200051332"), None);
        assert!(luhn("4111111111111111"));
        assert!(!luhn("4111111111111112"));
    }
}
