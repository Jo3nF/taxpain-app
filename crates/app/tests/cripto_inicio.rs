//! Punto de partida de Criptomonedas: lo que calculó la gestoría a una fecha de corte manda; las
//! operaciones de antes (incompletas) son historial; después se sigue con precio medio. Y la
//! propuesta desde el diario, la hoja de saldos y el 200. Datos inventados.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::{record_entry, set_account, trial_balance};
use taxpain_app::carpeta::{self, Found};
use taxpain_app::cripto::inicio::{self, Inicio, PosicionInicio};
use taxpain_app::cripto::{self, Naturaleza, Op, Platform, Price, Settings, asientos, importar};
use taxpain_core::accounting::{AccountCode, EntryLine, JournalEntry, Origin};
use taxpain_core::calendar::TaxpayerProfile;
use taxpain_core::cripto::{Metodo, Precio, Qty, Tipo};
use taxpain_core::money::Money;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime};

const NOW: OffsetDateTime = datetime!(2026-11-02 10:00 UTC);
const HOY: Date = date!(2026 - 11 - 02);

fn m(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn q(s: &str) -> Qty {
    Qty::parse(s).unwrap()
}

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

fn platform(v: &Vault, name: &str, abroad: bool) -> i64 {
    cripto::save_platform(
        v,
        &Platform {
            id: 0,
            name: name.into(),
            kind: "exchange".into(),
            country: if abroad { "IE".into() } else { "ES".into() },
            abroad,
            account: String::new(),
        },
        NOW,
    )
    .unwrap()
}

fn op(date: Date, kind: Tipo, platform: i64, qty: &str, eur: i64) -> Op {
    Op {
        id: 0,
        date,
        time: "10:00:00".into(),
        kind,
        platform_id: platform,
        to_platform_id: None,
        ticker: "BTC".into(),
        qty: q(qty),
        ticker2: None,
        qty2: None,
        eur: m(eur),
        fee_eur: Money::ZERO,
        ref_price: None,
        note: String::new(),
        source: "manual".into(),
        fingerprint: None,
        entry_id: None,
    }
}

fn posicion(ticker: &str, platform_id: Option<i64>, cantidad: &str, coste: i64) -> PosicionInicio {
    PosicionInicio {
        ticker: ticker.into(),
        platform_id,
        cantidad: q(cantidad),
        coste: m(coste),
    }
}

fn line(account: &str, euros: i64, debit: bool) -> EntryLine {
    EntryLine::signed(AccountCode::new(account).unwrap(), m(euros), debit).unwrap()
}

fn asiento(v: &Vault, date: Date, origin: Origin, lines: Vec<EntryLine>) {
    record_entry(
        v.connection(),
        &JournalEntry {
            date,
            concept: "Asiento del gestor".into(),
            origin,
            lines,
        },
        NOW,
    )
    .unwrap();
}

fn saldo(v: &Vault, year: i32, prefix: &str) -> Money {
    trial_balance(v, year, None)
        .unwrap()
        .iter()
        .filter(|r| r.account.as_str().starts_with(prefix))
        .map(|r| r.debit - r.credit)
        .sum()
}

/// Punto de partida a 31/12/2025 con 1 BTC a 30.000 € (la gestoría): la venta de 0,5 BTC en 2026
/// por 40.000 € gana 25.000 €; una compra después recalcula el medio con todo el BTC; las
/// operaciones incompletas de antes no recalculan nada, ni se contabilizan.
#[test]
fn desde_el_punto_de_partida_con_precio_medio() {
    let v = vault();
    cripto::save_settings(
        &v,
        &Settings {
            metodo: Metodo::Pmp,
            naturaleza: Naturaleza::Intangible,
            ..Settings::default()
        },
        NOW,
    )
    .unwrap();
    let kraken = platform(&v, "Kraken", true);
    // Historial incompleto: una venta en 2023 de BTC que no se sabe cuándo se compró.
    cripto::save_op(
        &v,
        &op(date!(2023 - 05 - 10), Tipo::Venta, kraken, "2", 50_000),
        NOW,
    )
    .unwrap();
    // Sin punto de partida, la venta sale con coste cero y como hueco.
    let r = cripto::report(&v, 2023).unwrap();
    assert_eq!(r.huecos, 1);
    assert!(
        r.avisos[0].starts_with("Faltan operaciones"),
        "{:?}",
        r.avisos
    );
    assert_eq!(r.resumen_pmp.ganancias, m(50_000));
    assert!(inicio::aviso_sin_inicio(&v).unwrap().is_some());

    inicio::guardar(
        &v,
        &Inicio {
            fecha: date!(2025 - 12 - 31),
            cuenta: "2050004".into(),
            origen: "Cálculos de la gestoría".into(),
            nota: String::new(),
            posiciones: vec![posicion("btc", None, "1", 30_000)],
        },
        NOW,
    )
    .unwrap();
    assert!(inicio::aviso_sin_inicio(&v).unwrap().is_none());
    let i = inicio::inicio(&v).unwrap().unwrap();
    assert_eq!(i.posiciones[0].ticker, "BTC");
    assert_eq!(i.coste_total(), m(30_000));

    cripto::save_op(
        &v,
        &op(date!(2026 - 03 - 10), Tipo::Venta, kraken, "0.5", 40_000),
        NOW,
    )
    .unwrap();
    cripto::save_op(
        &v,
        &op(date!(2026 - 06 - 10), Tipo::Compra, kraken, "0.5", 35_000),
        NOW,
    )
    .unwrap();
    cripto::save_op(
        &v,
        &op(date!(2026 - 09 - 10), Tipo::Venta, kraken, "0.5", 30_000),
        NOW,
    )
    .unwrap();

    // 2023: calculado por la gestoría; la operación es historial.
    let r = cripto::report(&v, 2023).unwrap();
    assert!(r.antes_del_inicio);
    assert_eq!(
        (r.ops_ejercicio, r.ops_historial, r.ops_sin_asiento),
        (1, 1, 0)
    );
    assert!(r.posiciones.is_empty() && r.realizados.is_empty());
    assert_eq!(r.resumen_pmp.transmisiones, 0);
    assert_eq!(r.huecos, 0);
    assert!(
        r.avisos[0].contains("calculado por la gestoría"),
        "{:?}",
        r.avisos
    );
    assert!(r.m721.nota.contains("anterior al punto de partida"));
    // 2025: las posiciones a 31/12 son las del punto de partida.
    let r = cripto::report(&v, 2025).unwrap();
    assert!(r.antes_del_inicio);
    assert_eq!(r.posiciones.len(), 1);
    assert_eq!(
        (
            r.posiciones[0].cantidad,
            r.posiciones[0].coste,
            r.posiciones[0].platform.as_str()
        ),
        (q("1"), m(30_000), "Sin plataforma")
    );
    // 2026: 25.000 de la primera venta; la compra deja 1 BTC por 50.000 → la segunda venta
    // cuesta 25.000 y gana 5.000.
    let r = cripto::report(&v, 2026).unwrap();
    assert!(!r.antes_del_inicio);
    assert_eq!(r.realizados.len(), 2);
    assert_eq!(
        (r.realizados[0].coste, r.realizados[0].resultado),
        (m(15_000), m(25_000))
    );
    assert_eq!(
        (r.realizados[1].coste, r.realizados[1].resultado),
        (m(25_000), m(5_000))
    );
    assert_eq!(r.resumen_pmp.neto, m(30_000));
    assert_eq!(r.coste_total, m(25_000));
    assert_eq!(r.ops_sin_asiento, 3);
    assert!(
        r.avisos.iter().all(|a| !a.contains("sólo tenía")),
        "{:?}",
        r.avisos
    );

    // Asientos: nunca para el ejercicio de antes; los de 2026, contra la cuenta de la gestoría.
    assert!(asientos::contabilizar(&v, 2025, false, NOW).is_err());
    assert!(asientos::deterioro(&v, 2025, NOW).is_err());
    let c = asientos::contabilizar(&v, 2026, false, NOW).unwrap();
    assert_eq!(c.asientos, 3);
    // Con el saldo de apertura de la gestoría (30.000), la cuenta queda al coste de lo que hay.
    asiento(
        &v,
        date!(2026 - 01 - 01),
        Origin::Apertura,
        vec![
            line("2050004", 30_000, true),
            line("1130000", 30_000, false),
        ],
    );
    assert_eq!(saldo(&v, 2026, "2050004"), m(25_000));
    assert_eq!(saldo(&v, 2026, "771"), m(-30_000));
    // La operación de 2023 sigue sin asiento.
    assert!(
        cripto::ops(&v, Some(2023)).unwrap()[0].entry_id.is_none(),
        "la de antes no se contabiliza"
    );
}

/// El 721 a 31/12 sale de las cantidades desde el punto de partida, en la plataforma que se le
/// dio; la Renta de un año anterior no se calcula.
#[test]
fn el_721_desde_el_punto_de_partida() {
    let v = vault();
    let kraken = platform(&v, "Kraken", true);
    let ledger = platform(&v, "Ledger", false);
    inicio::guardar(
        &v,
        &Inicio {
            fecha: date!(2025 - 12 - 31),
            cuenta: String::new(),
            origen: String::new(),
            nota: String::new(),
            posiciones: vec![
                posicion("BTC", Some(kraken), "0.8", 24_000),
                posicion("BTC", Some(ledger), "0.2", 6_000),
            ],
        },
        NOW,
    )
    .unwrap();
    // No se puede quitar una plataforma del punto de partida.
    assert!(cripto::delete_platform(&v, ledger).is_err());
    cripto::save_op(
        &v,
        &op(date!(2026 - 03 - 10), Tipo::Venta, kraken, "0.3", 30_000),
        NOW,
    )
    .unwrap();
    cripto::set_prices(
        &v,
        &[
            Price {
                date: date!(2025 - 12 - 31),
                ticker: "BTC".into(),
                price: Precio::from_euros(80_000),
                source: "manual".into(),
            },
            Price {
                date: date!(2026 - 12 - 31),
                ticker: "BTC".into(),
                price: Precio::from_euros(100_000),
                source: "manual".into(),
            },
        ],
        NOW,
    )
    .unwrap();
    let m25 = cripto::report(&v, 2025).unwrap().m721;
    assert_eq!(m25.total_extranjero, m(64_000)); // 0,8 en Kraken a 80.000
    let r = cripto::report(&v, 2026).unwrap();
    assert_eq!(r.m721.total_extranjero, m(50_000)); // 0,5 en Kraken a 100.000
    assert_eq!(r.realizados[0].coste, m(9_000)); // 0,3 × 30.000 (medio de todo el BTC)
    let m26 = cripto::saldo_extranjero(&v, 2026).unwrap().unwrap();
    assert_eq!(m26.total_extranjero, m(50_000));
    assert!(cripto::saldo_extranjero(&v, 2024).unwrap().is_none());
    assert!(cripto::renta(&v, 2025).unwrap().is_none());
}

#[test]
fn guardar_comprueba_lo_que_se_pone() {
    let v = vault();
    let mal = |posiciones: Vec<PosicionInicio>| {
        inicio::guardar(
            &v,
            &Inicio {
                fecha: date!(2025 - 12 - 31),
                cuenta: String::new(),
                origen: String::new(),
                nota: String::new(),
                posiciones,
            },
            NOW,
        )
        .unwrap_err()
        .to_string()
    };
    assert!(mal(vec![posicion("BTC", None, "0", 10)]).contains("mayor que cero"));
    assert!(
        mal(vec![
            posicion("BTC", None, "1", 10),
            posicion("btc", None, "2", 10)
        ])
        .contains("dos veces")
    );
    assert!(mal(vec![posicion("BTC", Some(99), "1", 10)]).contains("no existe"));
    assert!(inicio::inicio(&v).unwrap().is_none());
    inicio::guardar(
        &v,
        &Inicio {
            fecha: date!(2025 - 12 - 31),
            cuenta: String::new(),
            origen: String::new(),
            nota: String::new(),
            posiciones: vec![posicion("ETH", None, "1", 0)],
        },
        NOW,
    )
    .unwrap();
    inicio::borrar(&v, NOW).unwrap();
    assert!(inicio::inicio(&v).unwrap().is_none());
}

/// Hoja de saldos como la de la gestoría: «Datos» con las cantidades por fecha (CoinTracking) y
/// una hoja hecha a mano con el coste pendiente por moneda y su total.
fn hoja_gestoria(reparto: bool) -> Vec<u8> {
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet().set_name("Control").unwrap();
    ws.write_string(3, 2, "ENTRADAS").unwrap();
    ws.write_number(4, 2, 90000.0).unwrap();
    if reparto {
        ws.write_string(6, 5, "BTC").unwrap();
        ws.write_number(6, 6, 73_000.004_1).unwrap();
        ws.write_string(7, 5, "ETH").unwrap();
        ws.write_number(7, 6, 4_999.996).unwrap();
        ws.write_string(8, 5, "TOTAL").unwrap();
        ws.write_number(8, 6, 78_000.0).unwrap();
    }
    let ws = wb.add_worksheet().set_name("Datos").unwrap();
    for (j, h) in [
        "Fecha",
        "EUR Valor en EUR",
        "EUR Cantidad",
        "BTC Valor en EUR",
        "BTC Cantidad",
        "ETH Valor en EUR",
        "ETH Cantidad",
        "ZEC Valor en EUR",
        "ZEC Cantidad",
    ]
    .iter()
    .enumerate()
    {
        ws.write_string(1, u16::try_from(j).unwrap(), *h).unwrap();
    }
    let filas: [(&str, [f64; 8]); 3] = [
        ("now", [0.5, 0.5, 70000.0, 0.9, 3000.0, 2.0, 30.0, 1.5]),
        (
            "01.12.2025",
            [0.5, 0.5, 80000.0, 1.0, 3500.0, 2.0, 40.0, 1.5],
        ),
        (
            "01.09.2025",
            [0.5, 0.5, 90000.0, 1.0, 3600.0, 2.0, 0.0, 0.0],
        ),
    ];
    for (i, (fecha, vals)) in filas.iter().enumerate() {
        let r = u32::try_from(i).unwrap() + 2;
        ws.write_string(r, 0, *fecha).unwrap();
        for (j, x) in vals.iter().enumerate() {
            ws.write_number(r, u16::try_from(j).unwrap() + 1, *x)
                .unwrap();
        }
    }
    wb.save_to_buffer().unwrap()
}

/// Diario de la gestoría de 2025: apertura de la cuenta de criptomonedas con 90.000 €, una venta
/// que da de baja 12.000 € de coste y el asiento de cierre (que no cuenta).
fn diario_2025(v: &Vault) {
    set_account(v, "2050004", "Criptoactivos", NOW).unwrap();
    asiento(
        v,
        date!(2025 - 01 - 01),
        Origin::Apertura,
        vec![
            line("2050004", 90_000, true),
            line("1130000", 90_000, false),
        ],
    );
    asiento(
        v,
        date!(2025 - 06 - 20),
        Origin::Manual,
        vec![
            line("5720001", 35_000, true),
            line("2050004", 12_000, false),
            line("7690009", 23_000, false),
        ],
    );
    asiento(
        v,
        date!(2025 - 12 - 31),
        Origin::Cierre,
        vec![
            line("1130000", 78_000, true),
            line("2050004", 78_000, false),
        ],
    );
}

fn importar_hoja(v: &Vault, reparto: bool) {
    let l = importar::leer(v, "control criptos.xlsx", &hoja_gestoria(reparto)).unwrap();
    assert_eq!(l.kind, "saldos");
    assert!(l.hoja_saldos.is_some());
    importar::aplicar(v, &l, NOW).unwrap();
}

/// El coste total sale del diario (saldo de la cuenta de criptomonedas a 31/12/2025 sin el
/// cierre); las cantidades y el reparto, de la hoja; cuadran → reparto seguro.
#[test]
fn propuesta_desde_el_diario_y_la_hoja_con_reparto() {
    let v = vault();
    diario_2025(&v);
    // El 200 de 2025 según la AEAT, con el inmovilizado intangible del balance.
    v.connection()
        .execute(
            "INSERT INTO aeat_filing(nif, model, year, period, csv, receipt, presented_at,
               checked_at, boxes, boxes_source)
             VALUES ('B00000000', '200', 2025, '0A', 'CSV1', '2001', '2026-07-24', 'x',
               '{\"casillas\":{\"102\":7800000}}', 'pdf')",
            [],
        )
        .unwrap();
    importar_hoja(&v, true);
    let p = inicio::propuesta(&v, HOY, None).unwrap();
    assert_eq!(p.ejercicio, 2025, "{}", p.motivo);
    assert!(p.motivo.contains("200 presentado"), "{}", p.motivo);
    assert_eq!(p.fecha, date!(2025 - 12 - 31));
    assert_eq!(p.coste_diario, Some(m(78_000)));
    assert_eq!(p.cuenta, "2050004");
    assert_eq!(p.casilla_102, Some(m(78_000)));
    assert_eq!(p.fecha_cantidades, Some(date!(2025 - 12 - 01)));
    assert!(p.reparto_seguro, "{:#?}", p.notas);
    assert_eq!(p.por_repartir, Money::ZERO);
    let get = |t: &str| p.posiciones.iter().find(|x| x.ticker == t).unwrap();
    assert_eq!((get("BTC").cantidad, get("BTC").coste), (q("1"), m(73_000)));
    assert_eq!((get("ETH").cantidad, get("ETH").coste), (q("2"), m(5_000)));
    assert_eq!(
        (get("ZEC").cantidad, get("ZEC").coste),
        (q("1.5"), Money::ZERO)
    );
    assert!(p.posiciones.iter().all(|x| x.ticker != "EUR"));
    assert!(
        p.notas
            .iter()
            .any(|n| n.contains("Cuadra con el inmovilizado intangible"))
    );
    assert!(p.notas.iter().any(|n| n.contains("ZEC va con coste cero")));
    assert!(
        p.notas
            .iter()
            .any(|n| n.contains("Inmovilizado intangible"))
    );
    // Se guarda tal cual.
    inicio::guardar(
        &v,
        &Inicio {
            fecha: p.fecha,
            cuenta: p.cuenta.clone(),
            origen: p.origen.clone(),
            nota: String::new(),
            posiciones: p.posiciones.clone(),
        },
        NOW,
    )
    .unwrap();
    let r = cripto::report(&v, 2025).unwrap();
    assert_eq!(r.coste_total, m(78_000));
}

/// Sin reparto en la hoja y con varias monedas, se propone el total del diario para repartir.
#[test]
fn propuesta_sin_reparto_pide_repartir() {
    let v = vault();
    diario_2025(&v);
    importar_hoja(&v, false);
    let p = inicio::propuesta(&v, HOY, None).unwrap();
    assert!(p.motivo.contains("último con diario"), "{}", p.motivo);
    assert_eq!(p.coste_diario, Some(m(78_000)));
    assert!(!p.reparto_seguro);
    assert_eq!(p.por_repartir, m(78_000));
    assert!(p.posiciones.iter().all(|x| x.coste.is_zero()));
    assert!(p.notas.iter().any(|n| n.contains("Falta repartir")));
}

/// Cada cuenta del diario es de una moneda («… BTC», «… ETH»): el coste de cada una es su saldo.
/// Y una cuenta que no se llama «cripto…» se encuentra si el usuario dice cuál es.
#[test]
fn propuesta_con_una_cuenta_por_moneda_o_la_que_diga_el_usuario() {
    let v = vault();
    set_account(&v, "2050001", "Criptoactivos BTC", NOW).unwrap();
    set_account(&v, "2050002", "Criptoactivos ETH", NOW).unwrap();
    set_account(&v, "2060009", "Activos digitales", NOW).unwrap();
    asiento(
        &v,
        date!(2025 - 01 - 01),
        Origin::Apertura,
        vec![
            line("2050001", 60_000, true),
            line("2050002", 4_000, true),
            line("2060009", 1_000, true),
            line("1130000", 65_000, false),
        ],
    );
    importar_hoja(&v, false);
    let p = inicio::propuesta(&v, HOY, None).unwrap();
    assert_eq!(p.coste_diario, Some(m(64_000)));
    assert!(p.cuenta.is_empty(), "dos cuentas: no hay una sola");
    assert!(p.reparto_seguro, "{:#?}", p.notas);
    let get = |t: &str| p.posiciones.iter().find(|x| x.ticker == t).unwrap().coste;
    assert_eq!(
        (get("BTC"), get("ETH"), get("ZEC")),
        (m(60_000), m(4_000), Money::ZERO)
    );
    assert!(
        p.notas
            .iter()
            .any(|n| n.contains("saldo de su cuenta del diario"))
    );
    // La que dice el usuario, aunque no se llame «cripto…».
    let p = inicio::propuesta(&v, HOY, Some("2060009")).unwrap();
    assert_eq!(p.coste_diario, Some(m(1_000)));
    assert_eq!(p.cuenta, "2060009");
    let p = inicio::propuesta(&v, HOY, Some("2069999")).unwrap();
    assert!(p.coste_diario.is_none());
    assert!(
        p.notas
            .iter()
            .any(|n| n.contains("La cuenta 2069999 no tiene saldo"))
    );
}

/// Sin hoja ni diario: las cantidades de las operaciones importadas, con su advertencia.
#[test]
fn propuesta_sin_diario_ni_hoja() {
    let v = vault();
    let kraken = platform(&v, "Kraken", true);
    cripto::save_op(
        &v,
        &op(date!(2024 - 05 - 10), Tipo::Compra, kraken, "0.4", 20_000),
        NOW,
    )
    .unwrap();
    let p = inicio::propuesta(&v, HOY, None).unwrap();
    assert_eq!(p.fecha, date!(2025 - 12 - 31));
    assert!(p.coste_diario.is_none());
    assert!(p.cantidades_de_operaciones);
    assert_eq!(p.posiciones[0].cantidad, q("0.4"));
    assert!(!p.reparto_seguro);
}

/// Importar de la carpeta un historial incompleto sin punto de partida lo avisa y lo sugiere;
/// con punto de partida, no.
#[test]
fn la_carpeta_avisa_de_operaciones_incompletas_sin_punto_de_partida() {
    let v = vault();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let p = |rel: &str, s: &str| {
        let f = root.join(rel);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, s).unwrap();
    };
    p(
        "cripto/operaciones.csv",
        "Fecha;Tipo;Activo;Cantidad;Euros;Comisión;Plataforma\n\
         10/01/2022;Compra;BTC;0,5;20.000,00;;Bit2Me\n\
         15/06/2022;Venta;BTC;0,8;30.000,00;;Bit2Me\n",
    );
    let inv = carpeta::scan(&v, root).unwrap();
    let item = inv
        .items
        .iter()
        .find(|i| i.path == "cripto/operaciones.csv")
        .unwrap();
    assert!(
        matches!(item.found, Found::Cripto { huecos: 1, .. }),
        "{:?}",
        item.found
    );
    assert!(
        item.warnings
            .iter()
            .any(|w| w.contains("faltan operaciones")),
        "{:?}",
        item.warnings
    );
    let out = carpeta::apply(&v, root, std::slice::from_ref(&item.path), NOW).unwrap();
    assert!(
        out[0].message.contains("punto de partida"),
        "{}",
        out[0].message
    );

    inicio::guardar(
        &v,
        &Inicio {
            fecha: date!(2025 - 12 - 31),
            cuenta: String::new(),
            origen: String::new(),
            nota: String::new(),
            posiciones: vec![posicion("BTC", None, "0.1", 3_000)],
        },
        NOW,
    )
    .unwrap();
    let inv = carpeta::scan(&v, root).unwrap();
    let item = &inv.items[0];
    assert!(
        matches!(item.found, Found::Cripto { huecos: 0, .. }),
        "{:?}",
        item.found
    );
    assert!(
        !item
            .warnings
            .iter()
            .any(|w| w.contains("faltan operaciones"))
    );
    let r = cripto::report(&v, 2022).unwrap();
    assert!(r.antes_del_inicio && r.huecos == 0);
}

fn inicio_a(fecha: Date, posiciones: Vec<PosicionInicio>) -> Inicio {
    Inicio {
        fecha,
        cuenta: String::new(),
        origen: String::new(),
        nota: String::new(),
        posiciones,
    }
}

/// La fecha de corte es el cierre de un ejercicio ya terminado: ni hoy ni después.
#[test]
fn la_fecha_de_corte_no_puede_ser_futura() {
    let v = vault();
    for f in [HOY, date!(2027 - 12 - 31)] {
        let e = inicio::guardar(&v, &inicio_a(f, vec![posicion("BTC", None, "1", 10)]), NOW)
            .unwrap_err()
            .to_string();
        assert!(e.contains("anterior a hoy"), "{e}");
    }
    assert!(inicio::inicio(&v).unwrap().is_none());
}

/// Cambiar el punto de partida (o la fecha de corte) cuando ya hay asientos de operaciones de
/// después cambia su coste: se avisa de que hay que rehacerlos. Y las que pasan a ser historial
/// con asiento de TaxPain, también.
#[test]
fn cambiar_el_punto_de_partida_con_asientos_avisa() {
    let v = vault();
    let kraken = platform(&v, "Kraken", true);
    let primero = inicio_a(
        date!(2024 - 12 - 31),
        vec![posicion("BTC", None, "1", 30_000)],
    );
    assert!(inicio::guardar(&v, &primero, NOW).unwrap().is_empty());
    cripto::save_op(
        &v,
        &op(date!(2025 - 03 - 10), Tipo::Venta, kraken, "0.5", 40_000),
        NOW,
    )
    .unwrap();
    assert_eq!(
        asientos::contabilizar(&v, 2025, false, NOW)
            .unwrap()
            .asientos,
        1
    );
    // Lo mismo otra vez: nada que avisar.
    assert!(inicio::guardar(&v, &primero, NOW).unwrap().is_empty());
    // Otro coste: el asiento de 2025 se hizo con el de antes.
    let avisos = inicio::guardar(
        &v,
        &inicio_a(
            date!(2024 - 12 - 31),
            vec![posicion("BTC", None, "1", 20_000)],
        ),
        NOW,
    )
    .unwrap();
    assert!(
        avisos
            .iter()
            .any(|a| a.contains("rehaz") && a.contains("2025")),
        "{avisos:?}"
    );
    // Corte más tarde: la venta contabilizada pasa a ser historial.
    let avisos = inicio::guardar(
        &v,
        &inicio_a(
            date!(2025 - 12 - 31),
            vec![posicion("BTC", None, "0.5", 10_000)],
        ),
        NOW,
    )
    .unwrap();
    assert!(avisos.iter().any(|a| a.contains("historial")), "{avisos:?}");
    // Quitarlo también cambia el coste.
    assert!(
        inicio::borrar(&v, NOW)
            .unwrap()
            .iter()
            .any(|a| a.contains("historial") || a.contains("rehaz")),
    );
}

/// Hoja de saldos de CoinTracking con SOL2 (Solana), polvo (unidades que valen menos de 1 €) y
/// el último saldo un mes antes del corte.
fn hoja_cointracking() -> Vec<u8> {
    let mut wb = rust_xlsxwriter::Workbook::new();
    let ws = wb.add_worksheet().set_name("Datos").unwrap();
    for (j, h) in [
        "Fecha",
        "SOL2 Valor en EUR",
        "SOL2 Cantidad",
        "PEPE Valor en EUR",
        "PEPE Cantidad",
        "DUST Valor en EUR",
        "DUST Cantidad",
        "BTC Valor en EUR",
        "BTC Cantidad",
    ]
    .iter()
    .enumerate()
    {
        ws.write_string(0, u16::try_from(j).unwrap(), *h).unwrap();
    }
    ws.write_string(1, 0, "30.11.2025").unwrap();
    // BTC sin valor en la hoja: sale del precio de cierre.
    for (j, x) in [
        Some(1500.0),
        Some(10.0),
        Some(0.4),
        Some(12345.0),
        Some(0.0),
        Some(0.000_01),
        None,
        Some(0.000_000_5),
    ]
    .iter()
    .enumerate()
    {
        if let Some(x) = x {
            ws.write_number(1, u16::try_from(j).unwrap() + 1, *x)
                .unwrap();
        }
    }
    wb.save_to_buffer().unwrap()
}

#[test]
fn propuesta_con_la_hoja_de_cointracking_y_polvo() {
    let v = vault();
    let l = importar::leer(&v, "saldos.xlsx", &hoja_cointracking()).unwrap();
    importar::aplicar(&v, &l, NOW).unwrap();
    // BTC sin valor en la hoja pero con precio de cierre: 0,0000005 × 80.000 = 0,04 €.
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
    let p = inicio::propuesta(&v, HOY, None).unwrap();
    let t: Vec<&str> = p.posiciones.iter().map(|x| x.ticker.as_str()).collect();
    assert_eq!(t, vec!["SOL"], "{:#?}", p.posiciones);
    assert_eq!(p.posiciones[0].cantidad, q("10"));
    // El polvo no se borra: se propone aparte para añadirlo si se quiere.
    let mut polvo: Vec<&str> = p.polvo.iter().map(|x| x.ticker.as_str()).collect();
    polvo.sort_unstable();
    assert_eq!(polvo, vec!["BTC", "DUST", "PEPE"]);
    assert!(
        p.notas.iter().any(|n| n.contains("menos de 1 €")),
        "{:#?}",
        p.notas
    );
    assert!(
        p.notas
            .iter()
            .any(|n| n.contains("30/11/2025") && n.contains("comprueba")),
        "{:#?}",
        p.notas
    );
}

/// Persona física con monedas del punto de partida sin plataforma: no pueden quedarse fuera del
/// 721 en silencio (la nota del 721 y el aviso de umbrales lo dicen).
#[test]
fn el_721_avisa_de_las_monedas_sin_plataforma() {
    let v = vault();
    v.set_setting("taxpayer_profile", &TaxpayerProfile::autonomo())
        .unwrap();
    let kraken = platform(&v, "Kraken", true);
    inicio::guardar(
        &v,
        &inicio_a(
            date!(2025 - 12 - 31),
            vec![
                posicion("BTC", None, "1", 30_000),
                posicion("ETH", Some(kraken), "2", 4_000),
            ],
        ),
        NOW,
    )
    .unwrap();
    let r = cripto::report(&v, 2026).unwrap();
    assert_eq!(r.m721.sin_plataforma, vec!["BTC".to_owned()]);
    assert!(r.m721.nota.contains("pónsela"), "{}", r.m721.nota);
    let alertas = taxpain_app::umbrales::alerts(&v, date!(2027 - 02 - 01)).unwrap();
    assert!(
        alertas.iter().any(|a| a.code == "cripto_sin_plataforma"),
        "{alertas:?}"
    );
}
