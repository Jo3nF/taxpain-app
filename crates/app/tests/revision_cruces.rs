//! Cruces de la revisión de la carpeta («segunda opinión» sobre la gestoría): 390 frente a los
//! 303 y cadena de compensación, resúmenes anuales frente a los trimestrales, 347/349/190 por
//! NIF, 200 frente a 202, retenciones y cuentas, plazos y recargos, depósito y legalización,
//! pagos del banco y socios y administradores. Datos inventados, NIF de prueba.

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};
use taxpain_app::companies::{self, CompanyInfo};
use taxpain_app::revision::{self, Gravedad, Hallazgo, Revision};
use taxpain_core::money::Money;
use taxpain_invoicing::pdf::{Para, text_document};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

const PROPIO: &str = "B98765431";
const ASESORES: &str = "B11111119";
const PROVEEDOR: &str = "B12345674";
const CLIENTE: &str = "B22222228";

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

fn pdf(texto: &str) -> Vec<u8> {
    text_document("Documento", &[vec![Para::Text(texto.into())]]).unwrap()
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

fn recibida(nif: &str, nombre: &str, numero: &str, fecha: &str, importes: [&str; 3]) -> Value {
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

fn emitida(nif: &str, numero: &str, fecha: &str, importes: [&str; 3]) -> Value {
    json!({
        "tipo": "factura_emitida",
        "descripcion": format!("Factura {numero}"),
        "factura": {
            "nif_emisor": PROPIO, "nombre_emisor": "Empresa Demo SL",
            "nif_destinatario": nif, "nombre_destinatario": "Cliente Demo",
            "numero": numero, "fecha": fecha,
            "base": importes[0], "iva": importes[1], "total": importes[2]
        }
    })
}

type Fichero = (String, Vec<u8>, Option<Value>);

/// Un fichero con su lectura (el PDF lleva un texto distinto para que no sean copias).
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
                r.hallazgos.iter().map(|h| &h.codigo).collect::<Vec<_>>()
            )
        })
}

fn hay(r: &Revision, codigo: &str) -> bool {
    r.hallazgos.iter().any(|h| h.codigo == codigo)
}

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

/// Los cuatro 303 de 2025: el 2T sale a compensar 300 € y el 3T sólo arrastra 100 €; el 4T
/// arrastra 500 € cuando no quedaba nada.
fn trescientos_tres() -> Vec<Fichero> {
    vec![
        con(
            "MODELOS/303-1T.pdf",
            modelo(
                "303",
                2025,
                "1T",
                json!({"27": "2100.00", "45": "1100.00", "46": "1000.00", "71": "1000.00"}),
            ),
        ),
        con(
            "MODELOS/303-2T.pdf",
            modelo(
                "303",
                2025,
                "2T",
                json!({"27": "700.00", "45": "1000.00", "46": "-300.00", "71": "-300.00"}),
            ),
        ),
        con(
            "MODELOS/303-3T.pdf",
            modelo(
                "303",
                2025,
                "3T",
                json!({"27": "2000.00", "45": "1000.00", "46": "1000.00", "110": "100.00", "78": "100.00", "71": "900.00"}),
            ),
        ),
        con(
            "MODELOS/303-4T.pdf",
            modelo(
                "303",
                2025,
                "4T",
                json!({"27": "2000.00", "45": "1000.00", "46": "1000.00", "110": "500.00", "78": "500.00", "71": "500.00"}),
            ),
        ),
    ]
}

#[test]
fn el_390_frente_a_los_303_y_la_cadena_de_compensacion() {
    let mut f = trescientos_tres();
    // El 390 dice 3.000 € de resultado y los 303 suman 2.700 €.
    f.push(con(
        "MODELOS/390.pdf",
        modelo(
            "390",
            2025,
            "0A",
            json!({"47": "6800.00", "64": "4100.00", "65": "3000.00"}),
        ),
    ));
    let r = revisar(f);
    let h = hallazgo(&r, "cruces.390_303");
    assert!(
        h.detalle.contains("[65]") && h.detalle.contains("300,00 €"),
        "{}",
        h.detalle
    );
    assert!(
        !h.detalle.contains("[47]"),
        "las cuotas devengadas casan: {}",
        h.detalle
    );
    // 2T → 3T: quedaban 300 € a compensar y sólo se arrastran 100 €.
    let h = hallazgo(&r, "cruces.303_saldo_perdido");
    assert_eq!(h.importe, Some(eur(200)));
    assert!(h.titulo.contains("3T 2025"), "{}", h.titulo);
    // 3T → 4T: no quedaba nada y el 4T compensa 500 €.
    let h = hallazgo(&r, "cruces.303_compensa_de_mas");
    assert_eq!(h.gravedad, Gravedad::Error);
    assert!(h.detalle.contains("500,00 €"), "{}", h.detalle);
}

/// Un 303 con resultado 0 no trae la [71] (las casillas a cero no van en el fichero) y el
/// siguiente tampoco la [110]: la cadena se comprueba igual, con esas casillas a 0.
#[test]
fn la_cadena_sigue_con_un_303_a_cero() {
    let r = revisar(vec![
        con(
            "MODELOS/303-1T.pdf",
            modelo(
                "303",
                2026,
                "1T",
                json!({"27": "1000.00", "45": "1000.00", "110": "140.00", "87": "140.00"}),
            ),
        ),
        con(
            "MODELOS/303-2T.pdf",
            modelo(
                "303",
                2026,
                "2T",
                json!({"27": "2000.00", "45": "1000.00", "46": "1000.00", "71": "1000.00"}),
            ),
        ),
        // Sin casillas leídas: no se compara con él (no hay aviso falso del 2T al 3T).
        con("MODELOS/303-3T.pdf", modelo("303", 2026, "3T", json!({}))),
    ]);
    let h = hallazgo(&r, "cruces.303_saldo_perdido");
    assert_eq!(h.importe, Some(eur(140)));
    assert!(h.titulo.contains("2T 2026"), "{}", h.titulo);
    assert_eq!(
        r.hallazgos
            .iter()
            .filter(|h| h.codigo == "cruces.303_saldo_perdido")
            .count(),
        1
    );
    let t = r
        .tablas
        .iter()
        .find(|t| t.titulo.starts_with("Cadena de cuotas"))
        .expect("tabla de la cadena");
    assert!(
        t.filas.iter().any(|f| f[0] == "1T 2026 → 2T 2026"),
        "{:?}",
        t.filas
    );
}

#[test]
fn el_390_sin_todos_los_303_no_se_cuadra() {
    let mut f = trescientos_tres();
    f.remove(0);
    f.push(con(
        "MODELOS/390.pdf",
        modelo("390", 2025, "0A", json!({"65": "3000.00"})),
    ));
    let r = revisar(f);
    assert!(!hay(&r, "cruces.390_303"));
    assert!(hallazgo(&r, "cruces.390_faltan_303").detalle.contains("1T"));
}

#[test]
fn resumenes_anuales_y_perceptores() {
    let mut f: Vec<Fichero> = (1..=4)
        .map(|q| {
            con(
                &format!("MODELOS/111-{q}T.pdf"),
                modelo(
                    "111",
                    2025,
                    &format!("{q}T"),
                    json!({"09": "150.00", "28": "150.00", "30": "150.00"}),
                ),
            )
        })
        .collect();
    let mut asesores = recibida(
        ASESORES,
        "Asesores Demo SL",
        "A-1",
        "2025-03-10",
        ["1000.00", "210.00", "1060.00"],
    );
    asesores["factura"]["retencion"] = json!("150.00");
    f.push(con("GASTOS/asesores.pdf", asesores));
    let mut otro = recibida(
        PROVEEDOR,
        "Diseño Demo SL",
        "D-1",
        "2025-06-10",
        ["1000.00", "210.00", "1060.00"],
    );
    otro["factura"]["retencion"] = json!("150.00");
    f.push(con("GASTOS/diseno.pdf", otro));
    // El 190 sólo declara a los asesores (150 €) y los 111 suman 600 €.
    let mut m190 = modelo(
        "190",
        2025,
        "0A",
        json!({"01": "1", "02": "1000.00", "03": "150.00"}),
    );
    m190["modelo"]["registros"] = json!([
        {"nif": ASESORES, "nombre": "Asesores Demo SL", "clave": "G", "importe": "1000.00", "retencion": "150.00"}
    ]);
    f.push(con("MODELOS/190.pdf", m190));
    let r = revisar(f);
    let h = hallazgo(&r, "cruces.resumen_anual");
    assert!(
        h.titulo.contains("190") && h.detalle.contains("600,00 €"),
        "{}",
        h.detalle
    );
    let h = hallazgo(&r, "cruces.190_falta");
    assert!(h.detalle.contains(PROVEEDOR), "{}", h.detalle);
    assert!(!h.detalle.contains(ASESORES), "{}", h.detalle);
}

/// Dos 111 del 1T: uno a cero presentado el 10/04 y su sustitutiva del 15/04 con los 150 €. Vale
/// la vigente (la presentada después), aunque su fichero se lea antes: el 190 cuadra con los
/// 600 € del año. Un 111 complementario del que sólo se lee el resultado [30] suma lo ingresado
/// antes [29]. Antes valía la última por nombre de fichero (la sustituida) y salía un descuadre.
#[test]
fn el_111_vigente_frente_al_190() {
    let con_fecha = |ruta: &str, periodo: &str, casillas: Value, presentado: &str| {
        let mut l = modelo("111", 2025, periodo, casillas);
        l["modelo"]["presentado"] = json!(presentado);
        con(ruta, l)
    };
    let f = vec![
        con_fecha(
            "MODELOS/111-1T-a.pdf",
            "1T",
            json!({"09": "150.00", "28": "150.00", "30": "150.00"}),
            "2025-04-15",
        ),
        con_fecha(
            "MODELOS/111-1T-b.pdf",
            "1T",
            json!({"09": "0.00", "28": "0.00", "30": "0.00"}),
            "2025-04-10",
        ),
        con_fecha(
            "MODELOS/111-2T.pdf",
            "2T",
            json!({"28": "150.00", "30": "150.00"}),
            "2025-07-15",
        ),
        // Complementaria del 3T: 100 € ingresados antes y 50 € ahora.
        con_fecha(
            "MODELOS/111-3T.pdf",
            "3T",
            json!({"29": "100.00", "30": "50.00"}),
            "2025-10-30",
        ),
        con_fecha(
            "MODELOS/111-4T.pdf",
            "4T",
            json!({"28": "150.00", "30": "150.00"}),
            "2026-01-15",
        ),
        con(
            "MODELOS/190.pdf",
            modelo(
                "190",
                2025,
                "0A",
                json!({"01": "1", "02": "4000.00", "03": "600.00"}),
            ),
        ),
    ];
    let r = revisar(f);
    assert!(
        !hay(&r, "cruces.resumen_anual"),
        "{:?}",
        r.hallazgos
            .iter()
            .filter(|h| h.codigo == "cruces.resumen_anual")
            .map(|h| &h.detalle)
            .collect::<Vec<_>>()
    );
    let t = r
        .tablas
        .iter()
        .find(|t| t.titulo == "Resúmenes anuales frente a los modelos del año")
        .unwrap();
    assert_eq!(t.filas[0][3], "600,00 €", "{:?}", t.filas);
}

/// El 190 de 2024 y su complementaria, que sólo trae el perceptor que faltaba (art. 11.2 de la
/// Orden EHA/3127/2009): el 190 es la suma de los dos (3.000 € + 300 €) y cuadra con los 111.
/// Lo mismo si la complementaria no dice que lo es pero sus perceptores son otros. Una
/// sustitutiva, en cambio, reemplaza a la anterior. Antes valía sólo la presentada después.
#[test]
fn el_190_con_su_complementaria() {
    let perceptor = |nif: &str, nombre: &str, retencion: &str| json!({"nif": nif, "nombre": nombre, "clave": "A", "importe": "10000.00", "retencion": retencion});
    let un_190 = |ruta: &str, casillas: Value, presentado: &str, registros: Value| {
        let mut l = modelo("190", 2024, "0A", casillas);
        l["modelo"]["presentado"] = json!(presentado);
        l["modelo"]["registros"] = registros;
        con(ruta, l)
    };
    let original = || {
        un_190(
            "MODELOS/2024/190.pdf",
            json!({"01": "2", "02": "20000.00", "03": "3000.00"}),
            "2025-01-20",
            json!([
                perceptor("00000001R", "Ana García López", "1500.00"),
                perceptor("00000002W", "Luis Martín Ruiz", "1500.00"),
            ]),
        )
    };
    let segunda = |tipo: Option<&str>, registros: Value| {
        let (ruta, bytes, mut l) = un_190(
            "MODELOS/2024/190-2.pdf",
            json!({"01": "1", "02": "10000.00", "03": "300.00"}),
            "2025-03-01",
            registros,
        );
        if let (Some(t), Some(l)) = (tipo, l.as_mut()) {
            l["modelo"]["tipo"] = json!(t);
        }
        (ruta, bytes, l)
    };
    let ficheros = |segunda: Fichero| {
        let mut f: Vec<Fichero> = (1..=4)
            .map(|q| {
                con(
                    &format!("MODELOS/2024/111-{q}T.pdf"),
                    modelo(
                        "111",
                        2024,
                        &format!("{q}T"),
                        json!({"28": "825.00", "30": "825.00"}),
                    ),
                )
            })
            .collect();
        f.push(segunda);
        f.push(original());
        f
    };
    let total_190 = |r: &Revision| {
        r.tablas
            .iter()
            .find(|t| t.titulo == "Resúmenes anuales frente a los modelos del año")
            .and_then(|t| t.filas.iter().find(|f| f[1] == "190 / 111"))
            .map(|f| f[2].clone())
    };
    let nuevo = json!([perceptor("00000003A", "Eva Sanz Gil", "300.00")]);
    // Con su tipo.
    let r = revisar(ficheros(segunda(Some("complementaria"), nuevo.clone())));
    assert!(!hay(&r, "cruces.resumen_anual"), "{:?}", total_190(&r));
    assert_eq!(total_190(&r).as_deref(), Some("3.300,00 €"));
    // Sin el tipo, con otro perceptor.
    let r = revisar(ficheros(segunda(None, nuevo)));
    assert!(!hay(&r, "cruces.resumen_anual"), "{:?}", total_190(&r));
    // La sustitutiva con todos: 3.300 € también, pero porque los trae ella.
    let todos = json!([
        perceptor("00000001R", "Ana García López", "1500.00"),
        perceptor("00000002W", "Luis Martín Ruiz", "1500.00"),
        perceptor("00000003A", "Eva Sanz Gil", "300.00"),
    ]);
    let (ruta, bytes, mut l) = segunda(Some("sustitutiva"), todos);
    if let Some(l) = l.as_mut() {
        l["modelo"]["casillas"] = json!({"01": "3", "02": "30000.00", "03": "3300.00"});
    }
    let r = revisar(ficheros((ruta, bytes, l)));
    assert_eq!(total_190(&r).as_deref(), Some("3.300,00 €"));
    assert!(!hay(&r, "cruces.resumen_anual"));
}

#[test]
fn el_347_y_el_349_por_nif() {
    let f = vec![
        con(
            "GASTOS/proveedor.pdf",
            recibida(
                PROVEEDOR,
                "Proveedor Demo SL",
                "P-1",
                "2025-05-10",
                ["4000.00", "840.00", "4840.00"],
            ),
        ),
        con(
            "INGRESOS/a1.pdf",
            emitida(
                CLIENTE,
                "A-1",
                "2025-02-10",
                ["5000.00", "1050.00", "6050.00"],
            ),
        ),
        con(
            "INGRESOS/a2.pdf",
            emitida(
                "FR12345678901",
                "A-2",
                "2025-02-20",
                ["2000.00", "0", "2000.00"],
            ),
        ),
        {
            // El 347 declara al cliente por menos y no trae al proveedor.
            let mut m = modelo("347", 2025, "0A", json!({}));
            m["modelo"]["registros"] = json!([
                {"nif": CLIENTE, "nombre": "Cliente Demo", "clave": "B", "importe": "5000.00"}
            ]);
            con("MODELOS/347.pdf", m)
        },
        {
            // El 349 del 1T no trae la entrega al cliente francés.
            let mut m = modelo("349", 2025, "1T", json!({"02": "0"}));
            m["modelo"]["registros"] = json!([
                {"nif": "DE123456789", "nombre": "Kunde Demo", "clave": "S", "importe": "100.00"}
            ]);
            con("MODELOS/349-1T.pdf", m)
        },
    ];
    let r = revisar(f);
    assert!(hallazgo(&r, "cruces.347_falta").detalle.contains(PROVEEDOR));
    let h = hallazgo(&r, "cruces.347_de_menos");
    assert!(h.detalle.contains("6.050,00 €"), "{}", h.detalle);
    assert!(
        hallazgo(&r, "cruces.349_falta")
            .detalle
            .contains("FR12345678901")
    );
    assert!(
        hallazgo(&r, "cruces.349_sin_facturas")
            .detalle
            .contains("DE123456789")
    );
}

#[test]
fn el_200_frente_a_los_202_las_retenciones_y_las_cuentas() {
    let mut venta = emitida(
        CLIENTE,
        "A-9",
        "2025-06-01",
        ["2000.00", "420.00", "2120.00"],
    );
    venta["factura"]["retencion"] = json!("300.00");
    let f = vec![
        con(
            "MODELOS/200.pdf",
            modelo(
                "200",
                2025,
                "0A",
                json!({"00500": "10000.00", "00255": "80000.00", "00601": "500.00", "00603": "0", "01766": "100.00", "01586": "900.00"}),
            ),
        ),
        con(
            "MODELOS/202-1P.pdf",
            modelo("202", 2025, "1P", json!({"03": "500.00"})),
        ),
        con(
            "MODELOS/202-2P.pdf",
            modelo("202", 2025, "2P", json!({"03": "400.00"})),
        ),
        con("INGRESOS/a9.pdf", venta),
        con(
            "REGISTRO/deposito-2024.pdf",
            json!({
                "tipo": "cuentas_anuales",
                "descripcion": "Justificante del depósito de cuentas 2025",
                "registro": {"tramite": "deposito_cuentas", "ejercicio": 2025, "presentado": "2026-07-20",
                             "resultado": "12000.00", "cifra_negocios": "80000.00"}
            }),
        ),
    ];
    let r = revisar(f);
    let h = hallazgo(&r, "cruces.200_202_sin_descontar");
    assert_eq!(h.importe, Some(eur(400)));
    assert!(h.titulo.contains("2P"), "{}", h.titulo);
    assert_eq!(
        hallazgo(&r, "cruces.200_retenciones_sin_deducir").importe,
        Some(eur(200))
    );
    let h = hallazgo(&r, "cruces.200_cuentas");
    assert!(
        h.detalle.contains("[00500]") && !h.detalle.contains("[00255]"),
        "{}",
        h.detalle
    );
    // Lo recuperable del resumen incluye el pago fraccionado y las retenciones.
    assert!(
        r.resumen.recuperable >= eur(600),
        "{:?}",
        r.resumen.recuperable
    );
}

#[test]
fn presentaciones_fuera_de_plazo_con_su_recargo() {
    let mut m = modelo("303", 2025, "1T", json!({"71": "1000.00"}));
    // Plazo hasta el 20 de abril de 2025 (domingo → 21); presentado el 25 de junio: 2 meses
    // completos → 3 %.
    m["modelo"]["presentado"] = json!("2025-06-25");
    let mut a_tiempo = modelo("111", 2025, "1T", json!({"30": "100.00"}));
    a_tiempo["modelo"]["presentado"] = json!("2025-04-15");
    let r = revisar(vec![
        con("MODELOS/303-1T.pdf", m),
        con("MODELOS/111-1T.pdf", a_tiempo),
    ]);
    let h = hallazgo(&r, "cruces.fuera_de_plazo");
    assert_eq!(h.importe, Some(eur(-30)), "{}", h.detalle);
    assert!(
        h.detalle.contains("3,00 %") || h.detalle.contains("3 %"),
        "{}",
        h.detalle
    );
    assert_eq!(
        r.hallazgos
            .iter()
            .filter(|h| h.codigo == "cruces.fuera_de_plazo")
            .count(),
        1
    );
}

#[test]
fn deposito_y_legalizacion_por_ejercicio() {
    let f = vec![
        con(
            "GASTOS/p.pdf",
            recibida(
                PROVEEDOR,
                "Proveedor Demo SL",
                "P-1",
                "2024-05-10",
                ["100.00", "21.00", "121.00"],
            ),
        ),
        // Depósito de 2024 en septiembre de 2025: fuera de plazo (acababa a finales de julio).
        // Con las marcas del Registro (la entrada y el asiento): un texto sin ellas no es un
        // justificante del Registro.
        (
            "REGISTRO/deposito-2024.pdf".into(),
            pdf(
                "REGISTRO MERCANTIL DE VALENCIA. Entrada 1/2025/1234, Diario 2025, Asiento 567. \
                 Depósito de cuentas anuales del ejercicio 2024. Fecha de presentación: 15/09/2025",
            ),
            None,
        ),
    ];
    let r = revisar(f);
    let h = hallazgo(&r, "cruces.deposito_fuera_de_plazo");
    assert_eq!(h.ejercicio, Some(2024));
    let h = hallazgo(&r, "cruces.legalizacion_sin_justificante");
    assert!(h.titulo.contains("2024"), "{}", h.titulo);
    assert!(r.pedir.iter().any(|p| p.que.contains("legalización")));
}

/// Un extracto MT940 con movimientos (fecha AAMMDD, D/C, importe, concepto).
fn mt940(movs: &[(&str, &str, &str)]) -> Vec<u8> {
    let mut s = String::from(
        "{1:F01DEMOESMMAXXX0000000000}{4:\n:20:DEMO\n:25:ES9121000418450200051332\n:28C:1/1\n:60F:C250101EUR10000,00\n",
    );
    for (f, dc, imp) in movs {
        s.push_str(&format!(
            ":61:{f}{dc}{imp}NTRFNONREF\n:86:AEAT IMPUESTOS DOMICILIADOS\n"
        ));
    }
    s.push_str(":62F:C250731EUR8796,00\n-}");
    s.into_bytes()
}

#[test]
fn pagos_del_banco_frente_a_los_modelos() {
    let f = vec![
        con(
            "MODELOS/303-1T.pdf",
            modelo("303", 2025, "1T", json!({"71": "1000.00"})),
        ),
        con(
            "MODELOS/111-1T.pdf",
            modelo("111", 2025, "1T", json!({"30": "150.00"})),
        ),
        con(
            "MODELOS/115-2T.pdf",
            modelo("115", 2025, "2T", json!({"05": "190.00"})),
        ),
        (
            "BANCO/enero-julio.sta".into(),
            mt940(&[
                ("250421", "D", "1000,00"),
                ("250421", "D", "149,00"),
                ("250610", "D", "55,00"),
            ]),
            None,
        ),
    ];
    let r = revisar(f);
    // El 303 casa; el 111 tiene un cargo de Hacienda por otro importe (149 €, no el de 1.000 €
    // del 303); el 115 del 2T no se ve.
    assert!(
        hallazgo(&r, "cruces.pago_otro_importe")
            .titulo
            .starts_with("El cargo del 111 del 1T 2025"),
        "{}",
        hallazgo(&r, "cruces.pago_otro_importe").titulo
    );
    assert!(
        !hallazgo(&r, "cruces.pago_otro_importe")
            .detalle
            .contains("1.000,00")
    );
    assert!(
        hallazgo(&r, "cruces.pago_no_esta")
            .titulo
            .contains("pago del 115 del 2T 2025")
    );
    // Sin cargo en el banco: urgente, en periodo ejecutivo, y sale el primero.
    let u = hallazgo(&r, "cruces.pago_no_esta")
        .urgente
        .as_ref()
        .expect("urgente");
    assert_eq!(u.porcentaje_hoy, Some(5));
    assert_eq!(u.siguiente_porcentaje, Some(10));
    assert!(r.hallazgos[0].urgente.is_some());
    assert!(r.resumen.urgentes >= 1);
    assert!(
        !r.hallazgos
            .iter()
            .any(|h| h.codigo.starts_with("cruces.pago") && h.titulo.contains("303"))
    );
    // El de 149 € ya sale como pago del 111 por otro importe: no se repite como suelto.
    let h = hallazgo(&r, "cruces.cargos_sin_modelo");
    assert!(
        h.detalle.contains("55,00 €") && !h.detalle.contains("149,00 €"),
        "{}",
        h.detalle
    );
}

/// El 200 de 2021 (antes del primer ejercicio revisable, 2022) se paga en julio de 2022, dentro
/// del periodo revisado: su cargo no es «de ningún modelo». Con un certificado de estar al
/// corriente de los últimos doce meses, el texto de los cargos que quedan dice lo que cubre. Antes,
/// el cargo del 200 de 2021 salía entre los sueltos.
#[test]
fn los_cargos_de_modelos_del_ejercicio_anterior_y_el_certificado() {
    let ficheros = |certificado: Option<&str>| {
        let mut f = vec![
            con(
                "MODELOS/2021/200.pdf",
                modelo("200", 2021, "0A", json!({"01586": "3000.00"})),
            ),
            (
                "BANCO/2022.sta".into(),
                mt940(&[("220725", "D", "3000,00"), ("220910", "D", "55,00")]),
                None,
            ),
        ];
        if let Some(fecha) = certificado {
            f.push(con(
                "EMPRESA/certificado.pdf",
                json!({
                    "tipo": "certificado_aeat",
                    "descripcion": "Certificado positivo de estar al corriente de las obligaciones tributarias",
                    "empresa": {"nif": PROPIO, "date": fecha}
                }),
            ));
        }
        f
    };
    let r = revisar(ficheros(None));
    let h = hallazgo(&r, "cruces.cargos_sin_modelo");
    assert!(h.detalle.contains("55,00 €"), "{}", h.detalle);
    assert!(!h.detalle.contains("3.000,00 €"), "{}", h.detalle);
    assert!(h.detalle.contains("recargos, sanciones"), "{}", h.detalle);
    // Con un certificado de hace menos de un año, lo dice.
    let r = revisar(ficheros(Some("2026-03-02")));
    let h = hallazgo(&r, "cruces.cargos_sin_modelo");
    assert!(
        h.detalle
            .contains("al corriente con la AEAT del 02/03/2026"),
        "{}",
        h.detalle
    );
    assert!(h.detalle.contains("art. 75.2 RGAT"), "{}", h.detalle);
    assert!(!h.detalle.contains("recargos, sanciones"), "{}", h.detalle);
    // Uno de hace más de doce meses ya no vale.
    let r = revisar(ficheros(Some("2025-09-01")));
    assert!(
        hallazgo(&r, "cruces.cargos_sin_modelo")
            .detalle
            .contains("recargos, sanciones")
    );
    // Otro certificado de la AEAT (el de situación censal) no es el de estar al corriente.
    let mut f = ficheros(None);
    f.push(con(
        "EMPRESA/censal.pdf",
        json!({
            "tipo": "certificado_aeat",
            "descripcion": "Certificado de situación censal (no es el de estar al corriente)",
            "empresa": {"nif": PROPIO, "date": "2026-03-02"}
        }),
    ));
    let r = revisar(f);
    assert!(
        hallazgo(&r, "cruces.cargos_sin_modelo")
            .detalle
            .contains("recargos, sanciones")
    );
}

/// Tres plazos de un fraccionamiento y un certificado de estar al corriente: el certificado no
/// los descarta (lo aplazado o fraccionado no lo impide, art. 74.1.g RGAT; tampoco un recargo,
/// una sanción o intereses pagados en periodo voluntario) y sólo cubre las declaraciones con
/// plazo vencido en los doce meses anteriores a los dos meses previos a su fecha (art. 74.3
/// RGAT). Antes decía que lo más probable era que fueran modelos que no están en la carpeta.
#[test]
fn el_certificado_no_descarta_un_fraccionamiento() {
    let mut extracto = String::from(
        "{1:F01DEMOESMMAXXX0000000000}{4:\n:20:DEMO\n:25:ES9121000418450200051332\n:28C:1/1\n\
         :60F:C250101EUR10000,00\n",
    );
    for (f, plazo) in [("250305", 1), ("250407", 2), ("250505", 3)] {
        extracto.push_str(&format!(
            ":61:{f}D250,00NTRFNONREF\n:86:AEAT FRACCIONAMIENTO PLAZO {plazo}\n"
        ));
    }
    extracto.push_str(":62F:C250731EUR9250,00\n-}");
    let r = revisar(vec![
        ("BANCO/2025.sta".into(), extracto.into_bytes(), None),
        con(
            "EMPRESA/certificado.pdf",
            json!({
                "tipo": "certificado_aeat",
                "descripcion": "Certificado positivo de estar al corriente de las obligaciones tributarias",
                "empresa": {"nif": PROPIO, "date": "2026-03-02"}
            }),
        ),
    ]);
    let h = hallazgo(&r, "cruces.cargos_sin_modelo");
    assert!(h.detalle.contains("750,00 €"), "{}", h.detalle);
    assert!(!h.detalle.contains("lo más probable"), "{}", h.detalle);
    assert!(
        h.detalle.contains("aplazamiento o fraccionamiento")
            && h.detalle.contains("art. 74.1.g RGAT"),
        "{}",
        h.detalle
    );
    assert!(
        h.detalle.contains("un recargo, una sanción o intereses"),
        "{}",
        h.detalle
    );
    assert!(h.detalle.contains("art. 74.3 RGAT"), "{}", h.detalle);
}

#[test]
fn administradores_y_saldos_de_socios() {
    use taxpain_app::{accounting, payroll};
    use taxpain_core::accounting::account::acc;
    use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};

    let v = vault();
    payroll::save_employee(
        &v,
        &payroll::Employee {
            nif: "12345678Z".into(),
            name: "PEREZ GOMEZ, JUAN".into(),
            province: "46".into(),
            birth_year: 1970,
            family: 3,
            contract: 1,
            administrator: true,
            active: true,
            ..Default::default()
        },
        datetime!(2025-01-02 9:00 UTC),
    )
    .unwrap();
    // La sociedad presta 3.000 € al socio en 2025 y no se devuelven.
    accounting::manual_entry(
        &v,
        &JournalEntry {
            date: date!(2025 - 03 - 01),
            concept: "Préstamo al socio".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("551"), eur(3_000)),
                EntryLine::credit(acc("572"), eur(3_000)),
            ],
        },
        datetime!(2025-03-01 9:00 UTC),
    )
    .unwrap();
    let r = revisar_con(
        &v,
        vec![con(
            "EMPRESA/poderes.pdf",
            json!({
                "tipo": "escritura_poderes",
                "descripcion": "Escritura de nombramiento de administradora",
                "empresa": {"nif": PROPIO, "administrator": "María López García", "date": "2023-05-04"}
            }),
        )],
    );
    let h = hallazgo(&r, "cruces.administrador_no_coincide");
    assert!(
        h.titulo.contains("PEREZ GOMEZ") && h.detalle.contains("María López"),
        "{}",
        h.detalle
    );
    let h = hallazgo(&r, "cruces.socios_deudores");
    assert!(
        h.titulo.contains("3.000,00 €") && h.detalle.contains("551"),
        "{}",
        h.titulo
    );
}

#[test]
fn sin_datos_de_mas_no_hay_cruces() {
    let r = revisar(vec![con(
        "GASTOS/p.pdf",
        recibida(
            PROVEEDOR,
            "Proveedor Demo SL",
            "P-1",
            "2025-05-10",
            ["100.00", "21.00", "121.00"],
        ),
    )]);
    let cruces: Vec<&str> = r
        .hallazgos
        .iter()
        .map(|h| h.codigo.as_str())
        .filter(|c| c.starts_with("cruces.") && !c.contains("sin_justificante"))
        .collect();
    assert!(cruces.is_empty(), "{cruces:?}");
}

/// Sin los festivos oficiales del año (TaxPain tiene los de 2026 y 2027), el plazo se calcula
/// sólo con los fines de semana: un día de retraso puede ser un festivo (de la comunidad o del
/// municipio) y no se avisa; uno de meses, sí. Con el calendario del año, un día ya es fuera de
/// plazo.
#[test]
fn sin_los_festivos_del_año_un_retraso_de_dias_no_se_avisa() {
    let con_fecha = |m: &str, y: i32, p: &str, casillas: Value, fecha: &str| {
        let mut l = modelo(m, y, p, casillas);
        l["modelo"]["presentado"] = json!(fecha);
        l
    };
    let r = revisar(vec![
        // Plazo calculado: lunes 20/10/2025; presentado el martes.
        con(
            "MODELOS/303-3T-2025.pdf",
            con_fecha("303", 2025, "3T", json!({"71": "100.00"}), "2025-10-21"),
        ),
        con(
            "MODELOS/111-2T-2025.pdf",
            con_fecha("111", 2025, "2T", json!({"30": "100.00"}), "2025-09-30"),
        ),
        // 2026 sí tiene sus festivos: el plazo del 2T acababa el lunes 20/07/2026.
        con(
            "MODELOS/303-2T-2026.pdf",
            con_fecha("303", 2026, "2T", json!({"71": "100.00"}), "2026-07-21"),
        ),
    ]);
    let tarde: Vec<&String> = r
        .hallazgos
        .iter()
        .filter(|h| h.codigo == "cruces.fuera_de_plazo")
        .map(|h| &h.titulo)
        .collect();
    assert_eq!(tarde.len(), 2, "{tarde:?}");
    assert!(
        tarde.iter().any(|t| t.contains("111 del 2T 2025")),
        "{tarde:?}"
    );
    assert!(
        tarde.iter().any(|t| t.contains("303 del 2T 2026")),
        "{tarde:?}"
    );
    let h = r
        .hallazgos
        .iter()
        .find(|h| h.titulo.contains("111 del 2T 2025"))
        .unwrap();
    assert!(
        h.detalle.contains("festivos oficiales de 2025"),
        "{}",
        h.detalle
    );
}

/// Un cargo del banco que es el pago de un modelo no sirve también de pago tardío de otro del
/// mismo importe (dos pagos fraccionados iguales y sólo el cargo del segundo).
#[test]
fn un_cargo_no_paga_dos_modelos() {
    let extracto = "{1:F01DEMOESMMAXXX0000000000}{4:\n:20:DEMO\n:25:ES9121000418450200051332\n\
                    :28C:1/1\n:60F:C250101EUR10000,00\n:61:251222D500,00NTRFNONREF\n\
                    :86:AEAT IMPUESTOS DOMICILIADOS\n:62F:C251231EUR9500,00\n-}";
    let r = revisar(vec![
        con(
            "MODELOS/202-2P.pdf",
            modelo("202", 2025, "2P", json!({"03": "500.00"})),
        ),
        con(
            "MODELOS/202-3P.pdf",
            modelo("202", 2025, "3P", json!({"03": "500.00"})),
        ),
        ("BANCO/2025.sta".into(), extracto.as_bytes().to_vec(), None),
    ]);
    assert!(!hay(&r, "cruces.pago_tarde"), "{:?}", r.hallazgos);
    assert!(
        hallazgo(&r, "cruces.pago_no_esta")
            .titulo
            .contains("202 del 2P 2025")
    );
    let pagos = r
        .tablas
        .iter()
        .find(|t| t.titulo == "Pagos de los modelos en el banco")
        .unwrap();
    let estado = |p: &str| {
        pagos
            .filas
            .iter()
            .find(|f| f[1] == p)
            .map(|f| f[5].clone())
            .unwrap()
    };
    assert_eq!(estado("3P 2025"), "pagado");
    assert_eq!(estado("2P 2025"), "no se ve el cargo");
}

/// Un extracto MT940 de una cuenta, del `desde` al `hasta` (AAMMDD), con sus cargos.
fn mt940_de(cuenta: &str, desde: &str, hasta: &str, cargos: &[(&str, &str, &str)]) -> Vec<u8> {
    let mut s = format!(
        "{{1:F01DEMOESMMAXXX0000000000}}{{4:\n:20:DEMO\n:25:{cuenta}\n:28C:1/1\n:60F:C{desde}EUR10000,00\n"
    );
    for (f, imp, concepto) in cargos {
        s.push_str(&format!(":61:{f}D{imp}NTRFNONREF\n:86:{concepto}\n"));
    }
    s.push_str(&format!(":62F:C{hasta}EUR9000,00\n-}}"));
    s.into_bytes()
}

/// Los impuestos se pagan desde una cuenta: si de esa cuenta no hay extracto de las fechas del
/// pago, no se puede decir que no se pagó aunque haya extractos de otra cuenta. Y el 200 del
/// ejercicio anterior, que se paga en el primero que se revisa, no es un cargo sin modelo.
#[test]
fn los_pagos_se_buscan_en_la_cuenta_de_los_impuestos() {
    let r = revisar(vec![
        con(
            "MODELOS/303-1T.pdf",
            modelo("303", 2025, "1T", json!({"71": "1000.00"})),
        ),
        con(
            "MODELOS/303-3T.pdf",
            modelo("303", 2025, "3T", json!({"71": "300.00"})),
        ),
        con(
            "MODELOS/200-2021.pdf",
            modelo("200", 2021, "0A", json!({"01586": "700.00"})),
        ),
        (
            "BANCO/impuestos.sta".into(),
            mt940_de(
                "ES9121000418450200051332",
                "220101",
                "250630",
                &[
                    ("220720", "700,00", "ADEUDO TRIBUTOS"),
                    ("250421", "1000,00", "ADEUDO TRIBUTOS"),
                ],
            ),
            None,
        ),
        (
            "BANCO/otra.sta".into(),
            mt940_de(
                "ES7620770024003102575766",
                "250101",
                "251231",
                &[("250910", "25,00", "RECIBO DEMO")],
            ),
            None,
        ),
    ]);
    assert!(!hay(&r, "cruces.pago_no_esta"), "{:?}", r.hallazgos);
    assert!(
        hallazgo(&r, "cruces.pagos_sin_extracto")
            .detalle
            .contains("303 3T 2025")
    );
    assert!(!hay(&r, "cruces.cargos_sin_modelo"), "{:?}", r.hallazgos);
}

/// Si en la carpeta hay facturas del periodo sin el NIF del otro o sin el importe leídos, que
/// el modelo declare más que las facturas no quiere decir que falten facturas.
#[test]
fn con_facturas_sin_leer_no_se_dice_que_falten_facturas() {
    let mut m = modelo("349", 2025, "2T", json!({"02": "1000.00"}));
    m["modelo"]["registros"] = json!([
        {"nif": "DE123456789", "nombre": "Kunde Demo", "clave": "S", "importe": "1000.00"}
    ]);
    // La venta, reconocida sólo por las reglas: sin el NIF-IVA del cliente.
    let venta = text_document(
        "Documento",
        &[[
            "FACTURA",
            &format!("EMPRESA DEMO SL · {PROPIO}"),
            "Factura nº: A-7",
            "Fecha: 10/05/2025",
            "Cliente: Kunde Demo GmbH",
            "Base imponible 1.000,00",
            "Total factura 1.000,00",
        ]
        .iter()
        .map(|l| Para::Text((*l).to_owned()))
        .collect()],
    )
    .unwrap();
    let r = revisar(vec![
        con("MODELOS/349-2T.pdf", m.clone()),
        ("INGRESOS/a7.pdf".to_owned(), venta, None),
    ]);
    let h = hallazgo(&r, "cruces.349_sin_facturas");
    assert!(
        h.detalle.contains("1 factura de ese periodo"),
        "{}",
        h.detalle
    );
    assert!(
        !h.detalle.contains("Lo normal es que falten"),
        "{}",
        h.detalle
    );
    // Sin facturas sin leer, sí: una leída con IA sin el NIF del cliente está leída.
    let mut leida = emitida("DE123456789", "A-8", "2025-05-20", ["10.00", "0", "10.00"]);
    leida["factura"]["nif_destinatario"] = Value::Null;
    let r = revisar(vec![
        con("MODELOS/349-2T.pdf", m),
        con("INGRESOS/a8.pdf", leida),
    ]);
    let h = hallazgo(&r, "cruces.349_sin_facturas");
    assert!(
        h.detalle.contains("Lo normal es que falten"),
        "{}",
        h.detalle
    );
}

/// Un asiento del diario con sus apuntes: (cuenta, debe, haber) en euros.
fn asiento(v: &Vault, fecha: time::Date, concepto: &str, apuntes: &[(&str, i64, i64)]) {
    use taxpain_app::accounting;
    use taxpain_core::accounting::account::acc;
    use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};

    accounting::manual_entry(
        v,
        &JournalEntry {
            date: fecha,
            concept: concepto.into(),
            origin: Origin::Manual,
            lines: apuntes
                .iter()
                .map(|(c, d, h)| {
                    if *d > 0 {
                        EntryLine::debit(acc(c), eur(*d))
                    } else {
                        EntryLine::credit(acc(c), eur(*h))
                    }
                })
                .collect(),
        },
        datetime!(2025-01-02 9:00 UTC),
    )
    .unwrap();
}

/// El diario de 2025 con las nóminas (150 € al mes de retención), una factura de un profesional
/// en el 1T (75 €), un alquiler (190 €, al 115), un dividendo (1.900 €, al 123), el pago del 111
/// anterior y una retención apuntada por error y corregida.
fn diario_con_retenciones(v: &Vault) {
    asiento(
        v,
        date!(2025 - 01 - 20),
        "Pago del 111 del 4T",
        &[("4751", 100, 0), ("572", 0, 100)],
    );
    for (mes, dia) in [(1, 31), (4, 30), (7, 31), (10, 31)] {
        asiento(
            v,
            time::Date::from_calendar_date(2025, time::Month::try_from(mes).unwrap(), dia).unwrap(),
            "Nómina",
            &[("640", 1_000, 0), ("4751", 0, 150), ("465", 0, 850)],
        );
    }
    asiento(
        v,
        date!(2025 - 02 - 10),
        "Factura del abogado",
        &[
            ("623", 500, 0),
            ("472", 105, 0),
            ("4751", 0, 75),
            ("410", 0, 530),
        ],
    );
    asiento(
        v,
        date!(2025 - 04 - 01),
        "Alquiler de la oficina",
        &[
            ("621", 1_000, 0),
            ("472", 210, 0),
            ("4751", 0, 190),
            ("410", 0, 1_020),
        ],
    );
    asiento(
        v,
        date!(2025 - 05 - 10),
        "Factura con una retención que no era",
        &[("623", 100, 0), ("4751", 0, 15), ("410", 0, 85)],
    );
    asiento(
        v,
        date!(2025 - 05 - 20),
        "Corrección de la retención",
        &[("4751", 15, 0), ("410", 0, 15)],
    );
    asiento(
        v,
        date!(2025 - 11 - 15),
        "Dividendo con cargo a reservas",
        &[("113", 10_000, 0), ("4751", 0, 1_900), ("572", 0, 8_100)],
    );
}

fn ciento_once(periodo: &str, retenciones: &str) -> Fichero {
    con(
        &format!("MODELOS/111-{periodo}.pdf"),
        modelo(
            "111",
            2025,
            periodo,
            json!({"03": "150.00", "28": retenciones, "30": retenciones}),
        ),
    )
}

/// El 1T: la 4751 recibe 225 € (nómina y abogado) y el 111 declara 150 €: 75 € retenidos y no
/// ingresados, con su recargo, y el 190 también se queda corto. El alquiler y el dividendo van
/// aparte (115 y 123) y la retención corregida no cuenta. Sin la 4751, o si cuadra, nada.
#[test]
fn las_retenciones_del_diario_frente_a_los_111_y_el_190() {
    let v = vault();
    diario_con_retenciones(&v);
    let mut f = vec![
        ciento_once("1T", "150.00"),
        ciento_once("2T", "150.00"),
        ciento_once("3T", "150.00"),
        ciento_once("4T", "150.00"),
        con(
            "MODELOS/190.pdf",
            modelo(
                "190",
                2025,
                "0A",
                json!({"01": "2", "02": "4500.00", "03": "600.00"}),
            ),
        ),
    ];
    let r = revisar_con(&v, f.clone());
    let h = hallazgo(&r, "cruces.retenciones_no_ingresadas");
    assert_eq!(h.gravedad, Gravedad::Aviso);
    assert_eq!(h.ejercicio, Some(2025));
    assert!(h.titulo.contains("75,00 €"), "{}", h.titulo);
    assert!(
        h.detalle
            .contains("1T 2025: 225,00 € en la 4751 y 150,00 € en el 111 (75,00 € sin ingresar)"),
        "{}",
        h.detalle
    );
    assert!(!h.detalle.contains("2T 2025:"), "{}", h.detalle);
    for art in ["art. 27 LGT", "179.3", "191"] {
        assert!(h.detalle.contains(art), "{art}: {}", h.detalle);
    }
    assert!(
        h.detalle
            .contains("alquileres, del capital mobiliario y de no residentes"),
        "{}",
        h.detalle
    );
    assert!(
        h.detalle.contains("El 190 de 2025 declara 600,00 €")
            && h.detalle.contains("abona 675,00 €"),
        "{}",
        h.detalle
    );
    // 75 € y el 15 % de recargo (más de doce meses desde el 21/04/2025): 86,25 € a pagar.
    assert_eq!(h.importe, Some(Money::from_cents(-8_625)), "{}", h.detalle);
    assert!(
        r.pedir.iter().any(|p| p.que.contains("del 111 de 1T 2025")
            && p.que.contains("75,00 €")
            && p.que.contains("corregir el 190 de 2025")),
        "{:?}",
        r.pedir
    );
    let t = r
        .tablas
        .iter()
        .find(|t| t.titulo == "Retenciones del diario (4751) frente a los 111")
        .unwrap();
    assert_eq!(t.filas.len(), 4, "{:?}", t.filas);
    assert_eq!(t.filas[1][4], "190,00 €", "{:?}", t.filas);
    assert_eq!(t.filas[3][4], "1.900,00 €", "{:?}", t.filas);

    // Si el 111 del 1T recoge las dos retenciones (y el 190, las del año), nada.
    f[0] = ciento_once("1T", "225.00");
    f[4] = con(
        "MODELOS/190.pdf",
        modelo(
            "190",
            2025,
            "0A",
            json!({"01": "2", "02": "4500.00", "03": "675.00"}),
        ),
    );
    let r = revisar_con(&v, f.clone());
    assert!(
        !hay(&r, "cruces.retenciones_no_ingresadas"),
        "{:?}",
        r.hallazgos
    );

    // Sin diario, nada; y un diario resumido a tres cifras (la 475) no dice qué son retenciones.
    let r = revisar(f.clone());
    assert!(!hay(&r, "cruces.retenciones_no_ingresadas"));
    let resumido = vault();
    for (mes, dia) in [(3, 31), (6, 30), (9, 30), (12, 31)] {
        asiento(
            &resumido,
            time::Date::from_calendar_date(2025, time::Month::try_from(mes).unwrap(), dia).unwrap(),
            "Resumen del trimestre",
            &[("640", 3_000, 0), ("475", 0, 900), ("465", 0, 2_100)],
        );
    }
    // Con el pago del 4T del año anterior dentro del resumen de enero: un cargo a la 4751 no
    // dice que el diario lleve las retenciones en ella.
    asiento(
        &resumido,
        date!(2025 - 01 - 31),
        "Resumen de enero",
        &[("4751", 300, 0), ("572", 0, 300)],
    );
    f[0] = ciento_once("1T", "150.00");
    let r = revisar_con(&resumido, f);
    assert!(
        !hay(&r, "cruces.retenciones_no_ingresadas"),
        "{:?}",
        r.hallazgos
    );
    assert!(
        !r.tablas
            .iter()
            .any(|t| t.titulo == "Retenciones del diario (4751) frente a los 111")
    );
}

fn dia(y: i32, m: u8, d: u8) -> time::Date {
    time::Date::from_calendar_date(y, time::Month::try_from(m).unwrap(), d).unwrap()
}

/// Las nóminas de 2025: 150 € de retención al mes en enero, abril, julio y octubre.
fn nominas(v: &Vault) {
    for (mes, d) in [(1, 31), (4, 30), (7, 31), (10, 31)] {
        asiento(
            v,
            dia(2025, mes, d),
            "Nómina",
            &[("640", 1_000, 0), ("4751", 0, 150), ("465", 0, 850)],
        );
    }
}

fn ciento_noventa(retenciones: &str) -> Fichero {
    con(
        "MODELOS/190.pdf",
        modelo(
            "190",
            2025,
            "0A",
            json!({"01": "2", "02": "4500.00", "03": retenciones}),
        ),
    )
}

fn tabla_4751(r: &Revision) -> Vec<Vec<String>> {
    r.tablas
        .iter()
        .find(|t| t.titulo == "Retenciones del diario (4751) frente a los 111")
        .map(|t| t.filas.clone())
        .unwrap_or_default()
}

/// La factura del abogado se apunta el 28 de marzo y se paga el 10 de abril: la retención va en
/// el 111 del 2T (la obligación de retener nace con el pago), el del 1T recoge 75 € menos y el
/// del 2T 75 € más, y el año cuadra con el 190. No es una retención sin ingresar. Si la recoge
/// el 3T (se pagó en julio), tampoco; si ni el 2T ni el 3T la recogen, sí.
#[test]
fn una_retencion_pagada_el_trimestre_siguiente_no_esta_sin_ingresar() {
    let v = vault();
    nominas(&v);
    asiento(
        &v,
        dia(2025, 3, 28),
        "Factura del abogado",
        &[
            ("623", 500, 0),
            ("472", 105, 0),
            ("4751", 0, 75),
            ("410", 0, 530),
        ],
    );
    asiento(
        &v,
        dia(2025, 4, 10),
        "Pago al abogado",
        &[("410", 530, 0), ("572", 0, 530)],
    );
    let mut f = vec![
        ciento_once("1T", "150.00"),
        ciento_once("2T", "225.00"),
        ciento_once("3T", "150.00"),
        ciento_once("4T", "150.00"),
        ciento_noventa("675.00"),
    ];
    let r = revisar_con(&v, f.clone());
    for codigo in [
        "cruces.retenciones_no_ingresadas",
        "cruces.retenciones_por_comprobar",
    ] {
        assert!(!hay(&r, codigo), "{codigo}: {:?}", r.hallazgos);
    }
    let t = tabla_4751(&r);
    assert_eq!(t[0][3], "75,00 €", "{t:?}");
    assert_eq!(t[0][5], "0,00 €", "{t:?}");
    assert_eq!(t[1][3], "-75,00 €", "{t:?}");

    // Pagada en julio: va en el 111 del 3T.
    f[1] = ciento_once("2T", "150.00");
    f[2] = ciento_once("3T", "225.00");
    let r = revisar_con(&v, f.clone());
    assert!(
        !hay(&r, "cruces.retenciones_no_ingresadas"),
        "{:?}",
        r.hallazgos
    );

    // Ningún 111 la recoge.
    f[2] = ciento_once("3T", "150.00");
    f[4] = ciento_noventa("600.00");
    let r = revisar_con(&v, f);
    let h = hallazgo(&r, "cruces.retenciones_no_ingresadas");
    assert!(h.titulo.contains("75,00 €"), "{}", h.titulo);
    assert!(
        h.detalle.contains("78.1 RIRPF") && h.detalle.contains("108.1 RIRPF"),
        "{}",
        h.detalle
    );
}

/// Lo que el 111 del 2T recoge de menos y el del 3T de más se compensa en parte: el resto está
/// sin ingresar. Lo que el 111 del 4T no recoge puede ir en el de uno de los dos trimestres
/// siguientes: sin el diario del año siguiente no se puede ver y queda por comprobar, como nota
/// y sin importe.
#[test]
fn lo_que_los_111_siguientes_aun_no_dicen_queda_por_comprobar() {
    let v = vault();
    nominas(&v);
    asiento(
        &v,
        dia(2025, 5, 12),
        "Factura del asesor",
        &[("623", 600, 0), ("4751", 0, 90), ("410", 0, 510)],
    );
    asiento(
        &v,
        dia(2025, 12, 29),
        "Factura del notario",
        &[("623", 200, 0), ("4751", 0, 30), ("410", 0, 170)],
    );
    let r = revisar_con(
        &v,
        vec![
            ciento_once("1T", "150.00"),
            ciento_once("2T", "150.00"),
            ciento_once("3T", "190.00"),
            ciento_once("4T", "150.00"),
        ],
    );
    let h = hallazgo(&r, "cruces.retenciones_no_ingresadas");
    assert!(h.titulo.contains("50,00 €"), "{}", h.titulo);
    assert!(
        h.detalle.contains(
            "2T 2025: 240,00 € en la 4751 y 150,00 € en el 111, más 40,00 € en el del 3T 2025 \
             (50,00 € sin ingresar)"
        ),
        "{}",
        h.detalle
    );
    assert!(!h.detalle.contains("4T 2025"), "{}", h.detalle);
    let nota = hallazgo(&r, "cruces.retenciones_por_comprobar");
    assert_eq!(nota.gravedad, Gravedad::Info);
    assert_eq!(nota.importe, None);
    assert!(nota.titulo.contains("30,00 €"), "{}", nota.titulo);
    assert!(
        nota.detalle.contains("4T 2025")
            && nota
                .detalle
                .contains("el diario de 2026 no está en TaxPain o no usa la 4751"),
        "{}",
        nota.detalle
    );
    let t = tabla_4751(&r);
    assert_eq!(t[1][5], "50,00 €", "{t:?}");
    assert_eq!(t[3][5], "30,00 € (por comprobar)", "{t:?}");
}

/// Si los 111 casan con el diario, lo retenido está ingresado: que el 190 se quede corto ya lo
/// dice «el 190 no cuadra con los 111» y no sale otro aviso. Sin los 111 del 3T y del 4T, el 190
/// frente al diario queda por comprobar.
#[test]
fn si_los_111_casan_con_el_diario_el_190_no_da_otro_aviso() {
    let v = vault();
    nominas(&v);
    let mut f = vec![
        ciento_once("1T", "150.00"),
        ciento_once("2T", "150.00"),
        ciento_once("3T", "150.00"),
        ciento_once("4T", "150.00"),
        ciento_noventa("450.00"),
    ];
    let r = revisar_con(&v, f.clone());
    assert!(hay(&r, "cruces.resumen_anual"), "{:?}", r.hallazgos);
    for codigo in [
        "cruces.retenciones_no_ingresadas",
        "cruces.retenciones_por_comprobar",
    ] {
        assert!(!hay(&r, codigo), "{codigo}: {:?}", r.hallazgos);
    }
    f.drain(2..4);
    let r = revisar_con(&v, f);
    assert!(
        !hay(&r, "cruces.retenciones_no_ingresadas"),
        "{:?}",
        r.hallazgos
    );
    let nota = hallazgo(&r, "cruces.retenciones_por_comprobar");
    assert_eq!(nota.gravedad, Gravedad::Info);
    assert!(
        nota.detalle.contains("El 190 de 2025 declara 450,00 €")
            && nota.detalle.contains("abona 600,00 €")
            && nota.detalle.contains("3T 2025 y 4T 2025"),
        "{}",
        nota.detalle
    );
}

/// La retención de un no residente va al 216, no al 111: con «IRNR» o «no residente» en el
/// concepto del asiento, va aparte.
#[test]
fn las_retenciones_de_no_residentes_van_al_216() {
    let v = vault();
    nominas(&v);
    asiento(
        &v,
        dia(2025, 5, 12),
        "Factura de un desarrollador no residente, retención IRNR",
        &[("623", 1_000, 0), ("47510009", 0, 190), ("410", 0, 810)],
    );
    let r = revisar_con(
        &v,
        vec![
            ciento_once("1T", "150.00"),
            ciento_once("2T", "150.00"),
            ciento_once("3T", "150.00"),
            ciento_once("4T", "150.00"),
        ],
    );
    for codigo in [
        "cruces.retenciones_no_ingresadas",
        "cruces.retenciones_por_comprobar",
    ] {
        assert!(!hay(&r, codigo), "{codigo}: {:?}", r.hallazgos);
    }
    let t = tabla_4751(&r);
    assert_eq!(t[1][1], "150,00 €", "{t:?}");
    assert_eq!(t[1][4], "190,00 €", "{t:?}");
}

/// Una línea de un libro de recibidas: número, fecha («dd/mm»), NIF, nombre, base, cuota,
/// retención y total.
type LineaConRetencion<'a> = (
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
    &'a str,
);

/// Un libro de recibidas de 2025 como los de a3, con la columna de la retención.
fn libro_con_retenciones(lineas: &[LineaConRetencion<'_>]) -> Fichero {
    libro_recibidas("LIBROS/2025 - Libro registro recibidas.xlsx", true, lineas)
}

/// Un libro de recibidas de 2025 como los de a3, con la columna de la retención o sin ella (y
/// entonces con el total sin restarla).
fn libro_recibidas(ruta: &str, con_retencion: bool, lineas: &[LineaConRetencion<'_>]) -> Fichero {
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("Facturas").unwrap();
    let mut filas: Vec<Vec<String>> = vec![
        vec!["Operaciones Interiores - Facturas Recibidas".into()],
        vec![],
        vec!["Empresa: EMPRESA DEMO SL".into()],
        vec!["Período De 01 Ene a 31 Dic 2025".into()],
        vec!["Fecha: 04/03/2026".into()],
        vec![],
        [
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
        ]
        .iter()
        .filter(|t| con_retencion || **t != "Retención")
        .map(|t| (*t).to_owned())
        .collect(),
    ];
    for (n, (num, fecha, nif, nombre, base, cuota, ret, total)) in lineas.iter().enumerate() {
        let sin_restar = (base.parse::<i64>().unwrap() + cuota.parse::<i64>().unwrap()).to_string();
        let mut fila: Vec<String> = [
            &(n + 1).to_string(),
            &(n + 1).to_string(),
            *num,
            *fecha,
            &format!("Su Fra. {num}"),
            *nif,
            *nombre,
            *base,
            "21,0",
            *cuota,
        ]
        .iter()
        .map(|c| (*c).to_owned())
        .collect();
        if con_retencion {
            fila.extend([(*ret).to_owned(), (*total).to_owned()]);
        } else {
            fila.push(sin_restar);
        }
        filas.push(fila);
    }
    for (r, fila) in filas.iter().enumerate() {
        for (c, celda) in fila.iter().enumerate() {
            if !celda.is_empty() {
                ws.write_string(u32::try_from(r).unwrap(), u16::try_from(c).unwrap(), celda)
                    .unwrap();
            }
        }
    }
    (ruta.into(), wb.save_to_buffer().unwrap(), None)
}

/// Dos copias del libro de recibidas con las mismas facturas, la primera sin la columna de la
/// retención: cuenta la retención de la que la trae, y los 111 casan. Una factura que sólo está
/// en la copia sin la columna no dice su retención: ese trimestre no se compara con el libro,
/// sino con los PDF (aquí no hay ninguno: «sin sus facturas»).
#[test]
fn el_111_con_dos_copias_del_libro_una_sin_la_retencion() {
    let asesor = |num: &'static str, fecha: &'static str| {
        (
            num,
            fecha,
            ASESORES,
            "ASESORES DEMO SL",
            "300",
            "63",
            "45",
            "318",
        )
    };
    let lineas = [asesor("A-10", "10/04"), asesor("A-11", "10/07")];
    let ciento_once = |p: &str| {
        con(
            &format!("MODELOS/111-{p}.pdf"),
            modelo("111", 2025, p, json!({"09": "45.00", "28": "45.00"})),
        )
    };
    let r = revisar(vec![
        libro_recibidas(
            "LIBROS/2025 A - Libro registro recibidas.xlsx",
            false,
            &lineas,
        ),
        libro_recibidas(
            "LIBROS/2025 B - Libro registro recibidas.xlsx",
            true,
            &lineas,
        ),
        ciento_once("2T"),
        ciento_once("3T"),
    ]);
    assert!(!hay(&r, "modelos.111_retenciones"), "{:?}", r.hallazgos);
    assert!(!hay(&r, "modelos.111_sin_facturas"), "{:?}", r.hallazgos);

    let mut con_otra = lineas.to_vec();
    con_otra.push((
        "P-9",
        "15/08",
        PROVEEDOR,
        "PROVEEDOR DEMO SL",
        "100",
        "21",
        "",
        "121",
    ));
    let r = revisar(vec![
        libro_recibidas(
            "LIBROS/2025 A - Libro registro recibidas.xlsx",
            false,
            &con_otra,
        ),
        libro_recibidas(
            "LIBROS/2025 B - Libro registro recibidas.xlsx",
            true,
            &lineas,
        ),
        ciento_once("2T"),
        ciento_once("3T"),
    ]);
    assert!(!hay(&r, "modelos.111_retenciones"), "{:?}", r.hallazgos);
    let sin = hallazgo(&r, "modelos.111_sin_facturas");
    assert!(
        sin.detalle.contains("3T 2025") && !sin.detalle.contains("2T 2025"),
        "{}",
        sin.detalle
    );
}

/// Con el libro de recibidas que trae las retenciones, el 111 se compara con el libro: el del 2T
/// casa con él aunque no estén los PDF (no es un error de la gestoría); el del 3T declara 30 € que
/// el libro no tiene; el del 4T casa con el libro (nada), pero en la carpeta hay una factura con
/// retención que no está en el libro. El 1T, que el libro no cubre, sigue con los PDF.
#[test]
fn el_111_frente_a_las_retenciones_del_libro_de_recibidas() {
    let libro = libro_con_retenciones(&[
        (
            "A-10",
            "10/04",
            ASESORES,
            "ASESORES DEMO SL",
            "300",
            "63",
            "45",
            "318",
        ),
        (
            "ALQ-4",
            "01/05",
            "12345678Z",
            "PROPIETARIA DEMO",
            "1000",
            "210",
            "190",
            "1020",
        ),
        (
            "P-80",
            "15/11",
            PROVEEDOR,
            "PROVEEDOR DEMO SL",
            "100",
            "21",
            "0",
            "121",
        ),
    ]);
    let mut fuera = recibida(
        CLIENTE,
        "Notaria Demo SL",
        "N-7",
        "2025-11-20",
        ["100.00", "21.00", "106.00"],
    );
    fuera["factura"]["retencion"] = json!("15.00");
    let r = revisar(vec![
        libro,
        con("GASTOS/notaria.pdf", fuera),
        con(
            "MODELOS/111-1T.pdf",
            modelo("111", 2025, "1T", json!({"09": "60.00", "28": "60.00"})),
        ),
        con(
            "MODELOS/111-2T.pdf",
            modelo("111", 2025, "2T", json!({"09": "45.00", "28": "45.00"})),
        ),
        con(
            "MODELOS/111-3T.pdf",
            modelo("111", 2025, "3T", json!({"09": "30.00", "28": "30.00"})),
        ),
        con(
            "MODELOS/111-4T.pdf",
            modelo("111", 2025, "4T", json!({"28": "0.00"})),
        ),
    ]);
    let todos = |codigo: &str| -> Vec<&Hallazgo> {
        r.hallazgos.iter().filter(|h| h.codigo == codigo).collect()
    };
    let no_casan = todos("modelos.111_retenciones");
    assert_eq!(no_casan.len(), 1, "{no_casan:?}");
    assert!(
        no_casan[0].titulo.contains("3T 2025") && no_casan[0].titulo.contains("libro de recibidas"),
        "{}",
        no_casan[0].titulo
    );
    assert!(
        no_casan[0].detalle.contains("30,00 €") && no_casan[0].detalle.contains("retienen 0,00 €"),
        "{}",
        no_casan[0].detalle
    );
    let h = hallazgo(&r, "modelos.111_fuera_del_libro");
    assert!(h.titulo.contains("4T 2025"), "{}", h.titulo);
    assert!(
        h.titulo.contains("1 factura que no está en el libro"),
        "{}",
        h.titulo
    );
    assert!(h.detalle.contains("15,00 € retenidos"), "{}", h.detalle);
    // Sólo el 1T sale como 111 sin facturas: el 2T casa con el libro.
    let sin = hallazgo(&r, "modelos.111_sin_facturas");
    assert!(
        sin.detalle.contains("1T 2025") && !sin.detalle.contains("2T 2025"),
        "{}",
        sin.detalle
    );
}
