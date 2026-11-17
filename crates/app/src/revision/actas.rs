//! Actas de la junta (o decisiones del socio único): la que aprueba las cuentas de un ejercicio y
//! cómo aplica su resultado, partida a partida (reserva legal, voluntarias, de nivelación, de
//! capitalización, dividendos, compensación de pérdidas, remanente), y las demás decisiones que
//! mueven reservas (un dividendo con cargo a reservas, una dotación posterior). Sale del bloque
//! `acta` de la lectura con IA o, si no lo trae, del texto del PDF (la tabla «Base de reparto /
//! Distribución» que llevan las actas y la memoria).

use std::sync::LazyLock;

use regex::Regex;
use taxpain_core::money::Money;
use time::{Date, Month};

use super::UN_EURO;
use super::expediente::Doc;
use crate::documentos::{DocKind, fold};
use crate::lectura_ia::Reading;

/// Lo que dice un acta de la junta sobre el resultado y las reservas.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Aplicacion {
    /// Ejercicio cuyas cuentas se aprueban (o al que se refiere la decisión).
    pub ejercicio: Option<i32>,
    /// Fecha del acta (la de la junta o la de la decisión del socio único).
    pub fecha: Option<Date>,
    /// Cierre del ejercicio, si el acta lo dice («cerrado el 30 de junio de 2025»); si no, se
    /// toma el 31 de diciembre.
    pub cierre: Option<Date>,
    /// Aprueba las cuentas y la aplicación del resultado. Si no, es otra decisión: un reparto
    /// con cargo a reservas o una dotación posterior.
    pub aprueba: bool,
    /// Leída entera: cada partida se reconoce, con el importe de la columna del ejercicio, y
    /// entre todas suman lo que se reparte. Si no, no se puede decir que no dota una reserva.
    pub completa: bool,
    /// Resultado que se reparte (negativo si son pérdidas).
    pub resultado: Option<Money>,
    pub reserva_legal: Option<Money>,
    pub reservas_voluntarias: Option<Money>,
    pub reserva_nivelacion: Option<Money>,
    pub reserva_capitalizacion: Option<Money>,
    pub dividendos: Option<Money>,
    pub compensacion_perdidas: Option<Money>,
    pub remanente: Option<Money>,
}

/// Una partida de la aplicación del resultado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Partida {
    Resultado,
    Legal,
    Voluntarias,
    Nivelacion,
    Capitalizacion,
    Dividendos,
    Compensacion,
    Remanente,
}

impl Aplicacion {
    fn hueco(&mut self, p: Partida) -> &mut Option<Money> {
        match p {
            Partida::Resultado => &mut self.resultado,
            Partida::Legal => &mut self.reserva_legal,
            Partida::Voluntarias => &mut self.reservas_voluntarias,
            Partida::Nivelacion => &mut self.reserva_nivelacion,
            Partida::Capitalizacion => &mut self.reserva_capitalizacion,
            Partida::Dividendos => &mut self.dividendos,
            Partida::Compensacion => &mut self.compensacion_perdidas,
            Partida::Remanente => &mut self.remanente,
        }
    }

    /// Las partidas a las que va el resultado, con su importe (sin el resultado).
    fn partidas(&self) -> [(&'static str, Option<Money>); 7] {
        [
            ("a reserva legal", self.reserva_legal),
            ("a reservas voluntarias", self.reservas_voluntarias),
            ("a reserva de nivelación", self.reserva_nivelacion),
            ("a reserva de capitalización", self.reserva_capitalizacion),
            ("a dividendos", self.dividendos),
            (
                "a compensar pérdidas de ejercicios anteriores",
                self.compensacion_perdidas,
            ),
            ("a remanente", self.remanente),
        ]
    }

    /// ¿Dice a qué partidas va el resultado? (Sin ninguna, el acta no se ha podido leer.)
    pub fn con_partidas(&self) -> bool {
        self.partidas().iter().any(|(_, m)| m.is_some())
    }

    /// Lo que suman las partidas.
    fn suma(&self) -> Money {
        self.partidas()
            .iter()
            .filter_map(|(_, m)| *m)
            .fold(Money::ZERO, |a, m| a + m)
    }

    /// Lo que va a la reserva de nivelación (cero si no va nada).
    pub fn nivelacion(&self) -> Money {
        self.reserva_nivelacion.unwrap_or(Money::ZERO)
    }

    /// Lo que queda libre (reservas voluntarias, dividendos y remanente): con eso se podía dotar
    /// una reserva que el acta no dota.
    pub fn libre(&self) -> Money {
        [self.reservas_voluntarias, self.dividendos, self.remanente]
            .into_iter()
            .flatten()
            .fold(Money::ZERO, |a, m| a + m)
    }

    /// «1.000,00 € a reserva legal y 9.000,00 € a reservas voluntarias».
    pub fn texto(&self) -> String {
        let partes: Vec<String> = self
            .partidas()
            .iter()
            .filter_map(|(que, m)| m.filter(|m| !m.is_zero()).map(|m| format!("{m} {que}")))
            .collect();
        match partes.as_slice() {
            [] => String::new(),
            [uno] => uno.clone(),
            [resto @ .., ultimo] => format!("{} y {ultimo}", resto.join(", ")),
        }
    }

    /// Último día del ejercicio `y`: el que dice el acta o, si no, el 31 de diciembre.
    fn cierre_de(&self, y: i32) -> Option<Date> {
        self.cierre
            .filter(|c| c.year() == y)
            .or_else(|| Date::from_calendar_date(y, Month::December, 31).ok())
    }

    /// ¿Es un acta del ejercicio `y`? Lo dice y, si tiene fecha, es posterior a su cierre: las
    /// cuentas se aprueban después de cerrar el ejercicio.
    pub fn del_ejercicio(&self, y: i32) -> bool {
        self.ejercicio == Some(y)
            && match (self.fecha, self.cierre_de(y)) {
                (Some(f), Some(c)) => f > c,
                _ => true,
            }
    }

    /// ¿Es el acta que aprueba las cuentas de `y`?
    pub fn aprueba_las_de(&self, y: i32) -> bool {
        self.aprueba && self.del_ejercicio(y)
    }

    /// Las partidas suman `total` (con un euro de margen).
    fn suma_bien(&self, total: Money) -> bool {
        (self.suma() - total).abs() <= UN_EURO
    }
}

/// La partida de una línea ya plegada con [`fold`] (minúsculas, sin tildes). Una reserva que
/// cita su artículo («reserva indisponible art. 105 LIS») es la de ese artículo.
fn partida(linea: &str) -> Option<Partida> {
    static ARTICULO: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"\bart(?:iculo|\.)?\s*(105|25)\b").ok());
    let has = |s: &str| linea.contains(s);
    if has("total") {
        return None;
    }
    let articulo = ARTICULO
        .as_ref()
        .and_then(|re| re.captures(linea))
        .map(|c| c[1].to_owned());
    let reserva = has("reserva");
    Some(
        if has("nivelacion") || (reserva && articulo.as_deref() == Some("105")) {
            Partida::Nivelacion
        } else if has("capitalizacion") || (reserva && articulo.as_deref() == Some("25")) {
            Partida::Capitalizacion
        } else if has("reserva legal") {
            Partida::Legal
        } else if has("reservas voluntarias")
            || has("reserva voluntaria")
            || has("reservas de libre disposicion")
        {
            Partida::Voluntarias
        } else if has("compensa") && (has("perdidas") || has("negativos")) {
            Partida::Compensacion
        } else if has("dividendo") {
            Partida::Dividendos
        } else if has("remanente") {
            Partida::Remanente
        } else if has("perdidas y ganancias")
            || has("resultado del ejercicio")
            || has("beneficio del ejercicio")
            || has("beneficios del ejercicio")
        {
            Partida::Resultado
        } else {
            return None;
        },
    )
}

/// ¿Aprueba el texto (ya plegado) las cuentas de un ejercicio? Lo dice la aplicación de su
/// resultado (art. 273.1 LSC: la junta resuelve sobre ella al aprobar el balance). Un reparto
/// de dividendos con cargo a reservas no la lleva.
fn aprueba_cuentas(t: &str) -> bool {
    [
        "aplicacion del resultado",
        "distribucion del resultado",
        "propuesta de aplicacion",
        "aplicacion de resultados",
        "distribucion de resultados",
        "aplicacion del beneficio",
        "distribucion del beneficio",
    ]
    .iter()
    .any(|s| t.contains(s))
        || (t.contains("cuentas anuales")
            && (t.contains("aprob") || t.contains("aprueb"))
            && (t.contains("perdidas y ganancias") || t.contains("resultado del ejercicio")))
}

/// ¿Dice la descripción de una lectura que el acta aprueba las cuentas?
fn menciona_aprobacion(descripcion: &str) -> bool {
    let t = fold(descripcion);
    t.contains("cuentas") && (t.contains("aprob") || t.contains("aprueb"))
}

/// El mes por su nombre ya plegado.
fn mes(nombre: &str) -> Option<Month> {
    Some(match nombre {
        "enero" => Month::January,
        "febrero" => Month::February,
        "marzo" => Month::March,
        "abril" => Month::April,
        "mayo" => Month::May,
        "junio" => Month::June,
        "julio" => Month::July,
        "agosto" => Month::August,
        "septiembre" | "setiembre" => Month::September,
        "octubre" => Month::October,
        "noviembre" => Month::November,
        "diciembre" => Month::December,
        _ => return None,
    })
}

/// El ejercicio cuyas cuentas aprueba el texto (ya plegado), con su cierre si lo dice. Cuenta más
/// el año que va junto a «cuentas anuales», después el que va junto al resultado y después el de
/// cualquier «ejercicio 2025» (o «ejercicio social cerrado el 31 de diciembre de 2025»). Con la
/// fecha del acta, sólo un ejercicio ya cerrado: «la retribución para el ejercicio 2026» de un
/// acta de junio de 2026 no es el que se aprueba.
fn ejercicio_de(plano: &str, fecha: Option<Date>) -> (Option<i32>, Option<Date>) {
    static AÑO: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\b20\d{2}\b").ok());
    static EJERCICIO: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(
            r"ejercicio(?: social| economico)?(?: (?:cerrado|finalizado|terminado) (?:el|a|al) \d{1,2} de [a-z]+ de)?\s*$",
        )
        .ok()
    });
    static CIERRE: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(r"(?:cerrado|finalizado|terminado) (?:el|a|al) (\d{1,2}) de ([a-z]+) de\s*$")
            .ok()
    });
    let (Some(año), Some(ejercicio), Some(cierre)) =
        (AÑO.as_ref(), EJERCICIO.as_ref(), CIERRE.as_ref())
    else {
        return (None, None);
    };
    let mut mejor: Option<(u8, i32, Option<Date>)> = None;
    for m in año.find_iter(plano) {
        let Ok(y) = m.as_str().parse::<i32>() else {
            continue;
        };
        let p = m.start();
        let desde = plano[..p]
            .char_indices()
            .rev()
            .nth(90)
            .map_or(0, |(i, _)| i);
        let antes = &plano[desde..p];
        let prioridad = if antes.contains("cuentas anuales") {
            1
        } else if antes.contains("resultado") {
            2
        } else if ejercicio.is_match(antes) {
            3
        } else {
            continue;
        };
        let dicho = cierre
            .captures(antes)
            .and_then(|c| Date::from_calendar_date(y, mes(&c[2])?, c[1].parse().ok()?).ok());
        let fin = dicho.or_else(|| Date::from_calendar_date(y, Month::December, 31).ok());
        if fecha.zip(fin).is_some_and(|(f, fin)| f <= fin) {
            continue;
        }
        if mejor.is_none_or(|(pr, _, _)| prioridad < pr) {
            mejor = Some((prioridad, y, dicho));
        }
    }
    mejor.map_or((None, None), |(_, y, c)| (Some(y), c))
}

/// Los años de una cabecera de columnas («DISTRIBUCIÓN 2025 2024», «2025 2024»): una línea con
/// algún año y, aparte, sólo palabras de cabecera.
fn cabecera(linea: &str) -> Option<Vec<i32>> {
    const PALABRAS: [&str; 18] = [
        "base",
        "de",
        "reparto",
        "distribucion",
        "aplicacion",
        "del",
        "resultado",
        "ejercicio",
        "ejercicios",
        "importe",
        "importes",
        "euros",
        "eur",
        "€",
        "propuesta",
        "la",
        "el",
        "y",
    ];
    let mut años = Vec::new();
    for w in linea
        .split(|c: char| c.is_whitespace() || matches!(c, ':' | '(' | ')' | '.' | ','))
        .filter(|w| !w.is_empty())
    {
        match w.parse::<i32>() {
            Ok(y) if (2000..2100).contains(&y) && w.len() == 4 => años.push(y),
            _ if PALABRAS.contains(&w) => {}
            _ => return None,
        }
    }
    (!años.is_empty()).then_some(años)
}

/// Los importes de una línea («1.000,00», «(500,00)», «-500,00 €»), en orden.
fn importes(linea: &str) -> Vec<Money> {
    static IMPORTE: LazyLock<Option<Regex>> =
        LazyLock::new(|| Regex::new(r"^\(?-?\d{1,3}(?:\.\d{3})*,\d{2}\)?$").ok());
    let Some(re) = IMPORTE.as_ref() else {
        return Vec::new();
    };
    linea
        .split_whitespace()
        .map(|w| w.trim_end_matches(['€', ';']))
        .filter(|w| re.is_match(w))
        .filter_map(|w| {
            let negativo = w.starts_with('(') || w.starts_with('-');
            let m = crate::import::parse_amount(w.trim_matches(['(', ')', '-']))?;
            Some(if negativo { -m } else { m })
        })
        .collect()
}

/// El importe de una línea en la columna del ejercicio. `Err` si no se sabe cuál es: varios
/// importes sin una cabecera con el mismo número de años que diga cuál es la del ejercicio, o
/// uno solo en una tabla de varias columnas o de otro año.
fn de_su_columna(
    linea: &str,
    columnas: &[i32],
    ejercicio: Option<i32>,
) -> Result<Option<Money>, ()> {
    let im = importes(linea);
    match (im.as_slice(), columnas) {
        ([], _) => Ok(None),
        ([m], []) => Ok(Some(*m)),
        ([m], [y]) if Some(*y) == ejercicio => Ok(Some(*m)),
        (varios, cols) if varios.len() == cols.len() && cols.len() >= 2 => cols
            .iter()
            .position(|y| Some(*y) == ejercicio)
            .map(|i| Some(varios[i]))
            .ok_or(()),
        _ => Err(()),
    }
}

/// En qué parte del texto se está.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Seccion {
    /// Antes de la tabla: sólo cuentan las líneas que son partidas conocidas.
    Antes,
    /// La base de reparto: de dónde sale lo que se reparte.
    Base,
    /// La distribución: adónde va. Toda línea con importe tiene que ser una partida conocida.
    Reparto,
    /// Después del total distribuido.
    Despues,
}

/// ¿Empieza un acuerdo nuevo («TERCERO.», «Cuarta»)? Acaba la tabla.
fn otro_acuerdo(linea: &str) -> bool {
    [
        "tercer",
        "cuart",
        "quint",
        "sext",
        "septim",
        "octav",
        "noven",
        "decim",
        "no habiendo",
        "y no habiendo",
    ]
    .iter()
    .any(|p| linea.starts_with(p))
}

/// La aplicación del resultado por el texto del acta: las líneas de la tabla de distribución
/// («A reserva legal ........ 1.000,00»), cada partida con el importe de la columna del
/// ejercicio. `fecha`: la del acta, si se sabe, para elegir el ejercicio que aprueba.
pub(crate) fn de_texto(texto: &str, fecha: Option<Date>) -> Option<Aplicacion> {
    let plano = fold(texto);
    if ![
        "aplicacion del resultado",
        "distribucion del resultado",
        "base de reparto",
        "propuesta de aplicacion",
        "distribucion",
    ]
    .iter()
    .any(|s| plano.contains(s))
    {
        return None;
    }
    let (ejercicio, cierre) = ejercicio_de(&plano, fecha);
    let mut a = Aplicacion {
        ejercicio: ejercicio.or(fecha.map(|f| f.year() - 1)),
        fecha,
        cierre,
        aprueba: aprueba_cuentas(&plano),
        completa: true,
        ..Aplicacion::default()
    };
    let mut columnas: Vec<i32> = Vec::new();
    let mut seccion = Seccion::Antes;
    // Lo que se reparte según la base de reparto (el resultado y, si los hay, el remanente o las
    // reservas de las que sale) y los totales de la tabla.
    let mut base = Money::ZERO;
    let mut total_base = None;
    let mut total = None;
    // El principio del nombre de una partida que sigue en la línea siguiente.
    let mut partido = String::new();
    for linea in texto.lines() {
        let l = fold(linea);
        let l = l.trim();
        if l.is_empty() {
            continue;
        }
        if let Some(años) = cabecera(l) {
            columnas = años;
        }
        if l.contains("base de reparto") {
            seccion = Seccion::Base;
            continue;
        }
        if seccion != Seccion::Despues
            && (l.contains("distribucion")
                || (seccion == Seccion::Base && l.contains("aplicacion")))
        {
            seccion = Seccion::Reparto;
            continue;
        }
        if seccion == Seccion::Reparto && otro_acuerdo(l) {
            seccion = Seccion::Despues;
        }
        if seccion == Seccion::Despues {
            continue;
        }
        let Ok(importe) = de_su_columna(l, &columnas, a.ejercicio) else {
            if seccion != Seccion::Antes {
                a.completa = false;
            }
            continue;
        };
        let Some(m) = importe else {
            if seccion == Seccion::Reparto && cabecera(l).is_none() {
                partido = format!("{partido} {l}");
            }
            continue;
        };
        let nombre = format!("{partido} {l}");
        partido.clear();
        let p = partida(&nombre);
        match seccion {
            Seccion::Base => {
                if nombre.contains("total") {
                    total_base = total_base.or(Some(m));
                    continue;
                }
                base += m;
                if p == Some(Partida::Resultado) {
                    a.resultado = a.resultado.or(Some(m));
                }
            }
            Seccion::Reparto => {
                if nombre.contains("total") {
                    total = Some(m);
                    seccion = Seccion::Despues;
                    continue;
                }
                match p {
                    Some(Partida::Resultado) | None => a.completa = false,
                    Some(p) => {
                        let hueco = a.hueco(p);
                        *hueco = Some(hueco.unwrap_or(Money::ZERO) + m);
                    }
                }
            }
            Seccion::Antes => match p {
                Some(Partida::Resultado) => {
                    base += m;
                    a.resultado = a.resultado.or(Some(m));
                }
                // Una partida conocida sin la cabecera «Distribución»: empieza la tabla.
                Some(p) => {
                    seccion = Seccion::Reparto;
                    *a.hueco(p) = Some(m);
                }
                None => {}
            },
            Seccion::Despues => {}
        }
    }
    // Sin ninguna partida, no hay tabla; salvo que la haya y no se haya podido leer (importes en
    // columnas sin saber cuál es la del ejercicio): entonces es un acta que no se lee entera.
    if !a.con_partidas() && a.completa {
        return None;
    }
    // Lo que se reparte: el total distribuido o, sin él, la base de reparto o el resultado.
    let repartido = total
        .or(total_base)
        .or((!base.is_zero()).then_some(base))
        .or(a.resultado);
    a.completa = a.completa && a.resultado.is_some() && repartido.is_some_and(|t| a.suma_bien(t));
    Some(a)
}

/// La aplicación del resultado del bloque `acta` de una lectura, y si dice que aprueba las
/// cuentas («aprueba_cuentas»), si lo dice.
pub(crate) fn de_lectura(j: &serde_json::Value) -> Option<(Aplicacion, Option<bool>)> {
    let texto = |k: &str| {
        j.get(k)
            .and_then(|v| match v {
                serde_json::Value::String(s) => Some(s.trim().to_owned()),
                serde_json::Value::Number(n) => Some(n.to_string()),
                serde_json::Value::Bool(b) => Some(b.to_string()),
                _ => None,
            })
            .filter(|s| !s.is_empty() && s != "null")
    };
    let importe = |k: &str| texto(k).and_then(|s| crate::received::parse_amount(&s));
    let mut a = Aplicacion {
        ejercicio: texto("ejercicio").and_then(|y| y.parse().ok()),
        fecha: texto("fecha").and_then(|s| crate::import::parse_date(&s)),
        resultado: importe("resultado"),
        reserva_legal: importe("reserva_legal"),
        reservas_voluntarias: importe("reservas_voluntarias"),
        reserva_nivelacion: importe("reserva_nivelacion"),
        reserva_capitalizacion: importe("reserva_capitalizacion"),
        dividendos: importe("dividendos"),
        compensacion_perdidas: importe("compensacion_perdidas"),
        remanente: importe("remanente"),
        ..Aplicacion::default()
    };
    if !a.con_partidas() {
        return None;
    }
    a.completa = a.resultado.is_some_and(|r| a.suma_bien(r));
    let dicho = texto("aprueba_cuentas").and_then(|s| match fold(&s).as_str() {
        "true" | "si" | "1" => Some(true),
        "false" | "no" | "0" => Some(false),
        _ => None,
    });
    Some((a, dicho))
}

/// Completa un acta de la junta de la carpeta con lo que dice del resultado y las reservas: lo
/// del bloque `acta` de su lectura o, si no lo trae, lo del texto del PDF. La fecha, si no la
/// dice, es la del documento; el ejercicio, si no lo dice, el anterior al del acta (las cuentas
/// se aprueban al año siguiente). Un bloque que no dice si aprueba las cuentas las aprueba si
/// reparte un resultado o si lo dicen el texto del PDF o la descripción.
pub(crate) fn completar(d: &mut Doc, bytes: &[u8], lectura: Option<&Reading>) {
    let Some((DocKind::ActaJunta, facts)) = &d.empresa else {
        return;
    };
    let fecha = facts.date;
    let pdf = bytes
        .starts_with(b"%PDF")
        .then(|| crate::pdf_text::text(bytes))
        .flatten();
    let bloque = lectura.and_then(|r| r.acta.as_ref()).and_then(de_lectura);
    d.aplicacion = match bloque {
        Some((mut a, dicho)) => {
            a.fecha = a.fecha.or(fecha);
            a.ejercicio = a.ejercicio.or(a.fecha.map(|f| f.year() - 1));
            a.aprueba = dicho.unwrap_or_else(|| {
                a.resultado.is_some()
                    || pdf.as_deref().is_some_and(|t| aprueba_cuentas(&fold(t)))
                    || lectura.is_some_and(|r| menciona_aprobacion(&r.descripcion))
            });
            Some(a)
        }
        None => pdf.as_deref().and_then(|t| de_texto(t, fecha)),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eur(c: i64) -> Option<Money> {
        Some(Money::from_cents(c))
    }

    fn dia(y: i32, m: Month, d: u8) -> Option<Date> {
        Date::from_calendar_date(y, m, d).ok()
    }

    const ACTA: &str = "ACTA DE LA DECISIÓN DEL SOCIO ÚNICO DE EMPRESA DEMO SL\n\
        En Valencia, a 30 de junio de 2026, el socio único adopta las siguientes decisiones:\n\
        PRIMERA. Aprueba las cuentas anuales del ejercicio social cerrado el 31 de diciembre \
        de 2025.\n\
        SEGUNDA. Aprobación de la propuesta de aplicación del resultado del ejercicio 2025\n\
        BASE DE REPARTO   2025\n\
          Pérdidas y ganancias        20.000,00\n\
          Total        20.000,00\n\
        DISTRIBUCIÓN   2025\n\
          A reserva legal            1.000,00\n\
        A reservas voluntarias        18.500,00\n\
        A compensación de pérdidas de ejercicios anteriores          500,00\n\
          Total distribuido        20.000,00\n\
        TERCERA. Aprueba la gestión del administrador, que cobrará 12.000,00 euros.\n";

    #[test]
    fn aplicacion_del_resultado_por_el_texto_del_acta() {
        let a = de_texto(ACTA, None).unwrap();
        assert_eq!(a.ejercicio, Some(2025));
        assert!(a.aprueba && a.completa);
        assert_eq!(a.resultado, eur(2_000_000));
        assert_eq!(a.reserva_legal, eur(100_000));
        assert_eq!(a.reservas_voluntarias, eur(1_850_000));
        assert_eq!(a.compensacion_perdidas, eur(50_000));
        assert_eq!(a.reserva_nivelacion, None);
        assert_eq!(a.nivelacion(), Money::ZERO);
        assert_eq!(a.libre(), Money::from_cents(1_850_000));
        assert_eq!(
            a.texto(),
            "1.000,00 € a reserva legal, 18.500,00 € a reservas voluntarias y 500,00 € a \
             compensar pérdidas de ejercicios anteriores"
        );
        // Con la reserva de nivelación en la tabla.
        let con = ACTA.replace(
            "A reservas voluntarias        18.500,00",
            "A reserva de nivelación de bases imponibles   2.000,00\nA reservas voluntarias  16.500,00",
        );
        let a = de_texto(&con, None).unwrap();
        assert_eq!(a.reserva_nivelacion, eur(200_000));
        assert_eq!(a.reservas_voluntarias, eur(1_650_000));
        assert!(a.completa);
        // El nombre de una partida partido en dos líneas.
        let partido = ACTA.replace(
            "A compensación de pérdidas de ejercicios anteriores          500,00",
            "A compensación de pérdidas de\nejercicios anteriores          500,00",
        );
        let a = de_texto(&partido, None).unwrap();
        assert_eq!(a.compensacion_perdidas, eur(50_000));
        assert!(a.completa);
        // El remanente de la base de reparto es de dónde sale el dinero, no adónde va.
        let con_remanente = "Aplicación del resultado\nBASE DE REPARTO\n\
            Pérdidas y ganancias  20.000,00\nRemanente  5.000,00\nDISTRIBUCIÓN\n\
            A dividendos  25.000,00\nTotal distribuido 25.000,00\n";
        let a = de_texto(con_remanente, None).unwrap();
        assert_eq!(a.remanente, None);
        assert_eq!(a.dividendos, eur(2_500_000));
        assert_eq!(a.resultado, eur(2_000_000));
        assert!(a.completa);
        // Un acta sin la tabla de distribución (otra junta) no dice nada.
        assert!(
            de_texto(
                "Acta de la junta general extraordinaria: cambio de domicilio",
                None
            )
            .is_none()
        );
    }

    /// Una partida que no se reconoce, o unas partidas que no suman lo que se reparte: el acta no
    /// se ha leído entera. Una reserva con su artículo sí se reconoce.
    #[test]
    fn el_acta_que_no_se_lee_entera() {
        let otra = ACTA.replace("A reservas voluntarias", "A reservas especiales");
        let a = de_texto(&otra, None).unwrap();
        assert!(!a.completa);
        assert_eq!(a.reservas_voluntarias, None);
        let falta = ACTA.replace("A reserva legal            1.000,00\n", "");
        assert!(!de_texto(&falta, None).unwrap().completa);
        let con_articulo = ACTA.replace(
            "A reserva legal            1.000,00",
            "A reserva legal  500,00\nA reserva indisponible (art. 105 LIS)  500,00",
        );
        let a = de_texto(&con_articulo, None).unwrap();
        assert_eq!(a.reserva_nivelacion, eur(50_000));
        assert!(a.completa);
        // Sin el resultado no se sabe qué se reparte.
        let sin = "DISTRIBUCIÓN\nA reserva legal 1.000,00\nA reservas voluntarias 9.000,00\n";
        assert!(!de_texto(sin, None).unwrap().completa);
    }

    /// Con la columna del año anterior, vale la del ejercicio; si la cabecera no dice cuál es,
    /// no se sabe.
    #[test]
    fn la_columna_del_ejercicio() {
        let dos = "Aprobación de la propuesta de aplicación del resultado del ejercicio 2025\n\
            BASE DE REPARTO 2025 2024\nPérdidas y ganancias 20.000,00 6.000,00\n\
            DISTRIBUCIÓN 2025 2024\nA reserva legal 1.000,00 600,00\n\
            A reserva de nivelación 2.000,00 0,00\nA reservas voluntarias 17.000,00 5.400,00\n\
            Total distribuido 20.000,00 6.000,00\n";
        let a = de_texto(dos, dia(2026, Month::June, 30)).unwrap();
        assert_eq!(a.ejercicio, Some(2025));
        assert_eq!(a.reserva_nivelacion, eur(200_000));
        assert_eq!(a.reservas_voluntarias, eur(1_700_000));
        assert!(a.completa);
        // Las columnas en otro orden.
        let al_reves = dos
            .replace("2025 2024", "2024 2025")
            .replace("20.000,00 6.000,00", "6.000,00 20.000,00")
            .replace("1.000,00 600,00", "600,00 1.000,00")
            .replace("2.000,00 0,00", "0,00 2.000,00")
            .replace("17.000,00 5.400,00", "5.400,00 17.000,00");
        assert_eq!(
            de_texto(&al_reves, dia(2026, Month::June, 30))
                .unwrap()
                .reserva_nivelacion,
            eur(200_000)
        );
        // Dos importes sin cabecera: no se sabe cuál es el del ejercicio.
        let sin_cabecera = dos.replace(" 2025 2024", "");
        let a = de_texto(&sin_cabecera, dia(2026, Month::June, 30)).unwrap();
        assert!(!a.completa);
        assert_eq!(a.reserva_nivelacion, None);
    }

    /// El ejercicio es el de las cuentas que se aprueban, ya cerrado en la fecha del acta: no el
    /// de otro acuerdo del acta.
    #[test]
    fn el_ejercicio_de_las_cuentas() {
        let t = "En Valencia, a 30 de junio de 2026. PRIMERO. Aprobar las cuentas anuales del \
            ejercicio económico 2025. SEGUNDO. Aplicación del resultado. DISTRIBUCIÓN\n\
            A reservas voluntarias 9.000,00\n\
            TERCERO. Fijar la retribución del administrador para el ejercicio 2026.";
        let a = de_texto(t, dia(2026, Month::June, 30)).unwrap();
        assert_eq!(a.ejercicio, Some(2025));
        // Sin la fecha del acta, cuenta más el año de las cuentas anuales.
        assert_eq!(de_texto(t, None).unwrap().ejercicio, Some(2025));
        // Un ejercicio que no acaba el 31 de diciembre.
        let t = "La junta aprueba las cuentas anuales del ejercicio cerrado a 30 de junio de \
            2025 y la propuesta de aplicación del resultado. DISTRIBUCIÓN\n\
            A reservas voluntarias 9.000,00\n";
        let a = de_texto(t, dia(2025, Month::November, 15)).unwrap();
        assert_eq!(a.ejercicio, Some(2025));
        assert_eq!(a.cierre, dia(2025, Month::June, 30));
        assert!(a.del_ejercicio(2025));
        // Un acta anterior al cierre no es de ese ejercicio.
        let mut antes = a.clone();
        antes.fecha = dia(2025, Month::March, 1);
        assert!(!antes.del_ejercicio(2025));
    }

    /// Una junta que reparte un dividendo con cargo a reservas no aprueba cuentas.
    #[test]
    fn la_junta_que_reparte_reservas() {
        let t = "ACTA DE LA JUNTA GENERAL EXTRAORDINARIA Y UNIVERSAL DE EMPRESA DEMO SL\n\
            En Valencia, a 10 de marzo de 2026, los socios acuerdan la distribución de un \
            dividendo con cargo a reservas voluntarias:\nDividendo a repartir 6.300,00\n";
        let a = de_texto(t, dia(2026, Month::March, 10)).unwrap();
        assert!(!a.aprueba);
        assert!(!a.aprueba_las_de(2025));
        assert!(
            de_texto(ACTA, dia(2026, Month::June, 30))
                .unwrap()
                .aprueba_las_de(2025)
        );
    }

    #[test]
    fn aplicacion_del_resultado_de_la_lectura() {
        let (a, dicho) = de_lectura(&serde_json::json!({
            "ejercicio": 2025, "fecha": "2026-06-30", "resultado": "20000.00",
            "reserva_legal": "1000.00", "reservas_voluntarias": "17000.00",
            "reserva_nivelacion": "2000.00", "dividendos": null
        }))
        .unwrap();
        assert_eq!(a.ejercicio, Some(2025));
        assert_eq!(a.fecha, dia(2026, Month::June, 30));
        assert_eq!(a.nivelacion(), Money::from_cents(200_000));
        assert_eq!(a.dividendos, None);
        assert!(a.completa);
        assert_eq!(dicho, None);
        // Una decisión posterior que dice que no aprueba las cuentas.
        let (a, dicho) = de_lectura(&serde_json::json!({
            "ejercicio": 2025, "aprueba_cuentas": false, "reserva_nivelacion": "2000.00",
            "reservas_voluntarias": "-2000.00"
        }))
        .unwrap();
        assert_eq!(dicho, Some(false));
        assert!(!a.completa);
        // Sin ninguna partida, no hay aplicación que mirar.
        assert!(de_lectura(&serde_json::json!({"ejercicio": 2025})).is_none());
    }
}
