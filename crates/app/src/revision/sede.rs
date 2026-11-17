//! Documentos de la Sede de la AEAT que hay en la carpeta ([`crate::oficiales`]): los
//! certificados (cuándo se expidieron, qué dicen y hasta cuándo valen) y los requerimientos
//! (qué pedían, su plazo, sus contestaciones y lo que suponen para la prescripción). Las
//! devoluciones, que salen de los modelos y del banco, están en los cruces.

use std::collections::BTreeSet;

use taxpain_core::calendar::{HolidayCalendar, calendar_for};
use time::{Date, Duration, Month};

use super::expediente::{Doc, Expediente};
use super::{Area, Gravedad, Nuevo, Responsable, Salida, fecha, lista};
use crate::documentos::DocKind;
use crate::oficiales::Oficial;

pub(crate) fn revisar(exp: &Expediente, hoy: Date, s: &mut Salida) {
    certificados(exp, hoy, s);
    requerimientos(exp, hoy, s);
}

/// `d` más `meses` (el mismo día del mes o, si ese mes no lo tiene, el último).
pub(crate) fn mas_meses(d: Date, meses: u32) -> Option<Date> {
    let total = i64::from(d.year()) * 12 + i64::from(u8::from(d.month())) - 1 + i64::from(meses);
    let y = i32::try_from(total.div_euclid(12)).ok()?;
    let m = Month::try_from(u8::try_from(total.rem_euclid(12) + 1).ok()?).ok()?;
    Date::from_calendar_date(y, m, d.day().min(m.length(y))).ok()
}

fn fecha_o_nada(d: Option<Date>) -> String {
    d.map(fecha).unwrap_or_default()
}

/// Validez de un certificado de estar al corriente si el certificado no la dice: doce meses
/// desde que se expide (art. 75.2 del Reglamento de gestión e inspección, RD 1065/2007).
const MESES_CORRIENTE: u32 = 12;

/// Días antes de que caduque un certificado en los que se avisa.
const AVISO_CADUCA: i64 = 30;

fn nombre_certificado(k: DocKind) -> &'static str {
    match k {
        DocKind::CertificadoAeat => "Estar al corriente",
        DocKind::CertificadoSituacionCensal => "Situación censal",
        DocKind::CertificadoIae => "Actividades económicas (IAE)",
        DocKind::CertificadoResidencia => "Residencia fiscal",
        _ => "Otro certificado",
    }
}

fn es_certificado(k: DocKind) -> bool {
    matches!(
        k,
        DocKind::CertificadoAeat
            | DocKind::CertificadoSituacionCensal
            | DocKind::CertificadoIae
            | DocKind::CertificadoResidencia
            | DocKind::CertificadoTributario
    )
}

/// Las declaraciones censales (036 o 037) de la carpeta y de TaxPain con su fecha.
fn censales(exp: &Expediente) -> Vec<(Date, String)> {
    let mut out: Vec<(Date, String)> = exp
        .docs
        .iter()
        .filter_map(|d| match &d.empresa {
            Some((DocKind::Censal036, f)) => f.date.map(|x| (x, d.ruta.clone())),
            _ => None,
        })
        .collect();
    out.extend(
        exp.taxpain
            .documentos
            .iter()
            .filter(|d| d.kind == DocKind::Censal036)
            .filter_map(|d| d.fecha.or(d.facts.date).map(|x| (x, d.fichero.clone()))),
    );
    out.sort();
    out
}

/// Hasta cuándo vale un certificado y cómo está hoy: (validez, estado). `despues` es la primera
/// declaración censal posterior al certificado, si la hay.
fn validez(
    k: DocKind,
    o: &Oficial,
    hoy: Date,
    despues: Option<&(Date, String)>,
) -> (String, String) {
    if k == DocKind::CertificadoAeat || o.validez_meses.is_some() {
        let meses = o.validez_meses.unwrap_or(MESES_CORRIENTE);
        let Some(hasta) = o.fecha.and_then(|f| mas_meses(f, meses)) else {
            return (format!("{meses} meses"), String::new());
        };
        let estado = if hasta < hoy {
            format!("caducó el {}", fecha(hasta))
        } else if hasta - hoy <= Duration::days(AVISO_CADUCA) {
            format!("caduca el {}", fecha(hasta))
        } else {
            "vigente".to_owned()
        };
        return (format!("{meses} meses, hasta el {}", fecha(hasta)), estado);
    }
    if o.mientras_no_cambien || k == DocKind::CertificadoSituacionCensal {
        let estado = despues.map_or_else(
            || "vigente si no ha cambiado el censo".to_owned(),
            |(f, _)| {
                format!(
                    "puede no estar al día: hay un 036 del {} posterior",
                    fecha(*f)
                )
            },
        );
        return ("mientras no cambien sus datos".to_owned(), estado);
    }
    if let Some(y) = o.ejercicio {
        return (format!("del ejercicio {y}"), String::new());
    }
    (String::new(), String::new())
}

fn resultado(k: DocKind, o: &Oficial) -> String {
    match (k, o.positivo) {
        (DocKind::CertificadoSituacionCensal, _) => {
            let m: Vec<&str> = o.modelos_obligados().into_iter().collect();
            if m.is_empty() {
                String::new()
            } else {
                format!("obligaciones: {}", m.join(", "))
            }
        }
        (_, Some(true)) => "positivo".to_owned(),
        (_, Some(false)) => "negativo".to_owned(),
        _ => String::new(),
    }
}

/// Los certificados de la AEAT de la carpeta, con lo que dicen y hasta cuándo valen. Del último
/// de estar al corriente se avisa si es negativo, si ha caducado o si va a caducar; del último de
/// situación censal, si hay un 036 posterior (vale mientras no cambien sus datos).
fn certificados(exp: &Expediente, hoy: Date, s: &mut Salida) {
    let mut certs: Vec<(&Doc, DocKind, &Oficial)> = exp
        .oficiales()
        .filter(|(_, k, _)| es_certificado(*k))
        .collect();
    if certs.is_empty() {
        return;
    }
    certs.sort_by(|a, b| {
        (nombre_certificado(a.1), a.2.fecha, &a.0.ruta).cmp(&(
            nombre_certificado(b.1),
            b.2.fecha,
            &b.0.ruta,
        ))
    });
    let censales = censales(exp);
    let posterior = |o: &Oficial| o.fecha.and_then(|f| censales.iter().find(|(c, _)| *c > f));
    // El más reciente de cada tipo es el que vale; de los anteriores se dice que hay otro.
    let mas_reciente = |k: DocKind, o: &Oficial| {
        certs
            .iter()
            .filter(|(_, kk, oo)| *kk == k && oo.fecha > o.fecha)
            .filter_map(|(_, _, oo)| oo.fecha)
            .max()
    };
    let mut filas = Vec::new();
    for (d, k, o) in &certs {
        let (validez, estado) = match mas_reciente(*k, o) {
            Some(f) => (
                validez(*k, o, hoy, None).0,
                format!("hay uno más reciente, del {}", fecha(f)),
            ),
            None => validez(*k, o, hoy, posterior(o)),
        };
        filas.push(vec![
            nombre_certificado(*k).to_owned(),
            fecha_o_nada(o.fecha),
            resultado(*k, o),
            validez,
            estado,
            d.ruta.clone(),
        ]);
    }
    let ultimo = |kind: DocKind| {
        certs
            .iter()
            .filter(|(_, k, o)| *k == kind && o.fecha.is_some())
            .max_by_key(|(_, _, o)| o.fecha)
    };
    if let Some((d, _, o)) = ultimo(DocKind::CertificadoAeat)
        && let Some(f) = o.fecha
    {
        corriente(d, o, f, hoy, s);
    }
    if let Some((d, _, o)) = ultimo(DocKind::CertificadoSituacionCensal)
        && let Some(f) = o.fecha
        && let Some((c, ruta)) = posterior(o)
    {
        s.add(
            Nuevo {
                codigo: "aeat.censal_posterior",
                area: Area::Aeat,
                gravedad: Gravedad::Info,
                responsable: Responsable::Empresa,
                titulo: format!(
                    "El certificado de situación censal del {} puede no estar al día",
                    fecha(f)
                ),
                ficheros: vec![d.ruta.clone(), ruta.clone()],
            }
            .detalle(format!(
                "Vale mientras no cambien los datos del censo, y después hay una declaración \
                 censal (036) del {}. Si te lo piden, saca uno nuevo en la Sede de la AEAT: el de \
                 la carpeta puede no tener lo que cambió.",
                fecha(*c)
            )),
            None,
            None,
        );
    }
    s.tabla(
        "Certificados de la AEAT",
        "Certificados AEAT",
        &[
            "Certificado",
            "Fecha",
            "Resultado",
            "Validez",
            "Estado",
            "Fichero",
        ],
        filas,
    );
}

/// El último certificado de estar al corriente: negativo, caducado o a punto de caducar.
fn corriente(d: &Doc, o: &Oficial, f: Date, hoy: Date, s: &mut Salida) {
    let meses = o.validez_meses.unwrap_or(MESES_CORRIENTE);
    let de_donde = if o.validez_meses.is_some() {
        "lo dice el propio certificado"
    } else {
        "art. 75.2 del Reglamento de gestión e inspección, RD 1065/2007"
    };
    if o.positivo == Some(false) {
        s.add(
            Nuevo {
                codigo: "aeat.corriente_negativo",
                area: Area::Aeat,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Gestoria,
                titulo: format!(
                    "El certificado de estar al corriente del {} es negativo",
                    fecha(f)
                ),
                ficheros: vec![d.ruta.clone()],
            }
            .detalle(
                "La AEAT dice que la empresa no estaba al corriente de sus obligaciones \
                 tributarias: alguna declaración sin presentar o alguna deuda sin pagar (art. \
                 74.1 del Reglamento de gestión e inspección, RD 1065/2007). Qué hacer: pide a la \
                 gestoría que mire en la Sede qué falta y, cuando esté resuelto, saca otro \
                 certificado."
                    .to_owned(),
            ),
            Some(f.year()),
            None,
        );
        s.urgente(super::Urgente::sin_calculo(
            None,
            "Un certificado negativo suele querer decir una deuda pendiente o una declaración \
             sin presentar: TaxPain no sabe cuál ni cuánto, ni si ya está en periodo ejecutivo \
             (recargo del 5, 10 o 20 %, art. 28 LGT). Míralo en la Sede (Deudas).",
        ));
        return;
    }
    let Some(hasta) = mas_meses(f, meses) else {
        return;
    };
    let (titulo, cuando) = if hasta < hoy {
        (
            format!(
                "El certificado de estar al corriente caducó el {}",
                fecha(hasta)
            ),
            "caducó",
        )
    } else if hasta - hoy <= Duration::days(AVISO_CADUCA) {
        (
            format!(
                "El certificado de estar al corriente caduca el {}",
                fecha(hasta)
            ),
            "caduca",
        )
    } else {
        return;
    };
    s.add(
        Nuevo {
            codigo: "aeat.corriente_caducado",
            area: Area::Aeat,
            gravedad: Gravedad::Info,
            responsable: Responsable::Empresa,
            titulo,
            ficheros: vec![d.ruta.clone()],
        }
        .detalle(format!(
            "El último de la carpeta es del {} y vale {meses} meses ({de_donde}): {cuando} el {}. \
             Si te lo piden (una subvención, un contrato con la Administración, un banco), saca \
             otro en la Sede de la AEAT; quien lo pide puede exigir uno más reciente.",
            fecha(f),
            fecha(hasta)
        )),
        None,
        None,
    );
}

/// Una contestación a un requerimiento: su recibo de presentación en el registro.
struct Contestacion {
    fecha: Option<Date>,
    /// Número de asiento registral.
    asiento: Option<String>,
    fichero: String,
}

/// Un requerimiento de la AEAT con lo que hay de él en la carpeta.
struct Requerimiento<'a> {
    doc: &'a Doc,
    o: &'a Oficial,
    /// Expediente o referencia.
    id: Option<String>,
    /// Cuándo se notificó (se aceptó o se accedió en la DEHú o en la Sede).
    notificado: Option<Date>,
    /// Último día para contestar y si es seguro (lo dice la AEAT o se ha contado con los
    /// festivos oficiales del año).
    plazo: Option<(Date, bool)>,
    contestaciones: Vec<Contestacion>,
    /// Recibos de las solicitudes de ampliación del plazo: no son contestaciones.
    ampliaciones: Vec<Contestacion>,
    /// Impuestos que cita (ver [`conceptos`]).
    conceptos: Vec<String>,
}

/// Días naturales que hay para acceder a una notificación electrónica antes de que se dé por
/// rechazada (art. 43.2 Ley 39/2015): sin la fecha de la notificación, el plazo para contestar
/// puede empezar hasta entonces.
const DIAS_PARA_ACCEDER: i64 = 10;

/// ¿Es el recibo de una solicitud de ampliación del plazo (el trámite RB012 de la AEAT)? No es
/// una contestación.
fn ampliacion(o: &Oficial) -> bool {
    [&o.tramite, &o.asunto].iter().any(|t| {
        t.as_deref().is_some_and(|t| {
            let t = crate::documentos::fold(t);
            t.contains("ampliacion de plazo") || t.contains("ampliacion del plazo")
        })
    })
}

/// Último día de un plazo de `dias` hábiles contados desde el siguiente a `desde` (art. 30 Ley
/// 39/2015: sin sábados, domingos ni festivos) y si se han podido contar los festivos oficiales.
fn dias_habiles(exp: &Expediente, desde: Date, dias: u32) -> (Date, bool) {
    let perfil = exp.perfil.as_ref();
    let (cal, cargados): (HolidayCalendar, bool) = calendar_for(
        desde.year(),
        perfil.and_then(|p| p.ccaa),
        perfil.map_or(&[][..], |p| &p.festivos_locales),
    );
    let mut d = desde;
    let mut n = 0;
    while n < dias {
        d += Duration::days(1);
        if cal.is_business_day(d) {
            n += 1;
        }
    }
    (d, cargados)
}

/// ¿Es `o` de este requerimiento? Por el expediente, por el código seguro de verificación del
/// requerimiento o por su número de notificación.
fn del_requerimiento(r: &Oficial, id: Option<&str>, o: &Oficial) -> bool {
    let mismo = |a: &Option<String>, b: Option<&str>| a.as_deref().is_some_and(|a| Some(a) == b);
    mismo(&o.expediente, id)
        || mismo(&o.referencia, id)
        || mismo(&o.csv_referido, r.csv.as_deref())
        || mismo(&o.notificacion, r.notificacion.as_deref())
}

/// Impuestos que cita un requerimiento. En el de una sociedad, el IRPF que cite es el de otro
/// (un socio, un administrador): sus retenciones salen aparte.
fn conceptos(exp: &Expediente, o: &Oficial) -> Vec<String> {
    let sociedad = match &exp.perfil {
        Some(p) => p.persona == taxpain_core::calendar::Persona::Juridica,
        None => exp
            .own_nif
            .as_deref()
            .and_then(|n| n.chars().next())
            .is_some_and(|c| c.is_ascii_alphabetic() && !"XYZKLM".contains(c)),
    };
    o.conceptos
        .iter()
        .filter(|c| !(sociedad && c.as_str() == "IRPF"))
        .cloned()
        .collect()
}

fn requerimientos(exp: &Expediente, hoy: Date, s: &mut Salida) {
    let mut vistos = BTreeSet::new();
    let mut reqs = Vec::new();
    for (d, k, o) in exp.oficiales() {
        if k != DocKind::RequerimientoAeat {
            continue;
        }
        // El mismo requerimiento puede estar dos veces (en el expediente y en las
        // notificaciones).
        let clave = o.csv.clone().unwrap_or_else(|| d.sha.clone());
        if !vistos.insert(clave) {
            continue;
        }
        let id = o.expediente.clone().or_else(|| o.referencia.clone());
        let otros: Vec<(&Doc, DocKind, &Oficial)> = exp
            .oficiales()
            .filter(|(x, _, oo)| !std::ptr::eq(*x, d) && del_requerimiento(o, id.as_deref(), oo))
            .collect();
        let notificado = otros
            .iter()
            .filter(|(_, k, oo)| {
                *k == DocKind::NotificacionAeat && oo.notificacion == o.notificacion
            })
            .filter_map(|(_, _, oo)| oo.accedida.or(oo.puesta))
            .min();
        let nuevo = otros
            .iter()
            .filter(|(_, k, _)| *k == DocKind::ResolucionAeat)
            .filter_map(|(_, _, oo)| oo.nuevo_plazo)
            .max();
        // El plazo cuenta desde el día siguiente a la notificación (art. 30.3 Ley 39/2015). Sin
        // su fecha, no se sabe: como mucho, desde los diez días naturales que hay para acceder a
        // ella (art. 43.2 Ley 39/2015), y queda como aproximado.
        let plazo = nuevo.map(|n| (n, true)).or_else(|| {
            let dias = o.plazo_dias?;
            match notificado {
                Some(n) => Some(dias_habiles(exp, n, dias)),
                None => {
                    let ultimo = o.fecha? + Duration::days(DIAS_PARA_ACCEDER);
                    Some((dias_habiles(exp, ultimo, dias).0, false))
                }
            }
        });
        let recibos = |amplia: bool| {
            let mut v: Vec<Contestacion> = otros
                .iter()
                .filter(|(_, k, oo)| *k == DocKind::RegistroAeat && ampliacion(oo) == amplia)
                .map(|(x, _, oo)| Contestacion {
                    fecha: oo.fecha,
                    asiento: oo.referencia.clone(),
                    fichero: x.ruta.clone(),
                })
                .collect();
            v.sort_by(|a, b| (a.fecha, &a.asiento).cmp(&(b.fecha, &b.asiento)));
            // El mismo recibo puede estar dos veces (en el expediente y en otra carpeta).
            v.dedup_by(|a, b| a.fecha == b.fecha && a.asiento == b.asiento);
            v
        };
        reqs.push(Requerimiento {
            doc: d,
            o,
            id,
            notificado,
            plazo,
            contestaciones: recibos(false),
            ampliaciones: recibos(true),
            conceptos: conceptos(exp, o),
        });
    }
    if reqs.is_empty() {
        return;
    }
    reqs.sort_by_key(|r| r.o.fecha);
    let mut filas = Vec::new();
    for r in &reqs {
        let organo = r.o.organo.clone().unwrap_or_else(|| "AEAT".to_owned());
        let años: Vec<String> = r.o.ejercicios.iter().map(i32::to_string).collect();
        let plazo = match (r.plazo, r.o.plazo_dias) {
            (Some((p, true)), _) => fecha(p),
            (Some((p, false)), _) => format!("hacia el {}", fecha(p)),
            (None, Some(n)) => format!("{n} días hábiles"),
            (None, None) => String::new(),
        };
        let asiento = |c: &Contestacion| {
            c.asiento
                .as_ref()
                .map_or_else(String::new, |a| format!(" ({a})"))
        };
        let contestado: Vec<String> = r
            .contestaciones
            .iter()
            .map(|c| {
                let tarde = match (c.fecha, r.plazo) {
                    (Some(f), Some((p, true))) if f > p => " (fuera de plazo)",
                    _ => "",
                };
                format!("{}{}{tarde}", fecha_o_nada(c.fecha), asiento(c))
            })
            .collect();
        let ampliado: Vec<String> = r
            .ampliaciones
            .iter()
            .map(|c| {
                format!(
                    "ampliación del plazo pedida el {}{}",
                    fecha_o_nada(c.fecha),
                    asiento(c)
                )
            })
            .collect();
        filas.push(vec![
            r.id.clone().unwrap_or_default(),
            organo.clone(),
            fecha_o_nada(r.o.fecha),
            fecha_o_nada(r.notificado),
            plazo,
            años.join(", "),
            r.conceptos.join(", "),
            if contestado.is_empty() {
                "ninguna en la carpeta".to_owned()
            } else {
                contestado.join("; ")
            } + &ampliado
                .iter()
                .map(|a| format!("; {a}"))
                .collect::<String>(),
        ]);
        hallazgo(exp, r, &organo, &contestado, hoy, s);
    }
    s.tabla(
        "Requerimientos de la AEAT",
        "Requerimientos AEAT",
        &[
            "Expediente",
            "Órgano",
            "Fecha",
            "Notificado",
            "Plazo hasta",
            "Ejercicios",
            "Impuestos que cita",
            "Contestaciones",
        ],
        filas,
    );
}

/// Hasta cuándo se puede comprobar un ejercicio sin interrupciones: cuatro años desde el día
/// siguiente al último del plazo de su declaración (arts. 66.a y 67.1 LGT). La del impuesto que
/// cita el requerimiento (la más tardía si cita varios): el Impuesto sobre Sociedades (200), el
/// IVA (390) o las retenciones (190); si no cita ninguno, la del Impuesto sobre Sociedades en una
/// sociedad y la de la Renta en una persona física.
fn prescripcion_ordinaria(exp: &Expediente, y: i32, conceptos: &[String]) -> Option<Date> {
    use taxpain_core::calendar::{Persona, deadlines_in_year};
    let perfil = super::cruces::perfil_amplio(exp);
    let mut modelos: Vec<&str> = conceptos
        .iter()
        .filter_map(|c| match c.as_str() {
            "Impuesto sobre Sociedades" => Some("200"),
            "IVA" => Some("390"),
            "retenciones" => Some("190"),
            "IRPF" => Some("100"),
            _ => None,
        })
        .collect();
    if modelos.is_empty() {
        modelos.push(if perfil.persona == Persona::Juridica {
            "200"
        } else {
            "100"
        });
    }
    let (h, _) = calendar_for(y + 1, perfil.ccaa, &perfil.festivos_locales);
    deadlines_in_year(&perfil, y + 1, &h)
        .into_iter()
        .filter(|d| d.fiscal_year == y && modelos.contains(&d.model.code()))
        .map(|d| d.due_on)
        .max()
        .and_then(|d| mas_meses(d, 48))
}

/// Lo que se dice de un requerimiento: si no está su contestación (y el plazo acabó, o no se
/// sabe su fecha) o si los ejercicios que pedía pueden seguir abiertos por él.
fn hallazgo(
    exp: &Expediente,
    r: &Requerimiento<'_>,
    organo: &str,
    contestado: &[String],
    hoy: Date,
    s: &mut Salida,
) {
    let f = r.o.fecha;
    let cual = format!(
        "{}{}{}",
        if organo == "AEAT" {
            "de la AEAT".to_owned()
        } else {
            format!("de {organo}")
        },
        f.map_or_else(String::new, |f| format!(" del {}", fecha(f))),
        r.id.as_ref()
            .map_or_else(String::new, |i| format!(" ({i})"))
    );
    let mut ficheros = vec![r.doc.ruta.clone()];
    ficheros.extend(r.contestaciones.iter().map(|c| c.fichero.clone()));
    ficheros.extend(r.ampliaciones.iter().map(|c| c.fichero.clone()));
    let vencido = r.plazo.is_some_and(|(p, _)| p < hoy);
    if r.contestaciones.is_empty() && (vencido || f.is_none()) {
        s.add(
            Nuevo {
                codigo: "aeat.requerimiento_sin_contestar",
                area: Area::Aeat,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Documentacion,
                titulo: format!("No está la contestación al requerimiento {cual}"),
                ficheros,
            }
            .detalle(format!(
                "En la carpeta no hay ningún recibo de presentación que conteste a este \
                 requerimiento{}. Si no se contestó en plazo, puede ser una infracción (art. 203 \
                 LGT). Qué hacer: pide a la gestoría el recibo de la contestación (está en la \
                 Sede, en «Mis expedientes») o que lo conteste cuanto antes.",
                if f.is_none() {
                    ", y no se ha podido leer su fecha para saber si el plazo ha acabado"
                } else {
                    ""
                }
            )),
            f.map(Date::year),
            None,
        );
        return;
    }
    if r.o.ejercicios.is_empty() {
        return;
    }
    let Some(desde) = r.notificado.or(f) else {
        return;
    };
    let Some(hasta) = mas_meses(desde, 48) else {
        return;
    };
    let ultima = r
        .contestaciones
        .iter()
        .filter_map(|c| c.fecha)
        .max()
        .and_then(|c| mas_meses(c, 48))
        .filter(|u| *u > hasta);
    let final_ = ultima.unwrap_or(hasta);
    // Sólo cambia algo en los ejercicios que habrían prescrito antes: los demás siguen
    // abiertos por su plazo ordinario.
    let (afectados, despues): (BTreeSet<i32>, BTreeSet<i32>) =
        r.o.ejercicios.iter().partition(|y| {
            prescripcion_ordinaria(exp, **y, &r.conceptos).is_none_or(|o| o < final_)
        });
    if afectados.is_empty() {
        return;
    }
    let años = super::modelos::años_texto(&r.o.ejercicios);
    let abiertos = super::modelos::años_texto(&afectados);
    let abierto = final_ >= hoy;
    let mut detalle = format!(
        "Pedía información de {años}{}{}. ",
        if r.conceptos.is_empty() {
            String::new()
        } else {
            format!(" (cita: {})", r.conceptos.join(", "))
        },
        r.notificado
            .map_or_else(String::new, |n| format!(" y se notificó el {}", fecha(n)))
    );
    if !contestado.is_empty() {
        detalle.push_str(&format!("Contestaciones: {}. ", lista(contestado, 6)));
    }
    detalle.push_str(&format!(
        "Un requerimiento notificado puede interrumpir la prescripción de lo que pide (art. 68.1.a \
         LGT), y las contestaciones también (art. 68.1.c LGT); después, los cuatro años vuelven a \
         empezar (art. 68.6 LGT). Si fue así, lo de {abiertos} que pedía se podría comprobar hasta \
         el {}{}, aunque por las fechas de presentación ya hubiera prescrito. ",
        fecha(hasta),
        ultima.map_or_else(String::new, |u| format!(
            " (o hasta el {}, contando la última contestación)",
            fecha(u)
        ))
    ));
    if let Some(y) = despues.iter().max() {
        detalle.push_str(&format!(
            "De {} no cambia nada: su plazo ordinario acaba después (el de {y}, el {}). ",
            super::modelos::años_texto(&despues),
            prescripcion_ordinaria(exp, *y, &r.conceptos).map_or_else(String::new, fecha)
        ));
    }
    detalle.push_str(if abierto {
        "Guarda el expediente y lo que se entregó al menos hasta entonces y confirma con la \
         gestoría qué impuestos y ejercicios quedan abiertos."
    } else {
        "Esas fechas ya han pasado."
    });
    s.add(
        Nuevo {
            codigo: "aeat.requerimiento",
            area: Area::Aeat,
            gravedad: if abierto {
                Gravedad::Aviso
            } else {
                Gravedad::Info
            },
            responsable: Responsable::Gestoria,
            titulo: format!("Requerimiento {cual}: {abiertos} pueden seguir abiertos"),
            ficheros,
        }
        .detalle(detalle),
        Some(desde.year()),
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn meses_despues() {
        assert_eq!(
            mas_meses(date!(2025 - 10 - 10), 12),
            Some(date!(2026 - 10 - 10))
        );
        assert_eq!(
            mas_meses(date!(2023 - 05 - 08), 48),
            Some(date!(2027 - 05 - 08))
        );
        assert_eq!(
            mas_meses(date!(2024 - 01 - 31), 1),
            Some(date!(2024 - 02 - 29))
        );
        assert_eq!(
            mas_meses(date!(2025 - 11 - 30), 6),
            Some(date!(2026 - 05 - 30))
        );
    }
}
