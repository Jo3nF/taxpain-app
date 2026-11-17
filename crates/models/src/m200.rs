//! Modelo 200: Impuesto sobre Sociedades, liquidación básica y fichero del DR 2025 para
//! **importar en Sociedades WEB** (que lo completa, valida y presenta: TaxPain no presenta el 200).
//!
//! Base legal: Ley 27/2014 (LIS): art. 10 (base imponible a partir del resultado contable),
//! art. 26 (compensación de bases negativas), art. 29 (tipos de gravamen, en la redacción de la
//! Ley 7/2024), art. 30 (cuota íntegra), art. 41 (retenciones y pagos fraccionados).
//! Casillas del DR 2025 (`DR200e25.xls`, 77 páginas): [00500], [00301], [00302], [00501],
//! [01815], [00417], [00418], [00550], [01032], [00547], [00552], [00558], [00562], [00582], [00592],
//! [01766], [00599], [00601], [00603], [00605], [00611], [01586].
//!
//! Liquidación completa del caso de una sociedad que no esté en un régimen especial: las
//! correcciones al resultado contable con su casilla oficial (catálogo en
//! [`crate::m200_catalogo`]), la reserva de capitalización calculada (art. 25), las bases
//! negativas por ejercicio de origen (art. 26 y página 15), la reserva de nivelación (art. 105),
//! la entidad patrimonial (art. 5.2: sin tipos reducidos ni incentivos de reducida dimensión),
//! bonificaciones, deducción por doble imposición internacional (arts. 31 y 32) y otras
//! deducciones (tecleadas) y las retenciones desglosadas. Los regímenes especiales (grupos
//! fiscales, cooperativas, ZEC, SOCIMI…) se completan en Sociedades WEB. El fichero
//! ([`file`]) lleva la identificación, las participadas (página 02), el balance y la cuenta de
//! pérdidas y ganancias (páginas 03–08, con `fixtures/aeat/dr/200-cuentas.json`), la
//! liquidación (12, 13, 14 y 14 bis), el detalle de bases negativas (15, sólo con el diseño de
//! 2025) y la página de ingreso o devolución.
//!
//! Tipos (art. 29.1 LIS y DT 44.ª, redacción de la Ley 7/2024; `docs/research/MODELO_200.md`):
//! general 25 %; cifra de negocios < 1 M€: 2025 21/22 %, 2026 19/21 %, 2027+ 17/20 % (hasta
//! 50.000 € de base / resto); resto de reducida dimensión: 2025 24 %, 2026 23 %, 2027 22 %,
//! 2028 21 %, 2029+ 20 %; nueva creación: 15 %.
//!
//! Orden de la base (arts. 25 y 26 LIS): la reserva de capitalización se limita al 20 % de la
//! base previa (25 % si la cifra de negocios es < 1 M€); el límite de compensación de bases
//! negativas (1 M€ siempre y, por encima, el 70 % de la base) se calcula sobre la base **antes**
//! de la reserva; las grandes empresas, al 50 % o 25 % (DA 15.ª), y sin límite en los tres
//! primeros periodos con base positiva de las de nueva creación (art. 26.3).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use taxpain_core::{Money, Rate};
use time::Date;

use crate::dr::{DrError, PageWriter, Spec, envelope};
use crate::m303::Boxes;
use crate::{DEVELOPER_NIF, Declarant, SOFTWARE_VERSION};

pub const DR_JSON: &str = include_str!("../../../fixtures/aeat/dr/200.json");
const ACCOUNTS_JSON: &str = include_str!("../../../fixtures/aeat/dr/200-cuentas.json");

pub fn spec() -> &'static Spec {
    static SPEC: std::sync::OnceLock<Spec> = std::sync::OnceLock::new();
    SPEC.get_or_init(|| Spec::parse(DR_JSON))
}

/// Casillas de importe distintas de cero del fichero de un 200 (el inverso de [`file`]), con el
/// diseño de registro de 2025. Para otros ejercicios las posiciones pueden haber cambiado: quien
/// lo use tiene que comprobar que lo leído es coherente (`taxpain-app::lectura_justificante`).
///
/// En el 200 un mismo número de casilla sale en varias páginas con otro sentido (la [00562] es la
/// cuota íntegra en la liquidación y la distribución de dividendos en el estado de cambios del
/// patrimonio neto): se leen primero las páginas de la liquidación (12 a 15 y la del documento
/// de ingreso o devolución) y después las demás; la primera vez que sale una casilla es la que
/// vale.
pub fn read_boxes(text: &str) -> BTreeMap<u16, Money> {
    static ORDERED: std::sync::OnceLock<Spec> = std::sync::OnceLock::new();
    let ordered = ORDERED.get_or_init(|| liquidacion_primero(spec()));
    crate::dr::read_boxes(ordered, text, |_| None)
}

/// El diseño con las páginas de la liquidación delante (ver [`read_boxes`]).
fn liquidacion_primero(s: &Spec) -> Spec {
    let liquidacion = |sheet: &str| {
        ["200012", "200013", "200014", "200015", "200DID"]
            .iter()
            .any(|p| sheet.contains(p))
    };
    let (first, rest): (Vec<_>, Vec<_>) =
        s.pages.iter().cloned().partition(|p| liquidacion(&p.sheet));
    Spec {
        pages: first.into_iter().chain(rest).collect(),
        ..s.clone()
    }
}

/// Casillas del fichero de un 200 del ejercicio `year`, con el diseño de registro de ese
/// ejercicio (`dr_versions`: el 200 cambia cada año). `None` si TaxPain no tiene ese diseño o si
/// alguna página del fichero no mide lo que dice el diseño (entonces es de otra versión y no se
/// puede leer por posiciones).
pub fn read_boxes_of_year(text: &str, year: i32) -> Option<BTreeMap<u16, Money>> {
    let design = crate::dr_versions::design("200", year, "0A")?;
    let spec = design.spec();
    if !crate::dr::pages_fit(spec, text) {
        return None;
    }
    static ORDERED: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<&'static str, &'static Spec>>,
    > = std::sync::OnceLock::new();
    let ordered: &'static Spec = {
        let mut c = ORDERED
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        c.entry(design.name)
            .or_insert_with(|| Box::leak(Box::new(liquidacion_primero(spec))))
    };
    Some(crate::dr::read_boxes(ordered, text, |_| None))
}

/// Partida de cuentas anuales (`balance:A.I`, `pyg:7`…) → (página, casilla) del 200, por modelo.
fn account_boxes(model: &str) -> BTreeMap<String, (String, u16)> {
    let all: serde_json::Value =
        serde_json::from_str(ACCOUNTS_JSON).unwrap_or(serde_json::Value::Null);
    all.get(model)
        .and_then(|m| m.as_object())
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| {
                    let page = v.get(0)?.as_str()?.to_owned();
                    let b = u16::try_from(v.get(1)?.as_u64()?).ok()?;
                    Some((k.clone(), (page, b)))
                })
                .collect()
        })
        .unwrap_or_default()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaxRegime {
    /// Tipo general (25 %).
    #[default]
    General,
    /// Reducida dimensión con cifra de negocios inferior a 1 millón: dos tramos (hasta 50.000 €
    /// y el resto) que bajan cada ejercicio; ver `small_company_rates`.
    Micro,
    /// Resto de entidades de reducida dimensión: tipo único por ejercicio (24 % en 2025, 23 % en
    /// 2026…); ver `small_company_rates`.
    ReducidaDimension,
    /// Entidad de nueva creación: 15 % sobre toda la base (art. 29.1 LIS, segundo párrafo).
    NuevaCreacion,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Input200 {
    /// Resultado de la cuenta de pérdidas y ganancias (después del IS contabilizado).
    pub accounting_result: Money,
    /// Gasto por IS contabilizado (cuenta 630): corrección en aumento [00301].
    pub tax_expense: Money,
    /// Ingreso por IS contabilizado: corrección en disminución [00302].
    pub tax_income: Money,
    /// Multas, sanciones y recargos (no deducibles, art. 15.c LIS) [01815].
    #[serde(default)]
    pub fines: Money,
    /// Otras correcciones en aumento (liberalidades, donativos…); con las multas suman [00417].
    pub other_increases: Money,
    /// Otras correcciones en disminución [00418].
    pub other_decreases: Money,
    /// Diferencias entre amortización contable y fiscal (art. 12.1 LIS): aumento [00303] si la
    /// contable supera a la fiscal, disminución [00304] al revés. Suman en [00417]/[00418].
    #[serde(default)]
    pub depreciation_increase: Money,
    #[serde(default)]
    pub depreciation_decrease: Money,
    /// Reserva de capitalización [01032] (art. 25 LIS).
    pub capitalization_reserve: Money,
    /// Cifra de negocios de los 12 meses anteriores menor de 1 M€: el tope de la reserva de
    /// capitalización es el 25 % de la base (art. 25.1 LIS), también con el tipo de nueva creación
    /// o sin régimen puesto.
    #[serde(default)]
    pub turnover_under_1m: bool,
    /// Importe neto de la cifra de negocios de los 12 meses anteriores al inicio del periodo
    /// (`None` si no se sabe): con 20 M€ o más, la compensación de bases negativas se limita al
    /// 50 % o al 25 % (DA 15.ª LIS).
    #[serde(default)]
    pub turnover: Option<Money>,
    /// Bases imponibles negativas pendientes de ejercicios anteriores.
    pub pending_losses: Money,
    pub regime: TaxRegime,
    /// Retenciones e ingresos a cuenta soportados [01766].
    pub withholdings: Money,
    /// Pagos fraccionados (202) del ejercicio: 1P, 2P y 3P.
    pub installments: [Money; 3],
    /// Correcciones al resultado contable con su casilla del catálogo
    /// ([`crate::m200_catalogo::CORRECCIONES`]); se suman a las de arriba.
    #[serde(default)]
    pub ajustes: Vec<Importe>,
    /// Bases imponibles negativas pendientes por ejercicio de origen. Si hay alguna, sustituyen
    /// a `pending_losses` y se compensan de la más antigua a la más reciente.
    #[serde(default)]
    pub bins: Vec<Arrastre>,
    /// Entidad de nueva creación en uno de sus tres primeros periodos con base positiva: sin el
    /// límite del 70 % ni el del millón (art. 26.3 LIS; clave [00070] de la página 01).
    #[serde(default)]
    pub bin_nueva_creacion: bool,
    /// Reserva de capitalización calculada (art. 25 LIS). Sin ella, vale
    /// `capitalization_reserve` tal cual.
    #[serde(default)]
    pub capitalizacion: Option<Capitalizacion>,
    /// Reserva de nivelación (art. 105 LIS).
    #[serde(default)]
    pub nivelacion: Option<Nivelacion>,
    /// Entidad patrimonial (art. 5.2 LIS): ni tipos reducidos ni incentivos de reducida
    /// dimensión (arts. 29.1 y 101.1 LIS); clave [00066] de la página 01.
    #[serde(default)]
    pub patrimonial: bool,
    /// Bonificaciones, deducciones por doble imposición y otras deducciones de la cuota
    /// ([`crate::m200_catalogo::CUOTA`]).
    #[serde(default)]
    pub cuota: Vec<Importe>,
    /// Desglose de las retenciones (1785, 1787, 1793, 1795, 1798). Si hay, [01766] es su suma.
    #[serde(default)]
    pub retenciones: Vec<Importe>,
}

/// Importe de una casilla.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Importe {
    pub casilla: u16,
    pub importe: Money,
}

/// Importe pendiente de un ejercicio de origen (bases negativas, reservas).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arrastre {
    pub origen: i32,
    pub importe: Money,
}

/// Datos de la reserva de capitalización (art. 25 LIS).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capitalizacion {
    /// Incremento de los fondos propios del ejercicio (art. 25.2 LIS).
    pub incremento: Money,
    /// Porcentaje del incremento. Sin él, el del art. 25.1 LIS (BOE-A-2014-12328) para el año en
    /// que empieza el periodo: 10 % hasta 2023, 15 % en 2024 (art. 4.2 del RDL 4/2024) y 20 %
    /// desde 2025 (DF 8.ª.2 de la Ley 7/2024). Desde 2025 sube al 23 %, al 26,5 % o al 30 % si la
    /// plantilla media crece entre un 2 y un 5 %, entre un 5 y un 10 % o más de un 10 % y se
    /// mantiene tres años: se pone a mano (el 26,5 % no cabe en este campo entero).
    #[serde(default)]
    pub porcentaje: Option<u8>,
    /// Reducciones de ejercicios anteriores no aplicadas por falta de base.
    #[serde(default)]
    pub pendientes: Vec<Arrastre>,
}

/// Datos de la reserva de nivelación (art. 105 LIS).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Nivelacion {
    /// Minoración que se quiere aplicar en el ejercicio (como mucho, el 10 % de la base y 1 M€).
    pub minorar: Money,
    /// Minoraciones de los cinco ejercicios anteriores pendientes de adicionar.
    #[serde(default)]
    pub pendientes: Vec<Arrastre>,
}

/// Lo que había de un ejercicio de origen, lo aplicado en esta liquidación y lo que queda.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetalleArrastre {
    pub origen: i32,
    pub inicio: Money,
    pub aplicado: Money,
    pub pendiente: Money,
}

/// Resultado de la reserva de capitalización.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetalleCapitalizacion {
    pub porcentaje: u8,
    /// Derecho generado en el ejercicio (incremento × porcentaje).
    pub derecho: Money,
    /// Tope de la reducción (20 % o 25 % de la base previa; 10 % antes de 2025).
    pub tope: Money,
    pub aplicado: Money,
    /// Reserva indisponible que hay que dotar por la reducción (art. 25.1.b LIS).
    pub a_dotar: Money,
    pub detalle: Vec<DetalleArrastre>,
    /// Reducciones de más de dos ejercicios atrás que ya no se pueden aplicar.
    pub caducado: Money,
}

/// Resultado de la reserva de nivelación.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetalleNivelacion {
    pub minorado: Money,
    pub tope: Money,
    pub adicionado: Money,
    /// Reserva indisponible que hay que dotar por la minoración (art. 105.4 LIS).
    pub a_dotar: Money,
    pub detalle: Vec<DetalleArrastre>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Result200 {
    pub year: i32,
    pub boxes: Boxes,
    pub warnings: Vec<String>,
    /// Régimen de tipos aplicado (el general si es entidad patrimonial).
    #[serde(default)]
    pub regimen: TaxRegime,
    #[serde(default)]
    pub patrimonial: bool,
    #[serde(default)]
    pub bin_nueva_creacion: bool,
    /// Bases negativas por ejercicio de origen (vacío si sólo se dio un total).
    #[serde(default)]
    pub bins: Vec<DetalleArrastre>,
    /// Base imponible negativa generada en el ejercicio.
    #[serde(default)]
    pub bin_generada: Money,
    #[serde(default)]
    pub capitalizacion: Option<DetalleCapitalizacion>,
    #[serde(default)]
    pub nivelacion: Option<DetalleNivelacion>,
    /// Deducción por doble imposición internacional no aplicada por falta de cuota (art. 31.6).
    #[serde(default)]
    pub di_pendiente: Money,
    /// Otras deducciones no aplicadas por falta de cuota.
    #[serde(default)]
    pub deducciones_pendientes: Money,
}

impl Result200 {
    pub fn get(&self, b: u16) -> Money {
        self.boxes.get(&b).copied().unwrap_or(Money::ZERO)
    }

    /// [01586] Resultado de la autoliquidación.
    pub fn result(&self) -> Money {
        self.get(1586)
    }
}

const FIFTY_K: Money = Money::from_cents(5_000_000);
const ONE_M: Money = Money::from_cents(100_000_000);
/// Umbrales de cifra de negocios de la DA 15.ª LIS (20 y 60 M€).
pub const TWENTY_M: Money = Money::from_cents(2_000_000_000);
/// 60 millones de euros (tramos de la cifra de negocios de la página 01 del 200).
pub const SIXTY_M: Money = Money::from_cents(6_000_000_000);

/// Campo de la página 01 del 200 (DR200e25) que marca el tramo de la cifra de negocios de los
/// doce meses anteriores: 102 (inferior a 20 M€), 103 (de 20 a 60 M€) o 104 (60 M€ o más).
pub fn turnover_field(turnover: Option<Money>) -> u16 {
    match turnover {
        Some(t) if t >= SIXTY_M => 104,
        Some(t) if t >= TWENTY_M => 103,
        _ => 102,
    }
}
/// Días con los que la ley prorratea los importes anuales en los periodos impositivos de menos de
/// un año («la proporción […] entre el número de días del período impositivo entre 365 días»,
/// art. 29.1 LIS; igual para el millón del art. 26.1).
const YEAR_DAYS: i64 = 365;

/// `amount` × días/365 en un periodo impositivo de menos de un año; entero en uno completo
/// (365 o 366 días).
fn prorated(amount: Money, days: i64) -> Money {
    if days <= 0 || days >= YEAR_DAYS {
        amount
    } else {
        Money::from_cents(amount.cents() * days / YEAR_DAYS)
    }
}

/// Tipos (primer tramo hasta 50.000 €, resto) de las entidades de reducida dimensión con cifra
/// de negocios < 1 M€ y tipo único del resto de reducida dimensión, por ejercicio (art. 29.1 LIS
/// y DT 44.ª, redacción de la Ley 7/2024): la rebaja es gradual de 2025 a 2029.
pub(crate) fn small_company_rates(year: i32) -> ((i32, i32), i32) {
    // Tabla con la norma en `m200_catalogo::TIPOS`.
    let (a, b, r) = crate::m200_catalogo::tipos(year);
    ((a, b), r)
}

/// Cuota íntegra y tipo nominal (para [00558]) según el régimen y el ejercicio, en un periodo
/// impositivo de un año.
pub fn gross_tax(base: Money, regime: TaxRegime, year: i32) -> (Money, Rate) {
    gross_tax_for(base, regime, year, YEAR_DAYS)
}

/// Como [`gross_tax`] en un periodo impositivo de `days` días: si dura menos de un año, el tramo
/// de 50.000 € de las microempresas se reduce a 50.000 × días/365 (art. 29.1 LIS y DT 44.ª).
pub fn gross_tax_for(base: Money, regime: TaxRegime, year: i32, days: i64) -> (Money, Rate) {
    if base.cents() <= 0 {
        return (Money::ZERO, Rate::ZERO);
    }
    let tramos = |first: i32, rest: i32| {
        let a = base.min(prorated(FIFTY_K, days));
        let b = base - a;
        a.apply_rate(Rate::percent(first)) + b.apply_rate(Rate::percent(rest))
    };
    let ((micro_a, micro_b), reduced) = small_company_rates(year);
    match regime {
        TaxRegime::General => (base.apply_rate(Rate::percent(25)), Rate::percent(25)),
        TaxRegime::ReducidaDimension => (
            base.apply_rate(Rate::percent(reduced)),
            Rate::percent(reduced),
        ),
        TaxRegime::Micro => (tramos(micro_a, micro_b), Rate::percent(micro_b)),
        // 15 % sobre toda la base (art. 29.1, segundo párrafo).
        TaxRegime::NuevaCreacion => (base.apply_rate(Rate::percent(15)), Rate::percent(15)),
    }
}

/// Liquidación de un periodo impositivo de un año; con otra duración, [`compute_for`].
pub fn compute(year: i32, i: &Input200) -> Result200 {
    compute_for(year, i, YEAR_DAYS)
}

/// Liquidación de un periodo impositivo de `days` días (un ejercicio de transición al cambiar la
/// fecha de cierre, o el primero de la sociedad): el tramo de 50.000 € y el millón de bases
/// negativas se prorratean (arts. 29.1 y 26.1 LIS).
pub fn compute_for(year: i32, i: &Input200, days: i64) -> Result200 {
    use crate::m200_catalogo::{GrupoCuota, casilla_cuota, correccion};
    let mut b = Boxes::new();
    let mut warnings: Vec<String> = Vec::new();
    // Entidad patrimonial: tipo general y sin incentivos de reducida dimensión (arts. 29.1 y
    // 101.1 LIS). Se avisa, no se cambia en silencio.
    let regime = if i.patrimonial && i.regime != TaxRegime::General {
        warnings.push(
            "Entidad patrimonial (art. 5.2 LIS): no puede aplicar el tipo reducido ni los \
             incentivos de reducida dimensión (arts. 29.1 y 101.1 LIS). Se liquida al tipo \
             general del 25 %."
                .into(),
        );
        TaxRegime::General
    } else {
        i.regime
    };
    let r501 = i.accounting_result + i.tax_expense - i.tax_income;
    let mut increases = i.other_increases + i.fines + i.depreciation_increase;
    let mut decreases = i.other_decreases + i.depreciation_decrease;
    let add = |b: &mut Boxes, k: u16, m: Money| {
        *b.entry(k).or_insert(Money::ZERO) += m;
    };
    add(&mut b, 1815, i.fines);
    add(&mut b, 303, i.depreciation_increase);
    add(&mut b, 304, i.depreciation_decrease);
    add(&mut b, 413, i.other_increases);
    add(&mut b, 414, i.other_decreases);
    for a in &i.ajustes {
        if a.importe.cents() <= 0 {
            continue;
        }
        let Some(c) = correccion(a.casilla) else {
            warnings.push(format!(
                "La casilla [{:05}] no es una corrección al resultado que TaxPain sepa llevar: no \
                 se ha sumado ({})",
                a.casilla, a.importe
            ));
            continue;
        };
        if c.solo_erd && i.patrimonial {
            warnings.push(format!(
                "[{:05}] {} ({}): es un incentivo de las empresas de reducida dimensión y una \
                 entidad patrimonial no lo puede aplicar (art. 101.1 LIS). No se ha sumado.",
                c.casilla, c.concepto, a.importe
            ));
            continue;
        }
        add(&mut b, c.casilla, a.importe);
        if c.aumento {
            increases += a.importe;
        } else {
            decreases += a.importe;
        }
    }
    let r550 = r501 + increases - decreases;
    // Reserva de capitalización: como mucho el 20 % de la base previa (25 % si la cifra de
    // negocios es < 1 M€, que es el caso del régimen `Micro`), en los periodos iniciados desde el
    // 1/1/2025 (art. 25.1 LIS, redacción de la Ley 7/2024); antes, el 10 % para todas. `year` es
    // el año en que empieza el periodo.
    let cap_pct = if year < 2025 {
        10
    } else if i.regime == TaxRegime::Micro || i.turnover_under_1m {
        25
    } else {
        20
    };
    let reserve_cap = r550.max(Money::ZERO).apply_rate(Rate::percent(cap_pct));
    let mut capitalizacion = None;
    let reserve = match &i.capitalizacion {
        None => {
            let reserve = i.capitalization_reserve.max(Money::ZERO).min(reserve_cap);
            if reserve < i.capitalization_reserve {
                warnings.push(format!(
                    "La reserva de capitalización se limita al {cap_pct} % de la base: {reserve} \
                     (lo que exceda se aplica en los dos ejercicios siguientes, art. 25.1 LIS)"
                ));
            }
            reserve
        }
        Some(c) => {
            let d = capitalizar(year, c, reserve_cap, &mut warnings);
            let applied = d.aplicado;
            capitalizacion = Some(d);
            applied
        }
    };
    let after_reserve = r550 - reserve;
    // Compensación de bases negativas: hasta 1 M€ (prorrateado en un periodo de menos de un año)
    // y, por encima, el 70 % de la base previa a la reserva de capitalización (art. 26.1 LIS).
    // Con cifra de negocios de 20 M€ o más en los 12 meses anteriores, el 70 % pasa al 50 % (de
    // 20 a 60 M€) o al 25 % (60 M€ o más) en los periodos iniciados desde el 1/1/2024 (DA 15.ª
    // LIS, Ley 7/2024); el millón sigue. En los tres primeros periodos con base positiva de una
    // entidad de nueva creación no hay límite (art. 26.3 LIS).
    let loss_pct = match i.turnover {
        Some(t) if year >= 2024 && t >= SIXTY_M => 25,
        Some(t) if year >= 2024 && t >= TWENTY_M => 50,
        _ => 70,
    };
    let limit = if r550.cents() <= 0 {
        Money::ZERO
    } else if i.bin_nueva_creacion {
        r550
    } else {
        prorated(ONE_M, days).max(r550.apply_rate(Rate::percent(loss_pct)))
    };
    let pending_total: Money = if i.bins.is_empty() {
        i.pending_losses.max(Money::ZERO)
    } else {
        i.bins.iter().map(|x| x.importe.max(Money::ZERO)).sum()
    };
    let r547 = pending_total.min(limit).min(after_reserve.max(Money::ZERO));
    let bins = aplicar_por_antiguedad(&i.bins, r547);
    let r552 = after_reserve - r547;

    // Reserva de nivelación (art. 105 LIS): las minoraciones de los cinco ejercicios anteriores
    // se adicionan si hay base negativa (hasta su importe) y, lo que quede, en el quinto año; la
    // del ejercicio, hasta el 10 % de la base positiva y 1 M€ (prorrateado), sólo si es de
    // reducida dimensión. `TODO(VERIFY)`: si la pueden aplicar las de cifra de negocios < 1 M€
    // con su tipo reducido tras la Ley 7/2024 (el art. 105.1 hablaba del tipo del art. 29.1).
    let mut nivelacion = None;
    let mut r1330 = r552;
    if let Some(n) = &i.nivelacion {
        let erd =
            !i.patrimonial && matches!(regime, TaxRegime::Micro | TaxRegime::ReducidaDimension);
        let mut pend = n.pendientes.clone();
        pend.sort_by_key(|x| x.origen);
        let mut negativa = (-r552).max(Money::ZERO);
        let mut detalle = Vec::new();
        let mut adicionado = Money::ZERO;
        for p in pend.iter().filter(|p| p.importe.cents() > 0) {
            let mut a = p.importe.min(negativa);
            negativa -= a;
            if p.origen + 5 <= year {
                a = p.importe;
            }
            adicionado += a;
            detalle.push(DetalleArrastre {
                origen: p.origen,
                inicio: p.importe,
                aplicado: a,
                pendiente: p.importe - a,
            });
        }
        let tope = if r552.cents() > 0 {
            r552.apply_rate(Rate::percent(10))
                .min(prorated(ONE_M, days))
        } else {
            Money::ZERO
        };
        let pedido = n.minorar.max(Money::ZERO);
        let minorado = if !erd && pedido.cents() > 0 {
            warnings.push(
                "La reserva de nivelación es de las entidades de reducida dimensión (art. 105 \
                 LIS) que no sean patrimoniales: no se aplica la minoración."
                    .into(),
            );
            Money::ZERO
        } else {
            if pedido > tope {
                warnings.push(format!(
                    "La reserva de nivelación se limita al 10 % de la base imponible (y 1 M€): \
                     {tope} (art. 105.1 LIS)"
                ));
            }
            pedido.min(tope)
        };
        if minorado.cents() > 0 {
            detalle.push(DetalleArrastre {
                origen: year,
                inicio: minorado,
                aplicado: Money::ZERO,
                pendiente: minorado,
            });
        }
        r1330 = r552 + adicionado - minorado;
        if !adicionado.is_zero() || !minorado.is_zero() {
            b.insert(1033, adicionado);
            b.insert(1034, minorado);
            b.insert(1330, r1330);
        }
        nivelacion = Some(DetalleNivelacion {
            minorado,
            tope,
            adicionado,
            a_dotar: minorado,
            detalle,
        });
    }
    let bin_generada = (-r1330).max(Money::ZERO);
    let (r562, rate) = gross_tax_for(r1330, regime, year, days);

    // Bonificaciones y doble imposición, hasta la cuota íntegra ([00582] no es negativa).
    let mut resto = r562;
    let mut di_pendiente = Money::ZERO;
    let mut deducciones_pendientes = Money::ZERO;
    for grupo in [GrupoCuota::Bonificacion, GrupoCuota::DobleImposicion] {
        for x in i
            .cuota
            .iter()
            .filter(|x| casilla_cuota(x.casilla).is_some_and(|c| c.grupo == grupo))
        {
            let a = x.importe.max(Money::ZERO).min(resto);
            if a < x.importe {
                if grupo == GrupoCuota::DobleImposicion {
                    di_pendiente += x.importe - a;
                } else {
                    warnings.push(format!(
                        "La bonificación [{:05}] no cabe entera en la cuota: se aplican {a}",
                        x.casilla
                    ));
                }
            }
            resto -= a;
            add(&mut b, x.casilla, a);
        }
    }
    if di_pendiente.cents() > 0 {
        warnings.push(format!(
            "Quedan {di_pendiente} de deducción por doble imposición internacional sin aplicar \
             por falta de cuota: se pueden deducir en los periodos siguientes (art. 31.6 LIS)"
        ));
    }
    let r582 = resto;
    for x in i
        .cuota
        .iter()
        .filter(|x| casilla_cuota(x.casilla).is_some_and(|c| c.grupo == GrupoCuota::Deduccion))
    {
        let a = x.importe.max(Money::ZERO).min(resto);
        deducciones_pendientes += x.importe.max(Money::ZERO) - a;
        resto -= a;
        add(&mut b, x.casilla, a);
    }
    if i.cuota.iter().any(|x| {
        casilla_cuota(x.casilla).is_some_and(|c| c.grupo == GrupoCuota::Deduccion)
            && x.importe.cents() > 0
    }) {
        warnings.push(
            "Las deducciones se han tecleado a mano: comprueba sus límites (en general, el 25 % \
             de la cuota íntegra ajustada, o el 50 % con mucha I+D+i, art. 39.1 LIS) y su \
             desglose en las páginas 16 a 19 de Sociedades WEB."
                .into(),
        );
    }
    if deducciones_pendientes.cents() > 0 {
        warnings.push(format!(
            "Quedan {deducciones_pendientes} de deducciones sin aplicar por falta de cuota \
             (se arrastran según el art. 39.1 LIS)"
        ));
    }
    for x in &i.cuota {
        if casilla_cuota(x.casilla).is_none_or(|c| c.grupo == GrupoCuota::Retencion) {
            warnings.push(format!(
                "La casilla [{:05}] no es una bonificación ni una deducción de la cuota: no se ha \
                 aplicado",
                x.casilla
            ));
        }
    }
    let r592 = resto;
    // Retenciones: el desglose, si lo hay, da el total.
    let mut withholdings = i.withholdings;
    if !i.retenciones.is_empty() {
        let mut sum = Money::ZERO;
        for x in &i.retenciones {
            if casilla_cuota(x.casilla).is_some_and(|c| c.grupo == GrupoCuota::Retencion) {
                add(&mut b, x.casilla, x.importe.max(Money::ZERO));
                sum += x.importe.max(Money::ZERO);
            }
        }
        if !i.withholdings.is_zero() && sum != i.withholdings {
            warnings.push(format!(
                "El desglose de las retenciones ({sum}) no suma lo de la 473 ({}): se usa el \
                 desglose",
                i.withholdings
            ));
        }
        withholdings = sum;
    }
    let r599 = r592 - withholdings;
    let paid: Money = i.installments.iter().copied().sum();
    let r611 = r599 - paid;

    for (k, v) in [
        (500, i.accounting_result),
        (301, i.tax_expense),
        (302, i.tax_income),
        (501, r501),
        (417, increases),
        (418, decreases),
        (550, r550),
        (1032, reserve),
        (547, r547),
        (552, r552),
        (558, Money::from_cents(i64::from(rate.bp()))),
        (562, r562),
        (582, r582),
        (592, r592),
        (1766, withholdings),
        (599, r599),
        (601, i.installments[0]),
        (603, i.installments[1]),
        (605, i.installments[2]),
        (611, r611),
        (1586, r611),
    ] {
        b.insert(k, v);
    }
    b.retain(|k, v| !v.is_zero() || matches!(k, 500 | 552 | 562 | 592 | 599 | 611 | 1586));
    if bin_generada.cents() > 0 {
        warnings.push(format!(
            "Base imponible negativa: {bin_generada} se podrán compensar en ejercicios futuros"
        ));
    }
    if pending_total > r547 && r552.cents() > 0 {
        warnings.push("Quedan bases negativas pendientes por el límite del art. 26 LIS".into());
    }
    Result200 {
        year,
        boxes: b,
        warnings,
        regimen: regime,
        patrimonial: i.patrimonial,
        bin_nueva_creacion: i.bin_nueva_creacion,
        bins,
        bin_generada,
        capitalizacion,
        nivelacion,
        di_pendiente,
        deducciones_pendientes,
    }
}

/// Reparte `aplicado` entre los pendientes, del más antiguo al más reciente.
fn aplicar_por_antiguedad(pendientes: &[Arrastre], aplicado: Money) -> Vec<DetalleArrastre> {
    let mut v: Vec<&Arrastre> = pendientes
        .iter()
        .filter(|x| x.importe.cents() > 0)
        .collect();
    v.sort_by_key(|x| x.origen);
    let mut resto = aplicado;
    v.into_iter()
        .map(|x| {
            let a = x.importe.min(resto);
            resto -= a;
            DetalleArrastre {
                origen: x.origen,
                inicio: x.importe,
                aplicado: a,
                pendiente: x.importe - a,
            }
        })
        .collect()
}

/// Reserva de capitalización del ejercicio (art. 25 LIS): derecho = incremento de fondos
/// propios × porcentaje; se aplican primero las reducciones pendientes de los dos ejercicios
/// anteriores y después la del ejercicio, hasta el tope. Lo no aplicado por falta de base vale
/// en los periodos que acaben en los dos años siguientes (art. 25.1, último párrafo, que la Ley
/// 7/2024 no cambió).
/// Porcentaje general de la reserva de capitalización del año en que empieza el periodo (art.
/// 25.1 LIS): 10 % hasta 2023; 15 % en 2024 (RDL 4/2024, art. 4.2, BOE-A-2024-12944); 20 % desde
/// 2025 (Ley 7/2024, DF 8.ª.2, BOE-A-2024-26694).
pub fn capitalizacion_pct(year: i32) -> u8 {
    match year {
        ..=2023 => 10,
        2024 => 15,
        _ => 20,
    }
}

fn capitalizar(
    year: i32,
    c: &Capitalizacion,
    tope: Money,
    warnings: &mut Vec<String>,
) -> DetalleCapitalizacion {
    let porcentaje = c.porcentaje.unwrap_or(capitalizacion_pct(year));
    let derecho = c
        .incremento
        .max(Money::ZERO)
        .apply_rate(Rate::percent(i32::from(porcentaje)));
    let mut caducado = Money::ZERO;
    let mut lista: Vec<Arrastre> = Vec::new();
    for p in &c.pendientes {
        if p.importe.cents() <= 0 {
            continue;
        }
        if p.origen < year - 2 {
            caducado += p.importe;
        } else {
            lista.push(p.clone());
        }
    }
    if caducado.cents() > 0 {
        warnings.push(format!(
            "{caducado} de reserva de capitalización de hace más de dos ejercicios ya no se pueden \
             aplicar (art. 25.1 LIS)"
        ));
    }
    if derecho.cents() > 0 {
        lista.push(Arrastre {
            origen: year,
            importe: derecho,
        });
    }
    let total: Money = lista.iter().map(|x| x.importe).sum();
    let aplicado = total.min(tope);
    let detalle = aplicar_por_antiguedad(&lista, aplicado);
    if aplicado < total {
        warnings.push(format!(
            "La reserva de capitalización se limita a {tope} (el tope de la base, art. 25.1 \
             LIS): {} se aplican en los dos ejercicios siguientes",
            total - aplicado
        ));
    }
    DetalleCapitalizacion {
        porcentaje,
        derecho,
        tope,
        aplicado,
        a_dotar: aplicado,
        detalle,
        caducado,
    }
}

/// Datos del fichero que no salen de la liquidación.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct File200 {
    pub period_start: Date,
    pub period_end: Date,
    /// CNAE-2025 de la actividad principal (4 dígitos).
    pub cnae: String,
    /// Modelo de cuentas anuales: `pymes`, `abreviado` o `normal`.
    pub model: String,
    /// Partidas de balance y PyG (`balance:<id>`, `pyg:<id>`) con su importe (PyG con signo).
    pub lines: Vec<(String, Money)>,
    /// IBAN para domiciliar el ingreso o recibir la devolución.
    pub iban: Option<String>,
    pub domiciliar: bool,
    /// Importe neto de la cifra de negocios de los 12 meses anteriores al inicio del periodo:
    /// marca en la página 01 el tramo (inferior a 20 M€, de 20 a 60 M€ o 60 M€ o más). `None`:
    /// se marca el de menos de 20 M€.
    pub turnover: Option<Money>,
    /// Participaciones en otras entidades (página 02, apartado B.1): tres por página.
    pub participadas: Vec<Participada200>,
}

/// Una participada en la página 02 del 200.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Participada200 {
    pub nif: String,
    pub nombre: String,
    /// Código de provincia (dos cifras) si es española; si no, el del país.
    /// `TODO(VERIFY)`: el DR sólo dice «Código provincia / país».
    pub codigo: String,
    /// Participación en millonésimas (100 % = 1.000.000).
    pub pct: u32,
    pub nominal: Money,
    /// Valor en libros en el activo de la declarante.
    pub valor_libros: Money,
    /// Ingresos por dividendos recibidos en el ejercicio.
    pub dividendos: Money,
    pub capital: Money,
    pub reservas: Money,
    pub otras_pn: Money,
    pub resultado: Money,
}

/// Fichero del 200 (DR 2025) para importar en Sociedades WEB. Devuelve también avisos de lo que
/// no se ha podido llevar (partidas sin casilla equivalente).
pub fn file(
    r: &Result200,
    who: &Declarant,
    i: &File200,
    regime: TaxRegime,
) -> Result<(String, Vec<String>), DrError> {
    let s = spec();
    // Otro ejercicio que 2025: el aviso del diseño lo pone el borrador
    // (`dr_versions::aviso_diseno_no_publicado`), igual para todos los modelos.
    let mut warnings = Vec::new();
    // Entidad patrimonial: sin los tipos reducidos ni los incentivos (ver `compute_for`).
    let regime = if r.patrimonial {
        TaxRegime::General
    } else {
        regime
    };
    let result = r.result();
    let declaration = if result.is_zero() {
        "N"
    } else if result.is_negative() {
        "D"
    } else if i.domiciliar && i.iban.is_some() {
        "U"
    } else {
        "I"
    };
    let ymd = |d: Date| (d.year(), u8::from(d.month()), d.day());
    let (ys, ms, ds) = ymd(i.period_start);
    let (ye, me, de) = ymd(i.period_end);

    // 01: identificación y caracteres de la declaración.
    let mut w = PageWriter::new(s.page("DP200001")?);
    w.text(6, declaration)?
        .text(7, &who.nif)?
        .text(8, &who.name)?
        .text(9, &r.year.to_string())?
        .text(11, &ys.to_string())?
        .text(12, &format!("{ms:02}"))?
        .text(13, &format!("{ds:02}"))?
        .text(14, &ye.to_string())?
        .text(15, &format!("{me:02}"))?
        .text(16, &format!("{de:02}"))?
        .text(18, &i.cnae)?;
    let flag = |w: &mut PageWriter<'_>, b: u16| -> Result<(), DrError> {
        let n = w_box(s, "DP200001", b)?;
        w.text(n, "1")?;
        Ok(())
    };
    match regime {
        TaxRegime::Micro => flag(&mut w, 88)?,
        TaxRegime::NuevaCreacion => flag(&mut w, 71)?,
        TaxRegime::ReducidaDimension | TaxRegime::General => {}
    }
    if regime != TaxRegime::General {
        flag(&mut w, 6)?; // incentivos de reducida dimensión
    }
    if r.get(552).cents() <= 0 {
        flag(&mut w, 27)?;
    }
    if r.patrimonial {
        flag(&mut w, 66)?;
    }
    if r.bin_nueva_creacion {
        flag(&mut w, 70)?;
    }
    // Campos 102, 103 y 104 de la página 01 (posiciones 241, 242 y 243 del DR200e25): cifra
    // de negocios inferior a 20 M€, de al menos 20 M€ e inferior a 60 M€, o de al menos 60 M€.
    w.text(turnover_field(i.turnover), "1")?;
    let mut pages = vec![w.finish()?];

    // 02: participaciones en otras entidades (tres por página; el resto, en Sociedades WEB).
    if !i.participadas.is_empty() {
        pages.push(pagina_participadas(s, &i.participadas)?);
        if i.participadas.len() > 3 {
            warnings.push(format!(
                "La página 02 lleva tres participadas: añade las otras {} en Sociedades WEB \
                 (página complementaria)",
                i.participadas.len() - 3
            ));
        }
    }

    // 03–08: balance y cuenta de pérdidas y ganancias.
    let boxes = account_boxes(&i.model);
    let mut by_page: BTreeMap<String, Vec<(u16, Money)>> = BTreeMap::new();
    for (key, amount) in &i.lines {
        if amount.is_zero() {
            continue;
        }
        match boxes.get(key) {
            Some((page, b)) => by_page.entry(page.clone()).or_default().push((*b, *amount)),
            None => warnings.push(format!(
                "{key} ({amount}) no tiene casilla en el 200: añádelo a mano en Sociedades WEB"
            )),
        }
    }
    // Resultado de la cuenta de pérdidas y ganancias, también en la página 08.
    by_page
        .entry("08".into())
        .or_default()
        .push((500, r.get(500)));
    for page in ["03", "04", "05", "06", "07", "08"] {
        let mut w = PageWriter::new(s.page(&format!("DP2000{page}"))?);
        for (b, m) in by_page.get(page).into_iter().flatten() {
            w.amount_box(*b, *m)?;
        }
        pages.push(w.finish()?);
    }

    // 12, 13, 14 y 14 bis: liquidación.
    let de_pagina = |p: &str| -> Vec<u16> {
        crate::m200_catalogo::CORRECCIONES
            .iter()
            .filter(|c| c.pagina == p)
            .map(|c| c.casilla)
            .collect()
    };
    let cuota = |g: crate::m200_catalogo::GrupoCuota| -> Vec<u16> {
        crate::m200_catalogo::CUOTA
            .iter()
            .filter(|c| c.grupo == g)
            .map(|c| c.casilla)
            .collect()
    };
    use crate::m200_catalogo::GrupoCuota as G;
    let p12: Vec<u16> = [500u16, 301, 302, 501]
        .into_iter()
        .chain(de_pagina("12"))
        .collect();
    let p13: Vec<u16> = de_pagina("13").into_iter().chain([417, 418]).collect();
    let p14: Vec<u16> = [550u16, 1032, 547, 552, 1033, 1034, 1330, 558, 562]
        .into_iter()
        .chain(cuota(G::Bonificacion))
        .chain(cuota(G::DobleImposicion))
        .chain([582])
        .collect();
    let p14b: Vec<u16> = cuota(G::Deduccion)
        .into_iter()
        .chain([592])
        .chain(cuota(G::Retencion))
        .chain([1766, 599, 601, 603, 605, 611, 1586])
        .collect();
    for (page, list) in [
        ("DP200012", &p12),
        ("DP200013", &p13),
        ("DP200014", &p14),
        ("DP200014B", &p14b),
    ] {
        let mut w = PageWriter::new(s.page(page)?);
        for b in list {
            let v = r.get(*b);
            if !v.is_zero() {
                w.amount_box(*b, v)?;
            }
        }
        pages.push(w.finish()?);
    }

    // 15: detalle de la compensación de bases negativas por ejercicio de origen, con el diseño
    // de 2025 (sus filas son años fijos).
    if !r.bins.is_empty() || r.bin_generada.cents() > 0 {
        if r.year == 2025 {
            pages.push(pagina_bins(s, r, &mut warnings)?);
        } else {
            warnings.push(format!(
                "El detalle de las bases negativas por año (página 15) no va en el fichero: el \
                 diseño del {} aún no está en TaxPain. Tecléalo en Sociedades WEB.",
                r.year
            ));
        }
    }
    if r.capitalizacion.is_some() || r.nivelacion.is_some() {
        warnings.push(
            "El detalle por años de las reservas de capitalización y de nivelación (página 20 \
             bis) no va en el fichero: tecléalo en Sociedades WEB con las casillas que muestra \
             TaxPain."
                .into(),
        );
    }

    // Ingreso, domiciliación o devolución.
    let mut w = PageWriter::new(s.page("DP200DID")?);
    w.text(7, &r.year.to_string())?
        .text(9, "0A")?
        .text(10, &format!("{ds:02}"))?
        .text(11, &format!("{ms:02}"))?
        .text(12, &format!("{:02}", ys % 100))?
        .text(13, &format!("{de:02}"))?
        .text(14, &format!("{me:02}"))?
        .text(15, &format!("{:02}", ye % 100))?
        .text(16, &who.nif)?
        .text(17, &who.name)?;
    w.amount_box(552, r.get(552))?
        .amount_box(562, r.get(562))?
        .amount_box(1586, result)?
        .amount_box(621, result)?;
    match declaration {
        "D" => {
            w.amount(24, -result)?;
            if let Some(iban) = &i.iban {
                w.text(26, "1")?.text(27, iban)?;
            } else {
                warnings.push("falta el IBAN para la devolución".into());
            }
        }
        "U" | "I" => {
            w.text(33, declaration)?.amount(34, result)?;
            if let (Some(iban), "U") = (&i.iban, declaration) {
                w.text(35, iban)?;
            }
        }
        _ => {
            w.text(41, "1")?;
        }
    }
    pages.push(w.finish()?);
    warnings.push(
        "TODO(VERIFY): importa el fichero en Sociedades WEB (Importar) y revisa cada página antes de presentar"
            .into(),
    );
    Ok((
        envelope("200", r.year, "0A", SOFTWARE_VERSION, DEVELOPER_NIF, &pages),
        warnings,
    ))
}

/// Página 02 con las tres primeras participadas y los totales [01501]–[01503].
fn pagina_participadas(s: &Spec, ps: &[Participada200]) -> Result<String, DrError> {
    let page = s.page("DP200002")?;
    let mut w = PageWriter::new(page);
    // Primer campo de cada entidad (NIF): 36, 54 y 72; los demás van seguidos.
    for (k, p) in ps.iter().take(3).enumerate() {
        let n0 = 36 + 18 * u16::try_from(k).unwrap_or(0);
        w.text(n0, &p.nif)?
            .text(n0 + 1, &p.nombre.chars().take(30).collect::<String>())?
            .text(n0 + 2, &p.codigo)?
            .amount(n0 + 3, Money::from_cents(i64::from(p.pct / 100)))?
            .amount(n0 + 4, p.nominal.max(Money::ZERO))?
            .amount(n0 + 5, p.valor_libros.max(Money::ZERO))?
            .amount(n0 + 6, p.dividendos.max(Money::ZERO))?
            .amount(n0 + 14, p.capital.max(Money::ZERO))?
            .amount(n0 + 15, p.reservas)?
            .amount(n0 + 16, p.otras_pn)?
            .amount(n0 + 17, p.resultado)?;
    }
    let total = |f: fn(&Participada200) -> Money| -> Money {
        ps.iter().map(|p| f(p).max(Money::ZERO)).sum()
    };
    w.amount_box(1501, total(|p| p.nominal))?
        .amount_box(1502, total(|p| p.valor_libros))?
        .amount_box(1503, total(|p| p.dividendos))?;
    w.finish()
}

/// Filas de la página 15 (detalle de bases negativas): etiqueta del ejercicio («1997»,
/// «2025(*)», «2025», «TOTAL») → casillas de pendiente al inicio, aplicado y pendiente futuro.
fn filas_bins(s: &Spec) -> Result<BTreeMap<String, [Option<u16>; 3]>, DrError> {
    let page = s.page("DP200015")?;
    let mut out: BTreeMap<String, [Option<u16>; 3]> = BTreeMap::new();
    const PRE: &str = "Detalle compensación bases imponibles negativas - ";
    for f in &page.fields {
        let (Some(b), Some(rest)) = (f.box_, f.desc.strip_prefix(PRE)) else {
            continue;
        };
        let Some((label, what)) = rest.split_once(" - ") else {
            continue;
        };
        let col = if what.starts_with("Aplicado") {
            1
        } else if what.contains("futuros") {
            2
        } else {
            0
        };
        out.entry(label.trim().to_owned()).or_default()[col] = Some(b);
    }
    Ok(out)
}

fn pagina_bins(s: &Spec, r: &Result200, warnings: &mut Vec<String>) -> Result<String, DrError> {
    let filas = filas_bins(s)?;
    let mut w = PageWriter::new(s.page("DP200015")?);
    let (mut ini, mut apl, mut fin) = (Money::ZERO, Money::ZERO, Money::ZERO);
    for d in &r.bins {
        // Una base del mismo año de inicio es de un periodo anterior más corto: fila «(*)».
        let label = if d.origen == r.year {
            format!("{}(*)", r.year)
        } else {
            d.origen.to_string()
        };
        let Some(cols) = filas.get(&label) else {
            warnings.push(format!(
                "La base negativa de {} no tiene fila en la página 15: añádela en Sociedades WEB",
                d.origen
            ));
            continue;
        };
        for (c, m) in cols.iter().zip([d.inicio, d.aplicado, d.pendiente]) {
            if let Some(b) = c
                && !m.is_zero()
            {
                w.amount_box(*b, m)?;
            }
        }
        ini += d.inicio;
        apl += d.aplicado;
        fin += d.pendiente;
    }
    // Generada en el periodo: fila del propio ejercicio (sin columna de aplicado).
    // `TODO(VERIFY)`: si el TOTAL [00670]/[00671] incluye la generada; aquí no la incluye.
    if let Some(cols) = filas.get(&r.year.to_string())
        && r.bin_generada.cents() > 0
    {
        for b in [cols[0], cols[2]].into_iter().flatten() {
            w.amount_box(b, r.bin_generada)?;
        }
    }
    if let Some(cols) = filas.get("TOTAL") {
        for (c, m) in cols.iter().zip([ini, apl, fin]) {
            if let Some(b) = c
                && !m.is_zero()
            {
                w.amount_box(*b, m)?;
            }
        }
    }
    w.finish()
}

/// Campo de la casilla `b` en la página `page`.
fn w_box(s: &Spec, page: &str, b: u16) -> Result<u16, DrError> {
    Ok(s.page(page)?.field_for_box(b)?.n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eur(e: i64) -> Money {
        Money::from_euros(e, 0)
    }

    #[test]
    fn tope_de_capitalizacion_por_la_cifra_de_negocios() {
        // Nueva creación con cifra de negocios < 1 M€: el tope es el 25 % de la base (25.000 €),
        // no el 20 %.
        let i = Input200 {
            accounting_result: eur(100_000),
            capitalization_reserve: eur(30_000),
            regime: TaxRegime::NuevaCreacion,
            turnover_under_1m: true,
            ..Input200::default()
        };
        assert_eq!(compute(2026, &i).get(1032), eur(25_000));
        let sin = Input200 {
            turnover_under_1m: false,
            ..i
        };
        assert_eq!(compute(2026, &sin).get(1032), eur(20_000));
    }

    #[test]
    fn tope_de_capitalizacion_antes_de_2025() {
        // Periodo iniciado en 2024: el tope era el 10 % de la base para todas (art. 25.1 LIS
        // antes de la Ley 7/2024), también con cifra de negocios < 1 M€.
        let i = Input200 {
            accounting_result: eur(100_000),
            capitalization_reserve: eur(15_000),
            turnover_under_1m: true,
            ..Input200::default()
        };
        let r = compute(2024, &i);
        assert_eq!(r.get(1032), eur(10_000));
        assert!(r.warnings.iter().any(|w| w.contains("10 %")));
        assert_eq!(compute(2025, &i).get(1032), eur(15_000));
    }

    #[test]
    fn bases_negativas_de_las_grandes_empresas() {
        // Base 4 M€ y 5 M€ de bases pendientes: el 70 % (2,8 M€) con menos de 20 M€ de cifra de
        // negocios; el 50 % (2 M€) de 20 a 60 M€ y el 25 % (1 M€) desde 60 M€ (DA 15.ª LIS).
        let i = Input200 {
            accounting_result: eur(4_000_000),
            pending_losses: eur(5_000_000),
            turnover: Some(eur(19_999_999)),
            ..Input200::default()
        };
        assert_eq!(compute(2026, &i).get(547), eur(2_800_000));
        let medio = Input200 {
            turnover: Some(eur(20_000_000)),
            ..i.clone()
        };
        assert_eq!(compute(2026, &medio).get(547), eur(2_000_000));
        let grande = Input200 {
            turnover: Some(eur(60_000_000)),
            ..i.clone()
        };
        assert_eq!(compute(2026, &grande).get(547), eur(1_000_000));
        // El millón sigue siendo compensable: base 2 M€ al 25 % (500.000) → 1 M€.
        let poca_base = Input200 {
            accounting_result: eur(2_000_000),
            ..grande.clone()
        };
        assert_eq!(compute(2026, &poca_base).get(547), eur(1_000_000));
        // Antes de 2024 (la DA 15.ª del RDL 3/2016 la anuló la STC 11/2024): el 70 %.
        assert_eq!(compute(2023, &grande).get(547), eur(2_800_000));
    }

    #[test]
    fn sl_pequena_con_pagos_y_retenciones() {
        // Resultado 75.000 después de un gasto por IS de 15.000 → 90.000 antes de IS;
        // multa no deducible 1.000 → base 91.000; compensa 11.000 → 80.000.
        let i = Input200 {
            accounting_result: eur(75_000),
            tax_expense: eur(15_000),
            other_increases: eur(1_000),
            pending_losses: eur(11_000),
            regime: TaxRegime::ReducidaDimension,
            withholdings: eur(400),
            installments: [eur(1_000), eur(1_000), eur(1_000)],
            ..Input200::default()
        };
        let r = compute(2026, &i);
        assert_eq!(r.get(501), eur(90_000));
        assert_eq!(r.get(550), eur(91_000));
        assert_eq!(r.get(547), eur(11_000));
        assert_eq!(r.get(552), eur(80_000));
        assert_eq!(r.get(562), eur(18_400)); // 23 %
        assert_eq!(r.get(599), eur(18_000));
        assert_eq!(r.result(), eur(15_000));
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    }

    #[test]
    fn ejercicio_corto_prorratea_el_tramo_y_el_millon() {
        // Transición del 1/1 al 30/6/2026 (181 días): el tramo del 19 % es 50.000 × 181/365 =
        // 24.794,52 € (art. 29.1 LIS): 4.710,96 + 21 % de 35.205,48 (7.393,15) = 12.104,11.
        let micro = TaxRegime::Micro;
        assert_eq!(
            gross_tax_for(eur(60_000), micro, 2026, 181).0,
            Money::from_cents(1_210_411)
        );
        assert_eq!(gross_tax(eur(60_000), micro, 2026).0, eur(9_500 + 2_100));
        // Un año completo, bisiesto o no, no se prorratea.
        assert_eq!(
            gross_tax_for(eur(60_000), micro, 2028, 366),
            gross_tax(eur(60_000), micro, 2028)
        );
        // El millón de bases negativas también (art. 26.1 LIS): 1.000.000 × 181/365 =
        // 495.890,41 €, por encima del 70 % de la base (420.000).
        let i = Input200 {
            accounting_result: eur(600_000),
            pending_losses: eur(600_000),
            ..Input200::default()
        };
        assert_eq!(compute(2026, &i).get(547), eur(600_000));
        let r = compute_for(2026, &i, 181);
        assert_eq!(r.get(547), Money::from_cents(49_589_041));
        assert_eq!(r.get(552), eur(600_000) - Money::from_cents(49_589_041));
    }

    #[test]
    fn microempresa_por_tramos_y_perdidas() {
        assert_eq!(
            gross_tax(eur(80_000), TaxRegime::Micro, 2026).0,
            eur(9_500 + 6_300)
        );
        // 2025: 21 % / 22 %; 2027: 17 % / 20 %.
        assert_eq!(
            gross_tax(eur(80_000), TaxRegime::Micro, 2025).0,
            eur(10_500 + 6_600)
        );
        assert_eq!(
            gross_tax(eur(80_000), TaxRegime::Micro, 2027).0,
            eur(8_500 + 6_000)
        );
        assert_eq!(
            gross_tax(eur(100_000), TaxRegime::ReducidaDimension, 2025).0,
            eur(24_000)
        );
        assert_eq!(
            gross_tax(eur(40_000), TaxRegime::NuevaCreacion, 2026).0,
            eur(6_000)
        );
        // Nueva creación: 15 % sobre toda la base, también por encima de 50.000 €.
        assert_eq!(
            gross_tax(eur(80_000), TaxRegime::NuevaCreacion, 2026).0,
            eur(12_000)
        );
        // Microempresa desde 2027: 17 % / 20 % (no 15 %).
        assert_eq!(
            gross_tax(eur(80_000), TaxRegime::Micro, 2028).0,
            eur(8_500 + 6_000)
        );
        // El 23 % de la microempresa empieza en 2023 (Ley 31/2022): en 2022 era el general.
        assert_eq!(
            gross_tax(eur(80_000), TaxRegime::Micro, 2023).0,
            eur(18_400)
        );
        assert_eq!(
            gross_tax(eur(80_000), TaxRegime::Micro, 2022).0,
            eur(20_000)
        );
        let r = compute(
            2026,
            &Input200 {
                accounting_result: -eur(5_000),
                ..Input200::default()
            },
        );
        assert_eq!(r.get(562), Money::ZERO);
        assert_eq!(r.warnings.len(), 1);
    }

    #[test]
    fn ejemplo_del_informe_y_limites() {
        // docs/research/MODELO_200.md: resultado 50.000 tras un IS contabilizado de 10.000,
        // multa 1.000, bases negativas 8.000, retenciones 500, pagos 3.000, micro 2025.
        let r = compute(
            2025,
            &Input200 {
                accounting_result: eur(50_000),
                tax_expense: eur(10_000),
                fines: eur(1_000),
                pending_losses: eur(8_000),
                regime: TaxRegime::Micro,
                withholdings: eur(500),
                installments: [eur(1_000), eur(1_000), eur(1_000)],
                ..Input200::default()
            },
        );
        assert_eq!(r.get(1815), eur(1_000));
        assert_eq!(r.get(417), eur(1_000));
        assert_eq!(r.get(550), eur(61_000));
        assert_eq!(r.get(552), eur(53_000));
        assert_eq!(r.get(562), eur(11_160));
        assert_eq!(r.result(), eur(7_660));

        // Reserva de capitalización limitada al 20 % de la base (general): 100.000 → 20.000.
        let r = compute(
            2026,
            &Input200 {
                accounting_result: eur(100_000),
                capitalization_reserve: eur(30_000),
                ..Input200::default()
            },
        );
        assert_eq!(r.get(1032), eur(20_000));
        assert!(r.warnings.iter().any(|w| w.contains("20 %")));

        // Bases negativas: el 70 % se calcula antes de la reserva (2 M€ → 1,4 M€, no 1,12 M€).
        let r = compute(
            2026,
            &Input200 {
                accounting_result: eur(2_000_000),
                capitalization_reserve: eur(400_000),
                pending_losses: eur(3_000_000),
                ..Input200::default()
            },
        );
        assert_eq!(r.get(547), eur(1_400_000));
        assert_eq!(r.get(552), eur(200_000));
    }

    #[test]
    fn fichero_para_sociedades_web() {
        use crate::dr::PageReader;
        use time::macros::date;
        let i = Input200 {
            accounting_result: eur(50_000),
            tax_expense: eur(10_000),
            fines: eur(1_000),
            pending_losses: eur(8_000),
            regime: TaxRegime::Micro,
            withholdings: eur(500),
            installments: [eur(1_000), eur(1_000), eur(1_000)],
            ..Input200::default()
        };
        let r = compute(2025, &i);
        let who = Declarant {
            nif: "B12345674".into(),
            name: "Ejemplo Diseño SL".into(),
            given_name: None,
        };
        let f = File200 {
            period_start: date!(2025 - 01 - 01),
            period_end: date!(2025 - 12 - 31),
            cnae: "7410".into(),
            model: "pymes".into(),
            lines: vec![
                ("balance:TA".into(), eur(120_000)),
                ("balance:B.VI".into(), eur(80_000)),
                ("balance:PA-1.VII".into(), eur(50_000)),
                ("pyg:1".into(), eur(200_000)),
                ("pyg:7".into(), -eur(140_000)),
                ("pyg:D".into(), eur(50_000)),
                ("balance:XX".into(), eur(1)),
            ],
            iban: Some("ES9121000418450200051332".into()),
            domiciliar: true,
            turnover: Some(eur(150_000)),
            participadas: vec![],
        };
        let (x, warnings) = file(&r, &who, &f, TaxRegime::Micro).unwrap_or_else(|e| panic!("{e}"));
        assert!(x.starts_with("<T200020250A0000><AUX>"));
        assert!(x.ends_with("</T200020250A0000>"));
        assert!(warnings.iter().any(|w| w.contains("balance:XX")));
        let s = spec();
        // Cada página, con su longitud exacta y en orden.
        let mut rest = &x[x.find("</AUX>").unwrap_or(0) + 6..];
        let mut get = |sheet: &str| {
            let p = s.page(sheet).unwrap_or_else(|e| panic!("{e}"));
            // Por caracteres (el fichero va en ISO-8859-1: uno por posición).
            let cut = rest
                .char_indices()
                .nth(p.len())
                .map_or(rest.len(), |(i, _)| i);
            let (rec, tail) = rest.split_at(cut);
            rest = tail;
            (p, rec.to_owned())
        };
        let (p1, r1) = get("DP200001");
        assert_eq!(&r1[..12], "<T20001000> ");
        assert_eq!(&r1[12..13], "U");
        let rd = PageReader::new(p1, &r1).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(rd.raw(7).unwrap_or_default(), "B12345674");
        assert_eq!(rd.raw(18).unwrap_or_default(), "7410");
        let micro = p1.field_for_box(88).map(|f| f.n).unwrap_or(0);
        assert_eq!(rd.raw(micro).unwrap_or_default(), "1");
        let (p3, r3) = get("DP200003");
        assert!(PageReader::new(p3, &r3).is_ok());
        let (p4, r4) = get("DP200004");
        let rd = PageReader::new(p4, &r4).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(rd.amount_box(180).unwrap_or_default(), Some(eur(120_000)));
        assert_eq!(rd.amount_box(177).unwrap_or_default(), Some(eur(80_000)));
        for sheet in ["DP200005", "DP200006"] {
            get(sheet);
        }
        let (p7, r7) = get("DP200007");
        let rd = PageReader::new(p7, &r7).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(rd.amount_box(255).unwrap_or_default(), Some(eur(200_000)));
        assert_eq!(rd.amount_box(279).unwrap_or_default(), Some(-eur(140_000)));
        let (p8, r8) = get("DP200008");
        let rd = PageReader::new(p8, &r8).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(rd.amount_box(327).unwrap_or_default(), Some(eur(50_000)));
        assert_eq!(rd.amount_box(500).unwrap_or_default(), Some(eur(50_000)));
        let (p12, r12) = get("DP200012");
        let rd = PageReader::new(p12, &r12).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(rd.amount_box(1815).unwrap_or_default(), Some(eur(1_000)));
        get("DP200013");
        let (p14, r14) = get("DP200014");
        let rd = PageReader::new(p14, &r14).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(rd.amount_box(552).unwrap_or_default(), Some(eur(53_000)));
        assert_eq!(rd.amount_box(562).unwrap_or_default(), Some(eur(11_160)));
        let (p14b, r14b) = get("DP200014B");
        let rd = PageReader::new(p14b, &r14b).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(rd.amount_box(1586).unwrap_or_default(), Some(eur(7_660)));
        let (pd, rdid) = get("DP200DID");
        let rd = PageReader::new(pd, &rdid).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(rd.raw(33).unwrap_or_default(), "U");
        assert_eq!(rd.amount(34).unwrap_or_default(), Some(eur(7_660)));
        assert_eq!(
            rd.raw(35).unwrap_or_default().trim(),
            "ES9121000418450200051332"
        );
        assert_eq!(rest, "</T200020250A0000>");
    }

    /// Tramo de la cifra de negocios en la página 01: un solo campo marcado de los tres
    /// (102 < 20 M€; 103 de 20 a 60 M€; 104 desde 60 M€), en sus posiciones 241 a 243.
    #[test]
    fn tramo_de_la_cifra_de_negocios_en_la_pagina_01() {
        use crate::dr::PageReader;
        use time::macros::date;
        assert_eq!(turnover_field(None), 102);
        assert_eq!(turnover_field(Some(TWENTY_M - Money::from_cents(1))), 102);
        assert_eq!(turnover_field(Some(TWENTY_M)), 103);
        assert_eq!(turnover_field(Some(SIXTY_M - Money::from_cents(1))), 103);
        assert_eq!(turnover_field(Some(SIXTY_M)), 104);
        let r = compute(2025, &Input200::default());
        let who = Declarant {
            nif: "B12345674".into(),
            name: "Ejemplo Diseño SL".into(),
            given_name: None,
        };
        let s = spec();
        let p1 = s.page("DP200001").unwrap_or_else(|e| panic!("{e}"));
        for (n, pos) in [(102u16, 241usize), (103, 242), (104, 243)] {
            let f = p1.fields.iter().find(|f| f.n == n).map(|f| f.pos);
            assert_eq!(f, Some(pos), "campo {n}");
        }
        for (cifra, marcado) in [
            (eur(1_000_000), 102u16),
            (eur(30_000_000), 103),
            (eur(75_000_000), 104),
        ] {
            let f = File200 {
                period_start: date!(2025 - 01 - 01),
                period_end: date!(2025 - 12 - 31),
                cnae: "7410".into(),
                model: "pymes".into(),
                lines: vec![],
                iban: None,
                domiciliar: false,
                turnover: Some(cifra),
                participadas: vec![],
            };
            let (x, _) = file(&r, &who, &f, TaxRegime::General).unwrap_or_else(|e| panic!("{e}"));
            let rest = &x[x.find("</AUX>").unwrap_or(0) + 6..];
            let cut = rest
                .char_indices()
                .nth(p1.len())
                .map_or(rest.len(), |(i, _)| i);
            let rd = PageReader::new(p1, &rest[..cut]).unwrap_or_else(|e| panic!("{e}"));
            for n in [102u16, 103, 104] {
                let v = rd.raw(n).unwrap_or_default();
                assert_eq!(v == "1", n == marcado, "cifra {cifra}: campo {n} = «{v}»");
            }
        }
    }

    fn imp(casilla: u16, e: i64) -> Importe {
        Importe {
            casilla,
            importe: eur(e),
        }
    }

    fn arr(origen: i32, e: i64) -> Arrastre {
        Arrastre {
            origen,
            importe: eur(e),
        }
    }

    /// Caso completo inventado (Estudio Demo SL, 2025, reducida dimensión al 24 %) que cuadra
    /// de principio a fin: ajustes, capitalización, bases negativas por año, nivelación, doble
    /// imposición, deducciones, retenciones desglosadas y pagos fraccionados.
    fn caso_completo() -> Input200 {
        Input200 {
            accounting_result: eur(200_000),
            tax_expense: eur(50_000),
            regime: TaxRegime::ReducidaDimension,
            ajustes: vec![
                imp(1815, 2_000), // multa de tráfico
                imp(339, 1_000),  // atenciones a clientes por encima del 1 %
                imp(331, 5_000),  // deterioro de un inmovilizado
                imp(370, 38_000), // 95 % de 40.000 de dividendos de Participada Demo SL
                imp(9999, 1),     // casilla que no existe: se avisa y no suma
            ],
            capitalizacion: Some(Capitalizacion {
                incremento: eur(100_000),
                // A mano, para que el caso no dependa del porcentaje por defecto del año.
                porcentaje: Some(15),
                pendientes: vec![arr(2024, 3_000)],
            }),
            bins: vec![arr(2022, 20_000), arr(2019, 30_000)],
            nivelacion: Some(Nivelacion {
                minorar: eur(20_000),
                pendientes: vec![arr(2020, 4_000)],
            }),
            cuota: vec![imp(573, 1_200), imp(588, 500)],
            retenciones: vec![imp(1785, 7_600), imp(1793, 1_500)],
            installments: [eur(2_000), eur(2_000), eur(2_000)],
            ..Input200::default()
        }
    }

    #[test]
    fn caso_completo_cuadra_de_principio_a_fin() {
        let r = compute(2025, &caso_completo());
        assert_eq!(r.get(501), eur(250_000));
        assert_eq!(r.get(417), eur(8_000));
        assert_eq!(r.get(418), eur(38_000));
        assert_eq!(r.get(370), eur(38_000));
        assert_eq!(r.get(550), eur(220_000));
        // Capitalización: 15 % de 100.000 + 3.000 pendientes de 2024, por debajo del 20 % de
        // la base (44.000).
        let c = r.capitalizacion.clone().unwrap_or_default();
        assert_eq!(
            (c.porcentaje, c.derecho, c.tope),
            (15, eur(15_000), eur(44_000))
        );
        assert_eq!(r.get(1032), eur(18_000));
        assert_eq!(c.a_dotar, eur(18_000));
        // Bases negativas: 50.000 (por debajo del millón), primero la de 2019.
        assert_eq!(r.get(547), eur(50_000));
        assert_eq!(r.bins[0].origen, 2019);
        assert_eq!(r.bins[0].aplicado, eur(30_000));
        assert_eq!(r.bins[1].pendiente, Money::ZERO);
        assert_eq!(r.get(552), eur(152_000));
        // Nivelación: la de 2020 se adiciona en su quinto año; la del ejercicio, hasta el 10 %.
        assert_eq!(r.get(1033), eur(4_000));
        assert_eq!(r.get(1034), eur(15_200));
        assert_eq!(r.get(1330), eur(140_800));
        assert_eq!(r.get(562), Money::from_cents(3_379_200)); // 24 %
        assert_eq!(r.get(573), eur(1_200));
        assert_eq!(r.get(582), Money::from_cents(3_259_200));
        assert_eq!(r.get(588), eur(500));
        assert_eq!(r.get(592), Money::from_cents(3_209_200));
        assert_eq!(r.get(1766), eur(9_100));
        assert_eq!(r.get(599), Money::from_cents(2_299_200));
        assert_eq!(r.result(), Money::from_cents(1_699_200));
        assert!(r.warnings.iter().any(|w| w.contains("[09999]")));
        assert!(r.warnings.iter().any(|w| w.contains("10 % de la base")));
        assert_eq!(r.bin_generada, Money::ZERO);
    }

    #[test]
    fn caso_completo_en_el_fichero() {
        use crate::dr::PageReader;
        use time::macros::date;
        let r = compute(2025, &caso_completo());
        let who = Declarant {
            nif: "B12345674".into(),
            name: "Estudio Demo SL".into(),
            given_name: None,
        };
        let f = File200 {
            period_start: date!(2025 - 01 - 01),
            period_end: date!(2025 - 12 - 31),
            cnae: "7410".into(),
            model: "pymes".into(),
            lines: vec![],
            iban: None,
            domiciliar: false,
            turnover: Some(eur(2_000_000)),
            participadas: vec![Participada200 {
                nif: "B00000000".into(),
                nombre: "Participada Demo SL".into(),
                codigo: "46".into(),
                pct: 500_000,
                nominal: eur(1_500),
                valor_libros: eur(150_000),
                dividendos: eur(40_000),
                capital: eur(3_000),
                reservas: eur(200_000),
                otras_pn: Money::ZERO,
                resultado: -eur(5_000),
            }],
        };
        let (x, warnings) = file(&r, &who, &f, r.regimen).unwrap_or_else(|e| panic!("{e}"));
        assert!(warnings.iter().any(|w| w.contains("20 bis")));
        let s = spec();
        let mut rest = &x[x.find("</AUX>").unwrap_or(0) + 6..];
        let mut get = |sheet: &str| {
            let p = s.page(sheet).unwrap_or_else(|e| panic!("{e}"));
            let cut = rest
                .char_indices()
                .nth(p.len())
                .map_or(rest.len(), |(i, _)| i);
            let (rec, tail) = rest.split_at(cut);
            rest = tail;
            PageReader::new(p, rec).unwrap_or_else(|e| panic!("{sheet}: {e}"))
        };
        get("DP200001");
        let p2 = get("DP200002");
        assert_eq!(p2.raw(36).unwrap_or_default().trim(), "B00000000");
        assert_eq!(p2.raw(39).unwrap_or_default(), "05000");
        assert_eq!(p2.amount(42).unwrap_or_default(), Some(eur(40_000)));
        assert_eq!(p2.amount(53).unwrap_or_default(), Some(-eur(5_000)));
        assert_eq!(p2.amount_box(1503).unwrap_or_default(), Some(eur(40_000)));
        for sheet in [
            "DP200003", "DP200004", "DP200005", "DP200006", "DP200007", "DP200008",
        ] {
            get(sheet);
        }
        let p12 = get("DP200012");
        assert_eq!(p12.amount_box(1815).unwrap_or_default(), Some(eur(2_000)));
        assert_eq!(p12.amount_box(339).unwrap_or_default(), Some(eur(1_000)));
        assert_eq!(p12.amount_box(331).unwrap_or_default(), Some(eur(5_000)));
        let p13 = get("DP200013");
        assert_eq!(p13.amount_box(370).unwrap_or_default(), Some(eur(38_000)));
        assert_eq!(p13.amount_box(417).unwrap_or_default(), Some(eur(8_000)));
        let p14 = get("DP200014");
        assert_eq!(p14.amount_box(1330).unwrap_or_default(), Some(eur(140_800)));
        assert_eq!(p14.amount_box(573).unwrap_or_default(), Some(eur(1_200)));
        let p14b = get("DP200014B");
        assert_eq!(p14b.amount_box(1785).unwrap_or_default(), Some(eur(7_600)));
        assert_eq!(p14b.amount_box(588).unwrap_or_default(), Some(eur(500)));
        assert_eq!(
            p14b.amount_box(1586).unwrap_or_default(),
            Some(Money::from_cents(1_699_200))
        );
        let p15 = get("DP200015");
        assert_eq!(p15.amount_box(2193).unwrap_or_default(), Some(eur(30_000)));
        assert_eq!(p15.amount_box(2194).unwrap_or_default(), Some(eur(30_000)));
        assert_eq!(p15.amount_box(897).unwrap_or_default(), Some(eur(20_000)));
        assert_eq!(p15.amount_box(670).unwrap_or_default(), Some(eur(50_000)));
        assert_eq!(p15.amount_box(547).unwrap_or_default(), Some(eur(50_000)));
        get("DP200DID");
        assert_eq!(rest, "</T200020250A0000>");
    }

    #[test]
    fn entidad_patrimonial_sin_tipo_reducido_ni_incentivos() {
        use crate::dr::PageReader;
        use time::macros::date;
        let i = Input200 {
            accounting_result: eur(100_000),
            regime: TaxRegime::Micro,
            patrimonial: true,
            ajustes: vec![imp(312, 10_000)],
            nivelacion: Some(Nivelacion {
                minorar: eur(5_000),
                pendientes: vec![],
            }),
            ..Input200::default()
        };
        let r = compute(2026, &i);
        assert_eq!(r.regimen, TaxRegime::General);
        assert_eq!(r.get(550), eur(100_000)); // la libertad de amortización no se aplica
        assert_eq!(r.get(1034), Money::ZERO);
        assert_eq!(r.get(562), eur(25_000));
        assert!(r.warnings.iter().any(|w| w.contains("art. 101.1")));
        assert!(r.warnings.iter().any(|w| w.contains("tipo general")));
        // Página 01: clave [00066] y sin la de reducida dimensión ni la de microempresa.
        let f = File200 {
            period_start: date!(2026 - 01 - 01),
            period_end: date!(2026 - 12 - 31),
            cnae: "6820".into(),
            model: "pymes".into(),
            lines: vec![],
            iban: None,
            domiciliar: false,
            turnover: None,
            participadas: vec![],
        };
        let who = Declarant {
            nif: "B12345674".into(),
            name: "Estudio Demo SL".into(),
            given_name: None,
        };
        let (x, _) = file(&r, &who, &f, TaxRegime::Micro).unwrap_or_else(|e| panic!("{e}"));
        let p1 = spec().page("DP200001").unwrap_or_else(|e| panic!("{e}"));
        let rest = &x[x.find("</AUX>").unwrap_or(0) + 6..];
        let cut = rest
            .char_indices()
            .nth(p1.len())
            .map_or(rest.len(), |(i, _)| i);
        let rd = PageReader::new(p1, &rest[..cut]).unwrap_or_else(|e| panic!("{e}"));
        let campo = |b: u16| p1.field_for_box(b).map(|f| f.n).unwrap_or(0);
        assert_eq!(rd.raw(campo(66)).unwrap_or_default(), "1");
        assert_ne!(rd.raw(campo(6)).unwrap_or_default(), "1");
        assert_ne!(rd.raw(campo(88)).unwrap_or_default(), "1");
    }

    #[test]
    fn nivelacion_se_adiciona_con_base_negativa() {
        let i = Input200 {
            accounting_result: -eur(10_000),
            regime: TaxRegime::ReducidaDimension,
            nivelacion: Some(Nivelacion {
                minorar: eur(1_000),
                pendientes: vec![arr(2023, 6_000), arr(2024, 8_000)],
            }),
            ..Input200::default()
        };
        let r = compute(2026, &i);
        // 6.000 de 2023 y 4.000 de 2024 hasta cubrir los 10.000 negativos; sin minoración.
        assert_eq!(r.get(1033), eur(10_000));
        assert_eq!(r.get(1034), Money::ZERO);
        assert_eq!(r.get(1330), Money::ZERO);
        let n = r.nivelacion.unwrap_or_default();
        assert_eq!(n.detalle[1].pendiente, eur(4_000));
        assert_eq!(r.bin_generada, Money::ZERO);
    }

    #[test]
    fn bases_negativas_de_nueva_creacion_sin_limite() {
        let i = Input200 {
            accounting_result: eur(3_000_000),
            bins: vec![arr(2023, 4_000_000)],
            ..Input200::default()
        };
        assert_eq!(compute(2026, &i).get(547), eur(2_100_000));
        let nueva = Input200 {
            bin_nueva_creacion: true,
            ..i
        };
        let r = compute(2026, &nueva);
        assert_eq!(r.get(547), eur(3_000_000));
        assert_eq!(r.bins[0].pendiente, eur(1_000_000));
    }

    #[test]
    fn doble_imposicion_y_deducciones_hasta_la_cuota() {
        let i = Input200 {
            accounting_result: eur(10_000),
            cuota: vec![imp(567, 1_000), imp(573, 2_000), imp(583, 300)],
            ..Input200::default()
        };
        let r = compute(2026, &i);
        // Cuota 2.500: bonificación 1.000, doble imposición 1.500 (500 pendientes), sin cuota
        // para la deducción.
        assert_eq!(r.get(567), eur(1_000));
        assert_eq!(r.get(573), eur(1_500));
        assert_eq!(r.di_pendiente, eur(500));
        assert_eq!(r.get(582), Money::ZERO);
        assert_eq!(r.deducciones_pendientes, eur(300));
        assert!(r.warnings.iter().any(|w| w.contains("31.6")));
    }

    #[test]
    fn porcentaje_de_la_reserva_de_capitalizacion_por_ejercicio() {
        assert_eq!(
            [2023, 2024, 2025, 2026].map(capitalizacion_pct),
            [10, 15, 20, 20]
        );
        let i = Input200 {
            accounting_result: eur(500_000),
            capitalizacion: Some(Capitalizacion {
                incremento: eur(100_000),
                porcentaje: None,
                pendientes: vec![],
            }),
            ..Input200::default()
        };
        for (year, derecho) in [(2023, 10_000), (2024, 15_000), (2025, 20_000)] {
            let c = compute(year, &i).capitalizacion.unwrap_or_default();
            assert_eq!(c.derecho, eur(derecho), "{year}");
        }
    }

    #[test]
    fn capitalizacion_caducada_y_limitada() {
        let i = Input200 {
            accounting_result: eur(50_000),
            capitalizacion: Some(Capitalizacion {
                incremento: eur(200_000),
                porcentaje: Some(15),
                pendientes: vec![arr(2023, 2_000), arr(2025, 1_000)],
            }),
            ..Input200::default()
        };
        let r = compute(2026, &i);
        let c = r.capitalizacion.clone().unwrap_or_default();
        // Tope: 20 % de 50.000; la de 2023 caduca; 1.000 de 2025 + 30.000 del año → 10.000.
        assert_eq!(c.caducado, eur(2_000));
        assert_eq!(r.get(1032), eur(10_000));
        assert_eq!(c.detalle[0].aplicado, eur(1_000));
        assert_eq!(c.detalle[1].pendiente, eur(21_000));
    }
}
