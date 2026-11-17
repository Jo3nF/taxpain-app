//! Modelo 200 completo (G05) y ficha fiscal de participadas (G15): caso inventado de principio a
//! fin (Estudio Demo SL y Participada Demo SL, con cifras redondas) y casos de cada bloque.

#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use taxpain_app::accounting::record_entry;
use taxpain_app::invoicing;
use taxpain_app::modelo200::participadas::{self, DatosFiscales, Dividendo, EstadoArt21};
use taxpain_app::modelo200::{self, AjusteGuardado, ArrastreGuardado, Datos200, TipoArrastre};
use taxpain_app::models::{self, ModelSettings, YearSettings};
use taxpain_app::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
use taxpain_app::vinculadas::{self, Participada};
use taxpain_core::accounting::account::acc;
use taxpain_core::accounting::{EntryLine, JournalEntry, Origin};
use taxpain_core::invoice::{Party, PartyId, RoundingMode};
use taxpain_core::money::{Money, Rate};
use taxpain_models::m200::{Importe, TaxRegime};
use taxpain_vault::rusqlite::params;
use taxpain_vault::{AlwaysConfirm, MemoryKeyProvider, Vault};
use time::macros::date;
use time::{Date, OffsetDateTime, Time};

fn eur(e: i64) -> Money {
    Money::from_euros(e, 0)
}

fn at(d: Date) -> OffsetDateTime {
    d.with_time(Time::from_hms(10, 0, 0).unwrap()).assume_utc()
}

fn now() -> OffsetDateTime {
    at(date!(2026 - 06 - 01))
}

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
    invoicing::save_settings(&v, &s, at(date!(2025 - 01 - 01))).unwrap();
    v
}

fn cuenta(v: &Vault, code: &str, name: &str) {
    v.connection()
        .execute(
            "INSERT INTO account(code, name, created_at) VALUES (?1, ?2, '2025-01-01T00:00:00Z')
             ON CONFLICT(code) DO UPDATE SET name = excluded.name",
            params![code, name],
        )
        .unwrap();
}

fn entry(v: &Vault, d: Date, origin: Origin, lines: Vec<EntryLine>) {
    record_entry(
        v.connection(),
        &JournalEntry {
            date: d,
            concept: "Asiento de prueba".into(),
            origin,
            lines,
        },
        at(d),
    )
    .unwrap();
}

fn regimen(v: &Vault, year: i32, r: TaxRegime) {
    let mut s: ModelSettings = models::settings(v).unwrap();
    s.years.insert(
        year,
        YearSettings {
            corporate_regime: Some(r),
            ..YearSettings::default()
        },
    );
    models::save_settings(v, &s, now()).unwrap();
}

fn participada(v: &Vault, nif: &str, country: &str, pct: u32, acquired: Date) -> i64 {
    vinculadas::save_participada(
        v,
        &Participada {
            id: 0,
            nif: nif.into(),
            name: "Participada Demo SL".into(),
            province: if country == "ES" {
                "46".into()
            } else {
                String::new()
            },
            country: country.into(),
            pct,
            acquired,
            cost: eur(150_000),
            account: "2403".into(),
            grupo_fiscal: false,
            notes: String::new(),
            active: true,
        },
        now(),
    )
    .unwrap()
}

/// Estudio Demo SL en 2025: ventas 500.000, sueldos 300.000, una multa, atenciones a clientes
/// por encima del 1 %, un deterioro del inmovilizado, 40.000 de dividendos de Participada Demo
/// SL (50 % desde 2020) con una retención indebida del 19 % y 50.000 de IS contabilizado.
fn empresa() -> (Vault, i64) {
    let v = setup();
    regimen(&v, 2025, TaxRegime::ReducidaDimension);
    cuenta(&v, "6780001", "Multa de tráfico");
    cuenta(&v, "6290001", "Atenciones a clientes");
    let e = Origin::Manual;
    entry(
        &v,
        date!(2025 - 01 - 01),
        Origin::Apertura,
        vec![
            EntryLine::debit(acc("2403"), eur(150_000)),
            EntryLine::debit(acc("5720"), eur(50_000)),
            EntryLine::credit(acc("100"), eur(200_000)),
        ],
    );
    entry(
        &v,
        date!(2025 - 03 - 31),
        e.clone(),
        vec![
            EntryLine::debit(acc("4300001"), eur(500_000)),
            EntryLine::credit(acc("705"), eur(500_000)),
        ],
    );
    entry(
        &v,
        date!(2025 - 04 - 30),
        e.clone(),
        vec![
            EntryLine::debit(acc("640"), eur(300_000)),
            EntryLine::credit(acc("465"), eur(300_000)),
        ],
    );
    entry(
        &v,
        date!(2025 - 05 - 10),
        e.clone(),
        vec![
            EntryLine::debit(acc("6780001"), eur(2_000)),
            EntryLine::debit(acc("6290001"), eur(6_000)),
            EntryLine::credit(acc("5720"), eur(8_000)),
        ],
    );
    entry(
        &v,
        date!(2025 - 06 - 30),
        e.clone(),
        vec![
            EntryLine::debit(acc("5720"), eur(32_400)),
            EntryLine::debit(acc("473"), eur(7_600)),
            EntryLine::credit(acc("760"), eur(40_000)),
        ],
    );
    entry(
        &v,
        date!(2025 - 12 - 31),
        e.clone(),
        vec![
            EntryLine::debit(acc("6910"), eur(5_000)),
            EntryLine::credit(acc("2910"), eur(5_000)),
        ],
    );
    entry(
        &v,
        date!(2025 - 12 - 31),
        e,
        vec![
            EntryLine::debit(acc("6300"), eur(50_000)),
            EntryLine::credit(acc("4752"), eur(50_000)),
        ],
    );
    let id = participada(&v, "A22222228", "ES", 500_000, date!(2020 - 03 - 01));
    participadas::guardar_fiscal(
        &v,
        &DatosFiscales {
            participada_id: id,
            nominal: eur(1_500),
            valor_fiscal: None,
            origen: participadas::OrigenParticipacion::Compra,
            tributacion_minima: true,
        },
        now(),
    )
    .unwrap();
    participadas::guardar_fondos(
        &v,
        &participadas::FondosParticipada {
            participada_id: id,
            year: 2025,
            capital: eur(3_000),
            reservas: eur(200_000),
            otras_pn: Money::ZERO,
            resultado: eur(50_000),
        },
        now(),
    )
    .unwrap();
    participadas::guardar_dividendo(
        &v,
        &Dividendo {
            id: 0,
            participada_id: id,
            fecha_acuerdo: date!(2025 - 06 - 30),
            fecha_cobro: Some(date!(2025 - 06 - 30)),
            bruto: eur(40_000),
            retencion: eur(7_600),
            documento: String::new(),
            notas: String::new(),
        },
        now(),
    )
    .unwrap();
    (v, id)
}

fn arrastre(v: &Vault, tipo: TipoArrastre, origen: i32, e: i64) {
    modelo200::guardar_arrastre(
        v,
        &ArrastreGuardado {
            tipo,
            origen,
            ejercicio: 2025,
            pendiente: eur(e),
            nota: String::new(),
        },
        now(),
    )
    .unwrap();
}

fn get(d: &models::Draft, b: u16) -> Money {
    d.boxes
        .iter()
        .find(|(k, _)| *k == b)
        .map_or(Money::ZERO, |(_, m)| *m)
}

#[test]
fn caso_completo_de_principio_a_fin() {
    let (v, _) = empresa();
    arrastre(&v, TipoArrastre::Bin, 2019, 30_000);
    arrastre(&v, TipoArrastre::Bin, 2022, 20_000);
    arrastre(&v, TipoArrastre::Capitalizacion, 2024, 3_000);
    arrastre(&v, TipoArrastre::Nivelacion, 2020, 4_000);
    modelo200::guardar_datos(
        &v,
        2025,
        &Datos200 {
            aplicar_capitalizacion: true,
            incremento_fondos_propios: Some(eur(100_000)),
            nivelacion: eur(20_000),
            cuota: vec![Importe {
                casilla: 588,
                importe: eur(500),
            }],
            ..Datos200::default()
        },
        now(),
    )
    .unwrap();

    let h = modelo200::hoja(&v, 2025).unwrap();
    let linea = |ls: &[modelo200::Linea], b: u16| {
        ls.iter()
            .find(|l| l.casilla == Some(b))
            .map_or(Money::ZERO, |l| l.importe)
    };
    // Resultado: 500.000 − 300.000 − 8.000 − 5.000 + 40.000 − 50.000.
    assert_eq!(linea(&h.resultado, 500), eur(177_000));
    assert_eq!(linea(&h.resultado, 501), eur(227_000));
    // Correcciones propuestas desde la contabilidad y la ficha.
    let aj = |clave: &str| h.ajustes.iter().find(|a| a.clave == clave).unwrap();
    assert_eq!(
        (aj("multas").casilla, aj("multas").importe),
        (1815, eur(2_000))
    );
    assert_eq!(aj("atenciones").importe, eur(1_000)); // 6.000 − 1 % de 500.000
    assert_eq!(aj("deterioro_inmovilizado").importe, eur(5_000));
    let art21 = aj("art21:A22222228");
    assert_eq!((art21.casilla, art21.importe), (370, eur(38_000)));
    assert_eq!(linea(&h.base_previa, 550), eur(197_000));
    // Capitalización: 20 % de 100.000 (art. 25.1 LIS desde 2025, Ley 7/2024) + 3.000 de 2024;
    // tope 20 % de 197.000.
    assert_eq!(linea(&h.base, 1032), eur(23_000));
    let dot = h.capitalizacion.dotacion.clone().unwrap();
    assert_eq!(
        (dot.a_dotar, dot.estado.as_str()),
        (eur(23_000), "pendiente")
    );
    // Bases negativas: las dos enteras, primero la de 2019.
    assert_eq!(linea(&h.base, 547), eur(50_000));
    assert_eq!(h.bins.len(), 2);
    assert_eq!(h.bins[0].origen, 2019);
    assert_eq!(linea(&h.base, 552), eur(124_000));
    // Nivelación: 10 % de 124.000 y la de 2020 que se adiciona en su quinto año.
    assert_eq!(linea(&h.base, 1034), eur(12_400));
    assert_eq!(linea(&h.base, 1033), eur(4_000));
    assert_eq!(linea(&h.base, 1330), eur(115_600));
    // Cuota al 24 % (reducida dimensión, 2025), deducción de I+D de 500.
    assert_eq!(linea(&h.cuota, 562), eur(27_744));
    assert_eq!(linea(&h.cuota, 592), eur(27_244));
    // Retención de la participada: capital mobiliario.
    assert_eq!(linea(&h.retenciones, 1785), eur(7_600));
    assert_eq!(linea(&h.retenciones, 1766), eur(7_600));
    let r599 = linea(&h.liquidacion, 599);
    assert_eq!(r599, eur(19_644));
    let pagos =
        linea(&h.liquidacion, 601) + linea(&h.liquidacion, 603) + linea(&h.liquidacion, 605);
    assert_eq!(linea(&h.liquidacion, 1586), r599 - pagos);
    assert_eq!(h.patrimonial, Some(false));
    assert!(
        h.avisos.iter().any(|a| a.contains("no procedía retención")),
        "{:?}",
        h.avisos
    );

    // El borrador del modelo y su fichero dicen lo mismo, con las participadas y la página 15.
    let d = models::draft(&v, "200", 2025, 0).unwrap();
    assert_eq!(get(&d, 370), eur(38_000));
    assert_eq!(get(&d, 1330), eur(115_600));
    assert_eq!(d.result, r599 - pagos);
    assert!(d.file.contains("<T20002000>"), "falta la página 02");
    assert!(d.file.contains("<T20015000>"), "falta la página 15");

    // Paso al ejercicio siguiente: sólo queda la minoración de nivelación de 2025.
    let n = modelo200::pasar_al_siguiente(&v, 2025, now(), true).unwrap();
    assert_eq!(n, 1);
    let sig = modelo200::arrastres(&v, 2026).unwrap();
    assert_eq!(sig[0].tipo, TipoArrastre::Nivelacion);
    assert_eq!((sig[0].origen, sig[0].pendiente), (2025, eur(12_400)));
    assert!(modelo200::pasar_al_siguiente(&v, 2025, now(), false).is_err());
}

#[test]
fn ajustes_editables_descartables_y_a_mano() {
    let (v, _) = empresa();
    let base = |v: &Vault| {
        let h = modelo200::hoja(v, 2025).unwrap();
        h.base_previa
            .iter()
            .find(|l| l.casilla == Some(550))
            .unwrap()
            .importe
    };
    assert_eq!(base(&v), eur(197_000));
    // Descartar la multa y corregir las atenciones a 500.
    modelo200::guardar_ajuste(
        &v,
        2025,
        &AjusteGuardado {
            clave: "multas".into(),
            casilla: 1815,
            importe: None,
            concepto: String::new(),
        },
        now(),
    )
    .unwrap();
    modelo200::guardar_ajuste(
        &v,
        2025,
        &AjusteGuardado {
            clave: "atenciones".into(),
            casilla: 339,
            importe: Some(eur(500)),
            concepto: String::new(),
        },
        now(),
    )
    .unwrap();
    assert_eq!(base(&v), eur(197_000 - 2_000 - 500));
    // Una a mano: gastos de actuaciones contrarias al ordenamiento (art. 15.f).
    let clave = modelo200::guardar_ajuste(
        &v,
        2025,
        &AjusteGuardado {
            clave: String::new(),
            casilla: 1816,
            importe: Some(eur(1_000)),
            concepto: "Gasto inventado".into(),
        },
        now(),
    )
    .unwrap();
    assert!(clave.starts_with("manual:"));
    assert_eq!(base(&v), eur(195_500));
    // Casilla que no es del catálogo: error.
    assert!(
        modelo200::guardar_ajuste(
            &v,
            2025,
            &AjusteGuardado {
                clave: String::new(),
                casilla: 9999,
                importe: Some(eur(1)),
                concepto: String::new(),
            },
            now(),
        )
        .is_err()
    );
    // Volver a la propuesta de la multa.
    modelo200::quitar_ajuste(&v, 2025, "multas", now(), true).unwrap();
    assert_eq!(base(&v), eur(197_500));
}

#[test]
fn art21_condicionado_no_residente_y_sin_el_cinco_por_ciento() {
    let (v, id) = empresa();
    let fichas = participadas::fichas(&v, 2025).unwrap();
    let f = fichas.iter().find(|f| f.participada.id == id).unwrap();
    assert_eq!(f.dividendos[0].art21.estado, EstadoArt21::Cumple);
    assert_eq!(f.exento, eur(38_000));
    assert_eq!(f.dividendos[0].retencion_procede, Money::ZERO);
    // Comprada hace menos de un año: condicionada a mantenerla.
    let p = Participada {
        acquired: date!(2025 - 03 - 01),
        ..f.participada.clone()
    };
    let a = participadas::art21(&p, &f.fiscal, date!(2025 - 06 - 30));
    assert_eq!(a.estado, EstadoArt21::Condicionada);
    assert_eq!(a.ano_cumplido, date!(2026 - 03 - 01));
    // Un 3 %: no cumple.
    let p = Participada {
        pct: 30_000,
        ..f.participada.clone()
    };
    assert_eq!(
        participadas::art21(&p, &f.fiscal, date!(2025 - 06 - 30)).estado,
        EstadoArt21::NoCumple
    );
    // No residente sin tributación mínima: no cumple.
    let p = Participada {
        country: "PT".into(),
        ..f.participada.clone()
    };
    let sin = DatosFiscales {
        tributacion_minima: false,
        ..f.fiscal.clone()
    };
    assert_eq!(
        participadas::art21(&p, &sin, date!(2025 - 06 - 30)).estado,
        EstadoArt21::NoCumple
    );
    // Antes de 2021 la exención era plena.
    assert_eq!(
        participadas::art21(&f.participada, &f.fiscal, date!(2020 - 06 - 30)).exento_pct,
        100
    );
    // Dividendos: validación y borrado con confirmación.
    let mal = Dividendo {
        id: 0,
        participada_id: id,
        fecha_acuerdo: date!(2025 - 07 - 01),
        fecha_cobro: Some(date!(2025 - 06 - 01)),
        bruto: eur(100),
        retencion: Money::ZERO,
        documento: String::new(),
        notas: String::new(),
    };
    assert!(participadas::guardar_dividendo(&v, &mal, now()).is_err());
    let d = f.dividendos[0].dividendo.id;
    assert!(participadas::borrar_dividendo(&v, d, now(), false).is_err());
    participadas::borrar_dividendo(&v, d, now(), true).unwrap();
    let h = modelo200::hoja(&v, 2025).unwrap();
    assert!(h.avisos.iter().any(|a| a.contains("760")), "{:?}", h.avisos);
}

#[test]
fn entidad_patrimonial_liquida_al_tipo_general() {
    let v = setup();
    regimen(&v, 2025, TaxRegime::Micro);
    // Casi todo el activo es tesorería sin afectar: entidad patrimonial.
    entry(
        &v,
        date!(2025 - 01 - 01),
        Origin::Apertura,
        vec![
            EntryLine::debit(acc("5720"), eur(1_000_000)),
            EntryLine::credit(acc("100"), eur(1_000_000)),
        ],
    );
    entry(
        &v,
        date!(2025 - 06 - 30),
        Origin::Manual,
        vec![
            EntryLine::debit(acc("5720"), eur(100_000)),
            EntryLine::credit(acc("752"), eur(100_000)),
        ],
    );
    let h = modelo200::hoja(&v, 2025).unwrap();
    assert_eq!(h.patrimonial, Some(true));
    assert_eq!(h.regimen_propuesto, TaxRegime::General);
    assert_eq!(h.regimen_aplicado, TaxRegime::General);
    let cuota = h.cuota.iter().find(|l| l.casilla == Some(562)).unwrap();
    assert_eq!(cuota.importe, eur(25_000));
    assert!(h.avisos.iter().any(|a| a.contains("Entidad patrimonial")));
    let d = models::draft(&v, "200", 2025, 0).unwrap();
    assert_eq!(get(&d, 562), eur(25_000));
}

#[test]
fn arrastres_validan_y_se_borran_con_confirmacion() {
    let v = setup();
    let mal = ArrastreGuardado {
        tipo: TipoArrastre::Bin,
        origen: 2027,
        ejercicio: 2026,
        pendiente: eur(1),
        nota: String::new(),
    };
    assert!(modelo200::guardar_arrastre(&v, &mal, now()).is_err());
    let bien = ArrastreGuardado {
        origen: 2023,
        ..mal
    };
    modelo200::guardar_arrastre(&v, &bien, now()).unwrap();
    assert_eq!(modelo200::arrastres(&v, 2026).unwrap().len(), 1);
    assert!(modelo200::borrar_arrastre(&v, TipoArrastre::Bin, 2023, 2026, now(), false).is_err());
    modelo200::borrar_arrastre(&v, TipoArrastre::Bin, 2023, 2026, now(), true).unwrap();
    assert!(modelo200::arrastres(&v, 2026).unwrap().is_empty());
    // Datos del ejercicio: casillas de la cuota validadas.
    let d = Datos200 {
        cuota: vec![Importe {
            casilla: 1785,
            importe: eur(1),
        }],
        ..Datos200::default()
    };
    assert!(modelo200::guardar_datos(&v, 2026, &d, now()).is_err());
}
