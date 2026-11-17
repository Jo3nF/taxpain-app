//! Plazos: los modelos presentados fuera de plazo, con el recargo que correspondería (art. 27
//! LGT), y el depósito de cuentas y la legalización de libros de cada ejercicio en el Registro
//! Mercantil.

use std::collections::BTreeMap;

use taxpain_core::calendar::rules::add_months;
use taxpain_core::money::{Money, Rate};
use time::{Date, Month};

use super::super::expediente::TipoTramite;
use super::super::modelos::años_con_datos;
use super::super::{
    Area, Destinatario, Gravedad, Nuevo, Responsable, Salida, Urgente, eur, fecha, lista,
    revisable_desde,
};
use super::{Contexto, a_ingresar, nombre, primera};

pub(super) fn revisar(cx: &Contexto<'_>, s: &mut Salida) {
    fuera_de_plazo(cx, s);
    registro(cx, s);
}

/// Sin carpeta, sólo las presentaciones fuera de plazo: el depósito de cuentas y la legalización
/// de libros se comprueban con los justificantes del Registro Mercantil de la carpeta.
pub(super) fn revisar_sin_carpeta(cx: &Contexto<'_>, s: &mut Salida) {
    fuera_de_plazo(cx, s);
}

/// Recargo por presentar sin requerimiento fuera de plazo (art. 27.2 LGT).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Recargo {
    /// Porcentaje en puntos básicos.
    pub tipo: Rate,
    pub importe: Money,
    /// Meses completos de retraso.
    pub meses: i32,
    /// Más de doce meses: además, intereses de demora.
    pub intereses: bool,
}

/// Meses completos entre el último día del plazo y la presentación.
fn meses_completos(fin: Date, presentado: Date) -> i32 {
    let mut m = 0;
    while m < 240 && add_months(fin, m + 1) <= presentado {
        m += 1;
    }
    m
}

/// Desde el 11 de julio de 2021 (Ley 11/2021): 1 % más un 1 % por cada mes completo de retraso
/// hasta los doce meses; después, 15 % e intereses de demora (art. 27.2 LGT, texto consolidado
/// BOE-A-2003-23186 leído el 2026-10-27 y otra vez el 2026-11-16, con la reducción del 25 % del
/// art. 27.5 y los recargos del 5, 10 y 20 % del art. 28). Antes: 5, 10, 15 o 20 % según se presentara en los 3, 6 o 12
/// meses siguientes o más tarde. La DT 1.ª.1 de la Ley 11/2021 aplica la escala nueva a «los
/// recargos exigidos con anterioridad a su entrada en vigor, siempre que su aplicación resulte
/// más favorable para el obligado tributario y el recargo no haya adquirido firmeza»; aquí se usa
/// la de la fecha de presentación, que es lo que se liquida hoy (los recargos de antes de 2021
/// ya son firmes salvo recurso).
pub(crate) fn recargo(fin: Date, presentado: Date, cuota: Money) -> Option<Recargo> {
    if presentado <= fin || cuota <= Money::ZERO {
        return None;
    }
    let meses = meses_completos(fin, presentado);
    let mas_de_un_año = presentado > add_months(fin, 12);
    let ley_11_2021 = Date::from_calendar_date(2021, Month::July, 11).ok()?;
    let pct = if presentado >= ley_11_2021 {
        if mas_de_un_año {
            15
        } else {
            1 + meses.min(11)
        }
    } else if mas_de_un_año {
        20
    } else if meses < 3 {
        5
    } else if meses < 6 {
        10
    } else {
        15
    };
    let tipo = Rate::percent(pct);
    Some(Recargo {
        tipo,
        importe: cuota.apply_rate(tipo),
        meses,
        intereses: mas_de_un_año,
    })
}

/// Días de más que puede durar un plazo por los festivos cuando TaxPain no tiene el calendario
/// oficial de ese año. Si el último día es inhábil, el plazo pasa al primer día hábil siguiente
/// (art. 30.5 Ley 39/2015), y cuentan también los festivos de la comunidad y del municipio del
/// interesado (art. 30.6; comprobado en el BOE el 2026-10-31). Lo más largo es Semana Santa en
/// las comunidades con jueves santo y lunes de Pascua: un plazo que acaba en jueves pasa al
/// martes, cinco días. Se deja una semana de margen.
const SIN_FESTIVOS: i64 = 7;

/// Modelos con fecha de presentación posterior al último día del plazo. En los años sin los
/// festivos oficiales cargados, el plazo se calcula sólo con los fines de semana: un retraso de
/// pocos días puede no serlo y no se avisa.
///
/// Si la declaración vigente complementa o sustituye a otra, el plazo se juzga con la primera:
/// si llegó a tiempo (o no se sabe cuándo se presentó), la declaración no se presentó tarde, y
/// subsanarla sin requerimiento no da responsabilidad por la anterior (art. 179.3 LGT). Sólo la
/// complementaria de una autoliquidación que ingresa algo después del plazo lleva el recargo,
/// sobre lo que ingresa ella (art. 27 LGT).
fn fuera_de_plazo(cx: &Contexto<'_>, s: &mut Salida) {
    use crate::lectura_justificante::Tipo;
    let mut filas = Vec::new();
    for ((m, y, p), d) in &cx.dec {
        let Some(vigente) = d.presentado else {
            continue;
        };
        let Some(plazo) = cx.plazo(m, *y, p) else {
            continue;
        };
        // La complementaria de una autoliquidación que ingresa algo (o no se sabe): cuenta su
        // fecha, y el recargo es sobre lo que ingresa ella.
        let complementaria = d.rectifica
            && d.tipo == Tipo::Complementaria
            && !super::super::modelos::informativo(m)
            && super::tiene_resultado(m)
            && a_ingresar(m, d).is_none_or(|c| c > Money::ZERO);
        // La fecha que cuenta y si es la de la primera (de la que no se saben los importes).
        let (presentado, de_la_original) = if d.rectifica && !complementaria {
            match d.original {
                Some(o) => (o, true),
                None => continue,
            }
        } else {
            (vigente, false)
        };
        if presentado <= plazo.due_on {
            continue;
        }
        let dias = (presentado - plazo.due_on).whole_days();
        let con_festivos = cx.con_festivos(plazo);
        if !con_festivos && dias <= SIN_FESTIVOS {
            continue;
        }
        let cuota = a_ingresar(m, d)
            .filter(|c| *c > Money::ZERO)
            .filter(|_| !de_la_original);
        let r = cuota.and_then(|c| recargo(plazo.due_on, presentado, c));
        // Una autoliquidación cuyas casillas no se han podido leer: no se sabe si salía a
        // ingresar (ni, por tanto, si hay recargo). Tampoco de la primera de varias: las
        // casillas son de la última.
        let sin_cuota =
            super::tiene_resultado(m) && (!super::resultado_sabido(m, d) || de_la_original);
        filas.push(vec![
            m.clone(),
            format!("{p} {y}"),
            fecha(plazo.due_on),
            fecha(presentado),
            format!("{dias} días"),
            cuota.map(eur).unwrap_or_default(),
            r.as_ref()
                .map(|r| format!("{} ({})", eur(r.importe), r.tipo))
                .unwrap_or_default(),
        ]);
        let cual = if complementaria {
            format!(
                "la complementaria del {}",
                nombre(m, *y, p).trim_start_matches("el ")
            )
        } else {
            nombre(m, *y, p)
        };
        let original = match (complementaria, d.original) {
            (true, Some(o)) => format!(
                " Es una complementaria: la declaración original se presentó el {}{}.",
                fecha(o),
                if o <= plazo.due_on { ", a tiempo" } else { "" }
            ),
            (true, None) => " Es una complementaria.".to_owned(),
            _ if de_la_original => format!(
                " Es la fecha de la primera declaración del periodo; la vigente, que la \
                 complementa o sustituye, es del {}.",
                fecha(vigente)
            ),
            _ => String::new(),
        };
        let aproximado = if con_festivos {
            String::new()
        } else {
            format!(
                " TaxPain no tiene los festivos oficiales de {}: el plazo se ha calculado sin \
                 ellos y el retraso puede ser de unos días menos.",
                plazo.due_on.year()
            )
        };
        let aproximado = format!("{original}{aproximado}");
        let (detalle, importe) = match (&r, cuota) {
            _ if sin_cuota && complementaria => (
                format!(
                    "Se presentó el {} y el plazo acababa el {} ({dias} días tarde).{aproximado} \
                     No se han podido leer sus casillas: si ingresaba algo, lleva un recargo por \
                     presentar fuera de plazo sin requerimiento, sobre lo que ingresa ella (art. \
                     27 LGT). Qué hacer: pregunta a la gestoría qué se corrigió y si Hacienda lo \
                     ha notificado.",
                    fecha(presentado),
                    fecha(plazo.due_on)
                ),
                None,
            ),
            _ if sin_cuota => (
                format!(
                    "Se presentó el {} y el plazo acababa el {} ({dias} días tarde).{aproximado} \
                     {}: si salía a ingresar, lleva un recargo por presentar fuera de plazo sin \
                     requerimiento (art. 27 LGT); si no, puede tener una sanción (art. 198 LGT, \
                     reducida a la mitad si se presentó sin requerimiento). Qué hacer: pregunta a \
                     la gestoría por qué se presentó tarde y si Hacienda lo ha notificado.",
                    fecha(presentado),
                    fecha(plazo.due_on),
                    if de_la_original {
                        "No se sabe lo que salía a ingresar en la primera"
                    } else {
                        "No se han podido leer sus casillas"
                    }
                ),
                None,
            ),
            (Some(r), Some(c)) => (
                format!(
                    "Se presentó el {} y el plazo acababa el {} ({dias} días tarde, {} completos).\
                     {aproximado} Con {}{} a ingresar, el recargo por presentar fuera de plazo \
                     sin requerimiento (art. 27 LGT) es del {}: {}{}. Si se paga entero en el \
                     plazo de la carta de pago, se reduce un 25 % (art. 27.5 LGT). Qué hacer: \
                     comprueba si Hacienda ya lo ha notificado; si el retraso fue de la gestoría, \
                     pídele que lo asuma.",
                    fecha(presentado),
                    fecha(plazo.due_on),
                    super::super::plural(usize::try_from(r.meses).unwrap_or(0), "mes", "meses"),
                    eur(c),
                    if complementaria {
                        " en la complementaria"
                    } else {
                        ""
                    },
                    r.tipo,
                    eur(r.importe),
                    if r.intereses {
                        ", más intereses de demora desde el día siguiente a los doce meses"
                    } else {
                        ""
                    }
                ),
                Some(-r.importe),
            ),
            _ => (
                format!(
                    "Se presentó el {} y el plazo acababa el {} ({dias} días tarde).{aproximado} \
                     No sale a ingresar, así que no hay recargo; pero una declaración informativa \
                     o sin ingreso fuera de plazo puede tener una sanción (art. 198 LGT, reducida \
                     a la mitad si se presentó sin requerimiento). Qué hacer: pregunta a la \
                     gestoría por qué se presentó tarde.",
                    fecha(presentado),
                    fecha(plazo.due_on)
                ),
                None,
            ),
        };
        s.add(
            Nuevo {
                codigo: "cruces.fuera_de_plazo",
                area: Area::Plazos,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Gestoria,
                titulo: format!("{} se presentó fuera de plazo", primera(&cual)),
                ficheros: vec![d.fichero.clone()],
            }
            .detalle(detalle),
            Some(*y),
            importe,
        );
        if let Some(r) = &r {
            s.urgente(Urgente::recargo_fijado(
                plazo.due_on,
                r.importe,
                r.tipo.bp() / 100,
            ));
        }
    }
    s.tabla(
        "Presentaciones fuera de plazo",
        "Fuera de plazo",
        &[
            "Modelo",
            "Periodo",
            "Plazo hasta",
            "Presentado",
            "Retraso",
            "A ingresar",
            "Recargo (art. 27 LGT)",
        ],
        filas,
    );
}

/// Un justificante del Registro en la carpeta.
struct Visto {
    presentado: Option<Date>,
    fichero: String,
}

/// Fecha del acta de la carpeta que aprueba las cuentas de `y` (la más temprana, si hay
/// varias), con su fichero. Sólo la que las aprueba, y después del cierre del ejercicio: una
/// junta que reparte reservas no cuenta.
fn aprobacion<'a>(cx: &Contexto<'a>, y: i32) -> Option<(Date, &'a str)> {
    cx.exp
        .docs
        .iter()
        .filter_map(|d| {
            let a = d.aplicacion.as_ref().filter(|a| a.aprueba_las_de(y))?;
            a.fecha.map(|f| (f, d.ruta.as_str()))
        })
        .min_by_key(|(f, _)| *f)
}

/// Depósito de cuentas (art. 279.1 LSC: en el mes siguiente a la aprobación; con el acta en la
/// carpeta, un mes desde su fecha y, si no, desde el final de los seis meses que hay para
/// aprobarlas) y legalización de libros (art. 18 Ley 14/2013: en los cuatro meses siguientes al
/// cierre) de cada ejercicio con datos en la carpeta.
fn registro(cx: &Contexto<'_>, s: &mut Salida) {
    if !cx.sociedad() {
        return;
    }
    let mut vistos: BTreeMap<(i32, bool), Vec<Visto>> = BTreeMap::new();
    for d in &cx.exp.docs {
        let Some(t) = &d.tramite else { continue };
        // Sin ejercicio, el anterior al de la presentación (se presentan al año siguiente).
        let Some(y) = t.ejercicio.or(t.presentado.map(|p| p.year() - 1)) else {
            continue;
        };
        vistos
            .entry((y, t.tipo == TipoTramite::DepositoCuentas))
            .or_default()
            .push(Visto {
                presentado: t.presentado,
                fichero: d.ruta.clone(),
            });
    }
    let desde = revisable_desde(cx.hoy);
    let mut filas = Vec::new();
    let mut faltan: BTreeMap<bool, Vec<i32>> = BTreeMap::new();
    for y in años_con_datos(cx.exp, &cx.dec).range(desde..cx.hoy.year()) {
        for deposito in [false, true] {
            let codigo = if deposito {
                "deposito-cuentas"
            } else {
                "legalizacion-libros"
            };
            let Some(plazo) = cx.plazo(codigo, *y, "0A") else {
                continue;
            };
            // Con el acta que aprueba las cuentas, el depósito vence un mes después de su fecha.
            let acta = aprobacion(cx, *y).filter(|_| deposito);
            let fin = acta.map_or(plazo.due_on, |(f, _)| add_months(f, 1));
            if fin >= cx.hoy {
                continue;
            }
            let que = if deposito {
                "Depósito de cuentas"
            } else {
                "Legalización de libros"
            };
            let Some(v) = vistos.get(&(*y, deposito)) else {
                filas.push(vec![
                    y.to_string(),
                    que.into(),
                    fecha(fin),
                    String::new(),
                    "sin justificante en la carpeta".into(),
                    String::new(),
                ]);
                faltan.entry(deposito).or_default().push(*y);
                continue;
            };
            // El primero que se presentó.
            let primero = v.iter().filter_map(|v| v.presentado).min();
            let fichero = v
                .iter()
                .find(|v| v.presentado == primero)
                .map(|v| v.fichero.clone())
                .unwrap_or_default();
            let estado = match primero {
                None => "sin fecha de presentación".to_owned(),
                Some(p) if p <= fin => "en plazo".to_owned(),
                Some(p) => format!("fuera de plazo ({} días)", (p - fin).whole_days()),
            };
            filas.push(vec![
                y.to_string(),
                que.into(),
                fecha(fin),
                primero.map(fecha).unwrap_or_default(),
                estado,
                fichero.clone(),
            ]);
            let Some(p) = primero.filter(|p| *p > fin) else {
                continue;
            };
            // Art. 283 LSC (BOE-A-2010-10544, comprobado el 2026-10-31): multa de 1.200 a 60.000 €
            // y, con una facturación anual de más de 6.000.000 €, «el límite de la multa para cada
            // año de retraso se elevará a 300.000 euros» (283.1); si se depositan antes de que se
            // inicie el procedimiento sancionador, «la sanción se impondrá en su grado mínimo y
            // reducida en un cincuenta por ciento» (283.3); la infracción prescribe a los tres años
            // (283.4).
            let consecuencia = if deposito {
                "Mientras las cuentas no están depositadas el Registro no inscribe nada de la \
                 sociedad (cierre registral, art. 282 LSC) y el ICAC puede multar entre 1.200 y \
                 60.000 €, o hasta 300.000 € por cada año de retraso si la facturación anual pasa \
                 de 6.000.000 € (art. 283.1 LSC). Si se depositaron antes de que empezara el \
                 procedimiento sancionador, la multa se impone en su grado mínimo y reducida un \
                 50 % (art. 283.3 LSC), y la infracción prescribe a los tres años (art. 283.4 \
                 LSC)."
            } else {
                // Art. 333.2 RRM (RD 1784/1996): los libros se presentan a legalizar antes de
                // que pasen cuatro meses desde el cierre; art. 333.3: «En el caso de que la
                // legalización se solicite fuera del plazo legal, el Registrador lo hará constar
                // así en la diligencia del Libro y en el asiento correspondiente del
                // Libro-fichero de legalizaciones» (BOE-A-1996-17533, comprobado el 2026-10-27).
                "El Registro legaliza igualmente los libros presentados tarde, pero lo hace \
                 constar, y en una inspección o un pleito los libros legalizados fuera de plazo \
                 valen menos como prueba."
            };
            s.add(
                Nuevo {
                    codigo: if deposito {
                        "cruces.deposito_fuera_de_plazo"
                    } else {
                        "cruces.legalizacion_fuera_de_plazo"
                    },
                    area: Area::Plazos,
                    gravedad: Gravedad::Aviso,
                    responsable: Responsable::Gestoria,
                    titulo: format!("{que} de {y} fuera de plazo"),
                    ficheros: vec![fichero],
                }
                .detalle(format!(
                    "Se presentó en el Registro Mercantil el {} y el plazo acababa el {}{}. \
                     {consecuencia} Qué hacer: pregunta a la gestoría por qué se presentó tarde y \
                     que no vuelva a pasar.",
                    fecha(p),
                    fecha(fin),
                    acta.map(|(f, _)| format!(
                        " (un mes desde el acta del {} que aprueba las cuentas, art. 279.1 LSC)",
                        fecha(f)
                    ))
                    .unwrap_or_default()
                )),
                Some(*y),
                None,
            );
        }
    }
    for (deposito, años) in faltan {
        let años_txt: Vec<String> = años.iter().map(ToString::to_string).collect();
        let (que, codigo, titulo, consecuencia) = if deposito {
            (
                "el justificante del depósito de las cuentas anuales",
                "cruces.deposito_sin_justificante",
                "Sin justificante del depósito de cuentas",
                "Si no se depositaron, el Registro no inscribe nada de la sociedad (art. 282 LSC) \
                 y el ICAC puede multar entre 1.200 y 60.000 €, o hasta 300.000 € por cada año de \
                 retraso si la facturación anual pasa de 6.000.000 € (art. 283.1 LSC); depositarlas \
                 antes de que empiece el procedimiento sancionador deja la multa en su grado \
                 mínimo y reducida un 50 % (art. 283.3 LSC).",
            )
        } else {
            (
                "el justificante de la legalización de los libros",
                "cruces.legalizacion_sin_justificante",
                "Sin justificante de la legalización de libros",
                "Los libros (diario, inventarios y cuentas, actas) se legalizan cada año en el \
                 Registro Mercantil (art. 18 Ley 14/2013).",
            )
        };
        s.add(
            Nuevo {
                codigo,
                area: Area::Plazos,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Documentacion,
                titulo: format!("{titulo} de {}", lista(&años_txt, 6)),
                ficheros: vec![],
            }
            .detalle(format!(
                "En la carpeta no está {que} de {}. {consecuencia} Qué hacer: pídelo a la gestoría \
                 o descárgalo del Registro Mercantil para comprobar que se hizo y en plazo.",
                lista(&años_txt, 6)
            )),
            años.last().copied(),
            None,
        );
        s.pedir(
            Destinatario::Registro,
            format!("{} de {}", primera(que), lista(&años_txt, 6)),
        );
    }
    s.tabla(
        "Depósito de cuentas y legalización de libros",
        "Registro Mercantil",
        &[
            "Ejercicio",
            "Trámite",
            "Plazo hasta",
            "Presentado",
            "Estado",
            "Fichero",
        ],
        filas,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn eur(e: i64) -> Money {
        Money::from_euros(e, 0)
    }

    #[test]
    fn recargo_del_articulo_27() {
        let fin = date!(2025 - 04 - 21);
        // Un día tarde: 1 %.
        let r = recargo(fin, date!(2025 - 04 - 22), eur(1_000)).unwrap();
        assert_eq!((r.tipo, r.importe, r.meses), (Rate::percent(1), eur(10), 0));
        // Un mes completo: 2 %.
        let r = recargo(fin, date!(2025 - 05 - 21), eur(1_000)).unwrap();
        assert_eq!((r.tipo, r.meses), (Rate::percent(2), 1));
        // Cinco meses y pico: 6 %.
        let r = recargo(fin, date!(2025 - 09 - 30), eur(1_000)).unwrap();
        assert_eq!(r.tipo, Rate::percent(6));
        // Más de doce meses: 15 % e intereses.
        let r = recargo(fin, date!(2026 - 05 - 01), eur(1_000)).unwrap();
        assert_eq!(
            (r.tipo, r.importe, r.intereses),
            (Rate::percent(15), eur(150), true)
        );
        // En plazo o sin ingreso, nada.
        assert!(recargo(fin, fin, eur(1_000)).is_none());
        assert!(recargo(fin, date!(2025 - 05 - 30), Money::ZERO).is_none());
        // Antes de la Ley 11/2021: 5 % en los tres primeros meses.
        let r = recargo(date!(2021 - 01 - 20), date!(2021 - 03 - 01), eur(1_000)).unwrap();
        assert_eq!(r.tipo, Rate::percent(5));
    }
}
