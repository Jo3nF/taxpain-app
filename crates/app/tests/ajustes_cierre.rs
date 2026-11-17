//! Ajustes de cierre: periodificaciones (480/485) con su reversión el 1 de enero, facturas
//! pendientes de recibir y de emitir (4009/4309), deterioro de clientes (694/490, art. 13.1 LIS)
//! con su reversión y su corrección en el 200, provisiones (14x) y saldos anómalos.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::ajustes_cierre::{
    self, Exclusion, Motivo, Nuevo, Tipo, comprobaciones, crear, deshacer, deshacer_reversion,
    propuesta, resumen, revertir,
};
use taxpain_app::invoicing::{self, InvoiceDraft};
use taxpain_app::modelo200::{self, AjusteGuardado};
use taxpain_app::received::{AssetClass, Nature, Reviewed, Source, ingest_file, review};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::{accounting, cierre, models};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
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

const NOW: OffsetDateTime = datetime!(2027-01-20 10:00 UTC);

fn addr() -> Option<taxpain_core::invoice::Address> {
    Some(taxpain_core::invoice::Address {
        street: "Calle Inventada 1".into(),
        post_code: "46001".into(),
        town: "València".into(),
        province: "Valencia".into(),
        country: "ES".into(),
    })
}

/// Estudio Demo SL (datos inventados).
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

fn entry(v: &Vault, d: Date, lines: &[(&str, i64, i64)]) {
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
    accounting::manual_entry(
        v,
        &JournalEntry {
            date: d,
            concept: "Asiento de prueba".into(),
            origin: Origin::Manual,
            lines,
        },
        at(d),
    )
    .unwrap();
}

fn issue(v: &Vault, d: Date, euros: i64) -> i64 {
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
                description: "Mantenimiento anual".into(),
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
            description: "Mantenimiento".into(),
            notes: None,
            currency: None,
            oss: None,
        },
        at(d),
        true,
    )
    .unwrap()
    .id
}

fn receive(v: &Vault, d: Date, base: i64, number: &str) -> i64 {
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
            supplier_name: "Seguros Inventados".into(),
            number: number.into(),
            issue_date: d,
            base: eur(base),
            vat: Money::ZERO,
            withholding: Money::ZERO,
            total: eur(base),
            deductible_pct: 100,
            capital_good: false,
            nature: Nature::General,
            asset_class: None::<AssetClass>,
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
    rec.id
}

fn saldo(v: &Vault, year: i32, account: &str) -> Money {
    let cents: i64 = v
        .connection()
        .query_row(
            "SELECT COALESCE(SUM(l.debit - l.credit), 0) FROM journal_line l
               JOIN journal_entry e ON e.id = l.entry_id
              WHERE e.fiscal_year = ?1 AND l.account = ?2
                AND e.origin_kind NOT IN ('Cierre', 'Regularizacion')",
            taxpain_vault::rusqlite::params![year, account],
            |r| r.get(0),
        )
        .unwrap();
    Money::from_cents(cents)
}

#[test]
fn periodificacion_de_un_seguro_con_su_reversion() {
    let v = setup();
    // Seguro anual del 1/10/2026 al 30/9/2027: 1.200 € a la 629.
    let id = receive(&v, date!(2026 - 10 - 01), 1_200, "POL-1");
    let r = resumen(&v, 2026, false).unwrap();
    let p = r
        .periodificables
        .iter()
        .find(|p| p.kind == "recibida" && p.factura_id == id)
        .unwrap();
    assert_eq!(p.importe, eur(1_200));
    assert_eq!(p.ajuste_id, None);
    let n = Nuevo {
        tipo: Some(Tipo::GastoAnticipado),
        factura_id: Some(id),
        desde: Some(date!(2026 - 10 - 01)),
        hasta: Some(date!(2027 - 09 - 30)),
        ..Default::default()
    };
    // Sin el periodo no se puede.
    assert!(
        propuesta(
            &v,
            2026,
            &Nuevo {
                hasta: None,
                ..n.clone()
            }
        )
        .is_err()
    );
    let pr = propuesta(&v, 2026, &n).unwrap();
    // 273 de 365 días son de 2027.
    assert_eq!(pr.importe, Money::from_cents(89_753));
    assert_eq!(pr.fecha, date!(2026 - 12 - 31));
    assert_eq!(pr.reversion_fecha, Some(date!(2027 - 01 - 01)));
    assert_eq!(pr.lines[0].account, "480");
    assert_eq!(pr.reversion[0].credit, Money::from_cents(89_753));
    assert!(crear(&v, 2026, &n, NOW, false).is_err());
    let aj = crear(&v, 2026, &n, NOW, true).unwrap();
    assert!(crear(&v, 2026, &n, NOW, true).is_err(), "ya periodificada");
    assert_eq!(saldo(&v, 2026, "480"), Money::from_cents(89_753));
    assert_eq!(saldo(&v, 2026, "629"), Money::from_cents(120_000 - 89_753));
    // La reversión, el 1 de enero: la 480 vuelve al gasto de 2027.
    assert_eq!(saldo(&v, 2027, "480"), -Money::from_cents(89_753));
    assert_eq!(saldo(&v, 2027, "629"), Money::from_cents(89_753));
    let r = resumen(&v, 2026, false).unwrap();
    assert_eq!(r.ajustes.len(), 1);
    assert_eq!(r.ajustes[0].reversion_fecha, Some(date!(2027 - 01 - 01)));
    assert!(r.periodificables.iter().any(|p| p.ajuste_id == Some(aj)));
    // El diario lo distingue y no lo deja borrar como manual.
    let j = accounting::journal(&v, 2026).unwrap();
    let e = j
        .iter()
        .find(|e| e.entry.origin == Origin::AjusteCierre(aj))
        .unwrap();
    assert!(accounting::delete_manual_entry(&v, e.id, NOW, true).is_err());
    // Deshacer quita el ajuste y la reversión.
    deshacer(&v, aj, NOW, true).unwrap();
    assert_eq!(saldo(&v, 2026, "480"), Money::ZERO);
    assert_eq!(saldo(&v, 2027, "480"), Money::ZERO);
    assert!(resumen(&v, 2026, false).unwrap().ajustes.is_empty());
}

#[test]
fn ingreso_anticipado_de_una_emitida() {
    let v = setup();
    // Mantenimiento anual facturado el 1/12/2026 por 3.650 € (1/12/2026–30/11/2027).
    let id = issue(&v, date!(2026 - 12 - 01), 3_650);
    let n = Nuevo {
        tipo: Some(Tipo::IngresoAnticipado),
        factura_id: Some(id),
        desde: Some(date!(2026 - 12 - 01)),
        hasta: Some(date!(2027 - 11 - 30)),
        ..Default::default()
    };
    let aj = crear(&v, 2026, &n, NOW, true).unwrap();
    // 334 de 365 días son de 2027: 3.340 €.
    assert_eq!(saldo(&v, 2026, "485"), -eur(3_340));
    assert_eq!(saldo(&v, 2026, "705"), -eur(310));
    assert_eq!(saldo(&v, 2027, "485"), eur(3_340));
    assert_eq!(saldo(&v, 2027, "705"), -eur(3_340));
    assert_eq!(resumen(&v, 2026, false).unwrap().ajustes[0].id, aj);
}

#[test]
fn facturas_pendientes_de_recibir_y_de_emitir() {
    let v = setup();
    let recibir = Nuevo {
        tipo: Some(Tipo::PendienteRecibir),
        descripcion: "Asesoría de diciembre (llega en enero)".into(),
        importe: Some(eur(300)),
        cuenta: Some("623".into()),
        ..Default::default()
    };
    assert!(
        crear(
            &v,
            2026,
            &Nuevo {
                cuenta: Some("705".into()),
                ..recibir.clone()
            },
            NOW,
            true
        )
        .is_err(),
        "la cuenta de gasto es del grupo 6"
    );
    assert!(
        crear(
            &v,
            2026,
            &Nuevo {
                importe: None,
                ..recibir.clone()
            },
            NOW,
            true
        )
        .is_err()
    );
    let a = crear(&v, 2026, &recibir, NOW, true).unwrap();
    let emitir = Nuevo {
        tipo: Some(Tipo::PendienteEmitir),
        descripcion: "Trabajo de diciembre sin facturar".into(),
        importe: Some(eur(1_000)),
        cuenta: Some("705".into()),
        ..Default::default()
    };
    crear(&v, 2026, &emitir, NOW, true).unwrap();
    assert_eq!(saldo(&v, 2026, "623"), eur(300));
    assert_eq!(saldo(&v, 2026, "4009"), -eur(300));
    assert_eq!(saldo(&v, 2026, "4309"), eur(1_000));
    assert_eq!(saldo(&v, 2026, "705"), -eur(1_000));
    // Reversión el 1/1/2027: la factura de enero se contabiliza como siempre y el gasto queda
    // en 2026.
    assert_eq!(saldo(&v, 2027, "4009"), eur(300));
    assert_eq!(saldo(&v, 2027, "623"), -eur(300));
    assert_eq!(saldo(&v, 2027, "4309"), -eur(1_000));
    // La recibida de enero se propone como pendiente de recibir de 2026.
    let enero = receive(&v, date!(2027 - 01 - 15), 500, "ENE-1");
    let r = resumen(&v, 2026, false).unwrap();
    let s = r
        .sugerencias
        .iter()
        .find(|s| s.factura_id == enero)
        .unwrap();
    assert_eq!((s.kind.as_str(), s.base), ("recibida", eur(500)));
    assert_eq!(s.cuenta.as_deref(), Some("629"));
    // Cerrado 2026, ya no se puede deshacer.
    accounting::close_year(&v, 2026, NOW, true).unwrap();
    assert!(deshacer(&v, a, NOW, true).is_err());
    assert!(crear(&v, 2026, &recibir, NOW, true).is_err());
}

#[test]
fn deterioro_de_clientes_con_el_art_13_1_y_su_reversion() {
    let v = setup();
    // Vence el 9/4/2026 (30 días): más de seis meses al cierre.
    let vieja = issue(&v, date!(2026 - 03 - 10), 1_000);
    // Vence el 1/10/2026: al cierre, menos de seis meses.
    let nueva = issue(&v, date!(2026 - 09 - 01), 500);
    let r = resumen(&v, 2026, false).unwrap();
    assert_eq!(r.deteriorables.len(), 1);
    let d = &r.deteriorables[0];
    assert_eq!(d.factura_id, vieja);
    assert_eq!(d.importe, eur(1_210));
    assert!(d.dias > 180 && !d.vinculada);
    let c = cierre::checklist(&v, 2026).unwrap();
    assert!(!c.items.iter().find(|i| i.id == "ajustes").unwrap().ok);

    let n = Nuevo {
        tipo: Some(Tipo::Deterioro),
        factura_id: Some(vieja),
        vencimiento: Some(1),
        ..Default::default()
    };
    let p = propuesta(&v, 2026, &n).unwrap();
    assert!(p.deducible, "{}", p.motivo_fiscal);
    let a = crear(&v, 2026, &n, NOW, true).unwrap();
    assert_eq!(saldo(&v, 2026, "694"), eur(1_210));
    assert_eq!(saldo(&v, 2026, "490"), -eur(1_210));
    assert!(resumen(&v, 2026, false).unwrap().deteriorables.is_empty());
    assert!(
        crear(&v, 2026, &n, NOW, true).is_err(),
        "ya tiene deterioro"
    );
    // La reciente: deterioro contable, no deducible (seis meses sin cumplir).
    let b = crear(
        &v,
        2026,
        &Nuevo {
            tipo: Some(Tipo::Deterioro),
            factura_id: Some(nueva),
            vencimiento: Some(1),
            importe: Some(eur(500)),
            ..Default::default()
        },
        NOW,
        true,
    )
    .unwrap();
    // Con garantía no es deducible ni con seis meses; con pleito, sí.
    let garantia = Nuevo {
        exclusion: Some(Exclusion::Garantia),
        ..n.clone()
    };
    deshacer(&v, a, NOW, true).unwrap();
    assert!(!propuesta(&v, 2026, &garantia).unwrap().deducible);
    let judicial = Nuevo {
        motivo: Some(Motivo::Judicial),
        ..garantia
    };
    assert!(propuesta(&v, 2026, &judicial).unwrap().deducible);
    let a = crear(&v, 2026, &n, NOW, true).unwrap();
    let f = ajustes_cierre::fiscal(v.connection(), 2026).unwrap();
    assert_eq!((f.aumentos, f.disminuciones), (eur(500), Money::ZERO));
    // El 200 lo propone en su casilla, deterioro de créditos no deducible [00321], y lo suma en
    // las correcciones en aumento [00417]: la base previa sube 500.
    let caja = |d: &models::Draft, k: u16| {
        d.boxes
            .iter()
            .find(|(c, _)| *c == k)
            .map_or(Money::ZERO, |x| x.1)
    };
    let d = models::draft(&v, "200", 2026, 0).unwrap();
    assert_eq!(caja(&d, 321), eur(500));
    assert_eq!(caja(&d, 417), eur(500));
    assert_eq!(caja(&d, 550), caja(&d, 501) + eur(500));
    let h = modelo200::hoja(&v, 2026).unwrap();
    let aj = h.ajustes.iter().find(|a| a.casilla == 321).unwrap();
    assert_eq!(
        (aj.origen.as_str(), aj.estado.as_str(), aj.aumento),
        ("Ajustes de cierre", "propuesta", true)
    );
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("ajustes de cierre") && w.contains("500,00")),
        "{:?}",
        d.warnings
    );

    // Se cierra 2026 y en 2027 se cobra la reciente: se revierte su deterioro.
    accounting::close_year(&v, 2026, NOW, true).unwrap();
    assert!(deshacer(&v, a, NOW, true).is_err(), "2026 cerrado");
    assert!(revertir(&v, b, 2026, NOW, true).is_err());
    let r27 = resumen(&v, 2027, false).unwrap();
    assert_eq!(r27.anteriores.len(), 2);
    assert_eq!(r27.deterioro_anterior, eur(1_710));
    revertir(&v, b, 2027, NOW, true).unwrap();
    assert!(revertir(&v, b, 2027, NOW, true).is_err(), "ya revertido");
    assert_eq!(saldo(&v, 2027, "794"), -eur(500));
    assert_eq!(saldo(&v, 2027, "490"), -eur(1_210));
    // Lo no deducible de 2026 revertido en 2027 es una disminución en el 200 de 2027.
    let f = ajustes_cierre::fiscal(v.connection(), 2027).unwrap();
    assert_eq!((f.aumentos, f.disminuciones), (Money::ZERO, eur(500)));
    // En el 200 de 2027, en disminución [00322] y [00418]: la base previa baja 500.
    let d = models::draft(&v, "200", 2027, 0).unwrap();
    assert_eq!(caja(&d, 322), eur(500));
    assert_eq!(caja(&d, 418), eur(500));
    assert_eq!(caja(&d, 550), caja(&d, 501) - eur(500));
    let h = modelo200::hoja(&v, 2027).unwrap();
    let aj = h.ajustes.iter().find(|a| a.casilla == 322).unwrap();
    assert!(!aj.aumento && aj.origen == "Ajustes de cierre");
    deshacer_reversion(&v, b, NOW, true).unwrap();
    assert_eq!(saldo(&v, 2027, "794"), Money::ZERO);
}

#[test]
fn provision_por_un_litigio() {
    let v = setup();
    let n = Nuevo {
        tipo: Some(Tipo::Provision),
        descripcion: "Demanda de un antiguo proveedor".into(),
        importe: Some(eur(4_000)),
        cuenta: Some("678".into()),
        contrapartida: Some("142".into()),
        ..Default::default()
    };
    assert!(
        crear(
            &v,
            2026,
            &Nuevo {
                contrapartida: Some("490".into()),
                ..n.clone()
            },
            NOW,
            true
        )
        .is_err()
    );
    let p = propuesta(&v, 2026, &n).unwrap();
    assert!(!p.deducible && p.reversion.is_empty());
    let id = crear(&v, 2026, &n, NOW, true).unwrap();
    assert_eq!(saldo(&v, 2026, "142"), -eur(4_000));
    assert_eq!(saldo(&v, 2026, "678"), eur(4_000));
    assert_eq!(
        ajustes_cierre::fiscal(v.connection(), 2026)
            .unwrap()
            .aumentos,
        eur(4_000)
    );
    // Deducible si el usuario lo dice: ya no hay corrección.
    deshacer(&v, id, NOW, true).unwrap();
    let id = crear(
        &v,
        2026,
        &Nuevo {
            deducible: Some(true),
            ..n
        },
        NOW,
        true,
    )
    .unwrap();
    assert!(
        ajustes_cierre::fiscal(v.connection(), 2026)
            .unwrap()
            .aumentos
            .is_zero()
    );
    // Exceso en 2027: 142 a 7952.
    revertir(&v, id, 2027, NOW, true).unwrap();
    assert_eq!(saldo(&v, 2027, "7952"), -eur(4_000));
    assert!(deshacer(&v, id, NOW, true).is_err(), "antes la reversión");
}

/// La provisión de los ajustes de cierre (no deducible) entra en [00337] y su asiento no vuelve a
/// salir en las propuestas genéricas por cuentas (la 695 y el resto de la 678); una dotación a
/// mano a la 695, fuera de los ajustes de cierre, sí.
#[test]
fn provisiones_de_los_ajustes_de_cierre_sin_doble_computo() {
    let v = setup();
    let caja = |d: &models::Draft, k: u16| {
        d.boxes
            .iter()
            .find(|(c, _)| *c == k)
            .map_or(Money::ZERO, |x| x.1)
    };
    // Provisión por devoluciones (695 a 4994) y litigio (678 a 142), las dos no deducibles.
    for (desc, cuenta, contra, imp) in [
        ("Devoluciones de ventas previstas", "695", "4994", 300),
        ("Demanda de un antiguo proveedor", "678", "142", 4_000),
    ] {
        crear(
            &v,
            2026,
            &Nuevo {
                tipo: Some(Tipo::Provision),
                descripcion: desc.into(),
                importe: Some(eur(imp)),
                cuenta: Some(cuenta.into()),
                contrapartida: Some(contra.into()),
                ..Default::default()
            },
            NOW,
            true,
        )
        .unwrap();
    }
    let h = modelo200::hoja(&v, 2026).unwrap();
    let de_cierre: Vec<_> = h
        .ajustes
        .iter()
        .filter(|a| a.origen == "Ajustes de cierre")
        .collect();
    assert_eq!(de_cierre.len(), 2);
    assert!(
        de_cierre
            .iter()
            .all(|a| a.casilla == 337 && a.importe > Money::ZERO)
    );
    assert!(
        !h.ajustes
            .iter()
            .any(|a| a.clave == "provisiones" || a.clave == "gastos_excepcionales"),
        "{:?}",
        h.ajustes
    );
    let d = models::draft(&v, "200", 2026, 0).unwrap();
    assert_eq!(caja(&d, 337), eur(4_300));
    assert_eq!(caja(&d, 417), eur(4_300));
    assert!(
        d.warnings
            .iter()
            .any(|w| w.contains("ajustes de cierre") && w.contains("4.000,00")),
        "{:?}",
        d.warnings
    );

    // Una dotación a la 695 hecha a mano (no por los ajustes de cierre) sigue saliendo en la
    // genérica, sin incluir, sólo por su importe.
    entry(
        &v,
        date!(2026 - 12 - 31),
        &[("6950000", 200, 0), ("4994000", 0, 200)],
    );
    let h = modelo200::hoja(&v, 2026).unwrap();
    let g = h.ajustes.iter().find(|a| a.clave == "provisiones").unwrap();
    assert_eq!(
        (g.propuesto, g.estado.as_str()),
        (Some(eur(200)), "sin_incluir")
    );
    assert_eq!(
        caja(&models::draft(&v, "200", 2026, 0).unwrap(), 337),
        eur(4_300)
    );

    // Editable como las demás: se cambia el importe de la del litigio.
    let litigio = h
        .ajustes
        .iter()
        .find(|a| a.origen == "Ajustes de cierre" && a.propuesto == Some(eur(4_000)))
        .unwrap();
    modelo200::guardar_ajuste(
        &v,
        2026,
        &AjusteGuardado {
            clave: litigio.clave.clone(),
            casilla: 337,
            importe: Some(eur(1_000)),
            concepto: String::new(),
        },
        NOW,
    )
    .unwrap();
    let h = modelo200::hoja(&v, 2026).unwrap();
    let e = h.ajustes.iter().find(|a| a.clave == litigio.clave).unwrap();
    assert_eq!((e.importe, e.estado.as_str()), (eur(1_000), "editada"));
    assert_eq!(
        caja(&models::draft(&v, "200", 2026, 0).unwrap(), 337),
        eur(1_300)
    );
}

#[test]
fn comprobaciones_de_saldos_anomalos() {
    let v = setup();
    let ok = |v: &Vault, id: &str| {
        comprobaciones(v, 2026)
            .unwrap()
            .into_iter()
            .find(|c| c.id == id)
            .unwrap()
    };
    assert!(comprobaciones(&v, 2026).unwrap().iter().all(|c| c.ok));
    // Pago a un proveedor sin factura, más de lo que hay en el banco.
    entry(
        &v,
        date!(2026 - 05 - 01),
        &[("4000001", 200, 0), ("5720001", 0, 200)],
    );
    entry(&v, date!(2026 - 05 - 02), &[("555", 50, 0), ("551", 0, 50)]);
    let p = ok(&v, "proveedores");
    assert!(!p.ok);
    assert_eq!(p.cuentas[0].account, "4000001");
    assert_eq!(p.cuenta.as_deref(), Some("4000001"));
    assert_eq!(p.ir, "mayor");
    let b = ok(&v, "bancos");
    assert!(!b.ok && b.ir == "banco");
    assert_eq!(b.cuentas[0].saldo, -eur(200));
    assert!(!ok(&v, "partidas_pendientes").ok);
    assert!(!ok(&v, "socios").ok);
    assert!(ok(&v, "clientes").ok);
    // Una venta sin liquidar el IVA.
    issue(&v, date!(2026 - 06 - 01), 100);
    let i = ok(&v, "iva");
    assert!(!i.ok && i.ir == "impuestos");
    let c = cierre::checklist(&v, 2026).unwrap();
    let s = c.items.iter().find(|i| i.id == "saldos").unwrap();
    assert!(!s.ok && s.detail.contains("555"), "{s:?}");
}
