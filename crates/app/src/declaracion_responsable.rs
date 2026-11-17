//! Declaración responsable del sistema informático de facturación (RD 1007/2023 art. 13; Orden
//! HAC/1177/2024 art. 15; docs/VERIFACTU.md §11).
//!
//! La firma el productor ([`taxpain_core::productor`]) y tiene que constar «de modo visible en el
//! propio sistema informático en cada una de sus versiones» (art. 13.2): la app la enseña en
//! Ajustes › Acerca de TaxPain y la guarda en PDF para firmarla y entregarla. Los datos salen de
//! los mismos sitios que el bloque `SistemaInformatico` de cada registro, para que no se
//! contradigan. Lo que falta (la dirección del productor, la fecha y el lugar de la firma) sale
//! entre corchetes.

use serde::Serialize;
use taxpain_invoicing::pdf::{self, Para};
use taxpain_invoicing::verifactu::SistemaInformatico;

use crate::error::{AppError, Result};

/// Título literal (Orden HAC/1177/2024 art. 15.1).
pub const TITULO: &str = "DECLARACIÓN RESPONSABLE DEL SISTEMA INFORMÁTICO DE FACTURACIÓN";

/// Un apartado de la declaración, con el texto descriptivo delante del dato.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Apartado {
    /// `a`…`l` (art. 15.2 de la orden).
    pub letra: char,
    pub etiqueta: String,
    pub valor: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Declaracion {
    pub titulo: String,
    /// Versión de TaxPain a la que se refiere (una declaración por versión).
    pub version: String,
    pub apartados: Vec<Apartado>,
    /// Datos del productor que faltan para poder firmarla.
    pub faltan: Vec<String>,
    /// Información complementaria (anexo que recomienda la orden).
    pub anexo: Vec<String>,
}

fn si_no(b: bool) -> String {
    if b { "Sí" } else { "No" }.into()
}

/// La declaración de esta versión de TaxPain.
#[must_use]
pub fn declaracion() -> Declaracion {
    use taxpain_core::productor;
    let sif = SistemaInformatico::taxpain("");
    let faltan: Vec<String> = productor::faltan_datos()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let a = |letra: char, etiqueta: &str, valor: String| Apartado {
        letra,
        etiqueta: etiqueta.into(),
        valor,
    };
    let apartados =
        vec![
        a(
            'a',
            "Nombre del sistema informático de facturación",
            sif.nombre_sistema.clone(),
        ),
        a(
            'b',
            "Código identificador del sistema informático (IdSistemaInformatico)",
            sif.id_sistema.clone(),
        ),
        a('c', "Identificador completo de la versión", sif.version.clone()),
        a(
            'd',
            "Componentes hardware y software, descripción y funcionalidades",
            "Aplicación de escritorio para macOS (Tauri 2: núcleo en Rust e interfaz web local), \
             sin servidor propio: los datos se guardan en el Mac del usuario en una base de datos \
             cifrada (SQLCipher) y los certificados, sellados en el Llavero. Funcionalidades de \
             facturación: emisión de facturas completas, simplificadas y rectificativas \
             (sustitución y diferencias) con numeración correlativa por serie; generación al \
             expedir cada factura de su registro de facturación de alta, y de anulación cuando \
             procede, encadenados con huella SHA-256 e inalterables; remisión automática de los \
             registros a la AEAT por su servicio web con el certificado electrónico del usuario, \
             con control de flujo, reintentos y subsanación; código QR y leyenda VERI*FACTU en \
             la factura; consulta de los registros remitidos. Incluye además contabilidad, \
             libros registro y borradores de modelos tributarios, que no forman parte del \
             sistema de facturación."
                .into(),
        ),
        a(
            'e',
            "Sólo puede funcionar como VERI*FACTU",
            si_no(sif.solo_verifactu),
        ),
        // TODO(VERIFY): TaxPain lleva varias empresas en la misma instalación (una base de datos
        // por empresa). Hay que confirmar si eso es «permitir su uso por varios obligados» del
        // art. 15.2.f y, si lo es, poner `TipoUsoPosibleMultiOT = S` en los registros
        // (`SistemaInformatico::taxpain`) y aquí a la vez.
        a(
            'f',
            "Permite su uso por varios obligados tributarios o por un mismo usuario para varios \
             obligados",
            si_no(sif.multi_ot),
        ),
        a(
            'g',
            "Tipos de firma utilizados (sólo en modalidad no VERI*FACTU)",
            if sif.solo_verifactu {
                "No aplica: el sistema sólo funciona como VERI*FACTU y los registros remitidos no \
                 se firman (art. 16.2 del Reglamento aprobado por el RD 1007/2023)."
                    .into()
            } else {
                "XAdES Enveloped".into()
            },
        ),
        a(
            'h',
            "Nombre o razón social del productor",
            productor::NOMBRE.into(),
        ),
        a('i', "NIF del productor", productor::NIF.into()),
        a(
            'j',
            "Dirección postal completa de contacto del productor",
            productor::DIRECCION
                .map_or_else(|| "[dirección postal completa]".into(), str::to_owned),
        ),
        a(
            'k',
            "Manifestación de cumplimiento",
            format!(
                "{} declara que el sistema informático de facturación aquí descrito, en la \
                 versión indicada, cumple con lo dispuesto en el artículo 29.2.j) de la Ley \
                 58/2003, de 17 de diciembre, General Tributaria; en el Reglamento que establece \
                 los requisitos que deben adoptar los sistemas y programas informáticos o \
                 electrónicos que soporten los procesos de facturación de empresarios y \
                 profesionales, aprobado por el Real Decreto 1007/2023, de 5 de diciembre; en la \
                 Orden HAC/1177/2024, de 17 de octubre, y en las especificaciones técnicas \
                 publicadas en la sede electrónica de la Agencia Estatal de Administración \
                 Tributaria.",
                productor::NOMBRE
            ),
        ),
        a(
            'l',
            "Fecha y lugar de la firma",
            "[día, mes y año] en [localidad], España".into(),
        ),
    ];
    Declaracion {
        titulo: TITULO.into(),
        version: sif.version,
        apartados,
        faltan,
        anexo: vec![
            "Los registros de facturación se remiten al servicio web VERI*FACTU de la AEAT; \
             mientras el envío a producción no está habilitado, esta versión sólo envía al \
             entorno de pruebas y las facturas no llevan QR ni leyenda."
                .into(),
            "El productor conserva esta declaración para cada versión del sistema (art. 15.4 de \
             la Orden HAC/1177/2024) y la entrega a quien adquiera el sistema."
                .into(),
        ],
    }
}

/// La declaración como PDF para firmar.
pub fn pdf() -> Result<Vec<u8>> {
    let d = declaracion();
    let mut p = vec![Para::Title(d.titulo.clone()), Para::Gap];
    for a in &d.apartados {
        p.push(Para::Bold(format!("{}) {}:", a.letra, a.etiqueta)));
        p.push(Para::Text(a.valor.clone()));
        p.push(Para::Gap);
    }
    p.push(Para::Gap);
    p.push(Para::Text("Firma del productor:".into()));
    p.push(Para::Gap);
    p.push(Para::Gap);
    p.push(Para::Gap);
    p.push(Para::Bold("Anexo".into()));
    for t in &d.anexo {
        p.push(Para::Text(t.clone()));
    }
    pdf::text_document(&format!("{} · TaxPain {}", d.titulo, d.version), &[p])
        .map_err(AppError::Other)
}

/// Nombre del fichero: una declaración por versión.
#[must_use]
pub fn nombre_fichero() -> String {
    format!(
        "declaracion-responsable-TaxPain-{}.pdf",
        SistemaInformatico::taxpain("").version
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lleva_los_doce_apartados_en_orden_y_los_datos_del_registro() {
        let d = declaracion();
        let letras: String = d.apartados.iter().map(|a| a.letra).collect();
        assert_eq!(letras, "abcdefghijkl");
        let sif = SistemaInformatico::taxpain("x");
        assert_eq!(d.apartados[1].valor, sif.id_sistema);
        assert_eq!(d.apartados[2].valor, sif.version);
        assert_eq!(d.apartados[7].valor, taxpain_core::productor::NOMBRE);
        assert_eq!(d.apartados[8].valor, taxpain_core::productor::NIF);
        assert_eq!(d.titulo, TITULO);
        // Sin dirección, la declaración avisa de que falta y deja el hueco a la vista.
        if taxpain_core::productor::DIRECCION.is_none() {
            assert!(d.faltan.iter().any(|f| f.contains("dirección")));
            assert!(d.apartados[9].valor.starts_with('['));
        }
        assert!(nombre_fichero().contains(&sif.version));
    }

    #[test]
    fn el_pdf_tiene_el_titulo_y_la_manifestacion() {
        let bytes = pdf().unwrap();
        assert!(bytes.starts_with(b"%PDF"));
        let text = crate::pdf_text::text(&bytes).unwrap_or_default();
        assert!(text.contains("DECLARACIÓN RESPONSABLE"), "{text}");
        assert!(text.contains("29.2.j)"), "{text}");
    }
}
