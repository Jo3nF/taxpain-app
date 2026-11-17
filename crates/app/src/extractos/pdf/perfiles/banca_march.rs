//! Banca March: los extractos de «Cuentas a la vista» y de «Tarjetas de débito» que da su banca
//! en línea (telemarch).
//!
//! El lector de PDF saca cada fila o título de la página como un bloque, separado del siguiente
//! por una línea en blanco. Hay dos maquetas de la cuenta:
//! - la de 2020-2024, con una línea por movimiento:
//!   `dd/mm/aaaa dd/mm oooo <concepto> <importe> € <saldo> €` (fecha, fecha valor sin año,
//!   oficina, concepto, importe con signo y saldo);
//! - la de 2025, que añade el «Concepto ordenante»: el movimiento se parte en 2 o 3 líneas del
//!   mismo bloque y los importes van al final de la última. Si un movimiento queda partido entre
//!   dos páginas, el trozo que falta del concepto sale solo, justo detrás de la cabecera de la
//!   tabla de la página siguiente.
//!
//! La cabecera de la tabla se repite en cada página y no hay pies, totales ni saldo inicial: el
//! saldo inicial es el saldo de la primera fila menos su importe. El «Saldo actual» de arriba es
//! el de cuando se generó el PDF, no el del final del periodo.

use std::sync::LazyLock;

use regex::Regex;
use time::Date;

use super::comun::{al_derecho, bloques, cantidad, fecha, importe, limpiar, sin_año, sin_punto};
use crate::extractos::{Clase, Extracto, Movimiento, iban};

static GENERADO: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"^\p{L}{2,4}\.? ?, (\d{1,2}) (\p{L}+)\.? +(\d{4}), \d{1,2}:\d{2}").ok()
});
static PERIODO: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^Fecha de (\d{2}/\d{2}/\d{4}) hasta (\d{2}/\d{2}/\d{4})$").ok());
static SALDO_ACTUAL: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^Saldo actual: (-?\d{1,3}(?:\.\d{3})*,\d{2}) ?€").ok());
static MOVIMIENTO: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"^(\d{2}/\d{2}/\d{4}) (\d{2})/(\d{2}) \d{4}(?: (.*?))? (-?\d{1,3}(?:\.\d{3})*,\d{2}) ?€ (-?\d{1,3}(?:\.\d{3})*,\d{2}) ?€$",
    )
    .ok()
});
static TARJETA: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^(?:[*xX•\d]{4} ?){3}(\d{4}) - (.+)$").ok());
static MOV_TARJETA: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"^(\d{2}/\d{2}/\d{4}) (\d{2}/\d{2}/\d{4})(?: (.*?))? (-?\d{1,3}(?:\.\d{3})*,\d{2}) ?€$",
    )
    .ok()
});

/// Lo que dice la cabecera de cualquiera de los dos extractos.
#[derive(Default)]
struct Cabecera {
    generado: Option<Date>,
    desde: Option<Date>,
    hasta: Option<Date>,
    /// «Últimos movimientos» en lugar de un periodo.
    ultimos: bool,
}

impl Cabecera {
    /// Lee el bloque si es de la cabecera. ¿Lo era?
    fn leer(&mut self, b: &str) -> Option<()> {
        if b == "Últimos movimientos" {
            self.ultimos = true;
            return Some(());
        }
        if let Some(c) = GENERADO.as_ref()?.captures(b) {
            self.generado = fecha(&format!("{} {} {}", &c[1], &c[2], &c[3]));
            return Some(());
        }
        let c = PERIODO.as_ref()?.captures(b)?;
        self.desde = fecha(&c[1]);
        self.hasta = fecha(&c[2]);
        Some(())
    }
}

/// Extractos de Banca March, si el texto es de uno.
pub(super) fn leer(texto: &str) -> Option<Vec<Extracto>> {
    if !texto.contains("telemarch.bancamarch.es") || !texto.contains("Extracto de movimientos") {
        return None;
    }
    let bloques: Vec<String> = bloques(texto)
        .iter()
        .map(|b| limpiar(&b.join(" ")))
        .collect();
    let clase = bloques.iter().find_map(|b| {
        if b.starts_with("Cuentas a la vista") {
            Some(Clase::Cuenta)
        } else if b.starts_with("Tarjetas de débito") {
            Some(Clase::TarjetaDebito)
        } else if b.starts_with("Tarjetas de crédito") {
            Some(Clase::TarjetaCredito)
        } else {
            None
        }
    })?;
    let e = match clase {
        Clase::Cuenta => cuenta(&bloques)?,
        _ => tarjeta(&bloques, clase)?,
    };
    Some(vec![e])
}

/// El titular de la línea de la cuenta, sin el punto de detrás ni el «[object Object] (…)» con
/// el que lo envuelve a veces la web del banco.
fn titular(s: &str) -> String {
    let s = s.trim();
    let s = s
        .strip_prefix("[object Object] (")
        .and_then(|r| r.strip_suffix(')'))
        .unwrap_or(s);
    sin_punto(s)
}

fn cuenta(bloques: &[String]) -> Option<Extracto> {
    let mut e = Extracto::nuevo("PDF");
    e.banco = "Banca March".to_owned();
    let mut cab = Cabecera::default();
    let mut saldo_actual = None;
    let mut en_tabla = false;
    for b in bloques {
        if cab.leer(b).is_some() {
            continue;
        }
        if b.starts_with("F. operación") {
            en_tabla = true;
            continue;
        }
        if !en_tabla {
            if let Some((num, resto)) = b.split_once(" - ")
                && e.cuenta.is_empty()
            {
                if let Some(i) = iban::en_texto(num).first() {
                    e.con_iban(i);
                    e.titular = titular(resto);
                } else if let Some(m) = iban::enmascarada(num) {
                    e.cuenta = m.clave();
                    e.titular = titular(resto);
                }
            }
            if let Some(c) = SALDO_ACTUAL.as_ref()?.captures(b) {
                saldo_actual = importe(&c[1]);
            }
            continue;
        }
        if let Some(c) = MOVIMIENTO.as_ref()?.captures(b) {
            let (Some(f), Some(imp), Some(saldo)) = (fecha(&c[1]), importe(&c[5]), importe(&c[6]))
            else {
                e.avisos
                    .push(format!("No se ha podido leer el movimiento «{b}»."));
                continue;
            };
            let valor = match (c[2].parse(), c[3].parse()) {
                (Ok(d), Ok(m)) => sin_año(d, m, f),
                _ => None,
            };
            e.movimientos.push(Movimiento {
                importe_eur: None,
                fecha: f,
                fecha_valor: valor,
                concepto: c.get(4).map(|x| x.as_str()).unwrap_or_default().to_owned(),
                importe: imp,
                saldo: Some(saldo),
                referencia: String::new(),
            });
        } else if let Some(m) = e.movimientos.last_mut() {
            // El trozo de concepto de un movimiento partido entre dos páginas.
            m.concepto = limpiar(&format!("{} {b}", m.concepto));
        }
    }
    al_derecho(&mut e.movimientos, false);
    e.desde = cab.desde;
    e.hasta = cab.hasta.or(cab.generado);
    if let (Some(actual), Some(ultimo)) = (saldo_actual, e.movimientos.last().and_then(|m| m.saldo))
        && actual != ultimo
    {
        let al_dia = cab.ultimos || (cab.hasta.is_some() && cab.hasta == cab.generado);
        e.avisos.push(if al_dia {
            format!(
                "El «Saldo actual» del extracto ({}) no coincide con el saldo del último \
                 movimiento ({}).",
                cantidad(actual, "EUR"),
                cantidad(ultimo, "EUR")
            )
        } else {
            format!(
                "El «Saldo actual» del extracto ({}) es el del día en que se generó y no el del \
                 último movimiento del periodo ({}): habrá movimientos posteriores.",
                cantidad(actual, "EUR"),
                cantidad(ultimo, "EUR")
            )
        });
    }
    Some(e)
}

fn tarjeta(bloques: &[String], clase: Clase) -> Option<Extracto> {
    let mut e = Extracto::nuevo("PDF");
    e.banco = "Banca March".to_owned();
    e.clase = clase;
    let mut cab = Cabecera::default();
    let mut en_tabla = false;
    for b in bloques {
        if cab.leer(b).is_some() {
            continue;
        }
        if b.starts_with("F. operación") {
            en_tabla = true;
            continue;
        }
        if !en_tabla {
            if let Some(c) = TARJETA.as_ref()?.captures(b)
                && e.cuenta.is_empty()
            {
                e.con_tarjeta(Some("0061"), &c[1]);
                e.titular = limpiar(&c[2]);
            }
            continue;
        }
        if let Some(c) = MOV_TARJETA.as_ref()?.captures(b) {
            let (Some(f), Some(imp)) = (fecha(&c[1]), importe(&c[4])) else {
                e.avisos
                    .push(format!("No se ha podido leer el movimiento «{b}»."));
                continue;
            };
            e.movimientos.push(Movimiento {
                importe_eur: None,
                fecha: f,
                // La fecha de recepción: el día en que la compra se carga en la cuenta.
                fecha_valor: fecha(&c[2]),
                concepto: c.get(3).map(|x| x.as_str()).unwrap_or_default().to_owned(),
                importe: imp,
                saldo: None,
                referencia: String::new(),
            });
        } else if let Some(m) = e.movimientos.last_mut() {
            m.concepto = limpiar(&format!("{} {b}", m.concepto));
        }
    }
    if e.cuenta.is_empty() {
        return None;
    }
    // El banco los da por fecha de recepción: se ordenan por la de la compra.
    e.movimientos.sort_by_key(|m| m.fecha);
    e.desde = cab.desde;
    e.hasta = cab.hasta.or(cab.generado);
    if clase == Clase::TarjetaDebito {
        e.avisos.push(
            "Las compras con tarjeta de débito ya salen una a una en el extracto de la cuenta \
             («4B COMPRA…», con la fecha de recepción como fecha de la operación): este listado \
             sirve para completar sus conceptos, no para sumarlas otra vez."
                .to_owned(),
        );
    }
    Some(e)
}

#[cfg(test)]
mod tests {
    use super::*;
    use taxpain_core::money::Money;
    use time::macros::date;

    /// Maqueta de 2025: concepto ordenante, filas partidas en varias líneas y un movimiento
    /// partido entre dos páginas.
    const V2: &str = "\n\nBanca March - https://telemarch.bancamarch.es\n\njue, 27 mar 2025, 10:00 \
CET\n\nCuentas a la vista - Extracto de movimientos\n\nFecha de 01/03/2025 hasta 27/03/2025\n\nES91 \
2100 0418 4502 0005 1332 - EMPRESA DEMO SL.\n\nSaldo actual: 10.191,04 € Retenciones: 0,00 € Saldo \
disponible: 10.191,04 €\n\nF.\noperación F.\nvalor Oficina Concepto Concepto ordenante Importe \
Saldo\n\n01/03/2025 28/02 0200 LIQUID.PROPIA CUENTA -10,00 € 12.340,50 €\n\n03/03/2025 01/03 0100 4B \
COMPRA PAPELERIA EJEMPLO -45,30 € 12.295,20 €\n\n04/03/2025 04/03 0300 TRASPASO EMPLEADO DEMO\nUNO \
-1.500,00 € 10.795,20 €\n\n05/03/2025 05/03 0100 AB.TR.SEPA 00012345678 CLIENTE\nFICTICIO SA PAGO \
FACTURA 2025 0001 A EMPRESA\nDEMO SL 2.420,00 € 13.215,20 €\n\n10/03/2025 10/03 0100 RECIBO \
GESTORIA INVENTADA, S.L 00999\n0000001 -242,00 € 12.973,20 €\n\n12/03/2025 10/03 0100 4B COMPRA \
TIENDA FALSA 19,99 € 12.993,19 €\n\n14/03/2025 14/03 0300 AD.TRANSF.INMEDIATA 00098765432 Traspaso \
-500,00 € 12.493,19 €\n\nF.\noperación F.\nvalor Oficina Concepto Concepto ordenante Importe \
Saldo\n\nEMPRESA DEM\n\n20/03/2025 20/03 0100 ADEUDO TRIBUTOS -1.234,56 € 11.258,63 €\n\n21/03/2025 \
21/03 0100 AB.TR.SEPA 00011112222 CLIENTE\nINVENTADO SL            PAGO DE \
PRUEBA\nNUMERO DOS 150,00 € 11.408,63 €\n\n27/03/2025 27/03 0300 TRANSFER. PROVEEDOR DEMO SL \
-1.217,59 € 10.191,04 €";

    /// Maqueta de 2020-2024: una línea por movimiento, la fecha de generación al estilo antiguo,
    /// la cabecera repetida en la segunda página, una fecha valor del año anterior y dos
    /// movimientos iguales el mismo día.
    const V1: &str = "\n\nBanca March - https://telemarch.bancamarch.es\n\nmié , 12 ene  2022, 09:15 \
CET\n\nCuentas a la vista - Extracto de movimientos\n\nFecha de 01/12/2021 hasta 05/01/2022\n\nES91 \
2100 0418 4502 0005 1332 - EMPRESA DEMO SL.\n\nSaldo actual: 44.479,75 € Retenciones: 0,00 € Saldo \
disponible: 44.479,75 €\n\nF. operación F. valor Oficina Concepto Importe Saldo\n\n01/12/2021 30/11 \
0100 RECIBO GESTORIA INVENTADA, S.L 00999 0000001 -96,80 € 44.903,20 €\n\n03/12/2021 03/12 0300 \
TRANSFER. CLIENTE FICTICIO SA 2.500,00 € 47.403,20 €\n\n16/12/2021 16/12 0100 COMISION \
MANTENIMIENTO -5,00 € 47.398,20 €\n\n01/01/2022 31/12 0200 LIQUID.PROPIA CUENTA -10,00 € 47.388,20 \
€\n\nF. operación F. valor Oficina Concepto Importe Saldo\n\n03/01/2022 01/01 0100 4B COMPRA \
PAPELERIA EJEMPLO -5,20 € 47.383,00 €\n\n04/01/2022 04/01 0300 TRANSFER. PROVEEDOR DEMO SL \
-1.500,00 € 45.883,00 €\n\n04/01/2022 04/01 0300 TRANSFER. PROVEEDOR DEMO SL -1.500,00 € 44.383,00 \
€\n\n04/01/2022 07/01 0400 4B COMPRA RESTAURANTE FICTICIO 123,45 € 44.506,45 €\n\n05/01/2022 05/01 \
0300 TRIBUTOS 00999 0001234 -21,50 € 44.484,95 €";

    const TARJETA_DEBITO: &str = "\n\nBanca March - https://telemarch.bancamarch.es\n\nlun, 15 ene \
2024, 10:00 CET\n\nTarjetas de débito - Extracto de movimientos\n\nFecha de 01/12/2023 hasta \
15/01/2024\n\n**** **** **** 1234 - EMPRESA DEMO SL (DEMO)\n\nF. operación F. recepción Concepto \
Establecimiento/Cajero Importe\n\n01/12/2023 04/12/2023 VENTA ONLINE TIENDA FALSA -45,30 \
€\n\n03/12/2023 03/12/2023 VENTA RESTAURANTE FICTICIO -120,00 €\n\n02/12/2023 05/12/2023 VENTA \
ONLINE SERVICIO DEMO.COM -5,20 €\n\n04/12/2023 07/12/2023 0000000000000 45,30 €\n\n06/12/2023 \
08/12/2023 AJUSTE HABER: ANULACION VENTA COMERCIO 0000000000000 10,00 €";

    fn uno(t: &str) -> Extracto {
        let mut v = leer(t).unwrap();
        assert_eq!(v.len(), 1);
        v.remove(0)
    }

    type Fila<'a> = (Date, Option<Date>, i64, Option<i64>, &'a str);

    fn filas(e: &Extracto) -> Vec<Fila<'_>> {
        e.movimientos
            .iter()
            .map(|m| {
                (
                    m.fecha,
                    m.fecha_valor,
                    m.importe.cents(),
                    m.saldo.map(Money::cents),
                    m.concepto.as_str(),
                )
            })
            .collect()
    }

    fn cuenta_demo(e: &Extracto) {
        assert_eq!(e.formato, "PDF");
        assert_eq!(e.banco, "Banca March");
        assert_eq!(e.iban.as_deref(), Some("ES9121000418450200051332"));
        assert_eq!(e.cuenta, "210004180200051332");
        assert_eq!(e.titular, "EMPRESA DEMO SL");
        assert_eq!(e.divisa, "EUR");
        assert_eq!(e.clase, Clase::Cuenta);
    }

    #[test]
    fn cuenta_con_la_maqueta_de_2025() {
        let mut e = uno(V2);
        cuenta_demo(&e);
        assert_eq!(e.desde, Some(date!(2025 - 03 - 01)));
        assert_eq!(e.hasta, Some(date!(2025 - 03 - 27)));
        assert_eq!((e.saldo_inicial, e.saldo_final), (None, None));
        assert!(e.avisos.is_empty(), "{:?}", e.avisos);
        assert_eq!(
            filas(&e),
            [
                (
                    date!(2025 - 03 - 01),
                    Some(date!(2025 - 02 - 28)),
                    -1_000,
                    Some(1_234_050),
                    "LIQUID.PROPIA CUENTA"
                ),
                (
                    date!(2025 - 03 - 03),
                    Some(date!(2025 - 03 - 01)),
                    -4_530,
                    Some(1_229_520),
                    "4B COMPRA PAPELERIA EJEMPLO"
                ),
                (
                    date!(2025 - 03 - 04),
                    Some(date!(2025 - 03 - 04)),
                    -150_000,
                    Some(1_079_520),
                    "TRASPASO EMPLEADO DEMO UNO"
                ),
                (
                    date!(2025 - 03 - 05),
                    Some(date!(2025 - 03 - 05)),
                    242_000,
                    Some(1_321_520),
                    "AB.TR.SEPA 00012345678 CLIENTE FICTICIO SA PAGO FACTURA 2025 0001 A EMPRESA \
                     DEMO SL"
                ),
                (
                    date!(2025 - 03 - 10),
                    Some(date!(2025 - 03 - 10)),
                    -24_200,
                    Some(1_297_320),
                    "RECIBO GESTORIA INVENTADA, S.L 00999 0000001"
                ),
                (
                    date!(2025 - 03 - 12),
                    Some(date!(2025 - 03 - 10)),
                    1_999,
                    Some(1_299_319),
                    "4B COMPRA TIENDA FALSA"
                ),
                (
                    date!(2025 - 03 - 14),
                    Some(date!(2025 - 03 - 14)),
                    -50_000,
                    Some(1_249_319),
                    "AD.TRANSF.INMEDIATA 00098765432 Traspaso EMPRESA DEM"
                ),
                (
                    date!(2025 - 03 - 20),
                    Some(date!(2025 - 03 - 20)),
                    -123_456,
                    Some(1_125_863),
                    "ADEUDO TRIBUTOS"
                ),
                (
                    date!(2025 - 03 - 21),
                    Some(date!(2025 - 03 - 21)),
                    15_000,
                    Some(1_140_863),
                    "AB.TR.SEPA 00011112222 CLIENTE INVENTADO SL PAGO DE PRUEBA NUMERO DOS"
                ),
                (
                    date!(2025 - 03 - 27),
                    Some(date!(2025 - 03 - 27)),
                    -121_759,
                    Some(1_019_104),
                    "TRANSFER. PROVEEDOR DEMO SL"
                ),
            ]
        );
        assert!(e.movimientos.iter().all(|m| m.referencia.is_empty()));
        assert!(e.cerrar().cuadra());
        assert_eq!(e.saldo_inicial, Some(Money::from_cents(1_235_050)));
        assert_eq!(e.saldo_final, Some(Money::from_cents(1_019_104)));
    }

    #[test]
    fn cuenta_con_la_maqueta_de_2020_a_2024() {
        let mut e = uno(V1);
        cuenta_demo(&e);
        assert_eq!(e.desde, Some(date!(2021 - 12 - 01)));
        assert_eq!(e.hasta, Some(date!(2022 - 01 - 05)));
        assert_eq!(
            filas(&e),
            [
                (
                    date!(2021 - 12 - 01),
                    Some(date!(2021 - 11 - 30)),
                    -9_680,
                    Some(4_490_320),
                    "RECIBO GESTORIA INVENTADA, S.L 00999 0000001"
                ),
                (
                    date!(2021 - 12 - 03),
                    Some(date!(2021 - 12 - 03)),
                    250_000,
                    Some(4_740_320),
                    "TRANSFER. CLIENTE FICTICIO SA"
                ),
                (
                    date!(2021 - 12 - 16),
                    Some(date!(2021 - 12 - 16)),
                    -500,
                    Some(4_739_820),
                    "COMISION MANTENIMIENTO"
                ),
                (
                    date!(2022 - 01 - 01),
                    Some(date!(2021 - 12 - 31)),
                    -1_000,
                    Some(4_738_820),
                    "LIQUID.PROPIA CUENTA"
                ),
                (
                    date!(2022 - 01 - 03),
                    Some(date!(2022 - 01 - 01)),
                    -520,
                    Some(4_738_300),
                    "4B COMPRA PAPELERIA EJEMPLO"
                ),
                (
                    date!(2022 - 01 - 04),
                    Some(date!(2022 - 01 - 04)),
                    -150_000,
                    Some(4_588_300),
                    "TRANSFER. PROVEEDOR DEMO SL"
                ),
                (
                    date!(2022 - 01 - 04),
                    Some(date!(2022 - 01 - 04)),
                    -150_000,
                    Some(4_438_300),
                    "TRANSFER. PROVEEDOR DEMO SL"
                ),
                (
                    date!(2022 - 01 - 04),
                    Some(date!(2022 - 01 - 07)),
                    12_345,
                    Some(4_450_645),
                    "4B COMPRA RESTAURANTE FICTICIO"
                ),
                (
                    date!(2022 - 01 - 05),
                    Some(date!(2022 - 01 - 05)),
                    -2_150,
                    Some(4_448_495),
                    "TRIBUTOS 00999 0001234"
                ),
            ]
        );
        // El «Saldo actual» es del 12/01, después del periodo: sólo un aviso.
        assert_eq!(e.avisos.len(), 1);
        assert!(e.avisos[0].contains("44.479,75 EUR"), "{}", e.avisos[0]);
        assert!(e.cerrar().cuadra());
        assert_eq!(e.saldo_inicial, Some(Money::from_cents(4_500_000)));
        assert_eq!(e.saldo_final, Some(Money::from_cents(4_448_495)));
    }

    #[test]
    fn ultimos_movimientos_y_titular_envuelto() {
        let t = V1
            .replace(
                "Fecha de 01/12/2021 hasta 05/01/2022",
                "Últimos movimientos",
            )
            .replace("EMPRESA DEMO SL.", "[object Object] (EMPRESA DEMO SL.)")
            .replace("44.479,75", "44.484,95");
        let e = uno(&t);
        assert_eq!(e.titular, "EMPRESA DEMO SL");
        assert_eq!(e.desde, None);
        // Sin periodo, hasta la fecha en que se generó.
        assert_eq!(e.hasta, Some(date!(2022 - 01 - 12)));
        assert!(e.avisos.is_empty(), "{:?}", e.avisos);
        assert_eq!(e.movimientos.len(), 9);
    }

    #[test]
    fn tarjeta_de_debito() {
        let e = uno(TARJETA_DEBITO);
        assert_eq!(e.banco, "Banca March");
        assert_eq!(e.clase, Clase::TarjetaDebito);
        assert_eq!(e.cuenta, "tarjeta:0061:1234");
        assert_eq!(e.iban, None);
        assert_eq!(e.titular, "EMPRESA DEMO SL (DEMO)");
        assert_eq!(e.divisa, "EUR");
        assert_eq!(e.desde, Some(date!(2023 - 12 - 01)));
        assert_eq!(e.hasta, Some(date!(2024 - 01 - 15)));
        assert_eq!((e.saldo_inicial, e.saldo_final), (None, None));
        assert_eq!(e.avisos.len(), 1);
        assert_eq!(
            filas(&e),
            [
                (
                    date!(2023 - 12 - 01),
                    Some(date!(2023 - 12 - 04)),
                    -4_530,
                    None,
                    "VENTA ONLINE TIENDA FALSA"
                ),
                (
                    date!(2023 - 12 - 02),
                    Some(date!(2023 - 12 - 05)),
                    -520,
                    None,
                    "VENTA ONLINE SERVICIO DEMO.COM"
                ),
                (
                    date!(2023 - 12 - 03),
                    Some(date!(2023 - 12 - 03)),
                    -12_000,
                    None,
                    "VENTA RESTAURANTE FICTICIO"
                ),
                (
                    date!(2023 - 12 - 04),
                    Some(date!(2023 - 12 - 07)),
                    4_530,
                    None,
                    "0000000000000"
                ),
                (
                    date!(2023 - 12 - 06),
                    Some(date!(2023 - 12 - 08)),
                    1_000,
                    None,
                    "AJUSTE HABER: ANULACION VENTA COMERCIO 0000000000000"
                ),
            ]
        );
        let total: i64 = e.movimientos.iter().map(|m| m.importe.cents()).sum();
        assert_eq!(total, -11_520);
    }

    #[test]
    fn sin_la_huella_de_banca_march_no_es_suyo() {
        assert!(leer(&V1.replace("telemarch.bancamarch.es", "example.com")).is_none());
        assert!(leer(&V1.replace("Cuentas a la vista", "Fondos de inversión")).is_none());
    }
}
