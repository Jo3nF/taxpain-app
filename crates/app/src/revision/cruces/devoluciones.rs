//! Devoluciones que pidió la empresa (el 303 del último periodo o el 200 a devolver): si se ven
//! cobradas en el banco y si Hacienda tardó más de seis meses, que es cuando le tocan intereses
//! de demora.

use std::collections::BTreeMap;

use taxpain_core::money::Money;
use time::{Date, Duration};

use super::super::modelos::{Declarado, declarados};
use super::super::sede::mas_meses;
use super::super::{Area, Gravedad, Nuevo, Responsable, Salida, eur, fecha, lista, plural};
use super::datos::MovBanco;
use super::datos::movimientos;
use super::pagos::de_hacienda;
use super::{Contexto, a_ingresar, nombre, primera};

/// Días entre la orden de pago de la AEAT y el abono en la cuenta.
const MARGEN_ABONO: i64 = 5;

/// ¿Se pidió la devolución? Lo dice la portada del justificante («Solicitud de devolución»).
/// Sin ella, por las casillas: el 200 con resultado negativo (salvo una renuncia, que es rara) y
/// el 303 del último periodo que aplica todo lo pendiente de compensar ([78] = [110], sin nada en
/// la [87]).
fn pidio_devolucion(m: &str, p: &str, d: &Declarado, resultado: Option<Money>) -> bool {
    if resultado.is_some_and(|r| r >= Money::ZERO) {
        return false;
    }
    if let Some(v) = d.devolucion {
        return v;
    }
    let Some(r) = resultado else {
        return false;
    };
    match m {
        "200" => r < Money::ZERO,
        "303" => {
            matches!(p, "4T" | "12")
                && d.c(78) > Money::ZERO
                && d.c(78) == d.c(110)
                && d.casillas.get(&87).is_none_or(|x| x.is_zero())
        }
        _ => false,
    }
}

/// «…1234»: las últimas cifras de la cuenta.
fn cuenta_corta(c: &str) -> String {
    let d: Vec<char> = c.chars().filter(char::is_ascii_alphanumeric).collect();
    if d.len() > 4 {
        format!("…{}", d[d.len() - 4..].iter().collect::<String>())
    } else {
        d.into_iter().collect()
    }
}

/// «4T 2021» o «2022».
fn periodo(y: i32, p: &str) -> String {
    if p == "0A" {
        y.to_string()
    } else {
        format!("{p} {y}")
    }
}

/// Tramos con extractos de cada cuenta, juntando los seguidos (un extracto por mes).
fn cobertura(tramos: &[(String, Date, Date)]) -> BTreeMap<&str, Vec<(Date, Date)>> {
    let mut por_cuenta: BTreeMap<&str, Vec<(Date, Date)>> = BTreeMap::new();
    for (c, a, b) in tramos {
        por_cuenta.entry(c.as_str()).or_default().push((*a, *b));
    }
    for v in por_cuenta.values_mut() {
        v.sort();
        let mut juntos: Vec<(Date, Date)> = Vec::new();
        for (a, b) in v.drain(..) {
            match juntos.last_mut() {
                Some((_, fin)) if a <= *fin + Duration::days(3) => *fin = (*fin).max(b),
                _ => juntos.push((a, b)),
            }
        }
        *v = juntos;
    }
    por_cuenta
}

/// El abono de la devolución: del mismo importe o, si llegó tarde, con algo más (los intereses),
/// desde que se presentó hasta un año después de los seis meses.
fn abono(movs: &[MovBanco], importe: Money, presentada: Date, limite: Date) -> Option<&MovBanco> {
    let hasta = limite + Duration::days(400);
    let con_intereses = |mv: &MovBanco| {
        mv.fecha > limite
            && mv.importe > importe
            && mv.importe.cents() <= importe.cents() + importe.cents() / 10
            && (de_hacienda(&mv.concepto) || mv.concepto.to_uppercase().contains("DEVOL"))
    };
    movs.iter()
        .filter(|mv| presentada <= mv.fecha && mv.fecha <= hasta)
        .filter(|mv| mv.importe == importe || con_intereses(mv))
        .min_by_key(|mv| (mv.importe != importe, mv.fecha))
}

pub(super) fn revisar(cx: &Contexto<'_>, s: &mut Salida) {
    let desde = super::super::revisable_desde(cx.hoy);
    let dec = declarados(cx.exp);
    let mut pedidas = Vec::new();
    for ((m, y, p), d) in &dec {
        if *y < desde - 1 {
            continue;
        }
        let resultado = a_ingresar(m, d);
        if !pidio_devolucion(m, p, d, resultado) {
            continue;
        }
        let Some(plazo) = cx.plazo(m, *y, p) else {
            continue;
        };
        pedidas.push((m, *y, p, d, resultado.map(|r| -r), plazo.due_on));
    }
    if pedidas.is_empty() {
        return;
    }
    let (movs, tramos) = movimientos(cx.exp);
    let cubiertas = cobertura(&tramos);
    let cubre = |a: Date, b: Date| {
        cubiertas
            .values()
            .any(|v| v.iter().any(|(x, y)| *x <= a && b <= *y))
    };
    let mut filas = Vec::new();
    let mut sin_extracto = Vec::new();
    for (m, y, p, d, importe, due) in pedidas {
        let cual = nombre(m, y, p);
        let presentada = d.presentado.unwrap_or(due);
        // El plazo para devolver empieza al acabar el de presentación, o al presentarla si fue
        // fuera de plazo (art. 125.2 LGT); pasados seis meses, la AEAT debe intereses de demora
        // sin que se pidan (art. 31.2 LGT).
        let Some(limite) = mas_meses(presentada.max(due), 6) else {
            continue;
        };
        let mut fila = vec![
            m.clone(),
            periodo(y, p),
            importe.map_or_else(|| "sin leer".to_owned(), eur),
            fecha(presentada),
            fecha(limite),
        ];
        let Some(importe) = importe else {
            fila.extend([
                String::new(),
                "no se ha podido leer el importe de la declaración".to_owned(),
            ]);
            filas.push(fila);
            continue;
        };
        if let Some(mv) = abono(&movs, importe, presentada, limite) {
            let tarde = mv.fecha > limite + Duration::days(MARGEN_ABONO);
            let extra = mv.importe - importe;
            fila.extend([
                format!("{} ({})", fecha(mv.fecha), cuenta_corta(&mv.cuenta)),
                if tarde {
                    "cobrada después de los seis meses".to_owned()
                } else {
                    "cobrada".to_owned()
                },
            ]);
            filas.push(fila);
            if tarde {
                let intereses = if extra > Money::ZERO {
                    format!(
                        " El abono fue de {}: los {} de más deben ser los intereses.",
                        eur(mv.importe),
                        eur(extra)
                    )
                } else {
                    " El abono fue sólo de lo pedido: reclama los intereses en la Sede de la \
                     AEAT."
                        .to_owned()
                };
                s.add(
                    Nuevo {
                        codigo: "aeat.devolucion_tarde",
                        area: Area::Aeat,
                        gravedad: Gravedad::Aviso,
                        responsable: Responsable::Empresa,
                        titulo: format!(
                            "La devolución {} se cobró después de los seis meses",
                            de(&cual)
                        ),
                        ficheros: vec![d.fichero.clone(), mv.origen.clone()],
                    }
                    .detalle(format!(
                        "Se pidieron {} y Hacienda tenía hasta el {} para devolverlos sin \
                         intereses: seis meses desde el fin del plazo de presentación, o desde \
                         la presentación si fue fuera de plazo (arts. 31.2 y 125.2 LGT). Se \
                         cobraron el {}. Pasado ese plazo, la AEAT debe intereses de demora sin \
                         que haga falta pedirlos (art. 31.2 LGT).{intereses}",
                        eur(importe),
                        fecha(limite),
                        fecha(mv.fecha)
                    )),
                    Some(y),
                    None,
                );
            }
            continue;
        }
        if cx.hoy <= limite {
            fila.extend([
                String::new(),
                format!("pendiente: Hacienda tiene hasta el {}", fecha(limite)),
            ]);
            filas.push(fila);
            continue;
        }
        if !cubre(presentada, limite.min(cx.hoy)) {
            sin_extracto.push(format!("{} ({})", primera(&cual), eur(importe)));
            fila.extend([String::new(), "sin extractos de esas fechas".to_owned()]);
            filas.push(fila);
            continue;
        }
        fila.extend([String::new(), "no se ve el abono".to_owned()]);
        filas.push(fila);
        s.add(
            Nuevo {
                codigo: "aeat.devolucion_no_esta",
                area: Area::Aeat,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Documentacion,
                titulo: format!("No se ve el cobro de la devolución {}", de(&cual)),
                ficheros: vec![d.fichero.clone()],
            }
            .detalle(format!(
                "Se pidieron {} a devolver el {} y en los extractos de la carpeta no hay ningún \
                 abono de ese importe. Puede haberse cobrado en otra cuenta (la que se dio en la \
                 declaración) o seguir pendiente; si sigue pendiente, desde el {} Hacienda debe \
                 intereses de demora (art. 31.2 LGT). Qué hacer: míralo en la Sede de la AEAT \
                 («Mis expedientes») y añade a la carpeta el extracto de la cuenta del cobro.",
                eur(importe),
                fecha(presentada),
                fecha(limite)
            )),
            Some(y),
            None,
        );
    }
    if !sin_extracto.is_empty() {
        s.add(
            Nuevo {
                codigo: "aeat.devoluciones_sin_extracto",
                area: Area::Aeat,
                gravedad: Gravedad::Info,
                responsable: Responsable::Documentacion,
                titulo: format!(
                    "{} sin extracto del banco para comprobar el cobro",
                    primera(&plural(sin_extracto.len(), "devolución", "devoluciones"))
                ),
                ficheros: vec![],
            }
            .detalle(format!(
                "No hay extractos de todas las fechas en que se podían cobrar: {}. Añade a la \
                 carpeta los extractos de esos meses para comprobar que se cobraron.",
                lista(&sin_extracto, 8)
            )),
            None,
            None,
        );
    }
    s.tabla(
        "Devoluciones pedidas",
        "Devoluciones",
        &[
            "Modelo",
            "Periodo",
            "A devolver",
            "Presentada",
            "Sin intereses hasta",
            "Abono en el banco",
            "Estado",
        ],
        filas,
    );
}

/// «de» + «el 200…» → «del 200…».
fn de(cual: &str) -> String {
    match cual.strip_prefix("el ") {
        Some(resto) => format!("del {resto}"),
        None => format!("de {cual}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn la_cuenta_por_sus_ultimas_cifras() {
        assert_eq!(cuenta_corta("ES91 2100 0418 4502 0005 1332"), "…1332");
        assert_eq!(cuenta_corta("123"), "123");
    }

    #[test]
    fn tramos_seguidos_de_una_cuenta() {
        let t = vec![
            (
                "ES1".to_owned(),
                date!(2025 - 01 - 01),
                date!(2025 - 01 - 31),
            ),
            (
                "ES1".to_owned(),
                date!(2025 - 02 - 01),
                date!(2025 - 02 - 28),
            ),
            (
                "ES1".to_owned(),
                date!(2025 - 04 - 01),
                date!(2025 - 04 - 30),
            ),
        ];
        let c = cobertura(&t);
        assert_eq!(
            c["ES1"],
            vec![
                (date!(2025 - 01 - 01), date!(2025 - 02 - 28)),
                (date!(2025 - 04 - 01), date!(2025 - 04 - 30))
            ]
        );
    }
}
