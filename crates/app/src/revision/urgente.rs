//! «Urgente: puede haber deuda»: los hallazgos que pueden suponer dinero pendiente con Hacienda
//! (o la Seguridad Social), con el recargo estimado a fecha de hoy y el siguiente escalón.
//!
//! Dos situaciones distintas, y la ley las trata distinto:
//!
//! - **Sin presentar** (o una complementaria por hacer): si se presenta ahora sin requerimiento,
//!   recargo del art. 27.2 LGT: 1 % más otro 1 % por cada mes completo desde el fin del plazo;
//!   pasados doce meses, 15 % más intereses de demora. Sube cada mes: se dice cuándo y cuánto.
//! - **Presentado y sin pagar**: la deuda entra en periodo ejecutivo al día siguiente del plazo
//!   (art. 161.1.b LGT) con el recargo del art. 28 LGT: 5 % si se paga antes de que notifiquen la
//!   providencia de apremio; 10 % si se paga en el plazo de la providencia; 20 % e intereses
//!   después. TaxPain no sabe si la providencia ya se ha notificado, y lo dice.

use serde::{Deserialize, Serialize};
use taxpain_core::calendar::rules::add_months;
use taxpain_core::money::{Money, Rate};
use time::{Date, Duration};

use super::{Hallazgo, eur, fecha};

/// Lo urgente de un hallazgo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Urgente {
    /// Lo que puede deberse sin contar el recargo (la cuota). `None` si no se sabe.
    pub en_juego: Option<Money>,
    /// Último día del plazo de pago o de presentación (`aaaa-mm-dd`), si se sabe.
    pub vencio: Option<String>,
    /// Porcentaje del recargo a fecha de la revisión.
    pub porcentaje_hoy: Option<i32>,
    /// Recargo estimado a fecha de la revisión.
    pub recargo_hoy: Option<Money>,
    /// Desde qué día (`aaaa-mm-dd`) sube al siguiente escalón, si se sabe.
    pub siguiente_desde: Option<String>,
    pub siguiente_porcentaje: Option<i32>,
    /// Recargo con el siguiente escalón.
    pub siguiente_recargo: Option<Money>,
    /// Explicación: de qué recargo se trata y lo que no se sabe.
    pub nota: String,
}

fn iso(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), u8::from(d.month()), d.day())
}

fn pct(m: Money, p: i32) -> Money {
    m.apply_rate(Rate::percent(p))
}

impl Urgente {
    /// Lo que se presentaría hoy fuera de plazo sin requerimiento (art. 27.2 LGT). Sin `cuota`,
    /// sólo los porcentajes.
    pub fn art27(vencio: Date, cuota: Option<Money>, hoy: Date) -> Self {
        // Con una cuota de 100 € se sacan los porcentajes aunque no se sepa el importe.
        let base = cuota
            .filter(|c| *c > Money::ZERO)
            .unwrap_or(Money::from_euros(100, 0));
        let hoy_r = super::cruces::plazos::recargo(vencio, hoy, base);
        let (porcentaje_hoy, meses, intereses) = match &hoy_r {
            Some(r) => (Some(r.tipo.bp() / 100), r.meses, r.intereses),
            None => (None, -1, false),
        };
        let (siguiente_desde, siguiente_porcentaje) = if intereses {
            (None, None)
        } else if hoy <= vencio {
            (Some(vencio + Duration::days(1)), Some(1))
        } else if meses >= 11 {
            // A los doce meses (el día siguiente), el 15 %.
            (Some(add_months(vencio, 12) + Duration::days(1)), Some(15))
        } else {
            (
                Some(add_months(vencio, meses + 1)),
                porcentaje_hoy.map(|p| p + 1),
            )
        };
        let con = |p: Option<i32>| cuota.zip(p).map(|(c, p)| pct(c, p));
        let mut nota = match porcentaje_hoy {
            None => format!(
                "El plazo acaba el {}: aún se puede presentar sin recargo.",
                fecha(vencio)
            ),
            Some(p) if intereses => format!(
                "Si se presenta hoy sin requerimiento, recargo del {p} % más intereses de demora \
                 desde el {} (art. 27.2 LGT); los intereses siguen sumando cada día.",
                fecha(add_months(vencio, 12) + Duration::days(1))
            ),
            Some(p) => format!(
                "Si se presenta hoy sin requerimiento, recargo del {p} % (art. 27.2 LGT: 1 % más \
                 otro 1 % por cada mes completo desde el {}).",
                fecha(vencio)
            ),
        };
        if cuota.is_none() {
            nota.push_str(
                " No se sabe lo que sale a ingresar: el recargo es ese porcentaje de la cuota.",
            );
        }
        nota.push_str(
            " Si Hacienda lo ha requerido antes, ya no hay recargo sino sanción (art. 191 LGT).",
        );
        Urgente {
            en_juego: cuota,
            vencio: Some(iso(vencio)),
            porcentaje_hoy,
            recargo_hoy: con(porcentaje_hoy),
            siguiente_desde: siguiente_desde.map(iso),
            siguiente_porcentaje,
            siguiente_recargo: con(siguiente_porcentaje),
            nota,
        }
    }

    /// Presentado y sin ingresar: periodo ejecutivo (arts. 161 y 28 LGT).
    pub fn sin_ingreso(vencio: Date, cuota: Money, hoy: Date) -> Self {
        let vencido = hoy > vencio;
        Urgente {
            en_juego: Some(cuota),
            vencio: Some(iso(vencio)),
            porcentaje_hoy: vencido.then_some(5),
            recargo_hoy: vencido.then(|| pct(cuota, 5)),
            siguiente_desde: if vencido {
                None
            } else {
                Some(iso(vencio + Duration::days(1)))
            },
            siguiente_porcentaje: Some(if vencido { 10 } else { 5 }),
            siguiente_recargo: Some(pct(cuota, if vencido { 10 } else { 5 })),
            nota: format!(
                "Si se presentó sin ingresar, la deuda está en periodo ejecutivo desde el {} \
                 (art. 161 LGT): recargo del 5 % si se paga entera antes de que notifiquen la \
                 providencia de apremio, del 10 % si se paga en el plazo que da la providencia y \
                 del 20 % más intereses de demora después (art. 28 LGT). TaxPain no sabe si ya \
                 se ha notificado la providencia: mira las notificaciones de la Sede. Si se pagó \
                 desde otra cuenta o está aplazado, no hay deuda.",
                fecha(vencio + Duration::days(1))
            ),
        }
    }

    /// Ya presentado fuera de plazo: el recargo del art. 27 quedó fijado por la fecha de
    /// presentación (no sube), y se paga con la carta de pago que manda Hacienda.
    pub fn recargo_fijado(vencio: Date, recargo: Money, porcentaje: i32) -> Self {
        Urgente {
            en_juego: Some(recargo),
            vencio: Some(iso(vencio)),
            porcentaje_hoy: Some(porcentaje),
            recargo_hoy: Some(recargo),
            siguiente_desde: None,
            siguiente_porcentaje: None,
            siguiente_recargo: None,
            nota: format!(
                "El recargo del {porcentaje} % (art. 27 LGT) lo fija la fecha en que se presentó: \
                 no sube por esperar. Hacienda lo liquida con una carta de pago; pagado entero en \
                 su plazo, se reduce un 25 % (art. 27.5 LGT). Si no se paga en ese plazo, pasa a \
                 periodo ejecutivo con el recargo del art. 28 LGT. TaxPain no sabe si la carta \
                 ya ha llegado ni si se ha pagado; tampoco si la cuota se ingresó."
            ),
        }
    }

    /// Sin cálculo: algo que indica deuda sin importe ni plazo.
    pub fn sin_calculo(en_juego: Option<Money>, nota: impl Into<String>) -> Self {
        Urgente {
            en_juego,
            vencio: None,
            porcentaje_hoy: None,
            recargo_hoy: None,
            siguiente_desde: None,
            siguiente_porcentaje: None,
            siguiente_recargo: None,
            nota: nota.into(),
        }
    }

    /// Lo que hay en juego: la cuota más el recargo de hoy.
    pub fn total(&self) -> Money {
        self.en_juego.unwrap_or(Money::ZERO) + self.recargo_hoy.unwrap_or(Money::ZERO)
    }

    /// Una línea para el informe: cuánto, recargo de hoy y siguiente escalón.
    pub fn resumen(&self) -> String {
        let mut p = Vec::new();
        if let Some(c) = self.en_juego {
            p.push(format!("En juego: {}", eur(c)));
        }
        if let Some(v) = self.vencio.as_deref().and_then(parse) {
            p.push(format!("venció el {}", fecha(v)));
        }
        match (self.porcentaje_hoy, self.recargo_hoy) {
            (Some(pc), Some(r)) => p.push(format!("recargo hoy {pc} %: {}", eur(r))),
            (Some(pc), None) => p.push(format!("recargo hoy {pc} %")),
            _ => {}
        }
        match (
            self.siguiente_porcentaje,
            self.siguiente_desde.as_deref().and_then(parse),
            self.siguiente_recargo,
        ) {
            (Some(pc), Some(d), Some(r)) => {
                let sube = r - self.recargo_hoy.unwrap_or(Money::ZERO);
                p.push(format!(
                    "desde el {} sube al {pc} %: {} ({} más)",
                    fecha(d),
                    eur(r),
                    eur(sube)
                ))
            }
            (Some(pc), Some(d), None) => p.push(format!("desde el {} sube al {pc} %", fecha(d))),
            (Some(pc), None, Some(r)) => p.push(format!("siguiente escalón {pc} %: {}", eur(r))),
            _ => {}
        }
        if p.is_empty() {
            "Importe sin calcular".to_owned()
        } else {
            let mut s = p.join("; ");
            s.push('.');
            s
        }
    }
}

fn parse(s: &str) -> Option<Date> {
    let f = time::macros::format_description!("[year]-[month]-[day]");
    Date::parse(s, &f).ok()
}

/// Ordena: primero los urgentes, por lo que hay en juego (de más a menos) y luego por
/// antigüedad (lo que venció antes, primero); después los demás como siempre (gravedad, área,
/// ejercicio más reciente, título).
pub fn ordenar(h: &mut [Hallazgo]) {
    h.sort_by(|a, b| match (&a.urgente, &b.urgente) {
        (Some(x), Some(y)) => y
            .total()
            .cmp(&x.total())
            .then(
                x.vencio
                    .as_deref()
                    .unwrap_or("9999")
                    .cmp(y.vencio.as_deref().unwrap_or("9999")),
            )
            .then(a.ejercicio.cmp(&b.ejercicio))
            .then(a.titulo.cmp(&b.titulo)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a
            .gravedad
            .cmp(&b.gravedad)
            .then(a.area.cmp(&b.area))
            .then(b.ejercicio.cmp(&a.ejercicio))
            .then(a.titulo.cmp(&b.titulo)),
    });
}

#[cfg(test)]
mod tests {
    use time::macros::date;

    use super::*;

    fn e(x: i64) -> Money {
        Money::from_euros(x, 0)
    }

    #[test]
    fn recargo_por_meses_y_siguiente_escalon() {
        let fin = date!(2026 - 07 - 20);
        // Antes del plazo: sin recargo; desde el día siguiente, el 1 %.
        let u = Urgente::art27(fin, Some(e(1_000)), date!(2026 - 07 - 10));
        assert_eq!(u.porcentaje_hoy, None);
        assert_eq!(u.siguiente_desde.as_deref(), Some("2026-07-21"));
        assert_eq!(u.siguiente_recargo, Some(e(10)));
        // Tres meses completos y algo: 4 %, y el 20/11 sube al 5 %.
        let u = Urgente::art27(fin, Some(e(1_000)), date!(2026 - 11 - 16));
        assert_eq!(u.porcentaje_hoy, Some(4));
        assert_eq!(u.recargo_hoy, Some(e(40)));
        assert_eq!(u.siguiente_desde.as_deref(), Some("2026-11-20"));
        assert_eq!(u.siguiente_porcentaje, Some(5));
        assert_eq!(u.siguiente_recargo, Some(e(50)));
        assert!(u.resumen().contains("sube al 5 %"), "{}", u.resumen());
        // Once meses: el 12 %, y pasados los doce meses el 15 %.
        let u = Urgente::art27(fin, Some(e(1_000)), date!(2027 - 06 - 25));
        assert_eq!(u.porcentaje_hoy, Some(12));
        assert_eq!(u.siguiente_desde.as_deref(), Some("2027-07-21"));
        assert_eq!(u.siguiente_porcentaje, Some(15));
        // Más de un año: 15 % e intereses, sin siguiente escalón.
        let u = Urgente::art27(fin, Some(e(1_000)), date!(2027 - 09 - 01));
        assert_eq!(u.porcentaje_hoy, Some(15));
        assert_eq!(u.siguiente_porcentaje, None);
        assert!(u.nota.contains("intereses"));
        // Sin cuota: porcentajes, sin importes.
        let u = Urgente::art27(fin, None, date!(2026 - 11 - 16));
        assert_eq!(u.porcentaje_hoy, Some(4));
        assert_eq!(u.recargo_hoy, None);
    }

    #[test]
    fn sin_ingreso_es_periodo_ejecutivo() {
        let u = Urgente::sin_ingreso(
            date!(2026 - 07 - 20),
            Money::from_cents(102_426),
            date!(2026 - 11 - 16),
        );
        assert_eq!(u.porcentaje_hoy, Some(5));
        assert_eq!(u.recargo_hoy, Some(Money::from_cents(5_121)));
        assert_eq!(u.siguiente_porcentaje, Some(10));
        assert!(u.nota.contains("no sabe si ya se ha notificado"));
    }

    #[test]
    fn urgentes_primero_por_importe_y_antiguedad() {
        use super::super::{Area, Gravedad, Responsable};
        let h = |t: &str, g: Gravedad, u: Option<Urgente>| Hallazgo {
            codigo: "x".into(),
            area: Area::Modelos,
            gravedad: g,
            responsable: Responsable::Gestoria,
            ejercicio: Some(2026),
            titulo: t.into(),
            detalle: String::new(),
            ficheros: vec![],
            importe: None,
            rectificable_hasta: None,
            rectificable_aprox: false,
            terceros: None,
            urgente: u,
        };
        let hoy = date!(2026 - 11 - 16);
        let mut v = vec![
            h("error", Gravedad::Error, None),
            h(
                "poco",
                Gravedad::Info,
                Some(Urgente::art27(date!(2026 - 07 - 20), Some(e(100)), hoy)),
            ),
            h(
                "viejo",
                Gravedad::Aviso,
                Some(Urgente::art27(date!(2025 - 07 - 20), Some(e(1_000)), hoy)),
            ),
            h(
                "nuevo",
                Gravedad::Aviso,
                Some(Urgente::art27(date!(2026 - 07 - 20), Some(e(1_000)), hoy)),
            ),
            h(
                "mucho",
                Gravedad::Aviso,
                Some(Urgente::sin_ingreso(date!(2026 - 07 - 20), e(5_000), hoy)),
            ),
        ];
        ordenar(&mut v);
        let t: Vec<&str> = v.iter().map(|x| x.titulo.as_str()).collect();
        // «viejo» lleva el 15 % y «nuevo» el 4 %: va antes por importe.
        assert_eq!(t, ["mucho", "viejo", "nuevo", "poco", "error"]);
        let mut v = vec![
            h(
                "b",
                Gravedad::Aviso,
                Some(Urgente::sin_calculo(Some(e(10)), "")),
            ),
            h(
                "a",
                Gravedad::Aviso,
                Some(Urgente {
                    vencio: Some("2024-01-20".into()),
                    ..Urgente::sin_calculo(Some(e(10)), "")
                }),
            ),
        ];
        ordenar(&mut v);
        assert_eq!(v[0].titulo, "a", "a igual importe, lo más antiguo");
    }
}
