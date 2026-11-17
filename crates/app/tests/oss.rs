//! Ventanilla única (OSS): umbral de 10.000 € del art. 73 LIVA, factura con el IVA del país del
//! cliente, su asiento en la 4779 del país, el 303 ([123]) y el 390 ([126]) sin ese IVA, el
//! libro y el borrador del 369 por país y tipo, y las correcciones de periodos anteriores.
//! Datos inventados: Estudio Demo SL y dos particulares de Francia y Alemania.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::invoicing::{self, InvoiceDraft};
use taxpain_app::models;
use taxpain_app::oss::{self, Ajustes, Clase, OssDraft};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::fx::{ForeignCurrency, RateSource};
use taxpain_core::invoice::{
    IdType, InvoiceKind, InvoiceLine, NotSubject, OssSupply, Party, PartyId, Rectification,
    RectificationMethod, RoundingMode, VatTreatment,
};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn addr(country: &str) -> Option<taxpain_core::invoice::Address> {
    Some(taxpain_core::invoice::Address {
        street: "Rue Inventée 1".into(),
        post_code: "75001".into(),
        town: "Ciudad".into(),
        province: String::new(),
        country: country.into(),
    })
}

fn setup() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    invoicing::save_settings(
        &v,
        &InvoicingSettings {
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
                pdf: true,
                facturae: true,
                ubl: false,
            },
            destination: None,
            rounding: RoundingMode::PerRate,
            verifactu: VerifactuSettings {
                enabled: true,
                installation_id: "TP-test".into(),
                production: false,
            },
            footer: None,
            activity: None,
            simplified_up_to_3000: false,
        },
        datetime!(2026-01-01 9:00 UTC),
    )
    .unwrap();
    for (pais, pct) in [("FR", 20), ("DE", 19)] {
        oss::confirmar_tipo(
            &v,
            pais,
            Rate::percent(pct),
            datetime!(2026-01-01 9:00 UTC),
            true,
        )
        .unwrap();
    }
    v
}

fn particular(country: &str, id: &str, name: &str) -> Party {
    Party {
        id: PartyId::Foreign {
            country: country.into(),
            id_type: IdType::DocumentoOficial,
            id: id.into(),
        },
        name: name.into(),
        address: addr(country),
        person: None,
    }
}

fn marie() -> Party {
    particular("FR", "X1234567", "Marie Exemple")
}

fn hans() -> Party {
    particular("DE", "T22000129", "Hans Beispiel")
}

fn draft(
    recipient: Party,
    euros: i64,
    vat: VatTreatment,
    day: time::Date,
    oss: Option<OssDraft>,
) -> InvoiceDraft {
    InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: day,
        operation_date: None,
        recipient: Some(recipient),
        lines: vec![InvoiceLine {
            description: "Curso en línea".into(),
            quantity_milli: 1000,
            unit_price: eur(euros),
            discount: Rate::ZERO,
            vat,
            surcharge: None,
            withholding: None,
            advance: false,
        }],
        rectified: vec![],
        rectified_amounts: None,
        description: "Curso en línea".into(),
        notes: None,
        currency: None,
        oss,
    }
}

const N2: VatTreatment = VatTreatment::NotSubject(NotSubject::N2);
const IVA21: VatTreatment = VatTreatment::Taxed {
    rate: Rate::percent(21),
};

fn servicios(pct: i32) -> Option<OssDraft> {
    Some(OssDraft {
        rate: Rate::percent(pct),
        supply: OssSupply::Servicios,
    })
}

fn get(d: &models::Draft, b: u16) -> Money {
    d.boxes
        .iter()
        .find(|(k, _)| *k == b)
        .map_or(Money::ZERO, |(_, v)| *v)
}

fn saldo(v: &Vault, cuenta: &str) -> Money {
    Money::from_cents(
        v.connection()
            .query_row(
                "SELECT COALESCE(SUM(credit - debit), 0) FROM journal_line WHERE account = ?1",
                [cuenta],
                |r| r.get(0),
            )
            .unwrap(),
    )
}

#[test]
fn el_umbral_se_supera_con_la_venta_que_pasa_de_diez_mil() {
    let v = setup();
    let now = datetime!(2026-11-17 10:00 UTC);
    let a = invoicing::issue(
        &v,
        &draft(marie(), 6_000, IVA21, date!(2026 - 03 - 10), None),
        now,
        true,
    )
    .unwrap();
    let b = invoicing::issue(
        &v,
        &draft(hans(), 4_000, IVA21, date!(2026 - 05 - 10), None),
        now,
        true,
    )
    .unwrap();
    // Sin clasificar, no cuentan (y se avisa).
    let e = oss::estado(&v, 2026).unwrap();
    assert_eq!(e.umbral.en_curso, Money::ZERO);
    assert_eq!(e.sin_clasificar, 2);
    assert!(e.avisos[0].contains("sin clasificar"), "{:?}", e.avisos);

    oss::clasificar(&v, a.id, Some(Clase::Servicios), now).unwrap();
    oss::clasificar(&v, b.id, Some(Clase::Servicios), now).unwrap();
    let e = oss::estado(&v, 2026).unwrap();
    // 10.000 € justos no superan el umbral, pero se avisa de que está cerca.
    assert_eq!(e.umbral.en_curso, eur(10_000));
    assert!(!e.tributa_en_destino);
    assert!(
        e.avisos.iter().any(|a| a.contains("Llevas")),
        "{:?}",
        e.avisos
    );

    // Un servicio de otra clase (consultoría) no cuenta.
    let c = invoicing::issue(
        &v,
        &draft(marie(), 300, IVA21, date!(2026 - 06 - 01), None),
        now,
        true,
    )
    .unwrap();
    oss::clasificar(&v, c.id, Some(Clase::Otro), now).unwrap();
    assert!(!oss::estado(&v, 2026).unwrap().tributa_en_destino);

    // La de septiembre lo supera: ya tenía que llevar el IVA francés.
    let d = invoicing::issue(
        &v,
        &draft(marie(), 50, IVA21, date!(2026 - 09 - 01), None),
        now,
        true,
    )
    .unwrap();
    oss::clasificar(&v, d.id, Some(Clase::Servicios), now).unwrap();
    let e = oss::estado(&v, 2026).unwrap();
    assert!(e.tributa_en_destino);
    assert_eq!(e.destino_desde, Some(date!(2026 - 09 - 01)));
    assert_eq!(e.con_iva_espanol, vec![d.number.clone()]);
    assert!(e.falta_035);
    assert!(e.avisos.iter().any(|a| a.contains("035")), "{:?}", e.avisos);

    // Con el alta apuntada, ya no falta el 035; el año siguiente, todo en destino.
    oss::guardar_ajustes(
        &v,
        &Ajustes {
            alta_035: Some(date!(2026 - 09 - 01)),
            ..Ajustes::default()
        },
        now,
    )
    .unwrap();
    assert!(!oss::estado(&v, 2026).unwrap().falta_035);
    let e27 = oss::estado(&v, 2027).unwrap();
    assert!(e27.umbral.superado_anterior);
    assert_eq!(e27.destino_desde, Some(date!(2027 - 01 - 01)));

    // La auditoría guarda las clasificaciones.
    let n: i64 = v
        .connection()
        .query_row(
            "SELECT count(*) FROM audit_log WHERE action = 'oss.clasificar'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 4);
}

#[test]
fn factura_con_el_iva_del_pais_del_cliente_303_390_y_asiento() {
    let v = setup();
    let now = datetime!(2026-11-17 10:00 UTC);
    let fr = invoicing::issue(
        &v,
        &draft(marie(), 1_000, N2, date!(2026 - 07 - 10), servicios(20)),
        now,
        true,
    )
    .unwrap();
    // Totales: 1.000 de base, sin IVA español y 200 de IVA francés; se cobra 1.200.
    let data: String = v
        .connection()
        .query_row(
            "SELECT data FROM invoice_issued WHERE id = ?1",
            [fr.id],
            |r| r.get(0),
        )
        .unwrap();
    let j: serde_json::Value = serde_json::from_str(&data).unwrap();
    assert_eq!(j["totals"]["vat"], 0);
    assert_eq!(j["totals"]["payable"], 120_000);
    assert_eq!(j["totals"]["oss"]["vat"], 20_000);
    assert_eq!(j["totals"]["oss"]["country"], "FR");
    // VERI*FACTU: no sujeta (N2) con la clave 17 del régimen OSS.
    let xml: String = v
        .connection()
        .query_row(
            "SELECT xml FROM verifactu_record WHERE invoice_id = ?1",
            [fr.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        xml.contains("<sf:ClaveRegimen>17</sf:ClaveRegimen>"),
        "{xml}"
    );
    assert!(xml.contains("<sf:CalificacionOperacion>N2"), "{xml}");
    // Sin Facturae (sus impuestos serían los españoles): se avisa.
    assert!(
        fr.export_error
            .as_deref()
            .unwrap_or("")
            .contains("Facturae"),
        "{:?}",
        fr.export_error
    );

    // Asiento: 430 1.200 / 705 1.000 y 4779011 (Francia) 200; nada en la 477 española.
    assert_eq!(saldo(&v, "4779011"), eur(200));
    assert_eq!(saldo(&v, "477"), Money::ZERO);

    let de = invoicing::issue(
        &v,
        &draft(
            hans(),
            500,
            N2,
            date!(2026 - 08 - 10),
            Some(OssDraft {
                rate: Rate::percent(19),
                supply: OssSupply::Bienes,
            }),
        ),
        now,
        true,
    )
    .unwrap();
    assert_eq!(saldo(&v, "4779006"), eur(95));

    // 303 del 3T: las bases en la informativa [123], sin IVA devengado ni [120].
    let d = models::draft(&v, "303", 2026, 3).unwrap();
    assert_eq!(get(&d, 123), eur(1_500), "{:?}", d.boxes);
    assert_eq!(get(&d, 120), Money::ZERO);
    assert_eq!(get(&d, 27), Money::ZERO);
    // 390: en [126] y en el volumen de operaciones.
    let d390 = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d390, 126), eur(1_500), "{:?}", d390.boxes);
    assert_eq!(get(&d390, 110), Money::ZERO);

    // Libro y 369 del 3T por país, clase y tipo.
    let libro = oss::libro(&v, 2026, 3).unwrap();
    assert_eq!(libro.len(), 2);
    let b = oss::borrador_369(&v, 2026, 3).unwrap();
    assert_eq!(b.vencimiento, Some(date!(2026 - 10 - 31)));
    assert_eq!(b.lineas.len(), 2);
    let l_de = &b.lineas[0];
    assert_eq!(
        (
            l_de.pais.as_str(),
            l_de.supply,
            l_de.rate,
            l_de.base,
            l_de.cuota
        ),
        (
            "DE",
            OssSupply::Bienes,
            Rate::percent(19),
            eur(500),
            eur(95)
        )
    );
    let l_fr = &b.lineas[1];
    assert_eq!(
        (l_fr.pais.as_str(), l_fr.rate, l_fr.cuota),
        ("FR", Rate::percent(20), eur(200))
    );
    assert_eq!(b.total, eur(295));
    // Asiento propuesto al pagar: las 4779 del país al debe y el banco al haber.
    assert!(
        b.asiento
            .iter()
            .any(|l| l.cuenta == "4779011" && l.debe == eur(200))
    );
    assert!(
        b.asiento
            .iter()
            .any(|l| l.cuenta == "572" && l.haber == eur(295))
    );

    // Rectificativa en el 4T de la factura francesa del 3T: corrección de aquel periodo.
    let mut r = draft(marie(), -100, N2, date!(2026 - 10 - 05), servicios(20));
    r.kind = InvoiceKind::Rectificativa(Rectification {
        code: 1,
        method: RectificationMethod::Diferencias,
    });
    r.rectified = vec![(fr.number.clone(), date!(2026 - 07 - 10))];
    invoicing::issue(&v, &r, now, true).unwrap();
    let b4 = oss::borrador_369(&v, 2026, 4).unwrap();
    assert!(b4.lineas.is_empty());
    assert_eq!(b4.correcciones.len(), 1);
    assert_eq!(
        (
            b4.correcciones[0].year,
            b4.correcciones[0].quarter,
            b4.correcciones[0].cuota
        ),
        (2026, 3, eur(-20))
    );
    assert_eq!(saldo(&v, "4779011"), eur(180));

    // La hoja del 369 se genera.
    let (name, bytes) = oss::hoja_369(&v, 2026, 3).unwrap();
    assert_eq!(name, "369_2026_3T.xlsx");
    assert!(bytes.starts_with(b"PK"));

    // La clase de una factura OSS es la suya.
    assert!(oss::clasificar(&v, de.id, Some(Clase::Otro), now).is_err());
    // Y cuentan para el umbral.
    assert_eq!(oss::estado(&v, 2026).unwrap().umbral.en_curso, eur(1_400));
}

#[test]
fn la_liquidacion_del_303_no_salda_la_4779() {
    use taxpain_app::accounting::settle_vat_quarter;
    let v = setup();
    let now = datetime!(2026-11-17 10:00 UTC);
    invoicing::issue(
        &v,
        &draft(marie(), 1_000, N2, date!(2026 - 07 - 10), servicios(20)),
        now,
        true,
    )
    .unwrap();
    invoicing::issue(
        &v,
        &draft(
            Party {
                id: PartyId::Nif("A22222228".into()),
                name: "Cliente Demo SA".into(),
                address: addr("ES"),
                person: None,
            },
            100,
            IVA21,
            date!(2026 - 07 - 11),
            None,
        ),
        now,
        true,
    )
    .unwrap();
    settle_vat_quarter(&v, 2026, 3, now).unwrap();
    // La 477 española queda saldada; la del IVA francés, pendiente del 369.
    assert_eq!(saldo(&v, "477"), Money::ZERO);
    assert_eq!(saldo(&v, "4779011"), eur(200));
}

#[test]
fn borradores_que_no_valen_con_iva_de_destino() {
    let v = setup();
    let now = datetime!(2026-11-17 10:00 UTC);
    let err = |d: InvoiceDraft| invoicing::issue(&v, &d, now, true).unwrap_err().to_string();
    // A una empresa con NIF-IVA, no (inversión del sujeto pasivo).
    let empresa = Party {
        id: PartyId::Foreign {
            country: "FR".into(),
            id_type: IdType::NifIva,
            id: "FR12345678901".into(),
        },
        name: "Société Exemple".into(),
        address: addr("FR"),
        person: None,
    };
    assert!(
        err(draft(
            empresa,
            100,
            N2,
            date!(2026 - 07 - 10),
            servicios(20)
        ))
        .contains("particular")
    );
    // Con IVA español en la línea, no.
    assert!(
        err(draft(
            marie(),
            100,
            IVA21,
            date!(2026 - 07 - 10),
            servicios(20)
        ))
        .contains("línea 1")
    );
    // En moneda extranjera, no.
    let mut d = draft(marie(), 100, N2, date!(2026 - 07 - 10), servicios(20));
    d.currency = Some(ForeignCurrency {
        code: "USD".into(),
        rate: taxpain_core::fx::ExchangeRate::parse("1,1").unwrap(),
        rate_date: date!(2026 - 07 - 09),
        source: RateSource::Bce,
    });
    assert!(err(d).contains("euros"));
    // Servicios a Irlanda del Norte (Reino Unido para los servicios), no.
    assert!(
        err(draft(
            particular("XI", "123", "Cliente de Belfast"),
            100,
            N2,
            date!(2026 - 07 - 10),
            servicios(20)
        ))
        .contains("Irlanda del Norte")
    );
    // Tipo fuera de rango.
    assert!(
        err(draft(
            marie(),
            100,
            N2,
            date!(2026 - 07 - 10),
            servicios(50)
        ))
        .contains("35 %")
    );
    // Nada emitido.
    let n: i64 = v
        .connection()
        .query_row("SELECT count(*) FROM invoice_issued", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn tipos_por_pais_con_confirmacion() {
    let v = setup();
    let now = datetime!(2026-11-17 10:00 UTC);
    let t = oss::tipos(&v).unwrap();
    assert_eq!(t.fecha, "2025-08-01");
    let at = t.paises.iter().find(|p| p.pais == "AT").unwrap();
    assert_eq!((at.tabla, at.confirmado), (Rate::percent(20), None));
    assert!(oss::confirmar_tipo(&v, "AT", Rate::percent(20), now, false).is_err());
    oss::confirmar_tipo(&v, "at", Rate::percent(20), now, true).unwrap();
    assert_eq!(
        oss::tipo_confirmado(&v, "AT").unwrap(),
        Some(Rate::percent(20))
    );
    assert!(oss::confirmar_tipo(&v, "ES", Rate::percent(21), now, true).is_err());
    let n: i64 = v
        .connection()
        .query_row(
            "SELECT count(*) FROM audit_log WHERE action = 'oss.confirmar_tipo'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    // Los dos de `setup` y éste.
    assert_eq!(n, 3);
    // Sin el tipo de Italia confirmado no se emite con IVA italiano.
    let e = invoicing::issue(
        &v,
        &draft(
            particular("IT", "AA1234567", "Giulia Esempio"),
            100,
            N2,
            date!(2026 - 07 - 10),
            servicios(22),
        ),
        now,
        true,
    )
    .unwrap_err()
    .to_string();
    assert!(
        e.contains("confirma antes el tipo general de IVA de Italia"),
        "{e}"
    );
}
