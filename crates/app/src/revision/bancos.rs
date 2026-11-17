//! Bancos: los extractos de la carpeta, los meses de cada cuenta sin extracto (con el saldo de
//! antes y de después de cada hueco), los saldos que no encadenan de un extracto al siguiente y
//! los extractos que no cuadran o no se pueden leer.
//!
//! Un extracto sin número de cuenta va con la única cuenta del mismo banco, divisa, clase y
//! titular con fechas seguidas o solapadas, si sus saldos encadenan con los de ella. Si puede ser
//! de varias, no se adivina: se dice, y no se revisa. Si no puede ser de ninguna (o sus saldos no
//! encadenan), es otra cuenta: se dice, y se revisa aparte, con los extractos sin número del
//! mismo banco (o la misma clave del lector), divisa y clase.

use std::collections::{BTreeMap, BTreeSet};

use taxpain_core::money::{Money, Rate};
use time::{Date, Duration, Month};

use super::expediente::{Clase, Expediente};
use super::{Area, Destinatario, Gravedad, Nuevo, Responsable, Salida, importe, lista, plural};
use crate::extractos::{Clase as ClaseCuenta, Cuadre, Extracto, dia_es};

/// Un extracto de la carpeta y el fichero del que sale.
#[derive(Clone)]
struct Tramo<'a> {
    ruta: &'a str,
    e: &'a Extracto,
    /// Lo que salió al comprobar sus saldos, para la tabla.
    saldos: String,
}

impl Tramo<'_> {
    /// El extracto no trae el número de la cuenta: su clave está vacía o es la que ponen los
    /// lectores sin él («revolut:Main», «hoja:…», «ia:…»), como al importarlo.
    fn sin_numero(&self) -> bool {
        crate::extractos::importar::sin_numero(self.e.cuenta.trim())
    }

    /// Del primer al último día que cubre: el periodo del extracto o, sin él, sus movimientos.
    fn fechas(&self) -> Option<(Date, Date)> {
        let primero = self.e.movimientos.iter().map(|m| m.fecha).min();
        let ultimo = self.e.movimientos.iter().map(|m| m.fecha).max();
        Some((self.e.desde.or(primero)?, self.e.hasta.or(ultimo)?))
    }
}

/// Días entre dos extractos que todavía cuentan como seguidos (un mes).
const SEGUIDOS: i64 = 31;

/// Días entre el final de un extracto y el principio del siguiente en que sus saldos tienen que
/// encadenar (con más, puede faltar un mes entero de movimientos).
const ENCADENAN: i64 = 40;

/// Por qué un extracto sin número de cuenta no va con ninguna cuenta con número.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Suelto {
    /// Puede ser de varias cuentas del mismo banco y divisa: no se revisa.
    Varias,
    /// No dice el banco.
    SinBanco,
    /// No hay ninguna cuenta con número del mismo banco, divisa y titular en esas fechas.
    SinCuenta,
    /// Por las fechas sería de esa cuenta, pero sus saldos no encadenan con los suyos.
    Saldos(String),
}

/// Con qué se juntan los extractos sin número que son otra cuenta: la clave que pone el lector
/// («revolut:Main», «hoja:…») o, sin ella, el banco; y la clase.
fn serie(t: &Tramo<'_>) -> String {
    let clave = t.e.cuenta.trim();
    let clave = if clave.is_empty() {
        format!("banco:{}", plano(&t.e.banco))
    } else {
        clave.to_lowercase()
    };
    format!("{clave}|{:?}", t.e.clase)
}

pub(crate) fn revisar(exp: &Expediente, hoy: Date, s: &mut Salida) {
    let mut por_cuenta: BTreeMap<(String, String), Vec<Tramo<'_>>> = BTreeMap::new();
    let mut sin_numero: Vec<Tramo<'_>> = Vec::new();
    let mut sin_leer: Vec<String> = Vec::new();
    let mut no_cuadran: Vec<(String, String)> = Vec::new();
    for d in exp.docs.iter().filter(|d| d.clase == Clase::Extracto) {
        if d.extracto_error.is_some() && d.extractos.is_empty() {
            sin_leer.push(d.ruta.clone());
            continue;
        }
        for l in &d.extractos {
            let e = &l.extracto;
            let saldos = match &l.cuadre {
                Cuadre::Cuadra => "cuadran".to_owned(),
                Cuadre::SinSaldos => "sin saldos".to_owned(),
                Cuadre::NoCuadra { diferencia, .. } => format!("no cuadran ({diferencia})"),
            };
            if let Cuadre::NoCuadra { detalle, .. } = &l.cuadre {
                no_cuadran.push((d.ruta.clone(), format!("{} ({detalle})", e.nombre())));
            }
            let t = Tramo {
                ruta: &d.ruta,
                e,
                saldos,
            };
            if t.sin_numero() {
                sin_numero.push(t);
            } else {
                por_cuenta
                    .entry((e.cuenta.clone(), e.divisa.clone()))
                    .or_default()
                    .push(t);
            }
        }
    }
    // Los extractos sin número de cuenta, con la cuenta que sólo puede ser la suya: por orden de
    // fechas, para que uno que sigue a otro ya asignado también vaya con ella.
    sin_numero.sort_by_key(|t| t.fechas());
    let mut sueltos: Vec<(Tramo<'_>, Suelto)> = Vec::new();
    for t in sin_numero {
        let candidatas: Vec<&(String, String)> = por_cuenta
            .iter()
            .filter(|(_, ts)| de_la_cuenta(&t, ts))
            .map(|(k, _)| k)
            .collect();
        let motivo = match candidatas.as_slice() {
            [k] if encadena(&t, &por_cuenta[*k]) => {
                let k = (*k).clone();
                por_cuenta.entry(k).or_default().push(t);
                continue;
            }
            [k] => Suelto::Saldos(nombre_de(&por_cuenta[*k])),
            [] if plano(&t.e.banco).is_empty() => Suelto::SinBanco,
            [] => Suelto::SinCuenta,
            _ => Suelto::Varias,
        };
        sueltos.push((t, motivo));
    }
    // Los que no pueden ser de ninguna cuenta con número son otra cuenta.
    let mut aparte: BTreeMap<(String, String), Vec<Tramo<'_>>> = BTreeMap::new();
    for (t, motivo) in &sueltos {
        if *motivo != Suelto::Varias {
            aparte
                .entry((serie(t), t.e.divisa.clone()))
                .or_default()
                .push(t.clone());
        }
    }
    let mut filas = Vec::new();
    for tramos in por_cuenta.values() {
        let nombre = nombre_de(tramos);
        for t in tramos {
            let cuenta = if t.sin_numero() {
                format!("{nombre} (el extracto no trae el número)")
            } else {
                t.e.nombre()
            };
            filas.push(fila(cuenta, t));
        }
    }
    for (t, _) in &sueltos {
        filas.push(fila(format!("{} (sin número de cuenta)", t.e.nombre()), t));
    }
    filas.sort();
    s.tabla(
        "Extractos del banco",
        "Extractos",
        &[
            "Cuenta",
            "Desde",
            "Hasta",
            "Movimientos",
            "Saldo inicial",
            "Saldo final",
            "Saldos",
            "Fichero",
        ],
        filas,
    );
    let mut cobertura = Vec::new();
    for tramos in por_cuenta.values_mut() {
        tramos.sort_by_key(|t| (t.e.desde, t.e.hasta));
        let nombre = nombre_de(tramos);
        cuenta(tramos, &nombre, hoy, s, &mut cobertura);
    }
    let mut aparte: Vec<(String, Vec<Tramo<'_>>)> = aparte
        .into_values()
        .map(|mut tramos| {
            tramos.sort_by_key(|t| (t.e.desde, t.e.hasta));
            (
                format!("{} (sin número de cuenta)", nombre_de(&tramos)),
                tramos,
            )
        })
        .collect();
    aparte.sort_by(|a, b| a.0.cmp(&b.0));
    for (nombre, tramos) in &aparte {
        cuenta(tramos, nombre, hoy, s, &mut cobertura);
    }
    s.tabla(
        "Meses con extracto, por cuenta",
        "Meses por cuenta",
        &[
            "Cuenta",
            "Desde",
            "Hasta",
            "Extractos",
            "Meses sin extracto",
        ],
        cobertura,
    );
    if !sueltos.is_empty() {
        sin_numero_de_cuenta(&sueltos, s);
    }
    if !no_cuadran.is_empty() {
        let (ficheros, cuales): (Vec<String>, Vec<String>) = no_cuadran.into_iter().unzip();
        s.add(
            Nuevo {
                codigo: "bancos.no_cuadran",
                area: Area::Bancos,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Empresa,
                titulo: format!(
                    "{} que no cuadra con sus saldos",
                    plural(ficheros.len(), "extracto", "extractos")
                ),
                ficheros,
            }
            .detalle(format!(
                "El saldo inicial más los movimientos no da el saldo que dice el banco: falta \
                 algún movimiento o el fichero no se ha leído bien. No se importan hasta \
                 revisarlos: {}.",
                cuales.join("; ")
            )),
            None,
            None,
        );
    }
    if !sin_leer.is_empty() {
        s.add(
            Nuevo {
                codigo: "bancos.sin_leer",
                area: Area::Bancos,
                gravedad: Gravedad::Info,
                responsable: Responsable::Empresa,
                titulo: format!(
                    "{} que TaxPain no ha podido leer",
                    plural(sin_leer.len(), "extracto", "extractos")
                ),
                ficheros: sin_leer.clone(),
            }
            .detalle(format!(
                "Son extractos de un formato que no reconoce ningún lector ({}). Léelos con \
                 «Leer con IA» en Importar (o con /leer-carpeta en Claude Code) para que entren \
                 sus movimientos, o descárgalos del banco en otro formato (Norma 43, Excel).",
                lista(&sin_leer, 3)
            )),
            None,
            None,
        );
    }
}

fn fila(cuenta: String, t: &Tramo<'_>) -> Vec<String> {
    let divisa = Some(t.e.divisa.as_str());
    vec![
        cuenta,
        dia_es(t.e.desde),
        dia_es(t.e.hasta),
        t.e.movimientos.len().to_string(),
        t.e.saldo_inicial
            .map(|m| importe(m, divisa))
            .unwrap_or_default(),
        t.e.saldo_final
            .map(|m| importe(m, divisa))
            .unwrap_or_default(),
        t.saldos.clone(),
        t.ruta.to_owned(),
    ]
}

/// El nombre de una cuenta: el de su primer extracto con número.
fn nombre_de(tramos: &[Tramo<'_>]) -> String {
    tramos
        .iter()
        .find(|t| !t.sin_numero())
        .or(tramos.first())
        .map(|t| t.e.nombre())
        .unwrap_or_default()
}

/// Para comparar nombres de bancos y de titulares: sólo letras y cifras, en minúsculas y sin
/// tildes («BANCO DEMO, S.A.» y «Banco Demo SA» son lo mismo).
fn plano(s: &str) -> String {
    crate::documentos::fold(s)
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

/// El nombre de un titular sin la forma jurídica del final («Empresa Demo, S.L.U.», «EMPRESA DEMO
/// SOCIEDAD LIMITADA» y «Empresa Demo» son el mismo), como [`plano`].
fn titular_plano(s: &str) -> String {
    const FORMA: [&str; 18] = [
        "s",
        "l",
        "u",
        "a",
        "p",
        "sl",
        "slu",
        "sa",
        "sau",
        "sll",
        "slp",
        "slne",
        "sociedad",
        "limitada",
        "unipersonal",
        "anonima",
        "profesional",
        "laboral",
    ];
    let f = crate::documentos::fold(s);
    let mut palabras: Vec<&str> = f
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    while palabras.len() > 1 && palabras.last().is_some_and(|w| FORMA.contains(w)) {
        palabras.pop();
    }
    palabras.concat()
}

/// ¿Puede ser de esta cuenta el extracto sin número `t`? Del mismo banco (que el extracto tiene
/// que decir), divisa y clase, del mismo titular si los dos lo dicen (con o sin la forma
/// jurídica) y con fechas seguidas o solapadas con las de la cuenta.
fn de_la_cuenta(t: &Tramo<'_>, cuenta: &[Tramo<'_>]) -> bool {
    let Some(c) = cuenta.first() else {
        return false;
    };
    let banco = plano(&t.e.banco);
    if banco.is_empty()
        || banco != plano(&c.e.banco)
        || t.e.divisa != c.e.divisa
        || t.e.clase != c.e.clase
    {
        return false;
    }
    let titular = titular_plano(&t.e.titular);
    let de_otro = cuenta.iter().any(|x| {
        let otro = titular_plano(&x.e.titular);
        !titular.is_empty() && !otro.is_empty() && titular != otro
    });
    if de_otro {
        return false;
    }
    let (Some((desde, hasta)), Some(desde_c), Some(hasta_c)) = (
        t.fechas(),
        cuenta.iter().filter_map(|x| x.fechas().map(|f| f.0)).min(),
        cuenta.iter().filter_map(|x| x.fechas().map(|f| f.1)).max(),
    ) else {
        return false;
    };
    desde <= hasta_c + Duration::days(SEGUIDOS) && hasta + Duration::days(SEGUIDOS) >= desde_c
}

/// ¿Encajan los saldos del extracto sin número `t` con los de la cuenta? No, si empieza o acaba
/// el mismo día que un extracto de la cuenta con otro saldo, o si no encadena con el extracto que
/// acaba justo antes o con el que empieza justo después (lo que en la cuenta sería «faltan
/// movimientos»). Sin saldos que comparar, sí: deciden las fechas.
fn encadena(t: &Tramo<'_>, cuenta: &[Tramo<'_>]) -> bool {
    let (Some(desde), Some(hasta)) = (t.e.desde, t.e.hasta) else {
        return true;
    };
    let distintos =
        |a: Option<Money>, b: Option<Money>| matches!((a, b), (Some(a), Some(b)) if a != b);
    let el_mismo_dia = cuenta.iter().any(|x| {
        (x.e.desde == Some(desde) && distintos(x.e.saldo_inicial, t.e.saldo_inicial))
            || (x.e.hasta == Some(hasta) && distintos(x.e.saldo_final, t.e.saldo_final))
    });
    let fin_de_antes = cuenta
        .iter()
        .filter_map(|x| x.e.hasta)
        .filter(|h| *h < desde)
        .max();
    let con_el_de_antes = fin_de_antes.is_some_and(|f| {
        desde - f <= Duration::days(ENCADENAN)
            && cuenta
                .iter()
                .any(|x| x.e.hasta == Some(f) && distintos(x.e.saldo_final, t.e.saldo_inicial))
    });
    let inicio_de_despues = cuenta
        .iter()
        .filter_map(|x| x.e.desde)
        .filter(|d| *d > hasta)
        .min();
    let con_el_de_despues = inicio_de_despues.is_some_and(|i| {
        i - hasta <= Duration::days(ENCADENAN)
            && cuenta
                .iter()
                .any(|x| x.e.desde == Some(i) && distintos(t.e.saldo_final, x.e.saldo_inicial))
    });
    !(el_mismo_dia || con_el_de_antes || con_el_de_despues)
}

/// Los extractos sin número de cuenta que no van con ninguna cuenta con número: una nota con
/// cada uno y por qué. Los que pueden ser de varias cuentas no se revisan; los demás se revisan
/// aparte, como otra cuenta.
fn sin_numero_de_cuenta(sueltos: &[(Tramo<'_>, Suelto)], s: &mut Salida) {
    let cuales: Vec<String> = sueltos
        .iter()
        .map(|(t, motivo)| {
            format!(
                "{} del {} al {} ({}; {})",
                t.e.nombre(),
                dia_es(t.e.desde),
                dia_es(t.e.hasta),
                t.ruta,
                match motivo {
                    Suelto::Varias =>
                        "hay varias cuentas del mismo banco y divisa en esas fechas".to_owned(),
                    Suelto::SinBanco => "tampoco dice el banco".to_owned(),
                    Suelto::SinCuenta => "no casa con ninguna cuenta con número del mismo banco, \
                                          divisa y titular en esas fechas"
                        .to_owned(),
                    Suelto::Saldos(c) =>
                        format!("sus saldos no encadenan con los de {c}, aunque las fechas sí"),
                }
            )
        })
        .collect();
    let mut detalle = format!(
        "No dicen de qué cuenta son y TaxPain no lo adivina: {}.",
        cuales.join("; ")
    );
    if sueltos.iter().any(|(_, m)| *m != Suelto::Varias) {
        detalle.push_str(
            " Los que no pueden ser de ninguna cuenta con número son otra cuenta: se revisan \
             aparte, juntos los del mismo banco (o la misma clave) y divisa, y salen en «Meses \
             con extracto» como «… (sin número de cuenta)».",
        );
    }
    if sueltos.iter().any(|(_, m)| *m == Suelto::Varias) {
        detalle.push_str(
            " Los que pueden ser de varias cuentas no se revisan: sus meses no salen en «Meses \
             con extracto».",
        );
    }
    detalle.push_str(" Si puedes, descárgalos del banco con el número de cuenta (el IBAN).");
    s.add(
        Nuevo {
            codigo: "bancos.sin_numero_de_cuenta",
            area: Area::Bancos,
            gravedad: Gravedad::Info,
            responsable: Responsable::Documentacion,
            titulo: format!(
                "{} sin número de cuenta",
                plural(sueltos.len(), "extracto", "extractos")
            ),
            ficheros: sueltos.iter().map(|(t, _)| t.ruta.to_owned()).collect(),
        }
        .detalle(detalle),
        None,
        None,
    );
}

/// Meses (año, mes) que cubren los extractos de una cuenta, al menos en parte.
fn meses(tramos: &[Tramo<'_>]) -> BTreeSet<(i32, u8)> {
    let mut out = BTreeSet::new();
    for t in tramos {
        let (Some(d), Some(h)) = (t.e.desde, t.e.hasta) else {
            continue;
        };
        let mut m = (d.year(), u8::from(d.month()));
        let fin = (h.year(), u8::from(h.month()));
        while m <= fin {
            out.insert(m);
            m = siguiente(m);
        }
    }
    out
}

fn siguiente((y, m): (i32, u8)) -> (i32, u8) {
    if m == 12 { (y + 1, 1) } else { (y, m + 1) }
}

const MESES: [&str; 12] = [
    "enero",
    "febrero",
    "marzo",
    "abril",
    "mayo",
    "junio",
    "julio",
    "agosto",
    "septiembre",
    "octubre",
    "noviembre",
    "diciembre",
];

fn nombre_mes((y, m): (i32, u8)) -> String {
    format!(
        "{} de {y}",
        MESES.get(usize::from(m).saturating_sub(1)).unwrap_or(&"?")
    )
}

/// Los meses seguidos juntos: «enero de 2024 a marzo de 2024, junio de 2024».
fn tramos_de_meses(meses: &[(i32, u8)]) -> String {
    seguidos(meses)
        .iter()
        .map(|r| nombre_de_tramo(r))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Los meses, en tramos de meses seguidos.
fn seguidos(meses: &[(i32, u8)]) -> Vec<Vec<(i32, u8)>> {
    let mut out: Vec<Vec<(i32, u8)>> = Vec::new();
    for m in meses {
        match out.last_mut() {
            Some(r) if r.last().is_some_and(|u| siguiente(*u) == *m) => r.push(*m),
            _ => out.push(vec![*m]),
        }
    }
    out
}

/// «marzo de 2024» o «enero de 2024 a marzo de 2024».
fn nombre_de_tramo(meses: &[(i32, u8)]) -> String {
    match (meses.first(), meses.last()) {
        (Some(a), Some(b)) if a != b => format!("{} a {}", nombre_mes(*a), nombre_mes(*b)),
        (Some(a), _) => nombre_mes(*a),
        _ => String::new(),
    }
}

/// Unos meses seguidos sin extracto y el saldo del extracto de antes (el final) y el del de
/// después (el inicial), si los dicen.
struct Hueco {
    meses: Vec<(i32, u8)>,
    antes: Option<Money>,
    despues: Option<Money>,
}

impl Hueco {
    /// ¿Apenas cambia el saldo de un lado al otro? Menos de 50 €, o menos del 1 % sin pasar de
    /// 50 € por mes del hueco: probablemente sólo comisiones. Sin los dos saldos no se sabe, ni
    /// con el saldo de antes a cero: la cuenta puede vaciarse cada día (o una tarjeta, liquidarse
    /// cada mes), y que el saldo sea igual no dice que no hubiera movimientos.
    fn plano(&self) -> bool {
        let (Some(a), Some(b)) = (self.antes, self.despues) else {
            return false;
        };
        if a.is_zero() {
            return false;
        }
        let cambio = (b - a).abs();
        let meses = i64::try_from(self.meses.len()).unwrap_or(i64::MAX);
        cambio <= Money::from_euros(50, 0)
            || (cambio <= a.abs().apply_rate(Rate::percent(1))
                && cambio.cents() <= 5_000_i64.saturating_mul(meses))
    }

    /// «marzo de 2025 a abril de 2025 (saldo de 1.000,00 € a 990,00 €)», o sin el saldo si no
    /// se sabe.
    fn texto(&self, divisa: &str) -> String {
        let tramo = nombre_de_tramo(&self.meses);
        match (self.antes, self.despues) {
            (Some(a), Some(b)) => format!(
                "{tramo} (saldo de {} a {})",
                importe(a, Some(divisa)),
                importe(b, Some(divisa))
            ),
            _ => tramo,
        }
    }
}

/// El primer día de un mes y el último.
fn primer_dia((y, m): (i32, u8)) -> Option<Date> {
    Date::from_calendar_date(y, Month::try_from(m).ok()?, 1).ok()
}

fn ultimo_dia((y, m): (i32, u8)) -> Option<Date> {
    let mes = Month::try_from(m).ok()?;
    Date::from_calendar_date(y, mes, mes.length(y)).ok()
}

/// Los huecos de una cuenta (sus meses sin extracto, en tramos seguidos) con el saldo de cada
/// lado: el final del último extracto que acaba antes y el inicial del primero que empieza
/// después.
fn huecos(tramos: &[Tramo<'_>], sin_extracto: &[(i32, u8)]) -> Vec<Hueco> {
    seguidos(sin_extracto)
        .into_iter()
        .map(|meses| {
            let inicio = meses.first().copied().and_then(primer_dia);
            let fin = meses.last().copied().and_then(ultimo_dia);
            let antes = inicio.and_then(|i| {
                tramos
                    .iter()
                    .filter(|t| t.e.hasta.is_some_and(|h| h < i))
                    .max_by_key(|t| (t.e.hasta, t.e.saldo_final.is_some()))
                    .and_then(|t| t.e.saldo_final)
            });
            let despues = fin.and_then(|f| {
                tramos
                    .iter()
                    .filter(|t| t.e.desde.is_some_and(|d| d > f))
                    .min_by_key(|t| (t.e.desde, t.e.saldo_inicial.is_none()))
                    .and_then(|t| t.e.saldo_inicial)
            });
            Hueco {
                meses,
                antes,
                despues,
            }
        })
        .collect()
}

/// Lo que se revisa de una cuenta: los meses sin extracto (con el saldo de antes y de después
/// de cada hueco: si apenas cambia, es una nota), los saldos que no encadenan entre un extracto
/// y el siguiente, y si el último acaba hace tiempo con dinero en la cuenta.
fn cuenta(
    tramos: &[Tramo<'_>],
    nombre: &str,
    hoy: Date,
    s: &mut Salida,
    cobertura: &mut Vec<Vec<String>>,
) {
    let Some(primero) = tramos.first() else {
        return;
    };
    let nombre = nombre.to_owned();
    let divisa = primero.e.divisa.as_str();
    let cubiertos = meses(tramos);
    let (Some(desde), Some(hasta)) = (cubiertos.first().copied(), cubiertos.last().copied()) else {
        return;
    };
    let mut faltan = Vec::new();
    let mut m = desde;
    while m <= hasta {
        if !cubiertos.contains(&m) {
            faltan.push(m);
        }
        m = siguiente(m);
    }
    cobertura.push(vec![
        nombre.clone(),
        nombre_mes(desde),
        nombre_mes(hasta),
        tramos.len().to_string(),
        if faltan.is_empty() {
            "ninguno".to_owned()
        } else {
            tramos_de_meses(&faltan)
        },
    ]);
    let (planos, otros): (Vec<Hueco>, Vec<Hueco>) =
        huecos(tramos, &faltan).into_iter().partition(Hueco::plano);
    if let Some(ultimo) = otros.last().and_then(|h| h.meses.last()) {
        let n: usize = otros.iter().map(|h| h.meses.len()).sum();
        let con_saldos = otros
            .iter()
            .any(|h| h.antes.is_some() && h.despues.is_some());
        let cuales: Vec<String> = otros.iter().map(|h| h.texto(divisa)).collect();
        s.add(
            Nuevo {
                codigo: "bancos.meses_sin_extracto",
                area: Area::Bancos,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Documentacion,
                titulo: format!("Faltan extractos de {nombre}"),
                ficheros: vec![],
            }
            .detalle(format!(
                "No hay ningún extracto de {}: {}. Sin ellos no se pueden comprobar los cobros y \
                 pagos de esos meses con la contabilidad.{}",
                plural(n, "mes", "meses"),
                cuales.join(if con_saldos { "; " } else { ", " }),
                if con_saldos {
                    " El saldo es el final del extracto de antes y el inicial del de después."
                } else {
                    ""
                }
            )),
            Some(ultimo.0),
            None,
        );
        let meses: Vec<(i32, u8)> = otros.iter().flat_map(|h| h.meses.clone()).collect();
        s.pedir(
            Destinatario::Empresa,
            format!(
                "Descargar del banco los extractos de {nombre} de {}",
                tramos_de_meses(&meses)
            ),
        );
    }
    if let Some(ultimo) = planos.last().and_then(|h| h.meses.last()) {
        let n: usize = planos.iter().map(|h| h.meses.len()).sum();
        let cuales: Vec<String> = planos.iter().map(|h| h.texto(divisa)).collect();
        s.add(
            Nuevo {
                codigo: "bancos.meses_sin_extracto_saldo_igual",
                area: Area::Bancos,
                gravedad: Gravedad::Info,
                responsable: Responsable::Documentacion,
                titulo: format!(
                    "{} sin extracto de {nombre} con el saldo casi igual",
                    if n == 1 { "Un mes" } else { "Meses" }
                ),
                ficheros: vec![],
            }
            .detalle(format!(
                "No hay extracto de {}: {}. El saldo apenas cambia entre el extracto de antes y el \
                 de después: probablemente sólo hubo comisiones. Si quieres comprobarlo, \
                 descárgalos del banco.",
                plural(n, "mes", "meses"),
                cuales.join("; ")
            )),
            Some(ultimo.0),
            None,
        );
    }
    // Saldos que no encadenan entre un extracto y el siguiente (sin solaparse ni dejar meses
    // sin extracto entre medias): faltan movimientos.
    for par in tramos.windows(2) {
        let (a, b) = (&par[0], &par[1]);
        let (Some(fin_a), Some(ini_b)) = (a.e.hasta, b.e.desde) else {
            continue;
        };
        if ini_b <= fin_a || ini_b - fin_a > Duration::days(ENCADENAN) {
            continue;
        }
        let (Some(saldo_a), Some(saldo_b)) = (a.e.saldo_final, b.e.saldo_inicial) else {
            continue;
        };
        if saldo_a != saldo_b {
            s.add(
                Nuevo {
                    codigo: "bancos.saldos_no_encadenan",
                    area: Area::Bancos,
                    gravedad: Gravedad::Aviso,
                    responsable: Responsable::Documentacion,
                    titulo: format!("Faltan movimientos de {nombre} entre dos extractos"),
                    ficheros: vec![a.ruta.to_owned(), b.ruta.to_owned()],
                }
                .detalle(format!(
                    "El extracto que acaba el {} termina con {} y el siguiente, que empieza el {}, \
                     sale de {}: faltan {} en movimientos entre los dos.",
                    dia_es(Some(fin_a)),
                    importe(saldo_a, Some(divisa)),
                    dia_es(Some(ini_b)),
                    importe(saldo_b, Some(divisa)),
                    importe(saldo_b - saldo_a, Some(divisa))
                )),
                Some(ini_b.year()),
                None,
            );
        }
    }
    // El último extracto de una cuenta con dinero acaba hace más de dos meses.
    let ultimo = tramos
        .iter()
        .filter_map(|t| t.e.hasta.map(|h| (h, t.e.saldo_final)))
        .max_by_key(|(h, _)| *h);
    if let Some((h, saldo)) = ultimo
        && primero.e.clase == ClaseCuenta::Cuenta
        && hoy - h > Duration::days(62)
        && saldo.is_some_and(|s| s != Money::ZERO)
    {
        s.add(
            Nuevo {
                codigo: "bancos.ultimo_extracto_antiguo",
                area: Area::Bancos,
                gravedad: Gravedad::Info,
                responsable: Responsable::Documentacion,
                titulo: format!("El último extracto de {nombre} es del {}", dia_es(Some(h))),
                ficheros: vec![],
            }
            .detalle(format!(
                "La cuenta tenía {} y no hay extractos posteriores. Si sigue abierta, faltan los \
                 de después.",
                importe(saldo.unwrap_or_default(), Some(divisa))
            )),
            Some(h.year()),
            None,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::Hallazgo;
    use super::super::expediente::Doc;
    use super::*;
    use crate::extractos::Leido;

    fn dia(y: i32, m: u8, d: u8) -> Date {
        Date::from_calendar_date(y, Month::try_from(m).unwrap(), d).unwrap()
    }

    /// Un extracto inventado de `banco` (cuenta vacía: sin número), con sus saldos en céntimos.
    fn extracto(
        banco: &str,
        cuenta: &str,
        desde: Date,
        hasta: Date,
        saldos: Option<(i64, i64)>,
    ) -> Extracto {
        let mut e = Extracto::nuevo("PDF");
        e.banco = banco.into();
        e.cuenta = cuenta.into();
        e.titular = "EMPRESA DEMO SL".into();
        e.desde = Some(desde);
        e.hasta = Some(hasta);
        if let Some((i, f)) = saldos {
            e.saldo_inicial = Some(Money::from_cents(i));
            e.saldo_final = Some(Money::from_cents(f));
        }
        e
    }

    fn doc(ruta: &str, e: Extracto) -> Doc {
        let cuadre = if e.saldo_inicial.is_some() {
            Cuadre::Cuadra
        } else {
            Cuadre::SinSaldos
        };
        Doc {
            ruta: ruta.into(),
            sha: ruta.into(),
            clase: Clase::Extracto,
            descripcion: String::new(),
            con_ia: false,
            ruta_año: None,
            ruta_trimestre: None,
            factura: None,
            modelo: None,
            libro: None,
            autorrepercusion: Vec::new(),
            extractos: vec![Leido {
                extracto: e,
                cuadre,
            }],
            extracto_error: None,
            empresa: None,
            tramite: None,
            aplicacion: None,
            nomina: None,
            mencion_sin_iva: None,
            oficial: None,
            texto_no_aeat: false,
            participada: None,
        }
    }

    fn revisar_en(docs: Vec<Doc>, hoy: Date) -> Salida {
        let exp = Expediente {
            carpeta: String::new(),
            empresa: "EMPRESA DEMO SL".into(),
            own_nif: Some("B12345674".into()),
            docs,
            no_admitidos: Vec::new(),
            presentados: Vec::new(),
            perfil: None,
            perfil_deducido: None,
            taxpain: Default::default(),
            aeat: Vec::new(),
            comprobado: Default::default(),
        };
        let mut s = Salida::default();
        revisar(&exp, hoy, &mut s);
        s
    }

    fn tabla<'a>(s: &'a Salida, titulo: &str) -> &'a [Vec<String>] {
        s.tablas
            .iter()
            .find(|t| t.titulo == titulo)
            .map_or(&[], |t| t.filas.as_slice())
    }

    fn codigos(s: &Salida) -> Vec<&str> {
        s.hallazgos.iter().map(|h| h.codigo.as_str()).collect()
    }

    const CUENTA: &str = "00491500051234567892";

    /// Dos extractos sin número de cuenta (uno sin clave y otro con la que pone el lector de
    /// hojas) del mismo banco, con fechas seguidas a las de la única cuenta de ese banco: van con
    /// ella, también el segundo, que sigue al primero aunque venga antes en la carpeta. Su último
    /// extracto ya no es el de junio (antes salía «el último extracto es del 30/06/2026» por los
    /// que no traían el número).
    #[test]
    fn un_extracto_sin_numero_va_con_su_cuenta() {
        let s = revisar_en(
            vec![
                doc(
                    "banco-1T.pdf",
                    extracto(
                        "Banco Demo",
                        CUENTA,
                        dia(2026, 1, 1),
                        dia(2026, 3, 31),
                        Some((100_000, 90_000)),
                    ),
                ),
                doc(
                    "banco-2T.pdf",
                    extracto(
                        "Banco Demo",
                        CUENTA,
                        dia(2026, 4, 1),
                        dia(2026, 6, 30),
                        Some((90_000, 50_000)),
                    ),
                ),
                doc(
                    "banco-octubre.xlsx",
                    extracto(
                        "Banco Demo",
                        "hoja:Banco Demo",
                        dia(2026, 10, 16),
                        dia(2026, 10, 31),
                        None,
                    ),
                ),
                doc(
                    "banco-sin-iban.pdf",
                    extracto("BANCO DEMO", "", dia(2026, 7, 1), dia(2026, 10, 15), None),
                ),
            ],
            dia(2026, 11, 1),
        );
        assert!(
            !codigos(&s).contains(&"bancos.ultimo_extracto_antiguo"),
            "{:?}",
            s.hallazgos
        );
        assert!(!codigos(&s).contains(&"bancos.sin_numero_de_cuenta"));
        let meses = tabla(&s, "Meses con extracto, por cuenta");
        assert_eq!(meses.len(), 1, "{meses:?}");
        assert_eq!(meses[0][0], "Banco Demo …7892 EUR");
        assert_eq!(meses[0][2], "octubre de 2026");
        assert_eq!(meses[0][4], "ninguno");
        let extractos = tabla(&s, "Extractos del banco");
        assert_eq!(
            extractos
                .iter()
                .filter(|f| f[0] == "Banco Demo …7892 EUR (el extracto no trae el número)")
                .count(),
            2,
            "{extractos:?}"
        );
    }

    /// Si hay dos cuentas del mismo banco y divisa en esas fechas, si es de otro titular, si no
    /// hay ninguna cuenta de ese banco o si no dice el banco, no se adivina: una nota con cada
    /// extracto, y no se cuentan como otra cuenta.
    #[test]
    fn sin_numero_y_sin_cuenta_clara_no_se_adivina() {
        let mut de_otro = extracto("Banco Demo", "", dia(2026, 7, 1), dia(2026, 8, 31), None);
        de_otro.titular = "OTRA EMPRESA DEMO SL".into();
        let s = revisar_en(
            vec![
                doc(
                    "a.pdf",
                    extracto(
                        "Banco Demo",
                        "00491500051111111111",
                        dia(2026, 1, 1),
                        dia(2026, 6, 30),
                        Some((10_000, 10_000)),
                    ),
                ),
                doc(
                    "b.pdf",
                    extracto(
                        "Banco Demo",
                        "00491500052222222222",
                        dia(2026, 1, 1),
                        dia(2026, 6, 30),
                        Some((20_000, 20_000)),
                    ),
                ),
                doc(
                    "dos-candidatas.pdf",
                    extracto("Banco Demo", "", dia(2026, 2, 1), dia(2026, 4, 30), None),
                ),
                doc("de-otro.pdf", de_otro),
                doc(
                    "otro-banco.pdf",
                    extracto("Otro Banco", "", dia(2026, 2, 1), dia(2026, 4, 30), None),
                ),
                doc(
                    "sin-banco.pdf",
                    extracto("", "", dia(2026, 2, 1), dia(2026, 4, 30), None),
                ),
            ],
            dia(2026, 7, 15),
        );
        let h = s
            .hallazgos
            .iter()
            .find(|h| h.codigo == "bancos.sin_numero_de_cuenta")
            .unwrap();
        assert_eq!(h.gravedad, Gravedad::Info);
        assert!(h.titulo.starts_with("4 extractos"), "{}", h.titulo);
        assert_eq!(h.ficheros.len(), 4);
        for que in [
            "hay varias cuentas del mismo banco y divisa",
            "tampoco dice el banco",
            "no casa con ninguna cuenta con número del mismo banco",
        ] {
            assert!(h.detalle.contains(que), "{que}: {}", h.detalle);
        }
        // El que puede ser de las dos cuentas no se revisa; los demás, aparte.
        let meses: Vec<&str> = tabla(&s, "Meses con extracto, por cuenta")
            .iter()
            .map(|f| f[0].as_str())
            .collect();
        assert_eq!(
            meses,
            vec![
                "Banco Demo …1111 EUR",
                "Banco Demo …2222 EUR",
                "Banco Demo … EUR (sin número de cuenta)",
                "Cuenta … EUR (sin número de cuenta)",
                "Otro Banco … EUR (sin número de cuenta)",
            ]
        );
    }

    /// Dos extractos de Revolut sin IBAN, de enero y de marzo, y ninguna cuenta de Revolut con
    /// número: son una cuenta aparte y se revisa como las demás (falta febrero, y faltan
    /// movimientos entre los dos). Igual con dos hojas del mismo banco.
    #[test]
    fn sin_cuenta_con_numero_se_revisan_como_otra_cuenta() {
        let s = revisar_en(
            vec![
                doc(
                    "revolut-enero.pdf",
                    extracto(
                        "Revolut",
                        "revolut:Main",
                        dia(2026, 1, 1),
                        dia(2026, 1, 31),
                        Some((100_000, 200_000)),
                    ),
                ),
                doc(
                    "revolut-marzo.pdf",
                    extracto(
                        "Revolut",
                        "revolut:Main",
                        dia(2026, 3, 1),
                        dia(2026, 3, 31),
                        Some((980_000, 900_000)),
                    ),
                ),
                doc(
                    "hoja-enero.xlsx",
                    extracto(
                        "Banco Demo",
                        "hoja:Banco Demo",
                        dia(2026, 1, 1),
                        dia(2026, 1, 31),
                        Some((50_000, 60_000)),
                    ),
                ),
                doc(
                    "hoja-mayo.xlsx",
                    extracto(
                        "Banco Demo",
                        "hoja:Banco Demo",
                        dia(2026, 5, 1),
                        dia(2026, 5, 31),
                        Some((300_000, 310_000)),
                    ),
                ),
            ],
            dia(2026, 6, 10),
        );
        let faltan: Vec<&Hallazgo> = s
            .hallazgos
            .iter()
            .filter(|h| h.codigo == "bancos.meses_sin_extracto")
            .collect();
        assert_eq!(faltan.len(), 2, "{:?}", s.hallazgos);
        assert!(
            faltan.iter().any(|h| h.titulo
                == "Faltan extractos de Revolut … EUR (sin número de cuenta)"
                && h.detalle.contains("febrero de 2026")),
            "{faltan:?}"
        );
        assert!(
            faltan.iter().any(|h| h.titulo.contains("Banco Demo")
                && h.detalle.contains("febrero de 2026 a abril de 2026")),
            "{faltan:?}"
        );
        let encadena = s
            .hallazgos
            .iter()
            .find(|h| h.codigo == "bancos.saldos_no_encadenan")
            .unwrap();
        assert!(
            encadena.detalle.contains("faltan 7.800,00 €"),
            "{}",
            encadena.detalle
        );
        assert!(
            s.pedir
                .iter()
                .any(|p| p.que.contains("Revolut") && p.que.contains("febrero de 2026")),
            "{:?}",
            s.pedir
        );
        let h = s
            .hallazgos
            .iter()
            .find(|h| h.codigo == "bancos.sin_numero_de_cuenta")
            .unwrap();
        assert!(h.detalle.contains("se revisan aparte"), "{}", h.detalle);
    }

    /// Un extracto sin número del mismo banco y titular con fechas seguidas a las de la cuenta,
    /// pero con saldos que no encadenan con los suyos, es de otra cuenta: no tapa los meses que
    /// le faltan a la cuenta ni da «faltan movimientos». Tampoco si empieza el mismo día que un
    /// extracto de la cuenta con otro saldo.
    #[test]
    fn un_extracto_sin_numero_que_no_encadena_no_va_con_la_cuenta() {
        let de_la_cuenta = |ruta: &str, m1: u8, m2: u8, saldos: (i64, i64)| {
            doc(
                ruta,
                extracto(
                    "Banco Demo",
                    CUENTA,
                    dia(2026, m1, 1),
                    ultimo_dia((2026, m2)).unwrap(),
                    Some(saldos),
                ),
            )
        };
        let otra = |m1: u8, m2: u8| {
            doc(
                "otra-cuenta.xlsx",
                extracto(
                    "Banco Demo",
                    "hoja:Banco Demo",
                    dia(2026, m1, 1),
                    ultimo_dia((2026, m2)).unwrap(),
                    Some((500_000, 600_000)),
                ),
            )
        };
        let s = revisar_en(
            vec![
                de_la_cuenta("enero.pdf", 1, 1, (100_000, 120_000)),
                de_la_cuenta("febrero.pdf", 2, 2, (120_000, 150_000)),
                de_la_cuenta("mayo.pdf", 5, 5, (250_000, 260_000)),
                de_la_cuenta("junio.pdf", 6, 6, (260_000, 300_000)),
                otra(3, 4),
            ],
            dia(2026, 7, 15),
        );
        assert!(
            !codigos(&s).contains(&"bancos.saldos_no_encadenan"),
            "{:?}",
            s.hallazgos
        );
        let falta = s
            .hallazgos
            .iter()
            .find(|h| h.codigo == "bancos.meses_sin_extracto" && h.titulo.contains("…7892"))
            .unwrap_or_else(|| panic!("{:?}", s.hallazgos));
        assert!(
            falta.detalle.contains("marzo de 2026 a abril de 2026"),
            "{}",
            falta.detalle
        );
        let h = s
            .hallazgos
            .iter()
            .find(|h| h.codigo == "bancos.sin_numero_de_cuenta")
            .unwrap();
        assert!(
            h.detalle
                .contains("sus saldos no encadenan con los de Banco Demo …7892 EUR"),
            "{}",
            h.detalle
        );

        // Empieza el mismo día que el de marzo de la cuenta, con otro saldo.
        let s = revisar_en(
            vec![
                de_la_cuenta("enero.pdf", 1, 1, (100_000, 100_000)),
                de_la_cuenta("febrero.pdf", 2, 2, (100_000, 100_000)),
                de_la_cuenta("marzo.pdf", 3, 3, (100_000, 100_000)),
                de_la_cuenta("abril.pdf", 4, 6, (100_000, 100_000)),
                otra(3, 3),
            ],
            dia(2026, 7, 15),
        );
        assert!(
            !codigos(&s).contains(&"bancos.saldos_no_encadenan"),
            "{:?}",
            s.hallazgos
        );
        assert!(codigos(&s).contains(&"bancos.sin_numero_de_cuenta"));
    }

    /// Con el saldo a cero antes y después de los meses sin extracto (una cuenta que se vacía cada
    /// día, o una tarjeta que se liquida cada mes), un saldo igual no dice que no hubiera
    /// movimientos: es un aviso y se piden.
    #[test]
    fn con_el_saldo_a_cero_a_los_dos_lados_no_es_una_nota() {
        let s = revisar_en(
            vec![
                doc(
                    "enero-2025.pdf",
                    extracto(
                        "Banco Demo",
                        CUENTA,
                        dia(2025, 1, 1),
                        dia(2025, 1, 31),
                        Some((0, 0)),
                    ),
                ),
                doc(
                    "enero-2026.pdf",
                    extracto(
                        "Banco Demo",
                        CUENTA,
                        dia(2026, 1, 1),
                        dia(2026, 1, 31),
                        Some((0, 0)),
                    ),
                ),
            ],
            dia(2026, 2, 15),
        );
        assert!(
            !codigos(&s).contains(&"bancos.meses_sin_extracto_saldo_igual"),
            "{:?}",
            s.hallazgos
        );
        assert!(codigos(&s).contains(&"bancos.meses_sin_extracto"));
        assert!(
            s.pedir
                .iter()
                .any(|p| p.que.contains("febrero de 2025 a diciembre de 2025")),
            "{:?}",
            s.pedir
        );
        assert!(
            !Hueco {
                meses: vec![(2025, 2)],
                antes: Some(Money::ZERO),
                despues: Some(Money::ZERO),
            }
            .plano()
        );
    }

    /// Un extracto sin número de otro titular no va con la cuenta; con el mismo nombre y otra
    /// forma de escribir la forma jurídica, sí.
    #[test]
    fn el_titular_del_extracto_sin_numero() {
        let cuenta = || {
            doc(
                "banco.pdf",
                extracto(
                    "Banco Demo",
                    CUENTA,
                    dia(2026, 1, 1),
                    dia(2026, 6, 30),
                    Some((10_000, 10_000)),
                ),
            )
        };
        let con_titular = |titular: &str| {
            let mut e = extracto("Banco Demo", "", dia(2026, 7, 1), dia(2026, 9, 30), None);
            e.titular = titular.into();
            doc("sin-iban.pdf", e)
        };
        for (titular, va) in [
            ("OTRA EMPRESA DEMO SL", false),
            ("Empresa Demo, S.L.", true),
            ("EMPRESA DEMO SOCIEDAD LIMITADA", true),
            ("", true),
        ] {
            let s = revisar_en(vec![cuenta(), con_titular(titular)], dia(2026, 10, 15));
            assert_eq!(
                !codigos(&s).contains(&"bancos.sin_numero_de_cuenta"),
                va,
                "{titular}: {:?}",
                s.hallazgos
            );
        }
        assert_eq!(titular_plano("Empresa Demo, S.L.U."), "empresademo");
    }

    /// Entre el extracto de enero y el de abril el saldo sólo baja 10 €: los meses sin extracto
    /// salen en una nota («probablemente sólo comisiones») y no se piden. Entre mayo y agosto
    /// sube 4.010 €: aviso con el saldo de cada lado, y se piden. Sin los saldos, aviso sin ellos.
    #[test]
    fn hueco_con_el_saldo_casi_igual_es_una_nota() {
        let tramo = |ruta: &str, m1: u8, m2: u8, saldos: Option<(i64, i64)>| {
            doc(
                ruta,
                extracto(
                    "Banco Demo",
                    CUENTA,
                    dia(2025, m1, 1),
                    dia(2025, m2, 28),
                    saldos,
                ),
            )
        };
        let s = revisar_en(
            vec![
                tramo("enero.pdf", 1, 1, Some((100_000, 100_000))),
                tramo("abril.pdf", 4, 5, Some((99_000, 99_000))),
                tramo("agosto.pdf", 8, 8, Some((500_000, 500_000))),
                tramo("octubre.pdf", 10, 10, None),
            ],
            dia(2025, 11, 15),
        );
        let nota = s
            .hallazgos
            .iter()
            .find(|h| h.codigo == "bancos.meses_sin_extracto_saldo_igual")
            .unwrap();
        assert_eq!(nota.gravedad, Gravedad::Info);
        assert!(
            nota.detalle
                .contains("febrero de 2025 a marzo de 2025 (saldo de 1.000,00 € a 990,00 €)")
                && nota.detalle.contains("probablemente sólo hubo comisiones"),
            "{}",
            nota.detalle
        );
        let aviso = s
            .hallazgos
            .iter()
            .find(|h| h.codigo == "bancos.meses_sin_extracto")
            .unwrap();
        assert!(
            aviso
                .detalle
                .contains("junio de 2025 a julio de 2025 (saldo de 990,00 € a 5.000,00 €)"),
            "{}",
            aviso.detalle
        );
        assert!(
            aviso.detalle.contains("septiembre de 2025"),
            "{}",
            aviso.detalle
        );
        assert!(!aviso.detalle.contains("febrero"), "{}", aviso.detalle);
        let pedir: Vec<&str> = s.pedir.iter().map(|p| p.que.as_str()).collect();
        assert!(
            pedir
                .iter()
                .any(|p| p.contains("junio de 2025 a julio de 2025")
                    && p.contains("septiembre de 2025")
                    && !p.contains("febrero")),
            "{pedir:?}"
        );
        // La tabla de meses sigue con todos los huecos.
        assert!(tabla(&s, "Meses con extracto, por cuenta")[0][4].contains("febrero de 2025"));
        // 100 € de 10.000 € en dos meses: el 1 % y no más de 50 € al mes.
        assert!(
            Hueco {
                meses: vec![(2025, 2), (2025, 3)],
                antes: Some(Money::from_cents(1_000_000)),
                despues: Some(Money::from_cents(990_000)),
            }
            .plano()
        );
        // 100 € de 100.000 € en un mes: más de 50 € en un mes.

        assert!(
            !Hueco {
                meses: vec![(2025, 2)],
                antes: Some(Money::from_cents(10_000_000)),
                despues: Some(Money::from_cents(9_990_000)),
            }
            .plano()
        );
        assert!(
            !Hueco {
                meses: vec![(2025, 2)],
                antes: None,
                despues: Some(Money::from_cents(10_000)),
            }
            .plano()
        );
    }

    /// Los saldos de una cuenta en otra divisa van en su divisa, no en euros.
    #[test]
    fn el_ultimo_extracto_en_su_divisa() {
        let mut e = extracto(
            "Banco Demo",
            CUENTA,
            dia(2026, 1, 1),
            dia(2026, 3, 15),
            Some((10_000, 8_740)),
        );
        e.divisa = "USD".into();
        let s = revisar_en(vec![doc("usd.pdf", e)], dia(2026, 11, 1));
        let h = s
            .hallazgos
            .iter()
            .find(|h| h.codigo == "bancos.ultimo_extracto_antiguo")
            .unwrap();
        assert!(h.detalle.contains("87,40 USD"), "{}", h.detalle);
        assert!(!h.detalle.contains('€'), "{}", h.detalle);
    }
}
