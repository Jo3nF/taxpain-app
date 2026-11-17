//! Préstamos concedidos y recibidos: alta conciliada con el banco, cuotas con retención (4751
//! y 473), devengo de intereses al cierre, reclasificación del largo al corto plazo, 123 y 193
//! con los intereses, deshacer y borrar, y el contrato en RTF. Datos inventados.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::invoicing;
use taxpain_app::prestamos::{self, EstadoAlta, EstadoCuota, Pago};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::{AppError, models};
use taxpain_core::accounting::prestamo::{
    Contraparte, OrigenContraparte, Prestamo, Sentido, Sistema, TipoCarencia, Vinculo,
};
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

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
            id: PartyId::Nif("B12345674".into()),
            name: "Estudio Demo SL".into(),
            address: Some(taxpain_core::invoice::Address {
                street: "Calle Inventada 1".into(),
                post_code: "46001".into(),
                town: "València".into(),
                province: "Valencia".into(),
                country: "ES".into(),
            }),
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

fn balance(v: &Vault, account: &str) -> Money {
    let cents: i64 = v
        .connection()
        .query_row(
            "SELECT COALESCE(SUM(l.debit - l.credit), 0) FROM journal_line l
             JOIN journal_entry e ON e.id = l.entry_id
             WHERE l.account = ?1 AND e.origin_kind <> 'Cierre'",
            [account],
            |r| r.get(0),
        )
        .unwrap();
    Money::from_cents(cents)
}

fn get(d: &models::Draft, b: u16) -> Money {
    d.boxes
        .iter()
        .find(|(k, _)| *k == b)
        .map_or(Money::ZERO, |(_, m)| *m)
}

/// Préstamo recibido de una socia (persona física, vinculada): 24.000 € al 5 % en 24 cuotas
/// mensuales lineales desde el 31/1/2026, con retención del 19 %.
fn de_la_socia() -> Prestamo {
    let contraparte = Contraparte {
        origen: OrigenContraparte::Texto,
        ref_id: None,
        nif: "12345678Z".into(),
        nombre: "Carmen Española Española".into(),
        provincia: "46".into(),
        persona_fisica: true,
        entidad_credito: false,
        vinculo: Vinculo::Otra,
    };
    let pr = prestamos::propuesta(Sentido::Recibido, &contraparte);
    Prestamo {
        id: 0,
        nombre: "Préstamo de la socia".into(),
        sentido: Sentido::Recibido,
        contraparte,
        inicio: date!(2026 - 01 - 31),
        principal: eur(24_000),
        tipo: Rate::percent(5),
        sistema: Sistema::Lineal,
        periodicidad: 1,
        plazo: 24,
        carencia: 0,
        carencia_tipo: TipoCarencia::Capital,
        cuentas: pr.cuentas,
        retencion: pr.retencion,
        notas: String::new(),
    }
}

fn banco(v: &Vault) {
    v.connection()
        .execute(
            "INSERT INTO bank_account(number, account, name) VALUES ('00490001', '5720001', 'Banco')",
            [],
        )
        .unwrap();
}

fn movimiento(v: &Vault, fecha: &str, cents: i64, hash: &str) -> i64 {
    let conn = v.connection();
    conn.execute(
        "INSERT INTO bank_tx(bank_account, op_date, value_date, amount, description, import_hash, created_at)
         VALUES (1, ?1, ?1, ?2, 'TRANSFERENCIA', ?3, '2026-11-01')",
        taxpain_vault::rusqlite::params![fecha, cents, hash],
    )
    .unwrap();
    conn.last_insert_rowid()
}

fn caja() -> Pago {
    Pago {
        tesoreria: Some("5720001".into()),
        bank_tx: None,
    }
}

#[test]
fn recibido_de_la_socia_con_retencion_123_y_193() {
    let v = setup();
    banco(&v);
    let now = datetime!(2026-11-16 10:00 UTC);
    let hoy = date!(2026 - 11 - 16);
    let p = de_la_socia();
    assert_eq!(p.cuentas.largo.as_str(), "1635001");
    assert_eq!(p.cuentas.corto.as_str(), "5135001");
    assert_eq!(p.retencion, Rate::percent(19));
    let id = prestamos::create(&v, &p, now).unwrap();

    // Sin el alta no se registran cuotas.
    let err = prestamos::post_installment(&v, id, 1, &caja(), now, true).unwrap_err();
    assert!(err.to_string().contains("alta"), "{err}");

    // Alta con el movimiento del banco: queda conciliado.
    let tx = movimiento(&v, "2026-01-31", 2_400_000, "alta");
    let otros = prestamos::bank_candidates(&v, id, 0, hoy).unwrap();
    assert_eq!(otros.iter().map(|m| m.id).collect::<Vec<_>>(), [tx]);
    assert!(matches!(
        prestamos::post_start(&v, id, &Pago::default(), now, false),
        Err(AppError::NotConfirmed)
    ));
    let alta = prestamos::post_start(
        &v,
        id,
        &Pago {
            tesoreria: None,
            bank_tx: Some(tx),
        },
        now,
        true,
    )
    .unwrap();
    let matched: Option<i64> = v
        .connection()
        .query_row("SELECT entry_id FROM bank_tx WHERE id = ?1", [tx], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(matched, Some(alta));
    assert_eq!(balance(&v, "5720001"), eur(24_000));
    // 12 cuotas de 1.000 € en los doce meses siguientes: a corto; el resto, a largo.
    assert_eq!(balance(&v, "5135001"), eur(-12_000));
    assert_eq!(balance(&v, "1635001"), eur(-12_000));
    assert!(
        prestamos::bank_candidates(&v, id, 0, hoy)
            .unwrap()
            .is_empty()
    );

    // Cuota 1 (28/2): 1.000 de capital y 100 € de intereses (24.000 × 5 % / 12), con 19 € de
    // retención a la 4751 y 1.081 € al banco.
    let e1 = prestamos::post_installment(&v, id, 1, &caja(), now, true).unwrap();
    assert!(e1 > alta);
    assert_eq!(balance(&v, "4751"), eur(-19));
    assert_eq!(balance(&v, "662"), eur(100));
    assert_eq!(balance(&v, "5135001"), eur(-11_000));
    // Por orden: la 3 no se puede antes que la 2.
    assert!(prestamos::post_installment(&v, id, 3, &caja(), now, true).is_err());
    prestamos::post_installment(&v, id, 2, &caja(), now, true).unwrap();

    // Devengo al 15/4 de la cuota 3 (31/3 → 30/4): 15 de 30 días de 91,67 € = 45,84 €.
    let d = prestamos::accrue(&v, Some(id), date!(2026 - 04 - 15), now, true).unwrap();
    assert_eq!(d.len(), 1);
    assert_eq!((d[0].n, d[0].importe), (3, Money::from_cents(4_584)));
    assert_eq!(balance(&v, "5145"), Money::from_cents(-4_584));
    // Repetirlo no devenga otra vez.
    assert!(
        prestamos::accrue(&v, Some(id), date!(2026 - 04 - 15), now, true)
            .unwrap()
            .is_empty()
    );
    // Al registrar la cuota 3, lo devengado sale de la 5145 y el gasto queda en 91,67 €.
    let vista = &prestamos::list(&v, hoy).unwrap()[0];
    assert_eq!(vista.cuotas[2].devengado, Money::from_cents(4_584));
    prestamos::post_installment(&v, id, 3, &caja(), now, true).unwrap();
    assert_eq!(balance(&v, "5145"), Money::ZERO);
    assert_eq!(
        balance(&v, "662"),
        Money::from_cents(10_000 + 9_583 + 9_167)
    );

    // 123 del 1T: cuotas del 28/2 y 31/3 (100 + 95,83 € de intereses, 19 + 18,21 € de
    // retención).
    let d123 = models::draft(&v, "123", 2026, 1).unwrap();
    assert_eq!(get(&d123, 5), Money::from_cents(19_583));
    assert_eq!(get(&d123, 8), Money::from_cents(1_900 + 1_821));
    assert_eq!(get(&d123, 7), Money::ZERO);
    assert_eq!(d123.result, Money::from_cents(3_721));
    assert!(
        d123.details
            .iter()
            .any(|(t, _)| t.contains("Préstamo de la socia"))
    );
    // 193: un registro de clave B, naturaleza 03, con las tres cuotas.
    let d193 = models::draft(&v, "193", 2026, 0).unwrap();
    assert!(
        d193.details
            .iter()
            .any(|(t, m)| t.contains("clave B naturaleza 03")
                && *m == Money::from_cents(1_900 + 1_821 + 1_742)),
        "{:?}",
        d193.details
    );
    assert!(d193.file.contains("12345678Z"));

    // Reclasificación al 31/12/2026: a largo plazo, lo que vence después del 31/12/2027
    // (enero de 2028: 1.000 €).
    let n = prestamos::reclassify(&v, 2026, now, true).unwrap();
    assert_eq!(n, 1);
    assert_eq!(balance(&v, "1635001"), eur(-1_000));
    assert_eq!(prestamos::reclassify(&v, 2026, now, true).unwrap(), 0);

    // Saldo vivo a hoy según el cuadro: quedan 15 cuotas (después de la de octubre).
    let vista = &prestamos::list(&v, hoy).unwrap()[0];
    assert_eq!(vista.saldo_vivo, eur(15_000));
    assert_eq!(vista.vencidas, 6, "de abril a octubre, sin registrar");
    assert!(
        vista.avisos.iter().any(|a| a.contains("600")),
        "{:?}",
        vista.avisos
    );

    // Deshacer: sólo la última; borrar, sólo sin asientos.
    assert!(prestamos::undo_installment(&v, id, 2, now, true).is_err());
    prestamos::undo_installment(&v, id, 3, now, true).unwrap();
    assert_eq!(
        balance(&v, "5145"),
        Money::from_cents(-4_584),
        "el devengo vuelve a estar pendiente"
    );
    assert!(prestamos::delete(&v, id, now, true).is_err());
    // Con asientos no se cambian las condiciones; el nombre, sí.
    let mut cambio = de_la_socia();
    cambio.tipo = Rate::percent(6);
    assert!(prestamos::update(&v, id, &cambio, now).is_err());
    cambio.tipo = Rate::percent(5);
    cambio.nombre = "Préstamo de Carmen".into();
    prestamos::update(&v, id, &cambio, now).unwrap();

    // Contrato en RTF.
    let rtf = prestamos::contract_rtf(&v, id).unwrap();
    assert!(rtf.starts_with("{\\rtf1"));
    assert!(rtf.contains("CONTRATO DE PR"));
    assert!(rtf.contains("12345678Z") && rtf.contains("B12345674"));
    assert!(rtf.contains("24.000,00"));

    // Todo queda en la auditoría.
    let acciones: Vec<String> = {
        let conn = v.connection();
        let mut st = conn
            .prepare("SELECT action FROM audit_log WHERE action LIKE 'prestamo.%' ORDER BY seq")
            .unwrap();
        st.query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    };
    for a in [
        "prestamo.create",
        "prestamo.alta",
        "prestamo.cuota",
        "prestamo.devengo",
        "prestamo.reclasificacion",
        "prestamo.cuota_undo",
        "prestamo.update",
    ] {
        assert!(acciones.iter().any(|x| x == a), "falta {a} en {acciones:?}");
    }
}

#[test]
fn concedido_a_la_participada_y_de_un_banco() {
    let v = setup();
    let now = datetime!(2026-11-16 10:00 UTC);
    let hoy = date!(2026 - 11 - 16);
    // Concedido a una participada inventada: intereses periódicos trimestrales, 19 % que nos
    // retiene la prestataria (473).
    let contraparte = Contraparte {
        origen: OrigenContraparte::Participada,
        nif: "A22222228".into(),
        nombre: "Participada Inventada SA".into(),
        vinculo: Vinculo::Grupo,
        ..Default::default()
    };
    let pr = prestamos::propuesta(Sentido::Concedido, &contraparte);
    let p = Prestamo {
        id: 0,
        nombre: "Préstamo a la participada".into(),
        sentido: Sentido::Concedido,
        contraparte,
        inicio: date!(2026 - 01 - 01),
        principal: eur(40_000),
        tipo: Rate::percent(4),
        sistema: Sistema::InteresesPeriodicos,
        periodicidad: 3,
        plazo: 36,
        carencia: 0,
        carencia_tipo: TipoCarencia::Capital,
        cuentas: pr.cuentas,
        retencion: pr.retencion,
        notas: String::new(),
    };
    let id = prestamos::create(&v, &p, now).unwrap();
    prestamos::post_start(&v, id, &caja(), now, true).unwrap();
    assert_eq!(balance(&v, "2423001"), eur(40_000));
    prestamos::post_installment(&v, id, 1, &caja(), now, true).unwrap();
    // 400 € de intereses del trimestre: 76 € de retención a la 473 y 324 € al banco.
    assert_eq!(balance(&v, "473"), eur(76));
    assert_eq!(balance(&v, "762"), eur(-400));
    assert_eq!(balance(&v, "5720001"), eur(-40_000 + 324));
    // Los intereses cobrados no van al 123 (los paga la participada).
    let d123 = models::draft(&v, "123", 2026, 1).unwrap();
    assert_eq!(d123.result, Money::ZERO);

    // Recibido de un banco: sin retención; «ya contabilizado» (saldos iniciales) y las cuotas
    // anteriores marcadas como contabilizadas fuera.
    let banco_c = Contraparte {
        nombre: "Banco Inventado".into(),
        nif: "A22222228".into(),
        entidad_credito: true,
        ..Default::default()
    };
    let pr = prestamos::propuesta(Sentido::Recibido, &banco_c);
    let b = Prestamo {
        id: 0,
        nombre: "Préstamo del banco".into(),
        sentido: Sentido::Recibido,
        contraparte: banco_c,
        inicio: date!(2025 - 06 - 15),
        principal: eur(60_000),
        tipo: Rate::from_bp(450),
        sistema: Sistema::Frances,
        periodicidad: 1,
        plazo: 60,
        carencia: 6,
        carencia_tipo: TipoCarencia::Capital,
        cuentas: pr.cuentas,
        retencion: pr.retencion,
        notas: String::new(),
    };
    assert_eq!(b.retencion, Rate::ZERO);
    let bid = prestamos::create(&v, &b, now).unwrap();
    prestamos::mark_start_outside(&v, bid, true, now).unwrap();
    let n = prestamos::mark_outside(&v, bid, date!(2026 - 06 - 30), now, true).unwrap();
    assert_eq!(n, 12, "de julio de 2025 a junio de 2026");
    let vista = prestamos::list(&v, hoy)
        .unwrap()
        .into_iter()
        .find(|x| x.prestamo.id == bid)
        .unwrap();
    assert_eq!(vista.alta, EstadoAlta::Fuera);
    assert_eq!(vista.cuotas[11].estado, EstadoCuota::Fuera);
    assert_eq!(vista.cuotas[12].estado, EstadoCuota::Pendiente);
    assert_eq!(vista.vencidas, 5, "de julio a noviembre de 2026");
    // Ni el banco ni sus intereses van al 123.
    assert_eq!(
        models::draft(&v, "123", 2026, 2).unwrap().result,
        Money::ZERO
    );
    // Sin asientos, se borra (con sus cuotas marcadas).
    prestamos::delete(&v, bid, now, true).unwrap();
    assert_eq!(prestamos::list(&v, hoy).unwrap().len(), 1);
}
