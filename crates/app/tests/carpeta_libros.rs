//! Carpeta de la empresa con libros del gestor: facturas que ya están en el libro (con otro
//! formato de número), de periodos ya declarados, leídas con el asistente, deshacer, cartera…
//! Datos inventados.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::carpeta::{self, Found};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::{invoicing, lectura_ia, libros, models, presentados, received};
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_invoicing::pdf::{Para, text_document};
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

/// PDF con texto de una factura recibida.
fn invoice_pdf(supplier: &str, nif: &str, number: &str, date: &str, lines: &[&str]) -> Vec<u8> {
    let mut doc = vec![
        Para::Bold(supplier.into()),
        Para::Text(format!("NIF {nif}")),
        Para::Text(format!("Factura nº {number}")),
        Para::Text(format!("Fecha {date}")),
    ];
    doc.extend(lines.iter().map(|l| Para::Text((*l).into())));
    text_document("Factura", &[doc]).unwrap()
}

fn get(d: &models::Draft, b: u16) -> Money {
    d.boxes
        .iter()
        .find(|(k, _)| *k == b)
        .map_or(Money::ZERO, |(_, v)| *v)
}

fn item<'a>(inv: &'a carpeta::Inventory, p: &str) -> &'a carpeta::Item {
    inv.items
        .iter()
        .find(|i| i.path.ends_with(p))
        .unwrap_or_else(|| panic!("{p} no está: {:?}", inv.items))
}

const LIBRO_RECIBIDAS: &str = "Fecha;Serie;Número;NIF Proveedor;Proveedor;Base;Cuota;Retención;Total\n\
01/03/2025;F;77;12345678Z;Asesoría Ejemplo;300,00;63,00;45,00;318,00\n\
10/03/2025;;9;;Bar Sin NIF;10,00;1,00;;11,00\n";

#[test]
fn una_factura_del_libro_no_se_deduce_dos_veces() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("2025/facturas")).unwrap();
    std::fs::write(
        root.join("2025/Libro facturas recibidas.csv"),
        LIBRO_RECIBIDAS,
    )
    .unwrap();
    // La misma factura del libro («F77»), con el número como lo pone el PDF («F-77»).
    let lines = [
        "Base imponible 300,00",
        "IVA 21% 63,00",
        "Retención IRPF 15% 45,00",
        "Total 318,00 €",
    ];
    std::fs::write(
        root.join("2025/facturas/asesoria.pdf"),
        invoice_pdf(
            "Asesoría Ejemplo",
            "12345678Z",
            "F-77",
            "01/03/2025",
            &lines,
        ),
    )
    .unwrap();
    // Una del mismo trimestre que no está en el libro del gestor.
    let other = ["Base imponible 100,00", "IVA 21% 21,00", "Total 121,00 €"];
    std::fs::write(
        root.join("2025/facturas/olvidada.pdf"),
        invoice_pdf(
            "Papelería Ejemplo SA",
            "A22222228",
            "P-5",
            "15/02/2025",
            &other,
        ),
    )
    .unwrap();
    // Y una nueva, de un periodo que no declaró el gestor.
    std::fs::write(
        root.join("2026.pdf"),
        invoice_pdf(
            "Papelería Ejemplo SA",
            "A22222228",
            "P-90",
            "15/09/2026",
            &other,
        ),
    )
    .unwrap();

    let inv = carpeta::scan(&v, root).unwrap();
    let a = item(&inv, "asesoria.pdf");
    assert!(
        matches!(a.found, Found::FacturaRecibida { in_book: true, .. }),
        "{a:?}"
    );
    assert!(a.importable);
    let o = item(&inv, "olvidada.pdf");
    assert!(
        matches!(
            o.found,
            Found::FacturaRecibida {
                historical: true,
                in_book: false,
                ..
            }
        ),
        "{o:?}"
    );
    assert!(!o.importable, "{o:?}");
    // No se propone, pero se puede marcar.
    assert!(o.selectable, "{o:?}");
    let n = item(&inv, "2026.pdf");
    assert!(
        matches!(
            n.found,
            Found::FacturaRecibida {
                historical: false,
                in_book: false,
                ..
            }
        ),
        "{n:?}"
    );

    let paths: Vec<String> = inv
        .items
        .iter()
        .filter(|i| i.importable)
        .map(|i| i.path.clone())
        .collect();
    let out = carpeta::apply(&v, root, &paths, now).unwrap();
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    let asesoria = out
        .iter()
        .find(|o| o.path.ends_with("asesoria.pdf"))
        .unwrap();
    assert!(asesoria.message.contains("adjuntada"), "{asesoria:?}");
    // Sólo la de 2026 queda para revisar; el IVA del 1T de 2025 es el del libro, una vez.
    let pending = received::list_received(&v, Some("pendiente")).unwrap();
    assert_eq!(pending.len(), 1, "{pending:?}");
    let d = models::draft(&v, "303", 2025, 1).unwrap();
    assert_eq!(get(&d, 29), Money::from_euros(64, 0), "{:?}", d.boxes);
    let file: String = v
        .connection()
        .query_row(
            "SELECT filename FROM invoice_received WHERE number = 'F77'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(file, "asesoria.pdf");

    // Otra vez: nada nuevo (y adjuntar no choca con el documento que ya existe).
    let again = carpeta::apply(&v, root, &paths, now).unwrap();
    assert!(again.iter().all(|o| o.ok), "{again:?}");
    assert_eq!(
        received::list_received(&v, Some("pendiente"))
            .unwrap()
            .len(),
        1
    );

    // Si el usuario marca la olvidada, entra para revisar con el aviso.
    let out = carpeta::apply(&v, root, &["2025/facturas/olvidada.pdf".to_owned()], now).unwrap();
    assert!(out[0].ok && out[0].message.contains("ojo"), "{out:?}");
    let pending = received::list_received(&v, Some("pendiente")).unwrap();
    let p = pending
        .iter()
        .find(|p| p.filename == "olvidada.pdf")
        .unwrap();
    assert!(
        p.extraction
            .warnings
            .iter()
            .any(|w| w.contains("ya declaró")),
        "{:?}",
        p.extraction.warnings
    );
}

/// Las facturas sueltas leídas con IA van después de los libros aunque su ruta vaya antes
/// («01_facturas» < «02_contabilidad»): en una sola pasada se adjuntan a su factura del libro en
/// vez de quedar pendientes en Recibidas. Aquí las reglas leen mal el número y el total de los
/// PDF (como un ticket en otra moneda o con el número recortado) y sólo la lectura con IA casa con
/// el libro, así que al importar el libro después no se reconocerían como suyas.
#[test]
fn en_una_pasada_las_facturas_leidas_con_ia_se_adjuntan_a_su_libro() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::create_dir_all(root.join("01_facturas")).unwrap();
    std::fs::create_dir_all(root.join("02_contabilidad")).unwrap();
    std::fs::write(
        root.join("02_contabilidad/Libro facturas recibidas.csv"),
        LIBRO_RECIBIDAS,
    )
    .unwrap();
    std::fs::write(
        root.join("01_facturas/asesoria.pdf"),
        invoice_pdf(
            "Asesoría Ejemplo",
            "12345678Z",
            "PED-555",
            "01/03/2025",
            &["Total 999,00 €"],
        ),
    )
    .unwrap();
    std::fs::write(
        root.join("01_facturas/bar.pdf"),
        invoice_pdf(
            "Bar Sin NIF",
            "",
            "T-1234",
            "10/03/2025",
            &["Total 99,00 €"],
        ),
    )
    .unwrap();
    std::fs::write(
        root.join("taxpain-lecturas.json"),
        r#"{"lecturas":[
          {"fichero":"01_facturas/asesoria.pdf","lectura":{"tipo":"factura_recibida","factura":{
            "nif_emisor":"12345678Z","nombre_emisor":"Asesoría Ejemplo","numero":"F-77",
            "fecha":"2025-03-01","base":"300","iva":"63","retencion":"45","total":"318"}}},
          {"fichero":"01_facturas/bar.pdf","lectura":{"tipo":"factura_recibida","factura":{
            "nombre_emisor":"Bar Sin NIF","numero":"9","fecha":"2025-03-10","base":"10",
            "iva":"1","total":"11"}}}
        ]}"#,
    )
    .unwrap();
    let inv = carpeta::scan(&v, root).unwrap();
    for p in ["asesoria.pdf", "bar.pdf"] {
        let i = item(&inv, p);
        assert!(
            matches!(&i.found, Found::Ia { tipo, importable: true, .. } if tipo == "factura_recibida"),
            "{i:?}"
        );
    }
    let paths: Vec<String> = inv
        .items
        .iter()
        .filter(|i| i.importable)
        .map(|i| i.path.clone())
        .collect();
    assert_eq!(paths.len(), 3, "{paths:?}");
    let out = carpeta::apply(&v, root, &paths, now).unwrap();
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    // El libro va primero.
    assert!(out[0].path.ends_with(".csv"), "{out:?}");
    for p in ["asesoria.pdf", "bar.pdf"] {
        let o = out.iter().find(|o| o.path.ends_with(p)).unwrap();
        assert!(o.message.contains("adjuntada"), "{o:?}");
    }
    assert!(
        received::list_received(&v, Some("pendiente"))
            .unwrap()
            .is_empty()
    );
    let attached: i64 = v
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM invoice_received WHERE status = 'revisada'
               AND document_id IN (SELECT id FROM document WHERE kind = 'recibida')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(attached, 2);
}

#[test]
fn el_pdf_pendiente_se_adjunta_al_importar_el_libro_y_deshacer_lo_devuelve() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let lines = [
        "Base imponible 300,00",
        "IVA 21% 63,00",
        "Retención IRPF 15% 45,00",
        "Total 318,00 €",
    ];
    std::fs::write(
        root.join("asesoria.pdf"),
        invoice_pdf(
            "Asesoría Ejemplo",
            "ES12345678Z",
            "F-0077",
            "01/03/2025",
            &lines,
        ),
    )
    .unwrap();
    // Primero el PDF (aún sin libro): queda pendiente.
    carpeta::apply(&v, root, &["asesoria.pdf".to_owned()], now).unwrap();
    assert_eq!(
        received::list_received(&v, Some("pendiente"))
            .unwrap()
            .len(),
        1
    );
    // Después el libro: la pendiente es la misma factura → se adjunta y se descarta.
    std::fs::write(root.join("Libro facturas recibidas.csv"), LIBRO_RECIBIDAS).unwrap();
    let out = carpeta::apply(&v, root, &["Libro facturas recibidas.csv".to_owned()], now).unwrap();
    assert!(out[0].ok && out[0].message.contains("pendiente"), "{out:?}");
    assert!(
        received::list_received(&v, Some("pendiente"))
            .unwrap()
            .is_empty()
    );

    // Deshacer la importación del libro: fuera sus facturas y la pendiente vuelve.
    let batches = libros::batches(&v).unwrap();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].invoices, 2);
    assert!(libros::undo(&v, batches[0].id, now, false).is_err());
    assert_eq!(libros::undo(&v, batches[0].id, now, true).unwrap(), 2);
    let n: i64 = v
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM invoice_received WHERE status = 'revisada'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 0);
    assert_eq!(
        received::list_received(&v, Some("pendiente"))
            .unwrap()
            .len(),
        1
    );
    assert!(libros::batches(&v).unwrap().is_empty());
}

#[test]
fn las_importadas_no_salen_en_la_cartera_ni_se_cobran_otra_vez() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let emitidas = "Fecha Expedición;Número Factura;NIF Destinatario;Nombre Destinatario;Base Imponible;Tipo IVA;Cuota IVA;Total\n\
                    15/01/2025;2025-001;B12345674;Cliente SL;1.000,00;21;210,00;1.210,00\n";
    let b = libros::parse("emitidas.csv", emitidas.as_bytes())
        .unwrap()
        .unwrap();
    let r = libros::import_issued(&v, &b, now).unwrap();
    assert!(r.batch.is_some());
    let b = libros::parse("recibidas.csv", LIBRO_RECIBIDAS.as_bytes())
        .unwrap()
        .unwrap();
    libros::import_received(&v, &b, "recibidas.csv", LIBRO_RECIBIDAS.as_bytes(), now).unwrap();
    let cartera = taxpain_app::cartera::list(&v, time::macros::date!(2026 - 10 - 24)).unwrap();
    assert!(cartera.is_empty(), "{cartera:?}");
    let id: i64 = v
        .connection()
        .query_row("SELECT id FROM invoice_issued", [], |r| r.get(0))
        .unwrap();
    let err = taxpain_app::cartera::installments(v.connection(), "emitida", id)
        .unwrap_err()
        .to_string();
    assert!(err.contains("libro del gestor"), "{err}");
    // Deshacer también las emitidas.
    let batch = libros::batches(&v)
        .unwrap()
        .into_iter()
        .find(|b| b.kind == "emitidas")
        .unwrap();
    assert_eq!(libros::undo(&v, batch.id, now, true).unwrap(), 1);
    assert!(invoicing::list_issued(&v, 2025).unwrap().is_empty());
}

#[test]
fn las_facturas_leidas_con_el_asistente_se_tratan_como_las_del_libro() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let emitidas = "Fecha Expedición;Número Factura;NIF Destinatario;Nombre Destinatario;Base Imponible;Tipo IVA;Cuota IVA;Total\n\
                    15/01/2025;2025-001;B12345674;Cliente SL;1.000,00;21;210,00;1.210,00\n";
    std::fs::write(root.join("Libro emitidas.csv"), emitidas).unwrap();
    std::fs::write(root.join("Libro recibidas.csv"), LIBRO_RECIBIDAS).unwrap();
    // PDF (de mentira: la lectura va por la huella) de facturas leídas por Claude Code.
    std::fs::write(root.join("emitida-1.pdf"), b"%PDF-1.4 emitida 1").unwrap();
    std::fs::write(root.join("emitida-10.pdf"), b"%PDF-1.4 emitida 10").unwrap();
    std::fs::write(root.join("emitida-rara.pdf"), b"%PDF-1.4 emitida rara").unwrap();
    std::fs::write(root.join("recibida.pdf"), b"%PDF-1.4 recibida").unwrap();
    std::fs::write(
        root.join("taxpain-lecturas.json"),
        r#"{"lecturas":[
          {"fichero":"emitida-1.pdf","lectura":{"tipo":"factura_emitida","factura":{
            "nif_emisor":"ES-B98765431","nif_destinatario":"B-12345674","numero":"2025/001",
            "fecha":"2025-01-15","base":"1000.00","iva":"210.00","total":"1210.00"}}},
          {"fichero":"emitida-10.pdf","lectura":{"tipo":"factura_emitida","factura":{
            "nif_emisor":"B98765431","nif_destinatario":"B-12345674","numero":"2025-010",
            "fecha":"2025-02-15","tipos":[{"base":"500","tipo":"0.21","cuota":"105.00"}],
            "total":"605","nombre_destinatario":null}}},
          {"fichero":"emitida-rara.pdf","lectura":{"tipo":"factura_emitida","factura":{
            "nif_emisor":"B98765431","nif_destinatario":"B12345674","numero":"2026-003",
            "fecha":"2026-02-15","tipos":[{"base":"100","tipo":"7","cuota":"7"}]}}},
          {"fichero":"recibida.pdf","lectura":{"tipo":"factura_recibida","factura":{
            "nif_emisor":"ES12345678Z","nombre_emisor":"Asesoría Ejemplo","numero":"F 77",
            "fecha":"2025-03-01","base":"300","iva":"63","retencion":"45","total":"318"}}}
        ]}"#,
    )
    .unwrap();
    let inv = carpeta::scan(&v, root).unwrap();
    assert_eq!(inv.claude_code.as_ref().unwrap().loaded, 4);
    // Con los libros sin importar aún: la recibida está en el libro de la carpeta.
    let r = item(&inv, "recibida.pdf");
    assert!(
        matches!(&r.found, Found::Ia { importable: true, detalle, .. } if detalle.contains("libro")),
        "{r:?}"
    );
    // Primero los libros.
    let out = carpeta::apply(
        &v,
        root,
        &[
            "Libro emitidas.csv".to_owned(),
            "Libro recibidas.csv".to_owned(),
        ],
        now,
    )
    .unwrap();
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    // Volver a leer: la emitida 2025/001 ya está («2025-001»); la 2025-010 es de un trimestre
    // que cubre el libro del gestor; las dos, sin marcar.
    let inv = carpeta::scan(&v, root).unwrap();
    for (p, what) in [
        ("emitida-1.pdf", "ya está"),
        ("emitida-10.pdf", "ya declaró"),
    ] {
        let i = item(&inv, p);
        assert!(
            matches!(&i.found, Found::Ia { importable: false, detalle, .. } if detalle.contains(what)),
            "{i:?}"
        );
    }
    let all: Vec<String> = [
        "emitida-1.pdf",
        "emitida-10.pdf",
        "emitida-rara.pdf",
        "recibida.pdf",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    let out = carpeta::apply(&v, root, &all, now).unwrap();
    let msg = |p: &str| out.iter().find(|o| o.path == p).unwrap().clone();
    assert!(
        msg("emitida-1.pdf").message.contains("ya estaba"),
        "{out:?}"
    );
    assert!(msg("emitida-10.pdf").ok, "{out:?}");
    // Un 7 % no es un tipo español: no se importa.
    assert!(!msg("emitida-rara.pdf").ok, "{out:?}");
    assert!(msg("recibida.pdf").message.contains("adjuntada"), "{out:?}");
    assert!(
        received::list_received(&v, Some("pendiente"))
            .unwrap()
            .is_empty()
    );
    // La 2025-010: NIF normalizado (el 347 la junta con las del libro), nombre del cliente,
    // tipo del 21 % (no del 0,21 %) y en un lote que se puede deshacer.
    let (tax_id, name, vat): (String, String, i64) = v
        .connection()
        .query_row(
            "SELECT recipient_tax_id, recipient_name, vat FROM invoice_issued WHERE number = '2025-010'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        (tax_id.as_str(), name.as_str(), vat),
        ("B12345674", "Cliente SL", 10_500)
    );
    let d = models::draft(&v, "303", 2025, 1).unwrap();
    assert_eq!(get(&d, 7), Money::from_euros(1500, 0), "{:?}", d.boxes);
    let lectura = libros::batches(&v)
        .unwrap()
        .into_iter()
        .find(|b| b.source == "lectura")
        .unwrap();
    assert_eq!(libros::undo(&v, lectura.id, now, true).unwrap(), 1);
    let d = models::draft(&v, "303", 2025, 1).unwrap();
    assert_eq!(get(&d, 7), Money::from_euros(1000, 0));
}

#[test]
fn un_modelo_leido_con_el_asistente_se_compara_bien_y_sustituye_a_la_lectura_del_pdf() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let emitidas = "Fecha Expedición;Número Factura;NIF Destinatario;Nombre Destinatario;Base Imponible;Tipo IVA;Cuota IVA;Total\n\
                    15/01/2025;2025-001;B12345674;Cliente SL;1.000,00;21;210,00;1.210,00\n";
    let b = libros::parse("emitidas.csv", emitidas.as_bytes())
        .unwrap()
        .unwrap();
    libros::import_issued(&v, &b, now).unwrap();
    let d = models::draft(&v, "303", 2025, 1).unwrap();
    // Lo que habría leído el asistente: todas las casillas, también las de tipos ([02] 21 %,
    // [65] 100 %).
    let mut casillas: Vec<String> = d
        .boxes
        .iter()
        .filter(|(_, m)| !m.is_zero())
        .map(|(b, m)| format!("\"{b:02}\":\"{}.{:02}\"", m.cents() / 100, m.cents() % 100))
        .collect();
    casillas.push("\"02\":\"21.00\"".into());
    let reading = lectura_ia::parse_reply(&format!(
        r#"{{"tipo":"modelo_aeat","modelo":{{"modelo":"303","ejercicio":2025,"periodo":"1t","casillas":{{{}}}}}}}"#,
        casillas.join(",")
    ))
    .unwrap();
    let pdf = b"%PDF-1.4 303 del gestor";
    // Antes, las reglas sacaron del mismo PDF una lectura incompleta.
    let partial = presentados::Read {
        model: "303".into(),
        year: 2025,
        period: "1T".into(),
        source: "pdf".into(),
        boxes: [(7u16, Money::from_euros(999, 0))].into_iter().collect(),
        boxes_read: true,
        ..presentados::Read::default()
    };
    presentados::store(&v, "303.pdf", pdf, &partial, now).unwrap();
    let msg = lectura_ia::apply(&v, "303.pdf", pdf, &reading, Some("B98765431"), now).unwrap();
    assert!(msg.contains("sustituye"), "{msg}");
    let filed = presentados::list(&v, Some(2025)).unwrap();
    assert_eq!(filed.len(), 1);
    let c = presentados::compare(&v, filed[0].id).unwrap();
    assert_eq!(c.mismatches, 0, "{:?}", c.rows);
}

#[test]
fn la_lectura_coge_el_almacen_fichero_a_fichero_y_no_sigue_enlaces() {
    let v = setup();
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("Libro facturas recibidas.csv"), LIBRO_RECIBIDAS).unwrap();
    std::fs::write(root.join("notas.txt"), "llamar al gestor").unwrap();
    std::fs::write(outside.path().join("secreto.csv"), LIBRO_RECIBIDAS).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.path().join("secreto.csv"), root.join("enlace.csv"))
        .unwrap();
    let locks = std::cell::Cell::new(0);
    let inv = carpeta::scan_with(root, || {
        locks.set(locks.get() + 1);
        Ok(&v)
    })
    .unwrap();
    assert!(locks.get() >= 3, "{}", locks.get());
    assert_eq!(inv.items.len(), 2, "{:?}", inv.items);
    #[cfg(unix)]
    assert_eq!(inv.skipped, 1);
    // Importar sólo mira los ficheros elegidos (y una ruta que no está, falla aparte).
    let out = carpeta::apply_with(
        root,
        &[
            "Libro facturas recibidas.csv".to_owned(),
            "no-existe.csv".to_owned(),
        ],
        || Ok(&v),
        datetime!(2026-10-24 10:00 UTC),
    )
    .unwrap();
    assert_eq!(out.len(), 2);
    assert!(
        out.iter().any(|o| o.ok) && out.iter().any(|o| !o.ok),
        "{out:?}"
    );
}

#[test]
fn el_diario_del_gestor_no_se_propone_si_taxpain_ya_tiene_asientos_de_esas_fechas() {
    use taxpain_core::accounting::account::acc;
    use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    taxpain_app::accounting::manual_entry(
        &v,
        &JournalEntry {
            date: time::macros::date!(2025 - 01 - 10),
            concept: "Aportación".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("5720001"), Money::from_euros(3000, 0)),
                EntryLine::credit(acc("100"), Money::from_euros(3000, 0)),
            ],
        },
        now,
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("diario.csv"),
        "Fecha;Asiento;Cuenta;Concepto;Debe;Haber\n\
         02/01/2025;1;572;Aportación;3.000,00;\n\
         02/01/2025;1;100;Aportación;;3.000,00\n\
         31/03/2025;2;628;Luz;50,00;\n\
         31/03/2025;2;572;Luz;;50,00\n",
    )
    .unwrap();
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    let d = &inv.items[0];
    assert!(matches!(d.found, Found::Diario { overlap: 1, .. }), "{d:?}");
    assert!(!d.importable && d.selectable);
    assert!(
        d.warnings.iter().any(|w| w.contains("dos veces")),
        "{:?}",
        d.warnings
    );
}

/// Diario con columnas que las reglas no entienden y su lectura con IA (fila de títulos 2).
const DIARIO_RARO: &str = "Apuntes de la empresa;;;;;\n\
F.;Nº;Cta.;Texto;Cargo €;Abono €\n\
02/01/2025;1;572;Aportación;3000;\n\
02/01/2025;1;100;Aportación;;3000\n\
31/03/2025;2;628;Luz;50;\n\
31/03/2025;2;572;Luz;;50\n";

const LECTURA_DIARIO_RARO: &str = r#"{"lecturas":[{"fichero":"diario raro.csv","lectura":{
  "tipo":"diario","descripcion":"Diario de 2025","hoja":{"fila_titulos":2,"primera_fila":3,
  "columnas":{"fecha":"A","asiento":"B","cuenta":"C","concepto":"D","debe":"E","haber":"F"}}}}]}"#;

/// Un diario leído con IA pasa las mismas comprobaciones que uno que leen las reglas: si el
/// ejercicio ya está cargado con «Un diario entero» (o con otro lote) no se propone, y dice por
/// qué; si ya se importó, ni se propone ni se puede marcar.
#[test]
fn el_diario_leido_con_ia_no_se_propone_si_el_ejercicio_ya_tiene_sus_asientos() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("diario raro.csv"), DIARIO_RARO).unwrap();
    std::fs::write(root.join("taxpain-lecturas.json"), LECTURA_DIARIO_RARO).unwrap();
    // El ejercicio 2025 del gestor, cargado asiento a asiento.
    let gestor = "Fecha;Asiento;Cuenta;Concepto;Debe;Haber\n\
                  02/01/2025;1;572;Aportación;3.000,00;\n\
                  02/01/2025;1;100;Aportación;;3.000,00\n\
                  31/03/2025;2;628;Luz;50,00;\n\
                  31/03/2025;2;572;Luz;;50,00\n";
    let lot = taxpain_app::ejercicio_gestor::import_file(
        &v,
        2025,
        "diario 2025.csv",
        gestor.as_bytes(),
        &taxpain_app::ejercicio_gestor::Options::default(),
        now,
    )
    .unwrap();
    let inv = carpeta::scan(&v, root).unwrap();
    let d = item(&inv, "diario raro.csv");
    assert!(
        matches!(&d.found, Found::Ia { tipo, .. } if tipo == "diario"),
        "{d:?}"
    );
    assert!(!d.importable, "{d:?}");
    assert!(d.selectable, "{d:?}");
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("Un diario entero") && w.contains("diario 2025.csv")),
        "{:?}",
        d.warnings
    );
    // Marcado a sabiendas, entra con el aviso.
    let out = carpeta::apply(&v, root, &["diario raro.csv".to_owned()], now).unwrap();
    assert!(out[0].ok && out[0].message.contains("ojo"), "{out:?}");
    // Ya importado: ni se propone ni se puede marcar.
    let inv = carpeta::scan(&v, root).unwrap();
    let d = item(&inv, "diario raro.csv");
    assert!(
        matches!(&d.found, Found::Ia { detalle, .. } if detalle.contains("ya importado")),
        "{d:?}"
    );
    assert!(!d.importable && !d.selectable, "{d:?}");
    // Sin los asientos del gestor (y sin el lote de la carpeta), se propone.
    taxpain_app::ejercicio_gestor::undo(&v, lot.id, now, true).unwrap();
    let batch: i64 = v
        .connection()
        .query_row("SELECT id FROM import_batch", [], |r| r.get(0))
        .unwrap();
    taxpain_app::import::undo(&v, batch, now, true).unwrap();
    let inv = carpeta::scan(&v, root).unwrap();
    let d = item(&inv, "diario raro.csv");
    assert!(d.importable && d.selectable, "{d:?}");
}

/// Libro de recibidas con dos compras a proveedores de la UE con inversión del sujeto pasivo (el
/// total no lleva la cuota) y una interior.
const RECIBIDAS_UE: &str = "Fecha;Número;NIF Proveedor;Proveedor;País;Base;Cuota;Total\n\
05/01/2026;NB-0001;IE1234567FA;Proveedor Nube Ltd;IE;15,00;3,15;15,00\n\
31/01/2026;SW-77;DE123456789;Servicios Web GmbH;DE;10,00;2,10;10,00\n\
15/02/2026;P-5;A22222228;Papelería Ejemplo SA;;100,00;21,00;121,00\n";

const EMITIDAS_CABECERA: &str = "Fecha Expedición;Número Factura;NIF Destinatario;Nombre Destinatario;Base Imponible;Tipo IVA;Cuota IVA;Total\n";

/// Las dos compras de la UE repetidas en el libro de emitidas (su autorrepercusión): la primera
/// con el total sin la cuota, la segunda con la cuota en el total pero con el mismo número y
/// proveedor que en el libro de recibidas.
const AUTORREPERCUSION: &str = "05/01/2026;NB-0001;IE1234567FA;Proveedor Nube Ltd;15,00;21;3,15;15,00\n\
31/01/2026;SW-77;DE123456789;Servicios Web GmbH;10,00;21;2,10;12,10\n";

fn issued_count(v: &Vault) -> i64 {
    v.connection()
        .query_row("SELECT COUNT(*) FROM invoice_issued", [], |r| r.get(0))
        .unwrap()
}

/// Un libro de emitidas con la autorrepercusión de compras de proveedores extranjeros: esas
/// líneas no son ventas. Se reconocen por sus importes (el total sin la cuota) o porque la misma
/// factura está en el libro de recibidas; al importarlo sólo entran las ventas, y el IVA
/// devengado del 303 es sólo el de las ventas.
#[test]
fn la_autorrepercusion_del_libro_de_emitidas_no_entra_como_ventas() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("Libro recibidas 2026.csv"), RECIBIDAS_UE).unwrap();
    std::fs::write(
        root.join("Libro emitidas 2026.csv"),
        format!(
            "{EMITIDAS_CABECERA}{AUTORREPERCUSION}\
             20/02/2026;2026-001;B12345674;Cliente SL;1.000,00;21;210,00;1.210,00\n"
        ),
    )
    .unwrap();
    let inv = carpeta::scan(&v, root).unwrap();
    let e = item(&inv, "Libro emitidas 2026.csv");
    assert!(
        matches!(
            e.found,
            Found::Libro {
                invoices: 3,
                self_assessed: 2,
                ..
            }
        ),
        "{e:?}"
    );
    // Tiene una venta: se propone, y avisa de las otras dos.
    assert!(e.importable, "{e:?}");
    assert!(
        e.warnings
            .iter()
            .any(|w| w.contains("2 de sus 3 líneas son la autorrepercusión")),
        "{:?}",
        e.warnings
    );
    let paths: Vec<String> = inv
        .items
        .iter()
        .filter(|i| i.importable)
        .map(|i| i.path.clone())
        .collect();
    let out = carpeta::apply(&v, root, &paths, now).unwrap();
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    // El de recibidas, primero (con él se reconoce la segunda línea).
    assert!(out[0].path.contains("recibidas"), "{out:?}");
    let msg = &out[1].message;
    assert!(
        msg.starts_with("1 facturas al libro de emitidas") && msg.contains("2 líneas del libro"),
        "{msg}"
    );
    assert_eq!(issued_count(&v), 1);
    // IVA devengado del 1T de 2026: sólo la venta.
    let d = models::draft(&v, "303", 2026, 1).unwrap();
    assert_eq!(get(&d, 7), Money::from_euros(1000, 0), "{:?}", d.boxes);
}

/// La base y el IVA que se enseñan del libro de emitidas son los de lo que entra como ventas,
/// sin la autorrepercusión: la que se ve por sus importes y la que dice el libro de recibidas.
#[test]
fn la_base_y_el_iva_del_libro_de_emitidas_son_los_de_sus_ventas() {
    let v = setup();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("Libro recibidas 2026.csv"), RECIBIDAS_UE).unwrap();
    std::fs::write(
        root.join("Libro emitidas 2026.csv"),
        format!(
            "{EMITIDAS_CABECERA}{AUTORREPERCUSION}\
             20/02/2026;2026-001;B12345674;Cliente SL;1.000,00;21;210,00;1.210,00\n"
        ),
    )
    .unwrap();
    let inv = carpeta::scan(&v, root).unwrap();
    let e = item(&inv, "Libro emitidas 2026.csv");
    let Found::Libro {
        invoices,
        self_assessed,
        base,
        vat,
        ..
    } = &e.found
    else {
        panic!("{e:?}");
    };
    assert_eq!((*invoices, *self_assessed), (3, 2));
    assert_eq!(
        (*base, *vat),
        (Money::from_euros(1000, 0), Money::from_euros(210, 0))
    );
}

/// Si todas las líneas del libro de emitidas son la autorrepercusión de compras, el libro no se
/// propone; marcado a sabiendas, no entra ninguna como venta (aunque el libro de recibidas de la
/// carpeta no se importe).
#[test]
fn un_libro_de_emitidas_que_es_toda_autorrepercusion_no_se_propone() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("Libro recibidas 2026.csv"), RECIBIDAS_UE).unwrap();
    std::fs::write(
        root.join("Libro emitidas 2026.csv"),
        format!("{EMITIDAS_CABECERA}{AUTORREPERCUSION}"),
    )
    .unwrap();
    let inv = carpeta::scan(&v, root).unwrap();
    let e = item(&inv, "Libro emitidas 2026.csv");
    assert!(
        matches!(
            e.found,
            Found::Libro {
                invoices: 2,
                self_assessed: 2,
                ..
            }
        ),
        "{e:?}"
    );
    assert!(!e.importable && e.selectable, "{e:?}");
    assert!(
        e.warnings
            .iter()
            .any(|w| w.starts_with("es la autorrepercusión de compras")),
        "{:?}",
        e.warnings
    );
    // Sólo la primera línea se reconoce por sus importes; la segunda, por el libro de recibidas
    // de la carpeta, aunque no se importe.
    let out = carpeta::apply(&v, root, &["Libro emitidas 2026.csv".to_owned()], now).unwrap();
    assert!(out[0].ok, "{out:?}");
    assert!(out[0].message.contains("2 líneas del libro son"), "{out:?}");
    assert_eq!(issued_count(&v), 0);
    // Con el de recibidas importado a la vez, tampoco.
    let out = carpeta::apply(
        &v,
        root,
        &[
            "Libro emitidas 2026.csv".to_owned(),
            "Libro recibidas 2026.csv".to_owned(),
        ],
        now,
    )
    .unwrap();
    assert!(out.iter().all(|o| o.ok), "{out:?}");
    assert_eq!(issued_count(&v), 0, "{out:?}");
}

/// Si la lectura con IA dice que el libro que las reglas leen como de emitidas no es un libro de
/// ventas, no se propone y se dice por qué.
#[test]
fn si_la_lectura_dice_que_no_son_ventas_el_libro_de_emitidas_no_se_propone() {
    let v = setup();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(
        root.join("Libro emitidas 2026.csv"),
        format!(
            "{EMITIDAS_CABECERA}20/02/2026;2026-001;B12345674;Cliente SL;1.000,00;21;210,00;1.210,00\n"
        ),
    )
    .unwrap();
    std::fs::write(
        root.join("taxpain-lecturas.json"),
        r#"{"lecturas":[{"fichero":"Libro emitidas 2026.csv","lectura":{"tipo":"otro",
          "descripcion":"No tiene ventas: es la autorrepercusión de las compras"}}]}"#,
    )
    .unwrap();
    let inv = carpeta::scan(&v, root).unwrap();
    let e = item(&inv, "Libro emitidas 2026.csv");
    assert!(
        matches!(&e.found, Found::Libro { reading: Some(r), .. } if r.contains("No tiene ventas")),
        "{e:?}"
    );
    assert!(!e.importable && e.selectable, "{e:?}");
    assert!(
        e.warnings
            .iter()
            .any(|w| w.contains("la lectura con IA dice que no es un libro de ventas")),
        "{:?}",
        e.warnings
    );
}

/// Las líneas del libro de emitidas que sólo el libro de recibidas de la carpeta dice que son la
/// autorrepercusión de compras tampoco entran como ventas si ese libro no se importa a la vez:
/// aquí no se propone, porque TaxPain ya tiene una compra propia en esas fechas.
#[test]
fn la_autorrepercusion_que_dice_el_libro_de_recibidas_no_entra_aunque_no_se_importe() {
    use taxpain_app::received::{Nature, Reviewed};
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    received::register_typed(
        &v,
        &Reviewed {
            supplier_nif: "B11111119".into(),
            supplier_name: "Gestoría Demo SL".into(),
            number: "G-1".into(),
            issue_date: time::macros::date!(2026 - 02 - 01),
            base: Money::from_euros(50, 0),
            vat: Money::from_euros(10, 50),
            withholding: Money::ZERO,
            total: Money::from_euros(60, 50),
            deductible_pct: 100,
            capital_good: false,
            nature: Nature::General,
            asset_class: None,
            vat_regime: Default::default(),
            supplier_country: None,
            self_assessed_rate: None,
            intra_goods: false,
            cash_basis: false,
            import_dua: false,
            prorata_use: Default::default(),
        },
        now,
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("Libro recibidas 2026.csv"), RECIBIDAS_UE).unwrap();
    std::fs::write(
        root.join("Libro emitidas 2026.csv"),
        format!(
            "{EMITIDAS_CABECERA}{AUTORREPERCUSION}\
             20/02/2026;2026-001;B12345674;Cliente SL;1.000,00;21;210,00;1.210,00\n"
        ),
    )
    .unwrap();
    let inv = carpeta::scan(&v, root).unwrap();
    assert!(!item(&inv, "Libro recibidas 2026.csv").importable);
    let e = item(&inv, "Libro emitidas 2026.csv");
    assert!(e.importable, "{e:?}");
    assert!(
        e.warnings
            .iter()
            .any(|w| w.contains("2 de sus 3 líneas son la autorrepercusión")),
        "{:?}",
        e.warnings
    );
    let paths: Vec<String> = inv
        .items
        .iter()
        .filter(|i| i.importable)
        .map(|i| i.path.clone())
        .collect();
    assert_eq!(paths, ["Libro emitidas 2026.csv"]);
    let out = carpeta::apply(&v, root, &paths, now).unwrap();
    assert!(out[0].ok, "{out:?}");
    assert!(
        out[0]
            .message
            .starts_with("1 facturas al libro de emitidas")
            && out[0].message.contains("2 líneas del libro"),
        "{out:?}"
    );
    assert_eq!(issued_count(&v), 1);
    let d = models::draft(&v, "303", 2026, 1).unwrap();
    assert_eq!(get(&d, 7), Money::from_euros(1000, 0), "{:?}", d.boxes);
}

#[test]
fn una_factura_con_nombre_de_documento_sigue_siendo_una_factura() {
    let v = setup();
    let dir = tempfile::tempdir().unwrap();
    // «NIF» y «036» en el nombre, pero dentro hay una factura (sin la palabra «factura»).
    let doc = vec![
        Para::Bold("Papelería Ejemplo SA".into()),
        Para::Text("NIF A22222228".into()),
        Para::Text("Nº T-0360".into()),
        Para::Text("Fecha 03/02/2026".into()),
        Para::Text("Base imponible 10,00".into()),
        Para::Text("IVA 21% 2,10".into()),
        Para::Text("Total 12,10 €".into()),
    ];
    std::fs::write(
        dir.path().join("NIF papeleria 036.pdf"),
        text_document("Ticket", &[doc]).unwrap(),
    )
    .unwrap();
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    assert!(
        matches!(inv.items[0].found, Found::FacturaRecibida { .. }),
        "{:?}",
        inv.items[0]
    );
}

#[test]
fn un_modelo_del_gestor_en_la_carpeta_marca_su_trimestre_como_declarado() {
    let v = setup();
    let dir = tempfile::tempdir().unwrap();
    // El 303 del 1T de 2025 «del gestor» (el fichero que saldría de sus datos).
    let gestor = setup();
    let b = libros::parse("recibidas.csv", LIBRO_RECIBIDAS.as_bytes())
        .unwrap()
        .unwrap();
    let now = datetime!(2026-10-24 10:00 UTC);
    libros::import_received(&gestor, &b, "r.csv", LIBRO_RECIBIDAS.as_bytes(), now).unwrap();
    // Con el diseño de registro de 2025, como el que presenta la gestoría.
    let d = models::draft(&gestor, "303", 2025, 1).unwrap();
    let m303 = taxpain_models::dr_versions::file_with_boxes("303", 2025, "1T", &d.boxes).unwrap();
    std::fs::write(dir.path().join("303 1T 2025.txt"), m303).unwrap();
    let other = ["Base imponible 100,00", "IVA 21% 21,00", "Total 121,00 €"];
    std::fs::write(
        dir.path().join("olvidada.pdf"),
        invoice_pdf(
            "Papelería Ejemplo SA",
            "A22222228",
            "P-5",
            "15/02/2025",
            &other,
        ),
    )
    .unwrap();
    let inv = carpeta::scan(&v, dir.path()).unwrap();
    let o = item(&inv, "olvidada.pdf");
    assert!(
        matches!(
            o.found,
            Found::FacturaRecibida {
                historical: true,
                ..
            }
        ),
        "{o:?}"
    );
    assert!(!o.importable);
}

#[test]
fn revisar_otra_vez_una_factura_del_libro_da_error() {
    let v = setup();
    let now = datetime!(2026-10-24 10:00 UTC);
    let b = libros::parse("recibidas.csv", LIBRO_RECIBIDAS.as_bytes())
        .unwrap()
        .unwrap();
    libros::import_received(&v, &b, "recibidas.csv", LIBRO_RECIBIDAS.as_bytes(), now).unwrap();
    // El PDF arrastrado a mano (sin pasar por la carpeta) queda pendiente…
    let pdf = invoice_pdf("Asesoría Ejemplo", "12345678Z", "F-0077", "01/03/2025", &[]);
    let out = received::ingest_file(&v, "f.pdf", &pdf, received::Source::Manual, now).unwrap();
    // …pero al revisarlo se ve que ya está en el libro («F77»).
    let reviewed = received::Reviewed {
        supplier_nif: "12345678Z".into(),
        supplier_name: "Asesoría Ejemplo".into(),
        number: "F-0077".into(),
        issue_date: time::macros::date!(2025 - 03 - 01),
        base: Money::from_euros(300, 0),
        vat: Money::from_euros(63, 0),
        withholding: Money::from_euros(45, 0),
        total: Money::from_euros(318, 0),
        deductible_pct: 100,
        capital_good: false,
        nature: received::Nature::Profesional,
        asset_class: None,
        vat_regime: Default::default(),
        supplier_country: None,
        self_assessed_rate: None,
        intra_goods: false,
        cash_basis: false,
        import_dua: false,
        prorata_use: Default::default(),
    };
    let err = received::review(&v, out.id, &reviewed, now)
        .unwrap_err()
        .to_string();
    assert!(err.contains("ya está en el libro"), "{err}");
    // La del año siguiente con el mismo número sí se puede revisar (la numeración empieza).
    let pdf = invoice_pdf("Asesoría Ejemplo", "12345678Z", "F-77", "01/03/2026", &[]);
    let out = received::ingest_file(&v, "g.pdf", &pdf, received::Source::Manual, now).unwrap();
    let next = received::Reviewed {
        number: "F-77".into(),
        issue_date: time::macros::date!(2026 - 03 - 01),
        ..reviewed
    };
    received::review(&v, out.id, &next, now).unwrap();
}

#[test]
fn una_factura_sin_fecha_de_un_trimestre_declarado_no_se_propone() {
    let v = setup();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let now = datetime!(2026-10-24 10:00 UTC);
    // El 303 del 3T de 2024 del gestor, en la carpeta.
    let d = models::draft(&setup(), "303", 2024, 3).unwrap();
    let m303 = taxpain_models::dr_versions::file_with_boxes("303", 2024, "3T", &d.boxes).unwrap();
    std::fs::create_dir_all(root.join("MODELOS")).unwrap();
    std::fs::write(root.join("MODELOS/303 3T 2024.txt"), m303).unwrap();
    // Dos recibos de una web extranjera sin fecha (la lectura la deja en null), en las carpetas
    // del 3T (declarado) y del 4T (sin declarar).
    for t in ["T3", "T4"] {
        let p = root.join(format!("FACTURAS/2024/{t}/GASTOS"));
        std::fs::create_dir_all(&p).unwrap();
        std::fs::write(
            p.join("recibo-web.pdf"),
            format!("%PDF-1.4 recibo {t}").as_bytes(),
        )
        .unwrap();
    }
    let lectura = |t: &str, n: &str| {
        format!(
            r#"{{"fichero":"FACTURAS/2024/{t}/GASTOS/recibo-web.pdf","lectura":{{
               "tipo":"factura_recibida","factura":{{"nombre_emisor":"Dominios Ejemplo Inc",
               "numero":"{n}","fecha":null,"base":"9.00","iva":"0","total":"9.00",
               "moneda":"USD"}}}}}}"#
        )
    };
    std::fs::write(
        root.join("taxpain-lecturas.json"),
        format!(
            r#"{{"lecturas":[{},{}]}}"#,
            lectura("T3", "WEB-1"),
            lectura("T4", "WEB-2")
        ),
    )
    .unwrap();
    let inv = carpeta::scan(&v, root).unwrap();
    // Por el 303 de la carpeta.
    let t3 = item(&inv, "2024/T3/GASTOS/recibo-web.pdf");
    assert!(!t3.importable, "{t3:?}");
    assert!(
        matches!(&t3.found, Found::Ia { detalle, .. } if detalle.contains("ya declaró el gestor")),
        "{t3:?}"
    );
    assert!(t3.selectable, "se puede marcar a mano: {t3:?}");
    assert!(item(&inv, "2024/T4/GASTOS/recibo-web.pdf").importable);
    // Con el 303 ya en TaxPain (y no en la carpeta), igual.
    let out = carpeta::apply(&v, root, &["MODELOS/303 3T 2024.txt".into()], now).unwrap();
    assert!(out[0].ok, "{out:?}");
    std::fs::remove_dir_all(root.join("MODELOS")).unwrap();
    let inv = carpeta::scan(&v, root).unwrap();
    assert!(!item(&inv, "2024/T3/GASTOS/recibo-web.pdf").importable);
    assert!(item(&inv, "2024/T4/GASTOS/recibo-web.pdf").importable);
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
