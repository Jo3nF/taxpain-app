//! Conexión de solo lectura con los bancos de punta a punta, sin red: ajustes y clave de la
//! aplicación, autorización pendiente, sesión con sus cuentas, descargas que se solapan (sin
//! duplicar, por el identificador del banco), un extracto en fichero de la misma cuenta, un
//! permiso caducado, la renovación del permiso y la desconexión. El banco es un doble con datos
//! fijos e inventados.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::Engine;
use taxpain_app::banca::{self, Ajustes, Api};
use taxpain_app::bank;
use taxpain_app::extractos::importar::{comprobado, importar};
use taxpain_app::extractos::{Clase, Extracto, Movimiento};
use taxpain_banca::BancaError;
use taxpain_banca::enable::{Saldo, Sesion, Transaccion};
use taxpain_core::money::Money;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime};

const IBAN: &str = "ES9121000418450200051332";
const AHORA: OffsetDateTime = datetime!(2026-10-30 10:00 UTC);
const APP: &str = "0a1b2c3d-0000-4000-8000-00000000a1b2";

fn vault() -> Vault {
    Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap()
}

/// Clave sintética de la aplicación en PEM (fixtures/certs/rsa-app-banca.der).
fn clave_pem() -> String {
    let der = std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/certs/rsa-app-banca.der"),
    )
    .unwrap();
    let b64 = base64::engine::general_purpose::STANDARD.encode(der);
    let tipo = ["PRIVATE", "KEY"].join(" ");
    let lineas: Vec<String> = b64
        .as_bytes()
        .chunks(64)
        .map(|c| String::from_utf8(c.to_vec()).unwrap())
        .collect();
    format!(
        "-----BEGIN {tipo}-----\n{}\n-----END {tipo}-----\n",
        lineas.join("\n")
    )
}

fn sesion(id: &str, uids: [&str; 2]) -> Sesion {
    serde_json::from_value(serde_json::json!({
        "session_id": id,
        "accounts": [
            {
                "uid": uids[0],
                "account_id": { "iban": IBAN },
                "name": "EMPRESA DEMO SL",
                "currency": "EUR",
                "cash_account_type": "CACC",
                "identification_hash": "huella-cuenta"
            },
            {
                "uid": uids[1],
                "account_id": { "other": { "identification": "4940********1234", "scheme_name": "CPAN" } },
                "name": "Tarjeta empresa",
                "currency": "EUR",
                "cash_account_type": "CARD",
                "identification_hash": "huella-tarjeta"
            }
        ],
        "aspsp": { "name": "BBVA", "country": "ES" },
        "psu_type": "business",
        "access": { "valid_until": "2027-04-28T09:00:00+00:00" }
    }))
    .unwrap()
}

fn tx(id: &str, dia: &str, importe: &str, signo: &str, concepto: &str) -> Transaccion {
    serde_json::from_value(serde_json::json!({
        // El identificador estable es la referencia de la entrada; el transaction_id cambia.
        "entry_reference": id,
        "transaction_id": format!("cambia-{id}-{dia}"),
        "transaction_amount": { "currency": "EUR", "amount": importe },
        "credit_debit_indicator": signo,
        "status": "BOOK",
        "booking_date": dia,
        "value_date": dia,
        "remittance_information": [concepto]
    }))
    .unwrap()
}

/// El banco: movimientos fijos por cuenta, y lo que se le ha pedido.
#[derive(Default)]
struct Banco {
    movimientos: Mutex<Vec<(String, Vec<Transaccion>)>>,
    caducadas: Vec<String>,
    pedidos: Mutex<Vec<(String, Option<Date>)>>,
}

impl Banco {
    fn poner(&self, uid: &str, txs: Vec<Transaccion>) {
        let mut m = self.movimientos.lock().unwrap();
        m.retain(|(u, _)| u != uid);
        m.push((uid.into(), txs));
    }
}

impl Api for Banco {
    fn movimientos(
        &self,
        uid: &str,
        desde: Option<Date>,
    ) -> impl Future<Output = taxpain_banca::Result<Vec<Transaccion>>> + Send {
        self.pedidos.lock().unwrap().push((uid.into(), desde));
        let r = if self.caducadas.iter().any(|c| c == uid) {
            Err(BancaError::Caducado)
        } else {
            Ok(self
                .movimientos
                .lock()
                .unwrap()
                .iter()
                .find(|(u, _)| u == uid)
                .map(|(_, t)| t.clone())
                .unwrap_or_default())
        };
        std::future::ready(r)
    }

    fn saldos(&self, _uid: &str) -> impl Future<Output = taxpain_banca::Result<Vec<Saldo>>> + Send {
        std::future::ready(Ok(vec![
            serde_json::from_value(serde_json::json!({
                "balance_amount": { "currency": "EUR", "amount": "2149.50" },
                "balance_type": "CLBD",
                "reference_date": "2026-10-29"
            }))
            .unwrap(),
        ]))
    }
}

fn bajar(v: &Vault, banco: &Banco, ahora: OffsetDateTime) -> banca::Actualizacion {
    futures_lite(banca::actualizar(|| Ok(v), banco, None, ahora)).unwrap()
}

/// Ejecuta un futuro que no espera a nada (el banco de las pruebas responde al momento).
fn futures_lite<F: Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
}

fn filas(v: &Vault) -> Vec<(String, i64, Option<String>)> {
    let mut s = v
        .connection()
        .prepare("SELECT op_date, amount, external_id FROM bank_tx ORDER BY op_date, id")
        .unwrap();
    s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Conecta BBVA (empresa) y devuelve la conexión.
fn conectar(
    v: &Vault,
    sesion_id: &str,
    uids: [&str; 2],
    ahora: OffsetDateTime,
) -> (i64, Vec<String>) {
    let p = banca::preparar(v, "BBVA", "ES", "business", Some(15_552_000), ahora).unwrap();
    let link = banca::pendiente(v, Some(&p.state), ahora).unwrap();
    let viejas = banca::guardar_sesion(v, link, &sesion(sesion_id, uids), ahora).unwrap();
    (link, viejas)
}

#[test]
fn ajustes_clave_y_autorizacion() {
    let v = vault();
    // Sin aceptar, nada.
    let e = banca::estado(&v, AHORA).unwrap();
    assert!(!e.ajustes.aceptado);
    assert_eq!(e.ajustes.url_vuelta, banca::URL_VUELTA);
    assert!(banca::firmante(&v).is_err());
    // Una dirección de vuelta que no es https ni 127.0.0.1 no se guarda.
    assert!(
        banca::guardar_ajustes(
            &v,
            &Ajustes {
                aceptado: true,
                url_vuelta: "http://ejemplo.es/vuelta".into()
            },
            AHORA
        )
        .is_err()
    );
    banca::guardar_ajustes(
        &v,
        &Ajustes {
            aceptado: true,
            url_vuelta: "http://127.0.0.1:47219/taxpain/banco".into(),
        },
        AHORA,
    )
    .unwrap();
    // Una clave que no es PEM no se guarda.
    assert!(banca::guardar_credenciales(&v, APP, "no es una clave", AHORA).is_err());
    banca::guardar_credenciales(&v, APP, &clave_pem(), AHORA).unwrap();
    let e = banca::estado(&v, AHORA).unwrap();
    assert_eq!(e.app_id.as_deref(), Some(APP));
    assert!(e.clave);
    assert!(e.vuelta_local);
    // La clave no sale nunca en el estado.
    let json = serde_json::to_string(&e).unwrap();
    assert!(!json.contains("PRIVATE"), "{json}");
    assert!(!json.contains("MII"), "{json}");
    assert_eq!(banca::firmante(&v).unwrap().kid(), APP);

    // Autorización pendiente: el permiso, como mucho el del banco (menos una hora de margen).
    let p = banca::preparar(&v, "BBVA", "ES", "business", Some(15_552_000), AHORA).unwrap();
    assert_eq!(p.state.len(), 32);
    assert_eq!(p.url_vuelta, "http://127.0.0.1:47219/taxpain/banco");
    assert_eq!(p.hasta, datetime!(2027-04-28 09:00 UTC));
    assert!(banca::estado(&v, AHORA).unwrap().pendiente.is_some());
    // Otro «state» no es esta autorización.
    assert!(banca::pendiente(&v, Some("otro"), AHORA).is_err());
    assert!(banca::pendiente(&v, None, AHORA).is_err());
    assert!(banca::pendiente(&v, Some(&p.state), AHORA).is_ok());
    // Pasada una hora, caduca.
    assert!(banca::pendiente(&v, Some(&p.state), AHORA + time::Duration::minutes(61)).is_err());
    assert!(banca::estado(&v, AHORA).unwrap().pendiente.is_none());
    // Un tipo raro no se pide.
    assert!(banca::preparar(&v, "BBVA", "ES", "otro", None, AHORA).is_err());

    // Sin clave, no se puede firmar.
    banca::quitar_credenciales(&v, AHORA).unwrap();
    assert!(banca::firmante(&v).is_err());
    assert!(!banca::estado(&v, AHORA).unwrap().clave);
}

#[test]
fn descargas_sin_duplicados_y_con_extractos_en_fichero() {
    let v = vault();
    banca::guardar_ajustes(
        &v,
        &Ajustes {
            aceptado: true,
            url_vuelta: banca::URL_VUELTA.into(),
        },
        AHORA,
    )
    .unwrap();
    // Antes de conectar, un extracto en fichero de la misma cuenta con el cobro del 2 de
    // octubre (sin identificador del banco).
    let fichero = Extracto {
        formato: "Hoja de cálculo".into(),
        banco: "BBVA".into(),
        iban: Some(IBAN.into()),
        cuenta: String::new(),
        titular: "EMPRESA DEMO SL".into(),
        divisa: "EUR".into(),
        clase: Clase::Cuenta,
        desde: None,
        hasta: None,
        saldo_inicial: None,
        saldo_final: None,
        movimientos: vec![Movimiento {
            importe_eur: None,
            fecha: date!(2026 - 10 - 02),
            fecha_valor: None,
            concepto: "TRANSFERENCIA CLIENTE DEMO".into(),
            importe: Money::from_cents(121_000),
            saldo: None,
            referencia: String::new(),
        }],
        avisos: Vec::new(),
    };
    let r = importar(
        &v,
        "octubre.xlsx",
        b"x",
        &[comprobado(fichero)],
        false,
        AHORA,
    )
    .unwrap();
    assert_eq!(r.movimientos_nuevos, 1);

    let (link, viejas) = conectar(&v, "s-1", ["c-1", "c-2"], AHORA);
    assert!(viejas.is_empty());
    let e = banca::estado(&v, AHORA).unwrap();
    let c = &e.conexiones[0];
    assert_eq!((c.banco.as_str(), c.estado.as_str()), ("BBVA", "activa"));
    assert_eq!(c.dias, Some(179));
    assert_eq!(c.cuentas.len(), 2);
    assert_eq!(c.cuentas[0].iban.as_deref(), Some("ES91 …1332"));
    assert!(c.cuentas[0].activa);
    // La tarjeta, sin IBAN, no se descarga salvo que se active.
    assert!(c.cuentas[1].tarjeta);
    assert!(!c.cuentas[1].activa);

    // Primera descarga: todo lo que dé el banco. El cobro del 2 ya estaba (del fichero); un
    // pendiente no entra; dos cafés iguales del mismo día con identificadores distintos son dos.
    let banco = Banco::default();
    let mut pendiente = tx("T9", "2026-10-29", "5.00", "DBIT", "PENDIENTE");
    pendiente.status = Some("PDNG".into());
    banco.poner(
        "c-1",
        vec![
            tx(
                "T1",
                "2026-10-02",
                "1210.00",
                "CRDT",
                "TRANSFERENCIA CLIENTE DEMO",
            ),
            tx("T2", "2026-10-05", "60.50", "DBIT", "PROVEEDOR DEMO SA"),
            tx("T3", "2026-10-06", "2.50", "DBIT", "CAFE"),
            tx("T4", "2026-10-06", "2.50", "DBIT", "CAFE"),
            pendiente,
        ],
    );
    let r = bajar(&v, &banco, AHORA);
    assert_eq!(r.cuentas.len(), 1, "{r:?}");
    assert!(r.cuentas[0].error.is_none(), "{r:?}");
    assert_eq!((r.movimientos_nuevos, r.ya_estaban), (3, 1), "{r:?}");
    let x = r.cuentas[0].resultado.as_ref().unwrap();
    assert!(x.importado, "{x:?}");
    // Sin el aviso de «sin saldos»: lo que da el banco por su conexión no se comprueba así.
    assert!(
        !x.avisos.iter().any(|a| a.contains("no trae saldos")),
        "{:?}",
        x.avisos
    );
    assert_eq!(banco.pedidos.lock().unwrap()[0], ("c-1".into(), None));
    // Al del fichero se le ha puesto el identificador del banco.
    assert_eq!(
        filas(&v),
        [
            ("2026-10-02".into(), 121_000, Some("T1".into())),
            ("2026-10-05".into(), -6_050, Some("T2".into())),
            ("2026-10-06".into(), -250, Some("T3".into())),
            ("2026-10-06".into(), -250, Some("T4".into())),
        ]
    );
    let e = banca::estado(&v, AHORA).unwrap();
    let cuenta = &e.conexiones[0].cuentas[0];
    assert_eq!(cuenta.hasta_dia.as_deref(), Some("2026-10-30"));
    assert_eq!(cuenta.saldo, Some(Money::from_cents(214_950)));
    assert_eq!(cuenta.saldo_fecha.as_deref(), Some("2026-10-29"));
    assert!(
        cuenta
            .subcuenta
            .as_deref()
            .is_some_and(|s| s.starts_with("572"))
    );
    assert!(e.conexiones[0].actualizada.is_some());

    // Segunda descarga, días después: desde diez días antes de la última (o del pendiente), con
    // lo de antes
    // (que no se repite, aunque el banco haya cambiado la fecha de uno) y uno nuevo.
    let despues = datetime!(2026-11-03 10:00 UTC);
    banco.poner(
        "c-1",
        vec![
            tx("T3", "2026-10-06", "2.50", "DBIT", "CAFE"),
            tx("T4", "2026-10-07", "2.50", "DBIT", "CAFE"),
            tx("T5", "2026-11-02", "300.00", "CRDT", "COBRO FRA 2026-0008"),
        ],
    );
    let r = bajar(&v, &banco, despues);
    assert_eq!((r.movimientos_nuevos, r.ya_estaban), (1, 2), "{r:?}");
    // Diez días antes del pendiente del 29 (que era anterior a la última descarga, el 30).
    assert_eq!(
        banco.pedidos.lock().unwrap()[1],
        ("c-1".into(), Some(date!(2026 - 10 - 19)))
    );
    assert_eq!(filas(&v).len(), 5);

    // La misma descarga otra vez: nada nuevo.
    let r = bajar(&v, &banco, despues);
    assert_eq!((r.movimientos_nuevos, r.ya_estaban), (0, 3));

    // Un extracto en fichero después, con el cobro del 2 de noviembre: ya estaba.
    let fichero = Extracto {
        formato: "Norma 43".into(),
        banco: "BBVA".into(),
        iban: Some(IBAN.into()),
        cuenta: String::new(),
        titular: String::new(),
        divisa: "EUR".into(),
        clase: Clase::Cuenta,
        desde: None,
        hasta: None,
        saldo_inicial: None,
        saldo_final: None,
        movimientos: vec![Movimiento {
            importe_eur: None,
            fecha: date!(2026 - 11 - 02),
            fecha_valor: None,
            concepto: "COBRO".into(),
            importe: Money::from_cents(30_000),
            saldo: None,
            referencia: String::new(),
        }],
        avisos: Vec::new(),
    };
    let r = importar(
        &v,
        "noviembre.n43",
        b"y",
        &[comprobado(fichero)],
        false,
        despues,
    )
    .unwrap();
    assert_eq!((r.movimientos_nuevos, r.ya_estaban), (0, 1));

    // Activar la tarjeta: entra como tarjeta de crédito, con su entidad y últimos dígitos.
    let tarjeta = banca::estado(&v, despues).unwrap().conexiones[0].cuentas[1].id;
    banca::activar_cuenta(&v, tarjeta, true).unwrap();
    banco.poner(
        "c-2",
        vec![tx("K1", "2026-10-15", "45.00", "DBIT", "LIBRERIA DEMO")],
    );
    let r = bajar(&v, &banco, despues);
    let t = r
        .cuentas
        .iter()
        .find_map(|c| {
            c.resultado
                .as_ref()
                .filter(|x| x.clase == Clase::TarjetaCredito)
        })
        .expect("la tarjeta");
    assert_eq!(t.movimientos_nuevos, 1);
    assert!(
        t.subcuenta
            .as_deref()
            .is_some_and(|s| s.starts_with("5201"))
    );
    let numero: String = v
        .connection()
        .query_row(
            "SELECT number FROM bank_account WHERE id = ?1",
            [t.cuenta_id.unwrap()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(numero, "tarjeta:0182:1234");

    // Renovar el permiso (otra sesión, otras uid): sustituye a la conexión anterior y conserva
    // hasta dónde se había bajado cada cuenta y en qué cuenta de TaxPain entra.
    let (nuevo, viejas) = conectar(&v, "s-2", ["d-1", "d-2"], despues);
    assert_eq!(viejas, ["s-1"]);
    let e = banca::estado(&v, despues).unwrap();
    assert_eq!(e.conexiones.len(), 1);
    assert_eq!(e.conexiones[0].id, nuevo);
    assert_ne!(nuevo, link);
    assert_eq!(
        e.conexiones[0].cuentas[0].hasta_dia.as_deref(),
        Some("2026-11-03")
    );
    assert!(e.conexiones[0].cuentas[1].activa);
    let d = banca::descargas(&v, None, despues).unwrap();
    assert_eq!(d[0].uid, "d-1");
    assert_eq!(d[0].desde, Some(date!(2026 - 10 - 24)));
    assert!(d[0].boveda.is_some());

    // Desconectar: la sesión, para retirarla en Enable Banking; los movimientos se quedan.
    assert_eq!(
        banca::quitar(&v, nuevo, despues).unwrap().as_deref(),
        Some("s-2")
    );
    assert!(banca::estado(&v, despues).unwrap().conexiones.is_empty());
    assert_eq!(filas(&v).len(), 6);
}

#[test]
fn permiso_caducado_o_retirado() {
    let v = vault();
    banca::guardar_ajustes(
        &v,
        &Ajustes {
            aceptado: true,
            url_vuelta: banca::URL_VUELTA.into(),
        },
        AHORA,
    )
    .unwrap();
    let (link, _) = conectar(&v, "s-1", ["c-1", "c-2"], AHORA);
    // El banco dice que el permiso se ha retirado: la conexión pasa a caducada, con el error.
    let banco = Banco {
        caducadas: vec!["c-1".into()],
        ..Banco::default()
    };
    let r = bajar(&v, &banco, AHORA);
    assert!(
        r.cuentas[0]
            .error
            .as_deref()
            .is_some_and(|e| e.contains("caducado")),
        "{r:?}"
    );
    let e = banca::estado(&v, AHORA).unwrap();
    assert_eq!(e.conexiones[0].estado, "caducada");
    assert!(e.conexiones[0].error.is_some());
    // Ya no se descarga.
    assert!(banca::descargas(&v, Some(link), AHORA).unwrap().is_empty());

    // Y una que llega al final de su plazo caduca sola.
    let (otra, _) = conectar(&v, "s-3", ["e-1", "e-2"], AHORA);
    let tarde = datetime!(2027-04-29 10:00 UTC);
    assert!(banca::descargas(&v, Some(otra), tarde).unwrap().is_empty());
    let e = banca::estado(&v, tarde).unwrap();
    assert!(e.conexiones.iter().all(|c| c.estado == "caducada"));
    assert_eq!(
        e.conexiones.iter().find(|c| c.id == otra).unwrap().dias,
        Some(0)
    );
}

/// Descarga con los casos raros de los identificadores: uno que el banco reutiliza con otro
/// importe, uno repetido en la misma respuesta (que además casa con uno de fichero), uno que el
/// usuario borró y un pendiente que tarda en contabilizarse.
#[test]
fn identificadores_raros_borrados_y_pendientes() {
    let v = vault();
    banca::guardar_ajustes(
        &v,
        &Ajustes {
            aceptado: true,
            url_vuelta: banca::URL_VUELTA.into(),
        },
        AHORA,
    )
    .unwrap();
    // Un extracto en fichero con un cobro del 3 de octubre.
    let fichero = Extracto {
        formato: "Hoja de cálculo".into(),
        banco: "BBVA".into(),
        iban: Some(IBAN.into()),
        cuenta: String::new(),
        titular: String::new(),
        divisa: "EUR".into(),
        clase: Clase::Cuenta,
        desde: None,
        hasta: None,
        saldo_inicial: None,
        saldo_final: None,
        movimientos: vec![Movimiento {
            importe_eur: None,
            fecha: date!(2026 - 10 - 03),
            fecha_valor: None,
            concepto: "COBRO".into(),
            importe: Money::from_cents(50_000),
            saldo: None,
            referencia: String::new(),
        }],
        avisos: Vec::new(),
    };
    importar(&v, "f.xlsx", b"f", &[comprobado(fichero)], false, AHORA).unwrap();
    conectar(&v, "s-1", ["c-1", "c-2"], AHORA);
    let banco = Banco::default();
    let mut retenido = tx("H1", "2026-10-08", "80.00", "DBIT", "HOTEL DEMO");
    retenido.status = Some("PDNG".into());
    retenido.booking_date = None;
    retenido.transaction_date = Some("2026-10-08".into());
    banco.poner(
        "c-1",
        vec![
            tx("A1", "2026-10-03", "500.00", "CRDT", "COBRO"),
            // La misma entrada dos veces en la respuesta.
            tx("A1", "2026-10-03", "500.00", "CRDT", "COBRO"),
            tx("B1", "2026-10-04", "20.00", "DBIT", "COMISION"),
            retenido,
        ],
    );
    let r = bajar(&v, &banco, AHORA);
    assert_eq!((r.movimientos_nuevos, r.ya_estaban), (1, 2), "{r:?}");
    assert_eq!(filas(&v).len(), 2, "{:?}", filas(&v));

    // La descarga siguiente pide desde antes del pendiente (8 de octubre − 10 días), no desde
    // diez días antes de hoy.
    let despues = datetime!(2026-11-05 10:00 UTC);
    banco.poner(
        "c-1",
        vec![
            tx("A1", "2026-10-03", "500.00", "CRDT", "COBRO"),
            // El banco reutiliza B1 para otro movimiento con otro importe: es otro.
            tx("B1", "2026-10-04", "35.00", "DBIT", "OTRA COMISION"),
            // El pendiente ya contabilizado, con su fecha de entonces.
            tx("H1", "2026-10-09", "80.00", "DBIT", "HOTEL DEMO"),
        ],
    );
    let r = bajar(&v, &banco, despues);
    assert_eq!(
        banco.pedidos.lock().unwrap()[1],
        ("c-1".into(), Some(date!(2026 - 09 - 28)))
    );
    assert_eq!((r.movimientos_nuevos, r.ya_estaban), (2, 1), "{r:?}");
    assert_eq!(filas(&v).len(), 4);

    // El usuario borra el segundo B1 (un duplicado, para él): no vuelve.
    let id: i64 = v
        .connection()
        .query_row(
            "SELECT id FROM bank_tx WHERE external_id = 'B1' AND amount = -3500",
            [],
            |r| r.get(0),
        )
        .unwrap();
    bank::delete_movement(&v, id, despues).unwrap();
    let r = bajar(&v, &banco, despues);
    assert_eq!(r.movimientos_nuevos, 0, "{r:?}");
    assert_eq!(filas(&v).len(), 3);
}

/// Una cuenta que no se puede importar no para a las demás, y la renovación sólo hereda lo de
/// la cuenta de la misma divisa.
#[test]
fn una_cuenta_que_falla_no_para_las_demas() {
    let v = vault();
    banca::guardar_ajustes(
        &v,
        &Ajustes {
            aceptado: true,
            url_vuelta: banca::URL_VUELTA.into(),
        },
        AHORA,
    )
    .unwrap();
    let (link, _) = conectar(&v, "s-1", ["c-1", "c-2"], AHORA);
    let tarjeta = banca::estado(&v, AHORA).unwrap().conexiones[0].cuentas[1].id;
    banca::activar_cuenta(&v, tarjeta, true).unwrap();
    let banco = Banco::default();
    banco.poner(
        "c-1",
        vec![tx("A1", "2026-10-03", "500.00", "CRDT", "COBRO")],
    );
    banco.poner(
        "c-2",
        vec![tx("K1", "2026-10-15", "45.00", "DBIT", "LIBRERIA")],
    );
    let r = bajar(&v, &banco, AHORA);
    assert_eq!(r.movimientos_nuevos, 2, "{r:?}");
    // La cuenta queda apuntando a una cuenta de TaxPain en dólares: su importación falla.
    v.connection()
        .execute(
            "INSERT INTO bank_account(number, account, name, currency) VALUES ('X', '5730001', 'USD', 'USD')",
            [],
        )
        .unwrap();
    let usd = v.connection().last_insert_rowid();
    v.connection()
        .execute(
            "UPDATE bank_link_account SET bank_account = ?1 WHERE uid = 'c-1'",
            [usd],
        )
        .unwrap();
    banco.poner(
        "c-1",
        vec![tx("A2", "2026-10-20", "10.00", "CRDT", "COBRO")],
    );
    banco.poner(
        "c-2",
        vec![tx("K2", "2026-10-21", "5.00", "DBIT", "LIBRERIA")],
    );
    let r = bajar(&v, &banco, datetime!(2026-10-31 10:00 UTC));
    assert_eq!(r.cuentas.len(), 2);
    assert!(r.cuentas[0].error.is_some(), "{r:?}");
    assert_eq!(
        r.cuentas[1]
            .resultado
            .as_ref()
            .map(|x| x.movimientos_nuevos),
        Some(1),
        "{r:?}"
    );
    // La conexión no se da por actualizada con un error.
    let e = banca::estado(&v, AHORA).unwrap();
    assert_eq!(e.conexiones[0].id, link);
}
