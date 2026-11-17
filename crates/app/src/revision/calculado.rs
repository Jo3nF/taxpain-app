//! Lo presentado por la gestoría frente a lo que calcula TaxPain con sus datos: cada modelo
//! guardado en «Importar → Comparar» (303, 390, 111, 115, 123, 130, 202, 349, 200 y las
//! informativas 347, 190, 180 y 193) casilla a casilla y, en las informativas con fichero,
//! perceptor a perceptor ([`crate::presentados::compare`]).
//!
//! Sólo cuando TaxPain tiene datos de ese periodo (alguna casilla o perceptor con importe): sin
//! ellos todo saldría distinto y no diría nada. Como el resto de la revisión, es orientativo: una
//! diferencia puede ser de la gestoría o de que a TaxPain le falten datos.

use taxpain_core::money::Money;

use super::expediente::Expediente;
use super::{Area, Gravedad, Nuevo, Responsable, Salida, eur};
use crate::presentados::Comparison;

/// Cuántas casillas o perceptores se detallan en el hallazgo (el resto, en la tabla).
const DETALLE: usize = 6;

/// ¿Calcula TaxPain algo de este modelo y periodo?
fn con_datos(c: &Comparison) -> bool {
    c.rows.iter().any(|r| !r.ours.is_zero()) || c.lines.iter().any(|l| l.ours.is_some())
}

fn importe(m: Option<Money>) -> String {
    m.map(eur).unwrap_or_else(|| "—".into())
}

pub(crate) fn revisar(exp: &Expediente, s: &mut Salida) {
    let mut filas = Vec::new();
    for c in &exp.taxpain.comparaciones {
        if c.mismatches == 0 || !con_datos(c) {
            continue;
        }
        let f = &c.filed;
        let periodo = if f.period == "0A" {
            f.year.to_string()
        } else {
            format!("{} {}", f.period, f.year)
        };
        let malas: Vec<_> = c.rows.iter().filter(|r| r.explanation.is_some()).collect();
        let lineas: Vec<_> = c.lines.iter().filter(|l| l.explanation.is_some()).collect();
        let mut partes: Vec<String> = malas
            .iter()
            .take(DETALLE)
            .map(|r| {
                format!(
                    "[{:02}]{}: presentado {}, TaxPain {}. {}",
                    r.box_,
                    if r.label.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", r.label)
                    },
                    importe(r.theirs),
                    eur(r.ours),
                    r.explanation.as_deref().unwrap_or_default()
                )
            })
            .collect();
        partes.extend(lineas.iter().take(DETALLE).map(|l| {
            format!(
                "{} ({}{}): {}",
                if l.name.is_empty() { &l.nif } else { &l.name },
                l.nif,
                if l.key.is_empty() {
                    String::new()
                } else {
                    format!(", clave {}", l.key)
                },
                l.explanation.as_deref().unwrap_or_default()
            )
        }));
        let resto = (malas.len() + lineas.len()).saturating_sub(partes.len());
        let mut detalle = format!(
            "TaxPain ha calculado el modelo {} de {periodo} con sus datos y no coincide con el \
             que presentó la gestoría ({}): {}",
            f.model,
            f.filename,
            partes.join(" ")
        );
        if resto > 0 {
            detalle.push_str(&format!(
                " Y {resto} más en la tabla «Lo presentado frente a TaxPain»."
            ));
        }
        detalle.push_str(
            " Puede ser un error de la gestoría o que a TaxPain le falten datos de ese periodo \
             (facturas, nóminas, asientos): compruébalo en Importar → Comparar antes de \
             rectificar.",
        );
        s.add(
            Nuevo {
                codigo: "modelos.distinto_de_taxpain",
                area: Area::Modelos,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Gestoria,
                titulo: format!(
                    "El {} de {periodo} presentado no coincide con lo que calcula TaxPain",
                    f.model
                ),
                ficheros: vec![f.filename.clone()],
            }
            .detalle(detalle),
            Some(f.year),
            None,
        );
        for r in &malas {
            filas.push(vec![
                f.model.clone(),
                periodo.clone(),
                format!("[{:02}] {}", r.box_, r.label),
                importe(r.theirs),
                eur(r.ours),
                eur(r.diff),
                r.explanation.clone().unwrap_or_default(),
            ]);
        }
        for l in &lineas {
            filas.push(vec![
                f.model.clone(),
                periodo.clone(),
                format!("{} {} {}", l.nif, l.name, l.key).trim().to_owned(),
                importe(l.theirs),
                importe(l.ours),
                eur(l.ours.unwrap_or(Money::ZERO) - l.theirs.unwrap_or(Money::ZERO)),
                l.explanation.clone().unwrap_or_default(),
            ]);
        }
    }
    s.tabla(
        "Lo presentado frente a TaxPain",
        "Presentado y TaxPain",
        &[
            "Modelo",
            "Periodo",
            "Casilla o perceptor",
            "Presentado",
            "TaxPain",
            "Diferencia",
            "De dónde puede venir",
        ],
        filas,
    );
}
