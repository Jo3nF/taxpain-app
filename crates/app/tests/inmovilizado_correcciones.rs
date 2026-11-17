//! Correcciones del inmovilizado (G35): editar un elemento con el recálculo de lo no
//! registrado, borrarlo, deshacer amortizaciones y bajas, y los incentivos fiscales de su
//! amortización (escaso valor, art. 12.3.e LIS; libertad con creación de empleo, art. 102; y
//! acelerada, art. 103) con la diferencia que va al 200 y su reversión en la baja. Datos
//! inventados.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::{
    add_fixed_asset, depreciation_differences, dispose_asset, fixed_assets, journal,
    post_depreciation, set_depreciation_plan, trial_balance,
};
use taxpain_app::inmovilizado::{
    CambiosBien, Incentivo, delete_asset, depreciation_preview, edit_asset, incentive_usage,
    set_incentive, undo_depreciation, undo_disposal,
};
use taxpain_core::accounting::AccountCode;
use taxpain_core::accounting::depreciation::{FixedAsset, Method};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime};

const AHORA: OffsetDateTime = datetime!(2027-08-01 10:00 UTC);

fn vault() -> Vault {
    Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap()
}

fn acc(s: &str) -> AccountCode {
    AccountCode::new(s).unwrap()
}

fn bien(name: &str, account: &str, desde: Date, euros: i64, pct: i32) -> FixedAsset {
    FixedAsset {
        id: 0,
        name: name.into(),
        account: acc(account),
        depreciation_account: acc(&format!("28{}", &account[1..])),
        expense_account: acc("681"),
        acquired_on: desde,
        cost: Money::from_euros(euros, 0),
        residual: Money::ZERO,
        rate: Rate::percent(pct),
        disposed_on: None,
        method: Method::Lineal,
        tax_rate: None,
    }
}

fn alta(v: &Vault, a: &FixedAsset) -> i64 {
    add_fixed_asset(v, a, AHORA).unwrap()
}

/// Cuotas de amortización del elemento en el diario del ejercicio: (fecha, cuota).
fn cuotas(v: &Vault, year: i32, name: &str) -> Vec<(Date, i64)> {
    journal(v, year)
        .unwrap()
        .into_iter()
        .filter(|e| e.entry.concept.starts_with("Amortización") && e.entry.concept.ends_with(name))
        .map(|e| {
            (
                e.entry.date,
                e.entry.lines.iter().map(|l| l.debit.cents()).sum(),
            )
        })
        .collect()
}

fn saldo(v: &Vault, year: i32, account: &str) -> Money {
    trial_balance(v, year, None)
        .unwrap()
        .iter()
        .find(|x| x.account.as_str() == account)
        .map_or(Money::ZERO, |x| x.balance())
}

#[test]
fn editar_un_elemento_recalcula_lo_que_no_esta_registrado() {
    let v = vault();
    let id = alta(
        &v,
        &bien("Portátil demo", "217", date!(2026 - 07 - 01), 1200, 25),
    );
    let cambios = CambiosBien {
        name: "Portátil demo 14\"".into(),
        acquired_on: date!(2026 - 07 - 01),
        rate: Rate::percent(20),
        residual: Money::from_euros(200, 0),
    };
    edit_asset(&v, id, &cambios, AHORA).unwrap();
    // 1.000 × 20 % × 184/365 días.
    let p = depreciation_preview(&v, 2026).unwrap();
    assert_eq!(p.total, Money::from_cents(10_082));
    assert_eq!(p.pendientes[0].name, "Portátil demo 14\"");
    post_depreciation(&v, 2026, AHORA).unwrap();
    let p = depreciation_preview(&v, 2026).unwrap();
    assert!(p.pendientes.is_empty());
    assert_eq!(p.total_registrado, Money::from_cents(10_082));

    // Con la de 2026 registrada, la fecha ya no se cambia; el coeficiente sí, hacia delante.
    let err = edit_asset(
        &v,
        id,
        &CambiosBien {
            acquired_on: date!(2026 - 09 - 01),
            ..cambios.clone()
        },
        AHORA,
    )
    .unwrap_err();
    assert!(err.to_string().contains("2026"), "{err}");
    let avisos = edit_asset(
        &v,
        id,
        &CambiosBien {
            rate: Rate::percent(50),
            ..cambios.clone()
        },
        AHORA,
    )
    .unwrap();
    assert!(avisos.iter().any(|a| a.contains("no cambia")), "{avisos:?}");
    assert_eq!(cuotas(&v, 2026, "14\""), [(date!(2026 - 12 - 31), 10_082)]);
    assert_eq!(
        depreciation_preview(&v, 2027).unwrap().total,
        Money::from_euros(500, 0)
    );
    // El residual no puede dejar menos de lo ya amortizado.
    assert!(
        edit_asset(
            &v,
            id,
            &CambiosBien {
                residual: Money::from_euros(1150, 0),
                ..cambios
            },
            AHORA,
        )
        .is_err()
    );
    // Con asientos no se borra; uno recién dado de alta, sí.
    assert!(delete_asset(&v, id, AHORA, true).is_err());
    let otro = alta(
        &v,
        &bien("Silla demo", "216", date!(2026 - 03 - 01), 200, 10),
    );
    assert!(delete_asset(&v, otro, AHORA, false).is_err());
    delete_asset(&v, otro, AHORA, true).unwrap();
    assert_eq!(fixed_assets(&v).unwrap().len(), 1);
}

#[test]
fn deshacer_las_amortizaciones_de_la_ultima_hacia_atras() {
    let v = vault();
    alta(
        &v,
        &bien("Portátil demo", "217", date!(2026 - 07 - 01), 1200, 25),
    );
    alta(
        &v,
        &bien("Mesa demo", "216", date!(2026 - 01 - 01), 1000, 10),
    );
    post_depreciation(&v, 2026, AHORA).unwrap();
    post_depreciation(&v, 2027, AHORA).unwrap();
    assert!(undo_depreciation(&v, 2027, None, AHORA, false).is_err());
    // 2026 no se deshace mientras esté la de 2027.
    let r = undo_depreciation(&v, 2026, None, AHORA, true).unwrap();
    assert_eq!(r.borrados, 0);
    assert_eq!(r.saltados.len(), 2, "{:?}", r.saltados);
    // La de 2027 de un solo elemento y luego la del otro.
    let r = undo_depreciation(&v, 2027, Some(2), AHORA, true).unwrap();
    assert_eq!((r.borrados, r.total), (1, Money::from_euros(100, 0)));
    assert_eq!(cuotas(&v, 2027, "Mesa demo"), []);
    let r = undo_depreciation(&v, 2027, None, AHORA, true).unwrap();
    assert_eq!((r.borrados, r.total), (1, Money::from_euros(300, 0)));
    let r = undo_depreciation(&v, 2026, None, AHORA, true).unwrap();
    assert_eq!(r.borrados, 2);
    assert_eq!(depreciation_preview(&v, 2026).unwrap().pendientes.len(), 2);
    assert!(undo_depreciation(&v, 2026, None, AHORA, true).is_err());
}

#[test]
fn deshacer_una_baja_deja_la_amortizacion_como_estaba() {
    let v = vault();
    let id = alta(
        &v,
        &bien("Portátil demo", "217", date!(2026 - 07 - 01), 1200, 25),
    );
    post_depreciation(&v, 2026, AHORA).unwrap();
    post_depreciation(&v, 2027, AHORA).unwrap();
    let full_2027 = cuotas(&v, 2027, "Portátil demo");
    assert_eq!(full_2027, [(date!(2027 - 12 - 31), 30_000)]);
    // Venta el 30/6/2027: la amortización de 2027 se rehace hasta ese día.
    dispose_asset(
        &v,
        id,
        date!(2027 - 06 - 30),
        Money::from_euros(1000, 0),
        AHORA,
    )
    .unwrap();
    assert_eq!(
        cuotas(&v, 2027, "Portátil demo"),
        [(date!(2027 - 06 - 30), 14_877)]
    );
    assert!(undo_disposal(&v, id, AHORA, false).is_err());
    let r = undo_disposal(&v, id, AHORA, true).unwrap();
    assert_eq!(r.restored.len(), 1);
    assert!(r.warnings.iter().any(|w| w.contains("543")));
    assert_eq!(cuotas(&v, 2027, "Portátil demo"), full_2027);
    assert!(fixed_assets(&v).unwrap()[0].disposed_on.is_none());
    assert_eq!(saldo(&v, 2027, "543"), Money::ZERO);
    assert_eq!(saldo(&v, 2027, "771"), Money::ZERO);
    assert_eq!(saldo(&v, 2027, "2817"), -Money::from_euros(300, 0));
    assert!(
        undo_disposal(&v, id, AHORA, true).is_err(),
        "ya no está de baja"
    );

    // Una baja del año anterior quitó la amortización de 2027: vuelve.
    dispose_asset(&v, id, date!(2026 - 10 - 01), Money::ZERO, AHORA).unwrap();
    assert_eq!(cuotas(&v, 2027, "Portátil demo"), []);
    undo_disposal(&v, id, AHORA, true).unwrap();
    assert_eq!(
        cuotas(&v, 2026, "Portátil demo"),
        [(date!(2026 - 12 - 31), 15_123)]
    );
    assert_eq!(cuotas(&v, 2027, "Portátil demo"), full_2027);

    // Sin la amortización del año registrada, la que hizo la baja se quita.
    undo_depreciation(&v, 2027, None, AHORA, true).unwrap();
    dispose_asset(&v, id, date!(2027 - 03 - 31), Money::ZERO, AHORA).unwrap();
    assert_eq!(cuotas(&v, 2027, "Portátil demo").len(), 1);
    let r = undo_disposal(&v, id, AHORA, true).unwrap();
    assert!(r.removed.is_some());
    assert_eq!(cuotas(&v, 2027, "Portátil demo"), []);
    assert_eq!(
        depreciation_preview(&v, 2027).unwrap().total,
        Money::from_euros(300, 0)
    );
}

#[test]
fn escaso_valor_con_sus_topes_y_la_reversion_en_la_baja() {
    let v = vault();
    let caro = alta(
        &v,
        &bien("Monitor demo", "217", date!(2026 - 01 - 01), 350, 20),
    );
    let err = set_incentive(&v, caro, Some(Incentivo::EscasoValor), None, AHORA).unwrap_err();
    assert!(err.to_string().contains("300"), "{err}");
    let id = alta(
        &v,
        &bien("Teclado demo", "217", date!(2026 - 01 - 01), 300, 20),
    );
    let avisos = set_incentive(&v, id, Some(Incentivo::EscasoValor), None, AHORA).unwrap();
    assert!(avisos.iter().any(|a| a.contains("nuevos")), "{avisos:?}");
    let a = fixed_assets(&v)
        .unwrap()
        .into_iter()
        .find(|a| a.id == id)
        .unwrap();
    assert_eq!(a.tax_rate, Some(Rate::percent(100)));
    // Con el incentivo, el plan fiscal no puede bajar del 100 %.
    assert!(set_depreciation_plan(&v, id, Method::Lineal, Some(Rate::percent(50)), AHORA).is_err());
    // 2026: contable 60, fiscal 300 → disminución de 240 en el 200.
    assert_eq!(
        depreciation_differences(&v, 2026).unwrap(),
        (Money::ZERO, Money::from_euros(240, 0))
    );
    post_depreciation(&v, 2026, AHORA).unwrap();
    // Se vende en 2027: la diferencia acumulada se revierte entera (aumento de 240).
    dispose_asset(
        &v,
        id,
        date!(2027 - 06 - 30),
        Money::from_euros(100, 0),
        AHORA,
    )
    .unwrap();
    assert_eq!(
        depreciation_differences(&v, 2027).unwrap(),
        (Money::from_euros(240, 0), Money::ZERO)
    );

    // El límite de 25.000 € del periodo.
    let mut puestos = 1;
    let mut err = None;
    for k in 0..90 {
        let x = alta(
            &v,
            &bien(
                &format!("Silla demo {k}"),
                "216",
                date!(2026 - 05 - 01),
                300,
                10,
            ),
        );
        match set_incentive(&v, x, Some(Incentivo::EscasoValor), None, AHORA) {
            Ok(_) => puestos += 1,
            Err(e) => {
                err = Some(e.to_string());
                break;
            }
        }
    }
    assert_eq!(puestos, 83, "83 × 300 = 24.900");
    assert!(err.unwrap().contains("25.000"));
    let u = incentive_usage(&v, 2026).unwrap();
    assert_eq!(u.escaso_valor, Money::from_euros(24_900, 0));
}

#[test]
fn incentivos_de_la_empresa_de_reducida_dimension() {
    let v = vault();
    let nave = alta(
        &v,
        &bien("Máquina demo", "213", date!(2026 - 02 - 01), 70_000, 12),
    );
    let empleo = |c| {
        Some(Incentivo::ErdEmpleo {
            incremento_plantilla: c,
        })
    };
    assert!(set_incentive(&v, nave, empleo(0), None, AHORA).is_err());
    // 0,50 personas: hasta 60.000 €.
    let err = set_incentive(&v, nave, empleo(50), None, AHORA).unwrap_err();
    assert!(err.to_string().contains("60.000"), "{err}");
    let avisos = set_incentive(&v, nave, empleo(100), None, AHORA).unwrap();
    assert!(
        avisos.iter().any(|a| a.contains("plantilla media")),
        "{avisos:?}"
    );
    assert!(
        avisos.iter().any(|a| a.contains("reducida dimensión")),
        "{avisos:?}"
    );
    assert_eq!(
        incentive_usage(&v, 2026).unwrap().erd_empleo_tope,
        Some(Money::from_euros(120_000, 0))
    );

    // Acelerada: hasta el doble del coeficiente de tablas.
    let furgo = alta(
        &v,
        &bien("Furgoneta demo", "218", date!(2026 - 03 - 01), 20_000, 16),
    );
    let acel = Some(Incentivo::ErdAcelerada {
        coef_tablas: Rate::percent(16),
    });
    assert!(set_incentive(&v, furgo, acel.clone(), Some(Rate::percent(40)), AHORA).is_err());
    set_incentive(&v, furgo, acel, None, AHORA).unwrap();
    let a = fixed_assets(&v)
        .unwrap()
        .into_iter()
        .find(|a| a.id == furgo)
        .unwrap();
    assert_eq!(a.tax_rate, Some(Rate::percent(32)));
    // Quitarlo deja el coeficiente fiscal que se diga.
    set_incentive(&v, furgo, None, None, AHORA).unwrap();
    let a = fixed_assets(&v)
        .unwrap()
        .into_iter()
        .find(|a| a.id == furgo)
        .unwrap();
    assert_eq!(a.tax_rate, None);
}
