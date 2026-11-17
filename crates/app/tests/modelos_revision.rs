//! Correcciones de la revisión fiscal de los modelos (datos sintéticos de una SL): anulación de
//! facturas de periodos ya declarados, casillas [59]/[120], 349 sólo con la UE, devengo de las
//! adquisiciones intracomunitarias y de las emitidas con fecha de operación, fecha de registro
//! en hora peninsular, 200 de un ejercicio no natural y 202 de más de 6 millones.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::invoicing::{self, InvoiceDraft, issue};
use taxpain_app::models::{self, ModelSettings};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::invoice::{InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime, Time};

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

fn at(d: Date) -> OffsetDateTime {
    d.with_time(Time::from_hms(10, 0, 0).unwrap()).assume_utc()
}

fn es_client() -> Party {
    Party {
        id: PartyId::Nif("B12345674".into()),
        name: "Cliente SL".into(),
        address: addr(),
        person: None,
    }
}

fn line(euros: i64, vat: VatTreatment) -> InvoiceLine {
    InvoiceLine {
        description: "Servicio".into(),
        quantity_milli: 1000,
        unit_price: eur(euros),
        discount: Rate::ZERO,
        vat,
        surcharge: None,
        withholding: None,
        advance: false,
    }
}

fn draft_to(recipient: Party, lines: Vec<InvoiceLine>, d: Date) -> InvoiceDraft {
    InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: d,
        operation_date: None,
        recipient: Some(recipient),
        lines,
        rectified: vec![],
        rectified_amounts: None,
        description: "x".into(),
        notes: None,
        currency: None,
        oss: None,
    }
}

fn taxed21() -> VatTreatment {
    VatTreatment::Taxed {
        rate: Rate::percent(21),
    }
}

fn get(d: &models::Draft, b: u16) -> Money {
    d.boxes
        .iter()
        .find(|(k, _)| *k == b)
        .map_or(Money::ZERO, |(_, v)| *v)
}

// ---------- 202 de más de 6 millones ----------

#[test]
fn m202_mas_de_seis_millones_por_la_base() {
    let v = setup();
    let s = ModelSettings {
        corporate_tax_base: Some(eur(10_000)),
        turnover_over_6m: true,
        ..models::settings(&v).unwrap()
    };
    models::save_settings(&v, &s, at(date!(2026 - 01 - 02))).unwrap();
    // Con más de 6 millones va obligatoriamente por la base del periodo (art. 40.3 LIS), con
    // fichero de la modalidad B; sin resultado en el periodo, nada a ingresar.
    let d = models::draft(&v, "202", 2026, 2).unwrap();
    assert!(!d.file.is_empty());
    assert!(d.boxes.iter().any(|(k, _)| *k == 34), "{:?}", d.boxes);
    assert!(
        d.warnings.iter().any(|w| w.contains("40.3")),
        "{:?}",
        d.warnings
    );
    assert_eq!(d.result, Money::ZERO);
    models::export(&v, &d, at(date!(2026 - 10 - 05))).unwrap();

    // Por debajo de 6 millones, la de la cuota (art. 40.2): 18 % de 10.000.
    let s = ModelSettings {
        turnover_over_6m: false,
        ..s
    };
    models::save_settings(&v, &s, at(date!(2026 - 01 - 02))).unwrap();
    let d = models::draft(&v, "202", 2026, 2).unwrap();
    assert!(!d.file.is_empty());
    assert_eq!(d.result, eur(1_800));
}

// ---------- Tipos de IVA sin casilla ----------

#[test]
fn m303_bloquea_tipos_sin_casilla() {
    let v = setup();
    issue(
        &v,
        &draft_to(
            es_client(),
            vec![line(1000, taxed21())],
            date!(2026 - 02 - 10),
        ),
        at(date!(2026 - 02 - 10)),
        true,
    )
    .unwrap();
    // Un 20 % (p. ej. mal leído de una factura) no tiene casilla: su cuota se perdería.
    let bad = issue(
        &v,
        &draft_to(
            es_client(),
            vec![line(
                500,
                VatTreatment::Taxed {
                    rate: Rate::percent(20),
                },
            )],
            date!(2026 - 03 - 10),
        ),
        at(date!(2026 - 03 - 10)),
        true,
    )
    .unwrap();
    let err = models::draft(&v, "303", 2026, 1).unwrap_err().to_string();
    assert!(err.contains(&bad.number), "{err}");
    assert!(err.contains("20 %") && err.contains("100,00 €"), "{err}");
    let err = models::draft(&v, "390", 2026, 0).unwrap_err().to_string();
    assert!(err.contains(&bad.number), "{err}");
    // Los demás trimestres no se ven afectados.
    assert!(models::draft(&v, "303", 2026, 2).is_ok());
}

// ---------- [59] / [120] y 349 sólo con la UE ----------

fn foreign(country: &str, id_type: taxpain_core::invoice::IdType, id: &str, name: &str) -> Party {
    Party {
        id: PartyId::Foreign {
            country: country.into(),
            id_type,
            id: id.into(),
        },
        name: name.into(),
        address: addr(),
        person: None,
    }
}

#[test]
fn servicios_a_la_ue_en_59_y_349_solo_con_la_ue() {
    use taxpain_core::invoice::{Exemption, IdType, NotSubject};
    let v = setup();
    let n2 = || VatTreatment::NotSubject(NotSubject::N2);
    let send = |p: Party, vat: VatTreatment, euros: i64| {
        let d = date!(2026 - 09 - 10);
        issue(&v, &draft_to(p, vec![line(euros, vat)], d), at(d), true).unwrap()
    };
    // Empresario francés con NIF-IVA: [59] y clave S.
    send(
        foreign("FR", IdType::NifIva, "12345678901", "Client SARL"),
        n2(),
        300,
    );
    // Cliente de EE. UU.: [120] y fuera del 349, sin aviso.
    send(
        foreign("US", IdType::Otro, "123456789", "Acme Inc"),
        n2(),
        200,
    );
    // Cliente de EE. UU. identificado por error con «NIF-IVA»: ya no se emite (no tiene).
    let d = date!(2026 - 09 - 10);
    let e = issue(
        &v,
        &draft_to(
            foreign("US", IdType::NifIva, "987654321", "Bad Data Inc"),
            vec![line(100, n2())],
            d,
        ),
        at(d),
        true,
    )
    .unwrap_err()
    .to_string();
    assert!(e.contains("«US»"), "{e}");
    // Servicios a Irlanda del Norte: Reino Unido a efectos de servicios ([120], no 349).
    send(
        foreign("XI", IdType::NifIva, "123456789", "Belfast Ltd"),
        n2(),
        50,
    );
    // Entrega intracomunitaria exenta a Alemania: [59] y clave E.
    send(
        foreign("DE", IdType::NifIva, "DE123456789", "Kunde GmbH"),
        VatTreatment::Exempt(Exemption::E5),
        1000,
    );

    let d303 = models::draft(&v, "303", 2026, 3).unwrap();
    assert_eq!(get(&d303, 59), eur(1300));
    assert_eq!(get(&d303, 120), eur(250));

    let d349 = models::draft(&v, "349", 2026, 3).unwrap();
    assert_eq!(d349.result, eur(1300), "{:?}", d349.details);
    assert_eq!(d349.details.len(), 2, "{:?}", d349.details);
    assert!(
        d349.details
            .iter()
            .all(|(n, _)| !n.contains("US") && !n.contains("XI"))
    );
    // Aviso del de Irlanda del Norte (servicios: Reino Unido).
    assert_eq!(d349.warnings.len(), 1, "{:?}", d349.warnings);
    assert!(d349.warnings[0].contains("«XI»"), "{:?}", d349.warnings);
    // [59] del 303 = total del 349 (la AEAT los cruza).
    assert_eq!(get(&d303, 59), d349.result);
}

// ---------- Devengo por la fecha de operación (art. 75 LIVA) ----------

fn emit_op(v: &Vault, p: Party, vat: VatTreatment, euros: i64, issued: Date, op: Date) -> String {
    let mut d = draft_to(p, vec![line(euros, vat)], issued);
    d.operation_date = Some(op);
    issue(v, &d, at(issued), true).unwrap().number
}

#[test]
fn devengo_por_la_fecha_de_operacion() {
    use taxpain_core::invoice::{Exemption, IdType};
    let v = setup();
    // Anticipo del 25/03 por una operación del 10/04: la fecha de operación no puede ser
    // posterior a la de la factura (VERI*FACTU §3.1.3.1), así que se factura sin ella y devenga
    // al facturar/cobrar (1T, art. 75.Dos LIVA).
    let mut anticipo = draft_to(
        es_client(),
        vec![line(500, taxed21())],
        date!(2026 - 03 - 25),
    );
    anticipo.operation_date = Some(date!(2026 - 04 - 10));
    assert!(issue(&v, &anticipo, at(date!(2026 - 03 - 25)), true).is_err());
    anticipo.operation_date = None;
    issue(&v, &anticipo, at(date!(2026 - 03 - 25)), true).unwrap();
    // Servicio prestado el 25/03 y facturado el 05/04: devenga en el 1T.
    emit_op(
        &v,
        es_client(),
        taxed21(),
        1000,
        date!(2026 - 04 - 05),
        date!(2026 - 03 - 25),
    );
    let de = || foreign("DE", IdType::NifIva, "DE123456789", "Kunde GmbH");
    // Entrega intracomunitaria: transporte el 10/02 y factura el 05/04 → devenga el 15/03 (1T).
    emit_op(
        &v,
        de(),
        VatTreatment::Exempt(Exemption::E5),
        700,
        date!(2026 - 04 - 05),
        date!(2026 - 02 - 10),
    );
    // Transporte el 20/03 y factura el 20/04 → devenga el 15/04 (2T).
    emit_op(
        &v,
        de(),
        VatTreatment::Exempt(Exemption::E5),
        2000,
        date!(2026 - 04 - 20),
        date!(2026 - 03 - 20),
    );
    let q1 = models::draft(&v, "303", 2026, 1).unwrap();
    let q2 = models::draft(&v, "303", 2026, 2).unwrap();
    assert_eq!((get(&q1, 7), get(&q1, 9)), (eur(1500), eur(315)));
    assert_eq!(get(&q2, 7), Money::ZERO);
    assert_eq!((get(&q1, 59), get(&q2, 59)), (eur(700), eur(2000)));
    // El 349 sigue al devengo, como [59].
    assert_eq!(models::draft(&v, "349", 2026, 1).unwrap().result, eur(700));
    assert_eq!(models::draft(&v, "349", 2026, 2).unwrap().result, eur(2000));

    // Operación de diciembre facturada en enero: en el 4T y en el 390 del año de la operación.
    emit_op(
        &v,
        es_client(),
        taxed21(),
        100,
        date!(2027 - 01 - 10),
        date!(2026 - 12 - 20),
    );
    let q4 = models::draft(&v, "303", 2026, 4).unwrap();
    assert_eq!(get(&q4, 9), eur(21));
    assert_eq!(
        get(&models::draft(&v, "303", 2027, 1).unwrap(), 9),
        Money::ZERO
    );
    let a390 = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&a390, 6), eur(315 + 21));
}

// ---------- Anulación de facturas de periodos declarados ----------

fn emit_21(v: &Vault, euros: i64, d: Date) -> taxpain_app::invoicing::IssuedInvoice {
    issue(
        v,
        &draft_to(es_client(), vec![line(euros, taxed21())], d),
        at(d),
        true,
    )
    .unwrap()
}

fn insert_return_303_1t(v: &Vault) {
    v.connection()
        .execute(
            "INSERT INTO tax_return(model, year, period, environment, declaration, result, boxes,
               file_sha256, file, csv, receipt_number, receipt, presented_at)
             VALUES ('303', 2026, '1T', 'produccion', 'I', 8400, '[]', 'x', 'x', 'CSV', '1', '{}',
               '2026-04-08T10:00:00Z')",
            [],
        )
        .unwrap();
}

fn insert_imported_303_1t(v: &Vault) {
    v.connection()
        .execute(
            "INSERT INTO filed_model(model, year, period, filename, file_sha256, source, boxes,
               imported_at)
             VALUES ('303', 2026, '1T', '303_1T.pdf', 'abc', 'pdf', '{}', '2026-04-09T10:00:00Z')",
            [],
        )
        .unwrap();
}

#[test]
fn anular_solo_con_el_periodo_de_iva_abierto() {
    // Factura del 10/02 anulada el 05/05: el 1T ya venció (20/04). Antes quedaba fuera del 1T
    // y el IVA ingresado no se recuperaba nunca; ahora hay que rectificarla.
    let v = setup();
    let a = emit_21(&v, 1000, date!(2026 - 02 - 10));
    let err = invoicing::cancel(&v, a.id, at(date!(2026 - 05 - 05)), true)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("rectificativa") && err.contains("R1") && err.contains("art. 89"),
        "{err}"
    );
    assert!(err.contains("plazo") && err.contains("1T de 2026"), "{err}");
    let q1 = models::draft(&v, "303", 2026, 1).unwrap();
    assert_eq!(get(&q1, 9), eur(210), "la factura sigue en el 1T");

    // Error del mismo trimestre: se puede anular y sale del 303.
    let b = emit_21(&v, 500, date!(2026 - 03 - 02));
    invoicing::cancel(&v, b.id, at(date!(2026 - 03 - 03)), true).unwrap();
    assert_eq!(
        get(&models::draft(&v, "303", 2026, 1).unwrap(), 9),
        eur(210)
    );

    // Acabado el trimestre pero dentro del plazo y sin presentar: también.
    let v = setup();
    let c = emit_21(&v, 300, date!(2026 - 03 - 20));
    invoicing::cancel(&v, c.id, at(date!(2026 - 04 - 10)), true).unwrap();

    // Presentado desde TaxPain en producción: no, aunque siga en plazo.
    let d = emit_21(&v, 400, date!(2026 - 03 - 25));
    insert_return_303_1t(&v);
    let err = invoicing::cancel(&v, d.id, at(date!(2026 - 04 - 10)), true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("presentado"), "{err}");

    // Presentado por la gestoría (importado de la carpeta): tampoco.
    let v = setup();
    let e = emit_21(&v, 400, date!(2026 - 03 - 25));
    insert_imported_303_1t(&v);
    assert!(
        invoicing::cancel(&v, e.id, at(date!(2026 - 04 - 10)), true)
            .unwrap_err()
            .to_string()
            .contains("presentado")
    );

    // El periodo es el del devengo: servicio de marzo facturado en abril, anulado en mayo con
    // el 1T vencido → no se anula aunque la factura sea del 2T.
    let v = setup();
    let mut late = draft_to(
        es_client(),
        vec![line(100, taxed21())],
        date!(2026 - 04 - 02),
    );
    late.operation_date = Some(date!(2026 - 03 - 28));
    let f = issue(&v, &late, at(date!(2026 - 04 - 02)), true).unwrap();
    assert!(invoicing::cancel(&v, f.id, at(date!(2026 - 05 - 05)), true).is_err());
}

// ---------- Fecha de registro en hora peninsular ----------

fn receive_full(
    v: &Vault,
    number: &str,
    reviewed: taxpain_app::received::Reviewed,
    now: OffsetDateTime,
) -> i64 {
    use taxpain_app::received::{Source, ingest_file, review};
    let rec = ingest_file(
        v,
        &format!("{number}.xml"),
        format!("<?xml version=\"1.0\"?><x n=\"{number}\"/>").as_bytes(),
        Source::Manual,
        now,
    )
    .unwrap();
    review(v, rec.id, &reviewed, now).unwrap();
    rec.id
}

fn reviewed(number: &str, base: i64, vat: i64, d: Date) -> taxpain_app::received::Reviewed {
    taxpain_app::received::Reviewed {
        supplier_nif: "B12345674".into(),
        supplier_name: "Proveedor SL".into(),
        number: number.into(),
        issue_date: d,
        base: eur(base),
        vat: eur(vat),
        withholding: eur(0),
        total: eur(base + vat),
        deductible_pct: 100,
        capital_good: false,
        nature: taxpain_app::received::Nature::General,
        asset_class: None,
        vat_regime: Default::default(),
        supplier_country: None,
        self_assessed_rate: None,
        intra_goods: false,
        cash_basis: false,
        import_dua: false,
        prorata_use: Default::default(),
    }
}

#[test]
fn fecha_de_registro_en_hora_peninsular() {
    let v = setup();
    // 1 de abril a las 00:30 en Madrid (CEST) = 31 de marzo a las 22:30 UTC: cuenta en el 2T.
    receive_full(
        &v,
        "G-9",
        reviewed("G-9", 1000, 210, date!(2026 - 03 - 31)),
        datetime!(2026-03-31 22:30 UTC),
    );
    // 1 de enero de 2027 a las 00:30 (CET) = 31 de diciembre a las 23:30 UTC: 1T de 2027.
    receive_full(
        &v,
        "G-10",
        reviewed("G-10", 100, 21, date!(2026 - 12 - 31)),
        datetime!(2026-12-31 23:30 UTC),
    );
    assert_eq!(
        get(&models::draft(&v, "303", 2026, 1).unwrap(), 29),
        Money::ZERO
    );
    assert_eq!(
        get(&models::draft(&v, "303", 2026, 2).unwrap(), 29),
        eur(210)
    );
    assert_eq!(
        get(&models::draft(&v, "303", 2026, 4).unwrap(), 29),
        Money::ZERO
    );
    assert_eq!(
        get(&models::draft(&v, "303", 2027, 1).unwrap(), 29),
        eur(21)
    );

    // Revisiones guardadas en UTC antes de la corrección: se pasan a la hora peninsular al leer.
    let id = receive_full(
        &v,
        "G-11",
        reviewed("G-11", 200, 42, date!(2026 - 06 - 30)),
        datetime!(2026-06-30 12:00 UTC),
    );
    v.connection()
        .execute(
            "UPDATE invoice_received SET reviewed_at = '2026-06-30T22:30:00Z' WHERE id = ?1",
            [id],
        )
        .unwrap();
    assert_eq!(
        get(&models::draft(&v, "303", 2026, 2).unwrap(), 29),
        eur(210)
    );
    assert_eq!(
        get(&models::draft(&v, "303", 2026, 3).unwrap(), 29),
        eur(42)
    );
}

// ---------- Devengo de las adquisiciones intracomunitarias ----------

fn intra(number: &str, base: i64, d: Date) -> taxpain_app::received::Reviewed {
    taxpain_app::received::Reviewed {
        supplier_nif: "IE1234567FA".into(),
        supplier_name: "Ejemplo Cloud Ltd".into(),
        vat: eur(0),
        total: eur(base),
        vat_regime: taxpain_app::received::VatRegime::Intracomunitaria,
        supplier_country: Some("IE".into()),
        self_assessed_rate: Some(Rate::percent(21)),
        ..reviewed(number, base, 0, d)
    }
}

#[test]
fn adquisicion_intracomunitaria_devenga_por_la_fecha_de_la_factura() {
    let v = setup();
    // Factura del proveedor irlandés del 28/03, registrada el 10/04: se autorrepercute en el 1T
    // (como la declara él en su 349) y se deduce en el 2T, cuando se tiene la factura.
    receive_full(
        &v,
        "IE-1",
        intra("IE-1", 100, date!(2026 - 03 - 28)),
        at(date!(2026 - 04 - 10)),
    );
    let q1 = models::draft(&v, "303", 2026, 1).unwrap();
    let q2 = models::draft(&v, "303", 2026, 2).unwrap();
    assert_eq!((get(&q1, 10), get(&q1, 11)), (eur(100), eur(21)));
    assert_eq!((get(&q1, 36), get(&q1, 37)), (Money::ZERO, Money::ZERO));
    assert_eq!(q1.result, eur(21));
    assert_eq!((get(&q2, 10), get(&q2, 11)), (Money::ZERO, Money::ZERO));
    assert_eq!((get(&q2, 36), get(&q2, 37)), (eur(100), eur(21)));
    assert!(
        q1.warnings.iter().any(|w| w.contains("IE-1")),
        "{:?}",
        q1.warnings
    );
    assert!(
        q2.warnings.iter().any(|w| w.contains("IE-1")),
        "{:?}",
        q2.warnings
    );
    // 349: clave I en el 1T, nada en el 2T.
    let m1 = models::draft(&v, "349", 2026, 1).unwrap();
    assert_eq!(m1.result, eur(100));
    assert!(m1.details[0].0.ends_with("clave I"), "{:?}", m1.details);
    assert_eq!(
        models::draft(&v, "349", 2026, 2).unwrap().result,
        Money::ZERO
    );

    // Factura y registro en el mismo trimestre: devengo y deducción juntos, sin avisos.
    receive_full(
        &v,
        "IE-2",
        intra("IE-2", 200, date!(2026 - 05 - 02)),
        at(date!(2026 - 05 - 03)),
    );
    let q2 = models::draft(&v, "303", 2026, 2).unwrap();
    assert_eq!((get(&q2, 10), get(&q2, 11)), (eur(200), eur(42)));
    assert_eq!((get(&q2, 36), get(&q2, 37)), (eur(300), eur(63)));
    assert!(
        !q2.warnings.iter().any(|w| w.contains("IE-2")),
        "{:?}",
        q2.warnings
    );

    // Entre ejercicios: factura del 28/12 registrada el 10/01 → devengo en el 390 de 2026 y
    // deducción en 2027.
    receive_full(
        &v,
        "IE-3",
        intra("IE-3", 1000, date!(2026 - 12 - 28)),
        at(date!(2027 - 01 - 10)),
    );
    let a = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(
        get(&a, 552),
        eur(210 + 21 + 42),
        "devengo de servicios intracomunitarios"
    );
    assert_eq!(
        get(&a, 638),
        eur(21 + 42),
        "deducción sólo de lo registrado en 2026"
    );
    let q1_27 = models::draft(&v, "303", 2027, 1).unwrap();
    assert_eq!((get(&q1_27, 11), get(&q1_27, 37)), (Money::ZERO, eur(210)));
}

// ---------- 200: pagos fraccionados de un ejercicio no natural ----------

fn filed_202(v: &Vault, year: i32, period: &str, euros: i64) {
    v.connection()
        .execute(
            "INSERT INTO tax_return(model, year, period, environment, declaration, result, boxes,
               file_sha256, file, csv, receipt_number, receipt, presented_at)
             VALUES ('202', ?1, ?2, 'produccion', 'I', ?3, ?4, 'x', 'x', 'CSV', '1', '{}',
               '2026-01-01T10:00:00Z')",
            taxpain_vault::rusqlite::params![
                year,
                period,
                euros * 100,
                format!("[[3, {}]]", euros * 100)
            ],
        )
        .unwrap();
}

#[test]
fn m200_pagos_fraccionados_de_un_ejercicio_de_julio_a_junio() {
    use taxpain_core::calendar::TaxpayerProfile;
    let v = setup();
    let mut p = TaxpayerProfile::sociedad_limitada();
    p.cierre_ejercicio = (time::Month::June, 30);
    v.set_setting("taxpayer_profile", &p).unwrap();
    // El 1P de 2026 es del ejercicio 2025/2026; el 2P y el 3P de 2026 y el 1P de 2027, del
    // 2026/2027; el 2P de 2027 ya es del siguiente.
    filed_202(&v, 2026, "1P", 777);
    filed_202(&v, 2026, "2P", 100);
    filed_202(&v, 2026, "3P", 200);
    filed_202(&v, 2027, "1P", 300);
    filed_202(&v, 2027, "2P", 999);
    let d = models::draft(&v, "200", 2026, 0).unwrap();
    assert_eq!(
        (get(&d, 601), get(&d, 603), get(&d, 605)),
        (eur(100), eur(200), eur(300)),
        "{:?}",
        d.boxes
    );
    // En el natural, los tres del año.
    let v = setup();
    filed_202(&v, 2026, "1P", 10);
    filed_202(&v, 2026, "2P", 20);
    filed_202(&v, 2026, "3P", 30);
    filed_202(&v, 2027, "1P", 40);
    let d = models::draft(&v, "200", 2026, 0).unwrap();
    assert_eq!(
        (get(&d, 601), get(&d, 603), get(&d, 605)),
        (eur(10), eur(20), eur(30))
    );
}

// ---------- 200: retenciones de la 473 y ajustes del IS (633/638) ----------

fn entry(v: &Vault, d: Date, concept: &str, lines: &[(&str, i64, i64)]) {
    use taxpain_core::accounting::account::acc;
    use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
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
    taxpain_app::accounting::manual_entry(
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

#[test]
fn m200_retenciones_de_la_473_y_ajustes_del_impuesto() {
    let v = setup();
    // Factura con retención del 15 %: 430 1.060 y 473 150 / 705 1.000 y 477 210.
    let mut d = draft_to(
        es_client(),
        vec![line(1000, taxed21())],
        date!(2026 - 03 - 10),
    );
    d.lines[0].withholding = Some(Rate::percent(15));
    issue(&v, &d, at(date!(2026 - 03 - 10)), true).unwrap();
    // Intereses del banco con retención del 19 %: no vienen de ninguna factura.
    entry(
        &v,
        date!(2026 - 06 - 30),
        "Intereses",
        &[("5720001", 81, 0), ("473", 19, 0), ("769", 0, 100)],
    );
    // Pago fraccionado (202) contra el banco: va en [601]–[605], no en retenciones.
    entry(
        &v,
        date!(2026 - 10 - 20),
        "Pago 202",
        &[("473", 500, 0), ("5720001", 0, 500)],
    );
    // Impuesto del ejercicio en dos asientos (630 → 4752 y 4752 → 473) y ajustes de años
    // anteriores: 633 (más gasto) y 638 (menos gasto).
    entry(
        &v,
        date!(2026 - 12 - 31),
        "IS",
        &[("6300", 1000, 0), ("4752", 0, 1000)],
    );
    entry(
        &v,
        date!(2026 - 12 - 31),
        "Retenciones y pagos a cuenta",
        &[("4752", 669, 0), ("473", 0, 669)],
    );
    entry(
        &v,
        date!(2026 - 12 - 31),
        "Ajuste IS 2025",
        &[("633", 50, 0), ("4752", 0, 50)],
    );
    entry(
        &v,
        date!(2026 - 12 - 31),
        "Ajuste IS 2024",
        &[("4709", 20, 0), ("638", 0, 20)],
    );
    let d = models::draft(&v, "200", 2026, 0).unwrap();
    // Retenciones: 150 de la factura + 19 de los intereses, sin el 202 ni el asiento del IS.
    assert_eq!(get(&d, 1766), eur(169), "{:?}", d.boxes);
    // Gasto por IS: 1.000 + 50 − 20.
    assert_eq!(get(&d, 301), eur(1030));
    // Resultado contable: 1.000 + 100 − 1.030 = 70; antes del IS, 1.100.
    assert_eq!(get(&d, 500), eur(70));
    assert_eq!(get(&d, 501), eur(1100));
    assert!(
        !d.warnings.iter().any(|w| w.contains("473")),
        "{:?}",
        d.warnings
    );
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

// ---------- 200: bases negativas y casilla de cifra de negocios de las grandes empresas ----------

/// Con 25 M€ de ventas en 2025 (los 12 meses anteriores al periodo de 2026), las bases negativas
/// se compensan hasta el 50 % de la base (DA 15.ª LIS) y el fichero no marca «inferior a 20
/// millones» (campo 102 de la página 01). Sin esas ventas, el 70 % y la marca.
#[test]
fn m200_bases_negativas_y_cifra_de_negocios_de_20_millones() {
    let draft_200 = |grande: bool| {
        let v = setup();
        if grande {
            entry(
                &v,
                date!(2025 - 01 - 10),
                "Ventas",
                &[("4300001", 25_000_000, 0), ("705", 0, 25_000_000)],
            );
        }
        entry(
            &v,
            date!(2026 - 03 - 01),
            "Ventas",
            &[("5720001", 4_000_000, 0), ("705", 0, 4_000_000)],
        );
        let mut s = models::settings(&v).unwrap();
        s.years.insert(
            2026,
            models::YearSettings {
                pending_losses: eur(5_000_000),
                ..Default::default()
            },
        );
        models::save_settings(&v, &s, at(date!(2026 - 12 - 31))).unwrap();
        models::draft(&v, "200", 2026, 0).unwrap()
    };
    // Campo 102 (posición 241) del registro de la página 01.
    let under_20m = |d: &models::Draft| {
        let page = &d.file[d.file.find("<T20001").expect("página 01")..];
        page.chars().nth(240)
    };
    let d = draft_200(true);
    assert_eq!(get(&d, 547), eur(2_000_000), "{:?}", d.boxes);
    assert_eq!(under_20m(&d), Some('0'));
    let d = draft_200(false);
    assert_eq!(get(&d, 547), eur(2_800_000), "{:?}", d.boxes);
    assert_eq!(under_20m(&d), Some('1'));
}
