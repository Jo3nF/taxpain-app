//! Importación de extractos de cualquier formato a la bóveda: sin duplicados entre extractos
//! que se solapan, cada cuenta por su clave y su divisa, cuentas enmascaradas, tarjetas y
//! conciliación sin mezclar divisas. Datos inventados.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::trial_balance;
use taxpain_app::bank::{self, import_n43, movements, reconcile_account, reconcile_invoice};
use taxpain_app::extractos::importar::{comprobado, importar, importar_en_cuenta};
use taxpain_app::extractos::{Clase, Cuadre, Extracto, Leido, Movimiento};
use taxpain_app::invoicing::{self, InvoiceDraft, issue};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::invoice::{InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime};

const IBAN_ES: &str = "ES9121000418450200051332";
/// Clave de la cuenta de `IBAN_ES` (entidad, oficina y número, como en su Norma 43).
const CLAVE_ES: &str = "210004180200051332";
const IBAN_GB: &str = "GB82WEST12345698765432";
const N43: &[u8] = include_bytes!("../../../fixtures/accounting/n43-ejemplo.txt");
const AHORA: OffsetDateTime = datetime!(2026-10-25 10:00 UTC);

fn vault() -> Vault {
    Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap()
}

fn m(fecha: Date, concepto: &str, importe: i64, saldo: Option<i64>) -> Movimiento {
    Movimiento {
        importe_eur: None,
        fecha,
        fecha_valor: None,
        concepto: concepto.into(),
        importe: Money::from_cents(importe),
        saldo: saldo.map(Money::from_cents),
        referencia: String::new(),
    }
}

fn extracto(formato: &str, cuenta: &str, divisa: &str, movimientos: Vec<Movimiento>) -> Extracto {
    Extracto {
        formato: formato.into(),
        banco: "Banco Demo".into(),
        iban: None,
        cuenta: cuenta.into(),
        titular: "EMPRESA DEMO SL".into(),
        divisa: divisa.into(),
        clase: Clase::Cuenta,
        desde: None,
        hasta: None,
        saldo_inicial: None,
        saldo_final: None,
        movimientos,
        avisos: Vec::new(),
    }
}

fn con_iban(mut e: Extracto, iban: &str) -> Extracto {
    e.iban = Some(iban.into());
    e
}

fn imp(v: &Vault, nombre: &str, e: Extracto) -> taxpain_app::extractos::importar::Resumen {
    importar(v, nombre, nombre.as_bytes(), &[comprobado(e)], false, AHORA).unwrap()
}

fn nuevos_y_ya(r: &taxpain_app::extractos::importar::Resumen) -> (usize, usize) {
    (r.movimientos_nuevos, r.ya_estaban)
}

/// Tres meses de una cuenta con saldo en cada fila, del más nuevo al más antiguo (como los
/// da el banco), con dos comisiones iguales el mismo día.
fn trimestre() -> Vec<Movimiento> {
    vec![
        m(
            date!(2026 - 03 - 20),
            "RECIBO LUZ DEMO",
            -8_450,
            Some(388_550),
        ),
        m(
            date!(2026 - 03 - 02),
            "TRANSFERENCIA CLIENTE DEMO",
            121_000,
            Some(397_000),
        ),
        m(date!(2026 - 02 - 15), "COMISION", -500, Some(276_000)),
        m(date!(2026 - 02 - 15), "COMISION", -500, Some(276_500)),
        m(
            date!(2026 - 02 - 01),
            "ALQUILER OFICINA",
            -60_500,
            Some(277_000),
        ),
        m(
            date!(2026 - 01 - 10),
            "TRANSFERENCIA CLIENTE DEMO",
            37_500,
            Some(337_500),
        ),
    ]
}

#[test]
fn importar_dos_veces_no_duplica() {
    let v = vault();
    let e = con_iban(
        extracto("Hoja de cálculo", CLAVE_ES, "EUR", trimestre()),
        IBAN_ES,
    );
    let r = imp(&v, "trimestre.xlsx", e.clone());
    let x = &r.extractos[0];
    assert!(x.importado, "{x:?}");
    assert_eq!(x.cuadre, Cuadre::Cuadra);
    assert!(x.cuenta_nueva);
    assert_eq!(x.subcuenta.as_deref(), Some("5720001"));
    assert_eq!(x.cuenta, "Banco Demo …1332 EUR");
    assert_eq!(nuevos_y_ya(&r), (6, 0));
    let again = imp(&v, "copia/trimestre.xlsx", e);
    assert_eq!(nuevos_y_ya(&again), (0, 6));
    assert!(!again.extractos[0].cuenta_nueva);
    // Aunque el lector ponga de clave el IBAN o el código cuenta cliente tal cual, es la misma.
    for clave in [IBAN_ES, "2100 0418 45 0200051332"] {
        let r = imp(&v, "otra.csv", extracto("CSV", clave, "EUR", trimestre()));
        assert_eq!(r.extractos[0].cuenta_id, again.extractos[0].cuenta_id);
        assert_eq!(nuevos_y_ya(&r), (0, 6));
    }
    let todos = movements(&v, false).unwrap();
    assert_eq!(todos.len(), 6);
    assert!(
        todos
            .iter()
            .all(|t| t.currency == "EUR" && t.balance.is_some())
    );
    // La cuenta guarda su IBAN (para las remesas).
    let banks = taxpain_app::remesas::banks(&v).unwrap();
    assert_eq!(banks.len(), 1);
    assert_eq!(banks[0].iban, IBAN_ES);
}

#[test]
fn extractos_que_se_solapan_de_formatos_distintos() {
    let v = vault();
    let todo = trimestre();
    // El de febrero en Excel; luego el trimestre en PDF (también trae febrero).
    let febrero: Vec<Movimiento> = todo[2..5].to_vec();
    let r = imp(
        &v,
        "febrero.xlsx",
        extracto("Hoja de cálculo", CLAVE_ES, "EUR", febrero),
    );
    assert_eq!(nuevos_y_ya(&r), (3, 0));
    let r = imp(
        &v,
        "trimestre.pdf",
        con_iban(extracto("PDF", CLAVE_ES, "EUR", todo.clone()), IBAN_ES),
    );
    assert_eq!(nuevos_y_ya(&r), (3, 3), "{r:?}");
    // Y un listado sin saldos de marzo (otro formato, con la fecha valor un día después en el
    // de la luz): todo estaba.
    let mut luz = m(date!(2026 - 03 - 21), "RECIBO LUZ", -8_450, None);
    luz.fecha_valor = Some(date!(2026 - 03 - 21));
    let mut e = extracto(
        "CSV",
        CLAVE_ES,
        "EUR",
        vec![
            m(date!(2026 - 03 - 02), "TRANSF. CLIENTE", 121_000, None),
            luz,
        ],
    );
    e.avisos.push("columna dudosa".into());
    let r = imp(&v, "marzo.csv", e);
    assert_eq!(r.extractos[0].cuadre, Cuadre::SinSaldos);
    assert!(r.extractos[0].importado);
    assert!(r.extractos[0].avisos.iter().any(|a| a == "columna dudosa"));
    assert!(
        r.extractos[0]
            .avisos
            .iter()
            .any(|a| a.contains("no trae saldos"))
    );
    // El día 21 no es la fecha de la operación (20) pero el importe y el día de al lado no
    // bastan: sólo casa por la fecha valor o por el saldo.
    assert_eq!(nuevos_y_ya(&r), (1, 1), "{r:?}");
    assert_eq!(movements(&v, false).unwrap().len(), 7);
}

#[test]
fn con_saldos_completa_los_que_no_lo_tenian() {
    let v = vault();
    let sin: Vec<Movimiento> = trimestre()
        .into_iter()
        .map(|mut x| {
            x.saldo = None;
            x
        })
        .collect();
    let mut e = extracto("OFX", CLAVE_ES, "EUR", sin);
    e.saldo_inicial = Some(Money::from_cents(300_000));
    e.saldo_final = Some(Money::from_cents(388_550));
    let r = imp(&v, "trimestre.ofx", e);
    assert_eq!(r.extractos[0].cuadre, Cuadre::Cuadra);
    assert_eq!(nuevos_y_ya(&r), (6, 0));
    let r = imp(
        &v,
        "trimestre.xlsx",
        extracto("Hoja", CLAVE_ES, "EUR", trimestre()),
    );
    assert_eq!(nuevos_y_ya(&r), (0, 6));
    assert_eq!(r.extractos[0].saldos_completados, 6);
    assert!(
        movements(&v, false)
            .unwrap()
            .iter()
            .all(|t| t.balance.is_some())
    );
}

#[test]
fn movimientos_iguales_del_mismo_dia_se_cuentan() {
    let v = vault();
    let d = date!(2026 - 05 - 05);
    // Un extracto con una retención; otro (que se solapa) con las dos de ese día.
    let r = imp(
        &v,
        "a.pdf",
        extracto(
            "PDF",
            CLAVE_ES,
            "EUR",
            vec![m(d, "RETENCION", -1_140, Some(98_860))],
        ),
    );
    assert_eq!(nuevos_y_ya(&r), (1, 0));
    let dos = vec![
        m(d, "RETENCION", -1_140, Some(98_860)),
        m(d, "RETENCION", -1_140, Some(97_720)),
    ];
    let r = imp(&v, "b.xlsx", extracto("Hoja", CLAVE_ES, "EUR", dos.clone()));
    assert_eq!(nuevos_y_ya(&r), (1, 1));
    let r = imp(&v, "b.xlsx", extracto("Hoja", CLAVE_ES, "EUR", dos));
    assert_eq!(nuevos_y_ya(&r), (0, 2));
    // Sin saldos también se cuentan: tres iguales contra las dos que hay.
    let tres = vec![
        m(d, "RETENCION", -1_140, None),
        m(d, "RETENCION", -1_140, None),
        m(d, "RETENCION", -1_140, None),
    ];
    let r = imp(&v, "c.csv", extracto("CSV", CLAVE_ES, "EUR", tres));
    assert_eq!(nuevos_y_ya(&r), (1, 2));
    assert_eq!(movements(&v, false).unwrap().len(), 3);
}

#[test]
fn el_mismo_iban_en_dos_divisas_son_dos_cuentas() {
    let v = vault();
    let d = date!(2026 - 04 - 01);
    let eur = con_iban(
        extracto(
            "PDF",
            IBAN_GB,
            "EUR",
            vec![m(d, "PAGO", 10_000, Some(10_000))],
        ),
        IBAN_GB,
    );
    let usd = con_iban(
        extracto(
            "PDF",
            IBAN_GB,
            "USD",
            vec![m(d, "PAGO", 10_000, Some(10_000))],
        ),
        IBAN_GB,
    );
    let r = importar(
        &v,
        "multidivisa.pdf",
        b"x",
        &[comprobado(eur.clone()), comprobado(usd.clone())],
        false,
        AHORA,
    )
    .unwrap();
    assert_eq!(nuevos_y_ya(&r), (2, 0));
    assert_ne!(r.extractos[0].cuenta_id, r.extractos[1].cuenta_id);
    assert_eq!(r.extractos[1].cuenta, "Banco Demo …5432 USD");
    assert_eq!(r.extractos[0].subcuenta.as_deref(), Some("5720001"));
    // La de dólares, en la 573 (bancos, moneda extranjera).
    assert_eq!(r.extractos[1].subcuenta.as_deref(), Some("5730001"));
    assert!(
        r.extractos[1]
            .avisos
            .iter()
            .any(|a| a.contains("Cuenta en USD"))
    );
    // Otra vez, en orden contrario: nada nuevo, cada uno en la suya.
    let r = importar(
        &v,
        "multidivisa.pdf",
        b"x",
        &[comprobado(usd), comprobado(eur)],
        false,
        AHORA,
    )
    .unwrap();
    assert_eq!(nuevos_y_ya(&r), (0, 2));
    let todos = movements(&v, false).unwrap();
    assert_eq!(
        todos
            .iter()
            .filter(|t| t.currency == "USD")
            .map(|t| t.amount)
            .collect::<Vec<_>>(),
        [Money::from_cents(10_000)]
    );
    // Las remesas SEPA sólo pueden ir a la de euros.
    assert_eq!(taxpain_app::remesas::banks(&v).unwrap().len(), 1);
}

#[test]
fn cuenta_enmascarada_y_provisional() {
    let v = vault();
    let d = date!(2026 - 06 - 01);
    // Primero la cuenta entera; un PDF que sólo enseña «ES91…1332» casa con ella.
    imp(
        &v,
        "entero.xlsx",
        con_iban(
            extracto("Hoja", CLAVE_ES, "EUR", vec![m(d, "A", 1_000, Some(1_000))]),
            IBAN_ES,
        ),
    );
    let r = imp(
        &v,
        "enmascarado.pdf",
        extracto(
            "PDF",
            "ES91*1332",
            "EUR",
            vec![
                m(d, "A", 1_000, Some(1_000)),
                m(date!(2026 - 06 - 02), "B", 500, Some(1_500)),
            ],
        ),
    );
    assert!(!r.extractos[0].cuenta_nueva);
    assert_eq!(nuevos_y_ya(&r), (1, 1));

    // Otra cuenta que sólo se ve enmascarada queda provisional…
    let gb = |movs| extracto("PDF", "GB82*5432", "EUR", movs);
    let r = imp(&v, "otra.pdf", gb(vec![m(d, "C", 2_000, Some(2_000))]));
    let x = &r.extractos[0];
    assert!(x.cuenta_nueva);
    assert!(x.avisos.iter().any(|a| a.contains("Cuenta provisional")));
    let provisional = x.cuenta_id;
    // …otro extracto enmascarado de la misma va a ella…
    let r = imp(
        &v,
        "otra-2.pdf",
        gb(vec![
            m(d, "C", 2_000, Some(2_000)),
            m(date!(2026 - 06 - 03), "D", -300, Some(1_700)),
        ]),
    );
    assert_eq!(r.extractos[0].cuenta_id, provisional);
    assert_eq!(nuevos_y_ya(&r), (1, 1));
    // …y cuando llega uno con el IBAN completo, se completa esa misma cuenta.
    let r = imp(
        &v,
        "otra-entera.csv",
        con_iban(
            extracto(
                "CSV",
                IBAN_GB,
                "EUR",
                vec![m(date!(2026 - 06 - 03), "D", -300, Some(1_700))],
            ),
            IBAN_GB,
        ),
    );
    assert_eq!(r.extractos[0].cuenta_id, provisional);
    assert!(!r.extractos[0].cuenta_nueva);
    assert_eq!(nuevos_y_ya(&r), (0, 1));
    let banks = taxpain_app::remesas::banks(&v).unwrap();
    assert_eq!(banks.len(), 2);
    assert!(
        banks
            .iter()
            .any(|b| b.iban == IBAN_GB && !b.name.contains("provisional"))
    );
}

#[test]
fn si_no_cuadra_no_entra_salvo_que_se_fuerce() {
    let v = vault();
    let mut e = extracto(
        "PDF",
        CLAVE_ES,
        "EUR",
        vec![m(date!(2026 - 07 - 01), "A", -4_000, None)],
    );
    e.saldo_inicial = Some(Money::from_cents(10_000));
    e.saldo_final = Some(Money::from_cents(5_000));
    let leido = comprobado(e);
    assert!(matches!(leido.cuadre, Cuadre::NoCuadra { .. }));
    let r = importar(
        &v,
        "malo.pdf",
        b"x",
        std::slice::from_ref(&leido),
        false,
        AHORA,
    )
    .unwrap();
    let x = &r.extractos[0];
    assert!(!x.importado);
    assert!(x.motivo.contains("no cuadra"), "{}", x.motivo);
    assert_eq!(x.cuenta_id, None);
    assert!(movements(&v, false).unwrap().is_empty());
    let r = importar(&v, "malo.pdf", b"x", &[leido], true, AHORA).unwrap();
    assert!(r.extractos[0].importado);
    assert!(
        r.extractos[0]
            .avisos
            .iter()
            .any(|a| a.contains("no cuadra"))
    );
    assert_eq!(movements(&v, false).unwrap().len(), 1);
}

#[test]
fn sin_cuenta_se_importa_en_la_que_elija_el_usuario() {
    let v = vault();
    let d = date!(2026 - 07 - 01);
    let r = imp(
        &v,
        "cuenta.xlsx",
        extracto("Hoja", CLAVE_ES, "EUR", vec![m(d, "A", 1_000, Some(1_000))]),
    );
    let id = r.extractos[0].cuenta_id.unwrap();
    let sin = comprobado(extracto(
        "CSV",
        "",
        "EUR",
        vec![
            m(d, "A", 1_000, None),
            m(date!(2026 - 07 - 02), "B", 200, None),
        ],
    ));
    let r = importar(
        &v,
        "sin.csv",
        b"x",
        std::slice::from_ref(&sin),
        false,
        AHORA,
    )
    .unwrap();
    assert!(!r.extractos[0].importado);
    assert!(r.extractos[0].motivo.contains("elige la cuenta"));
    let r = importar_en_cuenta(
        &v,
        "sin.csv",
        b"x",
        std::slice::from_ref(&sin),
        id,
        false,
        AHORA,
    )
    .unwrap();
    assert_eq!(nuevos_y_ya(&r), (1, 1));
    // No en una de otra divisa.
    let mut usd = sin.clone();
    usd.extracto.divisa = "USD".into();
    assert!(importar_en_cuenta(&v, "sin.csv", b"x", &[usd], id, false, AHORA).is_err());
}

#[test]
fn norma43_y_luego_otro_formato_de_la_misma_cuenta() {
    let v = vault();
    let s = import_n43(&v, N43, AHORA).unwrap();
    assert_eq!((s.new_movements, s.duplicates), (6, 0));
    let again = import_n43(&v, N43, AHORA).unwrap();
    assert_eq!((again.new_movements, again.duplicates), (0, 6));
    // El mismo mes en Excel, con el saldo de cada fila: no duplica y les pone el saldo.
    let movs = vec![
        m(
            date!(2026 - 10 - 01),
            "TRANSFERENCIA",
            121_000,
            Some(244_456),
        ),
        m(date!(2026 - 10 - 02), "RECIBO", -7_865, Some(236_591)),
        m(date!(2026 - 10 - 05), "COMPRA", -4_599, Some(231_992)),
        m(
            date!(2026 - 10 - 06),
            "SEGUROS SOCIALES",
            -183_420,
            Some(48_572),
        ),
        m(date!(2026 - 10 - 10), "INGRESO", 50_000, Some(98_572)),
        m(date!(2026 - 10 - 12), "COMISION", -1_250, Some(97_322)),
    ];
    let r = imp(
        &v,
        "octubre.xlsx",
        extracto("Hoja", "999900010000123456", "EUR", movs),
    );
    assert!(!r.extractos[0].cuenta_nueva, "{r:?}");
    assert_eq!(nuevos_y_ya(&r), (0, 6));
    assert_eq!(r.extractos[0].saldos_completados, 6);
    // Y el Norma 43 otra vez, después del Excel.
    let again = import_n43(&v, N43, AHORA).unwrap();
    assert_eq!((again.new_movements, again.duplicates), (0, 6));
    assert_eq!(movements(&v, false).unwrap().len(), 6);
}

#[test]
fn una_cuenta_de_antes_se_reconoce_y_se_completa() {
    // Como en una bóveda con cuentas de antes de las divisas: sin IBAN, tipo ni banco.
    let v = vault();
    v.connection()
        .execute(
            "INSERT INTO bank_account(bank_code, number, account, name)
             VALUES ('2100', ?1, '5720001', 'EMPRESA DEMO SL (1332)')",
            [CLAVE_ES],
        )
        .unwrap();
    let r = imp(
        &v,
        "trimestre.xlsx",
        con_iban(extracto("Hoja", CLAVE_ES, "EUR", trimestre()), IBAN_ES),
    );
    assert!(!r.extractos[0].cuenta_nueva);
    assert_eq!(r.extractos[0].subcuenta.as_deref(), Some("5720001"));
    let (iban, bank, kind): (Option<String>, String, String) = v
        .connection()
        .query_row(
            "SELECT iban, bank, kind FROM bank_account WHERE number = ?1",
            [CLAVE_ES],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        (iban.as_deref(), bank.as_str(), kind.as_str()),
        (Some(IBAN_ES), "Banco Demo", "cuenta")
    );
}

fn tarjeta(cuenta: &str, clase: Clase, movimientos: Vec<Movimiento>) -> Extracto {
    let mut e = extracto("PDF", cuenta, "EUR", movimientos);
    e.clase = clase;
    e
}

fn saldo(v: &Vault, cuenta: &str) -> Money {
    trial_balance(v, 2026, None)
        .unwrap()
        .iter()
        .find(|b| b.account.as_str() == cuenta)
        .map_or(Money::ZERO, |b| b.balance())
}

fn entradas_del_diario(v: &Vault) -> i64 {
    v.connection()
        .query_row("SELECT count(*) FROM journal_entry", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn tarjeta_de_credito_y_su_liquidacion() {
    let v = vault();
    // La cuenta, con el cargo mensual de la tarjeta.
    imp(
        &v,
        "cuenta.xlsx",
        extracto(
            "Hoja",
            CLAVE_ES,
            "EUR",
            vec![
                m(date!(2026 - 03 - 01), "SALDO", 100_000, Some(100_000)),
                m(
                    date!(2026 - 03 - 05),
                    "ADEUDO MENSUAL TARJETA",
                    -5_370,
                    Some(94_630),
                ),
            ],
        ),
    );
    // La tarjeta: sus compras y el abono del recibo, sin saldos.
    let r = imp(
        &v,
        "tarjeta.pdf",
        tarjeta(
            "tarjeta:2100:4321",
            Clase::TarjetaCredito,
            vec![
                m(date!(2026 - 03 - 04), "RECIBO MES ANTERIOR", 5_370, None),
                m(date!(2026 - 02 - 20), "HOSTING DEMO", -3_370, None),
                m(date!(2026 - 02 - 10), "LIBRERIA DEMO", -2_000, None),
            ],
        ),
    );
    let x = &r.extractos[0];
    assert!(x.importado);
    assert_eq!(x.clase, Clase::TarjetaCredito);
    assert_eq!(x.subcuenta.as_deref(), Some("5201001"));
    assert_eq!(x.cuenta, "Tarjeta de crédito Banco Demo …4321 EUR");
    let pend = movements(&v, true).unwrap();
    assert_eq!(pend.len(), 5);
    assert!(
        pend.iter()
            .filter(|t| t.account_kind == "tarjeta_credito")
            .count()
            == 3
    );
    // El cargo de la cuenta contra la 5201: el abono de la tarjeta queda casado con el mismo
    // asiento, sin contabilizarlo dos veces.
    let cargo = pend
        .iter()
        .find(|t| t.amount == Money::from_cents(-5_370))
        .unwrap();
    let antes = entradas_del_diario(&v);
    let e1 = reconcile_account(&v, cargo.id, "5201001", "Liquidación tarjeta", AHORA).unwrap();
    assert_eq!(entradas_del_diario(&v), antes + 1);
    let pend = movements(&v, true).unwrap();
    assert!(pend.iter().all(|t| t.amount != Money::from_cents(5_370)));
    assert_eq!(pend.len(), 3);
    // Las compras, contra su gasto: la 5201 queda saldada.
    for t in pend.iter().filter(|t| t.account_kind == "tarjeta_credito") {
        reconcile_account(&v, t.id, "629", &t.description, AHORA).unwrap();
    }
    assert_eq!(saldo(&v, "5201001"), Money::ZERO);
    assert_eq!(saldo(&v, "629"), Money::from_cents(5_370));
    let asiento = |id: i64| -> Option<i64> {
        v.connection()
            .query_row("SELECT entry_id FROM bank_tx WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap()
    };
    let abono: i64 = v
        .connection()
        .query_row("SELECT id FROM bank_tx WHERE amount = 5370", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(asiento(abono), Some(e1));
}

#[test]
fn traspaso_contabilizado_desde_la_otra_cara() {
    let v = vault();
    // Primero la tarjeta, y su recibo contra la 572 de la cuenta (que aún no se ha importado).
    imp(
        &v,
        "tarjeta.pdf",
        tarjeta(
            "tarjeta:2100:4321",
            Clase::TarjetaCredito,
            vec![m(date!(2026 - 03 - 03), "RECIBO MES ANTERIOR", 5_370, None)],
        ),
    );
    let abono = movements(&v, true).unwrap().pop().unwrap();
    let e1 = reconcile_account(&v, abono.id, "5720001", "Recibo tarjeta", AHORA).unwrap();
    // Luego la cuenta, con el cargo: contra la 5201 se casa con ese mismo asiento.
    let r = imp(
        &v,
        "cuenta.xlsx",
        extracto(
            "Hoja",
            CLAVE_ES,
            "EUR",
            vec![m(
                date!(2026 - 03 - 05),
                "ADEUDO TARJETA",
                -5_370,
                Some(94_630),
            )],
        ),
    );
    assert_eq!(r.extractos[0].subcuenta.as_deref(), Some("5720001"));
    let cargo = movements(&v, true).unwrap().pop().unwrap();
    let antes = entradas_del_diario(&v);
    let e2 = reconcile_account(&v, cargo.id, "5201001", "Recibo tarjeta", AHORA).unwrap();
    assert_eq!(e1, e2);
    assert_eq!(entradas_del_diario(&v), antes);
    assert!(movements(&v, true).unwrap().is_empty());
    assert_eq!(saldo(&v, "5201001"), Money::from_cents(5_370));
    assert_eq!(saldo(&v, "5720001"), Money::from_cents(-5_370));
}

#[test]
fn tarjeta_de_debito_solo_informa() {
    let v = vault();
    let r = imp(
        &v,
        "debito.pdf",
        tarjeta(
            "tarjeta:2100:8765",
            Clase::TarjetaDebito,
            vec![m(date!(2026 - 03 - 07), "CAFETERIA DEMO", -1_200, None)],
        ),
    );
    let x = &r.extractos[0];
    assert!(x.importado);
    assert_eq!(x.subcuenta.as_deref(), Some(""));
    assert!(x.avisos.iter().any(|a| a.contains("Tarjeta de débito")));
    assert!(movements(&v, true).unwrap().is_empty());
    let t = movements(&v, false).unwrap().pop().unwrap();
    assert_eq!(t.account_kind, "tarjeta_debito");
    assert!(bank::suggestions(&v, t.id).unwrap().is_empty());
    assert!(reconcile_account(&v, t.id, "629", "x", AHORA).is_err());
    let d = taxpain_app::dashboard::dashboard(&v, 2026, 3).unwrap();
    assert_eq!(d.pending.bank_unreconciled, 0);
    // Una tarjeta que se vio sin entidad es la misma cuando se ve con ella.
    let r = imp(
        &v,
        "debito.xls",
        tarjeta(
            "tarjeta:?:8765",
            Clase::TarjetaDebito,
            vec![m(date!(2026 - 03 - 07), "CAFETERIA DEMO SL", -1_200, None)],
        ),
    );
    assert_eq!(r.extractos[0].cuenta_id, x.cuenta_id);
    assert_eq!(nuevos_y_ya(&r), (0, 1));
}

fn facturacion(v: &Vault) -> i64 {
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("12345678Z".into()),
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
            pdf: true,
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
    invoicing::save_settings(v, &s, datetime!(2026-01-01 9:00 UTC)).unwrap();
    issue(
        v,
        &InvoiceDraft {
            kind: InvoiceKind::Completa,
            series: None,
            issue_date: date!(2026 - 03 - 01),
            operation_date: None,
            recipient: Some(Party {
                id: PartyId::Nif("B12345674".into()),
                name: "Cliente Ejemplo SA".into(),
                address: addr(),
                person: None,
            }),
            lines: vec![InvoiceLine {
                description: "Servicios".into(),
                quantity_milli: 1000,
                unit_price: Money::from_euros(1000, 0),
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
        },
        AHORA,
        true,
    )
    .unwrap()
    .id
}

#[test]
fn la_conciliacion_no_mezcla_divisas() {
    let v = vault();
    let factura = facturacion(&v);
    let d = date!(2026 - 03 - 10);
    // El mismo importe que la factura (1.210,00) en una cuenta en dólares y en una en euros.
    let usd = con_iban(
        extracto(
            "PDF",
            IBAN_GB,
            "USD",
            vec![m(d, "PAYMENT DEMO", 121_000, Some(121_000))],
        ),
        IBAN_GB,
    );
    let eur = extracto(
        "Hoja",
        CLAVE_ES,
        "EUR",
        vec![m(d, "TRANSFERENCIA CLIENTE", 121_000, Some(121_000))],
    );
    let leidos: Vec<Leido> = vec![comprobado(usd), comprobado(eur)];
    importar(&v, "dos.zip", b"x", &leidos, false, AHORA).unwrap();
    let pend = movements(&v, true).unwrap();
    let en_usd = pend.iter().find(|t| t.currency == "USD").unwrap();
    let en_eur = pend.iter().find(|t| t.currency == "EUR").unwrap();
    assert!(bank::suggestions(&v, en_usd.id).unwrap().is_empty());
    assert_eq!(bank::suggestions(&v, en_eur.id).unwrap().len(), 1);
    let err = reconcile_invoice(&v, en_usd.id, "emitida", factura, None, AHORA).unwrap_err();
    assert!(err.to_string().contains("USD"), "{err}");
    assert!(reconcile_account(&v, en_usd.id, "769", "x", AHORA).is_err());
    // Las reglas aprendidas no contabilizan movimientos en dólares.
    reconcile_invoice(&v, en_eur.id, "emitida", factura, None, AHORA).unwrap();
    assert!(
        taxpain_app::bank_rules::proposals(&v)
            .unwrap()
            .iter()
            .all(|p| p.tx.currency == "EUR")
    );
    // La de dólares no tiene asientos; la de euros, el cobro.
    assert_eq!(saldo(&v, "5730001"), Money::ZERO);
    assert_eq!(saldo(&v, "5720001"), Money::from_cents(121_000));
}

#[test]
fn lo_que_ya_contabilizo_un_diario_importado_no_se_concilia() {
    use taxpain_core::accounting::{AccountCode, EntryLine, JournalEntry, Origin};
    let v = vault();
    // El diario del gestor, importado, llega hasta febrero (con su banco).
    let acc = |c: &str| AccountCode::new(c).unwrap();
    taxpain_app::accounting::record_entry(
        v.connection(),
        &JournalEntry {
            date: date!(2026 - 02 - 28),
            concept: "Cobros de febrero".into(),
            origin: Origin::Importado(1),
            lines: vec![
                EntryLine::debit(acc("5720009"), Money::from_cents(1_000)),
                EntryLine::credit(acc("7050000"), Money::from_cents(1_000)),
            ],
        },
        AHORA,
    )
    .unwrap();
    let r = imp(
        &v,
        "trimestre.xlsx",
        extracto("Hoja", CLAVE_ES, "EUR", trimestre()),
    );
    let x = &r.extractos[0];
    assert_eq!((x.movimientos_nuevos, x.ya_contabilizados), (6, 4));
    assert!(
        x.avisos
            .iter()
            .any(|a| a.contains("ya contabilizó un diario"))
    );
    // Sólo quedan por conciliar los de marzo.
    let pend = movements(&v, true).unwrap();
    assert_eq!(pend.len(), 2);
    assert!(pend.iter().all(|t| t.op_date.starts_with("2026-03")));
    let enero = movements(&v, false)
        .unwrap()
        .into_iter()
        .find(|t| t.op_date == "2026-01-10")
        .unwrap();
    assert!(enero.booked_elsewhere);
    assert!(bank::suggestions(&v, enero.id).unwrap().is_empty());
    let err = reconcile_account(&v, enero.id, "705", "x", AHORA).unwrap_err();
    assert!(err.to_string().contains("ya contabilizado"), "{err}");
    let d = taxpain_app::dashboard::dashboard(&v, 2026, 3).unwrap();
    assert_eq!(d.pending.bank_unreconciled, 2);
}

#[test]
fn un_cargo_pendiente_que_cambia_de_importe_da_un_aviso() {
    let v = vault();
    let d = date!(2026 - 09 - 29);
    let tarjeta_con = |movs| tarjeta("tarjeta:2100:4321", Clase::TarjetaCredito, movs);
    imp(
        &v,
        "tarjeta-1.pdf",
        tarjeta_con(vec![m(d, "SERVICIO DEMO PENDIENTE", -2_340, None)]),
    );
    // El listado de después trae el cargo definitivo (en otra divisa, con otro cambio).
    let r = imp(
        &v,
        "tarjeta-2.pdf",
        tarjeta_con(vec![
            m(d, "SERVICIO DEMO", -2_405, None),
            m(date!(2026 - 09 - 30), "LIBRERIA DEMO", -1_000, None),
        ]),
    );
    assert_eq!(nuevos_y_ya(&r), (2, 0));
    let avisos: Vec<&String> = r.extractos[0]
        .avisos
        .iter()
        .filter(|a| a.starts_with("Posible duplicado"))
        .collect();
    assert_eq!(avisos.len(), 1, "{avisos:?}");
    assert!(avisos[0].contains("SERVICIO DEMO PENDIENTE"));
    // El que sobra se borra; uno conciliado, no.
    let todos = movements(&v, false).unwrap();
    let pendiente = todos.iter().find(|t| t.amount.cents() == -2_340).unwrap();
    bank::delete_movement(&v, pendiente.id, AHORA).unwrap();
    let libreria = todos.iter().find(|t| t.amount.cents() == -1_000).unwrap();
    reconcile_account(&v, libreria.id, "629", "Libros", AHORA).unwrap();
    assert!(bank::delete_movement(&v, libreria.id, AHORA).is_err());
    assert_eq!(movements(&v, false).unwrap().len(), 2);
}

/// Un extracto que no dice de qué cuenta es (búsquedas filtradas de algunos bancos): va a la
/// única cuenta de ese banco en esa divisa, en cualquier orden de importación, sin duplicar.
#[test]
fn extracto_sin_cuenta_va_a_la_unica_del_banco() {
    let movs = || {
        vec![
            m(date!(2026 - 01 - 05), "CLIENTE DEMO", 50_000, Some(150_000)),
            m(
                date!(2026 - 01 - 20),
                "PROVEEDOR DEMO",
                -20_000,
                Some(130_000),
            ),
        ]
    };
    let sin_saldos = || {
        let mut v = movs();
        for x in &mut v {
            x.saldo = None;
        }
        v.push(m(date!(2026 - 02 - 03), "HOSTING DEMO", -1_500, None));
        v
    };
    let con_cuenta = || {
        let mut e = con_iban(extracto("PDF", CLAVE_ES, "EUR", movs()), IBAN_ES);
        e.banco = "Banco Demo".into();
        e
    };
    let sin_cuenta = || {
        let mut e = extracto("PDF", "demo:Principal", "EUR", sin_saldos());
        e.banco = "Banco Demo".into();
        e
    };

    // Primero el que dice la cuenta y después el que no.
    let v = vault();
    assert_eq!(nuevos_y_ya(&imp(&v, "con.pdf", con_cuenta())), (2, 0));
    let r = imp(&v, "sin.pdf", sin_cuenta());
    assert_eq!(nuevos_y_ya(&r), (1, 2));
    assert!(
        r.extractos[0]
            .avisos
            .iter()
            .any(|a| a.contains("no dice de qué cuenta"))
    );
    let n: i64 = v
        .connection()
        .query_row("SELECT COUNT(*) FROM bank_account", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1);

    // Al revés: el que no dice la cuenta se completa con el IBAN del siguiente.
    let v = vault();
    assert_eq!(nuevos_y_ya(&imp(&v, "sin.pdf", sin_cuenta())), (3, 0));
    assert_eq!(nuevos_y_ya(&imp(&v, "con.pdf", con_cuenta())), (0, 2));
    let (n, numero): (i64, String) = v
        .connection()
        .query_row("SELECT COUNT(*), MAX(number) FROM bank_account", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!((n, numero.as_str()), (1, CLAVE_ES));
}

#[test]
fn cuenta_con_iban_no_entra_como_tarjeta_por_una_mencion() {
    // Sin columna de saldo y con más abonos que cargos: si se tomara por tarjeta, además de ir
    // a la 5201 se le daría la vuelta a los signos.
    let csv = "Consulta de movimientos de cuenta y tarjeta\n\
        Cuenta;ES91 2100 0418 4502 0005 1332\n\
        Fecha;Concepto;Importe\n\
        01/02/2025;TRANSFERENCIA CLIENTE A;1.000,00\n\
        02/02/2025;TRANSFERENCIA CLIENTE B;500,00\n\
        03/02/2025;TRANSFERENCIA CLIENTE C;250,00\n\
        04/02/2025;RECIBO LUZ;-80,00\n\
        Para más información sobre su tarjeta consulte la app\n";
    let v = vault();
    let r = taxpain_app::extractos::importar::leer_e_importar(
        &v,
        "cuenta.csv",
        csv.as_bytes(),
        false,
        AHORA,
    )
    .unwrap();
    assert_eq!(nuevos_y_ya(&r), (4, 0));
    let e = &r.extractos[0];
    assert_eq!(e.clase, Clase::Cuenta);
    assert!(e.subcuenta.as_deref().unwrap().starts_with("572"), "{e:?}");
    assert!(!e.avisos.iter().any(|a| a.contains("dado la vuelta")));
    let (kind, number): (String, String) = v
        .connection()
        .query_row("SELECT kind, number FROM bank_account", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!((kind.as_str(), number.as_str()), ("cuenta", CLAVE_ES));
    let movs = movements(&v, false).unwrap();
    assert_eq!(movs.iter().map(|m| m.amount.cents()).sum::<i64>(), 167_000);
}

/// Norma 43 con el titular corto relleno de espacios (como lo da el banco): se reconoce como
/// Norma 43 y no como una tabla sin movimientos.
#[test]
fn norma43_con_titular_corto() {
    let n43 = concat!(
        "112100041802000513322609152609302000000001012509783EMPRESA DEMO SL              \r\n",
        "22    0418260930260930170001000000000012500000000001                            \r\n",
        "2301COMISION MANTENIMIENTO SEPTIEMBRE                                           \r\n",
        "3321000418020005133200001000000000012500000000000000000000200000000100000978    \r\n",
        "88999999999999999999000004                                                      \r\n",
    );
    let leidos = taxpain_app::extractos::leer("extracto.n43", n43.as_bytes()).unwrap();
    assert_eq!(leidos.len(), 1);
    let e = &leidos[0].extracto;
    assert_eq!(e.formato, "Norma 43");
    assert_eq!(e.movimientos.len(), 1);
    assert_eq!(e.movimientos[0].importe, Money::from_cents(-1250));
    // Una tabla que empieza por «11» no se toma por Norma 43.
    let csv = "11/09/2026;Recibo luz;-45,20\n";
    assert!(
        taxpain_app::extractos::leer("e.csv", csv.as_bytes())
            .map(|l| l.iter().all(|l| l.extracto.formato != "Norma 43"))
            .unwrap_or(true)
    );
}

/// Un movimiento con fecha posterior a hoy se importa, pero con aviso.
#[test]
fn avisa_de_movimientos_con_fecha_futura() {
    let v = vault();
    let r = imp(
        &v,
        "futuro.csv",
        con_iban(
            extracto(
                "CSV",
                "",
                "EUR",
                vec![
                    m(date!(2026 - 10 - 20), "RECIBO LUZ", -4520, None),
                    m(date!(2026 - 12 - 10), "RECIBO AGUA", -1830, None),
                ],
            ),
            IBAN_ES,
        ),
    );
    assert_eq!(r.movimientos_nuevos, 2);
    let avisos = &r.extractos[0].avisos;
    assert!(
        avisos
            .iter()
            .any(|a| a.starts_with("Hay 1 movimiento con fecha posterior a hoy (25/10/2026)")),
        "{avisos:?}"
    );
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
