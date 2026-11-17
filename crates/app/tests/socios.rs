//! Socios y dividendos de una SL: reparto por participación, retención del 19 % (sin ella para
//! una sociedad con la exención del art. 21 LIS), asiento 526 / 4751 / 572, modelos 123 y 193,
//! y que nada de esto se cuele en el 111 ni en las retenciones del 200.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::invoicing;
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::socios::{self, PayoutInput, Shareholder};
use taxpain_app::{AppError, accounting, models};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn setup() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("B98765431".into()),
            name: "Estudio Digital Ejemplo SL".into(),
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
    invoicing::save_settings(&v, &s, datetime!(2026-01-01 9:00 UTC)).unwrap();
    v
}

fn entry(v: &Vault, d: time::Date, concept: &str, lines: &[(&str, i64, i64)]) {
    let lines = lines
        .iter()
        .map(|(a, debit, credit)| {
            if *debit > 0 {
                EntryLine::debit(acc(a), eur(*debit))
            } else {
                EntryLine::credit(acc(a), eur(*credit))
            }
        })
        .collect();
    accounting::manual_entry(
        v,
        &JournalEntry {
            date: d,
            concept: concept.into(),
            origin: Origin::Manual,
            lines,
        },
        datetime!(2026-06-01 10:00 UTC),
    )
    .unwrap();
}

fn balance(v: &Vault, account: &str) -> Money {
    let cents: i64 = v
        .connection()
        .query_row(
            "SELECT COALESCE(SUM(l.debit - l.credit), 0) FROM journal_line l WHERE l.account = ?1",
            [account],
            |r| r.get(0),
        )
        .unwrap();
    Money::from_cents(cents)
}

fn get(d: &models::Draft, b: u16) -> Money {
    d.boxes
        .iter()
        .find(|(k, _)| *k == b)
        .map_or(Money::ZERO, |(_, m)| *m)
}

fn holder(nif: &str, name: &str, pct: u32) -> Shareholder {
    Shareholder {
        id: 0,
        nif: nif.into(),
        name: name.into(),
        address: "Calle Mayor 1, València".into(),
        province: "46".into(),
        country: "ES".into(),
        pct,
        legal_person: false,
        resident: true,
        exempt: false,
        active: true,
    }
}

#[test]
fn dividendos_123_y_193() {
    let v = setup();
    let now = datetime!(2026-07-01 10:00 UTC);

    // Ficha de socios: comprobaciones.
    let mut bad = holder("12345678Z", "Socia Uno", 600_000);
    bad.legal_person = true;
    assert!(
        socios::save_shareholder(&v, &bad, now).is_err(),
        "NIF de persona física"
    );
    let mut bad = holder("12345678Z", "Socia Uno", 600_000);
    bad.province = "99".into();
    assert!(
        socios::save_shareholder(&v, &bad, now).is_err(),
        "provincia"
    );
    let mut bad = holder("12345678Z", "Socia Uno", 600_000);
    bad.exempt = true;
    assert!(
        socios::save_shareholder(&v, &bad, now).is_err(),
        "exención sólo sociedades"
    );

    socios::save_shareholder(&v, &holder("12345678Z", "Española Uno Ana", 600_000), now).unwrap();
    socios::save_shareholder(&v, &holder("00000000T", "Española Dos Luis", 150_000), now).unwrap();
    let mut sl = holder("B87654323", "Matriz Ejemplo SL", 250_000);
    sl.legal_person = true;
    sl.exempt = true;
    socios::save_shareholder(&v, &sl, now).unwrap();
    assert_eq!(socios::shareholders(&v).unwrap().len(), 3);

    // Sin dividendo acordado no se puede pagar.
    let input = PayoutInput {
        date: date!(2026 - 07 - 15),
        gross: eur(10_000),
        treasury: "5720001".into(),
        agreement: None,
    };
    let p = socios::propose(&v, &input).unwrap();
    assert_eq!(p.available, Money::ZERO);
    assert!(p.warnings.iter().any(|w| w.contains("526")));
    assert!(socios::pay(&v, &input, now, true).is_err());

    // La junta acuerda 10.000 € de dividendos (aquí, con cargo a reservas).
    entry(
        &v,
        date!(2026 - 06 - 30),
        "Reparto de reservas acordado por la junta",
        &[("113", 10_000, 0), ("526", 0, 10_000)],
    );
    let p = socios::propose(&v, &input).unwrap();
    assert_eq!(p.available, eur(10_000));
    let by_nif = |nif: &str| p.lines.iter().find(|l| l.nif == nif).unwrap().clone();
    assert_eq!(by_nif("12345678Z").gross, eur(6000));
    assert_eq!(by_nif("12345678Z").withheld, eur(1140));
    assert_eq!(by_nif("00000000T").withheld, eur(285));
    assert_eq!(by_nif("B87654323").gross, eur(2500));
    assert_eq!(by_nif("B87654323").withheld, Money::ZERO);
    assert_eq!(p.withheld, eur(1425));
    assert_eq!(p.net, eur(8575));
    assert_eq!(p.entry.len(), 3);
    assert!(p.warnings.iter().any(|w| w.contains("art. 21 LIS")));
    assert!(matches!(
        socios::pay(&v, &input, now, false),
        Err(AppError::NotConfirmed)
    ));
    let too_much = PayoutInput {
        gross: eur(10_001),
        ..input.clone()
    };
    assert!(socios::pay(&v, &too_much, now, true).is_err());

    let id = socios::pay(&v, &input, now, true).unwrap();
    assert_eq!(balance(&v, "526"), Money::ZERO);
    assert_eq!(balance(&v, "4751"), -eur(1425));
    assert_eq!(balance(&v, "5720001"), -eur(8575));
    let journal = accounting::journal(&v, 2026).unwrap();
    let e = journal
        .iter()
        .find(|e| e.entry.origin == Origin::Dividendo(id))
        .unwrap();
    assert_eq!(e.entry.concept, "Pago de dividendos");
    let pays = socios::payments_of_year(&v, 2026).unwrap();
    assert_eq!(pays.len(), 1);
    assert_eq!(pays[0].lines.len(), 3);

    // 123 del 3T: dos rentas con retención (la sociedad exenta no va).
    let d = models::draft(&v, "123", 2026, 3).unwrap();
    assert!(d.payees.contains(&(1, 2)), "{:?}", d.payees);
    assert_eq!(get(&d, 4), eur(7500));
    assert_eq!(get(&d, 7), eur(1425));
    assert_eq!(get(&d, 14), eur(1425));
    assert_eq!(d.result, eur(1425));
    assert_eq!(d.declaration, "I");
    assert!(d.file.starts_with("<T123020263T0000>"));
    assert!(d.file.contains("B98765431"));
    assert_eq!(models::period_code("123", 3), "3T");
    // El 2T no tiene rentas: no hay que presentarlo.
    let d2 = models::draft(&v, "123", 2026, 2).unwrap();
    assert_eq!(d2.result, Money::ZERO);
    assert!(
        d2.warnings
            .iter()
            .any(|w| w.contains("no hay que presentar"))
    );

    // Nada en el 111 ni en las retenciones del 200.
    let d111 = models::draft(&v, "111", 2026, 3).unwrap();
    assert_eq!(d111.result, Money::ZERO);
    assert_eq!(get(&d111, 3), Money::ZERO);
    let d200 = models::draft(&v, "200", 2026, 0).unwrap();
    assert_eq!(get(&d200, 1766), Money::ZERO);

    // 193: tres registros (dos con retención y la sociedad como no sometida a retención).
    let d193 = models::draft(&v, "193", 2026, 0).unwrap();
    assert_eq!(models::period_code("193", 0), "0A");
    assert_eq!(d193.result, eur(1425));
    assert_eq!(d193.details.len(), 3);
    assert!(
        d193.details
            .iter()
            .any(|(t, m)| t.contains("B87654323") && t.contains("naturaleza 08") && m.is_zero())
    );
    assert!(
        !d193.warnings.iter().any(|w| w.contains("no cuadran")),
        "{:?}",
        d193.warnings
    );
    let recs: Vec<&str> = d193.file.split("\r\n").collect();
    assert_eq!(recs.len(), 4);
    assert!(recs.iter().all(|r| r.chars().count() == 500));
    assert!(recs[0].starts_with("1193"));

    // Deshacer: vuelve el saldo de la 526.
    assert!(matches!(
        socios::undo(&v, id, now, false),
        Err(AppError::NotConfirmed)
    ));
    socios::undo(&v, id, now, true).unwrap();
    assert_eq!(balance(&v, "526"), -eur(10_000));
    assert!(socios::payments_of_year(&v, 2026).unwrap().is_empty());
    assert_eq!(
        models::draft(&v, "123", 2026, 3).unwrap().result,
        Money::ZERO
    );
}

#[test]
fn socio_no_residente_fuera_del_123() {
    let v = setup();
    let now = datetime!(2026-07-01 10:00 UTC);
    socios::save_shareholder(&v, &holder("12345678Z", "Española Uno Ana", 500_000), now).unwrap();
    let foreign = Shareholder {
        nif: "FR12345678901".into(),
        name: "Investisseur Exemple".into(),
        province: String::new(),
        country: "FR".into(),
        resident: false,
        ..holder("x", "x", 500_000)
    };
    socios::save_shareholder(&v, &foreign, now).unwrap();
    entry(
        &v,
        date!(2026 - 04 - 30),
        "Dividendo acordado",
        &[("113", 2000, 0), ("526", 0, 2000)],
    );
    let input = PayoutInput {
        date: date!(2026 - 05 - 10),
        gross: eur(2000),
        treasury: "572".into(),
        agreement: None,
    };
    let p = socios::propose(&v, &input).unwrap();
    assert!(p.warnings.iter().any(|w| w.contains("216")));
    assert_eq!(p.withheld, eur(380));
    socios::pay(&v, &input, now, true).unwrap();
    let d = models::draft(&v, "123", 2026, 2).unwrap();
    assert_eq!(d.result, eur(190), "sólo la socia residente");
    assert!(d.warnings.iter().any(|w| w.contains("216")));
    let d193 = models::draft(&v, "193", 2026, 0).unwrap();
    assert_eq!(d193.details.len(), 1);
}

#[test]
fn reparto_con_centimos_y_autocartera() {
    let a = Shareholder {
        id: 1,
        ..holder("12345678Z", "A", 333_333)
    };
    let b = Shareholder {
        id: 2,
        ..holder("00000000T", "B", 333_333)
    };
    let c = Shareholder {
        id: 3,
        ..holder("11111111H", "C", 333_334)
    };
    let lines = socios::split(Money::from_cents(100_001), &[a, b, c]);
    let total: Money = lines.iter().map(|l| l.gross).sum();
    assert_eq!(total, Money::from_cents(100_001));
    // Los céntimos del redondeo, al de más participación.
    assert_eq!(lines[2].gross, Money::from_cents(33_335));
    assert_eq!(lines[0].gross, Money::from_cents(33_333));
}

fn payout(d: time::Date, gross: i64) -> PayoutInput {
    PayoutInput {
        date: d,
        gross: eur(gross),
        treasury: "572".into(),
        agreement: None,
    }
}

/// Un pago con fecha anterior a otro ya contabilizado no puede volver a llevarse lo que ya se
/// pagó de la 526.
#[test]
fn pagos_fuera_de_orden_no_pasan_de_la_526() {
    let v = setup();
    let now = datetime!(2026-07-01 10:00 UTC);
    socios::save_shareholder(&v, &holder("12345678Z", "Uno Ana", 1_000_000), now).unwrap();
    entry(
        &v,
        date!(2026 - 06 - 30),
        "Dividendo acordado",
        &[("113", 10_000, 0), ("526", 0, 10_000)],
    );
    socios::pay(&v, &payout(date!(2026 - 12 - 15), 6_000), now, true).unwrap();
    // El 15/07 la 526 aún tenía 10.000 €, pero 6.000 ya se pagaron (con fecha de diciembre).
    let p = socios::propose(&v, &payout(date!(2026 - 07 - 15), 10_000)).unwrap();
    assert_eq!(p.available, eur(4_000));
    let err = socios::pay(&v, &payout(date!(2026 - 07 - 15), 10_000), now, true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("4.000,00"), "{err}");
    socios::pay(&v, &payout(date!(2026 - 07 - 15), 4_000), now, true).unwrap();
    assert_eq!(balance(&v, "526"), Money::ZERO);
}

/// Exención del art. 21 LIS marcada con menos del 5 % directo: se avisa (la indirecta cuenta).
#[test]
fn exencion_con_menos_del_5_por_ciento_avisa() {
    let v = setup();
    let now = datetime!(2026-07-01 10:00 UTC);
    let mut sl = holder("B87654323", "Matriz Ejemplo SL", 30_000);
    sl.legal_person = true;
    sl.exempt = true;
    socios::save_shareholder(&v, &sl, now).unwrap();
    socios::save_shareholder(&v, &holder("12345678Z", "Uno Ana", 970_000), now).unwrap();
    entry(
        &v,
        date!(2026 - 06 - 30),
        "Dividendo acordado",
        &[("113", 10_000, 0), ("526", 0, 10_000)],
    );
    let p = socios::propose(&v, &payout(date!(2026 - 07 - 15), 10_000)).unwrap();
    assert!(
        p.warnings
            .iter()
            .any(|w| w.contains("Matriz Ejemplo SL") && w.contains("del 3 %") && w.contains("5 %")),
        "{:?}",
        p.warnings
    );
    // Con el 5 % o más, no.
    let mut sl = socios::shareholders(&v)
        .unwrap()
        .into_iter()
        .find(|s| s.legal_person)
        .unwrap();
    sl.pct = 50_000;
    socios::save_shareholder(&v, &sl, now).unwrap();
    let p = socios::propose(&v, &payout(date!(2026 - 07 - 15), 10_000)).unwrap();
    assert!(!p.warnings.iter().any(|w| w.contains("art. 21.1.a")));
}

/// El 123 presentado (el mismo fichero que prepara TaxPain) no da diferencias: el «número de
/// rentas» [01]–[03] es un recuento, no un importe.
#[test]
fn comparar_123_presentado_sin_diferencias() {
    use taxpain_app::presentados;
    let v = setup();
    let now = datetime!(2026-07-01 10:00 UTC);
    socios::save_shareholder(&v, &holder("12345678Z", "Uno Ana", 600_000), now).unwrap();
    socios::save_shareholder(&v, &holder("00000000T", "Dos Luis", 400_000), now).unwrap();
    entry(
        &v,
        date!(2026 - 06 - 30),
        "Dividendo acordado",
        &[("113", 10_000, 0), ("526", 0, 10_000)],
    );
    socios::pay(&v, &payout(date!(2026 - 07 - 15), 10_000), now, true).unwrap();
    let d = models::draft(&v, "123", 2026, 3).unwrap();
    let r = presentados::read_file(&d.file).unwrap();
    assert!(
        !r.boxes.contains_key(&1) && !r.boxes.contains_key(&3),
        "{:?}",
        r.boxes
    );
    let id = presentados::store(&v, "123.txt", d.file.as_bytes(), &r, now).unwrap();
    let c = presentados::compare(&v, id).unwrap();
    assert_eq!(c.mismatches, 0, "{:?}", c.rows);
}

/// Cada pago de dividendos queda ligado al reparto del que sale (la última aplicación del
/// resultado contabilizada a su fecha), y deshacer una aplicación sólo lo bloquean los pagos de
/// ESE reparto: deshecho el pago del reparto de 2027, su aplicación se deshace aunque siga
/// pagado el dividendo de 2026.
#[test]
fn deshacer_la_aplicacion_solo_lo_bloquean_sus_propios_dividendos() {
    use taxpain_app::cierre;
    let v = setup();
    let now = datetime!(2028-07-01 10:00 UTC);
    socios::save_shareholder(&v, &holder("12345678Z", "Socia Única", 1_000_000), now).unwrap();
    let aplicacion = |year: i64, d: time::Date, euros: i64| {
        accounting::record_entry(
            v.connection(),
            &JournalEntry {
                date: d,
                concept: format!("Aplicación del resultado {year}"),
                origin: Origin::AplicacionResultado(year),
                lines: vec![
                    EntryLine::debit(acc("129"), eur(euros)),
                    EntryLine::credit(acc("526"), eur(euros)),
                ],
            },
            now,
        )
        .unwrap();
    };
    aplicacion(2026, date!(2027 - 06 - 30), 10_000);
    aplicacion(2027, date!(2028 - 06 - 30), 5_000);
    let pago_2026 = socios::pay(&v, &payout(date!(2027 - 07 - 15), 10_000), now, true).unwrap();
    let pago_2027 = socios::pay(&v, &payout(date!(2028 - 07 - 15), 5_000), now, true).unwrap();
    let reparto = |id: i64| -> Option<i64> {
        v.connection()
            .query_row(
                "SELECT allocation_year FROM dividend_payment WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap()
    };
    assert_eq!(reparto(pago_2026), Some(2026));
    assert_eq!(reparto(pago_2027), Some(2027));

    // Las dos aplicaciones tienen dividendos pagados: ninguna se deshace.
    for y in [2026, 2027] {
        let e = cierre::undo_allocation(&v, y, now, true)
            .unwrap_err()
            .to_string();
        assert!(e.contains("dividendos pagados"), "{y}: {e}");
    }
    // Deshecho el pago del reparto de 2027, su aplicación se deshace; la de 2026 sigue
    // bloqueada por su propio pago (aunque sea anterior al de 2027).
    socios::undo(&v, pago_2027, now, true).unwrap();
    cierre::undo_allocation(&v, 2027, now, true).unwrap();
    let e = cierre::undo_allocation(&v, 2026, now, true)
        .unwrap_err()
        .to_string();
    assert!(e.contains("dividendos pagados"), "{e}");
    socios::undo(&v, pago_2026, now, true).unwrap();
    cierre::undo_allocation(&v, 2026, now, true).unwrap();
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
