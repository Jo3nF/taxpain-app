//! Subvenciones (G41): concesión con efecto impositivo, cobro, imputación al ritmo de la
//! amortización del bien, deshacer, reintegrables, ECPN y memoria. Cifras inventadas.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::{add_fixed_asset, journal};
use taxpain_app::invoicing;
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::subvenciones::{
    Accion, Subvencion, Tipo, borrar, deshacer, guardar, memoria, propuesta_asiento, registrar,
    resumen,
};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::depreciation::FixedAsset;
use taxpain_core::accounting::entry::Origin;
use taxpain_core::accounting::statements::ModelKind;
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
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

fn subvencion(tipo: Tipo, importe: i64) -> Subvencion {
    Subvencion {
        id: 0,
        concedente: "Ayuntamiento de Ejemplo".into(),
        concedente_nif: String::new(),
        fecha_concesion: date!(2025 - 01 - 15),
        importe: eur(importe),
        finalidad: "comprar la maquinaria".into(),
        tipo,
        condiciones: "mantener la inversión cinco años".into(),
        reintegrable: false,
        fixed_asset_id: None,
        efecto_impositivo: true,
        tipo_impositivo: Rate::percent(25),
        minimis: true,
        cuenta_deudora: "4708".into(),
        notas: String::new(),
    }
}

fn lineas(v: &Vault, id: i64) -> Vec<(String, Money, Money)> {
    let mut out: Vec<(String, Money, Money)> = journal(v, 2025)
        .unwrap()
        .into_iter()
        .filter(|e| e.entry.origin == Origin::Subvencion(id))
        .flat_map(|e| e.entry.lines)
        .map(|l| (l.account.as_str().to_owned(), l.debit, l.credit))
        .collect();
    out.sort();
    out
}

#[test]
fn subvencion_de_capital_con_efecto_impositivo() {
    let v = setup();
    let now = datetime!(2025-12-31 10:00 UTC);
    // Máquina de 10.000 € al 20 % lineal desde el 1 de enero.
    let bien = add_fixed_asset(
        &v,
        &FixedAsset {
            id: 0,
            name: "Máquina demo".into(),
            account: acc("213"),
            depreciation_account: acc("2813"),
            expense_account: acc("681"),
            acquired_on: date!(2025 - 01 - 01),
            cost: eur(10_000),
            residual: Money::ZERO,
            rate: Rate::percent(20),
            disposed_on: None,
            method: Default::default(),
            tax_rate: None,
        },
        now,
    )
    .unwrap();
    let id = guardar(
        &v,
        &Subvencion {
            fixed_asset_id: Some(bien),
            ..subvencion(Tipo::Capital, 6_000)
        },
        now,
    )
    .unwrap();

    // Sin confirmar no se graba nada.
    assert!(registrar(&v, id, &Accion::Concesion, now, false).is_err());
    // Concesión: 4708 6.000 a 130 4.500 y 479 1.500 (25 %).
    let (e, _) = propuesta_asiento(&v, id, &Accion::Concesion).unwrap();
    assert_eq!(e.date, date!(2025 - 01 - 15));
    registrar(&v, id, &Accion::Concesion, now, true).unwrap();
    assert!(registrar(&v, id, &Accion::Concesion, now, true).is_err());
    // Cobro de 6.000 € en el banco; no se puede cobrar más.
    let cobro = Accion::Cobro {
        fecha: date!(2025 - 03 - 01),
        importe: eur(6_000),
        tesoreria: "5720001".into(),
    };
    registrar(&v, id, &cobro, now, true).unwrap();
    assert!(registrar(&v, id, &cobro, now, true).is_err());

    // Imputación de 2025: amortización del 20 % → 1.200 € a la 746 (900 de la 130 y 300 de la
    // 479).
    let r = resumen(&v, 2025).unwrap();
    assert_eq!(r.subvenciones[0].propuesta_imputacion, Some(eur(1_200)));
    let imp = Accion::Imputacion {
        year: 2025,
        importe: None,
    };
    registrar(&v, id, &imp, now, true).unwrap();
    assert_eq!(
        lineas(&v, id),
        vec![
            ("130".into(), Money::ZERO, eur(4_500)),
            ("130".into(), eur(900), Money::ZERO),
            ("4708".into(), Money::ZERO, eur(6_000)),
            ("4708".into(), eur(6_000), Money::ZERO),
            ("479".into(), Money::ZERO, eur(1_500)),
            ("479".into(), eur(300), Money::ZERO),
            ("5720001".into(), eur(6_000), Money::ZERO),
            ("746".into(), Money::ZERO, eur(1_200)),
        ]
    );
    let r = resumen(&v, 2025).unwrap();
    let x = &r.subvenciones[0];
    assert_eq!(x.imputado_ejercicio, eur(1_200));
    assert_eq!(x.pendiente_imputar, eur(4_800));
    assert_eq!(x.saldo_pn, eur(3_600));
    assert_eq!(x.pasivo_diferido, eur(1_200));
    assert_eq!(x.pendiente_cobro, Money::ZERO);
    assert_eq!(x.propuesta_imputacion, None);
    assert_eq!(r.minimis_tres_años, eur(6_000));
    // El año siguiente propone otros 1.200 €.
    let r26 = resumen(&v, 2026).unwrap();
    assert_eq!(r26.subvenciones[0].propuesta_imputacion, Some(eur(1_200)));

    // ECPN: 4.500 imputados al patrimonio neto (efecto −1.500) y −900 transferidos (efecto 300).
    let ecpn = taxpain_app::ecpn::ecpn(&v, 2025, ModelKind::Pymes).unwrap();
    let a: Vec<Money> = ecpn.ingresos_gastos.iter().map(|l| l.amount).collect();
    assert_eq!(a[1], eur(4_500));
    assert_eq!(a[2], eur(-1_500));
    assert_eq!(a[3], eur(-900));
    assert_eq!(a[4], eur(300));
    let col = ecpn
        .columnas
        .iter()
        .position(|c| c.starts_with("Subvenciones"))
        .unwrap();
    let fin = ecpn
        .fila(taxpain_app::ecpn::Fila::SaldoFinal)
        .unwrap()
        .importes[col];
    assert_eq!(fin, eur(3_600));

    // Memoria: el registro con concedente, finalidad y condiciones.
    let m = memoria(&v, 2025).unwrap();
    assert!(m[0].contains("Ayuntamiento de Ejemplo"));
    assert!(m[0].contains("comprar la maquinaria"));
    assert!(m[0].contains("Ayuda de minimis"));
    let paras = taxpain_app::memoria::paragraphs_modelo(&v, 2025, ModelKind::Pymes).unwrap();
    let texto: String = paras
        .iter()
        .map(|p| format!("{p:?}"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(texto.contains("Ayuntamiento de Ejemplo"));

    // Con asientos no se cambian las cifras ni se borra.
    assert!(
        guardar(
            &v,
            &Subvencion {
                id,
                fixed_asset_id: Some(bien),
                ..subvencion(Tipo::Capital, 7_000)
            },
            now
        )
        .is_err()
    );
    assert!(borrar(&v, id, now, true).is_err());
    // Deshacer quita el último asiento (la imputación) y vuelve a proponerla.
    deshacer(&v, id, now, true).unwrap();
    let r = resumen(&v, 2025).unwrap();
    assert_eq!(r.subvenciones[0].propuesta_imputacion, Some(eur(1_200)));
    assert!(!lineas(&v, id).iter().any(|l| l.0 == "746"));
}

#[test]
fn reintegrable_de_explotacion_y_sin_efecto_impositivo() {
    let v = setup();
    let now = datetime!(2025-12-31 10:00 UTC);
    let id = guardar(
        &v,
        &Subvencion {
            reintegrable: true,
            minimis: false,
            ..subvencion(Tipo::Explotacion, 1_000)
        },
        now,
    )
    .unwrap();
    // Reintegrable: es una deuda (522), y no se imputa.
    registrar(&v, id, &Accion::Concesion, now, true).unwrap();
    assert_eq!(
        lineas(&v, id),
        vec![
            ("4708".into(), eur(1_000), Money::ZERO),
            ("522".into(), Money::ZERO, eur(1_000)),
        ]
    );
    let r = resumen(&v, 2025).unwrap();
    assert!(r.subvenciones[0].en_deuda);
    assert!(
        r.subvenciones[0]
            .avisos
            .iter()
            .any(|a| a.contains("reintegrable"))
    );
    assert!(
        registrar(
            &v,
            id,
            &Accion::Imputacion {
                year: 2025,
                importe: Some(eur(100))
            },
            now,
            true
        )
        .is_err()
    );
    // Se cumplen las condiciones: de la 522 a la 740 (sin efecto impositivo en resultados).
    registrar(
        &v,
        id,
        &Accion::NoReintegrable {
            fecha: date!(2025 - 06 - 30),
        },
        now,
        true,
    )
    .unwrap();
    let l = lineas(&v, id);
    assert!(l.contains(&("522".into(), eur(1_000), Money::ZERO)));
    assert!(l.contains(&("740".into(), Money::ZERO, eur(1_000))));
    assert!(!l.iter().any(|x| x.0 == "479"));
    assert!(!resumen(&v, 2025).unwrap().subvenciones[0].en_deuda);

    // Una de gastos sin bien: la imputación se teclea.
    let g = guardar(
        &v,
        &Subvencion {
            efecto_impositivo: false,
            ..subvencion(Tipo::Gastos, 2_000)
        },
        now,
    )
    .unwrap();
    registrar(&v, g, &Accion::Concesion, now, true).unwrap();
    assert!(
        registrar(
            &v,
            g,
            &Accion::Imputacion {
                year: 2025,
                importe: None
            },
            now,
            true
        )
        .is_err()
    );
    registrar(
        &v,
        g,
        &Accion::Imputacion {
            year: 2025,
            importe: Some(eur(500)),
        },
        now,
        true,
    )
    .unwrap();
    let l = lineas(&v, g);
    assert!(l.contains(&("132".into(), eur(500), Money::ZERO)));
    assert!(l.contains(&("747".into(), Money::ZERO, eur(500))));
    // Sin asientos sí se borra (con confirmación).
    let b = guardar(&v, &subvencion(Tipo::Donacion, 300), now).unwrap();
    assert!(borrar(&v, b, now, false).is_err());
    borrar(&v, b, now, true).unwrap();
}
