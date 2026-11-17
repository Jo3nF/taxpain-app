//! Inversiones en valores con el almacén: importación sin duplicar (mismo fichero dos veces,
//! PDF que se solapan, sólo posiciones y luego el informe completo), coste medio, divisas,
//! dividendos con retención, 720 y asientos que cuadran. Datos inventados.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_core::calendar::TaxpayerProfile;
use taxpain_core::cripto::{Precio, Qty};
use taxpain_core::money::Money;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::OffsetDateTime;
use time::macros::{date, datetime};

use super::importar::{self, Extracto};
use super::*;

const NOW: OffsetDateTime = datetime!(2026-11-02 10:00 UTC);

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

fn xtb() -> Vec<Extracto> {
    vec![xtb::leer(xtb::tests::INFORME).unwrap()]
}

fn flex() -> Vec<Extracto> {
    flex::leer_xml(flex::tests::FLEX).unwrap()
}

#[test]
fn el_mismo_pdf_dos_veces_no_duplica_y_cuadra_con_el_broker() {
    let v = vault(false);
    let a = importar::aplicar(&v, &xtb(), "xtb-pdf", "h1", NOW).unwrap();
    assert_eq!(a.nuevas, 11);
    assert_eq!(a.duplicadas, 0);
    assert_eq!(a.cuentas_nuevas, 1);
    // El cuadre con las posiciones abiertas del PDF sale bien.
    assert!(a.avisos.is_empty(), "{:?}", a.avisos);
    let b = importar::aplicar(&v, &xtb(), "xtb-pdf", "h1", NOW).unwrap();
    assert_eq!((b.nuevas, b.duplicadas), (0, 11));
    let c = &cuentas(&v).unwrap()[0];
    assert_eq!(c.broker, Broker::Xtb);
    assert_eq!(c.number, "12345678");
    assert_eq!(c.country, "PL");
    assert!(c.abroad);
    assert!(c.last_sync_note.contains("0 nuevas, 11 ya estaban"));
    // La vista previa también lo sabe.
    let l = importar::leer(&v, &[("informe.pdf".into(), b"no es un pdf".to_vec())]).unwrap();
    assert!(l.ficheros[0].error.is_some());
}

#[test]
fn primero_las_posiciones_cerradas_y_luego_el_informe_completo() {
    let v = vault(false);
    let solo = xtb::tests::INFORME
        .split("CASH OPERATION HISTORY")
        .next()
        .unwrap()
        .to_owned();
    let parcial = xtb::leer(&solo).unwrap();
    let a = importar::aplicar(&v, std::slice::from_ref(&parcial), "xtb-pdf", "h0", NOW).unwrap();
    assert_eq!(a.nuevas, 6);
    // Luego el informe completo: sus compras y ventas sustituyen a las de las posiciones.
    let b = importar::aplicar(&v, &xtb(), "xtb-pdf", "h1", NOW).unwrap();
    assert_eq!(b.sustituidas, 6, "{b:?}");
    assert_eq!(b.nuevas, 11);
    // Y el de sólo posiciones otra vez ya no entra.
    let c = importar::aplicar(&v, &[parcial], "xtb-pdf", "h0", NOW).unwrap();
    assert_eq!((c.nuevas, c.duplicadas), (0, 6));
    let compras = ops(&v, None)
        .unwrap()
        .into_iter()
        .filter(|o| o.kind == TipoOp::Compra)
        .count();
    assert_eq!(compras, 3);
}

#[test]
fn informe_de_la_sociedad_con_coste_medio_divisas_y_retenciones() {
    let v = vault(false);
    importar::aplicar(&v, &xtb(), "xtb-pdf", "h1", NOW).unwrap();
    let r = report(&v, 2025).unwrap();
    assert_eq!(r.metodo, calculo::Metodo::Pmp);
    // Compras de ACME: 10 a 100 (1.000) y 0,5 a 100,20 (50,10): coste medio 100,0095… Se
    // venden las 10,5: cobrado 448 + 660 + 55 = 1.163; coste 1.050,10; ganancia 112,90.
    assert_eq!(r.resultado.ventas, 3);
    assert_eq!(r.resultado.neto, Money::from_cents(11_290));
    assert_eq!(r.resultado.dividendos, Money::from_cents(500));
    assert_eq!(r.resultado.retenciones_extranjero, Money::from_cents(132));
    assert_eq!(r.resultado.retenciones_espana, Money::from_cents(5));
    assert_eq!(r.resultado.intereses, Money::from_cents(25));
    // Posición a 31/12/2025 sin precio de esa fecha (el del PDF es de enero de 2026).
    assert_eq!(r.posiciones.len(), 1);
    assert_eq!(r.posiciones[0].cantidad, Qty::from_units(100));
    assert!(r.posiciones[0].valor.is_none());
    assert!(r.avisos.iter().any(|a| a.contains("Sin precio de cierre")));
    // Con el precio de cierre, el valor y la plusvalía latente.
    guardar_precios(
        &v,
        &[PrecioCierre {
            date: date!(2025 - 12 - 31),
            security_id: r.posiciones[0].security_id,
            price: Precio::parse("106").unwrap(),
            currency: "EUR".into(),
            source: "manual".into(),
        }],
        NOW,
    )
    .unwrap();
    let r = report(&v, 2025).unwrap();
    assert_eq!(r.posiciones[0].valor, Some(Money::from_euros(10_600, 0)));
    assert_eq!(r.posiciones[0].latente, Some(Money::from_euros(100, 0)));
    // Retención alemana del 26,375 %: el convenio (15 %, sin comprobar) la limita.
    let de = &r.doble_imposicion.paises[0];
    assert_eq!(de.pais, "DE");
    assert_eq!(de.tope_convenio, Some(Money::from_cents(75)));
    assert_eq!(de.deducible, Money::from_cents(75));
    assert_eq!(de.exceso, Money::from_cents(57));
    // Los asientos propuestos cuadran todos.
    assert!(!r.asientos.is_empty());
    for a in &r.asientos {
        let d: Money = a.lineas.iter().map(|l| l.debe).sum();
        let h: Money = a.lineas.iter().map(|l| l.haber).sum();
        assert_eq!(d, h, "{a:?}");
    }
    let venta = r
        .asientos
        .iter()
        .find(|a| a.concepto.starts_with("Venta de 4 "))
        .unwrap();
    assert!(venta.lineas.iter().any(|l| l.cuenta == "766"), "{venta:?}");
    assert!(
        venta
            .lineas
            .iter()
            .any(|l| l.cuenta.starts_with("540") && !l.haber.is_zero())
    );
}

#[test]
fn ibkr_en_dolares_con_dividendo_retencion_y_720() {
    let v = vault(false);
    let a = importar::aplicar(&v, &flex(), "api", "", NOW).unwrap();
    assert_eq!(a.nuevas, 9);
    assert_eq!(a.precios, 2);
    // El cuadre: ACME 10 + 10 del split = 20; ETFX 20 − 8 = 12, como dice IBKR.
    assert!(
        a.avisos.iter().all(|x| !x.contains("dice")),
        "{:?}",
        a.avisos
    );
    // Otra vez el mismo extracto (la sincronización siguiente): nada nuevo.
    let b = importar::aplicar(&v, &flex(), "api", "", NOW).unwrap();
    assert_eq!((b.nuevas, b.duplicadas), (0, 9));
    let r = report(&v, 2026).unwrap();
    let acme = r.posiciones.iter().find(|p| p.ticker == "ACME").unwrap();
    // Coste: 900 € + 0,90 € de comisión; 20 títulos a 60 $ con el cambio de fin de año (0,95).
    assert_eq!(acme.coste, Money::from_cents(90_090));
    assert_eq!(acme.valor, Some(Money::from_euros(1_140, 0)));
    // ETFX: 20 a 50 + 1,25; se venden 8 a 55 − 1,25: coste 8 × 50,0625 = 400,50.
    let venta = &r.realizados[0];
    assert_eq!(venta.transmision, Money::from_cents(43_875));
    assert_eq!(venta.coste, Money::from_cents(40_050));
    // Dividendo de 20 $ al 0,92 con 3 $ retenidos: el 15 % del convenio es justo lo retenido.
    let us = r
        .doble_imposicion
        .paises
        .iter()
        .find(|p| p.pais == "US")
        .unwrap();
    assert_eq!(us.rentas, Money::from_cents(1_840));
    assert_eq!(us.retenido, Money::from_cents(276));
    assert_eq!(us.deducible, Money::from_cents(276));
    assert!(us.convenio_comprobado);
    // 720: poco valor y sociedad: no toca, y lo dice.
    assert!(!r.m720.supera);
    assert!(r.m720.exenta_por_contabilidad);
    assert!(r.m720.nota.contains("42 ter.4"), "{}", r.m720.nota);
}

#[test]
fn aviso_del_720_por_encima_de_50000() {
    for fisica in [false, true] {
        let v = vault(fisica);
        let mut e = flex().remove(0);
        // Una compra grande en euros de un tercer valor, con su precio de cierre.
        e.ops.push(importar::OpLeida {
            fecha: date!(2026 - 05 - 05),
            hora: "10:00:00".into(),
            tipo: TipoOp::Compra,
            valor: Some("BIGCO".into()),
            cantidad: Qty::from_units(1_000),
            precio: Precio::parse("60"),
            divisa: "EUR".into(),
            importe: Money::from_euros(60_000, 0),
            comision: Money::ZERO,
            cambio: None,
            euros: Money::from_euros(60_000, 0),
            comision_eur: Money::ZERO,
            pais: String::new(),
            nota: String::new(),
            fuente: "ibkr".into(),
            huella: "ibkr:U0000001:t:999".into(),
            comision_huella: Vec::new(),
        });
        e.precios.push(importar::PrecioLeido {
            fecha: date!(2026 - 12 - 31),
            valor: "BIGCO".into(),
            precio: Precio::parse("61").unwrap(),
            divisa: "EUR".into(),
        });
        e.posiciones.clear();
        importar::aplicar(&v, &[e], "api", "", NOW).unwrap();
        let r = report(&v, 2026).unwrap();
        assert!(r.m720.supera, "{}", r.m720.nota);
        assert!(r.m720.total_extranjero > Money::from_euros(61_000, 0));
        if fisica {
            assert!(r.m720.nota.contains("toca el 720"), "{}", r.m720.nota);
            assert_eq!(r.metodo, calculo::Metodo::Fifo);
            assert!(r.asientos.is_empty());
        } else {
            assert!(
                r.m720.nota.contains("no tiene que presentar"),
                "{}",
                r.m720.nota
            );
        }
    }
}

#[test]
fn operaciones_a_mano_cuentas_valores_y_token() {
    let v = vault(false);
    let id = guardar_cuenta(
        &v,
        &Cuenta {
            id: 0,
            broker: Broker::Ibkr,
            name: "IBKR".into(),
            number: String::new(),
            currency: "EUR".into(),
            country: "IE".into(),
            abroad: true,
            account: String::new(),
            flex_query: String::new(),
            last_sync: None,
            last_sync_note: String::new(),
            token: false,
        },
        NOW,
    )
    .unwrap();
    assert!(credenciales_ibkr(&v, id).is_err());
    assert!(guardar_token_ibkr(&v, id, "corto", "123", NOW).is_err());
    guardar_token_ibkr(&v, id, "abcdef0123456789abcdef", "987654", NOW).unwrap();
    let c = cuenta(&v, id).unwrap();
    assert!(c.token);
    assert_eq!(c.flex_query, "987654");
    // El token no sale en lo que se serializa hacia la interfaz.
    let json = serde_json::to_string(&cuentas(&v).unwrap()).unwrap();
    assert!(!json.contains("abcdef0123"), "{json}");
    let (t, q) = credenciales_ibkr(&v, id).unwrap();
    assert_eq!(
        (t.as_str(), q.as_str()),
        ("abcdef0123456789abcdef", "987654")
    );
    // La primera sincronización completa la cuenta dada de alta a mano (no crea otra).
    importar::aplicar(&v, &flex(), "api", "", NOW).unwrap();
    assert_eq!(cuentas(&v).unwrap().len(), 1);
    assert_eq!(cuenta(&v, id).unwrap().number, "U0000001");
    quitar_token_ibkr(&v, id, NOW).unwrap();
    assert!(!cuenta(&v, id).unwrap().token);

    // Una compra en dólares a mano necesita el cambio.
    let sid = valores(&v).unwrap()[0].id;
    let mut op = Op {
        id: 0,
        account_id: id,
        to_account_id: None,
        security_id: Some(sid),
        date: date!(2026 - 11 - 02),
        time: "10:00:00".into(),
        kind: TipoOp::Compra,
        qty: Qty::from_units(2),
        price: Precio::parse("50"),
        currency: "usd".into(),
        amount: Money::from_euros(100, 0),
        fee: Money::from_euros(1, 0),
        fx: None,
        eur: Money::ZERO,
        fee_eur: Money::ZERO,
        country: String::new(),
        note: String::new(),
        source: "manual".into(),
        fingerprint: None,
        entry_id: None,
    };
    assert!(guardar_op(&v, &op, NOW).is_err());
    op.fx = Precio::parse("0.9");
    let oid = guardar_op(&v, &op, NOW).unwrap();
    let guardada = ops(&v, Some(2026))
        .unwrap()
        .into_iter()
        .find(|o| o.id == oid)
        .unwrap();
    assert_eq!(guardada.eur, Money::from_euros(90, 0));
    assert_eq!(guardada.fee_eur, Money::from_cents(90));
    assert_eq!(guardada.currency, "USD");
    borrar_op(&v, oid, NOW).unwrap();
    assert!(borrar_cuenta(&v, id, NOW).is_err());
}

#[test]
fn juntar_el_mismo_valor_de_dos_brokeres_por_su_isin() {
    let v = vault(false);
    importar::aplicar(&v, &xtb(), "xtb-pdf", "h1", NOW).unwrap();
    importar::aplicar(&v, &flex(), "api", "", NOW).unwrap();
    let antes = valores(&v).unwrap();
    let acme_xtb = antes
        .iter()
        .find(|x| x.ticker == "ACME.DE")
        .unwrap()
        .clone();
    let destino = antes.iter().find(|x| x.isin == "IE00B4L5Y983").unwrap().id;
    // El usuario le pone el ISIN del ETF de IBKR: se juntan (datos inventados).
    let id = guardar_valor(
        &v,
        &Valor {
            isin: "IE00B4L5Y983".into(),
            ..acme_xtb.clone()
        },
        NOW,
    )
    .unwrap();
    assert_eq!(id, destino);
    let despues = valores(&v).unwrap();
    assert_eq!(despues.len(), antes.len() - 1);
    assert!(
        ops(&v, None)
            .unwrap()
            .iter()
            .all(|o| o.security_id != Some(acme_xtb.id))
    );
    assert!(
        guardar_valor(
            &v,
            &Valor {
                isin: "XX123".into(),
                ..despues[0].clone()
            },
            NOW
        )
        .is_err()
    );
}

#[test]
fn isin_con_su_digito_de_control() {
    assert!(isin_valido("US0378331005"));
    assert!(isin_valido("IE00B4L5Y983"));
    assert!(!isin_valido("US0378331006"));
    assert!(!isin_valido("ACME.DE"));
}

#[test]
fn un_pdf_generado_se_lee_entero() {
    // El informe sintético en un PDF de verdad (como lo extrae pdf-extract).
    // Cada línea como una fila (sin partirla), como sale en el PDF de XTB.
    let paras: Vec<taxpain_invoicing::pdf::Para> = xtb::tests::INFORME
        .lines()
        .map(|l| taxpain_invoicing::pdf::Para::Row(l.to_owned(), String::new()))
        .collect();
    let pdf = taxpain_invoicing::pdf::text_document("Informe XTB", &[paras]).unwrap();
    let ex = importar::leer_fichero("informe.pdf", &pdf).unwrap();
    let n = ex[0].ops.len();
    assert_eq!(n, 11, "{:?}", ex[0].avisos);
    assert_eq!(importar::reconocer("informe.pdf", &pdf), Some("XTB (PDF)"));
}

#[test]
fn sincronizar_dos_veces_no_duplica_y_comprueba_la_cuenta() {
    let v = vault(false);
    let id = guardar_cuenta(
        &v,
        &Cuenta {
            id: 0,
            broker: Broker::Ibkr,
            name: "IBKR empresa".into(),
            number: String::new(),
            currency: "EUR".into(),
            country: "IE".into(),
            abroad: true,
            account: String::new(),
            flex_query: "123456".into(),
            last_sync: None,
            last_sync_note: String::new(),
            token: false,
        },
        NOW,
    )
    .unwrap();
    // Lo que devolvería el Flex Web Service (el servidor simulado está en taxpain-banca).
    let xml = vec![flex::tests::FLEX.to_owned()];
    let a = importar::aplicar_descarga(&v, id, &xml, NOW).unwrap();
    assert_eq!(a.nuevas, 9);
    let b = importar::aplicar_descarga(&v, id, &xml, NOW).unwrap();
    assert_eq!((b.nuevas, b.duplicadas), (0, 9));
    let c = cuenta(&v, id).unwrap();
    assert_eq!(c.number, "U0000001");
    assert!(c.last_sync.is_some());
    assert!(
        c.last_sync_note.starts_with("Sincronizado"),
        "{}",
        c.last_sync_note
    );
    // Un extracto de otra cuenta no entra.
    let otra = vec![flex::tests::FLEX.replace("U0000001", "U0000009")];
    let err = importar::aplicar_descarga(&v, id, &otra, NOW).unwrap_err();
    assert!(err.to_string().contains("U0000009"), "{err}");
    // El periodo: el año entero o, en el año en curso, hasta ayer.
    assert_eq!(
        importar::periodo_sincronizacion(2025, date!(2026 - 11 - 02)).unwrap(),
        (date!(2025 - 01 - 01), date!(2025 - 12 - 31))
    );
    assert_eq!(
        importar::periodo_sincronizacion(2026, date!(2026 - 11 - 02)).unwrap(),
        (date!(2026 - 01 - 01), date!(2026 - 11 - 01))
    );
    assert!(importar::periodo_sincronizacion(2027, date!(2026 - 11 - 02)).is_err());
}

#[test]
fn la_carpeta_reconoce_los_ficheros_de_broker_y_no_los_duplica() {
    use crate::carpeta::{self, Found};
    let v = vault(false);
    let dir = tempfile::tempdir().unwrap();
    let inv = dir.path().join("05_INVERSIONES").join("2025");
    std::fs::create_dir_all(&inv).unwrap();
    let paras: Vec<taxpain_invoicing::pdf::Para> = xtb::tests::INFORME
        .lines()
        .map(|l| taxpain_invoicing::pdf::Para::Row(l.to_owned(), String::new()))
        .collect();
    let pdf = taxpain_invoicing::pdf::text_document("XTB", &[paras]).unwrap();
    std::fs::write(inv.join("posiciones-cerradas-XTB.pdf"), &pdf).unwrap();
    std::fs::write(inv.join("ibkr-2026.xml"), flex::tests::FLEX).unwrap();
    let scan = carpeta::scan(&v, dir.path()).unwrap();
    let mut rutas = Vec::new();
    for i in &scan.items {
        match &i.found {
            Found::Valores {
                formato,
                cuenta,
                ops,
                duplicates,
                ..
            } => {
                assert!(i.importable, "{i:?}");
                assert_eq!(*duplicates, 0);
                if formato == "XTB (PDF)" {
                    assert_eq!((cuenta.as_str(), *ops), ("12345678", 11));
                } else {
                    assert_eq!(formato, "Interactive Brokers (Flex XML)");
                    assert_eq!((cuenta.as_str(), *ops), ("U0000001", 9));
                }
                rutas.push(i.path.clone());
            }
            otro => panic!("{} no es de bróker: {otro:?}", i.path),
        }
    }
    assert_eq!(rutas.len(), 2);
    let res = carpeta::apply(&v, dir.path(), &rutas, NOW).unwrap();
    let mensajes: Vec<String> = res.iter().map(|o| format!("{o:?}")).collect();
    assert!(
        mensajes
            .iter()
            .any(|m| m.contains("11 operaciones a Inversiones en valores")),
        "{mensajes:?}"
    );
    assert_eq!(cuentas(&v).unwrap().len(), 2);
    // Otra vez: ya están, no se proponen.
    let scan = carpeta::scan(&v, dir.path()).unwrap();
    assert!(scan.items.iter().all(|i| !i.importable), "{:?}", scan.items);
}

#[test]
fn exporta_el_informe_a_excel_y_pdf() {
    let v = vault(false);
    importar::aplicar(&v, &flex(), "api", "", NOW).unwrap();
    let (nombre, xlsx) = export::workbook(&v, 2026).unwrap();
    assert_eq!(nombre, "Inversiones en valores 2026.xlsx");
    assert!(xlsx.starts_with(b"PK"));
    let (nombre, pdf) = export::pdf_bytes(&v, 2026).unwrap();
    assert_eq!(nombre, "Inversiones en valores 2026.pdf");
    let texto = crate::pdf_text::text(&pdf).unwrap();
    assert!(texto.contains("Modelo 720"), "{texto}");
}

#[test]
fn alta_de_un_valor_desde_la_pantalla() {
    let v = vault(false);
    let nuevo = Valor {
        id: 0,
        isin: String::new(),
        ticker: "demo.de".into(),
        name: "ETF de prueba".into(),
        currency: String::new(),
        kind: Clase::Etf,
        country: String::new(),
        account: String::new(),
    };
    let id = guardar_valor(&v, &nuevo, NOW).unwrap();
    let x = valores(&v)
        .unwrap()
        .into_iter()
        .find(|x| x.id == id)
        .unwrap();
    assert_eq!((x.ticker.as_str(), x.currency.as_str()), ("DEMO.DE", "EUR"));
    assert_eq!(x.kind, Clase::Etf);
    // Con el ISIN de uno que ya existe, es ése.
    let con_isin = Valor {
        isin: "US0378331005".into(),
        ticker: "OTRO".into(),
        ..nuevo.clone()
    };
    let a = guardar_valor(&v, &con_isin, NOW).unwrap();
    let b = guardar_valor(
        &v,
        &Valor {
            ticker: "OTRO2".into(),
            ..con_isin
        },
        NOW,
    )
    .unwrap();
    assert_eq!(a, b);
}

/// Una cuenta de XTB en dólares: al importar, los importes se pasan a euros con el tipo que haya
/// del día (el del bróker o el del BCE ya descargado), no se copian como si fueran euros.
#[test]
fn xtb_en_dolares_se_pasa_a_euros_al_importar() {
    let v = vault(false);
    poner_cambio(
        v.connection(),
        date!(2025 - 03 - 01),
        "USD",
        Precio::parse("0.9").unwrap(),
        "manual",
    )
    .unwrap();
    let texto = xtb::tests::INFORME.replace("12345678 EUR", "12345678 USD");
    let e = xtb::leer(&texto).unwrap();
    importar::aplicar(&v, &[e], "xtb-pdf", "h", NOW).unwrap();
    let dep = ops(&v, None)
        .unwrap()
        .into_iter()
        .find(|o| o.kind == TipoOp::Deposito)
        .unwrap();
    assert_eq!(dep.currency, "USD");
    assert_eq!(dep.fx, Precio::parse("0.9"));
    assert_eq!(dep.eur, Money::from_euros(9_900, 0));
}

/// La cuota española que limita la deducción del art. 31 LIS va al tipo de la sociedad (el
/// régimen de «Datos de los modelos»), no siempre al 25 %.
#[test]
fn la_doble_imposicion_usa_el_tipo_de_la_sociedad() {
    let v = vault(false);
    let mut ms = crate::models::settings(&v).unwrap();
    ms.corporate_regime = taxpain_models::m200::TaxRegime::NuevaCreacion;
    crate::models::save_settings(&v, &ms, NOW).unwrap();
    importar::aplicar(&v, &flex(), "api", "", NOW).unwrap();
    let r = report(&v, 2026).unwrap();
    assert_eq!(r.doble_imposicion.tipo_pct, 15);
    // Dividendo de 18,40 € con 2,76 € retenidos en EE. UU.: la cuota al 15 % es 2,76 €.
    let us = &r.doble_imposicion.paises[0];
    assert_eq!(us.cuota_espanola, Money::from_cents(276));
    let mut ms = crate::models::settings(&v).unwrap();
    ms.corporate_regime = taxpain_models::m200::TaxRegime::ReducidaDimension;
    crate::models::save_settings(&v, &ms, NOW).unwrap();
    assert_eq!(report(&v, 2026).unwrap().doble_imposicion.tipo_pct, 23);
}

/// Una retención escrita a mano sin país sobre un dividendo de un valor español es española:
/// 473 en el asiento, en las retenciones españolas del resumen y fuera de la doble imposición.
#[test]
fn retencion_sin_pais_de_un_valor_espanol_es_espanola() {
    let v = vault(false);
    importar::aplicar(&v, &flex(), "api", "", NOW).unwrap();
    let cuenta_id = cuentas(&v).unwrap()[0].id;
    let sid = guardar_valor(
        &v,
        &Valor {
            id: 0,
            isin: "ES0113900J37".into(),
            ticker: "BANCO".into(),
            name: "BANCO DEMO".into(),
            currency: "EUR".into(),
            kind: Clase::Accion,
            country: String::new(),
            account: String::new(),
        },
        NOW,
    )
    .unwrap();
    let mut op = Op {
        id: 0,
        account_id: cuenta_id,
        to_account_id: None,
        security_id: Some(sid),
        date: date!(2026 - 06 - 01),
        time: "10:00:00".into(),
        kind: TipoOp::Dividendo,
        qty: Qty::ZERO,
        price: None,
        currency: "EUR".into(),
        amount: Money::from_euros(100, 0),
        fee: Money::ZERO,
        fx: None,
        eur: Money::ZERO,
        fee_eur: Money::ZERO,
        country: String::new(),
        note: String::new(),
        source: "manual".into(),
        fingerprint: None,
        entry_id: None,
    };
    guardar_op(&v, &op, NOW).unwrap();
    op.kind = TipoOp::Retencion;
    op.amount = Money::from_euros(19, 0);
    let rid = guardar_op(&v, &op, NOW).unwrap();
    let r = report(&v, 2026).unwrap();
    assert_eq!(r.resultado.retenciones_espana, Money::from_euros(19, 0));
    assert!(r.doble_imposicion.paises.iter().all(|p| p.pais != "ES"));
    let a = r.asientos.iter().find(|a| a.op == Some(rid)).unwrap();
    assert!(a.lineas.iter().any(|l| l.cuenta == "473"), "{a:?}");
}

/// Cartera de negociación en años sucesivos: el ajuste a valor razonable de cada cierre es la
/// variación desde el cierre anterior (no otra vez desde el coste) y la venta se compara con el
/// valor en libros.
#[test]
fn negociacion_en_anos_sucesivos_parte_del_valor_razonable_anterior() {
    let v = vault(false);
    let mut s = settings(&v).unwrap();
    s.cartera = Cartera::Negociacion;
    save_settings(&v, &s, NOW).unwrap();
    importar::aplicar(&v, &flex(), "api", "", NOW).unwrap();
    // ETFX: quedan 12 a 31/12/2026 a 56 € (672 €); coste 12 × 50 = 600 €.
    let r = report(&v, 2026).unwrap();
    let etfx = r.posiciones.iter().find(|p| p.ticker == "ETFX").unwrap();
    assert_eq!(etfx.latente, Some(Money::from_euros(72, 0)));
    // 2027: a 60 € el 31/12/2027 sin moverse: el ajuste es 12 × 4 = 48 €, no 120 €.
    guardar_precios(
        &v,
        &[PrecioCierre {
            date: date!(2027 - 12 - 31),
            security_id: etfx.security_id,
            price: Precio::parse("60").unwrap(),
            currency: "EUR".into(),
            source: "manual".into(),
        }],
        NOW,
    )
    .unwrap();
    let r = report(&v, 2027).unwrap();
    let etfx = r.posiciones.iter().find(|p| p.ticker == "ETFX").unwrap();
    assert_eq!(etfx.latente, Some(Money::from_euros(48, 0)));
    let ajuste = r
        .asientos
        .iter()
        .find(|a| a.concepto.contains("ETFX"))
        .unwrap();
    assert!(
        ajuste
            .lineas
            .iter()
            .any(|l| l.cuenta == "7630" && l.haber == Money::from_euros(48, 0)),
        "{ajuste:?}"
    );
}

/// Un PDF de XTB y un Flex de IBKR importados a la vez: cada cuenta dice lo suyo.
#[test]
fn importar_xtb_e_ibkr_a_la_vez_etiqueta_cada_cuenta() {
    let v = vault(false);
    let mut ex = xtb();
    ex.extend(flex());
    importar::aplicar(&v, &ex, "xtb-pdf", "", NOW).unwrap();
    let cs = cuentas(&v).unwrap();
    let ibkr = cs.iter().find(|c| c.broker == Broker::Ibkr).unwrap();
    assert!(
        ibkr.last_sync_note.starts_with("Fichero Flex"),
        "{}",
        ibkr.last_sync_note
    );
    let xtb = cs.iter().find(|c| c.broker == Broker::Xtb).unwrap();
    assert!(xtb.last_sync_note.starts_with("Informe de XTB"));
}

/// Un precio de cierre puesto a mano no lo pisa el del bróker: el mismo fichero otra vez no debe
/// seguir contando ese precio como pendiente (la carpeta lo daría siempre por importable).
#[test]
fn un_precio_a_mano_cuenta_como_ya_guardado() {
    let v = vault(false);
    importar::aplicar(&v, &xtb(), "xtb-pdf", "h1", NOW).unwrap();
    let sid = valores(&v)
        .unwrap()
        .into_iter()
        .find(|x| x.ticker == "MMKT.DE")
        .unwrap()
        .id;
    guardar_precios(
        &v,
        &[PrecioCierre {
            date: date!(2026 - 01 - 15),
            security_id: sid,
            price: Precio::parse("107").unwrap(),
            currency: "EUR".into(),
            source: "manual".into(),
        }],
        NOW,
    )
    .unwrap();
    let mut l = importar::Lectura {
        ficheros: Vec::new(),
        extractos: xtb(),
        nuevas: 0,
        duplicadas: 0,
        precios: 0,
        avisos: Vec::new(),
    };
    importar::contar(&v, &mut l).unwrap();
    assert_eq!((l.nuevas, l.precios), (0, 0));
}

fn xlsx_usd(sec_fee: bool) -> Vec<Extracto> {
    let bytes = xtb_xlsx::tests::usd_con(sec_fee);
    vec![xtb_xlsx::leer("USD_99000001_2026-01-01_2026-10-31.xlsx", &bytes).unwrap()]
}

/// Tipos del BCE de prueba: 1,25 dólares por euro todos los días de un periodo.
fn bce_fijo(v: &Vault) -> usize {
    let mut d = date!(2025 - 09 - 01);
    let mut t = Vec::new();
    while d <= date!(2026 - 11 - 30) {
        t.push((d, taxpain_core::fx::ExchangeRate::parse("1.25").unwrap()));
        d = d.next_day().unwrap();
    }
    aplicar_bce(v, "USD", &t, "prueba", NOW).unwrap()
}

/// El Excel de una cuenta de XTB en dólares: sin el tipo del BCE el informe avisa y no inventa;
/// con él, la venta (con la SEC fee al coste), la compra de antes del periodo, la opción que
/// vence sin valor (línea de derivados), intereses y retención. Dos veces, nada duplicado.
#[test]
fn excel_de_xtb_en_dolares_con_opcion_y_cambio_del_bce() {
    let v = vault(false);
    let a = importar::aplicar(&v, &xlsx_usd(true), "xtb-pdf", "", NOW).unwrap();
    assert_eq!(a.nuevas, 10, "{a:?}");
    assert!(
        a.avisos
            .iter()
            .any(|x| x.contains("Falta el informe anterior con la compra de OLDCO.US")),
        "{:?}",
        a.avisos
    );
    // El cuadre con las posiciones abiertas del informe sale bien.
    assert!(
        !a.avisos.iter().any(|x| x.contains("el bróker dice")),
        "{:?}",
        a.avisos
    );
    let c = &cuentas(&v).unwrap()[0];
    assert_eq!(
        (c.number.as_str(), c.currency.as_str()),
        ("99000001", "USD")
    );
    // Sin cambio: avisa y no cuenta nada en euros.
    let r = report(&v, 2026).unwrap();
    assert!(r.sin_euros > 0);
    assert!(
        r.avisos
            .iter()
            .any(|x| x.contains("Pasar a euros con el BCE"))
    );
    assert_eq!(pendientes_bce(&v).unwrap()[0].0, "USD");
    assert_eq!(
        pendientes_bce(&v).unwrap()[0].1,
        date!(2025 - 10 - 01),
        "desde la compra de antes del periodo"
    );
    // Con el tipo del BCE (0,8 € por dólar).
    assert_eq!(bce_fijo(&v), 9);
    assert!(pendientes_bce(&v).unwrap().is_empty());
    let r = report(&v, 2026).unwrap();
    assert_eq!(r.sin_euros, 0);
    let res = |t: &str| {
        r.realizados
            .iter()
            .filter(|x| x.ticker == t)
            .map(|x| x.resultado)
            .sum::<Money>()
    };
    // ACME: 800 € de coste; 1.200 $ − 0,05 $ de SEC fee = 959,96 €.
    assert_eq!(res("ACME.US"), Money::from_cents(15_996));
    // OLDCO: comprada en 2025 por 450 $ (360 €), vendida por 550 $ (440 €).
    assert_eq!(res("OLDCO.US"), Money::from_euros(80, 0));
    assert_eq!(r.resultado.ventas, 2);
    assert_eq!(r.resultado.neto, Money::from_cents(23_996));
    // La opción: 60 $ de prima (48 €) que vencen sin valor, aparte.
    assert_eq!(r.resultado.ventas_derivados, 1);
    assert_eq!(r.resultado.derivados, Money::from_euros(-48, 0));
    assert!(r.realizados.iter().any(|x| x.derivado));
    assert_eq!(r.resultado.intereses, Money::from_euros(8, 0));
    assert_eq!(r.resultado.retenciones_espana, Money::from_cents(152));
    assert_eq!(r.resultado.comisiones, Money::ZERO);
    // La posición abierta, con el precio del informe (en dólares).
    assert_eq!(r.posiciones.len(), 1);
    assert_eq!(r.posiciones[0].ticker, "BETA.US");
    assert_eq!(r.posiciones[0].cantidad, Qty::from_units(5));
    assert_eq!(r.posiciones[0].coste, Money::from_euros(400, 0));
    // Las opciones no van al 720.
    assert!(
        r.m720
            .cuentas
            .iter()
            .all(|c| c.valores.iter().all(|(t, ..)| !t.contains("_P_")))
    );
    // El asiento del vencimiento: la pérdida a 6630 contra la 5590.
    let venc = r
        .asientos
        .iter()
        .find(|x| x.concepto.contains("Vencimiento sin valor"))
        .unwrap();
    assert!(
        venc.lineas
            .iter()
            .any(|l| l.cuenta == "6630" && l.debe == Money::from_euros(48, 0))
    );
    assert!(venc.lineas.iter().any(|l| l.cuenta.starts_with("5590")));
    for x in &r.asientos {
        let d: Money = x.lineas.iter().map(|l| l.debe).sum();
        let h: Money = x.lineas.iter().map(|l| l.haber).sum();
        assert_eq!(d, h, "{x:?}");
    }
    // Otra vez el mismo Excel: nada nuevo.
    let b = importar::aplicar(&v, &xlsx_usd(true), "xtb-pdf", "", NOW).unwrap();
    assert_eq!((b.nuevas, b.duplicadas), (0, 10));
    assert_eq!(
        report(&v, 2026).unwrap().resultado.neto,
        Money::from_cents(23_996)
    );
}

/// La SEC fee llega en un informe posterior al de la venta: se suma a la venta ya guardada (y
/// no entra otra vez si se importa de nuevo).
#[test]
fn la_sec_fee_que_llega_despues_se_suma_a_la_venta() {
    let v = vault(false);
    bce_fijo(&v);
    importar::aplicar(&v, &xlsx_usd(false), "xtb-pdf", "", NOW).unwrap();
    let venta = |v: &Vault| {
        ops(v, None)
            .unwrap()
            .into_iter()
            .find(|o| o.fingerprint.as_deref() == Some("xtb:99000001:1002"))
            .unwrap()
    };
    assert!(venta(&v).fee.is_zero());
    importar::aplicar(&v, &xlsx_usd(true), "xtb-pdf", "", NOW).unwrap();
    let o = venta(&v);
    assert_eq!(
        (o.fee, o.fee_eur),
        (Money::from_cents(5), Money::from_cents(4))
    );
    importar::aplicar(&v, &xlsx_usd(true), "xtb-pdf", "", NOW).unwrap();
    assert_eq!(venta(&v).fee, Money::from_cents(5));
    assert_eq!(
        ops(&v, None)
            .unwrap()
            .iter()
            .filter(|o| o.kind == TipoOp::Comision)
            .count(),
        0
    );
}

/// El Excel de la cuenta en euros vende lo comprado en el PDF del año anterior: la compra no
/// se duplica (la de «Closed Positions» es la misma que la del PDF) y el resultado es el de
/// XTB. Al revés (primero el Excel, luego el PDF), la del PDF sustituye a la de la posición.
#[test]
fn excel_que_vende_lo_comprado_en_el_pdf_anterior() {
    use xtb_xlsx::tests::C::{N, S, T};
    let my = S("My Trades");
    let libro = xtb_xlsx::tests::libro(
        "12345678",
        "2025-12-31 23:00:00",
        "2026-03-01 10:00:00",
        &[vec![
            S("Monetario"),
            S("MMKT.DE"),
            S("ETF"),
            S("BUY"),
            N(100.0),
            N(105.0),
            // 11:00 en Madrid (verano) en el PDF.
            T("2025-07-01 09:00:00"),
            N(110.0),
            T("2026-02-10 10:00:00"),
            my,
            N(500.0),
            N(500.0),
            N(10_500.0),
            N(11_000.0),
            S(""),
            S(""),
            N(0.0),
            S(""),
            S(""),
            S(""),
            N(1.0),
            N(1.0),
            S("xStation5"),
            N(5_000_000_009.0),
            S(""),
        ]],
        &[vec![
            S("Stock sell"),
            S("Monetario"),
            S("MMKT.DE"),
            S("ETF"),
            T("2026-02-10 10:00:00"),
            N(11_000.0),
            S("800000001"),
            S("CLOSE BUY 100 @ 110.00"),
            my,
            S("5000000009"),
        ]],
        &[],
    );
    let excel = || vec![xtb_xlsx::leer("EUR_12345678_2025-12-31_2026-03-01.xlsx", &libro).unwrap()];
    let v = vault(false);
    importar::aplicar(&v, &xtb(), "xtb-pdf", "h1", NOW).unwrap();
    let mut l = importar::leer(
        &v,
        &[(
            "EUR_12345678_2025-12-31_2026-03-01.xlsx".into(),
            libro.clone(),
        )],
    )
    .unwrap();
    assert_eq!(l.ficheros[0].formato, "XTB (Excel)");
    assert_eq!((l.nuevas, l.duplicadas), (1, 1), "{l:?}");
    importar::contar(&v, &mut l).unwrap();
    assert!(
        !l.avisos.iter().any(|a| a.contains("Falta el informe")),
        "{:?}",
        l.avisos
    );
    let a = importar::aplicar(&v, &excel(), "xtb-pdf", "", NOW).unwrap();
    assert_eq!((a.nuevas, a.duplicadas), (1, 1));
    let r = report(&v, 2026).unwrap();
    assert_eq!(r.resultado.neto, Money::from_euros(500, 0));
    assert!(r.posiciones.is_empty());

    // Al revés: el Excel primero (la compra, de la posición) y luego el PDF.
    let v = vault(false);
    let a = importar::aplicar(&v, &excel(), "xtb-pdf", "", NOW).unwrap();
    assert_eq!(a.nuevas, 2);
    assert!(
        a.avisos
            .iter()
            .any(|x| x.contains("Falta el informe anterior"))
    );
    let b = importar::aplicar(&v, &xtb(), "xtb-pdf", "h1", NOW).unwrap();
    assert_eq!(b.sustituidas, 1, "{b:?}");
    let compras = ops(&v, None)
        .unwrap()
        .into_iter()
        .filter(|o| o.kind == TipoOp::Compra && o.qty == Qty::from_units(100))
        .collect::<Vec<_>>();
    assert_eq!(compras.len(), 1);
    assert_eq!(compras[0].source, "xtb");
    assert_eq!(
        report(&v, 2026).unwrap().resultado.neto,
        Money::from_euros(500, 0)
    );
    // Y el Excel otra vez ya no mete la compra de la posición.
    let c = importar::aplicar(&v, &excel(), "xtb-pdf", "", NOW).unwrap();
    assert_eq!(c.nuevas, 0);
}

#[test]
fn la_carpeta_manda_el_excel_de_xtb_a_valores() {
    use crate::carpeta::{self, Found};
    let v = vault(false);
    let dir = tempfile::tempdir().unwrap();
    let inv = dir.path().join("05_INVERSIONES").join("99000001");
    std::fs::create_dir_all(&inv).unwrap();
    std::fs::write(
        inv.join("USD_99000001_2026-01-01_2026-10-31.xlsx"),
        xtb_xlsx::tests::usd(),
    )
    .unwrap();
    let scan = carpeta::scan(&v, dir.path()).unwrap();
    assert_eq!(scan.items.len(), 1);
    let i = &scan.items[0];
    match &i.found {
        Found::Valores {
            formato,
            cuenta,
            ops,
            duplicates,
            ..
        } => {
            assert_eq!(formato, "XTB (Excel)");
            assert_eq!((cuenta.as_str(), *ops, *duplicates), ("99000001", 10, 0));
        }
        otro => panic!("no es de bróker: {otro:?}"),
    }
    assert!(i.importable);
    let res = carpeta::apply(&v, dir.path(), std::slice::from_ref(&i.path), NOW).unwrap();
    let mensajes: Vec<String> = res.iter().map(|o| format!("{o:?}")).collect();
    assert!(
        mensajes
            .iter()
            .any(|m| m.contains("10 operaciones a Inversiones en valores")),
        "{mensajes:?}"
    );
    let scan = carpeta::scan(&v, dir.path()).unwrap();
    assert!(scan.items.iter().all(|i| !i.importable), "{:?}", scan.items);
}

/// El cierre de un ejercicio que aún no ha terminado no tiene tipo del BCE: se usa el último
/// conocido, con su fecha. Un cierre pasado sin tipo sigue sin él.
#[test]
fn cierre_futuro_con_el_ultimo_cambio_conocido() {
    let v = vault(false);
    let ayer = OffsetDateTime::now_utc().date() - time::Duration::days(1);
    poner_cambio(
        v.connection(),
        ayer,
        "USD",
        Precio::parse("0.9").unwrap(),
        "bce",
    )
    .unwrap();
    let futuro = ayer + time::Duration::days(400);
    assert_eq!(
        cambio_a(&v, "USD", futuro).unwrap(),
        Some((ayer, Precio::parse("0.9").unwrap()))
    );
    assert_eq!(cambio_a(&v, "USD", date!(2020 - 12 - 31)).unwrap(), None);
}

/// Agrupación 1 por 10 de IBKR (tipo RS, dos filas: salen 105 títulos viejos y entran 10) con
/// 6 € en efectivo por la fracción de 0,5: el coste de 1.050 € no se pierde (1.000 € en los 10
/// títulos nuevos) y la fracción es una venta con 50 € de coste. Datos inventados.
#[test]
fn ibkr_agrupacion_uno_por_diez_conserva_el_coste() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<FlexQueryResponse queryName="TaxPain" type="AF">
<FlexStatements count="1">
<FlexStatement accountId="U0000002" fromDate="20260101" toDate="20261231" period="Year" whenGenerated="20270102;090000">
<AccountInformation accountId="U0000002" currency="EUR" name="EMPRESA DEMO SL" />
<Trades>
<Trade accountId="U0000002" currency="EUR" fxRateToBase="1" assetCategory="STK" subCategory="COMMON" symbol="DEMO" description="DEMO SA" isin="ES0000000002" issuerCountryCode="ES" tradeID="11" transactionID="21" tradeDate="20260310" dateTime="20260310;153000" quantity="105" tradePrice="10" tradeMoney="1050" ibCommission="0" ibCommissionCurrency="EUR" buySell="BUY" levelOfDetail="EXECUTION" />
</Trades>
<CorporateActions>
<CorporateAction accountId="U0000002" currency="EUR" assetCategory="STK" symbol="DEMO" isin="ES0000000002" reportDate="20260801" dateTime="20260801;202500" quantity="-105" proceeds="0" type="RS" actionID="77" transactionID="41" description="DEMO(ES0000000002) SPLIT 1 FOR 10 (DEMO, DEMO SA, ES0000000002)" />
<CorporateAction accountId="U0000002" currency="EUR" assetCategory="STK" symbol="DEMO" isin="ES0000000002" reportDate="20260801" dateTime="20260801;202500" quantity="10" proceeds="6" type="RS" actionID="77" transactionID="42" description="DEMO(ES0000000002) SPLIT 1 FOR 10 (DEMO, DEMO SA, ES0000000002)" />
</CorporateActions>
</FlexStatement>
</FlexStatements>
</FlexQueryResponse>"#;
    let v = vault(false);
    let ex = flex::leer_xml(xml).unwrap();
    let a = importar::aplicar(&v, &ex, "api", "", NOW).unwrap();
    assert_eq!(a.nuevas, 4, "{:?}", a.avisos);
    let r = report(&v, 2026).unwrap();
    assert_eq!(r.posiciones.len(), 1);
    assert_eq!(
        (r.posiciones[0].cantidad, r.posiciones[0].coste),
        (Qty::from_units(10), Money::from_euros(1_000, 0))
    );
    assert_eq!(r.realizados.len(), 1);
    let f = &r.realizados[0];
    assert_eq!(
        (f.cantidad, f.transmision, f.coste, f.resultado),
        (
            Qty::parse("0.5").unwrap(),
            Money::from_euros(6, 0),
            Money::from_euros(50, 0),
            Money::from_cents(-4_400)
        )
    );
}

/// Flex inventado con 105 títulos de DEMO comprados por 1.050 €, la agrupación 1 por 10 (RS,
/// dos filas) con `proceeds` en la fila de los nuevos, y más filas de Trade (`trades`) o de
/// CashTransaction (`efectivo`) con la fracción en efectivo.
fn flex_agrupacion(proceeds: &str, trades: &str, efectivo: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<FlexQueryResponse queryName="TaxPain" type="AF">
<FlexStatements count="1">
<FlexStatement accountId="U0000002" fromDate="20260101" toDate="20261231" period="Year" whenGenerated="20270102;090000">
<AccountInformation accountId="U0000002" currency="EUR" name="EMPRESA DEMO SL" />
<Trades>
<Trade accountId="U0000002" currency="EUR" fxRateToBase="1" assetCategory="STK" subCategory="COMMON" symbol="DEMO" description="DEMO SA" isin="ES0000000002" issuerCountryCode="ES" tradeID="11" transactionID="21" tradeDate="20260310" dateTime="20260310;153000" quantity="105" tradePrice="10" tradeMoney="1050" ibCommission="0" ibCommissionCurrency="EUR" buySell="BUY" levelOfDetail="EXECUTION" />
{trades}
</Trades>
<CashTransactions>
{efectivo}
</CashTransactions>
<CorporateActions>
<CorporateAction accountId="U0000002" currency="EUR" assetCategory="STK" symbol="DEMO" isin="ES0000000002" reportDate="20260801" dateTime="20260801;202500" quantity="-105" proceeds="0" type="RS" actionID="77" transactionID="41" description="DEMO(ES0000000002) SPLIT 1 FOR 10 (DEMO, DEMO SA, ES0000000002)" />
<CorporateAction accountId="U0000002" currency="EUR" assetCategory="STK" symbol="DEMO" isin="ES0000000002" reportDate="20260801" dateTime="20260801;202500" quantity="10" proceeds="{proceeds}" type="RS" actionID="77" transactionID="42" description="DEMO(ES0000000002) SPLIT 1 FOR 10 (DEMO, DEMO SA, ES0000000002)" />
</CorporateActions>
</FlexStatement>
</FlexStatements>
</FlexQueryResponse>"#
    )
}

/// Lo que debe quedar tras la agrupación: 10 títulos con 1.000 € de coste y una sola venta de
/// la fracción (0,5 títulos por 6 € con 50 € de coste).
fn una_sola_venta_de_la_fraccion(v: &Vault) {
    let r = report(v, 2026).unwrap();
    assert_eq!(r.posiciones.len(), 1);
    assert_eq!(
        (r.posiciones[0].cantidad, r.posiciones[0].coste),
        (Qty::from_units(10), Money::from_euros(1_000, 0))
    );
    assert_eq!(r.realizados.len(), 1, "{:?}", r.realizados);
    let f = &r.realizados[0];
    assert_eq!(
        (f.cantidad, f.transmision, f.coste),
        (
            Qty::parse("0.5").unwrap(),
            Money::from_euros(6, 0),
            Money::from_euros(50, 0),
        )
    );
}

/// Si IBKR da también la fracción como un Trade de venta (mismo valor y día), esa es la venta:
/// no se crea otra con el `proceeds` de la fila RS, y va detrás de la agrupación (aunque su hora
/// sea anterior) para vender títulos nuevos. TODO(VERIFY): formato sin contrastar con un Flex
/// real. Datos inventados.
#[test]
fn ibkr_agrupacion_con_trade_de_la_fraccion_no_la_cuenta_dos_veces() {
    let trade = r#"<Trade accountId="U0000002" currency="EUR" fxRateToBase="1" assetCategory="STK" subCategory="COMMON" symbol="DEMO" description="DEMO SA" isin="ES0000000002" issuerCountryCode="ES" tradeID="12" transactionID="22" tradeDate="20260801" dateTime="20260801;160000" quantity="-0.5" tradePrice="12" tradeMoney="-6" proceeds="6" ibCommission="0" ibCommissionCurrency="EUR" buySell="SELL" levelOfDetail="EXECUTION" />"#;
    let v = vault(false);
    let ex = flex::leer_xml(&flex_agrupacion("6", trade, "")).unwrap();
    let ventas: Vec<&importar::OpLeida> = ex[0]
        .ops
        .iter()
        .filter(|o| o.tipo == TipoOp::Venta)
        .collect();
    assert_eq!(ventas.len(), 1, "{:?}", ex[0].ops);
    assert_eq!(ventas[0].hora, "20:25:00", "detrás de la agrupación");
    let a = importar::aplicar(&v, &ex, "api", "", NOW).unwrap();
    assert_eq!(a.nuevas, 4, "{:?}", a.avisos);
    una_sola_venta_de_la_fraccion(&v);
}

/// Agrupación 1 por 3 (100 títulos dan 33,333…): la fracción calculada lleva 18 decimales y el
/// Trade de IBKR sólo 4 (0,3333). Casa igual, así que hay una sola venta de la fracción aunque
/// la fila RS traiga también `proceeds`. TODO(VERIFY): formato sin contrastar con un Flex real.
/// Datos inventados.
#[test]
fn ibkr_agrupacion_uno_por_tres_casa_el_trade_de_la_fraccion() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<FlexQueryResponse queryName="TaxPain" type="AF">
<FlexStatements count="1">
<FlexStatement accountId="U0000002" fromDate="20260101" toDate="20261231" period="Year" whenGenerated="20270102;090000">
<AccountInformation accountId="U0000002" currency="EUR" name="EMPRESA DEMO SL" />
<Trades>
<Trade accountId="U0000002" currency="EUR" fxRateToBase="1" assetCategory="STK" subCategory="COMMON" symbol="DEMO" description="DEMO SA" isin="ES0000000002" issuerCountryCode="ES" tradeID="11" transactionID="21" tradeDate="20260310" dateTime="20260310;153000" quantity="100" tradePrice="10" tradeMoney="1000" ibCommission="0" ibCommissionCurrency="EUR" buySell="BUY" levelOfDetail="EXECUTION" />
<Trade accountId="U0000002" currency="EUR" fxRateToBase="1" assetCategory="STK" subCategory="COMMON" symbol="DEMO" description="DEMO SA" isin="ES0000000002" issuerCountryCode="ES" tradeID="12" transactionID="22" tradeDate="20260801" dateTime="20260801;160000" quantity="-0.3333" tradePrice="12" tradeMoney="-4" proceeds="4" ibCommission="0" ibCommissionCurrency="EUR" buySell="SELL" levelOfDetail="EXECUTION" />
</Trades>
<CorporateActions>
<CorporateAction accountId="U0000002" currency="EUR" assetCategory="STK" symbol="DEMO" isin="ES0000000002" reportDate="20260801" dateTime="20260801;202500" quantity="-100" proceeds="0" type="RS" actionID="78" transactionID="41" description="DEMO(ES0000000002) SPLIT 1 FOR 3 (DEMO, DEMO SA, ES0000000002)" />
<CorporateAction accountId="U0000002" currency="EUR" assetCategory="STK" symbol="DEMO" isin="ES0000000002" reportDate="20260801" dateTime="20260801;202500" quantity="33" proceeds="4" type="RS" actionID="78" transactionID="42" description="DEMO(ES0000000002) SPLIT 1 FOR 3 (DEMO, DEMO SA, ES0000000002)" />
</CorporateActions>
</FlexStatement>
</FlexStatements>
</FlexQueryResponse>"#;
    let ex = flex::leer_xml(xml).unwrap();
    let ventas: Vec<&importar::OpLeida> = ex[0]
        .ops
        .iter()
        .filter(|o| o.tipo == TipoOp::Venta)
        .collect();
    assert_eq!(ventas.len(), 1, "{:?}", ex[0].ops);
    assert_eq!(ventas[0].cantidad, Qty::parse("0.3333").unwrap());
    assert_eq!(ventas[0].hora, "20:25:00", "detrás de la agrupación");
    let v = vault(false);
    let a = importar::aplicar(&v, &ex, "api", "", NOW).unwrap();
    assert_eq!(a.nuevas, 4, "{:?}", a.avisos);
    let r = report(&v, 2026).unwrap();
    assert_eq!(r.posiciones.len(), 1);
    assert_eq!(r.posiciones[0].cantidad, Qty::from_units(33));
    assert_eq!(r.realizados.len(), 1, "{:?}", r.realizados);
    let f = &r.realizados[0];
    assert_eq!(
        (f.cantidad, f.transmision),
        (Qty::parse("0.3333").unwrap(), Money::from_euros(4, 0))
    );
    assert_eq!(
        r.posiciones[0].coste + f.coste,
        Money::from_euros(1_000, 0),
        "el coste no se pierde"
    );
}

/// Si la fracción viene como un CashTransaction «cash in lieu» del mismo valor y día, no se
/// importa aparte: con `proceeds` en la fila RS hay una sola venta; sin él, la venta lleva el
/// importe del CashTransaction. TODO(VERIFY): formato sin contrastar con un Flex real. Datos
/// inventados.
#[test]
fn ibkr_agrupacion_con_efectivo_aparte_no_lo_cuenta_dos_veces() {
    let efectivo = r#"<CashTransaction accountId="U0000002" currency="EUR" fxRateToBase="1" assetCategory="STK" symbol="DEMO" isin="ES0000000002" dateTime="20260801;202500" amount="6" type="Other" transactionID="51" description="DEMO(ES0000000002) CASH IN LIEU OF FRACTIONAL SHARES" />"#;
    for proceeds in ["6", "0"] {
        let v = vault(false);
        let ex = flex::leer_xml(&flex_agrupacion(proceeds, "", efectivo)).unwrap();
        assert!(
            !ex[0].avisos.iter().any(|a| a.contains("no se reconocen")),
            "{:?}",
            ex[0].avisos
        );
        let a = importar::aplicar(&v, &ex, "api", "", NOW).unwrap();
        assert_eq!(a.nuevas, 4, "proceeds={proceeds}: {:?}", a.avisos);
        una_sola_venta_de_la_fraccion(&v);
    }
}
