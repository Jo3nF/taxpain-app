//! Modelos presentados: los que tocaban y no están, el 303 frente a los libros registro y el
//! diario del trimestre (o frente a sus facturas si no hay libro), el 349 frente al 303 y el 111
//! y el 115 frente a las retenciones de las facturas.

use std::collections::{BTreeMap, BTreeSet};

use taxpain_core::calendar::{calendar_for, deadlines_in_year};
use taxpain_core::money::Money;
use time::{Date, Duration, Month, Weekday};

use super::expediente::{
    Doc, Expediente, Factura, Registro, TipoDeclaracion, casillas_que_coinciden, nif_clave,
    nif_español,
};
use super::facturas::de_fuera;
use super::iva::{Libros, Movida, Recibidas, Segun};
use super::{
    Area, Destinatario, Gravedad, Nuevo, Responsable, Salida, UN_EURO, eur, fecha, lista, plural,
    revisable_desde, trimestre,
};

/// Modelos que se buscan en la carpeta (los de las sociedades y autónomos que presenta una
/// gestoría; el depósito de cuentas y la legalización de libros no son modelos de la AEAT).
pub(crate) const MODELOS: [&str; 14] = [
    "303", "390", "111", "190", "115", "180", "123", "193", "130", "131", "349", "347", "200",
    "202",
];

/// Resúmenes informativos: su complementaria sólo trae lo que faltaba en la anterior.
pub(crate) fn informativo(modelo: &str) -> bool {
    matches!(modelo, "180" | "190" | "193" | "347" | "349")
}

/// Un modelo presentado: sus casillas y de dónde salen.
#[derive(Debug, Clone)]
pub(crate) struct Declarado {
    pub casillas: BTreeMap<u32, Money>,
    pub fichero: String,
    /// Fecha de presentación, si se sabe (de la AEAT, del justificante o de la lectura).
    pub presentado: Option<Date>,
    /// Líneas por perceptor o tercero de los resúmenes anuales.
    pub registros: Vec<Registro>,
    /// Días del periodo impositivo del 200: 365 si no se sabe o es un año entero (ver
    /// [`super::expediente::Modelo::dias_periodo`]).
    pub dias: i64,
    /// No se han podido leer sus casillas: cuenta como presentado, pero no entra en los cruces
    /// (sus casillas no son cero, no se saben).
    pub sin_casillas: bool,
    /// Casillas que no se han podido leer (ver [`super::expediente::Modelo::dudosas`]).
    pub dudosas: BTreeSet<u32>,
    /// Es la complementaria de una declaración informativa cuya anterior no está: sus totales
    /// pueden ser sólo lo que faltaba en ella.
    pub parcial: bool,
    /// Informativas: número de registros (perceptores, declarados u operadores), si se sabe.
    pub num_registros: Option<u32>,
    /// Se pidió la devolución (lo dice la portada del justificante). `None` si no se sabe.
    pub devolucion: Option<bool>,
    /// Complementa o sustituye a otra declaración del periodo (lo dice su justificante, por su
    /// tipo o el número de la anterior, la AEAT, la lectura o el nombre del fichero, o se suma a
    /// la anterior) y de qué tipo es (`Normal` si no se sabe).
    pub rectifica: bool,
    pub tipo: TipoDeclaracion,
    /// Si complementa o sustituye a otra: cuándo se presentó la primera del periodo, si se sabe
    /// (la AEAT o otra declaración de la carpeta presentada antes).
    pub original: Option<Date>,
    /// Si complementa o sustituye a otra: las casillas de la primera del periodo (la que se
    /// presentó en `original`), si está. En una rectificativa del 200 que no ingresa nada, el
    /// pago del banco es el de la original.
    pub casillas_original: BTreeMap<u32, Money>,
}

impl Default for Declarado {
    /// Sin casillas ni fecha, de un periodo de un año entero.
    fn default() -> Self {
        Self {
            casillas: BTreeMap::new(),
            fichero: String::new(),
            presentado: None,
            registros: Vec::new(),
            dias: 365,
            sin_casillas: false,
            dudosas: BTreeSet::new(),
            parcial: false,
            num_registros: None,
            devolucion: None,
            rectifica: false,
            tipo: TipoDeclaracion::Normal,
            original: None,
            casillas_original: BTreeMap::new(),
        }
    }
}

impl Declarado {
    pub fn c(&self, n: u32) -> Money {
        self.casillas.get(&n).copied().unwrap_or(Money::ZERO)
    }

    /// ¿Se pueden usar sus importes en un cruce?
    pub fn con_casillas(&self) -> bool {
        !self.sin_casillas && !self.parcial
    }
}

pub(crate) type Clave = (String, i32, String);

/// De dónde sale una declaración, de menos a más fiable para saber cuál es la vigente.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Origen {
    /// Guardada en TaxPain (Importar → Comparar), sin fecha de presentación.
    Comparar,
    Carpeta,
    /// Lo que tiene la AEAT («Comprobar lo presentado»): sólo las vigentes.
    Aeat,
}

struct Candidato {
    origen: Origen,
    orden: usize,
    justificante: Option<String>,
    anterior: Option<String>,
    tipo: TipoDeclaracion,
    modelo: String,
    d: Declarado,
}

/// Modelos de la carpeta, los guardados en TaxPain (Importar → Comparar) y los que tiene la
/// AEAT, por modelo, ejercicio y periodo. Si hay varias declaraciones del mismo periodo, vale
/// la vigente (ver [`vigente`]): la presentada la última (con la misma fecha o sin ella, la de
/// la AEAT); si no, una que ninguna otra complementa ni
/// sustituye (por el justificante anterior que dice cada una) y, entre las que quedan, la
/// presentada más tarde, sin mezclar sus casillas con las de otra declaración (la sustitutiva o
/// la complementaria de una autoliquidación traen el periodo entero: en el 111 la [28], con lo
/// ingresado antes en la [29]). Si la vigente no trae casillas o líneas y otra lectura de la
/// misma declaración sí, se toman de ésa. En los resúmenes informativos, con sus
/// complementarias.
pub(crate) fn declarados(exp: &Expediente) -> BTreeMap<Clave, Declarado> {
    let mut por_clave: BTreeMap<Clave, Vec<Candidato>> = BTreeMap::new();
    let mut orden = 0;
    let mut meter = |clave: Clave, c: Candidato| {
        por_clave.entry(clave).or_default().push(c);
    };
    for f in &exp.presentados {
        orden += 1;
        meter(
            (f.model.clone(), f.year, f.period.to_uppercase()),
            Candidato {
                origen: Origen::Comparar,
                orden,
                justificante: None,
                anterior: None,
                tipo: TipoDeclaracion::Normal,
                modelo: f.model.clone(),
                d: Declarado {
                    casillas: f.boxes.iter().map(|(k, m)| (u32::from(*k), *m)).collect(),
                    fichero: f.filename.clone(),
                    ..Declarado::default()
                },
            },
        );
    }
    let carpeta = exp.docs.iter().map(|d| (d, Origen::Carpeta));
    let aeat = exp.aeat.iter().map(|d| (d, Origen::Aeat));
    for (doc, origen) in carpeta.chain(aeat) {
        let Some(m) = &doc.modelo else { continue };
        orden += 1;
        meter(
            (m.modelo.clone(), m.ejercicio, m.periodo.clone()),
            Candidato {
                origen,
                orden,
                justificante: m.justificante.clone(),
                anterior: m.anterior.clone(),
                tipo: m.tipo,
                modelo: m.modelo.clone(),
                d: Declarado {
                    casillas: m.casillas.clone(),
                    fichero: doc.ruta.clone(),
                    presentado: m.presentado,
                    registros: m.registros.clone(),
                    dias: m.dias_periodo(),
                    sin_casillas: m.sin_casillas,
                    dudosas: m.dudosas.clone(),
                    parcial: false,
                    num_registros: m.num_registros,
                    devolucion: m.devolucion,
                    rectifica: false,
                    tipo: m.tipo,
                    original: m.original,
                    casillas_original: BTreeMap::new(),
                },
            },
        );
    }
    por_clave
        .into_iter()
        .filter_map(|(clave, cs)| Some((clave, vigente(cs)?)))
        .collect()
}

/// La declaración vigente de un periodo entre varias (ver [`declarados`]).
fn vigente(cs: Vec<Candidato>) -> Option<Declarado> {
    let sustituidos: BTreeSet<String> = cs.iter().filter_map(|c| c.anterior.clone()).collect();
    let vivos: Vec<&Candidato> = cs
        .iter()
        .filter(|c| {
            c.justificante
                .as_ref()
                .is_none_or(|j| !sustituidos.contains(j))
        })
        .collect();
    let vivos = if vivos.is_empty() {
        cs.iter().collect()
    } else {
        vivos
    };
    let mut base = vivos.iter().copied().max_by_key(|c| {
        (
            c.origen == Origen::Aeat,
            c.d.presentado,
            !c.d.sin_casillas,
            c.origen,
            c.orden,
        )
    })?;
    // La AEAT sólo sabe lo presentado hasta que se consultó («Comprobar lo presentado»): una
    // declaración de la carpeta presentada después (la autoliquidación rectificativa de una
    // original que ya tenía la AEAT) es la vigente. Con la misma fecha o sin ella, la de la AEAT.
    if let Some(b) = base.d.presentado
        && let Some(despues) = vivos
            .iter()
            .copied()
            .filter(|c| c.d.presentado.is_some_and(|p| p > b))
            .max_by_key(|c| (c.d.presentado, !c.d.sin_casillas, c.origen, c.orden))
    {
        base = despues;
    }
    let mut d = base.d.clone();
    // Otra lectura que seguro que es la misma declaración: el mismo justificante o, si alguna no
    // dice su número, la misma fecha de presentación.
    let igual = |c: &Candidato| {
        !std::ptr::eq(c, base)
            && match (&c.justificante, &base.justificante) {
                (Some(a), Some(b)) => a == b,
                _ => matches!(
                    (c.d.presentado, base.d.presentado),
                    (Some(x), Some(y)) if x == y
                ),
            }
    };
    // Si no dice su tipo, el que diga ésa.
    if d.tipo == TipoDeclaracion::Normal
        && let Some(t) = cs
            .iter()
            .filter(|c| igual(c))
            .map(|c| c.tipo)
            .find(|t| *t != TipoDeclaracion::Normal)
    {
        d.tipo = t;
    }
    let rectifica = d.tipo != TipoDeclaracion::Normal || base.anterior.is_some();
    // La primera del periodo: la que dice la AEAT o la de la carpeta presentada antes.
    let primera = || {
        cs.iter()
            .filter_map(|c| c.d.presentado)
            .filter(|p| base.d.presentado.is_none_or(|b| *p < b))
            .chain(base.d.original)
            .min()
    };
    d.rectifica = rectifica;
    if rectifica {
        d.original = primera();
        if let Some(o) = d.original
            && let Some(c) = cs
                .iter()
                .filter(|c| c.d.presentado == Some(o) && !c.d.sin_casillas)
                .max_by_key(|c| (c.origen, c.d.casillas.len(), c.orden))
        {
            d.casillas_original = c.d.casillas.clone();
        }
    }
    // Otra lectura de la misma declaración: el mismo justificante o, si alguna no dice su
    // número, la misma fecha de presentación. Sin la fecha de alguna, la misma salvo que la
    // vigente complemente o sustituya a otra: la que no lo dice puede ser la original.
    let misma = |c: &Candidato| {
        !std::ptr::eq(c, base)
            && match (&c.justificante, &base.justificante) {
                (Some(a), Some(b)) => a == b,
                _ => match (c.d.presentado, base.d.presentado) {
                    (Some(x), Some(y)) => x == y,
                    _ => !rectifica && c.tipo == TipoDeclaracion::Normal && c.anterior.is_none(),
                },
            }
    };
    if d.sin_casillas
        && let Some(o) = vivos
            .iter()
            .filter(|c| misma(c))
            .filter(|c| !c.d.sin_casillas && !c.d.casillas.is_empty())
            .max_by_key(|c| (c.origen, c.orden))
    {
        d.casillas = o.d.casillas.clone();
        d.dudosas = o.d.dudosas.clone();
        d.sin_casillas = false;
        d.num_registros = d.num_registros.or(o.d.num_registros);
    }
    // Las casillas que no cuadraron al leer el justificante (dudosas), de otra lectura de la
    // misma declaración que las tenga (las que no tiene siguen dudosas).
    if !d.dudosas.is_empty()
        && let Some(o) = vivos
            .iter()
            .filter(|c| misma(c))
            .filter(|c| !c.d.sin_casillas && c.d.dudosas.is_empty())
            .filter(|c| d.dudosas.iter().any(|k| c.d.casillas.contains_key(k)))
            .max_by_key(|c| (c.origen, c.orden))
    {
        let dudosas = std::mem::take(&mut d.dudosas);
        for k in dudosas {
            match o.d.casillas.get(&k) {
                Some(v) => {
                    d.casillas.insert(k, *v);
                }
                None => {
                    d.dudosas.insert(k);
                }
            }
        }
    }
    // Las casillas que no trae y sí otra lectura que seguro que es la misma declaración (el
    // justificante no lee el número de perceptores del 111, que sí trae la lectura de la copia de
    // la gestoría): una casilla que falta no es cero. Primero, la lectura más fiable, y sólo si
    // en lo que leen las dos dicen lo mismo.
    if !d.sin_casillas {
        let mut otras: Vec<&Candidato> = vivos
            .iter()
            .copied()
            .filter(|c| igual(c) && !c.d.sin_casillas && c.d.dudosas.is_empty())
            .filter(|c| casillas_que_coinciden(&d.casillas, &c.d.casillas, &d.dudosas))
            .collect();
        otras.sort_by_key(|c| std::cmp::Reverse((c.origen, c.orden)));
        for o in otras {
            for (k, v) in &o.d.casillas {
                if !d.dudosas.contains(k) {
                    d.casillas.entry(*k).or_insert(*v);
                }
            }
        }
    }
    if d.registros.is_empty()
        && let Some(o) = vivos
            .iter()
            .filter(|c| misma(c))
            .filter(|c| !c.d.registros.is_empty())
            .max_by_key(|c| (c.origen, c.orden))
    {
        d.registros = o.d.registros.clone();
    }
    if d.presentado.is_none() {
        d.presentado = vivos
            .iter()
            .filter(|c| misma(c))
            .find_map(|c| c.d.presentado);
    }
    if d.devolucion.is_none() {
        d.devolucion = vivos
            .iter()
            .filter(|c| misma(c))
            .find_map(|c| c.d.devolucion);
    }
    d.dias = vivos
        .iter()
        .filter(|c| misma(c))
        .map(|c| c.d.dias)
        .fold(d.dias, i64::min);
    if informativo(&base.modelo) {
        // Las declaraciones anteriores del periodo: la que la vigente dice complementar o
        // sustituir y las de la carpeta o la AEAT presentadas antes (con fecha: de una lectura
        // sin ella no se sabe si es la anterior o una copia de la vigente).
        let anteriores: Vec<Declarado> = cs
            .iter()
            .filter(|c| c.origen != Origen::Comparar && !misma(c))
            .filter(|c| {
                (c.justificante.is_some() && c.justificante == base.anterior)
                    || matches!(
                        (c.d.presentado, base.d.presentado),
                        (Some(x), Some(y)) if x < y
                    )
            })
            .map(|c| c.d.clone())
            .collect();
        match con_complementarias(anteriores, &d) {
            Some(suma) => {
                d = Declarado {
                    rectifica: true,
                    tipo: d.tipo,
                    devolucion: d.devolucion,
                    original: primera(),
                    ..suma
                };
            }
            // Una complementaria sin su anterior: sus totales son sólo lo que faltaba.
            None => d.parcial = d.tipo == TipoDeclaracion::Complementaria,
        }
    }
    Some(d)
}

/// Junta dos lecturas de la misma declaración (de la misma fecha): las casillas y los registros
/// de la nueva, salvo que no traiga.
fn juntar(viejo: &mut Declarado, nuevo: Declarado) {
    let viejo_ = std::mem::take(viejo);
    let (casillas, sin_casillas, dudosas) = if nuevo.casillas.is_empty() || nuevo.sin_casillas {
        (viejo_.casillas, viejo_.sin_casillas, viejo_.dudosas)
    } else {
        (nuevo.casillas, nuevo.sin_casillas, nuevo.dudosas)
    };
    let registros = if nuevo.registros.is_empty() {
        viejo_.registros
    } else {
        nuevo.registros
    };
    *viejo = Declarado {
        casillas,
        registros,
        sin_casillas,
        dudosas,
        presentado: nuevo.presentado.or(viejo_.presentado),
        fichero: nuevo.fichero,
        dias: nuevo.dias.min(viejo_.dias),
        num_registros: nuevo.num_registros.or(viejo_.num_registros),
        devolucion: nuevo.devolucion.or(viejo_.devolucion),
        rectifica: nuevo.rectifica || viejo_.rectifica,
        tipo: if nuevo.tipo == TipoDeclaracion::Normal {
            viejo_.tipo
        } else {
            nuevo.tipo
        },
        original: nuevo.original.or(viejo_.original),
        casillas_original: if nuevo.casillas_original.is_empty() {
            viejo_.casillas_original
        } else {
            nuevo.casillas_original
        },
        parcial: false,
    };
}

/// Suma a `acc` la complementaria `d` de un resumen informativo: sus casillas y sus registros.
/// Lo que no se sabe de una (casillas sin leer o dudosas) tampoco se sabe de la suma.
fn sumar(acc: &mut Declarado, d: Declarado) {
    for (k, v) in d.casillas {
        *acc.casillas.entry(k).or_insert(Money::ZERO) += v;
    }
    acc.registros.extend(d.registros);
    acc.presentado = d.presentado.or(acc.presentado);
    acc.dias = acc.dias.min(d.dias);
    acc.sin_casillas |= d.sin_casillas;
    acc.dudosas.extend(d.dudosas);
    acc.num_registros = acc.num_registros.zip(d.num_registros).map(|(a, b)| a + b);
}

/// Un resumen informativo con sus declaraciones anteriores del mismo periodo, por orden de
/// presentación, y la vigente al final. La complementaria sólo trae los perceptores o terceros
/// que faltaban y se suma a lo anterior; la sustitutiva lo reemplaza entero (art. 11 de la Orden
/// EHA/3127/2009 en el 190; los demás resúmenes, igual). Sin saber cuál es, se suma si las dos
/// traen registros y no se repite ninguno; si no, reemplaza, como en las autoliquidaciones.
/// `None` si la vigente no se suma a ninguna: vale ella sola.
fn con_complementarias(mut anteriores: Vec<Declarado>, vigente: &Declarado) -> Option<Declarado> {
    anteriores.sort_by_key(|d| d.presentado);
    let mut distintas: Vec<Declarado> = Vec::new();
    for d in anteriores {
        match distintas.last_mut() {
            Some(u) if u.presentado == d.presentado => juntar(u, d),
            _ => distintas.push(d),
        }
    }
    let mut it = distintas.into_iter();
    let mut acc = it.next()?;
    for d in it {
        if complementa(&d, &acc) {
            sumar(&mut acc, d);
        } else {
            acc = d;
        }
    }
    if !complementa(vigente, &acc) {
        return None;
    }
    sumar(&mut acc, vigente.clone());
    Some(acc)
}

/// ¿Trae `d` sólo lo que faltaba en `anterior`?
fn complementa(d: &Declarado, anterior: &Declarado) -> bool {
    let de = |r: &Registro| {
        let quien = r
            .nif
            .as_deref()
            .map(nif_clave)
            .filter(|n| !n.is_empty())
            .or_else(|| r.nombre.as_deref().map(|n| n.trim().to_lowercase()));
        (quien, r.clave.clone())
    };
    match d.tipo {
        TipoDeclaracion::Complementaria => true,
        TipoDeclaracion::Sustitutiva => false,
        TipoDeclaracion::Normal => {
            let antes: BTreeSet<_> = anterior.registros.iter().map(de).collect();
            !d.registros.is_empty()
                && !antes.is_empty()
                && d.registros.iter().all(|r| !antes.contains(&de(r)))
        }
    }
}

pub(crate) fn revisar(exp: &Expediente, hoy: Date, s: &mut Salida) {
    let dec = declarados(exp);
    faltan(exp, &dec, hoy, s);
    m349_mensual(&dec, hoy, s);
    // Las comparaciones, sólo de los ejercicios que aún se pueden revisar y rectificar, y de
    // las declaraciones cuyas casillas se han leído.
    let desde = revisable_desde(hoy);
    let recientes: BTreeMap<Clave, Declarado> = dec
        .into_iter()
        .filter(|((_, y, _), d)| *y >= desde && d.con_casillas())
        .collect();
    m303(exp, &recientes, s);
    m349(&recientes, s);
    m111(exp, &recientes, s);
    m115(exp, &recientes, s);
}

/// Sin carpeta: los modelos que faltan y el 349 frente al 303 (lo que no necesita las facturas).
pub(crate) fn revisar_sin_carpeta(exp: &Expediente, hoy: Date, s: &mut Salida) {
    let dec = declarados(exp);
    faltan(exp, &dec, hoy, s);
    m349_mensual(&dec, hoy, s);
    let desde = revisable_desde(hoy);
    let recientes: BTreeMap<Clave, Declarado> = dec
        .into_iter()
        .filter(|((_, y, _), d)| *y >= desde && d.con_casillas())
        .collect();
    m349(&recientes, s);
}

/// Años de los que hay algo en la carpeta. La autorrepercusión de compras del libro de emitidas
/// también cuenta: son compras del año, aunque no sean ventas.
pub(crate) fn años_con_datos(exp: &Expediente, dec: &BTreeMap<Clave, Declarado>) -> BTreeSet<i32> {
    let mut años: BTreeSet<i32> = dec.keys().map(|(_, y, _)| *y).collect();
    años.extend(exp.facturas().filter_map(|(_, f)| f.fecha.map(Date::year)));
    for d in &exp.docs {
        let lineas = d.libro.iter().flat_map(|l| &l.invoices);
        años.extend(
            lineas
                .chain(&d.autorrepercusion)
                .map(|i| i.issue_date.year()),
        );
    }
    años
}

/// Lo que se sabe de un modelo que tocaba (según el perfil o la AEAT) y no está.
#[derive(Debug, PartialEq, Eq)]
enum Veredicto {
    /// Tocaba y no está: se pide a la gestoría.
    Falta,
    /// Está en la carpeta (lo dice el nombre del fichero), pero no se ha podido leer.
    SinLeer(String),
    /// No se sabe si tocaba: una nota, sin pedirlo.
    NoSeSabe(String),
    /// No tocaba; con la razón, si hay que decirla en la tabla.
    NoTocaba(Option<String>),
}

/// De dónde sale que un modelo tocaba.
#[derive(Clone, Copy)]
enum Fuente<'n> {
    /// Del perfil fiscal.
    Perfil,
    /// De «Comprobar lo presentado»: tocaba y la AEAT no lo tiene.
    Aeat,
    /// De «Comprobar lo presentado»: la AEAT no lo tiene, pero sólo tocaba en ciertos casos.
    AeatEnCiertosCasos(&'n str),
    /// Del perfil fiscal en un ejercicio comprobado: de ese modelo no contestó la AEAT (la
    /// consulta falló, no se hizo o el servicio no la permite).
    PerfilSinAeat,
    /// Del perfil fiscal en un ejercicio comprobado: su plazo acabó después de la comprobación.
    PerfilDespues,
}

/// Indicios de que la empresa paga un alquiler con retención (el 115 trimestral y el 180
/// anual): un contrato de alquiler, facturas recibidas con la retención del alquiler, algún 115
/// o 180 presentado o la obligación en el certificado de situación censal.
struct Alquiler {
    hay: bool,
    /// El último certificado de situación censal leído no tiene la obligación: su fecha.
    censo_sin: Option<Date>,
}

fn indicios_de_alquiler(exp: &Expediente, dec: &BTreeMap<Clave, Declarado>) -> Alquiler {
    use crate::documentos::DocKind;
    let contrato = exp
        .docs
        .iter()
        .any(|d| matches!(&d.empresa, Some((DocKind::ContratoAlquiler, _))))
        || exp
            .taxpain
            .documentos
            .iter()
            .any(|d| d.kind == DocKind::ContratoAlquiler);
    let facturas = exp.recibidas().any(|(_, f)| retencion_de_alquiler(f));
    let modelos = dec.keys().any(|(m, _, _)| m == "115" || m == "180");
    let censo = exp.censo();
    let en_el_censo = censo.is_some_and(|(_, o)| o.modelos_obligados().contains("115"));
    Alquiler {
        hay: contrato || facturas || modelos || en_el_censo,
        censo_sin: censo
            .filter(|(_, o)| !o.obligaciones.is_empty() && !o.modelos_obligados().contains("115"))
            .and_then(|(_, o)| o.fecha),
    }
}

/// Lo que dicen del IVA los certificados de situación censal de la carpeta.
struct CensoIva {
    /// La primera fecha de alta en el IVA (o en una actividad) que dan.
    alta: Option<Date>,
    /// Hay 303 o 390 de periodos de antes de esa alta: estuvo dada de alta antes (y de baja un
    /// tiempo), así que de los periodos de antes no se sabe.
    antes_del_alta: bool,
    /// El certificado más reciente tiene obligaciones periódicas y ninguna es el IVA: su fecha.
    /// Si hay un 303 de un periodo que empieza después, el certificado no está al día (la
    /// empresa volvió a darse de alta en el IVA) y no cuenta.
    sin_iva: Option<Date>,
    /// Final del último periodo con un 303 o un 390 presentado.
    ultimo: Option<Date>,
}

fn censo_iva(exp: &Expediente, dec: &BTreeMap<Clave, Declarado>) -> CensoIva {
    let alta = exp.alta_iva();
    let iva = || dec.keys().filter(|(m, _, _)| m == "303" || m == "390");
    let fines: Vec<Date> = iva()
        .filter_map(|(_, y, p)| fin_de_periodo(*y, p))
        .collect();
    let ultimo_inicio = iva().filter_map(|(_, y, p)| inicio_de_periodo(*y, p)).max();
    CensoIva {
        alta,
        antes_del_alta: alta.is_some_and(|a| fines.iter().any(|f| *f < a)),
        sin_iva: exp
            .censo()
            .filter(|(_, o)| !o.obligaciones.is_empty() && !o.modelos_obligados().contains("303"))
            .and_then(|(_, o)| o.fecha)
            .filter(|f| ultimo_inicio.is_none_or(|i| i <= *f)),
        ultimo: fines.into_iter().max(),
    }
}

impl CensoIva {
    /// Qué se sabe de un 303 o un 390 que no está, por las fechas del censo.
    fn veredicto(&self, y: i32, p: &str) -> Option<Veredicto> {
        let fin = fin_de_periodo(y, p)?;
        if let Some(alta) = self.alta
            && fin < alta
        {
            let f = super::fecha(alta);
            return Some(if self.antes_del_alta {
                Veredicto::NoSeSabe(format!(
                    "es de antes del alta en el IVA que da el certificado de situación censal \
                     ({f}) y hay declaraciones del IVA anteriores, así que estuvo de baja un \
                     tiempo: si en este periodo ya lo estaba, no tocaba"
                ))
            } else {
                Veredicto::NoTocaba(Some(format!(
                    "antes del alta en el IVA ({f}, según el certificado de situación censal)"
                )))
            });
        }
        // Quien se da de baja a mitad de un periodo presenta el de ese periodo: sólo los que
        // empiezan después del certificado no tocaban.
        let sin = self.sin_iva?;
        let f = super::fecha(sin);
        if inicio_de_periodo(y, p).is_some_and(|i| i > sin) {
            return Some(Veredicto::NoTocaba(Some(format!(
                "el certificado de situación censal del {f} no tiene el IVA entre sus \
                 obligaciones"
            ))));
        }
        self.ultimo.is_none_or(|u| fin > u).then(|| {
            Veredicto::NoSeSabe(format!(
                "el certificado de situación censal del {f} ya no tiene el IVA entre sus \
                 obligaciones: si se dio de baja antes de este periodo, no tocaba"
            ))
        })
    }
}

/// Más de 6 millones de cifra de negocios (art. 40.3 LIS): pagos fraccionados por la base del
/// periodo, que se presentan aunque no salga nada a ingresar. `TODO(VERIFY)`: la obligación de
/// presentar el 202 sin importe la dan las instrucciones del modelo, no la LIS.
const SEIS_MILLONES: Money = Money::from_euros(6_000_000, 0);

/// Cuota del IS de un 200 que sirve de base al 202 ([00599]: cuota íntegra menos deducciones,
/// bonificaciones, retenciones e ingresos a cuenta, art. 40.2 LIS). Con una base imponible
/// ([00552]) negativa o cero, la cuota íntegra es cero y la base del 202 no puede salir
/// positiva. Con pérdidas, el PDF tampoco escribe la [00599]: sale de la liquidación, que se ha
/// comprobado al leerlo ([00611] = [00599] − [00601] − [00603] − [00605]). `None` si no se sabe.
fn cuota_200(d: &Declarado) -> Option<Money> {
    const LIQUIDACION: [u32; 6] = [599, 601, 603, 605, 611, 1586];
    if !d.con_casillas() {
        return None;
    }
    if !d.dudosas.contains(&552)
        && let Some(base) = d.casillas.get(&552)
        && *base <= Money::ZERO
    {
        return Some(Money::ZERO);
    }
    if LIQUIDACION.iter().any(|c| d.dudosas.contains(c)) {
        return None;
    }
    if let Some(c) = d.casillas.get(&599) {
        return Some(*c);
    }
    let pagos = d.c(601) + d.c(603) + d.c(605);
    d.casillas.get(&611).map(|c| *c + pagos)
}

/// ¿Tocaba el 202 del periodo `p` de `y`? Si la cuota del último 200 cuyo plazo había vencido
/// el primer día del plazo del pago (el 1 de abril, el de `y` − 2 para el 1P; el 1 de octubre o
/// de diciembre, el de `y` − 1 para el 2P y el 3P) sale positiva, su 18 % (art. 40.2 LIS); o,
/// con más de 6 millones de cifra de negocios, siempre (art. 40.3 LIS).
fn tocaba_202(exp: &Expediente, dec: &BTreeMap<Clave, Declarado>, y: i32, p: &str) -> Veredicto {
    let grande = exp.perfil.as_ref().is_some_and(|p| p.gran_empresa)
        || dec
            .get(&("200".to_owned(), y - 1, "0A".to_owned()))
            .filter(|d| d.con_casillas() && !d.dudosas.contains(&255))
            .and_then(|d| d.casillas.get(&255))
            .is_some_and(|c| *c > SEIS_MILLONES);
    if grande {
        return Veredicto::Falta;
    }
    let de = if p == "1P" { y - 2 } else { y - 1 };
    let Some(d) = dec.get(&("200".to_owned(), de, "0A".to_owned())) else {
        return Veredicto::NoSeSabe(format!(
            "depende de la cuota del Impuesto sobre Sociedades de {de} y su 200 no está"
        ));
    };
    match cuota_200(d) {
        Some(c) if c > Money::ZERO => Veredicto::Falta,
        Some(_) => Veredicto::NoTocaba(None),
        None => Veredicto::NoSeSabe(format!(
            "depende de la cuota del 200 de {de}, que no se ha podido leer"
        )),
    }
}

/// Último día de un periodo («1T», «3P», «0A», «05»).
fn fin_de_periodo(y: i32, p: &str) -> Option<Date> {
    use time::Month;
    let mes = match p.as_bytes() {
        [n @ b'1'..=b'4', b'T'] => (n - b'0') * 3,
        [b'0', b'A'] => 12,
        [b'1', b'P'] => 4,
        [b'2', b'P'] => 10,
        [b'3', b'P'] => 12,
        _ => p.parse::<u8>().ok().filter(|m| (1..=12).contains(m))?,
    };
    let m = Month::try_from(mes).ok()?;
    Date::from_calendar_date(y, m, m.length(y)).ok()
}

/// Primer día de un periodo de IVA («1T», «0A», «05»).
fn inicio_de_periodo(y: i32, p: &str) -> Option<Date> {
    let mes = match p.as_bytes() {
        [n @ b'1'..=b'4', b'T'] => (n - b'0') * 3 - 2,
        [b'0', b'A'] => 1,
        _ => p.parse::<u8>().ok().filter(|m| (1..=12).contains(m))?,
    };
    Date::from_calendar_date(y, time::Month::try_from(mes).ok()?, 1).ok()
}

/// Modelos que son siempre anuales (su periodo es «0A»).
fn anual(modelo: &str) -> bool {
    matches!(modelo, "390" | "190" | "180" | "193" | "347" | "200")
}

/// Modelo, ejercicio y periodo que dice el nombre de un fichero: «Modelo_303_2025_1T_…»,
/// «303 1T 2018 EMPRESA», «MODELO 202 2025-3P», «Modelo_111_3T2024», «2021 MOD 200». Un
/// borrador, una factura, un recibo, el cálculo o la liquidación que prepara la gestoría, lo
/// pendiente de presentar o el pago (el NRC) no son el modelo presentado; si el nombre tiene más
/// de un modelo o de un ejercicio, no se sabe cuál es.
pub(crate) fn modelo_por_nombre(nombre: &str) -> Option<Clave> {
    let stem = nombre.rsplit_once('.').map_or(nombre, |(s, _)| s);
    let bajo = stem.to_lowercase();
    if [
        "borrador",
        "factura",
        "recibo",
        "carta de pago",
        "copia simple",
    ]
    .iter()
    .any(|p| bajo.contains(p))
    {
        return None;
    }
    // Palabras enteras: la autoliquidación sí es el modelo, y el pago fraccionado es el 202.
    let palabras: Vec<&str> = bajo
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    let no_es = [
        "calculo",
        "cálculo",
        "liquidacion",
        "liquidación",
        "pendiente",
        "nrc",
    ];
    if palabras.iter().enumerate().any(|(i, w)| {
        no_es.contains(w) || (*w == "pago" && palabras.get(i + 1) != Some(&"fraccionado"))
    }) {
        return None;
    }
    let es_año =
        |t: &str| t.len() == 4 && t.bytes().all(|b| b.is_ascii_digit()) && t.starts_with("20");
    let periodo = |t: &str| -> Option<String> {
        let t = t.to_uppercase();
        match t.as_bytes() {
            [n @ b'1'..=b'4', b'T'] | [b'T', n @ b'1'..=b'4'] => Some(format!("{}T", *n as char)),
            [n @ b'1'..=b'3', b'P'] => Some(format!("{}P", *n as char)),
            [b'0', b'A'] => Some("0A".to_owned()),
            _ => None,
        }
    };
    let (mut modelos, mut años, mut periodos) = (BTreeSet::new(), BTreeSet::new(), BTreeSet::new());
    for t in stem
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
    {
        if t.len() == 3 && MODELOS.contains(&t) {
            modelos.insert(t.to_owned());
        } else if es_año(t) {
            años.insert(t.to_owned());
        } else if let Some(p) = periodo(t) {
            periodos.insert(p);
        } else if t.len() == 6 {
            // «3T2024», «2024T3», «2P2024».
            let (a, b) = t.split_at(2);
            let (c, d) = t.split_at(4);
            if let (Some(p), true) = (periodo(a), es_año(b)) {
                años.insert(b.to_owned());
                periodos.insert(p);
            } else if let (true, Some(p)) = (es_año(c), periodo(d)) {
                años.insert(c.to_owned());
                periodos.insert(p);
            }
        }
    }
    let modelos: Vec<String> = modelos.into_iter().collect();
    let años: Vec<String> = años.into_iter().collect();
    let ([modelo], [año]) = (modelos.as_slice(), años.as_slice()) else {
        return None;
    };
    let periodo = if anual(modelo) {
        "0A".to_owned()
    } else {
        let ps: Vec<String> = periodos
            .into_iter()
            .filter(|p| (modelo == "202") == p.ends_with('P') && p != "0A")
            .collect();
        match ps.as_slice() {
            [p] => p.clone(),
            _ => return None,
        }
    };
    Some((modelo.clone(), año.parse().ok()?, periodo))
}

/// Ficheros de la carpeta que son un modelo por su nombre y de los que no se ha leído nada (un
/// PDF escaneado, un justificante que no se ha podido leer). Uno con texto que no es de la AEAT
/// (un cálculo o un correo de la gestoría) no prueba que se presentara.
fn por_nombre(exp: &Expediente) -> BTreeMap<Clave, String> {
    use super::expediente::Clase;
    let mut out = BTreeMap::new();
    // El modelo de otra empresa (una participada) no cuenta.
    for d in exp.docs.iter().filter(|d| {
        d.modelo.is_none()
            && !d.texto_no_aeat
            && matches!(d.clase, Clase::SinLeer | Clase::Otro | Clase::Modelo)
            && !d.descripcion.contains("otra empresa (NIF")
    }) {
        let nombre = d.ruta.rsplit('/').next().unwrap_or(&d.ruta);
        if let Some(clave) = modelo_por_nombre(nombre) {
            out.entry(clave).or_insert_with(|| d.ruta.clone());
        }
    }
    out
}

/// Modelos y periodos de `y` que tocaban según el perfil fiscal y cuyo plazo ya acabó, con el
/// último día del plazo.
fn esperados(
    perfil: &taxpain_core::calendar::TaxpayerProfile,
    y: i32,
    hoy: Date,
) -> BTreeMap<(String, String), Date> {
    let mut esperados = BTreeMap::new();
    for año in [y, y + 1] {
        let (h, _) = calendar_for(año, perfil.ccaa, &perfil.festivos_locales);
        for d in deadlines_in_year(perfil, año, &h) {
            let code = d.model.code();
            if d.fiscal_year != y || d.due_on >= hoy || !MODELOS.contains(&code) {
                continue;
            }
            esperados.insert((code.to_owned(), d.period.code()), d.due_on);
        }
    }
    esperados
}

/// «303 (1T, 2T); 390».
fn por_modelo(m: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    m.iter()
        .map(|(m, ps)| {
            if ps.len() == 1 && ps[0] == "0A" {
                m.clone()
            } else {
                format!("{m} ({})", ps.join(", "))
            }
        })
        .collect()
}

/// «2022 a 2026» si son seguidos; si no, «2022, 2024, 2025».
pub(crate) fn años_texto(años: &BTreeSet<i32>) -> String {
    let v: Vec<i32> = años.iter().copied().collect();
    match v.as_slice() {
        [] => String::new(),
        [a] => a.to_string(),
        [a, .., b] if usize::try_from(b - a).is_ok_and(|n| n + 1 == v.len()) => {
            format!("{a} a {b}")
        }
        _ => lista(&v.iter().map(i32::to_string).collect::<Vec<_>>(), 10),
    }
}

/// Sin perfil fiscal (ni guardado ni deducido): no se sabe qué tocaba.
fn sin_perfil(s: &mut Salida) {
    s.add(
        Nuevo {
            codigo: "modelos.sin_perfil",
            area: Area::Modelos,
            gravedad: Gravedad::Info,
            responsable: Responsable::Empresa,
            titulo: "Sin perfil fiscal: no se sabe qué modelos tocaban".into(),
            ficheros: vec![],
        }
        .detalle(
            "Configura el perfil fiscal (Calendario → Perfil) para que la revisión diga qué \
                 modelos tendría que haber presentado la gestoría y cuáles faltan."
                .into(),
        ),
        None,
        None,
    );
}

/// Modelos que tocaban y no están, de los ejercicios que Hacienda aún puede revisar (los cuatro
/// últimos): según lo que tiene la AEAT, si el ejercicio se comprobó con «Comprobar lo
/// presentado»; si no, según el perfil fiscal. Se separa lo que no está de lo que está en la
/// carpeta sin leer (una nota, no se pide), y del 115 y el 180 (sólo con indicios de un
/// alquiler), el 202 (sólo si la cuota del 200 que le sirve de base es positiva) y el IVA de
/// fuera de las fechas de alta y baja del certificado de situación censal.
fn faltan(exp: &Expediente, dec: &BTreeMap<Clave, Declarado>, hoy: Date, s: &mut Salida) {
    let (perfil, deducido) = match (&exp.perfil, &exp.perfil_deducido) {
        (Some(p), _) => (p, false),
        (None, Some(d)) => (&d.perfil, true),
        (None, None) => {
            sin_perfil(s);
            return;
        }
    };
    if let Some(d) = exp.perfil_deducido.as_ref().filter(|_| deducido) {
        s.add(
            Nuevo {
                codigo: "modelos.perfil_deducido",
                area: Area::Modelos,
                gravedad: Gravedad::Info,
                responsable: Responsable::Empresa,
                titulo: "Perfil fiscal deducido de lo presentado: confírmalo en el perfil fiscal"
                    .into(),
                ficheros: vec![],
            }
            .detalle(format!(
                "No hay perfil fiscal guardado: los modelos que faltan se miran con el que se \
                 deduce de lo presentado ({}). Con este perfil sólo se pide un modelo en los \
                 ejercicios con ese mismo modelo presentado el año anterior, el mismo o el \
                 siguiente. Ponlo en Calendario → Perfil («Rellenar con lo presentado») y \
                 guárdalo para confirmarlo.",
                d.motivos.join("; ")
            )),
            None,
            None,
        );
    }
    // Con el perfil deducido, sólo lo de los años con ese modelo cerca (el anterior, el mismo o
    // el siguiente): un perfil que nadie ha confirmado no dice que tocara en un año antiguo.
    let por_nombre = por_nombre(exp);
    let cerca = |m: &str, y: i32| {
        !deducido
            || dec
                .keys()
                .chain(por_nombre.keys())
                .any(|(mm, yy, _)| mm == m && (y - 1..=y + 1).contains(yy))
    };
    let segun = if deducido {
        "Según el perfil fiscal deducido de lo presentado"
    } else {
        "Según el perfil fiscal"
    };
    let desde = revisable_desde(hoy);
    let alquiler = indicios_de_alquiler(exp, dec);
    let censo = censo_iva(exp, dec);
    let mut años = años_con_datos(exp, dec);
    años.extend(por_nombre.keys().map(|(_, y, _)| *y));
    let mut sin_alquiler: BTreeSet<i32> = BTreeSet::new();
    let mut filas = Vec::new();
    for y in años.range(desde..=hoy.year()) {
        let comprobado = exp.comprobado.get(y);
        let candidatos: Vec<(String, String, Fuente<'_>)> =
            match comprobado {
                Some(c) => {
                    let mut v: Vec<(String, String, Fuente<'_>)> =
                        c.faltan
                            .iter()
                            .map(|(m, p)| (m.clone(), p.clone(), Fuente::Aeat))
                            .chain(c.no_consta.iter().map(|(m, p, n)| {
                                (m.clone(), p.clone(), Fuente::AeatEnCiertosCasos(n))
                            }))
                            .collect();
                    // Lo que no contestó la AEAT, según el perfil: los modelos de los que no
                    // respondió y lo que vencía después de la comprobación.
                    for ((m, p), vence) in esperados(perfil, *y, hoy) {
                        if v.iter().any(|(mm, pp, _)| *mm == m && *pp == p) {
                            continue;
                        }
                        let fuente = if !c.respondidos.contains(&m) {
                            Fuente::PerfilSinAeat
                        } else if c.fecha.is_none_or(|f| vence >= f) {
                            Fuente::PerfilDespues
                        } else {
                            continue;
                        };
                        v.push((m, p, fuente));
                    }
                    v
                }
                None => esperados(perfil, *y, hoy)
                    .into_keys()
                    .map(|(m, p)| (m, p, Fuente::Perfil))
                    .collect(),
            };
        let vencimientos = esperados(perfil, *y, hoy);
        // Las autoliquidaciones que faltan (pueden salir a ingresar) y el plazo más antiguo.
        let mut primera_falta: Option<Date> = None;
        let mut con_ingreso: Vec<String> = Vec::new();
        let mut faltan: BTreeMap<String, Vec<String>> = BTreeMap::new();
        // Lo mismo, separado por de dónde sale que tocaba (para explicarlo).
        let mut faltan_por: BTreeMap<u8, BTreeMap<String, Vec<String>>> = BTreeMap::new();
        let mut sin_leer: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut ficheros_sin_leer = Vec::new();
        // Lo que se dice en la tabla (lo que no se sabe y lo que no tocaba por algo) y, aparte,
        // lo que no se sabe.
        let mut notas: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut dudas: Vec<String> = Vec::new();
        for (m, p, fuente) in candidatos {
            let clave = (m.clone(), *y, p.clone());
            if dec.contains_key(&clave) || !cerca(&m, *y) {
                continue;
            }
            let veredicto = if let Some(ruta) = por_nombre.get(&clave) {
                Veredicto::SinLeer(ruta.clone())
            } else if m == "115" || m == "180" {
                if alquiler.hay {
                    Veredicto::Falta
                } else {
                    sin_alquiler.insert(*y);
                    Veredicto::NoTocaba(None)
                }
            } else if m == "202" {
                tocaba_202(exp, dec, *y, &p)
            } else if let Some(v) = (m == "303" || m == "390")
                .then(|| censo.veredicto(*y, &p))
                .flatten()
            {
                v
            } else {
                match fuente {
                    Fuente::Perfil
                    | Fuente::Aeat
                    | Fuente::PerfilSinAeat
                    | Fuente::PerfilDespues => Veredicto::Falta,
                    Fuente::AeatEnCiertosCasos(nota) => {
                        Veredicto::NoSeSabe(format!("la AEAT no lo tiene. {nota}"))
                    }
                }
            };
            match veredicto {
                Veredicto::Falta => {
                    if !informativo(&m) && super::cruces::tiene_resultado(&m) {
                        if let Some(v) = vencimientos.get(&(m.clone(), p.clone())).copied() {
                            primera_falta = Some(primera_falta.map_or(v, |x: Date| x.min(v)));
                        }
                        con_ingreso.push(if p == "0A" {
                            m.clone()
                        } else {
                            format!("{m} del {p}")
                        });
                    }
                    let de = match fuente {
                        Fuente::Aeat | Fuente::AeatEnCiertosCasos(_) => 0,
                        Fuente::Perfil => 1,
                        Fuente::PerfilSinAeat => 2,
                        Fuente::PerfilDespues => 3,
                    };
                    faltan_por
                        .entry(de)
                        .or_default()
                        .entry(m.clone())
                        .or_default()
                        .push(p.clone());
                    faltan.entry(m).or_default().push(p);
                }
                Veredicto::SinLeer(ruta) => {
                    sin_leer.entry(m).or_default().push(p);
                    ficheros_sin_leer.push(ruta);
                }
                Veredicto::NoSeSabe(razon) => {
                    let razon = razon.trim().trim_end_matches('.');
                    dudas.push(if p == "0A" {
                        format!("{m}: {razon}")
                    } else {
                        format!("{m} del {p}: {razon}")
                    });
                    notas.entry(m).or_default().push(con_periodo(&p, razon));
                }
                Veredicto::NoTocaba(Some(razon)) => {
                    notas.entry(m).or_default().push(con_periodo(&p, &razon));
                }
                Veredicto::NoTocaba(None) => {}
            }
        }
        let modelos: BTreeSet<&String> = faltan
            .keys()
            .chain(sin_leer.keys())
            .chain(notas.keys())
            .collect();
        for m in modelos {
            let presentados: Vec<String> = dec
                .keys()
                .filter(|(mm, yy, _)| mm == m && yy == y)
                .map(|(_, _, p)| p.clone())
                .collect();
            let mut nota: Vec<String> = sin_leer
                .get(m)
                .map(|ps| vec![format!("{}: en la carpeta, sin leer", ps.join(", "))])
                .unwrap_or_default();
            nota.extend(notas.get(m).cloned().unwrap_or_default());
            filas.push(vec![
                y.to_string(),
                m.clone(),
                presentados.join(", "),
                faltan.get(m).map(|ps| ps.join(", ")).unwrap_or_default(),
                nota.join(" · "),
            ]);
        }
        if !faltan.is_empty() {
            let texto = por_modelo(&faltan);
            let n: usize = faltan.values().map(Vec::len).sum();
            let detalle = match comprobado {
                Some(c) => {
                    let cuando = c.fecha.map_or_else(|| c.cuando.clone(), super::fecha);
                    let de = |k: u8| faltan_por.get(&k).map(|m| por_modelo(m).join("; "));
                    let mut partes = Vec::new();
                    if let Some(t) = de(0) {
                        partes.push(format!(
                            "La AEAT no tiene de {y}: {t}, y tocaban (lo dice «Comprobar lo \
                             presentado» del {cuando})."
                        ));
                    }
                    if let Some(t) = de(2) {
                        partes.push(format!(
                            "{segun} tendrían que estar {t}, y de esos modelos no \
                             contestó la AEAT en «Comprobar lo presentado» del {cuando} (la \
                             consulta falló o no se hizo)."
                        ));
                    }
                    if let Some(t) = de(3) {
                        partes.push(format!(
                            "{segun} tendrían que estar {t}, cuyo plazo acabó \
                             después de «Comprobar lo presentado» del {cuando}: vuelve a \
                             comprobarlo para saber si los tiene la AEAT."
                        ));
                    }
                    partes.push(
                        "Pídelos a la gestoría y, si no se presentaron, que los presente cuanto \
                         antes."
                            .to_owned(),
                    );
                    partes.join(" ")
                }
                None => format!(
                    "{segun}, de {y} tendrían que estar: {}. Pídelos a la gestoría \
                     (o descárgalos de la Sede de la AEAT con el certificado de la empresa). Si \
                     alguno no tocaba (sin retenciones, sin operaciones con la UE…), revisa el \
                     perfil fiscal.",
                    texto.join("; ")
                ),
            };
            s.add(
                Nuevo {
                    codigo: "modelos.faltan",
                    area: Area::Modelos,
                    gravedad: Gravedad::Aviso,
                    responsable: Responsable::Documentacion,
                    titulo: format!(
                        "Faltan {} de {y}",
                        plural(n, "modelo presentado", "modelos presentados")
                    ),
                    ficheros: vec![],
                }
                .detalle(detalle),
                Some(*y),
                None,
            );
            if !con_ingreso.is_empty() {
                let mut u = match primera_falta {
                    Some(v) => super::Urgente::art27(v, None, hoy),
                    None => super::Urgente::sin_calculo(None, String::new()),
                };
                u.nota = format!(
                    "Faltan autoliquidaciones que pueden salir a ingresar ({}). {}",
                    con_ingreso.join(", "),
                    if primera_falta.is_some() {
                        format!("Con la más antigua: {}", u.nota)
                    } else {
                        "Si salían a ingresar, presentadas ahora sin requerimiento llevan el \
                         recargo del art. 27 LGT (1 % más otro 1 % por mes completo; 15 % e \
                         intereses pasados doce meses)."
                            .to_owned()
                    }
                );
                s.urgente(u);
            }
            s.pedir(
                Destinatario::Gestoria,
                format!(
                    "Los modelos presentados de {y} que faltan: {}",
                    texto.join("; ")
                ),
            );
        }
        if !sin_leer.is_empty() {
            let n: usize = sin_leer.values().map(Vec::len).sum();
            s.add(
                Nuevo {
                    codigo: "modelos.sin_leer",
                    area: Area::Modelos,
                    gravedad: Gravedad::Info,
                    responsable: Responsable::Empresa,
                    titulo: format!(
                        "{} de {y} en la carpeta sin leer",
                        primera_mayuscula(&plural(n, "modelo presentado", "modelos presentados"))
                    ),
                    ficheros: ficheros_sin_leer,
                }
                .detalle(format!(
                    "Están en la carpeta (lo dice el nombre del fichero), pero no se han podido \
                     leer: {}. No hace falta pedirlos; para que entren en los cruces, léelos con \
                     IA (Importar → Carpeta de la empresa) o con Claude Code (/leer-carpeta).",
                    por_modelo(&sin_leer).join("; ")
                )),
                Some(*y),
                None,
            );
        }
        if !dudas.is_empty() {
            s.add(
                Nuevo {
                    codigo: "modelos.no_se_sabe",
                    area: Area::Modelos,
                    gravedad: Gravedad::Info,
                    responsable: Responsable::Documentacion,
                    titulo: format!(
                        "No se sabe si tocaban {} de {y}",
                        plural(dudas.len(), "modelo", "modelos")
                    ),
                    ficheros: vec![],
                }
                .detalle(format!(
                    "No están y no se piden, porque no se sabe si tocaban: {}.",
                    dudas.join("; ")
                )),
                Some(*y),
                None,
            );
        }
    }
    if !sin_alquiler.is_empty() && perfil.retiene_alquileres && !deducido {
        let censo = alquiler.censo_sin.map_or_else(String::new, |f| {
            format!(
                ", y el certificado de situación censal del {} no tiene esa obligación",
                super::fecha(f)
            )
        });
        s.add(
            Nuevo {
                codigo: "modelos.alquiler_sin_indicios",
                area: Area::Modelos,
                gravedad: Gravedad::Info,
                responsable: Responsable::Empresa,
                titulo: "No se piden el 115 ni el 180: no hay indicios de un alquiler".into(),
                ficheros: vec![],
            }
            .detalle(format!(
                "El perfil fiscal dice que la empresa paga un alquiler con retención, pero en la \
                 carpeta no hay ningún contrato de alquiler, ni facturas de alquiler con \
                 retención, ni ningún 115 o 180 presentado{censo}. Por eso no se piden los de {}. \
                 Si la empresa no alquila ningún local, quita la retención de alquileres del \
                 perfil fiscal (Calendario → Perfil); si lo alquila, el 115 se presenta cada \
                 trimestre y el 180 cada año.",
                años_texto(&sin_alquiler)
            )),
            None,
            None,
        );
    }
    s.tabla(
        "Modelos que faltan por ejercicio",
        "Modelos que faltan",
        &["Ejercicio", "Modelo", "Presentados", "Faltan", "Nota"],
        filas,
    );
}

/// «2T: la razón» (o sólo la razón en un modelo anual).
fn con_periodo(p: &str, razon: &str) -> String {
    if p == "0A" {
        razon.to_owned()
    } else {
        format!("{p}: {razon}")
    }
}

/// «3 modelos…» → «3 modelos…»; «modelo…» → «Modelo…».
fn primera_mayuscula(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|p| p.to_uppercase().chain(c).collect())
        .unwrap_or_default()
}

/// Facturas únicas (una por emisor y número, o por fichero), sin las de otra moneda.
pub(crate) fn unicas<'a>(it: impl Iterator<Item = (&'a str, &'a Factura)>) -> Vec<&'a Factura> {
    let mut vistas = BTreeSet::new();
    let mut out = Vec::new();
    for (sha, f) in it {
        if f.moneda.is_some() {
            continue;
        }
        let clave = match (f.emisor_clave(), f.numero.as_deref()) {
            (Some(e), Some(n)) => format!("{e}|{}", crate::libros::number_key(n)),
            _ => sha.to_owned(),
        };
        if vistas.insert(clave) {
            out.push(f);
        }
    }
    out
}

fn del_trimestre(f: &Factura, y: i32, q: u8) -> bool {
    f.fecha.is_some_and(|d| d.year() == y && trimestre(d) == q)
}

/// Trimestre de un periodo «1T»…«4T».
pub(crate) fn trimestre_de(p: &str) -> Option<u8> {
    match p.as_bytes() {
        [n @ b'1'..=b'4', b'T'] => Some(n - b'0'),
        _ => None,
    }
}

/// NIF-IVA de otro país de la UE.
pub(crate) fn de_la_ue(nif: &str) -> bool {
    let n = nif_clave(nif);
    n.get(..2).is_some_and(|p| {
        p != "ES" && taxpain_core::eu::EU_COUNTRIES.contains(&p) && nif_español(nif).is_none()
    })
}

/// El 303 frente a los libros registro y el diario del trimestre y, si no hay libro, frente a
/// las facturas de la carpeta: el libro manda (una factura que falta en la carpeta es un papel
/// que hay que conseguir, no un error del modelo).
fn m303(exp: &Expediente, dec: &BTreeMap<Clave, Declarado>, s: &mut Salida) {
    let libros = Libros::de(exp);
    let fuera = super::registro::recibidas_fuera_de_los_libros(exp);
    let mut filas = Vec::new();
    let mut filas_libro = Vec::new();
    for ((m, y, p), d) in dec {
        let Some(q) = trimestre_de(p).filter(|_| m == "303" && con_liquidacion(d)) else {
            continue;
        };
        let periodo = format!("{q}T {y}");
        let t = Trimestre {
            y: *y,
            q,
            periodo: &periodo,
            d,
        };
        contra_libros(exp, &libros, &t, &mut filas_libro, s);
        if !libros.de_recibidas(*y, q).is_empty() {
            sin_deducir_con_libro(exp, &libros, &fuera, &t, s);
        }
        if libros.de_recibidas(*y, q).is_empty() {
            soportado_con_facturas(exp, &t, &mut filas, s);
        }
        if libros.de_emitidas(*y, q).is_empty() {
            devengado_con_facturas(exp, &t, &mut filas, s);
        }
    }
    s.tabla(
        "303 frente a los libros registro",
        "303 frente a los libros",
        &[
            "Periodo",
            "Concepto",
            "Modelo",
            "Libro o diario",
            "Diferencia",
        ],
        filas_libro,
    );
    s.tabla(
        "303 frente a las facturas de la carpeta",
        "303 frente a las facturas",
        &["Periodo", "Concepto", "Modelo", "Facturas", "Diferencia"],
        filas,
    );
}

/// ¿Trae el 303 alguna casilla de la liquidación (de la [01] a la [46] y de la [150] a la
/// [158])? Uno de «Comprobar lo presentado» sin su PDF, o uno de la carpeta que no se pudo leer,
/// no trae ninguna: no se sabe qué declara y no se cruza con nada.
fn con_liquidacion(d: &Declarado) -> bool {
    d.casillas.keys().any(|k| matches!(k, 1..=46 | 150..=158))
}

/// Un 303 trimestral y su periodo.
struct Trimestre<'a> {
    y: i32,
    q: u8,
    periodo: &'a str,
    d: &'a Declarado,
}

/// Una casilla (o una suma de casillas) del 303 frente a lo que dicen los libros o el diario.
struct Cotejo {
    /// Para la tabla.
    concepto: String,
    /// Para el texto: «la base de IVA soportado [28]+[30]».
    frase: String,
    modelo: Money,
    libro: Money,
    /// Lo que suma (para el texto, si casa): «500,00 € de facturas interiores y…».
    desglose: Option<String>,
    /// Otro importe del modelo que también vale, con lo que se dice si casa con él.
    alternativa: Option<(Money, String)>,
}

impl Cotejo {
    fn casa(&self) -> bool {
        (self.modelo - self.libro).abs() <= UN_EURO || self.por_alternativa().is_some()
    }

    /// La alternativa, si no casa con el modelo y sí con ella.
    fn por_alternativa(&self) -> Option<&(Money, String)> {
        self.alternativa.as_ref().filter(|(m, _)| {
            (self.modelo - self.libro).abs() > UN_EURO && (*m - self.libro).abs() <= UN_EURO
        })
    }

    /// El importe del modelo con que se compara: el de la alternativa si casa con ella.
    fn del_modelo(&self) -> Money {
        self.por_alternativa().map_or(self.modelo, |(m, _)| *m)
    }
}

/// Una suma de casillas del 303 («[28]+[30]+[40]») y lo que suma: siempre las de `fijas` y,
/// de las de `extra`, las que traen algo.
fn casillas(d: &Declarado, fijas: &[u32], extra: &[u32]) -> (Money, String) {
    let usadas: Vec<u32> = fijas
        .iter()
        .copied()
        .chain(extra.iter().copied().filter(|n| !d.c(*n).is_zero()))
        .collect();
    let suma = usadas.iter().map(|n| d.c(*n)).sum();
    let texto = usadas
        .iter()
        .map(|n| format!("[{n:02}]"))
        .collect::<Vec<_>>()
        .join("+");
    (suma, texto)
}

/// La base del IVA devengado del 303 que se compara con los libros: las ventas con IVA ([01] 4 %,
/// [04] 10 %, [153] 5 % y [07] 21 %; la [150], del 0 %, no lleva cuota y el libro no la distingue
/// de las exentas) y la modificación de bases de la [14], donde van las rectificativas.
fn devengado_base(d: &Declarado) -> (Money, String) {
    casillas(d, &[1, 4, 153, 7], &[14])
}

/// «a, b y c».
fn y_lista(partes: &[String]) -> String {
    match partes {
        [] => String::new(),
        [a] => a.clone(),
        [resto @ .., ultima] => format!("{} y {ultima}", resto.join(", ")),
    }
}

/// El IVA soportado del 303 frente a las recibidas del libro de un trimestre: los cotejos y
/// una nota con el IVA de los proveedores de fuera de España.
fn soportado_con_libro(d: &Declarado, r: &Recibidas) -> (Vec<Cotejo>, String) {
    let mut out = Vec::new();
    let mut notas = String::new();
    // Las importaciones (el DUA) van en la [32] y la [34] y las rectificaciones de las
    // deducciones (las rectificativas recibidas), en la [40]; la gestoría puede dejarlas en
    // la [28], y la suma es la misma.
    let importa = !(d.c(32) + d.c(34)).is_zero() || !(d.c(33) + d.c(35)).is_zero();
    let (mb, cas_b) = casillas(d, &[28, 30], &[32, 34, 40]);
    let (mc, cas_c) = casillas(d, &[29, 31], &[33, 35, 41]);
    // La [28] lleva lo interior y la inversión del sujeto pasivo que se deduce.
    let isp = r.isp_deducible;
    let sin = (r.interior.base + isp.base, r.interior.cuota + isp.cuota);
    let con = (sin.0 + r.extranjero.base, sin.1 + r.extranjero.cuota);
    let casa = |x: (Money, Money)| (mb - x.0).abs() <= UN_EURO && (mc - x.1).abs() <= UN_EURO;
    let (lb, lc) = if r.extranjero.n > 0 && !casa(sin) && casa(con) {
        notas.push_str(&format!(
            " El 303 deduce el IVA de {} ({}), que no se puede deducir (art. 94.Tres y 97 LIVA; \
             ver las facturas de proveedores de fuera de España con IVA).",
            de_fuera(r.extranjero.n),
            eur(r.extranjero.cuota)
        ));
        con
    } else {
        if r.extranjero.n > 0 {
            notas.push_str(&format!(
                " No se cuenta el IVA de {} ({}): no se puede deducir (art. 94.Tres y 97 LIVA; \
                 ver las facturas de proveedores de fuera de España con IVA).",
                de_fuera(r.extranjero.n),
                eur(r.extranjero.cuota)
            ));
        }
        sin
    };
    let interiores = if importa {
        "facturas interiores e importaciones"
    } else {
        "facturas interiores"
    };
    let desglose = match (isp.base.is_zero(), (lb - isp.base).is_zero()) {
        (true, _) => None,
        (false, true) => Some("todo de inversión del sujeto pasivo".to_owned()),
        (false, false) => Some(format!(
            "{} de {interiores} y {} de inversión del sujeto pasivo",
            eur(lb - isp.base),
            eur(isp.base)
        )),
    };
    let que = if importa {
        "IVA soportado interior, importaciones e inversión del sujeto pasivo"
    } else {
        "IVA soportado interior e inversión del sujeto pasivo"
    };
    out.push(Cotejo {
        concepto: format!("{que}: base {cas_b}"),
        frase: format!("la base de IVA soportado {cas_b}"),
        modelo: mb,
        libro: lb,
        desglose,
        alternativa: None,
    });
    out.push(Cotejo {
        concepto: format!("{que}: cuota {cas_c}"),
        frase: format!("su cuota {cas_c}"),
        modelo: mc,
        libro: lc,
        desglose: None,
        alternativa: None,
    });
    if !r.intra.base.is_zero() || !d.c(10).is_zero() {
        out.push(Cotejo {
            concepto: "Adquisiciones intracomunitarias: base [10]".into(),
            frase: "las adquisiciones intracomunitarias [10]".into(),
            modelo: d.c(10),
            libro: r.intra.base,
            desglose: None,
            alternativa: None,
        });
    }
    if !r.isp.base.is_zero() || !d.c(12).is_zero() {
        out.push(Cotejo {
            concepto: "Inversión del sujeto pasivo: base [12]".into(),
            frase: "la inversión del sujeto pasivo [12]".into(),
            modelo: d.c(12),
            libro: r.isp.base,
            desglose: None,
            alternativa: None,
        });
    }
    (out, notas)
}

/// La nota de las facturas que se cuentan en el trimestre de la carpeta de su PDF.
fn nota_movidas(t: (i32, u8), movidas: &[Movida]) -> String {
    let cual = |m: &Movida| format!("la {} de {} ({})", m.numero, m.nombre, fecha(m.fecha));
    let entran: Vec<String> = movidas.iter().filter(|m| m.a == t).map(cual).collect();
    let salen: Vec<String> = movidas
        .iter()
        .filter(|m| m.de == t)
        .map(|m| {
            format!(
                "la {} de {} ({}, en la carpeta del {}T {})",
                m.numero,
                m.nombre,
                fecha(m.fecha),
                m.a.1,
                m.a.0
            )
        })
        .collect();
    let mut out = String::new();
    if !entran.is_empty() {
        out.push_str(&format!(
            " Se {} en este trimestre, por la carpeta de su PDF, {}: el libro no trae la fecha de \
             registro.",
            if entran.len() == 1 {
                "cuenta"
            } else {
                "cuentan"
            },
            lista(&entran, 6)
        ));
    }
    if !salen.is_empty() {
        let v = if salen.len() == 1 {
            "cuenta"
        } else {
            "cuentan"
        };
        out.push_str(&format!(
            " No se {v} {}: el libro no trae la fecha de registro y {v} en el trimestre de la \
             carpeta de su PDF.",
            lista(&salen, 6)
        ));
    }
    out
}

/// El 303 frente a los libros registro del trimestre (recibidas y emitidas) y frente a las
/// cuentas 472 y 477 del diario importado en TaxPain.
fn contra_libros(
    exp: &Expediente,
    libros: &Libros,
    t: &Trimestre,
    filas: &mut Vec<Vec<String>>,
    s: &mut Salida,
) {
    let (y, q, d) = (t.y, t.q, t.d);
    let libro_r = libros.de_recibidas(y, q);
    let libro_e = libros.de_emitidas(y, q);
    let diario = exp
        .taxpain
        .iva_diario
        .get(&(y, q))
        .filter(|x| x.con_472 || x.con_477);
    if libro_r.is_empty() && libro_e.is_empty() && diario.is_none() {
        return;
    }
    let mut con_libro: Vec<Cotejo> = Vec::new();
    let mut notas = String::new();
    if !libro_r.is_empty() {
        let (mut cotejos, mut nota) = soportado_con_libro(d, &libros.recibidas(y, q));
        // Si no casa, otras maneras de contar el libro que puede haber usado la gestoría: el
        // destino que el libro da a cada factura (por el NIF del proveedor) aunque su PDF diga
        // otra cosa y, si el libro no trae la fecha de registro, cada factura en el trimestre de
        // la carpeta de su PDF (la de marzo que está en la carpeta del 2T se dedujo en el 2T).
        let movidas = libros.movidas(y, q);
        let otras = [(false, false), (true, true), (false, true)]
            .into_iter()
            .filter(|(pdf, carpeta)| {
                (*pdf || libros.cambia_por_pdf(y, q)) && (!*carpeta || !movidas.is_empty())
            })
            .map(|(pdf, carpeta)| Segun { pdf, carpeta });
        if !cotejos.iter().all(Cotejo::casa) {
            for segun in otras {
                let (otros, otra) = soportado_con_libro(d, &libros.recibidas_segun(y, q, segun));
                if otros.iter().all(Cotejo::casa) {
                    cotejos = otros;
                    nota = otra;
                    if segun.carpeta {
                        nota.push_str(&nota_movidas((y, q), &movidas));
                    }
                    break;
                }
            }
        }
        con_libro.extend(cotejos);
        notas.push_str(&nota);
    }
    if !libro_e.is_empty() {
        // Las ventas con IVA (las del 0 % de la [150] no llevan cuota, y el libro no las
        // distingue de las exentas) y la modificación de bases y cuotas de la [14] y la [15],
        // donde van las rectificativas emitidas.
        let e = libros.emitidas(y, q);
        let (mb, cas_b) = devengado_base(d);
        let (mc, cas_c) = casillas(d, &[3, 6, 155, 9], &[15]);
        if !mb.is_zero() || !e.base.is_zero() {
            con_libro.push(Cotejo {
                concepto: format!("IVA devengado: base {cas_b}"),
                frase: "la base de IVA devengado".into(),
                modelo: mb,
                libro: e.base,
                desglose: None,
                alternativa: None,
            });
            con_libro.push(Cotejo {
                concepto: format!("IVA devengado: cuota {cas_c}"),
                frase: "su cuota".into(),
                modelo: mc,
                libro: e.cuota,
                desglose: None,
                alternativa: None,
            });
        }
    }
    // Sólo las cuentas con apuntes en el trimestre: un diario sin la 472 (o sin la 477) no
    // lleva ese IVA.
    let mut con_diario: Vec<Cotejo> = Vec::new();
    if let Some(x) = diario {
        if x.con_472 {
            // En un diario resumido, el IVA de una rectificativa recibida (la [41]) puede ir en la
            // misma línea de la 472 que la liquidación, y no se distingue de ella.
            let rectificacion = d.c(41);
            con_diario.push(Cotejo {
                concepto: "IVA soportado: total a deducir [45] y cuenta 472 del diario".into(),
                frase: "el total a deducir [45] con la cuenta 472".into(),
                modelo: d.c(45),
                libro: x.soportado,
                desglose: None,
                alternativa: (x.liquidacion_mezclada && !rectificacion.is_zero()).then(|| {
                    (
                        d.c(45) - rectificacion,
                        format!(
                            "sin la rectificación de deducciones [41], {}, que el diario \
                             resumido lleva junto con la liquidación",
                            eur(rectificacion)
                        ),
                    )
                }),
            });
        }
        if x.con_477 {
            con_diario.push(Cotejo {
                concepto: "IVA devengado: total [27] y cuenta 477 del diario".into(),
                frase: "el total devengado [27] con la cuenta 477".into(),
                modelo: d.c(27),
                libro: x.repercutido,
                desglose: None,
                alternativa: None,
            });
        }
    }
    for c in con_libro.iter().chain(&con_diario) {
        filas.push(vec![
            t.periodo.to_owned(),
            match c.por_alternativa() {
                Some((_, x)) => format!("{} ({x})", c.concepto),
                None => c.concepto.clone(),
            },
            eur(c.del_modelo()),
            eur(c.libro),
            eur(c.del_modelo() - c.libro),
        ]);
    }
    let mut ficheros = vec![d.fichero.clone()];
    for f in libro_r.into_iter().chain(libro_e) {
        if !ficheros.contains(&f) {
            ficheros.push(f);
        }
    }
    let periodo = t.periodo;
    let libro_ok = !con_libro.is_empty() && con_libro.iter().all(Cotejo::casa);
    let diario_ok = !con_diario.is_empty() && con_diario.iter().all(Cotejo::casa);
    if libro_ok || diario_ok {
        let con = match (libro_ok, diario_ok) {
            (true, true) => "los libros registro y el diario",
            (true, false) => "los libros registro",
            _ => "el diario",
        };
        let casan: Vec<String> = con_libro
            .iter()
            .filter(|_| libro_ok)
            .chain(con_diario.iter().filter(|_| diario_ok))
            .map(
                |c| match c.por_alternativa().map(|(_, x)| x).or(c.desglose.as_ref()) {
                    Some(x) => format!("{} ({}: {x})", c.frase, eur(c.modelo)),
                    None => format!("{} ({})", c.frase, eur(c.modelo)),
                },
            )
            .collect();
        s.add(
            Nuevo {
                codigo: "modelos.303_cuadra_libro",
                area: Area::Modelos,
                gravedad: Gravedad::Info,
                responsable: Responsable::Gestoria,
                titulo: format!("El 303 del {periodo} cuadra con {con}"),
                ficheros: ficheros.clone(),
            }
            .detalle(format!("Coinciden {}.{notas}", y_lista(&casan))),
            Some(t.y),
            None,
        );
    }
    let diferencias = |cotejos: &[Cotejo], donde: &str| -> Vec<String> {
        cotejos
            .iter()
            .filter(|c| !c.casa())
            .map(|c| {
                format!(
                    "{}: {} en el 303 y {} en {donde}",
                    c.frase,
                    eur(c.modelo),
                    eur(c.libro)
                )
            })
            .collect()
    };
    if !con_libro.is_empty() && !libro_ok {
        let dif = diferencias(&con_libro, "el libro");
        s.add(
            Nuevo {
                codigo: "modelos.303_libro",
                area: Area::Modelos,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Gestoria,
                titulo: format!("El 303 del {periodo} no casa con los libros registro"),
                ficheros: ficheros.clone(),
            }
            .detalle(format!(
                "No coinciden {}. Puede faltar o sobrar una factura en el libro, estar en otro \
                 trimestre o el modelo declarar algo que no está en el libro: pide a la gestoría \
                 que lo explique.{notas}",
                y_lista(&dif)
            )),
            Some(t.y),
            None,
        );
        s.pedir(
            Destinatario::Gestoria,
            format!(
                "La explicación de las diferencias entre el 303 del {periodo} y los libros registro"
            ),
        );
    }
    if !con_diario.is_empty() && !diario_ok {
        let dif = diferencias(&con_diario, "el diario");
        s.add(
            Nuevo {
                codigo: "modelos.303_diario",
                area: Area::Modelos,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Gestoria,
                titulo: format!(
                    "El 303 del {periodo} no casa con las cuentas 472 y 477 del diario"
                ),
                ficheros: vec![d.fichero.clone()],
            }
            .detalle(format!(
                "No coinciden {}, sin contar la liquidación del trimestre. Puede haber un asiento \
                 en otro trimestre o una factura sin contabilizar (o contabilizada sin declarar).",
                y_lista(&dif)
            )),
            Some(t.y),
            None,
        );
    }
}

/// Con el libro de recibidas, el IVA sin deducir del trimestre: las facturas con IVA español
/// que no están en ninguna hoja del libro (un posible olvido) y las de la hoja de IVA no
/// deducible (una decisión de la gestoría, que conviene revisar).
fn sin_deducir_con_libro(
    exp: &Expediente,
    libros: &Libros,
    fuera: &[(&Doc, &Factura)],
    t: &Trimestre,
    s: &mut Salida,
) {
    let (y, q, periodo) = (t.y, t.q, t.periodo);
    let sueltas: Vec<&(&Doc, &Factura)> = fuera
        .iter()
        .filter(|(_, f)| del_trimestre(f, y, q) && super::registro::con_iva_español(exp, f))
        .collect();
    let no_deducible = libros.recibidas(y, q).no_deducible;
    if sueltas.is_empty() && no_deducible.n == 0 {
        return;
    }
    let mut detalle = String::new();
    if !sueltas.is_empty() {
        let iva: Money = sueltas.iter().filter_map(|(_, f)| f.iva).sum();
        let cuales: Vec<String> = sueltas
            .iter()
            .map(|(_, f)| {
                let quien = f
                    .emisor_nombre
                    .as_deref()
                    .or(f.emisor_nif.as_deref())
                    .unwrap_or("emisor sin nombre");
                match f.numero.as_deref() {
                    Some(n) => format!("{n} de {quien}"),
                    None => format!("la de {quien}"),
                }
            })
            .collect();
        detalle.push_str(&format!(
            "{} con IVA español del trimestre no {} en ninguna hoja del libro registro ni se \
             {} en el 303: {} ({} de IVA). Si es un gasto de la empresa, su IVA se puede \
             deducir en los cuatro años siguientes (art. 99.Tres LIVA); si no lo es, no \
             debería estar en su carpeta.",
            plural(sueltas.len(), "factura", "facturas"),
            if sueltas.len() == 1 {
                "está"
            } else {
                "están"
            },
            if sueltas.len() == 1 {
                "dedujo"
            } else {
                "dedujeron"
            },
            lista(&cuales, 6),
            eur(iva)
        ));
    }
    if no_deducible.n > 0 {
        if !detalle.is_empty() {
            detalle.push(' ');
        }
        detalle.push_str(&format!(
            "La gestoría no dedujo el IVA de {} que tiene en la hoja de IVA no deducible del \
             libro ({}): es una decisión suya, no un olvido; revisa que sean gastos sin derecho \
             a deducir (comidas, regalos, ocio o lo que no es de la empresa, art. 96 LIVA).",
            plural(no_deducible.n, "factura", "facturas"),
            eur(no_deducible.cuota)
        ));
    }
    let mut ficheros = vec![t.d.fichero.clone()];
    ficheros.extend(sueltas.iter().map(|(d, _)| d.ruta.clone()));
    s.add(
        Nuevo {
            codigo: "modelos.303_iva_sin_deducir",
            area: Area::Modelos,
            gravedad: Gravedad::Info,
            responsable: Responsable::Gestoria,
            titulo: format!("Facturas con IVA del {periodo} que no se dedujeron en el 303"),
            ficheros,
        }
        .detalle(detalle),
        Some(y),
        None,
    );
}

/// El IVA soportado del 303 frente a las facturas recibidas del trimestre en la carpeta (cuando
/// no hay libro de recibidas que lo cubra).
fn soportado_con_facturas(
    exp: &Expediente,
    t: &Trimestre,
    filas: &mut Vec<Vec<String>>,
    s: &mut Salida,
) {
    let (y, q, d) = (&t.y, t.q, t.d);
    let periodo = t.periodo;
    let recibidas = unicas(
        exp.recibidas()
            .filter(|(_, f)| del_trimestre(f, *y, q))
            .map(|(doc, f)| (doc.sha.as_str(), f)),
    );
    // IVA soportado interior: facturas con IVA de proveedores españoles (o sin NIF y sin señas
    // de ser de fuera). Las de proveedores de fuera de España que cobran IVA a un tipo español no
    // deberían cobrarlo (o, sin el NIF de la empresa, son de un particular) y no se puede
    // deducir, pero la gestoría lo suele deducir igual: se cuentan si el 303 deduce más que las
    // interiores. Las de fuera de la UE sin ese IVA son inversión del sujeto pasivo: van a la
    // [12].
    let propia = |f: &Factura| f.emisor_nif.as_deref().is_some_and(|n| exp.es_propio(n));
    let de_fuera_ue = |f: &Factura| f.emisor_extranjero() && !f.emisor_de_la_ue();
    let con_iva =
        |f: &Factura| f.iva.is_some_and(|i| i.cents() > 0) && f.base.is_some() && !propia(f);
    let interiores: Vec<&Factura> = recibidas
        .iter()
        .copied()
        .filter(|f| con_iva(f) && !f.emisor_extranjero())
        .collect();
    let extranjeras: Vec<&Factura> = recibidas
        .iter()
        .copied()
        .filter(|f| con_iva(f) && f.emisor_extranjero() && f.con_tipo_español())
        .collect();
    let suma = |v: &[&Factura]| -> (Money, Money) {
        (
            v.iter().filter_map(|f| f.base).sum(),
            v.iter().filter_map(|f| f.iva).sum(),
        )
    };
    // La [28] lleva también la deducción de la inversión del sujeto pasivo ([12] y [13]) y la
    // [32] y la [34], las importaciones (el DUA de la aduana): las facturas interiores se
    // comparan con lo demás.
    let (c12, c13) = (d.c(12), d.c(13));
    let (b, cas_b) = casillas(d, &[28, 30], &[32, 34]);
    let (c, cas_c) = casillas(d, &[29, 31], &[33, 35]);
    let (mb, mi) = (b - c12, c - c13);
    let deduce_extranjeras = !extranjeras.is_empty() && mb - suma(&interiores).0 > UN_EURO;
    let interior: Vec<&Factura> = if deduce_extranjeras {
        interiores.iter().chain(&extranjeras).copied().collect()
    } else {
        interiores.clone()
    };
    // Inversión del sujeto pasivo: facturas de proveedores de fuera de la UE (en euros), sin las
    // que el 303 deduce como interiores. Y las de proveedores españoles sin IVA, que pueden ser
    // obras, chatarra o móviles con inversión del sujeto pasivo (art. 84.Uno.2.º LIVA) o
    // exentas: se cuentan si con ellas casa.
    let isp: Vec<&Factura> = recibidas
        .iter()
        .copied()
        .filter(|f| !propia(f) && de_fuera_ue(f) && f.base.or(f.total).is_some())
        .filter(|f| !(deduce_extranjeras && extranjeras.iter().any(|e| std::ptr::eq(*e, *f))))
        .collect();
    let isp_interior: Money = recibidas
        .iter()
        .filter(|f| {
            !propia(f)
                && f.iva.is_some_and(Money::is_zero)
                && f.emisor_nif
                    .as_deref()
                    .is_some_and(|n| nif_español(n).is_some())
        })
        .filter_map(|f| f.base)
        .filter(|b| b.cents() > 0)
        .sum();
    let en_divisa = exp
        .recibidas()
        .filter(|(_, f)| del_trimestre(f, *y, q) && f.moneda.is_some())
        .count();
    // Las reconocidas sólo por las reglas traen número, fecha y total, pero no la base ni
    // la cuota: con la carpeta leída a medias el cruce queda incompleto y no se puede
    // acusar a nadie.
    let sin_base = recibidas
        .iter()
        .filter(|f| f.base.is_none() && !f.emisor_nif.as_deref().is_some_and(|n| exp.es_propio(n)))
        .count();
    let mut nota = String::new();
    if sin_base > 0 {
        nota.push_str(&format!(
            " Hay {} del trimestre sin base leída (reconocidas sólo por las reglas), que no \
             se cuentan: léelas con IA para un cruce completo.",
            plural(sin_base, "factura", "facturas")
        ));
    }
    if deduce_extranjeras {
        nota.push_str(&format!(
            " Las facturas incluyen {} de proveedores de fuera de España con IVA español, \
             que no se puede deducir (ver las facturas de proveedores de fuera de España con \
             IVA).",
            plural(extranjeras.len(), "factura", "facturas")
        ));
    } else if !extranjeras.is_empty() {
        nota.push_str(&format!(
            " No se cuentan {} de proveedores de fuera de España con IVA español ({} de \
             cuota): ese IVA no se puede deducir.",
            plural(extranjeras.len(), "factura", "facturas"),
            eur(suma(&extranjeras).1)
        ));
    }
    if en_divisa > 0 {
        nota.push_str(&format!(
            " No se cuentan {} en otra moneda, que la gestoría habrá pasado a euros.",
            plural(en_divisa, "factura", "facturas")
        ));
    }
    let (sin_isp, menos) = if c12.is_zero() {
        (String::new(), "")
    } else {
        (
            format!(
                "Sin la inversión del sujeto pasivo de la casilla [12] ({}), ",
                eur(c12)
            ),
            "−[12]",
        )
    };
    if !interior.is_empty() {
        let (base, iva) = suma(&interior);
        filas.push(vec![
            periodo.to_owned(),
            format!("IVA soportado interior: base {cas_b}{menos}"),
            eur(mb),
            eur(base),
            eur(mb - base),
        ]);
        filas.push(vec![
            periodo.to_owned(),
            format!(
                "IVA soportado interior: cuota {cas_c}{}",
                if c12.is_zero() { "" } else { "−[13]" }
            ),
            eur(mi),
            eur(iva),
            eur(mi - iva),
        ]);
        let diff = mb - base;
        if deduce_extranjeras && diff.abs() <= UN_EURO {
            iva_extranjero_deducido(exp, t, &extranjeras, s);
        } else if diff > UN_EURO {
            s.add(
                Nuevo {
                    codigo: "modelos.303_falta_factura",
                    area: Area::Modelos,
                    // Con facturas sin leer la diferencia casi seguro es de ellas.
                    gravedad: if sin_base > 0 {
                        Gravedad::Info
                    } else {
                        Gravedad::Aviso
                    },
                    responsable: Responsable::Documentacion,
                    titulo: format!("El 303 del {periodo} deduce más de lo que suman las facturas"),
                    ficheros: vec![d.fichero.clone()],
                }
                .detalle(format!(
                    "{}eclara {} de base de IVA soportado ({} de cuota) y las facturas con IVA \
                     español de ese trimestre en la carpeta suman {} ({}): faltan {} de base \
                     ({} de cuota). Falta alguna factura en la carpeta, o el modelo deduce \
                     algo que no tiene factura.{nota}",
                    if sin_isp.is_empty() {
                        "D".to_owned()
                    } else {
                        format!("{sin_isp}d")
                    },
                    eur(mb),
                    eur(mi),
                    eur(base),
                    eur(iva),
                    eur(diff),
                    eur(mi - iva)
                )),
                Some(*y),
                None,
            );
            if sin_base == 0 {
                s.pedir(
                    Destinatario::Gestoria,
                    format!(
                        "La factura (o las facturas) que explica los {} de base deducidos de \
                         más en el 303 del {periodo}",
                        eur(diff)
                    ),
                );
            }
        } else if diff < -UN_EURO {
            s.add(
                Nuevo {
                    codigo: "modelos.303_iva_sin_deducir",
                    area: Area::Modelos,
                    gravedad: Gravedad::Info,
                    responsable: Responsable::Gestoria,
                    titulo: format!("Facturas con IVA del {periodo} que no se dedujeron en el 303"),
                    ficheros: vec![d.fichero.clone()],
                }
                .detalle(format!(
                    "Las facturas con IVA español del trimestre suman {} de base y el 303 \
                     deduce {}{}: {} sin deducir. Puede ser correcto (comidas, regalos, gastos \
                     personales o facturas a nombre de otro no son deducibles) o un olvido de \
                     la gestoría, que se puede deducir en los cuatro años siguientes.{nota}",
                    eur(base),
                    eur(mb),
                    if c12.is_zero() {
                        String::new()
                    } else {
                        format!(" sin la inversión del sujeto pasivo ([12]: {})", eur(c12))
                    },
                    eur(base - mb)
                )),
                Some(*y),
                None,
            );
        }
    }
    // Adquisiciones intracomunitarias: facturas sin IVA de proveedores de la UE.
    let intra: Money = recibidas
        .iter()
        .filter(|f| {
            f.iva.is_none_or(Money::is_zero) && f.emisor_nif.as_deref().is_some_and(de_la_ue)
        })
        .filter_map(|f| f.base.or(f.total))
        .sum();
    if !intra.is_zero() || !d.c(10).is_zero() {
        filas.push(vec![
            periodo.to_owned(),
            "Adquisiciones intracomunitarias: base [10]".into(),
            eur(d.c(10)),
            eur(intra),
            eur(d.c(10) - intra),
        ]);
        if (d.c(10) - intra).abs() > UN_EURO {
            s.add(
                Nuevo {
                    codigo: "modelos.303_intracomunitarias",
                    area: Area::Modelos,
                    gravedad: Gravedad::Info,
                    responsable: Responsable::Gestoria,
                    titulo: format!(
                        "Las adquisiciones intracomunitarias del 303 del {periodo} no casan con \
                         las facturas"
                    ),
                    ficheros: vec![d.fichero.clone()],
                }
                .detalle(format!(
                    "El 303 declara {} en la casilla [10] y las facturas sin IVA de \
                     proveedores de la UE de ese trimestre suman {}.",
                    eur(d.c(10)),
                    eur(intra)
                )),
                Some(*y),
                None,
            );
        }
    }
    // Inversión del sujeto pasivo: facturas de proveedores de fuera de la UE, con IVA o sin él
    // (si lo cobran, no deberían).
    let de_fuera_isp: Money = isp.iter().filter_map(|f| f.base.or(f.total)).sum();
    let con_interior = de_fuera_isp + isp_interior;
    let base_isp = if (c12 - de_fuera_isp).abs() > UN_EURO && (c12 - con_interior).abs() <= UN_EURO
    {
        con_interior
    } else {
        de_fuera_isp
    };
    // Lo que puede explicar la diferencia sin que falte nada: las facturas de fuera de la UE en
    // otra moneda, que no se cuentan (la gestoría las pasa a euros, o las toma tal cual, con un
    // margen por el cambio), y, si el 303 declara importaciones, las compras de bienes de fuera
    // de la UE, que van con su DUA en la [32] y no en la [12] (art. 97.Uno.3.º LIVA).
    let mut vistas = BTreeSet::new();
    let en_divisa_fuera: Money = exp
        .recibidas()
        .filter(|(_, f)| del_trimestre(f, *y, q) && f.moneda.is_some() && de_fuera_ue(f))
        .filter(|(doc, f)| {
            vistas.insert(match (f.emisor_clave(), f.numero.as_deref()) {
                (Some(e), Some(n)) => format!("{e}|{}", crate::libros::number_key(n)),
                _ => doc.sha.clone(),
            })
        })
        .filter_map(|(_, f)| f.total.or(f.base))
        .sum();
    let importaciones = d.c(32) + d.c(34);
    let dif = c12 - base_isp;
    let por_divisa = !en_divisa_fuera.is_zero()
        && dif > UN_EURO
        && dif <= en_divisa_fuera.apply_rate(taxpain_core::money::Rate::percent(150)) + UN_EURO;
    let por_importacion =
        importaciones.cents() > 0 && dif < -UN_EURO && -dif <= importaciones + UN_EURO;
    if !base_isp.is_zero() || !c12.is_zero() {
        filas.push(vec![
            periodo.to_owned(),
            if por_divisa {
                "Inversión del sujeto pasivo: base [12] (sin las facturas en otra moneda)".into()
            } else if por_importacion {
                "Inversión del sujeto pasivo: base [12] (con las compras importadas, que van en la \
                 [32])"
                    .into()
            } else {
                "Inversión del sujeto pasivo: base [12]".into()
            },
            eur(c12),
            eur(base_isp),
            eur(dif),
        ]);
        if dif.abs() > UN_EURO && !por_divisa && !por_importacion {
            s.add(
                Nuevo {
                    codigo: "modelos.303_inversion_sujeto_pasivo",
                    area: Area::Modelos,
                    gravedad: Gravedad::Info,
                    responsable: Responsable::Gestoria,
                    titulo: format!(
                        "La inversión del sujeto pasivo del 303 del {periodo} no casa con las \
                         facturas"
                    ),
                    ficheros: vec![d.fichero.clone()],
                }
                .detalle(format!(
                    "El 303 declara {} en la casilla [12] y las facturas de proveedores de \
                     fuera de la UE de ese trimestre suman {} (el IVA de sus servicios lo \
                     autorrepercute la empresa, art. 84.Uno.2.º LIVA).{}{}",
                    eur(c12),
                    eur(base_isp),
                    if c12 > base_isp {
                        " La [12] lleva también las compras a proveedores españoles con inversión \
                         del sujeto pasivo (obras de construcción, chatarra, móviles…, art. \
                         84.Uno.2.º LIVA): si las hay, comprueba que estén en la carpeta."
                    } else {
                        ""
                    },
                    if en_divisa_fuera.is_zero() {
                        ""
                    } else {
                        " No se cuentan las que van en otra moneda, que la gestoría habrá pasado \
                         a euros."
                    }
                )),
                Some(*y),
                None,
            );
        }
    }
}

/// El 303 casa con las facturas del trimestre contando las de proveedores de fuera de España que
/// cobran IVA español: lo deduce, y no se puede (art. 94.Tres LIVA).
fn iva_extranjero_deducido(exp: &Expediente, t: &Trimestre, facturas: &[&Factura], s: &mut Salida) {
    let iva: Money = facturas.iter().filter_map(|f| f.iva).sum();
    let cuales: Vec<String> = facturas
        .iter()
        .map(|f| {
            let quien = f
                .emisor_nombre
                .as_deref()
                .or(f.emisor_nif.as_deref())
                .unwrap_or("emisor sin nombre");
            let n = f.numero.as_deref().unwrap_or("sin número");
            format!(
                "{n} de {quien} ({} de IVA)",
                eur(f.iva.unwrap_or(Money::ZERO))
            )
        })
        .collect();
    let a_la_empresa = |f: &&&Factura| {
        f.destinatario_nif
            .as_deref()
            .is_some_and(|n| exp.es_propio(n))
    };
    let empresa = facturas.iter().filter(a_la_empresa).count();
    let entre_empresas = "entre empresas no deberían cobrarlo, porque es inversión del sujeto \
                          pasivo (art. 84.Uno.2.º LIVA)";
    let particular = "las que no llevan el NIF de la empresa son facturas a un particular (art. \
                      97.Uno LIVA)";
    let porque = match (empresa, facturas.len() - empresa) {
        (_, 0) => entre_empresas.to_owned(),
        (0, _) => particular.to_owned(),
        _ => format!("{entre_empresas}, y {particular}"),
    };
    let n = facturas.len();
    s.add(
        Nuevo {
            codigo: "modelos.303_iva_extranjero",
            area: Area::Modelos,
            gravedad: Gravedad::Info,
            responsable: Responsable::Gestoria,
            titulo: if n == 1 {
                format!(
                    "El 303 del {} deduce el IVA de 1 factura de un proveedor de fuera de España",
                    t.periodo
                )
            } else {
                format!(
                    "El 303 del {} deduce el IVA de {n} facturas de proveedores de fuera de España",
                    t.periodo
                )
            },
            ficheros: vec![t.d.fichero.clone()],
        }
        .detalle(format!(
            "Casa con las facturas del trimestre contando {}: {}. Ese IVA no se puede deducir \
             (art. 94.Tres LIVA): {porque}. Ver las facturas de proveedores de fuera de España \
             con IVA.",
            if n == 1 {
                "la de un proveedor de fuera de España con IVA español".to_owned()
            } else {
                format!(
                    "las {n} de proveedores de fuera de España con IVA español ({} de IVA)",
                    eur(iva)
                )
            },
            lista(&cuales, 6)
        )),
        Some(t.y),
        None,
    );
}

/// El IVA devengado del 303 frente a las facturas emitidas del trimestre en la carpeta (cuando
/// no hay libro de emitidas que lo cubra). Desde 2023 el régimen general tiene cinco filas de
/// base: [01] 4 %, [150] 0 %, [04] 10 %, [153] 5 % y [07] 21 % (Orden HFP/1124/2022; tipos del
/// 0 % y 5 % de los RDL 11/2022, 20/2022 y 4/2024). Las del 0 % no se comparan: no llevan
/// cuota y las facturas al 0 % no se distinguen de las exentas.
fn devengado_con_facturas(
    exp: &Expediente,
    t: &Trimestre,
    filas: &mut Vec<Vec<String>>,
    s: &mut Salida,
) {
    let (y, q, d) = (&t.y, t.q, t.d);
    let periodo = t.periodo;
    let emitidas = unicas(
        exp.emitidas()
            .filter(|(_, f)| del_trimestre(f, *y, q))
            .map(|(doc, f)| (doc.sha.as_str(), f)),
    );
    let con_iva: Vec<&&Factura> = emitidas
        .iter()
        .filter(|f| f.iva.is_some_and(|i| i.cents() > 0) && f.base.is_some())
        .collect();
    // Las ventas con IVA: las del 0 % ([150]) no llevan cuota y no se suman a las facturas.
    let (mb, cas) = casillas(d, &[1, 4, 153, 7], &[]);
    if con_iva.is_empty() && mb.is_zero() {
        return;
    }
    let base: Money = con_iva.iter().filter_map(|f| f.base).sum();
    filas.push(vec![
        periodo.to_owned(),
        format!("IVA devengado: base {cas}"),
        eur(mb),
        eur(base),
        eur(mb - base),
    ]);
    if !con_iva.is_empty() && (mb - base).abs() > UN_EURO {
        s.add(
            Nuevo {
                codigo: "modelos.303_devengado",
                area: Area::Modelos,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Gestoria,
                titulo: format!(
                    "Las ventas del 303 del {periodo} no casan con las facturas emitidas"
                ),
                ficheros: vec![d.fichero.clone()],
            }
            .detalle(format!(
                "El 303 declara {} de base de IVA devengado y las facturas emitidas con IVA de \
                 ese trimestre en la carpeta suman {}.",
                eur(mb),
                eur(base)
            )),
            Some(*y),
            None,
        );
    }
}

/// El 349 frente a las casillas intracomunitarias del 303 del mismo trimestre: [10]
/// adquisiciones y [59] entregas y servicios. Algunas gestorías ponen los servicios a empresas
/// de la UE en la [61] (hasta 2020) o en la [120] (operaciones no sujetas por reglas de
/// localización): si casa sumándolas, también vale.
fn m349(dec: &BTreeMap<Clave, Declarado>, s: &mut Salida) {
    for ((m, y, p), d) in dec {
        if m != "349" || trimestre_de(p).is_none() {
            continue;
        }
        let Some(m303) = dec.get(&("303".to_owned(), *y, p.clone())) else {
            continue;
        };
        let total = d.c(2);
        let del_303 = m303.c(10) + m303.c(59);
        if [del_303, del_303 + m303.c(61), del_303 + m303.c(120)]
            .iter()
            .any(|c| (total - *c).abs() <= UN_EURO)
        {
            continue;
        }
        s.add(
            Nuevo {
                codigo: "modelos.349_303",
                area: Area::Modelos,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Gestoria,
                titulo: format!("El 349 del {p} {y} no casa con el 303"),
                ficheros: vec![d.fichero.clone(), m303.fichero.clone()],
            }
            .detalle(format!(
                "El 349 declara {} de operaciones intracomunitarias y el 303 del mismo periodo, {} \
                 ([10] adquisiciones + [59] entregas). Tienen que coincidir.",
                eur(total),
                eur(del_303)
            )),
            Some(*y),
            None,
        );
    }
}

/// Más de esto en entregas y servicios a la UE, en el trimestre o en alguno de los cuatro
/// anteriores, y el 349 es mensual (art. 81.2.2.º RIVA).
const UMBRAL_349: Money = Money::from_euros(50_000, 0);

/// El trimestre anterior y el siguiente.
fn anterior((y, q): (i32, u8)) -> (i32, u8) {
    if q == 1 { (y - 1, 4) } else { (y, q - 1) }
}

fn siguiente((y, q): (i32, u8)) -> (i32, u8) {
    if q == 4 { (y + 1, 1) } else { (y, q + 1) }
}

/// Lo que queda por compensar o devolver después del 303 de un trimestre: el resultado ([71]) si
/// sale negativo más lo pendiente de periodos anteriores ([87]). `None` si no está ese 303 o no
/// trae su resultado.
fn por_compensar(dec: &BTreeMap<Clave, Declarado>, (y, q): (i32, u8)) -> Option<Money> {
    let d = dec.get(&("303".to_owned(), y, format!("{q}T")))?;
    d.casillas
        .contains_key(&71)
        .then(|| (-d.c(71)).max(Money::ZERO) + d.c(87))
}

/// Lo que habría que ingresar si se deduce menos en cada trimestre: nada mientras lo cubra lo que
/// queda por compensar o devolver, en ese 303 y en los siguientes que haya (corregido, queda
/// menos por compensar). `None` si falta el 303 de alguno de esos trimestres o su resultado.
pub(crate) fn a_ingresar(
    dec: &BTreeMap<Clave, Declarado>,
    de_menos: &BTreeMap<(i32, u8), Money>,
) -> Option<Money> {
    let mut total = Money::ZERO;
    let mut cubierto = Money::ZERO;
    for (t, x) in de_menos {
        let mut queda = por_compensar(dec, *t)?;
        let mut sig = siguiente(*t);
        while let Some(p) = por_compensar(dec, sig) {
            queda = queda.min(p);
            sig = siguiente(sig);
        }
        let cubre = (queda - cubierto).max(Money::ZERO).min(*x);
        cubierto += cubre;
        total += *x - cubre;
    }
    Some(total)
}

/// Entregas de bienes y servicios prestados a otros países de la UE en un trimestre: las líneas
/// del 349 con esas claves (E, M, H, S y T) o, sin ellas, la [59] del 303. `None` si no se sabe.
fn a_la_ue(dec: &BTreeMap<Clave, Declarado>, (y, q): (i32, u8)) -> Option<Money> {
    let p = format!("{q}T");
    if let Some(m) = dec
        .get(&("349".to_owned(), y, p.clone()))
        .filter(|m| !m.registros.is_empty())
    {
        return Some(
            m.registros
                .iter()
                .filter(|r| {
                    r.clave
                        .as_deref()
                        .is_some_and(|c| ["E", "M", "H", "S", "T"].contains(&c))
                })
                .filter_map(|r| r.importe)
                .sum(),
        );
    }
    // Un 303 cuyas casillas no se han podido leer no dice nada (su [59] no es cero).
    dec.get(&("303".to_owned(), y, p))
        .filter(|d| d.con_casillas())
        .map(|d| d.c(59))
}

/// Hasta cuándo se puede sancionar no haber presentado mensual el 349 de un trimestre: cuatro
/// años (art. 189.2 LGT) desde el plazo del de su último mes, el día 20 del mes siguiente (el de
/// diciembre, el 30 de enero; art. 81.2.1.º y 81.3 RIVA), o el lunes si cae en fin de semana.
fn prescribe_349((y, q): (i32, u8)) -> Option<Date> {
    let plazo = if q == 4 {
        Date::from_calendar_date(y + 1, Month::January, 30).ok()?
    } else {
        Date::from_calendar_date(y, Month::try_from(q * 3 + 1).ok()?, 20).ok()?
    };
    let plazo = match plazo.weekday() {
        Weekday::Saturday => plazo + Duration::days(2),
        Weekday::Sunday => plazo + Duration::days(1),
        _ => plazo,
    };
    Date::from_calendar_date(plazo.year() + 4, plazo.month(), plazo.day()).ok()
}

/// El 349 es mensual si las entregas y los servicios a la UE del trimestre o de alguno de los
/// cuatro anteriores pasan de 50.000 € (art. 81.2.2.º RIVA): aviso de los trimestrales que
/// tenían que ser mensuales (mientras no haya prescrito la sanción) y nota de los próximos.
fn m349_mensual(dec: &BTreeMap<Clave, Declarado>, hoy: Date, s: &mut Salida) {
    let desde = revisable_desde(hoy);
    for ((m, y, p), d) in dec {
        let Some(q) = trimestre_de(p).filter(|_| m == "349" && *y >= desde) else {
            continue;
        };
        let Some(prescribe) = prescribe_349((*y, q)).filter(|f| *f >= hoy) else {
            continue;
        };
        let mut t = (*y, q);
        let mut supera = None;
        for _ in 0..5 {
            if let Some(x) = a_la_ue(dec, t).filter(|x| *x > UMBRAL_349) {
                supera = Some((t, x));
                break;
            }
            t = anterior(t);
        }
        let Some(((sy, sq), x)) = supera else {
            continue;
        };
        let porque = if (sy, sq) == (*y, q) {
            format!(
                "En el {q}T {y} las entregas y los servicios a otros países de la UE suman {}, \
                 más de 50.000 €: desde el mes en que se pasa de esa cifra el 349 se presenta cada \
                 mes, y también en los cuatro trimestres siguientes (art. 81.2.2.º RIVA).",
                eur(x)
            )
        } else {
            format!(
                "En el {sq}T {sy} las entregas y los servicios a otros países de la UE sumaron {}, \
                 más de 50.000 €: en los cuatro trimestres siguientes el 349 se presenta cada mes \
                 (art. 81.2.2.º RIVA).",
                eur(x)
            )
        };
        s.add(
            Nuevo {
                codigo: "modelos.349_mensual",
                area: Area::Modelos,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Gestoria,
                titulo: format!("El 349 del {q}T {y} tenía que ser mensual"),
                ficheros: vec![d.fichero.clone()],
            }
            .detalle(format!(
                "{porque} El del {q}T {y} se presentó trimestral: pregunta a la gestoría por qué. \
                 La sanción por no presentarlo cada mes prescribe el {} (art. 189.2 LGT).",
                fecha(prescribe)
            )),
            Some(*y),
            None,
        );
    }
    // Los próximos: el último de los cuatro trimestres anteriores al de hoy que pasa de la cifra.
    let ahora = (hoy.year(), trimestre(hoy));
    let mut t = anterior(ahora);
    for _ in 0..4 {
        if let Some(x) = a_la_ue(dec, t).filter(|x| *x > UMBRAL_349) {
            let mut hasta = t;
            for _ in 0..4 {
                hasta = siguiente(hasta);
            }
            s.add(
                Nuevo {
                    codigo: "modelos.349_mensual_proximos",
                    area: Area::Modelos,
                    gravedad: Gravedad::Info,
                    responsable: Responsable::Gestoria,
                    titulo: format!(
                        "El 349 se presenta cada mes hasta el {}T {}",
                        hasta.1, hasta.0
                    ),
                    ficheros: vec![],
                }
                .detalle(format!(
                    "En el {}T {} las entregas y los servicios a otros países de la UE sumaron {}, \
                     más de 50.000 €: hasta el {}T {} el 349 no es trimestral, sino mensual (art. \
                     81.2.2.º RIVA).",
                    t.1,
                    t.0,
                    eur(x),
                    hasta.1,
                    hasta.0
                )),
                None,
                None,
            );
            break;
        }
        t = anterior(t);
    }
}

/// Retención del 19 % de un arrendamiento de inmueble urbano (art. 101.8 LIRPF y art. 100
/// RIRPF; se declara en el 115), a menos de 0,3 puntos del tipo. Las de profesionales van al
/// 15 % o al 7 % (art. 101.5 LIRPF) y se declaran en el 111.
pub(crate) fn retencion_de_alquiler(f: &Factura) -> bool {
    let (Some(b), Some(r)) = (f.base, f.retencion) else {
        return false;
    };
    es_de_alquiler(b, r)
}

/// ¿Es la retención `r` sobre la base `b` la del 19 % de un alquiler? (ver
/// [`retencion_de_alquiler`]).
pub(crate) fn es_de_alquiler(b: Money, r: Money) -> bool {
    b.cents() > 0 && (r.cents() * 10_000 - b.cents() * 1_900).abs() <= b.cents() * 30
}

/// Facturas recibidas del trimestre con retención, separadas: (las del 111, las del 115).
fn con_retencion(exp: &Expediente, y: i32, q: u8) -> (Vec<&Factura>, Vec<&Factura>) {
    unicas(
        exp.recibidas()
            .filter(|(_, f)| del_trimestre(f, y, q))
            .map(|(doc, f)| (doc.sha.as_str(), f)),
    )
    .into_iter()
    .filter(|f| f.retencion.is_some_and(|r| r.cents() > 0))
    .partition(|f| !retencion_de_alquiler(f))
}

/// El 115 frente a las retenciones de los alquileres del trimestre ([03]).
fn m115(exp: &Expediente, dec: &BTreeMap<Clave, Declarado>, s: &mut Salida) {
    for ((m, y, p), d) in dec {
        let Some(q) = trimestre_de(p).filter(|_| m == "115") else {
            continue;
        };
        let (_, alquileres) = con_retencion(exp, *y, q);
        if alquileres.is_empty() {
            continue;
        }
        let ret: Money = alquileres.iter().filter_map(|f| f.retencion).sum();
        let declarada = d.c(3);
        if (declarada - ret).abs() <= UN_EURO {
            continue;
        }
        s.add(
            Nuevo {
                codigo: "modelos.115_retenciones",
                area: Area::Modelos,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Gestoria,
                titulo: format!("El 115 del {p} {y} no casa con las retenciones de los alquileres"),
                ficheros: vec![d.fichero.clone()],
            }
            .detalle(format!(
                "El 115 declara {} de retenciones de arrendamientos ([03]) y las facturas de \
                 alquiler con retención del 19 % de ese trimestre en la carpeta suman {}.",
                eur(declarada),
                eur(ret)
            )),
            Some(*y),
            None,
        );
    }
}

/// El 111 frente a las retenciones de las facturas recibidas del trimestre ([08] y [09],
/// actividades económicas). Las retenciones del 19 % de los alquileres no cuentan: van al 115.
/// Si el libro de recibidas que cubre el trimestre trae retenciones, el 111 se compara con el
/// libro (ver [`m111_con_libro`]): una factura del libro sin su PDF es un papel que falta, no un
/// error del modelo. Sin libro, con los PDF de la carpeta.
fn m111(exp: &Expediente, dec: &BTreeMap<Clave, Declarado>, s: &mut Salida) {
    let libros = super::iva::Libros::de(exp);
    let fuera = super::registro::recibidas_fuera_de_los_libros(exp);
    let mut sin_facturas = Vec::new();
    for ((m, y, p), d) in dec {
        let Some(q) = trimestre_de(p).filter(|_| m == "111") else {
            continue;
        };
        if let Some(en_libro) = libros.retenciones(*y, q) {
            if let Some(declarada) = retenciones_de_profesionales(d) {
                let fuera_del_libro: Vec<&Factura> = unicas(
                    fuera
                        .iter()
                        .filter(|(_, f)| del_trimestre(f, *y, q))
                        .map(|(doc, f)| (doc.sha.as_str(), *f)),
                )
                .into_iter()
                .filter(|f| f.retencion.is_some_and(|r| r.cents() > 0) && !retencion_de_alquiler(f))
                .collect();
                m111_con_libro(*y, p, d, declarada, en_libro, &fuera_del_libro, s);
            }
            continue;
        }
        let (con_ret, _) = con_retencion(exp, *y, q);
        let declarada = d.c(9);
        // Facturas del trimestre de las que no se han leído los importes: su retención no se
        // sabe, así que no se puede decir que falten.
        let sin_importes = exp
            .recibidas()
            .any(|(_, f)| del_trimestre(f, *y, q) && f.base.is_none() && f.retencion.is_none());
        if con_ret.is_empty() {
            if declarada.cents() > 0 && !sin_importes {
                sin_facturas.push(format!("{p} {y} ({})", eur(declarada)));
            }
            continue;
        }
        let ret: Money = con_ret.iter().filter_map(|f| f.retencion).sum();
        if (declarada - ret).abs() <= UN_EURO {
            continue;
        }
        s.add(
            Nuevo {
                codigo: "modelos.111_retenciones",
                area: Area::Modelos,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Gestoria,
                titulo: format!("El 111 del {p} {y} no casa con las retenciones de las facturas"),
                ficheros: vec![d.fichero.clone()],
            }
            .detalle(format!(
                "El 111 declara {} de retenciones de profesionales ([09]) y las facturas recibidas \
                 con retención de ese trimestre en la carpeta suman {}.",
                eur(declarada),
                eur(ret)
            )),
            Some(*y),
            None,
        );
    }
    if !sin_facturas.is_empty() {
        s.add(
            Nuevo {
                codigo: "modelos.111_sin_facturas",
                area: Area::Modelos,
                gravedad: Gravedad::Info,
                responsable: Responsable::Documentacion,
                titulo: format!(
                    "{} con retenciones de profesionales sin sus facturas en la carpeta",
                    plural(sin_facturas.len(), "111", "111")
                ),
                ficheros: vec![],
            }
            .detalle(format!(
                "Declaran retenciones de profesionales pero en la carpeta no hay facturas con \
                 retención de esos trimestres: {}.",
                lista(&sin_facturas, 8)
            )),
            None,
            None,
        );
    }
}

/// Las retenciones de profesionales que declara un 111 ([09]), si se saben: sin la [09] leída,
/// sólo si el total del 111 ([28], o el resultado [30] más lo ingresado antes [29]) es cero (una
/// lectura que sólo trae el total no dice cuánto es de profesionales).
fn retenciones_de_profesionales(d: &Declarado) -> Option<Money> {
    if let Some(m) = d.casillas.get(&9) {
        return Some(*m);
    }
    d.casillas
        .get(&28)
        .copied()
        .or_else(|| d.casillas.get(&30).map(|r| *r + d.c(29)))
        .filter(|t| t.is_zero())
}

/// El 111 de un trimestre frente al libro de recibidas, que trae las retenciones (`en_libro`, sin
/// las de alquileres). Si casan, el 111 está bien aunque falten PDF; pero si en la carpeta hay
/// facturas con retención del trimestre que no están en el libro (`fuera_del_libro`), ni el libro
/// ni el 111 las recogen. Si no casan, aviso, salvo que el 111 recoja también las de fuera del
/// libro (entonces lo que falla es el libro, y eso ya sale en los libros registro).
fn m111_con_libro(
    y: i32,
    p: &str,
    d: &Declarado,
    declarada: Money,
    en_libro: Money,
    fuera_del_libro: &[&Factura],
    s: &mut Salida,
) {
    let fuera: Money = fuera_del_libro.iter().filter_map(|f| f.retencion).sum();
    let facturas = plural(fuera_del_libro.len(), "factura", "facturas");
    let estan = if fuera_del_libro.len() == 1 {
        "está"
    } else {
        "están"
    };
    if (declarada - en_libro).abs() <= UN_EURO {
        if fuera <= UN_EURO {
            return;
        }
        s.add(
            Nuevo {
                codigo: "modelos.111_fuera_del_libro",
                area: Area::Modelos,
                gravedad: Gravedad::Aviso,
                responsable: Responsable::Gestoria,
                titulo: format!(
                    "El 111 del {p} {y} no recoge la retención de {facturas} que no {estan} en el \
                     libro"
                ),
                ficheros: vec![d.fichero.clone()],
            }
            .detalle(format!(
                "El 111 declara {} de retenciones de profesionales ([09]), lo mismo que el libro de \
                 recibidas, pero en la carpeta hay {facturas} de ese trimestre con retención que no \
                 {estan} en el libro: {} retenidos. Si se pagaron descontando la retención, es una \
                 retención practicada y no ingresada: hay que apuntarla en el libro e ingresarla con \
                 una complementaria del 111, con el recargo del art. 27 LGT si ya pasó el plazo.",
                eur(declarada),
                eur(fuera)
            )),
            Some(y),
            None,
        );
        return;
    }
    if (declarada - en_libro - fuera).abs() <= UN_EURO {
        return;
    }
    let mut detalle = format!(
        "El 111 declara {} de retenciones de profesionales ([09]) y las facturas del libro de \
         recibidas registradas en ese trimestre retienen {}",
        eur(declarada),
        eur(en_libro)
    );
    if fuera.cents() > 0 {
        detalle.push_str(&format!(
            " (y hay {facturas} con retención en la carpeta que no {estan} en el libro: {})",
            eur(fuera)
        ));
    }
    detalle.push_str(
        ". Las de alquileres, al 19 %, no cuentan: van al 115. Pide a la gestoría que lo revise.",
    );
    s.add(
        Nuevo {
            codigo: "modelos.111_retenciones",
            area: Area::Modelos,
            gravedad: Gravedad::Aviso,
            responsable: Responsable::Gestoria,
            titulo: format!(
                "El 111 del {p} {y} no casa con las retenciones del libro de recibidas"
            ),
            ficheros: vec![d.fichero.clone()],
        }
        .detalle(detalle),
        Some(y),
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clave(m: &str, y: i32, p: &str) -> Option<Clave> {
        Some((m.to_owned(), y, p.to_owned()))
    }

    #[test]
    fn el_modelo_por_el_nombre_del_fichero() {
        let n = modelo_por_nombre;
        assert_eq!(
            n("Modelo_303_2025_1T_justificante_3030000000001.pdf"),
            clave("303", 2025, "1T")
        );
        assert_eq!(n("303 1T 2018 EMPRESA.PDF"), clave("303", 2018, "1T"));
        assert_eq!(
            n("B12345674 - MODELO 202 2025-3P - EMPRESA DEMO SL.pdf"),
            clave("202", 2025, "3P")
        );
        assert_eq!(n("Modelo_111_3T2024_DEMO.pdf"), clave("111", 2024, "3T"));
        assert_eq!(n("2021 MOD 200 DEMO.pdf"), clave("200", 2021, "0A"));
        assert_eq!(n("190 2020 DEMO.pdf"), clave("190", 2020, "0A"));
        // Un borrador, una factura o un nombre con dos modelos no dicen qué modelo presentado es.
        assert_eq!(n("Modelo_202_2P2024_DEMO_borrador.pdf"), None);
        assert_eq!(n("Factura 115 2025 1T.pdf"), None);
        assert_eq!(n("303 y 349 1T 2025.pdf"), None);
        // Ni el cálculo de la gestoría, ni lo pendiente, ni el pago.
        assert_eq!(n("Calculo IVA modelo 303 2T 2025.pdf"), None);
        assert_eq!(n("Liquidación 303 2T 2025 pendiente.pdf"), None);
        assert_eq!(n("NRC 303 2T 2025.pdf"), None);
        assert_eq!(n("Pago 111 1T 2025.pdf"), None);
        // La autoliquidación y el pago fraccionado sí son el modelo.
        assert_eq!(
            n("Autoliquidación 303 2T 2025.pdf"),
            clave("303", 2025, "2T")
        );
        assert_eq!(
            n("Pago fraccionado 202 1P 2025.pdf"),
            clave("202", 2025, "1P")
        );
        // Un trimestral sin periodo, o un 202 con un trimestre, tampoco.
        assert_eq!(n("Modelo 303 2025.pdf"), None);
        assert_eq!(n("Modelo 202 2025 1T.pdf"), None);
    }

    #[test]
    fn la_cuota_del_200_que_sirve_de_base_al_202() {
        let d = |c: &[(u32, i64)]| Declarado {
            casillas: c.iter().map(|(k, v)| (*k, Money::from_cents(*v))).collect(),
            ..Declarado::default()
        };
        // Con la [00599], ésa.
        assert_eq!(
            cuota_200(&d(&[(552, 2_000_000), (599, 400_000)])),
            Some(Money::from_cents(400_000))
        );
        // Con pérdidas, la cuota íntegra es cero aunque no estén ni la [00599] ni la [00611].
        assert_eq!(
            cuota_200(&d(&[(552, -250_000), (603, 50_000), (1586, -100_000)])),
            Some(Money::ZERO)
        );
        // Sin la [00599], de la cuota diferencial y los pagos fraccionados.
        assert_eq!(
            cuota_200(&d(&[(601, 90_000), (611, -90_000)])),
            Some(Money::ZERO)
        );
        // Sin base, sin [00599] y sin [00611] no se sabe (no es cero).
        assert_eq!(cuota_200(&d(&[(1586, -100_000)])), None);
        // Si la liquidación no cuadró al leerla, tampoco.
        let mut dudosa = d(&[(552, 2_000_000), (599, 400_000)]);
        dudosa.dudosas.insert(599);
        assert_eq!(cuota_200(&dudosa), None);
    }

    #[test]
    fn las_casillas_dudosas_salen_de_otra_lectura_de_la_misma_declaracion() {
        let m = |c: &[(u32, i64)]| c.iter().map(|(k, v)| (*k, Money::from_cents(*v))).collect();
        let candidato = |origen, orden, presentado, casillas, dudosas: &[u32]| Candidato {
            origen,
            orden,
            justificante: None,
            anterior: None,
            tipo: crate::lectura_justificante::Tipo::Normal,
            modelo: "200".to_owned(),
            d: Declarado {
                casillas,
                presentado,
                dudosas: dudosas.iter().copied().collect(),
                ..Declarado::default()
            },
        };
        // El justificante (con su fecha) gana, pero su liquidación no cuadró; la lectura de la
        // copia de la gestoría la tiene (sin la [00603]).
        let d = vigente(vec![
            candidato(
                Origen::Carpeta,
                1,
                Some(time::macros::date!(2024 - 07 - 16)),
                m(&[(552, 1_000_000), (1586, 999)]),
                &[599, 603, 1586],
            ),
            candidato(
                Origen::Carpeta,
                2,
                None,
                m(&[(599, 210_000), (1586, -150_000)]),
                &[],
            ),
        ])
        .unwrap();
        assert_eq!(d.presentado, Some(time::macros::date!(2024 - 07 - 16)));
        assert_eq!(d.c(1586), Money::from_cents(-150_000));
        assert_eq!(d.c(599), Money::from_cents(210_000));
        assert_eq!(d.dudosas, BTreeSet::from([603]));
    }

    /// El justificante del 111 no lee el número de perceptores ([01]); la lectura de la copia de
    /// la gestoría, del mismo día, sí: se toma de ella. Una lectura sin fecha (puede ser un
    /// borrador) o que no dice lo mismo en las casillas que leen las dos no completa nada.
    #[test]
    fn las_casillas_que_no_trae_el_justificante_salen_de_la_misma_declaracion() {
        use time::macros::date;
        let m = |c: &[(u32, i64)]| c.iter().map(|(k, v)| (*k, Money::from_cents(*v))).collect();
        let candidato = |orden, justificante: Option<&str>, presentado, casillas| Candidato {
            origen: Origen::Carpeta,
            orden,
            justificante: justificante.map(str::to_owned),
            anterior: None,
            tipo: TipoDeclaracion::Normal,
            modelo: "111".to_owned(),
            d: Declarado {
                casillas,
                presentado,
                ..Declarado::default()
            },
        };
        // El justificante es la vigente: se leyó después.
        let justificante = || {
            candidato(
                2,
                Some("1110000000013"),
                Some(date!(2025 - 04 - 14)),
                m(&[(2, 300_000), (3, 45_000), (28, 45_000), (30, 45_000)]),
            )
        };
        let copia = |casillas| candidato(1, None, Some(date!(2025 - 04 - 14)), casillas);
        let d = vigente(vec![
            justificante(),
            copia(m(&[(1, 100), (2, 300_000), (3, 45_000), (30, 45_000)])),
        ])
        .unwrap();
        assert_eq!(d.c(1), Money::from_cents(100));
        assert_eq!(d.c(2), Money::from_cents(300_000));
        let d = vigente(vec![
            justificante(),
            candidato(1, None, None, m(&[(1, 300), (2, 300_000)])),
        ])
        .unwrap();
        assert!(!d.casillas.contains_key(&1));
        let d = vigente(vec![
            justificante(),
            copia(m(&[(1, 100), (2, 299_900), (30, 45_000)])),
        ])
        .unwrap();
        assert!(!d.casillas.contains_key(&1));
        assert_eq!(d.c(2), Money::from_cents(300_000));
    }

    /// El justificante de la complementaria de un 349 dice el número de la original: con la
    /// original en la carpeta, la vigente es la suma de las dos, y se juzga con la fecha de la
    /// original. Sin ella, sus totales son sólo lo que faltaba y no entra en los cruces. Una
    /// sustitutiva, en cambio, vale sola.
    #[test]
    fn la_complementaria_de_una_informativa_se_suma_a_su_original() {
        use time::macros::date;
        let operador = |nif: &str, importe: i64| Registro {
            nif: Some(nif.to_owned()),
            clave: Some("E".to_owned()),
            importe: Some(Money::from_cents(importe)),
            ..Registro::default()
        };
        let candidato =
            |orden, numero: &str, anterior: Option<&str>, tipo, presentado, casilla| Candidato {
                origen: Origen::Carpeta,
                orden,
                justificante: Some(numero.to_owned()),
                anterior: anterior.map(str::to_owned),
                tipo,
                modelo: "349".to_owned(),
                d: Declarado {
                    casillas: [(2, Money::from_cents(casilla))].into_iter().collect(),
                    presentado: Some(presentado),
                    registros: vec![operador(
                        if orden == 1 {
                            "FR12345678901"
                        } else {
                            "DE123456789"
                        },
                        casilla,
                    )],
                    num_registros: Some(1),
                    tipo,
                    ..Declarado::default()
                },
            };
        let original = || {
            candidato(
                1,
                "3490000000013",
                None,
                TipoDeclaracion::Normal,
                date!(2025 - 07 - 18),
                100_000,
            )
        };
        let segunda = |tipo| {
            candidato(
                2,
                "3490000000021",
                Some("3490000000013"),
                tipo,
                date!(2025 - 09 - 10),
                50_000,
            )
        };
        let d = vigente(vec![original(), segunda(TipoDeclaracion::Complementaria)]).unwrap();
        assert!(d.con_casillas() && d.rectifica);
        assert_eq!(d.c(2), Money::from_cents(150_000));
        assert_eq!(d.registros.len(), 2);
        assert_eq!(d.num_registros, Some(2));
        assert_eq!(d.presentado, Some(date!(2025 - 09 - 10)));
        assert_eq!(d.original, Some(date!(2025 - 07 - 18)));
        // Sin la original: no se sabe el total del periodo.
        let d = vigente(vec![segunda(TipoDeclaracion::Complementaria)]).unwrap();
        assert!(d.parcial && !d.con_casillas());
        // La sustitutiva trae el periodo entero.
        let d = vigente(vec![original(), segunda(TipoDeclaracion::Sustitutiva)]).unwrap();
        assert!(d.con_casillas());
        assert_eq!(d.c(2), Money::from_cents(50_000));
        assert_eq!(d.original, Some(date!(2025 - 07 - 18)));
    }

    /// Sondas de la fusión de las dos líneas: cada declaración anterior de un resumen
    /// informativo cuenta una sola vez, aunque esté leída dos veces o la vigente venga de la
    /// AEAT y también esté en la carpeta.
    #[test]
    fn cada_declaracion_informativa_cuenta_una_vez() {
        use time::macros::date;
        let operador = |nif: &str, importe: i64| Registro {
            nif: Some(nif.to_owned()),
            clave: Some("E".to_owned()),
            importe: Some(Money::from_cents(importe)),
            ..Registro::default()
        };
        #[allow(clippy::too_many_arguments)]
        fn cand(
            origen: Origen,
            orden: usize,
            numero: Option<&str>,
            anterior: Option<&str>,
            tipo: TipoDeclaracion,
            presentado: Option<Date>,
            importe: i64,
            registros: Vec<Registro>,
        ) -> Candidato {
            Candidato {
                origen,
                orden,
                justificante: numero.map(str::to_owned),
                anterior: anterior.map(str::to_owned),
                tipo,
                modelo: "349".to_owned(),
                d: Declarado {
                    casillas: [(2, Money::from_cents(importe))].into_iter().collect(),
                    presentado,
                    num_registros: Some(registros.len() as u32),
                    registros,
                    tipo,
                    ..Declarado::default()
                },
            }
        }
        let n = TipoDeclaracion::Normal;
        let c = TipoDeclaracion::Complementaria;
        let s = TipoDeclaracion::Sustitutiva;
        let fr = || vec![operador("FR12345678901", 100_000)];
        let de = || vec![operador("DE123456789", 50_000)];
        let it = || vec![operador("IT12345678901", 20_000)];
        let d1 = Some(date!(2025 - 07 - 18));
        let d2 = Some(date!(2025 - 09 - 10));
        let d3 = Some(date!(2025 - 10 - 20));
        let o = Origen::Carpeta;

        // La original leída dos veces (justificante y lectura de IA del mismo día, sin número):
        // se suma una vez.
        let d = vigente(vec![
            cand(o, 1, Some("3490000000013"), None, n, d1, 100_000, fr()),
            cand(o, 2, None, None, n, d1, 100_000, fr()),
            cand(
                o,
                3,
                Some("3490000000021"),
                Some("3490000000013"),
                c,
                d2,
                50_000,
                de(),
            ),
        ])
        .unwrap();
        assert_eq!(d.c(2), Money::from_cents(150_000));
        assert_eq!(d.registros.len(), 2);

        // La complementaria vigente de la AEAT y su copia en la carpeta: se suma una vez a la
        // original.
        let d = vigente(vec![
            cand(o, 1, Some("3490000000013"), None, n, d1, 100_000, fr()),
            cand(
                o,
                2,
                Some("3490000000021"),
                Some("3490000000013"),
                c,
                d2,
                50_000,
                de(),
            ),
            cand(
                Origen::Aeat,
                3,
                Some("3490000000021"),
                Some("3490000000013"),
                c,
                d2,
                50_000,
                de(),
            ),
        ])
        .unwrap();
        assert_eq!(d.c(2), Money::from_cents(150_000));
        assert_eq!(d.registros.len(), 2);
        assert_eq!(d.original, d1);

        // Original, sustitutiva y complementaria de la sustitutiva: la sustitutiva más la
        // complementaria.
        let d = vigente(vec![
            cand(o, 1, Some("3490000000013"), None, n, d1, 100_000, fr()),
            cand(
                o,
                2,
                Some("3490000000021"),
                Some("3490000000013"),
                s,
                d2,
                50_000,
                de(),
            ),
            cand(
                o,
                3,
                Some("3490000000039"),
                Some("3490000000021"),
                c,
                d3,
                20_000,
                it(),
            ),
        ])
        .unwrap();
        assert!(d.con_casillas());
        assert_eq!(d.c(2), Money::from_cents(70_000));
        assert_eq!(d.registros.len(), 2);
        assert_eq!(d.original, d1);

        // Sin tipo ni números (la línea de Sociedades): la segunda sólo trae operadores nuevos y
        // se suma; si repite alguno, la sustituye.
        let d = vigente(vec![
            cand(o, 1, None, None, n, d1, 100_000, fr()),
            cand(o, 2, None, None, n, d2, 50_000, de()),
        ])
        .unwrap();
        assert_eq!(d.c(2), Money::from_cents(150_000));
        let d = vigente(vec![
            cand(o, 1, None, None, n, d1, 100_000, fr()),
            cand(o, 2, None, None, n, d2, 120_000, fr()),
        ])
        .unwrap();
        assert_eq!(d.c(2), Money::from_cents(120_000));
        assert!(d.con_casillas());
    }

    /// La consulta a la AEAT es anterior a la autoliquidación rectificativa del 200: la AEAT
    /// sólo tiene la original, y la vigente es la rectificativa de la carpeta, presentada
    /// después. Con la misma fecha (o sin la de la carpeta), la de la AEAT.
    #[test]
    fn la_rectificativa_presentada_despues_de_la_consulta_a_la_aeat() {
        use time::macros::date;
        let cand = |origen, orden, numero: &str, presentado, c1586| Candidato {
            origen,
            orden,
            justificante: Some(numero.to_owned()),
            anterior: None,
            tipo: TipoDeclaracion::Normal,
            modelo: "200".to_owned(),
            d: Declarado {
                casillas: [(1586, Money::from_cents(c1586))].into_iter().collect(),
                presentado,
                ..Declarado::default()
            },
        };
        let aeat = || {
            cand(
                Origen::Aeat,
                1,
                "2000000000013",
                Some(date!(2026 - 07 - 24)),
                500_000,
            )
        };
        let d = vigente(vec![
            aeat(),
            cand(
                Origen::Carpeta,
                2,
                "2000000000021",
                Some(date!(2026 - 11 - 05)),
                450_000,
            ),
        ])
        .unwrap();
        assert_eq!(d.c(1586), Money::from_cents(450_000));
        assert_eq!(d.presentado, Some(date!(2026 - 11 - 05)));
        let d = vigente(vec![
            aeat(),
            cand(
                Origen::Carpeta,
                2,
                "2000000000013",
                Some(date!(2026 - 07 - 24)),
                499_900,
            ),
        ])
        .unwrap();
        assert_eq!(d.c(1586), Money::from_cents(500_000));
        let d = vigente(vec![
            aeat(),
            cand(Origen::Carpeta, 2, "2000000000021", None, 450_000),
        ])
        .unwrap();
        assert_eq!(d.c(1586), Money::from_cents(500_000));
    }

    /// En una autoliquidación la complementaria trae el periodo entero: no se suma a nada.
    #[test]
    fn la_complementaria_de_una_autoliquidacion_vale_sola() {
        use time::macros::date;
        let cand = |orden, numero: &str, anterior: Option<&str>, tipo, presentado, c28| Candidato {
            origen: Origen::Carpeta,
            orden,
            justificante: Some(numero.to_owned()),
            anterior: anterior.map(str::to_owned),
            tipo,
            modelo: "111".to_owned(),
            d: Declarado {
                casillas: [(28, Money::from_cents(c28))].into_iter().collect(),
                presentado: Some(presentado),
                tipo,
                ..Declarado::default()
            },
        };
        let d = vigente(vec![
            cand(
                1,
                "1110000000013",
                None,
                TipoDeclaracion::Normal,
                date!(2025 - 04 - 14),
                45_000,
            ),
            cand(
                2,
                "1110000000021",
                Some("1110000000013"),
                TipoDeclaracion::Complementaria,
                date!(2025 - 06 - 02),
                60_000,
            ),
        ])
        .unwrap();
        assert_eq!(d.c(28), Money::from_cents(60_000));
        assert!(d.rectifica && d.con_casillas());
        assert_eq!(d.original, Some(date!(2025 - 04 - 14)));
    }

    #[test]
    fn los_años_seguidos_van_de_uno_a_otro() {
        let a = |v: &[i32]| años_texto(&v.iter().copied().collect());
        assert_eq!(a(&[2022, 2023, 2024]), "2022 a 2024");
        assert_eq!(a(&[2022, 2024]), "2022, 2024");
        assert_eq!(a(&[2025]), "2025");
    }

    fn ciento_once(casillas: &[(u32, i64)]) -> Declarado {
        Declarado {
            casillas: casillas
                .iter()
                .map(|(k, v)| (*k, Money::from_cents(*v)))
                .collect(),
            fichero: "111.pdf".into(),
            ..Declarado::default()
        }
    }

    /// Lo que declara de profesionales un 111: la [09] si está; sin ella, cero sólo si el total
    /// es cero. Con un total que no es cero y sin la [09], no se sabe (puede ser de nóminas).
    #[test]
    fn retenciones_de_profesionales_del_111() {
        let m = Money::from_cents;
        assert_eq!(
            retenciones_de_profesionales(&ciento_once(&[(9, 4_500), (28, 9_000)])),
            Some(m(4_500))
        );
        assert_eq!(
            retenciones_de_profesionales(&ciento_once(&[(28, 0), (30, 0)])),
            Some(m(0))
        );
        assert_eq!(
            retenciones_de_profesionales(&ciento_once(&[(30, 0)])),
            Some(m(0))
        );
        assert_eq!(
            retenciones_de_profesionales(&ciento_once(&[(28, 1_516), (30, 1_516)])),
            None
        );
        // Complementaria: 0 ahora y 50 € antes; el periodo no es cero.
        assert_eq!(
            retenciones_de_profesionales(&ciento_once(&[(29, 5_000), (30, 0)])),
            None
        );
        assert_eq!(retenciones_de_profesionales(&ciento_once(&[])), None);
    }
}
