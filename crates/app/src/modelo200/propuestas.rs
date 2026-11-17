//! Correcciones al resultado contable que TaxPain propone desde la contabilidad y las fichas
//! (cada una con su casilla del catálogo, `taxpain_models::m200_catalogo`). El usuario las puede
//! cambiar o descartar en la pantalla del 200 (`is_ajuste`).
//!
//! - Multas, sanciones y recargos (art. 15.c LIS, [01815]): subcuentas cuyo nombre dice «multa»,
//!   «sanción» o «recargo»; el resto de la 678 (gastos excepcionales) se propone sin incluir.
//! - Donativos y liberalidades (art. 15.e, [00339]): subcuentas con «donativo», «donación» o
//!   «liberalidad». Las atenciones a clientes o proveedores son deducibles hasta el 1 % de la
//!   cifra de negocios del periodo: el exceso va a la [00339].
//! - Deterioros (art. 13.2 LIS): del inmovilizado material, intangible e inversiones inmobiliarias
//!   (690–692, [00331]), de participaciones (696, [00325]) y de valores de deuda (698, [00327]);
//!   sus reversiones (790–792, 796) se proponen sin incluir, porque sólo se restan si el
//!   deterioro no se dedujo en su día. El de créditos (694, 697) es deducible si cumple el art.
//!   13.1: no se propone.
//! - Provisiones (art. 14 LIS): la 695 (operaciones comerciales) se propone sin incluir: la de
//!   devoluciones no es deducible, la de garantías de reparación sí hasta un límite (art. 14.6).
//! - Amortización contable distinta de la fiscal (art. 12.1, [00303]/[00304]): la de las fichas
//!   de inmovilizado con coeficiente fiscal.
//! - Gastos financieros (art. 16 LIS, [00363]): sólo si los netos pasan de 1 M€; el exceso sobre
//!   el 30 % del beneficio operativo. `TODO(VERIFY)`: el beneficio operativo se aproxima con las
//!   cuentas del PGC.
//! - Exención del art. 21 (dividendos, [00370] o [02181]): desde la ficha de participadas.
//!
//! Los deterioros de créditos y las provisiones registrados en los ajustes de cierre los propone
//! `modelo200::preparar` desde [`crate::ajustes_cierre::correcciones_200`] ([00321]/[00322],
//! [00337]/[00338]), porque allí ya se decidió si son deducibles. Para no contarlos dos veces,
//! las propuestas de este módulo que salen de saldos de cuentas (multas, 678, donativos,
//! deterioros, reversiones y la 695) no ven los asientos con `Origin::AjusteCierre`: se restan
//! sus saldos ([`Contexto::cubiertos`]). La cifra de negocios y el beneficio operativo del art. 16
//! sí los incluyen: son magnitudes contables, no correcciones.
//! - Cartera de valores (Inversiones, paso del cierre): el deterioro de las acciones y ETF a
//!   coste ([00325]) y su reversión ([00326]), y las bajadas de valor razonable de la cartera de
//!   negociación, que sólo no son deducibles en participaciones de al menos el 5 % (art. 15.l
//!   LIS): se proponen sin incluir en la [00413]. Lo grabado por Valores en la 696, 698 y 796 no
//!   se propone otra vez desde las cuentas.

use std::collections::BTreeMap;

use serde::Serialize;
use taxpain_core::money::{Money, Rate};

use super::participadas::{EstadoArt21, ParticipadaVista};

/// Una corrección propuesta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Propuesta {
    pub clave: String,
    pub casilla: u16,
    pub importe: Money,
    pub concepto: String,
    /// De dónde sale («subcuentas 6780001, 6780002», «ficha de Participada Demo SL»…).
    pub origen: String,
    pub nota: Option<String>,
    /// Entra en el cálculo sin que el usuario lo confirme.
    pub incluir: bool,
}

/// Lo que hace falta para proponer.
pub(crate) struct Contexto<'a> {
    /// Saldo de cada cuenta de los grupos 6 y 7 en el ejercicio (deudor positivo), sin la
    /// regularización ni el cierre.
    pub saldos: &'a BTreeMap<String, Money>,
    /// Parte de `saldos` que viene de los asientos de los ajustes de cierre: ya tiene su propia
    /// corrección (o es deducible según el ajuste), así que no entra en las propuestas por
    /// cuentas.
    pub cubiertos: &'a BTreeMap<String, Money>,
    /// Nombre de cada cuenta con saldo.
    pub nombres: &'a BTreeMap<String, String>,
    pub participadas: &'a [ParticipadaVista],
    pub amortizacion: (Money, Money),
    /// Días del periodo (prorrateo del millón del art. 16).
    pub dias: i64,
    /// Lo grabado por la valoración al cierre de la cartera de valores.
    pub valores: &'a crate::valores::contabilidad::Fiscal,
}

fn suma(saldos: &BTreeMap<String, Money>, prefijos: &[&str]) -> Money {
    saldos
        .iter()
        .filter(|(c, _)| prefijos.iter().any(|p| c.starts_with(p)))
        .map(|(_, m)| *m)
        .sum()
}

/// Importe neto de la cifra de negocios del periodo: el saldo acreedor del subgrupo 70 (ventas,
/// prestaciones de servicios, menos descuentos, devoluciones y rappels).
pub(crate) fn cifra_de_negocios(saldos: &BTreeMap<String, Money>) -> Money {
    -suma(saldos, &["70"])
}

fn lista(cuentas: &[String]) -> String {
    if cuentas.len() == 1 {
        format!("subcuenta {}", cuentas[0])
    } else {
        format!("subcuentas {}", cuentas.join(", "))
    }
}

/// Cuentas de gasto cuyo nombre contiene alguna de las palabras, con su saldo.
fn por_nombre(
    c: &Contexto<'_>,
    saldos: &BTreeMap<String, Money>,
    palabras: &[&str],
) -> (Vec<String>, Money) {
    let mut cuentas = Vec::new();
    let mut total = Money::ZERO;
    for (cuenta, saldo) in saldos {
        if !cuenta.starts_with('6') || saldo.cents() <= 0 {
            continue;
        }
        let n = c
            .nombres
            .get(cuenta)
            .map(|s| s.to_lowercase())
            .unwrap_or_default();
        if palabras.iter().any(|p| n.contains(p)) {
            cuentas.push(cuenta.clone());
            total += *saldo;
        }
    }
    (cuentas, total)
}

/// Propuestas y notas informativas.
pub(crate) fn calcular(c: &Contexto<'_>) -> (Vec<Propuesta>, Vec<String>) {
    // Saldos sin los asientos de los ajustes de cierre, para las propuestas por cuentas.
    let mut propios = c.saldos.clone();
    for (cuenta, m) in c.cubiertos {
        *propios.entry(cuenta.clone()).or_default() -= *m;
    }
    propios.retain(|_, m| !m.is_zero());
    let mut out = Vec::new();
    let mut notas = Vec::new();
    let mut push = |clave: &str,
                    casilla: u16,
                    importe: Money,
                    concepto: &str,
                    origen: String,
                    nota: Option<String>,
                    incluir: bool| {
        if importe.cents() > 0 {
            out.push(Propuesta {
                clave: clave.into(),
                casilla,
                importe,
                concepto: concepto.into(),
                origen,
                nota,
                incluir,
            });
        }
    };

    // Amortización (fichas de inmovilizado con coeficiente fiscal).
    push(
        "amortizacion_aumento",
        303,
        c.amortizacion.0,
        "Amortización contable superior a la fiscal",
        "fichas de inmovilizado (coeficiente fiscal)".into(),
        None,
        true,
    );
    push(
        "amortizacion_disminucion",
        304,
        c.amortizacion.1,
        "Amortización fiscal superior a la contable",
        "fichas de inmovilizado (coeficiente fiscal)".into(),
        None,
        true,
    );

    // Multas, sanciones y recargos.
    let (multas, total_multas) = por_nombre(c, &propios, &["multa", "sanci", "recargo"]);
    push(
        "multas",
        1815,
        total_multas,
        "Multas, sanciones y recargos",
        lista(&multas),
        None,
        true,
    );
    let otros_678: Money = propios
        .iter()
        .filter(|(k, m)| k.starts_with("678") && !multas.contains(k) && m.cents() > 0)
        .map(|(_, m)| *m)
        .sum();
    push(
        "gastos_excepcionales",
        1815,
        otros_678,
        "Gastos excepcionales (678) que pueden ser multas o sanciones",
        "cuenta 678".into(),
        Some(
            "No se incluye: confirma la parte que sea multa, sanción o recargo (art. 15.c LIS); \
             el resto de gastos excepcionales es deducible si está justificado."
                .into(),
        ),
        false,
    );

    // Donativos y liberalidades.
    let (donativos, total_donativos) =
        por_nombre(c, &propios, &["donativ", "donaci", "liberalidad"]);
    push(
        "donativos",
        339,
        total_donativos,
        "Donativos y liberalidades",
        lista(&donativos),
        Some(
            "Los donativos a entidades de la Ley 49/2002 no son gasto, pero dan derecho a la \
             deducción de la [00565] (art. 20 Ley 49/2002)."
                .into(),
        ),
        true,
    );

    // Atenciones a clientes o proveedores por encima del 1 % de la cifra de negocios.
    let incn = cifra_de_negocios(c.saldos);
    let (atenciones, total_atenciones) = por_nombre(c, &propios, &["atenci", "obsequi", "regalo"]);
    let tope = incn.max(Money::ZERO).apply_rate(Rate::percent(1));
    if total_atenciones.cents() > 0 {
        let exceso = (total_atenciones - tope).max(Money::ZERO);
        if exceso.cents() > 0 {
            push(
                "atenciones",
                339,
                exceso,
                "Atenciones a clientes o proveedores por encima del 1 % de la cifra de negocios",
                format!(
                    "{}: {total_atenciones}; 1 % de la cifra de negocios ({incn}): {tope}",
                    lista(&atenciones)
                ),
                None,
                true,
            );
        } else {
            notas.push(format!(
                "Atenciones a clientes y proveedores: {total_atenciones}, dentro del 1 % de la \
                 cifra de negocios ({tope}): deducibles (art. 15.e LIS)."
            ));
        }
    }

    // Deterioros (art. 13.2 LIS), sin lo de los ajustes de cierre (`propios`) ni lo que viene de la
    // cartera de valores (va aparte, abajo).
    let ajeno = |p: &[&str]| suma(&propios, p) - suma(&c.valores.saldos, p);
    let det = |p: &[&str]| ajeno(p).max(Money::ZERO);
    push(
        "deterioro_inmovilizado",
        331,
        det(&["690", "691", "692"]),
        "Deterioro del inmovilizado material, intangible e inversiones inmobiliarias",
        "cuentas 690, 691 y 692".into(),
        None,
        true,
    );
    push(
        "deterioro_participaciones",
        325,
        det(&["696"]),
        "Deterioro de participaciones en el capital o fondos propios",
        "cuenta 696".into(),
        None,
        true,
    );
    push(
        "deterioro_valores_deuda",
        327,
        det(&["698"]),
        "Deterioro de valores representativos de deuda",
        "cuenta 698".into(),
        None,
        true,
    );
    let reversion = |p: &[&str]| (-ajeno(p)).max(Money::ZERO);
    push(
        "reversion_inmovilizado",
        332,
        reversion(&["790", "791", "792"]),
        "Reversión del deterioro del inmovilizado",
        "cuentas 790, 791 y 792".into(),
        Some(
            "Sólo se resta si el deterioro que revierte no se dedujo cuando se dotó (art. 13.2 \
             LIS): confírmalo."
                .into(),
        ),
        false,
    );
    push(
        "reversion_participaciones",
        326,
        reversion(&["796"]),
        "Reversión del deterioro de participaciones",
        "cuenta 796".into(),
        Some("Sólo si el deterioro no se dedujo (art. 13.2.b LIS): confírmalo.".into()),
        false,
    );
    // Cartera de valores: deterioro y reversión del cierre, y valor razonable (art. 15.l).
    push(
        "valores_deterioro",
        325,
        c.valores.deterioro,
        "Deterioro de la cartera de valores (acciones, ETF y fondos a coste)",
        "Inversiones → valoración al cierre".into(),
        None,
        true,
    );
    push(
        "valores_reversion",
        326,
        c.valores.reversion,
        "Reversión del deterioro de la cartera de valores de cierres anteriores",
        "Inversiones → valoración al cierre".into(),
        Some(
            "Se resta porque el deterioro se sumó en el 200 del año en que se dotó (art. 13.2.b \
             LIS); si entonces no se sumó, descártala."
                .into(),
        ),
        true,
    );
    push(
        "valores_vr_perdidas",
        413,
        c.valores.perdidas_vr,
        "Bajadas de valor razonable de participaciones de al menos el 5 % (cartera de negociación)",
        "Inversiones → valoración al cierre (cuenta 6630)".into(),
        Some(
            "No se incluye: las bajadas de valor razonable que van a resultados son deducibles \
             salvo en participaciones que cumplan el art. 21.1 LIS (al menos el 5 %): sólo \
             entonces no lo son (art. 15.l LIS), salvo que antes se integrara una subida del \
             mismo valor. Inclúyela por la parte de esas participaciones."
                .into(),
        ),
        false,
    );
    push(
        "provisiones",
        337,
        det(&["695"]),
        "Provisiones por operaciones comerciales no deducibles",
        "cuenta 695".into(),
        Some(
            "No se incluye: la provisión por devoluciones de ventas no es deducible, la de \
             garantías de reparación y revisión sí hasta el límite del art. 14.6 LIS. Confirma \
             la parte no deducible."
                .into(),
        ),
        false,
    );

    // Limitación de gastos financieros (art. 16 LIS): sólo por encima de 1 M€ netos.
    let gastos = suma(c.saldos, &["661", "662", "664", "665", "669"]);
    let ingresos = -suma(c.saldos, &["761", "762", "769"]);
    let netos = gastos - ingresos;
    let millon = if c.dias > 0 && c.dias < 365 {
        Money::from_cents(100_000_000 * c.dias / 365)
    } else {
        Money::from_euros(1_000_000, 0)
    };
    if netos > millon {
        // Resultado de explotación: grupos 6 y 7 sin los financieros (66, 76, 696–699,
        // 796–799, 673, 773) ni el IS; más amortización, deterioros y resultados del
        // inmovilizado, menos subvenciones de capital, más dividendos (760).
        let financieros = [
            "66", "76", "630", "633", "638", "696", "697", "698", "699", "796", "797", "798",
            "799", "673", "773",
        ];
        let explotacion: Money = -c
            .saldos
            .iter()
            .filter(|(k, _)| {
                (k.starts_with('6') || k.starts_with('7'))
                    && !financieros.iter().any(|p| k.starts_with(p))
            })
            .map(|(_, m)| *m)
            .sum::<Money>();
        let bo = explotacion
            + suma(c.saldos, &["68"])
            + suma(c.saldos, &["746"])
            + suma(c.saldos, &["690", "691", "692", "790", "791", "792"])
            + suma(c.saldos, &["670", "671", "672", "770", "771", "772"])
            - suma(c.saldos, &["760"]);
        let limite = millon.max(bo.max(Money::ZERO).apply_rate(Rate::percent(30)));
        push(
            "gastos_financieros",
            363,
            (netos - limite).max(Money::ZERO),
            "Gastos financieros netos por encima del 30 % del beneficio operativo",
            format!(
                "gastos financieros netos {netos}; beneficio operativo {bo}; límite {limite}"
            ),
            // TODO(VERIFY): partidas del beneficio operativo (art. 16.1 LIS).
            Some(
                "Cálculo aproximado: el beneficio operativo se calcula con las cuentas del PGC (art. \
                 16.1 LIS); revisa las partidas y los gastos no deducidos de otros años [00364]."
                    .into(),
            ),
            true,
        );
    } else if netos.cents() > 0 {
        notas.push(format!(
            "Gastos financieros netos del ejercicio: {netos}. Por debajo de {millon}, deducibles \
             sin límite (art. 16.1 LIS)."
        ));
    }

    // Exención del art. 21 desde la ficha de participadas.
    for p in c.participadas {
        let exento: Money = p
            .dividendos
            .iter()
            .filter(|d| d.art21.estado != EstadoArt21::NoCumple)
            .map(|d| d.exento)
            .sum();
        let condicionada = p
            .dividendos
            .iter()
            .any(|d| d.art21.estado == EstadoArt21::Condicionada);
        let residente = p.participada.country == "ES";
        push(
            &format!("art21:{}", p.participada.nif),
            if residente { 370 } else { 2181 },
            exento,
            &format!(
                "Exención del 95 % de los dividendos de {}",
                p.participada.name
            ),
            format!(
                "ficha de {} (dividendos de {})",
                p.participada.name, p.bruto
            ),
            condicionada.then(|| {
                "Exención condicionada a mantener la participación hasta cumplir el año (art. \
                 21.1.a LIS)."
                    .to_owned()
            }),
            true,
        );
    }
    (out, notas)
}
