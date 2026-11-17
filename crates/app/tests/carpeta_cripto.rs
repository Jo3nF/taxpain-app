//! Carpeta de la empresa con criptomonedas: el ledgers de Kraken suelto o dentro de un zip (con
//! otro zip dentro), una hoja de saldos por fecha como la de CoinTracking, una hoja de
//! operaciones propia y lo que no se puede leer. Datos inventados.

#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use taxpain_app::carpeta::{self, Found};
use taxpain_app::cripto;
use taxpain_core::calendar::TaxpayerProfile;
use taxpain_core::cripto::Qty;
use taxpain_core::money::Money;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::datetime;

const NOW: time::OffsetDateTime = datetime!(2026-11-02 10:00 UTC);

fn vault() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    v.set_setting("taxpayer_profile", &TaxpayerProfile::sociedad_limitada())
        .unwrap();
    v
}

/// Ledgers de Kraken: dos compras de BTC (0,75 BTC por 30.000 €, a 40.000 €/BTC de coste medio),
/// un cierre con margen (euros contra 0 BTC, que no es una compraventa) y una venta de 0,5 BTC
/// por 30.000 € en 2025.
const LEDGERS: &str = "\"txid\",\"refid\",\"time\",\"type\",\"subtype\",\"aclass\",\"asset\",\"amount\",\"fee\",\"balance\"
\"LA1\",\"D1\",\"2024-01-09 10:00:00\",\"deposit\",\"\",\"currency\",\"ZEUR\",30000.0000,0.0000,30000.0000
\"LA2\",\"T1\",\"2024-01-10 10:00:00\",\"trade\",\"\",\"currency\",\"ZEUR\",-20000.0000,0.0000,10000.0000
\"LA3\",\"T1\",\"2024-01-10 10:00:00\",\"trade\",\"\",\"currency\",\"XXBT\",0.5000000000,0.0000000000,0.5000000000
\"LA4\",\"T2\",\"2024-03-01 10:00:00\",\"trade\",\"\",\"currency\",\"ZEUR\",-10000.0000,0.0000,0.0000
\"LA5\",\"T2\",\"2024-03-01 10:00:00\",\"trade\",\"\",\"currency\",\"XXBT\",0.2500000000,0.0000000000,0.7500000000
\"LA6\",\"M1\",\"2024-04-01 10:00:00\",\"margin\",\"\",\"currency\",\"ZEUR\",12.5000,0.0000,12.5000
\"LA7\",\"M1\",\"2024-04-01 10:00:00\",\"margin\",\"\",\"currency\",\"XXBT\",0.0000000000,0.0001000000,0.7500000000
\"LA8\",\"M1\",\"2024-04-01 14:00:00\",\"rollover\",\"\",\"currency\",\"ZEUR\",0.0000,0.2000,12.3000
\"LA9\",\"T3\",\"2025-10-13 10:00:00\",\"trade\",\"\",\"currency\",\"XXBT\",-0.5000000000,0.0000000000,0.2500000000
\"LB1\",\"T3\",\"2025-10-13 10:00:00\",\"trade\",\"\",\"currency\",\"ZEUR\",30000.0000,0.0000,30012.3000
";

/// Hoja de control como la de la gestoría: una primera hoja hecha a mano (que no es nada) y otra
/// con los saldos por fecha de CoinTracking («BTC Valor en EUR», «BTC Cantidad»).
fn saldos_xlsx() -> Vec<u8> {
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet().set_name("Cuentas corrientes").unwrap();
    ws.write_string(3, 2, "ENTRADAS").unwrap();
    ws.write_string(3, 3, "SALIDAS").unwrap();
    ws.write_number(4, 2, 30000.0).unwrap();
    let ws = wb.add_worksheet().set_name("Datos").unwrap();
    for (j, h) in ["Fecha", "BTC Valor en EUR", "BTC Cantidad"]
        .iter()
        .enumerate()
    {
        ws.write_string(1, u16::try_from(j).unwrap(), *h).unwrap();
    }
    ws.write_string(2, 0, "now").unwrap();
    ws.write_string(3, 0, "31.12.2025").unwrap();
    ws.write_number(3, 1, 20000.0).unwrap();
    ws.write_number(3, 2, 0.25).unwrap();
    ws.write_string(4, 0, "31.12.2024").unwrap();
    ws.write_number(4, 1, 67500.0).unwrap();
    ws.write_number(4, 2, 0.75).unwrap();
    wb.save_to_buffer().unwrap()
}

fn zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut z = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    for (name, bytes) in files {
        z.start_file(*name, opts).unwrap();
        z.write_all(bytes).unwrap();
    }
    z.finish().unwrap().into_inner()
}

fn write(root: &Path, rel: &str, bytes: &[u8]) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, bytes).unwrap();
}

fn item<'a>(inv: &'a carpeta::Inventory, path: &str) -> &'a carpeta::Item {
    inv.items
        .iter()
        .find(|i| i.path == path)
        .unwrap_or_else(|| panic!("no está {path}: {:#?}", inv.items))
}

#[test]
fn la_carpeta_propone_las_criptos_y_al_importarlas_salen_en_criptomonedas() {
    let v = vault();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "05_CRIPTO/control criptos.xlsx", &saldos_xlsx());
    // El ledgers dentro de un zip, otra vez dentro de un zip de dentro (se lee una vez), con
    // un PDF y un historial que no se sabe leer.
    let interior = zip(&[("ledgers.csv", LEDGERS.as_bytes())]);
    write(
        root,
        "Inspección/DOCUMENTACION.zip",
        &zip(&[
            ("CRYPTO/KRAKEN/ledgers.csv", LEDGERS.as_bytes()),
            ("CRYPTO/KRAKEN/ledgers 2024-2025.zip", &interior),
            ("CRYPTO/explicacion.pdf", b"%PDF-1.4 nada"),
            (
                "CRYPTO/BYBIT/Trade History Inverse Contract.csv",
                b"Contracts,Filled Type,Quantity,Exec price,Order Price,Direction,Transaction Time\n\
                  BTCUSD,Trade,100,40000,40000,Buy,2024-05-01 10:00:00 UTC\n",
            ),
            ("__MACOSX/._ledgers.csv", b"basura"),
        ]),
    );
    // Un zip sin nada de cripto.
    write(
        root,
        "Inspección/BYBIT.zip",
        &zip(&[(
            "BYBIT/Withdrawal History.csv",
            b"Coin,Amount,Transaction Fee,Wallet Address,tx_id,Status,Time\n\
              BTC,0.01,0,bc1qinventada,abc,Success,2024-02-09 14:18:38 UTC\n",
        )]),
    );
    // Un inventario con fecha, cantidad y valor no es de cripto.
    write(
        root,
        "inventario.csv",
        b"Fecha;Articulo;Cantidad;Valor\n31/12/2025;Tornillos;100;250,00\n",
    );

    let inv = carpeta::scan(&v, root).unwrap();
    let saldos = item(&inv, "05_CRIPTO/control criptos.xlsx");
    match &saldos.found {
        Found::Cripto { ops, prices, .. } => assert_eq!((*ops, *prices), (0, 2)),
        f => panic!("{f:?}"),
    }
    assert!(saldos.importable);
    let doc = item(&inv, "Inspección/DOCUMENTACION.zip");
    match &doc.found {
        Found::Cripto {
            ops,
            duplicates,
            unread,
            first,
            last,
            ..
        } => {
            // 3 operaciones (dos compras y una venta): el cierre con margen y los intereses no
            // son compraventas, y el mismo ledgers del zip de dentro no se cuenta dos veces.
            assert_eq!((*ops, *duplicates), (3, 0));
            assert_eq!(*unread, 3, "{:#?}", doc.warnings);
            assert_eq!(first.map(|d| d.year()), Some(2024));
            assert_eq!(last.map(|d| d.year()), Some(2025));
        }
        f => panic!("{f:?}"),
    }
    assert!(doc.importable);
    assert_eq!(doc.year, Some(2025));
    assert!(
        doc.warnings
            .iter()
            .any(|w| w.contains("Inverse Contract.csv") && w.contains("derivados")),
        "{:#?}",
        doc.warnings
    );
    assert!(
        doc.warnings.iter().any(|w| w.contains("mismo fichero")),
        "{:#?}",
        doc.warnings
    );
    let bybit = item(&inv, "Inspección/BYBIT.zip");
    match &bybit.found {
        Found::Otro { reason } => {
            assert!(reason.contains("no se ha podido leer"), "{reason}");
            assert!(reason.contains("retiradas de Bybit"), "{reason}");
        }
        f => panic!("{f:?}"),
    }
    assert!(!bybit.importable && !bybit.selectable);
    assert!(
        !matches!(item(&inv, "inventario.csv").found, Found::Cripto { .. }),
        "{:?}",
        item(&inv, "inventario.csv").found
    );

    let paths: Vec<String> = inv
        .items
        .iter()
        .filter(|i| i.importable)
        .map(|i| i.path.clone())
        .collect();
    let out = carpeta::apply(&v, root, &paths, NOW).unwrap();
    let linea = |p: &str| out.iter().find(|o| o.path == p).unwrap();
    let o = linea("Inspección/DOCUMENTACION.zip");
    assert!(o.ok, "{}", o.message);
    assert!(
        o.message
            .starts_with("3 operaciones de cripto a Criptomonedas"),
        "{}",
        o.message
    );
    assert!(o.message.contains("no se ha podido leer"), "{}", o.message);
    let o = linea("05_CRIPTO/control criptos.xlsx");
    assert_eq!(o.message, "2 precios de cierre a Criptomonedas");

    // La pantalla de Criptomonedas: coste medio ponderado de 40.000 €/BTC, venta de 0,5 BTC por
    // 30.000 € (10.000 € de ganancia) y 0,25 BTC a 31/12/2025 valorados a 80.000 €/BTC.
    let r = cripto::report(&v, 2025).unwrap();
    assert_eq!(r.ops_ejercicio, 1);
    assert_eq!(r.resumen_pmp.neto, Money::from_euros(10_000, 0));
    let btc: Vec<_> = r
        .posiciones
        .iter()
        .filter(|p| p.ticker == "BTC" && !p.cantidad.is_zero())
        .collect();
    assert_eq!(btc.len(), 1);
    assert_eq!(btc[0].platform, "Kraken");
    assert_eq!(btc[0].cantidad, Qty::parse("0.25").unwrap());
    assert_eq!(btc[0].coste, Money::from_euros(10_000, 0));
    assert_eq!(btc[0].valor, Some(Money::from_euros(20_000, 0)));

    // Leída otra vez, ya no se propone: las operaciones ya están.
    let inv = carpeta::scan(&v, root).unwrap();
    let doc = item(&inv, "Inspección/DOCUMENTACION.zip");
    assert!(
        matches!(
            doc.found,
            Found::Cripto {
                ops: 3,
                duplicates: 3,
                ..
            }
        ),
        "{:?}",
        doc.found
    );
    assert!(!doc.importable && doc.selectable);
    let out = carpeta::apply(&v, root, std::slice::from_ref(&doc.path), NOW).unwrap();
    assert!(
        out[0]
            .message
            .starts_with("0 operaciones de cripto, 3 ya estaban"),
        "{}",
        out[0].message
    );
    assert_eq!(cripto::ops(&v, None).unwrap().len(), 3);
}

#[test]
fn una_hoja_de_operaciones_propia_y_el_ledgers_suelto() {
    let v = vault();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "cripto/operaciones.csv",
        "Fecha;Tipo;Activo;Cantidad;Euros;Comisión;Plataforma\n\
         10/01/2025;Compra;BTC;0,5;20.000,00;10,00;Bit2Me\n\
         15/06/2025;Venta;BTC;0,2;12.000,00;;Bit2Me\n"
            .as_bytes(),
    );
    write(root, "cripto/kraken/ledgers.csv", LEDGERS.as_bytes());
    let inv = carpeta::scan(&v, root).unwrap();
    let hoja = item(&inv, "cripto/operaciones.csv");
    assert!(
        matches!(&hoja.found, Found::Cripto { ops: 2, resumen, .. } if resumen.starts_with("hoja de operaciones")),
        "{:?}",
        hoja.found
    );
    assert!(
        matches!(
            item(&inv, "cripto/kraken/ledgers.csv").found,
            Found::Cripto { ops: 3, .. }
        ),
        "{:?}",
        item(&inv, "cripto/kraken/ledgers.csv").found
    );
    let paths: Vec<String> = inv.items.iter().map(|i| i.path.clone()).collect();
    let out = carpeta::apply(&v, root, &paths, NOW).unwrap();
    assert!(out.iter().all(|o| o.ok), "{out:#?}");
    assert_eq!(cripto::ops(&v, None).unwrap().len(), 5);
    let nombres: Vec<String> = cripto::platforms(&v)
        .unwrap()
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert!(nombres.contains(&"Bit2Me".to_owned()) && nombres.contains(&"Kraken".to_owned()));
}
