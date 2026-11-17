#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::calendar::TaxpayerProfile;
use taxpain_core::cripto::{Precio, Qty, Tipo};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider};
use time::Time;
use time::macros::{date, datetime};

use super::*;
use crate::accounting::record_entry;
use crate::cripto::{self, Op, Platform, Price};

const AHORA: OffsetDateTime = datetime!(2026-11-17 10:00 UTC);
const HOY: Date = date!(2026 - 11 - 17);

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn set(xs: &[&str]) -> BTreeSet<String> {
    xs.iter().map(|x| (*x).to_owned()).collect()
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

fn neobanco(v: &Vault) -> i64 {
    v.connection()
        .execute(
            "INSERT INTO bank_account(iban, number, account, name, bank)
             VALUES ('LT121000011101001000', 'LT121000011101001000', '5720009', 'Neobanco',
                     'Neobanco Demo UAB')",
            [],
        )
        .unwrap();
    v.connection().last_insert_rowid()
}

fn ingreso(v: &Vault, d: Date, e: i64) {
    record_entry(
        v.connection(),
        &JournalEntry {
            date: d,
            concept: "Traspaso".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("5720009"), eur(e)),
                EntryLine::credit(acc("5720001"), eur(e)),
            ],
        },
        d.with_time(Time::from_hms(10, 0, 0).unwrap()).assume_utc(),
    )
    .unwrap();
}

fn declarado(
    year: i32,
    bloque: Bloque,
    valor: i64,
    medio: Option<i64>,
    bienes: &[&str],
) -> Declarado {
    Declarado {
        id: 0,
        modelo: bloque.modelo().into(),
        year,
        bloque,
        valor: eur(valor),
        saldo_medio: medio.map(eur),
        bienes: bienes.iter().map(|b| (*b).to_owned()).collect(),
        origen: "tecleado".into(),
        creado: String::new(),
    }
}

#[test]
fn la_primera_vez_por_encima_de_50000_y_por_el_saldo_medio() {
    let d = decidir(
        eur(40_000),
        Some(eur(55_000)),
        &set(&["X"]),
        &set(&[]),
        None,
    );
    assert!(d.supera && d.obligado, "{d:?}");
    assert!(d.motivo.contains("saldo medio"));
    let d = decidir(
        eur(50_000),
        Some(eur(10_000)),
        &set(&["X"]),
        &set(&[]),
        None,
    );
    assert!(!d.supera && !d.obligado);
    let d = decidir(eur(50_001), None, &set(&["X"]), &set(&[]), None);
    assert!(d.obligado);
}

#[test]
fn los_anos_siguientes_solo_con_mas_de_20000_de_incremento_o_una_extincion() {
    let a = declarado(2024, Bloque::Cuentas, 60_000, Some(58_000), &["X"]);
    // Sube 20.000 justos: no.
    let d = decidir(
        eur(80_000),
        Some(eur(70_000)),
        &set(&["X"]),
        &set(&[]),
        Some(&a),
    );
    assert!(d.supera && !d.obligado, "{d:?}");
    assert_eq!(d.incremento, Some(eur(20_000)));
    // Sube más de 20.000 el saldo medio: sí.
    let d = decidir(
        eur(70_000),
        Some(eur(78_001)),
        &set(&["X"]),
        &set(&[]),
        Some(&a),
    );
    assert!(d.obligado);
    // Una cuenta declarada que ya no está: sí, aunque baje.
    let d = decidir(
        eur(30_000),
        Some(eur(30_000)),
        &set(&["Y"]),
        &set(&[]),
        Some(&a),
    );
    assert!(d.obligado);
    assert_eq!(d.extinguidos, vec!["X".to_owned()]);
    // Extinguida en el año (sigue en la lista con origen C): sí.
    let d = decidir(
        eur(0),
        Some(eur(5_000)),
        &set(&["X"]),
        &set(&["X"]),
        Some(&a),
    );
    assert!(d.obligado);
}

#[test]
fn borrador_720_de_cuentas_con_saldo_medio_y_cotejo_con_lo_declarado() {
    let v = vault(true);
    let id = neobanco(&v);
    ingreso(&v, date!(2025 - 03 - 01), 30_000);
    ingreso(&v, date!(2025 - 12 - 01), 30_000);
    let b = borrador(&v, "720", 2025, HOY).unwrap();
    let c = &b.bloques[0];
    assert_eq!(c.bloque, Bloque::Cuentas);
    assert_eq!(c.total, eur(60_000));
    // 61 días con 30.000 y 31 con 60.000: 3.690.000 / 92.
    assert_eq!(c.total_medio, Some(Money::from_cents(4_010_870)));
    assert!(c.obligado && b.obligado);
    let r = &c.registros[0];
    assert_eq!((r.clave, r.alta, r.pais.as_str()), ("C", Alta::A, "LT"));
    assert_eq!(r.entidad, "Neobanco Demo UAB");
    assert_eq!(r.fecha_apertura, Some(date!(2025 - 03 - 01)));
    assert!(r.fecha_estimada && r.faltan.contains(&"BIC".to_owned()));
    assert!(!b.fichero.disponible);
    assert!(b.fichero.nota.contains("Orden HAP/72/2013"));
    assert!(!b.exenta_por_contabilidad);

    // Datos del bien: condición, fecha y BIC.
    guardar_bien(
        &v,
        &DatosBien {
            condicion: Condicion::Autorizado,
            fecha_apertura: Some(date!(2024 - 06 - 10)),
            bic: "demolt2x".into(),
            domicilio: "Calle Inventada 1, Vilna".into(),
            ..DatosBien::vacios(Origen::Banco, id)
        },
        AHORA,
    )
    .unwrap();
    let b = borrador(&v, "720", 2025, HOY).unwrap();
    let r = &b.bloques[0].registros[0];
    assert_eq!(r.condicion, Condicion::Autorizado);
    assert_eq!(r.bic, "DEMOLT2X");
    assert!(!r.fecha_estimada && r.faltan.is_empty(), "{:?}", r.faltan);

    // Se marca como presentado. En 2026 el saldo a 31/12 sólo sube 10.000 €, pero el saldo
    // medio del cuarto trimestre sube más de 20.000 € (de 40.108,70 a 70.000): toca.
    let hechos = marcar_presentado(&v, "720", 2025, true, HOY, AHORA).unwrap();
    assert_eq!(hechos.len(), 1);
    assert_eq!(hechos[0].bienes, vec!["LT121000011101001000".to_owned()]);
    ingreso(&v, date!(2026 - 02 - 01), 10_000);
    let b = borrador(&v, "720", 2026, date!(2027 - 01 - 10)).unwrap();
    let c = &b.bloques[0];
    assert_eq!(c.incremento, Some(eur(10_000)));
    assert_eq!(c.incremento_medio, Some(Money::from_cents(2_989_130)));
    assert!(c.obligado, "{}", c.motivo);
    assert_eq!(c.registros[0].alta, Alta::M);

    // Sin confirmar no se apunta ni se quita.
    assert!(marcar_presentado(&v, "720", 2026, false, HOY, AHORA).is_err());
    let id = b.declarados[0].id;
    assert!(quitar_declarado(&v, id, false, AHORA).is_err());
    quitar_declarado(&v, id, true, AHORA).unwrap();
    assert!(declarados_de(&v, "720").unwrap().is_empty());
}

#[test]
fn una_sociedad_esta_exenta_si_cumple_los_requisitos() {
    let v = vault(false);
    let id = neobanco(&v);
    ingreso(&v, date!(2025 - 03 - 01), 80_000);
    let b = borrador(&v, "720", 2025, HOY).unwrap();
    assert!(b.exenta_por_contabilidad);
    assert_eq!(b.requisitos.len(), 1);
    assert!(b.requisitos[0].contains("NIF de la entidad"));
    guardar_bien(
        &v,
        &DatosBien {
            nif_entidad: "LT999999999".into(),
            ..DatosBien::vacios(Origen::Banco, id)
        },
        AHORA,
    )
    .unwrap();
    let b = borrador(&v, "720", 2025, HOY).unwrap();
    assert!(b.requisitos.is_empty(), "{:?}", b.requisitos);
    assert!(b.notas.iter().any(|n| n.contains("Ley 5/2022")));
}

#[test]
fn lo_tecleado_de_otro_ano_y_lo_presentado_en_la_aeat() {
    let v = vault(true);
    neobanco(&v);
    ingreso(&v, date!(2025 - 03 - 01), 60_000);
    // Bloque y modelo no cuadran.
    let mut mal = declarado(2024, Bloque::Monedas, 1, None, &[]);
    mal.modelo = "720".into();
    assert!(guardar_declarado(&v, &mal, AHORA).is_err());
    guardar_declarado(
        &v,
        &declarado(
            2023,
            Bloque::Cuentas,
            55_000,
            Some(50_000),
            &["lt12 1000 0111 0100 1000"],
        ),
        AHORA,
    )
    .unwrap();
    let b = borrador(&v, "720", 2025, HOY).unwrap();
    let c = &b.bloques[0];
    assert_eq!(c.anterior.as_ref().unwrap().year, 2023);
    assert!(!c.obligado, "{}", c.motivo);
    assert_eq!(c.registros[0].alta, Alta::M);
    // Consta un 720 de 2024 en la AEAT sin importes.
    v.connection()
        .execute(
            "INSERT INTO aeat_filing(nif, model, year, period, csv, receipt, presented_at,
                     checked_at)
             VALUES ('12345678Z', '720', 2024, '0A', 'CSVDEMO', '7201234567890', '2025-03-20',
                     '2026-11-17')",
            [],
        )
        .unwrap();
    let b = borrador(&v, "720", 2025, HOY).unwrap();
    assert_eq!(b.presentados_aeat, vec![2024]);
    assert!(b.avisos.iter().any(|a| a.contains("720 de 2024")));
}

#[test]
fn borrador_721_por_custodio() {
    let v = vault(true);
    let kraken = cripto::save_platform(
        &v,
        &Platform {
            id: 0,
            name: "Exchange Demo".into(),
            kind: "exchange".into(),
            country: "US".into(),
            abroad: true,
            account: String::new(),
        },
        AHORA,
    )
    .unwrap();
    cripto::save_op(
        &v,
        &Op {
            id: 0,
            date: date!(2025 - 02 - 10),
            time: "10:00:00".into(),
            kind: Tipo::Compra,
            platform_id: kraken,
            to_platform_id: None,
            ticker: "BTC".into(),
            qty: Qty::parse("1").unwrap(),
            ticker2: None,
            qty2: None,
            eur: eur(40_000),
            fee_eur: Money::ZERO,
            ref_price: None,
            note: String::new(),
            source: "manual".into(),
            fingerprint: None,
            entry_id: None,
        },
        AHORA,
    )
    .unwrap();
    cripto::set_prices(
        &v,
        &[Price {
            date: date!(2025 - 12 - 31),
            ticker: "BTC".into(),
            price: Precio::parse("90000").unwrap(),
            source: "manual".into(),
        }],
        AHORA,
    )
    .unwrap();
    let b = borrador(&v, "721", 2025, HOY).unwrap();
    assert_eq!(b.bloques.len(), 1);
    let m = &b.bloques[0];
    assert_eq!(
        (m.bloque, m.total, m.obligado),
        (Bloque::Monedas, eur(90_000), true)
    );
    let r = &m.registros[0];
    assert_eq!((r.clave, r.pais.as_str()), ("M", "US"));
    assert_eq!(r.fecha_apertura, Some(date!(2025 - 02 - 10)));
    assert!(r.faltan.contains(&"NIF del custodio".to_owned()));
    assert!(b.fichero.nota.contains("Orden HFP/886/2023"));
    // La hoja sale.
    let (nombre, bytes) = super::export::workbook(&v, "721", 2025, HOY).unwrap();
    assert!(nombre.contains("721") && !bytes.is_empty());
    // Presentado y al año siguiente con el mismo valor: no.
    marcar_presentado(&v, "721", 2025, true, HOY, AHORA).unwrap();
    let d = declarados_de(&v, "721").unwrap();
    assert_eq!(d[0].bienes, vec!["BTC@EXCHANGE DEMO".to_owned()]);
}

#[test]
fn modelo_no_valido() {
    let v = vault(true);
    assert!(borrador(&v, "714", 2025, HOY).is_err());
}
