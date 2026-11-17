//! Cierre guiado de una SL: lista de comprobaciones, asiento del IS desde el 200, cierre y
//! aplicación del resultado con la reserva legal (art. 274 LSC).

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::invoicing::{self, InvoiceDraft};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::{accounting, cierre, models};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime, Time};

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn at(d: Date) -> OffsetDateTime {
    d.with_time(Time::from_hms(10, 0, 0).unwrap()).assume_utc()
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
    invoicing::save_settings(&v, &s, datetime!(2026-01-01 9:00 UTC)).unwrap();
    v
}

fn entry(v: &Vault, d: Date, concept: &str, lines: &[(&str, i64, i64)]) {
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
              WHERE e.fiscal_year = ?1 AND l.account = ?2",
            taxpain_vault::rusqlite::params![year, account],
            |r| r.get(0),
        )
        .unwrap();
    Money::from_cents(cents)
}

#[test]
fn cierre_de_una_sl_con_is_y_aplicacion() {
    let v = setup();
    // Capital de 3.000 €, una venta de 10.000 € y 2.000 € de gastos.
    entry(
        &v,
        date!(2026 - 01 - 01),
        "Capital",
        &[("5720001", 3000, 0), ("100", 0, 3000)],
    );
    invoicing::issue(
        &v,
        &InvoiceDraft {
            kind: InvoiceKind::Completa,
            series: None,
            issue_date: date!(2026 - 03 - 10),
            operation_date: None,
            recipient: Some(Party {
                id: PartyId::Nif("B12345674".into()),
                name: "Cliente SL".into(),
                address: addr(),
                person: None,
            }),
            lines: vec![InvoiceLine {
                description: "Servicio".into(),
                quantity_milli: 1000,
                unit_price: eur(10_000),
                discount: Rate::ZERO,
                vat: VatTreatment::Taxed {
                    rate: Rate::percent(21),
                },
                surcharge: None,
                withholding: None,
                advance: false,
            }],
            rectified: vec![],
            rectified_amounts: None,
            description: "Servicio".into(),
            notes: None,
            currency: None,
            oss: None,
        },
        at(date!(2026 - 03 - 10)),
        true,
    )
    .unwrap();
    entry(
        &v,
        date!(2026 - 06 - 30),
        "Gastos",
        &[("629", 2000, 0), ("5720001", 0, 2000)],
    );

    let c = cierre::checklist(&v, 2026).unwrap();
    assert!(c.company && !c.closed);
    let item = |c: &cierre::Checklist, id: &str| c.items.iter().find(|i| i.id == id).unwrap().ok;
    assert!(!item(&c, "is"), "IS sin contabilizar");
    assert!(!item(&c, "iva"), "sin liquidación del IVA");
    assert!(c.deadlines.iter().any(|d| d.date == date!(2027 - 06 - 30)));

    // IS desde el 200: 6300 al debe; sin pagos a cuenta, todo a la 4752.
    let p = cierre::corporate_tax(&v, 2026).unwrap();
    assert!(p.tax.cents() > 0);
    assert_eq!(p.tax, p.withholdings + p.installments + p.result);
    let debit: Money = p.lines.iter().map(|l| l.debit).sum();
    let credit: Money = p.lines.iter().map(|l| l.credit).sum();
    assert_eq!(debit, credit);
    assert!(cierre::post_corporate_tax(&v, 2026, at(date!(2027 - 01 - 15)), false).is_err());
    cierre::post_corporate_tax(&v, 2026, at(date!(2027 - 01 - 15)), true).unwrap();
    assert!(
        cierre::post_corporate_tax(&v, 2026, at(date!(2027 - 01 - 15)), true).is_err(),
        "no dos veces"
    );
    assert_eq!(balance(&v, 2026, "6300"), p.tax);
    assert_eq!(balance(&v, 2026, "4752"), -p.result);
    // El 200 lo ve como gasto por IS: misma base, misma cuota.
    let d = models::draft(&v, "200", 2026, 0).unwrap();
    let get = |b: u16| {
        d.boxes
            .iter()
            .find(|(k, _)| *k == b)
            .map_or(Money::ZERO, |(_, m)| *m)
    };
    assert_eq!(get(301), p.tax);
    assert_eq!(get(592), p.tax);
    assert!(item(&cierre::checklist(&v, 2026).unwrap(), "is"));
    // Si el 200 cambia (una multa de 1.000 €, no deducible), el asiento contabilizado se queda
    // atrás: se avisa y la comprobación deja de estar en verde.
    let mut years = std::collections::BTreeMap::new();
    years.insert(
        2026,
        models::YearSettings {
            corporate_fines: eur(1000),
            ..models::YearSettings::default()
        },
    );
    models::save_settings(
        &v,
        &models::ModelSettings {
            years,
            ..models::ModelSettings::default()
        },
        at(date!(2027 - 01 - 16)),
    )
    .unwrap();
    let stale = cierre::corporate_tax(&v, 2026).unwrap();
    assert!(stale.stale && stale.warnings.iter().any(|w| w.contains("ha cambiado")));
    assert!(!item(&cierre::checklist(&v, 2026).unwrap(), "is"));
    models::save_settings(
        &v,
        &models::ModelSettings::default(),
        at(date!(2027 - 01 - 16)),
    )
    .unwrap();
    assert!(!cierre::corporate_tax(&v, 2026).unwrap().stale);

    // Aplicación: sólo con el ejercicio cerrado.
    assert!(cierre::allocation(&v, 2026).is_err());
    accounting::close_year(&v, 2026, at(date!(2027 - 03 - 31)), true).unwrap();
    assert!(
        cierre::undo_corporate_tax(&v, 2026, at(date!(2027 - 04 - 01)), true).is_err(),
        "ejercicio cerrado"
    );
    let a = cierre::allocation(&v, 2026).unwrap();
    let profit = eur(8000) - p.tax;
    assert_eq!(a.result, profit);
    assert_eq!(a.capital, eur(3000));
    // 10 % del beneficio, pero sólo hasta el 20 % del capital (600 €).
    assert_eq!(a.legal_minimum, eur(600));
    assert_eq!(a.suggested.legal, eur(600));
    assert_eq!(a.suggested.voluntary, profit - eur(600));
    assert_eq!(a.meeting_deadline, date!(2027 - 06 - 30));

    let mut split = a.suggested;
    split.legal = eur(500);
    split.voluntary = profit - eur(500);
    assert!(
        cierre::post_allocation(
            &v,
            2026,
            date!(2027 - 05 - 15),
            &split,
            at(date!(2027 - 05 - 15)),
            true
        )
        .is_err(),
        "por debajo de la reserva legal mínima"
    );
    let split = cierre::Allocation {
        legal: eur(600),
        losses: Money::ZERO,
        voluntary: profit - eur(1600),
        dividends: eur(1000),
        remanente: Money::ZERO,
        capitalization: Money::ZERO,
    };
    assert!(
        cierre::post_allocation(
            &v,
            2026,
            date!(2026 - 12 - 31),
            &split,
            at(date!(2027 - 05 - 15)),
            true
        )
        .is_err(),
        "la junta es después del cierre"
    );
    cierre::post_allocation(
        &v,
        2026,
        date!(2027 - 05 - 15),
        &split,
        at(date!(2027 - 05 - 15)),
        true,
    )
    .unwrap();
    assert_eq!(balance(&v, 2027, "112"), -eur(600));
    assert_eq!(balance(&v, 2027, "526"), -eur(1000));
    assert_eq!(balance(&v, 2027, "129"), Money::ZERO, "129 saldada");
    assert!(item(&cierre::checklist(&v, 2026).unwrap(), "aplicacion"));

    // Borrador de la memoria: aplicación aprobada, capital y situación fiscal.
    use taxpain_invoicing::pdf::Para;
    let m = taxpain_app::memoria::paragraphs(&v, 2026).unwrap();
    let has_row = |label: &str, value: &str| {
        m.iter()
            .any(|p| matches!(p, Para::Row(l, r) if l.contains(label) && r == value))
    };
    assert!(has_row("A reserva legal", "600,00 €"), "{m:?}");
    assert!(has_row("A dividendos", "1.000,00 €"));
    assert!(has_row("Capital", "3.000,00 €"));
    assert!(has_row(
        "Importe neto de la cifra de negocios",
        "10.000,00 €"
    ));
    assert!(
        m.iter()
            .any(|p| matches!(p, Para::Row(l, _) if l == "Base imponible"))
    );
    assert!(
        taxpain_app::memoria::rtf(&v, 2026)
            .unwrap()
            .starts_with("{\\rtf1")
    );
    assert!(
        taxpain_app::memoria::pdf(&v, 2026)
            .unwrap()
            .starts_with(b"%PDF")
    );

    // Un dividendo de este reparto exigible (y pagado) en 2028 también impide deshacerlo.
    let paid = accounting::record_entry(
        v.connection(),
        &JournalEntry {
            date: date!(2028 - 01 - 10),
            concept: "Pago de dividendos".into(),
            origin: Origin::Dividendo(99),
            lines: vec![
                EntryLine::debit(acc("526"), eur(1000)),
                EntryLine::credit(acc("572"), eur(1000)),
            ],
        },
        at(date!(2028 - 01 - 10)),
    )
    .unwrap();
    let e = cierre::undo_allocation(&v, 2026, at(date!(2028 - 01 - 11)), true)
        .unwrap_err()
        .to_string();
    assert!(e.contains("dividendos pagados"), "{e}");
    for sql in [
        "DELETE FROM journal_line WHERE entry_id = ?1",
        "DELETE FROM journal_entry WHERE id = ?1",
    ] {
        v.connection().execute(sql, [paid]).unwrap();
    }

    cierre::undo_allocation(&v, 2026, at(date!(2027 - 05 - 16)), true).unwrap();
    assert_eq!(balance(&v, 2027, "112"), Money::ZERO);
}

/// Con pérdidas anteriores que dejan el patrimonio neto por debajo del capital, el beneficio las
/// compensa antes de la reserva legal y no hay dividendos (art. 28.3 RICAC, art. 273.2 LSC); un
/// resultado anterior sin aplicar no se suma al del año.
#[test]
fn perdidas_anteriores_y_resultado_sin_aplicar() {
    let v = setup();
    entry(
        &v,
        date!(2025 - 01 - 01),
        "Capital",
        &[("5720001", 3000, 0), ("100", 0, 3000)],
    );
    entry(
        &v,
        date!(2025 - 06 - 30),
        "Gastos",
        &[("629", 10_000, 0), ("5720001", 0, 10_000)],
    );
    accounting::close_year(&v, 2025, at(date!(2026 - 03 - 31)), true).unwrap();
    entry(
        &v,
        date!(2026 - 06 - 30),
        "Ingresos",
        &[("5720001", 2000, 0), ("705", 0, 2000)],
    );
    // Sin aplicar las pérdidas de 2025 se cierra 2026: siguen en la 129.
    accounting::close_year(&v, 2026, at(date!(2027 - 03 - 31)), true).unwrap();
    let a = cierre::allocation(&v, 2026).unwrap();
    assert_eq!(a.result, eur(2000), "sólo el resultado de 2026");
    assert!(
        a.warnings
            .iter()
            .any(|w| w.contains("ejercicios anteriores sin aplicar")),
        "{:?}",
        a.warnings
    );
    assert_eq!(a.max_dividends, Money::ZERO);

    // Se reabre 2026, se aplican las pérdidas de 2025 y se vuelve a cerrar.
    accounting::reopen_year(&v, 2026, at(date!(2027 - 04 - 01)), true).unwrap();
    cierre::post_allocation(
        &v,
        2025,
        date!(2026 - 05 - 15),
        &cierre::Allocation::default(),
        at(date!(2027 - 04 - 01)),
        true,
    )
    .unwrap();
    accounting::close_year(&v, 2026, at(date!(2027 - 04 - 02)), true).unwrap();
    let a = cierre::allocation(&v, 2026).unwrap();
    assert!(a.warnings.is_empty(), "{:?}", a.warnings);
    assert_eq!(a.losses, eur(10_000));
    // Patrimonio neto: 3.000 − 10.000 + 2.000 = −5.000, por debajo del capital.
    assert_eq!(a.equity, -eur(5000));
    assert_eq!(a.required_compensation, eur(2000));
    assert_eq!(a.legal_minimum, Money::ZERO);
    assert_eq!(a.max_dividends, Money::ZERO);
    assert_eq!(a.suggested.losses, eur(2000));
    let wrong = cierre::Allocation {
        legal: eur(200),
        dividends: eur(1800),
        ..cierre::Allocation::default()
    };
    let e = cierre::post_allocation(
        &v,
        2026,
        date!(2027 - 05 - 15),
        &wrong,
        at(date!(2027 - 05 - 15)),
        true,
    )
    .unwrap_err()
    .to_string();
    assert!(e.contains("28.3"), "{e}");
    cierre::post_allocation(
        &v,
        2026,
        date!(2027 - 05 - 15),
        &a.suggested,
        at(date!(2027 - 05 - 15)),
        true,
    )
    .unwrap();
    assert_eq!(balance(&v, 2027, "121"), eur(8000));
    // Con la aplicación contabilizada no se reabre 2026 (habría que rehacerla).
    let e = accounting::reopen_year(&v, 2026, at(date!(2027 - 05 - 16)), true)
        .unwrap_err()
        .to_string();
    assert!(e.contains("deshazla antes"), "{e}");
}

#[test]
fn plazos_tras_el_cierre() {
    // Seis meses tras el cierre y 25 días: 25 de julio en un año natural; con cierre el 30 de
    // junio, los seis meses acaban el 31 de diciembre (no el 30).
    assert_eq!(
        cierre::corporate_tax_deadline(date!(2026 - 12 - 31)),
        date!(2027 - 07 - 25)
    );
    assert_eq!(
        cierre::corporate_tax_deadline(date!(2026 - 06 - 30)),
        date!(2027 - 01 - 25)
    );
    assert_eq!(
        cierre::after_close(date!(2026 - 06 - 30), 4),
        date!(2026 - 10 - 31)
    );
    assert_eq!(
        cierre::after_close(date!(2026 - 12 - 31), 6),
        date!(2027 - 06 - 30)
    );
    // Depósito de cuentas: junta hasta el 30 de junio y un mes para depositar (arts. 164 y 279
    // LSC): el 30 de julio, la misma fecha que da el calendario fiscal (no el 31).
    assert_eq!(
        cierre::deposit_deadline(date!(2026 - 12 - 31)),
        date!(2027 - 07 - 30)
    );
    assert_eq!(
        cierre::deposit_deadline(date!(2026 - 06 - 30)),
        date!(2027 - 01 - 31)
    );
}

/// El dividendo a cuenta (557) repartido durante el año: el reparto ha de incluirlo y lo salda.
#[test]
fn dividendo_a_cuenta() {
    let v = setup();
    entry(
        &v,
        date!(2026 - 01 - 01),
        "Capital",
        &[("5720001", 3000, 0), ("100", 0, 3000)],
    );
    entry(
        &v,
        date!(2026 - 06 - 30),
        "Ingresos",
        &[("5720001", 10_000, 0), ("705", 0, 10_000)],
    );
    entry(
        &v,
        date!(2026 - 11 - 30),
        "Dividendo a cuenta",
        &[("557", 1000, 0), ("5720001", 0, 1000)],
    );
    accounting::close_year(&v, 2026, at(date!(2027 - 03 - 31)), true).unwrap();
    let a = cierre::allocation(&v, 2026).unwrap();
    assert_eq!(a.interim_dividend, eur(1000));
    assert_eq!(a.suggested.dividends, eur(1000));
    assert_eq!(a.max_dividends, eur(10_000));
    let mut none = a.suggested;
    none.voluntary += none.dividends;
    none.dividends = Money::ZERO;
    let e = cierre::post_allocation(
        &v,
        2026,
        date!(2027 - 05 - 15),
        &none,
        at(date!(2027 - 05 - 15)),
        true,
    )
    .unwrap_err()
    .to_string();
    assert!(e.contains("a cuenta"), "{e}");
    cierre::post_allocation(
        &v,
        2026,
        date!(2027 - 05 - 15),
        &a.suggested,
        at(date!(2027 - 05 - 15)),
        true,
    )
    .unwrap();
    assert_eq!(balance(&v, 2027, "557"), Money::ZERO, "557 saldada");
    assert_eq!(balance(&v, 2027, "526"), Money::ZERO, "nada más por pagar");
}

/// Liquidaciones del IVA anteriores a CONTABILIDAD-02: el 1T dejó 53 € a compensar en la 4700
/// y el 2T llevó a la 4750 los 315 € de cuota entera, aunque el 303 compensó los 53 e ingresó
/// 262. Quedan 53 € en la 4700 y 53 € en la 4750 a la vez: la lista lo avisa y el ajuste,
/// confirmado, los salda.
#[test]
fn ajuste_de_los_saldos_de_la_4700_y_la_4750() {
    let v = setup();
    let now = at(date!(2026 - 07 - 25));
    let liquidacion = |key: i64, d: Date, lines: Vec<EntryLine>| {
        accounting::record_entry(
            v.connection(),
            &JournalEntry {
                date: d,
                concept: "Liquidación IVA (antigua)".into(),
                origin: Origin::LiquidacionIva(key),
                lines,
            },
            now,
        )
        .unwrap();
    };
    entry(
        &v,
        date!(2026 - 02 - 10),
        "Compra",
        &[("629", 300, 0), ("472", 63, 0), ("5720001", 0, 363)],
    );
    entry(
        &v,
        date!(2026 - 02 - 20),
        "Venta",
        &[("5720001", 60, 0), ("705", 0, 50), ("477", 0, 10)],
    );
    liquidacion(
        202601,
        date!(2026 - 03 - 31),
        vec![
            EntryLine::debit(acc("477"), eur(10)),
            EntryLine::debit(acc("4700"), eur(53)),
            EntryLine::credit(acc("472"), eur(63)),
        ],
    );
    assert!(
        cierre::vat_balances(&v, 2026).unwrap().is_none(),
        "un exceso a compensar solo no es un saldo fantasma"
    );
    entry(
        &v,
        date!(2026 - 05 - 20),
        "Venta",
        &[("5720001", 1815, 0), ("705", 0, 1500), ("477", 0, 315)],
    );
    liquidacion(
        202602,
        date!(2026 - 06 - 30),
        vec![
            EntryLine::debit(acc("477"), eur(315)),
            EntryLine::credit(acc("4750"), eur(315)),
        ],
    );
    assert!(
        cierre::vat_balances(&v, 2026).unwrap().is_none(),
        "la 4750 de un 303 sin pagar es deuda de verdad"
    );
    // Pago del 303 del 2T por lo que salió a ingresar (315 − 53 = 262).
    let pago = accounting::record_entry(
        v.connection(),
        &JournalEntry {
            date: date!(2026 - 07 - 20),
            concept: "Pago modelo 303 2T 2026".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("4750"), eur(262)),
                EntryLine::credit(acc("5720001"), eur(262)),
            ],
        },
        now,
    )
    .unwrap();
    v.connection()
        .execute(
            "INSERT INTO tax_payment(model, year, period, date, account, treasury, amount, entry_id, created_at)
             VALUES ('303', 2026, '2T', '2026-07-20', '4750', '5720001', 26200, ?1, '2026-07-20')",
            [pago],
        )
        .unwrap();
    let p = cierre::vat_balances(&v, 2026).unwrap().unwrap();
    assert_eq!(
        (p.debtor, p.creditor, p.amount),
        (eur(53), eur(53), eur(53))
    );
    assert_eq!(
        p.date,
        date!(2026 - 07 - 20),
        "el último asiento de la 4700/4750 (el pago)"
    );
    let c = cierre::checklist(&v, 2026).unwrap();
    let item = c.items.iter().find(|i| i.id == "iva_saldos").unwrap();
    assert!(!item.ok, "{}", item.detail);
    assert!(cierre::post_vat_balances(&v, 2026, now, false).is_err());
    cierre::post_vat_balances(&v, 2026, now, true).unwrap();
    assert_eq!(balance(&v, 2026, "4700"), Money::ZERO);
    assert_eq!(balance(&v, 2026, "4750"), Money::ZERO);
    assert!(cierre::vat_balances(&v, 2026).unwrap().is_none());
    assert!(cierre::post_vat_balances(&v, 2026, now, true).is_err());
}

fn asiento(v: &Vault, d: Date, origin: Origin, lines: Vec<EntryLine>) -> i64 {
    accounting::record_entry(
        v.connection(),
        &JournalEntry {
            date: d,
            concept: "Asiento de prueba".into(),
            origin,
            lines,
        },
        at(d),
    )
    .unwrap()
}

/// 303 de `year`/`period` pagado: el cargo en el banco y su registro.
fn pago_303(v: &Vault, year: i32, period: &str, d: Date, euros: i64) {
    let id = asiento(
        v,
        d,
        Origin::Manual,
        vec![
            EntryLine::debit(acc("4750"), eur(euros)),
            EntryLine::credit(acc("5720001"), eur(euros)),
        ],
    );
    v.connection()
        .execute(
            "INSERT INTO tax_payment(model, year, period, date, account, treasury, amount, entry_id, created_at)
             VALUES ('303', ?1, ?2, ?3, '4750', '5720001', ?4, ?5, ?3)",
            taxpain_vault::rusqlite::params![year, period, d.to_string(), euros * 100, id],
        )
        .unwrap();
}

/// La 4750 de la apertura es el 303 del 4T anterior sin pago registrado: es deuda, no un saldo
/// fantasma, y no se propone llevarla contra la 4700 (se avisa de que falta el pago).
#[test]
fn la_4750_de_la_apertura_sin_pagar_no_se_ajusta() {
    let v = setup();
    asiento(
        &v,
        date!(2026 - 01 - 01),
        Origin::Apertura,
        vec![
            EntryLine::debit(acc("5720001"), eur(300)),
            EntryLine::credit(acc("4750"), eur(300)),
        ],
    );
    // 1T: 53 € a compensar; 2T: 100 € a ingresar, pagados.
    asiento(
        &v,
        date!(2026 - 03 - 31),
        Origin::LiquidacionIva(202601),
        vec![
            EntryLine::debit(acc("4700"), eur(53)),
            EntryLine::credit(acc("472"), eur(53)),
        ],
    );
    asiento(
        &v,
        date!(2026 - 06 - 30),
        Origin::LiquidacionIva(202602),
        vec![
            EntryLine::debit(acc("477"), eur(100)),
            EntryLine::credit(acc("4750"), eur(100)),
        ],
    );
    pago_303(&v, 2026, "2T", date!(2026 - 07 - 20), 100);
    assert!(cierre::vat_balances(&v, 2026).unwrap().is_none());
    let c = cierre::checklist(&v, 2026).unwrap();
    let item = c.items.iter().find(|i| i.id == "iva_saldos").unwrap();
    assert!(item.ok);
    assert!(item.detail.contains("pago registrado"), "{}", item.detail);
    // Con el pago del 4T de 2025 registrado, ya no se menciona.
    pago_303(&v, 2025, "4T", date!(2026 - 01 - 20), 300);
    let c = cierre::checklist(&v, 2026).unwrap();
    let item = c.items.iter().find(|i| i.id == "iva_saldos").unwrap();
    assert!(item.ok && !item.detail.contains("pago"), "{}", item.detail);
}

/// La 4700 de la última liquidación del año se compensa el año siguiente: no se lleva contra
/// la 4750 aunque ésta tenga saldo.
#[test]
fn la_4700_de_la_ultima_liquidacion_no_se_ajusta() {
    let v = setup();
    // 2T: 315 € a la 4750 y sólo 262 pagados (quedan 53 €).
    asiento(
        &v,
        date!(2026 - 06 - 30),
        Origin::LiquidacionIva(202602),
        vec![
            EntryLine::debit(acc("477"), eur(315)),
            EntryLine::credit(acc("4750"), eur(315)),
        ],
    );
    pago_303(&v, 2026, "2T", date!(2026 - 07 - 20), 262);
    // 4T: 140 € a compensar en 2027.
    asiento(
        &v,
        date!(2026 - 12 - 31),
        Origin::LiquidacionIva(202604),
        vec![
            EntryLine::debit(acc("4700"), eur(140)),
            EntryLine::credit(acc("472"), eur(140)),
        ],
    );
    assert!(cierre::vat_balances(&v, 2026).unwrap().is_none());
}

/// Si el ejercicio siguiente ya tiene apertura (la de los saldos iniciales o la del diario de la
/// gestoría), cerrar el anterior no añade otra: duplicaría todos los saldos.
#[test]
fn cerrar_no_duplica_la_apertura_que_ya_habia() {
    let v = setup();
    // Apertura de 2026 hecha antes (como la del asistente de saldos iniciales).
    accounting::record_entry(
        v.connection(),
        &JournalEntry {
            date: date!(2026 - 01 - 01),
            concept: "Apertura (saldos iniciales)".into(),
            origin: Origin::Apertura,
            lines: vec![
                EntryLine::debit(acc("5720001"), eur(3_000)),
                EntryLine::credit(acc("100"), eur(3_000)),
            ],
        },
        at(date!(2026 - 01 - 01)),
    )
    .unwrap();
    // Después llega el ejercicio 2025 (de la gestoría) y se cierra con TaxPain.
    entry(
        &v,
        date!(2025 - 03 - 01),
        "Constitución",
        &[("5720001", 3_000, 0), ("100", 0, 3_000)],
    );
    let r = accounting::close_year(&v, 2025, at(date!(2026 - 02 - 01)), true).unwrap();
    assert!(r.closing_entry.is_some());
    assert!(r.opening_entry.is_none(), "{r:?}");
    assert!(
        r.aviso
            .is_some_and(|a| a.contains("ya tenía un asiento de apertura"))
    );
    // El banco de 2026 sigue con 3.000 €, no 6.000.
    assert_eq!(balance(&v, 2026, "5720001"), eur(3_000));
}

/// Con saldo en los grupos 8 o 9 (p. ej. de un diario importado) no se cierra: la regularización
/// sólo salda 6 y 7, y el cierre los llevaría a la apertura. Hay que traspasarlos al grupo 13.
#[test]
fn no_cierra_con_saldo_en_los_grupos_8_y_9() {
    let v = setup();
    entry(
        &v,
        date!(2026 - 01 - 01),
        "Capital",
        &[("5720001", 3000, 0), ("100", 0, 3000)],
    );
    entry(
        &v,
        date!(2026 - 05 - 01),
        "Subvención de capital",
        &[("5720001", 1000, 0), ("940", 0, 1000)],
    );
    let now = at(date!(2027 - 01 - 15));
    let err = accounting::close_year(&v, 2026, now, true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("940") && err.contains("grupo 13"), "{err}");
    assert!(err.contains("acreedor de 1.000,00 €"), "{err}");
    assert!(!cierre::checklist(&v, 2026).unwrap().closed);
    // Traspasada a la 130 (PGC, quinta parte): ya se cierra y la apertura no lleva la 940.
    entry(
        &v,
        date!(2026 - 12 - 31),
        "Traspaso de la subvención",
        &[("940", 1000, 0), ("130", 0, 1000)],
    );
    accounting::close_year(&v, 2026, now, true).unwrap();
    let open = accounting::trial_balance(&v, 2027, None).unwrap();
    assert!(!open.iter().any(|b| b.account.as_str() == "940"));
    assert_eq!(
        open.iter()
            .find(|b| b.account.as_str() == "130")
            .unwrap()
            .balance(),
        -eur(1000)
    );
}
