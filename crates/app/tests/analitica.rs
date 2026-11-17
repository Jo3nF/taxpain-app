//! Analítica por centros de coste y presupuestos con desviaciones.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::manual_entry;
use taxpain_app::analitica::{
    Center, Share, lines, report, save_center, set_line, set_redistribution, set_rule,
};
use taxpain_app::presupuestos::{self, BudgetLine, from_actuals};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::money::Money;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

fn e(d: time::Date, debit: &str, credit: &str, euros: i64) -> JournalEntry {
    JournalEntry {
        date: d,
        concept: format!("{debit}/{credit}"),
        origin: Origin::Manual,
        lines: vec![
            EntryLine::debit(acc(debit), Money::from_euros(euros, 0)),
            EntryLine::credit(acc(credit), Money::from_euros(euros, 0)),
        ],
    }
}

fn s(c: &str, pct: u16) -> Share {
    Share {
        center: c.into(),
        pct_bp: pct * 100,
    }
}

#[test]
fn centros_reglas_y_cuenta_de_resultados() {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let now = datetime!(2026-10-20 10:00 UTC);
    for (d, de, ha, eur) in [
        (date!(2025 - 03 - 10), "621", "5720001", 1000),
        (date!(2025 - 03 - 20), "5720001", "705", 3000),
        (date!(2026 - 01 - 10), "621", "5720001", 1200),
        (date!(2026 - 02 - 15), "629", "5720001", 300),
        (date!(2026 - 02 - 20), "5720001", "705", 4000),
    ] {
        manual_entry(&v, &e(d, de, ha, eur), now).unwrap();
    }
    for (code, name) in [("TIENDA", "Tienda"), ("WEB", "Tienda online")] {
        save_center(
            &v,
            &Center {
                code: code.into(),
                name: name.into(),
                parent: None,
            },
            now,
        )
        .unwrap();
    }
    assert!(
        save_center(
            &v,
            &Center {
                code: "a b".into(),
                name: "x".into(),
                parent: None,
            },
            now
        )
        .is_err()
    );
    // El alquiler, 75 % tienda y 25 % web; las ventas, 60/40.
    assert!(
        set_rule(&v, "62", &[s("TIENDA", 70)], now).is_err(),
        "no suma 100"
    );
    assert!(
        set_rule(&v, "57", &[s("TIENDA", 100)], now).is_err(),
        "sólo 6 y 7"
    );
    assert!(set_rule(&v, "62", &[s("OTRO", 100)], now).is_err());
    set_rule(&v, "62", &[s("TIENDA", 75), s("WEB", 25)], now).unwrap();
    set_rule(&v, "705", &[s("TIENDA", 60), s("WEB", 40)], now).unwrap();
    // El 629 de febrero, todo a la web (reparto propio de la línea).
    let l = lines(&v, 2026, None, None).unwrap();
    let otros = l.iter().find(|x| x.account == "629").unwrap();
    assert_eq!(otros.source, "regla", "62 también cubre la 629");
    set_line(&v, otros.line_id, &[s("WEB", 100)], now).unwrap();
    let bank = taxpain_app::accounting::journal(&v, 2026).unwrap();
    assert!(
        bank.iter().all(|e| e.entry.lines.len() == 2),
        "el diario no cambia"
    );

    let r = report(&v, 2026, None, None, None).unwrap();
    let get = |c: &str| r.iter().find(|x| x.center == c).unwrap().clone();
    // Tienda: 60 % de 4.000 − 75 % de 1.200. Web: 40 % de 4.000 − 25 % de 1.200 − 300.
    assert_eq!(get("TIENDA").income, Money::from_euros(2400, 0));
    assert_eq!(get("TIENDA").expenses, Money::from_euros(900, 0));
    assert_eq!(get("WEB").result, Money::from_euros(1600 - 300 - 300, 0));
    assert!(r.iter().all(|x| !x.center.is_empty()), "todo repartido");
    // Hasta enero: sólo el alquiler.
    let jan = report(&v, 2026, None, Some(date!(2026 - 01 - 31)), None).unwrap();
    assert_eq!(jan.iter().map(|x| x.income).sum::<Money>(), Money::ZERO);

    // Presupuesto 2026 con lo real de 2025 + 10 %: 621 1.100 en marzo y 705 3.300 en marzo.
    assert_eq!(from_actuals(&v, 2026, 2025, 3, 1000, now).unwrap(), 2);
    let b = presupuestos::list(&v, 2026).unwrap();
    let alq = b.iter().find(|x| x.account == "621").unwrap();
    assert_eq!(alq.months[2], Money::from_euros(1100, 0));
    // Añadimos 629 (100 €/mes) para la web.
    presupuestos::set(
        &v,
        2026,
        &BudgetLine {
            account: "629".into(),
            center: "WEB".into(),
            months: vec![Money::from_euros(100, 0); 12],
        },
        now,
    )
    .unwrap();
    assert!(
        presupuestos::set(
            &v,
            2026,
            &BudgetLine {
                account: "57".into(),
                center: String::new(),
                months: vec![Money::ZERO; 12]
            },
            now
        )
        .is_err()
    );
    // Hasta febrero: alquiler real 1.200 sin presupuesto en ene–feb (desfavorable), ventas 4.000
    // frente a 0 (favorable), 629 web 300 frente a 200 (desfavorable, +50 %).
    let d = presupuestos::report(&v, 2026, 2).unwrap();
    let get = |a: &str| d.iter().find(|x| x.account == a).unwrap().clone();
    assert_eq!(get("621").actual, Money::from_euros(1200, 0));
    assert!(get("621").unfavourable);
    assert!(!get("705").unfavourable);
    let web = get("629");
    assert_eq!(
        (web.budget, web.actual),
        (Money::from_euros(200, 0), Money::from_euros(300, 0))
    );
    assert_eq!(web.deviation_bp, Some(5000));
    assert!(web.unfavourable);
    assert!(v.verify_audit().is_ok());
}

#[test]
fn niveles_y_reparto_secundario() {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let now = datetime!(2026-10-20 10:00 UTC);
    let c = |code: &str, parent: Option<&str>| Center {
        code: code.into(),
        name: code.into(),
        parent: parent.map(Into::into),
    };
    // NORTE → (BILBAO, VITORIA); SG (servicios generales) se reparte entre las dos tiendas.
    for (code, parent) in [
        ("NORTE", None),
        ("BILBAO", Some("NORTE")),
        ("VITORIA", Some("NORTE")),
        ("SG", None),
    ] {
        save_center(&v, &c(code, parent), now).unwrap();
    }
    assert!(
        save_center(&v, &c("NORTE", Some("BILBAO")), now).is_err(),
        "ciclo"
    );
    for (code, parent) in [("A", Some("BILBAO")), ("B", Some("A"))] {
        save_center(&v, &c(code, parent), now).unwrap();
    }
    assert!(
        save_center(&v, &c("X", Some("B")), now).is_err(),
        "quinto nivel"
    );
    for (d, de, ha, eur) in [
        (date!(2026 - 01 - 10), "621", "5720001", 1000),
        (date!(2026 - 01 - 11), "622", "5720001", 400),
        (date!(2026 - 02 - 20), "5720001", "705", 5000),
    ] {
        manual_entry(&v, &e(d, de, ha, eur), now).unwrap();
    }
    set_rule(&v, "621", &[s("BILBAO", 50), s("VITORIA", 50)], now).unwrap();
    set_rule(&v, "622", &[s("SG", 100)], now).unwrap();
    set_rule(&v, "705", &[s("BILBAO", 60), s("VITORIA", 40)], now).unwrap();
    set_redistribution(&v, "SG", &[s("BILBAO", 75), s("VITORIA", 25)], now).unwrap();
    assert!(
        set_redistribution(&v, "BILBAO", &[s("VITORIA", 100)], now).is_err(),
        "BILBAO recibe de SG: no se encadena"
    );
    let r = report(&v, 2026, None, None, None).unwrap();
    let get = |r: &[taxpain_app::analitica::CenterResult], c: &str| {
        r.iter().find(|x| x.center == c).unwrap().clone()
    };
    // Bilbao: 3.000 − 500 − 300 (75 % de SG); SG queda a cero.
    assert_eq!(get(&r, "BILBAO").result, Money::from_euros(2200, 0));
    assert_eq!(get(&r, "SG").expenses, Money::ZERO);
    // Nivel 1: NORTE suma sus tiendas; SG sale a cero.
    let r1 = report(&v, 2026, None, None, Some(1)).unwrap();
    assert_eq!(get(&r1, "NORTE").result, Money::from_euros(3600, 0));
    assert!(r1.iter().all(|x| x.center != "BILBAO"));
}
