//! BBVA: el «Histórico de movimientos» de la cuenta (en euros o en divisa) que da su banca en
//! línea, en PDF.
//!
//! El texto empieza por la cabecera de la tabla y sigue con los movimientos, del más nuevo al
//! más antiguo. Cada movimiento empieza por `dd/mm/aaaa dd/mm/aaaa ccccc` (fecha contable, fecha
//! valor y código de la operación) y ocupa de una a siete líneas: el concepto (con el
//! beneficiario u ordenante detrás de « | »), una línea en blanco, las observaciones y, al final
//! de la última línea (o en una línea suya), `<importe> <divisa> <saldo> <divisa>`. Los importes
//! llevan uno o dos decimales («8.642,3»). Los datos de la cuenta (titular, IBAN, periodo,
//! divisa) salen al final de la primera página, detrás de sus movimientos. No hay saldo inicial
//! ni final escritos.
//!
//! Los PDF vienen dentro de un sobre MIME, que el distribuidor quita antes de llegar aquí.

use std::sync::LazyLock;

use regex::Regex;
use taxpain_core::money::Money;
use time::Date;

use super::comun::{al_derecho, fecha, importe, unir};
use crate::extractos::{Extracto, Movimiento, iban};

static TITULAR: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"^Titular (.+?) Fecha (\d{2}/\d{2}/\d{4}) Hora \d{1,2}:\d{2}$").ok()
});
static CUENTA: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"^Cuenta ([A-Z]{2}[0-9A-Z ]{12,40}?) Periodo (\d{2}/\d{2}/\d{4}) ?- ?(\d{2}/\d{2}/\d{4})$",
    )
    .ok()
});
static DIVISA: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^Divisa ([A-Z]{3})(?: Importe (.+))?$").ok());
static BANCO: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^Banco .+ Filtros (.+)$").ok());
// El número de página («/1 2»). Unas observaciones «1/3» (plazos) no lo son.
static PAGINA: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"^/\d+ \d+$").ok());
static INICIO: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r"^(\d{2}/\d{2}/\d{4}) (\d{2}/\d{2}/\d{4}) (\d{5})(?: (.*))?$").ok()
});
static COLA: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"(?:^|\s)(-?\d{1,3}(?:\.\d{3})*,\d{1,2}) ([A-Z]{3}) (-?\d{1,3}(?:\.\d{3})*,\d{1,2}) ([A-Z]{3})$",
    )
    .ok()
});

/// Un movimiento que se está leyendo: sus fechas y sus trozos de texto (el primero es el
/// concepto; los demás, separados por líneas en blanco, las observaciones).
struct Abierto {
    fecha: Date,
    valor: Option<Date>,
    trozos: Vec<Vec<String>>,
}

impl Abierto {
    fn añadir(&mut self, t: &str) {
        if !t.is_empty()
            && let Some(ultimo) = self.trozos.last_mut()
        {
            ultimo.push(t.to_owned());
        }
    }

    fn concepto(&self) -> String {
        let concepto = self
            .trozos
            .first()
            .map(|t| unir(t.iter().map(String::as_str)))
            .unwrap_or_default();
        let observaciones = unir(self.trozos.iter().skip(1).flatten().map(String::as_str));
        if observaciones.is_empty() {
            concepto
        } else if concepto.is_empty() {
            observaciones
        } else {
            format!("{concepto} · {observaciones}")
        }
    }
}

/// ¿Es la cabecera de la tabla de movimientos?
fn es_cabecera(t: &str) -> bool {
    t.starts_with("F. CONTABLE F. VALOR C") && t.ends_with("IMPORTE SALDO")
}

/// Extracto de BBVA, si el texto es de uno.
pub(super) fn leer(texto: &str) -> Option<Vec<Extracto>> {
    if !texto.lines().any(|l| es_cabecera(l.trim()))
        || !texto.contains("BANCO BILBAO VIZCAYA ARGENTARIA")
    {
        return None;
    }
    let mut e = Extracto::nuevo("PDF");
    e.banco = "BBVA".to_owned();
    let mut divisa: Option<String> = None;
    let mut divisas_filas: Vec<String> = Vec::new();
    let mut generado = None;
    let mut filtros: Vec<String> = Vec::new();
    let mut abierto: Option<Abierto> = None;
    for linea in texto.lines() {
        let t = linea.trim();
        if es_cabecera(t) || t == "Movimientos" || PAGINA.as_ref()?.is_match(t) {
            continue;
        }
        // Los datos de la cuenta nunca son parte de un movimiento, aunque falte el importe del
        // anterior.
        if let Some(c) = TITULAR.as_ref()?.captures(t) {
            e.titular = c[1].trim().to_owned();
            generado = fecha(&c[2]);
            continue;
        }
        if let Some(c) = CUENTA.as_ref()?.captures(t) {
            let num = iban::compactar(&c[1]);
            if iban::valido(&num) {
                e.con_iban(&num);
            } else if let Some(m) = iban::enmascarada(&c[1]) {
                e.cuenta = m.clave();
            }
            e.desde = fecha(&c[2]);
            e.hasta = fecha(&c[3]);
            continue;
        }
        if let Some(c) = DIVISA.as_ref()?.captures(t) {
            divisa = Some(c[1].to_owned());
            if let Some(f) = c.get(2) {
                filtros.push(f.as_str().trim().to_owned());
            }
            continue;
        }
        if let Some(c) = BANCO.as_ref()?.captures(t) {
            filtros.push(c[1].trim().to_owned());
            continue;
        }
        if let Some(c) = INICIO.as_ref()?.captures(t) {
            if let Some(a) = abierto.take() {
                e.avisos.push(format!(
                    "No se ha encontrado el importe del movimiento del {} («{}»): no se ha \
                     leído.",
                    a.fecha,
                    a.concepto()
                ));
            }
            let Some(f) = fecha(&c[1]) else {
                continue;
            };
            let mut a = Abierto {
                fecha: f,
                valor: fecha(&c[2]),
                trozos: vec![Vec::new()],
            };
            let resto = c.get(4).map(|x| x.as_str()).unwrap_or_default().trim();
            if let Some(m) = cerrar(&mut a, resto, &mut divisas_filas) {
                e.movimientos.push(m);
            } else {
                abierto = Some(a);
            }
            continue;
        }
        let Some(a) = abierto.as_mut() else {
            continue;
        };
        if t.is_empty() {
            if a.trozos.last().is_some_and(|x| !x.is_empty()) {
                a.trozos.push(Vec::new());
            }
            continue;
        }
        if let Some(m) = cerrar(a, t, &mut divisas_filas) {
            e.movimientos.push(m);
            abierto = None;
        }
    }
    if let Some(a) = abierto {
        e.avisos.push(format!(
            "No se ha encontrado el importe del movimiento del {} («{}»): no se ha leído.",
            a.fecha,
            a.concepto()
        ));
    }
    if filtros.iter().any(|f| !f.eq_ignore_ascii_case("todos")) {
        e.avisos.push(format!(
            "El listado tiene filtros («{}»): puede que no estén todos los movimientos de la \
             cuenta.",
            filtros.join("», «")
        ));
    }
    if let Some(d) = divisa.or_else(|| divisas_filas.first().cloned()) {
        e.divisa = d;
    }
    if divisas_filas.iter().any(|d| *d != e.divisa) {
        e.avisos.push(format!(
            "Hay importes en una divisa distinta de la de la cuenta ({}).",
            e.divisa
        ));
    }
    // «Periodo» es lo que se pidió en la consulta: puede acabar después del día en que se
    // generó el PDF.
    if let (Some(h), Some(g)) = (e.hasta, generado)
        && h > g
    {
        e.hasta = Some(g);
    }
    al_derecho(&mut e.movimientos, true);
    Some(vec![e])
}

/// Añade la línea al movimiento y, si acaba en `<importe> <divisa> <saldo> <divisa>`, lo cierra.
fn cerrar(a: &mut Abierto, t: &str, divisas: &mut Vec<String>) -> Option<Movimiento> {
    let Some(c) = COLA.as_ref().and_then(|re| re.captures(t)) else {
        a.añadir(t);
        return None;
    };
    let (todo, imp, saldo) = (c.get(0)?, importe(&c[1]), importe(&c[3]));
    a.añadir(t[..todo.start()].trim());
    for d in [&c[2], &c[4]] {
        if !divisas.iter().any(|x| x == d) {
            divisas.push(d.to_owned());
        }
    }
    let (imp, saldo): (Money, Money) = (imp?, saldo?);
    Some(Movimiento {
        importe_eur: None,
        fecha: a.fecha,
        fecha_valor: a.valor,
        concepto: a.concepto(),
        importe: imp,
        saldo: Some(saldo),
        referencia: String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extractos::Clase;
    use time::macros::date;

    /// Cuenta en euros, dos páginas, con las cinco formas en que salen los movimientos: concepto
    /// partido, observaciones partidas, concepto de tres líneas con los importes en una línea
    /// suya, importes de un decimal y observaciones «1/2» que no son un número de página.
    const EUR: &[&str] = &[
        "",
        "",
        "F. CONTABLE F. VALOR CÓDIGO CONCEPTO|BENEF./ORDENANTE IMPORTE SALDO",
        "",
        "15/04/2026 15/04/2026 00163 ABONO POR TRANSFERENCIA A SU FAVOR ",
        "RECIBIDA EN EUROS | PAGADOR FICTICIO LTD",
        "",
        "LIQ. OP. N 000000000000001 12.500,8 EUR 16.135,92 EUR",
        "",
        "14/04/2026 14/04/2026 00007 TRANSFERENCIAS | SOCIO FICTICIO DEMO",
        "",
        "1/2 -1.000,00 EUR 3.635,12 EUR",
        "",
        "08/04/2026 08/04/2026 00317 PAGO DE NOMINAS POR SU CUENTA | TRABAJADOR ",
        "FICTICIO DEMO",
        "",
        "Nomina Marzo 2026 -1.875,5 EUR 4.635,12 EUR",
        "",
        "02/04/2026 02/04/2026 00007",
        " TRANSFERENCIAS | DEVOLUCIONES ",
        "TRIBUTARIAS, AGENCIA ESTATAL DE ",
        "ADMINISTRACION TRIBUTARIA",
        "",
        "IMPUESTO SOBRE SOCIEDADES",
        " 310,47 EUR 6.510,62 EUR",
        "",
        "31/03/2026 30/03/2026 00781 RETENCIóN PROMOCION COMERCIAL",
        "",
        "BONIF. DEVOLUCION IMPUESTOS -11,7 EUR 6.200,15 EUR",
        "",
        "31/03/2026 30/03/2026 00439 CASHBACK DE TARJETA",
        "",
        "BONIF. DEVOLUCION IMPUESTOS 60,30 EUR 6.211,85 EUR",
        "",
        "20/03/2026 20/03/2026 00007 TRANSFERENCIAS | CLIENTE FICTICIO DOS SL",
        "",
        "PAGO FACTURAS F-2026-002 Y F-2026-003 DE EMPRESA ",
        "DEMO SL 3.630,00 EUR 6.151,55 EUR",
        "",
        "16/03/2026 16/03/2026 00136",
        " ADEUDO A SU CARGO | ASESORIA FICTICIA ",
        "DEMO SL",
        "",
        "N 2026075000000002 ASESORIA FICTICIA DEMO ",
        "SL",
        " -121,00 EUR 2.521,55 EUR",
        "",
        "Titular EMPRESA DEMO SL Fecha 15/04/2026 Hora 10:30",
        "",
        "Cuenta ES9121000418450200051332 Periodo 01/03/2026-15/04/2026",
        "",
        "Divisa EUR Importe Todos",
        "",
        "Banco BANCO BILBAO VIZCAYA ARGENTARIA S.A. Filtros Todos",
        " /1 2",
        "",
        "Movimientos",
        "",
        "F. CONTABLE F. VALOR CÓDIGO CONCEPTO|BENEF./ORDENANTE IMPORTE SALDO",
        "",
        "12/03/2026 12/03/2026 00007 TRANSFERENCIAS | EMPRESA DEMO SOCIEDAD ",
        "LIMITADA.",
        "",
        "TRASPASO 500,00 EUR 2.642,55 EUR",
        "",
        "10/03/2026 10/03/2026 00326 CARGO POR PAGO DE IMPUESTOS - TRIBUTOS",
        "",
        "NRC. 0000000000000DEMO00000 -1.150,00 EUR 2.142,55 EUR",
        "",
        "06/03/2026 05/03/2026 00120 ADEUDO MENSUAL DE TARJETA",
        "",
        "************0000 -42,15 EUR 3.292,55 EUR",
        "",
        "05/03/2026 04/03/2026 00136 ADEUDO A SU CARGO | PROVEEDOR LUZ DEMO SA",
        "",
        "N 2026064000000001 PROVEEDOR LUZ DEMO SA -85,3 EUR 3.334,7 EUR",
        "",
        "02/03/2026 02/03/2026 00007 TRANSFERENCIAS | CLIENTE FICTICIO UNO SL",
        "",
        "FACTURA F-2026-001 2.420,00 EUR 3.420,00 EUR",
        "",
        "/2 2",
        "",
        "Movimientos",
    ];

    /// Cuenta en dólares, una página, con un movimiento en una sola línea sin observaciones.
    const USD: &[&str] = &[
        "",
        "",
        "F. CONTABLE F. VALOR CÓDIGO CONCEPTO|BENEF./ORDENANTE IMPORTE SALDO",
        "",
        "20/03/2026 20/03/2026 00022 TRASPASO | EMPRESA DEMO, S.L.",
        "",
        "0000000002 00000000000002 120,5 USD 370,25 USD",
        "",
        "03/03/2026 03/03/2026 00344 CARGO POR TRANSFERENCIA EMITIDA EN ",
        "DIVISAS | BROKER FICTICIO SA",
        "",
        "0000000001 00000000000001 -4.750,25 USD 249,75 USD",
        "",
        "10/02/2026 10/02/2026 00022 TRASPASO | CLIENTE FICTICIO USA LLC 5.000,00 USD 5.000,00 USD",
        "",
        "Titular EMPRESA DEMO SL Fecha 01/04/2026 Hora 16:00",
        "",
        "Cuenta ES7921000813610123456789 Periodo 01/01/2026-01/04/2026",
        "",
        "Divisa USD Importe Todos",
        "",
        "Banco BANCO BILBAO VIZCAYA ARGENTARIA S.A. Filtros Todos",
        " /1 1",
        "",
        "Movimientos",
    ];

    fn uno(t: &str) -> Extracto {
        let mut v = leer(t).unwrap();
        assert_eq!(v.len(), 1);
        v.remove(0)
    }

    #[test]
    fn cuenta_en_euros() {
        let mut e = uno(&EUR.join("\n"));
        assert_eq!(e.formato, "PDF");
        assert_eq!(e.banco, "BBVA");
        assert_eq!(e.iban.as_deref(), Some("ES9121000418450200051332"));
        assert_eq!(e.cuenta, "210004180200051332");
        assert_eq!(e.titular, "EMPRESA DEMO SL");
        assert_eq!(e.divisa, "EUR");
        assert_eq!(e.clase, Clase::Cuenta);
        assert_eq!(e.desde, Some(date!(2026 - 03 - 01)));
        assert_eq!(e.hasta, Some(date!(2026 - 04 - 15)));
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
        let d = |f: Date| Some(f);
        assert_eq!(
            filas,
            [
                (
                    date!(2026 - 03 - 02),
                    d(date!(2026 - 03 - 02)),
                    242_000,
                    Some(342_000),
                    "TRANSFERENCIAS | CLIENTE FICTICIO UNO SL · FACTURA F-2026-001"
                ),
                (
                    date!(2026 - 03 - 05),
                    d(date!(2026 - 03 - 04)),
                    -8_530,
                    Some(333_470),
                    "ADEUDO A SU CARGO | PROVEEDOR LUZ DEMO SA · N 2026064000000001 PROVEEDOR \
                     LUZ DEMO SA"
                ),
                (
                    date!(2026 - 03 - 06),
                    d(date!(2026 - 03 - 05)),
                    -4_215,
                    Some(329_255),
                    "ADEUDO MENSUAL DE TARJETA · ************0000"
                ),
                (
                    date!(2026 - 03 - 10),
                    d(date!(2026 - 03 - 10)),
                    -115_000,
                    Some(214_255),
                    "CARGO POR PAGO DE IMPUESTOS - TRIBUTOS · NRC. 0000000000000DEMO00000"
                ),
                (
                    date!(2026 - 03 - 12),
                    d(date!(2026 - 03 - 12)),
                    50_000,
                    Some(264_255),
                    "TRANSFERENCIAS | EMPRESA DEMO SOCIEDAD LIMITADA. · TRASPASO"
                ),
                (
                    date!(2026 - 03 - 16),
                    d(date!(2026 - 03 - 16)),
                    -12_100,
                    Some(252_155),
                    "ADEUDO A SU CARGO | ASESORIA FICTICIA DEMO SL · N 2026075000000002 \
                     ASESORIA FICTICIA DEMO SL"
                ),
                (
                    date!(2026 - 03 - 20),
                    d(date!(2026 - 03 - 20)),
                    363_000,
                    Some(615_155),
                    "TRANSFERENCIAS | CLIENTE FICTICIO DOS SL · PAGO FACTURAS F-2026-002 Y \
                     F-2026-003 DE EMPRESA DEMO SL"
                ),
                (
                    date!(2026 - 03 - 31),
                    d(date!(2026 - 03 - 30)),
                    6_030,
                    Some(621_185),
                    "CASHBACK DE TARJETA · BONIF. DEVOLUCION IMPUESTOS"
                ),
                (
                    date!(2026 - 03 - 31),
                    d(date!(2026 - 03 - 30)),
                    -1_170,
                    Some(620_015),
                    "RETENCIóN PROMOCION COMERCIAL · BONIF. DEVOLUCION IMPUESTOS"
                ),
                (
                    date!(2026 - 04 - 02),
                    d(date!(2026 - 04 - 02)),
                    31_047,
                    Some(651_062),
                    "TRANSFERENCIAS | DEVOLUCIONES TRIBUTARIAS, AGENCIA ESTATAL DE \
                     ADMINISTRACION TRIBUTARIA · IMPUESTO SOBRE SOCIEDADES"
                ),
                (
                    date!(2026 - 04 - 08),
                    d(date!(2026 - 04 - 08)),
                    -187_550,
                    Some(463_512),
                    "PAGO DE NOMINAS POR SU CUENTA | TRABAJADOR FICTICIO DEMO · Nomina Marzo \
                     2026"
                ),
                (
                    date!(2026 - 04 - 14),
                    d(date!(2026 - 04 - 14)),
                    -100_000,
                    Some(363_512),
                    "TRANSFERENCIAS | SOCIO FICTICIO DEMO · 1/2"
                ),
                (
                    date!(2026 - 04 - 15),
                    d(date!(2026 - 04 - 15)),
                    1_250_080,
                    Some(1_613_592),
                    "ABONO POR TRANSFERENCIA A SU FAVOR RECIBIDA EN EUROS | PAGADOR FICTICIO \
                     LTD · LIQ. OP. N 000000000000001"
                ),
            ]
        );
        assert_eq!((e.saldo_inicial, e.saldo_final), (None, None));
        assert!(e.cerrar().cuadra());
        assert_eq!(e.saldo_inicial, Some(Money::from_cents(100_000)));
        assert_eq!(e.saldo_final, Some(Money::from_cents(1_613_592)));
    }

    #[test]
    fn cuenta_en_dolares() {
        let mut e = uno(&USD.join("\n"));
        assert_eq!(e.iban.as_deref(), Some("ES7921000813610123456789"));
        assert_eq!(e.cuenta, "210008130123456789");
        assert_eq!(e.divisa, "USD");
        assert_eq!(e.desde, Some(date!(2026 - 01 - 01)));
        assert_eq!(e.hasta, Some(date!(2026 - 04 - 01)));
        let filas: Vec<_> = e
            .movimientos
            .iter()
            .map(|m| {
                (
                    m.fecha,
                    m.importe.cents(),
                    m.saldo.map(Money::cents),
                    &*m.concepto,
                )
            })
            .collect();
        assert_eq!(
            filas,
            [
                (
                    date!(2026 - 02 - 10),
                    500_000,
                    Some(500_000),
                    "TRASPASO | CLIENTE FICTICIO USA LLC"
                ),
                (
                    date!(2026 - 03 - 03),
                    -475_025,
                    Some(24_975),
                    "CARGO POR TRANSFERENCIA EMITIDA EN DIVISAS | BROKER FICTICIO SA · \
                     0000000001 00000000000001"
                ),
                (
                    date!(2026 - 03 - 20),
                    12_050,
                    Some(37_025),
                    "TRASPASO | EMPRESA DEMO, S.L. · 0000000002 00000000000002"
                ),
            ]
        );
        assert!(e.cerrar().cuadra());
        assert_eq!(e.saldo_inicial, Some(Money::ZERO));
        assert_eq!(e.saldo_final, Some(Money::from_cents(37_025)));
    }

    #[test]
    fn el_periodo_no_pasa_del_dia_en_que_se_genero() {
        let e = uno(&USD.join("\n").replace(
            "Periodo 01/01/2026-01/04/2026",
            "Periodo 01/01/2026-06/04/2026",
        ));
        assert_eq!(e.hasta, Some(date!(2026 - 04 - 01)));
    }

    #[test]
    fn movimiento_sin_importe_y_listado_filtrado() {
        let t = USD
            .join("\n")
            .replace(" 5.000,00 USD 5.000,00 USD", "")
            .replace("Filtros Todos", "Filtros Cargos");
        let e = uno(&t);
        assert_eq!(e.movimientos.len(), 2);
        assert_eq!(e.avisos.len(), 2, "{:?}", e.avisos);
    }

    #[test]
    fn observaciones_que_parecen_un_numero_de_pagina() {
        let t = USD.join("\n").replace(
            "0000000001 00000000000001 -4.750,25 USD 249,75 USD",
            "1/3\n -4.750,25 USD 249,75 USD",
        );
        let e = uno(&t);
        assert_eq!(
            e.movimientos[1].concepto,
            "CARGO POR TRANSFERENCIA EMITIDA EN DIVISAS | BROKER FICTICIO SA · 1/3"
        );
        assert_eq!(e.movimientos[1].importe.cents(), -475_025);
    }

    #[test]
    fn sin_la_huella_de_bbva_no_es_suyo() {
        assert!(
            leer(
                &EUR.join("\n")
                    .replace("BANCO BILBAO VIZCAYA ARGENTARIA", "OTRO BANCO")
            )
            .is_none()
        );
        assert!(leer(&EUR.join("\n").replace("F. CONTABLE", "FECHA")).is_none());
    }
}
