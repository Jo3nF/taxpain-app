//! Criptomonedas: operaciones, informe por los dos métodos, asientos que cuadran, deterioro,
//! 721 con precios de cierre, Renta con FIFO e importación con deduplicación.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::{close_year, journal, trial_balance};
use taxpain_app::cripto::{self, Naturaleza, Op, Platform, Price, Settings, asientos, importar};
use taxpain_app::umbrales;
use taxpain_core::accounting::Origin;
use taxpain_core::calendar::TaxpayerProfile;
use taxpain_core::cripto::{Metodo, Precio, Qty, Tipo};
use taxpain_core::money::Money;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime};

const NOW: OffsetDateTime = datetime!(2026-10-26 10:00 UTC);

fn m(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn q(s: &str) -> Qty {
    Qty::parse(s).unwrap()
}

fn vault(fisica: bool) -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    v.set_setting("taxpayer_profile", &profile(fisica)).unwrap();
    v
}

fn profile(fisica: bool) -> TaxpayerProfile {
    if fisica {
        TaxpayerProfile::autonomo()
    } else {
        TaxpayerProfile::sociedad_limitada()
    }
}

fn platform(v: &Vault, name: &str, abroad: bool) -> i64 {
    cripto::save_platform(
        v,
        &Platform {
            id: 0,
            name: name.into(),
            kind: "exchange".into(),
            country: if abroad { "US".into() } else { "ES".into() },
            abroad,
            account: String::new(),
        },
        NOW,
    )
    .unwrap()
}

fn op(date: Date, kind: Tipo, platform: i64, ticker: &str, qty: &str, eur: i64, fee: i64) -> Op {
    Op {
        id: 0,
        date,
        time: "10:00:00".into(),
        kind,
        platform_id: platform,
        to_platform_id: None,
        ticker: ticker.into(),
        qty: q(qty),
        ticker2: None,
        qty2: None,
        eur: m(eur),
        fee_eur: m(fee),
        ref_price: None,
        note: String::new(),
        source: "manual".into(),
        fingerprint: None,
        entry_id: None,
    }
}

fn saldo(v: &Vault, year: i32, prefix: &str) -> Money {
    trial_balance(v, year, None)
        .unwrap()
        .iter()
        .filter(|r| r.account.as_str().starts_with(prefix))
        .map(|r| r.debit - r.credit)
        .sum()
}

#[test]
fn informe_por_los_dos_metodos_y_asientos_de_existencias_que_cuadran() {
    let v = vault(false);
    let kraken = platform(&v, "Kraken", true);
    cripto::save_op(
        &v,
        &op(
            date!(2026 - 01 - 10),
            Tipo::Compra,
            kraken,
            "btc",
            "1",
            20_000,
            10,
        ),
        NOW,
    )
    .unwrap();
    cripto::save_op(
        &v,
        &op(
            date!(2026 - 02 - 10),
            Tipo::Compra,
            kraken,
            "BTC",
            "1",
            40_000,
            0,
        ),
        NOW,
    )
    .unwrap();
    cripto::save_op(
        &v,
        &op(
            date!(2026 - 03 - 10),
            Tipo::Venta,
            kraken,
            "BTC",
            "1",
            50_000,
            50,
        ),
        NOW,
    )
    .unwrap();
    let r = cripto::report(&v, 2026).unwrap();
    assert_eq!(r.metodo_declaracion, Metodo::Pmp);
    // PMP: coste (20.010 + 40.000) / 2 = 30.005; valor 49.950 → 19.945. FIFO: 49.950 − 20.010.
    assert_eq!(r.resumen_pmp.neto, Money::from_cents(1_994_500));
    assert_eq!(r.resumen_fifo.neto, Money::from_cents(2_994_000));
    assert_eq!(r.realizados.len(), 1);
    assert_eq!(r.realizados[0].coste, Money::from_cents(3_000_500));
    assert_eq!(r.posiciones[0].cantidad, q("1"));
    assert!(r.posiciones[0].valor.is_none(), "sin precio de cierre");
    assert!(r.avisos.iter().any(|a| a.contains("Sin precio de cierre")));
    assert_eq!(r.ops_sin_asiento, 3);

    // Asientos (existencias): cuadran y dejan el activo al coste de lo que queda.
    let c = asientos::contabilizar(&v, 2026, false, NOW).unwrap();
    assert_eq!(c.asientos, 3);
    let j = journal(&v, 2026).unwrap();
    assert_eq!(j.len(), 3);
    for e in &j {
        assert!(matches!(e.entry.origin, Origin::Cripto(_)));
        assert_eq!(e.entry.total_debit(), e.entry.total_credit());
    }
    assert_eq!(saldo(&v, 2026, "309"), Money::from_cents(3_000_500));
    assert_eq!(saldo(&v, 2026, "700"), m(-50_000));
    assert_eq!(saldo(&v, 2026, "610"), Money::from_cents(3_000_500));
    assert_eq!(saldo(&v, 2026, "626"), m(50));
    assert_eq!(saldo(&v, 2026, "572"), m(-10_060)); // −20.010 −40.000 +49.950
    let assets = cripto::assets(&v).unwrap();
    assert_eq!(
        (assets[0].ticker.as_str(), assets[0].account.as_str()),
        ("BTC", "3090001")
    );
    let r = cripto::report(&v, 2026).unwrap();
    assert_eq!((r.ops_con_asiento, r.ops_sin_asiento), (3, 0));

    // Rehacer tras cambiar el método: los asientos se regeneran con FIFO.
    cripto::save_settings(
        &v,
        &Settings {
            metodo: Metodo::Fifo,
            ..Settings::default()
        },
        NOW,
    )
    .unwrap();
    let c = asientos::contabilizar(&v, 2026, true, NOW).unwrap();
    assert_eq!((c.borrados, c.asientos), (3, 3));
    assert_eq!(saldo(&v, 2026, "610"), m(20_010));
    assert_eq!(saldo(&v, 2026, "309"), m(40_000));
}

#[test]
fn permuta_traspaso_recompensa_y_naturaleza_intangible() {
    let v = vault(false);
    cripto::save_settings(
        &v,
        &Settings {
            metodo: Metodo::Fifo,
            naturaleza: Naturaleza::Intangible,
            ..Settings::default()
        },
        NOW,
    )
    .unwrap();
    let kraken = platform(&v, "Kraken", true);
    let ledger = platform(&v, "Ledger", false);
    cripto::save_op(
        &v,
        &op(
            date!(2026 - 01 - 10),
            Tipo::Compra,
            kraken,
            "BTC",
            "1",
            40_000,
            0,
        ),
        NOW,
    )
    .unwrap();
    let mut permuta = op(
        date!(2026 - 02 - 01),
        Tipo::Permuta,
        kraken,
        "BTC",
        "0.5",
        30_000,
        20,
    );
    permuta.ticker2 = Some("ETH".into());
    permuta.qty2 = Some(q("10"));
    cripto::save_op(&v, &permuta, NOW).unwrap();
    let mut traspaso = op(
        date!(2026 - 03 - 01),
        Tipo::Traspaso,
        kraken,
        "BTC",
        "0.5",
        0,
        5,
    );
    traspaso.to_platform_id = Some(ledger);
    cripto::save_op(&v, &traspaso, NOW).unwrap();
    cripto::save_op(
        &v,
        &op(
            date!(2026 - 04 - 01),
            Tipo::Recompensa,
            kraken,
            "ETH",
            "0.1",
            300,
            0,
        ),
        NOW,
    )
    .unwrap();
    cripto::save_op(
        &v,
        &op(
            date!(2026 - 05 - 01),
            Tipo::Perdida,
            ledger,
            "BTC",
            "0.1",
            0,
            0,
        ),
        NOW,
    )
    .unwrap();

    let r = cripto::report(&v, 2026).unwrap();
    // Permuta: 30.000 − 20 − 20.000 = 9.980. Pérdida: −4.000. Traspaso: nada.
    assert_eq!(r.resumen_fifo.ganancias, m(9_980));
    assert_eq!(r.resumen_fifo.perdidas, m(4_000));
    assert_eq!(r.resumen_fifo.recompensas, m(300));
    assert_eq!(r.resumen_fifo.comisiones, m(5));
    assert_eq!(r.realizados.len(), 2);
    let btc_ledger = r.posiciones.iter().find(|p| p.ticker == "BTC").unwrap();
    assert_eq!(
        (
            btc_ledger.platform.as_str(),
            btc_ledger.cantidad,
            btc_ledger.coste,
            btc_ledger.abroad
        ),
        ("Ledger", q("0.4"), m(16_000), false)
    );
    let eth = r.posiciones.iter().find(|p| p.ticker == "ETH").unwrap();
    assert_eq!(
        (eth.cantidad, eth.coste, eth.abroad),
        (q("10.1"), m(30_300), true)
    );

    let c = asientos::contabilizar(&v, 2026, false, NOW).unwrap();
    assert_eq!(c.asientos, 5);
    for e in journal(&v, 2026).unwrap() {
        assert_eq!(e.entry.total_debit(), e.entry.total_credit());
    }
    assert_eq!(saldo(&v, 2026, "207"), m(46_300)); // 16.000 BTC + 30.300 ETH
    assert_eq!(saldo(&v, 2026, "771"), m(-10_000));
    assert_eq!(saldo(&v, 2026, "671"), m(4_000));
    assert_eq!(saldo(&v, 2026, "626"), m(25));
    assert_eq!(saldo(&v, 2026, "769"), m(-300));
}

#[test]
fn deterioro_al_cierre_solo_si_el_valor_baja_del_coste() {
    let v = vault(false);
    let p = platform(&v, "Bit2Me", false);
    cripto::save_op(
        &v,
        &op(
            date!(2026 - 01 - 10),
            Tipo::Compra,
            p,
            "BTC",
            "1",
            50_000,
            0,
        ),
        NOW,
    )
    .unwrap();
    cripto::save_op(
        &v,
        &op(
            date!(2026 - 01 - 10),
            Tipo::Compra,
            p,
            "ETH",
            "10",
            20_000,
            0,
        ),
        NOW,
    )
    .unwrap();
    assert!(asientos::deterioro(&v, 2026, NOW).is_err(), "sin precios");
    cripto::set_prices(
        &v,
        &[
            Price {
                date: date!(2026 - 12 - 31),
                ticker: "BTC".into(),
                price: Precio::from_euros(60_000),
                source: "manual".into(),
            },
            Price {
                date: date!(2026 - 12 - 31),
                ticker: "ETH".into(),
                price: Precio::from_euros(1_500),
                source: "manual".into(),
            },
        ],
        NOW,
    )
    .unwrap();
    let r = cripto::report(&v, 2026).unwrap();
    let btc = r.posiciones.iter().find(|x| x.ticker == "BTC").unwrap();
    assert_eq!((btc.valor, btc.latente), (Some(m(60_000)), Some(m(10_000))));
    let id = asientos::deterioro(&v, 2026, NOW).unwrap().unwrap();
    let e = journal(&v, 2026)
        .unwrap()
        .into_iter()
        .find(|e| e.id == id)
        .unwrap();
    assert_eq!(e.entry.origin, Origin::Cripto(-2026));
    assert_eq!(e.entry.date, date!(2026 - 12 - 31));
    assert_eq!(e.entry.total_debit(), m(5_000)); // sólo ETH: 20.000 − 15.000
    assert_eq!(saldo(&v, 2026, "693"), m(5_000));
    assert_eq!(saldo(&v, 2026, "390"), m(-5_000));
    // Repetirlo sustituye el asiento (no lo duplica).
    asientos::deterioro(&v, 2026, NOW).unwrap();
    assert_eq!(saldo(&v, 2026, "390"), m(-5_000));
    assert!(
        cripto::report(&v, 2026)
            .unwrap()
            .deterioro_registrado
            .is_some()
    );
}

#[test]
fn ejercicio_cerrado_protege_los_asientos() {
    let v = vault(false);
    let p = platform(&v, "Bit2Me", false);
    let id = cripto::save_op(
        &v,
        &op(
            date!(2025 - 03 - 10),
            Tipo::Compra,
            p,
            "BTC",
            "1",
            20_000,
            0,
        ),
        NOW,
    )
    .unwrap();
    asientos::contabilizar(&v, 2025, false, NOW).unwrap();
    close_year(&v, 2025, NOW, true).unwrap();
    let mut cambiada = op(
        date!(2025 - 03 - 10),
        Tipo::Compra,
        p,
        "BTC",
        "2",
        20_000,
        0,
    );
    cambiada.id = id;
    assert!(cripto::save_op(&v, &cambiada, NOW).is_err());
    assert!(cripto::delete_op(&v, id, NOW).is_err());
    // Una operación nueva de un año cerrado se guarda pero no se contabiliza.
    cripto::save_op(
        &v,
        &op(
            date!(2025 - 06 - 10),
            Tipo::Compra,
            p,
            "BTC",
            "1",
            25_000,
            0,
        ),
        NOW,
    )
    .unwrap();
    let c = asientos::contabilizar(&v, 2025, false, NOW).unwrap();
    assert_eq!((c.asientos, c.saltadas), (0, 1));
}

#[test]
fn umbral_del_721_con_precios_de_cierre() {
    let v = vault(true);
    let kraken = platform(&v, "Kraken", true);
    let ledger = platform(&v, "Ledger", false);
    cripto::save_op(
        &v,
        &op(
            date!(2025 - 03 - 10),
            Tipo::Compra,
            kraken,
            "BTC",
            "0.6",
            30_000,
            0,
        ),
        NOW,
    )
    .unwrap();
    cripto::save_op(
        &v,
        &op(
            date!(2025 - 03 - 10),
            Tipo::Compra,
            ledger,
            "BTC",
            "1",
            50_000,
            0,
        ),
        NOW,
    )
    .unwrap();
    let hoy = date!(2026 - 02 - 01);
    // Sin precio de cierre: aviso de precios, no del 721.
    let a = umbrales::alerts(&v, hoy).unwrap();
    assert!(a.iter().any(|x| x.code == "cripto_precios"));
    assert!(!a.iter().any(|x| x.code == "721"));
    // A 80.000 €/BTC: 48.000 en Kraken (no llega); el monedero propio no cuenta.
    cripto::set_prices(
        &v,
        &[Price {
            date: date!(2025 - 12 - 31),
            ticker: "BTC".into(),
            price: Precio::from_euros(80_000),
            source: "manual".into(),
        }],
        NOW,
    )
    .unwrap();
    let a = umbrales::alerts(&v, hoy).unwrap();
    assert!(
        !a.iter()
            .any(|x| x.code == "721" || x.code == "cripto_precios")
    );
    let m721 = cripto::report(&v, 2025).unwrap().m721;
    assert_eq!((m721.total_extranjero, m721.supera), (m(48_000), false));
    // A 90.000: 54.000 → aviso del 721 (persona física: warning).
    cripto::set_prices(
        &v,
        &[Price {
            date: date!(2025 - 12 - 31),
            ticker: "BTC".into(),
            price: Precio::from_euros(90_000),
            source: "manual".into(),
        }],
        NOW,
    )
    .unwrap();
    let a = umbrales::alerts(&v, hoy).unwrap();
    let x = a.iter().find(|x| x.code == "721").unwrap();
    assert_eq!(x.severity, umbrales::Severity::Warning);
    assert!(x.detail.contains("54.000,00"));
    let m721 = cripto::report(&v, 2025).unwrap().m721;
    assert!(m721.supera && m721.nota.contains("1 de enero y el 31 de marzo de 2026"));
    assert_eq!(m721.plataformas.len(), 1);
    // Marcado en el perfil: ya no avisa.
    v.set_setting("taxpayer_profile", &{
        let mut p = profile(true);
        p.monedas_virtuales_extranjero = true;
        p
    })
    .unwrap();
    assert!(
        !umbrales::alerts(&v, hoy)
            .unwrap()
            .iter()
            .any(|x| x.code == "721")
    );
}

#[test]
fn renta_de_persona_fisica_con_fifo_aunque_elija_pmp() {
    let v = vault(true);
    cripto::save_settings(
        &v,
        &Settings {
            metodo: Metodo::Pmp,
            ..Settings::default()
        },
        NOW,
    )
    .unwrap();
    let a = platform(&v, "Kraken", true);
    let b = platform(&v, "Bit2Me", false);
    cripto::save_op(
        &v,
        &op(
            date!(2026 - 01 - 10),
            Tipo::Compra,
            a,
            "BTC",
            "1",
            20_000,
            0,
        ),
        NOW,
    )
    .unwrap();
    cripto::save_op(
        &v,
        &op(
            date!(2026 - 02 - 10),
            Tipo::Compra,
            b,
            "BTC",
            "1",
            40_000,
            0,
        ),
        NOW,
    )
    .unwrap();
    // Vende en Bit2Me: con FIFO global sale primero la de Kraken (20.000).
    cripto::save_op(
        &v,
        &op(date!(2026 - 03 - 10), Tipo::Venta, b, "BTC", "1", 50_000, 0),
        NOW,
    )
    .unwrap();
    let r = cripto::renta(&v, 2026).unwrap().unwrap();
    assert_eq!(
        (r.ganancias, r.perdidas, r.neto),
        (m(30_000), m(0), m(30_000))
    );
    assert_eq!(r.lineas.len(), 1);
    assert_eq!(
        (
            r.lineas[0].clave,
            r.lineas[0].valor_transmision,
            r.lineas[0].valor_adquisicion
        ),
        ("F", m(50_000), m(20_000))
    );
    assert!(cripto::renta(&v, 2025).unwrap().is_none());
    let rep = cripto::report(&v, 2026).unwrap();
    assert_eq!(rep.metodo_declaracion, Metodo::Fifo);
    assert!(rep.avisos[0].contains("FIFO"));
    // El informe de una persona física enseña lo que declara: FIFO de todas sus BTC juntas
    // (como la Renta), así que la venta en Bit2Me saca primero el lote de Kraken (20.000).
    assert_eq!(rep.realizados[0].coste, m(20_000));
    assert!(!rep.avisos.iter().any(|a| a.contains("sólo tenía")));
}

#[test]
fn importacion_con_plataformas_nuevas_y_sin_duplicados() {
    let v = vault(false);
    let csv = "Type;Buy Amount;Buy Currency;Sell Amount;Sell Currency;Fee;Fee Currency;Exchange;Trade Group;Comment;Date\n\
               Trade;0.5;BTC;20000;EUR;10;EUR;Kraken;;;10.01.2026 10:00:00\n\
               Trade;10000;EUR;0.2;BTC;5;EUR;Kraken;;;10.05.2026 10:00:00\n";
    let l = importar::leer(&v, "CoinTracking - Trade List.csv", csv.as_bytes()).unwrap();
    assert_eq!(l.kind, "cointracking");
    assert_eq!(l.ops.len(), 2);
    assert_eq!(l.new_platforms, vec!["Kraken".to_owned()]);
    assert_eq!(l.duplicates, 0);
    assert_eq!(importar::aplicar(&v, &l, NOW).unwrap(), (2, 0, 0));
    let ps = cripto::platforms(&v).unwrap();
    assert_eq!(
        (ps[0].name.as_str(), ps[0].abroad, ps[0].country.as_str()),
        ("Kraken", true, "US")
    );
    // Segunda vez: todo duplicado.
    let l2 = importar::leer(&v, "CoinTracking - Trade List.csv", csv.as_bytes()).unwrap();
    assert_eq!(l2.duplicates, 2);
    assert_eq!(importar::aplicar(&v, &l2, NOW).unwrap(), (0, 2, 0));
    assert_eq!(cripto::ops(&v, None).unwrap().len(), 2);
    let r = cripto::report(&v, 2026).unwrap();
    assert_eq!(r.resumen_pmp.neto, Money::from_cents(199_100)); // 9.995 − 0,2 × 20.010
    // Hoja de saldos → precios.
    let saldos = ";BTC;\nFecha;Valor en EUR;Cantidad\n31/12/2026;27.000,00;0,3\n";
    let l3 = importar::leer(&v, "CoinTracking - Saldos.csv", saldos.as_bytes()).unwrap();
    assert_eq!(l3.kind, "saldos");
    assert_eq!(importar::aplicar(&v, &l3, NOW).unwrap(), (0, 0, 1));
    let r = cripto::report(&v, 2026).unwrap();
    assert_eq!(r.posiciones[0].valor, Some(m(27_000)));
    let (name, bytes) = cripto::export::workbook(&v, 2026).unwrap();
    assert!(name.ends_with(".xlsx") && bytes.starts_with(b"PK"));
    let (name, pdf) = cripto::export::pdf_bytes(&v, 2026).unwrap();
    assert!(name.ends_with(".pdf") && pdf.starts_with(b"%PDF"));
}

#[test]
fn ejercicio_no_natural_y_721_a_31_de_diciembre() {
    // Sociedad con ejercicio de julio a junio: el 2026 va del 1/7/2026 al 30/6/2027.
    let v = vault(false);
    let mut p = profile(false);
    p.cierre_ejercicio = (time::Month::June, 30);
    v.set_setting("taxpayer_profile", &p).unwrap();
    let kraken = platform(&v, "Kraken", true);
    for o in [
        op(
            date!(2025 - 12 - 01),
            Tipo::Compra,
            kraken,
            "ETH",
            "1",
            1_000,
            0,
        ),
        op(
            date!(2026 - 08 - 01),
            Tipo::Compra,
            kraken,
            "BTC",
            "1",
            40_000,
            0,
        ),
        op(
            date!(2027 - 03 - 01),
            Tipo::Venta,
            kraken,
            "BTC",
            "0.5",
            30_000,
            0,
        ),
    ] {
        cripto::save_op(&v, &o, NOW).unwrap();
    }
    let precio = |d: Date, t: &str, e: i64| Price {
        date: d,
        ticker: t.into(),
        price: Precio::from_euros(e),
        source: "manual".into(),
    };
    cripto::set_prices(
        &v,
        &[
            precio(date!(2026 - 12 - 31), "BTC", 120_000),
            precio(date!(2026 - 12 - 31), "ETH", 1_000),
            precio(date!(2027 - 06 - 30), "BTC", 50_000),
            precio(date!(2027 - 06 - 30), "ETH", 1_000),
        ],
        NOW,
    )
    .unwrap();
    // Las operaciones y lo realizado son las del ejercicio, no las del año natural.
    assert_eq!(cripto::ops(&v, Some(2026)).unwrap().len(), 2);
    let r = cripto::report(&v, 2026).unwrap();
    assert_eq!(r.fecha, date!(2027 - 06 - 30));
    assert_eq!(r.ops_ejercicio, 2);
    assert_eq!(r.realizados.len(), 1);
    assert_eq!(r.resumen_fifo.neto, m(10_000));
    assert_eq!(r.resumen_pmp.transmisiones, 1);
    // El 721, a 31 de diciembre: 1 BTC a 120.000 y 1 ETH a 1.000.
    assert_eq!(r.m721.fecha, date!(2026 - 12 - 31));
    assert_eq!((r.m721.total_extranjero, r.m721.supera), (m(121_000), true));
}

/// Comisiones en cripto de compras y ventas importadas de CoinTracking (la comisión sale del
/// saldo aparte del importe): en la propia cripto cambia la cantidad y no el coste; en una
/// tercera (BNB) se valora al precio de cierre del día y se suma al coste de lo comprado, y
/// entregar el BNB es una transmisión (sin contar la comisión dos veces como gasto).
#[test]
fn comisiones_en_cripto_de_compras_y_ventas() {
    let v = vault(false);
    let csv = "Type;Buy Amount;Buy Currency;Sell Amount;Sell Currency;Fee;Fee Currency;Exchange;Trade Group;Comment;Date\n\
               Trade;1;BNB;400;EUR;;;Binance;;;05.01.2026 10:00:00\n\
               Trade;1;BTC;40000;EUR;0.001;BTC;Binance;;;10.01.2026 10:00:00\n\
               Trade;1;ETH;2000;EUR;0.01;BNB;Binance;;;01.02.2026 10:00:00\n\
               Trade;45000;EUR;0.5;BTC;0.0005;BTC;Binance;;;01.03.2026 10:00:00\n";
    let l = importar::leer(&v, "CoinTracking - Trade List.csv", csv.as_bytes()).unwrap();
    assert_eq!(l.ops.len(), 5, "la comisión en BNB es una operación aparte");
    let compra = &l.ops[1].op;
    // Llegan 0,999 BTC; el coste sigue siendo 40.000 € (39.960 + 40 de comisión).
    assert_eq!(
        (compra.qty, compra.eur, compra.fee_eur),
        (q("0.999"), m(39_960), m(40))
    );
    let venta = &l.ops[4].op;
    // Salen 0,5005 BTC; valor de transmisión = 45.045 − 45 = 45.000 € cobrados.
    assert_eq!(
        (venta.qty, venta.eur, venta.fee_eur),
        (q("0.5005"), m(45_045), m(45))
    );
    importar::aplicar(&v, &l, NOW).unwrap();
    cripto::set_prices(
        &v,
        &[Price {
            date: date!(2026 - 02 - 01),
            ticker: "BNB".into(),
            price: Precio::from_euros(500),
            source: "manual".into(),
        }],
        NOW,
    )
    .unwrap();
    let r = cripto::report(&v, 2026).unwrap();
    let pos = |t: &str| r.posiciones.iter().find(|p| p.ticker == t).unwrap();
    // ETH: 2.000 € + 0,01 BNB × 500 € = 2.005 €.
    assert_eq!((pos("ETH").cantidad, pos("ETH").coste), (q("1"), m(2_005)));
    assert_eq!(pos("BNB").cantidad, q("0.99"));
    // BTC: quedan 0,999 − 0,5005 = 0,4985.
    assert_eq!(pos("BTC").cantidad, q("0.4985"));
    let venta = r.realizados.iter().find(|x| x.ticker == "BTC").unwrap();
    // Coste: 40.000 × 0,5005 / 0,999 = 20.040,04 €.
    assert_eq!(
        (venta.valor, venta.coste),
        (m(45_000), Money::from_cents(2_004_004))
    );
    let bnb = r.realizados.iter().find(|x| x.ticker == "BNB").unwrap();
    assert_eq!((bnb.valor, bnb.coste), (m(5), m(4)));
    assert_eq!(r.resumen_pmp.comisiones, Money::ZERO, "sin gasto aparte");

    // Asientos: la comisión en BNB de la compra va a la subcuenta del ETH, no a la 626.
    asientos::contabilizar(&v, 2026, false, NOW).unwrap();
    for e in &journal(&v, 2026).unwrap() {
        assert_eq!(e.entry.total_debit(), e.entry.total_credit());
    }
    let cuenta = |t: &str| {
        cripto::assets(&v)
            .unwrap()
            .into_iter()
            .find(|a| a.ticker == t)
            .unwrap()
            .account
    };
    assert_eq!(saldo(&v, 2026, &cuenta("ETH")), m(2_005));
    assert_eq!(saldo(&v, 2026, "626"), m(45));
    // Banco: −400 −40.000 −2.000 +45.000.
    assert_eq!(saldo(&v, 2026, "572"), m(2_600));
}

/// Operaciones de CoinTracking importadas antes de descontar la comisión en la misma cripto
/// (cantidad bruta) y con la misma huella: al volver a importar el fichero, la que no está
/// contabilizada se corrige y la contabilizada no se toca, con un aviso que dice qué borrar.
#[test]
fn reimportar_corrige_la_cantidad_bruta_de_antes() {
    let v = vault(false);
    let csv = "Type;Buy Amount;Buy Currency;Sell Amount;Sell Currency;Fee;Fee Currency;Exchange;Trade Group;Comment;Date\n\
               Trade;10;ETH;20000;EUR;0.01;ETH;Binance;;;10.06.2025 10:00:00\n\
               Trade;1;BTC;40000;EUR;0.001;BTC;Binance;;;10.01.2026 10:00:00\n";
    let l = importar::leer(&v, "CoinTracking - Trade List.csv", csv.as_bytes()).unwrap();
    assert_eq!(importar::aplicar(&v, &l, NOW).unwrap(), (2, 0, 0));
    // La de ETH (2025) queda contabilizada.
    asientos::contabilizar(&v, 2025, false, NOW).unwrap();
    // Como las guardaba la versión anterior: cantidad bruta, todo el pago como contravalor y la
    // comisión valorada aparte.
    let conn = v.connection();
    conn.execute(
        "UPDATE crypto_op SET qty = '1', eur = 4000000, fee_eur = 4000 WHERE ticker = 'BTC'",
        [],
    )
    .unwrap();
    conn.execute(
        "UPDATE crypto_op SET qty = '10', eur = 2000000, fee_eur = 2000 WHERE ticker = 'ETH'",
        [],
    )
    .unwrap();
    let antes = cripto::ops(&v, None).unwrap();
    let eth_antes = antes.iter().find(|o| o.ticker == "ETH").unwrap().clone();
    assert!(eth_antes.entry_id.is_some());

    let l2 = importar::leer(&v, "CoinTracking - Trade List.csv", csv.as_bytes()).unwrap();
    assert_eq!(l2.duplicates, 2);
    let btc = l2
        .warnings
        .iter()
        .find(|w| w.contains("compra de BTC del 10/01/2026"))
        .unwrap_or_else(|| panic!("sin aviso de BTC: {:?}", l2.warnings));
    assert!(btc.contains("Se corrige al importar"), "{btc}");
    let eth = l2
        .warnings
        .iter()
        .find(|w| w.contains("compra de ETH del 10/06/2025"))
        .unwrap_or_else(|| panic!("sin aviso de ETH: {:?}", l2.warnings));
    assert!(eth.contains("ya está contabilizada"), "{eth}");
    assert!(eth.contains("borra esa operación"), "{eth}");

    assert_eq!(importar::aplicar(&v, &l2, NOW).unwrap(), (0, 2, 0));
    let ops = cripto::ops(&v, None).unwrap();
    assert_eq!(ops.len(), 2, "no se duplica nada");
    let btc = ops.iter().find(|o| o.ticker == "BTC").unwrap();
    // 0,999 BTC con coste 40.000 € (39.960 + 40).
    assert_eq!(
        (btc.qty, btc.eur, btc.fee_eur),
        (q("0.999"), m(39_960), m(40))
    );
    let eth = ops.iter().find(|o| o.ticker == "ETH").unwrap();
    assert_eq!(eth, &eth_antes, "la contabilizada no se toca");

    // Una tercera vez ya no avisa de BTC.
    let l3 = importar::leer(&v, "CoinTracking - Trade List.csv", csv.as_bytes()).unwrap();
    assert!(!l3.warnings.iter().any(|w| w.contains("de BTC del")));
}

/// Una permuta de Kraken de cerca de medianoche importada por la versión anterior: con la
/// fecha del libro (UTC, el día antes que en Madrid), la cantidad bruta y el valor en euros
/// puesto a mano. Al volver a importar se corrige la cantidad recibida, pero no se pierde el
/// valor puesto a mano (la lectura la trae sin valor) ni se salta por el día de diferencia.
/// Datos inventados.
#[test]
fn reimportar_permuta_de_kraken_conserva_los_euros_puestos_a_mano() {
    let v = vault(false);
    let csv = "txid;refid;time;type;subtype;aclass;asset;amount;fee;balance\n\
               L3;T2;2026-02-01 23:30:00.0000;trade;;currency;XXBT;-0.2000000000;0;0.3\n\
               L4;T2;2026-02-01 23:30:00.0000;trade;;currency;XETH;4.0000000000;0.0100000000;4\n";
    let l = importar::leer(&v, "ledgers.csv", csv.as_bytes()).unwrap();
    assert_eq!(importar::aplicar(&v, &l, NOW).unwrap(), (1, 0, 0));
    let p = &cripto::ops(&v, None).unwrap()[0];
    assert_eq!(
        (p.kind, p.date, p.qty2),
        (Tipo::Permuta, date!(2026 - 02 - 02), Some(q("3.99")))
    );
    // Como la guardaba la versión anterior: fecha en UTC y lo recibido sin quitar la comisión.
    v.connection()
        .execute(
            "UPDATE crypto_op SET date = '2026-02-01', qty2 = '4' WHERE id = ?1",
            [p.id],
        )
        .unwrap();
    // El valor en euros, puesto a mano (editar conserva la huella).
    let mut a_mano = cripto::ops(&v, None).unwrap()[0].clone();
    a_mano.eur = m(8_000);
    cripto::save_op(&v, &a_mano, NOW).unwrap();

    let ejercicios = || -> i64 {
        v.connection()
            .query_row("SELECT COUNT(*) FROM fiscal_year", [], |r| r.get(0))
            .unwrap()
    };
    let antes = ejercicios();
    let l2 = importar::leer(&v, "ledgers.csv", csv.as_bytes()).unwrap();
    assert_eq!(ejercicios(), antes, "la vista previa no crea ejercicios");
    assert_eq!(l2.duplicates, 1);
    let aviso = l2
        .warnings
        .iter()
        .find(|w| w.contains("permuta de BTC del 01/02/2026"))
        .unwrap_or_else(|| panic!("sin aviso: {:?}", l2.warnings));
    assert!(aviso.contains("Se corrige al importar"), "{aviso}");
    assert!(
        aviso.contains("se conserva el valor guardado de 8.000,00 €"),
        "{aviso}"
    );
    assert_eq!(importar::aplicar(&v, &l2, NOW).unwrap(), (0, 1, 0));
    let ops = cripto::ops(&v, None).unwrap();
    assert_eq!(ops.len(), 1, "no se duplica");
    assert_eq!(
        (ops[0].qty, ops[0].qty2, ops[0].eur),
        (q("0.2"), Some(q("3.99")), m(8_000))
    );
}
