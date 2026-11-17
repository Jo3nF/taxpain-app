//! Comprobación de un extracto con sus saldos, y el signo de cada importe deducido del saldo
//! cuando el extracto no lo da (muchos PDF ponen cargos y abonos sin signo).

use serde::Serialize;
use taxpain_core::money::Money;

use time::Date;

use super::{Extracto, Movimiento};

/// Resultado de comprobar un extracto con sus saldos.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "estado", rename_all = "snake_case")]
pub enum Cuadre {
    /// Saldo inicial + movimientos = saldo final y, si hay saldo en cada fila, fila a fila.
    Cuadra,
    /// No cuadra: lo que falta para llegar al saldo que dice el banco y la primera fila
    /// (desde 0, en orden cronológico) en la que deja de cuadrar.
    NoCuadra {
        diferencia: Money,
        fila: Option<usize>,
        detalle: String,
    },
    /// El extracto no trae saldos con los que comprobarlo (listados de tarjeta, por ejemplo).
    SinSaldos,
}

impl Cuadre {
    pub fn cuadra(&self) -> bool {
        matches!(self, Self::Cuadra)
    }
}

/// Ordena los movimientos de más antiguo a más nuevo. Si vienen del más nuevo al más antiguo,
/// se les da la vuelta (así también queda bien el orden dentro de cada día); si van mezclados,
/// se ordenan por fecha sin cambiar el orden dentro del día.
pub(crate) fn ordenar(m: &mut [Movimiento]) {
    let (Some(primera), Some(ultima)) = (m.first(), m.last()) else {
        return;
    };
    if primera.fecha > ultima.fecha {
        m.reverse();
    }
    if !m.windows(2).all(|w| w[0].fecha <= w[1].fecha) {
        m.sort_by_key(|x| x.fecha);
    }
}

/// Dentro de cada día, pone los movimientos en el orden en que encadenan sus saldos (hay
/// bancos que dentro del día los listan en otro orden). Si un día no encadena, lo deja como
/// estaba.
fn encadenar_dias(e: &mut Extracto) {
    let mut previo = e.saldo_inicial.or_else(|| {
        let m = e.movimientos.first()?;
        Some(m.saldo? - m.importe)
    });
    let mut i = 0;
    while i < e.movimientos.len() {
        let fecha = e.movimientos[i].fecha;
        let fin = e.movimientos[i..]
            .iter()
            .position(|m| m.fecha != fecha)
            .map_or(e.movimientos.len(), |p| i + p);
        let dia = &mut e.movimientos[i..fin];
        let ultimo_original = dia.last().and_then(|m| m.saldo);
        if let Some(p) = previo
            && dia.len() > 1
            && dia.iter().all(|m| m.saldo.is_some())
        {
            let mut pendientes: Vec<Movimiento> = dia.to_vec();
            let mut orden: Vec<Movimiento> = Vec::with_capacity(dia.len());
            let mut saldo = p;
            while let Some(k) = pendientes
                .iter()
                .position(|m| m.saldo == Some(saldo + m.importe))
            {
                let m = pendientes.remove(k);
                saldo += m.importe;
                orden.push(m);
            }
            if pendientes.is_empty() {
                dia.clone_from_slice(&orden);
            }
        }
        previo = match (previo, ultimo_original) {
            (_, Some(s)) => Some(dia.last().and_then(|m| m.saldo).unwrap_or(s)),
            (Some(p), None) => Some(p + dia.iter().map(|m| m.importe).sum::<Money>()),
            (None, None) => None,
        };
        i = fin;
    }
}

/// Completa el saldo inicial y el final con los de las filas, si el extracto no los da aparte.
pub(crate) fn completar_saldos(e: &mut Extracto) {
    encadenar_dias(e);
    if e.saldo_inicial.is_none()
        && let Some(m) = e.movimientos.first()
        && let Some(s) = m.saldo
    {
        e.saldo_inicial = Some(s - m.importe);
    }
    if e.saldo_final.is_none()
        && let Some(s) = e.movimientos.last().and_then(|m| m.saldo)
    {
        e.saldo_final = Some(s);
    }
}

/// Comprueba el extracto (con los movimientos ya en orden cronológico). Devuelve también los
/// días cuyas filas no encadenan el saldo entre sí pero que, enteros, sí cuadran: el banco lista
/// los movimientos del día en otro orden, o el saldo de cada fila ya incluye uno que se anuló
/// el mismo día (BBVA). Esos días no hacen que el extracto no cuadre.
pub(crate) fn comprobar(e: &Extracto) -> (Cuadre, Vec<Date>) {
    let mut previo = e.saldo_inicial;
    let mut primera_mala: Option<(usize, Money)> = None;
    let mut alguna_fila = false;
    let mut dias_revueltos = Vec::new();
    let mut i = 0;
    while i < e.movimientos.len() {
        let fecha = e.movimientos[i].fecha;
        let fin = e.movimientos[i..]
            .iter()
            .position(|m| m.fecha != fecha)
            .map_or(e.movimientos.len(), |p| i + p);
        let dia = &e.movimientos[i..fin];
        let apertura = previo;
        let mut mala_del_dia: Option<(usize, Money)> = None;
        for (k, m) in dia.iter().enumerate() {
            if let (Some(p), Some(s)) = (previo, m.saldo) {
                alguna_fila = true;
                if mala_del_dia.is_none() && p + m.importe != s {
                    mala_del_dia = Some((i + k, s - (p + m.importe)));
                }
            }
            previo = m.saldo.or(previo.map(|p| p + m.importe));
        }
        if let Some(mala) = mala_del_dia {
            let suma: Money = dia.iter().map(|m| m.importe).sum();
            let cierre = dia.last().and_then(|m| m.saldo);
            match (apertura, cierre) {
                (Some(a), Some(c)) if a + suma == c => dias_revueltos.push(fecha),
                _ if primera_mala.is_none() => primera_mala = Some(mala),
                _ => {}
            }
        }
        i = fin;
    }
    let suma: Money = e.movimientos.iter().map(|m| m.importe).sum();
    if let (Some(ini), Some(fin)) = (e.saldo_inicial, e.saldo_final) {
        let calculado = ini + suma;
        if calculado != fin {
            return (
                Cuadre::NoCuadra {
                    diferencia: fin - calculado,
                    fila: primera_mala.map(|(i, _)| i),
                    detalle: format!(
                        "saldo inicial {ini} + movimientos {suma} = {calculado}, pero el extracto \
                     dice {fin}"
                    ),
                },
                dias_revueltos,
            );
        }
    }
    if let Some((i, d)) = primera_mala {
        let m = &e.movimientos[i];
        return (
            Cuadre::NoCuadra {
                diferencia: d,
                fila: Some(i),
                detalle: format!(
                    "el saldo deja de cuadrar el {} («{}», {}): faltan {d}",
                    m.fecha, m.concepto, m.importe
                ),
            },
            dias_revueltos,
        );
    }
    let cuadre = if (e.saldo_inicial.is_some() && e.saldo_final.is_some()) || alguna_fila {
        Cuadre::Cuadra
    } else {
        Cuadre::SinSaldos
    };
    (cuadre, dias_revueltos)
}

/// Signos deducidos del saldo de cada fila.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Signos {
    /// Cada importe con su signo, en el orden del documento (`None` si no se ha podido saber).
    pub importes: Vec<Option<Money>>,
    /// ¿El documento va de más antiguo a más nuevo?
    pub cronologico: bool,
    /// Cuántas filas se han resuelto.
    pub resueltas: usize,
}

/// Deduce el signo de cada importe (que viene sin él) con el saldo de cada fila: la diferencia
/// con el saldo de la fila anterior en el tiempo tiene que ser el importe, con su signo. Prueba
/// los dos órdenes del documento (de más antiguo a más nuevo y al revés) y se queda con el que
/// resuelve más filas. `inicial` es el saldo antes del movimiento más antiguo, si se sabe.
/// `None` si no hay saldos.
pub(crate) fn signos_por_saldo(
    importes: &[Money],
    saldos: &[Option<Money>],
    inicial: Option<Money>,
) -> Option<Signos> {
    if importes.len() != saldos.len() || saldos.iter().all(Option::is_none) {
        return None;
    }
    let n = importes.len();
    let resolver = |cronologico: bool| -> Vec<Option<Money>> {
        (0..n)
            .map(|i| {
                let anterior = if cronologico {
                    if i == 0 { inicial } else { saldos[i - 1] }
                } else if i + 1 == n {
                    inicial
                } else {
                    saldos[i + 1]
                };
                let d = saldos[i]? - anterior?;
                (d.abs() == importes[i].abs()).then_some(d)
            })
            .collect()
    };
    let adelante = resolver(true);
    let atras = resolver(false);
    let cuenta = |v: &[Option<Money>]| v.iter().filter(|x| x.is_some()).count();
    let (importes, cronologico) = if cuenta(&atras) > cuenta(&adelante) {
        (atras, false)
    } else {
        (adelante, true)
    };
    let resueltas = cuenta(&importes);
    Some(Signos {
        importes,
        cronologico,
        resueltas,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn mov(fecha: Date, importe: i64, saldo: Option<i64>) -> Movimiento {
        Movimiento {
            importe_eur: None,
            fecha,
            fecha_valor: None,
            concepto: format!("mov {importe}"),
            importe: Money::from_cents(importe),
            saldo: saldo.map(Money::from_cents),
            referencia: String::new(),
        }
    }

    fn extracto(m: Vec<Movimiento>) -> Extracto {
        let mut e = Extracto::nuevo("prueba");
        e.movimientos = m;
        e
    }

    #[test]
    fn cuadra_con_los_saldos_de_cada_fila() {
        // Del más nuevo al más antiguo, como muchos bancos.
        let mut e = extracto(vec![
            mov(date!(2025 - 01 - 03), -2_000, Some(9_000)),
            mov(date!(2025 - 01 - 02), 5_000, Some(11_000)),
            mov(date!(2025 - 01 - 01), -4_000, Some(6_000)),
        ]);
        assert_eq!(e.cerrar(), Cuadre::Cuadra);
        assert_eq!(e.movimientos[0].fecha, date!(2025 - 01 - 01));
        assert_eq!(e.saldo_inicial, Some(Money::from_cents(10_000)));
        assert_eq!(e.saldo_final, Some(Money::from_cents(9_000)));
        assert_eq!(e.desde, Some(date!(2025 - 01 - 01)));
        assert_eq!(e.hasta, Some(date!(2025 - 01 - 03)));
    }

    #[test]
    fn no_cuadra_si_falta_un_movimiento() {
        let mut e = extracto(vec![
            mov(date!(2025 - 01 - 01), -4_000, Some(6_000)),
            mov(date!(2025 - 01 - 03), -2_000, Some(1_000)),
        ]);
        let Cuadre::NoCuadra {
            diferencia, fila, ..
        } = e.cerrar()
        else {
            panic!("tenía que no cuadrar");
        };
        assert_eq!((diferencia.cents(), fila), (-3_000, Some(1)));

        let mut e = extracto(vec![mov(date!(2025 - 01 - 01), -4_000, None)]);
        e.saldo_inicial = Some(Money::from_cents(10_000));
        e.saldo_final = Some(Money::from_cents(5_000));
        assert!(
            matches!(e.cerrar(), Cuadre::NoCuadra { diferencia, .. } if diferencia.cents() == -1_000)
        );
    }

    #[test]
    fn sin_saldos_no_se_puede_comprobar() {
        let mut e = extracto(vec![mov(date!(2025 - 01 - 01), -4_000, None)]);
        assert_eq!(e.cerrar(), Cuadre::SinSaldos);
    }

    #[test]
    fn reordena_los_movimientos_del_mismo_dia() {
        // El mismo día, listados en un orden que no encadena con el saldo.
        let mut e = extracto(vec![
            mov(date!(2025 - 01 - 01), -1_000, Some(9_000)),
            mov(date!(2025 - 01 - 02), -500, Some(9_000)),
            mov(date!(2025 - 01 - 02), 500, Some(9_500)),
        ]);
        assert_eq!(e.cerrar(), Cuadre::Cuadra);
        assert_eq!(e.movimientos[1].importe.cents(), 500);
    }

    #[test]
    fn un_dia_que_cuadra_entero_aunque_sus_filas_no() {
        // Como BBVA: el saldo de cada fila del día ya incluye un cargo de 60 que se anuló ese
        // mismo día, así que las filas no encadenan, pero el día entero sí (100 − 10 + 5 = 95).
        let mut e = extracto(vec![
            mov(date!(2025 - 01 - 01), 10_000, Some(10_000)),
            mov(date!(2025 - 01 - 02), -1_000, Some(3_000)),
            mov(date!(2025 - 01 - 02), 500, Some(9_500)),
            mov(date!(2025 - 01 - 03), 500, Some(10_000)),
        ]);
        e.saldo_inicial = Some(Money::from_cents(0));
        assert_eq!(e.cerrar(), Cuadre::Cuadra);
        assert!(
            e.avisos.iter().any(|a| a.contains("2025-01-02")),
            "{:?}",
            e.avisos
        );

        // Si el día entero tampoco cuadra, no cuadra.
        let mut e = extracto(vec![
            mov(date!(2025 - 01 - 01), 10_000, Some(10_000)),
            mov(date!(2025 - 01 - 02), -1_000, Some(3_000)),
            mov(date!(2025 - 01 - 02), 500, Some(9_000)),
        ]);
        e.saldo_inicial = Some(Money::from_cents(0));
        assert!(matches!(e.cerrar(), Cuadre::NoCuadra { fila: Some(1), .. }));
    }

    #[test]
    fn signo_deducido_del_saldo() {
        let m = |c: i64| Money::from_cents(c);
        // De más antiguo a más nuevo, sin signo.
        let s = signos_por_saldo(
            &[m(4_000), m(5_000), m(2_000)],
            &[Some(m(6_000)), Some(m(11_000)), Some(m(9_000))],
            Some(m(10_000)),
        )
        .unwrap();
        assert!(s.cronologico);
        assert_eq!(
            s.importes,
            [Some(m(-4_000)), Some(m(5_000)), Some(m(-2_000))]
        );

        // Del más nuevo al más antiguo y sin saldo inicial: la última fila no se resuelve.
        let s = signos_por_saldo(
            &[m(2_000), m(5_000), m(4_000)],
            &[Some(m(9_000)), Some(m(11_000)), Some(m(6_000))],
            None,
        )
        .unwrap();
        assert!(!s.cronologico);
        assert_eq!(s.importes, [Some(m(-2_000)), Some(m(5_000)), None]);
        assert_eq!(s.resueltas, 2);

        assert_eq!(signos_por_saldo(&[m(1)], &[None], None), None);
    }
}
