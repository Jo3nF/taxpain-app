//! Resúmenes anuales por NIF frente a las facturas de la carpeta: el 347 (operaciones con
//! terceros de más de 3.005,06 €), el 349 (operaciones intracomunitarias), el 190
//! (retenciones de profesionales) y el 180 (retenciones de alquileres).

use std::collections::BTreeMap;

use taxpain_core::money::Money;
use time::Date;

use super::super::expediente::{Factura, Registro, nif_clave, nif_español};
use super::super::modelos::{Declarado, de_la_ue, retencion_de_alquiler, trimestre_de, unicas};
use super::super::{
    Area, Destinatario, Gravedad, Nuevo, Responsable, Salida, UN_EURO, eur, lista, plural,
};
use super::{Contexto, nombre, primera};

/// Umbral del 347: operaciones con un tercero de más de 3.005,06 € en el año (art. 33.1 RGAT).
const UMBRAL_347: Money = Money::from_cents(300_506);

pub(super) fn revisar(cx: &Contexto<'_>, s: &mut Salida) {
    m347(cx, s);
    m349(cx, s);
    perceptores(cx, s);
}

/// Importe y nombre por NIF.
type PorNif = BTreeMap<String, (Money, String)>;

fn sumar(m: &mut PorNif, nif: &str, nombre: Option<&str>, importe: Money) {
    let e = m
        .entry(nif_clave(nif))
        .or_insert((Money::ZERO, String::new()));
    e.0 += importe;
    if e.1.is_empty()
        && let Some(n) = nombre
    {
        n.trim().clone_into(&mut e.1);
    }
}

/// Lo declarado por NIF (los registros sin NIF no se pueden cruzar).
fn declarado(
    registros: &[Registro],
    filtro: impl Fn(&Registro) -> bool,
    importe: impl Fn(&Registro) -> Option<Money>,
) -> PorNif {
    let mut m = PorNif::new();
    for r in registros.iter().filter(|r| filtro(r)) {
        if let (Some(nif), Some(i)) = (&r.nif, importe(r)) {
            sumar(&mut m, nif, r.nombre.as_deref(), i);
        }
    }
    m
}

/// Total de una factura con IVA (o base más IVA si no se leyó el total).
fn total(f: &Factura) -> Option<Money> {
    f.total
        .or_else(|| f.base.map(|b| b + f.iva.unwrap_or(Money::ZERO)))
}

/// Lo que sale del cruce de un modelo por NIF.
#[derive(Default)]
struct Cruce {
    filas: Vec<Vec<String>>,
    /// En la carpeta (por encima del umbral) y no en el modelo.
    faltan: Vec<String>,
    /// El modelo declara menos que las facturas.
    de_menos: Vec<String>,
    /// El modelo declara más que las facturas (o el NIF no tiene facturas en la carpeta).
    sin_facturas: Vec<String>,
}

fn cruzar(etiqueta: &str, dec: &PorNif, car: &PorNif, umbral: Money) -> Cruce {
    let mut c = Cruce::default();
    let mut nifs: Vec<&String> = dec.keys().chain(car.keys()).collect();
    nifs.sort();
    nifs.dedup();
    for nif in nifs {
        let d = dec.get(nif);
        let f = car.get(nif);
        let nombre = d
            .map(|x| x.1.as_str())
            .filter(|n| !n.is_empty())
            .or(f.map(|x| x.1.as_str()))
            .unwrap_or_default();
        let quien = if nombre.is_empty() {
            nif.clone()
        } else {
            format!("{nif} {nombre}")
        };
        let (dm, fm) = (d.map(|x| x.0), f.map(|x| x.0));
        if dm.is_none() && fm.is_none_or(|m| m.abs() <= umbral) {
            continue;
        }
        c.filas.push(vec![
            etiqueta.to_owned(),
            nif.clone(),
            nombre.to_owned(),
            dm.map(eur).unwrap_or_else(|| "no está".into()),
            fm.map(eur).unwrap_or_default(),
            eur(dm.unwrap_or(Money::ZERO) - fm.unwrap_or(Money::ZERO)),
        ]);
        match (dm, fm) {
            (None, Some(f)) => c.faltan.push(format!("{quien} ({})", eur(f))),
            (Some(d), Some(f)) if f - d > UN_EURO => c.de_menos.push(format!(
                "{quien}: declara {} y las facturas suman {}",
                eur(d),
                eur(f)
            )),
            (Some(d), f) if d - f.unwrap_or(Money::ZERO) > UN_EURO => {
                c.sin_facturas.push(format!(
                    "{quien}: declara {} y las facturas de la carpeta suman {}",
                    eur(d),
                    eur(f.unwrap_or(Money::ZERO))
                ));
            }
            _ => {}
        }
    }
    c
}

/// Los textos de un modelo para los avisos del cruce por NIF.
struct Textos<'t> {
    modelo: &'t str,
    /// «el 347 de 2025».
    cual: String,
    /// «tercero», «perceptor» y su plural.
    uno: &'t str,
    quienes: &'t str,
    /// «operaciones», «retenciones».
    que: &'t str,
    codigos: [&'static str; 3],
    /// Facturas del periodo que no se han podido repartir por NIF (ver [`sin_leer`]).
    sin_leer: usize,
}

/// Facturas del periodo reconocidas sólo por las reglas (sin leer con IA) a las que les falta el
/// NIF del otro o el importe: no entran en el cruce por NIF, así que no se puede decir que
/// falten facturas. Las leídas con IA sin NIF (un proveedor de fuera) están leídas.
fn sin_leer(cx: &Contexto<'_>, dentro: impl Fn(Date) -> bool) -> usize {
    let del_periodo = |f: &Factura| f.fecha.is_some_and(&dentro);
    let recibidas = cx.exp.recibidas().filter(|(d, f)| {
        !d.con_ia && del_periodo(f) && (f.emisor_nif.is_none() || total(f).is_none())
    });
    let emitidas = cx.exp.emitidas().filter(|(d, f)| {
        !d.con_ia && del_periodo(f) && (f.destinatario_nif.is_none() || total(f).is_none())
    });
    recibidas.count() + emitidas.count()
}

/// «Hay 3 facturas de ese periodo sin el NIF del otro o sin el importe…».
fn nota_sin_leer(n: usize) -> String {
    format!(
        "Hay {} de ese periodo sin el NIF del otro o sin el importe leídos (reconocidas sólo por \
         las reglas), que no se cuentan: léelas con IA (Importar → Carpeta de la empresa) o con \
         Claude Code (/leer-carpeta) para un cruce completo.",
        plural(n, "factura", "facturas")
    )
}

fn avisos(c: &Cruce, t: &Textos<'_>, d: &Declarado, y: i32, s: &mut Salida) {
    let ficheros = vec![d.fichero.clone()];
    let cual = &t.cual;
    if !c.faltan.is_empty() {
        s.add(
            Nuevo {
                codigo: t.codigos[0],
                area: Area::Modelos,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Gestoria,
                titulo: format!(
                    "{}: {} {}",
                    primera(cual),
                    if c.faltan.len() == 1 {
                        "falta"
                    } else {
                        "faltan"
                    },
                    plural(c.faltan.len(), t.uno, t.quienes)
                ),
                ficheros: ficheros.clone(),
            }
            .detalle(format!(
                "Tienen {} en las facturas de la carpeta y no están en {cual}: {}. Qué hacer: \
                 pide a la gestoría una declaración complementaria con los que falten antes de \
                 que lo detecte Hacienda (los datos que faltan en una declaración informativa se \
                 sancionan, art. 198 LGT).",
                t.que,
                lista(&c.faltan, 10)
            )),
            Some(y),
            None,
        );
        s.pedir(
            Destinatario::Gestoria,
            format!(
                "Complementaria del {} de {y} con: {}",
                t.modelo,
                lista(&c.faltan, 10)
            ),
        );
    }
    if !c.de_menos.is_empty() {
        s.add(
            Nuevo {
                codigo: t.codigos[1],
                area: Area::Modelos,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Gestoria,
                titulo: format!(
                    "{} declara de menos con {}",
                    primera(cual),
                    plural(c.de_menos.len(), t.uno, t.quienes)
                ),
                ficheros: ficheros.clone(),
            }
            .detalle(format!(
                "Las facturas de la carpeta suman más de lo declarado: {}. Qué hacer: pide a la \
                 gestoría que lo revise y, si falta algo, que presente una complementaria.",
                lista(&c.de_menos, 10)
            )),
            Some(y),
            None,
        );
    }
    if !c.sin_facturas.is_empty() {
        s.add(
            Nuevo {
                codigo: t.codigos[2],
                area: Area::Modelos,
                gravedad: Gravedad::Info,
                responsable: Responsable::Documentacion,
                titulo: format!(
                    "{} declara {} sin todas sus facturas en la carpeta",
                    primera(cual),
                    t.que
                ),
                ficheros,
            }
            .detalle(if t.sin_leer > 0 {
                format!(
                    "Declara más de lo que suman las facturas de la carpeta que se han leído: {}. \
                     {} Si después están todas, el modelo declara de más.",
                    lista(&c.sin_facturas, 10),
                    nota_sin_leer(t.sin_leer)
                )
            } else {
                format!(
                    "Declara más de lo que suman las facturas de la carpeta: {}. Lo normal es que \
                     falten facturas en la carpeta (pídeselas a la gestoría o al proveedor); si \
                     están todas, el modelo declara de más.",
                    lista(&c.sin_facturas, 10)
                )
            }),
            Some(y),
            None,
        );
    }
}

/// Facturas únicas del año (o del periodo) recibidas y emitidas.
fn recibidas<'a>(cx: &'a Contexto<'_>, dentro: impl Fn(Date) -> bool) -> Vec<&'a Factura> {
    unicas(
        cx.exp
            .recibidas()
            .filter(|(_, f)| f.fecha.is_some_and(&dentro))
            .map(|(doc, f)| (doc.sha.as_str(), f)),
    )
}

fn emitidas<'a>(cx: &'a Contexto<'_>, dentro: impl Fn(Date) -> bool) -> Vec<&'a Factura> {
    unicas(
        cx.exp
            .emitidas()
            .filter(|(_, f)| f.fecha.is_some_and(&dentro))
            .map(|(doc, f)| (doc.sha.as_str(), f)),
    )
}

fn sin_retencion(f: &Factura) -> bool {
    f.retencion.is_none_or(|r| r.is_zero())
}

/// NIF español válido que no es el de la empresa.
fn español<'f>(cx: &Contexto<'_>, nif: Option<&'f str>) -> Option<&'f str> {
    nif.filter(|n| matches!(nif_español(n), Some(Ok(_))) && !cx.exp.es_propio(n))
}

/// Compras (clave A) y ventas (clave B) del año por NIF, IVA incluido, sin las que llevan
/// retención (van al 190 o al 180) ni las de fuera de España (art. 33.2 RGAT).
fn de_347(cx: &Contexto<'_>, y: i32) -> (PorNif, PorNif) {
    let del_año = |d: Date| d.year() == y;
    let mut compras = PorNif::new();
    for f in recibidas(cx, del_año) {
        if let (Some(nif), Some(t)) = (español(cx, f.emisor_nif.as_deref()), total(f))
            && sin_retencion(f)
        {
            sumar(&mut compras, nif, f.emisor_nombre.as_deref(), t);
        }
    }
    let mut ventas = PorNif::new();
    for f in emitidas(cx, del_año) {
        if let (Some(nif), Some(t)) = (español(cx, f.destinatario_nif.as_deref()), total(f))
            && sin_retencion(f)
        {
            sumar(&mut ventas, nif, f.destinatario_nombre.as_deref(), t);
        }
    }
    (compras, ventas)
}

/// El 347 por NIF frente a las facturas del año.
fn m347(cx: &Contexto<'_>, s: &mut Salida) {
    // Con el SII no se presenta el 347 (art. 32.1.e RGAT).
    if cx
        .exp
        .perfil
        .as_ref()
        .and_then(|p| p.iva)
        .is_some_and(|i| i.sii())
    {
        return;
    }
    let mut filas = Vec::new();
    let mut totales = Vec::new();
    let años: Vec<i32> = cx
        .exp
        .facturas()
        .filter_map(|(_, f)| f.fecha.map(Date::year))
        .chain(cx.de("347").map(|((_, y, _), _)| *y))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .filter(|y| *y >= super::super::revisable_desde(cx.hoy))
        .collect();
    for y in años {
        let (compras, ventas) = de_347(cx, y);
        let por_leer = sin_leer(cx, |d: Date| d.year() == y);
        let Some(d) = cx.modelo("347", y, "0A") else {
            // Sin 347: ¿tocaba? (si el perfil ya lo pide, lo dice «modelos que faltan»).
            let pedido = cx.exp.perfil.as_ref().is_some_and(|p| p.obligado_347);
            let vencido = cx.plazo("347", y, "0A").is_some_and(|p| p.due_on < cx.hoy);
            let mut encima: Vec<String> = compras
                .iter()
                .chain(ventas.iter())
                .filter(|(_, (m, _))| *m > UMBRAL_347)
                .map(|(n, (m, nombre))| format!("{n} {nombre} ({})", eur(*m)))
                .collect();
            encima.dedup();
            if !pedido && vencido && !encima.is_empty() {
                s.add(
                    Nuevo {
                        codigo: "cruces.347_tocaba",
                        area: Area::Modelos,
                        gravedad: Gravedad::Aviso,
                        responsable: Responsable::Documentacion,
                        titulo: format!("Parece que en {y} tocaba presentar el 347"),
                        ficheros: vec![],
                    }
                    .detalle(format!(
                        "Con {} pasan de 3.005,06 € en el año según las facturas de la carpeta: \
                         {}. Pregunta a la gestoría si presentó el 347 de {y} y pídeselo; si no \
                         se presentó, hay que presentarlo cuanto antes.",
                        plural(encima.len(), "tercero", "terceros"),
                        lista(&encima, 8)
                    )),
                    Some(y),
                    None,
                );
            }
            continue;
        };
        // Una complementaria sólo trae lo que faltaba en la anterior: no se cruza.
        if d.parcial {
            continue;
        }
        if d.registros.is_empty() {
            // Sin sus líneas, sus totales frente a los terceros de más de 3.005,06 € de las
            // facturas.
            if !totales_347(d, &compras, &ventas, y, por_leer, &mut totales, s)
                && compras
                    .values()
                    .chain(ventas.values())
                    .any(|(m, _)| *m > UMBRAL_347)
            {
                sin_registros(d, &nombre("347", y, "0A"), y, s);
            }
            continue;
        }
        // Las claves A (compras) y B (ventas); sin clave, se cruza con las dos.
        let es = |c: &'static str| move |r: &Registro| r.clave.as_deref().is_none_or(|k| k == c);
        let importe = |r: &Registro| r.importe;
        let mut c = cruzar(
            "A compras",
            &declarado(&d.registros, es("A"), importe),
            &compras,
            UMBRAL_347,
        );
        let v = cruzar(
            "B ventas",
            &declarado(&d.registros, es("B"), importe),
            &ventas,
            UMBRAL_347,
        );
        c.filas.extend(v.filas);
        c.faltan.extend(v.faltan);
        c.de_menos.extend(v.de_menos);
        c.sin_facturas.extend(v.sin_facturas);
        for f in &mut c.filas {
            f.insert(0, y.to_string());
        }
        filas.append(&mut c.filas);
        avisos(
            &c,
            &Textos {
                modelo: "347",
                cual: nombre("347", y, "0A"),
                uno: "tercero",
                quienes: "terceros",
                que: "operaciones de más de 3.005,06 €",
                codigos: [
                    "cruces.347_falta",
                    "cruces.347_de_menos",
                    "cruces.347_sin_facturas",
                ],
                sin_leer: por_leer,
            },
            d,
            y,
            s,
        );
    }
    s.tabla(
        "347 frente a las facturas, por sus totales",
        "347 por totales",
        &[
            "Ejercicio",
            "Registros del 347",
            "Importe del 347",
            "Terceros de más de 3.005,06 € en las facturas",
            "Importe en las facturas",
            "Diferencia",
        ],
        totales,
    );
    s.tabla(
        "347 por NIF frente a las facturas",
        "347 por NIF",
        &[
            "Ejercicio",
            "Clave",
            "NIF",
            "Nombre",
            "347",
            "Facturas",
            "Diferencia",
        ],
        filas,
    );
}

/// Un 347 leído sin sus líneas por NIF, pero con sus totales (número de declarados e importe):
/// se comparan con los terceros de más de 3.005,06 € de las facturas de la carpeta. Si no
/// cuadran, una nota (puede faltar alguna factura en la carpeta). Devuelve si se ha podido
/// comparar.
fn totales_347(
    d: &Declarado,
    compras: &PorNif,
    ventas: &PorNif,
    y: i32,
    por_leer: usize,
    filas: &mut Vec<Vec<String>>,
    s: &mut Salida,
) -> bool {
    let (Some(n), Some(importe)) = (d.num_registros, d.casillas.get(&2).copied()) else {
        return false;
    };
    if !d.con_casillas() {
        return false;
    }
    let encima: Vec<Money> = compras
        .values()
        .chain(ventas.values())
        .map(|(m, _)| *m)
        .filter(|m| *m > UMBRAL_347)
        .collect();
    let suma = encima.iter().fold(Money::ZERO, |a, m| a + *m);
    let n_facturas = u32::try_from(encima.len()).unwrap_or(u32::MAX);
    let diferencia = importe - suma;
    filas.push(vec![
        y.to_string(),
        n.to_string(),
        eur(importe),
        n_facturas.to_string(),
        eur(suma),
        eur(diferencia),
    ]);
    if n == n_facturas && diferencia.abs() <= UN_EURO {
        return true;
    }
    let cual = nombre("347", y, "0A");
    s.add(
        Nuevo {
            codigo: "cruces.347_totales",
            area: Area::Modelos,
            gravedad: Gravedad::Info,
            responsable: Responsable::Documentacion,
            titulo: format!(
                "{} no cuadra con las facturas de la carpeta",
                primera(&cual)
            ),
            ficheros: vec![d.fichero.clone()],
        }
        .detalle(format!(
            "Declara {} por {} y en las facturas de la carpeta hay {} de más de 3.005,06 € por \
             {} (diferencia: {}). {} Para cruzarlo NIF a NIF hace falta leer sus líneas (con \
             Claude Code, /leer-carpeta, o con el asistente).",
            plural(
                usize::try_from(n).unwrap_or(usize::MAX),
                "declarado",
                "declarados"
            ),
            eur(importe),
            plural(encima.len(), "tercero", "terceros"),
            eur(suma),
            eur(diferencia),
            if por_leer > 0 {
                nota_sin_leer(por_leer)
            } else {
                "Lo normal es que falten facturas en la carpeta o que la gestoría haya contado \
                 otras operaciones."
                    .to_owned()
            }
        )),
        Some(y),
        None,
    );
    true
}

/// Un resumen anual leído sin sus líneas: no se puede cruzar por NIF.
fn sin_registros(d: &Declarado, cual: &str, y: i32, s: &mut Salida) {
    s.add(
        Nuevo {
            codigo: "cruces.sin_registros",
            area: Area::Modelos,
            gravedad: Gravedad::Info,
            responsable: Responsable::Empresa,
            titulo: format!("{} no se ha leído por NIF", primera(cual)),
            ficheros: vec![d.fichero.clone()],
        }
        .detalle(format!(
            "Para cruzar {cual} con las facturas hace falta leer sus líneas (un NIF por línea). \
             Léelo con Claude Code (/leer-carpeta) o con el asistente y vuelve a revisar."
        )),
        Some(y),
        None,
    );
}

/// ¿Está la fecha en el periodo del 349 («1T», «03» o «0A»)?
fn en_periodo(y: i32, p: &str, d: Date) -> bool {
    if d.year() != y {
        return false;
    }
    if let Some(q) = trimestre_de(p) {
        return super::super::trimestre(d) == q;
    }
    match p.parse::<u8>() {
        Ok(0) | Err(_) => p == "0A",
        Ok(m) => u8::from(d.month()) == m,
    }
}

/// Claves del 349 de las ventas (E entregas, T triangulares, S servicios prestados…) y de las
/// compras (A adquisiciones, I servicios adquiridos).
fn clave_de_venta(c: &str) -> bool {
    !matches!(c, "A" | "I")
}

/// El 349 por NIF frente a las facturas sin IVA con empresas de la UE del periodo.
fn m349(cx: &Contexto<'_>, s: &mut Salida) {
    let mut filas = Vec::new();
    for ((_, y, p), d) in cx.de("349") {
        let (y, p) = (*y, p.as_str());
        let dentro = |f: Date| en_periodo(y, p, f);
        let sin_iva = |f: &Factura| f.iva.is_none_or(Money::is_zero);
        let mut compras = PorNif::new();
        for f in recibidas(cx, dentro) {
            if let (Some(n), Some(b)) = (f.emisor_nif.as_deref(), f.base.or(f.total))
                && de_la_ue(n)
                && sin_iva(f)
            {
                sumar(&mut compras, n, f.emisor_nombre.as_deref(), b);
            }
        }
        let mut ventas = PorNif::new();
        for f in emitidas(cx, dentro) {
            if let (Some(n), Some(b)) = (f.destinatario_nif.as_deref(), f.base.or(f.total))
                && de_la_ue(n)
                && sin_iva(f)
            {
                sumar(&mut ventas, n, f.destinatario_nombre.as_deref(), b);
            }
        }
        if d.parcial {
            continue;
        }
        if d.registros.is_empty() {
            if !compras.is_empty() || !ventas.is_empty() {
                sin_registros(d, &nombre("349", y, p), y, s);
            }
            continue;
        }
        let importe = |r: &Registro| r.importe;
        let mut c = cruzar(
            "Compras (A, I)",
            &declarado(
                &d.registros,
                |r| r.clave.as_deref().is_none_or(|k| !clave_de_venta(k)),
                importe,
            ),
            &compras,
            Money::ZERO,
        );
        let v = cruzar(
            "Ventas (E, S, T)",
            &declarado(
                &d.registros,
                |r| r.clave.as_deref().is_none_or(clave_de_venta),
                importe,
            ),
            &ventas,
            Money::ZERO,
        );
        c.filas.extend(v.filas);
        c.faltan.extend(v.faltan);
        c.de_menos.extend(v.de_menos);
        c.sin_facturas.extend(v.sin_facturas);
        for f in &mut c.filas {
            f.insert(0, format!("{p} {y}"));
        }
        filas.append(&mut c.filas);
        avisos(
            &c,
            &Textos {
                modelo: "349",
                cual: nombre("349", y, p),
                uno: "operador",
                quienes: "operadores",
                que: "operaciones intracomunitarias",
                codigos: [
                    "cruces.349_falta",
                    "cruces.349_de_menos",
                    "cruces.349_sin_facturas",
                ],
                sin_leer: sin_leer(cx, dentro),
            },
            d,
            y,
            s,
        );
    }
    s.tabla(
        "349 por NIF frente a las facturas",
        "349 por NIF",
        &[
            "Periodo",
            "Operaciones",
            "NIF",
            "Nombre",
            "349",
            "Facturas",
            "Diferencia",
        ],
        filas,
    );
}

/// Perceptores del 190 (profesionales) y del 180 (alquileres) frente a las retenciones de las
/// facturas recibidas del año.
fn perceptores(cx: &Contexto<'_>, s: &mut Salida) {
    let mut filas = Vec::new();
    for (modelo, alquiler, que) in [
        ("190", false, "retenciones de profesionales"),
        ("180", true, "retenciones de alquileres"),
    ] {
        for ((_, y, _), d) in cx.de(modelo) {
            let y = *y;
            let mut car = PorNif::new();
            for f in recibidas(cx, |f| f.year() == y) {
                if let (Some(n), Some(r)) = (f.emisor_nif.as_deref(), f.retencion)
                    && r.cents() > 0
                    && retencion_de_alquiler(f) == alquiler
                {
                    sumar(&mut car, n, f.emisor_nombre.as_deref(), r);
                }
            }
            if d.parcial {
                continue;
            }
            if !d.registros.iter().any(|r| r.retencion.is_some()) {
                if !car.is_empty() {
                    sin_registros(d, &nombre(modelo, y, "0A"), y, s);
                }
                continue;
            }
            // En el 190 sólo los profesionales (clave G) se cruzan con facturas; sin clave, todos.
            let dec = declarado(
                &d.registros,
                |r| alquiler || r.clave.as_deref().is_none_or(|k| k.starts_with('G')),
                |r| r.retencion,
            );
            let mut c = cruzar(modelo, &dec, &car, Money::ZERO);
            for f in &mut c.filas {
                f.insert(0, y.to_string());
            }
            filas.append(&mut c.filas);
            avisos(
                &c,
                &Textos {
                    modelo,
                    cual: nombre(modelo, y, "0A"),
                    uno: "perceptor",
                    quienes: "perceptores",
                    que,
                    codigos: if alquiler {
                        [
                            "cruces.180_falta",
                            "cruces.180_de_menos",
                            "cruces.180_sin_facturas",
                        ]
                    } else {
                        [
                            "cruces.190_falta",
                            "cruces.190_de_menos",
                            "cruces.190_sin_facturas",
                        ]
                    },
                    sin_leer: sin_leer(cx, |f: Date| f.year() == y),
                },
                d,
                y,
                s,
            );
        }
    }
    s.tabla(
        "190 y 180 por perceptor frente a las facturas",
        "190 y 180 por perceptor",
        &[
            "Ejercicio",
            "Modelo",
            "NIF",
            "Nombre",
            "Retenciones declaradas",
            "Retenciones de las facturas",
            "Diferencia",
        ],
        filas,
    );
}
