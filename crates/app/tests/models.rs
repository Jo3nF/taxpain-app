//! Modelos desde los datos del almacén: facturas y contabilidad → 303, 130, 111 y 115.
//! Escenario sintético de un autónomo con las cuentas hechas a mano.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::record_entry;
use taxpain_app::invoicing::{self, InvoiceDraft, issue};
use taxpain_app::models::{self, ModelSettings};
use taxpain_app::received::{AssetClass, Nature, Reviewed, Source, ingest_file, review};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{
    InvoiceKind, InvoiceLine, Party, PartyId, PersonName, RoundingMode, VatTreatment,
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

fn scenario() -> Vault {
    let v = setup();
    // 1T: factura 1.000 + 21 % con retención 15 %; gasto 1.500 + 315.
    emit(&v, 1000, Some(Rate::percent(15)), date!(2026 - 02 - 10));
    receive(&v, "B12345674", "G-1", 1500, 315, 0, date!(2026 - 02 - 15));
    // 3T: factura 6.000 + 21 % sin retención; gasto 500 + 105; profesional 400 + 84 − 60;
    // alquiler 1.000 + 210 − 190 (19 %); nómina 2.000 con 300 de IRPF.
    emit(&v, 6000, None, date!(2026 - 08 - 01));
    receive(&v, "B12345674", "G-2", 500, 105, 0, date!(2026 - 08 - 05));
    receive(&v, "A22222228", "P-1", 400, 84, 60, date!(2026 - 08 - 06));
    receive(
        &v,
        "B12345674",
        "ALQ-9",
        1000,
        210,
        190,
        date!(2026 - 09 - 01),
    );
    record_entry(
        v.connection(),
        &JournalEntry {
            date: date!(2026 - 07 - 31),
            concept: "Nómina Ana".into(),
            origin: Origin::Nomina,
            lines: vec![
                EntryLine::debit(acc("640"), eur(2000)),
                EntryLine::credit(acc("4751"), eur(300)),
                EntryLine::credit(acc("465"), eur(1700)),
            ],
        },
        at(date!(2026 - 07 - 31)),
    )
    .unwrap();
    v
}

fn get(d: &models::Draft, b: u16) -> Money {
    d.boxes
        .iter()
        .find(|(k, _)| *k == b)
        .map_or(Money::ZERO, |(_, v)| *v)
}

#[test]
fn m303_con_compensacion_encadenada() {
    let v = scenario();
    // 1T: 210 − 315 = −105 → a compensar.
    let t1 = models::draft(&v, "303", 2026, 1).unwrap();
    assert_eq!(t1.result, -eur(105));
    assert_eq!(t1.declaration, "C");
    // 3T: 1.260 − (105 + 84 + 210) = 861; se compensan los 105 → 756.
    let t3 = models::draft(&v, "303", 2026, 3).unwrap();
    assert_eq!(get(&t3, 7), eur(6000));
    assert_eq!(get(&t3, 9), eur(1260));
    assert_eq!(get(&t3, 28), eur(1900));
    assert_eq!(get(&t3, 29), eur(399));
    assert_eq!(get(&t3, 46), eur(861));
    assert_eq!(get(&t3, 110), eur(105));
    assert_eq!(get(&t3, 78), eur(105));
    assert_eq!(t3.result, eur(756));
    assert_eq!(t3.declaration, "I");
    assert!(t3.file.starts_with("<T303020263T0000>"));
    assert_eq!(t3.file_name, "12345678Z_2026_3T_303.txt");
}

#[test]
fn m130_acumulado_desde_la_contabilidad() {
    let v = scenario();
    // Hasta el 30/9: ingresos 7.000; gastos 1.500 + 500 + 400 + 1.000 + 2.000 = 5.400, más el
    // 5 % de 1.600 de la simplificada (por defecto) = 80.
    let d = models::draft(&v, "130", 2026, 3).unwrap();
    assert_eq!(get(&d, 1), eur(7000));
    assert_eq!(get(&d, 2), eur(5480));
    assert_eq!(get(&d, 3), eur(1520));
    assert_eq!(get(&d, 4), eur(304));
    // El 1T salió negativo ([07] = −150): no cuenta en [05].
    assert_eq!(get(&d, 5), Money::ZERO);
    assert_eq!(get(&d, 6), eur(150));
    assert_eq!(get(&d, 7), eur(154));
    assert_eq!(d.result, eur(154));
    assert_eq!(d.declaration, "I");
    assert!(d.file.contains("ESPAÑOLA ESPAÑOLA"));
}

#[test]
fn m130_sin_rendimiento_del_año_anterior_avisa_de_la_casilla_13() {
    let v = scenario();
    // Sin contabilidad de 2025 ni dato tecleado: minoración cero y aviso.
    let d = models::draft(&v, "130", 2026, 3).unwrap();
    assert_eq!(get(&d, 13), Money::ZERO);
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("casilla [13]") && w.contains("indica 0")),
        "{:?}",
        d.warnings
    );
    // Sin actividad en 2025 (rendimiento 0): 100 € de minoración y sin aviso.
    models::save_settings(
        &v,
        &ModelSettings {
            previous_year_net: Some(Money::ZERO),
            ..ModelSettings::default()
        },
        datetime!(2026-10-01 9:00 UTC),
    )
    .unwrap();
    let d = models::draft(&v, "130", 2026, 3).unwrap();
    assert_eq!(get(&d, 12), eur(154));
    assert_eq!(get(&d, 13), eur(100));
    assert_eq!(get(&d, 14), eur(54));
    assert!(
        !d.warnings.iter().any(|w| w.contains("casilla [13]")),
        "{:?}",
        d.warnings
    );
}

#[test]
fn m200_avisa_de_los_dividendos_de_participadas_sin_la_exencion() {
    let v = setup();
    let mut s = invoicing::settings(&v).unwrap().unwrap();
    s.issuer.id = PartyId::Nif("B12345674".into());
    s.issuer.person = None;
    invoicing::save_settings(&v, &s, datetime!(2026-01-02 9:00 UTC)).unwrap();
    record_entry(
        v.connection(),
        &JournalEntry {
            date: date!(2026 - 06 - 15),
            concept: "Dividendo de Participada SL".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("572"), eur(20_000)),
                EntryLine::credit(acc("760"), eur(20_000)),
            ],
        },
        at(date!(2026 - 06 - 15)),
    )
    .unwrap();
    let d = models::draft(&v, "200", 2026, 0).unwrap();
    assert_eq!(get(&d, 500), eur(20_000));
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("art. 21 LIS") && w.contains("20.000")),
        "{:?}",
        d.warnings
    );
    // Con el 95 % tecleado en «Correcciones en disminución», sin aviso.
    models::save_settings(
        &v,
        &ModelSettings {
            corporate_decreases: eur(19_000),
            cnae: "6201".into(),
            ..ModelSettings::default()
        },
        datetime!(2026-09-01 9:00 UTC),
    )
    .unwrap();
    let d = models::draft(&v, "200", 2026, 0).unwrap();
    assert_eq!(get(&d, 418), eur(19_000));
    assert!(
        !d.warnings.iter().any(|w| w.contains("art. 21 LIS")),
        "{:?}",
        d.warnings
    );
}

#[test]
fn m190_avisa_de_la_subclave_cuando_el_profesional_retiene_al_7_o_al_1() {
    let v = scenario();
    // Profesional en inicio de actividad (7 %) y actividad en módulos (1 %): el 190 los lleva a
    // la G.01 y avisa de la subclave que les corresponde (G.03 y H.04).
    receive(&v, "B98765431", "P-7", 1000, 210, 70, date!(2026 - 09 - 10));
    receive(
        &v,
        "B76543214",
        "P-1b",
        2000,
        420,
        20,
        date!(2026 - 09 - 12),
    );
    let d = models::draft(&v, "190", 2026, 4).unwrap();
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("B98765431") && w.contains("7 %") && w.contains("G.03")),
        "{:?}",
        d.warnings
    );
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("B76543214") && w.contains("1 %") && w.contains("H.04")),
        "{:?}",
        d.warnings
    );
    // El profesional al 15 % del escenario no genera aviso.
    assert!(!d.warnings.iter().any(|w| w.contains("A22222228")));

    // Con la clave fijada en las condiciones del proveedor (G.03), el 190 la lleva en las
    // posiciones 78-80 y el aviso desaparece.
    use taxpain_app::cartera::{self, PartyTerms};
    let account = cartera::parties(&v)
        .unwrap()
        .into_iter()
        .find(|p| p.role == "proveedor" && p.tax_id == "B98765431")
        .unwrap()
        .account;
    let terms = |key: Option<&str>| PartyTerms {
        days: vec![30],
        pay_day: None,
        method: "transferencia".into(),
        treasury: None,
        iban: None,
        mandate_ref: None,
        mandate_date: None,
        key_190: key.map(Into::into),
    };
    let now = at(date!(2027 - 01 - 10));
    assert!(cartera::set_party_terms(&v, &account, Some(&terms(Some("G07"))), now).is_err());
    cartera::set_party_terms(&v, &account, Some(&terms(Some("g03"))), now).unwrap();
    let d = models::draft(&v, "190", 2026, 4).unwrap();
    assert!(
        !d.warnings.iter().any(|w| w.contains("B98765431")),
        "{:?}",
        d.warnings
    );
    let rec = d
        .file
        .split("\r\n")
        .find(|r| r.starts_with("2190") && &r[17..26] == "B98765431")
        .unwrap();
    assert_eq!(&rec[77..80], "G03", "{rec}");
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("B76543214") && w.contains("H.04"))
    );
}

#[test]
fn m390_resume_el_año_y_cuadra_con_los_303() {
    let v = scenario();
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    // Devengado: 1.000 + 6.000 al 21 % → 7.000 / 1.470; deducible 3.400 / 714.
    assert_eq!(get(&d, 5), eur(7000));
    assert_eq!(get(&d, 6), eur(1470));
    assert_eq!(get(&d, 605), eur(3400));
    assert_eq!(get(&d, 606), eur(714));
    assert_eq!(get(&d, 65), eur(756));
    assert_eq!(get(&d, 95), eur(756));
    assert_eq!(d.result, eur(756));
    // El fichero sigue el diseño de 2026, el suyo: sin avisos.
    assert!(d.warnings.is_empty(), "{:?}", d.warnings);
    assert!(d.file.starts_with("<T390020260A0000>"));
    let s = taxpain_models::m390::spec_for(2026);
    assert_eq!(s.source, "dr390e2026.xlsx");
    assert!(taxpain_models::dr::pages_fit(s, &d.file));
    // Dato estadístico D: el cliente B12345674 supera los 3.005,06 € del 347, así que el campo
    // 44 de la página 1 («operaciones con terceras personas»; el 43 hasta 2025) va a «1». El 22,
    // nuevo en 2026, es el pago a cuenta de carburantes: «2» (no).
    let p1 = s.page("Pág. 1").unwrap();
    let start = d.file.find("<T39001000>").unwrap();
    let rec: String = d.file[start..].chars().take(p1.len()).collect();
    let r = taxpain_models::dr::PageReader::new(p1, &rec).unwrap();
    assert_eq!(r.raw(44).ok().as_deref(), Some("1"));
    assert_eq!(r.raw(22).ok().as_deref(), Some("2"));
    // El de 2025 (sin operaciones), con su diseño y la marca del 347 en el campo 43.
    let d = models::draft(&v, "390", 2025, 0).unwrap();
    assert!(
        d.warnings.iter().all(|w| !w.contains("diseño")),
        "{:?}",
        d.warnings
    );
    let s = taxpain_models::m390::spec_for(2025);
    assert_eq!(s.source, "dr390e2025.xlsx");
    assert!(taxpain_models::dr::pages_fit(s, &d.file));
    let p1 = s.page("Pág. 1").unwrap();
    let start = d.file.find("<T39001000>").unwrap();
    let rec: String = d.file[start..].chars().take(p1.len()).collect();
    let r = taxpain_models::dr::PageReader::new(p1, &rec).unwrap();
    assert_eq!(r.raw(43).ok().as_deref(), Some("0"));
    assert_eq!(r.raw(22).ok().as_deref(), Some("0"), "sustitutiva");
}

#[test]
fn aviso_del_diseno_con_el_que_se_genera() {
    let w = |m: &str, y: i32, p: &str| models::design_warning(m, y, p);
    // El 390 se genera con el diseño de 2025 hasta 2025 y con el de 2026 («Ejercicio 2026 y
    // siguientes») desde 2026: sin aviso en 2025 ni en 2026; en 2027 sale con el de 2026 y dice
    // que el de 2027 aún no está publicado.
    assert_eq!(w("390", 2025, "0A"), None);
    assert_eq!(w("390", 2026, "0A"), None);
    let w27 = w("390", 2027, "0A").unwrap_or_default();
    assert!(
        w27.starts_with(
            "Diseño de registro del 390 de 2027 no publicado todavía: se usa el de dr390e2026"
        ),
        "{w27}"
    );
    // 2024 tiene su propio diseño y el fichero sale con el de 2025.
    assert_eq!(
        w("390", 2024, "0A").as_deref(),
        Some(
            "El fichero sigue el diseño de registro dr390e2025; el de 2024 0A es otro \
             (dr390e2024): no lo presentes con este fichero (las casillas del borrador sí valen)."
        )
    );
    // Antes del primero que tiene TaxPain.
    let old = w("390", 2016, "0A").unwrap_or_default();
    assert!(
        old.starts_with(
            "El fichero sigue el diseño de registro dr390e2025; TaxPain no tiene el de 2016 0A:"
        ),
        "{old}"
    );
    // El 200 de 2025 se genera con el suyo (antes decía que era otro que él mismo); el de 2026
    // aún no está publicado.
    assert_eq!(w("200", 2025, "0A"), None);
    let next = w("200", 2026, "0A").unwrap_or_default();
    assert!(
        next.starts_with(
            "Diseño de registro del 200 de 2026 no publicado todavía: se usa el de DR200e25 \
             (el del ejercicio 2025)."
        ),
        "{next}"
    );
    // El 303 de un ejercicio anterior sale con el vigente.
    let q = w("303", 2025, "3T").unwrap_or_default();
    assert!(
        q.starts_with(
            "El fichero sigue el diseño de registro DR303e26v101; el de 2025 3T es otro \
             (DR303e25):"
        ),
        "{q}"
    );
    assert_eq!(w("303", 2026, "3T"), None);
    // Las informativas: con el de 2025 y aviso desde 2026.
    assert_eq!(w("347", 2025, "0A"), None);
    assert!(w("347", 2026, "0A").is_some_and(|x| x.contains("se usa el de DR347 de 2025")));
}

/// Factura de una línea (la venta de un elemento del inmovilizado) a Cliente SL.
fn sale_draft(what: &str, base: i64, vat: VatTreatment, d: Date) -> InvoiceDraft {
    InvoiceDraft {
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
            description: what.into(),
            quantity_milli: 1000,
            unit_price: eur(base),
            discount: Rate::ZERO,
            vat,
            surcharge: None,
            withholding: None,
            advance: false,
        }],
        rectified: vec![],
        rectified_amounts: None,
        description: what.into(),
        notes: None,
        currency: None,
        oss: None,
    }
}

fn emit_sale(v: &Vault, what: &str, base: i64, vat: VatTreatment, d: Date) {
    issue(v, &sale_draft(what, base, vat, d), at(d), true).unwrap();
}

/// Rectificativa por diferencias de la factura `number` (expedida el `issued`) con una línea de
/// base `base`.
fn rectify_sale(
    v: &Vault,
    (number, issued): (&str, Date),
    what: &str,
    base: i64,
    d: Date,
) -> invoicing::IssuedInvoice {
    use taxpain_core::invoice::{Rectification, RectificationMethod};
    let mut r = sale_draft(what, base, iva21(), d);
    r.kind = InvoiceKind::Rectificativa(Rectification {
        code: 1,
        method: RectificationMethod::Diferencias,
    });
    r.rectified = vec![(number.into(), issued)];
    issue(v, &r, at(d), true).unwrap()
}

fn iva21() -> VatTreatment {
    VatTreatment::Taxed {
        rate: Rate::percent(21),
    }
}

/// Alta de un elemento del inmovilizado comprado en 2023 por `cost` y su venta por `price` el
/// día `d`.
fn sell_asset(v: &Vault, name: &str, account: &str, cost: i64, price: i64, d: Date) {
    use taxpain_app::accounting::{add_fixed_asset, dispose_asset};
    use taxpain_core::accounting::depreciation::FixedAsset;
    let a = FixedAsset {
        id: 0,
        name: name.into(),
        account: acc(account),
        depreciation_account: acc(&format!("28{}", &account[1..])),
        expense_account: acc("681"),
        acquired_on: date!(2023 - 01 - 10),
        cost: eur(cost),
        residual: Money::ZERO,
        rate: Rate::percent(10),
        disposed_on: None,
        method: Default::default(),
        tax_rate: None,
    };
    let id = add_fixed_asset(v, &a, at(d)).unwrap();
    dispose_asset(v, id, d, eur(price), at(d)).unwrap();
}

/// Las ventas de inmovilizado del año (art. 108 LIVA) no cuentan en el volumen de operaciones
/// del 390 (art. 121.Tres): están en su casilla de operaciones por su factura y se restan en
/// [106] (inmuebles) y [107] (los demás bienes de inversión, desde 3.005,06 € de coste).
#[test]
fn ventas_de_inmovilizado_fuera_del_volumen_del_390() {
    use taxpain_core::invoice::Exemption;
    let v = setup();
    let now = datetime!(2026-12-30 10:00 UTC);
    let sell = |name: &str, account: &str, cost: i64, price: i64, d: Date| {
        sell_asset(&v, name, account, cost, price, d);
    };
    let iva21 = iva21();
    // Una factura de 4.000 de servicios en marzo (A2026-0001).
    emit(&v, 4000, None, date!(2026 - 03 - 02));
    // Torno (maquinaria, 12.000): bien de inversión, vendido por 3.000 + IVA → [107].
    sell("Torno", "213", 12_000, 3000, date!(2026 - 05 - 10));
    emit_sale(&v, "Venta del torno", 3000, iva21, date!(2026 - 05 - 12));
    // Silla (2.000 de coste, menos de 3.005,06 €) y programa (intangible): no son bienes de
    // inversión y siguen en el volumen.
    sell("Silla", "216", 2000, 500, date!(2026 - 06 - 01));
    emit_sale(&v, "Venta de la silla", 500, iva21, date!(2026 - 06 - 01));
    sell("Programa", "206", 6000, 2500, date!(2026 - 07 - 01));
    emit_sale(&v, "Venta del programa", 2500, iva21, date!(2026 - 07 - 01));
    // Local (construcción): segunda entrega exenta por 60.000 → [105] y [106].
    sell("Local", "211", 80_000, 60_000, date!(2026 - 09 - 15));
    emit_sale(
        &v,
        "Venta del local",
        60_000,
        VatTreatment::Exempt(Exemption::E1),
        date!(2026 - 09 - 15),
    );
    // Furgoneta vendida por 4.000 sin factura: no está en el volumen y no se resta, aunque la
    // de servicios de marzo sea de 4.000 (a más de 45 días).
    sell("Furgoneta", "218", 9000, 4000, date!(2026 - 11 - 20));

    let d = models::draft(&v, "390", 2026, 0).unwrap();
    // Régimen general: 4.000 de servicios, 3.000 del torno, 500 de la silla y 2.500 del
    // programa. Exentas sin derecho a deducir: el local.
    assert_eq!(get(&d, 99), eur(10_000));
    assert_eq!(get(&d, 105), eur(60_000));
    assert_eq!(get(&d, 106), eur(60_000));
    assert_eq!(get(&d, 107), eur(3000));
    assert_eq!(get(&d, 108), eur(10_000 + 60_000 - 60_000 - 3000));
    assert_eq!(
        d.details,
        vec![
            (
                "[107] Venta de Torno (213) del 10/05/2026 · factura A2026-0002".to_string(),
                eur(3000)
            ),
            (
                "[106] Venta de Local (211) del 15/09/2026 · factura A2026-0005".to_string(),
                eur(60_000)
            ),
        ]
    );
    let van: Vec<_> = d
        .warnings
        .iter()
        .filter(|w| w.starts_with("Venta de Furgoneta"))
        .collect();
    assert_eq!(van.len(), 1, "{:?}", d.warnings);
    // La factura puede devengar en el año siguiente: la ventana pasa del 31 de diciembre.
    assert!(
        van[0].contains("del 20/11/2026 por 4.000,00 €")
            && van[0].contains("del 06/10/2026 al 04/01/2027")
            && van[0].contains("volumen de operaciones del 390")
            && van[0].contains("no se resta en [107]"),
        "{}",
        van[0]
    );
    // En el fichero: [106], [107] y [108] en las posiciones 650, 667 y 684 de la página 6.
    let p6: String = d.file[d.file.find("<T39006000>").unwrap()..]
        .chars()
        .take(862)
        .collect();
    assert_eq!(&p6[649..666], "00000000006000000");
    assert_eq!(&p6[666..683], "00000000000300000");
    assert_eq!(&p6[683..700], "00000000000700000");

    // Exonerado del 390: la página 04 del 303 del 4T lleva lo mismo en [79] y [99], y el aviso.
    models::save_settings(
        &v,
        &ModelSettings {
            exempt_from_390: true,
            ..ModelSettings::default()
        },
        now,
    )
    .unwrap();
    let q4 = models::draft(&v, "303", 2026, 4).unwrap();
    assert_eq!(get(&q4, 79), eur(60_000));
    assert_eq!(get(&q4, 99), eur(3000));
    assert_eq!(get(&q4, 88), eur(7000));
    // El aviso habla de las casillas de la página 04 ([99] son los bienes de inversión; [107],
    // el porcentaje del territorio común) y no del 390, que el exonerado no presenta.
    let van: Vec<_> = q4
        .warnings
        .iter()
        .filter(|w| w.starts_with("Venta de Furgoneta"))
        .collect();
    assert_eq!(van.len(), 1, "{:?}", q4.warnings);
    assert!(
        van[0].contains("no se resta en [99]")
            && van[0].contains("volumen de operaciones de la página 04")
            && !van[0].contains("[107]")
            && !van[0].contains("390"),
        "{}",
        van[0]
    );
}

/// La venta se resta en el 390 del año en que devenga su factura, que es el del volumen donde
/// está su base (art. 121.Dos LIVA), aunque la baja sea del año anterior o del siguiente.
#[test]
fn venta_de_inmovilizado_facturada_en_otro_año() {
    use taxpain_core::invoice::{Exemption, IdType};
    let v = setup();
    // Torno entregado el 20/12/2026 a una empresa francesa y facturado el 11/01/2027 con esa
    // fecha de operación: la entrega intracomunitaria devenga al expedir la factura, antes del
    // día 15 de enero (art. 75.Uno.8.º LIVA), así que su base está en el [103] de 2027.
    sell_asset(&v, "Torno", "213", 12_000, 3000, date!(2026 - 12 - 20));
    // Prensa facturada el 28/12/2026 (A2026-0001) y dada de baja el 05/01/2027: devenga en
    // 2026.
    emit_sale(
        &v,
        "Venta de la prensa",
        2000,
        iva21(),
        date!(2026 - 12 - 28),
    );
    // Fresadora entregada el 28/12/2026 y facturada el 08/01/2027 sin fecha de operación
    // (A2027-0001): devenga en 2027.
    sell_asset(&v, "Fresadora", "213", 9000, 8000, date!(2026 - 12 - 28));
    emit_sale(
        &v,
        "Venta de la fresadora",
        8000,
        iva21(),
        date!(2027 - 01 - 08),
    );
    let mut torno = sale_draft(
        "Venta del torno",
        3000,
        VatTreatment::Exempt(Exemption::E5),
        date!(2027 - 01 - 11),
    );
    torno.operation_date = Some(date!(2026 - 12 - 20));
    torno.recipient = Some(Party {
        id: PartyId::Foreign {
            country: "FR".into(),
            id_type: IdType::NifIva,
            id: "12345678901".into(),
        },
        name: "Client SARL".into(),
        address: addr(),
        person: None,
    });
    issue(&v, &torno, at(date!(2027 - 01 - 11)), true).unwrap();
    sell_asset(&v, "Prensa", "213", 5000, 2000, date!(2027 - 01 - 05));

    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), eur(2000));
    assert_eq!(get(&d, 103), Money::ZERO);
    assert_eq!(get(&d, 107), eur(2000));
    assert_eq!(get(&d, 108), Money::ZERO);
    assert_eq!(
        d.details,
        vec![(
            "[107] Venta de Prensa (213) del 05/01/2027 · factura A2026-0001".to_string(),
            eur(2000)
        )]
    );
    // El torno y la fresadora no piden su factura: se restan en el 390 de 2027, y se dice.
    assert!(
        !d.warnings.iter().any(|w| w.contains("emítela")),
        "{:?}",
        d.warnings
    );
    let find = |d: &models::Draft, what: &str| {
        d.warnings
            .iter()
            .find(|w| w.starts_with(what))
            .cloned()
            .unwrap_or_else(|| panic!("sin aviso de {what}: {:?}", d.warnings))
    };
    let torno = find(&d, "Venta de Torno");
    assert!(
        torno.contains("A2027-0002") && torno.contains("devenga en 2027"),
        "{torno}"
    );
    assert!(!torno.contains("fecha de la operación"), "{torno}");
    // La fresadora es nacional y su factura no lleva la fecha de la entrega: se recuerda.
    let fresadora = find(&d, "Venta de Fresadora");
    assert!(
        fresadora.contains("A2027-0001") && fresadora.contains("fecha de la operación"),
        "{fresadora}"
    );

    let d = models::draft(&v, "390", 2027, 0).unwrap();
    assert_eq!(get(&d, 99), eur(8000));
    assert_eq!(get(&d, 103), eur(3000));
    assert_eq!(get(&d, 107), eur(11_000));
    assert_eq!(get(&d, 108), Money::ZERO);
    assert_eq!(
        d.details,
        vec![
            (
                "[107] Venta de Torno (213) del 20/12/2026 · factura A2027-0002".to_string(),
                eur(3000)
            ),
            (
                "[107] Venta de Fresadora (213) del 28/12/2026 · factura A2027-0001".to_string(),
                eur(8000)
            ),
        ]
    );
    // La prensa ya se restó en 2026.
    let prensa = find(&d, "Venta de Prensa");
    assert!(
        prensa.contains("A2026-0001") && prensa.contains("devenga en 2026"),
        "{prensa}"
    );
    assert!(
        !d.warnings.iter().any(|w| w.contains("emítela")),
        "{:?}",
        d.warnings
    );
}

/// Una rectificativa de la venta deja en el volumen sólo lo que queda de ella, y eso es lo que
/// se resta; si es del año siguiente, la rectificación está en el volumen de ese año y allí se
/// compensa.
#[test]
fn venta_de_inmovilizado_rectificada() {
    let v = setup();
    // Torno vendido el 10/05/2026 (A2026-0001) y devuelto: R2026-0001 lo anula el 01/06.
    sell_asset(&v, "Torno", "213", 12_000, 3000, date!(2026 - 05 - 10));
    emit_sale(&v, "Venta del torno", 3000, iva21(), date!(2026 - 05 - 10));
    rectify_sale(
        &v,
        ("A2026-0001", date!(2026 - 05 - 10)),
        "Devolución del torno",
        -3000,
        date!(2026 - 06 - 01),
    );
    // Fresadora vendida por 5.000 (A2026-0002) y rebajada 1.000 (R2026-0002).
    sell_asset(&v, "Fresadora", "213", 6000, 5000, date!(2026 - 07 - 10));
    emit_sale(
        &v,
        "Venta de la fresadora",
        5000,
        iva21(),
        date!(2026 - 07 - 10),
    );
    rectify_sale(
        &v,
        ("A2026-0002", date!(2026 - 07 - 10)),
        "Descuento en la fresadora",
        -1000,
        date!(2026 - 07 - 20),
    );
    // Prensa vendida por 2.000 (A2026-0003) y devuelta en 2027 (R2027-0001).
    sell_asset(&v, "Prensa", "213", 5000, 2000, date!(2026 - 10 - 10));
    emit_sale(
        &v,
        "Venta de la prensa",
        2000,
        iva21(),
        date!(2026 - 10 - 10),
    );
    rectify_sale(
        &v,
        ("A2026-0003", date!(2026 - 10 - 10)),
        "Devolución de la prensa",
        -2000,
        date!(2027 - 02 - 15),
    );

    let d = models::draft(&v, "390", 2026, 0).unwrap();
    // [99]: 3.000 − 3.000 + 5.000 − 1.000 + 2.000; se restan 0 + 4.000 + 2.000.
    assert_eq!(get(&d, 99), eur(6000));
    assert_eq!(get(&d, 107), eur(6000));
    assert_eq!(get(&d, 108), Money::ZERO);
    assert_eq!(
        d.details,
        vec![
            (
                "[107] Venta de Fresadora (213) del 10/07/2026 · factura A2026-0002".to_string(),
                eur(4000)
            ),
            (
                "[107] Venta de Prensa (213) del 10/10/2026 · factura A2026-0003".to_string(),
                eur(2000)
            ),
        ]
    );
    let find = |d: &models::Draft, what: &str| {
        d.warnings
            .iter()
            .find(|w| w.starts_with(what))
            .cloned()
            .unwrap_or_else(|| panic!("sin aviso de {what}: {:?}", d.warnings))
    };
    let torno = find(&d, "Venta de Torno");
    assert!(
        torno.contains("R2026-0001") && torno.contains("0,00 €"),
        "{torno}"
    );
    let fresadora = find(&d, "Venta de Fresadora");
    assert!(
        fresadora.contains("R2026-0002") && fresadora.contains("4.000,00 €"),
        "{fresadora}"
    );
    // La devolución de 2027 no cambia el volumen de 2026.
    assert!(
        !d.warnings.iter().any(|w| w.starts_with("Venta de Prensa")),
        "{:?}",
        d.warnings
    );

    // 2027: la rectificativa resta 2.000 de [99] y se compensa en [107].
    let d = models::draft(&v, "390", 2027, 0).unwrap();
    assert_eq!(get(&d, 99), eur(-2000));
    assert_eq!(get(&d, 107), eur(-2000));
    assert_eq!(get(&d, 108), Money::ZERO);
    assert_eq!(
        d.details,
        vec![(
            "[107] Venta de Prensa (213) del 10/10/2026 · rectificativa R2027-0001".to_string(),
            eur(-2000)
        )]
    );
    let prensa = find(&d, "Venta de Prensa");
    assert!(
        prensa.contains("A2026-0003") && prensa.contains("R2027-0001"),
        "{prensa}"
    );
    // En el fichero, con el signo: [107] en la posición 667 de la página 6.
    let p6: String = d.file[d.file.find("<T39006000>").unwrap()..]
        .chars()
        .take(862)
        .collect();
    assert_eq!(&p6[666..683], "N0000000000200000");

    // Exonerado del 390: la página 04 del 303 del 4T de 2026 tampoco resta el torno devuelto, y
    // su volumen [88] no queda en negativo.
    models::save_settings(
        &v,
        &ModelSettings {
            exempt_from_390: true,
            ..ModelSettings::default()
        },
        datetime!(2027-02-20 10:00 UTC),
    )
    .unwrap();
    let q4 = models::draft(&v, "303", 2026, 4).unwrap();
    assert_eq!(get(&q4, 80), eur(6000));
    assert_eq!(get(&q4, 99), eur(6000));
    assert_eq!(get(&q4, 88), Money::ZERO);
    let torno = find(&q4, "Venta de Torno");
    assert!(
        torno.contains("se resta en [99]") && !torno.contains("[107]"),
        "{torno}"
    );
}

/// Si ninguna línea nombra el elemento, se toma la de su base más cercana y se avisa; la que lo
/// nombra manda sobre otra más cercana, y no se la queda una venta anterior que no nombra.
#[test]
fn venta_de_inmovilizado_con_la_base_de_otra_factura() {
    let v = setup();
    // Furgoneta vendida el 10/05/2026 por 1.500 sin factura y una de servicios de 1.500 el 20/05
    // (A2026-0001).
    sell_asset(&v, "Furgoneta", "218", 9000, 1500, date!(2026 - 05 - 10));
    emit_sale(
        &v,
        "Consultoría de mayo",
        1500,
        iva21(),
        date!(2026 - 05 - 20),
    );
    // Torno vendido el 10/06/2026 por 3.000: una de servicios de 3.000 al día siguiente
    // (A2026-0002) y la del torno el 20/06 (A2026-0003).
    sell_asset(&v, "Torno", "213", 12_000, 3000, date!(2026 - 06 - 10));
    emit_sale(&v, "Mantenimiento", 3000, iva21(), date!(2026 - 06 - 11));
    emit_sale(
        &v,
        "Venta del TORNO usado",
        3000,
        iva21(),
        date!(2026 - 06 - 20),
    );
    // Remolque vendido el 01/09/2026 por 2.000 sin factura y carretilla vendida por lo mismo el
    // 05/09 con la suya (A2026-0004, «carretilla elevadora»): la línea es de la carretilla
    // aunque la baja del remolque vaya antes.
    sell_asset(&v, "Remolque", "218", 5000, 2000, date!(2026 - 09 - 01));
    sell_asset(
        &v,
        "Carretilla elevadora",
        "213",
        6000,
        2000,
        date!(2026 - 09 - 05),
    );
    emit_sale(
        &v,
        "Venta de la carretilla",
        2000,
        iva21(),
        date!(2026 - 09 - 06),
    );

    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), eur(9500));
    assert_eq!(get(&d, 107), eur(6500));
    assert_eq!(get(&d, 108), eur(3000));
    assert_eq!(
        d.details,
        vec![
            (
                "[107] Venta de Furgoneta (218) del 10/05/2026 · factura A2026-0001".to_string(),
                eur(1500)
            ),
            (
                "[107] Venta de Torno (213) del 10/06/2026 · factura A2026-0003".to_string(),
                eur(3000)
            ),
            (
                "[107] Venta de Carretilla elevadora (213) del 05/09/2026 · factura A2026-0004"
                    .to_string(),
                eur(2000)
            ),
        ]
    );
    let trailer: Vec<_> = d
        .warnings
        .iter()
        .filter(|w| w.starts_with("Venta de Remolque"))
        .collect();
    assert_eq!(trailer.len(), 1, "{:?}", d.warnings);
    assert!(
        trailer[0].contains("A2026-0004")
            && trailer[0].contains("ya es la venta de Carretilla elevadora")
            && trailer[0].contains("emite la de esta"),
        "{}",
        trailer[0]
    );
    assert!(
        !d.warnings
            .iter()
            .any(|w| w.starts_with("Venta de Carretilla")),
        "{:?}",
        d.warnings
    );
    let van: Vec<_> = d
        .warnings
        .iter()
        .filter(|w| w.starts_with("Venta de Furgoneta"))
        .collect();
    assert_eq!(van.len(), 1, "{:?}", d.warnings);
    assert!(
        van[0].contains("«Consultoría de mayo»")
            && van[0].contains("A2026-0001")
            && van[0].contains("comprueba"),
        "{}",
        van[0]
    );
    // Del torno no hay aviso propio; sólo sale, con las demás, en el de vincular las facturas.
    assert!(
        !d.warnings.iter().any(|w| w.starts_with("Venta de Torno")),
        "{:?}",
        d.warnings
    );
    let sin_vincular: Vec<_> = d
        .warnings
        .iter()
        .filter(|w| w.starts_with("Ventas del inmovilizado sin vincular"))
        .collect();
    assert!(
        sin_vincular.len() == 1
            && sin_vincular[0].contains("Furgoneta (A2026-0001)")
            && sin_vincular[0].contains("Torno (A2026-0003)")
            && sin_vincular[0].contains("Carretilla elevadora (A2026-0004)")
            && !sin_vincular[0].contains("Remolque"),
        "{:?}",
        d.warnings
    );
}

/// Una venta facturada como no sujeta no se resta ni pide factura; si la única línea con su
/// base no vale (un anticipo, una rectificativa o la de otra venta), el aviso dice por qué.
#[test]
fn venta_de_inmovilizado_no_sujeta_o_sin_su_línea() {
    use taxpain_core::invoice::NotSubject;
    let v = setup();
    // Prensa transmitida con una rama de actividad (art. 7.1.º LIVA) el 15/06/2026: no sujeta,
    // fuera del volumen (A2026-0001).
    sell_asset(&v, "Prensa", "213", 10_000, 6000, date!(2026 - 06 - 15));
    emit_sale(
        &v,
        "Prensa (rama de actividad)",
        6000,
        VatTreatment::NotSubject(NotSubject::N1),
        date!(2026 - 06 - 15),
    );
    // Grúa en Portugal vendida allí el 01/08/2026: no sujeta por localización, en [110], y [107]
    // sólo recoge entregas sujetas (A2026-0002).
    sell_asset(&v, "Grúa", "213", 20_000, 7000, date!(2026 - 08 - 01));
    emit_sale(
        &v,
        "Venta de la grúa",
        7000,
        VatTreatment::NotSubject(NotSubject::N2),
        date!(2026 - 08 - 01),
    );
    // Fresadora vendida el 01/09/2026 por 4.000 con sólo su anticipo facturado el 20/08
    // (A2026-0003).
    let mut anticipo = sale_draft(
        "Anticipo de la fresadora",
        4000,
        iva21(),
        date!(2026 - 08 - 20),
    );
    anticipo.lines[0].advance = true;
    issue(&v, &anticipo, at(date!(2026 - 08 - 20)), true).unwrap();
    sell_asset(&v, "Fresadora", "213", 8000, 4000, date!(2026 - 09 - 01));
    // Dos taladros vendidos el 10/10/2026 por 3.500 cada uno con una sola línea (A2026-0004).
    sell_asset(&v, "Taladro A", "213", 4000, 3500, date!(2026 - 10 - 10));
    sell_asset(&v, "Taladro B", "213", 4000, 3500, date!(2026 - 10 - 10));
    emit_sale(&v, "Venta de taladro", 3500, iva21(), date!(2026 - 10 - 10));
    // Cizalla vendida el 10/11/2026 por 1.800: la única línea con esa base es la de la
    // rectificativa R2026-0001, que sube 1.800 una reparación de 500 (A2026-0005).
    emit_sale(&v, "Reparación", 500, iva21(), date!(2026 - 11 - 02));
    rectify_sale(
        &v,
        ("A2026-0005", date!(2026 - 11 - 02)),
        "Reparación (más horas)",
        1800,
        date!(2026 - 11 - 12),
    );
    sell_asset(&v, "Cizalla", "213", 6000, 1800, date!(2026 - 11 - 10));

    let d = models::draft(&v, "390", 2026, 0).unwrap();
    // [99]: el anticipo, un taladro y la reparación con su rectificativa; [110]: la grúa. Sólo
    // se resta el taladro de la línea.
    assert_eq!(get(&d, 99), eur(4000 + 3500 + 500 + 1800));
    assert_eq!(get(&d, 110), eur(7000));
    assert_eq!(get(&d, 107), eur(3500));
    assert_eq!(get(&d, 108), eur(9800 + 7000 - 3500));
    assert_eq!(
        d.details,
        vec![(
            "[107] Venta de Taladro A (213) del 10/10/2026 · factura A2026-0004".to_string(),
            eur(3500)
        )]
    );
    // Ni la prensa ni la grúa piden factura.
    assert!(
        !d.warnings
            .iter()
            .any(|w| w.starts_with("Venta de Prensa") || w.starts_with("Venta de Grúa")),
        "{:?}",
        d.warnings
    );
    let find = |what: &str| {
        d.warnings
            .iter()
            .find(|w| w.starts_with(what))
            .cloned()
            .unwrap_or_else(|| panic!("sin aviso de {what}: {:?}", d.warnings))
    };
    let fresadora = find("Venta de Fresadora");
    assert!(
        fresadora.contains("anticipo") && fresadora.contains("A2026-0003"),
        "{fresadora}"
    );
    assert!(!fresadora.contains("ninguna factura"), "{fresadora}");
    let taladro = find("Venta de Taladro B");
    assert!(
        taladro.contains("A2026-0004") && taladro.contains("Taladro A"),
        "{taladro}"
    );
    assert!(!taladro.contains("ninguna factura"), "{taladro}");
    let cizalla = find("Venta de Cizalla");
    assert!(
        cizalla.contains("rectificativa R2026-0001") && !cizalla.contains("ninguna factura"),
        "{cizalla}"
    );
}

#[test]
fn exonerado_del_390_presenta_la_pagina_04_en_el_4t() {
    let v = scenario();
    models::save_settings(
        &v,
        &ModelSettings {
            exempt_from_390: true,
            ..ModelSettings::default()
        },
        datetime!(2026-10-01 9:00 UTC),
    )
    .unwrap();
    // Sin actividad en los ajustes: la página sale, pero avisa.
    let d = models::draft(&v, "303", 2026, 4).unwrap();
    assert!(d.file.contains("<T30304000>"));
    assert!(
        d.warnings.iter().any(|w| w.contains("actividad principal")),
        "{:?}",
        d.warnings
    );
    // Fuera del 4T no hay página 04.
    let t3 = models::draft(&v, "303", 2026, 3).unwrap();
    assert!(!t3.file.contains("<T30304000>"));

    let mut inv = invoicing::settings(&v).unwrap().unwrap();
    inv.activity = Some(taxpain_invoicing::books::Activity {
        code: "A".into(),
        kind: "05".into(),
        iae: "0742".into(),
    });
    invoicing::save_settings(&v, &inv, datetime!(2026-10-01 9:00 UTC)).unwrap();
    let d = models::draft(&v, "303", 2026, 4).unwrap();
    assert!(!d.warnings.iter().any(|w| w.contains("actividad principal")));
    // Volumen del año: 7.000 en régimen general, igual que el [108] del 390.
    let d390 = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 80), eur(7000));
    assert_eq!(get(&d, 88), get(&d390, 108));
    let spec = taxpain_models::m303::spec();
    let p4 = spec.page("04").unwrap();
    let start = d.file.find("<T30304000>").unwrap();
    let rec: String = d.file[start..].chars().take(p4.len()).collect();
    let r = taxpain_models::dr::PageReader::new(p4, &rec).unwrap();
    assert_eq!(r.raw(6).ok().as_deref(), Some("A05"));
    assert_eq!(r.raw(7).ok().as_deref(), Some("0742"));
    // El cliente B12345674 supera el umbral del 347.
    assert_eq!(r.raw(18).ok().as_deref(), Some("X"));
    assert_eq!(r.amount_box(80).ok().flatten(), Some(eur(7000)));
    assert_eq!(r.amount_box(88).ok().flatten(), Some(eur(7000)));
}

#[test]
fn informativas_anuales() {
    let v = scenario();
    // 347: el cliente B12345674 sólo por la factura sin retención (7.260 IVA incluido; la de
    // 1.210 con retención ya la declara el cliente en su 190) y el proveedor B12345674 sin
    // retención (1.815 + 605 = 2.420, por debajo del umbral).
    let d = models::draft(&v, "347", 2026, 0).unwrap();
    assert_eq!(d.details.len(), 1, "{:?}", d.details);
    assert_eq!(d.details[0].1, eur(7260));
    assert!(d.details[0].0.contains("ventas"));
    assert_eq!(d.file.split("\r\n").count(), 2);
    // 190: el profesional con retención (60 €); 180: el alquiler (190 €) con aviso de inmueble.
    let d190 = models::draft(&v, "190", 2026, 0).unwrap();
    assert_eq!(d190.result, eur(60));
    assert!(
        d190.warnings
            .iter()
            .any(|w| w.contains("nómina hechos a mano"))
    );
    let d180 = models::draft(&v, "180", 2026, 0).unwrap();
    assert_eq!(d180.result, eur(190));
    assert!(d180.warnings.iter().any(|w| w.contains("catastral")));
}

/// Lo cobrado en metálico (Cobros y pagos con forma «efectivo») de más de 6.000 € por cliente
/// va en el 347 como importe percibido en metálico (pos. 101-115) con el ejercicio (art. 34.1.h
/// RGAT).
#[test]
fn m347_declara_lo_cobrado_en_metalico_de_mas_de_6000() {
    use taxpain_app::cartera;
    let v = setup();
    // 7.000 + 21 % = 8.470, cobrados en caja.
    emit(&v, 7000, None, date!(2026 - 03 - 10));
    let today = date!(2026 - 12 - 01);
    let sale = cartera::list(&v, today)
        .unwrap()
        .into_iter()
        .find(|d| d.kind == "emitida")
        .unwrap()
        .invoice_id;
    let settle = |method: Option<&str>| {
        cartera::settle_with(
            &v,
            "emitida",
            sale,
            1,
            date!(2026 - 04 - 02),
            "570",
            method,
            None,
            at(today),
        )
    };
    assert!(settle(Some("bitcoin")).is_err(), "forma no válida");
    settle(Some("efectivo")).unwrap();
    let d = models::draft(&v, "347", 2026, 0).unwrap();
    let rec = d
        .file
        .split("\r\n")
        .find(|r| r.contains("B12345674"))
        .unwrap();
    assert_eq!(&rec[100..115], "000000000847000", "{rec}");
    assert_eq!(&rec[131..135], "2026");
    // Deshecho el cobro y registrado por banco, el metálico desaparece.
    cartera::unsettle(&v, "emitida", sale, 1, at(today), true).unwrap();
    settle(Some("transferencia")).unwrap();
    let d = models::draft(&v, "347", 2026, 0).unwrap();
    let rec = d
        .file
        .split("\r\n")
        .find(|r| r.contains("B12345674"))
        .unwrap();
    assert_eq!(&rec[100..115], "000000000000000", "{rec}");
}

#[test]
fn m130_cuenta_la_amortizacion_devengada() {
    let v = setup();
    emit(&v, 10_000, None, date!(2026 - 01 - 15));
    // Portátil de 1.200 € el 1/7 (25 % anual): va al inmovilizado, no a gasto.
    receive_as(
        &v,
        "B12345674",
        "PC-1",
        (1200, 252, 0),
        date!(2026 - 07 - 01),
        Nature::General,
        true,
    );
    // En normal, [02] es sólo gasto contable más amortización (sin el 5 % de la simplificada).
    let mut years = std::collections::BTreeMap::new();
    years.insert(
        2026,
        models::YearSettings {
            irpf_regime: Some(taxpain_models::m100::IrpfRegime::Normal),
            ..models::YearSettings::default()
        },
    );
    models::save_settings(
        &v,
        &ModelSettings {
            years,
            ..ModelSettings::default()
        },
        at(date!(2026 - 01 - 02)),
    )
    .unwrap();
    let gastos = |q: u8| get(&models::draft(&v, "130", 2026, q).unwrap(), 2);
    assert_eq!(gastos(2), Money::ZERO);
    // 1/7–30/9: 300 × 92 / 365 = 75,62.
    assert_eq!(gastos(3), Money::from_cents(7562));
    // 1/7–31/12: 300 × 184 / 365 = 151,23; igual tras contabilizar la amortización (sin duplicar).
    assert_eq!(gastos(4), Money::from_cents(15_123));
    let ids =
        taxpain_app::accounting::post_depreciation(&v, 2026, at(date!(2026 - 12 - 31))).unwrap();
    assert_eq!(ids.len(), 1);
    assert_eq!(gastos(4), Money::from_cents(15_123));
    assert_eq!(gastos(3), Money::from_cents(7562));
}

#[test]
fn m111_y_m115_separan_alquileres() {
    let v = scenario();
    let d111 = models::draft(&v, "111", 2026, 3).unwrap();
    assert_eq!(get(&d111, 2), eur(2000));
    assert_eq!(get(&d111, 3), eur(300));
    assert_eq!(get(&d111, 8), eur(400));
    assert_eq!(get(&d111, 9), eur(60));
    assert_eq!(d111.result, eur(360));
    assert_eq!(d111.payees, vec![(1, 1), (7, 1)]);

    let d115 = models::draft(&v, "115", 2026, 3).unwrap();
    assert_eq!(get(&d115, 2), eur(1000));
    assert_eq!(d115.result, eur(190));
    assert_eq!(d115.payees, vec![(1, 1)]);
    // Detectado por el 19 %, sin marcar: se pide comprobarlo.
    assert!(
        d115.warnings
            .iter()
            .any(|w| w.contains("Alquileres detectados")),
        "{:?}",
        d115.warnings
    );

    // 1T sin retenciones practicadas: negativas.
    assert_eq!(models::draft(&v, "111", 2026, 1).unwrap().declaration, "N");
}

#[test]
fn domiciliacion_y_exportacion_auditada() {
    let v = scenario();
    models::save_settings(
        &v,
        &ModelSettings {
            iban: Some("ES9121000418450200051332".into()),
            domiciliar: true,
            ..ModelSettings::default()
        },
        datetime!(2026-10-01 9:00 UTC),
    )
    .unwrap();
    let d = models::draft(&v, "303", 2026, 3).unwrap();
    assert_eq!(d.declaration, "U");
    // Página DID: SWIFT-BIC (11, en blanco para cuentas españolas) y después el IBAN.
    assert!(d.file.contains(&format!(
        "<T303DID00>{}ES9121000418450200051332",
        " ".repeat(11)
    )));
    let bytes = models::export(&v, &d, datetime!(2026-10-14 9:00 UTC)).unwrap();
    assert_eq!(bytes.len(), d.file.chars().count());
    let log = taxpain_vault::audit::entries(v.connection()).unwrap();
    assert!(
        log.iter()
            .any(|e| e.action == "models.export" && e.subject == "303/2026/3T")
    );
    assert!(v.verify_audit().is_ok());
}

#[test]
fn m349_con_entregas_y_servicios_intracomunitarios() {
    use taxpain_core::invoice::{Exemption, IdType, NotSubject};
    let v = setup();
    let foreign = |country: &str, id: &str, name: &str| Party {
        id: PartyId::Foreign {
            country: country.into(),
            id_type: IdType::NifIva,
            id: id.into(),
        },
        name: name.into(),
        address: addr(),
        person: None,
    };
    let send = |recipient: Party, vat: VatTreatment, euros: i64, d: Date| {
        let draft = InvoiceDraft {
            kind: InvoiceKind::Completa,
            series: None,
            issue_date: d,
            operation_date: None,
            recipient: Some(recipient),
            lines: vec![InvoiceLine {
                description: "Servicio".into(),
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
            description: "Exportación".into(),
            notes: None,
            currency: None,
            oss: None,
        };
        issue(&v, &draft, at(d), true).unwrap();
    };
    let de = || foreign("DE", "DE123456789", "Kunde GmbH");
    send(
        de(),
        VatTreatment::Exempt(Exemption::E5),
        1000,
        date!(2026 - 07 - 10),
    );
    send(
        de(),
        VatTreatment::Exempt(Exemption::E5),
        500,
        date!(2026 - 08 - 10),
    );
    send(
        foreign("FR", "12345678901", "Client SARL"),
        VatTreatment::NotSubject(NotSubject::N2),
        300,
        date!(2026 - 09 - 10),
    );
    // Fuera del trimestre.
    send(
        de(),
        VatTreatment::Exempt(Exemption::E5),
        999,
        date!(2026 - 10 - 01),
    );

    let d = models::draft(&v, "349", 2026, 3).unwrap();
    assert_eq!(d.result, eur(1800));
    assert_eq!(d.details.len(), 2);
    assert!(d.details[0].0.contains("DE123456789") && d.details[0].0.ends_with("clave E"));
    assert_eq!(d.details[0].1, eur(1500));
    assert!(d.details[1].0.contains("FR12345678901"));
    assert!(d.warnings.is_empty(), "{:?}", d.warnings);
    assert_eq!(d.file.split("\r\n").count(), 3);

    // 4T: una rectificativa (−200) de la entrega de julio no se netea con el 4T: se avisa.
    use taxpain_core::invoice::{Rectification, RectificationMethod};
    let rect = InvoiceDraft {
        kind: InvoiceKind::Rectificativa(Rectification {
            code: 1,
            method: RectificationMethod::Diferencias,
        }),
        series: None,
        issue_date: date!(2026 - 10 - 05),
        operation_date: None,
        recipient: Some(de()),
        lines: vec![InvoiceLine {
            description: "Abono".into(),
            quantity_milli: 1000,
            unit_price: -eur(200),
            discount: Rate::ZERO,
            vat: VatTreatment::Exempt(Exemption::E5),
            surcharge: None,
            withholding: None,
            advance: false,
        }],
        rectified: vec![("A2026-0001".into(), date!(2026 - 07 - 10))],
        rectified_amounts: None,
        description: "Abono".into(),
        notes: None,
        currency: None,
        oss: None,
    };
    issue(&v, &rect, at(date!(2026 - 10 - 05)), true).unwrap();
    let q4 = models::draft(&v, "349", 2026, 4).unwrap();
    assert_eq!(q4.result, eur(999));
    assert!(q4.warnings.is_empty(), "{:?}", q4.warnings);
    // Registro de rectificación del 3T: antes 1.500, ahora 1.300.
    let rect = q4
        .details
        .iter()
        .find(|(n, _)| n.contains("rectifica 3T 2026"))
        .unwrap();
    assert!(
        rect.0.contains("DE123456789") && rect.0.contains("antes 1.500,00 €"),
        "{rect:?}"
    );
    assert_eq!(rect.1, eur(1300));
    let lines: Vec<&str> = q4.file.split("\r\n").collect();
    assert_eq!(lines.len(), 3, "declarante, operador y rectificación");
    let rec = lines[2];
    assert_eq!(&rec[146..152], "20263T");
    assert_eq!(&rec[152..165], "0000000130000");
    assert_eq!(&rec[165..178], "0000000150000");
}

#[test]
fn el_130_no_es_para_sociedades() {
    let v = setup();
    let mut s = invoicing::settings(&v).unwrap().unwrap();
    s.issuer.id = PartyId::Nif("B12345674".into());
    s.issuer.person = None;
    invoicing::save_settings(&v, &s, datetime!(2026-01-02 9:00 UTC)).unwrap();
    assert!(models::draft(&v, "130", 2026, 1).is_err());
    assert!(models::draft(&v, "303", 2026, 1).is_ok());
    // 202: sin la cuota del último IS avisa y sale a cero; con ella, 18 %.
    let d = models::draft(&v, "202", 2026, 2).unwrap();
    assert_eq!(d.declaration, "N");
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("Impuesto sobre Sociedades de 2025")),
        "{:?}",
        d.warnings
    );
    models::save_settings(
        &v,
        &ModelSettings {
            corporate_tax_base: Some(eur(5000)),
            cnae: "6201".into(),
            corporate_tax_rate: "25".into(),
            ..ModelSettings::default()
        },
        datetime!(2026-09-01 9:00 UTC),
    )
    .unwrap();
    let d = models::draft(&v, "202", 2026, 2).unwrap();
    assert_eq!(d.result, eur(900));
    assert_eq!(d.file_name, "B12345674_2026_2P_202.txt");
    assert!(models::draft(&v, "202", 2026, 4).is_err());
    // 1P sin base propia: usa la misma con aviso (debería ser la del IS de hace dos años).
    let p1 = models::draft(&v, "202", 2026, 1).unwrap();
    assert_eq!(p1.result, eur(900));
    assert!(p1.warnings.iter().any(|w| w.contains("dos ejercicios")));
    // 200: sin operaciones, base cero; los tres 202 (900 €) salen a devolver.
    let d = models::draft(&v, "200", 2026, 0).unwrap();
    assert_eq!(d.result, -eur(2700));
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("No consta presentado el 202 de 1P 2026, 2P 2026, 3P 2026"))
    );
    // Con la base del 1P propia (IS de 2024 = 2.000 €): 1P = 360, el 200 descuenta 2.160.
    models::save_settings(
        &v,
        &ModelSettings {
            corporate_tax_base: Some(eur(5000)),
            corporate_tax_base_1p: Some(eur(2000)),
            cnae: "6201".into(),
            corporate_tax_rate: "25".into(),
            ..ModelSettings::default()
        },
        datetime!(2026-09-01 9:00 UTC),
    )
    .unwrap();
    assert_eq!(models::draft(&v, "202", 2026, 1).unwrap().result, eur(360));
    let d = models::draft(&v, "200", 2026, 0).unwrap();
    assert_eq!(d.result, -eur(2160));
    // Fichero para importar en Sociedades WEB: a devolver (tipo D) con su página DID.
    assert_eq!(d.file_name, "B12345674_200_2026.200");
    assert!(d.file.starts_with("<T200020260A0000><AUX>"));
    assert!(d.file.contains("<T20001000> DB12345674"));
    assert!(d.file.contains("<T200DID00>"));
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("Sociedades WEB") && w.contains("no lo presenta"))
    );
    let bytes = models::export(&v, &d, datetime!(2027-07-01 9:00 UTC)).unwrap();
    assert_eq!(bytes.len(), d.file.chars().count());
    assert!(models::draft(&v, "369", 2026, 1).is_err());
    // El 2P lo presentó la gestoría (tecleado): el 200 usa lo ingresado, no el borrador.
    let boxes = std::collections::BTreeMap::from([(3u16, eur(1000))]);
    taxpain_app::presentados::save_manual(
        &v,
        "202",
        2026,
        "2P",
        &boxes,
        datetime!(2026-10-20 9:00 UTC),
    )
    .unwrap();
    let d = models::draft(&v, "200", 2026, 0).unwrap();
    assert_eq!(d.result, -eur(360 + 1000 + 900));
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("No consta presentado el 202 de 1P 2026, 3P 2026:")),
        "{:?}",
        d.warnings
    );
    // El 200 de 2025 (del gestor) dejó una base negativa de 10.000 €: en 2026 se avisa si no se
    // indican las bases pendientes.
    let boxes = std::collections::BTreeMap::from([(552u16, -eur(10_000))]);
    taxpain_app::presentados::save_manual(
        &v,
        "200",
        2025,
        "0A",
        &boxes,
        datetime!(2026-07-20 9:00 UTC),
    )
    .unwrap();
    let d = models::draft(&v, "200", 2026, 0).unwrap();
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("10.000,00 € de bases imponibles negativas")),
        "{:?}",
        d.warnings
    );
}

#[test]
fn presentaciones_guardadas_e_inmutables() {
    let v = scenario();
    let d = models::draft(&v, "303", 2026, 1).unwrap();
    let receipt = models::FiledReceipt {
        csv: "AAAAAAAAAAAAAAAA".into(),
        receipt_number: "3030000000001".into(),
        pdf_url: "https://www2.agenciatributaria.gob.es/x".into(),
        ..models::FiledReceipt::default()
    };
    assert!(models::check_presenter(&v, "12345678Z").is_ok());
    assert!(models::check_presenter(&v, "B12345674").is_err());
    let id =
        models::record_filing(&v, &d, false, &receipt, datetime!(2026-04-15 9:00 UTC)).unwrap();
    let list = models::filed_returns(&v, Some(2026)).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].csv, "AAAAAAAAAAAAAAAA");
    assert_eq!(list[0].environment, "pruebas");
    assert_eq!(list[0].period, "1T");
    assert!(
        v.connection()
            .execute("DELETE FROM tax_return WHERE id = ?1", [id])
            .is_err()
    );
    assert!(
        v.connection()
            .execute("UPDATE tax_return SET csv = 'X' WHERE id = ?1", [id])
            .is_err()
    );
    let log = taxpain_vault::audit::entries(v.connection()).unwrap();
    assert!(
        log.iter()
            .any(|e| e.action == "models.file" && e.subject == "303/2026/1T")
    );
}

#[test]
fn las_presentaciones_en_produccion_mandan_en_los_acumulados() {
    let v = scenario();
    // Recalculado, el 1T sale a compensar 105 €; si en producción se presentó a compensar 200,
    // el 3T arrastra 200.
    let mut d1 = models::draft(&v, "303", 2026, 1).unwrap();
    for b in d1.boxes.iter_mut() {
        if b.0 == 71 {
            b.1 = -eur(200);
        }
    }
    let receipt = models::FiledReceipt {
        csv: "BBBBBBBBBBBBBBBB".into(),
        ..models::FiledReceipt::default()
    };
    models::record_filing(&v, &d1, true, &receipt, datetime!(2026-04-15 9:00 UTC)).unwrap();
    let t3 = models::draft(&v, "303", 2026, 3).unwrap();
    assert_eq!(get(&t3, 110), eur(200));
    // Las presentaciones de pruebas no cuentan.
    let v2 = scenario();
    let d = models::draft(&v2, "303", 2026, 1).unwrap();
    models::record_filing(&v2, &d1, false, &receipt, datetime!(2026-04-15 9:00 UTC)).unwrap();
    assert_eq!(
        get(&models::draft(&v2, "303", 2026, 3).unwrap(), 110),
        -d.result
    );
}

#[test]
fn la_naturaleza_marcada_manda_sobre_la_heuristica() {
    let v = setup();
    let d = date!(2026 - 08 - 10);
    // Alquiler con una retención que no es del 19 % (p. ej. un arrendador con otra situación).
    receive_as(
        &v,
        "B12345674",
        "ALQ-1",
        (1000, 210, 150),
        d,
        Nature::Alquiler,
        false,
    );
    // Profesional con retención del 19 % (no es un alquiler).
    receive_as(
        &v,
        "A22222228",
        "P-19",
        (1000, 210, 190),
        d,
        Nature::Profesional,
        false,
    );
    // Ordenador: bien de inversión.
    receive_as(
        &v,
        "A22222228",
        "ORD-1",
        (1500, 315, 0),
        d,
        Nature::General,
        true,
    );
    let d115 = models::draft(&v, "115", 2026, 3).unwrap();
    assert_eq!(d115.result, eur(150));
    // Ya marcado como alquiler: no hay nada que comprobar.
    assert!(
        !d115
            .warnings
            .iter()
            .any(|w| w.contains("Alquileres detectados")),
        "{:?}",
        d115.warnings
    );
    assert_eq!(models::draft(&v, "111", 2026, 3).unwrap().result, eur(190));
    let d303 = models::draft(&v, "303", 2026, 3).unwrap();
    assert_eq!(get(&d303, 30), eur(1500));
    assert_eq!(get(&d303, 31), eur(315));
    assert_eq!(get(&d303, 28), eur(2000));
    // El ordenador va al inmovilizado (217) y no a gasto: el 130 sólo cuenta en [02] su
    // amortización devengada (375 × 52 / 365 = 53,42 del 10/8 al 30/9).
    let d130 = models::draft(&v, "130", 2026, 3).unwrap();
    assert_eq!(get(&d130, 2), eur(2000) + Money::from_cents(5342));
    let assets = taxpain_app::accounting::fixed_assets(&v).unwrap();
    assert_eq!(assets.len(), 1);
    assert_eq!(assets[0].account.as_str(), "217");
    assert_eq!(assets[0].cost, eur(1500));
    assert_eq!(assets[0].rate, Rate::percent(25));
}

#[test]
fn presentacion_en_duda_bloquea_el_reintento() {
    use models::FilingEvent;
    let v = scenario();
    let d = models::draft(&v, "303", 2026, 3).unwrap();
    let now = at(date!(2026 - 10 - 14));
    assert!(models::ensure_reviewed(&d, &d.file).is_ok());
    assert!(models::ensure_reviewed(&d, "otro fichero").is_err());

    assert!(!models::filing_in_doubt(&v, &d, false).unwrap());
    models::record_filing_event(&v, &d, false, FilingEvent::Request, "", now).unwrap();
    // Interrumpida (la app se cerró a mitad): en duda.
    assert!(models::filing_in_doubt(&v, &d, false).unwrap());
    // Sin conexión (nada salió del Mac): no hay duda.
    models::record_filing_event(&v, &d, false, FilingEvent::NotSent, "sin red", now).unwrap();
    assert!(!models::filing_in_doubt(&v, &d, false).unwrap());
    models::record_filing_event(&v, &d, false, FilingEvent::Request, "", now).unwrap();
    models::record_filing_event(&v, &d, false, FilingEvent::Unknown, "timeout", now).unwrap();
    assert!(models::filing_in_doubt(&v, &d, false).unwrap());
    // Producción es independiente de pruebas.
    assert!(!models::filing_in_doubt(&v, &d, true).unwrap());
    // El usuario consultó y no consta: reintenta; un rechazo deja de estar en duda.
    models::record_filing_event(&v, &d, false, FilingEvent::RetryAfterUnknown, "", now).unwrap();
    assert!(!models::filing_in_doubt(&v, &d, false).unwrap());
    models::record_filing_event(&v, &d, false, FilingEvent::Request, "", now).unwrap();
    models::record_filing_event(&v, &d, false, FilingEvent::Rejected, "E01", now).unwrap();
    assert!(!models::filing_in_doubt(&v, &d, false).unwrap());
}

#[test]
fn retenciones_por_fecha_de_pago_no_de_revision() {
    let v = setup();
    // Factura de un profesional de diciembre (pagada entonces), revisada en enero.
    let rec = ingest_file(
        &v,
        "p.xml",
        b"<?xml version=\"1.0\"?><x n=\"dic\"/>",
        Source::Manual,
        at(date!(2026 - 01 - 10)),
    )
    .unwrap();
    review(
        &v,
        rec.id,
        &Reviewed {
            supplier_nif: "A22222228".into(),
            supplier_name: "Abogada".into(),
            number: "P-99".into(),
            issue_date: date!(2025 - 12 - 20),
            base: eur(400),
            vat: eur(84),
            withholding: eur(60),
            total: eur(424),
            deductible_pct: 100,
            capital_good: false,
            nature: Nature::Profesional,
            asset_class: None,
            vat_regime: Default::default(),
            supplier_country: None,
            self_assessed_rate: None,
            intra_goods: false,
            cash_basis: false,
            import_dua: false,
            prorata_use: Default::default(),
        },
        at(date!(2026 - 01 - 10)),
    )
    .unwrap();
    // El 111 del 4T 2025 la incluye; el del 1T 2026, no (art. 78.1 RIRPF).
    let q4 = models::draft(&v, "111", 2025, 4).unwrap();
    assert_eq!(get(&q4, 9), eur(60));
    let q1 = models::draft(&v, "111", 2026, 1).unwrap();
    assert_eq!(get(&q1, 9), Money::ZERO);
    // El IVA, en cambio, se deduce cuando se registra (1T 2026).
    let iva = models::draft(&v, "303", 2026, 1).unwrap();
    assert_eq!(get(&iva, 29), eur(84));
}

#[test]
fn compra_intracomunitaria_con_autorrepercusion() {
    use taxpain_app::received::VatRegime;
    let v = setup();
    let ingest = |name: &str| {
        ingest_file(
            &v,
            name,
            format!("<?xml version=\"1.0\"?><x n=\"{name}\"/>").as_bytes(),
            Source::Manual,
            at(date!(2026 - 08 - 03)),
        )
        .unwrap()
        .id
    };
    let base = Reviewed {
        supplier_nif: "1234567T".into(),
        supplier_name: "Proveedor Irlanda Ltd".into(),
        number: "IE-2026-77".into(),
        issue_date: date!(2026 - 08 - 01),
        base: eur(100),
        vat: Money::ZERO,
        withholding: Money::ZERO,
        total: eur(100),
        deductible_pct: 100,
        capital_good: false,
        nature: Nature::General,
        asset_class: None,
        vat_regime: VatRegime::Intracomunitaria,
        supplier_country: Some("IE".into()),
        self_assessed_rate: None,
        intra_goods: false,
        cash_basis: false,
        import_dua: false,
        prorata_use: Default::default(),
    };
    let id = ingest("ie.xml");
    // Sin país, o con IVA en la factura, no se admite como intracomunitaria.
    assert!(
        review(
            &v,
            id,
            &Reviewed {
                supplier_country: None,
                ..base.clone()
            },
            at(date!(2026 - 08 - 03))
        )
        .is_err()
    );
    assert!(
        review(
            &v,
            id,
            &Reviewed {
                vat: eur(21),
                total: eur(121),
                ..base.clone()
            },
            at(date!(2026 - 08 - 03))
        )
        .is_err()
    );
    review(&v, id, &base, at(date!(2026 - 08 - 03))).unwrap();

    let d = models::draft(&v, "303", 2026, 3).unwrap();
    assert_eq!((get(&d, 10), get(&d, 11)), (eur(100), eur(21)));
    assert_eq!((get(&d, 36), get(&d, 37)), (eur(100), eur(21)));
    assert_eq!(d.result, Money::ZERO, "devenga y deduce lo mismo");
    assert!(
        d.warnings.iter().all(|w| !w.contains("sin IVA")),
        "{:?}",
        d.warnings
    );

    let d349 = models::draft(&v, "349", 2026, 3).unwrap();
    assert_eq!(d349.details.len(), 1, "{:?}", d349.details);
    assert!(d349.details[0].0.contains("IE1234567T") && d349.details[0].0.ends_with("clave I"));
    assert_eq!(d349.result, eur(100));

    // Contabilidad: 472 y 477 por 21 €; el proveedor por 100 €.
    let tb = taxpain_app::accounting::trial_balance(&v, 2026, Some(3)).unwrap();
    let bal = |c: &str| {
        tb.iter()
            .find(|b| b.account.as_str() == c)
            .map_or(Money::ZERO, |b| b.balance())
    };
    assert_eq!(bal("472"), eur(21));
    assert_eq!(bal("477"), -eur(21));
    // 347: un proveedor sin NIF español no va.
    let d347 = models::draft(&v, "347", 2026, 0).unwrap();
    assert!(d347.details.iter().all(|(n, _)| !n.contains("1234567T")));
    // 390: servicios intracomunitarios al 21 %: devengo [551]/[552] y deducción [637]/[638].
    let d390 = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!((get(&d390, 551), get(&d390, 552)), (eur(100), eur(21)));
    assert_eq!((get(&d390, 637), get(&d390, 638)), (eur(100), eur(21)));
    assert_eq!((get(&d390, 597), get(&d390, 598)), (eur(100), eur(21)));
    assert_eq!((get(&d390, 33), get(&d390, 34)), (eur(100), eur(21)));
    assert_eq!(get(&d390, 64), eur(21));
    assert_eq!(get(&d390, 65), Money::ZERO);
    assert!(!d390.file.is_empty());
}

#[test]
fn cotejo_con_los_datos_fiscales_de_la_aeat() {
    use taxpain_models::datos_fiscales::parse;
    let v = setup();
    // 2025: una factura de 1.000 € con 15 % de retención a B12345674.
    emit(&v, 1000, Some(Rate::percent(15)), date!(2025 - 05 - 10));
    assert!(models::cross_check(&v, 2025).unwrap().is_none());
    let pad = |s: &str, n: usize| format!("{s:<n$}");
    let file = format!(
        "0DDFF2025\n1{}20260402\n2RTC0001B12345674{}G1{:013}{:013}\n2RTC0002A22222228{}G1{:013}{:013}\n2PF00001130I1T46{}{:013}\n2CT00001{}0521{:017}\n99\n",
        " ".repeat(50),
        pad("CLIENTE UNO SL", 40),
        100_000,
        15_000,
        pad("CLIENTE DOS SA", 40),
        50_000,
        7_500,
        pad("1300000000001", 13),
        3_000,
        pad("281234567890", 12),
        354_000
    );
    let data = parse(file.as_bytes()).unwrap();
    models::store_tax_data(&v, file.as_bytes(), &data, at(date!(2026 - 04 - 10))).unwrap();
    let c = models::cross_check(&v, 2025).unwrap().unwrap();
    assert_eq!(c.process_date.as_deref(), Some("20260402"));
    let line = |label: &str| c.lines.iter().find(|l| l.label.contains(label)).unwrap();
    let uno = line("B12345674): ingresos");
    assert!(uno.matches, "{uno:?}");
    assert!(line("B12345674): retenciones").matches);
    // Un pagador que declara ingresos que no están en TaxPain: diferencia.
    let dos = line("A22222228): ingresos");
    assert!(!dos.matches && dos.aeat == eur(500) && dos.taxpain == Money::ZERO);
    // 130 del 1T en la AEAT y no presentado desde TaxPain.
    assert!(!line("Modelo 130 1T").matches);
    // Cuotas de autónomo sin contabilizar: diferencia y nota.
    let reta = line("RETA");
    assert_eq!((reta.aeat, reta.taxpain), (eur(3540), Money::ZERO));
    assert!(c.notes.iter().any(|n| n.contains("642")));
}

#[test]
fn m303_con_criterio_de_caja() {
    use taxpain_app::cartera;
    let v = setup();
    let s = ModelSettings {
        cash_basis: true,
        ..Default::default()
    };
    models::save_settings(&v, &s, at(date!(2026 - 01 - 02))).unwrap();
    // 1T: venta de 1.000 (+210) en dos plazos iguales, otra de 2.000 (+420) que nunca se cobra,
    // y una compra de 500 (+105).
    emit(&v, 1000, None, date!(2026 - 02 - 10));
    emit(&v, 2000, None, date!(2026 - 03 - 01));
    receive(&v, "B12345674", "G-1", 500, 105, 0, date!(2026 - 02 - 15));
    let today = date!(2026 - 12 - 01);
    let find = |kind: &str, amount: Money| {
        cartera::list(&v, today)
            .unwrap()
            .into_iter()
            .find(|d| d.kind == kind && d.invoice_amount == amount)
            .unwrap()
            .invoice_id
    };
    let sale = find("emitida", eur(1210));
    let buy = find("recibida", eur(605));
    cartera::set_schedule(
        &v,
        "emitida",
        sale,
        Some(&[
            (date!(2026 - 04 - 10), Money::from_cents(60_500)),
            (date!(2026 - 07 - 10), Money::from_cents(60_500)),
        ]),
        at(today),
    )
    .unwrap();

    // 1T: nada cobrado ni pagado → sin IVA, pero la información adicional por la regla general.
    let t1 = models::draft(&v, "303", 2026, 1).unwrap();
    assert_eq!(get(&t1, 9), Money::ZERO);
    assert_eq!(get(&t1, 29), Money::ZERO);
    assert_eq!((get(&t1, 62), get(&t1, 63)), (eur(3000), eur(630)));
    assert_eq!((get(&t1, 74), get(&t1, 75)), (eur(500), eur(105)));
    assert_eq!(t1.result, Money::ZERO);
    assert!(t1.warnings.iter().any(|w| w.contains("criterio de caja")));
    // Identificación: acogido al criterio de caja [pos. 113] = 1; destinatario [114] = 2.
    let p1 = &t1.file[t1.file.find("<T30301000>").unwrap()..];
    let id: String = p1.chars().skip(112).take(2).collect();
    assert_eq!(id, "12");

    // 2T: se cobra el primer plazo (la mitad) y se paga la compra.
    cartera::settle(
        &v,
        "emitida",
        sale,
        1,
        date!(2026 - 04 - 20),
        "572",
        at(today),
    )
    .unwrap();
    cartera::settle(
        &v,
        "recibida",
        buy,
        1,
        date!(2026 - 05 - 10),
        "572",
        at(today),
    )
    .unwrap();
    let t2 = models::draft(&v, "303", 2026, 2).unwrap();
    assert_eq!(
        (get(&t2, 7), get(&t2, 9)),
        (eur(500), Money::from_cents(10_500))
    );
    assert_eq!(get(&t2, 29), eur(105));
    assert_eq!(get(&t2, 62), Money::ZERO);
    assert_eq!(t2.result, Money::ZERO);
    // La liquidación contable del 2T usa el IVA devengado y deducido, no el de las facturas.
    let id = taxpain_app::accounting::settle_vat_quarter(&v, 2026, 2, at(today))
        .unwrap()
        .unwrap();
    let e = taxpain_app::accounting::journal(&v, 2026)
        .unwrap()
        .into_iter()
        .find(|e| e.id == id)
        .unwrap();
    let on = |a: &str| -> Money {
        e.entry
            .lines
            .iter()
            .filter(|l| l.account.as_str().starts_with(a))
            .map(|l| l.debit - l.credit)
            .sum()
    };
    assert_eq!(on("477"), Money::from_cents(10_500));
    assert_eq!(on("472"), -eur(105));

    // 3T: el segundo plazo.
    cartera::settle(
        &v,
        "emitida",
        sale,
        2,
        date!(2026 - 07 - 15),
        "570",
        at(today),
    )
    .unwrap();
    let t3 = models::draft(&v, "303", 2026, 3).unwrap();
    assert_eq!(get(&t3, 9), Money::from_cents(10_500));

    // La venta no cobrada se devenga el 31/12/2027 (4T de 2027).
    let t4 = models::draft(&v, "303", 2027, 4).unwrap();
    assert_eq!((get(&t4, 7), get(&t4, 9)), (eur(2000), eur(420)));
    assert_eq!(
        get(&models::draft(&v, "303", 2026, 4).unwrap(), 9),
        Money::ZERO
    );
}

#[test]
fn pago_de_los_modelos_contabilizado() {
    use taxpain_app::accounting::journal;
    use taxpain_app::tax_payments::{bank_candidates, pay, pay_from_bank, payment, undo};
    let v = scenario();
    let now = at(date!(2026 - 10 - 20));
    let balance = |acc: &str| -> Money {
        journal(&v, 2026)
            .unwrap()
            .iter()
            .flat_map(|e| e.entry.lines.iter())
            .filter(|l| l.account.as_str() == acc)
            .map(|l| l.debit - l.credit)
            .sum()
    };

    // 303 3T (756 a ingresar) en efectivo: primero la liquidación del trimestre (4750 al haber)
    // y luego el pago (4750 al debe, 570 al haber).
    let p = pay(&v, "303", 2026, 3, date!(2026 - 10 - 20), "570", now).unwrap();
    assert_eq!(p.amount, eur(756));
    assert_eq!(p.account, "4750");
    assert_eq!(balance("570"), -eur(756));
    assert!(pay(&v, "303", 2026, 3, date!(2026 - 10 - 20), "570", now).is_err());
    // 1T a compensar: no hay nada que pagar; y el 390 no se paga.
    assert!(pay(&v, "303", 2026, 1, date!(2026 - 04 - 20), "570", now).is_err());
    assert!(pay(&v, "390", 2026, 4, date!(2027 - 01 - 30), "570", now).is_err());
    assert!(pay(&v, "111", 2026, 3, date!(2026 - 10 - 20), "4300001", now).is_err());
    undo(&v, "303", 2026, 3, now).unwrap();
    assert_eq!(payment(&v, "303", 2026, 3).unwrap(), None);
    assert_eq!(balance("570"), Money::ZERO);

    // 111 3T (360) con el cargo del banco: se concilia y no se duplica el asiento.
    let conn = v.connection();
    conn.execute(
        "INSERT INTO bank_account(number, account, name) VALUES ('00490001', '5720001', 'Banco')",
        [],
    )
    .unwrap();
    for (amount, hash) in [(-36_000, "a"), (-12_345, "b")] {
        conn.execute(
            "INSERT INTO bank_tx(bank_account, op_date, value_date, amount, description, import_hash, created_at)
             VALUES (1, '2026-10-20', '2026-10-20', ?1, 'AEAT MOD 111', ?2, '2026-10-21')",
            taxpain_vault::rusqlite::params![amount, hash],
        )
        .unwrap();
    }
    let c = bank_candidates(&v, "111", 2026, 3).unwrap();
    assert_eq!(c.len(), 1);
    assert!(pay_from_bank(&v, "111", 2026, 3, c[0].id + 1, now).is_err());
    let p = pay_from_bank(&v, "111", 2026, 3, c[0].id, now).unwrap();
    assert_eq!((p.account.as_str(), p.bank_tx), ("4751", Some(c[0].id)));
    assert_eq!(balance("5720001"), -eur(360));
    // Queda la retención del alquiler (115), que también va a la 4751.
    assert_eq!(balance("4751"), -eur(190));
    assert!(undo(&v, "111", 2026, 3, now).is_err());
    // Se deshace desde el banco: el 111 vuelve a estar pendiente de pago y se puede repetir.
    taxpain_app::conciliados::unreconcile(&v, c[0].id, now, true).unwrap();
    assert_eq!(payment(&v, "111", 2026, 3).unwrap(), None);
    assert_eq!(balance("5720001"), Money::ZERO);
    assert_eq!(balance("4751"), -eur(550));
    pay_from_bank(&v, "111", 2026, 3, c[0].id, now).unwrap();
}

#[test]
fn casillas_del_303_fijadas_a_mano() {
    let v = scenario();
    let now = at(date!(2026 - 10 - 10));
    // 3T calculado: devengado 1.260, deducible 399 → [46] 861.
    models::set_adjustment(&v, "303", 2026, 3, 29, Some(eur(500)), now).unwrap();
    let t3 = models::draft(&v, "303", 2026, 3).unwrap();
    assert_eq!(get(&t3, 29), eur(500));
    assert_eq!(get(&t3, 45), eur(500));
    assert_eq!(get(&t3, 46), eur(760));
    assert!(t3.warnings.iter().any(|w| w.contains("[29] fijada a mano")));
    // [77] (IVA de importación pendiente de ingreso) fijada: entra en [69] y [71] (fórmulas del
    // diseño de registro), y el resultado del modelo es el de [71].
    models::set_adjustment(&v, "303", 2026, 3, 77, Some(eur(100)), now).unwrap();
    let t3 = models::draft(&v, "303", 2026, 3).unwrap();
    assert_eq!(get(&t3, 69), get(&t3, 66) + eur(100) - get(&t3, 78));
    assert_eq!(get(&t3, 71), get(&t3, 69));
    assert_eq!(t3.result, get(&t3, 71));
    models::set_adjustment(&v, "303", 2026, 3, 77, None, now).unwrap();
    // Los totales no se fijan, ni casillas de modelos informativos.
    assert!(models::set_adjustment(&v, "303", 2026, 3, 46, Some(eur(1)), now).is_err());
    assert!(models::set_adjustment(&v, "303", 2026, 3, 9999, Some(eur(1)), now).is_err());
    assert!(models::set_adjustment(&v, "130", 2026, 3, 7, Some(eur(1)), now).is_err());
    assert!(models::set_adjustment(&v, "347", 2026, 0, 1, Some(eur(1)), now).is_err());
    // 130: gastos fijados a mano; el rendimiento y el 20 % se recalculan.
    let before = models::draft(&v, "130", 2026, 3).unwrap();
    models::set_adjustment(&v, "130", 2026, 3, 2, Some(get(&before, 2) + eur(100)), now).unwrap();
    let after = models::draft(&v, "130", 2026, 3).unwrap();
    assert_eq!(get(&after, 3), get(&before, 3) - eur(100));
    assert_eq!(get(&after, 4), get(&before, 4) - eur(20));
    // 111: retenciones de profesionales [09] fijadas → [28] y [30] siguen.
    models::set_adjustment(&v, "111", 2026, 3, 9, Some(eur(75)), now).unwrap();
    let m111 = models::draft(&v, "111", 2026, 3).unwrap();
    assert_eq!(get(&m111, 9), eur(75));
    assert_eq!(get(&m111, 30), get(&m111, 3) + eur(75));
    assert_eq!(m111.result, get(&m111, 30));
    // 115: [03] fijada.
    models::set_adjustment(&v, "115", 2026, 3, 3, Some(eur(19)), now).unwrap();
    let m115 = models::draft(&v, "115", 2026, 3).unwrap();
    assert_eq!(get(&m115, 5), eur(19));
    assert_eq!(models::adjustments(&v, "303", 2026, 3).unwrap().len(), 1);
    // Vuelta al automático.
    models::set_adjustment(&v, "303", 2026, 3, 29, None, now).unwrap();
    assert_eq!(
        get(&models::draft(&v, "303", 2026, 3).unwrap(), 46),
        eur(861)
    );
}

#[test]
fn cartas_del_347() {
    let v = scenario();
    let (name, pdf) = models::letters_347(&v, 2026, "Valencia, 15 de enero de 2027").unwrap();
    assert_eq!(name, "Cartas 347 2026 12345678Z.pdf");
    let text = pdf_extract::extract_text_from_mem(&pdf).unwrap();
    assert!(text.contains("Cliente SL"), "{text}");
    // La factura del 1T lleva retención (va en el 190, no en el 347): sólo la del 3T.
    assert!(text.contains("Total del año 7.260,00 €"), "{text}");
    assert!(text.contains("Valencia, 15 de enero de 2027"));
    assert!(models::letters_347(&v, 2025, "x").is_err());
}

#[test]
fn el_190_cuadra_con_los_111() {
    let v = scenario();
    // Sin cambios, el 190 (profesional, 60 €) y los 111 (60 € del profesional + 300 de la
    // nómina hecha a mano, que no va al 190) no cuadran: se avisa.
    let d = models::draft(&v, "190", 2026, 0).unwrap();
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("no cuadran con la suma de los 111")),
        "{:?}",
        d.warnings
    );
    let d = models::draft(&v, "180", 2026, 0).unwrap();
    assert!(
        !d.warnings.iter().any(|w| w.contains("no cuadran")),
        "{:?}",
        d.warnings
    );
}

#[test]
fn prorrata_general_con_regularizacion() {
    use taxpain_core::invoice::Exemption;
    let v = setup();
    let s = ModelSettings {
        prorata: Some(80),
        cnae: "8622".into(),
        ..Default::default()
    };
    models::save_settings(&v, &s, at(date!(2026 - 01 - 02))).unwrap();
    // 1T: 1.000 con IVA, 1.000 exentos (art. 20) y una compra de 1.000 + 210.
    emit(&v, 1000, None, date!(2026 - 02 - 10));
    let exempt = InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: date!(2026 - 02 - 11),
        operation_date: None,
        recipient: Some(Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Cliente SL".into(),
            address: addr(),
            person: None,
        }),
        lines: vec![InvoiceLine {
            description: "Asistencia sanitaria".into(),
            quantity_milli: 1000,
            unit_price: eur(1000),
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
    issue(&v, &exempt, at(date!(2026 - 02 - 11)), true).unwrap();
    receive(&v, "B12345674", "G-1", 1000, 210, 0, date!(2026 - 02 - 15));

    // 1T: se deduce el 80 % provisional (168 de 210).
    let t1 = models::draft(&v, "303", 2026, 1).unwrap();
    assert_eq!((get(&t1, 28), get(&t1, 29)), (eur(800), eur(168)));
    assert_eq!(t1.result, eur(42));
    // 4T: definitiva del 50 % → regularización de (50 − 80) % de 210 = −63 en [44].
    let t4 = models::draft(&v, "303", 2026, 4).unwrap();
    assert_eq!(get(&t4, 44), -eur(63));
    assert!(
        t4.file.contains("<T30305000>"),
        "página 05 en el último periodo"
    );
    assert!(t4.file.contains("8622"));
}

/// Las ventas de inmovilizado no cuentan en la prorrata (art. 104.Tres.3.º LIVA): ni el torno
/// vendido con IVA (numerador y denominador) ni el local con la segunda entrega exenta (sólo
/// denominador). Queda 1.000 con derecho de 2.000: definitiva del 50 %, no del 7 %.
#[test]
fn prorrata_sin_las_ventas_de_inmovilizado() {
    use taxpain_core::invoice::Exemption;
    let v = setup();
    let s = ModelSettings {
        prorata: Some(80),
        cnae: "8622".into(),
        ..Default::default()
    };
    models::save_settings(&v, &s, at(date!(2026 - 01 - 02))).unwrap();
    emit(&v, 1000, None, date!(2026 - 02 - 10));
    emit_sale(
        &v,
        "Asistencia sanitaria",
        1000,
        VatTreatment::Exempt(Exemption::E1),
        date!(2026 - 02 - 11),
    );
    receive(&v, "B12345674", "G-1", 1000, 210, 0, date!(2026 - 02 - 15));
    sell_asset(&v, "Torno", "213", 12_000, 3000, date!(2026 - 05 - 10));
    emit_sale(&v, "Venta del torno", 3000, iva21(), date!(2026 - 05 - 12));
    sell_asset(&v, "Local", "211", 80_000, 60_000, date!(2026 - 09 - 15));
    emit_sale(
        &v,
        "Venta del local",
        60_000,
        VatTreatment::Exempt(Exemption::E1),
        date!(2026 - 09 - 15),
    );
    // Regularización con la definitiva del 50 %: (50 − 80) % de 210 = −63. Con las ventas,
    // 4.000 / 65.000 → 7 %: (7 − 80) % de 210 = −153,30.
    let t4 = models::draft(&v, "303", 2026, 4).unwrap();
    assert_eq!(get(&t4, 44), -eur(63), "{:?}", t4.boxes);
}

#[test]
fn importacion_con_dua() {
    let v = setup();
    let now = at(date!(2026 - 03 - 10));
    let rec = ingest_file(&v, "dua.pdf", b"%PDF-1.4 DUA", Source::Manual, now).unwrap();
    let dua = |total: i64| Reviewed {
        supplier_nif: "B12345674".into(),
        supplier_name: "Agente de aduanas SL".into(),
        number: "26ES00281011234567".into(),
        issue_date: date!(2026 - 03 - 10),
        base: eur(1000),
        vat: eur(210),
        withholding: Money::ZERO,
        total: eur(total),
        deductible_pct: 100,
        capital_good: false,
        nature: Nature::General,
        asset_class: None,
        vat_regime: Default::default(),
        supplier_country: None,
        self_assessed_rate: None,
        intra_goods: false,
        cash_basis: false,
        import_dua: true,
        prorata_use: Default::default(),
    };
    assert!(
        review(&v, rec.id, &dua(1210), now).is_err(),
        "el total es el IVA"
    );
    review(&v, rec.id, &dua(210), now).unwrap();
    // 303: IVA deducible de importaciones [32]/[33], no en [28]/[29].
    let t1 = models::draft(&v, "303", 2026, 1).unwrap();
    assert_eq!((get(&t1, 32), get(&t1, 33)), (eur(1000), eur(210)));
    assert_eq!(get(&t1, 29), Money::ZERO);
    assert_eq!(t1.result, -eur(210));
    // Asiento: sólo el IVA (472 contra el agente); la mercancía va en la factura extranjera.
    let j = taxpain_app::accounting::journal(&v, 2026).unwrap();
    let lines = &j[0].entry.lines;
    assert_eq!(lines.len(), 2);
    assert!(
        lines
            .iter()
            .any(|l| l.account.as_str() == "472" && l.debit == eur(210))
    );
    // En la cartera se paga el IVA.
    let c = taxpain_app::cartera::list(&v, date!(2026 - 03 - 31)).unwrap();
    assert_eq!(c[0].amount, eur(210));
    // 390: importaciones de bienes corrientes al 21 % [621]/[622] y total [52]/[53].
    let d390 = models::draft(&v, "390", 2026, 4).unwrap();
    assert_eq!(get(&d390, 622), eur(210));
    assert_eq!(get(&d390, 53), eur(210));
}

#[test]
fn prorrata_especial_e_inversiones() {
    use taxpain_core::invoice::Exemption;
    let v = setup();
    // 2025: ordenador de 10.000 + 2.100, con todo el IVA deducido (prorrata del 100 %).
    receive_as(
        &v,
        "B12345674",
        "BI-1",
        (10_000, 2_100, 0),
        date!(2025 - 06 - 10),
        Nature::General,
        true,
    );
    let s = ModelSettings {
        prorata: Some(100),
        prorata_special: true,
        prorata_history: [(2025, 100)].into_iter().collect(),
        cnae: "8622".into(),
        activity_description: "Clínica".into(),
        ..Default::default()
    };
    models::save_settings(&v, &s, at(date!(2026 - 01 - 02))).unwrap();
    // 2026: 1.000 con IVA y 1.000 exentos → definitiva del 50 %.
    emit(&v, 1000, None, date!(2026 - 02 - 10));
    let exempt = InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: date!(2026 - 02 - 11),
        operation_date: None,
        recipient: Some(Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Cliente SL".into(),
            address: addr(),
            person: None,
        }),
        lines: vec![InvoiceLine {
            description: "Asistencia sanitaria".into(),
            quantity_milli: 1000,
            unit_price: eur(1000),
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
    issue(&v, &exempt, at(date!(2026 - 02 - 11)), true).unwrap();
    // Compras: de uso común, sólo para lo gravado y sólo para lo exento.
    receive(&v, "B12345674", "G-1", 1000, 210, 0, date!(2026 - 02 - 15));
    receive(&v, "B12345674", "G-2", 500, 105, 0, date!(2026 - 02 - 16));
    receive(&v, "B12345674", "G-3", 300, 63, 0, date!(2026 - 02 - 17));
    for (n, u) in [("G-2", "plena"), ("G-3", "ninguna")] {
        v.connection()
            .execute(
                "UPDATE invoice_received SET prorata_use = ?1 WHERE number = ?2",
                [u, n],
            )
            .unwrap();
    }
    // 1T: la de uso común al 100 % provisional, la plena entera y la otra nada.
    let t1 = models::draft(&v, "303", 2026, 1).unwrap();
    assert_eq!((get(&t1, 28), get(&t1, 29)), (eur(1500), eur(315)));
    // 4T: [44] = 210 × (50 − 100) % = −105 (sólo la de uso común); [43] = 2.100 × (50 − 100) %
    // / 5 = −210 por el ordenador de 2025.
    let t4 = models::draft(&v, "303", 2026, 4).unwrap();
    assert_eq!(get(&t4, 44), -eur(105));
    assert_eq!(get(&t4, 43), -eur(210));
    let p05 = t4
        .file
        .lines()
        .flat_map(|l| l.split("<T30305000>").nth(1))
        .next()
        .unwrap_or_else(|| panic!("sin página 05: {}", t4.file));
    assert!(p05.contains("E05000"), "{p05}");
    // 390: regularizaciones [522] y [63], y la página 7 con la prorrata especial del 50 %.
    let a = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&a, 522), -eur(105));
    assert_eq!(get(&a, 63), -eur(210));
    assert!(a.file.contains("<T39007000>"), "{}", a.file);
    // Apartado 12: el CNAE con las 4 cifras del diseño de 2026 (posición 240), prorrata especial
    // (278) del 50 % (279).
    let p7: String = a.file[a.file.find("<T39007000>").unwrap()..]
        .chars()
        .take(781)
        .collect();
    assert!(p7.ends_with("</T39007000>"), "{p7}");
    assert_eq!(&p7[239..243], "8622");
    assert_eq!(&p7[277..283], "E05000");
    assert!(
        !a.warnings.iter().any(|w| w.contains("no coincide")),
        "{:?}",
        a.warnings
    );
}

#[test]
fn amortizacion_fiscal_distinta_en_el_200() {
    use taxpain_app::accounting::{add_fixed_asset, set_depreciation_plan};
    use taxpain_core::accounting::depreciation::{FixedAsset, Method};
    let v = setup();
    let mut s = invoicing::settings(&v).unwrap().unwrap();
    s.issuer.id = PartyId::Nif("B12345674".into());
    s.issuer.person = None;
    invoicing::save_settings(&v, &s, datetime!(2026-01-02 9:00 UTC)).unwrap();
    let now = datetime!(2026-01-02 9:00 UTC);
    let mut a = FixedAsset {
        id: 0,
        name: "Servidor".into(),
        account: acc("217"),
        depreciation_account: acc("2817"),
        expense_account: acc("681"),
        acquired_on: date!(2026 - 01 - 01),
        cost: eur(1200),
        residual: Money::ZERO,
        rate: Rate::percent(25),
        disposed_on: None,
        method: Method::Lineal,
        // Libertad de amortización: fiscalmente, todo el primer año.
        tax_rate: Some(Rate::percent(100)),
    };
    let id = add_fixed_asset(&v, &a, now).unwrap();
    // Contable 300, fiscal 1.200: disminución de 900 [00304].
    let d = models::draft(&v, "200", 2026, 0).unwrap();
    assert_eq!(get(&d, 304), eur(900));
    assert_eq!(get(&d, 418), eur(900));
    // Porcentaje constante (37,5 %): contable 450 → disminución 750.
    set_depreciation_plan(&v, id, Method::Constante, Some(Rate::percent(100)), now).unwrap();
    let d = models::draft(&v, "200", 2026, 0).unwrap();
    assert_eq!(get(&d, 304), eur(750));
    // En 2027 la fiscal ya está agotada: la contable entera es aumento [00303].
    let d = models::draft(&v, "200", 2027, 0).unwrap();
    assert_eq!(get(&d, 303), Money::from_cents(28_125));
    // El porcentaje constante no vale para mobiliario.
    a.account = acc("216");
    a.depreciation_account = acc("2816");
    a.method = Method::Constante;
    assert!(add_fixed_asset(&v, &a, now).is_err());
}

#[test]
fn modelo_184_de_una_comunidad_de_bienes() {
    use taxpain_app::atribucion::{self, Partner, Settings184};
    let v = setup();
    let mut s = invoicing::settings(&v).unwrap().unwrap();
    s.issuer.id = PartyId::Nif("E12345674".into());
    s.issuer.name = "Hermanos Pérez CB".into();
    s.issuer.person = None;
    invoicing::save_settings(&v, &s, datetime!(2026-01-02 9:00 UTC)).unwrap();
    let now = datetime!(2027-01-20 9:00 UTC);
    atribucion::save_settings(
        &v,
        &Settings184 {
            regime: 1,
            iae: "659.9".into(),
            representative_nif: "12345678Z".into(),
            representative_name: "Pérez Gómez Ana".into(),
            ..Settings184::default()
        },
        now,
    )
    .unwrap();
    for (nif, pct, joined) in [
        ("12345678Z", 600_000, None),
        ("00000000T", 400_000, Some(date!(2026 - 07 - 01))),
    ] {
        atribucion::save_partner(
            &v,
            &Partner {
                nif: nif.into(),
                name: format!("Socio {nif}"),
                province: "46".into(),
                pct,
                joined_on: joined,
                ..Partner::default()
            },
            now,
        )
        .unwrap();
    }
    assert!(
        atribucion::save_partner(
            &v,
            &Partner {
                nif: "12345678Z".into(),
                name: "X".into(),
                province: "99".into(),
                pct: 1,
                ..Partner::default()
            },
            now
        )
        .is_err()
    );
    // Ventas 10.000, compras 3.000 y alquiler del local 1.000.
    for (d, de, ha, e) in [
        (date!(2026 - 03 - 01), "5720001", "700", 10_000),
        (date!(2026 - 03 - 02), "600", "5720001", 3_000),
        (date!(2026 - 03 - 03), "621", "5720001", 1_000),
    ] {
        record_entry(
            v.connection(),
            &JournalEntry {
                date: d,
                concept: "x".into(),
                origin: Origin::Manual,
                lines: vec![
                    EntryLine::debit(acc(de), eur(e)),
                    EntryLine::credit(acc(ha), eur(e)),
                ],
            },
            now,
        )
        .unwrap();
    }
    let d = models::draft(&v, "184", 2026, 0).unwrap();
    assert_eq!(d.result, eur(6_000));
    assert_eq!(d.details.len(), 2);
    assert_eq!(d.details[0].1, eur(3_600));
    assert_eq!(d.details[1].1, eur(2_400));
    // Sólo el aviso de que el diseño de 2026 aún no está publicado (sale con el de 2025).
    assert!(
        d.warnings
            .iter()
            .all(|w| w.starts_with("Diseño de registro del 184 de 2026 no publicado")),
        "{:?}",
        d.warnings
    );
    let lines: Vec<&str> = d.file.split("\r\n").collect();
    assert_eq!(lines.len(), 1 + 1 + 2);
    // Segundo socio: miembro 184 días (desde el 1 de julio).
    let s2: String = lines[3].chars().skip(82).take(3).collect();
    assert_eq!(s2, "184");
}

#[test]
fn consolidacion_de_dos_empresas() {
    use taxpain_app::consolidation::{company_data, consolidate};
    use taxpain_core::accounting::statements::ModelKind;
    // A (12345678Z) vende 1.000 + IVA a B (B12345674), que se la compra.
    let a = setup();
    emit(&a, 1000, None, date!(2026 - 03 - 10));
    let b = setup();
    let mut s = invoicing::settings(&b).unwrap().unwrap();
    s.issuer.id = PartyId::Nif("B12345674".into());
    s.issuer.person = None;
    s.issuer.name = "Cliente SL".into();
    invoicing::save_settings(&b, &s, datetime!(2026-01-02 9:00 UTC)).unwrap();
    receive(&b, "12345678Z", "A-1", 1000, 210, 0, date!(2026 - 03 - 12));
    let group = vec!["12345678Z".to_owned(), "B12345674".to_owned()];
    let da = company_data(&a, "Empresa A", "12345678Z", 2026, &group).unwrap();
    let db = company_data(&b, "Empresa B", "B12345674", 2026, &group).unwrap();
    // Sin eliminar: ventas 1.000 y compras 1.000; eliminando, ni ventas ni deudas internas.
    let sum = consolidate(&[da.clone(), db.clone()], ModelKind::Pymes, false);
    let saldo = |c: &taxpain_app::consolidation::Consolidated, g: char| -> Money {
        c.balances
            .iter()
            .filter(|x| x.account.as_str().starts_with(g))
            .map(|x| x.balance())
            .sum()
    };
    assert_eq!(saldo(&sum, '7'), -eur(1000));
    let c = consolidate(&[da, db], ModelKind::Pymes, true);
    assert_eq!(saldo(&c, '7'), Money::ZERO);
    assert_eq!(saldo(&c, '6'), Money::ZERO);
    assert!(c.accounts.balanced, "{:?}", c.warnings);
    assert!(
        !c.warnings.iter().any(|w| w.contains("no cuadra")),
        "{:?}",
        c.warnings
    );
    assert_eq!(c.eliminations.len(), 4);
}

#[test]
fn datos_de_los_modelos_por_ejercicio() {
    use taxpain_app::models::YearSettings;
    let v = setup();
    let mut s = invoicing::settings(&v).unwrap().unwrap();
    s.issuer.id = PartyId::Nif("B12345674".into());
    s.issuer.person = None;
    invoicing::save_settings(&v, &s, datetime!(2026-01-02 9:00 UTC)).unwrap();
    let mut years = std::collections::BTreeMap::new();
    years.insert(
        2026,
        YearSettings {
            corporate_tax_base: Some(eur(5000)),
            corporate_tax_base_1p: Some(eur(2000)),
            ..YearSettings::default()
        },
    );
    models::save_settings(
        &v,
        &ModelSettings {
            cnae: "6201".into(),
            corporate_tax_rate: "25".into(),
            years,
            ..ModelSettings::default()
        },
        datetime!(2026-09-01 9:00 UTC),
    )
    .unwrap();
    // 2026: los suyos, sin avisos de configuración antigua.
    let d = models::draft(&v, "202", 2026, 2).unwrap();
    assert_eq!(d.result, eur(900));
    assert!(
        !d.warnings
            .iter()
            .any(|w| w.contains("no indican el ejercicio"))
    );
    assert_eq!(models::draft(&v, "202", 2026, 1).unwrap().result, eur(360));
    // 2027 sin datos: no se arrastra la cuota de 2026 (antes se reutilizaba sin avisar).
    let d = models::draft(&v, "202", 2027, 2).unwrap();
    assert_eq!(d.result, Money::ZERO);
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("Impuesto sobre Sociedades de 2026")),
        "{:?}",
        d.warnings
    );
    // Con el 200 de 2026 presentado por el gestor ([00599] = 3.000 €), el 2P de 2027 sale de él.
    let mut boxes = std::collections::BTreeMap::new();
    boxes.insert(599u16, eur(3000));
    taxpain_app::presentados::save_manual(
        &v,
        "200",
        2026,
        "0A",
        &boxes,
        datetime!(2027-08-01 9:00 UTC),
    )
    .unwrap();
    let d = models::draft(&v, "202", 2027, 2).unwrap();
    assert_eq!(d.result, eur(540));
    // Y ese 200 tecleado se puede comparar con el borrador de TaxPain.
    let filed = taxpain_app::presentados::list(&v, Some(2026)).unwrap();
    let c = taxpain_app::presentados::compare(&v, filed[0].id).unwrap();
    assert!(c.rows.iter().any(|r| r.box_ == 599), "{:?}", c.rows);
    assert!(
        d.warnings.iter().any(|w| w.contains("[00599]")),
        "{:?}",
        d.warnings
    );
}

#[test]
fn m349_avisa_del_paso_a_mensual_al_superar_50000() {
    use taxpain_core::calendar::{Periodicity, TaxpayerProfile};
    use taxpain_core::invoice::{Exemption, IdType};
    let v = setup();
    let send = |euros: i64, d: Date| {
        let draft = InvoiceDraft {
            kind: InvoiceKind::Completa,
            series: None,
            issue_date: d,
            operation_date: None,
            recipient: Some(Party {
                id: PartyId::Foreign {
                    country: "DE".into(),
                    id_type: IdType::NifIva,
                    id: "DE123456789".into(),
                },
                name: "Kunde GmbH".into(),
                address: addr(),
                person: None,
            }),
            lines: vec![InvoiceLine {
                description: "Mercancía".into(),
                quantity_milli: 1000,
                unit_price: eur(euros),
                discount: Rate::ZERO,
                vat: VatTreatment::Exempt(Exemption::E5),
                surcharge: None,
                withholding: None,
                advance: false,
            }],
            rectified: vec![],
            rectified_amounts: None,
            description: "Entrega intracomunitaria".into(),
            notes: None,
            currency: None,
            oss: None,
        };
        issue(&v, &draft, at(d), true).unwrap();
    };
    let avisa = |year: i32, q: u8| {
        models::draft(&v, "349", year, q)
            .unwrap()
            .warnings
            .iter()
            .any(|w| w.contains("50.000 €") && w.contains("2T 2026"))
    };
    send(30_000, date!(2026 - 04 - 10));
    assert!(!avisa(2026, 2));
    // 55.000 € en el 2T: mensual desde ese mes (RIVA art. 81.2)…
    send(25_000, date!(2026 - 05 - 10));
    assert!(avisa(2026, 2));
    // …y en los cuatro trimestres siguientes; en el quinto ya no cuenta.
    assert!(avisa(2027, 2));
    assert!(!avisa(2027, 3));
    // Con el 349 ya mensual en el perfil no hay nada que avisar.
    v.set_setting(
        "taxpayer_profile",
        &TaxpayerProfile {
            intracomunitarias: Some(Periodicity::Mensual),
            ..TaxpayerProfile::autonomo()
        },
    )
    .unwrap();
    assert!(!avisa(2026, 2));
}

/// Alta de un elemento del inmovilizado comprado en 2023 por `cost`; devuelve su id.
fn new_asset(v: &Vault, name: &str, account: &str, cost: i64, d: Date) -> i64 {
    use taxpain_app::accounting::add_fixed_asset;
    use taxpain_core::accounting::depreciation::FixedAsset;
    let a = FixedAsset {
        id: 0,
        name: name.into(),
        account: acc(account),
        depreciation_account: acc(&format!("28{}", &account[1..])),
        expense_account: acc("681"),
        acquired_on: date!(2023 - 01 - 10),
        cost: eur(cost),
        residual: Money::ZERO,
        rate: Rate::percent(10),
        disposed_on: None,
        method: Default::default(),
        tax_rate: None,
    };
    add_fixed_asset(v, &a, at(d)).unwrap()
}

/// Con la venta vinculada a su factura en Contabilidad › Inmovilizado, el 390 toma esa factura
/// sin buscarla, aunque la línea no nombre el elemento, y resta lo que queda de la venta con su
/// rectificativa. Una venta sin vincular se sigue buscando, y el borrador avisa de que en la
/// contabilidad cuenta dos veces.
#[test]
fn venta_de_inmovilizado_vinculada_a_su_factura() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    let v = setup();
    // Torno (213, 12.000): factura de 3.000 + IVA con una línea que no lo nombra (A2026-0001),
    // vinculada al darlo de baja.
    let torno = new_asset(&v, "Torno", "213", 12_000, date!(2026 - 05 - 01));
    let f = issue(
        &v,
        &sale_draft("Maquinaria usada", 3000, iva21(), date!(2026 - 05 - 12)),
        at(date!(2026 - 05 - 12)),
        true,
    )
    .unwrap();
    dispose_asset_with_invoice(
        &v,
        torno,
        date!(2026 - 05 - 10),
        eur(3000),
        Some(f.id),
        at(date!(2026 - 05 - 12)),
    )
    .unwrap();
    // Rectificativa de −500 (R2026-0001): la factura vende el torno entero y se vincula sola.
    rectify_sale(
        &v,
        (&f.number, date!(2026 - 05 - 12)),
        "Rebaja del precio",
        -500,
        date!(2026 - 06 - 01),
    );
    // Remolque (218, 9.000) vendido por 4.000 con su factura, sin vincular (A2026-0002).
    sell_asset(&v, "Remolque", "218", 9000, 4000, date!(2026 - 09 - 01));
    emit_sale(
        &v,
        "Venta del remolque",
        4000,
        iva21(),
        date!(2026 - 09 - 02),
    );

    let d = models::draft(&v, "390", 2026, 0).unwrap();
    // [99]: 3.000 − 500 + 4.000; [107]: lo mismo, porque todo son ventas de bienes de inversión.
    assert_eq!(get(&d, 99), eur(6500));
    assert_eq!(get(&d, 107), eur(6500));
    assert_eq!(get(&d, 108), Money::ZERO);
    assert_eq!(
        d.details,
        vec![
            (
                "[107] Venta de Torno (213) del 10/05/2026 · factura A2026-0001".to_string(),
                eur(2500)
            ),
            (
                "[107] Venta de Remolque (218) del 01/09/2026 · factura A2026-0002".to_string(),
                eur(4000)
            ),
        ]
    );
    // Del torno sólo se dice lo que queda con su rectificativa: está vinculado, así que ni se
    // pide comprobar la línea ni vincularlo. Del remolque, que se buscó, que conviene vincularlo.
    let torno: Vec<_> = d.warnings.iter().filter(|w| w.contains("Torno")).collect();
    assert_eq!(torno.len(), 1, "{:?}", d.warnings);
    assert!(
        torno[0].starts_with("Venta de Torno del 10/05/2026 por 3.000,00 €")
            && torno[0].contains("R2026-0001")
            && torno[0].contains("quedan 2.500,00 €"),
        "{}",
        torno[0]
    );
    let sin_vincular: Vec<_> = d
        .warnings
        .iter()
        .filter(|w| w.starts_with("Ventas del inmovilizado sin vincular"))
        .collect();
    assert_eq!(sin_vincular.len(), 1, "{:?}", d.warnings);
    assert!(
        sin_vincular[0].contains("Remolque (A2026-0002)")
            && sin_vincular[0].contains("volumen de operaciones del 390")
            && sin_vincular[0].contains("cuenta dos veces")
            && sin_vincular[0].contains("Contabilidad › Inmovilizado"),
        "{}",
        sin_vincular[0]
    );
}

/// La venta vinculada se resta en el año en que devenga su factura: la baja del 20/12/2026 con su
/// factura del 11/01/2027 (sin fecha de operación) se resta en el 390 de 2027, y el de 2026 lo
/// dice.
#[test]
fn venta_vinculada_con_la_factura_del_año_siguiente() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    use taxpain_app::venta_inmovilizado::link_sale_invoice;
    let v = setup();
    let torno = new_asset(&v, "Torno", "213", 12_000, date!(2026 - 12 - 01));
    dispose_asset_with_invoice(
        &v,
        torno,
        date!(2026 - 12 - 20),
        eur(3000),
        None,
        at(date!(2026 - 12 - 20)),
    )
    .unwrap();
    let f = issue(
        &v,
        &sale_draft("Venta del torno", 3000, iva21(), date!(2027 - 01 - 11)),
        at(date!(2027 - 01 - 11)),
        true,
    )
    .unwrap();
    link_sale_invoice(&v, torno, f.id, None, at(date!(2027 - 01 - 11))).unwrap();

    let d26 = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d26, 107), Money::ZERO);
    assert!(
        d26.warnings.iter().any(|w| w.starts_with("Venta de Torno")
            && w.contains("devenga en 2027")
            && !w.contains("vincula")),
        "{:?}",
        d26.warnings
    );
    let d27 = models::draft(&v, "390", 2027, 0).unwrap();
    assert_eq!(get(&d27, 99), eur(3000));
    assert_eq!(get(&d27, 107), eur(3000));
    assert_eq!(get(&d27, 108), Money::ZERO);
}

/// Factura a Cliente SL con una línea por cada (concepto, base, IVA).
fn sale_lines(lines: &[(&str, i64, VatTreatment)], d: Date) -> InvoiceDraft {
    let mut f = sale_draft("Venta", 0, iva21(), d);
    f.lines = lines
        .iter()
        .map(|(what, base, vat)| InvoiceLine {
            description: (*what).into(),
            quantity_milli: 1000,
            unit_price: eur(*base),
            discount: Rate::ZERO,
            vat: *vat,
            surcharge: None,
            withholding: None,
            advance: false,
        })
        .collect();
    f
}

/// Una rectificativa que anula la venta vinculada no se pasa sola a la venta (TaxPain no deshace
/// la baja), pero el 390 la cuenta como sin el vínculo: de la venta no queda nada en el volumen,
/// no se resta y [108] no queda en negativo. Si la anulación es del año siguiente, allí se
/// compensa.
#[test]
fn venta_vinculada_anulada_por_su_rectificativa() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    use taxpain_app::venta_inmovilizado::asset_sales;
    // Torno (213, 12.000) vendido el 10/05/2026 por 3.000 con A2026-0001, vinculada al darlo de
    // baja, y devuelto el día `back` con una rectificativa de −3.000, que queda pendiente.
    let sold = |back: Date| {
        let v = setup();
        let torno = new_asset(&v, "Torno", "213", 12_000, date!(2026 - 05 - 01));
        let d = date!(2026 - 05 - 10);
        let f = issue(
            &v,
            &sale_draft("Venta del torno", 3000, iva21(), d),
            at(d),
            true,
        )
        .unwrap();
        dispose_asset_with_invoice(&v, torno, d, eur(3000), Some(f.id), at(d)).unwrap();
        rectify_sale(&v, (&f.number, d), "Devolución del torno", -3000, back);
        let pending = &asset_sales(&v).unwrap()[0].pending;
        assert!(pending.len() == 1 && pending[0].cancels_sale, "{pending:?}");
        v
    };

    let v = sold(date!(2026 - 06 - 01));
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), Money::ZERO);
    assert_eq!(get(&d, 107), Money::ZERO);
    assert_eq!(get(&d, 108), Money::ZERO);
    assert!(d.details.is_empty(), "{:?}", d.details);
    let torno: Vec<_> = d
        .warnings
        .iter()
        .filter(|w| w.starts_with("Venta de Torno"))
        .collect();
    assert!(
        torno.len() == 1 && torno[0].contains("R2026-0001") && torno[0].contains("quedan 0,00 €"),
        "{:?}",
        d.warnings
    );
    // Exonerado del 390: la página 04 del 303 del 4T tampoco la resta y [88] no queda en negativo.
    models::save_settings(
        &v,
        &ModelSettings {
            exempt_from_390: true,
            ..ModelSettings::default()
        },
        datetime!(2026-12-30 10:00 UTC),
    )
    .unwrap();
    let q4 = models::draft(&v, "303", 2026, 4).unwrap();
    assert_eq!(get(&q4, 99), Money::ZERO);
    assert_eq!(get(&q4, 88), Money::ZERO);

    // Devuelto el 15/02/2027: en 2026 se resta entero y en 2027 se compensa la devolución.
    let v = sold(date!(2027 - 02 - 15));
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), eur(3000));
    assert_eq!(get(&d, 107), eur(3000));
    assert_eq!(get(&d, 108), Money::ZERO);
    let d = models::draft(&v, "390", 2027, 0).unwrap();
    assert_eq!(get(&d, 99), eur(-3000));
    assert_eq!(get(&d, 107), eur(-3000));
    assert_eq!(get(&d, 108), Money::ZERO);
    assert_eq!(
        d.details,
        vec![(
            "[107] Venta de Torno (213) del 10/05/2026 · rectificativa R2027-0001".to_string(),
            eur(-3000)
        )]
    );
}

/// La rectificativa pendiente de una factura que vende el elemento y algo más (no se pasa sola a
/// la venta): el 390 resta lo que queda de la venta, como sin el vínculo.
#[test]
fn venta_vinculada_con_la_rectificativa_de_una_factura_de_dos_líneas() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    use taxpain_app::venta_inmovilizado::asset_sales;
    let v = setup();
    // Furgoneta (218, 20.000) vendida el 10/03/2026 por 10.000 con A2026-0001, que también cobra
    // 500 de transporte; R2026-0001 rebaja 1.000 el 15/04.
    let furgoneta = new_asset(&v, "Furgoneta", "218", 20_000, date!(2026 - 03 - 01));
    let d0 = date!(2026 - 03 - 10);
    let f = issue(
        &v,
        &sale_lines(
            &[("Furgoneta", 10_000, iva21()), ("Transporte", 500, iva21())],
            d0,
        ),
        at(d0),
        true,
    )
    .unwrap();
    dispose_asset_with_invoice(&v, furgoneta, d0, eur(10_000), Some(f.id), at(d0)).unwrap();
    rectify_sale(
        &v,
        (&f.number, d0),
        "Descuento",
        -1000,
        date!(2026 - 04 - 15),
    );
    assert_eq!(asset_sales(&v).unwrap()[0].pending.len(), 1);

    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), eur(9500));
    assert_eq!(get(&d, 107), eur(9000));
    assert_eq!(get(&d, 108), eur(500));
    assert_eq!(
        d.details,
        vec![(
            "[107] Venta de Furgoneta (218) del 10/03/2026 · factura A2026-0001".to_string(),
            eur(9000)
        )]
    );
}

/// La rectificativa del año siguiente de una venta vinculada se compensa en [107] de ese año,
/// aunque no esté pasada a la venta.
#[test]
fn venta_vinculada_con_la_rectificativa_del_año_siguiente() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    let v = setup();
    // Prensa (213, 8.000) vendida el 10/10/2026 por 5.000 con A2026-0001, que también cobra 300
    // de montaje; R2027-0001 rebaja 1.000 el 15/02/2027.
    let prensa = new_asset(&v, "Prensa", "213", 8000, date!(2026 - 10 - 01));
    let d0 = date!(2026 - 10 - 10);
    let f = issue(
        &v,
        &sale_lines(&[("Prensa", 5000, iva21()), ("Montaje", 300, iva21())], d0),
        at(d0),
        true,
    )
    .unwrap();
    dispose_asset_with_invoice(&v, prensa, d0, eur(5000), Some(f.id), at(d0)).unwrap();
    rectify_sale(
        &v,
        (&f.number, d0),
        "Descuento",
        -1000,
        date!(2027 - 02 - 15),
    );

    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), eur(5300));
    assert_eq!(get(&d, 107), eur(5000));
    assert_eq!(get(&d, 108), eur(300));
    let d = models::draft(&v, "390", 2027, 0).unwrap();
    assert_eq!(get(&d, 99), eur(-1000));
    assert_eq!(get(&d, 107), eur(-1000));
    assert_eq!(get(&d, 108), Money::ZERO);
    assert_eq!(
        d.details,
        vec![(
            "[107] Venta de Prensa (213) del 10/10/2026 · rectificativa R2027-0001".to_string(),
            eur(-1000)
        )]
    );
    assert!(
        d.warnings.iter().any(|w| w.starts_with("Venta de Prensa")
            && w.contains("A2026-0001")
            && w.contains("R2027-0001")),
        "{:?}",
        d.warnings
    );
}

/// [106] y [107] son de entregas sujetas: la venta vinculada cuya línea es no sujeta no se resta,
/// aunque su factura lleve otra línea sujeta.
#[test]
fn venta_vinculada_no_sujeta_con_otra_línea_sujeta() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    use taxpain_core::invoice::NotSubject;
    let v = setup();
    // A2026-0001 (30/06/2026): maquinaria transmitida con una rama de actividad (art. 7.1.º LIVA,
    // no sujeta) y una furgoneta al 21 %, las dos dadas de baja con ella.
    let maquinaria = new_asset(&v, "Maquinaria", "213", 60_000, date!(2026 - 06 - 01));
    let furgoneta = new_asset(&v, "Furgoneta", "218", 20_000, date!(2026 - 06 - 01));
    let d0 = date!(2026 - 06 - 30);
    let f = issue(
        &v,
        &sale_lines(
            &[
                (
                    "Maquinaria (rama de actividad, art. 7.1.º)",
                    40_000,
                    VatTreatment::NotSubject(NotSubject::N1),
                ),
                ("Furgoneta", 10_000, iva21()),
            ],
            d0,
        ),
        at(d0),
        true,
    )
    .unwrap();
    for (id, price) in [(maquinaria, 40_000), (furgoneta, 10_000)] {
        dispose_asset_with_invoice(&v, id, d0, eur(price), Some(f.id), at(d0)).unwrap();
    }
    // Grúa (213, 20.000) vendida en Portugal el 01/08/2026 (no sujeta por localización, en [110])
    // con A2026-0002, que también cobra 500 de desmontaje al 21 %.
    let grua = new_asset(&v, "Grúa", "213", 20_000, date!(2026 - 07 - 01));
    let d1 = date!(2026 - 08 - 01);
    let g = issue(
        &v,
        &sale_lines(
            &[
                (
                    "Venta de la grúa",
                    7000,
                    VatTreatment::NotSubject(NotSubject::N2),
                ),
                ("Desmontaje", 500, iva21()),
            ],
            d1,
        ),
        at(d1),
        true,
    )
    .unwrap();
    dispose_asset_with_invoice(&v, grua, d1, eur(7000), Some(g.id), at(d1)).unwrap();

    let d = models::draft(&v, "390", 2026, 0).unwrap();
    // [99]: la furgoneta y el desmontaje; sólo se resta la furgoneta.
    assert_eq!(get(&d, 99), eur(10_500));
    assert_eq!(get(&d, 110), eur(7000));
    assert_eq!(get(&d, 107), eur(10_000));
    assert_eq!(get(&d, 108), eur(7500));
    assert_eq!(
        d.details,
        vec![(
            "[107] Venta de Furgoneta (218) del 30/06/2026 · factura A2026-0001".to_string(),
            eur(10_000)
        )]
    );
    assert!(
        !d.warnings
            .iter()
            .any(|w| w.starts_with("Venta de Maquinaria") || w.starts_with("Venta de Grúa")),
        "{:?}",
        d.warnings
    );
}

/// Dada de baja sin precio y vinculada después, la venta se lleva toda la factura, también el
/// suplido no sujeto: se resta sólo la línea de la furgoneta.
#[test]
fn venta_sin_precio_vinculada_después_con_un_suplido() {
    use taxpain_app::accounting::dispose_asset;
    use taxpain_app::venta_inmovilizado::link_sale_invoice;
    use taxpain_core::invoice::NotSubject;
    let v = setup();
    let furgoneta = new_asset(&v, "Furgoneta", "218", 20_000, date!(2026 - 03 - 01));
    dispose_asset(
        &v,
        furgoneta,
        date!(2026 - 03 - 10),
        Money::ZERO,
        at(date!(2026 - 03 - 10)),
    )
    .unwrap();
    let d0 = date!(2026 - 03 - 12);
    let f = issue(
        &v,
        &sale_lines(
            &[
                ("Furgoneta", 10_000, iva21()),
                (
                    "Tasa DGT (suplido)",
                    150,
                    VatTreatment::NotSubject(NotSubject::N1),
                ),
            ],
            d0,
        ),
        at(d0),
        true,
    )
    .unwrap();
    let l = link_sale_invoice(&v, furgoneta, f.id, None, at(d0)).unwrap();
    assert_eq!(l.amount, eur(10_150));

    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), eur(10_000));
    assert_eq!(get(&d, 107), eur(10_000));
    assert_eq!(get(&d, 108), Money::ZERO);
    assert_eq!(
        d.details,
        vec![(
            "[107] Venta de Furgoneta (218) del 10/03/2026 · factura A2026-0001".to_string(),
            eur(10_000)
        )]
    );
}

/// Una factura vinculada a una venta sigue valiendo para las demás que lleva: la línea del
/// remolque, sin vincular, se resta; la de la silla, vinculada aunque no es un bien de inversión,
/// no la toma otra venta.
#[test]
fn factura_vinculada_que_vende_otro_elemento_sin_vincular() {
    use taxpain_app::accounting::{dispose_asset, dispose_asset_with_invoice};
    let v = setup();
    let torno = new_asset(&v, "Torno", "213", 12_000, date!(2026 - 05 - 01));
    let silla = new_asset(&v, "Silla", "216", 2000, date!(2026 - 05 - 01));
    let remolque = new_asset(&v, "Remolque", "218", 9000, date!(2026 - 05 - 01));
    let taladro = new_asset(&v, "Taladro", "213", 4000, date!(2026 - 05 - 01));
    // A2026-0001: torno 3.000, remolque 4.000 y silla 500. El torno y la silla se dan de baja con
    // ella; el remolque, por 4.000 sin elegir la factura; el taladro, por 500 sin factura.
    let d0 = date!(2026 - 05 - 10);
    let f = issue(
        &v,
        &sale_lines(
            &[
                ("Torno", 3000, iva21()),
                ("Remolque", 4000, iva21()),
                ("Silla", 500, iva21()),
            ],
            d0,
        ),
        at(d0),
        true,
    )
    .unwrap();
    dispose_asset_with_invoice(&v, torno, d0, eur(3000), Some(f.id), at(d0)).unwrap();
    dispose_asset_with_invoice(&v, silla, d0, eur(500), Some(f.id), at(d0)).unwrap();
    dispose_asset(&v, remolque, d0, eur(4000), at(d0)).unwrap();
    dispose_asset(&v, taladro, d0, eur(500), at(d0)).unwrap();

    let d = models::draft(&v, "390", 2026, 0).unwrap();
    // [107]: el torno y el remolque; la silla sigue en el volumen.
    assert_eq!(get(&d, 99), eur(7500));
    assert_eq!(get(&d, 107), eur(7000));
    assert_eq!(get(&d, 108), eur(500));
    assert_eq!(
        d.details,
        vec![
            (
                "[107] Venta de Torno (213) del 10/05/2026 · factura A2026-0001".to_string(),
                eur(3000)
            ),
            (
                "[107] Venta de Remolque (218) del 10/05/2026 · factura A2026-0001".to_string(),
                eur(4000)
            ),
        ]
    );
    assert!(
        !d.warnings.iter().any(|w| w.contains("ninguna factura")),
        "{:?}",
        d.warnings
    );
    let taladro: Vec<_> = d
        .warnings
        .iter()
        .filter(|w| w.starts_with("Venta de Taladro"))
        .collect();
    assert!(
        taladro.len() == 1 && taladro[0].contains("ya es la venta de Silla"),
        "{:?}",
        d.warnings
    );
    let sin_vincular: Vec<_> = d
        .warnings
        .iter()
        .filter(|w| w.starts_with("Ventas del inmovilizado sin vincular"))
        .collect();
    assert!(
        sin_vincular.len() == 1
            && sin_vincular[0].contains("Remolque (A2026-0001)")
            && !sin_vincular[0].contains("Torno")
            && !sin_vincular[0].contains("Silla"),
        "{:?}",
        d.warnings
    );
}

/// Vinculada a una factura del año siguiente sin la fecha de la operación, el 390 del año de la
/// entrega dice que la factura debía llevarla y devengar en ese año.
#[test]
fn venta_vinculada_facturada_el_año_siguiente_sin_fecha_de_operación() {
    use taxpain_app::accounting::dispose_asset;
    use taxpain_app::venta_inmovilizado::link_sale_invoice;
    let v = setup();
    // Fresadora (213, 9.000) entregada el 28/12/2026 por 8.000 y facturada el 08/01/2027 sin fecha
    // de operación (A2027-0001), vinculada después.
    let fresadora = new_asset(&v, "Fresadora", "213", 9000, date!(2026 - 12 - 01));
    dispose_asset(
        &v,
        fresadora,
        date!(2026 - 12 - 28),
        eur(8000),
        at(date!(2026 - 12 - 28)),
    )
    .unwrap();
    let d0 = date!(2027 - 01 - 08);
    let f = issue(
        &v,
        &sale_draft("Venta de la fresadora", 8000, iva21(), d0),
        at(d0),
        true,
    )
    .unwrap();
    link_sale_invoice(&v, fresadora, f.id, None, at(d0)).unwrap();

    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 107), Money::ZERO);
    let w: Vec<_> = d
        .warnings
        .iter()
        .filter(|w| w.starts_with("Venta de Fresadora"))
        .collect();
    assert_eq!(w.len(), 1, "{:?}", d.warnings);
    assert!(
        w[0].contains("A2027-0001 devenga en 2027")
            && w[0].contains("Si la entrega fue el 28/12/2026")
            && w[0].contains("(art. 6.1.g RD 1619/2012)")
            && w[0].contains("devengar en 2026 (art. 75 LIVA)"),
        "{}",
        w[0]
    );
    let d = models::draft(&v, "390", 2027, 0).unwrap();
    assert_eq!(get(&d, 99), eur(8000));
    assert_eq!(get(&d, 107), eur(8000));
    assert_eq!(get(&d, 108), Money::ZERO);
}

/// Vinculada a una factura no sujeta del año siguiente, la venta no se resta en ningún año, y el
/// 390 de la entrega no dice que se resta en el siguiente.
#[test]
fn venta_vinculada_no_sujeta_facturada_el_año_siguiente() {
    use taxpain_app::accounting::dispose_asset;
    use taxpain_app::venta_inmovilizado::link_sale_invoice;
    use taxpain_core::invoice::NotSubject;
    let v = setup();
    // Grúa (213, 20.000) entregada en Portugal el 20/12/2026 por 7.000 y facturada como no sujeta
    // el 11/01/2027 (A2027-0001), vinculada después.
    let grua = new_asset(&v, "Grúa", "213", 20_000, date!(2026 - 12 - 01));
    dispose_asset(
        &v,
        grua,
        date!(2026 - 12 - 20),
        eur(7000),
        at(date!(2026 - 12 - 20)),
    )
    .unwrap();
    let d0 = date!(2027 - 01 - 11);
    let f = issue(
        &v,
        &sale_draft(
            "Venta de la grúa",
            7000,
            VatTreatment::NotSubject(NotSubject::N2),
            d0,
        ),
        at(d0),
        true,
    )
    .unwrap();
    link_sale_invoice(&v, grua, f.id, None, at(d0)).unwrap();

    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 107), Money::ZERO);
    assert!(
        !d.warnings.iter().any(|w| w.starts_with("Venta de Grúa")),
        "{:?}",
        d.warnings
    );
    let d = models::draft(&v, "390", 2027, 0).unwrap();
    assert_eq!(get(&d, 110), eur(7000));
    assert_eq!(get(&d, 107), Money::ZERO);
    assert_eq!(get(&d, 108), eur(7000));
    assert!(d.details.is_empty(), "{:?}", d.details);
    assert!(
        !d.warnings.iter().any(|w| w.starts_with("Venta de Grúa")),
        "{:?}",
        d.warnings
    );
}

/// Una venta restada con una factura importada del libro de la gestoría no sale en el aviso de
/// vincular: TaxPain no la puede vincular y, sin su asiento, no cuenta dos veces.
#[test]
fn venta_con_la_factura_importada_no_pide_vincularla() {
    use taxpain_app::libros;
    let v = setup();
    // Furgoneta (218, 20.000) dada de baja en TaxPain el 10/05/2026 por 10.000; su factura viene
    // del libro de emitidas de la gestoría.
    sell_asset(
        &v,
        "Furgoneta",
        "218",
        20_000,
        10_000,
        date!(2026 - 05 - 10),
    );
    let csv = "Fecha Expedición;Número Factura;NIF Destinatario;Nombre Destinatario;Base Imponible;Tipo IVA;Cuota IVA;Total\n\
               12/05/2026;2026-001;B12345674;Cliente SL;10.000,00;21;2.100,00;12.100,00\n";
    let book = libros::parse("libro_emitidas.csv", csv.as_bytes())
        .unwrap()
        .unwrap();
    let imported = libros::import_issued(&v, &book, at(date!(2026 - 06 - 01))).unwrap();
    assert_eq!(imported.added, 1);
    // Torno (213, 12.000) vendido por 3.000 con su factura de TaxPain (A2026-0001), sin vincular.
    sell_asset(&v, "Torno", "213", 12_000, 3000, date!(2026 - 06 - 10));
    emit_sale(&v, "Venta del torno", 3000, iva21(), date!(2026 - 06 - 10));

    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), eur(13_000));
    assert_eq!(get(&d, 107), eur(13_000));
    assert_eq!(get(&d, 108), Money::ZERO);
    // La línea importada no nombra la furgoneta: se pide comprobarla, no vincularla.
    assert!(
        d.warnings
            .iter()
            .any(|w| w.starts_with("Venta de Furgoneta") && w.contains("comprueba")),
        "{:?}",
        d.warnings
    );
    let sin_vincular: Vec<_> = d
        .warnings
        .iter()
        .filter(|w| w.starts_with("Ventas del inmovilizado sin vincular"))
        .collect();
    assert!(
        sin_vincular.len() == 1
            && sin_vincular[0].contains("Torno (A2026-0001)")
            && !sin_vincular[0].contains("Furgoneta"),
        "{:?}",
        d.warnings
    );
}

/// La rectificativa de una factura que vende dos elementos cuenta una vez en [107]: si no dice qué
/// parte es de cada venta, se reparte entre ellas y se avisa; si se ha pasado a cada venta su
/// parte, con esa parte. Igual con las ventas sin vincular, y en el año siguiente, donde se
/// compensa entera.
#[test]
fn rectificativa_de_una_factura_que_vende_dos_elementos() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    use taxpain_app::venta_inmovilizado::link_sale_invoice;
    // A2026-0001 (10/05/2026): torno 3.000 y fresadora 5.000. Se dan de baja con ella las
    // `linked` primeras (el torno y luego la fresadora) y las demás por su precio, sin elegir la
    // factura. La rectificativa de `amount` es del día `back` y queda pendiente.
    let sold = |linked: usize, amount: i64, back: Date| {
        let v = setup();
        let torno = new_asset(&v, "Torno", "213", 12_000, date!(2026 - 05 - 01));
        let fresadora = new_asset(&v, "Fresadora", "213", 6000, date!(2026 - 05 - 01));
        let d0 = date!(2026 - 05 - 10);
        let f = issue(
            &v,
            &sale_lines(
                &[
                    ("Torno usado", 3000, iva21()),
                    ("Fresadora usada", 5000, iva21()),
                ],
                d0,
            ),
            at(d0),
            true,
        )
        .unwrap();
        for (n, (id, price)) in [(torno, 3000), (fresadora, 5000)].into_iter().enumerate() {
            let invoice = (n < linked).then_some(f.id);
            dispose_asset_with_invoice(&v, id, d0, eur(price), invoice, at(d0)).unwrap();
        }
        let r = rectify_sale(&v, (&f.number, d0), "Descuento", amount, back);
        (v, [torno, fresadora], r.id)
    };
    let m390 = |v: &Vault, year: i32| {
        let d = models::draft(v, "390", year, 0).unwrap();
        ((get(&d, 99), get(&d, 107), get(&d, 108)), d)
    };
    let detail = |name: &str, what: &str, m: i64| {
        (
            format!("[107] Venta de {name} (213) del 10/05/2026 · {what}"),
            eur(m),
        )
    };
    let spread = |d: &models::Draft, number: &str| {
        d.warnings
            .iter()
            .filter(|w| {
                w.starts_with(&format!("La rectificativa {number} de A2026-0001"))
                    && w.contains("Torno y Fresadora")
                    && w.contains("no dice qué parte es de cada venta")
            })
            .count()
    };

    // Las dos vinculadas y 800 de descuento sin repartir: 300 del torno y 500 de la fresadora.
    let (v, [torno, fresadora], r) = sold(2, -800, date!(2026 - 06 - 01));
    let (boxes, d) = m390(&v, 2026);
    assert_eq!(boxes, (eur(7200), eur(7200), Money::ZERO));
    assert_eq!(
        d.details,
        vec![
            detail("Torno", "factura A2026-0001", 2700),
            detail("Fresadora", "factura A2026-0001", 4500),
        ]
    );
    assert_eq!(spread(&d, "R2026-0001"), 1, "{:?}", d.warnings);
    // Pasada a mano a cada venta su parte (600 y 200), cuenta esa parte.
    let now = at(date!(2026 - 06 - 02));
    link_sale_invoice(&v, torno, r, Some(-eur(600)), now).unwrap();
    link_sale_invoice(&v, fresadora, r, Some(-eur(200)), now).unwrap();
    let (boxes, d) = m390(&v, 2026);
    assert_eq!(boxes, (eur(7200), eur(7200), Money::ZERO));
    assert_eq!(
        d.details,
        vec![
            detail("Torno", "factura A2026-0001", 2400),
            detail("Fresadora", "factura A2026-0001", 4800),
        ]
    );
    assert_eq!(spread(&d, "R2026-0001"), 0, "{:?}", d.warnings);

    // 4.000 de descuento, con las dos vinculadas, sólo el torno o ninguna.
    for linked in [2, 1, 0] {
        let (v, _, _) = sold(linked, -4000, date!(2026 - 06 - 01));
        let (boxes, d) = m390(&v, 2026);
        assert_eq!(boxes, (eur(4000), eur(4000), Money::ZERO), "{linked}");
        assert_eq!(
            d.details,
            vec![
                detail("Torno", "factura A2026-0001", 1500),
                detail("Fresadora", "factura A2026-0001", 2500),
            ],
            "{linked}"
        );
        assert_eq!(spread(&d, "R2026-0001"), 1, "{linked}: {:?}", d.warnings);
    }
    // Y si es de 2027: en 2026 se restan las dos ventas enteras y en 2027 se compensan los 4.000.
    for linked in [2, 1, 0] {
        let (v, _, _) = sold(linked, -4000, date!(2027 - 02 - 15));
        let (boxes, _) = m390(&v, 2026);
        assert_eq!(boxes, (eur(8000), eur(8000), Money::ZERO), "{linked}");
        let (boxes, d) = m390(&v, 2027);
        assert_eq!(boxes, (eur(-4000), eur(-4000), Money::ZERO), "{linked}");
        assert_eq!(
            d.details,
            vec![
                detail("Torno", "rectificativa R2027-0001", -1500),
                detail("Fresadora", "rectificativa R2027-0001", -2500),
            ],
            "{linked}"
        );
        assert_eq!(spread(&d, "R2027-0001"), 1, "{linked}: {:?}", d.warnings);
    }
}

/// La rectificativa pasada en parte a la venta vinculada (el resto es de otra línea de la
/// factura) le quita sólo esa parte, como en la contabilidad.
#[test]
fn rectificativa_pasada_en_parte_a_la_venta() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    use taxpain_app::venta_inmovilizado::link_sale_invoice;
    let v = setup();
    // Furgoneta (218, 20.000) vendida el 10/03/2026 por 10.000 con A2026-0001, que también cobra
    // 1.000 de transporte; R2026-0001 rebaja 1.000 el 15/04, y 300 son de la furgoneta.
    let furgoneta = new_asset(&v, "Furgoneta", "218", 20_000, date!(2026 - 03 - 01));
    let d0 = date!(2026 - 03 - 10);
    let f = issue(
        &v,
        &sale_lines(
            &[
                ("Furgoneta", 10_000, iva21()),
                ("Transporte", 1000, iva21()),
            ],
            d0,
        ),
        at(d0),
        true,
    )
    .unwrap();
    dispose_asset_with_invoice(&v, furgoneta, d0, eur(10_000), Some(f.id), at(d0)).unwrap();
    let d1 = date!(2026 - 04 - 15);
    let r = rectify_sale(&v, (&f.number, d0), "Descuento", -1000, d1);
    link_sale_invoice(&v, furgoneta, r.id, Some(-eur(300)), at(d1)).unwrap();

    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), eur(10_000));
    assert_eq!(get(&d, 107), eur(9700));
    assert_eq!(get(&d, 108), eur(300));
    assert_eq!(
        d.details,
        vec![(
            "[107] Venta de Furgoneta (218) del 10/03/2026 · factura A2026-0001".to_string(),
            eur(9700)
        )]
    );
    assert!(
        d.warnings
            .iter()
            .any(|w| w.starts_with("Venta de Furgoneta")
                && w.contains("R2026-0001")
                && w.contains("quedan 9.700,00 €")),
        "{:?}",
        d.warnings
    );
}

/// Lo que la rectificativa pasa a la venta cuenta también si la sube, y no si es de una línea no
/// sujeta, que no está en el volumen.
#[test]
fn rectificativa_pasada_a_la_venta_que_la_sube_o_no_es_sujeta() {
    use taxpain_app::accounting::{dispose_asset, dispose_asset_with_invoice};
    use taxpain_app::venta_inmovilizado::link_sale_invoice;
    use taxpain_core::invoice::{NotSubject, Rectification, RectificationMethod};
    let v = setup();
    // Torno (213, 12.000) vendido el 10/05/2026 por 3.000 con A2026-0001, que lo vende entero:
    // R2026-0001 sube 500 el precio y se pasa sola a la venta.
    let torno = new_asset(&v, "Torno", "213", 12_000, date!(2026 - 05 - 01));
    let d0 = date!(2026 - 05 - 10);
    let f = issue(
        &v,
        &sale_draft("Venta del torno", 3000, iva21(), d0),
        at(d0),
        true,
    )
    .unwrap();
    dispose_asset_with_invoice(&v, torno, d0, eur(3000), Some(f.id), at(d0)).unwrap();
    rectify_sale(
        &v,
        (&f.number, d0),
        "Más precio",
        500,
        date!(2026 - 06 - 01),
    );
    // Furgoneta (218, 20.000) dada de baja sin precio y vinculada a A2026-0002 (furgoneta 10.000
    // y la tasa de la DGT, un suplido no sujeto, 150); R2026-0002 devuelve la tasa y también se
    // pasa sola a la venta.
    let furgoneta = new_asset(&v, "Furgoneta", "218", 20_000, date!(2026 - 07 - 01));
    let d1 = date!(2026 - 07 - 10);
    dispose_asset(&v, furgoneta, d1, Money::ZERO, at(d1)).unwrap();
    let g = issue(
        &v,
        &sale_lines(
            &[
                ("Furgoneta", 10_000, iva21()),
                (
                    "Tasa DGT (suplido)",
                    150,
                    VatTreatment::NotSubject(NotSubject::N1),
                ),
            ],
            d1,
        ),
        at(d1),
        true,
    )
    .unwrap();
    link_sale_invoice(&v, furgoneta, g.id, None, at(d1)).unwrap();
    let d2 = date!(2026 - 07 - 20);
    let mut r = sale_draft(
        "Devolución de la tasa",
        -150,
        VatTreatment::NotSubject(NotSubject::N1),
        d2,
    );
    r.kind = InvoiceKind::Rectificativa(Rectification {
        code: 1,
        method: RectificationMethod::Diferencias,
    });
    r.rectified = vec![(g.number.clone(), d1)];
    issue(&v, &r, at(d2), true).unwrap();
    let sales = taxpain_app::venta_inmovilizado::asset_sales(&v).unwrap();
    assert!(
        sales
            .iter()
            .all(|s| s.invoices.len() == 2 && s.pending.is_empty()),
        "{sales:?}"
    );

    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), eur(3500 + 10_000));
    assert_eq!(get(&d, 107), eur(3500 + 10_000));
    assert_eq!(get(&d, 108), Money::ZERO);
    assert_eq!(
        d.details,
        vec![
            (
                "[107] Venta de Torno (213) del 10/05/2026 · factura A2026-0001".to_string(),
                eur(3500)
            ),
            (
                "[107] Venta de Furgoneta (218) del 10/07/2026 · factura A2026-0002".to_string(),
                eur(10_000)
            ),
        ]
    );
}

/// Dada de baja sin precio y vinculada a toda una factura de dos líneas que ni la nombran ni
/// tienen esa base: se resta con la línea sujeta mayor y se pide comprobar que es la suya.
#[test]
fn venta_vinculada_a_toda_una_factura_que_no_la_nombra() {
    use taxpain_app::accounting::dispose_asset;
    use taxpain_app::venta_inmovilizado::link_sale_invoice;
    use taxpain_core::invoice::NotSubject;
    // Furgoneta (218, 20.000) dada de baja sin precio el 10/03/2026 y vinculada después, por todo
    // lo que lleva a ventas, a A2026-0001 (12/03): «Vehículo usado 1234-ABC» por 10.000 al 21 % y
    // otra línea, `other`.
    let linked = |other: (&str, i64, VatTreatment)| {
        let v = setup();
        let furgoneta = new_asset(&v, "Furgoneta", "218", 20_000, date!(2026 - 03 - 01));
        let d0 = date!(2026 - 03 - 10);
        dispose_asset(&v, furgoneta, d0, Money::ZERO, at(d0)).unwrap();
        let d1 = date!(2026 - 03 - 12);
        let f = issue(
            &v,
            &sale_lines(&[("Vehículo usado 1234-ABC", 10_000, iva21()), other], d1),
            at(d1),
            true,
        )
        .unwrap();
        let l = link_sale_invoice(&v, furgoneta, f.id, None, at(d1)).unwrap();
        assert_eq!(l.amount, eur(10_000 + other.1));
        models::draft(&v, "390", 2026, 0).unwrap()
    };
    let check = |d: &models::Draft, line: &str| {
        let w: Vec<_> = d
            .warnings
            .iter()
            .filter(|w| w.starts_with("Venta de Furgoneta"))
            .collect();
        assert!(
            w.len() == 1 && w[0].contains(&format!("«{line}»")) && w[0].contains("comprueba"),
            "{:?}",
            d.warnings
        );
    };
    // Con la cesión de la cartera de clientes, no sujeta: se resta la línea del vehículo.
    let d = linked((
        "Cesión de la cartera de clientes (art. 7.1.º)",
        40_000,
        VatTreatment::NotSubject(NotSubject::N1),
    ));
    assert_eq!(get(&d, 99), eur(10_000));
    assert_eq!(get(&d, 107), eur(10_000));
    assert_eq!(get(&d, 108), Money::ZERO);
    check(&d, "Vehículo usado 1234-ABC");
    // Con un mantenimiento mayor y sujeto no se sabe cuál es: se toma el mayor y se avisa.
    let d = linked(("Mantenimiento anual", 12_000, iva21()));
    assert_eq!(get(&d, 99), eur(22_000));
    assert_eq!(get(&d, 107), eur(12_000));
    check(&d, "Mantenimiento anual");
}

/// La venta vinculada facturada en dos líneas que la nombran se resta con las dos, hasta lo
/// vinculado.
#[test]
fn venta_vinculada_facturada_en_dos_líneas() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    let v = setup();
    // Furgoneta (218, 20.000) vendida el 10/03/2026 por 10.500 con A2026-0001: la furgoneta por
    // 10.000 y sus estanterías por 500.
    let furgoneta = new_asset(&v, "Furgoneta", "218", 20_000, date!(2026 - 03 - 01));
    let d0 = date!(2026 - 03 - 10);
    let f = issue(
        &v,
        &sale_lines(
            &[
                ("Furgoneta Renault 1234-ABC", 10_000, iva21()),
                ("Estanterías instaladas en la furgoneta", 500, iva21()),
            ],
            d0,
        ),
        at(d0),
        true,
    )
    .unwrap();
    dispose_asset_with_invoice(&v, furgoneta, d0, eur(10_500), Some(f.id), at(d0)).unwrap();

    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), eur(10_500));
    assert_eq!(get(&d, 107), eur(10_500));
    assert_eq!(get(&d, 108), Money::ZERO);
    assert_eq!(
        d.details,
        vec![(
            "[107] Venta de Furgoneta (218) del 10/03/2026 · factura A2026-0001".to_string(),
            eur(10_500)
        )]
    );
    assert!(
        !d.warnings
            .iter()
            .any(|w| w.starts_with("Venta de Furgoneta")),
        "{:?}",
        d.warnings
    );
}

/// La línea de la factura vinculada a una venta no la toma una venta sin vincular de otro año,
/// aunque la vinculada no cuente en el volumen de ése.
#[test]
fn la_línea_vinculada_no_la_toma_una_venta_de_otro_año() {
    use taxpain_app::accounting::dispose_asset;
    use taxpain_app::venta_inmovilizado::link_sale_invoice;
    let v = setup();
    // Torno dado de baja el 10/05/2026 por 3.000 y facturado el 20/12/2026 (A2026-0001), vinculada.
    let torno = new_asset(&v, "Torno", "213", 12_000, date!(2026 - 05 - 01));
    let d0 = date!(2026 - 05 - 10);
    dispose_asset(&v, torno, d0, eur(3000), at(d0)).unwrap();
    let d1 = date!(2026 - 12 - 20);
    let f = issue(
        &v,
        &sale_draft("Venta del torno", 3000, iva21(), d1),
        at(d1),
        true,
    )
    .unwrap();
    link_sale_invoice(&v, torno, f.id, None, at(d1)).unwrap();
    // Remolque (218, 9.000) vendido el 10/01/2027 por 3.000, sin factura.
    sell_asset(&v, "Remolque", "218", 9000, 3000, date!(2027 - 01 - 10));

    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 107), eur(3000));
    let d = models::draft(&v, "390", 2027, 0).unwrap();
    assert_eq!(get(&d, 107), Money::ZERO);
    let w: Vec<_> = d
        .warnings
        .iter()
        .filter(|w| w.starts_with("Venta de Remolque"))
        .collect();
    assert!(
        w.len() == 1
            && w[0].contains("A2026-0001")
            && w[0].contains("ya es la venta de Torno")
            && !w[0].contains("devenga en 2026"),
        "{:?}",
        d.warnings
    );
}

/// Una venta sin vincular cuya base es lo que queda sin vincular de la línea de otra venta
/// vinculada: el aviso pide vincularle esa factura, no que se emita.
#[test]
fn venta_sin_vincular_en_la_línea_de_otra_vinculada() {
    use taxpain_app::accounting::{dispose_asset, dispose_asset_with_invoice};
    use taxpain_app::venta_inmovilizado::link_sale_invoice;
    let v = setup();
    let torno = new_asset(&v, "Torno", "213", 12_000, date!(2026 - 05 - 01));
    let fresadora = new_asset(&v, "Fresadora", "213", 6000, date!(2026 - 05 - 01));
    // A2026-0001 (10/05/2026): «Torno y fresadora usados» por 8.000 en una línea. El torno se da
    // de baja con ella por 3.000; la fresadora, por 5.000 sin elegir la factura.
    let d0 = date!(2026 - 05 - 10);
    let f = issue(
        &v,
        &sale_draft("Torno y fresadora usados", 8000, iva21(), d0),
        at(d0),
        true,
    )
    .unwrap();
    dispose_asset_with_invoice(&v, torno, d0, eur(3000), Some(f.id), at(d0)).unwrap();
    dispose_asset(&v, fresadora, d0, eur(5000), at(d0)).unwrap();

    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), eur(8000));
    assert_eq!(get(&d, 107), eur(3000));
    assert_eq!(get(&d, 108), eur(5000));
    let w: Vec<_> = d
        .warnings
        .iter()
        .filter(|w| w.starts_with("Venta de Fresadora"))
        .collect();
    assert!(
        w.len() == 1
            && w[0].contains("A2026-0001")
            && w[0].contains("venta de Torno")
            && w[0].contains("5.000,00 € sin vincular")
            && !w[0].contains("ninguna factura")
            && !w[0].contains("Sin factura"),
        "{:?}",
        d.warnings
    );
    // Vinculada también a la fresadora, se restan las dos.
    link_sale_invoice(&v, fresadora, f.id, None, at(d0)).unwrap();
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 107), eur(8000));
    assert_eq!(get(&d, 108), Money::ZERO);
    assert!(
        !d.warnings
            .iter()
            .any(|w| w.starts_with("Venta de Fresadora")),
        "{:?}",
        d.warnings
    );
}

/// La venta restada con la factura de un ejercicio cerrado no sale en el aviso de vincular:
/// TaxPain no la deja vincular sin reabrirlo.
#[test]
fn venta_con_la_factura_de_un_ejercicio_cerrado_no_pide_vincularla() {
    use taxpain_app::accounting::close_year;
    let v = setup();
    // Remolque (218, 9.000) vendido el 01/09/2026 por 7.000 con su factura, sin vincular; después
    // se cierra 2026 (y los anteriores, que tienen sus amortizaciones).
    sell_asset(&v, "Remolque", "218", 9000, 7000, date!(2026 - 09 - 01));
    emit_sale(
        &v,
        "Venta del remolque",
        7000,
        iva21(),
        date!(2026 - 09 - 01),
    );
    for year in 2023..=2026 {
        close_year(&v, year, at(date!(2027 - 01 - 31)), true).unwrap();
    }

    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), eur(7000));
    assert_eq!(get(&d, 107), eur(7000));
    assert_eq!(get(&d, 108), Money::ZERO);
    assert!(
        !d.warnings
            .iter()
            .any(|w| w.starts_with("Ventas del inmovilizado sin vincular")),
        "{:?}",
        d.warnings
    );
}

// Hallazgos de la revisión del 390 (31/10/2026): ventas del inmovilizado con rectificativas,
// líneas compartidas y facturas de otros años.

/// Rectificativa por diferencias de las facturas `of` (número y fecha de expedición) con una línea
/// por cada (concepto, base, IVA).
fn rectify_lines(
    v: &Vault,
    of: &[(&str, Date)],
    lines: &[(&str, i64, VatTreatment)],
    d: Date,
) -> invoicing::IssuedInvoice {
    use taxpain_core::invoice::{Rectification, RectificationMethod};
    let mut r = sale_lines(lines, d);
    r.kind = InvoiceKind::Rectificativa(Rectification {
        code: 1,
        method: RectificationMethod::Diferencias,
    });
    r.rectified = of.iter().map(|(n, d)| ((*n).to_string(), *d)).collect();
    issue(v, &r, at(d), true).unwrap()
}

fn n1() -> VatTreatment {
    VatTreatment::NotSubject(taxpain_core::invoice::NotSubject::N1)
}

fn n2() -> VatTreatment {
    VatTreatment::NotSubject(taxpain_core::invoice::NotSubject::N2)
}

/// 303 del 4T de `year` de un exonerado del 390 (página 04).
fn exonerado_4t(v: &Vault, year: i32) -> models::Draft {
    models::save_settings(
        v,
        &ModelSettings {
            exempt_from_390: true,
            ..ModelSettings::default()
        },
        at(date!(2026 - 12 - 30)),
    )
    .unwrap();
    models::draft(v, "303", year, 4).unwrap()
}

/// Avisos del borrador que empiezan por `what`.
fn avisos<'a>(d: &'a models::Draft, what: &str) -> Vec<&'a String> {
    d.warnings.iter().filter(|w| w.starts_with(what)).collect()
}

/// La rectificativa pasada sola a la venta vinculada a toda su factura (con un suplido no sujeto)
/// cambia la venta en lo que cambia su base sujeta, no en lo que pasa a la venta: el suplido no
/// está en el volumen.
#[test]
fn rectificativa_con_suplido_de_una_venta_vinculada_a_toda_su_factura() {
    use taxpain_app::accounting::dispose_asset;
    use taxpain_app::venta_inmovilizado::{asset_sales, link_sale_invoice};
    // Furgoneta (218, 20.000) dada de baja sin precio el 10/03/2026 y vinculada a toda A2026-0001
    // (furgoneta 10.000 y tasa de la DGT 150, suplido no sujeto); R2026-0001 (15/04) cambia el
    // precio en `price` y el suplido en `fee`, y se pasa sola a la venta.
    let sold = |price: i64, fee: i64| {
        let v = setup();
        let furgoneta = new_asset(&v, "Furgoneta", "218", 20_000, date!(2026 - 03 - 01));
        let d0 = date!(2026 - 03 - 10);
        dispose_asset(&v, furgoneta, d0, Money::ZERO, at(d0)).unwrap();
        let f = issue(
            &v,
            &sale_lines(
                &[
                    ("Furgoneta", 10_000, iva21()),
                    ("Tasa DGT (suplido)", 150, n1()),
                ],
                d0,
            ),
            at(d0),
            true,
        )
        .unwrap();
        let l = link_sale_invoice(&v, furgoneta, f.id, None, at(d0)).unwrap();
        assert_eq!(l.amount, eur(10_150));
        rectify_lines(
            &v,
            &[(&f.number, d0)],
            &[
                ("Ajuste del precio de la furgoneta", price, iva21()),
                ("Suplido", fee, n1()),
            ],
            date!(2026 - 04 - 15),
        );
        let s = &asset_sales(&v).unwrap()[0];
        assert_eq!(s.invoices[1].amount, eur(price + fee), "{s:?}");
        v
    };

    let v = sold(-1000, 200);
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), eur(9000));
    assert_eq!(get(&d, 107), eur(9000));
    assert_eq!(get(&d, 108), Money::ZERO);
    let w = avisos(&d, "Venta de Furgoneta");
    assert!(
        w.len() == 1 && w[0].contains("quedan 9.000,00 €"),
        "{:?}",
        d.warnings
    );
    let q4 = exonerado_4t(&v, 2026);
    assert_eq!(get(&q4, 99), eur(9000));
    assert_eq!(get(&q4, 88), Money::ZERO);

    let v = sold(500, -150);
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), eur(10_500));
    assert_eq!(get(&d, 107), eur(10_500));
    assert_eq!(get(&d, 108), Money::ZERO);
}

/// Lo pasado a cada venta de una rectificativa cuenta sin la parte no sujeta del vínculo: la
/// furgoneta, vinculada con la tasa, no se lleva parte de la rebaja del remolque ni del local.
#[test]
fn rectificativa_vinculada_a_dos_ventas_con_el_suplido_en_una() {
    use taxpain_app::accounting::{dispose_asset, dispose_asset_with_invoice};
    use taxpain_app::venta_inmovilizado::link_sale_invoice;
    use taxpain_core::invoice::Exemption;
    // A2026-0001 (10/03/2026): `other` («Remolque» 4.000 al 21 % o «Local» 60.000 exento), la
    // furgoneta 10.000 y la tasa de la DGT 150 (suplido). `other` se da de baja con ella por su
    // base; la furgoneta, sin precio y vinculada después por lo que queda (con la tasa).
    // R2026-0001 (15/04) rebaja `other` en `cut`, la furgoneta en `van` y devuelve la tasa; cada
    // venta lleva lo suyo: `other` su rebaja y la furgoneta la suya con la tasa.
    let sold = |other: (&str, &str, i64, i64, VatTreatment), cut: i64, van: i64| {
        let v = setup();
        let (name, account, cost, price, vat) = other;
        let first = new_asset(&v, name, account, cost, date!(2026 - 03 - 01));
        let furgoneta = new_asset(&v, "Furgoneta", "218", 20_000, date!(2026 - 03 - 01));
        let d0 = date!(2026 - 03 - 10);
        let f = issue(
            &v,
            &sale_lines(
                &[
                    (name, price, vat),
                    ("Furgoneta", 10_000, iva21()),
                    ("Tasa DGT (suplido)", 150, n1()),
                ],
                d0,
            ),
            at(d0),
            true,
        )
        .unwrap();
        dispose_asset_with_invoice(&v, first, d0, eur(price), Some(f.id), at(d0)).unwrap();
        dispose_asset(&v, furgoneta, d0, Money::ZERO, at(d0)).unwrap();
        let l = link_sale_invoice(&v, furgoneta, f.id, None, at(d0)).unwrap();
        assert_eq!(l.amount, eur(10_150));
        let d1 = date!(2026 - 04 - 15);
        let r = rectify_lines(
            &v,
            &[(&f.number, d0)],
            &[
                ("Rebaja", cut, vat),
                ("Devolución de la furgoneta", van, iva21()),
                ("Devolución de la tasa", -150, n1()),
            ],
            d1,
        );
        link_sale_invoice(&v, first, r.id, Some(eur(cut)), at(d1)).unwrap();
        link_sale_invoice(&v, furgoneta, r.id, Some(eur(van - 150)), at(d1)).unwrap();
        models::draft(&v, "390", 2026, 0).unwrap()
    };

    // Remolque 4.000 rebajado 1.000 y furgoneta devuelta: 3.000 del remolque.
    let d = sold(("Remolque", "218", 9000, 4000, iva21()), -1000, -10_000);
    assert_eq!(get(&d, 99), eur(3000));
    assert_eq!(get(&d, 107), eur(3000));
    assert_eq!(get(&d, 108), Money::ZERO);
    // Local exento 60.000 rebajado 5.000 y furgoneta rebajada 1.000.
    let d = sold(
        (
            "Local",
            "211",
            80_000,
            60_000,
            VatTreatment::Exempt(Exemption::E1),
        ),
        -5000,
        -1000,
    );
    assert_eq!(get(&d, 105), eur(55_000));
    assert_eq!(get(&d, 106), eur(55_000));
    assert_eq!(get(&d, 107), eur(9000));
    assert_eq!(get(&d, 108), Money::ZERO);
}

/// Las rectificativas sin pasar a la venta cambian su precio aunque lo suban, y dan lo mismo en
/// cualquier orden; si son de otro año, se compensan allí.
#[test]
fn rectificativas_sin_pasar_que_suben_y_bajan_la_venta() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    // Torno (213, 12.000) vendido el 10/05/2026 por `price` con A2026-0001 («Venta del torno», y
    // «Transporte» 200 si `transport`), sin vincular o vinculada si hay transporte (entonces las
    // rectificativas no se pasan solas); rectificativas «Ajuste del precio del torno» por cada
    // (importe, fecha).
    let sold = |price: i64, transport: bool, rects: &[(i64, Date)]| {
        let v = setup();
        let torno = new_asset(&v, "Torno", "213", 12_000, date!(2026 - 05 - 01));
        let d0 = date!(2026 - 05 - 10);
        let mut lines = vec![("Venta del torno", price, iva21())];
        if transport {
            lines.push(("Transporte", 200, iva21()));
        }
        let f = issue(&v, &sale_lines(&lines, d0), at(d0), true).unwrap();
        let invoice = transport.then_some(f.id);
        dispose_asset_with_invoice(&v, torno, d0, eur(price), invoice, at(d0)).unwrap();
        for (m, d) in rects {
            rectify_sale(&v, (&f.number, d0), "Ajuste del precio del torno", *m, *d);
        }
        v
    };
    let boxes = |v: &Vault, year: i32| {
        let d = models::draft(v, "390", year, 0).unwrap();
        ((get(&d, 99), get(&d, 107), get(&d, 108)), d)
    };
    let (june1, june15) = (date!(2026 - 06 - 01), date!(2026 - 06 - 15));
    for transport in [false, true] {
        for rects in [
            [(600, june1), (-1000, june15)],
            [(-1000, june1), (600, june15)],
        ] {
            let v = sold(5000, transport, &rects);
            let (b, _) = boxes(&v, 2026);
            let extra = if transport { 200 } else { 0 };
            assert_eq!(
                b,
                (eur(4600 + extra), eur(4600), eur(extra)),
                "{transport} {rects:?}"
            );
        }
    }
    // Sólo una subida de 500 sobre 3.000.
    let v = sold(3000, false, &[(500, june1)]);
    let (b, d) = boxes(&v, 2026);
    assert_eq!(b, (eur(3500), eur(3500), Money::ZERO));
    let w = avisos(&d, "Venta de Torno");
    assert!(
        w.len() == 1 && w[0].contains("quedan 3.500,00 €"),
        "{:?}",
        d.warnings
    );
    // +600 en 2027 y −1.000 en 2028: cada año compensa la suya.
    let v = sold(
        5000,
        false,
        &[(600, date!(2027 - 02 - 15)), (-1000, date!(2028 - 02 - 15))],
    );
    assert_eq!(boxes(&v, 2026).0, (eur(5000), eur(5000), Money::ZERO));
    let (b, d) = boxes(&v, 2027);
    assert_eq!(b, (eur(600), eur(600), Money::ZERO));
    assert!(
        avisos(&d, "Venta de Torno")
            .iter()
            .any(|w| w.contains("R2027-0001")),
        "{:?}",
        d.warnings
    );
    assert_eq!(boxes(&v, 2028).0, (eur(-1000), eur(-1000), Money::ZERO));
}

/// La rectificativa pasada entera a una venta no sujeta (la grúa) no cambia la de otra venta de
/// la misma factura.
#[test]
fn rectificativa_pasada_a_una_venta_no_sujeta() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    use taxpain_app::venta_inmovilizado::link_sale_invoice;
    let v = setup();
    // A2026-0001 (10/03/2026): la grúa (no sujeta, en Portugal) 7.000, su desmontaje 500 y la
    // furgoneta 10.000. La grúa se da de baja con ella por 7.500 (con el desmontaje); la
    // furgoneta, por 10.000.
    let grua = new_asset(&v, "Grúa", "213", 20_000, date!(2026 - 03 - 01));
    let furgoneta = new_asset(&v, "Furgoneta", "218", 20_000, date!(2026 - 03 - 01));
    let d0 = date!(2026 - 03 - 10);
    let f = issue(
        &v,
        &sale_lines(
            &[
                ("Venta de la grúa", 7000, n2()),
                ("Desmontaje", 500, iva21()),
                ("Furgoneta", 10_000, iva21()),
            ],
            d0,
        ),
        at(d0),
        true,
    )
    .unwrap();
    dispose_asset_with_invoice(&v, grua, d0, eur(7500), Some(f.id), at(d0)).unwrap();
    dispose_asset_with_invoice(&v, furgoneta, d0, eur(10_000), Some(f.id), at(d0)).unwrap();
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(
        (get(&d, 99), get(&d, 110), get(&d, 107), get(&d, 108)),
        (eur(10_500), eur(7000), eur(10_000), eur(7500))
    );
    // R2026-0001 (15/04) anula la grúa y su desmontaje, y se pasa entera a la grúa.
    let d1 = date!(2026 - 04 - 15);
    let r = rectify_lines(
        &v,
        &[(&f.number, d0)],
        &[
            ("Anulación de la grúa", -7000, n2()),
            ("Anulación del desmontaje", -500, iva21()),
        ],
        d1,
    );
    link_sale_invoice(&v, grua, r.id, Some(-eur(7500)), at(d1)).unwrap();
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(
        (get(&d, 99), get(&d, 110), get(&d, 107), get(&d, 108)),
        (eur(10_000), Money::ZERO, eur(10_000), Money::ZERO)
    );
    assert_eq!(
        d.details,
        vec![(
            "[107] Venta de Furgoneta (218) del 10/03/2026 · factura A2026-0001".to_string(),
            eur(10_000)
        )]
    );
    let q4 = exonerado_4t(&v, 2026);
    assert_eq!(get(&q4, 99), eur(10_000));
    assert_eq!(get(&q4, 88), Money::ZERO);
}

/// Una rectificativa de dos facturas de venta (art. 15.2 RD 1619/2012) cuenta una vez: si no dice
/// qué parte es de cada una, se reparte en proporción a sus bases y se avisa; se puede pasar a
/// cada venta su parte y no se pasa sola entera a la primera.
#[test]
fn rectificativa_de_dos_facturas_de_venta() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    use taxpain_app::venta_inmovilizado::{asset_sales, link_sale_invoice};
    // Torno (213, 12.000) vendido el 10/05/2026 con A2026-0001 por 3.000 y `second` (218, 9.000)
    // con A2026-0002 por `price`; vinculadas al darlos de baja si `linked`. R2026-0001 (o
    // R2027-0001) rectifica las dos con `lines`.
    struct Sold {
        v: Vault,
        torno: i64,
        second: i64,
        r: i64,
    }
    let sold = |linked: bool, second: (&str, i64), lines: &[(&str, i64, VatTreatment)], back| {
        let v = setup();
        let torno = new_asset(&v, "Torno", "213", 12_000, date!(2026 - 05 - 01));
        let other = new_asset(&v, second.0, "218", 9000, date!(2026 - 05 - 01));
        let d0 = date!(2026 - 05 - 10);
        let mut numbers = Vec::new();
        for (id, what, price) in [
            (torno, "Venta del torno".to_string(), 3000),
            (other, format!("Venta de {}", second.0), second.1),
        ] {
            let f = issue(&v, &sale_draft(&what, price, iva21(), d0), at(d0), true).unwrap();
            let invoice = linked.then_some(f.id);
            dispose_asset_with_invoice(&v, id, d0, eur(price), invoice, at(d0)).unwrap();
            numbers.push(f.number);
        }
        let r = rectify_lines(&v, &[(&numbers[0], d0), (&numbers[1], d0)], lines, back);
        Sold {
            v,
            torno,
            second: other,
            r: r.id,
        }
    };
    let boxes = |v: &Vault, year: i32| {
        let d = models::draft(v, "390", year, 0).unwrap();
        ((get(&d, 99), get(&d, 107), get(&d, 108)), d)
    };
    let several = |d: &models::Draft| {
        d.warnings
            .iter()
            .filter(|w| w.contains("rectifica varias facturas"))
            .count()
    };
    let discounts = [
        ("Descuento del torno", -300, iva21()),
        ("Descuento del remolque", -400, iva21()),
    ];
    let june = date!(2026 - 06 - 01);
    for linked in [true, false] {
        let s = sold(linked, ("Remolque", 4000), &discounts, june);
        // No se pasa sola a ninguna venta: queda pendiente en las dos.
        let sales = asset_sales(&s.v).unwrap();
        for a in sales.iter().filter(|a| linked && a.invoices.len() == 1) {
            assert!(
                a.pending.iter().any(|p| p.invoice_id == s.r),
                "{linked}: {sales:?}"
            );
        }
        assert!(
            sales.iter().all(|a| a.invoices.len() <= 1),
            "{linked}: {sales:?}"
        );
        let (b, d) = boxes(&s.v, 2026);
        assert_eq!(b, (eur(6300), eur(6300), Money::ZERO), "{linked}");
        assert_eq!(
            d.details,
            vec![
                (
                    "[107] Venta de Torno (213) del 10/05/2026 · factura A2026-0001".to_string(),
                    eur(2700)
                ),
                (
                    "[107] Venta de Remolque (218) del 10/05/2026 · factura A2026-0002".to_string(),
                    eur(3600)
                ),
            ],
            "{linked}"
        );
        assert_eq!(several(&d), 1, "{linked}: {:?}", d.warnings);
        if linked {
            // Pasada a cada venta su parte, cuenta esa parte y no hay aviso.
            let now = at(date!(2026 - 06 - 02));
            link_sale_invoice(&s.v, s.torno, s.r, Some(-eur(300)), now).unwrap();
            link_sale_invoice(&s.v, s.second, s.r, Some(-eur(400)), now).unwrap();
            let (b, d) = boxes(&s.v, 2026);
            assert_eq!(b, (eur(6300), eur(6300), Money::ZERO));
            assert_eq!(several(&d), 0, "{:?}", d.warnings);
        }
        // En 2027: 2026 entero y la rectificativa compensada una vez.
        let s = sold(
            linked,
            ("Remolque", 4000),
            &discounts,
            date!(2027 - 02 - 15),
        );
        assert_eq!(boxes(&s.v, 2026).0, (eur(7000), eur(7000), Money::ZERO));
        let (b, _) = boxes(&s.v, 2027);
        assert_eq!(b, (eur(-700), eur(-700), Money::ZERO), "{linked}");
    }
    // Torno 3.000 y fresadora 5.000 con un descuento de 500 en el torno en una rectificativa de
    // las dos facturas; pasado al torno, no rebaja la fresadora.
    let discount = [("Descuento en el torno", -500, iva21())];
    for linked in [false, true] {
        let s = sold(linked, ("Fresadora", 5000), &discount, june);
        assert_eq!(
            boxes(&s.v, 2026).0,
            (eur(7500), eur(7500), Money::ZERO),
            "{linked}"
        );
        if linked {
            link_sale_invoice(&s.v, s.torno, s.r, Some(-eur(500)), at(june)).unwrap();
            let (b, d) = boxes(&s.v, 2026);
            assert_eq!(b, (eur(7500), eur(7500), Money::ZERO));
            assert!(
                d.details
                    .iter()
                    .any(|(t, m)| t.starts_with("[107] Venta de Fresadora") && *m == eur(5000)),
                "{:?}",
                d.details
            );
        }
        let s = sold(
            linked,
            ("Fresadora", 5000),
            &discount,
            date!(2027 - 02 - 15),
        );
        assert_eq!(
            boxes(&s.v, 2027).0,
            (eur(-500), eur(-500), Money::ZERO),
            "{linked}"
        );
    }
}

/// Rápel de una rectificativa de una factura de servicios («Consultoría») y otra de venta: si
/// nombra la consultoría es de su factura y no toca la venta; si no nombra nada, se reparte entre
/// las dos por su base y se avisa.
#[test]
fn rapel_de_una_factura_de_servicios_y_otra_de_venta() {
    let sold = |what: &str| {
        let v = setup();
        emit(&v, 10_000, None, date!(2026 - 05 - 02));
        sell_asset(&v, "Torno", "213", 12_000, 3000, date!(2026 - 05 - 10));
        emit_sale(&v, "Venta del torno", 3000, iva21(), date!(2026 - 05 - 10));
        rectify_lines(
            &v,
            &[
                ("A2026-0001", date!(2026 - 05 - 02)),
                ("A2026-0002", date!(2026 - 05 - 10)),
            ],
            &[(what, -500, iva21())],
            date!(2026 - 12 - 15),
        );
        models::draft(&v, "390", 2026, 0).unwrap()
    };
    let several = |d: &models::Draft| {
        d.warnings.iter().any(|w| {
            w.contains("R2026-0001")
                && w.contains("rectifica varias facturas")
                && w.contains("A2026-0001")
                && w.contains("A2026-0002")
        })
    };
    let d = sold("Rápel anual de consultoría");
    assert_eq!(
        (get(&d, 99), get(&d, 107), get(&d, 108)),
        (eur(12_500), eur(3000), eur(9500))
    );
    assert!(!several(&d), "{:?}", d.warnings);
    let d = sold("Rápel anual");
    assert_eq!(get(&d, 99), eur(12_500));
    // 500 × 3.000 / 13.000 = 115,38 del torno.
    assert_eq!(get(&d, 107), Money::from_cents(288_462));
    assert_eq!(get(&d, 108), Money::from_cents(961_538));
    assert!(several(&d), "{:?}", d.warnings);
}

/// Lo pasado de más a una venta (o lo que falta por pasar) no deja [108] en negativo: lo que no
/// cabe en una venta es de las demás de la factura y, si la factura no tiene más líneas, de sus
/// ventas, con aviso. Una rectificativa no puede bajar la venta de cero.
#[test]
fn vínculos_de_la_rectificativa_que_no_cuadran_con_la_factura() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    use taxpain_app::venta_inmovilizado::{link_sale_invoice, unlink_sale_invoice};
    use taxpain_vault::rusqlite::params;
    // (a) A2026-0001 (10/05/2026): torno 3.000, vinculado, y fresadora 5.000, sin vincular;
    // R2026-0001 (01/06) anula las dos líneas.
    let v = setup();
    let torno = new_asset(&v, "Torno", "213", 12_000, date!(2026 - 05 - 01));
    let d0 = date!(2026 - 05 - 10);
    let f = issue(
        &v,
        &sale_lines(
            &[
                ("Torno usado", 3000, iva21()),
                ("Fresadora usada", 5000, iva21()),
            ],
            d0,
        ),
        at(d0),
        true,
    )
    .unwrap();
    dispose_asset_with_invoice(&v, torno, d0, eur(3000), Some(f.id), at(d0)).unwrap();
    sell_asset(&v, "Fresadora", "213", 6000, 5000, d0);
    let d1 = date!(2026 - 06 - 01);
    let r = rectify_lines(
        &v,
        &[(&f.number, d0)],
        &[
            ("Anulación del torno", -3000, iva21()),
            ("Anulación de la fresadora", -5000, iva21()),
        ],
        d1,
    );
    // Pasar los 8.000 al torno lo dejaría en −5.000: no se puede.
    let e = link_sale_invoice(&v, torno, r.id, Some(-eur(8000)), at(d1)).unwrap_err();
    assert!(e.to_string().contains("de cero"), "{e}");
    // Un vínculo así de antes (escrito a mano en el almacén) tampoco deja [108] en negativo.
    let entry: i64 = v
        .connection()
        .query_row(
            "SELECT disposal_entry FROM fixed_asset WHERE id = ?1",
            [torno],
            |r| r.get(0),
        )
        .unwrap();
    v.connection()
        .execute(
            "INSERT INTO asset_sale_invoice(asset_id, invoice_id, amount, entry_id, created_at)
             VALUES (?1, ?2, ?3, ?4, '2026-06-01T10:00:00Z')",
            params![torno, r.id, -800_000, entry],
        )
        .unwrap();
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(
        (get(&d, 99), get(&d, 107), get(&d, 108)),
        (Money::ZERO, Money::ZERO, Money::ZERO)
    );

    // (b) Torno vendido entero con A2026-0001 (3.000); R2026-0001 rebaja 500 y se pasa sola, pero
    // después se desvincula y se pasan sólo 200.
    let v = setup();
    let torno = new_asset(&v, "Torno", "213", 12_000, date!(2026 - 05 - 01));
    let f = issue(
        &v,
        &sale_draft("Venta del torno", 3000, iva21(), d0),
        at(d0),
        true,
    )
    .unwrap();
    dispose_asset_with_invoice(&v, torno, d0, eur(3000), Some(f.id), at(d0)).unwrap();
    let r = rectify_sale(&v, (&f.number, d0), "Descuento", -500, d1);
    unlink_sale_invoice(&v, torno, r.id, at(d1), true).unwrap();
    link_sale_invoice(&v, torno, r.id, Some(-eur(200)), at(d1)).unwrap();
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(
        (get(&d, 99), get(&d, 107), get(&d, 108)),
        (eur(2500), eur(2500), Money::ZERO)
    );
    assert!(
        d.warnings
            .iter()
            .any(|w| w.starts_with("La rectificativa R2026-0001")
                && w.contains("200,00 €")
                && w.contains("500,00 €")),
        "{:?}",
        d.warnings
    );
}

/// El reparto de una rectificativa entre las ventas de su factura no depende del orden en que se
/// dieron de alta: el céntimo que sobra va al resto mayor y, si empatan, por cuenta y nombre.
#[test]
fn el_reparto_de_la_rectificativa_no_depende_del_orden_de_alta() {
    let d0 = date!(2026 - 05 - 10);
    // Torno (213) 1.000, nave (211) 2.000 y local (211) 3.000 en A2026-0001, sin vincular, dados
    // de alta en los dos órdenes; R2026-0001 de −100,00: 16,67, 33,33 y 50,00.
    let three = [
        ("Torno", "213", 12_000, 1000),
        ("Nave", "211", 50_000, 2000),
        ("Local", "211", 80_000, 3000),
    ];
    let lines = [("Torno", 1000), ("Nave", 2000), ("Local", 3000)];
    for order in [
        three.to_vec(),
        three.iter().rev().copied().collect::<Vec<_>>(),
    ] {
        let v = setup();
        for (name, account, cost, price) in &order {
            sell_asset(&v, name, account, *cost, *price, d0);
        }
        let lines: Vec<(&str, i64, VatTreatment)> =
            lines.iter().map(|(n, b)| (*n, *b, iva21())).collect();
        let f = issue(&v, &sale_lines(&lines, d0), at(d0), true).unwrap();
        rectify_sale(
            &v,
            (&f.number, d0),
            "Descuento",
            -100,
            date!(2026 - 06 - 01),
        );
        let d = models::draft(&v, "390", 2026, 0).unwrap();
        assert_eq!(get(&d, 106), Money::from_cents(491_667), "{order:?}");
        assert_eq!(get(&d, 107), Money::from_cents(98_333), "{order:?}");
    }
    // Dos ventas de 100.000 (nave y torno) y una rectificativa de −1.000,01: el céntimo, a la nave
    // (cuenta 211) en los dos órdenes.
    let two = [("Nave", "211", 150_000), ("Torno", "213", 120_000)];
    for order in [two.to_vec(), two.iter().rev().copied().collect::<Vec<_>>()] {
        let v = setup();
        for (name, account, cost) in &order {
            sell_asset(&v, name, account, *cost, 100_000, d0);
        }
        let f = issue(
            &v,
            &sale_lines(
                &[("Nave", 100_000, iva21()), ("Torno", 100_000, iva21())],
                d0,
            ),
            at(d0),
            true,
        )
        .unwrap();
        let mut r = sale_lines(&[("Descuento", 0, iva21())], date!(2026 - 06 - 01));
        r.lines[0].unit_price = Money::from_cents(-100_001);
        r.kind = InvoiceKind::Rectificativa(taxpain_core::invoice::Rectification {
            code: 1,
            method: taxpain_core::invoice::RectificationMethod::Diferencias,
        });
        r.rectified = vec![(f.number.clone(), d0)];
        issue(&v, &r, at(date!(2026 - 06 - 01)), true).unwrap();
        let d = models::draft(&v, "390", 2026, 0).unwrap();
        assert_eq!(get(&d, 106), Money::from_cents(9_949_999), "{order:?}");
        assert_eq!(get(&d, 107), Money::from_cents(9_950_000), "{order:?}");
    }
}

/// En un ejercicio cerrado los avisos no piden vincular sin decir que hay que reabrirlo antes:
/// TaxPain no deja vincular facturas ni rectificativas de un ejercicio cerrado.
#[test]
fn avisos_que_piden_vincular_en_un_ejercicio_cerrado() {
    use taxpain_app::accounting::{close_year, dispose_asset_with_invoice};
    use taxpain_app::venta_inmovilizado::link_sale_invoice;
    let close = |v: &Vault| {
        for year in 2023..=2026 {
            close_year(v, year, at(date!(2027 - 01 - 31)), true).unwrap();
        }
    };
    let d0 = date!(2026 - 05 - 10);
    // (a) «Torno y fresadora usados» 8.000: el torno vinculado por 3.000 y la fresadora por 5.000
    // sin vincular.
    let v = setup();
    let torno = new_asset(&v, "Torno", "213", 12_000, date!(2026 - 05 - 01));
    let fresadora = new_asset(&v, "Fresadora", "213", 6000, date!(2026 - 05 - 01));
    let f = issue(
        &v,
        &sale_draft("Torno y fresadora usados", 8000, iva21(), d0),
        at(d0),
        true,
    )
    .unwrap();
    dispose_asset_with_invoice(&v, torno, d0, eur(3000), Some(f.id), at(d0)).unwrap();
    dispose_asset_with_invoice(&v, fresadora, d0, eur(5000), None, at(d0)).unwrap();
    close(&v);
    let e = link_sale_invoice(&v, fresadora, f.id, None, at(d0)).unwrap_err();
    assert!(e.to_string().contains("cerrado"), "{e}");
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 107), eur(3000));
    let w = avisos(&d, "Venta de Fresadora");
    assert!(
        w.len() == 1 && w[0].contains("5.000,00 € sin vincular") && w[0].contains("reábrelo"),
        "{:?}",
        d.warnings
    );

    // (b) Torno 3.000 y fresadora 5.000 en dos líneas, vinculadas o no, y R2026-0001 de −800 sin
    // repartir.
    for linked in [true, false] {
        let v = setup();
        let torno = new_asset(&v, "Torno", "213", 12_000, date!(2026 - 05 - 01));
        let fresadora = new_asset(&v, "Fresadora", "213", 6000, date!(2026 - 05 - 01));
        let f = issue(
            &v,
            &sale_lines(
                &[
                    ("Torno usado", 3000, iva21()),
                    ("Fresadora usada", 5000, iva21()),
                ],
                d0,
            ),
            at(d0),
            true,
        )
        .unwrap();
        let invoice = linked.then_some(f.id);
        dispose_asset_with_invoice(&v, torno, d0, eur(3000), invoice, at(d0)).unwrap();
        dispose_asset_with_invoice(&v, fresadora, d0, eur(5000), invoice, at(d0)).unwrap();
        let r = rectify_sale(
            &v,
            (&f.number, d0),
            "Descuento",
            -800,
            date!(2026 - 06 - 01),
        );
        close(&v);
        if linked {
            let e = link_sale_invoice(&v, torno, r.id, Some(-eur(300)), at(d0)).unwrap_err();
            assert!(e.to_string().contains("cerrado"), "{e}");
        }
        let d = models::draft(&v, "390", 2026, 0).unwrap();
        assert_eq!(get(&d, 107), eur(7200), "{linked}");
        let w = avisos(&d, "La rectificativa R2026-0001");
        assert!(
            w.len() == 1 && w[0].contains("reábrelo") && !w[0].contains("Pasa a cada venta"),
            "{linked}: {:?}",
            d.warnings
        );
    }
}

/// Una venta sin vincular se resta en el año en que devenga su factura aunque la línea más
/// cercana a otra venta del año siguiente sea la suya: las facturas se buscan una vez para todas
/// las ventas, no en cada 390.
#[test]
fn venta_sin_vincular_que_se_resta_en_su_año_aunque_otra_la_tenga_cerca() {
    use taxpain_core::invoice::{Exemption, IdType};
    // Carretilla 1 vendida el 10/11/2026 con A2026-0001 (01/12); carretilla 2, el 10/12/2026 con
    // A2027-0001 (05/01/2027): entrega intracomunitaria con fecha de operación 10/12/2026, o
    // nacional sin ella. Las dos, «Venta de carretilla elevadora» por 5.000.
    for intra in [true, false] {
        let v = setup();
        sell_asset(&v, "Carretilla 1", "213", 8000, 5000, date!(2026 - 11 - 10));
        emit_sale(
            &v,
            "Venta de carretilla elevadora",
            5000,
            iva21(),
            date!(2026 - 12 - 01),
        );
        sell_asset(&v, "Carretilla 2", "213", 8000, 5000, date!(2026 - 12 - 10));
        let d1 = date!(2027 - 01 - 05);
        let mut f = sale_draft(
            "Venta de carretilla elevadora",
            5000,
            if intra {
                VatTreatment::Exempt(Exemption::E5)
            } else {
                iva21()
            },
            d1,
        );
        if intra {
            f.operation_date = Some(date!(2026 - 12 - 10));
            f.recipient = Some(Party {
                id: PartyId::Foreign {
                    country: "FR".into(),
                    id_type: IdType::NifIva,
                    id: "12345678901".into(),
                },
                name: "Client SARL".into(),
                address: addr(),
                person: None,
            });
        }
        issue(&v, &f, at(d1), true).unwrap();

        let d = models::draft(&v, "390", 2026, 0).unwrap();
        assert_eq!(get(&d, 107), eur(5000), "{intra}");
        assert_eq!(get(&d, 108), Money::ZERO, "{intra}");
        assert!(
            avisos(&d, "Venta de Carretilla 2")
                .iter()
                .any(|w| w.contains("A2027-0001") && w.contains("devenga en 2027")),
            "{intra}: {:?}",
            d.warnings
        );
        let d = models::draft(&v, "390", 2027, 0).unwrap();
        let operations = if intra { get(&d, 103) } else { get(&d, 99) };
        assert_eq!(operations, eur(5000), "{intra}");
        assert_eq!(get(&d, 107), eur(5000), "{intra}");
        assert_eq!(get(&d, 108), Money::ZERO, "{intra}");
        assert_eq!(
            d.details,
            vec![(
                "[107] Venta de Carretilla 2 (213) del 10/12/2026 · factura A2027-0001".to_string(),
                eur(5000)
            )],
            "{intra}"
        );
        if !intra {
            let q4 = exonerado_4t(&v, 2027);
            assert_eq!(get(&q4, 99), eur(5000));
            assert_eq!(get(&q4, 88), Money::ZERO);
        }
    }

    // Al revés: torno entregado el 30/12/2026 y facturado el 01/12 («Maquinaria usada»,
    // A2026-0001); fresadora entregada el 20/02/2027 y facturada el 08/01/2027 (A2027-0001).
    let v = setup();
    sell_asset(&v, "Torno", "213", 12_000, 5000, date!(2026 - 12 - 30));
    emit_sale(&v, "Maquinaria usada", 5000, iva21(), date!(2026 - 12 - 01));
    sell_asset(&v, "Fresadora", "213", 8000, 5000, date!(2027 - 02 - 20));
    emit_sale(
        &v,
        "Venta de la fresadora",
        5000,
        iva21(),
        date!(2027 - 01 - 08),
    );
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 107), eur(5000));
    assert_eq!(get(&d, 108), Money::ZERO);
    let d = models::draft(&v, "390", 2027, 0).unwrap();
    assert_eq!(get(&d, 107), eur(5000));
    assert_eq!(get(&d, 108), Money::ZERO);
}

/// La venta sin factura no toma la línea de otra venta del año siguiente que la nombra: se avisa
/// de que esa línea ya es la otra venta, sin aconsejar cambiar la fecha de su factura.
#[test]
fn venta_sin_factura_junto_a_la_de_otra_del_año_siguiente() {
    let v = setup();
    sell_asset(&v, "Torno", "213", 12_000, 5000, date!(2026 - 12 - 30));
    sell_asset(&v, "Fresadora", "213", 8000, 5000, date!(2027 - 02 - 16));
    emit_sale(
        &v,
        "Venta de la fresadora",
        5000,
        iva21(),
        date!(2027 - 02 - 10),
    );
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 107), Money::ZERO);
    let w = avisos(&d, "Venta de Torno");
    assert!(
        w.len() == 1
            && w[0].contains("A2027-0001")
            && w[0].contains("ya es la venta de Fresadora")
            && !w[0].contains("fecha de la operación")
            && !w[0].contains("devenga en 2027"),
        "{:?}",
        d.warnings
    );
    let d = models::draft(&v, "390", 2027, 0).unwrap();
    assert_eq!(get(&d, 107), eur(5000));
    assert_eq!(get(&d, 108), Money::ZERO);
}

/// La venta vinculada va a la línea que la nombra antes que a otra con lo vinculado.
#[test]
fn la_línea_que_nombra_la_venta_vinculada_gana_a_la_de_su_base() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    // A2026-0001 (10/05/2026): «Torno y fresadora usados» 8.000 y el transporte 3.000 (suplido no
    // sujeto o al 21 %). El torno se da de baja con ella por 3.000; la fresadora, por 5.000 sin
    // elegir la factura.
    for (transport, ops, volume) in [(n1(), 8000, 5000), (iva21(), 11_000, 8000)] {
        let v = setup();
        let torno = new_asset(&v, "Torno", "213", 12_000, date!(2026 - 05 - 01));
        let fresadora = new_asset(&v, "Fresadora", "213", 6000, date!(2026 - 05 - 01));
        let d0 = date!(2026 - 05 - 10);
        let f = issue(
            &v,
            &sale_lines(
                &[
                    ("Torno y fresadora usados", 8000, iva21()),
                    ("Transporte (suplido)", 3000, transport),
                ],
                d0,
            ),
            at(d0),
            true,
        )
        .unwrap();
        dispose_asset_with_invoice(&v, torno, d0, eur(3000), Some(f.id), at(d0)).unwrap();
        dispose_asset_with_invoice(&v, fresadora, d0, eur(5000), None, at(d0)).unwrap();
        let d = models::draft(&v, "390", 2026, 0).unwrap();
        assert_eq!(get(&d, 99), eur(ops));
        assert_eq!(get(&d, 107), eur(3000));
        assert_eq!(get(&d, 108), eur(volume));
        let w = avisos(&d, "Venta de Fresadora");
        assert!(
            w.len() == 1
                && w[0].contains("5.000,00 € sin vincular")
                && w[0].contains("vincula también A2026-0001 a Fresadora")
                && !w[0].contains("Sin factura"),
            "{:?}",
            d.warnings
        );
    }
}

/// Ventas sin vincular facturadas en una línea con otra (el terreno y la construcción de un local;
/// un torno y una fresadora) o con un descuento en otra línea: la línea que las nombra y suma sus
/// precios es su factura.
#[test]
fn ventas_sin_vincular_en_una_línea_o_con_descuento() {
    use taxpain_core::invoice::Exemption;
    // Local de la calle Mayor 1: terreno (210) 20.000 y construcción (211) 40.000, vendidos el
    // 15/09/2026 en una línea exenta de 60.000.
    let v = setup();
    sell_asset(
        &v,
        "Terreno del local",
        "210",
        30_000,
        20_000,
        date!(2026 - 09 - 15),
    );
    sell_asset(
        &v,
        "Construcción del local",
        "211",
        80_000,
        40_000,
        date!(2026 - 09 - 15),
    );
    emit_sale(
        &v,
        "Venta del local de la calle Mayor 1",
        60_000,
        VatTreatment::Exempt(Exemption::E1),
        date!(2026 - 09 - 15),
    );
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 105), eur(60_000));
    assert_eq!(get(&d, 106), eur(60_000));
    assert_eq!(get(&d, 108), Money::ZERO);
    assert!(
        !d.warnings.iter().any(|w| w.contains("Sin factura")),
        "{:?}",
        d.warnings
    );
    // Torno 3.000 y fresadora 5.000 en «Torno y fresadora usados» por 8.000.
    let v = setup();
    sell_asset(&v, "Torno", "213", 12_000, 3000, date!(2026 - 05 - 10));
    sell_asset(&v, "Fresadora", "213", 6000, 5000, date!(2026 - 05 - 10));
    emit_sale(
        &v,
        "Torno y fresadora usados",
        8000,
        iva21(),
        date!(2026 - 05 - 10),
    );
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 107), eur(8000));
    assert_eq!(get(&d, 108), Money::ZERO);
    // Torno vendido por 4.500: «Torno usado» 5.000 y «Descuento» −500.
    let v = setup();
    sell_asset(&v, "Torno", "213", 12_000, 4500, date!(2026 - 05 - 10));
    issue(
        &v,
        &sale_lines(
            &[("Torno usado", 5000, iva21()), ("Descuento", -500, iva21())],
            date!(2026 - 05 - 10),
        ),
        at(date!(2026 - 05 - 10)),
        true,
    )
    .unwrap();
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), eur(4500));
    assert_eq!(get(&d, 107), eur(4500));
    assert_eq!(get(&d, 108), Money::ZERO);
}

/// Una venta sin vincular por 3.000 con una factura de 3.000 y un descuento de 300 sólo resta lo
/// que está en el volumen (2.700).
#[test]
fn venta_sin_vincular_con_un_descuento_en_su_factura() {
    let v = setup();
    sell_asset(&v, "Torno", "213", 12_000, 3000, date!(2026 - 05 - 10));
    issue(
        &v,
        &sale_lines(
            &[
                ("Venta del torno", 3000, iva21()),
                ("Descuento", -300, iva21()),
            ],
            date!(2026 - 05 - 10),
        ),
        at(date!(2026 - 05 - 10)),
        true,
    )
    .unwrap();
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), eur(2700));
    assert_eq!(get(&d, 107), eur(2700));
    assert_eq!(get(&d, 108), Money::ZERO);
}

/// Una línea no sujeta con la base de la venta que no la nombra (un servicio a un cliente de fuera)
/// no es su factura: se avisa de la venta sin factura.
#[test]
fn línea_no_sujeta_ajena_no_calla_el_aviso() {
    let v = setup();
    sell_asset(&v, "Torno", "213", 12_000, 5000, date!(2026 - 05 - 10));
    let mut f = sale_draft("Consultoría de mayo", 5000, n2(), date!(2026 - 05 - 30));
    f.recipient = Some(Party {
        id: PartyId::Foreign {
            country: "US".into(),
            id_type: taxpain_core::invoice::IdType::Otro,
            id: "98-7654321".into(),
        },
        name: "Client Inc".into(),
        address: addr(),
        person: None,
    });
    issue(&v, &f, at(date!(2026 - 05 - 30)), true).unwrap();
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 110), eur(5000));
    assert_eq!(get(&d, 107), Money::ZERO);
    let w = avisos(&d, "Venta de Torno del 10/05/2026 por 5.000,00 €");
    assert!(
        w.len() == 1 && w[0].contains("«Consultoría de mayo»") && w[0].contains("no sujeta"),
        "{:?}",
        d.warnings
    );
}

/// Una factura de antes del alta del elemento no es su venta (TaxPain no deja vincularla).
#[test]
fn factura_de_antes_del_alta_no_es_la_venta() {
    use taxpain_app::accounting::{add_fixed_asset, dispose_asset};
    use taxpain_core::accounting::depreciation::FixedAsset;
    let v = setup();
    emit_sale(
        &v,
        "Venta de furgoneta",
        5000,
        iva21(),
        date!(2026 - 03 - 20),
    );
    let a = FixedAsset {
        id: 0,
        name: "Furgoneta".into(),
        account: acc("218"),
        depreciation_account: acc("2818"),
        expense_account: acc("681"),
        acquired_on: date!(2026 - 04 - 01),
        cost: eur(20_000),
        residual: Money::ZERO,
        rate: Rate::percent(10),
        disposed_on: None,
        method: Default::default(),
        tax_rate: None,
    };
    let id = add_fixed_asset(&v, &a, at(date!(2026 - 04 - 01))).unwrap();
    dispose_asset(
        &v,
        id,
        date!(2026 - 04 - 20),
        eur(5000),
        at(date!(2026 - 04 - 20)),
    )
    .unwrap();
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 107), Money::ZERO);
    assert!(
        avisos(&d, "Venta de Furgoneta")
            .iter()
            .any(|w| w.contains("ninguna factura")),
        "{:?}",
        d.warnings
    );
    assert!(
        avisos(&d, "Ventas del inmovilizado sin vincular").is_empty(),
        "{:?}",
        d.warnings
    );
}

/// Dos ventas y dos facturas con la misma base: cada venta con una, aunque la más cercana a la
/// primera sea la de la segunda.
#[test]
fn dos_ventas_con_dos_facturas_de_la_misma_base() {
    let v = setup();
    emit_sale(&v, "Maquinaria usada", 5000, iva21(), date!(2026 - 02 - 01));
    emit_sale(&v, "Maquinaria usada", 5000, iva21(), date!(2026 - 03 - 11));
    sell_asset(&v, "Torno", "213", 12_000, 5000, date!(2026 - 03 - 01));
    sell_asset(&v, "Fresadora", "213", 8000, 5000, date!(2026 - 04 - 20));
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), eur(10_000));
    assert_eq!(get(&d, 107), eur(10_000));
    assert_eq!(get(&d, 108), Money::ZERO);
    assert_eq!(
        d.details,
        vec![
            (
                "[107] Venta de Torno (213) del 01/03/2026 · factura A2026-0001".to_string(),
                eur(5000)
            ),
            (
                "[107] Venta de Fresadora (213) del 20/04/2026 · factura A2026-0002".to_string(),
                eur(5000)
            ),
        ]
    );
}

/// Los avisos de las rectificativas: en plural si son dos, y el de comprobar la línea con lo que
/// se resta.
#[test]
fn avisos_de_dos_rectificativas_y_de_lo_que_se_resta() {
    use taxpain_app::accounting::dispose_asset;
    use taxpain_app::venta_inmovilizado::link_sale_invoice;
    let v = setup();
    sell_asset(&v, "Torno", "213", 12_000, 5000, date!(2026 - 05 - 10));
    emit_sale(&v, "Venta del torno", 5000, iva21(), date!(2026 - 05 - 10));
    rectify_sale(
        &v,
        ("A2026-0001", date!(2026 - 05 - 10)),
        "Descuento",
        -300,
        date!(2026 - 06 - 01),
    );
    rectify_sale(
        &v,
        ("A2026-0001", date!(2026 - 05 - 10)),
        "Descuento",
        -200,
        date!(2026 - 07 - 01),
    );
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 107), eur(4500));
    assert!(
        avisos(&d, "Venta de Torno").iter().any(|w| w.contains(
            "tiene las rectificativas R2026-0001 y R2026-0002; de la venta quedan 4.500,00 €"
        )),
        "{:?}",
        d.warnings
    );
    // Furgoneta vinculada a toda A2026-0001 (vehículo 10.000 y mantenimiento 12.000) y rebajada
    // 1.000: se restan 11.000, y eso dice el aviso de comprobar la línea.
    let v = setup();
    let furgoneta = new_asset(&v, "Furgoneta", "218", 20_000, date!(2026 - 03 - 01));
    let d0 = date!(2026 - 03 - 10);
    dispose_asset(&v, furgoneta, d0, Money::ZERO, at(d0)).unwrap();
    let f = issue(
        &v,
        &sale_lines(
            &[
                ("Vehículo usado 1234-ABC", 10_000, iva21()),
                ("Mantenimiento anual", 12_000, iva21()),
            ],
            d0,
        ),
        at(d0),
        true,
    )
    .unwrap();
    link_sale_invoice(&v, furgoneta, f.id, None, at(d0)).unwrap();
    rectify_sale(
        &v,
        (&f.number, d0),
        "Descuento",
        -1000,
        date!(2026 - 04 - 15),
    );
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 107), eur(11_000));
    let w: Vec<_> = avisos(&d, "Venta de Furgoneta")
        .into_iter()
        .filter(|w| w.contains("comprueba"))
        .collect();
    assert!(
        w.len() == 1 && w[0].contains("11.000,00 €") && !w[0].contains("12.000,00 €"),
        "{:?}",
        d.warnings
    );
}

/// Vinculada a toda su factura (la furgoneta y una línea de estanterías que no la nombra), la
/// venta se resta entera: lo vinculado de más está en una línea sujeta que no es de otra venta.
#[test]
fn venta_vinculada_a_toda_su_factura_con_una_línea_que_no_la_nombra() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    let v = setup();
    let furgoneta = new_asset(&v, "Furgoneta", "218", 20_000, date!(2026 - 03 - 01));
    let d0 = date!(2026 - 03 - 10);
    let f = issue(
        &v,
        &sale_lines(
            &[
                ("Furgoneta Renault 1234-ABC", 10_000, iva21()),
                ("Estanterías y rótulos", 800, iva21()),
            ],
            d0,
        ),
        at(d0),
        true,
    )
    .unwrap();
    dispose_asset_with_invoice(&v, furgoneta, d0, eur(10_800), Some(f.id), at(d0)).unwrap();
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), eur(10_800));
    assert_eq!(get(&d, 107), eur(10_800));
    assert_eq!(get(&d, 108), Money::ZERO);
    assert!(
        avisos(&d, "Venta de Furgoneta").is_empty(),
        "{:?}",
        d.warnings
    );
}

/// Dos ventas vinculadas no se restan con la misma línea: la que tiene su base es de la
/// furgoneta, y el torno, vinculado por más de lo que queda sujeto, va en la no sujeta. Igual en
/// los dos órdenes de alta.
#[test]
fn dos_ventas_vinculadas_no_se_restan_con_la_misma_línea() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    // A2026-0001 (30/06/2026): unidad productiva (art. 7.1.º LIVA, no sujeta) 40.000 y un vehículo
    // 10.000 al 21 %. El torno se vincula por 25.000 y la furgoneta por 10.000.
    for torno_first in [true, false] {
        let v = setup();
        let d0 = date!(2026 - 06 - 30);
        let assets = [
            ("Torno", "213", 30_000, 25_000),
            ("Furgoneta", "218", 20_000, 10_000),
        ];
        let order: Vec<_> = if torno_first {
            assets.to_vec()
        } else {
            assets.iter().rev().copied().collect()
        };
        let ids: Vec<(i64, i64)> = order
            .iter()
            .map(|(name, account, cost, price)| {
                (
                    new_asset(&v, name, account, *cost, date!(2026 - 06 - 01)),
                    *price,
                )
            })
            .collect();
        let f = issue(
            &v,
            &sale_lines(
                &[
                    ("Unidad productiva (art. 7.1.º LIVA)", 40_000, n1()),
                    ("Vehículo usado 1234-ABC", 10_000, iva21()),
                ],
                d0,
            ),
            at(d0),
            true,
        )
        .unwrap();
        for (id, price) in ids {
            dispose_asset_with_invoice(&v, id, d0, eur(price), Some(f.id), at(d0)).unwrap();
        }
        let d = models::draft(&v, "390", 2026, 0).unwrap();
        assert_eq!(get(&d, 99), eur(10_000), "{torno_first}");
        assert_eq!(get(&d, 107), eur(10_000), "{torno_first}");
        assert_eq!(get(&d, 108), Money::ZERO, "{torno_first}");
        assert_eq!(
            d.details,
            vec![(
                "[107] Venta de Furgoneta (218) del 30/06/2026 · factura A2026-0001".to_string(),
                eur(10_000)
            )],
            "{torno_first}"
        );
        let q4 = exonerado_4t(&v, 2026);
        assert_eq!(get(&q4, 88), Money::ZERO, "{torno_first}");
    }
}

/// Dos ventas vinculadas por la misma base a una factura con dos líneas de esa base que no las
/// nombran: el resultado no depende del orden de alta, y se pide comprobarlo.
#[test]
fn dos_líneas_con_la_base_vinculada_que_no_nombran_el_elemento() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    use taxpain_core::invoice::Exemption;
    // A2026-0001 (30/06/2026): «Lote 1» 50.000 exento y «Lote 2» 50.000 no sujeto. La nave (211)
    // y `other` (el programa, 206, o la prensa, 213) se vinculan por 50.000 cada una.
    let sold = |other: (&str, &str), nave_first: bool| {
        let v = setup();
        let d0 = date!(2026 - 06 - 30);
        let assets = [("Nave", "211", 150_000), (other.0, other.1, 60_000)];
        let order: Vec<_> = if nave_first {
            assets.to_vec()
        } else {
            assets.iter().rev().copied().collect()
        };
        let ids: Vec<i64> = order
            .iter()
            .map(|(name, account, cost)| new_asset(&v, name, account, *cost, date!(2026 - 06 - 01)))
            .collect();
        let f = issue(
            &v,
            &sale_lines(
                &[
                    ("Lote 1", 50_000, VatTreatment::Exempt(Exemption::E1)),
                    ("Lote 2", 50_000, n2()),
                ],
                d0,
            ),
            at(d0),
            true,
        )
        .unwrap();
        for id in ids {
            dispose_asset_with_invoice(&v, id, d0, eur(50_000), Some(f.id), at(d0)).unwrap();
        }
        models::draft(&v, "390", 2026, 0).unwrap()
    };
    for other in [("Programa", "206"), ("Prensa", "213")] {
        let a = sold(other, true);
        let b = sold(other, false);
        for b_ in [105, 106, 107, 108, 110] {
            assert_eq!(get(&a, b_), get(&b, b_), "{other:?} [{b_}]");
        }
        assert_eq!(a.details, b.details, "{other:?}");
        for d in [&a, &b] {
            assert!(
                d.warnings.iter().any(|w| w.contains("comprueba")),
                "{other:?}: {:?}",
                d.warnings
            );
        }
    }
}

/// El aviso del reparto de una rectificativa nombra sólo las ventas entre las que se reparte, no
/// la que ya tiene su parte.
#[test]
fn el_aviso_del_reparto_sólo_nombra_las_ventas_que_lo_reciben() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    use taxpain_app::venta_inmovilizado::link_sale_invoice;
    let v = setup();
    let d0 = date!(2026 - 05 - 10);
    let ids: Vec<(i64, i64)> = [
        ("Torno", 12_000, 3000),
        ("Fresadora", 6000, 5000),
        ("Prensa", 8000, 4000),
    ]
    .iter()
    .map(|(name, cost, price)| {
        (
            new_asset(&v, name, "213", *cost, date!(2026 - 05 - 01)),
            *price,
        )
    })
    .collect();
    let f = issue(
        &v,
        &sale_lines(
            &[
                ("Torno", 3000, iva21()),
                ("Fresadora", 5000, iva21()),
                ("Prensa", 4000, iva21()),
            ],
            d0,
        ),
        at(d0),
        true,
    )
    .unwrap();
    for (id, price) in &ids {
        dispose_asset_with_invoice(&v, *id, d0, eur(*price), Some(f.id), at(d0)).unwrap();
    }
    let d1 = date!(2026 - 06 - 01);
    let r = rectify_lines(
        &v,
        &[(&f.number, d0)],
        &[
            ("Descuento de la prensa", -1000, iva21()),
            ("Descuento", -200, iva21()),
        ],
        d1,
    );
    link_sale_invoice(&v, ids[2].0, r.id, Some(-eur(1000)), at(d1)).unwrap();
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 107), eur(10_800));
    let w = avisos(&d, "La rectificativa R2026-0001");
    assert!(
        w.len() == 1 && w[0].contains("que vende Torno y Fresadora,") && !w[0].contains("Prensa"),
        "{:?}",
        d.warnings
    );
}

/// La factura final de una venta con su anticipo del año anterior sólo resta lo que lleva al
/// volumen (sin el anticipo): [108] no queda en negativo. El anticipo, en el volumen de su año,
/// aún no se resta (pendiente, arts. 75.Dos y 121.Dos LIVA).
#[test]
fn venta_con_el_anticipo_del_año_anterior_no_deja_108_en_negativo() {
    let v = setup();
    let mut anticipo = sale_draft("Anticipo del torno", 2000, iva21(), date!(2025 - 12 - 10));
    anticipo.lines[0].advance = true;
    issue(&v, &anticipo, at(date!(2025 - 12 - 10)), true).unwrap();
    sell_asset(&v, "Torno", "213", 12_000, 8000, date!(2026 - 01 - 20));
    let mut f = sale_lines(
        &[
            ("Venta del torno", 8000, iva21()),
            ("Anticipo del torno", -2000, iva21()),
        ],
        date!(2026 - 01 - 20),
    );
    f.lines[1].advance = true;
    issue(&v, &f, at(date!(2026 - 01 - 20)), true).unwrap();
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), eur(6000));
    assert_eq!(get(&d, 107), eur(6000));
    assert_eq!(get(&d, 108), Money::ZERO);
}

/// Sin factura con su base, pero con una del libro de la gestoría cerca (que puede ser la suya con
/// otra base): el aviso dice que TaxPain no puede vincularla y que hay que corregir la casilla.
#[test]
fn venta_sin_factura_con_una_del_libro_de_la_gestoría_cerca() {
    use taxpain_app::libros;
    let v = setup();
    sell_asset(
        &v,
        "Furgoneta",
        "218",
        20_000,
        10_000,
        date!(2026 - 05 - 10),
    );
    let csv = "Fecha Expedición;Número Factura;NIF Destinatario;Nombre Destinatario;Base Imponible;Tipo IVA;Cuota IVA;Total\n\
               12/05/2026;2026-001;B12345674;Cliente SL;10.500,00;21;2.205,00;12.705,00\n";
    let book = libros::parse("libro_emitidas.csv", csv.as_bytes())
        .unwrap()
        .unwrap();
    libros::import_issued(&v, &book, at(date!(2026 - 06 - 01))).unwrap();
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(get(&d, 99), eur(10_500));
    assert_eq!(get(&d, 107), Money::ZERO);
    let w = avisos(&d, "Venta de Furgoneta");
    assert!(
        w.len() == 1
            && w[0].contains("libro de la gestoría")
            && w[0].contains("no puede vincularla")
            && w[0].contains("corrige [107]"),
        "{:?}",
        d.warnings
    );
}

// Hallazgos de la tercera revisión del 390 (31/10/2026): el anticipo del mismo año, lo no sujeto y
// el resto de las rectificativas, y las líneas que nombran otra.

/// Con el anticipo y la factura final en el mismo año, la venta está entera en el volumen de ese
/// año (el anticipo en su factura y el resto en la final) y se resta entera, vinculada o no.
#[test]
fn venta_con_el_anticipo_del_mismo_año_se_resta_entera() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    for linked in [false, true] {
        let v = setup();
        let torno = new_asset(&v, "Torno", "213", 12_000, date!(2026 - 01 - 10));
        let d0 = date!(2026 - 03 - 10);
        let mut anticipo = sale_draft("Anticipo del torno", 2000, iva21(), d0);
        anticipo.lines[0].advance = true;
        issue(&v, &anticipo, at(d0), true).unwrap();
        let d1 = date!(2026 - 04 - 20);
        let mut f = sale_lines(
            &[
                ("Venta del torno", 8000, iva21()),
                ("Anticipo del torno", -2000, iva21()),
            ],
            d1,
        );
        f.lines[1].advance = true;
        let f = issue(&v, &f, at(d1), true).unwrap();
        let invoice = linked.then_some(f.id);
        dispose_asset_with_invoice(&v, torno, d1, eur(8000), invoice, at(d1)).unwrap();
        let d = models::draft(&v, "390", 2026, 0).unwrap();
        assert_eq!(
            (get(&d, 99), get(&d, 107), get(&d, 108)),
            (eur(8000), eur(8000), Money::ZERO),
            "{linked}: {:?}",
            d.warnings
        );
        let q4 = exonerado_4t(&v, 2026);
        assert_eq!(
            (get(&q4, 99), get(&q4, 88)),
            (eur(8000), Money::ZERO),
            "{linked}"
        );
    }
    // Un anticipo de 1.000 en 2025 y otro en 2026, y la final descuenta los dos: en 2026 están
    // el de 2026 y la final, 7.000 de la venta.
    let v = setup();
    for (d, m) in [(date!(2025 - 12 - 10), 1000), (date!(2026 - 02 - 10), 1000)] {
        let mut anticipo = sale_draft("Anticipo del torno", m, iva21(), d);
        anticipo.lines[0].advance = true;
        issue(&v, &anticipo, at(d), true).unwrap();
    }
    let d1 = date!(2026 - 03 - 20);
    sell_asset(&v, "Torno", "213", 12_000, 8000, d1);
    let mut f = sale_lines(
        &[
            ("Venta del torno", 8000, iva21()),
            ("Anticipo del torno", -2000, iva21()),
        ],
        d1,
    );
    f.lines[1].advance = true;
    issue(&v, &f, at(d1), true).unwrap();
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(
        (get(&d, 99), get(&d, 107), get(&d, 108)),
        (eur(7000), eur(7000), Money::ZERO)
    );
}

/// Lo pasado a cada venta que suma unas líneas de la rectificativa es lo de esas líneas: la
/// anulación no sujeta de la grúa no se lleva parte de la rebaja de la furgoneta, ni ésta parte de
/// los portes o de la notaría, que no ha cambiado.
#[test]
fn rectificativa_pasada_por_sus_líneas_sujetas_y_no_sujetas() {
    use taxpain_app::accounting::{dispose_asset, dispose_asset_with_invoice};
    use taxpain_app::venta_inmovilizado::link_sale_invoice;
    use taxpain_core::invoice::Exemption;
    let (d0, d1) = (date!(2026 - 03 - 10), date!(2026 - 04 - 15));
    // A2026-0001: la grúa (no sujeta, en Portugal) 7.000, la furgoneta 10.000, la tasa de la DGT 150
    // (suplido) y el mantenimiento 2.000. La grúa se da de baja con ella por 7.000; la furgoneta,
    // sin precio y vinculada por 10.150 (con la tasa). R2026-0001 anula la grúa y rebaja 1.000 la
    // furgoneta, y a cada venta se le pasa lo suyo.
    let v = setup();
    let grua = new_asset(&v, "Grúa", "213", 20_000, d0);
    let furgoneta = new_asset(&v, "Furgoneta", "218", 20_000, d0);
    let f = issue(
        &v,
        &sale_lines(
            &[
                ("Venta de la grúa", 7000, n2()),
                ("Furgoneta", 10_000, iva21()),
                ("Tasa DGT (suplido)", 150, n1()),
                ("Mantenimiento anual", 2000, iva21()),
            ],
            d0,
        ),
        at(d0),
        true,
    )
    .unwrap();
    dispose_asset_with_invoice(&v, grua, d0, eur(7000), Some(f.id), at(d0)).unwrap();
    dispose_asset(&v, furgoneta, d0, Money::ZERO, at(d0)).unwrap();
    link_sale_invoice(&v, furgoneta, f.id, Some(eur(10_150)), at(d0)).unwrap();
    let r = rectify_lines(
        &v,
        &[(&f.number, d0)],
        &[
            ("Anulación de la grúa", -7000, n2()),
            ("Rebaja de la furgoneta", -1000, iva21()),
        ],
        d1,
    );
    link_sale_invoice(&v, grua, r.id, Some(-eur(7000)), at(d1)).unwrap();
    link_sale_invoice(&v, furgoneta, r.id, Some(-eur(1000)), at(d1)).unwrap();
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(
        (get(&d, 99), get(&d, 110), get(&d, 107), get(&d, 108)),
        (eur(11_000), Money::ZERO, eur(9000), eur(2000))
    );
    let q4 = exonerado_4t(&v, 2026);
    assert_eq!((get(&q4, 99), get(&q4, 88)), (eur(9000), eur(2000)));

    // La furgoneta 10.000, la tasa 150, los portes a Oporto 300 (no sujetos) y, si `extra`, el
    // mantenimiento 2.000; R2026-0001 rebaja 1.000 la furgoneta (que se le pasa) y devuelve los
    // portes.
    for extra in [true, false] {
        let v = setup();
        let furgoneta = new_asset(&v, "Furgoneta", "218", 20_000, d0);
        let mut lines = vec![
            ("Furgoneta", 10_000, iva21()),
            ("Tasa DGT (suplido)", 150, n1()),
            ("Portes a Oporto", 300, n2()),
        ];
        if extra {
            lines.push(("Mantenimiento anual", 2000, iva21()));
        }
        let f = issue(&v, &sale_lines(&lines, d0), at(d0), true).unwrap();
        dispose_asset(&v, furgoneta, d0, Money::ZERO, at(d0)).unwrap();
        link_sale_invoice(&v, furgoneta, f.id, Some(eur(10_150)), at(d0)).unwrap();
        let r = rectify_lines(
            &v,
            &[(&f.number, d0)],
            &[
                ("Rebaja de la furgoneta", -1000, iva21()),
                ("Portes a Oporto", -300, n2()),
            ],
            d1,
        );
        link_sale_invoice(&v, furgoneta, r.id, Some(-eur(1000)), at(d1)).unwrap();
        let d = models::draft(&v, "390", 2026, 0).unwrap();
        let rest = if extra { eur(2000) } else { Money::ZERO };
        assert_eq!((get(&d, 107), get(&d, 108)), (eur(9000), rest), "{extra}");
        assert!(
            !d.warnings.iter().any(|w| w.contains("sólo se ha pasado")),
            "{extra}: {:?}",
            d.warnings
        );
    }

    // El local de la calle Mayor 1 (exento) 60.000 con los gastos de notaría 300 (suplido) y la
    // furgoneta 10.000 con la tasa 150, vinculados por 60.300 y 10.150; R2026-0001 devuelve la
    // notaría (pasada al local) y rebaja 1.000 la furgoneta (pasada a ella).
    let v = setup();
    let local = new_asset(&v, "Local", "211", 80_000, d0);
    let furgoneta = new_asset(&v, "Furgoneta", "218", 20_000, d0);
    let f = issue(
        &v,
        &sale_lines(
            &[
                (
                    "Local de la calle Mayor 1",
                    60_000,
                    VatTreatment::Exempt(Exemption::E1),
                ),
                ("Gastos de notaría (suplido)", 300, n1()),
                ("Furgoneta", 10_000, iva21()),
                ("Tasa DGT (suplido)", 150, n1()),
            ],
            d0,
        ),
        at(d0),
        true,
    )
    .unwrap();
    for (id, amount) in [(local, 60_300), (furgoneta, 10_150)] {
        dispose_asset(&v, id, d0, Money::ZERO, at(d0)).unwrap();
        link_sale_invoice(&v, id, f.id, Some(eur(amount)), at(d0)).unwrap();
    }
    let r = rectify_lines(
        &v,
        &[(&f.number, d0)],
        &[
            ("Devolución de los gastos de notaría", -300, n1()),
            ("Rebaja de la furgoneta", -1000, iva21()),
        ],
        d1,
    );
    link_sale_invoice(&v, local, r.id, Some(-eur(300)), at(d1)).unwrap();
    link_sale_invoice(&v, furgoneta, r.id, Some(-eur(1000)), at(d1)).unwrap();
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!((get(&d, 106), get(&d, 107)), (eur(60_000), eur(9000)));
    let q4 = exonerado_4t(&v, 2026);
    assert_eq!((get(&q4, 79), get(&q4, 99)), (eur(60_000), eur(9000)));
}

/// La rectificativa pasada en parte (a la fresadora) cuyo resto no cabe en la venta sin pasar (el
/// torno, anulado): lo que no cabe es de las otras líneas de la factura, no de lo pasado.
#[test]
fn rectificativa_cuyo_resto_no_cabe_en_la_venta_sin_pasar() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    use taxpain_app::venta_inmovilizado::link_sale_invoice;
    // A2026-0001 (10/05/2026): torno 3.000, fresadora 5.000 y consultoría 2.000, con las dos
    // ventas vinculadas. R2026-0001 (01/06) anula el torno, rebaja 1.000 la fresadora (que se le
    // pasa) y 1.000 con la línea `third`, que nombra la consultoría o nada.
    for third in ["Descuento de la consultoría", "Rápel"] {
        let v = setup();
        let d0 = date!(2026 - 05 - 10);
        let torno = new_asset(&v, "Torno", "213", 12_000, d0);
        let fresadora = new_asset(&v, "Fresadora", "213", 6000, d0);
        let f = issue(
            &v,
            &sale_lines(
                &[
                    ("Torno usado", 3000, iva21()),
                    ("Fresadora usada", 5000, iva21()),
                    ("Consultoría", 2000, iva21()),
                ],
                d0,
            ),
            at(d0),
            true,
        )
        .unwrap();
        dispose_asset_with_invoice(&v, torno, d0, eur(3000), Some(f.id), at(d0)).unwrap();
        dispose_asset_with_invoice(&v, fresadora, d0, eur(5000), Some(f.id), at(d0)).unwrap();
        let d1 = date!(2026 - 06 - 01);
        let r = rectify_lines(
            &v,
            &[(&f.number, d0)],
            &[
                ("Anulación del torno", -3000, iva21()),
                ("Descuento de la fresadora", -1000, iva21()),
                (third, -1000, iva21()),
            ],
            d1,
        );
        link_sale_invoice(&v, fresadora, r.id, Some(-eur(1000)), at(d1)).unwrap();
        let d = models::draft(&v, "390", 2026, 0).unwrap();
        assert_eq!(
            (get(&d, 99), get(&d, 107), get(&d, 108)),
            (eur(5000), eur(4000), eur(1000)),
            "{third}"
        );
        assert_eq!(
            d.details,
            vec![(
                "[107] Venta de Fresadora (213) del 10/05/2026 · factura A2026-0001".to_string(),
                eur(4000)
            )],
            "{third}"
        );
    }
}

/// Rectificativa de dos facturas con la parte de una pasada a su venta: lo que falta es de la
/// otra, porque la primera no tiene más líneas que esa venta.
#[test]
fn rectificativa_de_dos_facturas_con_la_parte_de_una_pasada() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    use taxpain_app::venta_inmovilizado::link_sale_invoice;
    use taxpain_core::invoice::Exemption;
    // A2026-0001 vende el local (exento) por 60.000 y A2026-0002 el remolque por 4.000, las dos el
    // 10/05/2026; el local, vinculado, y el remolque, si `linked`. R2026-0001 (01/06) rebaja las
    // dos, y al local se le pasa su parte.
    for linked in [true, false] {
        let v = setup();
        let d0 = date!(2026 - 05 - 10);
        let local = new_asset(&v, "Local", "211", 80_000, d0);
        let remolque = new_asset(&v, "Remolque", "218", 9000, d0);
        let exempt = VatTreatment::Exempt(Exemption::E1);
        let a = issue(
            &v,
            &sale_draft("Venta del local", 60_000, exempt, d0),
            at(d0),
            true,
        )
        .unwrap();
        let b = issue(
            &v,
            &sale_draft("Venta del remolque", 4000, iva21(), d0),
            at(d0),
            true,
        )
        .unwrap();
        dispose_asset_with_invoice(&v, local, d0, eur(60_000), Some(a.id), at(d0)).unwrap();
        let invoice = linked.then_some(b.id);
        dispose_asset_with_invoice(&v, remolque, d0, eur(4000), invoice, at(d0)).unwrap();
        let d1 = date!(2026 - 06 - 01);
        let r = rectify_lines(
            &v,
            &[(&a.number, d0), (&b.number, d0)],
            &[
                ("Descuento del local", -3000, exempt),
                ("Descuento del remolque", -400, iva21()),
            ],
            d1,
        );
        link_sale_invoice(&v, local, r.id, Some(-eur(3000)), at(d1)).unwrap();
        let d = models::draft(&v, "390", 2026, 0).unwrap();
        assert_eq!(
            (get(&d, 105), get(&d, 106), get(&d, 107), get(&d, 108)),
            (eur(57_000), eur(57_000), eur(3600), Money::ZERO),
            "{linked}"
        );
        assert!(
            !d.warnings
                .iter()
                .any(|w| w.contains("sólo se ha pasado") || w.contains("rectifica varias")),
            "{linked}: {:?}",
            d.warnings
        );
    }
}

/// Un descuento de la factura que nombra otra línea (la consultoría, el mantenimiento) rebaja esa
/// línea y no la de la venta (art. 78.Tres.2.º LIVA).
#[test]
fn descuento_que_nombra_otra_línea_de_la_factura() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    let d0 = date!(2026 - 05 - 10);
    // Torno vendido por 3.000 sin vincular, con la consultoría y su descuento en la factura.
    let v = setup();
    sell_asset(&v, "Torno", "213", 12_000, 3000, d0);
    issue(
        &v,
        &sale_lines(
            &[
                ("Torno usado", 3000, iva21()),
                ("Consultoría", 1000, iva21()),
                ("Descuento de la consultoría", -500, iva21()),
            ],
            d0,
        ),
        at(d0),
        true,
    )
    .unwrap();
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(
        (get(&d, 99), get(&d, 107), get(&d, 108)),
        (eur(3500), eur(3000), eur(500))
    );
    // Furgoneta vinculada por 10.000, con el mantenimiento y su descuento: sin aviso.
    let v = setup();
    let furgoneta = new_asset(&v, "Furgoneta", "218", 20_000, d0);
    let f = issue(
        &v,
        &sale_lines(
            &[
                ("Furgoneta 1234-ABC", 10_000, iva21()),
                ("Mantenimiento", 2000, iva21()),
                ("Descuento del mantenimiento", -500, iva21()),
            ],
            d0,
        ),
        at(d0),
        true,
    )
    .unwrap();
    dispose_asset_with_invoice(&v, furgoneta, d0, eur(10_000), Some(f.id), at(d0)).unwrap();
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(
        (get(&d, 99), get(&d, 107), get(&d, 108)),
        (eur(11_500), eur(10_000), eur(1500))
    );
    assert!(
        avisos(&d, "Venta de Furgoneta").is_empty(),
        "{:?}",
        d.warnings
    );
    // El descuento que nombra el torno es suyo: vendido por 2.700, su línea con el descuento.
    let v = setup();
    sell_asset(&v, "Torno", "213", 12_000, 2700, d0);
    issue(
        &v,
        &sale_lines(
            &[
                ("Torno usado", 3000, iva21()),
                ("Consultoría", 1000, iva21()),
                ("Descuento del torno", -300, iva21()),
            ],
            d0,
        ),
        at(d0),
        true,
    )
    .unwrap();
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(
        (get(&d, 99), get(&d, 107), get(&d, 108)),
        (eur(3700), eur(2700), eur(1000))
    );
}

/// La rectificativa sin pasar que sólo nombra otra línea de la factura de la venta (la
/// consultoría) no cambia la venta, ni si la sube ni si la baja; si es del año siguiente, allí no
/// hay nada que compensar.
#[test]
fn rectificativa_sin_pasar_de_otra_línea_de_la_factura() {
    use taxpain_app::accounting::dispose_asset_with_invoice;
    let d0 = date!(2026 - 05 - 10);
    // Torno vendido por 3.000 con A2026-0001 (el torno y la consultoría 1.000), vinculada si
    // `linked`, y la rectificativa «`what`» de `amount` del día `back`.
    let sold = |linked: bool, what: &str, amount: i64, back: Date| {
        let v = setup();
        let torno = new_asset(&v, "Torno", "213", 12_000, d0);
        let f = issue(
            &v,
            &sale_lines(
                &[
                    ("Torno usado", 3000, iva21()),
                    ("Consultoría", 1000, iva21()),
                ],
                d0,
            ),
            at(d0),
            true,
        )
        .unwrap();
        let invoice = linked.then_some(f.id);
        dispose_asset_with_invoice(&v, torno, d0, eur(3000), invoice, at(d0)).unwrap();
        rectify_sale(&v, (&f.number, d0), what, amount, back);
        v
    };
    let boxes = |v: &Vault, year: i32| {
        let d = models::draft(v, "390", year, 0).unwrap();
        (get(&d, 99), get(&d, 107), get(&d, 108))
    };
    let june = date!(2026 - 06 - 01);
    for linked in [false, true] {
        let v = sold(linked, "Ampliación de la consultoría", 500, june);
        assert_eq!(
            boxes(&v, 2026),
            (eur(4500), eur(3000), eur(1500)),
            "{linked}"
        );
        let v = sold(linked, "Descuento de la consultoría", -500, june);
        assert_eq!(
            boxes(&v, 2026),
            (eur(3500), eur(3000), eur(500)),
            "{linked}"
        );
        let v = sold(
            linked,
            "Ampliación de la consultoría",
            500,
            date!(2027 - 02 - 15),
        );
        assert_eq!(
            boxes(&v, 2026),
            (eur(4000), eur(3000), eur(1000)),
            "{linked}"
        );
        assert_eq!(
            boxes(&v, 2027),
            (eur(500), Money::ZERO, eur(500)),
            "{linked}"
        );
    }
}

/// La línea que nombra el terreno y la construcción del local y suma sus precios es de las dos
/// ventas, aunque otra factura tenga una línea con la base del terreno.
#[test]
fn línea_que_nombra_dos_ventas_gana_a_otra_con_la_base_de_una() {
    use taxpain_core::invoice::Exemption;
    let v = setup();
    let d0 = date!(2026 - 09 - 15);
    sell_asset(&v, "Terreno del local", "210", 30_000, 20_000, d0);
    sell_asset(&v, "Construcción del local", "211", 80_000, 40_000, d0);
    emit_sale(
        &v,
        "Venta del local de la calle Mayor 1",
        60_000,
        VatTreatment::Exempt(Exemption::E1),
        d0,
    );
    emit_sale(
        &v,
        "Consultoría de septiembre",
        20_000,
        iva21(),
        date!(2026 - 09 - 30),
    );
    let d = models::draft(&v, "390", 2026, 0).unwrap();
    assert_eq!(
        (get(&d, 99), get(&d, 105), get(&d, 106), get(&d, 108)),
        (eur(20_000), eur(60_000), eur(60_000), eur(20_000)),
        "{:?}",
        d.warnings
    );
    assert_eq!(
        d.details,
        vec![
            (
                "[106] Venta de Terreno del local (210) del 15/09/2026 · factura A2026-0001"
                    .to_string(),
                eur(20_000)
            ),
            (
                "[106] Venta de Construcción del local (211) del 15/09/2026 · factura A2026-0001"
                    .to_string(),
                eur(40_000)
            ),
        ]
    );
}

/// Los avisos del reparto de una rectificativa entre ventas sin vincular cuya factura es de un
/// ejercicio cerrado dicen que hay que reabrirlo para vincularla: TaxPain no lo deja.
#[test]
fn aviso_del_reparto_con_la_factura_de_las_ventas_en_un_ejercicio_cerrado() {
    use taxpain_app::accounting::{close_year, dispose_asset_with_invoice};
    use taxpain_app::venta_inmovilizado::link_sale_invoice;
    let d0 = date!(2026 - 05 - 10);
    let back = date!(2027 - 02 - 15);
    // Torno 3.000 y fresadora 5.000 vendidos el 10/05/2026 sin vincular, en una factura o en dos
    // (`two`); 2023 a 2026 cerrados y R2027-0001 de −800 que rectifica todas.
    for two in [false, true] {
        let v = setup();
        let torno = new_asset(&v, "Torno", "213", 12_000, d0);
        let fresadora = new_asset(&v, "Fresadora", "213", 6000, d0);
        let numbers: Vec<String> = if two {
            [("Torno usado", 3000), ("Fresadora usada", 5000)]
                .iter()
                .map(|(what, base)| {
                    issue(&v, &sale_draft(what, *base, iva21(), d0), at(d0), true)
                        .unwrap()
                        .number
                })
                .collect()
        } else {
            let lines = [
                ("Torno usado", 3000, iva21()),
                ("Fresadora usada", 5000, iva21()),
            ];
            vec![
                issue(&v, &sale_lines(&lines, d0), at(d0), true)
                    .unwrap()
                    .number,
            ]
        };
        dispose_asset_with_invoice(&v, torno, d0, eur(3000), None, at(d0)).unwrap();
        dispose_asset_with_invoice(&v, fresadora, d0, eur(5000), None, at(d0)).unwrap();
        for year in 2023..=2026 {
            close_year(&v, year, at(date!(2027 - 01 - 31)), true).unwrap();
        }
        let of: Vec<(&str, Date)> = numbers.iter().map(|n| (n.as_str(), d0)).collect();
        rectify_lines(&v, &of, &[("Descuento", -800, iva21())], back);
        let first: i64 = v
            .connection()
            .query_row(
                "SELECT id FROM invoice_issued WHERE number = ?1",
                [&numbers[0]],
                |r| r.get(0),
            )
            .unwrap();
        let e = link_sale_invoice(&v, torno, first, None, at(back)).unwrap_err();
        assert!(e.to_string().contains("cerrado"), "{e}");
        let d = models::draft(&v, "390", 2027, 0).unwrap();
        assert_eq!(get(&d, 107), eur(-800), "{two}");
        let w = avisos(&d, "La rectificativa R2027-0001");
        assert!(
            w.len() == 1
                && w[0].contains("el ejercicio 2026 está cerrado")
                && w[0].contains("reábrelo")
                && !w[0].contains("Vincula a cada venta su factura y pásale"),
            "{two}: {:?}",
            d.warnings
        );
    }
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
