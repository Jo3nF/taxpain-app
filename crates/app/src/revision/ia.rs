//! Un hallazgo de una revisión en una conversación con la IA: el tema que se envía (texto que
//! genera TaxPain; se anonimiza después, con lo demás) y la cabecera que ve el usuario en el
//! panel (lo que sabe TaxPain, su primer mensaje y preguntas para empezar).
//!
//! Nunca van las rutas de los ficheros (suelen llevar el nombre de un cliente o un proveedor):
//! sólo cuántos son, y donde el texto las cita van como «fichero 1», «fichero 2»… El detalle de
//! la revisión de la carpeta puede citar lo que dicen sus documentos: va como documento externo.

use taxpain_core::money::Money;

use super::{Gravedad, Hallazgo, Origen, Responsable, Revision, componer, plural};
use crate::conversaciones::TopicHeader;

/// Con punto final (también tras un paréntesis o unas comillas).
fn frase(t: &str) -> String {
    let t = t.trim();
    if t.ends_with(['.', '!', '?', '…']) || t.is_empty() {
        t.to_owned()
    } else {
        format!("{t}.")
    }
}

/// `aaaa-mm-dd` → `dd/mm/aaaa`.
fn fecha(iso: &str) -> Option<String> {
    let mut p = iso.split('-');
    let (y, m, d) = (p.next()?, p.next()?, p.next()?);
    let bien = y.len() == 4 && m.len() == 2 && d.len() == 2;
    bien.then(|| format!("{d}/{m}/{y}"))
}

fn gravedad_ia(g: Gravedad) -> &'static str {
    match g {
        Gravedad::Error => "error (algo está mal y cuesta dinero o tiene riesgo)",
        Gravedad::Aviso => "aviso (hay que revisarlo)",
        Gravedad::Info => "para saber",
    }
}

fn gravedad_usuario(g: Gravedad) -> &'static str {
    match g {
        Gravedad::Error => "Error",
        Gravedad::Aviso => "Aviso",
        Gravedad::Info => "Para saber",
    }
}

fn responsable_ia(r: Responsable) -> &'static str {
    match r {
        Responsable::Empresa => "la empresa",
        Responsable::Gestoria => "la gestoría",
        Responsable::Documentacion => "hay que conseguir un documento que falta",
    }
}

/// La línea de arriba del panel (igual que `Topic::group`).
pub(crate) fn grupo(origen: Origen, ejercicio: Option<i32>) -> String {
    let de = match origen {
        Origen::Aeat => "Qué revisar",
        Origen::Carpeta => "Revisión de la carpeta",
    };
    match ejercicio {
        Some(y) => format!("{de} · {y}"),
        None => de.to_owned(),
    }
}

/// El texto sin las rutas de `ficheros` ni sus nombres sueltos: cada una pasa a «fichero N»,
/// por su orden en la lista. Las más largas primero, para no partir una ruta por su nombre.
fn sin_rutas(texto: &str, ficheros: &[String]) -> String {
    let mut cambios: Vec<(String, usize)> = Vec::new();
    for (i, f) in ficheros.iter().enumerate() {
        let f = componer(f.trim());
        let nombre = f.rsplit_once('/').map(|(_, n)| n.to_owned());
        if f.chars().count() >= 5 {
            cambios.push((f, i + 1));
        }
        // El nombre suelto sólo si parece un fichero (con extensión): «2025» no se toca.
        if let Some(n) = nombre.filter(|n| n.chars().count() >= 5 && n.contains('.'))
            && !cambios.iter().any(|(c, _)| *c == n)
        {
            cambios.push((n, i + 1));
        }
    }
    cambios.sort_by_key(|(c, _)| std::cmp::Reverse(c.len()));
    let mut out = componer(texto);
    for (c, n) in cambios {
        out = out.replace(&c, &format!("fichero {n}"));
    }
    out
}

/// El tema de un hallazgo, su cabecera y los terceros que nombra (van con marcadores). `None`
/// si ya no sale en la revisión `r` (se busca por su código, su ejercicio y su título: el
/// código y el ejercicio no bastan).
pub(crate) fn tema_hallazgo(
    r: &Revision,
    origen: Origen,
    codigo: &str,
    ejercicio: Option<i32>,
    titulo: &str,
) -> Option<(TopicHeader, String, Vec<String>)> {
    let h = r
        .hallazgos
        .iter()
        .find(|h| h.codigo == codigo && h.ejercicio == ejercicio && h.titulo == titulo)?;
    let (header, block) = tema(h, origen);
    Some((header, block, h.terceros.clone().unwrap_or_default()))
}

fn tema(h: &Hallazgo, origen: Origen) -> (TopicHeader, String) {
    let importe = h.importe.filter(|m| m.cents() != 0);
    // Aproximada si TaxPain no tiene los festivos del año en que acababa el plazo.
    let rectificable = h.rectificable_hasta.as_deref().and_then(fecha).map(|f| {
        if h.rectificable_aprox {
            format!("{f} aprox.")
        } else {
            f
        }
    });
    let (de, pantalla) = match origen {
        Origen::Aeat => (
            "«Qué revisar» de TaxPain (lo que tiene la AEAT frente a lo que sabe TaxPain)",
            "Evolución → Qué revisar",
        ),
        Origen::Carpeta => (
            "la revisión de la carpeta de la empresa de TaxPain (sus documentos frente a lo que \
             sabe TaxPain)",
            "Importar → Revisión",
        ),
    };

    let mut b = format!("Hallazgo de {de}: «{}».", sin_rutas(&h.titulo, &h.ficheros));
    b.push_str(&format!(
        "\n- Área: {}. Importancia: {}. Quién tiene que actuar: {}.",
        h.area.titulo(),
        gravedad_ia(h.gravedad),
        responsable_ia(h.responsable)
    ));
    if let Some(y) = h.ejercicio {
        b.push_str(&format!("\n- Ejercicio: {y}."));
    }
    match importe {
        Some(m) if m > Money::ZERO => b.push_str(&format!(
            "\n- Recuperable si se corrige (estimación de TaxPain): {m}."
        )),
        Some(m) => b.push_str(&format!(
            "\n- A pagar (estimación de TaxPain): {}.",
            m.abs()
        )),
        None => {}
    }
    if let Some(f) = &rectificable {
        b.push_str(&format!(
            "\n- Se puede rectificar hasta el {f}: cuatro años desde el fin del plazo de \
             presentación (art. 66 de la Ley 58/2003 General Tributaria, BOE BOE-A-2003-23186, \
             artículo 66).{}",
            if h.rectificable_aprox {
                " Es aproximado: TaxPain no tiene los festivos del año en que acababa el plazo de \
                 presentación, y si cayó en festivo el plazo para rectificar acaba algún día \
                 después."
            } else {
                ""
            }
        ));
    }
    if !h.ficheros.is_empty() {
        b.push_str(&format!(
            "\n- {} (sus nombres no se envían: donde salen, van como «fichero 1», «fichero 2»…).",
            plural(h.ficheros.len(), "fichero afectado", "ficheros afectados")
        ));
    }
    let detalle = sin_rutas(&h.detalle, &h.ficheros);
    match origen {
        Origen::Aeat => b.push_str(&format!("\n- Detalle: {}", frase(&detalle))),
        Origen::Carpeta => b.push_str(&format!(
            "\n- Detalle (cita lo que dicen los documentos de la carpeta):\n{}",
            taxpain_assistant::external_document("detalle del hallazgo", &detalle)
        )),
    }
    b.push_str(&format!("\n- Pantalla de TaxPain donde se ve: {pantalla}."));
    b.push_str(
        "\n- TaxPain sólo revisa y avisa: no corrige ni presenta nada. Corregir un modelo (una \
         complementaria o una rectificación) lo decide el usuario con su gestoría; las cifras \
         son estimaciones para confirmar con ella.",
    );

    let mut facts = vec![
        (
            "Importancia".to_owned(),
            gravedad_usuario(h.gravedad).to_owned(),
        ),
        ("Responsable".to_owned(), h.responsable.titulo().to_owned()),
        ("Área".to_owned(), h.area.titulo().to_owned()),
    ];
    if let Some(y) = h.ejercicio {
        facts.push(("Ejercicio".to_owned(), y.to_string()));
    }
    match importe {
        Some(m) if m > Money::ZERO => facts.push(("Recuperable".to_owned(), m.to_string())),
        Some(m) => facts.push(("A pagar".to_owned(), m.abs().to_string())),
        None => {}
    }
    if let Some(f) = &rectificable {
        facts.push(("Se puede rectificar hasta".to_owned(), f.clone()));
    }
    if !h.ficheros.is_empty() {
        facts.push((
            "Ficheros afectados".to_owned(),
            h.ficheros.len().to_string(),
        ));
    }

    let apertura = match origen {
        Origen::Aeat => "Al comparar lo que tiene la AEAT con lo que sabe TaxPain sale esto:",
        Origen::Carpeta => "Al revisar la carpeta de la empresa sale esto:",
    };
    let mut intro = format!("{apertura} {}", frase(&h.detalle));
    match importe {
        Some(m) if m > Money::ZERO => intro.push_str(&format!(" Recuperable (estimado): {m}.")),
        Some(m) => intro.push_str(&format!(" A pagar (estimado): {}.", m.abs())),
        None => {}
    }
    if let Some(f) = &rectificable {
        intro.push_str(&format!(" Se puede rectificar hasta el {f}."));
    }
    intro.push_str(
        " Pregunta lo que quieras o añade datos que TaxPain no tenga (cifras, decisiones o \
         documentos).",
    );

    let mut suggestions = vec![
        "¿Qué significa y es grave?".to_owned(),
        "¿Qué le pido a mi gestoría?".to_owned(),
    ];
    if importe.is_some_and(|m| m > Money::ZERO) {
        suggestions.push("¿Cómo pido la devolución y hasta cuándo puedo?".to_owned());
    }

    (
        TopicHeader {
            group: grupo(origen, h.ejercicio),
            title: h.titulo.clone(),
            facts,
            intro,
            suggestions,
            notice: None,
        },
        b,
    )
}

#[cfg(test)]
mod tests {
    use super::super::{Area, Resumen};
    use super::*;

    fn revision(hallazgos: Vec<Hallazgo>) -> Revision {
        Revision {
            generada: "2026-10-30".into(),
            empresa: "Cliente Uno SL".into(),
            nif: Some("B12345674".into()),
            carpeta: "/Users/prueba/Gestoría".into(),
            resumen: Resumen {
                ficheros: 3,
                con_detalle: 3,
                sin_leer: 0,
                no_admitidos: 0,
                por_tipo: vec![],
                errores: 0,
                avisos: 1,
                info: 0,
                urgentes: 0,
                en_juego_urgente: Money::ZERO,
                recuperable: Money::ZERO,
                a_favor_proximo_200: Money::ZERO,
                retenciones_proximo_200: Money::ZERO,
                exencion_proximo_200: Money::ZERO,
                impuesto_dividendos_proximo_200: Money::ZERO,
            },
            hallazgos,
            tablas: vec![],
            pedir: vec![],
            dividendos: vec![],
        }
    }

    /// Un hallazgo de la carpeta que cita sus ficheros (con el nombre de un proveedor en la ruta).
    fn libro() -> Hallazgo {
        Hallazgo {
            codigo: "libros.sin_pdf".into(),
            area: Area::Libros,
            gravedad: Gravedad::Aviso,
            responsable: Responsable::Documentacion,
            ejercicio: Some(2025),
            titulo: "2 facturas del libro de recibidas de 2025 sin el PDF en la carpeta".into(),
            detalle: "Están en el libro (2025/Libros/Libro recibidas 2025.xlsx) pero no hay su \
                      factura en la carpeta: F-0912, F-0288. Mira también 2025/Proveedores/\
                      Imprenta Villalobos/F-0911.pdf y F-0911.pdf."
                .into(),
            ficheros: vec![
                "2025/Libros/Libro recibidas 2025.xlsx".into(),
                "2025/Proveedores/Imprenta Villalobos/F-0911.pdf".into(),
            ],
            importe: None,
            rectificable_hasta: None,
            rectificable_aprox: false,
            terceros: Some(vec![]),
            urgente: None,
        }
    }

    fn fuera_de_plazo() -> Hallazgo {
        Hallazgo {
            codigo: "cruces.fuera_de_plazo".into(),
            area: Area::Plazos,
            gravedad: Gravedad::Aviso,
            responsable: Responsable::Gestoria,
            ejercicio: Some(2024),
            titulo: "El 111 del 2T 2024 se presentó fuera de plazo".into(),
            detalle: "Venció el 22/07/2024 y se presentó el 05/09/2024: recargo del 2 % sobre \
                      lo ingresado (art. 27 LGT)"
                .into(),
            ficheros: vec!["AEAT · modelo 111 2024 2T · justificante 1110000000002".into()],
            importe: Some(Money::from_cents(-9_100)),
            rectificable_hasta: Some("2028-04-20".into()),
            rectificable_aprox: false,
            terceros: Some(vec![]),
            urgente: None,
        }
    }

    #[test]
    fn bloque_del_hallazgo_sin_rutas_ni_nombres_de_ficheros() {
        let r = revision(vec![fuera_de_plazo(), libro()]);
        let h = libro();
        let (cab, b, _) =
            tema_hallazgo(&r, Origen::Carpeta, &h.codigo, h.ejercicio, &h.titulo).unwrap();
        assert!(
            b.starts_with(
                "Hallazgo de la revisión de la carpeta de la empresa de TaxPain (sus documentos \
                 frente a lo que sabe TaxPain): «2 facturas del libro de recibidas de 2025 sin el \
                 PDF en la carpeta».\n- Área: Libros registro. Importancia: aviso (hay que \
                 revisarlo). Quién tiene que actuar: hay que conseguir un documento que falta.\n\
                 - Ejercicio: 2025."
            ),
            "{b}"
        );
        assert!(b.contains("\n- 2 ficheros afectados (sus nombres no se envían"));
        // El detalle, como documento externo y con los ficheros por su número.
        assert!(b.contains(
            "<<<DOCUMENTO EXTERNO: detalle del hallazgo>>>\nEstán en el libro (fichero 1) pero \
             no hay su factura en la carpeta: F-0912, F-0288. Mira también fichero 2 y fichero 2."
        ));
        for fuera in [
            "Villalobos",
            "Proveedores",
            "Libro recibidas",
            ".xlsx",
            ".pdf",
            "/Users/prueba",
            "Cliente Uno",
            "B12345674",
        ] {
            assert!(!b.contains(fuera), "«{fuera}» en el tema:\n{b}");
        }
        assert!(b.contains("\n- Pantalla de TaxPain donde se ve: Importar → Revisión."));
        // El usuario sí ve el detalle tal cual (no se envía).
        assert!(cab.intro.contains("Imprenta Villalobos/F-0911.pdf"));
        // Otro título, otro ejercicio u otro código: ya no es ese hallazgo.
        assert!(tema_hallazgo(&r, Origen::Carpeta, &h.codigo, Some(2024), &h.titulo).is_none());
        assert!(tema_hallazgo(&r, Origen::Carpeta, &h.codigo, h.ejercicio, "Otro").is_none());
        assert!(
            tema_hallazgo(&r, Origen::Carpeta, "libros.otro", h.ejercicio, &h.titulo).is_none()
        );
    }

    #[test]
    fn bloque_de_un_hallazgo_de_la_aeat_con_importe_y_plazo() {
        let h = fuera_de_plazo();
        let (_, b) = tema(&h, Origen::Aeat);
        assert!(b.starts_with(
            "Hallazgo de «Qué revisar» de TaxPain (lo que tiene la AEAT frente a lo que sabe \
             TaxPain): «El 111 del 2T 2024 se presentó fuera de plazo».\n- Área: Plazos y \
             Registro Mercantil. Importancia: aviso (hay que revisarlo). Quién tiene que actuar: \
             la gestoría.\n- Ejercicio: 2024.\n- A pagar (estimación de TaxPain): 91,00 €.\n- Se \
             puede rectificar hasta el 20/04/2028: cuatro años desde el fin del plazo de \
             presentación (art. 66 de la Ley 58/2003 General Tributaria, BOE BOE-A-2003-23186, \
             artículo 66).\n- 1 fichero afectado"
        ));
        // Lo de la AEAT lo escribe TaxPain: no va como documento externo.
        assert!(b.contains(
            "\n- Detalle: Venció el 22/07/2024 y se presentó el 05/09/2024: recargo del 2 % sobre \
             lo ingresado (art. 27 LGT).\n"
        ));
        assert!(!b.contains("DOCUMENTO EXTERNO"));
        assert!(!b.contains("justificante 1110000000002"));
        assert!(b.contains("Evolución → Qué revisar"));
    }

    #[test]
    fn cabecera_y_sugerencias_del_hallazgo() {
        let (h, _) = tema(&fuera_de_plazo(), Origen::Aeat);
        assert_eq!(h.group, "Qué revisar · 2024");
        assert_eq!(h.title, "El 111 del 2T 2024 se presentó fuera de plazo");
        assert_eq!(
            h.facts,
            [
                ("Importancia", "Aviso"),
                ("Responsable", "Gestoría"),
                ("Área", "Plazos y Registro Mercantil"),
                ("Ejercicio", "2024"),
                ("A pagar", "91,00 €"),
                ("Se puede rectificar hasta", "20/04/2028"),
                ("Ficheros afectados", "1"),
            ]
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
        );
        assert_eq!(
            h.intro,
            "Al comparar lo que tiene la AEAT con lo que sabe TaxPain sale esto: Venció el \
             22/07/2024 y se presentó el 05/09/2024: recargo del 2 % sobre lo ingresado (art. 27 \
             LGT). A pagar (estimado): 91,00 €. Se puede rectificar hasta el 20/04/2028. Pregunta \
             lo que quieras o añade datos que TaxPain no tenga (cifras, decisiones o documentos)."
        );
        assert_eq!(
            h.suggestions,
            ["¿Qué significa y es grave?", "¿Qué le pido a mi gestoría?"]
        );
        // Con algo que recuperar, cómo pedir la devolución.
        let mut d = fuera_de_plazo();
        d.importe = Some(Money::from_cents(456_000));
        let (h, b) = tema(&d, Origen::Aeat);
        assert_eq!(
            h.suggestions[2],
            "¿Cómo pido la devolución y hasta cuándo puedo?"
        );
        assert!(
            h.facts
                .contains(&("Recuperable".into(), "4.560,00 €".into()))
        );
        assert!(b.contains("\n- Recuperable si se corrige (estimación de TaxPain): 4.560,00 €."));
        // Sin ejercicio, la línea de arriba no lo lleva.
        let (h, _) = tema(
            &Hallazgo {
                ejercicio: None,
                ..libro()
            },
            Origen::Carpeta,
        );
        assert_eq!(h.group, "Revisión de la carpeta");
    }

    /// Con el plazo de rectificar aproximado (TaxPain no tiene los festivos de ese año), la fecha
    /// lleva «aprox.» y el bloque explica por qué.
    #[test]
    fn el_plazo_aproximado_se_dice() {
        let h = Hallazgo {
            rectificable_hasta: Some("2028-07-25".into()),
            rectificable_aprox: true,
            ..fuera_de_plazo()
        };
        let (cab, b) = tema(&h, Origen::Carpeta);
        assert!(
            cab.facts.contains(&(
                "Se puede rectificar hasta".into(),
                "25/07/2028 aprox.".into()
            )),
            "{:?}",
            cab.facts
        );
        assert!(
            cab.intro.contains("hasta el 25/07/2028 aprox."),
            "{}",
            cab.intro
        );
        assert!(b.contains("Es aproximado"), "{b}");
        let (cab, b) = tema(&fuera_de_plazo(), Origen::Carpeta);
        assert!(
            cab.facts
                .contains(&("Se puede rectificar hasta".into(), "20/04/2028".into()))
        );
        assert!(!b.contains("aproximado"), "{b}");
    }

    #[test]
    fn el_texto_fijo_no_tiene_marcadores_por_patron() {
        // Fechas, importes, el identificador del BOE y los artículos llegan a la IA tal cual.
        let mut s = taxpain_assistant::pii::Scrubber::new().without_names();
        let (_, b) = tema(&fuera_de_plazo(), Origen::Aeat);
        assert_eq!(s.scrub(&b), b);
        assert_eq!(s.replaced(), 0);
    }

    #[test]
    fn sin_rutas_no_toca_lo_que_no_es_un_fichero() {
        let f = vec!["2025".to_owned(), "Escrituras constitución.pdf".to_owned()];
        assert_eq!(
            sin_rutas("Ejercicio 2025: falta Escrituras constitución.pdf.", &f),
            "Ejercicio 2025: falta fichero 2."
        );
        // Los nombres del Mac llegan descompuestos (NFD): se comparan compuestos.
        let nfd = vec!["Facturas/Mun\u{303}oz.pdf".to_owned()];
        assert_eq!(
            sin_rutas("Ver Facturas/Mun\u{303}oz.pdf y Muñoz.pdf", &nfd),
            "Ver fichero 1 y fichero 1"
        );
    }
}
