//! La empresa año a año con lo que tiene la AEAT y lo que hay que revisar (cruces entre modelos
//! sin carpeta). Datos inventados.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::evolucion;
use taxpain_app::invoicing;
use taxpain_app::presentado_aeat::{self, Answer, Download, Presented, Visor};
use taxpain_app::revision;
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_core::calendar::TaxpayerProfile;
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_models::dr_versions::file_with_boxes;
use taxpain_vault::rusqlite::params;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::{date, datetime};

const NIF: &str = "B12345674";

fn setup() -> Vault {
    let v = Vault::open_in_memory(
        Arc::new(MemoryKeyProvider::default()),
        Arc::new(AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif(NIF.into()),
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
    v.set_setting("taxpayer_profile", &TaxpayerProfile::sociedad_limitada())
        .unwrap();
    v
}

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn p(model: &str, year: i32, period: &str, n: u32, at: &str) -> Presented {
    Presented {
        model: model.into(),
        year,
        period: period.into(),
        csv: format!("{:X<16}", format!("C{model}{year}{period}{n}")),
        file_number: String::new(),
        receipt: format!("{model}{year}{n:06}"),
        previous_receipt: None,
        presented_at: at.into(),
    }
}

/// Guarda las declaraciones de un año y sus ficheros (con el diseño de su año).
fn presentar(v: &Vault, year: i32, list: Vec<(Presented, Vec<(u16, Money)>)>) {
    let a = Answer {
        models: list.iter().map(|(x, _)| x.model.clone()).collect(),
        list: list.iter().map(|(x, _)| x.clone()).collect(),
        ..Answer::default()
    };
    presentado_aeat::record(v, NIF, year, &a, datetime!(2026-10-27 10:00 UTC)).unwrap();
    for (x, boxes) in list {
        if let Some(f) = file_with_boxes(&x.model, x.year, &x.period, &boxes) {
            presentado_aeat::attach(
                v,
                &x.csv,
                Download::Pdf(b"%PDF-1.4 justificante"),
                Visor::File(&f),
                datetime!(2026-10-27 10:00 UTC),
            )
            .unwrap();
        }
    }
}

/// Casillas de una declaración que TaxPain no escribe (el 200, las informativas): directas.
fn casillas(v: &Vault, csv: &str, json: &str) {
    v.connection()
        .execute(
            "UPDATE aeat_filing SET boxes = ?1, boxes_source = 'fichero' WHERE csv = ?2",
            params![json, csv],
        )
        .unwrap();
}

/// Un 303 que cuadra: [03] y [27] devengado, [29] y [45] deducible, [46] y [71] resultado.
fn m303(devengado: i64, deducible: i64) -> Vec<(u16, Money)> {
    vec![
        (3, eur(devengado)),
        (27, eur(devengado)),
        (29, eur(deducible)),
        (45, eur(deducible)),
        (46, eur(devengado - deducible)),
        (64, eur(devengado - deducible)),
        (66, eur(devengado - deducible)),
        (69, eur(devengado - deducible)),
        (71, eur(devengado - deducible)),
    ]
}

#[test]
fn evolucion_año_a_año_y_cruces_entre_modelos() {
    let v = setup();
    // 2024: sólo el 390 (volumen 100.000).
    let y24 = p("390", 2024, "0A", 1, "2025-01-30T10:00:00");
    presentar(
        &v,
        2024,
        vec![(
            y24,
            vec![
                (108, eur(100_000)),
                (47, eur(2_100)),
                (64, eur(1_000)),
                (84, eur(1_100)),
                (86, eur(1_100)),
            ],
        )],
    );
    // 2025: cuatro 303, el 390 (con 210 € más de devengado que la suma de los 303), un 111, su
    // 190 (con otra cifra) y el 200.
    let mut l = Vec::new();
    for (q, at) in [
        ("1T", "2025-04-15"),
        ("2T", "2025-07-15"),
        ("3T", "2025-10-15"),
        ("4T", "2026-01-29"),
    ] {
        l.push((
            p("303", 2025, q, 1, &format!("{at}T10:00:00")),
            m303(1_000, 400),
        ));
    }
    l.push((
        p("390", 2025, "0A", 2, "2026-01-29T10:00:00"),
        vec![
            (108, eur(150_000)),
            (47, eur(4_210)),
            (64, eur(1_600)),
            (84, eur(2_610)),
            (86, eur(2_610)),
        ],
    ));
    // El 111 del 1T, presentado en mayo (fuera de plazo).
    l.push((
        p("111", 2025, "1T", 3, "2025-05-30T10:00:00"),
        vec![(28, eur(300)), (30, eur(300))],
    ));
    l.push((p("190", 2025, "0A", 4, "2026-01-29T10:00:00"), vec![]));
    l.push((p("200", 2025, "0A", 5, "2026-07-24T10:00:00"), vec![]));
    let csv190 = l[6].0.csv.clone();
    let csv200 = l[7].0.csv.clone();
    presentar(&v, 2025, l);
    casillas(
        &v,
        &csv190,
        r#"{"registros":2,"importe":500000,"retenciones":25000}"#,
    );
    casillas(
        &v,
        &csv200,
        r#"{"casillas":{"255":14000000,"500":2000000,"552":2000000,"562":500000,"1586":300000,"180":9000000,"186":6000000}}"#,
    );

    let e = evolucion::evolucion(&v, date!(2026 - 10 - 27)).unwrap();
    assert_eq!(e.años, [2024, 2025]);
    assert!(!e.sin_comprobar);
    let fila = |id: &str| {
        e.grupos
            .iter()
            .flat_map(|g| g.filas.iter())
            .find(|f| f.id == id)
            .unwrap_or_else(|| panic!("sin fila {id}"))
    };
    // El volumen, del 390, con su variación frente a 2024 (+50 %).
    let vol = fila("volumen");
    assert_eq!(vol.valores[0].valor, Some(eur(100_000).cents()));
    assert_eq!(vol.valores[1].valor, Some(eur(150_000).cents()));
    assert_eq!(vol.valores[1].diferencia, Some(eur(50_000).cents()));
    assert_eq!(vol.valores[1].variacion, Some(500));
    assert_eq!(vol.valores[1].fuente.as_deref(), Some("390 [108]"));
    // El resultado del IVA, del 390 [84] (sin compensaciones de otros años).
    let res = fila("iva_resultado");
    assert_eq!(res.valores[1].valor, Some(eur(2_610).cents()));
    assert_eq!(res.valores[1].fuente.as_deref(), Some("390 [84]"));
    // Retenciones: del 111 (no del 190); los perceptores, del 190.
    let ret = fila("ret_trabajo");
    assert_eq!(ret.valores[1].valor, Some(eur(300).cents()));
    assert_eq!(fila("perceptores").valores[1].valor, Some(2));
    assert!(fila("perceptores").recuento);
    // Del 200.
    assert_eq!(
        fila("cifra_negocios").valores[1].valor,
        Some(eur(140_000).cents())
    );
    assert_eq!(
        fila("cifra_negocios").valores[1].fuente.as_deref(),
        Some("200 [00255]")
    );
    assert_eq!(
        fila("cuota_integra").valores[1].valor,
        Some(eur(5_000).cents())
    );
    // 2024 sin 200: sin dato, no cero.
    assert_eq!(fila("cifra_negocios").valores[0].valor, None);
    // Declaraciones y fuera de plazo (el 111 del 1T).
    assert_eq!(fila("presentadas").valores[1].valor, Some(8));
    assert_eq!(fila("fuera_de_plazo").valores[1].valor, Some(1));

    // Qué revisar: el 390 no cuadra con los 303 y el 190 declara menos que el 111.
    let r = revision::revisar_aeat(&v, date!(2026 - 10 - 27)).unwrap();
    let codigos: Vec<&str> = r.hallazgos.iter().map(|h| h.codigo.as_str()).collect();
    assert!(codigos.contains(&"cruces.390_303"), "{codigos:?}");
    assert!(codigos.contains(&"cruces.resumen_anual"), "{codigos:?}");
    assert!(codigos.contains(&"cruces.fuera_de_plazo"), "{codigos:?}");
    // Sin carpeta no se compara con facturas ni se piden justificantes del Registro.
    assert!(
        !codigos.iter().any(|c| c.starts_with("modelos.303_")
            || c.contains("sin_facturas")
            || c.contains("sin_justificante")),
        "{codigos:?}"
    );
    // Los ficheros de los hallazgos dicen que es de la AEAT.
    let h = r
        .hallazgos
        .iter()
        .find(|h| h.codigo == "cruces.390_303")
        .unwrap();
    assert!(
        h.ficheros.iter().any(|f| f.starts_with("AEAT · modelo")),
        "{:?}",
        h.ficheros
    );
}

#[test]
fn sin_nada_comprobado() {
    let v = setup();
    let e = evolucion::evolucion(&v, date!(2026 - 10 - 27)).unwrap();
    assert!(e.sin_comprobar);
    assert!(e.grupos.is_empty());
}

#[test]
fn grupos_de_lo_mas_importante_a_lo_menos() {
    use taxpain_core::accounting::account::acc;
    use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
    let v = setup();
    // 2025: el 390, un 111, un 202 y el 200; y un asiento en la contabilidad de TaxPain.
    let l = vec![
        (
            p("390", 2025, "0A", 1, "2026-01-29T10:00:00"),
            vec![(108, eur(150_000)), (47, eur(4_200)), (64, eur(1_600))],
        ),
        (
            p("111", 2025, "1T", 2, "2025-04-15T10:00:00"),
            vec![(28, eur(300)), (30, eur(300))],
        ),
        (
            p("202", 2025, "1P", 3, "2025-04-15T10:00:00"),
            vec![(3, eur(1_000))],
        ),
        (p("200", 2025, "0A", 4, "2026-07-24T10:00:00"), vec![]),
    ];
    let csv200 = l[3].0.csv.clone();
    presentar(&v, 2025, l);
    casillas(
        &v,
        &csv200,
        r#"{"casillas":{"255":14000000,"296":2500000,"500":2000000,"552":2000000,"562":500000,"1586":300000}}"#,
    );
    taxpain_app::accounting::manual_entry(
        &v,
        &JournalEntry {
            date: date!(2025 - 03 - 01),
            concept: "Venta".into(),
            origin: Origin::Manual,
            lines: vec![
                EntryLine::debit(acc("430"), eur(1_000)),
                EntryLine::credit(acc("705"), eur(1_000)),
            ],
        },
        datetime!(2025-03-01 10:00 UTC),
    )
    .unwrap();

    let e = evolucion::evolucion(&v, date!(2026 - 10 - 27)).unwrap();
    let titulos: Vec<&str> = e.grupos.iter().map(|g| g.titulo).collect();
    assert_eq!(
        titulos,
        [
            "Actividad y cuentas",
            "Impuesto sobre Sociedades",
            "IVA",
            "Retenciones",
            "Declaraciones",
            "Contabilidad de TaxPain",
        ]
    );
    // Dentro de «Actividad y cuentas», la cifra de negocios primero.
    let ids: Vec<&str> = e.grupos[0].filas.iter().map(|f| f.id).collect();
    assert_eq!(
        ids,
        [
            "cifra_negocios",
            "resultado_explotacion",
            "resultado_antes",
            "resultado",
            "fondos_propios",
            "efectivo",
            "total_activo",
            "gastos_personal",
            "otros_gastos",
        ]
    );
    // Los pagos fraccionados (202), con el Impuesto sobre Sociedades.
    let ids: Vec<&str> = e.grupos[1].filas.iter().map(|f| f.id).collect();
    assert_eq!(
        ids,
        [
            "bin",
            "base_imponible",
            "cuota_integra",
            "pagos_fraccionados",
            "resultado_is",
        ]
    );
}

/// Sin 390, el resultado del IVA del año suma los [64] de los 303, no los [71]: el [71] ya
/// resta lo compensado de periodos anteriores y la suma contaría dos veces la compensación.
#[test]
fn resultado_del_iva_sin_390_no_cuenta_las_compensaciones() {
    let v = setup();
    let t1 = p("303", 2026, "1T", 1, "2026-04-15T10:00:00");
    let t2 = p("303", 2026, "2T", 2, "2026-07-15T10:00:00");
    let (c1, c2) = (t1.csv.clone(), t2.csv.clone());
    presentar(&v, 2026, vec![(t1, vec![]), (t2, vec![])]);
    // 1T: 1.000 a compensar. 2T: 3.000 de resultado, 1.000 compensados, 2.000 a ingresar.
    casillas(
        &v,
        &c1,
        r#"{"casillas":{"27":100000,"45":200000,"46":-100000,"64":-100000,"71":-100000}}"#,
    );
    casillas(
        &v,
        &c2,
        r#"{"casillas":{"27":400000,"45":100000,"46":300000,"64":300000,"78":100000,"71":200000}}"#,
    );
    let e = evolucion::evolucion(&v, date!(2026 - 10 - 27)).unwrap();
    let res = e
        .grupos
        .iter()
        .flat_map(|g| g.filas.iter())
        .find(|f| f.id == "iva_resultado")
        .unwrap();
    // 3.000 − 1.000 = 2.000 (no 1.000, que saldría de sumar los [71]).
    assert_eq!(res.valores[0].valor, Some(eur(2_000).cents()));
    assert_eq!(
        res.valores[0].fuente.as_deref(),
        Some("suma de los 303 [64]")
    );
}

#[test]
fn el_año_sin_200_sale_como_avance_con_las_ventas_de_los_303() {
    let v = setup();
    let y25 = p("200", 2025, "0A", 1, "2026-07-24T10:00:00");
    let csv200 = y25.csv.clone();
    presentar(&v, 2025, vec![(y25, vec![])]);
    casillas(&v, &csv200, r#"{"casillas":{"255":10000000}}"#);
    // 2026, sin 200 aún: dos 303 con ventas al 21 % [01] y a otros países de la UE [59].
    let mut l = Vec::new();
    for (q, at, base, ue) in [
        ("2T", "2026-07-15", 30_000, 0),
        ("1T", "2026-04-15", 20_000, 5_000),
    ] {
        let mut b = m303(base * 21 / 100, 100);
        b.extend([(1, eur(base)), (2, eur(base * 21 / 100)), (59, eur(ue))]);
        l.push((p("303", 2026, q, 2, &format!("{at}T10:00:00")), b));
    }
    presentar(&v, 2026, l);

    let e = evolucion::evolucion(&v, date!(2026 - 10 - 27)).unwrap();
    let f = e
        .grupos
        .iter()
        .flat_map(|g| g.filas.iter())
        .find(|f| f.id == "cifra_negocios")
        .unwrap();
    assert_eq!(e.años, [2025, 2026]);
    assert!(!f.valores[0].avance);
    let a = &f.valores[1];
    // 20.000 + 5.000 + 30.000 = 55.000, sin compararlo con el año entero de 2025.
    assert_eq!(a.valor, Some(eur(55_000).cents()));
    assert!(a.avance);
    assert_eq!((a.diferencia, a.variacion), (None, None));
    assert_eq!(
        a.fuente.as_deref(),
        Some("avance con los 303 1T y 2T (sin 200)")
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
