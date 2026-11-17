//! Libros registro de IVA e IRPF en el formato electrónico común de la AEAT (diseño
//! `LSI.xlsx`, especificación V.18.02.2026; validaciones literales en
//! `fixtures/accounting/lsi-validaciones.md`).
//!
//! Hojas unificadas: EXPEDIDAS_INGRESOS (36 columnas, A–AJ) y RECIBIDAS_GASTOS (42 columnas,
//! A–AP). Una línea por tipo de IVA/recargo (validación 7), «Total Factura» por línea
//! (validación 37), retención en las líneas de base (nota 15) y columnas de IRPF vacías para
//! personas jurídicas (nota 17).

use serde::{Deserialize, Serialize};
use taxpain_core::invoice::{Invoice, InvoiceKind, PartyId, Totals, VatTreatment};
use taxpain_core::money::{Money, Rate};
use time::Date;

/// Celda del libro.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum Cell {
    Empty,
    Text(String),
    /// Importe con 2 decimales.
    Amount(Money),
    /// Tipo porcentual con 2 decimales.
    Rate(Rate),
    Int(i64),
    /// dd/mm/aaaa
    Date(Date),
}

/// Actividad del asiento (nota 16): código A/B/C/D, tipo de 2 posiciones y epígrafe IAE.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Activity {
    pub code: String,
    pub kind: String,
    pub iae: String,
}

/// Datos comunes del titular de los libros.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BookContext {
    pub activity: Activity,
    /// Persona física (lleva IRPF) o jurídica (columnas IRPF vacías).
    pub persona_fisica: bool,
}

pub const ISSUED_SHEET: &str = "EXPEDIDAS_INGRESOS";
pub const RECEIVED_SHEET: &str = "RECIBIDAS_GASTOS";
pub const ISSUED_COLUMNS: usize = 36;
pub const RECEIVED_COLUMNS: usize = 42;

fn quarter(d: Date) -> String {
    format!("{}T", (d.month() as u8 - 1) / 3 + 1)
}

fn text(s: impl Into<String>) -> Cell {
    let s = s.into();
    if s.is_empty() {
        Cell::Empty
    } else {
        Cell::Text(s)
    }
}

/// (Tipo, Código país, Identificación) del NIF (nota 2).
fn nif_cells(id: &PartyId) -> [Cell; 3] {
    match id {
        PartyId::Nif(n) => [Cell::Empty, Cell::Empty, text(n.clone())],
        PartyId::Foreign {
            country,
            id_type,
            id,
        } => [
            text(id_type.code()),
            text(country.clone()),
            text(id.clone()),
        ],
    }
}

/// (Calificación, Exenta, Tipo IVA) de una línea de desglose.
fn qualification(v: VatTreatment) -> (Cell, Cell, Rate) {
    match v {
        VatTreatment::Taxed { rate } => (text("S1"), Cell::Empty, rate),
        VatTreatment::ReverseCharge => (text("S2"), Cell::Empty, Rate::ZERO),
        VatTreatment::Exempt(e) => (Cell::Empty, text(format!("{e:?}")), Rate::ZERO),
        VatTreatment::NotSubject(n) => (text(format!("{n:?}")), Cell::Empty, Rate::ZERO),
    }
}

/// Retención (tipo, importe) de las líneas de la factura que caen en un grupo del desglose.
fn withholding_of(
    invoice: &Invoice,
    vat: VatTreatment,
    surcharge: Option<Rate>,
) -> Option<(Rate, Money)> {
    let mut rate = None;
    let mut amount = Money::ZERO;
    for l in invoice
        .lines
        .iter()
        .filter(|l| l.vat == vat && l.surcharge == surcharge)
    {
        if let Some(r) = l.withholding {
            rate.get_or_insert(r);
            amount += taxpain_core::invoice::totals::line_base(l).apply_rate(r);
        }
    }
    rate.map(|r| (r, amount))
}

/// Filas de una factura emitida en EXPEDIDAS_INGRESOS. `accrual`: fecha de devengo del IVA (la de
/// la operación si es anterior a la de expedición, art. 75 LIVA), que fija Ejercicio y Periodo.
pub fn issued_rows(
    invoice: &Invoice,
    totals: &Totals,
    accrual: Date,
    ctx: &BookContext,
) -> Vec<Vec<Cell>> {
    let op = invoice.operation_date.unwrap_or(invoice.issue_date);
    let [nif_type, nif_country, nif_id] = invoice
        .recipient
        .as_ref()
        .map(|r| nif_cells(&r.id))
        .unwrap_or([Cell::Empty, Cell::Empty, Cell::Empty]);
    let name = invoice
        .recipient
        .as_ref()
        .map(|r| r.name.chars().take(40).collect::<String>())
        .unwrap_or_default();
    let irpf = ctx.persona_fisica;
    totals
        .breakdown
        .iter()
        .map(|b| {
            let (calif, exenta, vat_rate) = qualification(b.vat);
            let clave = if matches!(
                b.vat,
                VatTreatment::Exempt(taxpain_core::invoice::Exemption::E2)
            ) {
                "02"
            } else {
                "01"
            };
            let wh = withholding_of(invoice, b.vat, b.surcharge_rate).filter(|_| irpf);
            // Ejercicio y periodo de la autoliquidación en que se declara: los de la fecha de
            // operación «y en su defecto la fecha de expedición» (validación 11 del LSI: una
            // factura del 3-4 por una operación del 30-3 va al 1T), como en el 303 de TaxPain.
            vec![
                Cell::Int(i64::from(accrual.year())),
                text(quarter(accrual)),
                text(ctx.activity.code.clone()),
                text(ctx.activity.kind.clone()),
                text(ctx.activity.iae.clone()),
                text(invoice.kind.code()),
                if irpf { text("I01") } else { Cell::Empty },
                if irpf {
                    Cell::Amount(b.base)
                } else {
                    Cell::Empty
                },
                Cell::Date(invoice.issue_date),
                if op != invoice.issue_date {
                    Cell::Date(op)
                } else {
                    Cell::Empty
                },
                Cell::Empty,
                text(invoice.number.chars().take(20).collect::<String>()),
                Cell::Empty,
                nif_type.clone(),
                nif_country.clone(),
                nif_id.clone(),
                text(name.clone()),
                text(clave),
                calif,
                exenta,
                Cell::Amount(b.base + b.vat_amount + b.surcharge_amount),
                Cell::Amount(b.base),
                Cell::Rate(vat_rate),
                Cell::Amount(b.vat_amount),
                b.surcharge_rate.map_or(Cell::Empty, Cell::Rate),
                if b.surcharge_rate.is_some() {
                    Cell::Amount(b.surcharge_amount)
                } else {
                    Cell::Empty
                },
                Cell::Empty,
                Cell::Empty,
                Cell::Empty,
                Cell::Empty,
                wh.map_or(Cell::Empty, |(r, _)| Cell::Rate(r)),
                wh.map_or(Cell::Empty, |(_, a)| Cell::Amount(a)),
                Cell::Empty,
                Cell::Empty,
                Cell::Empty,
                Cell::Empty,
            ]
        })
        .collect()
}

/// Factura recibida revisada (lo que se guarda tras la revisión humana).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceivedRecord {
    pub internal_number: String,
    pub supplier: PartyId,
    pub supplier_name: String,
    pub number: String,
    pub issue_date: Date,
    pub received_date: Date,
    pub base: Money,
    pub vat_rate: Rate,
    pub vat: Money,
    pub deductible_vat: Money,
    pub withholding_rate: Option<Rate>,
    pub withholding: Money,
    /// Concepto de gasto IRPF (G01…G48, nota 11).
    pub expense_concept: String,
    /// Bien de inversión (columna «Bien de Inversión», nota 20).
    #[serde(default)]
    pub capital_good: bool,
    /// Régimen de IVA de la compra. Con inversión del sujeto pasivo, `vat` y `deductible_vat`
    /// son la cuota autorrepercutida y la factura no lleva IVA.
    #[serde(default)]
    pub regime: ReceivedRegime,
}

/// Régimen de IVA de una recibida a efectos del libro (validaciones 7, 23 y 26 de LSI.xlsx).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReceivedRegime {
    /// Clave 01, inversión del sujeto pasivo «N».
    #[default]
    Interior,
    /// Clave 09 (adquisiciones intracomunitarias de bienes y servicios), ISP «N».
    Intracomunitaria,
    /// Clave 01 con inversión del sujeto pasivo «S» (servicios de empresas de fuera de la UE,
    /// ISP interior).
    Isp,
}

/// Concepto de gasto (nota 11) según la cuenta de gasto del PGC.
pub fn expense_concept(account: &str) -> &'static str {
    match account.get(..3).unwrap_or("") {
        "600" | "601" | "602" => "G01",
        "621" => "G12",
        "622" => "G13",
        "623" => "G19",
        "625" => "G20",
        "626" => "G24",
        "628" => "GY4",
        "631" | "634" => "G26",
        "640" => "G04",
        "642" => "G05",
        "662" | "663" => "G23",
        "680" => "G38",
        "681" => "GY8",
        _ => "G22",
    }
}

/// Filas de una factura recibida en RECIBIDAS_GASTOS.
pub fn received_rows(r: &ReceivedRecord, ctx: &BookContext) -> Vec<Vec<Cell>> {
    let [nif_type, nif_country, nif_id] = nif_cells(&r.supplier);
    let irpf = ctx.persona_fisica;
    let non_deductible = r.vat - r.deductible_vat;
    let kind = InvoiceKind::Completa.code();
    vec![vec![
        Cell::Int(i64::from(r.received_date.year())),
        text(quarter(r.received_date)),
        text(ctx.activity.code.clone()),
        text(ctx.activity.kind.clone()),
        text(ctx.activity.iae.clone()),
        text(kind),
        if irpf {
            text(r.expense_concept.clone())
        } else {
            Cell::Empty
        },
        if irpf {
            Cell::Amount(r.base + non_deductible)
        } else {
            Cell::Empty
        },
        Cell::Date(r.issue_date),
        Cell::Empty,
        text(r.number.chars().take(40).collect::<String>()),
        Cell::Empty,
        Cell::Date(r.received_date),
        text(r.internal_number.clone()),
        Cell::Empty,
        nif_type,
        nif_country,
        nif_id,
        text(r.supplier_name.chars().take(40).collect::<String>()),
        text(if r.regime == ReceivedRegime::Intracomunitaria {
            "09"
        } else {
            "01"
        }),
        text(if r.capital_good { "S" } else { "N" }),
        text(if r.regime == ReceivedRegime::Isp {
            "S"
        } else {
            "N"
        }),
        text("N"),
        Cell::Empty,
        Cell::Empty,
        // Con autorrepercusión la factura no lleva IVA: el total es la base.
        Cell::Amount(if r.regime == ReceivedRegime::Interior {
            r.base + r.vat
        } else {
            r.base
        }),
        Cell::Amount(r.base),
        Cell::Rate(r.vat_rate),
        Cell::Amount(r.vat),
        Cell::Amount(r.deductible_vat),
        Cell::Empty,
        Cell::Empty,
        Cell::Empty,
        Cell::Empty,
        Cell::Empty,
        Cell::Empty,
        if irpf {
            r.withholding_rate.map_or(Cell::Empty, Cell::Rate)
        } else {
            Cell::Empty
        },
        if irpf && !r.withholding.is_zero() {
            Cell::Amount(r.withholding)
        } else {
            Cell::Empty
        },
        Cell::Empty,
        Cell::Empty,
        Cell::Empty,
        Cell::Empty,
    ]]
}

/// Cabeceras de las filas 7 a 10 de la plantilla oficial (columna → texto).
pub fn issued_header() -> Vec<Vec<(usize, &'static str)>> {
    vec![
        vec![
            (0, "Autoliquidación"),
            (2, "Actividad"),
            (5, "Tipo de Factura"),
            (6, "Concepto de Ingreso"),
            (7, "Ingreso Computable"),
            (8, "Fecha Expedición"),
            (9, "Fecha Operación"),
            (10, "Identificación de la Factura"),
            (13, "NIF Destinatario"),
            (16, "Nombre Destinatario"),
            (17, "Clave de Operación"),
            (18, "Calificación de la Operación"),
            (19, "Operación Exenta"),
            (20, "Total Factura"),
            (21, "Base Imponible"),
            (22, "Tipo de IVA"),
            (23, "Cuota IVA Repercutida"),
            (24, "Tipo de Recargo Eq."),
            (25, "Cuota Recargo Eq."),
            (
                26,
                "Cobro (Operación Criterio de Caja de IVA y/o artículo 7.2.1º de Reglamento del IRPF)",
            ),
            (30, "Tipo Retención del IRPF"),
            (31, "Importe Retenido del IRPF"),
            (32, "Registro Acuerdo Facturación"),
            (33, "Inmueble"),
            (35, "Referencia Externa"),
        ],
        vec![
            (0, "Ejercicio"),
            (1, "Periodo"),
            (2, "Código"),
            (3, "Tipo"),
            (4, "Grupo o Epígrafe del IAE"),
            (10, "Serie"),
            (11, "Número"),
            (12, "Número-Final"),
            (13, "Tipo"),
            (14, "Código País"),
            (15, "Identificación"),
            (26, "Fecha"),
            (27, "Importe"),
            (28, "Medio Utilizado"),
            (29, "Identificación Medio Utilizado"),
            (33, "Situación"),
            (34, "Referencia Catastral"),
        ],
        vec![
            (0, "Decimal (4,0)"),
            (1, "Alfanumérico (2)"),
            (2, "Alfanumérico (1)"),
            (3, "Alfanumérico (2)"),
            (4, "Alfanumérico (4)"),
            (5, "Alfanumérico (2)"),
            (6, "Alfanumérico (3)"),
            (7, "Decimal(12,2)"),
            (8, "Fecha(dd/mm/yyyy)"),
            (9, "Fecha(dd/mm/yyyy)"),
            (10, "Alfanumérico (20)"),
            (11, "Alfanumérico (20)"),
            (12, "Alfanumérico (20)"),
            (13, "Alfanumérico (2)"),
            (14, "Alfanumérico (2)"),
            (15, "Alfanumérico (20)"),
            (16, "Alfanumérico (40)"),
            (17, "Alfanumérico (2)"),
            (18, "Alfanumérico (2)"),
            (19, "Alfanumérico (2)"),
            (20, "Decimal(12,2)"),
            (21, "Decimal(12,2)"),
            (22, "Decimal(4,2)"),
            (23, "Decimal(12,2)"),
            (24, "Decimal(4,2)"),
            (25, "Decimal(12,2)"),
            (26, "Fecha(dd/mm/yyyy)"),
            (27, "Decimal(12,2)"),
            (28, "Alfanumérico (2)"),
            (29, "Alfanumérico (34)"),
            (30, "Decimal(4,2)"),
            (31, "Decimal(12,2)"),
            (32, "Alfanumérico (15)"),
            (33, "Alfanumérico (1)"),
            (34, "Alfanumérico (20)"),
            (35, "Alfanumérico (40)"),
        ],
        vec![
            (6, "IRPF"),
            (7, "IRPF"),
            (17, "IVA"),
            (18, "IVA"),
            (19, "IVA"),
            (30, "IRPF"),
            (31, "IRPF"),
            (32, "IVA"),
        ],
    ]
}

pub fn received_header() -> Vec<Vec<(usize, &'static str)>> {
    vec![
        vec![
            (0, "Autoliquidación"),
            (2, "Actividad"),
            (5, "Tipo de Factura"),
            (6, "Concepto de Gasto"),
            (7, "Gasto Deducible"),
            (8, "Fecha Expedición"),
            (9, "Fecha Operación"),
            (10, "Identificación Factura del Expedidor"),
            (12, "Fecha Recepción"),
            (13, "Número Recepción"),
            (14, "Número Recepción Final"),
            (15, "NIF Expedidor"),
            (18, "Nombre Expedidor"),
            (19, "Clave de Operación"),
            (20, "Bien de Inversión"),
            (21, "Inversión del Sujeto Pasivo"),
            (22, "Deducible en Periodo Posterior"),
            (23, "Periodo Deducción"),
            (25, "Total Factura"),
            (26, "Base Imponible"),
            (27, "Tipo de IVA"),
            (28, "Cuota IVA Soportado"),
            (29, "Cuota Deducible"),
            (30, "Tipo de Recargo Eq."),
            (31, "Cuota Recargo Eq."),
            (
                32,
                "Pago (Operación Criterio de Caja de IVA y/o artículo 7.2.1º de Reglamento del IRPF)",
            ),
            (36, "Tipo Retención del IRPF"),
            (37, "Importe Retenido del IRPF"),
            (38, "Registro Acuerdo Facturación"),
            (39, "Inmueble"),
            (41, "Referencia Externa"),
        ],
        vec![
            (0, "Ejercicio"),
            (1, "Periodo"),
            (2, "Código"),
            (3, "Tipo"),
            (4, "Grupo o Epígrafe del IAE"),
            (10, "(Serie-Número)"),
            (11, "Número-Final"),
            (15, "Tipo"),
            (16, "Código País"),
            (17, "Identificación"),
            (23, "Ejercicio"),
            (24, "Periodo"),
            (32, "Fecha"),
            (33, "Importe"),
            (34, "Medio Utilizado"),
            (35, "Identificación Medio Utilizado"),
            (39, "Situación"),
            (40, "Referencia Catastral"),
        ],
        vec![
            (0, "Decimal (4,0)"),
            (1, "Alfanumérico (2)"),
            (2, "Alfanumérico (1)"),
            (3, "Alfanumérico (2)"),
            (4, "Alfanumérico (4)"),
            (5, "Alfanumérico (2)"),
            (6, "Alfanumérico (3)"),
            (7, "Decimal(12,2)"),
            (8, "Fecha(dd/mm/yyyy)"),
            (9, "Fecha(dd/mm/yyyy)"),
            (10, "Alfanumérico (40)"),
            (11, "Alfanumérico (20)"),
            (12, "Fecha(dd/mm/yyyy)"),
            (13, "Alfanumérico (20)"),
            (14, "Alfanumérico (20)"),
            (15, "Alfanumérico (2)"),
            (16, "Alfanumérico (2)"),
            (17, "Alfanumérico (20)"),
            (18, "Alfanumérico (40)"),
            (19, "Alfanumérico (2)"),
            (20, "Alfanumérico (1)"),
            (21, "Alfanumérico (1)"),
            (22, "Alfanumérico (1)"),
            (23, "Decimal (4,0)"),
            (24, "Alfanumérico (2)"),
            (25, "Decimal(12,2)"),
            (26, "Decimal(12,2)"),
            (27, "Decimal(4,2)"),
            (28, "Decimal(12,2)"),
            (29, "Decimal(12,2)"),
            (30, "Decimal(4,2)"),
            (31, "Decimal(12,2)"),
            (32, "Fecha(dd/mm/yyyy)"),
            (33, "Decimal(12,2)"),
            (34, "Alfanumérico (2)"),
            (35, "Alfanumérico (34)"),
            (36, "Decimal(4,2)"),
            (37, "Decimal(12,2)"),
            (38, "Alfanumérico (15)"),
            (39, "Alfanumérico (1)"),
            (40, "Alfanumérico (20)"),
            (41, "Alfanumérico (40)"),
        ],
        vec![
            (6, "IRPF"),
            (7, "IRPF"),
            (12, "IVA"),
            (19, "IVA"),
            (20, "IVA"),
            (21, "IVA"),
            (22, "IVA"),
            (23, "IVA"),
            (24, "IVA"),
            (36, "IRPF"),
            (37, "IRPF"),
            (38, "IVA"),
        ],
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use taxpain_core::invoice::{Exemption, InvoiceLine, Party, RoundingMode, compute_totals};
    use time::macros::date;

    fn ctx(fisica: bool) -> BookContext {
        BookContext {
            activity: Activity {
                code: "A".into(),
                kind: "05".into(),
                iae: "0742".into(),
            },
            persona_fisica: fisica,
        }
    }

    fn line(cents: i64, vat: VatTreatment, wh: Option<i32>) -> InvoiceLine {
        InvoiceLine {
            description: "x".into(),
            quantity_milli: 1000,
            unit_price: Money::from_cents(cents),
            discount: Rate::ZERO,
            vat,
            surcharge: None,
            withholding: wh.map(Rate::percent),
            advance: false,
        }
    }

    fn invoice() -> Invoice {
        Invoice {
            kind: InvoiceKind::Completa,
            number: "A2026-0001".into(),
            issue_date: date!(2026 - 05 - 10),
            operation_date: None,
            issuer: Party {
                id: PartyId::Nif("12345678Z".into()),
                name: "Yo".into(),
                address: None,
                person: None,
            },
            recipient: Some(Party {
                id: PartyId::Nif("B12345674".into()),
                name: "Cliente SL".into(),
                address: None,
                person: None,
            }),
            lines: vec![
                line(
                    100_000,
                    VatTreatment::Taxed {
                        rate: Rate::percent(21),
                    },
                    Some(15),
                ),
                line(20_000, VatTreatment::Exempt(Exemption::E1), None),
            ],
            rectified: vec![],
            description: "x".into(),
            notes: None,
        }
    }

    #[test]
    fn expedidas_persona_fisica() {
        let f = invoice();
        let t = compute_totals(&f.lines, RoundingMode::PerRate);
        let rows = issued_rows(&f, &t, f.issue_date, &ctx(true));
        assert_eq!(rows.len(), 2, "una línea por tipo");
        assert!(rows.iter().all(|r| r.len() == ISSUED_COLUMNS));
        let taxed = rows.iter().find(|r| r[18] == text("S1")).unwrap();
        assert_eq!(taxed[0], Cell::Int(2026));
        assert_eq!(taxed[1], text("2T"));
        assert_eq!(taxed[6], text("I01"));
        assert_eq!(taxed[15], text("B12345674"));
        assert_eq!(
            taxed[20],
            Cell::Amount(Money::from_euros(1210, 0)),
            "total por línea = base + cuota"
        );
        assert_eq!(taxed[22], Cell::Rate(Rate::percent(21)));
        assert_eq!(taxed[30], Cell::Rate(Rate::percent(15)));
        assert_eq!(taxed[31], Cell::Amount(Money::from_euros(150, 0)));
        let exempt = rows.iter().find(|r| r[19] == text("E1")).unwrap();
        assert_eq!(exempt[18], Cell::Empty);
        assert_eq!(
            exempt[20],
            Cell::Amount(Money::from_euros(200, 0)),
            "exenta: total = base"
        );
        assert_eq!(exempt[30], Cell::Empty);
    }

    #[test]
    fn expedidas_con_fecha_de_operacion_anterior_van_a_su_trimestre() {
        // Operación del 30/3 expedida el 3/4: periodo 1T (validación 11), con las dos fechas.
        let mut f = invoice();
        f.issue_date = date!(2026 - 04 - 03);
        f.operation_date = Some(date!(2026 - 03 - 30));
        let t = compute_totals(&f.lines, RoundingMode::PerRate);
        let rows = issued_rows(&f, &t, date!(2026 - 03 - 30), &ctx(true));
        assert_eq!(rows[0][0], Cell::Int(2026));
        assert_eq!(rows[0][1], text("1T"));
        assert_eq!(rows[0][8], Cell::Date(date!(2026 - 04 - 03)));
        assert_eq!(rows[0][9], Cell::Date(date!(2026 - 03 - 30)));
        // Y en el cambio de año, al ejercicio de la operación.
        f.issue_date = date!(2027 - 01 - 05);
        f.operation_date = Some(date!(2026 - 12 - 28));
        let rows = issued_rows(&f, &t, date!(2026 - 12 - 28), &ctx(true));
        assert_eq!((&rows[0][0], &rows[0][1]), (&Cell::Int(2026), &text("4T")));
    }

    #[test]
    fn expedidas_persona_juridica_sin_irpf() {
        let f = invoice();
        let t = compute_totals(&f.lines, RoundingMode::PerRate);
        for r in issued_rows(&f, &t, f.issue_date, &ctx(false)) {
            assert_eq!(
                (&r[6], &r[7], &r[30], &r[31]),
                (&Cell::Empty, &Cell::Empty, &Cell::Empty, &Cell::Empty)
            );
        }
    }

    #[test]
    fn recibidas() {
        let r = ReceivedRecord {
            internal_number: "R-7".into(),
            supplier: PartyId::Nif("B12345674".into()),
            supplier_name: "Arrendador SL".into(),
            number: "F-12".into(),
            issue_date: date!(2026 - 09 - 30),
            received_date: date!(2026 - 10 - 02),
            base: Money::from_euros(1000, 0),
            vat_rate: Rate::percent(21),
            vat: Money::from_euros(210, 0),
            deductible_vat: Money::from_euros(105, 0),
            withholding_rate: Some(Rate::percent(19)),
            withholding: Money::from_euros(190, 0),
            expense_concept: expense_concept("621").into(),
            capital_good: false,
            regime: ReceivedRegime::Interior,
        };
        let rows = received_rows(&r, &ctx(true));
        assert_eq!(rows[0].len(), RECEIVED_COLUMNS);
        assert_eq!(rows[0][1], text("4T"));
        assert_eq!(rows[0][6], text("G12"));
        assert_eq!(
            rows[0][7],
            Cell::Amount(Money::from_euros(1105, 0)),
            "gasto + IVA no deducible"
        );
        assert_eq!(rows[0][29], Cell::Amount(Money::from_euros(105, 0)));
        assert_eq!(rows[0][37], Cell::Amount(Money::from_euros(190, 0)));
        assert_eq!(expense_concept("629"), "G22");
        assert_eq!(
            (&rows[0][19], &rows[0][20], &rows[0][21]),
            (&text("01"), &text("N"), &text("N"))
        );
        // Adquisición intracomunitaria: clave 09, ISP «N», total = base, cuota autorrepercutida.
        let ue = ReceivedRecord {
            supplier: PartyId::Foreign {
                country: "IE".into(),
                id_type: taxpain_core::invoice::IdType::NifIva,
                id: "1234567T".into(),
            },
            vat: Money::from_euros(210, 0),
            deductible_vat: Money::from_euros(210, 0),
            withholding_rate: None,
            withholding: Money::ZERO,
            capital_good: true,
            regime: ReceivedRegime::Intracomunitaria,
            ..r.clone()
        };
        let row = &received_rows(&ue, &ctx(true))[0];
        assert_eq!((&row[15], &row[16]), (&text("02"), &text("IE")));
        assert_eq!(
            (&row[19], &row[20], &row[21]),
            (&text("09"), &text("S"), &text("N"))
        );
        assert_eq!(row[25], Cell::Amount(Money::from_euros(1000, 0)));
        assert_eq!(row[28], Cell::Amount(Money::from_euros(210, 0)));
        // Otra inversión del sujeto pasivo: clave 01 con ISP «S».
        let isp = ReceivedRecord {
            regime: ReceivedRegime::Isp,
            capital_good: false,
            ..ue
        };
        let row = &received_rows(&isp, &ctx(true))[0];
        assert_eq!((&row[19], &row[21]), (&text("01"), &text("S")));
        let h = received_header();
        assert!(h.iter().flatten().all(|(c, _)| *c < RECEIVED_COLUMNS));
        assert!(
            issued_header()
                .iter()
                .flatten()
                .all(|(c, _)| *c < ISSUED_COLUMNS)
        );
    }
}
