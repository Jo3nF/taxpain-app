//! Datos para la Renta y el Patrimonio de la socia administradora (G34): nóminas con su
//! retención, cuota del RETA pagada por la sociedad, dividendos, préstamo sin interés, valor
//! teórico de las participaciones y semáforo de la exención. Cifras inventadas.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::autonomo_societario::{CuotaInput, Pagador, register_cuota};
use taxpain_app::invoicing;
use taxpain_app::payroll::{Employee, PayslipInput, register, save_employee};
use taxpain_app::prestamos;
use taxpain_app::renta_socio::{Semaforo, informe, pdf, personas};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::socios::{self, PayoutInput, Shareholder};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::prestamo::{
    Contraparte, OrigenContraparte, Prestamo, Sentido, Sistema, TipoCarencia, Vinculo,
};
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
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

fn asiento(v: &Vault, fecha: time::Date, concepto: &str, lineas: &[(&str, i64, bool)]) {
    let conn = v.connection();
    let tx = conn.unchecked_transaction().unwrap();
    taxpain_app::accounting::record_entry(
        &tx,
        &JournalEntry {
            date: fecha,
            concept: concepto.into(),
            origin: Origin::Manual,
            lines: lineas
                .iter()
                .map(|(c, e, debe)| EntryLine::signed(acc(c), eur(*e), *debe).unwrap())
                .collect(),
        },
        datetime!(2025-01-01 10:00 UTC),
    )
    .unwrap();
    tx.commit().unwrap();
}

#[test]
fn informe_de_la_socia_administradora() {
    let v = setup();
    let now = datetime!(2025-12-01 10:00 UTC);
    let id = save_employee(
        &v,
        &Employee {
            nif: "12345678Z".into(),
            name: "Española Española Carmen".into(),
            province: "46".into(),
            active: true,
            administrator: true,
            ss_regime: DirectorSs::Autonomo,
            ..Default::default()
        },
        now,
    )
    .unwrap();
    socios::save_shareholder(
        &v,
        &Shareholder {
            nif: "12345678Z".into(),
            name: "Española Española Carmen".into(),
            province: "46".into(),
            country: "ES".into(),
            pct: 600_000,
            resident: true,
            active: true,
            ..Default::default()
        },
        now,
    )
    .unwrap();
    socios::save_shareholder(
        &v,
        &Shareholder {
            nif: "A22222228".into(),
            name: "Inversiones Demo SA".into(),
            province: "46".into(),
            country: "ES".into(),
            pct: 400_000,
            legal_person: true,
            resident: true,
            active: true,
            ..Default::default()
        },
        now,
    )
    .unwrap();
    // Balance de 2024: capital 3.000 y reservas 97.000 → patrimonio neto de 100.000 €.
    asiento(
        &v,
        date!(2024 - 12 - 31),
        "Saldos de 2024",
        &[
            ("5720001", 100_000, true),
            ("100", 3_000, false),
            ("113", 97_000, false),
        ],
    );
    for m in 1..=12u8 {
        let d =
            time::Date::from_calendar_date(2025, time::Month::try_from(m).unwrap(), 28).unwrap();
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
        register_cuota(
            &v,
            &CuotaInput {
                employee_id: id,
                date: d,
                amount: eur(300),
                pagador: Pagador::Sociedad,
                asiento: false,
            },
            now,
        )
        .unwrap();
    }
    // Dividendo de 10.000 € a repartir: a la socia, el 60 % (6.000 €, retención de 1.140 €).
    asiento(
        &v,
        date!(2025 - 06 - 30),
        "Dividendo acordado por la junta",
        &[("113", 10_000, true), ("526", 10_000, false)],
    );
    socios::pay(
        &v,
        &PayoutInput {
            date: date!(2025 - 07 - 15),
            gross: eur(10_000),
            treasury: "5720001".into(),
            agreement: None,
        },
        now,
        true,
    )
    .unwrap();
    // Préstamo de la socia a la sociedad sin interés: 10.000 € el 1 de enero de 2025.
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
    prestamos::create(
        &v,
        &Prestamo {
            id: 0,
            nombre: "Préstamo de la socia".into(),
            sentido: Sentido::Recibido,
            contraparte,
            inicio: date!(2025 - 01 - 01),
            principal: eur(10_000),
            tipo: Rate::ZERO,
            sistema: Sistema::Lineal,
            periodicidad: 12,
            plazo: 60,
            carencia: 0,
            carencia_tipo: TipoCarencia::Capital,
            cuentas: pr.cuentas,
            retencion: pr.retencion,
            notas: String::new(),
        },
        now,
    )
    .unwrap();

    let ps = personas(&v).unwrap();
    assert_eq!(ps.len(), 1);
    assert!(ps[0].socio && ps[0].administrador);

    let hoy = date!(2026 - 03 - 01);
    let i = informe(&v, 2025, "12345678z", None, hoy).unwrap();
    let c = |t: &str| i.conceptos.iter().find(|c| c.concepto.contains(t)).unwrap();
    let nominas = c("Retribuciones dinerarias");
    assert_eq!(nominas.importe, eur(24_000));
    assert_eq!(nominas.retencion, eur(4_560));
    assert_eq!(nominas.casilla, "0001");
    assert!(nominas.fuente.contains("clave E"));
    let especie = c("cuota del RETA pagada por la sociedad");
    assert_eq!(especie.importe, eur(3_600));
    // Ingreso a cuenta de la especie al 19 % (sociedad pequeña en las nóminas).
    assert_eq!(c("Cuotas del RETA del año").importe, eur(3_600));
    let div = c("Dividendos");
    assert_eq!(div.importe, eur(6_000));
    assert_eq!(div.retencion, eur(1_140));
    let presunto = c("Intereses presuntos");
    assert!(presunto.estimado);
    assert!(presunto.importe.cents() > 0);
    assert!(presunto.nota.contains("art. 6.5 LIRPF"));

    // Patrimonio: 60 % del patrimonio neto de 2024 (100.000 €) = 60.000 €.
    let p = &i.patrimonio;
    assert_eq!(p.ejercicio_balance, 2024);
    assert_eq!(p.patrimonio_neto, Some(eur(100_000)));
    assert_eq!(p.valor_teorico, Some(eur(60_000)));
    assert_eq!(p.requisitos[1].estado, Semaforo::Cumple);
    // Sin los otros rendimientos, el requisito de la remuneración queda sin datos.
    assert_eq!(p.requisitos[2].estado, Semaforo::SinDatos);
    assert_ne!(p.exencion, Semaforo::Cumple);
    // Con 10.000 € de otros rendimientos: 27.600 / 37.600 = 73,4 % > 50 %.
    let i = informe(&v, 2025, "12345678Z", Some(eur(10_000)), hoy).unwrap();
    assert_eq!(i.patrimonio.requisitos[2].estado, Semaforo::Cumple);
    // Con 40.000 € ya no llega al 50 %.
    let i = informe(&v, 2025, "12345678Z", Some(eur(40_000)), hoy).unwrap();
    assert_eq!(i.patrimonio.requisitos[2].estado, Semaforo::NoCumple);
    assert_eq!(i.patrimonio.exencion, Semaforo::NoCumple);
    assert_eq!(i.patrimonio.valor_exento_orientativo, None);

    let (nombre, bytes) = pdf(&v, 2025, "12345678Z", None, hoy).unwrap();
    assert_eq!(nombre, "Datos Renta y Patrimonio 2025 12345678Z.pdf");
    assert!(bytes.starts_with(b"%PDF"));
    // Un NIF que no es socio ni administrador.
    assert!(informe(&v, 2025, "A22222228", None, hoy).is_err());
}
