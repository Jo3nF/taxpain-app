//! Libros registro del gestor → libros de facturas → modelos, sin asientos ni VERI*FACTU.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::invoicing;
use taxpain_app::libros::{self, BookKind};
use taxpain_app::models;
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::datetime;

fn setup() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("B98765431".into()),
            name: "Ejemplo Servicios SL".into(),
            address: addr(),
            person: None,
        },
        default_series: "A".into(),
        rectifying_series: "R".into(),
        simplified_series: "S".into(),
        simplified_rectifying_series: "RS".into(),
        language: Language::Es,
        default_vat: Rate::percent(21),
        default_withholding: None,
        outputs: Outputs {
            pdf: false,
            facturae: false,
            ubl: false,
        },
        destination: None,
        rounding: RoundingMode::PerRate,
        verifactu: VerifactuSettings {
            enabled: false,
            installation_id: "x".into(),
            production: false,
        },
        footer: None,
        activity: None,
        simplified_up_to_3000: false,
    };
    invoicing::save_settings(&v, &s, datetime!(2025-01-01 9:00 UTC)).unwrap();
    v
}

const EMITIDAS: &str = "Fecha Expedición;Número Factura;NIF Destinatario;Nombre Destinatario;Base Imponible;Tipo IVA;Cuota IVA;Total\n\
15/01/2025;2025-001;B12345674;Cliente SL;1.000,00;21;210,00;1.210,00\n\
20/02/2025;2025-002;FR12345678901;Client SARL;500,00;0;0,00;500,00\n";

const RECIBIDAS: &str = "Fecha;Nº Factura;NIF Proveedor;Proveedor;Base;Cuota soportada;Retención;Total\n\
01/03/2025;F-77;12345678Z;Asesoría Ejemplo;300,00;63,00;45,00;318,00\n";

#[test]
fn los_libros_del_gestor_alimentan_el_303_sin_asientos() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let issued = libros::parse("libro_emitidas.csv", EMITIDAS.as_bytes())
        .unwrap()
        .unwrap();
    assert_eq!(issued.kind, BookKind::Emitidas);
    let r = libros::import_issued(&v, &issued, now).unwrap();
    assert_eq!((r.added, r.skipped), (2, 0));
    assert!(
        r.warnings.iter().any(|w| w.contains("no sujeta")),
        "{:?}",
        r.warnings
    );
    // Otra vez: no se duplican.
    assert_eq!(libros::import_issued(&v, &issued, now).unwrap().skipped, 2);

    let received = libros::parse("recibidas.csv", RECIBIDAS.as_bytes())
        .unwrap()
        .unwrap();
    let r =
        libros::import_received(&v, &received, "recibidas.csv", RECIBIDAS.as_bytes(), now).unwrap();
    assert_eq!(r.added, 1);

    let d = models::draft(&v, "303", 2025, 1).unwrap();
    let get = |b: u16| {
        d.boxes
            .iter()
            .find(|(k, _)| *k == b)
            .map_or(Money::ZERO, |(_, v)| *v)
    };
    assert_eq!(get(7), Money::from_euros(1000, 0), "{:?}", d.boxes);
    assert_eq!(get(9), Money::from_euros(210, 0));
    assert_eq!(get(29), Money::from_euros(63, 0));
    // Servicio a un empresario de la UE con NIF-IVA: no sujeto por reglas de localización, en
    // [59] (se cruza con la clave S del 349), no en [120].
    assert_eq!(get(59), Money::from_euros(500, 0));
    assert_eq!(get(120), Money::ZERO);

    // La retención del profesional va al 111.
    let m111 = models::draft(&v, "111", 2025, 1).unwrap();
    assert!(
        m111.boxes
            .iter()
            .any(|(_, v)| *v == Money::from_euros(45, 0)),
        "{:?}",
        m111.boxes
    );

    // Una importada no se anula desde TaxPain (no se expidió aquí).
    let id: i64 = v
        .connection()
        .query_row("SELECT id FROM invoice_issued LIMIT 1", [], |r| r.get(0))
        .unwrap();
    let err = invoicing::cancel(&v, id, now, true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("libro de la gestoría"), "{err}");
    let listed = invoicing::list_issued(&v, 2025).unwrap();
    assert!(listed.iter().all(|i| i.origin == "importada"));

    // Ni asientos ni registros VERI*FACTU.
    let entries: i64 = v
        .connection()
        .query_row("SELECT COUNT(*) FROM journal_entry", [], |r| r.get(0))
        .unwrap();
    assert_eq!(entries, 0);
}

#[test]
fn el_fichero_del_gestor_se_lee_y_se_compara() {
    use taxpain_app::presentados;
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let issued = libros::parse("libro_emitidas.csv", EMITIDAS.as_bytes())
        .unwrap()
        .unwrap();
    libros::import_issued(&v, &issued, now).unwrap();
    let received = libros::parse("recibidas.csv", RECIBIDAS.as_bytes())
        .unwrap()
        .unwrap();
    libros::import_received(&v, &received, "recibidas.csv", RECIBIDAS.as_bytes(), now).unwrap();

    // El 303 que habría presentado el gestor: las mismas casillas que calcula TaxPain, en un
    // fichero con el diseño de registro de 2025 (TaxPain genera con el vigente, el de 2026).
    for (model, q) in [("303", 1u8), ("111", 1)] {
        let d = models::draft(&v, model, 2025, q).unwrap();
        let file =
            taxpain_models::dr_versions::file_with_boxes(model, 2025, "1T", &d.boxes).unwrap();
        let r = presentados::read_file(&file).unwrap_or_else(|| panic!("{model} no leído"));
        assert_eq!(
            (r.model.as_str(), r.year, r.period.as_str()),
            (model, 2025, "1T")
        );
        // Importes (la [65] del 303 es un porcentaje).
        let nonzero: Vec<(u16, Money)> = d
            .boxes
            .iter()
            .copied()
            .filter(|(b, m)| !m.is_zero() && *b != 65)
            .collect();
        let read: Vec<(u16, Money)> = r.boxes.iter().map(|(k, v)| (*k, *v)).collect();
        assert_eq!(read, nonzero, "{model}");
        let id = presentados::store(&v, "gestor.txt", file.as_bytes(), &r, now).unwrap();
        let c = presentados::compare(&v, id).unwrap();
        assert_eq!(c.mismatches, 0, "{model}: {:?}", c.rows);
    }

    // Lo tecleado a mano con una diferencia en la [27].
    let mut boxes = std::collections::BTreeMap::new();
    boxes.insert(27u16, Money::from_euros(250, 0));
    let id = presentados::save_manual(&v, "303", 2025, "1T", &boxes, now).unwrap();
    let c = presentados::compare(&v, id).unwrap();
    let r27 = c.rows.iter().find(|r| r.box_ == 27).unwrap();
    assert_eq!(r27.diff, Money::from_euros(-40, 0));
    assert!(c.mismatches >= 1);
}

#[test]
fn la_carpeta_de_la_gestoria_se_clasifica_y_se_importa() {
    use taxpain_app::carpeta::{self, Found};
    use taxpain_app::presentados;

    // El 303 «del gestor»: el que sale de los mismos libros en otro almacén.
    let other = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let b = libros::parse("emitidas.csv", EMITIDAS.as_bytes())
        .unwrap()
        .unwrap();
    libros::import_issued(&other, &b, now).unwrap();
    let b = libros::parse("recibidas.csv", RECIBIDAS.as_bytes())
        .unwrap()
        .unwrap();
    libros::import_received(&other, &b, "r.csv", RECIBIDAS.as_bytes(), now).unwrap();
    let d = models::draft(&other, "303", 2025, 1).unwrap();
    let m303 = taxpain_models::dr_versions::file_with_boxes("303", 2025, "1T", &d.boxes).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("2025/modelos")).unwrap();
    std::fs::write(root.join("2025/Libro facturas emitidas.csv"), EMITIDAS).unwrap();
    std::fs::write(root.join("2025/Libro facturas recibidas.csv"), RECIBIDAS).unwrap();
    std::fs::write(root.join("2025/modelos/303_1T.txt"), &m303).unwrap();
    std::fs::write(
        root.join("2025/diario.csv"),
        "Fecha;Asiento;Cuenta;Concepto;Debe;Haber\n\
         02/01/2025;1;572;Aportación;3.000,00;\n\
         02/01/2025;1;100;Aportación;;3.000,00\n",
    )
    .unwrap();
    std::fs::write(root.join("notas.txt"), "llamar al gestor").unwrap();
    std::fs::write(root.join(".DS_Store"), "x").unwrap();
    std::fs::write(root.join("logo.png"), "logo").unwrap();
    std::fs::write(root.join("foto tarjeta NIF.jpg"), "foto").unwrap();
    std::fs::write(
        root.join("escritura.txt"),
        "ESCRITURA DE CONSTITUCIÓN DE SOCIEDAD DE RESPONSABILIDAD LIMITADA. Ante mí, Don Ramón \
         Pérez Notario, Notario de Valencia. Que constituye una sociedad denominada EJEMPLO \
         SERVICIOS, S.L., con domicilio social en Calle de la Paz 15, 46003 Valencia. CNAE \
         6201. N.I.F.: B98765431.",
    )
    .unwrap();

    let v = setup();
    let inv = carpeta::scan(&v, root).unwrap();
    let kind = |p: &str| {
        inv.items
            .iter()
            .find(|i| i.path.ends_with(p))
            .map(|i| i.found.clone())
            .unwrap_or_else(|| panic!("{p} no está: {:?}", inv.items))
    };
    assert!(matches!(
        kind("emitidas.csv"),
        Found::Libro { invoices: 2, .. }
    ));
    assert!(matches!(
        kind("recibidas.csv"),
        Found::Libro { invoices: 1, .. }
    ));
    assert!(matches!(
        kind("diario.csv"),
        Found::Diario { entries: 1, .. }
    ));
    assert!(
        matches!(kind("303_1T.txt"), Found::Modelo { ref model, year: 2025, .. } if model == "303")
    );
    assert!(matches!(kind("notas.txt"), Found::Otro { .. }));
    assert!(matches!(kind("logo.png"), Found::Otro { .. }));
    assert!(matches!(
        kind("tarjeta NIF.jpg"),
        Found::Documento { by_name: true, .. }
    ));
    assert!(
        matches!(kind("escritura.txt"), Found::Documento { ref facts, .. } if facts.cnae.as_deref() == Some("6201"))
    );
    assert_eq!(inv.skipped, 0); // .DS_Store es oculto: ni se cuenta

    let paths: Vec<String> = inv
        .items
        .iter()
        .filter(|i| i.importable)
        .map(|i| i.path.clone())
        .collect();
    assert_eq!(paths.len(), 6);
    let out = carpeta::apply(&v, root, &paths, now).unwrap();
    assert!(out.iter().all(|o| o.ok), "{out:?}");

    // El 303 de TaxPain cuadra con el del gestor.
    let filed = presentados::list(&v, Some(2025)).unwrap();
    assert_eq!(filed.len(), 1);
    let c = presentados::compare(&v, filed[0].id).unwrap();
    assert_eq!(c.mismatches, 0, "{:?}", c.rows);

    // Los documentos de la empresa proponen datos (el CNAE; la razón social ya coincide
    // salvo la forma) y se aplican con conformidad.
    use taxpain_app::documentos;
    assert_eq!(documentos::list(&v).unwrap().len(), 2);
    let sug = documentos::suggestions(&v).unwrap();
    assert!(
        sug.iter()
            .any(|s| s.field == "cnae" && s.proposed == "6201"),
        "{sug:?}"
    );
    documentos::apply_suggestion(&v, "cnae", now).unwrap();
    assert_eq!(taxpain_app::models::settings(&v).unwrap().cnae, "6201");
    assert!(
        !documentos::suggestions(&v)
            .unwrap()
            .iter()
            .any(|s| s.field == "cnae")
    );

    // Volver a aplicar no duplica nada.
    let again = carpeta::apply(&v, root, &paths, now).unwrap();
    assert!(again.iter().all(|o| o.ok), "{again:?}");
    let issued: i64 = v
        .connection()
        .query_row("SELECT COUNT(*) FROM invoice_issued", [], |r| r.get(0))
        .unwrap();
    assert_eq!(issued, 2);
}

#[test]
fn con_full_ia_una_hoja_sin_titulos_se_importa_con_las_columnas_del_asistente() {
    use taxpain_app::carpeta::{self, Found};
    use taxpain_app::lectura_ia;

    let v = setup();
    let now = datetime!(2026-10-26 10:00 UTC);
    let dir = tempfile::tempdir().unwrap();
    let csv = "F-1;15/01/2025;B12345674;Cliente SL;1000,00;21;210,00;1210,00\n\
               F-2;20/02/2025;B12345674;Cliente SL;100,00;21;21,00;121,00\n";
    std::fs::write(dir.path().join("ventas.csv"), csv).unwrap();

    // Sin el asistente, las reglas no saben qué es.
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    assert!(
        matches!(inv.items[0].found, Found::Otro { .. }),
        "{:?}",
        inv.items[0]
    );

    // Lo que habría dicho el asistente.
    let r = lectura_ia::parse_reply(
        r#"{"tipo":"libro_emitidas","descripcion":"Ventas 2025","hoja":{"fila_titulos":0,"primera_fila":1,"columnas":{"numero":"A","fecha":"B","nif":"C","nombre":"D","base":"E","tipo_iva":"F","cuota_iva":"G","total":"H"}}}"#,
    )
    .unwrap();
    lectura_ia::store(&v, csv.as_bytes(), &r, true, now).unwrap();

    let inv = carpeta::scan(&v, dir.path()).unwrap();
    match &inv.items[0].found {
        Found::Ia {
            titulo,
            detalle,
            importable,
            year,
            ..
        } => {
            assert_eq!(titulo, "Libro de facturas emitidas");
            assert_eq!(detalle, "2 facturas");
            assert!(*importable);
            assert_eq!(*year, Some(2025));
        }
        f => panic!("{f:?}"),
    }
    let out = carpeta::apply(&v, dir.path(), &[inv.items[0].path.clone()], now).unwrap();
    assert!(out[0].ok, "{out:?}");
    let d = models::draft(&v, "303", 2025, 1).unwrap();
    let base7 = d.boxes.iter().find(|(b, _)| *b == 7).map(|(_, m)| *m);
    assert_eq!(base7, Some(Money::from_euros(1100, 0)));
}

#[test]
fn sin_full_ia_las_fotos_se_leen_con_el_reconocimiento_de_texto_del_mac() {
    use taxpain_app::carpeta::{self, Found};
    // Lector simulado (en el Mac es Vision).
    carpeta::set_ocr(Box::new(|name: &str, _bytes: &[u8]| {
        if name.starts_with("IMG_tarjeta") {
            Some(
                "AGENCIA TRIBUTARIA. Tarjeta de identificación fiscal. NÚMERO DE IDENTIFICACIÓN \
                 FISCAL: B98765431. Denominación: EJEMPLO SERVICIOS SL"
                    .to_owned(),
            )
        } else if name.starts_with("ticket papeleria") {
            Some(
                "Papelería Ejemplo SA\nNIF A22222228\nFactura simplificada nº T-123\nFecha 03/02/2025\n\
                 Base imponible 10,00\nIVA 21% 2,10\nTotal 12,10 €"
                    .to_owned(),
            )
        } else {
            None
        }
    }));
    let v = setup();
    let now = datetime!(2026-10-26 10:00 UTC);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("IMG_tarjeta.jpg"), [0xFF, 0xD8, 1]).unwrap();
    std::fs::write(dir.path().join("ticket papeleria.png"), [0x89, b'P', 2]).unwrap();
    std::fs::write(dir.path().join("IMG_0001.jpg"), [0xFF, 0xD8, 3]).unwrap();
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    let found = |p: &str| {
        inv.items
            .iter()
            .find(|i| i.path.contains(p))
            .unwrap()
            .found
            .clone()
    };
    assert!(
        matches!(found("tarjeta"), Found::Documento { ref facts, by_name: false, .. } if facts.nif.as_deref() == Some("B98765431")),
        "{:?}",
        found("tarjeta")
    );
    assert!(
        matches!(found("ticket"), Found::FacturaRecibida { ref number, .. } if number.as_deref() == Some("T-123")),
        "{:?}",
        found("ticket")
    );
    assert!(matches!(found("IMG_0001"), Found::Otro { .. }));

    let paths: Vec<String> = inv
        .items
        .iter()
        .filter(|i| i.importable)
        .map(|i| i.path.clone())
        .collect();
    let out = carpeta::apply(&v, dir.path(), &paths, now).unwrap();
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    let pending = taxpain_app::received::list_received(&v, Some("pendiente")).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].total, Some(Money::from_cents(1210)));
}

#[test]
fn las_lecturas_de_claude_code_se_cargan_sin_api() {
    use taxpain_app::carpeta::{self, Found};
    let v = setup();
    let now = datetime!(2026-10-26 10:00 UTC);
    let dir = tempfile::tempdir().unwrap();
    let csv = "F-1;15/01/2025;B12345674;Cliente SL;1000,00;21;210,00;1210,00\n";
    std::fs::create_dir_all(dir.path().join("2025")).unwrap();
    std::fs::write(dir.path().join("2025/ventas raras.csv"), csv).unwrap();
    std::fs::write(
        dir.path().join("taxpain-lecturas.json"),
        r#"{"version":1,"generado_por":"Claude Code","lecturas":[
            {"fichero":"2025/ventas raras.csv","lectura":{"tipo":"libro_emitidas","descripcion":"Ventas 2025",
             "hoja":{"fila_titulos":0,"primera_fila":1,"columnas":{"numero":"A","fecha":"B","nif":"C","nombre":"D","base":"E","tipo_iva":"F","cuota_iva":"G","total":"H"}}}},
            {"fichero":"2025/no existe.pdf","lectura":{"tipo":"factura_recibida"}}
        ]}"#,
    )
    .unwrap();
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    let cc = inv.claude_code.clone().unwrap();
    assert_eq!(cc.loaded, 1);
    assert_eq!(cc.missing, vec!["2025/no existe.pdf".to_string()]);
    // El JSON no sale como fichero de la carpeta.
    assert_eq!(inv.items.len(), 1);
    assert!(matches!(&inv.items[0].found, Found::Ia { detalle, .. } if detalle == "1 facturas"));
    let out = carpeta::apply(&v, dir.path(), &[inv.items[0].path.clone()], now).unwrap();
    assert!(out[0].ok, "{out:?}");
}

/// PDF mínimo cuya página no tiene `MediaBox`: pdf-extract 0.12 entra en pánico al leerlo.
fn panicking_pdf() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R >>",
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, o) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj {o} endobj\n", i + 1).bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
    for o in offsets {
        out.extend(format!("{o:010} 00000 n \n").bytes());
    }
    out.extend(
        format!(
            "trailer << /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .bytes(),
    );
    out
}

#[test]
fn un_pdf_que_tumba_al_lector_no_para_la_lectura_de_la_carpeta() {
    use taxpain_app::carpeta::{self, Found};
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("factura rota.pdf"), panicking_pdf()).unwrap();
    std::fs::write(dir.path().join("Libro facturas emitidas.csv"), EMITIDAS).unwrap();
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    assert_eq!(inv.items.len(), 2, "{:?}", inv.items);
    let rota = inv.items.iter().find(|i| i.path.ends_with(".pdf")).unwrap();
    assert!(matches!(rota.found, Found::Otro { .. }), "{rota:?}");
    // Y se puede seguir usando el almacén.
    let paths: Vec<String> = inv.items.iter().map(|i| i.path.clone()).collect();
    let out = carpeta::apply(&v, dir.path(), &paths, now).unwrap();
    assert!(out.iter().all(|o| o.ok), "{out:?}");
}

#[test]
fn una_lectura_mal_escrita_no_impide_cargar_las_demas() {
    use taxpain_app::carpeta::{self, Found};
    let v = setup();
    let dir = tempfile::tempdir().unwrap();
    let csv = "F-1;15/01/2025;B12345674;Cliente SL;1000,00;21;210,00;1210,00\n";
    std::fs::write(dir.path().join("ventas.csv"), csv).unwrap();
    std::fs::write(
        dir.path().join("escritura.pdf"),
        b"%PDF-1.4 no es un pdf de verdad",
    )
    .unwrap();
    // Lo que escribe Claude Code siguiendo .claude/commands/leer-carpeta.md: `null` en lo que
    // no aparece (también dentro de «hoja» y de «columnas»), números de fila como texto…
    std::fs::write(
        dir.path().join("taxpain-lecturas.json"),
        r#"{"version":1,"generado_por":"Claude Code","lecturas":[
            {"fichero":"ventas.csv","lectura":{"tipo":"libro_emitidas","descripcion":null,
             "hoja":{"fila_titulos":null,"primera_fila":"1","columnas":{"numero":"A","fecha":"B","nif":"C","nombre":"D","base":"E","tipo_iva":"F","cuota_iva":"G","total":"H","serie":null,"pais":null}},
             "factura":null,"modelo":null,"empresa":null}},
            {"fichero":"escritura.pdf","lectura":{"tipo":"escritura_constitucion","descripcion":"Constitución",
             "empresa":{"nif":"B98765431","name":null,"capital":null}}},
            {"fichero":"otro.pdf","lectura":{"tipo":null}},
            {"fichero":null,"lectura":{"tipo":"otro"}},
            "esto no es una lectura",
            {"fichero":"x.pdf"}
        ]}"#,
    )
    .unwrap();
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    let cc = inv.claude_code.clone().unwrap();
    assert_eq!((cc.loaded, cc.invalid), (2, 4), "{cc:?}");
    assert!(cc.error.is_none());
    let ventas = inv.items.iter().find(|i| i.path == "ventas.csv").unwrap();
    assert!(
        matches!(&ventas.found, Found::Ia { detalle, .. } if detalle == "1 facturas"),
        "{ventas:?}"
    );

    // Un JSON a medio escribir no para la lectura de la carpeta: se avisa y ya.
    std::fs::write(
        dir.path().join("taxpain-lecturas.json"),
        r#"{"lecturas":[{"fichero":"ventas.csv","lectura":{"tipo":"#,
    )
    .unwrap();
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    let cc = inv.claude_code.clone().unwrap();
    assert!(cc.error.is_some(), "{cc:?}");
    assert_eq!(inv.items.len(), 2);
}

#[test]
fn las_lecturas_de_claude_code_no_pisan_una_lectura_mas_nueva_del_asistente() {
    use taxpain_app::carpeta;
    use taxpain_app::lectura_ia;
    let v = setup();
    let dir = tempfile::tempdir().unwrap();
    let csv = "F-1;15/01/2025;B12345674;Cliente SL;1000,00;21;210,00;1210,00\n";
    std::fs::write(dir.path().join("ventas.csv"), csv).unwrap();
    let pkg = dir.path().join("taxpain-lecturas.json");
    std::fs::write(
        &pkg,
        r#"{"lecturas":[{"fichero":"ventas.csv","lectura":{"tipo":"otro","descripcion":"del paquete"}}]}"#,
    )
    .unwrap();
    let set_mtime = |t: time::OffsetDateTime| {
        std::fs::File::options()
            .write(true)
            .open(&pkg)
            .unwrap()
            .set_modified(t.into())
            .unwrap();
    };
    set_mtime(datetime!(2026-10-20 10:00 UTC));
    // Después, el asistente de la app lee el mismo fichero (y lo entiende mejor).
    let better =
        lectura_ia::parse_reply(r#"{"tipo":"libro_emitidas","descripcion":"de la app"}"#).unwrap();
    lectura_ia::store(
        &v,
        csv.as_bytes(),
        &better,
        true,
        datetime!(2026-10-22 10:00 UTC),
    )
    .unwrap();
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    assert_eq!(inv.claude_code.as_ref().unwrap().kept, 1);
    let stored = lectura_ia::stored(&v, csv.as_bytes()).unwrap().unwrap();
    assert_eq!(stored.descripcion, "de la app");
    // Un paquete escrito más tarde sí manda.
    set_mtime(datetime!(2026-10-23 10:00 UTC));
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    assert_eq!(inv.claude_code.as_ref().unwrap().loaded, 1);
    let stored = lectura_ia::stored(&v, csv.as_bytes()).unwrap().unwrap();
    assert_eq!(stored.descripcion, "del paquete");
}

fn get(d: &models::Draft, b: u16) -> Money {
    d.boxes
        .iter()
        .find(|(k, _)| *k == b)
        .map_or(Money::ZERO, |(_, v)| *v)
}

fn sheet(t: &[&[&str]]) -> Vec<Vec<String>> {
    t.iter()
        .map(|r| r.iter().map(|c| (*c).to_owned()).collect())
        .collect()
}

#[test]
fn la_numeracion_que_empieza_cada_ano_no_se_pierde_al_importar() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let issued = |date: &str, base: &str, vat: &str| {
        sheet(&[
            &["Libro registro de facturas expedidas"],
            &[
                "Fecha Expedición",
                "Número Factura",
                "NIF Destinatario",
                "Nombre Destinatario",
                "Base Imponible",
                "Tipo de IVA",
                "Cuota IVA Repercutida",
            ],
            &[date, "1", "B12345674", "Cliente SL", base, "21", vat],
        ])
    };
    let b24 = libros::parse_rows("emitidas2024.xlsx", &issued("10/02/2024", "500", "105")).unwrap();
    let b25 = libros::parse_rows("emitidas2025.xlsx", &issued("10/08/2025", "800", "168")).unwrap();
    assert_eq!(libros::import_issued(&v, &b24, now).unwrap().added, 1);
    let r = libros::import_issued(&v, &b25, now).unwrap();
    assert_eq!(r.added, 1, "{r:?}");
    assert!(
        r.warnings.iter().any(|w| w.contains("1 (2025)")),
        "{:?}",
        r.warnings
    );
    // Otra vez: las dos ya están.
    assert_eq!(libros::import_issued(&v, &b24, now).unwrap().skipped, 1);
    assert_eq!(libros::import_issued(&v, &b25, now).unwrap().skipped, 1);
    let d = models::draft(&v, "303", 2025, 3).unwrap();
    assert_eq!(get(&d, 7), Money::from_euros(800, 0));
    // El 303 de 2025 1T no se come la «1» de 2024 (el libro del año anterior también se carga).
    let d = models::draft(&v, "303", 2024, 1).unwrap();
    assert_eq!(get(&d, 7), Money::from_euros(500, 0));

    // Recibidas: el mismo proveedor reinicia su numeración, y dos proveedores sin NIF con el
    // mismo número.
    let csv = "Fecha;Nº Factura;NIF Proveedor;Proveedor;Base;Cuota;Total\n\
               15/12/2024;7;12345678Z;Asesoría Ejemplo;100,00;21,00;121,00\n\
               15/01/2025;7;12345678Z;Asesoría Ejemplo;200,00;42,00;242,00\n\
               20/01/2025;1;;Bar Pepe;10,00;1,00;11,00\n\
               21/01/2025;1;;Taxi Juan;20,00;2,00;22,00\n";
    let b = libros::parse("recibidas.csv", csv.as_bytes())
        .unwrap()
        .unwrap();
    assert_eq!(b.invoices.len(), 4);
    let r = libros::import_received(&v, &b, "recibidas.csv", csv.as_bytes(), now).unwrap();
    assert_eq!((r.added, r.skipped), (4, 0), "{r:?}");
    let again = libros::import_received(&v, &b, "recibidas.csv", csv.as_bytes(), now).unwrap();
    assert_eq!((again.added, again.skipped), (0, 4), "{again:?}");
    let d = models::draft(&v, "303", 2025, 1).unwrap();
    assert_eq!(get(&d, 29), Money::from_euros(45, 0), "{:?}", d.boxes);
}

#[test]
fn las_compras_a_proveedores_extranjeros_del_libro_se_autorrepercuten() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    // Nube Demo Ltd: la cuota del libro es la autorrepercutida (el total no la incluye).
    // Un proveedor de EE. UU. igual (inversión del sujeto pasivo). Y uno de Alemania sin
    // columna de total que la aclare.
    let r = sheet(&[
        &["Libro registro de facturas recibidas"],
        &[
            "Fecha",
            "Nº Factura",
            "NIF Proveedor",
            "Proveedor",
            "Base",
            "Cuota",
            "Total",
        ],
        &[
            "2026-03-01",
            "IE-1",
            "IE1234567T",
            "Nube Demo Ltd",
            "100",
            "21",
            "100",
        ],
        &[
            "2026-03-02",
            "US-1",
            "US123456789",
            "Cloud Inc",
            "200",
            "42",
            "200",
        ],
    ]);
    let b = libros::parse_rows("recibidas.csv", &r).unwrap();
    assert!(b.warnings.is_empty(), "{:?}", b.warnings);
    libros::import_received(&v, &b, "recibidas.csv", b"x1", now).unwrap();
    let d = models::draft(&v, "303", 2026, 1).unwrap();
    // Adquisición intracomunitaria [10]/[11] y su deducción [36]/[37]; ISP [12]/[13] y [28]/[29].
    assert_eq!(get(&d, 10), Money::from_euros(100, 0), "{:?}", d.boxes);
    assert_eq!(get(&d, 11), Money::from_euros(21, 0));
    assert_eq!(get(&d, 36), Money::from_euros(100, 0));
    assert_eq!(get(&d, 37), Money::from_euros(21, 0));
    assert_eq!(get(&d, 12), Money::from_euros(200, 0));
    assert_eq!(get(&d, 13), Money::from_euros(42, 0));
    assert_eq!(get(&d, 29), Money::from_euros(42, 0));
    assert_eq!(d.result, Money::ZERO);
    let m349 = models::draft(&v, "349", 2026, 1).unwrap();
    assert_eq!(m349.result, Money::from_euros(100, 0), "{:?}", m349.details);
    assert!(m349.details[0].0.contains("clave I"), "{:?}", m349.details);
}

#[test]
fn el_recargo_de_equivalencia_de_las_emitidas_importadas_llega_al_303() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let r = sheet(&[
        &["Libro registro de facturas expedidas"],
        &[
            "Fecha Expedición",
            "Número Factura",
            "NIF Destinatario",
            "Nombre Destinatario",
            "Base Imponible",
            "Tipo de IVA",
            "Cuota IVA Repercutida",
            "Cuota Recargo",
            "Total Factura",
        ],
        &[
            "15/05/2026",
            "2026-RE-1",
            "B12345674",
            "Tienda SL",
            "1.000,00",
            "21",
            "210,00",
            "52,00",
            "1.262,00",
        ],
        &[
            "16/05/2026",
            "2026-RE-2",
            "B12345674",
            "Tienda SL",
            "100,00",
            "10",
            "10,00",
            "3,00",
            "113,00",
        ],
    ]);
    let b = libros::parse_rows("emitidas.xlsx", &r).unwrap();
    let out = libros::import_issued(&v, &b, now).unwrap();
    // El segundo recargo (3 %) no es de ningún tipo: aviso.
    assert!(
        out.warnings.iter().any(|w| w.contains("2026-RE-2")),
        "{:?}",
        out.warnings
    );
    // Y el 303 no se prepara hasta corregirlo: su cuota quedaría sin declarar.
    let e = models::draft(&v, "303", 2026, 2).unwrap_err().to_string();
    assert!(e.contains("2026-RE-2"), "{e}");

    // Sin la factura mala, el recargo del 5,2 % llega a [22]/[24].
    let v = setup();
    let r: Vec<Vec<String>> = r.into_iter().take(3).collect();
    let b = libros::parse_rows("emitidas.xlsx", &r).unwrap();
    libros::import_issued(&v, &b, now).unwrap();
    let d = models::draft(&v, "303", 2026, 2).unwrap();
    assert_eq!(get(&d, 22), Money::from_euros(1000, 0), "{:?}", d.boxes);
    assert_eq!(get(&d, 24), Money::from_euros(52, 0));
}

#[test]
fn un_cliente_de_fuera_de_la_ue_no_va_al_349() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let r = sheet(&[
        &["Libro registro de facturas expedidas"],
        &[
            "Fecha Expedición",
            "Número Factura",
            "NIF Destinatario",
            "País",
            "Nombre Destinatario",
            "Base Imponible",
            "Tipo de IVA",
            "Cuota IVA Repercutida",
        ],
        &[
            "10/11/2026",
            "US-1",
            "123456789",
            "US",
            "Acme Inc",
            "2000",
            "0",
            "0",
        ],
        &[
            "11/11/2026",
            "FR-1",
            "FR12345678901",
            "",
            "Client SARL",
            "300",
            "0",
            "0",
        ],
    ]);
    let b = libros::parse_rows("emitidas-ext.xlsx", &r).unwrap();
    libros::import_issued(&v, &b, now).unwrap();
    let listed = taxpain_app::invoicing::list_issued(&v, 2026).unwrap();
    assert_eq!(listed.len(), 2);
    let data: String = v
        .connection()
        .query_row(
            "SELECT data FROM invoice_issued WHERE number = 'US-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(data.contains("\"Otro\""), "{data}");
    assert!(!data.contains("NifIva"), "{data}");
    let data: String = v
        .connection()
        .query_row(
            "SELECT data FROM invoice_issued WHERE number = 'FR-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(data.contains("NifIva"), "{data}");
}

#[test]
fn los_clientes_y_proveedores_griegos_son_de_la_ue() {
    // Grecia: país «GR» (ISO) y NIF-IVA con «EL». Antes salía «GREL…», el cliente quedaba como
    // «otro documento» (fuera del 349, el servicio en [120]) y el proveedor, como interior/ISP.
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let issued = sheet(&[
        &["Libro registro de facturas expedidas"],
        &[
            "Fecha Expedición",
            "Número Factura",
            "NIF Destinatario",
            "País",
            "Nombre Destinatario",
            "Base Imponible",
            "Tipo de IVA",
            "Cuota IVA Repercutida",
        ],
        &[
            "10/11/2026",
            "GR-1",
            "EL123456789",
            "GR",
            "Hellas AE",
            "400",
            "0",
            "0",
        ],
        &[
            "12/11/2026",
            "GR-2",
            "987654321",
            "GR",
            "Athina IKE",
            "100",
            "0",
            "0",
        ],
        // Un país que no existe (ISO 3166-1): se ignora y se avisa, sin inventar un NIF-IVA.
        &[
            "13/11/2026",
            "ZZ-3",
            "12345678Z",
            "ZZ",
            "Cliente Nacional",
            "100",
            "21",
            "21",
        ],
    ]);
    let b = libros::parse_rows("emitidas-gr.xlsx", &issued).unwrap();
    assert_eq!(b.invoices[0].nif.as_deref(), Some("EL123456789"));
    assert_eq!(b.invoices[1].nif.as_deref(), Some("EL987654321"));
    assert_eq!(b.invoices[2].nif.as_deref(), Some("12345678Z"));
    assert_eq!(b.invoices[2].country, None);
    assert!(
        b.warnings
            .iter()
            .any(|w| w.contains("ZZ-3") && w.contains("ISO 3166-1")),
        "{:?}",
        b.warnings
    );
    libros::import_issued(&v, &b, now).unwrap();
    let data: String = v
        .connection()
        .query_row(
            "SELECT data FROM invoice_issued WHERE number = 'GR-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(data.contains("NifIva"), "{data}");
    assert!(!data.contains("GREL"), "{data}");
    // Servicios a operadores de la UE: [59] del 303 y clave S del 349.
    let d = models::draft(&v, "303", 2026, 4).unwrap();
    assert_eq!(get(&d, 59), Money::from_euros(500, 0), "{:?}", d.boxes);
    assert_eq!(get(&d, 120), Money::ZERO);
    let m349 = models::draft(&v, "349", 2026, 4).unwrap();
    assert_eq!(m349.result, Money::from_euros(500, 0), "{:?}", m349.details);

    // Proveedor griego sin columna de país (sólo el prefijo «EL»): adquisición intracomunitaria.
    let received = sheet(&[
        &["Libro registro de facturas recibidas"],
        &[
            "Fecha",
            "Nº Factura",
            "NIF Proveedor",
            "Proveedor",
            "Base",
            "Cuota",
            "Total",
        ],
        &[
            "2026-11-03",
            "EL-7",
            "EL094259216",
            "Proveedor Hellas",
            "200",
            "42",
            "200",
        ],
    ]);
    let b = libros::parse_rows("recibidas-gr.csv", &received).unwrap();
    assert_eq!(b.invoices[0].country.as_deref(), Some("GR"));
    libros::import_received(&v, &b, "recibidas-gr.csv", b"gr", now).unwrap();
    let regime: String = v
        .connection()
        .query_row(
            "SELECT vat_regime FROM invoice_received WHERE number = 'EL-7'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(regime, "intracomunitaria");
    let d = models::draft(&v, "303", 2026, 4).unwrap();
    assert_eq!(get(&d, 10), Money::from_euros(200, 0), "{:?}", d.boxes);
}

#[test]
fn el_libro_del_gestor_no_duplica_las_facturas_que_taxpain_ya_tiene_con_el_numero_escrito_de_otra_forma()
 {
    use taxpain_app::invoicing::{InvoiceDraft, issue};
    use taxpain_app::received::{self, Nature, Reviewed, Source};
    use taxpain_core::invoice::{InvoiceKind, InvoiceLine, VatTreatment};
    use time::macros::date;

    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    // Una recibida revisada a mano de su PDF («F-0077») y después el libro del gestor con la
    // misma factura escrita a su manera («F77»): es la misma y su IVA se deduce una sola vez.
    let rec =
        received::ingest_file(&v, "f.pdf", b"%PDF-1.4 asesoria", Source::Manual, now).unwrap();
    let r = Reviewed {
        supplier_nif: "12345678Z".into(),
        supplier_name: "Asesoría Ejemplo".into(),
        number: "F-0077".into(),
        issue_date: date!(2025 - 03 - 01),
        base: Money::from_euros(300, 0),
        vat: Money::from_euros(63, 0),
        withholding: Money::from_euros(45, 0),
        total: Money::from_euros(318, 0),
        deductible_pct: 100,
        capital_good: false,
        nature: Nature::Profesional,
        asset_class: None,
        vat_regime: Default::default(),
        supplier_country: None,
        self_assessed_rate: None,
        intra_goods: false,
        cash_basis: false,
        import_dua: false,
        prorata_use: Default::default(),
    };
    received::review(&v, rec.id, &r, now).unwrap();
    let csv = "Fecha;Nº Factura;NIF Proveedor;Proveedor;Base;Cuota;Retención;Total\n\
               01/03/2025;F77;12345678Z;Asesoría Ejemplo;300,00;63,00;45,00;318,00\n";
    let b = libros::parse("recibidas.csv", csv.as_bytes())
        .unwrap()
        .unwrap();
    let imp = libros::import_received(&v, &b, "recibidas.csv", csv.as_bytes(), now).unwrap();
    assert_eq!((imp.added, imp.skipped), (0, 1), "{imp:?}");
    let revisadas: i64 = v
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM invoice_received WHERE status = 'revisada'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(revisadas, 1);

    // Una emitida expedida en TaxPain («A2025-0001») y el libro del gestor con «A2025-1».
    let d = InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: date!(2025 - 02 - 10),
        operation_date: None,
        recipient: Some(Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Cliente SL".into(),
            address: addr(),
            person: None,
        }),
        lines: vec![InvoiceLine {
            description: "Servicio".into(),
            quantity_milli: 1000,
            unit_price: Money::from_euros(1000, 0),
            discount: Rate::ZERO,
            vat: VatTreatment::Taxed {
                rate: Rate::percent(21),
            },
            surcharge: None,
            withholding: None,
            advance: false,
        }],
        rectified: vec![],
        rectified_amounts: None,
        description: "x".into(),
        notes: None,
        currency: None,
        oss: None,
    };
    let inv = issue(&v, &d, now, true).unwrap();
    assert_eq!(inv.number, "A2025-0001");
    let csv = "Fecha Expedición;Número Factura;NIF Destinatario;Nombre Destinatario;Base Imponible;Tipo IVA;Cuota IVA;Total\n\
               10/02/2025;A2025-1;B12345674;Cliente SL;1.000,00;21;210,00;1.210,00\n";
    let b = libros::parse("emitidas.csv", csv.as_bytes())
        .unwrap()
        .unwrap();
    let imp = libros::import_issued(&v, &b, now).unwrap();
    assert_eq!((imp.added, imp.skipped), (0, 1), "{imp:?}");
    let d = models::draft(&v, "303", 2025, 1).unwrap();
    assert_eq!(get(&d, 7), Money::from_euros(1000, 0), "{:?}", d.boxes);
    // La cuota de la revisada a mano se deduce en el trimestre de la revisión (4T 2026) y el
    // libro no la vuelve a meter en el 1T 2025.
    assert_eq!(get(&d, 29), Money::ZERO, "{:?}", d.boxes);
    let d = models::draft(&v, "303", 2026, 4).unwrap();
    assert_eq!(get(&d, 29), Money::from_euros(63, 0), "{:?}", d.boxes);
}

#[test]
fn un_libro_de_fechas_con_facturas_propias_no_se_propone_importar() {
    use taxpain_app::carpeta::{self, Found};
    use taxpain_app::invoicing::{InvoiceDraft, issue};
    use taxpain_core::invoice::{InvoiceKind, InvoiceLine, VatTreatment};
    use time::macros::date;

    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    // Una venta expedida en TaxPain en febrero de 2025 que no está en el libro del gestor.
    let d = InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: date!(2025 - 02 - 10),
        operation_date: None,
        recipient: Some(Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Cliente SL".into(),
            address: addr(),
            person: None,
        }),
        lines: vec![InvoiceLine {
            description: "Servicio".into(),
            quantity_milli: 1000,
            unit_price: Money::from_euros(1000, 0),
            discount: Rate::ZERO,
            vat: VatTreatment::Taxed {
                rate: Rate::percent(21),
            },
            surcharge: None,
            withholding: None,
            advance: false,
        }],
        rectified: vec![],
        rectified_amounts: None,
        description: "x".into(),
        notes: None,
        currency: None,
        oss: None,
    };
    issue(&v, &d, now, true).unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Libro emitidas.csv"), EMITIDAS).unwrap();
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    let libro = &inv.items[0];
    assert!(
        matches!(libro.found, Found::Libro { overlap: 1, .. }),
        "{libro:?}"
    );
    assert!(!libro.importable && libro.selectable, "{libro:?}");
    assert!(
        libro.warnings.iter().any(|w| w.contains("dos veces")),
        "{:?}",
        libro.warnings
    );
    // Sin facturas propias en esas fechas, se propone como siempre.
    let otra = setup();
    let inv = carpeta::scan(&otra, dir.path()).unwrap();
    assert!(
        matches!(inv.items[0].found, Found::Libro { overlap: 0, .. }) && inv.items[0].importable
    );
}

#[test]
fn una_lectura_sin_lo_esencial_no_se_aplica() {
    use taxpain_app::lectura_ia::{apply, parse_reply, simulate};
    use taxpain_app::presentados;

    let v = setup();
    let now = datetime!(2026-10-26 10:00 UTC);
    let own = Some("B98765431");
    // Emitida sin NIF del emisor: podría ser una factura ajena; no entra como venta propia.
    let r = parse_reply(
        r#"{"tipo":"factura_emitida","factura":{"nombre_emisor":"Otra Empresa SL",
            "nif_destinatario":"B12345674","nombre_destinatario":"Cliente SL","numero":"X-9",
            "fecha":"2025-03-01","base":"500.00","iva":"105.00","total":"605.00"}}"#,
    )
    .unwrap();
    let msg = apply(&v, "x9.pdf", b"%PDF x9", &r, own, now).unwrap();
    assert!(msg.contains("NIF del emisor"), "{msg}");
    let emitidas: i64 = v
        .connection()
        .query_row("SELECT COUNT(*) FROM invoice_issued", [], |r| r.get(0))
        .unwrap();
    assert_eq!(emitidas, 0);
    assert!(
        simulate(&v, "x9.pdf", b"%PDF x9", &r)
            .unwrap_err()
            .contains("NIF del emisor")
    );
    // Importe en formato inglés en una emitida propia: 1.234,56 €, no 1,23 €.
    let r = parse_reply(
        r#"{"tipo":"factura_emitida","factura":{"nif_emisor":"B98765431","nombre_emisor":"Ejemplo Servicios SL",
            "nif_destinatario":"B12345674","nombre_destinatario":"Cliente SL","numero":"A-77",
            "fecha":"2025-03-01","base":"1,234.56","iva":"259.26","total":"1,493.82"}}"#,
    )
    .unwrap();
    assert_eq!(
        simulate(&v, "a77.pdf", b"%PDF a77", &r).unwrap(),
        "factura completa"
    );
    apply(&v, "a77.pdf", b"%PDF a77", &r, own, now).unwrap();
    let base: i64 = v
        .connection()
        .query_row(
            "SELECT base FROM invoice_issued WHERE number = 'A-77'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(base, 123_456);
    // Importes que no cuadran: se avisa (sin bloquear).
    let r = parse_reply(
        r#"{"tipo":"factura_recibida","factura":{"nif_emisor":"B12345674","nombre_emisor":"Proveedor SL",
            "numero":"P-1","fecha":"2025-03-01","base":"1,234.56","iva":"259.26","total":"1.49"}}"#,
    )
    .unwrap();
    let msg = simulate(&v, "p1.pdf", b"%PDF p1", &r).unwrap();
    assert!(msg.contains("no cuadran"), "{msg}");

    // Modelo con número, ejercicio o periodo imposibles: no se guarda un modelo fantasma.
    for (modelo, ejercicio, periodo, que) in [
        ("abc", "2025", "1T", "modelo"),
        ("303", "20255", "1T", "ejercicio"),
        ("303", "2025", "abc", "periodo"),
    ] {
        let r = parse_reply(&format!(
            r#"{{"tipo":"modelo_aeat","modelo":{{"modelo":"{modelo}","ejercicio":{ejercicio},"periodo":"{periodo}","casillas":{{"27":"1,234.56"}}}}}}"#
        ))
        .unwrap();
        let err = simulate(&v, "m.pdf", b"%PDF m", &r).unwrap_err();
        assert!(err.contains(que), "{err}");
        let err = apply(&v, "m.pdf", b"%PDF m", &r, own, now)
            .unwrap_err()
            .to_string();
        assert!(err.contains(que), "{err}");
    }
    assert!(presentados::list(&v, None).unwrap().is_empty());
    // Uno bien escrito (con espacio detrás, en inglés el importe) se guarda con 1.234,56 €.
    let r = parse_reply(
        r#"{"tipo":"modelo_aeat","modelo":{"modelo":"303 ","ejercicio":2025,"periodo":"1T","casillas":{"27":"1,234.56"}}}"#,
    )
    .unwrap();
    apply(&v, "m303.pdf", b"%PDF m303", &r, own, now).unwrap();
    let filed = presentados::list(&v, Some(2025)).unwrap();
    assert_eq!(filed.len(), 1);
    assert_eq!(filed[0].model, "303");
    assert_eq!(
        filed[0].boxes.get(&27).copied(),
        Some(Money::from_cents(123_456))
    );
}

#[test]
fn las_subcarpetas_demasiado_profundas_se_cuentan_aparte() {
    use taxpain_app::carpeta;

    let v = setup();
    let dir = tempfile::tempdir().unwrap();
    // Nueve niveles: el noveno no se recorre, y se dice.
    let deep = dir.path().join("1/2/3/4/5/6/7/8/9");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(deep.join("libro.csv"), EMITIDAS).unwrap();
    std::fs::write(dir.path().join("1/2/3/4/5/6/7/8/libro.csv"), EMITIDAS).unwrap();
    std::fs::write(dir.path().join("notas.doc"), "x").unwrap();
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    assert_eq!(inv.items.len(), 1, "{:?}", inv.items);
    assert_eq!((inv.skipped, inv.over_limit, inv.too_deep), (1, 0, 1));
}

/// La autorrepercusión de compras en el libro de emitidas: la línea con el total sin la cuota, la
/// de la hoja de autorrepercusión y la que es la misma factura (número, proveedor y base) que una
/// del libro de recibidas, ya sea del de la carpeta o del de TaxPain. Una venta a un cliente que
/// también es proveedor con el mismo número pero otra base sigue siendo una venta.
#[test]
fn la_autorrepercusion_del_libro_de_emitidas_se_reconoce() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let emitidas = "Fecha Expedición;Número Factura;NIF Destinatario;Nombre Destinatario;Base Imponible;Tipo IVA;Cuota IVA;Total\n\
                    05/01/2026;NB-0001;IE1234567FA;Proveedor Nube Ltd;15,00;21;3,15;15,00\n\
                    31/01/2026;SW-77;DE123456789;Servicios Web GmbH;10,00;21;2,10;12,10\n\
                    10/02/2026;7;B12345674;Socio Comercial SL;500,00;21;105,00;605,00\n";
    let mut b = libros::parse("emitidas.csv", emitidas.as_bytes())
        .unwrap()
        .unwrap();
    assert_eq!(b.kind, BookKind::Emitidas);
    // Por sus importes: sólo la primera, y sin el aviso de que el total no cuadra.
    let marcadas: Vec<bool> = b.invoices.iter().map(|i| i.self_assessed).collect();
    assert_eq!(marcadas, [true, false, false]);
    assert!(
        !b.warnings.iter().any(|w| w.contains("no cuadra")),
        "{:?}",
        b.warnings
    );
    // El libro de recibidas: la segunda es la misma compra; la «7» del socio, con otra base, no.
    let recibidas = "Fecha;Número;NIF Proveedor;Proveedor;País;Base;Cuota;Total\n\
                     31/01/2026;SW-0077;DE123456789;Servicios Web GmbH;DE;10,00;2,10;10,00\n\
                     12/02/2026;7;B12345674;Socio Comercial SL;;40,00;8,40;48,40\n";
    let r = libros::parse("recibidas.csv", recibidas.as_bytes())
        .unwrap()
        .unwrap();
    assert_eq!(libros::mark_self_assessed(&mut b.clone(), &r.invoices), 2);
    libros::import_received(&v, &r, "recibidas.csv", recibidas.as_bytes(), now).unwrap();
    assert_eq!(
        libros::mark_self_assessed_in_vault(v.connection(), &mut b).unwrap(),
        2
    );
    assert!(!b.invoices[2].self_assessed);
    // Al importarlo, sólo entra la venta.
    let out = libros::import_issued(&v, &b, now).unwrap();
    assert_eq!((out.added, out.self_assessed), (1, 2));
    assert_eq!(invoicing::list_issued(&v, 2026).unwrap().len(), 1);

    // Un libro que empieza por la hoja de autorrepercusión: ésa no son ventas.
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet();
    ws.set_name("Inversión sujeto pasivo").unwrap();
    for (r, linea) in emitidas.lines().enumerate() {
        for (c, celda) in linea.split(';').enumerate() {
            ws.write_string(u32::try_from(r).unwrap(), u16::try_from(c).unwrap(), celda)
                .unwrap();
        }
    }
    let xlsx = wb.save_to_buffer().unwrap();
    let b = libros::parse("emitidas.xlsx", &xlsx).unwrap().unwrap();
    assert_eq!(b.invoices.len(), 3);
    assert!(b.invoices.iter().all(|i| i.self_assessed));
}

/// Dos copias del mismo fichero en carpetas distintas (la escritura y la que se aportó a un
/// expediente), leídas cada una con más o menos datos: al cargar el paquete se juntan campo a
/// campo, en vez de quedarse la última aunque tenga menos. Una lectura «otro» no cambia el tipo:
/// sólo completa lo que falta.
#[test]
fn las_lecturas_de_copias_del_mismo_fichero_se_juntan() {
    use taxpain_app::lectura_ia;
    let v = setup();
    let now = datetime!(2026-10-26 10:00 UTC);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let escritura = b"%PDF-1.4 escritura de ampliacion de capital".to_vec();
    let acta = b"%PDF-1.4 acta de la junta".to_vec();
    for (ruta, bytes) in [
        ("SOCIEDAD/Escritura ampliación.pdf", &escritura),
        ("AEAT/Expediente/Copia de la escritura.pdf", &escritura),
        ("SOCIEDAD/Acta.pdf", &acta),
        ("AEAT/Expediente/Acta.pdf", &acta),
    ] {
        let p = root.join(ruta);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
    std::fs::write(
        root.join("taxpain-lecturas.json"),
        r#"{"lecturas":[
          {"fichero":"SOCIEDAD/Escritura ampliación.pdf","lectura":{"tipo":"escritura_modificacion",
            "descripcion":"Ampliación de capital de Empresa Demo SL","empresa":{"name":"Empresa Demo SL",
            "nif":"B12345674","capital":"14000","administrator":"Ana Ejemplo","notary":"Notario Ejemplo",
            "protocol":"123","date":null}}},
          {"fichero":"AEAT/Expediente/Copia de la escritura.pdf","lectura":{"tipo":"escritura_modificacion",
            "descripcion":"Copia aportada al expediente","empresa":{"name":"Empresa Demo SL",
            "date":"2024-11-08"}}},
          {"fichero":"SOCIEDAD/Acta.pdf","lectura":{"tipo":"acta_junta","descripcion":"Acta de la junta",
            "empresa":{"name":"Empresa Demo SL","date":"2026-06-30"}}},
          {"fichero":"AEAT/Expediente/Acta.pdf","lectura":{"tipo":"otro","descripcion":"Anexo",
            "empresa":{"nif":"B12345674"}}}
        ]}"#,
    )
    .unwrap();
    let l = lectura_ia::load_package(&v, root, now).unwrap().unwrap();
    assert_eq!((l.loaded, l.merged, l.invalid), (4, 2, 0), "{l:?}");
    // La escritura: la más completa, con la fecha de la otra.
    let r = lectura_ia::stored(&v, &escritura).unwrap().unwrap();
    assert_eq!(r.tipo, "escritura_modificacion");
    assert_eq!(r.descripcion, "Ampliación de capital de Empresa Demo SL");
    let e = r.empresa.unwrap();
    assert_eq!(e["administrator"], "Ana Ejemplo");
    assert_eq!(e["capital"], "14000");
    assert_eq!(e["date"], "2024-11-08");
    // El acta: queda la lectura del acta; la que dice «otro» sólo le completa el NIF.
    let r = lectura_ia::stored(&v, &acta).unwrap().unwrap();
    assert_eq!(r.tipo, "acta_junta");
    assert_eq!(r.empresa.unwrap()["nif"], "B12345674");
    // Otra vez: lo mismo.
    let again = lectura_ia::load_package(&v, root, now).unwrap().unwrap();
    assert_eq!((again.loaded, again.merged), (4, 2));
}

/// Un libro de hoja de cálculo con estas hojas (nombre, filas separadas por «;»).
fn libro_xlsx(hojas: &[(&str, &str)]) -> Vec<u8> {
    let mut wb = rust_xlsxwriter::Workbook::new();
    for (nombre, filas) in hojas {
        let ws = wb.add_worksheet();
        ws.set_name(*nombre).unwrap();
        for (r, linea) in filas.lines().enumerate() {
            for (c, celda) in linea.split(';').enumerate() {
                ws.write_string(u32::try_from(r).unwrap(), u16::try_from(c).unwrap(), celda)
                    .unwrap();
            }
        }
    }
    wb.save_to_buffer().unwrap()
}

/// En el libro de emitidas, una hoja de «Entregas intracomunitarias» o de «Inversión sujeto
/// pasivo» puede ser de ventas: las entregas exentas a clientes de la UE (art. 25 LIVA) y las
/// ventas con inversión del sujeto pasivo (art. 84.Uno.2º LIVA) no llevan cuota. Una hoja de
/// inversión del sujeto pasivo sin ninguna cuota es de ventas, en la primera hoja y en las demás;
/// con cuotas, es la autorrepercusión de compras, también sus líneas al 0 % (una compra exenta o
/// no sujeta apuntada con las demás).
#[test]
fn las_ventas_sin_cuota_de_una_hoja_de_entregas_o_de_inversion_son_ventas() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let cabecera = "Fecha Expedición;Número Factura;NIF Destinatario;Nombre Destinatario;Base Imponible;Tipo IVA;Cuota IVA;Total\n";
    let entregas = format!(
        "{cabecera}10/03/2025;E-1;FR12345678901;Client Demo SARL;1.000,00;0;0,00;1.000,00\n\
         12/03/2025;E-2;DE123456789;Kunde Demo GmbH;2.000,00;0;0,00;2.000,00\n"
    );
    let b = libros::parse(
        "emitidas.xlsx",
        &libro_xlsx(&[("Entregas intracomunitarias", &entregas)]),
    )
    .unwrap()
    .unwrap();
    assert_eq!(b.kind, BookKind::Emitidas);
    assert!(b.invoices.iter().all(|i| !i.self_assessed), "{b:?}");
    let out = libros::import_issued(&v, &b, now).unwrap();
    assert_eq!((out.added, out.self_assessed), (2, 0), "{out:?}");

    // Un subcontratista de obra que factura con inversión del sujeto pasivo.
    let obra = format!(
        "{cabecera}15/04/2025;O-1;B12345674;Constructora Demo SL;8.000,00;0;0,00;8.000,00\n"
    );
    let b = libros::parse(
        "emitidas obra.xlsx",
        &libro_xlsx(&[("Inversión sujeto pasivo", &obra)]),
    )
    .unwrap()
    .unwrap();
    assert!(!b.invoices[0].self_assessed, "{b:?}");
    let out = libros::import_issued(&v, &b, now).unwrap();
    assert_eq!((out.added, out.self_assessed), (1, 0), "{out:?}");

    // Las mismas hojas detrás de la de facturas: entran sus ventas. La hoja de autorrepercusión
    // (con cuotas y una compra al 0 %), no.
    let facturas =
        format!("{cabecera}05/05/2025;A-1;B22222228;Cliente Demo SL;100,00;21;21,00;121,00\n");
    let autorrepercusion = format!(
        "{cabecera}07/05/2025;NB-9;IE1234567FA;Proveedor Nube Ltd;15,00;21;3,15;18,15\n\
         08/05/2025;77;;Dominios Demo Inc;12,40;0;0,00;12,40\n"
    );
    let b = libros::parse(
        "emitidas varias.xlsx",
        &libro_xlsx(&[
            ("Facturas", &facturas),
            ("Entregas intracomunitarias", &entregas),
            ("Inversión sujeto pasivo", &obra),
            ("Sujeto pasivo", &autorrepercusion),
        ]),
    )
    .unwrap()
    .unwrap();
    let numeros: Vec<&str> = b.invoices.iter().map(|i| i.number.as_str()).collect();
    assert_eq!(numeros, ["A-1", "E-1", "E-2", "O-1"], "{:?}", b.warnings);
    assert!(
        b.warnings
            .iter()
            .any(|w| w.contains("«Sujeto pasivo» es la autorrepercusión")),
        "{:?}",
        b.warnings
    );
    // La misma hoja de autorrepercusión como primera: sus dos líneas, también la del 0 %.
    let b = libros::parse(
        "emitidas compras.xlsx",
        &libro_xlsx(&[("Sujeto pasivo", &autorrepercusion)]),
    )
    .unwrap()
    .unwrap();
    assert_eq!(b.invoices.len(), 2, "{b:?}");
    assert!(b.invoices.iter().all(|i| i.self_assessed), "{b:?}");
}

/// Copias del mismo fichero con lecturas que no casan: el tipo lo dice la lectura que sabe qué
/// es (no la «otro», aunque traiga más datos; ésa sólo completa lo que falta), y los importes de
/// una factura se toman enteros de una sola lectura, sin mezclarlos con los de otra.
#[test]
fn las_lecturas_de_copias_no_mezclan_el_tipo_ni_los_importes() {
    use taxpain_app::lectura_ia;
    let v = setup();
    let now = datetime!(2026-10-26 10:00 UTC);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let escritura = b"%PDF-1.4 escritura de constitucion".to_vec();
    let factura = b"%PDF-1.4 factura de servicios".to_vec();
    for (ruta, bytes) in [
        ("AEAT/Expediente/Anexo 1.pdf", &escritura),
        ("SOCIEDAD/Escritura de constitución.pdf", &escritura),
        ("GASTOS/Factura A-7.pdf", &factura),
        ("BANCO/Justificantes/Factura A-7.pdf", &factura),
    ] {
        let p = root.join(ruta);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
    std::fs::write(
        root.join("taxpain-lecturas.json"),
        r#"{"lecturas":[
          {"fichero":"AEAT/Expediente/Anexo 1.pdf","lectura":{"tipo":"otro",
            "descripcion":"Anexo del expediente","empresa":{"street":"Calle Mayor 1",
            "post_code":"46001","town":"Valencia","capital":"3000","administrator":"Ana Ejemplo"}}},
          {"fichero":"SOCIEDAD/Escritura de constitución.pdf","lectura":{"tipo":"escritura_constitucion",
            "descripcion":"Constitución de Empresa Demo SL","empresa":{"name":"Empresa Demo SL",
            "nif":"B12345674"}}},
          {"fichero":"GASTOS/Factura A-7.pdf","lectura":{"tipo":"factura_recibida","factura":{
            "nif_emisor":"B11111119","nombre_emisor":"Asesores Demo SL","fecha":"2025-03-01",
            "base":"100","iva":"21","total":"121"}}},
          {"fichero":"BANCO/Justificantes/Factura A-7.pdf","lectura":{"tipo":"factura_recibida",
            "factura":{"base":"100","iva":"21","retencion":"15","total":"106"}}}
        ]}"#,
    )
    .unwrap();
    let l = lectura_ia::load_package(&v, root, now).unwrap().unwrap();
    assert_eq!((l.loaded, l.merged), (4, 2), "{l:?}");
    let r = lectura_ia::stored(&v, &escritura).unwrap().unwrap();
    assert_eq!(r.tipo, "escritura_constitucion");
    assert_eq!(r.descripcion, "Constitución de Empresa Demo SL");
    let e = r.empresa.unwrap();
    assert_eq!(
        (&e["name"], &e["nif"]),
        (&"Empresa Demo SL".into(), &"B12345674".into())
    );
    assert_eq!(
        e["administrator"], "Ana Ejemplo",
        "la «otro» completa lo que falta"
    );
    let r = lectura_ia::stored(&v, &factura).unwrap().unwrap();
    let f = r.factura.unwrap();
    assert_eq!(
        (&f["base"], &f["iva"], &f["total"]),
        (&"100".into(), &"21".into(), &"121".into())
    );
    assert!(f.get("retencion").is_none(), "{f}");
    assert_eq!(f["nif_emisor"], "B11111119");
}

/// Domicilio inventado: la factura completa lo exige (art. 6.1.e RD 1619/2012).
fn addr() -> Option<taxpain_core::invoice::Address> {
    Some(taxpain_core::invoice::Address {
        street: "Calle Inventada 1".into(),
        post_code: "46001".into(),
        town: "València".into(),
        province: "Valencia".into(),
        country: "ES".into(),
    })
}
