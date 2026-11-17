//! Libros registro de IVA del gestor (Excel, OpenDocument o CSV) → libros de facturas de
//! TaxPain, para los años anteriores a usar la app.
//!
//! - Se reconoce el libro por sus columnas (fecha, número, NIF o nombre, base, tipo o cuota) con
//!   los nombres habituales: el formato de libros registro de la AEAT, a3, Sage, ContaSOL…
//! - Una factura con varios tipos de IVA ocupa varias filas con el mismo número: se juntan.
//! - Las facturas entran **sin asiento**: la contabilidad de esos años viene del diario del
//!   gestor (si no, se contaría dos veces). Las emitidas tampoco generan registro VERI*FACTU:
//!   no se expiden ahora, ya se expidieron.
//!
//! `TODO(VERIFY)`: las facturas al 0 % se tratan como exentas (art. 20 LIVA) o, con cliente
//! extranjero, como no sujetas por reglas de localización; revisa las que salgan con aviso.

use std::collections::BTreeMap;

use serde::Serialize;
use sha2::{Digest, Sha256};
use taxpain_core::Nif;
use taxpain_core::invoice::{
    Exemption, Invoice, InvoiceKind, InvoiceLine, NotSubject, Party, PartyId, Rectification,
    RectificationMethod, Series, TaxBreakdown, Totals, VatTreatment,
};
use taxpain_core::money::{Money, Rate};
use taxpain_vault::Vault;
use taxpain_vault::audit::{self, Actor};
use taxpain_vault::rusqlite::{Connection, OptionalExtension, params};
use time::{Date, OffsetDateTime};

use crate::error::{AppError, Result};
use crate::import::{Hoja, año_de_cabecera, norm, parse_amount, parse_date, parse_date_in};
use crate::invoicing::now_str;

/// Libro de facturas emitidas (expedidas) o recibidas.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BookKind {
    Emitidas,
    Recibidas,
}

/// Una línea de IVA de una factura del libro.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BookLine {
    pub base: Money,
    /// Tipo en centésimas de punto (2100 = 21 %).
    pub rate_bp: i32,
    pub vat: Money,
    pub surcharge: Money,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BookInvoice {
    pub number: String,
    pub issue_date: Date,
    /// Fecha de registro o recepción (en recibidas, decide el periodo de la deducción, art. 99
    /// LIVA); si el libro no la tiene, la de la factura.
    pub registered: Date,
    /// NIF del cliente o del proveedor (sin país), si lo hay.
    pub nif: Option<String>,
    /// País (ISO alfa-2) si no es España.
    pub country: Option<String>,
    pub name: String,
    pub lines: Vec<BookLine>,
    /// Retención, con el signo de la base (negativa en las rectificativas).
    pub withholding: Money,
    pub total: Money,
    /// El total del libro incluye la cuota de IVA (`Some(false)`: es una compra con inversión
    /// del sujeto pasivo y la cuota del libro es la autorrepercutida; `None`: el libro no trae
    /// total o no cuadra).
    pub vat_in_total: Option<bool>,
    /// Porcentaje del IVA que se deduce: 100, o 0 en las facturas de la hoja de IVA no
    /// deducible del libro de recibidas.
    pub deductible_pct: u8,
    /// En un libro de emitidas: no es una venta, sino la autorrepercusión de una compra
    /// (inversión del sujeto pasivo o adquisición intracomunitaria, arts. 84.Uno.2º y 85 LIVA),
    /// que la gestoría apunta también en emitidas para declarar su IVA. Se sabe porque lleva
    /// cuota y el total del libro no la incluye, porque va en una hoja de adquisiciones, inversión
    /// del sujeto pasivo o autorrepercusión con cuotas ([`parse_hojas`]) o porque la misma factura
    /// (número y proveedor) está en el libro de recibidas ([`mark_self_assessed`]).
    pub self_assessed: bool,
}

impl BookInvoice {
    pub fn base(&self) -> Money {
        self.lines.iter().fold(Money::ZERO, |a, l| a + l.base)
    }
    pub fn vat(&self) -> Money {
        self.lines.iter().fold(Money::ZERO, |a, l| a + l.vat)
    }
    pub fn surcharge(&self) -> Money {
        self.lines.iter().fold(Money::ZERO, |a, l| a + l.surcharge)
    }
}

/// Libro leído.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Book {
    pub kind: BookKind,
    pub invoices: Vec<BookInvoice>,
    pub warnings: Vec<String>,
}

#[derive(Default, Debug)]
struct Cols {
    date: Option<usize>,
    op_date: Option<usize>,
    reg_date: Option<usize>,
    series: Option<usize>,
    number: Option<usize>,
    nif: Option<usize>,
    country: Option<usize>,
    name: Option<usize>,
    base: Option<usize>,
    /// Tipo de IVA (%).
    rate: Option<usize>,
    /// Cuota de IVA (importe).
    vat: Option<usize>,
    /// La columna de la cuota se llama «IVA» a secas y no hay columna de tipo: en cada fila se
    /// mira con la base si trae la cuota o el tipo.
    vat_ambiguous: bool,
    /// Cuota y tipo del recargo de equivalencia.
    surcharge: Option<usize>,
    surcharge_rate: Option<usize>,
    /// Importe y tipo de la retención.
    withholding: Option<usize>,
    withholding_rate: Option<usize>,
    total: Option<usize>,
    /// Porcentaje deducible (la columna «% deducible» que se pone al juntar las hojas).
    deductible: Option<usize>,
}

impl Cols {
    fn usable(&self) -> bool {
        self.date.is_some()
            && self.number.is_some()
            && self.base.is_some()
            && (self.vat.is_some() || self.rate.is_some())
            && (self.nif.is_some() || self.name.is_some())
    }
}

fn set(slot: &mut Option<usize>, i: usize) {
    if slot.is_none() {
        *slot = Some(i);
    }
}

/// Columnas de una fila de cabecera. El `%` del título (que `norm` quita) separa los tipos de
/// los importes: «% IVA» o «Tipo IVA» es el tipo; «Cuota IVA», «IVA €» o «Importe IVA», la
/// cuota; «% IRPF», el tipo de la retención, e «IRPF» o «Retención», su importe. «IVA» a secas
/// es la cuota si no hay otra columna de cuota (y el tipo si la hay).
fn columns(row: &[String]) -> Cols {
    let mut c = Cols::default();
    let mut plain_iva = None;
    for (i, cell) in row.iter().enumerate() {
        let h = norm(cell);
        if h.is_empty() {
            continue;
        }
        let raw = cell.to_lowercase();
        let words: Vec<String> = raw
            .replace('.', "")
            .split(|ch: char| !ch.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(norm)
            .collect();
        let has = |s: &str| h.contains(s);
        let word = |s: &str| words.iter().any(|w| w == s);
        let pct = raw.contains('%') || has("porcentaje") || has("tipo") || has("tasa");
        let amount =
            raw.contains('€') || has("cuota") || has("importe") || word("eur") || word("euros");
        if h == "deducible" && pct {
            set(&mut c.deductible, i);
        } else if has("fecha") {
            if has("operacion") || has("devengo") {
                set(&mut c.op_date, i);
            } else if has("registro") || has("contab") || has("recepcion") {
                set(&mut c.reg_date, i);
            } else if !has("pago") && !has("cobro") {
                set(&mut c.date, i);
            }
        } else if has("serie") && !has("factura") {
            set(&mut c.series, i);
        } else if (has("factura")
            || h == "numero"
            || h == "num"
            || has("documento")
            || h == "nfra"
            || h == "numfact"
            || h == "nfact")
            && !has("total")
            && !has("tipo")
            && !has("importe")
            && !has("rectific")
        {
            set(&mut c.number, i);
        } else if (has("nif") || has("cif") || has("identificacion")) && !has("tipo") {
            set(&mut c.nif, i);
        } else if has("pais") {
            set(&mut c.country, i);
        } else if has("nombre")
            || has("razonsocial")
            || h == "cliente"
            || h == "proveedor"
            || h == "tercero"
            || h == "destinatario"
            || h == "expedidor"
        {
            set(&mut c.name, i);
        } else if has("base") {
            set(&mut c.base, i);
        } else if has("recargo") || word("re") || word("req") {
            if pct && !amount {
                set(&mut c.surcharge_rate, i);
            } else {
                set(&mut c.surcharge, i);
            }
        } else if has("retencion") || has("irpf") || word("ret") {
            if pct && !amount {
                set(&mut c.withholding_rate, i);
            } else {
                set(&mut c.withholding, i);
            }
        } else if (has("cuota") && !has("nodeducible"))
            || (has("iva") && amount)
            || h == "totaliva"
            || (h.starts_with("iva")
                && ["repercut", "soportad", "deducible", "devengad"]
                    .iter()
                    .any(|w| has(w)))
        {
            set(&mut c.vat, i);
        } else if pct && (has("iva") || has("impositivo") || h == "tipo" || h == "porcentaje") {
            set(&mut c.rate, i);
        } else if h == "iva" {
            plain_iva.get_or_insert(i);
        } else if has("total") {
            set(&mut c.total, i);
        }
    }
    if let Some(i) = plain_iva {
        if c.vat.is_none() {
            c.vat = Some(i);
            c.vat_ambiguous = c.rate.is_none();
        } else {
            set(&mut c.rate, i);
        }
    }
    c
}

/// Libro emitidas o recibidas según las cabeceras, el texto de encima o el nombre del fichero.
fn direction(filename: &str, rows: &[Vec<String>], header: usize) -> Option<BookKind> {
    let mut text = norm(filename);
    for r in rows.iter().take(header + 1) {
        for c in r {
            text.push(' ');
            text.push_str(&norm(c));
        }
    }
    let issued = [
        "destinatario",
        "cliente",
        "repercutid",
        "expedidas",
        "emitid",
        "ventas",
        "ingresos",
    ];
    let received = [
        "expedidor",
        "proveedor",
        "soportad",
        "deducible",
        "recibid",
        "compras",
        "gastos",
    ];
    let a = issued.iter().filter(|w| text.contains(*w)).count();
    let b = received.iter().filter(|w| text.contains(*w)).count();
    match a.cmp(&b) {
        std::cmp::Ordering::Greater => Some(BookKind::Emitidas),
        std::cmp::Ordering::Less => Some(BookKind::Recibidas),
        std::cmp::Ordering::Equal => None,
    }
}

/// Tipos de IVA y de recargo conocidos (para redondear el tipo calculado con base y cuota).
const KNOWN_RATES: [i32; 11] = [0, 50, 140, 175, 400, 500, 520, 1000, 1050, 1200, 2100];

/// ¿Es un tipo de IVA o de recargo de equivalencia conocido?
pub(crate) fn known_rate(bp: i32) -> bool {
    KNOWN_RATES.contains(&bp)
}

/// Tipo de una celda («21», «21,00 %», «0,21»), en centésimas de punto. `None` si está vacía o
/// no puede ser un tipo (más del 100 %: será un importe).
fn cell_rate(cell: &str) -> Option<i32> {
    if cell.trim().is_empty() {
        return None;
    }
    let c = parse_amount(&cell.replace('%', ""))?.cents();
    if !(0..=10_000).contains(&c) {
        return None;
    }
    let c = i32::try_from(c).ok()?;
    // «0,21» es una fracción (21 %); «21», un porcentaje. Por debajo de 1 se lee como fracción
    // salvo que sea un tipo conocido tal cual («0,5» de recargo).
    Some(if c < 100 && (c == 0 || !known_rate(c)) {
        c * 100
    } else {
        c
    })
}

/// Tipo calculado con la base y la cuota, redondeado a un tipo conocido si está a menos de
/// 0,3 puntos.
fn computed_rate(base: Money, vat: Money) -> i32 {
    if base.is_zero() {
        return 0;
    }
    let exact = (i128::from(vat.cents()) * 10_000 + i128::from(base.cents()) / 2)
        / i128::from(base.cents());
    let exact = i32::try_from(exact).unwrap_or(0);
    KNOWN_RATES
        .iter()
        .copied()
        .find(|r| (r - exact).abs() <= 30)
        .unwrap_or(exact)
}

/// Tipo a partir de una celda («21», «21,00 %», «0,21») o, si falta o es 0, de la base y la
/// cuota.
pub(crate) fn rate_bp(cell: Option<&str>, base: Money, vat: Money) -> i32 {
    match cell.and_then(cell_rate) {
        Some(bp) if bp != 0 => bp,
        _ => computed_rate(base, vat),
    }
}

/// ¿Cuadran base × tipo y la cuota (al céntimo, con 2 de margen por redondeos)?
fn consistent(base: Money, bp: i32, vat: Money) -> bool {
    computed_rate(base, vat) == bp || (base.apply_rate(Rate::from_bp(bp)) - vat).cents().abs() <= 2
}

/// Tipo y cuota de una fila. Manda la cuota del libro: si el tipo no cuadra con ella (o no es un
/// tipo, sino un importe), el tipo se calcula con la base y la cuota y se avisa.
pub(crate) fn line_rate_vat(
    base: Money,
    rate_cell: Option<&str>,
    vat_cell: Option<&str>,
    ambiguous: bool,
    number: &str,
    warnings: &mut Vec<String>,
) -> (i32, Money) {
    let rate_cell = rate_cell.filter(|s| !s.trim().is_empty());
    let vat = vat_cell
        .filter(|s| !s.trim().is_empty())
        .and_then(parse_amount);
    match (vat, rate_cell) {
        (Some(vat), Some(rc)) => match cell_rate(rc) {
            Some(bp) if consistent(base, bp, vat) => (bp, vat),
            // Texto («Exento») o un importe en la columna del tipo: el de la cuota, sin más.
            None => (computed_rate(base, vat), vat),
            Some(_) => {
                let calc = computed_rate(base, vat);
                if !base.is_zero() {
                    warnings.push(format!(
                        "{number}: el tipo del libro ({}) no cuadra con la base y la cuota; se \
                         toma el {} % que sale de la cuota",
                        rc.trim(),
                        Money::from_cents(i64::from(calc))
                    ));
                }
                (calc, vat)
            }
        },
        (Some(vat), None) => {
            // «IVA» a secas con el tipo dentro («21») en vez de la cuota: si como cuota no da
            // un tipo conocido y como tipo sí, es el tipo.
            if ambiguous
                && !known_rate(computed_rate(base, vat))
                && let Some(bp) = vat_cell.and_then(cell_rate).filter(|bp| known_rate(*bp))
            {
                return (bp, base.apply_rate(Rate::from_bp(bp)));
            }
            (computed_rate(base, vat), vat)
        }
        (None, Some(rc)) => match cell_rate(rc) {
            Some(bp) => (bp, base.apply_rate(Rate::from_bp(bp))),
            // Más del 100 %: no es el tipo, es la cuota.
            None => {
                let vat = parse_amount(rc).unwrap_or(Money::ZERO);
                (computed_rate(base, vat), vat)
            }
        },
        (None, None) => (0, Money::ZERO),
    }
}

/// Importe de una columna de importe o, si sólo hay la del tipo, base × tipo.
fn amount_or_rate(base: Money, amount: Option<Money>, rate_cell: Option<&str>) -> Money {
    match (amount, rate_cell.and_then(cell_rate)) {
        (Some(m), _) => m,
        (None, Some(bp)) => base.apply_rate(Rate::from_bp(bp)),
        (None, None) => Money::ZERO,
    }
}

/// Busca la cabecera en las primeras filas y lee el libro. `None` si no parece un libro
/// registro de facturas.
pub fn parse_rows(filename: &str, rows: &[Vec<String>]) -> Option<Book> {
    let (header, cols) = rows
        .iter()
        .take(25)
        .enumerate()
        .map(|(i, r)| (i, columns(r)))
        .find(|(_, c)| c.usable())?;
    let kind = direction(filename, rows, header)?;
    // Fechas sin año («dd/mm»): el del periodo de la cabecera del listado.
    let año = año_de_cabecera(&rows[..header]);
    let parse_date = |s: &str| parse_date_in(s, año);
    let cell = |r: &[String], i: Option<usize>| -> Option<String> {
        i.and_then(|i| r.get(i)).map(|s| s.trim().to_owned())
    };
    let money = |r: &[String], i: Option<usize>| -> Option<Money> {
        cell(r, i)
            .filter(|s| !s.is_empty())
            .and_then(|s| parse_amount(&s))
    };
    let mut warnings = Vec::new();
    let mut skipped = 0usize;
    // Una factura con varios tipos ocupa varias filas: número, tercero y año (la numeración
    // puede empezar de nuevo cada año).
    let mut groups: BTreeMap<(String, String, i32), BookInvoice> = BTreeMap::new();
    let mut order: Vec<(String, String, i32)> = Vec::new();
    for r in &rows[header + 1..] {
        let Some(date) = cell(r, cols.date).as_deref().and_then(parse_date) else {
            if r.iter().any(|c| !c.trim().is_empty()) {
                skipped += 1;
            }
            continue;
        };
        let number = {
            let n = cell(r, cols.number).unwrap_or_default();
            match cell(r, cols.series).filter(|s| !s.is_empty() && !n.starts_with(s.as_str())) {
                Some(s) => format!("{s}{n}"),
                None => n,
            }
        };
        if number.is_empty() {
            skipped += 1;
            continue;
        }
        let raw_country = cell(r, cols.country).unwrap_or_default();
        let (nif, country) =
            normalize_nif(&cell(r, cols.nif).unwrap_or_default(), Some(&raw_country));
        let bad_country = raw_country.trim().to_ascii_uppercase();
        if !bad_country.is_empty()
            && bad_country != "ES"
            && bad_country != "EL"
            && !taxpain_core::iso3166::is_country(&bad_country)
            && !taxpain_core::eu::EU_COUNTRIES.contains(&bad_country.as_str())
        {
            warnings.push(format!(
                "{number}: el país «{}» no es un código ISO 3166-1 (dos letras, como FR o US): se                  ignora",
                raw_country.trim()
            ));
        }
        let name = cell(r, cols.name).unwrap_or_default();
        let base = money(r, cols.base).unwrap_or(Money::ZERO);
        let (rate_bp, vat) = line_rate_vat(
            base,
            cell(r, cols.rate).as_deref(),
            cell(r, cols.vat).as_deref(),
            cols.vat_ambiguous,
            &number,
            &mut warnings,
        );
        let line = BookLine {
            base,
            rate_bp,
            vat,
            surcharge: amount_or_rate(
                base,
                money(r, cols.surcharge),
                cell(r, cols.surcharge_rate).as_deref(),
            ),
        };
        let key = (
            number.clone(),
            nif.clone().unwrap_or_else(|| norm(&name)),
            date.year(),
        );
        let total = money(r, cols.total).unwrap_or(Money::ZERO);
        // La retención con el signo de la base (negativa en las rectificativas).
        let withholding = amount_or_rate(
            base,
            money(r, cols.withholding),
            cell(r, cols.withholding_rate).as_deref(),
        )
        .abs();
        let withholding = if base.is_negative() {
            -withholding
        } else {
            withholding
        };
        match groups.get_mut(&key) {
            Some(inv) => {
                inv.lines.push(line);
                inv.withholding += withholding;
                if inv.total.is_zero() {
                    inv.total = total;
                }
            }
            None => {
                order.push(key.clone());
                groups.insert(
                    key,
                    BookInvoice {
                        number,
                        issue_date: date,
                        registered: cell(r, cols.reg_date)
                            .as_deref()
                            .and_then(parse_date)
                            .unwrap_or(date),
                        nif,
                        country,
                        name,
                        lines: vec![line],
                        withholding,
                        total,
                        vat_in_total: None,
                        deductible_pct: cell(r, cols.deductible)
                            .and_then(|p| p.parse::<u8>().ok())
                            .filter(|p| *p <= 100)
                            .unwrap_or(100),
                        self_assessed: false,
                    },
                );
            }
        }
    }
    let mut invoices: Vec<BookInvoice> = order
        .into_iter()
        .filter_map(|k| groups.remove(&k))
        .collect();
    for inv in &mut invoices {
        let computed = inv.base() + inv.vat() + inv.surcharge() - inv.withholding;
        let with_tax = inv.base() + inv.vat() + inv.surcharge();
        let without_vat = inv.base() + inv.surcharge() - inv.withholding;
        // El total del libro puede ser con o sin la retención restada; si no cuadra con
        // ninguno, se avisa y se usa el calculado. Un total sin la cuota es una compra con
        // inversión del sujeto pasivo (la cuota del libro es la autorrepercutida): en recibidas,
        // la compra; en emitidas, su autorrepercusión, que no es una venta.
        if inv.total.is_zero() {
            inv.total = computed;
        } else if inv.total == computed || inv.total == with_tax {
            inv.vat_in_total = Some(true);
            inv.total = computed;
        } else if !inv.vat().is_zero()
            && (inv.total == without_vat || inv.total == inv.base() + inv.surcharge())
        {
            inv.vat_in_total = Some(false);
            inv.total = without_vat;
            inv.self_assessed = kind == BookKind::Emitidas;
        } else {
            warnings.push(format!(
                "{}: el total del libro ({}) no cuadra con base + cuotas − retención ({})",
                inv.number, inv.total, computed
            ));
            inv.total = computed;
        }
    }
    if skipped > 0 {
        warnings.push(format!(
            "{skipped} fila(s) sin fecha o sin número no se leen (totales, títulos…)"
        ));
    }
    if invoices.is_empty() {
        return None;
    }
    Some(Book {
        kind,
        invoices,
        warnings,
    })
}

/// NIF de un libro o de una lectura: (NIF normalizado, país si no es España). Un NIF español con
/// o sin «ES» delante queda sin él («B12345674»); un NIF-IVA de otro país queda con su prefijo
/// («FR12345678901») y da el país; con la columna de país y un identificador sin prefijo, se le
/// pone el prefijo.
pub(crate) fn normalize_nif(raw: &str, country: Option<&str>) -> (Option<String>, Option<String>) {
    // País ISO («GR» aunque el libro ponga el prefijo del NIF-IVA, «EL»; «XI» es Irlanda del
    // Norte en VIES). Un código que no es de ningún país (ISO 3166-1) se ignora: no se le
    // inventa un prefijo al NIF.
    let mut country = country
        .map(|c| c.trim().to_ascii_uppercase())
        .filter(|c| c.len() == 2 && c != "ES")
        .map(|c| if c == "EL" { "GR".to_owned() } else { c })
        .filter(|c| taxpain_core::iso3166::is_country(c) || c == "XI");
    let compact: String = raw
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_uppercase();
    if compact.is_empty() {
        return (None, country);
    }
    if let Ok(n) = Nif::parse(&compact) {
        return (Some(n.as_str().to_owned()), country);
    }
    if let Some(rest) = compact.strip_prefix("ES")
        && let Ok(n) = Nif::parse(rest)
    {
        return (Some(n.as_str().to_owned()), None);
    }
    // Un texto en la columna del NIF («VARIOS», «SIN NIF») no es un identificador: los tickets
    // (facturas simplificadas) no llevan el NIF del destinatario (art. 6.1.d) y 7 RD 1619/2012)
    // y el libro los registra sin él. Tomarlo por un NIF-IVA inventaría el país (VA, SI…).
    if !compact.bytes().any(|b| b.is_ascii_digit()) {
        return (None, country);
    }
    // NIF-IVA de otro país («FR12345678901»; Grecia: país «GR», prefijo «EL»).
    let prefixed = compact.len() > 2 && compact[..2].bytes().all(|b| b.is_ascii_alphabetic());
    match &country {
        None if prefixed => {
            country = Some(match &compact[..2] {
                "EL" => "GR".to_owned(),
                c => c.to_owned(),
            });
        }
        Some(c) => {
            let p = taxpain_core::eu::vies_prefix(c);
            if !compact.starts_with(p.as_str()) {
                return (Some(format!("{p}{compact}")), country);
            }
        }
        _ => {}
    }
    (Some(compact), country)
}

/// Lee un fichero de hoja de cálculo o CSV como libro registro, con todas sus hojas.
pub fn parse(filename: &str, bytes: &[u8]) -> Result<Option<Book>> {
    Ok(parse_hojas(
        filename,
        &crate::import::sheets(filename, bytes)?,
    ))
}

/// Qué es una hoja de un libro registro, por su nombre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClaseHoja {
    /// Facturas normales (o un mes, o una serie), también las entregas o ventas
    /// intracomunitarias.
    Normal,
    /// Adquisiciones intracomunitarias, inversión del sujeto pasivo, importaciones: en
    /// recibidas son compras; en emitidas, su autorrepercusión si alguna línea lleva cuota y, si
    /// ninguna la lleva, ventas ([`parse_hojas`]).
    Especial,
    /// Facturas con IVA no deducible.
    NoDeducible,
}

pub(crate) fn clase_de_hoja(nombre: &str) -> ClaseHoja {
    let n = norm(nombre);
    if n.contains("nodeduc") {
        ClaseHoja::NoDeducible
    } else if n.contains("entrega") || n.contains("venta") {
        // «Entregas intracomunitarias», «Ventas con inversión del sujeto pasivo»: son ventas.
        ClaseHoja::Normal
    } else if [
        "adquisicion",
        "adqint",
        "sujetopasivo",
        "inversion",
        "autorrepercu",
        "intracomunitari",
        "importacion",
    ]
    .iter()
    .any(|w| n.contains(w))
    {
        ClaseHoja::Especial
    } else {
        ClaseHoja::Normal
    }
}

/// Un libro con varias hojas (por meses, o con las hojas de adquisiciones, inversión del sujeto
/// pasivo y no deducibles de a3 o Sage): las facturas de todas las que son del mismo libro. En
/// recibidas entran todas (las de IVA no deducible, con un 0 % deducible). En emitidas, una hoja
/// de adquisiciones, inversión del sujeto pasivo o autorrepercusión con alguna línea con cuota es
/// la autorrepercusión de compras, que ya sale de las recibidas: si es la primera, sus líneas se
/// marcan ([`BookInvoice::self_assessed`]); si no, no entra. Si ninguna línea lleva cuota, son
/// ventas (entregas exentas, art. 25 LIVA, o con inversión del sujeto pasivo, art. 84.Uno.2º
/// LIVA): en la autorrepercusión de compras hay cuotas.
pub(crate) fn parse_hojas(filename: &str, hojas: &[Hoja]) -> Option<Book> {
    let mut libro: Option<Book> = None;
    for h in hojas {
        let Some(mut b) = parse_rows(filename, &h.filas) else {
            continue;
        };
        let clase = clase_de_hoja(&h.nombre);
        if clase == ClaseHoja::NoDeducible {
            for inv in &mut b.invoices {
                inv.deductible_pct = 0;
            }
        }
        // Una hoja de adquisiciones o de inversión del sujeto pasivo de un libro de emitidas en la
        // que alguna línea lleva cuota es la autorrepercusión de compras (también sus líneas sin
        // cuota: compras exentas o no sujetas que la gestoría apunta con las demás). Si ninguna la
        // lleva, son ventas.
        let compras = b.kind == BookKind::Emitidas
            && clase == ClaseHoja::Especial
            && b.invoices.iter().any(|i| !i.vat().is_zero());
        let Some(l) = libro.as_mut() else {
            if compras {
                for inv in &mut b.invoices {
                    inv.self_assessed = true;
                }
            }
            libro = Some(b);
            continue;
        };
        if b.kind != l.kind || (l.kind == BookKind::Emitidas && clase == ClaseHoja::NoDeducible) {
            l.warnings.push(format!(
                "la hoja «{}» no es de este libro: no entra",
                h.nombre
            ));
            continue;
        }
        if compras {
            l.warnings.push(format!(
                "la hoja «{}» es la autorrepercusión de compras, no ventas: no entra",
                h.nombre
            ));
            continue;
        }
        l.invoices.extend(b.invoices);
        l.warnings.extend(
            b.warnings
                .into_iter()
                .map(|w| format!("hoja «{}»: {w}", h.nombre)),
        );
    }
    libro
}

/// Resultado de importar un libro.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Imported {
    pub added: usize,
    /// Ya estaban (mismo número; en recibidas, mismo proveedor y número).
    pub skipped: usize,
    pub warnings: Vec<String>,
    /// Lote de la importación (para deshacerla con [`undo`]); `None` si no se añadió nada.
    pub batch: Option<i64>,
    /// Líneas de un libro de emitidas que no entran porque son la autorrepercusión de compras
    /// ([`BookInvoice::self_assessed`]).
    pub self_assessed: usize,
}

/// De dónde vienen las facturas que se importan (para el lote que se puede deshacer).
#[derive(Clone, Copy, Debug)]
pub struct BatchOrigin<'a> {
    pub filename: &'a str,
    pub bytes: &'a [u8],
    /// `libro` (libro registro del gestor) o `lectura` (factura leída con el asistente).
    pub source: &'a str,
}

fn open_batch(
    conn: &Connection,
    kind: &str,
    origin: &BatchOrigin,
    now: OffsetDateTime,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO book_import(kind, source, filename, sha256, invoices, created_at)
         VALUES (?1, ?2, ?3, ?4, 0, ?5)",
        params![
            kind,
            origin.source,
            origin.filename,
            hex::encode(Sha256::digest(origin.bytes)),
            now_str(now)
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Cierra el lote con las facturas añadidas (si no se añadió ninguna, no queda lote).
fn close_batch(conn: &Connection, id: i64, added: usize) -> Result<Option<i64>> {
    if added == 0 {
        conn.execute("DELETE FROM book_import WHERE id = ?1", [id])?;
        return Ok(None);
    }
    conn.execute(
        "UPDATE book_import SET invoices = ?1 WHERE id = ?2",
        params![i64::try_from(added).unwrap_or(0), id],
    )?;
    Ok(Some(id))
}

fn other(e: impl std::fmt::Display) -> AppError {
    AppError::Other(e.to_string())
}

/// Tratamiento de IVA de una línea del libro.
fn treatment(line: &BookLine, foreign: bool) -> VatTreatment {
    if line.rate_bp > 0 || !line.vat.is_zero() {
        VatTreatment::Taxed {
            rate: Rate::from_bp(line.rate_bp),
        }
    } else if foreign {
        VatTreatment::NotSubject(NotSubject::N2)
    } else {
        VatTreatment::Exempt(Exemption::E1)
    }
}

/// Serie interna de las facturas importadas de un libro (no se usa para emitir).
fn history_series(v: &Vault, now: OffsetDateTime) -> Result<i64> {
    let conn = v.connection();
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM invoice_series WHERE code = 'HIST'",
            [],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    let s = Series {
        code: "HIST".into(),
        prefix: String::new(),
        yearly_reset: true,
        ..Series::ordinaria()
    };
    conn.execute(
        "INSERT INTO invoice_series(code, data, created_at) VALUES ('HIST', ?1, ?2)",
        params![serde_json::to_string(&s).map_err(other)?, now_str(now)],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Nosotros como emisor (de la configuración de facturas o de la empresa).
fn own_party(v: &Vault) -> Result<Party> {
    if let Some(s) = crate::invoicing::settings(v)? {
        return Ok(s.issuer);
    }
    let info = crate::companies::info(v)?;
    Ok(Party {
        id: PartyId::Nif(info.nif.unwrap_or_default()),
        name: info.name,
        address: None,
        person: None,
    })
}

/// Número con el año para una factura importada cuyo número ya tiene otra de otro año (la
/// numeración del gestor empieza de nuevo cada año): «1 (2025)». Sólo se usa en las importadas;
/// las expedidas en TaxPain no se tocan.
pub fn year_qualified(number: &str, year: i32) -> String {
    format!("{number} ({year})")
}

/// Número con el que se guarda una emitida importada: `None` si ya está (mismo número, tal cual,
/// con el año o escrito de otra forma, en el mismo año); el del libro si está libre; con el año
/// si lo tiene una factura de otro año.
fn issued_number(conn: &Connection, number: &str, year: i32) -> Result<Option<String>> {
    let qualified = year_qualified(number, year);
    let same_year: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM invoice_issued WHERE number IN (?1, ?2)
           AND substr(issue_date, 1, 4) = ?3)",
        params![number, qualified, year.to_string()],
        |r| r.get(0),
    )?;
    // La misma factura con el número escrito de otra forma («A2025-1» en el libro del gestor y
    // «A2025-0001» expedida en TaxPain): entraría dos veces en el 303 y el 390.
    if same_year || issued_with_key(conn, number, year)?.is_some() {
        return Ok(None);
    }
    let taken: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM invoice_issued WHERE number = ?1)",
        [number],
        |r| r.get(0),
    )?;
    Ok(Some(if taken { qualified } else { number.to_owned() }))
}

/// Número con el que se guarda una recibida importada (como [`issued_number`], por proveedor:
/// por NIF o, sin NIF, por nombre).
fn received_number(
    conn: &Connection,
    nif: Option<&str>,
    name: &str,
    number: &str,
    year: i32,
) -> Result<Option<String>> {
    let qualified = year_qualified(number, year);
    let same_year: bool = match nif {
        Some(n) => conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM invoice_received WHERE status = 'revisada'
               AND supplier_nif = ?1 AND number IN (?2, ?3) AND substr(issue_date, 1, 4) = ?4)",
            params![n, number, qualified, year.to_string()],
            |r| r.get(0),
        )?,
        None => conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM invoice_received WHERE status = 'revisada'
               AND COALESCE(supplier_nif, '') = '' AND upper(COALESCE(supplier_name, '')) = upper(?1)
               AND number IN (?2, ?3) AND substr(issue_date, 1, 4) = ?4)",
            params![name, number, qualified, year.to_string()],
            |r| r.get(0),
        )?,
    };
    // La misma factura con el número escrito de otra forma («F77» en el libro y «F-0077»
    // revisada a mano de su PDF): su IVA se deduciría dos veces (art. 99 LIVA).
    if same_year || received_with_key(conn, nif, name, number, year)?.is_some() {
        return Ok(None);
    }
    // Sólo con NIF puede chocar con el índice único (proveedor, número).
    let taken: bool = match nif {
        Some(n) => conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM invoice_received WHERE status = 'revisada'
               AND supplier_nif = ?1 AND number = ?2)",
            params![n, number],
            |r| r.get(0),
        )?,
        None => false,
    };
    Ok(Some(if taken { qualified } else { number.to_owned() }))
}

/// Tipos del recargo de equivalencia (centésimas de punto) y el de IVA con el que van.
const SURCHARGE_RATES: [(i32, i32); 4] = [(520, 2100), (140, 1000), (50, 400), (175, 0)];

/// Tipo del recargo de equivalencia de una línea, deducido de su cuota y su base: el que va con
/// su tipo de IVA si cuadra al céntimo; si no, el conocido más cercano (a menos de una décima
/// parte). `Err` con el tipo calculado si no es ninguno.
fn surcharge_rate(line: &BookLine) -> Option<std::result::Result<Rate, Rate>> {
    if line.surcharge.is_zero() {
        return None;
    }
    let fits = |bp: i32| {
        (line.base.apply_rate(Rate::from_bp(bp)) - line.surcharge)
            .cents()
            .abs()
            <= 2
    };
    if let Some((bp, _)) = SURCHARGE_RATES
        .iter()
        .find(|(bp, vat)| *vat == line.rate_bp && fits(*bp))
    {
        return Some(Ok(Rate::from_bp(*bp)));
    }
    let exact = if line.base.is_zero() {
        0
    } else {
        i32::try_from(
            (i128::from(line.surcharge.cents()) * 10_000 + i128::from(line.base.cents()) / 2)
                / i128::from(line.base.cents()),
        )
        .unwrap_or(0)
    };
    Some(
        SURCHARGE_RATES
            .iter()
            .map(|(bp, _)| *bp)
            .find(|bp| (bp - exact).abs() * 10 <= *bp)
            .map(Rate::from_bp)
            .ok_or(Rate::from_bp(exact)),
    )
}

/// Facturas emitidas del libro → libro de emitidas (sin VERI*FACTU ni asiento).
pub fn import_issued(v: &Vault, book: &Book, now: OffsetDateTime) -> Result<Imported> {
    let origin = BatchOrigin {
        filename: "libro de emitidas",
        bytes: &[],
        source: "libro",
    };
    import_issued_as(v, book, &origin, now)
}

/// Como [`import_issued`], diciendo de qué fichero vienen (para el lote).
pub fn import_issued_as(
    v: &Vault,
    book: &Book,
    origin: &BatchOrigin,
    now: OffsetDateTime,
) -> Result<Imported> {
    if book.kind != BookKind::Emitidas {
        return Err(other("el libro no es de facturas emitidas"));
    }
    let issuer = own_party(v)?;
    let series = history_series(v, now)?;
    let mut out = Imported {
        warnings: book.warnings.clone(),
        ..Imported::default()
    };
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let batch = open_batch(&tx, "emitidas", origin, now)?;
    // La autorrepercusión de una compra no es una venta: entraría en el IVA devengado del 303,
    // en el 390 y en el 347 como si se hubiera vendido algo.
    let mut book = book.clone();
    mark_self_assessed_in_vault(&tx, &mut book)?;
    for inv in &book.invoices {
        if inv.self_assessed {
            out.self_assessed += 1;
            continue;
        }
        let year = inv.issue_date.year();
        let Some(number) = issued_number(&tx, &inv.number, year)? else {
            out.skipped += 1;
            continue;
        };
        if number != inv.number {
            out.warnings.push(format!(
                "{} de {year}: ese número ya lo tiene una factura de otro año (la numeración \
                 empieza de nuevo cada año); se guarda como «{number}»",
                inv.number
            ));
        }
        let seq: i64 = tx.query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM invoice_issued WHERE series_id = ?1 AND year = ?2",
            params![series, year],
            |r| r.get(0),
        )?;
        let foreign = inv.country.is_some();
        // Sólo los clientes de la UE tienen NIF-IVA (y van al 349); los de fuera, «otro
        // documento» (como en los libros registro).
        let eu = inv
            .country
            .as_deref()
            .is_some_and(|c| taxpain_core::eu::is_other_member_state(c, true));
        let recipient = inv.nif.as_ref().map(|n| Party {
            id: match &inv.country {
                Some(c) => PartyId::Foreign {
                    country: c.clone(),
                    id_type: if eu {
                        taxpain_core::invoice::IdType::NifIva
                    } else {
                        taxpain_core::invoice::IdType::Otro
                    },
                    id: n
                        .strip_prefix(taxpain_core::eu::vies_prefix(c).as_str())
                        .unwrap_or(n)
                        .to_owned(),
                },
                None => PartyId::Nif(n.clone()),
            },
            name: inv.name.clone(),
            address: None,
            person: None,
        });
        let kind = if inv.base().is_negative() {
            InvoiceKind::Rectificativa(Rectification {
                code: 1,
                method: RectificationMethod::Diferencias,
            })
        } else if recipient.is_some() {
            InvoiceKind::Completa
        } else {
            InvoiceKind::Simplificada
        };
        // Recargo de equivalencia: el tipo sale de la cuota del recargo y la base.
        let surcharges: Vec<Option<Rate>> = inv
            .lines
            .iter()
            .map(|l| match surcharge_rate(l) {
                None => None,
                Some(Ok(r)) => Some(r),
                Some(Err(r)) => {
                    out.warnings.push(format!(
                        "{number}: el recargo de equivalencia ({} sobre {}) no es de ningún tipo \
                         conocido (5,2 %, 1,4 %, 0,5 %, 1,75 %): revísalo",
                        l.surcharge, l.base
                    ));
                    Some(r)
                }
            })
            .collect();
        let breakdown: Vec<TaxBreakdown> = inv
            .lines
            .iter()
            .zip(&surcharges)
            .map(|(l, sr)| TaxBreakdown {
                vat: treatment(l, foreign),
                surcharge_rate: *sr,
                base: l.base,
                vat_amount: l.vat,
                surcharge_amount: l.surcharge,
            })
            .collect();
        if inv.lines.iter().any(|l| l.rate_bp == 0 && l.vat.is_zero()) {
            out.warnings.push(format!(
                "{number}: sin IVA; se toma como {} (revísala)",
                if foreign {
                    "no sujeta por reglas de localización"
                } else {
                    "exenta (art. 20 LIVA)"
                }
            ));
        }
        let base = inv.base();
        let withholding_rate = if base.is_zero() || inv.withholding.is_zero() {
            Rate::ZERO
        } else {
            Rate::from_bp(rate_bp(None, base, inv.withholding))
        };
        let totals = Totals {
            breakdown,
            withholdings: if inv.withholding.is_zero() {
                vec![]
            } else {
                vec![(withholding_rate, base, inv.withholding)]
            },
            base,
            vat: inv.vat(),
            surcharge: inv.surcharge(),
            withholding: inv.withholding,
            tax_total: inv.vat() + inv.surcharge(),
            invoice_total: base + inv.vat() + inv.surcharge(),
            payable: inv.total,
            oss: None,
        };
        let invoice = Invoice {
            kind,
            number: number.clone(),
            issue_date: inv.issue_date,
            operation_date: None,
            issuer: issuer.clone(),
            recipient,
            lines: inv
                .lines
                .iter()
                .zip(&surcharges)
                .map(|(l, sr)| InvoiceLine {
                    description: "Importada del libro registro".into(),
                    quantity_milli: 1000,
                    unit_price: l.base,
                    discount: Rate::ZERO,
                    vat: treatment(l, foreign),
                    surcharge: *sr,
                    withholding: (!inv.withholding.is_zero()).then_some(withholding_rate),
                    advance: false,
                })
                .collect(),
            rectified: vec![],
            description: "Importada del libro registro de la gestoría".into(),
            notes: None,
        };
        let data = serde_json::json!({ "invoice": invoice, "totals": totals });
        tx.execute(
            "INSERT INTO invoice_issued(series_id, year, seq, number, issue_date, kind, recipient_name,
               recipient_tax_id, base, vat, surcharge, withholding, total, payable, status, data,
               created_at, origin, book_import_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, 'emitida', ?15, ?16, 'importada', ?17)",
            params![
                series,
                year,
                seq,
                number,
                inv.issue_date.to_string(),
                kind.code(),
                (!inv.name.is_empty()).then_some(&inv.name),
                inv.nif,
                base.cents(),
                inv.vat().cents(),
                inv.surcharge().cents(),
                inv.withholding.cents(),
                (base + inv.vat() + inv.surcharge()).cents(),
                inv.total.cents(),
                data.to_string(),
                now_str(now),
                batch,
            ],
        )?;
        out.added += 1;
    }
    if out.self_assessed > 0 {
        out.warnings.push(self_assessed_warning(out.self_assessed));
    }
    out.batch = close_batch(&tx, batch, out.added)?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "issued.import_book",
        origin.filename,
        &serde_json::json!({
            "added": out.added,
            "skipped": out.skipped,
            "self_assessed": out.self_assessed,
            "batch": out.batch,
        }),
    )?;
    tx.commit()?;
    Ok(out)
}

/// Régimen de IVA de una recibida del libro, como en la revisión manual
/// ([`crate::received::VatRegime`]): (régimen, IVA de la factura, tipo autorrepercutido).
/// - Total del libro sin la cuota: inversión del sujeto pasivo; la cuota del libro es la
///   autorrepercutida (intracomunitaria si el proveedor es de la UE).
/// - Proveedor de otro país de la UE: adquisición intracomunitaria (la factura no lleva IVA).
/// - Proveedor de fuera de la UE con cuota en el libro y sin total que la incluya: inversión
///   del sujeto pasivo (servicios, art. 84.Uno.2º LIVA).
fn received_regime(
    inv: &BookInvoice,
    warnings: &mut Vec<String>,
) -> (&'static str, Money, Option<i32>) {
    let eu = inv
        .country
        .as_deref()
        .is_some_and(|c| taxpain_core::eu::is_other_member_state(c, true));
    let foreign = inv.country.is_some();
    let charged = inv.vat() + inv.surcharge();
    let rate = || {
        let rates: Vec<i32> = inv.lines.iter().map(|l| l.rate_bp).collect();
        match rates.as_slice() {
            [r] if *r != 0 => *r,
            _ => computed_rate(inv.base(), inv.vat()),
        }
    };
    let who = format!("{} de {}", inv.number, inv.name);
    let self_assessed = if eu { "intracomunitaria" } else { "isp" };
    match (inv.vat_in_total, eu, foreign) {
        (Some(false), _, _) => (self_assessed, Money::ZERO, Some(rate())),
        (_, true, _) if inv.vat().is_zero() => {
            warnings.push(format!(
                "{who}: proveedor de la UE sin cuota en el libro; se toma como adquisición \
                 intracomunitaria y se autorrepercute al 21 % (revisa el tipo)"
            ));
            ("intracomunitaria", Money::ZERO, Some(2100))
        }
        (Some(true), true, _) | (Some(true), false, true) => {
            warnings.push(format!(
                "{who}: proveedor extranjero con IVA en la factura; se toma como compra con IVA \
                 español (revísala)"
            ));
            ("interior", charged, None)
        }
        (_, true, _) => (self_assessed, Money::ZERO, Some(rate())),
        (_, false, true) if inv.vat().is_zero() => {
            warnings.push(format!(
                "{who}: proveedor de fuera de la UE sin cuota en el libro; si es un servicio, el \
                 IVA se autorrepercute (inversión del sujeto pasivo) y si es una importación va \
                 en el DUA: revísala"
            ));
            ("interior", charged, None)
        }
        (_, false, true) => (self_assessed, Money::ZERO, Some(rate())),
        _ => ("interior", charged, None),
    }
}

/// Facturas recibidas del libro → libro de recibidas, revisadas y sin asiento. El libro entero
/// se guarda una vez como documento de todas ellas.
pub fn import_received(
    v: &Vault,
    book: &Book,
    filename: &str,
    bytes: &[u8],
    now: OffsetDateTime,
) -> Result<Imported> {
    let origin = BatchOrigin {
        filename,
        bytes,
        source: "libro",
    };
    import_received_as(v, book, &origin, now)
}

/// Como [`import_received`], diciendo de dónde vienen (para el lote).
pub fn import_received_as(
    v: &Vault,
    book: &Book,
    origin: &BatchOrigin,
    now: OffsetDateTime,
) -> Result<Imported> {
    let (filename, bytes) = (origin.filename, origin.bytes);
    if book.kind != BookKind::Recibidas {
        return Err(other("el libro no es de facturas recibidas"));
    }
    let mut out = Imported {
        warnings: book.warnings.clone(),
        ..Imported::default()
    };
    let sha = hex::encode(Sha256::digest(bytes));
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let doc: i64 = match tx
        .query_row(
            "SELECT id FROM document WHERE uuid = ?1",
            [format!("libro-{}", &sha[..32])],
            |r| r.get(0),
        )
        .optional()?
    {
        Some(id) => id,
        None => {
            tx.execute(
                "INSERT INTO document(uuid, kind, filename, mime, sha256, size, content, created_at)
                 VALUES (?1, 'libro_recibidas', ?2, 'application/octet-stream', ?3, ?4, ?5, ?6)",
                params![
                    format!("libro-{}", &sha[..32]),
                    filename,
                    sha,
                    i64::try_from(bytes.len()).unwrap_or(0),
                    bytes,
                    now_str(now)
                ],
            )?;
            tx.last_insert_rowid()
        }
    };
    let mut intra = 0usize;
    let mut pending = pending_received(&tx)?;
    let batch = open_batch(&tx, "recibidas", origin, now)?;
    for inv in &book.invoices {
        let year = inv.issue_date.year();
        let Some(number) = received_number(&tx, inv.nif.as_deref(), &inv.name, &inv.number, year)?
        else {
            out.skipped += 1;
            continue;
        };
        if number != inv.number {
            out.warnings.push(format!(
                "{} de {} ({year}): ese número ya lo tiene otra factura del mismo proveedor de \
                 otro año; se guarda como «{number}»",
                inv.number, inv.name
            ));
        }
        if inv.nif.is_none() {
            out.warnings.push(format!(
                "{} de {}: sin NIF del proveedor (no irá al 347)",
                inv.number, inv.name
            ));
        }
        // La cuota de un leasing ya registrada desde Leasing no se importa otra vez.
        if let Some(nif) = inv.nif.as_deref()
            && let Some((msg, _)) = crate::leasing::installment_clash(
                &tx,
                nif,
                &number,
                inv.issue_date,
                inv.base(),
                inv.total,
            )?
        {
            out.warnings
                .push(format!("{number} de {}: no se importa. {msg}", inv.name));
            out.skipped += 1;
            continue;
        }
        let (regime, vat, self_rate) = received_regime(inv, &mut out.warnings);
        if regime == "intracomunitaria" {
            intra += 1;
        }
        // Con autorrepercusión la factura no lleva IVA: el total es la base menos la retención.
        let total = if self_rate.is_some() {
            inv.base() - inv.withholding
        } else {
            inv.total
        };
        // Retención al 19 %: alquiler (115); con otro tipo, profesional (111).
        let nature = if inv.withholding.is_zero() {
            "general"
        } else if rate_bp(None, inv.base(), inv.withholding) == 1900 {
            "alquiler"
        } else {
            "profesional"
        };
        let row_sha = hex::encode(Sha256::digest(
            format!(
                "{sha}|{}|{}|{number}|{}",
                inv.nif.as_deref().unwrap_or_default(),
                inv.name,
                inv.issue_date
            )
            .as_bytes(),
        ));
        tx.execute(
            "INSERT INTO invoice_received(document_id, file_sha256, filename, source, status, extraction,
               supplier_nif, supplier_name, number, issue_date, base, vat, withholding, total,
               deductible_pct, reviewed_at, created_at, nature, supplier_country, posted,
               vat_regime, self_assessed_rate, book_import_id)
             VALUES (?1, ?2, ?3, 'manual', 'revisada', '{}', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?19, ?12, ?15,
               ?13, ?14, 0, ?16, ?17, ?18)",
            params![
                doc,
                row_sha,
                filename,
                inv.nif,
                inv.name,
                number,
                inv.issue_date.to_string(),
                inv.base().cents(),
                vat.cents(),
                inv.withholding.cents(),
                total.cents(),
                format!("{}T00:00:00Z", inv.registered),
                nature,
                inv.country,
                now_str(now),
                regime,
                self_rate,
                batch,
                inv.deductible_pct,
            ],
        )?;
        let row = tx.last_insert_rowid();
        // Su PDF estaba pendiente en Recibidas: se adjunta a la del libro y la pendiente se
        // descarta (si no, al revisarla se deduciría dos veces).
        let totals = [total, inv.base() + inv.vat()];
        if let Some(pos) = pending.iter().position(|(_, _, _, p)| {
            p.same_as(
                inv.nif.as_deref(),
                &inv.name,
                &number,
                Some(inv.issue_date),
                &totals,
            )
        }) {
            let (pid, pdoc, pfile, _) = pending.remove(pos);
            tx.execute(
                "UPDATE invoice_received SET document_id = ?1, filename = ?2 WHERE id = ?3",
                params![pdoc, pfile, row],
            )?;
            tx.execute(
                "UPDATE invoice_received SET status = 'descartada' WHERE id = ?1 AND status = 'pendiente'",
                [pid],
            )?;
            out.warnings.push(format!(
                "{number} de {}: su PDF estaba pendiente en Recibidas; se adjunta a la del libro y \
                 se descarta la pendiente",
                inv.name
            ));
        }
        out.added += 1;
    }
    if intra > 0 {
        out.warnings.push(format!(
            "{intra} factura(s) de proveedores de la UE se toman como adquisiciones \
             intracomunitarias de servicios (clave I del 349): revisa las que sean de bienes \
             (clave A)"
        ));
    }
    out.batch = close_batch(&tx, batch, out.added)?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "received.import_book",
        filename,
        &serde_json::json!({ "added": out.added, "skipped": out.skipped, "batch": out.batch }),
    )?;
    tx.commit()?;
    Ok(out)
}

// ---------- Buscar una factura recibida en el libro ----------

/// Clave de un número de factura para compararlo con el de otro sitio: mayúsculas, sin
/// separadores, sin ceros a la izquierda en cada grupo de cifras y sin el año que se añade a
/// las importadas repetidas («F-0077» = «F77» = «f 77»; «7 (2025)» = «7»).
pub fn number_key(s: &str) -> String {
    let s = s.trim();
    let s = match s.strip_suffix(')').and_then(|x| x.rsplit_once(" (")) {
        Some((head, y)) if y.len() == 4 && y.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => s,
    };
    fn flush(digits: &mut String, out: &mut String) {
        if !digits.is_empty() {
            let t = digits.trim_start_matches('0');
            out.push_str(if t.is_empty() { "0" } else { t });
            digits.clear();
        }
    }
    let mut out = String::new();
    let mut digits = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else {
            flush(&mut digits, &mut out);
            if c.is_alphanumeric() {
                out.extend(c.to_uppercase());
            }
        }
    }
    flush(&mut digits, &mut out);
    out
}

/// Nombre para comparar: letras y cifras, sin la forma social («Bar Pepe, S.L.» = «bar pepe»).
fn name_key(s: &str) -> String {
    let n = norm(s);
    for suffix in [
        "sociedadlimitadaunipersonal",
        "sociedadlimitada",
        "sociedadanonima",
        "slu",
        "slp",
        "sll",
        "sl",
        "sau",
        "sa",
        "cb",
    ] {
        if let Some(rest) = n.strip_suffix(suffix)
            && rest.len() >= 3
        {
            return rest.to_owned();
        }
    }
    n
}

fn names_match(a: &str, b: &str) -> bool {
    let (a, b) = (name_key(a), name_key(b));
    let (short, long) = if a.len() <= b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    short.len() >= 3 && (short == long || (short.len() >= 5 && long.contains(short.as_str())))
}

fn nif_key(raw: &str) -> Option<String> {
    normalize_nif(raw, None).0
}

/// Datos de una factura recibida (de un PDF, una foto o una lectura) para buscarla en el libro.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReceivedRef {
    pub nif: Option<String>,
    pub name: Option<String>,
    pub number: Option<String>,
    pub date: Option<Date>,
    pub total: Option<Money>,
}

impl ReceivedRef {
    pub fn from_extraction(e: &taxpain_invoicing::ingest::Extraction) -> Self {
        ReceivedRef {
            nif: e.supplier_nif.as_ref().map(|f| f.value.clone()),
            name: e.supplier_name.as_ref().map(|f| f.value.clone()),
            number: e.number.as_ref().map(|f| f.value.clone()),
            date: e.issue_date.as_ref().map(|f| f.value),
            total: e.total.as_ref().map(|f| f.value),
        }
    }

    /// ¿Es la misma factura que otra (del libro)? Mismo proveedor (NIF o, si a alguna le falta,
    /// nombre) y mismo número normalizado del mismo año, o misma fecha y mismo total (`totals`:
    /// el total y, si lleva retención, base + IVA).
    pub fn same_as(
        &self,
        nif: Option<&str>,
        name: &str,
        number: &str,
        date: Option<Date>,
        totals: &[Money],
    ) -> bool {
        let party = match (self.nif.as_deref().and_then(nif_key), nif.and_then(nif_key)) {
            (Some(a), Some(b)) => a == b,
            _ => self.name.as_deref().is_some_and(|n| names_match(n, name)),
        };
        if !party {
            return false;
        }
        let same_year = match (self.date, date) {
            (Some(a), Some(b)) => a.year() == b.year(),
            _ => true,
        };
        let same_number = self
            .number
            .as_deref()
            .map(number_key)
            .is_some_and(|k| !k.is_empty() && k == number_key(number))
            && same_year;
        let same_amount = self.date.is_some()
            && self.date == date
            && self
                .total
                .is_some_and(|t| !t.is_zero() && totals.contains(&t));
        same_number || same_amount
    }
}

/// ¿Es la línea `i` de un libro de emitidas la misma factura que `r`, del libro de recibidas?
/// Del mismo año, con el mismo número normalizado, la misma base y del mismo proveedor (por NIF
/// o, si a alguna le falta, por el nombre). Con la base no se confunde una venta a un cliente
/// que también es proveedor y numera igual.
fn same_purchase(i: &BookInvoice, r: &BookInvoice) -> bool {
    if i.issue_date.year() != r.issue_date.year() || !same_base(i.base(), r.base()) {
        return false;
    }
    let key = number_key(&i.number);
    if key.is_empty() || key != number_key(&r.number) {
        return false;
    }
    match (
        i.nif.as_deref().and_then(nif_key),
        r.nif.as_deref().and_then(nif_key),
    ) {
        (Some(a), Some(b)) => a == b,
        _ => names_match(&i.name, &r.name),
    }
}

/// Marca las líneas de un libro de emitidas que son la autorrepercusión de una compra de
/// `received` (las facturas de un libro de recibidas): la misma factura, por número y
/// proveedor. Devuelve cuántas líneas del libro son autorrepercusión, también las que ya lo eran
/// por sus importes o por su hoja; en un libro de recibidas, 0.
pub fn mark_self_assessed(book: &mut Book, received: &[BookInvoice]) -> usize {
    if book.kind != BookKind::Emitidas {
        return 0;
    }
    for i in &mut book.invoices {
        if !i.self_assessed && received.iter().any(|r| same_purchase(i, r)) {
            i.self_assessed = true;
        }
    }
    book.invoices.iter().filter(|i| i.self_assessed).count()
}

fn same_base(a: Money, b: Money) -> bool {
    (a - b).cents().abs() <= 1
}

/// Como [`mark_self_assessed`], con las recibidas del libro de recibidas de TaxPain.
pub fn mark_self_assessed_in_vault(conn: &Connection, book: &mut Book) -> Result<usize> {
    if book.kind != BookKind::Emitidas {
        return Ok(0);
    }
    let mut stmt = conn.prepare(
        "SELECT COALESCE(number, ''), COALESCE(base, 0) FROM invoice_received
         WHERE status = 'revisada' AND substr(issue_date, 1, 4) = ?1
           AND ((?2 IS NOT NULL AND supplier_nif = ?2)
             OR (?2 IS NULL AND COALESCE(supplier_nif, '') = ''
                 AND upper(COALESCE(supplier_name, '')) = upper(?3)))",
    )?;
    for i in &mut book.invoices {
        let key = number_key(&i.number);
        if i.self_assessed || key.is_empty() {
            continue;
        }
        let rows = stmt.query_map(
            params![i.issue_date.year().to_string(), i.nif, i.name],
            |r| Ok((r.get::<_, String>(0)?, Money::from_cents(r.get(1)?))),
        )?;
        for row in rows {
            let (number, base) = row?;
            if number_key(&number) == key && same_base(base, i.base()) {
                i.self_assessed = true;
                break;
            }
        }
    }
    Ok(book.invoices.iter().filter(|i| i.self_assessed).count())
}

/// Aviso de las líneas de un libro de emitidas que son la autorrepercusión de compras.
pub(crate) fn self_assessed_warning(n: usize) -> String {
    format!(
        "{} la autorrepercusión de compras (inversión del sujeto pasivo o adquisiciones \
         intracomunitarias, arts. 84.Uno.2º y 85 LIVA), no ventas: no entran en el libro de \
         emitidas, porque esas compras van en el de recibidas",
        if n == 1 {
            "1 línea del libro es".to_owned()
        } else {
            format!("{n} líneas del libro son")
        }
    )
}

/// Factura del libro de recibidas (revisada) que es la misma que `r`, si la hay.
pub fn find_received(conn: &Connection, r: &ReceivedRef) -> Result<Option<i64>> {
    if r.nif.is_none() && r.name.is_none() {
        return Ok(None);
    }
    let nif = r.nif.as_deref().and_then(nif_key);
    let mut stmt = conn.prepare(
        "SELECT id, supplier_nif, COALESCE(supplier_name, ''), COALESCE(number, ''), issue_date,
                COALESCE(total, 0), COALESCE(base, 0) + COALESCE(vat, 0)
         FROM invoice_received WHERE status = 'revisada'
           AND (?1 IS NULL OR supplier_nif = ?1 OR COALESCE(supplier_nif, '') = '')
         ORDER BY id",
    )?;
    let rows = stmt.query_map([nif], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, i64>(6)?,
        ))
    })?;
    for row in rows {
        let (id, n, name, number, date, total, gross) = row?;
        let date = date.as_deref().and_then(parse_date);
        let totals = [Money::from_cents(total), Money::from_cents(gross)];
        if r.same_as(n.as_deref(), &name, &number, date, &totals) {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// Aviso de una factura recibida de un periodo que ya declaró el gestor y que no está en su
/// libro.
pub(crate) const NOT_IN_BOOK: &str = "es de un periodo que ya declaró el gestor y no está en su \
     libro: no se propone importarla (si la importas y la revisas, su IVA se deduciría en el \
     trimestre en que la revises)";

/// ¿El trimestre de `date` ya lo declaró el gestor? Hay un 303 suyo importado de la carpeta de
/// ese trimestre (o del mes) o su 390 del año, o recibidas de su libro registradas en ese
/// trimestre. Una factura de ese periodo que no está en su libro no se debe deducir ahora sin
/// mirarla (lo normal es que ya esté deducida).
pub fn period_covered(conn: &Connection, date: Date) -> Result<bool> {
    let month = u8::from(date.month());
    let q = (month - 1) / 3 + 1;
    let year = date.year();
    let start = format!("{year}-{:02}-01", q * 3 - 2);
    let end = format!("{year}-{:02}-31", q * 3);
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM filed_model WHERE year = ?1
                  AND ((model = '303' AND period IN (?2, ?3)) OR model = '390'))
             OR EXISTS(SELECT 1 FROM invoice_received WHERE posted = 0 AND status = 'revisada'
                  AND substr(reviewed_at, 1, 10) BETWEEN ?4 AND ?5)",
        params![year, format!("{q}T"), format!("{month:02}"), start, end],
        |r| r.get(0),
    )?)
}

/// Adjunta un documento (el PDF de la factura) a una recibida del libro. Si ya lo estaba, no
/// hace nada; si el mismo fichero estaba pendiente de revisar en Recibidas, esa pendiente se
/// descarta (es la misma factura, ya en el libro). Devuelve qué se hizo.
pub fn attach_document(
    v: &Vault,
    id: i64,
    filename: &str,
    bytes: &[u8],
    now: OffsetDateTime,
) -> Result<String> {
    let sha = hex::encode(Sha256::digest(bytes));
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let current: Option<i64> = tx
        .query_row(
            "SELECT document_id FROM invoice_received WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    let existing: Option<i64> = tx
        .query_row(
            "SELECT id FROM document WHERE sha256 = ?1 ORDER BY id LIMIT 1",
            [&sha],
            |r| r.get(0),
        )
        .optional()?;
    if current.is_some() && current == existing {
        return Ok("ya estaba adjuntada a su factura del libro de recibidas".into());
    }
    let doc = match existing {
        Some(d) => d,
        None => {
            tx.execute(
                "INSERT INTO document(uuid, kind, filename, mime, sha256, size, content, created_at)
                 VALUES (?1, 'recibida', ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    format!("recibida-{}", &sha[..32]),
                    filename,
                    if bytes.starts_with(b"%PDF") {
                        "application/pdf"
                    } else {
                        "application/octet-stream"
                    },
                    sha,
                    i64::try_from(bytes.len()).unwrap_or(0),
                    bytes,
                    now_str(now)
                ],
            )?;
            tx.last_insert_rowid()
        }
    };
    tx.execute(
        "UPDATE invoice_received SET document_id = ?1, filename = ?2 WHERE id = ?3",
        params![doc, filename, id],
    )?;
    let discarded = tx.execute(
        "UPDATE invoice_received SET status = 'descartada'
         WHERE status = 'pendiente' AND (file_sha256 = ?1 OR document_id = ?2)",
        params![sha, doc],
    )?;
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "received.attach_book",
        filename,
        &serde_json::json!({ "invoice": id, "discarded_pending": discarded }),
    )?;
    tx.commit()?;
    Ok(if discarded > 0 {
        "adjuntada a su factura del libro de recibidas (estaba pendiente en Recibidas: se descarta \
         la pendiente)"
            .into()
    } else {
        "adjuntada a su factura del libro de recibidas".into()
    })
}

/// Número de una recibida ya revisada del mismo proveedor con el mismo número normalizado y del
/// mismo año que la que se va a revisar (`exclude`), si la hay: sería la misma factura dos
/// veces (p. ej. la del libro del gestor, «F77», y su PDF, «F-077»).
pub fn reviewed_duplicate(
    conn: &Connection,
    nif: &str,
    number: &str,
    date: Date,
    exclude: i64,
) -> Result<Option<String>> {
    let key = number_key(number);
    let mut stmt = conn.prepare(
        "SELECT COALESCE(number, '') FROM invoice_received
         WHERE status = 'revisada' AND supplier_nif = ?1 AND id <> ?2
           AND substr(issue_date, 1, 4) = ?3",
    )?;
    let rows = stmt.query_map(params![nif, exclude, date.year().to_string()], |r| {
        r.get::<_, String>(0)
    })?;
    for n in rows {
        let n = n?;
        if !key.is_empty() && number_key(&n) == key {
            return Ok(Some(n));
        }
    }
    Ok(None)
}

/// Añade un aviso a una recibida pendiente de revisar (se ve al revisarla).
pub(crate) fn warn_pending(v: &Vault, id: i64, warning: &str) -> Result<()> {
    let conn = v.connection();
    let Some(stored) = conn
        .query_row(
            "SELECT extraction FROM invoice_received WHERE id = ?1 AND status = 'pendiente'",
            [id],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    else {
        return Ok(());
    };
    let mut e: taxpain_invoicing::ingest::Extraction =
        serde_json::from_str(&stored).unwrap_or_default();
    if !e.warnings.iter().any(|w| w == warning) {
        e.warnings.push(warning.to_owned());
    }
    conn.execute(
        "UPDATE invoice_received SET extraction = ?1 WHERE id = ?2 AND status = 'pendiente'",
        params![serde_json::to_string(&e).map_err(other)?, id],
    )?;
    Ok(())
}

/// Número con el que ya está en el libro de emitidas una factura del mismo número (normalizado)
/// y del mismo año, si la hay.
pub fn find_issued(conn: &Connection, number: &str, date: Date) -> Result<Option<String>> {
    issued_with_key(conn, number, date.year())
}

/// Como [`find_issued`], por año.
fn issued_with_key(conn: &Connection, number: &str, year: i32) -> Result<Option<String>> {
    let key = number_key(number);
    if key.is_empty() {
        return Ok(None);
    }
    let mut stmt = conn.prepare(
        "SELECT number FROM invoice_issued WHERE substr(issue_date, 1, 4) = ?1 ORDER BY id",
    )?;
    let rows = stmt.query_map([year.to_string()], |r| r.get::<_, String>(0))?;
    for n in rows {
        let n = n?;
        if number_key(&n) == key {
            return Ok(Some(n));
        }
    }
    Ok(None)
}

/// Número con el que ya está revisada en el libro de recibidas una factura del mismo proveedor
/// (por NIF o, sin NIF, por nombre), del mismo número normalizado y del mismo año, si la hay.
fn received_with_key(
    conn: &Connection,
    nif: Option<&str>,
    name: &str,
    number: &str,
    year: i32,
) -> Result<Option<String>> {
    let key = number_key(number);
    if key.is_empty() {
        return Ok(None);
    }
    let mut stmt = conn.prepare(
        "SELECT COALESCE(number, '') FROM invoice_received
         WHERE status = 'revisada' AND substr(issue_date, 1, 4) = ?1
           AND ((?2 IS NOT NULL AND supplier_nif = ?2)
             OR (?2 IS NULL AND COALESCE(supplier_nif, '') = ''
                 AND upper(COALESCE(supplier_name, '')) = upper(?3)))
         ORDER BY id",
    )?;
    let rows = stmt.query_map(params![year.to_string(), nif, name], |r| {
        r.get::<_, String>(0)
    })?;
    for n in rows {
        let n = n?;
        if number_key(&n) == key {
            return Ok(Some(n));
        }
    }
    Ok(None)
}

/// Facturas propias de TaxPain (expedidas en la app o recibidas revisadas a mano, no de un
/// libro importado) entre dos fechas: si se importa un libro del gestor de esas fechas, las que
/// no se reconozcan por su número se contarían dos veces.
pub fn own_invoices_between(
    conn: &Connection,
    kind: BookKind,
    first: Date,
    last: Date,
) -> Result<usize> {
    let sql = match kind {
        BookKind::Emitidas => {
            "SELECT COUNT(*) FROM invoice_issued WHERE book_import_id IS NULL
               AND issue_date BETWEEN ?1 AND ?2"
        }
        BookKind::Recibidas => {
            "SELECT COUNT(*) FROM invoice_received WHERE status = 'revisada'
               AND book_import_id IS NULL AND issue_date BETWEEN ?1 AND ?2"
        }
    };
    let n: i64 = conn.query_row(sql, params![first.to_string(), last.to_string()], |r| {
        r.get(0)
    })?;
    Ok(usize::try_from(n).unwrap_or(0))
}

/// Como [`period_covered`] para las emitidas: un modelo del gestor de ese trimestre o emitidas
/// de su libro (no de facturas sueltas) en él.
pub fn issued_period_covered(conn: &Connection, date: Date) -> Result<bool> {
    let month = u8::from(date.month());
    let q = (month - 1) / 3 + 1;
    let year = date.year();
    let start = format!("{year}-{:02}-01", q * 3 - 2);
    let end = format!("{year}-{:02}-31", q * 3);
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM filed_model WHERE year = ?1
                  AND ((model = '303' AND period IN (?2, ?3)) OR model = '390'))
             OR EXISTS(SELECT 1 FROM invoice_issued i JOIN book_import b ON b.id = i.book_import_id
                  WHERE b.source = 'libro' AND i.issue_date BETWEEN ?4 AND ?5)",
        params![year, format!("{q}T"), format!("{month:02}"), start, end],
        |r| r.get(0),
    )?)
}

/// Recibidas pendientes de revisar (de un PDF de la carpeta o arrastrado) que son la misma
/// factura que una del libro que se importa: (id, documento, fichero, datos).
type Pending = (i64, i64, String, ReceivedRef);

fn pending_received(conn: &Connection) -> Result<Vec<Pending>> {
    let mut stmt = conn.prepare(
        "SELECT id, document_id, filename, supplier_nif, supplier_name, number, issue_date, total
         FROM invoice_received WHERE status = 'pendiente' ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, String>(2)?,
            ReceivedRef {
                nif: r.get(3)?,
                name: r.get(4)?,
                number: r.get(5)?,
                date: r
                    .get::<_, Option<String>>(6)?
                    .as_deref()
                    .and_then(parse_date),
                total: r.get::<_, Option<i64>>(7)?.map(Money::from_cents),
            },
        ))
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

// ---------- Deshacer una importación ----------

/// Lote de facturas importadas.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BookBatch {
    pub id: i64,
    /// `emitidas` o `recibidas`.
    pub kind: String,
    /// `libro` o `lectura`.
    pub source: String,
    pub filename: String,
    pub invoices: i64,
    pub created_at: String,
}

/// Importaciones de facturas, la última primero.
pub fn batches(v: &Vault) -> Result<Vec<BookBatch>> {
    let mut stmt = v.connection().prepare(
        "SELECT id, kind, source, filename, invoices, created_at FROM book_import ORDER BY id DESC",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(BookBatch {
            id: r.get(0)?,
            kind: r.get(1)?,
            source: r.get(2)?,
            filename: r.get(3)?,
            invoices: r.get(4)?,
            created_at: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

/// Deshace una importación de facturas: borra las del lote (emitidas importadas o recibidas sin
/// asiento). No se puede si alguna ya tiene un cobro o pago registrado, está conciliada con el
/// banco o en una remesa, o la rectifica otra factura. Las recibidas que estaban pendientes y se
/// descartaron al adjuntar su PDF a una del lote vuelven a Recibidas. Devuelve cuántas se borran.
pub fn undo(v: &Vault, id: i64, now: OffsetDateTime, confirmed: bool) -> Result<usize> {
    if !confirmed {
        return Err(AppError::NotConfirmed);
    }
    let conn = v.connection();
    let tx = conn.unchecked_transaction()?;
    let (kind, filename): (String, String) = tx
        .query_row(
            "SELECT kind, filename FROM book_import WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(AppError::NotFound)?;
    let issued = kind == "emitidas";
    let (table, inv_kind) = if issued {
        ("invoice_issued", "emitida")
    } else {
        ("invoice_received", "recibida")
    };
    let extra = if issued {
        "OR EXISTS(SELECT 1 FROM invoice_issued x WHERE x.rectifies_id = i.id)"
    } else {
        "OR EXISTS(SELECT 1 FROM journal_entry e WHERE e.origin_kind = 'FacturaRecibida'
              AND e.origin_id = i.id)"
    };
    let blocked: Option<String> = tx
        .query_row(
            &format!(
                "SELECT COALESCE(i.number, '') FROM {table} i WHERE i.book_import_id = ?1 AND (
                   EXISTS(SELECT 1 FROM settlement s WHERE s.kind = ?2 AND s.invoice_id = i.id)
                   OR EXISTS(SELECT 1 FROM bank_tx b WHERE b.matched_kind = ?2 AND b.matched_id = i.id)
                   OR EXISTS(SELECT 1 FROM remittance_item r WHERE r.kind = ?2 AND r.invoice_id = i.id)
                   {extra}) LIMIT 1"
            ),
            params![id, inv_kind],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(n) = blocked {
        return Err(other(format!(
            "no se puede deshacer: la factura {n} ya tiene un cobro o pago, está conciliada con \
             el banco o en una remesa, o la rectifica otra"
        )));
    }
    tx.execute(
        &format!(
            "DELETE FROM invoice_installment WHERE kind = ?1
               AND invoice_id IN (SELECT id FROM {table} WHERE book_import_id = ?2)"
        ),
        params![inv_kind, id],
    )?;
    let mut docs: Vec<i64> = Vec::new();
    if !issued {
        tx.execute(
            "UPDATE invoice_received SET status = 'pendiente'
             WHERE status = 'descartada' AND book_import_id IS NULL
               AND document_id IN (SELECT document_id FROM invoice_received WHERE book_import_id = ?1)",
            [id],
        )?;
        let mut stmt = tx.prepare(
            "SELECT DISTINCT document_id FROM invoice_received WHERE book_import_id = ?1",
        )?;
        docs = stmt
            .query_map([id], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
    }
    let n = tx.execute(
        &format!("DELETE FROM {table} WHERE book_import_id = ?1"),
        [id],
    )?;
    tx.execute("DELETE FROM book_import WHERE id = ?1", [id])?;
    // Los documentos que ya no usa nadie (el libro, los PDF adjuntados).
    for d in docs {
        let used: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM invoice_received WHERE document_id = ?1)
                 OR EXISTS(SELECT 1 FROM company_document WHERE document_id = ?1)
                 OR EXISTS(SELECT 1 FROM filed_model WHERE document_id = ?1)",
            [d],
            |r| r.get(0),
        )?;
        if !used {
            tx.execute("DELETE FROM document WHERE id = ?1", [d])?;
        }
    }
    audit::append(
        &tx,
        &now_str(now),
        Actor::User,
        "book_import.undo",
        &id.to_string(),
        &serde_json::json!({ "kind": kind, "file": filename, "invoices": n }),
    )?;
    tx.commit()?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(t: &[&[&str]]) -> Vec<Vec<String>> {
        t.iter()
            .map(|r| r.iter().map(|c| (*c).to_owned()).collect())
            .collect()
    }

    #[test]
    fn libro_de_emitidas_formato_aeat_con_dos_tipos() {
        let r = rows(&[
            &["Libro registro de facturas expedidas 2025"],
            &[],
            &[
                "Fecha Expedición",
                "Serie",
                "Número Factura",
                "NIF Destinatario",
                "Nombre Destinatario",
                "Base Imponible",
                "Tipo de IVA",
                "Cuota IVA Repercutida",
                "Total Factura",
            ],
            &[
                "15/01/2025",
                "A",
                "2025-001",
                "B12345674",
                "Cliente SL",
                "1.000,00",
                "21",
                "210,00",
                "1.310,00",
            ],
            &[
                "15/01/2025",
                "A",
                "2025-001",
                "B12345674",
                "Cliente SL",
                "100,00",
                "10",
                "10,00",
                "1.310,00",
            ],
            &[
                "03/02/2025",
                "A",
                "2025-002",
                "",
                "Venta al contado",
                "50",
                "21",
                "10,5",
                "60,5",
            ],
            &["", "", "", "", "TOTAL", "1.150,00", "", "230,50", ""],
        ]);
        let b = parse_rows("libro.xlsx", &r).unwrap_or_else(|| panic!("no leído"));
        assert_eq!(b.kind, BookKind::Emitidas);
        assert_eq!(b.invoices.len(), 2);
        let a = &b.invoices[0];
        assert_eq!(a.number, "A2025-001");
        assert_eq!(a.lines.len(), 2);
        assert_eq!(a.base(), Money::from_cents(110_000));
        assert_eq!(a.vat(), Money::from_cents(22_000));
        assert_eq!(a.lines[1].rate_bp, 1000);
        assert_eq!(b.invoices[1].nif, None);
        assert_eq!(b.invoices[1].vat(), Money::from_cents(1050));
        assert!(
            b.warnings.iter().any(|w| w.contains("1 fila")),
            "{:?}",
            b.warnings
        );
    }

    #[test]
    fn libro_de_recibidas_con_retencion_y_tipo_calculado() {
        let r = rows(&[
            &[
                "Fecha",
                "Nº Factura",
                "NIF Proveedor",
                "Proveedor",
                "Base",
                "Cuota",
                "Retención",
                "Total",
            ],
            &[
                "2025-03-01",
                "F-77",
                "12345678Z",
                "Asesoría",
                "300",
                "63",
                "45",
                "318",
            ],
            &[
                "2025-03-05",
                "ALQ-3",
                "B12345674",
                "Inmuebles SL",
                "1000",
                "210",
                "190",
                "1020",
            ],
        ]);
        let b = parse_rows("recibidas.csv", &r).unwrap_or_else(|| panic!("no leído"));
        assert_eq!(b.kind, BookKind::Recibidas);
        assert_eq!(b.invoices[0].lines[0].rate_bp, 2100);
        assert_eq!(b.invoices[0].withholding, Money::from_cents(4500));
        assert_eq!(b.invoices[0].total, Money::from_cents(31_800));
        assert!(b.warnings.is_empty(), "{:?}", b.warnings);
    }

    #[test]
    fn no_confunde_un_diario_con_un_libro() {
        let r = rows(&[
            &["Fecha", "Asiento", "Cuenta", "Concepto", "Debe", "Haber"],
            &["01/01/2025", "1", "572", "Apertura", "100", ""],
        ]);
        assert!(parse_rows("diario.xlsx", &r).is_none());
    }

    fn one(header: &[&str], row: &[&str]) -> Book {
        let r = rows(&[&["Libro registro de facturas recibidas"], header, row]);
        parse_rows("recibidas.csv", &r).unwrap_or_else(|| panic!("no leído: {header:?}"))
    }

    #[test]
    fn iva_a_secas_junto_a_la_base_es_la_cuota() {
        // «Base | IVA | Total» con la cuota en «IVA»: no es un tipo del 21.000 %.
        let b = one(
            &[
                "Fecha",
                "Nº Factura",
                "NIF Proveedor",
                "Proveedor",
                "Base",
                "IVA",
                "Total",
            ],
            &[
                "2025-03-01",
                "F-1",
                "12345678Z",
                "Asesoría",
                "1.000,00",
                "210,00",
                "1.210,00",
            ],
        );
        let l = &b.invoices[0].lines[0];
        assert_eq!((l.rate_bp, l.vat), (2100, Money::from_cents(21_000)));
        assert!(b.warnings.is_empty(), "{:?}", b.warnings);
        // La misma columna con el tipo dentro («21»): se reconoce por la base.
        let b = one(
            &[
                "Fecha",
                "Nº Factura",
                "NIF Proveedor",
                "Proveedor",
                "Base",
                "IVA",
                "Total",
            ],
            &[
                "2025-03-01",
                "F-1",
                "12345678Z",
                "Asesoría",
                "1.000,00",
                "21",
                "1.210,00",
            ],
        );
        let l = &b.invoices[0].lines[0];
        assert_eq!((l.rate_bp, l.vat), (2100, Money::from_cents(21_000)));
        // «% IVA | IVA €»: tipo y cuota.
        let b = one(
            &[
                "Fecha",
                "Nº Factura",
                "NIF Proveedor",
                "Proveedor",
                "Base",
                "% IVA",
                "IVA €",
            ],
            &[
                "2025-03-01",
                "F-1",
                "12345678Z",
                "Asesoría",
                "100",
                "10",
                "10",
            ],
        );
        let l = &b.invoices[0].lines[0];
        assert_eq!((l.rate_bp, l.vat), (1000, Money::from_cents(1000)));
        // «% IVA | IVA»: «IVA» es la cuota porque ya hay columna de tipo.
        let b = one(
            &[
                "Fecha",
                "Nº Factura",
                "NIF Proveedor",
                "Proveedor",
                "Base",
                "% IVA",
                "IVA",
            ],
            &[
                "2025-03-01",
                "F-1",
                "12345678Z",
                "Asesoría",
                "200",
                "0,21",
                "42",
            ],
        );
        let l = &b.invoices[0].lines[0];
        assert_eq!((l.rate_bp, l.vat), (2100, Money::from_cents(4200)));
    }

    #[test]
    fn el_tipo_de_la_retencion_no_es_su_importe() {
        let b = one(
            &[
                "Fecha",
                "Nº Factura",
                "NIF Proveedor",
                "Proveedor",
                "Base",
                "Cuota",
                "% IRPF",
                "IRPF",
                "Total",
            ],
            &[
                "2025-07-01",
                "F-9",
                "12345678Z",
                "Abogado",
                "1000",
                "210",
                "15",
                "150",
                "1060",
            ],
        );
        assert_eq!(b.invoices[0].withholding, Money::from_cents(15_000));
        assert!(b.warnings.is_empty(), "{:?}", b.warnings);
        // Sólo con el tipo: base × tipo.
        let b = one(
            &[
                "Fecha",
                "Nº Factura",
                "NIF Proveedor",
                "Proveedor",
                "Base",
                "Cuota",
                "% Retención",
            ],
            &[
                "2025-07-01",
                "F-9",
                "12345678Z",
                "Abogado",
                "1000",
                "210",
                "15",
            ],
        );
        assert_eq!(b.invoices[0].withholding, Money::from_cents(15_000));
    }

    #[test]
    fn manda_la_cuota_si_el_tipo_no_cuadra_y_el_recargo_se_lee_aparte() {
        let b = one(
            &[
                "Fecha",
                "Nº Factura",
                "NIF Proveedor",
                "Proveedor",
                "Base",
                "Tipo",
                "Cuota",
                "Cuota RE",
                "Total",
            ],
            &[
                "2025-03-01",
                "F-1",
                "12345678Z",
                "Mayorista",
                "1000",
                "10",
                "210",
                "52",
                "1262",
            ],
        );
        let l = &b.invoices[0].lines[0];
        assert_eq!(
            (l.rate_bp, l.vat, l.surcharge),
            (2100, Money::from_cents(21_000), Money::from_cents(5200))
        );
        assert!(
            b.warnings.iter().any(|w| w.contains("no cuadra")),
            "{:?}",
            b.warnings
        );
    }

    #[test]
    fn la_numeracion_que_empieza_cada_ano_no_junta_facturas() {
        let r = rows(&[
            &["Libro registro de facturas expedidas"],
            &[
                "Fecha",
                "Número",
                "NIF Cliente",
                "Cliente",
                "Base",
                "Tipo IVA",
                "Cuota IVA",
            ],
            &[
                "20/12/2024",
                "1",
                "B12345674",
                "Cliente SL",
                "100",
                "21",
                "21",
            ],
            &[
                "10/01/2025",
                "1",
                "B12345674",
                "Cliente SL",
                "200",
                "21",
                "42",
            ],
        ]);
        let b = parse_rows("emitidas.csv", &r).unwrap_or_else(|| panic!("no leído"));
        assert_eq!(b.invoices.len(), 2);
        assert_eq!(b.invoices[1].base(), Money::from_cents(20_000));
    }

    #[test]
    fn la_retencion_de_una_rectificativa_va_en_negativo() {
        let b = one(
            &[
                "Fecha",
                "Nº Factura",
                "NIF Proveedor",
                "Proveedor",
                "Base",
                "Cuota",
                "Retención",
                "Total",
            ],
            &[
                "2025-03-01",
                "R-1",
                "12345678Z",
                "Asesoría",
                "-100",
                "-21",
                "15",
                "-106",
            ],
        );
        let inv = &b.invoices[0];
        assert_eq!(inv.withholding, Money::from_cents(-1500));
        assert_eq!(inv.total, Money::from_cents(-10_600));
        assert!(b.warnings.is_empty(), "{:?}", b.warnings);
    }

    #[test]
    fn tipos_escritos_de_varias_formas() {
        assert_eq!(cell_rate("21"), Some(2100));
        assert_eq!(cell_rate("21,00 %"), Some(2100));
        assert_eq!(cell_rate("0,21"), Some(2100));
        assert_eq!(cell_rate("0.04"), Some(400));
        assert_eq!(cell_rate("5,2"), Some(520));
        assert_eq!(cell_rate("0,5"), Some(50));
        assert_eq!(cell_rate("210,00"), None);
        assert_eq!(cell_rate(""), None);
        assert_eq!(
            rate_bp(Some("0"), Money::from_cents(1000), Money::from_cents(100)),
            1000
        );
    }

    #[test]
    fn nif_normalizado() {
        assert_eq!(
            normalize_nif("ES-B12345674", None),
            (Some("B12345674".into()), None)
        );
        assert_eq!(
            normalize_nif("fr 12345678901", None),
            (Some("FR12345678901".into()), Some("FR".into()))
        );
        assert_eq!(
            normalize_nif("123456789", Some("us")),
            (Some("US123456789".into()), Some("US".into()))
        );
        assert_eq!(normalize_nif(" ", Some("ES")), (None, None));
        // Un país que no existe en la ISO 3166-1 no cuenta (ni pone prefijo al NIF).
        assert_eq!(
            normalize_nif("123456789", Some("ZZ")),
            (Some("123456789".into()), None)
        );
        assert_eq!(
            normalize_nif("123456789", Some("XI")),
            (Some("XI123456789".into()), Some("XI".into()))
        );
        // Texto en la columna del NIF (tickets): sin NIF, y sin inventar el país (VA, SI).
        for raw in ["VARIOS", "SIN NIF", "-", "varios"] {
            assert_eq!(normalize_nif(raw, None), (None, None), "{raw}");
        }
        assert_eq!(
            normalize_nif("VARIOS", Some("FR")),
            (None, Some("FR".into()))
        );
        // Grecia: país «GR», NIF-IVA con «EL» (nunca «GREL…»).
        for (raw, country) in [
            ("EL123456789", Some("GR")),
            ("123456789", Some("GR")),
            ("el 123456789", None),
            ("123456789", Some("EL")),
        ] {
            assert_eq!(
                normalize_nif(raw, country),
                (Some("EL123456789".into()), Some("GR".into())),
                "{raw} {country:?}"
            );
        }
    }
}
