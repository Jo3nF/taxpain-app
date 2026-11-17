//! La fila de títulos de la tabla de movimientos y qué es cada columna.
//!
//! Una fila es la de títulos si reconoce, con el [`diccionario`](super::diccionario), al menos
//! una columna de fecha, una de dinero y una tercera de otra cosa (concepto, saldo, divisa…),
//! y casi no tiene celdas que parezcan datos. Las dudas que deja el título se resuelven con lo
//! que trae la columna: «Valor» puede ser la fecha valor o el importe, una columna «Tipo» puede
//! llevar el signo («D»/«H»), y de dos columnas de fecha se usa la que sigue el saldo.

use super::super::iban;
use super::super::numeros::{Orden, fecha, importe};
use super::diccionario::{Campo, campo, divisa_de_titulo};
use super::texto::{clave, palabras};

/// Qué es cada columna de una tabla de movimientos (por su posición en la fila).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Columnas {
    pub fecha: usize,
    /// Otra columna de fecha de la operación, para las filas que no traen la principal (los
    /// pendientes de Revolut no tienen fecha de cargo).
    pub fecha_otra: Option<usize>,
    pub fecha_valor: Option<usize>,
    /// Las columnas del concepto, en su orden (concepto, beneficiario, observaciones…).
    pub conceptos: Vec<usize>,
    /// Importe con signo (o sin él, si el signo va en otra columna o hay que sacarlo del
    /// saldo).
    pub importe: Option<usize>,
    pub cargo: Option<usize>,
    pub abono: Option<usize>,
    pub saldo: Option<usize>,
    pub divisa: Option<usize>,
    pub referencias: Vec<usize>,
    pub signo: Option<usize>,
    pub comision: Option<usize>,
    /// La divisa de la comisión («Fee currency»), si va en otra columna.
    pub divisa_comision: Option<usize>,
    /// Columnas de estado («Status», «Balance Impact»): las filas pendientes, rechazadas o de
    /// sólo aviso no son movimientos.
    pub estados: Vec<usize>,
    /// La cuenta propia, si va en cada fila.
    pub cuenta: Option<usize>,
    /// ¿El importe es el neto, ya sin las comisiones?
    pub neto: bool,
    /// La divisa que dice el título del importe o del saldo («Amount (EUR)»).
    pub divisa_titulo: Option<String>,
    /// Los títulos como claves, para reconocer el CSV de algunos bancos.
    pub titulos: Vec<String>,
    /// Filas que ocupa la cabecera: 1, o 2 si los títulos van partidos en dos filas.
    pub alto: usize,
    /// Las columnas del cambio de divisa, para sacar los euros de una cuenta en otra divisa.
    pub cambio: ColumnasCambio,
}

/// Columnas con las que una cuenta en otra divisa da los euros de cada movimiento.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct ColumnasCambio {
    pub divisa_origen: Option<usize>,
    pub importe_origen: Option<usize>,
    pub cambio_de: Option<usize>,
    pub cambio_a: Option<usize>,
    pub importe_cambio_a: Option<usize>,
    pub tipo_cambio: Option<usize>,
    pub contravalor: Option<usize>,
}

impl ColumnasCambio {
    pub fn alguna(&self) -> bool {
        *self != Self::default()
    }
}

fn es_de_fecha(c: Campo) -> bool {
    matches!(
        c,
        Campo::Fecha | Campo::FechaOperacion | Campo::FechaContable
    )
}

fn es_de_dinero(c: Campo) -> bool {
    matches!(
        c,
        Campo::Importe | Campo::ImporteNeto | Campo::ImporteBruto | Campo::Cargo | Campo::Abono
    )
}

/// ¿Parece un dato (una fecha o un importe) y no un título?
fn parece_dato(c: &str) -> bool {
    let c = c.trim();
    !c.is_empty() && (importe(c).is_some() || fecha(c, None, Orden::DiaMes).is_some())
}

/// Grupo de un campo, para contar cuántas cosas distintas reconoce una fila.
fn grupo(c: Campo) -> u8 {
    match c {
        Campo::Fecha | Campo::FechaOperacion | Campo::FechaContable => 0,
        Campo::FechaValor => 1,
        Campo::Valor => 2,
        Campo::Concepto => 3,
        Campo::Importe | Campo::ImporteNeto | Campo::ImporteBruto | Campo::Cargo | Campo::Abono => {
            4
        }
        Campo::Saldo | Campo::SaldoDisponible => 5,
        Campo::Divisa => 6,
        Campo::Referencia => 7,
        Campo::Signo => 8,
        Campo::Comision | Campo::DivisaComision => 9,
        Campo::Estado => 10,
        Campo::Tipo => 11,
        Campo::Cuenta
        | Campo::Ignorar
        | Campo::Libro
        | Campo::DivisaOrigen
        | Campo::ImporteOrigen
        | Campo::CambioDe
        | Campo::CambioA
        | Campo::ImporteCambioA
        | Campo::TipoCambio
        | Campo::Contravalor => 12,
    }
}

/// Puntos de una fila como fila de títulos; `None` si no lo es.
fn puntos(fila: &[String]) -> Option<usize> {
    if fila.iter().filter(|c| parece_dato(c)).count() > 1 {
        return None;
    }
    let campos: Vec<Campo> = fila.iter().filter_map(|c| campo(c)).collect();
    if campos.contains(&Campo::Libro) {
        return None;
    }
    // Hay tablas cuya única fecha es la fecha valor («F. VALOR»).
    let fecha = campos
        .iter()
        .any(|c| es_de_fecha(*c) || *c == Campo::FechaValor);
    let dinero = campos
        .iter()
        .any(|c| es_de_dinero(*c) || *c == Campo::Valor);
    if !fecha || !dinero {
        return None;
    }
    let mut grupos: Vec<u8> = campos
        .iter()
        .map(|c| grupo(*c))
        .filter(|g| *g != 12)
        .collect();
    grupos.sort_unstable();
    grupos.dedup();
    (grupos.len() >= 3).then_some(grupos.len() * 2 + campos.len())
}

/// Las filas de títulos de una hoja: la posición de cada una y cuántas filas ocupa (los
/// títulos partidos en dos filas, «Fecha» encima de «operación», se juntan).
pub(super) fn cabeceras(filas: &[Vec<String>]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < filas.len() {
        if puntos(&filas[i]).is_some() {
            out.push((i, 1));
            i += 1;
            continue;
        }
        if let Some(sig) = filas.get(i + 1)
            && puntos(sig).is_none()
            && !filas[i].iter().chain(sig).any(|c| parece_dato(c))
            && filas[i].iter().filter(|c| !c.trim().is_empty()).count() >= 2
        {
            let n = filas[i].len().max(sig.len());
            let juntas: Vec<String> = (0..n)
                .map(|c| {
                    let a = filas[i].get(c).map_or("", |s| s.trim());
                    let b = sig.get(c).map_or("", |s| s.trim());
                    format!("{a} {b}").trim().to_owned()
                })
                .collect();
            if puntos(&juntas).is_some() {
                out.push((i, 2));
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// ¿Es el valor de una columna de signo («D», «H», «+», «Debit», «Abono»…)?
pub(super) fn es_signo(v: &str) -> bool {
    let k = clave(v);
    let t = v.trim();
    matches!(t, "+" | "-")
        || matches!(
            k.as_str(),
            "d" | "h"
                | "c"
                | "a"
                | "s"
                | "dr"
                | "cr"
                | "db"
                | "deb"
                | "cred"
                | "debe"
                | "haber"
                | "cargo"
                | "abono"
                | "debito"
                | "credito"
                | "debit"
                | "credit"
                | "af"
                | "bij"
                | "soll"
                | "haben"
                | "in"
                | "out"
                | "incoming"
                | "outgoing"
        )
}

/// ¿Es un código de divisa («EUR», «usd», «€»)?
pub(super) fn es_divisa(v: &str) -> bool {
    let t = v.trim();
    matches!(t, "€" | "$" | "£")
        || (t.len() == 3 && t.bytes().all(|b| b.is_ascii_alphabetic()))
        || matches!(clave(t).as_str(), "euros" | "euro")
}

/// Cuánto de cada cosa trae una columna (en las filas de datos).
#[derive(Debug, Default)]
struct Contenido {
    llenas: usize,
    fechas: usize,
    importes: usize,
    divisas: usize,
    signos: usize,
    ibanes: usize,
}

impl Contenido {
    fn de(filas: &[Vec<String>], col: usize) -> Self {
        let mut c = Self::default();
        for f in filas.iter().take(400) {
            let v = f.get(col).map_or("", |s| s.trim());
            if v.is_empty() {
                continue;
            }
            c.llenas += 1;
            if fecha(v, None, Orden::DiaMes).is_some()
                || fecha(v, None, Orden::MesDia).is_some()
                || (v.len() <= 6 && fecha(v, Some(2000), Orden::DiaMes).is_some())
            {
                c.fechas += 1;
            }
            if importe(v).is_some() {
                c.importes += 1;
            }
            if es_divisa(v) {
                c.divisas += 1;
            }
            if es_signo(v) {
                c.signos += 1;
            }
            if !iban::en_texto(v).is_empty() {
                c.ibanes += 1;
            }
        }
        c
    }

    /// ¿Casi todas (el 80 %) las celdas con algo son de esa cosa?
    fn casi_todas(&self, n: usize) -> bool {
        self.llenas > 0 && n * 5 >= self.llenas * 4
    }
}

/// Qué es cada columna de la tabla con la cabecera en `cab` (que ocupa `alto` filas) y los
/// datos hasta `fin`. `None` si no hay columna de fecha o de dinero.
pub(super) fn columnas(
    filas: &[Vec<String>],
    cab: usize,
    alto: usize,
    fin: usize,
) -> Option<Columnas> {
    filas.get(cab)?;
    let ancho = (cab..cab + alto)
        .filter_map(|i| filas.get(i))
        .map(Vec::len)
        .max()
        .unwrap_or(0);
    let titulos: Vec<String> = (0..ancho)
        .map(|c| {
            (cab..cab + alto)
                .filter_map(|i| filas.get(i).and_then(|f| f.get(c)))
                .map(|s| s.trim())
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_owned()
        })
        .collect();
    let datos = filas.get(cab + alto..fin.min(filas.len())).unwrap_or(&[]);
    let mut campos: Vec<Option<Campo>> = titulos.iter().map(|t| campo(t)).collect();
    if campos.contains(&Some(Campo::Libro)) {
        return None;
    }
    let contenido: Vec<Contenido> = (0..ancho).map(|c| Contenido::de(datos, c)).collect();
    let hay_saldo = campos
        .iter()
        .any(|c| matches!(c, Some(Campo::Saldo | Campo::SaldoDisponible)));
    let hay_importe = campos.iter().any(|c| c.is_some_and(es_de_dinero));
    // Un título de concepto que ocupa varias columnas (celdas combinadas) deja vacías las de
    // su derecha: son también concepto.
    for c in 1..ancho {
        if campos[c].is_none() && titulos[c].is_empty() && campos[c - 1] == Some(Campo::Concepto) {
            campos[c] = Some(Campo::Concepto);
        }
    }
    // Lo que dice el contenido.
    for (c, k) in campos.iter_mut().enumerate() {
        let n = &contenido[c];
        match *k {
            Some(Campo::Valor) => {
                *k = Some(if n.casi_todas(n.fechas) {
                    Campo::FechaValor
                } else if n.casi_todas(n.divisas) {
                    Campo::Divisa
                } else if n.casi_todas(n.importes) && !hay_importe {
                    Campo::Importe
                } else {
                    Campo::Ignorar
                });
            }
            Some(Campo::Tipo | Campo::Signo) | None
                if n.llenas > 0 && n.signos * 10 >= n.llenas * 9 =>
            {
                *k = Some(Campo::Signo);
            }
            Some(Campo::Signo) if n.casi_todas(n.importes) && !hay_importe => {
                *k = Some(Campo::Importe);
            }
            Some(Campo::Signo) => *k = Some(Campo::Ignorar),
            _ => {}
        }
    }
    // La fecha: con saldo, la contable (la que sigue el saldo); sin él, la de la operación.
    let preferencia: [Campo; 3] = if hay_saldo {
        [Campo::FechaContable, Campo::Fecha, Campo::FechaOperacion]
    } else {
        [Campo::FechaOperacion, Campo::Fecha, Campo::FechaContable]
    };
    let con_fechas = |c: usize| contenido[c].fechas > 0 || contenido[c].llenas == 0;
    let valor = (0..ancho).find(|&c| campos[c] == Some(Campo::FechaValor) && con_fechas(c));
    let fecha_col = preferencia
        .iter()
        .find_map(|p| (0..ancho).find(|&c| campos[c] == Some(*p) && con_fechas(c)))
        .or(valor)?;
    let fecha_valor = valor.filter(|c| *c != fecha_col);
    let fecha_otra = preferencia.iter().find_map(|p| {
        (0..ancho).find(|&c| c != fecha_col && campos[c] == Some(*p) && con_fechas(c))
    });
    let con_importes = |c: usize| contenido[c].importes > 0 || contenido[c].llenas == 0;
    let de = |k: Campo| (0..ancho).find(|&c| campos[c] == Some(k) && con_importes(c));
    let (cargo, abono) = (de(Campo::Cargo), de(Campo::Abono));
    let neto = de(Campo::ImporteNeto);
    let importe_col = neto
        .or_else(|| de(Campo::Importe))
        .or_else(|| de(Campo::ImporteBruto));
    let (importe_col, cargo, abono) = match (importe_col, cargo, abono) {
        (Some(i), c, a) if c.is_none() || a.is_none() => (Some(i), None, None),
        (_, c, a) => (None, c, a),
    };
    if importe_col.is_none() && cargo.is_none() && abono.is_none() {
        return None;
    }
    // Un saldo que no cambia nunca aunque cambien los importes no es el de después de cada
    // movimiento: es el de la cuenta, repetido en cada fila.
    let fijo = |c: usize| {
        let mut v: Vec<&str> = datos
            .iter()
            .filter_map(|f| f.get(c))
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();
        let n = v.len();
        v.sort_unstable();
        v.dedup();
        n >= 3 && v.len() == 1
    };
    let saldo = de(Campo::Saldo)
        .or_else(|| de(Campo::SaldoDisponible))
        .filter(|c| !fijo(*c));
    let mut conceptos: Vec<usize> = (0..ancho)
        .filter(|&c| campos[c] == Some(Campo::Concepto))
        .collect();
    if conceptos.is_empty() {
        conceptos = (0..ancho)
            .filter(|&c| campos[c] == Some(Campo::Tipo))
            .collect();
    }
    let divisa = (0..ancho).find(|&c| {
        campos[c] == Some(Campo::Divisa)
            && (contenido[c].llenas == 0 || contenido[c].divisas * 2 >= contenido[c].llenas)
    });
    let divisa_titulo = [importe_col, cargo, abono, saldo]
        .iter()
        .flatten()
        .find_map(|c| divisa_de_titulo(&titulos[*c]));
    Some(Columnas {
        fecha: fecha_col,
        fecha_otra,
        fecha_valor,
        conceptos,
        importe: importe_col,
        cargo,
        abono,
        saldo,
        divisa,
        referencias: (0..ancho)
            .filter(|&c| campos[c] == Some(Campo::Referencia))
            .collect(),
        signo: (0..ancho).find(|&c| campos[c] == Some(Campo::Signo)),
        comision: (0..ancho).find(|&c| campos[c] == Some(Campo::Comision)),
        divisa_comision: (0..ancho).find(|&c| campos[c] == Some(Campo::DivisaComision)),
        estados: (0..ancho)
            .filter(|&c| campos[c] == Some(Campo::Estado))
            .collect(),
        // Si hay varias columnas de la cuenta («Account» con el nombre y «International account
        // number» con el IBAN, en Revolut), la que trae el IBAN.
        cuenta: (0..ancho)
            .filter(|&c| campos[c] == Some(Campo::Cuenta))
            .max_by_key(|&c| (contenido[c].ibanes > 0, std::cmp::Reverse(c))),
        neto: neto.is_some() && importe_col == neto,
        divisa_titulo,
        titulos: titulos.iter().map(|t| palabras(t).join(" ")).collect(),
        alto,
        cambio: {
            let k = |x: Campo| (0..ancho).find(|&c| campos[c] == Some(x));
            ColumnasCambio {
                divisa_origen: k(Campo::DivisaOrigen),
                importe_origen: k(Campo::ImporteOrigen),
                cambio_de: k(Campo::CambioDe),
                cambio_a: k(Campo::CambioA),
                importe_cambio_a: k(Campo::ImporteCambioA),
                tipo_cambio: k(Campo::TipoCambio),
                contravalor: k(Campo::Contravalor),
            }
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fila(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn encuentra_la_cabecera_debajo_de_los_datos_de_la_cuenta() {
        let filas = vec![
            fila(&["Titular", "EMPRESA DEMO SL"]),
            fila(&["Fecha", "01/02/2025"]),
            fila(&[]),
            fila(&[
                "F. CONTABLE",
                "F. VALOR",
                "CÓDIGO",
                "CONCEPTO",
                "IMPORTE",
                "SALDO",
            ]),
            fila(&[
                "03/02/2025",
                "03/02/2025",
                "00007",
                "TRANSFERENCIA",
                "-10",
                "90",
            ]),
            fila(&["01/02/2025", "01/02/2025", "00007", "ABONO", "100", "100"]),
        ];
        assert_eq!(cabeceras(&filas), [(3, 1)]);
        let c = columnas(&filas, 3, 1, filas.len()).unwrap();
        assert_eq!(
            (c.fecha, c.fecha_valor, c.importe, c.saldo),
            (0, Some(1), Some(4), Some(5))
        );
        assert_eq!(c.conceptos, [3]);
    }

    #[test]
    fn cargo_y_abono_signo_y_valor_por_el_contenido() {
        let filas = vec![
            fila(&["Fecha", "Valor", "Concepto", "Importe", "D/H", "Saldo"]),
            fila(&["03/02/2025", "04/02/2025", "RECIBO", "10,00", "D", "90,00"]),
            fila(&["05/02/2025", "05/02/2025", "ABONO", "5,00", "H", "95,00"]),
        ];
        let c = columnas(&filas, 0, 1, filas.len()).unwrap();
        assert_eq!(
            (c.fecha, c.fecha_valor, c.importe, c.signo),
            (0, Some(1), Some(3), Some(4))
        );
        let filas = vec![
            fila(&["Date", "Description", "Paid out", "Paid in", "Balance"]),
            fila(&["03/02/2025", "SHOP", "10.00", "", "90.00"]),
        ];
        let c = columnas(&filas, 0, 1, filas.len()).unwrap();
        assert_eq!(
            (c.importe, c.cargo, c.abono, c.saldo),
            (None, Some(2), Some(3), Some(4))
        );
        // Un «Tipo» que no es signo no es nada.
        let filas = vec![
            fila(&[
                "FECHA DE OPERACIÓN",
                "CONCEPTO",
                "TIPO DE MOVIMIENTO",
                "IMPORTE",
                "DIVISA",
            ]),
            fila(&["2025-10-05", "TIENDA", "Compra", "-18,42", "EUR"]),
        ];
        let c = columnas(&filas, 0, 1, filas.len()).unwrap();
        assert_eq!(
            (c.signo, c.divisa, c.conceptos.clone()),
            (None, Some(4), vec![1])
        );
    }

    #[test]
    fn fechas_preferidas_y_titulos_en_dos_filas() {
        let filas = vec![
            fila(&[
                "Type",
                "Started Date",
                "Completed Date",
                "Description",
                "Amount",
                "Fee",
                "Currency",
                "State",
                "Balance",
            ]),
            fila(&[
                "CARD_PAYMENT",
                "2025-01-02 10:00:00",
                "2025-01-03 09:00:00",
                "Shop",
                "-1.00",
                "0.00",
                "EUR",
                "COMPLETED",
                "9.00",
            ]),
        ];
        let c = columnas(&filas, 0, 1, filas.len()).unwrap();
        assert_eq!(c.fecha, 2);
        assert_eq!(
            (c.comision, c.estados.first().copied(), c.divisa),
            (Some(5), Some(7), Some(6))
        );
        let filas = vec![
            fila(&["Fecha", "Fecha", "", "Importe"]),
            fila(&["operación", "valor", "Concepto", "(EUR)"]),
            fila(&["01/01/2025", "01/01/2025", "X", "1,00"]),
        ];
        assert_eq!(cabeceras(&filas), [(0, 2)]);
        let c = columnas(&filas, 0, 2, filas.len()).unwrap();
        assert_eq!((c.fecha, c.fecha_valor, c.importe), (0, Some(1), Some(3)));
        assert_eq!(c.divisa_titulo.as_deref(), Some("EUR"));
        // Los títulos de un diario o de un mayor no son los de un extracto.
        let diario = fila(&[
            "Fecha",
            "Asiento",
            "Concepto",
            "Cuenta",
            "Importe debe",
            "Importe haber",
        ]);
        assert!(cabeceras(&[diario]).is_empty());
        let mayor = fila(&[
            "Cuenta",
            "Descripción",
            "Punt.",
            "Fecha",
            "Concepto",
            "Debe",
            "Haber",
            "Saldo",
        ]);
        assert!(cabeceras(&[mayor]).is_empty());
        // Una fila de datos no es una cabecera.
        assert!(cabeceras(&[fila(&["01/01/2025", "Fecha", "1,00", "Importe"])]).is_empty());
    }
}
