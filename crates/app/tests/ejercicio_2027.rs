//! El ejercicio 2027 no rompe nada: calendario completo con sus festivos y los borradores de
//! todos los modelos con datos de 2027 en una empresa inventada (Estudio Demo SL), cada uno con
//! el último diseño de registro vigente y su aviso.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::record_entry;
use taxpain_app::invoicing::{self, InvoiceDraft, issue};
use taxpain_app::received::{Nature, Reviewed, Source, ingest_file, review};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::{cambio_ejercicio, models};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::calendar::{Ccaa, Model, TaxpayerProfile, calendar_for, deadlines_in_year};
use taxpain_core::invoice::{
    Address, InvoiceKind, InvoiceLine, Party, PartyId, PersonName, RoundingMode, VatTreatment,
};
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

fn addr() -> Option<Address> {
    Some(Address {
        street: "Calle Inventada 1".into(),
        post_code: "28001".into(),
        town: "Madrid".into(),
        province: "Madrid".into(),
        country: "ES".into(),
    })
}

fn empresa() -> Vault {
    con_titular(
        Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Estudio Demo SL".into(),
            address: addr(),
            person: None,
        },
        TaxpayerProfile::sociedad_limitada(),
    )
}

/// La autónoma de prueba, para el 130 (sólo personas físicas).
fn autonoma() -> Vault {
    con_titular(
        Party {
            id: PartyId::Nif("12345678Z".into()),
            name: "Carmen Española Española".into(),
            address: addr(),
            person: Some(PersonName {
                name: "Carmen".into(),
                first_surname: "Española".into(),
                second_surname: Some("Española".into()),
            }),
        },
        TaxpayerProfile::autonomo(),
    )
}

fn con_titular(issuer: Party, base: TaxpayerProfile) -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
        issuer,
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
    invoicing::save_settings(&v, &s, datetime!(2027-01-02 9:00 UTC)).unwrap();
    let perfil = TaxpayerProfile {
        ccaa: Some(Ccaa::Madrid),
        retiene_trabajo_profesionales: true,
        retiene_alquileres: true,
        obligado_347: true,
        ..base
    };
    v.set_setting("taxpayer_profile", &perfil).unwrap();
    v
}

fn emitir(v: &Vault, euros: i64, d: Date) {
    let draft = InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: d,
        operation_date: None,
        recipient: Some(Party {
            id: PartyId::Nif("A22222228".into()),
            name: "Cliente Inventado SA".into(),
            address: addr(),
            person: None,
        }),
        lines: vec![InvoiceLine {
            description: "Proyecto".into(),
            quantity_milli: 1000,
            unit_price: eur(euros),
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
        description: "Servicios".into(),
        notes: None,
        currency: None,
        oss: None,
    };
    issue(v, &draft, at(d), true).unwrap();
}

fn recibir(v: &Vault, nif: &str, number: &str, (base, vat, wh): (i64, i64, i64), d: Date) {
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
            supplier_name: "Proveedor Inventado".into(),
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

/// Un año entero de 2027: una venta y un gasto por trimestre, un profesional con retención, un
/// alquiler y una nómina.
fn año_2027() -> Vault {
    con_datos(empresa())
}

fn con_datos(v: Vault) -> Vault {
    for (q, mes) in [(1, 2), (2, 5), (3, 8), (4, 11)] {
        let d = Date::from_calendar_date(2027, time::Month::try_from(mes).unwrap(), 10).unwrap();
        emitir(&v, 10_000, d);
        recibir(&v, "B12345674", &format!("G-{q}"), (2_000, 420, 0), d);
    }
    recibir(
        &v,
        "12345678Z",
        "P-1",
        (1_000, 210, 150),
        date!(2027 - 03 - 15),
    );
    recibir(
        &v,
        "12345678Z",
        "ALQ-1",
        (1_000, 210, 190),
        date!(2027 - 04 - 01),
    );
    record_entry(
        v.connection(),
        &JournalEntry {
            date: date!(2027 - 06 - 30),
            concept: "Nómina de junio".into(),
            origin: Origin::Nomina,
            lines: vec![
                EntryLine::debit(acc("640"), eur(2_000)),
                EntryLine::credit(acc("4751"), eur(300)),
                EntryLine::credit(acc("465"), eur(1_700)),
            ],
        },
        at(date!(2027 - 06 - 30)),
    )
    .unwrap();
    v
}

#[test]
fn calendario_de_2027_de_la_empresa() {
    let v = año_2027();
    let perfil: TaxpayerProfile = v.setting("taxpayer_profile").unwrap().unwrap();
    let (cal, cargados) = calendar_for(2027, perfil.ccaa, &perfil.festivos_locales);
    assert!(cargados);
    let ds = deadlines_in_year(&perfil, 2027, &cal);
    for m in [
        Model::M303,
        Model::M111,
        Model::M115,
        Model::M202,
        Model::M200,
        Model::M390,
    ] {
        assert!(ds.iter().any(|d| d.model == m), "falta el {}", m.code());
    }
    // El 4T de 2026 (303) vence en sábado 30/01/2027: el lunes 1 de febrero.
    let d = ds
        .iter()
        .find(|d| d.model == Model::M303 && d.fiscal_year == 2026)
        .unwrap();
    assert_eq!(d.due_on, date!(2027 - 02 - 01));
    // Aviso del Panel en diciembre de 2026 para 2027.
    let aviso = cambio_ejercicio::aviso_cambio_de_año(&v, date!(2026 - 12 - 01))
        .unwrap()
        .unwrap();
    assert_eq!(aviso.year, 2027);
    assert!(aviso.por_revisar() > 0);
    assert!(
        cambio_ejercicio::aviso_cambio_de_año(&v, date!(2027 - 03 - 01))
            .unwrap()
            .is_none()
    );
}

#[test]
fn borradores_de_todos_los_modelos_en_2027() {
    let v = año_2027();
    let periodos: &[(&str, &[u8])] = &[
        ("303", &[1, 2, 3, 4]),
        ("111", &[1, 2, 3, 4]),
        ("115", &[1, 2, 3, 4]),
        ("123", &[1, 2, 3, 4]),
        ("349", &[1, 2, 3, 4]),
        ("202", &[1, 2, 3]),
        ("390", &[0]),
        ("347", &[0]),
        ("190", &[0]),
        ("180", &[0]),
        ("193", &[0]),
        ("184", &[0]),
        ("200", &[0]),
    ];
    for (m, qs) in periodos {
        for q in *qs {
            let d = models::draft(&v, m, 2027, *q).unwrap_or_else(|e| panic!("{m} {q}: {e}"));
            let aviso = format!("Diseño de registro del {m} de 2027 no publicado todavía");
            assert!(
                d.warnings.iter().any(|w| w.starts_with(&aviso)),
                "{m} {q}: {:?}",
                d.warnings
            );
        }
    }
    // El 130, en la autónoma con los mismos datos.
    let a = con_datos(autonoma());
    for q in 1..=4 {
        let d = models::draft(&a, "130", 2027, q).unwrap_or_else(|e| panic!("130 {q}: {e}"));
        assert!(
            d.warnings
                .iter()
                .any(|w| w.starts_with("Diseño de registro del 130 de 2027 no publicado")),
            "{:?}",
            d.warnings
        );
    }
    // Las cifras salen: 303 del 1T con 2.100 de IVA devengado y 420 + 210 deducido.
    let d = models::draft(&v, "303", 2027, 1).unwrap();
    assert!(
        d.boxes.iter().any(|(_, x)| *x == eur(2_100)),
        "{:?}",
        d.boxes
    );
    // 111 del 1T: la retención del profesional; 115 del 2T: la del alquiler.
    assert_eq!(models::draft(&v, "111", 2027, 1).unwrap().result, eur(150));
    assert_eq!(models::draft(&v, "115", 2027, 2).unwrap().result, eur(190));
}
