//! Extracto camt.053: importación sin duplicados y el IBAN de cada cuenta para las remesas.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::bank::{import_statement, movements};
use taxpain_app::remesas;
use taxpain_core::money::Money;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::datetime;

const CAMT: &[u8] = include_bytes!("../../../fixtures/bank/camt053-ejemplo.xml");
const N43: &[u8] = include_bytes!("../../../fixtures/accounting/n43-ejemplo.txt");

#[test]
fn camt053_y_norma43_son_la_misma_cuenta() {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let now = datetime!(2026-10-20 10:00 UTC);
    let s = import_statement(&v, CAMT, now).unwrap();
    assert_eq!((s.accounts, s.new_movements, s.duplicates), (1, 2, 0));
    let again = import_statement(&v, CAMT, now).unwrap();
    assert_eq!((again.new_movements, again.duplicates), (0, 2));
    let m = movements(&v, true).unwrap();
    assert!(m.iter().any(|x| x.amount == Money::from_euros(1210, 0)
        && x.description == "FRA 2026-0042 CLIENTE EJEMPLO SA"));
    // Otra cuenta por Norma 43: las dos salen con su IBAN para las remesas (el de la Norma 43,
    // calculando sus dígitos de control).
    import_statement(&v, N43, now).unwrap();
    let banks = remesas::banks(&v).unwrap();
    assert_eq!(banks.len(), 2);
    assert_eq!(banks[0].iban, "ES9121000418450200051332");
    assert!(banks[1].iban.starts_with("ES") && banks[1].iban.ends_with("0000123456"));
    assert!(taxpain_core::sepa::normalize_iban(&banks[1].iban).is_ok());
}

#[test]
fn un_camt_que_no_cuadra_no_entra_salvo_que_se_fuerce() {
    use taxpain_app::extractos::Cuadre;
    use taxpain_app::extractos::importar::importar_fichero;

    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let now = datetime!(2026-10-20 10:00 UTC);
    // El saldo final (CLBD) no encadena: 1000 + 1210 − 78,60 = 2131,40, pero dice 2000,00. El
    // lector de camt no lo comprueba, así que se queda sin importar con su motivo (y el botón de
    // «Importar igualmente») en vez de fallar entero.
    let malo = String::from_utf8_lossy(CAMT).replacen("2131.40", "2000.00", 1);
    let r = importar_fichero(&v, "extracto.xml", malo.as_bytes(), None, false, now).unwrap();
    assert_eq!(r.extractos.len(), 1);
    let e = &r.extractos[0];
    assert!(!e.importado);
    assert!(matches!(e.cuadre, Cuadre::NoCuadra { .. }));
    assert!(e.motivo.contains("no cuadra"), "{}", e.motivo);
    assert!(movements(&v, false).unwrap().is_empty());
    // Forzado, entra con su aviso.
    let r = importar_fichero(&v, "extracto.xml", malo.as_bytes(), None, true, now).unwrap();
    assert!(r.extractos[0].importado);
    assert_eq!((r.movimientos_nuevos, r.ya_estaban), (2, 0));
    assert!(
        r.extractos[0]
            .avisos
            .iter()
            .any(|a| a.contains("aunque no cuadra"))
    );
    assert_eq!(movements(&v, false).unwrap().len(), 2);
}
