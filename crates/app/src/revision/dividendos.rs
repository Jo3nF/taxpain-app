//! Dividendos que paga la empresa sin el acuerdo en la carpeta: si un 123 de la empresa declara
//! retenciones de dividendos ([04] base y [07] retención de «Dividendos y otras rentas de
//! participación en fondos propios», desde los devengos de 2024; ver `taxpain_models::m123`),
//! en la carpeta tiene que estar el acta que acuerda el reparto: la aplicación del resultado del
//! ejercicio anterior con dividendos, un reparto con cargo a reservas o un dividendo a cuenta (y
//! la aplicación del resultado del mismo ejercicio que lo recoge).
//!
//! Los 123 de hasta 2023 no separan los dividendos del resto de rentas (intereses de préstamos
//! de socios…): no se miran.

use std::collections::{BTreeMap, BTreeSet};

use taxpain_core::money::Money;
use time::Date;

use super::expediente::{Doc, Expediente};
use super::{Area, Destinatario, Gravedad, Nuevo, Responsable, Salida, UN_EURO};
use crate::documentos::DocKind;

/// Retención de los dividendos (art. 101.4 LIRPF y art. 128.6.a LIS): el 19 %.
const TIPO: i64 = 19;

/// Lo que declaran los 123 de un ejercicio.
#[derive(Default)]
struct Declarados {
    retencion: Money,
    base: Money,
    periodos: Vec<String>,
}

pub(crate) fn revisar(exp: &Expediente, s: &mut Salida) {
    // Sin carpeta no hay actas que mirar: no se dice que falten.
    if exp.carpeta.is_empty() {
        return;
    }
    let dec = super::modelos::declarados(exp);
    let mut por_año: BTreeMap<i32, Declarados> = BTreeMap::new();
    for ((m, y, p), d) in &dec {
        if m != "123" || *y < 2024 || !d.con_casillas() {
            continue;
        }
        // Sin el desglose no se sabe si son dividendos.
        if ![4, 5, 7, 8].iter().any(|k| d.casillas.contains_key(k)) {
            continue;
        }
        if d.c(7).cents() <= 0 && d.c(4).cents() <= 0 {
            continue;
        }
        let e = por_año.entry(*y).or_default();
        e.retencion += d.c(7);
        e.base += d.c(4);
        e.periodos.push(p.clone());
    }
    for (y, d) in por_año {
        aviso(exp, y, &d, s);
    }
}

fn eur(m: Money) -> String {
    m.to_string()
}

/// ¿Puede el acta acordar el reparto de lo pagado en `y`? La que aprueba las cuentas de `y - 1`
/// (o cualquiera de `y - 1` o `y`: un reparto de reservas o un dividendo a cuenta) y la que
/// aprueba las de `y` (que recoge lo repartido a cuenta).
fn del_reparto(fecha: Option<Date>, ejercicio: Option<i32>, y: i32) -> bool {
    match fecha {
        Some(f) => f.year() == y || f.year() == y - 1,
        None => ejercicio.is_some_and(|e| e == y || e == y - 1),
    }
}

fn aviso(exp: &Expediente, y: i32, d: &Declarados, s: &mut Salida) {
    let bruto = if d.base.cents() > 0 {
        d.base
    } else {
        Money::from_cents((d.retencion.cents() * 100 + TIPO / 2) / TIPO)
    };
    let actas: Vec<&Doc> = exp
        .docs
        .iter()
        .filter(|x| matches!(&x.empresa, Some((DocKind::ActaJunta, _))))
        .collect();
    // Los dividendos de las actas que pueden acordarlo, uno por fecha (dos con la misma fecha
    // son copias de la misma acta).
    let mut acordado: BTreeMap<Option<Date>, Money> = BTreeMap::new();
    for a in actas.iter().filter_map(|x| x.aplicacion.as_ref()) {
        let div = a.dividendos.unwrap_or(Money::ZERO);
        if div.cents() <= 0 || !(del_reparto(a.fecha, a.ejercicio, y) || a.aprueba_las_de(y)) {
            continue;
        }
        let e = acordado.entry(a.fecha).or_insert(Money::ZERO);
        *e = (*e).max(div);
    }
    let suma: Money = acordado.values().copied().sum();
    // Lo recoge (con un 5 % de margen: el bruto sale de la retención).
    if suma.cents() > 0 && suma.cents() * 100 >= bruto.cents() * 95 - UN_EURO.cents() * 100 {
        return;
    }
    // Actas de esos años que no se han podido leer: pueden ser el acuerdo.
    let sin_leer: Vec<String> = actas
        .iter()
        .filter(|x| x.aplicacion.is_none())
        .filter(|x| {
            let fecha = x.empresa.as_ref().and_then(|(_, f)| f.date);
            fecha.is_none_or(|f| (y - 1..=y + 1).contains(&f.year()))
        })
        .map(|x| x.ruta.clone())
        .collect();
    let aprobacion = actas
        .iter()
        .filter_map(|x| x.aplicacion.as_ref().map(|a| (x, a)))
        .find(|(_, a)| a.aprueba_las_de(y));
    let periodos: BTreeSet<&str> = d.periodos.iter().map(String::as_str).collect();
    let periodos: Vec<&str> = periodos.into_iter().collect();
    let base = if d.base.cents() > 0 {
        format!("con una base de {}", eur(d.base))
    } else {
        format!(
            "sin la base: con la retención del 19 %, unos {} de dividendo bruto",
            eur(bruto)
        )
    };
    let mut ficheros: Vec<String> = Vec::new();
    let en_carpeta = if suma.cents() > 0 {
        format!(
            "Las actas de la carpeta recogen {} de dividendos, menos que lo que declara el 123.",
            eur(suma)
        )
    } else if let Some((x, a)) = aprobacion {
        ficheros.push(x.ruta.clone());
        let lleva = a.texto();
        format!(
            "En la carpeta no hay ningún acta que acuerde ese reparto, y la que aprueba las \
             cuentas de {y} {} y no recoge ningún dividendo.",
            if lleva.is_empty() {
                "aplica el resultado".to_owned()
            } else {
                format!("lleva {lleva}")
            }
        )
    } else {
        "En la carpeta no hay ningún acta que acuerde ese reparto.".to_owned()
    };
    let gravedad = if sin_leer.is_empty() || suma.cents() > 0 {
        Gravedad::Aviso
    } else {
        Gravedad::Info
    };
    let leer = if sin_leer.is_empty() {
        String::new()
    } else {
        format!(
            " Hay actas en la carpeta que no se han podido leer ({}): si alguna acuerda el \
             reparto, basta con leerla con IA o con Claude Code.",
            super::lista(&sin_leer, 5)
        )
    };
    ficheros.extend(sin_leer);
    s.add(
        Nuevo {
            codigo: "socios.dividendo_sin_acta",
            area: Area::Socios,
            gravedad,
            responsable: Responsable::Gestoria,
            titulo: format!("Dividendos de {y} sin el acuerdo de reparto en la carpeta"),
            ficheros,
        }
        .detalle(format!(
            "Los 123 de {y} ({}) declaran {} de retenciones de dividendos pagados por la empresa \
             a sus socios, {base}. {en_carpeta} Un dividendo necesita el acuerdo de la junta \
             general o la decisión del socio único, que se consigna en acta (art. 15 LSC): si sale \
             del resultado, en la aplicación del resultado al aprobar las cuentas (arts. 160 y \
             273 LSC); si sale de reservas de libre disposición, un acuerdo de distribución \
             (art. 273 LSC); y si es a cuenta del resultado del ejercicio, el acuerdo con el \
             estado contable que muestre liquidez suficiente (art. 277 LSC), y la aplicación del \
             resultado de {y} debería recogerlo.{leer} Pide el acta a la gestoría.",
            periodos.join(", "),
            eur(d.retencion),
        )),
        Some(y),
        None,
    );
    s.pedir(
        Destinatario::Gestoria,
        format!(
            "El acta de la junta (o la decisión del socio único) que acuerda el reparto de los \
             dividendos de {y} (unos {} brutos, con {} de retención en el 123) y, si fue a \
             cuenta, el estado contable de liquidez",
            eur(bruto),
            eur(d.retencion)
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::super::actas::Aplicacion;
    use super::super::expediente::Clase;
    use super::*;

    fn eur(c: i64) -> Money {
        Money::from_cents(c)
    }

    fn ciento23(y: i32, periodo: &str, base: i64, ret: i64) -> crate::presentados::Filed {
        crate::presentados::Filed {
            id: 1,
            model: "123".into(),
            year: y,
            period: periodo.into(),
            filename: format!("123-{y}-{periodo}.pdf"),
            source: "carpeta".into(),
            boxes: [(4, eur(base)), (5, eur(0)), (7, eur(ret)), (9, eur(ret))]
                .into_iter()
                .collect(),
        }
    }

    fn acta(ruta: &str, a: Option<Aplicacion>) -> Doc {
        Doc {
            ruta: ruta.into(),
            sha: ruta.into(),
            clase: Clase::Otro,
            descripcion: String::new(),
            con_ia: false,
            ruta_año: None,
            ruta_trimestre: None,
            factura: None,
            modelo: None,
            libro: None,
            autorrepercusion: Vec::new(),
            extractos: Vec::new(),
            extracto_error: None,
            empresa: Some((
                DocKind::ActaJunta,
                crate::documentos::Facts {
                    date: a.as_ref().and_then(|a| a.fecha),
                    ..Default::default()
                },
            )),
            tramite: None,
            aplicacion: a,
            nomina: None,
            mencion_sin_iva: None,
            oficial: None,
            texto_no_aeat: false,
            participada: None,
        }
    }

    fn aprobacion_2025(dividendos: Option<i64>, voluntarias: i64) -> Aplicacion {
        Aplicacion {
            ejercicio: Some(2025),
            fecha: Date::from_calendar_date(2026, time::Month::June, 30).ok(),
            aprueba: true,
            completa: true,
            resultado: Some(eur(voluntarias + dividendos.unwrap_or(0))),
            reservas_voluntarias: Some(eur(voluntarias)),
            dividendos: dividendos.map(eur),
            ..Aplicacion::default()
        }
    }

    fn revisar_con(docs: Vec<Doc>) -> Salida {
        let exp = Expediente {
            carpeta: "/carpeta".into(),
            empresa: "EMPRESA DEMO SL".into(),
            own_nif: Some("B12345674".into()),
            docs,
            no_admitidos: Vec::new(),
            presentados: vec![ciento23(2025, "4T", 0, 2_000_000)],
            perfil: None,
            perfil_deducido: None,
            taxpain: Default::default(),
            aeat: Vec::new(),
            comprobado: Default::default(),
        };
        let mut s = Salida::default();
        revisar(&exp, &mut s);
        s
    }

    /// El 123 del 4T de 2025 retiene 20.000 € de dividendos y el acta de las cuentas de 2025 lo
    /// lleva todo a reservas voluntarias: aviso, con el bruto estimado y el acta en «Qué pedir».
    #[test]
    fn dividendo_sin_acta() {
        let s = revisar_con(vec![acta(
            "ACTAS/2026 aprobacion.pdf",
            Some(aprobacion_2025(None, 30_000_000)),
        )]);
        let h = s
            .hallazgos
            .iter()
            .find(|h| h.codigo == "socios.dividendo_sin_acta")
            .expect("aviso");
        assert_eq!(h.gravedad, Gravedad::Aviso);
        assert_eq!(h.responsable, Responsable::Gestoria);
        assert_eq!(h.ejercicio, Some(2025));
        assert!(h.detalle.contains("20.000,00 €"), "{}", h.detalle);
        // 20.000 / 0,19 = 105.263,16 €.
        assert!(h.detalle.contains("105.263,16 €"), "{}", h.detalle);
        assert!(
            h.detalle.contains("300.000,00 € a reservas voluntarias"),
            "{}",
            h.detalle
        );
        assert!(h.detalle.contains("art. 277 LSC"), "{}", h.detalle);
        assert_eq!(h.ficheros, ["ACTAS/2026 aprobacion.pdf"]);
        assert!(
            s.pedir.iter().any(
                |p| p.a_quien == Destinatario::Gestoria && p.que.contains("dividendos de 2025")
            )
        );
        // Sin ninguna acta, también.
        assert!(
            revisar_con(vec![])
                .hallazgos
                .iter()
                .any(|h| h.codigo == "socios.dividendo_sin_acta")
        );
    }

    /// Si el acta recoge el dividendo (en la aplicación del resultado de 2025, a cuenta, o un
    /// reparto de reservas en 2025), no se dice nada.
    #[test]
    fn dividendo_con_acta() {
        let s = revisar_con(vec![acta(
            "ACTAS/2026 aprobacion.pdf",
            Some(aprobacion_2025(Some(10_526_316), 20_000_000)),
        )]);
        assert!(s.hallazgos.is_empty(), "{:?}", s.hallazgos);
        assert!(s.pedir.is_empty());
        let reservas = Aplicacion {
            ejercicio: Some(2025),
            fecha: Date::from_calendar_date(2025, time::Month::November, 20).ok(),
            aprueba: false,
            dividendos: Some(eur(10_600_000)),
            ..Aplicacion::default()
        };
        let s = revisar_con(vec![acta("ACTAS/2025 reparto.pdf", Some(reservas))]);
        assert!(s.hallazgos.is_empty(), "{:?}", s.hallazgos);
    }

    /// Un acta que no se ha podido leer puede ser el acuerdo: sólo una nota.
    #[test]
    fn acta_sin_leer() {
        let s = revisar_con(vec![acta("ACTAS/escaneada.pdf", None)]);
        let h = &s.hallazgos[0];
        assert_eq!(h.gravedad, Gravedad::Info);
        assert!(h.ficheros.contains(&"ACTAS/escaneada.pdf".to_owned()));
    }
}
