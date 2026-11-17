//! Asientos automáticos desde facturas (ver `docs/ACCOUNTING_RULES.md`):
//!
//! - Emitida: 430 (cliente) y 473 (retención soportada) al debe; 70x (base) y 477 (IVA y
//!   recargo repercutidos) al haber; el IVA de otro Estado miembro (ventanilla única, OSS), en
//!   su subcuenta 4779 por país.
//! - Recibida: 6xx (base + IVA no deducible) y 472 (IVA deducible) al debe; 400/410
//!   (proveedor) y 4751 (retención practicada) al haber.
//!
//! Los importes negativos (rectificativas por diferencias) van al lado contrario. Las líneas de
//! anticipo llevan su base a la 438 (anticipos de clientes) en vez de a ventas.

use serde::{Deserialize, Serialize};
use time::Date;

use super::account::{AccountCode, acc};
use super::entry::{EntryLine, JournalEntry, Origin};
use crate::invoice::{Invoice, Totals};
use crate::money::{Money, Rate};

/// Cuentas que usa la contabilización automática (configurables por empresa).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostingAccounts {
    /// 705 Prestaciones de servicios (profesionales) o 700 Ventas de mercaderías.
    pub revenue: AccountCode,
    /// 629 Otros servicios, 600 Compras… (por defecto para recibidas).
    pub expense: AccountCode,
    pub customers: AccountCode,
    /// 400 Proveedores o 410 Acreedores por prestaciones de servicios.
    pub suppliers: AccountCode,
    pub vat_output: AccountCode,
    pub vat_input: AccountCode,
    /// 473 Hacienda Pública, retenciones y pagos a cuenta.
    pub withholding_receivable: AccountCode,
    /// 4751 Hacienda Pública, acreedora por retenciones practicadas.
    pub withholding_payable: AccountCode,
    pub bank: AccountCode,
    /// Dígitos de las subcuentas de terceros (7 → 4300001).
    pub subaccount_width: usize,
}

impl Default for PostingAccounts {
    fn default() -> Self {
        PostingAccounts {
            revenue: acc("705"),
            expense: acc("629"),
            customers: acc("430"),
            suppliers: acc("410"),
            vat_output: acc("477"),
            vat_input: acc("472"),
            withholding_receivable: acc("473"),
            withholding_payable: acc("4751"),
            bank: acc("572"),
            subaccount_width: 7,
        }
    }
}

/// Asiento de una factura emitida. `customer` es la subcuenta del cliente.
pub fn post_issued(
    invoice: &Invoice,
    totals: &Totals,
    a: &PostingAccounts,
    customer: &AccountCode,
    id: i64,
) -> JournalEntry {
    post_issued_with_advance(invoice, totals, a, customer, id, None)
}

/// [`post_issued`] con los euros de la 438 dados (`advance`, con el signo de la base) en vez de
/// la base de las líneas de anticipo: en moneda extranjera, un anticipo descontado en la final
/// se cancela por los euros con que se contabilizó (partida no monetaria, NRV 11.ª PGC) y la
/// diferencia con la base en euros de la factura va a ventas.
pub fn post_issued_with_advance(
    invoice: &Invoice,
    totals: &Totals,
    a: &PostingAccounts,
    customer: &AccountCode,
    id: i64,
    advance: Option<Money>,
) -> JournalEntry {
    // Anticipos: su base va a la 438 (al haber al cobrar el anticipo; al debe al descontarlo,
    // con importe negativo, en la factura final).
    let advance: Money = advance.unwrap_or_else(|| {
        invoice
            .lines
            .iter()
            .filter(|l| l.advance)
            .map(crate::invoice::totals::line_base)
            .sum()
    });
    let lines = [
        EntryLine::signed(customer.clone(), totals.payable, true),
        EntryLine::signed(a.withholding_receivable.clone(), totals.withholding, true),
        EntryLine::signed(a.revenue.clone(), totals.base - advance, false),
        EntryLine::signed(acc("438"), advance, false),
        EntryLine::signed(a.vat_output.clone(), totals.vat + totals.surcharge, false),
        // IVA de otro Estado miembro (ventanilla única): en su subcuenta de la 477 por país,
        // que la liquidación del 303 no salda (se paga con el 369).
        totals.oss.as_ref().and_then(|o| {
            EntryLine::signed(
                crate::invoice::oss::cuenta_oss(&o.country, a.subaccount_width),
                o.vat,
                false,
            )
        }),
    ];
    JournalEntry {
        date: invoice.issue_date,
        concept: format!("Factura emitida {}", invoice.number),
        origin: Origin::FacturaEmitida(id),
        lines: lines.into_iter().flatten().collect(),
    }
}

/// Datos de una factura recibida ya revisada.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceivedInvoice {
    pub id: i64,
    pub date: Date,
    pub number: String,
    pub supplier_name: String,
    pub base: Money,
    pub vat: Money,
    pub withholding: Money,
    /// Porcentaje del IVA deducible (0–100).
    pub deductible_pct: u8,
    pub expense_account: Option<AccountCode>,
    /// IVA autorrepercutido por inversión del sujeto pasivo (adquisiciones intracomunitarias,
    /// servicios de empresas no establecidas…; art. 84.Uno.2º y 85 LIVA): la factura llega sin
    /// IVA y el destinatario lo devenga (477) y lo deduce (472) a la vez.
    pub self_assessed_vat: Money,
    /// DUA de importación: sólo se contabiliza el IVA pagado en la aduana (472 contra el
    /// proveedor o agente de aduanas); la base va en la factura del proveedor extranjero.
    #[serde(default)]
    pub import_dua: bool,
}

/// Asiento de una factura recibida. `supplier` es la subcuenta del proveedor.
pub fn post_received(
    r: &ReceivedInvoice,
    a: &PostingAccounts,
    supplier: &AccountCode,
) -> JournalEntry {
    let pct = Rate::percent(i32::from(r.deductible_pct.min(100)));
    let deductible = r.vat.apply_rate(pct) + r.self_assessed_vat.apply_rate(pct);
    let non_deductible = r.vat + r.self_assessed_vat - deductible;
    let (payable, expensed) = if r.import_dua {
        (r.vat - r.withholding, non_deductible)
    } else {
        (r.base + r.vat - r.withholding, r.base + non_deductible)
    };
    let expense = r
        .expense_account
        .clone()
        .unwrap_or_else(|| a.expense.clone());
    let lines = [
        EntryLine::signed(expense, expensed, true),
        EntryLine::signed(a.vat_input.clone(), deductible, true),
        EntryLine::signed(supplier.clone(), payable, false),
        EntryLine::signed(a.withholding_payable.clone(), r.withholding, false),
        EntryLine::signed(a.vat_output.clone(), r.self_assessed_vat, false),
    ];
    JournalEntry {
        date: r.date,
        concept: format!("Factura recibida {} {}", r.number, r.supplier_name),
        origin: Origin::FacturaRecibida(r.id),
        lines: lines.into_iter().flatten().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::invoice::{
        InvoiceKind, InvoiceLine, Party, PartyId, RoundingMode, VatTreatment, compute_totals,
    };
    use proptest::prelude::*;
    use time::macros::date;

    fn invoice(lines: Vec<InvoiceLine>) -> Invoice {
        Invoice {
            kind: InvoiceKind::Completa,
            number: "A2026-0001".into(),
            issue_date: date!(2026 - 10 - 13),
            operation_date: None,
            issuer: Party {
                id: PartyId::Nif("12345678Z".into()),
                name: "Yo".into(),
                address: None,
                person: None,
            },
            recipient: None,
            lines,
            rectified: vec![],
            description: "x".into(),
            notes: None,
        }
    }

    fn line(cents: i64, vat: i32, wh: Option<i32>, re: Option<i32>) -> InvoiceLine {
        InvoiceLine {
            description: "x".into(),
            quantity_milli: 1000,
            unit_price: Money::from_cents(cents),
            discount: Rate::ZERO,
            vat: VatTreatment::Taxed {
                rate: Rate::from_bp(vat),
            },
            surcharge: re.map(Rate::from_bp),
            withholding: wh.map(Rate::from_bp),
            advance: false,
        }
    }

    #[test]
    fn emitida_con_iva_de_destino_va_a_la_subcuenta_4779_del_pais() {
        let mut l = line(10_000, 0, None, None);
        l.vat = VatTreatment::NotSubject(crate::invoice::NotSubject::N2);
        let inv = invoice(vec![l]);
        let mut t = compute_totals(&inv.lines, RoundingMode::PerRate);
        crate::invoice::oss::apply(
            &mut t,
            "DE",
            crate::invoice::OssSupply::Servicios,
            Rate::percent(19),
        );
        let e = post_issued(&inv, &t, &PostingAccounts::default(), &acc("4300001"), 1);
        let get = |c: &str| e.lines.iter().find(|x| x.account.as_str() == c).cloned();
        assert_eq!(get("4300001").unwrap().debit, Money::from_euros(119, 0));
        assert_eq!(get("705").unwrap().credit, Money::from_euros(100, 0));
        assert_eq!(get("4779006").unwrap().credit, Money::from_euros(19, 0));
        assert!(get("477").is_none());
    }

    #[test]
    fn inversion_del_sujeto_pasivo_devenga_y_deduce() {
        // Servicio de una empresa de otro país de la UE: 100 € sin IVA, 21 € autorrepercutidos.
        let r = ReceivedInvoice {
            id: 4,
            date: date!(2026 - 10 - 02),
            number: "IE-1".into(),
            supplier_name: "Proveedor UE".into(),
            base: Money::from_euros(100, 0),
            vat: Money::ZERO,
            withholding: Money::ZERO,
            deductible_pct: 100,
            expense_account: None,
            self_assessed_vat: Money::from_euros(21, 0),
            import_dua: false,
        };
        let a = PostingAccounts::default();
        let e = post_received(&r, &a, &acc("4100009"));
        assert_eq!(e.validate(), Ok(()));
        let amount = |code: &str, debit: bool| {
            e.lines
                .iter()
                .filter(|l| l.account.as_str() == code)
                .map(|l| if debit { l.debit } else { l.credit })
                .sum::<Money>()
        };
        assert_eq!(amount(a.vat_input.as_str(), true), Money::from_euros(21, 0));
        assert_eq!(
            amount(a.vat_output.as_str(), false),
            Money::from_euros(21, 0)
        );
        assert_eq!(amount("4100009", false), Money::from_euros(100, 0));
    }

    #[test]
    fn emitida_con_retencion() {
        // 1.000 + 21 % − 15 %: 430 1.060 y 473 150 / 705 1.000 y 477 210.
        let inv = invoice(vec![line(100_000, 2100, Some(1500), None)]);
        let t = compute_totals(&inv.lines, RoundingMode::PerRate);
        let a = PostingAccounts::default();
        let e = post_issued(&inv, &t, &a, &a.customers.subaccount(7, 1).unwrap(), 9);
        e.validate().unwrap();
        let get = |c: &str| {
            e.lines
                .iter()
                .find(|l| l.account.as_str() == c)
                .unwrap()
                .clone()
        };
        assert_eq!(get("4300001").debit, Money::from_euros(1060, 0));
        assert_eq!(get("473").debit, Money::from_euros(150, 0));
        assert_eq!(get("705").credit, Money::from_euros(1000, 0));
        assert_eq!(get("477").credit, Money::from_euros(210, 0));
        assert_eq!(e.origin, Origin::FacturaEmitida(9));
    }

    #[test]
    fn anticipo_y_factura_final() {
        let a = PostingAccounts::default();
        let customer = a.customers.subaccount(7, 1).unwrap();
        let advance = |cents| InvoiceLine {
            advance: true,
            ..line(cents, 2100, None, None)
        };
        let side = |e: &JournalEntry, c: &str| -> Money {
            e.lines
                .iter()
                .filter(|l| l.account.as_str() == c)
                .map(|l| l.debit - l.credit)
                .sum()
        };
        // Anticipo de 1.000 + 21 %: 430 1.210 / 438 1.000 y 477 210.
        let inv = invoice(vec![advance(100_000)]);
        let t = compute_totals(&inv.lines, RoundingMode::PerRate);
        let e = post_issued(&inv, &t, &a, &customer, 1);
        e.validate().unwrap();
        assert_eq!(side(&e, "438"), -Money::from_euros(1000, 0));
        assert_eq!(side(&e, "705"), Money::ZERO);
        // Final de 3.000 descontando el anticipo: 430 2.420 y 438 1.000 / 705 3.000 y 477 420.
        let inv = invoice(vec![line(300_000, 2100, None, None), advance(-100_000)]);
        let t = compute_totals(&inv.lines, RoundingMode::PerRate);
        let e = post_issued(&inv, &t, &a, &customer, 2);
        e.validate().unwrap();
        assert_eq!(side(&e, "4300001"), Money::from_euros(2420, 0));
        assert_eq!(side(&e, "438"), Money::from_euros(1000, 0));
        assert_eq!(side(&e, "705"), -Money::from_euros(3000, 0));
        assert_eq!(side(&e, "477"), -Money::from_euros(420, 0));
        // En moneda extranjera, la 438 por los euros del anticipo (909,09) y no por los de la
        // final (833,33): la diferencia, a ventas.
        let inv = invoice(vec![line(250_000, 0, None, None), advance(-83_333)]);
        let t = compute_totals(&inv.lines, RoundingMode::PerRate);
        let e =
            post_issued_with_advance(&inv, &t, &a, &customer, 3, Some(-Money::from_cents(90_909)));
        e.validate().unwrap();
        assert_eq!(side(&e, "438"), Money::from_cents(90_909));
        assert_eq!(side(&e, "705"), -Money::from_cents(257_576));
    }

    #[test]
    fn rectificativa_negativa_invierte_lados() {
        let inv = invoice(vec![line(-10_000, 2100, None, None)]);
        let t = compute_totals(&inv.lines, RoundingMode::PerRate);
        let a = PostingAccounts::default();
        let e = post_issued(&inv, &t, &a, &acc("4300001"), 1);
        e.validate().unwrap();
        assert_eq!(
            e.lines
                .iter()
                .find(|l| l.account.as_str() == "4300001")
                .unwrap()
                .credit,
            Money::from_euros(121, 0)
        );
        assert_eq!(
            e.lines
                .iter()
                .find(|l| l.account.as_str() == "705")
                .unwrap()
                .debit,
            Money::from_euros(100, 0)
        );
    }

    #[test]
    fn recibida_con_iva_parcialmente_deducible_y_retencion() {
        // Alquiler: 1.000 + 210 IVA − 190 retención; 50 % deducible.
        let r = ReceivedInvoice {
            id: 3,
            date: date!(2026 - 10 - 01),
            number: "F-1".into(),
            supplier_name: "Arrendador".into(),
            base: Money::from_euros(1000, 0),
            vat: Money::from_euros(210, 0),
            withholding: Money::from_euros(190, 0),
            deductible_pct: 50,
            expense_account: Some(acc("621")),
            self_assessed_vat: Money::ZERO,
            import_dua: false,
        };
        let a = PostingAccounts::default();
        let e = post_received(&r, &a, &acc("4100003"));
        e.validate().unwrap();
        let get = |c: &str| {
            e.lines
                .iter()
                .find(|l| l.account.as_str() == c)
                .unwrap()
                .clone()
        };
        assert_eq!(get("621").debit, Money::from_euros(1105, 0));
        assert_eq!(get("472").debit, Money::from_euros(105, 0));
        assert_eq!(get("4100003").credit, Money::from_euros(1020, 0));
        assert_eq!(get("4751").credit, Money::from_euros(190, 0));
    }

    proptest! {
        /// Cualquier factura emitida o recibida genera un asiento cuadrado.
        #[test]
        fn siempre_cuadra(
            items in prop::collection::vec((-1_000_000i64..5_000_000, prop::sample::select(vec![0, 400, 1000, 2100]),
                prop::option::of(prop::sample::select(vec![700, 1500, 1900])),
                prop::option::of(prop::sample::select(vec![50, 140, 520]))), 1..10),
            pct in 0u8..=100,
            per_line in any::<bool>(),
        ) {
            let lines: Vec<_> = items.iter().filter(|(c, ..)| *c != 0).map(|&(c, v, w, r)| line(c, v, w, r)).collect();
            prop_assume!(!lines.is_empty());
            let inv = invoice(lines);
            let t = compute_totals(&inv.lines, if per_line { RoundingMode::PerLine } else { RoundingMode::PerRate });
            let a = PostingAccounts::default();
            let e = post_issued(&inv, &t, &a, &acc("4300001"), 1);
            prop_assume!(e.lines.len() >= 2);
            prop_assert_eq!(e.validate(), Ok(()));
            let r = ReceivedInvoice {
                id: 1, date: inv.issue_date, number: "x".into(), supplier_name: "y".into(),
                base: t.base, vat: t.vat + t.surcharge, withholding: t.withholding, deductible_pct: pct, expense_account: None,
                self_assessed_vat: Money::ZERO,
                import_dua: false,
            };
            let e2 = post_received(&r, &a, &acc("4100001"));
            prop_assume!(e2.lines.len() >= 2);
            prop_assert_eq!(e2.validate(), Ok(()));
        }
    }
}
