//! Extractos en tabla: hojas de cálculo, CSV y tablas HTML de cualquier banco.
//!
//! Cada hoja se recorre buscando las filas de títulos de las tablas de movimientos
//! ([`cabecera`]): las que reconocen, con un diccionario de sinónimos en varios idiomas
//! ([`diccionario`]), una columna de fecha, una de dinero (importe con signo, o cargo y abono)
//! y alguna más (concepto, saldo, divisa…). Lo que hay encima y debajo de cada tabla son los
//! datos de su cuenta ([`datos`]): el IBAN o el número de cuenta, la tarjeta, el titular, la
//! divisa, el periodo y los saldos; el banco sale de su huella ([`bancos`]). Las filas se leen
//! con [`filas`]: los movimientos, las de saldo que van entre ellos, las que siguen el concepto
//! de la de arriba; el signo que falta se saca del saldo y se dejan de más antiguo a más nuevo.
//!
//! Un libro puede traer varias cuentas (una por hoja, o varias tablas en la misma hoja, cada
//! una con sus datos encima) y una misma cuenta puede venir partida en páginas o en hojas, que
//! se juntan; una tabla con columna de divisa da un extracto por divisa.

mod bancos;
mod cabecera;
mod datos;
mod diccionario;
mod filas;
mod texto;

use taxpain_core::money::Money;

use super::{Clase, Extracto, Movimiento, iban};
use crate::import::Hoja;
use cabecera::Columnas;
use datos::Datos;
use filas::{Fila, Lectura};

/// Una tabla de movimientos de una hoja, ya leída, con los datos de su cuenta.
#[derive(Debug)]
struct Bloque {
    formato: &'static str,
    columnas: Columnas,
    lectura: Lectura,
    datos: Datos,
}

/// Formato de origen de una hoja, para enseñarlo: el CSV llega como una hoja sin nombre y las
/// tablas HTML como «tabla 1», «tabla 2»…
fn formato(h: &Hoja) -> &'static str {
    let tabla_html = h
        .nombre
        .strip_prefix("tabla ")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    if h.nombre.is_empty() {
        "CSV"
    } else if tabla_html {
        "Tabla HTML"
    } else {
        "Hoja de cálculo"
    }
}

/// Las tablas de movimientos de todas las hojas, en orden.
fn bloques(hojas: &[Hoja]) -> Vec<Bloque> {
    let mut out = Vec::new();
    // Filas de las hojas (o tablas HTML) sin tabla de movimientos: pueden ser los datos de la
    // cuenta de la tabla siguiente.
    let mut pendientes: Vec<&[String]> = Vec::new();
    let nombre_hoja: Vec<Vec<String>> = hojas.iter().map(|h| vec![h.nombre.clone()]).collect();
    for (h, nombre) in hojas.iter().zip(&nombre_hoja) {
        let cabs = cabecera::cabeceras(&h.filas);
        if cabs.is_empty() {
            pendientes.extend(h.filas.iter().map(Vec::as_slice));
            continue;
        }
        // El nombre de la hoja puede decir la cuenta («ES91…», «Cuenta …1332»).
        if !iban::en_texto(&h.nombre).is_empty() || iban::enmascarada(&h.nombre).is_some() {
            pendientes.push(nombre.as_slice());
        }
        let mut zona = 0;
        for (k, &(cab, alto)) in cabs.iter().enumerate() {
            let siguiente = cabs.get(k + 1).map(|c| c.0);
            let fin = siguiente.unwrap_or(h.filas.len());
            let Some(columnas) = cabecera::columnas(&h.filas, cab, alto, fin) else {
                continue;
            };
            let mut arriba: Vec<&[String]> = std::mem::take(&mut pendientes);
            arriba.extend(h.filas[zona.min(cab)..cab].iter().map(Vec::as_slice));
            let mut datos = datos::leer(arriba);
            // Una hoja que se llama como una divisa («USD») es la de esa divisa.
            if datos.divisa.is_none() && h.nombre.trim().len() == 3 {
                datos.divisa = datos::divisa(&h.nombre);
            }
            let cerca = datos.hasta.or(datos.generado).or(datos.desde);
            let de_libro = formato(h) == "Hoja de cálculo";
            let lectura = filas::leer(&h.filas, cab + alto, fin, &columnas, cerca, de_libro);
            // Lo de debajo de los movimientos: hasta la fila que diga otra cuenta, si viene
            // otra tabla detrás; si no, todo.
            let corte = match siguiente {
                Some(s) => (lectura.fin..s)
                    .find(|&r| datos::identifica(&h.filas[r]))
                    .unwrap_or(s),
                None => fin,
            };
            let abajo = datos::leer(
                h.filas
                    .get(lectura.fin.min(corte)..corte)
                    .unwrap_or(&[])
                    .iter()
                    .map(Vec::as_slice),
            );
            datos.completar(&abajo);
            zona = corte;
            out.push(Bloque {
                formato: formato(h),
                columnas,
                lectura,
                datos,
            });
        }
    }
    out
}

/// IBAN de una cuenta española de la que se sabe el banco, la oficina y el número, pero no los
/// dígitos de control.
fn iban_de_partes(entidad: &str, oficina: &str, numero: &str) -> Option<String> {
    const PESOS: [u32; 10] = [1, 2, 4, 8, 5, 10, 9, 7, 3, 6];
    let control = |cifras: &str| -> Option<u32> {
        let mut suma = 0;
        for (c, p) in cifras.chars().zip(PESOS) {
            suma += c.to_digit(10)? * p;
        }
        Some(match 11 - suma % 11 {
            11 => 0,
            10 => 1,
            d => d,
        })
    };
    let d1 = control(&format!("00{entidad}{oficina}"))?;
    let d2 = control(numero)?;
    iban::desde_ccc(&format!("{entidad}{oficina}{d1}{d2}{numero}"))
}

const SIN_CUENTA: &str =
    "El extracto no dice de qué cuenta es: comprueba que va a la cuenta correcta.";

/// Pone la cuenta del extracto con lo que dicen los datos.
fn identificar(e: &mut Extracto, d: &Datos, nombre: &str) {
    let entidad = d
        .entidad
        .clone()
        .or_else(|| bancos::entidad(&e.banco).map(str::to_owned));
    if e.banco.is_empty()
        && let Some(b) = entidad.as_deref().and_then(iban::nombre_entidad)
    {
        e.banco = b.to_owned();
    }
    if d.es_tarjeta
        && let Some(u) = &d.tarjeta
    {
        e.con_tarjeta(entidad.as_deref(), u);
        return;
    }
    if let Some(i) = &d.iban {
        e.con_iban(i);
        return;
    }
    if let Some((oficina, numero)) = &d.parcial {
        // La entidad para fabricar el IBAN sólo si la dice el contrato, una etiqueta o la
        // huella del banco: con un nombre suelto en el texto («… 39001 Santander») saldría el
        // IBAN de otra entidad y, al llegar el Norma 43 con el de verdad, una segunda cuenta.
        let fiable = d.entidad.is_some() || d.banco_seguro || d.banco.is_none();
        if fiable
            && let Some(i) = entidad
                .as_deref()
                .and_then(|ent| iban_de_partes(ent, oficina, numero))
        {
            e.con_iban(&i);
        } else {
            e.cuenta = format!("{oficina}{numero}");
        }
        return;
    }
    if let Some(m) = &d.enmascarada {
        e.cuenta = m.clave();
        return;
    }
    if let Some(n) = &d.numero {
        e.cuenta.clone_from(n);
        return;
    }
    if let Some(u) = &d.tarjeta {
        e.con_tarjeta(entidad.as_deref(), u);
        return;
    }
    e.cuenta = format!(
        "hoja:{}",
        if e.banco.is_empty() { nombre } else { &e.banco }
    );
    e.avisos.push(SIN_CUENTA.to_owned());
}

/// La cuenta (si va en cada fila) y la divisa de un grupo de movimientos.
type Grupo = (Option<String>, String);

/// Los extractos de una tabla (uno por cuenta y divisa).
fn extractos(nombre: &str, b: Bloque) -> Vec<Extracto> {
    let Bloque {
        formato,
        columnas: col,
        mut lectura,
        datos,
    } = b;
    let mut e = Extracto::nuevo(formato);
    if let Some(banco) = datos.banco.or_else(|| bancos::por_titulos(&col.titulos)) {
        e.banco = banco.to_owned();
    }
    // Con el IBAN a la vista (entero o enmascarado) y sin número de tarjeta es una cuenta
    // corriente (PGC: 572 «Bancos c/c», no 5201 «Deudas por crédito dispuesto»), aunque el
    // título o el pie hablen de la tarjeta.
    let cuenta_con_iban = datos.tarjeta.is_none()
        && (datos.iban.is_some()
            || datos
                .enmascarada
                .as_ref()
                .is_some_and(|m| m.prefijo.is_some()));
    e.clase = if datos.es_tarjeta && !cuenta_con_iban {
        datos.clase.unwrap_or(Clase::TarjetaCredito)
    } else if bancos::es_monedero(&e.banco) {
        Clase::Monedero
    } else {
        Clase::Cuenta
    };
    identificar(&mut e, &datos, nombre);
    if let Some(t) = &datos.titular {
        e.titular.clone_from(t);
    }
    let divisa = datos
        .divisa
        .clone()
        .or_else(|| col.divisa_titulo.clone())
        .or_else(|| lectura.simbolo.clone())
        .unwrap_or_else(|| "EUR".to_owned());
    e.divisa.clone_from(&divisa);
    let inicial = lectura.saldo_inicial.or(datos.saldo_inicial);
    filas::signos(&mut lectura, &col, inicial, &mut e.avisos);
    filas::ordenar(&mut lectura.filas);
    if lectura.sin_importe > 0 {
        e.avisos.push(super::aviso_sin_importe(lectura.sin_importe));
    }
    if lectura.fecha_ilegible > 0 {
        e.avisos
            .push(super::aviso_fechas_ilegibles(lectura.fecha_ilegible));
    }
    if lectura.descartados > 0 {
        e.avisos.push(format!(
            "{} movimientos pendientes, rechazados o anulados no se han leído: no cuentan en el \
             saldo.",
            lectura.descartados
        ));
    }
    let hay_saldos = lectura.filas.iter().any(|f| f.saldo.is_some());
    // Una tarjeta que da las compras en positivo y los pagos en negativo (como las
    // estadounidenses): aquí las compras restan.
    let positivos = lectura
        .filas
        .iter()
        .filter(|f| f.importe > Money::ZERO)
        .count();
    if matches!(e.clase, Clase::TarjetaCredito | Clase::TarjetaDebito)
        && !hay_saldos
        && lectura.filas.len() >= 3
        && positivos * 10 > lectura.filas.len() * 7
    {
        for f in &mut lectura.filas {
            f.importe = -f.importe;
        }
        e.avisos.push(
            "La tarjeta daba las compras en positivo: se les ha dado la vuelta para que resten."
                .to_owned(),
        );
    }
    e.saldo_inicial = inicial;
    e.saldo_final = lectura.saldo_final.or(datos.saldo_final);
    if let Some((_, actual)) = datos.saldo_actual {
        let ultimo = lectura.filas.iter().rev().find_map(|f| f.saldo);
        match ultimo {
            None if e.saldo_final.is_none() => e.saldo_final = Some(actual),
            Some(u) if u != actual => e.avisos.push(format!(
                "El saldo que da el extracto ({actual}) no es el del último movimiento ({u}): \
                 puede haber movimientos posteriores a los de la tabla."
            )),
            _ => {}
        }
    }
    e.desde = datos.desde;
    e.hasta = match (datos.hasta, datos.generado) {
        (Some(h), Some(g)) if g < h => Some(g),
        (h, _) => h,
    };
    // Un extracto por divisa y, si la tabla trae la cuenta en cada fila, por cuenta.
    let mut cuentas: Vec<&str> = lectura
        .filas
        .iter()
        .filter_map(|f| f.cuenta.as_deref())
        .collect();
    cuentas.sort_unstable();
    cuentas.dedup();
    let varias = cuentas.len() > 1;
    let por_fila = !datos.tiene_cuenta() || varias;
    let mut grupos: Vec<(Grupo, Vec<Fila>)> = Vec::new();
    for f in lectura.filas {
        let k = (
            f.cuenta.clone().filter(|_| por_fila),
            f.divisa.clone().unwrap_or_else(|| divisa.clone()),
        );
        match grupos.iter_mut().find(|(g, _)| *g == k) {
            Some((_, v)) => v.push(f),
            None => grupos.push((k, vec![f])),
        }
    }
    if grupos.is_empty() {
        ajustar_periodo(&mut e);
        return vec![e];
    }
    let solo = grupos.len() == 1;
    grupos
        .into_iter()
        .map(|((cuenta, d), filas)| {
            let mut x = e.clone();
            if let Some(c) = cuenta {
                if iban::valido(&c) {
                    x.con_iban(&c);
                } else {
                    x.cuenta = c;
                }
                x.avisos.retain(|a| a != SIN_CUENTA);
            }
            // Los saldos de los datos de encima son de la cuenta y la divisa principales.
            if !solo && (d != divisa || varias) {
                x.saldo_inicial = None;
                x.saldo_final = None;
            }
            let en_euros = d == "EUR";
            x.divisa = d;
            x.movimientos = movimientos(filas, en_euros);
            ajustar_periodo(&mut x);
            x
        })
        .collect()
}

/// El periodo que dice el extracto, ensanchado para que quepan todos sus movimientos.
fn ajustar_periodo(e: &mut Extracto) {
    let (Some(primero), Some(ultimo)) = (
        e.movimientos.iter().map(|m| m.fecha).min(),
        e.movimientos.iter().map(|m| m.fecha).max(),
    ) else {
        return;
    };
    e.desde = Some(e.desde.map_or(primero, |d| d.min(primero)));
    e.hasta = Some(e.hasta.map_or(ultimo, |h| h.max(ultimo)));
}

/// Los movimientos de las filas; los euros de cada uno, sólo si la cuenta no es en euros.
fn movimientos(filas: Vec<Fila>, en_euros: bool) -> Vec<Movimiento> {
    filas
        .into_iter()
        .map(|f| Movimiento {
            importe_eur: f.importe_eur.filter(|_| !en_euros),
            fecha: f.fecha,
            fecha_valor: f.fecha_valor,
            concepto: f.concepto,
            importe: f.importe,
            saldo: f.saldo,
            referencia: f.referencia,
        })
        .collect()
}

/// Junta en `a` otro trozo de la misma cuenta y divisa (otra página u otra hoja).
fn unir(a: &mut Extracto, mut b: Extracto) {
    let b_antes = match (a.movimientos.first(), b.movimientos.first()) {
        (Some(x), Some(y)) => y.fecha < x.fecha,
        _ => false,
    };
    if b_antes {
        std::mem::swap(a, &mut b);
    }
    // Ahora `a` es el trozo más antiguo y `b` el más nuevo.
    a.movimientos.append(&mut b.movimientos);
    if a.saldo_inicial.is_none() {
        a.saldo_inicial = b.saldo_inicial;
    }
    if b.saldo_final.is_some() {
        a.saldo_final = b.saldo_final;
    }
    a.desde = [a.desde, b.desde].into_iter().flatten().min();
    a.hasta = [a.hasta, b.hasta].into_iter().flatten().max();
    if a.titular.is_empty() {
        a.titular = b.titular;
    }
    if a.banco.is_empty() {
        a.banco = b.banco;
    }
    for av in b.avisos {
        if !a.avisos.contains(&av) {
            a.avisos.push(av);
        }
    }
}

/// Un CSV partido por un separador que no era: con la coma decimal y una sola cifra por
/// fila, la coma puede parecer el separador de un CSV con «;». Se juntan otra vez las celdas
/// de cada fila con el separador `antes` y se parten por `ahora`.
fn repartir(filas: &[Vec<String>], antes: char, ahora: char) -> Vec<Vec<String>> {
    let antes = antes.to_string();
    filas
        .iter()
        .map(|f| f.join(&antes).split(ahora).map(str::to_owned).collect())
        .collect()
}

/// Las hojas con los caracteres de control C1 cambiados por los de Windows-1252: un CSV en
/// Windows-1252 leído como Latin-1 trae «\u{80}» donde ponía «€».
fn sin_c1(hojas: &[Hoja]) -> Option<Vec<Hoja>> {
    let c1 = |c: char| ('\u{80}'..'\u{a0}').contains(&c);
    if !hojas
        .iter()
        .flat_map(|h| h.filas.iter().flatten())
        .any(|c| c.contains(c1))
    {
        return None;
    }
    Some(
        hojas
            .iter()
            .map(|h| Hoja {
                nombre: h.nombre.clone(),
                filas: h
                    .filas
                    .iter()
                    .map(|f| {
                        f.iter()
                            .map(|c| c.chars().map(super::marcado::windows_1252).collect())
                            .collect()
                    })
                    .collect(),
            })
            .collect(),
    )
}

/// Extractos de las hojas de un libro (uno por cuenta y divisa).
pub(crate) fn leer_hojas(nombre: &str, hojas: &[Hoja]) -> Vec<Extracto> {
    let arregladas = sin_c1(hojas);
    let hojas = arregladas.as_deref().unwrap_or(hojas);
    let out = leer_todas(nombre, hojas);
    // Un CSV sin tabla de movimientos puede estar partido por el separador que no es.
    if out.is_empty()
        && let [h] = hojas
        && h.nombre.is_empty()
    {
        // Las líneas acabadas sólo en «\r» (los Mac antiguos) llegan como una sola fila.
        if h.filas.iter().flatten().any(|c| c.contains('\r')) {
            for antes in [";", ",", "\t"] {
                let texto = h
                    .filas
                    .iter()
                    .map(|f| f.join(antes))
                    .collect::<Vec<_>>()
                    .join("\n")
                    .replace("\r\n", "\n")
                    .replace('\r', "\n");
                let otra = Hoja {
                    nombre: String::new(),
                    filas: crate::import::csv(texto.as_bytes()),
                };
                let r = leer_todas(nombre, std::slice::from_ref(&otra));
                if !r.is_empty() {
                    return r;
                }
            }
        }
        for (antes, ahora) in [
            (',', ';'),
            (',', '\t'),
            (';', ','),
            (';', '\t'),
            ('\t', ';'),
            ('\t', ','),
            (',', '|'),
            (';', '|'),
            ('\t', '|'),
        ] {
            if !h.filas.iter().flatten().any(|c| c.contains(ahora)) {
                continue;
            }
            let otra = Hoja {
                nombre: String::new(),
                filas: repartir(&h.filas, antes, ahora),
            };
            let r = leer_todas(nombre, std::slice::from_ref(&otra));
            if !r.is_empty() {
                return r;
            }
        }
    }
    out
}

fn leer_todas(nombre: &str, hojas: &[Hoja]) -> Vec<Extracto> {
    let mut previo: Option<Datos> = None;
    let mut out: Vec<Extracto> = Vec::new();
    for mut b in bloques(hojas) {
        if let Some(p) = &previo {
            b.datos.heredar(p);
        }
        previo = Some(b.datos.clone());
        for e in extractos(nombre, b) {
            match out
                .iter_mut()
                .find(|o| o.cuenta == e.cuenta && o.divisa == e.divisa)
            {
                Some(o) => unir(o, e),
                None => out.push(e),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iban_de_una_cuenta_sin_entidad() {
        assert_eq!(
            iban_de_partes("2100", "0418", "0200051332").as_deref(),
            Some("ES9121000418450200051332")
        );
    }
}
