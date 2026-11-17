use taxpain_core::money::Money;
use taxpain_models::m200::TaxRegime;
use time::macros::date;

use super::catalogo::FICHAS;
use super::*;

fn e(euros: i64) -> Money {
    Money::from_euros(euros, 0)
}

/// Una sociedad pequeña con beneficio: 100.000 € de base y 60.000 € de beneficio el año pasado.
fn ctx() -> Ctx {
    Ctx {
        hoy: date!(2026 - 10 - 30),
        año: 2026,
        cierre: date!(2026 - 12 - 31),
        dias: 365,
        fisica: false,
        regimen: TaxRegime::Micro,
        regimen_ajustes: TaxRegime::Micro,
        erd: true,
        incn: Some(e(400_000)),
        base: Some(e(100_000)),
        cuota: Some(e(20_000)),
        resultado_anterior: Some((e(60_000), "200 de 2025 [00500]".into())),
        dividendos_acordados: Money::ZERO,
        aumento_contabilizado: None,
        reserva_legal: Some(Money::ZERO),
        bases_pendientes: Money::ZERO,
        bin_indicadas: Money::ZERO,
        // Los 200 hasta el año pasado, sin bases negativas pendientes.
        bin_arrastre: Some((2025, Money::ZERO)),
        capitalizacion_indicada: Money::ZERO,
        capitalizacion_anterior: None,
        tipo_presentado: None,
        nivelacion_anterior: None,
        dividendos_participadas: Money::ZERO,
        dividendos_grupo: Money::ZERO,
        dividendos_otras: Money::ZERO,
        exencion_anterior: false,
        correcciones_disminucion: Money::ZERO,
        trabajadores: false,
        cnae: Some("6201".into()),
        primer_año: Some(2018),
        impagadas: Vec::new(),
        clientes: Money::ZERO,
        cripto_ganancias: Money::ZERO,
        cripto_perdidas_latentes: Money::ZERO,
        cripto_monedas: Vec::new(),
        hay_cripto: false,
        modulos: false,
        inicio_marcado: false,
        gran_empresa: false,
        primer_beneficio: Some(Some(2018)),
        inversiones: None,
        ccaa: None,
        prorrata: None,
        prorrata_especial: false,
        recuperable: Vec::new(),
    }
}

fn evaluar(id: &str, c: &Ctx) -> super::catalogo::Evaluacion {
    let f = FICHAS.iter().find(|f| f.id == id).expect(id);
    (f.evaluar)(c)
}

#[test]
fn ids_unicos_y_fichas_completas() {
    let mut ids = BTreeSet::new();
    for f in FICHAS {
        assert!(ids.insert(f.id), "id repetido: {}", f.id);
        assert!(!f.pasos.is_empty(), "{} sin pasos", f.id);
        assert!(!f.requisitos.is_empty(), "{} sin requisitos", f.id);
        assert!(f.enlace.starts_with("https://"), "{} sin enlace", f.id);
        assert!(!f.norma.is_empty());
    }
}

#[test]
fn ahorro_con_la_escala_de_la_microempresa() {
    // 2026, cifra < 1 M€: 19 % hasta 50.000 € y 21 % el resto. Rebajar 10.000 € de una base de
    // 100.000 € ahorra 2.100 € (todo en el tramo del 21 %).
    let c = ctx();
    assert_eq!(c.ahorro_base(e(10_000)), Some(e(2_100)));
    assert_eq!(c.tipo(), Some(21));
}

#[test]
fn capitalizacion_con_el_beneficio_del_año_anterior() {
    // 20 % de 60.000 € = 12.000 € (por debajo del 25 % de la base, 25.000 €) → 2.520 €.
    let r = evaluar("capitalizacion", &ctx());
    assert_eq!(r.encaje, Encaje::Conviene);
    assert_eq!(r.ahorro, Some(e(2_520)));
    assert_eq!(r.plazo, Some(date!(2026 - 12 - 31)));
    // Si se reparte todo como dividendo, ya no crece nada.
    let mut c = ctx();
    c.dividendos_acordados = e(60_000);
    assert_eq!(evaluar("capitalizacion", &c).encaje, Encaje::Valorar);
    // Ya indicada en los datos de los modelos.
    let mut c = ctx();
    c.capitalizacion_indicada = e(5_000);
    assert_eq!(evaluar("capitalizacion", &c).encaje, Encaje::Aprovechado);
    // Con pérdidas, no hay nada que rebajar.
    let mut c = ctx();
    c.base = Some(e(-1_000));
    assert_eq!(evaluar("capitalizacion", &c).encaje, Encaje::NoAplica);
    // Si la junta ya aplicó el resultado, manda lo que fue a reservas: 30.000 € → 6.000 €.
    let mut c = ctx();
    c.aumento_contabilizado = Some(e(30_000));
    let r = evaluar("capitalizacion", &c);
    assert_eq!(r.encaje, Encaje::Conviene);
    assert_eq!(r.ahorro, Some(e(1_260)));
    // Las bases negativas pendientes que se comen la base dejan el ahorro en nada.
    let mut c = ctx();
    c.bases_pendientes = e(1_000_000);
    assert_eq!(evaluar("capitalizacion", &c).ahorro, None);
}

#[test]
fn nivelacion_solo_reducida_dimension() {
    let r = evaluar("nivelacion", &ctx());
    // Microempresa: su tipo está en el primer párrafo del art. 29.1, así que «te conviene».
    assert_eq!(r.encaje, Encaje::Conviene);
    // Después de la reserva de capitalización (12.000 €): 10 % de 88.000 € al 21 %.
    assert_eq!(r.ahorro, Some(e(1_848)));
    // Con 40.000 € de bases negativas quedan 48.000 €: 10 % = 4.800 € al 19 %.
    let mut c = ctx();
    c.bases_pendientes = e(40_000);
    assert_eq!(evaluar("nivelacion", &c).ahorro, Some(e(912)));
    let mut c = ctx();
    c.erd = false;
    c.regimen = TaxRegime::General;
    assert_eq!(evaluar("nivelacion", &c).encaje, Encaje::NoAplica);
}

#[test]
fn audiovisual_con_la_cuota() {
    // Con la cuota que queda tras la reserva de capitalización que conviene (base 88.000 €:
    // 9.500 € + 21 % de 38.000 € = 17.480 €), el financiador llega al 50 %: 8.740 € de deducción
    // por 7.283,33 € aportados; ganancia 1.456,67 €.
    let r = evaluar("audiovisual", &ctx());
    assert_eq!(r.encaje, Encaje::Conviene);
    assert_eq!(r.ahorro, Some(Money::from_cents(145_667)));
    assert!(
        r.motivo.contains("aporta como mucho unos 7.283,33 €"),
        "{}",
        r.motivo
    );
    // Un autónomo puede, pero sin la cuota de la Renta no se calcula.
    let mut c = ctx();
    c.fisica = true;
    assert_eq!(evaluar("audiovisual", &c).encaje, Encaje::Valorar);
    let mut c = ctx();
    c.cuota = Some(Money::ZERO);
    c.base = Some(Money::ZERO);
    assert_eq!(evaluar("audiovisual", &c).encaje, Encaje::NoAplica);
}

#[test]
fn dividendos_de_participadas_sin_exencion() {
    let mut c = ctx();
    c.dividendos_participadas = e(10_000);
    // En la 760 sin más: pueden ser de fondos o de acciones con menos del 5 %.
    assert_eq!(evaluar("participadas", &c).encaje, Encaje::Valorar);
    c.dividendos_grupo = e(10_000);
    let r = evaluar("participadas", &c);
    assert_eq!(r.encaje, Encaje::Conviene);
    assert_eq!(r.ahorro, Some(Money::from_cents(199_500)));
    c.correcciones_disminucion = e(9_500);
    assert_eq!(evaluar("participadas", &c).encaje, Encaje::Aprovechado);
}

#[test]
fn iva_de_impagadas_en_su_ventana() {
    let mut c = ctx();
    c.impagadas = vec![
        // Emitida hace 7 meses: dentro de la ventana (6 a 18 meses).
        Impagada {
            emision: date!(2026 - 03 - 15),
            vencimiento: date!(2026 - 04 - 15),
            pendiente: e(1_210),
            iva: e(210),
        },
        // Hace más de 2 años: fuera de plazo (pasados los 18 meses).
        Impagada {
            emision: date!(2024 - 09 - 01),
            vencimiento: date!(2024 - 10 - 01),
            pendiente: e(605),
            iva: e(105),
        },
        // Hace 2 meses: aún no.
        Impagada {
            emision: date!(2026 - 08 - 20),
            vencimiento: date!(2026 - 09 - 20),
            pendiente: e(121),
            iva: e(21),
        },
    ];
    let r = evaluar("iva_impagadas", &c);
    assert_eq!(r.encaje, Encaje::Conviene);
    assert_eq!(r.ahorro, Some(e(210)));
    assert_eq!(r.plazo, Some(date!(2027 - 09 - 15)));
    // Empresa grande (volumen de más de 6 M€): sólo a partir del año.
    let mut g = ctx();
    g.impagadas = c.impagadas.clone();
    g.incn = Some(e(8_000_000));
    g.erd = true;
    assert_eq!(evaluar("iva_impagadas", &g).encaje, Encaje::Valorar);
    // Gasto por deterioro: las vencidas hace más de seis meses al cierre (las dos primeras).
    let d = evaluar("deterioro_creditos", &c);
    assert_eq!(d.encaje, Encaje::Conviene);
}

#[test]
fn cripto_compensa_perdidas_latentes() {
    let mut c = ctx();
    c.fisica = true;
    c.hay_cripto = true;
    c.cripto_ganancias = e(4_000);
    c.cripto_perdidas_latentes = e(1_500);
    let r = evaluar("cripto_perdidas", &c);
    assert_eq!(r.encaje, Encaje::Conviene);
    // 1.500 € al 19 %.
    assert_eq!(r.ahorro, Some(e(285)));
    assert_eq!(r.plazo, Some(date!(2026 - 12 - 31)));
}

#[test]
fn autonomo_plan_de_pensiones() {
    let mut c = ctx();
    c.fisica = true;
    c.base = Some(e(40_000));
    let r = evaluar("plan_pensiones_autonomo", &c);
    // Con 40.000 € el último euro tributa al 37 % (el mínimo personal no cambia el tramo): 5.750 €
    // al 37 %.
    assert_eq!(r.encaje, Encaje::Conviene);
    assert_eq!(r.ahorro, Some(Money::from_cents(212_750)));
    // Con un rendimiento bajo (15.000 € → 9.450 € al 19 %), puede interesar pero no conviene.
    c.base = Some(e(15_000));
    let r = evaluar("plan_pensiones_autonomo", &c);
    assert_eq!(r.encaje, Encaje::Valorar);
    // Sin Renta que pagar, no.
    c.base = Some(e(5_000));
    assert_eq!(
        evaluar("plan_pensiones_autonomo", &c).encaje,
        Encaje::NoAplica
    );
    // En módulos, ni gastos ni inicio de actividad.
    c.modulos = true;
    assert_eq!(evaluar("gastos_autonomo", &c).encaje, Encaje::NoAplica);
    assert_eq!(evaluar("inicio_actividad", &c).encaje, Encaje::NoAplica);
}

#[test]
fn marcas_se_guardan_y_se_limpian() {
    let v = Vault::open_in_memory(
        std::sync::Arc::new(taxpain_vault::MemoryKeyProvider::default()),
        std::sync::Arc::new(taxpain_vault::AlwaysConfirm),
    )
    .unwrap();
    let m = Marca {
        estado: Estado::EnMarcha,
        pasos: [0, 2, 99].into_iter().collect(),
    };
    let hoy = date!(2026 - 10 - 30);
    marcar(&v, hoy, "capitalizacion", &m).unwrap();
    let todas: Marcas = v.setting(MARCAS).unwrap().unwrap();
    // Por ejercicio; el paso 99 no existe y se descarta.
    assert_eq!(
        todas[&2026]["capitalizacion"].pasos,
        [0, 2].into_iter().collect()
    );
    // El año siguiente empieza sin marcas.
    marcar(&v, date!(2027 - 02 - 01), "nivelacion", &m).unwrap();
    let todas: Marcas = v.setting(MARCAS).unwrap().unwrap();
    assert!(!todas[&2027].contains_key("capitalizacion"));
    marcar(&v, hoy, "capitalizacion", &Marca::default()).unwrap();
    let todas: Marcas = v.setting(MARCAS).unwrap().unwrap();
    assert!(!todas.contains_key(&2026));
    assert!(marcar(&v, hoy, "no_existe", &m).is_err());
    // Unas marcas con el formato anterior (sin ejercicio) se ignoran sin fallar.
    let viejas: BTreeMap<String, Marca> = [("capitalizacion".to_owned(), m.clone())].into();
    v.set_setting(MARCAS, &viejas).unwrap();
    assert!(leer_marcas(&v).unwrap().is_empty());
}

#[test]
fn recuperar_lo_pagado_de_mas() {
    let mut c = ctx();
    assert_eq!(evaluar("rectificar", &c).encaje, Encaje::NoAplica);
    c.recuperable = vec![
        Recuperable {
            ejercicio: Some(2023),
            titulo: "Dividendos sin exención".into(),
            importe: Money::from_cents(123_456),
            hasta: Some(date!(2029 - 07 - 25)),
        },
        Recuperable {
            ejercicio: Some(2025),
            titulo: "Dividendos sin exención".into(),
            importe: Money::from_cents(234_567),
            hasta: Some(date!(2031 - 07 - 25)),
        },
    ];
    let r = evaluar("rectificar", &c);
    assert_eq!(r.encaje, Encaje::Conviene);
    assert_eq!(r.ahorro, Some(Money::from_cents(358_023)));
    assert_eq!(r.plazo, Some(date!(2029 - 07 - 25)));
    assert!(
        r.motivo
            .contains("2023: Dividendos sin exención (1.234,56 €)")
    );
}

#[test]
fn tipo_reducido_mal_puesto() {
    let mut c = ctx();
    assert_eq!(evaluar("tipo_reducido", &c).encaje, Encaje::Aprovechado);
    // Con el general en los ajustes y 100.000 € de base: 25.000 € frente a 9.500 + 10.500. El
    // tipo reducido se aplica por ley: no suma como ahorro, sólo avisa del borrador.
    c.regimen_ajustes = TaxRegime::General;
    let r = evaluar("tipo_reducido", &c);
    assert_eq!(r.encaje, Encaje::Valorar);
    assert_eq!(r.ahorro, None);
    assert!(
        r.motivo.contains("unos 5.000,00 € este año"),
        "{}",
        r.motivo
    );
    // Si la gestoría ya presentó el 200 con un tipo reducido, no hay ahorro: sólo el ajuste.
    let mut g = ctx();
    g.regimen_ajustes = TaxRegime::General;
    g.tipo_presentado = Some((2025, Money::from_cents(2_100)));
    let r = evaluar("tipo_reducido", &g);
    assert_eq!(r.encaje, Encaje::Aprovechado);
    assert!(r.motivo.contains("(21,00 %)"), "{}", r.motivo);
    c.incn = None;
    assert_eq!(evaluar("tipo_reducido", &c).encaje, Encaje::Valorar);
}

#[test]
fn tipo_marginal_de_la_microempresa() {
    // Hasta 50.000 € de base, el 19 %; por encima, el 21 %.
    let mut c = ctx();
    c.base = Some(e(40_000));
    assert_eq!(c.tipo(), Some(19));
    c.base = Some(e(80_000));
    assert_eq!(c.tipo(), Some(21));
}

#[test]
fn ningun_texto_con_espacios_de_mas() {
    // Las líneas largas de los textos se parten con «\» al final: si se pierde, la frase se
    // queda con una tira de espacios en medio.
    for f in FICHAS {
        let todos = [f.titulo, f.resumen, f.norma]
            .into_iter()
            .chain(f.requisitos.iter().copied())
            .chain(f.pasos.iter().copied())
            .chain(f.riesgos.iter().copied());
        for t in todos {
            assert!(!t.contains("  "), "{}: «{t}»", f.id);
        }
        let e = (f.evaluar)(&ctx());
        assert!(!e.motivo.contains("  "), "{}: «{}»", f.id, e.motivo);
    }
}

#[test]
fn bases_negativas_pendientes() {
    let mut c = ctx();
    assert_eq!(evaluar("bases_negativas", &c).encaje, Encaje::NoAplica);
    // Sin los 200 anteriores no se sabe: puede interesar (y el cine no «conviene» todavía).
    c.bin_arrastre = None;
    assert_eq!(evaluar("bases_negativas", &c).encaje, Encaje::Valorar);
    assert_eq!(evaluar("audiovisual", &c).encaje, Encaje::Valorar);
    // 30.000 € pendientes según los 200 y nada en los ajustes: 100.000 € → 70.000 € de base.
    c.bin_arrastre = Some((2025, e(30_000)));
    c.bases_pendientes = e(30_000);
    let r = evaluar("bases_negativas", &c);
    assert_eq!(r.encaje, Encaje::Valorar);
    assert_eq!(r.ahorro, Some(e(6_300)));
    c.bin_indicadas = e(30_000);
    assert_eq!(evaluar("bases_negativas", &c).encaje, Encaje::Aprovechado);
}

#[test]
fn cripto_en_una_sociedad() {
    // En una sociedad no hay base del ahorro: la pérdida al vender rebaja la base al tipo del IS.
    let mut c = ctx();
    c.hay_cripto = true;
    c.cripto_perdidas_latentes = e(2_000);
    let r = evaluar("cripto_perdidas", &c);
    assert_eq!(r.encaje, Encaje::Valorar);
    assert_eq!(r.ahorro, Some(e(420)));
    assert_eq!(r.plazo, Some(date!(2026 - 12 - 31)));
    // Sin base, no se promete ahorro.
    c.base = Some(e(-5_000));
    assert_eq!(evaluar("cripto_perdidas", &c).ahorro, None);
}

#[test]
fn tipo_reducido_al_reves() {
    // En los ajustes, microempresa; por la cifra de negocios, reducida dimensión (23 %): el
    // borrador calcula de menos.
    let mut c = ctx();
    c.regimen = TaxRegime::ReducidaDimension;
    c.regimen_ajustes = TaxRegime::Micro;
    let r = evaluar("tipo_reducido", &c);
    assert_eq!(r.encaje, Encaje::Valorar);
    assert!(r.motivo.contains("de menos"), "{}", r.motivo);
}

#[test]
fn administrador_retribucion_o_dividendos() {
    // Con el 21 % de Sociedades, el dividendo paga 21 % + 79 % × 19 % ≈ 36 %: la retribución sale
    // mejor hasta el tramo del 30 % (35.200 €).
    let r = evaluar("retribucion_administrador", &ctx());
    assert_eq!(r.encaje, Encaje::Valorar);
    assert!(r.motivo.contains("un 36 %"), "{}", r.motivo);
    assert!(r.motivo.contains("35.200,00 €"), "{}", r.motivo);
}

#[test]
fn empresa_emergente_joven_y_tecnologica() {
    let mut c = ctx();
    assert_eq!(evaluar("empresa_emergente", &c).encaje, Encaje::NoAplica);
    c.primer_año = Some(2023);
    let r = evaluar("empresa_emergente", &c);
    assert_eq!(r.encaje, Encaje::Valorar);
    // 100.000 € de base: 20.000 € de microempresa frente a 15.000 € al 15 %.
    assert_eq!(r.ahorro, Some(e(5_000)));
    c.dividendos_acordados = e(1_000);
    assert_eq!(evaluar("empresa_emergente", &c).encaje, Encaje::NoAplica);
}

#[test]
fn restar_meses_de_fecha_a_fecha() {
    use super::catalogo::restar_meses;
    assert_eq!(
        restar_meses(date!(2026 - 12 - 31), 6),
        date!(2026 - 06 - 30)
    );
    assert_eq!(
        restar_meses(date!(2026 - 06 - 30), 6),
        date!(2025 - 12 - 30)
    );
    assert_eq!(
        restar_meses(date!(2026 - 03 - 31), 1),
        date!(2026 - 02 - 28)
    );
}

#[test]
fn nivelacion_con_duda_solo_en_reducida_dimension() {
    let mut c = ctx();
    c.regimen = TaxRegime::ReducidaDimension;
    c.regimen_ajustes = TaxRegime::ReducidaDimension;
    let r = evaluar("nivelacion", &c);
    assert_eq!(r.encaje, Encaje::Valorar);
    assert!(
        r.motivo.contains("Confírmalo antes con tu asesor"),
        "{}",
        r.motivo
    );
}

#[test]
fn dividendos_de_una_cartera_no() {
    // 4.000 € del grupo y 6.000 € de una cartera (7603): sólo los del grupo están exentos.
    let mut c = ctx();
    c.dividendos_participadas = e(10_000);
    c.dividendos_grupo = e(4_000);
    c.dividendos_otras = e(6_000);
    let r = evaluar("participadas", &c);
    assert_eq!(r.encaje, Encaje::Conviene);
    assert_eq!(r.ahorro, Some(e(798)));
    assert!(r.motivo.contains("De los otros 6.000,00 €"), "{}", r.motivo);
}

#[test]
fn nueva_creacion_por_el_primer_año_con_beneficio() {
    // Empezó en 2023 con pérdidas y gana por primera vez en 2026: tiene el 15 %.
    let mut c = ctx();
    c.primer_año = Some(2023);
    c.primer_beneficio = Some(None);
    assert_eq!(evaluar("nueva_creacion", &c).encaje, Encaje::Valorar);
    // Si ya ganó en 2023, no.
    c.primer_beneficio = Some(Some(2023));
    assert_eq!(evaluar("nueva_creacion", &c).encaje, Encaje::NoAplica);
    // Primer beneficio el año pasado: este año es el segundo.
    c.primer_beneficio = Some(Some(2025));
    assert_eq!(evaluar("nueva_creacion", &c).encaje, Encaje::Valorar);
}

#[test]
fn autonomo_con_rendimiento_alto_y_prorrata() {
    let mut c = ctx();
    c.fisica = true;
    c.base = Some(e(120_000));
    assert_eq!(evaluar("pasar_a_sociedad", &c).encaje, Encaje::Valorar);
    c.base = Some(e(30_000));
    assert_eq!(evaluar("pasar_a_sociedad", &c).encaje, Encaje::NoAplica);
    assert_eq!(evaluar("prorrata_especial", &c).encaje, Encaje::NoAplica);
    c.prorrata = Some(80);
    let r = evaluar("prorrata_especial", &c);
    assert_eq!(r.encaje, Encaje::Valorar);
    assert_eq!(r.plazo, Some(date!(2027 - 01 - 30)));
}

#[test]
fn aviso_de_sociedad_patrimonial() {
    let mut c = ctx();
    c.regimen_ajustes = TaxRegime::General;
    c.inversiones = Some((70, 60));
    let r = evaluar("tipo_reducido", &c);
    assert!(
        r.motivo.contains("el 60 % del activo son valores"),
        "{}",
        r.motivo
    );
}

#[test]
fn las_rebajas_juntas_ahorran_menos_que_por_separado() {
    // Microempresa con 60.000 € de base: la exención (9.500 €) y la capitalización (12.000 €)
    // por separado ahorran 1.995 € + 2.480 € = 4.475 €; juntas bajan la base a 38.500 €, todo
    // al 19 %: 11.600 € − 7.315 € = 4.285 €.
    let mut c = ctx();
    c.base = Some(e(60_000));
    assert_eq!(c.ahorro_base(e(9_500)), Some(e(1_995)));
    assert_eq!(c.ahorro_base(e(12_000)), Some(e(2_480)));
    assert_eq!(c.ahorro_conjunto(e(9_500), e(12_000)), e(4_285));
    // El tope de la capitalización baja con la base: con 40.000 € de exención, la base previa
    // queda en 20.000 € y la reserva no pasa de 5.000 € (25 %).
    assert_eq!(
        c.ahorro_conjunto(e(40_000), e(12_000)),
        c.ahorro_conjunto(e(40_000), e(5_000))
    );
}

#[test]
fn reserva_de_baleares_solo_alli() {
    let mut c = ctx();
    assert_eq!(evaluar("reserva_baleares", &c).encaje, Encaje::NoAplica);
    c.ccaa = Some(taxpain_core::calendar::Ccaa::IllesBalears);
    let r = evaluar("reserva_baleares", &c);
    assert_eq!(r.encaje, Encaje::Valorar);
    // Hasta el plazo del 200: 25 de julio del año siguiente.
    assert_eq!(r.plazo, Some(date!(2027 - 07 - 25)));
    c.año = 2029;
    assert_eq!(evaluar("reserva_baleares", &c).encaje, Encaje::NoAplica);
}

// ---------------------------------------------------------------- Conversación con la IA

fn situacion() -> Situacion {
    Situacion {
        año: 2026,
        cierre: date!(2026 - 12 - 31),
        persona_fisica: false,
        regimen: Some("Microempresa (cifra de negocios menor de 1 M€)".into()),
        cifra_negocios: Some(Cifra {
            valor: e(400_000),
            fuente: "200 de 2025 [00255]".into(),
        }),
        base: Some(Cifra {
            valor: e(100_000),
            fuente: "borrador del 200 de 2026".into(),
        }),
        cuota: None,
        tipo: Some(21),
        trabajadores: false,
        cnae: Some("6201".into()),
        notas: vec!["Las cifras de 2026 son las de la contabilidad hasta hoy".into()],
    }
}

/// Las recomendaciones `ids` evaluadas con `c`, con sus marcas.
fn con(c: &Ctx, ids: &[(&str, Marca)]) -> Ahorro {
    let recomendaciones = ids
        .iter()
        .map(|(id, m)| {
            let f = FICHAS.iter().find(|f| f.id == *id).expect(id);
            recomendacion(f, c, m.clone())
        })
        .collect();
    Ahorro {
        situacion: situacion(),
        recomendaciones,
        ahorro_posible: e(2_520),
        aplazado_posible: Money::ZERO,
    }
}

fn marca(estado: Estado, pasos: &[usize]) -> Marca {
    Marca {
        estado,
        pasos: pasos.iter().copied().collect(),
    }
}

#[test]
fn bloque_de_la_ficha_lleva_motivo_ahorro_plazo_pasos_y_norma_con_su_boe() {
    let a = con(&ctx(), &[("capitalizacion", marca(Estado::EnMarcha, &[0]))]);
    let (_, b) = tema_tarjeta(&a, "capitalizacion").unwrap();
    let r = &a.recomendaciones[0];
    assert!(
        b.starts_with(
            "Recomendación de «Ahorrar impuestos» de TaxPain para el ejercicio 2026: «Reserva de \
             capitalización: deja el beneficio en la empresa» (Impuesto sobre el beneficio).\n\
             - Encaje según TaxPain: le conviene. "
        ),
        "{b}"
    );
    assert!(b.contains(&r.motivo), "{b}");
    assert!(
        b.contains("\n- Ahorro estimado: 2.520,00 € (cálculo: "),
        "{b}"
    );
    assert!(
        b.contains("\n- Cuenta lo que haga hasta el cierre del ejercicio (31/12/2026)."),
        "{b}"
    );
    assert!(b.contains("\n- Estado que ha marcado el usuario: en marcha."));
    assert!(b.contains("\n- Pasos (1 de 5 hechos):\n  1. [hecho] Antes del cierre"));
    assert!(b.contains("\n  2. [pendiente] Calcula el aumento"));
    assert!(b.contains("\n- Requisitos: Que los fondos propios"));
    assert!(b.contains("\n- Ten en cuenta: Si en los tres años siguientes"));
    assert!(b.contains(
        "\n- Norma: Art. 25 de la Ley 27/2014 del Impuesto sobre Sociedades (BOE \
         BOE-A-2014-12328, artículo 25)."
    ));
    assert!(b.contains("\n- Pantalla de TaxPain donde se pone o se ve: Modelos."));
    assert!(
        b.contains(
            "\n- Situación usada: sociedad; Microempresa (cifra de negocios menor de 1 M€); cifra \
             de negocios 400.000,00 € (200 de 2025 [00255]); base imponible 100.000,00 € \
             (borrador del 200 de 2026); tipo para estimar el ahorro 21 %; sin trabajadores; \
             CNAE 6201.\n- Avisos de TaxPain sobre estas cifras: Las cifras de 2026 son las de la \
             contabilidad hasta hoy."
        ),
        "{b}"
    );
    // No es de planificación fiscal: sin ese aviso. Y nada de la fecha de hoy (cambiaría el
    // tema cada día).
    assert!(!b.contains("planificación fiscal"));
    assert!(!b.contains("30/10/2026"));

    // Una fecha límite que no es el cierre, una norma sin artículo y otra fuera del BOE.
    let mut a = con(&ctx(), &[("capitalizacion", Marca::default())]);
    a.recomendaciones[0].plazo = Some(date!(2027 - 07 - 25));
    a.recomendaciones[0].enlace = "https://www.boe.es/buscar/act.php?id=BOE-A-2022-21739";
    let (h, b) = tema_tarjeta(&a, "capitalizacion").unwrap();
    assert!(b.contains("\n- Fecha límite: 25/07/2027."), "{b}");
    assert!(b.contains("Sociedades (BOE BOE-A-2022-21739)."), "{b}");
    assert!(
        h.facts
            .contains(&("Fecha límite".into(), "25/07/2027".into()))
    );
    a.recomendaciones[0].enlace = "https://sede.agenciatributaria.gob.es/Sede/irpf.html";
    let (_, b) = tema_tarjeta(&a, "capitalizacion").unwrap();
    assert!(
        b.contains("(enlace: https://sede.agenciatributaria.gob.es/Sede/irpf.html)."),
        "{b}"
    );
    // Una ficha que no es para este contribuyente no tiene tema.
    assert!(tema_tarjeta(&a, "plan_pensiones_autonomo").is_none());
}

#[test]
fn cabecera_y_sugerencias_segun_el_encaje() {
    let a = con(&ctx(), &[("capitalizacion", marca(Estado::EnMarcha, &[0]))]);
    let (h, _) = tema_tarjeta(&a, "capitalizacion").unwrap();
    assert_eq!(h.group, "Ahorrar impuestos · 2026");
    assert_eq!(
        h.title,
        "Reserva de capitalización: deja el beneficio en la empresa"
    );
    let etiquetas: Vec<&str> = h.facts.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(
        etiquetas,
        [
            "Encaje",
            "Ahorro estimado",
            "Fecha límite",
            "Estado",
            "Pasos hechos",
            "Norma"
        ]
    );
    assert_eq!(h.facts[0].1, "Te conviene");
    assert_eq!(h.facts[1].1, "2.520,00 €");
    assert_eq!(h.facts[2].1, "hasta el cierre del ejercicio (31/12/2026)");
    assert_eq!(h.facts[3].1, "En marcha");
    assert_eq!(h.facts[4].1, "1 de 5");
    assert!(
        h.intro.starts_with("Con tus datos, te conviene. "),
        "{}",
        h.intro
    );
    assert!(h.intro.contains(
        " Ahorro estimado: 2.520,00 €. Cuenta lo que hagas hasta el cierre del ejercicio \
         (31/12/2026). Pregunta lo que quieras"
    ));
    assert_eq!(
        h.suggestions,
        [
            "Explícamelo con mis datos",
            "¿Qué tengo que hacer y antes de cuándo?",
            "¿Cómo se ha calculado el ahorro?"
        ]
    );
    assert_eq!(h.notice, None);

    // Hecho: en lugar de qué hacer, qué papeles guardar.
    let a = con(&ctx(), &[("capitalizacion", marca(Estado::Hecho, &[]))]);
    let (h, _) = tema_tarjeta(&a, "capitalizacion").unwrap();
    assert_eq!(h.suggestions[1], "¿Qué papeles guardo por si me los piden?");
    assert_eq!(h.facts[3].1, "Hecho");

    // Puede interesarte (todo el beneficio se reparte), ya lo aprovecha y no le aplica.
    let mut c = ctx();
    c.dividendos_acordados = e(60_000);
    let (h, b) = tema_tarjeta(
        &con(&c, &[("capitalizacion", Marca::default())]),
        "capitalizacion",
    )
    .unwrap();
    assert!(
        h.intro
            .starts_with("Puede interesarte: depende de algo que TaxPain no sabe. ")
    );
    assert!(b.contains("Encaje según TaxPain: puede interesarle: depende de algo"));
    assert_eq!(
        h.suggestions,
        [
            "¿Qué dato falta para saber si me conviene?",
            "Explícamelo con mis datos",
            "¿Qué riesgos tiene?"
        ]
    );
    let mut c = ctx();
    c.capitalizacion_indicada = e(5_000);
    let (h, _) = tema_tarjeta(
        &con(&c, &[("capitalizacion", Marca::default())]),
        "capitalizacion",
    )
    .unwrap();
    assert!(h.intro.starts_with("Ya lo aprovechas. "));
    assert_eq!(
        h.suggestions,
        [
            "¿Lo aprovecho del todo?",
            "¿Qué tengo que vigilar los próximos años?"
        ]
    );
    let mut c = ctx();
    c.base = Some(e(-1_000));
    let (h, _) = tema_tarjeta(
        &con(&c, &[("capitalizacion", Marca::default())]),
        "capitalizacion",
    )
    .unwrap();
    assert!(h.intro.starts_with("Con tus datos, no te aplica. "));
    assert_eq!(
        h.suggestions,
        [
            "¿Por qué no me aplica?",
            "¿Qué tendría que cambiar para que me aplique?"
        ]
    );

    // El plan del año: las que convienen o se pueden valorar, una a una, y las que no aplican.
    let mut a = con(
        &ctx(),
        &[
            ("capitalizacion", Marca::default()),
            ("nivelacion", marca(Estado::Descartado, &[])),
            ("patent_box", Marca::default()),
        ],
    );
    a.recomendaciones[2].encaje = Encaje::NoAplica;
    let (h, b) = tema_plan(&a);
    assert_eq!(h.group, "Ahorrar impuestos · tu plan de 2026");
    assert_eq!(h.title, "Tu plan de ahorro de 2026");
    assert!(b.starts_with(
        "Plan de «Ahorrar impuestos» de TaxPain, ejercicio 2026 (cierre 31/12/2026).\n- Ahorro \
         posible según TaxPain: 2.520,00 €"
    ));
    assert!(b.contains(
        "\n  - capitalizacion · Reserva de capitalización: deja el beneficio en la empresa · le \
         conviene · ahorro 2.520,00 € · hasta el cierre del ejercicio (31/12/2026) · pendiente"
    ));
    let nivelacion = b
        .lines()
        .find(|l| l.starts_with("  - nivelacion · "))
        .unwrap();
    assert!(
        nivelacion.contains(" · aplazado 1.848,00 € · ")
            && nivelacion.ends_with(" · no le interesa"),
        "{nivelacion}"
    );
    // Las que no aplican, en una sola línea y sin detalle.
    assert!(b.contains(&format!(
        "\n- No le aplican con sus datos: {}.",
        a.recomendaciones[2].titulo
    )));
    assert!(!b.contains("  - patent_box"));
    assert!(b.contains("\n- Situación usada: sociedad;"));
    assert_eq!(h.facts[0], ("Ahorro posible".into(), "2.520,00 €".into()));
    assert!(h.facts.contains(&("Te convienen".into(), "1".into())));
    assert!(
        h.facts
            .contains(&("Cierre del ejercicio".into(), "31/12/2026".into()))
    );
    assert_eq!(
        h.intro,
        "En 2026 podrías ahorrarte unos 2.520,00 € con 1 recomendación que te conviene. \
         Pregunta por dónde empezar o añade datos que TaxPain no tenga."
    );
    assert_eq!(
        h.suggestions,
        [
            "¿Por cuál empiezo?",
            "¿Qué tengo que hacer antes del 31 de diciembre de 2026?",
            "¿Se pueden combinar entre ellas?"
        ]
    );
}

#[test]
fn aviso_en_las_fichas_de_planificacion() {
    let mut c = ctx();
    c.fisica = true;
    c.base = Some(e(120_000));
    let ids = [
        "pasar_a_sociedad",
        "retribucion_administrador",
        "retribucion_flexible",
    ];
    let a = con(&c, &ids.map(|id| (id, Marca::default())));
    for id in ids {
        let (_, b) = tema_tarjeta(&a, id).unwrap();
        assert!(
            b.contains("\n- Cuidado: es planificación fiscal.")
                && b.contains("(arts. 15 y 16 LGT)")
                && b.contains("(art. 18 LIS)"),
            "{id}: {b}"
        );
    }
    let a = con(&ctx(), &[("capitalizacion", Marca::default())]);
    let (_, b) = tema_tarjeta(&a, "capitalizacion").unwrap();
    assert!(!b.contains("Cuidado: es planificación fiscal"));
}

#[test]
fn fichas_sin_marcadores_por_patron() {
    // El texto fijo de las fichas no tiene nada que el anonimizador tape por patrón (cifras que
    // parezcan un teléfono, una cuenta o un NIF): la IA lo recibe tal cual.
    let mut s = taxpain_assistant::pii::Scrubber::new().without_names();
    for f in FICHAS {
        let textos = [f.titulo, f.resumen, f.norma, f.enlace]
            .into_iter()
            .chain(f.requisitos.iter().copied())
            .chain(f.pasos.iter().copied())
            .chain(f.riesgos.iter().copied());
        for t in textos {
            assert_eq!(s.scrub(t), t, "{}", f.id);
        }
    }
    assert_eq!(s.replaced(), 0);
}

// ------------------------------------------------- Sin contabilidad del año: libros registro

/// Una sociedad (NIF de prueba) sin diario, con un 200 de 2025 de base alta.
fn sin_diario() -> Vault {
    use crate::settings::{InvoicingSettings, Language, Outputs, VerifactuSettings};
    use taxpain_core::invoice::{Party, PartyId, RoundingMode};
    let v = Vault::open_in_memory(
        std::sync::Arc::new(taxpain_vault::MemoryKeyProvider::default()),
        std::sync::Arc::new(taxpain_vault::AlwaysConfirm),
    )
    .unwrap();
    let s = InvoicingSettings {
        issuer: Party {
            id: PartyId::Nif("B98765431".into()),
            name: "Ejemplo Servicios SL".into(),
            address: addr(),
            person: None,
        },
        default_series: "A".into(),
        rectifying_series: "R".into(),
        simplified_series: "S".into(),
        simplified_rectifying_series: "RS".into(),
        language: Language::Es,
        default_vat: taxpain_core::money::Rate::percent(21),
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
    let now = time::macros::datetime!(2026-10-30 10:00 UTC);
    crate::invoicing::save_settings(&v, &s, now).unwrap();
    let b200: BTreeMap<u16, Money> = [
        (255, e(400_000)),
        (550, e(180_000)),
        (552, e(180_000)),
        (562, e(39_000)),
    ]
    .into();
    crate::presentados::save_manual(&v, "200", 2025, "0A", &b200, now).unwrap();
    v
}

fn libros(v: &Vault, emitidas: &str, recibidas: &str) {
    let now = time::macros::datetime!(2026-10-30 10:00 UTC);
    let b = crate::libros::parse("emitidas.csv", emitidas.as_bytes())
        .unwrap()
        .unwrap();
    crate::libros::import_issued(v, &b, now).unwrap();
    let b = crate::libros::parse("recibidas.csv", recibidas.as_bytes())
        .unwrap()
        .unwrap();
    crate::libros::import_received(v, &b, "recibidas.csv", recibidas.as_bytes(), now).unwrap();
}

/// Sin diario de 2026 y con poca facturación: la base sale de los libros de 2026 (menos los
/// sueldos de los 111 y más el 5 % del dividendo del banco), no del 200 de 2025.
#[test]
fn sin_diario_la_base_sale_de_los_libros_del_año() {
    let v = sin_diario();
    libros(
        &v,
        "Fecha Expedición;Número Factura;NIF Destinatario;Nombre Destinatario;Base Imponible;Tipo IVA;Cuota IVA;Total\n\
         15/01/2026;2026-001;B12345674;Cliente SL;8.000,00;21;1.680,00;9.680,00\n\
         20/09/2026;2026-002;B12345674;Cliente SL;4.000,00;21;840,00;4.840,00\n",
        "Fecha;Nº Factura;NIF Proveedor;Proveedor;Base;Cuota soportada;Retención;Total\n\
         01/03/2026;F-77;A12345674;Proveedor Ejemplo SA;4.000,00;840,00;0,00;4.840,00\n",
    );
    let now = time::macros::datetime!(2026-10-30 10:00 UTC);
    for p in ["1T", "2T", "3T"] {
        let b: BTreeMap<u16, Money> = [(1, e(1)), (2, e(1_000)), (3, e(150))].into();
        crate::presentados::save_manual(&v, "111", 2026, p, &b, now).unwrap();
    }
    let c = v.connection();
    c.execute(
        "INSERT INTO bank_account(id, number, account, name) VALUES (1, '0001', '572', 'Banco')",
        [],
    )
    .unwrap();
    c.execute(
        "INSERT INTO bank_tx(bank_account, op_date, value_date, amount, description, import_hash,
                             created_at)
         VALUES (1, '2026-05-14', '2026-05-14', 100000, 'TRANSF. PARTICIPADA SL DIVIDENDO', 'h1',
                 '2026-05-14')",
        [],
    )
    .unwrap();
    let (ctx, s) = contexto(&v, date!(2026 - 10 - 30)).unwrap();
    // 12.000 − 4.000 − 3.000 + 5 % de 1.000.
    assert_eq!(ctx.base, Some(e(5_050)));
    let base = s.base.unwrap();
    assert!(base.fuente.contains("libros registro"), "{}", base.fuente);
    assert!(base.fuente.contains("111"), "{}", base.fuente);
    assert!(!base.fuente.contains("200"), "{}", base.fuente);
    // Microempresa por la cifra de negocios de 2025: 21 % hasta 50.000 €.
    assert_eq!(
        ctx.cuota,
        Some(gross_tax_for(e(5_050), ctx.regimen, 2026, 365).0)
    );
    assert!(
        s.notas.iter().any(|n| n.contains("gastos sin factura")),
        "{:?}",
        s.notas
    );
    assert!(
        s.notas.iter().any(|n| n.contains("art. 21")),
        "{:?}",
        s.notas
    );
    let a = ahorro(&v, date!(2026 - 10 - 30)).unwrap();
    assert!(a.ahorro_posible < e(2_000), "{}", a.ahorro_posible);
}

/// Con la revisión de la carpeta guardada que confirma el dividendo con el acta de la
/// participada (1.234,57 € brutos, 234,57 € retenidos, 1.000 € en el banco): el 5 % del bruto, no
/// de lo cobrado, y una nota con la retención que se resta de la cuota del 200.
#[test]
fn sin_diario_con_el_dividendo_confirmado_usa_el_bruto() {
    let v = sin_diario();
    libros(
        &v,
        "Fecha Expedición;Número Factura;NIF Destinatario;Nombre Destinatario;Base Imponible;Tipo IVA;Cuota IVA;Total\n\
         15/01/2026;2026-001;B12345674;Cliente SL;8.000,00;21;1.680,00;9.680,00\n",
        "Fecha;Nº Factura;NIF Proveedor;Proveedor;Base;Cuota soportada;Retención;Total\n\
         01/03/2026;F-77;A12345674;Proveedor Ejemplo SA;4.000,00;840,00;0,00;4.840,00\n",
    );
    let c = v.connection();
    c.execute(
        "INSERT INTO bank_account(id, number, account, name) VALUES (1, '0001', '572', 'Banco')",
        [],
    )
    .unwrap();
    c.execute(
        "INSERT INTO bank_tx(bank_account, op_date, value_date, amount, description, import_hash,
                             created_at)
         VALUES (1, '2026-05-14', '2026-05-14', 100000, 'TRANSF. PARTICIPADA SL DIVIDENDO', 'h1',
                 '2026-05-14')",
        [],
    )
    .unwrap();
    let r = crate::revision::Revision {
        generada: "2026-10-30".into(),
        empresa: "Ejemplo Servicios SL".into(),
        nif: Some("B98765431".into()),
        carpeta: "/carpeta".into(),
        resumen: crate::revision::Resumen {
            ficheros: 2,
            con_detalle: 2,
            sin_leer: 0,
            no_admitidos: 0,
            por_tipo: vec![],
            errores: 1,
            avisos: 0,
            info: 0,
            urgentes: 0,
            en_juego_urgente: Money::ZERO,
            recuperable: Money::ZERO,
            a_favor_proximo_200: Money::from_cents(23_457),
            retenciones_proximo_200: Money::from_cents(23_457),
            exencion_proximo_200: Money::ZERO,
            impuesto_dividendos_proximo_200: Money::ZERO,
            pendiente_cobro: Money::ZERO,
        },
        hallazgos: vec![],
        tablas: vec![],
        pedir: vec![],
        devoluciones_pendientes: vec![],
        dividendos: vec![crate::revision::DividendoConfirmado {
            ejercicio: 2026,
            fecha: "2026-05-14".into(),
            pagadora: "PARTICIPADA SL".into(),
            neto: e(1_000),
            bruto: Money::from_cents(123_457),
            retencion: Money::from_cents(23_457),
            grupo: true,
        }],
    };
    crate::revision::guardar_ultima(&v, &r).unwrap();
    let (ctx, s) = contexto(&v, date!(2026 - 10 - 30)).unwrap();
    // 8.000 − 4.000 + 5 % de 1.234,57 (61,73).
    assert_eq!(ctx.base, Some(Money::from_cents(406_173)));
    assert!(
        s.notas
            .iter()
            .any(|n| n.contains("PARTICIPADA SL") && n.contains("1.234,57 €")),
        "{:?}",
        s.notas
    );
    assert!(
        s.notas
            .iter()
            .any(|n| n.contains("retenciones soportadas en esos dividendos (234,57 €)")),
        "{:?}",
        s.notas
    );
}

/// Con pérdidas en los libros de 2026: base 0 y ningún ahorro de cuota.
#[test]
fn sin_diario_y_con_perdidas_base_cero() {
    let v = sin_diario();
    libros(
        &v,
        "Fecha Expedición;Número Factura;NIF Destinatario;Nombre Destinatario;Base Imponible;Tipo IVA;Cuota IVA;Total\n\
         15/02/2026;2026-001;B12345674;Cliente SL;1.000,00;21;210,00;1.210,00\n",
        "Fecha;Nº Factura;NIF Proveedor;Proveedor;Base;Cuota soportada;Retención;Total\n\
         01/03/2026;F-77;A12345674;Proveedor Ejemplo SA;5.000,00;1.050,00;0,00;6.050,00\n",
    );
    let (ctx, s) = contexto(&v, date!(2026 - 10 - 30)).unwrap();
    assert_eq!(ctx.base, Some(Money::ZERO));
    assert_eq!(ctx.cuota, Some(Money::ZERO));
    assert!(
        s.notas.iter().any(|n| n.contains("pérdidas")),
        "{:?}",
        s.notas
    );
    let a = ahorro(&v, date!(2026 - 10 - 30)).unwrap();
    assert_eq!(a.ahorro_posible, Money::ZERO);
}

/// Sin libros ni diario de 2026: el 200 de 2025, avisando de que es otro año.
#[test]
fn sin_libros_el_200_anterior_como_referencia() {
    let v = sin_diario();
    let (ctx, s) = contexto(&v, date!(2026 - 10 - 30)).unwrap();
    assert_eq!(ctx.base, Some(e(180_000)));
    assert!(s.base.unwrap().fuente.contains("otro año"));
    assert!(
        s.notas.iter().any(|n| n.contains("puede no parecerse")),
        "{:?}",
        s.notas
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
