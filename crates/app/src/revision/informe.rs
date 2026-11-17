//! El informe de la revisión en Markdown, PDF y Excel.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use rust_xlsxwriter::{Format, Workbook};
use taxpain_invoicing::pdf::{Para, text_document};

use super::{Destinatario, Gravedad, Hallazgo, Revision, componer, eur, lista};
use crate::books::xlsx;
use crate::error::{AppError, Result};

const AVISO_LEGAL: &str = "Revisión automática hecha por TaxPain con lo que hay en la carpeta: \
es orientativa y no sustituye a un asesor. Las cifras de impuestos son estimaciones que hay que \
confirmar con la gestoría.";

fn gravedad_titulo(g: Gravedad) -> &'static str {
    match g {
        Gravedad::Error => "Errores",
        Gravedad::Aviso => "Avisos",
        Gravedad::Info => "Para saber",
    }
}

fn fecha_es(iso: &str) -> String {
    match iso.split('-').collect::<Vec<_>>()[..] {
        [y, m, d] => format!("{d}/{m}/{y}"),
        _ => iso.to_owned(),
    }
}

fn cabecera(r: &Revision) -> String {
    match &r.nif {
        Some(n) if !r.empresa.is_empty() => format!("{} ({n})", r.empresa),
        Some(n) => n.clone(),
        None => r.empresa.clone(),
    }
}

fn subtitulo(h: &Hallazgo) -> String {
    let mut s = h.responsable.titulo().to_owned();
    s.push_str(" · ");
    s.push_str(h.area.titulo());
    if let Some(y) = h.ejercicio {
        let _ = write!(s, " · {y}");
    }
    if let Some(i) = h.importe.filter(|i| !i.is_zero()) {
        let _ = write!(
            s,
            " · {} {}",
            if i.is_negative() {
                "a pagar"
            } else if super::PROXIMO_200.contains(&h.codigo.as_str()) {
                "a favor en el próximo 200"
            } else {
                "recuperable"
            },
            eur(i.abs())
        );
    }
    s
}

/// «En el próximo 200 se recuperan unos 1.900,00 €: …» y lo que se pierde sin la exención.
pub(crate) fn proximo_200(s: &super::Resumen) -> String {
    let mut out = String::new();
    if s.retenciones_proximo_200.cents() > 0 {
        let _ = write!(
            out,
            "En el próximo 200 se recuperan unos {}: {} de retenciones soportadas, que se \
             restan de la cuota, menos {} de impuesto que corresponde a esos dividendos con la \
             exención (estimación).",
            eur(s.a_favor_proximo_200),
            eur(s.retenciones_proximo_200),
            eur(s.impuesto_dividendos_proximo_200)
        );
    }
    if s.exencion_proximo_200.cents() > 0 {
        if !out.is_empty() {
            out.push(' ');
        }
        let _ = write!(
            out,
            "Si no se aplica la exención de los dividendos (casilla [00370]), se pierden unos {}: \
             compruébalo en el borrador antes de presentarlo.",
            eur(s.exencion_proximo_200)
        );
    }
    out
}

/// Título de la sección de lo urgente.
pub const URGENTE: &str = "Urgente: puede haber deuda";

fn urgentes(r: &Revision) -> Vec<&Hallazgo> {
    r.hallazgos.iter().filter(|h| h.urgente.is_some()).collect()
}

/// La entrada de la sección: cuántos y cuánto hay en juego.
fn urgente_intro(r: &Revision) -> String {
    format!(
        "{} que pueden suponer dinero pendiente con Hacienda o la Seguridad Social, con el \
         recargo estimado a {} y cuánto sube si se espera. En juego (lo que se sabe): {}. \
         TaxPain no sabe si la deuda ya está en periodo ejecutivo ni si Hacienda ya la ha \
         notificado: compruébalo en la Sede.",
        super::n_hallazgos(urgentes(r).len()),
        fecha_es(&r.generada),
        eur(r.resumen.en_juego_urgente)
    )
}

fn resumen_lineas(r: &Revision) -> Vec<String> {
    let s = &r.resumen;
    let mut out = vec![
        format!(
            "{} ficheros: {} con datos, {} sin leer y {} en formatos que no se leen.",
            s.ficheros, s.con_detalle, s.sin_leer, s.no_admitidos
        ),
        format!(
            "{} errores, {} avisos y {} notas.",
            s.errores, s.avisos, s.info
        ),
    ];
    if s.recuperable.cents() > 0 {
        out.push(format!(
            "Recuperable estimado corrigiendo los modelos: {}.",
            eur(s.recuperable)
        ));
    }
    let p = proximo_200(s);
    if !p.is_empty() {
        out.push(p);
    }
    if !r.devoluciones_pendientes.is_empty() {
        out.push(format!(
            "Devoluciones pedidas, pendientes de cobrar: {}.",
            eur(s.pendiente_cobro)
        ));
        out.extend(r.devoluciones_pendientes.iter().map(devolucion_pendiente));
    }
    out
}

/// «El 200 de 2023: 1.234,56 € (estimación), pedida por solicitud de rectificación
/// (registro …) el 12/11/2026; pendiente de resolver…».
pub(crate) fn devolucion_pendiente(d: &super::DevolucionPendiente) -> String {
    format!(
        "{}: {}{}, pedida por {}{}; {}.",
        super::cruces::primera(&super::cruces::nombre(&d.modelo, d.ejercicio, &d.periodo)),
        eur(d.importe),
        if d.estimado { " (estimación)" } else { "" },
        d.via,
        d.presentada
            .as_deref()
            .map(|f| format!(" el {}", fecha_es(f)))
            .unwrap_or_default(),
        d.estado
    )
}

/// Markdown (para leerlo o pasarlo a la gestoría).
pub fn markdown(r: &Revision) -> String {
    let mut s = String::new();
    let _ = writeln!(s, "# Revisión de la carpeta de {}\n", cabecera(r));
    let _ = writeln!(
        s,
        "Hecha el {} sobre la carpeta `{}`. {AVISO_LEGAL}\n",
        fecha_es(&r.generada),
        r.carpeta
    );
    let us = urgentes(r);
    if !us.is_empty() {
        let _ = writeln!(s, "## {URGENTE}\n\n{}\n", urgente_intro(r));
        for h in us {
            let u = h.urgente.as_ref().expect("urgente");
            let _ = writeln!(s, "### {}\n", h.titulo);
            let _ = writeln!(s, "*{}*\n", subtitulo(h));
            let _ = writeln!(s, "**{}**\n", u.resumen());
            let _ = writeln!(s, "{}\n", u.nota);
            let _ = writeln!(s, "{}\n", h.detalle);
            if !h.ficheros.is_empty() {
                let _ = writeln!(s, "Ficheros: {}\n", lista(&h.ficheros, 10));
            }
        }
    }
    s.push_str("## Resumen\n\n");
    for l in resumen_lineas(r) {
        let _ = writeln!(s, "- {l}");
    }
    s.push('\n');
    for g in [Gravedad::Error, Gravedad::Aviso, Gravedad::Info] {
        let hs: Vec<&Hallazgo> = r
            .hallazgos
            .iter()
            .filter(|h| h.gravedad == g && h.urgente.is_none())
            .collect();
        if hs.is_empty() {
            continue;
        }
        let _ = writeln!(s, "## {}\n", gravedad_titulo(g));
        for h in hs {
            let _ = writeln!(s, "### {}\n", h.titulo);
            let _ = writeln!(s, "*{}*\n", subtitulo(h));
            let _ = writeln!(s, "{}\n", h.detalle);
            if !h.ficheros.is_empty() {
                let _ = writeln!(s, "Ficheros: {}\n", lista(&h.ficheros, 10));
            }
        }
    }
    if !r.pedir.is_empty() {
        s.push_str("## Qué pedir\n\n");
        let quienes: BTreeSet<Destinatario> = r.pedir.iter().map(|p| p.a_quien).collect();
        for q in quienes {
            let _ = writeln!(s, "### {}\n", q.titulo());
            for p in r.pedir.iter().filter(|p| p.a_quien == q) {
                let _ = writeln!(s, "- {}", p.que);
            }
            s.push('\n');
        }
    }
    if !r.tablas.is_empty() {
        s.push_str("## Tablas\n\n");
        let celda = |c: &str| c.replace('|', "\\|").replace('\n', "<br>");
        for t in &r.tablas {
            let _ = writeln!(s, "### {}\n", t.titulo);
            let _ = writeln!(s, "| {} |", t.columnas.join(" | "));
            let _ = writeln!(
                s,
                "|{}|",
                t.columnas
                    .iter()
                    .map(|_| "---")
                    .collect::<Vec<_>>()
                    .join("|")
            );
            for f in &t.filas {
                let _ = writeln!(
                    s,
                    "| {} |",
                    f.iter().map(|c| celda(c)).collect::<Vec<_>>().join(" | ")
                );
            }
            s.push('\n');
        }
    }
    s
}

/// PDF (A4, texto).
pub fn pdf(r: &Revision) -> Result<Vec<u8>> {
    let mut p = vec![
        Para::Title(format!("Revisión de la carpeta de {}", cabecera(r))),
        Para::Text(format!(
            "Hecha el {} sobre la carpeta {}.",
            fecha_es(&r.generada),
            r.carpeta
        )),
        Para::Text(AVISO_LEGAL.into()),
    ];
    let us = urgentes(r);
    if !us.is_empty() {
        p.push(Para::Gap);
        p.push(Para::Title(URGENTE.into()));
        p.push(Para::Text(urgente_intro(r)));
        for h in us {
            let u = h.urgente.as_ref().expect("urgente");
            p.push(Para::Gap);
            p.push(Para::Bold(h.titulo.clone()));
            p.push(Para::Text(subtitulo(h)));
            p.push(Para::Bold(u.resumen()));
            p.push(Para::Text(u.nota.clone()));
            p.push(Para::Text(h.detalle.clone()));
            if !h.ficheros.is_empty() {
                p.push(Para::Text(format!("Ficheros: {}", lista(&h.ficheros, 6))));
            }
        }
    }
    p.push(Para::Gap);
    p.push(Para::Bold("Resumen".into()));
    p.extend(resumen_lineas(r).into_iter().map(Para::Text));
    for g in [Gravedad::Error, Gravedad::Aviso, Gravedad::Info] {
        let hs: Vec<&Hallazgo> = r
            .hallazgos
            .iter()
            .filter(|h| h.gravedad == g && h.urgente.is_none())
            .collect();
        if hs.is_empty() {
            continue;
        }
        p.push(Para::Gap);
        p.push(Para::Title(gravedad_titulo(g).into()));
        for h in hs {
            p.push(Para::Gap);
            p.push(Para::Bold(h.titulo.clone()));
            p.push(Para::Text(subtitulo(h)));
            p.push(Para::Text(h.detalle.clone()));
            if !h.ficheros.is_empty() {
                p.push(Para::Text(format!("Ficheros: {}", lista(&h.ficheros, 6))));
            }
        }
    }
    if !r.pedir.is_empty() {
        p.push(Para::Gap);
        p.push(Para::Title("Qué pedir".into()));
        for q in r.pedir.iter().map(|x| x.a_quien).collect::<BTreeSet<_>>() {
            p.push(Para::Bold(q.titulo().into()));
            for x in r.pedir.iter().filter(|x| x.a_quien == q) {
                p.push(Para::Text(format!("- {}", x.que)));
            }
        }
    }
    let mut docs = vec![p];
    if !r.tablas.is_empty() {
        let mut t = vec![Para::Title("Tablas".into())];
        for tabla in &r.tablas {
            t.push(Para::Gap);
            t.push(Para::Bold(tabla.titulo.clone()));
            t.push(Para::Text(tabla.columnas.join(" · ")));
            for f in &tabla.filas {
                t.push(Para::Text(
                    f.iter()
                        .map(|c| c.replace('\n', ", "))
                        .collect::<Vec<_>>()
                        .join(" · "),
                ));
            }
        }
        docs.push(t);
    }
    // Rutas y textos que lleguen descompuestos (NFD) del Mac: la fuente del PDF no los dibuja.
    let docs: Vec<Vec<Para>> = docs
        .into_iter()
        .map(|d| d.into_iter().map(compuesto).collect())
        .collect();
    text_document("Revisión de la carpeta", &docs).map_err(AppError::Other)
}

fn compuesto(p: Para) -> Para {
    match p {
        Para::Title(t) => Para::Title(componer(&t)),
        Para::Text(t) => Para::Text(componer(&t)),
        Para::Bold(t) => Para::Bold(componer(&t)),
        Para::Row(a, b) => Para::Row(componer(&a), componer(&b)),
        Para::Gap => Para::Gap,
    }
}

/// Nombre de hoja de Excel válido y único (31 caracteres, sin `[]:*?/\`).
fn hoja(titulo: &str, usados: &mut BTreeSet<String>) -> String {
    let limpio: String = titulo
        .chars()
        .filter(|c| !"[]:*?/\\".contains(*c))
        .collect();
    let base: String = limpio.chars().take(28).collect();
    let mut nombre = base.clone();
    let mut n = 2;
    while !usados.insert(nombre.to_lowercase()) {
        nombre = format!("{base} {n}");
        n += 1;
    }
    nombre
}

/// Excel: el resumen, los hallazgos y una hoja por tabla.
pub fn excel(r: &Revision) -> Result<Vec<u8>> {
    let bold = Format::new().set_bold();
    let mut wb = Workbook::new();
    let mut usados = BTreeSet::new();
    let us = urgentes(r);
    if !us.is_empty() {
        let ws = wb.add_worksheet();
        ws.set_name(hoja("Urgente", &mut usados)).map_err(xlsx)?;
        crate::celda_segura::texto_con_formato(ws, 0, 0, URGENTE, &bold).map_err(xlsx)?;
        crate::celda_segura::texto(ws, 1, 0, urgente_intro(r)).map_err(xlsx)?;
        let cols = [
            ("Qué", 50),
            ("Ejercicio", 10),
            ("En juego", 14),
            ("Venció", 12),
            ("Recargo hoy", 14),
            ("Siguiente escalón", 40),
            ("Recargo", 80),
            ("Detalle", 100),
        ];
        for (i, (c, w)) in cols.iter().enumerate() {
            let col = u16::try_from(i).unwrap_or(0);
            crate::celda_segura::texto_con_formato(ws, 3, col, *c, &bold).map_err(xlsx)?;
            ws.set_column_width(col, *w).map_err(xlsx)?;
        }
        for (i, h) in us.iter().enumerate() {
            let u = h.urgente.as_ref().expect("urgente");
            let row = u32::try_from(i + 4).unwrap_or(u32::MAX);
            let siguiente = match (u.siguiente_porcentaje, &u.siguiente_desde) {
                (Some(p), Some(d)) => format!(
                    "{p} % desde el {}{}",
                    fecha_es(d),
                    u.siguiente_recargo
                        .map(|x| format!(": {}", eur(x)))
                        .unwrap_or_default()
                ),
                (Some(p), None) => format!(
                    "{p} %{}",
                    u.siguiente_recargo
                        .map(|x| format!(": {}", eur(x)))
                        .unwrap_or_default()
                ),
                _ => String::new(),
            };
            let valores = [
                h.titulo.clone(),
                h.ejercicio.map(|y| y.to_string()).unwrap_or_default(),
                u.en_juego.map(eur).unwrap_or_default(),
                u.vencio.as_deref().map(fecha_es).unwrap_or_default(),
                match (u.porcentaje_hoy, u.recargo_hoy) {
                    (Some(p), Some(x)) => format!("{p} %: {}", eur(x)),
                    (Some(p), None) => format!("{p} %"),
                    (None, Some(x)) => eur(x),
                    _ => String::new(),
                },
                siguiente,
                u.nota.clone(),
                h.detalle.clone(),
            ];
            for (j, v) in valores.iter().enumerate() {
                crate::celda_segura::texto(ws, row, u16::try_from(j).unwrap_or(0), v)
                    .map_err(xlsx)?;
            }
        }
    }
    {
        let ws = wb.add_worksheet();
        ws.set_name(hoja("Resumen", &mut usados)).map_err(xlsx)?;
        crate::celda_segura::texto_con_formato(
            ws,
            0,
            0,
            format!("Revisión de la carpeta de {}", cabecera(r)),
            &bold,
        )
        .map_err(xlsx)?;
        crate::celda_segura::texto(ws, 1, 0, format!("Hecha el {}", fecha_es(&r.generada)))
            .map_err(xlsx)?;
        crate::celda_segura::texto(ws, 2, 0, &r.carpeta).map_err(xlsx)?;
        let mut fila = 4;
        for l in resumen_lineas(r) {
            crate::celda_segura::texto(ws, fila, 0, l).map_err(xlsx)?;
            fila += 1;
        }
        fila += 1;
        crate::celda_segura::texto(ws, fila, 0, AVISO_LEGAL).map_err(xlsx)?;
        ws.set_column_width(0, 100).map_err(xlsx)?;
    }
    {
        let ws = wb.add_worksheet();
        ws.set_name(hoja("Hallazgos", &mut usados)).map_err(xlsx)?;
        let cols = [
            ("Gravedad", 10),
            ("Urgente", 10),
            ("Área", 22),
            ("Responsable", 14),
            ("Ejercicio", 10),
            ("Qué", 50),
            ("Detalle", 100),
            ("Importe", 14),
            ("Ficheros", 80),
        ];
        for (i, (c, w)) in cols.iter().enumerate() {
            let col = u16::try_from(i).unwrap_or(0);
            crate::celda_segura::texto_con_formato(ws, 0, col, *c, &bold).map_err(xlsx)?;
            ws.set_column_width(col, *w).map_err(xlsx)?;
        }
        for (i, h) in r.hallazgos.iter().enumerate() {
            let row = u32::try_from(i + 1).unwrap_or(u32::MAX);
            let valores = [
                gravedad_titulo(h.gravedad).to_owned(),
                if h.urgente.is_some() { "Sí" } else { "" }.to_owned(),
                h.area.titulo().to_owned(),
                h.responsable.titulo().to_owned(),
                h.ejercicio.map(|y| y.to_string()).unwrap_or_default(),
                h.titulo.clone(),
                h.detalle.clone(),
                h.importe.map(eur).unwrap_or_default(),
                h.ficheros.join("\n"),
            ];
            for (j, v) in valores.iter().enumerate() {
                crate::celda_segura::texto(ws, row, u16::try_from(j).unwrap_or(0), v)
                    .map_err(xlsx)?;
            }
        }
    }
    if !r.pedir.is_empty() {
        let ws = wb.add_worksheet();
        ws.set_name(hoja("Qué pedir", &mut usados)).map_err(xlsx)?;
        crate::celda_segura::texto_con_formato(ws, 0, 0, "A quién", &bold).map_err(xlsx)?;
        crate::celda_segura::texto_con_formato(ws, 0, 1, "Qué", &bold).map_err(xlsx)?;
        ws.set_column_width(0, 24).map_err(xlsx)?;
        ws.set_column_width(1, 120).map_err(xlsx)?;
        for (i, p) in r.pedir.iter().enumerate() {
            let row = u32::try_from(i + 1).unwrap_or(u32::MAX);
            crate::celda_segura::texto(ws, row, 0, p.a_quien.titulo()).map_err(xlsx)?;
            crate::celda_segura::texto(ws, row, 1, &p.que).map_err(xlsx)?;
        }
    }
    for t in &r.tablas {
        let ws = wb.add_worksheet();
        let nombre = if t.hoja.is_empty() {
            &t.titulo
        } else {
            &t.hoja
        };
        ws.set_name(hoja(nombre, &mut usados)).map_err(xlsx)?;
        for (j, c) in t.columnas.iter().enumerate() {
            let col = u16::try_from(j).unwrap_or(0);
            crate::celda_segura::texto_con_formato(ws, 0, col, c, &bold).map_err(xlsx)?;
            ws.set_column_width(col, 22).map_err(xlsx)?;
        }
        for (i, f) in t.filas.iter().enumerate() {
            let row = u32::try_from(i + 1).unwrap_or(u32::MAX);
            for (j, v) in f.iter().enumerate() {
                crate::celda_segura::texto(ws, row, u16::try_from(j).unwrap_or(0), v)
                    .map_err(xlsx)?;
            }
        }
    }
    wb.save_to_buffer().map_err(xlsx)
}

/// Nombre de fichero propuesto para guardar el informe.
pub fn nombre_fichero(r: &Revision, extension: &str) -> String {
    format!("revision-{}.{extension}", r.generada)
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use taxpain_core::money::Money;

    use super::super::{Area, Peticion, Responsable, Resumen, Tabla};
    use super::*;

    /// Textos de datos que una hoja de cálculo tomaría por fórmula.
    const PELIGROSOS: [&str; 4] = ["=SUMA(1;2)", "+34 600 000 000", "-2+3", "@SUMA(A1)"];

    fn revision() -> Revision {
        let hallazgo = |titulo: &str, detalle: &str| Hallazgo {
            codigo: "prueba.formula".into(),
            area: Area::Libros,
            gravedad: Gravedad::Aviso,
            responsable: Responsable::Documentacion,
            ejercicio: Some(2025),
            titulo: titulo.into(),
            detalle: detalle.into(),
            ficheros: vec!["2025/Libros/Libro de prueba.xlsx".into()],
            importe: Some(Money::from_cents(-1_050)),
            rectificable_hasta: None,
            rectificable_aprox: false,
            terceros: None,
            urgente: None,
        };
        Revision {
            generada: "2026-10-30".into(),
            empresa: "Cliente Uno SL".into(),
            nif: Some("B12345674".into()),
            carpeta: PELIGROSOS[3].into(),
            resumen: Resumen {
                ficheros: 1,
                con_detalle: 1,
                sin_leer: 0,
                no_admitidos: 0,
                por_tipo: vec![],
                errores: 0,
                avisos: 2,
                info: 0,
                urgentes: 0,
                en_juego_urgente: Money::ZERO,
                recuperable: Money::ZERO,
                a_favor_proximo_200: Money::ZERO,
                retenciones_proximo_200: Money::ZERO,
                exencion_proximo_200: Money::ZERO,
                impuesto_dividendos_proximo_200: Money::ZERO,
                pendiente_cobro: Money::ZERO,
            },
            hallazgos: vec![
                hallazgo(PELIGROSOS[0], "Detalle normal"),
                hallazgo("Título normal", PELIGROSOS[1]),
            ],
            tablas: vec![Tabla {
                titulo: "Tabla de prueba".into(),
                hoja: String::new(),
                columnas: vec!["Concepto".into()],
                filas: vec![vec![PELIGROSOS[2].into()]],
            }],
            pedir: vec![Peticion {
                a_quien: Destinatario::Gestoria,
                que: PELIGROSOS[3].into(),
            }],
            devoluciones_pendientes: vec![],
            dividendos: vec![],
        }
    }

    fn con_urgente() -> Revision {
        let mut r = revision();
        let mut u = r.hallazgos[1].clone();
        u.titulo = "No se ve el pago del 111 del 2T 2026 en el banco".into();
        u.urgente = Some(crate::revision::Urgente::sin_ingreso(
            time::macros::date!(2026 - 07 - 20),
            Money::from_euros(1_000, 0),
            time::macros::date!(2026 - 11 - 16),
        ));
        r.hallazgos.push(u);
        crate::revision::urgente::ordenar(&mut r.hallazgos);
        r
    }

    #[test]
    fn la_seccion_urgente_va_antes_del_resumen() {
        let r = con_urgente();
        assert!(r.hallazgos[0].urgente.is_some(), "los urgentes, primero");
        let md = markdown(&r);
        let u = md.find("## Urgente: puede haber deuda").expect("sección");
        assert!(u < md.find("## Resumen").unwrap());
        assert!(md.contains("recargo hoy 5 %: 50,00 €"));
        // Y no se repite en los avisos.
        assert_eq!(md.matches("### No se ve el pago del 111").count(), 1);
        assert!(!markdown(&revision()).contains("Urgente"));
        let x = excel(&r).unwrap();
        let hojas = partes(&x)
            .into_iter()
            .find(|(n, _)| n == "xl/workbook.xml")
            .unwrap()
            .1;
        let i = hojas.find("name=\"Urgente\"").expect("hoja Urgente");
        assert!(i < hojas.find("name=\"Resumen\"").unwrap());
        assert!(!pdf(&r).unwrap().is_empty());
    }

    fn partes(xlsx: &[u8]) -> Vec<(String, String)> {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(xlsx)).unwrap();
        (0..z.len())
            .map(|i| {
                let mut f = z.by_index(i).unwrap();
                let mut s = String::new();
                f.read_to_string(&mut s).unwrap();
                (f.name().unwrap().into_owned(), s)
            })
            .collect()
    }

    #[test]
    fn el_excel_del_informe_escribe_como_texto_lo_que_parece_una_formula() {
        let bytes = excel(&revision()).unwrap();
        let partes = partes(&bytes);
        let parte = |n: &str| &partes.iter().find(|(k, _)| k == n).unwrap().1;
        // Textos compartidos, en orden.
        let si = regex::Regex::new(r"<si><t[^>]*>([^<]*)</t></si>").unwrap();
        let textos: Vec<&str> = si
            .captures_iter(parte("xl/sharedStrings.xml"))
            .map(|c| c.get(1).unwrap().as_str())
            .collect();
        // Formatos de celda, en orden.
        let estilos = parte("xl/styles.xml");
        let xfs = &estilos[estilos.find("<cellXfs").unwrap()..estilos.find("</cellXfs>").unwrap()];
        let xf: Vec<&str> = xfs.split("<xf ").skip(1).collect();
        let celda =
            regex::Regex::new(r#"<c r="[A-Z]+\d+"(?: s="(\d+)")? t="s"><v>(\d+)</v></c>"#).unwrap();
        let mut vistos = BTreeSet::new();
        for (nombre, hoja) in partes
            .iter()
            .filter(|(k, _)| k.starts_with("xl/worksheets/sheet"))
        {
            assert!(!hoja.contains("<f>"), "{nombre}: {hoja}");
            for c in celda.captures_iter(hoja) {
                let texto = textos[c[2].parse::<usize>().unwrap()];
                if PELIGROSOS.contains(&texto) {
                    let s: usize = c.get(1).map_or(0, |s| s.as_str().parse().unwrap());
                    assert!(xf[s].contains(r#"quotePrefix="1""#), "{texto}: {}", xf[s]);
                    vistos.insert(texto);
                }
            }
        }
        assert_eq!(vistos.len(), PELIGROSOS.len(), "{vistos:?}");
    }
}
