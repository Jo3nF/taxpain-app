//! Extractos en tabla de muchos bancos: hojas de cálculo (xlsx y xls), CSV, tablas HTML
//! guardadas como `.xls` y XML de Excel 2003. Cada prueba imita la forma de la descarga de un
//! banco (los títulos, los datos de la cuenta de encima, el orden, los signos, los decimales)
//! con datos inventados.

#![allow(clippy::unwrap_used)]

use rust_xlsxwriter::Workbook;
use taxpain_app::extractos::{Clase, Cuadre, Extracto, Leido, leer};
use time::Date;
use time::macros::date;

const IBAN: &str = "ES9121000418450200051332";
const IBAN_GB: &str = "GB82WEST12345698765432";

/// Una celda de una hoja de prueba.
enum C<'a> {
    T(&'a str),
    N(f64),
    V,
}

/// Libro de Excel con una hoja por cada `(nombre, filas)`.
fn xlsx(hojas: &[(&str, Vec<Vec<C>>)]) -> Vec<u8> {
    let mut wb = Workbook::new();
    for (nombre, filas) in hojas {
        let ws = wb.add_worksheet();
        ws.set_name(*nombre).unwrap();
        for (r, fila) in filas.iter().enumerate() {
            let r = u32::try_from(r).unwrap();
            for (c, celda) in fila.iter().enumerate() {
                let c = u16::try_from(c).unwrap();
                match celda {
                    C::T(t) => {
                        ws.write_string(r, c, *t).unwrap();
                    }
                    C::N(n) => {
                        ws.write_number(r, c, *n).unwrap();
                    }
                    C::V => {}
                }
            }
        }
    }
    wb.save_to_buffer().unwrap()
}

/// Filas de texto (con `""` para las vacías).
fn texto(filas: &[&[&str]]) -> Vec<Vec<C<'static>>> {
    filas
        .iter()
        .map(|f| {
            f.iter()
                .map(|s| {
                    if s.is_empty() {
                        C::V
                    } else {
                        C::T(Box::leak((*s).to_owned().into_boxed_str()))
                    }
                })
                .collect()
        })
        .collect()
}

fn uno(nombre: &str, bytes: &[u8]) -> Leido {
    let mut l = leer(nombre, bytes).unwrap();
    assert_eq!(l.len(), 1, "{nombre}: {l:#?}");
    l.remove(0)
}

fn importes(e: &Extracto) -> Vec<i64> {
    e.movimientos.iter().map(|m| m.importe.cents()).collect()
}

fn fechas(e: &Extracto) -> Vec<Date> {
    e.movimientos.iter().map(|m| m.fecha).collect()
}

fn cents(m: Option<taxpain_core::money::Money>) -> Option<i64> {
    m.map(taxpain_core::money::Money::cents)
}

// ---------- Un .xls BIFF8 en UTF-16 que dice ser Windows-1252 ----------

fn registro(tipo: u16, cuerpo: &[u8]) -> Vec<u8> {
    let mut v = tipo.to_le_bytes().to_vec();
    v.extend(u16::try_from(cuerpo.len()).unwrap().to_le_bytes());
    v.extend(cuerpo);
    v
}

/// Texto de la tabla de textos de BIFF8, en UTF-16.
fn texto_biff(s: &str) -> Vec<u8> {
    let u: Vec<u16> = s.encode_utf16().collect();
    let mut v = u16::try_from(u.len()).unwrap().to_le_bytes().to_vec();
    v.push(1);
    for x in u {
        v.extend(x.to_le_bytes());
    }
    v
}

/// Libro BIFF8 con una hoja de textos, como los de JExcelAPI: la página de códigos que se
/// le diga (1252 en los que salen mal) y todos los textos en UTF-16.
fn biff8(filas: &[Vec<Option<&str>>], pagina: u16) -> Vec<u8> {
    let mut tabla: Vec<&str> = Vec::new();
    let mut total = 0u32;
    for f in filas {
        for c in f.iter().flatten() {
            total += 1;
            if !tabla.contains(c) {
                tabla.push(c);
            }
        }
    }
    let mut sst = total.to_le_bytes().to_vec();
    sst.extend(u32::try_from(tabla.len()).unwrap().to_le_bytes());
    for s in &tabla {
        sst.extend(texto_biff(s));
    }
    let bof = |tipo: u16| {
        let mut b = vec![0x00, 0x06];
        b.extend(tipo.to_le_bytes());
        b.extend([0xf2, 0x15, 0xcc, 0x07, 0, 0, 0, 0, 6, 0, 0, 0]);
        b
    };
    let globales = |pos: u32| {
        let mut hoja = pos.to_le_bytes().to_vec();
        hoja.extend([0, 0, 7, 1]);
        hoja.extend("report1".encode_utf16().flat_map(u16::to_le_bytes));
        [
            registro(0x0809, &bof(0x0005)),
            registro(0x0042, &pagina.to_le_bytes()),
            registro(0x0022, &[0, 0]),
            registro(0x0085, &hoja),
            registro(0x00FC, &sst),
            registro(0x000A, &[]),
        ]
        .concat()
    };
    let largo = u32::try_from(globales(0).len()).unwrap();
    let mut libro = globales(largo);
    let mut hoja = registro(0x0809, &bof(0x0010));
    let mut dim = 0u32.to_le_bytes().to_vec();
    dim.extend(u32::try_from(filas.len()).unwrap().to_le_bytes());
    dim.extend([0, 0, 7, 0, 0, 0]);
    hoja.extend(registro(0x0200, &dim));
    for (r, f) in filas.iter().enumerate() {
        let r = u16::try_from(r).unwrap();
        for (c, v) in f.iter().enumerate() {
            let c = u16::try_from(c).unwrap();
            let mut cuerpo = r.to_le_bytes().to_vec();
            cuerpo.extend(c.to_le_bytes());
            cuerpo.extend(0x15u16.to_le_bytes());
            match v {
                None => hoja.extend(registro(0x0201, &cuerpo)),
                Some(s) => {
                    let i = tabla.iter().position(|t| t == s).unwrap();
                    cuerpo.extend(u32::try_from(i).unwrap().to_le_bytes());
                    hoja.extend(registro(0x00FD, &cuerpo));
                }
            }
        }
    }
    hoja.extend(registro(0x000A, &[]));
    libro.extend(hoja);
    // Sin mini-stream: el flujo, al menos de 4096 bytes y en sectores enteros.
    let largo = libro.len().max(4096).div_ceil(512) * 512;
    libro.resize(largo, 0);
    cfb(&libro)
}

/// Contenedor OLE (CFB versión 3) con un solo flujo «Workbook».
fn cfb(flujo: &[u8]) -> Vec<u8> {
    let n = u32::try_from(flujo.len() / 512).unwrap();
    let (dir, fat) = (n, n + 1);
    let mut cab = vec![0u8; 512];
    cab[..8].copy_from_slice(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]);
    let mut pon = |pos: usize, b: &[u8]| cab[pos..pos + b.len()].copy_from_slice(b);
    pon(24, &0x003Eu16.to_le_bytes());
    pon(26, &3u16.to_le_bytes());
    pon(28, &0xFFFEu16.to_le_bytes());
    pon(30, &9u16.to_le_bytes());
    pon(32, &6u16.to_le_bytes());
    pon(44, &1u32.to_le_bytes());
    pon(48, &dir.to_le_bytes());
    pon(56, &4096u32.to_le_bytes());
    pon(60, &0xFFFF_FFFEu32.to_le_bytes());
    pon(68, &0xFFFF_FFFEu32.to_le_bytes());
    pon(76, &fat.to_le_bytes());
    for i in 1..109 {
        pon(76 + 4 * i, &0xFFFF_FFFFu32.to_le_bytes());
    }
    let entrada = |nombre: &str, tipo: u8, hijo: u32, inicio: u32, tam: u32| {
        let mut e = vec![0u8; 128];
        let mut nm: Vec<u8> = nombre.encode_utf16().flat_map(u16::to_le_bytes).collect();
        if !nombre.is_empty() {
            nm.extend([0, 0]);
        }
        e[..nm.len()].copy_from_slice(&nm);
        e[64..66].copy_from_slice(&u16::try_from(nm.len()).unwrap().to_le_bytes());
        e[66] = tipo;
        e[67] = 1;
        e[68..72].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        e[72..76].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
        e[76..80].copy_from_slice(&hijo.to_le_bytes());
        e[116..120].copy_from_slice(&inicio.to_le_bytes());
        e[120..124].copy_from_slice(&tam.to_le_bytes());
        e
    };
    let mut out = cab;
    out.extend(flujo);
    out.extend(entrada("Root Entry", 5, 1, 0xFFFF_FFFE, 0));
    out.extend(entrada(
        "Workbook",
        2,
        0xFFFF_FFFF,
        0,
        u32::try_from(flujo.len()).unwrap(),
    ));
    out.extend(entrada("", 0, 0xFFFF_FFFF, 0, 0));
    out.extend(entrada("", 0, 0xFFFF_FFFF, 0, 0));
    let mut tabla: Vec<u32> = (1..n).collect();
    tabla.extend([0xFFFF_FFFE, 0xFFFF_FFFE, 0xFFFF_FFFD]);
    tabla.resize(128, 0xFFFF_FFFF);
    out.extend(tabla.iter().flat_map(|x| x.to_le_bytes()));
    out
}

#[test]
fn xls_en_utf16_con_la_pagina_de_codigos_1252_como_los_de_ing() {
    let meta = |a: &'static str, b: &'static str| vec![Some(a), Some(b)];
    let mov = |f: &'static str, d: &'static str, c: &'static str, i: &'static str, s| {
        vec![
            Some(f),
            None,
            Some(d),
            if c.is_empty() { None } else { Some(c) },
            Some(i),
            Some(s),
        ]
    };
    let filas = vec![
        meta("Operación:", "Consulta de movimientos"),
        meta("Número de cuenta:", "2100 0418 45 0200051332"),
        meta("Titular:", "EMPRESA DEMO SL"),
        meta("Apoderado:", "PERSONA DEMO"),
        meta("Periodo de consulta:", "01/01/2016 - 31/03/2016"),
        vec![
            Some("FECHA"),
            None,
            Some("DESCRIPCIÓN"),
            Some("CONCEPTO"),
            Some("IMPORTE"),
            Some("SALDO"),
        ],
        mov(
            "29/03/2016",
            "Compra Tarjeta SUPERMERCADO DEMO  xxxx9999",
            "",
            "-38,40",
            "6.222,81",
        ),
        mov(
            "18/03/2016",
            "Transferencia recibida CLIENTE DEMO SA",
            "Factura «2016-001» – 987,65 €",
            "987,65",
            "6.261,21",
        ),
        mov(
            "05/01/2016",
            "Compra Tarjeta SOFTWARE DEMO  xxxx9999",
            "",
            "-16,25",
            "5.273,56",
        ),
        mov(
            "05/01/2016",
            "Compra Tarjeta HOSTING DEMO  xxxx9999",
            "",
            "-4,24",
            "5.289,81",
        ),
        mov(
            "04/01/2016",
            "Recibo ASESORIA DEMO SL",
            "",
            "-121,00",
            "5.294,05",
        ),
    ];
    let l = uno("Consulta_de_Movimientos.xls", &biff8(&filas, 1252));
    let e = &l.extracto;
    assert_eq!(l.cuadre, Cuadre::Cuadra);
    assert_eq!(e.iban.as_deref(), Some(IBAN));
    assert_eq!(e.banco, "CaixaBank");
    assert_eq!(e.titular, "EMPRESA DEMO SL");
    assert_eq!(e.formato, "Hoja de cálculo");
    assert_eq!(importes(e), [-12_100, -424, -1_625, 98_765, -3_840]);
    assert_eq!(
        e.movimientos[1].concepto,
        "Compra Tarjeta HOSTING DEMO xxxx9999"
    );
    // Los caracteres de fuera de Latin-1 también salen bien: no basta con quitar los ceros.
    assert_eq!(
        e.movimientos[3].concepto,
        "Transferencia recibida CLIENTE DEMO SA · Factura «2016-001» – 987,65 €"
    );
    assert_eq!(
        (e.desde, e.hasta),
        (Some(date!(2016 - 01 - 01)), Some(date!(2016 - 03 - 31)))
    );
    assert_eq!(cents(e.saldo_inicial), Some(541_505));
    assert_eq!(cents(e.saldo_final), Some(622_281));
    // El mismo libro con la página de códigos bien puesta sale igual.
    assert_eq!(uno("bien.xls", &biff8(&filas, 1200)).extracto, *e);
}

// ---------- Tablas HTML guardadas como .xls ----------

#[test]
fn tabla_html_en_una_linea_con_saldo_inicial_y_resumen_debajo() {
    // Como la de Bankinter: en Latin-1, sin saltos de línea, etiquetas en mayúsculas y
    // minúsculas, la cuenta sin entidad («oficina/dd.ccccccc») y el saldo inicial en la
    // columna del importe.
    let html = "<TABLE border='1'><TR><TD colspan='5'><b>N\u{fa}mero de cuenta:</b> 1234/10.0000123</TD></TR>\
        <TR><TD colspan='5'><b>Empresa:</b> EMPRESA DEMO SL</TD></TR><TR><TD colspan='5'></TD></TR>\
        <TR><TH> FECHA CONTABLE </TH><TH> FECHA VALOR </TH><TH> DESCRIPCION </TH><TH> IMPORTE </TH><TH> SALDO </TH></TR>\
        <tr><td></td><td></td><td>SALDO INICIAL</td><td>1000,00</td><td></td></tr>\
        <tr><td>07/03/2016</td><td>06/03/2016</td><td>COMISION DE ADMINISTRACION</td><td>-5,00</td><td>995,00</td></tr>\
        <tr><td>21/03/2016</td><td>21/03/2016</td><td>COMIS  /PROVEEDOR DEMO</td><td>-5,00</td><td>990,00</td></tr>\
        <tr><td>21/03/2016</td><td>21/03/2016</td><td>TRANSF /CLIENTE DEMO SA</td><td>1500,00</td><td>2490,00</td></tr>\
        <tr><td colspan=\"5\"></td></tr>\
        <tr><td colspan=\"2\"></td><td colspan=\"2\"><b>Saldo total</b></td><td>2490,00</td></tr>\
        <tr><td colspan=\"2\"></td><td colspan=\"2\"><b>Abonos pendientes</b></td><td>0,00</td></tr>\
        <tr><td colspan=\"2\"></td><td colspan=\"2\"><b>Total retenciones</b></td><td>0,00</td></tr>\
        <tr><td colspan=\"2\"></td><td colspan=\"2\"><b>Saldo disponible</b></td><td>2490,00</td></tr></TABLE>";
    let bytes: Vec<u8> = html
        .chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap())
        .collect();
    let l = uno("movimientos.xls", &bytes);
    let e = &l.extracto;
    assert_eq!(l.cuadre, Cuadre::Cuadra);
    assert_eq!(e.formato, "Tabla HTML");
    assert_eq!(e.banco, "Bankinter");
    assert_eq!(e.titular, "EMPRESA DEMO SL");
    // El IBAN, con la entidad del banco y los dígitos de control calculados.
    assert_eq!(e.iban.as_deref(), Some("ES8201281234000100000123"));
    assert_eq!(importes(e), [-500, -500, 150_000]);
    assert_eq!(e.movimientos[0].fecha_valor, Some(date!(2016 - 03 - 06)));
    assert_eq!(e.movimientos[1].concepto, "COMIS /PROVEEDOR DEMO");
    assert_eq!(cents(e.saldo_inicial), Some(100_000));
    assert_eq!(cents(e.saldo_final), Some(249_000));
}

#[test]
fn pagina_html_con_los_datos_en_otra_tabla_y_celdas_combinadas() {
    let html = r#"<!DOCTYPE html><html><head><meta charset="utf-8"><title>Movimientos</title>
<style>td { font-size: 9pt }</style></head><body>
<table class="cabecera">
  <tr><td>Titular</td><td>EMPRESA DEMO SL</td></tr>
  <tr><td>IBAN</td><td>ES91&nbsp;2100&nbsp;0418&nbsp;4502&nbsp;0005&nbsp;1332</td></tr>
  <tr><td>Moneda</td><td>EUR</td></tr>
</table>
<table class="movimientos">
  <thead><tr><th>Fecha</th><th>Fecha valor</th><th colspan="2">Concepto</th><th>Importe</th><th>Saldo</th></tr></thead>
  <tbody>
  <tr><td>15/01/2025</td><td>15/01/2025</td><td>TRANSFERENCIA</td><td>CLIENTE&nbsp;DEMO&nbsp;SA<br/>FACTURA 7</td><td>1.210,00</td><td>3.210,00</td></tr>
  <tr><td>10/01/2025</td><td>09/01/2025</td><td colspan="2">RECIBO ASESOR&Iacute;A DEMO</td><td>-121,00</td><td>2.000,00</td></tr>
  <tr><td rowspan="2">05/01/2025</td><td>05/01/2025</td><td colspan="2">COMPRA TARJETA</td><td>-79,00</td><td>2.121,00</td></tr>
  <tr><td>05/01/2025</td><td colspan="2">COMISI&Oacute;N</td><td>-1,50</td><td>2.200,00</td></tr>
  </tbody>
</table></body></html>"#;
    let l = uno("export.xls", html.as_bytes());
    let e = &l.extracto;
    assert_eq!(l.cuadre, Cuadre::Cuadra, "{e:#?}");
    assert_eq!(e.iban.as_deref(), Some(IBAN));
    assert_eq!(e.titular, "EMPRESA DEMO SL");
    // Del más nuevo al más antiguo: se le da la vuelta entera.
    assert_eq!(importes(e), [-150, -7_900, -12_100, 121_000]);
    assert_eq!(e.movimientos[0].concepto, "COMISIÓN");
    assert_eq!(
        e.movimientos[3].concepto,
        "TRANSFERENCIA · CLIENTE DEMO SA FACTURA 7"
    );
    assert_eq!(cents(e.saldo_inicial), Some(220_150));
}

#[test]
fn xml_de_excel_2003() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<?mso-application progid="Excel.Sheet"?>
<Workbook xmlns="urn:schemas-microsoft-com:office:spreadsheet"
 xmlns:o="urn:schemas-microsoft-com:office:office"
 xmlns:ss="urn:schemas-microsoft-com:office:spreadsheet">
 <Worksheet ss:Name="Movimientos">
  <Table>
   <Row><Cell ss:MergeAcross="4"><Data ss:Type="String">Extracto de la cuenta GB82 WEST 1234 5698 7654 32</Data></Cell></Row>
   <Row><Cell><Data ss:Type="String">Account holder:</Data></Cell><Cell><Data ss:Type="String">EMPRESA DEMO LTD</Data></Cell></Row>
   <Row ss:Index="4">
    <Cell><Data ss:Type="String">Date</Data></Cell><Cell><Data ss:Type="String">Description</Data></Cell>
    <Cell><Data ss:Type="String">Amount</Data></Cell><Cell><Data ss:Type="String">Currency</Data></Cell>
    <Cell><Data ss:Type="String">Balance</Data></Cell></Row>
   <Row><Cell><Data ss:Type="DateTime">2025-03-01T00:00:00.000</Data></Cell><Cell><Data ss:Type="String">Opening deposit</Data></Cell>
    <Cell><Data ss:Type="Number">2500</Data></Cell><Cell><Data ss:Type="String">GBP</Data></Cell><Cell><Data ss:Type="Number">2500</Data></Cell></Row>
   <Row><Cell><Data ss:Type="DateTime">2025-03-04T00:00:00.000</Data></Cell><Cell><Data ss:Type="String">Office rent &amp; services</Data></Cell>
    <Cell><Data ss:Type="Number">-1200.5</Data></Cell><Cell><Data ss:Type="String">GBP</Data></Cell><Cell><Data ss:Type="Number">1299.5</Data></Cell></Row>
  </Table>
 </Worksheet>
</Workbook>"#;
    let l = uno("statement.xls", xml.as_bytes());
    let e = &l.extracto;
    assert_eq!(l.cuadre, Cuadre::Cuadra);
    assert_eq!(e.iban.as_deref(), Some(IBAN_GB));
    assert_eq!(e.cuenta, IBAN_GB);
    assert_eq!(e.divisa, "GBP");
    assert_eq!(e.titular, "EMPRESA DEMO LTD");
    assert_eq!(importes(e), [250_000, -120_050]);
    assert_eq!(cents(e.saldo_inicial), Some(0));
}

// ---------- Hojas de cálculo de bancos españoles ----------

#[test]
fn hoja_con_datos_de_cuenta_y_fechas_como_numero_de_serie() {
    // Como la de Santander u Openbank: los datos de la cuenta arriba, las fechas como
    // números de serie de Excel y los importes como números.
    let mut filas = texto(&[
        &["Movimientos de la cuenta"],
        &["Cuenta", "ES91 2100 0418 4502 0005 1332"],
        &["Titular", "EMPRESA DEMO SL"],
        &["Saldo anterior", "1.000,00 €"],
        &[],
        &[
            "Fecha Operación",
            "Fecha Valor",
            "Concepto",
            "Importe",
            "Divisa",
            "Saldo",
            "Nº Documento",
        ],
    ]);
    for (f, c, i, s, d) in [
        (45_660.0, "RECIBO LUZ DEMO", -60.25, 939.75, "0001"),
        (
            45_661.0,
            "TRANSFERENCIA DE CLIENTE DEMO",
            2_000.0,
            2_939.75,
            "0002",
        ),
        (
            45_663.0,
            "PAGO TARJETA TIENDA DEMO",
            -39.9,
            2_899.85,
            "0003",
        ),
    ] {
        filas.push(vec![
            C::N(f),
            C::N(f),
            C::T(c),
            C::N(i),
            C::T("EUR"),
            C::N(s),
            C::T(d),
        ]);
    }
    filas.push(texto(&[&["", "", "Saldo final", "", "", "2.899,85 €"]]).remove(0));
    let l = uno("movimientos.xlsx", &xlsx(&[("Movimientos", filas)]));
    let e = &l.extracto;
    assert_eq!(l.cuadre, Cuadre::Cuadra);
    assert_eq!(e.cuenta, "210004180200051332");
    assert_eq!(
        fechas(e),
        [
            date!(2025 - 01 - 03),
            date!(2025 - 01 - 04),
            date!(2025 - 01 - 06)
        ]
    );
    assert_eq!(importes(e), [-6_025, 200_000, -3_990]);
    assert_eq!(e.movimientos[2].referencia, "0003");
    assert_eq!(cents(e.saldo_inicial), Some(100_000));
    assert_eq!(cents(e.saldo_final), Some(289_985));
}

#[test]
fn hoja_con_concepto_en_dos_columnas_y_fecha_valor_sin_año() {
    // Como la de Banca March: el IBAN con el titular detrás, «Saldo actual» de la hora de la
    // descarga, la fecha valor sin año y el concepto del ordenante aparte.
    let mut filas = texto(&[
        &["Banco Demo S.A. - https://banca.demo.example"],
        &["Cuentas a la vista - Extracto de movimientos - mié, 15 ene 2025, 10:20 CET"],
        &[],
        &["Fecha de 01/12/2024 hasta 15/01/2025"],
        &[],
        &["ES91 2100 0418 4502 0005 1332 - EMPRESA DEMO SL."],
        &[
            "Saldo actual: 1.196,35\u{a0}€ Retenciones: 3,10\u{a0}€ Saldo disponible: 1.193,25\u{a0}€",
        ],
        &[
            "F. operación",
            "F. valor",
            "Oficina",
            "Concepto",
            "Concepto ordenante",
            "Importe",
            "Saldo",
        ],
    ]);
    for (f, v, c, o, i, s) in [
        (
            "30/12/2024",
            "28/12",
            "COMISION MANTENIMIENTO",
            "",
            -7.5,
            992.5,
        ),
        (
            "02/01/2025",
            "31/12",
            "AB.TR.SEPA CLIENTE DEMO",
            "FACTURA 12",
            210.0,
            1_202.5,
        ),
        (
            "09/01/2025",
            "07/01",
            "4B COMPRA PAPELERIA DEMO",
            "",
            -6.15,
            1_196.35,
        ),
    ] {
        filas.push(vec![
            C::T(f),
            C::T(v),
            C::T("0001"),
            C::T(c),
            if o.is_empty() { C::V } else { C::T(o) },
            C::N(i),
            C::N(s),
        ]);
    }
    let l = uno("Extracto.xlsx", &xlsx(&[("Sheet1", filas)]));
    let e = &l.extracto;
    assert_eq!(l.cuadre, Cuadre::Cuadra);
    assert_eq!(e.iban.as_deref(), Some(IBAN));
    assert_eq!(e.titular, "EMPRESA DEMO SL");
    assert_eq!(importes(e), [-750, 21_000, -615]);
    assert_eq!(
        e.movimientos
            .iter()
            .map(|m| m.fecha_valor)
            .collect::<Vec<_>>(),
        [
            Some(date!(2024 - 12 - 28)),
            Some(date!(2024 - 12 - 31)),
            Some(date!(2025 - 01 - 07))
        ]
    );
    assert_eq!(
        e.movimientos[1].concepto,
        "AB.TR.SEPA CLIENTE DEMO · FACTURA 12"
    );
    assert_eq!(
        (e.desde, e.hasta),
        (Some(date!(2024 - 12 - 01)), Some(date!(2025 - 01 - 15)))
    );
    assert!(e.avisos.is_empty(), "{:?}", e.avisos);
}

#[test]
fn hoja_con_etiquetas_a_la_izquierda_y_la_tabla_mas_abajo() {
    // Como la de BBVA: etiqueta en C y valor en F, los títulos en la fila 16, del más nuevo al
    // más antiguo, beneficiario enmascarado, observaciones «-» y una referencia por fila.
    let mut filas = texto(&[
        &[],
        &[],
        &[],
        &[],
        &["", "", "Movimientos"],
        &[],
        &["", "", "Titular", "", "", "EMPRESA DEMO SL"],
        &["", "", "Cuenta", "", "", IBAN],
        &["", "", "Divisa", "", "", "EUR"],
        &[
            "",
            "",
            "Banco",
            "",
            "",
            "BANCO BILBAO VIZCAYA ARGENTARIA S.A. ",
        ],
        &["", "", "Fecha", "", "", "05/03/2026 Hora  10:15 "],
        &["", "", "Importe", "", "", "Todos "],
        &["", "", "Periodo", "", "", "01/09/2024-05/03/2026"],
        &["", "", "Filtros", "", "", "Todos "],
        &[],
        &[
            "",
            "",
            "F. CONTABLE",
            "F. VALOR",
            "CÓDIGO",
            "CONCEPTO",
            "BENEFICIARIO/ORDENANTE",
            "OBSERVACIONES",
            "IMPORTE",
            "SALDO",
            "DIVISA",
            "OFICINA",
            "REMESA",
        ],
    ]);
    for (f, c, b, o, i, s, r) in [
        (
            "10/02/2026",
            "PAGO DE NOMINAS",
            "*********************",
            "NOMINA ENERO",
            -1_500.0,
            1_278.5,
            "R3",
        ),
        ("10/02/2026", "COMISIONES", "", "-", -12.3, 2_778.5, "R2"),
        (
            "02/02/2026",
            "TRANSFERENCIAS",
            "CLIENTE DEMO SA",
            "FRA 2026-1",
            2_790.8,
            2_790.8,
            "R1",
        ),
    ] {
        filas.push(vec![
            C::V,
            C::V,
            C::T(f),
            C::T(f),
            C::T("00007"),
            C::T(c),
            if b.is_empty() { C::V } else { C::T(b) },
            C::T(o),
            C::N(i),
            C::N(s),
            C::T("EUR"),
            C::T("0001"),
            C::T(r),
        ]);
    }
    let l = uno("Historico.xlsx", &xlsx(&[("Historico", filas)]));
    let e = &l.extracto;
    assert_eq!(l.cuadre, Cuadre::Cuadra);
    assert_eq!(e.banco, "BBVA");
    assert_eq!(e.iban.as_deref(), Some(IBAN));
    assert_eq!(importes(e), [279_080, -1_230, -150_000]);
    assert_eq!(
        e.movimientos
            .iter()
            .map(|m| m.concepto.as_str())
            .collect::<Vec<_>>(),
        [
            "TRANSFERENCIAS · CLIENTE DEMO SA · FRA 2026-1",
            "COMISIONES",
            "PAGO DE NOMINAS · NOMINA ENERO"
        ]
    );
    assert_eq!(e.movimientos[0].referencia, "R1");
    assert_eq!(cents(e.saldo_inicial), Some(0));
    assert_eq!(
        (e.desde, e.hasta),
        (Some(date!(2024 - 09 - 01)), Some(date!(2026 - 03 - 05)))
    );
}

#[test]
fn listado_de_tarjeta_de_credito_sin_saldos() {
    let filas = texto(&[
        &[],
        &[],
        &[],
        &[],
        &["", "", "Listado de movimientos de tarjeta"],
        &[],
        &["", "", "Nº de tarjeta", "", "5400000000001234"],
        &["", "", "Tipo de tarjeta", "", "TARJETA NEGOCIOS CREDITO"],
        &["", "", "Nº de contrato", "", "01820000000000000000001234"],
        &["", "", "Titular de la tarjeta", "", "PERSONA DEMO"],
        &["", "", "Fecha", "", "30/06/2026"],
        &["", "", "Periodo", "", "30/06/2024 - 30/06/2026"],
        &[],
        &[],
        &[],
        &[
            "",
            "",
            "FECHA DE OPERACIÓN",
            "CONCEPTO",
            "TIPO DE MOVIMIENTO",
            "IMPORTE",
            "DIVISA",
        ],
        &[
            "",
            "",
            "2026-05-05",
            "RECIBO MES ANTERIOR",
            "Sin categoría",
            "99,45",
            "EUR",
        ],
        &[
            "",
            "",
            "2026-04-18",
            "RESTAURANTE DEMO",
            "Compra",
            "-87,25",
            "EUR",
        ],
        &[
            "",
            "",
            "2026-04-11",
            "HOSTING DEMO.COM* X1Y2",
            "Compra",
            "-12,20",
            "EUR",
        ],
        &[
            "",
            "",
            "2026-04-05",
            "RECIBO MES ANTERIOR",
            "Sin categoría",
            "18,50",
            "EUR",
        ],
        &[
            "",
            "",
            "2026-03-12",
            "LIBRERIA DEMO",
            "Compra",
            "-18,50",
            "EUR",
        ],
    ]);
    let l = uno("tarjeta.xls", &xlsx(&[("listado", filas)]));
    let e = &l.extracto;
    assert_eq!(l.cuadre, Cuadre::SinSaldos);
    assert_eq!(e.clase, Clase::TarjetaCredito);
    assert_eq!(e.cuenta, "tarjeta:0182:1234");
    assert_eq!(e.banco, "BBVA");
    assert_eq!(e.titular, "PERSONA DEMO");
    assert_eq!(importes(e), [-1_850, 1_850, -1_220, -8_725, 9_945]);
    assert_eq!(e.movimientos[0].fecha, date!(2026 - 03 - 12));
}

#[test]
fn ing_nuevo_con_solo_fecha_valor_y_categorias() {
    let filas = texto(&[
        &["Número de cuenta:", "ES91 2100 0418 4502 0005 1332"],
        &["Titular:", "EMPRESA DEMO SL"],
        &[],
        &[
            "F. VALOR",
            "CATEGORÍA",
            "SUBCATEGORÍA",
            "DESCRIPCIÓN",
            "COMENTARIO",
            "IMAGEN",
            "IMPORTE (€)",
            "SALDO (€)",
        ],
        &[
            "05/02/2025",
            "Compras",
            "Otros",
            "Pago en TIENDA DEMO",
            "",
            "",
            "-20,00",
            "980,00",
        ],
        &[
            "01/02/2025",
            "Nómina",
            "Ingresos",
            "Transferencia recibida",
            "Enero",
            "",
            "1.000,00",
            "1.000,00",
        ],
    ]);
    let l = uno("movements.xls", &xlsx(&[("Movimientos", filas)]));
    let e = &l.extracto;
    assert_eq!(l.cuadre, Cuadre::Cuadra);
    assert_eq!(importes(e), [100_000, -2_000]);
    assert_eq!(e.movimientos[0].concepto, "Transferencia recibida · Enero");
    assert_eq!(e.movimientos[0].fecha_valor, None);
    assert_eq!(e.divisa, "EUR");
}

#[test]
fn libro_con_dos_cuentas_y_una_hoja_partida_en_paginas() {
    let cuenta = |iban: &str, divisa: &str| {
        texto(&[
            &["Cuenta:", iban],
            &["Divisa:", divisa],
            &["Fecha", "Concepto", "Importe", "Saldo"],
        ])
    };
    let mut a = cuenta(IBAN, "EUR");
    a.extend(texto(&[
        &["02/01/2025", "UNO", "-1,00", "99,00"],
        &["03/01/2025", "DOS", "-2,00", "97,00"],
        &[],
        // La misma cuenta, en la página siguiente con sus títulos.
        &["Fecha", "Concepto", "Importe", "Saldo"],
        &["04/01/2025", "TRES", "-3,00", "94,00"],
    ]));
    let mut b = cuenta(IBAN_GB, "GBP");
    b.extend(texto(&[&["02/01/2025", "ONE", "10.00", "110.00"]]));
    let l = leer("cuentas.xlsx", &xlsx(&[("Cuenta 1", a), ("Cuenta 2", b)])).unwrap();
    assert_eq!(l.len(), 2);
    let es = l.iter().find(|l| l.extracto.divisa == "EUR").unwrap();
    let gb = l.iter().find(|l| l.extracto.divisa == "GBP").unwrap();
    assert_eq!(es.extracto.cuenta, "210004180200051332");
    assert_eq!(importes(&es.extracto), [-100, -200, -300]);
    assert_eq!(es.cuadre, Cuadre::Cuadra);
    assert_eq!(gb.extracto.cuenta, IBAN_GB);
    assert_eq!(importes(&gb.extracto), [1_000]);

    // Dos cuentas en la misma hoja, cada una con sus datos encima.
    let mut h = cuenta(IBAN, "EUR");
    h.extend(texto(&[&["02/01/2025", "UNO", "-1,00", "99,00"], &[]]));
    h.extend(cuenta(IBAN_GB, "GBP"));
    h.extend(texto(&[&["02/01/2025", "ONE", "10.00", "110.00"]]));
    let l = leer("una-hoja.xlsx", &xlsx(&[("Hoja1", h)])).unwrap();
    assert_eq!(l.len(), 2);
    assert!(l.iter().all(|l| l.extracto.movimientos.len() == 1));
}

// ---------- CSV de bancos y entidades de pago extranjeros ----------

#[test]
fn csv_de_revolut_con_comisiones_estados_y_dos_divisas() {
    let csv = "Type,Product,Started Date,Completed Date,Description,Amount,Fee,Currency,State,Balance\n\
        TOPUP,Current,2025-01-02 09:00:00,2025-01-02 09:00:05,Top-up by *1234,500.00,0.00,EUR,COMPLETED,500.00\n\
        CARD_PAYMENT,Current,2025-01-03 12:00:00,2025-01-04 08:00:00,Cafe Demo,-12.50,0.00,EUR,COMPLETED,487.50\n\
        TRANSFER,Current,2025-01-05 10:00:00,2025-01-05 10:00:01,To DEMO SUPPLIER,-100.00,0.50,EUR,COMPLETED,387.00\n\
        CARD_PAYMENT,Current,2025-01-06 10:00:00,,Pending shop,-9.99,0.00,EUR,PENDING,\n\
        EXCHANGE,Current,2025-01-05 11:00:00,2025-01-05 11:00:00,Exchanged to USD,50.00,0.00,USD,COMPLETED,50.00\n\
        CARD_PAYMENT,Current,2025-01-07 11:00:00,2025-01-07 11:00:00,Hosting Demo Inc,-20.00,0.00,USD,COMPLETED,30.00\n";
    let l = leer("account-statement.csv", csv.as_bytes()).unwrap();
    assert_eq!(l.len(), 2);
    let eur = l.iter().find(|l| l.extracto.divisa == "EUR").unwrap();
    let usd = l.iter().find(|l| l.extracto.divisa == "USD").unwrap();
    assert_eq!(eur.extracto.banco, "Revolut");
    assert_eq!(eur.extracto.formato, "CSV");
    assert_eq!(eur.cuadre, Cuadre::Cuadra);
    assert_eq!(usd.cuadre, Cuadre::Cuadra);
    // La comisión va restada y el pendiente no cuenta.
    assert_eq!(importes(&eur.extracto), [50_000, -1_250, -10_050]);
    // La fecha es la del cargo (la que sigue el saldo).
    assert_eq!(eur.extracto.movimientos[1].fecha, date!(2025 - 01 - 04));
    assert_eq!(importes(&usd.extracto), [5_000, -2_000]);
    assert!(eur.extracto.avisos.iter().any(|a| a.contains("pendientes")));
}

#[test]
fn csv_de_revolut_business_con_comision_en_yenes_y_el_iban_en_otra_columna() {
    // Del más nuevo al más antiguo. El cambio a yenes cobra la comisión en yenes (473 JPY): el
    // importe ya va sin ella y no se puede restar como si fueran euros.
    let csv = format!(
        "Date started (UTC),Date completed (UTC),ID,Type,State,Description,Reference,Payer,\
         Orig currency,Orig amount,Payment currency,Amount,Total amount,Exchange rate,Fee,\
         Fee currency,Balance,Account,International account number,Beneficiary IBAN\n\
         2025-03-05,2025-03-05,c,CARD_PAYMENT,COMPLETED,Tienda Demo,,Ana Demo,USD,100.00,EUR,\
         -90.00,-92.50,1.11,-2.50,EUR,377.50,EUR Main,{IBAN},\n\
         2025-03-04,2025-03-04,b,EXCHANGE,COMPLETED,Main · EUR → Japon · JPY,,Ana Demo,JPY,\
         78711,EUR,-430.00,-430.00,183.0,-473,JPY,470.00,EUR Main,{IBAN},\n\
         2025-03-01,2025-03-01,a,TOPUP,COMPLETED,Dinero añadido por CLIENTE DEMO,,,EUR,900.00,\
         EUR,900.00,900.00,,0.00,EUR,900.00,EUR Main,{IBAN},{IBAN_GB}\n"
    );
    let l = uno("transaction-statement.csv", csv.as_bytes());
    assert_eq!(l.cuadre, Cuadre::Cuadra, "{l:#?}");
    assert_eq!(importes(&l.extracto), [90_000, -43_000, -9_250]);
    assert_eq!(l.extracto.iban.as_deref(), Some(IBAN));
}

#[test]
fn csv_de_wise_del_mas_nuevo_al_mas_antiguo() {
    let csv = "\"TransferWise ID\",Date,Amount,Currency,Description,\"Payment Reference\",\"Running Balance\",\"Exchange From\",\"Exchange To\",\"Exchange Rate\",\"Payer Name\",\"Payee Name\",\"Payee Account Number\",Merchant,\"Total fees\"\n\
        TRANSFER-3,31-01-2025,-250.00,EUR,\"Sent money to Demo Supplier\",\"INV 77\",749.10,,,,,\"Demo Supplier\",,,0.90\n\
        CARD-2,15-01-2025,-0.90,EUR,\"Card transaction of 0.90 EUR\",,1000.00,,,,,,,\"Demo Coffee\",0.00\n\
        TRANSFER-1,02-01-2025,1000.90,EUR,\"Received money from Demo Client\",,1000.90,,,,\"Demo Client\",,,,0.00\n";
    let l = uno("statement.csv", csv.as_bytes());
    let e = &l.extracto;
    assert_eq!(e.banco, "Wise");
    assert_eq!(l.cuadre, Cuadre::Cuadra, "{e:#?}");
    assert_eq!(importes(e), [100_090, -90, -25_090]);
    assert_eq!(e.movimientos[2].referencia, "TRANSFER-3");
    assert!(e.movimientos[2].concepto.contains("INV 77"));
}

#[test]
fn csv_de_n26_sin_saldo() {
    let csv = "\"Booking Date\",\"Value Date\",\"Partner Name\",\"Partner Iban\",Type,\"Payment Reference\",\"Account Name\",\"Amount (EUR)\",\"Original Amount\",\"Original Currency\",\"Exchange Rate\"\n\
        2025-02-01,2025-02-01,\"Demo Landlord\",GB82WEST12345698765432,Debit Transfer,\"Rent February\",\"Main Account\",-800.0,,,\n\
        2025-02-03,2025-02-03,\"Demo Store\",,Presentment,,\"Main Account\",-25.4,-27.1,USD,1.067\n\
        2025-02-05,2025-02-05,\"Demo Client\",ES9121000418450200051332,Credit Transfer,\"Invoice 9\",\"Main Account\",1500.0,,,\n";
    let l = uno("n26-csv-transactions.csv", csv.as_bytes());
    let e = &l.extracto;
    assert_eq!(e.banco, "N26");
    assert_eq!(l.cuadre, Cuadre::SinSaldos);
    assert_eq!(importes(e), [-80_000, -2_540, 150_000]);
    assert_eq!(e.divisa, "EUR");
    assert_eq!(e.movimientos[0].concepto, "Demo Landlord · Rent February");
    // El IBAN de la otra parte no es el de la cuenta.
    assert!(e.iban.is_none());
    assert_eq!(e.cuenta, "hoja:N26");
}

#[test]
fn csv_de_qonto_con_rechazados_e_iva_aparte() {
    let csv = "Status;Settlement date (UTC);Operation date (UTC);Total amount (incl. VAT);Currency;Counterparty name;Payment method;Reference;Category;VAT amount\n\
        settled;03-03-2025 10:00:00;01-03-2025 09:00:00;-121,00;EUR;Demo Software;card;SUB-1;software;-21,00\n\
        declined;04-03-2025 10:00:00;04-03-2025 09:00:00;-50,00;EUR;Demo Shop;card;;other;0\n\
        settled;05-03-2025 10:00:00;05-03-2025 09:00:00;2420,00;EUR;Demo Client;transfer;F-9;sales;420,00\n";
    let l = uno("qonto.csv", csv.as_bytes());
    let e = &l.extracto;
    assert_eq!(e.banco, "Qonto");
    assert_eq!(importes(e), [-12_100, 242_000]);
    // Sin saldo, la fecha es la de la operación.
    assert_eq!(e.movimientos[0].fecha, date!(2025 - 03 - 01));
    assert_eq!(e.movimientos[0].referencia, "SUB-1");
}

#[test]
fn csv_de_paypal_con_neto_fechas_americanas_y_divisas() {
    let csv = "\"Date\",\"Time\",\"TimeZone\",\"Name\",\"Type\",\"Status\",\"Currency\",\"Gross\",\"Fee\",\"Net\",\"From Email Address\",\"To Email Address\",\"Transaction ID\",\"Balance\"\n\
        \"01/02/2025\",\"10:00:00\",\"PST\",\"Demo Client\",\"Express Checkout Payment\",\"Completed\",\"EUR\",\"100.00\",\"-3.20\",\"96.80\",\"client@example.com\",\"shop@example.com\",\"TX1\",\"96.80\"\n\
        \"01/15/2025\",\"11:00:00\",\"PST\",\"\",\"General Currency Conversion\",\"Completed\",\"EUR\",\"-46.80\",\"0.00\",\"-46.80\",\"\",\"\",\"TX2\",\"50.00\"\n\
        \"01/15/2025\",\"11:00:00\",\"PST\",\"\",\"General Currency Conversion\",\"Completed\",\"USD\",\"50.00\",\"0.00\",\"50.00\",\"\",\"\",\"TX3\",\"50.00\"\n";
    let l = leer("Download.CSV", csv.as_bytes()).unwrap();
    assert_eq!(l.len(), 2);
    let eur = l.iter().find(|l| l.extracto.divisa == "EUR").unwrap();
    assert_eq!(eur.extracto.banco, "PayPal");
    assert_eq!(eur.extracto.clase, Clase::Monedero);
    assert_eq!(eur.cuadre, Cuadre::Cuadra);
    assert_eq!(importes(&eur.extracto), [9_680, -4_680]);
    assert_eq!(
        fechas(&eur.extracto),
        [date!(2025 - 01 - 02), date!(2025 - 01 - 15)]
    );
}

#[test]
fn csv_ingles_con_pagado_y_cobrado_y_libras() {
    let csv = "Date,Description,Paid out,Paid in,Balance\n\
        14/03/2025,DEMO UTILITIES DD,£45.10,,\"£1,954.90\"\n\
        12/03/2025,FASTER PAYMENT DEMO CLIENT,,\"£1,000.00\",\"£2,000.00\"\n\
        10/03/2025,CARD PAYMENT DEMO CAFE,£5.00,,\"£1,000.00\"\n";
    let l = uno("statement.csv", csv.as_bytes());
    let e = &l.extracto;
    assert_eq!(l.cuadre, Cuadre::Cuadra);
    assert_eq!(e.divisa, "GBP");
    assert_eq!(importes(e), [-500, 100_000, -4_510]);
    assert_eq!(cents(e.saldo_inicial), Some(100_500));
}

#[test]
fn csv_aleman_con_datos_encima_y_coma_decimal() {
    let csv = "\"Kontonummer:\";\"GB82 WEST 1234 5698 7654 32\";\n\
        \"Kontoinhaber:\";\"EMPRESA DEMO GMBH\";\n\
        \"Zeitraum:\";\"01.01.2025 - 31.01.2025\";\n\
        \"Kontostand vom 31.01.2025:\";\"1.735,50 EUR\";\n\
        \n\
        \"Buchungstag\";\"Valuta\";\"Verwendungszweck\";\"Betrag\";\"Saldo\";\"Währung\"\n\
        \"30.01.2025\";\"30.01.2025\";\"Miete Januar\";\"-1.000,00\";\"1.735,50\";\"EUR\"\n\
        \"15.01.2025\";\"16.01.2025\";\"Rechnung 12 Demo Kunde\";\"2.500,00\";\"2.735,50\";\"EUR\"\n\
        \"02.01.2025\";\"02.01.2025\";\"Kontoführung\";\"-4,50\";\"235,50\";\"EUR\"\n";
    let l = uno("umsaetze.csv", csv.as_bytes());
    let e = &l.extracto;
    assert_eq!(l.cuadre, Cuadre::Cuadra);
    assert_eq!(e.iban.as_deref(), Some(IBAN_GB));
    assert_eq!(e.titular, "EMPRESA DEMO GMBH");
    assert_eq!(importes(e), [-450, 250_000, -100_000]);
    assert_eq!(e.movimientos[1].fecha_valor, Some(date!(2025 - 01 - 16)));
    assert_eq!(
        (e.desde, e.hasta),
        (Some(date!(2025 - 01 - 01)), Some(date!(2025 - 01 - 31)))
    );
    assert!(e.avisos.is_empty(), "{:?}", e.avisos);
}

#[test]
fn csv_de_francia_con_debito_credito_y_saldos_aparte() {
    let csv = "Compte;FR76 3000 6000 0112 3456 7890 189\n\
        Solde initial;1 000,00\n\
        Date opération;Date valeur;Libellé;Débit;Crédit\n\
        03/02/2025;03/02/2025;PRLV DEMO ENERGIE;64,30;\n\
        05/02/2025;06/02/2025;VIR DEMO CLIENT;;1 250,00\n\
        Solde final;2 185,70\n";
    let l = uno("releve.csv", csv.as_bytes());
    let e = &l.extracto;
    assert_eq!(l.cuadre, Cuadre::Cuadra, "{e:#?}");
    assert_eq!(e.iban.as_deref(), Some("FR7630006000011234567890189"));
    assert_eq!(importes(e), [-6_430, 125_000]);
    assert_eq!(cents(e.saldo_inicial), Some(100_000));
    assert_eq!(cents(e.saldo_final), Some(218_570));
}

#[test]
fn csv_americano_con_mes_delante_y_saldo_inicial_en_la_tabla() {
    let csv = "Description,,Summary Amt.\n\
        Beginning balance as of 01/01/2025,,\"1,000.00\"\n\
        \n\
        Date,Description,Amount,Running Bal.\n\
        01/01/2025,Beginning balance as of 01/01/2025,,\"1,000.00\"\n\
        01/13/2025,\"DEMO PAYROLL DEPOSIT\",\"2,500.00\",\"3,500.00\"\n\
        01/20/2025,\"DEMO RENT PAYMENT\",\"-1,800.00\",\"1,700.00\"\n";
    let l = uno("stmt.csv", csv.as_bytes());
    let e = &l.extracto;
    assert_eq!(l.cuadre, Cuadre::Cuadra);
    assert_eq!(fechas(e), [date!(2025 - 01 - 13), date!(2025 - 01 - 20)]);
    assert_eq!(importes(e), [250_000, -180_000]);
    assert_eq!(cents(e.saldo_inicial), Some(100_000));
}

#[test]
fn csv_con_columna_debe_haber_y_otro_sin_signo() {
    let csv = "Fecha;Concepto;Importe;D/H;Saldo\n\
        02/04/2025;RECIBO DEMO;30,00;D;470,00\n\
        03/04/2025;INGRESO DEMO;100,00;H;570,00\n";
    let l = uno("movs.csv", csv.as_bytes());
    assert_eq!(importes(&l.extracto), [-3_000, 10_000]);
    assert_eq!(l.cuadre, Cuadre::Cuadra);

    // Sin signo ni columna de signo: se saca del saldo.
    let csv = "Fecha;Concepto;Importe;Saldo\n\
        03/04/2025;INGRESO DEMO;100,00;570,00\n\
        02/04/2025;RECIBO DEMO;30,00;470,00\n\
        01/04/2025;TRANSFERENCIA DEMO;20,00;500,00\n";
    let l = uno("movs.csv", csv.as_bytes());
    assert_eq!(importes(&l.extracto)[1..], [-3_000, 10_000]);
    assert_eq!(l.cuadre, Cuadre::Cuadra);
}

#[test]
fn hoja_de_cargos_y_abonos_con_signos_del_banco_al_reves() {
    // Algunos bancos dan el debe y el haber como los ve el banco: el saldo lo delata.
    let csv = "Fecha;Concepto;Debe;Haber;Saldo\n\
        01/04/2025;INGRESO DEMO;100,00;;600,00\n\
        02/04/2025;RECIBO DEMO;;30,00;570,00\n";
    let l = uno("movs.csv", csv.as_bytes());
    assert_eq!(importes(&l.extracto), [10_000, -3_000]);
    assert!(l.extracto.avisos.iter().any(|a| a.contains("al revés")));
}

#[test]
fn csv_de_caixabank_y_sabadell_con_tabuladores_y_referencias() {
    let tsv = "Concepto\tFecha\tFecha valor\tMovimiento\tMás datos\tImporte\tSaldo\n\
        DEMO\t10/04/2025\t10/04/2025\tTRANSF A FAVOR\tCLIENTE DEMO\t500,00\t1.500,00\n\
        DEMO\t11/04/2025\t11/04/2025\tRECIBO\tSEGURO DEMO\t-45,99\t1.454,01\n";
    let l = uno("movimientos.txt", tsv.as_bytes());
    assert_eq!(importes(&l.extracto), [50_000, -4_599]);
    assert_eq!(
        l.extracto.movimientos[0].concepto,
        "DEMO · TRANSF A FAVOR · CLIENTE DEMO"
    );
    let csv = "F. Operativa;Concepto;F. Valor;Importe;Saldo;Referencia 1;Referencia 2\n\
        15/04/2025;COMPRA TARJ. DEMO;15/04/2025;-12,00;988,00;1111;2222\n";
    let l = uno("sabadell.csv", csv.as_bytes());
    assert_eq!(importes(&l.extracto), [-1_200]);
    assert_eq!(l.extracto.movimientos[0].referencia, "1111 2222");
}

#[test]
fn csv_con_la_cuenta_en_cada_fila_y_varias_cuentas() {
    // Como bunq: la cuenta propia en cada fila.
    let csv = "\"Date\",\"Interest Date\",\"Amount\",\"Account\",\"Counterparty\",\"Name\",\"Description\"\n\
        \"2025-05-02\",\"2025-05-02\",\"-15.00\",\"GB82WEST12345698765432\",\"\",\"Demo Cafe\",\"Lunch\"\n\
        \"2025-05-03\",\"2025-05-03\",\"200.00\",\"GB82WEST12345698765432\",\"ES9121000418450200051332\",\"Demo Client\",\"Invoice 4\"\n";
    let l = uno("bunq.csv", csv.as_bytes());
    assert_eq!(l.extracto.iban.as_deref(), Some(IBAN_GB));
    assert!(l.extracto.avisos.is_empty(), "{:?}", l.extracto.avisos);
    // Como Rabobank: varias cuentas en el mismo CSV, con su saldo.
    let csv = "\"IBAN/BBAN\",\"Munt\",\"Volgnr\",\"Datum\",\"Rentedatum\",\"Bedrag\",\"Saldo na trn\",\"Naam tegenpartij\",\"Omschrijving-1\"\n\
        \"GB82WEST12345698765432\",\"EUR\",\"1\",\"2025-05-02\",\"2025-05-02\",\"-15,00\",\"85,00\",\"Demo Cafe\",\"Lunch\"\n\
        \"ES9121000418450200051332\",\"EUR\",\"1\",\"2025-05-02\",\"2025-05-02\",\"+200,00\",\"1200,00\",\"Demo Client\",\"Factura 4\"\n\
        \"GB82WEST12345698765432\",\"EUR\",\"2\",\"2025-05-03\",\"2025-05-03\",\"-5,00\",\"80,00\",\"Demo Shop\",\"\"\n";
    let l = leer("rabo.csv", csv.as_bytes()).unwrap();
    assert_eq!(l.len(), 2);
    let gb = l.iter().find(|l| l.extracto.cuenta == IBAN_GB).unwrap();
    assert_eq!(importes(&gb.extracto), [-1_500, -500]);
    assert_eq!(gb.cuadre, Cuadre::Cuadra);
    assert_eq!(gb.extracto.movimientos[0].referencia, "1");
    let es = l
        .iter()
        .find(|l| l.extracto.iban.as_deref() == Some(IBAN))
        .unwrap();
    assert_eq!(importes(&es.extracto), [20_000]);
}

#[test]
fn csv_neerlandes_con_af_bij_y_fechas_de_ocho_cifras() {
    let csv = "\"Datum\";\"Naam / Omschrijving\";\"Rekening\";\"Tegenrekening\";\"Code\";\"Af Bij\";\"Bedrag (EUR)\";\"Mutatiesoort\";\"Mededelingen\";\"Saldo na mutatie\"\n\
        \"20250603\";\"Demo Supermarkt\";\"GB82WEST12345698765432\";\"\";\"BA\";\"Af\";\"23,45\";\"Betaalautomaat\";\"Pasvolgnr: 001\";\"976,55\"\n\
        \"20250601\";\"Demo Klant BV\";\"GB82WEST12345698765432\";\"ES9121000418450200051332\";\"OV\";\"Bij\";\"1000,00\";\"Overschrijving\";\"Factuur 3\";\"1000,00\"\n";
    let l = uno("NL-transacties.csv", csv.as_bytes());
    let e = &l.extracto;
    assert_eq!(l.cuadre, Cuadre::Cuadra);
    assert_eq!(e.iban.as_deref(), Some(IBAN_GB));
    assert_eq!(fechas(e), [date!(2025 - 06 - 01), date!(2025 - 06 - 03)]);
    assert_eq!(importes(e), [100_000, -2_345]);
    assert_eq!(e.movimientos[0].concepto, "Demo Klant BV · Factuur 3");
}

#[test]
fn csv_de_revolut_en_espanol_y_paypal_en_espanol_con_avisos_sin_saldo() {
    let csv = "Tipo,Producto,Fecha de inicio,Fecha de finalización,Descripción,Importe,Comisión,Divisa,Estado,Saldo\n\
        TRANSFERENCIA,Actual,2025-02-01 10:00:00,2025-02-01 10:00:02,Recibido de CLIENTE DEMO,300.00,0.00,EUR,COMPLETADO,300.00\n\
        PAGO CON TARJETA,Actual,2025-02-03 18:00:00,2025-02-04 09:00:00,Tienda Demo,-30.00,0.00,EUR,COMPLETADO,270.00\n\
        PAGO CON TARJETA,Actual,2025-02-05 18:00:00,,Otra Tienda Demo,-10.00,0.00,EUR,PENDIENTE,\n\
        PAGO CON TARJETA,Actual,2025-02-06 18:00:00,2025-02-06 18:00:00,Tienda Rechazada,-99.00,0.00,EUR,REVERTIDO,\n";
    let l = uno("account-statement_2025-02-01.csv", csv.as_bytes());
    assert_eq!(importes(&l.extracto), [30_000, -3_000]);
    assert_eq!(l.extracto.movimientos[1].fecha, date!(2025 - 02 - 04));
    assert_eq!(l.cuadre, Cuadre::Cuadra);

    let csv = "\u{feff}\"Fecha\",\"Hora\",\"Zona horaria\",\"Nombre\",\"Tipo\",\"Estado\",\"Divisa\",\"Bruto\",\"Tarifa\",\"Neto\",\"Id. de transacción\",\"Saldo\",\"Impacto en el saldo\"\n\
        \"01/03/2025\",\"10:00:00\",\"CET\",\"Cliente Demo\",\"Pago estándar\",\"Completado\",\"EUR\",\"50,00\",\"-2,10\",\"47,90\",\"T1\",\"47,90\",\"Crédito\"\n\
        \"02/03/2025\",\"11:00:00\",\"CET\",\"Tienda Demo\",\"Autorización\",\"Pendiente\",\"EUR\",\"-5,00\",\"0,00\",\"-5,00\",\"T2\",\"47,90\",\"Memo\"\n\
        \"03/03/2025\",\"12:00:00\",\"CET\",\"Tienda Demo\",\"Pago\",\"Completado\",\"EUR\",\"-7,90\",\"0,00\",\"-7,90\",\"T3\",\"40,00\",\"Débito\"\n";
    let l = uno("paypal.csv", csv.as_bytes());
    assert_eq!(importes(&l.extracto), [4_790, -790]);
    assert_eq!(l.cuadre, Cuadre::Cuadra);
}

#[test]
fn csv_de_francia_con_columnas_en_camelcase_con_el_saldo_de_la_cuenta_repetido() {
    let csv = "dateOp;dateVal;label;category;categoryParent;supplierFound;amount;comment;accountNum;accountLabel;accountbalance\n\
        2025-04-02;2025-04-02;CARTE DEMO CAFE;Restaurants;Loisirs;demo;-4,50;;00012345678;Compte Demo;1520,00\n\
        2025-04-01;2025-04-01;VIR DEMO CLIENT;Virements;Revenus;;1200,00;Facture 5;00012345678;Compte Demo;1520,00\n\
        2025-03-28;2025-03-28;PRLV DEMO TELECOM;Telecom;Maison;demo;-29,99;;00012345678;Compte Demo;1520,00\n";
    let l = uno("export.csv", csv.as_bytes());
    let e = &l.extracto;
    // El saldo repetido no es el de cada movimiento: no hay saldos con que comprobarlo.
    assert_eq!(l.cuadre, Cuadre::SinSaldos);
    assert_eq!(importes(e), [-2_999, 120_000, -450]);
    assert_eq!(e.cuenta, "00012345678");
    assert_eq!(e.movimientos[1].concepto, "VIR DEMO CLIENT · Facture 5");
    assert_eq!(e.movimientos[0].fecha_valor, Some(date!(2025 - 03 - 28)));
}

#[test]
fn csv_en_windows_1252_y_con_finales_de_linea_de_mac() {
    // «€» es el byte 0x80 en Windows-1252; las líneas acaban sólo en «\r».
    let mut csv: Vec<u8> = Vec::new();
    for l in [
        "Fecha;Concepto;Importe;Saldo",
        "01/06/2025;RECIBO DEMO;-12,00 \u{80};88,00 \u{80}",
        "02/06/2025;INGRESO DEMO;100,00 \u{80};188,00 \u{80}",
    ] {
        csv.extend(l.chars().map(|c| u8::try_from(u32::from(c)).unwrap()));
        csv.push(b'\r');
    }
    let l = uno("movimientos.csv", &csv);
    assert_eq!(importes(&l.extracto), [-1_200, 10_000]);
    assert_eq!(l.cuadre, Cuadre::Cuadra);
}

#[test]
fn hojas_con_titulos_en_ingles_y_columnas_vacias_o_el_saldo_antes_del_importe() {
    // Como la de Deutsche Bank: los títulos en inglés con guiones bajos y cuatro columnas
    // vacías por las celdas combinadas.
    let filas = texto(&[
        &["Fecha:", "10-04-2026 10:52:52"],
        &[],
        &["Cuenta:", IBAN, "", "Moneda:", "EUR"],
        &["Fecha desde:", "01/03/2026", "Fecha hasta:", "31/03/2026"],
        &[
            "Saldo actual:",
            "1.050,00",
            "Saldo retenido:",
            "0,00",
            "Saldo disponible:",
            "1.050,00",
        ],
        &[],
        &[
            "date",
            "value_date",
            "concept",
            "",
            "",
            "",
            "",
            "amount",
            "balance",
        ],
        &[
            "02/03/2026",
            "02/03/2026",
            "TRANSFERENCIA RECIBIDA",
            "",
            "",
            "",
            "",
            "100,00",
            "1.100,00",
        ],
        &[
            "05/03/2026",
            "05/03/2026",
            "RECIBO DEMO",
            "",
            "",
            "",
            "",
            "-50,00",
            "1.050,00",
        ],
    ]);
    let l = uno("export.xls", &xlsx(&[("Hoja1", filas)]));
    let e = &l.extracto;
    assert_eq!(l.cuadre, Cuadre::Cuadra);
    assert_eq!(e.iban.as_deref(), Some(IBAN));
    assert_eq!(importes(e), [10_000, -5_000]);
    assert_eq!(
        (e.desde, e.hasta),
        (Some(date!(2026 - 03 - 01)), Some(date!(2026 - 03 - 31)))
    );
    assert!(e.avisos.is_empty(), "{:?}", e.avisos);

    // Como Abanca: el saldo antes que el importe, el importe con « EUR» y un aviso legal
    // debajo.
    let csv = "Fecha;Concepto;Saldo;Importe;Fecha operación;Fecha valor\n\
        05-05-2025;RECIBO DEMO;1180.49 EUR;-19.51 EUR;2025-05-05T00:00:00;2025-05-05T00:00:00\n\
        06-05-2025;TRANSFERENCIA DEMO;1380.49 EUR;200.00 EUR;2025-05-06T00:00:00;2025-05-07T00:00:00\n\
        Este documento es meramente informativo y no tiene validez contractual.\n";
    let l = uno("abanca.csv", csv.as_bytes());
    assert_eq!(importes(&l.extracto), [-1_951, 20_000]);
    assert_eq!(l.cuadre, Cuadre::Cuadra);
    assert_eq!(
        l.extracto.movimientos[1].fecha_valor,
        Some(date!(2025 - 05 - 07))
    );
}

#[test]
fn hojas_por_divisa_con_numeros_de_la_hoja() {
    // Una hoja por divisa, sin más datos, y los importes como números de la hoja: «1.234» es
    // uno con veintitrés, no mil doscientos treinta y cuatro.
    let hoja = |filas: &[(&'static str, f64, f64)]| {
        let mut v = texto(&[&["Date", "Description", "Amount", "Balance"]]);
        for (f, i, s) in filas {
            v.push(vec![C::T(f), C::T("DEMO"), C::N(*i), C::N(*s)]);
        }
        v
    };
    let libro = xlsx(&[
        (
            "EUR",
            hoja(&[("2025-01-02", 10.0, 10.0), ("2025-01-03", -1.234, 8.766)]),
        ),
        ("USD", hoja(&[("2025-01-02", 5.5, 5.5)])),
    ]);
    let l = leer("saldos.xlsx", &libro).unwrap();
    assert_eq!(l.len(), 2);
    let eur = l.iter().find(|l| l.extracto.divisa == "EUR").unwrap();
    assert_eq!(importes(&eur.extracto), [1_000, -123]);
    let usd = l.iter().find(|l| l.extracto.divisa == "USD").unwrap();
    assert_eq!(importes(&usd.extracto), [550]);
    assert_eq!(usd.cuadre, Cuadre::Cuadra);
}

#[test]
fn lo_que_no_es_un_extracto() {
    // Una lista de cobros sin fecha, o una tabla cualquiera, no son movimientos bancarios.
    let csv = "Name;Email;Amount;Fee\nCliente Demo;demo@example.com;10,00;0,50\n";
    assert!(leer("pagos.csv", csv.as_bytes()).is_err());
    let csv = "Producto;Precio;Stock\nTornillo;0,10;100\n";
    assert!(leer("stock.csv", csv.as_bytes()).is_err());
    assert!(leer("vacia.csv", b"").is_err());
}

#[test]
fn cuenta_con_iban_que_menciona_la_tarjeta_en_el_titulo_y_el_pie() {
    // «Tarjeta» en el título y en un pie no hace tarjeta de crédito (5201) a una cuenta
    // corriente que enseña su IBAN (572).
    let csv = "Consulta de movimientos de cuenta y tarjeta\n\
        Cuenta;ES91 2100 0418 4502 0005 1332\n\
        Titular;EMPRESA DEMO SL\n\
        Fecha;Concepto;Importe;Saldo\n\
        01/02/2025;TRANSFERENCIA CLIENTE A;1.000,00;2.000,00\n\
        02/02/2025;TRANSFERENCIA CLIENTE B;500,00;2.500,00\n\
        03/02/2025;TRANSFERENCIA CLIENTE C;250,00;2.750,00\n\
        04/02/2025;RECIBO LUZ;-80,00;2.670,00\n\
        Para más información sobre su tarjeta consulte la app\n";
    let l = uno("cuenta.csv", csv.as_bytes());
    assert_eq!(l.cuadre, Cuadre::Cuadra);
    assert_eq!(l.extracto.clase, Clase::Cuenta);
    assert_eq!(l.extracto.cuenta, "210004180200051332");
    assert_eq!(l.extracto.iban.as_deref(), Some(IBAN));
    assert_eq!(importes(&l.extracto), [100_000, 50_000, 25_000, -8_000]);
    // Con el IBAN enmascarado, lo mismo (y la clave es la de la cuenta, no la de una tarjeta).
    let csv = csv.replace(
        "ES91 2100 0418 4502 0005 1332",
        "ES91 **** **** **** **** 1332",
    );
    let l = uno("cuenta.csv", csv.as_bytes());
    assert_eq!(l.extracto.clase, Clase::Cuenta);
    assert_eq!(l.extracto.cuenta, "ES91*1332");
    // Un listado de tarjeta de verdad lo sigue siendo.
    let csv = "Listado de movimientos de tarjeta\n\
        Nº de tarjeta;5400 **** **** 1234\n\
        Fecha;Concepto;Importe\n\
        01/02/2025;RESTAURANTE DEMO;-40,00\n\
        02/02/2025;HOSTING DEMO;-12,00\n";
    let l = uno("tarjeta.csv", csv.as_bytes());
    assert_eq!(l.extracto.clase, Clase::TarjetaCredito);
    assert_eq!(l.extracto.cuenta, "tarjeta:?:1234");
}

#[test]
fn una_ciudad_en_la_cabecera_no_elige_el_banco_ni_fabrica_el_iban() {
    // «Santander» en la dirección no es Banco Santander: con la cuenta parcial (oficina y
    // número, sin entidad) la clave queda provisional, sin IBAN de otra entidad.
    let csv = "EMPRESA DEMO SL - Calle Falsa 1 - 39001 Santander\n\
        Número de cuenta;0418/45.00051332\n\
        Fecha;Concepto;Importe;Saldo\n\
        01/02/2025;TRANSFERENCIA CLIENTE A;1.000,00;2.000,00\n\
        02/02/2025;RECIBO LUZ;-80,00;1.920,00\n";
    let l = uno("movs.csv", csv.as_bytes());
    assert_eq!(l.cuadre, Cuadre::Cuadra);
    assert_eq!(l.extracto.banco, "");
    assert_eq!(l.extracto.iban, None);
    assert_eq!(l.extracto.cuenta, "04184500051332");
    // Con el banco en su etiqueta, sí: el IBAN con la entidad del Santander (0049).
    let con_banco = csv.replacen(
        "Número de cuenta;",
        "Banco;Banco Santander\nNúmero de cuenta;",
        1,
    );
    let l = uno("movs.csv", con_banco.as_bytes());
    assert_eq!(l.extracto.banco, "Banco Santander");
    assert_eq!(l.extracto.iban.as_deref(), Some("ES4400490418964500051332"));
    // Y con la huella de la descarga de Bankinter, con la suya (0128).
    let bankinter = format!("{csv}Abonos pendientes;0,00\nTotal retenciones;0,00\n");
    let l = uno("movs.csv", bankinter.as_bytes());
    assert_eq!(l.extracto.banco, "Bankinter");
    assert!(l.extracto.iban.as_deref().unwrap().starts_with("ES"));
    assert_eq!(&l.extracto.cuenta[..8], "01280418");
}

#[test]
fn una_fila_con_fecha_que_no_se_entiende_avisa() {
    // El 29/02/2025 no existe (o es un OCR mal leído): la fila se pierde, y antes nadie lo
    // decía; con saldos, además, el extracto no cuadra.
    let csv = "Fecha;Concepto;Importe;Saldo\n\
        28/02/2025;RECIBO A;-10,00;90,00\n\
        29/02/2025;RECIBO B;-20,00;70,00\n\
        01/03/2025;INGRESO C;5,00;75,00\n";
    let l = uno("movs.csv", csv.as_bytes());
    assert_eq!(l.extracto.movimientos.len(), 2);
    assert!(matches!(l.cuadre, Cuadre::NoCuadra { .. }));
    assert!(
        l.extracto
            .avisos
            .iter()
            .any(|a| a.starts_with("1 fila con importe tiene una fecha que no se entiende")),
        "{:?}",
        l.extracto.avisos
    );
}

#[test]
fn la_cache_de_lectura_no_guarda_errores_ni_mezcla_extensiones() {
    // Los mismos bytes como imagen (sin lector de texto en las pruebas: error) y como CSV: el
    // error de la imagen no puede valer para el CSV, ni el acierto del CSV para la imagen.
    let csv = "Fecha;Concepto;Importe;Saldo\n\
        01/04/2025;INGRESO CACHE DEMO;100,00;600,00\n\
        02/04/2025;RECIBO CACHE DEMO;-30,00;570,00\n";
    assert!(leer("foto.png", csv.as_bytes()).is_err());
    let l = uno("movs.csv", csv.as_bytes());
    assert_eq!(l.extracto.movimientos.len(), 2);
    assert!(leer("foto.png", csv.as_bytes()).is_err());
    // Y otra vez el CSV, ya de la caché, igual.
    assert_eq!(uno("movs.csv", csv.as_bytes()), l);
}
