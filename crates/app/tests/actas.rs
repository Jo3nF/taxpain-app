//! Actas de la junta (o del socio único) con la aplicación del resultado y los dividendos, y
//! certificados de retenciones de los socios que cuadran con el 193.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::actas::{self, Organ, ReserveChange};
use taxpain_app::invoicing;
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::socios::{self, PayoutInput, Shareholder};
use taxpain_app::{accounting, cierre, models};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_invoicing::pdf::Para;
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
            address: None,
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

fn text(p: &[Para]) -> String {
    p.iter()
        .map(|x| match x {
            Para::Title(t) | Para::Text(t) | Para::Bold(t) => t.clone(),
            Para::Row(l, r) => format!("{l} = {r}"),
            Para::Gap => String::new(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// SL de 2026 con 10.000 € de beneficio, cerrada; la junta de 2027 aprueba las cuentas con
/// 2.000 € de dividendo, que se paga el 1 de junio.
fn sl_con_junta(holders: &[Shareholder]) -> Vault {
    let v = setup();
    let now = datetime!(2026-07-01 10:00 UTC);
    for h in holders {
        socios::save_shareholder(&v, h, now).unwrap();
    }
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
    accounting::close_year(&v, 2026, datetime!(2027-03-31 10:00 UTC), true).unwrap();
    let a = cierre::allocation(&v, 2026).unwrap();
    let mut s = a.suggested;
    assert!(s.voluntary >= eur(2000), "{s:?}");
    s.voluntary -= eur(2000);
    s.dividends += eur(2000);
    cierre::post_allocation(
        &v,
        2026,
        date!(2027 - 05 - 15),
        &s,
        datetime!(2027-05-15 10:00 UTC),
        true,
    )
    .unwrap();
    let input = PayoutInput {
        date: date!(2027 - 06 - 01),
        gross: eur(2000),
        treasury: "5720001".into(),
        agreement: None,
    };
    socios::pay(&v, &input, datetime!(2027-06-01 10:00 UTC), true).unwrap();
    v
}

#[test]
fn acta_de_la_junta_y_certificados() {
    let v = sl_con_junta(&[
        holder("12345678Z", "Socia Uno Ana", 600_000),
        holder("00000000T", "Socio Dos Luis", 400_000),
    ]);
    assert_eq!(actas::organ(&v).unwrap(), Organ::Junta);

    let p = actas::cuentas(&v, 2026).unwrap();
    let t = text(&p);
    assert!(t.contains("junta general"), "{t}");
    assert!(
        t.contains("15/05/2027"),
        "fecha de la junta: la del asiento"
    );
    assert!(t.contains("beneficios de 10.000,00"), "{t}");
    assert!(t.contains("A dividendos = 2.000,00"), "{t}");
    assert!(t.contains("A reserva legal"), "{t}");
    assert!(t.contains("[lugar]"), "huecos para completar");
    assert!(!t.contains("aún no está contabilizada"));

    let pays = socios::payments_of_year(&v, 2027).unwrap();
    let d = text(&actas::dividendo(&v, pays[0].id).unwrap());
    assert!(d.contains("01/06/2027"), "{d}");
    assert!(d.contains("Socia Uno Ana (NIF 12345678Z)"), "{d}");
    assert!(d.contains("Retención = 380,00"), "19 % de 2.000: {d}");
    assert!(d.contains("Líquido a pagar = 1.620,00"), "{d}");

    // Certificados: uno por socio, y sus retenciones suman lo del 193.
    let certs = actas::certificates(&v, 2027).unwrap();
    assert_eq!(certs.len(), 2);
    let ana = certs.iter().find(|c| c.nif == "12345678Z").unwrap();
    assert_eq!(ana.gross, eur(1200));
    assert_eq!(ana.withheld, eur(228));
    assert_eq!(ana.net, eur(972));
    assert_eq!(ana.lines[0].rate, Rate::percent(19));
    let d193 = models::draft(&v, "193", 2027, 0).unwrap();
    let total: Money = certs.iter().map(|c| c.withheld).sum();
    assert_eq!(total, d193.result);
    let docs = actas::certificate_docs(&v, 2027, Some("00000000T")).unwrap();
    assert_eq!(docs.len(), 1);
    let c = text(&docs[0]);
    assert!(c.contains("Socio Dos Luis · NIF 00000000T"), "{c}");
    assert!(c.contains("Importe líquido = 648,00"), "{c}");
    assert!(
        actas::certificate_docs(&v, 2026, None).is_err(),
        "sin dividendos en 2026"
    );

    // Libro de actas de 2027: la aprobación de las cuentas y el reparto, por fecha.
    let book = actas::of_year(&v, 2027).unwrap();
    assert_eq!(book.len(), 2);
    assert_eq!(book[0].0, date!(2027 - 05 - 15));
    assert!(text(&book[1].2).contains("pago el 01/06/2027"));
    assert!(actas::of_year(&v, 2026).unwrap().is_empty());

    // RTF y PDF.
    let all: Vec<Vec<Para>> = book.into_iter().map(|b| b.2).collect();
    let rtf = actas::rtf(&all);
    assert!(rtf.starts_with("{\\rtf1") && rtf.contains("\\page"));
    assert!(actas::pdf("Actas", &all).unwrap().starts_with(b"%PDF"));
}

#[test]
fn decisiones_del_socio_unico_y_reservas() {
    let v = sl_con_junta(&[holder("12345678Z", "Socia Única Ana", 1_000_000)]);
    assert!(matches!(
        actas::organ(&v).unwrap(),
        Organ::SocioUnico { .. }
    ));
    let t = text(&actas::cuentas(&v, 2026).unwrap());
    assert!(t.contains("socio único"), "{t}");
    assert!(t.contains("Decisión primero"), "{t}");
    assert!(t.contains("Fdo.: Socia Única Ana"), "{t}");

    let r = ReserveChange {
        date: Some(date!(2027 - 09 - 01)),
        from: "113".into(),
        to: "1149".into(),
        amount: eur(500),
        reason: "dotar la reserva de capitalización".into(),
    };
    let t = text(&actas::reservas(&v, &r).unwrap());
    assert!(
        t.contains(
            "Traspasar 500,00 € de reservas voluntarias (cuenta 113) a reserva de capitalización"
        ),
        "{t}"
    );
    assert!(t.contains("cinco años"), "{t}");
    assert!(
        actas::reservas(
            &v,
            &ReserveChange {
                amount: Money::ZERO,
                ..r
            }
        )
        .is_err()
    );
}

#[test]
fn propuesta_sin_contabilizar() {
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
        "Gastos",
        &[("629", 800, 0), ("5720001", 0, 800)],
    );
    accounting::close_year(&v, 2026, datetime!(2027-03-31 10:00 UTC), true).unwrap();
    let t = text(&actas::cuentas(&v, 2026).unwrap());
    assert!(t.contains("pérdidas de 800,00"), "{t}");
    assert!(
        t.contains("resultados negativos de ejercicios anteriores"),
        "{t}"
    );
    assert!(t.contains("aún no está contabilizada"), "{t}");
    assert!(t.contains("[fecha]"), "{t}");
}

#[test]
fn peticion_desde_el_escritorio() {
    let r: actas::Request = serde_json::from_str(
        r#"{"kind":"reservas","date":null,"from":"113","to":"1149","amount":50000,"reason":""}"#,
    )
    .unwrap();
    assert!(matches!(r, actas::Request::Reservas(ReserveChange { ref to, .. }) if to == "1149"));
    let r: actas::Request =
        serde_json::from_str(r#"{"kind":"certificados","year":2026,"nif":null}"#).unwrap();
    assert_eq!(
        r,
        actas::Request::Certificados {
            year: 2026,
            nif: None
        }
    );
}
