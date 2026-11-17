//! Lector genérico de extractos en PDF con PDF hechos aquí mismo, con el texto colocado en
//! su sitio como lo pondrían bancos con maquetas distintas (columnas de cargo y abono, debe y
//! haber, «Money out / Money in», alemán, del más nuevo al más antiguo, filas centradas,
//! cabeceras repetidas en cada página…). Ningún dato es real.

#![allow(clippy::unwrap_used)]

use taxpain_app::extractos::{self, Clase, Cuadre, Leido};

/// Un texto en una página: `x` e `y` en puntos desde arriba a la izquierda y tamaño de letra.
type Texto<'a> = (i32, i32, i32, &'a str);

/// Texto de un PDF (WinAnsi) con su escape.
fn cadena(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        let codigo: u32 = match c {
            '€' => 0x80,
            '−' => u32::from(b'-'),
            c => u32::from(c),
        };
        match c {
            '(' | ')' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            _ if (0x20..0x7f).contains(&codigo) => out.push(char::from_u32(codigo).unwrap()),
            _ if codigo <= 0xff => out.push_str(&format!("\\{codigo:03o}")),
            _ => out.push('?'),
        }
    }
    out
}

/// Un PDF A4 con una página por cada lista de textos, en Helvetica.
fn pdf(paginas: &[Vec<Texto<'_>>]) -> Vec<u8> {
    pdf_con_giro(paginas, &[])
}

/// Un PDF como [`pdf`], con el texto de las páginas de `giradas` dibujado girado un cuarto de
/// vuelta (como en una página apaisada) y, en las demás, los textos que empiezan por «Inscrita
/// en el Registro» en vertical (como los textos legales del margen).
fn pdf_con_giro(paginas: &[Vec<Texto<'_>>], giradas: &[usize]) -> Vec<u8> {
    let n = paginas.len();
    let mut objetos: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {n} >>",
            (0..n)
                .map(|i| format!("{} 0 R", 4 + 2 * i))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
    for (i, textos) in paginas.iter().enumerate() {
        let mut contenido = String::new();
        for (x, y, tam, t) in textos {
            let matriz = if giradas.contains(&i) {
                // Lo que se lee de izquierda a derecha va de abajo arriba, y las líneas, de
                // izquierda a derecha.
                format!("0 1 -1 0 {y} {x}")
            } else if t.starts_with("Inscrita en el Registro") {
                format!("0 1 -1 0 {x} {}", 842 - y)
            } else {
                format!("1 0 0 1 {x} {}", 842 - y)
            };
            contenido.push_str(&format!(
                "BT /F1 {tam} Tf {matriz} Tm ({}) Tj ET\n",
                cadena(t)
            ));
        }
        objetos.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 \
             3 0 R >> >> /Contents {} 0 R >>",
            5 + 2 * i
        ));
        objetos.push(format!(
            "<< /Length {} >>\nstream\n{contenido}endstream",
            contenido.len()
        ));
    }
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut posiciones = Vec::new();
    for (i, o) in objetos.iter().enumerate() {
        posiciones.push(out.len());
        out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objetos.len() + 1).bytes());
    for p in posiciones {
        out.extend(format!("{p:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objetos.len() + 1
        )
        .bytes(),
    );
    out
}

fn leer(paginas: &[Vec<Texto<'_>>]) -> Vec<Leido> {
    extractos::leer("extracto-demo.pdf", &pdf(paginas)).unwrap()
}

fn importes(l: &Leido) -> Vec<i64> {
    l.extracto
        .movimientos
        .iter()
        .map(|m| m.importe.cents())
        .collect()
}

fn fechas(l: &Leido) -> Vec<String> {
    l.extracto
        .movimientos
        .iter()
        .map(|m| m.fecha.to_string())
        .collect()
}

/// Banco español clásico: columnas de cargo y abono sin signo, saldo, saldo anterior y final,
/// conceptos en dos líneas.
#[test]
fn cargos_y_abonos_en_columnas_con_saldo_anterior() {
    let p = vec![
        (40, 40, 14, "Banco Demo - Extracto de cuenta"),
        (40, 70, 9, "Titular: EMPRESA DEMO SL"),
        (40, 84, 9, "IBAN ES91 2100 0418 4502 0005 1332"),
        (40, 98, 9, "Periodo del 01/03/2024 al 31/03/2024"),
        (40, 130, 8, "Fecha"),
        (95, 130, 8, "Valor"),
        (150, 130, 8, "Concepto"),
        (380, 130, 8, "Cargo"),
        (450, 130, 8, "Abono"),
        (520, 130, 8, "Saldo"),
        (150, 145, 8, "SALDO ANTERIOR"),
        (510, 145, 8, "1.000,00"),
        (40, 160, 8, "04/03/2024"),
        (95, 160, 8, "04/03"),
        (150, 160, 8, "RECIBO LUZ"),
        (375, 160, 8, "45,30"),
        (515, 160, 8, "954,70"),
        (150, 170, 8, "COMPAÑIA ELECTRICA DEMO"),
        (40, 185, 8, "15/03/2024"),
        (95, 185, 8, "15/03"),
        (150, 185, 8, "TRANSFERENCIA DE CLIENTE DEMO"),
        (440, 185, 8, "1.500,00"),
        (510, 185, 8, "2.454,70"),
        (40, 200, 8, "20/03/2024"),
        (95, 200, 8, "20/03"),
        (150, 200, 8, "COMISION MANTENIMIENTO"),
        (378, 200, 8, "20,00"),
        (510, 200, 8, "2.434,70"),
        (150, 215, 8, "SALDO FINAL"),
        (510, 215, 8, "2.434,70"),
        (500, 800, 7, "Página 1 de 1"),
    ];
    let l = leer(&[p]);
    assert_eq!(l.len(), 1);
    let e = &l[0].extracto;
    assert_eq!(l[0].cuadre, Cuadre::Cuadra, "{:?}", e.avisos);
    assert_eq!(importes(&l[0]), [-4530, 150_000, -2000]);
    assert_eq!(e.iban.as_deref(), Some("ES9121000418450200051332"));
    assert_eq!(e.cuenta, "210004180200051332");
    assert_eq!(e.titular, "EMPRESA DEMO SL");
    assert_eq!(e.banco, "CaixaBank");
    assert_eq!(e.saldo_inicial.map(|m| m.cents()), Some(100_000));
    assert_eq!(e.saldo_final.map(|m| m.cents()), Some(243_470));
    assert_eq!(
        e.desde.map(|d| d.to_string()).as_deref(),
        Some("2024-03-01")
    );
    assert_eq!(
        e.movimientos[0].concepto,
        "RECIBO LUZ COMPAÑIA ELECTRICA DEMO"
    );
    assert_eq!(
        e.movimientos[0]
            .fecha_valor
            .map(|d| d.to_string())
            .as_deref(),
        Some("2024-03-04")
    );
    assert_eq!(e.movimientos[2].saldo.map(|m| m.cents()), Some(243_470));
}

/// Debe y haber, del más nuevo al más antiguo, en dos páginas con la cabecera repetida y pies
/// de página.
#[test]
fn debe_y_haber_del_mas_nuevo_al_mas_antiguo_en_dos_paginas() {
    let cabecera = |y: i32| {
        vec![
            (40, y, 8, "F. operación"),
            (150, y, 8, "Descripción"),
            (380, y, 8, "Debe"),
            (450, y, 8, "Haber"),
            (520, y, 8, "Saldo"),
        ]
    };
    let fila = |y: i32,
                f: &'static str,
                c: &'static str,
                debe: Option<&'static str>,
                haber: Option<&'static str>,
                s: &'static str| {
        let mut v = vec![(40, y, 8, f), (150, y, 8, c), (510, y, 8, s)];
        if let Some(d) = debe {
            v.push((380, y, 8, d));
        }
        if let Some(h) = haber {
            v.push((450, y, 8, h));
        }
        v
    };
    let mut p1 = vec![
        (40, 40, 12, "Extracto de movimientos"),
        (40, 60, 9, "Cuenta ES91 2100 0418 4502 0005 1332"),
    ];
    p1.extend(cabecera(100));
    p1.extend(fila(
        115,
        "28/06/2024",
        "PAGO PROVEEDOR DEMO",
        Some("300,00"),
        None,
        "1.700,00",
    ));
    p1.extend(fila(
        130,
        "20/06/2024",
        "COBRO FACTURA 12",
        None,
        Some("500,00"),
        "2.000,00",
    ));
    p1.extend(fila(
        145,
        "20/06/2024",
        "COMISION",
        Some("10,00"),
        None,
        "1.500,00",
    ));
    p1.push((
        40,
        790,
        7,
        "Este documento no sustituye al extracto oficial.",
    ));
    p1.push((500, 800, 7, "Página 1 de 2"));
    let mut p2 = vec![(40, 40, 12, "Extracto de movimientos")];
    p2.extend(cabecera(100));
    p2.extend(fila(
        115,
        "10/06/2024",
        "TRANSFERENCIA RECIBIDA",
        None,
        Some("1.010,00"),
        "1.510,00",
    ));
    p2.extend(fila(
        130,
        "02/06/2024",
        "RECIBO SEGURO",
        Some("90,00"),
        None,
        "500,00",
    ));
    p2.push((
        40,
        790,
        7,
        "Este documento no sustituye al extracto oficial.",
    ));
    p2.push((500, 800, 7, "Página 2 de 2"));
    let l = leer(&[p1, p2]);
    assert_eq!(l.len(), 1);
    assert_eq!(l[0].cuadre, Cuadre::Cuadra, "{:?}", l[0].extracto.avisos);
    // En orden cronológico, también dentro del mismo día.
    assert_eq!(importes(&l[0]), [-9000, 101_000, -1000, 50_000, -30_000]);
    assert_eq!(
        fechas(&l[0]),
        [
            "2024-06-02",
            "2024-06-10",
            "2024-06-20",
            "2024-06-20",
            "2024-06-28"
        ]
    );
    assert_eq!(l[0].extracto.saldo_inicial.map(|m| m.cents()), Some(59_000));
}

/// Banco en inglés: «Money out / Money in / Balance», fechas con el mes en letra y libras.
#[test]
fn money_out_money_in_en_libras() {
    let p = vec![
        (40, 40, 12, "Demo Bank plc - Statement"),
        (40, 60, 9, "Account holder: EMPRESA DEMO LTD"),
        (40, 74, 9, "IBAN: GB82 WEST 1234 5698 7654 32"),
        (40, 88, 9, "Currency: GBP"),
        (40, 102, 9, "Statement period: 1 Jan 2025 to 31 Jan 2025"),
        (40, 130, 8, "Date"),
        (120, 130, 8, "Description"),
        (360, 130, 8, "Money out"),
        (430, 130, 8, "Money in"),
        (510, 130, 8, "Balance"),
        (40, 145, 8, "Opening balance"),
        (505, 145, 8, "£2,000.00"),
        (40, 160, 8, "3 Jan 2025"),
        (120, 160, 8, "Card payment DEMO SHOP"),
        (365, 160, 8, "£45.10"),
        (505, 160, 8, "£1,954.90"),
        (40, 175, 8, "10 Jan 2025"),
        (120, 175, 8, "Transfer from CLIENT DEMO"),
        (430, 175, 8, "£1,200.00"),
        (505, 175, 8, "£3,154.90"),
        (40, 190, 8, "31 Jan 2025"),
        (120, 190, 8, "Direct debit DEMO INSURANCE"),
        (360, 190, 8, "£154.90"),
        (505, 190, 8, "£3,000.00"),
        (40, 205, 8, "Closing balance"),
        (505, 205, 8, "£3,000.00"),
    ];
    let l = leer(&[p]);
    let e = &l[0].extracto;
    assert_eq!(l[0].cuadre, Cuadre::Cuadra, "{:?}", e.avisos);
    assert_eq!(e.divisa, "GBP");
    assert_eq!(e.cuenta, "GB82WEST12345698765432");
    assert_eq!(e.titular, "EMPRESA DEMO LTD");
    assert_eq!(importes(&l[0]), [-4510, 120_000, -15_490]);
    assert_eq!(fechas(&l[0])[0], "2025-01-03");
}

/// Banco alemán: «Buchungstag / Valuta / Verwendungszweck / Betrag / Saldo», con el menos
/// detrás y fechas sin año en la columna de valuta.
#[test]
fn aleman_con_el_menos_detras() {
    let p = vec![
        (40, 40, 12, "Kontoauszug"),
        (40, 60, 9, "Kontoinhaber: EMPRESA DEMO GMBH"),
        (40, 74, 9, "IBAN DE89 3704 0044 0532 0130 00"),
        (40, 88, 9, "Kontoauszug vom 01.02.2025 bis 28.02.2025"),
        (40, 120, 8, "Buchungstag"),
        (100, 120, 8, "Valuta"),
        (150, 120, 8, "Verwendungszweck"),
        (420, 120, 8, "Betrag"),
        (510, 120, 8, "Saldo"),
        (40, 135, 8, "Alter Kontostand"),
        (505, 135, 8, "5.000,00"),
        (40, 150, 8, "03.02.2025"),
        (100, 150, 8, "03.02."),
        (150, 150, 8, "Miete Februar DEMO"),
        (410, 150, 8, "1.200,00-"),
        (505, 150, 8, "3.800,00"),
        (40, 165, 8, "14.02.2025"),
        (100, 165, 8, "14.02."),
        (150, 165, 8, "Gutschrift KUNDE DEMO"),
        (410, 165, 8, "2.345,67+"),
        (505, 165, 8, "6.145,67"),
        (40, 180, 8, "Neuer Kontostand"),
        (505, 180, 8, "6.145,67"),
    ];
    let l = leer(&[p]);
    let e = &l[0].extracto;
    assert_eq!(l[0].cuadre, Cuadre::Cuadra, "{:?}", e.avisos);
    assert_eq!(importes(&l[0]), [-120_000, 234_567]);
    assert_eq!(e.cuenta, "DE89370400440532013000");
    assert_eq!(
        e.movimientos[1]
            .fecha_valor
            .map(|d| d.to_string())
            .as_deref(),
        Some("2025-02-14")
    );
    assert_eq!(e.saldo_inicial.map(|m| m.cents()), Some(500_000));
}

/// Una sola columna de importe sin signo y el saldo: el signo sale del saldo.
#[test]
fn importe_sin_signo_y_saldo() {
    let p = vec![
        (40, 50, 9, "Cuenta ES91 2100 0418 4502 0005 1332"),
        (40, 100, 8, "Fecha"),
        (130, 100, 8, "Concepto"),
        (420, 100, 8, "Importe"),
        (510, 100, 8, "Saldo"),
        (40, 115, 8, "01/04/2024"),
        (130, 115, 8, "INGRESO EFECTIVO"),
        (415, 115, 8, "300,00"),
        (505, 115, 8, "1.300,00"),
        (40, 130, 8, "02/04/2024"),
        (130, 130, 8, "PAGO TARJETA DEMO"),
        (420, 130, 8, "25,50"),
        (505, 130, 8, "1.274,50"),
        (40, 145, 8, "05/04/2024"),
        (130, 145, 8, "RECIBO TELEFONO"),
        (420, 145, 8, "74,50"),
        (505, 145, 8, "1.200,00"),
    ];
    let l = leer(&[p]);
    assert_eq!(l[0].cuadre, Cuadre::Cuadra, "{:?}", l[0].extracto.avisos);
    // El primero no tiene saldo anterior con el que compararse: se deja como ingreso y el saldo
    // inicial sale de él.
    assert_eq!(importes(&l[0]), [30_000, -2550, -7450]);
}

/// Sin columna de saldo: el signo sale de la columna de cargos o de abonos.
#[test]
fn sin_saldo_el_signo_por_la_columna() {
    let p = vec![
        (40, 50, 9, "IBAN ES91 2100 0418 4502 0005 1332"),
        (40, 100, 8, "Fecha"),
        (130, 100, 8, "Concepto"),
        (420, 100, 8, "Cargos"),
        (500, 100, 8, "Abonos"),
        (40, 115, 8, "01/04/2024"),
        (130, 115, 8, "COBRO CLIENTE DEMO"),
        (495, 115, 8, "1.000,00"),
        (40, 130, 8, "02/04/2024"),
        (130, 130, 8, "PAGO PROVEEDOR DEMO"),
        (425, 130, 8, "250,00"),
    ];
    let l = leer(&[p]);
    assert_eq!(l[0].cuadre, Cuadre::SinSaldos);
    assert_eq!(importes(&l[0]), [100_000, -25_000]);
}

/// Filas centradas: el concepto va en dos líneas, una encima y otra debajo de la línea con la
/// fecha y los importes, y la cabecera, partida en dos líneas.
#[test]
fn filas_centradas_y_cabecera_en_dos_lineas() {
    let p = vec![
        (30, 40, 16, "Movimientos"),
        (30, 70, 8, "Titular"),
        (100, 70, 8, "EMPRESA DEMO SL"),
        (30, 84, 8, "Cuenta"),
        (100, 84, 8, "ES9121000418450200051332"),
        (30, 98, 8, "Divisa"),
        (100, 98, 8, "EUR"),
        (30, 130, 7, "Fecha"),
        (100, 130, 7, "Fecha"),
        (200, 130, 7, "Concepto"),
        (440, 130, 7, "Importe"),
        (530, 130, 7, "Saldo"),
        (30, 139, 7, "operación"),
        (100, 139, 7, "valor"),
        (200, 160, 8, "TRANSFERENCIAS | CLIENTE DEMO"),
        (30, 166, 8, "10/05/2024"),
        (100, 166, 8, "10/05/2024"),
        (420, 166, 8, "800,00 EUR"),
        (510, 166, 8, "2.800,00 EUR"),
        (200, 172, 6, "Factura 7"),
        (200, 200, 8, "RECIBO | SUMINISTROS DEMO"),
        (30, 206, 8, "12/05/2024"),
        (100, 206, 8, "12/05/2024"),
        (425, 206, 8, "-60,5 EUR"),
        (510, 206, 8, "2.739,5 EUR"),
        (200, 212, 6, "Mayo"),
    ];
    let l = leer(&[p]);
    let e = &l[0].extracto;
    assert_eq!(l[0].cuadre, Cuadre::Cuadra, "{:?}", e.avisos);
    assert_eq!(importes(&l[0]), [80_000, -6050]);
    assert_eq!(
        e.movimientos[0].concepto,
        "TRANSFERENCIAS | CLIENTE DEMO Factura 7"
    );
    assert_eq!(e.movimientos[1].concepto, "RECIBO | SUMINISTROS DEMO Mayo");
    assert_eq!(e.saldo_inicial.map(|m| m.cents()), Some(200_000));
}

/// Un PDF con una sección por divisa: un extracto por cada una.
#[test]
fn una_seccion_por_divisa() {
    let seccion =
        |divisa: &'static str,
         filas: &[(&'static str, &'static str, &'static str, &'static str)]| {
            let mut v = vec![
                (40, 40, 12, "Extracto de cuenta"),
                (40, 60, 9, "IBAN"),
                (120, 60, 9, "GB82 WEST 1234 5698 7654 32"),
                (40, 74, 9, "Divisa"),
                (120, 74, 9, divisa),
                (40, 110, 8, "Fecha"),
                (130, 110, 8, "Descripción"),
                (360, 110, 8, "Dinero saliente"),
                (440, 110, 8, "Dinero entrante"),
                (530, 110, 8, "Saldo"),
            ];
            for (k, (f, c, imp, s)) in filas.iter().enumerate() {
                let y = 125 + 15 * i32::try_from(k).unwrap();
                let x = if imp.starts_with('+') { 450 } else { 370 };
                v.extend([
                    (40, y, 8, *f),
                    (130, y, 8, *c),
                    (x, y, 8, imp.trim_start_matches('+')),
                    (525, y, 8, *s),
                ]);
            }
            v
        };
    let eur = seccion(
        "EUR",
        &[
            ("2 mar 2024", "Cobro demo", "+€100.00", "€100.00"),
            ("5 mar 2024", "Pago demo", "€40.00", "€60.00"),
        ],
    );
    let usd = seccion(
        "USD",
        &[("7 mar 2024", "Venta demo", "+$250.00", "$250.00")],
    );
    let l = leer(&[eur, usd]);
    assert_eq!(l.len(), 2);
    let divisas: Vec<&str> = l.iter().map(|x| x.extracto.divisa.as_str()).collect();
    assert!(
        divisas.contains(&"EUR") && divisas.contains(&"USD"),
        "{divisas:?}"
    );
    for x in &l {
        assert_eq!(x.cuadre, Cuadre::Cuadra);
        let n = if x.extracto.divisa == "EUR" { 2 } else { 1 };
        assert_eq!(x.extracto.movimientos.len(), n);
    }
}

/// Listado de una tarjeta sin saldos, con las compras sin signo y una devolución con «-».
#[test]
fn listado_de_tarjeta_con_compras_sin_signo() {
    let p = vec![
        (40, 40, 12, "Movimientos de la tarjeta de crédito"),
        (40, 60, 9, "Tarjeta"),
        (120, 60, 9, "4111 **** **** 1111"),
        (40, 74, 9, "Titular"),
        (120, 74, 9, "EMPRESA DEMO SL"),
        (40, 110, 8, "Fecha"),
        (130, 110, 8, "Comercio"),
        (480, 110, 8, "Importe"),
        (40, 125, 8, "03/05/2024"),
        (130, 125, 8, "LIBRERIA DEMO"),
        (480, 125, 8, "23,90"),
        (40, 140, 8, "08/05/2024"),
        (130, 140, 8, "GASOLINERA DEMO"),
        (480, 140, 8, "60,00"),
        (40, 155, 8, "09/05/2024"),
        (130, 155, 8, "DEVOLUCION LIBRERIA DEMO"),
        (480, 155, 8, "-23,90"),
    ];
    let l = leer(&[p]);
    let e = &l[0].extracto;
    assert_eq!(l[0].cuadre, Cuadre::SinSaldos);
    assert_eq!(e.clase, Clase::TarjetaCredito);
    assert_eq!(e.cuenta, "tarjeta:?:1111");
    assert_eq!(importes(&l[0]), [-2390, -6000, 2390]);
}

/// La fecha sólo en el primer movimiento de cada día: los de debajo la heredan.
#[test]
fn fecha_solo_en_el_primer_movimiento_del_dia() {
    let p = vec![
        (40, 50, 9, "IBAN GB82 WEST 1234 5698 7654 32"),
        (40, 100, 8, "Date"),
        (110, 100, 8, "Details"),
        (380, 100, 8, "Paid out"),
        (450, 100, 8, "Paid in"),
        (520, 100, 8, "Balance"),
        (40, 115, 8, "Balance brought forward"),
        (515, 115, 8, "500.00"),
        (40, 130, 8, "02 Apr 24"),
        (110, 130, 8, "CARD DEMO CAFE"),
        (385, 130, 8, "4.50"),
        (110, 145, 8, "CARD DEMO BOOKS"),
        (380, 145, 8, "15.50"),
        (515, 145, 8, "480.00"),
        (40, 160, 8, "03 Apr 24"),
        (110, 160, 8, "TRANSFER DEMO CLIENT"),
        (450, 160, 8, "120.00"),
        (515, 160, 8, "600.00"),
    ];
    let l = leer(&[p]);
    assert_eq!(l[0].cuadre, Cuadre::Cuadra, "{:?}", l[0].extracto.avisos);
    assert_eq!(importes(&l[0]), [-450, -1550, 12_000]);
    assert_eq!(fechas(&l[0]), ["2024-04-02", "2024-04-02", "2024-04-03"]);
}

/// Un PDF sin tabla de movimientos (una carta, una factura) no es un extracto.
#[test]
fn un_pdf_sin_movimientos_no_es_un_extracto() {
    let p = vec![
        (40, 40, 12, "Factura 2024-001"),
        (40, 60, 9, "Cliente: EMPRESA DEMO SL"),
        (40, 80, 9, "Fecha: 01/02/2024"),
        (40, 100, 9, "Total: 121,00 €"),
    ];
    assert!(extractos::leer("factura-demo.pdf", &pdf(&[p])).is_err());
}

/// Banco de Francia: «Date opération / Libellé / Débit / Crédit / Solde», con espacio de miles.
#[test]
fn extracto_de_francia_con_espacio_de_miles() {
    let p = vec![
        (40, 40, 12, "Relevé de compte"),
        (40, 60, 9, "Titulaire : EMPRESA DEMO SARL"),
        (40, 74, 9, "IBAN FR14 2004 1010 0505 0001 3M02 606"),
        (40, 88, 9, "Période du 01/09/2024 au 30/09/2024"),
        (40, 120, 8, "Date opération"),
        (130, 120, 8, "Libellé"),
        (380, 120, 8, "Débit"),
        (450, 120, 8, "Crédit"),
        (520, 120, 8, "Solde"),
        (130, 135, 8, "Solde précédent"),
        (505, 135, 8, "2 500,00"),
        (40, 150, 8, "05/09/2024"),
        (130, 150, 8, "PRLV DEMO ENERGIE"),
        (375, 150, 8, "1 234,56"),
        (510, 150, 8, "1 265,44"),
        (40, 165, 8, "12/09/2024"),
        (130, 165, 8, "VIR RECU CLIENT DEMO"),
        (445, 165, 8, "3 000,00"),
        (505, 165, 8, "4 265,44"),
        (130, 180, 8, "Nouveau solde"),
        (505, 180, 8, "4 265,44"),
    ];
    let l = leer(&[p]);
    let e = &l[0].extracto;
    assert_eq!(l[0].cuadre, Cuadre::Cuadra, "{:?}", e.avisos);
    assert_eq!(importes(&l[0]), [-123_456, 300_000]);
    assert_eq!(e.cuenta, "FR1420041010050500013M02606");
    assert_eq!(e.saldo_final.map(|m| m.cents()), Some(426_544));
}

/// Importes con la marca de debe o haber detrás («45,30 D», «1.500,00 H») y el saldo también.
#[test]
fn marcas_de_debe_y_haber() {
    let p = vec![
        (40, 50, 9, "C.C.C. 2100 0418 45 0200051332"),
        (40, 100, 8, "Fecha"),
        (130, 100, 8, "Concepto"),
        (410, 100, 8, "Importe"),
        (500, 100, 8, "Saldo"),
        (40, 115, 8, "03/02/2025"),
        (130, 115, 8, "RECIBO DEMO"),
        (405, 115, 8, "45,30 D"),
        (495, 115, 8, "954,70 H"),
        (40, 130, 8, "04/02/2025"),
        (130, 130, 8, "ABONO NOMINA DEMO"),
        (400, 130, 8, "1.500,00 H"),
        (490, 130, 8, "2.454,70 H"),
    ];
    let l = leer(&[p]);
    let e = &l[0].extracto;
    assert_eq!(l[0].cuadre, Cuadre::Cuadra, "{:?}", e.avisos);
    assert_eq!(importes(&l[0]), [-4530, 150_000]);
    // El IBAN, deducido del código cuenta cliente.
    assert_eq!(e.iban.as_deref(), Some("ES9121000418450200051332"));
}

/// Página apaisada con todo el texto girado y, en otra, un texto legal en vertical en el
/// margen, encima de las filas.
#[test]
fn texto_girado_y_textos_legales_en_vertical() {
    let tabla = |y0: i32| {
        vec![
            (40, y0, 9, "IBAN ES91 2100 0418 4502 0005 1332"),
            (40, y0 + 40, 8, "Concepto"),
            (250, y0 + 40, 8, "Fecha"),
            (400, y0 + 40, 8, "Importe"),
            (500, y0 + 40, 8, "Saldo"),
            (40, y0 + 55, 8, "RECIBO AGUA DEMO"),
            (250, y0 + 55, 8, "02/05/2024"),
            (395, y0 + 55, 8, "-30,00"),
            (490, y0 + 55, 8, "970,00"),
            (40, y0 + 70, 8, "COBRO CLIENTE DEMO"),
            (250, y0 + 70, 8, "06/05/2024"),
            (395, y0 + 70, 8, "130,00"),
            (485, y0 + 70, 8, "1.100,00"),
        ]
    };
    // La fecha no va en la primera columna.
    let l = extractos::leer("girado.pdf", &pdf_con_giro(&[tabla(60)], &[0])).unwrap();
    assert_eq!(l[0].cuadre, Cuadre::Cuadra, "{:?}", l[0].extracto.avisos);
    assert_eq!(importes(&l[0]), [-3000, 13_000]);
    assert_eq!(l[0].extracto.movimientos[0].concepto, "RECIBO AGUA DEMO");
    let mut p = tabla(60);
    p.push((
        20,
        160,
        6,
        "Inscrita en el Registro Mercantil de Demo, tomo 1, folio 2",
    ));
    let l = leer(&[p]);
    assert_eq!(l[0].cuadre, Cuadre::Cuadra);
    assert_eq!(l[0].extracto.movimientos[1].concepto, "COBRO CLIENTE DEMO");
}

/// Sin cabecera reconocible (la tabla sin títulos): por las líneas que empiezan por una fecha,
/// con los cargos y los abonos en dos columnas y el saldo al final.
#[test]
fn tabla_sin_cabecera_con_dos_columnas_de_importes() {
    let p = vec![
        (40, 50, 9, "IBAN ES91 2100 0418 4502 0005 1332"),
        (40, 100, 8, "03/06/2024"),
        (130, 100, 8, "RECIBO GAS DEMO"),
        (395, 100, 8, "61,20"),
        (500, 100, 8, "938,80"),
        (40, 115, 8, "05/06/2024"),
        (130, 115, 8, "INGRESO CHEQUE DEMO"),
        (460, 115, 8, "250,00"),
        (495, 115, 8, "1.188,80"),
        (40, 130, 8, "07/06/2024"),
        (130, 130, 8, "CUOTA ASOCIACION DEMO"),
        (395, 130, 8, "18,80"),
        (495, 130, 8, "1.170,00"),
    ];
    let l = leer(&[p]);
    assert_eq!(l[0].cuadre, Cuadre::Cuadra, "{:?}", l[0].extracto.avisos);
    assert_eq!(importes(&l[0]), [-6120, 25_000, -1880]);
}

/// Un PDF con dos cuentas (dos IBAN, cada uno con su tabla): un extracto por cuenta.
#[test]
fn dos_cuentas_en_el_mismo_pdf() {
    let cuenta = |iban: &'static str, f: &'static str, imp: &'static str, s: &'static str| {
        vec![
            (40, 40, 12, "Posición de cuentas"),
            (40, 60, 9, iban),
            (40, 100, 8, "Fecha"),
            (130, 100, 8, "Concepto"),
            (420, 100, 8, "Importe"),
            (510, 100, 8, "Saldo"),
            (40, 115, 8, f),
            (130, 115, 8, "MOVIMIENTO DEMO"),
            (420, 115, 8, imp),
            (505, 115, 8, s),
        ]
    };
    let a = cuenta(
        "IBAN ES91 2100 0418 4502 0005 1332",
        "02/07/2024",
        "-15,00",
        "485,00",
    );
    let b = cuenta(
        "IBAN GB82 WEST 1234 5698 7654 32",
        "03/07/2024",
        "40,00",
        "140,00",
    );
    let l = leer(&[a, b]);
    assert_eq!(l.len(), 2);
    let cuentas: Vec<&str> = l.iter().map(|x| x.extracto.cuenta.as_str()).collect();
    assert!(cuentas.contains(&"210004180200051332"), "{cuentas:?}");
    assert!(cuentas.contains(&"GB82WEST12345698765432"), "{cuentas:?}");
}

/// La fecha en una línea propia y debajo los movimientos de ese día, sin fecha.
#[test]
fn fecha_en_una_linea_propia_y_los_movimientos_del_dia_debajo() {
    let p = vec![
        (40, 50, 9, "IBAN ES91 2100 0418 4502 0005 1332"),
        (40, 100, 8, "Fecha"),
        (130, 100, 8, "Concepto"),
        (420, 100, 8, "Importe"),
        (510, 100, 8, "Saldo"),
        (40, 115, 8, "10/09/2024"),
        (130, 128, 8, "COMPRA DEMO UNO"),
        (420, 128, 8, "-12,00"),
        (505, 128, 8, "488,00"),
        (130, 141, 8, "COMPRA DEMO DOS"),
        (420, 141, 8, "-8,00"),
        (505, 141, 8, "480,00"),
        (40, 158, 8, "11/09/2024"),
        (130, 171, 8, "TRANSFERENCIA DEMO"),
        (420, 171, 8, "20,00"),
        (505, 171, 8, "500,00"),
    ];
    let l = leer(&[p]);
    assert_eq!(l[0].cuadre, Cuadre::Cuadra, "{:?}", l[0].extracto.avisos);
    assert_eq!(importes(&l[0]), [-1200, -800, 2000]);
    assert_eq!(fechas(&l[0]), ["2024-09-10", "2024-09-10", "2024-09-11"]);
}

/// Un IBAN en el concepto de un movimiento no es otra cuenta, aunque la tabla siga en otra
/// página con su cabecera; y la fecha valor, detrás de la de la operación en la misma columna.
#[test]
fn iban_en_un_concepto_y_dos_fechas_en_una_columna() {
    let cabecera = |y: i32| {
        vec![
            (40, y, 8, "Fechas"),
            (160, y, 8, "Concepto"),
            (420, y, 8, "Importe"),
            (510, y, 8, "Saldo"),
        ]
    };
    let mut p1 = vec![(40, 50, 9, "IBAN ES91 2100 0418 4502 0005 1332")];
    p1.extend(cabecera(100));
    p1.extend([
        (40, 115, 8, "02/12/2024 03/12/2024"),
        (160, 115, 8, "TRANSFERENCIA A GB82 WEST 1234 5698 7654 32"),
        (420, 115, 8, "-100,00"),
        (505, 115, 8, "900,00"),
    ]);
    let mut p2 = cabecera(60);
    p2.extend([
        (40, 75, 8, "09/12/2024 09/12/2024"),
        (160, 75, 8, "INGRESO DEMO"),
        (420, 75, 8, "50,00"),
        (505, 75, 8, "950,00"),
    ]);
    let l = leer(&[p1, p2]);
    assert_eq!(l.len(), 1);
    let e = &l[0].extracto;
    assert_eq!(e.cuenta, "210004180200051332");
    assert_eq!(l[0].cuadre, Cuadre::Cuadra);
    assert_eq!(
        e.movimientos[0]
            .fecha_valor
            .map(|d| d.to_string())
            .as_deref(),
        Some("2024-12-03")
    );
}

/// Una sola columna de fecha llamada «F. valor», categorías que no son el concepto y los
/// importes con el «(€)» en el título.
#[test]
fn una_sola_fecha_llamada_fecha_valor_y_categorias() {
    let p = vec![
        (40, 50, 9, "Número de cuenta: ES91 2100 0418 4502 0005 1332"),
        (40, 100, 7, "F. VALOR"),
        (100, 100, 7, "CATEGORÍA"),
        (170, 100, 7, "SUBCATEGORÍA"),
        (250, 100, 7, "DESCRIPCIÓN"),
        (400, 100, 7, "IMPORTE (€)"),
        (480, 100, 7, "SALDO (€)"),
        (40, 115, 8, "03/11/2024"),
        (100, 115, 8, "Hogar"),
        (170, 115, 8, "Luz"),
        (250, 115, 8, "Recibo luz demo"),
        (405, 115, 8, "-40,00"),
        (480, 115, 8, "1.460,00"),
        (40, 130, 8, "05/11/2024"),
        (100, 130, 8, "Ingresos"),
        (170, 130, 8, "Nómina"),
        (250, 130, 8, "Nómina empresa demo"),
        (400, 130, 8, "1.500,00"),
        (480, 130, 8, "2.960,00"),
    ];
    let l = leer(&[p]);
    let e = &l[0].extracto;
    assert_eq!(l[0].cuadre, Cuadre::Cuadra, "{:?}", e.avisos);
    assert_eq!(importes(&l[0]), [-4000, 150_000]);
    assert_eq!(e.movimientos[0].concepto, "Recibo luz demo");
    assert_eq!(fechas(&l[0]), ["2024-11-03", "2024-11-05"]);
}

/// Una factura con una tabla de fechas e importes y el IBAN del proveedor para pagarla no es un
/// extracto.
#[test]
fn una_factura_con_tabla_no_es_un_extracto() {
    let p = vec![
        (40, 40, 14, "FACTURA 2024-0042"),
        (40, 60, 9, "PROVEEDOR DEMO SL - NIF B00000000"),
        (40, 74, 9, "Cliente: EMPRESA DEMO SL"),
        (40, 110, 8, "Fecha"),
        (130, 110, 8, "Concepto"),
        (480, 110, 8, "Importe"),
        (40, 125, 8, "01/03/2024"),
        (130, 125, 8, "Servicio de mantenimiento"),
        (480, 125, 8, "100,00"),
        (40, 140, 8, "15/03/2024"),
        (130, 140, 8, "Revisión anual"),
        (480, 140, 8, "50,00"),
        (300, 170, 9, "Base imponible"),
        (480, 170, 9, "150,00"),
        (300, 185, 9, "IVA 21%"),
        (480, 185, 9, "31,50"),
        (300, 200, 9, "Total factura"),
        (480, 200, 9, "181,50"),
        (
            40,
            240,
            8,
            "Pago por transferencia a ES91 2100 0418 4502 0005 1332",
        ),
    ];
    let err = extractos::leer("factura-demo.pdf", &pdf(&[p])).unwrap_err();
    assert!(err.to_string().contains("no parece un extracto"), "{err}");
}

/// Una fila con importe cuya fecha tiene forma de fecha pero no se entiende en el orden de la
/// tabla (día y mes cambiados en una tabla que va día/mes): el movimiento se pierde y se avisa.
#[test]
fn una_fecha_que_no_se_entiende_avisa() {
    let p = vec![
        (40, 30, 11, "Extracto de movimientos"),
        (40, 50, 9, "Cuenta ES91 2100 0418 4502 0005 1332"),
        (40, 100, 8, "Fecha"),
        (130, 100, 8, "Concepto"),
        (420, 100, 8, "Importe"),
        (510, 100, 8, "Saldo"),
        (40, 115, 8, "28/02/2025"),
        (130, 115, 8, "INGRESO EFECTIVO"),
        (415, 115, 8, "300,00"),
        (505, 115, 8, "1.300,00"),
        (40, 130, 8, "05/13/2025"),
        (130, 130, 8, "PAGO TARJETA DEMO"),
        (420, 130, 8, "25,50"),
        (505, 130, 8, "1.274,50"),
        (40, 145, 8, "20/03/2025"),
        (130, 145, 8, "RECIBO TELEFONO"),
        (420, 145, 8, "74,50"),
        (505, 145, 8, "1.200,00"),
        (40, 160, 8, "25/03/2025"),
        (130, 160, 8, "RECIBO LUZ"),
        (420, 160, 8, "100,00"),
        (505, 160, 8, "1.100,00"),
    ];
    let l = leer(&[p]);
    assert_eq!(l[0].extracto.movimientos.len(), 3, "{:?}", l[0].extracto);
    assert!(
        l[0].extracto
            .avisos
            .iter()
            .any(|a| a.starts_with("1 fila con importe tiene una fecha que no se entiende")),
        "{:?}",
        l[0].extracto.avisos
    );
}
