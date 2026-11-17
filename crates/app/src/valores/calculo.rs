//! Motor de cálculo de la cartera de valores: coste de lo que queda y resultado de cada venta.
//! Puro: no sabe de base de datos, de divisas ni de asientos (todo llega ya en euros).
//!
//! Reglas (en llano, con su norma; leídas en el BOE el 2026-11-02):
//! - **Sociedad: coste medio ponderado por valor homogéneo.** El PGC (RD 1514/2007, NRV 9.ª
//!   2.4.2, activos financieros a coste) y el PGC PYMES (RD 1515/2007, NRV 9.ª 2.3.2) dicen:
//!   «Cuando deba asignarse valor a estos activos por baja del balance u otro motivo, se aplicará
//!   el método del coste medio ponderado por grupos homogéneos, entendiéndose por estos los
//!   valores que tienen iguales derechos». El PGC lo repite para los de valor razonable con
//!   cambios en el patrimonio neto (2.3.2, «valor medio ponderado»). Para los mantenidos para
//!   negociar no fija método: se usa el mismo por coherencia (`TODO(VERIFY)`). El grupo es el
//!   valor, esté en el bróker que esté (mismo ISIN = mismos derechos).
//! - **Comisiones**: en los activos a coste (o a valor razonable con cambios en el patrimonio
//!   neto) son más coste de adquisición («más los costes de transacción que les sean
//!   directamente atribuibles», NRV 9.ª 2.3.1 PYMES / 2.4.1 PGC) y las de la venta, menos valor
//!   de transmisión; en los mantenidos para negociar van a resultados («se reconocerán en la
//!   cuenta de pérdidas y ganancias del ejercicio», NRV 9.ª 2.2.1 PYMES / 2.1.1 PGC).
//! - **Persona física: FIFO.** Art. 37.2 LIRPF: «cuando existan valores homogéneos se
//!   considerará que los transmitidos por el contribuyente son aquéllos que adquirió en primer
//!   lugar»; los gastos de compra y venta, más coste y menos valor de transmisión (art. 35
//!   LIRPF). FIFO global por valor, sin separar brókeres.
//! - **Desdoblamiento (split)**: los títulos nuevos no cuestan nada; el coste total se reparte
//!   entre todos (y en FIFO conservan la fecha de los antiguos).
//! - **Traspaso entre cuentas propias**: no realiza nada; sólo cambia dónde están los títulos.

use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};
use taxpain_core::cripto::Qty;
use taxpain_core::money::Money;
use time::Date;

/// Método con el que salen los títulos vendidos.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Metodo {
    /// Coste medio ponderado por grupos homogéneos (NRV 9.ª PGC): sociedades.
    #[default]
    Pmp,
    /// Primero en entrar, primero en salir (art. 37.2 LIRPF): personas físicas.
    Fifo,
}

impl Metodo {
    pub fn label(self) -> &'static str {
        match self {
            Metodo::Pmp => "coste medio ponderado",
            Metodo::Fifo => "FIFO",
        }
    }
}

/// Lo que mueve títulos (las demás operaciones no pasan por el motor).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Movimiento {
    Compra,
    Venta,
    /// De `cuenta` a `destino`, sin realizar nada.
    Traspaso,
    /// Títulos que se añaden (o, en negativo, se quitan) sin coste.
    Split,
    /// Valor en libros nuevo de todos los títulos del valor (`euros`): el valor razonable al
    /// cierre en la cartera de negociación. No mueve títulos.
    Valoracion,
}

/// Un movimiento de títulos ya en euros.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mov {
    pub id: i64,
    pub fecha: Date,
    /// `HH:MM:SS`: ordena los del mismo día.
    pub hora: String,
    pub tipo: Movimiento,
    pub valor: i64,
    pub cuenta: i64,
    pub destino: Option<i64>,
    pub cantidad: Qty,
    /// Compra: pagado sin la comisión. Venta: cobrado sin restar la comisión.
    pub euros: Money,
    pub comision: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lote {
    pub fecha: Date,
    pub cantidad: Qty,
    pub coste: Money,
}

/// Lo que queda de un valor en una cuenta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Posicion {
    pub valor: i64,
    pub cuenta: i64,
    pub cantidad: Qty,
    /// Parte del coste del valor que corresponde a los títulos de esta cuenta.
    pub coste: Money,
}

/// Resultado de una venta.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Realizado {
    pub op: i64,
    pub fecha: Date,
    pub valor: i64,
    pub cuenta: i64,
    pub cantidad: Qty,
    /// Valor de transmisión (cobrado, menos la comisión si va al coste).
    pub transmision: Money,
    pub coste: Money,
    /// `transmision − coste`.
    pub resultado: Money,
    /// Fecha de la compra más antigua de lo vendido (para la regla de los dos meses).
    pub desde: Option<Date>,
}

/// Comisión de compra o venta que va a resultados (mantenidos para negociar).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gasto {
    pub op: i64,
    pub fecha: Date,
    pub importe: Money,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Calculo {
    pub posiciones: Vec<Posicion>,
    pub realizados: Vec<Realizado>,
    pub gastos: Vec<Gasto>,
    pub avisos: Vec<String>,
}

#[derive(Default)]
struct Cartera {
    lotes: VecDeque<Lote>,
    por_cuenta: BTreeMap<i64, Qty>,
    /// Lotes de una agrupación (contrasplit) que llega en dos movimientos: primero salen todos
    /// los títulos viejos y luego entran los nuevos. Aquí esperan, con su coste y su fecha, a
    /// los títulos nuevos.
    en_suspenso: VecDeque<Lote>,
}

fn suma(a: Qty, b: Qty) -> Qty {
    a.checked_add(b).unwrap_or(a)
}

fn resta(a: Qty, b: Qty) -> Qty {
    a.checked_sub(b).unwrap_or(a)
}

impl Cartera {
    fn cantidad(&self) -> Qty {
        self.lotes
            .iter()
            .fold(Qty::ZERO, |a, l| suma(a, l.cantidad))
    }

    fn coste(&self) -> Money {
        self.lotes.iter().map(|l| l.coste).sum()
    }

    fn entrada(&mut self, metodo: Metodo, fecha: Date, cantidad: Qty, coste: Money) {
        match (metodo, self.lotes.front_mut()) {
            // Con el coste medio basta un lote que acumula títulos y coste.
            (Metodo::Pmp, Some(l)) => {
                l.cantidad = suma(l.cantidad, cantidad);
                l.coste += coste;
            }
            _ => self.lotes.push_back(Lote {
                fecha,
                cantidad,
                coste,
            }),
        }
    }

    /// Da de baja `cantidad`: (coste, fecha del lote más antiguo, cantidad que faltaba).
    fn salida(&mut self, cantidad: Qty) -> (Money, Option<Date>, Qty) {
        let mut pendiente = cantidad;
        let mut coste = Money::ZERO;
        let mut desde = None;
        while !pendiente.is_zero() {
            let Some(l) = self.lotes.front_mut() else {
                break;
            };
            desde = Some(desde.map_or(l.fecha, |d: Date| d.min(l.fecha)));
            if l.cantidad <= pendiente {
                pendiente = resta(pendiente, l.cantidad);
                coste += l.coste;
                self.lotes.pop_front();
            } else {
                let parte = pendiente
                    .parte_de(l.coste, l.cantidad)
                    .unwrap_or(Money::ZERO);
                l.cantidad = resta(l.cantidad, pendiente);
                l.coste -= parte;
                coste += parte;
                pendiente = Qty::ZERO;
            }
        }
        (coste, desde, pendiente)
    }

    /// Pone el valor en libros de todos los lotes a `total`, repartido por títulos.
    fn valorar(&mut self, total: Money) {
        let cantidad = self.cantidad();
        if cantidad.is_zero() || cantidad.is_negative() {
            return;
        }
        let n = self.lotes.len();
        let mut repartido = Money::ZERO;
        for (i, l) in self.lotes.iter_mut().enumerate() {
            l.coste = if i + 1 == n {
                total - repartido
            } else {
                l.cantidad.parte_de(total, cantidad).unwrap_or(Money::ZERO)
            };
            repartido += l.coste;
        }
    }

    /// Títulos que se añaden (o quitan) sin coste, repartidos entre los lotes en proporción. El
    /// coste total no cambia: se reparte entre los títulos que quedan. Un desdoblamiento o una
    /// agrupación no es una alteración en la composición del patrimonio (art. 33.1 LIRPF, según
    /// la doctrina de la DGT) ni una transmisión: el precio de adquisición se reparte entre los
    /// títulos que resultan (NRV 9.ª PGC). TODO(VERIFY): citar la consulta vinculante concreta
    /// de la DGT.
    ///
    /// Devuelve los títulos que faltaban si salen más de los que hay (falta un extracto o una
    /// compra anterior); entonces se reparte como siempre y el llamador avisa.
    fn split(&mut self, extra: Qty) -> Option<Qty> {
        let mut extra = extra;
        let mut total = self.cantidad();
        // Entran los títulos nuevos de una agrupación cuyos viejos ya salieron: vuelven los
        // lotes con su coste y se dejan en los títulos nuevos.
        if !extra.is_negative() && total.is_zero() && !self.en_suspenso.is_empty() {
            self.lotes = std::mem::take(&mut self.en_suspenso);
            total = self.cantidad();
            extra = resta(extra, total);
        }
        if total.is_zero() || total.is_negative() {
            return None;
        }
        // Salen justo todos los títulos (primera mitad de una agrupación): el coste espera a los
        // nuevos en vez de perderse. Sólo si quedan exactamente cero: si salen más de los que
        // hay no es una agrupación sino que falta algo, y se sigue como antes.
        let queda = suma(total, extra);
        if queda.is_zero() {
            self.en_suspenso = std::mem::take(&mut self.lotes);
            return None;
        }
        let faltan = queda.is_negative().then_some(queda.abs());
        let n = self.lotes.len();
        let mut repartido = Qty::ZERO;
        for (i, l) in self.lotes.iter_mut().enumerate() {
            let parte = if i + 1 == n {
                resta(extra, repartido)
            } else {
                // extra × lote / total, con la escala de la cantidad.
                Qty::from_raw(
                    extra
                        .raw()
                        .checked_mul(l.cantidad.raw() / 1_000_000)
                        .map_or(0, |p| p / (total.raw() / 1_000_000).max(1)),
                )
            };
            l.cantidad = suma(l.cantidad, parte);
            repartido = suma(repartido, parte);
        }
        self.lotes.retain(|l| !l.cantidad.is_zero());
        faltan
    }
}

fn orden(movs: &[Mov]) -> Vec<&Mov> {
    let mut v: Vec<&Mov> = movs.iter().collect();
    v.sort_by(|a, b| {
        a.fecha
            .cmp(&b.fecha)
            .then_with(|| a.hora.cmp(&b.hora))
            .then_with(|| a.id.cmp(&b.id))
    });
    v
}

/// Calcula con los movimientos hasta `hasta` (incluido; `None` = todos). `comision_al_coste`:
/// las comisiones son más coste y menos valor de transmisión (si no, van a resultados).
/// `nombre` da el nombre de un valor para los avisos.
pub fn calcular(
    movs: &[Mov],
    metodo: Metodo,
    comision_al_coste: bool,
    hasta: Option<Date>,
    nombre: &dyn Fn(i64) -> String,
) -> Calculo {
    let mut carteras: BTreeMap<i64, Cartera> = BTreeMap::new();
    let mut out = Calculo::default();
    for m in orden(movs) {
        if hasta.is_some_and(|h| m.fecha > h) {
            break;
        }
        let c = carteras.entry(m.valor).or_default();
        match m.tipo {
            Movimiento::Compra => {
                let coste = if comision_al_coste {
                    m.euros + m.comision
                } else {
                    if !m.comision.is_zero() {
                        out.gastos.push(Gasto {
                            op: m.id,
                            fecha: m.fecha,
                            importe: m.comision,
                        });
                    }
                    m.euros
                };
                c.entrada(metodo, m.fecha, m.cantidad, coste);
                let q = c.por_cuenta.entry(m.cuenta).or_default();
                *q = suma(*q, m.cantidad);
            }
            Movimiento::Venta => {
                let en_cuenta = c.por_cuenta.get(&m.cuenta).copied().unwrap_or_default();
                if en_cuenta < m.cantidad {
                    out.avisos.push(format!(
                        "Venta del {} de {} títulos de {}: en esa cuenta sólo había {} antes. \
                         ¿Falta un extracto o una compra anterior?",
                        m.fecha,
                        m.cantidad,
                        nombre(m.valor),
                        en_cuenta
                    ));
                }
                let (coste, desde, faltan) = c.salida(m.cantidad);
                if !faltan.is_zero() {
                    out.avisos.push(format!(
                        "Venta del {} de {}: faltan {} títulos comprados antes; su coste se \
                         toma como cero hasta que importes las compras.",
                        m.fecha,
                        nombre(m.valor),
                        faltan
                    ));
                }
                let q = c.por_cuenta.entry(m.cuenta).or_default();
                *q = resta(*q, m.cantidad);
                let transmision = if comision_al_coste {
                    m.euros - m.comision
                } else {
                    if !m.comision.is_zero() {
                        out.gastos.push(Gasto {
                            op: m.id,
                            fecha: m.fecha,
                            importe: m.comision,
                        });
                    }
                    m.euros
                };
                out.realizados.push(Realizado {
                    op: m.id,
                    fecha: m.fecha,
                    valor: m.valor,
                    cuenta: m.cuenta,
                    cantidad: m.cantidad,
                    transmision,
                    coste,
                    resultado: transmision - coste,
                    desde,
                });
            }
            Movimiento::Traspaso => {
                let Some(destino) = m.destino.filter(|d| *d != m.cuenta) else {
                    continue;
                };
                let origen = c.por_cuenta.entry(m.cuenta).or_default();
                if *origen < m.cantidad {
                    out.avisos.push(format!(
                        "Traspaso del {} de {} títulos de {}: en la cuenta de origen sólo había {}.",
                        m.fecha,
                        m.cantidad,
                        nombre(m.valor),
                        *origen
                    ));
                }
                *origen = resta(*origen, m.cantidad);
                let d = c.por_cuenta.entry(destino).or_default();
                *d = suma(*d, m.cantidad);
            }
            Movimiento::Valoracion => c.valorar(m.euros),
            Movimiento::Split => {
                if let Some(faltan) = c.split(m.cantidad) {
                    out.avisos.push(format!(
                        "Desdoblamiento o agrupación del {} de {}: salen {} títulos y faltan {} \
                         comprados antes. ¿Falta un extracto o una compra anterior?",
                        m.fecha,
                        nombre(m.valor),
                        m.cantidad.abs(),
                        faltan
                    ));
                }
                let q = c.por_cuenta.entry(m.cuenta).or_default();
                *q = suma(*q, m.cantidad);
            }
        }
    }
    for (valor, c) in &carteras {
        if !c.en_suspenso.is_empty() {
            out.avisos.push(format!(
                "Agrupación de {}: salieron todos los títulos y no han entrado los nuevos; su \
                 coste ({}) espera a que se anoten.",
                nombre(*valor),
                c.en_suspenso.iter().map(|l| l.coste).sum::<Money>()
            ));
        }
        let total = c.cantidad();
        let coste = c.coste();
        for (cuenta, q) in &c.por_cuenta {
            if q.is_zero() {
                continue;
            }
            let parte = if total.is_zero() {
                Money::ZERO
            } else {
                q.parte_de(coste, total).unwrap_or(Money::ZERO)
            };
            out.posiciones.push(Posicion {
                valor: *valor,
                cuenta: *cuenta,
                cantidad: *q,
                coste: parte,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use time::macros::date;

    fn mov(id: i64, fecha: Date, tipo: Movimiento, q: &str, euros: i64, comision: i64) -> Mov {
        Mov {
            id,
            fecha,
            hora: "10:00:00".into(),
            tipo,
            valor: 1,
            cuenta: 1,
            destino: None,
            cantidad: Qty::parse(q).unwrap(),
            euros: Money::from_cents(euros),
            comision: Money::from_cents(comision),
        }
    }

    fn sin_nombre(_: i64) -> String {
        "ACME".into()
    }

    #[test]
    fn coste_medio_con_comisiones_al_coste() {
        // 10 a 100 € + 1 € y 10 a 120 € + 1 €: coste medio (1.001 + 1.201) / 20 = 110,10 €.
        // Se venden 5 a 130 € con 1 € de comisión: 649 − 550,50 = 98,50 €.
        let movs = [
            mov(
                1,
                date!(2026 - 01 - 10),
                Movimiento::Compra,
                "10",
                100_000,
                100,
            ),
            mov(
                2,
                date!(2026 - 02 - 10),
                Movimiento::Compra,
                "10",
                120_000,
                100,
            ),
            mov(
                3,
                date!(2026 - 03 - 10),
                Movimiento::Venta,
                "5",
                65_000,
                100,
            ),
        ];
        let c = calcular(&movs, Metodo::Pmp, true, None, &sin_nombre);
        assert!(c.avisos.is_empty(), "{:?}", c.avisos);
        let r = &c.realizados[0];
        assert_eq!(r.transmision, Money::from_cents(64_900));
        assert_eq!(r.coste, Money::from_cents(55_050));
        assert_eq!(r.resultado, Money::from_cents(9_850));
        assert_eq!(c.posiciones[0].cantidad, Qty::parse("15").unwrap());
        assert_eq!(c.posiciones[0].coste, Money::from_cents(165_150));
    }

    #[test]
    fn fifo_y_comisiones_a_resultados() {
        let movs = [
            mov(
                1,
                date!(2026 - 01 - 10),
                Movimiento::Compra,
                "10",
                100_000,
                100,
            ),
            mov(
                2,
                date!(2026 - 02 - 10),
                Movimiento::Compra,
                "10",
                120_000,
                100,
            ),
            mov(
                3,
                date!(2026 - 03 - 10),
                Movimiento::Venta,
                "15",
                195_000,
                100,
            ),
        ];
        // FIFO con comisiones al coste: 10 del primer lote (1.001) + 5 del segundo (600,50).
        let c = calcular(&movs, Metodo::Fifo, true, None, &sin_nombre);
        assert_eq!(c.realizados[0].coste, Money::from_cents(160_150));
        assert_eq!(c.realizados[0].desde, Some(date!(2026 - 01 - 10)));
        // Mantenidos para negociar: las tres comisiones van a resultados.
        let c = calcular(&movs, Metodo::Pmp, false, None, &sin_nombre);
        assert_eq!(c.gastos.len(), 3);
        assert_eq!(c.realizados[0].transmision, Money::from_cents(195_000));
        assert_eq!(c.realizados[0].coste, Money::from_cents(165_000));
    }

    #[test]
    fn split_traspaso_y_venta_sin_compras() {
        let mut t = mov(3, date!(2026 - 03 - 01), Movimiento::Traspaso, "4", 0, 0);
        t.destino = Some(2);
        let mut v = mov(5, date!(2026 - 05 - 01), Movimiento::Venta, "2", 30_000, 0);
        v.cuenta = 2;
        let movs = [
            mov(
                1,
                date!(2026 - 01 - 10),
                Movimiento::Compra,
                "10",
                100_000,
                0,
            ),
            mov(2, date!(2026 - 02 - 10), Movimiento::Split, "10", 0, 0),
            t,
            v,
            mov(
                6,
                date!(2026 - 06 - 01),
                Movimiento::Venta,
                "20",
                100_000,
                0,
            ),
        ];
        let c = calcular(&movs, Metodo::Fifo, true, None, &sin_nombre);
        // Tras el split hay 20 títulos a 50 €: la venta de 2 cuesta 100 €.
        assert_eq!(c.realizados[0].coste, Money::from_cents(10_000));
        // La última venta pide 20 de la cuenta 1, que sólo tiene 16 (y quedan 18 en total).
        assert_eq!(c.avisos.len(), 2, "{:?}", c.avisos);
        assert!(c.avisos[1].contains("faltan 2"), "{:?}", c.avisos);
    }

    /// Agrupación 1 por 10 como la da IBKR (salen los 105 títulos viejos y entran 10,5) y las
    /// 0,5 de fracción pagadas en efectivo: el coste se conserva en los títulos nuevos (y en
    /// FIFO, con la fecha de cada compra) y la fracción es una transmisión parcial.
    #[test]
    fn agrupacion_uno_por_diez_conserva_el_coste() {
        let compras = [
            mov(
                1,
                date!(2026 - 01 - 10),
                Movimiento::Compra,
                "50",
                50_000,
                0,
            ),
            mov(
                2,
                date!(2026 - 02 - 10),
                Movimiento::Compra,
                "55",
                55_000,
                0,
            ),
        ];
        let fraccion = mov(5, date!(2026 - 03 - 01), Movimiento::Venta, "0.5", 6_000, 0);
        for orden in [["-105", "10.5"], ["10.5", "-105"]] {
            let mut movs = compras.to_vec();
            movs.push(mov(
                3,
                date!(2026 - 03 - 01),
                Movimiento::Split,
                orden[0],
                0,
                0,
            ));
            movs.push(mov(
                4,
                date!(2026 - 03 - 01),
                Movimiento::Split,
                orden[1],
                0,
                0,
            ));
            movs.push(fraccion.clone());
            let c = calcular(&movs, Metodo::Fifo, true, None, &sin_nombre);
            assert!(c.avisos.is_empty(), "{:?}", c.avisos);
            // La fracción sale del primer lote (5 títulos por 500 €): 50 € de coste.
            let r = &c.realizados[0];
            assert_eq!(
                (r.cantidad, r.coste, r.resultado, r.desde),
                (
                    Qty::parse("0.5").unwrap(),
                    Money::from_cents(5_000),
                    Money::from_cents(1_000),
                    Some(date!(2026 - 01 - 10))
                )
            );
            assert_eq!(c.posiciones.len(), 1);
            assert_eq!(
                (c.posiciones[0].cantidad, c.posiciones[0].coste),
                (Qty::parse("10").unwrap(), Money::from_cents(100_000))
            );
            let c = calcular(&movs, Metodo::Pmp, true, None, &sin_nombre);
            assert_eq!(c.realizados[0].coste, Money::from_cents(5_000));
            assert_eq!(c.posiciones[0].coste, Money::from_cents(100_000));
        }
    }

    /// Un split que saca más títulos de los que hay no es una agrupación (falta un extracto o
    /// una compra): no manda los lotes a suspenso, avisa de los que faltan y una compra
    /// posterior no deja el coste anterior colgado en un aviso.
    #[test]
    fn split_con_mas_salida_que_titulos_avisa_y_no_suspende() {
        let movs = [
            mov(
                1,
                date!(2026 - 01 - 10),
                Movimiento::Compra,
                "10",
                100_000,
                0,
            ),
            mov(2, date!(2026 - 03 - 01), Movimiento::Split, "-15", 0, 0),
            mov(
                3,
                date!(2026 - 04 - 01),
                Movimiento::Compra,
                "10",
                200_000,
                0,
            ),
        ];
        let c = calcular(&movs, Metodo::Fifo, true, None, &sin_nombre);
        assert!(
            c.avisos.iter().any(|a| a.contains("faltan 5")),
            "{:?}",
            c.avisos
        );
        assert!(
            !c.avisos.iter().any(|a| a.contains("Agrupación")),
            "{:?}",
            c.avisos
        );
        // El coste de la primera compra sigue en la cartera, no esperando en suspenso.
        assert_eq!(c.posiciones[0].coste, Money::from_cents(300_000));
    }

    #[test]
    fn posiciones_por_cuenta_con_su_parte_del_coste() {
        let mut b = mov(
            2,
            date!(2026 - 02 - 10),
            Movimiento::Compra,
            "30",
            330_000,
            0,
        );
        b.cuenta = 2;
        let movs = [
            mov(
                1,
                date!(2026 - 01 - 10),
                Movimiento::Compra,
                "10",
                100_000,
                0,
            ),
            b,
        ];
        let c = calcular(
            &movs,
            Metodo::Pmp,
            true,
            Some(date!(2026 - 12 - 31)),
            &sin_nombre,
        );
        // Coste medio 107,50 €: 1.075 € en la cuenta 1 y 3.225 € en la 2.
        assert_eq!(c.posiciones.len(), 2);
        assert_eq!(c.posiciones[0].coste, Money::from_cents(107_500));
        assert_eq!(c.posiciones[1].coste, Money::from_cents(322_500));
    }
}
