//! Memoria por modelo (PYMES, abreviado, normal), ECPN y aviso del modelo de cuentas. Datos
//! inventados (Estudio Demo SL, B12345674).

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::{close_year, manual_entry};
use taxpain_app::ecpn::{self, Fila};
use taxpain_app::memoria::{self, TextosEjercicio, TextosMemoria};
use taxpain_app::modelo_cuentas;
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::statements::ModelKind;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{Address, Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_invoicing::pdf::Para;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn entry(d: time::Date, lines: &[(&str, i64, i64)]) -> JournalEntry {
    JournalEntry {
        date: d,
        concept: "x".into(),
        origin: Origin::Manual,
        lines: lines
            .iter()
            .map(|(a, debe, haber)| {
                if *debe > 0 {
                    EntryLine::debit(acc(a), eur(*debe))
                } else {
                    EntryLine::credit(acc(a), eur(*haber))
                }
            })
            .collect(),
    }
}

/// 2025: capital 3.000 y beneficio 1.500, cerrado. 2026: ordenador, amortización, ventas,
/// reparto de 500 € de dividendo con cargo a reservas.
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
            address: Some(Address {
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
    let now = datetime!(2026-05-10 10:00 UTC);
    taxpain_app::invoicing::save_settings(&v, &s, now).unwrap();
    for e in [
        entry(
            date!(2025 - 01 - 02),
            &[("5720001", 3000, 0), ("100", 0, 3000)],
        ),
        entry(
            date!(2025 - 06 - 10),
            &[("5720001", 2000, 0), ("705", 0, 2000)],
        ),
        entry(
            date!(2025 - 06 - 11),
            &[("629", 500, 0), ("5720001", 0, 500)],
        ),
    ] {
        manual_entry(&v, &e, now).unwrap();
    }
    close_year(&v, 2025, now, true).unwrap();
    for e in [
        entry(
            date!(2026 - 02 - 01),
            &[("217", 1200, 0), ("5720001", 0, 1200)],
        ),
        entry(date!(2026 - 12 - 31), &[("681", 300, 0), ("2817", 0, 300)]),
        entry(
            date!(2026 - 03 - 01),
            &[("5720001", 4000, 0), ("705", 0, 4000)],
        ),
        // Resultado de 2025 a reservas voluntarias y, luego, dividendo con cargo a ellas.
        entry(date!(2026 - 04 - 01), &[("129", 1500, 0), ("113", 0, 1500)]),
        entry(date!(2026 - 04 - 02), &[("113", 500, 0), ("526", 0, 500)]),
    ] {
        manual_entry(&v, &e, now).unwrap();
    }
    v
}

fn bolds(p: &[Para]) -> Vec<String> {
    p.iter()
        .filter_map(|x| match x {
            Para::Bold(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

fn has_row(p: &[Para], label: &str, value: &str) -> bool {
    p.iter()
        .any(|x| matches!(x, Para::Row(l, r) if l.contains(label) && r == value))
}

#[test]
fn cada_modelo_con_sus_notas() {
    let v = setup();
    let pymes = memoria::paragraphs_modelo(&v, 2026, ModelKind::Pymes).unwrap();
    let b = bolds(&pymes);
    assert_eq!(b.len(), 14, "{b:?}");
    assert_eq!(b[11], "12. Operaciones con partes vinculadas");

    let abr = memoria::paragraphs_modelo(&v, 2026, ModelKind::Abreviado).unwrap();
    assert!(matches!(&abr[0], Para::Title(t) if t.contains("Memoria abreviada")));
    let b = bolds(&abr);
    assert_eq!(b[11], "12. Hechos posteriores al cierre");
    assert_eq!(b[12], "13. Operaciones con partes vinculadas");
    assert!(abr.iter().any(
        |x| matches!(x, Para::Text(t) if t.contains("estado de cambios en el patrimonio neto y esta memoria, todos abreviados"))
    ));

    let normal = memoria::paragraphs_modelo(&v, 2026, ModelKind::Normal).unwrap();
    let b = bolds(&normal);
    assert!(b.contains(&"5. Inmovilizado material".to_owned()), "{b:?}");
    assert!(b.contains(&"9. Instrumentos financieros".to_owned()));
    assert!(b.iter().any(|t| t.ends_with("Información segmentada")));
    // El modelo de los ajustes manda en `paragraphs`.
    modelo_cuentas::elegir(&v, ModelKind::Abreviado, datetime!(2026-05-10 10:00 UTC)).unwrap();
    assert_eq!(memoria::paragraphs(&v, 2026).unwrap(), abr);
    assert!(
        memoria::rtf_modelo(&v, 2026, ModelKind::Normal)
            .unwrap()
            .starts_with("{\\rtf1")
    );
    assert!(
        memoria::pdf_modelo(&v, 2026, ModelKind::Abreviado)
            .unwrap()
            .starts_with(b"%PDF")
    );
}

#[test]
fn las_notas_salen_de_la_contabilidad() {
    let v = setup();
    let p = memoria::paragraphs_modelo(&v, 2026, ModelKind::Abreviado).unwrap();
    // Inmovilizado: entrada, dotación y valor neto.
    assert!(has_row(&p, "Entradas", "1.200,00 €"), "{p:?}");
    assert!(has_row(&p, "Dotación del ejercicio", "300,00 €"));
    assert!(has_row(&p, "Valor neto contable al cierre", "900,00 €"));
    // Fondos propios y sus movimientos.
    assert!(has_row(&p, "Capital", "3.000,00 €"));
    assert!(has_row(&p, "Otras reservas", "1.000,00 €"));
    assert!(has_row(&p, "Reservas al cierre", "1.000,00 €"));
    assert!(has_row(&p, "Resultado del ejercicio", "3.700,00 €"));
    // Pasivo: el dividendo pendiente de pago.
    assert!(has_row(&p, "Otras deudas", "500,00 €"));
    // Cifra de negocios y corchetes sólo en lo que no se sabe.
    assert!(has_row(
        &p,
        "Importe neto de la cifra de negocios",
        "4.000,00 €"
    ));
    let rtf = memoria::to_rtf(&p);
    assert!(rtf.contains("Honorarios de auditor"));
}

#[test]
fn textos_tecleados_en_la_memoria() {
    let v = setup();
    let t = TextosMemoria {
        nota_fija: "La sociedad se acogió en 2024 al régimen del canje de valores (art. 86 LIS)."
            .into(),
        ejercicio: TextosEjercicio {
            hechos_posteriores: "  No se han producido.  ".into(),
            honorarios_auditoria: Some(Money::ZERO),
            formulacion: Some(date!(2027 - 03 - 25)),
            firmantes: vec!["Carmen Española Española".into(), "  ".into()],
        },
    };
    memoria::guardar_textos(&v, 2026, &t, datetime!(2027-03-25 9:00 UTC)).unwrap();
    let back = memoria::textos(&v, 2026).unwrap();
    assert_eq!(back.ejercicio.hechos_posteriores, "No se han producido.");
    assert_eq!(back.ejercicio.firmantes.len(), 1);
    // Otro ejercicio: la nota fija sí, los del año no.
    let otro = memoria::textos(&v, 2027).unwrap();
    assert!(otro.nota_fija.contains("art. 86"));
    assert!(otro.ejercicio.firmantes.is_empty());

    let rtf = memoria::rtf_modelo(&v, 2026, ModelKind::Abreviado).unwrap();
    assert!(rtf.contains("art. 86 LIS"));
    assert!(rtf.contains("no se someten a auditor"));
    assert!(rtf.contains("25/03/2027"));
    assert!(rtf.contains("Fdo.: Carmen Espa"));
    assert!(!rtf.contains("[no se han producido hechos"));
    let neg = TextosMemoria {
        ejercicio: TextosEjercicio {
            honorarios_auditoria: Some(-eur(1)),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(memoria::guardar_textos(&v, 2026, &neg, datetime!(2027-03-25 9:00 UTC)).is_err());
}

#[test]
fn ecpn_cuadra_con_el_balance() {
    let v = setup();
    let e = ecpn::ecpn(&v, 2026, ModelKind::Abreviado).unwrap();
    assert!(e.obligatorio);
    let col = |n: &str| e.columnas.iter().position(|c| c == n).unwrap();
    let ini = e.fila(Fila::SaldoInicial).unwrap();
    let fin = e.fila(Fila::SaldoFinal).unwrap();
    assert_eq!(ini.importes[col("Capital")], eur(3000));
    assert_eq!(ini.importes[col("Resultado del ejercicio")], eur(1500));
    assert_eq!(ini.total, eur(4500));
    // Ingresos y gastos reconocidos: el resultado de 2026 (4.000 − 300).
    let ig = e.fila(Fila::IngresosGastos).unwrap();
    assert_eq!(ig.importes[col("Resultado del ejercicio")], eur(3700));
    assert_eq!(e.ingresos_gastos.last().unwrap().amount, eur(3700));
    // El dividendo con cargo a reservas.
    assert_eq!(e.fila(Fila::Dividendos).unwrap().total, -eur(500));
    // Final: capital 3.000, reservas 1.000, resultado 3.700 = 7.700, el patrimonio del balance.
    assert_eq!(fin.importes[col("Reservas")], eur(1000));
    assert_eq!(fin.total, eur(7700));
    let ca =
        taxpain_app::accounting::annual_accounts_model(&v, 2026, ModelKind::Abreviado).unwrap();
    assert_eq!(ca.balance.get("PA"), fin.total);
    // En PYMES, voluntario; PDF y RTF.
    assert!(!ecpn::ecpn(&v, 2026, ModelKind::Pymes).unwrap().obligatorio);
    assert!(
        ecpn::pdf(&v, 2026, ModelKind::Normal)
            .unwrap()
            .starts_with(b"%PDF")
    );
    let r = ecpn::rtf(&v, 2026, ModelKind::Abreviado).unwrap();
    assert!(r.contains("Estado abreviado total de cambios"));
}

#[test]
fn aviso_del_modelo_por_tamano() {
    let v = setup();
    let a = modelo_cuentas::aviso(&v, 2026).unwrap();
    assert_eq!(a.ejercicios.len(), 2, "2026 y 2025");
    assert_eq!(a.ejercicios[0].cifra_negocios, eur(4000));
    assert_eq!(a.puede_abreviado, Some(true));
    assert_eq!(a.puede_micro, Some(true));
    assert_eq!(a.recomendado, ModelKind::Pymes);
    assert!(a.aviso.is_none());
    modelo_cuentas::elegir(&v, ModelKind::Normal, datetime!(2026-05-10 10:00 UTC)).unwrap();
    let a = modelo_cuentas::aviso(&v, 2026).unwrap();
    assert_eq!(a.elegido, ModelKind::Normal);
    assert!(a.aviso.unwrap().contains("puedes usar"));
    // Sin el año anterior: primer ejercicio.
    let a = modelo_cuentas::aviso(&v, 2025).unwrap();
    assert_eq!(a.ejercicios.len(), 1);
    assert!(a.explicacion[0].contains("primer ejercicio"));
}
