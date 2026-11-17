//! Bankinter: la pantalla de «Movimientos» de su banca en línea impresa en PDF.
//!
//! Arriba van la empresa titular, el usuario que la imprimió (una persona, no el titular), la
//! fecha y hora, y la cuenta con su saldo en ese momento, que sale antes de su etiqueta
//! («ES.. 3.250,40 EUR Saldo»). Luego el periodo («01.02.2020 - 01.05.2020») y una línea por
//! movimiento, del más nuevo al más antiguo:
//! `dd.mm.aaaa dd.mm.aaaa <categoría> <descripción> <importe> EUR <saldo> EUR`. La categoría y
//! la descripción salen juntas y no se pueden separar. La segunda página es sólo una nota.

use std::sync::LazyLock;

use regex::Regex;

use super::comun::{al_derecho, cantidad, fecha, importe, limpiar};
use crate::extractos::{Extracto, Movimiento, iban};

static CUENTA: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"^([A-Z]{2}\d{2}[0-9A-Z ]{10,40}?) ([-+]?\d{1,3}(?:\.\d{3})*,\d{2}) EUR Saldo$")
        .ok()
});
static PERIODO: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^(\d{2}\.\d{2}\.\d{4}) - (\d{2}\.\d{2}\.\d{4})$").ok());
static MOVIMIENTO: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"^(\d{2}\.\d{2}\.\d{4}) (\d{2}\.\d{2}\.\d{4}) (.+) ([-+]?\d{1,3}(?:\.\d{3})*,\d{2}) EUR ([-+]?\d{1,3}(?:\.\d{3})*,\d{2}) EUR$",
    )
    .ok()
});

/// Extracto de Bankinter, si el texto es de uno.
pub(super) fn leer(texto: &str) -> Option<Vec<Extracto>> {
    if !texto.contains("FECHA CONTABLE FECHA VALOR CATEGOR") || !texto.contains("Movimientos*") {
        return None;
    }
    let mut e = Extracto::nuevo("PDF");
    e.banco = "Bankinter".to_owned();
    let mut saldo_actual = None;
    let mut generado = None;
    let mut anterior = "";
    for l in texto.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if l.starts_with("INFORMACIÓN DE INTERÉS") {
            break;
        }
        match anterior {
            "Empresa" => e.titular = limpiar(l),
            "Fecha y hora" => generado = fecha(l),
            _ => {}
        }
        anterior = l;
        if let Some(c) = MOVIMIENTO.as_ref()?.captures(l) {
            let (Some(f), Some(imp), Some(saldo)) = (fecha(&c[1]), importe(&c[4]), importe(&c[5]))
            else {
                e.avisos
                    .push(format!("No se ha podido leer el movimiento «{l}»."));
                continue;
            };
            e.movimientos.push(Movimiento {
                importe_eur: None,
                fecha: f,
                fecha_valor: fecha(&c[2]),
                concepto: limpiar(&c[3]),
                importe: imp,
                saldo: Some(saldo),
                referencia: String::new(),
            });
        } else if let Some(c) = PERIODO.as_ref()?.captures(l) {
            e.desde = fecha(&c[1]);
            e.hasta = fecha(&c[2]);
        } else if let Some(c) = CUENTA.as_ref()?.captures(l) {
            let num = iban::compactar(&c[1]);
            if iban::valido(&num) {
                e.con_iban(&num);
            } else if let Some(m) = iban::enmascarada(&c[1]) {
                e.cuenta = m.clave();
            }
            saldo_actual = importe(&c[2]);
        }
    }
    al_derecho(&mut e.movimientos, true);
    if let (Some(h), Some(g)) = (e.hasta, generado)
        && h > g
    {
        e.hasta = Some(g);
    }
    if let (Some(actual), Some(ultimo)) = (saldo_actual, e.movimientos.last().and_then(|m| m.saldo))
        && actual != ultimo
    {
        e.avisos.push(format!(
            "El saldo de la cabecera ({}) es el del momento en que se imprimió y no el del \
             último movimiento ({}): habrá movimientos posteriores.",
            cantidad(actual, "EUR"),
            cantidad(ultimo, "EUR")
        ));
    }
    Some(vec![e])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extractos::Clase;
    use taxpain_core::money::Money;
    use time::macros::date;

    const MOVIMIENTOS: &str = "\n\nEmpresa\n\nEMPRESA DEMO SL\n Usuario\n\nUSUARIO DEMO\n Fecha y \
hora\n\n2/4/24  09:15\n\nCuenta Relación\n\nES91 2100 0418 4502 0005 1332 1.649,72 EUR Saldo  \
\n\nMovimientos*\n\n01.01.2024 - 31.03.2024\n\nFECHA CONTABLE FECHA VALOR CATEGORÍA DESCRIPCIÓN \
IMPORTE SALDO\n\n28.03.2024 27.03.2024 Gastos Comis. mant. -3,61 EUR 1.649,72 EUR\n\n18.03.2024 \
18.03.2024 Otros ingresos Abono demo 30,00 EUR 1.653,33 EUR\n\n07.03.2024 06.03.2024 Gastos Comis. \
mant. -5,00 EUR 1.623,33 EUR\n\n20.02.2024 20.02.2024 Impuestos Pago impuestos demo -1.034,12 EUR \
1.628,33 EUR\n\n07.02.2024 06.02.2024 Gastos Comis. mant. -5,00 EUR 2.662,45 EUR\n\n22.01.2024 \
22.01.2024 Gastos Recibo proveedor ficticio -89,95 EUR 2.667,45 EUR\n\n15.01.2024 15.01.2024 \
Ingresos Transferencia recibida demo 1.250,00 EUR 2.757,40 EUR\n\n08.01.2024 06.01.2024 Gastos \
Comis. mant. -5,00 EUR 1.507,40 EUR\n\nINFORMACIÓN DE INTERÉS\n\n* Los movimientos mostrados se \
corresponden con los que usted está viendo actualmente en pantalla";

    #[test]
    fn movimientos_de_la_cuenta() {
        let mut v = leer(MOVIMIENTOS).unwrap();
        assert_eq!(v.len(), 1);
        let mut e = v.remove(0);
        assert_eq!(e.formato, "PDF");
        assert_eq!(e.banco, "Bankinter");
        assert_eq!(e.iban.as_deref(), Some("ES9121000418450200051332"));
        assert_eq!(e.cuenta, "210004180200051332");
        // El titular es la empresa, no el usuario que imprimió la pantalla.
        assert_eq!(e.titular, "EMPRESA DEMO SL");
        assert_eq!(e.divisa, "EUR");
        assert_eq!(e.clase, Clase::Cuenta);
        assert_eq!(e.desde, Some(date!(2024 - 01 - 01)));
        assert_eq!(e.hasta, Some(date!(2024 - 03 - 31)));
        assert!(e.avisos.is_empty(), "{:?}", e.avisos);
        let filas: Vec<_> = e
            .movimientos
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
            .collect();
        assert_eq!(
            filas,
            [
                (
                    date!(2024 - 01 - 08),
                    Some(date!(2024 - 01 - 06)),
                    -500,
                    Some(150_740),
                    "Gastos Comis. mant."
                ),
                (
                    date!(2024 - 01 - 15),
                    Some(date!(2024 - 01 - 15)),
                    125_000,
                    Some(275_740),
                    "Ingresos Transferencia recibida demo"
                ),
                (
                    date!(2024 - 01 - 22),
                    Some(date!(2024 - 01 - 22)),
                    -8_995,
                    Some(266_745),
                    "Gastos Recibo proveedor ficticio"
                ),
                (
                    date!(2024 - 02 - 07),
                    Some(date!(2024 - 02 - 06)),
                    -500,
                    Some(266_245),
                    "Gastos Comis. mant."
                ),
                (
                    date!(2024 - 02 - 20),
                    Some(date!(2024 - 02 - 20)),
                    -103_412,
                    Some(162_833),
                    "Impuestos Pago impuestos demo"
                ),
                (
                    date!(2024 - 03 - 07),
                    Some(date!(2024 - 03 - 06)),
                    -500,
                    Some(162_333),
                    "Gastos Comis. mant."
                ),
                (
                    date!(2024 - 03 - 18),
                    Some(date!(2024 - 03 - 18)),
                    3_000,
                    Some(165_333),
                    "Otros ingresos Abono demo"
                ),
                (
                    date!(2024 - 03 - 28),
                    Some(date!(2024 - 03 - 27)),
                    -361,
                    Some(164_972),
                    "Gastos Comis. mant."
                ),
            ]
        );
        assert!(e.cerrar().cuadra());
        assert_eq!(e.saldo_inicial, Some(Money::from_cents(151_240)));
        assert_eq!(e.saldo_final, Some(Money::from_cents(164_972)));
    }

    #[test]
    fn saldo_de_la_cabecera_posterior() {
        let e = &leer(&MOVIMIENTOS.replace("1.649,72 EUR Saldo", "1.700,00 EUR Saldo")).unwrap()[0];
        assert_eq!(e.avisos.len(), 1);
    }

    #[test]
    fn sin_la_huella_de_bankinter_no_es_suyo() {
        assert!(leer(&MOVIMIENTOS.replace("Movimientos*", "Movimientos")).is_none());
    }
}
