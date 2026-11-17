//! Operaciones vinculadas con datos inventados: ESTUDIO DEMO SL, su socia única y administradora
//! Carmen Española Española (12345678Z) y Participada Demo SL (B00000000, NIF de prueba).

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::record_entry;
use taxpain_app::invoicing::{self, InvoiceDraft, issue};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::vinculadas::{
    self, Concepto, Metodo, OperacionManual, Participada, Textos, Valoracion, Vinculada, Vinculo,
    documentacion, hoja,
};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime, Time};

const CARMEN: &str = "12345678Z";
const PARTICIPADA: &str = "B00000000";
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

fn emit(v: &Vault, nif: &str, name: &str, euros: i64, d: Date) {
    let draft = InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: d,
        operation_date: None,
        recipient: Some(Party {
            id: PartyId::Nif(nif.into()),
            name: name.into(),
            address: addr(),
            person: None,
        }),
        lines: vec![InvoiceLine {
            description: "Servicios de administración".into(),
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
    issue(v, &draft, at(d), true).unwrap();
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

fn participada() -> Participada {
    Participada {
        id: 0,
        nif: PARTICIPADA.into(),
        name: "Participada Demo SL".into(),
        province: "46".into(),
        country: "ES".into(),
        pct: 500_000,
        acquired: date!(2024 - 03 - 01),
        cost: eur(150_000),
        account: "2403".into(),
        grupo_fiscal: false,
        notes: String::new(),
        active: true,
    }
}

#[test]
fn socia_administradora_sin_operaciones_que_cuenten_no_llega_al_232() {
    let v = setup();
    let r = vinculadas::registro(&v, 2026).unwrap();
    assert_eq!(r.personas.len(), 1);
    let carmen = &r.personas[0];
    assert_eq!(carmen.nif, CARMEN);
    assert_eq!(
        carmen.vinculos,
        vec![Vinculo::Socio, Vinculo::Administrador]
    );
    // La retribución por el cargo se ve, pero no cuenta (art. 18.2.b LIS).
    let ret = r
        .operaciones
        .iter()
        .find(|o| o.concepto == Concepto::RetribucionAdministrador)
        .unwrap();
    assert_eq!(ret.importe, eur(36_000));
    assert!(ret.excluida.is_some());
    assert!(!r.obligado);
    assert_eq!(carmen.total, Money::ZERO);
    assert_eq!(r.plazo, "noviembre de 2027");
    assert!(r.avisos.iter().any(|a| a.contains("18.2.b")));
}

#[test]
fn participada_con_factura_y_prestamo_pasa_de_250_000() {
    let v = setup();
    // Con el 20 % no es vinculada.
    let mut p = participada();
    p.pct = 200_000;
    let id = vinculadas::save_participada(&v, &p, NOW).unwrap();
    emit(
        &v,
        PARTICIPADA,
        "Participada Demo SL",
        120_000,
        date!(2026 - 03 - 10),
    );
    asiento(&v, date!(2026 - 04 - 01), "5320001", "5720001", 300_000);
    asiento(&v, date!(2026 - 12 - 31), "5330001", "7620001", 9_000);
    let r = vinculadas::registro(&v, 2026).unwrap();
    assert!(r.personas.iter().all(|x| x.nif != PARTICIPADA));
    // Subcuentas candidatas sin asignar.
    assert!(
        r.cuentas
            .iter()
            .any(|c| c.account == "5320001" && c.nif.is_none())
    );
    assert!(r.cuentas.iter().any(|c| c.account == "7620001"));
    // Con el 50 %, sí; las subcuentas se asignan.
    p.id = id;
    p.pct = 500_000;
    vinculadas::save_participada(&v, &p, NOW).unwrap();
    for a in ["5320001", "7620001"] {
        vinculadas::set_cuenta(&v, a, Some(PARTICIPADA), NOW).unwrap();
    }
    let r = vinculadas::registro(&v, 2026).unwrap();
    let ops: Vec<_> = r
        .operaciones
        .iter()
        .filter(|o| o.nif == PARTICIPADA)
        .collect();
    let get = |c: Concepto, ingreso: bool| {
        ops.iter()
            .find(|o| o.concepto == c && o.ingreso == ingreso)
            .map(|o| o.importe)
    };
    assert_eq!(get(Concepto::Servicios, true), Some(eur(120_000)));
    assert_eq!(get(Concepto::Financiacion, false), Some(eur(300_000)));
    assert_eq!(get(Concepto::Intereses, true), Some(eur(9_000)));
    assert!(ops.iter().all(|o| o.en_232));
    assert!(r.obligado);
    assert!(
        r.motivos[0].contains("Participada Demo SL"),
        "{:?}",
        r.motivos
    );
    assert_eq!(r.documentar, vec!["Participada Demo SL".to_owned()]);
    // La regla del 50 %: la cifra de negocios son las facturas (120.000 €).
    assert_eq!(r.incn, Some(eur(120_000)));
    assert!(
        r.motivos.iter().any(|m| m.contains("50 %")),
        "{:?}",
        r.motivos
    );
    // En el grupo fiscal, nada va al 232.
    p.grupo_fiscal = true;
    vinculadas::save_participada(&v, &p, NOW).unwrap();
    let r = vinculadas::registro(&v, 2026).unwrap();
    assert!(!r.obligado);
    assert!(r.operaciones.iter().all(|o| o.excluida.is_some()));
}

#[test]
fn familiar_tecleado_con_venta_de_inmueble_especifica() {
    let v = setup();
    let hermano = Vinculada {
        id: 0,
        nif: "00000000T".into(),
        name: "Hermano Demo".into(),
        legal_person: false,
        province: "46".into(),
        country: "ES".into(),
        vinculo: Vinculo::Familiar,
        detalle: "hermano de la socia".into(),
        pct: None,
        grupo_fiscal: false,
        clave_emitidas: 6,
        clave_recibidas: 6,
        active: true,
    };
    // Un NIF de sociedad marcado como persona física no se acepta.
    let mal = Vinculada {
        nif: PARTICIPADA.into(),
        ..hermano.clone()
    };
    assert!(vinculadas::save_vinculada(&v, &mal, NOW).is_err());
    let id = vinculadas::save_vinculada(&v, &hermano, NOW).unwrap();
    assert_eq!(
        vinculadas::vinculada_de(&v, "00000000-t")
            .unwrap()
            .unwrap()
            .id,
        id
    );
    // Sólo con vinculadas.
    let mut op = OperacionManual {
        id: 0,
        year: 2026,
        nif: "B98765431".into(),
        clave: 1,
        ingreso: true,
        amount: eur(110_000),
        especifica: true,
        description: "Venta de un local".into(),
    };
    assert!(vinculadas::save_operacion(&v, &op, NOW).is_err());
    op.nif = "00000000T".into();
    let op_id = vinculadas::save_operacion(&v, &op, NOW).unwrap();
    let r = vinculadas::registro(&v, 2026).unwrap();
    let o = r.operaciones.iter().find(|o| o.nif == "00000000T").unwrap();
    assert!(o.especifica && o.en_232);
    assert!(r.motivos.iter().any(|m| m.contains("específicas")));
    // El método y los textos se guardan y salen en la documentación.
    vinculadas::save_valoracion(
        &v,
        &Valoracion {
            year: 2026,
            nif: "00000000T".into(),
            concepto: Concepto::BienesTangibles,
            ingreso: true,
            metodo: Metodo::PrecioLibreComparable,
            descripcion: "Local de 80 m² en la calle Inventada".into(),
            justificacion: "Tasación de una sociedad de tasación: 110.000 €".into(),
        },
        NOW,
    )
    .unwrap();
    vinculadas::save_textos(
        &v,
        &Textos {
            year: 2026,
            actividad: "Estudio de diseño gráfico".into(),
            justificacion: "Precios de mercado".into(),
        },
        NOW,
    )
    .unwrap();
    let rtf = documentacion::rtf(&v, 2026).unwrap();
    assert!(rtf.starts_with("{\\rtf1"));
    assert!(rtf.contains("Hermano Demo"));
    assert!(rtf.contains("Tasaci"));
    assert!(rtf.contains("operaciones espec"), "aviso de específicas");
    let pdf = documentacion::pdf(&v, 2026).unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    let (name, xlsx) = hoja::workbook(&v, 2026).unwrap();
    assert_eq!(name, "Modelo 232 2026.xlsx");
    assert!(xlsx.starts_with(b"PK"));
    // Quitar la operación y la vinculada deja rastro en la auditoría.
    vinculadas::delete_operacion(&v, op_id, NOW).unwrap();
    vinculadas::delete_vinculada(&v, id, NOW).unwrap();
    let n: i64 = v
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM audit_log WHERE action LIKE 'vinculada%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(n >= 6, "{n}");
    assert!(!vinculadas::registro(&v, 2026).unwrap().obligado);
}

#[test]
fn la_nota_de_la_memoria_sale_del_registro() {
    let v = setup();
    let p = taxpain_app::memoria::rtf(&v, 2026).unwrap();
    assert!(p.contains("36.000,00"), "retribución del administrador");
    assert!(!p.contains("[importe, o"));
}

#[test]
fn una_persona_fisica_no_tiene_registro() {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    v.set_setting(
        taxpain_app::companies::COMPANY_KEY,
        &serde_json::json!({ "name": "Carmen", "nif": CARMEN }),
    )
    .unwrap();
    assert!(vinculadas::registro(&v, 2026).is_err());
}
