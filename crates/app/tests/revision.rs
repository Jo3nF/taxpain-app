//! Revisión de la carpeta de una empresa: una carpeta inventada con las lecturas de Claude Code
//! (`taxpain-lecturas.json`) y los errores típicos (copias, facturas repetidas, NIF mal,
//! numeración con saltos, libro sin PDF, 303 que no casa, 200 sin la exención de dividendos y
//! con bases negativas compensadas de más). Datos inventados, NIF de prueba.

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
const PAPELERIA: &str = "B12345674";
const ASESORES: &str = "B11111119";
const CLIENTE: &str = "B22222228";

fn vault() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let info = CompanyInfo {
        name: "Ejemplo Revisión SL".into(),
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

fn recibida(nif: &str, nombre: &str, numero: &str, fecha: &str, importes: [&str; 3]) -> Value {
    json!({
        "tipo": "factura_recibida",
        "descripcion": format!("Factura {numero} de {nombre}"),
        "factura": {
            "nif_emisor": nif, "nombre_emisor": nombre,
            "nif_destinatario": PROPIO, "nombre_destinatario": "Ejemplo Revisión SL",
            "numero": numero, "fecha": fecha,
            "base": importes[0], "iva": importes[1], "total": importes[2]
        }
    })
}

fn emitida(numero: &str, fecha: &str) -> Value {
    json!({
        "tipo": "factura_emitida",
        "descripcion": format!("Factura {numero} a Cliente Ejemplo"),
        "factura": {
            "nif_emisor": PROPIO, "nombre_emisor": "Ejemplo Revisión SL",
            "nif_destinatario": CLIENTE, "nombre_destinatario": "Cliente Ejemplo SL",
            "numero": numero, "fecha": fecha,
            "base": "1000.00", "iva": "210.00", "total": "1210.00"
        }
    })
}

fn modelo(modelo: &str, ejercicio: i32, periodo: &str, casillas: Value) -> Value {
    json!({
        "tipo": "modelo_aeat",
        "descripcion": format!("Modelo {modelo} {periodo} {ejercicio}"),
        "modelo": {
            "modelo": modelo, "ejercicio": ejercicio, "periodo": periodo,
            "casillas": casillas
        }
    })
}

/// La carpeta de prueba: cada fichero con su lectura en `taxpain-lecturas.json`.
fn carpeta(root: &Path) {
    let mut lecturas = Vec::new();
    let mut fichero = |ruta: &str, bytes: Vec<u8>, lectura: Option<Value>| {
        escribir(root, ruta, &bytes);
        if let Some(l) = lectura {
            lecturas.push(json!({ "fichero": ruta, "lectura": l }));
        }
    };

    // Una factura y su copia exacta (misma huella: una sola lectura vale para las dos).
    let papeleria = pdf("Papelería Ejemplo, factura P-101");
    fichero(
        "FACTURAS/2025/T1/GASTOS/papeleria-101.pdf",
        papeleria.clone(),
        Some(recibida(
            PAPELERIA,
            "Papelería Ejemplo SL",
            "P-101",
            "2025-02-10",
            ["300.00", "63.00", "363.00"],
        )),
    );
    fichero(
        "FACTURAS/2025/T1/GASTOS/papeleria-101 copia.pdf",
        papeleria,
        None,
    );
    // Proveedor de la UE que cobra IVA español (no debería).
    fichero(
        "FACTURAS/2025/T1/GASTOS/nube.pdf",
        pdf("Nube Ejemplo Ltd, invoice 55"),
        Some(recibida(
            "IE1234567T",
            "Nube Ejemplo Ltd",
            "INV-55",
            "2025-03-01",
            ["10.00", "2.10", "12.10"],
        )),
    );
    // La misma factura de los asesores en dos ficheros distintos (y uno en otra carpeta).
    let asesores = |texto: &str| {
        let mut l = recibida(
            ASESORES,
            "Asesores Ejemplo SL",
            "2025/7",
            "2025-04-15",
            ["1000.00", "210.00", "1060.00"],
        );
        l["factura"]["retencion"] = json!("150.00");
        (pdf(texto), Some(l))
    };
    let (b, l) = asesores("Asesores Ejemplo, factura 2025/7");
    fichero("FACTURAS/2025/T2/GASTOS/asesores-7.pdf", b, l);
    let (b, l) = asesores("Asesores Ejemplo, factura 2025/7 (copia escaneada)");
    fichero("FACTURAS/2025/T3/GASTOS/asesores-7-escaneada.pdf", b, l);
    // Total que no es base + IVA.
    fichero(
        "FACTURAS/2025/T2/GASTOS/mal-sumada.pdf",
        pdf("Papelería Ejemplo, factura P-150"),
        Some(recibida(
            PAPELERIA,
            "Papelería Ejemplo SL",
            "P-150",
            "2025-05-05",
            ["100.00", "21.00", "150.00"],
        )),
    );
    // NIF con el dígito de control mal (el bueno acaba en 4).
    fichero(
        "FACTURAS/2025/T2/GASTOS/nif-mal.pdf",
        pdf("Imprenta Ejemplo, factura X-1"),
        Some(recibida(
            "B12345675",
            "Imprenta Ejemplo SL",
            "X-1",
            "2025-05-06",
            ["50.00", "10.50", "60.50"],
        )),
    );
    // A nombre de una persona física.
    let mut personal = recibida(
        PAPELERIA,
        "Papelería Ejemplo SL",
        "P-160",
        "2025-06-01",
        ["20.00", "4.20", "24.20"],
    );
    personal["factura"]["nif_destinatario"] = json!("12345678Z");
    personal["factura"]["nombre_destinatario"] = json!("Persona Ejemplo");
    fichero(
        "FACTURAS/2025/T2/GASTOS/personal.pdf",
        pdf("Papelería Ejemplo, factura P-160"),
        Some(personal),
    );
    // En dólares.
    let mut dolares = recibida(
        "US123456789",
        "Hosting Ejemplo Inc.",
        "H-9",
        "2025-05-20",
        ["20.00", "0", "20.00"],
    );
    dolares["factura"]["moneda"] = json!("usd");
    fichero(
        "FACTURAS/2025/T2/GASTOS/hosting.pdf",
        pdf("Hosting Ejemplo, invoice H-9"),
        Some(dolares),
    );
    // Emitidas con un salto: falta la A-3.
    for (n, f) in [
        ("A-1", "2025-01-10"),
        ("A-2", "2025-02-10"),
        ("A-4", "2025-03-10"),
    ] {
        fichero(
            &format!("FACTURAS/2025/T1/INGRESOS/{n}.pdf"),
            pdf(&format!("Ejemplo Revisión, factura {n}")),
            Some(emitida(n, f)),
        );
    }
    // Libro de recibidas del gestor: la P-101 (que tiene PDF) y la Z-9 (que no).
    let csv = format!(
        "Fecha;Número;NIF;Proveedor;Base;Cuota;Total\n\
         10/02/2025;P-101;{PAPELERIA};Papelería Ejemplo SL;300,00;63,00;363,00\n\
         20/03/2025;Z-9;{ASESORES};Asesores Ejemplo SL;200,00;42,00;242,00\n"
    );
    fichero(
        "LIBROS/2025/recibidas.csv",
        csv.into_bytes(),
        Some(json!({
            "tipo": "libro_recibidas",
            "descripcion": "Libro de facturas recibidas de 2025",
            "hoja": {
                "fila_titulos": 1, "primera_fila": 2,
                "columnas": {
                    "fecha": "A", "numero": "B", "nif": "C", "nombre": "D",
                    "base": "E", "cuota_iva": "F", "total": "G"
                }
            }
        })),
    );
    // 303 del 1T de 2025: deduce 500 € de base y las facturas suman 310 €.
    fichero(
        "MODELOS/2025/303-1T.pdf",
        pdf("Modelo 303 1T 2025"),
        Some(modelo(
            "303",
            2025,
            "1T",
            json!({"07": "3000.00", "09": "630.00", "28": "500.00", "29": "105.00"}),
        )),
    );
    // 200 de 2024 con base negativa y de 2025 con dividendos sin exención y más bases
    // negativas compensadas de las que quedaban. Entidad de reducida dimensión (cifra de
    // negocios de 2 M€): en 2025, el 24 %.
    fichero(
        "MODELOS/2024/200.pdf",
        pdf("Modelo 200 2024"),
        Some(modelo(
            "200",
            2024,
            "0A",
            json!({
                "00255": "2000000.00", "00500": "-10000.00", "00550": "-10000.00",
                "01330": "-10000.00", "00562": "0"
            }),
        )),
    );
    fichero(
        "MODELOS/2025/200.pdf",
        pdf("Modelo 200 2025"),
        Some(modelo(
            "200",
            2025,
            "0A",
            json!({
                "00255": "2000000.00", "00298": "20000.00", "00299": "20000.00",
                "00500": "80000.00", "00550": "80000.00", "00547": "15000.00",
                "00552": "65000.00", "01330": "65000.00", "00562": "15600.00",
                "01785": "3800.00", "01501": "3000.00", "01502": "3000.00"
            }),
        )),
    );
    // Un fichero que no se lee y otro sin lectura.
    fichero(
        "OTROS/factura-antigua.zip",
        b"PK\x03\x04 zip".to_vec(),
        None,
    );
    fichero("OTROS/nota.pdf", pdf("Nota sin importancia"), None);

    let paquete = json!({ "version": 1, "generado_por": "Claude Code", "lecturas": lecturas });
    std::fs::write(
        root.join("taxpain-lecturas.json"),
        serde_json::to_vec_pretty(&paquete).unwrap(),
    )
    .unwrap();
}

fn revisar() -> Revision {
    let dir = tempfile::tempdir().unwrap();
    carpeta(dir.path());
    revision::revisar(&vault(), dir.path(), date!(2026 - 10 - 25)).unwrap()
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

fn tabla<'a>(r: &'a Revision, titulo: &str) -> &'a [Vec<String>] {
    &r.tablas
        .iter()
        .find(|t| t.titulo == titulo)
        .unwrap_or_else(|| panic!("falta la tabla {titulo}"))
        .filas
}

#[test]
fn resumen_de_la_carpeta() {
    let r = revisar();
    assert_eq!(r.nif.as_deref(), Some(PROPIO));
    assert_eq!(r.generada, "2026-10-25");
    // 17 que se leen (con la copia) y el zip.
    assert_eq!(r.resumen.ficheros, 18);
    assert_eq!(r.resumen.no_admitidos, 1);
    assert!(r.resumen.errores >= 1);
    // Primero los urgentes (pueden suponer una deuda; entre ellos manda el importe, ver
    // `revision::urgente::ordenar`) y después el resto, de más a menos grave.
    let urgentes = r
        .hallazgos
        .iter()
        .take_while(|h| h.urgente.is_some())
        .count();
    assert!(r.hallazgos[urgentes..].iter().all(|h| h.urgente.is_none()));
    let resto: Vec<Gravedad> = r.hallazgos[urgentes..].iter().map(|h| h.gravedad).collect();
    let mut ordenado = resto.clone();
    ordenado.sort();
    assert_eq!(resto, ordenado);
}

#[test]
fn ficheros_copias_y_formatos() {
    let r = revisar();
    let copias = hallazgo(&r, "ficheros.copias");
    assert_eq!(copias.gravedad, Gravedad::Aviso);
    assert_eq!(copias.ficheros.len(), 2);
    let zip = hallazgo(&r, "ficheros.no_admitidos");
    assert_eq!(zip.ficheros, ["OTROS/factura-antigua.zip"]);
    assert!(
        r.pedir
            .iter()
            .any(|p| p.que.contains("factura-antigua.zip"))
    );
    let sin = hallazgo(&r, "ficheros.sin_leer");
    assert!(sin.ficheros.iter().any(|f| f == "OTROS/nota.pdf"));
}

#[test]
fn facturas_con_problemas() {
    let r = revisar();
    // La misma factura en dos ficheros distintos (la copia exacta no cuenta aquí).
    let rep = hallazgo(&r, "facturas.repetidas");
    assert_eq!(rep.ficheros.len(), 2);
    assert!(rep.ficheros.iter().all(|f| f.contains("asesores-7")));
    // La de los asesores lleva retención: cuadra. La mal sumada, no.
    let mal = tabla(&r, "Facturas cuyo total no cuadra");
    assert_eq!(mal.len(), 1);
    assert!(mal[0][0].ends_with("mal-sumada.pdf"));
    let nif = tabla(&r, "NIF con la letra de control mal");
    assert_eq!(nif.len(), 1);
    assert_eq!(nif[0][2], "B12345675");
    // Ni el NIF-IVA irlandés ni el número de EE. UU. son «NIF mal».
    assert!(
        !nif.iter()
            .any(|f| f[2].starts_with("IE") || f[2].starts_with("US"))
    );
    let otro = hallazgo(&r, "facturas.a_nombre_de_otro");
    assert_eq!(otro.ficheros, ["FACTURAS/2025/T2/GASTOS/personal.pdf"]);
    assert!(otro.detalle.contains("persona física"));
    let fuera = tabla(&r, "Proveedores de fuera de España que cobran IVA");
    assert_eq!(fuera.len(), 1);
    assert_eq!(fuera[0][0], "Nube Ejemplo Ltd");
    assert_eq!(fuera[0][3], "21 %");
    let usd = tabla(&r, "Facturas en otra moneda, por emisor");
    assert_eq!(usd[0][1], "USD");
    assert_eq!(usd[0][3], "20,00 USD");
    // La copia escaneada está en la carpeta del 3T y es del 2T.
    let otra = hallazgo(&r, "facturas.otro_trimestre");
    assert_eq!(
        otra.ficheros,
        ["FACTURAS/2025/T3/GASTOS/asesores-7-escaneada.pdf"]
    );
}

#[test]
fn numeracion_de_las_emitidas() {
    let r = revisar();
    let s = hallazgo(&r, "emitidas.saltos");
    assert!(
        s.detalle.contains("A-3") || s.detalle.contains('3'),
        "{}",
        s.detalle
    );
}

#[test]
fn libro_frente_a_los_pdf() {
    let r = revisar();
    let sin = hallazgo(&r, "libros.sin_pdf");
    assert!(sin.detalle.contains("Z-9"), "{}", sin.detalle);
    assert!(!sin.detalle.contains("P-101"), "{}", sin.detalle);
    // El libro sólo tiene facturas del 1T: los PDF de mayo y junio (la mal sumada, la del NIF
    // mal…) no se buscan en él, porque será de otro libro. Del 1T, la de la nube (con IVA de un
    // proveedor de fuera) no está en el libro; la de la papelería, sí.
    assert!(!hay(&r, "libros.pdf_fuera_del_libro"), "{:?}", r.hallazgos);
    let fuera = hallazgo(&r, "libros.otras_fuera_del_libro");
    assert_eq!(fuera.ficheros, ["FACTURAS/2025/T1/GASTOS/nube.pdf"]);
    assert!(
        fuera.detalle.contains("del 01/01/2025 al 31/03/2025"),
        "{}",
        fuera.detalle
    );
}

#[test]
fn el_303_frente_a_las_facturas() {
    let r = revisar();
    // Sin perfil fiscal guardado, los modelos que faltan se miran con el deducido de los modelos
    // de la carpeta.
    assert!(!hay(&r, "modelos.sin_perfil"));
    assert!(hay(&r, "modelos.perfil_deducido"));
    // Con el libro de recibidas del trimestre manda el libro: los 500 € del modelo son los
    // 300 € de la papelería y los 200 € de los asesores, y que falte el PDF de los asesores es
    // un papel que conseguir (libros.sin_pdf), no un 303 que deduce de más.
    let h = hallazgo(&r, "modelos.303_cuadra_libro");
    assert_eq!(h.ejercicio, Some(2025));
    assert!(h.detalle.contains("(500,00 €)"), "{}", h.detalle);
    assert!(!hay(&r, "modelos.303_falta_factura"));
    // Las ventas sí casan: 3.000 € en la [07] y tres emitidas de 1.000 €.
    assert!(!hay(&r, "modelos.303_devengado"));
}

#[test]
fn impuesto_sobre_sociedades() {
    let r = revisar();
    let ex = hallazgo(&r, "sociedades.exencion_dividendos");
    assert_eq!(ex.gravedad, Gravedad::Error);
    assert_eq!(ex.ejercicio, Some(2025));
    // 95 % de 20.000 € = 19.000 € menos de base al 24 %: 4.560 €.
    assert_eq!(ex.importe, Some(Money::from_euros(4_560, 0)));
    // El 25/07/2026 fue sábado: el plazo del 200 de 2025 acabó el lunes 27 y se puede rectificar
    // hasta el 27/07/2030.
    assert!(ex.detalle.contains("27/07/2030"), "{}", ex.detalle);
    assert_eq!(ex.rectificable_hasta.as_deref(), Some("2030-07-27"));
    // Periodo iniciado en 2024 o después: autoliquidación rectificativa, no solicitud.
    assert!(
        ex.detalle.contains("autoliquidación rectificativa") && ex.detalle.contains("120.4"),
        "{}",
        ex.detalle
    );
    assert!(!ex.detalle.contains("120.3"));
    // En 2024 quedaron 10.000 € de bases negativas y en 2025 se compensan 15.000 €.
    let bin = hallazgo(&r, "sociedades.bases_negativas");
    assert_eq!(bin.ejercicio, Some(2025));
    assert_eq!(bin.importe, Some(-Money::from_euros(1_200, 0)));
    assert!(bin.titulo.contains("5.000,00 €"), "{}", bin.titulo);
    // Retenido el 19 % de los dividendos de las participadas.
    assert!(hay(&r, "sociedades.retencion_dividendos"));
    // El cuadro de participaciones sin lo cobrado de cada una (casilla 01503): hay que
    // rellenarlo en la rectificativa.
    let cuadro = hallazgo(&r, "sociedades.dividendos_sin_detallar");
    assert_eq!(cuadro.gravedad, Gravedad::Aviso);
    assert_eq!(cuadro.ejercicio, Some(2025));
    assert!(cuadro.detalle.contains("20.000,00 €"), "{}", cuadro.detalle);
    assert!(
        r.pedir
            .iter()
            .any(|p| p.que.contains("2025") && p.que.contains("casilla 01503"))
    );
    assert_eq!(r.resumen.recuperable, Money::from_euros(3_360, 0));
    assert!(r.pedir.iter().any(|p| {
        p.que
            .contains("Rectificar el Impuesto sobre Sociedades de 2025")
            && p.que.contains("autoliquidación rectificativa")
    }));
    assert_eq!(
        tabla(&r, "Impuesto sobre Sociedades por ejercicio").len(),
        2
    );
}

#[test]
fn la_ultima_revision_se_guarda() {
    let v = vault();
    assert!(revision::ultima(&v).is_none());
    let r = revisar();
    revision::guardar_ultima(&v, &r).unwrap();
    assert_eq!(revision::ultima(&v), Some(r));
}

#[test]
fn rutas_del_mac_con_la_tilde_separada() {
    let dir = tempfile::tempdir().unwrap();
    // «AÑO» como lo guarda el Finder: N + tilde combinante.
    escribir(dir.path(), "MODELOS A\u{d1}O 2025/copia.zip", b"PK");
    escribir(dir.path(), "MODELOS AN\u{303}O 2024/viejo.zip", b"PK");
    let r = revision::revisar(&vault(), dir.path(), date!(2026 - 10 - 25)).unwrap();
    let h = hallazgo(&r, "ficheros.no_admitidos");
    assert!(
        h.ficheros
            .contains(&"MODELOS A\u{d1}O 2024/viejo.zip".to_owned())
    );
    let pdf = revision::informe::pdf(&r).unwrap();
    assert!(pdf.starts_with(b"%PDF"));
}

#[test]
fn informe_en_markdown_pdf_y_excel() {
    let r = revisar();
    let md = revision::informe::markdown(&r);
    assert!(md.starts_with("# Revisión de la carpeta de Ejemplo Revisión SL (B98765431)"));
    assert!(md.contains("## Errores"));
    assert!(md.contains("## Qué pedir"));
    assert!(md.contains("### Impuesto sobre Sociedades por ejercicio"));
    let pdf = revision::informe::pdf(&r).unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    let xlsx = revision::informe::excel(&r).unwrap();
    assert!(xlsx.starts_with(b"PK"));
    assert_eq!(
        revision::informe::nombre_fichero(&r, "pdf"),
        "revision-2026-10-25.pdf"
    );
    // La revisión se puede guardar y volver a leer (la app la pasa a la pantalla en JSON).
    let json = serde_json::to_string(&r).unwrap();
    let otra: Revision = serde_json::from_str(&json).unwrap();
    assert_eq!(otra.hallazgos.len(), r.hallazgos.len());
}

/// Una carpeta a medida: ficheros con (o sin) lectura, y su revisión.
fn revisar_carpeta(ficheros: Vec<(&str, Vec<u8>, Option<Value>)>) -> Revision {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut lecturas = Vec::new();
    for (ruta, bytes, lectura) in ficheros {
        escribir(root, ruta, &bytes);
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
    revision::revisar(&vault(), root, date!(2026 - 10 - 25)).unwrap()
}

#[test]
fn las_retenciones_del_alquiler_van_al_115_y_no_al_111() {
    let mut profesional = recibida(
        ASESORES,
        "Asesores Ejemplo SL",
        "2025/3",
        "2025-02-10",
        ["300.00", "63.00", "318.00"],
    );
    profesional["factura"]["retencion"] = json!("45.00");
    let mut alquiler = recibida(
        "12345678Z",
        "Propietaria Ejemplo",
        "ALQ-2",
        "2025-02-01",
        ["1000.00", "210.00", "1020.00"],
    );
    alquiler["factura"]["retencion"] = json!("190.00");
    let r = revisar_carpeta(vec![
        (
            "GASTOS/asesores.pdf",
            pdf("Asesores Ejemplo, factura 2025/3"),
            Some(profesional),
        ),
        (
            "GASTOS/alquiler.pdf",
            pdf("Alquiler oficina febrero"),
            Some(alquiler),
        ),
        (
            "MODELOS/111-1T.pdf",
            pdf("Modelo 111 1T 2025"),
            Some(modelo(
                "111",
                2025,
                "1T",
                json!({"08": "300.00", "09": "45.00"}),
            )),
        ),
        (
            "MODELOS/115-1T.pdf",
            pdf("Modelo 115 1T 2025"),
            Some(modelo(
                "115",
                2025,
                "1T",
                json!({"02": "1000.00", "03": "100.00"}),
            )),
        ),
    ]);
    // El 111 casa con la factura del profesional (15 %): la del alquiler (19 %) no cuenta.
    assert!(!hay(&r, "modelos.111_retenciones"), "{:?}", r.hallazgos);
    // El 115 no casa: declara 100 € y el alquiler retiene 190 €.
    let h = hallazgo(&r, "modelos.115_retenciones");
    assert!(
        h.detalle.contains("100,00 €") && h.detalle.contains("190,00 €"),
        "{}",
        h.detalle
    );
}

#[test]
fn el_303_con_facturas_sin_base_leida_no_acusa_a_la_gestoria() {
    use taxpain_app::revision::Gravedad;

    // Una recibida leída con IA (300 € de base) y otra reconocida sólo por las reglas (sin base).
    let leida = recibida(
        PAPELERIA,
        "Papelería Ejemplo SL",
        "P-101",
        "2025-02-10",
        ["300.00", "63.00", "363.00"],
    );
    let por_reglas = text_document(
        "Factura",
        &[vec![
            Para::Bold("Papelería Ejemplo SL".into()),
            Para::Text(format!("NIF {PAPELERIA}")),
            Para::Text("Factura nº P-102".into()),
            Para::Text("Fecha 15/02/2025".into()),
            Para::Text("Total 242,00 €".into()),
        ]],
    )
    .unwrap();
    let m303 = modelo("303", 2025, "1T", json!({"28": "500.00", "29": "105.00"}));
    let r = revisar_carpeta(vec![
        (
            "GASTOS/papeleria-101.pdf",
            pdf("Papelería Ejemplo, factura P-101"),
            Some(leida),
        ),
        ("GASTOS/papeleria-102.pdf", por_reglas, None),
        ("MODELOS/303-1T.pdf", pdf("Modelo 303 1T 2025"), Some(m303)),
    ]);
    let h = hallazgo(&r, "modelos.303_falta_factura");
    assert_eq!(h.gravedad, Gravedad::Info, "{}", h.detalle);
    assert!(
        h.detalle.contains("1 factura del trimestre sin base leída"),
        "{}",
        h.detalle
    );
    assert!(
        !r.pedir.iter().any(|p| p.que.contains("deducidos de más")),
        "{:?}",
        r.pedir
    );
}

#[test]
fn el_303_de_2024_con_ventas_al_cinco_por_ciento_casa_con_las_emitidas() {
    let emitida = json!({
        "tipo": "factura_emitida",
        "descripcion": "Factura A-1 a Cliente Ejemplo",
        "factura": {
            "nif_emisor": PROPIO, "nombre_emisor": "Ejemplo Revisión SL",
            "nif_destinatario": CLIENTE, "nombre_destinatario": "Cliente Ejemplo SL",
            "numero": "A-1", "fecha": "2024-02-10",
            "base": "1000.00", "iva": "50.00", "total": "1050.00"
        }
    });
    // Las ventas al 5 % van en las casillas [153]-[155] (Orden HFP/1124/2022).
    let m303 = modelo(
        "303",
        2024,
        "1T",
        json!({"153": "1000.00", "154": "5", "155": "50.00"}),
    );
    let r = revisar_carpeta(vec![
        (
            "INGRESOS/A-1.pdf",
            pdf("Ejemplo Revisión, factura A-1"),
            Some(emitida),
        ),
        (
            "MODELOS/303-1T-2024.pdf",
            pdf("Modelo 303 1T 2024"),
            Some(m303),
        ),
    ]);
    assert!(!hay(&r, "modelos.303_devengado"), "{:?}", r.hallazgos);
}

/// El libro de emitidas que es la autorrepercusión de compras (sus líneas son las del libro de
/// recibidas, con el total sin la cuota): no son ventas, así que ni se cruzan con los PDF de
/// emitidas ni cuentan en su numeración; la revisión lo dice en una nota.
#[test]
fn el_libro_de_emitidas_de_autorrepercusion_no_cuenta_como_ventas() {
    let recibidas = "Fecha;Número;NIF Proveedor;Proveedor;País;Base;Cuota;Total\n\
                     05/01/2026;NB-0001;IE1234567FA;Proveedor Nube Ltd;IE;15,00;3,15;15,00\n\
                     05/03/2026;NB-0005;IE1234567FA;Proveedor Nube Ltd;IE;15,00;3,15;15,00\n";
    let emitidas = "Fecha Expedición;Número Factura;NIF Destinatario;Nombre Destinatario;Base Imponible;Tipo IVA;Cuota IVA;Total\n\
                    05/01/2026;NB-0001;IE1234567FA;Proveedor Nube Ltd;15,00;21;3,15;18,15\n\
                    05/03/2026;NB-0005;IE1234567FA;Proveedor Nube Ltd;15,00;21;3,15;18,15\n";
    let r = revisar_carpeta(vec![
        (
            "LIBROS/Libro recibidas 2026.csv",
            recibidas.as_bytes().to_vec(),
            None,
        ),
        (
            "LIBROS/Libro emitidas 2026.csv",
            emitidas.as_bytes().to_vec(),
            None,
        ),
    ]);
    let nota = hallazgo(&r, "libros.autorrepercusion");
    assert_eq!(
        nota.titulo,
        "El libro de emitidas de 2026 es la autorrepercusión de compras"
    );
    assert!(nota.detalle.contains("NB-0001"), "{}", nota.detalle);
    assert_eq!(nota.gravedad, Gravedad::Info);
    assert!(
        !r.hallazgos.iter().any(|h| h.codigo == "emitidas.saltos"
            || (h.codigo.starts_with("libros.") && h.titulo.contains("emitidas de 2026 sin"))),
        "{:?}",
        r.hallazgos
            .iter()
            .map(|h| (&h.codigo, &h.titulo))
            .collect::<Vec<_>>()
    );
}

/// Un libro de emitidas cuya hoja se llama «Entregas intracomunitarias» es de ventas (exentas,
/// sin cuota): la revisión no lo toma por autorrepercusión y sigue cruzándolo con los PDF.
#[test]
fn el_libro_de_entregas_intracomunitarias_cuenta_como_ventas() {
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("Entregas intracomunitarias").unwrap();
    let filas = [
        "Fecha Expedición;Número Factura;NIF Destinatario;Nombre Destinatario;Base Imponible;Tipo IVA;Cuota IVA;Total",
        "10/03/2025;E-1;FR12345678901;Client Demo SARL;1.000,00;0;0,00;1.000,00",
        "12/03/2025;E-2;DE123456789;Kunde Demo GmbH;2.000,00;0;0,00;2.000,00",
    ];
    for (r, linea) in filas.iter().enumerate() {
        for (c, celda) in linea.split(';').enumerate() {
            ws.write_string(u32::try_from(r).unwrap(), u16::try_from(c).unwrap(), celda)
                .unwrap();
        }
    }
    let r = revisar_carpeta(vec![(
        "LIBROS/Libro emitidas 2025.xlsx",
        wb.save_to_buffer().unwrap(),
        None,
    )]);
    assert!(!hay(&r, "libros.autorrepercusion"), "{:?}", r.hallazgos);
    let h = hallazgo(&r, "libros.sin_pdf");
    assert!(
        h.titulo.starts_with("2 facturas del libro de emitidas"),
        "{}",
        h.titulo
    );
}

/// Que la lectura con IA no llame «libro_emitidas» a un libro de ventas normal (con la cuota en
/// el total) no lo convierte en autorrepercusión: la revisión lo sigue cruzando como ventas.
#[test]
fn la_lectura_con_ia_no_convierte_un_libro_de_ventas_en_autorrepercusion() {
    let emitidas = format!(
        "Fecha Expedición;Número Factura;NIF Destinatario;Nombre Destinatario;Base Imponible;Tipo IVA;Cuota IVA;Total\n\
         10/02/2025;A-101;{CLIENTE};Cliente Ejemplo SL;1.000,00;21;210,00;1.210,00\n\
         10/03/2025;A-103;{CLIENTE};Cliente Ejemplo SL;1.000,00;21;210,00;1.210,00\n"
    );
    for tipo in ["otro", "libro_recibidas"] {
        let r = revisar_carpeta(vec![(
            "LIBROS/Libro emitidas 2025.csv",
            emitidas.as_bytes().to_vec(),
            Some(json!({"tipo": tipo, "descripcion": "Listado de facturas de 2025"})),
        )]);
        assert!(
            !hay(&r, "libros.autorrepercusion"),
            "{tipo}: {:?}",
            r.hallazgos
        );
        assert!(hay(&r, "emitidas.saltos"), "{tipo}: {:?}", r.hallazgos);
        let h = hallazgo(&r, "libros.sin_pdf");
        assert!(
            h.titulo.starts_with("2 facturas del libro de emitidas"),
            "{}",
            h.titulo
        );
    }
}

/// Un año del que sólo está el libro de emitidas con la autorrepercusión de compras sigue siendo
/// un año con datos: esas compras son operaciones del año, así que se siguen pidiendo la
/// legalización de los libros y el depósito de las cuentas de ese ejercicio.
#[test]
fn la_autorrepercusion_cuenta_como_datos_del_año() {
    let emitidas = "Fecha Expedición;Número Factura;NIF Destinatario;Nombre Destinatario;Base Imponible;Tipo IVA;Cuota IVA;Total\n\
                    05/01/2025;NB-0001;IE1234567FA;Proveedor Nube Ltd;15,00;21;3,15;15,00\n\
                    05/03/2025;NB-0005;IE1234567FA;Proveedor Nube Ltd;15,00;21;3,15;15,00\n";
    let r = revisar_carpeta(vec![(
        "LIBROS/Libro emitidas 2025.csv",
        emitidas.as_bytes().to_vec(),
        None,
    )]);
    assert!(hay(&r, "libros.autorrepercusion"), "{:?}", r.hallazgos);
    for codigo in [
        "cruces.legalizacion_sin_justificante",
        "cruces.deposito_sin_justificante",
    ] {
        let h = hallazgo(&r, codigo);
        assert!(h.titulo.contains("2025"), "{}", h.titulo);
    }
}

/// La autorrepercusión también es de compras a proveedores españoles con inversión del sujeto
/// pasivo (una ejecución de obra, art. 84.Uno.2º f LIVA): la nota no dice que sean de fuera de
/// España.
#[test]
fn la_nota_de_autorrepercusion_vale_para_proveedores_espanoles() {
    let recibidas = format!(
        "Fecha;Número;NIF Proveedor;Proveedor;País;Base;Cuota;Total\n\
         10/04/2026;OB-7;{PAPELERIA};Constructora Demo SL;;8.000,00;1.680,00;8.000,00\n"
    );
    let emitidas = format!(
        "Fecha Expedición;Número Factura;NIF Destinatario;Nombre Destinatario;Base Imponible;Tipo IVA;Cuota IVA;Total\n\
         10/04/2026;OB-7;{PAPELERIA};Constructora Demo SL;8.000,00;21;1.680,00;8.000,00\n"
    );
    let r = revisar_carpeta(vec![
        (
            "LIBROS/Libro recibidas 2026.csv",
            recibidas.into_bytes(),
            None,
        ),
        (
            "LIBROS/Libro emitidas 2026.csv",
            emitidas.into_bytes(),
            None,
        ),
    ]);
    let nota = hallazgo(&r, "libros.autorrepercusion");
    assert!(
        !nota.detalle.contains("fuera de España"),
        "{}",
        nota.detalle
    );
    assert!(
        nota.detalle.contains("inversión del sujeto pasivo"),
        "{}",
        nota.detalle
    );
}
