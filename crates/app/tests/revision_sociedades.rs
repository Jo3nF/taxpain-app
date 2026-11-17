//! Revisión de la carpeta: el Impuesto sobre Sociedades, la sociedad y su socio con sus papeles
//! (el acta que aprueba las cuentas, el balance del 200 siguiente, los dividendos que llegan al
//! banco, las nóminas del administrador). Datos inventados, NIF de prueba.

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};
use taxpain_app::companies::{self, CompanyInfo};
use taxpain_app::revision::{self, Gravedad, Hallazgo, Revision};
use taxpain_invoicing::pdf::{Para, text_document};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

const PROPIO: &str = "B12345674";

fn vault() -> Vault {
    vault_de("Empresa Demo SL")
}

fn vault_de(nombre: &str) -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let info = CompanyInfo {
        name: nombre.into(),
        nif: Some(PROPIO.into()),
    };
    companies::set_info(&v, &info, datetime!(2026-01-01 9:00 UTC)).unwrap();
    v
}

fn pdf(parrafos: Vec<Para>) -> Vec<u8> {
    text_document("Documento", &[parrafos]).unwrap()
}

fn escribir(root: &Path, ruta: &str, bytes: &[u8]) {
    let p = root.join(ruta);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

fn modelo(m: &str, ejercicio: i32, periodo: &str, casillas: Value) -> Value {
    json!({
        "tipo": "modelo_aeat",
        "descripcion": format!("Modelo {m} {periodo} {ejercicio}"),
        "modelo": { "modelo": m, "ejercicio": ejercicio, "periodo": periodo, "casillas": casillas }
    })
}

type Fichero = (String, Vec<u8>, Option<Value>);

fn con(ruta: &str, lectura: Value) -> Fichero {
    (
        ruta.to_owned(),
        pdf(vec![Para::Text(format!("Documento {ruta}"))]),
        Some(lectura),
    )
}

fn revisar(ficheros: Vec<Fichero>, hoy: time::Date) -> Revision {
    revisar_con(&vault(), ficheros, hoy)
}

fn revisar_con(v: &Vault, ficheros: Vec<Fichero>, hoy: time::Date) -> Revision {
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
    revision::revisar(v, root, hoy).unwrap()
}

fn hallazgo<'a>(r: &'a Revision, codigo: &str) -> &'a Hallazgo {
    r.hallazgos
        .iter()
        .find(|h| h.codigo == codigo)
        .unwrap_or_else(|| {
            panic!(
                "falta {codigo}: {:?}",
                r.hallazgos.iter().map(|h| &h.codigo).collect::<Vec<_>>()
            )
        })
}

fn hay(r: &Revision, codigo: &str) -> bool {
    r.hallazgos.iter().any(|h| h.codigo == codigo)
}

/// 200 de 2025 de una microempresa: base 30.000 €, reserva de nivelación 3.000 € (el 10 %),
/// base 27.000 € y cuota 5.670 € (21 %). Sin la rebaja, 6.300 €: se ahorraron 630 €.
fn doscientos_2025() -> Fichero {
    con(
        "MODELOS/2025/200.pdf",
        modelo(
            "200",
            2025,
            "0A",
            json!({
                "00255": "500000.00", "00500": "25000.00", "00550": "30000.00",
                "00552": "30000.00", "01034": "3000.00", "01330": "27000.00",
                "00562": "5670.00", "01002": "4000.00"
            }),
        ),
    )
}

/// Un acta de la junta en PDF, sin lectura: la aplicación del resultado sale de su texto.
fn acta_en_pdf(distribucion: &[(&str, &str)]) -> Fichero {
    acta_ordinaria("CONTABILIDAD/2025/acta.pdf", distribucion)
}

/// El acta de la junta ordinaria del 30/06/2026 que aprueba las cuentas de 2025 (resultado de
/// 25.000 €), en PDF y sin lectura, en la ruta que se diga.
fn acta_ordinaria(ruta: &str, distribucion: &[(&str, &str)]) -> Fichero {
    let mut p = vec![
        Para::Title("ACTA DE LA JUNTA GENERAL ORDINARIA DE EMPRESA DEMO SL".into()),
        Para::Text(
            "En Valencia, a 30 de junio de 2026, reunida la junta general ordinaria con todos los \
             socios, se adoptan los siguientes acuerdos."
                .into(),
        ),
        Para::Text(
            "PRIMERO. Aprobar las cuentas anuales del ejercicio social cerrado el 31 de diciembre \
             de 2025."
                .into(),
        ),
        Para::Text("SEGUNDO. Aprobar la propuesta de aplicación del resultado:".into()),
        Para::Bold("BASE DE REPARTO".into()),
        Para::Row("Pérdidas y ganancias".into(), "25.000,00".into()),
        Para::Bold("DISTRIBUCIÓN".into()),
    ];
    for (que, cuanto) in distribucion {
        p.push(Para::Row((*que).into(), (*cuanto).into()));
    }
    p.push(Para::Row("Total distribuido".into(), "25.000,00".into()));
    (ruta.into(), pdf(p), None)
}

/// Un acta de junta extraordinaria, en PDF y sin lectura, que reparte un dividendo con cargo a
/// reservas: no aprueba las cuentas de ningún ejercicio.
fn acta_extraordinaria(ruta: &str, dia: &str) -> Fichero {
    let p = vec![
        Para::Title(
            "ACTA DE LA JUNTA GENERAL EXTRAORDINARIA Y UNIVERSAL DE EMPRESA DEMO SL".into(),
        ),
        Para::Text(format!(
            "En Valencia, a {dia}, reunidos todos los socios en junta general extraordinaria y \
             universal, acuerdan por unanimidad la distribución de un dividendo con cargo a \
             reservas voluntarias:"
        )),
        Para::Row("Dividendo a repartir".into(), "6.300,00".into()),
        Para::Text("El dividendo se pagará por transferencia en los próximos días.".into()),
    ];
    (ruta.into(), pdf(p), None)
}

fn sin_dotar(r: &Revision) -> Option<&Hallazgo> {
    r.hallazgos
        .iter()
        .find(|h| h.codigo == "sociedades.reserva_nivelacion_sin_dotar")
}

/// El acta copia la tabla de la memoria, con la columna del año anterior: en 2025 van 3.000 € a
/// la reserva de nivelación y en 2024 nada. Vale la columna de 2025: está dotada. Antes se leía
/// el importe con el que acaba la línea (el de 2024) y salía «no se dotó».
#[test]
fn la_tabla_del_acta_con_la_columna_del_año_anterior() {
    let tabla = |columnas: &str| {
        let p =
            vec![
            Para::Title("ACTA DE LA JUNTA GENERAL ORDINARIA DE EMPRESA DEMO SL".into()),
            Para::Text(
                "En Valencia, a 30 de junio de 2026, reunida la junta general ordinaria con todos \
                 los socios, se adoptan los siguientes acuerdos."
                    .into(),
            ),
            Para::Text(
                "PRIMERO. Aprobar las cuentas anuales del ejercicio social cerrado el 31 de \
                 diciembre de 2025."
                    .into(),
            ),
            Para::Text("SEGUNDO. Aprobar la propuesta de aplicación del resultado:".into()),
            Para::Row("BASE DE REPARTO".into(), columnas.into()),
            Para::Row("Pérdidas y ganancias".into(), "25.000,00      6.500,00".into()),
            Para::Row("DISTRIBUCIÓN".into(), columnas.into()),
            Para::Row("A reserva legal".into(), "1.000,00      650,00".into()),
            Para::Row("A reserva de nivelación".into(), "3.000,00      0,00".into()),
            Para::Row("A reservas voluntarias".into(), "21.000,00      5.850,00".into()),
            Para::Row("Total distribuido".into(), "25.000,00      6.500,00".into()),
        ];
        ("CONTABILIDAD/2025/acta.pdf".to_owned(), pdf(p), None)
    };
    let r = revisar(
        vec![doscientos_2025(), tabla("2025      2024")],
        date!(2026 - 10 - 31),
    );
    assert!(sin_dotar(&r).is_none(), "{:?}", sin_dotar(&r));
    let h = hallazgo(&r, "sociedades.reserva_nivelacion");
    assert_eq!(h.gravedad, Gravedad::Info);
    assert!(h.titulo.contains("dotada"), "{}", h.titulo);
    // Si no se sabe qué columna es la del ejercicio, no se concluye nada.
    let r = revisar(
        vec![doscientos_2025(), tabla("2023      2022")],
        date!(2026 - 10 - 31),
    );
    assert!(sin_dotar(&r).is_none(), "{:?}", sin_dotar(&r));
    assert_eq!(
        hallazgo(&r, "sociedades.reserva_nivelacion").gravedad,
        Gravedad::Aviso
    );
}

/// Una junta extraordinaria que reparte un dividendo con cargo a reservas no aprueba las cuentas:
/// vale el acta ordinaria, que dota la reserva, aunque la otra vaya antes por la ruta. Antes, la
/// extraordinaria se tomaba por el acta que aprueba las cuentas y salía «no se dotó».
#[test]
fn la_junta_que_reparte_reservas_no_aprueba_las_cuentas() {
    let r = revisar(
        vec![
            doscientos_2025(),
            acta_extraordinaria("ACTAS/acta-dividendo.pdf", "15 de septiembre de 2026"),
            acta_ordinaria(
                "ACTAS/acta-ordinaria.pdf",
                &[
                    ("A reserva legal", "1.000,00"),
                    ("A reserva de nivelación", "3.000,00"),
                    ("A reservas voluntarias", "21.000,00"),
                ],
            ),
        ],
        date!(2026 - 10 - 31),
    );
    assert!(sin_dotar(&r).is_none(), "{:?}", sin_dotar(&r));
    let h = hallazgo(&r, "sociedades.reserva_nivelacion");
    assert_eq!(h.gravedad, Gravedad::Info);
    assert!(
        h.ficheros.contains(&"ACTAS/acta-ordinaria.pdf".to_owned()),
        "{:?}",
        h.ficheros
    );
}

/// El depósito de las cuentas vence un mes después de la junta que las aprueba, no de otra junta
/// anterior que reparte reservas: con la extraordinaria del 10/03/2026 y la ordinaria del
/// 30/06/2026, el depósito del 20/07/2026 está en plazo (hasta el 30/07/2026).
#[test]
fn el_deposito_cuenta_desde_la_junta_que_aprueba_las_cuentas() {
    let deposito = json!({
        "tipo": "registro_mercantil",
        "descripcion": "Justificante del depósito de las cuentas de 2025",
        "empresa": { "name": "EMPRESA DEMO SL", "date": "2026-07-20" },
        "registro": { "tramite": "deposito_cuentas", "ejercicio": 2025, "presentado": "2026-07-20" }
    });
    let r = revisar(
        vec![
            doscientos_2025(),
            acta_extraordinaria("ACTAS/2026-03-10 extraordinaria.pdf", "10 de marzo de 2026"),
            acta_ordinaria(
                "ACTAS/2026-06-30 ordinaria.pdf",
                &[
                    ("A reserva legal", "1.000,00"),
                    ("A reserva de nivelación", "3.000,00"),
                    ("A reservas voluntarias", "21.000,00"),
                ],
            ),
            con("REGISTRO/deposito-2025.pdf", deposito),
        ],
        date!(2026 - 10 - 31),
    );
    assert!(!hay(&r, "cruces.deposito_fuera_de_plazo"));
    let t = r
        .tablas
        .iter()
        .find(|t| t.titulo == "Depósito de cuentas y legalización de libros")
        .unwrap();
    assert!(
        t.filas
            .iter()
            .any(|f| f[0] == "2025" && f[2] == "30/07/2026" && f[4] == "en plazo"),
        "{:?}",
        t.filas
    );
}

/// El arreglo que propone la revisión: una decisión posterior (con su lectura) que pasa a la
/// reserva de nivelación lo que el acta llevó a reservas voluntarias. Con ella, la reserva está
/// dotada. Antes sólo se miraba la primera acta del ejercicio y el error seguía.
#[test]
fn la_decision_posterior_que_dota_la_reserva() {
    let decision = json!({
        "tipo": "acta_junta",
        "descripcion": "Acta de la junta general extraordinaria de 16/10/2026: dota la reserva de nivelación de 2025 con cargo a reservas voluntarias",
        "empresa": { "name": "EMPRESA DEMO SL", "date": "2026-10-16" },
        "acta": {
            "ejercicio": 2025, "fecha": "2026-10-16", "aprueba_cuentas": false,
            "reserva_nivelacion": "3000.00", "reservas_voluntarias": "-3000.00"
        }
    });
    let r = revisar(
        vec![
            doscientos_2025(),
            acta_ordinaria(
                "ACTAS/2026-06-30 ordinaria.pdf",
                &[
                    ("A reserva legal", "1.000,00"),
                    ("A reservas voluntarias", "24.000,00"),
                ],
            ),
            con("ACTAS/2026-10-16 nivelacion.pdf", decision),
        ],
        date!(2026 - 10 - 31),
    );
    assert!(sin_dotar(&r).is_none(), "{:?}", sin_dotar(&r));
    let h = hallazgo(&r, "sociedades.reserva_nivelacion");
    assert_eq!(h.gravedad, Gravedad::Info);
    assert!(h.detalle.contains("16/10/2026"), "{}", h.detalle);
    assert!(
        h.ficheros
            .contains(&"ACTAS/2026-10-16 nivelacion.pdf".to_owned()),
        "{:?}",
        h.ficheros
    );
    assert!(
        !r.pedir
            .iter()
            .any(|p| p.que.contains("reserva de nivelación"))
    );
}

/// Una partida que TaxPain no reconoce («reserva indisponible») puede ser la reserva de
/// nivelación: el acta no se ha leído entera y no se concluye que no se dotó. Con la aplicación
/// del resultado del 200 ([01271]) que la dota, está dotada; sin ella, se pide comprobarlo. Lo
/// mismo si las partidas leídas no suman lo que se reparte.
#[test]
fn el_acta_que_no_se_lee_entera_no_dice_que_no_se_dotara() {
    let con_01271 = con(
        "MODELOS/2025/200.pdf",
        modelo(
            "200",
            2025,
            "0A",
            json!({
                "00255": "500000.00", "00500": "25000.00", "00550": "30000.00",
                "00552": "30000.00", "01034": "3000.00", "01330": "27000.00",
                "00562": "5670.00", "01002": "4000.00", "00666": "25000.00", "01271": "3000.00"
            }),
        ),
    );
    let indisponible = [
        ("A reserva legal", "1.000,00"),
        ("A reserva indisponible", "3.000,00"),
        ("A reservas voluntarias", "21.000,00"),
    ];
    let r = revisar(
        vec![con_01271, acta_en_pdf(&indisponible)],
        date!(2026 - 10 - 31),
    );
    assert!(sin_dotar(&r).is_none(), "{:?}", sin_dotar(&r));
    assert_eq!(
        hallazgo(&r, "sociedades.reserva_nivelacion").gravedad,
        Gravedad::Info
    );
    let r = revisar(
        vec![doscientos_2025(), acta_en_pdf(&indisponible)],
        date!(2026 - 10 - 31),
    );
    assert!(sin_dotar(&r).is_none(), "{:?}", sin_dotar(&r));
    let h = hallazgo(&r, "sociedades.reserva_nivelacion");
    assert_eq!(h.gravedad, Gravedad::Aviso);
    assert!(
        h.detalle.contains("no se ha podido leer entera"),
        "{}",
        h.detalle
    );
    // Las partidas leídas suman 22.000 € de los 25.000 €.
    let r = revisar(
        vec![
            doscientos_2025(),
            acta_en_pdf(&[
                ("A reserva legal", "1.000,00"),
                ("A reservas voluntarias", "21.000,00"),
            ]),
        ],
        date!(2026 - 10 - 31),
    );
    assert!(sin_dotar(&r).is_none(), "{:?}", sin_dotar(&r));
}

/// Un acta leída entera que no dota la reserva frente a la aplicación del resultado del 200
/// ([01271]) que sí la lleva: no se sabe cuál es la buena, así que se pide comprobarlo (no es un
/// error con coste).
#[test]
fn el_acta_y_el_200_no_dicen_lo_mismo() {
    let con_01271 = con(
        "MODELOS/2025/200.pdf",
        modelo(
            "200",
            2025,
            "0A",
            json!({
                "00255": "500000.00", "00500": "25000.00", "00550": "30000.00",
                "00552": "30000.00", "01034": "3000.00", "01330": "27000.00",
                "00562": "5670.00", "00666": "25000.00", "01271": "3000.00"
            }),
        ),
    );
    let r = revisar(
        vec![
            con_01271,
            acta_en_pdf(&[
                ("A reserva legal", "1.000,00"),
                ("A reservas voluntarias", "24.000,00"),
            ]),
        ],
        date!(2026 - 10 - 31),
    );
    assert!(sin_dotar(&r).is_none(), "{:?}", sin_dotar(&r));
    let h = hallazgo(&r, "sociedades.reserva_nivelacion");
    assert_eq!(h.gravedad, Gravedad::Aviso);
    assert!(h.detalle.contains("[01271]"), "{}", h.detalle);
    assert!(h.detalle.contains("acta del 30/06/2026"), "{}", h.detalle);
}

/// El acta aprueba las cuentas «del ejercicio económico 2025» y en otro punto fija la retribución
/// del administrador «para el ejercicio 2026»: es el acta de 2025. Antes se tomaba por la de
/// 2026 y salía el depósito de las cuentas de 2026 con un plazo anterior a su cierre.
#[test]
fn el_ejercicio_del_acta_es_el_de_las_cuentas_que_aprueba() {
    let p = vec![
        Para::Title("ACTA DE LA JUNTA GENERAL ORDINARIA DE EMPRESA DEMO SL".into()),
        Para::Text(
            "En Valencia, a 30 de junio de 2026, reunida la junta general ordinaria con todos los \
             socios, se adoptan los siguientes acuerdos."
                .into(),
        ),
        Para::Text("PRIMERO. Aprobar las cuentas anuales del ejercicio económico 2025.".into()),
        Para::Text("SEGUNDO. Aprobar la propuesta de aplicación del resultado:".into()),
        Para::Bold("BASE DE REPARTO".into()),
        Para::Row("Pérdidas y ganancias".into(), "25.000,00".into()),
        Para::Bold("DISTRIBUCIÓN".into()),
        Para::Row("A reserva legal".into(), "2.500,00".into()),
        Para::Row("A reservas voluntarias".into(), "22.500,00".into()),
        Para::Row("Total distribuido".into(), "25.000,00".into()),
        Para::Text(
            "TERCERO. Fijar la retribución del administrador para el ejercicio 2026 en 18.000,00 \
             euros."
                .into(),
        ),
    ];
    let deposito = json!({
        "tipo": "registro_mercantil",
        "descripcion": "Justificante del depósito de las cuentas de 2025",
        "empresa": { "name": "EMPRESA DEMO SL", "date": "2026-07-20" },
        "registro": { "tramite": "deposito_cuentas", "ejercicio": 2025, "presentado": "2026-07-20" }
    });
    let r = revisar(
        vec![
            ("ACTAS/acta-2026.pdf".into(), pdf(p), None),
            con("REGISTRO/deposito-2025.pdf", deposito),
            con(
                "MODELOS/2026/303-1T.pdf",
                modelo("303", 2026, "1T", json!({"71": "100.00"})),
            ),
            con(
                "MODELOS/2025/303-1T.pdf",
                modelo("303", 2025, "1T", json!({"71": "100.00"})),
            ),
        ],
        date!(2027 - 03 - 01),
    );
    let t = r
        .tablas
        .iter()
        .find(|t| t.titulo == "Depósito de cuentas y legalización de libros")
        .unwrap();
    assert!(
        !t.filas
            .iter()
            .any(|f| f[0] == "2026" && f[1] == "Depósito de cuentas"),
        "{:?}",
        t.filas
    );
    assert!(
        t.filas
            .iter()
            .any(|f| f[0] == "2025" && f[2] == "30/07/2026" && f[4] == "en plazo"),
        "{:?}",
        t.filas
    );
    assert!(!hay(&r, "cruces.deposito_sin_justificante"));
}

/// La junta aprueba las cuentas de 2025 y lleva el resultado a reserva legal y voluntarias: la
/// reserva de nivelación no se dotó. Antes, la revisión sólo pedía comprobarlo en el acta.
#[test]
fn la_reserva_de_nivelacion_que_el_acta_no_dota() {
    let r = revisar(
        vec![
            doscientos_2025(),
            acta_en_pdf(&[
                ("A reserva legal", "1.000,00"),
                ("A reservas voluntarias", "24.000,00"),
            ]),
        ],
        date!(2026 - 10 - 31),
    );
    let h = hallazgo(&r, "sociedades.reserva_nivelacion_sin_dotar");
    assert_eq!(h.gravedad, Gravedad::Error);
    assert_eq!(h.ejercicio, Some(2025));
    assert!(h.titulo.contains("no se dotó"), "{}", h.titulo);
    // 630 € de impuesto ahorrado más el 5 %.
    assert!(h.titulo.contains("661,50 €"), "{}", h.titulo);
    assert!(h.detalle.contains("art. 105.6 LIS"), "{}", h.detalle);
    assert!(
        h.ficheros
            .contains(&"CONTABILIDAD/2025/acta.pdf".to_owned())
    );
    assert_eq!(h.rectificable_hasta, None);
    assert!(!hay(&r, "sociedades.reserva_nivelacion"));
}

/// El acta leída con IA trae la aplicación del resultado en su bloque `acta`: si la dota, está
/// dotada y no se pide nada.
#[test]
fn la_reserva_de_nivelacion_dotada_segun_la_lectura_del_acta() {
    let acta = json!({
        "tipo": "acta_junta",
        "descripcion": "Acta de la junta general ordinaria de 30/06/2026",
        "empresa": { "name": "EMPRESA DEMO SL", "date": "2026-06-30" },
        "acta": {
            "ejercicio": 2025, "fecha": "2026-06-30", "resultado": "25000.00",
            "reserva_legal": "1000.00", "reserva_nivelacion": "3000.00",
            "reservas_voluntarias": "21000.00"
        }
    });
    let r = revisar(
        vec![doscientos_2025(), con("CONTABILIDAD/2025/acta.pdf", acta)],
        date!(2026 - 10 - 31),
    );
    let h = hallazgo(&r, "sociedades.reserva_nivelacion");
    assert_eq!(h.gravedad, Gravedad::Info);
    assert!(h.titulo.contains("dotada"), "{}", h.titulo);
    assert!(!hay(&r, "sociedades.reserva_nivelacion_sin_dotar"));
    assert!(
        !r.pedir
            .iter()
            .any(|p| p.que.contains("reserva de nivelación"))
    );
}

/// Una lectura de IA del acta sin el bloque `acta` (sólo la descripción): la aplicación del
/// resultado sale del texto del PDF, como en un acta sin leer.
#[test]
fn el_acta_leida_sin_el_bloque_se_lee_por_su_texto() {
    let (ruta, bytes, _) = acta_en_pdf(&[
        ("A reserva legal", "1.000,00"),
        ("A reservas voluntarias", "24.000,00"),
    ]);
    let lectura = json!({
        "tipo": "acta_junta",
        "descripcion": "Decisión del socio único: aprueba las cuentas de 2025",
        "empresa": { "name": "EMPRESA DEMO SL", "date": "2026-06-30" }
    });
    let r = revisar(
        vec![doscientos_2025(), (ruta, bytes, Some(lectura))],
        date!(2026 - 10 - 31),
    );
    assert_eq!(
        hallazgo(&r, "sociedades.reserva_nivelacion_sin_dotar").gravedad,
        Gravedad::Error
    );
}

/// La reserva de nivelación de 2023 que el balance del 200 de 2024 ya recoge (la [01002] sube lo
/// minorado) y que la [01033] de 2024 ya sumó a la base: dotada y cumplida. Antes salía un aviso
/// para que la gestoría confirmara la dotación.
#[test]
fn la_reserva_de_nivelacion_que_ya_recoge_el_200_siguiente() {
    let r = revisar(
        vec![
            con(
                "MODELOS/2023/200.pdf",
                modelo(
                    "200",
                    2023,
                    "0A",
                    json!({
                        "00255": "300000.00", "00550": "20000.00", "00552": "20000.00",
                        "01034": "2000.00", "01330": "18000.00", "00562": "4140.00",
                        "01002": "5000.00"
                    }),
                ),
            ),
            con(
                "MODELOS/2024/200.pdf",
                modelo(
                    "200",
                    2024,
                    "0A",
                    json!({
                        "00255": "300000.00", "00550": "-2000.00", "01033": "2000.00",
                        "01330": "0.00", "01002": "7000.00"
                    }),
                ),
            ),
        ],
        date!(2026 - 10 - 31),
    );
    let h = hallazgo(&r, "sociedades.reserva_nivelacion");
    assert_eq!(h.ejercicio, Some(2023));
    assert_eq!(h.gravedad, Gravedad::Info);
    assert!(h.detalle.contains("200 de 2024"), "{}", h.detalle);
    assert!(h.detalle.contains("se puede disponer"), "{}", h.detalle);
    assert!(
        !r.pedir
            .iter()
            .any(|p| p.que.contains("reserva de nivelación"))
    );
}

/// Un extracto MT940 con movimientos (fecha AAMMDD, D/C, importe, concepto).
fn mt940(movs: &[(&str, &str, &str, &str)]) -> Vec<u8> {
    let mut s = String::from(
        "{1:F01DEMOESMMAXXX0000000000}{4:\n:20:DEMO\n:25:ES9121000418450200051332\n:28C:1/1\n\
         :60F:C260101EUR1000,00\n",
    );
    for (f, dc, imp, concepto) in movs {
        s.push_str(&format!(":61:{f}{dc}{imp}NTRFNONREF\n:86:{concepto}\n"));
    }
    s.push_str(":62F:C260630EUR7804,00\n-}");
    s.into_bytes()
}

/// Dos dividendos de 3.402 € cobrados en mayo y junio de 2026 (4.200 € menos el 19 %): una nota
/// para el 200 de 2026 con la exención del art. 21 LIS y la retención que se recupera si se
/// contabilizan en bruto. Antes, la revisión no decía nada hasta que hubiera un 200.
#[test]
fn los_dividendos_cobrados_van_al_proximo_200() {
    let r = revisar(
        vec![(
            "BANCO/2026-1S.sta".into(),
            mt940(&[
                (
                    "260514",
                    "C",
                    "3402,00",
                    "TRANSFERENCIA PARTICIPADA UNO SL DIVIDENDO",
                ),
                (
                    "260619",
                    "C",
                    "3402,00",
                    "TRANSFERENCIA PARTICIPADA DOS SL DIVIDENDO",
                ),
            ]),
            None,
        )],
        date!(2026 - 10 - 31),
    );
    let h = hallazgo(&r, "sociedades.dividendos_cobrados");
    assert_eq!(h.ejercicio, Some(2026));
    assert_eq!(h.gravedad, Gravedad::Aviso);
    assert!(h.detalle.contains("bruto es 8.400,00 €"), "{}", h.detalle);
    assert!(h.detalle.contains("retención 1.596,00 €"), "{}", h.detalle);
    assert_eq!(h.rectificable_hasta, None);
    assert!(h.ficheros.contains(&"BANCO/2026-1S.sta".to_owned()));
}

/// Que la pagadora no retenga sólo vale con al menos el 5 % durante un año (art. 21.1 LIS), y hay
/// que comunicárselo (art. 61.p RIS); con menos, el 19 % es correcto. Antes se le pedía que no
/// retuviera sin más.
#[test]
fn no_retener_solo_con_el_5_por_ciento_durante_un_año() {
    let r = revisar(
        vec![(
            "BANCO/2026-1S.sta".into(),
            mt940(&[("260427", "C", "1215,00", "ABONO DIVIDENDO CARTERA")]),
            None,
        )],
        date!(2026 - 10 - 31),
    );
    let h = hallazgo(&r, "sociedades.dividendos_cobrados");
    assert!(
        h.detalle.contains(
            "Si se tiene al menos el 5 % desde hace un año, comunícaselo con los documentos que \
             lo justifiquen para que no retenga en los próximos repartos (art. 61.p RIS)"
        ),
        "{}",
        h.detalle
    );
    assert!(
        h.detalle.contains("la retención del 19 % es correcta"),
        "{}",
        h.detalle
    );
    assert!(
        !h.detalle
            .contains("y que no retenga en los próximos repartos"),
        "{}",
        h.detalle
    );
}

/// Un dividendo cobrado en 2025 y sin el 200 de 2025 en la carpeta. Pasado su plazo (el
/// 27/07/2026: el 25 fue sábado; art. 124.1 LIS), ese 200 ya tenía que estar presentado: hay que comprobar que
/// aplica la exención y resta la retención y, si no, rectificarlo, con su plazo. Antes del plazo,
/// va en el 200 que se presenta. Antes salía siempre «antes de cerrar el ejercicio» y sin plazo
/// para rectificar.
#[test]
fn el_dividendo_del_año_pasado_con_el_200_ya_vencido() {
    let banco = || {
        (
            "BANCO/2025.sta".to_owned(),
            mt940(&[(
                "250612",
                "C",
                "2430,00",
                "TRANSFERENCIA PARTICIPADA TRES SL DIVIDENDO",
            )]),
            None,
        )
    };
    let r = revisar(vec![banco()], date!(2026 - 10 - 31));
    let h = hallazgo(&r, "sociedades.dividendos_cobrados");
    assert_eq!(h.ejercicio, Some(2025));
    assert!(
        h.detalle.contains(
            "El 200 de 2025 ya tenía que estar presentado (el plazo acabó el 27/07/2026)"
        ),
        "{}",
        h.detalle
    );
    assert!(h.detalle.contains("hasta el 27/07/2030"), "{}", h.detalle);
    assert!(
        !h.detalle.contains("antes de cerrar el ejercicio"),
        "{}",
        h.detalle
    );
    assert_eq!(h.rectificable_hasta.as_deref(), Some("2030-07-27"));
    assert!(!h.rectificable_aprox);
    assert!(
        r.pedir.iter().any(|p| p
            .que
            .starts_with("Comprobar que el 200 de 2025 aplica la exención")),
        "{:?}",
        r.pedir
    );
    // Antes de que acabe el plazo, para el 200 que se va a presentar.
    let r = revisar(vec![banco()], date!(2026 - 05 - 10));
    let h = hallazgo(&r, "sociedades.dividendos_cobrados");
    assert!(
        h.detalle
            .contains("antes de presentar el 200 de 2025 (el plazo acaba el 27/07/2026)"),
        "{}",
        h.detalle
    );
    assert_eq!(h.rectificable_hasta, None);
}

/// El cobro de 10.000 € de PARTICIPADA UNO SL en el banco (marzo de 2026).
fn cobro_participada(importe: &str) -> Fichero {
    (
        "BANCO/2026-1S.sta".into(),
        mt940(&[(
            "260316",
            "C",
            importe,
            "TRANSFERENCIAS | PARTICIPADA UNO S.L. · DIVIDENDO",
        )]),
        None,
    )
}

/// Un acta de PARTICIPADA UNO SL (A12345674) en su carpeta de participadas, con lo que le
/// corresponde a la empresa (EMPRESA DEMO SL, B12345674, el 40 %).
fn acta_participada(nombre: &str, fecha: &str, acuerdo: &str) -> Fichero {
    (
        format!("PARTICIPADAS/PARTICIPADA UNO SL/Actas y dividendos/{nombre}"),
        pdf(vec![
            Para::Title("ACTA DE LA JUNTA GENERAL DE PARTICIPADA UNO SL (A12345674)".into()),
            Para::Text(format!(
                "En Valencia, a {fecha}, asisten D. JUAN PÉREZ en representación de EMPRESA DEMO \
                 SL con CIF {PROPIO}, con un nominal de 1.500,00 euros que representa un 40 % del \
                 capital social, y OTRA SOCIA SL con el otro 60 %."
            )),
            Para::Text(acuerdo.into()),
        ]),
        None,
    )
}

/// Un 200 de 2025 de la empresa con la cifra de negocios y la base imponible, para estimar el
/// impuesto que se ahorra con la exención en el de 2026.
fn doscientos_2025_cifra() -> Fichero {
    con(
        "MODELOS/2025/200.pdf",
        modelo(
            "200",
            2025,
            "0A",
            json!({"00255": "300000.00", "01330": "60000.00", "00500": "60000.00"}),
        ),
    )
}

/// Las dos participadas del grupo: el acta de reparto dice 12.345,68 € brutos para la empresa
/// y el banco, 10.000,00 €: el 19 % retenido (2.345,68 €), que entre sociedades con al menos el
/// 5 % durante un año no se tenía que retener. Un error de la gestoría, con la retención como
/// importe a favor en el 200 de 2026 y, aparte, el impuesto que se ahorra con la exención. No
/// suma a lo recuperable corrigiendo modelos. Antes era un aviso condicional sin importe.
#[test]
fn la_retencion_de_un_dividendo_del_grupo_confirmada_con_el_acta() {
    let r = revisar(
        vec![
            cobro_participada("10000,00"),
            acta_participada(
                "Acta_cuentas_2024.pdf",
                "30 de junio de 2024",
                "PRIMERO. Aprobar las cuentas de 2023 y llevar el resultado a reservas.",
            ),
            acta_participada(
                "Acta_reparto_dividendos_2026.pdf",
                "10 de marzo de 2026",
                "SEGUNDO. A EMPRESA DEMO SL, titular del 40 % del capital social, le corresponde \
                 un dividendo bruto de 12.345,68 € (con cargo a reservas voluntarias).",
            ),
            doscientos_2025_cifra(),
        ],
        date!(2026 - 11 - 01),
    );
    let h = hallazgo(&r, "sociedades.retencion_dividendos_grupo");
    assert_eq!(h.gravedad, Gravedad::Error);
    assert_eq!(h.ejercicio, Some(2026));
    assert_eq!(h.importe.map(|m| m.cents()), Some(234_568));
    assert!(
        h.titulo
            .contains("no se debía practicar: 2.345,68 € adelantados"),
        "{}",
        h.titulo
    );
    assert!(
        h.detalle
            .contains("PARTICIPADA UNO SL: 12.345,68 € brutos, 2.345,68 € retenidos")
            && h.detalle
                .contains("el 40 % según sus documentos desde 2024")
            && h.detalle.contains("art. 128.4.d LIS y art. 61.p RIS")
            && h.detalle
                .contains("se resta en el 200 de 2026 (se presenta hasta el"),
        "{}",
        h.detalle
    );
    assert!(
        h.ficheros
            .iter()
            .any(|f| f.ends_with("Acta_reparto_dividendos_2026.pdf")),
        "{:?}",
        h.ficheros
    );
    assert_eq!(h.rectificable_hasta, None);
    // Sin el aviso condicional de antes.
    assert!(
        !r.hallazgos
            .iter()
            .any(|h| h.codigo == "sociedades.dividendos_cobrados")
    );
    // El 95 % de 12.345,68 € (11.728,40 €) al tipo de 2026 de las de menos de 1 M€ de cifra.
    assert!(
        h.detalle.contains("se dejan de tributar 11.728,40 €"),
        "{}",
        h.detalle
    );
    assert_eq!(r.resumen.retenciones_proximo_200.cents(), 234_568);
    // Sin la exención se perderían 11.728,40 € × 19 % = 2.228,40 €.
    assert_eq!(r.resumen.exencion_proximo_200.cents(), 222_840);
    // Con ella tributa el 5 % (617,28 €) al 19 %: 117,28 €, y se recupera la retención menos eso.
    assert_eq!(r.resumen.impuesto_dividendos_proximo_200.cents(), 11_728);
    assert_eq!(r.resumen.a_favor_proximo_200.cents(), 234_568 - 11_728);
    assert_eq!(r.resumen.recuperable.cents(), 0);
    assert_eq!(r.dividendos.len(), 1);
    assert_eq!(r.dividendos[0].bruto.cents(), 1_234_568);
    assert!(r.dividendos[0].grupo);
    assert!(
        r.pedir
            .iter()
            .any(|p| p.que.contains("certificado de retenciones de 2026")),
        "{:?}",
        r.pedir
    );
    let md = revision::informe::markdown(&r);
    assert!(
        md.contains("En el próximo 200 se recuperan unos 2.228,40 €")
            && md.contains("a favor en el próximo 200"),
        "{md}"
    );
}

/// Sin documentos de la participada que digan el bruto, el aviso de siempre: condicional, sin
/// importe y sin cifra para el próximo 200.
#[test]
fn sin_el_acta_el_dividendo_sigue_siendo_un_aviso_condicional() {
    let r = revisar(vec![cobro_participada("10000,00")], date!(2026 - 11 - 01));
    let h = hallazgo(&r, "sociedades.dividendos_cobrados");
    assert_eq!(h.gravedad, Gravedad::Aviso);
    assert_eq!(h.importe, None);
    assert!(
        h.detalle.contains("el dividendo bruto es 12.345,68 €"),
        "{}",
        h.detalle
    );
    assert!(r.resumen.a_favor_proximo_200.is_zero());
    assert!(r.dividendos.is_empty());
}

/// El acta da como bruto lo mismo que se cobró: sin retención. Sólo la exención, con importe 0.
#[test]
fn un_dividendo_sin_retencion_solo_lleva_la_exencion() {
    let r = revisar(
        vec![
            cobro_participada("10000,00"),
            acta_participada(
                "Acta_cuentas_2024.pdf",
                "30 de junio de 2024",
                "PRIMERO. Aprobar las cuentas de 2023.",
            ),
            acta_participada(
                "Acta_reparto_dividendos_2026.pdf",
                "10 de marzo de 2026",
                "SEGUNDO. A EMPRESA DEMO SL le corresponde un dividendo bruto de 10.000,00 €, sin \
                 retención por tener derecho a la exención.",
            ),
        ],
        date!(2026 - 11 - 01),
    );
    assert!(
        !r.hallazgos
            .iter()
            .any(|h| h.codigo == "sociedades.retencion_dividendos_grupo")
    );
    let h = hallazgo(&r, "sociedades.dividendos_confirmados");
    assert_eq!(h.gravedad, Gravedad::Info);
    assert_eq!(h.importe.map(|m| m.cents()), Some(0));
    assert!(h.titulo.contains("sin retención"), "{}", h.titulo);
    assert!(h.detalle.contains("el 95 % está exento"), "{}", h.detalle);
    assert!(r.resumen.retenciones_proximo_200.is_zero());
}

/// Con el acta de reparto pero sin nada que diga desde cuándo se tiene la participación: no se
/// afirma que no se tenía que retener. Un aviso con la retención, que igualmente se resta.
#[test]
fn sin_acreditar_el_5_por_ciento_no_se_afirma_el_error() {
    let r = revisar(
        vec![
            cobro_participada("10000,00"),
            acta_participada(
                "Acta_reparto_dividendos_2026.pdf",
                "10 de marzo de 2026",
                "SEGUNDO. A EMPRESA DEMO SL le corresponde un dividendo bruto de 12.345,68 €.",
            ),
        ],
        date!(2026 - 11 - 01),
    );
    assert!(
        !r.hallazgos
            .iter()
            .any(|h| h.codigo == "sociedades.retencion_dividendos_grupo")
    );
    let h = hallazgo(&r, "sociedades.dividendos_confirmados");
    assert_eq!(h.gravedad, Gravedad::Aviso);
    assert_eq!(h.importe.map(|m| m.cents()), Some(234_568));
    assert!(
        h.detalle
            .contains("Los documentos no dicen que la empresa tenga al menos el 5 %"),
        "{}",
        h.detalle
    );
    assert_eq!(r.resumen.retenciones_proximo_200.cents(), 234_568);
    assert!(r.resumen.exencion_proximo_200.is_zero());
}

fn escritura() -> Fichero {
    con(
        "SOCIEDAD/constitucion.pdf",
        json!({
            "tipo": "escritura_constitucion",
            "descripcion": "Escritura de constitución de Empresa Demo SL",
            "empresa": {
                "nif": PROPIO, "name": "EMPRESA DEMO SL", "date": "2020-03-02",
                "administrator": "PEREZ GOMEZ JUAN (12345678Z), administrador único"
            }
        }),
    )
}

fn nomina(mes: &str) -> Fichero {
    nomina_con(mes, "2150.00", "245.10", "11.40")
}

/// Una nómina del administrador (con su bloque) con el bruto, la retención y el tipo del IRPF.
fn nomina_con(mes: &str, bruto: &str, retencion: &str, tipo: &str) -> Fichero {
    con(
        &format!("NOMINAS/{mes}.pdf"),
        json!({
            "tipo": "nomina",
            "descripcion": format!("Nómina de {mes} de Juan Pérez Gómez"),
            "nomina": {
                "nif_trabajador": "12345678Z", "nombre_trabajador": "PEREZ GOMEZ JUAN",
                "periodo": mes, "bruto": bruto, "retencion": retencion,
                "tipo_retencion": tipo
            }
        }),
    )
}

/// Una nómina del administrador en PDF, sin importes: sólo su DNI en el texto.
fn nomina_sin_importes(ruta: &str) -> Fichero {
    (
        ruta.into(),
        pdf(vec![
            Para::Text("RECIBO DE SALARIOS. EMPRESA DEMO SL NIF B12345674".into()),
            Para::Text("TRABAJADOR PEREZ GOMEZ JUAN D.N.I. 12345678Z".into()),
            Para::Text("TRIBUTACION I.R.P.F. 12,00".into()),
        ]),
        Some(json!({ "tipo": "nomina", "descripcion": "Nómina del administrador" })),
    )
}

/// El 200 de un año con su cifra de negocios [00255].
fn doscientos_con_cifra(y: i32, cifra: &str) -> Fichero {
    con(
        &format!("MODELOS/{y}/200.pdf"),
        modelo("200", y, "0A", json!({"00255": cifra, "00500": "1000.00"})),
    )
}

fn clave_a(r: &Revision) -> Option<&Hallazgo> {
    r.hallazgos
        .iter()
        .find(|h| h.codigo == "cruces.administrador_clave_a")
}

/// Las nóminas del administrador con la retención fija del cargo (el 19 %, con menos de
/// 100.000 € de cifra de negocios el año anterior) son las de la clave E: no hay aviso. Antes,
/// cualquier nómina contaba como clave A.
#[test]
fn las_nominas_con_la_retencion_del_cargo_no_son_clave_a() {
    let r = revisar(
        vec![
            escritura(),
            nomina_con("2025-01", "2650.00", "503.50", "19"),
            nomina_con("2025-02", "2650.00", "503.50", "19"),
            doscientos_con_cifra(2024, "80000.00"),
        ],
        date!(2026 - 10 - 31),
    );
    assert!(clave_a(&r).is_none(), "{:?}", clave_a(&r));
    // Con más de 100.000 €, el cargo va al 35 %: el 19 % se queda corto.
    let r = revisar(
        vec![
            escritura(),
            nomina_con("2025-01", "2650.00", "503.50", "19"),
            doscientos_con_cifra(2024, "180000.00"),
        ],
        date!(2026 - 10 - 31),
    );
    let h = clave_a(&r).unwrap();
    assert!(
        h.detalle.contains("1T 2025: 424,00 € (al 35 %"),
        "{}",
        h.detalle
    );
}

/// Las nóminas de antes de que lo nombraran administrador son de un empleado: no cuentan. Ni
/// los trimestres de antes del nombramiento.
#[test]
fn las_nominas_de_antes_del_nombramiento() {
    let constitucion = con(
        "SOCIEDAD/constitucion.pdf",
        json!({
            "tipo": "escritura_constitucion",
            "descripcion": "Escritura de constitución de Empresa Demo SL",
            "empresa": {
                "nif": PROPIO, "name": "EMPRESA DEMO SL", "date": "2020-03-02",
                "administrator": "GARCIA LOPEZ ANA (00000001R), administradora única"
            }
        }),
    );
    let nombramiento = con(
        "SOCIEDAD/nombramiento.pdf",
        json!({
            "tipo": "escritura_modificacion",
            "descripcion": "Escritura de nombramiento de administrador de Empresa Demo SL",
            "empresa": {
                "nif": PROPIO, "name": "EMPRESA DEMO SL", "date": "2024-06-01",
                "administrator": "PEREZ GOMEZ JUAN (12345678Z), administrador único"
            }
        }),
    );
    let mut f = vec![
        constitucion,
        nombramiento,
        nomina_con("2023-02", "2100.00", "252.00", "12"),
        nomina_con("2023-03", "2100.00", "252.00", "12"),
        doscientos_con_cifra(2022, "80000.00"),
    ];
    let r = revisar(f.clone(), date!(2026 - 10 - 31));
    assert!(clave_a(&r).is_none(), "{:?}", clave_a(&r));
    // Con una nómina de después, sólo cuenta esa.
    f.push(nomina_con("2024-07", "2100.00", "252.00", "12"));
    f.push(doscientos_con_cifra(2023, "80000.00"));
    let r = revisar(f, date!(2026 - 10 - 31));
    let h = clave_a(&r).unwrap();
    assert!(
        h.detalle.contains("1 nómina de la carpeta de 2024"),
        "{}",
        h.detalle
    );
    assert!(!h.detalle.contains("2023"), "{}", h.detalle);
    assert!(h.detalle.contains("3T 2024: 147,00 €"), "{}", h.detalle);
}

/// Un 111 con un solo perceptor no es del administrador si nada dice que cobrara ese trimestre:
/// aquí sólo hay nóminas suyas de 2021. Antes se le atribuía el 1T 2025.
#[test]
fn el_111_sin_nada_que_diga_que_cobro_el() {
    let r = revisar(
        vec![
            escritura(),
            nomina_con("2021-11", "1700.00", "170.00", "10"),
            doscientos_con_cifra(2024, "80000.00"),
            con(
                "MODELOS/2025/111-1T.pdf",
                modelo(
                    "111",
                    2025,
                    "1T",
                    json!({"01": "1", "02": "5400.00", "03": "540.00", "28": "540.00", "30": "540.00"}),
                ),
            ),
        ],
        date!(2026 - 10 - 31),
    );
    let h = clave_a(&r).unwrap();
    assert!(!h.detalle.contains("1T 2025"), "{}", h.detalle);
    assert!(
        !h.ficheros.contains(&"MODELOS/2025/111-1T.pdf".to_owned()),
        "{:?}",
        h.ficheros
    );
}

/// Con una nómina suya en el trimestre, el 111 de un solo perceptor es suyo, y cuenta lo que
/// cobra en especie: el 19 % de 3.200 € + 450 € (art. 102.1 RIRPF) es 693,50 €; retenidos 352 € y
/// 49,50 € de ingreso a cuenta: faltan 292,00 €. El 111 está entre los ficheros.
#[test]
fn la_retribucion_en_especie_cuenta_en_la_diferencia() {
    let r = revisar(
        vec![
            escritura(),
            nomina_con("2025-02", "1066.67", "117.33", "11"),
            doscientos_con_cifra(2024, "80000.00"),
            con(
                "MODELOS/2025/111-1T.pdf",
                modelo(
                    "111",
                    2025,
                    "1T",
                    json!({"01": "1", "02": "3200.00", "03": "352.00",
                           "04": "1", "05": "450.00", "06": "49.50",
                           "28": "401.50", "30": "401.50"}),
                ),
            ),
        ],
        date!(2026 - 10 - 31),
    );
    let h = clave_a(&r).unwrap();
    assert!(
        h.detalle.contains("1T 2025: 292,00 € (al 19 %"),
        "{}",
        h.detalle
    );
    assert!(
        h.ficheros.contains(&"MODELOS/2025/111-1T.pdf".to_owned()),
        "{:?}",
        h.ficheros
    );
    let t = r
        .tablas
        .iter()
        .find(|t| t.titulo == "Retención del administrador: clave A frente a clave E")
        .unwrap();
    assert_eq!(t.filas[0][1], "3.650,00 €");
    assert_eq!(t.filas[0][2], "401,50 €");
}

/// El primer ejercicio de la sociedad duró seis meses (del 01/07/2024 al 31/12/2024): su cifra de
/// negocios se eleva al año para el tipo del cargo (art. 80.1.3.º RIRPF). 60.000 € en 184 días
/// son más de 100.000 € al año: el 1T 2025 va al 35 %.
#[test]
fn la_cifra_de_negocios_de_un_periodo_corto_se_eleva_al_año() {
    let mut corto = modelo(
        "200",
        2024,
        "0A",
        json!({"00255": "60000.00", "00500": "1000.00"}),
    );
    corto["modelo"]["inicio"] = json!("2024-07-01");
    corto["modelo"]["fin"] = json!("2024-12-31");
    let r = revisar(
        vec![
            escritura(),
            nomina_con("2025-01", "2400.00", "288.00", "12"),
            con("MODELOS/2024/200.pdf", corto),
        ],
        date!(2026 - 10 - 31),
    );
    let h = clave_a(&r).unwrap();
    // 2.400 × 35 % = 840 €, retenidos 288 €: faltan 552 €.
    assert!(
        h.detalle.contains("1T 2025: 552,00 € (al 35 %"),
        "{}",
        h.detalle
    );
}

/// Hay nóminas suyas sin importes en 2024 y una con importes en el 1T 2025: la diferencia del
/// 1T 2025 es «al menos», y el aviso dice qué trimestres con sueldo faltan por calcular y hasta
/// cuándo se puede reclamar el primero. Antes daba el total como si fuera todo.
#[test]
fn los_trimestres_con_sueldo_que_faltan_por_calcular() {
    let r = revisar(
        vec![
            escritura(),
            nomina_sin_importes("NOMINAS/2024/T1/nomina-enero.pdf"),
            nomina_con("2025-01", "2150.00", "245.10", "11.40"),
            doscientos_con_cifra(2023, "80000.00"),
            doscientos_con_cifra(2024, "80000.00"),
        ],
        date!(2026 - 10 - 31),
    );
    let h = clave_a(&r).unwrap();
    assert!(h.detalle.contains("al menos 163,40 €"), "{}", h.detalle);
    assert!(h.detalle.contains("1T 2024 a 4T 2024"), "{}", h.detalle);
    // El 111 del 1T 2024 se presentaba hasta el 22/04/2024 (el 20 era sábado): hasta el
    // 22/04/2028.
    assert!(h.detalle.contains("22/04/2028"), "{}", h.detalle);
    let t = r
        .tablas
        .iter()
        .find(|t| t.titulo == "Retención del administrador: clave A frente a clave E")
        .unwrap();
    assert!(
        t.filas
            .iter()
            .any(|f| f[0] == "2T 2024" && f[5] == "sin calcular"),
        "{:?}",
        t.filas
    );
}

/// El administrador único cobra con nóminas (clave A, retención del 11,40 %). Si es por el cargo,
/// va con la clave E y, con menos de 100.000 € de cifra de negocios en 2024, al 19 %: en el 1T
/// 2025 faltan 1.225,50 − 735,30 = 490,20 €, que Hacienda puede reclamar hasta el 21/04/2029
/// (el plazo del 111 acabó el lunes 21/04/2025).
#[test]
fn el_administrador_que_cobra_como_empleado() {
    let r = revisar(
        vec![
            escritura(),
            nomina("2025-01"),
            nomina("2025-02"),
            nomina("2025-03"),
            con(
                "MODELOS/2024/200.pdf",
                modelo(
                    "200",
                    2024,
                    "0A",
                    json!({"00255": "80000.00", "00500": "1000.00"}),
                ),
            ),
        ],
        date!(2026 - 10 - 31),
    );
    let h = hallazgo(&r, "cruces.administrador_clave_a");
    assert_eq!(h.gravedad, Gravedad::Aviso);
    assert!(
        h.titulo.starts_with("PEREZ GOMEZ JUAN es el administrador"),
        "{}",
        h.titulo
    );
    assert!(
        h.detalle.contains("3 nóminas de la carpeta de 2025"),
        "{}",
        h.detalle
    );
    assert!(h.detalle.contains("11,4 %"), "{}", h.detalle);
    assert!(
        h.detalle.contains("art. 101.2 LIRPF") && h.detalle.contains("217 LSC"),
        "{}",
        h.detalle
    );
    assert!(
        h.detalle.contains("1T 2025: 490,20 € (al 19 %"),
        "{}",
        h.detalle
    );
    assert!(h.detalle.contains("21/04/2029"), "{}", h.detalle);
    assert_eq!(h.importe, None);
    let t = r
        .tablas
        .iter()
        .find(|t| t.titulo == "Retención del administrador: clave A frente a clave E")
        .unwrap();
    assert_eq!(t.filas.len(), 1);
    assert_eq!(t.filas[0][0], "1T 2025");
    assert!(r.pedir.iter().any(|p| p.que.contains("clave E")));
}

/// Con el 111 del trimestre (un solo perceptor de rendimientos del trabajo) y la cifra de
/// negocios de 2022 por encima de 100.000 €, el 4T 2023 va al 35 %. Las nóminas sin importes
/// (sólo el DNI en el texto) bastan para saber que cobra como empleado.
#[test]
fn el_administrador_como_empleado_con_los_111() {
    let nomina_sin_bloque: Fichero = (
        "NOMINAS/2023-12.pdf".into(),
        pdf(vec![
            Para::Text("RECIBO DE SALARIOS. EMPRESA DEMO SL NIF B12345674".into()),
            Para::Text("TRABAJADOR PEREZ GOMEZ JUAN D.N.I. 12345678Z".into()),
            Para::Text("TRIBUTACION I.R.P.F. 12,00        300,00".into()),
        ]),
        Some(json!({ "tipo": "nomina", "descripcion": "Nómina de diciembre de 2023" })),
    );
    let r = revisar(
        vec![
            escritura(),
            nomina_sin_bloque,
            con(
                "MODELOS/2022/200.pdf",
                modelo(
                    "200",
                    2022,
                    "0A",
                    json!({"00255": "250000.00", "00500": "1000.00"}),
                ),
            ),
            con(
                "MODELOS/2023/111-4T.pdf",
                modelo(
                    "111",
                    2023,
                    "4T",
                    json!({"01": "1", "02": "6600.00", "03": "792.00", "28": "792.00", "30": "792.00"}),
                ),
            ),
        ],
        date!(2026 - 10 - 31),
    );
    let h = hallazgo(&r, "cruces.administrador_clave_a");
    assert!(
        h.detalle
            .contains("1 nómina de la carpeta de 2023 con una retención del IRPF del 12 %"),
        "{}",
        h.detalle
    );
    // 6.600 × 35 % = 2.310 €, retenidos 792 €: faltan 1.518 €, reclamables hasta el 22/01/2028
    // (el plazo del 111 del 4T acabó el lunes 22/01/2024: el 20 era sábado).
    assert!(
        h.detalle.contains(
            "4T 2023: 1.518,00 € (al 35 %; Hacienda puede reclamarla hasta el 22/01/2028)"
        ),
        "{}",
        h.detalle
    );
}

/// El administrador dado de alta en Nóminas de TaxPain sin marcarlo como administrador: sus
/// recibos van a la clave A. Con ellos sale la diferencia del trimestre.
#[test]
fn el_administrador_en_las_nominas_de_taxpain() {
    use taxpain_app::payroll;
    use taxpain_core::money::Money;

    let v = vault();
    let ahora = datetime!(2025-01-02 9:00 UTC);
    let id = payroll::save_employee(
        &v,
        &payroll::Employee {
            nif: "12345678Z".into(),
            name: "PEREZ GOMEZ, JUAN".into(),
            province: "46".into(),
            birth_year: 1980,
            family: 3,
            contract: 1,
            administrator: false,
            active: true,
            ..Default::default()
        },
        ahora,
    )
    .unwrap();
    for d in [
        date!(2025 - 01 - 31),
        date!(2025 - 02 - 28),
        date!(2025 - 03 - 31),
    ] {
        payroll::register(
            &v,
            &payroll::PayslipInput {
                employee_id: id,
                date: d,
                gross: Money::from_euros(2_150, 0),
                irpf: Money::from_cents(24_510),
                ss_employee: Money::ZERO,
                ss_company: Money::ZERO,
                small_entity: None,
            },
            ahora,
        )
        .unwrap();
    }
    let r = revisar_con(
        &v,
        vec![
            escritura(),
            con(
                "MODELOS/2024/200.pdf",
                modelo(
                    "200",
                    2024,
                    "0A",
                    json!({"00255": "80000.00", "00500": "1000.00"}),
                ),
            ),
        ],
        date!(2026 - 10 - 31),
    );
    let h = hallazgo(&r, "cruces.administrador_clave_a");
    assert!(h.detalle.contains("Nóminas de TaxPain"), "{}", h.detalle);
    assert!(h.detalle.contains("1T 2025: 490,20 €"), "{}", h.detalle);
}

/// Con el acta que aprueba las cuentas en la carpeta, el depósito vence un mes después de su
/// fecha (art. 279.1 LSC): con el acta del 15/05/2026, el 15/06/2026, y el depósito del
/// 20/07/2026 llega tarde (con el plazo general, el 30/07/2026, habría salido en plazo). El texto
/// dice cómo queda la multa si se deposita antes del procedimiento (art. 283.3 LSC) y cuándo
/// prescribe (art. 283.4 LSC).
#[test]
fn el_deposito_de_cuentas_vence_un_mes_despues_del_acta() {
    let acta = json!({
        "tipo": "acta_junta",
        "descripcion": "Acta de la junta general ordinaria de 15/05/2026",
        "empresa": { "name": "EMPRESA DEMO SL", "date": "2026-05-15" },
        "acta": {
            "ejercicio": 2025, "fecha": "2026-05-15", "resultado": "25000.00",
            "reserva_legal": "1000.00", "reserva_nivelacion": "3000.00",
            "reservas_voluntarias": "21000.00"
        }
    });
    let deposito = json!({
        "tipo": "registro_mercantil",
        "descripcion": "Justificante del depósito de las cuentas de 2025",
        "empresa": { "name": "EMPRESA DEMO SL", "date": "2026-07-20" },
        "registro": { "tramite": "deposito_cuentas", "ejercicio": 2025, "presentado": "2026-07-20" }
    });
    let r = revisar(
        vec![
            doscientos_2025(),
            con("CONTABILIDAD/2025/acta.pdf", acta),
            con("REGISTRO/deposito-2025.pdf", deposito),
        ],
        date!(2026 - 10 - 31),
    );
    let h = hallazgo(&r, "cruces.deposito_fuera_de_plazo");
    assert_eq!(h.ejercicio, Some(2025));
    assert!(
        h.detalle.contains("el plazo acababa el 15/06/2026"),
        "{}",
        h.detalle
    );
    assert!(h.detalle.contains("acta del 15/05/2026"), "{}", h.detalle);
    assert!(h.detalle.contains("art. 279.1 LSC"), "{}", h.detalle);
    assert!(
        h.detalle
            .contains("grado mínimo y reducida un 50 % (art. 283.3 LSC)"),
        "{}",
        h.detalle
    );
    assert!(
        h.detalle.contains("tres años (art. 283.4 LSC)"),
        "{}",
        h.detalle
    );
    // Con más de 6.000.000 € de facturación, hasta 300.000 € por año de retraso (art. 283.1 LSC,
    // segundo párrafo).
    assert!(
        h.detalle.contains(
            "hasta 300.000 € por cada año de retraso si la facturación anual pasa de \
                       6.000.000 € (art. 283.1 LSC)"
        ),
        "{}",
        h.detalle
    );
    assert!(!h.detalle.contains("suele evitar"), "{}", h.detalle);
    let t = r
        .tablas
        .iter()
        .find(|t| t.titulo == "Depósito de cuentas y legalización de libros")
        .unwrap();
    assert!(
        t.filas
            .iter()
            .any(|f| f[0] == "2025" && f[2] == "15/06/2026" && f[4].starts_with("fuera de plazo")),
        "{:?}",
        t.filas
    );
}

fn recibida(
    numero: &str,
    nif_destinatario: Option<&str>,
    nombre_destinatario: Option<&str>,
) -> Value {
    json!({
        "tipo": "factura_recibida",
        "descripcion": format!("Factura {numero} de Tienda Demo SL"),
        "factura": {
            "nif_emisor": "B22222228", "nombre_emisor": "Tienda Demo SL",
            "nif_destinatario": nif_destinatario, "nombre_destinatario": nombre_destinatario,
            "numero": numero, "fecha": "2025-05-10",
            "base": "100.00", "iva": "21.00", "total": "121.00"
        }
    })
}

/// Las facturas recibidas a nombre del administrador (por su DNI o por su nombre, según la
/// escritura) salen aparte de las que están a nombre de otro; sin NIF del destinatario se compara
/// su nombre con el de la sociedad, y sin destinatario no se dice nada (puede ser una
/// simplificada) ni de un nombre sin forma social (puede ser el comercial). Antes, sin NIF del
/// destinatario no se miraba el nombre y la del administrador salía como «a nombre de otro».
#[test]
fn facturas_a_nombre_del_administrador_o_de_otro() {
    let r = revisar(
        vec![
            escritura(),
            con("GASTOS/1.pdf", recibida("1", Some("12345678Z"), None)),
            con(
                "GASTOS/2.pdf",
                recibida("2", None, Some("Juan Pérez Gómez")),
            ),
            con(
                "GASTOS/3.pdf",
                recibida("3", None, Some("Proveedora Ajena SL")),
            ),
            con(
                "GASTOS/4.pdf",
                recibida("4", None, Some("EMPRESA DEMO, S.L.")),
            ),
            con("GASTOS/5.pdf", recibida("5", None, None)),
            con(
                "GASTOS/6.pdf",
                recibida("6", Some("00000000T"), Some("Persona Ajena")),
            ),
            con("GASTOS/7.pdf", recibida("7", Some(PROPIO), None)),
            // Sin NIF y sin forma social: puede ser el nombre comercial de la empresa.
            con("GASTOS/8.pdf", recibida("8", None, Some("Café Central"))),
        ],
        date!(2026 - 10 - 31),
    );
    let h = hallazgo(&r, "facturas.a_nombre_del_administrador");
    assert_eq!(h.ficheros, ["GASTOS/1.pdf", "GASTOS/2.pdf"]);
    assert!(
        h.detalle.starts_with(
            "Están a nombre de PEREZ GOMEZ JUAN (el administrador según las escrituras)"
        ),
        "{}",
        h.detalle
    );
    let h = hallazgo(&r, "facturas.a_nombre_de_otro");
    assert_eq!(h.ficheros, ["GASTOS/3.pdf", "GASTOS/6.pdf"]);
    assert!(
        h.detalle
            .contains("Una está a nombre de una persona física"),
        "{}",
        h.detalle
    );
}

/// Sin el justificante del depósito de las cuentas de 2025, el aviso dice la multa del art. 283.1
/// LSC entera: también el límite de 300.000 € por año de retraso con más de 6.000.000 € de
/// facturación.
#[test]
fn el_deposito_sin_justificante_con_la_multa_entera() {
    let r = revisar(
        vec![con(
            "MODELOS/2025/303-1T.pdf",
            modelo("303", 2025, "1T", json!({"71": "100.00"})),
        )],
        date!(2026 - 10 - 31),
    );
    let h = hallazgo(&r, "cruces.deposito_sin_justificante");
    assert!(h.titulo.contains("2025"), "{}", h.titulo);
    assert!(
        h.detalle.contains(
            "hasta 300.000 € por cada año de retraso si la facturación anual pasa de 6.000.000 € \
             (art. 283.1 LSC)"
        ),
        "{}",
        h.detalle
    );
}

/// Con la escritura guardada en TaxPain, el texto dice «(en TaxPain)» con su mayúscula. Antes
/// salía «(en taxpain)».
#[test]
fn la_escritura_guardada_en_taxpain_se_nombra_bien() {
    let v = vault();
    let facts = taxpain_app::documentos::Facts {
        date: Some(date!(2020 - 03 - 02)),
        administrator: Some("PEREZ GOMEZ JUAN (12345678Z), administrador único".into()),
        ..Default::default()
    };
    taxpain_app::documentos::store(
        &v,
        "escritura.pdf",
        b"%PDF-escritura",
        &taxpain_app::documentos::Recognized {
            kind: taxpain_app::documentos::DocKind::EscrituraConstitucion,
            title: "Escritura de constitución".into(),
            facts,
            by_name: false,
        },
        datetime!(2026-01-10 10:00 UTC),
    )
    .unwrap();
    let r = revisar_con(
        &v,
        vec![nomina("2025-01"), doscientos_con_cifra(2024, "80000.00")],
        date!(2026 - 10 - 31),
    );
    let h = clave_a(&r).unwrap();
    assert!(
        h.detalle
            .starts_with("Según escritura de constitución (en TaxPain) del 02/03/2020"),
        "{}",
        h.detalle
    );
}

/// Un pago de nómina a su nombre en el banco dice que cobró en el trimestre: el 111 de un solo
/// perceptor de ese trimestre es suyo (sin el pago, no; ver `el_111_sin_nada_que_diga_que_cobro_el`).
#[test]
fn el_pago_de_la_nomina_en_el_banco_dice_que_cobro_el() {
    let r = revisar(
        vec![
            escritura(),
            nomina_con("2021-11", "1700.00", "170.00", "10"),
            doscientos_con_cifra(2024, "80000.00"),
            con(
                "MODELOS/2025/111-1T.pdf",
                modelo(
                    "111",
                    2025,
                    "1T",
                    json!({"01": "1", "02": "5400.00", "03": "540.00", "28": "540.00", "30": "540.00"}),
                ),
            ),
            (
                "BANCO/2025.sta".into(),
                mt940(&[(
                    "250205",
                    "D",
                    "1530,00",
                    "TRASPASO PEREZ GOMEZ JUAN NOMINA ENERO",
                )]),
                None,
            ),
        ],
        date!(2026 - 10 - 31),
    );
    let h = clave_a(&r).unwrap();
    // 5.400 × 19 % = 1.026 €, retenidos 540 €: faltan 486 €.
    assert!(h.detalle.contains("1T 2025: 486,00 €"), "{}", h.detalle);
    assert!(
        h.ficheros.contains(&"BANCO/2025.sta".to_owned()),
        "{:?}",
        h.ficheros
    );
}

/// La sociedad lleva los apellidos del administrador («Pérez Gómez Consultores SL») y la factura
/// está a nombre de él, sin NIF: es suya, no de la sociedad. Antes, compartir los apellidos con la
/// sociedad bastaba para darla por buena. Un nombre que está entero en el de la sociedad sí es
/// la sociedad.
#[test]
fn la_factura_del_administrador_con_los_apellidos_de_la_sociedad() {
    let v = vault_de("Pérez Gómez Consultores SL");
    let r = revisar_con(
        &v,
        vec![
            escritura(),
            con(
                "GASTOS/1.pdf",
                recibida("1", None, Some("Juan Pérez Gómez")),
            ),
            con("GASTOS/2.pdf", recibida("2", None, Some("Pérez Gómez"))),
        ],
        date!(2026 - 10 - 31),
    );
    let h = hallazgo(&r, "facturas.a_nombre_del_administrador");
    assert_eq!(h.ficheros, ["GASTOS/1.pdf"]);
}

/// El administrador que sólo está en Nóminas de TaxPain («APELLIDOS, NOMBRE»): el texto lo nombra
/// entero y dice de dónde sale. Antes cortaba el nombre en la coma y decía «según las
/// escrituras».
#[test]
fn el_administrador_de_nominas_con_su_nombre_entero() {
    use taxpain_app::payroll;
    let v = vault();
    payroll::save_employee(
        &v,
        &payroll::Employee {
            nif: "12345678Z".into(),
            name: "PEREZ GOMEZ, JUAN".into(),
            province: "46".into(),
            birth_year: 1980,
            family: 3,
            contract: 1,
            administrator: true,
            active: true,
            ..Default::default()
        },
        datetime!(2025-01-02 9:00 UTC),
    )
    .unwrap();
    let r = revisar_con(
        &v,
        vec![con(
            "GASTOS/1.pdf",
            recibida("1", Some("12345678Z"), Some("Juan Pérez")),
        )],
        date!(2026 - 10 - 31),
    );
    let h = hallazgo(&r, "facturas.a_nombre_del_administrador");
    assert!(
        h.detalle.starts_with(
            "Está a nombre de PEREZ GOMEZ, JUAN (el administrador según Nóminas de TaxPain)"
        ),
        "{}",
        h.detalle
    );
}
