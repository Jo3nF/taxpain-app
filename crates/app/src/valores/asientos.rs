//! Asientos de la cartera de valores de una sociedad: se enseñan aquí y se graban como lote
//! deshacible con «Contabilizar» ([`super::contabilidad`]); el ajuste del cierre, desde el paso
//! del cierre.
//!
//! - Compra: 540x (o 250x; 541x/251x los bonos) / efectivo en el bróker. A coste, con la
//!   comisión; en negociación, la comisión a 669.
//! - Venta: efectivo / 540x por el coste medio, y la diferencia a 766 o 666 (a coste) o a 7630 o
//!   6630 (negociación).
//! - Dividendo: efectivo / 760 por el íntegro; la retención en origen, 6300 / efectivo (gasto por
//!   impuesto: Resolución del ICAC de 9-2-2016, art. 12.1; consulta 7 del BOICAC 115,
//!   `TODO(VERIFY)` contra el BOICAC oficial, que no se pudo abrir); la española, 473 / efectivo.
//! - Intereses: efectivo / 769 (pagados: 669 / efectivo). Comisiones de la cuenta: 669 /
//!   efectivo. Otros tributos: 631 / efectivo.
//! - Ingresos y retiradas de efectivo: no se proponen (se contabilizan desde el banco).
//! - En negociación, el ajuste a valor razonable al cierre: 540x / 7630 o 6630 / 540x.
//!
//! - Opciones y otros derivados: activos mantenidos para negociar (NRV 9.ª), en la 5590
//!   «Activos por derivados financieros a corto plazo» (`TODO(VERIFY)` su nombre en el cuadro
//!   del PGC PYMES) y su resultado siempre a 7630 o 6630, sea cual sea la cartera.
//!
//! Una subcuenta por valor (5400001, 5400002…): cada valor «registrado en su contabilidad de
//! forma individualizada» (art. 42 ter.4.b RGAT).

use serde::Serialize;
use taxpain_core::money::Money;
use time::Date;

use super::calculo::Calculo;
use super::informe::PosicionView;
use super::{Cartera, Clase, Cuenta, Op, Plazo, Settings, TipoOp, Valor};

/// Cuenta de las opciones y otros derivados con valor positivo: 5590 «Activos por derivados
/// financieros a corto plazo».
pub const DERIVADOS: &str = "5590";

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Linea {
    pub cuenta: String,
    pub debe: Money,
    pub haber: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Propuesto {
    pub fecha: Date,
    pub concepto: String,
    /// Operación de la que sale (ninguna en el ajuste del cierre).
    pub op: Option<i64>,
    /// Asiento ya grabado de esa operación.
    pub entry_id: Option<i64>,
    pub lineas: Vec<Linea>,
}

/// Subcuenta de un valor: la suya o la base (540, 250, 541 o 251) más su número.
pub fn subcuenta(s: &Settings, x: Option<&Valor>, id: i64) -> String {
    if let Some(a) = x.map(|x| x.account.as_str()).filter(|a| !a.is_empty()) {
        return a.to_owned();
    }
    let deuda = x.is_some_and(|x| x.kind.deuda());
    let derivados = DERIVADOS.to_owned();
    let base = match (s.plazo, deuda) {
        _ if x.is_some_and(|x| x.kind == Clase::Derivado) => &derivados,
        (Plazo::Corto, false) => &s.cuentas.corto_patrimonio,
        (Plazo::Corto, true) => &s.cuentas.corto_deuda,
        (Plazo::Largo, false) => &s.cuentas.largo_patrimonio,
        (Plazo::Largo, true) => &s.cuentas.largo_deuda,
    };
    let ancho = 7usize.saturating_sub(base.len()).max(1);
    format!("{base}{id:0ancho$}")
}

struct Asiento {
    lineas: Vec<Linea>,
}

impl Asiento {
    fn new() -> Self {
        Asiento { lineas: Vec::new() }
    }

    /// Importe al debe (o al haber si es negativo).
    fn debe(&mut self, cuenta: &str, m: Money) -> &mut Self {
        if m.is_negative() {
            return self.haber(cuenta, -m);
        }
        if !m.is_zero() {
            self.lineas.push(Linea {
                cuenta: cuenta.to_owned(),
                debe: m,
                haber: Money::ZERO,
            });
        }
        self
    }

    fn haber(&mut self, cuenta: &str, m: Money) -> &mut Self {
        if m.is_negative() {
            return self.debe(cuenta, -m);
        }
        if !m.is_zero() {
            self.lineas.push(Linea {
                cuenta: cuenta.to_owned(),
                debe: Money::ZERO,
                haber: m,
            });
        }
        self
    }

    fn cuadra(&self) -> bool {
        self.lineas.len() >= 2
            && self.lineas.iter().map(|l| l.debe).sum::<Money>()
                == self.lineas.iter().map(|l| l.haber).sum::<Money>()
    }
}

pub fn proponer(
    s: &Settings,
    cs: &[Cuenta],
    vs: &[Valor],
    ops_year: &[&Op],
    calc: &Calculo,
    posiciones: &[PosicionView],
    fecha: Date,
) -> Vec<Propuesto> {
    let c = &s.cuentas;
    let negociacion = s.cartera == Cartera::Negociacion;
    let (beneficio, perdida) = if negociacion {
        (&c.beneficio_vr, &c.perdida_vr)
    } else {
        (&c.beneficio, &c.perdida)
    };
    let mut out = Vec::new();
    for o in ops_year {
        let cuenta = cs.iter().find(|x| x.id == o.account_id);
        let teso = cuenta
            .map(|x| x.account.as_str())
            .filter(|a| !a.is_empty())
            .unwrap_or(&c.tesoreria);
        let x = o.security_id.and_then(|id| vs.iter().find(|x| x.id == id));
        let ticker = x.map(|x| x.ticker.as_str()).unwrap_or("");
        let valor = o.security_id.map(|id| subcuenta(s, x, id));
        let mut a = Asiento::new();
        let concepto = match o.kind {
            TipoOp::Compra => {
                let Some(valor) = &valor else { continue };
                if negociacion {
                    a.debe(valor, o.eur)
                        .debe(&c.comisiones, o.fee_eur)
                        .haber(teso, o.eur + o.fee_eur);
                } else {
                    a.debe(valor, o.eur + o.fee_eur)
                        .haber(teso, o.eur + o.fee_eur);
                }
                format!("Compra de {} {ticker}", o.qty)
            }
            TipoOp::Venta => {
                let (Some(valor), Some(r)) =
                    (&valor, calc.realizados.iter().find(|r| r.op == o.id))
                else {
                    continue;
                };
                a.debe(teso, o.eur - o.fee_eur);
                if negociacion {
                    a.debe(&c.comisiones, o.fee_eur);
                }
                a.haber(valor, r.coste);
                let derivado = x.is_some_and(|x| x.kind == Clase::Derivado);
                let (beneficio, perdida) = if derivado {
                    (&c.beneficio_vr, &c.perdida_vr)
                } else {
                    (beneficio, perdida)
                };
                if r.resultado.is_negative() {
                    a.debe(perdida, -r.resultado);
                } else {
                    a.haber(beneficio, r.resultado);
                }
                if derivado && o.eur.is_zero() {
                    format!("Vencimiento sin valor de {} {ticker}", o.qty)
                } else {
                    format!("Venta de {} {ticker}", o.qty)
                }
            }
            TipoOp::Dividendo => {
                a.debe(teso, o.eur).haber(&c.dividendos, o.eur);
                format!("Dividendo de {ticker}")
            }
            TipoOp::Retencion => {
                let pais = super::informe::pais_de(o, vs);
                let es = pais == "ES";
                let gasto = if es {
                    &c.retencion_espana
                } else {
                    &c.impuesto_extranjero
                };
                a.debe(gasto, o.eur).haber(teso, o.eur);
                if es {
                    format!("Retención española {ticker}")
                } else {
                    format!("Retención en origen ({pais}) {ticker}")
                }
            }
            TipoOp::Interes => {
                if o.eur.is_negative() {
                    a.debe(&c.comisiones, -o.eur).haber(teso, -o.eur);
                    "Intereses pagados al bróker".into()
                } else {
                    a.debe(teso, o.eur).haber(&c.intereses, o.eur);
                    "Intereses del efectivo en el bróker".into()
                }
            }
            TipoOp::Comision => {
                a.debe(&c.comisiones, o.eur).haber(teso, o.eur);
                "Comisión del bróker".into()
            }
            TipoOp::Impuesto => {
                a.debe(&c.tributos, o.eur).haber(teso, o.eur);
                "Impuesto".into()
            }
            TipoOp::Deposito | TipoOp::Retirada | TipoOp::Traspaso | TipoOp::Split => continue,
        };
        if a.cuadra() {
            out.push(Propuesto {
                fecha: o.date,
                concepto: format!(
                    "{concepto} ({})",
                    cuenta.map(|x| x.name.as_str()).unwrap_or("")
                ),
                op: Some(o.id),
                entry_id: o.entry_id,
                lineas: a.lineas,
            });
        }
    }
    if negociacion {
        for p in posiciones {
            let Some(l) = p.latente.filter(|l| !l.is_zero()) else {
                continue;
            };
            let x = vs.iter().find(|x| x.id == p.security_id);
            let valor = subcuenta(s, x, p.security_id);
            let mut a = Asiento::new();
            if l.is_negative() {
                a.debe(&c.perdida_vr, -l).haber(&valor, -l);
            } else {
                a.debe(&valor, l).haber(&c.beneficio_vr, l);
            }
            if a.cuadra() {
                out.push(Propuesto {
                    fecha,
                    concepto: format!(
                        "Ajuste a valor razonable de {} al cierre (propuesta: confírmalo antes)",
                        p.ticker
                    ),
                    op: None,
                    entry_id: None,
                    lineas: a.lineas,
                });
            }
        }
    }
    out
}
