//! Corregir una factura recibida ya revisada, deshacer su revisión y los avisos antes de
//! confirmar (carencia G11).

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::corregir_recibida::{comprobar, comprobar_deshacer, corregir, deshacer};
use taxpain_app::invoicing;
use taxpain_app::received::{Reviewed, Source, ingest_file, list_received, register_typed, review};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::rusqlite::params;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::OffsetDateTime;
use time::macros::{date, datetime};

const NOW: OffsetDateTime = datetime!(2026-10-13 10:00 UTC);

fn vault() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("B12345674".into()),
            name: "Estudio Demo SL".into(),
            address: None,
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

/// Factura de un proveedor inventado: 1.000 € + 21 % = 1.210 €.
fn factura() -> Reviewed {
    Reviewed {
        supplier_nif: "A22222228".into(),
        supplier_name: "Suministros Inventados SA".into(),
        number: "F-100".into(),
        issue_date: date!(2026 - 10 - 05),
        base: Money::from_euros(1000, 0),
        vat: Money::from_euros(210, 0),
        withholding: Money::ZERO,
        total: Money::from_euros(1210, 0),
        deductible_pct: 100,
        capital_good: false,
        nature: Default::default(),
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

/// (asiento, ejercicio, número) de la recibida.
fn entry(v: &Vault, id: i64) -> (i64, i32, i64) {
    v.connection()
        .query_row(
            "SELECT id, fiscal_year, number FROM journal_entry
              WHERE origin_kind = 'FacturaRecibida' AND origin_id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap()
}

/// Líneas del asiento: (id, cuenta, debe, haber).
fn lines(v: &Vault, entry: i64) -> Vec<(i64, String, i64, i64)> {
    let conn = v.connection();
    let mut st = conn
        .prepare(
            "SELECT id, account, debit, credit FROM journal_line WHERE entry_id = ?1 ORDER BY id",
        )
        .unwrap();
    st.query_map([entry], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
    })
    .unwrap()
    .collect::<Result<_, _>>()
    .unwrap()
}

fn line_of(ls: &[(i64, String, i64, i64)], prefix: &str) -> (i64, i64, i64) {
    let l = ls.iter().find(|l| l.1.starts_with(prefix)).unwrap();
    (l.0, l.2, l.3)
}

fn count(v: &Vault, sql: &str, id: i64) -> i64 {
    v.connection().query_row(sql, [id], |r| r.get(0)).unwrap()
}

/// 303 de ese periodo dado por presentado (importado en «Modelos presentados»).
fn filed_303(v: &Vault, year: i32, period: &str) {
    v.connection()
        .execute(
            "INSERT INTO filed_model(model, year, period, filename, file_sha256, source, boxes, imported_at)
             VALUES ('303', ?1, ?2, 'm303.txt', ?3, 'manual', '{}', '2026-10-20T10:00:00Z')",
            params![year, period, format!("sha-{year}-{period}")],
        )
        .unwrap();
}

#[test]
fn corregir_rehace_el_asiento_en_su_sitio_con_punteos_y_reparto() {
    let v = vault();
    let id = register_typed(&v, &factura(), NOW).unwrap();
    let (e, year, number) = entry(&v, id);
    let before = lines(&v, e);
    let (iva, _, _) = line_of(&before, "472");
    let (prov, _, credit) = line_of(&before, "41");
    let (gasto, _, _) = line_of(&before, "6");
    assert_eq!(credit, 121_000);
    let conn = v.connection();
    // Punteo del IVA y del proveedor, reparto analítico del gasto y una observación.
    for l in [iva, prov] {
        conn.execute(
            "INSERT INTO line_tick(line_id, ticked_at) VALUES (?1, '2026-10-14T10:00:00Z')",
            [l],
        )
        .unwrap();
    }
    conn.execute(
        "INSERT INTO cost_center(code, name, created_at) VALUES ('T', 'Taller', '2026-10-14')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO line_cost(line_id, center, pct_bp) VALUES (?1, 'T', 10000)",
        [gasto],
    )
    .unwrap();
    conn.execute(
        "UPDATE journal_entry SET notes = 'pedido 7' WHERE id = ?1",
        [e],
    )
    .unwrap();
    let registered: String = conn
        .query_row(
            "SELECT reviewed_at FROM invoice_received WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();

    // Se confirmó al 100 % y es un vehículo de uso mixto: el 50 % (el total no cambia).
    let corrected = Reviewed {
        deductible_pct: 50,
        issue_date: date!(2026 - 10 - 06),
        ..factura()
    };
    let later = datetime!(2026-10-20 9:00 UTC);
    let c = comprobar(&v, id, &corrected, None, later).unwrap();
    assert!(c.correccion);
    assert_eq!(c.trimestre, "4T 2026");
    assert_eq!(c.antes.as_ref().unwrap().deducible_pct, 100);
    assert_eq!(c.despues.deducible_pct, 50);
    assert!(c.avisos.is_empty(), "{:?}", c.avisos);

    let out = corregir(&v, id, &corrected, None, later).unwrap();
    assert_eq!(out.ejercicio_anterior, None);
    assert_eq!(out.punteos_quitados, 1, "el del IVA, que cambia de importe");

    // Mismo asiento (número y observaciones); líneas actualizadas, no borradas.
    let (e2, year2, number2) = entry(&v, id);
    assert_eq!((e2, year2, number2), (e, year, number));
    let after = lines(&v, e);
    assert_eq!(line_of(&after, "472"), (iva, 10_500, 0));
    assert_eq!(line_of(&after, "41"), (prov, 0, 121_000));
    assert_eq!(line_of(&after, "6"), (gasto, 110_500, 0));
    assert_eq!(after.len(), before.len());
    let conn = v.connection();
    assert_eq!(
        count(
            &v,
            "SELECT COUNT(*) FROM line_tick WHERE line_id = ?1",
            prov
        ),
        1
    );
    assert_eq!(
        count(&v, "SELECT COUNT(*) FROM line_tick WHERE line_id = ?1", iva),
        0
    );
    assert_eq!(
        count(
            &v,
            "SELECT COUNT(*) FROM line_cost WHERE line_id = ?1",
            gasto
        ),
        1
    );
    let (date, notes): (String, String) = conn
        .query_row(
            "SELECT date, notes FROM journal_entry WHERE id = ?1",
            [e],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((date.as_str(), notes.as_str()), ("2026-10-06", "pedido 7"));

    // La fila: datos nuevos y la misma fecha de registro (sigue en su 303).
    let (pct, issue, at): (i64, String, String) = conn
        .query_row(
            "SELECT deductible_pct, issue_date, reviewed_at FROM invoice_received WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!((pct, issue.as_str()), (50, "2026-10-06"));
    assert_eq!(at, registered);
    let listed = list_received(&v, Some("revisada")).unwrap();
    assert_eq!(listed[0].reviewed.as_ref().unwrap().deductible_pct, 50);
    assert_eq!(listed[0].registered_on, Some(date!(2026 - 10 - 13)));

    // Auditoría con el antes y el después.
    let details: String = conn
        .query_row(
            "SELECT details FROM audit_log WHERE action = 'received.correct'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let j: serde_json::Value = serde_json::from_str(&details).unwrap();
    assert_eq!(j["before"]["reviewed"]["deductible_pct"], 100);
    assert_eq!(j["after"]["reviewed"]["deductible_pct"], 50);
    assert!(j["entry_before"]["lines"].is_array());

    // Corregir sin cambiar nada no hace nada.
    let err = corregir(&v, id, &corrected, None, later).unwrap_err();
    assert!(err.to_string().contains("no has cambiado nada"), "{err}");
}

#[test]
fn corregir_el_proveedor_cambia_su_subcuenta_y_la_fecha_cambia_de_ejercicio() {
    let v = vault();
    let first = Reviewed {
        issue_date: date!(2026 - 01 - 10),
        ..factura()
    };
    let id = register_typed(&v, &first, NOW).unwrap();
    let (e, year, _) = entry(&v, id);
    assert_eq!(year, 2026);
    let old_supplier = line_of(&lines(&v, e), "41");
    let corrected = Reviewed {
        supplier_nif: "12345678Z".into(),
        supplier_name: "Carmen Española Española".into(),
        issue_date: date!(2025 - 12 - 20),
        ..first
    };
    let c = comprobar(&v, id, &corrected, None, NOW).unwrap();
    assert!(
        c.avisos
            .iter()
            .any(|a| a.contains("del ejercicio 2026 al 2025")),
        "{:?}",
        c.avisos
    );
    let out = corregir(&v, id, &corrected, None, NOW).unwrap();
    assert_eq!(out.ejercicio_anterior, Some(2026));
    let (e2, year2, _) = entry(&v, id);
    assert_eq!((e2, year2), (e, 2025));
    let after = lines(&v, e);
    let new_supplier = after.iter().find(|l| l.1.starts_with("41")).unwrap();
    assert_ne!(new_supplier.0, old_supplier.0, "otra subcuenta: otra línea");
    assert_eq!(new_supplier.3, 121_000);
    assert_eq!(after.len(), 3);
}

#[test]
fn pagada_se_corrige_sin_romper_el_pago_y_no_se_deshace() {
    let v = vault();
    let id = register_typed(&v, &factura(), NOW).unwrap();
    let pay =
        taxpain_app::cartera::settle(&v, "recibida", id, 1, date!(2026 - 10 - 15), "572", NOW)
            .unwrap();
    // El total cambia: no, rompería el pago.
    let wrong_total = Reviewed {
        base: Money::from_euros(900, 0),
        vat: Money::from_euros(189, 0),
        total: Money::from_euros(1089, 0),
        ..factura()
    };
    let err = comprobar(&v, id, &wrong_total, None, NOW).unwrap_err();
    assert!(err.to_string().contains("ya está pagada"), "{err}");
    assert!(corregir(&v, id, &wrong_total, None, NOW).is_err());
    // Otro desglose con el mismo total (un tipo del 10 % mal tecleado como 21 %): sí.
    let same_total = Reviewed {
        base: Money::from_euros(1100, 0),
        vat: Money::from_euros(110, 0),
        ..factura()
    };
    let c = comprobar(&v, id, &same_total, None, NOW).unwrap();
    assert!(
        c.avisos.iter().any(|a| a.contains("el pago no cambia")),
        "{:?}",
        c.avisos
    );
    corregir(&v, id, &same_total, None, NOW).unwrap();
    let (e, ..) = entry(&v, id);
    assert_eq!(line_of(&lines(&v, e), "472").1, 11_000);
    // El pago sigue: su asiento y su registro en Cartera.
    assert_eq!(
        count(&v, "SELECT COUNT(*) FROM journal_entry WHERE id = ?1", pay),
        1
    );
    assert_eq!(
        count(
            &v,
            "SELECT COUNT(*) FROM settlement WHERE kind = 'recibida' AND invoice_id = ?1",
            id
        ),
        1
    );
    // Deshacer la revisión, no: tiene un pago.
    let err = comprobar_deshacer(&v, id).unwrap_err();
    assert!(err.to_string().contains("Cartera"), "{err}");
    assert!(deshacer(&v, id, NOW).is_err());
}

#[test]
fn ejercicio_cerrado_no_se_corrige_ni_se_deshace() {
    let v = vault();
    let id = register_typed(&v, &factura(), NOW).unwrap();
    v.connection()
        .execute("UPDATE fiscal_year SET closed = 1 WHERE year = 2026", [])
        .unwrap();
    let corrected = Reviewed {
        deductible_pct: 50,
        ..factura()
    };
    let err = corregir(&v, id, &corrected, None, NOW).unwrap_err();
    assert!(err.to_string().contains("está cerrado"), "{err}");
    assert!(err.to_string().contains("reabrirlo"), "{err}");
    let err = comprobar_deshacer(&v, id).unwrap_err();
    assert!(err.to_string().contains("está cerrado"), "{err}");
    // Tampoco moviéndola a un ejercicio cerrado desde uno abierto.
    let v = vault();
    let id = register_typed(&v, &factura(), NOW).unwrap();
    v.connection()
        .execute(
            "INSERT INTO fiscal_year(year, start_date, end_date, closed)
             VALUES (2025, '2025-01-01', '2025-12-31', 1)",
            [],
        )
        .unwrap();
    let to_closed = Reviewed {
        issue_date: date!(2025 - 12 - 30),
        ..factura()
    };
    let err = corregir(&v, id, &to_closed, None, NOW).unwrap_err();
    assert!(err.to_string().contains("2025 está cerrado"), "{err}");
}

#[test]
fn avisa_si_el_303_esta_presentado_o_la_liquidacion_contabilizada() {
    let v = vault();
    let id = register_typed(&v, &factura(), NOW).unwrap();
    let corrected = Reviewed {
        deductible_pct: 50,
        ..factura()
    };
    // Registrada el 13/10/2026: su 303 es el del 4T.
    filed_303(&v, 2026, "4T");
    let c = comprobar(&v, id, &corrected, None, NOW).unwrap();
    let aviso = c.avisos.iter().find(|a| a.contains("303")).unwrap();
    assert!(aviso.contains("4T de 2026 ya está presentado"), "{aviso}");
    assert!(aviso.contains("rectificativa"), "{aviso}");
    assert!(aviso.contains("114 LIVA"), "{aviso}");
    // Se avisa, no se impide (y no se presenta nada): la corrección se guarda con el aviso.
    let out = corregir(&v, id, &corrected, None, NOW).unwrap();
    assert_eq!(out.avisos, c.avisos);
    let v = vault();
    let id = register_typed(&v, &factura(), NOW).unwrap();
    // Liquidación del IVA del 4T ya en el diario (sin 303 presentado).
    v.connection()
        .execute(
            "INSERT INTO journal_entry(fiscal_year, number, date, concept, origin_kind, origin_id, created_at)
             VALUES (2026, 99, '2026-12-31', 'Liquidación IVA 4T', 'LiquidacionIva', 202604, '2026-10-13')",
            [],
        )
        .unwrap();
    let c = comprobar(&v, id, &corrected, None, NOW).unwrap();
    assert!(
        c.avisos
            .iter()
            .any(|a| a.contains("liquidación del IVA del 4T 2026") && a.contains("rehazla")),
        "{:?}",
        c.avisos
    );
    // Un cambio que no toca el IVA (sólo la fecha de una interior) no avisa del 303.
    let only_date = Reviewed {
        issue_date: date!(2026 - 10 - 04),
        ..factura()
    };
    let c = comprobar(&v, id, &only_date, None, NOW).unwrap();
    assert!(c.avisos.is_empty(), "{:?}", c.avisos);
}

#[test]
fn confirmar_una_pendiente_ensena_el_resumen_y_el_trimestre() {
    let v = vault();
    let pending = ingest_file(
        &v,
        "factura.txt",
        b"una factura escrita",
        Source::Manual,
        NOW,
    )
    .unwrap()
    .id;
    let late = Reviewed {
        issue_date: date!(2026 - 09 - 28),
        ..factura()
    };
    let c = comprobar(&v, pending, &late, None, NOW).unwrap();
    assert!(!c.correccion);
    assert!(c.antes.is_none());
    assert_eq!(c.trimestre, "4T 2026");
    assert_eq!(c.despues.proveedor_nif, "A22222228");
    assert_eq!(c.despues.total, Money::from_euros(1210, 0));
    assert!(
        c.avisos
            .iter()
            .any(|a| a.contains("es del 3T 2026") && a.contains("303 del 4T 2026")),
        "{:?}",
        c.avisos
    );
    // Comprobar no graba nada.
    assert_eq!(
        list_received(&v, Some("pendiente")).unwrap().len(),
        1,
        "sigue pendiente"
    );
    // Los mismos errores que la revisión, antes de confirmar.
    let bad = Reviewed {
        total: Money::from_euros(1300, 0),
        ..factura()
    };
    let err = comprobar(&v, pending, &bad, None, NOW).unwrap_err();
    assert!(err.to_string().contains("no cuadran"), "{err}");
}

#[test]
fn deshacer_la_revision_la_devuelve_a_pendiente_con_sus_datos() {
    let v = vault();
    let id = register_typed(&v, &factura(), NOW).unwrap();
    let (e, ..) = entry(&v, id);
    let avisos = comprobar_deshacer(&v, id).unwrap();
    assert!(
        avisos.iter().any(|a| a.contains("303 del 4T 2026")),
        "{avisos:?}"
    );
    deshacer(&v, id, datetime!(2026-10-20 9:00 UTC)).unwrap();
    assert_eq!(
        count(&v, "SELECT COUNT(*) FROM journal_entry WHERE id = ?1", e),
        0
    );
    assert_eq!(
        count(
            &v,
            "SELECT COUNT(*) FROM journal_line WHERE entry_id = ?1",
            e
        ),
        0
    );
    let p = list_received(&v, Some("pendiente")).unwrap();
    assert_eq!(p.len(), 1);
    let ex = &p[0].extraction;
    assert_eq!(ex.number.as_ref().unwrap().value, "F-100");
    assert_eq!(ex.base.as_ref().unwrap().value, Money::from_euros(1000, 0));
    assert!(ex.warnings.iter().any(|w| w.contains("revisión deshecha")));
    assert!(p[0].reviewed.is_none());
    let at: Option<String> = v
        .connection()
        .query_row(
            "SELECT reviewed_at FROM invoice_received WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(at.is_none());
    let audited: i64 = v
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE action = 'received.unreview'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(audited, 1);
    // Se puede revisar de nuevo (otro asiento).
    review(&v, id, &factura(), datetime!(2026-10-21 9:00 UTC)).unwrap();
    let (e2, ..) = entry(&v, id);
    assert_eq!(lines(&v, e2).len(), 3);
    // Una que no está revisada no se deshace ni se corrige.
    deshacer(&v, id, NOW).unwrap();
    assert!(deshacer(&v, id, NOW).is_err());
    assert!(corregir(&v, id, &factura(), None, NOW).is_err());
}

#[test]
fn bien_de_inversion_y_asiento_con_documentos_no_se_tocan_aqui() {
    let v = vault();
    let computer = Reviewed {
        base: Money::from_euros(4000, 0),
        vat: Money::from_euros(840, 0),
        total: Money::from_euros(4840, 0),
        capital_good: true,
        asset_class: Some(taxpain_app::received::AssetClass::Informatica),
        ..factura()
    };
    let id = register_typed(&v, &computer, NOW).unwrap();
    let err = corregir(
        &v,
        id,
        &Reviewed {
            deductible_pct: 50,
            ..computer.clone()
        },
        None,
        NOW,
    )
    .unwrap_err();
    assert!(err.to_string().contains("inmovilizado"), "{err}");
    assert!(comprobar_deshacer(&v, id).is_err());
    // Pasar una ya revisada a bien de inversión: deshaciendo y revisando otra vez.
    let id = register_typed(
        &v,
        &Reviewed {
            number: "F-101".into(),
            ..factura()
        },
        NOW,
    )
    .unwrap();
    let err = comprobar(
        &v,
        id,
        &Reviewed {
            number: "F-101".into(),
            ..computer
        },
        None,
        NOW,
    )
    .unwrap_err();
    assert!(err.to_string().contains("deshaz la revisión"), "{err}");
    // Un asiento con observaciones no se borra al deshacer.
    let (e, ..) = entry(&v, id);
    v.connection()
        .execute("UPDATE journal_entry SET notes = 'ojo' WHERE id = ?1", [e])
        .unwrap();
    let err = comprobar_deshacer(&v, id).unwrap_err();
    assert!(err.to_string().contains("observaciones"), "{err}");
}
