//! Las filas de la tabla de movimientos, con las columnas de su cabecera.
//!
//! Una línea cuya columna de fecha empieza por una fecha abre un movimiento. Las líneas sin
//! fecha que hay alrededor son la continuación de su concepto: debajo de la fecha si las filas
//! van alineadas arriba, o repartidas encima y debajo si van centradas (muchos bancos centran
//! la fecha y los importes en una fila de dos o tres líneas). Lo que no es de la tabla
//! (cabeceras repetidas, pies de página, números de página, sumas) no entra.

use super::cabecera::{self, Cabecera, Tipo};
use super::maqueta::Linea;
use super::piezas::{self, Cifra, Pieza};
use crate::extractos::numeros::sin_tildes;

/// Un movimiento tal como se lee en la tabla, antes de saber el año de las fechas sin año ni
/// el signo de los importes sin signo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Fila {
    /// Fecha de la operación tal cual (se lee después con el orden de día y mes del extracto).
    pub fecha: String,
    pub valor: Option<String>,
    pub concepto: String,
    pub referencia: String,
    pub importe: Option<Cifra>,
    pub cargo: Option<Cifra>,
    pub abono: Option<Cifra>,
    pub saldo: Option<Cifra>,
    /// Marca de debe (`true`) o haber (`false`) de una columna aparte.
    pub signo: Option<bool>,
    /// Tramo de la tabla (cada cabecera abre uno) en el que está.
    pub tramo: usize,
    /// ¿Hereda la fecha del movimiento de encima (no la lleva)?
    pub heredada: bool,
}

/// Qué saldo dice una línea que no es un movimiento.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClaseSaldo {
    Inicial,
    Final,
}

/// La tabla de movimientos leída.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Tabla {
    pub filas: Vec<Fila>,
    /// Saldos de las líneas de la tabla que no son movimientos («Saldo anterior», «Saldo
    /// final»), en el orden del documento.
    pub saldos: Vec<(ClaseSaldo, Cifra)>,
    /// Tipos de columna que tiene la tabla.
    pub tipos: Vec<Tipo>,
    /// Líneas que son de la tabla (cabeceras y movimientos), para no buscar en ellas los datos
    /// de la cuenta.
    pub usadas: Vec<bool>,
    /// Línea en la que empieza la primera cabecera.
    pub primera: usize,
    /// Línea en la que empieza cada tramo de la tabla (la de su cabecera).
    pub tramos: Vec<usize>,
    /// ¿Se ha leído por una cabecera reconocida (o por las líneas que empiezan por fecha)?
    pub con_cabecera: bool,
    pub avisos: Vec<String>,
}

/// Qué es cada línea dentro de una tabla.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Papel {
    /// Abre un movimiento.
    Fecha,
    /// Puede ser continuación de un movimiento.
    Resto,
    /// No es de la tabla.
    Fuera,
}

/// Palabras de las líneas de saldo (sin tildes y en minúsculas).
const SALDO_INICIAL: [&str; 22] = [
    "saldo anterior",
    "saldo inicial",
    "saldo de apertura",
    "saldo al inicio",
    "saldo a inicio",
    "opening balance",
    "previous balance",
    "balance brought forward",
    "brought forward",
    "starting balance",
    "beginning balance",
    "solde precedent",
    "ancien solde",
    "solde initial",
    "solde d'ouverture",
    "alter kontostand",
    "anfangssaldo",
    "saldo iniziale",
    "saldo precedente",
    "saldo vorher",
    "kontostand am anfang",
    "saldo inicio",
];
const SALDO_FINAL: [&str; 19] = [
    "saldo final",
    "saldo de cierre",
    "saldo al cierre",
    "saldo a cierre",
    "nuevo saldo",
    "closing balance",
    "ending balance",
    "new balance",
    "balance carried forward",
    "carried forward",
    "nouveau solde",
    "solde final",
    "solde de cloture",
    "neuer kontostand",
    "endsaldo",
    "saldo finale",
    "saldo nuevo",
    "kontostand am ende",
    "final balance",
];

/// ¿Dice la línea un saldo inicial o final?
pub(crate) fn clase_saldo(texto: &str) -> Option<ClaseSaldo> {
    let t = sin_tildes(texto);
    // Lo retenido o lo disponible de otra cosa no es el saldo de la cuenta.
    if t.contains("retenid") || t.contains("bloquead") || t.contains("pendiente") {
        return None;
    }
    if SALDO_INICIAL.iter().any(|s| t.contains(s)) {
        return Some(ClaseSaldo::Inicial);
    }
    if SALDO_FINAL.iter().any(|s| t.contains(s)) {
        return Some(ClaseSaldo::Final);
    }
    None
}

/// ¿Es una línea de las que no son de ninguna tabla (número de página, sumas, pie)?
pub(crate) fn es_basura(texto: &str) -> bool {
    let t = sin_tildes(texto.trim());
    let t = t.trim();
    if t.is_empty() {
        return true;
    }
    // «1/5», «2 / 3», «- 1 -», «Página 2», «Page 1 of 3», «Seite 1 von 2».
    let sin_numeros: String = t
        .chars()
        .filter(|c| !c.is_ascii_digit() && !c.is_whitespace() && !matches!(c, '/' | '-' | '.'))
        .collect();
    // Sólo números: un número de página («2», «1/5», «- 3 -»), no una fila con dos fechas y un
    // código, una fecha con su año ni un año suelto.
    if sin_numeros.is_empty() {
        let grupos: Vec<&str> = t
            .split(|c: char| !c.is_ascii_digit())
            .filter(|g| !g.is_empty())
            .collect();
        return grupos.len() <= 2 && grupos.iter().all(|g| g.len() <= 3);
    }
    if matches!(
        sin_numeros.as_str(),
        "pagina" | "pag" | "page" | "pageof" | "seite" | "seitevon" | "paginade" | "pagde" | "p"
    ) {
        return true;
    }
    const BASURA: [&str; 13] = [
        "suma y sigue",
        "sumas y siguen",
        "suma anterior",
        "continua en",
        "continued",
        "fortsetzung",
        "a transportar",
        "transporte",
        "totales",
        "subtotal",
        "documento generado",
        "generado el",
        "©",
    ];
    // «Total 1.234,56», «Total cargos»: sumas (pero «Total Gas SL» puede ser un concepto).
    let suma = t == "total"
        || (t.starts_with("total ") && t.bytes().any(|b| b.is_ascii_digit()))
        || t.starts_with("total cargos")
        || t.starts_with("total abonos");
    suma || BASURA
        .iter()
        .any(|b| t.starts_with(b) || t.contains(&format!(" {b}")))
}

/// Las piezas de una línea, cada una con su columna.
struct LineaTabla {
    piezas: Vec<(Pieza, usize)>,
}

/// La tabla de movimientos de unas líneas, si tienen una cabecera reconocible.
pub(crate) fn leer(lineas: &[Linea]) -> Option<Tabla> {
    let mut cabeceras = cabecera::buscar(lineas);
    if cabeceras.is_empty() {
        return None;
    }
    let piezas: Vec<Vec<Pieza>> = lineas.iter().map(piezas::de_linea).collect();
    let mut usadas = vec![false; lineas.len()];
    // Tramos de cada cabecera: desde la línea siguiente hasta la siguiente cabecera.
    let n = cabeceras.len();
    let tramos: Vec<(usize, usize)> = (0..n)
        .map(|k| {
            let ini = cabeceras[k].hasta + 1;
            let fin = cabeceras.get(k + 1).map_or(lineas.len(), |c| c.desde);
            (ini, fin)
        })
        .collect();
    // Los límites de las columnas, con los datos de todos los tramos que tienen la misma
    // cabecera (la repetida en cada página): así también salen bien en una página con una sola
    // fila.
    let firma = |c: &Cabecera| -> Vec<(String, i32)> {
        c.columnas
            .iter()
            .map(|x| (x.titulo.clone(), x.x0 / 30))
            .collect()
    };
    let firmas: Vec<Vec<(String, i32)>> = cabeceras.iter().map(firma).collect();
    for (k, c) in cabeceras.iter_mut().enumerate() {
        for u in usadas.iter_mut().take(c.hasta + 1).skip(c.desde) {
            *u = true;
        }
        let datos: Vec<Vec<Pieza>> = (0..n)
            .filter(|j| firmas[*j] == firmas[k])
            .flat_map(|j| {
                let (ini, fin) = tramos[j];
                piezas[ini..fin]
                    .iter()
                    .filter(|p| p.iter().any(|x| x.fecha || x.cifra.is_some()))
                    .cloned()
            })
            .collect();
        cabecera::poner_limites(c, &datos);
        corregir_por_contenido(c, &datos);
    }
    // Papel de cada línea de cada tramo.
    let mut papeles: Vec<Vec<Papel>> = Vec::with_capacity(n);
    // Las piezas de cada línea con su columna (por el número de línea).
    let mut columnas: Vec<Option<LineaTabla>> = (0..lineas.len()).map(|_| None).collect();
    let mut saldos: Vec<(usize, ClaseSaldo, Cifra)> = Vec::new();
    for (k, c) in cabeceras.iter().enumerate() {
        let (ini, fin) = tramos[k];
        let col_fecha = c.col_fecha();
        let mut ps = Vec::with_capacity(fin - ini);
        for i in ini..fin {
            let lt = LineaTabla {
                piezas: piezas[i]
                    .iter()
                    .map(|p| (p.clone(), c.columna(p.x0, p.x1)))
                    .collect(),
            };
            let texto = lineas[i].texto();
            let papel = if let Some(cl) = clase_saldo(&texto) {
                if let Some(cifra) = lt
                    .piezas
                    .iter()
                    .rev()
                    .find(|(p, col)| p.cifra.is_some() && c.tipo(*col) != Tipo::Otro)
                    .and_then(|(p, _)| p.cifra)
                {
                    saldos.push((i, cl, cifra));
                }
                Papel::Fuera
            } else if es_basura(&texto) {
                Papel::Fuera
            } else if lt
                .piezas
                .iter()
                .find(|(_, col)| Some(*col) == col_fecha)
                .is_some_and(|(p, _)| p.fecha)
            {
                // La primera pieza de la columna de la fecha es una fecha (la columna no tiene
                // por qué ser la primera de la tabla).
                Papel::Fecha
            } else {
                Papel::Resto
            };
            ps.push(papel);
            columnas[i] = Some(lt);
        }
        papeles.push(ps);
    }
    quitar_plantilla(lineas, &tramos, &mut papeles);
    let centradas = filas_centradas(lineas, &tramos, &papeles);
    // Cada fila: su tramo, la línea con la fecha y sus líneas de continuación.
    let mut bloques: Vec<(usize, usize, Vec<usize>)> = Vec::new();
    for (k, (ini, _)) in tramos.iter().enumerate() {
        let ps = &papeles[k];
        let primera_bloque = bloques.len();
        let anclas: Vec<usize> = (0..ps.len()).filter(|j| ps[*j] == Papel::Fecha).collect();
        for a in &anclas {
            bloques.push((k, ini + a, Vec::new()));
        }
        let bloque_de =
            |a: usize| primera_bloque + anclas.iter().position(|x| *x == a).unwrap_or(0);
        let separadas = |a: &Linea, b: &Linea| {
            a.pagina != b.pagina || (b.y - a.y).abs() > a.tam.max(b.tam) * 26 / 10
        };
        if centradas {
            // A la fila más cercana de la misma página, si está cerca.
            for j in 0..ps.len() {
                if ps[j] != Papel::Resto {
                    continue;
                }
                let l = &lineas[ini + j];
                let previa = anclas.iter().rev().find(|a| **a < j).copied();
                let siguiente = anclas.iter().find(|a| **a > j).copied();
                let d = |a: usize| (lineas[ini + a].y - l.y).abs();
                let cerca = match (previa, siguiente) {
                    (Some(p), Some(n)) => Some(if d(n) < d(p) { n } else { p }),
                    (Some(a), None) | (None, Some(a)) => Some(a),
                    (None, None) => None,
                };
                if let Some(a) = cerca
                    && !separadas(&lineas[ini + a], l)
                    && let Some(bl) = bloques.get_mut(bloque_de(a))
                {
                    bl.2.push(ini + j);
                }
            }
            continue;
        }
        // Alineadas arriba: debajo de su fila, sin huecos grandes. Lo que hay al principio
        // del tramo, justo debajo de una cabecera repetida, es lo que queda de la última fila
        // de la página anterior.
        let mut actual: Option<(usize, usize)> = match (k, primera_bloque) {
            (0, _) | (_, 0) => None,
            _ => Some((primera_bloque - 1, ini - 1)),
        };
        for (j, papel) in ps.iter().enumerate() {
            let i = ini + j;
            match papel {
                Papel::Fecha => actual = Some((bloque_de(j), i)),
                Papel::Fuera => actual = None,
                Papel::Resto => {
                    actual = actual.filter(|(_, u)| !separadas(&lineas[*u], &lineas[i]));
                    if let Some((b, _)) = actual {
                        if let Some(bl) = bloques.get_mut(b) {
                            bl.2.push(i);
                        }
                        actual = Some((b, i));
                    }
                }
            }
        }
    }
    // Divisa más habitual de los importes de las filas de cada tramo (o, si no la llevan, la
    // del tramo anterior): los de otra divisa (el importe original de un pago con cambio de
    // moneda) no son del movimiento.
    let mut divisas: Vec<Vec<&'static str>> = vec![Vec::new(); n];
    for (k, i, _) in &bloques {
        let Some(lt) = &columnas[*i] else {
            continue;
        };
        for (p, col) in &lt.piezas {
            if let Some(d) = p.cifra.and_then(|c| c.divisa)
                && cabeceras[*k].tipo(*col).es_dinero()
            {
                divisas[*k].push(d);
            }
        }
    }
    let mut divisa_tramo: Vec<Option<&'static str>> = Vec::with_capacity(n);
    for d in &divisas {
        let previa = divisa_tramo.last().copied().flatten();
        divisa_tramo.push(mas_repetida(d).or(previa));
    }
    let mut filas = Vec::with_capacity(bloques.len());
    for (k, i, resto) in &bloques {
        let c = &cabeceras[*k];
        let mut ls: Vec<usize> = resto.clone();
        ls.push(*i);
        ls.sort_unstable();
        let ls: Vec<(usize, &LineaTabla)> = ls
            .into_iter()
            .filter_map(|q| columnas[q].as_ref().map(|lt| (q, lt)))
            .collect();
        let fs = fila(c, *i, &ls, divisa_tramo[*k], *k);
        if !fs.is_empty() {
            for (q, _) in &ls {
                usadas[*q] = true;
            }
            filas.extend(fs);
        }
    }
    if filas.is_empty() {
        return None;
    }
    let mut tipos: Vec<Tipo> = cabeceras
        .iter()
        .flat_map(|c| c.columnas.iter().map(|x| x.tipo))
        .collect();
    tipos.sort_by_key(|t| *t as u8);
    tipos.dedup();
    // Los saldos de antes de la primera fila o de después de la última.
    let primera_fila = bloques.first().map_or(0, |b| b.1);
    let ultima_fila = bloques.last().map_or(0, |b| b.1);
    let saldos = saldos
        .into_iter()
        .filter(|(i, cl, _)| match cl {
            ClaseSaldo::Inicial => *i < primera_fila || *i > ultima_fila,
            ClaseSaldo::Final => *i > ultima_fila || *i < primera_fila,
        })
        .map(|(_, cl, c)| (cl, c))
        .collect();
    Some(Tabla {
        filas,
        saldos,
        tipos,
        usadas,
        primera: cabeceras.first().map_or(0, |c| c.desde),
        tramos: cabeceras.iter().map(|c| c.desde).collect(),
        con_cabecera: true,
        avisos: Vec::new(),
    })
}

/// Corrige el tipo de una columna por lo que hay en ella: una «Valor» con importes es el
/// importe (en portugués) y una «Valuta» con códigos de divisa es la divisa (en italiano).
fn corregir_por_contenido(c: &mut Cabecera, filas: &[Vec<Pieza>]) {
    let n = c.columnas.len();
    let mut fechas = vec![0usize; n];
    let mut cifras = vec![0usize; n];
    let mut divisas = vec![0usize; n];
    for f in filas {
        for p in f {
            let k = c.columna(p.x0, p.x1);
            if p.fecha {
                fechas[k] += 1;
            } else if p.cifra.is_some() {
                cifras[k] += 1;
            } else if p.texto.len() == 3 && piezas::divisa(&p.texto).is_some() {
                divisas[k] += 1;
            }
        }
    }
    let hay_dinero = c.columnas.iter().any(|x| x.tipo.es_dinero());
    let mut cambios = Vec::new();
    for (k, col) in c.columnas.iter().enumerate() {
        // Una columna «Cargo/Abono» o «Debe/Haber» con importes es la del importe con signo.
        if col.tipo == Tipo::Signo && cifras[k] >= 2 && !hay_dinero {
            cambios.push((k, Tipo::Importe));
            continue;
        }
        if col.tipo != Tipo::FechaValor || fechas[k] > 0 {
            continue;
        }
        if divisas[k] > cifras[k] {
            cambios.push((k, Tipo::Otro));
        } else if cifras[k] >= 2 && !hay_dinero {
            cambios.push((k, Tipo::Importe));
        }
    }
    for (k, t) in cambios {
        c.columnas[k].tipo = t;
    }
}

/// Las líneas de plantilla (cabeceras y pies de página que se repiten en muchas páginas, con
/// los números cambiados) no son de la tabla. Sólo se miran las de los márgenes: encima de la
/// primera fila o debajo de la última de su página.
fn quitar_plantilla(lineas: &[Linea], tramos: &[(usize, usize)], papeles: &mut [Vec<Papel>]) {
    let clave = |l: &Linea| -> String {
        l.texto()
            .chars()
            .map(|c| if c.is_ascii_digit() { '#' } else { c })
            .filter(|c| !c.is_whitespace())
            .collect()
    };
    // Primera y última fila de cada página.
    let mut extremos: Vec<(u32, i32, i32)> = Vec::new();
    for (k, (ini, _)) in tramos.iter().enumerate() {
        for (j, p) in papeles[k].iter().enumerate() {
            if *p != Papel::Fecha {
                continue;
            }
            let l = &lineas[ini + j];
            match extremos.iter_mut().find(|(pg, _, _)| *pg == l.pagina) {
                Some(e) => {
                    e.1 = e.1.min(l.y);
                    e.2 = e.2.max(l.y);
                }
                None => extremos.push((l.pagina, l.y, l.y)),
            }
        }
    }
    if extremos.len() < 2 {
        return;
    }
    let en_margen = |l: &Linea| {
        extremos
            .iter()
            .find(|(pg, _, _)| *pg == l.pagina)
            .is_some_and(|(_, a, b)| l.y < *a || l.y > *b)
    };
    let mut cuenta: Vec<(String, Vec<u32>)> = Vec::new();
    for (k, (ini, _)) in tramos.iter().enumerate() {
        for (j, p) in papeles[k].iter().enumerate() {
            let l = &lineas[ini + j];
            if *p != Papel::Resto || !en_margen(l) {
                continue;
            }
            let c = clave(l);
            match cuenta.iter_mut().find(|(x, _)| *x == c) {
                Some((_, pgs)) => {
                    if !pgs.contains(&l.pagina) {
                        pgs.push(l.pagina);
                    }
                }
                None => cuenta.push((c, vec![l.pagina])),
            }
        }
    }
    let minimo = extremos.len().div_ceil(2).max(2);
    for (k, (ini, _)) in tramos.iter().enumerate() {
        for j in 0..papeles[k].len() {
            let l = &lineas[ini + j];
            if papeles[k][j] != Papel::Resto || !en_margen(l) {
                continue;
            }
            let c = clave(l);
            if cuenta.iter().any(|(x, pgs)| *x == c && pgs.len() >= minimo) {
                papeles[k][j] = Papel::Fuera;
            }
        }
    }
}

/// La divisa que más se repite.
fn mas_repetida(v: &[&'static str]) -> Option<&'static str> {
    let mut mejor: Option<(&'static str, usize)> = None;
    for d in v {
        let n = v.iter().filter(|x| *x == d).count();
        if mejor.is_none_or(|(_, m)| n > m) {
            mejor = Some((d, n));
        }
    }
    mejor.map(|(d, _)| d)
}

/// ¿Van las filas centradas (con líneas de concepto encima de la fecha)? Se ve al principio de
/// las tablas: si entre la cabecera y la primera fila hay líneas pegadas a la fila y no a la
/// cabecera.
fn filas_centradas(lineas: &[Linea], tramos: &[(usize, usize)], papeles: &[Vec<Papel>]) -> bool {
    let mut si = 0;
    let mut no = 0;
    for (k, (ini, _)) in tramos.iter().enumerate() {
        let Some(a) = papeles[k].iter().position(|p| *p == Papel::Fecha) else {
            continue;
        };
        let ancla = &lineas[ini + a];
        let Some(cab) = ini.checked_sub(1).and_then(|c| lineas.get(c)) else {
            continue;
        };
        if cab.pagina != ancla.pagina {
            continue;
        }
        for j in 0..a {
            let l = &lineas[ini + j];
            if papeles[k][j] != Papel::Resto || l.pagina != ancla.pagina {
                continue;
            }
            let a_fila = ancla.y - l.y;
            let a_cab = l.y - cab.y;
            if a_fila <= ancla.tam * 26 / 10 {
                if a_fila * 10 < a_cab * 6 {
                    si += 1;
                } else {
                    no += 1;
                }
            }
        }
    }
    si > 0 && si >= no
}

/// Lo que se va juntando de un movimiento mientras se leen sus líneas.
#[derive(Default)]
struct Junta {
    fecha: Option<String>,
    valor: Option<String>,
    concepto: Vec<String>,
    referencia: Vec<String>,
    /// Importe, cargo, abono y saldo de la línea que abre el movimiento…
    dinero: [Option<Cifra>; 4],
    /// …y de las demás.
    dinero_resto: [Option<Cifra>; 4],
    signo: Option<bool>,
    heredada: bool,
}

/// Hueco de cada tipo de columna de dinero en [`Junta::dinero`].
fn hueco(t: Tipo) -> Option<usize> {
    match t {
        Tipo::Importe => Some(0),
        Tipo::Cargo => Some(1),
        Tipo::Abono => Some(2),
        Tipo::Saldo => Some(3),
        _ => None,
    }
}

/// Los movimientos de una fila de la tabla (la línea de la fecha y las de continuación, de
/// arriba abajo). Normalmente uno; más si debajo de la fecha hay líneas con su propio importe
/// (los bancos que ponen la fecha sólo en el primer movimiento del día): ésas son movimientos
/// del mismo día.
fn fila(
    c: &Cabecera,
    ancla: usize,
    ls: &[(usize, &LineaTabla)],
    divisa: Option<&'static str>,
    tramo: usize,
) -> Vec<Fila> {
    let Some(col_fecha) = c.col_fecha() else {
        return Vec::new();
    };
    let hay_concepto = c.tiene(Tipo::Concepto);
    // Un importe con una divisa que no es la de la tabla (o con divisa, si los de la tabla no
    // la llevan) es el importe original de un pago en otra moneda.
    let otra_divisa = |p: &Pieza| match (p.cifra.and_then(|c| c.divisa), divisa) {
        (Some(a), Some(b)) => a != b,
        (Some(_), None) => true,
        _ => false,
    };
    // Una línea con su propio importe en una columna de dinero, en un trozo suyo (no al final
    // de un texto, como «Comisión: 0,30 €» en la descripción).
    let con_dinero = |lt: &LineaTabla| {
        lt.piezas.iter().any(|(p, col)| {
            p.cifra.is_some()
                && !otra_divisa(p)
                && c.tipo(*col).es_dinero()
                && !lt
                    .piezas
                    .iter()
                    .any(|(o, _)| o.trozo == p.trozo && o.es_texto())
        })
    };
    let mut juntas: Vec<Junta> = vec![Junta::default()];
    let mut actual = 0;
    for (q, lt) in ls {
        let mut abre = *q == ancla;
        if abre {
            actual = 0;
        } else if *q > ancla
            && con_dinero(lt)
            && juntas[actual].dinero[..3]
                .iter()
                .chain(&juntas[actual].dinero_resto[..3])
                .any(Option::is_some)
        {
            juntas.push(Junta {
                heredada: true,
                ..Junta::default()
            });
            actual = juntas.len() - 1;
            abre = true;
        }
        let j = &mut juntas[actual];
        for (p, col) in &lt.piezas {
            let tipo = c.tipo(*col);
            if let (Some(h), Some(cifra)) = (hueco(tipo), p.cifra) {
                if otra_divisa(p) {
                    continue;
                }
                let d = if abre {
                    &mut j.dinero
                } else {
                    &mut j.dinero_resto
                };
                if d[h].is_none() {
                    d[h] = Some(cifra);
                }
                continue;
            }
            match tipo {
                Tipo::Fecha if *col == col_fecha && p.fecha && *q == ancla && j.fecha.is_none() => {
                    j.fecha = Some(p.texto.clone());
                }
                // La de valor, en su columna o detrás de la de la operación en la misma.
                Tipo::FechaValor | Tipo::Fecha if p.fecha && j.valor.is_none() => {
                    j.valor = Some(p.texto.clone());
                }
                Tipo::Signo => {
                    let t = sin_tildes(&p.texto);
                    j.signo = match t.trim() {
                        "d" | "-" | "debe" | "dr" | "db" | "cargo" | "s" | "soll" | "debit" => {
                            Some(true)
                        }
                        "h" | "+" | "haber" | "cr" | "abono" | "haben" | "credit" => Some(false),
                        _ => j.signo,
                    };
                }
                Tipo::Referencia => j.referencia.push(p.texto.clone()),
                Tipo::Otro => {
                    if !hay_concepto && p.es_texto() {
                        j.concepto.push(p.texto.clone());
                    }
                }
                Tipo::Fecha | Tipo::FechaValor => {
                    // Lo que acompaña a la fecha en su columna (el tipo de operación, por
                    // ejemplo), salvo la hora y el año que se ha ido a la línea de abajo.
                    let t = p.texto.trim();
                    let año_suelto = t.len() == 4 && t.bytes().all(|b| b.is_ascii_digit());
                    if !p.fecha && !piezas::es_hora(t) && !año_suelto && !t.starts_with('(') {
                        j.concepto.push(p.texto.clone());
                    }
                }
                _ => {
                    // Concepto, o un texto suelto en una columna de importes que no es la
                    // divisa.
                    if tipo == Tipo::Concepto || piezas::divisa(&p.texto).is_none() {
                        j.concepto.push(p.texto.clone());
                    }
                }
            }
        }
    }
    let (fecha, valor) = (juntas[0].fecha.clone(), juntas[0].valor.clone());
    let Some(fecha) = fecha else {
        return Vec::new();
    };
    juntas
        .into_iter()
        .filter_map(|mut j| {
            if j.dinero[..3].iter().all(Option::is_none) {
                // Los importes en una línea de continuación (filas con el importe abajo).
                for h in 0..4 {
                    if j.dinero[h].is_none() {
                        j.dinero[h] = j.dinero_resto[h];
                    }
                }
            }
            let [importe, cargo, abono, saldo] = j.dinero;
            if importe.is_none() && cargo.is_none() && abono.is_none() && saldo.is_none() {
                return None;
            }
            Some(Fila {
                fecha: fecha.clone(),
                valor: if j.heredada {
                    valor.clone()
                } else {
                    j.valor.or(valor.clone())
                },
                concepto: j
                    .concepto
                    .join(" ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" "),
                referencia: j.referencia.join(" "),
                importe,
                cargo,
                abono,
                saldo,
                signo: j.signo,
                tramo,
                heredada: j.heredada,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(y: i32, trozos: &[(i32, &str)]) -> Linea {
        Linea::de_trozos(1, y, trozos)
    }

    #[test]
    fn lineas_de_saldo_y_basura() {
        assert_eq!(
            clase_saldo("Saldo anterior 1.234,56"),
            Some(ClaseSaldo::Inicial)
        );
        assert_eq!(clase_saldo("Opening balance"), Some(ClaseSaldo::Inicial));
        assert_eq!(clase_saldo("Neuer Kontostand"), Some(ClaseSaldo::Final));
        assert_eq!(clase_saldo("Saldo final retenido 0,00"), None);
        assert_eq!(clase_saldo("RECIBO MES ANTERIOR"), None);
        for b in [
            "1/5",
            "Página 2",
            "Page 1 of 3",
            "- 2 -",
            "Suma y sigue 1.234,56",
            "Total",
        ] {
            assert!(es_basura(b), "{b}");
        }
        assert!(!es_basura("TRANSFERENCIA A EMPRESA DEMO SL"));
        assert!(!es_basura("Total Gas SL"));
    }

    #[test]
    fn filas_alineadas_arriba_con_continuacion() {
        let ls = vec![
            l(
                100,
                &[
                    (500, "Fecha"),
                    (1200, "Concepto"),
                    (3500, "Importe"),
                    (4500, "Saldo"),
                ],
            ),
            l(
                220,
                &[
                    (500, "02/01/2025"),
                    (1200, "RECIBO LUZ"),
                    (3500, "-50,00"),
                    (4500, "950,00"),
                ],
            ),
            l(310, &[(1200, "EMPRESA DEMO SL")]),
            l(
                420,
                &[
                    (500, "03/01/2025"),
                    (1200, "TRANSFERENCIA"),
                    (3500, "100,00"),
                    (4500, "1.050,00"),
                ],
            ),
            l(2000, &[(4800, "1/2")]),
        ];
        let t = leer(&ls).unwrap();
        assert_eq!(t.filas.len(), 2);
        assert_eq!(t.filas[0].concepto, "RECIBO LUZ EMPRESA DEMO SL");
        assert_eq!(t.filas[1].concepto, "TRANSFERENCIA");
        assert_eq!(t.filas[0].importe.map(|c| c.importe.cents()), Some(-5000));
        assert_eq!(t.filas[1].saldo.map(|c| c.importe.cents()), Some(105_000));
    }

    #[test]
    fn filas_centradas_con_concepto_encima_y_debajo() {
        let ls = vec![
            l(
                100,
                &[
                    (300, "F. CONTABLE"),
                    (2000, "CONCEPTO"),
                    (4400, "IMPORTE"),
                    (5400, "SALDO"),
                ],
            ),
            l(360, &[(2000, "TRANSFERENCIAS | EMPRESA DEMO")]),
            l(
                415,
                &[
                    (300, "10/03/2024"),
                    (4300, "500,00 EUR"),
                    (5200, "2.500,5 EUR"),
                ],
            ),
            l(470, &[(2000, "Cuota mensual")]),
            l(735, &[(2000, "RECIBO TARJETA")]),
            l(
                790,
                &[
                    (300, "10/03/2024"),
                    (4300, "-20,25 EUR"),
                    (5200, "2.480,25 EUR"),
                ],
            ),
            l(850, &[(2000, "************1234")]),
        ];
        let t = leer(&ls).unwrap();
        assert_eq!(t.filas.len(), 2);
        assert_eq!(
            t.filas[0].concepto,
            "TRANSFERENCIAS | EMPRESA DEMO Cuota mensual"
        );
        assert_eq!(t.filas[1].concepto, "RECIBO TARJETA ************1234");
        assert_eq!(t.filas[0].saldo.map(|c| c.importe.cents()), Some(250_050));
    }

    #[test]
    fn importes_de_otra_divisa_y_lineas_de_cambio() {
        let ls = vec![
            l(
                100,
                &[
                    (435, "Fecha (UTC)"),
                    (1728, "Descripción"),
                    (3449, "Dinero saliente"),
                    (4193, "Dinero entrante"),
                    (5365, "Saldo"),
                ],
            ),
            l(
                320,
                &[
                    (435, "12 ene 2024"),
                    (1728, "Proveedor Demo"),
                    (3796, "€30.00"),
                    (5305, "€970.00"),
                ],
            ),
            l(
                427,
                &[
                    (1728, "Tipo de cambio: 1 EUR = 1.10 USD, Comisión: €0.30"),
                    (3842, "$33.00"),
                ],
            ),
            l(
                612,
                &[
                    (435, "10 ene 2024"),
                    (1728, "Cliente Demo"),
                    (4469, "€1 000.00"),
                    (5245, "€1 000.00"),
                ],
            ),
        ];
        let t = leer(&ls).unwrap();
        assert_eq!(t.filas.len(), 2);
        assert_eq!(t.filas[0].cargo.map(|c| c.importe.cents()), Some(3000));
        assert!(
            t.filas[0]
                .concepto
                .starts_with("Proveedor Demo Tipo de cambio")
        );
        assert_eq!(t.filas[1].abono.map(|c| c.importe.cents()), Some(100_000));
        assert_eq!(t.filas[1].cargo, None);
    }
}
