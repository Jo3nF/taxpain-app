//! Facturas periódicas: plantilla desde una factura, marcadores del periodo, emisión con
//! confirmación (con fecha de hoy si se pasó) y fin de la serie. Y «Duplicar».

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::invoicing::{self, InvoiceDraft};
use taxpain_app::recurring::{self, LateVat, TemplatePatch};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::invoice::{
    IdType, InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment,
};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

fn settings() -> InvoicingSettings {
    InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Agencia de Prueba SL".into(),
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
            pdf: true,
            facturae: false,
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
    }
}

fn vault() -> Vault {
    Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap()
}

fn cuota(date: time::Date, client: Party) -> InvoiceDraft {
    InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: date,
        operation_date: None,
        recipient: Some(client),
        lines: vec![InvoiceLine {
            description: "Mantenimiento web".into(),
            quantity_milli: 1000,
            unit_price: Money::from_euros(100, 0),
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
        description: "Mantenimiento web".into(),
        notes: None,
        currency: None,
        oss: None,
    }
}

fn carmen() -> Party {
    Party {
        id: PartyId::Nif("12345678Z".into()),
        name: "Carmen Española".into(),
        address: addr(),
        person: None,
    }
}

/// Cambio de nombre sin tocar las fechas (lo que manda la interfaz).
fn rename(name: &str) -> TemplatePatch {
    TemplatePatch {
        name: name.into(),
        every_months: 1,
        next_date: None,
        end: Some(date!(2026 - 06 - 30)),
        active: true,
        line_descriptions: None,
        line_prices: None,
    }
}

#[test]
fn cuota_mensual() {
    let v = vault();
    let now = datetime!(2026-09-01 10:00 UTC);
    invoicing::save_settings(&v, &settings(), now).unwrap();
    let first = invoicing::issue(&v, &cuota(date!(2026 - 09 - 01), carmen()), now, true).unwrap();

    // «Duplicar»: el mismo cliente y líneas.
    let dup = recurring::draft_of(&v, first.id).unwrap();
    assert_eq!(dup.lines.len(), 1);
    assert!(dup.recipient.is_some());

    let t = recurring::create_from_invoice(&v, first.id, 1, now).unwrap();
    assert_eq!(t.next_date(), Some(date!(2026 - 10 - 01)));
    assert_eq!(recurring::due_count(&v, date!(2026 - 09 - 15)).unwrap(), 0);
    // El concepto lleva el mes y sube el precio.
    recurring::update(
        &v,
        &t.id,
        &TemplatePatch {
            name: "Mantenimiento Carmen".into(),
            every_months: 1,
            next_date: Some(date!(2026 - 10 - 01)),
            end: Some(date!(2026 - 11 - 30)),
            active: true,
            line_descriptions: Some(vec!["Mantenimiento web {mes}".into()]),
            line_prices: Some(vec![12_000]),
        },
        now,
    )
    .unwrap();
    assert_eq!(recurring::due_count(&v, date!(2026 - 10 - 01)).unwrap(), 1);

    // Sin confirmar no se emite; se emite el 3 de octubre: fecha de hoy, operación del 1.
    assert!(recurring::issue(&v, &t.id, date!(2026 - 10 - 03), now, false).is_err());
    let (d, _) = recurring::preview_draft(&v, &t.id, date!(2026 - 10 - 03)).unwrap();
    assert_eq!(d.issue_date, date!(2026 - 10 - 03));
    assert_eq!(d.operation_date, Some(date!(2026 - 10 - 01)));
    assert_eq!(d.lines[0].description, "Mantenimiento web octubre 2026");
    let out = recurring::issue(
        &v,
        &t.id,
        date!(2026 - 10 - 03),
        at(date!(2026 - 10 - 03)),
        true,
    )
    .unwrap();
    assert_eq!(out.totals.base, Money::from_euros(120, 0));
    let rows = recurring::list(&v, date!(2026 - 10 - 03)).unwrap();
    assert_eq!(rows[0].next_date, Some(date!(2026 - 11 - 01)));
    assert!(!rows[0].due);
    // Antes de tiempo, no.
    assert!(
        recurring::issue(
            &v,
            &t.id,
            date!(2026 - 10 - 20),
            at(date!(2026 - 10 - 20)),
            true
        )
        .is_err()
    );
    // La de noviembre y fin: la de diciembre ya no existe.
    recurring::issue(
        &v,
        &t.id,
        date!(2026 - 11 - 01),
        at(date!(2026 - 11 - 01)),
        true,
    )
    .unwrap();
    let rows = recurring::list(&v, date!(2026 - 12 - 15)).unwrap();
    assert_eq!(rows[0].next_date, None);
    assert_eq!(recurring::due_count(&v, date!(2026 - 12 - 15)).unwrap(), 0);
    recurring::delete(&v, &t.id, now).unwrap();
    assert!(
        recurring::list(&v, date!(2026 - 12 - 15))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn del_31_sigue_el_31_y_editar_no_vuelve_a_facturar() {
    let v = vault();
    let now = datetime!(2026-01-31 10:00 UTC);
    invoicing::save_settings(&v, &settings(), now).unwrap();
    let first = invoicing::issue(&v, &cuota(date!(2026 - 01 - 31), carmen()), now, true).unwrap();
    let t = recurring::create_from_invoice(&v, first.id, 1, now).unwrap();
    recurring::update(&v, &t.id, &rename("Cuota Carmen"), now).unwrap();
    // De febrero a junio, cada una el día que toca: el 31 donde lo hay (no el 28 para siempre).
    let mut dates = vec![];
    while let Some(d) = recurring::list(&v, date!(2026 - 07 - 15)).unwrap()[0].next_date {
        recurring::issue(&v, &t.id, d, at(d), true).unwrap();
        dates.push(d);
    }
    assert_eq!(
        dates,
        [
            date!(2026 - 02 - 28),
            date!(2026 - 03 - 31),
            date!(2026 - 04 - 30),
            date!(2026 - 05 - 31),
            date!(2026 - 06 - 30),
        ]
    );
    // Terminada: cambiarle el nombre no vuelve a poner las cinco por emitir.
    recurring::update(&v, &t.id, &rename("Cuota Carmen 2026"), now).unwrap();
    let rows = recurring::list(&v, date!(2026 - 07 - 15)).unwrap();
    assert_eq!(rows[0].next_date, None);
    assert_eq!(rows[0].template.name, "Cuota Carmen 2026");
    assert_eq!(recurring::due_count(&v, date!(2026 - 07 - 15)).unwrap(), 0);
    assert!(
        recurring::issue(
            &v,
            &t.id,
            date!(2026 - 07 - 15),
            at(date!(2026 - 07 - 15)),
            true
        )
        .is_err()
    );
    // Alargarla: sigue por julio (el 31).
    let mut p = rename("Cuota Carmen 2026");
    p.end = None;
    recurring::update(&v, &t.id, &p, now).unwrap();
    let rows = recurring::list(&v, date!(2026 - 07 - 15)).unwrap();
    assert_eq!(rows[0].next_date, Some(date!(2026 - 07 - 31)));
    // Trimestral sin tocar la fecha: la siguiente sigue siendo la de julio y luego, octubre.
    p.every_months = 3;
    recurring::update(&v, &t.id, &p, now).unwrap();
    assert_eq!(
        recurring::list(&v, date!(2026 - 07 - 15)).unwrap()[0].next_date,
        Some(date!(2026 - 07 - 31))
    );
    recurring::issue(
        &v,
        &t.id,
        date!(2026 - 07 - 31),
        at(date!(2026 - 07 - 31)),
        true,
    )
    .unwrap();
    let rows = recurring::list(&v, date!(2026 - 08 - 01)).unwrap();
    assert_eq!(rows[0].next_date, Some(date!(2026 - 10 - 31)));
    // Cambiar la fecha a propósito la toma como nueva (con su día).
    p.next_date = Some(date!(2026 - 11 - 15));
    recurring::update(&v, &t.id, &p, now).unwrap();
    let rows = recurring::list(&v, date!(2026 - 08 - 01)).unwrap();
    assert_eq!(rows[0].next_date, Some(date!(2026 - 11 - 15)));
}

#[test]
fn plantillas_antiguas_recuperan_el_dia() {
    let v = vault();
    let now = datetime!(2026-01-31 10:00 UTC);
    invoicing::save_settings(&v, &settings(), now).unwrap();
    let first = invoicing::issue(&v, &cuota(date!(2026 - 01 - 31), carmen()), now, true).unwrap();
    recurring::create_from_invoice(&v, first.id, 1, now).unwrap();
    // Como se guardaban antes: sin el día, desde el 28/2 y sin ninguna emitida.
    let mut raw: serde_json::Value = v.setting("invoice_templates").unwrap().unwrap();
    raw[0].as_object_mut().unwrap().remove("day");
    v.set_setting("invoice_templates", &raw).unwrap();
    let rows = recurring::list(&v, date!(2026 - 02 - 01)).unwrap();
    assert_eq!(rows[0].template.day, Some(31));
    assert_eq!(rows[0].next_date, Some(date!(2026 - 02 - 28)));
    let id = rows[0].template.id.clone();
    recurring::issue(
        &v,
        &id,
        date!(2026 - 02 - 28),
        at(date!(2026 - 02 - 28)),
        true,
    )
    .unwrap();
    let rows = recurring::list(&v, date!(2026 - 03 - 01)).unwrap();
    assert_eq!(rows[0].next_date, Some(date!(2026 - 03 - 31)));
}

#[test]
fn atrasada_de_un_trimestre_ya_cerrado_pregunta() {
    let v = vault();
    let now = datetime!(2026-06-01 10:00 UTC);
    invoicing::save_settings(&v, &settings(), now).unwrap();
    let client = Party {
        id: PartyId::Foreign {
            country: "FR".into(),
            id_type: IdType::NifIva,
            id: "FR12345678901".into(),
        },
        name: "Acme France SAS".into(),
        address: addr(),
        person: None,
    };
    let mut d = cuota(date!(2026 - 06 - 01), client);
    d.lines[0].description = "Maintenance {month}".into();
    // Servicio a una empresa de la UE: no sujeto aquí (art. 69 LIVA).
    d.lines[0].vat = VatTreatment::NotSubject(taxpain_core::invoice::NotSubject::N2);
    let first = invoicing::issue_in(&v, &d, Some(Language::En), now, true).unwrap();
    let t = recurring::create_from_invoice(&v, first.id, 1, now).unwrap();
    // La factura de origen era en inglés: la plantilla, también.
    assert_eq!(t.language, Some(Language::En));
    assert_eq!(
        invoicing::language_of(&v, first.id).unwrap(),
        Some(Language::En)
    );

    // La de julio se emite el 25 de octubre: el 303 del 3T venció el 20.
    let today = date!(2026 - 10 - 25);
    let late = datetime!(2026-10-25 10:00 UTC);
    let (p, lang) = recurring::preview_draft(&v, &t.id, today).unwrap();
    assert_eq!(lang, Some(Language::En));
    assert_eq!(p.lines[0].description, "Maintenance July 2026");
    let why = recurring::late_vat(&v, &p, today).unwrap().unwrap();
    assert!(why.contains("3T de 2026"), "{why}");
    // Sin decirlo, no se emite (ni avanza la plantilla).
    let err = recurring::issue(&v, &t.id, today, late, true)
        .unwrap_err()
        .to_string();
    assert!(err.contains("periodo atrasado"), "{err}");
    assert_eq!(recurring::list(&v, today).unwrap()[0].template.issued, 0);
    // «Sin fecha de la operación»: el IVA va al periodo actual.
    let jul =
        recurring::issue_with(&v, &t.id, today, late, true, None, Some(LateVat::Today)).unwrap();
    // «Con la del periodo»: la de agosto sigue en el 3T (habrá que corregir aquel 303).
    let aug =
        recurring::issue_with(&v, &t.id, today, late, true, None, Some(LateVat::Period)).unwrap();
    let op = |id: i64| -> Option<String> {
        v.connection()
            .query_row(
                "SELECT json_extract(data, '$.invoice.operation_date') FROM invoice_issued
                 WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .unwrap()
    };
    assert_eq!(op(jul.id), None);
    assert_eq!(op(aug.id).as_deref(), Some("2026-08-01"));
    // Septiembre, igual; octubre es del trimestre en curso: sin preguntar.
    recurring::issue(&v, &t.id, today, late, true).unwrap_err();
    recurring::issue_with(&v, &t.id, today, late, true, None, Some(LateVat::Period)).unwrap();
    let (oct, _) = recurring::preview_draft(&v, &t.id, today).unwrap();
    assert_eq!(oct.operation_date, Some(date!(2026 - 10 - 01)));
    assert_eq!(recurring::late_vat(&v, &oct, today).unwrap(), None);
    recurring::issue(&v, &t.id, today, late, true).unwrap();
    let rows = recurring::list(&v, today).unwrap();
    assert_eq!(rows[0].template.issued, 4);
    assert_eq!(rows[0].next_date, Some(date!(2026 - 11 - 01)));
}

/// Reloj del día en que se emite: una factura no puede llevar fecha posterior a «hoy».
fn at(d: time::Date) -> time::OffsetDateTime {
    d.with_time(time::Time::from_hms(10, 0, 0).unwrap())
        .assume_utc()
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
