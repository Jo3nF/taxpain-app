//! Modelo 303 (IVA, autoliquidación) del régimen general, trimestral.
//!
//! Base legal: Ley 37/1992 del IVA (arts. 71, 99, 164), RD 1624/1992 (art. 71) y Orden
//! HAC/3625/2003 y modificaciones (modelo 303). Diseño de registro: `DR303e26v101.xlsx`
//! (ejercicio 2026 y siguientes).
//!
//! Alcance (lo que no está aquí se avisa en `warnings`, nunca se ignora en silencio):
//! - Devengado: tipos 4/10/21 %, recargo de equivalencia 0,5/1,4/1,75/5,2 %, rectificativas en
//!   [14]/[15] y [25]/[26]. Información adicional: [59] (entregas intracomunitarias exentas y
//!   servicios no sujetos a empresarios de la UE con NIF-IVA), [60], [120] (resto de no sujetas
//!   por localización), [122].
//! - Deducible: operaciones interiores corrientes [28]/[29] y bienes de inversión [30]/[31];
//!   rectificaciones de deducciones en [40]/[41].
//! - Fuera de alcance por ahora: adquisiciones intracomunitarias e inversión del sujeto pasivo
//!   recibidas ([10]–[13], [36]–[39]), régimen simplificado, REDEME (mensual), tributación
//!   foral. Las importaciones (DUA) van en [32]–[35].
//! - Criterio de caja: las fechas de devengo y deducción las decide quien llama (al cobrar o
//!   pagar); aquí se marcan la identificación [15]/[16] y la información adicional [62]/[63]
//!   y [74]/[75] que se pasen en las casillas.
//!
//! `TODO(VERIFY)`:
//! - Rectificativas emitidas por diferencias en [14]/[15] (art. 80 y 89 LIVA): confirmar con las
//!   instrucciones del modelo que no van en las casillas de cada tipo.
//! - Base en [28]/[30] de facturas con IVA parcialmente deducible: se declara la parte de la base
//!   proporcional a la cuota deducible.
//! - Páginas obligatorias del fichero: se generan la 01, la 03 y la DID si hay IBAN.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::invoice::{Exemption, NotSubject, TaxBreakdown, VatTreatment};
use taxpain_core::{Money, Rate};
use time::Date;

use crate::dr::{DrError, PageWriter, Spec, envelope};
use crate::{DEVELOPER_NIF, Declarant, SOFTWARE_VERSION};

pub const DR_JSON: &str = include_str!("../../../fixtures/aeat/dr/303.json");

/// Casilla → importe (céntimos; [65] es un porcentaje con dos decimales).
pub type Boxes = BTreeMap<u16, Money>;

/// Desglose de IVA de una factura emitida (no anulada) con fecha de devengo en el trimestre.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssuedVat {
    pub number: String,
    pub date: Date,
    /// Rectificativa: sus importes (con signo) van a las casillas de modificación.
    pub rectification: bool,
    pub breakdown: Vec<TaxBreakdown>,
    /// Emitida con el régimen especial del criterio de caja (en el 390 va aparte).
    #[serde(default)]
    pub cash_basis: bool,
    /// Destinatario empresario de otro Estado miembro identificado con NIF-IVA (el mismo
    /// criterio que lleva la operación al 349): sus servicios no sujetos (N2) van en [59], no
    /// en [120].
    #[serde(default)]
    pub eu_business: bool,
    /// Venta a un consumidor de otro Estado miembro que tributa allí por la ventanilla única
    /// (OSS): sus líneas no sujetas (N2) van en la casilla informativa [123] (y su IVA, en el
    /// 369), no en [120].
    #[serde(default)]
    pub oss: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReceivedKind {
    /// Bienes y servicios corrientes, operaciones interiores.
    Corriente,
    /// Bienes de inversión (art. 108 LIVA), operaciones interiores.
    BienInversion,
}

/// Inversión del sujeto pasivo en una factura recibida sin IVA: el destinatario autorrepercute.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelfAssessedKind {
    /// Adquisición intracomunitaria de bienes o servicios (arts. 13, 15 y 84.Uno.2º LIVA):
    /// devengo [10]/[11]; deducción [36]/[37] o [38]/[39] si es bien de inversión.
    Intracomunitaria,
    /// Otras operaciones con inversión del sujeto pasivo (servicios de empresas de fuera de la
    /// UE, art. 84.Uno.2º.a; ISP interior, art. 84.Uno.2º.c–g): devengo [12]/[13]; deducción
    /// como operación interior [28]/[29] o [30]/[31].
    Otra,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfAssessed {
    pub kind: SelfAssessedKind,
    /// Cuota autorrepercutida (base × tipo).
    pub quota: Money,
    /// Adquisición intracomunitaria de bienes (si no, de servicios). Sólo lo distingue el 390.
    #[serde(default)]
    pub goods: bool,
    /// Qué se declara en este periodo: el devengo, la deducción o ambos.
    #[serde(default)]
    pub part: SelfAssessedPart,
}

/// Parte de una autorrepercusión que va en el periodo. El devengo ([10]–[13]) es el de la
/// operación (fecha de la factura: arts. 75.Uno.8º y 76 LIVA) y la deducción ([28]–[31] o
/// [36]–[39]) la del registro de la factura (art. 99 LIVA y 98.Dos: hace falta tenerla), así
/// que una factura de marzo registrada en abril se autorrepercute en el 1T y se deduce en el
/// 2T. Lo habitual es que coincidan (`Ambas`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelfAssessedPart {
    /// Devengo y deducción en el mismo periodo.
    #[default]
    Ambas,
    /// Sólo el devengo (se registró en un periodo posterior).
    Devengo,
    /// Sólo la deducción (se devengó en un periodo anterior).
    Deduccion,
}

/// IVA soportado deducible de una factura recibida revisada.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReceivedVat {
    pub number: String,
    pub date: Date,
    pub kind: ReceivedKind,
    pub rectification: bool,
    pub base: Money,
    pub vat: Money,
    /// Porcentaje deducible (0–100).
    pub deductible_pct: u8,
    /// Inversión del sujeto pasivo (la factura no lleva IVA).
    #[serde(default)]
    pub self_assessed: Option<SelfAssessed>,
    /// IVA de una importación (DUA): [32]/[33] o [34]/[35].
    #[serde(default)]
    pub import: bool,
    /// Con prorrata especial: a qué se destina la compra.
    #[serde(default)]
    pub prorata_use: ProrataUse,
    /// La compra es de un sector diferenciado (art. 9.1.º.c y 101 LIVA) y `deductible_pct` ya
    /// lleva el porcentaje de deducción de ese sector: no se le aplica la prorrata general.
    #[serde(default)]
    pub outside_prorata: bool,
}

/// Destino de una compra con prorrata especial (art. 106 LIVA).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProrataUse {
    /// Se usa a la vez en operaciones con y sin derecho a deducir: se deduce con la prorrata.
    #[default]
    Comun,
    /// Sólo en operaciones con derecho a deducir: se deduce entera.
    Plena,
    /// Sólo en operaciones sin derecho a deducir: no se deduce nada.
    Ninguna,
}

impl ProrataUse {
    pub fn code(self) -> &'static str {
        match self {
            ProrataUse::Comun => "comun",
            ProrataUse::Plena => "plena",
            ProrataUse::Ninguna => "ninguna",
        }
    }

    pub fn parse(s: &str) -> ProrataUse {
        match s {
            "plena" => ProrataUse::Plena,
            "ninguna" => ProrataUse::Ninguna,
            _ => ProrataUse::Comun,
        }
    }
}

/// Datos que no salen de las facturas.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Options303 {
    /// [110] cuotas a compensar pendientes de periodos anteriores.
    pub pending_compensation: Money,
    /// IBAN para domiciliar el ingreso (tipo U) o para la devolución (tipo D, sólo 4T).
    pub iban: Option<String>,
    /// Domiciliar el ingreso: sólo con esta marca un resultado a ingresar es «U». Tener un IBAN
    /// (p. ej. para devoluciones) no autoriza a cargar la cuenta.
    #[serde(default)]
    pub domiciliar: bool,
    /// En el 4T con resultado negativo: pedir la devolución (si no, se compensa).
    pub request_refund: bool,
    /// Exonerado del modelo 390 (sólo cuenta en el 4T).
    pub exempt_from_390: bool,
    /// Acogido al régimen especial del criterio de caja (identificación, nota 5).
    #[serde(default)]
    pub cash_basis: bool,
    /// Casillas fijadas a mano (sustituyen al cálculo; los totales se recalculan con ellas).
    #[serde(default)]
    pub adjustments: Boxes,
    /// Prorrata general (arts. 102 a 106 LIVA).
    #[serde(default)]
    pub prorata: Option<Prorata>,
    /// Exonerado del 390: datos de la página 04 del último periodo (actividades, 347 y
    /// volumen de operaciones). Sin ellos la página sale con los importes a cero.
    #[serde(default)]
    pub exonerado_390: Option<Exonerado390>,
    /// Regularización única de los bienes de inversión entregados en el periodo durante su
    /// periodo de regularización (art. 110 LIVA), en [43]. Se aplica aunque no haya prorrata
    /// (art. 110.Dos).
    #[serde(default)]
    pub capital_goods_sale_regularization: Money,
    /// Inscrito en el registro de devolución mensual (REDEME, art. 30 RIVA): el saldo a favor
    /// se puede pedir a devolver en cualquier periodo (art. 116 LIVA), no sólo en el último, y
    /// se marca en la identificación (campo 12 de la página 01).
    #[serde(default)]
    pub redeme: bool,
    /// Acogido voluntariamente al SII (campo 22 de la página 01; art. 62.6 RIVA).
    #[serde(default)]
    pub sii_voluntario: bool,
}

/// Apartado «Exclusivamente a cumplimentar en el último periodo de liquidación por aquellos
/// sujetos pasivos que queden exonerados de la declaración-resumen anual del IVA» (página 04
/// del DR 303 2026; Orden EHA/3111/2009 art. 1.2; instrucciones del 390): sustituye al 390 con
/// las actividades, la marca del 347 y el volumen de operaciones del ejercicio.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exonerado390 {
    /// Clave de la actividad principal (3, p. ej. «A03»), como el campo 26 del 390 (27 desde
    /// 2026).
    pub activity_code: String,
    /// Epígrafe del IAE de la actividad principal (4), como el campo 27 del 390 (28 desde 2026).
    pub iae: String,
    /// Ha efectuado operaciones por las que debe presentar el 347 (campo 18, «X»).
    pub files_347: bool,
    /// Volumen de operaciones del ejercicio (art. 121 LIVA), el mismo que el del 390.
    pub volume: Volumen390,
}

/// Volumen de operaciones del ejercicio (art. 121 LIVA) por su casilla del 390, que en la
/// página 04 del 303 se copia casilla a casilla: [99]→[80], [653]→[81], [103]→[93],
/// [104]→[94], [105]→[83], [110]→[84], [126]→[126], [125]→[125], [106]→[79], [107]→[99] y
/// [108]→[88]. Los regímenes que TaxPain no calcula ([127], [128], [86], [95]-[98]) van a cero en las dos
/// declaraciones.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Volumen390 {
    /// [99] del 390 → [80]: operaciones en régimen general.
    pub general: Money,
    /// [653] → [81]: operaciones del criterio de caja por la regla general del art. 75.
    pub cash_basis: Money,
    /// [103] → [93]: entregas intracomunitarias de bienes y servicios.
    pub intra: Money,
    /// [104] → [94]: exportaciones y otras exentas con derecho a deducción.
    pub exports: Money,
    /// [105] → [83]: exentas sin derecho a deducción.
    pub exempt: Money,
    /// [110] → [84]: no sujetas por reglas de localización.
    pub not_subject: Money,
    /// [126] → [126]: no sujetas por reglas de localización acogidas a la ventanilla única.
    #[serde(default)]
    pub oss: Money,
    /// [125] → [125]: sujetas con inversión del sujeto pasivo.
    pub reverse_charge: Money,
    /// [106] → [79]: entregas de inmuebles no habituales (ya sumadas arriba; se restan).
    #[serde(default)]
    pub real_estate: Money,
    /// [107] → [99]: entregas de bienes de inversión (ya sumadas arriba; se restan).
    #[serde(default)]
    pub capital_goods: Money,
}

impl Volumen390 {
    /// [88] = [80]+[81]+[93]+[94]+[83]+[84]+[125]+[126] (+ las casillas a cero) − [79] − [99]: igual
    /// que [108].
    pub fn total(&self) -> Money {
        self.general
            + self.cash_basis
            + self.intra
            + self.exports
            + self.exempt
            + self.not_subject
            + self.oss
            + self.reverse_charge
            - self.real_estate
            - self.capital_goods
    }
}

/// Prorrata: durante el año se deduce con el porcentaje provisional (el definitivo del año
/// anterior) y en el último periodo se regulariza con el definitivo del año [44]. Con la
/// especial (art. 106 LIVA) el porcentaje sólo se aplica a las compras de uso común.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prorata {
    /// Porcentaje provisional (0-100).
    pub provisional: u8,
    /// Prorrata especial (las compras se separan por su destino).
    #[serde(default)]
    pub special: bool,
    /// Último periodo: regularización de bienes de inversión de años anteriores [43]
    /// (arts. 107 a 110 LIVA).
    #[serde(default)]
    pub investment_regularization: Money,
    /// Sólo en el último periodo: porcentaje definitivo del año, importe total de las
    /// operaciones, las que dan derecho a deducir y la regularización [44].
    pub definitive: Option<u8>,
    pub operations: Money,
    pub deductible_operations: Money,
    pub regularization: Money,
    /// CNAE de la actividad (página 05).
    pub cnae: String,
}

/// Porcentaje de prorrata (art. 104.Dos LIVA): operaciones con derecho a deducir entre el total,
/// redondeado a la unidad superior.
pub fn prorata_percent(deductible_operations: Money, operations: Money) -> u8 {
    let total = operations.cents();
    if total <= 0 {
        return 100;
    }
    let num = i128::from(deductible_operations.cents().clamp(0, total)) * 100;
    let den = i128::from(total);
    let pct = (num + den - 1) / den;
    u8::try_from(pct.clamp(0, 100)).unwrap_or(100)
}

/// Casillas que salen de otras (totales, resultado y compensación): no se fijan a mano.
pub const DERIVED: [u16; 11] = [27, 45, 46, 64, 65, 66, 69, 71, 78, 87, 110];

/// Se puede fijar a mano: existe en el diseño de registro y no es un total.
pub fn adjustable(b: u16) -> bool {
    !DERIVED.contains(&b)
        && ["01", "03"]
            .iter()
            .any(|p| spec().page(p).is_ok_and(|pg| pg.field_for_box(b).is_ok()))
}

/// Tipo de declaración (nota 1 de la página 01).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeclarationType {
    /// I: ingreso.
    Ingreso,
    /// U: domiciliación del ingreso.
    Domiciliacion,
    /// C: solicitud de compensación.
    Compensacion,
    /// D: devolución.
    Devolucion,
    /// N: sin actividad / resultado cero.
    SinActividad,
}

impl DeclarationType {
    pub fn code(self) -> &'static str {
        match self {
            DeclarationType::Ingreso => "I",
            DeclarationType::Domiciliacion => "U",
            DeclarationType::Compensacion => "C",
            DeclarationType::Devolucion => "D",
            DeclarationType::SinActividad => "N",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Result303 {
    pub year: i32,
    pub quarter: u8,
    pub boxes: Boxes,
    pub declaration: DeclarationType,
    /// Sin ninguna operación en el periodo.
    pub no_activity: bool,
    /// Destinatario de operaciones acogidas al criterio de caja (identificación, nota 5).
    #[serde(default)]
    pub cash_basis_recipient: bool,
    /// Prorrata aplicada (para la página 05 del último periodo).
    #[serde(default)]
    pub prorata: Option<Prorata>,
    pub warnings: Vec<String>,
    /// Emitidas con un tipo de IVA o de recargo que no tiene casilla y con cuota (ver
    /// [`unmapped_rates`]): su IVA no está en [27]. Quien prepara el borrador lo bloquea.
    #[serde(default)]
    pub unmapped: Vec<String>,
}

impl Result303 {
    pub fn get(&self, b: u16) -> Money {
        self.boxes.get(&b).copied().unwrap_or(Money::ZERO)
    }

    /// [71] Resultado de la autoliquidación.
    pub fn result(&self) -> Money {
        self.get(71)
    }
}

/// Casillas (base, cuota) del devengado por tipo de IVA.
fn vat_boxes(rate: Rate) -> Option<(u16, u16)> {
    match rate.bp() {
        400 => Some((1, 3)),
        1000 => Some((4, 6)),
        2100 => Some((7, 9)),
        _ => None,
    }
}

/// Casillas (base, cuota) del recargo de equivalencia por tipo.
fn surcharge_boxes(rate: Rate) -> Option<(u16, u16)> {
    match rate.bp() {
        50 => Some((168, 170)),
        140 => Some((19, 21)),
        175 => Some((156, 158)),
        520 => Some((22, 24)),
        _ => None,
    }
}

fn add(b: &mut Boxes, k: u16, v: Money) {
    if !v.is_zero() {
        *b.entry(k).or_insert(Money::ZERO) += v;
    }
}

/// Emitidas (no rectificativas) con un tipo de IVA o de recargo de equivalencia sin casilla en
/// el 303 y con cuota: esa cuota no llegaría a [27]. No se inventa una casilla ni se redondea al
/// tipo más cercano (podría ser un error de lectura de la factura): quien prepara el borrador
/// lo bloquea con la lista para que se corrijan las facturas. Las rectificativas van a [14]/[15]
/// sea cual sea el tipo, y un tipo sin cuota (0 %) no pierde IVA.
pub fn unmapped_rates(issued: &[IssuedVat]) -> Vec<String> {
    let mut out = Vec::new();
    for inv in issued.iter().filter(|i| !i.rectification) {
        for t in &inv.breakdown {
            let VatTreatment::Taxed { rate } = t.vat else {
                continue;
            };
            if vat_boxes(rate).is_none() && !t.vat_amount.is_zero() {
                out.push(format!("{}: IVA del {rate} ({})", inv.number, t.vat_amount));
            }
            if let Some(sr) = t.surcharge_rate
                && surcharge_boxes(sr).is_none()
                && !t.surcharge_amount.is_zero()
            {
                out.push(format!(
                    "{}: recargo de equivalencia del {sr} ({})",
                    inv.number, t.surcharge_amount
                ));
            }
        }
    }
    out
}

const DEVENGADO: [u16; 15] = [152, 167, 3, 155, 6, 9, 11, 13, 15, 158, 170, 18, 21, 24, 26];
const DEDUCIBLE: [u16; 10] = [29, 31, 33, 35, 37, 39, 41, 42, 43, 44];

/// Calcula el 303 del periodo `quarter` de `year`: trimestre (1–4) o mes (101–112, ver
/// [`crate::periodo`]).
pub fn compute(
    year: i32,
    quarter: u8,
    issued: &[IssuedVat],
    received: &[ReceivedVat],
    opts: &Options303,
) -> Result303 {
    let mut b = Boxes::new();
    let mut warnings = Vec::new();

    for inv in issued {
        for t in &inv.breakdown {
            match t.vat {
                VatTreatment::Taxed { rate } => {
                    if inv.rectification {
                        add(&mut b, 14, t.base);
                        add(&mut b, 15, t.vat_amount);
                    } else if let Some((kb, kc)) = vat_boxes(rate) {
                        add(&mut b, kb, t.base);
                        add(&mut b, kc, t.vat_amount);
                    } else {
                        warnings.push(format!(
                            "{}: tipo de IVA del {rate} sin casilla en este cálculo; revísalo a mano",
                            inv.number
                        ));
                    }
                    if let Some(sr) = t.surcharge_rate {
                        if inv.rectification {
                            add(&mut b, 25, t.base);
                            add(&mut b, 26, t.surcharge_amount);
                        } else if let Some((kb, kc)) = surcharge_boxes(sr) {
                            add(&mut b, kb, t.base);
                            add(&mut b, kc, t.surcharge_amount);
                        } else {
                            warnings.push(format!(
                                "{}: recargo de equivalencia del {sr} sin casilla",
                                inv.number
                            ));
                        }
                    }
                }
                VatTreatment::ReverseCharge => add(&mut b, 122, t.base),
                VatTreatment::Exempt(Exemption::E5) => add(&mut b, 59, t.base),
                VatTreatment::Exempt(Exemption::E2 | Exemption::E3 | Exemption::E4) => {
                    add(&mut b, 60, t.base);
                }
                // Exentas interiores (art. 20) y otras: no tienen casilla en el 303.
                VatTreatment::Exempt(Exemption::E1 | Exemption::E6) => {}
                // No sujetas por reglas de localización: los servicios a un empresario de otro
                // Estado miembro con NIF-IVA (art. 69.Uno.1º LIVA; los autorrepercute él) son
                // «entregas intracomunitarias de bienes y servicios» [59] y se cruzan con la
                // clave S del 349; el resto (clientes de fuera de la UE, arts. 69 y 70) va en
                // [120]. Instrucciones del 303, información adicional, casillas 59 y 120.
                VatTreatment::NotSubject(NotSubject::N2) if inv.eu_business => {
                    add(&mut b, 59, t.base);
                }
                // Ventanilla única: «Operaciones no sujetas por reglas de localización acogidas
                // a los regímenes especiales de ventanilla única» [123] (DR303e26v101, página
                // 03, campo 9); la [120] las excluye.
                VatTreatment::NotSubject(NotSubject::N2) if inv.oss => add(&mut b, 123, t.base),
                VatTreatment::NotSubject(NotSubject::N2) => add(&mut b, 120, t.base),
                VatTreatment::NotSubject(NotSubject::N1) => {}
            }
        }
    }

    let mut without_vat = 0usize;
    // Deducciones a las que se aplica el porcentaje de prorrata (todas con la general; las de
    // uso común con la especial), aparte hasta el final.
    let mut common = Boxes::new();
    for r in received {
        let pct = Rate::percent(i32::from(r.deductible_pct.min(100)));
        let (prorated, excluded) = match &opts.prorata {
            None => (false, false),
            Some(_) if r.outside_prorata => (false, false),
            Some(p) if !p.special => (true, false),
            Some(_) => (
                r.prorata_use == ProrataUse::Comun,
                r.prorata_use == ProrataUse::Ninguna,
            ),
        };
        if let Some(sa) = r.self_assessed {
            // Devengo por el total y deducción por la parte deducible.
            // Rectificaciones (abonos del proveedor): modificación de bases y cuotas [14]/[15] y
            // rectificación de deducciones [40]/[41], como en el resto de recibidas.
            let (dev, ded) = match (sa.kind, r.kind) {
                _ if r.rectification => ((14, 15), (40, 41)),
                (SelfAssessedKind::Intracomunitaria, ReceivedKind::Corriente) => {
                    ((10, 11), (36, 37))
                }
                (SelfAssessedKind::Intracomunitaria, ReceivedKind::BienInversion) => {
                    ((10, 11), (38, 39))
                }
                (SelfAssessedKind::Otra, ReceivedKind::Corriente) => ((12, 13), (28, 29)),
                (SelfAssessedKind::Otra, ReceivedKind::BienInversion) => ((12, 13), (30, 31)),
            };
            if sa.part != SelfAssessedPart::Deduccion {
                add(&mut b, dev.0, r.base);
                add(&mut b, dev.1, sa.quota);
            }
            if excluded || sa.part == SelfAssessedPart::Devengo {
                continue;
            }
            let t = if prorated { &mut common } else { &mut b };
            add(t, ded.0, r.base.apply_rate(pct));
            add(t, ded.1, sa.quota.apply_rate(pct));
            continue;
        }
        // Sin cuota (compras exentas, adquisiciones intracomunitarias o con inversión del sujeto
        // pasivo sin marcar): no hay IVA soportado que deducir en [28]–[41]. Como en el 390.
        if r.vat.is_zero() {
            without_vat += 1;
            continue;
        }
        if excluded {
            continue; // Prorrata especial: compra sólo para operaciones sin derecho a deducir.
        }
        let vat = r.vat.apply_rate(pct);
        let base = r.base.apply_rate(pct);
        if vat.is_zero() && !r.vat.is_zero() {
            continue; // IVA no deducible: no se declara.
        }
        let (kb, kc) = match (r.rectification, r.import, r.kind) {
            (true, _, _) => (40, 41),
            (false, true, ReceivedKind::Corriente) => (32, 33),
            (false, true, ReceivedKind::BienInversion) => (34, 35),
            (false, false, ReceivedKind::Corriente) => (28, 29),
            (false, false, ReceivedKind::BienInversion) => (30, 31),
        };
        let t = if prorated { &mut common } else { &mut b };
        add(t, kb, base);
        add(t, kc, vat);
    }

    if without_vat > 0 {
        warnings.push(format!(
            "{without_vat} factura(s) recibida(s) sin IVA no se declaran. Si son adquisiciones \
             intracomunitarias o con inversión del sujeto pasivo (p. ej. servicios de empresas \
             de otro país), márcalo al revisarlas para autorrepercutir el IVA ([10]–[13] y \
             [36]–[39])."
        ));
    }
    for (k, label) in [(1u16, "[01]"), (4, "[04]"), (7, "[07]")] {
        if b.get(&k).is_some_and(|v| v.cents() < 0) {
            warnings.push(format!(
                "La base {label} es negativa: las rectificativas deberían ir en [14]"
            ));
        }
    }

    // Prorrata: las deducciones sujetas a ella, por el porcentaje provisional; en el último
    // periodo, la regularización con el definitivo [44] y la de bienes de inversión [43].
    if let Some(p) = &opts.prorata {
        let pct = Rate::percent(i32::from(p.provisional.min(100)));
        for (k, v) in &common {
            add(&mut b, *k, v.apply_rate(pct));
        }
        if crate::periodo::ultimo(quarter) {
            add(&mut b, 44, p.regularization);
            add(&mut b, 43, p.investment_regularization);
        }
        warnings.push(format!(
            "Prorrata {} del {} % en las deducciones{}",
            if p.special { "especial" } else { "general" },
            p.provisional,
            if p.special { " de uso común" } else { "" }
        ));
    }

    // Entrega de bienes de inversión durante su periodo de regularización (art. 110 LIVA).
    if !opts.capital_goods_sale_regularization.is_zero() {
        add(&mut b, 43, opts.capital_goods_sale_regularization);
        warnings.push(format!(
            "[43] incluye {} de regularización por la venta de bienes de inversión (art. 110 LIVA)",
            opts.capital_goods_sale_regularization
        ));
    }

    // Casillas corregidas a mano: sustituyen al cálculo antes de los totales.
    for (k, v) in &opts.adjustments {
        if adjustable(*k) {
            b.insert(*k, *v);
            warnings.push(format!("Casilla [{k:02}] fijada a mano: {v}"));
        } else {
            warnings.push(format!(
                "La casilla [{k:02}] no se puede fijar a mano (es un total o no existe)"
            ));
        }
    }

    let sum = |b: &Boxes, ks: &[u16]| ks.iter().filter_map(|k| b.get(k)).copied().sum::<Money>();
    let devengado = sum(&b, &DEVENGADO);
    let deducible = sum(&b, &DEDUCIBLE);
    b.insert(27, devengado);
    b.insert(45, deducible);
    let r46 = devengado - deducible;
    b.insert(46, r46);
    let fijada = |b: &Boxes, k: u16| b.get(&k).copied().unwrap_or(Money::ZERO);
    // [64] = [46] + [58] (régimen simplificado) + [76] (regularización del art. 80.Cinco.5ª):
    // TaxPain no calcula esas dos, pero entran si se fijan a mano.
    let r64 = r46 + fijada(&b, 58) + fijada(&b, 76);
    b.insert(64, r64);
    // [65] territorio común: 100 % al Estado; [66] = [64] × [65].
    b.insert(65, Money::from_cents(10_000));
    b.insert(66, r64);
    // Lo que se suma al resultado antes de compensar (fórmula de [69] del diseño de registro):
    // [77] IVA a la importación pendiente de ingreso, [68] regularización anual (tributación
    // conjunta con las Haciendas Forales) y [108] otros ajustes de una rectificativa.
    let ajustes = fijada(&b, 77) + fijada(&b, 68) + fijada(&b, 108);

    // Compensación de cuotas de periodos anteriores hasta dejar el resultado en cero.
    let pending = opts.pending_compensation.max(Money::ZERO);
    // En el último periodo, si se pide la devolución, se aplica todo lo pendiente ([78] = [110])
    // para que se devuelva el saldo a favor completo (art. 115.Uno LIVA; instrucciones del 303).
    let refund_period = crate::periodo::ultimo(quarter) || opts.redeme;
    let refund_all = refund_period && opts.request_refund;
    let antes = r64 + ajustes;
    let applied = if refund_all {
        pending
    } else if antes.cents() > 0 {
        pending.min(antes)
    } else {
        Money::ZERO
    };
    add(&mut b, 110, pending);
    add(&mut b, 78, applied);
    add(&mut b, 87, pending - applied);
    // [69] = [66] + [77] − [78] + [68] + [108]; [71] = [69] − [70] + [109] − [112] (diseño de
    // registro: [70] lo ingresado por la autoliquidación anterior de una complementaria, [109]
    // devoluciones ya acordadas, [112] IVA del depósito distinto del aduanero).
    let r69 = antes - applied;
    b.insert(69, r69);
    let r71 = r69 - fijada(&b, 70) + fijada(&b, 109) - fijada(&b, 112);
    b.insert(71, r71);

    let no_activity = issued.is_empty() && received.is_empty();
    let declaration = match r71.cents() {
        c if c > 0 && opts.domiciliar && opts.iban.is_some() => DeclarationType::Domiciliacion,
        c if c > 0 => DeclarationType::Ingreso,
        0 => DeclarationType::SinActividad,
        _ if refund_period && opts.request_refund => DeclarationType::Devolucion,
        _ => DeclarationType::Compensacion,
    };
    if declaration == DeclarationType::Devolucion && opts.iban.is_none() {
        warnings.push("Para pedir la devolución hace falta un IBAN".into());
    }

    b.retain(|k, v| !v.is_zero() || matches!(k, 27 | 45 | 46 | 64 | 65 | 66 | 69 | 71));
    Result303 {
        year,
        quarter,
        boxes: b,
        declaration,
        no_activity,
        cash_basis_recipient: false,
        prorata: opts.prorata.clone(),
        warnings,
        unmapped: unmapped_rates(issued),
    }
}

pub fn spec() -> &'static Spec {
    static SPEC: std::sync::OnceLock<Spec> = std::sync::OnceLock::new();
    SPEC.get_or_init(|| Spec::parse(DR_JSON))
}

/// Fichero del 303 (texto; codificar con [`crate::dr::to_latin1`]).
pub fn file(r: &Result303, who: &Declarant, opts: &Options303) -> Result<String, DrError> {
    let s = spec();
    let last = crate::periodo::ultimo(r.quarter);
    let period = crate::periodo::codigo(r.quarter);

    // Página 01: identificación y liquidación del régimen general.
    let p1 = s.page("01")?;
    let mut w = PageWriter::new(p1);
    w.text(6, r.declaration.code())?
        .text(7, &who.nif)?
        .text(8, &who.full_name())?
        .text(9, &r.year.to_string())?
        .text(10, &period)?;
    // Identificación (nota 5): «2» = NO; régimen simplificado «3» = sólo régimen general.
    for n in [11, 12, 14, 15, 16, 19, 22] {
        w.text(n, "2")?;
    }
    if opts.redeme {
        w.text(12, "1")?;
    }
    if opts.sii_voluntario {
        w.text(22, "1")?;
    }
    // Criterio de caja: acogido [15] y destinatario de operaciones del régimen [16].
    if opts.cash_basis {
        w.text(15, "1")?;
    }
    if r.cash_basis_recipient {
        w.text(16, "1")?;
    }
    w.text(13, "3")?;
    // Prorrata especial (nota 6): blanco salvo en el último periodo, «2» = NO.
    for n in [17, 18] {
        if last {
            w.text(n, "2")?;
        } else {
            w.raw(n, " ")?;
        }
    }
    // Exonerado del 390 (nota 4) y volumen de operaciones (nota 3): «0» salvo en el 4T. El
    // campo 24 es el volumen **anual** de operaciones distinto de cero (art. 121 LIVA; DR 303
    // 2026, página 01, nota 3): el de la página 04 ([88]), no el del trimestre. Sin los datos
    // de la página 04, se supone con volumen si el trimestre tiene operaciones.
    let annual_volume = match &opts.exonerado_390 {
        Some(e) => !e.volume.total().is_zero(),
        None => !r.no_activity,
    };
    let (exempt, volume) = match (last, opts.exempt_from_390) {
        (false, _) => ("0", "0"),
        (true, false) => ("2", "0"),
        (true, true) => ("1", if annual_volume { "1" } else { "2" }),
    };
    w.text(23, exempt)?.text(24, volume)?;
    // Gasolinas (nota 8): «0» en los periodos 1T a 4T y 01; del 02 en adelante, «1» o «2» (TaxPain
    // no calcula el pago a cuenta de gasolinas [112]: «2» = NO).
    let mensual_tras_enero = crate::periodo::mes(r.quarter).is_some_and(|m| m >= 2);
    w.text(25, if mensual_tras_enero { "2" } else { "0" })?;
    for (box_, v) in &r.boxes {
        if p1.field_for_box(*box_).is_ok() {
            w.amount_box(*box_, *v)?;
        }
    }
    // [169] tipo del recargo del 0,5 %: el DR lo indica («"00050". Nota 7») pero no como
    // constante, así que se escribe aquí cuando hay base en [168].
    if !r.get(168).is_zero() {
        w.raw(54, "00050")?;
    }
    let page1 = w.finish()?;

    // Página 03: información adicional y resultado.
    let p3 = s.page("03")?;
    let mut w = PageWriter::new(p3);
    for (box_, v) in &r.boxes {
        if p3.field_for_box(*box_).is_ok() {
            w.amount_box(*box_, *v)?;
        }
    }
    if r.no_activity {
        w.text(30, "X")?;
    }
    let page3 = w.finish()?;

    let mut pages = vec![page1, page3];
    // Página 04 (último periodo, exonerados del 390): actividades, marca del 347 y volumen de
    // operaciones, el apartado que sustituye al 390 (Orden EHA/3111/2009 art. 1.2;
    // instrucciones del 303 2026, apartado de exonerados, y del 390/2025, pág. 2).
    if last && opts.exempt_from_390 {
        let e = opts.exonerado_390.clone().unwrap_or_default();
        let mut w = PageWriter::new(s.page("04")?);
        // Clave y epígrafe de la actividad principal. `TODO(VERIFY)`: las «otras» actividades
        // (campos 8-17) no se llevan; TaxPain guarda una sola (instrucciones del 390, apdo. 1).
        w.text(6, &e.activity_code.chars().take(3).collect::<String>())?
            .text(7, &e.iae.chars().take(4).collect::<String>())?;
        if e.files_347 {
            w.text(18, "X")?;
        }
        // Tributación por razón de territorio ([89]-[92] y [107] «Territorio común»): sólo para
        // quien tributa conjuntamente al Estado y a las Diputaciones Forales; en régimen común
        // se deja en blanco, como el apartado 8 del 390 («lo cumplimentarán, exclusivamente,
        // los sujetos pasivos que tributen conjuntamente a la Administración del Estado y a
        // alguna de las Diputaciones Forales», Instrucciones_modelo_390-2025.pdf) que este
        // apartado sustituye. El DR 303 2026 (DR303e26v101.xlsx, página 04) define [107] como
        // numérico de 3 enteros y 2 decimales sin nota de obligatoriedad (cotejado el
        // 2026-10-27). El 100 % del régimen común sólo se consigna en la [65] de la página 03.
        let vol = &e.volume;
        w.amount_box(80, vol.general)?
            .amount_box(81, vol.cash_basis)?
            .amount_box(93, vol.intra)?
            .amount_box(94, vol.exports)?
            .amount_box(83, vol.exempt)?
            .amount_box(84, vol.not_subject)?
            .amount_box(126, vol.oss)?
            .amount_box(125, vol.reverse_charge)?
            .amount_box(79, vol.real_estate)?
            .amount_box(99, vol.capital_goods)?
            .amount_box(88, vol.total())?;
        pages.push(w.finish()?);
    }
    // Página 05 (último periodo con prorrata): operaciones y porcentaje definitivo.
    if last
        && let Some(p) = &r.prorata
        && let Some(def) = p.definitive
    {
        let mut w = PageWriter::new(s.page("05")?);
        w.text(
            6,
            &p.cnae
                .chars()
                .filter(char::is_ascii_digit)
                .take(4)
                .collect::<String>(),
        )?;
        w.amount_box(501, p.operations)?
            .amount_box(502, p.deductible_operations)?
            .text(9, if p.special { "E" } else { "G" })?
            .raw(10, &format!("{:03}00", def.min(100)))?;
        pages.push(w.finish()?);
    }
    if let Some(iban) = &opts.iban
        && matches!(
            r.declaration,
            DeclarationType::Domiciliacion | DeclarationType::Devolucion
        )
    {
        let mut w = PageWriter::new(s.page("DID")?);
        w.text(6, iban)?;
        pages.push(w.finish()?);
    }
    Ok(envelope(
        "303",
        r.year,
        &period,
        SOFTWARE_VERSION,
        DEVELOPER_NIF,
        &pages,
    ))
}
