//! Revisión de la carpeta: el IVA de los modelos y las facturas frente a los libros registro de
//! la gestoría (con sus hojas de interiores, inversión del sujeto pasivo, adquisiciones
//! intracomunitarias y no deducibles) y frente a las cuentas 472 y 477 del diario. Datos
//! inventados, NIF de prueba.

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::sync::Arc;

use rust_xlsxwriter::Workbook;
use serde_json::{Value, json};
use taxpain_app::companies::{self, CompanyInfo};
use taxpain_app::revision::{self, Gravedad, Hallazgo, Revision};
use taxpain_core::money::Money;
use taxpain_invoicing::pdf::{Para, text_document};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

const PROPIO: &str = "B98765431";
const PAPELERIA: &str = "B12345674";
const ASESORES: &str = "B11111119";
const RESTAURANTE: &str = "B33333337";
const CLIENTE: &str = "B22222228";
const PIEZAS_UE: &str = "FR12345678901";

fn vault() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let info = CompanyInfo {
        name: "Empresa Demo SL".into(),
        nif: Some(PROPIO.into()),
    };
    companies::set_info(&v, &info, datetime!(2026-01-01 9:00 UTC)).unwrap();
    v
}

/// Un PDF con texto distinto para cada fichero (las lecturas van por la huella).
fn pdf(texto: &str) -> Vec<u8> {
    text_document("Documento", &[vec![Para::Text(texto.into())]]).unwrap()
}

fn escribir(root: &Path, ruta: &str, bytes: &[u8]) {
    let p = root.join(ruta);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

/// Un fichero de la carpeta con (o sin) su lectura.
type Fichero = (String, Vec<u8>, Option<Value>);

fn con(ruta: &str, lectura: Value) -> Fichero {
    (
        ruta.to_owned(),
        pdf(&format!("Documento {ruta}")),
        Some(lectura),
    )
}

fn revisar_con(v: &Vault, ficheros: Vec<Fichero>) -> Revision {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut lecturas = Vec::new();
    for (ruta, bytes, lectura) in ficheros {
        escribir(root, &ruta, &bytes);
        if let Some(l) = lectura {
            lecturas.push(json!({ "fichero": ruta, "lectura": l }));
        }
    }
    let paquete = json!({ "version": 1, "generado_por": "Claude Code", "lecturas": lecturas });
    std::fs::write(
        root.join("taxpain-lecturas.json"),
        serde_json::to_vec_pretty(&paquete).unwrap(),
    )
    .unwrap();
    revision::revisar(v, root, date!(2026 - 10 - 25)).unwrap()
}

fn revisar(ficheros: Vec<Fichero>) -> Revision {
    revisar_con(&vault(), ficheros)
}

fn hallazgo<'a>(r: &'a Revision, codigo: &str) -> &'a Hallazgo {
    r.hallazgos
        .iter()
        .find(|h| h.codigo == codigo)
        .unwrap_or_else(|| {
            panic!(
                "falta {codigo}: {:?}",
                r.hallazgos
                    .iter()
                    .map(|h| (&h.codigo, &h.titulo))
                    .collect::<Vec<_>>()
            )
        })
}

fn hay(r: &Revision, codigo: &str) -> bool {
    r.hallazgos.iter().any(|h| h.codigo == codigo)
}

fn tabla<'a>(r: &'a Revision, titulo: &str) -> &'a [Vec<String>] {
    &r.tablas
        .iter()
        .find(|t| t.titulo == titulo)
        .unwrap_or_else(|| panic!("falta la tabla {titulo}"))
        .filas
}

fn modelo(m: &str, ejercicio: i32, periodo: &str, casillas: Value) -> Value {
    json!({
        "tipo": "modelo_aeat",
        "descripcion": format!("Modelo {m} {periodo} {ejercicio}"),
        "modelo": { "modelo": m, "ejercicio": ejercicio, "periodo": periodo, "casillas": casillas }
    })
}

fn recibida(
    nif: Option<&str>,
    nombre: &str,
    numero: &str,
    fecha: &str,
    importes: [&str; 3],
) -> Value {
    json!({
        "tipo": "factura_recibida",
        "descripcion": format!("Factura {numero} de {nombre}"),
        "factura": {
            "nif_emisor": nif, "nombre_emisor": nombre,
            "nif_destinatario": PROPIO, "nombre_destinatario": "Empresa Demo SL",
            "numero": numero, "fecha": fecha,
            "base": importes[0], "iva": importes[1], "total": importes[2]
        }
    })
}

/// Títulos de un libro registro de a3.
const TITULOS: [&str; 12] = [
    "NºOrden",
    "N.Referencia",
    "Núm.Fact.",
    "Fecha",
    "Concepto",
    "N.I.F.",
    "Expedidor",
    "Base Imponible",
    "%IVA",
    "Cuota",
    "Retención",
    "Total Fra.",
];

/// Una línea del libro: número, fecha («dd/mm»), NIF, nombre, base, tipo, cuota y total.
type Linea<'a> = (
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
);

/// Libro registro de Excel como los de a3: una hoja por `(nombre, título, líneas)`, con el
/// periodo en la cabecera y las fechas sin año.
fn libro_xlsx(periodo: &str, destinatario: bool, hojas: &[(&str, &str, Vec<Linea>)]) -> Vec<u8> {
    let mut wb = Workbook::new();
    for (nombre, titulo, lineas) in hojas {
        let ws = wb.add_worksheet();
        ws.set_name(*nombre).unwrap();
        let mut titulos = TITULOS;
        if destinatario {
            titulos[6] = "Destinatario";
        }
        let mut filas: Vec<Vec<String>> = vec![
            vec![(*titulo).to_owned()],
            vec![],
            vec!["Empresa: EMPRESA DEMO SL".into()],
            vec![periodo.to_owned()],
            vec!["Fecha: 04/03/2026".into()],
            vec![],
            titulos.iter().map(|t| (*t).to_owned()).collect(),
        ];
        for (n, (num, fecha, nif, nombre, base, tipo, cuota, total)) in lineas.iter().enumerate() {
            filas.push(
                [
                    &(n + 1).to_string(),
                    &(n + 1).to_string(),
                    *num,
                    *fecha,
                    &format!("Su Fra. {num}"),
                    *nif,
                    *nombre,
                    *base,
                    *tipo,
                    *cuota,
                    "0",
                    *total,
                ]
                .iter()
                .map(|c| (*c).to_owned())
                .collect(),
            );
        }
        for (r, fila) in filas.iter().enumerate() {
            for (c, celda) in fila.iter().enumerate() {
                if !celda.is_empty() {
                    ws.write_string(u32::try_from(r).unwrap(), u16::try_from(c).unwrap(), celda)
                        .unwrap();
                }
            }
        }
    }
    wb.save_to_buffer().unwrap()
}

const PERIODO_2025: &str = "Período De 01 Ene a 31 Dic 2025";

/// El libro de recibidas de 2025 de la gestoría: interiores, inversión del sujeto pasivo de un
/// proveedor de fuera de la UE (total sin la cuota), una adquisición intracomunitaria y una
/// comida en la hoja de no deducibles. En el 2T: 500 € interiores (105 € de cuota), 50 € de
/// inversión del sujeto pasivo (10,50 €), 100 € intracomunitarios (21 €); en el 3T, 100 €.
fn libro_recibidas() -> Fichero {
    let bytes = libro_xlsx(
        PERIODO_2025,
        false,
        &[
            (
                "Facturas",
                "Operaciones Interiores - Facturas Recibidas",
                vec![
                    (
                        "P-201",
                        "10/04",
                        PAPELERIA,
                        "PAPELERIA DEMO SL",
                        "300",
                        "21,0",
                        "63",
                        "363",
                    ),
                    (
                        "A-55",
                        "20/05",
                        ASESORES,
                        "ASESORES DEMO SL",
                        "200",
                        "21,0",
                        "42",
                        "242",
                    ),
                    (
                        "P-230",
                        "10/08",
                        PAPELERIA,
                        "PAPELERIA DEMO SL",
                        "100",
                        "21,0",
                        "21",
                        "121",
                    ),
                ],
            ),
            (
                "Sujeto pasivo",
                "Inversiones Sujeto Pasivo",
                vec![(
                    "US-9",
                    "15/06",
                    "",
                    "SOFTWARE DEMO INC",
                    "50",
                    "21,0",
                    "10,5",
                    "50",
                )],
            ),
            (
                "Adquisiciones",
                "Adquisiciones Intracomunitarias",
                vec![(
                    "FR-3",
                    "12/05",
                    PIEZAS_UE,
                    "PIECES DEMO SARL",
                    "100",
                    "21,0",
                    "21",
                    "100",
                )],
            ),
            (
                "No deducibles",
                "Operaciones I.V.A. no Deducible",
                vec![(
                    "R-7",
                    "05/06",
                    RESTAURANTE,
                    "RESTAURANTE DEMO SL",
                    "150",
                    "10,0",
                    "15",
                    "165",
                )],
            ),
        ],
    );
    (
        "LIBROS/2025/2025 - Libro registro recibidas.xlsx".into(),
        bytes,
        None,
    )
}

/// El libro de emitidas de 2025: 1.000 € de ventas en el 2T.
fn libro_emitidas() -> Fichero {
    let bytes = libro_xlsx(
        PERIODO_2025,
        true,
        &[(
            "Facturas",
            "Operaciones Interiores - Facturas Expedidas",
            vec![
                (
                    "2025-1",
                    "15/04",
                    CLIENTE,
                    "CLIENTE DEMO SL",
                    "600",
                    "21,0",
                    "126",
                    "726",
                ),
                (
                    "2025-2",
                    "15/06",
                    CLIENTE,
                    "CLIENTE DEMO SL",
                    "400",
                    "21,0",
                    "84",
                    "484",
                ),
            ],
        )],
    );
    (
        "LIBROS/2025/2025 - Libro registro emitidas.xlsx".into(),
        bytes,
        None,
    )
}

/// El 303 del 2T 2025 que cuadra con los libros.
fn m303_2t() -> Fichero {
    con(
        "MODELOS/2025/303-2T.pdf",
        modelo(
            "303",
            2025,
            "2T",
            json!({
                "07": "1000.00", "09": "210.00",
                "10": "100.00", "11": "21.00", "12": "50.00", "13": "10.50",
                "27": "241.50", "28": "550.00", "29": "115.50",
                "36": "100.00", "37": "21.00", "45": "136.50"
            }),
        ),
    )
}

#[test]
fn el_303_cuadra_con_los_libros_registro() {
    let r = revisar(vec![libro_recibidas(), libro_emitidas(), m303_2t()]);
    let h = hallazgo(&r, "modelos.303_cuadra_libro");
    assert_eq!(h.gravedad, Gravedad::Info);
    assert_eq!(h.ejercicio, Some(2025));
    assert_eq!(
        h.titulo,
        "El 303 del 2T 2025 cuadra con los libros registro"
    );
    for x in [
        "[28]+[30] (550,00 €: 500,00 € de facturas interiores y 50,00 € de inversión del sujeto \
         pasivo)",
        "[10] (100,00 €)",
        "[12] (50,00 €)",
        "la base de IVA devengado (1.000,00 €)",
    ] {
        assert!(h.detalle.contains(x), "«{x}» no está en: {}", h.detalle);
    }
    assert!(!hay(&r, "modelos.303_libro"), "{:?}", r.hallazgos);
    let filas = tabla(&r, "303 frente a los libros registro");
    assert!(
        filas.iter().all(|f| f[0] == "2T 2025" && f[4] == "0,00 €"),
        "{filas:?}"
    );
    assert_eq!(filas.len(), 6, "{filas:?}");
}

#[test]
fn el_303_que_no_casa_con_el_libro_es_un_aviso_para_la_gestoria() {
    // En el 3T el libro sólo tiene 100 € (21 € de cuota) y el 303 deduce 180 €.
    let m3t = con(
        "MODELOS/2025/303-3T.pdf",
        modelo("303", 2025, "3T", json!({"28": "180.00", "29": "37.80"})),
    );
    let r = revisar(vec![libro_recibidas(), m3t]);
    let h = hallazgo(&r, "modelos.303_libro");
    assert_eq!(h.gravedad, Gravedad::Aviso);
    assert_eq!(
        h.titulo,
        "El 303 del 3T 2025 no casa con los libros registro"
    );
    assert!(
        h.detalle.contains(
            "la base de IVA soportado [28]+[30]: 180,00 € en el 303 y 100,00 € en el libro"
        ),
        "{}",
        h.detalle
    );
    assert!(
        h.ficheros
            .iter()
            .any(|f| f.ends_with("Libro registro recibidas.xlsx"))
    );
    assert!(
        r.pedir
            .iter()
            .any(|p| p.que.contains("303 del 3T 2025") && p.que.contains("libros registro"))
    );
    assert!(!hay(&r, "modelos.303_cuadra_libro"));
}

#[test]
fn con_el_libro_no_se_acusa_al_303_por_los_pdf_que_faltan() {
    // Sólo está el PDF de la papelería (300 €): el resto del libro no tiene su PDF en la carpeta,
    // y eso es un papel que falta, no un 303 que deduce de más.
    let papeleria = con(
        "FACTURAS/2025/T2/papeleria-201.pdf",
        recibida(
            Some(PAPELERIA),
            "Papelería Demo SL",
            "P-201",
            "2025-04-10",
            ["300.00", "63.00", "363.00"],
        ),
    );
    let r = revisar(vec![
        libro_recibidas(),
        libro_emitidas(),
        m303_2t(),
        papeleria,
    ]);
    assert!(!hay(&r, "modelos.303_falta_factura"), "{:?}", r.hallazgos);
    assert!(hay(&r, "modelos.303_cuadra_libro"));
    // Lo que falta es la factura del asesor (y las demás del libro): documentación.
    assert!(r.hallazgos.iter().any(|h| h.codigo == "libros.sin_pdf"
        && h.titulo.contains("recibidas")
        && h.detalle.contains("A-55")));
}

#[test]
fn la_autorrepercusion_del_libro_de_emitidas_no_son_ventas() {
    // El libro de emitidas sólo trae la autorrepercusión de las compras de fuera (las mismas
    // facturas que el de recibidas): el 303 no tiene ventas y cuadra.
    let periodo = "Período De 01 Ene a 30 Sep 2026";
    let compras = vec![
        (
            "US-17",
            "05/04",
            "",
            "SOFTWARE DEMO INC",
            "80",
            "21,0",
            "16,8",
            "80",
        ),
        (
            "US-20",
            "05/07",
            "",
            "SOFTWARE DEMO INC",
            "80",
            "21,0",
            "16,8",
            "80",
        ),
        (
            "FR-21",
            "10/08",
            PIEZAS_UE,
            "PIECES DEMO SARL",
            "40",
            "21,0",
            "8,4",
            "40",
        ),
    ];
    let recibidas = (
        "LIBROS/2026/recibidas.xlsx".to_owned(),
        libro_xlsx(
            periodo,
            false,
            &[("Facturas", "Facturas Recibidas", compras.clone())],
        ),
        None,
    );
    let emitidas = (
        "LIBROS/2026/emitidas.xlsx".to_owned(),
        libro_xlsx(
            periodo,
            true,
            &[("Facturas", "Facturas Expedidas", compras)],
        ),
        None,
    );
    let m303 = con(
        "MODELOS/2026/303-3T.pdf",
        modelo(
            "303",
            2026,
            "3T",
            json!({
                "10": "40.00", "11": "8.40", "12": "80.00", "13": "16.80", "27": "25.20",
                "28": "80.00", "29": "16.80", "36": "40.00", "37": "8.40", "45": "25.20"
            }),
        ),
    );
    let r = revisar(vec![recibidas, emitidas, m303]);
    let h = hallazgo(&r, "modelos.303_cuadra_libro");
    assert_eq!(
        h.titulo,
        "El 303 del 3T 2026 cuadra con los libros registro"
    );
    assert!(!hay(&r, "modelos.303_libro"), "{:?}", r.hallazgos);
    assert!(!hay(&r, "modelos.303_devengado"), "{:?}", r.hallazgos);
    // Ni son emitidas sin PDF ni saltos en una numeración (la del proveedor).
    let h = hallazgo(&r, "libros.autorrepercusion");
    assert_eq!(
        h.titulo,
        "El libro de emitidas de 2026 es la autorrepercusión de compras"
    );
    assert!(
        !r.hallazgos
            .iter()
            .any(|h| h.codigo == "libros.sin_pdf" && h.titulo.contains("emitidas")),
        "{:?}",
        r.hallazgos
    );
    assert!(!hay(&r, "emitidas.saltos"), "{:?}", r.hallazgos);
}

#[test]
fn el_303_frente_a_la_472_y_la_477_del_diario() {
    use taxpain_app::accounting;
    use taxpain_core::accounting::account::acc;
    use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};

    let v = vault();
    let asiento = |fecha: time::Date, concepto: &str, lineas: Vec<EntryLine>| {
        accounting::manual_entry(
            &v,
            &JournalEntry {
                date: fecha,
                concept: concepto.into(),
                origin: Origin::Manual,
                lines: lineas,
            },
            datetime!(2025-07-01 9:00 UTC),
        )
        .unwrap();
    };
    let e = |c: i64| Money::from_cents(c);
    // 2T: una compra (21 € de IVA) y una venta (210 €), y la liquidación, que no cuenta.
    asiento(
        date!(2025 - 05 - 10),
        "Compra",
        vec![
            EntryLine::debit(acc("629"), e(10_000)),
            EntryLine::debit(acc("472"), e(2_100)),
            EntryLine::credit(acc("410"), e(12_100)),
        ],
    );
    asiento(
        date!(2025 - 06 - 10),
        "Venta",
        vec![
            EntryLine::debit(acc("430"), e(121_000)),
            EntryLine::credit(acc("700"), e(100_000)),
            EntryLine::credit(acc("477"), e(21_000)),
        ],
    );
    asiento(
        date!(2025 - 06 - 30),
        "Liquidación del IVA del 2T",
        vec![
            EntryLine::debit(acc("477"), e(21_000)),
            EntryLine::credit(acc("472"), e(2_100)),
            EntryLine::credit(acc("4750"), e(18_900)),
        ],
    );
    // 4T: sólo una compra con inversión del sujeto pasivo, que se devenga y se deduce, y la
    // liquidación, que sale a cero (sin la 4700 ni la 4750).
    asiento(
        date!(2025 - 11 - 10),
        "Compra con inversión del sujeto pasivo",
        vec![
            EntryLine::debit(acc("629"), e(10_000)),
            EntryLine::debit(acc("472"), e(2_100)),
            EntryLine::credit(acc("477"), e(2_100)),
            EntryLine::credit(acc("410"), e(10_000)),
        ],
    );
    asiento(
        date!(2025 - 12 - 31),
        "Liquidación del IVA del 4T",
        vec![
            EntryLine::debit(acc("477"), e(2_100)),
            EntryLine::credit(acc("472"), e(2_100)),
        ],
    );
    // 3T: 42 € de IVA soportado en el diario y el 303 deduce 21 €.
    asiento(
        date!(2025 - 08 - 10),
        "Compra",
        vec![
            EntryLine::debit(acc("629"), e(20_000)),
            EntryLine::debit(acc("472"), e(4_200)),
            EntryLine::credit(acc("410"), e(24_200)),
        ],
    );
    let r = revisar_con(
        &v,
        vec![
            con(
                "MODELOS/303-2T.pdf",
                modelo("303", 2025, "2T", json!({"27": "210.00", "45": "21.00"})),
            ),
            con(
                "MODELOS/303-3T.pdf",
                modelo("303", 2025, "3T", json!({"27": "0", "45": "21.00"})),
            ),
            con(
                "MODELOS/303-4T.pdf",
                modelo("303", 2025, "4T", json!({"27": "21.00", "45": "21.00"})),
            ),
        ],
    );
    assert!(
        r.hallazgos
            .iter()
            .any(|h| h.titulo == "El 303 del 4T 2025 cuadra con el diario"),
        "{:?}",
        r.hallazgos.iter().map(|h| &h.titulo).collect::<Vec<_>>()
    );
    let h = hallazgo(&r, "modelos.303_cuadra_libro");
    assert_eq!(h.titulo, "El 303 del 2T 2025 cuadra con el diario");
    assert!(
        h.detalle
            .contains("el total a deducir [45] con la cuenta 472 (21,00 €)"),
        "{}",
        h.detalle
    );
    let h = hallazgo(&r, "modelos.303_diario");
    assert_eq!(h.gravedad, Gravedad::Aviso);
    assert_eq!(
        h.titulo,
        "El 303 del 3T 2025 no casa con las cuentas 472 y 477 del diario"
    );
    assert!(
        h.detalle.contains(
            "el total a deducir [45] con la cuenta 472: 21,00 € en el 303 y 42,00 € en el diario"
        ),
        "{}",
        h.detalle
    );
}

/// Una recibida en dólares (el libro la lleva en euros).
fn en_dolares(nif: Option<&str>, nombre: &str, numero: &str, fecha: &str, total: &str) -> Value {
    let mut l = recibida(nif, nombre, numero, fecha, [total, "0", total]);
    l["factura"]["moneda"] = json!("USD");
    l
}

#[test]
fn el_libro_casa_con_los_pdf_aunque_recorte_o_tape_el_numero() {
    const SOFTWARE: &str = "EU372000041";
    const REGISTRO: &str = "Q1234567D";
    const TELEFONO: &str = "A11111119";
    let periodo = PERIODO_2025;
    let lineas = vec![
        // El libro guarda sólo el final del número, con la O por el 0.
        (
            "CDOE7-0004",
            "14/01",
            SOFTWARE,
            "SOFTWARE DEMO, LLC",
            "15,10",
            "21,0",
            "3,17",
            "15,10",
        ),
        // Registrada unos días después de la fecha de la factura y sin la serie.
        (
            "2025/1234",
            "03/04",
            REGISTRO,
            "REGISTRO DEMO",
            "4,10",
            "21,0",
            "0,86",
            "4,96",
        ),
        // Número tapado con asteriscos.
        (
            "TLF001****",
            "11/05",
            TELEFONO,
            "TELEFONOS DEMO SA",
            "20,00",
            "21,0",
            "4,20",
            "24,20",
        ),
        // En dólares: el libro, en euros y con otro número.
        (
            "0918273645",
            "17/09",
            SOFTWARE,
            "SOFTWARE DEMO, LLC",
            "80,00",
            "21,0",
            "16,80",
            "80,00",
        ),
        // Dos del mismo día e importe: sólo hay el PDF de una.
        (
            "2025/50011",
            "15/12",
            REGISTRO,
            "REGISTRO DEMO",
            "40,00",
            "21,0",
            "8,40",
            "48,40",
        ),
        (
            "2025/50012",
            "15/12",
            REGISTRO,
            "REGISTRO DEMO",
            "40,00",
            "21,0",
            "8,40",
            "48,40",
        ),
        // La misma factura apuntada con el número tapado y con el número entero: el PDF es de
        // la del número entero.
        (
            "TLF002****",
            "12/06",
            TELEFONO,
            "TELEFONOS DEMO SA",
            "20,00",
            "21,0",
            "4,20",
            "24,20",
        ),
        (
            "TLF0021111",
            "12/06",
            TELEFONO,
            "TELEFONOS DEMO SA",
            "20,00",
            "21,0",
            "4,20",
            "24,20",
        ),
    ];
    let libro = (
        "LIBROS/2025/recibidas.xlsx".to_owned(),
        libro_xlsx(
            periodo,
            false,
            &[("Facturas", "Facturas Recibidas", lineas)],
        ),
        None,
    );
    let r = revisar(vec![
        libro,
        con(
            "FACTURAS/2025/T1/software-0004.pdf",
            en_dolares(
                Some(SOFTWARE),
                "Software Demo, LLC",
                "155CD0E7-0004",
                "2025-01-14",
                "18.00",
            ),
        ),
        con(
            "FACTURAS/2025/T1/registro.pdf",
            recibida(
                Some(REGISTRO),
                "Registro Demo",
                "TS9 2025/1234",
                "2025-03-21",
                ["4.10", "0.86", "4.96"],
            ),
        ),
        con(
            "FACTURAS/2025/T2/telefono.pdf",
            recibida(
                Some(TELEFONO),
                "Teléfonos Demo, S.A.",
                "MV-TLF0015577",
                "2025-05-10",
                ["20.00", "4.20", "24.20"],
            ),
        ),
        con(
            "FACTURAS/2025/T3/software-anual.pdf",
            en_dolares(
                Some(SOFTWARE),
                "Software Demo",
                "ab12c-07",
                "2025-09-17",
                "92.00",
            ),
        ),
        con(
            "FACTURAS/2025/T2/telefono-junio.pdf",
            recibida(
                Some(TELEFONO),
                "Teléfonos Demo, S.A.",
                "MV-TLF0021111",
                "2025-06-12",
                ["20.00", "4.20", "24.20"],
            ),
        ),
        con(
            "FACTURAS/2025/T4/registro-50012.pdf",
            recibida(
                Some(REGISTRO),
                "Registro Demo",
                "LG 2025/50012",
                "2025-12-15",
                ["40.00", "8.40", "48.40"],
            ),
        ),
    ]);
    // Sólo falta el PDF de la otra factura del registro del 15/12. La línea tapada es la misma
    // factura que la del número entero, que tiene su PDF: es una línea repetida, no otro PDF que
    // pedir.
    let sin = hallazgo(&r, "libros.sin_pdf");
    assert_eq!(
        sin.titulo,
        "1 factura del libro de recibidas de 2025 sin el PDF en la carpeta"
    );
    assert!(
        sin.detalle.contains("2025/50011") && !sin.detalle.contains("TLF002****"),
        "{}",
        sin.detalle
    );
    assert!(
        hallazgo(&r, "libros.repetidas")
            .detalle
            .contains("TLF002****")
    );
    assert!(
        !hay(&r, "libros.pdf_fuera_del_libro") && !hay(&r, "libros.otras_fuera_del_libro"),
        "{:?}",
        r.hallazgos
            .iter()
            .map(|h| (&h.titulo, &h.ficheros))
            .collect::<Vec<_>>()
    );
}

#[test]
fn el_303_sin_libro_descuenta_la_inversion_del_sujeto_pasivo() {
    // Sin libro: 300 € de la papelería y 50 € de un proveedor de fuera de la UE sin NIF ni IVA
    // (inversión del sujeto pasivo: el 303 los lleva en la [12] y los deduce en la [28]).
    let r = revisar(vec![
        con(
            "FACTURAS/2025/T1/papeleria.pdf",
            recibida(
                Some(PAPELERIA),
                "Papelería Demo SL",
                "P-1",
                "2025-02-10",
                ["300.00", "63.00", "363.00"],
            ),
        ),
        con(
            "FACTURAS/2025/T1/software.pdf",
            recibida(
                None,
                "Software Demo, Inc.",
                "S-1",
                "2025-03-01",
                ["50.00", "0", "50.00"],
            ),
        ),
        con(
            "MODELOS/2025/303-1T.pdf",
            modelo(
                "303",
                2025,
                "1T",
                json!({"12": "50.00", "13": "10.50", "28": "350.00", "29": "73.50"}),
            ),
        ),
    ]);
    assert!(!hay(&r, "modelos.303_falta_factura"), "{:?}", r.hallazgos);
    assert!(!hay(&r, "modelos.303_inversion_sujeto_pasivo"));
    let filas = tabla(&r, "303 frente a las facturas de la carpeta");
    assert!(
        filas
            .iter()
            .any(|f| f[1] == "IVA soportado interior: base [28]+[30]−[12]"
                && f[2] == "300,00 €"
                && f[4] == "0,00 €"),
        "{filas:?}"
    );
    assert!(
        filas
            .iter()
            .any(|f| f[1] == "Inversión del sujeto pasivo: base [12]" && f[4] == "0,00 €"),
        "{filas:?}"
    );
}

#[test]
fn una_factura_sin_nif_de_una_empresa_de_fuera_no_es_interior() {
    // Un proveedor de fuera de la UE sin NIF que cobra IVA: no es una compra interior (no se
    // puede deducir ese IVA); si la gestoría no la lleva a la [12], es eso lo que no casa, no
    // un IVA sin deducir.
    let r = revisar(vec![
        con(
            "FACTURAS/2025/T1/papeleria.pdf",
            recibida(
                Some(PAPELERIA),
                "Papelería Demo SL",
                "P-1",
                "2025-02-10",
                ["300.00", "63.00", "363.00"],
            ),
        ),
        con(
            "FACTURAS/2025/T1/videos.pdf",
            recibida(
                None,
                "Videos Demo Pte. Ltd.",
                "V-1",
                "2025-03-01",
                ["10.00", "2.10", "12.10"],
            ),
        ),
        con(
            "MODELOS/2025/303-1T.pdf",
            modelo("303", 2025, "1T", json!({"28": "300.00", "29": "63.00"})),
        ),
    ]);
    assert!(!hay(&r, "modelos.303_iva_sin_deducir"), "{:?}", r.hallazgos);
    let h = hallazgo(&r, "modelos.303_inversion_sujeto_pasivo");
    assert!(
        h.detalle.contains("0,00 € en la casilla [12]") && h.detalle.contains("suman 10,00 €"),
        "{}",
        h.detalle
    );
}

#[test]
fn el_iva_sin_deducir_con_libro_son_las_facturas_que_no_estan_en_el() {
    // El 303 del 2T cuadra con el libro. En la carpeta hay además una factura con IVA que no
    // está en ninguna hoja del libro, y la comida del libro está en la hoja de no deducibles.
    let imprenta = con(
        "FACTURAS/2025/T2/imprenta.pdf",
        recibida(
            Some("B44444446"),
            "Imprenta Demo SL",
            "I-8",
            "2025-06-12",
            ["80.00", "16.80", "96.80"],
        ),
    );
    let papeleria = con(
        "FACTURAS/2025/T2/papeleria-201.pdf",
        recibida(
            Some(PAPELERIA),
            "Papelería Demo SL",
            "P-201",
            "2025-04-10",
            ["300.00", "63.00", "363.00"],
        ),
    );
    let r = revisar(vec![
        libro_recibidas(),
        libro_emitidas(),
        m303_2t(),
        imprenta,
        papeleria,
    ]);
    assert!(hay(&r, "modelos.303_cuadra_libro"));
    let h = hallazgo(&r, "modelos.303_iva_sin_deducir");
    assert_eq!(
        h.titulo,
        "Facturas con IVA del 2T 2025 que no se dedujeron en el 303"
    );
    for x in [
        "1 factura con IVA español del trimestre no está en ninguna hoja del libro registro",
        "I-8 de Imprenta Demo SL (16,80 € de IVA)",
        "1 factura que tiene en la hoja de IVA no deducible del libro (15,00 €)",
        "no un olvido",
    ] {
        assert!(h.detalle.contains(x), "«{x}» no está en: {}", h.detalle);
    }
    // La de la papelería está en el libro: no sale.
    assert!(!h.detalle.contains("P-201"), "{}", h.detalle);
    assert!(h.ficheros.iter().any(|f| f.ends_with("imprenta.pdf")));
}

#[test]
fn proveedores_de_fuera_con_iva_entre_empresas_y_a_particulares() {
    // Con el NIF de la empresa: entre empresas, la factura debería ir sin IVA.
    let nube = recibida(
        Some("IE1234567T"),
        "Nube Demo Ltd",
        "N-1",
        "2025-03-01",
        ["10.00", "2.10", "12.10"],
    );
    // Sin NIF del emisor, pero de un país de fuera (lo dice la lectura) y con el NIF de la
    // empresa.
    let mut diseno = recibida(
        None,
        "Diseño Demo",
        "D-1",
        "2025-04-02",
        ["50.00", "10.50", "60.50"],
    );
    diseno["factura"]["pais_emisor"] = json!("US");
    // Sin NIF del emisor (con forma social de fuera) y sin el NIF ni el nombre de la empresa:
    // puede ser una venta a un particular.
    let mut videos = recibida(
        None,
        "Videos Demo Pte. Ltd.",
        "V-1",
        "2025-05-03",
        ["20.00", "4.20", "24.20"],
    );
    videos["factura"]["nif_destinatario"] = Value::Null;
    videos["factura"]["nombre_destinatario"] = Value::Null;
    let r = revisar(vec![
        con("FACTURAS/nube.pdf", nube),
        con("FACTURAS/diseno.pdf", diseno),
        con("FACTURAS/videos.pdf", videos),
    ]);
    let b2b = hallazgo(&r, "facturas.extranjero_con_iva");
    assert_eq!(b2b.ficheros, ["FACTURAS/diseno.pdf", "FACTURAS/nube.pdf"]);
    assert!(
        b2b.detalle
            .contains("pide las facturas corregidas, sin IVA"),
        "{}",
        b2b.detalle
    );
    let b2c = hallazgo(&r, "facturas.extranjero_con_iva_particular");
    assert_eq!(b2c.gravedad, Gravedad::Info);
    assert_eq!(b2c.ficheros, ["FACTURAS/videos.pdf"]);
    assert!(
        b2c.detalle
            .contains("Para el proveedor puede ser una venta a un particular")
            && b2c.detalle.contains("Mira si son gastos de la empresa"),
        "{}",
        b2c.detalle
    );
    let filas = tabla(&r, "Proveedores de fuera de España que cobran IVA");
    assert!(
        filas
            .iter()
            .any(|f| f[0] == "Diseño Demo" && f[1] == "US" && f[5] == "Sí"),
        "{filas:?}"
    );
    assert!(
        filas
            .iter()
            .any(|f| f[0] == "Videos Demo Pte. Ltd." && f[5] == "No"),
        "{filas:?}"
    );
}

/// El libro de recibidas de enero a septiembre de 2026 (con una sola hoja).
fn libro_2026(lineas: Vec<Linea>) -> Fichero {
    (
        "LIBROS/2026/recibidas (enero a septiembre).xlsx".into(),
        libro_xlsx(
            "Período De 01 Ene a 30 Sep 2026",
            false,
            &[("Facturas", "Facturas Recibidas", lineas)],
        ),
        None,
    )
}

#[test]
fn el_libro_se_cruza_solo_con_las_facturas_de_su_periodo() {
    let r = revisar(vec![
        libro_2026(vec![
            (
                "P-1",
                "10/01",
                PAPELERIA,
                "PAPELERIA DEMO SL",
                "100",
                "21,0",
                "21",
                "121",
            ),
            (
                "P-9",
                "20/09",
                PAPELERIA,
                "PAPELERIA DEMO SL",
                "100",
                "21,0",
                "21",
                "121",
            ),
        ]),
        // De octubre: el libro llega hasta septiembre.
        con(
            "FACTURAS/2026/T4/asesores.pdf",
            recibida(
                Some(ASESORES),
                "Asesores Demo SL",
                "26/9001",
                "2026-10-02",
                ["120.00", "25.20", "145.20"],
            ),
        ),
        // De agosto, y no está en el libro: ésta sí.
        con(
            "FACTURAS/2026/T3/imprenta.pdf",
            recibida(
                Some("B44444446"),
                "Imprenta Demo SL",
                "I-3",
                "2026-08-12",
                ["80.00", "16.80", "96.80"],
            ),
        ),
    ]);
    let h = hallazgo(&r, "libros.pdf_fuera_del_libro");
    assert_eq!(h.ficheros, ["FACTURAS/2026/T3/imprenta.pdf"]);
    assert!(
        h.detalle.contains("del 01/01/2026 al 30/09/2026"),
        "{}",
        h.detalle
    );
}

#[test]
fn una_factura_apuntada_dos_veces_en_el_libro() {
    // La misma factura del teléfono, una vez con el número tapado y otra entero.
    let r = revisar(vec![libro_2026(vec![
        (
            "TLF003****",
            "12/04",
            "A11111119",
            "TELEFONOS DEMO SA",
            "20,00",
            "21,0",
            "4,20",
            "24,20",
        ),
        (
            "TLF0039999",
            "12/04",
            "A11111119",
            "TELEFONOS DEMO SA",
            "20,00",
            "21,0",
            "4,20",
            "24,20",
        ),
        // Del mismo día e importe pero con otro número: no es la misma.
        (
            "B/101",
            "02/01",
            PAPELERIA,
            "PAPELERIA DEMO SL",
            "150",
            "21,0",
            "31,5",
            "181,5",
        ),
        (
            "B/107",
            "02/01",
            PAPELERIA,
            "PAPELERIA DEMO SL",
            "150",
            "21,0",
            "31,5",
            "181,5",
        ),
    ])]);
    let h = hallazgo(&r, "libros.repetidas");
    assert_eq!(
        h.titulo,
        "Una factura apuntada dos veces en el libro de recibidas de 2026"
    );
    assert!(
        h.detalle
            .contains("la TLF003**** y la TLF0039999 de TELEFONOS DEMO SA (12/04/2026"),
        "{}",
        h.detalle
    );
    assert!(h.detalle.contains("sobran 4,20 € de IVA"), "{}", h.detalle);
    assert!(!h.detalle.contains("B/107"), "{}", h.detalle);
    assert_eq!(h.importe, Some(-Money::from_cents(420)));
    assert!(
        r.pedir
            .iter()
            .any(|p| p.que.contains("que parece apuntada dos veces"))
    );
}

#[test]
fn el_iva_de_la_comida_y_la_bebida_no_se_deduce() {
    let mut compra = recibida(
        Some("A55555551"),
        "Almacenes Demo, S.A.",
        "0012345678",
        "2026-07-22",
        ["270.00", "36.80", "306.80"],
    );
    compra["descripcion"] =
        json!("Factura de Almacenes Demo por compra de alimentación y bebidas (vinos, conservas)");
    let r = revisar(vec![
        libro_2026(vec![
            (
                "0012345678",
                "22/07",
                "A55555551",
                "ALMACENES DEMO SA",
                "20,00",
                "4,0",
                "0,80",
                "306,80",
            ),
            (
                "0012345678",
                "22/07",
                "A55555551",
                "ALMACENES DEMO SA",
                "150,00",
                "10,0",
                "15,00",
                "",
            ),
            (
                "0012345678",
                "22/07",
                "A55555551",
                "ALMACENES DEMO SA",
                "100,00",
                "21,0",
                "21,00",
                "",
            ),
            (
                "C-77",
                "15/08",
                RESTAURANTE,
                "RESTAURANTE DEMO SL",
                "100",
                "10,0",
                "10",
                "110",
            ),
            (
                "P-2",
                "20/08",
                PAPELERIA,
                "PAPELERIA DEMO SL",
                "100",
                "21,0",
                "21",
                "121",
            ),
        ]),
        con("FACTURAS/2026/T3/almacenes.pdf", compra),
    ]);
    let h = hallazgo(&r, "libros.iva_sin_derecho");
    assert_eq!(h.gravedad, Gravedad::Aviso);
    assert_eq!(
        h.titulo,
        "El libro de recibidas de 2026 deduce el IVA de 2 gastos que no dan, o pueden no dar, \
         derecho a deducir"
    );
    for x in [
        "la 0012345678 de ALMACENES DEMO SA (22/07/2026): alimentos o bebidas (art. 96.Uno.3.º \
         LIVA), 36,80 € de IVA",
        "la C-77 de RESTAURANTE DEMO SL (15/08/2026): restaurante, hotel o viaje (art. \
         96.Uno.6.º LIVA), 10,00 € de IVA",
        "Corregido, el 303 de ese trimestre deduce 36,80 € menos.",
    ] {
        assert!(h.detalle.contains(x), "«{x}» no está en: {}", h.detalle);
    }
    assert!(!h.detalle.contains("P-2"), "{}", h.detalle);
    // Sólo cuenta lo que seguro no se deduce (el restaurante depende del gasto).
    assert_eq!(h.importe, Some(-Money::from_cents(3_680)));
}

#[test]
fn el_349_es_mensual_con_mas_de_50000_euros_a_la_ue() {
    let m303 = |periodo: &str, año: i32, ue: &str| {
        con(
            &format!("MODELOS/{año}/303-{periodo}.pdf"),
            modelo("303", año, periodo, json!({ "59": ue })),
        )
    };
    let m349 = |periodo: &str, año: i32, total: &str| {
        con(
            &format!("MODELOS/{año}/349-{periodo}.pdf"),
            modelo("349", año, periodo, json!({ "01": "1", "02": total })),
        )
    };
    let r = revisar(vec![
        // En el 1T 2025 pasa de 50.000 €: ese 349 y los de los cuatro trimestres siguientes son
        // mensuales.
        m303("1T", 2025, "60000.00"),
        m349("1T", 2025, "60000.00"),
        m303("2T", 2025, "10000.00"),
        m349("2T", 2025, "10000.00"),
        // El 2T 2026 ya no (los cuatro anteriores, del 2T 2025 al 1T 2026, no pasan).
        m303("2T", 2026, "10000.00"),
        m349("2T", 2026, "10000.00"),
        // En el 3T 2026 vuelve a pasar: los próximos son mensuales.
        m303("3T", 2026, "70000.00"),
        m349("3T", 2026, "70000.00"),
    ]);
    let mut titulos: Vec<&str> = r
        .hallazgos
        .iter()
        .filter(|h| h.codigo == "modelos.349_mensual")
        .map(|h| h.titulo.as_str())
        .collect();
    titulos.sort_unstable();
    assert_eq!(
        titulos,
        [
            "El 349 del 1T 2025 tenía que ser mensual",
            "El 349 del 2T 2025 tenía que ser mensual",
            "El 349 del 3T 2026 tenía que ser mensual",
        ]
    );
    let h = r
        .hallazgos
        .iter()
        .find(|h| h.titulo == "El 349 del 2T 2025 tenía que ser mensual")
        .unwrap();
    assert!(
        h.detalle.contains("En el 1T 2025") && h.detalle.contains("art. 81.2.2.º RIVA"),
        "{}",
        h.detalle
    );
    let h = hallazgo(&r, "modelos.349_mensual_proximos");
    assert_eq!(h.titulo, "El 349 se presenta cada mes hasta el 3T 2027");
}

#[test]
fn las_emitidas_sin_iva_a_la_ue_dicen_por_que_no_lo_llevan() {
    let emitida = |numero: &str| {
        json!({
            "tipo": "factura_emitida",
            "descripcion": format!("Factura {numero} a un cliente de Francia"),
            "factura": {
                "nif_emisor": PROPIO, "nombre_emisor": "Empresa Demo SL",
                "nif_destinatario": PIEZAS_UE, "nombre_destinatario": "Pieces Demo SARL",
                "numero": numero, "fecha": "2025-03-10",
                "base": "1000.00", "iva": "0", "total": "1000.00"
            }
        })
    };
    let r = revisar(vec![
        (
            "INGRESOS/E-1.pdf".into(),
            pdf("Factura E-1. Servicios de consultoría. Base 1.000,00 €. Total 1.000,00 €."),
            Some(emitida("E-1")),
        ),
        (
            "INGRESOS/E-2.pdf".into(),
            pdf(
                "Factura E-2. Servicios de consultoría. Total 1.000,00 €. Inversión del sujeto pasivo.",
            ),
            Some(emitida("E-2")),
        ),
    ]);
    let h = hallazgo(&r, "emitidas.sin_mencion");
    assert_eq!(h.ficheros, ["INGRESOS/E-1.pdf"]);
    assert!(
        h.detalle
            .contains("art. 6.1.m del Reglamento de facturación"),
        "{}",
        h.detalle
    );
}

#[test]
fn un_trimestre_con_solo_inversion_del_sujeto_pasivo() {
    // En el 3T sólo hay una compra a un proveedor de fuera de la UE (la papelería del 3T se
    // registró en octubre).
    let r = revisar(vec![
        libro_2026(vec![
            (
                "US-30",
                "05/08",
                "",
                "SOFTWARE DEMO INC",
                "40",
                "21,0",
                "8,4",
                "40",
            ),
            (
                "P-31",
                "01/10",
                PAPELERIA,
                "PAPELERIA DEMO SL",
                "100",
                "21,0",
                "21",
                "121",
            ),
        ]),
        con(
            "MODELOS/2026/303-3T.pdf",
            modelo(
                "303",
                2026,
                "3T",
                json!({"12": "40.00", "13": "8.40", "28": "40.00", "29": "8.40"}),
            ),
        ),
    ]);
    let h = hallazgo(&r, "modelos.303_cuadra_libro");
    assert!(
        h.detalle
            .contains("[28]+[30] (40,00 €: todo de inversión del sujeto pasivo)"),
        "{}",
        h.detalle
    );
}

#[test]
fn una_inversion_del_sujeto_pasivo_apuntada_dos_veces_no_cambia_el_resultado() {
    let r = revisar(vec![libro_2026(vec![
        (
            "2026-0040",
            "05/03",
            "",
            "SOFTWARE DEMO INC",
            "40",
            "21,0",
            "8,4",
            "40",
        ),
        (
            "INV-2026-0040",
            "05/03",
            "",
            "SOFTWARE DEMO INC",
            "40",
            "21,0",
            "8,4",
            "40",
        ),
    ])]);
    let h = hallazgo(&r, "libros.repetidas");
    assert!(
        h.detalle.contains("la 2026-0040 y la INV-2026-0040")
            && h.detalle.contains("no cambie el resultado"),
        "{}",
        h.detalle
    );
    assert_eq!(h.importe, None);
}

#[test]
fn que_pedir_con_una_sola_factura_del_libro_sin_pdf() {
    let r = revisar(vec![libro_2026(vec![(
        "P-1",
        "10/01",
        PAPELERIA,
        "PAPELERIA DEMO SL",
        "100",
        "21,0",
        "21",
        "121",
    )])]);
    assert!(
        r.pedir
            .iter()
            .any(|p| p.que
                == "La factura recibida de 2026 que está en el libro pero no en la carpeta"),
        "{:?}",
        r.pedir
    );
}

#[test]
fn un_303_sin_casillas_no_se_cruza() {
    use taxpain_app::presentado_aeat::{self, Answer, Presented};
    // Los 303 del 1T y del 2T están en TaxPain por «Comprobar lo presentado», pero aún sin su PDF:
    // no se sabe qué declaran. El 2T lo cubre el libro; el 1T tiene una factura en PDF.
    let v = vault();
    let presentado = |periodo: &str, n: u32| Presented {
        model: "303".into(),
        year: 2025,
        period: periodo.into(),
        csv: format!("DEMOCSV00000000{n}"),
        file_number: String::new(),
        receipt: format!("303202520000{n}"),
        previous_receipt: None,
        presented_at: "2025-07-15T10:00:00".into(),
    };
    let a = Answer {
        models: vec!["303".into()],
        list: vec![presentado("1T", 1), presentado("2T", 2)],
        ..Answer::default()
    };
    presentado_aeat::record(&v, PROPIO, 2025, &a, datetime!(2026-10-27 10:00 UTC)).unwrap();
    let papeleria = con(
        "FACTURAS/2025/T1/papeleria.pdf",
        recibida(
            Some(PAPELERIA),
            "Papelería Demo SL",
            "P-1",
            "2025-02-10",
            ["300.00", "63.00", "363.00"],
        ),
    );
    let r = revisar_con(&v, vec![libro_recibidas(), libro_emitidas(), papeleria]);
    for codigo in [
        "modelos.303_libro",
        "modelos.303_cuadra_libro",
        "modelos.303_falta_factura",
        "modelos.303_iva_sin_deducir",
    ] {
        assert!(
            !hay(&r, codigo),
            "{codigo}: {:?}",
            r.hallazgos
                .iter()
                .map(|h| (&h.titulo, &h.detalle))
                .collect::<Vec<_>>()
        );
    }
    assert!(
        !r.pedir.iter().any(|p| p.que.contains("303 del")),
        "{:?}",
        r.pedir
    );
}

/// Un libro de 2025 de una hoja.
fn libro_2025(ruta: &str, emitidas: bool, lineas: Vec<Linea>) -> Fichero {
    (
        ruta.to_owned(),
        libro_xlsx(
            PERIODO_2025,
            emitidas,
            &[(
                "Facturas",
                if emitidas {
                    "Facturas Expedidas"
                } else {
                    "Facturas Recibidas"
                },
                lineas,
            )],
        ),
        None,
    )
}

#[test]
fn las_rectificativas_y_las_importaciones_van_en_sus_casillas() {
    // Una rectificativa recibida en la [40]/[41], una importación (el DUA de la aduana) en la
    // [32]/[33] y una rectificativa emitida en la [14]/[15]: el 303 casa con los libros.
    let recibidas = libro_2025(
        "LIBROS/2025/recibidas.xlsx",
        false,
        vec![
            (
                "P-1",
                "10/02",
                PAPELERIA,
                "PAPELERIA DEMO SL",
                "500",
                "21,0",
                "105",
                "605",
            ),
            (
                "R-1",
                "20/03",
                PAPELERIA,
                "PAPELERIA DEMO SL",
                "-100",
                "21,0",
                "-21",
                "-121",
            ),
            (
                "25ES00281011234567",
                "25/02",
                "Q2826000H",
                "AGENCIA TRIBUTARIA (ADUANA)",
                "1000",
                "21,0",
                "210",
                "1210",
            ),
        ],
    );
    let emitidas = libro_2025(
        "LIBROS/2025/emitidas.xlsx",
        true,
        vec![
            (
                "2025-1",
                "10/02",
                CLIENTE,
                "CLIENTE DEMO SL",
                "1000",
                "21,0",
                "210",
                "1210",
            ),
            (
                "R2025-1",
                "20/03",
                CLIENTE,
                "CLIENTE DEMO SL",
                "-200",
                "21,0",
                "-42",
                "-242",
            ),
        ],
    );
    let m303 = con(
        "MODELOS/2025/303-1T.pdf",
        modelo(
            "303",
            2025,
            "1T",
            json!({
                "07": "1000.00", "09": "210.00", "14": "-200.00", "15": "-42.00", "27": "168.00",
                "28": "500.00", "29": "105.00", "32": "1000.00", "33": "210.00",
                "40": "-100.00", "41": "-21.00", "45": "294.00"
            }),
        ),
    );
    let r = revisar(vec![recibidas, emitidas, m303]);
    assert!(
        !hay(&r, "modelos.303_libro"),
        "{:?}",
        r.hallazgos
            .iter()
            .map(|h| (&h.titulo, &h.detalle))
            .collect::<Vec<_>>()
    );
    let h = hallazgo(&r, "modelos.303_cuadra_libro");
    for x in [
        "la base de IVA soportado [28]+[30]+[32]+[40] (1.400,00 €)",
        "la base de IVA devengado (800,00 €)",
    ] {
        assert!(h.detalle.contains(x), "«{x}» no está en: {}", h.detalle);
    }
    let filas = tabla(&r, "303 frente a los libros registro");
    for concepto in [
        "IVA soportado interior, importaciones e inversión del sujeto pasivo: cuota \
         [29]+[31]+[33]+[41]",
        "IVA devengado: base [01]+[04]+[153]+[07]+[14]",
        "IVA devengado: cuota [03]+[06]+[155]+[09]+[15]",
    ] {
        assert!(filas.iter().any(|f| f[1] == concepto), "{filas:?}");
    }
}

#[test]
fn las_ventas_al_cero_por_ciento_no_cuentan() {
    // En 2024 los alimentos básicos iban al 0 % ([150]): no llevan cuota y el libro (o las
    // facturas) no distingue una venta al 0 % de una exenta. Se comparan las ventas con IVA.
    let venta = |numero: &str, base: &str, iva: &str, total: &str| {
        json!({
            "tipo": "factura_emitida",
            "descripcion": format!("Factura {numero} a Cliente Demo"),
            "factura": {
                "nif_emisor": PROPIO, "nombre_emisor": "Empresa Demo SL",
                "nif_destinatario": CLIENTE, "nombre_destinatario": "Cliente Demo SL",
                "numero": numero, "fecha": "2024-02-10",
                "base": base, "iva": iva, "total": total
            }
        })
    };
    let m303 = |ruta: &str| {
        con(
            ruta,
            modelo(
                "303",
                2024,
                "1T",
                json!({"150": "1000.00", "152": "0", "04": "500.00", "06": "50.00", "27": "50.00"}),
            ),
        )
    };
    // Con el libro de emitidas.
    let libro = (
        "LIBROS/2024/emitidas.xlsx".to_owned(),
        libro_xlsx(
            "Período De 01 Ene a 31 Mar 2024",
            true,
            &[(
                "Facturas",
                "Facturas Expedidas",
                vec![
                    (
                        "2024-1",
                        "10/02",
                        CLIENTE,
                        "CLIENTE DEMO SL",
                        "1000",
                        "0,0",
                        "0",
                        "1000",
                    ),
                    (
                        "2024-2",
                        "15/02",
                        CLIENTE,
                        "CLIENTE DEMO SL",
                        "500",
                        "10,0",
                        "50",
                        "550",
                    ),
                ],
            )],
        ),
        None,
    );
    let r = revisar(vec![libro, m303("MODELOS/2024/303-1T.pdf")]);
    assert!(!hay(&r, "modelos.303_libro"), "{:?}", r.hallazgos);
    // Sin libro, con las facturas en PDF.
    let r = revisar(vec![
        con(
            "INGRESOS/2024-1.pdf",
            venta("2024-1", "1000.00", "0", "1000.00"),
        ),
        con(
            "INGRESOS/2024-2.pdf",
            venta("2024-2", "500.00", "50.00", "550.00"),
        ),
        m303("MODELOS/2024/303-1T-bis.pdf"),
    ]);
    assert!(!hay(&r, "modelos.303_devengado"), "{:?}", r.hallazgos);
}

#[test]
fn sin_libro_la_importacion_va_en_la_32() {
    let r = revisar(vec![
        con(
            "FACTURAS/2025/T1/papeleria.pdf",
            recibida(
                Some(PAPELERIA),
                "Papelería Demo SL",
                "P-1",
                "2025-02-10",
                ["300.00", "63.00", "363.00"],
            ),
        ),
        con(
            "FACTURAS/2025/T1/dua.pdf",
            recibida(
                Some("Q2826000H"),
                "Agencia Tributaria (Aduana)",
                "25ES00281011234567",
                "2025-02-25",
                ["1000.00", "210.00", "1210.00"],
            ),
        ),
        con(
            "MODELOS/2025/303-1T.pdf",
            modelo(
                "303",
                2025,
                "1T",
                json!({"28": "300.00", "29": "63.00", "32": "1000.00", "33": "210.00"}),
            ),
        ),
    ]);
    assert!(!hay(&r, "modelos.303_iva_sin_deducir"), "{:?}", r.hallazgos);
    assert!(!hay(&r, "modelos.303_falta_factura"), "{:?}", r.hallazgos);
}

#[test]
fn sin_libro_el_303_que_deduce_el_iva_de_un_proveedor_de_fuera() {
    // Un proveedor de fuera de la UE (con el NIF-IVA de la ventanilla única) cobra IVA español y
    // la gestoría lo deduce: la factura está en la carpeta, no falta ninguna.
    let r = revisar(vec![
        con(
            "FACTURAS/2025/T1/papeleria.pdf",
            recibida(
                Some(PAPELERIA),
                "Papelería Demo SL",
                "P-1",
                "2025-02-10",
                ["300.00", "63.00", "363.00"],
            ),
        ),
        con(
            "FACTURAS/2025/T1/nube.pdf",
            recibida(
                Some("EU372000041"),
                "Nube Demo Inc.",
                "N-1",
                "2025-03-01",
                ["100.00", "21.00", "121.00"],
            ),
        ),
        con(
            "MODELOS/2025/303-1T.pdf",
            modelo("303", 2025, "1T", json!({"28": "400.00", "29": "84.00"})),
        ),
    ]);
    assert!(!hay(&r, "modelos.303_falta_factura"), "{:?}", r.hallazgos);
    assert!(
        !hay(&r, "modelos.303_inversion_sujeto_pasivo"),
        "{:?}",
        r.hallazgos
    );
    assert!(
        !r.pedir.iter().any(|p| p.que.contains("deducidos de más")),
        "{:?}",
        r.pedir
    );
    let h = hallazgo(&r, "modelos.303_iva_extranjero");
    assert_eq!(h.gravedad, Gravedad::Info);
    assert_eq!(
        h.titulo,
        "El 303 del 1T 2025 deduce el IVA de 1 factura de un proveedor de fuera de España"
    );
    for x in [
        "N-1 de Nube Demo Inc. (21,00 € de IVA)",
        "art. 94.Tres LIVA",
    ] {
        assert!(h.detalle.contains(x), "«{x}» no está en: {}", h.detalle);
    }
    // Si no lo deduce, cuadra sin ella y no se dice nada.
    let r = revisar(vec![
        con(
            "FACTURAS/2025/T1/papeleria.pdf",
            recibida(
                Some(PAPELERIA),
                "Papelería Demo SL",
                "P-1",
                "2025-02-10",
                ["300.00", "63.00", "363.00"],
            ),
        ),
        con(
            "FACTURAS/2025/T1/nube.pdf",
            recibida(
                Some("EU372000041"),
                "Nube Demo Inc.",
                "N-1",
                "2025-03-01",
                ["100.00", "21.00", "121.00"],
            ),
        ),
        con(
            "MODELOS/2025/303-1T.pdf",
            modelo(
                "303",
                2025,
                "1T",
                json!({"12": "100.00", "13": "21.00", "28": "400.00", "29": "84.00"}),
            ),
        ),
    ]);
    for codigo in [
        "modelos.303_falta_factura",
        "modelos.303_iva_sin_deducir",
        "modelos.303_iva_extranjero",
        "modelos.303_inversion_sujeto_pasivo",
    ] {
        assert!(!hay(&r, codigo), "{codigo}: {:?}", r.hallazgos);
    }
}

#[test]
fn sin_libro_la_12_tambien_lleva_la_inversion_del_sujeto_pasivo_interior() {
    // Una obra de un subcontratista español con inversión del sujeto pasivo (art. 84.Uno.2.º f
    // LIVA): va sin IVA y el 303 la lleva en la [12].
    let r = revisar(vec![
        con(
            "FACTURAS/2025/T1/obra.pdf",
            recibida(
                Some("B44444446"),
                "Reformas Demo SL",
                "O-1",
                "2025-02-10",
                ["1000.00", "0", "1000.00"],
            ),
        ),
        con(
            "MODELOS/2025/303-1T.pdf",
            modelo(
                "303",
                2025,
                "1T",
                json!({"12": "1000.00", "13": "210.00", "28": "1000.00", "29": "210.00"}),
            ),
        ),
    ]);
    assert!(
        !hay(&r, "modelos.303_inversion_sujeto_pasivo"),
        "{:?}",
        r.hallazgos
    );
}

/// Un asiento en el diario de TaxPain.
fn asiento(
    v: &Vault,
    fecha: time::Date,
    concepto: &str,
    origen: taxpain_core::accounting::Origin,
    lineas: Vec<(&str, i64, i64)>,
) {
    use taxpain_core::accounting::account::acc;
    use taxpain_core::accounting::{EntryLine, JournalEntry};
    let lines = lineas
        .into_iter()
        .map(|(cuenta, debe, haber)| {
            if debe > 0 {
                EntryLine::debit(acc(cuenta), Money::from_cents(debe))
            } else {
                EntryLine::credit(acc(cuenta), Money::from_cents(haber))
            }
        })
        .collect();
    taxpain_app::accounting::manual_entry(
        v,
        &JournalEntry {
            date: fecha,
            concept: concepto.into(),
            origin: origen,
            lines,
        },
        datetime!(2026-02-01 9:00 UTC),
    )
    .unwrap();
}

#[test]
fn el_diario_resumido_por_meses_con_la_liquidacion_dentro() {
    use taxpain_core::accounting::Origin;
    // Un diario resumido del gestor: un asiento por mes, con cuentas de tres cifras, y la
    // liquidación del trimestre dentro del asiento del último mes (la 472 abonada, la 477
    // cargada y el resultado en la 470 o en la 4750).
    let v = vault();
    let mes = |fecha: time::Date, lineas: Vec<(&str, i64, i64)>| {
        asiento(
            &v,
            fecha,
            "Diario resumido del gestor",
            Origin::Gestor(1),
            lineas,
        );
    };
    mes(
        date!(2025 - 01 - 31),
        vec![
            ("629", 35_000, 0),
            ("472", 7_000, 0),
            ("475", 0, 30_000),
            ("477", 0, 100),
            ("410", 0, 11_900),
        ],
    );
    mes(
        date!(2025 - 02 - 28),
        vec![
            ("629", 290_000, 0),
            ("472", 60_000, 0),
            ("477", 0, 100),
            ("410", 0, 349_900),
        ],
    );
    // Marzo: 40 € de IVA del mes y la liquidación del 1T (710 € a compensar en la 470).
    mes(
        date!(2025 - 03 - 31),
        vec![
            ("629", 20_000, 0),
            ("472", 4_000, 0),
            ("477", 300, 0),
            ("470", 70_700, 0),
            ("472", 0, 71_000),
            ("477", 0, 100),
            ("410", 0, 23_900),
        ],
    );
    // 4T: 30 € de IVA cada mes; en diciembre, 2.100 € de IVA de una venta y la liquidación,
    // a ingresar en la 4750.
    mes(
        date!(2025 - 10 - 31),
        vec![("629", 15_000, 0), ("472", 3_000, 0), ("410", 0, 18_000)],
    );
    mes(
        date!(2025 - 11 - 30),
        vec![("629", 15_000, 0), ("472", 3_000, 0), ("410", 0, 18_000)],
    );
    mes(
        date!(2025 - 12 - 31),
        vec![
            ("629", 15_000, 0),
            ("472", 3_000, 0),
            ("430", 1_210_000, 0),
            ("477", 210_000, 0),
            ("472", 0, 9_000),
            ("4750", 0, 201_000),
            ("700", 0, 1_000_000),
            ("477", 0, 210_000),
            ("410", 0, 18_000),
        ],
    );
    let r = revisar_con(
        &v,
        vec![
            con(
                "MODELOS/303-1T.pdf",
                modelo("303", 2025, "1T", json!({"27": "3.00", "45": "710.00"})),
            ),
            con(
                "MODELOS/303-4T.pdf",
                modelo("303", 2025, "4T", json!({"27": "2100.00", "45": "90.00"})),
            ),
        ],
    );
    assert!(
        !hay(&r, "modelos.303_diario"),
        "{:?}",
        r.hallazgos
            .iter()
            .filter(|h| h.codigo == "modelos.303_diario")
            .map(|h| &h.detalle)
            .collect::<Vec<_>>()
    );
    let titulos: Vec<&str> = r.hallazgos.iter().map(|h| h.titulo.as_str()).collect();
    for t in [
        "El 303 del 1T 2025 cuadra con el diario",
        "El 303 del 4T 2025 cuadra con el diario",
    ] {
        assert!(titulos.contains(&t), "{titulos:?}");
    }
}

#[test]
fn el_cierre_y_la_apertura_importados_no_son_iva_del_trimestre() {
    use taxpain_core::accounting::Origin;
    // La liquidación del 4T se asentó en enero: el cierre del 31/12 salda la 472 y la 477 y la
    // apertura del 1/1 las reabre. El diario es importado (no se sabe por su origen qué asiento
    // es cada uno, sí por su concepto).
    let v = vault();
    let imp = |fecha: time::Date, concepto: &str, lineas: Vec<(&str, i64, i64)>| {
        asiento(&v, fecha, concepto, Origin::Importado(1), lineas);
    };
    imp(
        date!(2025 - 11 - 10),
        "Compra",
        vec![("629", 10_000, 0), ("472", 2_100, 0), ("410", 0, 12_100)],
    );
    imp(
        date!(2025 - 12 - 10),
        "Venta",
        vec![("430", 121_000, 0), ("700", 0, 100_000), ("477", 0, 21_000)],
    );
    imp(
        date!(2025 - 12 - 31),
        "Asiento de cierre",
        vec![
            ("477", 21_000, 0),
            ("410", 12_100, 0),
            ("129", 90_000, 0),
            ("472", 0, 2_100),
            ("430", 0, 121_000),
        ],
    );
    imp(
        date!(2026 - 01 - 01),
        "Asiento de apertura",
        vec![
            ("472", 2_100, 0),
            ("430", 121_000, 0),
            ("477", 0, 21_000),
            ("410", 0, 12_100),
            ("129", 0, 90_000),
        ],
    );
    imp(
        date!(2026 - 01 - 20),
        "Liquidación del IVA del 4T 2025",
        vec![("477", 21_000, 0), ("472", 0, 2_100), ("4750", 0, 18_900)],
    );
    let r = revisar_con(
        &v,
        vec![
            con(
                "MODELOS/303-4T-2025.pdf",
                modelo("303", 2025, "4T", json!({"27": "210.00", "45": "21.00"})),
            ),
            con(
                "MODELOS/303-1T-2026.pdf",
                modelo("303", 2026, "1T", json!({"27": "0", "45": "0"})),
            ),
        ],
    );
    assert!(!hay(&r, "modelos.303_diario"), "{:?}", r.hallazgos);
    assert!(
        r.hallazgos
            .iter()
            .any(|h| h.titulo == "El 303 del 4T 2025 cuadra con el diario"),
        "{:?}",
        r.hallazgos.iter().map(|h| &h.titulo).collect::<Vec<_>>()
    );
}

#[test]
fn un_trimestre_sin_la_472_en_el_diario_no_se_cruza_con_ella() {
    use taxpain_core::accounting::Origin;
    // TaxPain sólo tiene las ventas; las compras las lleva la gestoría.
    let v = vault();
    asiento(
        &v,
        date!(2025 - 05 - 10),
        "Venta",
        Origin::Manual,
        vec![("430", 121_000, 0), ("700", 0, 100_000), ("477", 0, 21_000)],
    );
    let r = revisar_con(
        &v,
        vec![con(
            "MODELOS/303-2T.pdf",
            modelo(
                "303",
                2025,
                "2T",
                json!({"27": "210.00", "28": "300.00", "29": "63.00", "45": "63.00"}),
            ),
        )],
    );
    assert!(!hay(&r, "modelos.303_diario"), "{:?}", r.hallazgos);
    let h = hallazgo(&r, "modelos.303_cuadra_libro");
    assert_eq!(h.titulo, "El 303 del 2T 2025 cuadra con el diario");
    assert!(
        h.detalle
            .contains("el total devengado [27] con la cuenta 477 (210,00 €)")
            && !h.detalle.contains("[45]"),
        "{}",
        h.detalle
    );
}

#[test]
fn una_venta_y_una_compra_con_el_mismo_numero_no_son_autorrepercusion() {
    // Dos empresas vinculadas que se facturan con la misma numeración: la 2025-001 emitida y la
    // 2025-001 recibida del mismo NIF son una venta y una compra interior, no la
    // autorrepercusión de una compra (que va en el libro de recibidas sin la cuota en el total y
    // con la misma base y cuota).
    let emitidas = libro_2025(
        "LIBROS/2025/emitidas.xlsx",
        true,
        vec![
            (
                "2025-001",
                "15/04",
                CLIENTE,
                "CLIENTE DEMO SL",
                "600",
                "21,0",
                "126",
                "726",
            ),
            (
                "2025-002",
                "15/05",
                CLIENTE,
                "CLIENTE DEMO SL",
                "400",
                "21,0",
                "84",
                "484",
            ),
        ],
    );
    let recibidas = libro_2025(
        "LIBROS/2025/recibidas.xlsx",
        false,
        vec![
            (
                "2025-001",
                "20/04",
                CLIENTE,
                "CLIENTE DEMO SL",
                "200",
                "21,0",
                "42",
                "242",
            ),
            (
                "2025-002",
                "20/05",
                CLIENTE,
                "CLIENTE DEMO SL",
                "200",
                "21,0",
                "42",
                "242",
            ),
        ],
    );
    let m303 = con(
        "MODELOS/2025/303-2T.pdf",
        modelo(
            "303",
            2025,
            "2T",
            json!({"07": "1000.00", "09": "210.00", "27": "210.00",
                   "28": "400.00", "29": "84.00", "45": "84.00"}),
        ),
    );
    let r = revisar(vec![emitidas, recibidas, m303]);
    assert!(!hay(&r, "libros.autorrepercusion"), "{:?}", r.hallazgos);
    assert!(!hay(&r, "modelos.303_libro"), "{:?}", r.hallazgos);
    let h = hallazgo(&r, "modelos.303_cuadra_libro");
    assert!(
        h.detalle.contains("la base de IVA devengado (1.000,00 €)"),
        "{}",
        h.detalle
    );
}

#[test]
fn una_sola_linea_de_autorrepercusion_en_singular() {
    let compra = vec![(
        "US-17",
        "05/04",
        "",
        "SOFTWARE DEMO INC",
        "80",
        "21,0",
        "16,8",
        "80",
    )];
    let recibidas = libro_2025("LIBROS/2025/recibidas.xlsx", false, compra);
    let emitidas = libro_2025(
        "LIBROS/2025/emitidas.xlsx",
        true,
        vec![
            (
                "US-17",
                "05/04",
                "",
                "SOFTWARE DEMO INC",
                "80",
                "21,0",
                "16,8",
                "80",
            ),
            (
                "2025-1",
                "15/04",
                CLIENTE,
                "CLIENTE DEMO SL",
                "600",
                "21,0",
                "126",
                "726",
            ),
        ],
    );
    let r = revisar(vec![recibidas, emitidas]);
    let h = hallazgo(&r, "libros.autorrepercusion");
    assert!(
        h.detalle
            .starts_with("Una línea es el IVA autorrepercutido de una compra")
            && h.detalle.contains("quita esa línea de las emitidas"),
        "{}",
        h.detalle
    );
}

#[test]
fn la_inversion_del_sujeto_pasivo_no_deducible_va_en_la_12() {
    // La compra a un proveedor de fuera de la UE está en la hoja de IVA no deducible, con el
    // total sin la cuota: su IVA se devenga en la [12] aunque no se deduzca en la [28].
    let bytes = libro_xlsx(
        PERIODO_2025,
        false,
        &[
            (
                "Facturas",
                "Operaciones Interiores - Facturas Recibidas",
                vec![(
                    "P-201",
                    "10/04",
                    PAPELERIA,
                    "PAPELERIA DEMO SL",
                    "300",
                    "21,0",
                    "63",
                    "363",
                )],
            ),
            (
                "No deducibles",
                "Operaciones I.V.A. no Deducible",
                vec![(
                    "US-9",
                    "15/06",
                    "",
                    "SOFTWARE DEMO INC",
                    "50",
                    "21,0",
                    "10,5",
                    "50",
                )],
            ),
        ],
    );
    let libro = ("LIBROS/2025/recibidas.xlsx".to_owned(), bytes, None);
    let m303 = con(
        "MODELOS/2025/303-2T.pdf",
        modelo(
            "303",
            2025,
            "2T",
            json!({"12": "50.00", "13": "10.50", "27": "10.50",
                   "28": "300.00", "29": "63.00", "45": "63.00"}),
        ),
    );
    let r = revisar(vec![libro, m303]);
    assert!(!hay(&r, "modelos.303_libro"), "{:?}", r.hallazgos);
    let h = hallazgo(&r, "modelos.303_cuadra_libro");
    assert!(
        h.detalle
            .contains("la inversión del sujeto pasivo [12] (50,00 €)")
            && h.detalle.contains("[28]+[30] (300,00 €)"),
        "{}",
        h.detalle
    );
}

#[test]
fn las_bebidas_y_la_comida_que_compra_un_bar_para_venderlas_se_deducen() {
    // Un bar: compra bebidas y comida para servirlas y vende al 10 %. Ese IVA se deduce (art.
    // 96.Dos.2.º LIVA): no es un aviso ni hay nada que pagar.
    let recibidas = libro_2025(
        "LIBROS/2025/recibidas.xlsx",
        false,
        vec![
            (
                "B-1",
                "10/04",
                "A55555551",
                "BEBIDAS DEMO SA",
                "1000",
                "21,0",
                "210",
                "1210",
            ),
            (
                "C-1",
                "12/04",
                RESTAURANTE,
                "CERVEZAS DEMO SL",
                "500",
                "21,0",
                "105",
                "605",
            ),
            (
                "D-1",
                "15/04",
                ASESORES,
                "DISTRIBUCIONES ALIMENTACION DEMO SL",
                "800",
                "10,0",
                "80",
                "880",
            ),
        ],
    );
    let emitidas = libro_2025(
        "LIBROS/2025/emitidas.xlsx",
        true,
        vec![(
            "T-1",
            "30/04",
            "",
            "VENTAS DEL BAR (TICKETS)",
            "6000",
            "10,0",
            "600",
            "6600",
        )],
    );
    let m303 = con(
        "MODELOS/2025/303-2T.pdf",
        modelo(
            "303",
            2025,
            "2T",
            json!({"04": "6000.00", "06": "600.00", "27": "600.00",
                   "28": "2300.00", "29": "395.00", "45": "395.00"}),
        ),
    );
    let r = revisar(vec![recibidas, emitidas, m303]);
    let avisos: Vec<&Hallazgo> = r
        .hallazgos
        .iter()
        .filter(|h| h.codigo == "libros.iva_sin_derecho")
        .collect();
    assert!(
        avisos
            .iter()
            .all(|h| h.gravedad == Gravedad::Info && h.importe.is_none()),
        "{avisos:?}"
    );
    if let Some(h) = avisos.first() {
        assert!(h.detalle.contains("art. 96.Dos.2.º"), "{}", h.detalle);
    }
}

#[test]
fn las_entradas_de_un_blog_y_la_cesta_de_la_compra_no_son_ocio_ni_regalos() {
    let mut blog = recibida(
        Some(PAPELERIA),
        "Contenidos Demo SL",
        "CD-7",
        "2025-04-10",
        ["300.00", "63.00", "363.00"],
    );
    blog["descripcion"] =
        json!("Factura de Contenidos Demo por la redacción de 6 entradas del blog");
    let mut web = recibida(
        Some(ASESORES),
        "Desarrollos Demo SL",
        "DW-3",
        "2025-05-10",
        ["1000.00", "210.00", "1210.00"],
    );
    web["descripcion"] = json!(
        "Factura de Desarrollos Demo por la programación de la cesta de la compra y el menú de \
         la tienda online, y de la web de un cine"
    );
    let recibidas = libro_2025(
        "LIBROS/2025/recibidas.xlsx",
        false,
        vec![
            (
                "CD-7",
                "10/04",
                PAPELERIA,
                "CONTENIDOS DEMO SL",
                "300",
                "21,0",
                "63",
                "363",
            ),
            (
                "DW-3",
                "10/05",
                ASESORES,
                "DESARROLLOS DEMO SL",
                "1000",
                "21,0",
                "210",
                "1210",
            ),
        ],
    );
    let r = revisar(vec![
        recibidas,
        con("FACTURAS/2025/blog.pdf", blog),
        con("FACTURAS/2025/web.pdf", web),
    ]);
    assert!(
        !hay(&r, "libros.iva_sin_derecho"),
        "{:?}",
        r.hallazgos
            .iter()
            .filter(|h| h.codigo == "libros.iva_sin_derecho")
            .map(|h| &h.detalle)
            .collect::<Vec<_>>()
    );
}

#[test]
fn la_copia_sin_numero_de_una_factura_del_libro_no_esta_fuera_de_el() {
    // Dos PDF de la P-201: uno leído entero y una copia escaneada leída sin el número.
    let libro = libro_2025(
        "LIBROS/2025/recibidas.xlsx",
        false,
        vec![(
            "P-201",
            "10/04",
            PAPELERIA,
            "PAPELERIA DEMO SL",
            "300",
            "21,0",
            "63",
            "363",
        )],
    );
    let entera = con(
        "FACTURAS/2025/T2/papeleria-201.pdf",
        recibida(
            Some(PAPELERIA),
            "Papelería Demo SL",
            "P-201",
            "2025-04-10",
            ["300.00", "63.00", "363.00"],
        ),
    );
    let mut copia = recibida(
        Some(PAPELERIA),
        "Papelería Demo SL",
        "P-201",
        "2025-04-10",
        ["300.00", "63.00", "363.00"],
    );
    copia["factura"]["numero"] = Value::Null;
    let copia = con("FACTURAS/2025/T2/escaneo.pdf", copia);
    // La copia va antes en la carpeta: el orden no cambia nada.
    let r = revisar(vec![libro, copia, entera]);
    assert!(!hay(&r, "libros.pdf_fuera_del_libro"), "{:?}", r.hallazgos);
    assert!(!hay(&r, "libros.sin_pdf"), "{:?}", r.hallazgos);
}

#[test]
fn la_linea_repetida_de_una_factura_con_pdf_no_pide_otro_pdf() {
    // La misma factura del teléfono apuntada con el número tapado y con el número entero; en la
    // carpeta está dos veces (una copia con el número tapado en el nombre del fichero).
    let libro = libro_2025(
        "LIBROS/2025/recibidas.xlsx",
        false,
        vec![
            (
                "TLF003****",
                "12/04",
                "A11111119",
                "TELEFONOS DEMO SA",
                "20,00",
                "21,0",
                "4,20",
                "24,20",
            ),
            (
                "TLF0039999",
                "12/04",
                "A11111119",
                "TELEFONOS DEMO SA",
                "20,00",
                "21,0",
                "4,20",
                "24,20",
            ),
            // Y otra repetida sin ningún PDF: se pide una vez, la del número entero.
            (
                "ASE000****",
                "20/05",
                ASESORES,
                "ASESORES DEMO SL",
                "100",
                "21,0",
                "21",
                "121",
            ),
            (
                "ASE0001234",
                "20/05",
                ASESORES,
                "ASESORES DEMO SL",
                "100",
                "21,0",
                "21",
                "121",
            ),
        ],
    );
    let telefono = |ruta: &str| {
        con(
            ruta,
            recibida(
                Some("A11111119"),
                "Teléfonos Demo SA",
                "TLF0039999",
                "2025-04-12",
                ["20.00", "4.20", "24.20"],
            ),
        )
    };
    let r = revisar(vec![
        libro,
        telefono("FACTURAS/2025/T1/Factura_TLF003XXXX.pdf"),
        telefono("FACTURAS/2025/T2/factura-TLF0039999.pdf"),
    ]);
    assert!(hay(&r, "libros.repetidas"));
    let h = hallazgo(&r, "libros.sin_pdf");
    assert_eq!(
        h.titulo,
        "1 factura del libro de recibidas de 2025 sin el PDF en la carpeta"
    );
    assert!(
        h.detalle.contains("ASE0001234 (ASESORES DEMO SL)")
            && !h.detalle.contains("ASE000****")
            && !h.detalle.contains("TLF"),
        "{}",
        h.detalle
    );
}

/// Un libro registro con la fecha de la factura y la de registro (fechas completas).
fn libro_con_registro(ruta: &str, titulo: &str, filas: &[[&str; 9]]) -> Fichero {
    let mut wb = Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("Facturas").unwrap();
    let cab = [
        "Fecha factura",
        "Fecha registro",
        "Número factura",
        "NIF",
        "Proveedor",
        "Base imponible",
        "% IVA",
        "Cuota IVA",
        "Total factura",
    ];
    ws.write_string(0, 0, titulo).unwrap();
    for (c, t) in cab.iter().enumerate() {
        ws.write_string(2, u16::try_from(c).unwrap(), *t).unwrap();
    }
    for (r, fila) in filas.iter().enumerate() {
        for (c, celda) in fila.iter().enumerate() {
            if !celda.is_empty() {
                ws.write_string(
                    u32::try_from(r + 3).unwrap(),
                    u16::try_from(c).unwrap(),
                    *celda,
                )
                .unwrap();
            }
        }
    }
    (ruta.to_owned(), wb.save_to_buffer().unwrap(), None)
}

#[test]
fn una_factura_de_diciembre_registrada_en_enero_esta_en_el_libro_del_año_siguiente() {
    let l2024 = libro_con_registro(
        "LIBROS/2024/recibidas 2024.xlsx",
        "Libro registro de facturas recibidas 2024",
        &[
            [
                "10/10/2024",
                "10/10/2024",
                "A-90",
                ASESORES,
                "ASESORES DEMO SL",
                "200,00",
                "21",
                "42,00",
                "242,00",
            ],
            [
                "15/11/2024",
                "15/11/2024",
                "P-100",
                PAPELERIA,
                "PAPELERIA DEMO SL",
                "100,00",
                "21",
                "21,00",
                "121,00",
            ],
        ],
    );
    let l2025 = libro_con_registro(
        "LIBROS/2025/recibidas 2025.xlsx",
        "Libro registro de facturas recibidas 2025",
        &[
            [
                "28/12/2024",
                "08/01/2025",
                "P-140",
                PAPELERIA,
                "PAPELERIA DEMO SL",
                "50,00",
                "21",
                "10,50",
                "60,50",
            ],
            [
                "10/02/2025",
                "10/02/2025",
                "A-1",
                ASESORES,
                "ASESORES DEMO SL",
                "200,00",
                "21",
                "42,00",
                "242,00",
            ],
        ],
    );
    let pdf = |ruta: &str, nif: &str, nombre: &str, numero: &str, fecha: &str, imp: [&str; 3]| {
        con(ruta, recibida(Some(nif), nombre, numero, fecha, imp))
    };
    let r = revisar(vec![
        l2024,
        l2025,
        pdf(
            "FACTURAS/2024/A-90.pdf",
            ASESORES,
            "Asesores Demo SL",
            "A-90",
            "2024-10-10",
            ["200.00", "42.00", "242.00"],
        ),
        pdf(
            "FACTURAS/2024/P-100.pdf",
            PAPELERIA,
            "Papelería Demo SL",
            "P-100",
            "2024-11-15",
            ["100.00", "21.00", "121.00"],
        ),
        pdf(
            "FACTURAS/2024/P-140.pdf",
            PAPELERIA,
            "Papelería Demo SL",
            "P-140",
            "2024-12-28",
            ["50.00", "10.50", "60.50"],
        ),
        pdf(
            "FACTURAS/2025/A-1.pdf",
            ASESORES,
            "Asesores Demo SL",
            "A-1",
            "2025-02-10",
            ["200.00", "42.00", "242.00"],
        ),
        // Ésta no está en ningún libro: sale una vez, en el de 2024.
        pdf(
            "FACTURAS/2024/I-5.pdf",
            "B44444446",
            "Imprenta Demo SL",
            "I-5",
            "2024-11-20",
            ["80.00", "16.80", "96.80"],
        ),
    ]);
    let fuera: Vec<&Hallazgo> = r
        .hallazgos
        .iter()
        .filter(|h| h.codigo == "libros.pdf_fuera_del_libro")
        .collect();
    assert_eq!(fuera.len(), 1, "{fuera:?}");
    assert_eq!(fuera[0].ficheros, ["FACTURAS/2024/I-5.pdf"]);
    assert!(!hay(&r, "libros.sin_pdf"), "{:?}", r.hallazgos);
}

#[test]
fn el_303_frente_al_libro_con_lo_que_dicen_los_pdf_de_cada_linea() {
    // El libro no trae la fecha de registro. La SW-1 lleva el NIF-IVA de Irlanda que el
    // proveedor tiene allí, pero su factura es de una sociedad de EE. UU. («PBC»): la gestoría la
    // declara en la [12], no en la [10]. La D-1 es de marzo, pero su PDF está en la carpeta del
    // 2T y se dedujo en el 2T.
    let libro = libro_2026(vec![
        (
            "SW-1",
            "20/01",
            "IE1234567T",
            "SOFTWARE DEMO PBC",
            "20",
            "21,0",
            "4,2",
            "20",
        ),
        (
            "P-1",
            "10/02",
            PAPELERIA,
            "PAPELERIA DEMO SL",
            "100",
            "21,0",
            "21",
            "121",
        ),
        (
            "D-1",
            "09/03",
            "",
            "DOMINIOS DEMO INC",
            "20",
            "21,0",
            "4,2",
            "20",
        ),
        (
            "P-2",
            "10/05",
            PAPELERIA,
            "PAPELERIA DEMO SL",
            "200",
            "21,0",
            "42",
            "242",
        ),
    ]);
    let sw = con(
        "FACTURAS/2026/T1/software.pdf",
        recibida(
            None,
            "Software Demo, PBC",
            "SW-1",
            "2026-01-20",
            ["20.00", "0", "20.00"],
        ),
    );
    let dominio = con(
        "FACTURAS/2026/T2/dominio.pdf",
        recibida(
            None,
            "Dominios Demo, Inc.",
            "D-1",
            "2026-03-09",
            ["20.00", "0", "20.00"],
        ),
    );
    let m1 = con(
        "MODELOS/2026/303-1T.pdf",
        modelo(
            "303",
            2026,
            "1T",
            json!({"12": "20.00", "13": "4.20", "27": "4.20",
                   "28": "120.00", "29": "25.20", "45": "25.20"}),
        ),
    );
    let m2 = con(
        "MODELOS/2026/303-2T.pdf",
        modelo(
            "303",
            2026,
            "2T",
            json!({"12": "20.00", "13": "4.20", "27": "4.20",
                   "28": "220.00", "29": "46.20", "45": "46.20"}),
        ),
    );
    let r = revisar(vec![libro, sw, dominio, m1, m2]);
    assert!(
        !hay(&r, "modelos.303_libro"),
        "{:?}",
        r.hallazgos
            .iter()
            .filter(|h| h.codigo == "modelos.303_libro")
            .map(|h| &h.detalle)
            .collect::<Vec<_>>()
    );
    let cuadran: Vec<&Hallazgo> = r
        .hallazgos
        .iter()
        .filter(|h| h.codigo == "modelos.303_cuadra_libro")
        .collect();
    let titulos: Vec<&str> = cuadran.iter().map(|h| h.titulo.as_str()).collect();
    for t in [
        "El 303 del 1T 2026 cuadra con los libros registro",
        "El 303 del 2T 2026 cuadra con los libros registro",
    ] {
        assert!(titulos.contains(&t), "{titulos:?}");
    }
    let t2 = cuadran
        .iter()
        .find(|h| h.titulo.contains("2T 2026"))
        .unwrap();
    assert!(
        t2.detalle.contains(
            "Se cuenta en este trimestre, por la carpeta de su PDF, la D-1 de DOMINIOS DEMO INC \
             (09/03/2026): el libro no trae la fecha de registro."
        ),
        "{}",
        t2.detalle
    );
    let t1 = cuadran
        .iter()
        .find(|h| h.titulo.contains("1T 2026"))
        .unwrap();
    assert!(
        t1.detalle.contains(
            "No se cuenta la D-1 de DOMINIOS DEMO INC (09/03/2026, en la carpeta del 2T 2026)"
        ) && t1
            .detalle
            .contains("la inversión del sujeto pasivo [12] (20,00 €)"),
        "{}",
        t1.detalle
    );
}

#[test]
fn una_compra_de_un_proveedor_con_cierre_en_el_nombre_cuenta_en_la_472() {
    use taxpain_core::accounting::Origin;
    let v = vault();
    let imp = |fecha: time::Date, concepto: &str, lineas: Vec<(&str, i64, i64)>| {
        asiento(&v, fecha, concepto, Origin::Importado(1), lineas);
    };
    imp(
        date!(2025 - 05 - 10),
        "Fra. P-1 Papelería Demo SL",
        vec![("629", 10_000, 0), ("472", 2_100, 0), ("410", 0, 12_100)],
    );
    imp(
        date!(2025 - 05 - 20),
        "Fra. 77 Cierres Metálicos Demo SL",
        vec![("622", 100_000, 0), ("472", 21_000, 0), ("410", 0, 121_000)],
    );
    imp(
        date!(2025 - 06 - 12),
        "Fra. 9 Apertura de puertas Demo",
        vec![("622", 10_000, 0), ("472", 2_100, 0), ("410", 0, 12_100)],
    );
    let r = revisar_con(
        &v,
        vec![con(
            "MODELOS/2025/303-2T.pdf",
            modelo(
                "303",
                2025,
                "2T",
                json!({"28": "1200.00", "29": "252.00", "45": "252.00"}),
            ),
        )],
    );
    assert!(!hay(&r, "modelos.303_diario"), "{:?}", r.hallazgos);
    let h = hallazgo(&r, "modelos.303_cuadra_libro");
    assert_eq!(h.titulo, "El 303 del 2T 2025 cuadra con el diario");
}

#[test]
fn el_cierre_y_la_apertura_en_valenciano_no_son_iva_del_trimestre() {
    use taxpain_core::accounting::Origin;
    let v = vault();
    let imp = |fecha: time::Date, concepto: &str, lineas: Vec<(&str, i64, i64)>| {
        asiento(&v, fecha, concepto, Origin::Importado(1), lineas);
    };
    imp(
        date!(2025 - 11 - 10),
        "Compra",
        vec![("629", 10_000, 0), ("472", 2_100, 0), ("410", 0, 12_100)],
    );
    imp(
        date!(2025 - 12 - 10),
        "Venda",
        vec![("430", 121_000, 0), ("700", 0, 100_000), ("477", 0, 21_000)],
    );
    imp(
        date!(2025 - 12 - 31),
        "Assentament de tancament",
        vec![
            ("477", 21_000, 0),
            ("410", 12_100, 0),
            ("129", 90_000, 0),
            ("472", 0, 2_100),
            ("430", 0, 121_000),
        ],
    );
    imp(
        date!(2026 - 01 - 01),
        "Assentament d'obertura",
        vec![
            ("472", 2_100, 0),
            ("430", 121_000, 0),
            ("477", 0, 21_000),
            ("410", 0, 12_100),
            ("129", 0, 90_000),
        ],
    );
    imp(
        date!(2026 - 01 - 20),
        "Liquidació de l'IVA del 4T 2025",
        vec![("477", 21_000, 0), ("472", 0, 2_100), ("4750", 0, 18_900)],
    );
    let r = revisar_con(
        &v,
        vec![con(
            "MODELOS/303-4T-2025.pdf",
            modelo("303", 2025, "4T", json!({"27": "210.00", "45": "21.00"})),
        )],
    );
    assert!(!hay(&r, "modelos.303_diario"), "{:?}", r.hallazgos);
    assert!(
        r.hallazgos
            .iter()
            .any(|h| h.titulo == "El 303 del 4T 2025 cuadra con el diario"),
        "{:?}",
        r.hallazgos.iter().map(|h| &h.titulo).collect::<Vec<_>>()
    );
}

#[test]
fn el_diario_resumido_con_retenciones_y_una_rectificacion_de_deducciones() {
    use taxpain_core::accounting::Origin;
    // Un diario resumido por meses, con cuentas de tres cifras: la 475 lleva cada mes las
    // retenciones, y en marzo, en la misma línea de la 472, el abono de la liquidación (201,60 €)
    // y el IVA de una factura rectificativa recibida (8,40 €). El 303 lleva la rectificación en
    // la [40] y la [41].
    let v = vault();
    let mes = |fecha: time::Date, lineas: Vec<(&str, i64, i64)>| {
        asiento(
            &v,
            fecha,
            "Diario resumido del gestor",
            Origin::Gestor(1),
            lineas,
        );
    };
    mes(
        date!(2025 - 01 - 31),
        vec![
            ("629", 50_000, 0),
            ("472", 10_500, 0),
            ("475", 0, 5_000),
            ("410", 0, 55_500),
        ],
    );
    mes(
        date!(2025 - 02 - 28),
        vec![
            ("629", 30_000, 0),
            ("472", 6_300, 0),
            ("475", 0, 3_000),
            ("410", 0, 33_300),
        ],
    );
    mes(
        date!(2025 - 03 - 31),
        vec![
            ("629", 20_000, 0),
            ("472", 4_200, 0),
            ("475", 0, 2_000),
            ("410", 0, 22_200),
            ("410", 4_840, 0),
            ("623", 0, 4_000),
            ("470", 20_160, 0),
            ("472", 0, 21_000),
        ],
    );
    let r = revisar_con(
        &v,
        vec![con(
            "MODELOS/303-1T.pdf",
            modelo(
                "303",
                2025,
                "1T",
                json!({"28": "1000.00", "29": "210.00", "40": "-40.00", "41": "-8.40",
                       "45": "201.60"}),
            ),
        )],
    );
    assert!(
        !hay(&r, "modelos.303_diario"),
        "{:?}",
        r.hallazgos
            .iter()
            .filter(|h| h.codigo == "modelos.303_diario")
            .map(|h| &h.detalle)
            .collect::<Vec<_>>()
    );
    let h = hallazgo(&r, "modelos.303_cuadra_libro");
    assert_eq!(h.titulo, "El 303 del 1T 2025 cuadra con el diario");
    assert!(h.detalle.contains("[41]"), "{}", h.detalle);
}

#[test]
fn sin_libro_la_maquina_importada_no_es_inversion_del_sujeto_pasivo() {
    // Una máquina comprada a un proveedor del Reino Unido: es una importación, con el IVA
    // pagado en la aduana (el DUA) y declarado en la [32], no en la [12] (art. 97.Uno.3.º LIVA).
    let maquina = con(
        "FACTURAS/2025/T2/maquina.pdf",
        recibida(
            Some("GB123456789"),
            "Machines Demo Ltd",
            "UK-5",
            "2025-05-10",
            ["1000.00", "0", "1000.00"],
        ),
    );
    let dua = con(
        "FACTURAS/2025/T2/dua.pdf",
        recibida(
            Some("Q2826000H"),
            "Agencia Tributaria (Aduana)",
            "25ES00281011234999",
            "2025-05-12",
            ["1000.00", "210.00", "210.00"],
        ),
    );
    let m303 = con(
        "MODELOS/2025/303-2T.pdf",
        modelo(
            "303",
            2025,
            "2T",
            json!({"32": "1000.00", "33": "210.00", "45": "210.00"}),
        ),
    );
    let r = revisar(vec![maquina, dua, m303]);
    assert!(
        !hay(&r, "modelos.303_inversion_sujeto_pasivo"),
        "{:?}",
        r.hallazgos
    );
}

#[test]
fn sin_libro_la_12_con_facturas_en_dolares() {
    // Las dos facturas del dominio van en dólares: la gestoría las pasó a euros (o las tomó tal
    // cual) en la [12]. No se pueden contar, pero explican la diferencia.
    let dolares = |ruta: &str, numero: &str, fecha: &str, total: &str| {
        con(
            ruta,
            en_dolares(None, "Dominios Demo, Inc.", numero, fecha, total),
        )
    };
    let r = revisar(vec![
        dolares(
            "FACTURAS/2025/T1/dominio-1.pdf",
            "D-11",
            "2025-01-10",
            "12.00",
        ),
        dolares(
            "FACTURAS/2025/T1/dominio-2.pdf",
            "D-12",
            "2025-02-10",
            "34.90",
        ),
        con(
            "MODELOS/2025/303-1T.pdf",
            modelo(
                "303",
                2025,
                "1T",
                json!({"12": "46.90", "13": "9.85", "28": "46.90", "29": "9.85"}),
            ),
        ),
        // En el 2T la [12] es mucho mayor que lo que va en dólares: eso sí se dice.
        dolares(
            "FACTURAS/2025/T2/dominio-3.pdf",
            "D-13",
            "2025-04-10",
            "10.00",
        ),
        con(
            "MODELOS/2025/303-2T.pdf",
            modelo(
                "303",
                2025,
                "2T",
                json!({"12": "500.00", "13": "105.00", "28": "500.00", "29": "105.00"}),
            ),
        ),
    ]);
    let notas: Vec<&str> = r
        .hallazgos
        .iter()
        .filter(|h| h.codigo == "modelos.303_inversion_sujeto_pasivo")
        .map(|h| h.titulo.as_str())
        .collect();
    assert_eq!(
        notas,
        ["La inversión del sujeto pasivo del 303 del 2T 2025 no casa con las facturas"]
    );
}

#[test]
fn un_hotel_con_spa_sin_nif_es_una_compra_interior() {
    let r = revisar(vec![
        con(
            "FACTURAS/2025/T2/hotel.pdf",
            recibida(
                None,
                "Hotel Spa Demo",
                "H-77",
                "2025-05-10",
                ["100.00", "10.00", "110.00"],
            ),
        ),
        con(
            "FACTURAS/2025/T2/papel.pdf",
            recibida(
                Some(PAPELERIA),
                "Papelería Demo SL",
                "P-1",
                "2025-05-12",
                ["200.00", "42.00", "242.00"],
            ),
        ),
        con(
            "MODELOS/2025/303-2T.pdf",
            modelo(
                "303",
                2025,
                "2T",
                json!({"28": "300.00", "29": "52.00", "45": "52.00"}),
            ),
        ),
    ]);
    for codigo in [
        "facturas.extranjero_con_iva",
        "facturas.extranjero_con_iva_particular",
        "modelos.303_iva_extranjero",
        "modelos.303_falta_factura",
    ] {
        assert!(!hay(&r, codigo), "{codigo}: {:?}", r.hallazgos);
    }
}

#[test]
fn un_proveedor_de_fuera_que_cobra_iva_a_la_empresa_sin_su_nif() {
    // La CC-1 no lleva el NIF de la empresa, pero la gestoría la lleva en el libro como
    // inversión del sujeto pasivo: la empresa es el sujeto pasivo y el IVA está mal cobrado. La
    // VD-2 va a nombre de la empresa, sin su NIF.
    let libro = libro_2026(vec![(
        "CC-1",
        "11/08",
        "",
        "VIDEOS DEMO PTE LTD",
        "24,20",
        "21,0",
        "5,08",
        "24,20",
    )]);
    let mut cc = recibida(
        None,
        "Videos Demo Pte. Ltd.",
        "CC-1",
        "2026-08-11",
        ["20.00", "4.20", "24.20"],
    );
    cc["factura"]["nif_destinatario"] = Value::Null;
    let mut vd = recibida(
        None,
        "Videos Demo Pte. Ltd.",
        "VD-2",
        "2026-08-20",
        ["10.00", "2.10", "12.10"],
    );
    vd["factura"]["nif_destinatario"] = Value::Null;
    vd["factura"]["nombre_destinatario"] = json!("EMPRESA DEMO, S.L.");
    let r = revisar(vec![
        libro,
        con("FACTURAS/2026/T3/cc-1.pdf", cc),
        con("FACTURAS/2026/T3/vd-2.pdf", vd),
    ]);
    let h = hallazgo(&r, "facturas.extranjero_con_iva");
    assert_eq!(
        h.ficheros,
        ["FACTURAS/2026/T3/cc-1.pdf", "FACTURAS/2026/T3/vd-2.pdf"]
    );
    assert!(
        h.detalle
            .starts_with("1 proveedor de otro país cobra IVA en facturas a la empresa"),
        "{}",
        h.detalle
    );
    assert!(
        !hay(&r, "facturas.extranjero_con_iva_particular"),
        "{:?}",
        r.hallazgos
    );
}

#[test]
fn un_hotel_de_otro_pais_lleva_su_iva_aunque_se_le_de_el_nif_iva() {
    let mut l = recibida(
        Some("FR98765432109"),
        "Hôtel Démo SARL",
        "H-12",
        "2025-05-10",
        ["200.00", "20.00", "220.00"],
    );
    l["factura"]["nif_destinatario"] = Value::Null;
    l["factura"]["nombre_destinatario"] = Value::Null;
    l["descripcion"] = json!("Factura de hotel en París (2 noches)");
    let r = revisar(vec![con("FACTURAS/2025/hotel-paris.pdf", l)]);
    let h = hallazgo(&r, "facturas.extranjero_con_iva_particular");
    assert_eq!(
        h.titulo,
        "1 factura de un proveedor de fuera de España con IVA, sin el NIF ni el nombre de la \
         empresa"
    );
    assert!(
        h.detalle.starts_with("1 proveedor de otro país cobra IVA")
            && h.detalle.contains("un hotel, un restaurante, una entrada o un servicio ligado a un inmueble de ese país")
            && h.detalle.contains("art. 47, 53 y 55 de la Directiva 2006/112/CE")
            && !h.detalle.contains("no hay nada que reclamarles"),
        "{}",
        h.detalle
    );
}

#[test]
fn con_libro_el_iva_de_un_proveedor_de_fuera_no_se_puede_deducir() {
    let libro = libro_2026(vec![
        (
            "P-1",
            "10/07",
            PAPELERIA,
            "PAPELERIA DEMO SL",
            "100",
            "21,0",
            "21",
            "121",
        ),
        (
            "YT-1",
            "24/07",
            "IE1234567T",
            "VIDEOS DEMO LTD",
            "10",
            "21,0",
            "2,1",
            "12,1",
        ),
    ]);
    let m303 = con(
        "MODELOS/2026/303-3T.pdf",
        modelo(
            "303",
            2026,
            "3T",
            json!({"28": "100.00", "29": "21.00", "45": "21.00"}),
        ),
    );
    let r = revisar(vec![libro, m303]);
    let h = hallazgo(&r, "modelos.303_cuadra_libro");
    assert!(
        h.detalle.contains(
            "No se cuenta el IVA de 1 factura de un proveedor de fuera de España (2,10 €): no se \
             puede deducir (art. 94.Tres y 97 LIVA;"
        ) && !h.detalle.contains("no deberían cobrarlo"),
        "{}",
        h.detalle
    );
}

/// Una emitida a un cliente de Francia con el texto del PDF y la lectura.
fn emitida_a_francia(ruta: &str, texto: &str, importes: Value) -> Fichero {
    let mut factura = json!({
        "nif_emisor": PROPIO, "nombre_emisor": "Empresa Demo SL",
        "nif_destinatario": PIEZAS_UE, "nombre_destinatario": "Pieces Demo SARL",
        "numero": ruta.trim_start_matches("INGRESOS/").trim_end_matches(".pdf"),
        "fecha": "2025-05-10"
    });
    for (k, v) in importes.as_object().unwrap() {
        factura[k] = v.clone();
    }
    (
        ruta.to_owned(),
        pdf(texto),
        Some(json!({
            "tipo": "factura_emitida",
            "descripcion": "Factura emitida a un cliente de Francia",
            "factura": factura
        })),
    )
}

#[test]
fn la_mencion_de_las_emitidas_a_la_ue_en_otra_lengua_o_abreviada() {
    let sin_iva = json!({"base": "1000.00", "iva": "0", "total": "1000.00"});
    let r = revisar(vec![
        emitida_a_francia(
            "INGRESOS/E-2.pdf",
            "Factura E-2. Serveis de disseny web. Base 1.000,00. Inversió del subjecte passiu",
            sin_iva.clone(),
        ),
        emitida_a_francia(
            "INGRESOS/E-3.pdf",
            "Factura E-3. Web design services. Total 1.000,00. VAT: ISP (inv. suj. pasivo)",
            sin_iva.clone(),
        ),
        // Con IVA, aunque la lectura no traiga la cuota: no es una factura sin IVA.
        emitida_a_francia(
            "INGRESOS/E-9.pdf",
            "Factura E-9. Alquiler de stand en una feria de Madrid. Base 1.000,00. IVA 21 %: \
             210,00. Total 1.210,00",
            json!({"base": "1000.00", "total": "1210.00"}),
        ),
    ]);
    assert!(!hay(&r, "emitidas.sin_mencion"), "{:?}", r.hallazgos);
    // Y la que no lo dice: se corrige con una rectificativa, la pida o no el cliente.
    let r = revisar(vec![emitida_a_francia(
        "INGRESOS/E-1.pdf",
        "Factura E-1. Servicios de consultoría. Base 1.000,00 €. Total 1.000,00 €.",
        sin_iva,
    )]);
    let h = hallazgo(&r, "emitidas.sin_mencion");
    assert!(
        h.detalle.contains(
            "Cada una se corrige con una factura rectificativa que lo diga (art. 15.1 del \
             Reglamento)"
        ),
        "{}",
        h.detalle
    );
}

#[test]
fn lo_que_se_deduce_de_mas_en_un_303_a_compensar_no_es_a_pagar() {
    // Una factura apuntada dos veces en el 2T y comida en el 3T. Los 303 salen a compensar y lo
    // que queda por compensar cubre lo que se deduce de más: no hay nada que ingresar.
    let libro = || {
        libro_2026(vec![
            (
                "TLF003****",
                "12/04",
                "A11111119",
                "TELEFONOS DEMO SA",
                "20,00",
                "21,0",
                "4,20",
                "24,20",
            ),
            (
                "TLF0039999",
                "12/04",
                "A11111119",
                "TELEFONOS DEMO SA",
                "20,00",
                "21,0",
                "4,20",
                "24,20",
            ),
            (
                "0012345678",
                "22/07",
                "A55555551",
                "ALMACENES DEMO SA",
                "175,24",
                "21,0",
                "36,80",
                "212,04",
            ),
        ])
    };
    let compra = || {
        let mut c = recibida(
            Some("A55555551"),
            "Almacenes Demo, S.A.",
            "0012345678",
            "2026-07-22",
            ["175.24", "36.80", "212.04"],
        );
        c["descripcion"] = json!("Factura de Almacenes Demo por compra de alimentación y bebidas");
        con("FACTURAS/2026/T3/almacenes.pdf", c)
    };
    let m303 = |q: &str, casillas: Value| {
        con(
            &format!("MODELOS/2026/303-{q}.pdf"),
            modelo("303", 2026, q, casillas),
        )
    };
    let r = revisar(vec![
        libro(),
        compra(),
        m303(
            "2T",
            json!({"28": "40.00", "29": "8.40", "45": "8.40", "71": "-50.00", "87": "10.00"}),
        ),
        m303(
            "3T",
            json!({"28": "175.24", "29": "36.80", "45": "36.80", "71": "-120.00", "87": "80.00"}),
        ),
    ]);
    for codigo in ["libros.repetidas", "libros.iva_sin_derecho"] {
        let h = hallazgo(&r, codigo);
        assert_eq!(h.importe, None, "{codigo}: {}", h.detalle);
        assert!(
            h.detalle.contains("no hay que ingresar nada"),
            "{codigo}: {}",
            h.detalle
        );
    }
    // Si el 303 del 3T sale a ingresar, la comida sí es lo que se pagaría de más.
    let r = revisar(vec![
        libro(),
        compra(),
        m303(
            "3T",
            json!({"28": "175.24", "29": "36.80", "45": "36.80", "71": "100.00"}),
        ),
    ]);
    assert_eq!(
        hallazgo(&r, "libros.iva_sin_derecho").importe,
        Some(-Money::from_cents(3_680))
    );
}

#[test]
fn el_349_mensual_que_ya_ha_prescrito_no_se_avisa() {
    // En el 1T 2022 pasa de 50.000 €: los 349 de 2022 tenían que ser mensuales. A 25/10/2026,
    // la sanción de los del 1T al 3T ha prescrito (el de septiembre vencía el 20/10/2022; cuatro
    // años, art. 189.2 LGT); la del 4T (el de diciembre vencía el 30/01/2023), no.
    let m303 = |periodo: &str, ue: &str| {
        con(
            &format!("MODELOS/2022/303-{periodo}.pdf"),
            modelo("303", 2022, periodo, json!({ "59": ue })),
        )
    };
    let m349 = |periodo: &str, total: &str| {
        con(
            &format!("MODELOS/2022/349-{periodo}.pdf"),
            modelo("349", 2022, periodo, json!({ "01": "1", "02": total })),
        )
    };
    let r = revisar(vec![
        m303("1T", "60000.00"),
        m349("1T", "60000.00"),
        m303("3T", "10000.00"),
        m349("3T", "10000.00"),
        m303("4T", "10000.00"),
        m349("4T", "10000.00"),
    ]);
    let titulos: Vec<&str> = r
        .hallazgos
        .iter()
        .filter(|h| h.codigo == "modelos.349_mensual")
        .map(|h| h.titulo.as_str())
        .collect();
    assert_eq!(titulos, ["El 349 del 4T 2022 tenía que ser mensual"]);
    assert!(
        hallazgo(&r, "modelos.349_mensual")
            .detalle
            .contains("prescribe el 30/01/2027"),
        "{}",
        hallazgo(&r, "modelos.349_mensual").detalle
    );
}

#[test]
fn si_la_gestoria_sigue_el_nif_del_libro_tambien_cuadra() {
    // La misma factura de una sociedad de EE. UU. con el NIF-IVA de Irlanda en el libro, pero
    // esta vez la gestoría la declara en la [10], como dice el libro: también cuadra.
    let libro = libro_2026(vec![
        (
            "SW-7",
            "20/07",
            "IE1234567T",
            "SOFTWARE DEMO PBC",
            "20",
            "21,0",
            "4,2",
            "20",
        ),
        (
            "P-7",
            "10/08",
            PAPELERIA,
            "PAPELERIA DEMO SL",
            "100",
            "21,0",
            "21",
            "121",
        ),
    ]);
    let sw = con(
        "FACTURAS/2026/T3/software.pdf",
        recibida(
            None,
            "Software Demo, PBC",
            "SW-7",
            "2026-07-20",
            ["20.00", "0", "20.00"],
        ),
    );
    let m3 = con(
        "MODELOS/2026/303-3T.pdf",
        modelo(
            "303",
            2026,
            "3T",
            json!({"10": "20.00", "11": "4.20", "27": "4.20", "28": "100.00", "29": "21.00",
                   "36": "20.00", "37": "4.20", "45": "25.20"}),
        ),
    );
    let r = revisar(vec![libro, sw, m3]);
    assert!(!hay(&r, "modelos.303_libro"), "{:?}", r.hallazgos);
    assert_eq!(
        hallazgo(&r, "modelos.303_cuadra_libro").titulo,
        "El 303 del 3T 2026 cuadra con los libros registro"
    );
}
