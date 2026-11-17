//! IVA de holding y cartera (G28): reglas de afectación en el 303, sectores diferenciados y
//! regularización de bienes de inversión (arts. 107 a 110 LIVA). Datos inventados.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::invoicing::{self, InvoiceDraft, issue};
use taxpain_app::iva_holding::{self, Ambito, Config, DatosBien, Regla, Sector, VentaIva};
use taxpain_app::models::{self, ModelSettings};
use taxpain_app::received::{AssetClass, Nature, Reviewed, Source, ingest_file, review};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::invoice::{
    Exemption, InvoiceKind, InvoiceLine, Party, PartyId, PersonName, RoundingMode, VatTreatment,
};
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

fn get(d: &models::Draft, b: u16) -> Money {
    d.boxes
        .iter()
        .find(|(k, _)| *k == b)
        .map_or(Money::ZERO, |(_, v)| *v)
}

fn emit_exempt(v: &Vault, euros: i64, d: Date) {
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
            description: "Operación exenta".into(),
            quantity_milli: 1000,
            unit_price: eur(euros),
            discount: Rate::ZERO,
            vat: VatTreatment::Exempt(Exemption::E1),
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

fn regla(ambito: Ambito, clave: &str, pct: u8, desde: Date) -> Regla {
    Regla {
        id: 0,
        ambito,
        clave: clave.into(),
        pct,
        sector_id: None,
        criterio: "Criterio de reparto inventado para la prueba".into(),
        desde,
    }
}

#[test]
fn reglas_de_afectacion_en_el_303_y_documentadas() {
    let v = setup();
    let now = at(date!(2026 - 02 - 01));
    emit(&v, 1000, None, date!(2026 - 02 - 10));
    receive(&v, "B12345674", "G-0", 100, 21, 0, date!(2026 - 01 - 05));
    receive(&v, "A22222228", "G-1", 1000, 210, 0, date!(2026 - 02 - 15));
    receive(&v, "B12345674", "G-2", 2000, 420, 0, date!(2026 - 02 - 16));
    let antes = models::draft(&v, "303", 2026, 1).unwrap();
    assert_eq!(get(&antes, 29), eur(651));
    // La gestora de la cartera: nada deducible; la cuenta 629 (por defecto), el 60 %.
    let mut r = regla(Ambito::Proveedor, "a-22222228", 0, date!(2026 - 02 - 01));
    r.criterio = "Custodia de la cartera: sólo la tenencia".into();
    iva_holding::guardar_regla(&v, &r, now).unwrap();
    iva_holding::guardar_regla(
        &v,
        &regla(Ambito::Cuenta, "629", 60, date!(2026 - 02 - 01)),
        now,
    )
    .unwrap();
    // Una segunda regla para la misma clave se rechaza; un criterio vacío, también.
    assert!(
        iva_holding::guardar_regla(
            &v,
            &regla(Ambito::Cuenta, "629", 50, date!(2026 - 02 - 01)),
            now
        )
        .is_err()
    );
    let mut sin = regla(Ambito::Cuenta, "628", 50, date!(2026 - 02 - 01));
    sin.criterio = " ".into();
    assert!(iva_holding::guardar_regla(&v, &sin, now).is_err());
    // G-0 se registró antes de las reglas: entera. G-1: 0. G-2: 60 % de 420 = 252.
    let t1 = models::draft(&v, "303", 2026, 1).unwrap();
    assert_eq!(get(&t1, 29), eur(273));
    assert_eq!(get(&t1, 28), eur(1300));
    let ap = iva_holding::aplicaciones(&v, 2026).unwrap();
    assert_eq!(ap.len(), 2);
    assert_eq!(ap.iter().map(|a| a.no_deducido).sum::<Money>(), eur(378));
    assert!(ap.iter().any(|a| a.criterio.contains("tenencia")));
    // Queda en la auditoría.
    let n: i64 = v
        .connection()
        .query_row(
            "SELECT count(*) FROM audit_log WHERE action = 'iva.afectacion'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 2);
    // Borrar pide confirmación.
    let id = iva_holding::reglas(&v).unwrap()[0].id;
    assert!(iva_holding::borrar_regla(&v, id, false, now).is_err());
    iva_holding::borrar_regla(&v, id, true, now).unwrap();
    assert_eq!(iva_holding::reglas(&v).unwrap().len(), 1);
}

#[test]
fn sectores_diferenciados_se_avisan_y_se_separan() {
    let v = setup();
    let now = at(date!(2026 - 01 - 02));
    let s = ModelSettings {
        prorata: Some(80),
        ..Default::default()
    };
    models::save_settings(&v, &s, now).unwrap();
    emit(&v, 1000, None, date!(2026 - 02 - 10));
    receive(&v, "A22222228", "F-1", 1000, 210, 0, date!(2026 - 02 - 15));
    receive(&v, "B12345674", "F-2", 1000, 210, 0, date!(2026 - 02 - 16));
    let gestion = iva_holding::guardar_sector(
        &v,
        &Sector {
            id: 0,
            nombre: "Gestión de filiales".into(),
            cnae: "7010".into(),
            pct: 100,
            volumen: eur(200_000),
            notas: String::new(),
        },
        now,
    )
    .unwrap();
    let financiacion = iva_holding::guardar_sector(
        &v,
        &Sector {
            id: 0,
            nombre: "Financiación al grupo".into(),
            cnae: "6499".into(),
            pct: 0,
            volumen: eur(80_000),
            notas: String::new(),
        },
        now,
    )
    .unwrap();
    assert!(gestion != financiacion);
    let mut r = regla(Ambito::Proveedor, "A22222228", 100, date!(2026 - 01 - 01));
    r.sector_id = Some(financiacion);
    iva_holding::guardar_regla(&v, &r, now).unwrap();
    // Sin separar: las dos al 80 % provisional (168 + 168) y el aviso.
    let t1 = models::draft(&v, "303", 2026, 1).unwrap();
    assert_eq!(get(&t1, 29), eur(336));
    assert!(
        t1.warnings
            .iter()
            .any(|w| w.contains("sectores diferenciados")),
        "{:?}",
        t1.warnings
    );
    // Separando: la del sector de financiación (0 %) no se deduce; la otra, al 80 %.
    iva_holding::guardar_config(
        &v,
        &Config {
            sectores_diferenciados: true,
        },
        now,
    )
    .unwrap();
    let t1 = models::draft(&v, "303", 2026, 1).unwrap();
    assert_eq!(get(&t1, 29), eur(168));
    assert!(!t1.warnings.iter().any(|w| w.contains("no se separa")));
    let d = iva_holding::diagnostico(&v, 2026).unwrap();
    assert!(d.comprobacion.hay_diferenciados);
    assert_eq!(d.aplicaciones.len(), 1);
    assert_eq!(
        d.aplicaciones[0].sector.as_deref(),
        Some("Financiación al grupo")
    );
    // Borrar la actividad deja la regla sin sector.
    iva_holding::borrar_sector(&v, financiacion, true, now).unwrap();
    assert_eq!(iva_holding::reglas(&v).unwrap()[0].sector_id, None);
}

/// Prorrata general del 50 % en 2026 (1.000 con IVA y 1.000 exentos) frente al 100 % de 2025.
fn con_prorrata(v: &Vault) {
    let s = ModelSettings {
        prorata: Some(100),
        prorata_history: [(2025, 100)].into_iter().collect(),
        cnae: "7010".into(),
        activity_description: "Gestión".into(),
        ..Default::default()
    };
    models::save_settings(v, &s, at(date!(2026 - 01 - 02))).unwrap();
    emit(v, 1000, None, date!(2026 - 02 - 10));
    emit_exempt(v, 1000, date!(2026 - 02 - 11));
}

#[test]
fn regularizacion_anual_sin_los_bienes_de_menos_de_3005_euros() {
    let v = setup();
    receive_as(
        &v,
        "B12345674",
        "BI-1",
        (10_000, 2_100, 0),
        date!(2025 - 06 - 10),
        Nature::General,
        true,
    );
    // 2.000 € de base: no es bien de inversión a efectos del IVA (art. 108.Dos.5.º).
    receive_as(
        &v,
        "B12345674",
        "BI-2",
        (2_000, 420, 0),
        date!(2025 - 06 - 11),
        Nature::General,
        true,
    );
    con_prorrata(&v);
    // [43] = 2.100 × (50 − 100) % / 5 = −210, sólo por BI-1.
    let t4 = models::draft(&v, "303", 2026, 4).unwrap();
    assert_eq!(get(&t4, 43), -eur(210));
    let l = iva_holding::libro(&v, 2026, date!(2026 - 11 - 17)).unwrap();
    assert_eq!(l.total_anual, -eur(210));
    assert_eq!(l.bienes.len(), 2);
    let bi1 = l.bienes.iter().find(|b| b.factura == "BI-1").unwrap();
    assert!(bi1.asset_id.is_some(), "la recibida enlaza su elemento");
    assert_eq!(bi1.prorrata_compra, Some(100));
    assert_eq!(bi1.anios[0].diferencia, Some(-50));
    let bi2 = l.bienes.iter().find(|b| b.factura == "BI-2").unwrap();
    assert!(bi2.avisos.iter().any(|a| a.contains("3.005,06")));
}

#[test]
fn regularizacion_por_venta_exenta_en_el_periodo_de_la_entrega() {
    let v = setup();
    receive_as(
        &v,
        "B12345674",
        "BI-1",
        (10_000, 2_100, 0),
        date!(2025 - 06 - 10),
        Nature::General,
        true,
    );
    con_prorrata(&v);
    let asset: i64 = v
        .connection()
        .query_row("SELECT asset_id FROM iva_bien_inversion", [], |r| r.get(0))
        .unwrap();
    taxpain_app::accounting::dispose_asset(
        &v,
        asset,
        date!(2026 - 03 - 10),
        eur(5_000),
        at(date!(2026 - 03 - 10)),
    )
    .unwrap();
    // Sin saber cómo tributó: se avisa y no se regulariza.
    let t1 = models::draft(&v, "303", 2026, 1).unwrap();
    assert_eq!(get(&t1, 43), Money::ZERO);
    assert!(
        t1.warnings.iter().any(|w| w.contains("art. 110")),
        "{:?}",
        t1.warnings
    );
    iva_holding::guardar_bien(
        &v,
        asset,
        &DatosBien {
            venta: Some(VentaIva::Exenta),
            ..Default::default()
        },
        at(date!(2026 - 03 - 11)),
    )
    .unwrap();
    // Quedan 2026 a 2029 (4 años): 2.100 × (0 − 100) % × 4 / 5 = −1.680 en el 1T.
    let t1 = models::draft(&v, "303", 2026, 1).unwrap();
    assert_eq!(get(&t1, 43), -eur(1_680));
    // Y ya no hay regularización anual en el 4T.
    let t4 = models::draft(&v, "303", 2026, 4).unwrap();
    assert_eq!(get(&t4, 43), Money::ZERO);
    let l = iva_holding::libro(&v, 2026, date!(2026 - 11 - 17)).unwrap();
    assert_eq!(l.total_ventas, -eur(1_680));
    let venta = l.bienes[0].venta.as_ref().unwrap();
    assert_eq!(
        (venta.anios_restantes, venta.regularizacion),
        (4, Some(-eur(1_680)))
    );
    // El 390 lleva la regularización a [63].
    let a = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&a, 63), -eur(1_680));
}

#[test]
fn venta_exenta_sin_prorrata_tambien_regulariza() {
    // Art. 110.Dos: aunque no se aplicara la prorrata. Deducido al 100 % en 2025, vendido exento
    // en 2027: quedan 2027 a 2029 → 2.100 × −100 % × 3 / 5 = −1.260.
    let v = setup();
    receive_as(
        &v,
        "B12345674",
        "BI-1",
        (10_000, 2_100, 0),
        date!(2025 - 06 - 10),
        Nature::General,
        true,
    );
    let asset: i64 = v
        .connection()
        .query_row("SELECT asset_id FROM iva_bien_inversion", [], |r| r.get(0))
        .unwrap();
    taxpain_app::accounting::dispose_asset(
        &v,
        asset,
        date!(2027 - 05 - 10),
        eur(5_000),
        at(date!(2027 - 05 - 10)),
    )
    .unwrap();
    iva_holding::guardar_bien(
        &v,
        asset,
        &DatosBien {
            venta: Some(VentaIva::Exenta),
            ..Default::default()
        },
        at(date!(2027 - 05 - 11)),
    )
    .unwrap();
    let t2 = models::draft(&v, "303", 2027, 2).unwrap();
    assert_eq!(get(&t2, 43), -eur(1_260));
}

#[test]
fn diagnostico_de_una_holding_pura() {
    let v = setup();
    receive(&v, "B12345674", "G-1", 1000, 210, 0, date!(2026 - 02 - 15));
    taxpain_app::vinculadas::save_participada(
        &v,
        &taxpain_app::vinculadas::Participada {
            id: 0,
            nif: "A22222228".into(),
            name: "Participada Demo SA".into(),
            province: "46".into(),
            country: "ES".into(),
            pct: 600_000,
            acquired: date!(2020 - 01 - 01),
            cost: eur(100_000),
            account: "2403".into(),
            grupo_fiscal: false,
            notes: String::new(),
            active: true,
        },
        at(date!(2026 - 01 - 02)),
    )
    .unwrap();
    let d = iva_holding::diagnostico(&v, 2026).unwrap();
    assert_eq!(d.perfil, iva_holding::Perfil::HoldingPura);
    assert!(
        d.avisos.iter().any(|a| a.contains("no se deduce")),
        "{:?}",
        d.avisos
    );
    assert!(d.notas.iter().any(|n| n.norma.contains("104.Tres.5.º")));
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
