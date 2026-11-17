//! Pagos del banco frente a los modelos: cada modelo que sale a ingresar tiene que tener su
//! cargo en el banco por el mismo importe y dentro del plazo (la domiciliación se carga el
//! último día). Y al revés: los cargos de Hacienda que no son de ningún modelo de la carpeta
//! (recargos, sanciones, aplazamientos o modelos que faltan).

use std::collections::{BTreeMap, BTreeSet};

use taxpain_core::calendar::rules::add_months;
use taxpain_core::money::Money;
use time::{Date, Duration, Month};

use super::super::modelos::{Clave, Declarado, declarados};
use super::super::{
    Area, Gravedad, Nuevo, Responsable, Salida, Urgente, eur, fecha, lista, plural,
};
use super::datos::MovBanco;
use super::{Contexto, a_ingresar, nombre, primera};
use crate::documentos::DocKind;

/// ¿Parece un cargo de Hacienda por su concepto? Las bonificaciones y promociones del banco
/// (una bonificación de impuestos, un «cashback», la retención de una promoción) no lo son.
pub(super) fn de_hacienda(concepto: &str) -> bool {
    let c = concepto.to_uppercase();
    if ["BONIF", "CASHBACK", "PROMOCI"]
        .iter()
        .any(|k| c.contains(k))
    {
        return false;
    }
    [
        "AEAT",
        "A.E.A.T",
        "HACIENDA",
        "TRIBUT",
        "AGENCIA ESTATAL",
        "IMPUEST",
        "IMPTO",
        "MODELO",
    ]
    .iter()
    .any(|k| c.contains(k))
}

/// Fecha e importe de un movimiento: lo que lo identifica aunque esté repetido.
fn clave(mv: &MovBanco) -> (Date, i64) {
    (mv.fecha, mv.importe.cents())
}

/// Cargos ya casados con un modelo. El mismo cargo puede estar repetido (en la carpeta y en
/// TaxPain, o en dos extractos que se solapan) y cuenta como uno; dos cargos iguales del mismo
/// extracto son dos. Un cargo casado con un modelo no sirve para otro.
struct Usos {
    hay: BTreeMap<(Date, i64), usize>,
    usados: BTreeMap<(Date, i64), usize>,
}

impl Usos {
    fn new(movs: &[MovBanco]) -> Self {
        let mut por_origen: BTreeMap<(&str, (Date, i64)), usize> = BTreeMap::new();
        for mv in movs {
            *por_origen
                .entry((mv.origen.as_str(), clave(mv)))
                .or_default() += 1;
        }
        let mut hay: BTreeMap<(Date, i64), usize> = BTreeMap::new();
        for ((_, k), n) in por_origen {
            let e = hay.entry(k).or_default();
            *e = (*e).max(n);
        }
        Usos {
            hay,
            usados: BTreeMap::new(),
        }
    }

    fn libre(&self, mv: &MovBanco) -> bool {
        let k = clave(mv);
        self.usados.get(&k).copied().unwrap_or(0) < self.hay.get(&k).copied().unwrap_or(0)
    }

    fn usar(&mut self, mv: &MovBanco) {
        if self.libre(mv) {
            *self.usados.entry(clave(mv)).or_default() += 1;
        }
    }
}

pub(super) fn revisar(cx: &Contexto<'_>, s: &mut Salida) {
    let (movs, tramos) = super::datos::movimientos(cx.exp);
    if movs.is_empty() {
        return;
    }
    let mut usos = Usos::new(&movs);
    // Los modelos del ejercicio anterior que se pagaron ya en el primero que se revisa (el 4T y
    // el 200 se pagan al año siguiente): sus cargos son suyos, aunque de ellos no se avisa.
    let desde = super::super::revisable_desde(cx.hoy);
    let anteriores: BTreeMap<Clave, Declarado> = declarados(cx.exp)
        .into_iter()
        .filter(|((m, y, p), _)| {
            *y == desde - 1
                && cx
                    .plazo(m, *y, p)
                    .is_some_and(|pl| pl.due_on.year() >= desde)
        })
        .collect();
    let modelos: Vec<(&Clave, &Declarado)> = anteriores.iter().chain(cx.dec.iter()).collect();
    // Primero, los cargos que casan exactos con algún modelo: no cuentan como «otro importe»
    // de otro modelo con el mismo plazo.
    let exactos: BTreeSet<(Date, i64)> = modelos
        .iter()
        .copied()
        .filter_map(|((m, y, p), d)| {
            let cuota = a_ingresar(m, d).filter(|c| *c > Money::ZERO)?;
            let plazo = cx.plazo(m, *y, p)?;
            let fin = plazo.due_on.max(d.presentado.unwrap_or(plazo.due_on)) + Duration::days(10);
            movs.iter()
                .filter(|mv| mv.importe == -cuota && plazo.starts_on <= mv.fecha && mv.fecha <= fin)
                .map(|mv| (mv.fecha, mv.importe.cents()))
                .min()
        })
        .collect();
    // Las cuentas desde las que se pagan los impuestos: las que tienen el cargo de algún modelo.
    // Un plazo sólo está cubierto si hay extractos de esas cuentas de esas fechas (los de otra
    // cuenta no dicen nada del pago). Si no se ve ningún pago, valen todas.
    let de_pago: BTreeSet<&str> = movs
        .iter()
        .filter(|mv| exactos.contains(&clave(mv)))
        .map(|mv| mv.cuenta.as_str())
        .collect();
    let cubre = |d: Date| {
        tramos
            .iter()
            .filter(|(c, _, _)| de_pago.is_empty() || de_pago.contains(c.as_str()))
            .any(|(_, a, b)| *a <= d && d <= *b)
    };
    let mut filas = Vec::new();
    let mut sin_extracto = Vec::new();
    // Los del ejercicio anterior cuyo cargo se ha casado aquí.
    let mut pagados: BTreeSet<Clave> = BTreeSet::new();
    for ((m, y, p), d) in modelos {
        let Some(cuota) = a_ingresar(m, d).filter(|c| *c > Money::ZERO) else {
            continue;
        };
        let anterior = *y < desde;
        let Some(plazo) = cx.plazo(m, *y, p) else {
            continue;
        };
        let cual = nombre(m, *y, p);
        let fin = plazo.due_on.max(d.presentado.unwrap_or(plazo.due_on)) + Duration::days(10);
        if anterior {
            // Sólo se casa su cargo, para que no salga como cargo suelto.
            if let Some(mv) = movs
                .iter()
                .filter(|mv| mv.importe == -cuota && usos.libre(mv))
                .filter(|mv| plazo.starts_on <= mv.fecha && mv.fecha <= fin)
                .min_by_key(|mv| mv.fecha)
            {
                usos.usar(mv);
                pagados.insert((m.clone(), *y, p.clone()));
                filas.push(vec![
                    m.clone(),
                    format!("{p} {y}"),
                    eur(cuota),
                    fecha(plazo.due_on),
                    fecha(mv.fecha),
                    "pagado".into(),
                ]);
            }
            continue;
        }
        if !cubre(plazo.due_on) {
            sin_extracto.push(format!("{m} {p} {y} ({})", eur(cuota)));
            filas.push(vec![
                m.clone(),
                format!("{p} {y}"),
                eur(cuota),
                fecha(plazo.due_on),
                String::new(),
                "sin extracto de esas fechas".into(),
            ]);
            continue;
        }
        let mismo = |mv: &&MovBanco| mv.importe == -cuota && usos.libre(mv);
        let en_plazo = movs
            .iter()
            .filter(mismo)
            .filter(|mv| plazo.starts_on <= mv.fecha && mv.fecha <= fin)
            .min_by_key(|mv| mv.fecha);
        if let Some(mv) = en_plazo {
            usos.usar(mv);
            filas.push(vec![
                m.clone(),
                format!("{p} {y}"),
                eur(cuota),
                fecha(plazo.due_on),
                fecha(mv.fecha),
                "pagado".into(),
            ]);
            continue;
        }
        // Un cargo posterior que es el pago en plazo de otro modelo (el del pago fraccionado
        // siguiente por el mismo importe, por ejemplo) no es el de éste.
        let tarde = movs
            .iter()
            .filter(mismo)
            .filter(|mv| !exactos.contains(&clave(mv)))
            .filter(|mv| fin < mv.fecha && mv.fecha <= fin + Duration::days(400))
            .min_by_key(|mv| mv.fecha);
        if let Some(mv) = tarde {
            usos.usar(mv);
            filas.push(vec![
                m.clone(),
                format!("{p} {y}"),
                eur(cuota),
                fecha(plazo.due_on),
                fecha(mv.fecha),
                "pagado tarde".into(),
            ]);
            s.add(
                Nuevo {
                    codigo: "cruces.pago_tarde",
                    area: Area::Bancos,
                    gravedad: Gravedad::Aviso,
                    responsable: Responsable::Gestoria,
                    titulo: format!("{} se pagó después del plazo", primera(&cual)),
                    ficheros: vec![d.fichero.clone(), mv.origen.clone()],
                }
                .detalle(format!(
                    "Salen a ingresar {} y el cargo de ese importe en el banco es del {} \
                     (el plazo acababa el {}). Si se presentó sin pagar, la deuda pasa a periodo \
                     ejecutivo con un recargo del 5 al 20 % (art. 28 LGT). Qué hacer: busca la \
                     carta de pago o la providencia de apremio y pregunta a la gestoría.",
                    eur(cuota),
                    fecha(mv.fecha),
                    fecha(plazo.due_on)
                )),
                Some(*y),
                None,
            );
            s.urgente(Urgente::sin_calculo(
                Some(cuota.apply_rate(taxpain_core::money::Rate::percent(5))),
                format!(
                    "Se pagó {} días tarde: si no estaba aplazado, lleva el recargo del periodo \
                     ejecutivo del 5 % sobre {} (art. 28.2 LGT), que Hacienda liquida aparte; \
                     si ya se había notificado la providencia de apremio, del 10 o del 20 %. \
                     TaxPain no sabe si ya se ha liquidado ni pagado.",
                    (mv.fecha - plazo.due_on).whole_days(),
                    eur(cuota)
                ),
            ));
            continue;
        }
        let otros: Vec<&MovBanco> = movs
            .iter()
            .filter(|mv| {
                mv.importe < Money::ZERO
                    && de_hacienda(&mv.concepto)
                    && !exactos.contains(&clave(mv))
                    && usos.libre(mv)
                    && plazo.starts_on <= mv.fecha
                    && mv.fecha <= fin
            })
            .collect();
        // Un cargo ya tomado como «pago por otro importe» de este modelo no sale además como
        // cargo suelto.
        for mv in &otros {
            usos.usar(mv);
        }
        filas.push(vec![
            m.clone(),
            format!("{p} {y}"),
            eur(cuota),
            fecha(plazo.due_on),
            String::new(),
            if otros.is_empty() {
                "no se ve el cargo".into()
            } else {
                "cargo de Hacienda por otro importe".into()
            },
        ]);
        let (titulo, detalle) = if otros.is_empty() {
            (
                format!("No se ve el pago {} en el banco", de(&cual)),
                format!(
                    "Sale a ingresar {} y en los extractos no hay ningún cargo de ese importe \
                     entre el {} y el {}. Puede haberse pagado desde otra cuenta que no está en la \
                     carpeta, con un aplazamiento o no haberse pagado. Qué hacer: busca el \
                     justificante del pago (NRC) y, si no se pagó, pide a la gestoría que lo \
                     regularice cuanto antes.",
                    eur(cuota),
                    fecha(plazo.starts_on),
                    fecha(fin)
                ),
            )
        } else {
            let cargos: Vec<String> = otros
                .iter()
                .map(|mv| format!("{} el {}", eur(-mv.importe), fecha(mv.fecha)))
                .collect();
            (
                format!(
                    "El cargo {} en el banco no es por el mismo importe",
                    de(&cual)
                ),
                format!(
                    "Sale a ingresar {} y los cargos de Hacienda de esas fechas son de {}. Puede \
                     ser una complementaria, un pago de otro modelo junto con este o un error en \
                     la domiciliación. Qué hacer: compara el justificante del pago (NRC) con el \
                     modelo y pregunta a la gestoría.",
                    eur(cuota),
                    lista(&cargos, 4)
                ),
            )
        };
        s.add(
            Nuevo {
                codigo: if otros.is_empty() {
                    "cruces.pago_no_esta"
                } else {
                    "cruces.pago_otro_importe"
                },
                area: Area::Bancos,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Empresa,
                titulo,
                ficheros: vec![d.fichero.clone()],
            }
            .detalle(detalle),
            Some(*y),
            None,
        );
        let pagado: Money = otros.iter().map(|mv| -mv.importe).sum();
        if otros.is_empty() || pagado < cuota - super::super::UN_EURO {
            s.urgente(Urgente::sin_ingreso(plazo.due_on, cuota - pagado, cx.hoy));
        }
    }
    if !sin_extracto.is_empty() {
        s.add(
            Nuevo {
                codigo: "cruces.pagos_sin_extracto",
                area: Area::Bancos,
                gravedad: Gravedad::Info,
                responsable: Responsable::Documentacion,
                titulo: format!(
                    "{} sin extracto del banco para comprobar el pago",
                    plural(
                        sin_extracto.len(),
                        "modelo a ingresar",
                        "modelos a ingresar"
                    )
                ),
                ficheros: vec![],
            }
            .detalle(format!(
                "No hay extractos de las fechas en que se pagaban: {}. Añade a la carpeta los \
                 extractos de esos meses para comprobar que cada modelo se pagó.",
                lista(&sin_extracto, 8)
            )),
            None,
            None,
        );
    }
    de_modelos_anteriores(cx, &movs, &pagados, &mut usos);
    sueltos(cx, &movs, &usos, s);
    s.tabla(
        "Pagos de los modelos en el banco",
        "Pagos de los modelos",
        &[
            "Modelo",
            "Periodo",
            "A ingresar",
            "Plazo hasta",
            "Cargo en el banco",
            "Estado",
        ],
        filas,
    );
}

/// «de» + «el 115…» → «del 115…».
fn de(cual: &str) -> String {
    match cual.strip_prefix("el ") {
        Some(resto) => format!("del {resto}"),
        None => format!("de {cual}"),
    }
}

/// Cargos que pagan modelos de antes del primer ejercicio revisable que no se han casado arriba
/// (el 4T o el 200 del ejercicio anterior se pagan ya dentro del periodo revisado, a veces
/// tarde): uno por modelo, el primero por su importe a ingresar desde que empieza su plazo hasta
/// 400 días después de que acabe (sin plazo conocido, en su año y los dos siguientes). Se dan por
/// usados: no son cargos sueltos. Otro cargo igual sí sigue suelto (puede ser un pago repetido).
fn de_modelos_anteriores(
    cx: &Contexto<'_>,
    movs: &[MovBanco],
    pagados: &BTreeSet<Clave>,
    usos: &mut Usos,
) {
    let desde = super::super::revisable_desde(cx.hoy);
    for ((m, y, p), d) in declarados(cx.exp) {
        if y >= desde || pagados.contains(&(m.clone(), y, p.clone())) {
            continue;
        }
        let Some(cuota) = a_ingresar(&m, &d).filter(|c| *c > Money::ZERO) else {
            continue;
        };
        let ventana = match cx.plazo(&m, y, &p) {
            Some(pl) => Some((
                pl.starts_on,
                pl.due_on.max(d.presentado.unwrap_or(pl.due_on)) + Duration::days(400),
            )),
            None => Date::from_calendar_date(y, Month::January, 1)
                .ok()
                .zip(Date::from_calendar_date(y + 2, Month::December, 31).ok()),
        };
        let Some((a, b)) = ventana else {
            continue;
        };
        if let Some(mv) = movs
            .iter()
            .filter(|mv| mv.importe == -cuota && a <= mv.fecha && mv.fecha <= b)
            .filter(|mv| usos.libre(mv))
            .min_by_key(|mv| mv.fecha)
        {
            usos.usar(mv);
        }
    }
}

/// Un certificado de estar al corriente con la AEAT de la carpeta que aún vale: de los últimos
/// doce meses (art. 75.2 RGAT) o de los que diga él. El de la Sede de la AEAT (por su código
/// seguro de verificación y su título), si dice que es positivo. Si no, sólo los leídos con IA
/// cuya descripción dice que es ese certificado y que es positivo: con el tipo de documento no
/// basta, porque los de situación censal o de residencia también son certificados de la AEAT, y
/// las reglas toman por certificado la solicitud o una liquidación que hablan de él.
fn certificado_al_corriente(cx: &Contexto<'_>) -> Option<(Date, u32)> {
    let vale = |f: &Date| *f <= cx.hoy && add_months(*f, 12) >= cx.hoy;
    let al_corriente = |t: &str| {
        let t = crate::documentos::fold(t);
        (t.contains("estar al corriente") || t.contains("se encuentra al corriente"))
            && ![
                "no es el de estar al corriente",
                "situacion censal",
                "residencia",
                "negativ",
                "no esta al corriente",
                "no se encuentra al corriente",
                "solicitud",
                "resguardo",
                "liquidacion",
            ]
            .iter()
            .any(|x| t.contains(x))
    };
    let de_la_sede = cx
        .exp
        .oficiales()
        .filter(|(_, k, o)| *k == DocKind::CertificadoAeat && o.positivo == Some(true))
        .filter_map(|(_, _, o)| {
            let (f, meses) = (o.fecha?, o.validez_meses.unwrap_or(12));
            let hasta = super::super::sede::mas_meses(f, meses)?;
            (f <= cx.hoy && hasta >= cx.hoy).then_some((f, meses))
        });
    cx.exp
        .docs
        .iter()
        .filter(|d| d.con_ia && al_corriente(&d.descripcion))
        .filter_map(|d| match &d.empresa {
            Some((DocKind::CertificadoAeat, f)) => f.date,
            _ => None,
        })
        .filter(vale)
        .map(|f| (f, 12))
        .chain(de_la_sede)
        .max()
}

/// Cargos de Hacienda que no casan con ningún modelo de la carpeta.
fn sueltos(cx: &Contexto<'_>, movs: &[MovBanco], usos: &Usos, s: &mut Salida) {
    let desde = super::super::revisable_desde(cx.hoy);
    let mut vistos = BTreeSet::new();
    let mut cargos = Vec::new();
    let mut total = Money::ZERO;
    for mv in movs {
        let clave = (mv.fecha, mv.importe.cents());
        if mv.importe >= Money::ZERO
            || mv.fecha.year() < desde
            || !de_hacienda(&mv.concepto)
            || !usos.libre(mv)
            || !vistos.insert(clave)
        {
            continue;
        }
        total += -mv.importe;
        cargos.push(format!(
            "{} el {} («{}»)",
            eur(-mv.importe),
            fecha(mv.fecha),
            mv.concepto.trim()
        ));
    }
    if cargos.is_empty() {
        return;
    }
    s.add(
        Nuevo {
            codigo: "cruces.cargos_sin_modelo",
            area: Area::Bancos,
            gravedad: Gravedad::Info,
            responsable: Responsable::Documentacion,
            titulo: format!(
                "{} de Hacienda que no son de ningún modelo de la carpeta",
                plural(cargos.len(), "cargo", "cargos")
            ),
            ficheros: vec![],
        }
        .detalle(match certificado_al_corriente(cx) {
            // Art. 74 RGAT (BOE-A-2007-15984, comprobado el 2026-11-01): al corriente es no deber
            // nada en periodo ejecutivo salvo lo aplazado, fraccionado o suspendido (74.1.g) y
            // tener presentadas las autoliquidaciones con plazo vencido en los doce meses
            // anteriores a los dos meses previos al certificado (74.3). No dice qué son los cargos.
            Some((c, meses)) => format!(
                "Suman {}: {}. En la carpeta está el certificado de estar al corriente con la AEAT \
                 del {} ({}). Dice que en esa fecha no había deudas \
                 en periodo ejecutivo, salvo las aplazadas, fraccionadas o suspendidas (art. \
                 74.1.g RGAT), y que estaban presentadas las autoliquidaciones con plazo vencido \
                 en los doce meses anteriores a los dos meses previos a su fecha (art. 74.3 RGAT). \
                 No descarta que sean plazos de un aplazamiento o fraccionamiento, un recargo, una \
                 sanción o intereses pagados en periodo voluntario, ni modelos que no están en la \
                 carpeta. Qué hacer: pide a la gestoría qué es cada uno.",
                eur(total),
                lista(&cargos, 8),
                fecha(c),
                if meses == 12 {
                    "vale doce meses, art. 75.2 RGAT".to_owned()
                } else {
                    format!("vale {meses} meses, según el propio certificado")
                }
            ),
            None => format!(
                "Suman {}: {}. Pueden ser recargos, sanciones, intereses, plazos de un \
                 aplazamiento o modelos que no están en la carpeta. Qué hacer: pide a la gestoría \
                 qué es cada uno (y las notificaciones de Hacienda que haya).",
                eur(total),
                lista(&cargos, 8)
            ),
        }),
        None,
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::de_hacienda;

    #[test]
    fn cargos_de_hacienda_por_el_concepto() {
        assert!(de_hacienda("ADEUDO TRIBUTOS"));
        assert!(de_hacienda("AEAT IMPUESTOS DOMICILIADOS"));
        assert!(de_hacienda("TRIBUTOS 00000 0000001"));
        // Las promociones del banco no son pagos a Hacienda.
        assert!(!de_hacienda("RETENCION PROMOCION · BONIF. IMPUESTOS"));
        assert!(!de_hacienda("CASHBACK · BONIF. IMPUESTOS"));
        assert!(!de_hacienda("RECIBO LUZ"));
    }
}
