//! Facturas de la carpeta (recibidas y emitidas): repetidas, importes que no cuadran, NIF con la
//! letra de control mal, a nombre de otro, proveedores extranjeros con IVA español, divisas y
//! trimestre equivocado.

use std::collections::{BTreeMap, BTreeSet};

use taxpain_core::money::Money;
use taxpain_core::nif::NifError;
use time::Date;

use super::expediente::{Clase, Doc, Expediente, Factura, nif_español, nombres_parecidos};
use super::{Area, Gravedad, Nuevo, Responsable, Salida, eur, fecha, importe, plural, trimestre};

pub(crate) fn revisar(exp: &Expediente, s: &mut Salida) {
    repetidas(exp, s);
    no_cuadran(exp, s);
    nif_mal(exp, s);
    a_nombre_de_otro(exp, s);
    extranjeros_con_iva(exp, s);
    divisas(exp, s);
    trimestre_de_la_carpeta(exp, s);
}

fn quien(f: &Factura) -> String {
    f.emisor_nombre
        .clone()
        .or_else(|| f.emisor_nif.clone())
        .unwrap_or_default()
}

fn num(f: &Factura) -> String {
    f.numero.clone().unwrap_or_default()
}

fn opt_eur(m: Option<Money>) -> String {
    m.map(eur).unwrap_or_default()
}

fn opt_fecha(f: &Factura) -> String {
    f.fecha.map(fecha).unwrap_or_default()
}

/// Misma factura (mismo emisor y número) en ficheros distintos. Las copias exactas ya salen en
/// «Ficheros»; aquí, las que son la misma factura con distinto fichero.
fn repetidas(exp: &Expediente, s: &mut Salida) {
    let mut grupos: BTreeMap<(String, String), Vec<&Doc>> = BTreeMap::new();
    for (d, f) in exp.facturas() {
        let (Some(emisor), Some(n)) = (f.emisor_clave(), f.numero.as_deref()) else {
            continue;
        };
        let clave = crate::libros::number_key(n);
        if clave.is_empty() {
            continue;
        }
        grupos.entry((emisor, clave)).or_default().push(d);
    }
    let mut filas = Vec::new();
    let mut ficheros = Vec::new();
    let año = |d: &Doc| d.factura.as_ref().and_then(|f| f.fecha).map(Date::year);
    for docs in grupos.values() {
        // La numeración puede empezar cada año: la 1 de 2024 y la 1 de 2025 no son la misma.
        let años: BTreeSet<i32> = docs.iter().filter_map(|d| año(d)).collect();
        let partes: Vec<Vec<&Doc>> = if años.len() > 1 {
            let mut por_año: BTreeMap<Option<i32>, Vec<&Doc>> = BTreeMap::new();
            for d in docs {
                por_año.entry(año(d)).or_default().push(d);
            }
            por_año.into_values().collect()
        } else {
            vec![docs.clone()]
        };
        for docs in partes {
            let huellas: BTreeSet<&str> = docs.iter().map(|d| d.sha.as_str()).collect();
            if huellas.len() < 2 {
                continue;
            }
            let Some(f) = docs[0].factura.as_ref() else {
                continue;
            };
            let rutas: Vec<String> = docs.iter().map(|d| d.ruta.clone()).collect();
            filas.push(vec![quien(f), num(f), opt_fecha(f), rutas.join("\n")]);
            ficheros.extend(rutas);
        }
    }
    if filas.is_empty() {
        return;
    }
    s.add(
        Nuevo {
            codigo: "facturas.repetidas",
            area: Area::Facturas,
            gravedad: Gravedad::Aviso,
            responsable: Responsable::Empresa,
            titulo: format!(
                "{} en más de un fichero",
                plural(filas.len(), "factura repetida", "facturas repetidas")
            ),
            ficheros,
        }
        .detalle(
            "La misma factura (mismo emisor y número) está en ficheros distintos, a veces en \
             trimestres distintos. Comprueba que sólo se ha contabilizado y deducido una vez."
                .into(),
        ),
        None,
        None,
    );
    s.tabla(
        "Facturas repetidas",
        "Facturas repetidas",
        &["Emisor", "Número", "Fecha", "Ficheros"],
        filas,
    );
}

/// El total no es base + IVA − retención.
fn no_cuadran(exp: &Expediente, s: &mut Salida) {
    let mut filas = Vec::new();
    let mut ficheros = Vec::new();
    for (d, f) in exp.facturas() {
        let (Some(base), Some(total)) = (f.base, f.total) else {
            continue;
        };
        let iva = f.iva.unwrap_or(Money::ZERO);
        let ret = f.retencion.unwrap_or(Money::ZERO);
        let esperado = base + iva - ret;
        let diff = total - esperado;
        // El total a pagar a veces no descuenta la retención (o la factura lo pone así).
        if diff.abs() <= Money::from_cents(1)
            || (total - (base + iva)).abs() <= Money::from_cents(1)
        {
            continue;
        }
        filas.push(vec![
            d.ruta.clone(),
            quien(f),
            num(f),
            eur(base),
            eur(iva),
            eur(ret),
            eur(total),
            eur(diff),
        ]);
        ficheros.push(d.ruta.clone());
    }
    if filas.is_empty() {
        return;
    }
    s.add(
        Nuevo {
            codigo: "facturas.no_cuadran",
            area: Area::Facturas,
            gravedad: Gravedad::Info,
            responsable: Responsable::Empresa,
            titulo: format!(
                "{} cuyo total no es base + IVA − retención",
                plural(filas.len(), "factura", "facturas")
            ),
            ficheros,
        }
        .detalle(
            "Casi siempre es un suplido (tasas o gastos pagados por cuenta de la empresa, sin \
             IVA, art. 78.Tres.3.º LIVA), una comisión o un error de la factura. El suplido no es \
             base imponible: comprueba que no se ha contabilizado como tal y que, si va en otra \
             factura, no se cuenta dos veces."
                .into(),
        ),
        None,
        None,
    );
    s.tabla(
        "Facturas cuyo total no cuadra",
        "Totales que no cuadran",
        &[
            "Fichero",
            "Emisor",
            "Número",
            "Base",
            "IVA",
            "Retención",
            "Total",
            "Diferencia",
        ],
        filas,
    );
}

/// NIF españoles con la letra (o el dígito) de control mal.
fn nif_mal(exp: &Expediente, s: &mut Salida) {
    let mut filas = Vec::new();
    let mut ficheros = Vec::new();
    for (d, f) in exp.facturas() {
        for (papel, nif) in [
            ("Emisor", f.emisor_nif.as_deref()),
            ("Destinatario", f.destinatario_nif.as_deref()),
        ] {
            let Some(nif) = nif else { continue };
            if matches!(nif_español(nif), Some(Err(NifError::Control))) {
                filas.push(vec![d.ruta.clone(), papel.into(), nif.into()]);
                ficheros.push(d.ruta.clone());
            }
        }
    }
    if filas.is_empty() {
        return;
    }
    s.add(
        Nuevo {
            codigo: "facturas.nif_control",
            area: Area::Facturas,
            gravedad: Gravedad::Aviso,
            responsable: Responsable::Empresa,
            titulo: format!(
                "{} con la letra de control mal",
                plural(filas.len(), "NIF", "NIF")
            ),
            ficheros,
        }
        .detalle(
            "Un NIF mal escrito invalida la factura a efectos de la deducción del IVA (art. 6 \
             del Reglamento de facturación) y hace que el 347 o el 349 no casen. Pide al \
             emisor una factura corregida."
                .into(),
        ),
        None,
        None,
    );
    s.tabla(
        "NIF con la letra de control mal",
        "NIF con el control mal",
        &["Fichero", "Quién", "NIF"],
        filas,
    );
}

/// Un administrador de la sociedad («NOMBRE (NIF), cargo») y de dónde sale («las escrituras»,
/// «Nóminas de TaxPain»…).
struct Admin {
    texto: String,
    segun: &'static str,
}

/// Los administradores de la sociedad: los que nombran las escrituras, los estatutos, el
/// Registro o las actas (de la carpeta o guardados en TaxPain) y, después, los de Nóminas.
fn administradores(exp: &Expediente) -> Vec<Admin> {
    use super::cruces::socios::nombra;
    use crate::documentos::DocKind;
    let segun = |k: DocKind| match k {
        DocKind::Estatutos => "los estatutos",
        DocKind::RegistroMercantil => "el Registro Mercantil",
        DocKind::ActaJunta => "las actas",
        _ => "las escrituras",
    };
    let mut out: Vec<Admin> = exp
        .docs
        .iter()
        .filter_map(|d| d.empresa.as_ref().map(|(k, f)| (k, f)))
        .chain(exp.taxpain.documentos.iter().map(|g| (&g.kind, &g.facts)))
        .filter(|(k, _)| nombra(**k))
        .filter_map(|(k, f)| {
            Some(Admin {
                texto: f.administrator.clone()?,
                segun: segun(*k),
            })
        })
        .filter(|a| !a.texto.trim().is_empty())
        .collect();
    out.sort_by(|a, b| a.texto.cmp(&b.texto));
    out.dedup_by(|a, b| a.texto == b.texto);
    out.extend(
        exp.taxpain
            .administradores
            .iter()
            .map(|a| Admin {
                texto: format!("{} ({})", a.nombre, a.nif),
                segun: "Nóminas de TaxPain",
            })
            .filter(|a| !a.texto.trim().is_empty()),
    );
    out
}

/// Palabras de un nombre sin la forma social, de `min` letras o más.
fn sin_forma(s: &str, min: usize) -> Vec<String> {
    const FORMAS: [&str; 7] = [
        "SOCIEDAD",
        "LIMITADA",
        "ANONIMA",
        "UNIPERSONAL",
        "PROFESIONAL",
        "SLU",
        "SAU",
    ];
    super::cruces::socios::palabras(s)
        .into_iter()
        .filter(|w| w.chars().count() >= min && !FORMAS.contains(&w.as_str()))
        .collect()
}

/// ¿Es el nombre de la empresa? Si comparten alguna palabra que no sea la forma social (de cuatro
/// letras o más; o una empieza por la otra, con cinco o más: «Servicio» y «Servicios»). Sin
/// el nombre de la empresa, no se sabe y se da por bueno.
fn es_la_empresa(nombre: &str, empresa: &str) -> bool {
    let suyas = sin_forma(empresa, 4);
    if suyas.is_empty() {
        return true;
    }
    sin_forma(nombre, 4).iter().any(|w| {
        suyas.iter().any(|e| {
            w == e
                || (w.len().min(e.len()) >= 5
                    && (w.starts_with(e.as_str()) || e.starts_with(w.as_str())))
        })
    })
}

/// ¿Está el nombre entero en el de la empresa («Pérez Gómez» en «Pérez Gómez Consultores SL»)?
/// Entonces es la empresa aunque sea también el de su administrador; «Juan Pérez Gómez», no.
fn entero_en_la_empresa(nombre: &str, empresa: &str) -> bool {
    let suyas = sin_forma(empresa, 3);
    let p = sin_forma(nombre, 3);
    !p.is_empty() && p.iter().all(|w| suyas.contains(w))
}

/// ¿Lleva el nombre una forma social («Proveedora Ejemplo, S.L.», «Example Ltd»)? Es otra
/// sociedad, no un nombre comercial de la empresa.
fn con_forma_social(nombre: &str) -> bool {
    const FORMAS: [&str; 17] = [
        "sl", "slu", "slp", "sll", "sa", "sau", "scp", "cb", "sociedad", "ltd", "limited", "gmbh",
        "inc", "llc", "bv", "sas", "srl",
    ];
    crate::documentos::fold(nombre)
        .replace('.', "")
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| FORMAS.contains(&w))
}

/// Facturas recibidas a nombre de otra persona o empresa, o del administrador. Sin el NIF del
/// destinatario, por su nombre: si es el de la empresa no pasa nada, y si no es el del
/// administrador sólo cuenta si es el de otra sociedad (con su forma social; otro nombre puede
/// ser el comercial de la empresa). Sin ninguno de los dos no se dice nada (puede ser una
/// factura simplificada, que no lleva los datos del destinatario).
fn a_nombre_de_otro(exp: &Expediente, s: &mut Salida) {
    if exp.own_nif.is_none() {
        return;
    }
    let admins = administradores(exp);
    let mut personas = Vec::new();
    let mut filas = Vec::new();
    let mut del_admin = Vec::new();
    for (d, f) in exp.recibidas() {
        let dest = f
            .destinatario_nif
            .as_deref()
            .filter(|n| !super::expediente::nif_clave(n).is_empty());
        let nombre = f
            .destinatario_nombre
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty());
        match (dest, nombre) {
            (Some(n), _) if exp.es_propio(n) => continue,
            (None, None) => continue,
            _ => {}
        }
        // Del administrador: por su NIF o, sin él, por dos palabras o más de su nombre. Se mira
        // antes que el nombre de la empresa, que puede llevar sus apellidos («Pérez Gómez
        // Consultores SL»): entonces la factura es de la empresa sólo si su nombre está entero en
        // el de la empresa.
        let suyo = admins.iter().find(|a| {
            let n = nombre
                .filter(|n| super::cruces::socios::palabras(n).len() >= 2)
                .unwrap_or_default();
            super::cruces::socios::es_persona(&a.texto, n, dest)
        });
        if let (None, Some(n)) = (dest, nombre)
            && es_la_empresa(n, &exp.empresa)
            && (suyo.is_none() || entero_en_la_empresa(n, &exp.empresa))
        {
            continue;
        }
        let fila = vec![
            d.ruta.clone(),
            quien(f),
            num(f),
            opt_fecha(f),
            match (nombre, dest) {
                (Some(n), Some(nif)) => format!("{n} ({nif})"),
                (Some(n), None) => n.to_owned(),
                (None, Some(nif)) => nif.to_owned(),
                (None, None) => String::new(),
            },
            opt_eur(f.total),
        ];
        if let Some(a) = suyo {
            let quien = super::cruces::socios::sin_cargo(&a.texto).to_owned();
            del_admin.push(((quien, a.segun), fila));
            continue;
        }
        if dest.is_none() && !nombre.is_some_and(con_forma_social) {
            continue;
        }
        let persona = dest
            .is_some_and(|dest| matches!(nif_español(dest), Some(Ok(n)) if n.is_persona_fisica()));
        if persona {
            personas.push(d.ruta.clone());
        }
        filas.push(fila);
    }
    a_nombre_del_administrador(del_admin, s);
    if filas.is_empty() {
        return;
    }
    let ficheros: Vec<String> = filas.iter().map(|f| f[0].clone()).collect();
    let mut detalle = String::from(
        "El IVA de una factura que no está a nombre de la sociedad no se puede deducir (art. 97 \
         LIVA), y el gasto puede no ser deducible en el Impuesto sobre Sociedades. Si el gasto \
         es de la empresa, pide la factura a su nombre.",
    );
    if !personas.is_empty() {
        detalle.push_str(&format!(
            " {} a nombre de una persona física (un DNI): si es un gasto personal, no debería \
             estar en la contabilidad de la empresa.",
            if personas.len() == 1 {
                "Una está".to_owned()
            } else {
                format!("{} están", personas.len())
            }
        ));
    }
    s.add(
        Nuevo {
            codigo: "facturas.a_nombre_de_otro",
            area: Area::Facturas,
            gravedad: Gravedad::Aviso,
            responsable: Responsable::Empresa,
            titulo: format!(
                "{} a nombre de otro",
                plural(filas.len(), "factura recibida", "facturas recibidas")
            ),
            ficheros,
        }
        .detalle(detalle),
        None,
        None,
    );
    s.tabla(
        "Facturas recibidas a nombre de otro",
        "A nombre de otro",
        &[
            "Fichero",
            "Emisor",
            "Número",
            "Fecha",
            "A nombre de",
            "Total",
        ],
        filas,
    );
}

/// Facturas recibidas a nombre del administrador (por su NIF o su nombre, según las escrituras,
/// el Registro, las actas o Nóminas), con el nombre del administrador y de dónde sale.
fn a_nombre_del_administrador(
    del_admin: Vec<((String, &'static str), Vec<String>)>,
    s: &mut Salida,
) {
    if del_admin.is_empty() {
        return;
    }
    let mut quienes: Vec<(String, &'static str)> =
        del_admin.iter().map(|(a, _)| a.clone()).collect();
    quienes.sort();
    quienes.dedup();
    let quienes = match quienes.as_slice() {
        [(nombre, segun)] => format!("{nombre} (el administrador según {segun})"),
        _ => {
            let l: Vec<String> = quienes
                .iter()
                .map(|(nombre, segun)| format!("{nombre} (según {segun})"))
                .collect();
            format!("los administradores {}", super::lista(&l, 3))
        }
    };
    let filas: Vec<Vec<String>> = del_admin.into_iter().map(|(_, f)| f).collect();
    let ficheros: Vec<String> = filas.iter().map(|f| f[0].clone()).collect();
    s.add(
        Nuevo {
            codigo: "facturas.a_nombre_del_administrador",
            area: Area::Facturas,
            gravedad: Gravedad::Aviso,
            responsable: Responsable::Empresa,
            titulo: format!(
                "{} a nombre del administrador",
                plural(filas.len(), "factura recibida", "facturas recibidas")
            ),
            ficheros,
        }
        .detalle(format!(
            "{} a nombre de {quienes}, no de la sociedad: su IVA no se puede deducir (art. 97 \
             LIVA) y el gasto puede no ser deducible en el Impuesto sobre Sociedades. Si el gasto \
             es de la empresa, pide la factura a su nombre; si es personal, no debería estar en \
             la contabilidad de la empresa.",
            if filas.len() == 1 { "Está" } else { "Están" },
        )),
        None,
        None,
    );
    s.tabla(
        "Facturas recibidas a nombre del administrador",
        "A nombre del administrador",
        &[
            "Fichero",
            "Emisor",
            "Número",
            "Fecha",
            "A nombre de",
            "Total",
        ],
        filas,
    );
}

/// Facturas de un mismo proveedor, para las tablas por proveedor.
#[derive(Debug, Default)]
struct PorProveedor {
    nombre: String,
    nif: String,
    facturas: usize,
    tipos: BTreeSet<i64>,
    importe: Money,
}

/// Proveedores de fuera de España que cobran IVA (español, por la ventanilla única, o el de su
/// país). Sin NIF, se sabe que son de fuera por su país, su moneda o su forma social (Inc.,
/// LLC, Ltd.…). Las facturas a la empresa (con su NIF, a su nombre o que la gestoría lleva en el
/// libro como inversión del sujeto pasivo) deberían ir sin IVA, con inversión del sujeto pasivo:
/// hay que pedirlas corregidas. Las demás pueden ser, para el proveedor, ventas a un particular:
/// su IVA no se puede deducir.
/// Proveedor de fuera con IVA: (no es a la empresa, nombre, moneda).
type ClaveExtranjero = (bool, String, Option<String>);

/// Los PDF de recibidas que la gestoría lleva en el libro como inversión del sujeto pasivo (la
/// línea, autorrepercutida: el total sin la cuota).
fn en_libro_como_isp(exp: &Expediente) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for c in super::registro::casados(exp)
        .iter()
        .filter(|c| c.libro.kind == crate::libros::BookKind::Recibidas)
    {
        for (n, i) in c.lineas.iter().enumerate() {
            if i.vat_in_total == Some(false)
                && let Some((d, _)) = c.pdf(n)
            {
                out.insert(d.ruta.clone());
            }
        }
    }
    out
}

/// «1 factura de un proveedor de fuera de España», «3 facturas de proveedores de fuera de
/// España».
pub(crate) fn de_fuera(n: usize) -> String {
    if n == 1 {
        "1 factura de un proveedor de fuera de España".to_owned()
    } else {
        format!("{n} facturas de proveedores de fuera de España")
    }
}

/// Hotel, restaurante, entrada o inmueble de otro país: llevan el IVA de allí.
const DE_ALLI: &str = "un hotel, un restaurante, una entrada o un servicio ligado a un inmueble \
                       de ese país lleva el IVA de allí aunque se le dé el NIF-IVA (art. 47, 53 \
                       y 55 de la Directiva 2006/112/CE)";

fn extranjeros_con_iva(exp: &Expediente, s: &mut Salida) {
    let como_isp = en_libro_como_isp(exp);
    let mut grupos: BTreeMap<ClaveExtranjero, PorProveedor> = BTreeMap::new();
    let mut empresa = Vec::new();
    let mut particular = Vec::new();
    for (d, f) in exp.recibidas() {
        if !f.emisor_extranjero() || f.emisor_nif.as_deref().is_some_and(|n| exp.es_propio(n)) {
            continue;
        }
        let iva = f.iva.unwrap_or(Money::ZERO);
        if iva.cents() <= 0 {
            continue;
        }
        let a_la_empresa = f
            .destinatario_nif
            .as_deref()
            .is_some_and(|n| exp.es_propio(n))
            || f.destinatario_nombre
                .as_deref()
                .is_some_and(|n| !exp.empresa.is_empty() && nombres_parecidos(n, &exp.empresa))
            || como_isp.contains(&d.ruta);
        let g = grupos
            .entry((!a_la_empresa, quien(f).to_lowercase(), f.moneda.clone()))
            .or_insert_with(|| PorProveedor {
                nombre: quien(f),
                nif: f
                    .emisor_nif
                    .clone()
                    .or_else(|| f.emisor_pais.clone())
                    .unwrap_or_default(),
                ..PorProveedor::default()
            });
        g.facturas += 1;
        g.tipos.extend(f.tipo());
        g.importe += iva;
        if a_la_empresa {
            empresa.push(d.ruta.clone());
        } else {
            particular.push(d.ruta.clone());
        }
    }
    if grupos.is_empty() {
        return;
    }
    let proveedores = |b2c: bool| {
        let n = grupos.keys().filter(|(p, _, _)| *p == b2c).count();
        if n == 1 {
            "1 proveedor de otro país cobra".to_owned()
        } else {
            format!("{n} proveedores de otros países cobran")
        }
    };
    if !empresa.is_empty() {
        s.add(
            Nuevo {
                codigo: "facturas.extranjero_con_iva",
                area: Area::Facturas,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Empresa,
                titulo: format!("{} con IVA", de_fuera(empresa.len())),
                ficheros: empresa,
            }
            .detalle(format!(
                "{} IVA en facturas a la empresa (con su NIF, a su nombre o que la gestoría lleva \
                 en el libro como inversión del sujeto pasivo). Entre empresas no deberían \
                 cobrarlo: es inversión del sujeto pasivo (art. 84.Uno.2.º LIVA) y el IVA lo \
                 autorrepercute la empresa; el que cobran no se puede deducir en el 303 (art. \
                 94.Tres LIVA). Suele pasar porque no tienen el NIF-IVA de la empresa (o no \
                 está dada de alta en el ROI): dáselo en la cuenta de cada proveedor y pide las \
                 facturas corregidas, sin IVA. Pero {DE_ALLI}; si es de otro país de la UE, se \
                 puede pedir con el modelo 360 (art. 117 bis LIVA).",
                proveedores(false)
            )),
            None,
            None,
        );
    }
    if !particular.is_empty() {
        s.add(
            Nuevo {
                codigo: "facturas.extranjero_con_iva_particular",
                area: Area::Facturas,
                gravedad: Gravedad::Info,
                responsable: Responsable::Empresa,
                titulo: format!(
                    "{} con IVA, sin el NIF ni el nombre de la empresa",
                    de_fuera(particular.len())
                ),
                ficheros: particular,
            }
            .detalle(format!(
                "{} IVA en facturas que no llevan el NIF ni el nombre de la empresa. Para el \
                 proveedor puede ser una venta a un particular, con el IVA bien cobrado, pero \
                 ese IVA no se puede deducir (art. 97 LIVA). Mira si son gastos de la empresa: \
                 si lo son, comprueba que el proveedor tiene el NIF-IVA de la empresa, y con él \
                 las siguientes irán sin IVA (inversión del sujeto pasivo); {DE_ALLI}. Si no son \
                 gastos de la empresa, no deberían estar en su carpeta.",
                proveedores(true)
            )),
            None,
            None,
        );
    }
    let mut grupos: Vec<(ClaveExtranjero, PorProveedor)> = grupos.into_iter().collect();
    grupos.sort_by_key(|a| (a.0.0, std::cmp::Reverse(a.1.importe)));
    let filas = grupos
        .iter()
        .map(|((b2c, _, moneda), g)| {
            vec![
                g.nombre.clone(),
                g.nif.clone(),
                g.facturas.to_string(),
                g.tipos
                    .iter()
                    .map(|t| format!("{t} %"))
                    .collect::<Vec<_>>()
                    .join(", "),
                importe(g.importe, moneda.as_deref()),
                if *b2c { "No" } else { "Sí" }.to_owned(),
            ]
        })
        .collect();
    s.tabla(
        "Proveedores de fuera de España que cobran IVA",
        "Extranjeros con IVA",
        &[
            "Proveedor",
            "NIF-IVA o país",
            "Facturas",
            "Tipo",
            "IVA",
            "A la empresa",
        ],
        filas,
    );
}

/// Facturas en otra moneda: hay que pasarlas a euros al registrarlas.
fn divisas(exp: &Expediente, s: &mut Salida) {
    let mut grupos: BTreeMap<(String, String), PorProveedor> = BTreeMap::new();
    let mut por_moneda: BTreeMap<String, usize> = BTreeMap::new();
    let mut ficheros = Vec::new();
    for (d, f) in exp.facturas() {
        let Some(m) = f.moneda.as_deref() else {
            continue;
        };
        *por_moneda.entry(m.to_owned()).or_default() += 1;
        let g = grupos
            .entry((quien(f).to_lowercase(), m.to_owned()))
            .or_insert_with(|| PorProveedor {
                nombre: quien(f),
                ..PorProveedor::default()
            });
        g.facturas += 1;
        g.importe += f.total.unwrap_or(Money::ZERO);
        ficheros.push(d.ruta.clone());
    }
    if ficheros.is_empty() {
        return;
    }
    let mut grupos: Vec<((String, String), PorProveedor)> = grupos.into_iter().collect();
    grupos.sort_by(|a, b| b.1.facturas.cmp(&a.1.facturas).then(a.0.cmp(&b.0)));
    let filas = grupos
        .iter()
        .map(|((_, m), g)| {
            vec![
                g.nombre.clone(),
                m.clone(),
                g.facturas.to_string(),
                importe(g.importe, Some(m)),
            ]
        })
        .collect();
    let cuantas = por_moneda
        .iter()
        .map(|(m, n)| format!("{n} en {m}"))
        .collect::<Vec<_>>()
        .join(", ");
    s.add(
        Nuevo {
            codigo: "facturas.divisas",
            area: Area::Facturas,
            gravedad: Gravedad::Info,
            responsable: Responsable::Empresa,
            titulo: format!(
                "{} en otra moneda",
                plural(ficheros.len(), "factura", "facturas")
            ),
            ficheros,
        }
        .detalle(format!(
            "{cuantas}. Se registran en euros con el tipo de cambio del devengo (art. 79.Once \
             LIVA); la diferencia con lo que cobra el banco es una diferencia de cambio. En esta \
             revisión no se suman con las de euros."
        )),
        None,
        None,
    );
    s.tabla(
        "Facturas en otra moneda, por emisor",
        "Otra moneda",
        &["Emisor", "Moneda", "Facturas", "Total"],
        filas,
    );
}

/// Facturas guardadas en la carpeta de otro trimestre (o de otro año).
fn trimestre_de_la_carpeta(exp: &Expediente, s: &mut Salida) {
    let mut filas = Vec::new();
    for (d, f) in exp.facturas() {
        if !matches!(d.clase, Clase::FacturaRecibida | Clase::FacturaEmitida) {
            continue;
        }
        let (Some(fe), Some(año)) = (f.fecha, d.ruta_año) else {
            continue;
        };
        let mal_año = fe.year() != año;
        let mal_trimestre = d.ruta_trimestre.is_some_and(|q| q != trimestre(fe));
        if !(mal_año || mal_trimestre) {
            continue;
        }
        let carpeta = match d.ruta_trimestre {
            Some(q) => format!("{q}T {año}"),
            None => año.to_string(),
        };
        filas.push(vec![
            d.ruta.clone(),
            quien(f),
            num(f),
            fecha(fe),
            format!("{}T {}", trimestre(fe), fe.year()),
            carpeta,
        ]);
    }
    if filas.is_empty() {
        return;
    }
    let ficheros: Vec<String> = filas.iter().map(|f| f[0].clone()).collect();
    s.add(
        Nuevo {
            codigo: "facturas.otro_trimestre",
            area: Area::Facturas,
            gravedad: Gravedad::Info,
            responsable: Responsable::Empresa,
            titulo: format!(
                "{} en la carpeta de otro periodo",
                plural(filas.len(), "factura", "facturas")
            ),
            ficheros,
        }
        .detalle(
            "La fecha de la factura no es la del trimestre o el año de la carpeta donde está. \
             Si la gestoría trabaja por carpetas, puede haberla declarado en el trimestre \
             equivocado (o no haberla declarado)."
                .into(),
        ),
        None,
        None,
    );
    s.tabla(
        "Facturas en la carpeta de otro periodo",
        "En otro periodo",
        &[
            "Fichero",
            "Emisor",
            "Número",
            "Fecha",
            "Su trimestre",
            "Carpeta",
        ],
        filas,
    );
}
