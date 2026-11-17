//! Periodos mensuales (303, 111, 115, 123 y 349: gran empresa, REDEME, SII), el SII (cuándo
//! es obligatorio y el plazo de cada factura) y la comparación de todos los modelos de la
//! gestoría con lo que calcula TaxPain (casillas con su explicación, perceptores del 190 y su
//! hallazgo en la revisión). Datos inventados.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;

use taxpain_app::invoicing::{self, InvoiceDraft, issue};
use taxpain_app::models::{self, ModelSettings};
use taxpain_app::presentados;
use taxpain_app::received::{AssetClass, Nature, Reviewed, Source, ingest_file, review};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::invoice::{
    InvoiceKind, InvoiceLine, Party, PartyId, PersonName, RoundingMode, VatTreatment,
};
use taxpain_core::money::{Money, Rate};
use taxpain_models::periodo;
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
            id: PartyId::Nif("12345678Z".into()),
            name: "Carmen Española Española".into(),
            address: addr(),
            person: Some(PersonName {
                name: "Carmen".into(),
                first_surname: "Española".into(),
                second_surname: Some("Española".into()),
            }),
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

fn emit(v: &Vault, euros: i64, withholding: Option<Rate>, d: Date) {
    let draft = InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: d,
        operation_date: None,
        recipient: Some(Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Cliente SL".into(),
            address: addr(),
            person: None,
        }),
        lines: vec![InvoiceLine {
            description: "Consultoría".into(),
            quantity_milli: 1000,
            unit_price: eur(euros),
            discount: Rate::ZERO,
            vat: VatTreatment::Taxed {
                rate: Rate::percent(21),
            },
            surcharge: None,
            withholding,
            advance: false,
        }],
        rectified: vec![],
        rectified_amounts: None,
        description: "Servicios".into(),
        notes: None,
        currency: None,
        oss: None,
    };
    issue(v, &draft, at(d), true).unwrap();
}

/// Recibida revisada el día `d` (la fecha de registro decide el trimestre).
fn receive(v: &Vault, nif: &str, number: &str, base: i64, vat: i64, wh: i64, d: Date) {
    receive_as(v, nif, number, (base, vat, wh), d, Nature::General, false);
}

fn receive_as(
    v: &Vault,
    nif: &str,
    number: &str,
    (base, vat, wh): (i64, i64, i64),
    d: Date,
    nature: Nature,
    capital_good: bool,
) {
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
            supplier_name: "Proveedor".into(),
            number: number.into(),
            issue_date: d,
            base: eur(base),
            vat: eur(vat),
            withholding: eur(wh),
            total: eur(base + vat - wh),
            deductible_pct: 100,
            capital_good,
            nature,
            asset_class: capital_good.then_some(AssetClass::Informatica),
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

fn addr() -> Option<taxpain_core::invoice::Address> {
    Some(taxpain_core::invoice::Address {
        street: "Calle Inventada 1".into(),
        post_code: "46001".into(),
        town: "València".into(),
        province: "Valencia".into(),
        country: "ES".into(),
    })
}

fn get(d: &models::Draft, b: u16) -> Money {
    d.boxes
        .iter()
        .find(|(k, _)| *k == b)
        .map_or(Money::ZERO, |(_, v)| *v)
}

fn julio_y_agosto() -> Vault {
    let v = setup();
    emit(&v, 6000, None, date!(2026 - 07 - 10));
    emit(&v, 1000, None, date!(2026 - 08 - 03));
    receive(&v, "B12345674", "G-7", 500, 105, 0, date!(2026 - 07 - 20));
    receive(&v, "A22222228", "P-8", 400, 84, 60, date!(2026 - 08 - 06));
    receive(
        &v,
        "B12345674",
        "ALQ-9",
        1000,
        210,
        190,
        date!(2026 - 09 - 01),
    );
    v
}

#[test]
fn el_303_de_cada_mes_suma_el_del_trimestre() {
    let v = julio_y_agosto();
    let jul = models::draft(&v, "303", 2026, periodo::de_mes(7)).unwrap();
    let ago = models::draft(&v, "303", 2026, periodo::de_mes(8)).unwrap();
    let sep = models::draft(&v, "303", 2026, periodo::de_mes(9)).unwrap();
    let t3 = models::draft(&v, "303", 2026, 3).unwrap();
    assert_eq!(get(&jul, 7), eur(6000));
    assert_eq!(get(&ago, 7), eur(1000));
    assert_eq!(get(&jul, 29), eur(105));
    assert_eq!(
        get(&jul, 27) + get(&ago, 27) + get(&sep, 27),
        get(&t3, 27),
        "el devengado de los tres meses es el del trimestre"
    );
    // El fichero lleva el periodo «07» (el DR del 303 admite «01»…«12»).
    assert!(jul.file.starts_with("<T3030202607"), "{}", &jul.file[..20]);
    assert!(
        jul.file_name.ends_with("_2026_07_303.txt"),
        "{}",
        jul.file_name
    );
    assert_eq!(models::period_code("303", jul.quarter), "07");
    // Sin perfil mensual, se avisa de que lo normal es el trimestre.
    assert!(
        jul.warnings
            .iter()
            .any(|w| w.contains("no dice que el 303 sea mensual"))
    );
}

#[test]
fn la_compensacion_se_encadena_mes_a_mes_y_el_redeme_devuelve_cada_mes() {
    let v = setup();
    // Enero: sólo un gasto (sale a compensar 210); febrero: una venta de 1.000 (210 de IVA).
    receive(&v, "B12345674", "G-1", 1000, 210, 0, date!(2026 - 01 - 15));
    emit(&v, 1000, None, date!(2026 - 02 - 10));
    let ene = models::draft(&v, "303", 2026, periodo::de_mes(1)).unwrap();
    assert_eq!(ene.result, eur(-210));
    assert_eq!(ene.declaration, "C");
    let feb = models::draft(&v, "303", 2026, periodo::de_mes(2)).unwrap();
    assert_eq!(get(&feb, 110), eur(210));
    assert_eq!(get(&feb, 78), eur(210));
    assert_eq!(feb.result, Money::ZERO);
    // En el REDEME se puede pedir la devolución en cualquier mes (art. 116 LIVA).
    let s = ModelSettings {
        redeme: true,
        request_refund: true,
        iban: Some("ES9121000418450200051332".into()),
        ..Default::default()
    };
    models::save_settings(&v, &s, datetime!(2026-02-01 9:00 UTC)).unwrap();
    let ene = models::draft(&v, "303", 2026, periodo::de_mes(1)).unwrap();
    assert_eq!(ene.declaration, "D");
    // El campo 12 de la página 01 (inscrito en el REDEME) va a «1».
    let p1 = taxpain_models::m303::spec().page("01").unwrap();
    let at = ene.file.find("<T30301000>").unwrap();
    let rec: String = ene.file[at..].chars().take(p1.len()).collect();
    let r = taxpain_models::dr::PageReader::new(p1, &rec).unwrap();
    assert_eq!(r.raw(12).unwrap(), "1");
    assert_eq!(r.raw(10).unwrap(), "01");
    // Sin el REDEME, el saldo a favor de enero se compensa aunque se pida la devolución.
    let s = ModelSettings {
        request_refund: true,
        ..Default::default()
    };
    models::save_settings(&v, &s, datetime!(2026-02-01 9:00 UTC)).unwrap();
    let ene = models::draft(&v, "303", 2026, periodo::de_mes(1)).unwrap();
    assert_eq!(ene.declaration, "C");
}

#[test]
fn retenciones_mensuales_de_la_gran_empresa() {
    let v = julio_y_agosto();
    let ago = models::draft(&v, "111", 2026, periodo::de_mes(8)).unwrap();
    assert_eq!(get(&ago, 9), eur(60));
    assert!(
        ago.file_name.ends_with("_2026_08_111.txt"),
        "{}",
        ago.file_name
    );
    let jul = models::draft(&v, "111", 2026, periodo::de_mes(7)).unwrap();
    assert_eq!(get(&jul, 9), Money::ZERO);
    let sep = models::draft(&v, "115", 2026, periodo::de_mes(9)).unwrap();
    assert_eq!(get(&sep, 3), eur(190));
    assert_eq!(models::period_code("115", sep.quarter), "09");
    // El 123 del mes, sin rentas: no se presenta.
    let d = models::draft(&v, "123", 2026, periodo::de_mes(9)).unwrap();
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("no hay que presentar el 123"))
    );
    // Con el perfil de gran empresa, el borrador trimestral avisa de que es sólo comprobación.
    let mut p = taxpain_core::calendar::TaxpayerProfile::autonomo();
    p.gran_empresa = true;
    v.set_setting("taxpayer_profile", &p).unwrap();
    let t3 = models::draft(&v, "111", 2026, 3).unwrap();
    assert!(
        t3.warnings[0].contains("lo presentas cada mes"),
        "{:?}",
        t3.warnings
    );
    assert_eq!(models::monthly_model(&v, "111").unwrap(), Some(true));
    // El 130 no tiene meses.
    assert!(models::draft(&v, "130", 2026, periodo::de_mes(9)).is_err());
}

#[test]
fn el_sii_obligatorio_y_sus_plazos() {
    let v = julio_y_agosto();
    let hoy = date!(2026 - 08 - 04);
    let e = taxpain_app::sii::estado(&v, hoy).unwrap();
    assert!(!e.obligatorio && !e.voluntario);
    let s = ModelSettings {
        redeme: true,
        ..Default::default()
    };
    models::save_settings(&v, &s, datetime!(2026-08-01 9:00 UTC)).unwrap();
    let e = taxpain_app::sii::estado(&v, hoy).unwrap();
    assert!(e.obligatorio);
    assert!(e.motivos[0].contains("REDEME"));
    // Sin el IVA mensual en el perfil, se avisa.
    assert!(e.avisos.iter().any(|a| a.contains("Mensual (con SII)")));
    assert!(!e.formato_sii);
    let p =
        taxpain_app::sii::plazos(&v, date!(2026 - 07 - 01), date!(2026 - 08 - 31), hoy).unwrap();
    // Emitida el viernes 10 de julio: lunes 13, martes 14, miércoles 15 y jueves 16.
    let f = p
        .iter()
        .find(|x| x.libro == "emitida" && x.fecha == date!(2026 - 07 - 10))
        .unwrap();
    assert_eq!(f.limite, date!(2026 - 07 - 16));
    assert!(f.dias < 0);
    // Recibida registrada el jueves 6 de agosto: hasta el miércoles 12.
    let r = p.iter().find(|x| x.numero == "P-8").unwrap();
    assert_eq!(r.limite, date!(2026 - 08 - 12));
    assert_eq!(r.dias, 8);
}

#[test]
fn compara_el_303_con_su_explicacion_y_lo_lleva_a_la_revision() {
    let v = julio_y_agosto();
    // La gestoría declaró menos IVA soportado en el 3T.
    let t3 = models::draft(&v, "303", 2026, 3).unwrap();
    let mut boxes: BTreeMap<u16, Money> = t3.boxes.iter().copied().collect();
    boxes.insert(28, get(&t3, 28) - eur(100));
    boxes.insert(29, get(&t3, 29) - eur(21));
    let id = presentados::save_manual(
        &v,
        "303",
        2026,
        "3T",
        &boxes,
        datetime!(2026-10-20 9:00 UTC),
    )
    .unwrap();
    let c = presentados::compare(&v, id).unwrap();
    let r29 = c.rows.iter().find(|r| r.box_ == 29).unwrap();
    assert_eq!(r29.diff, eur(21));
    assert!(r29.label.contains("Cuota"), "{}", r29.label);
    let e = r29.explanation.as_deref().unwrap();
    assert!(e.starts_with("TaxPain calcula 21,00 € más"), "{e}");
    assert!(e.contains("IVA soportado"), "{e}");
    // Las casillas que cuadran no llevan explicación.
    assert!(
        c.rows
            .iter()
            .find(|r| r.box_ == 7)
            .unwrap()
            .explanation
            .is_none()
    );

    // Un mes presentado también se compara («07» es el periodo 107).
    let jul = models::draft(&v, "303", 2026, periodo::de_mes(7)).unwrap();
    let boxes: BTreeMap<u16, Money> = jul.boxes.iter().copied().collect();
    let id = presentados::save_manual(
        &v,
        "303",
        2026,
        "07",
        &boxes,
        datetime!(2026-08-20 9:00 UTC),
    )
    .unwrap();
    assert_eq!(presentados::compare(&v, id).unwrap().mismatches, 0);

    // La revisión sin carpeta (Evolución → Qué revisar) lo cuenta como hallazgo.
    let rev = taxpain_app::revision::revisar_aeat(&v, date!(2026 - 11 - 17)).unwrap();
    let h = rev
        .hallazgos
        .iter()
        .find(|h| h.codigo == "modelos.distinto_de_taxpain")
        .unwrap_or_else(|| panic!("{:#?}", rev.hallazgos));
    assert!(h.titulo.contains("303 de 3T 2026"), "{}", h.titulo);
    assert!(h.detalle.contains("[29]"), "{}", h.detalle);
    assert_eq!(
        rev.hallazgos
            .iter()
            .filter(|h| h.codigo == "modelos.distinto_de_taxpain")
            .count(),
        1,
        "el de julio cuadra"
    );
    assert!(
        rev.tablas
            .iter()
            .any(|t| t.titulo == "Lo presentado frente a TaxPain")
    );
}

#[test]
fn compara_el_190_perceptor_a_perceptor() {
    use taxpain_models::informative::{self, Key190, Payment190};
    let v = julio_y_agosto();
    // El 190 de la gestoría: el profesional de TaxPain y otro que TaxPain no tiene.
    let p = |nif: &str, g: i64, r: i64| Payment190 {
        nif: nif.into(),
        name: format!("Perceptor {nif}"),
        province: "46".into(),
        gross: eur(g),
        withheld: eur(r),
        employee: None,
        key: Key190::G(1),
        deductible: Money::ZERO,
    };
    let r = informative::compute_190(2026, &[p("A22222228", 400, 60), p("12345678Z", 1000, 150)]);
    let who = taxpain_models::Declarant {
        nif: "12345678Z".into(),
        name: "Española Española".into(),
        given_name: Some("Carmen".into()),
    };
    let file = informative::file_190(&r, &who).unwrap();
    let bytes = taxpain_models::dr::to_latin1(&file).unwrap();
    let read = presentados::read_informative_filed(&bytes).unwrap();
    assert_eq!(read.boxes.get(&2), Some(&eur(1400)));
    assert_eq!(read.boxes.get(&3), Some(&eur(210)));
    let id = presentados::store(
        &v,
        "190_2026.txt",
        &bytes,
        &read,
        datetime!(2027-01-20 9:00 UTC),
    )
    .unwrap();
    let c = presentados::compare(&v, id).unwrap();
    assert_eq!(c.lines.len(), 2, "{:#?}", c.lines);
    let falta = c.lines.iter().find(|l| l.nif == "12345678Z").unwrap();
    assert_eq!(falta.ours, None);
    assert_eq!(falta.theirs, Some(eur(1000)));
    assert!(falta.explanation.as_deref().unwrap().contains("TaxPain no"));
    let bien = c.lines.iter().find(|l| l.nif == "A22222228").unwrap();
    assert_eq!(
        (bien.key.as_str(), bien.explanation.as_deref()),
        ("G01", None)
    );
    // Totales: [02] y [03], con su diferencia explicada.
    let r2 = c.rows.iter().find(|r| r.box_ == 2).unwrap();
    assert_eq!((r2.ours, r2.theirs), (eur(400), Some(eur(1400))));
    assert!(r2.explanation.as_deref().unwrap().contains("perceptor"));
    assert_eq!(c.mismatches, 3);
}
