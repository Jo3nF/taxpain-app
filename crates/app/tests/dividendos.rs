//! Acuerdos de dividendos de una SL inventada (Estudio Demo SL, B12345674): el flujo completo
//! del dividendo a cuenta (estado de liquidez → 557 a 526 → acta en borrador enlazada → pago con
//! retención → 123 del trimestre → aplicación del resultado que salda la 557), el dividendo con
//! cargo a reservas con el semáforo de los arts. 273 y 274 LSC, y el del resultado.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::dividendos::{self, AgreementInput, AgreementKind, Light};
use taxpain_app::invoicing;
use taxpain_app::libros_societarios::actas;
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::socios::{self, PayoutInput, Shareholder};
use taxpain_app::{accounting, cierre, models};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime};

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn at(d: Date) -> OffsetDateTime {
    d.with_hms(10, 0, 0).unwrap().assume_utc()
}

fn setup() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Estudio Demo SL".into(),
            address: Some(taxpain_core::invoice::Address {
                street: "Calle Inventada 1".into(),
                post_code: "46001".into(),
                town: "València".into(),
                province: "Valencia".into(),
                country: "ES".into(),
            }),
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

fn entry(v: &Vault, d: Date, lines: &[(&str, i64, i64)]) {
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
            concept: "Asiento de prueba".into(),
            origin: Origin::Manual,
            lines,
        },
        at(d),
    )
    .unwrap();
}

fn balance(v: &Vault, year: i32, account: &str) -> Money {
    let cents: i64 = v
        .connection()
        .query_row(
            "SELECT COALESCE(SUM(l.debit - l.credit), 0) FROM journal_line l
               JOIN journal_entry e ON e.id = l.entry_id
              WHERE e.fiscal_year = ?1 AND l.account LIKE ?2 || '%'
                AND e.origin_kind <> 'Cierre'",
            (year, account),
            |r| r.get(0),
        )
        .unwrap();
    Money::from_cents(cents)
}

fn holder(nif: &str, name: &str, pct: u32) -> Shareholder {
    Shareholder {
        id: 0,
        nif: nif.into(),
        name: name.into(),
        address: "Calle Inventada 2, València".into(),
        province: "46".into(),
        country: "ES".into(),
        pct,
        legal_person: false,
        resident: true,
        exempt: false,
        active: true,
    }
}

fn input(kind: AgreementKind, d: Date, amount: i64) -> AgreementInput {
    AgreementInput {
        kind,
        date: d,
        amount: eur(amount),
        payment_date: None,
        statement_date: None,
        tax_rate: None,
        reserve_account: None,
        year: None,
        organ: None,
    }
}

fn light(c: &dividendos::AgreementCheck, code: &str) -> Light {
    c.checks
        .iter()
        .find(|x| x.code == code)
        .unwrap_or_else(|| panic!("falta {code}: {:?}", c.checks))
        .light
}

fn payout(d: Date, gross: i64) -> PayoutInput {
    PayoutInput {
        date: d,
        gross: eur(gross),
        treasury: "572".into(),
        agreement: None,
    }
}

#[test]
fn dividendo_a_cuenta_de_principio_a_fin() {
    let v = setup();
    let now = at(date!(2026 - 10 - 01));
    socios::save_shareholder(
        &v,
        &holder("12345678Z", "Carmen Española Española", 600_000),
        now,
    )
    .unwrap();
    socios::save_shareholder(
        &v,
        &holder("00000000T", "Socio Dos Inventado", 400_000),
        now,
    )
    .unwrap();
    // Capital 10.000, reserva legal ya al 20 % y voluntarias 38.000; 40.000 de beneficio hasta
    // septiembre.
    entry(
        &v,
        date!(2026 - 01 - 01),
        &[
            ("572", 50_000, 0),
            ("100", 0, 10_000),
            ("112", 0, 2_000),
            ("113", 0, 38_000),
        ],
    );
    entry(
        &v,
        date!(2026 - 06 - 30),
        &[("572", 40_000, 0), ("705", 0, 40_000)],
    );

    let mut i = input(AgreementKind::ACuenta, date!(2026 - 10 - 15), 20_000);
    i.statement_date = Some(date!(2026 - 09 - 30));
    i.payment_date = Some(date!(2026 - 10 - 20));
    let c = dividendos::check(&v, &i).unwrap();
    let l = c.liquidity.clone().unwrap();
    assert_eq!(l.result, eur(40_000));
    assert_eq!(l.legal_reserve, Money::ZERO, "ya dotada");
    assert_eq!(l.estimated_tax, eur(10_000));
    assert_eq!(l.limit, eur(30_000));
    assert!(!c.blocked, "{:?}", c.checks);
    for code in [
        "reserva_legal",
        "patrimonio_neto",
        "investigacion",
        "limite_277",
        "liquidez_277",
        "socios",
    ] {
        assert_eq!(light(&c, code), Light::Verde, "{code}: {:?}", c.checks);
    }
    // Todo el activo es tesorería sin justificar: puede ser patrimonial (aviso, no bloqueo).
    assert_eq!(light(&c, "entidad_patrimonial"), Light::Ambar);
    assert_eq!(c.entry[0].account, "557");
    assert_eq!(c.withheld, eur(3_800));

    // Por encima del límite del art. 277: rojo y no se contabiliza.
    let mut big = i.clone();
    big.amount = eur(35_000);
    let cb = dividendos::check(&v, &big).unwrap();
    assert!(cb.blocked);
    assert_eq!(light(&cb, "limite_277"), Light::Rojo);
    let e = dividendos::record(&v, &big, now, true)
        .unwrap_err()
        .to_string();
    assert!(e.contains("277") || e.contains("máxima"), "{e}");
    assert!(
        dividendos::record(&v, &i, now, false).is_err(),
        "sin confirmar"
    );

    // Se contabiliza: 557 a 526 y el acta en borrador enlazada.
    let r = dividendos::record(&v, &i, now, true).unwrap();
    assert_eq!(balance(&v, 2026, "557"), eur(20_000));
    assert_eq!(balance(&v, 2026, "526"), -eur(20_000));
    let m = actas::minute(&v, r.minute_id).unwrap();
    assert_eq!(m.template, "dividendo_cuenta");
    assert_eq!(m.dividend_agreement, Some(r.id));
    assert_eq!(m.date, date!(2026 - 10 - 15));
    assert!(m.agreements.contains("estado contable"), "{}", m.agreements);
    // Un segundo a cuenta ve lo ya repartido.
    let mut again = i.clone();
    again.amount = eur(15_000);
    let c2 = dividendos::check(&v, &again).unwrap();
    assert_eq!(
        c2.liquidity.as_ref().unwrap().already_distributed,
        eur(20_000)
    );
    assert_eq!(light(&c2, "limite_277"), Light::Rojo);

    // El pago se liga al acuerdo; se avisa mientras el acta está en borrador.
    let p = socios::propose(&v, &payout(date!(2026 - 10 - 20), 20_000)).unwrap();
    assert_eq!(p.agreement.as_ref().map(|a| a.id), Some(r.id));
    assert!(
        p.warnings.iter().any(|w| w.contains("acta")),
        "{:?}",
        p.warnings
    );
    actas::approve_minute(&v, r.minute_id, now, true).unwrap();
    let p = socios::propose(&v, &payout(date!(2026 - 10 - 20), 20_000)).unwrap();
    assert!(
        !p.warnings.iter().any(|w| w.contains("acta")),
        "{:?}",
        p.warnings
    );
    let early = socios::pay(&v, &payout(date!(2026 - 10 - 10), 20_000), now, true);
    assert!(early.is_err(), "antes del acuerdo no lo coge");
    let pay = socios::pay(&v, &payout(date!(2026 - 10 - 20), 20_000), now, true).unwrap();
    let paid = socios::payments_of_year(&v, 2026).unwrap();
    assert_eq!(paid[0].id, pay);
    assert_eq!(paid[0].agreement_id, Some(r.id));
    assert_eq!(balance(&v, 2026, "4751"), -eur(3_800));
    let a = dividendos::agreements(&v).unwrap();
    assert_eq!(a[0].paid, eur(20_000));
    assert_eq!(a[0].pending, Money::ZERO);
    assert!(a[0].minute_approved && a[0].minute_number == Some(1));
    // La retención va al 123 del cuarto trimestre.
    let d = models::draft(&v, "123", 2026, 4).unwrap();
    assert_eq!(d.result, eur(3_800));
    let e = dividendos::undo(&v, r.id, now, true)
        .unwrap_err()
        .to_string();
    assert!(e.contains("pagos"), "{e}");

    // Al aprobar las cuentas, la aplicación del resultado salda la 557.
    accounting::close_year(&v, 2026, at(date!(2027 - 03 - 31)), true).unwrap();
    let al = cierre::allocation(&v, 2026).unwrap();
    assert_eq!(al.interim_dividend, eur(20_000));
    let split = cierre::Allocation {
        legal: Money::ZERO,
        losses: Money::ZERO,
        voluntary: eur(15_000),
        dividends: eur(25_000),
        remanente: Money::ZERO,
        capitalization: Money::ZERO,
    };
    cierre::post_allocation(
        &v,
        2026,
        date!(2027 - 05 - 15),
        &split,
        at(date!(2027 - 05 - 15)),
        true,
    )
    .unwrap();
    assert_eq!(balance(&v, 2027, "557"), Money::ZERO, "557 saldada");
    assert_eq!(balance(&v, 2027, "526"), -eur(5_000));
    let e = dividendos::undo(&v, r.id, now, true)
        .unwrap_err()
        .to_string();
    assert!(e.contains("pagos"), "{e}");

    // El dividendo complementario del resultado: se registra el acuerdo con el acta de cuentas.
    let mut res = input(AgreementKind::Resultado, date!(2027 - 05 - 15), 0);
    res.year = Some(2026);
    let cr = dividendos::check(&v, &res).unwrap();
    assert!(!cr.blocked, "{:?}", cr.checks);
    assert_eq!(cr.amount, eur(5_000));
    assert!(cr.entry.is_empty());
    let rr = dividendos::record(&v, &res, now, true).unwrap();
    assert_eq!(actas::minute(&v, rr.minute_id).unwrap().template, "cuentas");
    assert!(
        dividendos::check(&v, &res).is_err(),
        "no se registra dos veces"
    );
    let e = cierre::undo_allocation(&v, 2026, now, true)
        .unwrap_err()
        .to_string();
    assert!(e.contains("acuerdo"), "{e}");
    socios::pay(&v, &payout(date!(2027 - 06 - 01), 5_000), now, true).unwrap();
    let paid = socios::payments_of_year(&v, 2027).unwrap();
    assert_eq!(paid[0].agreement_id, Some(rr.id), "el del resultado");
}

#[test]
fn dividendo_con_cargo_a_reservas_y_sus_limites() {
    let v = setup();
    let now = at(date!(2026 - 03 - 01));
    socios::save_shareholder(
        &v,
        &holder("12345678Z", "Carmen Española Española", 1_000_000),
        now,
    )
    .unwrap();
    // Capital 10.000, voluntarias 5.000, pérdidas anteriores 4.000: patrimonio neto 11.000.
    entry(
        &v,
        date!(2026 - 01 - 01),
        &[
            ("572", 11_000, 0),
            ("121", 4_000, 0),
            ("100", 0, 10_000),
            ("113", 0, 5_000),
        ],
    );
    let d = date!(2026 - 03 - 01);
    let c = dividendos::check(&v, &input(AgreementKind::Reservas, d, 3_000)).unwrap();
    assert!(c.blocked);
    assert_eq!(light(&c, "patrimonio_neto"), Light::Rojo, "{:?}", c.checks);
    assert_eq!(light(&c, "reservas_libres"), Light::Verde);
    // Sin reserva legal dotada: aviso (las voluntarias sí se reparten).
    assert_eq!(light(&c, "reserva_legal"), Light::Ambar);
    let c = dividendos::check(&v, &input(AgreementKind::Reservas, d, 6_000)).unwrap();
    assert_eq!(light(&c, "reservas_libres"), Light::Rojo);
    let ok = input(AgreementKind::Reservas, d, 1_000);
    let c = dividendos::check(&v, &ok).unwrap();
    assert!(!c.blocked, "{:?}", c.checks);
    assert_eq!(c.entry[0].account, "113");
    // Socio único: el acta es una decisión del socio único.
    assert_eq!(c.organ, actas::MinuteKind::SocioUnico);

    // Con gastos de investigación activados por encima de las reservas que quedan: rojo.
    entry(
        &v,
        date!(2026 - 02 - 01),
        &[("200", 4_500, 0), ("572", 0, 4_500)],
    );
    let c = dividendos::check(&v, &ok).unwrap();
    assert_eq!(light(&c, "investigacion"), Light::Rojo, "{:?}", c.checks);
    assert!(dividendos::record(&v, &ok, now, true).is_err());
    entry(
        &v,
        date!(2026 - 02 - 02),
        &[("572", 4_500, 0), ("200", 0, 4_500)],
    );

    // Se contabiliza 113 a 526, con su acta; deshacerlo quita el asiento y el borrador.
    let r = dividendos::record(&v, &ok, now, true).unwrap();
    assert_eq!(balance(&v, 2026, "113"), -eur(4_000));
    assert_eq!(balance(&v, 2026, "526"), -eur(1_000));
    let m = actas::minute(&v, r.minute_id).unwrap();
    assert_eq!(m.template, "dividendo");
    assert_eq!(m.kind, actas::MinuteKind::SocioUnico);
    dividendos::undo(&v, r.id, now, true).unwrap();
    assert_eq!(balance(&v, 2026, "526"), Money::ZERO);
    assert!(actas::minute(&v, r.minute_id).is_err(), "borrador borrado");
    assert!(dividendos::agreements(&v).unwrap().is_empty());

    // El órgano de administración también puede acordar (a cuenta): se respeta el elegido.
    let mut by_admin = ok.clone();
    by_admin.organ = Some(actas::MinuteKind::Administrador);
    let r = dividendos::record(&v, &by_admin, now, true).unwrap();
    assert_eq!(
        actas::minute(&v, r.minute_id).unwrap().kind,
        actas::MinuteKind::Administrador
    );
    // Un pago sin acuerdo pendiente avisa.
    socios::pay(&v, &payout(date!(2026 - 03 - 10), 1_000), now, true).unwrap();
    entry(
        &v,
        date!(2026 - 03 - 11),
        &[("113", 500, 0), ("526", 0, 500)],
    );
    let p = socios::propose(&v, &payout(date!(2026 - 03 - 12), 500)).unwrap();
    assert!(p.agreement.is_none());
    assert!(
        p.warnings.iter().any(|w| w.contains("ningún acuerdo")),
        "{:?}",
        p.warnings
    );
}

#[test]
fn sin_beneficios_no_hay_dividendo_a_cuenta() {
    let v = setup();
    let now = at(date!(2026 - 06 - 01));
    socios::save_shareholder(
        &v,
        &holder("12345678Z", "Carmen Española Española", 1_000_000),
        now,
    )
    .unwrap();
    entry(
        &v,
        date!(2026 - 01 - 01),
        &[("572", 30_000, 0), ("100", 0, 10_000), ("113", 0, 20_000)],
    );
    entry(
        &v,
        date!(2026 - 04 - 01),
        &[("629", 1_000, 0), ("572", 0, 1_000)],
    );
    let c = dividendos::check(
        &v,
        &input(AgreementKind::ACuenta, date!(2026 - 06 - 01), 1_000),
    )
    .unwrap();
    assert!(c.blocked);
    assert_eq!(light(&c, "limite_277"), Light::Rojo);
    // El estado contable no puede ser posterior al acuerdo ni de otro ejercicio.
    let mut late = input(AgreementKind::ACuenta, date!(2026 - 06 - 01), 1_000);
    late.statement_date = Some(date!(2026 - 06 - 02));
    assert!(dividendos::check(&v, &late).is_err());
    late.statement_date = Some(date!(2025 - 12 - 31));
    assert!(dividendos::check(&v, &late).is_err());
}
