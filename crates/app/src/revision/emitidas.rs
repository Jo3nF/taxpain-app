//! Numeración de las facturas emitidas: por serie, saltos y fechas desordenadas. Se miran las
//! facturas emitidas de la carpeta y las de los libros de emitidas.

use std::collections::{BTreeMap, BTreeSet};

use time::Date;

use super::expediente::Expediente;
use super::{Area, Gravedad, Nuevo, Responsable, Salida, fecha, plural};

/// Serie y número de una factura: el último grupo de cifras es el número y lo de delante, la
/// serie («WEB-0005» → «WEB-», 5; «2025-0001» → «2025-», 1). `None` si no acaba en cifras.
pub(crate) fn serie_y_numero(n: &str) -> Option<(String, u64)> {
    let n = n.trim();
    let digitos = n.chars().rev().take_while(char::is_ascii_digit).count();
    if digitos == 0 || digitos > 9 {
        return None;
    }
    let (serie, numero) = n.split_at(n.len() - digitos);
    Some((
        serie.trim().to_uppercase().replace(' ', ""),
        numero.parse().ok()?,
    ))
}

/// Saltos de una lista ordenada de números, como rangos: «31–33, 36».
fn saltos(numeros: &[u64]) -> Vec<String> {
    let mut out = Vec::new();
    for w in numeros.windows(2) {
        let (a, b) = (w[0], w[1]);
        if b > a + 1 {
            out.push(if b == a + 2 {
                (a + 1).to_string()
            } else {
                format!("{}–{}", a + 1, b - 1)
            });
        }
    }
    out
}

/// Factura emitida vista en la carpeta o en un libro de emitidas: su número, su fecha y dónde
/// está.
#[derive(Debug, Clone)]
struct Vista {
    numero: String,
    fecha: Option<Date>,
    fichero: String,
}

/// Facturas de una serie, por número (con todas las veces que se ha visto cada uno).
type Serie = BTreeMap<u64, Vec<Vista>>;

/// Las facturas por serie. Una serie que vuelve a empezar cada año (el mismo número con fechas
/// de años distintos) se mira año a año: «sin serie de 2024», «sin serie de 2025».
fn agrupar(vistas: Vec<Vista>) -> Vec<(String, Serie)> {
    let mut series: BTreeMap<String, Serie> = BTreeMap::new();
    for v in vistas {
        let Some((serie, n)) = serie_y_numero(&v.numero) else {
            continue;
        };
        series
            .entry(serie)
            .or_default()
            .entry(n)
            .or_default()
            .push(v);
    }
    let mut out = Vec::new();
    for (serie, facturas) in series {
        let nombre = if serie.is_empty() {
            "sin serie".to_owned()
        } else {
            format!("«{serie}»")
        };
        let reinicia = facturas.values().any(|vs| {
            vs.iter()
                .filter_map(|v| v.fecha.map(Date::year))
                .collect::<BTreeSet<_>>()
                .len()
                > 1
        });
        if !reinicia {
            out.push((nombre, facturas));
            continue;
        }
        let mut por_año: BTreeMap<Option<i32>, Serie> = BTreeMap::new();
        for (n, vs) in facturas {
            for v in vs {
                por_año
                    .entry(v.fecha.map(Date::year))
                    .or_default()
                    .entry(n)
                    .or_default()
                    .push(v);
            }
        }
        for (año, facturas) in por_año {
            let nombre = match año {
                Some(y) => format!("{nombre} de {y}"),
                None => format!("{nombre} sin fecha"),
            };
            out.push((nombre, facturas));
        }
    }
    out
}

/// Emitidas sin IVA a clientes de otros países de la UE que no dicen por qué no lo llevan: las
/// de servicios tienen que poner «inversión del sujeto pasivo» y las de bienes, la exención
/// (art. 6.1.j y m del Reglamento de facturación).
fn sin_mencion(exp: &Expediente, s: &mut Salida) {
    let ficheros: Vec<String> = exp
        .emitidas()
        .filter(|(d, _)| d.mencion_sin_iva == Some(false))
        .map(|(d, _)| d.ruta.clone())
        .collect();
    if ficheros.is_empty() {
        return;
    }
    s.add(
        Nuevo {
            codigo: "emitidas.sin_mencion",
            area: Area::Emitidas,
            gravedad: Gravedad::Info,
            responsable: Responsable::Empresa,
            titulo: format!(
                "{} sin IVA a clientes de la UE sin la mención de la inversión del sujeto pasivo",
                plural(ficheros.len(), "factura emitida", "facturas emitidas")
            ),
            ficheros,
        }
        .detalle(
            "Van sin IVA a clientes con NIF-IVA de otro país de la UE y su texto no dice por qué. \
             En los servicios tiene que poner «inversión del sujeto pasivo» (art. 6.1.m del \
             Reglamento de facturación) y en las entregas de bienes, que están exentas, el \
             artículo de la exención (art. 25 LIVA; art. 6.1.j del Reglamento). Cada una se \
             corrige con una factura rectificativa que lo diga (art. 15.1 del Reglamento), y en \
             las próximas hay que ponerlo."
                .into(),
        ),
        None,
        None,
    );
}

pub(crate) fn revisar(exp: &Expediente, s: &mut Salida) {
    sin_mencion(exp, s);
    let mut vistas = Vec::new();
    for (d, f) in exp.emitidas() {
        if let Some(n) = f.numero.as_deref() {
            vistas.push(Vista {
                numero: n.to_owned(),
                fecha: f.fecha,
                fichero: d.ruta.clone(),
            });
        }
    }
    // Las líneas que son la autorrepercusión de compras llevan el número del proveedor.
    let compras = super::iva::recibidas_de(exp);
    for d in &exp.docs {
        let Some(libro) = d.libro.as_ref() else {
            continue;
        };
        if libro.kind != crate::libros::BookKind::Emitidas {
            continue;
        }
        for i in libro
            .invoices
            .iter()
            .filter(|i| !super::iva::es_autorrepercusion(i, &compras))
        {
            vistas.push(Vista {
                numero: i.number.clone(),
                fecha: Some(i.issue_date),
                fichero: d.ruta.clone(),
            });
        }
    }
    let mut filas = Vec::new();
    for (nombre, facturas) in &agrupar(vistas) {
        let numeros: Vec<u64> = facturas.keys().copied().collect();
        let (Some(primera), Some(ultima)) = (numeros.first(), numeros.last()) else {
            continue;
        };
        let huecos = saltos(&numeros);
        let mut desordenadas = Vec::new();
        let mut anterior: Option<(&str, Date)> = None;
        for vs in facturas.values() {
            let Some(f) = vs.iter().filter_map(|v| v.fecha).min() else {
                continue;
            };
            let numero = vs.first().map_or("", |v| v.numero.as_str());
            if let Some((a, fa)) = anterior
                && f < fa
            {
                desordenadas.push(format!(
                    "la {numero} ({}) es anterior a la {a} ({})",
                    fecha(f),
                    fecha(fa)
                ));
            }
            anterior = Some((numero, f));
        }
        filas.push(vec![
            nombre.clone(),
            primera.to_string(),
            ultima.to_string(),
            numeros.len().to_string(),
            huecos.join(", "),
        ]);
        let mut ficheros: Vec<String> = Vec::new();
        for v in facturas.values().flatten() {
            if !ficheros.contains(&v.fichero) {
                ficheros.push(v.fichero.clone());
            }
        }
        if !huecos.is_empty() {
            s.add(
                Nuevo {
                    codigo: "emitidas.saltos",
                    area: Area::Emitidas,
                    gravedad: Gravedad::Aviso,
                    responsable: Responsable::Empresa,
                    titulo: format!("Saltos en la numeración de la serie {nombre}"),
                    ficheros: ficheros.clone(),
                }
                .detalle(format!(
                    "Van de la {primera} a la {ultima} y faltan: {}. La numeración tiene que \
                     ser correlativa (art. 6.1.a del Reglamento de facturación): busca esas \
                     facturas (pueden estar en otra carpeta) o confirma que se anularon. Si es \
                     una serie de autofacturación del cliente, los saltos pueden ser normales.",
                    huecos.join(", ")
                )),
                None,
                None,
            );
        }
        if !desordenadas.is_empty() {
            s.add(
                Nuevo {
                    codigo: "emitidas.desordenadas",
                    area: Area::Emitidas,
                    gravedad: Gravedad::Info,
                    responsable: Responsable::Empresa,
                    titulo: format!(
                        "{} de la serie {nombre} con fecha fuera de orden",
                        plural(desordenadas.len(), "factura", "facturas")
                    ),
                    ficheros,
                }
                .detalle(format!(
                    "En una serie, una factura con número mayor no puede tener fecha anterior: \
                     {}.",
                    desordenadas.join("; ")
                )),
                None,
                None,
            );
        }
    }
    s.tabla(
        "Series de facturas emitidas",
        "Series emitidas",
        &["Serie", "Primera", "Última", "Facturas", "Faltan"],
        filas,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serie_y_numero_de_una_factura() {
        assert_eq!(serie_y_numero("WEB-0005"), Some(("WEB-".into(), 5)));
        assert_eq!(serie_y_numero("2025-0001"), Some(("2025-".into(), 1)));
        assert_eq!(
            serie_y_numero("AUTO_XY12_30"),
            Some(("AUTO_XY12_".into(), 30))
        );
        assert_eq!(serie_y_numero("123"), Some((String::new(), 123)));
        assert_eq!(serie_y_numero("ANUNCIOS-000?"), None);
    }

    fn vista(numero: &str, fecha: Date) -> Vista {
        Vista {
            numero: numero.into(),
            fecha: Some(fecha),
            fichero: format!("{numero}.pdf"),
        }
    }

    #[test]
    fn series_que_empiezan_cada_año() {
        use time::macros::date;
        // Sin serie, del 1 al 3 en 2024 y otra vez del 1 al 2 en 2025: dos series sin saltos.
        let g = agrupar(vec![
            vista("1", date!(2024 - 01 - 10)),
            vista("2", date!(2024 - 05 - 10)),
            vista("3", date!(2024 - 09 - 10)),
            vista("1", date!(2025 - 01 - 15)),
            vista("2", date!(2025 - 02 - 15)),
        ]);
        let nombres: Vec<&str> = g.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(nombres, ["sin serie de 2024", "sin serie de 2025"]);
        assert!(
            g.iter()
                .all(|(_, f)| saltos(&f.keys().copied().collect::<Vec<_>>()).is_empty())
        );
        // Una serie que sigue de un año a otro se mira entera: falta la A-3.
        let g = agrupar(vec![
            vista("A-1", date!(2024 - 11 - 10)),
            vista("A-2", date!(2024 - 12 - 10)),
            vista("A-4", date!(2025 - 01 - 10)),
        ]);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].0, "«A-»");
        assert_eq!(saltos(&g[0].1.keys().copied().collect::<Vec<_>>()), ["3"]);
    }

    #[test]
    fn saltos_como_rangos() {
        assert_eq!(saltos(&[30, 34, 35, 38]), vec!["31–33", "36–37"]);
        assert_eq!(saltos(&[1, 3]), vec!["2"]);
        assert!(saltos(&[1, 2, 3]).is_empty());
    }
}
