//! Asientos de partida doble. Invariantes: al menos dos líneas, cada línea con importe en el
//! debe **o** en el haber (positivo), y Σdebe = Σhaber.

use serde::{Deserialize, Serialize};
use time::Date;

use super::account::AccountCode;
use crate::money::Money;

/// Origen del asiento (trazabilidad hacia el documento que lo genera).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Origin {
    FacturaEmitida(i64),
    FacturaRecibida(i64),
    Banco(i64),
    Amortizacion(i64),
    /// Liquidación del IVA del periodo: `año × 100 + periodo` (2026T1 → 202601).
    LiquidacionIva(i64),
    /// Cobro o pago de una factura fuera del banco conciliado (caja u otra cuenta): id del
    /// registro de la cartera.
    Cartera(i64),
    /// Importado de otro programa (SUENLACE de a3 o Excel): id del lote de importación.
    Importado(i64),
    /// Asiento de un ejercicio entero llevado por el gestor (a3, Sage, ContaSOL…) cargado para
    /// tener el histórico: id del lote (`gestor_import`). La apertura, la regularización y el
    /// cierre de ese ejercicio van con sus propios orígenes.
    Gestor(i64),
    /// Pago de un modelo fuera del banco conciliado: id del registro `tax_payment`.
    PagoModelo(i64),
    /// Baja o venta de un elemento del inmovilizado: id del elemento.
    BajaInmovilizado(i64),
    /// Alta o cuota de un contrato de leasing: id del contrato.
    Leasing(i64),
    /// Alta, cuota, devengo de intereses o reclasificación de un préstamo concedido o recibido:
    /// id del préstamo.
    Prestamo(i64),
    Nomina,
    Manual,
    Apertura,
    Regularizacion,
    Cierre,
    /// Impuesto sobre Sociedades del ejercicio (630 / 473 / 4752 o 4709): el ejercicio.
    ImpuestoSociedades(i64),
    /// Aplicación del resultado de un ejercicio (129 → reservas, dividendos…): ese ejercicio.
    AplicacionResultado(i64),
    /// Pago de dividendos a los socios (526 al debe; retención en la 4751 y tesorería al haber):
    /// id del registro `dividend_payment`.
    Dividendo(i64),
    /// Gastos de viaje de un administrador o empleado (dietas, kilometraje, peajes): id del
    /// registro `travel_expense`.
    GastosViaje(i64),
    /// Contraasiento de una factura emitida anulada: id de la factura.
    AnulacionFactura(i64),
    /// Recibo devuelto o transferencia rechazada de una remesa que el banco ya apuntó: id de la
    /// remesa.
    DevolucionRemesa(i64),
    /// Regularización de existencias al cierre (61x/71x contra el grupo 3): el ejercicio.
    Existencias(i64),
    /// Operación con criptomonedas (id de `crypto_op`) o, en negativo, el deterioro al cierre
    /// de ese ejercicio (−2026).
    Cripto(i64),
    /// Ajuste de cierre (periodificación, factura pendiente de recibir o de emitir, deterioro
    /// de un crédito comercial o provisión) o su reversión: id del registro `ajuste_cierre`.
    AjusteCierre(i64),
    /// Valoración al cierre de las partidas monetarias en moneda extranjera (bancos, clientes y
    /// proveedores en divisa; PGC, NRV 11.ª): el ejercicio; en negativo (−2026), su reversión
    /// el primer día del ejercicio siguiente.
    DiferenciasCambio(i64),
    /// Operación de la cartera de valores (id de `security_op`) o, en negativo, la valoración
    /// al cierre de ese ejercicio (−2026): ajuste a valor razonable o deterioro.
    Valores(i64),
    /// Acuerdo de reparto de un dividendo a cuenta (557 a 526) o con cargo a reservas (113 o 120
    /// a 526): id del registro `dividend_agreement`.
    AcuerdoDividendo(i64),
    /// Concesión, paso a no reintegrable, cobro o imputación a resultados de una subvención,
    /// donación o legado: id del registro `subvencion`.
    Subvencion(i64),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryLine {
    pub account: AccountCode,
    pub debit: Money,
    pub credit: Money,
    pub description: Option<String>,
}

impl EntryLine {
    pub fn debit(account: AccountCode, amount: Money) -> Self {
        EntryLine {
            account,
            debit: amount,
            credit: Money::ZERO,
            description: None,
        }
    }

    pub fn credit(account: AccountCode, amount: Money) -> Self {
        EntryLine {
            account,
            debit: Money::ZERO,
            credit: amount,
            description: None,
        }
    }

    /// Línea en el lado natural si el importe es positivo; en el contrario si es negativo
    /// (rectificativas por diferencias, abonos).
    pub fn signed(account: AccountCode, amount: Money, natural_debit: bool) -> Option<Self> {
        if amount.is_zero() {
            return None;
        }
        let debit_side = natural_debit != amount.is_negative();
        Some(if debit_side {
            Self::debit(account, amount.abs())
        } else {
            Self::credit(account, amount.abs())
        })
    }

    /// Saldo deudor (debe − haber).
    pub fn net(&self) -> Money {
        self.debit - self.credit
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    pub date: Date,
    pub concept: String,
    pub origin: Origin,
    pub lines: Vec<EntryLine>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EntryError {
    #[error("el asiento necesita al menos dos líneas")]
    TooFewLines,
    #[error("línea {0}: importe en el debe y en el haber, nulo o negativo")]
    InvalidLine(usize),
    #[error("el asiento no cuadra: debe {debit}, haber {credit}")]
    Unbalanced { debit: Money, credit: Money },
    #[error("falta el concepto del asiento")]
    MissingConcept,
}

impl JournalEntry {
    pub fn total_debit(&self) -> Money {
        self.lines.iter().map(|l| l.debit).sum()
    }

    pub fn total_credit(&self) -> Money {
        self.lines.iter().map(|l| l.credit).sum()
    }

    pub fn validate(&self) -> Result<(), EntryError> {
        if self.concept.trim().is_empty() {
            return Err(EntryError::MissingConcept);
        }
        if self.lines.len() < 2 {
            return Err(EntryError::TooFewLines);
        }
        for (i, l) in self.lines.iter().enumerate() {
            let one_side = l.debit.is_zero() != l.credit.is_zero();
            if !one_side || l.debit.is_negative() || l.credit.is_negative() {
                return Err(EntryError::InvalidLine(i + 1));
            }
        }
        let (d, c) = (self.total_debit(), self.total_credit());
        if d != c {
            return Err(EntryError::Unbalanced {
                debit: d,
                credit: c,
            });
        }
        Ok(())
    }

    /// Une líneas de la misma cuenta y lado (más legible en el diario).
    pub fn compact(mut self) -> Self {
        let mut out: Vec<EntryLine> = Vec::new();
        for l in self.lines.drain(..) {
            if let Some(o) = out.iter_mut().find(|o| {
                o.account == l.account
                    && o.debit.is_zero() == l.debit.is_zero()
                    && o.description == l.description
            }) {
                o.debit += l.debit;
                o.credit += l.credit;
            } else {
                out.push(l);
            }
        }
        self.lines = out;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounting::account::acc;
    use time::macros::date;

    fn e(lines: Vec<EntryLine>) -> JournalEntry {
        JournalEntry {
            date: date!(2026 - 10 - 13),
            concept: "x".into(),
            origin: Origin::Manual,
            lines,
        }
    }

    #[test]
    fn validacion() {
        let m = Money::from_euros;
        assert_eq!(
            e(vec![
                EntryLine::debit(acc("572"), m(100, 0)),
                EntryLine::credit(acc("100"), m(100, 0))
            ])
            .validate(),
            Ok(())
        );
        assert_eq!(
            e(vec![EntryLine::debit(acc("572"), m(1, 0))]).validate(),
            Err(EntryError::TooFewLines)
        );
        assert!(matches!(
            e(vec![
                EntryLine::debit(acc("572"), m(100, 0)),
                EntryLine::credit(acc("100"), m(99, 0))
            ])
            .validate(),
            Err(EntryError::Unbalanced { .. })
        ));
        let mut both = EntryLine::debit(acc("572"), m(1, 0));
        both.credit = m(1, 0);
        assert_eq!(
            e(vec![both.clone(), both]).validate(),
            Err(EntryError::InvalidLine(1))
        );
        assert_eq!(
            e(vec![
                EntryLine::debit(acc("572"), Money::ZERO),
                EntryLine::credit(acc("100"), Money::ZERO)
            ])
            .validate(),
            Err(EntryError::InvalidLine(1))
        );
    }

    #[test]
    fn lineas_con_signo() {
        let l = EntryLine::signed(acc("430"), Money::from_cents(-500), true).unwrap();
        assert_eq!((l.debit, l.credit), (Money::ZERO, Money::from_cents(500)));
        assert!(EntryLine::signed(acc("430"), Money::ZERO, true).is_none());
    }

    #[test]
    fn compactar() {
        let x = e(vec![
            EntryLine::debit(acc("472"), Money::from_cents(10)),
            EntryLine::debit(acc("472"), Money::from_cents(20)),
            EntryLine::credit(acc("400"), Money::from_cents(30)),
        ])
        .compact();
        assert_eq!(x.lines.len(), 2);
        assert_eq!(x.lines[0].debit, Money::from_cents(30));
    }
}
