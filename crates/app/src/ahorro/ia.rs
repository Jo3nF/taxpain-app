//! «Ahorrar impuestos» en una conversación con la IA: el tema que se envía de una recomendación o
//! del plan del año (texto que genera TaxPain; se anonimiza después, con lo demás) y la cabecera
//! que ve el usuario en el panel (lo que sabe TaxPain, su primer mensaje y preguntas para
//! empezar).

use time::Date;

use super::{Ahorro, Area, Encaje, Estado, Recomendacion, Situacion};
use crate::conversaciones::TopicHeader;

/// Las fichas de planificación fiscal: la IA tiene que avisar de sus riesgos con la AEAT.
const PLANIFICACION: &[&str] = &[
    "pasar_a_sociedad",
    "retribucion_administrador",
    "retribucion_flexible",
];

const AVISO_PLANIFICACION: &str = "Cuidado: es planificación fiscal. Cíñete a la ficha y a la \
ley; avisa de que la AEAT puede revisarla por conflicto en la aplicación de la norma o simulación \
(arts. 15 y 16 LGT) y, si afecta a socios o administradores, por operaciones vinculadas a valor de \
mercado (art. 18 LIS); recomienda decidirlo con su asesor.";

fn fecha(d: Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

/// «31 de diciembre de 2026».
fn fecha_larga(d: Date) -> String {
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
    let mes = MESES[usize::from(u8::from(d.month())) - 1];
    format!("{} de {mes} de {}", d.day(), d.year())
}

/// Con punto final.
fn frase(t: &str) -> String {
    let t = t.trim();
    if t.ends_with(['.', '!', '?', ')', '»']) || t.is_empty() {
        t.to_owned()
    } else {
        format!("{t}.")
    }
}

fn area(a: Area) -> &'static str {
    match a {
        Area::Beneficio => "Impuesto sobre el beneficio",
        Area::Inversion => "Inversiones que desgravan",
        Area::Gastos => "Gastos y amortizaciones",
        Area::Personas => "Sueldos y pensiones",
        Area::Iva => "IVA",
        Area::Donativos => "Donativos",
        Area::Cripto => "Criptomonedas",
    }
}

/// El encaje para la IA (del usuario, en tercera persona).
fn encaje_ia(e: Encaje) -> &'static str {
    match e {
        Encaje::Conviene => "le conviene",
        Encaje::Valorar => "puede interesarle: depende de algo que TaxPain no sabe",
        Encaje::Aprovechado => "ya lo aprovecha",
        Encaje::NoAplica => "no le aplica",
    }
}

/// El encaje como lo ve el usuario en la tarjeta.
fn encaje_usuario(e: Encaje) -> &'static str {
    match e {
        Encaje::Conviene => "Te conviene",
        Encaje::Valorar => "Puede interesarte",
        Encaje::Aprovechado => "Ya lo aprovechas",
        Encaje::NoAplica => "No te aplica",
    }
}

fn estado_ia(e: Estado) -> &'static str {
    match e {
        Estado::Pendiente => "pendiente",
        Estado::EnMarcha => "en marcha",
        Estado::Hecho => "hecho",
        Estado::Descartado => "no le interesa",
    }
}

fn estado_usuario(e: Estado) -> &'static str {
    match e {
        Estado::Pendiente => "Pendiente",
        Estado::EnMarcha => "En marcha",
        Estado::Hecho => "Hecho",
        Estado::Descartado => "No me interesa",
    }
}

/// La pantalla de TaxPain de `Recomendacion::pantalla`, como se llama en la app.
fn pantalla(p: &str) -> Option<&'static str> {
    Some(match p {
        "modelos" => "Modelos",
        "evolucion" => "Evolución",
        "cartera" => "Cobros y pagos",
        "cripto" => "Criptomonedas",
        "contabilidad" => "Contabilidad",
        "inmovilizado" => "Contabilidad → Inmovilizado",
        "recibidas" => "Recibidas",
        _ => return None,
    })
}

/// La norma con su identificador del BOE y el artículo, sacados del enlace
/// (`…act.php?id=BOE-A-2014-12328#a25`), para que la IA la pueda leer con `leer_articulo`.
fn norma(r: &Recomendacion) -> String {
    let id = r
        .enlace
        .split_once("id=")
        .map(|(_, rest)| rest.split(['#', '&']).next().unwrap_or(""))
        .filter(|id| id.starts_with("BOE-"));
    let articulo = r
        .enlace
        .rsplit_once("#a")
        .map(|(_, a)| a)
        .filter(|a| !a.is_empty() && a.bytes().all(|b| b.is_ascii_digit()));
    match (id, articulo) {
        (Some(id), Some(a)) => format!("{} (BOE {id}, artículo {a})", r.norma),
        (Some(id), None) => format!("{} (BOE {id})", r.norma),
        _ => format!("{} (enlace: {})", r.norma, r.enlace),
    }
}

/// «Situación usada: …» (y los avisos de TaxPain sobre esas cifras), igual en una tarjeta y en
/// el plan.
fn situacion(s: &Situacion) -> String {
    let mut partes = vec![if s.persona_fisica {
        "persona física (autónomo)".to_owned()
    } else {
        "sociedad".to_owned()
    }];
    if let Some(r) = &s.regimen {
        partes.push(r.clone());
    }
    if let Some(c) = &s.cifra_negocios {
        partes.push(format!("cifra de negocios {} ({})", c.valor, c.fuente));
    }
    if let Some(b) = &s.base {
        let que = if s.persona_fisica {
            "rendimiento neto"
        } else {
            "base imponible"
        };
        partes.push(format!("{que} {} ({})", b.valor, b.fuente));
    }
    if let Some(c) = &s.cuota {
        partes.push(format!("cuota {} ({})", c.valor, c.fuente));
    }
    if let Some(t) = s.tipo {
        partes.push(format!("tipo para estimar el ahorro {t} %"));
    }
    partes.push(
        if s.trabajadores {
            "con trabajadores"
        } else {
            "sin trabajadores"
        }
        .to_owned(),
    );
    if let Some(c) = &s.cnae {
        partes.push(format!("CNAE {c}"));
    }
    let mut out = format!("- Situación usada: {}.", partes.join("; "));
    if !s.notas.is_empty() {
        out.push_str(&format!(
            "\n- Avisos de TaxPain sobre estas cifras: {}",
            s.notas
                .iter()
                .map(|n| frase(n))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    out
}

/// El plazo como lo dice la tarjeta: hasta el cierre del ejercicio o una fecha límite.
fn plazo(r: &Recomendacion, cierre: Date) -> Option<(String, String)> {
    let p = r.plazo?;
    Some(if p == cierre {
        (
            format!(
                "Cuenta lo que haga hasta el cierre del ejercicio ({})",
                fecha(p)
            ),
            format!("hasta el cierre del ejercicio ({})", fecha(p)),
        )
    } else {
        (format!("Fecha límite: {}", fecha(p)), fecha(p))
    })
}

/// El tema de una recomendación y su cabecera. `None` si la ficha no es para este contribuyente
/// (no está entre las recomendaciones).
pub(crate) fn tema_tarjeta(a: &Ahorro, id: &str) -> Option<(TopicHeader, String)> {
    let r = a.recomendaciones.iter().find(|r| r.id == id)?;
    let s = &a.situacion;
    let hechos = r.marca.pasos.iter().filter(|i| **i < r.pasos.len()).count();

    let mut b = format!(
        "Recomendación de «Ahorrar impuestos» de TaxPain para el ejercicio {}: «{}» ({}).",
        s.año,
        r.titulo,
        area(r.area)
    );
    b.push_str(&format!(
        "\n- Encaje según TaxPain: {}. {}",
        encaje_ia(r.encaje),
        frase(&r.motivo)
    ));
    if let Some(x) = r.ahorro {
        let que = if r.aplazado {
            "Impuesto aplazado (se paga más adelante)"
        } else {
            "Ahorro estimado"
        };
        match &r.calculo {
            Some(c) => b.push_str(&format!("\n- {que}: {x} (cálculo: {c}).")),
            None => b.push_str(&format!("\n- {que}: {x}.")),
        }
    }
    if let Some((texto, _)) = plazo(r, s.cierre) {
        b.push_str(&format!("\n- {texto}."));
    }
    b.push_str(&format!(
        "\n- Estado que ha marcado el usuario: {}.",
        estado_ia(r.marca.estado)
    ));
    b.push_str(&format!("\n- Qué es: {}", frase(r.resumen)));
    if !r.requisitos.is_empty() {
        b.push_str(&format!("\n- Requisitos: {}", r.requisitos.join(" | ")));
    }
    b.push_str(&format!(
        "\n- Pasos ({hechos} de {} hechos):",
        r.pasos.len()
    ));
    for (i, p) in r.pasos.iter().enumerate() {
        let marca = if r.marca.pasos.contains(&i) {
            "hecho"
        } else {
            "pendiente"
        };
        b.push_str(&format!("\n  {}. [{marca}] {p}", i + 1));
    }
    if !r.riesgos.is_empty() {
        b.push_str(&format!("\n- Ten en cuenta: {}", r.riesgos.join(" | ")));
    }
    b.push_str(&format!("\n- Norma: {}.", norma(r)));
    if let Some(p) = r.pantalla.and_then(pantalla) {
        b.push_str(&format!(
            "\n- Pantalla de TaxPain donde se pone o se ve: {p}."
        ));
    }
    b.push('\n');
    b.push_str(&situacion(s));
    if PLANIFICACION.contains(&r.id) {
        b.push_str("\n- ");
        b.push_str(AVISO_PLANIFICACION);
    }

    let mut facts = vec![("Encaje".to_owned(), encaje_usuario(r.encaje).to_owned())];
    if let Some(x) = r.ahorro {
        let que = if r.aplazado {
            "Impuesto aplazado"
        } else {
            "Ahorro estimado"
        };
        facts.push((que.to_owned(), x.to_string()));
    }
    if let Some((_, corto)) = plazo(r, s.cierre) {
        facts.push(("Fecha límite".to_owned(), corto));
    }
    facts.push((
        "Estado".to_owned(),
        estado_usuario(r.marca.estado).to_owned(),
    ));
    facts.push((
        "Pasos hechos".to_owned(),
        format!("{hechos} de {}", r.pasos.len()),
    ));
    facts.push(("Norma".to_owned(), r.norma.to_owned()));

    let apertura = match r.encaje {
        Encaje::Conviene => "Con tus datos, te conviene.",
        Encaje::Valorar => "Puede interesarte: depende de algo que TaxPain no sabe.",
        Encaje::Aprovechado => "Ya lo aprovechas.",
        Encaje::NoAplica => "Con tus datos, no te aplica.",
    };
    let mut intro = format!("{apertura} {}", frase(&r.motivo));
    if let Some(x) = r.ahorro {
        let que = if r.aplazado {
            "Impuesto aplazado"
        } else {
            "Ahorro estimado"
        };
        intro.push_str(&format!(" {que}: {x}."));
    }
    if let Some(p) = r.plazo {
        if p == s.cierre {
            intro.push_str(&format!(
                " Cuenta lo que hagas hasta el cierre del ejercicio ({}).",
                fecha(p)
            ));
        } else {
            intro.push_str(&format!(" Fecha límite: {}.", fecha(p)));
        }
    }
    intro.push_str(
        " Pregunta lo que quieras o añade datos que TaxPain no tenga (cifras, decisiones o \
         documentos).",
    );

    Some((
        TopicHeader {
            group: format!("Ahorrar impuestos · {}", s.año),
            title: r.titulo.to_owned(),
            facts,
            intro,
            suggestions: sugerencias(r),
            notice: None,
        },
        b,
    ))
}

/// Preguntas para empezar según el encaje (y lo que ha marcado el usuario).
fn sugerencias(r: &Recomendacion) -> Vec<String> {
    let calculo = if r.aplazado {
        "¿Cómo se ha calculado el impuesto aplazado?"
    } else {
        "¿Cómo se ha calculado el ahorro?"
    };
    let mut s: Vec<&str> = match r.encaje {
        Encaje::Conviene if r.ahorro.is_some() => vec![
            "Explícamelo con mis datos",
            "¿Qué tengo que hacer y antes de cuándo?",
            calculo,
        ],
        Encaje::Conviene => vec![
            "Explícamelo con mis datos",
            "¿Qué tengo que hacer y antes de cuándo?",
            "¿Qué riesgos tiene en mi caso?",
        ],
        Encaje::Valorar => vec![
            "¿Qué dato falta para saber si me conviene?",
            "Explícamelo con mis datos",
            "¿Qué riesgos tiene?",
        ],
        Encaje::Aprovechado => vec![
            "¿Lo aprovecho del todo?",
            "¿Qué tengo que vigilar los próximos años?",
        ],
        Encaje::NoAplica => vec![
            "¿Por qué no me aplica?",
            "¿Qué tendría que cambiar para que me aplique?",
        ],
    };
    if r.marca.estado == Estado::Hecho && s.len() > 1 {
        s[1] = "¿Qué papeles guardo por si me los piden?";
    }
    s.into_iter().map(str::to_owned).collect()
}

/// Lo que todavía puede hacer (ni hecho ni descartado).
fn abierta(r: &Recomendacion) -> bool {
    !matches!(r.marca.estado, Estado::Hecho | Estado::Descartado)
}

/// «1 recomendación», «3 recomendaciones».
fn recomendaciones(n: usize) -> String {
    if n == 1 {
        "1 recomendación".into()
    } else {
        format!("{n} recomendaciones")
    }
}

/// El tema del plan del año (todas las recomendaciones) y su cabecera.
pub(crate) fn tema_plan(a: &Ahorro) -> (TopicHeader, String) {
    let s = &a.situacion;
    let mut b = format!(
        "Plan de «Ahorrar impuestos» de TaxPain, ejercicio {} (cierre {}).",
        s.año,
        fecha(s.cierre)
    );
    b.push_str(&format!(
        "\n- Ahorro posible según TaxPain: {} (lo que le conviene y no ha hecho ni descartado; \
         TaxPain lo calcula en conjunto, porque las rebajas de la base juntas no ahorran lo \
         mismo que por separado).",
        a.ahorro_posible
    ));
    if a.aplazado_posible.cents() > 0 {
        b.push_str(&format!(
            "\n- Impuesto que se puede aplazar (se paga más adelante): {}.",
            a.aplazado_posible
        ));
    }
    b.push('\n');
    b.push_str(&situacion(s));
    b.push_str(
        "\n- Recomendaciones (id · título · encaje · ahorro o impuesto aplazado · plazo · \
         estado):",
    );
    for r in a.recomendaciones.iter().filter(|r| {
        matches!(
            r.encaje,
            Encaje::Conviene | Encaje::Valorar | Encaje::Aprovechado
        )
    }) {
        let ahorro = match r.ahorro {
            Some(x) if r.aplazado => format!("aplazado {x}"),
            Some(x) => format!("ahorro {x}"),
            None => "sin estimar".into(),
        };
        let plazo = plazo(r, s.cierre).map_or_else(|| "sin plazo".into(), |(_, p)| p);
        b.push_str(&format!(
            "\n  - {} · {} · {} · {ahorro} · {plazo} · {}",
            r.id,
            r.titulo,
            encaje_ia(r.encaje),
            estado_ia(r.marca.estado)
        ));
    }
    let no_aplican: Vec<&str> = a
        .recomendaciones
        .iter()
        .filter(|r| r.encaje == Encaje::NoAplica)
        .map(|r| r.titulo)
        .collect();
    if !no_aplican.is_empty() {
        b.push_str(&format!(
            "\n- No le aplican con sus datos: {}.",
            no_aplican.join("; ")
        ));
    }

    let convienen = a
        .recomendaciones
        .iter()
        .filter(|r| r.encaje == Encaje::Conviene && abierta(r))
        .count();
    let valorar = a
        .recomendaciones
        .iter()
        .filter(|r| r.encaje == Encaje::Valorar && abierta(r))
        .count();
    let mut facts = vec![("Ahorro posible".to_owned(), a.ahorro_posible.to_string())];
    if a.aplazado_posible.cents() > 0 {
        facts.push((
            "Impuesto que se puede aplazar".to_owned(),
            a.aplazado_posible.to_string(),
        ));
    }
    facts.push(("Te convienen".to_owned(), convienen.to_string()));
    facts.push(("Puedes valorar".to_owned(), valorar.to_string()));
    facts.push(("Cierre del ejercicio".to_owned(), fecha(s.cierre)));

    let intro = if a.ahorro_posible.cents() > 0 {
        let aplazar = if a.aplazado_posible.cents() > 0 {
            format!(" y aplazar unos {}", a.aplazado_posible)
        } else {
            String::new()
        };
        format!(
            "En {} podrías ahorrarte unos {} con {} que te {}{aplazar}. Pregunta por dónde \
             empezar o añade datos que TaxPain no tenga.",
            s.año,
            a.ahorro_posible,
            recomendaciones(convienen),
            if convienen == 1 {
                "conviene"
            } else {
                "convienen"
            }
        )
    } else {
        format!(
            "Según tus datos, en {} te {} {} y puedes valorar {valorar}. Pregunta por dónde \
             empezar o añade datos que TaxPain no tenga.",
            s.año,
            if convienen == 1 {
                "conviene"
            } else {
                "convienen"
            },
            recomendaciones(convienen),
        )
    };

    (
        TopicHeader {
            group: format!("Ahorrar impuestos · tu plan de {}", s.año),
            title: format!("Tu plan de ahorro de {}", s.año),
            facts,
            intro,
            suggestions: vec![
                "¿Por cuál empiezo?".into(),
                format!("¿Qué tengo que hacer antes del {}?", fecha_larga(s.cierre)),
                "¿Se pueden combinar entre ellas?".into(),
            ],
            notice: None,
        },
        b,
    )
}
