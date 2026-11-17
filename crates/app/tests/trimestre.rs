//! Cierre de trimestre guiado, guía «Dejar la gestoría» y obligaciones no fiscales de una SL.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::invoicing::{self, InvoiceDraft};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::trimestre::{self, Estado, Marca};
use taxpain_app::{accounting, obligaciones, puesta_marcha};
use taxpain_core::invoice::{InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment};
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
    invoicing::save_settings(&v, &s, datetime!(2026-01-01 9:00 UTC)).unwrap();
    v
}

fn venta(v: &Vault, d: Date) {
    invoicing::issue(
        v,
        &InvoiceDraft {
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
                description: "Servicio".into(),
                quantity_milli: 1000,
                unit_price: eur(1_000),
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
            description: "Servicio".into(),
            notes: None,
            currency: None,
            oss: None,
        },
        at(d),
        true,
    )
    .unwrap();
}

#[test]
fn trimestre_de_una_sl_con_cuadre_y_liquidacion() {
    let v = setup();
    venta(&v, date!(2026 - 02 - 10));
    let hoy = date!(2026 - 04 - 05);
    let c = trimestre::cierre(&v, 2026, 1, hoy).unwrap();
    assert!(c.sin_perfil, "sin perfil: el tipo de una SL");
    let modelos: Vec<&str> = c.modelos.iter().map(|m| m.model.as_str()).collect();
    assert_eq!(modelos, ["303", "202", "111", "115"]);
    let m303 = &c.modelos[0];
    assert_eq!(m303.result, eur(210));
    assert!(!m303.presentado);
    let devengado = m303
        .cuadres
        .iter()
        .find(|x| x.concepto.contains("[27]"))
        .unwrap();
    assert_eq!((devengado.modelo, devengado.diario), (eur(210), eur(210)));
    assert!(devengado.ok);
    let paso = |c: &trimestre::CierreTrimestre, id: &str| {
        c.pasos.iter().find(|p| p.id == id).unwrap().clone()
    };
    assert_eq!(paso(&c, "recibidas").estado, Estado::Ok);
    assert_eq!(
        paso(&c, "nominas").estado,
        Estado::NoAplica,
        "sin empleados"
    );
    assert_eq!(
        paso(&c, "banco").estado,
        Estado::Aviso,
        "sin extracto del banco"
    );
    assert_eq!(paso(&c, "liquidacion_iva").estado, Estado::Pendiente);
    assert_eq!(paso(&c, "amortizaciones").estado, Estado::NoAplica);
    assert!(c.a_ingresar >= eur(210));
    assert!(c.listo, "{:?} {:?}", c.pasos, c.modelos);
    assert!(!c.presentado);

    // El banco se lleva fuera: se marca a mano y queda en la auditoría.
    trimestre::marcar(&v, 2026, 1, "banco", Some(Marca::Hecho), at(hoy)).unwrap();
    assert!(trimestre::marcar(&v, 2026, 1, "recibidas", Some(Marca::Hecho), at(hoy)).is_err());
    let c = trimestre::cierre(&v, 2026, 1, hoy).unwrap();
    assert_eq!(paso(&c, "banco").marca, Some(Marca::Hecho));
    assert_eq!(paso(&c, "banco").estado, Estado::Ok);
    trimestre::marcar(&v, 2026, 1, "banco", None, at(hoy)).unwrap();
    let c = trimestre::cierre(&v, 2026, 1, hoy).unwrap();
    assert_eq!(paso(&c, "banco").marca, None);

    // Contabilizada la liquidación, el paso queda hecho.
    accounting::settle_vat_quarter(&v, 2026, 1, at(hoy)).unwrap();
    let c = trimestre::cierre(&v, 2026, 1, hoy).unwrap();
    assert_eq!(paso(&c, "liquidacion_iva").estado, Estado::Ok);

    let r = trimestre::resumen(&v, 2026, hoy).unwrap();
    assert!(r[0].listo && !r[0].futuro);
    assert!(r[1].en_curso);
    assert!(r[2].futuro && r[3].futuro);
}

#[test]
fn guia_y_obligaciones_guardan_lo_marcado() {
    let v = setup();
    let hoy = date!(2026 - 11 - 16);
    let g = puesta_marcha::guia(&v, hoy).unwrap();
    assert!(!g.empezada);
    assert_eq!(g.resueltos, 0);
    assert!(g.correo.cuerpo.contains("Estudio Demo SL (NIF B12345674)"));
    assert!(g.correo.cuerpo.contains("apoderamientos"));
    let revocar = g.pasos.iter().find(|p| p.id == "revocar").unwrap();
    assert!(
        revocar.enlaces[0]
            .url
            .starts_with("https://sede.agenciatributaria.gob.es/")
    );
    puesta_marcha::marcar(
        &v,
        "saldos",
        puesta_marcha::EstadoPaso::Hecho,
        Some(date!(2026 - 10 - 01)),
        "Corte al 1 de octubre",
        at(hoy),
    )
    .unwrap();
    assert!(
        puesta_marcha::marcar(
            &v,
            "otro",
            puesta_marcha::EstadoPaso::Hecho,
            None,
            "",
            at(hoy)
        )
        .is_err()
    );
    let g = puesta_marcha::guia(&v, hoy).unwrap();
    assert!(g.empezada);
    assert_eq!(g.resueltos, 1);
    let s = g.pasos.iter().find(|p| p.id == "saldos").unwrap();
    assert_eq!(s.fecha, Some(date!(2026 - 10 - 01)));
    assert_eq!(s.nota, "Corte al 1 de octubre");

    let o = obligaciones::lista(&v).unwrap();
    assert!(!o.hay_empleados);
    assert!(o.lista.len() >= 10);
    assert!(
        o.lista
            .iter()
            .all(|x| x.estado == obligaciones::EstadoObligacion::SinRevisar)
    );
    obligaciones::marcar(
        &v,
        "lssi_web",
        obligaciones::EstadoObligacion::Hecho,
        None,
        "Aviso legal publicado",
        at(hoy),
    )
    .unwrap();
    let o = obligaciones::lista(&v).unwrap();
    let web = o.lista.iter().find(|x| x.id == "lssi_web").unwrap();
    assert_eq!(web.estado, obligaciones::EstadoObligacion::Hecho);
    let acciones: i64 = v
        .connection()
        .query_row(
            "SELECT count(*) FROM audit_log WHERE action IN ('onboarding.step', 'obligations.mark')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(acciones, 2);
}
