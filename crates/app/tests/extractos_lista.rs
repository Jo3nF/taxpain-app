//! Extractos importados (G36): la lista por cuenta con su fichero, periodo y saldos, los huecos
//! entre extractos, deshacer uno importado por error (con sus conciliaciones) y el contravalor
//! en euros que traen los extractos de Wise y Revolut Business. Datos inventados.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::bank::{movements, reconcile_account};
use taxpain_app::extractos::importar::{Resumen, comprobado, importar, importar_en_cuenta};
use taxpain_app::extractos::{Clase, Extracto, Movimiento, leer};
use taxpain_app::extractos_importados::{deshacer, lista};
use taxpain_core::money::Money;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime};

const IBAN_ES: &str = "ES9121000418450200051332";
const IBAN_GB: &str = "GB82WEST12345698765432";
const AHORA: OffsetDateTime = datetime!(2026-10-25 10:00 UTC);

fn vault() -> Vault {
    Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap()
}

fn m(fecha: Date, concepto: &str, importe: i64, saldo: i64) -> Movimiento {
    Movimiento {
        importe_eur: None,
        fecha,
        fecha_valor: None,
        concepto: concepto.into(),
        importe: Money::from_cents(importe),
        saldo: Some(Money::from_cents(saldo)),
        referencia: String::new(),
    }
}

fn extracto(iban: &str, divisa: &str, movimientos: Vec<Movimiento>) -> Extracto {
    Extracto {
        formato: "Hoja de cálculo".into(),
        banco: "Banco Demo".into(),
        iban: Some(iban.into()),
        cuenta: String::new(),
        titular: "ESTUDIO DEMO SL".into(),
        divisa: divisa.into(),
        clase: Clase::Cuenta,
        desde: None,
        hasta: None,
        saldo_inicial: None,
        saldo_final: None,
        movimientos,
        avisos: Vec::new(),
    }
}

fn imp(v: &Vault, nombre: &str, e: Extracto) -> Resumen {
    let r = importar(v, nombre, nombre.as_bytes(), &[comprobado(e)], false, AHORA).unwrap();
    assert!(r.extractos.iter().all(|x| x.importado), "{r:?}");
    r
}

fn enero() -> Vec<Movimiento> {
    vec![
        m(date!(2026 - 01 - 05), "CLIENTE DEMO", 50_000, 150_000),
        m(date!(2026 - 01 - 20), "PROVEEDOR DEMO", -20_000, 130_000),
    ]
}

#[test]
fn lista_de_extractos_con_sus_saldos_y_los_huecos() {
    let v = vault();
    let r = imp(&v, "enero.xlsx", extracto(IBAN_ES, "EUR", enero()));
    assert!(r.extractos[0].extracto_id.is_some());
    // Febrero no está y marzo empieza con un saldo que no enlaza: faltan movimientos.
    imp(
        &v,
        "marzo.xlsx",
        extracto(
            IBAN_ES,
            "EUR",
            vec![
                m(date!(2026 - 03 - 02), "RECIBO DEMO", -5_000, 120_000),
                m(date!(2026 - 03 - 30), "CLIENTE DEMO", 10_000, 130_000),
            ],
        ),
    );
    let l = lista(&v, None).unwrap();
    assert_eq!(l.extractos.len(), 2);
    let e = &l.extractos[0];
    assert_eq!(e.fichero, "enero.xlsx");
    assert_eq!(e.desde, Some(date!(2026 - 01 - 05)));
    assert_eq!(e.hasta, Some(date!(2026 - 01 - 20)));
    assert_eq!(e.saldo_inicial, Some(Money::from_cents(100_000)));
    assert_eq!(e.saldo_final, Some(Money::from_cents(130_000)));
    assert_eq!((e.movimientos, e.nuevos, e.propios), (2, 2, 2));
    assert_eq!(l.huecos.len(), 1, "{:?}", l.huecos);
    let h = &l.huecos[0];
    assert!(h.seguro);
    assert_eq!(h.desde, date!(2026 - 01 - 21));
    assert_eq!(h.hasta, date!(2026 - 03 - 01));
    assert!(h.detalle.contains("1.250,00"), "{}", h.detalle);

    // Con el de febrero, que enlaza los saldos, ya no hay hueco.
    imp(
        &v,
        "febrero.xlsx",
        extracto(
            IBAN_ES,
            "EUR",
            vec![m(date!(2026 - 02 - 10), "RECIBO DEMO", -5_000, 125_000)],
        ),
    );
    let l = lista(&v, None).unwrap();
    assert!(l.huecos.is_empty(), "{:?}", l.huecos);
    // Volver a importar el mismo fichero no repite el extracto.
    imp(&v, "enero.xlsx", extracto(IBAN_ES, "EUR", enero()));
    assert_eq!(lista(&v, None).unwrap().extractos.len(), 3);
}

#[test]
fn deshacer_un_extracto_borra_sus_movimientos_y_deja_los_que_trae_otro() {
    let v = vault();
    imp(&v, "enero.xlsx", extracto(IBAN_ES, "EUR", enero()));
    // Un extracto que se solapa: trae el del día 20 (que ya estaba) y uno nuevo.
    let r = imp(
        &v,
        "enero-bis.xlsx",
        extracto(
            IBAN_ES,
            "EUR",
            vec![
                m(date!(2026 - 01 - 20), "PROVEEDOR DEMO", -20_000, 130_000),
                m(date!(2026 - 01 - 25), "RECIBO DEMO", -1_000, 129_000),
            ],
        ),
    );
    let bis = r.extractos[0].extracto_id.unwrap();
    let l = lista(&v, None).unwrap();
    let primero = l
        .extractos
        .iter()
        .find(|e| e.fichero == "enero.xlsx")
        .unwrap();
    assert_eq!(primero.propios, 1);
    let segundo = l.extractos.iter().find(|e| e.id == bis).unwrap();
    assert_eq!((segundo.nuevos, segundo.propios), (1, 1));

    // Sin confirmar, nada.
    assert!(deshacer(&v, bis, false, AHORA, false).is_err());
    // Conciliado, sólo si se pide desconciliar.
    let recibo = movements(&v, false)
        .unwrap()
        .into_iter()
        .find(|t| t.amount.cents() == -1_000)
        .unwrap();
    reconcile_account(&v, recibo.id, "629", "Recibo", AHORA).unwrap();
    let err = deshacer(&v, bis, false, AHORA, true).unwrap_err();
    assert!(err.to_string().contains("conciliado"), "{err}");
    assert_eq!(
        lista(&v, None)
            .unwrap()
            .extractos
            .iter()
            .find(|e| e.id == bis)
            .unwrap()
            .conciliados,
        1
    );
    let d = deshacer(&v, bis, true, AHORA, true).unwrap();
    assert_eq!((d.borrados, d.desconciliados, d.compartidos), (1, 1, 1));
    // Quedan los dos de enero; el del 20 sigue (lo trae el primero) y el asiento se ha ido.
    let quedan = movements(&v, false).unwrap();
    assert_eq!(quedan.len(), 2);
    assert!(quedan.iter().all(|t| t.matched.is_none()));
    let l = lista(&v, None).unwrap();
    assert_eq!(l.extractos.len(), 1);
    assert_eq!(l.extractos[0].propios, 2);
}

/// Wise: los euros salen de «Exchange To Amount» (pago con tarjeta en euros desde la cuenta en
/// dólares) y del importe entre el tipo si se cambió desde euros.
const WISE: &str = "\"TransferWise ID\",Date,Amount,Currency,Description,\"Payment Reference\",\
\"Running Balance\",\"Exchange From\",\"Exchange To\",\"Exchange Rate\",\"Payer Name\",\
\"Payee Name\",\"Payee Account Number\",Merchant,\"Total fees\",\"Exchange To Amount\"
CARD-2,15-09-2026,-108.50,USD,\"Card transaction of 100.00 EUR issued by Demo Shop\",,891.50,USD,EUR,0.92166,,,,\"Demo Shop\",0.00,100.00
TRANSFER-1,10-09-2026,1000.00,USD,\"Converted EUR to USD\",,1000.00,EUR,USD,1.08500,,,,,0.00,1000.00
";

/// Revolut Business: los euros, en «Orig amount» cuando «Orig currency» es el euro.
const REVOLUT: &str = "Date started (UTC),Date completed (UTC),ID,Type,State,Description,\
Reference,Orig currency,Orig amount,Payment currency,Amount,Fee,Balance
2026-09-20,2026-09-20,rb-2,CARD_PAYMENT,COMPLETED,Demo Hotel,,EUR,-50.00,USD,-54.20,0.00,945.80
2026-09-18,2026-09-18,rb-1,TOPUP,COMPLETED,Cliente Demo,,USD,1000.00,USD,1000.00,0.00,1000.00
";

#[test]
fn los_extractos_de_wise_y_revolut_traen_los_euros() {
    let w = leer("statement_USD.csv", WISE.as_bytes()).unwrap();
    assert_eq!(w.len(), 1);
    let e = &w[0].extracto;
    assert_eq!(e.divisa, "USD");
    let euros: Vec<(i64, Option<i64>)> = e
        .movimientos
        .iter()
        .map(|m| (m.importe.cents(), m.importe_eur.map(Money::cents)))
        .collect();
    assert_eq!(euros, [(100_000, Some(92_166)), (-10_850, Some(-10_000))]);

    let r = leer("revolut-business.csv", REVOLUT.as_bytes()).unwrap();
    let e = &r[0].extracto;
    assert_eq!(e.divisa, "USD");
    let euros: Vec<(i64, Option<i64>)> = e
        .movimientos
        .iter()
        .map(|m| (m.importe.cents(), m.importe_eur.map(Money::cents)))
        .collect();
    assert_eq!(euros, [(100_000, None), (-5_420, Some(-5_000))]);
}

#[test]
fn el_contravalor_del_extracto_se_guarda_al_importar() {
    let v = vault();
    // La cuenta en dólares, de un extracto anterior.
    imp(
        &v,
        "agosto.xlsx",
        extracto(
            IBAN_GB,
            "USD",
            vec![m(date!(2026 - 08 - 31), "INGRESO DEMO", 1_000, 1_000)],
        ),
    );
    let cuenta = lista(&v, None).unwrap().extractos[0].cuenta_id;
    let leidos = leer("statement_USD.csv", WISE.as_bytes()).unwrap();
    let r = importar_en_cuenta(
        &v,
        "statement_USD.csv",
        WISE.as_bytes(),
        &leidos,
        cuenta,
        false,
        AHORA,
    )
    .unwrap();
    assert_eq!(r.extractos[0].con_contravalor, 2, "{r:?}");
    assert!(
        r.extractos[0]
            .avisos
            .iter()
            .any(|a| a.contains("euros que da el propio extracto"))
    );
    let movs = movements(&v, false).unwrap();
    let tarjeta = movs.iter().find(|t| t.amount.cents() == -10_850).unwrap();
    assert_eq!(tarjeta.amount_eur, Some(Money::from_cents(-10_000)));
    assert_eq!(tarjeta.fx_source.as_deref(), Some("manual"));
    let cambio = movs.iter().find(|t| t.amount.cents() == 100_000).unwrap();
    assert_eq!(cambio.amount_eur, Some(Money::from_cents(92_166)));
    // Se contabiliza con esos euros.
    reconcile_account(&v, tarjeta.id, "629", "Tienda demo", AHORA).unwrap();
}
