//! Modelos frente a modelos: el 390 frente a los 303 del año y la cadena de cuotas a compensar
//! de un 303 al siguiente; los resúmenes anuales de retenciones (190, 180, 193) frente a los
//! trimestrales (111, 115, 123); y el 200 frente a los 202, a las retenciones de las facturas
//! emitidas y a las cuentas depositadas.

use std::collections::BTreeSet;

use taxpain_core::money::Money;

use super::super::expediente::TipoTramite;
use super::super::modelos::{Declarado, unicas};
use super::super::{
    Area, Destinatario, Gravedad, Nuevo, Responsable, Salida, UN_EURO, Urgente, eur,
};
use super::{Contexto, a_ingresar, nombre, primera};

pub(super) fn revisar(cx: &Contexto<'_>, s: &mut Salida) {
    m390(cx, s);
    cadena_303(cx, s);
    resumenes(cx, s);
    m200_202(cx, s);
    m200_retenciones(cx, s);
    m200_cuentas(cx, s);
}

/// Periodos de un modelo en el año: los doce meses si hay alguno mensual; si no, los cuatro
/// trimestres. Devuelve los presentados y los que faltan.
fn del_año<'c>(
    cx: &'c Contexto<'_>,
    modelo: &str,
    y: i32,
) -> (Vec<(String, &'c Declarado)>, Vec<String>) {
    let mensual = cx
        .dec
        .keys()
        .any(|(m, yy, p)| m == modelo && *yy == y && p.len() == 2 && p.as_bytes()[1] != b'T');
    let periodos: Vec<String> = if mensual {
        (1..=12).map(|m| format!("{m:02}")).collect()
    } else {
        (1..=4).map(|q| format!("{q}T")).collect()
    };
    let mut hay = Vec::new();
    let mut faltan = Vec::new();
    for p in periodos {
        match cx.modelo(modelo, y, &p) {
            Some(d) => hay.push((p, d)),
            None => faltan.push(p),
        }
    }
    (hay, faltan)
}

/// Resultado del régimen general de un 303: [46] o, si no se leyó, [27] − [45].
/// [71] del periodo entero: el de una complementaria es sólo la diferencia con la anterior
/// ([71] = [69] − [70] + [109] − [112]), así que se le vuelve a sumar lo ingresado antes ([70])
/// y se le restan las devoluciones anteriores ([109]) (como `presentados::whole_period`).
fn r71_entero(d: &Declarado) -> Option<Money> {
    d.casillas.get(&71).map(|r| *r + d.c(70) - d.c(109))
}

fn resultado_303(d: &Declarado) -> Option<Money> {
    if d.casillas.contains_key(&46) {
        Some(d.c(46))
    } else if d.casillas.contains_key(&27) || d.casillas.contains_key(&45) {
        Some(d.c(27) - d.c(45))
    } else {
        None
    }
}

/// El 390 frente a la suma de los 303 del año (casillas del diseño de 2025: [47] cuotas
/// devengadas, [64] deducciones, [65] resultado del régimen general, [85] compensaciones del año
/// anterior aplicadas, [95] ingresos, [97] a compensar y [98] a devolver del último periodo).
/// La numeración vale también para 2022: el diseño de registro de ese ejercicio
/// (`dr390e2022.xlsx`) define [47], [65], [84], [85], [86], [95], [97], [98] y [662] con el mismo
/// significado que el de 2025 (`dr390e2025.xlsx`); lo que cambió en 2023 fueron los tipos del 5 %
/// y el 0 % y la paginación (cotejado el 2026-10-27).
fn m390(cx: &Contexto<'_>, s: &mut Salida) {
    let mut filas = Vec::new();
    for ((_, y, _), anual) in cx.de("390") {
        let y = *y;
        let (hay, faltan) = del_año(cx, "303", y);
        if !faltan.is_empty() {
            if !hay.is_empty() {
                s.add(
                    Nuevo {
                        codigo: "cruces.390_faltan_303",
                        area: Area::Modelos,
                        gravedad: Gravedad::Info,
                        responsable: Responsable::Documentacion,
                        titulo: format!("No se puede cuadrar el 390 de {y}: faltan 303"),
                        ficheros: vec![anual.fichero.clone()],
                    }
                    .detalle(format!(
                        "Para comprobar que el 390 es la suma de los 303 del año faltan los de: \
                         {}. Pídelos a la gestoría.",
                        faltan.join(", ")
                    )),
                    Some(y),
                    None,
                );
            }
            continue;
        }
        // Con alguna declaración sin sus casillas leídas no se sabe si cuadra.
        if !anual.con_casillas() || hay.iter().any(|(_, d)| !d.con_casillas()) {
            continue;
        }
        let suma = |f: &dyn Fn(&Declarado) -> Option<Money>| -> Option<Money> {
            hay.iter().map(|(_, d)| f(d)).sum()
        };
        let ultimo = hay.last().map(|(_, d)| *d);
        let mut lineas: Vec<(&str, u32, Option<Money>)> = vec![
            (
                "[47] Total de cuotas de IVA devengado (suma de [27])",
                47,
                suma(&|d| d.casillas.get(&27).copied()),
            ),
            (
                "[64] Total de deducciones (suma de [45])",
                64,
                suma(&|d| d.casillas.get(&45).copied()),
            ),
            (
                "[65] Resultado del régimen general (suma de [46])",
                65,
                suma(&resultado_303),
            ),
            (
                "[95] Total de ingresos (suma de los [71] a ingresar)",
                95,
                suma(&|d| r71_entero(d).map(|r| r.max(Money::ZERO))),
            ),
        ];
        // [97] + [98]: lo que quedó a favor en el último periodo.
        let a_favor = ultimo.and_then(|d| r71_entero(d).map(|r| (-r).max(Money::ZERO)));
        lineas.push((
            "[97] + [98] A compensar o devolver (último [71])",
            97,
            a_favor,
        ));
        let mut distintas = Vec::new();
        for (concepto, casilla, trimestral) in lineas {
            let Some(t) = trimestral else { continue };
            let declarado = if casilla == 97 {
                if !anual.casillas.contains_key(&97) && !anual.casillas.contains_key(&98) {
                    continue;
                }
                anual.c(97) + anual.c(98)
            } else {
                if !anual.casillas.contains_key(&casilla) {
                    continue;
                }
                anual.c(casilla)
            };
            let dif = declarado - t;
            filas.push(vec![
                y.to_string(),
                concepto.to_owned(),
                eur(declarado),
                eur(t),
                eur(dif),
            ]);
            if dif.abs() > UN_EURO {
                distintas.push(format!(
                    "{concepto}: el 390 dice {} y los 303 suman {} (diferencia {})",
                    eur(declarado),
                    eur(t),
                    eur(dif)
                ));
            }
        }
        if distintas.is_empty() {
            continue;
        }
        let mut ficheros = vec![anual.fichero.clone()];
        ficheros.extend(hay.iter().map(|(_, d)| d.fichero.clone()));
        s.add(
            Nuevo {
                codigo: "cruces.390_303",
                area: Area::Modelos,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Gestoria,
                titulo: format!("El 390 de {y} no cuadra con los 303 del año"),
                ficheros,
            }
            .detalle(format!(
                "El resumen anual del IVA tiene que ser la suma de las declaraciones del año. {}. \
                 Qué hacer: pide a la gestoría que lo revise; si el error está en el 390, se \
                 corrige con una declaración sustitutiva; si está en un 303, con una \
                 complementaria (si salía a pagar de menos) o una rectificación (si se pagó de \
                 más) de ese periodo.",
                distintas.join("; ")
            )),
            Some(y),
            None,
        );
        s.pedir(
            Destinatario::Gestoria,
            format!("Explicación de las diferencias entre el 390 de {y} y los 303 del año"),
        );
    }
    s.tabla(
        "390 frente a los 303 del año",
        "390 frente a 303",
        &[
            "Ejercicio",
            "Concepto",
            "390",
            "Suma de los 303",
            "Diferencia",
        ],
        filas,
    );
}

/// Periodo siguiente de un 303: 1T → 2T, 4T → 1T del año siguiente; 01 → 02, 12 → 01.
fn siguiente(y: i32, p: &str) -> Option<(i32, String)> {
    let b = p.as_bytes();
    match b {
        [n @ b'1'..=b'3', b'T'] => Some((y, format!("{}T", n - b'0' + 1))),
        [b'4', b'T'] => Some((y + 1, "1T".into())),
        _ => {
            let m: u8 = p.parse().ok()?;
            match m {
                1..=11 => Some((y, format!("{:02}", m + 1))),
                12 => Some((y + 1, "01".into())),
                _ => None,
            }
        }
    }
}

/// La cadena de cuotas a compensar de un 303 al siguiente: lo pendiente al empezar un periodo
/// ([110]) tiene que ser lo que quedó pendiente en el anterior ([87]) más lo que salió a
/// compensar en él (el [71] negativo), salvo que en el último periodo del año se pidiera la
/// devolución (art. 99.Cinco y 115 LIVA). Dentro de cada 303, [87] = [110] − [78].
fn cadena_303(cx: &Contexto<'_>, s: &mut Salida) {
    let mut filas = Vec::new();
    for ((_, y, p), d) in cx.de("303") {
        let (y, p) = (*y, p.as_str());
        // Dentro del mismo 303.
        if d.casillas.contains_key(&110) || d.casillas.contains_key(&87) {
            let dentro = d.c(110) - d.c(78) - d.c(87);
            if dentro.abs() > UN_EURO {
                s.add(
                    Nuevo {
                        codigo: "cruces.303_compensacion_interna",
                        area: Area::Modelos,
                        gravedad: Gravedad::Aviso,
                        responsable: Responsable::Gestoria,
                        titulo: format!(
                            "Las cuotas a compensar de {} no cuadran",
                            nombre("303", y, p)
                        ),
                        ficheros: vec![d.fichero.clone()],
                    }
                    .detalle(format!(
                        "Pendiente al empezar [110] {} menos lo aplicado [78] {} tendría que ser lo \
                         pendiente para después [87], que dice {} (diferencia {}). Pide a la \
                         gestoría que lo revise.",
                        eur(d.c(110)),
                        eur(d.c(78)),
                        eur(d.c(87)),
                        eur(dentro)
                    )),
                    Some(y),
                    None,
                );
            }
        }
        // Con el siguiente.
        let Some((ys, ps)) = siguiente(y, p) else {
            continue;
        };
        let Some(sig) = cx.modelo("303", ys, &ps) else {
            continue;
        };
        // Sólo con las casillas leídas de los dos: una casilla a cero no viene en el fichero
        // de la AEAT (ni suele estar en la lectura), así que la que falta vale 0; un modelo sin
        // ninguna casilla leída no dice nada y no se compara.
        if d.casillas.is_empty()
            || sig.casillas.is_empty()
            || !d.con_casillas()
            || !sig.con_casillas()
        {
            continue;
        }
        let r71 = d.c(71);
        let pendiente = d.c(87);
        let a_compensar = (-r71).max(Money::ZERO);
        let ultimo = p == "4T" || p == "12";
        // En el último periodo, el saldo a favor se puede pedir que se devuelva: el 390 lo dice.
        let devolucion = cx
            .modelo("390", y, "0A")
            .map(|a| (a.c(98) > Money::ZERO, a.c(97) > Money::ZERO));
        let (minimo, maximo) = match (ultimo, devolucion) {
            (false, _) | (true, Some((false, true))) => {
                (pendiente + a_compensar, pendiente + a_compensar)
            }
            (true, Some((true, _))) => (pendiente, pendiente),
            (true, _) => (pendiente, pendiente + a_compensar),
        };
        let arrastra = sig.c(110);
        filas.push(vec![
            format!("{p} {y} → {ps} {ys}"),
            eur(pendiente),
            eur(r71),
            if minimo == maximo {
                eur(minimo)
            } else {
                format!("{} o {}", eur(minimo), eur(maximo))
            },
            eur(arrastra),
        ]);
        let ficheros = vec![d.fichero.clone(), sig.fichero.clone()];
        if arrastra - maximo > UN_EURO {
            s.add(
                Nuevo {
                    codigo: "cruces.303_compensa_de_mas",
                    area: Area::Modelos,
                    gravedad: Gravedad::Error,
                    responsable: Responsable::Gestoria,
                    titulo: format!(
                        "{} compensa más IVA del que quedaba a favor",
                        primera(&nombre("303", ys, &ps))
                    ),
                    ficheros,
                }
                .detalle(format!(
                    "Arrastra {} de cuotas a compensar de periodos anteriores ([110]), pero {} \
                     dejó {} ([87] {} más {} a compensar del resultado [71]): sobran {}. \
                     Compensar de más es una infracción (art. 195 LGT) y Hacienda lo regularizará \
                     con intereses. Qué hacer: pide a la gestoría una complementaria de ese \
                     periodo, sin el exceso, antes de que llegue un requerimiento.",
                    eur(arrastra),
                    nombre("303", y, p),
                    eur(maximo),
                    eur(pendiente),
                    eur(a_compensar),
                    eur(arrastra - maximo)
                )),
                Some(ys),
                Some(-(arrastra - maximo)),
            );
            if let Some(pl) = cx.plazo("303", ys, &ps) {
                s.urgente(Urgente::art27(pl.due_on, Some(arrastra - maximo), cx.hoy));
            }
        } else if minimo - arrastra > UN_EURO {
            s.add(
                Nuevo {
                    codigo: "cruces.303_saldo_perdido",
                    area: Area::Modelos,
                    gravedad: Gravedad::Aviso,
                    responsable: Responsable::Gestoria,
                    titulo: format!(
                        "Se ha dejado de arrastrar IVA a compensar en {}",
                        nombre("303", ys, &ps)
                    ),
                    ficheros,
                }
                .detalle(format!(
                    "{} dejó {} a compensar en periodos siguientes, pero {} sólo arrastra {} en \
                     [110]: faltan {} a favor de la empresa. Qué hacer: pide a la gestoría que \
                     lo incluya en el siguiente 303 (hay cuatro años para compensarlo, art. \
                     99.Cinco LIVA) o que pida la rectificación.",
                    primera(&nombre("303", y, p)),
                    eur(minimo),
                    nombre("303", ys, &ps),
                    eur(arrastra),
                    eur(minimo - arrastra)
                )),
                Some(ys),
                Some(minimo - arrastra),
            );
        }
    }
    s.tabla(
        "Cadena de cuotas de IVA a compensar",
        "Compensación del 303",
        &[
            "Periodos",
            "Pendiente al acabar [87]",
            "Resultado [71]",
            "Debería arrastrar",
            "Arrastra el siguiente [110]",
        ],
        filas,
    );
}

/// Un resumen anual de retenciones y su modelo periódico.
pub(super) struct Resumen {
    anual: &'static str,
    periodico: &'static str,
    /// Casilla del total de retenciones del resumen, si no se leyeron sus registros.
    total_anual: u32,
    /// Casillas del total de retenciones del periódico (la primera que esté).
    total_periodico: &'static [u32],
    /// Casilla de lo ingresado en las declaraciones anteriores del mismo periodo (en una
    /// complementaria): si del periódico sólo se tiene el resultado, se le suma para tener el
    /// periodo entero.
    anteriores: u32,
    que: &'static str,
}

/// Resúmenes anuales frente a sus modelos periódicos. Casillas del resumen del 190 y del 180:
/// [01] perceptores, [02] percepciones o bases, [03] retenciones. `TODO(VERIFY)`: casilla del
/// total de retenciones del resumen del 193 (se toma la [03]; mejor con los registros leídos).
pub(super) const RESUMENES: [Resumen; 3] = [
    Resumen {
        anual: "190",
        periodico: "111",
        total_anual: 3,
        total_periodico: &[28, 30],
        anteriores: 29,
        que: "retenciones del trabajo y de profesionales",
    },
    Resumen {
        anual: "180",
        periodico: "115",
        total_anual: 3,
        total_periodico: &[3, 5],
        anteriores: 4,
        que: "retenciones de alquileres",
    },
    Resumen {
        anual: "193",
        periodico: "123",
        total_anual: 3,
        total_periodico: &[9, 14],
        anteriores: 13,
        que: "retenciones del capital mobiliario",
    },
];

/// Total de retenciones de un resumen anual: la suma de sus registros o, si no se leyeron, su
/// casilla de total.
pub(super) fn retenciones_anuales(d: &Declarado, casilla: u32) -> Option<Money> {
    if d.registros.iter().any(|r| r.retencion.is_some()) {
        Some(d.registros.iter().filter_map(|r| r.retencion).sum())
    } else {
        d.casillas.get(&casilla).copied()
    }
}

/// Retenciones de un periódico (111, 115, 123): su casilla de total o, si sólo se tiene el
/// resultado, el resultado más lo ingresado antes en el mismo periodo (el de una complementaria
/// es sólo la diferencia).
pub(super) fn total_periodico(r: &Resumen, d: &Declarado) -> Option<Money> {
    let (primera, resultado) = r.total_periodico.split_first()?;
    d.casillas.get(primera).copied().or_else(|| {
        resultado
            .iter()
            .find_map(|c| d.casillas.get(c))
            .map(|m| *m + d.c(r.anteriores))
    })
}

/// 190, 180 y 193 frente a la suma de sus 111, 115 y 123 del año.
fn resumenes(cx: &Contexto<'_>, s: &mut Salida) {
    let mut filas = Vec::new();
    for r in &RESUMENES {
        for ((_, y, _), anual) in cx.de(r.anual) {
            let y = *y;
            if !anual.con_casillas() {
                continue;
            }
            let Some(total) = retenciones_anuales(anual, r.total_anual) else {
                continue;
            };
            let (hay, faltan) = del_año(cx, r.periodico, y);
            if hay.iter().any(|(_, d)| !d.con_casillas()) {
                continue;
            }
            let suma: Money = hay.iter().filter_map(|(_, d)| total_periodico(r, d)).sum();
            filas.push(vec![
                y.to_string(),
                format!("{} / {}", r.anual, r.periodico),
                eur(total),
                eur(suma),
                eur(total - suma),
                faltan.join(", "),
            ]);
            let dif = total - suma;
            // Con periodos que faltan, sólo es seguro que está mal si el anual declara menos.
            if dif.abs() <= UN_EURO || (!faltan.is_empty() && dif > Money::ZERO) {
                continue;
            }
            let mut ficheros = vec![anual.fichero.clone()];
            ficheros.extend(hay.iter().map(|(_, d)| d.fichero.clone()));
            let qué_hacer = if dif > Money::ZERO {
                format!(
                    "El resumen declara más de lo ingresado: o falta ingresar retenciones en algún \
                     {} (complementaria, con recargo si ya pasó el plazo) o sobra algún perceptor \
                     en el {}.",
                    r.periodico, r.anual
                )
            } else {
                format!(
                    "El resumen declara menos de lo ingresado: falta algún perceptor en el {} \
                     (complementaria del resumen) o se ingresó de más en algún {} \
                     (rectificación para recuperarlo).",
                    r.anual, r.periodico
                )
            };
            s.add(
                Nuevo {
                    codigo: "cruces.resumen_anual",
                    area: Area::Modelos,
                    gravedad: Gravedad::Aviso,
                    responsable: Responsable::Gestoria,
                    titulo: format!(
                        "El {} de {y} no cuadra con los {} del año",
                        r.anual, r.periodico
                    ),
                    ficheros,
                }
                .detalle(format!(
                    "El {} declara {} de {} y los {} del año suman {} (diferencia {}).{} {qué_hacer} \
                     Pide a la gestoría que lo revise.",
                    r.anual,
                    eur(total),
                    r.que,
                    r.periodico,
                    eur(suma),
                    eur(dif),
                    if faltan.is_empty() {
                        String::new()
                    } else {
                        format!(" (Faltan en la carpeta los de {}.)", faltan.join(", "))
                    }
                )),
                Some(y),
                None,
            );
        }
    }
    s.tabla(
        "Resúmenes anuales frente a los modelos del año",
        "Resúmenes anuales",
        &[
            "Ejercicio",
            "Modelos",
            "Resumen anual",
            "Suma del año",
            "Diferencia",
            "Faltan",
        ],
        filas,
    );
}

/// Pagos fraccionados del 200 ([00601], [00603], [00605]) frente a los 202 del ejercicio.
/// `TODO(VERIFY)`: ejercicios que no coinciden con el año natural.
fn m200_202(cx: &Contexto<'_>, s: &mut Salida) {
    let mut filas = Vec::new();
    for ((_, y, _), d200) in cx.de("200") {
        let y = *y;
        if !d200.con_casillas() {
            continue;
        }
        for (n, casilla) in [(1, 601), (2, 603), (3, 605)] {
            let periodo = format!("{n}P");
            let d202 = cx.modelo("202", y, &periodo);
            // Un pago del 200 que no se ha podido leer, o un 202 sin sus casillas: no se sabe.
            if d200.dudosas.contains(&casilla) || d202.is_some_and(|d| !d.con_casillas()) {
                continue;
            }
            let en_200 = d200.c(casilla);
            let pagado = d202
                .and_then(|d| a_ingresar("202", d))
                .map(|m| m.max(Money::ZERO));
            if en_200.is_zero() && pagado.is_none_or(Money::is_zero) {
                continue;
            }
            filas.push(vec![
                y.to_string(),
                format!("{periodo} [{casilla:05}]"),
                eur(en_200),
                pagado.map(eur).unwrap_or_else(|| "no está".into()),
            ]);
            let Some(pagado) = pagado else {
                s.add(
                    Nuevo {
                        codigo: "cruces.200_falta_202",
                        area: Area::Sociedades,
                        gravedad: Gravedad::Info,
                        responsable: Responsable::Documentacion,
                        titulo: format!("Falta el 202 del {periodo} {y} para comprobar el 200"),
                        ficheros: vec![d200.fichero.clone()],
                    }
                    .detalle(format!(
                        "El 200 de {y} descuenta {} de pago fraccionado del {periodo} ([{casilla:05}]) \
                         y su 202 no está en la carpeta. Pídelo a la gestoría para comprobarlo.",
                        eur(en_200)
                    )),
                    Some(y),
                    None,
                );
                continue;
            };
            let dif = pagado - en_200;
            let mut ficheros = vec![d200.fichero.clone()];
            ficheros.extend(d202.map(|d| d.fichero.clone()));
            if dif > UN_EURO {
                s.add(
                    Nuevo {
                        codigo: "cruces.200_202_sin_descontar",
                        area: Area::Sociedades,
                        gravedad: Gravedad::Aviso,
                        responsable: Responsable::Gestoria,
                        titulo: format!(
                            "El 200 de {y} no descuenta todo el pago fraccionado del {periodo}"
                        ),
                        ficheros,
                    }
                    .detalle(format!(
                        "El 202 del {periodo} {y} ingresó {} y el 200 sólo descuenta {} en \
                         [{casilla:05}]: {} pagados dos veces. Qué hacer: pide a la gestoría la \
                         rectificación del 200 para recuperarlo.",
                        eur(pagado),
                        eur(en_200),
                        eur(dif)
                    )),
                    Some(y),
                    Some(dif),
                );
            } else if dif < -UN_EURO {
                s.add(
                    Nuevo {
                        codigo: "cruces.200_202_de_mas",
                        area: Area::Sociedades,
                        gravedad: Gravedad::Aviso,
                        responsable: Responsable::Gestoria,
                        titulo: format!(
                            "El 200 de {y} descuenta más pago fraccionado del {periodo} del que \
                             se ingresó"
                        ),
                        ficheros,
                    }
                    .detalle(format!(
                        "El 200 descuenta {} en [{casilla:05}] y el 202 del {periodo} {y} sólo \
                         ingresó {}: Hacienda reclamará {} con intereses. Qué hacer: pide a la \
                         gestoría que lo revise (complementaria del 200 si es un error).",
                        eur(en_200),
                        eur(pagado),
                        eur(-dif)
                    )),
                    Some(y),
                    Some(dif),
                );
                if let Some(pl) = cx.plazo("200", y, "0A") {
                    s.urgente(Urgente::art27(pl.due_on, Some(-dif), cx.hoy));
                }
            }
        }
    }
    s.tabla(
        "Pagos fraccionados: 200 frente a los 202",
        "200 frente a 202",
        &["Ejercicio", "Pago", "En el 200", "Ingresado en el 202"],
        filas,
    );
}

/// Retenciones que descuenta el 200 ([01766]) frente a las que pusieron los clientes en las
/// facturas emitidas del ejercicio (art. 127 LIS).
fn m200_retenciones(cx: &Contexto<'_>, s: &mut Salida) {
    for ((_, y, _), d200) in cx.de("200") {
        let y = *y;
        if !d200.con_casillas() || d200.dudosas.contains(&1766) {
            continue;
        }
        let emitidas = unicas(
            cx.exp
                .emitidas()
                .filter(|(_, f)| f.fecha.is_some_and(|d| d.year() == y))
                .map(|(doc, f)| (doc.sha.as_str(), f)),
        );
        if emitidas.is_empty() {
            continue;
        }
        let con: Money = emitidas.iter().filter_map(|f| f.retencion).sum();
        let deduce = d200.c(1766);
        let dif = con - deduce;
        if dif > UN_EURO {
            s.add(
                Nuevo {
                    codigo: "cruces.200_retenciones_sin_deducir",
                    area: Area::Sociedades,
                    gravedad: Gravedad::Aviso,
                    responsable: Responsable::Gestoria,
                    titulo: format!("El 200 de {y} no deduce todas las retenciones de las ventas"),
                    ficheros: vec![d200.fichero.clone()],
                }
                .detalle(format!(
                    "Las facturas emitidas de {y} llevan {} de retenciones que practicaron los \
                     clientes y el 200 deduce {} ([01766]): {} sin deducir. Qué hacer: comprueba \
                     con los certificados de retenciones de los clientes y pide a la gestoría la \
                     rectificación del 200 para recuperarlo.",
                    eur(con),
                    eur(deduce),
                    eur(dif)
                )),
                Some(y),
                Some(dif),
            );
        } else if -dif > UN_EURO {
            s.add(
                Nuevo {
                    codigo: "cruces.200_retenciones_otras",
                    area: Area::Sociedades,
                    gravedad: Gravedad::Info,
                    responsable: Responsable::Documentacion,
                    titulo: format!(
                        "El 200 de {y} deduce más retenciones de las que hay en las facturas"
                    ),
                    ficheros: vec![d200.fichero.clone()],
                }
                .detalle(format!(
                    "El 200 deduce {} de retenciones ([01766]) y las facturas emitidas de la \
                     carpeta suman {}. El resto puede ser de intereses, dividendos o alquileres \
                     cobrados: guarda sus certificados de retenciones por si Hacienda los pide.",
                    eur(deduce),
                    eur(con)
                )),
                Some(y),
                None,
            );
        }
    }
}

/// El 200 frente a las cuentas depositadas: resultado del ejercicio ([00500]) y cifra de
/// negocios ([00255]).
fn m200_cuentas(cx: &Contexto<'_>, s: &mut Salida) {
    let mut vistos = BTreeSet::new();
    for d in &cx.exp.docs {
        let Some(t) = &d.tramite else { continue };
        let Some(y) = t
            .ejercicio
            .filter(|_| t.tipo == TipoTramite::DepositoCuentas)
        else {
            continue;
        };
        let Some(d200) = cx.modelo("200", y, "0A").filter(|d| d.con_casillas()) else {
            continue;
        };
        if !vistos.insert(y) {
            continue;
        }
        let mut distintas = Vec::new();
        for (que, casilla, cuentas) in [
            ("resultado del ejercicio [00500]", 500, t.resultado),
            ("cifra de negocios [00255]", 255, t.cifra_negocios),
        ] {
            let (Some(c), Some(m)) = (cuentas, d200.casillas.get(&casilla)) else {
                continue;
            };
            if (*m - c).abs() > UN_EURO {
                distintas.push(format!(
                    "{que}: el 200 dice {} y las cuentas {}",
                    eur(*m),
                    eur(c)
                ));
            }
        }
        if distintas.is_empty() {
            continue;
        }
        s.add(
            Nuevo {
                codigo: "cruces.200_cuentas",
                area: Area::Sociedades,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Gestoria,
                titulo: format!("El 200 de {y} no coincide con las cuentas depositadas"),
                ficheros: vec![d200.fichero.clone(), d.ruta.clone()],
            }
            .detalle(format!(
                "{}. El Impuesto sobre Sociedades parte del resultado de las cuentas aprobadas \
                 (art. 10.3 LIS): si no coinciden, uno de los dos está mal. Qué hacer: pide a la \
                 gestoría que lo explique; si el 200 está mal, se corrige con una complementaria \
                 o una rectificativa.",
                distintas.join("; ")
            )),
            Some(y),
            None,
        );
    }
}
