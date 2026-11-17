//! Enlaces del módulo de préstamos con el registro de operaciones vinculadas (232), el libro de
//! contratos con el socio único, la previsión de tesorería y la memoria. Datos inventados:
//! ESTUDIO DEMO SL (B12345674), su socia única y administradora Carmen Española Española
//! (12345678Z) y una participada o sociedad del grupo con NIF A22222228.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::record_entry;
use taxpain_app::forecast::{self, Category};
use taxpain_app::invoicing;
use taxpain_app::libros_societarios::contratos;
use taxpain_app::prestamos;
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::vinculadas::{self, Concepto, Participada, Vinculo};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::prestamo::{
    Contraparte, OrigenContraparte, Prestamo, Sentido, Sistema, TipoCarencia,
    Vinculo as VinculoPrestamo,
};
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime, Time};

const CARMEN: &str = "12345678Z";
const OTRA: &str = "A22222228";
const NOW: OffsetDateTime = datetime!(2026-11-16 10:00 UTC);

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
            name: "ESTUDIO DEMO SL".into(),
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
    invoicing::save_settings(&v, &s, datetime!(2025-01-01 9:00 UTC)).unwrap();
    // Socia única y administradora, con doce nóminas de 3.000 €.
    let c = v.connection();
    c.execute(
        "INSERT INTO shareholder(nif, name, province, pct, created_at)
         VALUES (?1, 'Carmen Española Española', '46', 1000000, '2025-01-01')",
        [CARMEN],
    )
    .unwrap();
    c.execute(
        "INSERT INTO employee(nif, name, province, administrator, updated_at)
         VALUES (?1, 'Carmen Española Española', '46', 1, '2025-01-01')",
        [CARMEN],
    )
    .unwrap();
    for m in 1..=12 {
        c.execute(
            "INSERT INTO payslip(employee_id, date, gross, irpf, ss_employee, ss_company,
               created_at, director_subkey)
             VALUES (1, ?1, 300000, 57000, 0, 0, '2026-01-01', 1)",
            [format!("2026-{m:02}-28")],
        )
        .unwrap();
    }
    v
}

fn asiento(v: &Vault, d: Date, debe: &str, haber: &str, e: i64) {
    record_entry(
        v.connection(),
        &JournalEntry {
            date: d,
            concept: "Movimiento".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc(debe), eur(e)),
                EntryLine::credit(acc(haber), eur(e)),
            ],
        },
        at(d),
    )
    .unwrap();
}

#[allow(clippy::too_many_arguments)]
fn prestamo(
    nombre: &str,
    sentido: Sentido,
    contraparte: Contraparte,
    inicio: Date,
    euros: i64,
    tipo_bp: i32,
    sistema: Sistema,
    periodicidad: u8,
    plazo: u16,
) -> Prestamo {
    let pr = prestamos::propuesta(sentido, &contraparte);
    Prestamo {
        id: 0,
        nombre: nombre.into(),
        sentido,
        contraparte,
        inicio,
        principal: eur(euros),
        tipo: Rate::from_bp(tipo_bp),
        sistema,
        periodicidad,
        plazo,
        carencia: 0,
        carencia_tipo: TipoCarencia::Capital,
        cuentas: pr.cuentas,
        retencion: pr.retencion,
        notas: String::new(),
    }
}

fn carmen() -> Contraparte {
    Contraparte {
        origen: OrigenContraparte::Socio,
        ref_id: Some(1),
        nif: CARMEN.into(),
        nombre: "Carmen Española Española".into(),
        provincia: "46".into(),
        persona_fisica: true,
        entidad_credito: false,
        vinculo: VinculoPrestamo::Otra,
    }
}

fn otra(vinculo: VinculoPrestamo) -> Contraparte {
    Contraparte {
        origen: OrigenContraparte::Texto,
        ref_id: None,
        nif: OTRA.into(),
        nombre: "Otra Demo SA".into(),
        provincia: "46".into(),
        persona_fisica: false,
        entidad_credito: false,
        vinculo,
    }
}

/// Préstamo de la socia: 120.000 € al 2 % con intereses mensuales (200 €) desde el 31/1/2026.
fn de_la_socia(v: &Vault) -> (i64, Prestamo) {
    let p = prestamo(
        "Préstamo de la socia",
        Sentido::Recibido,
        carmen(),
        date!(2026 - 01 - 31),
        120_000,
        200,
        Sistema::InteresesPeriodicos,
        1,
        24,
    );
    (prestamos::create(v, &p, NOW).unwrap(), p)
}

fn importe(r: &vinculadas::Registro, nif: &str, c: Concepto, ingreso: bool) -> Option<Money> {
    r.operaciones
        .iter()
        .find(|o| o.nif == nif && o.concepto == c && o.ingreso == ingreso)
        .map(|o| o.importe)
}

#[test]
fn el_prestamo_de_la_socia_va_al_registro_sin_contarse_dos_veces() {
    let v = setup();
    let (_, p) = de_la_socia(&v);
    // El gestor también lo tiene en el diario, en la subcuenta asignada a la socia.
    asiento(&v, date!(2026 - 01 - 31), "5720001", "1635001", 120_000);
    vinculadas::set_cuenta(&v, "1635001", Some(CARMEN), NOW).unwrap();
    let r = vinculadas::registro(&v, 2026).unwrap();
    // Principal recibido (clave 5, ingreso) una sola vez e intereses de once cuotas (clave 9).
    assert_eq!(
        importe(&r, CARMEN, Concepto::Financiacion, true),
        Some(eur(120_000))
    );
    assert_eq!(
        importe(&r, CARMEN, Concepto::Intereses, false),
        Some(eur(2_200))
    );
    let fin = r
        .operaciones
        .iter()
        .find(|o| o.concepto == Concepto::Financiacion)
        .unwrap();
    assert_eq!(fin.clave, Some(5));
    assert_eq!(
        fin.origen,
        vec!["préstamo «Préstamo de la socia»".to_owned()]
    );
    assert!(r.cuentas.iter().any(|c| c.account == "1635001"));
    // El aviso de tipo bajo dice lo mismo que la ficha del préstamo.
    let aviso = prestamos::aviso_tipo_vinculada(&p).unwrap();
    assert!(prestamos::avisos(&p).contains(&aviso));
    assert!(
        r.avisos
            .contains(&format!("Préstamo «Préstamo de la socia»: {aviso}")),
        "{:?}",
        r.avisos
    );
    // Un año después: sin principal dispuesto, doce cuotas de intereses.
    let r = vinculadas::registro(&v, 2027).unwrap();
    assert_eq!(importe(&r, CARMEN, Concepto::Financiacion, true), None);
    assert_eq!(
        importe(&r, CARMEN, Concepto::Intereses, false),
        Some(eur(2_400))
    );
}

#[test]
fn concedido_a_una_participada_sin_marcar_se_cuenta_y_avisa() {
    let v = setup();
    vinculadas::save_participada(
        &v,
        &Participada {
            id: 0,
            nif: OTRA.into(),
            name: "Otra Demo SA".into(),
            province: "46".into(),
            country: "ES".into(),
            pct: 500_000,
            acquired: date!(2024 - 03 - 01),
            cost: eur(150_000),
            account: "2403".into(),
            grupo_fiscal: false,
            notes: String::new(),
            active: true,
        },
        NOW,
    )
    .unwrap();
    // 50.000 € al 5 % con intereses trimestrales (625 €) desde el 31/3/2026.
    let p = prestamo(
        "Préstamo a Otra Demo",
        Sentido::Concedido,
        otra(VinculoPrestamo::Ninguno),
        date!(2026 - 03 - 31),
        50_000,
        500,
        Sistema::InteresesPeriodicos,
        3,
        12,
    );
    prestamos::create(&v, &p, NOW).unwrap();
    let r = vinculadas::registro(&v, 2026).unwrap();
    assert_eq!(
        importe(&r, OTRA, Concepto::Financiacion, false),
        Some(eur(50_000))
    );
    assert_eq!(
        importe(&r, OTRA, Concepto::Intereses, true),
        Some(eur(1_875))
    );
    assert!(
        r.avisos
            .iter()
            .any(|a| a.contains("no está marcado como vinculado")),
        "{:?}",
        r.avisos
    );
    // Al 5 %, sin aviso de tipo bajo.
    assert!(!r.avisos.iter().any(|a| a.contains("interés legal")));
}

#[test]
fn marcado_del_grupo_entra_en_la_lista_aunque_no_tenga_ficha() {
    let v = setup();
    let p = prestamo(
        "Préstamo de Otra Demo",
        Sentido::Recibido,
        otra(VinculoPrestamo::Grupo),
        date!(2026 - 06 - 30),
        300_000,
        0,
        Sistema::AlVencimiento,
        12,
        12,
    );
    prestamos::create(&v, &p, NOW).unwrap();
    let r = vinculadas::registro(&v, 2026).unwrap();
    let persona = r.personas.iter().find(|x| x.nif == OTRA).unwrap();
    assert_eq!(persona.vinculos, vec![Vinculo::Grupo]);
    // 300.000 € recibidos: más de 250.000 € con la misma entidad.
    assert!(r.obligado);
    assert!(r.avisos.iter().any(|a| a.contains("sin intereses")));
}

#[test]
fn el_prestamo_con_el_socio_unico_es_un_contrato_a_transcribir() {
    let v = setup();
    let (id, _) = de_la_socia(&v);
    let source = format!("prestamo:{id}");
    let props = contratos::proposals(&v, 2026).unwrap();
    let c = props
        .proposals
        .iter()
        .find(|c| c.source.as_deref() == Some(source.as_str()))
        .unwrap();
    assert_eq!(c.date, date!(2026 - 01 - 31));
    assert_eq!(c.amount, Some(eur(120_000)));
    assert!(
        c.object
            .contains("Préstamo de Carmen Española Española, socio único")
    );
    assert!(c.conditions.contains("24 meses"), "{}", c.conditions);
    assert!(c.conditions.contains("2 %"), "{}", c.conditions);
    assert!(
        c.conditions.contains("retención del 19 %"),
        "{}",
        c.conditions
    );
    // Transcrito, ya no se propone; en 2027 tampoco (empezó en 2026).
    contratos::save_contract(&v, c, NOW).unwrap();
    let props = contratos::proposals(&v, 2026).unwrap();
    assert!(
        props
            .proposals
            .iter()
            .all(|c| c.source.as_deref() != Some(source.as_str()))
    );
    let props = contratos::proposals(&v, 2027).unwrap();
    assert!(
        props
            .proposals
            .iter()
            .all(|c| c.source.is_none() || !c.source.as_deref().unwrap().starts_with("prestamo:"))
    );
}

#[test]
fn la_memoria_cita_los_prestamos_con_vinculadas() {
    let v = setup();
    de_la_socia(&v);
    asiento(&v, date!(2026 - 01 - 31), "5720001", "1635001", 120_000);
    let m = taxpain_app::memoria::rtf(&v, 2026).unwrap();
    assert!(m.contains("saldo vivo al cierre 120.000,00"), "saldo vivo");
    assert!(m.contains("intereses del ejercicio 2.200,00"), "intereses");
    assert!(m.contains("nota 12"), "fila de deudas con vinculadas");
}

#[test]
fn la_prevision_cuenta_las_cuotas_sin_registrar() {
    let v = setup();
    // Recibido de un banco, sin intereses para que las cuotas sean redondas: 12 × 1.000 €.
    let banco = Contraparte {
        origen: OrigenContraparte::Texto,
        ref_id: None,
        nif: String::new(),
        nombre: "Banco Inventado".into(),
        provincia: String::new(),
        persona_fisica: false,
        entidad_credito: true,
        vinculo: VinculoPrestamo::Ninguno,
    };
    let p = prestamo(
        "del banco",
        Sentido::Recibido,
        banco,
        date!(2026 - 01 - 15),
        12_000,
        0,
        Sistema::Lineal,
        1,
        12,
    );
    let id = prestamos::create(&v, &p, NOW).unwrap();
    prestamos::mark_start_outside(&v, id, true, NOW).unwrap();
    prestamos::mark_outside(&v, id, date!(2026 - 10 - 31), NOW, true).unwrap();
    // Concedido con el alta en diciembre y todo al vencimiento.
    let p = prestamo(
        "a la participada",
        Sentido::Concedido,
        otra(VinculoPrestamo::Grupo),
        date!(2026 - 12 - 01),
        5_000,
        0,
        Sistema::AlVencimiento,
        12,
        6,
    );
    prestamos::create(&v, &p, NOW).unwrap();
    let f = forecast::forecast(&v, date!(2026 - 11 - 16)).unwrap();
    let mut loans: Vec<(Date, Money)> = f
        .items
        .iter()
        .filter(|i| i.category == Category::Loans)
        .map(|i| (i.date, i.amount))
        .collect();
    loans.sort();
    assert_eq!(
        loans,
        vec![
            (date!(2026 - 12 - 01), eur(-5_000)),
            (date!(2026 - 12 - 15), eur(-1_000)),
            (date!(2027 - 01 - 15), eur(-1_000)),
            (date!(2027 - 06 - 01), eur(5_000)),
        ]
    );
    // La cuota del 15 de noviembre, sin registrar y ya pasada, no se cuenta.
    assert!(
        f.notes.iter().any(|n| n.contains("préstamos")),
        "{:?}",
        f.notes
    );
}
