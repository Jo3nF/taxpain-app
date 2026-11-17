//! Extractos en PDF de bancos conocidos, de punta a punta: el PDF (también dentro de su sobre
//! MIME), el texto que saca la app, el perfil del banco y la comprobación con los saldos. Los
//! PDF se escriben aquí con datos inventados: cada línea es un párrafo, así que el lector de PDF
//! las separa con una línea en blanco, como en los extractos de verdad.

#![allow(clippy::unwrap_used)]

use taxpain_app::extractos::{self, Clase, Cuadre};
use taxpain_core::money::Money;
use taxpain_invoicing::pdf::{Para, text_document};
use time::macros::date;

/// PDF con una línea de texto por párrafo.
fn pdf(lineas: &[&str]) -> Vec<u8> {
    let doc: Vec<Para> = lineas.iter().map(|l| Para::Text((*l).into())).collect();
    text_document("Extracto", &[doc]).unwrap()
}

/// El PDF dentro del sobre MIME con el que lo da la banca en línea de BBVA.
fn en_sobre(pdf: &[u8]) -> Vec<u8> {
    let frontera = "uuid:00000000-0000-4000-8000-000000000000";
    let mut out = format!(
        "\r\n--{frontera}\r\nContent-Type: multipart/mixed\r\nContent-Transfer-Encoding: \
         binary\r\nContent-ID: <BBVA+Historico+movimientos>\r\n\r\n"
    )
    .into_bytes();
    out.extend_from_slice(pdf);
    out.extend_from_slice(format!("\r\n--{frontera}--").as_bytes());
    out
}

#[test]
fn historico_de_bbva_dentro_de_su_sobre() {
    let lineas = [
        "F. CONTABLE F. VALOR CÓDIGO CONCEPTO|BENEF./ORDENANTE IMPORTE SALDO",
        "20/03/2026 20/03/2026 00007 TRANSFERENCIAS | CLIENTE FICTICIO DOS SL",
        "FACTURA F-2026-002 3.630,00 EUR 6.151,5 EUR",
        "10/03/2026 10/03/2026 00326 CARGO POR PAGO DE IMPUESTOS - TRIBUTOS",
        "NRC. 0000000000000DEMO00000 -1.150,00 EUR 2.521,5 EUR",
        "02/03/2026 02/03/2026 00007 TRANSFERENCIAS | CLIENTE FICTICIO UNO SL 2.420,00 EUR \
         3.671,50 EUR",
        "Titular EMPRESA DEMO SL Fecha 15/04/2026 Hora 10:30",
        "Cuenta ES9121000418450200051332 Periodo 01/03/2026-15/04/2026",
        "Divisa EUR Importe Todos",
        "Banco BANCO BILBAO VIZCAYA ARGENTARIA S.A. Filtros Todos",
        " /1 1",
        "Movimientos",
    ];
    let bytes = en_sobre(&pdf(&lineas));
    assert!(!bytes.starts_with(b"%PDF"));
    let leidos = extractos::leer("BBVA Histórico movimientos.pdf", &bytes).unwrap();
    assert_eq!(leidos.len(), 1);
    let l = &leidos[0];
    assert_eq!(l.cuadre, Cuadre::Cuadra);
    let e = &l.extracto;
    assert_eq!(e.formato, "PDF");
    assert_eq!(e.banco, "BBVA");
    assert_eq!(e.iban.as_deref(), Some("ES9121000418450200051332"));
    assert_eq!(e.cuenta, "210004180200051332");
    assert_eq!(e.titular, "EMPRESA DEMO SL");
    assert_eq!(e.clase, Clase::Cuenta);
    assert_eq!(
        (e.desde, e.hasta),
        (Some(date!(2026 - 03 - 01)), Some(date!(2026 - 04 - 15)))
    );
    assert_eq!(e.saldo_inicial, Some(Money::from_cents(125_150)));
    assert_eq!(e.saldo_final, Some(Money::from_cents(615_150)));
    let filas: Vec<_> = e
        .movimientos
        .iter()
        .map(|m| (m.fecha, m.importe.cents(), m.concepto.as_str()))
        .collect();
    assert_eq!(
        filas,
        [
            (
                date!(2026 - 03 - 02),
                242_000,
                "TRANSFERENCIAS | CLIENTE FICTICIO UNO SL"
            ),
            (
                date!(2026 - 03 - 10),
                -115_000,
                "CARGO POR PAGO DE IMPUESTOS - TRIBUTOS · NRC. 0000000000000DEMO00000"
            ),
            (
                date!(2026 - 03 - 20),
                363_000,
                "TRANSFERENCIAS | CLIENTE FICTICIO DOS SL · FACTURA F-2026-002"
            ),
        ]
    );
}

#[test]
fn extracto_de_banca_march() {
    let lineas = [
        "Banca March - https://telemarch.bancamarch.es",
        "mié, 12 ene 2022, 09:15 CET",
        "Cuentas a la vista - Extracto de movimientos",
        "Fecha de 01/12/2021 hasta 05/01/2022",
        "ES91 2100 0418 4502 0005 1332 - EMPRESA DEMO SL.",
        "Saldo actual: 47.383,00 € Retenciones: 0,00 € Saldo disponible: 47.383,00 €",
        "F. operación F. valor Oficina Concepto Importe Saldo",
        "03/12/2021 03/12 0300 TRANSFER. CLIENTE FICTICIO SA 2.500,00 € 47.403,20 €",
        "01/01/2022 31/12 0200 LIQUID.PROPIA CUENTA -10,00 € 47.393,20 €",
        "03/01/2022 01/01 0100 4B COMPRA PAPELERIA EJEMPLO -10,20 € 47.383,00 €",
    ];
    let leidos = extractos::leer("extracto.pdf", &pdf(&lineas)).unwrap();
    assert_eq!(leidos.len(), 1);
    assert_eq!(leidos[0].cuadre, Cuadre::Cuadra);
    let e = &leidos[0].extracto;
    assert_eq!(e.banco, "Banca March");
    assert_eq!(e.cuenta, "210004180200051332");
    assert_eq!(e.titular, "EMPRESA DEMO SL");
    assert_eq!(e.saldo_inicial, Some(Money::from_cents(4_490_320)));
    assert_eq!(e.saldo_final, Some(Money::from_cents(4_738_300)));
    assert!(e.avisos.is_empty(), "{:?}", e.avisos);
    let m = &e.movimientos[1];
    assert_eq!(
        (
            m.fecha,
            m.fecha_valor,
            m.importe.cents(),
            m.concepto.as_str()
        ),
        (
            date!(2022 - 01 - 01),
            Some(date!(2021 - 12 - 31)),
            -1_000,
            "LIQUID.PROPIA CUENTA"
        )
    );
}

#[test]
fn listado_de_la_tarjeta_de_bbva_sin_saldos() {
    let lineas = [
        "FECHA DE OPERACIÓN CONCEPTO TIPO DE MOVIMIENTO IMPORTE",
        "2026-05-05 RECIBO MES ANTERIOR Sin categoría 99,45 EUR",
        "2026-04-18 RESTAURANTE FICTICIO Compra -87,25 EUR",
        "2026-04-11 HOSTING DEMO.COM* X1Y2 Compra -12,20 EUR",
        "Nº de tarjeta 540000******1234 Fecha 30/06/2026",
        "Tipo de tarjeta TARJETA NEGOCIOS CREDITO Periodo 01/04/2026 - 30/06/2026",
        "Nº de contrato 01820000000000000000001234 Tipo de movimiento TODOS",
        "Titular de la tarjeta EMPRESA DEMO SL Importes TODOS",
        " /1 1",
        "Listado de movimientos de tarjeta",
    ];
    let leidos = extractos::leer("tarjeta.pdf", &pdf(&lineas)).unwrap();
    assert_eq!(leidos.len(), 1);
    assert_eq!(leidos[0].cuadre, Cuadre::SinSaldos);
    let e = &leidos[0].extracto;
    assert_eq!((e.banco.as_str(), e.clase), ("BBVA", Clase::TarjetaCredito));
    assert_eq!(e.cuenta, "tarjeta:0182:1234");
    assert_eq!(
        e.movimientos
            .iter()
            .map(|m| m.importe.cents())
            .collect::<Vec<_>>(),
        [-1_220, -8_725, 9_945]
    );
}
