//! «Modelos que faltan» en la revisión de la carpeta: lo que no está frente a lo que está sin
//! leer, lo que dice la AEAT en los ejercicios comprobados, el 115 y el 180 sólo con indicios de
//! un alquiler, el 202 según la cuota del 200 que le sirve de base, el 347 sin líneas por sus
//! totales y el IVA de antes del alta en el censo. Datos inventados (NIF de prueba).

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};
use taxpain_app::companies::{self, CompanyInfo};
use taxpain_app::presentado_aeat::{self, Answer, Presented};
use taxpain_app::revision::{self, Hallazgo, Revision};
use taxpain_core::calendar::TaxpayerProfile;
use taxpain_invoicing::pdf::{Para, text_document};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

const PROPIA: &str = "B12345674";
const PROVEEDOR: &str = "B11111119";
const OTRO: &str = "B22222228";

fn vault(perfil: &TaxpayerProfile) -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    companies::set_info(
        &v,
        &CompanyInfo {
            name: "Empresa Demo SL".into(),
            nif: Some(PROPIA.into()),
        },
        datetime!(2026-01-01 9:00 UTC),
    )
    .unwrap();
    v.set_setting("taxpayer_profile", perfil).unwrap();
    v
}

/// Una SL que sólo presenta el Impuesto sobre Sociedades y sus pagos fraccionados (sin IVA ni
/// retenciones), para mirar el 202 sin más modelos alrededor.
fn solo_sociedades() -> TaxpayerProfile {
    TaxpayerProfile {
        iva: None,
        retiene_trabajo_profesionales: false,
        retiene_alquileres: false,
        ..TaxpayerProfile::sociedad_limitada()
    }
}

fn pdf(texto: &str) -> Vec<u8> {
    text_document("Documento", &[vec![Para::Text(texto.into())]]).unwrap()
}

fn modelo(modelo: &str, ejercicio: i32, periodo: &str, casillas: Value) -> Value {
    json!({
        "tipo": "modelo_aeat",
        "descripcion": format!("Modelo {modelo} {periodo} {ejercicio}"),
        "modelo": {"modelo": modelo, "ejercicio": ejercicio, "periodo": periodo, "casillas": casillas}
    })
}

fn recibida(nif: &str, numero: &str, fecha: &str, importes: [&str; 4]) -> Value {
    json!({
        "tipo": "factura_recibida",
        "descripcion": format!("Factura {numero}"),
        "factura": {
            "nif_emisor": nif, "nombre_emisor": "Proveedor Demo SL",
            "nif_destinatario": PROPIA, "nombre_destinatario": "Empresa Demo SL",
            "numero": numero, "fecha": fecha,
            "base": importes[0], "iva": importes[1], "retencion": importes[2], "total": importes[3]
        }
    })
}

fn escribir(root: &Path, ruta: &str, bytes: &[u8]) {
    let p = root.join(ruta);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

/// Revisa una carpeta con sus ficheros y lecturas, con el almacén `v`.
fn revisar_con(v: &Vault, ficheros: Vec<(String, Vec<u8>, Option<Value>)>) -> Revision {
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
    revision::revisar(v, root, date!(2026 - 10 - 31)).unwrap()
}

/// Un fichero con su lectura (cada uno con su texto, porque las lecturas van por la huella).
fn con(ruta: &str, lectura: Value) -> (String, Vec<u8>, Option<Value>) {
    (
        ruta.to_owned(),
        pdf(&format!("Documento {ruta}")),
        Some(lectura),
    )
}

fn faltan(r: &Revision, y: i32) -> Option<&Hallazgo> {
    r.hallazgos
        .iter()
        .find(|h| h.codigo == "modelos.faltan" && h.ejercicio == Some(y))
}

fn titulos(r: &Revision) -> Vec<String> {
    r.hallazgos
        .iter()
        .map(|h| format!("{}: {} — {}", h.codigo, h.titulo, h.detalle))
        .collect()
}

#[test]
fn sin_indicios_de_alquiler_no_se_piden_el_115_ni_el_180() {
    let v = vault(&TaxpayerProfile::sociedad_limitada());
    let r = revisar_con(
        &v,
        vec![
            con(
                "MODELOS/303_1T.pdf",
                modelo("303", 2025, "1T", json!({"71": "100.00"})),
            ),
            // El certificado de situación censal sólo tiene la retención del trabajo.
            (
                "SEDE/censal.pdf".into(),
                text_document(
                    "Documento",
                    &[[
                        "CERTIFICADO DE SITUACIÓN CENSAL",
                        "Nº DE REFERENCIA: 20250000002",
                        &format!("NIF: {PROPIA}"),
                        "SITUACIÓN TRIBUTARIA",
                        "IMPUESTO SOBRE EL VALOR AÑADIDO",
                        "Regímenes aplicables Fecha",
                        "- General 01/01/2019",
                        "OBLIGACIONES PERIÓDICAS",
                        "MODELO PERIODICIDAD",
                        "IRPF RETENCION TRABAJO PERSONAL TRIMESTRAL",
                        "IMPUESTO DE SOCIEDADES ANUAL",
                        "IMPUESTO SOBRE EL VALOR AÑADIDO TRIMESTRAL",
                        "Y para que conste, se expide la presente certificación.",
                        "Documento firmado electrónicamente (Ley 40/2015) por la Agencia Estatal de \
                         Administración Tributaria, con fecha 15 de abril de 2026. Autenticidad \
                         verificable mediante Código Seguro Verificación CENS1234DEMO5678 en \
                         sede.agenciatributaria.gob.es",
                    ]
                    .iter()
                    .map(|l| Para::Text((*l).to_owned()))
                    .collect()],
                )
                .unwrap(),
                None,
            ),
        ],
    );
    let f = faltan(&r, 2025).unwrap();
    assert!(
        !f.detalle.contains("115") && !f.detalle.contains("180"),
        "{}",
        f.detalle
    );
    assert!(!r.pedir.iter().any(|p| p.que.contains("115")));
    let nota = r
        .hallazgos
        .iter()
        .find(|h| h.codigo == "modelos.alquiler_sin_indicios")
        .unwrap_or_else(|| panic!("{:#?}", titulos(&r)));
    assert!(nota.detalle.contains("15/04/2026"), "{}", nota.detalle);

    // Con una factura de alquiler con retención (19 %), sí tocaban.
    let v = vault(&TaxpayerProfile::sociedad_limitada());
    let r = revisar_con(
        &v,
        vec![
            con(
                "MODELOS/303_1T.pdf",
                modelo("303", 2025, "1T", json!({"71": "100.00"})),
            ),
            con(
                "GASTOS/alquiler.pdf",
                recibida(
                    OTRO,
                    "A-1",
                    "2025-02-01",
                    ["1000.00", "210.00", "190.00", "1020.00"],
                ),
            ),
        ],
    );
    let f = faltan(&r, 2025).unwrap();
    assert!(f.detalle.contains("115 ("), "{}", f.detalle);
    assert!(f.detalle.contains("180"), "{}", f.detalle);
}

#[test]
fn el_202_segun_la_cuota_del_200_que_le_sirve_de_base() {
    let v = vault(&solo_sociedades());
    let r = revisar_con(
        &v,
        vec![
            // 2023: cuota positiva. 2024: pérdidas; el PDF no trae la [00599] ni la [00611],
            // pero con la base imponible negativa la cuota íntegra es cero.
            con(
                "MODELOS/200_2023.pdf",
                modelo(
                    "200",
                    2023,
                    "0A",
                    json!({"599": "5000.00", "611": "5000.00", "1586": "5000.00"}),
                ),
            ),
            con(
                "MODELOS/200_2024.pdf",
                modelo(
                    "200",
                    2024,
                    "0A",
                    json!({"552": "-2000.00", "601": "900.00", "603": "900.00", "1586": "-1800.00"}),
                ),
            ),
            // Algo de 2025 y de 2026 en la carpeta, para que se revisen esos años.
            con(
                "GASTOS/2025.pdf",
                recibida(
                    PROVEEDOR,
                    "P-25",
                    "2025-05-10",
                    ["100.00", "21.00", "0", "121.00"],
                ),
            ),
            con(
                "GASTOS/2026.pdf",
                recibida(
                    PROVEEDOR,
                    "P-26",
                    "2026-03-10",
                    ["100.00", "21.00", "0", "121.00"],
                ),
            ),
        ],
    );
    // 2025: el 1P toca (cuota de 2023) y falta; el 2P y el 3P no (cuota de 2024 a cero): ni se
    // piden ni quedan como dudosos.
    let f = faltan(&r, 2025).unwrap_or_else(|| panic!("{:#?}", titulos(&r)));
    assert!(f.detalle.contains("202 (1P)"), "{}", f.detalle);
    assert!(!f.detalle.contains("2P"), "{}", f.detalle);
    assert!(
        !r.hallazgos
            .iter()
            .any(|h| h.codigo == "modelos.no_se_sabe" && h.ejercicio == Some(2025)),
        "{:#?}",
        titulos(&r)
    );
    // 2026: el 1P no toca (cuota de 2024 a cero); el 2P depende del 200 de 2025, que no está:
    // no se pide, se dice.
    assert!(faltan(&r, 2026).is_none(), "{:#?}", titulos(&r));
    let nota = r
        .hallazgos
        .iter()
        .find(|h| h.codigo == "modelos.no_se_sabe" && h.ejercicio == Some(2026))
        .unwrap_or_else(|| panic!("{:#?}", titulos(&r)));
    assert!(
        nota.detalle.contains("202 del 2P") && nota.detalle.contains("2025"),
        "{}",
        nota.detalle
    );
    assert!(!nota.detalle.contains("1P"), "{}", nota.detalle);
    assert!(
        !r.pedir
            .iter()
            .any(|p| p.que.contains("2026") && p.que.contains("202")),
        "{:?}",
        r.pedir
    );
}

#[test]
fn el_347_sin_lineas_se_compara_por_sus_totales() {
    let perfil = TaxpayerProfile {
        obligado_347: true,
        ..solo_sociedades()
    };
    let v = vault(&perfil);
    let r = revisar_con(
        &v,
        vec![
            con(
                "GASTOS/a.pdf",
                recibida(
                    PROVEEDOR,
                    "P-1",
                    "2025-03-10",
                    ["3000.00", "630.00", "0", "3630.00"],
                ),
            ),
            con(
                "GASTOS/b.pdf",
                recibida(
                    OTRO,
                    "O-1",
                    "2025-05-10",
                    ["4000.00", "840.00", "0", "4840.00"],
                ),
            ),
            // El 347 de 2025 sin sus líneas: 2 declarados por 8.470,00 €, lo mismo que las facturas.
            con(
                "MODELOS/347_2025.pdf",
                modelo("347", 2025, "0A", json!({"01": "2", "02": "8470.00"})),
            ),
        ],
    );
    let t = r
        .tablas
        .iter()
        .find(|t| t.titulo == "347 frente a las facturas, por sus totales")
        .unwrap_or_else(|| panic!("{:#?}", titulos(&r)));
    assert_eq!(t.filas[0][0], "2025");
    assert_eq!(t.filas[0][5], "0,00 €");
    assert!(
        !r.hallazgos
            .iter()
            .any(|h| h.codigo == "cruces.347_totales" || h.codigo == "cruces.sin_registros"),
        "{:#?}",
        titulos(&r)
    );

    // Si declara un tercero de más, una nota (no se pide nada).
    let v = vault(&perfil);
    let r = revisar_con(
        &v,
        vec![
            con(
                "GASTOS/a.pdf",
                recibida(
                    PROVEEDOR,
                    "P-1",
                    "2025-03-10",
                    ["3000.00", "630.00", "0", "3630.00"],
                ),
            ),
            con(
                "MODELOS/347_2025.pdf",
                modelo("347", 2025, "0A", json!({"01": "2", "02": "8470.00"})),
            ),
        ],
    );
    let h = r
        .hallazgos
        .iter()
        .find(|h| h.codigo == "cruces.347_totales")
        .unwrap_or_else(|| panic!("{:#?}", titulos(&r)));
    assert!(
        h.detalle.contains("2 declarados por 8.470,00 €"),
        "{}",
        h.detalle
    );
    assert!(!r.pedir.iter().any(|p| p.que.contains("347")));
}

fn presentada(model: &str, period: &str, n: u32) -> Presented {
    Presented {
        model: model.into(),
        year: 2025,
        period: period.into(),
        csv: format!("CSV{model}{period}{n:010}"),
        file_number: format!("2025{model}0000{n}"),
        receipt: format!("{model}{n:010}"),
        previous_receipt: None,
        presented_at: format!("2025-{:02}-15 10:00:00", 4 + n),
    }
}

#[test]
fn en_un_ejercicio_comprobado_con_la_aeat_manda_lo_que_tiene_la_aeat() {
    let v = vault(&TaxpayerProfile::sociedad_limitada());
    // La AEAT tiene los 303 del 1T al 3T y el 111 del 1T de 2025; no tiene el 4T ni el 390, que
    // tocaban seguro, ni el resto de los 111, que sólo tocaban si hubo retenciones.
    let a = Answer {
        models: ["303", "390", "111", "190", "115", "180", "202", "200"]
            .iter()
            .map(|m| (*m).to_owned())
            .collect(),
        list: vec![
            presentada("303", "1T", 1),
            presentada("303", "2T", 2),
            presentada("303", "3T", 3),
            presentada("111", "1T", 4),
        ],
        ..Answer::default()
    };
    presentado_aeat::record(&v, PROPIA, 2025, &a, datetime!(2026-10-20 9:00 UTC)).unwrap();
    let r = revisar_con(
        &v,
        vec![con(
            "MODELOS/303_1T.pdf",
            modelo("303", 2025, "1T", json!({"71": "100.00"})),
        )],
    );
    let f = faltan(&r, 2025).unwrap_or_else(|| panic!("{:#?}", titulos(&r)));
    assert!(f.detalle.contains("La AEAT no tiene"), "{}", f.detalle);
    assert!(f.detalle.contains("303 (4T)"), "{}", f.detalle);
    assert!(f.detalle.contains("390"), "{}", f.detalle);
    assert!(!f.detalle.contains("111"), "{}", f.detalle);
    // Los 111 del 2T al 4T: no se piden; se dice que sólo tocaban con retenciones.
    let nota = r
        .hallazgos
        .iter()
        .find(|h| h.codigo == "modelos.no_se_sabe" && h.ejercicio == Some(2025))
        .unwrap_or_else(|| panic!("{:#?}", titulos(&r)));
    assert!(nota.detalle.contains("111"), "{}", nota.detalle);
    assert!(nota.detalle.contains("retención"), "{}", nota.detalle);
}

#[test]
fn el_iva_de_antes_del_alta_en_el_censo_no_se_pide() {
    let v = vault(&TaxpayerProfile {
        retiene_trabajo_profesionales: false,
        retiene_alquileres: false,
        pagos_fraccionados_is: false,
        ..TaxpayerProfile::sociedad_limitada()
    });
    let censal = text_document(
        "Documento",
        &[[
            "CERTIFICADO DE SITUACIÓN CENSAL",
            "Nº DE REFERENCIA: 20250000003",
            &format!("NIF: {PROPIA}"),
            "SITUACIÓN TRIBUTARIA",
            "IMPUESTO SOBRE EL VALOR AÑADIDO",
            "Regímenes aplicables Fecha",
            "- General 01/07/2024",
            "ACTIVIDADES ECONÓMICAS",
            "Descripción: SERVICIOS DEMO Fecha de alta: 01/07/2024",
            "OBLIGACIONES PERIÓDICAS",
            "IMPUESTO SOBRE EL VALOR AÑADIDO TRIMESTRAL",
            "Y para que conste, se expide la presente certificación.",
            "Documento firmado electrónicamente (Ley 40/2015) por la Agencia Estatal de \
             Administración Tributaria, con fecha 10 de enero de 2025. Autenticidad verificable \
             mediante Código Seguro Verificación CENS1234DEMO9999 en sede.agenciatributaria.gob.es",
        ]
        .iter()
        .map(|l| Para::Text((*l).to_owned()))
        .collect()],
    )
    .unwrap();
    let r = revisar_con(
        &v,
        vec![
            con(
                "GASTOS/a.pdf",
                recibida(
                    PROVEEDOR,
                    "P-1",
                    "2024-09-10",
                    ["100.00", "21.00", "0", "121.00"],
                ),
            ),
            ("SEDE/censal.pdf".into(), censal, None),
        ],
    );
    let f = faltan(&r, 2024).unwrap_or_else(|| panic!("{:#?}", titulos(&r)));
    // Alta el 1 de julio: el 1T y el 2T no tocaban; el 3T, el 4T y el 390, sí.
    assert!(f.detalle.contains("303 (3T, 4T)"), "{}", f.detalle);
    let fila = r
        .tablas
        .iter()
        .find(|t| t.titulo == "Modelos que faltan por ejercicio")
        .and_then(|t| t.filas.iter().find(|f| f[0] == "2024" && f[1] == "303"))
        .unwrap();
    assert!(fila[4].contains("antes del alta"), "{fila:?}");
}

#[test]
fn un_modelo_en_la_carpeta_sin_leer_no_se_pide() {
    let v = vault(&solo_sociedades());
    let r = revisar_con(
        &v,
        vec![
            con(
                "MODELOS/200_2023.pdf",
                modelo(
                    "200",
                    2023,
                    "0A",
                    json!({"599": "5000.00", "611": "5000.00"}),
                ),
            ),
            // El justificante del 1P de 2025, guardado por TaxPain, que no se puede leer.
            (
                "MODELOS/Modelo_202_2025_1P_justificante_2020000000001.pdf".into(),
                pdf("Escaneado"),
                None,
            ),
        ],
    );
    assert!(
        faltan(&r, 2025).is_none_or(|f| !f.detalle.contains("202 (1P)")),
        "{:#?}",
        titulos(&r)
    );
    let h = r
        .hallazgos
        .iter()
        .find(|h| h.codigo == "modelos.sin_leer" && h.ejercicio == Some(2025))
        .unwrap_or_else(|| panic!("{:#?}", titulos(&r)));
    assert!(h.detalle.contains("202 (1P)"), "{}", h.detalle);
    assert!(
        h.ficheros
            .iter()
            .any(|f| f.ends_with("Modelo_202_2025_1P_justificante_2020000000001.pdf"))
    );

    // Con otros nombres («303 2T 2025 …»), lo mismo; un borrador no es el modelo presentado.
    let v = vault(&TaxpayerProfile {
        retiene_trabajo_profesionales: false,
        retiene_alquileres: false,
        pagos_fraccionados_is: false,
        ..TaxpayerProfile::sociedad_limitada()
    });
    let r = revisar_con(
        &v,
        vec![
            con(
                "MODELOS/303_1T.pdf",
                modelo("303", 2025, "1T", json!({"71": "100.00"})),
            ),
            con(
                "MODELOS/303_3T.pdf",
                modelo("303", 2025, "3T", json!({"71": "100.00"})),
            ),
            (
                "MODELOS/copias/303 2T 2025 EMPRESA DEMO.pdf".into(),
                pdf("Escaneado"),
                None,
            ),
            (
                "MODELOS/copias/Modelo_303_4T2025_DEMO_borrador.pdf".into(),
                pdf("Borrador"),
                None,
            ),
        ],
    );
    let f = faltan(&r, 2025).unwrap_or_else(|| panic!("{:#?}", titulos(&r)));
    assert!(f.detalle.contains("303 (4T)"), "{}", f.detalle);
    let h = r
        .hallazgos
        .iter()
        .find(|h| h.codigo == "modelos.sin_leer" && h.ejercicio == Some(2025))
        .unwrap_or_else(|| panic!("{:#?}", titulos(&r)));
    assert!(h.detalle.contains("303 (2T)"), "{}", h.detalle);
}

/// Un certificado de situación censal inventado, con su alta en el IVA y sus obligaciones.
fn censal(fecha: &str, alta: Option<&str>, obligaciones: &[&str]) -> Vec<u8> {
    let mut l = vec![
        "CERTIFICADO DE SITUACIÓN CENSAL".to_owned(),
        "Nº DE REFERENCIA: 20250000004".to_owned(),
        format!("NIF: {PROPIA}"),
        "SITUACIÓN TRIBUTARIA".to_owned(),
    ];
    if let Some(a) = alta {
        l.push("IMPUESTO SOBRE EL VALOR AÑADIDO".to_owned());
        l.push("Regímenes aplicables Fecha".to_owned());
        l.push(format!("- General {a}"));
    }
    l.push("IMPUESTO SOBRE SOCIEDADES".to_owned());
    l.push("OBLIGACIONES PERIÓDICAS".to_owned());
    l.extend(obligaciones.iter().map(|o| (*o).to_owned()));
    l.push("Y para que conste, se expide la presente certificación.".to_owned());
    l.push(format!(
        "Documento firmado electrónicamente (Ley 40/2015) por la Agencia Estatal de \
         Administración Tributaria, con fecha {fecha}. Autenticidad verificable mediante Código \
         Seguro Verificación CENS1234DEMO7777 en sede.agenciatributaria.gob.es"
    ));
    text_document("Documento", &[l.into_iter().map(Para::Text).collect()]).unwrap()
}

#[test]
fn el_iva_fuera_de_las_fechas_del_censo() {
    let perfil = TaxpayerProfile {
        retiene_trabajo_profesionales: false,
        retiene_alquileres: false,
        pagos_fraccionados_is: false,
        ..TaxpayerProfile::sociedad_limitada()
    };
    // El alta en el IVA es del 1 de octubre de 2024, pero hay un 303 del 1T 2024: estuvo dada de
    // alta antes. Del 2T y el 3T no se sabe (no se piden); el 4T y el 390, sí.
    let v = vault(&perfil);
    let r = revisar_con(
        &v,
        vec![
            con(
                "MODELOS/303_1T_2024.pdf",
                modelo("303", 2024, "1T", json!({"71": "100.00"})),
            ),
            (
                "SEDE/censal.pdf".into(),
                censal(
                    "10 de enero de 2025",
                    Some("01/10/2024"),
                    &["IMPUESTO SOBRE EL VALOR AÑADIDO TRIMESTRAL"],
                ),
                None,
            ),
        ],
    );
    let f = faltan(&r, 2024).unwrap_or_else(|| panic!("{:#?}", titulos(&r)));
    assert!(f.detalle.contains("303 (4T)"), "{}", f.detalle);
    assert!(f.detalle.contains("390"), "{}", f.detalle);
    let nota = r
        .hallazgos
        .iter()
        .find(|h| h.codigo == "modelos.no_se_sabe" && h.ejercicio == Some(2024))
        .unwrap_or_else(|| panic!("{:#?}", titulos(&r)));
    assert!(
        nota.detalle.contains("303 del 2T") && nota.detalle.contains("303 del 3T"),
        "{}",
        nota.detalle
    );

    // El certificado más reciente (30/06/2025) ya no tiene el IVA: del 3T y el 4T de 2025, que
    // empiezan después, no se piden; del 2T (el último presentado es el 1T) no se sabe.
    let v = vault(&perfil);
    let r = revisar_con(
        &v,
        vec![
            con(
                "MODELOS/303_1T_2025.pdf",
                modelo("303", 2025, "1T", json!({"71": "100.00"})),
            ),
            (
                "SEDE/censal-2024.pdf".into(),
                censal(
                    "10 de enero de 2024",
                    Some("01/01/2020"),
                    &["IMPUESTO SOBRE EL VALOR AÑADIDO TRIMESTRAL"],
                ),
                None,
            ),
            (
                "SEDE/censal-2025.pdf".into(),
                censal(
                    "30 de junio de 2025",
                    None,
                    &["IMPUESTO DE SOCIEDADES ANUAL"],
                ),
                None,
            ),
        ],
    );
    assert!(
        faltan(&r, 2025).is_none_or(|f| !f.detalle.contains("303")),
        "{:#?}",
        titulos(&r)
    );
    let nota = r
        .hallazgos
        .iter()
        .find(|h| h.codigo == "modelos.no_se_sabe" && h.ejercicio == Some(2025))
        .unwrap_or_else(|| panic!("{:#?}", titulos(&r)));
    assert!(nota.detalle.contains("303 del 2T"), "{}", nota.detalle);
    assert!(!nota.detalle.contains("3T"), "{}", nota.detalle);
    let fila = r
        .tablas
        .iter()
        .find(|t| t.titulo == "Modelos que faltan por ejercicio")
        .and_then(|t| t.filas.iter().find(|f| f[0] == "2025" && f[1] == "303"))
        .unwrap();
    assert!(fila[4].contains("4T: el certificado"), "{fila:?}");
}

/// Una SL con el IVA trimestral, sin retenciones ni pagos fraccionados.
fn solo_iva() -> TaxpayerProfile {
    TaxpayerProfile {
        retiene_trabajo_profesionales: false,
        retiene_alquileres: false,
        pagos_fraccionados_is: false,
        ..TaxpayerProfile::sociedad_limitada()
    }
}

fn fila_de<'a>(r: &'a Revision, y: &str, m: &str) -> Option<&'a Vec<String>> {
    r.tablas
        .iter()
        .find(|t| t.titulo == "Modelos que faltan por ejercicio")
        .and_then(|t| t.filas.iter().find(|f| f[0] == y && f[1] == m))
}

#[test]
fn los_modelos_de_los_que_no_contesto_la_aeat_se_piden_por_el_perfil() {
    let v = vault(&solo_iva());
    // La consulta del 303 falló: la AEAT sólo contestó del 390 (que no tiene) y del 200.
    let a = Answer {
        models: ["303", "390", "200"].map(String::from).to_vec(),
        list: vec![Presented {
            model: "200".into(),
            year: 2025,
            period: "0A".into(),
            csv: "CSV2000A00000000".into(),
            file_number: "2025200000001".into(),
            receipt: "2000000000011".into(),
            previous_receipt: None,
            presented_at: "2026-07-20 10:00:00".into(),
        }],
        errors: vec!["303: el servicio de la AEAT no respondió".into()],
        ..Answer::default()
    };
    presentado_aeat::record(&v, PROPIA, 2025, &a, datetime!(2026-10-20 9:00 UTC)).unwrap();
    let r = revisar_con(
        &v,
        vec![con(
            "MODELOS/303_1T.pdf",
            modelo("303", 2025, "1T", json!({"71": "100.00"})),
        )],
    );
    let f = faltan(&r, 2025).unwrap_or_else(|| panic!("{:#?}", titulos(&r)));
    // El 390 lo dice la AEAT; los 303 del 2T al 4T, el perfil (la AEAT no contestó del 303).
    assert!(
        f.detalle.contains("La AEAT no tiene de 2025: 390"),
        "{}",
        f.detalle
    );
    assert!(f.detalle.contains("303 (2T, 3T, 4T)"), "{}", f.detalle);
    assert!(f.detalle.contains("no contestó la AEAT"), "{}", f.detalle);
    assert!(f.titulo.contains("Faltan 4 modelos"), "{}", f.titulo);
    assert!(
        r.pedir
            .iter()
            .any(|p| p.que.contains("2025") && p.que.contains("303 (2T, 3T, 4T)")),
        "{:?}",
        r.pedir
    );
}

#[test]
fn lo_que_vencio_despues_de_comprobar_no_lo_dice_la_aeat() {
    let v = vault(&solo_iva());
    // Comprobado el 01/12/2025: la AEAT tiene los 303 del 1T al 3T. El 4T y el 390 vencían en
    // enero de 2026, y el 200 en julio de 2026.
    let a = Answer {
        models: ["303", "390", "200"].map(String::from).to_vec(),
        list: vec![
            presentada("303", "1T", 1),
            presentada("303", "2T", 2),
            presentada("303", "3T", 3),
        ],
        ..Answer::default()
    };
    presentado_aeat::record(&v, PROPIA, 2025, &a, datetime!(2025-12-01 9:00 UTC)).unwrap();
    let r = revisar_con(&v, vec![]);
    let f = faltan(&r, 2025).unwrap_or_else(|| panic!("{:#?}", titulos(&r)));
    assert!(!f.detalle.contains("La AEAT no tiene"), "{}", f.detalle);
    assert!(
        f.detalle
            .contains("Según el perfil fiscal tendrían que estar 200; 303 (4T); 390,"),
        "{}",
        f.detalle
    );
    assert!(
        f.detalle
            .contains("después de «Comprobar lo presentado» del 01/12/2025")
    );
    // En «Comprobar lo presentado», no «Falta»: no consta, y hay que volver a comprobar.
    let rep = presentado_aeat::report(&v, 2025, date!(2026 - 10 - 31)).unwrap();
    let q4 = rep
        .rows
        .iter()
        .find(|x| x.model == "303" && x.period == "4T")
        .unwrap();
    assert_eq!(q4.status, presentado_aeat::Status::NotFound, "{q4:?}");
    assert!(
        q4.note.as_deref().unwrap().contains("vuelve a comprobar"),
        "{q4:?}"
    );
    assert_eq!(rep.missing(), 0, "{:?}", rep.rows);
}

#[test]
fn un_certificado_sin_el_iva_con_303_posteriores_no_esta_al_dia() {
    let v = vault(&solo_iva());
    let r = revisar_con(
        &v,
        vec![
            con(
                "MODELOS/303_1T_2024.pdf",
                modelo("303", 2024, "1T", json!({"71": "100.00"})),
            ),
            con(
                "MODELOS/303_2T_2024.pdf",
                modelo("303", 2024, "2T", json!({"71": "100.00"})),
            ),
            con(
                "MODELOS/303_4T_2024.pdf",
                modelo("303", 2024, "4T", json!({"71": "100.00"})),
            ),
            (
                "SEDE/censal.pdf".into(),
                censal(
                    "10 de enero de 2024",
                    None,
                    &["IMPUESTO DE SOCIEDADES ANUAL"],
                ),
                None,
            ),
        ],
    );
    // Hay 303 presentados después del certificado: el censo cambió y el 3T toca.
    let f = faltan(&r, 2024).unwrap_or_else(|| panic!("{:#?}", titulos(&r)));
    assert!(f.detalle.contains("303 (3T)"), "{}", f.detalle);
    let fila = fila_de(&r, "2024", "303").unwrap();
    assert!(!fila[4].contains("certificado"), "{fila:?}");
}

#[test]
fn un_calculo_de_la_gestoria_no_es_el_modelo_presentado() {
    let v = vault(&solo_iva());
    let r = revisar_con(
        &v,
        vec![
            con(
                "MODELOS/303_1T.pdf",
                modelo("303", 2025, "1T", json!({"71": "100.00"})),
            ),
            // Con texto y sin las marcas de la AEAT: no es el justificante del 2T.
            (
                "MODELOS/Modelo 303 2T 2025 EMPRESA DEMO.pdf".into(),
                pdf(
                    "Liquidación trimestral del IVA (preparada por la gestoría, sin presentar). \
                     Base 1.000,00. Cuota 210,00. Resultado 147,00.",
                ),
                None,
            ),
            // Y por el nombre, tampoco el cálculo.
            (
                "MODELOS/Calculo IVA modelo 303 3T 2025.pdf".into(),
                pdf("Escaneado"),
                None,
            ),
        ],
    );
    let f = faltan(&r, 2025).unwrap_or_else(|| panic!("{:#?}", titulos(&r)));
    assert!(f.detalle.contains("303 (2T, 3T, 4T)"), "{}", f.detalle);
    assert!(
        !r.hallazgos
            .iter()
            .any(|h| h.codigo == "modelos.sin_leer" && h.ejercicio == Some(2025)),
        "{:#?}",
        titulos(&r)
    );
}
