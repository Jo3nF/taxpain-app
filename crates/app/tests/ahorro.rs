//! «Ahorrar impuestos» de punta a punta con una sociedad inventada: microempresa con socio único,
//! beneficio el año anterior que se reparte en parte, dividendos de una participada y una
//! factura sin cobrar. Las cifras esperadas están calculadas a mano.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::record_entry;
use taxpain_app::ahorro::{Encaje, Estado, Marca, Recomendacion, ahorro, marcar};
use taxpain_app::invoicing::{self, InvoiceDraft, issue};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

fn e(euros: i64) -> Money {
    Money::from_euros(euros, 0)
}

fn c(cents: i64) -> Money {
    Money::from_cents(cents)
}

fn asiento(v: &Vault, fecha: time::Date, origin: Origin, lineas: Vec<EntryLine>) {
    record_entry(
        v.connection(),
        &JournalEntry {
            date: fecha,
            concept: "Prueba".into(),
            origin,
            lines: lineas,
        },
        datetime!(2026-10-30 9:00 UTC),
    )
    .unwrap();
}

fn empresa() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("B12345674".into()),
            name: "EMPRESA DEMO SL".into(),
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
    let d = EntryLine::debit;
    let h = EntryLine::credit;
    // 2025: 3.000 € de capital, 120.000 € de ventas y 40.000 € de gastos → 80.000 € de beneficio.
    asiento(
        &v,
        date!(2025 - 01 - 02),
        Origin::Manual,
        vec![d(acc("5720001"), e(3_000)), h(acc("100"), e(3_000))],
    );
    asiento(
        &v,
        date!(2025 - 06 - 30),
        Origin::Manual,
        vec![d(acc("5720001"), e(120_000)), h(acc("705"), e(120_000))],
    );
    asiento(
        &v,
        date!(2025 - 07 - 31),
        Origin::Manual,
        vec![d(acc("629"), e(40_000)), h(acc("5720001"), e(40_000))],
    );
    // 2026: apertura con el beneficio pendiente de aplicar.
    asiento(
        &v,
        date!(2026 - 01 - 01),
        Origin::Apertura,
        vec![
            d(acc("5720001"), e(83_000)),
            h(acc("100"), e(3_000)),
            h(acc("129"), e(80_000)),
        ],
    );
    // Junta de junio: 600 € a la reserva legal (hasta el 20 % del capital), 30.000 € de
    // dividendos (ya pagados) y el resto a voluntarias.
    asiento(
        &v,
        date!(2026 - 06 - 30),
        Origin::Manual,
        vec![
            d(acc("129"), e(80_000)),
            h(acc("112"), e(600)),
            h(acc("113"), e(49_400)),
            h(acc("526"), e(30_000)),
        ],
    );
    asiento(
        &v,
        date!(2026 - 07 - 15),
        Origin::Manual,
        vec![d(acc("526"), e(30_000)), h(acc("5720001"), e(30_000))],
    );
    // Dividendos de una filial del grupo (7600, 10.000 €), ventas (100.000 €) y gastos (30.000 €).
    asiento(
        &v,
        date!(2026 - 05 - 10),
        Origin::Manual,
        vec![d(acc("5720001"), e(10_000)), h(acc("7600"), e(10_000))],
    );
    asiento(
        &v,
        date!(2026 - 09 - 30),
        Origin::Manual,
        vec![d(acc("5720001"), e(100_000)), h(acc("705"), e(100_000))],
    );
    asiento(
        &v,
        date!(2026 - 09 - 30),
        Origin::Manual,
        vec![d(acc("629"), e(30_000)), h(acc("5720001"), e(30_000))],
    );
    // Una factura de 1.000 € + IVA emitida en marzo y sin cobrar.
    issue(
        &v,
        &factura(1_000, date!(2026 - 03 - 10)),
        datetime!(2026-03-10 10:00 UTC),
        true,
    )
    .unwrap();
    v
}

/// La factura de 1.000 € + IVA a un cliente (o su rectificativa por diferencias si `euros` es
/// negativo).
fn factura(euros: i64, fecha: time::Date) -> InvoiceDraft {
    InvoiceDraft {
        kind: InvoiceKind::Completa,
        series: None,
        issue_date: fecha,
        operation_date: None,
        recipient: Some(Party {
            id: PartyId::Nif("B98765431".into()),
            name: "CLIENTE DEMO SL".into(),
            address: addr(),
            person: None,
        }),
        lines: vec![InvoiceLine {
            description: "Servicio".into(),
            quantity_milli: 1000,
            unit_price: e(euros),
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
    }
}

fn ficha<'a>(r: &'a [Recomendacion], id: &str) -> &'a Recomendacion {
    r.iter().find(|x| x.id == id).expect(id)
}

#[test]
fn una_microempresa_con_beneficio() {
    let v = empresa();
    let a = ahorro(&v, date!(2026 - 10 - 30)).unwrap();
    let s = &a.situacion;
    assert_eq!(s.año, 2026);
    assert!(!s.persona_fisica);
    // Cifra de negocios de 2025 (ventas de la contabilidad): microempresa.
    assert_eq!(s.cifra_negocios.as_ref().unwrap().valor, e(120_000));
    assert!(s.regimen.as_deref().unwrap().starts_with("Microempresa"));
    // Base de 2026: 111.000 € de ingresos (ventas, factura y dividendos) − 30.000 € de gastos.
    assert_eq!(s.base.as_ref().unwrap().valor, e(81_000));
    // Cuota: 19 % de 50.000 € + 21 % de 31.000 €.
    assert_eq!(s.cuota.as_ref().unwrap().valor, e(16_010));
    assert_eq!(s.tipo, Some(21));
    let r = &a.recomendaciones;

    // Capitalización: 80.000 € − 30.000 € de dividendos − 600 € de reserva legal = 49.400 €;
    // 20 % = 9.880 € de rebaja al 21 %.
    let cap = ficha(r, "capitalizacion");
    assert_eq!(cap.encaje, Encaje::Conviene, "{}", cap.motivo);
    assert_eq!(cap.ahorro, Some(c(207_480)));
    assert!(cap.motivo.contains("49.400,00 €"), "{}", cap.motivo);

    // Dividendos de la participada sin exención: 95 % de 10.000 € al 21 %.
    let part = ficha(r, "participadas");
    assert_eq!(part.encaje, Encaje::Conviene);
    assert_eq!(part.ahorro, Some(e(1_995)));

    // Financiador de cine, con la cuota que queda tras las rebajas que convienen (capitalización
    // 9.880 €, exención 9.500 € y deterioro 1.000 €: base 60.620 €, cuota 11.730,20 €): 50 % =
    // 5.865,10 € de deducción por 4.887,58 € aportados; ganancia 977,52 €, menos de 1.000 €:
    // puede interesar, no «te conviene».
    let cine = ficha(r, "audiovisual");
    assert_eq!(cine.encaje, Encaje::Valorar, "{}", cine.motivo);
    assert_eq!(cine.ahorro, Some(c(97_752)));

    // Nivelación (microempresa: sin la duda de las de reducida dimensión), 10 % de la base tras
    // las demás rebajas que convienen (81.000 € − 9.880 € − 9.500 € − 1.000 € = 60.620 €) al
    // 21 %: impuesto aplazado.
    let niv = ficha(r, "nivelacion");
    assert_eq!(niv.encaje, Encaje::Conviene);
    assert_eq!(niv.ahorro, Some(c(127_302)));
    assert!(niv.aplazado);

    // IVA de la factura sin cobrar (210 €), hasta 18 meses desde la factura.
    let iva = ficha(r, "iva_impagadas");
    assert_eq!(iva.encaje, Encaje::Conviene, "{}", iva.motivo);
    assert_eq!(iva.ahorro, Some(e(210)));
    assert_eq!(iva.plazo, Some(date!(2027 - 09 - 10)));

    // Deterioro: 1.210 € − 210 € de IVA = 1.000 € de gasto al 21 %; el 1 % global no repite
    // la factura que ya se deteriora.
    let det = ficha(r, "deterioro_creditos");
    assert_eq!(det.encaje, Encaje::Conviene, "{}", det.motivo);
    assert_eq!(det.ahorro, Some(e(210)));

    // Sin trabajadores, el plan de empleo no se ofrece; las fichas de autónomo no salen.
    assert_eq!(ficha(r, "plan_pensiones_empleo").encaje, Encaje::NoAplica);
    assert!(r.iter().all(|x| x.id != "plan_pensiones_autonomo"));

    // Los ajustes tienen el tipo general (no se ha elegido régimen): el borrador calcula unos
    // 20.250 € − 16.010 € = 4.240 € de más. Es un aviso, no un ahorro (el tipo es por ley).
    let tipo = ficha(r, "tipo_reducido");
    assert_eq!(tipo.encaje, Encaje::Valorar);
    assert_eq!(tipo.ahorro, None);
    assert!(tipo.motivo.contains("4.240,00 €"), "{}", tipo.motivo);

    // Total: lo que conviene y no es aplazado.
    let total = c(207_480) + e(1_995) + e(210) + e(210);
    assert_eq!(a.ahorro_posible, total);
    assert_eq!(a.aplazado_posible, c(127_302));
    // Lo que conviene sale primero, de más a menos ahorro.
    assert_eq!(r[0].id, "capitalizacion");
    assert_eq!(r[1].id, "participadas");

    // Marcar una como hecha la saca del total.
    marcar(
        &v,
        date!(2026 - 10 - 30),
        "participadas",
        &Marca {
            estado: Estado::Hecho,
            pasos: [0, 1].into_iter().collect(),
        },
    )
    .unwrap();
    let b = ahorro(&v, date!(2026 - 10 - 30)).unwrap();
    assert_eq!(b.ahorro_posible, total - e(1_995));
    assert_eq!(
        ficha(&b.recomendaciones, "participadas").marca.estado,
        Estado::Hecho
    );
}

#[test]
fn sin_datos_no_falla() {
    // Empresa sin contabilidad ni declaraciones: todo «puede interesarte» o «no aplica», sin
    // cifras, y con la nota de qué falta.
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let a = ahorro(&v, date!(2026 - 10 - 30)).unwrap();
    assert_eq!(a.ahorro_posible, Money::ZERO);
    assert!(a.situacion.base.is_none());
    assert!(!a.situacion.notas.is_empty());
    assert!(
        a.recomendaciones
            .iter()
            .all(|r| r.encaje != Encaje::Conviene || r.ahorro.is_none())
    );
}

#[test]
fn con_la_rectificativa_r3_ya_no_hay_iva_que_recuperar() {
    let v = empresa();
    let original: i64 = v
        .connection()
        .query_row("SELECT id FROM invoice_issued WHERE kind = 'F1'", [], |r| {
            r.get(0)
        })
        .unwrap();
    let antes = ahorro(&v, date!(2026 - 10 - 30)).unwrap();
    assert_eq!(
        ficha(&antes.recomendaciones, "iva_impagadas").encaje,
        Encaje::Conviene
    );
    // Rectificativa por créditos incobrables (R3) con su procedimiento (art. 80.Cuatro LIVA):
    // seguimiento, reclamación por burofax y R3 de lo no cobrado (1.000 € + 210 € de IVA).
    let now = datetime!(2026-10-20 10:00 UTC);
    let id = taxpain_app::incobrables::abrir(
        &v,
        original,
        None,
        taxpain_app::incobrables::Exclusion::Ninguna,
        now,
        true,
    )
    .unwrap();
    taxpain_app::incobrables::reclamar(
        &v,
        id,
        date!(2026 - 10 - 01),
        taxpain_app::incobrables::Medio::Fehaciente,
        "",
        now,
        true,
    )
    .unwrap();
    taxpain_app::incobrables::emitir_r3(&v, id, date!(2026 - 10 - 20), now, true).unwrap();
    let despues = ahorro(&v, date!(2026 - 10 - 30)).unwrap();
    // El IVA de esa factura ya se ha modificado: no se vuelve a recomendar.
    assert_ne!(
        ficha(&despues.recomendaciones, "iva_impagadas").encaje,
        Encaje::Conviene
    );
    // Lo que sigue pendiente (1.210 € − 210 € de la cuota recuperada) es la base: gasto por
    // deterioro, sin restarle IVA.
    let det = ficha(&despues.recomendaciones, "deterioro_creditos");
    assert_eq!(det.encaje, Encaje::Conviene, "{}", det.motivo);
    assert!(det.motivo.contains("1.000,00 €"), "{}", det.motivo);
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
