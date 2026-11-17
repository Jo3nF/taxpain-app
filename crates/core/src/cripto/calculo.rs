//! Motor de cálculo de criptomonedas: lotes por activo y plataforma, coste de adquisición por
//! precio medio ponderado (PMP) o FIFO, resultado realizado de cada transmisión, posiciones a
//! una fecha y valoración con un precio de cierre. Puro: no sabe de base de datos ni de asientos.
//!
//! Reglas (en llano, con su norma):
//! - **Compra con euros**: lote nuevo con coste = euros pagados + comisión (NRV 10.ª PGC, precio
//!   de adquisición; art. 35.1.b LIRPF, gastos inherentes a la adquisición).
//! - **Venta a euros, pago de un gasto en cripto, comisión pagada en cripto**: transmisión. Valor
//!   de transmisión = euros − comisión (art. 35.2 LIRPF: se deducen los gastos inherentes);
//!   coste según el método; resultado = valor − coste.
//! - **Permuta cripto-cripto**: transmisión del entregado por su valor de mercado (art. 37.1.h
//!   LIRPF: el mayor entre el valor de mercado de lo entregado y lo recibido; NRV 2.ª PGC para la
//!   sociedad) y adquisición del recibido por ese mismo valor (DGT V0999-18).
//! - **Traspaso entre plataformas propias**: no realiza nada; los lotes se mueven con su coste
//!   (y con su fecha, en FIFO). La comisión en euros es gasto.
//! - **Recompensa (staking, airdrop, intereses)**: ingreso por su valor de mercado y lote con ese
//!   coste (art. 37.1.l LIRPF por analogía: incorporación de bienes sin transmisión).
//! - **Pérdida o robo**: baja del lote por su coste con valor de transmisión cero.
//! - **Cobro de una factura en cripto**: adquisición por el importe cobrado (+ comisión).
//!
//! **Método**: una sociedad elige PMP o FIFO para las existencias (NRV 10.ª.1.3 PGC: «precio
//! medio o coste medio ponderado… El método FIFO es aceptable»); en el IRPF de la persona física
//! las transmitidas son las adquiridas en primer lugar (art. 37.2 LIRPF por analogía, DGT
//! V0999-18, V0975-22), es decir, FIFO. Por eso se calculan siempre los dos.
//!
//! Los lotes van por activo **y plataforma**: así se sabe qué hay en cada sitio para el 721 y
//! un traspaso no mezcla costes. El cálculo «Renta» usa FIFO **global** por activo: la DGT
//! (V2520-22) habla de «ventas parciales de criptomonedas de un mismo tipo … adquiridas en
//! diferentes momentos» sin distinguir plataformas, y la ayuda de Renta Web 2025 (apartado
//! 7.6.6.2) dice que las transmitidas son las adquiridas en primer lugar aunque las operaciones
//! se hayan hecho en distintos exchanges (comprobado el 2026-10-27).
//!
//! **Punto de partida** ([`Inicio`]): cuando faltan los registros de años antiguos (exchanges
//! que cerraron, DeFi), el cálculo empieza con las posiciones y el coste pendiente a una fecha
//! de corte (los de la contabilidad ya cerrada y declarada). Las operaciones hasta ese día no
//! cuentan (son historial) y antes de esa fecha no se calcula nada. Cada posición de partida es
//! un solo lote con fecha de corte (en FIFO no se conocen los lotes de antes). Las unidades de
//! partida sin plataforma (`plataforma` 0) sirven para las salidas de cualquier plataforma a la
//! que le falten unidades.

use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};
use time::Date;

use super::qty::{Precio, Qty};
use crate::money::Money;

/// Tipo de operación.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tipo {
    /// Compra con euros.
    Compra,
    /// Venta a euros.
    Venta,
    /// Permuta cripto-cripto (se entrega `ticker`, se recibe `ticker2`).
    Permuta,
    /// Traspaso entre plataformas propias (`plataforma` → `destino`).
    Traspaso,
    /// Comisión pagada en cripto (transmisión por su valor + gasto).
    Comision,
    /// Staking, airdrop, intereses, recompensa: ingreso a valor de mercado.
    Recompensa,
    /// Pérdida, robo, quiebra de la plataforma: baja sin contraprestación.
    Perdida,
    /// Pago de un gasto o factura con cripto (transmisión).
    Pago,
    /// Cobro de una factura en cripto (adquisición).
    Cobro,
}

impl Tipo {
    pub const TODOS: [Tipo; 9] = [
        Tipo::Compra,
        Tipo::Venta,
        Tipo::Permuta,
        Tipo::Traspaso,
        Tipo::Comision,
        Tipo::Recompensa,
        Tipo::Perdida,
        Tipo::Pago,
        Tipo::Cobro,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Tipo::Compra => "compra",
            Tipo::Venta => "venta",
            Tipo::Permuta => "permuta",
            Tipo::Traspaso => "traspaso",
            Tipo::Comision => "comision",
            Tipo::Recompensa => "recompensa",
            Tipo::Perdida => "perdida",
            Tipo::Pago => "pago",
            Tipo::Cobro => "cobro",
        }
    }

    pub fn parse(s: &str) -> Option<Tipo> {
        Tipo::TODOS.into_iter().find(|t| t.code() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            Tipo::Compra => "Compra",
            Tipo::Venta => "Venta",
            Tipo::Permuta => "Permuta",
            Tipo::Traspaso => "Traspaso",
            Tipo::Comision => "Comisión en cripto",
            Tipo::Recompensa => "Recompensa",
            Tipo::Perdida => "Pérdida o robo",
            Tipo::Pago => "Pago en cripto",
            Tipo::Cobro => "Cobro en cripto",
        }
    }

    /// Transmite (da de baja) `cantidad` de `ticker`.
    pub fn transmite(self) -> bool {
        matches!(
            self,
            Tipo::Venta | Tipo::Permuta | Tipo::Comision | Tipo::Perdida | Tipo::Pago
        )
    }
}

/// Método de valoración de las salidas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Metodo {
    /// Precio medio ponderado (NRV 10.ª.1.3 PGC).
    #[default]
    Pmp,
    /// Primera entrada, primera salida (NRV 10.ª.1.3 PGC; art. 37.2 LIRPF).
    Fifo,
}

impl Metodo {
    pub fn label(self) -> &'static str {
        match self {
            Metodo::Pmp => "precio medio ponderado",
            Metodo::Fifo => "FIFO",
        }
    }
}

/// Una operación ya normalizada (la app la construye desde la base de datos).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operacion {
    pub id: i64,
    pub fecha: Date,
    /// `HH:MM:SS` (ordena las operaciones del mismo día).
    pub hora: String,
    pub tipo: Tipo,
    pub plataforma: i64,
    /// Plataforma de destino de un traspaso.
    pub destino: Option<i64>,
    pub ticker: String,
    pub cantidad: Qty,
    /// Activo recibido en una permuta.
    pub ticker2: Option<String>,
    pub cantidad2: Option<Qty>,
    /// Contravalor en euros (sin la comisión): lo pagado, lo cobrado o el valor de mercado.
    pub euros: Money,
    /// Comisión en euros.
    pub comision: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lote {
    pub fecha: Date,
    pub cantidad: Qty,
    pub coste: Money,
}

/// Lo que queda de un activo en una plataforma.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Posicion {
    pub ticker: String,
    pub plataforma: i64,
    pub cantidad: Qty,
    pub coste: Money,
    pub lotes: Vec<Lote>,
}

/// Resultado realizado por una transmisión.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Realizado {
    pub op: i64,
    pub fecha: Date,
    pub tipo: Tipo,
    pub ticker: String,
    pub plataforma: i64,
    pub cantidad: Qty,
    /// Valor de transmisión (ya sin la comisión).
    pub valor: Money,
    pub coste: Money,
    /// `valor − coste`.
    pub resultado: Money,
}

/// Ingreso por recompensa (staking, airdrop…).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ingreso {
    pub op: i64,
    pub fecha: Date,
    pub ticker: String,
    pub cantidad: Qty,
    pub importe: Money,
}

/// Gasto por comisión (traspasos y comisiones en cripto).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gasto {
    pub op: i64,
    pub fecha: Date,
    pub importe: Money,
}

/// Salida de más unidades de las que había: falta una compra, un traspaso o el punto de
/// partida.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hueco {
    pub op: i64,
    pub fecha: Date,
    pub ticker: String,
    pub plataforma: i64,
    /// Unidades que faltaban.
    pub falta: Qty,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Calculo {
    pub metodo: Metodo,
    pub posiciones: Vec<Posicion>,
    pub realizados: Vec<Realizado>,
    pub recompensas: Vec<Ingreso>,
    pub gastos: Vec<Gasto>,
    pub avisos: Vec<String>,
    #[serde(default)]
    pub huecos: Vec<Hueco>,
}

/// Plataforma de las posiciones de partida que no tienen una.
pub const SIN_PLATAFORMA: i64 = 0;

/// Una posición del punto de partida: unidades y coste pendiente a la fecha de corte.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inicial {
    pub ticker: String,
    /// [`SIN_PLATAFORMA`] si no se sabe dónde están.
    pub plataforma: i64,
    pub cantidad: Qty,
    pub coste: Money,
}

/// Punto de partida: posiciones y coste pendiente a la fecha de corte.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inicio {
    /// Último día que no cuenta: el cálculo sigue desde el día siguiente.
    pub fecha: Date,
    pub posiciones: Vec<Inicial>,
}

/// Resumen de un ejercicio.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Resumen {
    pub metodo: Metodo,
    pub year: i32,
    pub ganancias: Money,
    pub perdidas: Money,
    /// Ganancias − pérdidas.
    pub neto: Money,
    pub recompensas: Money,
    pub comisiones: Money,
    pub transmisiones: usize,
}

#[derive(Default)]
struct Cartera {
    lotes: VecDeque<Lote>,
}

impl Cartera {
    fn cantidad(&self) -> Qty {
        self.lotes
            .iter()
            .fold(Qty::ZERO, |a, l| a.checked_add(l.cantidad).unwrap_or(a))
    }

    fn coste(&self) -> Money {
        self.lotes.iter().map(|l| l.coste).sum()
    }

    fn entrada(&mut self, metodo: Metodo, fecha: Date, cantidad: Qty, coste: Money) {
        match metodo {
            Metodo::Fifo => self.lotes.push_back(Lote {
                fecha,
                cantidad,
                coste,
            }),
            Metodo::Pmp => {
                // Un solo lote que acumula cantidad y coste (la fecha es la de la primera
                // entrada).
                if let Some(l) = self.lotes.front_mut() {
                    l.cantidad = l.cantidad.checked_add(cantidad).unwrap_or(l.cantidad);
                    l.coste += coste;
                } else {
                    self.lotes.push_back(Lote {
                        fecha,
                        cantidad,
                        coste,
                    });
                }
            }
        }
    }

    /// Da de baja `cantidad` y devuelve (coste de lo salido, lotes salidos, cantidad que
    /// faltaba).
    fn salida(&mut self, cantidad: Qty) -> (Money, Vec<Lote>, Qty) {
        let mut pendiente = cantidad;
        let mut coste = Money::ZERO;
        let mut salidos = Vec::new();
        while !pendiente.is_zero() {
            let Some(l) = self.lotes.front_mut() else {
                break;
            };
            if l.cantidad <= pendiente {
                let l = self.lotes.pop_front().unwrap_or(Lote {
                    fecha: Date::MIN,
                    cantidad: Qty::ZERO,
                    coste: Money::ZERO,
                });
                pendiente = pendiente.checked_sub(l.cantidad).unwrap_or(Qty::ZERO);
                coste += l.coste;
                salidos.push(l);
            } else {
                let parte = pendiente
                    .parte_de(l.coste, l.cantidad)
                    .unwrap_or(Money::ZERO);
                l.cantidad = l.cantidad.checked_sub(pendiente).unwrap_or(Qty::ZERO);
                l.coste -= parte;
                salidos.push(Lote {
                    fecha: l.fecha,
                    cantidad: pendiente,
                    coste: parte,
                });
                coste += parte;
                pendiente = Qty::ZERO;
            }
        }
        (coste, salidos, pendiente)
    }
}

fn orden(ops: &[Operacion]) -> Vec<&Operacion> {
    let mut v: Vec<&Operacion> = ops.iter().collect();
    v.sort_by(|a, b| {
        a.fecha
            .cmp(&b.fecha)
            .then_with(|| a.hora.cmp(&b.hora))
            .then_with(|| a.id.cmp(&b.id))
    });
    v
}

type Clave = (String, i64);

/// Lo que se va calculando: las carteras por activo y plataforma (o sólo por activo) y, con
/// precio medio por plataforma, el coste medio de cada activo con todas sus unidades juntas.
struct Motor {
    metodo: Metodo,
    carteras: BTreeMap<Clave, Cartera>,
    /// Precio medio con plataformas: unidades y coste de cada activo en todas ellas. Las
    /// carteras sólo llevan las unidades de cada sitio; el coste de una salida sale de aquí.
    medias: Option<BTreeMap<String, (Qty, Money)>>,
}

/// Una transmisión: coste, unidades que faltaban en la plataforma y unidades que faltaban en
/// total (las que salen con coste cero).
struct Salida {
    coste: Money,
    falta: Qty,
    falta_total: Qty,
}

impl Motor {
    fn entrada(&mut self, k: Clave, fecha: Date, cantidad: Qty, coste: Money) {
        if let Some(m) = self.medias.as_mut() {
            let e = m.entry(k.0.clone()).or_insert((Qty::ZERO, Money::ZERO));
            e.0 = e.0.checked_add(cantidad).unwrap_or(e.0);
            e.1 += coste;
        }
        self.carteras
            .entry(k)
            .or_default()
            .entrada(self.metodo, fecha, cantidad, coste);
    }

    /// Unidades que salen de la cartera `k`; lo que falte sale de las unidades de partida sin
    /// plataforma de ese activo, si las hay.
    fn unidades(&mut self, k: &Clave, cantidad: Qty) -> (Money, Vec<Lote>, Qty) {
        let (mut coste, mut salidos, mut faltan) =
            self.carteras.entry(k.clone()).or_default().salida(cantidad);
        if !faltan.is_zero()
            && k.1 != SIN_PLATAFORMA
            && let Some(c) = self.carteras.get_mut(&(k.0.clone(), SIN_PLATAFORMA))
        {
            let (c2, l2, f2) = c.salida(faltan);
            coste += c2;
            salidos.extend(l2);
            faltan = f2;
        }
        (coste, salidos, faltan)
    }

    /// Transmisión de `cantidad` de la cartera `k`.
    fn salida(&mut self, k: &Clave, cantidad: Qty) -> Salida {
        let (coste, _, falta) = self.unidades(k, cantidad);
        let Some(m) = self.medias.as_mut() else {
            return Salida {
                coste,
                falta,
                falta_total: falta,
            };
        };
        // Precio medio de todas las unidades del activo, estén donde estén.
        let e = m.entry(k.0.clone()).or_insert((Qty::ZERO, Money::ZERO));
        let sale = cantidad.min(e.0);
        let coste = if sale == e.0 {
            e.1
        } else {
            sale.parte_de(e.1, e.0).unwrap_or(Money::ZERO)
        };
        e.0 = e.0.checked_sub(sale).unwrap_or(Qty::ZERO);
        e.1 -= coste;
        Salida {
            coste,
            falta,
            falta_total: cantidad.checked_sub(sale).unwrap_or(Qty::ZERO),
        }
    }

    /// Traspaso de `cantidad` de `k` a `destino`: los lotes se mueven con su coste y su fecha.
    fn traspaso(&mut self, k: &Clave, destino: Clave, cantidad: Qty) -> Qty {
        let (_, salidos, falta) = self.unidades(k, cantidad);
        let metodo = self.metodo;
        let dest = self.carteras.entry(destino).or_default();
        for l in salidos {
            dest.entrada(metodo, l.fecha, l.cantidad, l.coste);
        }
        falta
    }

    fn posiciones(self) -> Vec<Posicion> {
        let mut out: Vec<Posicion> = self
            .carteras
            .into_iter()
            .filter(|(_, c)| !c.lotes.is_empty() && !c.cantidad().is_zero())
            .map(|((ticker, plataforma), c)| Posicion {
                ticker,
                plataforma,
                cantidad: c.cantidad(),
                coste: c.coste(),
                lotes: c.lotes.into_iter().collect(),
            })
            .collect();
        let Some(medias) = self.medias else {
            return out;
        };
        // Con el precio medio de todo el activo, el coste de cada plataforma es su parte de las
        // unidades que figuran en las plataformas; la última se lleva el redondeo. Si una venta
        // de una plataforma sin unidades (falta un traspaso) dejó en otra más de las que quedan,
        // el coste que queda se reparte entre las que figuran: nunca suma más de lo que hay.
        for (ticker, (total, coste)) in medias {
            let idx: Vec<usize> = (0..out.len())
                .filter(|&i| out[i].ticker == ticker)
                .collect();
            let suma = idx.iter().fold(Qty::ZERO, |a, &i| {
                a.checked_add(out[i].cantidad).unwrap_or(a)
            });
            let mut repartido = Money::ZERO;
            for (n, &i) in idx.iter().enumerate() {
                let p = &mut out[i];
                p.coste = if n + 1 == idx.len() && suma >= total {
                    coste - repartido
                } else {
                    p.cantidad
                        .parte_de(coste, suma.max(total))
                        .unwrap_or(Money::ZERO)
                };
                repartido += p.coste;
                let fecha = p.lotes.first().map_or(Date::MIN, |l| l.fecha);
                p.lotes = vec![Lote {
                    fecha,
                    cantidad: p.cantidad,
                    coste: p.coste,
                }];
            }
        }
        out
    }
}

/// Calcula con las operaciones hasta `hasta` (incluida; `None` = todas). `por_plataforma = false`
/// junta los lotes de cada activo sin distinguir plataforma (FIFO global de la Renta).
///
/// Con precio medio y plataformas, las unidades se siguen por plataforma (para el 721 y los
/// traspasos) pero el coste medio es el de todas las unidades del activo: son bienes
/// intercambiables (NRV 10.ª.1.3 PGC: «bienes concretos que forman parte de un inventario de
/// bienes intercambiables entre sí») y una compra en cualquier sitio cambia el medio de todas.
pub fn calcular(
    ops: &[Operacion],
    metodo: Metodo,
    hasta: Option<Date>,
    por_plataforma: bool,
) -> Calculo {
    calcular_desde(ops, metodo, hasta, por_plataforma, None)
}

/// Como [`calcular`], empezando por el punto de partida si lo hay: sus posiciones entran a la
/// fecha de corte con su coste y las operaciones hasta ese día no cuentan. Antes de la fecha de
/// corte (`hasta` anterior) no se calcula nada: sin posiciones ni resultados.
pub fn calcular_desde(
    ops: &[Operacion],
    metodo: Metodo,
    hasta: Option<Date>,
    por_plataforma: bool,
    inicio: Option<&Inicio>,
) -> Calculo {
    let mut motor = Motor {
        metodo,
        carteras: BTreeMap::new(),
        medias: (metodo == Metodo::Pmp && por_plataforma).then(BTreeMap::new),
    };
    let mut realizados = Vec::new();
    let mut recompensas = Vec::new();
    let mut gastos = Vec::new();
    let mut avisos = Vec::new();
    let mut huecos = Vec::new();
    let clave = |ticker: &str, plataforma: i64| {
        (
            ticker.to_owned(),
            if por_plataforma {
                plataforma
            } else {
                SIN_PLATAFORMA
            },
        )
    };
    if let Some(i) = inicio {
        if hasta.is_some_and(|h| h < i.fecha) {
            return Calculo {
                metodo,
                posiciones: Vec::new(),
                realizados,
                recompensas,
                gastos,
                avisos,
                huecos,
            };
        }
        for p in i.posiciones.iter().filter(|p| !p.cantidad.is_zero()) {
            motor.entrada(clave(&p.ticker, p.plataforma), i.fecha, p.cantidad, p.coste);
        }
    }
    for op in orden(ops) {
        if hasta.is_some_and(|h| op.fecha > h) {
            break;
        }
        if inicio.is_some_and(|i| op.fecha <= i.fecha) {
            continue;
        }
        if op.cantidad.is_negative() || op.euros.is_negative() || op.comision.is_negative() {
            avisos.push(format!(
                "Operación {} ({}, {}): cantidad, euros o comisión en negativo; no se tiene en \
                 cuenta.",
                op.id, op.fecha, op.ticker
            ));
            continue;
        }
        let k = clave(&op.ticker, op.plataforma);
        let hueco = |falta: Qty| Hueco {
            op: op.id,
            fecha: op.fecha,
            ticker: op.ticker.clone(),
            plataforma: op.plataforma,
            falta,
        };
        match op.tipo {
            Tipo::Compra | Tipo::Cobro => {
                motor.entrada(k, op.fecha, op.cantidad, op.euros + op.comision);
            }
            Tipo::Recompensa => {
                if op.euros.is_zero() && !op.cantidad.is_zero() {
                    avisos.push(format!(
                        "Recompensa de {} {} del {} sin valor en euros: pon su valor de mercado \
                         ese día (es ingreso y coste de adquisición).",
                        op.cantidad, op.ticker, op.fecha
                    ));
                }
                recompensas.push(Ingreso {
                    op: op.id,
                    fecha: op.fecha,
                    ticker: op.ticker.clone(),
                    cantidad: op.cantidad,
                    importe: op.euros,
                });
                motor.entrada(k, op.fecha, op.cantidad, op.euros);
            }
            Tipo::Traspaso => {
                let Some(destino) = op.destino else {
                    avisos.push(format!(
                        "Traspaso de {} {} del {} sin plataforma de destino: no se mueve.",
                        op.cantidad, op.ticker, op.fecha
                    ));
                    continue;
                };
                if !op.comision.is_zero() {
                    gastos.push(Gasto {
                        op: op.id,
                        fecha: op.fecha,
                        importe: op.comision,
                    });
                }
                if !por_plataforma || destino == op.plataforma {
                    continue;
                }
                let faltan = motor.traspaso(&k, clave(&op.ticker, destino), op.cantidad);
                if !faltan.is_zero() {
                    huecos.push(hueco(faltan));
                    avisos.push(format!(
                        "Traspaso del {}: salen {} {} de la plataforma {} pero sólo había {}.",
                        op.fecha,
                        op.cantidad,
                        op.ticker,
                        op.plataforma,
                        op.cantidad.checked_sub(faltan).unwrap_or(Qty::ZERO)
                    ));
                }
            }
            Tipo::Venta | Tipo::Permuta | Tipo::Comision | Tipo::Perdida | Tipo::Pago => {
                let s = motor.salida(&k, op.cantidad);
                if !s.falta.is_zero() {
                    huecos.push(hueco(s.falta));
                    avisos.push(if s.falta_total.is_zero() {
                        format!(
                            "{} del {}: se transmiten {} {} pero la plataforma {} sólo tenía {} \
                             (¿falta un traspaso?); el coste sale del precio medio de todas.",
                            op.tipo.label(),
                            op.fecha,
                            op.cantidad,
                            op.ticker,
                            op.plataforma,
                            op.cantidad.checked_sub(s.falta).unwrap_or(Qty::ZERO)
                        )
                    } else {
                        format!(
                            "{} del {}: se transmiten {} {} pero la plataforma {} sólo tenía {}; \
                             lo que falta se computa con coste cero (¿falta una compra o un \
                             traspaso?).",
                            op.tipo.label(),
                            op.fecha,
                            op.cantidad,
                            op.ticker,
                            op.plataforma,
                            op.cantidad.checked_sub(s.falta).unwrap_or(Qty::ZERO)
                        )
                    });
                }
                let coste = s.coste;
                let valor = match op.tipo {
                    Tipo::Perdida => Money::ZERO,
                    _ => op.euros - op.comision,
                };
                if matches!(op.tipo, Tipo::Permuta | Tipo::Comision | Tipo::Pago)
                    && op.euros.is_zero()
                {
                    avisos.push(format!(
                        "{} de {} {} del {} sin valor en euros: pon el valor de mercado ese día \
                         (sin él la ganancia sale como pérdida por todo el coste).",
                        op.tipo.label(),
                        op.cantidad,
                        op.ticker,
                        op.fecha
                    ));
                }
                realizados.push(Realizado {
                    op: op.id,
                    fecha: op.fecha,
                    tipo: op.tipo,
                    ticker: op.ticker.clone(),
                    plataforma: op.plataforma,
                    cantidad: op.cantidad,
                    valor,
                    coste,
                    resultado: valor - coste,
                });
                if op.tipo == Tipo::Comision {
                    gastos.push(Gasto {
                        op: op.id,
                        fecha: op.fecha,
                        importe: op.euros,
                    });
                }
                if op.tipo == Tipo::Permuta {
                    match (&op.ticker2, op.cantidad2) {
                        (Some(t2), Some(c2)) if !c2.is_zero() => {
                            motor.entrada(clave(t2, op.plataforma), op.fecha, c2, op.euros);
                        }
                        _ => avisos.push(format!(
                            "Permuta del {}: falta el activo o la cantidad recibida.",
                            op.fecha
                        )),
                    }
                }
            }
        }
    }
    Calculo {
        metodo,
        posiciones: motor.posiciones(),
        realizados,
        recompensas,
        gastos,
        avisos,
        huecos,
    }
}

/// Resumen del año natural `year`.
pub fn resumen(c: &Calculo, year: i32) -> Resumen {
    let (Ok(desde), Ok(hasta)) = (
        Date::from_calendar_date(year, time::Month::January, 1),
        Date::from_calendar_date(year, time::Month::December, 31),
    ) else {
        return Resumen {
            metodo: c.metodo,
            year,
            ..Default::default()
        };
    };
    resumen_entre(c, year, desde, hasta)
}

/// Resumen del ejercicio `year`, que va de `desde` a `hasta` (ambos incluidos): el ejercicio
/// contable, aunque no sea el año natural.
pub fn resumen_entre(c: &Calculo, year: i32, desde: Date, hasta: Date) -> Resumen {
    let en = |f: Date| (desde..=hasta).contains(&f);
    let mut r = Resumen {
        metodo: c.metodo,
        year,
        ..Default::default()
    };
    for x in c.realizados.iter().filter(|x| en(x.fecha)) {
        r.transmisiones += 1;
        if x.resultado.is_negative() {
            r.perdidas += x.resultado.abs();
        } else {
            r.ganancias += x.resultado;
        }
    }
    r.neto = r.ganancias - r.perdidas;
    r.recompensas = c
        .recompensas
        .iter()
        .filter(|x| en(x.fecha))
        .map(|x| x.importe)
        .sum();
    r.comisiones = c
        .gastos
        .iter()
        .filter(|x| en(x.fecha))
        .map(|x| x.importe)
        .sum();
    r
}

/// Valoración de una posición a un precio: (valor de mercado, plusvalía latente = valor − coste).
pub fn valorar(p: &Posicion, precio: Precio) -> Option<(Money, Money)> {
    let valor = p.cantidad.valor(precio)?;
    Some((valor, valor - p.coste))
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    fn m(e: i64) -> Money {
        Money::from_euros(e, 0)
    }

    fn q(s: &str) -> Qty {
        Qty::parse(s).unwrap()
    }

    fn op(id: i64, fecha: Date, tipo: Tipo, ticker: &str, cantidad: &str, euros: i64) -> Operacion {
        Operacion {
            id,
            fecha,
            hora: "10:00:00".into(),
            tipo,
            plataforma: 1,
            destino: None,
            ticker: ticker.into(),
            cantidad: q(cantidad),
            ticker2: None,
            cantidad2: None,
            euros: m(euros),
            comision: Money::ZERO,
        }
    }

    /// Dos compras a precios distintos y una venta parcial: PMP y FIFO dan resultados distintos.
    #[test]
    fn pmp_y_fifo_difieren_en_una_venta_parcial() {
        let ops = vec![
            op(1, date!(2026 - 01 - 10), Tipo::Compra, "BTC", "1", 20_000),
            op(2, date!(2026 - 02 - 10), Tipo::Compra, "BTC", "1", 40_000),
            op(3, date!(2026 - 03 - 10), Tipo::Venta, "BTC", "1", 50_000),
        ];
        let pmp = calcular(&ops, Metodo::Pmp, None, true);
        let fifo = calcular(&ops, Metodo::Fifo, None, true);
        // PMP: coste medio 30.000 → ganancia 20.000. FIFO: sale la de 20.000 → ganancia 30.000.
        assert_eq!(pmp.realizados[0].coste, m(30_000));
        assert_eq!(pmp.realizados[0].resultado, m(20_000));
        assert_eq!(fifo.realizados[0].coste, m(20_000));
        assert_eq!(fifo.realizados[0].resultado, m(30_000));
        // Queda 1 BTC: a 30.000 en PMP y a 40.000 en FIFO.
        assert_eq!(pmp.posiciones[0].coste, m(30_000));
        assert_eq!(fifo.posiciones[0].coste, m(40_000));
        assert_eq!(fifo.posiciones[0].cantidad, q("1"));
        assert_eq!(fifo.posiciones[0].lotes.len(), 1);
        let r = resumen(&fifo, 2026);
        assert_eq!(
            (r.ganancias, r.perdidas, r.neto),
            (m(30_000), m(0), m(30_000))
        );
        assert!(pmp.avisos.is_empty());
    }

    #[test]
    fn la_comision_es_mayor_coste_en_la_compra_y_menor_valor_en_la_venta() {
        let mut compra = op(1, date!(2026 - 01 - 10), Tipo::Compra, "ETH", "2", 4_000);
        compra.comision = m(10);
        let mut venta = op(2, date!(2026 - 05 - 10), Tipo::Venta, "ETH", "1", 3_000);
        venta.comision = m(5);
        let c = calcular(&[compra, venta], Metodo::Pmp, None, true);
        let r = &c.realizados[0];
        assert_eq!(r.coste, m(2_005)); // (4.000 + 10) / 2
        assert_eq!(r.valor, m(2_995));
        assert_eq!(r.resultado, m(990));
        assert_eq!(c.posiciones[0].coste, m(2_005));
    }

    #[test]
    fn la_permuta_transmite_y_adquiere_por_el_valor_de_mercado() {
        let mut permuta = op(
            2,
            date!(2026 - 04 - 01),
            Tipo::Permuta,
            "BTC",
            "0.5",
            30_000,
        );
        permuta.ticker2 = Some("ETH".into());
        permuta.cantidad2 = Some(q("10"));
        let ops = vec![
            op(1, date!(2026 - 01 - 10), Tipo::Compra, "BTC", "1", 40_000),
            permuta,
        ];
        let c = calcular(&ops, Metodo::Fifo, None, true);
        assert_eq!(c.realizados[0].resultado, m(10_000)); // 30.000 − 20.000
        let eth = c.posiciones.iter().find(|p| p.ticker == "ETH").unwrap();
        assert_eq!((eth.cantidad, eth.coste), (q("10"), m(30_000)));
        let btc = c.posiciones.iter().find(|p| p.ticker == "BTC").unwrap();
        assert_eq!((btc.cantidad, btc.coste), (q("0.5"), m(20_000)));
    }

    #[test]
    fn el_traspaso_no_realiza_y_conserva_coste_y_fecha() {
        let mut traspaso = op(2, date!(2026 - 06 - 01), Tipo::Traspaso, "BTC", "1", 0);
        traspaso.destino = Some(2);
        traspaso.comision = m(3);
        let ops = vec![
            op(1, date!(2026 - 01 - 10), Tipo::Compra, "BTC", "1", 20_000),
            traspaso,
            Operacion {
                plataforma: 2,
                ..op(3, date!(2026 - 07 - 01), Tipo::Venta, "BTC", "0.4", 12_000)
            },
        ];
        let c = calcular(&ops, Metodo::Fifo, None, true);
        assert_eq!(c.realizados.len(), 1);
        assert_eq!(c.realizados[0].coste, m(8_000));
        assert_eq!(c.realizados[0].plataforma, 2);
        assert_eq!(
            c.gastos,
            vec![Gasto {
                op: 2,
                fecha: date!(2026 - 06 - 01),
                importe: m(3)
            }]
        );
        let p = &c.posiciones[0];
        assert_eq!(
            (p.plataforma, p.cantidad, p.coste),
            (2, q("0.6"), m(12_000))
        );
        assert_eq!(p.lotes[0].fecha, date!(2026 - 01 - 10));
        // Sin distinguir plataformas (Renta) da lo mismo sin el traspaso.
        let g = calcular(&ops, Metodo::Fifo, None, false);
        assert_eq!(g.realizados[0].coste, m(8_000));
        assert_eq!(g.posiciones[0].plataforma, 0);
    }

    #[test]
    fn la_recompensa_es_ingreso_y_coste_de_adquisicion() {
        let ops = vec![
            op(1, date!(2026 - 01 - 10), Tipo::Recompensa, "DOT", "10", 50),
            op(2, date!(2026 - 02 - 10), Tipo::Venta, "DOT", "10", 80),
        ];
        let c = calcular(&ops, Metodo::Pmp, None, true);
        assert_eq!(c.recompensas[0].importe, m(50));
        assert_eq!(c.realizados[0].resultado, m(30));
        let r = resumen(&c, 2026);
        assert_eq!((r.recompensas, r.ganancias), (m(50), m(30)));
    }

    #[test]
    fn perdida_comision_en_cripto_y_venta_sin_existencias() {
        let mut ops = vec![
            op(1, date!(2026 - 01 - 10), Tipo::Compra, "SOL", "4", 400),
            op(2, date!(2026 - 02 - 10), Tipo::Comision, "SOL", "1", 120),
            op(3, date!(2026 - 03 - 10), Tipo::Perdida, "SOL", "1", 0),
            op(4, date!(2026 - 04 - 10), Tipo::Venta, "SOL", "3", 300),
        ];
        ops[1].hora = "09:00:00".into();
        let c = calcular(&ops, Metodo::Pmp, None, true);
        assert_eq!(c.realizados[0].resultado, m(20)); // 120 − 100
        assert_eq!(c.gastos[0].importe, m(120));
        assert_eq!(c.realizados[1].resultado, m(-100));
        // Sólo quedaban 2: la tercera unidad sale con coste cero y hay aviso.
        assert_eq!(c.realizados[2].coste, m(200));
        assert_eq!(c.realizados[2].resultado, m(100));
        assert!(c.avisos.iter().any(|a| a.contains("sólo tenía 2")));
        assert!(c.posiciones.is_empty());
    }

    #[test]
    fn resumen_de_un_ejercicio_no_natural() {
        // Ejercicio del 1/7/2025 al 30/6/2026: cuentan la venta de octubre y la de mayo.
        let ops = vec![
            op(1, date!(2025 - 01 - 10), Tipo::Compra, "BTC", "3", 30_000),
            op(2, date!(2025 - 03 - 10), Tipo::Venta, "BTC", "1", 11_000),
            op(3, date!(2025 - 10 - 10), Tipo::Venta, "BTC", "1", 12_000),
            op(4, date!(2026 - 05 - 10), Tipo::Venta, "BTC", "1", 13_000),
        ];
        let c = calcular(&ops, Metodo::Fifo, None, true);
        let r = resumen_entre(&c, 2026, date!(2025 - 07 - 01), date!(2026 - 06 - 30));
        assert_eq!((r.transmisiones, r.neto), (2, m(5_000)));
        assert_eq!(resumen(&c, 2026).transmisiones, 1);
    }

    #[test]
    fn posiciones_a_una_fecha_y_valoracion() {
        let ops = vec![
            op(
                1,
                date!(2025 - 11 - 10),
                Tipo::Compra,
                "BTC",
                "0.25",
                20_000,
            ),
            op(2, date!(2026 - 01 - 05), Tipo::Venta, "BTC", "0.25", 25_000),
        ];
        let c = calcular(&ops, Metodo::Fifo, Some(date!(2025 - 12 - 31)), true);
        assert!(c.realizados.is_empty());
        let p = &c.posiciones[0];
        let (valor, latente) = valorar(p, Precio::from_euros(90_000)).unwrap();
        assert_eq!((valor, latente), (m(22_500), m(2_500)));
        assert_eq!(resumen(&c, 2025).transmisiones, 0);
    }

    fn inicio(fecha: Date, posiciones: &[(&str, i64, &str, i64)]) -> Inicio {
        Inicio {
            fecha,
            posiciones: posiciones
                .iter()
                .map(|(t, p, c, e)| Inicial {
                    ticker: (*t).into(),
                    plataforma: *p,
                    cantidad: q(c),
                    coste: m(*e),
                })
                .collect(),
        }
    }

    /// Punto de partida a 31/12/2025 con 1 BTC a 30.000 €: la venta de 0,5 BTC en 2026 por
    /// 40.000 € gana 25.000 € y una compra posterior recalcula el coste medio. Las operaciones
    /// incompletas de antes (una venta sin compra) no cuentan.
    #[test]
    fn el_punto_de_partida_sustituye_a_las_operaciones_antiguas() {
        let i = inicio(
            date!(2025 - 12 - 31),
            &[("BTC", SIN_PLATAFORMA, "1", 30_000)],
        );
        let ops = vec![
            // Historial incompleto: vende lo que no se sabe cuándo se compró.
            op(1, date!(2023 - 05 - 10), Tipo::Venta, "BTC", "2", 50_000),
            op(2, date!(2025 - 12 - 31), Tipo::Compra, "BTC", "9", 1),
            op(3, date!(2026 - 03 - 10), Tipo::Venta, "BTC", "0.5", 40_000),
            op(4, date!(2026 - 06 - 10), Tipo::Compra, "BTC", "0.5", 35_000),
            op(5, date!(2026 - 09 - 10), Tipo::Venta, "BTC", "0.5", 30_000),
        ];
        for metodo in [Metodo::Pmp, Metodo::Fifo] {
            let c = calcular_desde(&ops, metodo, None, true, Some(&i));
            assert!(c.huecos.is_empty(), "{:?}", c.huecos);
            assert!(c.avisos.is_empty(), "{:?}", c.avisos);
            assert_eq!(c.realizados.len(), 2);
            // 0,5 de 1 BTC a 30.000: coste 15.000, ganancia 25.000 (la venta es de Kraken, el
            // punto de partida sin plataforma).
            assert_eq!(c.realizados[0].coste, m(15_000));
            assert_eq!(c.realizados[0].resultado, m(25_000));
            assert_eq!(c.realizados[0].plataforma, 1);
            let r = resumen(&c, 2026);
            assert_eq!(r.transmisiones, 2);
            assert!(resumen(&c, 2023).transmisiones == 0);
        }
        // PMP: tras la compra, 1 BTC por 15.000 + 35.000 = 50.000 € → la venta de 0,5 cuesta
        // 25.000 y gana 5.000.
        let pmp = calcular_desde(&ops, Metodo::Pmp, None, true, Some(&i));
        assert_eq!(pmp.realizados[1].coste, m(25_000));
        assert_eq!(pmp.realizados[1].resultado, m(5_000));
        let total: Money = pmp.posiciones.iter().map(|p| p.coste).sum();
        assert_eq!(total, m(25_000));
        // A 31/12/2025 hay lo del punto de partida; antes, nada (no se calcula).
        let a_corte = calcular_desde(
            &ops,
            Metodo::Pmp,
            Some(date!(2025 - 12 - 31)),
            true,
            Some(&i),
        );
        assert_eq!(a_corte.posiciones.len(), 1);
        assert_eq!(
            (a_corte.posiciones[0].cantidad, a_corte.posiciones[0].coste),
            (q("1"), m(30_000))
        );
        let antes = calcular_desde(
            &ops,
            Metodo::Pmp,
            Some(date!(2024 - 12 - 31)),
            true,
            Some(&i),
        );
        assert!(antes.posiciones.is_empty() && antes.realizados.is_empty());
        // Sin punto de partida, el historial incompleto sale como hueco.
        let sin = calcular(&ops, Metodo::Pmp, None, true);
        assert_eq!(sin.huecos.len(), 1);
        assert_eq!((sin.huecos[0].op, sin.huecos[0].falta), (1, q("2")));
    }

    /// Una posición de partida con plataforma sólo sirve a esa plataforma; sin plataforma, a la
    /// que le falte. La Renta (sin plataformas) lo junta todo.
    #[test]
    fn el_punto_de_partida_por_plataforma() {
        let i = inicio(
            date!(2025 - 12 - 31),
            &[("ETH", 2, "2", 4_000), ("ETH", SIN_PLATAFORMA, "1", 3_000)],
        );
        let ops = vec![
            op(1, date!(2026 - 01 - 10), Tipo::Compra, "ETH", "1", 2_500),
            // En la plataforma 1 hay 1 ETH: el otro sale del de partida sin plataforma.
            op(2, date!(2026 - 02 - 10), Tipo::Venta, "ETH", "2", 6_000),
        ];
        // FIFO por plataforma: sale el ETH comprado en la 1 (2.500) y el de partida sin
        // plataforma (3.000); el de la 2 se queda como estaba.
        let c = calcular_desde(&ops, Metodo::Fifo, None, true, Some(&i));
        assert!(c.huecos.is_empty());
        assert_eq!(c.realizados[0].coste, m(5_500));
        let eth2 = c.posiciones.iter().find(|p| p.plataforma == 2).unwrap();
        assert_eq!((eth2.cantidad, eth2.coste), (q("2"), m(4_000)));
        assert!(c.posiciones.iter().all(|p| p.plataforma != SIN_PLATAFORMA));
        // Precio medio: el de todo el ETH, esté donde esté (4 ETH por 9.500 → salen 2 a 4.750),
        // con o sin plataformas; el que queda en la 2 lleva el medio.
        for por_plataforma in [true, false] {
            let c = calcular_desde(&ops, Metodo::Pmp, None, por_plataforma, Some(&i));
            assert!(c.huecos.is_empty());
            assert_eq!(c.realizados[0].coste, m(4_750));
            assert_eq!(c.posiciones.len(), 1);
            assert_eq!(
                (c.posiciones[0].cantidad, c.posiciones[0].coste),
                (q("2"), m(4_750))
            );
        }
        // Una venta de más en otra plataforma, con el de partida ya gastado, es un hueco.
        let mut mas = ops.clone();
        mas.push(Operacion {
            plataforma: 3,
            ..op(3, date!(2026 - 03 - 10), Tipo::Venta, "ETH", "1", 3_000)
        });
        let c = calcular_desde(&mas, Metodo::Pmp, None, true, Some(&i));
        assert_eq!(c.huecos.len(), 1);
        assert_eq!(c.huecos[0].plataforma, 3);
        // Con precio medio el coste sale igual del de todo el ETH (falta un traspaso, no una
        // compra).
        assert_eq!(c.realizados[1].coste, m(2_375));
        assert!(c.avisos[0].contains("falta un traspaso"), "{:?}", c.avisos);
        // Queda 1 ETH por 2.375 €, aunque la plataforma 2 siga teniendo 2 en sus registros (la
        // venta de la 3 no las tocó): el coste de las posiciones no puede pasar de lo que queda.
        let total: Money = c.posiciones.iter().map(|p| p.coste).sum();
        assert_eq!(total, m(2_375), "{:?}", c.posiciones);
    }

    /// Venta en una plataforma sin unidades (falta un traspaso) y luego una compra: el coste de
    /// las posiciones es siempre el del medio que queda, repartido por unidades.
    #[test]
    fn el_coste_de_las_posiciones_no_se_duplica_con_un_traspaso_que_falta() {
        let ops = vec![
            op(1, date!(2026 - 01 - 10), Tipo::Compra, "BTC", "1", 20_000),
            Operacion {
                plataforma: 2,
                ..op(2, date!(2026 - 02 - 10), Tipo::Venta, "BTC", "1", 30_000)
            },
            op(3, date!(2026 - 03 - 10), Tipo::Compra, "BTC", "1", 40_000),
        ];
        let c = calcular(&ops, Metodo::Pmp, None, true);
        assert_eq!(c.realizados[0].coste, m(20_000));
        assert_eq!(c.huecos.len(), 1);
        let total: Money = c.posiciones.iter().map(|p| p.coste).sum();
        assert_eq!(total, m(40_000), "{:?}", c.posiciones);
    }

    /// Una permuta y una comisión en cripto después del corte, y dos años: el coste pendiente se
    /// arrastra de un año a otro desde el punto de partida.
    #[test]
    fn tras_el_corte_permutas_comisiones_y_varios_años() {
        let i = inicio(
            date!(2025 - 12 - 31),
            &[("BTC", SIN_PLATAFORMA, "1", 30_000)],
        );
        let ops = vec![
            // Del día del corte: ya está en el saldo de la gestoría.
            op(1, date!(2025 - 12 - 31), Tipo::Venta, "BTC", "1", 90_000),
            Operacion {
                ticker2: Some("ETH".into()),
                cantidad2: Some(q("10")),
                ..op(
                    2,
                    date!(2026 - 05 - 10),
                    Tipo::Permuta,
                    "BTC",
                    "0.5",
                    40_000,
                )
            },
            op(3, date!(2026 - 05 - 11), Tipo::Comision, "ETH", "1", 4_000),
            op(4, date!(2027 - 02 - 01), Tipo::Venta, "BTC", "0.5", 50_000),
            op(5, date!(2027 - 03 - 01), Tipo::Venta, "ETH", "9", 45_000),
        ];
        for metodo in [Metodo::Pmp, Metodo::Fifo] {
            let c = calcular_desde(&ops, metodo, None, true, Some(&i));
            assert!(c.huecos.is_empty(), "{:?}", c.huecos);
            let r: Vec<(i64, Money)> = c.realizados.iter().map(|r| (r.op, r.coste)).collect();
            assert_eq!(
                r,
                vec![
                    (2, m(15_000)),
                    (3, m(4_000)),
                    (4, m(15_000)),
                    (5, m(36_000))
                ]
            );
            assert_eq!(resumen(&c, 2026).neto, m(25_000));
            assert_eq!(resumen(&c, 2027).neto, m(44_000));
            assert!(c.posiciones.is_empty(), "{:?}", c.posiciones);
        }
    }

    /// Con precio medio, una compra en otra plataforma cambia el medio de todas las unidades.
    #[test]
    fn el_precio_medio_es_de_todo_el_activo() {
        let ops = vec![
            op(1, date!(2026 - 01 - 10), Tipo::Compra, "BTC", "1", 20_000),
            Operacion {
                plataforma: 2,
                ..op(2, date!(2026 - 02 - 10), Tipo::Compra, "BTC", "1", 40_000)
            },
            op(3, date!(2026 - 03 - 10), Tipo::Venta, "BTC", "1", 50_000),
        ];
        let c = calcular(&ops, Metodo::Pmp, None, true);
        assert_eq!(c.realizados[0].coste, m(30_000));
        assert!(c.huecos.is_empty());
        let p2 = c.posiciones.iter().find(|p| p.plataforma == 2).unwrap();
        assert_eq!((p2.cantidad, p2.coste), (q("1"), m(30_000)));
        assert_eq!(c.posiciones.len(), 1);
    }
}
