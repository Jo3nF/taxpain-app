//! Asientos grabados de la cartera, valoración al cierre (deterioro a coste y valor razonable en
//! negociación) y su paso al 200. Operaciones inventadas, importes redondos.

#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::sync::Arc;

use taxpain_core::accounting::Origin;
use taxpain_core::calendar::TaxpayerProfile;
use taxpain_core::cripto::{Precio, Qty};
use taxpain_core::money::Money;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime};

use super::contabilidad::{self, EntradaCierre, Tratamiento};
use super::*;

const NOW: OffsetDateTime = datetime!(2026-11-16 10:00 UTC);

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn vault(fisica: bool) -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let p = if fisica {
        TaxpayerProfile::autonomo()
    } else {
        TaxpayerProfile::sociedad_limitada()
    };
    v.set_setting("taxpayer_profile", &p).unwrap();
    v
}

struct Demo {
    etf: i64,
    accion: i64,
    usa: i64,
}

fn valor(v: &Vault, ticker: &str, kind: Clase, country: &str) -> i64 {
    guardar_valor(
        v,
        &Valor {
            id: 0,
            isin: String::new(),
            ticker: ticker.into(),
            name: format!("{ticker} Demo"),
            currency: "EUR".into(),
            kind,
            country: country.into(),
            account: String::new(),
        },
        NOW,
    )
    .unwrap()
}

#[allow(clippy::too_many_arguments)]
fn op(
    v: &Vault,
    cuenta: i64,
    sec: i64,
    fecha: Date,
    kind: TipoOp,
    qty: i64,
    amount: Money,
    fee: Money,
    country: &str,
) -> i64 {
    guardar_op(
        v,
        &Op {
            id: 0,
            account_id: cuenta,
            to_account_id: None,
            security_id: Some(sec),
            date: fecha,
            time: "10:00:00".into(),
            kind,
            qty: Qty::parse(&qty.to_string()).unwrap(),
            price: None,
            currency: "EUR".into(),
            amount,
            fee,
            fx: None,
            eur: Money::ZERO,
            fee_eur: Money::ZERO,
            country: country.into(),
            note: String::new(),
            source: "manual".into(),
            fingerprint: None,
            entry_id: None,
        },
        NOW,
    )
    .unwrap()
}

fn precio(v: &Vault, sec: i64, fecha: Date, p: &str) {
    guardar_precios(
        v,
        &[PrecioCierre {
            date: fecha,
            security_id: sec,
            price: Precio::parse(p).unwrap(),
            currency: "EUR".into(),
            source: "manual".into(),
        }],
        NOW,
    )
    .unwrap();
}

/// Sociedad con un bróker: ETF de Irlanda, acción española y acción de EE. UU.
fn cartera(v: &Vault) -> Demo {
    let cuenta = guardar_cuenta(
        v,
        &serde_json::from_value(serde_json::json!({ "broker": "otro", "name": "Bróker Demo" }))
            .unwrap(),
        NOW,
    )
    .unwrap();
    let etf = valor(v, "ETFD", Clase::Etf, "IE");
    let accion = valor(v, "ACCD", Clase::Accion, "ES");
    let usa = valor(v, "USCO", Clase::Accion, "US");
    let d = Demo { etf, accion, usa };
    let c = cuenta;
    op(
        v,
        c,
        etf,
        date!(2026 - 02 - 01),
        TipoOp::Compra,
        100,
        eur(10_000),
        eur(10),
        "",
    );
    op(
        v,
        c,
        accion,
        date!(2026 - 03 - 01),
        TipoOp::Compra,
        50,
        eur(5_000),
        Money::ZERO,
        "",
    );
    op(
        v,
        c,
        usa,
        date!(2026 - 03 - 02),
        TipoOp::Compra,
        20,
        eur(2_000),
        Money::ZERO,
        "",
    );
    op(
        v,
        c,
        accion,
        date!(2026 - 06 - 01),
        TipoOp::Dividendo,
        0,
        eur(200),
        Money::ZERO,
        "ES",
    );
    op(
        v,
        c,
        accion,
        date!(2026 - 06 - 01),
        TipoOp::Retencion,
        0,
        eur(38),
        Money::ZERO,
        "ES",
    );
    op(
        v,
        c,
        usa,
        date!(2026 - 07 - 01),
        TipoOp::Dividendo,
        0,
        eur(100),
        Money::ZERO,
        "US",
    );
    op(
        v,
        c,
        usa,
        date!(2026 - 07 - 01),
        TipoOp::Retencion,
        0,
        eur(15),
        Money::ZERO,
        "US",
    );
    // Venta de 40 ETFD por 4.400 € con 5 € de comisión: coste 40 × 100,10 = 4.004 €.
    op(
        v,
        c,
        etf,
        date!(2026 - 09 - 01),
        TipoOp::Venta,
        40,
        eur(4_400),
        eur(5),
        "",
    );
    d
}

/// Saldo deudor de las cuentas que empiezan por `prefijo` en el ejercicio.
fn saldo(v: &Vault, year: i32, prefijo: &str) -> Money {
    crate::accounting::journal(v, year)
        .unwrap()
        .iter()
        .flat_map(|e| &e.entry.lines)
        .filter(|l| l.account.as_str().starts_with(prefijo))
        .map(|l| l.debit - l.credit)
        .sum()
}

fn asientos_valores(v: &Vault, year: i32) -> usize {
    crate::accounting::journal(v, year)
        .unwrap()
        .iter()
        .filter(|e| matches!(e.entry.origin, Origin::Valores(_)))
        .count()
}

#[test]
fn contabilizar_graba_compras_ventas_dividendos_y_retenciones_y_se_deshace() {
    let v = vault(false);
    let d = cartera(&v);
    assert!(matches!(
        contabilidad::contabilizar(&v, 2026, false, NOW, false),
        Err(crate::error::AppError::NotConfirmed)
    ));
    let c = contabilidad::contabilizar(&v, 2026, false, NOW, true).unwrap();
    assert_eq!((c.asientos, c.saltadas), (8, 0), "{c:?}");
    assert_eq!(asientos_valores(&v, 2026), 8);
    // A coste: la comisión de compra al coste; el resultado de la venta a 766.
    assert_eq!(saldo(&v, 2026, "5400001"), eur(10_010 - 4_004));
    assert_eq!(saldo(&v, 2026, "766"), -eur(391));
    assert_eq!(saldo(&v, 2026, "760"), -eur(300));
    // Retención española a 473; la de EE. UU., gasto por impuesto (6300).
    assert_eq!(saldo(&v, 2026, "473"), eur(38));
    assert_eq!(saldo(&v, 2026, "6300"), eur(15));
    // Las subcuentas quedan en el plan con el nombre del valor.
    assert_eq!(
        crate::accounting::account_name(v.connection(), "5400001")
            .unwrap()
            .as_deref(),
        Some("ETFD ETFD Demo")
    );
    // Lo grabado lo dice el informe; otra vez «Contabilizar» no duplica.
    let r = report(&v, 2026).unwrap();
    assert!(
        r.asientos
            .iter()
            .filter(|a| a.op.is_some())
            .all(|a| a.entry_id.is_some())
    );
    let c = contabilidad::contabilizar(&v, 2026, false, NOW, true).unwrap();
    assert_eq!(c.asientos, 0);
    assert_eq!(asientos_valores(&v, 2026), 8);
    // Una operación con asiento no se cambia ni se borra, ni la clasificación de la cartera.
    let o = ops(&v, Some(2026)).unwrap().into_iter().next().unwrap();
    assert!(guardar_op(&v, &o, NOW).is_err());
    assert!(borrar_op(&v, o.id, NOW).is_err());
    let mut s = settings(&v).unwrap();
    s.cartera = Cartera::Negociacion;
    assert!(save_settings(&v, &s, NOW).is_err());
    // Rehacer: mismos asientos; deshacer: ninguno, y ya se puede editar.
    let c = contabilidad::contabilizar(&v, 2026, true, NOW, true).unwrap();
    assert_eq!((c.borrados, c.asientos), (8, 8));
    assert_eq!(contabilidad::deshacer(&v, 2026, NOW, true).unwrap(), 8);
    assert_eq!(asientos_valores(&v, 2026), 0);
    guardar_op(&v, &o, NOW).unwrap();
    let _ = d;
}

#[test]
fn cierre_a_coste_deteriora_lo_que_cotiza_por_debajo_y_revierte_al_ano_siguiente() {
    let v = vault(false);
    let d = cartera(&v);
    contabilidad::contabilizar(&v, 2026, false, NOW, true).unwrap();
    // 60 ETFD a 80 € = 4.800 € frente a 6.006 € en libros: deterioro de 1.206 €.
    precio(&v, d.etf, date!(2026 - 12 - 31), "80");
    precio(&v, d.accion, date!(2026 - 12 - 31), "120");
    let p = contabilidad::cierre(&v, 2026, None).unwrap();
    assert!(
        p.valores
            .iter()
            .any(|x| x.security_id == d.usa && x.tratamiento == Tratamiento::SinPrecio)
    );
    assert!(
        contabilidad::contabilizar_cierre(&v, 2026, EntradaCierre::default(), NOW, true).is_err()
    );
    precio(&v, d.usa, date!(2026 - 12 - 31), "100");
    let p = contabilidad::cierre(&v, 2026, None).unwrap();
    let etf = p.valores.iter().find(|x| x.security_id == d.etf).unwrap();
    assert_eq!(etf.tratamiento, Tratamiento::Deterioro);
    assert_eq!(etf.ajuste, -eur(1_206));
    let acc = p
        .valores
        .iter()
        .find(|x| x.security_id == d.accion)
        .unwrap();
    assert_eq!(acc.tratamiento, Tratamiento::SinDeterioro);
    assert_eq!(p.deterioro, eur(1_206));
    // Si el usuario dice que no es deterioro, no hay asiento.
    let ex = EntradaCierre {
        excluidos: vec![d.etf],
    };
    let p2 = contabilidad::cierre(&v, 2026, Some(ex.clone())).unwrap();
    assert!(p2.lines.is_empty());
    assert!(contabilidad::contabilizar_cierre(&v, 2026, ex, NOW, true).is_err());
    // El punto del cierre está pendiente hasta grabarlo.
    assert_eq!(
        contabilidad::estado_cierre(&v, 2026).unwrap().map(|e| e.0),
        Some(false)
    );
    contabilidad::contabilizar_cierre(&v, 2026, EntradaCierre::default(), NOW, true).unwrap();
    assert_eq!(saldo(&v, 2026, "698"), eur(1_206));
    assert_eq!(saldo(&v, 2026, "596"), -eur(1_206));
    assert_eq!(
        contabilidad::estado_cierre(&v, 2026).unwrap().map(|e| e.0),
        Some(true)
    );
    let f = contabilidad::fiscal(&v, 2026).unwrap();
    assert_eq!((f.deterioro, f.reversion), (eur(1_206), Money::ZERO));
    // 2027 a 95 €: 5.700 €, deterioro de 306 €; se revierten los 1.206 € de 2026.
    precio(&v, d.etf, date!(2027 - 12 - 31), "95");
    precio(&v, d.accion, date!(2027 - 12 - 31), "120");
    precio(&v, d.usa, date!(2027 - 12 - 31), "100");
    contabilidad::contabilizar_cierre(&v, 2027, EntradaCierre::default(), NOW, true).unwrap();
    assert_eq!(saldo(&v, 2027, "798"), -eur(1_206));
    assert_eq!(saldo(&v, 2027, "698"), eur(306));
    let f = contabilidad::fiscal(&v, 2027).unwrap();
    assert_eq!((f.deterioro, f.reversion), (eur(306), eur(1_206)));
    // Deshacer el de 2027 lo quita.
    contabilidad::deshacer_cierre(&v, 2027, NOW, true).unwrap();
    assert!(contabilidad::fiscal(&v, 2027).unwrap().deterioro.is_zero());
}

#[test]
fn deterioro_de_valores_llega_al_200_sin_proponerse_dos_veces() {
    let v = vault(false);
    let d = cartera(&v);
    precio(&v, d.etf, date!(2026 - 12 - 31), "80");
    precio(&v, d.accion, date!(2026 - 12 - 31), "120");
    precio(&v, d.usa, date!(2026 - 12 - 31), "100");
    contabilidad::contabilizar_cierre(&v, 2026, EntradaCierre::default(), NOW, true).unwrap();
    let f = contabilidad::fiscal(&v, 2026).unwrap();
    let mut saldos = BTreeMap::new();
    saldos.insert("698".to_owned(), eur(1_206));
    let nombres = BTreeMap::new();
    let (props, _) =
        crate::modelo200::propuestas::calcular(&crate::modelo200::propuestas::Contexto {
            saldos: &saldos,
            nombres: &nombres,
            participadas: &[],
            amortizacion: (Money::ZERO, Money::ZERO),
            dias: 365,
            valores: &f,
            cubiertos: &std::collections::BTreeMap::new(),
        });
    let det = props
        .iter()
        .find(|p| p.clave == "valores_deterioro")
        .unwrap();
    assert_eq!(
        (det.casilla, det.importe, det.incluir),
        (325, eur(1_206), true)
    );
    // La 698 de Valores no sale además como deterioro de valores de deuda.
    assert!(
        props.iter().all(|p| p.clave != "deterioro_valores_deuda"),
        "{props:?}"
    );
}

#[test]
fn negociacion_lleva_el_valor_razonable_a_resultados_y_propone_el_15l_sin_incluir() {
    let v = vault(false);
    let mut s = settings(&v).unwrap();
    s.cartera = Cartera::Negociacion;
    save_settings(&v, &s, NOW).unwrap();
    let d = cartera(&v);
    contabilidad::contabilizar(&v, 2026, false, NOW, true).unwrap();
    // En negociación las comisiones van a 669 (10 + 5) y la venta a 7630: 4.400 − 4.000.
    assert_eq!(saldo(&v, 2026, "669"), eur(15));
    assert_eq!(saldo(&v, 2026, "7630"), -eur(400));
    // Cierre: 60 ETFD a 90 € (5.400 frente a 6.000: −600), ACCD 50 a 110 € (+500), USCO igual.
    precio(&v, d.etf, date!(2026 - 12 - 31), "90");
    precio(&v, d.accion, date!(2026 - 12 - 31), "110");
    precio(&v, d.usa, date!(2026 - 12 - 31), "100");
    let p = contabilidad::cierre(&v, 2026, None).unwrap();
    assert_eq!((p.beneficios, p.perdidas), (eur(500), eur(600)));
    contabilidad::contabilizar_cierre(&v, 2026, EntradaCierre::default(), NOW, true).unwrap();
    assert_eq!(saldo(&v, 2026, "6630"), eur(600));
    assert_eq!(saldo(&v, 2026, "5400001"), eur(5_400));
    let f = contabilidad::fiscal(&v, 2026).unwrap();
    assert_eq!(f.perdidas_vr, eur(600));
    let (props, _) =
        crate::modelo200::propuestas::calcular(&crate::modelo200::propuestas::Contexto {
            saldos: &BTreeMap::new(),
            nombres: &BTreeMap::new(),
            participadas: &[],
            amortizacion: (Money::ZERO, Money::ZERO),
            dias: 365,
            valores: &f,
            cubiertos: &std::collections::BTreeMap::new(),
        });
    let vr = props
        .iter()
        .find(|p| p.clave == "valores_vr_perdidas")
        .unwrap();
    assert_eq!((vr.casilla, vr.importe, vr.incluir), (413, eur(600), false));
    // 2027 a 100 €: el ajuste parte del valor razonable de 2026 (+600, no 0).
    precio(&v, d.etf, date!(2027 - 12 - 31), "100");
    precio(&v, d.accion, date!(2027 - 12 - 31), "110");
    precio(&v, d.usa, date!(2027 - 12 - 31), "100");
    let p = contabilidad::cierre(&v, 2027, None).unwrap();
    assert_eq!((p.beneficios, p.perdidas), (eur(600), Money::ZERO));
}

#[test]
fn una_persona_fisica_no_contabiliza_la_cartera() {
    let v = vault(true);
    cartera(&v);
    assert!(contabilidad::contabilizar(&v, 2026, false, NOW, true).is_err());
    assert!(contabilidad::cierre(&v, 2026, None).is_err());
    assert!(contabilidad::estado_cierre(&v, 2026).unwrap().is_none());
    assert!(contabilidad::fiscal(&v, 2026).unwrap().deterioro.is_zero());
}
