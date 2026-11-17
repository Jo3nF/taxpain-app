//! Entidad patrimonial (art. 5.2 LIS) con la media de los balances trimestrales y avisos del
//! patrimonio neto frente al capital (arts. 327 y 363.1.e LSC). Balances inventados de Estudio
//! Demo SL (B12345674) y de una SA de prueba (A22222228).

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::manual_entry;
use taxpain_app::companies::{self, CompanyInfo};
use taxpain_app::patrimonial::{
    Ajustes, Clase, anadir_venta, guardar_ajustes, informe, marcar, quitar_venta, senal,
};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};
use time::{Date, OffsetDateTime};

const NOW: OffsetDateTime = datetime!(2027-01-15 10:00 UTC);
const HOY: Date = date!(2027 - 01 - 15);

fn m(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn vault(nif: &str, nombre: &str) -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    companies::set_info(
        &v,
        &CompanyInfo {
            name: nombre.into(),
            nif: Some(nif.into()),
        },
        NOW,
    )
    .unwrap();
    v
}

/// Asiento manual de dos líneas: `debe` contra `haber`.
fn asiento(v: &Vault, fecha: Date, debe: &str, haber: &str, euros: i64) {
    manual_entry(
        v,
        &JournalEntry {
            date: fecha,
            concept: format!("{debe} a {haber}"),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc(debe), m(euros)),
                EntryLine::credit(acc(haber), m(euros)),
            ],
        },
        NOW,
    )
    .unwrap();
}

/// Estudio Demo SL: capital 100.000 en el banco el 2 de enero, un local afecto de 60.000 y un
/// fondo de inversión de 30.000 comprado el 1 de abril.
fn estudio_demo() -> Vault {
    let v = vault("B12345674", "Estudio Demo SL");
    asiento(&v, date!(2026 - 01 - 02), "5720001", "1000000", 100_000);
    asiento(&v, date!(2026 - 01 - 03), "2110001", "5720001", 60_000);
    asiento(&v, date!(2026 - 04 - 01), "5400001", "5720001", 30_000);
    v
}

#[test]
fn por_debajo_del_50_con_la_tesoreria_justificada() {
    let v = estudio_demo();
    // Tesorería 40.000 en el primer trimestre y 10.000 después; 10.000 justificados.
    guardar_ajustes(
        &v,
        2026,
        &Ajustes {
            tesoreria_afecta: m(10_000),
            tesoreria_motivo: "Tres meses de gastos".into(),
            ..Ajustes::default()
        },
        NOW,
    )
    .unwrap();
    let i = informe(&v, 2026, HOY).unwrap();
    assert_eq!(i.trimestres.len(), 4);
    let t1 = &i.trimestres[0];
    assert_eq!(t1.activo, m(100_000));
    assert_eq!(t1.no_afecto, m(30_000));
    assert_eq!(t1.tesoreria_justificada, m(10_000));
    let t2 = &i.trimestres[1];
    assert_eq!((t2.valores, t2.no_afecto), (m(30_000), Money::ZERO));
    // Media: computa (30.000 + 30.000 × 3) / 4 = 30.000 de 100.000.
    assert_eq!(i.media.computa, m(30_000));
    assert_eq!(i.media.pct, Rate::percent(30));
    assert!(!i.patrimonial);
    assert_eq!(i.media.margen, m(20_000));
    assert_eq!(senal(&v, 2026, HOY), Some((30, 22)));
}

#[test]
fn por_encima_del_50_con_un_inmueble_no_afecto() {
    let v = estudio_demo();
    // El local no se usa en la actividad: no afecto, con su motivo.
    marcar(
        &v,
        2026,
        "2110001",
        Clase::NoAfecto,
        "Sin uso desde 2025",
        NOW,
    )
    .unwrap();
    let i = informe(&v, 2026, HOY).unwrap();
    // T1: 60.000 + 40.000 de tesorería no justificada = 100 %; T2-T4: 60.000 + 30.000 + 10.000.
    assert_eq!(i.trimestres[0].pct, Rate::percent(100));
    assert_eq!(i.trimestres[3].pct, Rate::percent(100));
    assert!(i.patrimonial);
    let p = i.partidas.iter().find(|p| p.cuenta == "2110001").unwrap();
    assert!(p.marcada);
    assert_eq!((p.clase, p.propuesta), (Clase::NoAfecto, Clase::Afecto));
    assert!(!i.consecuencias.is_empty());
    // «Ahorrar impuestos» usa este cálculo.
    assert_eq!(senal(&v, 2026, HOY), Some((100, 82)));
    // La marca sigue en el ejercicio siguiente.
    asiento(&v, date!(2027 - 01 - 02), "2110001", "1000000", 60_000);
    let siguiente = informe(&v, 2027, HOY).unwrap();
    let p = siguiente
        .partidas
        .iter()
        .find(|p| p.cuenta == "2110001")
        .unwrap();
    assert_eq!((p.clase, p.desde), (Clase::NoAfecto, Some(2026)));
}

#[test]
fn las_participaciones_de_la_letra_d_no_computan() {
    let v = vault("B12345674", "Estudio Demo SL");
    asiento(&v, date!(2026 - 01 - 02), "5720001", "1000000", 100_000);
    // 70.000 en una participada del grupo (al menos el 5 %, desde hace años, con medios).
    asiento(&v, date!(2026 - 01 - 02), "2403001", "5720001", 70_000);
    guardar_ajustes(
        &v,
        2026,
        &Ajustes {
            tesoreria_afecta: m(30_000),
            ..Ajustes::default()
        },
        NOW,
    )
    .unwrap();
    let antes = informe(&v, 2026, HOY).unwrap();
    assert_eq!(antes.media.pct, Rate::percent(70));
    assert!(antes.patrimonial);
    marcar(
        &v,
        2026,
        "2403001",
        Clase::Participacion,
        "40 % de Demo Servicios SL desde 2019; dos personas de la casa la dirigen",
        NOW,
    )
    .unwrap();
    let despues = informe(&v, 2026, HOY).unwrap();
    assert_eq!(despues.media.pct, Rate::ZERO);
    assert_eq!(despues.media.participaciones, m(70_000));
    assert_eq!(despues.media.pct_con_participaciones, Rate::percent(70));
    assert!(!despues.patrimonial);
    assert_eq!(senal(&v, 2026, HOY), Some((0, 0)));
}

#[test]
fn la_tesoreria_de_la_venta_de_un_inmovilizado_afecto_no_computa() {
    let v = vault("B12345674", "Estudio Demo SL");
    asiento(&v, date!(2026 - 01 - 02), "5720001", "1000000", 20_000);
    asiento(&v, date!(2026 - 01 - 02), "2130001", "1000000", 80_000);
    // Vende la maquinaria afecta el 15 de mayo por 80.000 (sin beneficio, para simplificar).
    asiento(&v, date!(2026 - 05 - 15), "5720001", "2130001", 80_000);
    let sin = informe(&v, 2026, HOY).unwrap();
    // T2-T4: 100.000 de tesorería, todo no afecto.
    assert_eq!(sin.trimestres[1].pct, Rate::percent(100));
    assert!(sin.patrimonial);
    let id = anadir_venta(
        &v,
        date!(2026 - 05 - 15),
        m(80_000),
        "Venta de la maquinaria",
        NOW,
    )
    .unwrap();
    let con = informe(&v, 2026, HOY).unwrap();
    // T1 sin cambios (la venta es posterior): 20.000 de 100.000.
    assert_eq!(con.trimestres[0].ventas_excluidas, Money::ZERO);
    assert_eq!(con.trimestres[0].pct, Rate::percent(20));
    // T2: de 100.000 de tesorería, 80.000 son de la venta.
    assert_eq!(con.trimestres[1].ventas_excluidas, m(80_000));
    assert_eq!(con.trimestres[1].pct, Rate::percent(20));
    assert!(!con.patrimonial);
    // La venta vale también dos ejercicios después, pero no tres.
    assert_eq!(informe(&v, 2028, HOY).unwrap().ventas.len(), 1);
    assert!(informe(&v, 2029, HOY).unwrap().ventas.is_empty());
    assert!(quitar_venta(&v, id, NOW, false).is_err());
    quitar_venta(&v, id, NOW, true).unwrap();
    assert!(informe(&v, 2026, HOY).unwrap().ventas.is_empty());
}

#[test]
fn causa_de_disolucion_por_perdidas_en_una_sl() {
    let v = estudio_demo();
    // Pérdidas de 60.000 en el año: el patrimonio neto queda en 40.000, menos de la mitad del
    // capital de 100.000.
    asiento(&v, date!(2026 - 10 - 15), "6290000", "5720001", 60_000);
    let i = informe(&v, 2026, HOY).unwrap();
    let pn = i.patrimonio.fechas.last().unwrap();
    assert_eq!((pn.capital, pn.mercantil), (m(100_000), m(40_000)));
    let codigos: Vec<&str> = i.patrimonio.avisos.iter().map(|a| a.codigo).collect();
    assert!(codigos.contains(&"causa_disolucion"), "{codigos:?}");
    assert!(codigos.contains(&"sin_dividendos"));
    // En la SL no hay reducción obligatoria del art. 327.
    assert!(!codigos.contains(&"reduccion_obligatoria"));
    let causa = i
        .patrimonio
        .avisos
        .iter()
        .find(|a| a.codigo == "causa_disolucion")
        .unwrap();
    assert_eq!(causa.plazo, Some(date!(2027 - 02 - 28)));
    assert!(causa.norma.contains("363.1.e"));
    assert!(causa.detalle.contains("dos meses"));
    // También en los avisos del Panel.
    v.set_setting(
        "taxpayer_profile",
        &taxpain_core::calendar::TaxpayerProfile::sociedad_limitada(),
    )
    .unwrap();
    let alertas = taxpain_app::umbrales::alerts(&v, HOY).unwrap();
    assert!(alertas.iter().any(|a| a.code == "causa_disolucion"));

    // Un préstamo participativo de 20.000 computa en el patrimonio neto: ya no es causa.
    guardar_ajustes(
        &v,
        2026,
        &Ajustes {
            prestamos_participativos: m(20_000),
            ..Ajustes::default()
        },
        NOW,
    )
    .unwrap();
    let i = informe(&v, 2026, HOY).unwrap();
    assert_eq!(i.patrimonio.fechas.last().unwrap().mercantil, m(60_000));
    assert!(
        !i.patrimonio
            .avisos
            .iter()
            .any(|a| a.codigo == "causa_disolucion")
    );
}

#[test]
fn reduccion_obligatoria_en_una_sa_tras_un_ejercicio_sin_recuperarse() {
    let v = vault("A22222228", "Demo Anónima SA");
    asiento(&v, date!(2025 - 01 - 02), "5720001", "1000000", 90_000);
    // 2025: pérdidas de 35.000 (patrimonio neto 55.000 < 60.000, dos tercios del capital).
    asiento(&v, date!(2025 - 06 - 30), "6290000", "5720001", 35_000);
    let i = informe(&v, 2025, HOY).unwrap();
    let codigos: Vec<&str> = i.patrimonio.avisos.iter().map(|a| a.codigo).collect();
    assert!(codigos.contains(&"dos_tercios"), "{codigos:?}");
    assert!(!codigos.contains(&"reduccion_obligatoria"));

    // 2026 (sin cerrar 2025: el saldo viene de la apertura manual): sigue en 55.000.
    asiento(&v, date!(2026 - 01 - 01), "5720001", "1000000", 90_000);
    asiento(&v, date!(2026 - 01 - 01), "1210000", "5720001", 35_000);
    let i = informe(&v, 2026, HOY).unwrap();
    assert_eq!(i.patrimonio.anterior.as_ref().unwrap().mercantil, m(55_000));
    let codigos: Vec<&str> = i.patrimonio.avisos.iter().map(|a| a.codigo).collect();
    assert!(codigos.contains(&"reduccion_obligatoria"), "{codigos:?}");
    assert!(!codigos.contains(&"causa_disolucion"));
}

#[test]
fn un_autonomo_no_tiene_test_de_patrimonial() {
    let v = vault("12345678Z", "Carmen Española Española");
    assert!(informe(&v, 2026, HOY).is_err());
    assert_eq!(senal(&v, 2026, HOY), None);
}

#[test]
fn marcar_valida_la_cuenta_y_queda_en_la_auditoria() {
    let v = estudio_demo();
    assert!(marcar(&v, 2026, "1000000", Clase::NoAfecto, "", NOW).is_err());
    assert!(marcar(&v, 2026, "4300001", Clase::NoAfecto, "", NOW).is_err());
    marcar(&v, 2026, "430", Clase::Afecto, "", NOW).unwrap();
    let n: i64 = v
        .connection()
        .query_row(
            "SELECT count(*) FROM audit_log WHERE action LIKE 'patrimonial.%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(n, 1);
}
