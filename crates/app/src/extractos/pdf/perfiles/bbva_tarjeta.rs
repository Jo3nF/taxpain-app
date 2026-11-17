//! BBVA: los movimientos de una tarjeta, en sus dos PDF.
//!
//! - «Listado de movimientos de tarjeta»: una línea por movimiento,
//!   `aaaa-mm-dd <concepto> <tipo> <importe> EUR`, del más nuevo al más antiguo; el tipo
//!   («Compra», «Sin categoría»…) va pegado al concepto. Los datos de la tarjeta (número, fecha
//!   del listado, periodo, contrato, titular y filtros) salen detrás de los movimientos de la
//!   primera página, dos por línea.
//! - El informe «cardMovementsReport»: el lector de PDF desordena las etiquetas y sus valores,
//!   pone el titular delante de su etiqueta («NOMBRETitular:») y pega el importe al tipo de
//!   tarjeta («-21,30Crédito»), que sale en todas las filas y no dice nada del signo. No trae
//!   periodo ni fecha.
//!
//! El importe ya va con el signo de la tarjeta: las compras en negativo y la liquidación del mes
//! («RECIBO MES ANTERIOR», que es el cargo en la cuenta) en positivo. No hay saldos.

use std::sync::LazyLock;

use regex::Regex;

use super::comun::{al_derecho, cantidad, fecha, importe, limpiar};
use crate::extractos::{Clase, Extracto, Movimiento};

static FILA: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"^(\d{4}-\d{2}-\d{2}) (.+) (-?\d{1,3}(?:\.\d{3})*,\d{1,2}) ([A-Z]{3})$").ok()
});
static NUMERO: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"Nº de tarjeta ([0-9*xX•]{8,19}) Fecha (\d{2}/\d{2}/\d{4})").ok());
static TIPO_PERIODO: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"Tipo de tarjeta (.+?) Periodo (\d{2}/\d{2}/\d{4}) ?- ?(\d{2}/\d{2}/\d{4})").ok()
});
static CONTRATO: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"Nº de contrato ([0-9-]+) Tipo de movimiento (.+)$").ok());
static TITULAR: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"Titular de la tarjeta (.+?) Importes (.+)$").ok());
static FILA_INFORME: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"^(\d{2}/\d{2}/\d{4}) (.+?) (-?\d{1,3}(?:\.\d{3})*,\d{1,2}) ?(?:Crédito|Débito|Credito|Debito)?$",
    )
    .ok()
});
static PAN: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^[0-9*xX•]{8,15}(\d{4})$").ok());
static CONTRATO_INFORME: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^(\d{4})-\d{4}-\d{4}-\d{10,14}$").ok());

/// Tipos de movimiento que el listado pega detrás del concepto (de más largo a más corto).
const TIPOS: [&str; 11] = [
    "Sin categoría",
    "Disposición",
    "Devolución",
    "Comisiones",
    "Intereses",
    "Reintegro",
    "Comisión",
    "Traspaso",
    "Compra",
    "Abono",
    "Cuota",
];

/// Extracto de una tarjeta de BBVA, si el texto es de uno.
pub(super) fn leer(texto: &str) -> Option<Vec<Extracto>> {
    let lineas: Vec<&str> = texto.lines().map(str::trim).collect();
    if texto.contains("Listado de movimientos de tarjeta")
        && lineas
            .iter()
            .any(|l| l.starts_with("FECHA DE OPERACIÓN CONCEPTO"))
    {
        return listado(&lineas).map(|e| vec![e]);
    }
    if texto.contains("Nº CONTRATO")
        && texto.contains("Tipo tarjeta:")
        && texto.contains("Fecha Operación")
    {
        return informe(&lineas).map(|e| vec![e]);
    }
    None
}

/// Extracto vacío de una tarjeta de BBVA.
fn nuevo() -> Extracto {
    let mut e = Extracto::nuevo("PDF");
    e.banco = "BBVA".to_owned();
    e.clase = Clase::TarjetaCredito;
    e
}

/// Clase de la tarjeta por su tipo («TARJETA NEGOCIOS CREDITO»).
fn clase(tipo: &str) -> Clase {
    let t = tipo.to_uppercase();
    if t.contains("DEBITO") || t.contains("DÉBITO") {
        Clase::TarjetaDebito
    } else {
        Clase::TarjetaCredito
    }
}

/// Los últimos cuatro dígitos del número de la tarjeta (nunca los primeros, que son los del
/// emisor).
fn ultimos(numero: &str) -> Option<String> {
    let n = numero.trim();
    let u = n.get(n.len().checked_sub(4)?..)?;
    u.bytes().all(|b| b.is_ascii_digit()).then(|| u.to_owned())
}

fn listado(lineas: &[&str]) -> Option<Extracto> {
    let mut e = nuevo();
    let mut generado = None;
    let mut ult = None;
    let mut entidad = "0182".to_owned();
    let mut filtros: Vec<String> = Vec::new();
    for l in lineas {
        if let Some(c) = FILA.as_ref()?.captures(l) {
            let (Some(f), Some(imp)) = (fecha(&c[1]), importe(&c[3])) else {
                e.avisos
                    .push(format!("No se ha podido leer el movimiento «{l}»."));
                continue;
            };
            let mut concepto = c[2].trim();
            let mut tipo = "";
            if let Some(t) = TIPOS
                .iter()
                .find(|t| concepto.strip_suffix(**t).is_some_and(|r| r.ends_with(' ')))
            {
                tipo = t;
                concepto = concepto[..concepto.len() - t.len()].trim_end();
            }
            if tipo == "Sin categoría" && imp.is_negative() {
                e.avisos.push(format!(
                    "El movimiento del {f} («{concepto}», {}) parece pendiente de liquidar: su \
                     importe puede cambiar en un listado posterior.",
                    cantidad(imp, &c[4])
                ));
            }
            e.divisa = c[4].to_owned();
            e.movimientos.push(Movimiento {
                importe_eur: None,
                fecha: f,
                fecha_valor: None,
                concepto: limpiar(concepto),
                importe: imp,
                saldo: None,
                referencia: String::new(),
            });
            continue;
        }
        if let Some(c) = NUMERO.as_ref()?.captures(l) {
            ult = ultimos(&c[1]);
            generado = fecha(&c[2]);
        }
        if let Some(c) = TIPO_PERIODO.as_ref()?.captures(l) {
            e.clase = clase(&c[1]);
            e.desde = fecha(&c[2]);
            e.hasta = fecha(&c[3]);
        }
        if let Some(c) = CONTRATO.as_ref()?.captures(l) {
            let contrato: String = c[1].chars().filter(char::is_ascii_digit).collect();
            if let Some(ent) = contrato.get(..4) {
                entidad = ent.to_owned();
            }
            filtros.push(c[2].trim().to_owned());
        }
        if let Some(c) = TITULAR.as_ref()?.captures(l) {
            e.titular = limpiar(&c[1]);
            filtros.push(c[2].trim().to_owned());
        }
    }
    e.con_tarjeta(Some(&entidad), &ult?);
    if let (Some(h), Some(g)) = (e.hasta, generado)
        && h > g
    {
        e.hasta = Some(g);
    }
    if e.hasta.is_none() {
        e.hasta = generado;
    }
    if filtros.iter().any(|f| !f.eq_ignore_ascii_case("todos")) {
        e.avisos.push(format!(
            "El listado tiene filtros («{}»): puede que no estén todos los movimientos de la \
             tarjeta.",
            filtros.join("», «")
        ));
    }
    ordenar(&mut e);
    Some(e)
}

fn informe(lineas: &[&str]) -> Option<Extracto> {
    let mut e = nuevo();
    let mut ult = None;
    let mut entidad = "0182".to_owned();
    for l in lineas {
        if let Some(c) = FILA_INFORME.as_ref()?.captures(l) {
            let (Some(f), Some(imp)) = (fecha(&c[1]), importe(&c[3])) else {
                e.avisos
                    .push(format!("No se ha podido leer el movimiento «{l}»."));
                continue;
            };
            e.movimientos.push(Movimiento {
                importe_eur: None,
                fecha: f,
                fecha_valor: None,
                concepto: limpiar(&c[2]),
                importe: imp,
                saldo: None,
                referencia: String::new(),
            });
            continue;
        }
        if let Some(c) = PAN.as_ref()?.captures(l)
            && ult.is_none()
        {
            ult = Some(c[1].to_owned());
        } else if let Some(c) = CONTRATO_INFORME.as_ref()?.captures(l) {
            entidad = c[1].to_owned();
        } else if let Some(t) = l.strip_suffix("Titular:") {
            e.titular = limpiar(t);
        } else if l.starts_with("TARJETA ") {
            e.clase = clase(l);
        }
    }
    e.con_tarjeta(Some(&entidad), &ult?);
    ordenar(&mut e);
    Some(e)
}

/// Del más nuevo al más antiguo: se les da la vuelta y luego se ordenan por fecha sin cambiar
/// el orden de los del mismo día.
fn ordenar(e: &mut Extracto) {
    al_derecho(&mut e.movimientos, true);
    e.movimientos.sort_by_key(|m| m.fecha);
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::Date;
    use time::macros::date;

    /// Listado de dos páginas, con un movimiento pendiente (tipo «Sin categoría» en negativo).
    const LISTADO: &[&str] = &[
        "",
        "",
        "FECHA DE OPERACIÓN CONCEPTO TIPO DE MOVIMIENTO IMPORTE",
        "",
        "2026-06-28 SOFTWARE DEMO *SUBS SAN F Sin categoría -21,37 EUR",
        "",
        "2026-06-05 RECIBO MES ANTERIOR Sin categoría 63,40 EUR",
        "",
        "2026-05-27 SOFTWARE DEMO *SUBS Compra -21,40 EUR",
        "",
        "2026-05-14 PAPELERIA EJEMPLO SL Compra -42,00 EUR",
        "",
        "2026-05-05 RECIBO MES ANTERIOR Sin categoría 99,45 EUR",
        "",
        "2026-04-18 RESTAURANTE FICTICIO Compra -87,25 EUR",
        "",
        "2026-04-11 HOSTING DEMO.COM* X1Y2 Compra -12,20 EUR",
        "",
        "2026-04-05 RECIBO MES ANTERIOR Sin categoría 1.268,50 EUR",
        "",
        "2026-03-30 TIENDA ELECTRONICA DEMO Compra -1.250,00 EUR",
        "",
        "Nº de tarjeta 540000******1234 Fecha 30/06/2026",
        "",
        "Tipo de tarjeta TARJETA NEGOCIOS CREDITO Periodo 01/03/2026 - 30/06/2026",
        "",
        "Nº de contrato 01820000000000000000001234 Tipo de movimiento TODOS",
        "",
        "Titular de la tarjeta EMPRESA DEMO SL Importes TODOS",
        " /1 2",
        "",
        "Listado de movimientos de tarjeta",
        "",
        "FECHA DE OPERACIÓN CONCEPTO TIPO DE MOVIMIENTO IMPORTE",
        "",
        "2026-03-12 LIBRERIA FICTICIA Compra -18,50 EUR",
        " /2 2",
        "",
        "Listado de movimientos de tarjeta",
    ];

    const INFORME: &[&str] = &[
        "",
        "",
        "Movimientos",
        "",
        "Nº de tarjeta:",
        "",
        "Tipo tarjeta:",
        "",
        "Nº CONTRATO",
        " 5400000000001234",
        "",
        "TARJETA NEGOCIOS CREDITO",
        "",
        "0182-0000-0000-00000000001234",
        " Importe (€)Fecha Operación Tipo de movimientoConcepto",
        "",
        "EMPRESA DEMO SLTitular:",
        "",
        "05/05/2026 RECIBO MES ANTERIOR 99,45Crédito",
        "",
        "18/04/2026 RESTAURANTE FICTICIO -87,25Crédito",
        "",
        "11/04/2026 HOSTING DEMO.COM* X1Y2 -12,20Crédito",
    ];

    fn uno(t: &str) -> Extracto {
        let mut v = leer(t).unwrap();
        assert_eq!(v.len(), 1);
        v.remove(0)
    }

    fn filas(e: &Extracto) -> Vec<(Date, i64, &str)> {
        e.movimientos
            .iter()
            .map(|m| (m.fecha, m.importe.cents(), m.concepto.as_str()))
            .collect()
    }

    fn tarjeta_demo(e: &Extracto) {
        assert_eq!(e.formato, "PDF");
        assert_eq!(e.banco, "BBVA");
        assert_eq!(e.clase, Clase::TarjetaCredito);
        assert_eq!(e.cuenta, "tarjeta:0182:1234");
        assert_eq!(e.iban, None);
        assert_eq!(e.titular, "EMPRESA DEMO SL");
        assert_eq!(e.divisa, "EUR");
        assert_eq!((e.saldo_inicial, e.saldo_final), (None, None));
        assert!(
            e.movimientos
                .iter()
                .all(|m| m.saldo.is_none() && m.fecha_valor.is_none() && m.referencia.is_empty())
        );
    }

    #[test]
    fn listado_de_movimientos_de_tarjeta() {
        let mut e = uno(&LISTADO.join("\n"));
        tarjeta_demo(&e);
        assert_eq!(e.desde, Some(date!(2026 - 03 - 01)));
        assert_eq!(e.hasta, Some(date!(2026 - 06 - 30)));
        assert_eq!(
            filas(&e),
            [
                (date!(2026 - 03 - 12), -1_850, "LIBRERIA FICTICIA"),
                (date!(2026 - 03 - 30), -125_000, "TIENDA ELECTRONICA DEMO"),
                (date!(2026 - 04 - 05), 126_850, "RECIBO MES ANTERIOR"),
                (date!(2026 - 04 - 11), -1_220, "HOSTING DEMO.COM* X1Y2"),
                (date!(2026 - 04 - 18), -8_725, "RESTAURANTE FICTICIO"),
                (date!(2026 - 05 - 05), 9_945, "RECIBO MES ANTERIOR"),
                (date!(2026 - 05 - 14), -4_200, "PAPELERIA EJEMPLO SL"),
                (date!(2026 - 05 - 27), -2_140, "SOFTWARE DEMO *SUBS"),
                (date!(2026 - 06 - 05), 6_340, "RECIBO MES ANTERIOR"),
                (date!(2026 - 06 - 28), -2_137, "SOFTWARE DEMO *SUBS SAN F"),
            ]
        );
        // El pendiente de liquidar.
        assert_eq!(e.avisos.len(), 1, "{:?}", e.avisos);
        assert!(e.avisos[0].contains("SOFTWARE DEMO *SUBS SAN F"));
        assert_eq!(e.cerrar(), crate::extractos::Cuadre::SinSaldos);
    }

    #[test]
    fn listado_con_el_numero_entero_y_filtrado() {
        let t = LISTADO
            .join("\n")
            .replace("540000******1234", "5400000000001234")
            .replace("Importes TODOS", "Importes Cargos");
        let e = uno(&t);
        assert_eq!(e.cuenta, "tarjeta:0182:1234");
        assert_eq!(e.avisos.len(), 2, "{:?}", e.avisos);
    }

    #[test]
    fn informe_de_movimientos_de_tarjeta() {
        let e = uno(&INFORME.join("\n"));
        tarjeta_demo(&e);
        // Sin periodo: lo completa el cierre con las fechas de los movimientos.
        assert_eq!((e.desde, e.hasta), (None, None));
        assert!(e.avisos.is_empty(), "{:?}", e.avisos);
        assert_eq!(
            filas(&e),
            [
                (date!(2026 - 04 - 11), -1_220, "HOSTING DEMO.COM* X1Y2"),
                (date!(2026 - 04 - 18), -8_725, "RESTAURANTE FICTICIO"),
                (date!(2026 - 05 - 05), 9_945, "RECIBO MES ANTERIOR"),
            ]
        );
    }

    #[test]
    fn sin_la_huella_de_la_tarjeta_no_es_suyo() {
        let t = LISTADO
            .join("\n")
            .replace("Listado de movimientos de tarjeta", "Otro");
        assert!(leer(&t).is_none());
        let t = INFORME.join("\n").replace("Nº CONTRATO", "CONTRATO");
        assert!(leer(&t).is_none());
    }
}
