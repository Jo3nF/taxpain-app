//! Borrador del modelo 232 en una hoja de cálculo: un registro por vinculada, tipo de operación,
//! ingreso o pago y método de valoración, con los campos de la Orden HFP/816/2017 (art. 3.1:
//! NIF, condición, nombre, tipo de vinculación, provincia o país, clave del tipo de operación,
//! ingreso o pago, método de valoración e importe sin IVA).
//!
//! El 232 se presenta por Internet (art. 5 de la orden; formulario de la Sede). TODO(VERIFY):
//! la orden no publica un diseño de registro para importar un fichero, así que no se genera
//! ninguno: la hoja sirve para teclear el formulario y como soporte del cálculo.

use rust_xlsxwriter::{Format, Workbook};
use taxpain_vault::Vault;

use super::{Registro, registro};
use crate::celda_segura::{texto, texto_con_formato};
use crate::error::{AppError, Result};

fn xlsx(e: rust_xlsxwriter::XlsxError) -> AppError {
    AppError::Other(e.to_string())
}

/// Libro XLSX del borrador. Devuelve (nombre del fichero, bytes).
pub fn workbook(v: &Vault, year: i32) -> Result<(String, Vec<u8>)> {
    let r = registro(v, year)?;
    Ok((format!("Modelo 232 {year}.xlsx"), workbook_of(&r)?))
}

const CABECERA: [&str; 13] = [
    "¿Va en el 232?",
    "NIF",
    "Condición",
    "Apellidos y nombre o razón social",
    "Tipo de vinculación (art. 18.2 LIS)",
    "Provincia o país",
    "Clave del tipo de operación",
    "Tipo de operación",
    "Ingreso o pago",
    "Método de valoración",
    "Importe sin IVA",
    "Motivo",
    "Origen en TaxPain",
];

pub(crate) fn workbook_of(r: &Registro) -> Result<Vec<u8>> {
    let bold = Format::new().set_bold();
    let money = Format::new().set_num_format("#,##0.00");
    let mut wb = Workbook::new();
    {
        let ws = wb.add_worksheet();
        ws.set_name("232").map_err(xlsx)?;
        texto_con_formato(
            ws,
            0,
            0,
            format!(
                "Modelo 232 · operaciones con personas o entidades vinculadas · ejercicio {} · {} ({})",
                r.year, r.company, r.company_nif
            ),
            &bold,
        )
        .map_err(xlsx)?;
        texto(
            ws,
            1,
            0,
            if r.obligado {
                format!("Hay que presentarlo en {}.", r.plazo)
            } else {
                "Con los datos de TaxPain no se llega a los umbrales: no hay que presentarlo."
                    .to_owned()
            },
        )
        .map_err(xlsx)?;
        for (c, h) in CABECERA.iter().enumerate() {
            let c = u16::try_from(c).unwrap_or(0);
            texto_con_formato(ws, 3, c, *h, &bold).map_err(xlsx)?;
            ws.set_column_width(
                c,
                if c == 3 || c == 7 || c == 12 {
                    36.0
                } else {
                    16.0
                },
            )
            .map_err(xlsx)?;
        }
        for (i, o) in r.operaciones.iter().enumerate() {
            let row = 4 + u32::try_from(i).unwrap_or(u32::MAX - 4);
            let persona = r.personas.iter().find(|p| p.nif == o.nif);
            let cells: [String; 10] = [
                if o.en_232 { "Sí" } else { "No" }.to_owned(),
                o.nif.clone(),
                persona.map_or("", |p| p.condicion()).to_owned(),
                o.name.clone(),
                format!("{} ({})", o.vinculo.letra(), o.vinculo.label()),
                persona.map(super::Persona::residencia).unwrap_or_default(),
                o.clave.map(|c| c.to_string()).unwrap_or_default(),
                o.concepto.label().to_owned(),
                if o.ingreso { "Ingreso" } else { "Pago" }.to_owned(),
                o.metodo.label().to_owned(),
            ];
            for (c, s) in cells.iter().enumerate() {
                texto(ws, row, u16::try_from(c).unwrap_or(0), s).map_err(xlsx)?;
            }
            // Sólo para escribir la celda numérica: los céntimos de una operación caben en un f64.
            #[allow(clippy::float_arithmetic, clippy::cast_precision_loss)]
            let importe = o.importe.cents() as f64 / 100.0;
            ws.write_number_with_format(row, 10, importe, &money)
                .map_err(xlsx)?;
            texto(
                ws,
                row,
                11,
                o.excluida
                    .clone()
                    .or_else(|| o.motivo.clone())
                    .unwrap_or_else(|| "No llega a los umbrales".into()),
            )
            .map_err(xlsx)?;
            texto(ws, row, 12, o.origen.join(", ")).map_err(xlsx)?;
        }
        ws.set_freeze_panes(4, 0).map_err(xlsx)?;
    }
    {
        let ws = wb.add_worksheet();
        ws.set_name("Notas").map_err(xlsx)?;
        ws.set_column_width(0, 120.0).map_err(xlsx)?;
        let mut lines = vec![
            "Borrador preparado con TaxPain. No se presenta nada: el 232 se teclea en el \
             formulario de la Sede de la AEAT (Orden HFP/816/2017)."
                .to_owned(),
            "Ingresos y pagos van en registros separados, sin compensar; las operaciones con \
             distinto método de valoración, también (art. 3.1)."
                .to_owned(),
            "Tipo de vinculación: letra del art. 18.2 LIS. Comprueba en el formulario la clave \
             que corresponde."
                .to_owned(),
        ];
        if let Some(m) = r.incn {
            lines.push(format!(
                "Cifra de negocios del ejercicio: {m} (según {}).",
                r.incn_fuente
            ));
        }
        for m in &r.motivos {
            lines.push(format!("Umbral superado: {m}."));
        }
        lines.extend(r.avisos.iter().cloned());
        for (i, l) in lines.iter().enumerate() {
            texto(ws, u32::try_from(i).unwrap_or(0), 0, l).map_err(xlsx)?;
        }
    }
    wb.save_to_buffer().map_err(xlsx)
}
