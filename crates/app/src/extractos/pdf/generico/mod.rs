//! Lector genérico de extractos en PDF de cualquier banco.
//!
//! Sin saber de qué banco es, se rehace la maqueta de cada página con las posiciones del texto
//! ([`maqueta`]): líneas de arriba abajo y, en cada una, trozos separados por huecos de
//! columna, con sus fechas, importes y palabras ([`piezas`]). En ellas se busca la cabecera de
//! la tabla de movimientos por los nombres de sus columnas en diez idiomas ([`cabecera`]) y se
//! leen las filas con sus columnas ([`tabla`]): fecha, concepto, importe o cargo y abono,
//! saldo. Aparte se leen las líneas que empiezan por una fecha y acaban en importes
//! ([`lineas`]), que es lo único que se puede hacer con una tabla sin cabecera o con el texto
//! de un PDF escaneado (llega sin posiciones), y se queda la lectura que mejor cuadra. Los
//! datos de la cuenta se buscan en toda la página ([`datos`]).
//!
//! El signo de cada importe sale de su columna (cargo o abono), de su propio signo o marca
//! (`-`, `D`/`H`, `CR`/`DR`) o, si no lo lleva, del saldo de cada fila. Si con lo leído el
//! extracto no cuadra con sus saldos, se prueban otras lecturas (los signos por el saldo, las
//! columnas de cargo y abono al revés, el orden de las filas al revés) y se queda la que cuadra.

mod cabecera;
mod datos;
mod lineas;
pub(crate) mod maqueta;
mod piezas;
mod tabla;

use std::borrow::Cow;

use taxpain_core::money::Money;
use time::Date;

use self::cabecera::Tipo;
use self::maqueta::{Linea, Palabra, Trozo};
use self::tabla::{ClaseSaldo, Tabla};
use crate::error::{AppError, Result};
use crate::extractos::numeros::{self, Orden};
use crate::extractos::{Clase, Cuadre, Extracto, Movimiento, cuadre, mime};

/// Extractos de un PDF de un banco cualquiera. `texto` es el texto ya sacado (o leído con el
/// lector de texto del equipo, si el PDF es escaneado).
pub(crate) fn leer(nombre: &str, bytes: &[u8], texto: Option<&str>) -> Result<Vec<Extracto>> {
    let pdf: Cow<'_, [u8]> = if bytes.starts_with(b"%PDF") {
        Cow::Borrowed(bytes)
    } else {
        mime::pdf_dentro(bytes).map_or(Cow::Borrowed(bytes), Cow::Owned)
    };
    let con_posiciones = maqueta::letras(&pdf)
        .map(|l| maqueta::lineas(&l))
        .filter(|l| !l.is_empty());
    let (mut leido, mut descartado) = con_posiciones
        .as_deref()
        .map(leer_lineas)
        .unwrap_or_default();
    // Sin posiciones (o si con ellas no cuadra), el texto que ya se ha sacado del PDF.
    if let Some(t) = texto
        && (leido.is_empty() || nota(&leido).1 < 0)
    {
        let (por_texto, d) = leer_lineas(&lineas_de_texto(t));
        descartado |= d;
        if leido.is_empty() || nota(&por_texto) > nota(&leido) {
            leido = por_texto;
        }
    }
    match leido {
        e if !e.is_empty() => Ok(e),
        _ if con_posiciones.is_none() && texto.is_none_or(|t| t.trim().is_empty()) => {
            Err(AppError::Other(format!(
                "{nombre}: el PDF no tiene texto que leer (¿es una imagen escaneada?)"
            )))
        }
        _ if descartado => Err(AppError::Other(format!(
            "{nombre}: no parece un extracto bancario: tiene una tabla con fechas e importes, \
             pero ni el título de un extracto ni la cuenta ni saldos que cuadren (¿es una \
             factura?)"
        ))),
        _ => Err(AppError::Other(format!(
            "{nombre}: no se ha reconocido la tabla de movimientos del PDF"
        ))),
    }
}

/// Lo bien que ha salido una lectura, para comparar dos: cuántos extractos cuadran (o no
/// tienen saldos con los que comprobarlos), cuántos no cuadran (en negativo) y cuántos
/// movimientos tienen.
fn nota(es: &[Extracto]) -> (usize, i64, usize) {
    let mut buenos = 0;
    let mut malos = 0;
    for e in es {
        match e.clone().cerrar() {
            Cuadre::NoCuadra { .. } => malos -= 1,
            _ => buenos += 1,
        }
    }
    let movs = es.iter().map(|e| e.movimientos.len()).sum();
    (buenos, malos, movs)
}

/// Líneas de un texto sin posiciones (el de un PDF escaneado, por ejemplo). Cada carácter
/// cuenta como medio cuerpo de letra, así que los textos que conservan las columnas con
/// espacios se leen igual que un PDF con posiciones; los demás, por sus líneas.
fn lineas_de_texto(texto: &str) -> Vec<Linea> {
    const TAM: i32 = 90;
    const CAR: i32 = TAM / 2;
    let mut out = Vec::new();
    // Una línea en blanco separa poco (muchos lectores de texto la ponen entre las líneas de
    // un mismo movimiento); varias seguidas, ya un bloque de otro.
    let mut y: i32 = 0;
    for l in texto.lines() {
        if l.trim().is_empty() {
            y = y.saturating_add(TAM / 2);
            continue;
        }
        y = y.saturating_add(TAM * 3 / 2);
        let chars: Vec<char> = l.chars().collect();
        let mut palabras: Vec<Palabra> = Vec::new();
        let mut i = 0;
        while i < chars.len() {
            if chars[i].is_whitespace() {
                i += 1;
                continue;
            }
            let ini = i;
            while i < chars.len() && !chars[i].is_whitespace() {
                i += 1;
            }
            let x = |k: usize| {
                i32::try_from(k)
                    .unwrap_or(i32::MAX)
                    .saturating_mul(CAR)
                    .min(10_000_000)
            };
            palabras.push(Palabra {
                x0: x(ini),
                x1: x(i),
                texto: chars[ini..i].iter().collect(),
            });
        }
        // Dos o más espacios separan columnas.
        let mut trozos: Vec<Trozo> = Vec::new();
        for p in palabras {
            match trozos.last_mut() {
                Some(t) if p.x0 - t.x1 <= CAR => {
                    t.texto.push(' ');
                    t.texto.push_str(&p.texto);
                    t.x1 = p.x1;
                    t.palabras.push(p);
                }
                _ => trozos.push(Trozo {
                    x0: p.x0,
                    x1: p.x1,
                    texto: p.texto.clone(),
                    palabras: vec![p],
                }),
            }
        }
        if !trozos.is_empty() {
            out.push(Linea {
                pagina: 1,
                y,
                tam: TAM,
                trozos,
            });
        }
    }
    out
}

/// Los extractos de unas líneas, uno por cuenta y divisa (hay PDF con una sección por
/// divisa): por la tabla con cabecera y por las líneas que empiezan por una fecha, y la
/// lectura que mejor cuadra (a igualdad, la de la tabla, que sabe de qué columna es cada
/// importe).
/// Dice también si se ha descartado alguna lectura porque no parecía un extracto.
fn leer_lineas(lineas: &[Linea]) -> (Vec<Extracto>, bool) {
    let (por_tabla, d1) = tabla::leer(lineas)
        .filter(|t| !t.filas.is_empty())
        .map(|t| leer_tabla(lineas, t))
        .unwrap_or_default();
    let (por_lineas, d2) = lineas::leer(lineas)
        .map(|t| leer_tabla(lineas, t))
        .unwrap_or_default();
    let descartado = d1 || d2;
    if por_tabla.is_empty() || nota(&por_lineas) > nota(&por_tabla) {
        (por_lineas, descartado)
    } else {
        (por_tabla, descartado)
    }
}

/// Los extractos de una tabla ya leída (por su cabecera o por las líneas), los que parecen un
/// extracto, y si se ha descartado alguno.
fn leer_tabla(lineas: &[Linea], tabla: Tabla) -> (Vec<Extracto>, bool) {
    let fuera: Vec<bool> = tabla.usadas.iter().map(|u| !u).collect();
    let d = datos::leer(lineas, &fuera, tabla.primera);
    let fuera_txt: String = lineas
        .iter()
        .enumerate()
        .filter(|(i, _)| fuera.get(*i).copied().unwrap_or(true))
        .map(|(_, l)| l.texto())
        .collect::<Vec<_>>()
        .join("\n");
    // Señales de la tabla: leída por una cabecera, y con columnas que no tiene una factura
    // (cargos y abonos, saldo).
    let mut senales = datos::palabras_de_extracto(&fuera_txt);
    senales += i32::from(tabla.con_cabecera);
    if tabla.con_cabecera {
        senales +=
            i32::from(tabla.tipos.contains(&Tipo::Cargo) && tabla.tipos.contains(&Tipo::Abono));
        senales += i32::from(tabla.tipos.contains(&Tipo::Saldo));
    }
    let todos = leer_tabla_con(lineas, tabla, &fuera, d);
    let n = todos.len();
    let buenos: Vec<Extracto> = todos
        .into_iter()
        .filter(|e| parece_extracto(e, senales))
        .collect();
    let descartado = buenos.len() < n;
    (buenos, descartado)
}

/// ¿Es de verdad un extracto (y no una factura, una nómina o un modelo de impuestos con una
/// tabla con fechas e importes)? Se suman las señales: las del documento y su tabla (`senales`:
/// el título, las palabras de otros documentos en negativo, la cabecera y sus columnas), los
/// saldos que encadenan, la cuenta o la tarjeta y cuántos movimientos tiene.
fn parece_extracto(e: &Extracto, senales: i32) -> bool {
    let mut nota = senales;
    let con_saldo = e.movimientos.iter().filter(|m| m.saldo.is_some()).count();
    let cuadra = e.clone().cerrar().cuadra();
    if cuadra && con_saldo >= 2 {
        nota += 2;
    } else if cuadra && e.saldo_inicial.is_some() && e.saldo_final.is_some() {
        nota += 1;
    }
    if !e.cuenta.is_empty() {
        nota += 1;
    }
    let n = e.movimientos.len();
    nota += i32::from(n >= 3) + i32::from(n >= 10);
    nota >= 3
}

/// Los extractos de una tabla con los datos de la cuenta ya leídos.
fn leer_tabla_con(
    lineas: &[Linea],
    tabla: Tabla,
    fuera: &[bool],
    d: datos::Datos,
) -> Vec<Extracto> {
    // La cuenta y la divisa de cada tramo de la tabla: lo que dicen las líneas de encima de su
    // cabecera o, si no dicen nada, lo del tramo anterior.
    let mut contextos: Vec<(Option<String>, Option<&'static str>)> = Vec::new();
    for (k, desde) in tabla.tramos.iter().enumerate() {
        let pagina = lineas.get(*desde).map_or(0, |l| l.pagina);
        let tope = if k == 0 { 0 } else { tabla.tramos[k - 1] + 1 };
        let mut ini = *desde;
        while ini > tope && (k == 0 || lineas[ini - 1].pagina == pagina) {
            ini -= 1;
        }
        // Sin las filas de la tabla de encima (un IBAN en un concepto no es otra cuenta).
        let encima: Vec<Linea> = (ini..*desde)
            .filter(|i| fuera.get(*i).copied().unwrap_or(true))
            .map(|i| lineas[i].clone())
            .collect();
        let (iban_, divisa) = datos::contexto(&encima);
        let previo = contextos
            .last()
            .cloned()
            .unwrap_or((d.iban.clone(), d.divisa));
        contextos.push((iban_.or(previo.0), divisa.or(previo.1)));
    }
    let mut grupos: Vec<(Option<String>, Option<&'static str>)> = Vec::new();
    for f in &tabla.filas {
        if let Some(c) = contextos.get(f.tramo)
            && !grupos.contains(c)
        {
            grupos.push(c.clone());
        }
    }
    if grupos.len() <= 1 {
        let mut d = d;
        if let Some((i, dv)) = grupos.pop() {
            d.iban = i.or(d.iban);
            d.divisa = dv.or(d.divisa);
        }
        return construir(&tabla, &d).into_iter().collect();
    }
    // Varias cuentas o divisas: un extracto por cada una, sin los saldos sueltos (no se sabe
    // de cuál son).
    grupos
        .into_iter()
        .filter_map(|g| {
            let mut t = tabla.clone();
            t.filas.retain(|f| contextos.get(f.tramo) == Some(&g));
            t.saldos.clear();
            let mut d = d.clone();
            d.iban = g.0.or(d.iban);
            d.divisa = g.1.or(d.divisa);
            d.saldo_inicial = None;
            d.saldo_final = None;
            construir(&t, &d)
        })
        .collect()
}

/// Una fila con su fecha ya leída y el importe con el signo que se sabe.
#[derive(Clone, Debug)]
struct Previa {
    fecha: Date,
    valor: Option<Date>,
    concepto: String,
    referencia: String,
    /// El importe sin signo.
    bruto: Money,
    /// El importe con el signo que dice el extracto (columna, signo o marca), si lo dice.
    firmado: Option<Money>,
    /// ¿Viene de la columna de cargos (`Some(true)`) o de la de abonos (`Some(false)`)?
    columna_cargo: Option<bool>,
    saldo: Option<Money>,
}

/// Las fechas de las filas (con su año si no lo llevan) y el importe de cada una; de las que
/// traen importe pero una fecha que no se entiende, sólo cuántas son.
fn previas(t: &Tabla, d: &datos::Datos) -> (Vec<Previa>, Vec<(usize, Money)>, usize) {
    let orden = orden_de_fechas(t);
    let leer = |s: &str, año: Option<i32>| numeros::fecha(s.trim_end_matches('.'), año, orden);
    let con_año: Vec<Option<Date>> = t.filas.iter().map(|f| leer(&f.fecha, None)).collect();
    let referencia = d
        .hasta
        .or(d.referencia)
        .or_else(|| con_año.iter().flatten().max().copied());
    let mut fechas: Vec<Option<Date>> = Vec::with_capacity(t.filas.len());
    for (i, f) in t.filas.iter().enumerate() {
        let ya = con_año[i].or_else(|| {
            // La fecha conocida más cercana: la de la fila anterior, la de la siguiente o la
            // del extracto.
            let cerca = fechas
                .iter()
                .rev()
                .flatten()
                .next()
                .copied()
                .or_else(|| con_año[i..].iter().flatten().next().copied())
                .or(referencia)?;
            sin_año(&f.fecha, cerca, orden)
        });
        fechas.push(ya);
    }
    let mut out = Vec::with_capacity(t.filas.len());
    let mut solo_saldo = Vec::new();
    let mut fecha_ilegible = 0;
    for (f, fecha) in t.filas.iter().zip(fechas) {
        let Some(fecha) = fecha else {
            // Un movimiento con una fecha que no se entiende se pierde: se cuenta para avisar.
            if f.cargo.is_some() || f.abono.is_some() || f.importe.is_some() {
                fecha_ilegible += 1;
            }
            continue;
        };
        let valor = f
            .valor
            .as_deref()
            .and_then(|v| leer(v, None).or_else(|| sin_año(v, fecha, orden)));
        let saldo = f.saldo.map(|c| c.importe);
        let (bruto, firmado, columna_cargo) = match (f.cargo, f.abono, f.importe) {
            (Some(c), None, _) => (c.importe.abs(), Some(-c.importe.abs()), Some(true)),
            (None, Some(a), _) => (a.importe.abs(), Some(a.importe.abs()), Some(false)),
            (Some(c), Some(a), _) => {
                let v = a.importe.abs() - c.importe.abs();
                (v.abs(), Some(v), Some(c.importe.abs() > a.importe.abs()))
            }
            (None, None, Some(i)) => {
                let firmado = match f.signo {
                    Some(true) => Some(-i.importe.abs()),
                    Some(false) => Some(i.importe.abs()),
                    None => i.con_signo.then_some(i.importe),
                };
                (i.importe.abs(), firmado, None)
            }
            (None, None, None) => {
                if let Some(s) = saldo {
                    solo_saldo.push((out.len(), s));
                }
                continue;
            }
        };
        out.push(Previa {
            fecha,
            valor,
            concepto: f.concepto.clone(),
            referencia: f.referencia.clone(),
            bruto,
            firmado,
            columna_cargo,
            saldo,
        });
    }
    (out, solo_saldo, fecha_ilegible)
}

/// El orden de día y mes de las fechas numéricas de las filas: el que prueba alguna fecha (un
/// número mayor que 12) o, si ninguna lo prueba, el que deja las fechas en orden (subiendo o
/// bajando) con menos saltos atrás; a igualdad, día y mes.
fn orden_de_fechas(t: &Tabla) -> Orden {
    let muestras: Vec<&str> = t
        .filas
        .iter()
        .flat_map(|f| std::iter::once(f.fecha.as_str()).chain(f.valor.as_deref()))
        .collect();
    let probado = |o: Orden| {
        muestras.iter().any(|m| {
            let partes: Vec<u32> = m
                .split(['/', '-', '.'])
                .filter_map(|p| p.trim().parse().ok())
                .collect();
            match (o, partes.as_slice()) {
                (_, [a, ..]) if *a > 31 => false,
                (Orden::DiaMes, [a, b, ..]) => *a > 12 && *b <= 12,
                (Orden::MesDia, [a, b, ..]) => *b > 12 && *a <= 12,
                _ => false,
            }
        })
    };
    if probado(Orden::DiaMes) || !probado(Orden::MesDia) && !hay_ambiguas(&muestras) {
        return Orden::DiaMes;
    }
    if probado(Orden::MesDia) {
        return Orden::MesDia;
    }
    let saltos = |o: Orden| {
        let fs: Vec<Date> = t
            .filas
            .iter()
            .filter_map(|f| numeros::fecha(&f.fecha, Some(2000), o))
            .collect();
        let subiendo = fs.windows(2).filter(|w| w[1] < w[0]).count();
        let bajando = fs.windows(2).filter(|w| w[1] > w[0]).count();
        subiendo.min(bajando)
    };
    if saltos(Orden::MesDia) < saltos(Orden::DiaMes) {
        Orden::MesDia
    } else {
        Orden::DiaMes
    }
}

/// ¿Hay fechas numéricas en las que no se sabe qué es el día y qué el mes?
fn hay_ambiguas(muestras: &[&str]) -> bool {
    muestras.iter().any(|m| {
        let partes: Vec<u32> = m
            .split(['/', '-', '.'])
            .filter_map(|p| p.trim().parse().ok())
            .collect();
        matches!(partes.as_slice(), [a, b, ..] if *a <= 12 && *b <= 12 && *a != *b)
    })
}

/// Una fecha sin año («17/11», «25 sept»), con el año que la deja más cerca de `cerca`.
fn sin_año(s: &str, cerca: Date, orden: Orden) -> Option<Date> {
    let s = s.trim_end_matches('.');
    let y = cerca.year();
    [y - 1, y, y + 1]
        .iter()
        .filter_map(|a| numeros::fecha(s, Some(*a), orden))
        .min_by_key(|f| (*f - cerca).whole_days().abs())
}

/// Una forma de leer los signos y el orden de las filas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Lectura {
    /// Como dice el extracto; lo que no lleva signo, por el saldo.
    Tal,
    /// Todos los signos por el saldo.
    PorSaldo,
    /// Las columnas de cargos y abonos al revés.
    ColumnasAlReves,
    /// Todos los signos al revés (hay quien pone los cargos en positivo).
    SignosAlReves,
}

/// Los importes con signo de una lectura, en el orden del documento (`cronologico` dice si va
/// de más antiguo a más nuevo).
fn importes(
    p: &[Previa],
    lectura: Lectura,
    cronologico: bool,
    inicial: Option<Money>,
) -> Vec<Money> {
    let por_saldo = || -> Option<Vec<Option<Money>>> {
        let brutos: Vec<Money> = p.iter().map(|x| x.bruto).collect();
        let saldos: Vec<Option<Money>> = p.iter().map(|x| x.saldo).collect();
        let s = cuadre::signos_por_saldo(&brutos, &saldos, inicial)?;
        // Que valga el orden que se ha decidido (signos_por_saldo prueba los dos).
        if s.cronologico == cronologico || s.resueltas == 0 {
            Some(s.importes)
        } else {
            let n = p.len();
            Some(
                (0..n)
                    .map(|i| {
                        let anterior = if cronologico {
                            if i == 0 { inicial } else { saldos[i - 1] }
                        } else if i + 1 == n {
                            inicial
                        } else {
                            saldos[i + 1]
                        };
                        let dif = saldos[i]? - anterior?;
                        (dif.abs() == brutos[i]).then_some(dif)
                    })
                    .collect(),
            )
        }
    };
    match lectura {
        Lectura::Tal => {
            let ps = if p.iter().any(|x| x.firmado.is_none()) {
                por_saldo()
            } else {
                None
            };
            p.iter()
                .enumerate()
                .map(|(i, x)| {
                    x.firmado
                        .or_else(|| ps.as_ref().and_then(|v| v.get(i).copied().flatten()))
                        .unwrap_or(x.bruto)
                })
                .collect()
        }
        Lectura::PorSaldo => {
            let ps = por_saldo().unwrap_or_default();
            p.iter()
                .enumerate()
                .map(|(i, x)| {
                    ps.get(i)
                        .copied()
                        .flatten()
                        .or(x.firmado)
                        .unwrap_or(x.bruto)
                })
                .collect()
        }
        Lectura::ColumnasAlReves => p
            .iter()
            .map(|x| match (x.columna_cargo, x.firmado) {
                (Some(_), Some(f)) => -f,
                (_, f) => f.unwrap_or(x.bruto),
            })
            .collect(),
        Lectura::SignosAlReves => p.iter().map(|x| -x.firmado.unwrap_or(x.bruto)).collect(),
    }
}

/// Monta el extracto con una lectura y dice si cuadra.
fn montar(
    p: &[Previa],
    lectura: Lectura,
    cronologico: bool,
    base: &Extracto,
) -> (Extracto, Cuadre) {
    let inicial = base.saldo_inicial;
    let imps = importes(p, lectura, cronologico, inicial);
    let mut movs: Vec<Movimiento> = p
        .iter()
        .zip(imps)
        .map(|(x, importe)| Movimiento {
            importe_eur: None,
            fecha: x.fecha,
            fecha_valor: x.valor,
            concepto: x.concepto.clone(),
            importe,
            saldo: x.saldo,
            referencia: x.referencia.clone(),
        })
        .collect();
    if !cronologico {
        movs.reverse();
    }
    let mut e = base.clone();
    e.movimientos = movs;
    let mut prueba = e.clone();
    let c = prueba.cerrar();
    (e, c)
}

/// ¿Encadenan los saldos de las filas leídas en este orden (de más antiguo a más nuevo si
/// `cronologico`)? Cuántas filas encadenan.
fn encadenan(p: &[Previa], cronologico: bool) -> usize {
    let v: Vec<&Previa> = if cronologico {
        p.iter().collect()
    } else {
        p.iter().rev().collect()
    };
    v.windows(2)
        .filter(|w| match (w[0].saldo, w[1].saldo) {
            (Some(a), Some(b)) => (b - a).abs() == w[1].bruto,
            _ => false,
        })
        .count()
}

/// El extracto de una tabla y de los datos de la cuenta.
fn construir(t: &Tabla, d: &datos::Datos) -> Option<Extracto> {
    let (e, c) = construir_con_cuadre(t, d)?;
    // Si no cuadra y hay movimientos sin fecha propia (los de debajo de otro), puede que no
    // fueran movimientos: se prueba sin ellos.
    if !matches!(c, Cuadre::Cuadra | Cuadre::SinSaldos) && t.filas.iter().any(|f| f.heredada) {
        let mut sin = t.clone();
        sin.filas.retain(|f| !f.heredada);
        if let Some((e2, c2)) = construir_con_cuadre(&sin, d)
            && c2.cuadra()
        {
            return Some(e2);
        }
    }
    Some(e)
}

/// El extracto de una tabla, con lo que sale al comprobarlo con sus saldos.
fn construir_con_cuadre(t: &Tabla, d: &datos::Datos) -> Option<(Extracto, Cuadre)> {
    let (mut p, solo_saldo, fecha_ilegible) = previas(t, d);
    if p.is_empty() {
        return None;
    }
    let mut base = Extracto::nuevo("PDF");
    if let Some(i) = &d.iban {
        base.con_iban(i);
    }
    if let Some(c) = &d.cuenta {
        base.cuenta.clone_from(c);
    }
    if let Some((entidad, ultimos)) = &d.tarjeta {
        base.con_tarjeta(entidad.as_deref(), ultimos);
    }
    if let Some(b) = &d.banco
        && base.banco.is_empty()
    {
        base.banco.clone_from(b);
    }
    if let Some(c) = d.clase {
        base.clase = c;
    }
    if let Some(ti) = &d.titular {
        base.titular.clone_from(ti);
    }
    if let Some(dv) = d.divisa {
        base.divisa = dv.to_owned();
    }
    base.avisos.extend(t.avisos.iter().cloned());
    if fecha_ilegible > 0 {
        base.avisos
            .push(crate::extractos::aviso_fechas_ilegibles(fecha_ilegible));
    }
    if base.cuenta.is_empty() {
        base.avisos.push(
            "No se ha encontrado el número de la cuenta en el PDF: elige a qué cuenta va al \
             importarlo."
                .to_owned(),
        );
    }
    // Orden del documento: por las fechas o, si no se ve (todas del mismo día), por los saldos.
    let (primera, ultima) = (p.first().map(|x| x.fecha), p.last().map(|x| x.fecha));
    let cronologico = match (primera, ultima) {
        (Some(a), Some(b)) if a != b => a < b,
        _ => encadenan(&p, true) >= encadenan(&p, false),
    };
    // Saldos sueltos de la tabla (filas «Saldo anterior» sin importe) y de fuera de ella.
    let mut inicial = d.saldo_inicial.map(|c| c.importe);
    let mut final_ = d.saldo_final.map(|c| c.importe);
    for (cl, c) in &t.saldos {
        match cl {
            ClaseSaldo::Inicial if inicial.is_none() => inicial = Some(c.importe),
            ClaseSaldo::Final if final_.is_none() => final_ = Some(c.importe),
            _ => {}
        }
    }
    for (pos, s) in &solo_saldo {
        let al_principio = *pos == 0;
        let al_final = *pos == p.len();
        match (al_principio == cronologico, al_principio || al_final) {
            (true, true) if inicial.is_none() => inicial = Some(*s),
            (false, true) if final_.is_none() => final_ = Some(*s),
            _ => {}
        }
    }
    // Periodo: el que dice el extracto, si los movimientos caen dentro.
    let (min, max) = (
        p.iter().map(|x| x.fecha).min(),
        p.iter().map(|x| x.fecha).max(),
    );
    if let (Some(a), Some(b), Some(mn), Some(mx)) = (d.desde, d.hasta, min, max)
        && a <= mn
        && mx <= b
    {
        base.desde = Some(a);
        base.hasta = Some(b);
    }
    // Las lecturas posibles, de la más fiel a lo que dice el extracto a la menos.
    let hay_saldos = p.iter().any(|x| x.saldo.is_some());
    let es_tarjeta = matches!(base.clase, Clase::TarjetaCredito | Clase::TarjetaDebito);
    if es_tarjeta && !hay_saldos && inicial.is_none() && final_.is_none() {
        // Listados de tarjeta sin saldos: si la mayoría de importes van sin signo, son las
        // compras (cargos) y lo que lleva «-» son devoluciones.
        let sin_signo = p
            .iter()
            .filter(|x| x.columna_cargo.is_none() && x.firmado.is_none())
            .count();
        let negativos = p
            .iter()
            .filter(|x| x.firmado.is_some_and(Money::is_negative))
            .count();
        if sin_signo > negativos {
            for x in &mut p {
                if x.columna_cargo.is_some() {
                    continue;
                }
                x.firmado = Some(match x.firmado {
                    None => -x.bruto,
                    Some(v) if v.is_negative() => x.bruto,
                    Some(v) => v,
                });
            }
            base.avisos.push(
                "Los importes de la tarjeta vienen sin signo: se han tomado como compras (cargos) \
                 y los que llevan «-», como devoluciones."
                    .to_owned(),
            );
        }
    }
    let hay_columnas = p.iter().any(|x| x.columna_cargo.is_some());
    let mut lecturas = vec![(Lectura::Tal, cronologico)];
    if hay_saldos {
        lecturas.push((Lectura::PorSaldo, cronologico));
        lecturas.push((Lectura::PorSaldo, !cronologico));
    }
    if hay_columnas {
        lecturas.push((Lectura::ColumnasAlReves, cronologico));
    }
    lecturas.push((Lectura::SignosAlReves, cronologico));
    lecturas.push((Lectura::Tal, !cronologico));
    // Con los saldos sueltos y, si así no cuadra, sin ellos (pueden ser de otra cosa).
    let mut con_sueltos = base.clone();
    con_sueltos.saldo_inicial = inicial;
    con_sueltos.saldo_final = final_;
    let bases: Vec<&Extracto> = if inicial.is_some() || final_.is_some() {
        vec![&con_sueltos, &base]
    } else {
        vec![&base]
    };
    let mut primera_lectura: Option<(Extracto, Cuadre)> = None;
    for b in &bases {
        for (lectura, crono) in &lecturas {
            let (e, c) = montar(&p, *lectura, *crono, b);
            if c.cuadra() {
                let mut e = e;
                if !std::ptr::eq(*b, &con_sueltos) && (inicial.is_some() || final_.is_some()) {
                    e.avisos.push(
                        "Los saldos inicial y final que dice el PDF no cuadran con sus \
                         movimientos: se han dejado de lado."
                            .to_owned(),
                    );
                }
                return Some((e, c));
            }
            if primera_lectura.is_none() {
                primera_lectura = Some((e, c));
            }
        }
    }
    let (mut e, c) = primera_lectura?;
    if let Cuadre::NoCuadra { .. } = c {
        e.avisos.push(
            "El extracto no cuadra con sus saldos: revisa los signos y que no falte ningún \
             movimiento."
                .to_owned(),
        );
    }
    if !hay_saldos
        && !es_tarjeta
        && t.tipos.contains(&Tipo::Importe)
        && p.iter().all(|x| x.firmado.is_none())
    {
        e.avisos.push(
            "Los importes no llevan signo ni hay saldos con los que deducirlo: se han tomado \
             como ingresos."
                .to_owned(),
        );
    }
    Some((e, c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn importes(e: &Extracto) -> Vec<i64> {
        e.movimientos.iter().map(|m| m.importe.cents()).collect()
    }

    fn cerrado(mut e: Extracto) -> (Extracto, Cuadre) {
        let c = e.cerrar();
        (e, c)
    }

    #[test]
    fn texto_sin_posiciones_con_las_columnas_a_espacios() {
        // Como el de un PDF escaneado leído con un lector de texto que respeta la maqueta.
        let t = "EMPRESA DEMO SL                 IBAN ES91 2100 0418 4502 0005 1332\n\
                 \n\
                 Fecha       Concepto                     Cargo       Abono        Saldo\n\
                 02/01/2025  RECIBO LUZ                   50,00                   950,00\n\
                             COMPAÑIA DEMO\n\
                 03/01/2025  TRANSFERENCIA RECIBIDA                  100,00     1.050,00\n";
        let e = leer("escaneado.pdf", b"", Some(t)).unwrap().remove(0);
        let (e, c) = cerrado(e);
        assert_eq!(c, Cuadre::Cuadra);
        assert_eq!(importes(&e), [-5000, 10_000]);
        assert_eq!(e.movimientos[0].concepto, "RECIBO LUZ COMPAÑIA DEMO");
        assert_eq!(e.cuenta, "210004180200051332");
    }

    #[test]
    fn texto_aplanado_con_el_signo_por_el_saldo() {
        // Las columnas separadas por un solo espacio: no se sabe si el importe es un cargo o
        // un abono, lo dice el saldo. Del más nuevo al más antiguo.
        let t = "Extracto de la cuenta ES91 2100 0418 4502 0005 1332\n\
                 Fecha Concepto Importe Saldo\n\
                 20/01/2025 COMISION MANTENIMIENTO 20,00 2.434,70\n\
                 15/01/2025 TRANSFERENCIA CLIENTE DEMO 1.500,00 2.454,70\n\
                 03/01/2025 RECIBO LUZ DEMO 45,30 954,70\n\
                 ADEUDO SEPA REF 0001\n\
                 Saldo anterior 1.000,00\n";
        let e = leer("aplanado.pdf", b"", Some(t)).unwrap().remove(0);
        let (e, c) = cerrado(e);
        assert_eq!(c, Cuadre::Cuadra, "{:?}", e.avisos);
        assert_eq!(importes(&e), [-4530, 150_000, -2000]);
        assert_eq!(
            e.movimientos[0].concepto,
            "RECIBO LUZ DEMO ADEUDO SEPA REF 0001"
        );
        assert_eq!(e.saldo_inicial.map(Money::cents), Some(100_000));
    }

    #[test]
    fn texto_de_un_lector_de_texto_con_una_celda_por_linea() {
        // Como sale del lector de texto del Mac: cada celda de la tabla en su línea, de arriba
        // abajo y de izquierda a derecha.
        let t = "BANCO DEMO\nIBAN ES91 2100 0418 4502 0005 1332\nFecha\nConcepto\nImporte\nSaldo\n\
                 12/02/2025\nPAGO PROVEEDOR DEMO\n-120,00\n880,00\n\
                 10/02/2025\nTRANSFERENCIA RECIBIDA\nCLIENTE DEMO\n500,00\n1.000,00\n\
                 05/02/2025\nRECIBO TELEFONO\n-30,00\n500,00\n";
        let e = leer("escaneado.pdf", b"", Some(t)).unwrap().remove(0);
        let (e, c) = cerrado(e);
        assert_eq!(c, Cuadre::Cuadra, "{:?}", e.avisos);
        assert_eq!(importes(&e), [-3000, 50_000, -12_000]);
        assert_eq!(
            e.movimientos[1].concepto,
            "TRANSFERENCIA RECIBIDA CLIENTE DEMO"
        );
    }

    #[test]
    fn fechas_mes_dia_por_el_orden_de_las_filas() {
        // Ninguna fecha prueba si es día/mes o mes/día; leídas como mes/día van en orden.
        let t = "Date Description Amount Balance\n\
                 01/05/2025 DEMO STORE -10.00 990.00\n\
                 01/12/2025 DEMO CLIENT 200.00 1,190.00\n\
                 02/03/2025 DEMO FEE -5.00 1,185.00\n";
        let e = leer("us.pdf", b"", Some(t)).unwrap().remove(0);
        let fechas: Vec<String> = e.movimientos.iter().map(|m| m.fecha.to_string()).collect();
        assert_eq!(fechas, ["2025-01-05", "2025-01-12", "2025-02-03"]);
    }

    #[test]
    fn sin_movimientos_dice_por_que() {
        let err = leer("carta.pdf", b"", Some("Estimado cliente: le escribimos...")).unwrap_err();
        assert!(
            err.to_string()
                .contains("no se ha reconocido la tabla de movimientos")
        );
        let err = leer("vacio.pdf", b"%PDF-1.4", None).unwrap_err();
        assert!(err.to_string().contains("no tiene texto"));
    }
}

#[cfg(test)]
mod volcado {
    /// Vuelca la maqueta de los PDF de `GENERICO_VOLCAR` (rutas separadas por saltos de línea)
    /// en la carpeta `GENERICO_SALIDA`. Sólo para mirar PDF reales a mano.
    #[test]
    #[ignore]
    fn volcar_maqueta() {
        let (Ok(lista), Ok(salida)) = (
            std::env::var("GENERICO_VOLCAR"),
            std::env::var("GENERICO_SALIDA"),
        ) else {
            return;
        };
        for (i, ruta) in lista.lines().filter(|l| !l.trim().is_empty()).enumerate() {
            let bytes = std::fs::read(ruta.trim()).unwrap();
            let bytes = crate::extractos::mime::pdf_dentro(&bytes).unwrap_or(bytes);
            let mut out = String::new();
            if let Some(ls) = super::maqueta::letras(&bytes) {
                for l in super::maqueta::lineas(&ls) {
                    out.push_str(&format!("p{} y{:>5} t{:>3} |", l.pagina, l.y, l.tam));
                    for t in &l.trozos {
                        out.push_str(&format!(" [{}-{}] {} |", t.x0, t.x1, t.texto));
                    }
                    out.push('\n');
                }
            } else {
                out.push_str("SIN POSICIONES\n");
            }
            std::fs::write(format!("{salida}/{}.lay", i + 1), out).unwrap();
        }
    }

    /// Lee los PDF de `GENERICO_PDFS` y deja en `GENERICO_SALIDA` los datos de la cuenta que
    /// salen de cada uno. Sólo para mirar a mano.
    #[test]
    #[ignore]
    fn datos_de_pdfs() {
        let (Ok(lista), Ok(salida)) = (
            std::env::var("GENERICO_PDFS"),
            std::env::var("GENERICO_SALIDA"),
        ) else {
            return;
        };
        let mut out = String::new();
        for ruta in lista.lines().filter(|l| !l.trim().is_empty()) {
            let bytes = std::fs::read(ruta.trim()).unwrap();
            let bytes = crate::extractos::mime::pdf_dentro(&bytes).unwrap_or(bytes);
            let texto = crate::pdf_text::text(&bytes);
            match super::leer(ruta, &bytes, texto.as_deref()) {
                Ok(es) => {
                    for e in es {
                        out.push_str(&format!(
                            "{ruta}\n    cuenta {} · iban {:?} · banco {} · titular {} · {:?} · {} · \
                             {:?}–{:?} · avisos {:?}\n",
                            e.cuenta,
                            e.iban,
                            e.banco,
                            e.titular,
                            e.clase,
                            e.divisa,
                            e.desde,
                            e.hasta,
                            e.avisos
                        ));
                    }
                }
                Err(err) => out.push_str(&format!("{ruta}: ERROR {err}\n")),
            }
        }
        std::fs::write(salida, out).unwrap();
    }

    /// Lee sólo el texto (sin posiciones) de los ficheros de `GENERICO_TEXTOS` y deja el
    /// resumen en `GENERICO_SALIDA`. Sólo para probar a mano la lectura de los escaneados.
    #[test]
    #[ignore]
    fn leer_textos() {
        let (Ok(lista), Ok(salida)) = (
            std::env::var("GENERICO_TEXTOS"),
            std::env::var("GENERICO_SALIDA"),
        ) else {
            return;
        };
        let mut out = String::new();
        for ruta in lista.lines().filter(|l| !l.trim().is_empty()) {
            let t = std::fs::read_to_string(ruta.trim()).unwrap();
            match super::leer(ruta, b"", Some(&t)) {
                Ok(es) => {
                    for mut e in es {
                        let c = e.cerrar();
                        out.push_str(&format!(
                            "{ruta}: {} movs · {:?} → {:?} · {:?}\n",
                            e.movimientos.len(),
                            e.saldo_inicial,
                            e.saldo_final,
                            c
                        ));
                        if std::env::var("GENERICO_MOVS").is_ok() {
                            for m in &e.movimientos {
                                out.push_str(&format!(
                                    "    {} {} {:?} {}\n",
                                    m.fecha, m.importe, m.saldo, m.concepto
                                ));
                            }
                        }
                    }
                }
                Err(err) => out.push_str(&format!("{ruta}: ERROR {err}\n")),
            }
        }
        std::fs::write(salida, out).unwrap();
    }
}
