//! Autónomo societario: ficha del RETA, rendimiento computable con nóminas, cuota pagada por la
//! sociedad (especie) y dividendos, regularización estimada, asiento de la cuota y recibo del
//! administrador. Cifras inventadas.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::journal;
use taxpain_app::autonomo_societario::{
    CuotaInput, Pagador, delete_cuota, ficha, recibo, recibo_pdf, register_cuota, resumen,
    save_ficha,
};
use taxpain_app::invoicing;
use taxpain_app::payroll::{Employee, PayslipInput, register, save_employee};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::socios::{self, PayoutInput, Shareholder};
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_core::reta::Sentido;
use taxpain_models::informative::DirectorSs;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

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
    invoicing::save_settings(&v, &s, datetime!(2025-01-01 9:00 UTC)).unwrap();
    v
}

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn administradora(v: &Vault, regime: DirectorSs) -> i64 {
    save_employee(
        v,
        &Employee {
            nif: "12345678Z".into(),
            name: "Española Española Carmen".into(),
            province: "46".into(),
            active: true,
            administrator: true,
            ss_regime: regime,
            ..Default::default()
        },
        datetime!(2025-01-01 10:00 UTC),
    )
    .unwrap()
}

#[test]
fn rendimiento_tramo_regularizacion_y_cuotas() {
    let v = setup();
    let now = datetime!(2025-12-01 10:00 UTC);
    let id = administradora(&v, DirectorSs::Autonomo);

    // Doce nóminas de 2.000 € con la retención del 19 % (sociedad pequeña).
    for m in 1..=12u8 {
        let month = time::Month::try_from(m).unwrap();
        let d = time::Date::from_calendar_date(2025, month, 28).unwrap();
        register(
            &v,
            &PayslipInput {
                employee_id: id,
                date: d,
                gross: eur(2_000),
                irpf: eur(380),
                ss_employee: Money::ZERO,
                ss_company: Money::ZERO,
                small_entity: Some(true),
            },
            now,
        )
        .unwrap();
    }

    // Propuesta de partida: base mínima de los societarios, la sociedad paga, 642.
    let f = ficha(&v, id, 2025).unwrap();
    assert!(!f.guardada);
    assert_eq!(f.base_elegida, eur(1_000));
    assert_eq!(f.pagador, Pagador::Sociedad);
    assert_eq!(f.cuenta_gasto, "642");

    let mut f = f;
    f.base_elegida = eur(1_000);
    f.rendimiento_previsto = Some(eur(1_200));
    f.cuenta_tesoreria = "5720001".into();
    f.estatutos_retribuido = true;
    f.acuerdo_fecha = Some(date!(2024 - 06 - 30));
    f.acuerdo_importe = Some(eur(26_000));
    save_ficha(&v, &f, now).unwrap();
    // Cuentas de otro grupo: no.
    assert!(
        save_ficha(
            &v,
            &taxpain_app::autonomo_societario::FichaReta {
                cuenta_gasto: "4650".into(),
                ..f.clone()
            },
            now
        )
        .is_err()
    );

    // Cuota de 314 € al mes pagada por la sociedad, con asiento 642 / 5720001.
    let mut ids = Vec::new();
    for m in 1..=12u8 {
        let month = time::Month::try_from(m).unwrap();
        let d = time::Date::from_calendar_date(2025, month, 28).unwrap();
        ids.push(
            register_cuota(
                &v,
                &CuotaInput {
                    employee_id: id,
                    date: d,
                    amount: eur(314),
                    pagador: Pagador::Sociedad,
                    asiento: true,
                },
                now,
            )
            .unwrap(),
        );
    }
    let entries: Vec<_> = journal(&v, 2025)
        .unwrap()
        .into_iter()
        .filter(|e| e.entry.concept.starts_with("Cuota RETA"))
        .collect();
    assert_eq!(entries.len(), 12);
    let l = &entries[0].entry.lines;
    assert_eq!(l[0].account.as_str(), "642");
    assert_eq!(l[0].debit, eur(314));
    assert_eq!(l[1].account.as_str(), "5720001");
    assert_eq!(l[1].credit, eur(314));

    // Dividendo de 6.000 € al 100 % de la socia.
    socios::save_shareholder(
        &v,
        &Shareholder {
            nif: "12345678Z".into(),
            name: "Española Española Carmen".into(),
            province: "46".into(),
            country: "ES".into(),
            pct: 1_000_000,
            resident: true,
            active: true,
            ..Default::default()
        },
        now,
    )
    .unwrap();
    let conn = v.connection();
    let tx = conn.unchecked_transaction().unwrap();
    taxpain_app::accounting::record_entry(
        &tx,
        &taxpain_core::accounting::JournalEntry {
            date: date!(2025 - 06 - 30),
            concept: "Dividendo acordado por la junta".into(),
            origin: taxpain_core::accounting::Origin::Manual,
            lines: vec![
                taxpain_core::accounting::EntryLine::signed(
                    taxpain_core::accounting::account::acc("113"),
                    eur(6_000),
                    true,
                )
                .unwrap(),
                taxpain_core::accounting::EntryLine::signed(
                    taxpain_core::accounting::account::acc("526"),
                    eur(6_000),
                    false,
                )
                .unwrap(),
            ],
        },
        now,
    )
    .unwrap();
    tx.commit().unwrap();
    socios::pay(
        &v,
        &PayoutInput {
            date: date!(2025 - 07 - 15),
            gross: eur(6_000),
            treasury: "5720001".into(),
            agreement: None,
        },
        now,
        true,
    )
    .unwrap();

    // 24.000 + 3.768 (especie) + 6.000 = 33.768 € − 3 % = 32.754,96 €; 2.729,58 €/mes →
    // General 7 (mínima 1.356,21 €): con 1.000 € de base hay que ingresar la diferencia.
    let r = resumen(&v, id, 2025, date!(2025 - 11 - 16)).unwrap();
    assert_eq!(r.dinerarias, eur(24_000));
    assert_eq!(r.especie, eur(3_768));
    assert_eq!(r.dividendos, eur(6_000));
    assert_eq!(r.rendimiento.computable, Money::from_cents(3_275_496));
    assert_eq!(r.rendimiento.mensual, Money::from_cents(272_958));
    let reg = r.regularizacion.as_ref().unwrap();
    assert_eq!(reg.tramo_nombre, "General 7");
    assert_eq!(reg.sentido, Sentido::AIngresar);
    assert_eq!(reg.importe, Money::from_cents(511_020 - 376_800));
    assert_eq!(r.base_recomendada, Some(Money::from_cents(135_621)));
    assert_eq!(r.cuota_mensual, eur(314));
    assert!(r.avisos.iter().any(|a| a.contains("reclamará")));
    assert!(r.avisos.iter().any(|a| a.contains("no coincide")));
    // 27.768 € pagados frente a 26.000 € aprobados.
    assert_eq!(r.retribucion_total, eur(27_768));
    assert!(r.avisos.iter().any(|a| a.contains("supera el máximo")));
    assert!(r.avisos.iter().any(|a| a.contains("111")));
    // El ingreso a cuenta depende de la cifra de negocios, que aquí no se sabe.
    assert_eq!(r.tipo_retencion, None);
    assert_eq!(r.ventana_abierta.efectos, date!(2026 - 01 - 01));
    assert_eq!(r.proximas_ventanas.len(), 3);

    // Sin control efectivo, los dividendos no computan.
    save_ficha(
        &v,
        &taxpain_app::autonomo_societario::FichaReta {
            control_efectivo: false,
            ..f.clone()
        },
        now,
    )
    .unwrap();
    let r = resumen(&v, id, 2025, date!(2025 - 11 - 16)).unwrap();
    assert_eq!(r.rendimiento.dividendos_computados, Money::ZERO);
    assert_eq!(r.dias_alta_societario, 365);
    assert!(r.noventa_dias);
    assert_eq!(r.rendimiento.deduccion_pct, Rate::percent(3));

    // Alta como societaria el 15 de octubre: 78 días en 2025, menos de noventa. Sin el 3 % ni
    // la base mínima de los societarios (art. 308.1 TRLGSS; DT 7.ª RDL 13/2022).
    save_ficha(
        &v,
        &taxpain_app::autonomo_societario::FichaReta {
            alta_societario: Some(date!(2025 - 10 - 15)),
            meses_alta: 3,
            ..f.clone()
        },
        now,
    )
    .unwrap();
    let r = resumen(&v, id, 2025, date!(2025 - 11 - 16)).unwrap();
    assert_eq!(r.ficha.alta_societario, Some(date!(2025 - 10 - 15)));
    assert_eq!(r.dias_alta_societario, 78);
    assert!(!r.noventa_dias);
    assert_eq!(r.rendimiento.deduccion_pct, Rate::percent(7));
    assert!(
        r.avisos
            .iter()
            .any(|a| a.contains("78 días de alta como societario"))
    );
    // Con el alta el 3 de octubre ya son noventa: vuelve el 3 %.
    save_ficha(
        &v,
        &taxpain_app::autonomo_societario::FichaReta {
            alta_societario: Some(date!(2025 - 10 - 03)),
            meses_alta: 3,
            ..f.clone()
        },
        now,
    )
    .unwrap();
    let r = resumen(&v, id, 2025, date!(2025 - 11 - 16)).unwrap();
    assert_eq!(r.dias_alta_societario, 90);
    assert_eq!(r.rendimiento.deduccion_pct, Rate::percent(3));
    save_ficha(&v, &f, now).unwrap();

    // El año siguiente propone la ficha de 2025 (sin guardar).
    let f26 = ficha(&v, id, 2026).unwrap();
    assert!(!f26.guardada);
    assert_eq!(f26.year, 2026);
    assert_eq!(f26.acuerdo_importe, Some(eur(26_000)));

    // Recibo de diciembre: 2.000 € dinerarios + 314 € en especie, retención 380 €, 19 %.
    let slip_id: i64 = v
        .connection()
        .query_row(
            "SELECT id FROM payslip WHERE date = '2025-12-28'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let rc = recibo(&v, slip_id).unwrap();
    assert_eq!(rc.dineraria, eur(2_000));
    assert_eq!(rc.especie, eur(314));
    assert_eq!(rc.retencion, eur(380));
    assert_eq!(rc.liquido, eur(1_620));
    assert_eq!(rc.subclave, Some(3));
    assert_eq!(rc.tipo_retencion, Some(Rate::percent(19)));
    assert_eq!(rc.ingreso_a_cuenta, Money::from_cents(5_966));
    let (name, pdf) = recibo_pdf(&v, slip_id, "Valencia, 28 de diciembre de 2025").unwrap();
    assert_eq!(name, "Recibo administrador 12345678Z 2025-12.pdf");
    assert!(pdf.starts_with(b"%PDF"));

    // Borrar una cuota borra su asiento.
    assert!(delete_cuota(&v, ids[0], now, false).is_err());
    delete_cuota(&v, ids[0], now, true).unwrap();
    let left = journal(&v, 2025)
        .unwrap()
        .into_iter()
        .filter(|e| e.entry.concept.starts_with("Cuota RETA"))
        .count();
    assert_eq!(left, 11);
}

#[test]
fn solo_para_administradores_en_el_reta() {
    let v = setup();
    let now = datetime!(2025-12-01 10:00 UTC);
    let id = administradora(&v, DirectorSs::Asimilado);
    assert!(resumen(&v, id, 2025, date!(2025 - 11 - 16)).is_err());
    assert!(
        register_cuota(
            &v,
            &CuotaInput {
                employee_id: id,
                date: date!(2025 - 01 - 31),
                amount: eur(300),
                pagador: Pagador::Sociedad,
                asiento: true,
            },
            now,
        )
        .is_err()
    );
    // Cuota que paga la administradora: sin asiento aunque se pida.
    let v = setup();
    let id = administradora(&v, DirectorSs::Autonomo);
    register_cuota(
        &v,
        &CuotaInput {
            employee_id: id,
            date: date!(2025 - 01 - 31),
            amount: eur(300),
            pagador: Pagador::Administrador,
            asiento: true,
        },
        now,
    )
    .unwrap();
    assert!(journal(&v, 2025).unwrap().is_empty());
    let r = resumen(&v, id, 2025, date!(2025 - 11 - 16)).unwrap();
    assert_eq!(r.especie, Money::ZERO);
    assert_eq!(r.cuotas_total, eur(300));
    assert!(r.avisos.iter().any(|a| a.contains("estatutos")));
    assert!(r.avisos.iter().any(|a| a.contains("217.3")));
}
