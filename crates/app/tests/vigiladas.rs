//! Carpetas vigiladas: varias por empresa, con subcarpetas; las facturas recibidas entran solas y
//! lo demás queda como aviso para Importar. Datos inventados.

#![allow(clippy::unwrap_used)]

use std::path::Path;
use std::sync::Arc;

use taxpain_app::companies::{self, CompanyInfo};
use taxpain_app::received;
use taxpain_app::vigiladas::{self, OLD_KEY};
use taxpain_invoicing::pdf::{Para, text_document};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::OffsetDateTime;
use time::macros::datetime;

const AHORA: OffsetDateTime = datetime!(2026-11-12 10:00 UTC);
const N43: &[u8] = include_bytes!("../../../fixtures/accounting/n43-ejemplo.txt");

fn vault() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let info = CompanyInfo {
        name: "Empresa Demo SL".into(),
        nif: Some("B98765431".into()),
    };
    companies::set_info(&v, &info, AHORA).unwrap();
    v
}

fn factura(numero: &str) -> Vec<u8> {
    let doc = vec![
        Para::Bold("Papelería Ejemplo SA".into()),
        Para::Text("NIF A22222228".into()),
        Para::Text(format!("Factura nº {numero}")),
        Para::Text("Fecha 15/09/2026".into()),
        Para::Text("Base imponible 100,00".into()),
        Para::Text("IVA 21% 21,00".into()),
        Para::Text("Total 121,00 €".into()),
    ];
    text_document("Factura", &[doc]).unwrap()
}

/// Escribe un fichero con fecha de hace una hora: los recién escritos (Drive puede estar aún
/// bajándolos) se dejan para la pasada siguiente.
fn escribir(root: &Path, ruta: &str, bytes: &[u8]) {
    let p = root.join(ruta);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(&p, bytes).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&p)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .unwrap();
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[test]
fn facturas_solas_y_lo_demas_como_aviso() {
    let v = vault();
    let drive = tempfile::tempdir().unwrap();
    let escritorio = tempfile::tempdir().unwrap();
    escribir(drive.path(), "2026/3T/Facturas/p-90.pdf", &factura("P-90"));
    escribir(drive.path(), "2026/3T/Extractos/banco.n43", N43);
    escribir(escritorio.path(), "p-91.pdf", &factura("P-91"));
    let list = vigiladas::set_folders(&v, &[s(drive.path()), s(escritorio.path())]).unwrap();
    assert_eq!(list.len(), 2);

    let pass = vigiladas::scan(&v, AHORA).unwrap();
    assert_eq!(pass.invoices, 2, "{pass:?}");
    assert_eq!(pass.pending, 1, "{pass:?}");
    assert!(pass.errors.is_empty(), "{pass:?}");
    // Las facturas, en Recibidas pendientes de revisar (sin contabilizar).
    let recibidas = received::list_received(&v, None).unwrap();
    assert_eq!(recibidas.len(), 2);
    assert!(recibidas.iter().all(|r| r.status != "registrada"));
    // El extracto, como aviso: no se importa solo.
    let avisos = vigiladas::pending(&v).unwrap();
    assert_eq!(avisos.len(), 1);
    assert_eq!(avisos[0].folder, s(drive.path()));
    assert_eq!(avisos[0].path, "2026/3T/Extractos/banco.n43");
    assert_eq!(avisos[0].kind, "Extracto bancario");
    assert_eq!(avisos[0].seen, "2026-11-12");

    // La pasada siguiente no vuelve a mirar lo visto ni repite avisos.
    let pass = vigiladas::scan(&v, AHORA).unwrap();
    assert_eq!((pass.invoices, pass.pending), (0, 0));
    assert_eq!(vigiladas::pending(&v).unwrap().len(), 1);

    // Lo nuevo de una subcarpeta nueva, sí.
    let p95 = factura("P-95");
    escribir(drive.path(), "2026/4T/Facturas/p-95.pdf", &p95);
    let pass = vigiladas::scan(&v, AHORA).unwrap();
    assert_eq!((pass.invoices, pass.pending), (1, 0));
    // La misma factura copiada en otra carpeta no se duplica.
    escribir(escritorio.path(), "copia/p-95.pdf", &p95);
    let pass = vigiladas::scan(&v, AHORA).unwrap();
    assert_eq!(pass.invoices, 0);
    assert_eq!(received::list_received(&v, None).unwrap().len(), 3);

    // Mirar la carpeta en Importar quita sus avisos.
    vigiladas::dismiss_under(&v, &s(&drive.path().join("2026"))).unwrap();
    assert!(vigiladas::pending(&v).unwrap().is_empty());
}

#[test]
fn lista_de_carpetas() {
    let v = vault();
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    // La carpeta de antes (una sola) sigue vigilada.
    v.set_setting(OLD_KEY, &Some(s(a.path()))).unwrap();
    assert_eq!(vigiladas::folders(&v).unwrap(), [s(a.path())]);
    // Sin repetir ni barras finales; una que no existe, error.
    let list = vigiladas::set_folders(&v, &[s(a.path()), format!("{}/", s(a.path())), s(b.path())])
        .unwrap();
    assert_eq!(list, [s(a.path()), s(b.path())]);
    assert!(vigiladas::set_folders(&v, &["/no/existe/nunca".into()]).is_err());
    assert_eq!(vigiladas::folders(&v).unwrap().len(), 2);

    // Quitar una carpeta olvida sus avisos.
    escribir(b.path(), "banco.n43", N43);
    vigiladas::scan(&v, AHORA).unwrap();
    assert_eq!(vigiladas::pending(&v).unwrap().len(), 1);
    vigiladas::set_folders(&v, &[s(a.path())]).unwrap();
    assert!(vigiladas::pending(&v).unwrap().is_empty());
    // Volver a añadirla lo mira otra vez desde cero.
    vigiladas::set_folders(&v, &[s(a.path()), s(b.path())]).unwrap();
    assert_eq!(vigiladas::scan(&v, AHORA).unwrap().pending, 1);
    vigiladas::dismiss(&v, None).unwrap();
    assert!(vigiladas::pending(&v).unwrap().is_empty());

    // Una carpeta que ya no está (Drive sin montar) se avisa y no rompe las demás.
    let c = tempfile::tempdir().unwrap();
    vigiladas::set_folders(&v, &[s(c.path()), s(a.path())]).unwrap();
    drop(c);
    let pass = vigiladas::scan(&v, AHORA).unwrap();
    assert_eq!(pass.errors.len(), 1, "{pass:?}");
}

#[test]
fn una_hoja_de_calculo_enorme_no_tumba_la_pasada_ni_se_vuelve_a_leer() {
    // Una hoja con A1 y XFD1048576: de pocos KB, pero calamine reservaría 2^34 celdas.
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet();
    ws.write_string(0, 0, "Fecha").unwrap();
    ws.write_string(1_048_575, 16_383, "fin").unwrap();
    let bytes = wb.save_to_buffer().unwrap();
    let v = vault();
    let drive = tempfile::tempdir().unwrap();
    escribir(drive.path(), "adjuntos/extracto.xlsx", &bytes);
    escribir(drive.path(), "adjuntos/banco.n43", N43);
    vigiladas::set_folders(&v, &[s(drive.path())]).unwrap();

    let t = std::time::Instant::now();
    let pass = vigiladas::scan(&v, AHORA).unwrap();
    assert!(
        t.elapsed()
            < std::time::Duration::from_secs(
                30 * std::env::var("TAXPAIN_TEST_HOLGURA")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1)
            )
    );
    // El extracto bueno, como siempre; la hoja enorme se rechaza rápido y queda como vista (no
    // se reconoce: no es un documento que importar).
    assert_eq!(pass.pending, 1, "{pass:?}");
    let avisos = vigiladas::pending(&v).unwrap();
    assert_eq!(avisos.len(), 1, "{avisos:?}");
    assert_eq!(avisos[0].path, "adjuntos/banco.n43");
    assert_eq!(
        pass.unreadable, 0,
        "una hoja que no se entiende no es un fichero roto: {pass:?}"
    );
    assert_eq!(
        vigiladas::scan(&v, AHORA).unwrap(),
        vigiladas::Pass::default()
    );
}
