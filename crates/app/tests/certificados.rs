//! Certificados de retenciones para los perceptores: cada certificado cuadra con lo que el
//! borrador del 190, del 180 y del 193 declara de ese perceptor. Datos inventados.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;

use taxpain_app::certificados::{self, Detalle, Opciones};
use taxpain_app::received::{Nature, Reviewed, Source, ingest_file, review};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::socios::{self, PayoutInput, Shareholder};
use taxpain_app::{accounting, invoicing, models, payroll};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{Address, Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_models::informative::DirectorSs;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime, Time};

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn at(d: Date) -> OffsetDateTime {
    d.with_time(Time::from_hms(10, 0, 0).unwrap()).assume_utc()
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

fn receive(v: &Vault, nif: &str, number: &str, (base, vat, wh): (i64, i64, i64), d: Date) {
    let rec = ingest_file(
        v,
        &format!("{number}.xml"),
        format!("<?xml version=\"1.0\"?><x n=\"{number}\"/>").as_bytes(),
        Source::Manual,
        at(d),
    )
    .unwrap();
    review(
        v,
        rec.id,
        &Reviewed {
            supplier_nif: nif.into(),
            supplier_name: format!("Proveedor {nif}"),
            number: number.into(),
            issue_date: d,
            base: eur(base),
            vat: eur(vat),
            withholding: eur(wh),
            total: eur(base + vat - wh),
            deductible_pct: 100,
            capital_good: false,
            nature: Nature::General,
            asset_class: None,
            vat_regime: Default::default(),
            supplier_country: None,
            self_assessed_rate: None,
            intra_goods: false,
            cash_basis: false,
            import_dua: false,
            prorata_use: Default::default(),
        },
        at(d),
    )
    .unwrap();
}

/// Sociedad con un profesional, un alquiler, la administradora en nómina (clave E) y un reparto
/// de dividendos a una socia persona física y a una sociedad exenta.
fn scenario() -> Vault {
    let v = setup();
    // Profesional al 15 % (190, clave G.01), dos facturas.
    receive(
        &v,
        "00000000T",
        "P-1",
        (1_000, 210, 150),
        date!(2026 - 03 - 10),
    );
    receive(
        &v,
        "00000000T",
        "P-2",
        (2_000, 420, 300),
        date!(2026 - 09 - 10),
    );
    // Alquiler al 19 % (180).
    receive(
        &v,
        "A22222228",
        "ALQ-1",
        (1_000, 210, 190),
        date!(2026 - 02 - 01),
    );
    receive(
        &v,
        "A22222228",
        "ALQ-2",
        (1_000, 210, 190),
        date!(2026 - 03 - 01),
    );
    // Administradora asimilada en una entidad pequeña (E.02, 19 %), con cotizaciones.
    let now = datetime!(2026-01-02 9:00 UTC);
    let id = payroll::save_employee(
        &v,
        &payroll::Employee {
            nif: "12345678Z".into(),
            name: "ESPAÑOLA ESPAÑOLA CARMEN".into(),
            province: "46".into(),
            birth_year: 1980,
            family: 3,
            contract: 1,
            administrator: true,
            ss_regime: DirectorSs::Asimilado,
            active: true,
            ..Default::default()
        },
        now,
    )
    .unwrap();
    for d in [date!(2026 - 01 - 31), date!(2026 - 02 - 28)] {
        payroll::register(
            &v,
            &payroll::PayslipInput {
                employee_id: id,
                date: d,
                gross: eur(3_000),
                irpf: eur(570),
                ss_employee: eur(190),
                ss_company: eur(900),
                small_entity: Some(true),
            },
            now,
        )
        .unwrap();
    }
    // Dividendos: 10.000 acordados, 60 % a la socia y 40 % a una sociedad exenta.
    let holder = |nif: &str, name: &str, pct: u32, legal: bool| Shareholder {
        id: 0,
        nif: nif.into(),
        name: name.into(),
        address: "Calle Mayor 1, 46001 València".into(),
        province: "46".into(),
        country: "ES".into(),
        pct,
        legal_person: legal,
        resident: true,
        exempt: legal,
        active: true,
    };
    let now = datetime!(2026-07-01 10:00 UTC);
    socios::save_shareholder(
        &v,
        &holder("12345678Z", "Española Española Carmen", 600_000, false),
        now,
    )
    .unwrap();
    socios::save_shareholder(
        &v,
        &holder("B87654323", "Matriz Ejemplo SL", 400_000, true),
        now,
    )
    .unwrap();
    accounting::manual_entry(
        &v,
        &JournalEntry {
            date: date!(2026 - 06 - 30),
            concept: "Reparto acordado por la junta".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("113"), eur(10_000)),
                EntryLine::credit(acc("526"), eur(10_000)),
            ],
        },
        now,
    )
    .unwrap();
    socios::pay(
        &v,
        &PayoutInput {
            date: date!(2026 - 07 - 15),
            gross: eur(10_000),
            treasury: "5720001".into(),
            agreement: None,
        },
        now,
        true,
    )
    .unwrap();
    v
}

fn opts() -> Opciones {
    Opciones {
        place: "València".into(),
        date: date!(2027 - 01 - 20),
        signer: "Carmen Española Española".into(),
        signer_role: "Administradora única".into(),
        nifs: vec![],
        properties: BTreeMap::new(),
    }
}

/// Importe del detalle del borrador cuya etiqueta contiene todos los trozos.
fn detail(d: &models::Draft, parts: &[&str]) -> Money {
    let found: Vec<_> = d
        .details
        .iter()
        .filter(|(t, _)| parts.iter().all(|p| t.contains(p)))
        .collect();
    assert_eq!(found.len(), 1, "{parts:?} en {:?}", d.details);
    found[0].1
}

#[test]
fn los_certificados_del_190_cuadran_con_el_borrador() {
    let v = scenario();
    let d = models::draft(&v, "190", 2026, 0).unwrap();
    let certs = certificados::certificates(&v, "190", 2026, &opts()).unwrap();
    assert_eq!(certs.len(), 2);
    for c in &certs {
        for det in &c.detalles {
            let Detalle::Trabajo {
                key, retenciones, ..
            } = det
            else {
                panic!("{det:?}");
            };
            assert_eq!(detail(&d, &[&c.nif, &key.label()]), *retenciones);
        }
    }
    let total: Money = certs.iter().map(|c| c.retenido()).sum();
    assert_eq!(total, d.result);

    let prof = certs.iter().find(|c| c.nif == "00000000T").unwrap();
    assert_eq!(prof.integro(), eur(3_000));
    assert_eq!(prof.retenido(), eur(450));
    let admin = certs.iter().find(|c| c.nif == "12345678Z").unwrap();
    assert_eq!(
        admin.detalles,
        vec![Detalle::Trabajo {
            key: taxpain_models::informative::Key190::E(2),
            dinerarias: eur(6_000),
            retenciones: eur(1_140),
            especie: Money::ZERO,
            ingresos_a_cuenta: Money::ZERO,
            gastos: eur(380),
            reducciones: Money::ZERO,
        }]
    );
    // Las cifras del certificado son las del registro del fichero del 190.
    let r = models::result_190(&v, 2026).unwrap();
    let rec = r.lines.iter().find(|l| l.nif == "12345678Z").unwrap();
    assert_eq!(
        (rec.gross, rec.withheld, rec.deductible),
        (eur(6_000), eur(1_140), eur(380))
    );

    // Lo propuesto para la pantalla: el lugar del domicilio y la administradora de Nóminas.
    let p = certificados::prepare(&v, "190", 2026).unwrap();
    assert_eq!(p.place, "València");
    assert_eq!(p.signer, "ESPAÑOLA ESPAÑOLA CARMEN");
    assert_eq!(p.signer_role, "Administrador");
    assert_eq!(p.payees.len(), 2);
    assert!(
        p.payees
            .iter()
            .any(|x| x.concept == "clave E.02" && x.withheld == eur(1_140))
    );
}

#[test]
fn los_certificados_del_180_y_del_193_cuadran_con_el_borrador() {
    let v = scenario();
    let d180 = models::draft(&v, "180", 2026, 0).unwrap();
    let mut o = opts();
    o.properties.insert(
        "A22222228".into(),
        "Calle Inventada 3, bajo · ref. 0000000AA0000A0001AA".into(),
    );
    let c180 = certificados::certificates(&v, "180", 2026, &o).unwrap();
    assert_eq!(c180.len(), 1);
    assert_eq!(c180[0].retenido(), detail(&d180, &["A22222228"]));
    assert_eq!(c180[0].retenido(), d180.result);
    assert!(matches!(
        &c180[0].detalles[0],
        Detalle::Arrendamiento { inmueble, rentas, tipo, .. }
            if inmueble.contains("Calle Inventada 3") && *rentas == eur(2_000)
                && *tipo == Rate::percent(19)
    ));

    let d193 = models::draft(&v, "193", 2026, 0).unwrap();
    let c193 = certificados::certificates(&v, "193", 2026, &opts()).unwrap();
    assert_eq!(c193.len(), 2);
    for c in &c193 {
        for det in &c.detalles {
            let Detalle::Capital {
                naturaleza,
                integro,
                retenciones,
                ..
            } = det
            else {
                panic!("{det:?}");
            };
            let amount = detail(
                &d193,
                &[
                    &c.nif,
                    &format!("naturaleza {naturaleza:02}"),
                    &format!("bruto {integro}"),
                ],
            );
            assert_eq!(amount, *retenciones);
        }
    }
    let socia = c193.iter().find(|c| c.nif == "12345678Z").unwrap();
    assert_eq!(
        (socia.integro(), socia.retenido()),
        (eur(6_000), eur(1_140))
    );
    // Domicilio de la ficha del socio.
    assert_eq!(
        socia.address.as_deref(),
        Some("Calle Mayor 1, 46001 València")
    );
    let total: Money = c193.iter().map(|c| c.retenido()).sum();
    assert_eq!(total, d193.result);
}

#[test]
fn pdf_por_perceptor_con_todos_correo_y_auditoria() {
    let v = scenario();
    let each = certificados::pdf_each(&v, "193", 2026, &opts()).unwrap();
    assert_eq!(each.len(), 2);
    assert!(
        each.iter()
            .all(|(n, pdf)| n.ends_with(".pdf") && pdf.starts_with(b"%PDF"))
    );
    assert!(
        each.iter()
            .any(|(n, _)| n == "Certificado retenciones 193 2026 B87654323.pdf")
    );
    let (name, pdf) = certificados::pdf_all(&v, "190", 2026, &opts()).unwrap();
    assert_eq!(name, "Certificados retenciones 190 2026 B12345674.pdf");
    assert!(pdf.starts_with(b"%PDF"));
    // Sólo los elegidos.
    let mut o = opts();
    o.nifs = vec!["00000000t".into()];
    let one = certificados::certificates(&v, "190", 2026, &o).unwrap();
    assert_eq!(one.len(), 1);
    o.nifs = vec!["B87654323".into()];
    assert!(certificados::certificates(&v, "190", 2026, &o).is_err());
    // Sin lugar, no se expide.
    let mut o = opts();
    o.place = " ".into();
    assert!(certificados::pdf_all(&v, "190", 2026, &o).is_err());
    // Modelo sin certificados.
    assert!(certificados::prepare(&v, "347", 2026).is_err());

    let (name, eml) = certificados::email(
        &v,
        "193",
        2026,
        &opts(),
        "12345678Z",
        Some("socia@example.com"),
    )
    .unwrap();
    assert_eq!(name, "Certificado retenciones 193 2026 12345678Z.eml");
    let text = String::from_utf8_lossy(&eml);
    assert!(text.starts_with("X-Unsent: 1\r\n"));
    assert!(text.contains("To: socia@example.com"));
    assert!(text.contains("application/pdf"));
    assert!(
        certificados::email(
            &v,
            "193",
            2026,
            &opts(),
            "12345678Z",
            Some("no es un correo")
        )
        .is_err()
    );

    certificados::record_issued(
        &v,
        "193",
        2026,
        &["12345678Z".into()],
        "correo",
        datetime!(2027-01-20 10:00 UTC),
    )
    .unwrap();
    let n: i64 = v
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE action = 'certificados_retenciones.expedidos'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
}
