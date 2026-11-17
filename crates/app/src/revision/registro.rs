//! Libros registro frente a las facturas de la carpeta: facturas del libro que no tienen PDF y
//! PDF que no están en el libro (de los trimestres que cubre el libro). Se casan una a una: por el
//! número (también si el libro sólo guarda su final o lo tapa con «*»), por el total y la fecha,
//! o, en otra moneda, por el proveedor y la fecha.

use std::collections::{BTreeMap, BTreeSet};

use taxpain_core::money::Money;
use time::{Date, Duration};

use super::expediente::{
    Clase, Doc, Expediente, Factura, nif_clave, nif_español, nombre_clave, nombres_parecidos,
};
use super::modelos::{Clave, Declarado};
use super::{Area, Destinatario, Gravedad, Nuevo, Responsable, Salida, eur, fecha, lista, plural};
use crate::libros::{Book, BookInvoice, BookKind, number_key};

pub(crate) fn revisar(exp: &Expediente, s: &mut Salida) {
    let dec = super::modelos::declarados(exp);
    let vende_reducido = vende_con_tipo_reducido(exp, &dec);
    let casados = casados(exp);
    for (n, c) in casados.iter().enumerate() {
        // Un PDF que está en otro libro de la misma clase (el del año anterior, el de una
        // factura de diciembre registrada en enero) no está fuera de los libros.
        let en_otro: BTreeSet<&str> = casados
            .iter()
            .enumerate()
            .filter(|(m, o)| *m != n && o.libro.kind == c.libro.kind)
            .flat_map(|(_, o)| o.casados())
            .collect();
        cruzar(exp, c, &en_otro, vende_reducido, &dec, s);
    }
}

/// Un libro registro de la carpeta casado con los PDF de su periodo.
pub(crate) struct Casado<'a> {
    pub doc: &'a Doc,
    pub libro: &'a Book,
    /// Las líneas que se cruzan: en emitidas, sin la autorrepercusión de las compras.
    pub lineas: Vec<&'a BookInvoice>,
    /// En emitidas, las líneas que son la autorrepercusión de una compra.
    pub auto: Vec<&'a BookInvoice>,
    /// Las facturas de su clase y su periodo en la carpeta.
    pub pdfs: Vec<(&'a Doc, &'a Factura)>,
    /// El PDF de cada línea, si lo tiene (su posición en `pdfs`).
    pub con_pdf: Vec<Option<usize>>,
    /// Si cada PDF está en el libro.
    pub en_libro: Vec<bool>,
}

impl<'a> Casado<'a> {
    /// Los ficheros de los PDF que están en el libro.
    fn casados(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.pdfs
            .iter()
            .zip(&self.en_libro)
            .filter(|(_, ok)| **ok)
            .map(|(p, _)| p.0.ruta.as_str())
    }

    /// El PDF de una línea del libro, si lo tiene.
    pub fn pdf(&self, linea: usize) -> Option<(&'a Doc, &'a Factura)> {
        self.con_pdf
            .get(linea)
            .copied()
            .flatten()
            .map(|p| self.pdfs[p])
    }
}

/// Los libros registro de la carpeta (una vez cada copia exacta) casados con los PDF.
pub(crate) fn casados(exp: &Expediente) -> Vec<Casado<'_>> {
    let compras = super::iva::recibidas_de(exp);
    let mut vistos = BTreeSet::new();
    let mut out = Vec::new();
    for d in &exp.docs {
        let Some(libro) = d.libro.as_ref() else {
            continue;
        };
        // Un libro de emitidas que es todo autorrepercusión llega sin líneas (la lectura del libro
        // ya las apartó en `Doc::autorrepercusion`), pero entra para decirlo en una nota.
        if (libro.invoices.is_empty() && d.autorrepercusion.is_empty())
            || !vistos.insert(d.sha.as_str())
        {
            continue;
        }
        let recibidas = libro.kind == BookKind::Recibidas;
        let (mut auto, lineas): (Vec<&BookInvoice>, Vec<&BookInvoice>) = if recibidas {
            (Vec::new(), libro.invoices.iter().collect())
        } else {
            libro
                .invoices
                .iter()
                .partition(|i| super::iva::es_autorrepercusion(i, &compras))
        };
        // Y las que apartó la lectura del libro: su total no suma la cuota, o son compras del
        // libro de recibidas de la carpeta.
        auto.extend(&d.autorrepercusion);
        auto.sort_by(|a, b| (a.issue_date, &a.number).cmp(&(b.issue_date, &b.number)));
        let pdfs = pdfs_de(exp, libro, &d.autorrepercusion);
        let (con_pdf, en_libro) = emparejar(&lineas, &pdfs, recibidas);
        out.push(Casado {
            doc: d,
            libro,
            lineas,
            auto,
            pdfs,
            con_pdf,
            en_libro,
        });
    }
    out
}

/// Número de factura para buscarlo dentro de otro: mayúsculas, sólo letras y cifras (y el «*»
/// de los números tapados), con la O como 0 y la I como 1, que se confunden al teclearlos.
fn compacto(s: &str) -> Vec<char> {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == '*')
        .flat_map(char::to_uppercase)
        .map(|c| match c {
            'O' => '0',
            'I' => '1',
            c => c,
        })
        .collect()
}

/// Posiciones que valen cualquier carácter en un número tapado: el «*» y las «X» seguidas
/// («TLF000****», «F-001XXXX»).
fn comodines(n: &[char]) -> Vec<bool> {
    let mut out = vec![false; n.len()];
    let mut i = 0;
    while i < n.len() {
        match n[i] {
            '*' => {
                out[i] = true;
                i += 1;
            }
            'X' => {
                let largo = n[i..].iter().take_while(|c| **c == 'X').count();
                if largo >= 2 {
                    out[i..i + largo].iter_mut().for_each(|w| *w = true);
                }
                i += largo;
            }
            _ => i += 1,
        }
    }
    out
}

/// Cómo casan dos números de factura.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Numero {
    /// El mismo con [`crate::libros::number_key`] (sin separadores ni ceros a la izquierda),
    /// tomando la O por 0 y la I por 1.
    Igual,
    /// Uno, de 6 caracteres o más, es el final del otro: hay libros que sólo guardan los
    /// últimos.
    Final,
    /// Casan con los comodines de un número tapado.
    Comodin,
    No,
}

fn numeros(a: &str, b: &str) -> Numero {
    let cifras = |s: &str| -> String {
        s.chars()
            .map(|c| match c {
                'O' | 'o' => '0',
                'I' | 'i' => '1',
                c => c,
            })
            .collect()
    };
    let (ka, kb) = (number_key(&cifras(a)), number_key(&cifras(b)));
    if !ka.is_empty() && ka == kb {
        return Numero::Igual;
    }
    let (ca, cb) = (compacto(a), compacto(b));
    let (corto, largo) = if ca.len() <= cb.len() {
        (&ca, &cb)
    } else {
        (&cb, &ca)
    };
    let (wc, wl) = (comodines(corto), comodines(largo));
    if corto.len() < 6 || wc.iter().filter(|w| !**w).count() < 4 {
        return Numero::No;
    }
    let desde = largo.len() - corto.len();
    let mut comodin = false;
    for (i, c) in corto.iter().enumerate() {
        if wc[i] || wl[desde + i] {
            comodin = true;
        } else if *c != largo[desde + i] {
            return Numero::No;
        }
    }
    if comodin {
        Numero::Comodin
    } else {
        Numero::Final
    }
}

/// NIF y nombre de la otra parte de una factura de la carpeta: el emisor en recibidas, el
/// destinatario en emitidas.
fn otra_parte(f: &Factura, recibidas: bool) -> (Option<&str>, Option<&str>) {
    if recibidas {
        (f.emisor_nif.as_deref(), f.emisor_nombre.as_deref())
    } else {
        (
            f.destinatario_nif.as_deref(),
            f.destinatario_nombre.as_deref(),
        )
    }
}

/// ¿De la misma otra parte?
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Parte {
    /// El mismo NIF o un nombre parecido.
    Misma,
    /// A alguna le falta el NIF y el nombre no lo confirma.
    SinNif,
    /// NIF y nombre distintos.
    Otra,
}

fn parte(i: &BookInvoice, f: &Factura, recibidas: bool) -> Parte {
    let (nif, nombre) = otra_parte(f, recibidas);
    match (i.nif.as_deref(), nif) {
        (Some(a), Some(b)) if nif_clave(a) == nif_clave(b) => Parte::Misma,
        _ if nombre.is_some_and(|n| nombres_parecidos(&i.name, n)) => Parte::Misma,
        (Some(_), Some(_)) => Parte::Otra,
        _ => Parte::SinNif,
    }
}

/// La fecha del PDF está entre la de la factura del libro y la de su registro: unos días antes
/// (el libro puede llevar la fecha en que se registró) o después.
fn en_fechas(i: &BookInvoice, f: &Factura) -> bool {
    let Some(d) = f.fecha else {
        return false;
    };
    let desde = i.issue_date.min(i.registered) - Duration::days(20);
    let hasta = i.issue_date.max(i.registered) + Duration::days(3);
    (desde..=hasta).contains(&d)
}

/// Días entre la fecha del PDF y la del libro (para quedarse con el más cercano).
fn distancia(i: &BookInvoice, f: &Factura) -> i64 {
    f.fecha
        .map_or(i64::MAX, |d| (d - i.issue_date).whole_days().abs())
}

/// Pasos para casar el libro con los PDF, del más seguro al menos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Paso {
    /// Mismo número del mismo año y la misma otra parte (o alguna sin NIF).
    Numero,
    /// Un número es el final del otro, con el mismo NIF o nombre.
    Final,
    /// Un número tapado casa con el otro, con el mismo NIF o nombre.
    Comodin,
    /// Sin número que case: mismo NIF o nombre, mismo total y fechas cercanas.
    Importe,
    /// En otra moneda (el libro la tiene en euros): mismo NIF o nombre y fechas cercanas.
    Divisa,
}

fn casa(paso: Paso, i: &BookInvoice, f: &Factura, recibidas: bool) -> bool {
    let p = parte(i, f, recibidas);
    if p == Parte::Otra {
        return false;
    }
    // La numeración puede empezar cada año: de años distintos, no es la misma.
    let mismo_año = f.fecha.is_none_or(|d| d.year() == i.issue_date.year());
    let numero = f
        .numero
        .as_deref()
        .map_or(Numero::No, |n| numeros(&i.number, n));
    match paso {
        Paso::Numero => mismo_año && numero == Numero::Igual,
        Paso::Final => mismo_año && p == Parte::Misma && numero == Numero::Final,
        Paso::Comodin => mismo_año && p == Parte::Misma && numero == Numero::Comodin,
        Paso::Importe => {
            let totales = [i.total, i.base() + i.vat() + i.surcharge()];
            p == Parte::Misma
                && en_fechas(i, f)
                && f.total.is_some_and(|t| {
                    totales
                        .iter()
                        .any(|x| (t - *x).abs() <= Money::from_cents(1))
                })
        }
        Paso::Divisa => f.moneda.is_some() && p == Parte::Misma && en_fechas(i, f),
    }
}

/// Los PDF que son la misma factura (copias exactas, el mismo emisor y número del mismo año, o
/// una copia leída sin el número con la misma otra parte, fecha y total): se casan una vez.
fn agrupar(pdfs: &[(&Doc, &Factura)], recibidas: bool) -> Vec<Vec<usize>> {
    let parte = |f: &Factura| -> Option<String> {
        let (nif, nombre) = otra_parte(f, recibidas);
        nif.map(nif_clave)
            .filter(|n| !n.is_empty())
            .or_else(|| nombre.map(nombre_clave).filter(|n| !n.is_empty()))
    };
    let mut de: BTreeMap<String, usize> = BTreeMap::new();
    let mut grupos: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    // Primero las que tienen número: una copia leída sin él se junta después con la suya.
    let mut orden: Vec<usize> = (0..pdfs.len()).collect();
    orden.sort_by_key(|n| pdfs[*n].1.numero.is_none());
    for n in orden {
        let (d, f) = pdfs[n];
        let numero = f
            .numero
            .as_deref()
            .map(number_key)
            .filter(|k| !k.is_empty());
        let mut claves = vec![format!("h:{}", d.sha)];
        if let (Some(e), Some(k)) = (f.emisor_clave(), numero.as_ref()) {
            claves.push(format!("n:{e}|{k}|{:?}", f.fecha.map(Date::year)));
        }
        let importe = match (parte(f), f.fecha, f.total) {
            (Some(p), Some(fe), Some(t)) => Some(format!("t:{p}|{fe}|{}", t.cents())),
            _ => None,
        };
        let g = claves
            .iter()
            .chain(importe.iter().filter(|_| numero.is_none()))
            .find_map(|k| de.get(k).copied())
            .unwrap_or(n);
        for k in claves.into_iter().chain(importe) {
            de.entry(k).or_insert(g);
        }
        grupos.entry(g).or_default().push(n);
    }
    grupos.into_values().collect()
}

/// Casa una a una las facturas del libro con los PDF: (el PDF de cada factura del libro, si lo
/// tiene; los PDF que están en el libro).
fn emparejar(
    lineas: &[&BookInvoice],
    pdfs: &[(&Doc, &Factura)],
    recibidas: bool,
) -> (Vec<Option<usize>>, Vec<bool>) {
    let grupos = agrupar(pdfs, recibidas);
    let mut linea_ok: Vec<Option<usize>> = vec![None; lineas.len()];
    let mut grupo_ok = vec![false; grupos.len()];
    for paso in [
        Paso::Numero,
        Paso::Final,
        Paso::Comodin,
        Paso::Importe,
        Paso::Divisa,
    ] {
        for (li, i) in lineas.iter().enumerate() {
            if linea_ok[li].is_some() {
                continue;
            }
            let mejor = grupos
                .iter()
                .enumerate()
                .filter(|(g, _)| !grupo_ok[*g])
                .filter(|(_, ps)| ps.iter().any(|p| casa(paso, i, pdfs[*p].1, recibidas)))
                .min_by_key(|(_, ps)| distancia(i, pdfs[ps[0]].1));
            if let Some((g, ps)) = mejor {
                linea_ok[li] = Some(ps[0]);
                grupo_ok[g] = true;
            }
        }
    }
    let mut pdf_ok = vec![false; pdfs.len()];
    for (g, ps) in grupos.iter().enumerate() {
        for (k, p) in ps.iter().enumerate() {
            // De un grupo sin casar, sólo el primero cuenta como fuera del libro.
            pdf_ok[*p] = grupo_ok[g] || k > 0;
        }
    }
    (linea_ok, pdf_ok)
}

/// La factura recibida lleva IVA español (o, sin NIF, IVA a secas): si falta en el libro, ese
/// IVA no se ha deducido.
pub(crate) fn con_iva_español(exp: &Expediente, f: &Factura) -> bool {
    if f.moneda.is_some() || f.iva.is_some_and(|i| i.cents() <= 0) {
        return false;
    }
    match (f.emisor_nif.as_deref(), f.iva) {
        (Some(n), _) => nif_español(n).is_some() && !exp.es_propio(n),
        (None, Some(_)) => true,
        // Sin IVA leído ni NIF: no se sabe.
        (None, None) => false,
    }
}

/// Las líneas de un libro de emitidas que son la autorrepercusión de compras no son ventas: se
/// cuentan aparte y no se buscan sus PDF. Son las que apartó la lectura del libro (su total no
/// suma la cuota, o son compras del libro de recibidas de la carpeta) y las que están también en
/// el libro de recibidas.
fn autorrepercusion(c: &Casado, s: &mut Salida) {
    let (d, auto, ventas) = (c.doc, &c.auto, &c.lineas);
    if auto.is_empty() {
        return;
    }
    let años: BTreeSet<i32> = auto.iter().map(|i| i.issue_date.year()).collect();
    let años_txt = años
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let ejemplos: Vec<String> = auto
        .iter()
        .map(|i| format!("{} ({})", i.number, i.name))
        .collect();
    let lineas = lista(&ejemplos, 6);
    let titulo = if ventas.is_empty() {
        format!("El libro de emitidas de {años_txt} es la autorrepercusión de compras")
    } else {
        format!(
            "El libro de emitidas de {años_txt} trae la autorrepercusión de {}",
            plural(auto.len(), "compra", "compras")
        )
    };
    let detalle = if ventas.is_empty() {
        format!(
            "Todas sus líneas son el IVA autorrepercutido de compras con inversión del sujeto \
             pasivo o intracomunitarias (arts. 84.Uno.2.º y 85 LIVA), no ventas: {lineas}. La \
             gestoría las apunta también en el libro de emitidas para declarar su IVA. No se \
             cuentan como facturas emitidas (ni en la numeración ni frente al 303). Si lo \
             importas en TaxPain, no lo hagas como libro de emitidas."
        )
    } else if auto.len() == 1 {
        format!(
            "Una línea es el IVA autorrepercutido de una compra con inversión del sujeto pasivo \
             o intracomunitaria (arts. 84.Uno.2.º y 85 LIVA), no una venta: {lineas}. No se \
             cuenta como factura emitida (ni en la numeración ni frente al 303). Si lo importas \
             en TaxPain, quita esa línea de las emitidas."
        )
    } else {
        format!(
            "{} líneas son el IVA autorrepercutido de compras con inversión del sujeto pasivo o \
             intracomunitarias (arts. 84.Uno.2.º y 85 LIVA), no ventas: {lineas}. No se cuentan \
             como facturas emitidas (ni en la numeración ni frente al 303). Si lo importas en \
             TaxPain, quita esas líneas de las emitidas.",
            auto.len()
        )
    };
    s.add(
        Nuevo {
            codigo: "libros.autorrepercusion",
            area: Area::Libros,
            gravedad: Gravedad::Info,
            responsable: Responsable::Empresa,
            titulo,
            ficheros: vec![d.ruta.clone()],
        }
        .detalle(detalle),
        (años.len() == 1)
            .then(|| años.iter().copied().next())
            .flatten(),
        None,
    );
}

/// El trimestre del registro de una factura del libro (el de su deducción).
fn del_registro(i: &BookInvoice) -> (i32, u8) {
    (i.registered.year(), super::trimestre(i.registered))
}

/// Lo que se pagaría por deducir menos en cada trimestre (en negativo, como los importes a
/// pagar), con una nota: si los 303 salen a compensar o a devolver y lo pendiente lo cubre, no
/// hay nada que ingresar. Sin esos 303, todo.
fn a_pagar(
    dec: &BTreeMap<Clave, Declarado>,
    por_trimestre: &BTreeMap<(i32, u8), Money>,
    total: Money,
) -> (Option<Money>, String) {
    if total.is_zero() {
        return (None, String::new());
    }
    match super::modelos::a_ingresar(dec, por_trimestre) {
        Some(x) if x.is_zero() => (
            None,
            format!(
                " {} a compensar o a devolver y lo cubre{}: corregido, queda menos por compensar o \
                 devolver y no hay que ingresar nada.",
                if por_trimestre.len() == 1 {
                    "El 303 de ese trimestre sale"
                } else {
                    "Los 303 de esos trimestres salen"
                },
                if por_trimestre.len() == 1 { "" } else { "n" }
            ),
        ),
        Some(x) if x < total => (
            Some(-x),
            format!(
                " Con lo que quedaba por compensar en los 303, habría que ingresar unos {}.",
                eur(x)
            ),
        ),
        _ => (Some(-total), String::new()),
    }
}

/// Pares de líneas del libro que parecen la misma factura apuntada dos veces: del mismo
/// tercero y del mismo día, con los mismos importes y con números que pueden ser el mismo
/// (iguales, uno el final del otro o uno tapado).
fn pares_repetidos(lineas: &[&BookInvoice]) -> Vec<(usize, usize)> {
    let mut usadas = vec![false; lineas.len()];
    let mut out = Vec::new();
    for (a, x) in lineas.iter().enumerate() {
        if usadas[a] {
            continue;
        }
        let otra = lineas.iter().enumerate().skip(a + 1).find(|(b, y)| {
            !usadas[*b]
                && x.issue_date == y.issue_date
                && x.base() == y.base()
                && x.vat() == y.vat()
                && x.total == y.total
                && !x.base().is_zero()
                && super::iva::mismo_tercero(x, y)
                && numeros(&x.number, &y.number) != Numero::No
        });
        if let Some((b, _)) = otra {
            usadas[b] = true;
            out.push((a, b));
        }
    }
    out
}

/// Número tapado con «*» o con «X» seguidas.
fn tapado(n: &str) -> bool {
    comodines(&compacto(n)).contains(&true)
}

/// Dos líneas del libro del mismo tercero, del mismo día, con los mismos importes y con números
/// que pueden ser el mismo (iguales, uno el final del otro o uno tapado con «*»): la factura
/// está apuntada dos veces. También una factura con dos líneas de IVA iguales (dos filas con el
/// mismo número, que el libro junta).
fn repetidas_en_el_libro(
    d: &Doc,
    lineas: &[&BookInvoice],
    recibidas: bool,
    dec: &BTreeMap<Clave, Declarado>,
    años_txt: &str,
    ejercicio: Option<i32>,
    s: &mut Salida,
) {
    let mut repetidas: Vec<String> = Vec::new();
    let mut de_mas = Money::ZERO;
    let mut por_trimestre: BTreeMap<(i32, u8), Money> = BTreeMap::new();
    let pares: BTreeMap<usize, usize> = pares_repetidos(lineas).into_iter().collect();
    // El IVA que cambia el resultado del 303: el de las ventas o el deducido de una compra
    // interior (el autorrepercutido se devenga y se deduce; el no deducible no se dedujo).
    let cuenta =
        |i: &BookInvoice| !recibidas || super::iva::destino(i) == super::iva::Destino::Interior;
    for (a, x) in lineas.iter().enumerate() {
        for (k, l) in x.lines.iter().enumerate() {
            if !l.base.is_zero() && x.lines[..k].contains(l) {
                repetidas.push(format!(
                    "la {} de {} ({}) tiene dos veces la línea de {} de base",
                    x.number,
                    x.name,
                    fecha(x.issue_date),
                    eur(l.base)
                ));
                if cuenta(x) {
                    de_mas += l.vat;
                    *por_trimestre.entry(del_registro(x)).or_default() += l.vat;
                }
            }
        }
        if let Some(y) = pares.get(&a).map(|b| lineas[*b]) {
            repetidas.push(format!(
                "la {} y la {} de {} ({}, {})",
                x.number,
                y.number,
                x.name,
                fecha(x.issue_date),
                eur(x.total)
            ));
            if cuenta(y) {
                de_mas += y.vat();
                *por_trimestre.entry(del_registro(y)).or_default() += y.vat();
            }
        }
    }
    if repetidas.is_empty() {
        return;
    }
    let que = if recibidas { "recibidas" } else { "emitidas" };
    let n = repetidas.len();
    let (importe, pago) = if recibidas {
        a_pagar(dec, &por_trimestre, de_mas)
    } else {
        ((!de_mas.is_zero()).then_some(de_mas), String::new())
    };
    let efecto = if de_mas.is_zero() {
        "Si se llevó dos veces al 303, sus casillas no casan aunque no cambie el resultado (es \
         una inversión del sujeto pasivo, una intracomunitaria o un IVA que no se dedujo)"
            .to_owned()
    } else if recibidas {
        format!(
            "Si se dedujo dos veces, sobran {} de IVA en el 303 de ese trimestre (y el gasto \
             repetido, en el Impuesto sobre Sociedades)",
            eur(de_mas)
        )
    } else {
        format!(
            "Si se declaró dos veces, sobran {} de IVA devengado en el 303 de ese trimestre (y el \
             ingreso repetido, en el Impuesto sobre Sociedades)",
            eur(de_mas)
        )
    };
    s.add(
        Nuevo {
            codigo: "libros.repetidas",
            area: Area::Libros,
            gravedad: Gravedad::Aviso,
            responsable: Responsable::Gestoria,
            titulo: if n == 1 {
                format!("Una factura apuntada dos veces en el libro de {que} de {años_txt}")
            } else {
                format!("{n} facturas apuntadas dos veces en el libro de {que} de {años_txt}")
            },
            ficheros: vec![d.ruta.clone()],
        }
        .detalle(format!(
            "Parecen la misma factura: {}. {efecto}: pide a la gestoría que lo compruebe.{pago}",
            lista(&repetidas, 6)
        )),
        ejercicio,
        importe,
    );
    s.pedir(
        Destinatario::Gestoria,
        if n == 1 {
            format!(
                "Que compruebe la factura del libro de {que} de {años_txt} que parece apuntada dos \
                 veces"
            )
        } else {
            format!(
                "Que compruebe las {n} facturas del libro de {que} de {años_txt} que parecen \
                 apuntadas dos veces"
            )
        },
    );
}

/// Gastos cuyo IVA no se puede deducir, o sólo si son deducibles en el Impuesto sobre
/// Sociedades (art. 96.Uno LIVA).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SinDerecho {
    /// Alimentos, bebidas y tabaco (96.Uno.3.º).
    Alimentos,
    /// Espectáculos y servicios recreativos (96.Uno.4.º).
    Ocio,
    /// Atenciones a clientes, asalariados o terceros: regalos (96.Uno.5.º).
    Atenciones,
    /// Desplazamientos, hostelería y restauración: sólo si el gasto es deducible en el Impuesto
    /// sobre Sociedades (96.Uno.6.º).
    Restauracion,
}

impl SinDerecho {
    fn texto(self) -> &'static str {
        match self {
            SinDerecho::Alimentos => "alimentos o bebidas (art. 96.Uno.3.º LIVA)",
            SinDerecho::Ocio => "espectáculos u ocio (art. 96.Uno.4.º LIVA)",
            SinDerecho::Atenciones => "regalos o atenciones (art. 96.Uno.5.º LIVA)",
            SinDerecho::Restauracion => "restaurante, hotel o viaje (art. 96.Uno.6.º LIVA)",
        }
    }
}

/// Qué gasto es por lo que dice un texto (la lectura del PDF o los nombres del proveedor), por
/// palabras enteras: «fuente de alimentación» o «conservación» no son comida, y las palabras
/// que en una factura de otro oficio son corrientes («entradas» de un blog, la «cesta» de una
/// tienda en línea, el «menú» de una web, una cámara de «cine») sólo cuentan en su frase.
fn sin_derecho(texto: &str) -> Option<SinDerecho> {
    let palabras: Vec<String> = texto
        .split(|c: char| !c.is_alphanumeric())
        .map(crate::import::norm)
        .filter(|w| !w.is_empty())
        .collect();
    let hay = |lista: &[&str]| palabras.iter().any(|w| lista.contains(&w.as_str()));
    let frase = format!(" {} ", palabras.join(" "));
    let dice = |lista: &[&str]| lista.iter().any(|f| frase.contains(&format!(" {f} ")));
    // «Alimentación», salvo la de un aparato («fuente de alimentación», «cable de…»).
    let alimentacion = hay(&["alimentacion"])
        && !dice(&[
            "fuente de alimentacion",
            "cable de alimentacion",
            "adaptador de alimentacion",
            "toma de alimentacion",
            "alimentacion electrica",
        ]);
    if alimentacion
        || hay(&[
            "alimentos",
            "alimento",
            "alimenticios",
            "bebida",
            "bebidas",
            "vino",
            "vinos",
            "cerveza",
            "cervezas",
            "licor",
            "licores",
            "jamon",
            "embutidos",
            "supermercado",
            "tabaco",
            "conservas",
            "comestibles",
            "enoteca",
            "vinoteca",
        ])
    {
        Some(SinDerecho::Alimentos)
    } else if hay(&["regalo", "regalos", "obsequio", "obsequios"])
        || dice(&[
            "cesta de navidad",
            "cestas de navidad",
            "cesta navidena",
            "cestas navidenas",
            "lote de navidad",
            "lotes de navidad",
            "lote navideno",
            "lotes navidenos",
        ])
    {
        Some(SinDerecho::Atenciones)
    } else if hay(&[
        "espectaculo",
        "espectaculos",
        "concierto",
        "conciertos",
        "teatro",
        "cines",
    ]) || dice(&[
        "entrada de cine",
        "entradas de cine",
        "entradas del cine",
        "entradas para el cine",
        "sesion de cine",
    ]) {
        Some(SinDerecho::Ocio)
    } else if hay(&[
        "restaurante",
        "restaurantes",
        "restaurant",
        "comensales",
        "comida",
        "comidas",
        "cena",
        "cenas",
        "almuerzo",
        "cafeteria",
        "cerveceria",
        "bar",
        "sushi",
        "taberna",
        "hotel",
        "hoteles",
        "hostal",
        "hosteleria",
        "gastronomia",
    ]) || dice(&["menu del dia", "menus del dia", "menu degustacion"])
    {
        Some(SinDerecho::Restauracion)
    } else {
        None
    }
}

/// ¿Vende la empresa con IVA reducido (4, 5 o 10 %)? Por las ventas de los libros de emitidas,
/// las facturas emitidas de la carpeta y las casillas [01], [153] y [04] de los 303. Un bar, un
/// restaurante o una tienda de alimentación compran comida y bebida para venderla.
fn vende_con_tipo_reducido(exp: &Expediente, dec: &BTreeMap<Clave, Declarado>) -> bool {
    let reducido = |bp: i32| matches!(bp, 400 | 500 | 1000);
    let compras = super::iva::recibidas_de(exp);
    let en_libros = exp
        .docs
        .iter()
        .filter_map(|d| d.libro.as_ref())
        .filter(|l| l.kind == BookKind::Emitidas)
        .flat_map(|l| &l.invoices)
        .filter(|i| !super::iva::es_autorrepercusion(i, &compras))
        .any(|i| {
            i.lines
                .iter()
                .any(|l| l.base.cents() > 0 && !l.vat.is_zero() && reducido(l.rate_bp))
        });
    en_libros
        || exp
            .emitidas()
            .any(|(_, f)| f.tipo().is_some_and(|t| matches!(t, 4 | 5 | 10)))
        || dec
            .iter()
            .any(|((m, ..), d)| m == "303" && [1u32, 153, 4].iter().any(|c| d.c(*c).cents() > 0))
}

/// IVA que el libro deduce de gastos sin derecho a deducir: alimentos y bebidas, ocio, regalos
/// (y restaurantes, hoteles o viajes, que sólo se deducen si son gasto del Impuesto sobre
/// Sociedades). Se sabe qué es por la lectura de su PDF o por el nombre del proveedor. Es un
/// aviso, con lo que habría que deducir de menos, sólo si lo dice la lectura del PDF y no puede
/// ser algo que la empresa compra para venderlo (art. 96.Dos.2.º y 3.º LIVA): sólo por el
/// nombre del proveedor, o comida y bebida de una empresa que vende con IVA reducido, es una
/// nota.
#[allow(clippy::too_many_arguments)]
fn sin_derecho_a_deducir(
    d: &Doc,
    lineas: &[&BookInvoice],
    con_pdf: &[Option<usize>],
    pdfs: &[(&Doc, &Factura)],
    vende_reducido: bool,
    dec: &BTreeMap<Clave, Declarado>,
    años_txt: &str,
    ejercicio: Option<i32>,
    s: &mut Salida,
) {
    let mut filas = Vec::new();
    let mut casos = Vec::new();
    let mut no_deducible = Money::ZERO;
    let mut por_trimestre: BTreeMap<(i32, u8), Money> = BTreeMap::new();
    let mut dudoso = false;
    let mut seguro = false;
    let (mut restauracion, mut por_nombre, mut para_vender) = (false, false, false);
    for (i, p) in lineas.iter().zip(con_pdf) {
        if super::iva::destino(i) != super::iva::Destino::Interior {
            continue;
        }
        let mut nombres = i.name.clone();
        let mut lectura = None;
        if let Some((doc, f)) = p.map(|p| pdfs[p]) {
            lectura = sin_derecho(&doc.descripcion);
            if let Some(n) = &f.emisor_nombre {
                nombres.push(' ');
                nombres.push_str(n);
            }
        }
        let Some(que) = lectura.or_else(|| sin_derecho(&nombres)) else {
            continue;
        };
        let solo_nombre = lectura.is_none();
        let a_la_venta = que == SinDerecho::Alimentos && vende_reducido;
        let mut cual = que.texto().to_owned();
        if que == SinDerecho::Restauracion {
            restauracion = true;
        } else if solo_nombre {
            por_nombre = true;
            cual = format!("{cual}, por el nombre del proveedor");
        } else if a_la_venta {
            para_vender = true;
        }
        if que == SinDerecho::Restauracion || solo_nombre || a_la_venta {
            dudoso = true;
        } else {
            seguro = true;
            no_deducible += i.vat();
            *por_trimestre.entry(del_registro(i)).or_default() += i.vat();
        }
        casos.push(format!(
            "la {} de {} ({}): {cual}, {} de IVA",
            i.number,
            i.name,
            fecha(i.registered),
            eur(i.vat())
        ));
        filas.push(vec![
            fecha(i.registered),
            i.number.clone(),
            i.name.clone(),
            eur(i.vat()),
            cual,
        ]);
    }
    if casos.is_empty() {
        return;
    }
    let mut detalle = format!(
        "El libro deduce el IVA de {}. El IVA de los alimentos y las bebidas, los espectáculos y \
         el ocio y los regalos o atenciones a clientes, empleados o terceros no se puede deducir \
         (art. 96.Uno.3.º, 4.º y 5.º LIVA); el de restaurantes, hoteles y viajes, sólo si el \
         gasto es deducible en el Impuesto sobre Sociedades (art. 96.Uno.6.º LIVA). Sí se deduce \
         el de lo que la empresa compra para venderlo, tal cual o transformado, o para prestarlo \
         como servicio (un bar, una tienda, una agencia de viajes…; art. 96.Dos.2.º y 3.º LIVA).",
        lista(&casos, 6)
    );
    let (importe, pago) = a_pagar(dec, &por_trimestre, no_deducible);
    if !no_deducible.is_zero() {
        detalle.push_str(&format!(
            " Corregido, {} {} menos.{pago}",
            if por_trimestre.len() == 1 {
                "el 303 de ese trimestre deduce"
            } else {
                "los 303 de esos trimestres deducen"
            },
            eur(no_deducible)
        ));
    }
    if para_vender {
        detalle.push_str(
            " La empresa vende con IVA reducido (4, 5 o 10 %): si vende comida o bebida, las de \
             alimentos y bebidas pueden ser compras para vender.",
        );
    }
    if por_nombre {
        detalle.push_str(
            " Las que sólo se ven por el nombre del proveedor hay que mirarlas en su factura.",
        );
    }
    if restauracion {
        detalle.push_str(
            " Los de restaurante u hotel valen si son de la actividad (con clientes o en un viaje \
             de trabajo) y están a nombre de la empresa.",
        );
    }
    detalle.push_str(
        " Si no son gastos de la empresa, tampoco cuentan en el Impuesto sobre Sociedades.",
    );
    s.add(
        Nuevo {
            codigo: "libros.iva_sin_derecho",
            area: Area::Libros,
            gravedad: if seguro {
                Gravedad::Aviso
            } else {
                Gravedad::Info
            },
            responsable: Responsable::Gestoria,
            titulo: format!(
                "El libro de recibidas de {años_txt} deduce el IVA de {} que {} derecho a deducir",
                plural(casos.len(), "gasto", "gastos"),
                match (seguro, dudoso, casos.len() == 1) {
                    (true, false, true) => "no da",
                    (true, false, false) => "no dan",
                    (true, true, _) => "no dan, o pueden no dar,",
                    (false, _, true) => "puede no dar",
                    (false, _, false) => "pueden no dar",
                }
            ),
            ficheros: vec![d.ruta.clone()],
        }
        .detalle(detalle),
        ejercicio,
        importe,
    );
    s.tabla(
        format!("IVA deducido de gastos sin derecho a deducir ({años_txt})"),
        "IVA sin derecho a deducir",
        &["Fecha", "Número", "Proveedor", "IVA", "Gasto"],
        filas,
    );
}

/// Los PDF que se buscan en un libro: facturas de su clase, de los trimestres que cubre y con
/// datos para casarlas.
fn pdfs_de<'a>(
    exp: &'a Expediente,
    libro: &Book,
    apartadas: &[BookInvoice],
) -> Vec<(&'a Doc, &'a Factura)> {
    // El periodo es el de todas las líneas del libro, también las de la autorrepercusión que la
    // lectura apartó.
    let todas: Vec<BookInvoice>;
    let lineas: &[BookInvoice] = if apartadas.is_empty() {
        &libro.invoices
    } else {
        todas = libro.invoices.iter().chain(apartadas).cloned().collect();
        &todas
    };
    let Some((desde, hasta)) = super::iva::periodo_del_libro(lineas) else {
        return Vec::new();
    };
    let clase = if libro.kind == BookKind::Recibidas {
        Clase::FacturaRecibida
    } else {
        Clase::FacturaEmitida
    };
    exp.facturas()
        .filter(|(p, f)| {
            p.clase == clase
                && f.fecha.is_some_and(|x| (desde..=hasta).contains(&x))
                && (f.numero.is_some() || f.total.is_some())
        })
        .collect()
}

/// Facturas recibidas en PDF que no están en ninguna hoja de los libros de recibidas que las
/// cubren (las que no cubre ningún libro no salen).
pub(crate) fn recibidas_fuera_de_los_libros(exp: &Expediente) -> Vec<(&Doc, &Factura)> {
    let mut candidatas: Vec<(&Doc, &Factura)> = Vec::new();
    let mut en_libro: BTreeSet<&str> = BTreeSet::new();
    for c in casados(exp)
        .iter()
        .filter(|c| c.libro.kind == BookKind::Recibidas)
    {
        for (p, ok) in c.pdfs.iter().zip(&c.en_libro) {
            if *ok {
                en_libro.insert(p.0.ruta.as_str());
            } else if !candidatas.iter().any(|x| x.0.ruta == p.0.ruta) {
                candidatas.push(*p);
            }
        }
    }
    candidatas
        .into_iter()
        .filter(|(d, _)| !en_libro.contains(d.ruta.as_str()))
        .collect()
}

fn cruzar(
    exp: &Expediente,
    c: &Casado,
    en_otro: &BTreeSet<&str>,
    vende_reducido: bool,
    dec: &BTreeMap<Clave, Declarado>,
    s: &mut Salida,
) {
    let (d, libro) = (c.doc, c.libro);
    let recibidas = libro.kind == BookKind::Recibidas;
    autorrepercusion(c, s);
    let (lineas, pdfs, con_pdf) = (&c.lineas, &c.pdfs, &c.con_pdf);
    let años: BTreeSet<i32> = c
        .lineas
        .iter()
        .chain(&c.auto)
        .map(|i| i.issue_date.year())
        .collect();
    let años_txt = años
        .iter()
        .map(i32::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let que = if recibidas { "recibidas" } else { "emitidas" };
    let ejercicio = (años.len() == 1)
        .then(|| años.iter().copied().next())
        .flatten();

    repetidas_en_el_libro(d, lineas, recibidas, dec, &años_txt, ejercicio, s);
    if recibidas {
        sin_derecho_a_deducir(
            d,
            lineas,
            con_pdf,
            pdfs,
            vende_reducido,
            dec,
            &años_txt,
            ejercicio,
            s,
        );
    }

    // 1. Del libro, sin PDF. La línea repetida de una factura que tiene su PDF no pide otro, y
    // de una repetida sin PDF se pide uno (el del número entero).
    let mut sobra = vec![false; lineas.len()];
    for (a, b) in pares_repetidos(lineas) {
        match (con_pdf[a].is_some(), con_pdf[b].is_some()) {
            (true, false) => sobra[b] = true,
            (false, true) => sobra[a] = true,
            (false, false) if tapado(&lineas[a].number) && !tapado(&lineas[b].number) => {
                sobra[a] = true;
            }
            (false, false) => sobra[b] = true,
            (true, true) => {}
        }
    }
    let sin_pdf: Vec<&BookInvoice> = lineas
        .iter()
        .zip(con_pdf)
        .zip(&sobra)
        .filter(|((_, p), sobra)| p.is_none() && !**sobra)
        .map(|((i, _), _)| *i)
        .collect();
    if !sin_pdf.is_empty() {
        let filas: Vec<Vec<String>> = sin_pdf
            .iter()
            .map(|i| {
                vec![
                    fecha(i.issue_date),
                    i.number.clone(),
                    i.name.clone(),
                    i.nif.clone().unwrap_or_default(),
                    eur(i.base()),
                    eur(i.vat()),
                    eur(i.total),
                ]
            })
            .collect();
        let ejemplos: Vec<String> = sin_pdf
            .iter()
            .map(|i| format!("{} ({})", i.number, i.name))
            .collect();
        s.add(
            Nuevo {
                codigo: "libros.sin_pdf",
                area: Area::Libros,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Documentacion,
                titulo: format!(
                    "{} del libro de {que} de {años_txt} sin el PDF en la carpeta",
                    plural(sin_pdf.len(), "factura", "facturas")
                ),
                ficheros: vec![d.ruta.clone()],
            }
            .detalle(format!(
                "Están en el libro ({}) pero no hay su factura en la carpeta: {}. Hay que \
                 guardar todas las facturas (art. 29.2.e LGT); pídelas a la gestoría o al \
                 {}.",
                d.ruta,
                lista(&ejemplos, 6),
                if recibidas {
                    "proveedor"
                } else {
                    "programa de facturación"
                }
            )),
            ejercicio,
            None,
        );
        s.tabla(
            format!("Libro de {que} de {años_txt}: facturas sin PDF"),
            format!("Libro de {que}, sin PDF"),
            &["Fecha", "Número", "Nombre", "NIF", "Base", "IVA", "Total"],
            filas,
        );
        s.pedir(
            Destinatario::Gestoria,
            if sin_pdf.len() == 1 {
                format!(
                    "La factura {} de {años_txt} que está en el libro pero no en la carpeta",
                    if recibidas { "recibida" } else { "emitida" }
                )
            } else {
                format!(
                    "Las {} facturas {que} de {años_txt} que están en el libro pero no en la \
                     carpeta",
                    sin_pdf.len()
                )
            },
        );
    }

    // 2. PDF del periodo del libro que no están ni en él ni en otro libro de la misma clase. El
    // periodo, por la fecha de registro en recibidas: una factura de diciembre registrada en
    // enero no hace que el libro del año siguiente cubra el último trimestre del anterior.
    let periodo = super::iva::periodo_propio(libro);
    let fuera: Vec<(&Doc, &Factura)> = pdfs
        .iter()
        .zip(&c.en_libro)
        .filter(|(p, ok)| {
            !**ok
                && !en_otro.contains(p.0.ruta.as_str())
                && periodo.is_some_and(|(a, b)| p.1.fecha.is_some_and(|f| (a..=b).contains(&f)))
        })
        .map(|(p, _)| *p)
        .collect();
    if fuera.is_empty() {
        return;
    }
    let (graves, resto): (Vec<_>, Vec<_>) = fuera
        .into_iter()
        .partition(|(_, f)| !recibidas || con_iva_español(exp, f));
    let fila = |p: &Doc, f: &Factura| {
        vec![
            f.fecha.map(fecha).unwrap_or_default(),
            f.numero.clone().unwrap_or_default(),
            f.emisor_nombre.clone().unwrap_or_default(),
            f.base.map(eur).unwrap_or_default(),
            f.iva.map(eur).unwrap_or_default(),
            f.total.map(eur).unwrap_or_default(),
            p.ruta.clone(),
        ]
    };
    // Las del periodo del libro (uno de enero a septiembre no tiene las de octubre).
    let periodo = periodo
        .map(|(a, b)| {
            format!(
                " Se miran las del periodo del libro, del {} al {}.",
                fecha(a),
                fecha(b)
            )
        })
        .unwrap_or_default();
    if !graves.is_empty() {
        let detalle = if recibidas {
            "Facturas con IVA español que no están en el libro de recibidas: si son gastos de la \
             empresa, su IVA no se ha deducido (se puede recuperar en los cuatro años \
             siguientes); si no son de la empresa, no deberían estar en su carpeta."
        } else {
            "Facturas emitidas que no están en el libro de emitidas: su IVA puede no haberse \
             declarado en el 303. Confírmalo con la gestoría."
        };
        let detalle = format!("{detalle}{periodo}");
        s.add(
            Nuevo {
                codigo: "libros.pdf_fuera_del_libro",
                area: Area::Libros,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Gestoria,
                titulo: format!(
                    "{} de {años_txt} que no {} en el libro de {que}",
                    plural(graves.len(), "factura", "facturas"),
                    if graves.len() == 1 { "está" } else { "están" }
                ),
                ficheros: graves.iter().map(|(p, _)| p.ruta.clone()).collect(),
            }
            .detalle(detalle),
            ejercicio,
            None,
        );
        s.tabla(
            format!("Facturas {que} de {años_txt} que no están en el libro"),
            format!("Fuera del libro de {que}"),
            &[
                "Fecha", "Número", "Emisor", "Base", "IVA", "Total", "Fichero",
            ],
            graves.iter().map(|(p, f)| fila(p, f)).collect(),
        );
    }
    if !resto.is_empty() {
        s.add(
            Nuevo {
                codigo: "libros.otras_fuera_del_libro",
                area: Area::Libros,
                gravedad: Gravedad::Info,
                responsable: Responsable::Gestoria,
                titulo: format!(
                    "{} sin IVA español de {años_txt} que no {} en el libro",
                    plural(resto.len(), "factura", "facturas"),
                    if resto.len() == 1 { "está" } else { "están" }
                ),
                ficheros: resto.iter().map(|(p, _)| p.ruta.clone()).collect(),
            }
            .detalle(format!(
                "Son de proveedores de fuera de España, sin IVA o en otra moneda, y no están en \
                 ninguna hoja del libro (tampoco en las de adquisiciones intracomunitarias o \
                 inversión del sujeto pasivo). Si son gastos de la empresa, su IVA \
                 autorrepercutido no se ha declarado; las de otra moneda pueden estar en el \
                 libro con otro número o con el importe en euros.{periodo}"
            )),
            ejercicio,
            None,
        );
        s.tabla(
            format!("Facturas sin IVA español de {años_txt} que no están en el libro"),
            "Sin IVA fuera del libro",
            &[
                "Fecha", "Número", "Emisor", "Base", "IVA", "Total", "Fichero",
            ],
            resto.iter().map(|(p, f)| fila(p, f)).collect(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gastos_sin_derecho_a_deducir() {
        assert_eq!(
            sin_derecho("Compra de alimentación y bebidas"),
            Some(SinDerecho::Alimentos)
        );
        assert_eq!(
            sin_derecho("6 botellas de vino"),
            Some(SinDerecho::Alimentos)
        );
        assert_eq!(
            sin_derecho("Regalo de empresa: experiencia gastronómica"),
            Some(SinDerecho::Atenciones)
        );
        assert_eq!(
            sin_derecho("RESTAURANTE DEMO SL, comida para 4"),
            Some(SinDerecho::Restauracion)
        );
        assert_eq!(sin_derecho("Entradas de teatro"), Some(SinDerecho::Ocio));
        // Ni la fuente de alimentación de un ordenador ni la conservación de un edificio.
        assert_eq!(sin_derecho("Fuente de alimentación 650 W"), None);
        assert_eq!(sin_derecho("Cable de alimentación para el monitor"), None);
        assert_eq!(
            sin_derecho("Alimentación para la reunión"),
            Some(SinDerecho::Alimentos)
        );
        assert_eq!(sin_derecho("Servicio de conservación del edificio"), None);
        assert_eq!(sin_derecho("Cuota mensual de la gestoría"), None);
        // Palabras de otros oficios: sólo cuentan en su frase.
        assert_eq!(sin_derecho("Redacción de 6 entradas del blog"), None);
        assert_eq!(
            sin_derecho("Programación de la cesta de la compra y del menú de la web"),
            None
        );
        assert_eq!(sin_derecho("Alquiler de una cámara de cine"), None);
        assert_eq!(
            sin_derecho("Cestas de Navidad para los clientes"),
            Some(SinDerecho::Atenciones)
        );
        assert_eq!(sin_derecho("4 entradas de cine"), Some(SinDerecho::Ocio));
        assert_eq!(
            sin_derecho("Menú del día para 3"),
            Some(SinDerecho::Restauracion)
        );
    }

    #[test]
    fn numeros_que_casan() {
        assert_eq!(numeros("F-0077", "f 77"), Numero::Igual);
        // La O por el 0 y la I por el 1 al teclear.
        assert_eq!(numeros("PO-12", "P0-12"), Numero::Igual);
        // El libro guarda los últimos caracteres (y con la O por el 0).
        assert_eq!(numeros("CDOE7-0004", "155CD0E7-0004"), Numero::Final);
        assert_eq!(numeros("2025/3311", "TS9 2025/3311"), Numero::Final);
        // Un número tapado con «*» o con «X» seguidas.
        assert_eq!(numeros("QWE000****", "QWE0007788"), Numero::Comodin);
        assert_eq!(numeros("MV-TLF001XXXX", "TLF0015577"), Numero::Comodin);
        // Demasiado corto para un final, u otro número.
        assert_eq!(numeros("A/38", "B-A/38"), Numero::No);
        assert_eq!(numeros("B/101", "B/107"), Numero::No);
        assert_eq!(numeros("******", "123456"), Numero::No);
    }
}
