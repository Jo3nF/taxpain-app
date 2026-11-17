//! Ficheros: recuento por tipo, copias idénticas, formatos que no se leen y lo que falta por
//! leer con detalle.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use regex::Regex;

use super::expediente::{Clase, Expediente};
use super::{Area, Destinatario, Gravedad, Nuevo, Responsable, Salida, lista, plural};

pub(crate) fn revisar(exp: &Expediente, s: &mut Salida) {
    recuento(exp, s);
    copias(exp, s);
    no_admitidos(exp, s);
    sin_leer(exp, s);
}

/// La carpeta no se ha leído entera: por pasar del máximo de ficheros o por subcarpetas
/// demasiado profundas. Sin este aviso, lo que no se lee saldría como «falta».
pub(crate) fn incompleta(fuera: usize, profundas: usize, s: &mut Salida) {
    if fuera == 0 && profundas == 0 {
        return;
    }
    let mut partes = Vec::new();
    if fuera > 0 {
        partes.push(format!(
            "{} no se han leído porque la carpeta pasa del máximo que lee TaxPain",
            plural(fuera, "fichero", "ficheros")
        ));
    }
    if profundas > 0 {
        partes.push(format!(
            "{} a más de ocho niveles no se han recorrido",
            plural(profundas, "subcarpeta", "subcarpetas")
        ));
    }
    s.add(
        Nuevo {
            codigo: "ficheros.carpeta_incompleta",
            area: Area::Ficheros,
            gravedad: Gravedad::Error,
            responsable: Responsable::Documentacion,
            titulo: "La carpeta no se ha leído entera".to_string(),
            ficheros: Vec::new(),
        }
        .detalle(format!(
            "{}. Lo que no se ha leído puede salir en esta revisión como que falta. Divide la carpeta (por años o por empresas) y revisa cada parte.",
            partes.join("; ")
        )),
        None,
        None,
    );
}

fn recuento(exp: &Expediente, s: &mut Salida) {
    let filas = exp
        .por_tipo()
        .into_iter()
        .map(|c| vec![c.tipo, c.n.to_string()])
        .collect();
    s.tabla(
        "Ficheros por tipo",
        "Ficheros por tipo",
        &["Tipo", "Ficheros"],
        filas,
    );
}

/// ¿Lleva el nombre de una carpeta una referencia de la AEAT? Un asiento del registro
/// electrónico («RGE» y doce cifras), un expediente (el año, tres letras o cifras, ocho cifras y
/// una o dos letras), uno de un procedimiento (una letra y quince cifras) o una solicitud de
/// certificado («TCT» y cifras). `TODO(VERIFY)`: formatos de las carpetas que se descargan
/// de «Mis expedientes» de la Sede.
fn referencia_aeat(nombre: &str) -> bool {
    static RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
        Regex::new(
            r"\b(?:RGE\d{12}|(?:19|20)\d{2}[0-9A-Z]{3}\d{8}[A-Z]{1,2}|[A-Z]\d{15}|TCT\d{8,})\b",
        )
        .ok()
    });
    RE.as_ref().is_some_and(|re| re.is_match(nombre))
}

/// ¿Es el número de una notificación (trece cifras)?
fn numero_de_notificacion(nombre: &str) -> bool {
    static RE: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"\b\d{13}\b").ok());
    RE.as_ref().is_some_and(|re| re.is_match(nombre))
}

/// Carpetas (de la raíz hacia dentro) de una ruta, sin el fichero.
fn carpetas(ruta: &str) -> Vec<&str> {
    let mut v: Vec<&str> = ruta.split('/').collect();
    v.pop();
    v
}

/// Carpetas con un documento de la Sede de la AEAT reconocido por sus marcas (un recibo de
/// presentación, un requerimiento, un certificado…) que no es la copia de otro fichero: una copia
/// en otra carpeta (la de los documentos de la empresa, por ejemplo) no la hace de la Sede.
fn carpetas_con_documentos_de_la_sede<'a>(
    exp: &'a Expediente,
    por_huella: &BTreeMap<&str, Vec<String>>,
) -> BTreeSet<&'a str> {
    exp.docs
        .iter()
        .filter(|d| {
            d.oficial.is_some() && por_huella.get(d.sha.as_str()).is_some_and(|g| g.len() == 1)
        })
        .filter_map(|d| d.ruta.rsplit_once('/').map(|(c, _)| c))
        .collect()
}

/// ¿Está el fichero en la carpeta de un expediente de la Sede de la AEAT? En una carpeta con un
/// documento de la Sede, o en una cuyo nombre lleva una referencia de la AEAT (o el número de una
/// notificación, dentro de una carpeta de notificaciones). La raíz no cuenta.
fn en_la_sede(ruta: &str, con_documentos: &BTreeSet<&str>) -> bool {
    let cs = carpetas(ruta);
    if cs.is_empty() {
        return false;
    }
    if ruta
        .rsplit_once('/')
        .is_some_and(|(c, _)| con_documentos.contains(c))
    {
        return true;
    }
    let de_notificaciones = cs.iter().any(|c| {
        let c = crate::documentos::fold(c);
        c.contains("notificac") || c.contains("comunicac")
    });
    cs.iter()
        .any(|c| referencia_aeat(c) || (de_notificaciones && numero_de_notificacion(c)))
}

/// Ficheros con el mismo contenido (misma huella): se pueden contar dos veces. Las copias que
/// están en las carpetas de los expedientes de la Sede de la AEAT (la declaración presentada,
/// los anexos de un escrito, una notificación repetida) son como las guarda la Sede: si fuera de
/// esas carpetas hay como mucho una, sólo se dice. Y también se dice sólo de los justificantes
/// de la AEAT repetidos: cuentan una vez (por su número) y se guardan una vez (por su huella).
fn copias(exp: &Expediente, s: &mut Salida) {
    let mut por_huella: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for d in &exp.docs {
        por_huella.entry(&d.sha).or_default().push(d.ruta.clone());
    }
    let con_documentos = carpetas_con_documentos_de_la_sede(exp, &por_huella);
    let justificantes: BTreeSet<&str> = exp
        .docs
        .iter()
        .filter(|d| {
            d.clase == Clase::Modelo && d.modelo.as_ref().is_some_and(|m| m.justificante.is_some())
        })
        .map(|d| d.sha.as_str())
        .collect();
    type Grupos<'a> = BTreeMap<&'a str, Vec<String>>;
    let (justificantes, por_huella): (Grupos<'_>, Grupos<'_>) = por_huella
        .into_iter()
        .partition(|(sha, g)| g.len() > 1 && justificantes.contains(sha));
    if !justificantes.is_empty() {
        let sobran: usize = justificantes.values().map(|g| g.len() - 1).sum();
        s.add(
            Nuevo {
                codigo: "ficheros.copias_justificantes",
                area: Area::Ficheros,
                gravedad: Gravedad::Info,
                responsable: Responsable::Empresa,
                titulo: format!(
                    "{} de justificantes de la AEAT",
                    primera_mayuscula(&plural(sobran, "copia", "copias"))
                ),
                ficheros: justificantes.values().flatten().cloned().collect(),
            }
            .detalle(format!(
                "Hay {} de justificantes de modelos presentados que están más de una vez (el \
                 mismo PDF en dos carpetas). No se cuentan dos veces: la revisión los toma por su \
                 número de justificante y al importarlos se guardan una sola vez, así que no hace \
                 falta quitarlos.",
                plural(justificantes.len(), "grupo", "grupos")
            )),
            None,
            None,
        );
    }
    let (grupos, de_la_sede): (Vec<Vec<String>>, Vec<Vec<String>>) = por_huella
        .into_values()
        .filter(|g| g.len() > 1)
        .partition(|g| g.iter().filter(|r| !en_la_sede(r, &con_documentos)).count() > 1);
    if !de_la_sede.is_empty() {
        let sobran: usize = de_la_sede.iter().map(|g| g.len() - 1).sum();
        s.add(
            Nuevo {
                codigo: "ficheros.copias_sede",
                area: Area::Ficheros,
                gravedad: Gravedad::Info,
                responsable: Responsable::Empresa,
                titulo: format!(
                    "{} en las carpetas de los expedientes de la Sede de la AEAT",
                    primera_mayuscula(&plural(sobran, "copia", "copias"))
                ),
                ficheros: de_la_sede.iter().flatten().cloned().collect(),
            }
            .detalle(format!(
                "Hay {} de ficheros iguales en los que todas las copias, o todas menos una, \
                 están en las carpetas de los expedientes de la Sede de la AEAT: la declaración \
                 presentada, los anexos de un escrito, una notificación que se repite. Es como \
                 los guarda la Sede, así que no hace falta quitarlas; basta con no pasarlas a la \
                 gestoría como documentos nuevos.",
                plural(de_la_sede.len(), "grupo", "grupos")
            )),
            None,
            None,
        );
    }
    if grupos.is_empty() {
        return;
    }
    let sobran: usize = grupos.iter().map(|g| g.len() - 1).sum();
    let ficheros: Vec<String> = grupos.iter().flatten().cloned().collect();
    s.add(
        Nuevo {
            codigo: "ficheros.copias",
            area: Area::Ficheros,
            gravedad: Gravedad::Aviso,
            responsable: Responsable::Empresa,
            titulo: format!(
                "{} con el mismo contenido",
                plural(sobran, "copia de un fichero", "copias de ficheros")
            ),
            ficheros,
        }
        .detalle(format!(
            "Hay {} de ficheros idénticos: al importar o al pasarlos a la gestoría se pueden \
             contar dos veces (dos gastos, dos deducciones de IVA). Deja una sola copia de \
             cada uno.",
            plural(grupos.len(), "grupo", "grupos")
        )),
        None,
        None,
    );
    let filas = grupos
        .iter()
        .enumerate()
        .map(|(i, g)| vec![(i + 1).to_string(), g.join("\n")])
        .collect();
    s.tabla(
        "Copias idénticas",
        "Copias idénticas",
        &["Grupo", "Ficheros"],
        filas,
    );
}

/// «copia…» → «Copia…».
fn primera_mayuscula(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|p| p.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// Formatos que TaxPain no lee: zip, Word, Pages, HTML…
fn no_admitidos(exp: &Expediente, s: &mut Salida) {
    if exp.no_admitidos.is_empty() {
        return;
    }
    let factura = exp
        .no_admitidos
        .iter()
        .filter(|f| f.to_lowercase().contains("factura"))
        .cloned()
        .collect::<Vec<_>>();
    let mut detalle = format!(
        "No se han leído porque TaxPain no entiende su formato: {}.",
        lista(&exp.no_admitidos, 8)
    );
    if !factura.is_empty() {
        detalle.push_str(&format!(
            " Ojo: {} parece una factura; guárdala en PDF para que cuente.",
            lista(&factura, 3)
        ));
    }
    s.add(
        Nuevo {
            codigo: "ficheros.no_admitidos",
            area: Area::Ficheros,
            gravedad: Gravedad::Info,
            responsable: Responsable::Empresa,
            titulo: format!(
                "{} en un formato que no se lee",
                plural(exp.no_admitidos.len(), "fichero", "ficheros")
            ),
            ficheros: exp.no_admitidos.clone(),
        }
        .detalle(detalle),
        None,
        None,
    );
    for f in factura {
        s.pedir(
            Destinatario::Empresa,
            format!("Guardar en PDF la factura {f}, que sólo está en un formato que no se lee"),
        );
    }
}

/// Ficheros sin datos: con la lectura de IA o con Claude Code la revisión es completa.
fn sin_leer(exp: &Expediente, s: &mut Salida) {
    let sin: Vec<String> = exp
        .docs
        .iter()
        .filter(|d| d.clase == Clase::SinLeer)
        .map(|d| d.ruta.clone())
        .collect();
    let solo_reglas = exp
        .docs
        .iter()
        .filter(|d| {
            !d.con_ia
                && matches!(d.clase, Clase::FacturaRecibida | Clase::FacturaEmitida)
                && d.factura.as_ref().is_some_and(|f| f.base.is_none())
        })
        .count();
    if sin.is_empty() && solo_reglas == 0 {
        return;
    }
    let mut detalle = String::new();
    if !sin.is_empty() {
        detalle.push_str(&format!("No se sabe qué son: {}. ", lista(&sin, 8)));
    }
    if solo_reglas > 0 {
        detalle.push_str(&format!(
            "De {} sólo se ha leído el número, la fecha y el total (sin base ni IVA). ",
            plural(solo_reglas, "factura", "facturas")
        ));
    }
    detalle.push_str(
        "Para una revisión completa, léelos con IA (Importar → Carpeta de la empresa → «Leer \
         con IA») o con Claude Code (/leer-carpeta) y vuelve a revisar.",
    );
    s.add(
        Nuevo {
            codigo: "ficheros.sin_leer",
            area: Area::Ficheros,
            gravedad: Gravedad::Info,
            responsable: Responsable::Empresa,
            titulo: format!(
                "{} sin leer con detalle",
                plural(sin.len() + solo_reglas, "fichero", "ficheros")
            ),
            ficheros: sin,
        }
        .detalle(detalle),
        None,
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avisa_si_la_carpeta_no_se_ha_leido_entera() {
        let mut s = Salida::default();
        incompleta(0, 0, &mut s);
        assert!(s.hallazgos.is_empty());
        incompleta(8379, 1, &mut s);
        assert_eq!(s.hallazgos.len(), 1);
        let h = &s.hallazgos[0];
        assert_eq!(h.codigo, "ficheros.carpeta_incompleta");
        assert_eq!(h.gravedad, Gravedad::Error);
        assert!(
            h.detalle.contains("8379 ficheros no se han leído"),
            "{}",
            h.detalle
        );
        assert!(h.detalle.contains("1 subcarpeta"), "{}", h.detalle);
    }

    #[test]
    fn carpetas_de_los_expedientes_de_la_sede() {
        let nada = BTreeSet::new();
        assert!(en_la_sede(
            "AEAT/Expedientes/2024-01-10 Registro RGE123456782024/Anexo.pdf",
            &nada
        ));
        assert!(en_la_sede(
            "AEAT/Expedientes/2025-08-14 IS Devolución 2024 202420012345678G/Declaración.pdf",
            &nada
        ));
        assert!(en_la_sede(
            "Hacienda/Requerimiento I123452025000123/Escrito.pdf",
            &nada
        ));
        assert!(en_la_sede(
            "AEAT/Notificaciones/2025-03-03 Requerimiento 2512345678901/Documento.pdf",
            &nada
        ));
        // Trece cifras fuera de una carpeta de notificaciones no son una notificación.
        assert!(!en_la_sede("FACTURAS/2025 1234567890123/f.pdf", &nada));
        assert!(!en_la_sede("FACTURAS/2025/T1/factura.pdf", &nada));
        // La raíz no es una carpeta de la Sede, aunque tenga documentos de la Sede.
        let raiz = BTreeSet::from([""]);
        assert!(!en_la_sede("certificado.pdf", &raiz));
        let sede = BTreeSet::from(["Hacienda/Escritos"]);
        assert!(en_la_sede("Hacienda/Escritos/anexo.pdf", &sede));
    }
}
