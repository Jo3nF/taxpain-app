//! Documentación específica de operaciones vinculadas con contenido simplificado (art. 18.3 LIS;
//! art. 16.4 RIS), en RTF (para editarla en Pages o Word) o PDF, con el generador de la memoria.
//!
//! El art. 16.4 RIS pide, para las entidades con cifra de negocios inferior a 45 millones: a) la
//! descripción de la naturaleza, características e importe de las operaciones; b) el nombre o
//! razón social y el NIF de las vinculadas; c) el método de valoración; d) los comparables
//! obtenidos y el valor o intervalo de valores que resultan del método. Lo que no sale de
//! TaxPain (descripción, comparables, justificación) lo escribe el usuario; si falta, el
//! documento lo deja entre «[…]».
//!
//! No se aplica el contenido simplificado a las operaciones específicas (art. 18.3 LIS,
//! cuarto párrafo): se avisa. La documentación no se presenta: se tiene a disposición de la
//! Administración desde el plazo de la declaración del IS (art. 13.1 RIS).
//! TODO(VERIFY): si las operaciones con la misma persona de hasta 250.000 € quedan fuera de la
//! obligación de documentar (art. 18.3 LIS, último párrafo, según su redacción vigente).

use taxpain_invoicing::pdf::{self, Para};
use taxpain_vault::Vault;

use super::{Registro, pct_texto, registro};
use crate::error::{AppError, Result};

fn fecha(d: time::Date) -> String {
    format!("{:02}/{:02}/{}", d.day(), u8::from(d.month()), d.year())
}

fn o_corchete(s: &str, falta: &str) -> String {
    if s.trim().is_empty() {
        format!("[{falta}]")
    } else {
        s.trim().to_owned()
    }
}

/// Párrafos de la documentación del ejercicio.
pub fn paragraphs(v: &Vault, year: i32) -> Result<Vec<Para>> {
    let r = registro(v, year)?;
    let ms = crate::models::settings(v)?;
    Ok(paragraphs_of(&r, &ms.activity_description))
}

pub(crate) fn paragraphs_of(r: &Registro, actividad_ajustes: &str) -> Vec<Para> {
    let mut p = vec![
        Para::Title(format!(
            "Documentación de operaciones vinculadas · ejercicio {}",
            r.year
        )),
        Para::Text(format!(
            "{} · NIF {} · Ejercicio del {} al {}",
            r.company,
            if r.company_nif.is_empty() {
                "[NIF]"
            } else {
                &r.company_nif
            },
            fecha(r.start),
            fecha(r.end)
        )),
        Para::Text(
            "Contenido simplificado de la documentación específica (art. 18.3 de la Ley 27/2014 \
             del Impuesto sobre Sociedades y art. 16.4 de su Reglamento, RD 634/2015). BORRADOR \
             preparado con TaxPain: revísalo y completa lo que va entre corchetes."
                .into(),
        ),
        Para::Gap,
    ];
    if !r.simplificada {
        p.push(Para::Text(
            "AVISO: la cifra de negocios del ejercicio llega a 45 millones de euros: no se puede \
             usar el contenido simplificado (art. 16 RIS completo)."
                .into(),
        ));
    }
    if r.operaciones
        .iter()
        .any(|o| o.especifica && o.excluida.is_none())
    {
        p.push(Para::Text(
            "AVISO: hay operaciones específicas (transmisión de negocios, de participaciones no \
             cotizadas o de inmuebles, u operaciones sobre intangibles): para ellas no vale el \
             contenido simplificado (art. 18.3 LIS)."
                .into(),
        ));
    }

    // 1. La entidad.
    p.push(Para::Bold("1. Datos de la entidad".into()));
    let actividad = if r.textos.actividad.trim().is_empty() {
        actividad_ajustes.trim()
    } else {
        r.textos.actividad.trim()
    };
    p.push(Para::Text(format!(
        "{} (NIF {}). Actividad: {}.",
        r.company,
        r.company_nif,
        o_corchete(
            actividad,
            "describe la actividad y, si forma grupo, su estructura"
        )
    )));
    match r.incn {
        Some(m) => p.push(Para::Row(
            format!(
                "Importe neto de la cifra de negocios (según {})",
                r.incn_fuente
            ),
            m.to_string(),
        )),
        None => p.push(Para::Text(
            "Importe neto de la cifra de negocios: [importe].".into(),
        )),
    }
    p.push(Para::Gap);

    // 2. Las vinculadas.
    p.push(Para::Bold(
        "2. Personas o entidades vinculadas (art. 16.4.b RIS)".into(),
    ));
    let con_ops: Vec<_> = r
        .personas
        .iter()
        .filter(|x| r.operaciones.iter().any(|o| o.nif == x.nif))
        .collect();
    if con_ops.is_empty() {
        p.push(Para::Text(
            "No constan operaciones con personas o entidades vinculadas en el ejercicio.".into(),
        ));
    }
    for x in &con_ops {
        let vinculos = x
            .vinculos
            .iter()
            .map(|v| format!("{} (art. 18.2.{} LIS)", v.label(), v.letra()))
            .collect::<Vec<_>>()
            .join("; ");
        let pct = x
            .pct
            .map(|pct| format!(", participación del {}", pct_texto(pct)))
            .unwrap_or_default();
        let detalle = if x.detalle.is_empty() {
            String::new()
        } else {
            format!(" ({})", x.detalle)
        };
        p.push(Para::Text(format!(
            "{} · NIF {} · Persona {} residente en {}: {vinculos}{pct}{detalle}.",
            x.name,
            x.nif,
            x.condicion().to_lowercase(),
            if x.country == "ES" {
                "España".to_owned()
            } else {
                x.country.clone()
            }
        )));
    }
    p.push(Para::Gap);

    // 3. Las operaciones.
    p.push(Para::Bold(
        "3. Operaciones, método de valoración y comparables (art. 16.4.a, c y d RIS)".into(),
    ));
    let mut n = 0;
    for o in &r.operaciones {
        if o.excluida.is_some() {
            continue;
        }
        n += 1;
        p.push(Para::Text(format!(
            "3.{n}. {} con {} ({}; clave {} del modelo 232)",
            o.concepto.label(),
            o.name,
            if o.ingreso { "ingreso" } else { "pago" },
            o.clave.map_or_else(|| "-".to_owned(), |c| c.to_string())
        )));
        p.push(Para::Row(
            "  Importe del ejercicio (sin IVA)".into(),
            o.importe.to_string(),
        ));
        p.push(Para::Text(format!(
            "  Naturaleza y características: {}",
            o_corchete(
                &o.descripcion,
                "describe qué se presta o se entrega, condiciones, plazos y riesgos"
            )
        )));
        p.push(Para::Text(format!(
            "  Método de valoración: {}{}",
            o.metodo.label(),
            if o.metodo_elegido {
                ""
            } else {
                " [confírmalo]"
            }
        )));
        p.push(Para::Text(format!(
            "  Comparables y valor o intervalo de mercado: {}",
            o_corchete(
                &o.justificacion,
                "indica los comparables (precios con terceros, tarifas, estudios) y el valor o \
                 intervalo que resulta"
            )
        )));
        p.push(Para::Text(format!(
            "  Origen de los datos en TaxPain: {}.",
            o.origen.join(", ")
        )));
    }
    if n == 0 {
        p.push(Para::Text(
            "No hay operaciones sujetas a documentación en el ejercicio.".into(),
        ));
    }
    let excluidas: Vec<_> = r
        .operaciones
        .iter()
        .filter(|o| o.excluida.is_some())
        .collect();
    if !excluidas.is_empty() {
        p.push(Para::Text(
            "Operaciones con vinculadas fuera del régimen de documentación:".into(),
        ));
        for o in excluidas {
            p.push(Para::Row(
                format!(
                    "  {} · {} ({})",
                    o.name,
                    o.concepto.label(),
                    o.excluida.as_deref().unwrap_or_default()
                ),
                o.importe.to_string(),
            ));
        }
    }
    p.push(Para::Gap);

    // 4. Justificación general.
    p.push(Para::Bold("4. Justificación del valor de mercado".into()));
    p.push(Para::Text(o_corchete(
        &r.textos.justificacion,
        "explica por qué las condiciones pactadas son las que se habrían pactado entre partes \
         independientes",
    )));
    p.push(Para::Gap);

    // 5. Modelo 232.
    p.push(Para::Bold("5. Declaración informativa (modelo 232)".into()));
    if r.obligado {
        p.push(Para::Text(format!(
            "Hay que presentar el modelo 232 en {}, porque: {}.",
            r.plazo,
            r.motivos.join("; ")
        )));
    } else {
        p.push(Para::Text(
            "Con los datos de TaxPain, las operaciones del ejercicio no llegan a ninguno de los \
             umbrales del modelo 232 (más de 250.000 € con la misma persona o entidad, más de \
             100.000 € en operaciones específicas de un mismo tipo, o más del 50 % de la cifra \
             de negocios en operaciones del mismo tipo y método)."
                .into(),
        ));
    }
    p.push(Para::Gap);
    p.push(Para::Text(format!(
        "En [lugar], a [fecha]. Firmado: [administrador o administradores de {}].",
        r.company
    )));
    p
}

/// PDF de la documentación.
pub fn pdf(v: &Vault, year: i32) -> Result<Vec<u8>> {
    let p = paragraphs(v, year)?;
    pdf::text_document(&format!("Operaciones vinculadas {year}"), &[p]).map_err(AppError::Other)
}

/// RTF de la documentación (Pages, Word, TextEdit).
pub fn rtf(v: &Vault, year: i32) -> Result<String> {
    Ok(crate::memoria::to_rtf(&paragraphs(v, year)?))
}
