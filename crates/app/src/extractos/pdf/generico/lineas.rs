//! Lectura por líneas, sin cabecera: cada línea que empieza por una fecha abre un movimiento y
//! sus importes van al final de la línea (o de una de las de debajo). Si los movimientos
//! llevan dos importes, el último es el saldo cuando encadena con el de la fila de al lado.
//!
//! Es la lectura de los PDF escaneados (su texto llega sin posiciones) y de las tablas sin
//! cabecera reconocible.

use super::cabecera::{self, Tipo};
use super::maqueta::Linea;
use super::piezas::{self, Cifra, Pieza};
use super::tabla::{self, ClaseSaldo, Fila, Tabla};
use crate::extractos::iban;

/// Un movimiento leído de las líneas, antes de saber qué importe es el saldo.
struct Bruta {
    fila: Fila,
    cifras: Vec<(Cifra, i32)>,
}

/// La tabla de movimientos de unas líneas sin cabecera, si hay líneas que empiezan por una
/// fecha y llevan importes.
pub(crate) fn leer(lineas: &[Linea]) -> Option<Tabla> {
    let piezas: Vec<Vec<Pieza>> = lineas.iter().map(piezas::de_linea).collect();
    let mut usadas = vec![false; lineas.len()];
    let mut saldos: Vec<(ClaseSaldo, Cifra)> = Vec::new();
    let mut brutas: Vec<(usize, Bruta)> = Vec::new();
    // La divisa de la mayoría de los importes: los de otra (el original de un pago con cambio
    // de moneda) no son del movimiento.
    let mut divisas: Vec<(&'static str, usize)> = Vec::new();
    for p in piezas.iter().flatten() {
        if let Some(d) = p.cifra.and_then(|c| c.divisa) {
            match divisas.iter_mut().find(|(x, _)| *x == d) {
                Some((_, n)) => *n += 1,
                None => divisas.push((d, 1)),
            }
        }
    }
    let divisa = divisas.iter().max_by_key(|(_, n)| *n).map(|(d, _)| *d);
    let otra = |c: &Cifra| matches!((c.divisa, divisa), (Some(a), Some(b)) if a != b);
    // La forma de fecha más habitual al principio de las líneas.
    let formas: Vec<(usize, bool)> = piezas
        .iter()
        .filter_map(|p| p.first().filter(|p| p.fecha).map(|p| forma(&p.texto)))
        .collect();
    let dominante = formas
        .iter()
        .max_by_key(|f| formas.iter().filter(|g| g == f).count())
        .copied()
        .filter(|f| formas.iter().filter(|g| *g == f).count() * 4 >= formas.len() * 3);
    // Si hay una línea con los títulos de la tabla, los movimientos empiezan debajo.
    let mut i = lineas
        .iter()
        .position(|l| es_cabecera(&l.texto()))
        .map_or(0, |c| c + 1);
    while i < lineas.len() {
        let texto = lineas[i].texto();
        if let Some(cl) = tabla::clase_saldo(&texto) {
            if let Some(c) = piezas[i]
                .iter()
                .rev()
                .find_map(|p| p.cifra.filter(|_| !p.fecha))
            {
                saldos.push((cl, c));
            }
            i += 1;
            continue;
        }
        let ps = &piezas[i];
        // Una fecha sola en su línea puede parecer un número de página («1/5»): lo es si no
        // tiene la forma de las fechas de las demás filas.
        let como_las_demas = ps
            .first()
            .is_some_and(|p| dominante == Some(forma(&p.texto)));
        if !ps.first().is_some_and(|p| p.fecha) || (tabla::es_basura(&texto) && !como_las_demas) {
            i += 1;
            continue;
        }
        // La fecha de la operación, la de valor (si va detrás) y el resto.
        let fecha = ps[0].texto.clone();
        let mut k = 1;
        let valor = ps.get(1).filter(|p| p.fecha).map(|p| {
            k = 2;
            p.texto.clone()
        });
        let mut concepto: Vec<String> = Vec::new();
        let mut cifras: Vec<(Cifra, i32)> = Vec::new();
        for p in &ps[k..] {
            match p.cifra {
                Some(c) if otra(&c) => concepto.push(p.texto.clone()),
                Some(c) if !p.fecha || !cifras.is_empty() => cifras.push((c, p.x1)),
                _ if piezas::es_hora(&p.texto) => {}
                _ => concepto.push(p.texto.clone()),
            }
        }
        // Las líneas de debajo, hasta la siguiente fecha, un hueco grande o algo que no es del
        // movimiento.
        let mut j = i + 1;
        let mut usadas_fila = vec![i];
        let tenia = !cifras.is_empty();
        while j < lineas.len() && j - i <= 8 {
            let l = &lineas[j];
            let previa = &lineas[j - 1];
            let t = l.texto();
            // Con el importe y el saldo ya leídos de las líneas de debajo, una línea sólo de
            // texto ya es otra cosa (una leyenda, un resumen).
            let con_cifras = piezas[j].iter().any(|p| p.cifra.is_some() && !p.fecha);
            // Una línea que empieza por algo con forma de fecha distinta de la de las demás
            // filas («1/3» de un plazo, debajo de filas con fechas con año) es del concepto si
            // el movimiento aún no tiene importe.
            let otra_fecha = piezas[j].first().is_some_and(|p| p.fecha)
                && cifras.is_empty()
                && dominante.is_some_and(|d| forma(&piezas[j][0].texto) != d);
            if (piezas[j].first().is_some_and(|p| p.fecha) && !otra_fecha)
                || (!tenia && cifras.len() >= 2 && !con_cifras)
                || l.pagina != previa.pagina
                || l.y - previa.y > previa.tam.max(l.tam) * 26 / 10
                || tabla::es_basura(&t)
                || tabla::clase_saldo(&t).is_some()
                || !iban::en_texto(&t).is_empty()
                || es_cabecera(&t)
            {
                break;
            }
            for p in &piezas[j] {
                match p.cifra {
                    // Los importes de debajo cuentan si la línea de la fecha no tenía.
                    Some(c) if !tenia && !p.fecha && !otra(&c) => cifras.push((c, p.x1)),
                    _ => concepto.push(p.texto.clone()),
                }
            }
            usadas_fila.push(j);
            j += 1;
        }
        if !cifras.is_empty() {
            for u in &usadas_fila {
                usadas[*u] = true;
            }
            brutas.push((
                i,
                Bruta {
                    fila: Fila {
                        fecha,
                        valor,
                        concepto: concepto.join(" "),
                        referencia: String::new(),
                        importe: None,
                        cargo: None,
                        abono: None,
                        saldo: None,
                        signo: None,
                        tramo: 0,
                        heredada: false,
                    },
                    cifras,
                },
            ));
        }
        i = j;
    }
    if brutas.is_empty() {
        return None;
    }
    let con_saldo = ultimo_es_saldo(&brutas);
    // Importes que no son el saldo: si caen en dos columnas, la de la izquierda son los
    // cargos y la de la derecha los abonos (si es al revés, lo arregla el cuadre).
    let xs: Vec<i32> = brutas
        .iter()
        .filter_map(|(_, b)| {
            let n = b.cifras.len() - usize::from(con_saldo);
            (n >= 1).then(|| b.cifras[n - 1].1)
        })
        .collect();
    let corte = dos_columnas(&xs);
    let mut filas = Vec::with_capacity(brutas.len());
    let mut tipos = vec![Tipo::Fecha, Tipo::Concepto];
    for (_, mut b) in brutas {
        let n = b.cifras.len();
        if con_saldo && n >= 2 {
            b.fila.saldo = Some(b.cifras[n - 1].0);
            let (c, x) = b.cifras[n - 2];
            match corte {
                Some(k) if x <= k => b.fila.cargo = Some(c),
                Some(_) => b.fila.abono = Some(c),
                None => b.fila.importe = Some(c),
            }
        } else if let Some((c, _)) = b.cifras.last() {
            b.fila.importe = Some(*c);
        }
        if b.fila.importe.is_none() && b.fila.cargo.is_none() && b.fila.abono.is_none() {
            continue;
        }
        filas.push(b.fila);
    }
    tipos.push(if corte.is_some() {
        Tipo::Cargo
    } else {
        Tipo::Importe
    });
    if con_saldo {
        tipos.push(Tipo::Saldo);
    }
    Some(Tabla {
        filas,
        saldos,
        tipos,
        usadas,
        primera: 0,
        tramos: vec![0],
        con_cabecera: false,
        avisos: Vec::new(),
    })
}

/// Forma de una fecha: cuántos grupos de cifras tiene y si lleva letras («21/04/2031» →
/// (3, no), «3/3» → (2, no), «3 mar 2026» → (2, sí)).
fn forma(s: &str) -> (usize, bool) {
    let grupos = s
        .split(|c: char| !c.is_ascii_digit())
        .filter(|g| !g.is_empty())
        .count();
    (grupos, s.chars().any(char::is_alphabetic))
}

/// ¿Es una línea con los títulos de una tabla de movimientos (una fecha, un importe y algo
/// más), aunque vengan separados por un solo espacio?
fn es_cabecera(t: &str) -> bool {
    let mut tipos: Vec<Tipo> = t
        .split_whitespace()
        .filter_map(cabecera::tipo_de_titulo)
        .collect();
    if !tipos.contains(&Tipo::Fecha) || !tipos.iter().any(|x| x.es_dinero()) {
        return false;
    }
    tipos.sort_by_key(|x| *x as u8);
    tipos.dedup();
    tipos.len() >= 3 && piezas::de_linea_texto(t).iter().all(|p| p.es_texto())
}

/// ¿Es el último importe de las filas el saldo? Lo es si en la mayoría de pares de filas
/// seguidas la diferencia de saldos es el importe de una de ellas (en un orden o en el otro).
fn ultimo_es_saldo(brutas: &[(usize, Bruta)]) -> bool {
    let con_dos: Vec<(i64, i64)> = brutas
        .iter()
        .filter_map(|(_, b)| {
            let n = b.cifras.len();
            (n >= 2).then(|| {
                (
                    b.cifras[n - 2].0.importe.cents().abs(),
                    b.cifras[n - 1].0.importe.cents(),
                )
            })
        })
        .collect();
    if con_dos.len() * 2 < brutas.len() || con_dos.is_empty() {
        return false;
    }
    if con_dos.len() == 1 {
        return true;
    }
    let pares = con_dos.len() - 1;
    let adelante = con_dos
        .windows(2)
        .filter(|w| (w[1].1 - w[0].1).abs() == w[1].0)
        .count();
    let atras = con_dos
        .windows(2)
        .filter(|w| (w[0].1 - w[1].1).abs() == w[0].0)
        .count();
    adelante.max(atras) * 2 >= pares
}

/// Si las posiciones (borde derecho) de los importes forman dos columnas separadas, el punto
/// que las separa.
fn dos_columnas(xs: &[i32]) -> Option<i32> {
    let mut v = xs.to_vec();
    v.sort_unstable();
    let (min, max) = (*v.first()?, *v.last()?);
    // El mayor salto entre posiciones seguidas, si es grande (más de unos tres caracteres de
    // ancho) y deja un buen grupo a cada lado.
    let (k, salto) = v
        .windows(2)
        .enumerate()
        .map(|(k, w)| (k, w[1] - w[0]))
        .max_by_key(|(_, s)| *s)?;
    let izquierda = k + 1;
    let derecha = v.len() - izquierda;
    // Cada columna, alineada a la derecha (en un texto sin columnas, los importes van donde
    // acaba cada concepto).
    let ancho_izq = v[k] - min;
    let ancho_der = max - v[k + 1];
    (salto > 150 && izquierda >= 1 && derecha >= 1 && ancho_izq <= 60 && ancho_der <= 60)
        .then(|| v[k] + salto / 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn de_texto(t: &str) -> Vec<Linea> {
        super::super::lineas_de_texto(t)
    }

    #[test]
    fn lineas_con_fecha_e_importes_al_final() {
        let t = "EMPRESA DEMO SL\n\
                 01/02/2025 RECIBO LUZ -50,00 950,00\n\
                 COMPAÑIA DEMO\n\
                 03/02/2025 TRANSFERENCIA RECIBIDA 100,00 1.050,00\n\
                 05/02/2025 COMISION 2,00 1.048,00\n";
        let tb = leer(&de_texto(t)).unwrap();
        assert_eq!(tb.filas.len(), 3);
        assert_eq!(tb.filas[0].concepto, "RECIBO LUZ COMPAÑIA DEMO");
        assert_eq!(tb.filas[0].saldo.map(|c| c.importe.cents()), Some(95_000));
        assert_eq!(tb.filas[2].importe.map(|c| c.importe.cents()), Some(200));
    }

    #[test]
    fn importe_en_la_linea_de_debajo() {
        let t = "15/03/2025 PAGO TARJETA\n\
                 COMERCIO DEMO 12,50\n\
                 16/03/2025 OTRO PAGO\n\
                 COMERCIO DOS 7,00\n";
        let tb = leer(&de_texto(t)).unwrap();
        assert_eq!(tb.filas.len(), 2);
        assert_eq!(tb.filas[0].importe.map(|c| c.importe.cents()), Some(1250));
        assert_eq!(tb.filas[0].concepto, "PAGO TARJETA COMERCIO DEMO");
        assert_eq!(tb.filas[1].saldo, None);
    }

    #[test]
    fn cargos_y_abonos_en_dos_columnas_de_texto() {
        let t = "02/01/2025  RECIBO LUZ           50,00                 950,00\n\
                 03/01/2025  TRANSFERENCIA                   100,00   1.050,00\n";
        let tb = leer(&de_texto(t)).unwrap();
        assert_eq!(tb.filas[0].cargo.map(|c| c.importe.cents()), Some(5000));
        assert_eq!(tb.filas[1].abono.map(|c| c.importe.cents()), Some(10_000));
    }
}
