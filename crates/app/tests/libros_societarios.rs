//! Libros societarios de Estudio Demo SL (B12345674), unipersonal: libro registro de socios
//! cotejado con la ficha, contratos con la socia única propuestos desde las nóminas y los
//! dividendos, libro de actas con plantillas, certificación y su entrada en la legalización.

#![allow(clippy::unwrap_used)]

use std::io::Read;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use taxpain_app::accounting::{close_year, manual_entry};
use taxpain_app::invoicing;
use taxpain_app::legalizacion::{self, Request, base64};
use taxpain_app::libros_societarios::actas::{
    self, CertificateInput, MinuteKind, MinuteStatus, Template,
};
use taxpain_app::libros_societarios::contratos::{self, SoleContract};
use taxpain_app::libros_societarios::registro::{self, MovementKind, ShareMovement};
use taxpain_app::libros_societarios::{self as ls, BookKind};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::socios::{self, Shareholder};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{Address, Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

const CARMEN: &str = "12345678Z";

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
            id: PartyId::Nif("B12345674".into()),
            name: "Estudio Demo SL".into(),
            address: Some(Address {
                street: "Calle Mayor 1".into(),
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

fn entry(v: &Vault, d: time::Date, debit: &str, credit: &str, euros: i64) {
    manual_entry(
        v,
        &JournalEntry {
            date: d,
            concept: "Operación".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc(debit), eur(euros)),
                EntryLine::credit(acc(credit), eur(euros)),
            ],
        },
        datetime!(2026-06-01 10:00 UTC),
    )
    .unwrap();
}

fn mov(
    kind: MovementKind,
    d: time::Date,
    from: (&str, &str),
    to: (&str, &str),
    a: u32,
    b: u32,
) -> ShareMovement {
    ShareMovement {
        id: 0,
        date: d,
        kind,
        from_nif: from.0.into(),
        from_name: from.1.into(),
        to_nif: to.0.into(),
        to_name: to.1.into(),
        to_address: "Calle Mayor 1, 46001 València".into(),
        to_nationality: "española".into(),
        number_from: a,
        number_to: b,
        unit_nominal: eur(1),
        price: None,
        charge: String::new(),
        title: "Escritura de constitución".into(),
        notary: "Notario de ejemplo".into(),
        protocol: "123".into(),
        title_date: Some(d),
        notes: String::new(),
    }
}

fn carmen(pct: u32) -> Shareholder {
    Shareholder {
        id: 0,
        nif: CARMEN.into(),
        name: "Carmen Española Española".into(),
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
fn registro_de_socios_y_ficha() {
    let v = setup();
    let now = datetime!(2026-03-01 10:00 UTC);
    let c = ("12345678Z", "Carmen Española Española");
    let id = registro::save_movement(
        &v,
        &mov(
            MovementKind::Constitucion,
            date!(2024 - 01 - 15),
            ("", ""),
            c,
            1,
            3000,
        ),
        now,
    )
    .unwrap();
    // Sin ficha de socios: lo avisa.
    let r = registro::register(&v, date!(2026 - 03 - 01)).unwrap();
    assert_eq!(r.situation.capital, eur(3000));
    assert_eq!(r.situation.holders[0].pct, 1_000_000);
    assert!(!r.in_sync);
    assert!(
        r.checks.iter().any(|c| c.contains("no está en la ficha")),
        "{:?}",
        r.checks
    );
    assert!(r.checks.iter().any(|c| c.contains("unipersonal")));

    // Ficha con otro porcentaje: se ve y se corrige desde el libro.
    socios::save_shareholder(&v, &carmen(500_000), now).unwrap();
    let r = registro::register(&v, date!(2026 - 03 - 01)).unwrap();
    assert!(
        r.checks
            .iter()
            .any(|c| c.contains("la ficha de socios dice 50 %")),
        "{:?}",
        r.checks
    );
    assert!(registro::apply_to_shareholders(&v, now, false).is_err());
    assert_eq!(registro::apply_to_shareholders(&v, now, true).unwrap(), 1);
    assert_eq!(socios::shareholders(&v).unwrap()[0].pct, 1_000_000);
    assert!(
        registro::register(&v, date!(2026 - 03 - 01))
            .unwrap()
            .in_sync
    );

    // El capital del libro se coteja con la cuenta 100.
    entry(&v, date!(2026 - 01 - 02), "5720001", "100", 2000);
    let r = registro::register(&v, date!(2026 - 03 - 01)).unwrap();
    assert!(
        r.checks
            .iter()
            .any(|c| c.contains("no es el de la contabilidad")),
        "{:?}",
        r.checks
    );

    // Lo incoherente no entra: transmitir lo que no es del transmitente, o borrar la constitución
    // con movimientos que dependen de ella.
    let otro = ("A22222228", "Otra Sociedad SA");
    let bad = mov(
        MovementKind::Transmision,
        date!(2026 - 02 - 01),
        otro,
        c,
        1,
        10,
    );
    assert!(registro::save_movement(&v, &bad, now).is_err());
    let t = mov(
        MovementKind::Transmision,
        date!(2026 - 02 - 01),
        c,
        otro,
        1,
        300,
    );
    registro::save_movement(&v, &t, now).unwrap();
    assert!(registro::delete_movement(&v, id, now, true).is_err());
    let r = registro::register(&v, date!(2026 - 03 - 01)).unwrap();
    assert_eq!(
        r.situation
            .holders
            .iter()
            .map(|h| h.pct)
            .collect::<Vec<_>>(),
        [900_000, 100_000]
    );
    assert!(r.checks.iter().any(|c| c.contains("Otra Sociedad SA")));
    // Con la ficha de dos socios, aplicar sólo cambia a quien está en ella.
    assert_eq!(registro::apply_to_shareholders(&v, now, true).unwrap(), 1);
    assert_eq!(socios::shareholders(&v).unwrap()[0].pct, 900_000);

    // El libro en PDF.
    let pdf = ls::book(&v, BookKind::Socios, Some(2026), None, true).unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    let rtf =
        String::from_utf8(ls::book(&v, BookKind::Socios, None, None, false).unwrap()).unwrap();
    assert!(rtf.contains("Libro registro de socios"));
    assert!(rtf.contains("Transmisi\\u243?n"), "lleva la transmisión");
}

#[test]
fn contratos_con_la_socia_unica() {
    let v = setup();
    let now = datetime!(2026-12-01 10:00 UTC);
    socios::save_shareholder(&v, &carmen(1_000_000), now).unwrap();
    let conn = v.connection();
    conn.execute(
        "INSERT INTO employee(nif, name, administrator, updated_at) VALUES (?1, 'Carmen Española Española', 1, 'x')",
        [CARMEN],
    )
    .unwrap();
    for m in ["01", "02", "03"] {
        conn.execute(
            "INSERT INTO payslip(employee_id, date, gross, irpf, ss_employee, ss_company, created_at)
             VALUES (1, ?1, 200000, 30000, 0, 0, 'x')",
            [format!("2026-{m}-28")],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO dividend_payment(date, gross, withheld, treasury, created_at)
         VALUES ('2026-07-01', 1000000, 190000, '572', 'x')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO dividend_line(payment_id, nif, name, province, legal_person, resident, pct,
           gross, rate_bp, withheld)
         VALUES (1, ?1, 'Carmen Española Española', '46', 0, 1, 1000000, 1000000, 1900, 190000)",
        [CARMEN],
    )
    .unwrap();
    let p = contratos::proposals(&v, 2026).unwrap();
    assert_eq!(p.sole_partner.as_ref().unwrap().0, CARMEN);
    assert_eq!(p.proposals.len(), 2);
    assert_eq!(p.proposals[0].amount, Some(eur(6000)));
    assert!(p.proposals[0].object.contains("administrador"));
    assert_eq!(p.proposals[1].amount, Some(eur(10_000)));
    // Transcrita, ya no se propone (y no se transcribe dos veces).
    contratos::save_contract(&v, &p.proposals[0], now).unwrap();
    assert!(contratos::save_contract(&v, &p.proposals[0], now).is_err());
    assert_eq!(contratos::proposals(&v, 2026).unwrap().proposals.len(), 1);
    let manual = SoleContract {
        id: 0,
        date: date!(2026 - 09 - 01),
        object: "Arrendamiento a la sociedad del local de la socia".into(),
        amount: Some(eur(500)),
        conditions: "Renta mensual, un año prorrogable".into(),
        document: "Contrato privado".into(),
        source: None,
    };
    let id = contratos::save_contract(&v, &manual, now).unwrap();
    assert_eq!(contratos::contracts(&v).unwrap().len(), 2);
    assert!(contratos::delete_contract(&v, id, now, false).is_err());
    contratos::delete_contract(&v, id, now, true).unwrap();
    let pdf = ls::book(&v, BookKind::Contratos, Some(2026), None, true).unwrap();
    assert!(pdf.starts_with(b"%PDF"));
}

#[test]
fn libro_de_actas_plantillas_y_certificacion() {
    let v = setup();
    let now = datetime!(2026-06-01 10:00 UTC);
    socios::save_shareholder(&v, &carmen(1_000_000), now).unwrap();

    // Libre: la de la socia única.
    let d = actas::template(&v, &Template::Libre, date!(2026 - 02 - 01)).unwrap();
    assert_eq!(d.minute.kind, MinuteKind::SocioUnico);
    assert!(
        d.minute
            .agreements
            .contains("socio único de Estudio Demo SL")
    );
    assert_eq!(d.minute.place, "València");
    let a = actas::save_minute(&v, &d.minute, now).unwrap();
    // Nombramiento.
    let t = Template::Nombramiento {
        name: "Carmen Española Española".into(),
        nif: CARMEN.into(),
        address: String::new(),
        position: String::new(),
    };
    let d = actas::template(&v, &t, date!(2026 - 03 - 01)).unwrap();
    assert!(d.minute.agreements.contains("Nombrar administrador único"));
    assert!(d.warnings.iter().any(|w| w.contains("art. 215 LSC")));
    let b = actas::save_minute(&v, &d.minute, now).unwrap();

    // Aprobar numera por orden de aprobación dentro del ejercicio.
    assert!(actas::approve_minute(&v, b, now, false).is_err());
    assert_eq!(actas::approve_minute(&v, b, now, true).unwrap(), 1);
    assert_eq!(actas::approve_minute(&v, a, now, true).unwrap(), 2);
    let mut m = actas::minute(&v, b).unwrap();
    assert_eq!(m.status, MinuteStatus::Aprobada);
    assert_eq!(m.fiscal_year, 2026);
    m.agreements.push_str("\nAñadido");
    assert!(actas::save_minute(&v, &m, now).is_err(), "aprobada");
    assert!(actas::delete_minute(&v, b, now, true).is_err(), "aprobada");
    actas::reopen_minute(&v, b, now, true).unwrap();
    actas::save_minute(&v, &m, now).unwrap();
    assert!(
        actas::delete_minute(&v, b, now, true).is_err(),
        "tuvo número"
    );
    assert_eq!(
        actas::approve_minute(&v, b, now, true).unwrap(),
        1,
        "conserva su número"
    );

    // Certificación: sólo de una aprobada.
    let input = CertificateInput {
        name: String::new(),
        position: String::new(),
        place: String::new(),
        date: date!(2026 - 03 - 02),
    };
    let draft = actas::save_minute(
        &v,
        &actas::template(&v, &Template::Libre, date!(2026 - 04 - 01))
            .unwrap()
            .minute,
        now,
    )
    .unwrap();
    assert!(actas::certificate_paras(&v, draft, &input).is_err());
    let p = actas::certificate_paras(&v, b, &input).unwrap();
    let text = format!("{p:?}");
    assert!(text.contains("CERTIFICA"), "{text}");
    assert!(text.contains("acta nº 1 del ejercicio 2026"), "{text}");
    assert!(text.contains("Nombrar administrador único"));
    assert!(
        actas::certificate_document(&v, b, &input, true)
            .unwrap()
            .starts_with(b"%PDF")
    );

    // Libro del ejercicio: sólo las aprobadas.
    let docs = actas::book_docs(&v, 2026, Some(1)).unwrap();
    assert_eq!(docs.len(), 2);
    assert!(
        ls::book(&v, BookKind::Actas, Some(2026), Some(1), true)
            .unwrap()
            .starts_with(b"%PDF")
    );

    // Domicilio: decisión del administrador (art. 285.2 LSC).
    let d = actas::template(
        &v,
        &Template::Domicilio {
            address: "Calle Inventada 1, 28001 Madrid".into(),
        },
        date!(2026 - 05 - 01),
    )
    .unwrap();
    assert_eq!(d.minute.kind, MinuteKind::Administrador);
    assert!(
        d.minute
            .agreements
            .contains("de Calle Mayor 1, 46001 València, Valencia a Calle Inventada 1")
    );
    assert!(d.warnings.iter().any(|w| w.contains("modelo 036")));

    // Retribución: avisa si las nóminas del administrador superan el máximo.
    let conn = v.connection();
    conn.execute(
        "INSERT INTO employee(nif, name, administrator, updated_at) VALUES (?1, 'Carmen', 1, 'x')",
        [CARMEN],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO payslip(employee_id, date, gross, irpf, ss_employee, ss_company, created_at)
         VALUES (1, '2026-01-31', 3000000, 0, 0, 0, 'x')",
        [],
    )
    .unwrap();
    let d = actas::template(
        &v,
        &Template::Retribucion {
            year: 2026,
            max_amount: eur(24_000),
            concept: String::new(),
        },
        date!(2026 - 01 - 10),
    )
    .unwrap();
    assert!(d.minute.agreements.contains("24.000,00 €"));
    assert!(
        d.warnings.iter().any(|w| w.contains("30.000,00 €")),
        "{:?}",
        d.warnings
    );
}

#[test]
fn cuentas_y_dividendo_a_cuenta() {
    let v = setup();
    let now = datetime!(2027-02-10 10:00 UTC);
    socios::save_shareholder(&v, &carmen(1_000_000), datetime!(2026-01-01 10:00 UTC)).unwrap();
    entry(&v, date!(2026 - 01 - 02), "5720001", "100", 3000);
    entry(&v, date!(2026 - 05 - 10), "629", "5720001", 4000);
    entry(&v, date!(2026 - 11 - 20), "5720001", "705", 14_000);

    // Dividendo a cuenta a 30/11/2026: resultado 10.000, reserva legal 600 (hasta el 20 % del
    // capital), IS al 25 % 2.500: máximo 6.900. Tesorería 13.000.
    let t = Template::DividendoCuenta {
        amount: eur(8000),
        payment_date: Some(date!(2026 - 12 - 15)),
        statement_date: date!(2026 - 11 - 30),
        tax_rate: None,
    };
    let d = actas::template(&v, &t, date!(2026 - 12 - 01)).unwrap();
    let l = d.liquidity.unwrap();
    assert_eq!(l.result, eur(10_000));
    assert_eq!(l.legal_reserve, eur(600));
    assert_eq!(l.estimated_tax, eur(2500));
    assert_eq!(l.limit, eur(6900));
    assert_eq!(l.treasury, eur(13_000));
    assert_eq!(l.treasury_after, eur(5000));
    assert!(
        d.warnings.iter().any(|w| w.contains("277.b")),
        "{:?}",
        d.warnings
    );
    assert!(d.minute.agreements.contains("ANEXO. Estado contable"));
    assert!(
        d.minute
            .agreements
            .contains("Carmen Española Española (NIF 12345678Z), 100 %: 8.000,00 €")
    );
    let ok = Template::DividendoCuenta {
        amount: eur(5000),
        payment_date: None,
        statement_date: date!(2026 - 11 - 30),
        tax_rate: Some(Rate::percent(23)),
    };
    let d = actas::template(&v, &ok, date!(2026 - 12 - 01)).unwrap();
    assert!(
        !d.warnings.iter().any(|w| w.contains("277")),
        "{:?}",
        d.warnings
    );
    assert!(
        actas::template(
            &v,
            &Template::DividendoCuenta {
                amount: eur(5000),
                payment_date: None,
                statement_date: date!(2026 - 12 - 05),
                tax_rate: None,
            },
            date!(2026 - 12 - 01)
        )
        .is_err()
    );

    // Cuentas: con el ejercicio cerrado, la propuesta de aplicación y la gestión.
    assert!(actas::template(&v, &Template::Cuentas { year: 2026 }, date!(2027 - 05 - 01)).is_err());
    close_year(&v, 2026, now, true).unwrap();
    let d = actas::template(&v, &Template::Cuentas { year: 2026 }, date!(2027 - 05 - 01)).unwrap();
    assert_eq!(d.minute.kind, MinuteKind::SocioUnico);
    let a = &d.minute.agreements;
    assert!(
        a.contains("Aprobar las cuentas anuales del ejercicio 2026"),
        "{a}"
    );
    assert!(
        a.contains("Base de reparto: pérdidas y ganancias, 10.000,00 €"),
        "{a}"
    );
    assert!(a.contains("- A reserva legal: 600,00 €"), "{a}");
    assert!(a.contains("Aprobar la gestión"));
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("no está contabilizada"))
    );
    assert_eq!(d.minute.template, "cuentas");
    let id = actas::save_minute(&v, &d.minute, now).unwrap();
    actas::approve_minute(&v, id, now, true).unwrap();
    let cert = format!(
        "{:?}",
        actas::certificate_paras(
            &v,
            id,
            &CertificateInput {
                name: "Carmen Española Española".into(),
                position: String::new(),
                place: String::new(),
                date: date!(2027 - 05 - 02),
            }
        )
        .unwrap()
    );
    assert!(
        cert.contains("firmadas por todos los administradores"),
        "{cert}"
    );
}

fn unzip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    (0..z.len())
        .map(|i| {
            let mut f = z.by_index(i).unwrap();
            let mut b = Vec::new();
            f.read_to_end(&mut b).unwrap();
            (f.name().unwrap().into_owned(), b)
        })
        .collect()
}

#[test]
fn libros_societarios_en_la_legalizacion() {
    let v = setup();
    let now = datetime!(2027-02-10 10:00 UTC);
    socios::save_shareholder(&v, &carmen(1_000_000), datetime!(2026-01-01 10:00 UTC)).unwrap();
    entry(&v, date!(2026 - 01 - 02), "5720001", "100", 3000);
    close_year(&v, 2026, now, true).unwrap();
    let req = Request {
        year: 2026,
        diary_number: 1,
        inventory_number: 1,
        minutes_number: Some(1),
        partners_number: Some(1),
        contracts_number: Some(1),
        ..Default::default()
    };
    // Sin actas aprobadas, ni libro de socios, ni contratos: lo dice.
    let e = legalizacion::build(&v, &req, now).unwrap_err().to_string();
    assert!(e.contains("no hay actas aprobadas"), "{e}");
    let d = actas::template(&v, &Template::Libre, date!(2026 - 06 - 30)).unwrap();
    let id = actas::save_minute(&v, &d.minute, now).unwrap();
    actas::approve_minute(&v, id, now, true).unwrap();
    let e = legalizacion::build(&v, &req, now).unwrap_err().to_string();
    assert!(e.contains("registro de socios está vacío"), "{e}");
    registro::save_movement(
        &v,
        &mov(
            MovementKind::Constitucion,
            date!(2024 - 01 - 15),
            ("", ""),
            (CARMEN, "Carmen Española Española"),
            1,
            3000,
        ),
        now,
    )
    .unwrap();
    let e = legalizacion::build(&v, &req, now).unwrap_err().to_string();
    assert!(e.contains("no hay contratos"), "{e}");
    contratos::save_contract(
        &v,
        &SoleContract {
            id: 0,
            date: date!(2026 - 03 - 01),
            object: "Préstamo de la socia a la sociedad".into(),
            amount: Some(eur(10_000)),
            conditions: "Sin interés, a devolver en 2027".into(),
            document: "Contrato privado".into(),
            source: None,
        },
        now,
    )
    .unwrap();

    let p = legalizacion::build(&v, &req, now).unwrap();
    assert!(
        p.warnings
            .iter()
            .any(|w| w.contains("no hubo cambios de titularidad")),
        "{:?}",
        p.warnings
    );
    let files = unzip(&p.zip);
    let names: Vec<&str> = files.iter().map(|f| f.0.as_str()).collect();
    assert_eq!(
        names,
        [
            "DIARIO_001.xlsx",
            "INV_CUEN_001.xlsx",
            "ACTASCON_001.pdf",
            "SOCIOS_001.pdf",
            "SOCUNICO_001.pdf",
            "NOMBRES.TXT",
            "DATOS.TXT"
        ]
    );
    for (b, (_, bytes)) in p.books.iter().zip(&files) {
        assert_eq!(b.huella, base64(&Sha256::digest(bytes)));
    }
    let text =
        |n: &str| String::from_utf8(files.iter().find(|f| f.0 == n).unwrap().1.clone()).unwrap();
    assert_eq!(
        text("NOMBRES.TXT"),
        "DIARIO_001.xlsx\r\nINV_CUEN_001.xlsx\r\nACTASCON_001.pdf\r\nSOCIOS_001.pdf\r\nSOCUNICO_001.pdf\r\n"
    );
    let datos = text("DATOS.TXT");
    assert!(datos.contains("\r\n5015\r\n"), "{datos}");
    assert!(
        datos.contains(
            "\r\n00301Libro de actas 2026\r\n003021\r\n0030301012026\r\n0030431122026\r\n00305\r\n"
        ),
        "{datos}"
    );
    assert!(datos.contains("\r\n00401Libro registro de socios 2026\r\n"));
    assert!(datos.contains("\r\n00501Libro registro de contratos del socio unico 2026\r\n"));
    assert!(datos.ends_with(&format!("00506{}\r\n", p.books[4].huella)));
}
