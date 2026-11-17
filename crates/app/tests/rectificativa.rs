//! Rectificativa y complementaria de lo presentado (carencia G02): el recálculo con la
//! contabilidad de hoy, la diferencia, el recargo y el fichero con las marcas. Datos inventados.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;

use taxpain_app::invoicing::{self, InvoiceDraft, cancel, issue};
use taxpain_app::presentado_aeat::{self, Answer, Presented};
use taxpain_app::received::{Nature, Reviewed, Source, ingest_file, review};
use taxpain_app::rectificativa::{self, Opciones};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::{models, presentados};
use taxpain_core::invoice::{InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment};
use taxpain_core::money::{Money, Rate};
use taxpain_models::dr::PageReader;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime, Time};

const NOW: OffsetDateTime = datetime!(2026-01-02 9:00 UTC);

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn at(d: Date) -> OffsetDateTime {
    d.with_time(Time::from_hms(10, 0, 0).unwrap()).assume_utc()
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
    invoicing::save_settings(&v, &s, NOW).unwrap();
    v
}

/// Venta de `euros` + 21 % el día `d`; devuelve su id.
fn emit(v: &Vault, euros: i64, d: Date) -> i64 {
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
            description: "Diseño".into(),
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
    issue(v, &draft, at(d), true).unwrap().id
}

/// Factura de un profesional con retención (va al 111 del trimestre).
fn profesional(v: &Vault, number: &str, base: i64, wh: i64, d: Date) {
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
            supplier_nif: "12345678Z".into(),
            supplier_name: "Carmen Española Española".into(),
            number: number.into(),
            issue_date: d,
            base: eur(base),
            vat: eur(base * 21 / 100),
            withholding: eur(wh),
            total: eur(base + base * 21 / 100 - wh),
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
        at(d),
    )
    .unwrap();
}

/// Da por presentado `model` del `period` con las casillas `boxes`: tecleadas en «Modelos
/// presentados» y con su justificante según la AEAT.
fn presentar(v: &Vault, model: &str, period: &str, boxes: &[(u16, Money)], receipt: &str) {
    let b: BTreeMap<u16, Money> = boxes.iter().copied().collect();
    presentados::save_manual(v, model, 2026, period, &b, NOW).unwrap();
    let a = Answer {
        models: vec![model.into()],
        list: vec![Presented {
            model: model.into(),
            year: 2026,
            period: period.into(),
            csv: format!("CSVPRUEBA{model}{period}000"),
            file_number: String::new(),
            receipt: receipt.into(),
            previous_receipt: None,
            presented_at: "2026-04-15T10:00:00".into(),
        }],
        ..Answer::default()
    };
    presentado_aeat::record(v, "B12345674", 2026, &a, NOW).unwrap();
}

/// Página `suffix` del fichero, para leerla por posiciones.
fn page<'a>(spec: &'a taxpain_models::dr::Spec, file: &str, suffix: &str) -> PageReader<'a> {
    let p = spec.page(suffix).unwrap();
    let marker: String = taxpain_models::dr::page_marker(p);
    let at = file.find(&marker).unwrap();
    let record: String = file[at..].chars().take(p.len()).collect();
    PageReader::new(p, &record).unwrap()
}

#[test]
fn factura_anadida_a_un_303_presentado_sale_a_ingresar_con_recargo() {
    let v = setup();
    emit(&v, 1000, date!(2026 - 02 - 10));
    let presentado = models::draft(&v, "303", 2026, 1).unwrap();
    assert_eq!(presentado.result, eur(210));
    // Después de presentarlo aparece otra venta del trimestre.
    emit(&v, 500, date!(2026 - 03 - 10));
    presentar(&v, "303", "1T", &presentado.boxes, "3030000000017");

    let ant = rectificativa::anterior(&v, "303", 2026, 1)
        .unwrap()
        .unwrap();
    assert_eq!(ant.justificante.as_deref(), Some("3030000000017"));
    assert_eq!(ant.resultado, eur(210));

    let o = Opciones {
        fecha: Some(date!(2026 - 07 - 25)),
        ..Opciones::default()
    };
    let r = rectificativa::preparar(&v, "303", 2026, 1, &o, date!(2026 - 07 - 25)).unwrap();
    assert_eq!(r.cauce, "rectificativa");
    assert_eq!(r.ingresado_antes, eur(210));
    assert_eq!(r.total, eur(315));
    assert_eq!(
        (r.resultado, r.a_ingresar, r.a_devolver),
        (eur(105), eur(105), Money::ZERO)
    );
    // La venta añadida: base [07] y cuota [09] del 21 %.
    let fila = |b: u16| r.filas.iter().find(|f| f.casilla == b).unwrap();
    assert_eq!((fila(7).presentado, fila(7).ahora), (eur(1000), eur(1500)));
    assert_eq!(fila(9).diferencia, eur(105));
    assert_eq!(
        (fila(70).presentado, fila(70).ahora),
        (Money::ZERO, eur(210))
    );
    // Plazo: 20 de abril; tres meses completos hasta el 25 de julio, 4 %.
    assert_eq!(r.plazo, Some(date!(2026 - 04 - 20)));
    let rec = r.recargo.as_ref().unwrap();
    assert_eq!(
        (rec.porcentaje, rec.meses, rec.importe),
        (4, 3, Money::from_cents(420))
    );
    assert_eq!(r.total_a_pagar, Money::from_cents(10_920));
    assert_eq!(r.declaracion, "I");

    // El fichero, con las marcas de la rectificativa.
    let spec = taxpain_models::m303::spec();
    let p3 = page(spec, &r.file, "03");
    assert_eq!(p3.raw(31).unwrap(), "X");
    assert_eq!(p3.raw(32).unwrap(), "3030000000017");
    assert_eq!(p3.raw(35).unwrap(), "X");
    assert_eq!(p3.raw(36).unwrap(), " ");
    assert_eq!(p3.amount_box(69).unwrap(), Some(eur(315)));
    assert_eq!(p3.amount_box(70).unwrap(), Some(eur(210)));
    assert_eq!(p3.amount_box(71).unwrap(), Some(eur(105)));
    assert_eq!(p3.amount_box(111).unwrap(), Some(Money::ZERO));
    assert_eq!(page(spec, &r.file, "01").raw(6).unwrap(), "I");
    assert!(
        r.file_name.ends_with("_rectificativa.txt"),
        "{}",
        r.file_name
    );

    // Guardarlo deja huella en la auditoría.
    let bytes = rectificativa::exportar(&v, &r, at(date!(2026 - 07 - 25))).unwrap();
    assert_eq!(bytes.len(), r.file.len());
    let n: i64 = v
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE action = 'models.rectificativa.export'
               AND subject = '303/2026/1T'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn factura_quitada_de_un_303_presentado_sale_a_devolver() {
    let v = setup();
    emit(&v, 1000, date!(2026 - 02 - 10));
    let quitada = emit(&v, 500, date!(2026 - 03 - 10));
    let presentado = models::draft(&v, "303", 2026, 1).unwrap();
    assert_eq!(presentado.result, eur(315));
    // La segunda venta no se hizo: se anula (antes de dar el trimestre por presentado).
    cancel(&v, quitada, at(date!(2026 - 03 - 20)), true).unwrap();
    presentar(&v, "303", "1T", &presentado.boxes, "3030000000025");

    let o = Opciones {
        fecha: Some(date!(2026 - 10 - 15)),
        ..Opciones::default()
    };
    let r = rectificativa::preparar(&v, "303", 2026, 1, &o, date!(2026 - 10 - 15)).unwrap();
    assert_eq!(r.cauce, "rectificativa");
    assert_eq!((r.total, r.resultado), (eur(210), eur(-105)));
    assert_eq!(
        (r.a_ingresar, r.a_devolver, r.a_compensar),
        (Money::ZERO, eur(105), Money::ZERO)
    );
    assert!(r.recargo.is_none());
    // Intereses de lo ingresado de más desde el día siguiente a la presentación (15 de abril).
    assert!(r.intereses_devolucion.cents() > 0);
    let fila = |b: u16| r.filas.iter().find(|f| f.casilla == b).unwrap();
    assert_eq!(fila(9).diferencia, eur(-105));
    let spec = taxpain_models::m303::spec();
    let p3 = page(spec, &r.file, "03");
    assert_eq!(p3.raw(31).unwrap(), "X");
    assert_eq!(p3.raw(32).unwrap(), "3030000000025");
    assert_eq!(p3.amount_box(70).unwrap(), Some(eur(315)));
    assert_eq!(p3.amount_box(71).unwrap(), Some(eur(-105)));
    assert_eq!(p3.amount_box(111).unwrap(), Some(eur(105)));
    // Sin IBAN no hay página de la devolución: se avisa.
    assert!(
        r.avisos.iter().any(|a| a.contains("IBAN")),
        "{:?}",
        r.avisos
    );
}

#[test]
fn sin_cambios_no_hay_nada_que_rectificar() {
    let v = setup();
    emit(&v, 1000, date!(2026 - 02 - 10));
    let presentado = models::draft(&v, "303", 2026, 1).unwrap();
    presentar(&v, "303", "1T", &presentado.boxes, "3030000000033");
    let r = rectificativa::preparar(
        &v,
        "303",
        2026,
        1,
        &Opciones::default(),
        date!(2026 - 05 - 01),
    )
    .unwrap();
    assert_eq!(r.cauce, "sin_cambios");
    assert!(r.file.is_empty());
    assert!(rectificativa::exportar(&v, &r, NOW).is_err());
    // Sin presentar, no hay rectificativa.
    assert!(
        rectificativa::anterior(&v, "303", 2026, 2)
            .unwrap()
            .is_none()
    );
    assert!(rectificativa::preparar(&v, "303", 2026, 2, &Opciones::default(), NOW.date()).is_err());
}

#[test]
fn el_111_va_por_complementaria_y_si_baja_por_otra_via() {
    let v = setup();
    profesional(&v, "P-1", 400, 60, date!(2026 - 02 - 05));
    let presentado = models::draft(&v, "111", 2026, 1).unwrap();
    assert_eq!(presentado.result, eur(60));
    profesional(&v, "P-2", 1000, 150, date!(2026 - 03 - 05));
    presentar(&v, "111", "1T", &presentado.boxes, "1110000000014");
    // Sin justificante escrito usa el de la AEAT; en plazo, sin recargo.
    let o = Opciones {
        fecha: Some(date!(2026 - 04 - 18)),
        ..Opciones::default()
    };
    let r = rectificativa::preparar(&v, "111", 2026, 1, &o, date!(2026 - 04 - 18)).unwrap();
    assert_eq!(r.cauce, "complementaria");
    assert_eq!((r.ingresado_antes, r.resultado), (eur(60), eur(150)));
    assert!(r.recargo.is_none());
    let spec = taxpain_models::withholding::spec_111();
    let p1 = page(spec, &r.file, "01");
    assert_eq!(p1.raw(42).unwrap(), "X");
    assert_eq!(p1.raw(43).unwrap(), "1110000000014");
    // [28] total, [29] lo de la anterior, [30] lo de ahora (campos 39, 40 y 41).
    assert_eq!(p1.amount(39).unwrap(), Some(eur(210)));
    assert_eq!(p1.amount(40).unwrap(), Some(eur(60)));
    assert_eq!(p1.amount(41).unwrap(), Some(eur(150)));
    assert!(r.file_name.ends_with("_complementaria.txt"));

    // Si lo presentado era más, la complementaria no sirve: se pide la rectificación.
    let v = setup();
    profesional(&v, "P-1", 400, 60, date!(2026 - 02 - 05));
    presentar(
        &v,
        "111",
        "1T",
        &[(28, eur(200)), (30, eur(200))],
        "1110000000022",
    );
    let r = rectificativa::preparar(
        &v,
        "111",
        2026,
        1,
        &Opciones::default(),
        date!(2026 - 05 - 04),
    )
    .unwrap();
    assert_eq!(r.cauce, "otra_via");
    assert_eq!(r.resultado, eur(-140));
    assert!(r.file.is_empty());
    assert!(r.cauce_texto.contains("rectificación"));
}

#[test]
fn justificante_tecleado_y_mal_escrito() {
    let v = setup();
    emit(&v, 1000, date!(2026 - 02 - 10));
    let presentado = models::draft(&v, "303", 2026, 1).unwrap();
    emit(&v, 100, date!(2026 - 03 - 10));
    // Sólo tecleado en «Modelos presentados»: no se sabe su justificante.
    let b: BTreeMap<u16, Money> = presentado.boxes.iter().copied().collect();
    presentados::save_manual(&v, "303", 2026, "1T", &b, NOW).unwrap();
    let r = rectificativa::preparar(
        &v,
        "303",
        2026,
        1,
        &Opciones::default(),
        date!(2026 - 04 - 10),
    )
    .unwrap();
    assert_eq!(r.anterior.fuente, "gestor");
    assert!(r.file.is_empty());
    assert!(r.avisos.iter().any(|a| a.contains("justificante")));
    let mal = Opciones {
        justificante: Some("12345".into()),
        ..Opciones::default()
    };
    let r = rectificativa::preparar(&v, "303", 2026, 1, &mal, date!(2026 - 04 - 10)).unwrap();
    assert!(r.file.is_empty());
    let bien = Opciones {
        justificante: Some("3030000000041".into()),
        ..Opciones::default()
    };
    let r = rectificativa::preparar(&v, "303", 2026, 1, &bien, date!(2026 - 04 - 10)).unwrap();
    assert!(r.file.contains("3030000000041"));
    // En plazo (antes del 20 de abril) no hay recargo.
    assert!(r.recargo.is_none());
    assert_eq!(r.a_ingresar, eur(21));
}
